/* <LICENSES> */
/*-
 * Copyright (c) 1994
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley
 * by Pace Willisson (pace@blitz.com).  The Rock Ridge Extension
 * Support code is derived from software contributed to Berkeley
 * by Atsushi Murai (amurai@spec.co.jp).
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)cd9660_vnops.c	8.15 (Berkeley) 12/5/94
 */
/* </LICENSES> */

/* <CODE> */
//! The ISO 9660 file system's vnode operations: the tables for files (`cd9660_vops`) and
//! for the special files on a disc (`cd9660_specvops`), attributes, reading a file through
//! the buffer cache, reading a directory (`cd9660_readdir`, which merges associated files
//! for the default format), symbolic links, locking, the strategy routine, `pathconf` and
//! the kqueue filters.
//!
//! Upstream: sys/isofs/cd9660/cd9660_vnops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct isoreaddir` and `cd9660_read`'s read-ahead arrays are on the stack instead of
//!   `malloc(M_TEMP)`.
//! - `iso_uiodir` and `iso_shipdir` return `Err(None)` for the C's -1 (the user's buffer is
//!   full), which `cd9660_readdir` turns into success as the C does; `iso_uiodir` takes the
//!   entry by name (`Ent`) instead of a pointer into the structure. The `bcopy` of
//!   `d_reclen` bytes of the current entry is a copy of the whole entry (the bytes past
//!   `d_reclen` are never copied out).
//! - `cd9660_getattr` reads a zero-length symbolic link's target through `cd9660_readlink`
//!   into a `malloc(M_TEMP)` buffer, as in C, with a kernel `Uio` of one `Iovec`.
//! - `cd9660_readlink`'s direct copy into a kernel buffer of at least `MAXPATHLEN` bytes
//!   writes through the iovec's base pointer, as in C; `uio_offset` is not advanced on that
//!   path, as in C. A record too short for its fixed part is `EINVAL`.
//! - The helpers the C installs in many slots (`eopnotsupp`, `nullop`,
//!   `vop_generic_badop`) are closures, as in `spec_vops` (`docs/C_TO_RUST.md`);
//!   `cd9660_mmap` and `cd9660_seek`, in no table in C either, take no arguments.
//! - `option FIFO` is in GENERIC but `miscfs/fifofs` is not ported: `cd9660_fifovops` comes
//!   with it (`cd9660_vfsops.rs`).
//! - `cd9660_print` prints under `DIAGNOSTIC` or `DEBUG` (features); `VFSLCKDEBUG` is not
//!   configured.

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::isofs::cd9660::cd9660_bmap::cd9660_bmap;
use crate::isofs::cd9660::cd9660_extern::{
    ISO_FTYPE_9660, ISO_FTYPE_DEFAULT, ISO_FTYPE_RRIP, blkoff, blksize, lblkno, lblktosize,
};
use crate::isofs::cd9660::cd9660_lookup::{cd9660_bufatoff, cd9660_lookup};
use crate::isofs::cd9660::cd9660_node::{
    IN_ACCESS, cd9660_inactive, cd9660_reclaim, isodirino, vtoi,
};
use crate::isofs::cd9660::cd9660_rrip::{cd9660_rrip_getname, cd9660_rrip_getsymname};
use crate::isofs::cd9660::cd9660_util::isofntrans;
use crate::isofs::cd9660::iso::{
    ASSOCCHAR, Cdino, ISO_DIRECTORY_RECORD_SIZE, IsoDirectoryRecord, isonum_711,
};
use crate::kern::kern_event::{klist_insert_locked, klist_remove_locked};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{rrw_enter, rrw_exit, rrw_status};
use crate::kern::kern_subr::uiomove;
use crate::kern::spec_vnops::{
    spec_advlock, spec_close, spec_fsync, spec_ioctl, spec_kqfilter, spec_open, spec_pathconf,
    spec_read, spec_strategy, spec_write,
};
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::subr_xxx::{eopnotsupp, nullop};
use crate::kern::vfs_bio::{biodone, bread, breadn, brelse};
use crate::kern::vfs_default::{
    vop_generic_abortop, vop_generic_badop, vop_generic_bmap, vop_generic_bwrite,
    vop_generic_lookup, vop_generic_revoke,
};
use crate::kern::vfs_init::NAMEI_POOL;
use crate::kern::vfs_subr::{vaccess, vput};
use crate::kern::vfs_vops::{VOP_ABORTOP, VOP_BMAP, VOP_STRATEGY};
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::{B_ERROR, Buf, clrbuf};
use crate::sys::dirent::{DT_UNKNOWN, Dirent, MAXNAMLEN, dirent_size};
use crate::sys::errno::Errno;
use crate::sys::event::{
    __EV_POLL, __EV_SELECT, EV_EOF, EV_ONESHOT, EVFILT_READ, EVFILT_VNODE, EVFILT_WRITE,
    FILTEROP_ISFD, Filterops, Knote, NOTE_EOF, NOTE_REVOKE,
};
use crate::sys::file::foffset;
use crate::sys::lock::LK_RWFLAGS;
use crate::sys::malloc::{M_TEMP, M_WAITOK};
use crate::sys::param::{DEV_BSHIFT, MAXPATHLEN};
use crate::sys::pool::PR_WAITOK;
use crate::sys::stat::ALLPERMS;
use crate::sys::syslimits::NAME_MAX;
use crate::sys::types::{Gid, Mode, Nlink, Off, Register, Uid};
use crate::sys::ucred::{FSCRED, NOCRED, Ucred};
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::unistd::{
    _PC_CHOWN_RESTRICTED, _PC_LINK_MAX, _PC_NAME_MAX, _PC_NO_TRUNC, _PC_TIMESTAMP_RESOLUTION,
};
use crate::sys::vnode::{
    VA_UTIMES_CHANGE, VBLK, VCHR, VDIR, VFIFO, VLNK, VNOVAL, VREG, VSOCK, Vnode, VopAccessArgs,
    VopCloseArgs, VopGetattrArgs, VopIoctlArgs, VopIslockedArgs, VopKqfilterArgs, VopLinkArgs,
    VopLockArgs, VopOpenArgs, VopPathconfArgs, VopPrintArgs, VopReadArgs, VopReaddirArgs,
    VopReadlinkArgs, VopSetattrArgs, VopStrategyArgs, VopSymlinkArgs, VopUnlockArgs, Vops,
    cred_ref,
};

