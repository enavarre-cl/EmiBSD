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

/* <CODE> */
//! Process exit: `kern/kern_exit.c`.
//!
//! Upstream: sys/kern/kern_exit.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M6 (part b) ports `sys_exit`, `exit1`, `exit2`, `proc_free`,
//! `reaper`, `process_clear_orphan`, `process_reparent` and `process_zap`: enough for a
//! kernel thread (and soon `init`) to die and be reaped. M8 adds `sys___threxit`,
//! `dowait6`, `sys_wait4`, `sys_waitid`, `proc_finish_wait` and `process_untrace` (the file
//! is complete) and the `ACCOUNTING` record (`acct_process`) in `exit1`. M11e: the
//! `MULTIPROCESSOR` teardown as in C: `exit1` releases every hold of the kernel lock around
//! `uvm_purge`, and the reaper drops the lock its kthread starts with and takes it only
//! around the zombie's notification.
//!
//! ## Deviations
//! - What `exit1` tears down that does not exist yet is reported, each once:
//!   `stopprofclock`/`prof_write` (`cancel_all_itimers` is real since `kern_time.c`,
//!   `unveil_destroy` since `kern_unveil.c`, `kqpoll_exit` and the reaper's
//!   `knote_processexit` since `kern_event.c`), `process_untrace` (the `SIGKILL` to a
//!   traced child is sent); `process_zap` likewise `vrele`. `killjobc` and
//!   `leavepgrp` are real since the process group management of `kern_proc.c`. The signal side
//!   (`single_thread_set`, `process_suspend_signal`, `sigio_freelist`, `SAS_NOCLDWAIT`, the
//!   reaper's `SIGCHLD`, `sigactsfree`) is real since `kern_sig.c`, `fdfree` since
//!   `kern_descrip.c`, `lim_free` since the `plimit` port. The credentials (`crfree` in `proc_free`
//!   and `process_zap`, the real uid `process_zap` uncharges) are real since `kern_prot.c`.
//! - `initprocess` is null until `init` exists (M6-b): until then process 0 adopts the
//!   orphans `exit1` and `process_reparent` would hand to `init`.
//! - `dowait6` takes its out-parameters as `Option<&mut>`; `ps_opptr` (ptrace's old parent)
//!   is always null, since `ptrace(2)` (`sys_process.c`) is not ported, so
//!   `proc_finish_wait` always takes the zombie's branch.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::kassert;
use crate::kern::init_main::{INITPROCESS, PROCESS0};
use crate::kern::kern_descrip::fdfree;
use crate::kern::kern_event::{knote_processexit, kqpoll_exit};
use crate::kern::kern_fork::{NPROCESSES, NTHREADS, freepid};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_proc::{
    PROC_POOL, PROCESS_POOL, RUSAGE_POOL, ZOMBPROCESS, chgproccnt, killjobc, leavepgrp,
};
use crate::kern::kern_prot::crfree;
use crate::kern::kern_resource::{calcru, lim_free, ruadd, tuagg_add_process, tuagg_add_runtime};
use crate::kern::kern_sched::sched_exit;
use crate::kern::kern_sig::{
    process_suspend_signal, prsignal, psignal, ptsignal, sigactsfree, sigio_freelist,
    single_thread_set,
};
use crate::kern::kern_synch::{msleep_nsec, refcnt_finalize, sleep_finish, sleep_setup, wakeup};
use crate::kern::kern_timeout::timeout_del;
use crate::kern::sched_bsd::sched_assert_unlocked;
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::panic;
#[cfg(feature = "qemu")]
use crate::kprintf;
use crate::machine::Machine;
use crate::machine::copy::{copyout, copyout_obj};
use crate::machine::cpu::Cpu;
use crate::machine::intr::IPL_NONE;
use crate::machine::pmap::pmap_deactivate;
#[cfg(feature = "qemu")]
use crate::machine::{Exit, ExitStatus};
use crate::sys::errno::Errno;
use crate::sys::mutex::{MTX_NOWITNESS, Mutex, mutex_assert_locked};
use crate::sys::param::{PCATCH, PVM, PWAIT};
use crate::sys::pool::{PR_WAITOK, PR_ZERO};
use crate::sys::proc::{
    EXIT_NORMAL, EXIT_THREAD, P_SYSTEM, P_THREAD, P_WEXIT, PS_CONTINUED, PS_EXITING, PS_ISPWAIT,
    PS_NOZOMBIE, PS_ORPHAN, PS_PPWAIT, PS_PROFIL, PS_STOPPED, PS_STOPPING, PS_TRACED, PS_TRAPPED,
    PS_WAITED, PS_WAITEVENT, PS_ZOMBIE, Proc, ProcHash, ProcList, ProcRunq, Process, ProcessHash,
    ProcessList, ProcessOrphan, ProcessSibling, SDEAD, SINGLE_EXIT, p_hassibling,
};
use crate::sys::queue::{ListHead, TailqHead};
use crate::sys::resource::Rusage;
use crate::sys::sched::scheduler_wait_hook;
use crate::sys::siginfo::{
    CLD_CONTINUED, CLD_DUMPED, CLD_EXITED, CLD_KILLED, CLD_STOPPED, CLD_TRAPPED, Siginfo,
};
use crate::sys::signal::{SIGCHLD, SIGCONT, SIGKILL, SIGSEGV};
use crate::sys::signalvar::{SAS_NOCLDWAIT, SignalType};
use crate::sys::syscallargs::{SysExitArgs, SysThrexitArgs, SysWait4Args, SysWaitidArgs};
use crate::sys::systm::{INFSLP, SysArgs, kernel_lock, kernel_unlock, sysargs};
use crate::sys::types::{Pid, Register};
use crate::sys::wait::{
    _WCONTINUED, Idtype, P_ALL, P_PGID, P_PID, WAIT_ANY, WAIT_MYPGRP, WCONTINUED, WEXITED, WNOHANG,
    WNOWAIT, WSTOPPED, WTRAPPED, WUNTRACED, w_exitcode, w_stopcode, wcoredump, wstatus,
};
use crate::unported;
use crate::uvm::uvm_glue::{uvm_exit, uvm_purge, uvm_uarea_free};

