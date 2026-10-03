/*	$OpenBSD: kern_resource.c,v 1.97 2026/02/11 22:34:41 deraadt Exp $	*/
/*	$NetBSD: kern_resource.c,v 1.38 1996/10/23 07:19:38 matthias Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)kern_resource.c	8.5 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! Resource limits and usage: `kern/kern_resource.c`.
//!
//! Upstream: sys/kern/kern_resource.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part b2) ports the thread usage aggregation the scheduler
//! needs: `tuagg_sumup`, `tuagg_get_proc`, `tuagg_get_process`, `tuagg_add_process` and
//! `tuagg_add_runtime`; M6-b adds `calctsru`, `calcru` and `ruadd` for `exit1`. The
//! priority and limit syscalls (`sys_getpriority`, `sys_setpriority`, `donice`,
//! `sys_setrlimit`, `dosetrlimit`, `sys_getrlimit`), the `plimit` management
//! (`lim_startup`, `lim_fork`, `lim_free`, `lim_copy`, `lim_cur`, `lim_write_begin/commit`),
//! `sys_getrusage`, `dogetrusage` and `rucheck` come with the syscalls (M6-c).
//!
//! ## Deviations
//! - `tuagg_sumup` reads the source's fields one by one inside the `pc_cons` loop instead of
//!   copying the struct: the fields are `Cell`s.

use core::sync::atomic::Ordering;

use crate::kassert;
use crate::kern::kern_clock::STATHZ;
use crate::kern::kern_lock::{mtx_enter, mtx_leave, pc_cons_enter, pc_cons_leave};
use crate::kern::kern_tc::nanouptime;
use crate::machine::Machine;
use crate::machine::cpu::{Cpu, curcpu, curproc};
use crate::sys::mutex::mutex_assert_locked;
use crate::sys::proc::{
    Proc, ProcThrLink, Process, SDEAD, TU_ITICKS, TU_STICKS, TU_UTICKS, Tusage, tu_enter, tu_leave,
};
use crate::sys::queue::TailqHead;
use crate::sys::resource::Rusage;
use crate::sys::time::{
    Timespec, Timeval, timeradd, timespec_to_timeval, timespecadd, timespecsub,
};

/// `tuagg_sumup`: add the counts from `from` to `tu`, ensuring a consistent read of `from`.
pub fn tuagg_sumup(tu: &Tusage, from: &Tusage) {
    let mut generation = 0;
    let (mut uticks, mut sticks, mut iticks, mut ixrss, mut idrss, mut isrss, mut runtime);

    pc_cons_enter(&from.tu_pcl, &mut generation);
    loop {
        uticks = from.tu_ticks[TU_UTICKS].get();
        sticks = from.tu_ticks[TU_STICKS].get();
        iticks = from.tu_ticks[TU_ITICKS].get();
        ixrss = from.tu_ixrss.get();
        idrss = from.tu_idrss.get();
        isrss = from.tu_isrss.get();
        runtime = from.tu_runtime.get();
        if !pc_cons_leave(&from.tu_pcl, &mut generation) {
            break;
        }
    }

    tu.tu_ticks[TU_UTICKS].set(tu.tu_ticks[TU_UTICKS].get().wrapping_add(uticks));
    tu.tu_ticks[TU_STICKS].set(tu.tu_ticks[TU_STICKS].get().wrapping_add(sticks));
    tu.tu_ticks[TU_ITICKS].set(tu.tu_ticks[TU_ITICKS].get().wrapping_add(iticks));
    tu.tu_ixrss.set(tu.tu_ixrss.get().wrapping_add(ixrss));
    tu.tu_idrss.set(tu.tu_idrss.get().wrapping_add(idrss));
    tu.tu_isrss.set(tu.tu_isrss.get().wrapping_add(isrss));
    tu.tu_runtime
        .set(timespecadd(&tu.tu_runtime.get(), &runtime));
}

/// `tuagg_get_proc`: the usage of one thread.
pub fn tuagg_get_proc(tu: &Tusage, p: &Proc) {
    tuagg_clear(tu);
    tuagg_sumup(tu, &p.p_tu);
}

/// `tuagg_get_process`: the usage of a process: its own plus all living threads.
pub fn tuagg_get_process(tu: &Tusage, pr: &Process) {
    tuagg_clear(tu);

    mtx_enter(&pr.ps_mtx);
    tuagg_sumup(tu, &pr.ps_tu);
    // add on all living threads
    let mut q = pr.ps_threads.first();
    while let Some(thread) = q {
        tuagg_sumup(tu, &thread.p_tu);
        q = TailqHead::<ProcThrLink>::next(thread);
    }
    mtx_leave(&pr.ps_mtx);
}

/// `tuagg_add_process`: update the process `ps_tu` usage with the values from proc `p`
/// while doing so the times for proc `p` are reset. This requires that `p` is either
/// `curproc` or `SDEAD` and that the IPL is higher than `IPL_STATCLOCK`. `ps_mtx` uses
/// `IPL_HIGH` so this should always be the case.
pub fn tuagg_add_process(pr: &Process, p: &Proc) {
    mutex_assert_locked(&pr.ps_mtx, "tuagg_add_process");
    kassert!(curproc().is_some_and(|cur| core::ptr::eq(cur, p)) || p.p_stat.get() == SDEAD);

    let generation = tu_enter(&pr.ps_tu);
    tuagg_sumup(&pr.ps_tu, &p.p_tu);
    tu_leave(&pr.ps_tu, generation);

    // Now reset CPU time usage for the thread.
    tuagg_clear(&p.p_tu);
}

/// `memset(tu, 0, sizeof(*tu))` and `timespecclear` plus the tick and rss resets: every
/// counter of `tu` to zero (the lock is left alone).
fn tuagg_clear(tu: &Tusage) {
    tu.tu_runtime.set(Timespec::new(0, 0));
    for ticks in &tu.tu_ticks {
        ticks.set(0);
    }
    tu.tu_ixrss.set(0);
    tu.tu_idrss.set(0);
    tu.tu_isrss.set(0);
}

/// `tuagg_add_runtime`: compute the amount of time during which the current process was
/// running, and add that to its total so far.
pub fn tuagg_add_runtime() {
    let spc = Machine::ci_schedstate(curcpu());
    let Some(p) = curproc() else {
        return;
    };

    let ts = nanouptime();
    let delta = if ts < spc.spc_runtime.get() {
        // uptime is not monotonic (the C prints this under #if 0)
        Timespec::new(0, 0)
    } else {
        timespecsub(&ts, &spc.spc_runtime.get())
    };
    // update spc_runtime
    spc.spc_runtime.set(ts);
    let generation = tu_enter(&p.p_tu);
    p.p_tu
        .tu_runtime
        .set(timespecadd(&p.p_tu.tu_runtime.get(), &delta));
    tu_leave(&p.p_tu, generation);
}

/// `calctsru`: transform the running time and tick information in a struct tusage into
/// user, system, and interrupt time usage.
pub fn calctsru(tup: &Tusage) -> (Timespec, Timespec, Timespec) {
    let st = tup.tu_ticks[TU_STICKS].get();
    let ut = tup.tu_ticks[TU_UTICKS].get();
    let it = tup.tu_ticks[TU_ITICKS].get();

    if st + ut + it == 0 {
        return (
            Timespec::new(0, 0),
            Timespec::new(0, 0),
            Timespec::new(0, 0),
        );
    }

    let stathz = u64::try_from(STATHZ.load(Ordering::Relaxed))
        .unwrap_or(0)
        .max(1);
    let to_ts = |ticks: u64| {
        let ns = ticks * 1_000_000_000 / stathz;
        Timespec::new((ns / 1_000_000_000) as i64, (ns % 1_000_000_000) as i64)
    };
    (to_ts(ut), to_ts(st), to_ts(it))
}

/// `calcru`: `calctsru` in `timeval`s: (user, system, interrupt).
pub fn calcru(tup: &Tusage) -> (Timeval, Timeval, Timeval) {
    let (u, s, i) = calctsru(tup);
    (
        timespec_to_timeval(&u),
        timespec_to_timeval(&s),
        timespec_to_timeval(&i),
    )
}

/// `ruadd`: adds `ru2` into `ru`: the times, the larger `ru_maxrss`, the sums of the rest.
pub fn ruadd(ru: &Rusage, ru2: &Rusage) {
    ru.ru_utime
        .set(timeradd(&ru.ru_utime.get(), &ru2.ru_utime.get()));
    ru.ru_stime
        .set(timeradd(&ru.ru_stime.get(), &ru2.ru_stime.get()));
    if ru.ru_maxrss.get() < ru2.ru_maxrss.get() {
        ru.ru_maxrss.set(ru2.ru_maxrss.get());
    }
    // ru_first .. ru_last: ru_ixrss through ru_nivcsw
    for (a, b) in [
        (&ru.ru_ixrss, &ru2.ru_ixrss),
        (&ru.ru_idrss, &ru2.ru_idrss),
        (&ru.ru_isrss, &ru2.ru_isrss),
        (&ru.ru_minflt, &ru2.ru_minflt),
        (&ru.ru_majflt, &ru2.ru_majflt),
        (&ru.ru_nswap, &ru2.ru_nswap),
        (&ru.ru_inblock, &ru2.ru_inblock),
        (&ru.ru_oublock, &ru2.ru_oublock),
        (&ru.ru_msgsnd, &ru2.ru_msgsnd),
        (&ru.ru_msgrcv, &ru2.ru_msgrcv),
        (&ru.ru_nsignals, &ru2.ru_nsignals),
        (&ru.ru_nvcsw, &ru2.ru_nvcsw),
        (&ru.ru_nivcsw, &ru2.ru_nivcsw),
    ] {
        a.set(a.get().wrapping_add(b.get()));
    }
}
