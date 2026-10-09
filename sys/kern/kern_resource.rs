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

/* <CODE> */
//! Resource limits and usage: `kern/kern_resource.c`.
//!
//! Upstream: sys/kern/kern_resource.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part b2) ports the thread usage aggregation the scheduler
//! needs: `tuagg_sumup`, `tuagg_get_proc`, `tuagg_get_process`, `tuagg_add_process` and
//! `tuagg_add_runtime`; M6-b adds `calctsru`, `calcru` and `ruadd` for `exit1`. M7a adds the
//! `plimit` management (`lim_startup`, `lim_copy`, `lim_free`, `lim_fork`,
//! `lim_write_begin`/`lim_write_commit`, `lim_read_enter`, `lim_cur_proc`), `rucheck`,
//! `dosetrlimit`, `sys_setrlimit`, `sys_getrlimit`, `sys_getrusage` and `dogetrusage`, and
//! with the credentials the priority syscalls (`sys_getpriority`, `sys_setpriority`,
//! `donice`). The file is complete but for the KTRACE hooks, which are not configured.
//!
//! ## Deviations
//! - `tuagg_sumup` reads the source's fields one by one inside the `pc_cons` loop instead of
//!   copying the struct: the fields are `Cell`s.
//!   KTRACE is not configured.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::conf::param::{MAXFILES, MAXPROCESS};
use crate::kassert;
use crate::kern::kern_clock::STATHZ;
use crate::kern::kern_lock::{mtx_enter, mtx_leave, pc_cons_enter, pc_cons_leave};
use crate::kern::kern_proc::{ALLPROCESS, pgfind, prfind};
use crate::kern::kern_prot::suser;
use crate::kern::kern_rwlock::{rw_assert_wrlock, rw_enter_write, rw_exit_write};
use crate::kern::kern_sig::prsignal;
use crate::kern::kern_synch::{refcnt_init, refcnt_rele, refcnt_shared, refcnt_take};
use crate::kern::kern_tc::nanouptime;
use crate::kern::kern_timeout::timeout_add_msec;
use crate::kern::sched_bsd::{sched_lock, sched_unlock, setpriority};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::machine::Machine;
use crate::machine::copy::{copyin, copyout};
use crate::machine::cpu::{Cpu, curcpu, curproc};
use crate::machine::intr::IPL_MPFLOOR;
use crate::machine::{Machine as Md, VmParam};
use crate::sys::errno::Errno;
use crate::sys::mman::{PROT_NONE, PROT_READ, PROT_WRITE};
use crate::sys::mutex::mutex_assert_locked;
use crate::sys::param::NZERO;
use crate::sys::param::{MAXUPRC, NOFILE, NOFILE_MAX};
use crate::sys::pool::{PR_WAITOK, Pool};
use crate::sys::proc::p_hassibling;
use crate::sys::proc::{
    Proc, ProcThrLink, Process, SDEAD, TU_ITICKS, TU_STICKS, TU_UTICKS, Tusage, tu_enter, tu_leave,
};
use crate::sys::queue::TailqHead;
use crate::sys::resource::Rusage;
use crate::sys::resource::{PRIO_MAX, PRIO_MIN, PRIO_PGRP, PRIO_PROCESS, PRIO_USER};
use crate::sys::resource::{
    RLIM_INFINITY, RLIM_NLIMITS, RLIMIT_CPU, RLIMIT_DATA, RLIMIT_MEMLOCK, RLIMIT_NOFILE,
    RLIMIT_NPROC, RLIMIT_RSS, RLIMIT_STACK, RUSAGE_CHILDREN, RUSAGE_SELF, RUSAGE_THREAD, Rlimit,
};
use crate::sys::resourcevar::{Plimit, lim_read_leave};
use crate::sys::rwlock::Rwlock;
use crate::sys::signal::{SIGKILL, SIGXCPU};
use crate::sys::syscallargs::{
    SysGetpriorityArgs, SysGetrlimitArgs, SysGetrusageArgs, SysSetpriorityArgs, SysSetrlimitArgs,
};
use crate::sys::systm::{SysArgs, kernel_assert_locked, sysargs};
use crate::sys::time::{
    Timespec, Timeval, timeradd, timespec_to_timeval, timespecadd, timespecsub,
};
use crate::sys::types::Pid;
use crate::sys::types::{Register, Rlim};
use crate::uvm::uvm::UVM_ET_STACK;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_map::uvm_map_protect;
use crate::uvm::uvm_param::{ptoa, round_page, trunc_page};

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

