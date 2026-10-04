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
//! Status: `ported`. Milestone M4 ports `evcount_attach`, `evcount_detach`, `evcount_inc`,
//! `evcount_percpu` and `evcount_init_percpu`; the diagnostic tools (stage 2) port
//! `evcount_sysctl` (`kern.intrcnt`, `kern.evcount`); M11e the per-CPU counters
//! (`counters_alloc`, `counters_add`, `counters_inc`, `counters_read`, `counters_free`).
//!
//! ## Deviations
//! - The C's `TAILQ_HEAD_INITIALIZER`s are initialised on first use (`lists`).

use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::kassert;
use crate::kern::kern_sysctl::{sysctl_rdint, sysctl_rdquad, sysctl_rdstring};
use crate::kern::subr_percpu::{counters_alloc, counters_free, counters_read};
use crate::machine::intr::{splhigh, splx};
use crate::sys::errno::Errno;
use crate::sys::evcount::{Evcount, EvcountList};
use crate::sys::percpu::{counters_add, counters_inc};
use crate::sys::queue::TailqHead;
use crate::sys::sysctl::{
    KERN_INTRCNT_CNT, KERN_INTRCNT_NAME, KERN_INTRCNT_NUM, KERN_INTRCNT_VECTOR,
};

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
    ec.ec_percpu.set(None);
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
        ec.ec_percpu.set(Some(counters_alloc(1)));
    }
}

/// `evcount_init_percpu`: once `percpu` is up, gives the waiting counters their per-CPU
/// storage and merges the lists.
pub fn evcount_init_percpu() {
    let (list, init_list) = lists();
    kassert!(!EVCOUNT_PERCPU_DONE.load(Ordering::Relaxed));

    for ec in init_list.iter() {
        let percpu = counters_alloc(1);
        counters_add(percpu, 0, ec.ec_count.load(Ordering::Relaxed));
        ec.ec_percpu.set(Some(percpu));
        ec.ec_count.store(0, Ordering::Relaxed);
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
    if let Some(percpu) = ec.ec_percpu.take() {
        // SAFETY: `counters_alloc(1)` made it; the counter is detached, so nothing counts on
        // it any more.
        unsafe { counters_free(percpu, 1) };
    }
}

/// `evcount_inc`: counts one event.
pub fn evcount_inc(ec: &Evcount) {
    if let Some(percpu) = ec.ec_percpu.get() {
        counters_inc(percpu, 0);
    } else {
        ec.ec_count.fetch_add(1, Ordering::Relaxed);
    }
}

/// `evcount_sysctl`: `kern.intrcnt` and `kern.evcount`. `name` is `KERN_INTRCNT_NUM` (the
/// number of counters) or `<KERN_INTRCNT_CNT|NAME|VECTOR>.<index>` (the index-th counter's
/// count, name or interrupt vector). Read-only.
pub fn evcount_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    if newp != 0 {
        return Err(Errno::EPERM);
    }
    let Some(&what) = name.first() else {
        return Err(Errno::ENOTDIR);
    };

    let i = if what != KERN_INTRCNT_NUM {
        if name.len() != 2 {
            return Err(Errno::ENOTDIR);
        }
        if name[1] < 0 {
            return Err(Errno::EINVAL);
        }
        Some(name[1] as usize)
    } else {
        None
    };

    // The index-th counter, or the count of them all.
    let (list, _) = lists();
    let mut nintr = 0;
    let mut found = None;
    for ec in list.iter() {
        let this = nintr;
        nintr += 1;
        if Some(this) == i {
            found = Some(ec);
            break;
        }
    }

    match what {
        KERN_INTRCNT_NUM => sysctl_rdint(oldp, oldlenp, 0, nintr as i32),
        KERN_INTRCNT_CNT => {
            let ec = found.ok_or(Errno::ENOENT)?;
            let count = if let Some(percpu) = ec.ec_percpu.get() {
                let mut count = [0u64; 1];
                let mut scratch = [0u64; 1];
                counters_read(percpu, &mut count, 1, Some(&mut scratch));
                count[0]
            } else {
                let s = splhigh();
                let count = ec.ec_count.load(Ordering::Relaxed);
                splx(s);
                count
            };
            sysctl_rdquad(oldp, oldlenp, 0, count as i64)
        }
        KERN_INTRCNT_NAME => {
            let ec = found.ok_or(Errno::ENOENT)?;
            sysctl_rdstring(oldp, oldlenp, 0, ec.ec_name.get().as_bytes())
        }
        KERN_INTRCNT_VECTOR => {
            let ec = found.ok_or(Errno::ENOENT)?;
            let data = ec.ec_data.get();
            if data.is_null() {
                return Err(Errno::ENOENT);
            }
            // SAFETY: `evcount_attach`'s contract, as in C: a non-NULL `ec_data` points at
            // the counter's interrupt vector, an `int` that lives as long as the counter is
            // attached (a driver's static or softc field).
            let vector = unsafe { *data.cast::<i32>() };
            sysctl_rdint(oldp, oldlenp, 0, vector)
        }
        _ => Err(Errno::EOPNOTSUPP),
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

    #[test]
    fn sysctl_checks() {
        let mut len = 0;
        assert_eq!(
            evcount_sysctl(&[KERN_INTRCNT_NUM], 0, &mut len, 1, 4),
            Err(Errno::EPERM)
        );
        assert_eq!(
            evcount_sysctl(&[KERN_INTRCNT_CNT], 0, &mut len, 0, 0),
            Err(Errno::ENOTDIR)
        );
        assert_eq!(
            evcount_sysctl(&[KERN_INTRCNT_NAME, -1], 0, &mut len, 0, 0),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            evcount_sysctl(&[KERN_INTRCNT_NAME, i32::MAX], 0, &mut len, 0, 0),
            Err(Errno::ENOENT)
        );
        assert_eq!(
            evcount_sysctl(&[99, 0], 0, &mut len, 0, 0),
            Err(Errno::EOPNOTSUPP)
        );
    }
}
