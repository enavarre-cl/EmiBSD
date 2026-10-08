/*	$OpenBSD: signal.h,v 1.30 2026/03/21 01:56:51 daniel Exp $	*/
/*	$NetBSD: signal.h,v 1.21 1996/02/09 18:25:32 christos Exp $	*/
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
 *	@(#)signal.h	8.2 (Berkeley) 1/21/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/signal.h>`: the signal numbers, `sigset_t`, `struct sigaction`, the `SA_*` and
//! `SIG_*` values and `struct sigaltstack`.
//!
//! Upstream: sys/sys/signal.h @ 3ce1f3f79392
//!
//! Status: `ported`. Milestone M6 (part b) brought the numbers; `kern_sig.c` the rest:
//! `sigset_t` ([`Sigset`]), `sig_t` ([`Sig`]), `SIG_DFL`/`SIG_IGN`/`SIG_ERR`,
//! `struct sigaction`, the `SA_*` flags, `SIG_BLOCK`/`SIG_UNBLOCK`/`SIG_SETMASK`,
//! `struct sigvec` and `SV_*`, `sigmask()`, `struct sigaltstack` (`stack_t`), `SS_*`,
//! `MINSIGSTKSZ`/`SIGSTKSZ` and `ucontext_t` (the machine's `struct sigcontext`,
//! `machine::signal`). `siginfo_t` is `<sys/siginfo.h>` (`siginfo.rs`). The userland
//! `signal()` prototype is not kernel code.
//!
//! ## Deviations
//! - A handler (`sig_t`, `void (*)(int)`) is a user address the kernel never calls: [`Sig`]
//!   is a `usize`, `SIG_DFL`/`SIG_IGN`/`SIG_ERR` are 0, 1 and -1 as that integer. The union
//!   `__sigaction_u` is one field, `sa_handler`; [`Sigaction::sa_sigaction`] reads the same
//!   word, as the C's two names for one union member do.
//! - `struct sigaltstack` spells out its tail padding (`_pad`) so the structure has no
//!   implicit padding and can be copied in and out as it is (`AbiPod`).
//! - `MINSIGSTKSZ`/`SIGSTKSZ` are `usize` constants computed from the machine's
//!   `_MAX_PAGE_SHIFT`.

use crate::machine::copy::AbiPod;
use crate::machine::param::MachineParam;
use crate::machine::{Machine, MachineSignal};

/// `_NSIG`: counting 0 (mask is 1-32).
pub const _NSIG: i32 = 33;
/// `NSIG`.
pub const NSIG: i32 = _NSIG;

/// `SIGHUP`: hangup.
pub const SIGHUP: i32 = 1;
/// `SIGINT`: interrupt.
pub const SIGINT: i32 = 2;
/// `SIGQUIT`: quit.
pub const SIGQUIT: i32 = 3;
/// `SIGILL`: illegal instruction (not reset when caught).
pub const SIGILL: i32 = 4;
/// `SIGTRAP`: trace trap (not reset when caught).
pub const SIGTRAP: i32 = 5;
/// `SIGABRT`: abort().
pub const SIGABRT: i32 = 6;
/// `SIGIOT`: compatibility.
pub const SIGIOT: i32 = SIGABRT;
/// `SIGEMT`: EMT instruction.
pub const SIGEMT: i32 = 7;
/// `SIGFPE`: floating point exception.
pub const SIGFPE: i32 = 8;
/// `SIGKILL`: kill (cannot be caught or ignored).
pub const SIGKILL: i32 = 9;
/// `SIGBUS`: bus error.
pub const SIGBUS: i32 = 10;
/// `SIGSEGV`: segmentation violation.
pub const SIGSEGV: i32 = 11;
/// `SIGSYS`: bad argument to system call.
pub const SIGSYS: i32 = 12;
/// `SIGPIPE`: write on a pipe with no one to read it.
pub const SIGPIPE: i32 = 13;
/// `SIGALRM`: alarm clock.
pub const SIGALRM: i32 = 14;
/// `SIGTERM`: software termination signal from kill.
pub const SIGTERM: i32 = 15;
/// `SIGURG`: urgent condition on IO channel.
pub const SIGURG: i32 = 16;
/// `SIGSTOP`: sendable stop signal not from tty.
pub const SIGSTOP: i32 = 17;
/// `SIGTSTP`: stop signal from tty.
pub const SIGTSTP: i32 = 18;
/// `SIGCONT`: continue a stopped process.
pub const SIGCONT: i32 = 19;
/// `SIGCHLD`: to parent on child stop or exit.
pub const SIGCHLD: i32 = 20;
/// `SIGTTIN`: to readers pgrp upon background tty read.
pub const SIGTTIN: i32 = 21;
/// `SIGTTOU`: like TTIN for output if (tp->t_local&LTOSTOP).
pub const SIGTTOU: i32 = 22;
/// `SIGIO`: input/output possible signal.
pub const SIGIO: i32 = 23;
/// `SIGXCPU`: exceeded CPU time limit.
pub const SIGXCPU: i32 = 24;
/// `SIGXFSZ`: exceeded file size limit.
pub const SIGXFSZ: i32 = 25;
/// `SIGVTALRM`: virtual time alarm.
pub const SIGVTALRM: i32 = 26;
/// `SIGPROF`: profiling time alarm.
pub const SIGPROF: i32 = 27;
/// `SIGWINCH`: window size changes.
pub const SIGWINCH: i32 = 28;
/// `SIGINFO`: information request.
pub const SIGINFO: i32 = 29;
/// `SIGUSR1`: user defined signal 1.
pub const SIGUSR1: i32 = 30;
/// `SIGUSR2`: user defined signal 2.
pub const SIGUSR2: i32 = 31;
/// `SIGTHR`: thread library AST.
pub const SIGTHR: i32 = 32;

/// `sig_t`: type of signal function, `void (*)(int)`: a user address (see the module's
/// deviations).
pub type Sig = usize;

/// `SIG_DFL`: the default action.
pub const SIG_DFL: Sig = 0;
/// `SIG_IGN`: ignore the signal.
pub const SIG_IGN: Sig = 1;
/// `SIG_ERR`: `(void (*)(int))-1`.
pub const SIG_ERR: Sig = usize::MAX;

/// `sigset_t`: a set of signals, bit `n - 1` for signal `n`.
pub type Sigset = u32;

/// `struct sigaction`: signal vector "template" used in sigaction call.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sigaction {
    /// `__sigaction_u` (`sa_handler`, or `sa_sigaction` with `SA_SIGINFO`): signal handler.
    pub sa_handler: Sig,
    /// `sa_mask`: signal mask to apply.
    pub sa_mask: Sigset,
    /// `sa_flags`: see signal options below.
    pub sa_flags: i32,
}

