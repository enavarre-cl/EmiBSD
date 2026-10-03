/*	$OpenBSD: kern_exit.c,v 1.255 2026/09/19 16:29:14 gnezdo Exp $	*/
/*	$NetBSD: kern_exit.c,v 1.39 1996/04/22 01:38:25 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1991, 1993
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
 *	@(#)kern_exit.c	8.7 (Berkeley) 2/12/94
 */
/* </LICENSES> */

//! Process exit: `kern/kern_exit.c`.
//!
//! Upstream: sys/kern/kern_exit.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M6 (part b) ports `sys_exit`, `exit1`, `exit2`, `proc_free`,
//! `reaper`, `process_clear_orphan`, `process_reparent` and `process_zap`: enough for a
//! kernel thread (and soon `init`) to die and be reaped. `sys___threxit`, `dowait6`,
//! `sys_wait4`, `sys_waitid`, `proc_finish_wait` and `process_untrace` come with the
//! syscalls and ptrace (M6-c, M7).
//!
//! ## Deviations
//! - What `exit1` tears down that does not exist yet is reported, each once: the signal
//!   side (`single_thread_set`, `process_suspend_signal`, `sigio_freelist`,
//!   `SAS_NOCLDWAIT`, `prsignal`), `kqpoll_exit`, `stopprofclock`/`prof_write`, `fdfree`,
//!   `cancel_all_itimers`, `killjobc`, `unveil_destroy`, `uvm_purge`, `lim_free`,
//!   `process_untrace`; `process_zap` likewise `leavepgrp`, `vrele`, `sigactsfree`,
//!   `crfree`; the reaper `uvm_exit` and `knote_processexit`.
//! - `initprocess` is null until `init` exists (M6-b): until then process 0 adopts the
//!   orphans `exit1` and `process_reparent` would hand to `init`.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::kassert;
use crate::kern::init_main::{INITPROCESS, PROCESS0};
use crate::kern::kern_fork::{NPROCESSES, NTHREADS, freepid};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_proc::{PROC_POOL, PROCESS_POOL, RUSAGE_POOL, ZOMBPROCESS, chgproccnt};
use crate::kern::kern_resource::{calcru, ruadd, tuagg_add_process, tuagg_add_runtime};
use crate::kern::kern_sched::sched_exit;
use crate::kern::kern_synch::{msleep_nsec, refcnt_finalize, wakeup};
use crate::kern::kern_timeout::timeout_del;
use crate::kern::sched_bsd::sched_assert_unlocked;
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::panic;
#[cfg(feature = "qemu")]
use crate::kprintf;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::machine::intr::IPL_NONE;
use crate::machine::pmap::pmap_deactivate;
#[cfg(feature = "qemu")]
use crate::machine::{Exit, ExitStatus};
use crate::sys::errno::Errno;
use crate::sys::mutex::{MTX_NOWITNESS, Mutex, mutex_assert_locked};
use crate::sys::param::{PVM, PWAIT};
use crate::sys::pool::{PR_WAITOK, PR_ZERO};
use crate::sys::proc::{
    EXIT_NORMAL, P_SYSTEM, P_THREAD, P_WEXIT, PS_EXITING, PS_ISPWAIT, PS_NOZOMBIE, PS_ORPHAN,
    PS_PPWAIT, PS_PROFIL, PS_STOPPING, PS_TRACED, PS_WAITEVENT, PS_ZOMBIE, Proc, ProcHash,
    ProcList, ProcRunq, Process, ProcessHash, ProcessList, ProcessOrphan, ProcessSibling, SDEAD,
    SINGLE_EXIT, p_hassibling,
};
use crate::sys::queue::{ListHead, TailqHead};
use crate::sys::resource::Rusage;
use crate::sys::syscallargs::SysExitArgs;
use crate::sys::systm::{INFSLP, SysArgs, sysargs};
use crate::sys::types::Register;
use crate::unported;
use crate::uvm::uvm_glue::{uvm_exit, uvm_purge, uvm_uarea_free};

/// `sys_exit`: death of process.
pub fn sys_exit(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysExitArgs = sysargs(v);

    exit1(p, uap.rval.get(), 0, EXIT_NORMAL)
    // NOTREACHED
}

// sys___threxit: the thread syscalls (M6-c).

