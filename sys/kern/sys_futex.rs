/*	$OpenBSD: sys_futex.c,v 1.26 2025/08/18 03:51:45 dlg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2016-2017 Martin Pieuchot
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
/* </LICENSES> */

/* <CODE> */
//! `futex(2)`: `kern/sys_futex.c`.
//!
//! Upstream: sys/kern/sys_futex.c @ 3ce1f3f79392
//!
//! Status: `ported`. `futex_init`, `sys_futex` with `futex_wait`, `futex_wake` and
//! `futex_requeue`, the sleep queue buckets and the address keys (`futex_addrs`: the
//! process for a private futex, else the shared mapping's object or amap and offset).
//!
//! ## Deviations
//! - A waiting `struct futex` lives on the waiter's stack, linked into its bucket as in C;
//!   the list holds raw links (`TailqEntry`) and the one rule the C states is kept: only the
//!   holder of the bucket lock removes a futex and clears `ft_proc`, after which the waiter
//!   may return. `ft_proc` and `ft_fsq` are atomics because the waiter reads them without
//!   the lock.
//! - `__aligned(CACHELINESIZE)` is `#[repr(align(64))]`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::dev::rnd::arc4random;
use crate::kassert;
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_synch::{sleep_finish, sleep_setup, wakeup_proc};
use crate::kern::sched_bsd::{sched_lock, sched_unlock};
use crate::machine::copy::{copyin_obj, copyin32};
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::futex::{
    FUTEX_FLAG_MASK, FUTEX_OP_MASK, FUTEX_PRIVATE_FLAG, FUTEX_REQUEUE, FUTEX_WAIT, FUTEX_WAKE,
};
use crate::sys::mman::MAP_INHERIT_SHARE;
use crate::sys::param::{PCATCH, PWAIT};
use crate::sys::proc::{Proc, Process};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::syscallargs::SysFutexArgs;
use crate::sys::systm::{INFSLP, MAXTSLP, SysArgs, sysargs};
use crate::sys::time::{Timespec, timespec_to_nsec};
use crate::sys::types::Register;
use crate::uvm::uvm::uvm_et_isobj;
use crate::uvm::uvm_amap::VmAmap;
use crate::uvm::uvm_extern::Voff;
use crate::uvm::uvm_map::{
    VmMapEntryObject, uvm_map_lookup_entry, vm_map_lock_read, vm_map_unlock_read,
};
use crate::uvm::uvm_object::UvmObject;
use crate::uvm::uvm_param::ptoa;

/// `FT_PRIVATE`: futex is process-private (`futex_get` flags sit in `FUTEX_OP_MASK` space).
const FT_PRIVATE: i32 = FUTEX_PRIVATE_FLAG;

/// `FUTEX_SLPQUES_BITS`.
const FUTEX_SLPQUES_BITS: u32 = 6;
/// `FUTEX_SLPQUES_SIZE`.
const FUTEX_SLPQUES_SIZE: usize = 1 << FUTEX_SLPQUES_BITS;
/// `FUTEX_SLPQUES_MASK`.
const FUTEX_SLPQUES_MASK: u32 = FUTEX_SLPQUES_SIZE as u32 - 1;

