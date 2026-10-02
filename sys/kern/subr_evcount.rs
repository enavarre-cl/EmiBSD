/*	$OpenBSD: subr_evcount.c,v 1.16 2023/09/16 09:33:27 mpi Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2004 Artur Grabowski <art@openbsd.org>
 * Copyright (c) 2004 Aaron Campbell <aaron@openbsd.org>
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES,
 * INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY
 * AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL
 * THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 * EXEMPLARY, OR CONSEQUENTIAL  DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! Event counters: `kern/subr_evcount.c`.
//!
//! Upstream: sys/kern/subr_evcount.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `evcount_attach`, `evcount_detach`, `evcount_inc`,
//! `evcount_percpu` and `evcount_init_percpu` (the `counters_*` per-CPU side is reported
//! until `percpu` arrives, M5); `evcount_sysctl` comes with `sysctl(2)` (M6).

use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::kassert;
use crate::sys::evcount::{Evcount, EvcountList};
use crate::sys::queue::TailqHead;
use crate::unported;

/// A list head that can be a static: every access happens at attach time on the boot CPU.
struct EvcountHead(TailqHead<EvcountList>);

// SAFETY: see the type's doc; `evcount_inc` touches only the atomic count.
unsafe impl Sync for EvcountHead {}

/// `evcount_list`.
static EVCOUNT_LIST: EvcountHead = EvcountHead(TailqHead::new());
/// `evcount_percpu_init_list`: the counters that asked to be per-CPU before `percpu` was up.
static EVCOUNT_PERCPU_INIT_LIST: EvcountHead = EvcountHead(TailqHead::new());
/// `evcount_percpu_done`.
static EVCOUNT_PERCPU_DONE: AtomicBool = AtomicBool::new(false);
/// The lists are `TAILQ_HEAD_INITIALIZER`s in C; here they are initialised on first use.
static LISTS_INITIALISED: AtomicBool = AtomicBool::new(false);

fn lists() -> (
    &'static TailqHead<EvcountList>,
    &'static TailqHead<EvcountList>,
) {
    if !LISTS_INITIALISED.swap(true, Ordering::Relaxed) {
        EVCOUNT_LIST.0.init();
        EVCOUNT_PERCPU_INIT_LIST.0.init();
    }
    (&EVCOUNT_LIST.0, &EVCOUNT_PERCPU_INIT_LIST.0)
}

/// `evcount_attach`: registers `ec` as `name` with `data` as its user pointer.
pub fn evcount_attach(ec: &'static Evcount, name: &'static str, data: *const ()) {
    static NEXTID: AtomicI32 = AtomicI32::new(0);
    let (list, _) = lists();

    // memset(ec, 0, sizeof(*ec))
    ec.ec_count.store(0, Ordering::Relaxed);
    ec.ec_percpu.set(core::ptr::null());
    ec.ec_name.set(name);
    ec.ec_id.set(NEXTID.fetch_add(1, Ordering::Relaxed) + 1);
    ec.ec_data.set(data);
    // SAFETY: a counter is attached once, at attach time on the boot CPU.
    unsafe { list.insert_tail(ec) };
}

/// `evcount_percpu`: asks for a per-CPU counter.
pub fn evcount_percpu(ec: &'static Evcount) {
    let (list, init_list) = lists();
    if !EVCOUNT_PERCPU_DONE.load(Ordering::Relaxed) {
        // SAFETY: `ec` is attached (on `evcount_list`); attach time, boot CPU.
        unsafe {
            list.remove(ec);
            init_list.insert_tail(ec);
        }
    } else {
        // ec->ec_percpu = counters_alloc(1)
        let _ = unported!("counters_alloc (evcount_percpu, M5)");
    }
}

/// `evcount_init_percpu`: once `percpu` is up, gives the waiting counters their per-CPU
/// storage and merges the lists.
pub fn evcount_init_percpu() {
    let (list, init_list) = lists();
    kassert!(!EVCOUNT_PERCPU_DONE.load(Ordering::Relaxed));

    for _ec in init_list.iter() {
        // ec->ec_percpu = counters_alloc(1); counters_add(ec->ec_percpu, 0, ec->ec_count);
        // ec->ec_count = 0;
        let _ = unported!("counters_alloc (evcount_init_percpu, M5)");
    }

    // SAFETY: both lists are initialised and distinct; attach time, boot CPU.
    unsafe { list.concat(init_list) };
    EVCOUNT_PERCPU_DONE.store(true, Ordering::Relaxed);
}

/// `evcount_detach`: unregisters `ec`.
pub fn evcount_detach(ec: &'static Evcount) {
    let (list, _) = lists();
    // SAFETY: `ec` is attached; detach time, boot CPU.
    unsafe { list.remove(ec) };
    if !ec.ec_percpu.get().is_null() {
        let _ = unported!("counters_free (evcount_detach, M5)");
        ec.ec_percpu.set(core::ptr::null());
    }
}

/// `evcount_inc`: counts one event.
pub fn evcount_inc(ec: &Evcount) {
    if !ec.ec_percpu.get().is_null() {
        let _ = unported!("counters_inc (evcount_inc, M5)");
    } else {
        ec.ec_count.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_count_detach() {
        static A: Evcount = Evcount::new();
        static B: Evcount = Evcount::new();
        evcount_attach(&A, "com0", core::ptr::null());
        evcount_attach(&B, "pckbc0", core::ptr::null());
        assert_ne!(A.ec_id.get(), B.ec_id.get());
        assert_eq!(A.ec_name.get(), "com0");
        evcount_inc(&A);
        evcount_inc(&A);
        assert_eq!(A.ec_count.load(Ordering::Relaxed), 2);
        assert!(lists().0.iter().any(|e| core::ptr::eq(e, &A)));
        evcount_detach(&A);
        evcount_detach(&B);
        assert!(!lists().0.iter().any(|e| core::ptr::eq(e, &A)));
    }
}
