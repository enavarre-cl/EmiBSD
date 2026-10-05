/*	$OpenBSD: syscall_mi.h,v 1.37 2024/12/27 11:57:16 mpi Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)kern_xxx.c	8.2 (Berkeley) 11/14/93
 */
/* </LICENSES> */

//! `<sys/syscall_mi.h>`: the machine-independent part of a system call, around the
//! machine-dependent entry (`syscall()` on amd64, `svc_handler()` on arm64).
//!
//! Upstream: sys/sys/syscall_mi.h @ 3ce1f3f79392
//!
//! Status: `wip`: `pin_check`, `mi_syscall`, `mi_syscall_return`, `mi_child_return` and
//! `mi_ast`.
//!
//! ## Deviations
//! - `pin_check` is the C's: a system call must come from the call site the executable's
//!   `PT_OPENBSD_SYSCALLS` table (`ps_pin`, loaded by `exec`) or `pinsyscalls(2)`
//!   (`ps_libcpin`) names for its number, or be `sigreturn` from the signal trampoline;
//!   anything else gets `SIGABRT` (`sigabort`). Its message goes to the console with
//!   `printf` where the C's `uprintf` writes to the process's terminal (no tty layer yet).
//! - `mi_syscall` reports the `MAP_STACK` check (`uvm_map_inentry`, with the user map);
//!   `KTRACE` and dt(4) are not configured (`SYSCALL_DEBUG` is the feature `syscall_debug`;
//!   `mi_child_return` does not print the child's return).
//! - `SY_NOLOCK` (M11e): honoured as in C for the system calls whose bodies are audited for
//!   `MULTIPROCESSOR`; a `NOLOCK` system call whose body is not audited yet is listed in
//!   [`SY_NOLOCK_DEFERRED`] and still runs under the kernel lock, with the module (and agent
//!   audit) that unlocks each group. The rest of `mi_syscall` and the return path
//!   (`refreshcreds`, `pin_check`, `pledge_syscall`, `userret`) run unlocked, taking the lock
//!   where the C does.

use core::sync::atomic::Ordering;

use crate::kern::kern_pledge::{pledge_fail, pledge_syscall};
use crate::kern::kern_sig::{sigabort, single_thread_set, userret};
use crate::kern::sched_bsd::preempt;
use crate::kern::subr_prf::Str;
use crate::kprintf;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::machine::signal::MachineSignal;
use crate::sys::acct::APINSYS;
use crate::sys::errno::Errno;
use crate::sys::proc::{
    P_OWEUPC, PS_PLEDGE, Pinsyscall, Proc, SINGLE_DEEP, SINGLE_UNWIND, p_hassibling, refreshcreds,
};
use crate::sys::syscall::*;
use crate::sys::systm::{SY_NOLOCK, SysArgs, Sysent, kernel_lock, kernel_unlock};
use crate::sys::types::Register;
use crate::unported;

/// `NOLOCK` system calls (`syscalls.master`) whose bodies are not audited for
/// `MULTIPROCESSOR`: `mi_syscall` keeps the kernel lock for them although `sysent` marks them
/// `SY_NOLOCK` (see the module's deviations). Each group names what unlocks it; an entry
/// leaves the table when its body's locking is the C's. Since the end of the M11e audit
/// (files, uvm, kern, net and netinet) only the unported calls are left.
pub const SY_NOLOCK_DEFERRED: &[i32] = &[
    // __thrsleep(2) and __thrwakeup(2) are not ported (`sys_nosys`, which posts SIGSYS).
    SYS___thrsleep,
    SYS___thrwakeup,
];

/// [`SY_NOLOCK_DEFERRED`] as a table indexed by system call number.
const SY_NOLOCK_DEFERRED_MAP: [bool; SYS_MAXSYSCALL] = {
    let mut map = [false; SYS_MAXSYSCALL];
    let mut i = 0;
    while i < SY_NOLOCK_DEFERRED.len() {
        map[SY_NOLOCK_DEFERRED[i] as usize] = true;
        i += 1;
    }
    map
};

