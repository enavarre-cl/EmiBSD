/*	$OpenBSD: msdosfs_vnops.c,v 1.143 2024/10/18 05:52:32 miod Exp $	*/
/*	$NetBSD: msdosfs_vnops.c,v 1.63 1997/10/17 11:24:19 ws Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (C) 2005 Thomas Wang.
 * Copyright (C) 1994, 1995, 1997 Wolfgang Solfrank.
 * Copyright (C) 1994, 1995, 1997 TooLs GmbH.
 * All rights reserved.
 * Original code by Paul Popelka (paulp@uts.amdahl.com) (see below).
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by TooLs GmbH.
 * 4. The name of TooLs GmbH may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY TOOLS GMBH ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL TOOLS GMBH BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/*
 * Written by Paul Popelka (paulp@uts.amdahl.com)
 *
 * You can do anything you want with this software, just don't say you wrote
 * it, and don't remove this notice.
 *
 * This software is provided "as is".
 *
 * The author supplies this software to be publicly redistributed on the
 * understanding that the author is not responsible for the correct
 * functioning of this software in any circumstances and is not liable for
 * any damages caused by this software.
 *
 * October 1992
 */
/* </LICENSES> */

/* <CODE> */
//! The vnode operations of the msdos file system (`msdosfs_vops`): create, open, close,
//! access, attributes, read and write, fsync, remove, rename, mkdir, rmdir, readdir (with
//! the Win95 long names), the denode lock, bmap and strategy (a file's clusters to the
//! disk's blocks), pathconf, advisory locks and the kqueue filters.
//!
//! Upstream: sys/msdosfs/msdosfs_vnops.c @ 3ce1f3f79392
//!
//! In the ufs filesystem the inodes, superblocks, and indirect blocks are read/written using
//! the vnode for the filesystem. Blocks that represent the contents of a file are
//! read/written using the vnode for the file (including directories when they are
//! read/written as files). This presents problems for the dos filesystem because data that
//! should be in an inode (if dos had them) resides in the directory itself. Since we must
//! update directory entries without the benefit of having the vnode for the directory we must
//! use the vnode for the filesystem. This means that when a directory is actually
//! read/written (via read, write, or readdir, or seek) we must use the vnode for the
//! filesystem instead of the vnode for the directory as would happen in ufs. This is to
//! insure we retrieve the correct block from the buffer cache since the hash value is based
//! upon the vnode address and the desired block number.
//!
//! ## Deviations
//! - The `struct denode ndirent` that `msdosfs_create` and `msdosfs_mkdir` `bzero` on the
//!   stack is a `Denode::new()` there.
//! - `pool_put(&namei_pool, cnp->cn_pnbuf)` is `pnbuf_free`; credentials the C dereferences
//!   (`cred->cr_uid`) go through `ucred`, which panics on `NOCRED`/`FSCRED` where the C would
//!   follow a bad pointer.
//! - `getblk(..., 0, INFSLP)` "never fails" in C; here `getblk` returns an `Option` and the
//!   call is repeated until it yields the buffer (`getblk_wait`), as in `msdosfs_fat.rs`.
//! - The `goto`s of `msdosfs_create`, `msdosfs_write`, `msdosfs_rename`, `msdosfs_mkdir`,
//!   `msdosfs_rmdir` and `msdosfs_readdir` are labeled blocks; `msdosfs_rename`'s `bad`,
//!   `bad1` and `out` are the `RenameExit` its body ends with, `abortit` is
//!   `rename_abortit`.
//! - `msdosfs_readdir` copies the `struct dirent` out as its bytes (`dirent_uiomove`, the
//!   same layout and the same `d_reclen` bytes as `uiomove(&dirbuf, dirbuf.d_reclen, uio)`).
//! - `dosdirtemplate` is a static of two `Direntry`s.
//! - The filters reach their vnode through `kn_hook` (`kn_vnode`), as `ufs_vnops.rs`'s do.
//! - `msdosfs_print` prints under feature `debug` or `diagnostic`; the C's third condition,
//!   `VFSLCKDEBUG`, has no feature here.
//! - The `MSDOSFS_DEBUG` `printf`s are left out: the option is not in GENERIC and has no
//!   feature here. The `DIAGNOSTIC` `HASBUF` checks are behind feature `diagnostic`.

use core::mem::offset_of;
use core::ptr::{self, NonNull};

use crate::kern::kern_event::{klist_insert_locked, klist_remove_locked};
use crate::kern::kern_prot::{groupmember, suser_ucred};
use crate::kern::kern_rwlock::{rrw_enter, rrw_exit, rrw_status};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_tc::getnanotime;
use crate::kern::subr_pool::pool_put;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{
    bawrite, bdwrite, biodone, bread, bread_cluster, brelse, bwrite, getblk,
};
use crate::kern::vfs_cache::cache_purge;
use crate::kern::vfs_default::{vop_generic_abortop, vop_generic_bwrite, vop_generic_revoke};
use crate::kern::vfs_init::NAMEI_POOL;
use crate::kern::vfs_lookup::vfs_relookup;
use crate::kern::vfs_subr::{vaccess, vflushbuf, vput, vrele};
use crate::kern::vfs_vnops::{vn_fsizechk, vn_lock};
use crate::kern::vfs_vops::{VOP_ABORTOP, VOP_ACCESS, VOP_ISLOCKED, VOP_STRATEGY, VOP_UNLOCK};
use crate::machine::cpu::curproc;
use crate::machine::intr::{splbio, splx};
use crate::msdosfs::bpb::{getulong, getushort, putushort};
use crate::msdosfs::denode::{
    DE_ACCESS, DE_CREATE, DE_MODIFIED, DE_RENAME, DE_UPDATE, Denode, FC_LASTFC,
    MSDOSFS_FILESIZE_MAX, WIN_MAXLEN, detimes, vtode,
};
use crate::msdosfs::direntry::{
    ATTR_ARCHIVE, ATTR_DIRECTORY, ATTR_READONLY, ATTR_VOLUME, ATTR_WIN95, CASE_LOWER_BASE,
    CASE_LOWER_EXT, Direntry, SLOT_DELETED, SLOT_EMPTY, WIN_LAST, Winentry,
};
use crate::msdosfs::fat::{MSDOSFSROOT, fat32};
use crate::msdosfs::msdosfs_conv::{dos2unixfn, dos2unixtime, unix2dostime, win2unixfn, winChksum};
use crate::msdosfs::msdosfs_denode::{
    deextend, detrunc, deupdat, msdosfs_inactive, msdosfs_reclaim, reinsert,
};
use crate::msdosfs::msdosfs_fat::{clusteralloc, clusterfree, extendfile, pcbmap};
use crate::msdosfs::msdosfs_lookup::{
    createde, doscheckpath, dosdirempty, msdosfs_lookup, removede, uniqdosname,
};
use crate::msdosfs::msdosfsmount::{
    cntobn, de_clcount, de_cluster, de_cn2bn, de_cn2off, roottobn, vfstomsdosfs,
};
use crate::sys::buf::{Buf, clrbuf};
use crate::sys::dirent::{DT_DIR, DT_REG, Dirent, MAXNAMLEN, dirent_size};
use crate::sys::errno::Errno;
use crate::sys::event::{
    __EV_POLL, __EV_SELECT, EV_EOF, EV_ONESHOT, EVFILT_READ, EVFILT_VNODE, EVFILT_WRITE,
    FILTEROP_ISFD, Filterops, Knote, NOTE_ATTRIB, NOTE_DELETE, NOTE_EOF, NOTE_EXTEND, NOTE_LINK,
    NOTE_RENAME, NOTE_REVOKE, NOTE_TRUNCATE, NOTE_WRITE,
};
use crate::sys::file::foffset;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY, LK_RWFLAGS};
use crate::sys::lockf::lf_advlock;
use crate::sys::mount::{
    MNT_NOATIME, MNT_RDONLY, MNT_WAIT, MSDOSFSMNT_LONGNAME, MSDOSFSMNT_NOWIN95,
    MSDOSFSMNT_SHORTNAME, Mount,
};
use crate::sys::namei::{Componentname, ISDOTDOT, LOCKLEAF, LOCKPARENT, MODMASK, SAVESTART};
use crate::sys::param::MAXBSIZE;
use crate::sys::proc::Proc;
use crate::sys::stat::{
    S_IFDIR, S_IRGRP, S_IROTH, S_IRUSR, S_IWGRP, S_IWOTH, S_IWUSR, S_IXGRP, S_IXOTH, S_IXUSR,
    SF_ARCHIVED, SF_SETTABLE,
};
use crate::sys::systm::INFSLP;
use crate::sys::types::{Daddr, Dev, Gid, Mode, Nlink, Register, Uid};
use crate::sys::ucred::{FSCRED, NOCRED, Ucred};
use crate::sys::uio::Uio;
use crate::sys::unistd::{
    _PC_CHOWN_RESTRICTED, _PC_LINK_MAX, _PC_NAME_MAX, _PC_NO_TRUNC, _PC_TIMESTAMP_RESOLUTION,
};
use crate::sys::vnode::{
    IO_APPEND, IO_SYNC, IO_UNIT, VA_UTIMES_CHANGE, VA_UTIMES_NULL, VBLK, VCHR, VDIR, VN_KNOTE,
    VNON, VNOVAL, VREG, VWRITE, Vnode, VopAccessArgs, VopAdvlockArgs, VopBmapArgs, VopCloseArgs,
    VopCreateArgs, VopFsyncArgs, VopGetattrArgs, VopIoctlArgs, VopIslockedArgs, VopKqfilterArgs,
    VopLinkArgs, VopLockArgs, VopMkdirArgs, VopMknodArgs, VopOpenArgs, VopPathconfArgs,
    VopPrintArgs, VopReadArgs, VopReaddirArgs, VopReadlinkArgs, VopRemoveArgs, VopRenameArgs,
    VopRmdirArgs, VopSetattrArgs, VopStrategyArgs, VopSymlinkArgs, VopUnlockArgs, VopWriteArgs,
    Vops, cred_ref,
};
use crate::uvm::uvm_vnode::{uvm_vnp_setsize, uvm_vnp_uncache};

/// `sizeof(struct direntry)`.
const DIRENTRY_SIZE: u32 = Direntry::SIZE as u32;

/// How `msdosfs_rename` leaves its body: the C's labels it jumps to (or falls into).
#[derive(Clone, Copy, PartialEq, Eq)]
enum RenameExit {
    /// `bad:`: unlock the source, release its directory, then `bad1`.
    Bad,
    /// `bad1:`: release the target (if still held) and the target directory, then `out`.
    Bad1,
    /// `out:`: clear `DE_RENAME` and release the source.
    Out,
}

/// `msdosfs_vops`: the vnode operations vector of msdos vnodes.
pub static MSDOSFS_VOPS: Vops = Vops {
    vop_lookup: Some(msdosfs_lookup),
    vop_create: Some(msdosfs_create),
    vop_mknod: Some(msdosfs_mknod),
    vop_open: Some(msdosfs_open),
    vop_close: Some(msdosfs_close),
    vop_access: Some(msdosfs_access),
    vop_getattr: Some(msdosfs_getattr),
    vop_setattr: Some(msdosfs_setattr),
    vop_read: Some(msdosfs_read),
    vop_write: Some(msdosfs_write),
    vop_ioctl: Some(msdosfs_ioctl),
    vop_kqfilter: Some(msdosfs_kqfilter),
    vop_fsync: Some(msdosfs_fsync),
    vop_remove: Some(msdosfs_remove),
    vop_link: Some(msdosfs_link),
    vop_rename: Some(msdosfs_rename),
    vop_mkdir: Some(msdosfs_mkdir),
    vop_rmdir: Some(msdosfs_rmdir),
    vop_symlink: Some(msdosfs_symlink),
    vop_readdir: Some(msdosfs_readdir),
    vop_readlink: Some(msdosfs_readlink),
    vop_abortop: Some(vop_generic_abortop),
    vop_inactive: Some(msdosfs_inactive),
    vop_reclaim: Some(msdosfs_reclaim),
    vop_lock: Some(msdosfs_lock),
    vop_unlock: Some(msdosfs_unlock),
    vop_bmap: Some(msdosfs_bmap),
    vop_strategy: Some(msdosfs_strategy),
    vop_print: Some(msdosfs_print),
    vop_islocked: Some(msdosfs_islocked),
    vop_pathconf: Some(msdosfs_pathconf),
    vop_advlock: Some(msdosfs_advlock),
    vop_bwrite: Some(vop_generic_bwrite),
    vop_revoke: Some(vop_generic_revoke),
};

/// `msdosfsread_filtops`.
pub static MSDOSFSREAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_msdosfsdetach),
    f_event: Some(filt_msdosfsread),
    f_modify: None,
    f_process: None,
};

/// `msdosfswrite_filtops`.
pub static MSDOSFSWRITE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_msdosfsdetach),
    f_event: Some(filt_msdosfswrite),
    f_modify: None,
    f_process: None,
};

/// `msdosfsvnode_filtops`.
pub static MSDOSFSVNODE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_msdosfsdetach),
    f_event: Some(filt_msdosfsvnode),
    f_modify: None,
    f_process: None,
};

/// One entry of `dosdirtemplate`: a directory named `name`, lower case, with the C's
/// placeholder modification time and date (`{ 210, 4 }`).
const fn dosdirtemplate_entry(name: [u8; 8]) -> Direntry {
    Direntry {
        deName: name,
        deExtension: *b"   ",
        deAttributes: ATTR_DIRECTORY,
        deLowerCase: CASE_LOWER_BASE | CASE_LOWER_EXT,
        deCTimeHundredth: 0,
        deCTime: [0, 0],
        deCDate: [0, 0],
        deADate: [0, 0],
        deHighClust: [0, 0],
        deMTime: [210, 4],
        deMDate: [210, 4],
        deStartCluster: [0, 0],
        deFileSize: [0, 0, 0, 0],
    }
}

/// `dosdirtemplate`: the "." and ".." entries a new directory starts with.
static DOSDIRTEMPLATE: [Direntry; 2] = [
    dosdirtemplate_entry(*b".       "),
    dosdirtemplate_entry(*b"..      "),
];

/// The vnode's mount, which a msdosfs vnode always has.
fn vmount(vp: &Vnode) -> &'static Mount {
    match vp.v_mount.get() {
        Some(mp) => mp,
        None => panic(format_args!("msdosfs: vnode {:p} without a mount", vp)),
    }
}

/// `vp->v_mount->mnt_flag & MNT_RDONLY`.
fn rdonly(vp: &Vnode) -> bool {
    vmount(vp).mnt_flag.get() & MNT_RDONLY != 0
}

