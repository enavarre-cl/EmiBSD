//! Host tests for the advisory record locks: the splitting, merging, upgrading and
//! downgrading of one owner's ranges, the conflicts between owners, `F_GETLK`, the range
//! conversion of `lf_advlock` and `lf_purgelocks`.

use std::vec::Vec;
use std::{assert, assert_eq};

use super::*;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::sys::proc::Proc;

/// The test's thread as `curproc`, the pools initialised, an empty lock slot.
fn setup() -> (std::sync::MutexGuard<'static, ()>, &'static Proc) {
    let (g, p) = crate::kern::vfs_subr::tests::setup();
    lf_init();
    Machine::set_curproc(Machine::curcpu(), p);
    (g, p)
}

/// Clears `curproc` again, so later tests see the boot state.
fn teardown() {
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

const A: *const c_void = 0x1000 as *const c_void;
const B: *const c_void = 0x2000 as *const c_void;

/// A `struct flock` for `[start, start + len)` from `SEEK_SET`.
fn fl(type_: i16, start: Off, len: Off) -> Flock {
    Flock {
        l_start: start,
        l_len: len,
        l_pid: 0,
        l_type: type_,
        l_whence: SEEK_SET as i16,
    }
}

/// `F_SETLK` of `[start, start + len)` by `id`, without waiting.
fn setlk(
    slot: &LockfStateSlot,
    id: *const c_void,
    type_: i16,
    start: Off,
    len: Off,
) -> Result<(), Errno> {
    lf_advlock(slot, 0, id, F_SETLK, &mut fl(type_, start, len), F_POSIX)
}

/// `F_UNLCK` of `[start, start + len)` by `id`.
fn unlk(slot: &LockfStateSlot, id: *const c_void, start: Off, len: Off) -> Result<(), Errno> {
    lf_advlock(
        slot,
        0,
        id,
        i32::from(F_UNLCK),
        &mut fl(F_UNLCK, start, len),
        F_POSIX,
    )
}

/// The locks of the slot, in list order: `(owner, type, start, end)`.
fn locks(slot: &LockfStateSlot) -> Vec<(*const c_void, i16, Off, Off)> {
    slot.get().map_or_else(Vec::new, |ls| {
        ls.ls_locks
            .iter()
            .map(|l| {
                (
                    l.lf_id.get(),
                    l.lf_type.get(),
                    l.lf_start.get(),
                    l.lf_end.get(),
                )
            })
            .collect()
    })
}

#[test]
fn unlocking_the_middle_splits_a_lock_in_three_pieces_minus_one() {
    let (_g, _p) = setup();
    let slot = LockfStateSlot::new(None);
    setlk(&slot, A, F_WRLCK, 0, 100).unwrap();
    assert_eq!(locks(&slot), [(A, F_WRLCK, 0, 99)]);

    unlk(&slot, A, 10, 10).unwrap();
    assert_eq!(locks(&slot), [(A, F_WRLCK, 0, 9), (A, F_WRLCK, 20, 99)]);

    // Unlocking the front and the back shrinks the pieces (cases 2 and 4/5).
    unlk(&slot, A, 0, 5).unwrap();
    unlk(&slot, A, 90, 0).unwrap();
    assert_eq!(locks(&slot), [(A, F_WRLCK, 5, 9), (A, F_WRLCK, 20, 89)]);

    // Unlocking everything frees the state and clears the slot.
    unlk(&slot, A, 0, 0).unwrap();
    assert!(slot.get().is_none());
    teardown();
}

#[test]
fn a_different_type_in_the_middle_splits_the_owners_lock() {
    let (_g, _p) = setup();
    let slot = LockfStateSlot::new(None);
    setlk(&slot, A, F_RDLCK, 0, 100).unwrap();
    setlk(&slot, A, F_WRLCK, 40, 20).unwrap();
    assert_eq!(
        locks(&slot),
        [
            (A, F_RDLCK, 0, 39),
            (A, F_WRLCK, 40, 59),
            (A, F_RDLCK, 60, 99)
        ]
    );

    // The same type inside an existing lock changes nothing (case 2).
    setlk(&slot, A, F_RDLCK, 70, 5).unwrap();
    assert_eq!(locks(&slot).len(), 3);

    // A lock covering all of them replaces each of them (case 3).
    setlk(&slot, A, F_WRLCK, 0, 100).unwrap();
    assert_eq!(locks(&slot), [(A, F_WRLCK, 0, 99)]);

    // Downgrading the exact range keeps one lock (case 1).
    setlk(&slot, A, F_RDLCK, 0, 100).unwrap();
    assert_eq!(locks(&slot), [(A, F_RDLCK, 0, 99)]);
    unlk(&slot, A, 0, 0).unwrap();
    teardown();
}

#[test]
fn overlapping_ends_trim_the_older_lock() {
    let (_g, _p) = setup();
    let slot = LockfStateSlot::new(None);
    setlk(&slot, A, F_RDLCK, 0, 50).unwrap();
    // Starts inside, ends after: the old one keeps its front (case 4).
    setlk(&slot, A, F_WRLCK, 25, 50).unwrap();
    assert_eq!(locks(&slot), [(A, F_RDLCK, 0, 24), (A, F_WRLCK, 25, 74)]);
    // Starts before, ends inside: the old one keeps its back (case 5).
    setlk(&slot, A, F_RDLCK, 70, 30).unwrap();
    setlk(&slot, A, F_WRLCK, 60, 20).unwrap();
    assert_eq!(
        locks(&slot),
        [
            (A, F_RDLCK, 0, 24),
            (A, F_WRLCK, 25, 59),
            (A, F_WRLCK, 60, 79),
            (A, F_RDLCK, 80, 99)
        ]
    );
    unlk(&slot, A, 0, 0).unwrap();
    teardown();
}

#[test]
fn other_owners_conflict_on_writes_and_getlk_reports_the_blocker() {
    let (_g, p) = setup();
    let slot = LockfStateSlot::new(None);
    setlk(&slot, A, F_RDLCK, 0, 10).unwrap();
    setlk(&slot, A, F_WRLCK, 20, 10).unwrap();

    // Readers share; a writer over a reader or a reader over a writer must wait.
    setlk(&slot, B, F_RDLCK, 5, 10).unwrap();
    assert_eq!(setlk(&slot, B, F_WRLCK, 0, 3), Err(Errno::EAGAIN));
    assert_eq!(setlk(&slot, B, F_RDLCK, 25, 1), Err(Errno::EAGAIN));
    // Next to the locks there is no conflict.
    setlk(&slot, B, F_WRLCK, 30, 0).unwrap();

    // F_GETLK finds the first lock of another owner that blocks the request.
    let mut q = fl(F_WRLCK, 0, 0);
    lf_advlock(&slot, 0, B, F_GETLK, &mut q, F_POSIX).unwrap();
    assert_eq!((q.l_type, q.l_start, q.l_len), (F_RDLCK, 0, 10));
    assert_eq!(q.l_pid, p.process().ps_pid.get());
    let mut q = fl(F_RDLCK, 0, 15);
    lf_advlock(&slot, 0, B, F_GETLK, &mut q, F_POSIX).unwrap();
    assert_eq!(q.l_type, F_UNLCK);

    // Each owner's locks are sorted by start; another owner's do not place a new lock, so
    // they end up after the first owner's.
    assert_eq!(
        locks(&slot),
        [
            (A, F_RDLCK, 0, 9),
            (A, F_WRLCK, 20, 29),
            (B, F_RDLCK, 5, 14),
            (B, F_WRLCK, 30, -1)
        ]
    );
    unlk(&slot, A, 0, 0).unwrap();
    unlk(&slot, B, 0, 0).unwrap();
    assert!(slot.get().is_none());
    teardown();
}

#[test]
fn ranges_follow_whence_and_negative_lengths() {
    let (_g, _p) = setup();
    let slot = LockfStateSlot::new(None);
    // SEEK_END counts from the size; a negative length reaches backwards.
    let mut f = fl(F_WRLCK, -10, 5);
    f.l_whence = SEEK_END as i16;
    lf_advlock(&slot, 1000, A, F_SETLK, &mut f, F_POSIX).unwrap();
    lf_advlock(&slot, 0, A, F_SETLK, &mut fl(F_WRLCK, 100, -10), F_POSIX).unwrap();
    assert_eq!(locks(&slot), [(A, F_WRLCK, 90, 99), (A, F_WRLCK, 990, 994)]);

    assert_eq!(setlk(&slot, A, F_WRLCK, -1, 1), Err(Errno::EINVAL));
    assert_eq!(setlk(&slot, A, F_WRLCK, 5, -10), Err(Errno::EINVAL));
    assert_eq!(
        setlk(&slot, A, F_WRLCK, 10, Off::MAX),
        Err(Errno::EOVERFLOW)
    );
    let mut f = fl(F_WRLCK, 0, 1);
    f.l_whence = 7;
    assert_eq!(
        lf_advlock(&slot, 0, A, F_SETLK, &mut f, F_POSIX),
        Err(Errno::EINVAL)
    );

    // Unlocking or asking with no state answers without allocating one.
    let empty = LockfStateSlot::new(None);
    let mut q = fl(F_WRLCK, 0, 0);
    lf_advlock(&empty, 0, A, F_GETLK, &mut q, F_POSIX).unwrap();
    assert_eq!(q.l_type, F_UNLCK);
    assert!(empty.get().is_none());

    lf_purgelocks(&slot);
    assert!(slot.get().is_none());
    teardown();
}

#[test]
fn locks_are_charged_to_the_owners_uid() {
    let (_g, p) = setup();
    let uid = p.ucred().cr_uid.get();
    let count = || {
        let uip = uid_find(uid);
        let n = uip.ui_lockcnt.get();
        uid_release(uip);
        n
    };
    let before = count();
    let slot = LockfStateSlot::new(None);
    setlk(&slot, A, F_WRLCK, 0, 100).unwrap();
    unlk(&slot, A, 10, 10).unwrap(); // a split allocates a second lock
    assert_eq!(count(), before + 2);
    lf_purgelocks(&slot);
    assert_eq!(count(), before);
    teardown();
}
