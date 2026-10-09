/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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

/*
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * John Heidemann of the UCLA Ficus project.
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
 *	Id: ntfs_vnops.c,v 1.5 1999/05/12 09:43:06 semenu Exp
 *
 */
/* </LICENSES> */

/* <CODE> */
//! NTFS vnode operations: attributes and access (the mount's owner, group and mode),
//! `read` and `strategy` over `ntfs_readattr`, `readdir` over `ntfs_ntreaddir` (with `.` and
//! `..` simulated), `lookup` over `ntfs_ntlookupfile`, the identity block map, and the
//! release of an fnode (`reclaim`). The vnode lock is `nullop`: the ntnode lock is what the
//! subroutines take.
//!
//! Upstream: sys/ntfs/ntfs_vnops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `ntfs_readdir` copies each `struct dirent` out whole, as the C does (the bytes of
//!   `d_name` past the NUL are what an earlier, longer name left, the structure being
//!   zeroed once); it reads the entries of the directory buffer through
//!   [`Packed::read`], and an entry of no length ends the buffer's walk (the C walks to the
//!   `NTFS_IEFLAG_LAST` entry, past the buffer if there is none).
//! - `ntfs_read` and `ntfs_strategy` hand `ntfs_readattr` the `uio` or the buffer's data as
//!   [`Rdata`].
//! - `ntfs_print` prints under feature `debug` or `diagnostic` (`VFSLCKDEBUG` has none);
//!   the `DIAGNOSTIC` `vprint`s of `ntfs_inactive` and `ntfs_reclaim` under feature
//!   `diagnostic`. `ntfs_prtactive` is an `AtomicI32`.
//! - `ntfs_open`'s and `ntfs_close`'s `NTFS_DEBUG` printouts and the `DPRINTF`s are left out
//!   (`NTFS_DEBUG` is off).
//! - `vop_lock`, `vop_unlock` and `vop_islocked` are `nullop` closures (`islocked` answers
//!   0, `nullop`'s value).

use core::ptr;
use core::sync::atomic::AtomicI32;

use crate::kern::kern_subr::uiomove;
use crate::kern::subr_prf::{panic, printf};
use crate::kern::subr_xxx::{eopnotsupp, nullop};
use crate::kern::vfs_bio::biodone;
use crate::kern::vfs_cache::{cache_enter, cache_lookup, cache_purge};
use crate::kern::vfs_default::vop_generic_bwrite;
use crate::kern::vfs_subr::{vput, vref};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_UNLOCK};
use crate::machine::intr::{splbio, splx};
use crate::ntfs::ntfs::{
    AttrIndexentry, FTONT, NTFS_A_NAME, NTFS_FFLAG_DIR, NTFS_IEFLAG_LAST, NTFS_MAXFILENAME,
    NTFS_ROOTINO, Packed, VTOF, VTONT,
};
use crate::ntfs::ntfs_subr::{
    Rdata, ntfs_frele, ntfs_isnamepermitted, ntfs_ntget, ntfs_ntlookupfile, ntfs_ntput,
    ntfs_ntreaddir, ntfs_nttimetounix, ntfs_ntvattrget, ntfs_ntvattrrele, ntfs_readattr,
};
use crate::sys::buf::{B_ERROR, B_READ, clrbuf};
use crate::sys::dirent::{DT_DIR, DT_REG, Dirent, MAXNAMLEN};
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::mount::VFS_VGET;
use crate::sys::namei::{DELETE, ISDOTDOT, ISLASTCN, LOCKPARENT, MAKEENTRY, PDIRUNLOCK, RENAME};
use crate::sys::stat::{
    S_IRGRP, S_IROTH, S_IRUSR, S_IWGRP, S_IWOTH, S_IWUSR, S_IXGRP, S_IXOTH, S_IXUSR,
};
use crate::sys::types::{Ino, Mode, Nlink, Off, Register};
use crate::sys::ucred::Ucred;
use crate::sys::unistd::{_PC_CHOWN_RESTRICTED, _PC_LINK_MAX, _PC_NAME_MAX, _PC_NO_TRUNC};
use crate::sys::vnode::{
    VDIR, VEXEC, VLNK, VREAD, VREG, VWRITE, VopAccessArgs, VopBmapArgs, VopCloseArgs, VopFsyncArgs,
    VopGetattrArgs, VopInactiveArgs, VopLookupArgs, VopOpenArgs, VopPathconfArgs, VopPrintArgs,
    VopReadArgs, VopReaddirArgs, VopReclaimArgs, VopStrategyArgs, Vops, cred_ref,
};

/// `ntfs_prtactive`: 1 => print out reclaim of active vnodes.
pub static NTFS_PRTACTIVE: AtomicI32 = AtomicI32::new(0);

/// `ntfs_vops`: global vfs data structures.
pub static NTFS_VOPS: Vops = Vops {
    vop_getattr: Some(ntfs_getattr),
    vop_inactive: Some(ntfs_inactive),
    vop_reclaim: Some(ntfs_reclaim),
    vop_print: Some(ntfs_print),
    vop_pathconf: Some(ntfs_pathconf),
    vop_lock: Some(|_| nullop()),
    vop_unlock: Some(|_| nullop()),
    vop_islocked: Some(|_| 0),
    vop_lookup: Some(ntfs_lookup),
    vop_access: Some(ntfs_access),
    vop_close: Some(ntfs_close),
    vop_open: Some(ntfs_open),
    vop_readdir: Some(ntfs_readdir),
    vop_fsync: Some(ntfs_fsync),
    vop_bmap: Some(ntfs_bmap),
    vop_strategy: Some(ntfs_strategy),
    vop_bwrite: Some(vop_generic_bwrite),
    vop_read: Some(ntfs_read),

    vop_abortop: None,
    vop_advlock: None,
    vop_create: None,
    vop_ioctl: None,
    vop_link: None,
    vop_mknod: None,
    vop_readlink: None,
    vop_remove: Some(|_| eopnotsupp()),
    vop_rename: None,
    vop_revoke: None,
    vop_mkdir: None,
    vop_rmdir: None,
    vop_setattr: None,
    vop_symlink: None,
    vop_write: None,
    vop_kqfilter: None,
};

/// The credentials a vnode operation was handed.
fn ucred<'a>(cred: *const Ucred) -> &'a Ucred {
    // SAFETY: the credentials a vnode operation receives are held by its caller for the
    // operation's duration (`cred_ref`'s contract).
    match unsafe { cred_ref(cred) } {
        Some(c) => c,
        None => panic(format_args!(
            "ntfs: credential {:p} is not a real one",
            cred
        )),
    }
}

/// `ntfs_bmap` (`vop_bmap`): this is a noop, simply returning what one has been given.
pub fn ntfs_bmap(ap: &mut VopBmapArgs<'_>) -> Result<(), Errno> {
    if let Some(vpp) = ap.a_vpp.as_deref_mut() {
        *vpp = Some(ap.a_vp);
    }
    if let Some(bnp) = ap.a_bnp.as_deref_mut() {
        *bnp = ap.a_bn;
    }
    if let Some(runp) = ap.a_runp.as_deref_mut() {
        *runp = 0;
    }
    Ok(())
}

/// `ntfs_read` (`vop_read`).
pub fn ntfs_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let fp = VTOF(vp);
    let ip = FTONT(fp);
    let uio = &mut *ap.a_uio;
    let ntmp = ip.mp();

    // don't allow reading after end of file
    let f_size = fp.f_size.get();
    let off = uio.uio_offset as u64;
    let toread = if off > f_size {
        0
    } else {
        (uio.uio_resid as u64).min(f_size - off)
    };

    if toread == 0 {
        return Ok(());
    }

    let attrname = fp.attrname();
    if let Err(e) = ntfs_readattr(
        ntmp,
        ip,
        fp.f_attrtype.get(),
        attrname,
        uio.uio_offset,
        toread as usize,
        &mut Rdata::Uio(uio),
    ) {
        printf(format_args!(
            "ntfs_read: ntfs_readattr failed: {}\n",
            e as i32
        ));
        return Err(e);
    }

    Ok(())
}

