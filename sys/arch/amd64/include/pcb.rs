/*	$OpenBSD: pcb.h,v 1.19 2024/04/14 09:59:04 kettenis Exp $	*/
/*	$NetBSD: pcb.h,v 1.1 2003/04/26 18:39:45 fvdl Exp $	*/
/* <LICENSES> */
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
 *	@(#)pcb.h	5.10 (Berkeley) 5/12/91
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/pcb.h>`: the process control block.
//!
//! Upstream: sys/arch/amd64/include/pcb.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports `struct pcb`; `reset_segs` comes with user-mode
//! threads (M6).
//!
//! ## Deviations
//! - The fields the context switch and the fault paths write are `Cell`s; the FPU save area
//!   is plain data the FPU instructions read and write through a pointer.

use core::cell::{Cell, UnsafeCell};
use core::ptr;

use crate::arch::amd64::include::fpu::Savefpu;
use crate::arch::amd64::include::pmap::Pmap;

/// `struct pcb`. Please note that the `pcb_savefpu` field in struct below must be on a
/// 64-byte boundary.
#[repr(C)]
pub struct Pcb {
    /// `pcb_savefpu`: floating point state.
    pub pcb_savefpu: UnsafeCell<Savefpu>,
    /// `pcb_cr3`.
    pub pcb_cr3: Cell<u64>,
    /// `pcb_rsp`.
    pub pcb_rsp: Cell<u64>,
    /// `pcb_rbp`.
    pub pcb_rbp: Cell<u64>,
    /// `pcb_kstack`: kernel stack address.
    pub pcb_kstack: Cell<u64>,
    /// `pcb_fsbase`: per-thread offset: %fs.
    pub pcb_fsbase: Cell<u64>,
    /// `pcb_onfault`: copyin/out fault recovery.
    pub pcb_onfault: Cell<usize>,
    /// `pcb_pmap`: back pointer to our pmap.
    pub pcb_pmap: Cell<*const Pmap>,
}

// SAFETY: a thread's pcb is written by that thread's CPU (the context switch, the fault
// paths) and by `cpu_fork` before the thread first runs.
unsafe impl Sync for Pcb {}

impl Pcb {
    /// An all-zero pcb.
    pub const fn new() -> Self {
        Self {
            pcb_savefpu: UnsafeCell::new(Savefpu::zeroed()),
            pcb_cr3: Cell::new(0),
            pcb_rsp: Cell::new(0),
            pcb_rbp: Cell::new(0),
            pcb_kstack: Cell::new(0),
            pcb_fsbase: Cell::new(0),
            pcb_onfault: Cell::new(0),
            pcb_pmap: Cell::new(ptr::null()),
        }
    }

    /// `*pcb = *pcb1`: copies the whole block (`cpu_fork`).
    pub fn copy_from(&self, other: &Pcb) {
        // SAFETY: `other`'s save area is not being written (its thread is not on a CPU
        // switching, or it is this thread copying its own); the copy is a plain read.
        unsafe { ptr::write(self.pcb_savefpu.get(), ptr::read(other.pcb_savefpu.get())) };
        self.pcb_cr3.set(other.pcb_cr3.get());
        self.pcb_rsp.set(other.pcb_rsp.get());
        self.pcb_rbp.set(other.pcb_rbp.get());
        self.pcb_kstack.set(other.pcb_kstack.get());
        self.pcb_fsbase.set(other.pcb_fsbase.get());
        self.pcb_onfault.set(other.pcb_onfault.get());
        self.pcb_pmap.set(other.pcb_pmap.get());
    }
}

impl Default for Pcb {
    fn default() -> Self {
        Self::new()
    }
}

const _: () = {
    assert!(core::mem::offset_of!(Pcb, pcb_savefpu) == 0);
    assert!(core::mem::align_of::<Pcb>() == 64);
};
/* </CODE> */
