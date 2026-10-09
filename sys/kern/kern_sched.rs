/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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

/*
 * Copyright (c) 2007, 2008 Artur Grabowski <art@openbsd.org>
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
//! The run queues and CPU selection: `kern/kern_sched.c`.
//!
//! Upstream: sys/kern/kern_sched.c @ 3ce1f3f79392
//!
//! Status: `ported`. Milestone M5 (part a) ports `sched_init` and `sched_init_cpu` as far as
//! the clock interrupts: the four `clockintr_bind`s; part b2 adds the rest: the CPU sets
//! (`sched_idle_cpus`, `sched_queued_cpus`, `sched_all_cpus`, `cpuset_*`), the counters,
//! `sched_kthreads_create`, `sched_idle`, `sched_exit`, `sched_toidle`, `setrunqueue`,
//! `remrunqueue`, `sched_chooseproc`, `sched_choosecpu_fork`, `sched_choosecpu`,
//! `sched_steal_proc`, `sched_proc_to_cpu_cost`, `sched_peg_curproc`,
//! `sched_unpeg_curproc`, `sched_barrier`, `sysctl_hwncpuonline` and `cpu_is_online`. M11a
//! adds the `MULTIPROCESSOR` paths: the kernel lock in `sched_idle`, `sched_exit` and
//! `sched_toidle`; `SPCF_SHOULDHALT`/`SPCF_HALTED` in `sched_idle` and `sched_chooseproc`;
//! the CPU choice of `sched_choosecpu_fork` and `sched_choosecpu`, the stealing of
//! `sched_steal_proc` and the cost estimate (`log2`, `sched_cost_*`,
//! `sched_proc_to_cpu_cost`); `sched_start_secondary_cpus`, `sched_stop_secondary_cpus`,
//! `sched_barrier_task` and the `MULTIPROCESSOR` `sched_barrier`; `smr_idle` in `sched_idle`;
//! and `__HAVE_CPU_TOPOLOGY` (both machines define it): `sched_blockcpu`,
//! `sched_cpuadjust`, `sysctl_hwsmt` and `sysctl_hwblockcpu`.
//!
//! ## Deviations
//! - The `cpuset_*` functions with a `to` output return the new set instead.
//! - `ci->ci_cputype` (`__HAVE_CPU_TOPOLOGY`) is `Cpu::ci_cputype`, whose default is 0 (no
//!   topology known) until the machines port their topology probes (amd64 `identcpu.c`
//!   `cpu_topology`, arm64 `cpu.c` `cpu_identify`): `hw.smt=0` and `hw.blockcpu` then block
//!   no CPU.
//! - `sched_barrier` queues its task, which lives on its stack frame, with `task_add_local`
//!   (`kern_task.rs`); it waits for the task's signal before the frame goes.
//! - `sched_steal_proc` skips a CPU in `sched_queued_cpus` with no queue flagged, and
//!   `sched_proc_to_cpu_cost` a thread without an address space, where the C would index
//!   queue -1 or follow a null pointer; neither happens under `sched_lock`.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::kassert;
use crate::kern::init_main::{NCPUS, PROC0};
use crate::kern::kern_clock::statclock;
use crate::kern::kern_clockintr::{clockintr_bind, clockintr_cancel};
use crate::kern::kern_exit::exit2;
use crate::kern::kern_fork::fork1;
use crate::kern::kern_kthread::kthread_create_deferred;
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_lock::{__mp_release_all, _kernel_lock_held, KERNEL_LOCK};
use crate::kern::kern_resource::tuagg_add_runtime;
use crate::kern::kern_smr::smr_idle;
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_synch::{cond_init, cond_wait, sleep_finish, sleep_setup, wakeup};
use crate::kern::kern_sysctl::{sysctl_int_bounded, sysctl_rdstring, sysctl_string};
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_task::{SYSTQMP, task_add_local};
use crate::kern::kern_time::itimer_update;
#[cfg(feature = "multiprocessor")]
use crate::kern::sched_bsd::sched_unlock;
use crate::kern::sched_bsd::{mi_switch, roundrobin, sched_assert_locked, sched_lock};
use crate::kern::subr_prf::{panic, snprintf};
use crate::kern::subr_prof::profclock;
use crate::machine::Machine;
#[cfg(feature = "multiprocessor")]
use crate::machine::cpu::cpu_is_running;
use crate::machine::cpu::{Cpu, CpuInfo, MAXCPUS, cpu_info_foreach, curcpu, curproc, need_resched};
use crate::machine::intr::{IPL_NONE, splassert};
#[cfg(feature = "multiprocessor")]
use crate::machine::pmap::pmap_resident_count;
use crate::sys::errno::Errno;
#[cfg(feature = "multiprocessor")]
use crate::sys::param::PZERO;
#[cfg(feature = "multiprocessor")]
use crate::sys::proc::Cond;
use crate::sys::proc::{
    _MAXCOMLEN, Cpuset, FORK_IDLE, FORK_NOZOMBIE, FORK_SHAREFILES, FORK_SHAREVM, FORK_SYSTEM,
    P_CPUPEG, P_INSCHED, Proc, SRUN, SSLEEP, cpuset_asize,
};
use crate::sys::sched::{
    CPUTYP_E, CPUTYP_L, CPUTYP_P, CPUTYP_SMT, SCHED_NQS, SPCF_ITIMER, SPCF_PROFCLOCK,
    SPCF_SWITCHCLEAR,
};
#[cfg(feature = "multiprocessor")]
use crate::sys::sched::{SPCF_HALTED, SPCF_SHOULDHALT};
use crate::sys::systm::kernel_unlock;
#[cfg(feature = "multiprocessor")]
use crate::sys::systm::{INFSLP, cond_signal, kernel_assert_locked};
#[cfg(feature = "multiprocessor")]
use crate::sys::task::Task;
use crate::uvm::uvm_init::UVMEXP;

/*
 * To help choosing which cpu should run which process we keep track
 * of cpus which are currently idle and which cpus have processes
 * queued.
 */

