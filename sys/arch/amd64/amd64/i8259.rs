/*	$OpenBSD: i8259.c,v 1.13 2026/01/15 15:43:44 sf Exp $	*/
/*	$NetBSD: i8259.c,v 1.2 2003/03/02 18:27:15 fvdl Exp $	*/
/* <LICENSES> */
/*
 * Copyright 2002 (c) Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Frank van der Linden for Wasabi Systems, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed for the NetBSD Project by
 *      Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

/*-
 * Copyright (c) 1991 The Regents of the University of California.
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
 *	@(#)isa.c	7.2 (Berkeley) 5/13/91
 */
/* </LICENSES> */

//! The legacy 8259A interrupt controllers: `arch/amd64/amd64/i8259.c`.
//!
//! Upstream: sys/arch/amd64/amd64/i8259.c @ 3ce1f3f79392
//!
//! Status: `ported`. `i8259_imen`, `i8259_pic`, `i8259_default_setup`, `i8259_hwmask`,
//! `i8259_hwunmask`, `i8259_reinit_irqs` and `i8259_setup`.
//!
//! ## Deviations
//! - `i8259_imen` is an `AtomicU32` exported under its C name: the interrupt stubs in
//!   `vector.S` read and write its bytes (`i8259_asm_mask`/`unmask`).
//! - `i8259_stubs` is the table `vector.S` defines; `i8259_stubs_table` returns it as a slice
//!   for `struct pic`.
//! - The add-route hook is `i8259_addroute`: it marks a level-triggered pin (`IST_LEVEL`, the
//!   PCI interrupts `pci_intr_establish` routes here) level-sensitive in the edge/level
//!   control registers (ELCR, ports 0x4d0/0x4d1) before `i8259_setup`. OpenBSD/amd64 routes
//!   PCI interrupts through the I/O APIC (`ioapic.c`, not ported) and leaves the ELCR to the
//!   firmware; QEMU's OVMF leaves every pin edge-triggered, so a PCI line two devices share
//!   (virtio-blk disks on IRQ 10/11) loses an interrupt that arrives while the other device
//!   still holds the line: no new edge, and the request never completes. i386 programs the
//!   ELCR the same way for its PCI routing (`piix_set_trigger`). TODO(M13): goes with the I/O
//!   APIC port.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::arch::amd64::amd64::vector::i8259_stubs;
use crate::arch::amd64::include::cpu::{CpuInfo, cpu_info_primary, cpu_is_primary};
use crate::arch::amd64::include::cpufunc::{intr_disable, intr_restore};
use crate::arch::amd64::include::i8259::{ICU_OFFSET, IRQ_SLAVE};
use crate::arch::amd64::include::intr::Intrstub;
use crate::arch::amd64::include::intrdefs::{IST_LEVEL, NUM_LEGACY_IRQS};
use crate::arch::amd64::include::pic::{PIC_I8259, Pic};
use crate::arch::amd64::include::pio::{inb, outb};
use crate::dev::isa::isareg::{IO_ICU1, IO_ICU2};

/// `i8259_imen`: interrupt mask enable, the bits of the IRQs currently masked.
#[unsafe(no_mangle)]
pub static i8259_imen: AtomicU32 = AtomicU32::new(0);

/// `i8259_stubs[]` as a slice.
pub fn i8259_stubs_table() -> &'static [Intrstub] {
    // SAFETY: a table of 16 initialised entries in vector.S's .rodata.
    unsafe { &*core::ptr::addr_of!(i8259_stubs) }
}

/// `i8259_pic`. Perhaps this should be made into a real device.
pub static I8259_PIC: Pic = Pic {
    pic_name: "pic0",
    pic_type: PIC_I8259,
    pic_hwmask: Some(i8259_hwmask),
    pic_hwunmask: Some(i8259_hwunmask),
    pic_addroute: Some(i8259_addroute),
    pic_delroute: Some(i8259_setup),
    pic_allocidtvec: None,
    pic_level_stubs: Some(i8259_stubs_table),
    pic_edge_stubs: Some(i8259_stubs_table),
};