/// Which of the three entries of a `struct isoreaddir`.
#[derive(Clone, Copy)]
enum Ent {
    /// `saveent`.
    Save,
    /// `assocent`.
    Assoc,
    /// `current`.
    Current,
}

/// `struct isoreaddir`: structure for reading directories.
struct Isoreaddir<'a, 'b> {
    /// `saveent`.
    saveent: Dirent,
    /// `assocent`.
    assocent: Dirent,
    /// `current`.
    current: Dirent,
    /// `saveoff`.
    saveoff: Off,
    /// `assocoff`.
    assocoff: Off,
    /// `curroff`.
    curroff: Off,
    /// `uio`.
    uio: &'a mut Uio<'b>,
    /// `uio_off`.
    uio_off: Off,
    /// `eofflag`.
    eofflag: i32,
}

/// `cd9660_vops`: global vfs data structures for cd9660.
pub static CD9660_VOPS: Vops = Vops {
    vop_lookup: Some(cd9660_lookup),
    vop_create: Some(|_| eopnotsupp()),
    vop_mknod: Some(|_| eopnotsupp()),
    vop_open: Some(cd9660_open),
    vop_close: Some(cd9660_close),
    vop_access: Some(cd9660_access),
    vop_getattr: Some(cd9660_getattr),
    vop_setattr: Some(cd9660_setattr),
    vop_read: Some(cd9660_read),
    vop_write: Some(|_| eopnotsupp()),
    vop_ioctl: Some(cd9660_ioctl),
    vop_kqfilter: Some(cd9660_kqfilter),
    vop_revoke: Some(vop_generic_revoke),
    vop_fsync: Some(|_| nullop()),
    vop_remove: Some(|_| eopnotsupp()),
    vop_link: Some(cd9660_link),
    vop_rename: Some(|_| eopnotsupp()),
    vop_mkdir: Some(|_| eopnotsupp()),
    vop_rmdir: Some(|_| eopnotsupp()),
    vop_symlink: Some(cd9660_symlink),
    vop_readdir: Some(cd9660_readdir),
    vop_readlink: Some(cd9660_readlink),
    vop_abortop: Some(vop_generic_abortop),
    vop_inactive: Some(cd9660_inactive),
    vop_reclaim: Some(cd9660_reclaim),
    vop_lock: Some(cd9660_lock),
    vop_unlock: Some(cd9660_unlock),
    vop_bmap: Some(cd9660_bmap),
    vop_strategy: Some(cd9660_strategy),
    vop_print: Some(cd9660_print),
    vop_islocked: Some(cd9660_islocked),
    vop_pathconf: Some(cd9660_pathconf),
    vop_advlock: Some(|_| eopnotsupp()),
    vop_bwrite: Some(vop_generic_bwrite),
};

/// `cd9660_specvops`: special device vnode ops.
pub static CD9660_SPECVOPS: Vops = Vops {
    vop_access: Some(cd9660_access),
    vop_getattr: Some(cd9660_getattr),
    vop_setattr: Some(cd9660_setattr),
    vop_inactive: Some(cd9660_inactive),
    vop_reclaim: Some(cd9660_reclaim),
    vop_lock: Some(cd9660_lock),
    vop_unlock: Some(cd9660_unlock),
    vop_print: Some(cd9660_print),
    vop_islocked: Some(cd9660_islocked),

    // XXX: Keep in sync with spec_vops.
    vop_lookup: Some(vop_generic_lookup),
    vop_create: Some(|_| vop_generic_badop()),
    vop_mknod: Some(|_| vop_generic_badop()),
    vop_open: Some(spec_open),
    vop_close: Some(spec_close),
    vop_read: Some(spec_read),
    vop_write: Some(spec_write),
    vop_ioctl: Some(spec_ioctl),
    vop_kqfilter: Some(spec_kqfilter),
    vop_revoke: Some(vop_generic_revoke),
    vop_fsync: Some(spec_fsync),
    vop_remove: Some(|_| vop_generic_badop()),
    vop_link: Some(|_| vop_generic_badop()),
    vop_rename: Some(|_| vop_generic_badop()),
    vop_mkdir: Some(|_| vop_generic_badop()),
    vop_rmdir: Some(|_| vop_generic_badop()),
    vop_symlink: Some(|_| vop_generic_badop()),
    vop_readdir: Some(|_| vop_generic_badop()),
    vop_readlink: Some(|_| vop_generic_badop()),
    vop_abortop: Some(|_| vop_generic_badop()),
    vop_bmap: Some(vop_generic_bmap),
    vop_strategy: Some(spec_strategy),
    vop_pathconf: Some(spec_pathconf),
    vop_advlock: Some(spec_advlock),
    vop_bwrite: Some(vop_generic_bwrite),
};

/// `cd9660read_filtops`.
pub static CD9660READ_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_cd9660detach),
    f_event: Some(filt_cd9660read),
    f_modify: None,
    f_process: None,
};

/// `cd9660write_filtops`.
pub static CD9660WRITE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_cd9660detach),
    f_event: Some(filt_cd9660write),
    f_modify: None,
    f_process: None,
};

/// `cd9660vnode_filtops`.
pub static CD9660VNODE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_cd9660detach),
    f_event: Some(filt_cd9660vnode),
    f_modify: None,
    f_process: None,
};

