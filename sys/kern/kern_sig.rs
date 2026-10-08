/*	$OpenBSD: kern_sig.c,v 1.366 2026/08/23 17:06:56 daniel Exp $	*/
/*	$NetBSD: kern_sig.c,v 1.54 1996/04/22 01:38:32 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Theo de Raadt. All rights reserved.
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
 *	@(#)kern_sig.c	8.7 (Berkeley) 4/18/94
 */
/* </LICENSES> */

/* <CODE> */
//! Signals: `kern/kern_sig.c`.
//!
//! Upstream: sys/kern/kern_sig.c @ 3ce1f3f79392
//!
//! Status: `ported`. The whole file: `cansignal`, `signal_init`, `sigstkinit`,
//! `sigactsinit`/`sigactsfree`, `sys_sigaction`/`setsigvec`, `siginit`, `execsigs`,
//! `sys_sigprocmask`, `sys_sigpending`, `dosigsuspend`/`sys_sigsuspend`, `sigonstack`,
//! `sys_sigaltstack`, `sys_kill`, `sys_thrkill`, `killpg1`, `pgsignal`, `pgsigio`,
//! `postsig_done`, `trapsignal`, `psignal`/`prsignal`/`ptsignal`/`ptsignal_locked`,
//! `setsigctx`, `cursig`, `proc_trap`, `process_continue`, `process_stop`,
//! `proc_stop_setup`/`proc_stop_finish`, `process_suspend_signal`, `postsig`, `sigexit`,
//! `sigabort`, `sigismasked`, `coredump`/`coredump_write`/`coredump_unmap`, `sys_nosys`,
//! `sys___thrsigdivert`, `initsiginfo`, `userret`, `proc_suspend_check[_locked]`,
//! `single_thread_set`/`single_thread_clear` and the `sigio_*` functions. `sys_sigreturn`
//! is machine-dependent (`machdep.c`, `sig_machdep.c`); the one here forwards to the machine
//! (see the deviations). `filt_sig*` (the `EVFILT_SIGNAL` filter) are `kern_event.c`'s.
//!
//! ## Deviations
//! - Calls into subsystems that are not ported are reported with `unported!`: in
//!   `coredump`, the filesystem half: `vn_open` of the core file and everything after it
//!   (`VOP_GETATTR`/`VOP_SETATTR`, `coredump_elf`, `vn_close`) is reported and the dump fails
//!   with `ENOSYS`, so `sigexit` never sets `WCOREFLAG`; `coredump_write`'s `vn_rdwr` is
//!   reported likewise. `coredump` builds the core file name in a stack buffer instead of a
//!   `namei_pool` item, and `KERNELPATH` (`<sys/namei.h>`) is the boolean `incrash`; with no
//!   file descriptor table there is no `fd_rdir` to release. `WCOREFLAG` is `<sys/wait.h>`'s
//!   value, kept here until that header is ported.
//! - `KTRACE`, `WITNESS`, dt(4)'s `TRACEPOINT`, `SMALL_KERNEL` and `PMAP_CHECK_COPYIN` are
//!   not configured. The kernel lock is taken where the C takes it (M11a; nothing without
//!   `MULTIPROCESSOR`).
//! - `sys_sigreturn` lives in `machdep.c` (amd64) and `sig_machdep.c` (arm64); the system
//!   call table finds `pub fn sys_*` only under `sys/kern` and `sys/uvm`, so the entry here
//!   forwards to `machine::MachineSignal::sys_sigreturn`.
//! - Out parameters are return values where the C fills a structure for its caller
//!   (`initsiginfo` returns the `siginfo_t`); `cursig` and `setsigctx` keep the C's `struct
//!   sigctx *` as `&mut Sigctx`, since the callers reuse one context across a loop.
//!   Booleans that the C passes as `int` (`deep`, `all`, `checkctty`, `reset`) are `bool`.
//! - `sigio_setown`/`sigio_getown` take the `int` that `data` points to as `&i32`/`&mut i32`.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering, fence};

use crate::kassert;
use crate::kern::init_main::{INITPROCESS, PROCESS0};
use crate::kern::kern_event::knote_locked;
use crate::kern::kern_exit::exit1;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_pledge::pledge_kill;
use crate::kern::kern_proc::{ALLPROCESS, pgfind, prfind, tfind_user, zombiefind};
use crate::kern::kern_prot::{crdup, crfree, crhold, suser};
use crate::kern::kern_synch::{msleep_nsec, nowake, tsleep_nsec, unsleep, wakeup};
use crate::kern::sched_bsd::{mi_switch, sched_lock, sched_unlock, setrunnable, r#yield};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{Str, log, panic, panicstr, printf, snprintf};
use crate::machine::Machine;
use crate::machine::copy::{copyin_obj, copyout_obj};
use crate::machine::cpu::{Cpu, curproc};
use crate::machine::intr::{IPL_HIGH, IPL_NONE};
use crate::machine::signal::{MachineSignal, sendsig};
use crate::machine::tcb::tcb_get;
use crate::sys::acct::{ABTCFI, ATRAP, AXSIG};
use crate::sys::errno::Errno;
use crate::sys::event::NOTE_SIGNAL;
use crate::sys::malloc::{M_SIGIO, M_WAITOK};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::param::{MAXPATHLEN, MAXPHYS, NZERO, PCATCH, PPAUSE, PUSER, PWAIT, USPACE};
use crate::sys::pool::{PR_WAITOK, Pool};
use crate::sys::proc::{
    EXIT_NORMAL, EXIT_THREAD_NOCHECK, P_ALRMPEND, P_INSCHED, P_PROFPEND, P_SIGSUSPEND, P_SINTR,
    P_SUSPSIG, P_SUSPSINGLE, P_TRACESINGLE, P_WEXIT, PS_CONTINUED, PS_CONTROLT, PS_COREDUMP,
    PS_EXITING, PS_NOBROADCASTKILL, PS_PPWAIT, PS_SINGLEEXIT, PS_SINGLEUNWIND, PS_STOPPED,
    PS_STOPPING, PS_SUGID, PS_SYSTEM, PS_TRACED, PS_TRAPPED, PS_WAITED, PS_WAITEVENT, Pgrp, Proc,
    Process, SDEAD, SIDL, SINGLE_DEEP, SINGLE_EXIT, SINGLE_MASK, SINGLE_SUSPEND, SINGLE_UNWIND,
    SONPROC, SRUN, SSLEEP, SSTOP, p_hassibling,
};
use crate::sys::resource::RLIMIT_CORE;
use crate::sys::resourcevar::lim_cur;
use crate::sys::siginfo::{ILL_BTCFI, SI_USER, Siginfo, Sigval};
use crate::sys::sigio::{Sigio, SigioRef, Sigiolst};
use crate::sys::signal::{
    MINSIGSTKSZ, NSIG, SA_NOCLDSTOP, SA_NOCLDWAIT, SA_NODEFER, SA_ONSTACK, SA_RESETHAND,
    SA_RESTART, SA_SIGINFO, SIG_BLOCK, SIG_DFL, SIG_IGN, SIG_SETMASK, SIG_UNBLOCK, SIGABRT,
    SIGALRM, SIGBUS, SIGCHLD, SIGCONT, SIGFPE, SIGHUP, SIGILL, SIGINT, SIGKILL, SIGPROF, SIGSEGV,
    SIGSTOP, SIGSYS, SIGTERM, SIGTSTP, SIGTTIN, SIGTTOU, SIGUSR1, SIGUSR2, SIGVTALRM, SIGXFSZ,
    SS_DISABLE, SS_ONSTACK, Sigaction, Sigaltstack, Sigset, sigmask,
};
use crate::sys::signalvar::{
    SA_CONT, SA_CORE, SA_IGNORE, SA_KILL, SA_STOP, SA_TTYSTOP, SAS_NOCLDSTOP, SAS_NOCLDWAIT,
    SIG_CATCH, SIG_HOLD, Sigacts, Sigctx, SignalType, sigcantmask, sigpending,
};
use crate::sys::syscallargs::{
    SysKillArgs, SysSigactionArgs, SysSigaltstackArgs, SysSigprocmaskArgs, SysSigsuspendArgs,
    SysThrkillArgs, SysThrsigdivertArgs,
};
use crate::sys::syslog::LOG_ERR;
use crate::sys::systm::{INFSLP, SysArgs, kernel_lock, kernel_unlock, sysargs};
use crate::sys::time::{Timespec, timespec_to_nsec};
use crate::sys::ttycom::{TIOCGPGRP, TIOCSPGRP};
use crate::sys::types::{Pid, Register};
use crate::sys::ucred::Ucred;
use crate::unported;
use crate::uvm::uvm_map::{uvm_map_remap_as_stack, uvm_unmap};
use crate::uvm::uvm_param::ptoa;

/// `NSIG` as an array length.
const NSIG_LEN: usize = NSIG as usize;

/// `sigprop[NSIG]`: the array below categorizes the signals and their default actions.
pub const SIGPROP: [i32; NSIG_LEN] = [
    0,                    // unused
    SA_KILL,              // SIGHUP
    SA_KILL,              // SIGINT
    SA_KILL | SA_CORE,    // SIGQUIT
    SA_KILL | SA_CORE,    // SIGILL
    SA_KILL | SA_CORE,    // SIGTRAP
    SA_KILL | SA_CORE,    // SIGABRT
    SA_KILL | SA_CORE,    // SIGEMT
    SA_KILL | SA_CORE,    // SIGFPE
    SA_KILL,              // SIGKILL
    SA_KILL | SA_CORE,    // SIGBUS
    SA_KILL | SA_CORE,    // SIGSEGV
    SA_KILL | SA_CORE,    // SIGSYS
    SA_KILL,              // SIGPIPE
    SA_KILL,              // SIGALRM
    SA_KILL,              // SIGTERM
    SA_IGNORE,            // SIGURG
    SA_STOP,              // SIGSTOP
    SA_STOP | SA_TTYSTOP, // SIGTSTP
    SA_IGNORE | SA_CONT,  // SIGCONT
    SA_IGNORE,            // SIGCHLD
    SA_STOP | SA_TTYSTOP, // SIGTTIN
    SA_STOP | SA_TTYSTOP, // SIGTTOU
    SA_IGNORE,            // SIGIO
    SA_KILL,              // SIGXCPU
    SA_KILL,              // SIGXFSZ
    SA_KILL,              // SIGVTALRM
    SA_KILL,              // SIGPROF
    SA_IGNORE,            // SIGWINCH
    SA_IGNORE,            // SIGINFO
    SA_KILL,              // SIGUSR1
    SA_KILL,              // SIGUSR2
    SA_IGNORE,            // SIGTHR
];

/// `CONTSIGMASK`.
const CONTSIGMASK: Sigset = sigmask(SIGCONT);
/// `STOPSIGMASK`.
const STOPSIGMASK: Sigset =
    sigmask(SIGSTOP) | sigmask(SIGTSTP) | sigmask(SIGTTIN) | sigmask(SIGTTOU);

/// `WCOREFLAG` (`<sys/wait.h>`): the exit status bit that says a core was dumped.
const WCOREFLAG: i32 = 0o200;

/// `struct coredump_iostate`: what `coredump_write` and `coredump_unmap` work on.
pub struct CoredumpIostate<'a> {
    /// `io_proc`.
    pub io_proc: &'a Proc,
    /// `io_vp`: the core file (`struct vnode`, not ported).
    pub io_vp: *const (),
    /// `io_cred`.
    pub io_cred: &'static Ucred,
    /// `io_offset`.
    pub io_offset: i64,
}

/// `nosuidcoredump` \[a\].
pub static NOSUIDCOREDUMP: AtomicI32 = AtomicI32::new(1);

/// `sigacts_pool`: memory pool for sigacts structures.
pub static SIGACTS_POOL: Pool = Pool::new();

/// `sigio_lock`.
static SIGIO_LOCK: Mutex = Mutex::new(IPL_HIGH);

/// `sigprop[signum]`.
fn sigprop(signum: i32) -> i32 {
    SIGPROP[signum as usize]
}

/// `ffs(mask)`: the lowest set bit, counting from 1; 0 for an empty set.
const fn ffs(mask: Sigset) -> i32 {
    if mask == 0 {
        0
    } else {
        mask.trailing_zeros() as i32 + 1
    }
}

/// `pr->ps_pgrp`: every process is in a process group.
fn pgrp(pr: &Process) -> &Pgrp {
    let pg = pr.ps_pgrp.get();
    kassert!(!pg.is_null());
    // SAFETY: a process's group is set before the process is visible and outlives its
    // membership.
    unsafe { &*pg }
}

/// `pr->ps_pptr`: a process always has a parent (process 0 is its own).
fn parent(pr: &Process) -> &'static Process {
    // SAFETY: `ps_pptr` is set at creation and only ever re-pointed at a live process.
    unsafe { pr.ps_pptr.get().as_ref() }.unwrap_or(&PROCESS0)
}

