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

/* <CODE> */
//! `<sys/evcount.h>`: event counters, the interrupt counts `vmstat -i` shows.
//!
//! Upstream: sys/sys/evcount.h @ 3ce1f3f79392
//!
//! Status: `ported`. Milestone M4 ports `struct evcount`; M11e gives `ec_percpu` its type,
//! the `struct cpumem *` of the per-CPU counters (`subr_percpu.c`). The functions are
//! `kern/subr_evcount.rs`.
//!
//! ## Deviations
//! - `ec_count` is the first field and an `AtomicU64`: the amd64 interrupt stubs increment it
//!   in assembly through the handler's `ih_count`, and `evcount_inc` from Rust.
//! - `ec_name` is a `&'static str` (the C's `const char *` is always a driver name).
//! - `ec_percpu` is an `Option<CpumemPtr>` (`NULL` is `None`).

use core::cell::Cell;
use core::sync::atomic::AtomicU64;

use crate::queue_adapter;
use crate::sys::percpu::CpumemPtr;
use crate::sys::queue::TailqEntry;

/// `struct evcount`.
#[repr(C)]
pub struct Evcount {
    /// `ec_count`: main counter.
    pub ec_count: AtomicU64,
    /// `ec_id`: counter ID.
    pub ec_id: Cell<i32>,
    /// `ec_name`: counter name.
    pub ec_name: Cell<&'static str>,
    /// `ec_data`: user data (the interrupt vector, for `KERN_INTRCNT_VECTOR`).
    pub ec_data: Cell<*const ()>,
    /// `ec_percpu`: per-cpu counter.
    pub ec_percpu: Cell<Option<CpumemPtr>>,
    /// `next`: the `evcount_list` link.
    pub next: TailqEntry<Evcount>,
}

// SAFETY: written by `evcount_attach`/`evcount_detach`/`evcount_percpu` at attach time and by
// `evcount_init_percpu`, all on the boot CPU before the other CPUs run, and counted from
// interrupt context; the counts are atomic and the rest is only read afterwards.
unsafe impl Sync for Evcount {}

impl Evcount {
    /// A detached, zero counter.
    pub const fn new() -> Self {
        Self {
            ec_count: AtomicU64::new(0),
            ec_id: Cell::new(0),
            ec_name: Cell::new(""),
            ec_data: Cell::new(core::ptr::null()),
            ec_percpu: Cell::new(None),
            next: TailqEntry::new(),
        }
    }
}

impl Default for Evcount {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(, evcount)`: the list of counters, through `next`.
    pub EvcountList: Evcount, next => TailqEntry<Evcount>
);

const _: () = {
    assert!(core::mem::offset_of!(Evcount, ec_count) == 0);
};
/* </CODE> */