/// `*cred` of a credential the C dereferences: a real one (`NOCRED`/`FSCRED` panic).
fn ucred<'a>(cred: *const Ucred) -> &'a Ucred {
    // SAFETY: the credentials a vnode operation receives are held by its caller for the
    // operation's duration (`cred_ref`'s contract).
    match unsafe { cred_ref(cred) } {
        Some(c) => c,
        None => panic(format_args!(
            "cd9660: {} credential",
            if ptr::eq(cred, NOCRED) {
                "missing"
            } else if ptr::eq(cred, FSCRED) {
                "kernel"
            } else {
                "NULL"
            }
        )),
    }
}

/// `cd9660_setattr` (`vop_setattr`): only allowed for block and character special devices.
pub fn cd9660_setattr(ap: &mut VopSetattrArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let vap = &*ap.a_vap;

    if vap.va_flags != VNOVAL as u64
        || vap.va_uid != VNOVAL as Uid
        || vap.va_gid != VNOVAL as Gid
        || vap.va_atime.tv_nsec != i64::from(VNOVAL)
        || vap.va_mtime.tv_nsec != i64::from(VNOVAL)
        || vap.va_mode != VNOVAL as Mode
        || vap.va_vaflags & VA_UTIMES_CHANGE != 0
    {
        return Err(Errno::EROFS);
    }
    if vap.va_size != VNOVAL as u64 {
        return match vp.v_type.get() {
            VDIR => Err(Errno::EISDIR),
            VLNK | VREG => Err(Errno::EROFS),
            VCHR | VBLK | VSOCK | VFIFO => Ok(()),
            _ => Err(Errno::EINVAL),
        };
    }

    Err(Errno::EINVAL)
}

/// `cd9660_open` (`vop_open`): nothing to do.
pub fn cd9660_open(_ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    Ok(())
}

/// `cd9660_close` (`vop_close`): update the times on the inode on writeable file systems;
/// nothing to do here.
pub fn cd9660_close(_ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    Ok(())
}

/// `cd9660_access` (`vop_access`): check mode permission on inode pointer. Mode is READ,
/// WRITE or EXEC. The mode is shifted to select the owner/group/other fields. The super
/// user is granted all permissions.
pub fn cd9660_access(ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
    let ip = vtoi(ap.a_vp);
    let ino = ip.inode.get();

    vaccess(
        ap.a_vp.v_type.get(),
        Mode::from(ino.iso_mode) & ALLPERMS,
        ino.iso_uid,
        ino.iso_gid,
        ap.a_mode,
        ucred(ap.a_cred),
    )
}

/// `cd9660_getattr` (`vop_getattr`).
pub fn cd9660_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);
    let ino = ip.inode.get();
    let vap = &mut *ap.a_vap;

    vap.va_fsid = i64::from(ip.i_dev.get());
    vap.va_fileid = u64::from(ip.i_number.get());

    vap.va_mode = Mode::from(ino.iso_mode) & ALLPERMS;
    vap.va_nlink = ino.iso_links as Nlink;
    vap.va_uid = ino.iso_uid;
    vap.va_gid = ino.iso_gid;
    vap.va_atime = ino.iso_atime;
    vap.va_mtime = ino.iso_mtime;
    vap.va_ctime = ino.iso_ctime;
    vap.va_rdev = ino.iso_rdev;

    vap.va_size = ip.i_size.get();
    if ip.i_size.get() == 0 && vp.v_type.get() == VLNK {
        let Some(cp) = malloc(MAXPATHLEN, M_TEMP, M_WAITOK) else {
            panic(format_args!("cd9660_getattr: no memory"));
        };
        let mut aiov = [Iovec {
            iov_base: cp.as_ptr().cast::<c_void>(),
            iov_len: MAXPATHLEN,
        }];
        let mut auio = Uio {
            uio_iov: &mut aiov,
            uio_offset: 0,
            uio_resid: MAXPATHLEN,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: Some(ap.a_p),
        };
        let mut rdlnk = VopReadlinkArgs {
            a_vp: vp,
            a_uio: &mut auio,
            a_cred: ap.a_cred,
        };
        if cd9660_readlink(&mut rdlnk).is_ok() {
            vap.va_size = (MAXPATHLEN - auio.uio_resid) as u64;
        }
        free(cp, M_TEMP, MAXPATHLEN);
    }
    vap.va_flags = 0;
    vap.va_gen = 1;
    vap.va_blocksize = i64::from(ip.mnt().logical_block_size);
    vap.va_bytes = ip.i_size.get();
    vap.va_type = vp.v_type.get();
    Ok(())
}

/// `MAX_RA`: the most read-ahead blocks of `cd9660_read`.
const MAX_RA: usize = 32;

/// `cd9660_read` (`vop_read`): vnode op for reading.
pub fn cd9660_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let uio = &mut *ap.a_uio;
    let ip = vtoi(vp);

    if uio.uio_resid == 0 {
        return Ok(());
    }
    if uio.uio_offset < 0 {
        return Err(Errno::EINVAL);
    }
    ip.i_flag.set(ip.i_flag.get() | IN_ACCESS);
    let imp = ip.mnt();
    loop {
        let mut ci = ip.i_ci.get();

        let lbn = lblkno(imp, uio.uio_offset);
        let on = blkoff(imp, uio.uio_offset) as usize;
        let mut n = (imp.logical_block_size as usize - on).min(uio.uio_resid);
        let diff = ip.i_size.get() as Off - uio.uio_offset;
        if diff <= 0 {
            return Ok(());
        }
        if (diff as u64) < n as u64 {
            n = diff as usize;
        }
        let size = blksize(imp);
        let rablock = lbn + 1;
        let (bp, error) = if ci.ci_lastr + 1 == lbn {
            let mut blks = [0i64; MAX_RA];
            let mut sizes = [0i32; MAX_RA];
            let mut i = 0;
            while i < MAX_RA && lblktosize(imp, rablock + i as i64) < ip.i_size.get() as i64 {
                blks[i] = rablock + i as i64;
                sizes[i] = blksize(imp);
                i += 1;
            }
            breadn(vp, lbn, size, &blks[..i], &sizes[..i])
        } else {
            bread(vp, lbn, size)
        };
        ci.ci_lastr = lbn;
        ip.i_ci.set(ci);
        n = n.min((size as usize).saturating_sub(bp.b_resid.get()));
        if let Err(e) = error {
            brelse(bp);
            return Err(e);
        }

        let error = {
            // SAFETY: the buffer is ours (busy from `bread`) and mapped; the slice dies
            // before it is released.
            let data = unsafe { bp.data() };
            match data.get_mut(on..on + n) {
                Some(d) => uiomove(d, uio),
                None => Err(Errno::EIO),
            }
        };

        brelse(bp);
        if error.is_err() || uio.uio_resid == 0 || n == 0 {
            return error;
        }
    }
}

