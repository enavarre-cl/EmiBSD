/*	$OpenBSD: kern_sched.c,v 1.116 2026/04/09 01:30:02 jsg Exp $	*/
/* <LICENSES> */
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

//! The run queues and CPU selection: `kern/kern_sched.c`.
//!
//! Upstream: sys/kern/kern_sched.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part a) ports `sched_init` and `sched_init_cpu` as far as
//! the clock interrupts: the four `clockintr_bind`s; part b2 adds the rest: the CPU sets
//! (`sched_idle_cpus`, `sched_queued_cpus`, `sched_all_cpus`, `cpuset_*`), the counters,
//! `sched_kthreads_create`, `sched_idle`, `sched_exit`, `sched_toidle`, `setrunqueue`,
//! `remrunqueue`, `sched_chooseproc`, `sched_choosecpu_fork`, `sched_choosecpu`,
//! `sched_steal_proc`, `sched_proc_to_cpu_cost`, `sched_peg_curproc`,
//! `sched_unpeg_curproc`, `sched_barrier`, `sysctl_hwncpuonline` and `cpu_is_online`. The
//! `MULTIPROCESSOR` paths (`SPCF_SHOULDHALT`, the stealing and cost heuristics,
//! `sched_start/stop_secondary_cpus`, the barrier task) and `__HAVE_CPU_TOPOLOGY`
//! (`sched_blockcpu`, `sched_cpuadjust`, `sysctl_hwsmt`, `sysctl_hwblockcpu`) are not
//! configured.
//!
//! ## Deviations
//! - The `cpuset_*` functions with a `to` output return the new set instead.
//! - `smr_idle` (M7) is not called from `sched_idle`.

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
use crate::kern::kern_resource::tuagg_add_runtime;
use crate::kern::kern_time::itimer_update;
use crate::kern::sched_bsd::{mi_switch, roundrobin, sched_assert_locked, sched_lock};
use crate::kern::subr_prf::{panic, snprintf};
use crate::kern::subr_prof::profclock;
use crate::machine::Machine;
use crate::machine::cpu::{Cpu, CpuInfo, MAXCPUS, curcpu, curproc, need_resched};
use crate::machine::intr::{IPL_NONE, splassert};
use crate::sys::proc::{
    _MAXCOMLEN, Cpuset, FORK_IDLE, FORK_NOZOMBIE, FORK_SHAREFILES, FORK_SHAREVM, FORK_SYSTEM,
    P_CPUPEG, P_INSCHED, Proc, SRUN, SSLEEP, cpuset_asize,
};
use crate::sys::sched::{SCHED_NQS, SPCF_ITIMER, SPCF_PROFCLOCK, SPCF_SWITCHCLEAR};
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
/// of the MD code to call it for all other cpus.
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
    // SIMPLEQ_INIT(&spc->spc_deferred): kern_smr.c (M7).

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