/// `initprocess`, or process 0 while there is no init (see the module's deviations).
fn initprocess_or_process0() -> &'static Process {
    // SAFETY: a non-null `initprocess` is init's process, which never goes away.
    unsafe { INITPROCESS.load(Ordering::Relaxed).as_ref() }.unwrap_or(&PROCESS0)
}

/// `exit1`: deallocate address space and other resources, change proc state to zombie, and
/// unlink proc from allproc and parent's lists. Save exit status and rusage for wait().
/// Check for child processes and orphan them.
pub fn exit1(p: &Proc, xexit: i32, xsig: i32, flags: i32) -> ! {
    let mut flags = flags;

    p.p_flag.fetch_or(P_WEXIT, Ordering::Relaxed);

    let pr = p.process();

    // single-threaded?
    if !p_hassibling(p) {
        flags = EXIT_NORMAL;
    } else {
        // nope, multi-threaded
        if flags == EXIT_NORMAL {
            // single_thread_set(p, SINGLE_EXIT): kern_sig.c (M6-c).
            let _ = unported!("exit1: single_thread_set (M6-c)");
            let _ = SINGLE_EXIT;
        }
    }

    if flags == EXIT_NORMAL && pr.ps_flags.load(Ordering::Relaxed) & PS_EXITING == 0 {
        if pr.ps_pid.get() == 1 {
            // Under QEMU init's exit ends the run: the M6 exit criterion (see the module's
            // deviations). The C panics, as it has nothing to run.
            #[cfg(feature = "qemu")]
            {
                kprintf!("init exited with status {xexit} (signal {xsig})\n");
                Machine::exit(if xexit == 0 && xsig == 0 {
                    ExitStatus::Success
                } else {
                    ExitStatus::Failure
                });
            }
            #[cfg(not(feature = "qemu"))]
            panic(format_args!("init died (signal {xsig}, exit {xexit})"));
        }

        pr.ps_flags.fetch_or(PS_EXITING, Ordering::Relaxed);
        pr.ps_xexit.set(xexit as u32);
        pr.ps_xsig.set(xsig);

        // If parent is waiting for us to exit or exec, PS_PPWAIT is set; we wake up the
        // parent early to avoid deadlock.
        if pr.ps_flags.load(Ordering::Relaxed) & PS_PPWAIT != 0 {
            pr.ps_flags.fetch_and(!PS_PPWAIT, Ordering::Relaxed);
            let pptr = parent(pr);
            pptr.ps_flags.fetch_and(!PS_ISPWAIT, Ordering::Relaxed);
            pptr.ps_flags.fetch_or(PS_WAITEVENT, Ordering::Relaxed);
            wakeup(ptr::from_ref(pptr));
        }

        // Wait for concurrent `allprocess' loops
        refcnt_finalize(&pr.ps_refcnt, "psdtor");
    }

    // unlink ourselves from the active threads
    mtx_enter(&pr.ps_mtx);
    // SAFETY: `p` is on its process's thread list, under `ps_mtx`.
    unsafe { pr.ps_threads.remove(p) };
    pr.ps_threadcnt.set(pr.ps_threadcnt.get() - 1);
    pr.ps_exitcnt.set(pr.ps_exitcnt.get() + 1);

    // if somebody else wants to take us to single threaded mode or stop us, count ourselves
    // out.
    if !pr.ps_single.get().is_null() || pr.ps_flags.load(Ordering::Relaxed) & PS_STOPPING != 0 {
        let _ = unported!("exit1: process_suspend_signal (M6-c)");
    }

    // proc is off ps_threads list so update accounting of process now
    tuagg_add_runtime();
    tuagg_add_process(pr, p);

    if p.p_flag.load(Ordering::Relaxed) & P_THREAD == 0 {
        // main thread gotta wait because it has the pid, et al
        while pr.ps_threadcnt.get() + pr.ps_exitcnt.get() > 1 {
            let _ = msleep_nsec(
                ptr::addr_of!(pr.ps_threads),
                &pr.ps_mtx,
                PWAIT,
                "thrdeath",
                INFSLP,
            );
        }
    }
    mtx_leave(&pr.ps_mtx);

    let rup: &Rusage = match NonNull::new(pr.ps_ru.get().cast_mut()) {
        // SAFETY: the process's rusage, a pool item alive until `process_zap`.
        Some(rup) => unsafe { rup.as_ref() },
        None => {
            let Some(mem) = pool_get(&RUSAGE_POOL, PR_WAITOK | PR_ZERO) else {
                panic(format_args!("exit1: rusage_pool is empty"));
            };
            let new = mem.cast::<Rusage>();
            // SAFETY: a fresh, zeroed pool item, written once before anything else sees it.
            unsafe { new.as_ptr().write(Rusage::new()) };
            if pr.ps_ru.get().is_null() {
                pr.ps_ru.set(new.as_ptr());
                // SAFETY: as above; the process owns it now.
                unsafe { new.as_ref() }
            } else {
                pool_put(&RUSAGE_POOL, mem);
                // SAFETY: another thread installed the process's rusage meanwhile.
                unsafe { &*pr.ps_ru.get() }
            }
        }
    };
    p.p_siglist.store(0, Ordering::Relaxed);
    if p.p_flag.load(Ordering::Relaxed) & P_THREAD == 0 {
        pr.ps_siglist.store(0, Ordering::Relaxed);
    }

    // kqpoll_exit(): kqueue (M6-c). kcov: not configured.

    if p.p_flag.load(Ordering::Relaxed) & P_THREAD == 0 {
        if pr.ps_flags.load(Ordering::Relaxed) & PS_PROFIL != 0 {
            let _ = unported!("exit1: stopprofclock (M7)");
        }
        // prof_write(p): subr_prof.c (M7).

        // sigio_freelist(&pr->ps_sigiolst): sigio (M6-c).

        // close open files and release open-file table: fdfree(p) (kern_descrip.c, M6-c).
        let _ = unported!("exit1: fdfree (kern_descrip.c, M6-c)");

        // cancel_all_itimers(): kern_time.c (M6-c).

        timeout_del(&pr.ps_rucheck_to);
        // SYSVSEM: not configured.
        // killjobc(pr): kern_proc.c (M7, with the ttys).
        let _ = unported!("exit1: killjobc (M7)");
        // ACCOUNTING, KTRACE: not configured.

        // unveil_destroy(pr): kern_unveil.c (M7).

        // free(pr->ps_pin.pn_pins), free(pr->ps_libcpin.pn_pins): no pin tables yet.

        // If parent has the SAS_NOCLDWAIT flag set, we're not going to become a zombie:
        // pr->ps_pptr->ps_sigacts->ps_sigflags (M6-c).
        if !parent(pr).ps_sigacts.get().is_null() {
            let _ = unported!("exit1: SAS_NOCLDWAIT (sigacts, M6-c)");
        }

        // Teardown the virtual address space.
        if p.p_flag.load(Ordering::Relaxed) & P_SYSTEM == 0 {
            // exit1() might be called with a lock count greater than one and we want to
            // ensure the costly operation of tearing down the VM space is performed
            // unlocked. It is safe to release them all since exit1() will not return.
            // MULTIPROCESSOR: __mp_release_all(&kernel_lock).
            uvm_purge();
            // KERNEL_LOCK().
        }
    }

    p.p_fd.set(ptr::null()); // zap the thread's copy

    // Release the thread's read reference of resource limit structure.
    if !p.p_limit.get().is_null() {
        p.p_limit.set(ptr::null());
        let _ = unported!("exit1: lim_free (kern_resource.c, M6-c)");
    }

    // Remove proc from pidhash chain and allproc so looking it up won't work. We will put
    // the proc on the deadproc list later (using the p_runq member), and wake up the reaper
    // when we do. If this is the last thread of a process that isn't PS_NOZOMBIE, we'll put
    // the process on the zombprocess list below.
    //
    // NOTE: WE ARE NO LONGER ALLOWED TO SLEEP!
    p.p_stat.set(SDEAD);

    // SAFETY: `p` is on the tid hash chain and on allproc (fork1 put it there), under the
    // kernel lock.
    unsafe {
        ListHead::<ProcHash>::remove(p);
        ListHead::<ProcList>::remove(p);
    }

    if p.p_flag.load(Ordering::Relaxed) & P_THREAD == 0 {
        // SAFETY: `pr` is on the pid hash chain and on allprocess, under the kernel lock.
        unsafe {
            ListHead::<ProcessHash>::remove(pr);
            ListHead::<ProcessList>::remove(pr);
        }

        if pr.ps_flags.load(Ordering::Relaxed) & PS_NOZOMBIE == 0 {
            // SAFETY: `pr` is off allprocess (above) and not on zombprocess yet.
            unsafe { ZOMBPROCESS.0.insert_head(pr) };
        } else {
            // Not going to be a zombie, so it's now off all the lists scanned by
            // ispidtaken(), so block fast reuse of the pid now.
            freepid(pr.ps_pid.get());
        }

        // Reparent children to their original parent, in case they were being traced, or to
        // init(8).
        let initprocess = initprocess_or_process0();
        let mut qr = pr.ps_children.first();
        if qr.is_some() {
            // only need this if any child is S_ZOMB
            wakeup(ptr::from_ref(initprocess));
        }
        while let Some(child) = qr {
            qr = ListHead::<ProcessSibling>::next(child);
            // Traced processes are killed since their existence means someone is screwing
            // up.
            mtx_enter(&child.ps_mtx);
            if child.ps_flags.load(Ordering::Relaxed) & PS_TRACED != 0 {
                // process_untrace(qr); ptsignal/prsignal(SIGKILL): ptrace and signals (M7).
                let _ = unported!("exit1: process_untrace of a traced child (M7)");
                mtx_leave(&child.ps_mtx);
            } else {
                process_reparent(child, initprocess);
                mtx_leave(&child.ps_mtx);
            }
        }

        // Make sure orphans won't remember the exiting process.
        while let Some(orphan) = pr.ps_orphans.first() {
            mtx_enter(&orphan.ps_mtx);
            // KASSERT(qr->ps_opptr == pr); qr->ps_opptr = NULL: ptrace (M7).
            process_clear_orphan(orphan);
            mtx_leave(&orphan.ps_mtx);
        }
    }

    // add thread's accumulated rusage into the process's total
    ruadd(rup, &p.p_ru);

    // clear %cpu usage during swap
    p.p_pctcpu.store(0, Ordering::Relaxed);

    if p.p_flag.load(Ordering::Relaxed) & P_THREAD == 0 {
        // Final thread has died, so add on our children's rusage and calculate the total
        // times.
        let (utime, stime, _) = calcru(&pr.ps_tu);
        rup.ru_utime.set(utime);
        rup.ru_stime.set(stime);
        rup.ru_ixrss.set(pr.ps_tu.tu_ixrss.get() as i64);
        rup.ru_idrss.set(pr.ps_tu.tu_idrss.get() as i64);
        rup.ru_isrss.set(pr.ps_tu.tu_isrss.get() as i64);
        ruadd(rup, &pr.ps_cru);

        // Notify parent that we're gone. If we're not going to become a zombie, reparent to
        // process 1 (init) so that we can wake our original parent to possibly unblock
        // wait4() to return ECHILD.
        mtx_enter(&pr.ps_mtx);
        if pr.ps_flags.load(Ordering::Relaxed) & PS_NOZOMBIE != 0 {
            let ppr = parent(pr);
            process_reparent(pr, initprocess_or_process0());
            ppr.ps_flags.fetch_or(PS_WAITEVENT, Ordering::Relaxed);
            wakeup(ptr::from_ref(ppr));
        }
        mtx_leave(&pr.ps_mtx);
    }

    // just a thread? check if last one standing.
    if p.p_flag.load(Ordering::Relaxed) & P_THREAD != 0 {
        // scheduler_wait_hook(pr->ps_mainproc, p); XXX
        mtx_enter(&pr.ps_mtx);
        pr.ps_exitcnt.set(pr.ps_exitcnt.get() - 1);
        if pr.ps_threadcnt.get() + pr.ps_exitcnt.get() == 1 {
            wakeup(ptr::addr_of!(pr.ps_threads));
        }
        mtx_leave(&pr.ps_mtx);
    }

    // Other substructures are freed from reaper and wait().

    // Finally, call machine-dependent code.
    Machine::cpu_exit(p);

    // Deactivate the exiting address space before the vmspace is freed. Note that we will
    // continue to run on this vmspace's context until the switch to idle in sched_exit().
    //
    // Once we are no longer using the dead process's vmspace and stack, exit2() will be
    // called to schedule those resources to be released by the reaper thread.
    pmap_deactivate(p);
    sched_exit(p)
    // panic("sched_exit returned"): sched_exit never returns.
}

