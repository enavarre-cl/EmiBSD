//! Host tests for the clock interrupt queue, over the host double's `cpu_info` and the dummy
//! timecounter (which advances on every read).

use core::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Mutex as StdMutex;

use super::*;

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