/// `*cred` of a credential the C dereferences: a real one (`NOCRED`/`FSCRED` panic).
fn ucred<'a>(cred: *const Ucred) -> &'a Ucred {
    // SAFETY: the credentials a vnode operation receives are held by its caller for the
    // operation's duration (`cred_ref`'s contract).
    match unsafe { cred_ref(cred) } {
        Some(c) => c,
        None => panic(format_args!(
            "msdosfs: {} credential",
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

/// `cnp->cn_proc`, `None` when the component name has no thread.
fn cn_proc(cnp: &Componentname) -> Option<&Proc> {
    (!cnp.cn_proc.is_null()).then(|| cnp.proc())
}

/// `pool_put(&namei_pool, cnp->cn_pnbuf)`: give back the pathname buffer.
fn pnbuf_free(cnp: &Componentname) {
    if let Some(buf) = NonNull::new(cnp.cn_pnbuf) {
        pool_put(&NAMEI_POOL, buf);
    }
}

/// `getblk(vp, blkno, size, 0, INFSLP)`, which cannot fail without `slpflag`.
fn getblk_wait(vp: &'static Vnode, blkno: Daddr, size: i32) -> &'static Buf {
    loop {
        if let Some(bp) = getblk(vp, blkno, size, 0, INFSLP) {
            return bp;
        }
    }
}

/// The data of a buffer the caller owns.
///
/// # Safety
///
/// As `Buf::data`: the buffer is busy for the caller and mapped, and no other slice of it
/// is alive.
#[allow(clippy::mut_from_ref)] // the B_BUSY owner's view, as the C's b_data
unsafe fn bdata(bp: &Buf) -> &mut [u8] {
    // SAFETY: the caller's contract.
    unsafe { bp.data() }
}

/// `msdosfs_create` (`vop_create`): create a regular file.
///
/// On entry the directory to contain the file being created is locked; it stays locked. The
/// pathname buffer is freed always on error, or only if the `SAVESTART` bit in `cn_flags` is
/// clear on success.
pub fn msdosfs_create(ap: &mut VopCreateArgs<'_>) -> Result<(), Errno> {
    let cnp = &mut *ap.a_cnp;
    let pdep = vtode(ap.a_dvp);

    let error: Errno = 'bad: {
        // If this is the root directory and there is no space left we can't do anything.
        // This is because the root directory can not change size.
        if pdep.de_StartCluster.get() == MSDOSFSROOT
            && pdep.de_fndoffset.get() >= pdep.de_FileSize.get()
        {
            break 'bad Errno::ENOSPC;
        }

        // Create a directory entry for the file, then call createde() to have it installed.
        // NOTE: DOS files are always executable. We use the absence of the owner write bit
        // to make the file readonly.
        #[cfg(feature = "diagnostic")]
        if cnp.cn_flags & crate::sys::namei::HASBUF == 0 {
            panic(format_args!("msdosfs_create: no name"));
        }
        let ndirent = Denode::new();
        let mut name = [0u8; 11];
        if let Err(e) = uniqdosname(pdep, cnp, &mut name) {
            break 'bad e;
        }
        ndirent.de_Name.set(name);

        ndirent
            .de_Attributes
            .set(if ap.a_vap.va_mode & VWRITE as Mode != 0 {
                ATTR_ARCHIVE
            } else {
                ATTR_ARCHIVE | ATTR_READONLY
            });
        ndirent.de_StartCluster.set(0);
        ndirent.de_FileSize.set(0);
        ndirent.de_dev.set(pdep.de_dev.get());
        ndirent.de_devvp.set(pdep.de_devvp.get());
        ndirent.de_pmp.set(pdep.de_pmp.get());
        ndirent.de_flag.set(DE_ACCESS | DE_CREATE | DE_UPDATE);
        let ts = getnanotime();
        detimes(&ndirent, &ts, &ts, &ts);
        let mut dep = None;
        if let Err(e) = createde(&ndirent, pdep, Some(&mut dep), cnp) {
            break 'bad e;
        }
        let Some(dep) = dep else {
            panic(format_args!("msdosfs_create: no denode"));
        };
        if cnp.cn_flags & SAVESTART == 0 {
            pnbuf_free(cnp);
        }
        VN_KNOTE(ap.a_dvp, NOTE_WRITE);
        *ap.a_vpp = Some(dep.detov());
        return Ok(());
    };

    // bad:
    pnbuf_free(cnp);
    Err(error)
}

/// `msdosfs_mknod` (`vop_mknod`): DOS file systems have no special files.
pub fn msdosfs_mknod(ap: &mut VopMknodArgs<'_>) -> Result<(), Errno> {
    pnbuf_free(ap.a_cnp);
    VN_KNOTE(ap.a_dvp, NOTE_WRITE);
    Err(Errno::EINVAL)
}

/// `msdosfs_open` (`vop_open`): nothing to do.
pub fn msdosfs_open(_ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    Ok(())
}

/// `msdosfs_close` (`vop_close`): update the times of a file others still use.
pub fn msdosfs_close(ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let dep = vtode(vp);

    if vp.v_usecount.get() > 1 && VOP_ISLOCKED(vp) == 0 {
        let ts = getnanotime();
        detimes(dep, &ts, &ts, &ts);
    }
    Ok(())
}

/// `msdosfs_access` (`vop_access`): the permissions the attributes and the mount's mask give.
pub fn msdosfs_access(ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
    let dep = vtode(ap.a_vp);
    let pmp = dep.pmp();

    let mut dosmode: Mode = S_IRUSR | S_IRGRP | S_IROTH;
    if dep.de_Attributes.get() & ATTR_READONLY == 0 {
        dosmode |= S_IWUSR | S_IWGRP | S_IWOTH;
    }
    if dep.de_Attributes.get() & ATTR_DIRECTORY != 0 {
        dosmode |= S_IXUSR | S_IXGRP | S_IXOTH;
    }
    dosmode &= pmp.pm_mask.get();

    vaccess(
        ap.a_vp.v_type.get(),
        dosmode,
        pmp.pm_uid.get(),
        pmp.pm_gid.get(),
        ap.a_mode,
        ucred(ap.a_cred),
    )
}

/// `msdosfs_getattr` (`vop_getattr`).
pub fn msdosfs_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    let dep = vtode(ap.a_vp);
    let pmp = dep.pmp();
    let vap = &mut *ap.a_vap;

    let ts = getnanotime();
    detimes(dep, &ts, &ts, &ts);
    vap.va_fsid = i64::from(dep.de_dev.get());

    // The following computation of the fileid must be the same as that used in
    // msdosfs_readdir() to compute d_fileno. If not, pwd doesn't work.
    //
    // We now use the starting cluster number as the fileid/fileno. This works for both files
    // and directories (including the root directory, on FAT32). Even on FAT32, this will at
    // most be a 28-bit number, as the high 4 bits of FAT32 cluster numbers are reserved.
    //
    // However, we do need to do something for 0-length files, which will not have a starting
    // cluster number.
    //
    // These files cannot be directories, since (except for /, which is special-cased anyway)
    // directories contain entries for . and .., so must have non-zero length.
    //
    // In this case, we just create a non-cryptographic hash of the original fileid
    // calculation, and set the top bit.
    //
    // This algorithm has the benefit that all directories, and all non-zero-length files,
    // will have fileids that are persistent across mounts and reboots, and that cannot
    // collide (as long as the filesystem is not corrupt). Zero-length files will have fileids
    // that are persistent, but that may collide. We will just have to live with that.
    let mut fileid = dep.de_StartCluster.get();

    if dep.de_Attributes.get() & ATTR_DIRECTORY != 0 {
        // Special-case root
        if dep.de_StartCluster.get() == MSDOSFSROOT {
            fileid = if fat32(pmp) {
                pmp.pm_rootdirblk.get()
            } else {
                1
            };
        }
    } else if dep.de_FileSize.get() == 0 {
        let dirsperblk = u32::from(pmp.pm_BytesPerSec()) / DIRENTRY_SIZE;

        let mut fileid64 = u64::from(if dep.de_dirclust.get() == MSDOSFSROOT {
            roottobn(pmp, 0u32)
        } else {
            cntobn(pmp, dep.de_dirclust.get())
        });
        fileid64 = fileid64.wrapping_mul(u64::from(dirsperblk));
        fileid64 = fileid64.wrapping_add(u64::from(dep.de_diroffset.get() / DIRENTRY_SIZE));

        fileid = fileidhash(fileid64);
    }

    vap.va_fileid = u64::from(fileid);
    vap.va_mode = S_IRUSR | S_IRGRP | S_IROTH;
    if dep.de_Attributes.get() & ATTR_READONLY == 0 {
        vap.va_mode |= S_IWUSR | S_IWGRP | S_IWOTH;
    }
    if dep.de_Attributes.get() & ATTR_DIRECTORY != 0 {
        vap.va_mode |= S_IFDIR;
        vap.va_mode |= if vap.va_mode & S_IRUSR != 0 {
            S_IXUSR
        } else {
            0
        };
        vap.va_mode |= if vap.va_mode & S_IRGRP != 0 {
            S_IXGRP
        } else {
            0
        };
        vap.va_mode |= if vap.va_mode & S_IROTH != 0 {
            S_IXOTH
        } else {
            0
        };
    }
    vap.va_mode &= pmp.pm_mask.get();
    vap.va_nlink = 1;
    vap.va_gid = pmp.pm_gid.get();
    vap.va_uid = pmp.pm_uid.get();
    vap.va_rdev = 0;
    vap.va_size = u64::from(dep.de_FileSize.get());
    vap.va_mtime = dos2unixtime(
        u32::from(dep.de_MDate.get()),
        u32::from(dep.de_MTime.get()),
        0,
    );
    if pmp.pm_flags.get() & MSDOSFSMNT_LONGNAME as u32 != 0 {
        vap.va_atime = dos2unixtime(u32::from(dep.de_ADate.get()), 0, 0);
        vap.va_ctime = dos2unixtime(
            u32::from(dep.de_CDate.get()),
            u32::from(dep.de_CTime.get()),
            u32::from(dep.de_CTimeHundredth.get()),
        );
    } else {
        vap.va_atime = vap.va_mtime;
        vap.va_ctime = vap.va_mtime;
    }
    vap.va_flags = 0;
    if dep.de_Attributes.get() & ATTR_ARCHIVE == 0 {
        vap.va_flags |= u64::from(SF_ARCHIVED);
    }
    vap.va_gen = 0;
    vap.va_blocksize = i64::from(pmp.pm_bpcluster.get());
    vap.va_bytes = u64::from(
        dep.de_FileSize.get().wrapping_add(pmp.pm_crbomask.get()) & !pmp.pm_crbomask.get(),
    );
    vap.va_type = ap.a_vp.v_type.get();
    Ok(())
}

/// `msdosfs_setattr` (`vop_setattr`): the attributes a DOS file has: the archive flag, the
/// size, the access and modification times, and the owner write bit (as the read-only
/// attribute). The owner and group can only be "changed" to the mount's.
pub fn msdosfs_setattr(ap: &mut VopSetattrArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let dep = vtode(vp);
    let pmp = dep.pmp();
    let vap = &mut *ap.a_vap;
    let cred = ap.a_cred;

    if vap.va_type != VNON
        || vap.va_nlink != VNOVAL as Nlink
        || vap.va_fsid != i64::from(VNOVAL)
        || vap.va_fileid != VNOVAL as u64
        || vap.va_blocksize != i64::from(VNOVAL)
        || vap.va_rdev != VNOVAL as Dev
        || vap.va_bytes != VNOVAL as u64
        || vap.va_gen != VNOVAL as u64
    {
        return Err(Errno::EINVAL);
    }
    if vap.va_flags != VNOVAL as u64 {
        if rdonly(vp) {
            return Err(Errno::EINVAL);
        }
        let c = ucred(cred);
        if c.cr_uid.get() != pmp.pm_uid.get() {
            suser_ucred(c)?;
        }
        // We are very inconsistent about handling unsupported attributes. We ignored the
        // access time and the read and execute bits. We were strict for the other
        // attributes.
        //
        // Here we are strict, stricter than ufs in not allowing users to attempt to set
        // SF_SETTABLE bits or anyone to set unsupported bits. However, we ignore attempts to
        // set ATTR_ARCHIVE for directories `cp -pr' from a more sensible filesystem attempts
        // it a lot.
        if vap.va_flags & u64::from(SF_SETTABLE) != 0 {
            suser_ucred(c)?;
        }
        if vap.va_flags & !u64::from(SF_ARCHIVED) != 0 {
            return Err(Errno::EOPNOTSUPP);
        }
        if vap.va_flags & u64::from(SF_ARCHIVED) != 0 {
            dep.de_Attributes
                .set(dep.de_Attributes.get() & !ATTR_ARCHIVE);
        } else if dep.de_Attributes.get() & ATTR_DIRECTORY == 0 {
            dep.de_Attributes
                .set(dep.de_Attributes.get() | ATTR_ARCHIVE);
        }
        dep.set_flag(DE_MODIFIED);
    }

    if vap.va_uid != VNOVAL as Uid || vap.va_gid != VNOVAL as Gid {
        if rdonly(vp) {
            return Err(Errno::EINVAL);
        }
        let mut uid = vap.va_uid;
        if uid == VNOVAL as Uid {
            uid = pmp.pm_uid.get();
        }
        let mut gid = vap.va_gid;
        if gid == VNOVAL as Gid {
            gid = pmp.pm_gid.get();
        }
        let c = ucred(cred);
        if c.cr_uid.get() != pmp.pm_uid.get()
            || uid != pmp.pm_uid.get()
            || (gid != pmp.pm_gid.get() && !groupmember(gid, c))
        {
            suser_ucred(c)?;
        }
        if uid != pmp.pm_uid.get() || gid != pmp.pm_gid.get() {
            return Err(Errno::EINVAL);
        }
    }

    if vap.va_size != VNOVAL as u64 {
        match vp.v_type.get() {
            VDIR => return Err(Errno::EISDIR),
            // Truncation is only supported for regular files, Disallow it if the
            // filesystem is read-only.
            VREG if rdonly(vp) => return Err(Errno::EINVAL),
            VREG => {}
            _ => {
                // According to POSIX, the result is unspecified for file types other than
                // regular files, directories and shared memory objects. We don't support any
                // file types except regular files and directories in this file system, so
                // this (default) case is unreachable and can do anything. Keep falling
                // through to detrunc() for now.
            }
        }
        detrunc(dep, vap.va_size as u32, 0, cred, Some(ap.a_p))?;
    }
    if vap.va_vaflags & VA_UTIMES_CHANGE != 0
        || vap.va_atime.tv_nsec != i64::from(VNOVAL)
        || vap.va_mtime.tv_nsec != i64::from(VNOVAL)
    {
        if rdonly(vp) {
            return Err(Errno::EINVAL);
        }
        let c = ucred(cred);
        if c.cr_uid.get() != pmp.pm_uid.get()
            && let Err(e) = suser_ucred(c)
        {
            if vap.va_vaflags & VA_UTIMES_NULL == 0 {
                return Err(e);
            }
            VOP_ACCESS(vp, VWRITE, cred, ap.a_p)?;
        }
        if vp.v_type.get() != VDIR {
            if pmp.pm_flags.get() & MSDOSFSMNT_NOWIN95 as u32 == 0
                && vap.va_atime.tv_nsec != i64::from(VNOVAL)
            {
                dep.clr_flag(DE_ACCESS);
                let (dd, _, _) = unix2dostime(&vap.va_atime);
                dep.de_ADate.set(dd);
            }
            if vap.va_mtime.tv_nsec != i64::from(VNOVAL) {
                dep.clr_flag(DE_UPDATE);
                let (dd, dt, _) = unix2dostime(&vap.va_mtime);
                dep.de_MDate.set(dd);
                dep.de_MTime.set(dt);
            }
            dep.de_Attributes
                .set(dep.de_Attributes.get() | ATTR_ARCHIVE);
            dep.set_flag(DE_MODIFIED);
        }
    }
    // DOS files only have the ability to have their writability attribute set, so we use
    // the owner write bit to set the readonly attribute.
    if vap.va_mode != VNOVAL as Mode {
        if rdonly(vp) {
            return Err(Errno::EINVAL);
        }
        let c = ucred(cred);
        if c.cr_uid.get() != pmp.pm_uid.get() {
            suser_ucred(c)?;
        }
        if vp.v_type.get() != VDIR {
            // We ignore the read and execute bits.
            if vap.va_mode & VWRITE as Mode != 0 {
                dep.de_Attributes
                    .set(dep.de_Attributes.get() & !ATTR_READONLY);
            } else {
                dep.de_Attributes
                    .set(dep.de_Attributes.get() | ATTR_READONLY);
            }
            dep.de_Attributes
                .set(dep.de_Attributes.get() | ATTR_ARCHIVE);
            dep.set_flag(DE_MODIFIED);
        }
    }
    VN_KNOTE(vp, NOTE_ATTRIB);
    deupdat(dep, 1)
}

/// `msdosfs_read` (`vop_read`): read a file, or a directory as a file (through the
/// file system's device vnode, see the module's documentation).
pub fn msdosfs_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let dep = vtode(vp);
    let pmp = dep.pmp();
    let uio = &mut *ap.a_uio;
    let bpcluster = pmp.pm_bpcluster.get();

    // If they didn't ask for any data, then we are done.
    if uio.uio_resid == 0 {
        return Ok(());
    }
    if uio.uio_offset < 0 {
        return Err(Errno::EINVAL);
    }

    let isadir = dep.de_Attributes.get() & ATTR_DIRECTORY != 0;
    let mut error;
    loop {
        if uio.uio_offset >= i64::from(dep.de_FileSize.get()) {
            return Ok(());
        }

        let cn = de_cluster(pmp, uio.uio_offset) as u32;
        let mut size = bpcluster as i32;
        let on = uio.uio_offset as u32 & pmp.pm_crbomask.get();
        let mut n = u64::from(bpcluster - on).min(uio.uio_resid as u64) as u32;

        // de_FileSize is uint32_t, and we know that uio_offset < de_FileSize, so
        // uio->uio_offset < 2^32. Therefore the cast to uint32_t on the next line is safe.
        let diff = dep.de_FileSize.get() - uio.uio_offset as u32;
        if diff < n {
            n = diff;
        }

        // If we are operating on a directory file then be sure to do i/o with the vnode for
        // the filesystem instead of the vnode for the directory.
        let (bp, r) = if isadir {
            // convert cluster # to block #
            let mut bn: Daddr = 0;
            pcbmap(dep, cn, Some(&mut bn), None, Some(&mut size))?;
            bread(pmp.devvp(), bn, size)
        } else if de_cn2off(pmp, cn.wrapping_add(1)) >= dep.de_FileSize.get() {
            bread(vp, Daddr::from(cn), size)
        } else {
            bread_cluster(vp, Daddr::from(cn), size)
        };
        n = n.min(bpcluster.wrapping_sub(bp.b_resid.get() as u32));
        if let Err(e) = r {
            brelse(bp);
            return Err(e);
        }
        {
            // SAFETY: the buffer is busy for this function (from `bread`) and mapped; the
            // slice is not used after the buffer is released.
            let data = unsafe { bdata(bp) };
            error = uiomove(&mut data[on as usize..(on + n) as usize], uio);
        }
        brelse(bp);
        if !(error.is_ok() && uio.uio_resid > 0 && n != 0) {
            break;
        }
    }
    if !isadir && vmount(vp).mnt_flag.get() & MNT_NOATIME == 0 {
        dep.set_flag(DE_ACCESS);
    }
    error
}

/// `msdosfs_write` (`vop_write`): write data to a file.
pub fn msdosfs_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    let mut extended = false;
    let ioflag = ap.a_ioflag;
    let uio = &mut *ap.a_uio;
    let vp = ap.a_vp;
    let dep = vtode(vp);
    let pmp = dep.pmp();
    let cred = ap.a_cred;
    let bpcluster = pmp.pm_bpcluster.get();

    let thisvp = match vp.v_type.get() {
        VREG => {
            if ioflag & IO_APPEND != 0 {
                uio.uio_offset = i64::from(dep.de_FileSize.get());
            }
            vp
        }
        VDIR => return Err(Errno::EISDIR),
        _ => panic(format_args!("msdosfs_write(): bad file type")),
    };

    if uio.uio_offset < 0 {
        return Err(Errno::EINVAL);
    }

    if uio.uio_resid == 0 {
        return Ok(());
    }

    // Don't bother to try to write files larger than the f/s limit
    if uio.uio_offset > MSDOSFS_FILESIZE_MAX
        || uio.uio_resid as u64 > (MSDOSFS_FILESIZE_MAX - uio.uio_offset) as u64
    {
        return Err(Errno::EFBIG);
    }

    // do the filesize rlimit check
    let overrun = vn_fsizechk(vp, uio, ioflag)?;

    let error: Result<(), Errno> = 'out: {
        // If the offset we are starting the write at is beyond the end of the file, then
        // they've done a seek. Unix filesystems allow files with holes in them, DOS doesn't so
        // we must fill the hole with zeroed blocks.
        if uio.uio_offset > i64::from(dep.de_FileSize.get())
            && let Err(e) = deextend(dep, uio.uio_offset as u32, cred)
        {
            break 'out Err(e);
        }

        // Remember some values in case the write fails.
        let resid = uio.uio_resid;
        let osize = dep.de_FileSize.get();
        let mut error: Result<(), Errno> = Ok(());

        'errexit: {
            // If we write beyond the end of the file, extend it to its ultimate size ahead of
            // the time to hopefully get a contiguous area.
            let end = (uio.uio_offset as u64).wrapping_add(resid as u64);
            let lastcn = if end > u64::from(osize) {
                extended = true;
                let count =
                    de_clcount(pmp, end).wrapping_sub(u64::from(de_clcount(pmp, osize))) as u32;
                error = extendfile(dep, count, None, None, 0);
                if let Err(e) = error
                    && (e != Errno::ENOSPC || ioflag & IO_UNIT != 0)
                {
                    break 'errexit;
                }
                dep.fc(FC_LASTFC).fc_frcn
            } else {
                de_clcount(pmp, osize).wrapping_sub(1)
            };

            loop {
                let croffset = uio.uio_offset as u32 & pmp.pm_crbomask.get();
                let cn = de_cluster(pmp, uio.uio_offset) as u32;

                if cn > lastcn {
                    error = Err(Errno::ENOSPC);
                    break;
                }

                let end = (uio.uio_offset as u64).wrapping_add(uio.uio_resid as u64);
                let bp = if croffset == 0
                    && (de_cluster(pmp, end) > u64::from(cn)
                        || end >= u64::from(dep.de_FileSize.get()))
                {
                    // If either the whole cluster gets written, or we write the cluster from
                    // its start beyond EOF, then no need to read data from disk.
                    let bp = getblk_wait(thisvp, Daddr::from(cn), bpcluster as i32);
                    // SAFETY: the buffer is busy for this function (from `getblk`) and
                    // mapped.
                    unsafe { clrbuf(bp) };
                    // Do the bmap now, since pcbmap needs buffers for the fat table. (see
                    // msdosfs_strategy)
                    if bp.b_blkno.get() == bp.b_lblkno.get() {
                        let mut blkno = bp.b_blkno.get();
                        error = pcbmap(dep, bp.b_lblkno.get() as u32, Some(&mut blkno), None, None);
                        bp.b_blkno.set(if error.is_err() { -1 } else { blkno });
                    }
                    if bp.b_blkno.get() == -1 {
                        brelse(bp);
                        if error.is_ok() {
                            error = Err(Errno::EIO); // XXX
                        }
                        break;
                    }
                    bp
                } else {
                    // The block we need to write into exists, so read it in.
                    let (bp, r) = bread(thisvp, Daddr::from(cn), bpcluster as i32);
                    error = r;
                    if error.is_err() {
                        brelse(bp);
                        break;
                    }
                    bp
                };

                let n = (uio.uio_resid as u64).min(u64::from(bpcluster - croffset)) as u32;
                if uio.uio_offset + i64::from(n) > i64::from(dep.de_FileSize.get()) {
                    dep.de_FileSize.set((uio.uio_offset + i64::from(n)) as u32);
                    uvm_vnp_setsize(vp, i64::from(dep.de_FileSize.get()));
                }
                let _ = uvm_vnp_uncache(vp);
                // Should these vnode_pager_* functions be done on dir files?

                // Copy the data from user space into the buf header.
                {
                    // SAFETY: the buffer is busy for this function and mapped; the slice is
                    // not used after the buffer is written.
                    let data = unsafe { bdata(bp) };
                    error = uiomove(&mut data[croffset as usize..(croffset + n) as usize], uio);
                }

                // If they want this synchronous then write it and wait for it. Otherwise, if
                // on a cluster boundary write it asynchronously so we can move on to the next
                // block without delay. Otherwise do a delayed write because we may want to
                // write some more into the block later.
                // (`#if 0`: IO_NOCACHE sets B_NOCACHE.)
                if ioflag & IO_SYNC != 0 {
                    let _ = bwrite(bp);
                } else if n + croffset == bpcluster {
                    bawrite(bp);
                } else {
                    bdwrite(bp);
                }
                dep.set_flag(DE_UPDATE);
                if !(error.is_ok() && uio.uio_resid > 0) {
                    break;
                }
            }

            if resid > uio.uio_resid {
                VN_KNOTE(vp, NOTE_WRITE | if extended { NOTE_EXTEND } else { 0 });
            }

            if dep.de_FileSize.get() < osize {
                VN_KNOTE(vp, NOTE_TRUNCATE);
            }
        }

        // errexit: If the write failed and they want us to, truncate the file back to the
        // size it was before the write was attempted.
        if error.is_err() {
            if ioflag & IO_UNIT != 0 {
                let _ = detrunc(dep, osize, ioflag & IO_SYNC, NOCRED, curproc());
                uio.uio_offset -= (resid - uio.uio_resid) as i64;
                uio.uio_resid = resid;
            } else {
                let _ = detrunc(
                    dep,
                    dep.de_FileSize.get(),
                    ioflag & IO_SYNC,
                    NOCRED,
                    curproc(),
                );
                if uio.uio_resid != resid {
                    error = Ok(());
                }
            }
        } else if ioflag & IO_SYNC != 0 {
            error = deupdat(dep, 1);
        }
        error
    };

    // out: correct the result for writes clamped by vn_fsizechk()
    uio.uio_resid = (uio.uio_resid as isize + overrun) as usize;
    error
}