/// `p == curproc`.
fn is_curproc(p: &Proc) -> bool {
    curproc().is_some_and(|c| ptr::eq(c, p))
}

/// `cansignal`: can thread `p` send the signal `signum` to process `qr`?
pub fn cansignal(p: &Proc, qr: &Process, signum: i32) -> bool {
    let pr = p.process();
    let uc = p.ucred();
    let quc = qr.ucred();

    if uc.cr_uid.get() == 0 {
        return true; // root can always signal
    }

    if ptr::eq(pr, qr) {
        return true; // process can always signal itself
    }

    // optimization: if the same creds then the tests below will pass
    if ptr::eq(uc, quc) {
        return true;
    }

    if signum == SIGCONT && ptr::eq(qr.session(), pr.session()) {
        return true; // SIGCONT in session
    }

    // Using kill(), only certain signals can be sent to setugid child processes
    if qr.ps_flags.load(Ordering::Relaxed) & PS_SUGID != 0 {
        if matches!(
            signum,
            0 | SIGKILL
                | SIGINT
                | SIGTERM
                | SIGALRM
                | SIGSTOP
                | SIGTTIN
                | SIGTTOU
                | SIGTSTP
                | SIGHUP
                | SIGUSR1
                | SIGUSR2
        ) && (uc.cr_ruid.get() == quc.cr_ruid.get() || uc.cr_uid.get() == quc.cr_ruid.get())
        {
            return true;
        }
        return false;
    }

    uc.cr_ruid.get() == quc.cr_ruid.get()
        || uc.cr_ruid.get() == quc.cr_svuid.get()
        || uc.cr_uid.get() == quc.cr_ruid.get()
        || uc.cr_uid.get() == quc.cr_svuid.get()
}

/// `signal_init`: initialize signal-related data structures.
pub fn signal_init() {
    pool_init(
        &SIGACTS_POOL,
        size_of::<Sigacts>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "sigapl",
        None,
    );
}

/// `sigstkinit`: initialize a new sigaltstack structure.
pub fn sigstkinit(ss: &Cell<Sigaltstack>) {
    ss.set(Sigaltstack {
        ss_flags: SS_DISABLE,
        ss_size: 0,
        ss_sp: 0,
        _pad: 0,
    });
}

/// `sigactsinit`: create an initial sigacts structure, using the same signal state as `pr`.
pub fn sigactsinit(pr: &Process) -> &'static Sigacts {
    let Some(mem) = pool_get(&SIGACTS_POOL, PR_WAITOK) else {
        panic(format_args!("sigactsinit: sigacts_pool is empty"));
    };
    let ps = mem.cast::<Sigacts>();
    // SAFETY: a fresh pool item of `size_of::<Sigacts>()` bytes with the pool's alignment,
    // written once before anything else sees it.
    unsafe { ps.as_ptr().write(Sigacts::new()) };
    // SAFETY: as above; the process holds it until `sigactsfree`.
    let ps: &'static Sigacts = unsafe { ps.as_ref() };
    ps.copy_from(pr.sigacts());
    ps
}

/// `sigactsfree`: release a sigacts structure.
pub fn sigactsfree(ps: &'static Sigacts) {
    pool_put(&SIGACTS_POOL, NonNull::from(ps).cast::<u8>());
}

/// `sys_sigaction`: examine and change a signal's action.
pub fn sys_sigaction(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSigactionArgs = sysargs(v);
    let pr = p.process();
    let ps = pr.sigacts();

    let signum = uap.signum.get();
    let nsa = uap.nsa.get() as usize;
    let osa = uap.osa.get() as usize;

    if signum <= 0 || signum >= NSIG || (nsa != 0 && (signum == SIGKILL || signum == SIGSTOP)) {
        return Err(Errno::EINVAL);
    }
    let s = signum as usize;
    if osa != 0 {
        mtx_enter(&pr.ps_mtx);
        let mut sa = Sigaction {
            sa_handler: ps.ps_sigact[s].get(),
            sa_mask: ps.ps_catchmask[s].get(),
            sa_flags: 0,
        };
        let bit = sigmask(signum);
        if ps.ps_sigonstack.get() & bit != 0 {
            sa.sa_flags |= SA_ONSTACK;
        }
        if ps.ps_sigintr.get() & bit == 0 {
            sa.sa_flags |= SA_RESTART;
        }
        if ps.ps_sigreset.get() & bit != 0 {
            sa.sa_flags |= SA_RESETHAND;
        }
        if ps.ps_siginfo.get() & bit != 0 {
            sa.sa_flags |= SA_SIGINFO;
        }
        if signum == SIGCHLD {
            let sigflags = ps.ps_sigflags.load(Ordering::Relaxed);
            if sigflags & SAS_NOCLDSTOP != 0 {
                sa.sa_flags |= SA_NOCLDSTOP;
            }
            if sigflags & SAS_NOCLDWAIT != 0 {
                sa.sa_flags |= SA_NOCLDWAIT;
            }
        }
        mtx_leave(&pr.ps_mtx);
        if sa.sa_mask & bit == 0 {
            sa.sa_flags |= SA_NODEFER;
        }
        sa.sa_mask &= !bit;
        copyout_obj(&sa, osa)?;
        // KTRACE: not configured.
    }
    if nsa != 0 {
        let mut sa: Sigaction = copyin_obj(nsa)?;
        // KTRACE: not configured.
        setsigvec(p, signum, &mut sa);
    }
    Ok(())
}

/// `setsigvec`: installs `sa` as the action of `signum` (and adds `signum` to its mask unless
/// `SA_NODEFER`).
pub fn setsigvec(p: &Proc, signum: i32, sa: &mut Sigaction) {
    let pr = p.process();
    let ps = pr.sigacts();
    let s = signum as usize;

    let bit = sigmask(signum);

    mtx_enter(&pr.ps_mtx);
    ps.ps_sigact[s].set(sa.sa_handler);
    if sa.sa_flags & SA_NODEFER == 0 {
        sa.sa_mask |= sigmask(signum);
    }
    ps.ps_catchmask[s].set(sa.sa_mask & !sigcantmask());
    if signum == SIGCHLD {
        if sa.sa_flags & SA_NOCLDSTOP != 0 {
            ps.ps_sigflags.fetch_or(SAS_NOCLDSTOP, Ordering::Relaxed);
        } else {
            ps.ps_sigflags.fetch_and(!SAS_NOCLDSTOP, Ordering::Relaxed);
        }
        // If the SA_NOCLDWAIT flag is set or the handler is SIG_IGN we reparent the dying
        // child to PID 1 (init) which will reap the zombie. Because we use init to do our
        // dirty work we never set SAS_NOCLDWAIT for PID 1.
        // XXX exit1 rework means this is unnecessary?
        // SAFETY: a non-null `initprocess` is init's process, which never goes away.
        let init_is_us = unsafe { INITPROCESS.load(Ordering::Relaxed).as_ref() }
            .is_some_and(|init| ptr::eq(init.ps_sigacts.get(), ps));
        if !init_is_us && (sa.sa_flags & SA_NOCLDWAIT != 0 || sa.sa_handler == SIG_IGN) {
            ps.ps_sigflags.fetch_or(SAS_NOCLDWAIT, Ordering::Relaxed);
        } else {
            ps.ps_sigflags.fetch_and(!SAS_NOCLDWAIT, Ordering::Relaxed);
        }
    }
    let set = |c: &Cell<Sigset>, on: bool| {
        if on {
            c.set(c.get() | bit);
        } else {
            c.set(c.get() & !bit);
        }
    };
    set(&ps.ps_sigreset, sa.sa_flags & SA_RESETHAND != 0);
    set(&ps.ps_siginfo, sa.sa_flags & SA_SIGINFO != 0);
    set(&ps.ps_sigintr, sa.sa_flags & SA_RESTART == 0);
    set(&ps.ps_sigonstack, sa.sa_flags & SA_ONSTACK != 0);
    // Set bit in ps_sigignore for signals that are set to SIG_IGN, and for signals set to
    // SIG_DFL where the default is to ignore. However, don't put SIGCONT in ps_sigignore, as
    // we have to restart the process.
    if sa.sa_handler == SIG_IGN || (sigprop(signum) & SA_IGNORE != 0 && sa.sa_handler == SIG_DFL) {
        p.p_siglist.fetch_and(!bit, Ordering::Relaxed);
        pr.ps_siglist.fetch_and(!bit, Ordering::Relaxed);
        if signum != SIGCONT {
            set(&ps.ps_sigignore, true); // easier in psignal
        }
        set(&ps.ps_sigcatch, false);
    } else {
        set(&ps.ps_sigignore, false);
        set(&ps.ps_sigcatch, sa.sa_handler != SIG_DFL);
    }
    mtx_leave(&pr.ps_mtx);
}

/// `siginit`: initialize signal state for process 0; set to ignore signals that are ignored
/// by default.
pub fn siginit(ps: &Sigacts) {
    for i in 0..NSIG {
        if sigprop(i) & SA_IGNORE != 0 && i != SIGCONT {
            ps.ps_sigignore.set(ps.ps_sigignore.get() | sigmask(i));
        }
    }
    ps.ps_sigflags
        .store(SAS_NOCLDWAIT | SAS_NOCLDSTOP, Ordering::Relaxed);
}

/// `execsigs`: reset signals for an exec by the specified thread.
pub fn execsigs(p: &Proc) {
    let pr = p.process();
    let ps = pr.sigacts();
    mtx_enter(&pr.ps_mtx);

    // Reset caught signals. Held signals remain held through p_sigmask (unless they were
    // caught, and are now ignored by default).
    while ps.ps_sigcatch.get() != 0 {
        let nc = ffs(ps.ps_sigcatch.get());
        let mask = sigmask(nc);
        ps.ps_sigcatch.set(ps.ps_sigcatch.get() & !mask);
        if sigprop(nc) & SA_IGNORE != 0 {
            if nc != SIGCONT {
                ps.ps_sigignore.set(ps.ps_sigignore.get() | mask);
            }
            p.p_siglist.fetch_and(!mask, Ordering::Relaxed);
            pr.ps_siglist.fetch_and(!mask, Ordering::Relaxed);
        }
        ps.ps_sigact[nc as usize].set(SIG_DFL);
    }
    // Reset stack state to the user stack. Clear set of signals caught on the signal stack.
    sigstkinit(&p.p_sigstk);
    ps.ps_sigonstack.set(0);
    ps.ps_sigflags.fetch_and(!SAS_NOCLDWAIT, Ordering::Relaxed);
    if ps.ps_sigact[SIGCHLD as usize].get() == SIG_IGN {
        ps.ps_sigact[SIGCHLD as usize].set(SIG_DFL);
    }
    mtx_leave(&pr.ps_mtx);
}

/// `sys_sigprocmask`: manipulate signal mask. Note that we receive new mask, not pointer,
/// and return old mask as return value; the library stub does the rest.
pub fn sys_sigprocmask(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSigprocmaskArgs = sysargs(v);

    kassert!(is_curproc(p));

    retval[0] = p.p_sigmask.get() as Register;
    let mask = uap.mask.get() & !sigcantmask();

    match uap.how.get() {
        SIG_BLOCK => p.p_sigmask.set(p.p_sigmask.get() | mask),
        SIG_UNBLOCK => p.p_sigmask.set(p.p_sigmask.get() & !mask),
        SIG_SETMASK => p.p_sigmask.set(mask),
        _ => return Err(Errno::EINVAL),
    }
    Ok(())
}

/// `sys_sigpending`: the signals pending for the thread and its process.
pub fn sys_sigpending(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    retval[0] = (p.p_siglist.load(Ordering::Relaxed)
        | p.process().ps_siglist.load(Ordering::Relaxed)) as Register;
    Ok(())
}

/// `dosigsuspend`: temporarily replace calling proc's signal mask for the duration of a
/// system call. Original signal mask will be restored by `userret()`.
pub fn dosigsuspend(p: &Proc, newmask: Sigset) {
    kassert!(is_curproc(p));

    p.p_oldmask.set(p.p_sigmask.get());
    p.p_sigmask.set(newmask);
    p.p_flag.fetch_or(P_SIGSUSPEND, Ordering::Relaxed);
}

/// `sys_sigsuspend`: suspend thread until signal, providing mask to be set in the meantime.
/// Note nonstandard calling convention: libc stub passes mask, not pointer, to save a copyin.
pub fn sys_sigsuspend(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSigsuspendArgs = sysargs(v);

    dosigsuspend(p, (uap.mask.get() as Sigset) & !sigcantmask());
    while tsleep_nsec(nowake(), PPAUSE | PCATCH, "sigsusp", INFSLP).is_ok() {
        continue;
    }
    // always return EINTR rather than ERESTART...
    Err(Errno::EINTR)
}

/// `sigonstack`: whether the user stack pointer `stack` is on curproc's alternate signal
/// stack.
pub fn sigonstack(stack: usize) -> bool {
    let Some(p) = curproc() else {
        return false;
    };
    let ss = p.p_sigstk.get();

    if ss.ss_flags & SS_DISABLE != 0 {
        false
    } else {
        stack.wrapping_sub(ss.ss_sp) < ss.ss_size
    }
}

