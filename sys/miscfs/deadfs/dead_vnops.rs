/*	$OpenBSD: dead_vnops.c,v 1.43 2024/10/18 05:52:32 miod Exp $	*/
/*	$NetBSD: dead_vnops.c,v 1.16 1996/02/13 13:12:48 mycroft Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)dead_vnops.c	8.2 (Berkeley) 11/21/94
 */
/* </LICENSES> */

//! The operations of a dead vnode (`dead_vops`): what `vclean` leaves behind when it revokes
//! a vnode from its file system. Opens fail as if the device did not exist, reads return
//! EOF (ttys) or `EIO`, most other operations `EBADF`, and the operations that may race with
//! the cleaning wait for it to finish (`chkvnlock`) and retry on the vnode's new operations.
//!
//! Upstream: sys/miscfs/deadfs/dead_vnops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The operations take their argument structures; `dead_ebadf`, `nullop` and
//!   `vop_generic_badop` fill many slots of different types, so the table writes them as
//!   closures. `chkvnlock` returns `bool`.
//! - `dead_kqfilter` sets `kn_fop = &dead_filtops` (`kern_event.c`, not ported): reported.
//! - `dead_strategy` reads `bp->b_vp` and calls `biodone` (`struct buf`, `vfs_bio.c`):
//!   reported.

use core::ptr;

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_synch::msleep_nsec;
use crate::kern::subr_prf::panic;
use crate::kern::subr_xxx::nullop;
use crate::kern::vfs_default::{vop_generic_badop, vop_generic_lookup};
use crate::kern::vfs_subr::VNODE_MTX;
use crate::kern::vfs_vops::{VOP_BMAP, VOP_LOCK, VOP_UNLOCK};
use crate::sys::errno::Errno;
use crate::sys::lock::LK_DRAIN;
use crate::sys::param::PINOD;
use crate::sys::systm::INFSLP;
use crate::sys::vnode::{
    VISTTY, VXLOCK, VXWANT, Vnode, VopBmapArgs, VopInactiveArgs, VopIoctlArgs, VopKqfilterArgs,
    VopLockArgs, VopOpenArgs, VopPrintArgs, VopReadArgs, VopStrategyArgs, VopWriteArgs, Vops,
};
use crate::unported;

/// `dead_vops`: the operations of a revoked vnode.
pub static DEAD_VOPS: Vops = Vops {
    vop_lookup: Some(vop_generic_lookup),
    vop_create: Some(|_| vop_generic_badop()),
    vop_mknod: Some(|_| vop_generic_badop()),
    vop_open: Some(dead_open),
    vop_close: Some(|_| nullop()),
    vop_access: Some(|_| dead_ebadf()),
    vop_getattr: Some(|_| dead_ebadf()),
    vop_setattr: Some(|_| dead_ebadf()),
    vop_read: Some(dead_read),
    vop_write: Some(dead_write),
    vop_ioctl: Some(dead_ioctl),
    vop_kqfilter: Some(dead_kqfilter),
    vop_revoke: None,
    vop_fsync: Some(|_| nullop()),
    vop_remove: Some(|_| vop_generic_badop()),
    vop_link: Some(|_| vop_generic_badop()),
    vop_rename: Some(|_| vop_generic_badop()),
    vop_mkdir: Some(|_| vop_generic_badop()),
    vop_rmdir: Some(|_| vop_generic_badop()),
    vop_symlink: Some(|_| vop_generic_badop()),
    vop_readdir: Some(|_| dead_ebadf()),
    vop_readlink: Some(|_| dead_ebadf()),
    vop_abortop: Some(|_| vop_generic_badop()),
    vop_inactive: Some(dead_inactive),
    vop_reclaim: Some(|_| nullop()),
    vop_lock: Some(dead_lock),
    vop_unlock: Some(|_| nullop()),
    vop_islocked: Some(|_| 0), // nullop
    vop_bmap: Some(dead_bmap),
    vop_strategy: Some(dead_strategy),
    vop_print: Some(dead_print),
    vop_pathconf: Some(|_| dead_ebadf()),
    vop_advlock: Some(|_| dead_ebadf()),
    vop_bwrite: Some(|_| nullop()),
};