/// `msdosfs_ioctl` (`vop_ioctl`): no ioctls on msdos files.
pub fn msdosfs_ioctl(_ap: &mut VopIoctlArgs<'_>) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

/// `msdosfs_fsync` (`vop_fsync`): flush the blocks of a file to disk.
///
/// This function is worthless for vnodes that represent directories. Maybe we could just do
/// a sync if they try an fsync on a directory file.
pub fn msdosfs_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    vflushbuf(vp, ap.a_waitfor == MNT_WAIT);
    deupdat(vtode(vp), i32::from(ap.a_waitfor == MNT_WAIT))
}

/// `msdosfs_remove` (`vop_remove`): remove the entry of a file that is not a directory.
pub fn msdosfs_remove(ap: &mut VopRemoveArgs<'_>) -> Result<(), Errno> {
    let dep = vtode(ap.a_vp);
    let ddep = vtode(ap.a_dvp);

    let error = if ap.a_vp.v_type.get() == VDIR {
        Err(Errno::EPERM)
    } else {
        removede(ddep, dep)
    };

    VN_KNOTE(ap.a_vp, NOTE_DELETE);
    VN_KNOTE(ap.a_dvp, NOTE_WRITE);

    error
}

/// `msdosfs_link` (`vop_link`): DOS filesystems don't know what links are. But since we
/// already called `msdosfs_lookup()` with create and lockparent, the parent is locked so we
/// have to free it before we return the error.
pub fn msdosfs_link(ap: &mut VopLinkArgs<'_>) -> Result<(), Errno> {
    let _ = VOP_ABORTOP(ap.a_dvp, ap.a_cnp);
    vput(ap.a_dvp);
    Err(Errno::EOPNOTSUPP)
}

/// `abortit:` of `msdosfs_rename`: abort both lookups and release every vnode.
#[allow(clippy::too_many_arguments)] // the C label's state
fn rename_abortit(
    error: Result<(), Errno>,
    tdvp: &'static Vnode,
    tvp: Option<&'static Vnode>,
    tcnp: &mut Componentname,
    fdvp: &'static Vnode,
    fvp: &'static Vnode,
    fcnp: &mut Componentname,
) -> Result<(), Errno> {
    let _ = VOP_ABORTOP(tdvp, tcnp);
    if tvp.is_some_and(|t| ptr::eq(t, tdvp)) {
        vrele(tdvp);
    } else {
        vput(tdvp);
    }
    if let Some(tvp) = tvp {
        vput(tvp);
    }
    let _ = VOP_ABORTOP(fdvp, fcnp);
    vrele(fdvp);
    vrele(fvp);
    error
}

