/*	$OpenBSD: kern_softintr.c,v 1.1 2025/04/23 15:07:00 visa Exp $	*/
/*
 * Copyright (c) 2021, 2025 Visa Hankala
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

//! Machine-independent soft interrupts: `kern/kern_softintr.c` (`__USE_MI_SOFTINTR`).
//!
//! Upstream: sys/kern/kern_softintr.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `softintr_init`, `softintr_dispatch`,
//! `softintr_establish`, `softintr_disestablish` and `softintr_schedule`. `sched_barrier`
//! (the wait for a running handler on another CPU) and `assertwaitok` are reported until M5.
//!
//! ## Deviations
//! - `KERNEL_LOCK()`/`KERNEL_UNLOCK()` around a handler without `SIF_MPSAFE` are nothing
//!   without `MULTIPROCESSOR`, as in C.
//! - The handle is a `NonNull<SoftintrHand>` where the C passes `void *`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::panic;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::machine::intr::{
    IPL_HIGH, IPL_MPSAFE, IPL_SOFTCLOCK, IPL_SOFTNET, IPL_SOFTTTY, IPL_TTY, softintr,
};
use crate::queue_adapter;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT};
use crate::sys::mutex::Mutex;
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::softintr::{NSOFTINTR, SOFTINTR_CLOCK, SOFTINTR_NET, SOFTINTR_TTY};
use crate::unported;
use crate::uvm::uvm_init::UVMEXP;

/// A soft interrupt handler's function.
pub type SoftintrFn = fn(*mut c_void);

/// `struct softintr_hand`: an established soft interrupt handler.
pub struct SoftintrHand {
    /// `sih_q`: the queue link, when pending.
    pub sih_q: TailqEntry<SoftintrHand>,
    /// `sih_fn`.
    pub sih_fn: SoftintrFn,
    /// `sih_arg`.
    pub sih_arg: *mut c_void,
    /// `sih_runner`: the CPU running the handler, if one is.
    pub sih_runner: Cell<*const ()>,
    /// `sih_level`: `SOFTINTR_*`.
    pub sih_level: i32,
    /// `SIF_*`.
    pub sih_flags: u16,
    /// `SIS_*`.
    pub sih_state: Cell<u16>,
}

/// `SIF_MPSAFE`: the handler runs without the kernel lock.
pub const SIF_MPSAFE: u16 = 0x0001;

/// `SIS_DYING`: being disestablished.
pub const SIS_DYING: u16 = 0x0001;
/// `SIS_PENDING`: queued.
pub const SIS_PENDING: u16 = 0x0002;
/// `SIS_RESTART`: scheduled again while running.
pub const SIS_RESTART: u16 = 0x0004;

queue_adapter!(
    /// `TAILQ_HEAD(softintr_queue, softintr_hand)`.
    pub SoftintrQueue: SoftintrHand, sih_q => TailqEntry<SoftintrHand>
);

/// `softintr_queue[NSOFTINTR]`: one queue per level, under `softintr_lock`.
struct SoftintrQueues([TailqHead<SoftintrQueue>; NSOFTINTR]);

// SAFETY: every access is under `SOFTINTR_LOCK`.
unsafe impl Sync for SoftintrQueues {}

/// `softintr_queue`.
static SOFTINTR_QUEUE: SoftintrQueues = SoftintrQueues([const { TailqHead::new() }; NSOFTINTR]);
/// `softintr_lock`.
static SOFTINTR_LOCK: Mutex = Mutex::new(IPL_HIGH);

/// `softintr_init`: the queues.
pub fn softintr_init() {
    for queue in &SOFTINTR_QUEUE.0 {
        queue.init();
    }
}

/// `softintr_dispatch`: runs the handlers pending at `level`, from the soft interrupt stub.
pub fn softintr_dispatch(level: i32) {
    let ci = Machine::curcpu_ptr();
    let queue = &SOFTINTR_QUEUE.0[level as usize];

    mtx_enter(&SOFTINTR_LOCK);
    while let Some(sih) = queue.first() {
        kassert!((sih.sih_state.get() & (SIS_PENDING | SIS_RESTART)) == SIS_PENDING);
        kassert!(sih.sih_runner.get().is_null());

        sih.sih_state.set(sih.sih_state.get() & !SIS_PENDING);
        // SAFETY: `sih` is on this queue (it was its first element), under the lock.
        unsafe { queue.remove(sih) };
        sih.sih_runner.set(ci);
        mtx_leave(&SOFTINTR_LOCK);

        // KERNEL_LOCK() unless SIF_MPSAFE: nothing without MULTIPROCESSOR.
        (sih.sih_fn)(sih.sih_arg);

        mtx_enter(&SOFTINTR_LOCK);
        kassert!((sih.sih_state.get() & SIS_PENDING) == 0);
        sih.sih_runner.set(ptr::null());
        if sih.sih_state.get() & SIS_RESTART != 0 {
            // SAFETY: `sih` is established and not queued (its runner just finished).
            unsafe { queue.insert_tail(sih) };
            sih.sih_state.set(sih.sih_state.get() | SIS_PENDING);
            sih.sih_state.set(sih.sih_state.get() & !SIS_RESTART);
        }

        UVMEXP.softs.fetch_add(1, Ordering::Relaxed);
    }
    mtx_leave(&SOFTINTR_LOCK);
}

/// `softintr_establish`: registers `func(arg)` to run at the soft level that serves `ipl`.
pub fn softintr_establish(
    ipl: i32,
    func: SoftintrFn,
    arg: *mut c_void,
) -> Option<NonNull<SoftintrHand>> {
    let mut flags = 0;

    if ipl & IPL_MPSAFE != 0 {
        flags |= SIF_MPSAFE;
    }
    let ipl = ipl & !IPL_MPSAFE;

    // IPL_SOFT: not defined on amd64 or arm64.
    let level = match ipl {
        l if l == IPL_SOFTCLOCK => SOFTINTR_CLOCK,
        l if l == IPL_SOFTNET => SOFTINTR_NET,
        l if l == IPL_TTY || l == IPL_SOFTTTY => SOFTINTR_TTY,
        _ => panic(format_args!("softintr_establish: unhandled ipl {ipl}")),
    };

    let sih = malloc(size_of::<SoftintrHand>(), M_DEVBUF, M_NOWAIT)?.cast::<SoftintrHand>();
    // SAFETY: a fresh allocation of the right size and alignment, written once before use.
    unsafe {
        sih.write(SoftintrHand {
            sih_q: TailqEntry::new(),
            sih_fn: func,
            sih_arg: arg,
            sih_runner: Cell::new(ptr::null()),
            sih_level: level,
            sih_flags: flags,
            sih_state: Cell::new(0),
        });
    }
    Some(sih)
}

/// `softintr_disestablish`: removes a handler, waiting for it if it is running elsewhere.
///
/// # Safety
///
/// `sih` must come from `softintr_establish` and not be used afterwards.
pub unsafe fn softintr_disestablish(sih: NonNull<SoftintrHand>) {
    // assertwaitok(): M5.
    // SAFETY: the caller's guarantee: an established handler.
    let hand = unsafe { sih.as_ref() };

    mtx_enter(&SOFTINTR_LOCK);
    hand.sih_state.set(hand.sih_state.get() | SIS_DYING);
    hand.sih_state.set(hand.sih_state.get() & !SIS_RESTART);
    if hand.sih_state.get() & SIS_PENDING != 0 {
        hand.sih_state.set(hand.sih_state.get() & !SIS_PENDING);
        // SAFETY: pending means queued on its level's queue, under the lock.
        unsafe { SOFTINTR_QUEUE.0[hand.sih_level as usize].remove(hand) };
    }
    let runner = hand.sih_runner.get();
    mtx_leave(&SOFTINTR_LOCK);

    if !runner.is_null() {
        let _ = unported!("sched_barrier (softintr_disestablish, M5)");
    }

    kassert!((hand.sih_state.get() & (SIS_PENDING | SIS_RESTART)) == 0);
    kassert!(hand.sih_runner.get().is_null());

    free(sih.cast::<u8>(), M_DEVBUF, size_of::<SoftintrHand>());
}

/// `softintr_schedule`: queues the handler (or marks it for a restart if it is running) and
/// raises the soft interrupt.
pub fn softintr_schedule(sih: NonNull<SoftintrHand>) {
    // SAFETY: an established handler (`softintr_establish`), alive until disestablished.
    let hand = unsafe { sih.as_ref() };
    let queue = &SOFTINTR_QUEUE.0[hand.sih_level as usize];

    mtx_enter(&SOFTINTR_LOCK);
    kassert!((hand.sih_state.get() & SIS_DYING) == 0);
    if hand.sih_runner.get().is_null() {
        kassert!((hand.sih_state.get() & SIS_RESTART) == 0);
        if hand.sih_state.get() & SIS_PENDING == 0 {
            // SAFETY: not pending, so not on any queue; under the lock.
            unsafe { queue.insert_tail(hand) };
            hand.sih_state.set(hand.sih_state.get() | SIS_PENDING);
            // Call softintr() while SPL is still at IPL_HIGH.
            softintr(hand.sih_level);
        }
    } else {
        kassert!((hand.sih_state.get() & SIS_PENDING) == 0);
        hand.sih_state.set(hand.sih_state.get() | SIS_RESTART);
    }
    mtx_leave(&SOFTINTR_LOCK);
}