/// `cd9660_ioctl` (`vop_ioctl`).
pub fn cd9660_ioctl(_ap: &mut VopIoctlArgs<'_>) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

/// `cd9660_mmap`: mmap a file. NB Currently unsupported (in no operations table).
pub fn cd9660_mmap() -> Result<(), Errno> {
    Err(Errno::EINVAL)
}

/// `cd9660_seek`: seek on a file. Nothing to do, so just return (in no operations table).
pub fn cd9660_seek() -> Result<(), Errno> {
    Ok(())
}

/// A `struct dirent` with every byte zero and the type `DT_UNKNOWN`.
const fn dirent_zeroed() -> Dirent {
    Dirent {
        d_fileno: 0,
        d_off: 0,
        d_reclen: 0,
        d_type: DT_UNKNOWN,
        d_namlen: 0,
        __d_padding: [0; 4],
        d_name: [0; MAXNAMLEN + 1],
    }
}

/// The bytes of a `struct dirent`, as `uiomove(dp, ...)` sees them.
fn dirent_bytes(dp: &mut Dirent) -> &mut [u8] {
    // SAFETY: `Dirent` is `#[repr(C)]` integers and byte arrays with no padding (its size is
    // the sum of its members, checked below), so every byte is initialised and any byte
    // pattern is a valid value; the slice borrows `dp`.
    unsafe { core::slice::from_raw_parts_mut(ptr::from_mut(dp).cast::<u8>(), size_of::<Dirent>()) }
}

/// `iso_uiodir`: copy one entry out to the user; `Err(None)` when it does not fit (the C's
/// -1, which clears the end-of-file flag).
fn iso_uiodir(idp: &mut Isoreaddir<'_, '_>, which: Ent, off: Off) -> Result<(), Option<Errno>> {
    let Isoreaddir {
        saveent,
        assocent,
        current,
        uio,
        uio_off,
        eofflag,
        ..
    } = idp;
    let dp = match which {
        Ent::Save => saveent,
        Ent::Assoc => assocent,
        Ent::Current => current,
    };

    let namlen = usize::from(dp.d_namlen);
    dp.d_name[namlen] = 0;
    dp.d_reclen = dirent_size(dp) as u16;

    if dp.d_name[..namlen].contains(&b'/') {
        // illegal file name
        return Err(Some(Errno::EINVAL));
    }

    let reclen = usize::from(dp.d_reclen);
    if uio.uio_resid < reclen {
        *eofflag = 0;
        return Err(None);
    }

    dp.d_off = off;
    uiomove(&mut dirent_bytes(dp)[..reclen], uio).map_err(Some)?;
    *uio_off = off;
    Ok(())
}

/// `iso_shipdir`: for the default format, hold the current entry back until the next one
/// shows whether it has an associated file of the same name, then ship them.
fn iso_shipdir(idp: &mut Isoreaddir<'_, '_>) -> Result<(), Option<Errno>> {
    let mut cl = usize::from(idp.current.d_namlen);
    let mut cstart = 0;

    let assoc = cl > 1 && idp.current.d_name[0] == ASSOCCHAR;
    if assoc {
        cl -= 1;
        cstart = 1;
    }

    let (sent, sstart, sl): (&Dirent, usize, i32) = if idp.saveent.d_namlen != 0 {
        (&idp.saveent, 0, i32::from(idp.saveent.d_namlen))
    } else {
        (&idp.assocent, 1, i32::from(idp.assocent.d_namlen) - 1)
    };
    if sl > 0 {
        let sl = sl as usize;
        if sl != cl || sent.d_name[sstart..sstart + sl] != idp.current.d_name[cstart..cstart + cl] {
            if idp.assocent.d_namlen != 0 {
                iso_uiodir(idp, Ent::Assoc, idp.assocoff)?;
                idp.assocent.d_namlen = 0;
            }
            if idp.saveent.d_namlen != 0 {
                iso_uiodir(idp, Ent::Save, idp.saveoff)?;
                idp.saveent.d_namlen = 0;
            }
        }
    }
    idp.current.d_reclen = dirent_size(&idp.current) as u16;
    if assoc {
        idp.assocoff = idp.curroff;
        idp.assocent = idp.current;
    } else {
        idp.saveoff = idp.curroff;
        idp.saveent = idp.current;
    }
    Ok(())
}

/// `cd9660_readdir` (`vop_readdir`): vnode op for readdir.
pub fn cd9660_readdir(ap: &mut VopReaddirArgs<'_, '_>) -> Result<(), Errno> {
    let vdp = ap.a_vp;
    let dp = vtoi(vdp);
    let imp = dp.mnt();
    let bmask = imp.im_bmask as Off;
    let lbs = imp.logical_block_size as usize;

    // These are passed to copyout(), so make sure there's no garbage being leaked in
    // padding or after short names. XXX Is it worth trying to figure out the type?
    let curroff = ap.a_uio.uio_offset;
    let mut idp = Isoreaddir {
        saveent: dirent_zeroed(),
        assocent: dirent_zeroed(),
        current: dirent_zeroed(),
        saveoff: 0,
        assocoff: 0,
        curroff,
        uio: &mut *ap.a_uio,
        uio_off: curroff,
        eofflag: 1,
    };

    let mut bp: Option<&'static Buf> = None;
    let mut entryoffsetinblock = (idp.curroff & bmask) as usize;
    if entryoffsetinblock != 0 {
        let (b, _) = cd9660_bufatoff(dp, idp.curroff)?;
        bp = Some(b);
    }
    let endsearch = dp.i_size.get() as Off;

    let mut error: Result<(), Option<Errno>> = Ok(());
    while idp.curroff < endsearch {
        // If offset is on a block boundary, read the next directory block. Release previous
        // if it exists.
        if idp.curroff & bmask == 0 {
            if let Some(b) = bp.take() {
                brelse(b);
            }
            match cd9660_bufatoff(dp, idp.curroff) {
                Ok((b, _)) => bp = Some(b),
                Err(e) => {
                    error = Err(Some(e));
                    break;
                }
            }
            entryoffsetinblock = 0;
        }
        let Some(b) = bp else {
            panic(format_args!("cd9660_readdir: no directory buffer"));
        };
        // SAFETY: the buffer is ours (busy from `bread`) and mapped; no other view of it
        // is alive.
        let data: &[u8] = unsafe { b.data() };

        // Get pointer to next entry.
        let reclen = usize::from(data.get(entryoffsetinblock).copied().unwrap_or(0));
        if reclen == 0 {
            // skip to next block, if any
            idp.curroff = (idp.curroff & !bmask) + imp.logical_block_size as Off;
            continue;
        }

        if reclen < ISO_DIRECTORY_RECORD_SIZE {
            error = Err(Some(Errno::EINVAL));
            // illegal entry, stop
            break;
        }

        if entryoffsetinblock + reclen > lbs {
            error = Err(Some(Errno::EINVAL));
            // illegal directory, so stop looking
            break;
        }

        let Some(ep) = data
            .get(entryoffsetinblock..)
            .and_then(IsoDirectoryRecord::new)
        else {
            error = Err(Some(Errno::EINVAL));
            break;
        };
        idp.current.d_namlen = isonum_711(ep.name_len());

        if reclen < ISO_DIRECTORY_RECORD_SIZE + usize::from(idp.current.d_namlen) {
            error = Err(Some(Errno::EINVAL));
            // illegal entry, stop
            break;
        }

        let mut ino: Cdino = if isonum_711(ep.flags()) & 2 != 0 {
            isodirino(&ep, imp)
        } else {
            ((b.b_blkno.get() << DEV_BSHIFT) + entryoffsetinblock as i64) as Cdino
        };

        idp.curroff += reclen as Off;

        let mut namelen = 0u16;
        if imp.iso_ftype == ISO_FTYPE_RRIP {
            cd9660_rrip_getname(&ep, &mut idp.current.d_name, &mut namelen, &mut ino, imp);
            idp.current.d_fileno = u64::from(ino);
            idp.current.d_namlen = namelen as u8;
            if idp.current.d_namlen != 0 {
                error = {
                    let off = idp.curroff;
                    iso_uiodir(&mut idp, Ent::Current, off)
                };
            }
        } else {
            // ISO_FTYPE_DEFAULT || ISO_FTYPE_9660
            idp.current.d_fileno = u64::from(ino);
            idp.current.d_name[..3].copy_from_slice(b"..\0");
            if idp.current.d_namlen == 1 && ep.name0() == 0 {
                idp.current.d_namlen = 1;
                error = {
                    let off = idp.curroff;
                    iso_uiodir(&mut idp, Ent::Current, off)
                };
            } else if idp.current.d_namlen == 1 && ep.name0() == 1 {
                idp.current.d_namlen = 2;
                error = {
                    let off = idp.curroff;
                    iso_uiodir(&mut idp, Ent::Current, off)
                };
            } else {
                let n = usize::from(idp.current.d_namlen);
                isofntrans(
                    &ep.name()[..n],
                    &mut idp.current.d_name,
                    &mut namelen,
                    imp.iso_ftype == ISO_FTYPE_9660,
                    isonum_711(ep.flags()) & 4 != 0,
                    imp.joliet_level,
                );
                idp.current.d_namlen = namelen as u8;
                error = if imp.iso_ftype == ISO_FTYPE_DEFAULT {
                    iso_shipdir(&mut idp)
                } else {
                    {
                        let off = idp.curroff;
                        iso_uiodir(&mut idp, Ent::Current, off)
                    }
                };
            }
        }
        if error.is_err() {
            break;
        }

        entryoffsetinblock += reclen;
    }

    if error.is_ok() && imp.iso_ftype == ISO_FTYPE_DEFAULT {
        idp.current.d_namlen = 0;
        error = iso_shipdir(&mut idp);
    }
    // the C's error < 0: the user's buffer is full
    let error = error.or_else(|e| e.map_or(Ok(()), Err));

    if let Some(b) = bp {
        brelse(b);
    }

    idp.uio.uio_offset = idp.uio_off;
    *ap.a_eofflag = idp.eofflag;

    error
}

/// `cd9660_readlink` (`vop_readlink`): return target name of a symbolic link. Shouldn't we
/// get the parents vnode and read the data from there? This could eventually result in
/// deadlocks in `cd9660_lookup`. But otherwise the block read here is in the block buffer
/// two times.
pub fn cd9660_readlink(ap: &mut VopReadlinkArgs<'_, '_>) -> Result<(), Errno> {
    let ip = vtoi(ap.a_vp);
    let imp = ip.mnt();
    let uio = &mut *ap.a_uio;

    if imp.iso_ftype != ISO_FTYPE_RRIP {
        return Err(Errno::EINVAL);
    }

    // Get parents directory record block that this inode included.
    let number = ip.i_number.get();
    let (bp, error) = bread(
        imp.im_devvp,
        i64::from(number >> imp.im_bshift) << (imp.im_bshift - DEV_BSHIFT as i32),
        imp.logical_block_size,
    );
    if error.is_err() {
        brelse(bp);
        return Err(Errno::EINVAL);
    }

    // Setup the directory pointer for this inode
    let off = (number & imp.im_bmask as u32) as usize;
    // SAFETY: the buffer is ours (busy from `bread`) and mapped; the record view dies
    // before it is released.
    let data: &[u8] = unsafe { bp.data() };
    let dirp = data.get(off..).and_then(IsoDirectoryRecord::new);

    // Just make sure, we have a right one....
    //   1: Check not cross boundary on block
    let Some(dirp) = dirp
        .filter(|d| off + usize::from(isonum_711(d.length())) <= imp.logical_block_size as usize)
    else {
        brelse(bp);
        return Err(Errno::EINVAL);
    };

    // Now get a buffer. Abuse a namei buffer for now.
    let direct = uio.uio_segflg == UioSeg::UIO_SYSSPACE
        && uio
            .uio_iov
            .first()
            .is_some_and(|iov| iov.iov_len >= MAXPATHLEN);
    let pooled: Option<NonNull<u8>> = if direct {
        None
    } else {
        let Some(m) = pool_get(&NAMEI_POOL, PR_WAITOK) else {
            panic(format_args!("cd9660_readlink: no namei buffer"));
        };
        Some(m)
    };
    let base = match pooled {
        Some(m) => m.as_ptr(),
        None => uio.uio_iov[0].iov_base.cast::<u8>(),
    };
    // SAFETY: either a `namei_pool` item (`MAXPATHLEN` bytes, ours until `pool_put`), or the
    // first iovec of a kernel (`UIO_SYSSPACE`) uio, which points at `iov_len >= MAXPATHLEN`
    // writable bytes for the uio's lifetime; nothing else views them meanwhile.
    let symname = unsafe { core::slice::from_raw_parts_mut(base, MAXPATHLEN) };

    // Ok, we just gathering a symbolic name in SL record.
    let mut symlen = 0u16;
    if cd9660_rrip_getsymname(&dirp, symname, &mut symlen, imp) == 0 {
        if let Some(m) = pooled {
            pool_put(&NAMEI_POOL, m);
        }
        brelse(bp);
        return Err(Errno::EINVAL);
    }
    // Don't forget before you leave from home ;-)
    brelse(bp);

    // return with the symbolic name to caller's.
    let symlen = usize::from(symlen).min(MAXPATHLEN);
    if let Some(m) = pooled {
        let error = uiomove(&mut symname[..symlen], uio);
        pool_put(&NAMEI_POOL, m);
        return error;
    }
    uio.uio_resid -= symlen;
    let iov = &mut uio.uio_iov[0];
    iov.iov_base = iov.iov_base.cast::<u8>().wrapping_add(symlen).cast();
    iov.iov_len -= symlen;
    Ok(())
}

/// `cd9660_link` (`vop_link`): read-only.
pub fn cd9660_link(ap: &mut VopLinkArgs<'_>) -> Result<(), Errno> {
    let _ = VOP_ABORTOP(ap.a_dvp, ap.a_cnp);
    vput(ap.a_dvp);
    Err(Errno::EROFS)
}

/// `cd9660_symlink` (`vop_symlink`): read-only.
pub fn cd9660_symlink(ap: &mut VopSymlinkArgs<'_>) -> Result<(), Errno> {
    let _ = VOP_ABORTOP(ap.a_dvp, ap.a_cnp);
    vput(ap.a_dvp);
    Err(Errno::EROFS)
}

/// `cd9660_lock` (`vop_lock`): lock an inode.
pub fn cd9660_lock(ap: &mut VopLockArgs) -> Result<(), Errno> {
    rrw_enter(&vtoi(ap.a_vp).i_lock, ap.a_flags & LK_RWFLAGS)
}

/// `cd9660_unlock` (`vop_unlock`): unlock an inode.
pub fn cd9660_unlock(ap: &mut VopUnlockArgs) -> Result<(), Errno> {
    rrw_exit(&vtoi(ap.a_vp).i_lock);
    Ok(())
}

/// `cd9660_strategy` (`vop_strategy`): calculate the logical to physical mapping if not
/// done already, then call the device strategy routine.
pub fn cd9660_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    let bp = ap.a_bp;
    let Some(vp) = bp.b_vp.get() else {
        panic(format_args!("cd9660_strategy: buffer without a vnode"));
    };

    let ip = vtoi(vp);
    if vp.v_type.get() == VBLK || vp.v_type.get() == VCHR {
        panic(format_args!("cd9660_strategy: spec"));
    }
    if bp.b_blkno.get() == bp.b_lblkno.get() {
        let mut blkno = bp.b_blkno.get();
        let error = VOP_BMAP(vp, bp.b_lblkno.get(), None, Some(&mut blkno), None);
        bp.b_blkno.set(blkno);
        if let Err(e) = error {
            bp.b_error.set(Some(e));
            bp.set(B_ERROR);
            let s = splbio();
            biodone(bp);
            splx(s);
            return Err(e);
        }
        if bp.b_blkno.get() == -1 {
            // SAFETY: a buffer handed to the strategy routine is busy for this I/O and
            // mapped; the strategy owns it until biodone.
            unsafe { clrbuf(bp) };
        }
    }
    if bp.b_blkno.get() == -1 {
        let s = splbio();
        biodone(bp);
        splx(s);
        return Ok(());
    }
    let Some(devvp) = ip.i_devvp.get() else {
        panic(format_args!("cd9660_strategy: no device vnode"));
    };
    bp.b_dev.set(devvp.v_rdev());
    let _ = VOP_STRATEGY(devvp, bp);
    Ok(())
}