/// Open always fails as if device did not exist.
pub fn dead_open(_ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    Err(Errno::ENXIO)
}

/// Vnode op for read.
pub fn dead_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    if chkvnlock(ap.a_vp) {
        panic(format_args!("dead_read: lock"));
    }
    // Return EOF for tty devices, EIO for others
    if ap.a_vp.v_flag.get() & VISTTY == 0 {
        return Err(Errno::EIO);
    }
    Ok(())
}

/// Vnode op for write.
pub fn dead_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    if chkvnlock(ap.a_vp) {
        panic(format_args!("dead_write: lock"));
    }
    Err(Errno::EIO)
}

/// Device ioctl operation: once the vnode has finished changing, its new operations answer.
pub fn dead_ioctl(ap: &mut VopIoctlArgs<'_>) -> Result<(), Errno> {
    if !chkvnlock(ap.a_vp) {
        return Err(Errno::EBADF);
    }
    match ap.a_vp.op().vop_ioctl {
        Some(f) => f(ap),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `dead_kqfilter`: read, write and (poll's) except filters see the dead filter ops.
pub fn dead_kqfilter(_ap: &mut VopKqfilterArgs) -> Result<(), Errno> {
    // kn_filter, kn_flags, kn_fop = &dead_filtops: struct knote (kern_event.c).
    Err(unported!("dead_kqfilter: dead_filtops (kern_event.c)"))
}

/// Just call the device strategy routine.
pub fn dead_strategy(_ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    // bp->b_vp, B_ERROR, biodone(bp) or VOP_STRATEGY(bp->b_vp, bp): struct buf (vfs_bio.c).
    Err(unported!("dead_strategy: struct buf (vfs_bio.c)"))
}

/// `dead_inactive`: nothing to do but unlock.
pub fn dead_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    let _ = VOP_UNLOCK(ap.a_vp);
    Ok(())
}

/// Wait until the vnode has finished changing state.
pub fn dead_lock(ap: &mut VopLockArgs) -> Result<(), Errno> {
    let vp = ap.a_vp;

    if ap.a_flags & LK_DRAIN != 0 || !chkvnlock(vp) {
        return Ok(());
    }

    VOP_LOCK(vp, ap.a_flags)
}

/// Wait until the vnode has finished changing state.
pub fn dead_bmap(ap: &mut VopBmapArgs<'_>) -> Result<(), Errno> {
    if !chkvnlock(ap.a_vp) {
        return Err(Errno::EIO);
    }
    VOP_BMAP(
        ap.a_vp,
        ap.a_bn,
        ap.a_vpp.as_deref_mut(),
        ap.a_bnp.as_deref_mut(),
        ap.a_runp.as_deref_mut(),
    )
}

/// Print out the contents of a dead vnode.
pub fn dead_print(_ap: &mut VopPrintArgs) -> Result<(), Errno> {
    #[cfg(any(feature = "debug", feature = "diagnostic"))]
    crate::kprintf!("tag VT_NON, dead vnode\n");
    Ok(())
}

/// Empty vnode failed operation.
pub fn dead_ebadf() -> Result<(), Errno> {
    Err(Errno::EBADF)
}

/// We have to wait during times when the vnode is in a state of change. Returns whether it
/// had to wait.
pub fn chkvnlock(vp: &'static Vnode) -> bool {
    let mut locked = false;

    mtx_enter(&VNODE_MTX);
    while vp.v_lflag.get() & VXLOCK != 0 {
        vp.v_lflag.set(vp.v_lflag.get() | VXWANT);
        let _ = msleep_nsec(ptr::from_ref(vp), &VNODE_MTX, PINOD, "chkvnlock", INFSLP);
        locked = true;
    }
    mtx_leave(&VNODE_MTX);
    locked
}
