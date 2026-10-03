//! Host tests for the clock interrupt queue, over the host double's `cpu_info` and the dummy
//! timecounter (which advances on every read).

use core::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Mutex as StdMutex;

use super::*;
use crate::kern::kern_tc::{tc_init, tc_reset_quality, tc_ticktock};
use crate::sys::timetc::Timecounter;

/// The host has one `cpu_info` and one queue: the tests serialise on it.
static LOCK: StdMutex<()> = StdMutex::new(());

static REARMED: AtomicU64 = AtomicU64::new(0);
static TRIGGERED: AtomicU32 = AtomicU32::new(0);

fn fake_rearm(_cookie: *mut c_void, nsecs: u64) {
    REARMED.store(nsecs, Ordering::Relaxed);
}

fn fake_trigger(_cookie: *mut c_void) {
    TRIGGERED.fetch_add(1, Ordering::Relaxed);
}

const FAKE_INTRCLOCK: Intrclock = Intrclock {
    ic_cookie: ptr::null_mut(),
    ic_rearm: fake_rearm,
    ic_trigger: fake_trigger,
};

static RUNS: AtomicU32 = AtomicU32::new(0);

fn count_and_reschedule(cr: &Clockrequest, _frame: *mut c_void, arg: *mut c_void) {
    RUNS.fetch_add(1, Ordering::Relaxed);
    if !arg.is_null() {
        clockrequest_advance(cr, 1_000_000);
    }
}

#[test]
fn nsec_advance_counts_periods() {
    let mut next = 100;
    assert_eq!(nsec_advance(&mut next, 10, 50), 0);
    assert_eq!(next, 100);
    assert_eq!(nsec_advance(&mut next, 10, 105), 1);
    assert_eq!(next, 110);
    assert_eq!(nsec_advance(&mut next, 10, 155), 5);
    assert_eq!(next, 160);
}

#[test]
fn bind_schedule_dispatch_cancel() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let ci = curcpu();
    let cq = Machine::ci_queue(ci);
    clockqueue_init(cq);
    assert!(cq.cq_flags.get() & CQ_INIT != 0);
    clockqueue_intrclock_install(cq, &FAKE_INTRCLOCK);

    static ONCE: Clockintr = Clockintr::new();
    static AGAIN: Clockintr = Clockintr::new();
    clockintr_bind(&ONCE, ci, count_and_reschedule, ptr::null_mut());
    clockintr_bind(&AGAIN, ci, count_and_reschedule, 1 as *mut c_void);

    // Both due now: `0` is in the past.
    RUNS.store(0, Ordering::Relaxed);
    clockintr_schedule(&ONCE, 0);
    clockintr_schedule(&AGAIN, 0);
    assert!(TRIGGERED.load(Ordering::Relaxed) > 0, "due now: triggered");
    assert_eq!(cq.cq_pend.iter().count(), 2);

    crate::machine::intr::splraise(IPL_CLOCK);
    assert_eq!(clockintr_dispatch(ptr::null_mut()), 1);
    assert_eq!(RUNS.load(Ordering::Relaxed), 2);
    // ONCE ran and is idle; AGAIN rescheduled itself 1 ms after its dispatch time.
    assert_eq!(cq.cq_pend.iter().count(), 1);
    assert!(ptr::eq(cq.cq_pend.first().unwrap(), &AGAIN));
    assert!(REARMED.load(Ordering::Relaxed) > 0, "rearmed for AGAIN");
    let stat = cq.cq_stat.get();
    assert_eq!(stat.cs_run, 2);
    assert_eq!(stat.cs_prompt, 1);

    // Too early: nothing runs, the clock is rearmed, cs_early counts.
    assert_eq!(clockintr_dispatch(ptr::null_mut()), 0);
    assert_eq!(cq.cq_stat.get().cs_early, 1);

    // Advance by a period from now: at least one period, then it is pending in the future.
    let n = clockintr_advance(&ONCE, 5_000_000);
    assert!(n >= 1);
    assert_eq!(cq.cq_pend.iter().count(), 2);
    clockintr_cancel(&ONCE);
    assert_eq!(cq.cq_pend.iter().count(), 1);
    clockintr_cancel(&AGAIN);
    assert!(cq.cq_pend.is_empty());
    assert_eq!(clockintr_dispatch(ptr::null_mut()), 0);
    assert_eq!(cq.cq_stat.get().cs_spurious, 1);

    clockintr_unbind(&ONCE, 0);
    clockintr_unbind(&AGAIN, 0);
    assert!(ONCE.cl_queue.get().is_null());
    assert_eq!(cq.cq_all.iter().count(), 0);
    crate::machine::intr::spl0();
}

