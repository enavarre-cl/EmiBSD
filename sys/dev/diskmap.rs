/* <LICENSES> */
/*
 * Copyright (c) 2009, 2010 Joel Sing <jsing@openbsd.org>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! diskmap(4): the disk mapper, `/dev/diskmap`. opendev(3) opens it and hands it a disk's
//! name or its disklabel UID (`DIOCMAP`); the device swaps the caller's descriptor for one
//! open on the disk's real device node.
//!
//! Upstream: sys/dev/diskmap.c @ 3ce1f3f79392
//!
//! `cdevsw[90]` of amd64 and arm64 (`cdev_disk_init(1,diskmap)`). The DUID lookup is
//! `disk_map` (`subr_disk.rs`), which `mount(2)` uses too.
//!
//! ## Deviations
//! - The device name buffer is a `PATH_MAX` `Vec` (the C's `malloc(M_DEVBUF)`), and
//!   `disk_map` writes the mapped name into a second one, where the C maps in place.

use alloc::vec;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::kassert;
use crate::kern::kern_descrip::{closef, fdinsert, fnew};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::subr_disk::disk_map;
use crate::kern::vfs_lookup::ndinit;
use crate::kern::vfs_syscalls::getvnode;
use crate::kern::vfs_vnops::{VNOPS, vn_close, vn_open};
use crate::kern::vfs_vops::VOP_UNLOCK;
use crate::machine::copy::{copyinstr, copyoutstr};
use crate::sys::dkio::{DIOCMAP, DkDiskmap};
use crate::sys::errno::Errno;
use crate::sys::file::{DTYPE_VNODE, frele};
use crate::sys::filedesc::{fdplock, fdpunlock};
use crate::sys::namei::{NiDirp, UNVEIL_READ};
use crate::sys::pledge::PLEDGE_RPATH;
use crate::sys::proc::Proc;
use crate::sys::syslimits::PATH_MAX;
use crate::sys::types::Dev;
use crate::sys::uio::Uio;

/// `*(struct dk_diskmap *)addr`, read from the ioctl's argument buffer.
fn dk_diskmap_load(addr: &[u8]) -> DkDiskmap {
    let mut b = [0u8; size_of::<DkDiskmap>()];
    let n = addr.len().min(b.len());
    b[..n].copy_from_slice(&addr[..n]);
    let word = |at: usize| {
        let mut w = [0u8; size_of::<usize>()];
        w.copy_from_slice(&b[at..at + size_of::<usize>()]);
        usize::from_ne_bytes(w)
    };
    let int = |at: usize| i32::from_ne_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
    DkDiskmap {
        device: word(core::mem::offset_of!(DkDiskmap, device)) as *mut u8,
        fd: int(core::mem::offset_of!(DkDiskmap, fd)),
        flags: int(core::mem::offset_of!(DkDiskmap, flags)),
    }
}

/// `diskmapopen`: nothing to do.
pub fn diskmapopen(_dev: Dev, _flag: i32, _fmt: i32, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

/// `diskmapclose`: nothing to do.
pub fn diskmapclose(_dev: Dev, _flag: i32, _fmt: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    Ok(())
}

/// `diskmapioctl`: `DIOCMAP` maps the disk name or disklabel UID at `dm->device` to its
/// device node (writing the mapped name back), opens that node with the flags of the file
/// at `dm->fd` and puts the new file in that descriptor's place.
pub fn diskmapioctl(
    _dev: Dev,
    cmd: u64,
    addr: &mut [u8],
    _flag: i32,
    p: &Proc,
) -> Result<(), Errno> {
    if cmd != DIOCMAP {
        return Err(Errno::ENOTTY);
    }

    // Map a request for a disk to the correct device. We should be supplied with either a
    // diskname or a disklabel UID.
    let dm = dk_diskmap_load(addr);
    let fd = dm.fd;
    let mut devname = vec![0u8; PATH_MAX];
    copyinstr(dm.device as usize, &mut devname)?;
    let mut mapped = vec![0u8; PATH_MAX];
    if disk_map(&devname, &mut mapped, dm.flags) {
        devname = mapped;
        let len = devname.iter().position(|&c| c == 0).unwrap_or(PATH_MAX - 1);
        copyoutstr(&devname[..=len], dm.device as usize)?;
    }

    // Attempt to open actual device.
    let fp0 = getvnode(p, fd)?;
    let fflag = fp0.f_flag.load(Ordering::SeqCst);

    let mut nd = ndinit(0, 0, NiDirp::Sys(&devname), p);
    nd.ni_pledge = PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_READ;
    if let Err(error) = vn_open(&mut nd, fflag as i32, 0) {
        let _ = frele(fp0, p);
        return Err(error);
    }
    let Some(vp) = nd.ni_vp else {
        let _ = frele(fp0, p);
        return Err(Errno::ENOENT);
    };
    let _ = VOP_UNLOCK(vp);

    let fdp = p.fd();
    fdplock(fdp);
    let bad = |error: Errno| -> Result<(), Errno> {
        fdpunlock(fdp);
        let _ = vn_close(vp, fflag as i32, p.p_ucred.get(), Some(p));
        let _ = frele(fp0, p);
        Err(error)
    };

    // Stop here if the 'struct file *' has been replaced, for example by another thread
    // calling dup2(2), while this thread was sleeping in vn_open(). Note that this would
    // not happen for correct usages of "/dev/diskmap".
    if !fdp.ofile(fd as usize).is_some_and(|f| ptr::eq(f, fp0)) {
        return bad(Errno::EAGAIN);
    }

    let Some(fp) = fnew(p) else {
        return bad(Errno::ENFILE);
    };

    // Zap old file.
    mtx_enter(&fdp.fd_fplock);
    kassert!(fdp.ofile(fd as usize).is_some_and(|f| ptr::eq(f, fp0)));
    let flags = fdp.ofileflags(fd as usize);
    fdp.set_ofile(fd as usize, None);
    fdp.set_ofileflags(fd as usize, 0);
    mtx_leave(&fdp.fd_fplock);

    // Insert new file.
    fp.f_flag.store(fflag, Ordering::SeqCst);
    fp.f_type.set(DTYPE_VNODE);
    fp.f_ops.set(Some(&VNOPS));
    fp.f_data.set(ptr::from_ref(vp).cast_mut().cast());
    fdinsert(fdp, fd, flags, fp);
    fdpunlock(fdp);

    let _ = closef(fp0, p);

    Ok(())
}

/// `diskmapread`: the mapper has no data.
pub fn diskmapread(_dev: Dev, _uio: &mut Uio<'_>, _flag: i32) -> Result<(), Errno> {
    Err(Errno::ENXIO)
}

/// `diskmapwrite`: the mapper takes no data.
pub fn diskmapwrite(_dev: Dev, _uio: &mut Uio<'_>, _flag: i32) -> Result<(), Errno> {
    Err(Errno::ENXIO)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_argument_is_read_at_the_c_offsets() {
        let mut b = [0u8; size_of::<DkDiskmap>()];
        b[..8].copy_from_slice(&0x1234_5678usize.to_ne_bytes()[..8]);
        b[8..12].copy_from_slice(&7i32.to_ne_bytes());
        b[12..16].copy_from_slice(&3i32.to_ne_bytes());
        let dm = dk_diskmap_load(&b);
        assert_eq!(dm.device as usize, 0x1234_5678);
        assert_eq!((dm.fd, dm.flags), (7, 3));
    }

    #[test]
    fn the_argument_has_the_c_size() {
        assert_eq!(size_of::<DkDiskmap>(), 16);
    }
}
/* </TESTS> */