impl Sigaction {
    /// `sa_sigaction`: if `SA_SIGINFO` is set, `sa_sigaction` is to be used instead of
    /// `sa_handler`; both name the one union member.
    pub const fn sa_sigaction(&self) -> Sig {
        self.sa_handler
    }
}

// SAFETY: `repr(C)`: a pointer-sized integer and two 32-bit ones, no padding; every bit
// pattern is a valid value.
unsafe impl AbiPod for Sigaction {}

/// `SA_ONSTACK`: take signal on signal stack.
pub const SA_ONSTACK: i32 = 0x0001;
/// `SA_RESTART`: restart system on signal return.
pub const SA_RESTART: i32 = 0x0002;
/// `SA_RESETHAND`: reset to SIG_DFL when taking signal.
pub const SA_RESETHAND: i32 = 0x0004;
/// `SA_NODEFER`: don't mask the signal we're delivering.
pub const SA_NODEFER: i32 = 0x0010;
/// `SA_NOCLDWAIT`: don't create zombies (assign to pid 1).
pub const SA_NOCLDWAIT: i32 = 0x0020;
/// `SA_NOCLDSTOP`: do not generate SIGCHLD on child stop.
pub const SA_NOCLDSTOP: i32 = 0x0008;
/// `SA_SIGINFO`: generate siginfo_t.
pub const SA_SIGINFO: i32 = 0x0040;

/// `SIG_BLOCK`: block specified signal set.
pub const SIG_BLOCK: i32 = 1;
/// `SIG_UNBLOCK`: unblock specified signal set.
pub const SIG_UNBLOCK: i32 = 2;
/// `SIG_SETMASK`: set specified signal set.
pub const SIG_SETMASK: i32 = 3;

/// `struct sigvec`: 4.3 compatibility: signal vector "template" used in sigvec call.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sigvec {
    /// `sv_handler`: signal handler.
    pub sv_handler: Sig,
    /// `sv_mask`: signal mask to apply.
    pub sv_mask: i32,
    /// `sv_flags` (also `sv_onstack`): see signal options below.
    pub sv_flags: i32,
}

