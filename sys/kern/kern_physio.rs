/*	$OpenBSD: kern_physio.c,v 1.49 2024/02/03 18:51:58 beck Exp $	*/
/*	$NetBSD: kern_physio.c,v 1.28 1997/05/19 10:43:28 pk Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1994 Christopher G. Demetriou
 * Copyright (c) 1982, 1986, 1990, 1993
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
 *	@(#)kern_physio.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! physio(9): "physical I/O" on behalf of a user, directly between a raw (character) device
//! and the user's buffers, bypassing the buffer cache.
//!
//! Upstream: sys/kern/kern_physio.c @ 3ce1f3f79392
//!
//! The routines are described in Leffler, et al.: The Design and Implementation of the 4.3BSD
//! UNIX Operating System (Addison Welley, 1989), on pages 231-233. Comments in brackets are
//! from their pseudo-code implementation.
//!
//! A raw device's `d_read`/`d_write` (`rdread`, `sdread`, ...) calls [`physio`] with its
//! strategy routine and a `minphys` that bounds one transfer; physio wires each piece of the
//! user's buffer (`uvm_vslock_device`, which bounces it through DMA-reachable pages when the
//! device cannot reach them), maps it into the kernel (`vmapbuf`, the machine's
//! `vm_machdep.c`), hands one private buffer to the strategy routine and waits for it.
//!
//! ## Deviations
//! - The strategy and minphys routines are Rust `fn`s (`fn(&'static Buf)`, `fn(&Buf)`);
//!   `flags` is a `b_flags` value (`B_READ` or `B_WRITE`).
//! - `KASSERTMSG` is `kassert!` (the message is the C's comment).
//! - `pool_get(&bufpool, PR_WAITOK)` cannot fail in C; a `None` here panics.

use core::ptr::{self, NonNull};

use crate::kassert;
use crate::kern::kern_synch::tsleep_nsec;
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::BUFPOOL;
use crate::kern::vfs_subr::brelvp;
use crate::machine::cpu::{curproc, vmapbuf, vunmapbuf};
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::{B_BUSY, B_DONE, B_ERROR, B_PHYS, B_RAW, B_READ, B_WRITE, Buf};
use crate::sys::errno::Errno;
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::param::{DEV_BSHIFT, DEV_BSIZE, MAXPHYS, PRIBIO};
use crate::sys::pool::{PR_WAITOK, PR_ZERO};
use crate::sys::systm::INFSLP;
use crate::sys::types::{Daddr, Dev, Off};
use crate::sys::uio::Uio;
use crate::uvm::uvm_glue::{uvm_vslock_device, uvm_vsunlock_device};

/// Do "physical I/O" on behalf of a user: transfer `uio` (user space, at a `DEV_BSIZE`
/// multiple offset) to or from `dev` through `strategy`, at most `minphys` bytes at a time.
///
/// `EINVAL` when the offset is not block aligned; otherwise the first transfer error (the
/// buffer's `b_error`, `EIO` without one) or a wiring failure. On return `uio` accounts for
/// what was transferred, including a short transfer at the end of the device, which ends
/// the I/O without an error.
pub fn physio(
    strategy: fn(&'static Buf),
    dev: Dev,
    flags: i64,
    minphys: fn(&Buf),
    uio: &mut Uio<'_>,
) -> Result<(), Errno> {
    if uio.uio_offset % DEV_BSIZE as Off != 0 {
        return Err(Errno::EINVAL);
    }

    let Some(p) = curproc() else {
        panic(format_args!("physio: no curproc"));
    };
    let flags = flags & (B_READ | B_WRITE);

    // Create a buffer.
    let s = splbio();
    let Some(mem) = pool_get(&BUFPOOL, PR_WAITOK | PR_ZERO) else {
        panic(format_args!("physio: pool_get"));
    };
    let item = mem.cast::<Buf>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Buf>()` bytes, written once
    // before anything else sees it.
    unsafe { item.as_ptr().write(Buf::new()) };
    // SAFETY: as above; the item stays allocated until the pool_put at the end, after the
    // strategy routine is done with it.
    let bp: &'static Buf = unsafe { item.as_ref() };

    // [set up the fixed part of the buffer for a transfer]
    // bp->b_vnbufs.le_next = NOLIST: `b_onvnbufs` is false.
    bp.b_dev.set(dev);
    bp.b_error.set(None);
    bp.b_proc.set(ptr::from_ref(p));
    bp.b_flags.set(B_BUSY);
    splx(s);

    let mut error = Ok(());

    // [while there are data to transfer and no I/O error]
    'done: for i in 0..uio.uio_iovcnt() {
        while uio.uio_iov[i].iov_len > 0 {
            let iov_base = uio.uio_iov[i].iov_base as usize;

            // [mark the buffer busy for physical I/O] (i.e. set B_PHYS (because it's an I/O
            // to user memory), and B_RAW, because B_RAW is to be "Set by physio for raw
            // transfers.", in addition to the "busy" and read/write flag.)
            bp.clr(B_DONE | B_ERROR);
            bp.set(B_BUSY | B_PHYS | B_RAW | flags);

            // [set up the buffer for a maximum-sized transfer]
            bp.b_blkno.set((uio.uio_offset >> DEV_BSHIFT) as Daddr);
            bp.b_bcount.set(physio_bcount(uio.uio_iov[i].iov_len));

            // [call minphys to bound the transfer size] and remember the amount of data to
            // transfer, for later comparison.
            minphys(bp);
            let todo = bp.b_bcount.get();
            kassert!(todo >= 0); // minphys broken
            let todo = todo as usize;

            // [lock the part of the user address space involved in the transfer]
            // Beware vmapbuf(); it clobbers b_data and saves it in b_saveaddr. However,
            // vunmapbuf() restores it.
            let access = if flags & B_READ != 0 {
                PROT_READ | PROT_WRITE
            } else {
                PROT_READ
            };
            let map = match uvm_vslock_device(p, iov_base, todo, access) {
                Ok(map) => map,
                Err(e) => {
                    error = Err(e);
                    break 'done;
                }
            };
            match map {
                Some(bounce) => bp.b_data.set(bounce.as_ptr()),
                None => {
                    bp.b_data.set(iov_base as *mut u8);
                    vmapbuf(bp, todo);
                }
            }

            // [call strategy to start the transfer]
            strategy(bp);

            // Note that the raise/wait/lower/get error steps below would be done by
            // biowait(), but we want to unlock the address space before we lower the
            // priority.
            //
            // [raise the priority level to splbio]
            let s = splbio();

            // [wait for the transfer to complete]
            while !bp.isset(B_DONE) {
                let _ = tsleep_nsec(ptr::from_ref(bp), PRIBIO + 1, "physio", INFSLP);
            }

            // Mark it busy again, so nobody else will use it.
            bp.set(B_BUSY);

            // [lower the priority level]
            splx(s);

            // [unlock the part of the address space previously locked]
            if map.is_none() {
                vunmapbuf(bp, todo);
            }
            uvm_vsunlock_device(p, iov_base, todo, map);

            // remember error value (save a splbio/splx pair)
            if bp.isset(B_ERROR) {
                error = Err(bp.b_error.get().unwrap_or(Errno::EIO));
            }

            // [deduct the transfer size from the total number of data to transfer]
            let resid = bp.b_resid.get();
            kassert!(resid <= i64::MAX as usize); // strategy broken
            let done = bp.b_bcount.get() - resid as i64;
            kassert!(done >= 0); // strategy broken
            kassert!(done as usize <= todo); // strategy broken
            physio_advance(uio, i, done as usize);

            // Now, check for an error. Also, handle weird end-of-disk semantics.
            if error.is_err() || (done as usize) < todo {
                break 'done;
            }
        }
    }

    // done:
    // [clean up the state of the buffer]
    let s = splbio();
    // XXXCDC: is this necessary?
    if bp.b_vp.get().is_some() {
        brelvp(bp);
    }
    splx(s);
    pool_put(&BUFPOOL, NonNull::from(bp).cast());

    error
}

/// The `b_bcount` physio starts a piece with: the iovec's length, limited to `LONG_MAX`
/// because `iov_len` is a `size_t` (unsigned) and `b_bcount` a `long` (signed), before the
/// provided minphys bounds it.
fn physio_bcount(iov_len: usize) -> i64 {
    i64::try_from(iov_len).unwrap_or(i64::MAX)
}

/// Accounts `done` transferred bytes of the iovec `i` of `uio`: the iovec shrinks and moves
/// on, the offset grows, the residual count shrinks.
fn physio_advance(uio: &mut Uio<'_>, i: usize, done: usize) {
    let iov = &mut uio.uio_iov[i];
    iov.iov_len -= done;
    iov.iov_base = iov.iov_base.wrapping_byte_add(done);
    uio.uio_offset += done as Off;
    uio.uio_resid -= done;
}

/// Leffler, et al., says on p. 231: "The minphys() routine is called by physio() to adjust
/// the size of each I/O transfer before the latter is passed to the strategy routine..."
///
/// so, just adjust the buffer's count accounting to `MAXPHYS` here.
pub fn minphys(bp: &Buf) {
    if bp.b_bcount.get() > MAXPHYS as i64 {
        bp.b_bcount.set(MAXPHYS as i64);
    }
}

#[cfg(test)]
mod tests;