/// A 15-bit timecounter (amd64's i8254 behind the LAPIC timer) whose count the test sets.
static STEP_COUNT: AtomicU32 = AtomicU32::new(0);

fn step_get(_tc: &Timecounter) -> u32 {
    STEP_COUNT.load(Ordering::Relaxed) & 0x7fff
}

static STEPTC: Timecounter = Timecounter::new(step_get, 0x7fff, 1_193_182, "steptc", i32::MAX, 0);

/// The clock interrupt was held off past the counter's period: the next reading is 0x100
/// counts behind the dispatch's `start`.
fn wrap_the_counter(_cr: &Clockrequest, _frame: *mut c_void, _arg: *mut c_void) {
    STEP_COUNT.fetch_add(0x8000 - 0x100, Ordering::Relaxed);
}

#[test]
fn dispatch_survives_a_counter_that_steps_back() {
    let _t = crate::kern::kern_tc::tests::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let ci = curcpu();
    let cq = Machine::ci_queue(ci);
    clockqueue_init(cq);
    clockqueue_intrclock_install(cq, &FAKE_INTRCLOCK);

    // The counter takes over at 0 and is read 0x4000 counts (13.7 ms) after that windup.
    STEP_COUNT.store(0, Ordering::Relaxed);
    tc_init(&STEPTC);
    tc_ticktock();
    STEP_COUNT.store(0x4000, Ordering::Relaxed);

    // WRAP runs now and wraps the counter; LATER keeps the dispatch looking at the clock again.
    static WRAP: Clockintr = Clockintr::new();
    static LATER: Clockintr = Clockintr::new();
    clockintr_bind(&WRAP, ci, wrap_the_counter, ptr::null_mut());
    clockintr_bind(&LATER, ci, count_and_reschedule, ptr::null_mut());
    clockintr_schedule(&WRAP, 0);
    clockintr_schedule(&LATER, u64::MAX / 2);

    // The other tests count from the statistics they find: give them back afterwards.
    let saved = cq.cq_stat.get();
    let dispatched = saved.cs_dispatched;
    crate::machine::intr::splraise(IPL_CLOCK);
    assert_eq!(clockintr_dispatch(ptr::null_mut()), 1);
    // The C's uint64_t sum takes the step back modulo 2^64: 0x100 counts short of 2^64.
    let back = 0u64.wrapping_sub(cq.cq_stat.get().cs_dispatched.wrapping_sub(dispatched));
    let counts_0x100 = (0x100u64 * 1_000_000_000) / 1_193_182;
    assert!(back.abs_diff(counts_0x100) <= 2, "{back} ns back");
    assert!(ptr::eq(cq.cq_pend.first().unwrap(), &LATER));

    clockintr_cancel(&LATER);
    clockintr_unbind(&WRAP, 0);
    clockintr_unbind(&LATER, 0);
    crate::machine::intr::spl0();
    cq.cq_stat.set(saved);
    // Back to the dummy counter.
    tc_reset_quality(&STEPTC, -1);
    tc_ticktock();
}

#[test]
fn stagger_sets_an_offset_without_scheduling() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let ci = curcpu();
    let cq = Machine::ci_queue(ci);
    clockqueue_init(cq);
    static CL: Clockintr = Clockintr::new();
    clockintr_bind(&CL, ci, count_and_reschedule, ptr::null_mut());
    clockintr_stagger(&CL, 10_000_000, 3, 4);
    assert_eq!(CL.cl_expiration.get(), 7_500_000);
    assert!(CL.cl_flags.get() & CLST_PENDING == 0);
    clockintr_unbind(&CL, 0);
}

#[test]
fn advance_random_stays_within_the_mask() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cq = Machine::ci_queue(curcpu());
    clockqueue_init(cq);
    let cr = &cq.cq_request;
    cq.cq_uptime.set(1_000_000);
    cr.cr_expiration.set(0);
    cr.cr_flags.set(0);
    let count = clockrequest_advance_random(cr, 100, 0xff);
    assert!(count >= 1);
    assert!(cr.cr_expiration.get() > 1_000_000);
    assert!(cr.cr_expiration.get() <= 1_000_000 + 100 + 0xff);
    assert!(cr.cr_flags.get() & CR_RESCHEDULE != 0);
    cr.cr_flags.set(0);
}