/// `pr->ps_pptr`: a process always has a parent (process 0 is its own).
fn parent(pr: &Process) -> &'static Process {
    // SAFETY: `ps_pptr` is set at creation and only ever re-pointed at a live process.
    unsafe { pr.ps_pptr.get().as_ref() }.unwrap_or(&PROCESS0)
}

/// `deadproc_mutex`: locking of this prochead is special; it's accessed in a critical
/// section of process exit, and thus locking it can't modify interrupt state. We use a simple
/// spin lock for this prochead. We use the `p_runq` member to linkup to deadproc.
static DEADPROC_MUTEX: Mutex = Mutex::new(IPL_NONE);

/// `deadproc`'s head, made `Sync`: touched under `deadproc_mutex`.
struct DeadprocHead(TailqHead<ProcRunq>);
// SAFETY: see the type's doc.
unsafe impl Sync for DeadprocHead {}

/// `deadproc`: the threads waiting for the reaper.
static DEADPROC: DeadprocHead = DeadprocHead(TailqHead::new());

/// `deadproc_mutex`'s `MUTEX_INITIALIZER_FLAGS(IPL_NONE, "deadproc", MTX_NOWITNESS)`: the
/// witness flag is kept for the record.
const DEADPROC_MUTEX_FLAGS: i32 = MTX_NOWITNESS;