/// `sys_exit`: death of process.
pub fn sys_exit(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysExitArgs = sysargs(v);

    exit1(p, uap.rval.get(), 0, EXIT_NORMAL)
    // NOTREACHED
}

/// `__threxit(2)`: the calling thread exits; the process goes on unless it was the last
/// thread. `*notdead` (when not NULL) is cleared first, which is how `pthread_join` learns
/// the thread is gone.
#[allow(non_snake_case)] // the C name: sys___threxit
pub fn sys___threxit(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysThrexitArgs = sysargs(v);

    let notdead = uap.notdead.get() as usize;
    if notdead != 0 {
        let zero: Pid = 0;
        if copyout(&zero.to_ne_bytes(), notdead).is_err() {
            psignal(p, SIGSEGV);
        }
    }
    exit1(p, 0, 0, EXIT_THREAD)
    // NOTREACHED
}

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
            let _ = single_thread_set(p, SINGLE_EXIT);
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
        process_suspend_signal(pr);
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

    kqpoll_exit(p);

    // kcov_exit(p): kcov is not configured.

    if p.p_flag.load(Ordering::Relaxed) & P_THREAD == 0 {
        if pr.ps_flags.load(Ordering::Relaxed) & PS_PROFIL != 0 {
            let _ = unported!("exit1: stopprofclock (M7)");
        }
        // prof_write(p): subr_prof.c (M7).

        sigio_freelist(&pr.ps_sigiolst);

        // close open files and release open-file table
        fdfree(p);

        crate::kern::kern_time::cancel_all_itimers();

        timeout_del(&pr.ps_rucheck_to);
        // SYSVSEM: not configured.
        killjobc(pr);
        let _ = crate::kern::kern_acct::acct_process(p);
        // KTRACE: not configured.

        crate::kern::kern_unveil::unveil_destroy(pr);

        pin_free(&pr.ps_pin);
        pin_free(&pr.ps_libcpin);

        // If parent has the SAS_NOCLDWAIT flag set, we're not going to become a zombie.
        if parent(pr).sigacts().ps_sigflags.load(Ordering::Relaxed) & SAS_NOCLDWAIT != 0 {
            pr.ps_flags.fetch_or(PS_NOZOMBIE, Ordering::Relaxed);
        }

        // Teardown the virtual address space.
        if p.p_flag.load(Ordering::Relaxed) & P_SYSTEM == 0 {
            // exit1() might be called with a lock count greater than one and we want to
            // ensure the costly operation of tearing down the VM space is performed
            // unlocked. It is safe to release them all since exit1() will not return.
            #[cfg(feature = "multiprocessor")]
            let _ = crate::kern::kern_lock::__mp_release_all(&crate::kern::kern_lock::KERNEL_LOCK);
            uvm_purge();
            kernel_lock();
        }
    }

    p.p_fd.set(ptr::null()); // zap the thread's copy

    // Release the thread's read reference of resource limit structure.
    // SAFETY: p_limit is null or a reference this thread holds.
    if let Some(limit) = unsafe { p.p_limit.get().as_ref() } {
        p.p_limit.set(ptr::null());
        lim_free(limit);
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
                process_untrace(child);
                mtx_leave(&child.ps_mtx);

                if child.ps_flags.load(Ordering::Relaxed) & PS_EXITING == 0 {
                    // If single threading is active, direct the signal to the active thread
                    // to avoid deadlock.
                    // SAFETY: a non-null `ps_single` is a live thread of `child`.
                    match unsafe { child.ps_single.get().as_ref() } {
                        Some(single) => ptsignal(single, SIGKILL, SignalType::STHREAD),
                        None => prsignal(child, SIGKILL),
                    }
                }
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
    crfree(p.ucred());
    pool_put(&PROC_POOL, NonNull::from(p).cast::<u8>());
    NTHREADS.fetch_sub(1, Ordering::Relaxed);
}

