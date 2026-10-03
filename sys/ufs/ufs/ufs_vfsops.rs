/*	$OpenBSD: ufs_vfsops.c,v 1.21 2025/09/20 13:53:36 mpi Exp $	*/
/*	$NetBSD: ufs_vfsops.c,v 1.4 1996/02/09 22:36:12 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1991, 1993, 1994
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
 *	@(#)ufs_vfsops.c	8.4 (Berkeley) 4/16/94
 */
/* </LICENSES> */

//! The file-system-type operations the UFS file systems share: `ufs_start`, `ufs_root`,
//! `ufs_check_export`, `ufs_init` and the generic half of `fhtovp`.
//!
//! Upstream: sys/ufs/ufs/ufs_vfsops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `ufs_init`'s `static int done` is the atomic [`UFS_INIT_DONE`]; the host tests clear it
//!   to initialise again over fresh memory.
//! - `ufs_check_export`: without `NFSSERVER` there is no export list (`vfs_export_lookup`
//!   finds nothing), so every client is refused with `EACCES`, as the C does when the lookup
//!   fails; `struct netcred` (its `netc_exflags`, `netc_anon`) is not ported.
//! - `ufsdirhash_init` (`UFS_DIRHASH`) waits for `ufs_dirhash.c` (`ufs_lookup.rs`).

use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::kern::vfs_subr::{vfs_export_lookup, vput};
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::mount::{Mount, VFS_VGET, Vfsconf};
use crate::sys::proc::Proc;
use crate::sys::ucred::Ucred;
use crate::sys::vnode::Vnode;
use crate::ufs::ufs::dinode::ROOTINO;
use crate::ufs::ufs::inode::{Ufid, vtoi};
use crate::ufs::ufs::quota::ufs_quota_init;
use crate::ufs::ufs::ufs_ihash::ufs_ihashinit;

/// `ufs_init`'s `done`: the UFS layer is initialised.
pub static UFS_INIT_DONE: AtomicBool = AtomicBool::new(false);

/// `ufs_start` (`vfs_start`): make a filesystem operational. Nothing to do at the moment.
pub fn ufs_start(_mp: &'static Mount, _flags: i32, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

/// `ufs_root` (`vfs_root`): return the root of a filesystem, referenced and locked.
pub fn ufs_root(mp: &'static Mount) -> Result<&'static Vnode, Errno> {
    VFS_VGET(mp, u64::from(ROOTINO))
}

/// `ufs_check_export` (`vfs_checkexp`): verify a remote client has export rights and return
/// these rights via `exflagsp` and `credanonp`.
pub fn ufs_check_export(
    mp: &'static Mount,
    nam: &Mbuf,
    _exflagsp: &mut i32,
    _credanonp: &mut *const Ucred,
) -> Result<(), Errno> {
    // Get the export permission structure for this <mp, client> tuple. `um_export` is not
    // kept without NFSSERVER (ufsmount.rs).
    let np = vfs_export_lookup(mp, ptr::null_mut(), ptr::from_ref(nam).cast());
    if np.is_null() {
        return Err(Errno::EACCES);
    }

    // *exflagsp = np->netc_exflags; *credanonp = &np->netc_anon: struct netcred
    // (NFSSERVER, not configured; vfs_export_lookup never finds one).
    Err(crate::unported!(
        "ufs_check_export: struct netcred (NFSSERVER)"
    ))
}

/// `ufs_init`: initialize UFS file systems, done only once.
pub fn ufs_init(_vfsp: &'static Vfsconf) -> Result<(), Errno> {
    if UFS_INIT_DONE.swap(true, Ordering::Relaxed) {
        return Ok(());
    }
    ufs_ihashinit();
    ufs_quota_init();
    // UFS_DIRHASH: ufsdirhash_init() (ufs_dirhash.c, not ported).

    Ok(())
}

/// `ufs_fhtovp`: the generic part of `fhtovp` called after the underlying filesystem has
/// validated the file handle: the vnode of the handle's inode, referenced and locked,
/// `ESTALE` when the inode was freed or reused since.
pub fn ufs_fhtovp(mp: &'static Mount, ufhp: &Ufid) -> Result<&'static Vnode, Errno> {
    let nvp = VFS_VGET(mp, u64::from(ufhp.ufid_ino))?;
    let ip = vtoi(nvp);
    if ip.dip_mode() == 0 || ip.dip_gen() != ufhp.ufid_gen {
        vput(nvp);
        return Err(Errno::ESTALE);
    }
    Ok(nvp)
}