/// `exit2`: we are called from `sched_idle()` once it is safe to schedule the dead process's
/// resources to be freed. So this is not allowed to sleep.
///
/// We lock the deadproc list, place the proc on that list (using the `p_runq` member), and
/// wake up the reaper.
pub fn exit2(p: &Proc) {
    // account the remainder of time spent in exit1()
    mtx_enter(&p.process().ps_mtx);
    tuagg_add_process(p.process(), p);
    mtx_leave(&p.process().ps_mtx);

    mtx_enter(&DEADPROC_MUTEX);
    // SAFETY: `p` is SDEAD and on no queue (the idle thread took it off `spc_deadproc`);
    // under `deadproc_mutex`.
    unsafe { DEADPROC.0.insert_tail(p) };
    mtx_leave(&DEADPROC_MUTEX);

    wakeup(ptr::addr_of!(DEADPROC));
    let _ = DEADPROC_MUTEX_FLAGS;
}

/// `proc_free`: returns a dead thread to the pool.
pub fn proc_free(p: &Proc) {
    // crfree(p->p_ucred): kern_prot.c (M6-c).
    let _ = unported!("proc_free: crfree (M6-c)");
    pool_put(&PROC_POOL, NonNull::from(p).cast::<u8>());
    NTHREADS.fetch_sub(1, Ordering::Relaxed);
}