/// `RUCHECK_INTERVAL`: resource usage check interval in msec.
const RUCHECK_INTERVAL: u64 = 1000;

/// `SIGXCPU_INTERVAL`: SIGXCPU interval in seconds of process runtime.
const SIGXCPU_INTERVAL: i64 = 5;

/// `maxdmap`: patchable maximum data limit.
pub static MAXDMAP: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(<Md as VmParam>::MAXDSIZ as u64);
/// `maxsmap`: patchable maximum stack limit.
pub static MAXSMAP: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(<Md as VmParam>::MAXSSIZ as u64);

/// `rlimit_lock`: serializes resource limit updates. This lock has to be held together with
/// `ps_mtx` when updating the process' `ps_limit`.
pub static RLIMIT_LOCK: Rwlock = Rwlock::new("rlimitlk");

/// `sys_getpriority`.
pub fn sys_getpriority(curp: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetpriorityArgs = sysargs(v);
    let mut low = NZERO + PRIO_MAX + 1;
    let who = uap.who.get();

    match uap.which.get() {
        PRIO_PROCESS => {
            let pr = if who == 0 {
                Some(curp.process())
            } else {
                prfind(who as Pid)
            };
            if let Some(pr) = pr
                && i32::from(pr.ps_nice.get()) < low
            {
                low = i32::from(pr.ps_nice.get());
            }
        }

        PRIO_PGRP => {
            let pg = if who == 0 {
                // SAFETY: a live process's pgrp outlives it while it is a member.
                unsafe { curp.process().ps_pgrp.get().as_ref() }
            } else {
                pgfind(who as Pid)
            };
            if let Some(pg) = pg {
                for pr in pg.pg_members.iter() {
                    if i32::from(pr.ps_nice.get()) < low {
                        low = i32::from(pr.ps_nice.get());
                    }
                }
            }
        }

        PRIO_USER => {
            let who = if who == 0 {
                curp.ucred().cr_uid.get()
            } else {
                who
            };
            for pr in ALLPROCESS.0.iter() {
                if pr.ucred().cr_uid.get() == who && i32::from(pr.ps_nice.get()) < low {
                    low = i32::from(pr.ps_nice.get());
                }
            }
        }

        _ => return Err(Errno::EINVAL),
    }
    if low == NZERO + PRIO_MAX + 1 {
        return Err(Errno::ESRCH);
    }
    retval[0] = (low - NZERO) as Register;
    Ok(())
}

/// `sys_setpriority`.
pub fn sys_setpriority(curp: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetpriorityArgs = sysargs(v);
    let who = uap.who.get();
    let prio = uap.prio.get();
    let mut found = false;
    let mut error = Ok(());

    match uap.which.get() {
        PRIO_PROCESS => {
            let pr = if who == 0 {
                Some(curp.process())
            } else {
                prfind(who as Pid)
            };
            if let Some(pr) = pr {
                error = donice(curp, pr, prio);
                found = true;
            }
        }

        PRIO_PGRP => {
            let pg = if who == 0 {
                // SAFETY: a live process's pgrp outlives it while it is a member.
                unsafe { curp.process().ps_pgrp.get().as_ref() }
            } else {
                pgfind(who as Pid)
            };
            if let Some(pg) = pg {
                for pr in pg.pg_members.iter() {
                    error = donice(curp, pr, prio);
                    found = true;
                }
            }
        }

        PRIO_USER => {
            let who = if who == 0 {
                curp.ucred().cr_uid.get()
            } else {
                who
            };
            for pr in ALLPROCESS.0.iter() {
                if pr.ucred().cr_uid.get() == who {
                    error = donice(curp, pr, prio);
                    found = true;
                }
            }
        }

        _ => return Err(Errno::EINVAL),
    }
    if !found {
        return Err(Errno::ESRCH);
    }
    error
}

