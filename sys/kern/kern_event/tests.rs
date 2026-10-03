//! Host tests for `kern_event.c`: knotes registered, activated through their klist, scanned
//! and deleted; `EVFILT_USER` triggers; `EVFILT_TIMER` driven by the timeout wheel; a
//! descriptor closed under a poll knote turning it into `EBADF`; the thread's poll kqueue.
//!
//! The tests build their thread and descriptor table by hand, as `kern_descrip`'s do: the
//! host's one CPU has no `curproc` to give them, so descriptors are placed with `fd_used`
//! and `fdinsert` instead of `falloc`, and kqueues are made as `dokqueue` makes them minus
//! the descriptor.

use core::cell::Cell;
use std::boxed::Box;
use std::sync::MutexGuard;
use std::{assert, assert_eq};

use super::*;
use crate::kern::kern_clock::TICKS;
use crate::kern::kern_descrip::{closef, fd_used, fdinit, fdremove, filedesc_init, fnew};
use crate::kern::kern_proc::procinit;
use crate::kern::kern_prot::{crget, crhold};
use crate::kern::kern_timeout::{softclock, timeout_hardclock_update, timeout_startup};
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::event::{EVFILT_TIMER, EVFILT_USER, NOTE_FFOR, ev_set};
use crate::sys::file::{DTYPE_PIPE, fref};

/// Real memory, the process, file and kqueue pools.
pub(crate) fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_real_memory();
    crate::machine::cons::consinit();
    procinit();
    filedesc_init();
    kqueue_init();
    guard
}

/// A thread of a fresh process with credentials and a new descriptor table.
pub(crate) fn thread() -> &'static Proc {
    let pr: &'static Process = Box::leak(Box::new(Process::new()));
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    p.p_p.set(pr);
    pr.ps_mainproc.set(p);
    let cr = crget();
    p.p_ucred.set(cr);
    pr.ps_ucred.set(crhold(cr));
    let fdp = fdinit();
    pr.ps_fd.set(fdp);
    p.p_fd.set(fdp);
    p
}

/// `dokqueue` without the descriptor: a kqueue on the thread's table.
pub(crate) fn new_kqueue(p: &Proc) -> &'static Kqueue {
    let fdp = p.fd();
    let kq = kqueue_alloc(fdp).unwrap();
    fdplock(fdp);
    // SAFETY: a new kqueue, on no list; `close_kqueue` takes it off.
    unsafe { fdp.fd_kqlist.insert_head(kq) };
    fdpunlock(fdp);
    kq
}

/// `kqueue_close` of a kqueue made by `new_kqueue`.
pub(crate) fn close_kqueue(p: &Proc, kq: &'static Kqueue) {
    kqueue_purge(Some(p), kq);
    kqueue_terminate(Some(p), kq);
    assert_eq!(refcnt_read(&kq.kq_refcnt), 1);
    // SAFETY: the test's reference, the last one.
    unsafe { KQRELE(kq) };
}

/// One non-blocking scan: the events, and the scan's error.
pub(crate) fn scan(p: &Proc, kq: &Kqueue, out: &mut [Kevent]) -> (usize, Result<(), Errno>) {
    let scan = KqueueScanState::new();
    // SAFETY: `scan` is a local that stays in place until `kqueue_scan_finish`.
    unsafe { kqueue_scan_setup(&scan, kq) };
    let mut ts = Timespec::new(0, 0);
    let mut error = Ok(());
    let n = out.len();
    let ready = kqueue_scan(&scan, n, out, Some(&mut ts), p, &mut error);
    kqueue_scan_finish(&scan);
    (ready, error)
}

/// Places `fp` (fresh from `fnew`) at descriptor `fd`, as `falloc` and `fdinsert` do.
pub(crate) fn install(p: &Proc, fd: i32, fp: &'static File) {
    let fdp = p.fd();
    fdplock(fdp);
    fd_used(fdp, fd);
    fdinsert(fdp, fd, 0, fp);
    fdpunlock(fdp);
}

/// `fdrelease` without `curproc`: off the table, its knotes removed, closed.
pub(crate) fn close_fd(p: &Proc, fd: i32) {
    let fdp = p.fd();
    fdplock(fdp);
    let fp = fdp.ofile(fd as usize).unwrap();
    fref(fp);
    fdremove(fdp, fd);
    knote_fdclose(p, fd);
    fdpunlock(fdp);
    closef(fp, p).unwrap();
}

/// The test object: a klist and whether its event is ready.
static TEST_KLIST: Klist = Klist::new();
static TEST_READY: TestFlag = TestFlag(Cell::new(false));