/// `reaper`: process reaper. This is run by a kernel thread to free the resources of a
/// dead process. Once the resources are free, the process becomes a zombie, and the parent
/// is allowed to read the undead's status.
pub fn reaper(_arg: *mut c_void) {
    kernel_unlock();

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

            kernel_lock(); // KERNEL_LOCK()
            if pr.ps_flags.load(Ordering::Relaxed) & PS_NOZOMBIE == 0 {
                // Process is now a true zombie.
                pr.ps_flags.fetch_or(PS_ZOMBIE, Ordering::Relaxed);
            }

            // Notify listeners of our demise and clean up.
            knote_processexit(pr);

            if pr.ps_flags.load(Ordering::Relaxed) & PS_ZOMBIE != 0 {
                // Post SIGCHLD and wake up parent.
                let pptr = parent(pr);
                prsignal(pptr, SIGCHLD);
                pptr.ps_flags.fetch_or(PS_WAITEVENT, Ordering::Relaxed);
                wakeup(ptr::from_ref(pptr));
            } else {
                // No one will wait for us, just zap it.
                process_zap(pr);
            }
            kernel_unlock(); // KERNEL_UNLOCK()
        }
    }
}

/// The id `dowait6` compares a process with: its pid or its process group's id.
fn wait_matches(pr: &Process, idtype: Idtype, id: i64) -> bool {
    !((idtype == P_PID && id != i64::from(pr.ps_pid.get()))
        || (idtype == P_PGID && id != i64::from(pr.pgid())))
}

/// The `siginfo_t` `dowait6` fills for a child: who, as whom, and the `CLD_*` code.
fn wait_info(pr: &Process, code: i32, status: i32) -> Siginfo {
    let mut info = Siginfo::zeroed();
    info.set_si_pid(pr.ps_pid.get());
    info.set_si_uid(pr.ucred().cr_uid.get());
    info.si_signo = SIGCHLD;
    info.si_code = code;
    info.set_si_status(status);
    info
}

