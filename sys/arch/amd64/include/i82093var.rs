/*	$OpenBSD: i82093var.h,v 1.8 2025/09/05 16:57:48 kettenis Exp $	*/
/* $NetBSD: i82093var.h,v 1.1 2003/02/26 21:26:10 fvdl Exp $ */
/* <LICENSES> */
/*-
 * Copyright (c) 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by RedBack Networks Inc.
 *
 * Author: Bill Sommerfeld
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

//! amd64 `<machine/i82093var.h>`: the I/O APIC's software state and the encoding of an
//! interrupt handle's `line`.
//!
//! Upstream: sys/arch/amd64/include/i82093var.h @ 3ce1f3f79392
//!
//! M7b ported the `APIC_INT_*` encoding and the `APIC_IRQ_*` accessors, which
//! `pci_machdep.c` uses to tell MSI, MSI-X, I/O APIC and legacy (8259) interrupts apart;
//! M13 `struct ioapic_pin` and `struct ioapic_softc` with `ioapic.c`. The prototypes and the
//! `extern` globals (`ioapic_bsp_id`, `nioapics`, `ioapics`) are `amd64/ioapic.rs`'s.
//!
//! ## Deviations
//! - The `APIC_IRQ_*` macros are `const fn`s with lower-case names.
//! - `struct ioapic_softc` starts with its device, as the C's (whose `struct pic` starts with
//!   `pic_dev`), but `sc_pic` follows it: amd64's `struct pic` here holds a name instead of
//!   the device (`pic.rs`), and a `&str` is not valid as zeroes, so the softc
//!   `config_make_softc` zeroes holds it as `MaybeUninit`, written by `ioapic_attach`;
//!   `ioapic.rs` gets from a `&Pic` back to its softc by the field's offset. The C's
//!   `sc_pic.pic_mutex` (`MULTIPROCESSOR`) is `sc_mutex` (the ported `struct pic` has none).
//! - `sc_pins` points at the `sc_apic_sz` pins `ioapic_attach` allocates (the C's
//!   `mallocarray`); [`IoapicSoftc::pins`] returns them as a slice.

use core::cell::{Cell, UnsafeCell};
use core::mem::MaybeUninit;

use crate::arch::amd64::amd64::bus_space::{BusSpaceHandle, X86BusSpace};
use crate::arch::amd64::include::cpu::CpuInfo;
use crate::arch::amd64::include::pic::Pic;
use crate::machine::mpconfig::MpIntrMap;
use crate::sys::device::{Device, Softc};
use crate::sys::mutex::Mutex;

/// `struct ioapic_pin`.
pub struct IoapicPin {
    /// `ip_next`: next pin on this vector.
    pub ip_next: Cell<*const IoapicPin>,
    /// `ip_map`.
    pub ip_map: Cell<Option<&'static MpIntrMap>>,
    /// `ip_vector`: IDT vector.
    pub ip_vector: Cell<i32>,
    /// `ip_type`.
    pub ip_type: Cell<i32>,
    /// `ip_cpu`: target CPU.
    pub ip_cpu: Cell<*const CpuInfo>,
}

impl IoapicPin {
    /// A pin with nothing routed (`IST_NONE`).
    pub const fn new() -> Self {
        Self {
            ip_next: Cell::new(core::ptr::null()),
            ip_map: Cell::new(None),
            ip_vector: Cell::new(0),
            ip_type: Cell::new(0),
            ip_cpu: Cell::new(core::ptr::null()),
        }
    }
}

impl Default for IoapicPin {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct ioapic_softc`. The cells are written by `ioapic_attach` and, for the pins, by
/// `intr_establish` (under the kernel lock); the registers are reached under `sc_mutex`.
#[repr(C)]
pub struct IoapicSoftc {
    /// `sc_pic.pic_dev`: the device.
    pub sc_dev: Device,
    /// `sc_pic`: written once by `ioapic_attach`, before the pic is handed out.
    pub sc_pic: UnsafeCell<MaybeUninit<Pic>>,
    /// `sc_pic.pic_mutex`.
    pub sc_mutex: Mutex,
    /// `sc_next`.
    pub sc_next: Cell<*const IoapicSoftc>,
    /// `sc_apicid`.
    pub sc_apicid: Cell<i32>,
    /// `sc_apic_vers`.
    pub sc_apic_vers: Cell<i32>,
    /// `sc_apic_vecbase`: global int base if ACPI.
    pub sc_apic_vecbase: Cell<i32>,
    /// `sc_apic_sz`: apic size.
    pub sc_apic_sz: Cell<i32>,
    /// `sc_flags`.
    pub sc_flags: Cell<i32>,
    /// `sc_memt`.
    pub sc_memt: Cell<X86BusSpace>,
    /// `sc_memh`.
    pub sc_memh: Cell<BusSpaceHandle>,
    /// `sc_pins`: `sc_apic_sz` entries.
    pub sc_pins: Cell<*const IoapicPin>,
}

impl IoapicSoftc {
    /// `sc_pins[0 .. sc_apic_sz]`; empty until `ioapic_attach` allocated them.
    pub fn pins(&self) -> &[IoapicPin] {
        let p = self.sc_pins.get();
        if p.is_null() {
            return &[];
        }
        // SAFETY: `ioapic_attach` allocated `sc_apic_sz` initialised pins at `sc_pins`, never
        // freed (an I/O APIC does not detach).
        unsafe { core::slice::from_raw_parts(p, self.sc_apic_sz.get() as usize) }
    }

    /// `&sc->sc_pic`.
    ///
    /// # Safety
    ///
    /// `ioapic_attach` has initialised `sc_pic` (it does before publishing the softc).
    pub unsafe fn pic(&self) -> &Pic {
        // SAFETY: the caller's guarantee; `sc_pic` is never written again.
        unsafe { (*self.sc_pic.get()).assume_init_ref() }
    }
}

// SAFETY: `#[repr(C)]` with the device first; every field is valid as zeroes (the pic is
// `MaybeUninit`, the mutex at `IPL_NONE`, `X86BusSpace::Io`, a null handle and pointers).
unsafe impl Softc for IoapicSoftc {}

/// `APIC_INT_VIA_APIC`: the handle is routed through an I/O APIC (MP: `intr_handle_t` is
/// bitfielded: `ih & 0xff` the legacy irq number, this bit 0 for an old-style ISA irq).
pub const APIC_INT_VIA_APIC: i32 = 0x1000_0000;
/// `APIC_INT_VIA_MSG`: an MSI.
pub const APIC_INT_VIA_MSG: i32 = 0x2000_0000;
/// `APIC_INT_VIA_MSGX`: an MSI-X.
pub const APIC_INT_VIA_MSGX: i32 = 0x4000_0000;
/// `APIC_INT_APIC_MASK`: `(ih & 0xff0000) >> 16` is the I/O APIC id.
pub const APIC_INT_APIC_MASK: i32 = 0x00ff_0000;
/// `APIC_INT_APIC_SHIFT`.
pub const APIC_INT_APIC_SHIFT: i32 = 16;
/// `APIC_INT_PIN_MASK`: `(ih & 0x00ff00) >> 8` is the I/O APIC pin.
pub const APIC_INT_PIN_MASK: i32 = 0x0000_ff00;
/// `APIC_INT_PIN_SHIFT`.
pub const APIC_INT_PIN_SHIFT: i32 = 8;

/// `APIC_IRQ_APIC(x)`: the I/O APIC id of a handle.
pub const fn apic_irq_apic(x: i32) -> i32 {
    (x & APIC_INT_APIC_MASK) >> APIC_INT_APIC_SHIFT
}

/// `APIC_IRQ_PIN(x)`: the I/O APIC pin of a handle.
pub const fn apic_irq_pin(x: i32) -> i32 {
    (x & APIC_INT_PIN_MASK) >> APIC_INT_PIN_SHIFT
}

/// `APIC_IRQ_ISLEGACY(x)`: an old-style ISA irq.
pub const fn apic_irq_islegacy(x: i32) -> bool {
    x & APIC_INT_VIA_APIC == 0
}

/// `APIC_IRQ_LEGACY_IRQ(x)`: the legacy irq number.
pub const fn apic_irq_legacy_irq(x: i32) -> i32 {
    x & 0xff
}
