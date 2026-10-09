/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Scooter Morris at Genentech Inc.
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
 *	@(#)ufs_lockf.c	8.3 (Berkeley) 1/6/94
 */
/* </LICENSES> */

/* <CODE> */
//! Advisory record locking (`fcntl(2)`'s `F_SETLK`/`F_GETLK` and `flock(2)`): the byte-range
//! locks of one file, kept sorted by start in a `struct lockf_state` the file's vnode (or
//! inode) points at, with the blocked requests queued on the lock that blocks them.
//!
//! Upstream: sys/kern/vfs_lockf.c @ 3ce1f3f79392
//!
//! A `struct lockf` and a `struct lockf_state` are pool items, so they travel as
//! `&'static Lockf`/`&'static LockfState`, with `Cell` members guarded by `lockf_lock`, as
//! `struct mbuf` and `struct file` do; `lf_free` and `ls_rele` hand the item back to its pool,
//! after which the caller does not touch it, as in C.
//!
//! ## Deviations
//! - `struct lockf_state **state` is `&LockfStateSlot` (a `Cell<Option<&'static
//!   LockfState>>`) and `ls_owner` keeps it as a raw pointer to the slot, which lives in the
//!   vnode's `specinfo` (or a file system's inode) until `lf_purgelocks` empties it.
//! - `lf_findoverlap` returns the case and the overlap as a pair instead of filling
//!   `**overlap`; `lf_getblock` returns `Option`, `lf_deadlock` a `bool`.
//! - `lf_flags` is an `i32` (the C's `short`) since the `F_*` flags it holds are `i32`s.
//! - `pool_get(PR_WAITOK)` cannot sleep yet (`subr_pool.rs`): when it fails `lf_alloc`
//!   answers NULL as for the per-uid limit, so `lf_advlock` fails with `ENOLCK`, and
//!   `lf_split`, whose allocation the C never fails, panics.
//! - `LOCKF_DEBUG` is not configured: `lf_print`, `lf_printlist` and the `DPRINTF`/`LFPRINT`
//!   traces are compiled out, as in C.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kassert;
use crate::kern::kern_proc::{uid_find, uid_release};
use crate::kern::kern_rwlock::{rw_assert_wrlock, rw_enter_write, rw_exit_write};
use crate::kern::kern_synch::{rwsleep_nsec, wakeup_one};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::machine::cpu::curproc;
use crate::machine::intr::IPL_NONE;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::fcntl::{
    F_FLOCK, F_GETLK, F_INTR, F_POSIX, F_RDLCK, F_SETLK, F_UNLCK, F_WAIT, F_WRLCK, Flock,
};
use crate::sys::param::{PCATCH, PLOCK};
use crate::sys::pool::{PR_RWLOCK, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::systm::INFSLP;
use crate::sys::types::{Off, Pid, Uid};
use crate::sys::unistd::{SEEK_CUR, SEEK_END, SEEK_SET};

/// `SELF`: `lf_findoverlap` looks at the locks of the same owner.
const SELF: i32 = 0x1;
/// `OTHERS`: `lf_findoverlap` looks at the locks of other owners.
const OTHERS: i32 = 0x2;

/// `struct lockf`: a kernel structure which contains the information associated with a byte
/// range lock. The lockf structures are linked into the inode structure. Locks are sorted by
/// the starting byte of the lock for efficiency.
///
/// Protected by: `lockf_lock`.
pub struct Lockf {
    /// `lf_flags`: lock semantics: `F_POSIX`, `F_FLOCK`, `F_WAIT`.
    pub lf_flags: Cell<i32>,
    /// `lf_type`: lock type: `F_RDLCK`, `F_WRLCK`.
    pub lf_type: Cell<i16>,
    /// `lf_start`: the byte # of the start of the lock.
    pub lf_start: Cell<Off>,
    /// `lf_end`: the byte # of the end of the lock (-1=EOF).
    pub lf_end: Cell<Off>,
    /// `lf_id`: the id of the resource holding the lock.
    pub lf_id: Cell<*const c_void>,
    /// `lf_state`: state associated with the lock.
    pub lf_state: Cell<Option<&'static LockfState>>,
    /// `lf_entry`: the link in `ls_locks` or `ls_pending`.
    pub lf_entry: TailqEntry<Lockf>,
    /// `lf_blk`: the lock that blocks us.
    pub lf_blk: Cell<Option<&'static Lockf>>,
    /// `lf_blkhd`: the list of blocked locks.
    pub lf_blkhd: TailqHead<LfBlock>,
    /// `lf_block`: a request waiting for a lock.
    pub lf_block: TailqEntry<Lockf>,
    /// `lf_uid`: user ID responsible.
    pub lf_uid: Cell<Uid>,
    /// `lf_pid`: POSIX - owner pid.
    pub lf_pid: Cell<Pid>,
}

// SAFETY: every member is changed under `lockf_lock`, as in C.
unsafe impl Sync for Lockf {}

impl Lockf {
    /// A lock with every member clear, which `lf_alloc` and `lf_advlock` fill in.
    const fn new() -> Self {
        Self {
            lf_flags: Cell::new(0),
            lf_type: Cell::new(0),
            lf_start: Cell::new(0),
            lf_end: Cell::new(0),
            lf_id: Cell::new(ptr::null()),
            lf_state: Cell::new(None),
            lf_entry: TailqEntry::new(),
            lf_blk: Cell::new(None),
            lf_blkhd: TailqHead::new(),
            lf_block: TailqEntry::new(),
            lf_uid: Cell::new(0),
            lf_pid: Cell::new(0),
        }
    }

    /// `lock->lf_state`, which every lock out of `lf_advlock` or `lf_split` has.
    fn state(&self) -> &'static LockfState {
        match self.lf_state.get() {
            Some(ls) => ls,
            None => panic(format_args!("lockf {:p} without a state", self)),
        }
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(locklist, lockf)` through `lf_entry`: `ls_locks` and `ls_pending`.
    pub LfEntry: Lockf, lf_entry => TailqEntry<Lockf>
);

queue_adapter!(
    /// `TAILQ_HEAD(locklist, lockf)` through `lf_block`: a lock's `lf_blkhd`.
    pub LfBlock: Lockf, lf_block => TailqEntry<Lockf>
);

/// `struct lockf_state *`: the slot a vnode (`v_speclockf`) or an inode (`i_lockf`) keeps.
pub type LockfStateSlot = Cell<Option<&'static LockfState>>;

/// `struct lockf_state`: the locks of one file.
///
/// Protected by: `lockf_lock`.
pub struct LockfState {
    /// `ls_locks`: list of active locks.
    pub ls_locks: TailqHead<LfEntry>,
    /// `ls_pending`: list of pending locks.
    pub ls_pending: TailqHead<LfEntry>,
    /// `ls_owner`: owner (the slot that points back at us).
    pub ls_owner: Cell<*const LockfStateSlot>,
    /// `ls_refs`: reference counter.
    pub ls_refs: Cell<i32>,
}

// SAFETY: every member is changed under `lockf_lock`, as in C.
unsafe impl Sync for LockfState {}

impl LockfState {
    /// A zeroed state, as `pool_get(PR_ZERO)` returns it.
    const fn new() -> Self {
        Self {
            ls_locks: TailqHead::new(),
            ls_pending: TailqHead::new(),
            ls_owner: Cell::new(ptr::null()),
            ls_refs: Cell::new(0),
        }
    }
}

/// `lockf_state_pool`.
pub static LOCKF_STATE_POOL: Pool = Pool::new();
/// `lockf_pool`.
pub static LOCKF_POOL: Pool = Pool::new();

/// `lockf_lock`: serializes access to each instance of `struct lockf` and `struct
/// lockf_state` and each pointer from a vnode to `struct lockf_state`.
pub static LOCKF_LOCK: Rwlock = Rwlock::new("lockflk");

/// `maxlocksperuid`: we enforce a limit on locks by uid, so that a single user cannot run the
/// kernel out of memory. For now, the limit is pretty coarse. There is no limit on root.
///
/// Splitting a lock will always succeed, regardless of current allocations. If you're
/// slightly above the limit, we still have to permit an allocation so that the unlock can
/// succeed. If the unlocking causes too many splits, however, you're totally cutoff.
pub static MAXLOCKSPERUID: AtomicI32 = AtomicI32::new(1024);

/// `lf_init`: initialise the pools.
pub fn lf_init() {
    pool_init(
        &LOCKF_STATE_POOL,
        size_of::<LockfState>(),
        0,
        IPL_NONE,
        PR_WAITOK | PR_RWLOCK,
        "lockfspl",
        None,
    );
    pool_init(
        &LOCKF_POOL,
        size_of::<Lockf>(),
        0,
        IPL_NONE,
        PR_WAITOK | PR_RWLOCK,
        "lockfpl",
        None,
    );
}

/// `ls_ref`: another reference to a lock state.
fn ls_ref(ls: &LockfState) {
    rw_assert_wrlock(&LOCKF_LOCK);

    ls.ls_refs.set(ls.ls_refs.get() + 1);
}

/// `ls_rele`: drops a reference; the last one clears the owner's slot and frees the state.
fn ls_rele(ls: &'static LockfState) {
    rw_assert_wrlock(&LOCKF_LOCK);

    ls.ls_refs.set(ls.ls_refs.get() - 1);
    if ls.ls_refs.get() > 0 {
        return;
    }

    kassert!(ls.ls_locks.is_empty());
    kassert!(ls.ls_pending.is_empty());

    // SAFETY: `ls_owner` is the slot `lf_advlock` was given, which outlives its state (the
    // vnode or inode purges its locks before it goes away).
    unsafe { (*ls.ls_owner.get()).set(None) };
    pool_put(&LOCKF_STATE_POOL, NonNull::from(ls).cast());
}

/// `lf_alloc`: a lock charged to `uid`. 3 options for allowfail: 0 - always allocate. 1 -
/// cutoff at limit. 2 - cutoff at double limit.
fn lf_alloc(uid: Uid, allowfail: i32) -> Option<&'static Lockf> {
    let uip = uid_find(uid);
    let max = i64::from(MAXLOCKSPERUID.load(Ordering::Relaxed));
    if uid != 0
        && allowfail != 0
        && uip.ui_lockcnt.get() > if allowfail == 1 { max } else { max * 2 }
    {
        uid_release(uip);
        return None;
    }
    uip.ui_lockcnt.set(uip.ui_lockcnt.get() + 1);
    uid_release(uip);
    let Some(mem) = pool_get(&LOCKF_POOL, PR_WAITOK) else {
        // PR_WAITOK cannot sleep yet (see the module's deviations): give the charge back.
        let uip = uid_find(uid);
        uip.ui_lockcnt.set(uip.ui_lockcnt.get() - 1);
        uid_release(uip);
        return None;
    };
    let lock = mem.cast::<Lockf>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Lockf>()` bytes, written once
    // before anything else sees it.
    unsafe { lock.as_ptr().write(Lockf::new()) };
    // SAFETY: as above; the item stays allocated until `lf_free` gives it back.
    let lock: &'static Lockf = unsafe { lock.as_ref() };
    lock.lf_blkhd.init();
    lock.lf_uid.set(uid);
    Some(lock)
}

/// `lf_free`: uncharges and frees a lock, dropping its reference on the state.
fn lf_free(lock: &'static Lockf) {
    rw_assert_wrlock(&LOCKF_LOCK);

    kassert!(lock.lf_blkhd.is_empty());

    ls_rele(lock.state());

    let uip = uid_find(lock.lf_uid.get());
    uip.ui_lockcnt.set(uip.ui_lockcnt.get() - 1);
    uid_release(uip);
    pool_put(&LOCKF_POOL, NonNull::from(lock).cast());
}

/// Do an advisory lock operation.
pub fn lf_advlock(
    state: &LockfStateSlot,
    size: Off,
    id: *const c_void,
    op: i32,
    fl: &mut Flock,
    flags: i32,
) -> Result<(), Errno> {
    let Some(p) = curproc() else {
        panic(format_args!("lf_advlock: no curproc"));
    };

    // Convert the flock structure into a start and end.
    let mut start: Off = match i32::from(fl.l_whence) {
        // Caller is responsible for adding any necessary offset when SEEK_CUR is used.
        SEEK_SET | SEEK_CUR => fl.l_start,
        SEEK_END => size.wrapping_add(fl.l_start),
        _ => return Err(Errno::EINVAL),
    };
    if start < 0 {
        return Err(Errno::EINVAL);
    }
    let end: Off = if fl.l_len > 0 {
        if fl.l_len - 1 > Off::MAX - start {
            return Err(Errno::EOVERFLOW);
        }
        let end = start + (fl.l_len - 1);
        // Avoid ambiguity at the end of the range.
        if end == Off::MAX { -1 } else { end }
    } else if fl.l_len < 0 {
        if start.wrapping_add(fl.l_len) < 0 {
            return Err(Errno::EINVAL);
        }
        let end = start - 1;
        start += fl.l_len;
        end
    } else {
        -1
    };

    rw_enter_write(&LOCKF_LOCK);
    let error = 'out: {
        // Avoid the common case of unlocking when inode has no locks.
        let ls = match state.get() {
            None if op != F_SETLK => {
                fl.l_type = F_UNLCK;
                break 'out Ok(());
            }
            Some(ls) => ls,
            None => {
                let Some(mem) = pool_get(&LOCKF_STATE_POOL, PR_WAITOK | PR_ZERO) else {
                    // PR_WAITOK cannot sleep yet (see the module's deviations).
                    break 'out Err(Errno::ENOLCK);
                };
                let ls = mem.cast::<LockfState>();
                // SAFETY: a fresh, suitably aligned pool item of `size_of::<LockfState>()`
                // bytes, written once before anything else sees it.
                unsafe { ls.as_ptr().write(LockfState::new()) };
                // SAFETY: as above; the item stays allocated until `ls_rele` frees it.
                let ls: &'static LockfState = unsafe { ls.as_ref() };
                ls.ls_owner.set(state);
                ls.ls_locks.init();
                ls.ls_pending.init();
                state.set(Some(ls));
                ls
            }
        };
        ls_ref(ls);

        let allowfail = if op == F_SETLK { 1 } else { 2 };
        let Some(lock) = lf_alloc(p.ucred().cr_uid.get(), allowfail) else {
            ls_rele(ls);
            break 'out Err(Errno::ENOLCK);
        };
        lock.lf_flags.set(flags);
        lock.lf_type.set(fl.l_type);
        lock.lf_start.set(start);
        lock.lf_end.set(end);
        lock.lf_id.set(id);
        lock.lf_state.set(Some(ls));
        lock.lf_blk.set(None);
        lock.lf_pid.set(if flags & F_POSIX != 0 {
            p.process().ps_pid.get()
        } else {
            -1
        });

        match op {
            F_SETLK => lf_setlock(lock),
            _ if op == i32::from(F_UNLCK) => {
                let error = lf_clearlock(lock);
                lf_free(lock);
                error
            }
            F_GETLK => {
                let error = lf_getlock(lock, fl);
                lf_free(lock);
                error
            }
            _ => {
                lf_free(lock);
                Err(Errno::EINVAL)
            }
        }
    };

    rw_exit_write(&LOCKF_LOCK);
    error
}

/// Set a byte-range lock.
fn lf_setlock(lock: &'static Lockf) -> Result<(), Errno> {
    rw_assert_wrlock(&LOCKF_LOCK);

    let mut priority = PLOCK;
    if lock.lf_type.get() == F_WRLCK {
        priority += 4;
    }
    priority |= PCATCH;
    let ls = lock.state();
    // Scan lock list for this file looking for locks that would block us.
    while let Some(block) = lf_getblock(ls.ls_locks.first(), lock) {
        if lock.lf_flags.get() & F_WAIT == 0 {
            lf_free(lock);
            return Err(Errno::EAGAIN);
        }

        // Lock is blocked, check for deadlock before proceeding. Note: flock style locks
        // cover the whole file, there is no chance for deadlock.
        if lock.lf_flags.get() & F_POSIX != 0 && lf_deadlock(lock) {
            lf_free(lock);
            return Err(Errno::EDEADLK);
        }

        // For flock type locks, we must first remove any shared locks that we hold before we
        // sleep waiting for an exclusive lock.
        if lock.lf_flags.get() & F_FLOCK != 0 && lock.lf_type.get() == F_WRLCK {
            lock.lf_type.set(F_UNLCK);
            let _ = lf_clearlock(lock);
            lock.lf_type.set(F_WRLCK);
        }
        // Add our lock to the blocked list and sleep until we're free. Remember who blocked
        // us (for deadlock detection).
        lock.lf_blk.set(Some(block));
        // SAFETY: a lock being set is on no list; it stays in its pool item until `lf_free`,
        // which happens only after it left both lists again.
        unsafe {
            block.lf_blkhd.insert_tail(lock);
            ls.ls_pending.insert_tail(lock);
        }
        let error = rwsleep_nsec(ptr::from_ref(lock), &LOCKF_LOCK, priority, "lockf", INFSLP);
        // SAFETY: the lock was put on `ls_pending` above and only this thread removes it.
        unsafe { ls.ls_pending.remove(lock) };
        wakeup_one(ptr::from_ref(ls));
        if let Some(blk) = lock.lf_blk.get() {
            // SAFETY: `lf_blk` is set exactly while the lock is on that lock's `lf_blkhd`
            // (`lf_wakelock` clears both together).
            unsafe { blk.lf_blkhd.remove(lock) };
            lock.lf_blk.set(None);
        }
        if let Err(error) = error {
            lf_free(lock);
            return Err(error);
        }
        if lock.lf_flags.get() & F_INTR != 0 {
            lf_free(lock);
            return Err(Errno::EINTR);
        }
    }
    // No blocks!! Add the lock. Note that we will downgrade or upgrade any overlapping locks
    // this process already owns.
    //
    // Skip over locks owned by other processes. Handle any locks that overlap and are owned by
    // ourselves.
    let mut block = ls.ls_locks.first();
    let mut needtolink = true;
    loop {
        let (ovcase, overlap) = lf_findoverlap(block, lock, SELF);
        if ovcase != 0
            && let Some(ov) = overlap
        {
            block = TailqHead::<LfEntry>::next(ov);
        }
        // Six cases:
        //	0) no overlap
        //	1) overlap == lock
        //	2) overlap contains lock
        //	3) lock contains overlap
        //	4) overlap starts before lock
        //	5) overlap ends after lock
        //
        // Every list operation below works on `ls`'s locks under `lockf_lock`; `lock` is on
        // `ls_locks` exactly when `needtolink` is false.
        match (ovcase, overlap) {
            (0, overlap) => {
                // no overlap
                if needtolink {
                    match overlap {
                        // insert before overlap
                        // SAFETY: `ov` is on `ls_locks`, `lock` on no list.
                        Some(ov) => unsafe { TailqHead::<LfEntry>::insert_before(ov, lock) },
                        // first or last lock in list
                        // SAFETY: `lock` is on no list.
                        None => unsafe { ls.ls_locks.insert_tail(lock) },
                    }
                }
            }
            (1, Some(overlap)) => {
                // overlap == lock
                // If downgrading lock, others may be able to acquire it.
                if lock.lf_type.get() == F_RDLCK && overlap.lf_type.get() == F_WRLCK {
                    lf_wakelock(overlap, 0);
                }
                overlap.lf_type.set(lock.lf_type.get());
                lf_free(lock);
            }
            (2, Some(overlap)) => {
                // overlap contains lock
                // Check for common starting point and different types.
                if overlap.lf_type.get() == lock.lf_type.get() {
                    if !needtolink {
                        // SAFETY: `lock` is on `ls_locks` (`needtolink` is false).
                        unsafe { ls.ls_locks.remove(lock) };
                    }
                    lf_free(lock);
                    break;
                }
                if overlap.lf_start.get() == lock.lf_start.get() {
                    if !needtolink {
                        // SAFETY: as above.
                        unsafe { ls.ls_locks.remove(lock) };
                    }
                    // SAFETY: `overlap` is on `ls_locks`, `lock` is now on no list.
                    unsafe { TailqHead::<LfEntry>::insert_before(overlap, lock) };
                    overlap.lf_start.set(lock.lf_end.get() + 1);
                } else {
                    lf_split(overlap, lock);
                }
                lf_wakelock(overlap, 0);
            }
            (3, Some(overlap)) => {
                // lock contains overlap
                // If downgrading lock, others may be able to acquire it, otherwise take the
                // list.
                if lock.lf_type.get() == F_RDLCK && overlap.lf_type.get() == F_WRLCK {
                    lf_wakelock(overlap, 0);
                } else {
                    while let Some(ltmp) = overlap.lf_blkhd.first() {
                        // SAFETY: `ltmp` is the head of `overlap`'s blocked list; it moves to
                        // `lock`'s, and its `lf_blk` follows.
                        unsafe {
                            overlap.lf_blkhd.remove(ltmp);
                            ltmp.lf_blk.set(Some(lock));
                            lock.lf_blkhd.insert_tail(ltmp);
                        }
                    }
                }
                // Add the new lock if necessary and delete the overlap.
                if needtolink {
                    // SAFETY: `overlap` is on `ls_locks`, `lock` on no list.
                    unsafe { TailqHead::<LfEntry>::insert_before(overlap, lock) };
                    needtolink = false;
                }
                // SAFETY: `overlap` is on `ls_locks`.
                unsafe { ls.ls_locks.remove(overlap) };
                lf_free(overlap);
                continue;
            }
            (4, Some(overlap)) => {
                // overlap starts before lock
                // Add lock after overlap on the list.
                if !needtolink {
                    // SAFETY: `lock` is on `ls_locks` (`needtolink` is false).
                    unsafe { ls.ls_locks.remove(lock) };
                }
                // SAFETY: `overlap` is on `ls_locks`, `lock` is now on no list.
                unsafe { ls.ls_locks.insert_after(overlap, lock) };
                overlap.lf_end.set(lock.lf_start.get() - 1);
                lf_wakelock(overlap, 0);
                needtolink = false;
                continue;
            }
            (5, Some(overlap)) => {
                // overlap ends after lock
                // Add the new lock before overlap.
                if needtolink {
                    // SAFETY: `overlap` is on `ls_locks`, `lock` on no list.
                    unsafe { TailqHead::<LfEntry>::insert_before(overlap, lock) };
                }
                overlap.lf_start.set(lock.lf_end.get() + 1);
                lf_wakelock(overlap, 0);
            }
            _ => panic(format_args!(
                "lf_setlock: overlap case {ovcase} without a lock"
            )),
        }
        break;
    }
    // LFPRINT(("lf_setlock: got the lock", lock)): LOCKF_DEBUG.
    Ok(())
}

/// Remove a byte-range lock on an inode.
///
/// Generally, find the lock (or an overlap to that lock) and remove it (or shrink it), then
/// wakeup anyone we can.
fn lf_clearlock(lock: &'static Lockf) -> Result<(), Errno> {
    rw_assert_wrlock(&LOCKF_LOCK);

    let ls = lock.state();
    let mut lf = ls.ls_locks.first();
    if lf.is_none() {
        return Ok(());
    }

    loop {
        let (ovcase, overlap) = lf_findoverlap(lf, lock, SELF);
        let Some(overlap) = overlap.filter(|_| ovcase != 0) else {
            break;
        };
        lf_wakelock(overlap, 0);

        match ovcase {
            1 => {
                // overlap == lock
                // SAFETY: `overlap` is on `ls_locks`, under `lockf_lock`.
                unsafe { ls.ls_locks.remove(overlap) };
                lf_free(overlap);
            }
            2 => {
                // overlap contains lock: split it
                if overlap.lf_start.get() == lock.lf_start.get() {
                    overlap.lf_start.set(lock.lf_end.get() + 1);
                    break;
                }
                lf_split(overlap, lock);
                // The lock is now part of the list, lf_clearlock() must ensure that the lock
                // remains detached from the list.
                // SAFETY: `lf_split` just linked `lock` into `ls_locks`.
                unsafe { ls.ls_locks.remove(lock) };
            }
            3 => {
                // lock contains overlap
                lf = TailqHead::<LfEntry>::next(overlap);
                // SAFETY: `overlap` is on `ls_locks`, under `lockf_lock`.
                unsafe { ls.ls_locks.remove(overlap) };
                lf_free(overlap);
                continue;
            }
            4 => {
                // overlap starts before lock
                overlap.lf_end.set(lock.lf_start.get() - 1);
                lf = TailqHead::<LfEntry>::next(overlap);
                continue;
            }
            5 => {
                // overlap ends after lock
                overlap.lf_start.set(lock.lf_end.get() + 1);
            }
            _ => {}
        }
        break;
    }
    Ok(())
}

/// Check whether there is a blocking lock, and if so return its process identifier.
fn lf_getlock(lock: &'static Lockf, fl: &mut Flock) -> Result<(), Errno> {
    rw_assert_wrlock(&LOCKF_LOCK);

    let lf = lock.state().ls_locks.first();
    if let Some(block) = lf_getblock(lf, lock) {
        fl.l_type = block.lf_type.get();
        fl.l_whence = SEEK_SET as i16;
        fl.l_start = block.lf_start.get();
        if block.lf_end.get() == -1 {
            fl.l_len = 0;
        } else {
            fl.l_len = block.lf_end.get() - block.lf_start.get() + 1;
        }
        fl.l_pid = block.lf_pid.get();
    } else {
        fl.l_type = F_UNLCK;
    }
    Ok(())
}

/// Walk the list of locks for an inode and return the first blocking lock.
fn lf_getblock(mut lf: Option<&'static Lockf>, lock: &'static Lockf) -> Option<&'static Lockf> {
    rw_assert_wrlock(&LOCKF_LOCK);

    loop {
        let (ovcase, overlap) = lf_findoverlap(lf, lock, OTHERS);
        let overlap = overlap.filter(|_| ovcase != 0)?;
        // We've found an overlap, see if it blocks us
        if lock.lf_type.get() == F_WRLCK || overlap.lf_type.get() == F_WRLCK {
            return Some(overlap);
        }
        // Nope, point to the next one on the list and see if it blocks us
        lf = TailqHead::<LfEntry>::next(overlap);
    }
}

/// Walk the list of locks for an inode to find an overlapping lock (if any): the case (0
/// to 5, below) and the lock, or where the scan stopped for case 0.
///
/// NOTE: this returns only the FIRST overlapping lock. There may be more than one.
fn lf_findoverlap(
    mut lf: Option<&'static Lockf>,
    lock: &Lockf,
    type_: i32,
) -> (i32, Option<&'static Lockf>) {
    rw_assert_wrlock(&LOCKF_LOCK);

    let start = lock.lf_start.get();
    let end = lock.lf_end.get();
    while let Some(l) = lf {
        if (type_ & SELF != 0 && l.lf_id.get() != lock.lf_id.get())
            || (type_ & OTHERS != 0 && l.lf_id.get() == lock.lf_id.get())
        {
            lf = TailqHead::<LfEntry>::next(l);
            continue;
        }
        // OK, check for overlap
        //
        // Six cases:
        //	0) no overlap
        //	1) overlap == lock
        //	2) overlap contains lock
        //	3) lock contains overlap
        //	4) overlap starts before lock
        //	5) overlap ends after lock
        let (lstart, lend) = (l.lf_start.get(), l.lf_end.get());

        // Case 0
        if (lend != -1 && start > lend) || (end != -1 && lstart > end) {
            if type_ & SELF != 0 && end != -1 && lstart > end {
                return (0, Some(l));
            }
            lf = TailqHead::<LfEntry>::next(l);
            continue;
        }
        // Case 1
        if lstart == start && lend == end {
            return (1, Some(l));
        }
        // Case 2
        if lstart <= start && (lend == -1 || (end != -1 && lend >= end)) {
            return (2, Some(l));
        }
        // Case 3
        if start <= lstart && (end == -1 || (lend != -1 && end >= lend)) {
            return (3, Some(l));
        }
        // Case 4
        if lstart < start && (lend >= start || lend == -1) {
            return (4, Some(l));
        }
        // Case 5
        if lstart > start && end != -1 && (lend > end || lend == -1) {
            return (5, Some(l));
        }
        panic(format_args!("lf_findoverlap: default"));
    }
    (0, None)
}

/// Purge all locks associated with the given lock state.
pub fn lf_purgelocks(state: &LockfStateSlot) {
    rw_enter_write(&LOCKF_LOCK);

    if let Some(ls) = state.get() {
        ls_ref(ls);

        // Interrupt blocked locks and wait for all of them to finish.
        for lock in ls.ls_locks.iter() {
            lf_wakelock(lock, F_INTR);
        }
        while !ls.ls_pending.is_empty() {
            let _ = rwsleep_nsec(ptr::from_ref(ls), &LOCKF_LOCK, PLOCK, "lockfp", INFSLP);
        }

        // Any remaining locks cannot block other locks at this point and can safely be
        // removed.
        while let Some(lock) = ls.ls_locks.first() {
            // SAFETY: `lock` is the head of `ls_locks`, under `lockf_lock`.
            unsafe { ls.ls_locks.remove(lock) };
            lf_free(lock);
        }

        // This is the last expected thread to hold a lock state reference.
        kassert!(ls.ls_refs.get() == 1);
        ls_rele(ls);
    }

    rw_exit_write(&LOCKF_LOCK);
}

/// Split a lock and a contained region into two or three locks as necessary.
fn lf_split(lock1: &'static Lockf, lock2: &'static Lockf) {
    rw_assert_wrlock(&LOCKF_LOCK);

    let ls = lock1.state();
    // Check to see if splitting into only two pieces.
    if lock1.lf_start.get() == lock2.lf_start.get() {
        lock1.lf_start.set(lock2.lf_end.get() + 1);
        // SAFETY: `lock1` is on `ls_locks`, `lock2` on no list, under `lockf_lock`.
        unsafe { TailqHead::<LfEntry>::insert_before(lock1, lock2) };
        return;
    }
    if lock1.lf_end.get() == lock2.lf_end.get() {
        lock1.lf_end.set(lock2.lf_start.get() - 1);
        // SAFETY: as above.
        unsafe { ls.ls_locks.insert_after(lock1, lock2) };
        return;
    }
    // Make a new lock consisting of the last part of the encompassing lock
    let Some(splitlock) = lf_alloc(lock1.lf_uid.get(), 0) else {
        panic(format_args!("lf_split: out of memory"));
    };
    splitlock.lf_flags.set(lock1.lf_flags.get());
    splitlock.lf_type.set(lock1.lf_type.get());
    splitlock.lf_start.set(lock2.lf_end.get() + 1);
    splitlock.lf_end.set(lock1.lf_end.get());
    splitlock.lf_id.set(lock1.lf_id.get());
    splitlock.lf_state.set(lock1.lf_state.get());
    splitlock.lf_blk.set(None);
    splitlock.lf_pid.set(lock1.lf_pid.get());
    ls_ref(ls);
    lock1.lf_end.set(lock2.lf_start.get() - 1);

    // SAFETY: `lock1` is on `ls_locks`; `lock2` and the new `splitlock` are on no list.
    unsafe {
        ls.ls_locks.insert_after(lock1, lock2);
        ls.ls_locks.insert_after(lock2, splitlock);
    }
}

/// Wakeup a blocklist.
fn lf_wakelock(lock: &Lockf, flags: i32) {
    rw_assert_wrlock(&LOCKF_LOCK);

    while let Some(wakelock) = lock.lf_blkhd.first() {
        // SAFETY: `wakelock` is the head of `lf_blkhd`, under `lockf_lock`.
        unsafe { lock.lf_blkhd.remove(wakelock) };
        wakelock.lf_blk.set(None);
        wakelock.lf_flags.set(wakelock.lf_flags.get() | flags);
        wakeup_one(ptr::from_ref(wakelock));
    }
}

/// Returns true if the given lock would cause a deadlock.
fn lf_deadlock(lock: &'static Lockf) -> bool {
    let ls = lock.state();
    let mut lf = ls.ls_locks.first();
    while let Some(block) = lf_getblock(lf, lock) {
        lf = TailqHead::<LfEntry>::next(block);
        if block.lf_flags.get() & F_POSIX == 0 {
            continue;
        }

        for pending in ls.ls_pending.iter() {
            let Some(pblk) = pending.lf_blk.get() else {
                continue; // lock already unblocked
            };

            if pending.lf_pid.get() == block.lf_pid.get() && pblk.lf_pid.get() == lock.lf_pid.get()
            {
                return true;
            }
        }
    }

    false
}

// LOCKF_DEBUG (lf_print, lf_printlist): not configured.
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the advisory record locks: the splitting, merging, upgrading and
    // downgrading of one owner's ranges, the conflicts between owners, `F_GETLK`, the range
    // conversion of `lf_advlock` and `lf_purgelocks`.

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
}
/* </TESTS> */