/// `ntfs_getattr` (`vop_getattr`).
pub fn ntfs_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let fp = VTOF(vp);
    let ip = FTONT(fp);
    let ntmp = ip.mp();
    let vap = &mut *ap.a_vap;
    let times = fp.f_times.get();

    vap.va_fsid = i64::from(ip.i_dev.get());
    vap.va_fileid = u64::from(ip.i_number.get());
    vap.va_mode = ntmp.ntm_mode.get();
    vap.va_nlink = ip.i_nlink.get() as Nlink;
    vap.va_uid = ntmp.ntm_uid.get();
    vap.va_gid = ntmp.ntm_gid.get();
    vap.va_rdev = 0; // XXX UNODEV ?
    vap.va_size = fp.f_size.get();
    vap.va_bytes = fp.f_allocated.get();
    vap.va_atime = ntfs_nttimetounix(times.t_access);
    vap.va_mtime = ntfs_nttimetounix(times.t_write);
    vap.va_ctime = ntfs_nttimetounix(times.t_create);
    vap.va_flags = u64::from(ip.i_flag.get());
    vap.va_gen = 0;
    vap.va_blocksize = i64::from(ntmp.ntm_spc()) * i64::from(ntmp.ntm_bps());
    vap.va_type = vp.v_type.get();
    vap.va_filerev = 0;

    // Ensure that a directory link count is always 1 so that things like fts_read() do not
    // try to be smart and end up skipping over directories. Additionally, ip->i_nlink will
    // not be initialised until the ntnode has been loaded for the file.
    if vp.v_type.get() == VDIR || ip.i_nlink.get() < 1 {
        vap.va_nlink = 1;
    }

    Ok(())
}

/// `ntfs_inactive` (`vop_inactive`): last reference to an ntnode. If necessary, write or
/// delete it.
pub fn ntfs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    #[cfg(feature = "diagnostic")]
    if NTFS_PRTACTIVE.load(core::sync::atomic::Ordering::Relaxed) != 0 && vp.v_usecount.get() != 0 {
        crate::kern::vfs_subr::vprint(Some("ntfs_inactive: pushing active"), vp);
    }

    let _ = VOP_UNLOCK(vp);

    // XXX since we don't support any filesystem changes right now, nothing more needs to be
    // done
    Ok(())
}

/// `ntfs_reclaim` (`vop_reclaim`): reclaim an fnode/ntnode so that it can be used for other
/// purposes.
pub fn ntfs_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let fp = VTOF(vp);
    let ip = FTONT(fp);

    #[cfg(feature = "diagnostic")]
    if NTFS_PRTACTIVE.load(core::sync::atomic::Ordering::Relaxed) != 0 && vp.v_usecount.get() != 0 {
        crate::kern::vfs_subr::vprint(Some("ntfs_reclaim: pushing active"), vp);
    }

    ntfs_ntget(ip)?;

    // Purge old data structures associated with the inode.
    cache_purge(vp);

    ntfs_frele(fp);
    ntfs_ntput(ip);

    vp.v_data.set(ptr::null_mut());

    Ok(())
}

/// `ntfs_print` (`vop_print`).
pub fn ntfs_print(ap: &mut VopPrintArgs) -> Result<(), Errno> {
    #[cfg(any(feature = "debug", feature = "diagnostic"))]
    {
        let ip = VTONT(ap.a_vp);

        printf(format_args!(
            "tag VT_NTFS, ino {}, flag {:#x}, usecount {}, nlink {}\n",
            ip.i_number.get(),
            ip.i_flag.get(),
            ip.i_usecount.get(),
            ip.i_nlink.get()
        ));
    }
    #[cfg(not(any(feature = "debug", feature = "diagnostic")))]
    let _ = ap;

    Ok(())
}

/// `ntfs_strategy` (`vop_strategy`): calculate the logical to physical mapping if not done
/// already, then call the device strategy routine. Here: read the attribute's bytes at
/// `ntfs_cntob(b_blkno)` into the buffer, zero past the end of the file.
pub fn ntfs_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    let bp = ap.a_bp;
    let Some(vp) = bp.b_vp.get() else {
        panic(format_args!("ntfs_strategy: buffer without a vnode"));
    };
    let fp = VTOF(vp);
    let ip = FTONT(fp);
    let ntmp = ip.mp();
    let mut error = Ok(());

    if bp.isset(B_READ) {
        let boff = ntmp.ntfs_cntob(bp.b_blkno.get() as u64);
        if boff as u64 >= fp.f_size.get() {
            // SAFETY: a buffer handed to the strategy routine is busy for this I/O and
            // mapped; the strategy owns it until biodone.
            unsafe { clrbuf(bp) };
        } else {
            let bcount = bp.b_bcount.get() as u64;
            let toread = bcount.min(fp.f_size.get() - boff as u64) as u32;

            // SAFETY: as above; the slice dies before biodone.
            let data = unsafe { bp.data() };
            let attrname = fp.attrname();
            if let Err(e) = ntfs_readattr(
                ntmp,
                ip,
                fp.f_attrtype.get(),
                attrname,
                boff,
                toread as usize,
                &mut Rdata::Mem(&mut *data),
            ) {
                printf(format_args!("ntfs_strategy: ntfs_readattr failed\n"));
                bp.b_error.set(Some(e));
                bp.set(B_ERROR);
                error = Err(e);
            }

            if let Some(tail) = data.get_mut(toread as usize..) {
                tail.fill(0);
            }
        }
    } else {
        bp.b_error.set(Some(Errno::EROFS));
        bp.set(B_ERROR);
        error = Err(Errno::EROFS);
    }
    let s = splbio();
    biodone(bp);
    splx(s);
    error
}

/// `ntfs_access` (`vop_access`).
pub fn ntfs_access(ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = VTONT(vp);
    let ntmp = ip.mp();
    let cred = ucred(ap.a_cred);
    let mode = ap.a_mode;

    // Disallow write attempts unless the file is a socket, fifo, or a block or character
    // device resident on the file system.
    if mode & VWRITE != 0 {
        match vp.v_type.get() {
            VDIR | VLNK | VREG => return Err(Errno::EROFS),
            _ => {}
        }
    }

    // Otherwise, user id 0 always gets access.
    if cred.cr_uid.get() == 0 {
        return Ok(());
    }

    let mut mask: Mode = 0;
    let verdict = |mask: Mode| {
        if ntmp.ntm_mode.get() & mask == mask {
            Ok(())
        } else {
            Err(Errno::EACCES)
        }
    };

    // Otherwise, check the owner.
    if cred.cr_uid.get() == ntmp.ntm_uid.get() {
        if mode & VEXEC != 0 {
            mask |= S_IXUSR;
        }
        if mode & VREAD != 0 {
            mask |= S_IRUSR;
        }
        if mode & VWRITE != 0 {
            mask |= S_IWUSR;
        }
        return verdict(mask);
    }

    // Otherwise, check the groups.
    let ngroups = usize::try_from(cred.cr_ngroups.get()).unwrap_or(0);
    for gp in cred.cr_groups.iter().take(ngroups) {
        if ntmp.ntm_gid.get() == gp.get() {
            if mode & VEXEC != 0 {
                mask |= S_IXGRP;
            }
            if mode & VREAD != 0 {
                mask |= S_IRGRP;
            }
            if mode & VWRITE != 0 {
                mask |= S_IWGRP;
            }
            return verdict(mask);
        }
    }

    // Otherwise, check everyone else.
    if mode & VEXEC != 0 {
        mask |= S_IXOTH;
    }
    if mode & VREAD != 0 {
        mask |= S_IROTH;
    }
    if mode & VWRITE != 0 {
        mask |= S_IWOTH;
    }
    verdict(mask)
}