/// `sched_idle_cpus`.
pub static SCHED_IDLE_CPUS: Cpuset = Cpuset::new();
/// `sched_queued_cpus`.
pub static SCHED_QUEUED_CPUS: Cpuset = Cpuset::new();
/// `sched_all_cpus`.
pub static SCHED_ALL_CPUS: Cpuset = Cpuset::new();

/*
 * Some general scheduler counters.
 */

/// `sched_nmigrations`: Cpu migration counter.
pub static SCHED_NMIGRATIONS: AtomicU64 = AtomicU64::new(0);
/// `sched_nomigrations`: Cpu no migration counter.
pub static SCHED_NOMIGRATIONS: AtomicU64 = AtomicU64::new(0);
/// `sched_noidle`: Times we didn't pick the idle task.
pub static SCHED_NOIDLE: AtomicU64 = AtomicU64::new(0);
/// `sched_stolen`: Times we stole proc from other cpus.
pub static SCHED_STOLEN: AtomicU64 = AtomicU64::new(0);
/// `sched_choose`: Times we chose a cpu.
pub static SCHED_CHOOSE: AtomicU64 = AtomicU64::new(0);
/// `sched_wasidle`: Times we came out of idle.
pub static SCHED_WASIDLE: AtomicU64 = AtomicU64::new(0);

/// `sched_blockcpu` (`__HAVE_CPU_TOPOLOGY`): types of cpu to not schedule on (`CPUTYP_*`).
pub static SCHED_BLOCKCPU: AtomicI32 = AtomicI32::new(0);

/*
 * A few notes about cpu_switchto that is implemented in MD code.
 *
 * cpu_switchto takes two arguments, the old proc and the proc
 * it should switch to. The new proc will never be NULL, so we always have
 * a saved state that we need to switch to. The old proc however can
 * be NULL if the process is exiting. NULL for the old proc simply
 * means "don't bother saving old state".
 *
 * cpu_switchto is supposed to atomically load the new state of the process
 * including the pcb, pmap and setting curproc, the p_cpu pointer in the
 * proc and p_stat to SONPROC. Atomically with respect to interrupts, other
 * cpus in the system must not depend on this state being consistent.
 * Therefore no locking is necessary in cpu_switchto other than blocking
 * interrupts during the context switch.
 */

/// `sched_init`: called in `main()` before calling `sched_init_cpu(curcpu())`. Setup the
/// bare minimum to allow things like `setrunqueue()` to work even before the scheduler is
/// actually started.
pub fn sched_init() {
    cpuset_add(&SCHED_ALL_CPUS, curcpu());
}

/// `sched_init_cpu`: called from `main()` for the boot cpu, then it's the responsibility
/// of the MD code (`cpu_attach`) to call it for all other cpus. The CPU's idle thread is
/// created through the deferred kthread queue (at once once that queue has run).
pub fn sched_init_cpu(ci: &'static CpuInfo) {
    let spc = Machine::ci_schedstate(ci);

    for q in spc.spc_qs.iter() {
        q.init();
    }

    spc.spc_idleproc.set(ptr::null());

    clockintr_bind(&spc.spc_itimer, ci, itimer_update, ptr::null_mut());
    clockintr_bind(&spc.spc_profclock, ci, profclock, ptr::null_mut());
    clockintr_bind(&spc.spc_roundrobin, ci, roundrobin, ptr::null_mut());
    clockintr_bind(&spc.spc_statclock, ci, statclock, ptr::null_mut());

    kthread_create_deferred(sched_kthreads_create, ptr::from_ref(ci).cast_mut().cast());

    spc.spc_deadproc.init();
    spc.spc_deferred.init();

    // Slight hack here until the cpuset code handles cpu_info structures.
    cpuset_init_cpu(ci);
}

/// `sched_kthreads_create`: creates the CPU's idle thread.
pub fn sched_kthreads_create(v: *mut c_void) {
    static NUM: AtomicI32 = AtomicI32::new(0);

    // SAFETY: `sched_init_cpu` queued this with its `cpu_info`, a static.
    let ci: &'static CpuInfo = unsafe { &*v.cast::<CpuInfo>() };
    let spc = Machine::ci_schedstate(ci);

    let Ok(p) = fork1(
        &PROC0,
        FORK_SHAREVM | FORK_SHAREFILES | FORK_NOZOMBIE | FORK_SYSTEM | FORK_IDLE,
        sched_idle,
        ptr::from_ref(ci).cast_mut().cast(),
    ) else {
        panic(format_args!("fork idle"));
    };
    spc.spc_idleproc.set(p);

    // Name it as specified.
    let num = NUM.fetch_add(1, Ordering::Relaxed);
    let mut comm = [0u8; _MAXCOMLEN];
    let n = snprintf(&mut comm, format_args!("idle{num}")).min(_MAXCOMLEN - 1);
    p.process().set_comm(&comm[..n]);
}

