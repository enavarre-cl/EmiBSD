/*	$OpenBSD: ntfs_vnops.c,v 1.51 2024/10/18 05:52:32 miod Exp $	*/
/*	$NetBSD: ntfs_vnops.c,v 1.6 2003/04/10 21:57:26 jdolecek Exp $	*/
/* <LICENSES> */
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

#[cfg(test)]
mod tests;