/// `struct futex`: kernel representation of a futex.
///
/// The userland address that the futex is waiting on is represented by `ft_ps`, `ft_obj`,
/// `ft_amap`, and `ft_off`.
///
/// Whether the futex is waiting or woken up is represented by the `ft_proc` pointer being
/// set (ie, not NULL) or not (ie, NULL) respectively. When the futex is waiting it is
/// referenced by the list in a `futex_slpque`. When the futex gets woken up, it is removed
/// from the list and the `ft_proc` pointer is cleared to indicate that the reference held
/// by the list has been released. Only a thread holding the lock may remove the futex from
/// the list and clear `ft_proc`. This is true even for `futex_wait()`.
///
/// However, `futex_wait()` may read `ft_proc` without the lock so it can avoid contending
/// with the thread that just woke it up. This means that once `ft_proc` is cleared,
/// `futex_wait()` may return, the struct futex will no longer exist, and it is no longer
/// safe to access it from the wakeup side.
///
/// tl;dr: the thread holding the slpque lock "owns" the references to the futexes on the
/// list until it clears `ft_proc`.
pub struct Futex {
    /// \[f\] `ft_fsq`: current futex_slpque.
    ft_fsq: AtomicPtr<FutexSlpque>,
    /// \[f\] `ft_entry`: entry on futex_slpque.
    ft_entry: TailqEntry<Futex>,
    /// \[I\] `ft_ps`: for private futexes.
    ft_ps: Cell<*const Process>,
    /// \[f\] `ft_obj`: UVM object.
    ft_obj: Cell<*const UvmObject>,
    /// \[f\] `ft_amap`: UVM amap.
    ft_amap: Cell<*const VmAmap>,
    /// \[f\] `ft_off`: UVM offset.
    ft_off: Cell<Voff>,
    /// \[f\] `ft_proc`: waiting thread.
    ft_proc: AtomicPtr<Proc>,
}