/// `sys_sigaltstack`: set and/or get the alternate signal stack.
pub fn sys_sigaltstack(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSigaltstackArgs = sysargs(v);
    let onstack = sigonstack(Machine::proc_stack(p));

    let nss = uap.nss.get() as usize;
    let oss = uap.oss.get() as usize;

    if oss != 0 {
        let mut ss = p.p_sigstk.get();
        if onstack {
            ss.ss_flags |= SS_ONSTACK;
        }
        copyout_obj(&ss, oss)?;
    }
    if nss == 0 {
        return Ok(());
    }
    let mut ss: Sigaltstack = copyin_obj(nss)?;
    ss._pad = 0;
    if onstack {
        return Err(Errno::EPERM);
    }
    if ss.ss_flags & !SS_DISABLE != 0 {
        return Err(Errno::EINVAL);
    }
    if ss.ss_flags & SS_DISABLE != 0 {
        let mut cur = p.p_sigstk.get();
        cur.ss_flags = ss.ss_flags;
        p.p_sigstk.set(cur);
        return Ok(());
    }
    if ss.ss_size < MINSIGSTKSZ {
        return Err(Errno::ENOMEM);
    }

    uvm_map_remap_as_stack(p, ss.ss_sp, ss.ss_size)?;

    p.p_sigstk.set(ss);
    Ok(())
}

/// `sys_kill`: send `signum` to a process, a process group or every process.
pub fn sys_kill(cp: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysKillArgs = sysargs(v);
    let pid = uap.pid.get();
    let signum = uap.signum.get();

    pledge_kill(cp, pid)?;
    if (signum as u32) >= NSIG as u32 {
        return Err(Errno::EINVAL);
    }
    if pid > 0 {
        let (pr, zombie) = match prfind(pid) {
            Some(pr) => (pr, false),
            None => match zombiefind(pid) {
                Some(pr) => (pr, true),
                None => return Err(Errno::ESRCH),
            },
        };
        if !cansignal(cp, pr, signum) {
            return Err(Errno::EPERM);
        }

        // kill single process
        if signum != 0 && !zombie {
            prsignal(pr, signum);
        }
        return Ok(());
    }
    match pid {
        -1 => killpg1(cp, signum, 0, true),    // broadcast signal
        0 => killpg1(cp, signum, 0, false),    // signal own process group
        _ => killpg1(cp, signum, -pid, false), // negative explicit process group
    }
}

/// `sys_thrkill`: send `signum` to a thread of the calling process.
pub fn sys_thrkill(cp: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysThrkillArgs = sysargs(v);
    let tid = uap.tid.get();
    let signum = uap.signum.get();

    if (signum as u32) >= NSIG as u32 {
        return Err(Errno::EINVAL);
    }

    let p: &Proc = if tid != 0 {
        match tfind_user(tid, cp.process()) {
            Some(p) => p,
            None => return Err(Errno::ESRCH),
        }
    } else {
        cp
    };

    // optionally require the target thread to have the given tcb addr
    let tcb = uap.tcb.get() as usize;
    if tcb != 0 && tcb != tcb_get(p) {
        return Err(Errno::ESRCH);
    }

    if signum != 0 {
        ptsignal(p, signum, SignalType::STHREAD);
    }
    Ok(())
}

/// `killpg1`: common code for kill process group/broadcast kill. `cp` is calling process.
pub fn killpg1(cp: &Proc, signum: i32, pgid: Pid, all: bool) -> Result<(), Errno> {
    let mut nfound = 0;

    if all {
        // broadcast
        for pr in ALLPROCESS.0.iter() {
            if pr.ps_pid.get() <= 1
                || pr.ps_flags.load(Ordering::Relaxed) & (PS_SYSTEM | PS_NOBROADCASTKILL) != 0
                || ptr::eq(pr, cp.process())
                || !cansignal(cp, pr, signum)
            {
                continue;
            }
            nfound += 1;
            if signum != 0 {
                prsignal(pr, signum);
            }
        }
    } else {
        let pgrp = if pgid == 0 {
            // zero pgid means send to my process group.
            pgrp(cp.process())
        } else {
            match pgfind(pgid) {
                Some(pg) => pg,
                None => return Err(Errno::ESRCH),
            }
        };
        for pr in pgrp.pg_members.iter() {
            if pr.ps_pid.get() <= 1
                || pr.ps_flags.load(Ordering::Relaxed) & PS_SYSTEM != 0
                || !cansignal(cp, pr, signum)
            {
                continue;
            }
            nfound += 1;
            if signum != 0 {
                prsignal(pr, signum);
            }
        }
    }
    if nfound != 0 {
        Ok(())
    } else {
        Err(Errno::ESRCH)
    }
}

/// `CANDELIVER(uid, euid, pr)`.
fn candeliver(uid: u32, euid: u32, pr: &Process) -> bool {
    let cr = pr.ucred();
    euid == 0
        || uid == cr.cr_ruid.get()
        || uid == cr.cr_svuid.get()
        || uid == cr.cr_uid.get()
        || euid == cr.cr_ruid.get()
        || euid == cr.cr_svuid.get()
        || euid == cr.cr_uid.get()
}

/// `CANSIGIO(cr, pr)`.
fn cansigio(cr: &Ucred, pr: &Process) -> bool {
    candeliver(cr.cr_ruid.get(), cr.cr_uid.get(), pr)
}

/// `pgsignal`: send a signal to a process group. If `checkctty` is set, limit to members
/// which have a controlling terminal.
pub fn pgsignal(pgrp: Option<&Pgrp>, signum: i32, checkctty: bool) {
    if let Some(pgrp) = pgrp {
        for pr in pgrp.pg_members.iter() {
            if !checkctty || pr.ps_flags.load(Ordering::Relaxed) & PS_CONTROLT != 0 {
                prsignal(pr, signum);
            }
        }
    }
}

/// `pgsigio`: send a SIGIO or SIGURG signal to a process or process group using stored
/// credentials rather than those of the current process.
pub fn pgsigio(sir: &SigioRef, sig: i32, checkctty: bool) {
    if sir.sir_sigio.get().is_null() {
        return;
    }

    kernel_lock(); // KERNEL_LOCK()
    mtx_enter(&SIGIO_LOCK);
    // SAFETY: a registered sigio stays allocated until `sigio_del`, which runs after it is
    // unlinked under `sigio_lock`.
    if let Some(sigio) = unsafe { sir.sir_sigio.get().as_ref() } {
        // SAFETY: the sigio holds a reference to its credentials.
        let cred = unsafe { &*sigio.sio_ucred.get() };
        if sigio.sio_pgid.get() > 0 {
            // SAFETY: the process unlinks its sigios (`sigio_freelist`) before it goes.
            if let Some(pr) = unsafe { sigio.sio_proc.get().as_ref() }
                && cansigio(cred, pr)
            {
                prsignal(pr, sig);
            }
        } else if sigio.sio_pgid.get() < 0 {
            // SAFETY: as above, for the process group.
            if let Some(pg) = unsafe { sigio.sio_pgrp.get().as_ref() } {
                for pr in pg.pg_members.iter() {
                    if cansigio(cred, pr)
                        && (!checkctty || pr.ps_flags.load(Ordering::Relaxed) & PS_CONTROLT != 0)
                    {
                        prsignal(pr, sig);
                    }
                }
            }
        }
    }
    // out:
    mtx_leave(&SIGIO_LOCK);
    kernel_unlock(); // KERNEL_UNLOCK()
}

/// `postsig_done`: recalculate the signal mask and reset the signal disposition after
/// usermode frame for delivery is formed.
pub fn postsig_done(p: &Proc, signum: i32, catchmask: Sigset, reset: bool) {
    p.p_ru.ru_nsignals.set(p.p_ru.ru_nsignals.get() + 1);
    p.p_sigmask.set(p.p_sigmask.get() | catchmask);
    if reset {
        let mask = sigmask(signum);
        let pr = p.process();
        let ps = pr.sigacts();

        mtx_enter(&pr.ps_mtx);
        ps.ps_sigcatch.set(ps.ps_sigcatch.get() & !mask);
        if signum != SIGCONT && sigprop(signum) & SA_IGNORE != 0 {
            ps.ps_sigignore.set(ps.ps_sigignore.get() | mask);
        }
        ps.ps_sigact[signum as usize].set(SIG_DFL);
        mtx_leave(&pr.ps_mtx);
    }
}

/// `trapsignal`: send a signal caused by a trap to the current thread. If it will be caught
/// immediately, deliver it with correct code. Otherwise, post it normally.
pub fn trapsignal(p: &Proc, signum: i32, trapno: u64, code: i32, sigval: Sigval) {
    let pr = p.process();
    let mut signum = signum;
    let mut ctx = Sigctx::default();

    match signum {
        SIGILL if code == ILL_BTCFI => pr.ps_acflag.set(pr.ps_acflag.get() | ABTCFI),
        SIGILL | SIGBUS | SIGSEGV => pr.ps_acflag.set(pr.ps_acflag.get() | ATRAP),
        _ => {}
    }

    let mut mask = sigmask(signum);
    setsigctx(p, signum, &mut ctx);
    if pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED == 0
        && ctx.sig_catch
        && p.p_sigmask.get() & mask == 0
    {
        let si = initsiginfo(signum, trapno, code, sigval);
        // KTRACE: not configured.
        if sendsig(
            ctx.sig_action,
            signum,
            p.p_sigmask.get(),
            &si,
            ctx.sig_info,
            ctx.sig_onstack,
        )
        .is_err()
        {
            kernel_lock(); // KERNEL_LOCK()
            sigexit(p, SIGILL);
            // NOTREACHED
        }
        postsig_done(p, signum, ctx.sig_catchmask, ctx.sig_reset);
    } else {
        p.p_sisig.set(signum);
        p.p_sitrapno.set(trapno); // XXX for core dump/debugger
        p.p_sicode.set(code);
        p.p_sigval.set(sigval);

        // If traced, stop if signal is masked, and stay stopped until released by the
        // debugger. If our parent process is waiting for us, don't hang as we could
        // deadlock.
        if pr.ps_flags.load(Ordering::Relaxed) & (PS_TRACED | PS_PPWAIT) == PS_TRACED
            && signum != SIGKILL
            && p.p_sigmask.get() & mask != 0
        {
            signum = proc_trap(p, signum);

            // If we are no longer being traced, or the parent didn't give us a signal, skip
            // sending the signal.
            if pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED == 0 || signum == 0 {
                return;
            }
            mask = sigmask(signum);
            setsigctx(p, signum, &mut ctx);

            // update signal info
            p.p_sisig.set(signum);
        }

        // Signals like SIGBUS and SIGSEGV should not, when generated by the kernel, be
        // ignorable or blockable. If it is and we're not being traced, then just kill the
        // process. After vfs_shutdown(9), init(8) cannot receive signals because new code
        // pages of the signal handler cannot be mapped from halted storage. init(8) may not
        // die or the kernel panics. Better loop between signal handler and page fault trap
        // until the machine is halted.
        if pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED == 0
            && sigprop(signum) & SA_KILL != 0
            && (p.p_sigmask.get() & mask != 0 || ctx.sig_ignore)
            && pr.ps_pid.get() != 1
        {
            kernel_lock(); // KERNEL_LOCK()
            sigexit(p, signum);
            // NOTREACHED
        }
        ptsignal(p, signum, SignalType::STHREAD);
    }
}

/// `psignal`: send the signal to the process. If the signal has an action, the action is
/// usually performed by the target process rather than the caller; we add the signal to the
/// set of pending signals for the process.
///
/// Exceptions:
/// - When a stop signal is sent to a sleeping process that takes the default action, the
///   process is stopped without awakening it.
/// - SIGCONT restarts stopped processes (or puts them back to sleep) regardless of the
///   signal action (eg, blocked or ignored).
///
/// Other ignored signals are discarded immediately.
pub fn psignal(p: &Proc, signum: i32) {
    ptsignal(p, signum, SignalType::SPROCESS);
}

/// `prsignal`: send the signal to the process `pr` (to its first thread, from where
/// `ptsignal_locked` picks the thread that takes it).
pub fn prsignal(pr: &Process, signum: i32) {
    mtx_enter(&pr.ps_mtx);
    // Ignore signal if the target process is exiting
    if pr.ps_flags.load(Ordering::Relaxed) & PS_EXITING != 0 {
        mtx_leave(&pr.ps_mtx);
        return;
    }
    if let Some(p) = pr.ps_threads.first() {
        ptsignal_locked(p, signum, SignalType::SPROCESS);
    }
    mtx_leave(&pr.ps_mtx);
}

/// `ptsignal`: `type_` = `SPROCESS`: process signal, can be diverted (sigwait());
/// `STHREAD`: thread signal, but should be propagated if unhandled.
pub fn ptsignal(p: &Proc, signum: i32, type_: SignalType) {
    let pr = p.process();

    mtx_enter(&pr.ps_mtx);
    ptsignal_locked(p, signum, type_);
    mtx_leave(&pr.ps_mtx);
}

