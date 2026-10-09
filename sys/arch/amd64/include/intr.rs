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
 * Copyright (c) 1998, 2001 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum, and by Jason R. Thorpe.
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
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/intr.h>`: the interrupt sources, handler chains and `spl` helpers.
//!
//! Upstream: sys/arch/amd64/include/intr.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `struct intrstub`, `struct intrsource`,
//! `struct intrhand`, the `IS_*` flags, `IMASK`/`IUNMASK`, `APIC_LEVEL`, `IPLSHIFT`/`IPL`
//! and the `spl*` wrappers (the machine-independent ones are the `machine::intr` contract).
//! The `MULTIPROCESSOR` IPI prototypes come with M5. The functions this header declares are
//! `amd64/intr.rs`; the stubs it declares (`Xsoft*`, `i8259_stubs`) are `amd64/vector.rs`.
//!
//! ## Deviations
//! - The three structs are `#[repr(C)]` with `Cell` fields: the interrupt stubs read
//!   `is_maxlevel`, `is_handlers`, `is_recurse`, `is_resume`, `ih_level`, `ih_next` and
//!   `ih_count` by offset (`offset_of!`, the C's `assym.h`), and the C mutates them through
//!   shared pointers under the interrupt mask.
//! - `is_evname` is a NUL-terminated byte array behind `UnsafeCell`, written once by
//!   `intr_allocate_slot*`.

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::ptr;

use crate::arch::amd64::include::cpu::CpuInfo;
use crate::arch::amd64::include::pic::Pic;
use crate::sys::evcount::Evcount;

/// An interrupt handler: `int (*ih_fun)(void *)`; returns 0 when the interrupt was not for
/// it, 1 when handled (stop the chain unless an edge is shared), -1 when handled but others
/// may share.
pub type IntrFn = fn(*mut c_void) -> i32;

/// `struct intrstub`: the three assembly entry points of one interrupt source.
#[repr(C)]
pub struct Intrstub {
    /// `ist_entry`: the IDT gate's target.
    pub ist_entry: usize,
    /// `ist_recurse`: the entry `Xspllower` uses for a deferred interrupt.
    pub ist_recurse: usize,
    /// `ist_resume`: the entry `Xdoreti` uses for a deferred interrupt.
    pub ist_resume: usize,
}

/// `struct intrsource`: an interrupt source for a CPU. `struct cpu_info` has an array of
/// `MAX_INTR_SOURCES` of these; the index in the array is equal to the stub number of the
/// stubcode as present in `vector.S`. The primary CPU's array of interrupt sources has its
/// first 16 entries reserved for legacy ISA irq handlers.
#[repr(C)]
pub struct Intrsource {
    /// `is_maxlevel`: max. IPL for this source.
    pub is_maxlevel: Cell<i32>,
    /// `is_pin`: IRQ for legacy; pin for IO APIC.
    pub is_pin: Cell<i32>,
    /// `is_handlers`: handler chain.
    pub is_handlers: Cell<*const Intrhand>,
    /// `is_pic`: originating PIC.
    pub is_pic: Cell<*const Pic>,
    /// `is_recurse`: entry for spllower.
    pub is_recurse: Cell<usize>,
    /// `is_resume`: entry for doreti.
    pub is_resume: Cell<usize>,
    /// `is_evname`: event counter name.
    pub is_evname: UnsafeCell<[u8; 32]>,
    /// `is_flags`: see `IS_*`.
    pub is_flags: Cell<i32>,
    /// `is_type`: level, edge.
    pub is_type: Cell<i32>,
    /// `is_idtvec`.
    pub is_idtvec: Cell<i32>,
    /// `is_minlevel`.
    pub is_minlevel: Cell<i32>,
}

// SAFETY: a CPU's own source, written at establish/disestablish time with the source masked
// and read by that CPU's interrupt stubs.
unsafe impl Sync for Intrsource {}

impl Intrsource {
    /// A zeroed source (`M_ZERO`).
    pub const fn new() -> Self {
        Self {
            is_maxlevel: Cell::new(0),
            is_pin: Cell::new(0),
            is_handlers: Cell::new(ptr::null()),
            is_pic: Cell::new(ptr::null()),
            is_recurse: Cell::new(0),
            is_resume: Cell::new(0),
            is_evname: UnsafeCell::new([0; 32]),
            is_flags: Cell::new(0),
            is_type: Cell::new(0),
            is_idtvec: Cell::new(0),
            is_minlevel: Cell::new(0),
        }
    }
}

impl Default for Intrsource {
    fn default() -> Self {
        Self::new()
    }
}

/// `IS_LEGACY`: legacy ISA irq source.
pub const IS_LEGACY: i32 = 0x0001;
/// `IS_IPI`.
pub const IS_IPI: i32 = 0x0002;
/// `IS_LOG`.
pub const IS_LOG: i32 = 0x0004;

/// `struct intrhand`: interrupt handler chains. `*_intr_establish()` insert a handler into
/// the list. The handler is called with its (single) argument.
#[repr(C)]
pub struct Intrhand {
    /// `ih_fun`.
    pub ih_fun: Cell<Option<IntrFn>>,
    /// `ih_arg`.
    pub ih_arg: Cell<*mut c_void>,
    /// `ih_level`.
    pub ih_level: Cell<i32>,
    /// `ih_flags`: `IPL_MPSAFE`, `IPL_WAKEUP`.
    pub ih_flags: Cell<i32>,
    /// `ih_next`.
    pub ih_next: Cell<*const Intrhand>,
    /// `ih_pin`.
    pub ih_pin: Cell<i32>,
    /// `ih_slot`.
    pub ih_slot: Cell<i32>,
    /// `ih_cpu`.
    pub ih_cpu: Cell<*const CpuInfo>,
    /// `ih_irq`.
    pub ih_irq: Cell<i32>,
    /// `ih_count`: the event counter the stubs increment.
    pub ih_count: Evcount,
}

// SAFETY: as for `Intrsource`.
unsafe impl Sync for Intrhand {}

impl Intrhand {
    /// A handler with nothing set.
    pub const fn new() -> Self {
        Self {
            ih_fun: Cell::new(None),
            ih_arg: Cell::new(ptr::null_mut()),
            ih_level: Cell::new(0),
            ih_flags: Cell::new(0),
            ih_next: Cell::new(ptr::null()),
            ih_pin: Cell::new(0),
            ih_slot: Cell::new(0),
            ih_cpu: Cell::new(ptr::null()),
            ih_irq: Cell::new(0),
            ih_count: Evcount::new(),
        }
    }
}

impl Default for Intrhand {
    fn default() -> Self {
        Self::new()
    }
}

/// `IMASK(ci, level)`: the sources masked at `level`.
pub fn imask(ci: &CpuInfo, level: i32) -> u64 {
    ci.ci_imask[level as usize].get()
}

/// `IUNMASK(ci, level)`: the sources unmasked at `level`.
pub fn iunmask(ci: &CpuInfo, level: i32) -> u64 {
    ci.ci_iunmask[level as usize].get()
}

/// `APIC_LEVEL(l)`: convert spl level to local APIC level.
pub const fn apic_level(l: i32) -> i32 {
    l << 4
}

/// `IPLSHIFT`: the upper nibble of vectors is the IPL.
pub const IPLSHIFT: i32 = 4;

/// `IPL(level)`: extract the IPL.
pub const fn ipl(level: i32) -> i32 {
    level >> IPLSHIFT
}

const _: () = {
    assert!(size_of::<Intrstub>() == 24);
    assert!(core::mem::offset_of!(Intrhand, ih_count) % 8 == 0);
};
/* </CODE> */