/// `reaper`: process reaper. This is run by a kernel thread to free the resources of a
/// dead process. Once the resources are free, the process becomes a zombie, and the parent
/// is allowed to read the undead's status.
pub fn reaper(_arg: *mut c_void) {
    // KERNEL_UNLOCK(): nothing without MULTIPROCESSOR.

    sched_assert_unlocked();

    loop {
        mtx_enter(&DEADPROC_MUTEX);
        let p = loop {
            if let Some(p) = DEADPROC.0.first() {
                break p;
            }
            let _ = msleep_nsec(
                ptr::addr_of!(DEADPROC),
                &DEADPROC_MUTEX,
                PVM,
                "reaper",
                INFSLP,
            );
        };

        // Remove us from the deadproc list.
        // SAFETY: `p` is the first element, under `deadproc_mutex`.
        unsafe { DEADPROC.0.remove(p) };
        mtx_leave(&DEADPROC_MUTEX);

        // WITNESS_THREAD_EXIT(p): not configured.

        // Free the VM resources we're still holding on to. We must do this from a valid
        // thread because doing so may block.
        uvm_uarea_free(p);
        p.p_vmspace.set(ptr::null()); // zap the thread's copy

        if p.p_flag.load(Ordering::Relaxed) & P_THREAD != 0 {
            // Just a thread
            proc_free(p);
        } else {
            let pr = p.process();

            // Release the rest of the process's vmspace
            uvm_exit(pr);

            // KERNEL_LOCK().
            if pr.ps_flags.load(Ordering::Relaxed) & PS_NOZOMBIE == 0 {
                // Process is now a true zombie.
                pr.ps_flags.fetch_or(PS_ZOMBIE, Ordering::Relaxed);
            }

            // Notify listeners of our demise and clean up: knote_processexit (kqueue, M6-c).

            if pr.ps_flags.load(Ordering::Relaxed) & PS_ZOMBIE != 0 {
                // Post SIGCHLD and wake up parent: prsignal(pr->ps_pptr, SIGCHLD) (M6-c).
                let _ = unported!("reaper: prsignal(SIGCHLD) (M6-c)");
                let pptr = parent(pr);
                pptr.ps_flags.fetch_or(PS_WAITEVENT, Ordering::Relaxed);
                wakeup(ptr::from_ref(pptr));
            } else {
                // No one will wait for us, just zap it.
                process_zap(pr);
            }
            // KERNEL_UNLOCK().
        }
    }
}