/// `sched_idle`: the idle thread of a CPU. It runs without the kernel lock.
pub fn sched_idle(v: *mut c_void) {
    let Some(p) = curproc() else {
        panic(format_args!("sched_idle: no curproc"));
    };
    // SAFETY: `fork1` was given the CPU's static `cpu_info` as the argument.
    let ci: &'static CpuInfo = unsafe { &*v.cast::<CpuInfo>() };

    kernel_unlock(); // KERNEL_UNLOCK()

    // The idle thread is setup in fork1(). When the CPU hatches we enter here for the first
    // time. The CPU is now ready to take work and so add it to sched_all_cpus when
    // appropriate. After that just go away and properly reenter once idle.
    if Machine::ci_cputype(ci) & SCHED_BLOCKCPU.load(Ordering::Relaxed) == 0 {
        cpuset_add(&SCHED_ALL_CPUS, ci);
    }
    let spc = Machine::ci_schedstate(ci);

    kassert!(ptr::eq(ci, curcpu()));
    kassert!(ptr::eq(p, spc.spc_idleproc.get()));
    kassert!(ptr::eq(p.p_cpu.get(), ci));

    sched_lock();
    p.p_stat.set(SSLEEP);
    mi_switch();

    loop {
        while spc.spc_whichqs.load(Ordering::Relaxed) != 0 {
            sched_lock();
            p.p_stat.set(SSLEEP);
            mi_switch();

            while let Some(dead) = spc.spc_deadproc.first() {
                // SAFETY: `dead` is on this CPU's dead list, which only this thread empties.
                unsafe { spc.spc_deadproc.remove(dead) };
                exit2(dead);
            }
        }

        splassert(IPL_NONE, "sched_idle");

        smr_idle();

        cpuset_add(&SCHED_IDLE_CPUS, ci);
        Machine::cpu_idle_enter();
        while spc.spc_whichqs.load(Ordering::Relaxed) == 0 {
            #[cfg(feature = "multiprocessor")]
            {
                let flags = spc.spc_schedflags.load(Ordering::Relaxed);
                if flags & SPCF_SHOULDHALT != 0 && flags & SPCF_HALTED == 0 {
                    cpuset_del(&SCHED_IDLE_CPUS, ci);
                    sched_lock();
                    let halted = if spc.spc_whichqs.load(Ordering::Relaxed) != 0 {
                        0
                    } else {
                        SPCF_HALTED
                    };
                    spc.spc_schedflags.fetch_or(halted, Ordering::Relaxed);
                    sched_unlock();
                    wakeup(ptr::from_ref(spc));
                }
            }
            Machine::cpu_idle_cycle();
        }
        Machine::cpu_idle_leave();
        cpuset_del(&SCHED_IDLE_CPUS, ci);
    }
}

/// `sched_exit`: to free our address space we have to jump through a few hoops. The freeing
/// is done by the reaper, but until we have one reaper per cpu, we have no way of putting
/// this proc on the deadproc list and waking up the reaper without risking having our
/// address space and stack torn from under us before we manage to switch to another proc.
/// Therefore we have a per-cpu list of dead processes where we put this proc and have idle
/// clean up that list and move it to the reaper list.
pub fn sched_exit(p: &Proc) -> ! {
    let spc = Machine::ci_schedstate(curcpu());

    // SAFETY: `p` is the exiting thread; it stays allocated until `exit2` frees it from the
    // dead list.
    unsafe { spc.spc_deadproc.insert_tail(p) };

    tuagg_add_runtime();

    #[cfg(feature = "multiprocessor")]
    kernel_assert_locked(); // KERNEL_ASSERT_LOCKED()
    sched_toidle()
}

/// `sched_toidle`: switches to the idle thread without saving the current context (the
/// current thread is dead).
pub fn sched_toidle() -> ! {
    let spc = Machine::ci_schedstate(curcpu());

    // This process no longer needs to hold the kernel lock.
    #[cfg(feature = "multiprocessor")]
    if _kernel_lock_held() {
        let _ = __mp_release_all(&KERNEL_LOCK);
    }

    if spc.spc_schedflags.load(Ordering::Relaxed) & SPCF_ITIMER != 0 {
        spc.spc_schedflags
            .fetch_and(!SPCF_ITIMER, Ordering::Relaxed);
        clockintr_cancel(&spc.spc_itimer);
    }
    if spc.spc_schedflags.load(Ordering::Relaxed) & SPCF_PROFCLOCK != 0 {
        spc.spc_schedflags
            .fetch_and(!SPCF_PROFCLOCK, Ordering::Relaxed);
        clockintr_cancel(&spc.spc_profclock);
    }

    spc.spc_schedflags
        .fetch_and(!SPCF_SWITCHCLEAR, Ordering::Relaxed);

    sched_lock();
    // SAFETY: `sched_kthreads_create` set the CPU's idle thread, a thread that never exits.
    let Some(idle) = (unsafe { spc.spc_idleproc.get().as_ref() }) else {
        panic(format_args!("sched_toidle: no idleproc"));
    };
    idle.p_stat.set(SRUN);

    UVMEXP.swtch.fetch_add(1, Ordering::Relaxed);
    // TRACEPOINT(sched, off__cpu, ...): dt(4), not configured.
    // SAFETY: `idle` is runnable and never on a queue; the dead thread's context is not
    // saved, as the C's NULL old proc asks.
    unsafe { Machine::cpu_switchto(None, idle) };
    panic(format_args!("cpu_switchto returned"));
}

/// `setrunqueue`: puts `p` on `ci`'s run queue for `prio` (`ci` `None`: `sched_choosecpu`).
pub fn setrunqueue(ci: Option<&'static CpuInfo>, p: &Proc, prio: u8) {
    let queue = usize::from(prio >> 2);

    let ci = match ci {
        Some(ci) => ci,
        None => sched_choosecpu(p),
    };

    sched_assert_locked();
    kassert!(p.p_wchan.get().is_null());
    kassert!(p.p_flag.load(Ordering::Relaxed) & P_INSCHED == 0);

    p.p_cpu.set(ci);
    p.p_stat.set(SRUN);
    p.p_runpri.set(prio);

    let spc = Machine::ci_schedstate(ci);
    spc.spc_nrun.fetch_add(1, Ordering::Relaxed);
    // TRACEPOINT(sched, enqueue, ...): dt(4), not configured.

    // SAFETY: `p` is on no run queue (it is being made runnable), under the scheduler lock,
    // and a thread outlives its stay on a run queue.
    unsafe { spc.spc_qs[queue].insert_tail(p) };
    spc.spc_whichqs.fetch_or(1 << queue, Ordering::Relaxed);
    cpuset_add(&SCHED_QUEUED_CPUS, ci);

    if cpuset_isset(&SCHED_IDLE_CPUS, ci) {
        Machine::cpu_unidle(ci);
    } else if prio < spc.spc_curpriority.load(Ordering::Relaxed) {
        need_resched(ci);
    }
}