/// `ptsignal_locked`: `ptsignal` with the process's `ps_mtx` held.
pub fn ptsignal_locked(p: &Proc, signum: i32, type_: SignalType) {
    let pr = p.process();
    let mut p = p;
    let mut altaction = SIG_DFL;
    let mut wakeparent = false;

    mutex_assert_locked(&pr.ps_mtx, "ptsignal_locked");

    #[cfg(feature = "diagnostic")]
    if (signum as u32) >= NSIG as u32 || signum == 0 {
        #[allow(clippy::panic)] // the C's DIAGNOSTIC panic
        panic(format_args!("psignal signal number"));
    }

    // Ignore signal if the target process is exiting
    if pr.ps_flags.load(Ordering::Relaxed) & PS_EXITING != 0 {
        return;
    }

    let mut mask = sigmask(signum);
    let mut sigmask_ = p.p_sigmask.get();

    if type_ == SignalType::SPROCESS {
        // Accept SIGKILL to coredumping processes
        if pr.ps_flags.load(Ordering::Relaxed) & PS_COREDUMP != 0 && signum == SIGKILL {
            pr.ps_siglist.fetch_or(mask, Ordering::Relaxed);
            return;
        }

        // If the current thread can process the signal immediately (it's unblocked) then
        // have it take it.
        let cur = curproc().filter(|q| {
            ptr::eq(q.process(), pr)
                && q.p_flag.load(Ordering::Relaxed) & P_WEXIT == 0
                && q.p_sigmask.get() & mask == 0
        });
        if let Some(q) = cur {
            p = q;
            sigmask_ = q.p_sigmask.get();
        } else {
            // A process-wide signal can be diverted to a different thread that's in
            // sigwait() for this signal. If there isn't such a thread, then pick a thread
            // that doesn't have it blocked so that the stop/kill consideration isn't
            // delayed. Otherwise, mark it pending on the main thread.
            for q in pr.ps_threads.iter() {
                // ignore exiting threads
                if q.p_flag.load(Ordering::Relaxed) & P_WEXIT != 0 {
                    continue;
                }

                // skip threads that have the signal blocked
                let tmpmask = q.p_sigmask.get();
                if tmpmask & mask != 0 {
                    continue;
                }

                // okay, could send to this thread
                p = q;
                sigmask_ = tmpmask;

                // sigsuspend, sigwait, ppoll/pselect, etc? Definitely go to this thread, as
                // it's already blocked in the kernel.
                if q.p_flag.load(Ordering::Relaxed) & P_SIGSUSPEND != 0 {
                    break;
                }
            }
        }
    }

    knote_locked(&pr.ps_klist, i64::from(NOTE_SIGNAL) | i64::from(signum));

    let mut prop = sigprop(signum);
    let mut action;

    // If proc is traced, always give parent a chance.
    if pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED != 0 {
        action = SIG_DFL;
    } else {
        // If the signal is being ignored, then we forget about it immediately. (Note: we
        // don't set SIGCONT in ps_sigignore, and if it is set to SIG_IGN, action will be
        // SIG_DFL here.)
        let ps = pr.sigacts();
        let sigignore = ps.ps_sigignore.get();
        let sigcatch = ps.ps_sigcatch.get();

        if sigignore & mask != 0 {
            return;
        }
        if sigmask_ & mask != 0 {
            action = SIG_HOLD;
            if sigcatch & mask != 0 {
                altaction = SIG_CATCH;
            }
        } else if sigcatch & mask != 0 {
            action = SIG_CATCH;
        } else {
            action = SIG_DFL;

            if prop & SA_KILL != 0 && i32::from(pr.ps_nice.get()) > NZERO {
                pr.ps_nice.set(NZERO as u8);
            }

            // If sending a tty stop signal to a member of an orphaned process group,
            // discard the signal here if the action is default; don't stop the process
            // below if sleeping, and don't clear any pending SIGCONT.
            if prop & SA_TTYSTOP != 0 && pgrp(pr).pg_jobc.get() == 0 {
                return;
            }
        }
    }
    // If delivered to process, mark as pending there. Continue and stop signals are always
    // marked at process level.
    let siglist: &AtomicU32 = if type_ == SignalType::SPROCESS || prop & (SA_CONT | SA_STOP) != 0 {
        &pr.ps_siglist
    } else {
        &p.p_siglist
    };

    sched_lock();

    'out: {
        match p.p_stat.get() {
            SSTOP => {
                // If traced process is already stopped, then no further action is
                // necessary.
                if pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED != 0 {
                    break 'out;
                }

                // Kill signal always sets processes running.
                if signum == SIGKILL {
                    p.p_flag.fetch_and(!P_SUSPSIG, Ordering::Relaxed);
                    // Raise priority to at least PUSER.
                    if i32::from(p.p_usrpri.get()) > PUSER {
                        p.p_usrpri.set(PUSER as u8);
                    }
                    unsleep(p);
                    setrunnable(p);
                    break 'out;
                }

                if prop & SA_CONT != 0 {
                    // If SIGCONT is default (or ignored), we continue the process but don't
                    // leave the signal in p_siglist, as it has no further action. If SIGCONT
                    // is held, we continue the process and leave the signal in p_siglist. If
                    // the process catches SIGCONT, let it handle the signal itself. At the
                    // end continue the process.
                    pr.ps_flags.fetch_or(PS_CONTINUED, Ordering::Relaxed);
                    pr.ps_flags.fetch_and(
                        !(PS_WAITED | PS_STOPPED | PS_STOPPING | PS_TRAPPED),
                        Ordering::Relaxed,
                    );
                    if action == SIG_DFL {
                        mask = 0;
                    }
                    if action == SIG_CATCH {
                        // Raise priority to at least PUSER.
                        if i32::from(p.p_usrpri.get()) > PUSER {
                            p.p_usrpri.set(PUSER as u8);
                        }
                        unsleep(p);
                    }

                    process_continue(pr, P_SUSPSIG);
                    wakeparent = true;
                    break 'out;
                }

                // Defer further processing for signals which are held, except that stopped
                // processes must be continued by SIGCONT.
                if action == SIG_HOLD {
                    break 'out;
                }

                if prop & SA_STOP != 0 {
                    // Already stopped, don't need to stop again. (If we did the shell could
                    // get confused.)
                    mask = 0;
                    break 'out;
                }

                // If process is sleeping interruptibly, then simulate a wakeup so that when
                // it is continued, it will be made runnable and can look at the signal. But
                // don't make the process runnable, leave it stopped.
                if p.p_flag.load(Ordering::Relaxed) & P_SINTR != 0 {
                    unsleep(p);
                }
            }

            SSLEEP => {
                // If process is sleeping uninterruptibly we can't interrupt the sleep... the
                // signal will be noticed when the process returns through trap() or
                // syscall().
                if p.p_flag.load(Ordering::Relaxed) & P_SINTR == 0 {
                    break 'out;
                }
                // Process is sleeping and traced... make it runnable so it can discover the
                // signal in cursig() and stop for the parent.
                if pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED != 0 {
                    unsleep(p);
                    setrunnable(p);
                    break 'out;
                }

                // Recheck sigmask before waking up the process, there is a chance that
                // while sending the signal the process changed sigmask and went to sleep.
                sigmask_ = p.p_sigmask.get();
                if sigmask_ & mask != 0 {
                    break 'out;
                } else if action == SIG_HOLD {
                    // signal got unmasked, get proper action
                    action = altaction;

                    if action == SIG_DFL {
                        if prop & SA_KILL != 0 && i32::from(pr.ps_nice.get()) > NZERO {
                            pr.ps_nice.set(NZERO as u8);
                        }

                        // Discard tty stop signals sent to an orphaned process group, see
                        // above.
                        if prop & SA_TTYSTOP != 0 && pgrp(pr).pg_jobc.get() == 0 {
                            mask = 0;
                            prop = 0;
                            break 'out;
                        }
                    }
                }

                // If SIGCONT is default (or ignored) and process is asleep, we are finished;
                // the process should not be awakened.
                if prop & SA_CONT != 0 && action == SIG_DFL {
                    mask = 0;
                    break 'out;
                }
                // When a sleeping process receives a stop signal, process immediately if
                // possible.
                if prop & SA_STOP != 0 && action == SIG_DFL {
                    // If a child holding parent blocked, stopping could cause deadlock.
                    if pr.ps_flags.load(Ordering::Relaxed) & PS_PPWAIT != 0 {
                        break 'out;
                    }
                    mask = 0;
                    pr.ps_xsig.set(signum);
                    pr.ps_flags.fetch_or(PS_STOPPING, Ordering::Relaxed);
                    process_stop(pr, P_SUSPSIG, SINGLE_SUSPEND);
                    wakeparent = true;
                    break 'out;
                }
                // All other (caught or default) signals cause the process to run. Raise
                // priority to at least PUSER.
                if i32::from(p.p_usrpri.get()) > PUSER {
                    p.p_usrpri.set(PUSER as u8);
                }
                unsleep(p);
                setrunnable(p);
            }

            SONPROC => {
                if action == SIG_HOLD {
                    break 'out;
                }

                // set siglist before issuing the ast
                siglist.fetch_or(mask, Ordering::Relaxed);
                mask = 0;
                Machine::signotify(p);
                // FALLTHROUGH to the default: nothing more.
            }

            // SRUN, SIDL, SDEAD do nothing with the signal, other than kicking ourselves if
            // we are running. It will either never be noticed, or noticed very soon.
            _ => {}
        }
    }

    // out: finally adjust siglist
    if mask != 0 {
        siglist.fetch_or(mask, Ordering::Relaxed);
    }
    if prop & SA_CONT != 0 {
        siglist.fetch_and(!STOPSIGMASK, Ordering::Relaxed);
    }
    if prop & SA_STOP != 0 {
        siglist.fetch_and(!CONTSIGMASK, Ordering::Relaxed);
        pr.ps_flags.fetch_and(!PS_CONTINUED, Ordering::Relaxed);
    }

    sched_unlock();
    if wakeparent {
        if prop & SA_STOP != 0 {
            process_suspend_signal(pr);
        } else {
            let pptr = parent(pr);
            pptr.ps_flags.fetch_or(PS_WAITEVENT, Ordering::Relaxed);
            wakeup(ptr::from_ref(pptr));
        }
    }
}

/// `setsigctx`: fill the signal context which should be used by `postsig()` and
/// `issignal()`.
pub fn setsigctx(p: &Proc, signum: i32, sctx: &mut Sigctx) {
    let pr = p.process();
    let ps = pr.sigacts();
    let s = signum as usize;

    mtx_enter(&pr.ps_mtx);
    let mask = sigmask(signum);
    sctx.sig_action = ps.ps_sigact[s].get();
    sctx.sig_catchmask = ps.ps_catchmask[s].get();
    sctx.sig_reset = ps.ps_sigreset.get() & mask != 0;
    sctx.sig_info = ps.ps_siginfo.get() & mask != 0;
    sctx.sig_intr = ps.ps_sigintr.get() & mask != 0;
    sctx.sig_onstack = ps.ps_sigonstack.get() & mask != 0;
    sctx.sig_ignore = ps.ps_sigignore.get() & mask != 0;
    sctx.sig_catch = ps.ps_sigcatch.get() & mask != 0;
    sctx.sig_stop = sigprop(signum) & SA_STOP != 0 && sctx.sig_action == SIG_DFL;
    if sctx.sig_stop {
        // If the process is a member of an orphaned process group, ignore tty stop signals.
        if pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED != 0
            || (pgrp(pr).pg_jobc.get() == 0 && sigprop(signum) & SA_TTYSTOP != 0)
        {
            sctx.sig_stop = false;
            sctx.sig_ignore = true;
        }
    }
    mtx_leave(&pr.ps_mtx);
}