/// `ntfs_open` (`vop_open`): nothing to do. Files marked append-only must be opened for
/// appending.
pub fn ntfs_open(_ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    Ok(())
}

/// `ntfs_close` (`vop_close`): update the times on the inode (nothing to do).
pub fn ntfs_close(_ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    Ok(())
}

/// `sizeof(struct dirent)`.
const DIRENT_SIZE: usize = size_of::<Dirent>();

/// The bytes of a whole `struct dirent`, as `uiomove(&cde, sizeof(struct dirent), uio)`
/// copies them.
fn dirent_bytes(dp: &Dirent) -> [u8; DIRENT_SIZE] {
    let mut b = [0u8; DIRENT_SIZE];
    b[0..8].copy_from_slice(&dp.d_fileno.to_ne_bytes());
    b[8..16].copy_from_slice(&dp.d_off.to_ne_bytes());
    b[16..18].copy_from_slice(&dp.d_reclen.to_ne_bytes());
    b[18] = dp.d_type;
    b[19] = dp.d_namlen;
    b[Dirent::NAME_OFFSET..].copy_from_slice(&dp.d_name);
    b
}

/// `ntfs_readdir` (`vop_readdir`): every entry is a whole `struct dirent`, its offset in the
/// directory a multiple of the structure's size; `.` (except in the root) and `..` come
/// first.
pub fn ntfs_readdir(ap: &mut VopReaddirArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let fp = VTOF(vp);
    let ip = FTONT(fp);
    let uio = &mut *ap.a_uio;
    let ntmp = ip.mp();

    let mut cde = Dirent {
        d_fileno: 0,
        d_off: 0,
        d_reclen: 0,
        d_type: 0,
        d_namlen: 0,
        __d_padding: [0; 4],
        d_name: [0; MAXNAMLEN + 1],
    };

    let error: Result<(), Errno> = 'out: {
        // Simulate . in every dir except ROOT
        if ip.i_number.get() != NTFS_ROOTINO && uio.uio_offset == 0 {
            cde.d_fileno = Ino::from(ip.i_number.get());
            cde.d_reclen = DIRENT_SIZE as u16;
            cde.d_type = DT_DIR;
            cde.d_namlen = 1;
            cde.d_off = DIRENT_SIZE as Off;
            cde.d_name[0] = b'.';
            cde.d_name[1] = 0;
            if let Err(e) = uiomove(&mut dirent_bytes(&cde), uio) {
                break 'out Err(e);
            }
        }

        // Simulate .. in every dir including ROOT
        if (uio.uio_offset as u64) < 2 * DIRENT_SIZE as u64 {
            cde.d_fileno = Ino::from(NTFS_ROOTINO); // XXX
            cde.d_reclen = DIRENT_SIZE as u16;
            cde.d_type = DT_DIR;
            cde.d_namlen = 2;
            cde.d_off = 2 * DIRENT_SIZE as Off;
            cde.d_name[0] = b'.';
            cde.d_name[1] = b'.';
            cde.d_name[2] = 0;
            if let Err(e) = uiomove(&mut dirent_bytes(&cde), uio) {
                break 'out Err(e);
            }
        }

        let faked: u32 = if ip.i_number.get() == NTFS_ROOTINO {
            1
        } else {
            2
        };
        let mut num = ((uio.uio_offset as u64 / DIRENT_SIZE as u64) as u32).wrapping_sub(faked);

        while uio.uio_resid >= DIRENT_SIZE {
            let aoff = match ntfs_ntreaddir(ntmp, fp, num, uio.uio_procp) {
                Ok(Some(aoff)) => aoff,
                Ok(None) => break,
                Err(e) => break 'out Err(e),
            };

            let mut aoff = aoff;
            loop {
                // SAFETY: the buffer belongs to this fnode and only this vnode operation
                // uses it between `ntfs_ntreaddir` calls; the copy below is the only borrow.
                let iep = AttrIndexentry::read(unsafe { fp.dirblbuf() }, aoff);
                if iep.ie_flag & NTFS_IEFLAG_LAST != 0 || uio.uio_resid < DIRENT_SIZE {
                    break;
                }

                if ntfs_isnamepermitted(ntmp, &iep) {
                    let mut pos = 0usize;
                    let fname = iep.ie_fname;
                    for &wc in fname.iter().take(usize::from(iep.ie_fnamelen)) {
                        let sz = ntmp.wput(&mut cde.d_name[pos..NTFS_MAXFILENAME], wc);
                        pos += sz;
                    }
                    cde.d_name[pos] = 0;
                    cde.d_namlen = pos as u8;
                    if cde.d_name[..pos].contains(&b'/') {
                        break 'out Err(Errno::EINVAL);
                    }
                    cde.d_fileno = Ino::from(iep.ie_number);
                    cde.d_type = if iep.ie_fflag & NTFS_FFLAG_DIR != 0 {
                        DT_DIR
                    } else {
                        DT_REG
                    };
                    cde.d_reclen = DIRENT_SIZE as u16;
                    cde.d_off = uio.uio_offset + DIRENT_SIZE as Off;

                    if let Err(e) = uiomove(&mut dirent_bytes(&cde), uio) {
                        break 'out Err(e);
                    }
                    num = num.wrapping_add(1);
                }

                // NTFS_NEXTREC (an entry of no length ends the walk: the module's
                // deviations)
                if iep.reclen == 0 {
                    break;
                }
                aoff += usize::from(iep.reclen);
            }
        }

        Ok(())
    };

    // out:
    if let Some(b) = fp.f_dirblbuf.take() {
        crate::kern::kern_malloc::free(b, crate::sys::malloc::M_NTFSDIR, fp.f_dirblbuf_len.get());
    }
    error
}

/// `ntfs_lookup` (`vop_lookup`).
pub fn ntfs_lookup(ap: &mut VopLookupArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let dip = VTONT(dvp);
    let ntmp = dip.mp();
    let cnp = &mut *ap.a_cnp;
    let lockparent = cnp.cn_flags & LOCKPARENT != 0;

    VOP_ACCESS(dvp, VEXEC, cnp.cn_cred, cnp.proc())?;

    if cnp.cn_flags & ISLASTCN != 0 && (cnp.cn_nameiop == DELETE || cnp.cn_nameiop == RENAME) {
        return Err(Errno::EROFS);
    }

    // We now have a segment name to search for, and a directory to search.
    //
    // Before tediously performing a linear scan of the directory, check the name cache to
    // see if the directory/name pair we are looking for is known already.
    if let Some(vp) = cache_lookup(dvp, cnp)? {
        *ap.a_vpp = Some(vp);
        return Ok(());
    }

    let name = cnp.name();
    if name.len() == 1 && name[0] == b'.' {
        vref(dvp);
        *ap.a_vpp = Some(dvp);
    } else if cnp.cn_flags & ISDOTDOT != 0 {
        let _ = VOP_UNLOCK(dvp);
        cnp.cn_flags |= PDIRUNLOCK;

        let vap = ntfs_ntvattrget(ntmp, dip, NTFS_A_NAME, None, 0)?;

        let pnumber = vap.va_a_name().n_pnumber;
        let res = VFS_VGET(ntmp.mountp(), Ino::from(pnumber));
        ntfs_ntvattrrele(vap);
        match res {
            Ok(vp) => *ap.a_vpp = Some(vp),
            Err(e) => {
                if vn_lock(dvp, LK_EXCLUSIVE | LK_RETRY).is_ok() {
                    cnp.cn_flags &= !PDIRUNLOCK;
                }
                return Err(e);
            }
        }

        if lockparent && cnp.cn_flags & ISLASTCN != 0 {
            if let Err(e) = vn_lock(dvp, LK_EXCLUSIVE) {
                if let Some(vp) = *ap.a_vpp {
                    vput(vp);
                }
                return Err(e);
            }
            cnp.cn_flags &= !PDIRUNLOCK;
        }
    } else {
        ntfs_ntlookupfile(ntmp, dvp, cnp, ap.a_vpp)?;

        if !lockparent || cnp.cn_flags & ISLASTCN == 0 {
            let _ = VOP_UNLOCK(dvp);
            cnp.cn_flags |= PDIRUNLOCK;
        }
    }

    if cnp.cn_flags & MAKEENTRY != 0 {
        cache_enter(dvp, *ap.a_vpp, cnp);
    }

    Ok(())
}