/// `msdosfs_rename` (`vop_rename`).
///
/// Renames on files require moving the denode to a new hash queue since the denode's
/// location is used to compute which hash queue to put the file in. Unless it is a rename in
/// place. For example "mv a b".
///
/// What follows is the basic algorithm:
///
/// ```text
/// if (file move) {
///     if (dest file exists) {
///         remove dest file
///     }
///     if (dest and src in same directory) {
///         rewrite name in existing directory slot
///     } else {
///         write new entry in dest directory
///         update offset and dirclust in denode
///         move denode to new hash chain
///         clear old directory entry
///     }
/// } else {
///     directory move
///     if (dest directory exists) {
///         if (dest is not empty) {
///             return ENOTEMPTY
///         }
///         remove dest directory
///     }
///     if (dest and src in same directory) {
///         rewrite name in existing entry
///     } else {
///         be sure dest is not a child of src directory
///         write entry in dest directory
///         update "." and ".." in moved directory
///         update offset and dirclust in denode
///         move denode to new hash chain
///         clear old directory entry for moved directory
///     }
/// }
/// ```
///
/// On entry: source's parent directory is unlocked; source file or directory is unlocked;
/// destination's parent directory is locked; destination file or directory is locked if it
/// exists. On exit: all denodes should be released.
pub fn msdosfs_rename(ap: &mut VopRenameArgs<'_>) -> Result<(), Errno> {
    let mut tvp = ap.a_tvp;
    let tdvp = ap.a_tdvp;
    let a_fvp = ap.a_fvp;
    let mut fvp = ap.a_fvp;
    let fdvp = ap.a_fdvp;
    let tcnp = &mut *ap.a_tcnp;
    let fcnp = &mut *ap.a_fcnp;
    let mut doingdirectory = false;
    let mut newparent = false;

    let pmp = vfstomsdosfs(vmount(fdvp));

    #[cfg(feature = "diagnostic")]
    if tcnp.cn_flags & crate::sys::namei::HASBUF == 0
        || fcnp.cn_flags & crate::sys::namei::HASBUF == 0
    {
        panic(format_args!("msdosfs_rename: no name"));
    }
    // Check for cross-device rename.
    let fmp = fvp.v_mount.get().map(ptr::from_ref);
    if fmp != tdvp.v_mount.get().map(ptr::from_ref)
        || tvp.is_some_and(|t| fmp != t.v_mount.get().map(ptr::from_ref))
    {
        return rename_abortit(Err(Errno::EXDEV), tdvp, tvp, tcnp, fdvp, fvp, fcnp);
    }

    // If source and dest are the same, do nothing.
    if tvp.is_some_and(|t| ptr::eq(t, fvp)) {
        return rename_abortit(Ok(()), tdvp, tvp, tcnp, fdvp, fvp, fcnp);
    }

    if let Err(e) = vn_lock(fvp, LK_EXCLUSIVE | LK_RETRY) {
        return rename_abortit(Err(e), tdvp, tvp, tcnp, fdvp, fvp, fcnp);
    }
    let mut dp = vtode(fdvp);
    let ip = vtode(fvp);

    // Be sure we are not renaming ".", "..", or an alias of ".". This leads to a crippled
    // directory tree. It's pretty tough to do a "ls" or "pwd" with the "." directory entry
    // missing, and "cd .." doesn't work if the ".." entry is missing.
    if ip.de_Attributes.get() & ATTR_DIRECTORY != 0 {
        // Avoid ".", "..", and aliases of "." for obvious reasons.
        if (fcnp.cn_namelen == 1 && fcnp.name().first() == Some(&b'.'))
            || ptr::eq(dp, ip)
            || fcnp.cn_flags & ISDOTDOT != 0
            || tcnp.cn_flags & ISDOTDOT != 0
            || ip.de_flag.get() & DE_RENAME != 0
        {
            let _ = VOP_UNLOCK(fvp);
            return rename_abortit(Err(Errno::EINVAL), tdvp, tvp, tcnp, fdvp, fvp, fcnp);
        }
        ip.set_flag(DE_RENAME);
        doingdirectory = true;
    }
    VN_KNOTE(fdvp, NOTE_WRITE); // XXX right place?

    // When the target exists, both the directory and target vnodes are returned locked.
    dp = vtode(tdvp);
    let mut xp: Option<&'static Denode> = tvp.map(vtode);
    // Remember direntry place to use for destination
    let to_diroffset = dp.de_fndoffset.get();
    let to_count = dp.de_fndcnt.get();

    // If ".." must be changed (ie the directory gets a new parent) then the source directory
    // must not be in the directory hierarchy above the target, as this would orphan
    // everything below the source directory. Also the user must have write permission in the
    // source so as to be able to change "..". We must repeat the call to namei, as the parent
    // directory is unlocked by the call to doscheckpath().
    let mut error = VOP_ACCESS(fvp, VWRITE, tcnp.cn_cred, tcnp.proc());
    let _ = VOP_UNLOCK(fvp);
    if vtode(fdvp).de_StartCluster.get() != vtode(tdvp).de_StartCluster.get() {
        newparent = true;
    }
    vrele(fdvp);

    let exit: RenameExit = 'body: {
        if doingdirectory && newparent {
            if error.is_err() {
                // write access check above
                break 'body RenameExit::Bad1;
            }
            if xp.is_some()
                && let Some(t) = tvp
            {
                vput(t);
            }
            // doscheckpath() vput()'s dp, so we have to do a relookup afterwards
            error = doscheckpath(ip, dp);
            if error.is_err() {
                break 'body RenameExit::Out;
            }
            if tcnp.cn_flags & SAVESTART == 0 {
                panic(format_args!("msdosfs_rename: lost to startdir"));
            }
            let mut ntvp = None;
            error = vfs_relookup(tdvp, &mut ntvp, tcnp);
            if error.is_err() {
                break 'body RenameExit::Out;
            }
            tvp = ntvp;
            dp = vtode(tdvp);
            xp = tvp.map(vtode);
        }

        VN_KNOTE(tdvp, NOTE_WRITE);

        if let Some(x) = xp {
            // Target must be empty if a directory and have no links to it. Also, ensure
            // source and target are compatible (both directories, or both not directories).
            if x.de_Attributes.get() & ATTR_DIRECTORY != 0 {
                if !dosdirempty(x) {
                    error = Err(Errno::ENOTEMPTY);
                    break 'body RenameExit::Bad1;
                }
                if !doingdirectory {
                    error = Err(Errno::ENOTDIR);
                    break 'body RenameExit::Bad1;
                }
                cache_purge(tdvp);
            } else if doingdirectory {
                error = Err(Errno::EISDIR);
                break 'body RenameExit::Bad1;
            }
            error = removede(dp, x);
            if error.is_err() {
                break 'body RenameExit::Bad1;
            }
            let t = x.detov();
            VN_KNOTE(t, NOTE_DELETE);
            vput(t);
            xp = None;
        }

        // Convert the filename in tcnp into a dos filename. We copy this into the denode and
        // directory entry for the destination file/directory.
        let mut toname = [0u8; 11];
        error = uniqdosname(vtode(tdvp), tcnp, &mut toname);
        if error.is_err() {
            break 'body RenameExit::Bad1;
        }

        // Since from wasn't locked at various places above, have to do a relookup here.
        fcnp.cn_flags &= !MODMASK;
        fcnp.cn_flags |= LOCKPARENT | LOCKLEAF;
        if fcnp.cn_flags & SAVESTART == 0 {
            panic(format_args!("msdosfs_rename: lost from startdir"));
        }
        if !newparent {
            let _ = VOP_UNLOCK(tdvp);
        }
        let mut nfvp = None;
        let _ = vfs_relookup(fdvp, &mut nfvp, fcnp);
        let Some(nfvp) = nfvp else {
            // From name has disappeared.
            if doingdirectory {
                panic(format_args!("rename: lost dir entry"));
            }
            vrele(a_fvp);
            if newparent {
                let _ = VOP_UNLOCK(tdvp);
            }
            vrele(tdvp);
            return Ok(());
        };
        fvp = nfvp;
        let x = vtode(fvp);
        let zp = vtode(fdvp);
        let from_diroffset = zp.de_fndoffset.get();

        // Ensure that the directory entry still exists and has not changed till now. If the
        // source is a file the entry may have been unlinked or renamed. In either case there
        // is no further work to be done. If the source is a directory then it cannot have
        // been rmdir'ed or renamed; this is prohibited by the DE_RENAME flag.
        if !ptr::eq(x, ip) {
            if doingdirectory {
                panic(format_args!("rename: lost dir entry"));
            }
            vrele(a_fvp);
            if newparent {
                let _ = VOP_UNLOCK(fdvp);
            }
        } else {
            vrele(fvp);

            // First write a new entry in the destination directory and mark the entry in the
            // source directory as deleted. Then move the denode to the correct hash chain for
            // its new location in the filesystem. And, if we moved a directory, then update
            // its .. entry to point to the new parent directory.
            let oldname = ip.de_Name.get();
            ip.de_Name.set(toname); // update denode
            dp.de_fndoffset.set(to_diroffset);
            dp.de_fndcnt.set(to_count);
            error = createde(ip, dp, None, tcnp);
            if error.is_err() {
                ip.de_Name.set(oldname);
                if newparent {
                    let _ = VOP_UNLOCK(fdvp);
                }
                break 'body RenameExit::Bad;
            }
            ip.de_refcnt.set(ip.de_refcnt.get() + 1);
            zp.de_fndoffset.set(from_diroffset);
            error = removede(zp, ip);
            if error.is_err() {
                // XXX should really panic here, fs is corrupt
                if newparent {
                    let _ = VOP_UNLOCK(fdvp);
                }
                break 'body RenameExit::Bad;
            }

            cache_purge(fvp);

            if !doingdirectory {
                let mut dirclust = ip.de_dirclust.get();
                error = pcbmap(
                    dp,
                    de_cluster(pmp, to_diroffset),
                    None,
                    Some(&mut dirclust),
                    None,
                );
                ip.de_dirclust.set(dirclust);
                if error.is_err() {
                    // XXX should really panic here, fs is corrupt
                    if newparent {
                        let _ = VOP_UNLOCK(fdvp);
                    }
                    break 'body RenameExit::Bad;
                }
                ip.de_diroffset.set(to_diroffset);
                if ip.de_dirclust.get() != MSDOSFSROOT {
                    ip.de_diroffset
                        .set(ip.de_diroffset.get() & pmp.pm_crbomask.get());
                }
            }
            reinsert(ip);
            if newparent {
                let _ = VOP_UNLOCK(fdvp);
            }
        }

        // If we moved a directory to a new parent directory, then we must fixup the ".."
        // entry in the moved directory.
        if doingdirectory && newparent {
            let cn = ip.de_StartCluster.get();
            if cn == MSDOSFSROOT {
                // this should never happen
                panic(format_args!(
                    "msdosfs_rename: updating .. in root directory?"
                ));
            }
            let bn = cntobn(pmp, cn);
            let (bp, r) = bread(pmp.devvp(), Daddr::from(bn), pmp.pm_bpcluster.get() as i32);
            if let Err(e) = r {
                // XXX should really panic here, fs is corrupt
                error = Err(e);
                brelse(bp);
                break 'body RenameExit::Bad;
            }
            {
                // SAFETY: the buffer is busy for this function (from `bread`) and mapped; the
                // slice is not used after the buffer is written.
                let data = unsafe { bdata(bp) };
                putushort(&mut Direntry::at_mut(data, 0).deStartCluster, cn as u16);
                let mut pcl = dp.de_StartCluster.get();
                if fat32(pmp) && pcl == pmp.pm_rootdirblk.get() {
                    pcl = 0;
                }
                let dotdot = Direntry::SIZE;
                putushort(
                    &mut Direntry::at_mut(data, dotdot).deStartCluster,
                    pcl as u16,
                );
                if fat32(pmp) {
                    putushort(
                        &mut Direntry::at_mut(data, 0).deHighClust,
                        (cn >> 16) as u16,
                    );
                    putushort(
                        &mut Direntry::at_mut(data, dotdot).deHighClust,
                        (pcl >> 16) as u16,
                    );
                }
            }
            error = bwrite(bp);
            if error.is_err() {
                // XXX should really panic here, fs is corrupt
                break 'body RenameExit::Bad;
            }
        }

        VN_KNOTE(fvp, NOTE_RENAME);
        RenameExit::Bad
    };

    if exit == RenameExit::Bad {
        // bad:
        let _ = VOP_UNLOCK(fvp);
        vrele(fdvp);
    }
    if exit != RenameExit::Out {
        // bad1:
        if xp.is_some()
            && let Some(t) = tvp
        {
            vput(t);
        }
        vput(tdvp);
    }
    // out:
    ip.clr_flag(DE_RENAME);
    vrele(fvp);
    error
}

/// `msdosfs_mkdir` (`vop_mkdir`): make a directory: a cluster with its "." and ".." entries,
/// and its entry in the parent, which is released (`vput`) in all cases.
pub fn msdosfs_mkdir(ap: &mut VopMkdirArgs<'_>) -> Result<(), Errno> {
    let cnp = &mut *ap.a_cnp;
    let pdep = vtode(ap.a_dvp);
    let pmp = pdep.pmp();
    let bpcluster = pmp.pm_bpcluster.get();

    let error: Errno = 'bad2: {
        // If this is the root directory and there is no space left we can't do anything.
        // This is because the root directory can not change size.
        if pdep.de_StartCluster.get() == MSDOSFSROOT
            && pdep.de_fndoffset.get() >= pdep.de_FileSize.get()
        {
            break 'bad2 Errno::ENOSPC;
        }

        // Allocate a cluster to hold the about to be created directory.
        let mut newcluster: u32 = 0;
        if let Err(e) = clusteralloc(pmp, 0, 1, Some(&mut newcluster), None) {
            break 'bad2 e;
        }

        let ndirent = Denode::new();
        ndirent.de_pmp.set(Some(pmp));
        ndirent.de_flag.set(DE_ACCESS | DE_CREATE | DE_UPDATE);
        let ts = getnanotime();
        detimes(&ndirent, &ts, &ts, &ts);

        let error: Errno = 'bad: {
            // Now fill the cluster with the "." and ".." entries. And write the cluster to
            // disk. This way it is there for the parent directory to be pointing at if there
            // were a crash.
            let bn = cntobn(pmp, newcluster);
            // always succeeds
            let bp = getblk_wait(pmp.devvp(), Daddr::from(bn), bpcluster as i32);
            {
                // SAFETY: the buffer is busy for this function (from `getblk`) and mapped;
                // the slice is not used after the buffer is written.
                let data = unsafe { bdata(bp) };
                data[..bpcluster as usize].fill(0);
                let dotdot = Direntry::SIZE;
                *Direntry::at_mut(data, 0) = DOSDIRTEMPLATE[0];
                *Direntry::at_mut(data, dotdot) = DOSDIRTEMPLATE[1];

                let pcl = if fat32(pmp) && pdep.de_StartCluster.get() == pmp.pm_rootdirblk.get() {
                    0
                } else {
                    pdep.de_StartCluster.get()
                };
                for (off, cl) in [(0, newcluster), (dotdot, pcl)] {
                    let denp = Direntry::at_mut(data, off);
                    putushort(&mut denp.deStartCluster, cl as u16);
                    putushort(&mut denp.deCDate, ndirent.de_CDate.get());
                    putushort(&mut denp.deCTime, ndirent.de_CTime.get());
                    denp.deCTimeHundredth = ndirent.de_CTimeHundredth.get();
                    putushort(&mut denp.deADate, ndirent.de_ADate.get());
                    putushort(&mut denp.deMDate, ndirent.de_MDate.get());
                    putushort(&mut denp.deMTime, ndirent.de_MTime.get());
                }
                if fat32(pmp) {
                    putushort(
                        &mut Direntry::at_mut(data, 0).deHighClust,
                        (newcluster >> 16) as u16,
                    );
                    putushort(
                        &mut Direntry::at_mut(data, dotdot).deHighClust,
                        (pdep.de_StartCluster.get() >> 16) as u16,
                    );
                }
            }

            if let Err(e) = bwrite(bp) {
                break 'bad e;
            }

            // Now build up a directory entry pointing to the newly allocated cluster. This
            // will be written to an empty slot in the parent directory.
            #[cfg(feature = "diagnostic")]
            if cnp.cn_flags & crate::sys::namei::HASBUF == 0 {
                panic(format_args!("msdosfs_mkdir: no name"));
            }
            let mut name = [0u8; 11];
            if let Err(e) = uniqdosname(pdep, cnp, &mut name) {
                break 'bad e;
            }
            ndirent.de_Name.set(name);

            ndirent.de_Attributes.set(ATTR_DIRECTORY);
            ndirent.de_StartCluster.set(newcluster);
            ndirent.de_FileSize.set(0);
            ndirent.de_dev.set(pdep.de_dev.get());
            ndirent.de_devvp.set(pdep.de_devvp.get());
            let mut dep = None;
            if let Err(e) = createde(&ndirent, pdep, Some(&mut dep), cnp) {
                break 'bad e;
            }
            let Some(dep) = dep else {
                panic(format_args!("msdosfs_mkdir: no denode"));
            };
            if cnp.cn_flags & SAVESTART == 0 {
                pnbuf_free(cnp);
            }
            VN_KNOTE(ap.a_dvp, NOTE_WRITE | NOTE_LINK);
            vput(ap.a_dvp);
            *ap.a_vpp = Some(dep.detov());
            return Ok(());
        };

        // bad:
        let _ = clusterfree(pmp, newcluster, None);
        error
    };

    // bad2:
    pnbuf_free(cnp);
    vput(ap.a_dvp);
    Err(error)
}