/// `remrunqueue`: takes `p` off its run queue.
pub fn remrunqueue(p: &Proc) {
    let queue = usize::from(p.p_runpri.get() >> 2);

    sched_assert_locked();
    let Some(ci) = p.cpu() else {
        panic(format_args!(
            "remrunqueue: thread {} has no CPU",
            p.p_tid.get()
        ));
    };
    let spc = Machine::ci_schedstate(ci);
    spc.spc_nrun.fetch_sub(1, Ordering::Relaxed);
    // TRACEPOINT(sched, dequeue, ...): dt(4), not configured.

    // SAFETY: `p` is SRUN on `queue` of its CPU, under the scheduler lock.
    unsafe { spc.spc_qs[queue].remove(p) };
    if spc.spc_qs[queue].is_empty() {
        spc.spc_whichqs.fetch_and(!(1 << queue), Ordering::Relaxed);
        if spc.spc_whichqs.load(Ordering::Relaxed) == 0 {
            cpuset_del(&SCHED_QUEUED_CPUS, ci);
        }
    }
}

/// `sched_chooseproc`: picks the next thread to run on this CPU: the head of the highest
/// priority non-empty run queue, else a stolen one, else idle. A CPU asked to halt
/// (`SPCF_SHOULDHALT`) first hands its queued threads to other CPUs.
pub fn sched_chooseproc() -> &'static Proc {
    let ci = curcpu();
    let spc = Machine::ci_schedstate(ci);

    sched_assert_locked();

    #[cfg(feature = "multiprocessor")]
    if spc.spc_schedflags.load(Ordering::Relaxed) & SPCF_SHOULDHALT != 0 {
        let mut again = false;
        if spc.spc_whichqs.load(Ordering::Relaxed) != 0 {
            'queues: for q in spc.spc_qs.iter() {
                while let Some(p) = q.first() {
                    remrunqueue(p);
                    setrunqueue(None, p, p.p_runpri.get());
                    if ptr::eq(p.p_cpu.get(), curcpu()) {
                        kassert!(p.p_flag.load(Ordering::Relaxed) & P_CPUPEG != 0);
                        again = true; // goto again
                        break 'queues;
                    }
                }
            }
        }
        if !again {
            let p = idleproc(ci);
            p.p_stat.set(SRUN);
            kassert!(p.p_wchan.get().is_null());
            return p;
        }
    }
    // again:

    let whichqs = spc.spc_whichqs.load(Ordering::Relaxed);
    let p: &'static Proc = if whichqs != 0 {
        let queue = whichqs.trailing_zeros() as usize; // ffs() - 1
        let Some(p) = spc.spc_qs[queue].first() else {
            panic(format_args!(
                "sched_chooseproc: queue {queue} flagged but empty"
            ));
        };
        remrunqueue(p);
        SCHED_NOIDLE.fetch_add(1, Ordering::Relaxed);
        if p.p_stat.get() != SRUN {
            panic(format_args!(
                "thread {} not in SRUN: {}",
                p.p_tid.get(),
                p.p_stat.get()
            ));
        }
        p
    } else if let Some(p) = sched_steal_proc(ci) {
        p
    } else {
        let p = idleproc(ci);
        p.p_stat.set(SRUN);
        p
    };

    kassert!(p.p_wchan.get().is_null());
    kassert!(p.p_flag.load(Ordering::Relaxed) & P_INSCHED == 0);
    p
}

/// `spc->spc_idleproc`, which `sched_chooseproc` panics without.
fn idleproc(ci: &CpuInfo) -> &'static Proc {
    // SAFETY: `sched_kthreads_create` set the CPU's idle thread, a thread that never exits.
    let Some(p) = (unsafe { Machine::ci_schedstate(ci).spc_idleproc.get().as_ref() }) else {
        panic(format_args!(
            "no idleproc set on CPU{}",
            Machine::cpu_info_unit(ci)
        ));
    };
    p
}

/// `sched_choosecpu_fork`: the CPU a new thread starts on. Without `MULTIPROCESSOR`: this
/// one.
pub fn sched_choosecpu_fork(_parent: &Proc, _flags: i32) -> &'static CpuInfo {
    #[cfg(feature = "multiprocessor")]
    {
        // `#if 0` in the C: PPWAIT forks could take the parent's cpu, once exec can move a
        // thread to another cpu painlessly.

        // Look at all cpus that are currently idle and have nothing queued. If there are
        // none, pick the one with least queued procs first, then the one with lowest load
        // average.
        let set = cpuset_complement(&SCHED_QUEUED_CPUS, &SCHED_IDLE_CPUS);
        let mut set = cpuset_intersection(&set, &SCHED_ALL_CPUS);
        if cpuset_first(&set).is_none() {
            set = cpuset_copy(&SCHED_ALL_CPUS);
        }

        let mut choice: Option<&'static CpuInfo> = None;
        let mut best_run = u32::MAX;
        while let Some(ci) = cpuset_first(&set) {
            cpuset_del(&set, ci);

            let run = Machine::ci_schedstate(ci).spc_nrun.load(Ordering::Relaxed);

            if choice.is_none() || run < best_run {
                choice = Some(ci);
                best_run = run;
            }
        }

        if let Some(choice) = choice {
            return choice;
        }
    }
    curcpu()
}

