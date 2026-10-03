/*	$OpenBSD: vfs_default.c,v 1.52 2025/04/15 05:51:51 jsg Exp $  */
/* <LICENSES> */
/*
 * Portions of this code are:
 *
 * Copyright (c) 1989, 1993
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
 */
/* </LICENSES> */

//! The generic vnode operations file systems share: `vop_generic_revoke` (revoke a vnode and
//! its aliases), `vop_generic_badop` (an operation that must never be called),
//! `vop_generic_bmap` (the identity block map), `vop_generic_bwrite`, `vop_generic_abortop`
//! (drop the pathname buffer) and `vop_generic_lookup` (a lookup that always fails).
//!
//! Upstream: sys/kern/vfs_default.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - Each operation takes its own argument structure instead of `void *`. `vop_generic_badop`
//!   takes nothing and never returns: it fills many slots of different types, so a table
//!   writes it as a closure, `Some(|_| vop_generic_badop())`.

use core::ptr;

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_synch::msleep_nsec;
use crate::kern::subr_pool::pool_put;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::bwrite;
use crate::kern::vfs_init::NAMEI_POOL;
use crate::kern::vfs_subr::{VNODE_MTX, vfs_busy, vgonel};
use crate::kern::vfs_syscalls::dounmount;
use crate::machine::cpu::curproc;
use crate::sys::errno::Errno;
use crate::sys::mount::{MNT_DOOMED, MNT_FORCE, VB_WAIT, VB_WRITE};
use crate::sys::namei::{HASBUF, SAVESTART};
use crate::sys::param::PINOD;
use crate::sys::systm::INFSLP;
use crate::sys::vnode::{
    REVOKEALL, VALIASED, VBLK, VXLOCK, VXWANT, VopAbortopArgs, VopBmapArgs, VopBwriteArgs,
    VopLookupArgs, VopRevokeArgs,
};

/// Eliminate all activity associated with the requested vnode and with all vnodes aliased to
/// the requested vnode.
pub fn vop_generic_revoke(ap: &mut VopRevokeArgs) -> Result<(), Errno> {
    let p = curproc();

    #[cfg(feature = "diagnostic")]
    if ap.a_flags & REVOKEALL == 0 {
        panic(format_args!("vop_generic_revoke"));
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = REVOKEALL;

    let vp = ap.a_vp;

    while vp.v_type.get() == VBLK
        && let Some(mp) = vp.v_specinfo().and_then(|si| si.si_mountpoint.get())
    {
        // If we have a mount point associated with the vnode, we must flush it out now, as
        // to not leave a dangling zombie mount point laying around in VFS.
        if vfs_busy(mp, VB_WRITE | VB_WAIT).is_ok() {
            if let Some(p) = p {
                let _ = dounmount(mp, MNT_FORCE | MNT_DOOMED, p);
            }
            break;
        }
    }

    if vp.v_flag.get() & VALIASED != 0 {
        // If a vgone (or vclean) is already in progress, wait until it is done and return.
        mtx_enter(&VNODE_MTX);
        if vp.v_lflag.get() & VXLOCK != 0 {
            vp.v_lflag.set(vp.v_lflag.get() | VXWANT);
            let _ = msleep_nsec(
                ptr::from_ref(vp),
                &VNODE_MTX,
                PINOD,
                "vop_generic_revokeall",
                INFSLP,
            );
            mtx_leave(&VNODE_MTX);
            return Ok(());
        }

        // Ensure that vp will not be vgone'd while we are eliminating its aliases.
        vp.v_lflag.set(vp.v_lflag.get() | VXLOCK);
        mtx_leave(&VNODE_MTX);

        while vp.v_flag.get() & VALIASED != 0 {
            let Some(chain) = vp.v_specinfo().and_then(|si| si.si_hashchain.get()) else {
                break;
            };
            for vq in chain.iter() {
                if vq.v_rdev() != vp.v_rdev()
                    || vq.v_type.get() != vp.v_type.get()
                    || ptr::eq(vp, vq)
                {
                    continue;
                }
                vgonel(vq, p);
                break;
            }
        }

        // Remove the lock so that vgone below will really eliminate the vnode after which
        // time vgone will awaken any sleepers.
        mtx_enter(&VNODE_MTX);
        vp.v_lflag.set(vp.v_lflag.get() & !VXLOCK);
        mtx_leave(&VNODE_MTX);
    }

    vgonel(vp, p);

    Ok(())
}

/// `vop_generic_badop`: an operation that must never be reached.
pub fn vop_generic_badop() -> ! {
    panic(format_args!("vop_generic_badop"))
}

/// `vop_generic_bmap`: the identity block map: the vnode itself, the same block, no run.
pub fn vop_generic_bmap(ap: &mut VopBmapArgs<'_>) -> Result<(), Errno> {
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

/// `vop_generic_bwrite`: `bwrite(ap->a_bp)`.
pub fn vop_generic_bwrite(ap: &mut VopBwriteArgs) -> Result<(), Errno> {
    bwrite(ap.a_bp)
}

/// `vop_generic_abortop`: frees the pathname buffer a lookup left behind, unless the caller
/// asked to keep it (`SAVESTART`).
pub fn vop_generic_abortop(ap: &mut VopAbortopArgs<'_>) -> Result<(), Errno> {
    let cnp = &mut *ap.a_cnp;
    if cnp.cn_flags & (HASBUF | SAVESTART) == HASBUF
        && let Some(buf) = ptr::NonNull::new(cnp.cn_pnbuf)
    {
        pool_put(&NAMEI_POOL, buf);
    }

    Ok(())
}

/// Trivial lookup routine that always fails.
pub fn vop_generic_lookup(ap: &mut VopLookupArgs<'_>) -> Result<(), Errno> {
    *ap.a_vpp = None;
    Err(Errno::ENOTDIR)
}
