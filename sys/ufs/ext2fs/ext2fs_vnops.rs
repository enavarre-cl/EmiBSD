/*	$OpenBSD: ext2fs_vnops.c,v 1.95 2025/06/01 00:32:54 rsadowski Exp $	*/
/*	$NetBSD: ext2fs_vnops.c,v 1.1 1997/06/11 09:34:09 bouyer Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Manuel Bouyer.
 * Copyright (c) 1982, 1986, 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)ufs_vnops.c	8.14 (Berkeley) 10/26/94
 * Modified for ext2fs by Manuel Bouyer.
 */
/* </LICENSES> */

//! The second extended file system's vnode operations: the tables for files
//! (`ext2fs_vops`) and for the special files that live on it (`ext2fs_specvops`), and, so
//! far, the operations of `ext2fs_vnops.c` the rest of ext2fs needs to mount, read, write,
//! sync and unmount: `ext2fs_fsync` and `ext2fs_reclaim`.
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_vnops.c @ 3ce1f3f79392
//!
//! The port of this file is under way (`ports.toml` says `wip`): the tables, `ext2fs_fsync`
//! and `ext2fs_reclaim` are done; `ext2fs_create` ... `ext2fs_advlock`, `ext2fs_makeinode`,
//! `ext2fs_chmod`/`ext2fs_chown` and the lookup functions (`ext2fs_lookup.c`) are not.
//!
//! ## Deviations
//! - Until they are ported, the operations of this file and of `ext2fs_lookup.c` that the
//!   tables name are visible stubs that answer `ENOSYS` through `unported!`: `ext2fs_lookup`,
//!   `ext2fs_create`, `ext2fs_mknod`, `ext2fs_open`, `ext2fs_access`, `ext2fs_getattr`,
//!   `ext2fs_setattr`, `ext2fs_remove`, `ext2fs_link`, `ext2fs_rename`, `ext2fs_mkdir`,
//!   `ext2fs_rmdir`, `ext2fs_symlink`, `ext2fs_readdir`, `ext2fs_readlink`,
//!   `ext2fs_pathconf`, `ext2fs_advlock`.
//! - `option FIFO` is in GENERIC but `miscfs/fifofs` is not ported: `ext2fs_fifovops` and
//!   `ext2fsfifo_reclaim` come with it, and `ext2fs_vinit` refuses fifos meanwhile
//!   (`ext2fs_subr.rs`).
//! - The helpers the C installs in many slots (`vop_generic_badop`) are closures, as in
//!   `spec_vops` (`docs/C_TO_RUST.md`).

use core::ptr::{self, NonNull};

use crate::kern::spec_vnops::{
    spec_advlock, spec_ioctl, spec_kqfilter, spec_open, spec_pathconf, spec_strategy,
};
use crate::kern::subr_pool::pool_put;
use crate::kern::vfs_cache::cache_purge;
use crate::kern::vfs_default::{
    vop_generic_abortop, vop_generic_badop, vop_generic_bmap, vop_generic_bwrite,
    vop_generic_lookup, vop_generic_revoke,
};
use crate::kern::vfs_subr::{vflushbuf, vrele};
use crate::sys::errno::Errno;
use crate::sys::mount::MNT_WAIT;
use crate::sys::vnode::{
    VopAccessArgs, VopAdvlockArgs, VopCreateArgs, VopFsyncArgs, VopGetattrArgs, VopLinkArgs,
    VopLookupArgs, VopMkdirArgs, VopMknodArgs, VopOpenArgs, VopPathconfArgs, VopReaddirArgs,
    VopReadlinkArgs, VopReclaimArgs, VopRemoveArgs, VopRenameArgs, VopRmdirArgs, VopSetattrArgs,
    VopSymlinkArgs, Vops,
};
use crate::ufs::ext2fs::ext2fs_bmap::ext2fs_bmap;
use crate::ufs::ext2fs::ext2fs_inode::{ext2fs_inactive, ext2fs_update};
use crate::ufs::ext2fs::ext2fs_readwrite::{ext2fs_read, ext2fs_write};
use crate::ufs::ext2fs::ext2fs_vfsops::{EXT2FS_DINODE_POOL, EXT2FS_INODE_POOL};
use crate::ufs::ufs::inode::vtoi;
use crate::ufs::ufs::ufs_ihash::ufs_ihashrem;
use crate::ufs::ufs::ufs_vnops::{
    ufs_close, ufs_ioctl, ufs_islocked, ufs_kqfilter, ufs_lock, ufs_print, ufs_strategy,
    ufs_unlock, ufsspec_close, ufsspec_read, ufsspec_write,
};
use crate::unported;