/// `sched_choosecpu`: the CPU to run `p` on. Without `MULTIPROCESSOR`: this one.
pub fn sched_choosecpu(p: &Proc) -> &'static CpuInfo {
    #[cfg(feature = "multiprocessor")]
    {
        // If pegged to a cpu, don't allow it to move.
        if p.p_flag.load(Ordering::Relaxed) & P_CPUPEG != 0
            && let Some(ci) = p.cpu()
        {
            return ci;
        }

        SCHED_CHOOSE.fetch_add(1, Ordering::Relaxed);

        // Look at all cpus that are currently idle and have nothing queued. If there are
        // none, pick the cheapest of those. (idle + queued could mean that the cpu is
        // handling an interrupt at this moment and haven't had time to leave idle yet).
        let set = cpuset_complement(&SCHED_QUEUED_CPUS, &SCHED_IDLE_CPUS);
        let mut set = cpuset_intersection(&set, &SCHED_ALL_CPUS);

        // First, just check if our current cpu is in that set, if it is, this is simple.
        // Also, our cpu might not be idle, but if it's the current cpu and it has nothing
        // else queued and we're curproc, take it.
        if let Some(pcpu) = p.cpu() {
            let pspc = Machine::ci_schedstate(pcpu);
            if cpuset_isset(&set, pcpu)
                || (ptr::eq(pcpu, curcpu())
                    && pspc.spc_nrun.load(Ordering::Relaxed) == 0
                    && pspc.spc_schedflags.load(Ordering::Relaxed) & SPCF_SHOULDHALT == 0
                    && curproc().is_some_and(|cur| ptr::eq(cur, p)))
            {
                SCHED_WASIDLE.fetch_add(1, Ordering::Relaxed);
                return pcpu;
            }
        }

        if cpuset_first(&set).is_none() {
            set = cpuset_copy(&SCHED_ALL_CPUS);
        }

        let mut choice: Option<&'static CpuInfo> = None;
        let mut last_cost = i32::MAX;
        while let Some(ci) = cpuset_first(&set) {
            let cost = sched_proc_to_cpu_cost(ci, p);

            if choice.is_none() || cost < last_cost {
                choice = Some(ci);
                last_cost = cost;
            }
            cpuset_del(&set, ci);
        }

        if choice.is_some_and(|c| ptr::eq(p.p_cpu.get(), c)) {
            SCHED_NOMIGRATIONS.fetch_add(1, Ordering::Relaxed);
        } else {
            SCHED_NMIGRATIONS.fetch_add(1, Ordering::Relaxed);
        }

        if let Some(choice) = choice {
            return choice;
        }
    }
    let _ = p;
    curcpu()
}