// dowait6, sys_wait4, sys_waitid, proc_finish_wait, process_untrace: M6-c and M7.

/// `process_clear_orphan`.
pub fn process_clear_orphan(pr: &Process) {
    if pr.ps_flags.load(Ordering::Relaxed) & PS_ORPHAN != 0 {
        // SAFETY: an orphan is on its parent's orphan list, under the parent's `ps_mtx`.
        unsafe { ListHead::<ProcessOrphan>::remove(pr) };
        pr.ps_flags.fetch_and(!PS_ORPHAN, Ordering::Relaxed);
    }
}

/// `process_reparent`: make process `parent` the new parent of process `child`.
pub fn process_reparent(child: &Process, parent: &Process) {
    if ptr::eq(child.ps_pptr.get(), parent) {
        return;
    }

    // KASSERT(child->ps_opptr == NULL || child->ps_opptr == child->ps_pptr): ptrace (M7).

    // SAFETY: `child` is on its old parent's children list; it moves to the new one, under
    // the kernel lock.
    unsafe {
        ListHead::<ProcessSibling>::remove(child);
        parent.ps_children.insert_head(child);
    }

    process_clear_orphan(child);
    if child.ps_flags.load(Ordering::Relaxed) & PS_TRACED != 0 {
        child.ps_flags.fetch_or(PS_ORPHAN, Ordering::Relaxed);
        // SAFETY: the old parent is alive (it is reparenting its child) and `child` is on
        // no orphan list after `process_clear_orphan`.
        unsafe { self::parent(child).ps_orphans.insert_head(child) };
    }

    mutex_assert_locked(&child.ps_mtx, "process_reparent");
    child.ps_pptr.set(parent);
    child.ps_ppid.set(parent.ps_pid.get());

    // WITNESS_SETCHILD: not configured.
}

/// `process_zap`: finally finished with old proc entry. Unlink it from its process group and
/// free it.
pub fn process_zap(pr: &Process) {
    // SAFETY: a process always has its main thread until it is zapped here.
    let Some(p) = (unsafe { pr.ps_mainproc.get().as_ref() }) else {
        panic(format_args!("process_zap: no main thread"));
    };

    // leavepgrp(pr): kern_proc.c (M7, with the process group management).
    let _ = unported!("process_zap: leavepgrp (M7)");
    // SAFETY: `pr` is on its parent's children list, under the kernel lock.
    unsafe { ListHead::<ProcessSibling>::remove(pr) };
    process_clear_orphan(pr);

    // Decrement the count of procs running with this uid: pr->ps_ucred->cr_ruid, root until
    // the credentials exist (M6-c).
    chgproccnt(0, -1);

    // Release reference to text vnode: vrele (M7); nothing to release yet.
    pr.ps_textvp.set(ptr::null());

    kassert!(pr.ps_threadcnt.get() == 0);
    kassert!(pr.ps_exitcnt.get() == 1);
    if let Some(ru) = NonNull::new(pr.ps_ru.get().cast_mut()) {
        pool_put(&RUSAGE_POOL, ru.cast::<u8>());
    }
    kassert!(pr.ps_threads.is_empty());
    // sigactsfree(pr->ps_sigacts), lim_free(pr->ps_limit), crfree(pr->ps_ucred): M6-c.
    let _ = unported!("process_zap: sigactsfree/lim_free/crfree (M6-c)");
    pool_put(&PROCESS_POOL, NonNull::from(pr).cast::<u8>());
    NPROCESSES.fetch_sub(1, Ordering::Relaxed);

    proc_free(p);
}