/// A flag the tests (serialised by `setup`) set and the filter reads.
struct TestFlag(Cell<bool>);
// SAFETY: the tests that touch it hold `setup`'s guard, one at a time.
unsafe impl Sync for TestFlag {}

fn filt_testdetach(kn: &Knote) {
    klist_remove(&TEST_KLIST, kn);
}

fn filt_test(kn: &Knote, _hint: i64) -> bool {
    kn.kn_data().set(i64::from(TEST_READY.0.get()));
    TEST_READY.0.get()
}

fn filt_testmodify(kev: &mut Kevent, kn: &Knote) -> bool {
    knote_modify(kev, kn)
}

fn filt_testprocess(kn: &Knote, kev: Option<&mut Kevent>) -> bool {
    knote_process(kn, kev)
}

static TEST_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: None,
    f_detach: Some(filt_testdetach),
    f_event: Some(filt_test),
    f_modify: Some(filt_testmodify),
    f_process: Some(filt_testprocess),
};

fn test_rw(_fp: &File, _uio: &mut Uio<'_>, _flags: i32) -> Result<(), Errno> {
    Ok(())
}

fn test_ioctl(_fp: &File, _com: u64, _data: &mut [u8], _p: &Proc) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

/// The test file's `fo_kqfilter`: `EVFILT_READ` hooks the knote on `TEST_KLIST`.
fn test_kqfilter(_fp: &File, kn: &Knote) -> Result<(), Errno> {
    if kn.kn_filter().get() != EVFILT_READ {
        return Err(Errno::EINVAL);
    }
    kn.kn_fop.set(Some(&TEST_FILTOPS));
    klist_insert(&TEST_KLIST, kn);
    Ok(())
}

fn test_stat(_fp: &File, _ub: &mut Stat, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

fn test_close(_fp: &File, _p: Option<&Proc>) -> Result<(), Errno> {
    Ok(())
}

static TESTOPS: Fileops = Fileops {
    fo_read: test_rw,
    fo_write: test_rw,
    fo_ioctl: test_ioctl,
    fo_kqfilter: test_kqfilter,
    fo_stat: test_stat,
    fo_close: test_close,
    fo_seek: None,
};

/// A test file at `fd`.
fn open_test_file(p: &Proc, fd: i32) -> &'static File {
    let fp = fnew(p).unwrap();
    fp.f_flag.store((FREAD | FWRITE) as u32, Ordering::SeqCst);
    fp.f_type.set(DTYPE_PIPE);
    fp.f_ops.set(Some(&TESTOPS));
    install(p, fd, fp);
    fp
}

#[test]
fn knote_attaches_activates_and_detaches() {
    let _g = setup();
    let p = thread();
    let kq = new_kqueue(p);
    let fp = open_test_file(p, 3);
    TEST_READY.0.set(false);
    let mut out = [Kevent::default(); 4];

    let mut kev = ev_set(3, EVFILT_READ, EV_ADD, 0, 0, 0x1234);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert!(!klist_empty(&TEST_KLIST), "fo_kqfilter hooked the knote");
    assert_eq!(kq.kq_nknotes.get(), 1);
    assert_eq!(
        fp.f_count.load(Ordering::SeqCst),
        2,
        "the knote holds the file"
    );
    assert_eq!(scan(p, kq, &mut out), (0, Ok(())), "not ready yet");

    // The object posts an event: the knote is queued, and stays (level-triggered).
    TEST_READY.0.set(true);
    knote(&TEST_KLIST, 0);
    assert_eq!(kq.kq_count.get(), 1);
    assert_eq!(scan(p, kq, &mut out), (1, Ok(())));
    assert_eq!((out[0].ident, out[0].filter), (3, EVFILT_READ));
    assert_eq!((out[0].data, out[0].udata), (1, 0x1234));
    assert_eq!(scan(p, kq, &mut out), (1, Ok(())), "still ready");

    // No longer ready: the scan drops it.
    TEST_READY.0.set(false);
    assert_eq!(scan(p, kq, &mut out), (0, Ok(())));
    assert_eq!(kq.kq_count.get(), 0);

    // EV_DISABLE keeps a posted event from being queued; EV_ENABLE rechecks.
    let mut kev = ev_set(3, EVFILT_READ, EV_DISABLE, 0, 0, 0);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    TEST_READY.0.set(true);
    knote(&TEST_KLIST, 0);
    assert_eq!(kq.kq_count.get(), 0);
    let mut kev = ev_set(3, EVFILT_READ, EV_ENABLE, 0, 0, 0);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert_eq!(scan(p, kq, &mut out), (1, Ok(())));

    // EV_DELETE detaches it from the object and drops the file reference.
    let mut kev = ev_set(3, EVFILT_READ, EV_DELETE, 0, 0, 0);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert!(klist_empty(&TEST_KLIST));
    assert_eq!(kq.kq_nknotes.get(), 0);
    assert_eq!(fp.f_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        kqueue_register(kq, &mut kev, 0, Some(p)),
        Err(Errno::ENOENT)
    );

    // Bad descriptors and filters.
    let mut kev = ev_set(9, EVFILT_READ, EV_ADD, 0, 0, 0);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Err(Errno::EBADF));
    let mut kev = ev_set(3, EVFILT_WRITE, EV_ADD, 0, 0, 0);
    assert_eq!(
        kqueue_register(kq, &mut kev, 0, Some(p)),
        Err(Errno::EINVAL)
    );
    let mut kev = ev_set(3, -11, EV_ADD, 0, 0, 0);
    assert_eq!(
        kqueue_register(kq, &mut kev, 0, Some(p)),
        Err(Errno::EINVAL)
    );
    assert_eq!(kq.kq_nknotes.get(), 0);

    close_fd(p, 3);
    close_kqueue(p, kq);
}