/// `dowait6`: the body of `wait4(2)` and `waitid(2)`: finds a child of `q` that matches
/// `idtype`/`id` and has something to report under `options` (exited, trapped, stopped or
/// continued), fills `statusp`, `rusage` and `info`, and returns its pid in `retval`;
/// sleeps for one unless `WNOHANG`. `ECHILD` when no child matches.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn dowait6(
    q: &Proc,
    idtype: Idtype,
    id: i64,
    mut statusp: Option<&mut i32>,
    options: i32,
    rusage: Option<&Rusage>,
    mut info: Option<&mut Siginfo>,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    let qr = q.process();

    if let Some(info) = info.as_deref_mut() {
        *info = Siginfo::zeroed();
    }

    loop {
        qr.ps_flags.fetch_and(!PS_WAITEVENT, Ordering::Relaxed);
        let mut nfound = 0;
        for pr in qr.ps_children.iter() {
            mtx_enter(&pr.ps_mtx);
            let flags = pr.ps_flags.load(Ordering::Relaxed);
            if flags & PS_NOZOMBIE != 0 || !wait_matches(pr, idtype, id) {
                mtx_leave(&pr.ps_mtx);
                continue;
            }
            nfound += 1;
            if options & WEXITED != 0 && flags & PS_ZOMBIE != 0 {
                retval[0] = pr.ps_pid.get() as Register;
                if let Some(info) = info.as_deref_mut() {
                    let xsig = pr.ps_xsig.get();
                    *info = if xsig == 0 {
                        wait_info(pr, CLD_EXITED, pr.ps_xexit.get() as i32)
                    } else if wcoredump(xsig) {
                        wait_info(pr, CLD_DUMPED, wstatus(xsig))
                    } else {
                        wait_info(pr, CLD_KILLED, wstatus(xsig))
                    };
                }

                if let Some(statusp) = statusp.as_deref_mut() {
                    *statusp = w_exitcode(pr.ps_xexit.get() as i32, pr.ps_xsig.get());
                }
                if let Some(rusage) = rusage {
                    // SAFETY: a zombie keeps its `ps_ru` (a `rusage_pool` item) until
                    // `process_zap`.
                    if let Some(ru) = unsafe { pr.ps_ru.get().as_ref() } {
                        rusage.copy_from(ru);
                    }
                }
                mtx_leave(&pr.ps_mtx);
                if options & WNOWAIT == 0 {
                    proc_finish_wait(q, pr);
                }
                return Ok(());
            }
            if options & WTRAPPED != 0
                && flags & PS_TRACED != 0
                && flags & PS_WAITED == 0
                && flags & PS_STOPPED != 0
                && flags & PS_TRAPPED != 0
            {
                if options & WNOWAIT == 0 {
                    pr.ps_flags.fetch_or(PS_WAITED, Ordering::Relaxed);
                }

                retval[0] = pr.ps_pid.get() as Register;
                if let Some(info) = info.as_deref_mut() {
                    *info = wait_info(pr, CLD_TRAPPED, pr.ps_xsig.get());
                }

                if let Some(statusp) = statusp.as_deref_mut() {
                    *statusp = w_stopcode(pr.ps_xsig.get());
                }
                mtx_leave(&pr.ps_mtx);
                if let Some(rusage) = rusage {
                    rusage.copy_from(&Rusage::default());
                }
                return Ok(());
            }
            if (flags & PS_TRACED != 0 || options & WUNTRACED != 0)
                && flags & PS_WAITED == 0
                && flags & PS_STOPPED != 0
                && flags & PS_TRAPPED == 0
            {
                if options & WNOWAIT == 0 {
                    pr.ps_flags.fetch_or(PS_WAITED, Ordering::Relaxed);
                }

                retval[0] = pr.ps_pid.get() as Register;
                if let Some(info) = info.as_deref_mut() {
                    *info = wait_info(pr, CLD_STOPPED, pr.ps_xsig.get());
                }

                if let Some(statusp) = statusp.as_deref_mut() {
                    *statusp = w_stopcode(pr.ps_xsig.get());
                }
                mtx_leave(&pr.ps_mtx);
                if let Some(rusage) = rusage {
                    rusage.copy_from(&Rusage::default());
                }
                return Ok(());
            }
            if options & WCONTINUED != 0 && flags & PS_CONTINUED != 0 {
                if options & WNOWAIT == 0 {
                    pr.ps_flags.fetch_and(!PS_CONTINUED, Ordering::Relaxed);
                }

                retval[0] = pr.ps_pid.get() as Register;
                if let Some(info) = info.as_deref_mut() {
                    *info = wait_info(pr, CLD_CONTINUED, SIGCONT);
                }

                mtx_leave(&pr.ps_mtx);
                if let Some(statusp) = statusp.as_deref_mut() {
                    *statusp = _WCONTINUED;
                }
                if let Some(rusage) = rusage {
                    rusage.copy_from(&Rusage::default());
                }
                return Ok(());
            }
            mtx_leave(&pr.ps_mtx);
        }
        // Look in the orphans list too, to allow the parent to collect its child's exit
        // status even if child is being debugged.
        //
        // Debugger detaches from the parent upon successful switch-over from parent to
        // child. At this point due to re-parenting the parent loses the child to debugger
        // and a wait4(2) call would report that it has no children to wait for. By
        // maintaining a list of orphans we allow the parent to successfully wait until the
        // child becomes a zombie.
        if nfound == 0 {
            for pr in qr.ps_orphans.iter() {
                if pr.ps_flags.load(Ordering::Relaxed) & PS_NOZOMBIE != 0
                    || !wait_matches(pr, idtype, id)
                {
                    continue;
                }
                nfound += 1;
                break;
            }
        }
        if nfound == 0 {
            return Err(Errno::ECHILD);
        }
        if options & WNOHANG != 0 {
            retval[0] = 0;
            return Ok(());
        }
        sleep_setup(ptr::from_ref(qr).cast(), PWAIT | PCATCH, "wait");
        sleep_finish(
            INFSLP,
            qr.ps_flags.load(Ordering::Relaxed) & PS_WAITEVENT == 0,
        )?;
    }
}

