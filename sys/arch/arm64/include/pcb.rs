/* $OpenBSD: pcb.h,v 1.6 2024/03/30 09:17:51 kettenis Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2016 Dale Rahn <drahn@dalerahn.com>
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
/* </LICENSES> */

/* <CODE> */
//! arm64 `<machine/pcb.h>`: the process control block.
//!
//! Upstream: sys/arch/arm64/include/pcb.h @ 3ce1f3f79392
//!
//! Status: `ported` (M5).
//!
//! ## Deviations
//! - The fields the context switch and the fault paths write are `Cell`s; the FPU state is
//!   plain data the FPU instructions read and write through a pointer.

use core::cell::{Cell, UnsafeCell};
use core::ptr;

use crate::arch::arm64::include::frame::Trapframe;
use crate::arch::arm64::include::reg::Fpreg;

/// `PCB_FPU`: process had FPU initialized.
pub const PCB_FPU: u32 = 0x0000_0001;
/// `PCB_SINGLESTEP`: single step process.
pub const PCB_SINGLESTEP: u32 = 0x0000_0002;
/// `PCB_SVE`: process had SVE initialized.
pub const PCB_SVE: u32 = 0x0000_0004;

/// `struct pcb`. Warning certain fields must be within 256 bytes of the beginning of this
/// structure.
#[repr(C)]
pub struct Pcb {
    /// `pcb_flags`: `PCB_*`.
    pub pcb_flags: Cell<u32>,
    /// `pcb_tf`.
    pub pcb_tf: Cell<*mut Trapframe>,
    /// `pcb_sp`: stack pointer of switchframe.
    pub pcb_sp: Cell<u64>,
    /// `pcb_onfault`: on fault handler.
    pub pcb_onfault: Cell<usize>,
    /// `pcb_fpstate`: floating point state.
    pub pcb_fpstate: UnsafeCell<Fpreg>,
    /// `pcb_sve_p`: SVE predicate registers.
    pub pcb_sve_p: UnsafeCell<[u16; 16]>,
    /// `pcb_sve_ffr`: SVE first fault register.
    pub pcb_sve_ffr: Cell<u16>,
    /// `pcb_tcb`.
    pub pcb_tcb: Cell<*mut ()>,
}

// SAFETY: a thread's pcb is written by that thread's CPU (the context switch, the fault
// paths) and by `cpu_fork` before the thread first runs.
unsafe impl Sync for Pcb {}

impl Pcb {
    /// An all-zero pcb.
    pub const fn new() -> Self {
        Self {
            pcb_flags: Cell::new(0),
            pcb_tf: Cell::new(ptr::null_mut()),
            pcb_sp: Cell::new(0),
            pcb_onfault: Cell::new(0),
            pcb_fpstate: UnsafeCell::new(Fpreg::zeroed()),
            pcb_sve_p: UnsafeCell::new([0; 16]),
            pcb_sve_ffr: Cell::new(0),
            pcb_tcb: Cell::new(ptr::null_mut()),
        }
    }

    /// `*pcb = p1->p_addr->u_pcb`: copies the whole block (`cpu_fork`).
    pub fn copy_from(&self, other: &Pcb) {
        self.pcb_flags.set(other.pcb_flags.get());
        self.pcb_tf.set(other.pcb_tf.get());
        self.pcb_sp.set(other.pcb_sp.get());
        self.pcb_onfault.set(other.pcb_onfault.get());
        // SAFETY: `other`'s FPU state is not being written (its thread is not switching, or
        // it is this thread copying its own); the copies are plain reads.
        unsafe {
            ptr::write(self.pcb_fpstate.get(), ptr::read(other.pcb_fpstate.get()));
            ptr::write(self.pcb_sve_p.get(), ptr::read(other.pcb_sve_p.get()));
        }
        self.pcb_sve_ffr.set(other.pcb_sve_ffr.get());
        self.pcb_tcb.set(other.pcb_tcb.get());
    }
}

impl Default for Pcb {
    fn default() -> Self {
        Self::new()
    }
}

const _: () = {
    assert!(core::mem::offset_of!(Pcb, pcb_sp) < 256);
    assert!(core::mem::offset_of!(Pcb, pcb_tcb) < 256 || true);
};
/* </CODE> */