/// `cd9660_print` (`vop_print`): print out the contents of an inode.
pub fn cd9660_print(_ap: &mut VopPrintArgs) -> Result<(), Errno> {
    #[cfg(any(feature = "debug", feature = "diagnostic"))]
    crate::kprintf!("tag VT_ISOFS, isofs vnode\n");
    Ok(())
}

/// `cd9660_islocked` (`vop_islocked`): check for a locked inode.
pub fn cd9660_islocked(ap: &mut VopIslockedArgs) -> i32 {
    rrw_status(&vtoi(ap.a_vp).i_lock)
}

/// `cd9660_pathconf` (`vop_pathconf`): return POSIX pathconf information applicable to
/// cd9660 filesystems.
pub fn cd9660_pathconf(ap: &mut VopPathconfArgs<'_>) -> Result<(), Errno> {
    *ap.a_retval = match ap.a_name {
        _PC_LINK_MAX => 1,
        _PC_NAME_MAX => {
            if vtoi(ap.a_vp).mnt().iso_ftype == ISO_FTYPE_RRIP {
                NAME_MAX as Register
            } else {
                37
            }
        }
        _PC_CHOWN_RESTRICTED => 1,
        _PC_NO_TRUNC => 1,
        _PC_TIMESTAMP_RESOLUTION => 1_000_000_000, // one billion nanoseconds
        _ => return Err(Errno::EINVAL),
    };

    Ok(())
}