/// `wait4(2)`.
pub fn sys_wait4(q: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysWait4Args = sysargs(v);
    let ru = Rusage::default();
    let pid = uap.pid.get();
    let mut options = uap.options.get();
    let mut status = 0i32;

    if options & !(WUNTRACED | WNOHANG | WCONTINUED) != 0 {
        return Err(Errno::EINVAL);
    }
    options |= WEXITED | WTRAPPED;

    let (idtype, id) = if pid == WAIT_MYPGRP {
        (P_PGID, i64::from(q.process().pgid()))
    } else if pid == WAIT_ANY {
        (P_ALL, 0)
    } else if pid < 0 {
        (P_PGID, -i64::from(pid))
    } else {
        (P_PID, i64::from(pid))
    };

    let statusp = uap.status.get() as usize;
    let rusagep = uap.rusage.get() as usize;
    dowait6(
        q,
        idtype,
        id,
        if statusp != 0 {
            Some(&mut status)
        } else {
            None
        },
        options,
        if rusagep != 0 { Some(&ru) } else { None },
        None,
        retval,
    )?;
    if retval[0] > 0 && statusp != 0 {
        copyout(&status.to_ne_bytes(), statusp)?;
    }
    if retval[0] > 0 && rusagep != 0 {
        copyout(&ru.to_bytes(), rusagep)?;
        // KTRACE: not configured.
    }
    Ok(())
}

/// `waitid(2)`.
pub fn sys_waitid(q: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysWaitidArgs = sysargs(v);
    let mut info = Siginfo::zeroed();
    let idtype = uap.idtype.get();
    let options = uap.options.get();

    if options & !(WSTOPPED | WCONTINUED | WEXITED | WTRAPPED | WNOHANG | WNOWAIT) != 0 {
        return Err(Errno::EINVAL);
    }
    if options & (WSTOPPED | WCONTINUED | WEXITED | WTRAPPED) == 0 {
        return Err(Errno::EINVAL);
    }
    if idtype != P_ALL && idtype != P_PID && idtype != P_PGID {
        return Err(Errno::EINVAL);
    }

    dowait6(
        q,
        idtype,
        uap.id.get() as i64,
        None,
        options,
        None,
        Some(&mut info),
        retval,
    )?;
    copyout_obj(&info, uap.info.get() as usize)?;
    // KTRACE: not configured.
    retval[0] = 0;
    Ok(())
}