/// `sched_steal_proc`: attempt to steal a proc from some cpu: the cheapest unpegged thread
/// at the head priority of a CPU with queued threads. Without `MULTIPROCESSOR` there is
/// nobody to steal from.
pub fn sched_steal_proc(self_: &'static CpuInfo) -> Option<&'static Proc> {
    #[cfg(feature = "multiprocessor")]
    {
        kassert!(
            Machine::ci_schedstate(self_)
                .spc_schedflags
                .load(Ordering::Relaxed)
                & SPCF_SHOULDHALT
                == 0
        );

        // Don't steal if we don't want to schedule processes in this CPU.
        if !cpuset_isset(&SCHED_ALL_CPUS, self_) {
            return None;
        }

        let set = cpuset_copy(&SCHED_QUEUED_CPUS);

        let mut best: Option<&'static Proc> = None;
        let mut bestcost = i32::MAX;
        while let Some(ci) = cpuset_first(&set) {
            cpuset_del(&set, ci);

            let spc = Machine::ci_schedstate(ci);

            let whichqs = spc.spc_whichqs.load(Ordering::Relaxed);
            if whichqs == 0 {
                continue;
            }
            let queue = whichqs.trailing_zeros() as usize; // ffs() - 1
            for p in spc.spc_qs[queue].iter() {
                if p.p_flag.load(Ordering::Relaxed) & P_CPUPEG != 0 {
                    continue;
                }

                let cost = sched_proc_to_cpu_cost(self_, p);

                if best.is_none() || cost < bestcost {
                    best = Some(p);
                    bestcost = cost;
                }
            }
        }
        let best = best?;

        // TRACEPOINT(sched, steal, ...): dt(4), not configured.

        remrunqueue(best);
        best.p_cpu.set(self_);

        SCHED_STOLEN.fetch_add(1, Ordering::Relaxed);
        Some(best)
    }
    #[cfg(not(feature = "multiprocessor"))]
    {
        let _ = self_;
        None
    }
}

/// `log2`: base 2 logarithm of an int. returns 0 for 0 (yeye, I know).
#[cfg(any(feature = "multiprocessor", test))]
fn log2(mut i: u32) -> i32 {
    let mut ret = 0;

    loop {
        i >>= 1;
        if i == 0 {
            break;
        }
        ret += 1;
    }

    ret
}

/*
 * Calculate the cost of moving the proc to this cpu.
 *
 * What we want is some guesstimate of how much "performance" it will
 * cost us to move the proc here. Not just for caches and TLBs and NUMA
 * memory, but also for the proc itself. A highly loaded cpu might not
 * be the best candidate for this proc since it won't get run.
 *
 * Just total guesstimates for now.
 */

/// `sched_cost_priority`.
#[cfg(any(feature = "multiprocessor", test))]
pub static SCHED_COST_PRIORITY: AtomicI32 = AtomicI32::new(1);
/// `sched_cost_runnable`.
#[cfg(any(feature = "multiprocessor", test))]
pub static SCHED_COST_RUNNABLE: AtomicI32 = AtomicI32::new(3);
/// `sched_cost_resident`.
#[cfg(any(feature = "multiprocessor", test))]
pub static SCHED_COST_RESIDENT: AtomicI32 = AtomicI32::new(1);

/// What `sched_proc_to_cpu_cost` weighs, read from the CPU and the thread.
#[cfg(any(feature = "multiprocessor", test))]
#[derive(Clone, Copy, Debug)]
struct CpuCostInputs {
    /// The CPU is in `sched_idle_cpus`.
    idle: bool,
    /// The CPU is in `sched_queued_cpus`.
    queued: bool,
    /// `CPU_IS_PRIMARY(ci)`: it takes the hardware interrupts.
    primary: bool,
    /// `p->p_usrpri`.
    usrpri: u8,
    /// `spc->spc_curpriority`.
    curpriority: u8,
    /// `spc->spc_nrun`.
    nrun: u32,
    /// `pmap_resident_count` of the thread's map when it last ran on this CPU and has not
    /// slept since (`p->p_cpu == ci && p->p_slptime == 0`).
    resident: Option<i64>,
}

/// The arithmetic of `sched_proc_to_cpu_cost`.
#[cfg(any(feature = "multiprocessor", test))]
fn cpu_cost(c: CpuCostInputs) -> i32 {
    let priority = SCHED_COST_PRIORITY.load(Ordering::Relaxed);
    let runnable = SCHED_COST_RUNNABLE.load(Ordering::Relaxed);
    let resident = SCHED_COST_RESIDENT.load(Ordering::Relaxed);
    let mut cost = 0i32;

    // First, account for the priority of the proc we want to move. More willing to move,
    // the lower the priority of the destination and the higher the priority of the proc.
    if !c.idle {
        cost += (i32::from(c.usrpri) - i32::from(c.curpriority)) * priority;
        cost += runnable;
    }
    if c.queued {
        cost += c.nrun as i32 * runnable;
    }

    // Try to avoid the primary cpu as it handles hardware interrupts.
    //
    // XXX Needs to be revisited when we distribute interrupts over cpus.
    if c.primary {
        cost += runnable;
    }

    // If the proc is on this cpu already, lower the cost by how much it has been running and
    // an estimate of its footprint.
    if let Some(count) = c.resident {
        let l2resident = log2(count as u32);
        cost -= l2resident * resident;
    }

    cost
}

/// `sched_proc_to_cpu_cost`: calculate the cost of moving the proc to this cpu. Without
/// `MULTIPROCESSOR` every move is free.
pub fn sched_proc_to_cpu_cost(ci: &CpuInfo, p: &Proc) -> i32 {
    #[cfg(feature = "multiprocessor")]
    {
        let spc = Machine::ci_schedstate(ci);
        let resident =
            (ptr::eq(p.p_cpu.get(), ci) && p.p_slptime.get() == 0 && !p.p_vmspace.get().is_null())
                .then(|| pmap_resident_count(p.vmspace().vm_map.pmap()));
        cpu_cost(CpuCostInputs {
            idle: cpuset_isset(&SCHED_IDLE_CPUS, ci),
            queued: cpuset_isset(&SCHED_QUEUED_CPUS, ci),
            primary: Machine::cpu_is_primary(ci),
            usrpri: p.p_usrpri.get(),
            curpriority: spc.spc_curpriority.load(Ordering::Relaxed),
            nrun: spc.spc_nrun.load(Ordering::Relaxed),
            resident,
        })
    }
    #[cfg(not(feature = "multiprocessor"))]
    {
        let _ = (ci, p);
        0
    }
}

/// `sched_peg_curproc`: peg a proc to a cpu.
pub fn sched_peg_curproc(ci: &'static CpuInfo) {
    let Some(p) = curproc() else {
        panic(format_args!("sched_peg_curproc: no curproc"));
    };

    sched_lock();
    p.p_flag.fetch_or(P_CPUPEG, Ordering::Relaxed);
    setrunqueue(Some(ci), p, p.p_usrpri.get());
    p.p_ru.ru_nvcsw.set(p.p_ru.ru_nvcsw.get() + 1);
    mi_switch();
}

/// `sched_unpeg_curproc`.
pub fn sched_unpeg_curproc() {
    let Some(p) = curproc() else {
        panic(format_args!("sched_unpeg_curproc: no curproc"));
    };

    p.p_flag.fetch_and(!P_CPUPEG, Ordering::Relaxed);
}

/// `sched_start_secondary_cpus`: lets the running secondary CPUs take threads again.
#[cfg(feature = "multiprocessor")]
pub fn sched_start_secondary_cpus() {
    cpu_info_foreach(&mut |ci| {
        let spc = Machine::ci_schedstate(ci);

        if Machine::cpu_is_primary(ci) || !cpu_is_running(ci) {
            return;
        }
        spc.spc_schedflags
            .fetch_and(!(SPCF_SHOULDHALT | SPCF_HALTED), Ordering::Relaxed);
        if Machine::ci_cputype(ci) & SCHED_BLOCKCPU.load(Ordering::Relaxed) != 0 {
            return;
        }
        cpuset_add(&SCHED_ALL_CPUS, ci);
    });
}

/// `sched_stop_secondary_cpus`: makes sure we stop the secondary CPUs: each hands its
/// threads over and parks in its idle loop (`SPCF_HALTED`); returns once all have.
#[cfg(feature = "multiprocessor")]
pub fn sched_stop_secondary_cpus() {
    // Make sure we stop the secondary CPUs.
    cpu_info_foreach(&mut |ci| {
        let spc = Machine::ci_schedstate(ci);

        if Machine::cpu_is_primary(ci) || !cpu_is_running(ci) {
            return;
        }
        cpuset_del(&SCHED_ALL_CPUS, ci);
        spc.spc_schedflags
            .fetch_or(SPCF_SHOULDHALT, Ordering::Relaxed);
    });
    cpu_info_foreach(&mut |ci| {
        let spc = Machine::ci_schedstate(ci);

        if Machine::cpu_is_primary(ci) || !cpu_is_running(ci) {
            return;
        }
        let halted = || spc.spc_schedflags.load(Ordering::Relaxed) & SPCF_HALTED != 0;
        while !halted() {
            sleep_setup(ptr::from_ref(spc).cast(), PZERO, "schedstate");
            let _ = sleep_finish(INFSLP, !halted());
        }
    });
}

/// `struct sched_barrier_state`.
#[cfg(feature = "multiprocessor")]
struct SchedBarrierState {
    /// `ci`: the CPU to pass through.
    ci: &'static CpuInfo,
    /// `cond`: signalled from that CPU.
    cond: Cond,
}

/// `sched_barrier_task`: runs on `systqmp`, moves itself to the barrier's CPU and signals.
#[cfg(feature = "multiprocessor")]
pub fn sched_barrier_task(arg: *mut c_void) {
    // SAFETY: `sched_barrier` queued this with its state, which it keeps on its stack until
    // the cond below is signalled.
    let sb = unsafe { &*arg.cast::<SchedBarrierState>() };
    let ci = sb.ci;

    sched_peg_curproc(ci);
    cond_signal(&sb.cond);
    sched_unpeg_curproc();
}

/// `sched_barrier`: returns once `ci` (the primary CPU for `None`) has gone through the
/// scheduler, i.e. every interrupt handler it was running has finished.
#[cfg(feature = "multiprocessor")]
pub fn sched_barrier(ci: Option<&'static CpuInfo>) {
    let ci = match ci {
        Some(ci) => ci,
        None => {
            let mut primary = None;
            cpu_info_foreach(&mut |ci| {
                if primary.is_none() && Machine::cpu_is_primary(ci) {
                    primary = Some(ci);
                }
            });
            let Some(primary) = primary else {
                panic(format_args!("sched_barrier: no primary cpu"));
            };
            primary
        }
    };

    if ptr::eq(ci, curcpu()) {
        return;
    }

    let sb = SchedBarrierState {
        ci,
        cond: Cond::new(),
    };
    cond_init(&sb.cond);
    let task = Task::new(sched_barrier_task, ptr::from_ref(&sb).cast_mut().cast());

    // SAFETY: `task` and `sb` stay on this frame until `cond_wait` returns, which is after
    // the worker took the task off the queue and ran it up to its `cond_signal`.
    let _ = unsafe { task_add_local(SYSTQMP, &task) };
    cond_wait(&sb.cond, "sbar");
}

/// `sched_barrier`: without `MULTIPROCESSOR`, nothing to wait for.
#[cfg(not(feature = "multiprocessor"))]
pub fn sched_barrier(_ci: Option<&'static CpuInfo>) {}

/*
 * Functions to manipulate cpu sets.
 */

/// `cpuset_infos[MAXCPUS]`, made `Sync`: each slot is filled by `cpuset_init_cpu` while its
/// CPU attaches, before that CPU joins any set; read-only afterwards.
struct CpusetInfos([core::cell::Cell<*const CpuInfo>; MAXCPUS as usize]);
// SAFETY: see the type's doc.
unsafe impl Sync for CpusetInfos {}

/// `cpuset_infos`.
static CPUSET_INFOS: CpusetInfos =
    CpusetInfos([const { core::cell::Cell::new(ptr::null()) }; MAXCPUS as usize]);

/// `cpuset_init_cpu`.
pub fn cpuset_init_cpu(ci: &'static CpuInfo) {
    CPUSET_INFOS.0[Machine::cpu_info_unit(ci) as usize].set(ci);
}

/// `cpuset_add`.
pub fn cpuset_add(cs: &Cpuset, ci: &CpuInfo) {
    let num = Machine::cpu_info_unit(ci) as usize;
    cs.cs_set[num / 32].fetch_or(1 << (num % 32), Ordering::Relaxed);
}

/// `cpuset_del`.
pub fn cpuset_del(cs: &Cpuset, ci: &CpuInfo) {
    let num = Machine::cpu_info_unit(ci) as usize;
    cs.cs_set[num / 32].fetch_and(!(1 << (num % 32)), Ordering::Relaxed);
}

/// `cpuset_isset`.
pub fn cpuset_isset(cs: &Cpuset, ci: &CpuInfo) -> bool {
    let num = Machine::cpu_info_unit(ci) as usize;
    cs.cs_set[num / 32].load(Ordering::Relaxed) & (1 << (num % 32)) != 0
}

/// `cpuset_copy`: a copy of `from`.
pub fn cpuset_copy(from: &Cpuset) -> Cpuset {
    let to = Cpuset::new();
    for (t, f) in to.cs_set.iter().zip(from.cs_set.iter()) {
        t.store(f.load(Ordering::Relaxed), Ordering::Relaxed);
    }
    to
}

/// `CPUSET_ASIZE(ncpus)`: the words in use.
fn cpuset_words() -> usize {
    cpuset_asize(NCPUS.load(Ordering::Relaxed).max(1) as u32)
}

/// `cpuset_first`: the lowest numbered CPU in the set.
pub fn cpuset_first(cs: &Cpuset) -> Option<&'static CpuInfo> {
    for (i, word) in cs.cs_set.iter().enumerate().take(cpuset_words()) {
        let bits = word.load(Ordering::Relaxed);
        if bits != 0 {
            let ci = CPUSET_INFOS.0[i * 32 + bits.trailing_zeros() as usize].get();
            // SAFETY: `cpuset_init_cpu` stored a static `cpu_info` for every CPU in a set.
            return unsafe { ci.as_ref() };
        }
    }
    None
}

/// `cpuset_intersection`: `a & b`.
pub fn cpuset_intersection(a: &Cpuset, b: &Cpuset) -> Cpuset {
    let to = Cpuset::new();
    for i in 0..cpuset_words() {
        to.cs_set[i].store(
            a.cs_set[i].load(Ordering::Relaxed) & b.cs_set[i].load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
    }
    to
}

/// `cpuset_complement`: `b & ~a`.
pub fn cpuset_complement(a: &Cpuset, b: &Cpuset) -> Cpuset {
    let to = Cpuset::new();
    for i in 0..cpuset_words() {
        to.cs_set[i].store(
            b.cs_set[i].load(Ordering::Relaxed) & !a.cs_set[i].load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
    }
    to
}

/// `cpuset_cardinality`: the number of CPUs in the set.
pub fn cpuset_cardinality(cs: &Cpuset) -> u32 {
    cs.cs_set
        .iter()
        .take(cpuset_words())
        .map(|w| w.load(Ordering::Relaxed).count_ones())
        .sum()
}

/// `sysctl_hwncpuonline`.
pub fn sysctl_hwncpuonline() -> u32 {
    cpuset_cardinality(&SCHED_ALL_CPUS)
}

/// `cpu_is_online`.
pub fn cpu_is_online(ci: &CpuInfo) -> bool {
    cpuset_isset(&SCHED_ALL_CPUS, ci)
}

/// `sched_cpuadjust` (`__HAVE_CPU_TOPOLOGY`): blocks the secondary CPUs whose type is in
/// `newblockcpu` and lets the others schedule again.
pub fn sched_cpuadjust(newblockcpu: i32) -> Result<(), Errno> {
    if newblockcpu == SCHED_BLOCKCPU.load(Ordering::Relaxed) {
        return Ok(());
    }
    SCHED_BLOCKCPU.store(newblockcpu, Ordering::Relaxed);
    cpu_info_foreach(&mut |ci| {
        if Machine::cpu_is_primary(ci) || !Machine::cpu_is_running(ci) {
            return;
        }
        let inset = cpuset_isset(&SCHED_ALL_CPUS, ci);
        if Machine::ci_cputype(ci) & newblockcpu != 0 {
            if inset {
                cpuset_del(&SCHED_ALL_CPUS, ci);
            }
        } else if !inset {
            cpuset_add(&SCHED_ALL_CPUS, ci);
        }
    });
    Ok(())
}

/// `sysctl_hwsmt`: emulate hw.smt temporarily.
pub fn sysctl_hwsmt(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let newsmt = AtomicI32::new(1);

    if SCHED_BLOCKCPU.load(Ordering::Relaxed) & CPUTYP_SMT != 0 {
        newsmt.store(0, Ordering::Relaxed);
    }
    sysctl_int_bounded(oldp, oldlenp, newp, newlen, &newsmt, 0, 1)?;
    if newp == 0 {
        return Ok(());
    }
    let mut newblockcpu = SCHED_BLOCKCPU.load(Ordering::Relaxed);
    if newsmt.load(Ordering::Relaxed) != 0 {
        newblockcpu &= !CPUTYP_SMT;
    } else {
        newblockcpu |= CPUTYP_SMT;
    }
    sched_cpuadjust(newblockcpu)
}

/// The `hw.blockcpu` letters, in the C's order.
const BLOCKCPU_LETTERS: [(u8, i32); 4] = [
    (b'S', CPUTYP_SMT),
    (b'P', CPUTYP_P),
    (b'E', CPUTYP_E),
    (b'L', CPUTYP_L),
];

/// `sysctl_hwblockcpu`: the CPU types not to schedule on, as letters (`S`, `P`, `E`, `L`).
pub fn sysctl_hwblockcpu(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let mut type_ = [0u8; 8];
    let mut n = 0;
    let blockcpu = SCHED_BLOCKCPU.load(Ordering::Relaxed);

    for (letter, typ) in BLOCKCPU_LETTERS {
        if blockcpu & typ != 0 {
            type_[n] = letter;
            n += 1;
        }
    }
    if newp == 0 {
        return sysctl_rdstring(oldp, oldlenp, newp, &type_);
    }

    sysctl_string(oldp, oldlenp, newp, newlen, &mut type_)?;
    let mut newblockcpu = 0;
    for &c in type_.iter().take_while(|&&c| c != 0) {
        let Some(&(_, typ)) = BLOCKCPU_LETTERS.iter().find(|(l, _)| *l == c) else {
            return Err(Errno::EINVAL);
        };
        newblockcpu |= typ;
    }
    sched_cpuadjust(newblockcpu)
}

const _: () = assert!(
    SCHED_NQS == 32,
    "setrunqueue indexes the queues by prio >> 2"
);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of `kern/kern_sched.rs`: the CPU sets and the cost estimate.

    use super::*;

    #[test]
    fn cpusets_track_the_host_cpu() {
        let ci = curcpu();
        let set = Cpuset::new();
        assert!(!cpuset_isset(&set, ci));
        cpuset_add(&set, ci);
        assert!(cpuset_isset(&set, ci));
        assert_eq!(cpuset_cardinality(&set), 1);
        let empty = cpuset_complement(&set, &set);
        assert_eq!(cpuset_cardinality(&empty), 0);
        let both = cpuset_intersection(&set, &cpuset_copy(&set));
        assert_eq!(cpuset_cardinality(&both), 1);
        cpuset_del(&set, ci);
        assert!(!cpuset_isset(&set, ci));
    }

    #[test]
    fn cpuset_first_finds_the_registered_cpu() {
        let ci = curcpu();
        cpuset_init_cpu(ci);
        let set = Cpuset::new();
        assert!(cpuset_first(&set).is_none());
        cpuset_add(&set, ci);
        assert!(cpuset_first(&set).is_some_and(|c| ptr::eq(c, ci)));
    }

    #[test]
    fn log2_rounds_down_and_maps_zero_to_zero() {
        assert_eq!(log2(0), 0);
        assert_eq!(log2(1), 0);
        assert_eq!(log2(2), 1);
        assert_eq!(log2(3), 1);
        assert_eq!(log2(1024), 10);
        assert_eq!(log2(u32::MAX), 31);
    }

    /// A busy, non-primary CPU running priority 50 with nothing queued.
    const BUSY: CpuCostInputs = CpuCostInputs {
        idle: false,
        queued: false,
        primary: false,
        usrpri: 50,
        curpriority: 50,
        nrun: 0,
        resident: None,
    };

    #[test]
    fn an_idle_cpu_costs_nothing() {
        let c = CpuCostInputs { idle: true, ..BUSY };
        assert_eq!(cpu_cost(c), 0);
    }

    #[test]
    fn a_busy_cpu_costs_the_priority_gap_plus_one_runnable() {
        assert_eq!(cpu_cost(BUSY), 3);
        // A thread of worse priority (higher number) than the running one costs more.
        let worse = CpuCostInputs { usrpri: 60, ..BUSY };
        assert_eq!(cpu_cost(worse), 13);
        // A better one less: it would preempt.
        let better = CpuCostInputs { usrpri: 40, ..BUSY };
        assert_eq!(cpu_cost(better), -7);
    }

    #[test]
    fn queued_threads_and_the_primary_cpu_add_cost() {
        let queued = CpuCostInputs {
            queued: true,
            nrun: 2,
            ..BUSY
        };
        assert_eq!(cpu_cost(queued), 3 + 2 * 3);
        let primary = CpuCostInputs {
            primary: true,
            ..BUSY
        };
        assert_eq!(cpu_cost(primary), 3 + 3);
    }

    #[test]
    fn a_warm_cache_lowers_the_cost() {
        let warm = CpuCostInputs {
            resident: Some(256),
            ..BUSY
        };
        assert_eq!(cpu_cost(warm), 3 - 8);
        let empty = CpuCostInputs {
            resident: Some(0),
            ..BUSY
        };
        assert_eq!(cpu_cost(empty), 3);
    }
}
/* </TESTS> */
