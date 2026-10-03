/*	$OpenBSD: vm_machdep.c,v 1.51 2025/07/07 18:33:36 kettenis Exp $	*/
/*	$NetBSD: vm_machdep.c,v 1.1 2003/04/26 18:39:33 fvdl Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1995 Charles M. Hannum.  All rights reserved.
 * Copyright (c) 1982, 1986 The Regents of the University of California.
 * Copyright (c) 1989, 1990 William Jolitz
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * the Systems Programming Group of the University of Utah Computer
 * Science Department, and William Jolitz.
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
 *	@(#)vm_machdep.c	7.3 (Berkeley) 5/13/91
 */

/*
 *	Utah $Hdr: vm_machdep.c 1.16.1.1 89/06/23$
 */
/* </LICENSES> */

//! amd64 `vm_machdep.c`: the machine-dependent part of creating and tearing down threads.
//!
//! Upstream: sys/arch/amd64/amd64/vm_machdep.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part b2) ports `cpu_fork` and `cpu_exit`; `tcb_get` and
//! `tcb_set` (`<machine/tcb.h>`'s `TCB_GET`/`TCB_SET`) come with `kern_prot.c`; `vmapbuf`
//! and `vunmapbuf` (physio) come with the block layer (M7).
//!
//! ## Deviations
//! - The TCB is a `usize` (a user address the kernel never dereferences), not a `void *`.
//! - `cpu_fork` cannot `fpusave` a parent that ran in user mode (`CPUPF_USERXSTATE`) before
//!   user mode exists (M6): the case is reported.
//! - The switch frame's `sf_r12` holds the thread function as a pointer, which
//!   `proc_trampoline_run` (`locore.rs`) turns back into a Rust `fn`: Rust function
//!   pointers have no C calling convention to `call` from assembly.

use core::ffi::c_void;
use core::ptr;

use crate::arch::amd64::amd64::locore::proc_trampoline;
use crate::arch::amd64::amd64::machdep::reset_segs;
use crate::arch::amd64::amd64::pmap::pmap_activate;
use crate::arch::amd64::include::_types::_STACKALIGNBYTES;
use crate::arch::amd64::include::cpu::{CPUPF_USERXSTATE, curcpu};
use crate::arch::amd64::include::frame::{Switchframe, Trapframe};
use crate::arch::amd64::include::param::{PAGE_MASK, USPACE};
use crate::dev::rnd::arc4random;
use crate::kassert;
use crate::kern::init_main::PROC0;
use crate::sys::proc::Proc;
use crate::unported;

/// `cpu_fork`: finish a fork operation, with process `p2` nearly set up. Copy and update the
/// kernel stack and pcb, making the child ready to run, and marking it so that it can return
/// differently than the parent.
pub fn cpu_fork(
    p1: &Proc,
    p2: &Proc,
    stack: *mut u8,
    tcb: *mut u8,
    func: fn(*mut c_void),
    arg: *mut c_void,
) {
    let ci = curcpu();
    let pcb = p2.pcb();
    let pcb1 = p1.pcb();

    // Save the fpu h/w state to p1's pcb so that we can copy it.
    if !ptr::eq(p1, &PROC0) && ci.ci_pflags.get() & CPUPF_USERXSTATE != 0 {
        let _ = unported!("cpu_fork: fpusave of a user-mode parent (M6)");
    }

    p2.p_md.md_flags.set(p1.p_md.md_flags.get());

    #[cfg(feature = "diagnostic")]
    if !ptr::eq(p1, ci.ci_curproc.get()) && !ptr::eq(p1, &PROC0) {
        crate::kern::subr_prf::panic(format_args!("cpu_fork: curproc"));
    }
    pcb.copy_from(pcb1);

    // Activate the address space.
    pmap_activate(p2);

    // Record where this process's kernel stack is
    pcb.pcb_kstack.set(
        p2.p_addr.get() as u64 + USPACE as u64
            - 16
            - u64::from(arc4random() & PAGE_MASK as u32 & !(_STACKALIGNBYTES as u32)),
    );

    // Copy the trapframe.
    let tf = (pcb.pcb_kstack.get() as *mut Trapframe).wrapping_sub(1);
    p2.p_md.md_regs.set(tf);
    // SAFETY: `tf` is inside p2's fresh u-area, below `pcb_kstack`; p1's `md_regs` points at
    // its own trap frame (proc0's zero one, or the frame of the syscall that forked).
    unsafe { tf.write(p1.p_md.md_regs.get().read()) };

    // If specified, give the child a different stack and/or TCB
    if !stack.is_null() {
        // SAFETY: `tf` was just written above.
        unsafe { (*tf).tf_rsp = stack as i64 };
    }
    if !tcb.is_null() {
        pcb.pcb_fsbase.set(tcb as u64);
    }

    let sf = (tf as *mut Switchframe).wrapping_sub(1);
    // SAFETY: `sf` is right below the trap frame, still inside the u-area (USPACE is far
    // larger than both frames).
    unsafe {
        sf.write(Switchframe {
            sf_r12: func as *const () as usize as i64,
            sf_r13: arg as usize as i64,
            sf_rip: proc_trampoline as *const () as usize as i64,
            ..Switchframe::default()
        });
    }
    pcb.pcb_rsp.set(sf as u64);
    pcb.pcb_rbp.set(0);
}

/// `cpu_exit`: nothing to do on amd64 (the pmap goes with the vmspace).
pub fn cpu_exit(_p: &Proc) {}

// kv_physwait, vmapbuf, vunmapbuf: physio (M7).

/// `tcb_get` (`TCB_GET(p)`): the `%fs` base `p` runs with in user mode.
pub fn tcb_get(p: &Proc) -> usize {
    p.pcb().pcb_fsbase.get() as usize
}

/// `tcb_set` (`TCB_SET(p, addr)`): `p` (the running thread) gets `tcb` as its `%fs` base.
/// `reset_segs` makes the return to user mode load it from the pcb.
pub fn tcb_set(p: &Proc, tcb: usize) {
    kassert!(ptr::eq(p, curcpu().ci_curproc.get()));
    reset_segs();
    p.pcb().pcb_fsbase.set(tcb as u64);
}