/// `ext2fs_vops`: the operations of a file, directory or symbolic link on an ext2fs.
pub static EXT2FS_VOPS: Vops = Vops {
    vop_lookup: Some(ext2fs_lookup),
    vop_create: Some(ext2fs_create),
    vop_mknod: Some(ext2fs_mknod),
    vop_open: Some(ext2fs_open),
    vop_close: Some(ufs_close),
    vop_access: Some(ext2fs_access),
    vop_getattr: Some(ext2fs_getattr),
    vop_setattr: Some(ext2fs_setattr),
    vop_read: Some(ext2fs_read),
    vop_write: Some(ext2fs_write),
    vop_ioctl: Some(ufs_ioctl),
    vop_kqfilter: Some(ufs_kqfilter),
    vop_revoke: None,
    vop_fsync: Some(ext2fs_fsync),
    vop_remove: Some(ext2fs_remove),
    vop_link: Some(ext2fs_link),
    vop_rename: Some(ext2fs_rename),
    vop_mkdir: Some(ext2fs_mkdir),
    vop_rmdir: Some(ext2fs_rmdir),
    vop_symlink: Some(ext2fs_symlink),
    vop_readdir: Some(ext2fs_readdir),
    vop_readlink: Some(ext2fs_readlink),
    vop_abortop: Some(vop_generic_abortop),
    vop_inactive: Some(ext2fs_inactive),
    vop_reclaim: Some(ext2fs_reclaim),
    vop_lock: Some(ufs_lock),
    vop_unlock: Some(ufs_unlock),
    vop_bmap: Some(ext2fs_bmap),
    vop_strategy: Some(ufs_strategy),
    vop_print: Some(ufs_print),
    vop_islocked: Some(ufs_islocked),
    vop_pathconf: Some(ext2fs_pathconf),
    vop_advlock: Some(ext2fs_advlock),
    vop_bwrite: Some(vop_generic_bwrite),
};

