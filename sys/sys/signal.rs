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

//! `<sys/signal.h>`: the signal numbers.
//!
//! Upstream: sys/sys/signal.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M6 (part b) needs the numbers (`exit1` takes one, `exec`
//! aborts with `SIGABRT`, the traps will kill with `SIGSEGV`/`SIGBUS`/`SIGILL`). `sigset_t`,
//! `struct sigaction`, `SA_*`, `SIG_DFL`/`SIG_IGN`, `siginfo_t`, the `SIG_*` codes and
//! `struct sigaltstack` arrive with `kern_sig.c` (M6-c).

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

#[cfg(test)]
mod tests {
    use super::*;

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
            ("SIGUSR2", SIGUSR2 as i64),
            ("SIGTHR", SIGTHR as i64),
            ("_NSIG", _NSIG as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
