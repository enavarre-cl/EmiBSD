/*	$OpenBSD: signal.h,v 1.9 2016/05/10 18:39:42 deraadt Exp $	*/
/*	$NetBSD: signal.h,v 1.2 2003/04/28 23:16:17 bjh21 Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1991 Regents of the University of California.
 * All rights reserved.
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
 *	@(#)signal.h	7.16 (Berkeley) 3/17/91
 */
/* </LICENSES> */

//! amd64 `<machine/signal.h>`: `sig_atomic_t` and `struct sigcontext`.
//!
//! Upstream: sys/arch/amd64/include/signal.h @ 3ce1f3f79392
//!
//! Status: `ported` (with `kern_sig.c`).
//!
//! ## Deviations
//! - `sc_fpstate` (`struct fxsave64 *`) is a user address, a `usize`.

use crate::machine::copy::AbiPod;

/// `sig_atomic_t`.
pub type SigAtomic = i32;

/// `struct sigcontext`: information pushed on stack when a signal is delivered. This is used
/// by the kernel to restore state following execution of the signal handler. It is also made
/// available to the handler to allow it to restore state properly if a non-standard exit is
/// performed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sigcontext {
    // plain match trapframe
    /// `sc_rdi`.
    pub sc_rdi: i64,
    /// `sc_rsi`.
    pub sc_rsi: i64,
    /// `sc_rdx`.
    pub sc_rdx: i64,
    /// `sc_rcx`.
    pub sc_rcx: i64,
    /// `sc_r8`.
    pub sc_r8: i64,
    /// `sc_r9`.
    pub sc_r9: i64,
    /// `sc_r10`.
    pub sc_r10: i64,
    /// `sc_r11`.
    pub sc_r11: i64,
    /// `sc_r12`.
    pub sc_r12: i64,
    /// `sc_r13`.
    pub sc_r13: i64,
    /// `sc_r14`.
    pub sc_r14: i64,
    /// `sc_r15`.
    pub sc_r15: i64,
    /// `sc_rbp`.
    pub sc_rbp: i64,
    /// `sc_rbx`.
    pub sc_rbx: i64,
    /// `sc_rax`.
    pub sc_rax: i64,
    /// `sc_gs`.
    pub sc_gs: i64,
    /// `sc_fs`.
    pub sc_fs: i64,
    /// `sc_es`.
    pub sc_es: i64,
    /// `sc_ds`.
    pub sc_ds: i64,
    /// `sc_trapno`.
    pub sc_trapno: i64,
    /// `sc_err`.
    pub sc_err: i64,
    /// `sc_rip`.
    pub sc_rip: i64,
    /// `sc_cs`.
    pub sc_cs: i64,
    /// `sc_rflags`.
    pub sc_rflags: i64,
    /// `sc_rsp`.
    pub sc_rsp: i64,
    /// `sc_ss`.
    pub sc_ss: i64,

    /// `sc_fpstate`: the user address of the saved `struct fxsave64`.
    pub sc_fpstate: usize,
    /// `__sc_unused`.
    pub __sc_unused: i32,
    /// `sc_mask`.
    pub sc_mask: i32,
    /// `sc_cookie`.
    pub sc_cookie: i64,
}

// SAFETY: `repr(C)`: 64-bit integers and two 32-bit ones in a pair, no padding; every bit
// pattern is a valid value.
unsafe impl AbiPod for Sigcontext {}

const _: () = {
    assert!(size_of::<Sigcontext>() == 26 * 8 + 8 + 4 + 4 + 8);
    assert!(core::mem::offset_of!(Sigcontext, sc_mask) == 26 * 8 + 12);
};