/// `proc_finish_wait`: the waiter collected `pr`: give a ptrace-attached child back to its
/// old parent, or account for it and free it.
pub fn proc_finish_wait(waiter: &Proc, pr: &Process) {
    // If we got the child via a ptrace 'attach', we need to give it back to the old
    // parent.
    mtx_enter(&pr.ps_mtx);
    // SAFETY: a non-null `ps_opptr` is a live process (ptrace keeps it).
    let opptr = unsafe { pr.ps_opptr.get().as_ref() };
    match opptr {
        Some(tr) if !ptr::eq(tr, pr.ps_pptr.get()) => {
            pr.ps_opptr.set(ptr::null());
            pr.ps_flags.fetch_and(!PS_TRACED, Ordering::Relaxed);
            process_reparent(pr, tr);
            mtx_leave(&pr.ps_mtx);
            prsignal(tr, SIGCHLD);
            tr.ps_flags.fetch_or(PS_WAITEVENT, Ordering::Relaxed);
            wakeup(ptr::from_ref(tr));
        }
        _ => {
            mtx_leave(&pr.ps_mtx);
            // SAFETY: a zombie keeps its main thread until `process_zap`.
            if let Some(child) = unsafe { pr.ps_mainproc.get().as_ref() } {
                scheduler_wait_hook(waiter, child);
            }
            let rup = &waiter.process().ps_cru;
            // SAFETY: as in `dowait6`.
            if let Some(ru) = unsafe { pr.ps_ru.get().as_ref() } {
                ruadd(rup, ru);
            }
            // SAFETY: a zombie is on `zombprocess`, under the kernel lock.
            unsafe { ListHead::<ProcessList>::remove(pr) }; // off zombprocess
            freepid(pr.ps_pid.get());
            process_zap(pr);
        }
    }
}

/// `process_untrace`: give process back to original parent or init(8).
pub fn process_untrace(pr: &Process) {
    kassert!(pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED != 0);
    mutex_assert_locked(&pr.ps_mtx, "process_untrace");

    // SAFETY: as in `proc_finish_wait`.
    let ppr = unsafe { pr.ps_opptr.get().as_ref() }.filter(|op| !ptr::eq(*op, pr.ps_pptr.get()));

    // not being traced any more
    pr.ps_opptr.set(ptr::null());
    pr.ps_flags.fetch_and(!PS_TRACED, Ordering::Relaxed);
    process_reparent(pr, ppr.unwrap_or_else(initprocess_or_process0));
}

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

/// `free(pr->ps_pin.pn_pins, M_PINSYSCALL, ...)` (and `ps_libcpin`): gives a process's pin
/// table back.
pub fn pin_free(pin: &crate::sys::proc::Pinsyscall) {
    if let Some(pins) = NonNull::new(pin.pn_pins.get()) {
        crate::kern::kern_malloc::free(
            pins.cast::<u8>(),
            crate::sys::malloc::M_PINSYSCALL,
            pin.pn_npins.get().max(0) as usize * size_of::<u32>(),
        );
    }
    pin.pn_pins.set(ptr::null_mut());
}

/// `process_zap`: finally finished with old proc entry. Unlink it from its process group and
/// free it.
pub fn process_zap(pr: &Process) {
    // SAFETY: a process always has its main thread until it is zapped here.
    let Some(p) = (unsafe { pr.ps_mainproc.get().as_ref() }) else {
        panic(format_args!("process_zap: no main thread"));
    };

    leavepgrp(pr);
    // SAFETY: `pr` is on its parent's children list, under the kernel lock.
    unsafe { ListHead::<ProcessSibling>::remove(pr) };
    process_clear_orphan(pr);

    // Decrement the count of procs running with this uid.
    chgproccnt(pr.ucred().cr_ruid.get(), -1);

    // Release reference to text vnode
    let otvp = pr.ps_textvp.take();
    if let Some(otvp) = otvp {
        crate::kern::vfs_subr::vrele(otvp);
    }

    kassert!(pr.ps_threadcnt.get() == 0);
    kassert!(pr.ps_exitcnt.get() == 1);
    if let Some(ru) = NonNull::new(pr.ps_ru.get().cast_mut()) {
        pool_put(&RUSAGE_POOL, ru.cast::<u8>());
    }
    kassert!(pr.ps_threads.is_empty());
    sigactsfree(pr.sigacts());
    // SAFETY: the process's own reference, dropped once as it is freed.
    lim_free(unsafe { &*pr.ps_limit.get() });
    crfree(pr.ucred());
    pool_put(&PROCESS_POOL, NonNull::from(pr).cast::<u8>());
    NPROCESSES.fetch_sub(1, Ordering::Relaxed);

    proc_free(p);
}
/* </CODE> */