/// `donice`: sets the nice value of `chgpr` to `n` on behalf of `curp`.
pub fn donice(curp: &Proc, chgpr: &Process, n: i32) -> Result<(), Errno> {
    let ucred = curp.ucred();

    if ucred.cr_uid.get() != 0
        && ucred.cr_ruid.get() != 0
        && ucred.cr_uid.get() != chgpr.ucred().cr_uid.get()
        && ucred.cr_ruid.get() != chgpr.ucred().cr_uid.get()
    {
        return Err(Errno::EPERM);
    }
    let n = n.clamp(PRIO_MIN, PRIO_MAX) + NZERO;
    if n < i32::from(chgpr.ps_nice.get()) && suser(curp).is_err() {
        return Err(Errno::EACCES);
    }
    chgpr.ps_nice.set(n as u8);
    mtx_enter(&chgpr.ps_mtx);
    sched_lock();
    let mut q = chgpr.ps_threads.first();
    while let Some(p) = q {
        setpriority(p, p.p_estcpu.get(), n as u8);
        q = TailqHead::<ProcThrLink>::next(p);
    }
    sched_unlock();
    mtx_leave(&chgpr.ps_mtx);
    Ok(())
}

/// `sys_setrlimit`.
pub fn sys_setrlimit(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetrlimitArgs = sysargs(v);
    let mut buf = [0u8; size_of::<Rlimit>()];
    copyin(uap.rlp.get() as usize, &mut buf)?;
    let mut alim = rlimit_from_bytes(&buf);
    // KTRACE: not configured.
    dosetrlimit(p, uap.which.get() as u32, &mut alim)
}

/// `dosetrlimit`: sets limit `which` of `p`'s process to `limp`, clamped to the system's
/// maximum.
pub fn dosetrlimit(p: &Proc, which: u32, limp: &mut Rlimit) -> Result<(), Errno> {
    let which = which as usize;
    if which >= RLIM_NLIMITS || limp.rlim_cur > limp.rlim_max {
        return Err(Errno::EINVAL);
    }

    rw_enter_write(&RLIMIT_LOCK);

    // SAFETY: a process's ps_limit is set at creation and replaced only under rlimit_lock,
    // which we hold.
    let alimp = unsafe { &*p.process().ps_limit.get() }.pl_rlimit[which].get();
    if limp.rlim_max > alimp.rlim_max
        && let Err(error) = suser(p)
    {
        rw_exit_write(&RLIMIT_LOCK);
        return Err(error);
    }

    // Get exclusive write access to the limit structure.
    let limit = lim_write_begin();
    let alimp = limit.pl_rlimit[which].get();

    let maxlim: Rlim = match which {
        RLIMIT_DATA => MAXDMAP.load(Ordering::Relaxed),
        RLIMIT_STACK => MAXSMAP.load(Ordering::Relaxed),
        RLIMIT_NOFILE => MAXFILES.load(Ordering::Relaxed) as Rlim,
        RLIMIT_NPROC => MAXPROCESS.load(Ordering::Relaxed) as Rlim,
        _ => RLIM_INFINITY,
    };

    if limp.rlim_max > maxlim {
        limp.rlim_max = maxlim;
    }
    if limp.rlim_cur > limp.rlim_max {
        limp.rlim_cur = limp.rlim_max;
    }

    if which == RLIMIT_CPU && limp.rlim_cur != RLIM_INFINITY && alimp.rlim_cur == RLIM_INFINITY {
        timeout_add_msec(&p.process().ps_rucheck_to, RUCHECK_INTERVAL);
    }

    if which == RLIMIT_STACK {
        // Stack is allocated to the max at exec time with only "rlim_cur" bytes accessible.
        // If stack limit is going up make more accessible, if going down make inaccessible.
        if limp.rlim_cur != alimp.rlim_cur {
            let vm = p.vmspace();
            // MACHINE_STACK_GROWS_UP: neither amd64 nor arm64.
            let (prot, size, addr) = if limp.rlim_cur > alimp.rlim_cur {
                (
                    PROT_READ | PROT_WRITE,
                    (limp.rlim_cur - alimp.rlim_cur) as usize,
                    vm.vm_minsaddr.get() - limp.rlim_cur as usize,
                )
            } else {
                (
                    PROT_NONE,
                    (alimp.rlim_cur - limp.rlim_cur) as usize,
                    vm.vm_minsaddr.get() - alimp.rlim_cur as usize,
                )
            };
            let addr = trunc_page(addr);
            let size = round_page(size);
            let _ = uvm_map_protect(
                &vm.vm_map,
                addr,
                addr + size,
                prot,
                UVM_ET_STACK,
                false,
                false,
            );
        }
    }

    limit.pl_rlimit[which].set(*limp);

    lim_write_commit(limit);
    rw_exit_write(&RLIMIT_LOCK);

    Ok(())
}