/// `cd9660_kqfilter` (`vop_kqfilter`).
pub fn cd9660_kqfilter(ap: &mut VopKqfilterArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let kn = ap.a_kn;

    match kn.kn_filter().get() {
        EVFILT_READ => kn.kn_fop.set(Some(&CD9660READ_FILTOPS)),
        EVFILT_WRITE => kn.kn_fop.set(Some(&CD9660WRITE_FILTOPS)),
        EVFILT_VNODE => kn.kn_fop.set(Some(&CD9660VNODE_FILTOPS)),
        _ => return Err(Errno::EINVAL),
    }

    kn.kn_hook.set(ptr::from_ref(vp).cast_mut().cast());

    klist_insert_locked(&vp.v_klist, kn);

    Ok(())
}

/// `kn->kn_hook` of a cd9660 knote: its vnode.
fn kn_vnode(kn: &Knote) -> &'static Vnode {
    // SAFETY: `cd9660_kqfilter` points `kn_hook` at the vnode, a `vnode_pool` item that is
    // never freed.
    match unsafe { kn.kn_hook.get().cast::<Vnode>().as_ref() } {
        Some(vp) => vp,
        None => panic(format_args!("knote {:p}: no vnode", kn)),
    }
}

/// `filt_cd9660detach`.
pub fn filt_cd9660detach(kn: &Knote) {
    let vp = kn_vnode(kn);

    klist_remove_locked(&vp.v_klist, kn);
}