#[test]
fn oneshot_and_clear_post_event_actions() {
    let _g = setup();
    let p = thread();
    let kq = new_kqueue(p);
    open_test_file(p, 3);
    TEST_READY.0.set(true);
    let mut out = [Kevent::default(); 2];

    // EV_ONESHOT: reported once, then dropped.
    let mut kev = ev_set(3, EVFILT_READ, EV_ADD | EV_ONESHOT, 0, 0, 0);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert_eq!(scan(p, kq, &mut out), (1, Ok(())));
    assert_eq!(kq.kq_nknotes.get(), 0);
    assert!(klist_empty(&TEST_KLIST));
    assert_eq!(scan(p, kq, &mut out), (0, Ok(())));

    // EV_DISPATCH: reported once, then disabled until re-enabled.
    let mut kev = ev_set(3, EVFILT_READ, EV_ADD | EV_DISPATCH, 0, 0, 0);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert_eq!(scan(p, kq, &mut out), (1, Ok(())));
    knote(&TEST_KLIST, 0);
    assert_eq!(scan(p, kq, &mut out), (0, Ok(())));
    let mut kev = ev_set(3, EVFILT_READ, EV_ENABLE, 0, 0, 0);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert_eq!(scan(p, kq, &mut out), (1, Ok(())));

    close_fd(p, 3);
    assert!(klist_empty(&TEST_KLIST), "knote_fdclose detached it");
    assert_eq!(kq.kq_nknotes.get(), 0);
    close_kqueue(p, kq);
}

#[test]
fn user_events_trigger_and_combine_fflags() {
    let _g = setup();
    let p = thread();
    let kq = new_kqueue(p);
    let mut out = [Kevent::default(); 2];

    let mut kev = ev_set(1, EVFILT_USER, EV_ADD | EV_CLEAR, 0x0f, 7, 0);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert_eq!(p.fd().fd_nuserevents.load(Ordering::SeqCst), 1);
    assert_eq!(scan(p, kq, &mut out), (0, Ok(())), "not triggered");

    // NOTE_TRIGGER without EV_ADD goes through f_modify; NOTE_FFOR merges the flags.
    let mut kev = ev_set(1, EVFILT_USER, 0, NOTE_TRIGGER | NOTE_FFOR | 0x30, 9, 0);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert_eq!(scan(p, kq, &mut out), (1, Ok(())));
    assert_eq!((out[0].ident, out[0].filter), (1, EVFILT_USER));
    assert_eq!((out[0].fflags, out[0].data), (0x3f, 9));
    assert_eq!(
        scan(p, kq, &mut out),
        (0, Ok(())),
        "EV_CLEAR resets the trigger"
    );

    let mut kev = ev_set(1, EVFILT_USER, EV_DELETE, 0, 0, 0);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert_eq!(p.fd().fd_nuserevents.load(Ordering::SeqCst), 0);
    close_kqueue(p, kq);
}

/// One hardclock's worth of wheel work, as the timeout tests do.
fn tick_once() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    timeout_hardclock_update();
    softclock(ptr::null_mut());
}