/// `ntfs_fsync` (`vop_fsync`): flush the blocks of a file to disk.
///
/// This function is worthless for vnodes that represent directories. Maybe we could just do
/// a sync if they try an fsync on a directory file.
pub fn ntfs_fsync(_ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    Ok(())
}

/// `ntfs_pathconf` (`vop_pathconf`): return POSIX pathconf information applicable to NTFS
/// filesystem.
pub fn ntfs_pathconf(ap: &mut VopPathconfArgs<'_>) -> Result<(), Errno> {
    *ap.a_retval = match ap.a_name {
        _PC_LINK_MAX => 1,
        _PC_NAME_MAX => NTFS_MAXFILENAME as Register,
        _PC_CHOWN_RESTRICTED => 1,
        _PC_NO_TRUNC => 0,
        _ => return Err(Errno::EINVAL),
    };

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for NTFS over a small volume built in memory: a boot file, an MFT of 24 one-KB
    // records with update sequence fixups (`$MFT`, `$AttrDef`, the root, `$Bitmap`, `$UpCase`
    // and nine files), a resident file, a non-resident file with a hole, an LZNT1-compressed
    // file (a compressed unit, a stored unit and a sparse one), a directory whose names live in
    // an `$INDEX_ALLOCATION` buffer (found through `$BITMAP:$I30`), a file whose `$DATA` lives in
    // an extension record named by its `$ATTRIBUTE_LIST`, and a DOS name. A block device vnode
    // whose strategy reads the image stands for the disk. The tests mount the volume with
    // `ntfs_mountfs`, look names up, read files (by `VOP_READ` and through the buffer cache's
    // `ntfs_strategy`), read directories, take attributes and `statfs`, and unmount.

    use core::sync::atomic::Ordering;
    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq, vec};

    use super::*;
    use crate::kern::vfs_bio::{BCSTATS, BUFHEAD, BUFKVM, CLEANCACHE, bread, brelse, bufinit};
    use crate::kern::vfs_init::vfs_byname;
    use crate::kern::vfs_subr::{bdevvp, vflushbuf, vfs_mount_alloc, vfs_mount_free, vrele};
    use crate::kern::vfs_vops::{VOP_GETATTR, VOP_LOOKUP, VOP_READ, VOP_READDIR};
    use crate::machine::Machine;
    use crate::machine::cpu::Cpu;
    use crate::ntfs::ntfs::{NTFS_MFTINO, VFSTONTFS};
    use crate::ntfs::ntfs_compr::NTFS_COMPBLOCK_SIZE;
    use crate::ntfs::ntfs_compr::tests::{compress_block, sample};
    use crate::ntfs::ntfs_ihash::{ntfs_nthash_reset, ntfs_nthashinit, ntfs_nthashlookup};
    use crate::ntfs::ntfs_subr::ntfs_toupper_reset;
    use crate::ntfs::ntfs_vfsops::{ntfs_mountfs, ntfs_root, ntfs_statfs, ntfs_unmount};
    use crate::ntfs::ntfsmount::{NTFS_MFLAG_ALLNAMES, NTFS_MFLAG_CASEINS};
    use crate::sys::buf::Buf;
    use crate::sys::mount::{MNT_LOCAL, MNT_RDONLY, MNT_WAIT, Mount, NtfsArgs};
    use crate::sys::namei::{Componentname, LOOKUP};
    use crate::sys::param::DEV_BSIZE;
    use crate::sys::proc::Proc;
    use crate::sys::types::makedev;
    use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
    use crate::sys::vnode::{VROOT, Vattr, Vnode, VopFsyncArgs, VopInactiveArgs};

    /// The cluster size: two 512-byte sectors.
    const CL: usize = 1024;
    /// The volume's clusters.
    const NCL: usize = 256;
    /// The MFT: 24 records of one cluster from cluster 4.
    const MFTCN: usize = 4;
    const NREC: usize = 24;
    /// `$UpCase`: 128 clusters from cluster 32.
    const UPCASE_CN: usize = 32;
    /// `big.bin`: two clusters at 160, a hole, one cluster at 170.
    const BIG_CN1: usize = 160;
    const BIG_CN2: usize = 170;
    const BIG_LEN: usize = 3500;
    /// `comp.bin`: the compressed unit from 180, the stored unit at 190..206.
    const COMP_CN0: usize = 180;
    const COMP_CN1: usize = 190;
    const UNIT: usize = 16 * CL;
    const COMP_LEN: usize = 2 * UNIT + 500;
    /// `sub`'s index buffer.
    const SUBIDX_CN: usize = 210;
    /// The hello.txt times (1 000 000 000 s and 1234 * 100 ns after 1970).
    const HELLO_T: u64 = 116_444_736_000_000_000 + 10_000_000 * 1_000_000_000 + 1234;

    const HELLO: &[u8] = b"Hello, NTFS!\n";

    /// `big.bin`'s bytes: a pattern, zeros in its third cluster (a hole).
    fn big() -> Vec<u8> {
        (0..BIG_LEN)
            .map(|i| {
                if (2048..3072).contains(&i) {
                    0
                } else {
                    (i * 31 + 7) as u8
                }
            })
            .collect()
    }

    /// `comp.bin`'s bytes: text (compressed on disk), a pattern (stored), zeros (sparse).
    fn comp() -> Vec<u8> {
        let mut v = sample(UNIT, 77);
        v.extend((0..UNIT).map(|i| (i * 13 + 1) as u8));
        v.resize(COMP_LEN, 0);
        v
    }

    fn align8(n: usize) -> usize {
        (n + 7) & !7
    }

    fn put16(b: &mut [u8], o: usize, v: u16) {
        b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn put32(b: &mut [u8], o: usize, v: u32) {
        b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn put64(b: &mut [u8], o: usize, v: u64) {
        b[o..o + 8].copy_from_slice(&v.to_le_bytes());
    }

    fn wide(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// A resident attribute record.
    fn resident(t: u32, name: &str, value: &[u8]) -> Vec<u8> {
        let n = wide(name);
        let dataoff = align8(24 + n.len());
        let reclen = align8(dataoff + value.len());
        let mut a = vec![0u8; reclen];
        put32(&mut a, 0, t);
        put32(&mut a, 4, reclen as u32);
        a[9] = (n.len() / 2) as u8;
        a[10] = 24;
        put32(&mut a, 16, value.len() as u32);
        put16(&mut a, 20, dataoff as u16);
        a[24..24 + n.len()].copy_from_slice(&n);
        a[dataoff..dataoff + value.len()].copy_from_slice(value);
        a
    }

    /// A run list: (clusters, cluster number or a hole).
    fn encode_runs(runs: &[(u64, Option<u64>)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut prev: i64 = 0;
        for &(len, lcn) in runs {
            let lb: Vec<u8> = len
                .to_le_bytes()
                .into_iter()
                .take(8 - len.leading_zeros() as usize / 8)
                .collect();
            let ob: Vec<u8> = match lcn {
                None => Vec::new(),
                Some(lcn) => {
                    let d = lcn as i64 - prev;
                    prev = lcn as i64;
                    let mut n = 1;
                    while n < 8 && !(-(1i64 << (8 * n - 1))..(1i64 << (8 * n - 1))).contains(&d) {
                        n += 1;
                    }
                    d.to_le_bytes()[..n].to_vec()
                }
            };
            out.push(((ob.len() as u8) << 4) | lb.len() as u8);
            out.extend_from_slice(&lb);
            out.extend_from_slice(&ob);
        }
        out.push(0);
        out
    }

    /// A non-resident attribute record.
    fn nonresident(
        t: u32,
        name: &str,
        runs: &[(u64, Option<u64>)],
        datalen: u64,
        compressed: bool,
    ) -> Vec<u8> {
        let n = wide(name);
        let r = encode_runs(runs);
        let clusters: u64 = runs.iter().map(|r| r.0).sum();
        let runoff = align8(64 + n.len());
        let reclen = align8(runoff + r.len());
        let mut a = vec![0u8; reclen];
        put32(&mut a, 0, t);
        put32(&mut a, 4, reclen as u32);
        a[8] = 1;
        a[9] = (n.len() / 2) as u8;
        a[10] = 64;
        a[12] = u8::from(compressed);
        put64(&mut a, 16, 0);
        put64(&mut a, 24, clusters - 1);
        put16(&mut a, 32, runoff as u16);
        put16(&mut a, 34, if compressed { 4 } else { 0 });
        put64(&mut a, 40, clusters * CL as u64);
        put64(&mut a, 48, datalen);
        put64(&mut a, 56, datalen);
        a[64..64 + n.len()].copy_from_slice(&n);
        a[runoff..runoff + r.len()].copy_from_slice(&r);
        a
    }

    /// Apply the update sequence: number `usn` at the end of each sector, the bytes it covers
    /// saved in the array at `foff`.
    fn protect(b: &mut [u8], foff: usize, usn: u16) {
        put16(b, foff, usn);
        for (i, s) in (0..b.len() / 512).enumerate() {
            let end = s * 512 + 510;
            let orig = u16::from_le_bytes([b[end], b[end + 1]]);
            put16(b, foff + 2 + 2 * i, orig);
            put16(b, end, usn);
        }
    }

    /// An MFT record of `attrs`, protected.
    fn record(flags: u16, nlink: u16, mainrec: u64, attrs: &[Vec<u8>]) -> Vec<u8> {
        let mut r = vec![0u8; CL];
        r[0..4].copy_from_slice(b"FILE");
        put16(&mut r, 4, 0x30);
        put16(&mut r, 6, 3);
        put16(&mut r, 16, 1);
        put16(&mut r, 18, nlink);
        put16(&mut r, 20, 0x38);
        put16(&mut r, 22, flags);
        let mut off = 0x38;
        for a in attrs {
            r[off..off + a.len()].copy_from_slice(a);
            off += a.len();
        }
        put32(&mut r, off, u32::MAX);
        put32(&mut r, 24, (off + 8) as u32);
        put32(&mut r, 28, CL as u32);
        put64(&mut r, 32, mainrec);
        protect(&mut r, 0x30, 0x0101);
        r
    }

    /// The `$FILE_NAME` attribute of a name in `parent`.
    fn file_name(parent: u32, name: &str) -> Vec<u8> {
        let n = wide(name);
        let mut v = vec![0u8; 66 + n.len()];
        put32(&mut v, 0, parent);
        v[64] = (n.len() / 2) as u8;
        v[65] = 1;
        v[66..].copy_from_slice(&n);
        resident(0x30, "", &v)
    }

    /// An index entry of a name.
    fn entry(ino: u32, parent: u32, name: &str, nametype: u8, size: u64, fflag: u64) -> Vec<u8> {
        let n = wide(name);
        let reclen = align8(82 + n.len());
        let mut e = vec![0u8; reclen];
        put32(&mut e, 0, ino);
        put16(&mut e, 8, reclen as u16);
        put16(&mut e, 10, (66 + n.len()) as u16);
        put32(&mut e, 16, parent);
        for k in 0..4 {
            put64(&mut e, 24 + 8 * k, HELLO_T);
        }
        put64(&mut e, 56, size.next_multiple_of(CL as u64));
        put64(&mut e, 64, size);
        put64(&mut e, 72, fflag);
        e[80] = (n.len() / 2) as u8;
        e[81] = nametype;
        e[82..82 + n.len()].copy_from_slice(&n);
        e
    }

    /// The last entry of a node, with the VCN of its subnode.
    fn last(subnode: Option<u64>) -> Vec<u8> {
        let mut e = vec![0u8; if subnode.is_some() { 24 } else { 16 }];
        let n = e.len() as u16;
        put16(&mut e, 8, n);
        put32(&mut e, 12, 2 | u32::from(subnode.is_some()));
        if let Some(vcn) = subnode {
            put64(&mut e, 16, vcn);
        }
        e
    }

    /// An `$INDEX_ROOT:$I30` value over `entries`.
    fn index_root(flags: u16, entries: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = entries.concat();
        let mut v = vec![0u8; 32];
        put32(&mut v, 0, 0x30);
        put32(&mut v, 4, 1);
        put32(&mut v, 8, CL as u32);
        put32(&mut v, 12, 1);
        put32(&mut v, 16, 0x10);
        put32(&mut v, 20, (body.len() + 16) as u32);
        put32(&mut v, 24, (body.len() + 16) as u32);
        put16(&mut v, 28, flags);
        v.extend_from_slice(&body);
        v
    }

    /// An `$INDEX_ALLOCATION` buffer over `entries`, protected.
    fn index_block(entries: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = entries.concat();
        let mut b = vec![0u8; CL];
        b[0..4].copy_from_slice(b"INDX");
        put16(&mut b, 4, 0x28);
        put16(&mut b, 6, 3);
        put16(&mut b, 24, 0x28);
        put32(&mut b, 28, (0x28 + body.len()) as u32);
        put32(&mut b, 32, (CL - 0x18) as u32);
        b[0x40..0x40 + body.len()].copy_from_slice(&body);
        protect(&mut b, 0x28, 0x0202);
        b
    }

    /// The `$AttrDef` records: three definitions and the empty one that ends them.
    fn attrdef() -> Vec<u8> {
        let mut v = vec![0u8; 4 * 160];
        for (i, (name, t)) in [
            ("$STANDARD_INFORMATION", 0x10),
            ("$DATA", 0x80),
            ("$INDEX_ROOT", 0x90),
        ]
        .into_iter()
        .enumerate()
        {
            let n = wide(name);
            v[i * 160..i * 160 + n.len()].copy_from_slice(&n);
            put32(&mut v, i * 160 + 128, t);
        }
        v
    }

    /// The volume and the number of free clusters its bitmap records.
    fn image() -> (Vec<u8>, u64) {
        let mut img = vec![0u8; NCL * CL];
        let mut used = [false; NCL];
        let mut mark =
            |from: usize, n: usize| used[from..from + n].iter_mut().for_each(|u| *u = true);
        let at = |img: &mut Vec<u8>, cn: usize, b: &[u8]| {
            img[cn * CL..cn * CL + b.len()].copy_from_slice(b)
        };

        // The boot file.
        img[3..11].copy_from_slice(b"NTFS    ");
        put16(&mut img, 11, 512);
        img[13] = 2;
        img[21] = 0xF8;
        put64(&mut img, 40, (NCL * 2) as u64);
        put64(&mut img, 48, MFTCN as u64);
        put64(&mut img, 56, 8);
        img[64] = 0xF6;
        mark(0, 4);

        // $UpCase: the first 256 entries upper-case ASCII, the rest map to themselves.
        let upcase: Vec<u8> = (0..=0xFFFFu16)
            .map(|c| {
                if (0x61..=0x7a).contains(&c) {
                    c - 0x20
                } else {
                    c
                }
            })
            .flat_map(u16::to_le_bytes)
            .collect();
        at(&mut img, UPCASE_CN, &upcase);
        mark(UPCASE_CN, 128);

        // big.bin's clusters.
        let b = big();
        let mut on_disk = b.clone();
        on_disk.resize(4 * CL, 0x5A);
        at(&mut img, BIG_CN1, &on_disk[..2 * CL]);
        at(&mut img, BIG_CN2, &on_disk[3 * CL..]);
        mark(BIG_CN1, 2);
        mark(BIG_CN2, 1);

        // comp.bin: unit 0 compressed block by block, unit 1 stored.
        let c = comp();
        let mut unit0 = Vec::new();
        for chunk in c[..UNIT].chunks(NTFS_COMPBLOCK_SIZE) {
            unit0.extend_from_slice(&compress_block(chunk));
        }
        let k = unit0.len().div_ceil(CL);
        assert!(
            k < 16 && COMP_CN0 + k <= COMP_CN1,
            "unit 0 compresses to {} bytes",
            unit0.len()
        );
        at(&mut img, COMP_CN0, &unit0);
        at(&mut img, COMP_CN1, &c[UNIT..2 * UNIT]);
        mark(COMP_CN0, k);
        mark(COMP_CN1, 16);

        // sub's index buffer.
        let idx = index_block(&[
            entry(20, 18, "a.txt", 1, 6, 0),
            entry(21, 18, "b.txt", 1, 6, 0),
            last(None),
        ]);
        at(&mut img, SUBIDX_CN, &idx);
        mark(SUBIDX_CN, 1);

        // The MFT.
        mark(MFTCN, NREC);
        let mut recs: Vec<Vec<u8>> = vec![Vec::new(); NREC];
        recs[0] = record(
            1,
            1,
            0,
            &[nonresident(
                0x80,
                "",
                &[(NREC as u64, Some(MFTCN as u64))],
                (NREC * CL) as u64,
                false,
            )],
        );
        recs[4] = record(1, 1, 0, &[resident(0x80, "", &attrdef())]);
        let fdir = 0x1000_0000;
        recs[5] = record(
            3,
            1,
            0,
            &[
                file_name(5, "."),
                resident(
                    0x90,
                    "$I30",
                    &index_root(
                        0,
                        &[
                            entry(17, 5, "big.bin", 1, BIG_LEN as u64, 0),
                            entry(17, 5, "BIG~1.BIN", 2, BIG_LEN as u64, 0),
                            entry(19, 5, "comp.bin", 1, COMP_LEN as u64, 0x800),
                            entry(22, 5, "frag.txt", 1, 14, 0),
                            entry(16, 5, "hello.txt", 1, HELLO.len() as u64, 0),
                            entry(18, 5, "sub", 1, 0, fdir),
                            last(None),
                        ],
                    ),
                ),
            ],
        );
        let mut bitmap = vec![0u8; NCL / 8];
        let mut free = 0;
        for (i, &u) in used.iter().enumerate() {
            if u {
                bitmap[i / 8] |= 1 << (i % 8);
            } else {
                free += 1;
            }
        }
        recs[6] = record(1, 1, 0, &[resident(0x80, "", &bitmap)]);
        recs[10] = record(
            1,
            1,
            0,
            &[nonresident(
                0x80,
                "",
                &[(128, Some(UPCASE_CN as u64))],
                131_072,
                false,
            )],
        );
        recs[16] = record(
            1,
            1,
            0,
            &[file_name(5, "hello.txt"), resident(0x80, "", HELLO)],
        );
        recs[17] = record(
            1,
            2,
            0,
            &[
                file_name(5, "big.bin"),
                nonresident(
                    0x80,
                    "",
                    &[
                        (2, Some(BIG_CN1 as u64)),
                        (1, None),
                        (1, Some(BIG_CN2 as u64)),
                    ],
                    BIG_LEN as u64,
                    false,
                ),
            ],
        );
        recs[18] = record(
            3,
            1,
            0,
            &[
                file_name(5, "sub"),
                resident(0x90, "$I30", &index_root(1, &[last(Some(0))])),
                nonresident(
                    0xA0,
                    "$I30",
                    &[(1, Some(SUBIDX_CN as u64))],
                    CL as u64,
                    false,
                ),
                resident(0xB0, "$I30", &[1, 0, 0, 0, 0, 0, 0, 0]),
            ],
        );
        recs[19] = record(
            1,
            1,
            0,
            &[
                file_name(5, "comp.bin"),
                nonresident(
                    0x80,
                    "",
                    &[
                        (k as u64, Some(COMP_CN0 as u64)),
                        (16 - k as u64, None),
                        (16, Some(COMP_CN1 as u64)),
                        (16, None),
                    ],
                    COMP_LEN as u64,
                    true,
                ),
            ],
        );
        recs[20] = record(
            1,
            1,
            0,
            &[file_name(18, "a.txt"), resident(0x80, "", b"alpha\n")],
        );
        recs[21] = record(
            1,
            1,
            0,
            &[file_name(18, "b.txt"), resident(0x80, "", b"bravo\n")],
        );
        let mut alist = vec![0u8; 64];
        for (i, (t, ino)) in [(0x30u32, 22u32), (0x80, 23)].into_iter().enumerate() {
            put32(&mut alist, i * 32, t);
            put16(&mut alist, i * 32 + 4, 32);
            alist[i * 32 + 7] = 0x1A;
            put32(&mut alist, i * 32 + 16, ino);
        }
        recs[22] = record(
            1,
            1,
            0,
            &[file_name(5, "frag.txt"), resident(0x20, "", &alist)],
        );
        recs[23] = record(1, 0, 22, &[resident(0x80, "", b"fragment data\n")]);
        for (i, r) in recs.iter().enumerate() {
            if !r.is_empty() {
                at(&mut img, MFTCN + i, r);
            }
        }
        (img, free)
    }

    /// The disk the strategy below reads.
    static DISK: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

    /// A synchronous read of the image at `b_blkno`, then `biodone`.
    fn disk_io(bp: &'static Buf) {
        let off = bp.b_blkno.get() as usize * DEV_BSIZE;
        let len = bp.b_bcount.get() as usize;
        {
            let d = DISK.lock().unwrap_or_else(|e| e.into_inner());
            if !bp.isset(B_READ) || off + len > d.len() {
                bp.b_error.set(Some(Errno::EIO));
                bp.set(B_ERROR);
            } else {
                // SAFETY: the buffer is busy for this transfer and mapped.
                let data = unsafe { bp.data() };
                data.copy_from_slice(&d[off..off + len]);
                bp.b_resid.set(0);
            }
        }
        let s = splbio();
        biodone(bp);
        splx(s);
    }

    /// The fake disk's block device vnode operations.
    static DISK_VOPS: Vops = Vops {
        vop_open: Some(|_| nullop()),
        vop_close: Some(|_| nullop()),
        vop_lock: Some(|_| nullop()),
        vop_unlock: Some(|_| nullop()),
        vop_islocked: Some(|_| 0),
        vop_inactive: Some(|ap: &mut VopInactiveArgs<'_>| VOP_UNLOCK(ap.a_vp)),
        vop_reclaim: Some(|_| nullop()),
        vop_strategy: Some(|ap| {
            disk_io(ap.a_bp);
            Ok(())
        }),
        vop_fsync: Some(|ap: &mut VopFsyncArgs<'_>| {
            vflushbuf(ap.a_vp, ap.a_waitfor == MNT_WAIT);
            Ok(())
        }),
        ..Vops::EMPTY
    };

    /// The fake disk's device number.
    const DISKDEV: i32 = makedev(17, 3);

    /// Memory, the vfs, a fresh buffer cache, the image as the disk, the thread as `curproc`, and
    /// no NTFS hash or upper-case table (their memory was just reset).
    fn setup(image: Vec<u8>) -> (MutexGuard<'static, ()>, &'static Proc) {
        let (g, p) = crate::kern::vfs_subr::tests::setup();
        Machine::set_curproc(Machine::curcpu(), p);
        BUFHEAD.0.init();
        for c in [
            &BCSTATS.numbufs,
            &BCSTATS.numbufpages,
            &BCSTATS.numdirtypages,
            &BCSTATS.numcleanpages,
            &BCSTATS.pendingwrites,
            &BCSTATS.pendingreads,
            &BCSTATS.numwrites,
            &BCSTATS.numreads,
            &BCSTATS.cachehits,
            &BCSTATS.busymapped,
            &BCSTATS.delwribufs,
        ] {
            c.store(0, Ordering::Relaxed);
        }
        CLEANCACHE.hotbufpages.set(0);
        CLEANCACHE.warmbufpages.set(0);
        CLEANCACHE.cachepages.set(0);
        BUFKVM.store(0, Ordering::Relaxed);
        crate::conf::param::bufpages.store(0, Ordering::Relaxed);
        bufinit();
        ntfs_nthash_reset();
        ntfs_toupper_reset();
        *DISK.lock().unwrap_or_else(|e| e.into_inner()) = image;
        (g, p)
    }

    fn teardown() {
        Machine::set_curproc(Machine::curcpu(), ptr::null());
    }

    /// The mount arguments: owner 1000, group 100, mode 0555, `flag`.
    fn args(flag: u64) -> NtfsArgs {
        let mut a = NtfsArgs::from_bytes(&[0u8; NtfsArgs::SIZE]).unwrap();
        a.uid = 1000;
        a.gid = 100;
        a.mode = 0o555;
        a.flag = flag;
        a
    }

    /// `ntfs_mountfs` of the disk on a fresh read-only mount, as `ntfs_mount` does after
    /// `ntfs_nthashinit`.
    fn mount(p: &'static Proc, flag: u64) -> Result<&'static Mount, Errno> {
        let devvp = bdevvp(DISKDEV).unwrap().unwrap();
        devvp.v_op.set(Some(&DISK_VOPS));
        let mp = vfs_mount_alloc(None, vfs_byname(b"ntfs").unwrap());
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
        ntfs_nthashinit();
        match ntfs_mountfs(devvp, mp, &args(flag), p) {
            Ok(()) => Ok(mp),
            Err(e) => {
                vrele(devvp);
                vfs_mount_free(mp);
                Err(e)
            }
        }
    }

    /// `VOP_LOOKUP` of `name` in `dvp` (the last component, the parent kept locked).
    fn lookup(p: &'static Proc, dvp: &'static Vnode, name: &[u8]) -> Result<&'static Vnode, Errno> {
        let mut cnp = Componentname::new();
        cnp.cn_nameiop = LOOKUP;
        cnp.cn_flags = ISLASTCN | LOCKPARENT | if name == b".." { ISDOTDOT } else { 0 };
        cnp.cn_proc = p;
        cnp.cn_cred = p.ucred();
        cnp.cn_nameptr = name.as_ptr();
        cnp.cn_namelen = name.len() as i64;
        let mut vpp = None;
        VOP_LOOKUP(dvp, &mut vpp, &mut cnp)?;
        Ok(vpp.unwrap())
    }

    /// The whole file through `VOP_READ`, `chunk` bytes at a time.
    fn read_all(vp: &'static Vnode, chunk: usize) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut buf = vec![0u8; chunk];
            let mut iov = [Iovec {
                iov_base: buf.as_mut_ptr().cast(),
                iov_len: chunk,
            }];
            let mut uio = Uio {
                uio_iov: &mut iov,
                uio_offset: out.len() as Off,
                uio_resid: chunk,
                uio_segflg: UioSeg::UIO_SYSSPACE,
                uio_rw: UioRw::UIO_READ,
                uio_procp: None,
            };
            VOP_READ(vp, &mut uio, 0, ptr::null()).unwrap();
            let n = chunk - uio.uio_resid;
            if n == 0 {
                return out;
            }
            out.extend_from_slice(&buf[..n]);
        }
    }

    /// `getdents` of a directory, `len` bytes at a time: (name, fileno, type).
    fn read_dir(vp: &'static Vnode, len: usize) -> Vec<(Vec<u8>, u64, u8)> {
        let mut out = Vec::new();
        let mut offset: Off = 0;
        loop {
            let mut buf = vec![0u8; len];
            let mut iov = [Iovec {
                iov_base: buf.as_mut_ptr().cast(),
                iov_len: len,
            }];
            let mut uio = Uio {
                uio_iov: &mut iov,
                uio_offset: offset,
                uio_resid: len,
                uio_segflg: UioSeg::UIO_SYSSPACE,
                uio_rw: UioRw::UIO_READ,
                uio_procp: None,
            };
            let mut eof = 0;
            VOP_READDIR(vp, &mut uio, ptr::null(), &mut eof).unwrap();
            let used = len - uio.uio_resid;
            let mut pos = 0;
            while pos < used {
                let d = Dirent::from_bytes(&buf[pos..]).unwrap();
                assert_eq!(usize::from(d.d_reclen), DIRENT_SIZE);
                let name = buf[pos + Dirent::NAME_OFFSET..][..usize::from(d.d_namlen)].to_vec();
                // ".." claims the offset after two entries, also in the root, which has no ".".
                if name != b".." {
                    assert_eq!(d.d_off, offset + (pos + DIRENT_SIZE) as Off);
                }
                assert_eq!(buf[pos + Dirent::NAME_OFFSET + usize::from(d.d_namlen)], 0);
                out.push((name, d.d_fileno, d.d_type));
                pos += usize::from(d.d_reclen);
            }
            offset = uio.uio_offset;
            if used == 0 {
                return out;
            }
        }
    }

    fn getattr(p: &'static Proc, vp: &'static Vnode) -> Vattr {
        let mut va = Vattr::new();
        VOP_GETATTR(vp, &mut va, p.ucred(), p).unwrap();
        va
    }

    fn names(v: &[(Vec<u8>, u64, u8)]) -> Vec<(&str, u64, u8)> {
        v.iter()
            .map(|(n, i, t)| (core::str::from_utf8(n).unwrap(), *i, *t))
            .collect()
    }

    #[test]
    fn mount_lookup_read_readdir_and_unmount() {
        let (img, free) = image();
        let (_g, p) = setup(img);
        let mp = mount(p, 0).unwrap();
        let ntmp = VFSTONTFS(mp);
        assert_eq!(ntmp.ntm_bpmftrec.get(), 2);
        assert_eq!(ntmp.ntm_adnum.get(), 3);
        assert_eq!(ntmp.ntm_cfree.get(), free);
        assert_eq!(mp.mnt_flag.get() & MNT_LOCAL, MNT_LOCAL);
        assert_eq!(mp.mnt_stat.get().f_namemax, 255);
        assert_eq!(mp.mnt_stat.get().f_fsid.val[1], 6);

        let mut sb = mp.mnt_stat.get();
        ntfs_statfs(mp, &mut sb, p).unwrap();
        assert_eq!(sb.f_bsize, 512);
        assert_eq!(sb.f_iosize, 1024);
        assert_eq!(sb.f_blocks, (NCL * 2) as u64);
        assert_eq!(sb.f_bfree, free * 2);
        assert_eq!(sb.f_ffree, free);
        assert_eq!(sb.f_files, NREC as u64 + free);

        // The root.
        let root = ntfs_root(mp).unwrap();
        assert!(root.v_flag.get() & VROOT != 0);
        assert_eq!(root.v_type.get(), VDIR);
        assert!(ptr::eq(root, ntmp.sysvn(NTFS_ROOTINO)));
        assert_eq!(getattr(p, root).va_nlink, 1);
        let want = [
            ("..", 5, DT_DIR),
            ("big.bin", 17, DT_REG),
            ("comp.bin", 19, DT_REG),
            ("frag.txt", 22, DT_REG),
            ("hello.txt", 16, DT_REG),
            ("sub", 18, DT_DIR),
        ];
        assert_eq!(names(&read_dir(root, 4096)), want);
        assert_eq!(names(&read_dir(root, 2 * DIRENT_SIZE)), want);

        // A resident file.
        let hello = lookup(p, root, b"hello.txt").unwrap();
        assert_eq!(hello.v_type.get(), VREG);
        assert_eq!(read_all(hello, 4), HELLO);
        let va = getattr(p, hello);
        assert_eq!(va.va_size, HELLO.len() as u64);
        assert_eq!(va.va_fileid, 16);
        assert_eq!((va.va_uid, va.va_gid, va.va_mode), (1000, 100, 0o555));
        assert_eq!(
            (va.va_mtime.tv_sec, va.va_mtime.tv_nsec),
            (1_000_000_000, 123_400)
        );
        assert_eq!(va.va_blocksize, 1024);
        assert_eq!(va.va_nlink, 1);
        // A named attribute spec finds the same fnode; a bad one nothing; case matters.
        let again = lookup(p, root, b"hello.txt:$DATA").unwrap();
        assert!(ptr::eq(again, hello));
        vput(again);
        assert_eq!(
            lookup(p, root, b"hello.txt:$BOGUS").err(),
            Some(Errno::ENOENT)
        );
        assert_eq!(lookup(p, root, b"HELLO.TXT").err(), Some(Errno::ENOENT));
        assert_eq!(lookup(p, root, b"nothere").err(), Some(Errno::ENOENT));
        let dot = lookup(p, root, b".").unwrap();
        assert!(ptr::eq(dot, root));
        vrele(dot);
        vput(hello);

        // A non-resident file with a hole, by VOP_READ and through the buffer cache.
        let bigvp = lookup(p, root, b"big.bin").unwrap();
        assert_eq!(read_all(bigvp, 1000), big());
        assert_eq!(read_all(bigvp, 4096), big());
        let (bp, e) = bread(bigvp, 1, CL as i32);
        e.unwrap();
        // SAFETY: the buffer is ours until brelse.
        assert_eq!(unsafe { bp.data() }, &big()[CL..2 * CL]);
        brelse(bp);
        let (bp, e) = bread(bigvp, 3, CL as i32);
        e.unwrap();
        // SAFETY: as above; past the end of the file the buffer is zero.
        let d = unsafe { bp.data() };
        assert_eq!(&d[..BIG_LEN - 3 * CL], &big()[3 * CL..]);
        assert!(d[BIG_LEN - 3 * CL..].iter().all(|&b| b == 0));
        brelse(bp);
        assert_eq!(getattr(p, bigvp).va_bytes, 4 * CL as u64);
        // The DOS name of the same file.
        let dos = lookup(p, root, b"BIG~1.BIN").unwrap();
        assert!(ptr::eq(dos, bigvp));
        vput(dos);
        vput(bigvp);

        // A compressed file.
        let compvp = lookup(p, root, b"comp.bin").unwrap();
        assert_eq!(read_all(compvp, 5000), comp());
        vput(compvp);

        // A file whose $DATA is in an extension record.
        let frag = lookup(p, root, b"frag.txt").unwrap();
        assert_eq!(read_all(frag, 64), b"fragment data\n");
        vput(frag);

        // A directory whose names are in an index buffer.
        let sub = lookup(p, root, b"sub").unwrap();
        assert_eq!(sub.v_type.get(), VDIR);
        assert_eq!(
            names(&read_dir(sub, 4096)),
            [
                (".", 18, DT_DIR),
                ("..", 5, DT_DIR),
                ("a.txt", 20, DT_REG),
                ("b.txt", 21, DT_REG)
            ]
        );
        let a = lookup(p, sub, b"a.txt").unwrap();
        assert_eq!(read_all(a, 100), b"alpha\n");
        vput(a);
        let b = lookup(p, sub, b"b.txt").unwrap();
        assert_eq!(read_all(b, 100), b"bravo\n");
        vput(b);
        assert_eq!(lookup(p, sub, b"c.txt").err(), Some(Errno::ENOENT));
        let up = lookup(p, sub, b"..").unwrap();
        assert!(ptr::eq(up, root));
        vput(up);
        vput(sub);
        vput(root);

        // Unmount: every vnode and ntnode goes, and the upper-case table with the last mount.
        let dev = ntmp.ntm_dev.get();
        ntfs_unmount(mp, 0, p).unwrap();
        assert!(mp.mnt_data.get().is_null());
        assert_eq!(mp.mnt_flag.get() & MNT_LOCAL, 0);
        assert!(ntfs_nthashlookup(dev, NTFS_MFTINO).is_none());
        assert!(ntfs_nthashlookup(dev, 16).is_none());
        vfs_mount_free(mp);
        teardown();
    }

    #[test]
    fn case_insensitive_mounts_and_all_names() {
        let (img, _) = image();
        let (_g, p) = setup(img);
        let mp = mount(p, NTFS_MFLAG_CASEINS | NTFS_MFLAG_ALLNAMES).unwrap();
        let root = ntfs_root(mp).unwrap();
        let hello = lookup(p, root, b"HELLO.TXT").unwrap();
        assert_eq!(read_all(hello, 64), HELLO);
        vput(hello);
        let dir = read_dir(root, 4096);
        let all = names(&dir);
        assert!(all.contains(&("BIG~1.BIN", 17, DT_REG)));
        assert_eq!(all.len(), 7);
        vput(root);
        ntfs_unmount(mp, 0, p).unwrap();
        vfs_mount_free(mp);
        teardown();
    }

    #[test]
    fn damaged_volumes_are_not_mounted() {
        let (mut img, _) = image();
        img[3] = b'X';
        let (_g, p) = setup(img);
        assert_eq!(mount(p, 0).err(), Some(Errno::EINVAL));
        teardown();

        // A torn MFT record: the root's last sector does not carry the sequence number.
        let (mut img, _) = image();
        img[(MFTCN + 5) * CL + 1022] ^= 0xFF;
        let (_g2, p) = {
            drop(_g);
            setup(img)
        };
        assert_eq!(mount(p, 0).err(), Some(Errno::EINVAL));
        teardown();
    }

    #[test]
    fn pathconf_bmap_and_access() {
        let (img, _) = image();
        let (_g, p) = setup(img);
        let mp = mount(p, 0).unwrap();
        let root = ntfs_root(mp).unwrap();
        let hello = lookup(p, root, b"hello.txt").unwrap();

        let mut v: Register = 0;
        for (name, want) in [(_PC_LINK_MAX, 1), (_PC_NAME_MAX, 255), (_PC_NO_TRUNC, 0)] {
            crate::kern::vfs_vops::VOP_PATHCONF(hello, name, &mut v).unwrap();
            assert_eq!(v, want);
        }
        let mut bn = 0;
        let mut runp = 7;
        crate::kern::vfs_vops::VOP_BMAP(hello, 5, None, Some(&mut bn), Some(&mut runp)).unwrap();
        assert_eq!((bn, runp), (5, 0));

        // Writes are refused; others than root get the mount's mode bits.
        let cred = p.ucred();
        assert_eq!(VOP_ACCESS(hello, VWRITE, cred, p).err(), Some(Errno::EROFS));
        cred.cr_uid.set(1000);
        assert!(VOP_ACCESS(hello, VREAD, cred, p).is_ok());
        cred.cr_uid.set(2000);
        cred.cr_ngroups.set(0);
        assert!(VOP_ACCESS(hello, VREAD | VEXEC, cred, p).is_ok());
        cred.cr_uid.set(0);

        vput(hello);
        vput(root);
        ntfs_unmount(mp, 0, p).unwrap();
        vfs_mount_free(mp);
        teardown();
    }
}
/* </TESTS> */
