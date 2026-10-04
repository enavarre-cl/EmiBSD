/*	$OpenBSD: timetc.h,v 1.14 2023/02/04 19:19:35 cheloha Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2000 Poul-Henning Kamp <phk@FreeBSD.org>
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
 * If we meet some day, and you think this stuff is worth it, you
 * can buy me a beer in return. Poul-Henning Kamp
 */
/* </LICENSES> */

//! `<sys/timetc.h>`: the timecounter interface between the hardware which implements a
//! timecounter and the MI code which uses this to keep track of time.
//!
//! Upstream: sys/sys/timetc.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports `struct timecounter`, `struct timekeep` and
//! `TK_VERSION`; the functions are in `kern/kern_tc.rs`, with `tc_lock` (`TC_LOCK`, the C's
//! `extern`). `timekeep_object` (the page shared with userland, M6) is not here.
//!
//! A timecounter is a binary counter which has two properties:
//! - it runs at a fixed, known frequency.
//! - it has sufficient bits to not roll over in less than approximately
//!   max(2 msec, 2/HZ seconds). (The value 2 here is really 1 + delta, for some
//!   indeterminate value of delta.)
//!
//! ## Deviations
//! - The fields a driver fills in after its static initialiser (`tc_frequency`, `tc_priv`,
//!   `tc_get_timecount`, `tc_user`) and the ones `tc_init`/`tc_reset_quality` write are
//!   `Cell`s, so a `static` timecounter is registered through `&'static`.
//! - `tc_priv` is an opaque pointer, as in C; the driver casts it back.

use core::cell::Cell;
use core::ptr;

use crate::queue_adapter;
use crate::sys::queue::SlistEntry;
use crate::sys::time::Bintime;

/// `timecounter_get_t`: reads the counter. It is not required to mask any unimplemented
/// bits out, as long as they are constant.
pub type TimecounterGet = fn(&Timecounter) -> u32;

/// `struct timecounter`.
///
/// Locks used to protect struct members in this file:
/// - I: immutable after initialization
/// - T: `tc_lock`
/// - W: `windup_mtx`
pub struct Timecounter {
    /// \[I\] `tc_get_timecount`: this function reads the counter.
    pub tc_get_timecount: Cell<TimecounterGet>,
    /// \[I\] `tc_counter_mask`: this mask should mask off any unimplemented bits.
    pub tc_counter_mask: Cell<u32>,
    /// \[I\] `tc_frequency`: frequency of the counter in Hz.
    pub tc_frequency: Cell<u64>,
    /// \[I\] `tc_name`: name of the timecounter.
    pub tc_name: &'static str,
    /// \[I\] `tc_quality`: used to determine if this timecounter is better than another
    /// timecounter higher means better. Negative means "only use at explicit request".
    pub tc_quality: Cell<i32>,
    /// \[I\] `tc_priv`: pointer to the timecounter's private parts.
    pub tc_priv: Cell<*const ()>,
    /// \[I\] `tc_user`: expose this timecounter to userland.
    pub tc_user: Cell<i32>,
    /// \[I\] `tc_next`: pointer to the next timecounter.
    pub tc_next: SlistEntry<Timecounter>,
    /// \[T,W\] `tc_freq_adj`: current frequency adjustment.
    pub tc_freq_adj: Cell<i64>,
    /// \[I\] `tc_precision`: precision of the counter. Computed in `tc_init()`.
    pub tc_precision: Cell<u64>,
}

// SAFETY: a timecounter is written by its driver before `tc_init` and by `tc_init` on the
// boot CPU; afterwards only `tc_freq_adj` changes, under `windup_mtx`.
unsafe impl Sync for Timecounter {}

impl Timecounter {
    /// The C's static initialiser: `tc_freq_adj` and `tc_precision` zero.
    pub const fn new(
        tc_get_timecount: TimecounterGet,
        tc_counter_mask: u32,
        tc_frequency: u64,
        tc_name: &'static str,
        tc_quality: i32,
        tc_user: i32,
    ) -> Self {
        Self {
            tc_get_timecount: Cell::new(tc_get_timecount),
            tc_counter_mask: Cell::new(tc_counter_mask),
            tc_frequency: Cell::new(tc_frequency),
            tc_name,
            tc_quality: Cell::new(tc_quality),
            tc_priv: Cell::new(ptr::null()),
            tc_user: Cell::new(tc_user),
            tc_next: SlistEntry::new(),
            tc_freq_adj: Cell::new(0),
            tc_precision: Cell::new(0),
        }
    }

    /// `tc->tc_get_timecount(tc)`.
    pub fn get_timecount(&self) -> u32 {
        (self.tc_get_timecount.get())(self)
    }
}

queue_adapter!(
    /// `SLIST_HEAD(, timecounter)`: the registered timecounters, through `tc_next`.
    pub TcList: Timecounter, tc_next => SlistEntry<Timecounter>
);

/// `struct timekeep`: the timehands and timecounter state shared with userland.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Timekeep {
    // set at initialization
    /// `tk_version`: version number.
    pub tk_version: u32,

    // timehands members
    /// `tk_scale`.
    pub tk_scale: u64,
    /// `tk_offset_count`.
    pub tk_offset_count: u32,
    /// `tk_offset`.
    pub tk_offset: Bintime,
    /// `tk_naptime`.
    pub tk_naptime: Bintime,
    /// `tk_boottime`.
    pub tk_boottime: Bintime,
    /// `tk_generation` (volatile).
    pub tk_generation: u32,

    // timecounter members
    /// `tk_user`.
    pub tk_user: i32,
    /// `tk_counter_mask`.
    pub tk_counter_mask: u32,
}

/// `TK_VERSION`.
pub const TK_VERSION: u32 = 0;
