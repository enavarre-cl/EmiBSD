/* <LICENSES> */
/*
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Ralph Campbell.
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
 *	@(#)signal.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! arm64 `<machine/signal.h>`: `sig_atomic_t` and `struct sigcontext`.
//!
//! Upstream: sys/arch/arm64/include/signal.h @ 3ce1f3f79392
//!
//! Status: `ported` (with `kern_sig.c`).

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
    /// `__sc_unused`.
    pub __sc_unused: i32,
    /// `sc_mask`: signal mask to restore.
    pub sc_mask: i32,

    /// `sc_sp`.
    pub sc_sp: u64,
    /// `sc_lr`.
    pub sc_lr: u64,
    /// `sc_elr`.
    pub sc_elr: u64,
    /// `sc_spsr`.
    pub sc_spsr: u64,
    /// `sc_x[30]`.
    pub sc_x: [u64; 30],

    /// `sc_cookie`.
    pub sc_cookie: i64,
}

// SAFETY: `repr(C)`: two 32-bit integers, then 64-bit ones, no padding; every bit pattern is
// a valid value.
unsafe impl AbiPod for Sigcontext {}

const _: () = {
    assert!(size_of::<Sigcontext>() == 8 + 4 * 8 + 30 * 8 + 8);
    assert!(core::mem::offset_of!(Sigcontext, sc_cookie) == 8 + 34 * 8);
};
/* </CODE> */