#[test]
fn timers_fire_through_the_timeout_wheel() {
    let _g = setup();
    let _t = crate::kern::kern_timeout::tests::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    timeout_startup();
    let p = thread();
    let kq = new_kqueue(p);
    let mut out = [Kevent::default(); 2];

    // A one-shot 20 ms timer fires once and is gone.
    let mut kev = ev_set(5, EVFILT_TIMER, EV_ADD | EV_ONESHOT, 0, 20, 0);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert_eq!(scan(p, kq, &mut out), (0, Ok(())));
    let mut ticks = 0;
    while kq.kq_count.get() == 0 && ticks < 100 {
        tick_once();
        ticks += 1;
    }
    assert!(ticks < 100, "the timer fired");
    assert_eq!(scan(p, kq, &mut out), (1, Ok(())));
    assert_eq!(
        (out[0].ident, out[0].filter, out[0].data),
        (5, EVFILT_TIMER, 1)
    );
    assert_eq!(kq.kq_nknotes.get(), 0, "EV_ONESHOT dropped it");
    assert_eq!(p.fd().fd_nuserevents.load(Ordering::SeqCst), 0);

    // A periodic timer counts its expirations between scans (EV_CLEAR is implied).
    let mut kev = ev_set(6, EVFILT_TIMER, EV_ADD, 0, 10, 0);
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    for _ in 0..50 {
        tick_once();
    }
    assert_eq!(scan(p, kq, &mut out), (1, Ok(())));
    assert!(out[0].data > 1, "several expirations: {}", out[0].data);
    assert_eq!(out[0].flags & EV_CLEAR, EV_CLEAR);
    assert_eq!(scan(p, kq, &mut out), (0, Ok(())), "cleared");

    // A bad unit is refused at registration.
    let mut kev = ev_set(7, EVFILT_TIMER, EV_ADD, 0x100, 10, 0);
    assert_eq!(
        kqueue_register(kq, &mut kev, 0, Some(p)),
        Err(Errno::EINVAL)
    );

    close_kqueue(p, kq);
    assert_eq!(p.fd().fd_nuserevents.load(Ordering::SeqCst), 0);
}

#[test]
fn closing_a_polled_descriptor_reports_ebadf() {
    let _g = setup();
    let p = thread();
    open_test_file(p, 3);
    TEST_READY.0.set(false);
    let mut out = [Kevent::default(); 2];

    // The thread's poll kqueue, as poll(2) registers on it.
    assert_eq!(kqpoll_init(p, 1), Ok(()));
    let kq = p.kq();
    let serial = p.p_kq_serial.get();
    let mut kev = ev_set(
        3,
        EVFILT_READ,
        EV_ADD | EV_ENABLE | __EV_POLL,
        0,
        0,
        serial as usize,
    );
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert_eq!(scan(p, kq, &mut out), (0, Ok(())));

    // knote_fdclose rewires the knote to badfd_filtops instead of dropping it.
    close_fd(p, 3);
    assert!(klist_empty(&TEST_KLIST));
    assert_eq!(scan(p, kq, &mut out), (1, Ok(())));
    assert_eq!(out[0].flags & EV_ERROR, EV_ERROR);
    assert_eq!(out[0].data, i64::from(Errno::EBADF as i32));
    assert_eq!(kq.kq_nknotes.get(), 0, "EV_ONESHOT dropped it");
    kqpoll_done(p, 1);
    assert_eq!(p.p_kq_serial.get(), serial + 1);

    // A knote of an older serial is dropped by the scan, unreported.
    open_test_file(p, 3);
    TEST_READY.0.set(true);
    let mut kev = ev_set(
        3,
        EVFILT_READ,
        EV_ADD | EV_ENABLE | __EV_POLL,
        0,
        0,
        serial as usize,
    );
    assert_eq!(kqueue_register(kq, &mut kev, 0, Some(p)), Ok(()));
    assert_eq!(kq.kq_count.get(), 1);
    assert_eq!(scan(p, kq, &mut out), (0, Ok(())));
    assert_eq!(kq.kq_nknotes.get(), 0);

    close_fd(p, 3);
    kqpoll_exit(p);
    assert!(p.p_kq.get().is_null());
}

#[test]
fn timer_units_validate() {
    assert_eq!(filt_timervalidate(NOTE_SECONDS, 3), Ok(Timespec::new(3, 0)));
    assert_eq!(
        filt_timervalidate(NOTE_MSECONDS, 1500),
        Ok(Timespec::new(1, 500_000_000))
    );
    assert_eq!(
        filt_timervalidate(NOTE_USECONDS, 2_000_001),
        Ok(Timespec::new(2, 1000))
    );
    assert_eq!(
        filt_timervalidate(NOTE_NSECONDS, 7),
        Ok(Timespec::new(0, 7))
    );
    assert_eq!(
        filt_timervalidate(NOTE_SECONDS | NOTE_ABSTIME, 1),
        Ok(Timespec::new(1, 0))
    );
    assert_eq!(filt_timervalidate(0x20, 1), Err(Errno::EINVAL));
    assert_eq!(kn_hash(0x1234, 63), (0x1234 ^ 0x12) & 63);
}