/// Whether `mi_syscall` runs system call `code` (entry `callp`) under the kernel lock: the C's
/// `!(callp->sy_flags & SY_NOLOCK)`, plus the not yet audited [`SY_NOLOCK_DEFERRED`].
fn syscall_lock(callp: &Sysent, code: Register) -> bool {
    callp.sy_flags & SY_NOLOCK == 0
        || usize::try_from(code)
            .ok()
            .and_then(|code| SY_NOLOCK_DEFERRED_MAP.get(code))
            .is_none_or(|&deferred| deferred)
}

/// Check if a system call is entered from precisely correct location.
#[inline]
pub fn pin_check(p: &Proc, code: Register) -> Result<(), Errno> {
    let pr = p.process();
    let pc = Machine::proc_pc(p);

    // point at start of syscall instruction
    let addr = pc.wrapping_sub(
        <Machine as MachineSignal>::sigcoderet() - <Machine as MachineSignal>::sigcodecall(),
    );
    let ppin = &pr.ps_pin;
    let plibcpin = &pr.ps_libcpin;
    let within = |pin: &Pinsyscall| {
        !pin.pn_pins.get().is_null() && addr >= pin.pn_start.get() && addr < pin.pn_end.get()
    };

    // System calls come from the following places, checks are ordered by most common case:
    // 1) dynamic binary: syscalls in libc.so (in the ps_libcpin region)
    // 2a) static binary: syscalls in main program (in the ps_pin region)
    // 2b) dynamic binary: syscalls in ld.so (in the ps_pin region)
    // 3) sigtramp, containing only sigreturn(2)
    let mut pin: Option<&Pinsyscall> = None;
    let error = 'check: {
        if within(plibcpin) {
            pin = Some(plibcpin);
        } else if within(ppin) {
            pin = Some(ppin);
        } else if pc == pr.ps_sigcoderet.get() {
            if code == SYS_sigreturn as Register {
                return Ok(());
            }
            break 'check Errno::EPERM; // goto die
        }
        match pin {
            Some(pin) => {
                let npins = pin.pn_npins.get().max(0) as usize;
                let slot = usize::try_from(code).ok().filter(|&c| c < npins);
                // SAFETY: a non-null `pn_pins` holds `pn_npins` entries (exec, pinsyscalls),
                // freed only by this process's exec or exit.
                let entry = slot.map(|c| unsafe { pin.pn_pins.get().add(c).read() });
                match entry {
                    None | Some(0) => Errno::ENOSYS,
                    // correct location
                    Some(off) if (off as usize).wrapping_add(pin.pn_start.get()) == addr => {
                        return Ok(());
                    }
                    // multiple locations, hopefully a boring operation
                    Some(u32::MAX) => return Ok(()),
                    Some(_) => Errno::ENOSYS,
                }
            }
            None => Errno::ENOSYS,
        }
    };

    // die:
    // KTRACE (KTR_PINSYSCALL): not configured.
    kernel_lock(); // KERNEL_LOCK()
    let pinoff = pin
        .zip(usize::try_from(code).ok())
        .filter(|(pin, c)| *c < pin.pn_npins.get().max(0) as usize)
        .map_or(-1i64, |(pin, c)| {
            // SAFETY: as above, `c < pn_npins`.
            let entry = unsafe { pin.pn_pins.get().add(c).read() };
            i64::from(entry)
        });
    let is = |which: &Pinsyscall| {
        if pin.is_some_and(|pin| core::ptr::eq(pin, which)) {
            "(Y)"
        } else {
            ""
        }
    };
    // XXX remove or simplify this uprintf() call after OpenBSD 7.5 release
    kprintf!(
        "{}[{}]: pinsyscalls addr {:x} code {}, pinoff {:#x} (pin{} {} {:x}-{:x} {:x}) \
         (libcpin{} {} {:x}-{:x} {:x}) error {}\n",
        Str(pr.comm()),
        pr.ps_pid.get(),
        addr,
        code,
        pinoff as u32,
        is(ppin),
        ppin.pn_npins.get(),
        ppin.pn_start.get(),
        ppin.pn_end.get(),
        ppin.pn_end.get().wrapping_sub(ppin.pn_start.get()),
        is(plibcpin),
        plibcpin.pn_npins.get(),
        plibcpin.pn_start.get(),
        plibcpin.pn_end.get(),
        plibcpin.pn_end.get().wrapping_sub(plibcpin.pn_start.get()),
        error as i32
    );
    pr.ps_acflag.set(pr.ps_acflag.get() | APINSYS);

    // Try to stop threads immediately, because this process is suspect
    if p_hassibling(p) {
        let _ = single_thread_set(p, SINGLE_UNWIND | SINGLE_DEEP);
    }
    // Send uncatchable SIGABRT for coredump
    sigabort(p);
    kernel_unlock(); // KERNEL_UNLOCK()
    Err(error)
}