/// `cursig`: determine signal that should be delivered to process `p`, the current process,
/// 0 if none.
///
/// If the current process has received a signal (should be caught or cause termination,
/// should interrupt current syscall), return the signal number. Stop signals with default
/// action are processed immediately, then cleared; they aren't returned. This is checked
/// after each entry to the system for a syscall or trap. The normal call sequence is
///
/// ```text
/// while (signum = cursig(curproc, &ctx, 0))
///     postsig(signum, &ctx);
/// ```
///
/// Assumes that if the `P_SINTR` flag is set, we're holding both the kernel and scheduler
/// locks.
pub fn cursig(p: &Proc, sctx: &mut Sigctx, deep: bool) -> i32 {
    let pr = p.process();
    let mut keep: Sigset = 0;
    let mut signum;
    let mut mask;

    kassert!(is_curproc(p));

    loop {
        let ps_siglist = pr.ps_siglist.load(Ordering::Relaxed);
        fence(Ordering::Acquire); // membar_consumer()
        mask = sigpending(p);
        if pr.ps_flags.load(Ordering::Relaxed) & PS_PPWAIT != 0 {
            mask &= !STOPSIGMASK;
        }
        signum = ffs(mask);
        if signum == 0 {
            // no signal to send
            break; // goto keep
        }
        mask = sigmask(signum);

        // take the signal!
        if pr
            .ps_siglist
            .compare_exchange(
                ps_siglist,
                ps_siglist & !mask,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_err()
        {
            // lost race taking the process signal, restart
            continue;
        }
        p.p_siglist.fetch_and(!mask, Ordering::Relaxed);
        setsigctx(p, signum, sctx);

        // We should see pending but ignored signals only if PS_TRACED was on when they were
        // posted.
        if sctx.sig_ignore && pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED == 0 {
            continue;
        }

        // If cursig is called while going to sleep, abort now and stop the sleep. When the
        // call unwinded to userret cursig is called again and there the signal can be
        // handled cleanly.
        if deep {
            // Do not stop the thread here if multiple signals are pending and at least one
            // of them would force an unwind.
            //
            // ffs() favors low numbered signals and so stop signals may be picked up before
            // other pending signals.
            if sctx.sig_stop && sigpending(p) != 0 {
                keep |= mask;
                continue;
            }
            break; // goto keep
        }

        // If traced, always stop, and stay stopped until released by the debugger. If our
        // parent process is waiting for us, don't hang as we could deadlock.
        if pr.ps_flags.load(Ordering::Relaxed) & (PS_TRACED | PS_PPWAIT) == PS_TRACED
            && signum != SIGKILL
        {
            signum = proc_trap(p, signum);

            mask = sigmask(signum);
            setsigctx(p, signum, sctx);

            // If we are no longer being traced, or the parent didn't give us a signal, or
            // the signal is ignored, look for more signals.
            if pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED == 0
                || signum == 0
                || sctx.sig_ignore
            {
                continue;
            }

            // If the new signal is being masked, look for other signals but leave it for
            // later.
            if p.p_sigmask.get() & mask != 0 {
                p.p_siglist.fetch_or(mask, Ordering::Relaxed);
                continue;
            }
        }

        let prop = sigprop(signum);

        // Decide whether the signal should be returned. Return the signal's number, or fall
        // through to clear it from the pending mask.
        match sctx.sig_action {
            SIG_DFL => {
                // Don't take default actions on system processes.
                if pr.ps_pid.get() <= 1 {
                    // Are you sure you want to ignore SIGSEGV in init? XXX
                    #[cfg(feature = "diagnostic")]
                    printf(format_args!(
                        "Process (pid {}) got signal {}\n",
                        pr.ps_pid.get(),
                        signum
                    ));
                    continue; // == ignore
                }
                // If there is a pending stop signal to process with default action, stop
                // here, then clear the signal.
                if sctx.sig_stop {
                    mtx_enter(&pr.ps_mtx);
                    pr.ps_xsig.set(signum);
                    pr.ps_flags.fetch_or(PS_STOPPING, Ordering::Relaxed);
                    sched_lock();
                    process_stop(pr, P_SUSPSIG, SINGLE_SUSPEND);
                    p.p_flag.fetch_or(P_SUSPSIG, Ordering::Relaxed);
                    proc_stop_setup(p);
                    sched_unlock();
                    process_suspend_signal(pr);
                    proc_stop_finish(p);
                    mtx_leave(&pr.ps_mtx);
                    continue;
                } else if prop & SA_IGNORE != 0 {
                    // Except for SIGCONT, shouldn't get here. Default action is to ignore;
                    // drop it.
                    continue; // == ignore
                } else {
                    break; // goto keep
                }
            }
            SIG_IGN => {
                // Masking above should prevent us ever trying to take action on an ignored
                // signal other than SIGCONT, unless process is traced.
                if prop & SA_CONT == 0 && pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED == 0 {
                    printf(format_args!("cursig\n"));
                }
                continue; // == ignore
            }
            _ => {
                // This signal has an action, let postsig() process it.
                break; // goto keep
            }
        }
    }

    // keep: if we stashed a stop signal but no other signal is pending anymore pick the stop
    // signal up again.
    if keep != 0 && signum == 0 {
        signum = ffs(keep);
        setsigctx(p, signum, sctx);
    }
    // move the signal to p_siglist for later
    p.p_siglist.fetch_or(mask | keep, Ordering::Relaxed);
    signum
}

/// `proc_trap`: stop the traced thread `p` for its debugger with `signum`; returns the
/// signal the debugger hands back (0 for none).
pub fn proc_trap(p: &Proc, signum: i32) -> i32 {
    let pr = p.process();

    mtx_enter(&pr.ps_mtx);
    // Wait until any other suspend condition cleared, including other traps.
    let _ = proc_suspend_check_locked(p, false);

    pr.ps_flags
        .fetch_or(PS_STOPPING | PS_TRAPPED, Ordering::Relaxed);
    sched_lock();
    process_stop(pr, P_SUSPSIG, SINGLE_SUSPEND);
    p.p_flag.fetch_or(P_SUSPSIG, Ordering::Relaxed);
    proc_stop_setup(p);
    sched_unlock();
    pr.ps_xsig.set(signum);
    pr.ps_trapped.set(p);

    process_suspend_signal(pr);
    proc_stop_finish(p);
    // Clear all flags for proc and process by hand here since ptrace just calls setrunnable
    // on the thread without clearing anything.
    p.p_flag.fetch_and(!P_SUSPSIG, Ordering::Relaxed);
    pr.ps_flags
        .fetch_and(!(PS_WAITED | PS_STOPPED | PS_TRAPPED), Ordering::Relaxed);

    let signum = pr.ps_xsig.get();
    pr.ps_xsig.set(0);
    pr.ps_trapped.set(ptr::null());

    if p.p_flag.load(Ordering::Relaxed) & P_TRACESINGLE == 0 {
        sched_lock();
        process_continue(pr, P_SUSPSIG);
        sched_unlock();
    }
    p.p_flag.fetch_and(!P_TRACESINGLE, Ordering::Relaxed);
    mtx_leave(&pr.ps_mtx);

    signum
}

/// `process_continue`: continue all threads of a process that were stopped because of
/// `flag`.
pub fn process_continue(pr: &Process, flag: i32) {
    mutex_assert_locked(&pr.ps_mtx, "process_continue");

    // skip curproc if it is part of pr
    let me = curproc().filter(|c| ptr::eq(c.process(), pr));

    for q in pr.ps_threads.iter() {
        if me.is_some_and(|me| ptr::eq(me, q)) {
            continue;
        }
        if q.p_flag.load(Ordering::Relaxed) & flag == 0 {
            continue;
        }
        q.p_flag.fetch_and(!flag, Ordering::Relaxed);

        // XXX in ptsignal the SCHED_LOCK is already held so we can't grab it here until
        // that is fixed.
        crate::kern::sched_bsd::sched_assert_locked();
        // Stopping a process is not an atomic operation so it is possible that some
        // threads are not stopped when process_continue is called. These threads need to
        // be skipped.
        //
        // Clearing either makes the thread runnable or puts it back into some sleep queue.
        if q.p_stat.get() == SSTOP
            && q.p_flag.load(Ordering::Relaxed) & (P_SUSPSIG | P_SUSPSINGLE) == 0
        {
            if q.p_wchan.get().is_null() {
                setrunnable(q);
            } else {
                q.p_stat.set(SSLEEP);
            }
        }
    }
}

/// `process_stop`: signal all but `p` threads of a process `pr` to stop because of `flag`.
/// Depending on `mode` stopped and sleeping threads may be woken up.
pub fn process_stop(pr: &Process, flag: i32, mode: i32) {
    mutex_assert_locked(&pr.ps_mtx, "process_stop");

    // skip curproc if it is part of pr, caller takes care of that
    let me = curproc().filter(|c| ptr::eq(c.process(), pr));
    if let Some(me) = me {
        kassert!(me.p_flag.load(Ordering::Relaxed) & (P_SUSPSIG | P_SUSPSINGLE) == 0);
    }

    pr.ps_suspendcnt.set(pr.ps_threadcnt.get());
    for q in pr.ps_threads.iter() {
        if me.is_some_and(|me| ptr::eq(me, q)) {
            continue;
        }
        q.p_flag.fetch_or(flag, Ordering::Relaxed);

        // XXX in ptsignal the SCHED_LOCK is already held so we can't grab it here until
        // that is fixed.
        crate::kern::sched_bsd::sched_assert_locked();

        match q.p_stat.get() {
            SSTOP => {
                if mode == SINGLE_EXIT {
                    unsleep(q);
                    setrunnable(q);
                } else {
                    pr.ps_suspendcnt.set(pr.ps_suspendcnt.get().wrapping_sub(1));
                }
            }
            SSLEEP => {
                // if it's not interruptible, then just have to wait
                if q.p_flag.load(Ordering::Relaxed) & P_SINTR != 0 {
                    // merely need to suspend? just stop it
                    if mode == SINGLE_SUSPEND {
                        q.p_stat.set(SSTOP);
                        pr.ps_suspendcnt.set(pr.ps_suspendcnt.get().wrapping_sub(1));
                    } else {
                        // need to unwind or exit, so wake it
                        unsleep(q);
                        setrunnable(q);
                    }
                }
            }
            SONPROC => Machine::signotify(q),
            SRUN | SIDL | SDEAD => {}
            _ => {}
        }
    }
}

/// `proc_stop_setup`: prepare a proc to be stopped.
pub fn proc_stop_setup(p: &Proc) {
    mutex_assert_locked(&p.process().ps_mtx, "proc_stop_setup");
    // XXX in ptsignal the SCHED_LOCK is already held so we can't grab it here until that is
    // fixed.
    crate::kern::sched_bsd::sched_assert_locked();

    // TRACEPOINT(sched, stop, NULL): dt(4), not configured.

    p.p_flag.fetch_or(P_INSCHED, Ordering::Relaxed);
    p.p_stat.set(SSTOP);
}

/// `proc_stop_finish`: finish stopping a process if the condition still holds.
pub fn proc_stop_finish(p: &Proc) {
    let pr = p.process();

    mutex_assert_locked(&pr.ps_mtx, "proc_stop_finish");
    mtx_leave(&pr.ps_mtx);
    sched_lock();

    p.p_flag.fetch_and(!P_INSCHED, Ordering::Relaxed);
    if p.p_stat.get() == SSTOP {
        p.p_ru.ru_nvcsw.set(p.p_ru.ru_nvcsw.get() + 1);
        mi_switch();
    } else {
        kassert!(p.p_stat.get() == SONPROC);
        sched_unlock();
    }
    mtx_enter(&pr.ps_mtx);
}

/// `process_suspend_signal`: signal either the parent process or the `ps_single` thread
/// depending on the mode. Only do this if the suspendcnt dropped to 0. If curproc part of
/// the process count it out first.
pub fn process_suspend_signal(pr: &Process) {
    mutex_assert_locked(&pr.ps_mtx, "process_suspend_signal");

    // if part of the process, count us out
    if curproc().is_some_and(|c| ptr::eq(c.process(), pr)) {
        pr.ps_suspendcnt.set(pr.ps_suspendcnt.get().wrapping_sub(1));
    }

    if pr.ps_suspendcnt.get() != 0 {
        return;
    }

    if pr.ps_single.get().is_null() {
        pr.ps_flags
            .fetch_and(!(PS_STOPPING | PS_WAITED | PS_CONTINUED), Ordering::Relaxed);
        pr.ps_flags.fetch_or(PS_STOPPED, Ordering::Relaxed);

        let pptr = parent(pr);
        if pptr.sigacts().ps_sigflags.load(Ordering::Relaxed) & SAS_NOCLDSTOP == 0 {
            prsignal(pptr, SIGCHLD);
        }
        pptr.ps_flags.fetch_or(PS_WAITEVENT, Ordering::Relaxed);
        wakeup(ptr::from_ref(pptr));
    } else {
        wakeup(ptr::addr_of!(pr.ps_suspendcnt));
    }
}

/// `postsig`: take the action for the specified signal from the current set of pending
/// signals.
pub fn postsig(p: &Proc, signum: i32, sctx: &Sigctx) {
    kassert!(signum != 0);

    let mask = sigmask(signum);
    p.p_siglist.fetch_and(!mask, Ordering::Relaxed);

    let (trapno, code, sigval) = if p.p_sisig.get() != signum {
        (0, SI_USER, Sigval::from_ptr(0))
    } else {
        (p.p_sitrapno.get(), p.p_sicode.get(), p.p_sigval.get())
    };
    let si = initsiginfo(signum, trapno, code, sigval);

    // KTRACE: not configured.
    if sctx.sig_action == SIG_DFL {
        // Default action, where the default is to kill the process. (Other cases were
        // ignored above.)
        kernel_lock(); // KERNEL_LOCK()
        sigexit(p, signum);
        // NOTREACHED
    } else {
        // If we get here, the signal must be caught.
        #[cfg(feature = "diagnostic")]
        if sctx.sig_action == SIG_IGN || p.p_sigmask.get() & mask != 0 {
            #[allow(clippy::panic)] // the C's DIAGNOSTIC panic
            panic(format_args!("postsig action"));
        }
        // Set the new mask value and also defer further occurrences of this signal.
        //
        // Special case: user has done a sigpause. Here the current mask is not of interest,
        // but rather the mask from before the sigpause is what we want restored after the
        // signal processing is completed.
        let returnmask = if p.p_flag.load(Ordering::Relaxed) & P_SIGSUSPEND != 0 {
            p.p_flag.fetch_and(!P_SIGSUSPEND, Ordering::Relaxed);
            p.p_oldmask.get()
        } else {
            p.p_sigmask.get()
        };
        if p.p_sisig.get() == signum {
            p.p_sisig.set(0);
            p.p_sitrapno.set(0);
            p.p_sicode.set(SI_USER);
            p.p_sigval.set(Sigval::from_ptr(0));
        }

        if sendsig(
            sctx.sig_action,
            signum,
            returnmask,
            &si,
            sctx.sig_info,
            sctx.sig_onstack,
        )
        .is_err()
        {
            kernel_lock(); // KERNEL_LOCK()
            sigexit(p, SIGILL);
            // NOTREACHED
        }
        postsig_done(p, signum, sctx.sig_catchmask, sctx.sig_reset);
    }
}

/// `sigexit`: force the current process to exit with the specified signal, dumping core if
/// appropriate. We bypass the normal tests for masked and caught signals, allowing
/// unrecoverable failures to terminate the process without changing signal state. Mark the
/// accounting record with the signal termination. If dumping core, save the signal number
/// for the debugger. Calls exit and does not return.
pub fn sigexit(p: &Proc, signum: i32) -> ! {
    let mut signum = signum;

    // Mark process as going away
    p.p_flag.fetch_or(P_WEXIT, Ordering::Relaxed);

    let pr = p.process();
    pr.ps_acflag.set(pr.ps_acflag.get() | AXSIG);
    if sigprop(signum) & SA_CORE != 0 {
        p.p_sisig.set(signum);

        // if there are other threads, pause them
        if p_hassibling(p) {
            let _ = single_thread_set(p, SINGLE_UNWIND);
        }

        if coredump(p).is_ok() {
            signum |= WCOREFLAG;
        }
    }
    exit1(p, 0, signum, EXIT_NORMAL)
    // NOTREACHED
}

/// `sigabort`: send uncatchable SIGABRT for coredump.
pub fn sigabort(p: &Proc) {
    kassert!(
        is_curproc(p) || panicstr() || crate::kern::init_main::DB_ACTIVE.load(Ordering::Relaxed)
    );

    let mut sa = Sigaction {
        sa_handler: SIG_DFL,
        ..Sigaction::default()
    };
    setsigvec(p, SIGABRT, &mut sa);
    p.p_sigmask.set(p.p_sigmask.get() & !sigmask(SIGABRT));
    psignal(p, SIGABRT);
}

/// `sigismasked`: return `true` if `sig`, a given signal, is ignored or masked for `p`, a
/// given thread, and `false` otherwise.
pub fn sigismasked(p: &Proc, sig: i32) -> bool {
    let pr = p.process();

    kassert!(is_curproc(p));

    mtx_enter(&pr.ps_mtx);
    let rv = pr.sigacts().ps_sigignore.get() & sigmask(sig) != 0
        || p.p_sigmask.get() & sigmask(sig) != 0;
    mtx_leave(&pr.ps_mtx);

    rv
}

/// `coredump`: dump core, into a file named "progname.core", unless the process was
/// setuid/setgid. The file is written by the filesystems, which are not ported: see the
/// module's deviations.
pub fn coredump(p: &Proc) -> Result<(), Errno> {
    // SMALL_KERNEL: not configured.
    let pr = p.process();
    let mut cred = p.ucred();
    let vm = p.vmspace();
    let dir = "/var/crash";
    let nosuidcoredump_local = NOSUIDCOREDUMP.load(Ordering::Relaxed);

    pr.ps_flags.fetch_or(PS_COREDUMP, Ordering::Relaxed);

    // PMAP_CHECK_COPYIN: not configured.

    // Don't dump if will exceed file size limit.
    let size = USPACE + ptoa(vm.vm_dsize.get() as usize + vm.vm_ssize.get() as usize);
    if size as u64 >= lim_cur(RLIMIT_CORE) {
        return Err(Errno::EFBIG);
    }

    // name = pool_get(&namei_pool, PR_WAITOK): a stack buffer (see the module's deviations).
    let mut name = [0u8; MAXPATHLEN];
    let incrash: bool;
    let len;

    // If the process has inconsistent uids, nosuidcoredump determines coredump placement
    // policy.
    let sugid = pr.ps_flags.load(Ordering::Relaxed) & PS_SUGID != 0;
    if (sugid && suser(p).is_err()) || (sugid && nosuidcoredump_local != 0) {
        if nosuidcoredump_local == 3 {
            // If the program directory does not exist, dumps of that core will silently
            // fail.
            len = snprintf(
                &mut name,
                format_args!("{}/{}/{}.core", dir, Str(pr.comm()), pr.ps_pid.get() as u32),
            );
            incrash = true; // KERNELPATH
        } else if nosuidcoredump_local == 2 {
            len = snprintf(&mut name, format_args!("{}/{}.core", dir, Str(pr.comm())));
            incrash = true; // KERNELPATH
        } else {
            return Err(Errno::EPERM);
        }
    } else {
        len = snprintf(&mut name, format_args!("{}.core", Str(pr.comm())));
        incrash = false;
    }

    if len >= MAXPATHLEN {
        return Err(Errno::EACCES);
    }

    // Control the UID used to write out. The normal case uses the real UID. If the sugid
    // case is going to write into the controlled directory, we do so as root.
    if !incrash {
        cred = crdup(cred);
        cred.cr_uid.set(cred.cr_ruid.get());
        cred.cr_gid.set(cred.cr_rgid.get());
    } else {
        // p->p_fd->fd_rdir: no file descriptor table, so no root directory to release.
        p.p_ucred.set(crdup(p.ucred()));
        crfree(cred);
        cred = p.ucred();
        crhold(cred);
        cred.cr_uid.set(0);
        cred.cr_gid.set(0);
    }

    // incrash should be 0 or KERNELPATH only
    // NDINIT(&nd, 0, BYPASSUNVEIL | incrash, UIO_SYSSPACE, name, p) and vn_open(&nd,
    // O_CREAT | FWRITE | O_NOFOLLOW | O_NONBLOCK, S_IRUSR | S_IWUSR); then the regular-file,
    // link-count, mode and owner checks (VOP_GETATTR), the truncation (VOP_SETATTR), ACORE,
    // vn_close and coredump_elf(p, &io), which sets ACORE: the filesystems (see the module's
    // deviations).
    let error = unported!("coredump: vn_open of the core file (vfs)");

    // out:
    crfree(cred);
    Err(error)
}

/// `coredump_write`: writes `len` bytes at `data` (kernel or user space, as `segflg` says)
/// to the core file, in `MAXPHYS` chunks, giving up when `SIGKILL` arrives.
pub fn coredump_write(
    io: &mut CoredumpIostate<'_>,
    _segflg: i32,
    _data: *const u8,
    len: usize,
    isvnode: bool,
) -> Result<(), Errno> {
    let mut coffset = 0usize;
    let mut csize = len;

    loop {
        if sigmask(SIGKILL)
            & (io.io_proc.p_siglist.load(Ordering::Relaxed)
                | io.io_proc.process().ps_siglist.load(Ordering::Relaxed))
            != 0
        {
            return Err(Errno::EINTR);
        }

        // Rest of the loop sleeps with lock held, so...
        r#yield();

        let chunk = csize.min(MAXPHYS);
        // vn_rdwr(UIO_WRITE, io->io_vp, data + coffset, chunk, io->io_offset + coffset,
        // segflg, IO_UNIT, io->io_cred, NULL, io->io_proc): the filesystems.
        let error = unported!("coredump_write: vn_rdwr (vfs)");
        if error != Errno::EFAULT || !isvnode {
            let pr = io.io_proc.process();

            if error == Errno::ENOSPC {
                log(
                    LOG_ERR,
                    format_args!(
                        "coredump of {}({}) failed, filesystem full\n",
                        Str(pr.comm()),
                        pr.ps_pid.get()
                    ),
                );
            } else {
                log(
                    LOG_ERR,
                    format_args!(
                        "coredump of {}({}), write failed: errno {}\n",
                        Str(pr.comm()),
                        pr.ps_pid.get(),
                        error as i32
                    ),
                );
            }
            return Err(error);
        }

        coffset += chunk;
        csize -= chunk;
        if csize == 0 {
            break;
        }
    }

    let _ = coffset;
    io.io_offset += len as i64;
    Ok(())
}

/// `coredump_unmap`: unmaps what `coredump_elf` is done with.
pub fn coredump_unmap(io: &CoredumpIostate<'_>, start: usize, end: usize) {
    uvm_unmap(&io.io_proc.vmspace().vm_map, start, end);
}

/// `sys_nosys`: nonexistent system call-- signal process (may want to handle it). Flag error
/// in case process won't see signal immediately (blocked or ignored).
pub fn sys_nosys(p: &Proc, _v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    ptsignal(p, SIGSYS, SignalType::STHREAD);
    Err(Errno::ENOSYS)
}

/// `sys___thrsigdivert`: `sigwait(3)`'s system call: wait for one of the signals in
/// `sigmask`, which are taken out of the normal delivery.
#[allow(non_snake_case)] // the generator finds the C's name, `sys___thrsigdivert`
pub fn sys___thrsigdivert(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysThrsigdivertArgs = sysargs(v);
    let mut ctx = Sigctx::default();
    let mask = uap.sigmask.get() & !sigcantmask();
    let mut si = Siginfo::zeroed();
    let mut nsecs = INFSLP;
    let mut timeinvalid = false;
    let timeout = uap.timeout.get() as usize;
    let mut error: Result<(), Errno> = Ok(());

    if timeout != 0 {
        let ts: Timespec = copyin_obj(timeout)?;
        // KTRACE: not configured.
        if !ts.is_valid() {
            timeinvalid = true;
        } else {
            nsecs = timespec_to_nsec(&ts);
        }
    }

    dosigsuspend(p, p.p_sigmask.get() & !mask);
    loop {
        si.si_signo = cursig(p, &mut ctx, false);
        if si.si_signo != 0 {
            let smask = sigmask(si.si_signo);
            if smask & mask != 0 {
                p.p_siglist.fetch_and(!smask, Ordering::Relaxed);
                error = Ok(());
                break;
            }
        }

        // per-POSIX, delay this error until after the above
        if timeinvalid {
            error = Err(Errno::EINVAL);
        }
        // per-POSIX, return immediately if timeout is zero-valued
        if nsecs == 0 {
            error = Err(Errno::EAGAIN);
        }

        if error.is_err() {
            break;
        }

        error = tsleep_nsec(nowake(), PPAUSE | PCATCH, "sigwait", nsecs);
    }

    match error {
        Ok(()) => {
            retval[0] = si.si_signo as Register;
            let info = uap.info.get() as usize;
            if info != 0 {
                copyout_obj(&si, info)?;
                // KTRACE: not configured.
            }
            Ok(())
        }
        // Restarting is wrong if there's a timeout, as it'll be for the same interval again
        Err(Errno::ERESTART) if timeout != 0 => Err(Errno::EINTR),
        Err(e) => Err(e),
    }
}

/// `sys_sigreturn`: the machine-dependent `sigreturn(2)` (see the module's deviations).
pub fn sys_sigreturn(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    <Machine as MachineSignal>::sys_sigreturn(p, v, retval)
}

/// `initsiginfo`: the `siginfo_t` of signal `sig` with `code`; `trapno` and `val` fill the
/// fault members for a kernel-generated `SIGSEGV`/`SIGILL`/`SIGBUS`/`SIGFPE`, `val` the
/// value of a user-generated one.
pub fn initsiginfo(sig: i32, trapno: u64, code: i32, val: Sigval) -> Siginfo {
    let mut si = Siginfo::zeroed();

    si.si_signo = sig;
    si.si_code = code;
    if code == SI_USER {
        si.set_si_value(val);
    } else {
        match sig {
            SIGSEGV | SIGILL | SIGBUS | SIGFPE => {
                si.set_si_addr(val.sival_ptr());
                si.set_si_trapno(trapno as i32);
            }
            SIGXFSZ => {}
            _ => {}
        }
    }
    si
}

/// `userret`: the last thing before a thread returns to user mode: pending suspensions and
/// signals, then the CPU's current priority.
pub fn userret(p: &Proc) {
    let mut ctx = Sigctx::default();

    if p.p_flag.load(Ordering::Relaxed) & (P_SUSPSINGLE | P_SUSPSIG) != 0 {
        let _ = proc_suspend_check(p, false);
    }

    // send SIGPROF or SIGVTALRM if their timers interrupted this thread
    if p.p_flag.load(Ordering::Relaxed) & P_PROFPEND != 0 {
        p.p_flag.fetch_and(!P_PROFPEND, Ordering::Relaxed);
        psignal(p, SIGPROF);
    }
    if p.p_flag.load(Ordering::Relaxed) & P_ALRMPEND != 0 {
        p.p_flag.fetch_and(!P_ALRMPEND, Ordering::Relaxed);
        psignal(p, SIGVTALRM);
    }

    if sigpending(p) != 0 {
        loop {
            let signum = cursig(p, &mut ctx, false);
            if signum == 0 {
                break;
            }
            postsig(p, signum, &ctx);
        }
    }

    // If P_SIGSUSPEND is still set here, then we still need to restore the original sigmask
    // before returning to userspace. Also, this might unmask some pending signals, so we
    // need to check a second time for signals to post.
    if p.p_flag.load(Ordering::Relaxed) & P_SIGSUSPEND != 0 {
        p.p_sigmask.set(p.p_oldmask.get());
        p.p_flag.fetch_and(!P_SIGSUSPEND, Ordering::Relaxed);

        loop {
            let signum = cursig(p, &mut ctx, false);
            if signum == 0 {
                break;
            }
            postsig(p, signum, &ctx);
        }
    }

    // WITNESS_WARN(WARN_PANIC, NULL, "userret: returning"): not configured.

    if let Some(ci) = p.cpu() {
        Machine::ci_schedstate(ci)
            .spc_curpriority
            .store(p.p_usrpri.get(), Ordering::Relaxed);
    }
}

/// `proc_suspend_check_locked`: suspend (or, `deep`, report how to unwind) the thread `p`
/// while its process is being single-threaded or stopped.
pub fn proc_suspend_check_locked(p: &Proc, deep: bool) -> Result<(), Errno> {
    let pr = p.process();

    mutex_assert_locked(&pr.ps_mtx, "proc_suspend_check_locked");

    let single = pr.ps_single.get();
    if (single.is_null() || ptr::eq(single, p))
        && pr.ps_flags.load(Ordering::Relaxed) & PS_STOPPING == 0
    {
        return Ok(());
    }

    // if we're in deep, we need to unwind to the edge
    if deep {
        let mut err = Ok(());

        if pr.ps_flags.load(Ordering::Relaxed) & (PS_SINGLEUNWIND | PS_SINGLEEXIT) != 0 {
            return Err(Errno::ERESTART);
        }
        sched_lock();
        if p.p_stat.get() != SSTOP {
            err = Err(Errno::EWOULDBLOCK);
        }
        sched_unlock();
        return err;
    }

    loop {
        if pr.ps_flags.load(Ordering::Relaxed) & PS_SINGLEEXIT != 0 {
            mtx_leave(&pr.ps_mtx);
            kernel_lock(); // KERNEL_LOCK()
            exit1(p, 0, 0, EXIT_THREAD_NOCHECK);
            // NOTREACHED
        }

        sched_lock();
        proc_stop_setup(p);
        sched_unlock();
        process_suspend_signal(pr);

        // not exiting and don't need to unwind, so suspend
        proc_stop_finish(p);

        if pr.ps_single.get().is_null() && pr.ps_flags.load(Ordering::Relaxed) & PS_STOPPING == 0 {
            break;
        }
    }

    Ok(())
}

/// `proc_suspend_check`: `proc_suspend_check_locked` under the process's `ps_mtx`.
pub fn proc_suspend_check(p: &Proc, deep: bool) -> Result<(), Errno> {
    let pr = p.process();

    mtx_enter(&pr.ps_mtx);
    let error = proc_suspend_check_locked(p, deep);
    mtx_leave(&pr.ps_mtx);

    error
}

/// `single_thread_set`: stop other threads in the process. The mode controls how and where
/// the other threads should stop:
/// - `SINGLE_SUSPEND`: stop wherever they are, will later be released (via
///   `single_thread_clear()`)
/// - `SINGLE_UNWIND`: just unwind to kernel boundary, will be told to exit (by setting to
///   `SINGLE_EXIT`) or released as with `SINGLE_SUSPEND`
/// - `SINGLE_EXIT`: unwind to kernel boundary and exit
pub fn single_thread_set(p: &Proc, flags: i32) -> Result<(), Errno> {
    let pr = p.process();
    let mode = flags & SINGLE_MASK;

    kassert!(is_curproc(p));

    mtx_enter(&pr.ps_mtx);
    if let Err(error) = proc_suspend_check_locked(p, flags & SINGLE_DEEP != 0) {
        mtx_leave(&pr.ps_mtx);
        return Err(error);
    }

    match mode {
        SINGLE_SUSPEND => {}
        SINGLE_UNWIND => {
            pr.ps_flags.fetch_or(PS_SINGLEUNWIND, Ordering::Relaxed);
        }
        SINGLE_EXIT => {
            pr.ps_flags.fetch_or(PS_SINGLEEXIT, Ordering::Relaxed);
            pr.ps_flags.fetch_and(!PS_SINGLEUNWIND, Ordering::Relaxed);
        }
        _ => {
            #[cfg(feature = "diagnostic")]
            #[allow(clippy::panic)] // the C's DIAGNOSTIC panic
            panic(format_args!("single_thread_mode = {mode}"));
        }
    }
    pr.ps_single.set(p);

    sched_lock();
    process_stop(pr, P_SUSPSINGLE, mode);
    sched_unlock();

    // count ourself out
    pr.ps_suspendcnt.set(pr.ps_suspendcnt.get().wrapping_sub(1));

    // wait until all other threads suspended
    while pr.ps_suspendcnt.get() > 0 {
        let _ = msleep_nsec(
            ptr::addr_of!(pr.ps_suspendcnt),
            &pr.ps_mtx,
            PWAIT,
            "suspend",
            INFSLP,
        );
    }
    mtx_leave(&pr.ps_mtx);
    kassert!(
        ptr::eq(pr.ps_single.get(), p) && p.p_flag.load(Ordering::Relaxed) & P_SUSPSINGLE == 0
    );
    Ok(())
}

/// `single_thread_clear`: release the threads `single_thread_set` stopped.
pub fn single_thread_clear(p: &Proc) {
    let pr = p.process();

    kassert!(ptr::eq(pr.ps_single.get(), p));
    kassert!(is_curproc(p));

    mtx_enter(&pr.ps_mtx);
    pr.ps_single.set(ptr::null());
    pr.ps_flags
        .fetch_and(!(PS_SINGLEUNWIND | PS_SINGLEEXIT), Ordering::Relaxed);

    sched_lock();
    process_continue(pr, P_SUSPSINGLE);
    sched_unlock();

    mtx_leave(&pr.ps_mtx);
}

/// `sigio_del`: frees the sigios on `rmlist`.
pub fn sigio_del(rmlist: &Sigiolst) {
    while let Some(sigio) = rmlist.first() {
        // SAFETY: `sigio` is on `rmlist`.
        unsafe { Sigiolst::remove(sigio) };
        // SAFETY: the sigio holds a reference to its credentials, dropped here.
        crfree(unsafe { &*sigio.sio_ucred.get() });
        free(
            NonNull::from(sigio).cast::<u8>(),
            M_SIGIO,
            size_of::<Sigio>(),
        );
    }
}

/// `sigio_unlink`: detaches the sigio of `sir` from its owner and moves it to `rmlist`.
pub fn sigio_unlink(sir: &SigioRef, rmlist: &Sigiolst) {
    mutex_assert_locked(&SIGIO_LOCK, "sigio_unlink");

    // SAFETY: a registered sigio stays allocated until `sigio_del`, after this.
    if let Some(sigio) = unsafe { sir.sir_sigio.get().as_ref() } {
        kassert!(ptr::eq(sigio.sio_myref.get(), sir));
        sir.sir_sigio.set(ptr::null());

        if sigio.sio_pgid.get() > 0 {
            sigio.sio_proc.set(ptr::null());
        } else {
            sigio.sio_pgrp.set(ptr::null());
        }
        // SAFETY: a registered sigio is on its process's or group's list, under
        // `sigio_lock`; it moves to `rmlist`, which outlives it until `sigio_del`.
        unsafe {
            Sigiolst::remove(sigio);
            rmlist.insert_head(sigio);
        }
    }
}

/// `sigio_free`: unregisters `sir`.
pub fn sigio_free(sir: &SigioRef) {
    if sir.sir_sigio.get().is_null() {
        return;
    }

    let rmlist = Sigiolst::new();

    mtx_enter(&SIGIO_LOCK);
    sigio_unlink(sir, &rmlist);
    mtx_leave(&SIGIO_LOCK);

    sigio_del(&rmlist);
}

/// `sigio_freelist`: unregisters every sigio on a process's or group's list.
pub fn sigio_freelist(sigiolst: &Sigiolst) {
    if sigiolst.is_empty() {
        return;
    }

    let rmlist = Sigiolst::new();

    mtx_enter(&SIGIO_LOCK);
    while let Some(sigio) = sigiolst.first() {
        // SAFETY: a sigio's `sio_myref` names the registration that holds it.
        sigio_unlink(unsafe { &*sigio.sio_myref.get() }, &rmlist);
    }
    mtx_leave(&SIGIO_LOCK);

    sigio_del(&rmlist);
}

/// `sigio_setown`: registers the process (`*data` > 0) or process group (< 0, or
/// `TIOCSPGRP`) to receive `SIGIO`/`SIGURG` through `sir`; 0 unregisters.
pub fn sigio_setown(sir: &SigioRef, cmd: u64, data: &i32) -> Result<(), Errno> {
    let Some(p) = curproc() else {
        return Err(Errno::ESRCH);
    };
    let mut pgid: Pid = *data;

    if pgid == 0 {
        sigio_free(sir);
        return Ok(());
    }

    if cmd == TIOCSPGRP {
        if pgid < 0 {
            return Err(Errno::EINVAL);
        }
        pgid = -pgid;
    }

    let Some(mem) = malloc(size_of::<Sigio>(), M_SIGIO, M_WAITOK) else {
        panic(format_args!("sigio_setown: malloc"));
    };
    let new = mem.cast::<Sigio>();
    // SAFETY: a fresh allocation of `size_of::<Sigio>()` bytes, aligned by malloc(9),
    // written once before anything else sees it.
    unsafe { new.as_ptr().write(Sigio::new()) };
    // SAFETY: as above; it lives until `sigio_del` (or the failure path below) frees it.
    let sigio: &'static Sigio = unsafe { new.as_ref() };
    sigio.sio_pgid.set(pgid);
    sigio.sio_ucred.set(crhold(p.ucred()));
    sigio.sio_myref.set(sir);

    let rmlist = Sigiolst::new();

    // The kernel lock, and not sleeping between prfind()/pgfind() and linking of the sigio
    // ensure that the process or process group does not disappear unexpectedly.
    kernel_lock(); // KERNEL_LOCK()
    mtx_enter(&SIGIO_LOCK);

    let result = 'fail: {
        if pgid > 0 {
            let Some(pr) = prfind(pgid) else {
                break 'fail Err(Errno::ESRCH);
            };

            // Policy - Don't allow a process to FSETOWN a process in another session.
            //
            // Remove this test to allow maximum flexibility or restrict FSETOWN to the
            // current process or process group for maximum safety.
            if !ptr::eq(pr.session(), p.process().session()) {
                break 'fail Err(Errno::EPERM);
            }

            if pr.ps_flags.load(Ordering::Relaxed) & PS_EXITING != 0 {
                break 'fail Err(Errno::ESRCH);
            }
            sigio.sio_proc.set(pr);
            // SAFETY: a new sigio on no list; it stays until unlinked by `sigio_unlink`.
            unsafe { pr.ps_sigiolst.insert_head(sigio) };
        } else {
            // if (pgid < 0)
            let Some(pgrp) = pgfind(-pgid) else {
                break 'fail Err(Errno::ESRCH);
            };

            // Policy - Don't allow a process to FSETOWN a process in another session.
            //
            // Remove this test to allow maximum flexibility or restrict FSETOWN to the
            // current process or process group for maximum safety.
            if !ptr::eq(pgrp.pg_session.get(), p.process().session()) {
                break 'fail Err(Errno::EPERM);
            }
            sigio.sio_pgrp.set(pgrp);
            // SAFETY: as above.
            unsafe { pgrp.pg_sigiolst.insert_head(sigio) };
        }
        Ok(())
    };

    if let Err(error) = result {
        // fail:
        mtx_leave(&SIGIO_LOCK);
        kernel_unlock(); // KERNEL_UNLOCK()

        // SAFETY: the reference taken above.
        crfree(unsafe { &*sigio.sio_ucred.get() });
        free(new.cast::<u8>(), M_SIGIO, size_of::<Sigio>());

        return Err(error);
    }

    sigio_unlink(sir, &rmlist);
    sir.sir_sigio.set(sigio);

    mtx_leave(&SIGIO_LOCK);
    kernel_unlock(); // KERNEL_UNLOCK()

    sigio_del(&rmlist);

    Ok(())
}