/// `sys_getrlimit`.
pub fn sys_getrlimit(_p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetrlimitArgs = sysargs(v);
    let which = uap.which.get();
    if which < 0 || which as usize >= RLIM_NLIMITS {
        return Err(Errno::EINVAL);
    }
    let limit = lim_read_enter();
    let alimp = limit.pl_rlimit[which as usize].get();
    lim_read_leave(limit);
    // KTRACE: not configured.
    copyout(&rlimit_to_bytes(&alimp), uap.rlp.get() as usize)
}

/// `struct rlimit` as the user sees it: `rlim_cur` then `rlim_max`, native endian.
fn rlimit_to_bytes(r: &Rlimit) -> [u8; size_of::<Rlimit>()] {
    let mut b = [0u8; size_of::<Rlimit>()];
    b[..8].copy_from_slice(&r.rlim_cur.to_ne_bytes());
    b[8..].copy_from_slice(&r.rlim_max.to_ne_bytes());
    b
}

/// The inverse of [`rlimit_to_bytes`].
fn rlimit_from_bytes(b: &[u8; size_of::<Rlimit>()]) -> Rlimit {
    let mut cur = [0u8; 8];
    let mut max = [0u8; 8];
    cur.copy_from_slice(&b[..8]);
    max.copy_from_slice(&b[8..]);
    Rlimit {
        rlim_cur: Rlim::from_ne_bytes(cur),
        rlim_max: Rlim::from_ne_bytes(max),
    }
}

/// `sys_getrusage`.
pub fn sys_getrusage(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetrusageArgs = sysargs(v);
    let ru = Rusage::default();
    dogetrusage(p, uap.who.get(), &ru)?;
    // KTRACE: not configured.
    copyout(&ru.to_bytes(), uap.rusage.get() as usize)
}

/// `dogetrusage`: the resource usage `who` (`RUSAGE_SELF`, `RUSAGE_THREAD`,
/// `RUSAGE_CHILDREN`) of `p`, into `rup`.
pub fn dogetrusage(p: &Proc, who: i32, rup: &Rusage) -> Result<(), Errno> {
    let pr = p.process();
    let tu = Tusage::new();

    kernel_assert_locked(); // KERNEL_ASSERT_LOCKED()

    match who {
        RUSAGE_SELF => {
            // start with the sum of dead threads, if any
            // SAFETY: ps_ru is null or the process's own rusage, set once at its first
            // thread exit and freed with the process.
            match unsafe { pr.ps_ru.get().as_ref() } {
                Some(ru) => rup.copy_from(ru),
                None => rup.copy_from(&Rusage::default()),
            }
            tuagg_sumup(&tu, &pr.ps_tu);

            // add on all living threads
            let mut q = pr.ps_threads.first();
            while let Some(thread) = q {
                ruadd(rup, &thread.p_ru);
                tuagg_sumup(&tu, &thread.p_tu);
                q = TailqHead::<ProcThrLink>::next(thread);
            }

            let (ut, st, _) = calcru(&tu);
            rup.ru_utime.set(ut);
            rup.ru_stime.set(st);

            rup.ru_ixrss.set(tu.tu_ixrss.get() as i64);
            rup.ru_idrss.set(tu.tu_idrss.get() as i64);
            rup.ru_isrss.set(tu.tu_isrss.get() as i64);
        }
        RUSAGE_THREAD => {
            rup.copy_from(&p.p_ru);
            let (ut, st, _) = calcru(&p.p_tu);
            rup.ru_utime.set(ut);
            rup.ru_stime.set(st);
        }
        RUSAGE_CHILDREN => rup.copy_from(&pr.ps_cru),
        _ => return Err(Errno::EINVAL),
    }
    Ok(())
}