/// `msdosfs_rmdir` (`vop_rmdir`): remove an empty directory; both vnodes are released.
pub fn msdosfs_rmdir(ap: &mut VopRmdirArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let dvp = ap.a_dvp;
    let cnp = &*ap.a_cnp;

    let ip = vtode(vp);
    let dp = vtode(dvp);
    let mut dvp_held = true;
    let error: Result<(), Errno> = 'out: {
        // Verify the directory is empty (and valid). (Rmdir ".." won't be valid since ".."
        // will contain a reference to the current directory and thus be non-empty.)
        if !dosdirempty(ip) || ip.de_flag.get() & DE_RENAME != 0 {
            break 'out Err(Errno::ENOTEMPTY);
        }

        VN_KNOTE(dvp, NOTE_WRITE | NOTE_LINK);

        // Delete the entry from the directory. For dos filesystems this gets rid of the
        // directory entry on disk, the in memory copy still exists but the de_refcnt is <= 0.
        // This prevents it from being found by deget(). When the vput() on dep is done we give
        // up access and eventually msdosfs_reclaim() will be called which will remove it from
        // the denode cache.
        if let Err(e) = removede(dp, ip) {
            break 'out Err(e);
        }
        // This is where we decrement the link count in the parent directory. Since dos
        // filesystems don't do this we just purge the name cache and let go of the parent
        // directory denode.
        cache_purge(dvp);
        vput(dvp);
        dvp_held = false;
        // Truncate the directory that is being deleted.
        let error = detrunc(ip, 0, IO_SYNC, cnp.cn_cred, cn_proc(cnp));
        cache_purge(vp);
        error
    };
    // out:
    if dvp_held {
        vput(dvp);
    }
    VN_KNOTE(vp, NOTE_DELETE);
    vput(vp);
    error
}

/// `msdosfs_symlink` (`vop_symlink`): DOS filesystems don't know what symlinks are.
pub fn msdosfs_symlink(ap: &mut VopSymlinkArgs<'_>) -> Result<(), Errno> {
    let _ = VOP_ABORTOP(ap.a_dvp, ap.a_cnp);
    vput(ap.a_dvp);
    Err(Errno::EOPNOTSUPP)
}

/// `uiomove(dp, dp->d_reclen, uio)`: copy out the first `d_reclen` bytes of a
/// `struct dirent`.
fn dirent_uiomove(dp: &Dirent, uio: &mut Uio<'_>) -> Result<(), Errno> {
    let mut b = [0u8; size_of::<Dirent>()];
    let at = |f: usize, len: usize| f..f + len;
    b[at(offset_of!(Dirent, d_fileno), 8)].copy_from_slice(&dp.d_fileno.to_ne_bytes());
    b[at(offset_of!(Dirent, d_off), 8)].copy_from_slice(&dp.d_off.to_ne_bytes());
    b[at(offset_of!(Dirent, d_reclen), 2)].copy_from_slice(&dp.d_reclen.to_ne_bytes());
    b[offset_of!(Dirent, d_type)] = dp.d_type;
    b[offset_of!(Dirent, d_namlen)] = dp.d_namlen;
    b[at(offset_of!(Dirent, __d_padding), 4)].copy_from_slice(&dp.__d_padding);
    b[at(Dirent::NAME_OFFSET, MAXNAMLEN + 1)].copy_from_slice(&dp.d_name);
    let reclen = usize::from(dp.d_reclen).min(b.len());
    uiomove(&mut b[..reclen], uio)
}

/// `msdosfs_readdir` (`vop_readdir`): convert the DOS directory entries (with their Win95
/// long names) to `struct dirent`s. The root directory, which has no "." and "..", gets
/// them simulated at offsets 0 and 32 (`bias`).
pub fn msdosfs_readdir(ap: &mut VopReaddirArgs<'_, '_>) -> Result<(), Errno> {
    let dep = vtode(ap.a_vp);
    let pmp = dep.pmp();
    let uio = &mut *ap.a_uio;
    let mut bias: i64 = 0;
    let mut wlast: i64 = -1;
    let mut chksum: i32 = -1;
    let dsize = i64::from(DIRENTRY_SIZE);

    // msdosfs_readdir() won't operate properly on regular files since it does i/o only with
    // the filesystem vnode, and hence can retrieve the wrong block from the buffer cache for a
    // plain file. So, fail attempts to readdir() on a plain file.
    if dep.de_Attributes.get() & ATTR_DIRECTORY == 0 {
        return Err(Errno::ENOTDIR);
    }

    // To be safe, initialize dirbuf
    let mut dirbuf = Dirent {
        d_fileno: 0,
        d_off: 0,
        d_reclen: 0,
        d_type: 0,
        d_namlen: 0,
        __d_padding: [0; 4],
        d_name: [0; MAXNAMLEN + 1],
    };

    // If the user buffer is smaller than the size of one dos directory entry or the file
    // offset is not a multiple of the size of a directory entry, then we fail the read.
    let count = uio.uio_resid & !(Direntry::SIZE - 1);
    let mut offset = uio.uio_offset;
    if count < Direntry::SIZE || offset & (dsize - 1) != 0 {
        return Err(Errno::EINVAL);
    }
    let lost = uio.uio_resid - count;
    uio.uio_resid = count;

    let dirsperblk = u32::from(pmp.pm_BytesPerSec()) / DIRENTRY_SIZE;
    let shortname = pmp.pm_flags.get() & MSDOSFSMNT_SHORTNAME as u32 != 0;

    let error: Result<(), Errno> = 'out: {
        // If they are reading from the root directory then, we simulate the . and .. entries
        // since these don't exist in the root directory. We also set the offset bias to make
        // up for having to simulate these entries. By this I mean that at file offset 64 we
        // read the first entry in the root directory that lives on disk.
        if dep.de_StartCluster.get() == MSDOSFSROOT
            || (fat32(pmp) && dep.de_StartCluster.get() == pmp.pm_rootdirblk.get())
        {
            bias = 2 * dsize;
            if offset < bias {
                for n in offset / dsize..2 {
                    dirbuf.d_fileno = if fat32(pmp) {
                        u64::from(pmp.pm_rootdirblk.get())
                    } else {
                        1
                    };
                    dirbuf.d_type = DT_DIR;
                    let name: &[u8] = if n == 0 { b"." } else { b".." };
                    dirbuf.d_namlen = name.len() as u8;
                    // strlcpy(dirbuf.d_name, name, sizeof dirbuf.d_name)
                    dirbuf.d_name[..name.len()].copy_from_slice(name);
                    dirbuf.d_name[name.len()] = 0;
                    dirbuf.d_reclen = dirent_size(&dirbuf) as u16;
                    dirbuf.d_off = offset + dsize;
                    if uio.uio_resid < usize::from(dirbuf.d_reclen) {
                        break 'out Ok(());
                    }
                    if let Err(e) = dirent_uiomove(&dirbuf, uio) {
                        break 'out Err(e);
                    }
                    offset = dirbuf.d_off;
                }
            }
        }

        let mut error = Ok(());
        while uio.uio_resid > 0 {
            let lbn = de_cluster(pmp, offset - bias) as u32;
            let on = (offset - bias) & i64::from(pmp.pm_crbomask.get());
            let mut n = i64::from(
                ((i64::from(pmp.pm_bpcluster.get()) - on) as u32).min(uio.uio_resid as u32),
            );
            let diff = (i64::from(dep.de_FileSize.get()) - (offset - bias)) as i32;
            if diff <= 0 {
                break;
            }
            n = i64::from((n as u32).min(diff as u32));
            let mut bn: Daddr = 0;
            let mut cn: u32 = 0;
            let mut blsize: i32 = 0;
            error = pcbmap(dep, lbn, Some(&mut bn), Some(&mut cn), Some(&mut blsize));
            if error.is_err() {
                break;
            }
            let (bp, r) = bread(pmp.devvp(), bn, blsize);
            if let Err(e) = r {
                brelse(bp);
                return Err(e);
            }
            n = i64::from((n as u32).min((blsize as u32).wrapping_sub(bp.b_resid.get() as u32)));

            // SAFETY: the buffer is busy for this function (from `bread`) and mapped; the
            // slice is not used after the buffer is released.
            let data = unsafe { bdata(bp) };

            // Convert from dos directory entries to fs-independent directory entries.
            let mut pos = on as usize;
            while pos < (on + n) as usize {
                'next: {
                    let dentp = Direntry::at(data, pos);
                    // If this is an unused entry, we can stop.
                    if dentp.deName[0] == SLOT_EMPTY {
                        brelse(bp);
                        break 'out Ok(());
                    }
                    // Skip deleted entries.
                    if dentp.deName[0] == SLOT_DELETED {
                        chksum = -1;
                        wlast = -1;
                        break 'next;
                    }

                    // Handle Win95 long directory entries
                    if dentp.deAttributes == ATTR_WIN95 {
                        if shortname {
                            break 'next;
                        }
                        let wep = Winentry::at(data, pos);
                        chksum = win2unixfn(wep, &mut dirbuf, chksum);
                        if wep.weCnt & WIN_LAST != 0 {
                            wlast = offset;
                        }
                        break 'next;
                    }

                    // Skip volume labels
                    if dentp.deAttributes & ATTR_VOLUME != 0 {
                        chksum = -1;
                        wlast = -1;
                        break 'next;
                    }

                    // This computation of d_fileno must match the computation of va_fileid in
                    // msdosfs_getattr.
                    let mut fileno = u32::from(getushort(&dentp.deStartCluster));
                    if fat32(pmp) {
                        fileno |= u32::from(getushort(&dentp.deHighClust)) << 16;
                    }

                    if dentp.deAttributes & ATTR_DIRECTORY != 0 {
                        // Special-case root
                        if fileno == MSDOSFSROOT {
                            fileno = if fat32(pmp) {
                                pmp.pm_rootdirblk.get()
                            } else {
                                1
                            };
                        }

                        dirbuf.d_fileno = u64::from(fileno);
                        dirbuf.d_type = DT_DIR;
                    } else {
                        if getulong(&dentp.deFileSize) == 0 {
                            let mut fileno64 = u64::from(if cn == MSDOSFSROOT {
                                roottobn(pmp, 0u32)
                            } else {
                                cntobn(pmp, cn)
                            });

                            fileno64 = fileno64.wrapping_mul(u64::from(dirsperblk));
                            fileno64 = fileno64.wrapping_add((pos / Direntry::SIZE) as u64);

                            fileno = fileidhash(fileno64);
                        }

                        dirbuf.d_fileno = u64::from(fileno);
                        dirbuf.d_type = DT_REG;
                    }

                    let name11 = dentp.name11();
                    if chksum != i32::from(winChksum(&name11)) {
                        dirbuf.d_namlen = dos2unixfn(&name11, &mut dirbuf.d_name, shortname) as u8;
                    } else {
                        dirbuf.d_name[usize::from(dirbuf.d_namlen)] = 0;
                    }
                    chksum = -1;
                    dirbuf.d_reclen = dirent_size(&dirbuf) as u16;
                    dirbuf.d_off = offset + dsize;
                    if uio.uio_resid < usize::from(dirbuf.d_reclen) {
                        brelse(bp);
                        // Remember long-name offset.
                        if wlast != -1 {
                            offset = wlast;
                        }
                        break 'out Ok(());
                    }
                    wlast = -1;
                    if let Err(e) = dirent_uiomove(&dirbuf, uio) {
                        brelse(bp);
                        break 'out Err(e);
                    }
                }
                pos += Direntry::SIZE;
                offset += dsize;
            }
            brelse(bp);
        }
        error
    };

    // out:
    uio.uio_offset = offset;
    uio.uio_resid += lost;
    *ap.a_eofflag = i32::from(i64::from(dep.de_FileSize.get()) - (offset - bias) <= 0);
    error
}

/// `msdosfs_readlink` (`vop_readlink`): DOS filesystems don't know what symlinks are.
pub fn msdosfs_readlink(_ap: &mut VopReadlinkArgs<'_, '_>) -> Result<(), Errno> {
    Err(Errno::EINVAL)
}

/// `msdosfs_lock` (`vop_lock`): take the denode's lock (a recursive rwlock: the thread that
/// holds it may take it again).
pub fn msdosfs_lock(ap: &mut VopLockArgs) -> Result<(), Errno> {
    rrw_enter(&vtode(ap.a_vp).de_lock, ap.a_flags & LK_RWFLAGS)
}

/// `msdosfs_unlock` (`vop_unlock`).
pub fn msdosfs_unlock(ap: &mut VopUnlockArgs) -> Result<(), Errno> {
    rrw_exit(&vtode(ap.a_vp).de_lock);
    Ok(())
}

/// `msdosfs_islocked` (`vop_islocked`).
pub fn msdosfs_islocked(ap: &mut VopIslockedArgs) -> i32 {
    rrw_status(&vtode(ap.a_vp).de_lock)
}

/// `msdosfs_bmap` (`vop_bmap`): the device vnode holding the file system (`a_vpp`) and the
/// file system relative block number of the file's cluster `a_bn` (`a_bnp`).
pub fn msdosfs_bmap(ap: &mut VopBmapArgs<'_>) -> Result<(), Errno> {
    let dep = vtode(ap.a_vp);

    if let Some(vpp) = ap.a_vpp.as_deref_mut() {
        *vpp = dep.de_devvp.get();
    }
    let Some(bnp) = ap.a_bnp.as_deref_mut() else {
        return Ok(());
    };

    let cn = ap.a_bn as u32;
    if i64::from(cn) != ap.a_bn {
        return Err(Errno::EFBIG);
    }

    msdosfs_bmaparray(ap.a_vp, cn, bnp, ap.a_runp.as_deref_mut())
}

/// `msdosfs_bmaparray(vp, cn, bnp, runp)`: map cluster `cn` of the file to a block number
/// (`bnp`) and, with `runp`, count the clusters after it that follow on the disk.
pub fn msdosfs_bmaparray(
    vp: &'static Vnode,
    cn: u32,
    bnp: &mut Daddr,
    mut runp: Option<&mut i32>,
) -> Result<(), Errno> {
    let dep = vtode(vp);
    let pmp = dep.pmp();
    let mut maxrun: i32 = 0;

    let mp = vmount(vp);

    if let Some(r) = runp.as_deref_mut() {
        // XXX
        // If MAXBSIZE is the largest transfer the disks can handle, we probably want maxrun
        // to be 1 block less so that we don't create a block larger than the device can
        // handle.
        *r = 0;
        maxrun = ((MAXBSIZE as u32 / mp.mnt_stat.get().f_iosize).wrapping_sub(1))
            .min(pmp.pm_maxcluster.get().wrapping_sub(cn)) as i32;
    }

    pcbmap(dep, cn, Some(&mut *bnp), None, None)?;

    let mut run: i32 = 1;
    while run <= maxrun {
        let mut runbn: Daddr = 0;
        if pcbmap(
            dep,
            cn.wrapping_add(run as u32),
            Some(&mut runbn),
            None,
            None,
        )
        .is_err()
            || runbn != *bnp + de_cn2bn(pmp, i64::from(run))
        {
            break;
        }
        run += 1;
    }

    if let Some(r) = runp {
        *r = run - 1;
    }

    Ok(())
}