/// `mi_syscall`: the MD setup for a system call has been done; here's the MI part.
#[inline]
pub fn mi_syscall(
    p: &Proc,
    code: Register,
    callp: &Sysent,
    argp: &SysArgs,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    // SY_NOLOCK_DEFERRED: see the module's deviations.
    let lock = syscall_lock(callp, code);

    // refresh the thread's cache of the process's creds
    refreshcreds(p);

    #[cfg(feature = "syscall_debug")]
    {
        kernel_lock(); // KERNEL_LOCK()
        crate::kern::kern_xxx::scdebug_call(p, code, argp);
        kernel_unlock(); // KERNEL_UNLOCK()
    }
    // dt(4), KTRACE: not configured.

    // SP must be within MAP_STACK space: uvm_map_inentry(p, &p->p_spinentry, PROC_STACK(p),
    // ...) with the user map.
    let _ = unported!("mi_syscall: uvm_map_inentry (the MAP_STACK check)");

    pin_check(p, code)?;

    let pledged = p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE != 0;
    let mut tval = 0;
    if pledged && let Err(error) = pledge_syscall(p, code as i32, &mut tval) {
        kernel_lock(); // KERNEL_LOCK()
        let error = pledge_fail(p, error, tval);
        kernel_unlock(); // KERNEL_UNLOCK()
        return Err(error);
    }
    if lock {
        kernel_lock(); // KERNEL_LOCK()
    }
    let error = (callp.sy_call)(p, argp, retval);
    if lock {
        kernel_unlock(); // KERNEL_UNLOCK()
    }

    error
}

/// `mi_syscall_return`: finish MI stuff on return, after the registers have been set.
#[inline]
pub fn mi_syscall_return(
    p: &Proc,
    code: Register,
    error: Result<(), Errno>,
    retval: &[Register; 2],
) {
    #[cfg(feature = "syscall_debug")]
    {
        kernel_lock(); // KERNEL_LOCK()
        crate::kern::kern_xxx::scdebug_ret(p, code, error.err().map_or(0, |e| e as i32), retval);
        kernel_unlock(); // KERNEL_UNLOCK()
    }
    let _ = (code, error, retval);
    // dt(4), KTRACE: not configured.
    userret(p);
}

/// `mi_child_return`: finish MI stuff for a new process/thread to return.
#[inline]
pub fn mi_child_return(p: &Proc) {
    // TRACEPOINT(sched, on__cpu, NULL), SYSCALL_DEBUG, dt(4), KTRACE: not configured.
    userret(p);
}

/// `mi_ast`: do the specific processing necessary for an AST.
#[inline]
pub fn mi_ast(p: &Proc, resched: bool) {
    if p.p_flag.load(Ordering::Relaxed) & P_OWEUPC != 0 {
        // KERNEL_LOCK(); ADDUPROF(p); KERNEL_UNLOCK(): the profiling clock (subr_prof.c).
        let _ = unported!("mi_ast: ADDUPROF (subr_prof.c)");
    }
    if resched {
        preempt();
    }

    // XXX could move call to userret() here, but hppa calls ast() in syscall return and sh
    // calls it after userret()
}

#[cfg(test)]
mod tests;
