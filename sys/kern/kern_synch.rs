/*	$OpenBSD: kern_synch.c,v 1.234 2026/06/16 19:29:25 bluhm Exp $	*/
/*	$NetBSD: kern_synch.c,v 1.37 1996/04/22 01:38:37 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1990, 1991, 1993
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
 *	@(#)kern_synch.c	8.6 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! Sleep and wakeup: `kern/kern_synch.c`.
//!
//! Upstream: sys/kern/kern_synch.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 only needs a `wakeup(9)` that callers can name; it reports the
//! gap, once, because there is nothing to wake before the scheduler, as does `wakeup_one`.
//! M5 (part b1) adds the reference counts (`refcnt_init[_trace]`, `refcnt_take`,
//! `refcnt_rele[_wake]`, `refcnt_read`, `refcnt_shared`) and `cond_init`/
//! `cond_signal_handler`; `tsleep`, `msleep`, `sleep_setup`/`sleep_finish`, the sleep queues,
//! `wakeup_n`, `endtsleep`, `unsleep`, `refcnt_finalize` and `cond_wait` arrive with part b2.

use core::sync::atomic::{Ordering, fence};

use crate::kassert;
use crate::sys::proc::Cond;
use crate::sys::refcnt::Refcnt;
use crate::unported;

/// `wakeup(9)`: wakes every thread sleeping on `ident`; nobody sleeps yet.
pub fn wakeup<T>(_ident: *const T) {
    let _ = unported!("wakeup (kern_synch.c, M5)");
}

/// `wakeup_one(9)`: wakes one thread sleeping on `ident`; nobody sleeps yet.
pub fn wakeup_one<T>(_ident: *const T) {
    let _ = unported!("wakeup_one (kern_synch.c, M5)");
}

/// `refcnt_init`.
pub fn refcnt_init(r: &Refcnt) {
    refcnt_init_trace(r, 0);
}

/// `refcnt_init_trace`.
pub fn refcnt_init_trace(r: &Refcnt, trace: i32) {
    r.r_traceidx.set(trace);
    r.r_refs.store(1, Ordering::Relaxed);
    // TRACEINDEX(refcnt, ...): dt(4), not configured.
}

/// `refcnt_take`.
pub fn refcnt_take(r: &Refcnt) {
    let refs = r.r_refs.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    kassert!(refs > 1);
}

/// `refcnt_rele`: drops a reference; `true` when it was the last.
pub fn refcnt_rele(r: &Refcnt) -> bool {
    fence(Ordering::Release); // membar_exit_before_atomic()
    let refs = r.r_refs.fetch_sub(1, Ordering::Relaxed).wrapping_sub(1);
    kassert!(refs != u32::MAX);
    if refs == 0 {
        fence(Ordering::Acquire); // membar_enter_after_atomic()
        return true;
    }
    false
}

/// `refcnt_rele_wake`.
pub fn refcnt_rele_wake(r: &Refcnt) {
    if refcnt_rele(r) {
        wakeup_one(core::ptr::from_ref(r));
    }
}

// refcnt_finalize: sleeps for the last reference (M5-b2).

/// `refcnt_read`.
pub fn refcnt_read(r: &Refcnt) -> u32 {
    r.r_refs.load(Ordering::Relaxed)
}

/// `refcnt_shared(_r)`.
pub fn refcnt_shared(r: &Refcnt) -> bool {
    refcnt_read(r) > 1
}

/// `cond_init`.
pub fn cond_init(c: &Cond) {
    c.c_wait.store(1, Ordering::Relaxed);
}

/// `cond_signal_handler`: the timeout/task handler that signals a `cond`.
pub fn cond_signal_handler(arg: *mut core::ffi::c_void) {
    // SAFETY: `arg` is the `cond` its waiter armed the handler with, alive while it waits.
    let c = unsafe { &*arg.cast::<Cond>() };

    c.c_wait.store(0, Ordering::Relaxed);

    wakeup_one(core::ptr::from_ref(c));
}

// cond_wait: sleeps until the cond is signalled (M5-b2).