/// `rucheck`: check if the process exceeds its cpu resource allocation. If over max, kill
/// it.
pub fn rucheck(arg: *mut c_void) {
    // SAFETY: the timeout's argument is the process (`process_initialize`), which outlives
    // its timeouts (they are deleted in `exit1`).
    let pr = unsafe { &*(arg as *const Process) };
    let tu = Tusage::new();

    kernel_assert_locked(); // KERNEL_ASSERT_LOCKED()

    mtx_enter(&pr.ps_mtx);
    // SAFETY: a live process's ps_limit is valid; ps_mtx is held.
    let rlim = unsafe { &*pr.ps_limit.get() }.pl_rlimit[RLIMIT_CPU].get();
    tuagg_sumup(&tu, &pr.ps_tu);
    let mut q = pr.ps_threads.first();
    while let Some(thread) = q {
        tuagg_sumup(&tu, &thread.p_tu);
        q = TailqHead::<ProcThrLink>::next(thread);
    }
    mtx_leave(&pr.ps_mtx);

    let runtime = tu.tu_runtime.get().tv_sec;

    if runtime as Rlim >= rlim.rlim_cur {
        if runtime as Rlim >= rlim.rlim_max {
            prsignal(pr, SIGKILL);
        } else if runtime >= pr.ps_nextxcpu.get() {
            prsignal(pr, SIGXCPU);
            pr.ps_nextxcpu.set(runtime + SIGXCPU_INTERVAL);
        }
    }

    timeout_add_msec(&pr.ps_rucheck_to, RUCHECK_INTERVAL);
}

/// `plimit_pool`.
static PLIMIT_POOL: Pool = Pool::new();

/// `lim_startup`: the pool, and the limits of process 0 in `limit0`.
pub fn lim_startup(limit0: &Plimit) {
    pool_init(
        &PLIMIT_POOL,
        size_of::<Plimit>(),
        0,
        IPL_MPFLOOR,
        PR_WAITOK,
        "plimitpl",
        None,
    );

    for l in &limit0.pl_rlimit {
        l.set(Rlimit {
            rlim_cur: RLIM_INFINITY,
            rlim_max: RLIM_INFINITY,
        });
    }
    let maxfiles = MAXFILES.load(Ordering::Relaxed) as usize;
    limit0.pl_rlimit[RLIMIT_NOFILE].set(Rlimit {
        rlim_cur: NOFILE as Rlim,
        rlim_max: NOFILE_MAX.min(if maxfiles - NOFILE > NOFILE {
            maxfiles - NOFILE
        } else {
            NOFILE
        }) as Rlim,
    });
    set_cur(limit0, RLIMIT_NPROC, MAXUPRC as Rlim);
    let lim = ptoa(UVMEXP.free.load(Ordering::Relaxed) as usize) as Rlim;
    set_max(limit0, RLIMIT_RSS, lim);
    let lim = ptoa(64 * 1024) as Rlim; // Default to very low
    set_max(limit0, RLIMIT_MEMLOCK, lim);
    set_cur(limit0, RLIMIT_MEMLOCK, lim / 3);
    refcnt_init(&limit0.pl_refcnt);
}

/// Sets `rlim_cur` of limit `which`.
fn set_cur(limit: &Plimit, which: usize, v: Rlim) {
    let mut r = limit.pl_rlimit[which].get();
    r.rlim_cur = v;
    limit.pl_rlimit[which].set(r);
}

/// Sets `rlim_max` of limit `which`.
fn set_max(limit: &Plimit, which: usize, v: Rlim) {
    let mut r = limit.pl_rlimit[which].get();
    r.rlim_max = v;
    limit.pl_rlimit[which].set(r);
}