/// `filt_cd9660read`: the bytes past the file offset; always ready for poll and select.
pub fn filt_cd9660read(kn: &Knote, hint: i64) -> bool {
    let vp = kn_vnode(kn);
    let node = vtoi(vp);

    // filesystem is gone, so set the EOF flag and schedule the knote for deletion.
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF | EV_ONESHOT);
        return true;
    }

    kn.kn_data()
        .set((node.i_size.get() as i64).wrapping_sub(foffset(kn.fp())));
    if kn.kn_data().get() == 0 && kn.kn_sfflags.get() & NOTE_EOF != 0 {
        kn.kn_fflags().set(kn.kn_fflags().get() | NOTE_EOF);
        return true;
    }

    if kn.has_flags(__EV_POLL | __EV_SELECT) {
        return true;
    }

    kn.kn_data().get() != 0
}

/// `filt_cd9660write`: a file is always writable (as far as kqueue is concerned).
pub fn filt_cd9660write(kn: &Knote, hint: i64) -> bool {
    // filesystem is gone, so set the EOF flag and schedule the knote for deletion.
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF | EV_ONESHOT);
        return true;
    }

    kn.kn_data().set(0);
    true
}

/// `filt_cd9660vnode`: records the vnode events (`NOTE_*`) the user asked for.
pub fn filt_cd9660vnode(kn: &Knote, hint: i64) -> bool {
    let hint32 = hint as u32;
    if kn.kn_sfflags.get() & hint32 != 0 {
        kn.kn_fflags().set(kn.kn_fflags().get() | hint32);
    }
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF);
        return true;
    }
    kn.kn_fflags().get() != 0
}

