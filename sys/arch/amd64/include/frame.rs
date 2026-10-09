/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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

/*-
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

/*-
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz.
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
 *	@(#)frame.h	5.2 (Berkeley) 1/18/91
 */

/*
 * Adapted for NetBSD/amd64 by fvdl@wasabisystems.com
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/frame.h>`: the stack frames the kernel walks and builds.
//!
//! Upstream: sys/arch/amd64/include/frame.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `struct callframe`, M4 `struct trapframe`, `struct intrframe`
//! and `struct iretq_frame`; `struct switchframe` arrives with `cpu_switchto` (M5). M2's note: `struct callframe`, what the frame-pointer chain is made
//! of (`push rbp; mov rbp, rsp` leaves the caller's frame and the return address at `rbp`).
//! `struct trapframe`, `struct intrframe` and `struct switchframe` arrive with the trap and
//! context-switch code (M4, M5).

/// `struct callframe`: one link of the frame-pointer chain.
#[repr(C)]
pub struct Callframe {
    /// The caller's frame (its saved `rbp`).
    pub f_frame: *const Callframe,
    /// The return address into the caller.
    pub f_retaddr: i64,
    /// The first stack-passed argument slot (the callee's view).
    pub f_arg0: i64,
}

const _: () = {
    assert!(core::mem::size_of::<Callframe>() == 24);
    assert!(core::mem::offset_of!(Callframe, f_retaddr) == 8);
};

/// `struct trapframe`: what a trap or interrupt entry stub builds on the stack. The first six
/// registers are ordered by syscall args; `tf_err` is not at the hardware's position (the
/// entry code moves it up); from `tf_rip` on the hardware pushed it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Trapframe {
    /// `tf_rdi`.
    pub tf_rdi: i64,
    /// `tf_rsi`.
    pub tf_rsi: i64,
    /// `tf_rdx`.
    pub tf_rdx: i64,
    /// `tf_r10`.
    pub tf_r10: i64,
    /// `tf_r8`.
    pub tf_r8: i64,
    /// `tf_r9`: ...to here.
    pub tf_r9: i64,
    /// `tf_rcx`.
    pub tf_rcx: i64,
    /// `tf_r11`.
    pub tf_r11: i64,
    /// `tf_r12`.
    pub tf_r12: i64,
    /// `tf_r13`.
    pub tf_r13: i64,
    /// `tf_r14`.
    pub tf_r14: i64,
    /// `tf_r15`.
    pub tf_r15: i64,
    /// `tf_err`: not the hardware position.
    pub tf_err: i64,
    /// `tf_rbx`.
    pub tf_rbx: i64,
    /// `tf_rax`.
    pub tf_rax: i64,
    /// `tf_trapno`.
    pub tf_trapno: i64,
    /// `tf_rbp`: hardware puts err here, `INTRENTRY()` moves it up.
    pub tf_rbp: i64,
    // below portion defined in hardware
    /// `tf_rip`.
    pub tf_rip: i64,
    /// `tf_cs`.
    pub tf_cs: i64,
    /// `tf_rflags`.
    pub tf_rflags: i64,
    // These are pushed unconditionally on the x86-64
    /// `tf_rsp`.
    pub tf_rsp: i64,
    /// `tf_ss`.
    pub tf_ss: i64,
}

/// `struct intrframe`: interrupt stack frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Intrframe {
    /// `if_rdi`.
    pub if_rdi: i64,
    /// `if_rsi`.
    pub if_rsi: i64,
    /// `if_rdx`.
    pub if_rdx: i64,
    /// `if_r10`.
    pub if_r10: i64,
    /// `if_r8`.
    pub if_r8: i64,
    /// `if_r9`.
    pub if_r9: i64,
    /// `if_rcx`.
    pub if_rcx: i64,
    /// `if_r11`.
    pub if_r11: i64,
    /// `if_r12`.
    pub if_r12: i64,
    /// `if_r13`.
    pub if_r13: i64,
    /// `if_r14`.
    pub if_r14: i64,
    /// `if_r15`.
    pub if_r15: i64,
    /// `if_err`: `IREENT_MAGIC` if resume/recurse.
    pub if_err: i64,
    /// `if_rbx`.
    pub if_rbx: i64,
    /// `if_rax`.
    pub if_rax: i64,
    /// `if_ppl`: previous priority level.
    pub if_ppl: i64,
    /// `if_rbp`.
    pub if_rbp: i64,
    // below portion defined in hardware
    /// `if_rip`.
    pub if_rip: i64,
    /// `if_cs`.
    pub if_cs: i64,
    /// `if_rflags`.
    pub if_rflags: i64,
    // These are pushed unconditionally on the x86-64
    /// `if_rsp`.
    pub if_rsp: i64,
    /// `if_ss`.
    pub if_ss: i64,
}

/// `struct iretq_frame`: what the hardware pushes (and `iretq` pops).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IretqFrame {
    /// `iretq_rip`.
    pub iretq_rip: i64,
    /// `iretq_cs`.
    pub iretq_cs: i64,
    /// `iretq_rflags`.
    pub iretq_rflags: i64,
    /// `iretq_rsp`.
    pub iretq_rsp: i64,
    /// `iretq_ss`.
    pub iretq_ss: i64,
}

/// `struct switchframe`: stack frame inside `cpu_switchto`: the callee-saved registers it
/// pushes, then the return address `ret` pops. `cpu_fork` builds one so a new thread's first
/// `cpu_switchto` returns into `proc_trampoline`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Switchframe {
    /// `sf_r15`.
    pub sf_r15: i64,
    /// `sf_r14`.
    pub sf_r14: i64,
    /// `sf_r13`.
    pub sf_r13: i64,
    /// `sf_r12`.
    pub sf_r12: i64,
    /// `sf_rbp`.
    pub sf_rbp: i64,
    /// `sf_rbx`.
    pub sf_rbx: i64,
    /// `sf_rip`.
    pub sf_rip: i64,
}

/// `FRAMESIZE`: the size of a trap frame.
pub const FRAMESIZE: usize = size_of::<Trapframe>();

const _: () = {
    assert!(size_of::<Trapframe>() == 22 * 8);
    assert!(core::mem::offset_of!(Trapframe, tf_trapno) == 120);
    assert!(core::mem::offset_of!(Trapframe, tf_rip) == 136);
    assert!(size_of::<Intrframe>() == 22 * 8);
    assert!(size_of::<IretqFrame>() == 5 * 8);
    assert!(size_of::<Switchframe>() == 7 * 8);
};
/* </CODE> */