/// `lim_copy`: make a copy of the plimit structure. We share these structures copy-on-write
/// after fork, and copy when a limit is changed.
pub fn lim_copy(lim: &Plimit) -> &'static Plimit {
    let Some(mem) = pool_get(&PLIMIT_POOL, PR_WAITOK) else {
        crate::kern::subr_prf::panic(format_args!("lim_copy: plimit_pool is empty"));
    };
    let newlim = mem.cast::<Plimit>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Plimit>()` bytes, written once
    // before anything else sees it; it lives until `lim_free` drops the last reference.
    let newlim: &'static Plimit = unsafe {
        newlim.as_ptr().write(Plimit::new());
        newlim.as_ref()
    };
    for (n, o) in newlim.pl_rlimit.iter().zip(lim.pl_rlimit.iter()) {
        n.set(o.get());
    }
    refcnt_init(&newlim.pl_refcnt);
    newlim
}

/// `lim_free`: drops a reference; the last returns the structure to the pool.
pub fn lim_free(lim: &Plimit) {
    if !refcnt_rele(&lim.pl_refcnt) {
        return;
    }
    pool_put(&PLIMIT_POOL, NonNull::from(lim).cast::<u8>());
}

/// `lim_fork`: `child` shares `parent`'s limits.
pub fn lim_fork(parent: &Process, child: &Process) {
    mtx_enter(&parent.ps_mtx);
    let limit = parent.ps_limit.get();
    // SAFETY: a live process's ps_limit is valid; ps_mtx is held.
    refcnt_take(&unsafe { &*limit }.pl_refcnt);
    mtx_leave(&parent.ps_mtx);

    child.ps_limit.set(limit);

    // SAFETY: as above; the child now holds a reference.
    if unsafe { &*limit }.pl_rlimit[RLIMIT_CPU].get().rlim_cur != RLIM_INFINITY {
        timeout_add_msec(&child.ps_rucheck_to, RUCHECK_INTERVAL);
    }
}

/// `lim_write_begin`: return an exclusive write reference to the process' resource limit
/// structure. The caller has to release the structure by calling `lim_write_commit()`.
///
/// This invalidates any plimit read reference held by the calling thread.
pub fn lim_write_begin() -> &'static Plimit {
    let p = curproc_ref();

    rw_assert_wrlock(&RLIMIT_LOCK);

    // SAFETY: p_limit is null or a reference this thread holds.
    if let Some(l) = unsafe { p.p_limit.get().as_ref() } {
        lim_free(l);
    }
    p.p_limit.set(ptr::null());

    // It is safe to access ps_limit here without holding ps_mtx because rlimit_lock excludes
    // other writers.

    // SAFETY: a live process's ps_limit is valid and stays so while rlimit_lock is held.
    let limit: &'static Plimit = unsafe { &*p.process().ps_limit.get() };
    if p_hassibling(p) || refcnt_shared(&limit.pl_refcnt) {
        return lim_copy(limit);
    }

    limit
}

/// `lim_write_commit`: finish exclusive write access to the plimit structure. This makes the
/// structure visible to other threads in the process.
pub fn lim_write_commit(limit: &'static Plimit) {
    let p = curproc_ref();

    rw_assert_wrlock(&RLIMIT_LOCK);

    let pr = p.process();
    if !ptr::eq(limit, pr.ps_limit.get()) {
        mtx_enter(&pr.ps_mtx);
        let olimit = pr.ps_limit.get();
        pr.ps_limit.set(limit);
        mtx_leave(&pr.ps_mtx);

        // SAFETY: the process held a reference to the old structure, now ours to drop.
        lim_free(unsafe { &*olimit });
    }
}

/// `lim_read_enter`: begin read access to the process' resource limit structure. The access
/// has to be finished by calling `lim_read_leave()`.
///
/// Sections denoted by `lim_read_enter()` and `lim_read_leave()` cannot nest.
pub fn lim_read_enter() -> &'static Plimit {
    let p = curproc_ref();
    let pr = p.process();

    // This thread might not observe the latest value of ps_limit if another thread updated
    // the limits very recently on another CPU. However, the anomaly should disappear quickly,
    // especially if there is any synchronization activity between the threads (or the CPUs).

    let mut limit = p.p_limit.get();
    if !ptr::eq(limit, pr.ps_limit.get()) {
        mtx_enter(&pr.ps_mtx);
        limit = pr.ps_limit.get();
        // SAFETY: a live process's ps_limit is valid; ps_mtx is held.
        refcnt_take(&unsafe { &*limit }.pl_refcnt);
        mtx_leave(&pr.ps_mtx);
        // SAFETY: p_limit is null or a reference this thread holds.
        if let Some(l) = unsafe { p.p_limit.get().as_ref() } {
            lim_free(l);
        }
        p.p_limit.set(limit);
    }
    kassert!(!limit.is_null());
    // SAFETY: this thread holds a reference (p_limit) until it next refreshes or exits.
    unsafe { &*limit }
}

/// `lim_cur_proc`: get the value of the resource limit in given process.
pub fn lim_cur_proc(p: &Proc, which: usize) -> Rlim {
    let pr = p.process();

    kassert!(which < RLIM_NLIMITS);

    mtx_enter(&pr.ps_mtx);
    // SAFETY: a live process's ps_limit is valid; ps_mtx is held.
    let val = unsafe { &*pr.ps_limit.get() }.pl_rlimit[which]
        .get()
        .rlim_cur;
    mtx_leave(&pr.ps_mtx);
    val
}

/// `curproc`, which a limit operation always has.
fn curproc_ref() -> &'static Proc {
    match curproc() {
        Some(p) => p,
        None => crate::kern::subr_prf::panic(format_args!("kern_resource: no curproc")),
    }
}
/* </CODE> */