/// `msdosfs_strategy` (`vop_strategy`): map the buffer's cluster to the disk if not done yet,
/// then pass it to the device.
pub fn msdosfs_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    let bp = ap.a_bp;
    let Some(bvp) = bp.b_vp.get() else {
        panic(format_args!("msdosfs_strategy: buffer without a vnode"));
    };
    let dep = vtode(bvp);
    let mut error = Ok(());

    if bvp.v_type.get() == VBLK || bvp.v_type.get() == VCHR {
        panic(format_args!("msdosfs_strategy: spec"));
    }
    // If we don't already know the filesystem relative block number then get it using
    // pcbmap(). If pcbmap() returns the block number as -1 then we've got a hole in the file.
    // DOS filesystems don't allow files with holes, so we shouldn't ever see this.
    if bp.b_blkno.get() == bp.b_lblkno.get() {
        let mut blkno = bp.b_blkno.get();
        error = pcbmap(dep, bp.b_lblkno.get() as u32, Some(&mut blkno), None, None);
        bp.b_blkno.set(if error.is_err() { -1 } else { blkno });
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
        return error;
    }

    // Read/write the block from/to the disk that contains the desired file block.
    let Some(vp) = dep.de_devvp.get() else {
        panic(format_args!("msdosfs_strategy: denode without a device"));
    };
    bp.b_dev.set(vp.v_rdev());
    let _ = VOP_STRATEGY(vp, bp);
    Ok(())
}

/// `msdosfs_print` (`vop_print`): print out the contents of a denode.
pub fn msdosfs_print(ap: &mut VopPrintArgs) -> Result<(), Errno> {
    #[cfg(any(feature = "debug", feature = "diagnostic"))]
    {
        use crate::sys::types::{major, minor};
        let dep = vtode(ap.a_vp);

        crate::kprintf!(
            "tag VT_MSDOSFS, startcluster {}, dircluster {}, diroffset {} ",
            dep.de_StartCluster.get(),
            dep.de_dirclust.get(),
            dep.de_diroffset.get()
        );
        crate::kprintf!(
            " dev {}, {}, {}\n",
            major(dep.de_dev.get()),
            minor(dep.de_dev.get()),
            if VOP_ISLOCKED(ap.a_vp) != 0 {
                "(LOCKED)"
            } else {
                ""
            }
        );
        #[cfg(feature = "diagnostic")]
        crate::kprintf!("\n");
    }
    #[cfg(not(any(feature = "debug", feature = "diagnostic")))]
    let _ = ap;

    Ok(())
}

/// `msdosfs_advlock` (`vop_advlock`): advisory record locking support.
pub fn msdosfs_advlock(ap: &mut VopAdvlockArgs<'_>) -> Result<(), Errno> {
    let dep = vtode(ap.a_vp);

    lf_advlock(
        &dep.de_lockf,
        i64::from(dep.de_FileSize.get()),
        ap.a_id,
        ap.a_op,
        ap.a_fl,
        ap.a_flags,
    )
}

/// `msdosfs_pathconf` (`vop_pathconf`).
pub fn msdosfs_pathconf(ap: &mut VopPathconfArgs<'_>) -> Result<(), Errno> {
    let pmp = vtode(ap.a_vp).pmp();

    *ap.a_retval = match ap.a_name {
        _PC_LINK_MAX => 1,
        _PC_NAME_MAX => {
            if pmp.pm_flags.get() & MSDOSFSMNT_LONGNAME as u32 != 0 {
                WIN_MAXLEN as Register
            } else {
                12
            }
        }
        _PC_CHOWN_RESTRICTED => 1,
        _PC_NO_TRUNC => 0,
        _PC_TIMESTAMP_RESOLUTION => 2_000_000_000, // 2 billion nanoseconds
        _ => return Err(Errno::EINVAL),
    };

    Ok(())
}

/// `fileidhash`: Thomas Wang's hash function, severely hacked to always set the high bit on
/// the number it returns (so no longer a proper hash function).
fn fileidhash(mut fileid: u64) -> u32 {
    let c1: u64 = 0x6e5ea73858134343;
    let c2: u64 = 0xb34e8f99a2ec9ef5;

    // We now have the original fileid value, as 64-bit value. We need to reduce it to
    // 32-bits, with the top bit set.
    fileid ^= (c1 ^ fileid) >> 32;
    fileid = fileid.wrapping_mul(c1);
    fileid ^= (c2 ^ fileid) >> 31;
    fileid = fileid.wrapping_mul(c2);
    fileid ^= (c1 ^ fileid) >> 32;

    (fileid | 0x80000000) as u32
}

/// `msdosfs_kqfilter` (`vop_kqfilter`): attach a knote to the vnode.
pub fn msdosfs_kqfilter(ap: &mut VopKqfilterArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let kn = ap.a_kn;

    match kn.kn_filter().get() {
        EVFILT_READ => kn.kn_fop.set(Some(&MSDOSFSREAD_FILTOPS)),
        EVFILT_WRITE => kn.kn_fop.set(Some(&MSDOSFSWRITE_FILTOPS)),
        EVFILT_VNODE => kn.kn_fop.set(Some(&MSDOSFSVNODE_FILTOPS)),
        _ => return Err(Errno::EINVAL),
    }

    kn.kn_hook.set(ptr::from_ref(vp).cast_mut().cast());

    klist_insert_locked(&vp.v_klist, kn);

    Ok(())
}

/// `kn->kn_hook` of a msdosfs knote: its vnode.
fn kn_vnode(kn: &Knote) -> &'static Vnode {
    // SAFETY: `msdosfs_kqfilter` points `kn_hook` at the vnode, a `vnode_pool` item that is
    // never freed.
    match unsafe { kn.kn_hook.get().cast::<Vnode>().as_ref() } {
        Some(vp) => vp,
        None => panic(format_args!("knote {:p}: no vnode", kn)),
    }
}

/// `filt_msdosfsdetach`: unhooks the knote from the vnode.
pub fn filt_msdosfsdetach(kn: &Knote) {
    let vp = kn_vnode(kn);

    klist_remove_locked(&vp.v_klist, kn);
}

/// `filt_msdosfsread`: the bytes past the file offset; always ready for poll and select.
pub fn filt_msdosfsread(kn: &Knote, hint: i64) -> bool {
    let vp = kn_vnode(kn);
    let dep = vtode(vp);

    // filesystem is gone, so set the EOF flag and schedule the knote for deletion.
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF | EV_ONESHOT);
        return true;
    }

    kn.kn_data()
        .set(i64::from(dep.de_FileSize.get()) - foffset(kn.fp()));
    if kn.kn_data().get() == 0 && kn.kn_sfflags.get() & NOTE_EOF != 0 {
        kn.kn_fflags().set(kn.kn_fflags().get() | NOTE_EOF);
        return true;
    }

    if kn.has_flags(__EV_POLL | __EV_SELECT) {
        return true;
    }

    kn.kn_data().get() != 0
}

/// `filt_msdosfswrite`: a file is always writable.
pub fn filt_msdosfswrite(kn: &Knote, hint: i64) -> bool {
    // filesystem is gone, so set the EOF flag and schedule the knote for deletion.
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF | EV_ONESHOT);
        return true;
    }

    kn.kn_data().set(0);
    true
}

