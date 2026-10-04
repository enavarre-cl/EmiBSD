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

#[cfg(test)]
mod tests;