impl Futex {
    /// A futex key with no address yet.
    const fn new() -> Self {
        Self {
            ft_fsq: AtomicPtr::new(ptr::null_mut()),
            ft_entry: TailqEntry::new(),
            ft_ps: Cell::new(ptr::null()),
            ft_obj: Cell::new(ptr::null()),
            ft_amap: Cell::new(ptr::null()),
            ft_off: Cell::new(0),
            ft_proc: AtomicPtr::new(ptr::null_mut()),
        }
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(futex_list, futex)`.
    FutexList: Futex, ft_entry => TailqEntry<Futex>
);

/// `futex_is_eq`: whether two futexes name the same userland address.
fn futex_is_eq(a: &Futex, b: &Futex) -> bool {
    a.ft_off.get() == b.ft_off.get()
        && a.ft_ps.get() == b.ft_ps.get()
        && a.ft_obj.get() == b.ft_obj.get()
        && a.ft_amap.get() == b.ft_amap.get()
}

/// `struct futex_slpque`: one bucket of waiting futexes.
#[repr(align(64))]
pub struct FutexSlpque {
    /// \[f\] `fsq_list`.
    fsq_list: TailqHead<FutexList>,
    /// `fsq_lock`.
    fsq_lock: Rwlock,
    /// \[I\] `fsq_id`: for lock ordering.
    fsq_id: Cell<u32>,
}

// SAFETY: `fsq_list` and the futexes on it are touched only under `fsq_lock`; `fsq_id` is
// written once by `futex_init` before any system call.
unsafe impl Sync for FutexSlpque {}

/// `futex_slpques[]`.
static FUTEX_SLPQUES: [FutexSlpque; FUTEX_SLPQUES_SIZE] = [const {
    FutexSlpque {
        fsq_list: TailqHead::new(),
        fsq_lock: Rwlock::new("futexlk"),
        fsq_id: Cell::new(0),
    }
}; FUTEX_SLPQUES_SIZE];

/// `futex_init`: the buckets' lists, locks and lock-ordering ids.
pub fn futex_init() {
    for (i, fsq) in FUTEX_SLPQUES.iter().enumerate() {
        fsq.fsq_list.init();
        rw_init(&fsq.fsq_lock, "futexlk");

        let mut id = arc4random();
        id &= !FUTEX_SLPQUES_MASK;
        id |= i as u32;
        fsq.fsq_id.set(id);
    }
}

/// `futex(2)`.
pub fn sys_futex(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFutexArgs = sysargs(v);
    let uaddr = uap.f.get() as usize;
    let op = uap.op.get();
    let val = uap.val.get() as u32;
    let timeout = uap.timeout.get() as usize;
    let g = uap.g.get() as usize;
    let flags = op & FUTEX_FLAG_MASK;

    match op & FUTEX_OP_MASK {
        FUTEX_WAIT => futex_wait(p, uaddr, val, timeout, flags),
        FUTEX_WAKE => futex_wake(p, uaddr, val, flags, retval),
        FUTEX_REQUEUE => futex_requeue(p, uaddr, val, g, timeout as u32, flags, retval),
        _ => Err(Errno::ENOSYS),
    }
}

/// `futex_addrs`: fills `f`'s key for `uaddr`: the process for a private futex, else the
/// object or amap (and offset) of a shared mapping, else the address alone.
fn futex_addrs(p: &Proc, f: &Futex, uaddr: usize, flags: i32) {
    let map = &p.vmspace().vm_map;
    let mut obj: *const UvmObject = ptr::null();
    let mut amap: *const VmAmap = ptr::null();
    let mut off = uaddr as Voff;
    let ps: *const Process;

    if flags & FT_PRIVATE != 0 {
        ps = p.process();
    } else {
        ps = ptr::null();

        vm_map_lock_read(map);
        if let Some(entry) = uvm_map_lookup_entry(map, uaddr)
            && entry.inheritance.get() == MAP_INHERIT_SHARE
        {
            if uvm_et_isobj(entry) {
                if let VmMapEntryObject::Obj(o) = entry.object.get() {
                    obj = o;
                }
                off = entry.offset.get() + (uaddr - entry.start.get()) as Voff;
            } else if !entry.aref.ar_amap.get().is_null() {
                amap = entry.aref.ar_amap.get();
                off = ptoa(entry.aref.ar_pageoff.get() as usize) as Voff
                    + (uaddr - entry.start.get()) as Voff;
            }
        }
        vm_map_unlock_read(map);
    }

    f.ft_ps.set(ps);
    f.ft_obj.set(obj);
    f.ft_amap.set(amap);
    f.ft_off.set(off);
}

/// `futex_get_slpque`: the bucket of `f`'s key.
fn futex_get_slpque(f: &Futex) -> &'static FutexSlpque {
    let mut key = (f.ft_off.get() >> 3) as u32; // watevs
    key ^= key >> FUTEX_SLPQUES_BITS;

    &FUTEX_SLPQUES[(key & FUTEX_SLPQUES_MASK) as usize]
}

/// `futex_unwait`: takes `f` off its bucket (following a requeue) unless a waker already
/// did; whether it was still waiting.
fn futex_unwait(ofsq: &'static FutexSlpque, f: &Futex) -> bool {
    // REQUEUE can move a futex between buckets, so follow it if needed.
    let mut ofsq = ofsq;
    let fsq = loop {
        rw_enter_write(&ofsq.fsq_lock);
        // SAFETY: `ft_fsq` always points at one of the static buckets.
        let fsq: &'static FutexSlpque = unsafe { &*f.ft_fsq.load(Ordering::Relaxed) };
        if ptr::eq(ofsq, fsq) {
            break fsq;
        }

        rw_exit_write(&ofsq.fsq_lock);
        ofsq = fsq;
    };

    let rv = !f.ft_proc.load(Ordering::Relaxed).is_null();
    if rv {
        // SAFETY: `f` is on `fsq`'s list (its `ft_proc` is set) and the lock is held.
        unsafe { fsq.fsq_list.remove(f) };
    }
    rw_exit_write(&fsq.fsq_lock);

    rv
}

/// Put the current thread on the sleep queue of the futex at address `uaddr`. Let it
/// sleep for the specified `timeout` time, or indefinitely if the argument is NULL.
fn futex_wait(p: &Proc, uaddr: usize, val: u32, timeout: usize, flags: i32) -> Result<(), Errno> {
    let f = Futex::new();
    let mut nsecs = INFSLP;

    if timeout != 0 {
        let ts: Timespec = copyin_obj(timeout)?;
        // KTRACE: not configured.
        if ts.tv_sec < 0 || !ts.is_valid() {
            return Err(Errno::EINVAL);
        }

        nsecs = timespec_to_nsec(&ts).min(MAXTSLP);
        if nsecs == 0 {
            return Err(Errno::ETIMEDOUT);
        }
    }

    futex_addrs(p, &f, uaddr, flags);
    let fsq = futex_get_slpque(&f);

    // Mark futex as waiting.
    f.ft_fsq
        .store(ptr::from_ref(fsq).cast_mut(), Ordering::Relaxed);
    f.ft_proc
        .store(ptr::from_ref(p).cast_mut(), Ordering::Relaxed);
    rw_enter_write(&fsq.fsq_lock);
    // Make the waiting futex visible to wake/requeue
    // SAFETY: `f` is on no list; it stays on this stack frame until it is off the list
    // again (every return below goes through futex_unwait or a waker's removal).
    unsafe { fsq.fsq_list.insert_tail(&f) };
    rw_exit_write(&fsq.fsq_lock);

    // Do not return before f has been removed from the slpque!

    // Read user space futex value
    let cval = match copyin32(uaddr) {
        Ok(cval) => cval,
        Err(error) => {
            // exit:
            if !f.ft_proc.load(Ordering::Relaxed).is_null() {
                futex_unwait(fsq, &f);
            }
            return Err(error);
        }
    };

    // If the value changed, stop here.
    if cval != val {
        if !f.ft_proc.load(Ordering::Relaxed).is_null() {
            futex_unwait(fsq, &f);
        }
        return Err(Errno::EAGAIN);
    }

    sleep_setup(ptr::from_ref(&f).cast::<c_void>(), PWAIT | PCATCH, "fsleep");
    let mut error = sleep_finish(nsecs, !f.ft_proc.load(Ordering::Relaxed).is_null());
    // Remove ourself if we haven't been awaken.
    if error.is_err() || !f.ft_proc.load(Ordering::Relaxed).is_null() {
        if !futex_unwait(fsq, &f) {
            error = Ok(());
        }

        error = match error {
            Err(Errno::ERESTART) => Err(Errno::ECANCELED),
            Err(Errno::EWOULDBLOCK) => Err(Errno::ETIMEDOUT),
            other => other,
        };
    }

    error
}

/// `futex_list_wakeup`: wakes every futex of `fl` (taken off its bucket by the caller).
///
/// Setting `ft_proc` to NULL releases the futex reference currently held via the slpque
/// lock. `SCHED_LOCK` is only needed to call `wakeup_proc`.
fn futex_list_wakeup(fl: &TailqHead<FutexList>) {
    sched_lock();
    let mut f = fl.first();
    while let Some(cur) = f {
        let nf = TailqHead::<FutexList>::next(cur);
        let p = cur.ft_proc.swap(ptr::null_mut(), Ordering::Relaxed);
        // `cur` may be gone now: its waiter can return as soon as it sees NULL.
        // SAFETY: `ft_proc` named the waiting thread, alive while it waits.
        if let Some(p) = unsafe { p.as_ref() } {
            wakeup_proc(p);
        }
        f = nf;
    }
    sched_unlock();
}

/// Wakeup at most `n` sibling threads sleeping on a futex at address `uaddr` and requeue
/// at most `m` sibling threads on a futex at address `uaddr2`.
#[allow(clippy::too_many_arguments)] // the C's signature
fn futex_requeue(
    p: &Proc,
    uaddr: usize,
    n: u32,
    uaddr2: usize,
    m: u32,
    flags: i32,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    let fl: TailqHead<FutexList> = TailqHead::new();
    fl.init();
    let okey = Futex::new();
    let nkey = Futex::new();
    let mut count: u32 = 0;
    let mut m = m;

    if m == 0 {
        return futex_wake(p, uaddr, n, flags, retval);
    }

    futex_addrs(p, &okey, uaddr, flags);
    let ofsq = futex_get_slpque(&okey);
    futex_addrs(p, &nkey, uaddr2, flags);
    let nfsq = futex_get_slpque(&nkey);

    if ofsq.fsq_id.get() < nfsq.fsq_id.get() {
        rw_enter_write(&ofsq.fsq_lock);
        rw_enter_write(&nfsq.fsq_lock);
    } else if ofsq.fsq_id.get() > nfsq.fsq_id.get() {
        rw_enter_write(&nfsq.fsq_lock);
        rw_enter_write(&ofsq.fsq_lock);
    } else {
        rw_enter_write(&ofsq.fsq_lock);
    }

    let mut mf: Option<&Futex> = None;
    let mut f = ofsq.fsq_list.first();
    while let Some(cur) = f {
        let nf = TailqHead::<FutexList>::next(cur);
        kassert!(!cur.ft_proc.load(Ordering::Relaxed).is_null());

        if futex_is_eq(cur, &okey) {
            // SAFETY: `cur` is on `ofsq`'s list, whose lock is held; `fl` is local.
            unsafe {
                ofsq.fsq_list.remove(cur);
                fl.insert_tail(cur);
            }

            count += 1;
            if count == n {
                mf = nf;
                break;
            }
        }
        f = nf;
    }

    if !fl.is_empty() {
        futex_list_wakeup(&fl);
    }

    // update matching futexes
    if let Some(start) = mf {
        // only iterate from the current entry to the tail of the list as it is now in
        // case we're requeueing on the end of the same list.
        let last = ofsq.fsq_list.last();
        let mut mf = Some(start);
        while let Some(cur) = mf {
            mf = TailqHead::<FutexList>::next(cur);
            kassert!(!cur.ft_proc.load(Ordering::Relaxed).is_null());

            let at_last = last.is_some_and(|l| ptr::eq(l, cur));
            if futex_is_eq(cur, &okey) {
                // SAFETY: `cur` is on `ofsq`'s list; both locks are held.
                unsafe { ofsq.fsq_list.remove(cur) };
                cur.ft_fsq
                    .store(ptr::from_ref(nfsq).cast_mut(), Ordering::Relaxed);
                cur.ft_ps.set(nkey.ft_ps.get());
                cur.ft_obj.set(nkey.ft_obj.get());
                cur.ft_amap.set(nkey.ft_amap.get());
                cur.ft_off.set(nkey.ft_off.get());
                // SAFETY: `cur` is on no list now; `nfsq`'s lock is held.
                unsafe { nfsq.fsq_list.insert_tail(cur) };

                m -= 1;
                if m == 0 {
                    break;
                }
            }
            if at_last {
                break;
            }
        }
    }

    if ofsq.fsq_id.get() != nfsq.fsq_id.get() {
        rw_exit_write(&nfsq.fsq_lock);
    }
    rw_exit_write(&ofsq.fsq_lock);

    retval[0] = count as Register;
    Ok(())
}

/// Wakeup at most `n` sibling threads sleeping on a futex at address `uaddr`.
fn futex_wake(
    p: &Proc,
    uaddr: usize,
    n: u32,
    flags: i32,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    let fl: TailqHead<FutexList> = TailqHead::new();
    fl.init();
    let key = Futex::new();
    let mut count: u32 = 0;

    if n == 0 {
        retval[0] = 0;
        return Ok(());
    }

    futex_addrs(p, &key, uaddr, flags);
    let fsq = futex_get_slpque(&key);

    rw_enter_write(&fsq.fsq_lock);

    let mut f = fsq.fsq_list.first();
    while let Some(cur) = f {
        let nf = TailqHead::<FutexList>::next(cur);
        kassert!(!cur.ft_proc.load(Ordering::Relaxed).is_null());

        if futex_is_eq(cur, &key) {
            // SAFETY: `cur` is on `fsq`'s list, whose lock is held; `fl` is local.
            unsafe {
                fsq.fsq_list.remove(cur);
                fl.insert_tail(cur);
            }

            count += 1;
            if count == n {
                break;
            }
        }
        f = nf;
    }

    if !fl.is_empty() {
        futex_list_wakeup(&fl);
    }

    rw_exit_write(&fsq.fsq_lock);

    retval[0] = count as Register;
    Ok(())
}
/* </CODE> */
