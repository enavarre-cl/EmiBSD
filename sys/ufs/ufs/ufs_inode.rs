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
 * Copyright (c) 1991, 1993
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
 *	@(#)ufs_inode.c	8.7 (Berkeley) 7/22/94
 */
/* </LICENSES> */

/* <CODE> */
//! The end of an inode's life: `ufs_inactive` (the last reference went: free a removed file's
//! blocks and inode, write the times back) and `ufs_reclaim` (the vnode is being reused:
//! unhash the inode and drop what hangs from it).
//!
//! Upstream: sys/ufs/ufs/ufs_inode.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `UFS_DIRHASH`'s `ufsdirhash_free` in `ufs_reclaim` is under feature `ufs_dirhash`.
//! - The quota calls are `quota.rs`'s: `ufs_quota.rs`'s with feature `quota` (`option QUOTA`),
//!   the no-quota answers of `ufs_quota_stub.c` without it.

use crate::kern::vfs_cache::cache_purge;
#[cfg(feature = "diagnostic")]
use crate::kern::vfs_subr::PRTACTIVE;
use crate::kern::vfs_subr::{vrecycle, vrele};
use crate::kern::vfs_vops::VOP_UNLOCK;
use crate::sys::errno::Errno;
use crate::sys::mount::MNT_RDONLY;
use crate::sys::ucred::NOCRED;
use crate::sys::vnode::{Vnode, VopInactiveArgs};
use crate::ufs::ufs::inode::{
    IN_ACCESS, IN_CHANGE, IN_LAZYMOD, IN_MODIFIED, IN_UPDATE, UFS_INODE_FREE, UFS_TRUNCATE,
    UFS_UPDATE, vtoi,
};
use crate::ufs::ufs::quota::{getinoquota, ufs_quota_delete, ufs_quota_free_inode};
use crate::ufs::ufs::ufs_ihash::ufs_ihashrem;

/// `ufs_inactive` (`vop_inactive`): last reference to an inode. If necessary, write or
/// delete it.
pub fn ufs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);
    let mut error = Ok(());

    #[cfg(feature = "diagnostic")]
    if PRTACTIVE.load(core::sync::atomic::Ordering::Relaxed) != 0 && vp.v_usecount.get() != 0 {
        crate::kern::vfs_subr::vprint(Some("ufs_inactive: pushing active"), vp);
    }

    // Ignore inodes related to stale file handles.
    if !(ip.din_is_null() || ip.dip_mode() == 0) {
        let rdonly = vp
            .v_mount
            .get()
            .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0);
        if ip.dip_nlink() <= 0 && !rdonly {
            if getinoquota(ip).is_ok() {
                let _ = ufs_quota_free_inode(ip, NOCRED);
            }

            error = UFS_TRUNCATE(ip, 0, 0, NOCRED);

            ip.dip_set_rdev(0);
            let mode = ip.dip_mode();
            ip.dip_set_mode(0);
            ip.set_flag(IN_CHANGE | IN_UPDATE);

            let _ = UFS_INODE_FREE(ip, ip.i_number.get(), mode);
        }

        if ip.i_flag.get() & (IN_ACCESS | IN_CHANGE | IN_MODIFIED | IN_UPDATE) != 0 {
            let _ = UFS_UPDATE(ip, 0);
        }
    }
    // out:
    let _ = VOP_UNLOCK(vp);

    // If we are done with the inode, reclaim it so that it can be reused immediately.
    if ip.din_is_null() || ip.dip_mode() == 0 {
        vrecycle(vp, ap.a_p);
    }

    error
}

/// `ufs_reclaim`: reclaim an inode so that it can be used for other purposes (the UFS half
/// of a file system's `vop_reclaim`).
pub fn ufs_reclaim(vp: &'static Vnode) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    if PRTACTIVE.load(core::sync::atomic::Ordering::Relaxed) != 0 && vp.v_usecount.get() != 0 {
        crate::kern::vfs_subr::vprint(Some("ufs_reclaim: pushing active"), vp);
    }

    let ip = vtoi(vp);

    // Stop deferring timestamp writes.
    if ip.i_flag.get() & IN_LAZYMOD != 0 {
        ip.set_flag(IN_MODIFIED);
        let _ = UFS_UPDATE(ip, 0);
    }

    // Remove the inode from its hash chain.
    ufs_ihashrem(ip);
    // Purge old data structures associated with the inode.
    cache_purge(vp);

    if let Some(ump) = ip.i_ump.get()
        && let Some(devvp) = ump.um_devvp.get()
    {
        vrele(devvp);
    }
    #[cfg(feature = "ufs_dirhash")]
    if ip.i_dirhash.get().is_some() {
        crate::ufs::ufs::ufs_dirhash::ufsdirhash_free(ip);
    }
    let _ = ufs_quota_delete(ip);
    Ok(())
}
/* </CODE> */