/// `ext2fs_specvops`: the operations of a device special file on an ext2fs.
pub static EXT2FS_SPECVOPS: Vops = Vops {
    vop_close: Some(ufsspec_close),
    vop_access: Some(ext2fs_access),
    vop_getattr: Some(ext2fs_getattr),
    vop_setattr: Some(ext2fs_setattr),
    vop_read: Some(ufsspec_read),
    vop_write: Some(ufsspec_write),
    vop_fsync: Some(ext2fs_fsync),
    vop_inactive: Some(ext2fs_inactive),
    vop_reclaim: Some(ext2fs_reclaim),
    vop_lock: Some(ufs_lock),
    vop_unlock: Some(ufs_unlock),
    vop_print: Some(ufs_print),
    vop_islocked: Some(ufs_islocked),

    // XXX: Keep in sync with spec_vops.
    vop_lookup: Some(vop_generic_lookup),
    vop_create: Some(|_| vop_generic_badop()),
    vop_mknod: Some(|_| vop_generic_badop()),
    vop_open: Some(spec_open),
    vop_ioctl: Some(spec_ioctl),
    vop_kqfilter: Some(spec_kqfilter),
    vop_revoke: Some(vop_generic_revoke),
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

/// `ext2fs_create` (`vop_create`): not ported yet (see the module's deviations).
pub fn ext2fs_create(_ap: &mut VopCreateArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_create (ext2fs_vnops.c)"))
}

/// `ext2fs_mknod` (`vop_mknod`): not ported yet.
pub fn ext2fs_mknod(_ap: &mut VopMknodArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_mknod (ext2fs_vnops.c)"))
}

/// `ext2fs_open` (`vop_open`): not ported yet.
pub fn ext2fs_open(_ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_open (ext2fs_vnops.c)"))
}

/// `ext2fs_access` (`vop_access`): not ported yet.
pub fn ext2fs_access(_ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_access (ext2fs_vnops.c)"))
}

/// `ext2fs_getattr` (`vop_getattr`): not ported yet.
pub fn ext2fs_getattr(_ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_getattr (ext2fs_vnops.c)"))
}

/// `ext2fs_setattr` (`vop_setattr`): not ported yet.
pub fn ext2fs_setattr(_ap: &mut VopSetattrArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_setattr (ext2fs_vnops.c)"))
}

/// `ext2fs_remove` (`vop_remove`): not ported yet.
pub fn ext2fs_remove(_ap: &mut VopRemoveArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_remove (ext2fs_vnops.c)"))
}

/// `ext2fs_link` (`vop_link`): not ported yet.
pub fn ext2fs_link(_ap: &mut VopLinkArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_link (ext2fs_vnops.c)"))
}

/// `ext2fs_rename` (`vop_rename`): not ported yet.
pub fn ext2fs_rename(_ap: &mut VopRenameArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_rename (ext2fs_vnops.c)"))
}

/// `ext2fs_mkdir` (`vop_mkdir`): not ported yet.
pub fn ext2fs_mkdir(_ap: &mut VopMkdirArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_mkdir (ext2fs_vnops.c)"))
}

/// `ext2fs_rmdir` (`vop_rmdir`): not ported yet.
pub fn ext2fs_rmdir(_ap: &mut VopRmdirArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_rmdir (ext2fs_vnops.c)"))
}

/// `ext2fs_symlink` (`vop_symlink`): not ported yet.
pub fn ext2fs_symlink(_ap: &mut VopSymlinkArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_symlink (ext2fs_vnops.c)"))
}

/// `ext2fs_readlink` (`vop_readlink`): not ported yet.
pub fn ext2fs_readlink(_ap: &mut VopReadlinkArgs<'_, '_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_readlink (ext2fs_vnops.c)"))
}

/// `ext2fs_pathconf` (`vop_pathconf`): not ported yet.
pub fn ext2fs_pathconf(_ap: &mut VopPathconfArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_pathconf (ext2fs_vnops.c)"))
}

/// `ext2fs_advlock` (`vop_advlock`): not ported yet.
pub fn ext2fs_advlock(_ap: &mut VopAdvlockArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_advlock (ext2fs_vnops.c)"))
}

/// `ext2fs_fsync` (`vop_fsync`): synch an open file.
pub fn ext2fs_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    vflushbuf(vp, ap.a_waitfor == MNT_WAIT);
    ext2fs_update(vtoi(vp), i32::from(ap.a_waitfor == MNT_WAIT))
}

/// `ext2fs_reclaim` (`vop_reclaim`): reclaim an inode so that it can be used for other
/// purposes.
pub fn ext2fs_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    #[cfg(feature = "diagnostic")]
    if crate::kern::vfs_subr::PRTACTIVE.load(core::sync::atomic::Ordering::Relaxed) != 0
        && vp.v_usecount.get() != 0
    {
        crate::kern::vfs_subr::vprint(Some("ext2fs_reclaim: pushing active"), vp);
    }

    // Remove the inode from its hash chain.
    let ip = vtoi(vp);
    ufs_ihashrem(ip);

    // Purge old data structures associated with the inode.
    cache_purge(vp);
    if let Some(ump) = ip.i_ump.get()
        && let Some(devvp) = ump.um_devvp.get()
    {
        vrele(devvp);
    }

    if let Some(din) = NonNull::new(ip.dinode_u.get().cast::<u8>()) {
        pool_put(&EXT2FS_DINODE_POOL, din);
    }

    let ipp = NonNull::from(ip).cast::<u8>();
    vp.v_data.set(ptr::null_mut());
    pool_put(&EXT2FS_INODE_POOL, ipp);

    Ok(())
}

/// `ext2fs_readdir` (`vop_readdir`, `ext2fs_lookup.c`): not ported yet.
pub fn ext2fs_readdir(_ap: &mut VopReaddirArgs<'_, '_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_readdir (ext2fs_lookup.c)"))
}

/// `ext2fs_lookup` (`vop_lookup`, `ext2fs_lookup.c`): not ported yet.
pub fn ext2fs_lookup(_ap: &mut VopLookupArgs<'_>) -> Result<(), Errno> {
    Err(unported!("ext2fs_lookup (ext2fs_lookup.c)"))
}