/// `filt_msdosfsvnode`: records the vnode events (`NOTE_*`) the user asked for.
pub fn filt_msdosfsvnode(kn: &Knote, hint: i64) -> bool {
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the msdosfs vnode operations, through the system calls' own paths
    // (`namei`, `vn_open`, `vn_rdwr`, `domkdirat`, `dorenameat`, `dounlinkat`, ...) over a FAT
    // image (`mkfat`) on the fake disk of `msdosfs_vfsops.rs`'s tests, mounted as the root; one
    // test mounts it on a directory of a tmpfs root and looks up across the mount point. What
    // the operations write is checked on the disk image after a `msdosfs_sync`.

    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq, assert_ne};

    use super::*;
    use crate::kern::kern_prot::crget;
    use crate::kern::vfs_init::set_rootvnode;
    use crate::kern::vfs_lookup::{namei, ndinit};
    use crate::kern::vfs_subr::{vattr_null, vref};
    use crate::kern::vfs_syscalls::{domkdirat, dorenameat, dounlinkat};
    use crate::kern::vfs_vnops::{vn_close, vn_open, vn_rdwr};
    use crate::kern::vfs_vops::{
        VOP_GETATTR, VOP_KQFILTER, VOP_PATHCONF, VOP_READDIR, VOP_SETATTR,
    };
    use crate::msdosfs::msdosfs_vfsops::tests::mkfat::{self, Dir, Image};
    use crate::msdosfs::msdosfs_vfsops::tests::{DISK, mount, setup};
    use crate::msdosfs::msdosfs_vfsops::{msdosfs_root, msdosfs_statfs, msdosfs_sync};
    use crate::sys::fcntl::{AT_FDCWD, AT_REMOVEDIR, FREAD, FWRITE, O_CREAT};
    use crate::sys::lock::LK_RECURSEFAIL;
    use crate::sys::namei::{FOLLOW, LOCKLEAF, LOOKUP, NiDirp};
    use crate::sys::time::Timespec;
    use crate::sys::uio::{Iovec, UioRw, UioSeg};
    use crate::sys::vnode::Vattr;

    /// The long-named file of the smoke image, and its DOS name.
    const FATNAME: &[u8] = b"m10c-fat.txt";
    const FATSHORT: &[u8; 11] = b"M10C-FATTXT";
    const FATDATA: &[u8] = b"m10c-fat-42";

    /// `n` bytes of a pattern that does not repeat within a cluster.
    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    /// A FAT12 image: the smoke's long-named file, a 3000-byte file (6 clusters) and a
    /// subdirectory holding a long-named file.
    fn sample() -> Vec<u8> {
        let mut img = Image::new(mkfat::FAT12_1M);
        let root = img.root();
        img.add_file(root, Some(FATNAME), FATSHORT, FATDATA);
        img.add_file(root, None, b"README  TXT", &pattern(3000));
        let sub = img.mkdir(root, b"SUBDIR     ", 1);
        img.add_file(
            Dir::Clust(sub),
            Some(b"inner file.dat"),
            b"INNERF~1DAT",
            b"inside",
        );
        img.finish()
    }

    /// The test setup plus `image` mounted read-write as "/" (long names, mask 0777, owned by
    /// root) and made the thread's current directory. `mnt_stat` is filled as `sys_mount` does.
    fn setup_root(image: Vec<u8>) -> (MutexGuard<'static, ()>, &'static Proc, &'static Mount) {
        let (g, p) = setup(image);
        let mp = mount(p, false);
        let pmp = vfstomsdosfs(mp);
        pmp.pm_flags
            .set(pmp.pm_flags.get() | MSDOSFSMNT_LONGNAME as u32);
        pmp.pm_mask.set(0o777);
        let mut sb = mp.mnt_stat.get();
        msdosfs_statfs(mp, &mut sb, p).expect("statfs");
        mp.mnt_stat.set(sb);
        let root = msdosfs_root(mp).expect("root");
        set_rootvnode(Some(root));
        p.fd().fd_cdir.set(Some(root));
        vref(root);
        let _ = VOP_UNLOCK(root);
        (g, p, mp)
    }

    /// A user-space path for the `do*at` functions: the bytes and a NUL.
    fn c(path: &str) -> Vec<u8> {
        let mut v = path.as_bytes().to_vec();
        v.push(0);
        v
    }

    /// `open(path, O_RDWR | O_CREAT, mode)`: the vnode, unlocked and referenced.
    fn create(p: &'static Proc, path: &[u8], mode: Mode) -> &'static Vnode {
        let mut nd = ndinit(0, 0, NiDirp::Sys(path), p);
        vn_open(&mut nd, FREAD | FWRITE | O_CREAT, mode).expect("create");
        let vp = nd.ni_vp.expect("a vnode");
        let _ = VOP_UNLOCK(vp);
        vp
    }

    /// `close` of a vnode `create` gave.
    fn close(p: &'static Proc, vp: &'static Vnode) {
        vn_close(vp, FREAD | FWRITE, p.ucred(), Some(p)).expect("close");
    }

    /// The vnode `path` names, unlocked and referenced.
    fn lookup(p: &'static Proc, path: &[u8]) -> Result<&'static Vnode, Errno> {
        let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(path), p);
        namei(&mut nd)?;
        Ok(nd.ni_vp.expect("a vnode"))
    }

    /// `stat(path)`.
    fn stat(p: &'static Proc, path: &[u8]) -> Result<Vattr, Errno> {
        let mut nd = ndinit(LOOKUP, LOCKLEAF | FOLLOW, NiDirp::Sys(path), p);
        namei(&mut nd)?;
        let vp = nd.ni_vp.expect("a vnode");
        let mut va = Vattr::new();
        let error = VOP_GETATTR(vp, &mut va, p.ucred(), p);
        vput(vp);
        error.map(|()| va)
    }

    /// `vn_rdwr` of `len` bytes at `off` of `vp` (unlocked): the bytes moved. A write runs
    /// without a thread (`uio_procp`), as the test process has no resource limits for
    /// `vn_fsizechk` to look at.
    fn rdwr(p: &'static Proc, rw: UioRw, vp: &'static Vnode, buf: &mut [u8], off: i64) -> usize {
        let mut resid = 0;
        let procp = if rw == UioRw::UIO_WRITE {
            None
        } else {
            Some(p)
        };
        vn_rdwr(
            rw,
            vp,
            buf.as_mut_ptr().cast(),
            buf.len(),
            off,
            UioSeg::UIO_SYSSPACE,
            0,
            p.ucred(),
            Some(&mut resid),
            procp,
        )
        .expect("vn_rdwr");
        buf.len() - resid
    }

    /// The whole contents of the file `path`.
    fn read_file(p: &'static Proc, path: &[u8]) -> Vec<u8> {
        let size = stat(p, path).expect("stat").va_size as usize;
        let vp = lookup(p, path).expect("lookup");
        let mut buf = std::vec![0u8; size + 100];
        let n = rdwr(p, UioRw::UIO_READ, vp, &mut buf, 0);
        vrele(vp);
        buf.truncate(n);
        buf
    }

    /// `truncate(vp, size)` through `VOP_SETATTR`.
    fn truncate(p: &'static Proc, vp: &'static Vnode, size: u64) -> Result<(), Errno> {
        let mut va = Vattr::new();
        vattr_null(&mut va);
        va.va_size = size;
        setattr(p, vp, &mut va)
    }

    /// `VOP_SETATTR` with the vnode locked.
    fn setattr(p: &'static Proc, vp: &'static Vnode, va: &mut Vattr) -> Result<(), Errno> {
        let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
        let error = VOP_SETATTR(vp, va, p.ucred(), p);
        let _ = VOP_UNLOCK(vp);
        error
    }

    /// One `struct dirent` `getdents` returned.
    #[derive(Debug, PartialEq, Eq)]
    struct Ent {
        name: Vec<u8>,
        off: i64,
        fileno: u64,
        typ: u8,
    }

    /// One `VOP_READDIR` of the directory `path` at `offset` into a buffer of `size` bytes: the
    /// entries, the new offset and the EOF flag.
    fn readdir(p: &'static Proc, path: &[u8], offset: i64, size: usize) -> (Vec<Ent>, i64, i32) {
        let mut nd = ndinit(LOOKUP, LOCKLEAF | FOLLOW, NiDirp::Sys(path), p);
        namei(&mut nd).expect("lookup");
        let vp = nd.ni_vp.expect("a vnode");
        let mut buf = std::vec![0u8; size];
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: size,
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: offset,
            uio_resid: size,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let mut eof = 0;
        VOP_READDIR(vp, &mut uio, p.ucred(), &mut eof).expect("readdir");
        let used = size - uio.uio_resid;
        let newoff = uio.uio_offset;
        vput(vp);

        let mut ents = Vec::new();
        let mut off = 0;
        while off < used {
            let d = Dirent::from_bytes(&buf[off..]).expect("a dirent");
            let name = &buf[off + Dirent::NAME_OFFSET..][..usize::from(d.d_namlen)];
            assert_eq!(
                buf[off + Dirent::NAME_OFFSET + name.len()],
                0,
                "NUL-terminated"
            );
            ents.push(Ent {
                name: name.to_vec(),
                off: d.d_off,
                fileno: d.d_fileno,
                typ: d.d_type,
            });
            off += usize::from(d.d_reclen);
        }
        (ents, newoff, eof)
    }

    /// The names of `readdir` entries.
    fn names(ents: &[Ent]) -> Vec<&[u8]> {
        ents.iter().map(|e| &e.name[..]).collect()
    }

    /// Writes everything back (`msdosfs_sync` with `MNT_WAIT`) and returns the disk as an image.
    fn synced(p: &'static Proc, mp: &'static Mount) -> Image {
        msdosfs_sync(mp, MNT_WAIT, 0, p.ucred(), p).expect("sync");
        let mut img = Image::new(mkfat::FAT12_1M);
        img.disk = DISK.lock().unwrap_or_else(|e| e.into_inner()).clone();
        img
    }

    /// The short entry named `short` in `dir` of `img`.
    fn entry(img: &Image, dir: Dir, short: &[u8; 11]) -> Option<Direntry> {
        img.slots(dir)
            .into_iter()
            .map(|o| *Direntry::at(&img.disk, o))
            .find(|e| e.name11() == *short)
    }

    /// The data of the file whose entry is `e`, following its cluster chain on `img`.
    fn contents(img: &Image, e: &Direntry) -> Vec<u8> {
        let size = getulong(&e.deFileSize) as usize;
        let mut cn = u32::from(getushort(&e.deStartCluster));
        let mut out = Vec::new();
        while out.len() < size {
            let o = img.clust_off(cn);
            out.extend_from_slice(&img.disk[o..o + img.bpc()]);
            cn = img.fat_get(cn);
        }
        out.truncate(size);
        out
    }

    #[test]
    fn the_long_named_file_is_found_and_read() {
        let (_g, p, _mp) = setup_root(sample());

        // the smoke: cat /mnt/m10c-fat.txt
        assert_eq!(read_file(p, FATNAME), FATDATA);
        // a lookup by the DOS name finds the same file, case-insensitively
        assert_eq!(read_file(p, b"/m10c-fat.txt"), FATDATA);
        assert_eq!(read_file(p, b"/M10C-FAT.TXT"), FATDATA);
        // several clusters: read whole (bread_cluster), and from the middle of a cluster
        assert_eq!(read_file(p, b"/readme.txt"), pattern(3000));
        let vp = lookup(p, b"/README.TXT").expect("lookup");
        let mut buf = [0u8; 700];
        assert_eq!(rdwr(p, UioRw::UIO_READ, vp, &mut buf, 1000), 700);
        assert_eq!(&buf[..], &pattern(3000)[1000..1700]);
        // a read past the end moves nothing
        assert_eq!(rdwr(p, UioRw::UIO_READ, vp, &mut buf, 3000), 0);
        vrele(vp);
        // through a subdirectory
        assert_eq!(read_file(p, b"/subdir/inner file.dat"), b"inside");
        assert_eq!(lookup(p, b"/nonesuch").err(), Some(Errno::ENOENT));
    }

    #[test]
    fn readdir_lists_long_names_with_cookies_to_resume_from() {
        let (_g, p, _mp) = setup_root(sample());

        let (ents, off, eof) = readdir(p, b"/", 0, 4096);
        assert_eq!(
            names(&ents),
            [&b"."[..], b"..", FATNAME, b"README.TXT", b"SUBDIR"]
        );
        // "." and ".." are simulated in the root (fileno 1); the cookies then count 32-byte
        // slots past them, the long name entry included
        assert_eq!((ents[0].fileno, ents[0].typ, ents[0].off), (1, DT_DIR, 32));
        assert_eq!((ents[1].fileno, ents[1].off), (1, 64));
        assert_eq!((ents[2].off, ents[2].typ), (64 + 2 * 32, DT_REG));
        assert_eq!(ents[3].off, 64 + 3 * 32);
        assert_eq!((ents[4].off, ents[4].typ), (64 + 4 * 32, DT_DIR));
        assert_eq!(off, 64 + 4 * 32, "stopped at the first never-used slot");
        assert_eq!(eof, 0, "the fixed root directory goes on");
        // d_fileno is what getattr calls va_fileid
        for (i, path) in [
            (2, &b"/m10c-fat.txt"[..]),
            (3, b"/README.TXT"),
            (4, b"/SUBDIR"),
        ] {
            assert_eq!(stat(p, path).expect("stat").va_fileid, ents[i].fileno);
        }

        // resume from each cookie
        for i in 0..4 {
            let (rest, _, _) = readdir(p, b"/", ents[i].off, 4096);
            assert_eq!(names(&rest), names(&ents[i + 1..]), "from cookie {i}");
        }

        // a buffer too small for the long-named entry ends before its long name entry, so the
        // next call reads the long name again
        // (96 bytes: "." and ".." take 64, the 32 left are short of the long name's 40)
        let small = 96;
        assert!(small - 2 * dirent_size_of(2) < dirent_size_of(FATNAME.len()));
        let (first, off, _) = readdir(p, b"/", 0, small);
        assert_eq!(names(&first), [&b"."[..], b".."]);
        assert_eq!(off, 64, "the long name's first slot");
        let (next, _, _) = readdir(p, b"/", off, 4096);
        assert_eq!(names(&next)[0], FATNAME);

        // a subdirectory has real "." and ".." entries
        let (sub, _, _) = readdir(p, b"/subdir", 0, 4096);
        assert_eq!(names(&sub), [&b"."[..], b"..", b"inner file.dat"]);
        assert_eq!(sub[1].fileno, 1, "`..` of a directory in the root");

        // bad offsets and buffers, and a regular file
        let vp = lookup(p, b"/").expect("root");
        let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
        let mut buf = [0u8; 64];
        for (offset, len, want) in [(3, 64, Errno::EINVAL), (0, 16, Errno::EINVAL)] {
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
            assert_eq!(VOP_READDIR(vp, &mut uio, p.ucred(), &mut eof), Err(want));
        }
        vput(vp);
    }

    /// `DIRENT_SIZE` of an entry with a name of `namlen` bytes.
    fn dirent_size_of(namlen: usize) -> usize {
        crate::sys::dirent::dirent_recsize(namlen)
    }

    #[test]
    fn files_are_created_written_read_back_and_truncated() {
        let (_g, p, mp) = setup_root(sample());
        let pmp = vfstomsdosfs(mp);
        let free0 = pmp.pm_freeclustercount.get();

        let vp = create(p, b"/A Longer Name.bin", 0o644);
        let data = pattern(5000);
        let mut w = data.clone();
        assert_eq!(rdwr(p, UioRw::UIO_WRITE, vp, &mut w, 0), 5000);
        assert_eq!(
            free0 - pmp.pm_freeclustercount.get(),
            10,
            "5000 bytes in 512-byte clusters"
        );
        assert_eq!(read_file(p, b"/a longer name.bin"), data);

        // overwrite inside, then append past the end
        let mut w = std::vec![0xaa; 100];
        rdwr(p, UioRw::UIO_WRITE, vp, &mut w, 500);
        let mut want = data.clone();
        want[500..600].fill(0xaa);
        assert_eq!(read_file(p, b"/A Longer Name.bin"), want);
        // a write beyond EOF fills the hole with zeroes (DOS has no holes)
        let mut w = std::vec![0x55; 10];
        rdwr(p, UioRw::UIO_WRITE, vp, &mut w, 6000);
        want.resize(6000, 0);
        want.extend_from_slice(&[0x55; 10]);
        assert_eq!(read_file(p, b"/A Longer Name.bin"), want);

        truncate(p, vp, 100).expect("truncate");
        assert_eq!(stat(p, b"/A Longer Name.bin").expect("stat").va_size, 100);
        assert_eq!(read_file(p, b"/A Longer Name.bin"), &want[..100]);
        assert_eq!(free0 - pmp.pm_freeclustercount.get(), 1);
        close(p, vp);

        // on the disk: a long name entry, the short entry and the data
        let img = synced(p, mp);
        let mut short = [0u8; 11];
        assert_ne!(
            crate::msdosfs::msdosfs_conv::unix2dosfn(b"A Longer Name.bin", &mut short, 1),
            0
        );
        let e = entry(&img, Dir::Root, &short).expect("the entry on disk");
        assert_eq!(getulong(&e.deFileSize), 100);
        assert_eq!(contents(&img, &e), &want[..100]);
        assert_eq!(e.deAttributes & ATTR_ARCHIVE, ATTR_ARCHIVE);

        // a zero-length file: a hashed file id with the top bit set, the same in readdir
        close(p, create(p, b"/empty", 0o644));
        let va = stat(p, b"/empty").expect("stat");
        assert_eq!(va.va_size, 0);
        assert_ne!(va.va_fileid & 0x8000_0000, 0);
        let (ents, _, _) = readdir(p, b"/", 0, 4096);
        let empty = ents.iter().find(|e| e.name == b"empty").expect("listed");
        assert_eq!(empty.fileno, va.va_fileid);

        // directories cannot be written
        let root = lookup(p, b"/").expect("root");
        let mut b = [0u8; 4];
        assert_eq!(
            vn_rdwr(
                UioRw::UIO_WRITE,
                root,
                b.as_mut_ptr().cast(),
                4,
                0,
                UioSeg::UIO_SYSSPACE,
                0,
                p.ucred(),
                None,
                Some(p),
            ),
            Err(Errno::EISDIR)
        );
        vrele(root);
    }

    #[test]
    fn directories_are_made_listed_and_removed() {
        let (_g, p, mp) = setup_root(sample());

        domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755).expect("mkdir /d");
        domkdirat(p, AT_FDCWD, c("/d/a deeper one").as_ptr(), 0o755).expect("mkdir deeper");
        assert_eq!(
            domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755),
            Err(Errno::EEXIST)
        );
        let d = stat(p, b"/d").expect("stat /d");
        assert_eq!(d.va_type, VDIR);
        let (ents, _, _) = readdir(p, b"/d", 0, 4096);
        assert_eq!(names(&ents), [&b"."[..], b"..", b"a deeper one"]);
        assert_eq!(ents[0].fileno, d.va_fileid, "`.` is the directory itself");
        assert_eq!(ents[1].fileno, 1, "`..` is the root");
        assert_eq!(
            stat(p, b"/d/a deeper one/..").expect("..").va_fileid,
            d.va_fileid
        );

        // on the disk: "." and ".." of the new directory point at it and at the root
        let img = synced(p, mp);
        let e = entry(&img, Dir::Root, b"D          ").expect("/d on disk");
        assert_eq!(e.deAttributes, ATTR_DIRECTORY);
        let cn = u32::from(getushort(&e.deStartCluster));
        let o = img.clust_off(cn);
        let dot = Direntry::at(&img.disk, o);
        let dotdot = Direntry::at(&img.disk, o + 32);
        assert_eq!(dot.name11(), *b".          ");
        assert_eq!(u32::from(getushort(&dot.deStartCluster)), cn);
        assert_eq!(dotdot.name11(), *b"..         ");
        assert_eq!(getushort(&dotdot.deStartCluster), 0);

        // a file in it; rmdir refuses a non-empty directory, unlink refuses a directory
        close(p, create(p, b"/d/f", 0o644));
        assert_eq!(
            dounlinkat(p, AT_FDCWD, c("/d").as_ptr(), AT_REMOVEDIR),
            Err(Errno::ENOTEMPTY)
        );
        assert_eq!(
            dounlinkat(p, AT_FDCWD, c("/d/a deeper one").as_ptr(), 0),
            Err(Errno::EPERM)
        );
        dounlinkat(p, AT_FDCWD, c("/d/a deeper one").as_ptr(), AT_REMOVEDIR).expect("rmdir");
        dounlinkat(p, AT_FDCWD, c("/d/f").as_ptr(), 0).expect("unlink /d/f");
        assert_eq!(stat(p, b"/d/f").err(), Some(Errno::ENOENT));
        dounlinkat(p, AT_FDCWD, c("/d").as_ptr(), AT_REMOVEDIR).expect("rmdir /d");
        assert_eq!(stat(p, b"/d").err(), Some(Errno::ENOENT));
        let (ents, _, _) = readdir(p, b"/", 0, 4096);
        assert_eq!(
            names(&ents),
            [&b"."[..], b"..", FATNAME, b"README.TXT", b"SUBDIR"]
        );
    }

    #[test]
    fn files_are_removed_and_their_clusters_freed() {
        let (_g, p, mp) = setup_root(sample());
        let pmp = vfstomsdosfs(mp);
        let free0 = pmp.pm_freeclustercount.get();

        dounlinkat(p, AT_FDCWD, c("/README.TXT").as_ptr(), 0).expect("unlink");
        assert_eq!(stat(p, b"/readme.txt").err(), Some(Errno::ENOENT));
        assert_eq!(pmp.pm_freeclustercount.get() - free0, 6, "its 6 clusters");
        dounlinkat(p, AT_FDCWD, c("/m10c-fat.txt").as_ptr(), 0).expect("unlink long");
        let img = synced(p, mp);
        assert!(entry(&img, Dir::Root, b"README  TXT").is_none());
        assert!(entry(&img, Dir::Root, FATSHORT).is_none());
        // the long name entry went with it
        assert_eq!(img.disk[img.slots(Dir::Root)[0]], SLOT_DELETED);
        let (ents, _, _) = readdir(p, b"/", 0, 4096);
        assert_eq!(names(&ents), [&b"."[..], b"..", b"SUBDIR"]);
    }

    #[test]
    fn renames_move_entries_within_and_across_directories() {
        let (_g, p, mp) = setup_root(sample());

        // within a directory, to a long name
        dorenameat(
            p,
            AT_FDCWD,
            c("/README.TXT").as_ptr(),
            AT_FDCWD,
            c("/now a long name.txt").as_ptr(),
        )
        .expect("rename");
        assert_eq!(stat(p, b"/README.TXT").err(), Some(Errno::ENOENT));
        assert_eq!(read_file(p, b"/now a long name.txt"), pattern(3000));

        // into a subdirectory
        dorenameat(
            p,
            AT_FDCWD,
            c("/now a long name.txt").as_ptr(),
            AT_FDCWD,
            c("/subdir/moved").as_ptr(),
        )
        .expect("rename across");
        assert_eq!(read_file(p, b"/subdir/moved"), pattern(3000));
        let (ents, _, _) = readdir(p, b"/subdir", 0, 4096);
        assert_eq!(
            names(&ents),
            [&b"."[..], b"..", b"inner file.dat", b"moved"]
        );

        // over an existing file, which goes away
        dorenameat(
            p,
            AT_FDCWD,
            c("/m10c-fat.txt").as_ptr(),
            AT_FDCWD,
            c("/subdir/moved").as_ptr(),
        )
        .expect("rename over");
        assert_eq!(read_file(p, b"/subdir/moved"), FATDATA);
        assert_eq!(stat(p, b"/m10c-fat.txt").err(), Some(Errno::ENOENT));

        // a directory to a new parent: its ".." follows
        domkdirat(p, AT_FDCWD, c("/newparent").as_ptr(), 0o755).expect("mkdir");
        dorenameat(
            p,
            AT_FDCWD,
            c("/subdir").as_ptr(),
            AT_FDCWD,
            c("/newparent/sub").as_ptr(),
        )
        .expect("rename dir");
        let np = stat(p, b"/newparent").expect("stat").va_fileid;
        assert_eq!(stat(p, b"/newparent/sub/..").expect("..").va_fileid, np);
        assert_eq!(read_file(p, b"/newparent/sub/moved"), FATDATA);
        let img = synced(p, mp);
        let sub = entry(&img, Dir::Clust(np as u32), b"SUB        ").expect("sub on disk");
        let o = img.clust_off(u32::from(getushort(&sub.deStartCluster)));
        assert_eq!(
            u64::from(getushort(&Direntry::at(&img.disk, o + 32).deStartCluster)),
            np,
            "`..` on the disk"
        );

        // not into itself, and not "."
        assert_eq!(
            dorenameat(
                p,
                AT_FDCWD,
                c("/newparent").as_ptr(),
                AT_FDCWD,
                c("/newparent/sub/x").as_ptr(),
            ),
            Err(Errno::EINVAL)
        );
        // a file over a directory
        close(p, create(p, b"/plain", 0o644));
        assert_eq!(
            dorenameat(
                p,
                AT_FDCWD,
                c("/plain").as_ptr(),
                AT_FDCWD,
                c("/newparent").as_ptr(),
            ),
            Err(Errno::EISDIR)
        );
    }

    #[test]
    fn attributes_follow_the_mount_and_the_dos_entry() {
        let (_g, p, mp) = setup_root(sample());
        let pmp = vfstomsdosfs(mp);
        pmp.pm_mask.set(0o755);
        pmp.pm_uid.set(0);
        pmp.pm_gid.set(0);

        let f = stat(p, b"/README.TXT").expect("stat");
        assert_eq!((f.va_type, f.va_mode, f.va_nlink), (VREG, 0o644, 1));
        assert_eq!((f.va_size, f.va_bytes, f.va_blocksize), (3000, 3072, 512));
        assert_eq!(f.va_flags, 0, "the archive bit is set: not SF_ARCHIVED");
        let d = stat(p, b"/SUBDIR").expect("stat");
        assert_eq!(
            (d.va_type, d.va_mode),
            (VDIR, 0o755),
            "S_IFDIR is masked away"
        );
        let r = stat(p, b"/").expect("stat /");
        assert_eq!(r.va_fileid, 1, "the FAT12 root");

        let vp = lookup(p, b"/README.TXT").expect("lookup");
        // the owner write bit is the read-only attribute
        let mut va = Vattr::new();
        vattr_null(&mut va);
        va.va_mode = 0o444;
        setattr(p, vp, &mut va).expect("chmod");
        assert_eq!(stat(p, b"/README.TXT").expect("stat").va_mode, 0o444);
        assert_eq!(vtode(vp).de_Attributes.get() & ATTR_READONLY, ATTR_READONLY);
        vattr_null(&mut va);
        va.va_mode = 0o600;
        setattr(p, vp, &mut va).expect("chmod");
        assert_eq!(stat(p, b"/README.TXT").expect("stat").va_mode, 0o644);

        // times: DOS keeps two-second modification times and access dates
        let t = Timespec::new(1_577_836_800 + 3 * 3600 + 61, 0); // 2020-01-01 03:01:01
        vattr_null(&mut va);
        va.va_mtime = t;
        va.va_atime = t;
        setattr(p, vp, &mut va).expect("utimes");
        let f = stat(p, b"/README.TXT").expect("stat");
        assert_eq!(f.va_mtime, Timespec::new(t.tv_sec - 1, 0));
        assert_eq!(f.va_atime, Timespec::new(1_577_836_800, 0));

        // SF_ARCHIVED clears the archive attribute; other flags are not supported
        vattr_null(&mut va);
        va.va_flags = u64::from(SF_ARCHIVED);
        setattr(p, vp, &mut va).expect("chflags");
        assert_eq!(
            stat(p, b"/README.TXT").expect("stat").va_flags,
            u64::from(SF_ARCHIVED)
        );
        vattr_null(&mut va);
        va.va_flags = 1; // UF_NODUMP
        assert_eq!(setattr(p, vp, &mut va), Err(Errno::EOPNOTSUPP));

        // the owner can only be the mount's; unsettable attributes
        vattr_null(&mut va);
        va.va_uid = 1000;
        assert_eq!(setattr(p, vp, &mut va), Err(Errno::EINVAL));
        vattr_null(&mut va);
        va.va_nlink = 2;
        assert_eq!(setattr(p, vp, &mut va), Err(Errno::EINVAL));
        // a directory cannot be truncated
        let dvp = lookup(p, b"/SUBDIR").expect("lookup");
        assert_eq!(truncate(p, dvp, 0), Err(Errno::EISDIR));
        vrele(dvp);

        // access: the mask and the read-only attribute, for another user
        let other = crget();
        other.cr_uid.set(1000);
        other.cr_gid.set(1000);
        let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
        assert_eq!(VOP_ACCESS(vp, VWRITE, other, p), Err(Errno::EACCES));
        VOP_ACCESS(vp, crate::sys::vnode::VREAD, other, p).expect("readable");
        VOP_ACCESS(vp, VWRITE, p.ucred(), p).expect("root writes");
        let _ = VOP_UNLOCK(vp);

        // the change reached the entry
        let img = synced(p, mp);
        let e = entry(&img, Dir::Root, b"README  TXT").expect("on disk");
        assert_eq!(e.deAttributes & ATTR_ARCHIVE, 0);
        assert_eq!(getushort(&e.deMDate), (40 << 9) | (1 << 5) | 1);
        vrele(vp);
    }

    #[test]
    fn the_denode_lock_recurses_as_ufs_does_and_vnd_reads_under_it() {
        let (_g, p, _mp) = setup_root(sample());
        let mut nd = ndinit(0, 0, NiDirp::Sys(b"/README.TXT"), p);
        vn_open(&mut nd, FREAD | FWRITE, 0).expect("open");
        let vp = nd.ni_vp.expect("a vnode");
        let lock = &vtode(vp).de_lock;

        // vn_open returns the vnode locked by this thread
        assert_eq!(VOP_ISLOCKED(vp), LK_EXCLUSIVE);
        assert_eq!(lock.rrwl_wcnt.get(), 1);

        // the same thread takes it again: rrw_enter counts
        vn_lock(vp, LK_EXCLUSIVE | LK_RETRY).expect("recursive lock");
        assert_eq!(lock.rrwl_wcnt.get(), 2);
        assert_eq!(
            vn_lock(vp, LK_EXCLUSIVE | LK_RECURSEFAIL),
            Err(Errno::EDEADLK)
        );
        let _ = VOP_UNLOCK(vp);
        assert_eq!(VOP_ISLOCKED(vp), LK_EXCLUSIVE, "still held once");

        // vnd(4)'s VNDIOCSET reads the file through vn_rdwr (which locks it again) while
        // vn_open's lock is held
        let mut buf = [0u8; 512];
        assert_eq!(rdwr(p, UioRw::UIO_READ, vp, &mut buf, 512), 512);
        assert_eq!(&buf[..], &pattern(3000)[512..1024]);
        assert_eq!(lock.rrwl_wcnt.get(), 1);

        let _ = VOP_UNLOCK(vp);
        assert_eq!(VOP_ISLOCKED(vp), 0);
        close(p, vp);
    }

    #[test]
    fn bmap_maps_clusters_and_counts_runs() {
        let (_g, p, mp) = setup_root(sample());
        let pmp = vfstomsdosfs(mp);
        let vp = lookup(p, b"/README.TXT").expect("lookup");
        let dep = vtode(vp);

        let mut bn: Daddr = 0;
        let mut run = 0;
        msdosfs_bmaparray(vp, 0, &mut bn, Some(&mut run)).expect("bmap");
        assert_eq!(bn, cntobn(pmp, i64::from(dep.de_StartCluster.get())));
        assert_eq!(run, 5, "six contiguous clusters");
        msdosfs_bmaparray(vp, 5, &mut bn, Some(&mut run)).expect("bmap last");
        assert_eq!(run, 0);
        assert_eq!(
            msdosfs_bmaparray(vp, 6, &mut bn, None),
            Err(Errno::E2BIG),
            "past the end of the chain"
        );

        let mut devvp = None;
        let mut a = VopBmapArgs {
            a_vp: vp,
            a_bn: 1 << 40,
            a_vpp: Some(&mut devvp),
            a_bnp: Some(&mut bn),
            a_runp: None,
        };
        assert_eq!(msdosfs_bmap(&mut a), Err(Errno::EFBIG));
        assert!(devvp.is_some_and(|d| ptr::eq(d, pmp.devvp())));
        vrele(vp);
    }

    #[cfg(feature = "tmpfs")]
    #[test]
    fn lookups_cross_the_mount_point_both_ways() {
        use crate::kern::kern_rwlock::rw_obj_init;
        use crate::tmpfs::tmpfs_mem::TMPFS_BYTES_USED;
        use crate::tmpfs::tmpfs_vfsops::tests::{args, tmpfs_conf};
        use crate::tmpfs::tmpfs_vfsops::{tmpfs_mount, tmpfs_root};
        use crate::uvm::uvm_aobj::uao_init;
        use core::sync::atomic::Ordering;

        let (_g, p) = setup(sample());
        rw_obj_init();
        uao_init();
        TMPFS_BYTES_USED.store(0, Ordering::Relaxed);

        // a tmpfs root with /mnt
        let tmp = crate::kern::vfs_subr::vfs_mount_alloc(None, tmpfs_conf());
        let mut data = args(1 << 20, 0, 0, 0o755);
        let mut nd = ndinit(LOOKUP, 0, NiDirp::Sys(b"/"), p);
        tmpfs_mount(tmp, b"/", &mut data, &mut nd, p).expect("mount tmpfs");
        crate::kern::vfs_subr::vfs_unbusy(tmp);
        let root = tmpfs_root(tmp).expect("root");
        set_rootvnode(Some(root));
        p.fd().fd_cdir.set(Some(root));
        vref(root);
        let _ = VOP_UNLOCK(root);
        domkdirat(p, AT_FDCWD, c("/mnt").as_ptr(), 0o755).expect("mkdir /mnt");

        // mount_msdos /dev/vnd0c /mnt, as sys_mount ends
        let mp = mount(p, true);
        let mut sb = mp.mnt_stat.get();
        msdosfs_statfs(mp, &mut sb, p).expect("statfs");
        mp.mnt_stat.set(sb);
        let covered = lookup(p, b"/mnt").expect("/mnt");
        mp.mnt_vnodecovered.set(Some(covered));
        covered.set_v_mountedhere(Some(mp));

        // cat /mnt/m10c-fat.txt
        assert_eq!(read_file(p, b"/mnt/m10c-fat.txt"), FATDATA);
        let (ents, _, _) = readdir(p, b"/mnt", 0, 4096);
        assert_eq!(
            names(&ents),
            [&b"."[..], b"..", FATNAME, b"README.TXT", b"SUBDIR"]
        );
        // the root of the msdosfs, and back up through ".."
        let mroot = lookup(p, b"/mnt").expect("/mnt");
        assert_eq!(mroot.v_tag.get(), crate::sys::vnode::VT_MSDOSFS);
        vrele(mroot);
        assert_eq!(
            stat(p, b"/mnt/..").expect("..").va_fileid,
            stat(p, b"/").expect("/").va_fileid
        );
        assert_eq!(
            read_file(p, b"/mnt/subdir/../SUBDIR/inner file.dat"),
            b"inside"
        );
        // read-only: no creation
        assert_eq!(
            domkdirat(p, AT_FDCWD, c("/mnt/x").as_ptr(), 0o755),
            Err(Errno::EROFS)
        );
    }

    #[test]
    fn pathconf_and_kqueue_filters() {
        let (_g, p, mp) = setup_root(sample());
        let vp = lookup(p, b"/README.TXT").expect("lookup");
        let mut v: Register = 0;
        VOP_PATHCONF(vp, _PC_NAME_MAX, &mut v).expect("pathconf");
        assert_eq!(v, WIN_MAXLEN as Register);
        let pmp = vfstomsdosfs(mp);
        pmp.pm_flags
            .set(pmp.pm_flags.get() & !(MSDOSFSMNT_LONGNAME as u32));
        VOP_PATHCONF(vp, _PC_NAME_MAX, &mut v).expect("pathconf");
        assert_eq!(v, 12, "8.3");
        VOP_PATHCONF(vp, _PC_LINK_MAX, &mut v).expect("pathconf");
        assert_eq!(v, 1);
        VOP_PATHCONF(vp, _PC_TIMESTAMP_RESOLUTION, &mut v).expect("pathconf");
        assert_eq!(v, 2_000_000_000);
        assert_eq!(VOP_PATHCONF(vp, 9999, &mut v), Err(Errno::EINVAL));

        // EVFILT_VNODE: only the subscribed notes are recorded; revocation ends it
        let kn = Knote::new();
        kn.kn_filter().set(EVFILT_VNODE);
        VOP_KQFILTER(vp, 0, &kn).expect("kqfilter");
        assert!(
            kn.kn_fop
                .get()
                .is_some_and(|f| ptr::eq(f, &MSDOSFSVNODE_FILTOPS))
        );
        kn.kn_sfflags.set(NOTE_WRITE | NOTE_DELETE);
        assert!(!filt_msdosfsvnode(&kn, i64::from(NOTE_ATTRIB)));
        assert!(filt_msdosfsvnode(&kn, i64::from(NOTE_WRITE)));
        assert_eq!(kn.kn_fflags().get(), NOTE_WRITE);
        assert!(filt_msdosfsvnode(&kn, i64::from(NOTE_REVOKE)) && kn.has_flags(EV_EOF));
        filt_msdosfsdetach(&kn);

        // EVFILT_WRITE: always writable; an unknown filter is refused
        let kn = Knote::new();
        assert!(filt_msdosfswrite(&kn, 0) && kn.kn_data().get() == 0);
        assert!(filt_msdosfswrite(&kn, i64::from(NOTE_REVOKE)));
        assert!(kn.has_flags(EV_EOF) && kn.has_flags(EV_ONESHOT));
        kn.kn_filter().set(-100);
        assert_eq!(VOP_KQFILTER(vp, 0, &kn), Err(Errno::EINVAL));

        // no links, symlinks or device nodes on DOS
        assert_eq!(
            crate::kern::vfs_syscalls::dolinkat(
                p,
                AT_FDCWD,
                c("/README.TXT").as_ptr(),
                AT_FDCWD,
                c("/link").as_ptr(),
                0
            ),
            Err(Errno::EOPNOTSUPP)
        );
        vrele(vp);
    }

    #[test]
    fn fileidhash_sets_the_top_bit_and_spreads() {
        assert_ne!(fileidhash(0) & 0x8000_0000, 0);
        assert_ne!(fileidhash(1), fileidhash(2));
        assert_eq!(fileidhash(12345), fileidhash(12345));
    }
}
/* </TESTS> */