/// `i8259_default_setup`: programs both controllers, vectors at `ICU_OFFSET`, everything
/// masked.
pub fn i8259_default_setup() {
    // SAFETY: the 8259 ports are the PC platform's and nothing else drives them.
    unsafe {
        outb(IO_ICU1, 0x11); // reset; program device, four bytes
        outb(IO_ICU1 + 1, ICU_OFFSET as u8); // starting at this vector index
        outb(IO_ICU1 + 1, 1 << IRQ_SLAVE); // slave on line 2
        outb(IO_ICU1 + 1, 1); // 8086 mode
        outb(IO_ICU1 + 1, 0xff); // leave interrupts masked
        outb(IO_ICU1, 0x68); // special mask mode (if available)
        outb(IO_ICU1, 0x0a); // Read IRR by default.

        outb(IO_ICU2, 0x11); // reset; program device, four bytes
        outb(IO_ICU2 + 1, (ICU_OFFSET + 8) as u8); // staring at this vector index
        outb(IO_ICU2 + 1, IRQ_SLAVE as u8);
        outb(IO_ICU2 + 1, 1); // 8086 mode
        outb(IO_ICU2 + 1, 0xff); // leave interrupts masked
        outb(IO_ICU2, 0x68); // special mask mode (if available)
        outb(IO_ICU2, 0x0a); // Read IRR by default.
    }
}

/// `i8259_hwmask`: masks `pin` in hardware.
fn i8259_hwmask(_pic: &Pic, pin: i32) {
    let imen = i8259_imen.fetch_or(1 << pin, Ordering::Relaxed) | (1 << pin);
    let (port, byte) = if pin > 7 {
        (IO_ICU2 + 1, (imen >> 8) as u8)
    } else {
        (IO_ICU1 + 1, imen as u8)
    };
    // SAFETY: as for `i8259_default_setup`.
    unsafe { outb(port, byte) };
}

/// `i8259_hwunmask`: unmasks `pin` in hardware.
fn i8259_hwunmask(_pic: &Pic, pin: i32) {
    let s = intr_disable();
    let imen = i8259_imen.fetch_and(!(1 << pin), Ordering::Relaxed) & !(1 << pin);
    let (port, byte) = if pin > 7 {
        (IO_ICU2 + 1, (imen >> 8) as u8)
    } else {
        (IO_ICU1 + 1, imen as u8)
    };
    // SAFETY: as for `i8259_default_setup`; `s` is this CPU's saved flags.
    unsafe {
        outb(port, byte);
        intr_restore(s);
    }
}

/// `i8259_reinit_irqs`: masks every IRQ that has no source on the primary CPU.
fn i8259_reinit_irqs() {
    let ci = cpu_info_primary();

    let mut irqs: u32 = 0;
    for (irq, source) in ci.ci_isources.iter().enumerate().take(NUM_LEGACY_IRQS) {
        if !source.get().is_null() {
            irqs |= 1 << irq;
        }
    }
    if irqs >= 0x100 {
        // any IRQs >= 8 in use
        irqs |= 1 << IRQ_SLAVE;
    }
    let imen = !irqs;
    i8259_imen.store(imen, Ordering::Relaxed);
    // SAFETY: as for `i8259_default_setup`.
    unsafe {
        outb(IO_ICU1 + 1, imen as u8);
        outb(IO_ICU2 + 1, (imen >> 8) as u8);
    }
}

/// `ELCR0`, `ELCR1`: the edge/level control registers of IRQ 0-7 and 8-15 (`eisavar.h`).
const ELCR0: u16 = 0x4d0;
/// `ELCR1`.
const ELCR1: u16 = 0x4d1;

/// The add-route hook: a level-triggered pin is made level-sensitive in the ELCR (see the
/// deviations), then `i8259_setup`.
fn i8259_addroute(pic: &Pic, ci: &CpuInfo, pin: i32, idtvec: i32, r#type: i32) {
    // IRQ 0, 1, 2, 8 and 13 must stay edge-triggered (timer, keyboard, cascade, RTC, FPU).
    if r#type == IST_LEVEL && (3..16).contains(&pin) && pin != 8 && pin != 13 {
        let (port, bit) = if pin < 8 {
            (ELCR0, pin)
        } else {
            (ELCR1, pin - 8)
        };
        // SAFETY: the ELCR of the chipset's 8259 pair; setting the bit of a pin only PCI
        // (level) sources use changes how the controller samples that line.
        unsafe { outb(port, inb(port) | (1 << bit)) };
    }
    i8259_setup(pic, ci, pin, idtvec, r#type);
}

/// `i8259_setup`: the add/delete route hook; the 8259 has no routing, only the mask.
fn i8259_setup(_pic: &Pic, ci: &CpuInfo, _pin: i32, _idtvec: i32, _type: i32) {
    if cpu_is_primary(ci) {
        i8259_reinit_irqs();
    }
}
