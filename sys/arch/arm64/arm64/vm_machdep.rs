/*	$OpenBSD: vm_machdep.c,v 1.14 2025/05/21 09:06:58 mpi Exp $	*/
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

//! arm64 `vm_machdep.c`: the machine-dependent part of creating and tearing down threads.
//!
//! Upstream: sys/arch/arm64/arm64/vm_machdep.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part b2) ports `cpu_fork` and `cpu_exit`; `kv_physwait`,
//! `vmapbuf` and `vunmapbuf` (physio) come with the block layer (M7).
//!
//! ## Deviations
//! - The pointer authentication keys (`pm_ap*key`) the C copies from the parent's pmap wait
//!   for user pmaps (M6): kernel threads share the kernel pmap.
//! - The switch frame's `sf_x19` holds the thread function as a pointer, which
//!   `proc_trampoline_run` (`cpuswitch.rs`) turns back into a Rust `fn`: Rust function
//!   pointers have no C calling convention to `blr` from assembly.

use core::ffi::c_void;

use crate::arch::arm64::arm64::cpuswitch::proc_trampoline;
use crate::arch::arm64::arm64::fpu::fpu_save;
use crate::arch::arm64::arm64::pmap::pmap_activate;
use crate::arch::arm64::include::frame::{Switchframe, Trapframe};
use crate::arch::arm64::include::param::{USPACE, stackalign};
use crate::arch::arm64::include::pcb::PCB_FPU;
use crate::sys::proc::Proc;
use crate::sys::types::Register;

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
    let pcb = p2.pcb();
    let pcb1 = p1.pcb();

    // pm->pm_ap{ia,da,ib,db,ga}key = pm1's: user pmaps (M6).

    // Save FPU state to PCB if necessary.
    if pcb1.pcb_flags.get() & PCB_FPU != 0 {
        fpu_save(p1);
    }

    // Copy the pcb.
    pcb.copy_from(pcb1);

    pmap_activate(p2);

    let tf = stackalign(p2.p_addr.get() as usize + USPACE - size_of::<Trapframe>() - 0x10)
        as *mut Trapframe;
    pcb.pcb_tf.set(tf);
    // SAFETY: `tf` is inside p2's fresh u-area, below its top; p1's `pcb_tf` points at its
    // own trap frame (proc0's static one, or the frame of the syscall that forked).
    unsafe { tf.write(pcb1.pcb_tf.get().read()) };

    if !stack.is_null() {
        // SAFETY: `tf` was just written above.
        unsafe { (*tf).tf_sp = stackalign(stack as usize) as Register };
    }
    if !tcb.is_null() {
        pcb.pcb_tcb.set(tcb.cast());
    }

    let sf = (tf as *mut Switchframe).wrapping_sub(1);
    // SAFETY: `sf` is right below the trap frame, still inside the u-area (USPACE is far
    // larger than both frames).
    unsafe {
        sf.write(Switchframe {
            sf_x19: func as *const () as usize as Register,
            sf_x20: arg as usize as Register,
            sf_lr: proc_trampoline as *const () as usize as Register,
            ..Switchframe::default()
        });
    }
    pcb.pcb_sp.set(sf as u64);
}

/// `cpu_exit`: nothing to do on arm64.
pub fn cpu_exit(_p: &Proc) {}

// kv_physwait, vmapbuf, vunmapbuf: physio (M7).