/// `sigio_getown`: the pgid registered through `sir` (negated for `TIOCGPGRP`).
pub fn sigio_getown(sir: &SigioRef, cmd: u64, data: &mut i32) {
    let mut pgid: Pid = 0;

    mtx_enter(&SIGIO_LOCK);
    // SAFETY: a registered sigio stays allocated while `sigio_lock` is held.
    if let Some(sigio) = unsafe { sir.sir_sigio.get().as_ref() } {
        pgid = sigio.sio_pgid.get();
    }
    mtx_leave(&SIGIO_LOCK);

    if cmd == TIOCGPGRP {
        pgid = -pgid;
    }

    *data = pgid;
}

/// `sigio_copy`: registers the owner of `src` in `dst` too.
pub fn sigio_copy(dst: &SigioRef, src: &SigioRef) {
    sigio_free(dst);

    if src.sir_sigio.get().is_null() {
        return;
    }

    let Some(mem) = malloc(size_of::<Sigio>(), M_SIGIO, M_WAITOK) else {
        panic(format_args!("sigio_copy: malloc"));
    };
    let new = mem.cast::<Sigio>();
    // SAFETY: a fresh allocation, written once before anything else sees it.
    unsafe { new.as_ptr().write(Sigio::new()) };
    // SAFETY: as above.
    let newsigio: &'static Sigio = unsafe { new.as_ref() };
    let rmlist = Sigiolst::new();

    mtx_enter(&SIGIO_LOCK);

    // SAFETY: a registered sigio stays allocated while `sigio_lock` is held.
    let Some(sigio) = (unsafe { src.sir_sigio.get().as_ref() }) else {
        mtx_leave(&SIGIO_LOCK);
        free(new.cast::<u8>(), M_SIGIO, size_of::<Sigio>());
        return;
    };

    newsigio.sio_pgid.set(sigio.sio_pgid.get());
    // SAFETY: the source holds a reference to its credentials.
    newsigio
        .sio_ucred
        .set(crhold(unsafe { &*sigio.sio_ucred.get() }));
    newsigio.sio_myref.set(dst);
    if newsigio.sio_pgid.get() > 0 {
        newsigio.sio_proc.set(sigio.sio_proc.get());
        // SAFETY: the owner is alive (its sigio is registered); the new sigio is on no
        // list.
        unsafe { (*newsigio.sio_proc.get()).ps_sigiolst.insert_head(newsigio) };
    } else {
        newsigio.sio_pgrp.set(sigio.sio_pgrp.get());
        // SAFETY: as above, for the process group.
        unsafe { (*newsigio.sio_pgrp.get()).pg_sigiolst.insert_head(newsigio) };
    }

    sigio_unlink(dst, &rmlist);
    dst.sir_sigio.set(newsigio);

    mtx_leave(&SIGIO_LOCK);

    sigio_del(&rmlist);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    use crate::sys::proc::Pgrp;
    use crate::sys::signal::{SIGEMT, SIGINFO, SIGIO, SIGQUIT, SIGTHR, SIGTRAP, SIGURG, SIGWINCH};
    use crate::sys::signalvar::SA_CANTMASK;

    /// A process with one thread, its own `sigacts` and a process group, laid out by the test
    /// (nothing here is linked into the kernel's lists).
    struct World {
        pr: Process,
        p: Proc,
        ps: Sigacts,
        pg: Pgrp,
    }

    impl World {
        fn new(pid: Pid) -> std::boxed::Box<Self> {
            let w = std::boxed::Box::new(World {
                pr: Process::new(),
                p: Proc::new(),
                ps: Sigacts::new(),
                pg: Pgrp::new(),
            });
            w.pr.ps_pid.set(pid);
            w.pr.ps_sigacts.set(&w.ps);
            w.pg.pg_jobc.set(1);
            w.pr.ps_pgrp.set(&w.pg);
            w.p.p_p.set(&w.pr);
            w
        }
    }

    #[test]
    fn sigprop_is_the_c_table() {
        assert_eq!(SIGPROP.len(), NSIG as usize);
        assert_eq!(SIGPROP[0], 0);
        for sig in [
            SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGEMT, SIGFPE, SIGBUS, SIGSEGV, SIGSYS,
        ] {
            assert_eq!(SIGPROP[sig as usize], SA_KILL | SA_CORE, "signal {sig}");
        }
        for sig in [SIGURG, SIGCHLD, SIGIO, SIGWINCH, SIGINFO, SIGTHR] {
            assert_eq!(SIGPROP[sig as usize], SA_IGNORE, "signal {sig}");
        }
        assert_eq!(SIGPROP[SIGCONT as usize], SA_IGNORE | SA_CONT);
        assert_eq!(SIGPROP[SIGSTOP as usize], SA_STOP);
        for sig in [SIGTSTP, SIGTTIN, SIGTTOU] {
            assert_eq!(SIGPROP[sig as usize], SA_STOP | SA_TTYSTOP, "signal {sig}");
        }
        // The stop signals are exactly STOPSIGMASK; no signal is SA_CANTMASK in this table.
        let stops = (1..NSIG)
            .filter(|&s| SIGPROP[s as usize] & SA_STOP != 0)
            .fold(0, |m, s| m | sigmask(s));
        assert_eq!(stops, STOPSIGMASK);
        assert!(SIGPROP.iter().all(|&p| p & SA_CANTMASK == 0));
    }

    #[test]
    fn ffs_counts_from_one() {
        assert_eq!(ffs(0), 0);
        assert_eq!(ffs(1), 1);
        assert_eq!(ffs(sigmask(SIGUSR1) | sigmask(SIGUSR2)), SIGUSR1);
        assert_eq!(ffs(0x8000_0000), 32);
    }

    #[test]
    fn initsiginfo_fills_user_and_fault_members() {
        let si = initsiginfo(SIGUSR1, 0, SI_USER, Sigval::from_int(7));
        assert_eq!((si.si_signo, si.si_code), (SIGUSR1, SI_USER));
        assert_eq!(si.si_value().sival_int(), 7);

        let si = initsiginfo(SIGSEGV, 14, 2, Sigval::from_ptr(0x1000));
        assert_eq!((si.si_signo, si.si_code), (SIGSEGV, 2));
        assert_eq!(si.si_addr(), 0x1000);
        assert_eq!(si.si_trapno(), 14);

        // Kernel codes of other signals carry nothing more.
        let si = initsiginfo(SIGXFSZ, 3, 1, Sigval::from_ptr(0x1000));
        assert_eq!(si.si_addr(), 0);
    }

    #[test]
    fn siginit_ignores_what_is_ignored_by_default_but_sigcont() {
        let ps = Sigacts::new();
        siginit(&ps);
        let ignored = ps.ps_sigignore.get();
        assert_ne!(ignored & sigmask(SIGCHLD), 0);
        assert_ne!(ignored & sigmask(SIGURG), 0);
        assert_eq!(ignored & sigmask(SIGCONT), 0);
        assert_eq!(ignored & sigmask(SIGKILL), 0);
        assert_eq!(
            ps.ps_sigflags.load(Ordering::Relaxed),
            SAS_NOCLDWAIT | SAS_NOCLDSTOP
        );
    }

    #[test]
    fn setsigvec_records_the_action_and_execsigs_resets_it() {
        let w = World::new(5);
        let ps = &w.ps;

        let mut sa = Sigaction {
            sa_handler: 0x4000,
            sa_mask: sigmask(SIGHUP) | sigmask(SIGKILL),
            sa_flags: SA_RESTART | SA_SIGINFO,
        };
        setsigvec(&w.p, SIGUSR1, &mut sa);
        let bit = sigmask(SIGUSR1);
        assert_eq!(ps.ps_sigact[SIGUSR1 as usize].get(), 0x4000);
        // The signal blocks itself (no SA_NODEFER); SIGKILL can never be masked.
        assert_eq!(
            ps.ps_catchmask[SIGUSR1 as usize].get(),
            sigmask(SIGHUP) | bit
        );
        assert_ne!(ps.ps_sigcatch.get() & bit, 0);
        assert_ne!(ps.ps_siginfo.get() & bit, 0);
        assert_eq!(ps.ps_sigintr.get() & bit, 0);
        assert_eq!(ps.ps_sigignore.get() & bit, 0);

        // SIG_IGN for SIGCHLD ignores it and asks for no zombies (init does not exist here).
        w.pr.ps_siglist.store(sigmask(SIGCHLD), Ordering::Relaxed);
        let mut ign = Sigaction {
            sa_handler: SIG_IGN,
            ..Sigaction::default()
        };
        setsigvec(&w.p, SIGCHLD, &mut ign);
        assert_ne!(ps.ps_sigignore.get() & sigmask(SIGCHLD), 0);
        assert_eq!(w.pr.ps_siglist.load(Ordering::Relaxed), 0);
        assert_ne!(ps.ps_sigflags.load(Ordering::Relaxed) & SAS_NOCLDWAIT, 0);

        execsigs(&w.p);
        assert_eq!(ps.ps_sigact[SIGUSR1 as usize].get(), SIG_DFL);
        assert_eq!(ps.ps_sigcatch.get(), 0);
        assert_eq!(ps.ps_sigact[SIGCHLD as usize].get(), SIG_DFL);
        assert_eq!(ps.ps_sigflags.load(Ordering::Relaxed) & SAS_NOCLDWAIT, 0);
        assert_eq!(w.p.p_sigstk.get().ss_flags, SS_DISABLE);
    }

    #[test]
    fn cursig_picks_the_lowest_deliverable_signal() {
        let w = World::new(5);
        siginit(&w.ps);
        let mut ctx = Sigctx::default();

        // Nothing pending.
        assert_eq!(cursig(&w.p, &mut ctx, false), 0);

        // A caught SIGUSR2, a masked SIGHUP and an ignored-by-default SIGCHLD.
        let mut sa = Sigaction {
            sa_handler: 0x4000,
            ..Sigaction::default()
        };
        setsigvec(&w.p, SIGUSR2, &mut sa);
        w.p.p_sigmask.set(sigmask(SIGHUP));
        w.pr.ps_siglist.store(
            sigmask(SIGHUP) | sigmask(SIGUSR2) | sigmask(SIGCHLD),
            Ordering::Relaxed,
        );
        // SIGCHLD is pending although ignored (it was posted before), so cursig drops it and
        // returns the caught SIGUSR2; the masked SIGHUP stays pending.
        assert_eq!(cursig(&w.p, &mut ctx, false), SIGUSR2);
        assert_eq!(ctx.sig_action, 0x4000);
        assert!(ctx.sig_catch);
        // The taken signal moved to the thread's list for postsig.
        assert_eq!(w.p.p_siglist.load(Ordering::Relaxed), sigmask(SIGUSR2));
        assert_eq!(w.pr.ps_siglist.load(Ordering::Relaxed), sigmask(SIGHUP));

        // Unmasking SIGHUP (default action: kill) makes it the next one, ahead of SIGUSR2.
        w.p.p_sigmask.set(0);
        assert_eq!(cursig(&w.p, &mut ctx, false), SIGHUP);
        assert_eq!(ctx.sig_action, SIG_DFL);
        assert_eq!(sigpending(&w.p), sigmask(SIGHUP) | sigmask(SIGUSR2));
    }

    #[test]
    fn cursig_ignores_default_actions_for_init() {
        let w = World::new(1);
        let mut ctx = Sigctx::default();
        w.pr.ps_siglist.store(sigmask(SIGTERM), Ordering::Relaxed);
        assert_eq!(cursig(&w.p, &mut ctx, false), 0);
        assert_eq!(sigpending(&w.p), 0);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn sigprop_matches_kern_sig_c() {
        let path = crate::reftest::openbsd_src().join("sys/kern/kern_sig.c");
        let text = std::fs::read_to_string(&path).expect("kern_sig.c");
        let start = text.find("const int sigprop[NSIG] = {").expect("sigprop");
        let body = &text[start..];
        let body = &body[body.find('{').expect("{") + 1..body.find("};").expect("};")];
        let rows: std::vec::Vec<i32> = body
            .lines()
            .filter_map(|l| {
                let v = l.split("/*").next()?.trim().trim_end_matches(',');
                if v.is_empty() {
                    return None;
                }
                Some(v.split('|').fold(0, |m, f| {
                    m | match f.trim() {
                        "0" => 0,
                        "SA_KILL" => SA_KILL,
                        "SA_CORE" => SA_CORE,
                        "SA_STOP" => SA_STOP,
                        "SA_TTYSTOP" => SA_TTYSTOP,
                        "SA_IGNORE" => SA_IGNORE,
                        "SA_CONT" => SA_CONT,
                        other => panic!("unknown property {other}"),
                    }
                }))
            })
            .collect();
        assert_eq!(rows.as_slice(), &SIGPROP[..]);
    }
}
/* </TESTS> */
