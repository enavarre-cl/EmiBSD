/*	$OpenBSD: wait.h,v 1.20 2022/12/19 00:22:11 guenther Exp $	*/
/*	$NetBSD: wait.h,v 1.11 1996/04/09 20:55:51 cgd Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1993, 1994
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
 *	@(#)wait.h	8.2 (Berkeley) 7/10/94
 */
/* </LICENSES> */

//! `<sys/wait.h>`: the status words and options of `wait4(2)` and `waitid(2)`.
//!
//! Upstream: sys/sys/wait.h @ 3ce1f3f79392
//!
//! Status: `ported`. The status macros are `const fn`s with the C's names in lower case
//! (`_WSTATUS` is `wstatus`), the options and `WAIT_*` tokens constants, `idtype_t` an
//! `i32` with the `P_*` values (the system call passes it as an `int`). The user-space
//! prototypes (`wait`, `waitpid`, ...) are libc's.

/// `idtype_t`: what `waitid(2)`'s `id` names.
pub type Idtype = i32;

/// `_WSTATUS(x)`.
pub const fn wstatus(x: i32) -> i32 {
    x & 0o177
}

/// `_WSTOPPED`: `_WSTATUS` if process is stopped.
pub const _WSTOPPED: i32 = 0o177;
/// `_WCONTINUED`: process has continued.
pub const _WCONTINUED: i32 = 0o177777;

/// `WIFSTOPPED(x)`.
pub const fn wifstopped(x: i32) -> bool {
    (x & 0xff) == _WSTOPPED
}

/// `WSTOPSIG(x)`.
pub const fn wstopsig(x: i32) -> i32 {
    ((x as u32 >> 8) & 0xff) as i32
}

/// `WIFSIGNALED(x)`.
pub const fn wifsignaled(x: i32) -> bool {
    wstatus(x) != _WSTOPPED && wstatus(x) != 0
}

/// `WTERMSIG(x)`.
pub const fn wtermsig(x: i32) -> i32 {
    wstatus(x)
}

/// `WIFEXITED(x)`.
pub const fn wifexited(x: i32) -> bool {
    wstatus(x) == 0
}

/// `WEXITSTATUS(x)`.
pub const fn wexitstatus(x: i32) -> i32 {
    ((x as u32 >> 8) & 0xff) as i32
}

/// `WIFCONTINUED(x)`.
pub const fn wifcontinued(x: i32) -> bool {
    (x & _WCONTINUED) == _WCONTINUED
}

/// `WCOREFLAG`.
pub const WCOREFLAG: i32 = 0o200;

/// `WCOREDUMP(x)`.
pub const fn wcoredump(x: i32) -> bool {
    x & WCOREFLAG != 0
}

/// `W_EXITCODE(ret, sig)`.
pub const fn w_exitcode(ret: i32, sig: i32) -> i32 {
    (ret << 8) | sig
}

/// `W_STOPCODE(sig)`.
pub const fn w_stopcode(sig: i32) -> i32 {
    (sig << 8) | _WSTOPPED
}

/// `WNOHANG`: don't hang in wait.
pub const WNOHANG: i32 = 0x01;
/// `WUNTRACED`: report stopped-by-signal processes.
pub const WUNTRACED: i32 = 0x02;
/// `WCONTINUED`: report job control continued processes.
pub const WCONTINUED: i32 = 0x08;
/// `WEXITED`: report exited processes.
pub const WEXITED: i32 = 0x04;
/// `WSTOPPED`.
pub const WSTOPPED: i32 = WUNTRACED;
/// `WNOWAIT`: poll only.
pub const WNOWAIT: i32 = 0x10;
/// `WTRAPPED`: report stopped-by-tracing processes.
pub const WTRAPPED: i32 = 0x20;

/// `P_ALL`.
pub const P_ALL: Idtype = 0;
/// `P_PGID`.
pub const P_PGID: Idtype = 1;
/// `P_PID`.
pub const P_PID: Idtype = 2;

/// `WAIT_ANY`: any process.
pub const WAIT_ANY: i32 = -1;
/// `WAIT_MYPGRP`: any process in my process group.
pub const WAIT_MYPGRP: i32 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_words() {
        let exited = w_exitcode(3, 0);
        assert!(wifexited(exited) && !wifsignaled(exited) && wexitstatus(exited) == 3);
        let killed = w_exitcode(0, 6);
        assert!(wifsignaled(killed) && wtermsig(killed) == 6 && !wcoredump(killed));
        assert!(wcoredump(w_exitcode(0, 6 | WCOREFLAG)));
        let stopped = w_stopcode(17);
        assert!(wifstopped(stopped) && wstopsig(stopped) == 17);
        assert!(wifcontinued(_WCONTINUED));
    }
}
