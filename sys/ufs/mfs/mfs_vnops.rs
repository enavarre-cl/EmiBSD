/*	$OpenBSD: mfs_vnops.c,v 1.62 2024/10/18 05:52:33 miod Exp $	*/
/*	$NetBSD: mfs_vnops.c,v 1.8 1996/03/17 02:16:32 christos Exp $	*/
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
 *	@(#)mfs_vnops.c	8.5 (Berkeley) 7/28/94
 */
/* </LICENSES> */

//! The memory based file system's vnode operations: `mfs_vops` (the operations of its
//! "device" vnode) and `mfs_open`, `mfs_ioctl`, `mfs_strategy`, `mfs_doio`, `mfs_close`,
//! `mfs_inactive`, `mfs_reclaim` and `mfs_print`.
//!
//! Upstream: sys/ufs/mfs/mfs_vnops.c @ 3ce1f3f79392
//!
//! The I/O model: the file system lives in the address space of the process that called
//! `mount(2)` (`mount_mfs(8)`'s child), which stays inside `mfs_start`. `mfs_strategy` queues
//! a request on the mfsnode and wakes that process, which copies the data in or out with
//! `mfs_doio`; when the process itself asks (it unmounts on a signal), the copy is done at once.
//!
//! ## Deviations
//! - `mfs_vops` has `Some(|_| vop_generic_badop())` where the C names `vop_generic_badop`
//!   (a function of no arguments here, see `vfs_default.rs`), and `nullop` slots as
//!   closures, as `spec_vops` does.
//! - `mfs_doio` runs the data through `copyin`/`copyout` over the buffer's `b_bcount`
//!   bytes; a request that starts beyond the end of the file system has `b_bcount` set to
//!   the (negative) remainder as in the C, which this reads as no bytes.
//! - `mfs_print` exists under features `diagnostic` or `debug` (the C's
//!   `DEBUG || DIAGNOSTIC`); without them it does nothing, as the C. `VFSLCKDEBUG` is not
//!   configured.

use core::ptr;
use core::sync::atomic::Ordering;

#[cfg(feature = "diagnostic")]
use crate::kern::kern_bufq::bufq_peek;
use crate::kern::kern_bufq::{bufq_dequeue, bufq_destroy, bufq_queue};
use crate::kern::kern_malloc::free;
use crate::kern::kern_synch::wakeup;
use crate::kern::spec_vnops::spec_fsync;
use crate::kern::subr_prf::panic;
use crate::kern::subr_xxx::nullop;
use crate::kern::vfs_bio::biodone;
use crate::kern::vfs_default::{
    vop_generic_badop, vop_generic_bmap, vop_generic_bwrite, vop_generic_revoke,
};
use crate::kern::vfs_subr::vinvalbuf;
use crate::kern::vfs_vops::VOP_UNLOCK;
use crate::machine::copy::{copyin, copyout};
use crate::machine::cpu::curproc;
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::{B_ERROR, B_READ, Buf};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_MFSNODE;
use crate::sys::param::DEV_BSHIFT;
use crate::sys::systm::INFSLP;
use crate::sys::vnode::{
    V_SAVE, VBLK, VopCloseArgs, VopInactiveArgs, VopIoctlArgs, VopOpenArgs, VopPrintArgs,
    VopReclaimArgs, VopStrategyArgs, Vops,
};
use crate::ufs::mfs::mfsnode::{Mfsnode, vtomfs};

/// `mfs_vops`: mfs vnode operations.
pub static MFS_VOPS: Vops = Vops {
    vop_lookup: Some(|_| vop_generic_badop()),
    vop_create: Some(|_| vop_generic_badop()),
    vop_mknod: Some(|_| vop_generic_badop()),
    vop_open: Some(mfs_open),
    vop_close: Some(mfs_close),
    vop_access: Some(|_| vop_generic_badop()),
    vop_getattr: Some(|_| vop_generic_badop()),
    vop_setattr: Some(|_| vop_generic_badop()),
    vop_read: Some(|_| vop_generic_badop()),
    vop_write: Some(|_| vop_generic_badop()),
    vop_ioctl: Some(mfs_ioctl),
    vop_kqfilter: Some(|_| vop_generic_badop()),
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
    vop_inactive: Some(mfs_inactive),
    vop_reclaim: Some(mfs_reclaim),
    vop_lock: Some(|_| nullop()),
    vop_unlock: Some(|_| nullop()),
    vop_islocked: Some(|_| 0), // nullop
    vop_bmap: Some(vop_generic_bmap),
    vop_strategy: Some(mfs_strategy),
    vop_print: Some(mfs_print),
    vop_pathconf: Some(|_| vop_generic_badop()),
    vop_advlock: Some(|_| vop_generic_badop()),
    vop_bwrite: Some(vop_generic_bwrite),
};

/// `mfs_open` (`vop_open`): open called to allow memory filesystem to initialize and validate
/// before actual IO. Record our process identifier so we can tell when we are doing I/O to
/// ourself.
pub fn mfs_open(ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    if ap.a_vp.v_type.get() != VBLK {
        panic(format_args!("mfs_open not VBLK"));
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = ap;
    Ok(())
}

/// `mfs_ioctl` (`vop_ioctl`): ioctl operation.
pub fn mfs_ioctl(_ap: &mut VopIoctlArgs<'_>) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

/// `mfs_strategy` (`vop_strategy`): pass I/O requests to the memory filesystem process.
pub fn mfs_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    let bp = ap.a_bp;
    let vp = ap.a_vp;

    if vp.v_type.get() != VBLK || vp.v_usecount.get() == 0 {
        panic(format_args!("mfs_strategy: bad dev"));
    }

    let mfsp = vtomfs(vp);
    if curproc().is_some_and(|p| p.p_tid.get() == mfsp.mfs_tid.get()) {
        mfs_doio(mfsp, bp);
    } else {
        bufq_queue(&mfsp.mfs_bufq, bp);
        wakeup(ptr::from_ref(vp));
    }
    Ok(())
}

/// `mfs_doio`: memory file system I/O. Runs in the file system's process: `mfs_baseoff` is an
/// address in its memory, which `copyin` (a read of the file system) and `copyout` (a write)
/// reach.
pub fn mfs_doio(mfsp: &Mfsnode, bp: &'static Buf) {
    let offset = bp.b_blkno.get() << DEV_BSHIFT;

    if bp.b_bcount.get() > mfsp.mfs_size.get() - offset {
        bp.b_bcount.set(mfsp.mfs_size.get() - offset);
    }

    let base = mfsp.mfs_baseoff.get().wrapping_add(offset as usize);
    // SAFETY: the buffer is busy for the requester, which sleeps in `biowait` until
    // `biodone` below (or is `mfs_close`/`mfs_start` finishing it); buffers handed to a
    // strategy routine are mapped; `data` covers `b_bcount` bytes (none when negative) and
    // nothing else holds a slice of them meanwhile.
    let data = unsafe { bp.data() };
    let error = if bp.isset(B_READ) {
        copyin(base, data)
    } else {
        copyout(data, base)
    };
    bp.b_error.set(error.err());
    if error.is_err() {
        bp.set(B_ERROR);
    } else {
        bp.b_resid.set(0);
    }
    let s = splbio();
    biodone(bp);
    splx(s);
}

/// `mfs_close` (`vop_close`): memory filesystem close routine.
pub fn mfs_close(ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let mfsp = vtomfs(vp);

    // Finish any pending I/O requests.
    while let Some(bp) = bufq_dequeue(&mfsp.mfs_bufq) {
        mfs_doio(mfsp, bp);
        wakeup(ptr::from_ref(bp));
    }

    // On last close of a memory filesystem we must invalidate any in core blocks, so that we
    // can free up its vnode.
    vinvalbuf(vp, V_SAVE, ap.a_cred, ap.a_p, 0, INFSLP)?;

    // There should be no way to have any more buffers on this vnode.
    #[cfg(feature = "diagnostic")]
    if bufq_peek(&mfsp.mfs_bufq) {
        crate::kprintf!("mfs_close: dirty buffers\n");
    }

    // Send a request to the filesystem server to exit.
    mfsp.mfs_shutdown.store(1, Ordering::Relaxed);
    wakeup(ptr::from_ref(vp));
    Ok(())
}

/// `mfs_inactive` (`vop_inactive`): memory filesystem inactive routine.
pub fn mfs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    {
        let mfsp = vtomfs(ap.a_vp);
        if mfsp.mfs_shutdown.load(Ordering::Relaxed) != 0 && bufq_peek(&mfsp.mfs_bufq) {
            panic(format_args!("mfs_inactive: not inactive"));
        }
    }
    let _ = VOP_UNLOCK(ap.a_vp);
    Ok(())
}

/// `mfs_reclaim` (`vop_reclaim`): reclaim a memory filesystem devvp so that it can be reused.
pub fn mfs_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let mfsp = vtomfs(vp);

    bufq_destroy(&mfsp.mfs_bufq);

    if let Some(data) = ptr::NonNull::new(vp.v_data.get().cast::<u8>()) {
        free(data, M_MFSNODE, size_of::<Mfsnode>());
    }
    vp.v_data.set(ptr::null_mut());
    Ok(())
}

/// `mfs_print` (`vop_print`): print out the contents of an mfsnode.
pub fn mfs_print(ap: &mut VopPrintArgs) -> Result<(), Errno> {
    #[cfg(any(feature = "debug", feature = "diagnostic"))]
    {
        let mfsp = vtomfs(ap.a_vp);

        crate::kprintf!(
            "tag VT_MFS, tid {}, base {:#x}, size {}\n",
            mfsp.mfs_tid.get(),
            mfsp.mfs_baseoff.get(),
            mfsp.mfs_size.get()
        );
    }
    #[cfg(not(any(feature = "debug", feature = "diagnostic")))]
    let _ = ap;
    Ok(())
}

#[cfg(test)]
mod tests;