const _: () = {
    assert!(
        size_of::<Dirent>() == 8 + 8 + 2 + 1 + 1 + 4 + MAXNAMLEN + 1,
        "struct dirent has no padding"
    );
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the vnode operations that need no disc: `setattr`'s refusals,
    // `pathconf`, the kqueue filters, and the merging of associated files in `readdir`
    // (`iso_shipdir`/`iso_uiodir`) into a kernel buffer.

    use std::vec::Vec;

    use super::*;
    use crate::isofs::cd9660::iso::tests::{test_mnt, test_node};
    use crate::kern::vfs_subr::vattr_null;
    use crate::sys::vnode::Vattr;

    /// `cd9660_setattr` of `vap` on a vnode of type `t`.
    fn setattr(t: crate::sys::vnode::Vtype, vap: &mut Vattr) -> Result<(), Errno> {
        let vp: &'static Vnode = std::boxed::Box::leak(std::boxed::Box::new(Vnode::new()));
        vp.v_type.set(t);
        let p = crate::sys::proc::Proc::new();
        let mut a = VopSetattrArgs {
            a_vp: vp,
            a_vap: vap,
            a_cred: NOCRED,
            a_p: &p,
        };
        cd9660_setattr(&mut a)
    }

    #[test]
    fn setattr_refuses_everything_but_device_sizes() {
        let mut va = Vattr::new();
        vattr_null(&mut va);
        assert_eq!(setattr(VREG, &mut va), Err(Errno::EINVAL));
        va.va_size = 0;
        assert_eq!(setattr(VDIR, &mut va), Err(Errno::EISDIR));
        assert_eq!(setattr(VREG, &mut va), Err(Errno::EROFS));
        assert_eq!(setattr(VLNK, &mut va), Err(Errno::EROFS));
        assert_eq!(setattr(VCHR, &mut va), Ok(()));
        assert_eq!(setattr(VFIFO, &mut va), Ok(()));
        vattr_null(&mut va);
        va.va_uid = 0;
        assert_eq!(setattr(VCHR, &mut va), Err(Errno::EROFS));
        vattr_null(&mut va);
        va.va_vaflags |= VA_UTIMES_CHANGE;
        assert_eq!(setattr(VCHR, &mut va), Err(Errno::EROFS));
    }

    #[test]
    fn pathconf_depends_on_rock_ridge() {
        for (ftype, name_max) in [(ISO_FTYPE_RRIP, 255), (ISO_FTYPE_DEFAULT, 37)] {
            let (_ip, vp) = test_node(test_mnt(ftype));
            let mut v: Register = 0;
            let mut ask = |name: i32| {
                let mut a = VopPathconfArgs {
                    a_vp: vp,
                    a_name: name,
                    a_retval: &mut v,
                };
                cd9660_pathconf(&mut a).map(|()| v)
            };
            assert_eq!(ask(_PC_NAME_MAX), Ok(name_max));
            assert_eq!(ask(_PC_LINK_MAX), Ok(1));
            assert_eq!(ask(_PC_NO_TRUNC), Ok(1));
            assert_eq!(ask(_PC_TIMESTAMP_RESOLUTION), Ok(1_000_000_000));
            assert_eq!(ask(crate::sys::unistd::_PC_PIPE_BUF), Err(Errno::EINVAL));
        }
    }

    #[test]
    fn filters_report_writability_and_vnode_events() {
        use crate::sys::event::{NOTE_DELETE, NOTE_WRITE};
        let kn = Knote::new();
        assert!(filt_cd9660write(&kn, 0));
        assert_eq!(kn.kn_data().get(), 0);
        kn.kn_sfflags.set(NOTE_WRITE);
        assert!(!filt_cd9660vnode(&kn, i64::from(NOTE_DELETE)));
        assert!(filt_cd9660vnode(&kn, i64::from(NOTE_WRITE)));
        assert!(filt_cd9660vnode(&kn, i64::from(NOTE_REVOKE)) && kn.has_flags(EV_EOF));
    }

    #[test]
    fn readdir_ships_an_associated_file_before_its_file() {
        let mut buf = [0u8; 512];
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: 512,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let mut idp = Isoreaddir {
            saveent: dirent_zeroed(),
            assocent: dirent_zeroed(),
            current: dirent_zeroed(),
            saveoff: 0,
            assocoff: 0,
            curroff: 0,
            uio: &mut uio,
            uio_off: 0,
            eofflag: 1,
        };
        for (i, name) in [&b"FOO"[..], b"=FOO", b"BAR"].into_iter().enumerate() {
            idp.curroff = 100 * (i as Off + 1);
            idp.current.d_fileno = i as u64 + 1;
            idp.current.d_name[..name.len()].copy_from_slice(name);
            idp.current.d_namlen = name.len() as u8;
            iso_shipdir(&mut idp).unwrap();
        }
        idp.current.d_namlen = 0;
        iso_shipdir(&mut idp).unwrap();
        assert_eq!((idp.uio_off, idp.eofflag), (300, 1));
        let resid = idp.uio.uio_resid;

        let mut entries: Vec<(u64, i64, Vec<u8>)> = Vec::new();
        let mut off = 0;
        while off < 512 - resid {
            let d = Dirent::from_bytes(&buf[off..]).unwrap();
            let name = buf[off + 24..off + 24 + usize::from(d.d_namlen)].to_vec();
            assert_eq!(buf[off + 24 + usize::from(d.d_namlen)], 0);
            entries.push((d.d_fileno, d.d_off, name));
            off += usize::from(d.d_reclen);
        }
        assert_eq!(
            entries,
            [
                (2, 200, b"=FOO".to_vec()),
                (1, 100, b"FOO".to_vec()),
                (3, 300, b"BAR".to_vec())
            ]
        );
    }

    #[test]
    fn readdir_stops_when_the_buffer_is_full_and_refuses_slashes() {
        let mut buf = [0u8; 40];
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: 40,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let mut idp = Isoreaddir {
            saveent: dirent_zeroed(),
            assocent: dirent_zeroed(),
            current: dirent_zeroed(),
            saveoff: 0,
            assocoff: 0,
            curroff: 0,
            uio: &mut uio,
            uio_off: 0,
            eofflag: 1,
        };
        idp.current.d_name[..3].copy_from_slice(b"a/b");
        idp.current.d_namlen = 3;
        assert_eq!(
            iso_uiodir(&mut idp, Ent::Current, 1),
            Err(Some(Errno::EINVAL))
        );
        idp.current.d_name[..3].copy_from_slice(b"abc");
        assert_eq!(iso_uiodir(&mut idp, Ent::Current, 1), Ok(()));
        assert_eq!(iso_uiodir(&mut idp, Ent::Current, 2), Err(None));
        assert_eq!((idp.uio_off, idp.eofflag), (1, 0));
    }
}
/* </TESTS> */