/// `sched_idle`: the idle thread of a CPU.
pub fn sched_idle(v: *mut c_void) {
    let Some(p) = curproc() else {
        panic(format_args!("sched_idle: no curproc"));
    };
    // SAFETY: `fork1` was given the CPU's static `cpu_info` as the argument.
    let ci: &'static CpuInfo = unsafe { &*v.cast::<CpuInfo>() };

    // KERNEL_UNLOCK(): nothing without MULTIPROCESSOR.

    // The idle thread is setup in fork1(). When the CPU hatches we enter here for the first
    // time. The CPU is now ready to take work and so add it to sched_all_cpus when
    // appropriate. After that just go away and properly reenter once idle.
    // __HAVE_CPU_TOPOLOGY (ci_cputype & sched_blockcpu): not configured.
    cpuset_add(&SCHED_ALL_CPUS, ci);
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

        // smr_idle(): kern_smr.c (M7).

        cpuset_add(&SCHED_IDLE_CPUS, ci);
        Machine::cpu_idle_enter();
        while spc.spc_whichqs.load(Ordering::Relaxed) == 0 {
            // MULTIPROCESSOR: SPCF_SHOULDHALT / SPCF_HALTED, not configured.
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

    // KERNEL_ASSERT_LOCKED(): nothing without MULTIPROCESSOR.
    sched_toidle()
}

/// `sched_toidle`: switches to the idle thread without saving the current context (the
/// current thread is dead).
pub fn sched_toidle() -> ! {
    let spc = Machine::ci_schedstate(curcpu());

    // MULTIPROCESSOR: this process no longer needs to hold the kernel lock.

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
    spc.spc_nrun.set(spc.spc_nrun.get() + 1);
    // TRACEPOINT(sched, enqueue, ...): dt(4), not configured.

    // SAFETY: `p` is on no run queue (it is being made runnable), under the scheduler lock,
    // and a thread outlives its stay on a run queue.
    unsafe { spc.spc_qs[queue].insert_tail(p) };
    spc.spc_whichqs.fetch_or(1 << queue, Ordering::Relaxed);
    cpuset_add(&SCHED_QUEUED_CPUS, ci);

    if cpuset_isset(&SCHED_IDLE_CPUS, ci) {
        Machine::cpu_unidle(ci);
    } else if prio < spc.spc_curpriority.get() {
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
    spc.spc_nrun.set(spc.spc_nrun.get() - 1);
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
/// priority non-empty run queue, else a stolen one, else idle.
pub fn sched_chooseproc() -> &'static Proc {
    let ci = curcpu();
    let spc = Machine::ci_schedstate(ci);

    sched_assert_locked();

    // MULTIPROCESSOR: SPCF_SHOULDHALT, not configured.

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
        // SAFETY: `sched_kthreads_create` set the CPU's idle thread, a thread that never
        // exits.
        let Some(p) = (unsafe { spc.spc_idleproc.get().as_ref() }) else {
            panic(format_args!(
                "no idleproc set on CPU{}",
                Machine::cpu_info_unit(ci)
            ));
        };
        p.p_stat.set(SRUN);
        p
    };

    kassert!(p.p_wchan.get().is_null());
    kassert!(p.p_flag.load(Ordering::Relaxed) & P_INSCHED == 0);
    p
}

/// `sched_choosecpu_fork`: the CPU a new thread starts on. Without `MULTIPROCESSOR`: this
/// one.
pub fn sched_choosecpu_fork(_parent: &Proc, _flags: i32) -> &'static CpuInfo {
    // MULTIPROCESSOR: look at all cpus that are currently idle and have nothing queued. If
    // there are none, pick the one with least queued procs first, then the one with lowest
    // load average.
    curcpu()
}

/// `sched_choosecpu`: the CPU to run `p` on. Without `MULTIPROCESSOR`: this one.
pub fn sched_choosecpu(_p: &Proc) -> &'static CpuInfo {
    // MULTIPROCESSOR: if pegged to a cpu, don't allow it to move; else the cheapest of the
    // idle CPUs with nothing queued (sched_proc_to_cpu_cost), counting the migrations.
    curcpu()
}

/// `sched_steal_proc`: attempt to steal a proc from some cpu. Without `MULTIPROCESSOR`
/// there is nobody to steal from.
pub fn sched_steal_proc(_self_: &CpuInfo) -> Option<&'static Proc> {
    None
}

/// `sched_proc_to_cpu_cost`: calculate the cost of moving the proc to this cpu. Without
/// `MULTIPROCESSOR` every move is free.
pub fn sched_proc_to_cpu_cost(_ci: &CpuInfo, _p: &Proc) -> i32 {
    0
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

// sched_start_secondary_cpus, sched_stop_secondary_cpus, sched_barrier_task: MULTIPROCESSOR.

/// `sched_barrier`: without `MULTIPROCESSOR`, nothing to wait for.
pub fn sched_barrier(_ci: Option<&CpuInfo>) {}

/*
 * Functions to manipulate cpu sets.
 */

/// `cpuset_infos[MAXCPUS]`, made `Sync`: filled by `cpuset_init_cpu` while a CPU boots.
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

const _: () = assert!(
    SCHED_NQS == 32,
    "setrunqueue indexes the queues by prio >> 2"
);

#[cfg(test)]
mod tests {
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
}