/// `SV_ONSTACK`.
pub const SV_ONSTACK: i32 = SA_ONSTACK;
/// `SV_INTERRUPT`: same bit, opposite sense.
pub const SV_INTERRUPT: i32 = SA_RESTART;
/// `SV_RESETHAND`.
pub const SV_RESETHAND: i32 = SA_RESETHAND;

/// `sigmask(m)`: macro for converting signal number to a mask suitable for sigblock().
/// The shift wraps as the hardware's does, so `sigmask(0)`, which the C computes on paths
/// that then discard it, does not trap.
pub const fn sigmask(m: i32) -> Sigset {
    1u32.wrapping_shl((m - 1) as u32)
}

/// `BADSIG`.
pub const BADSIG: Sig = SIG_ERR;

/// `struct sigaltstack` (`stack_t`): structure used in sigaltstack call.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sigaltstack {
    /// `ss_sp`: signal stack base (a user address).
    pub ss_sp: usize,
    /// `ss_size`: signal stack length.
    pub ss_size: usize,
    /// `ss_flags`: `SS_DISABLE` and/or `SS_ONSTACK`.
    pub ss_flags: i32,
    /// The C structure's tail padding, spelled out (see the module's deviations).
    pub _pad: i32,
}

// SAFETY: `repr(C)`: two pointer-sized integers and two 32-bit ones, no padding; every bit
// pattern is a valid value.
unsafe impl AbiPod for Sigaltstack {}

/// `stack_t`.
pub type Stack = Sigaltstack;

/// `SS_ONSTACK`: take signals on alternate stack.
pub const SS_ONSTACK: i32 = 0x0001;
/// `SS_DISABLE`: disable taking signals on alternate stack.
pub const SS_DISABLE: i32 = 0x0004;
/// `MINSIGSTKSZ`: minimum allowable stack.
pub const MINSIGSTKSZ: usize = 3 << <Machine as MachineParam>::MAX_PAGE_SHIFT;
/// `SIGSTKSZ`: recommended stack size.
pub const SIGSTKSZ: usize = if <Machine as MachineParam>::MAX_PAGE_SHIFT < 14 {
    MINSIGSTKSZ + (1 << <Machine as MachineParam>::MAX_PAGE_SHIFT) * 4
} else {
    MINSIGSTKSZ + (1 << <Machine as MachineParam>::MAX_PAGE_SHIFT) * 2
};

/// `ucontext_t`: the machine's `struct sigcontext`.
pub type Ucontext = <Machine as MachineSignal>::Sigcontext;

const _: () = {
    assert!(size_of::<Sigaction>() == 16);
    assert!(size_of::<Sigaltstack>() == 24);
    assert!(size_of::<Sigvec>() == 16);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigmask_is_one_bit_per_signal() {
        assert_eq!(sigmask(SIGHUP), 1);
        assert_eq!(sigmask(SIGKILL), 0x100);
        assert_eq!(sigmask(SIGTHR), 0x8000_0000);
        assert_eq!(sigmask(SIGUSR1) | sigmask(SIGUSR2), 0x6000_0000);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/signal.h");
        let ours: &[(&str, i64)] = &[
            ("SIGHUP", SIGHUP as i64),
            ("SIGABRT", SIGABRT as i64),
            ("SIGKILL", SIGKILL as i64),
            ("SIGSEGV", SIGSEGV as i64),
            ("SIGCHLD", SIGCHLD as i64),
            ("SIGUSR1", SIGUSR1 as i64),
            ("SIGUSR2", SIGUSR2 as i64),
            ("SIGTHR", SIGTHR as i64),
            ("_NSIG", _NSIG as i64),
            ("SA_ONSTACK", SA_ONSTACK as i64),
            ("SA_RESTART", SA_RESTART as i64),
            ("SA_RESETHAND", SA_RESETHAND as i64),
            ("SA_NODEFER", SA_NODEFER as i64),
            ("SA_NOCLDWAIT", SA_NOCLDWAIT as i64),
            ("SA_NOCLDSTOP", SA_NOCLDSTOP as i64),
            ("SA_SIGINFO", SA_SIGINFO as i64),
            ("SIG_BLOCK", SIG_BLOCK as i64),
            ("SIG_UNBLOCK", SIG_UNBLOCK as i64),
            ("SIG_SETMASK", SIG_SETMASK as i64),
            ("SS_ONSTACK", SS_ONSTACK as i64),
            ("SS_DISABLE", SS_DISABLE as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
