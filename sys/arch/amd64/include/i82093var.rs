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
//! Status: `wip`. Milestone M7b ports the `APIC_INT_*` encoding and the `APIC_IRQ_*`
//! accessors, which `pci_machdep.c` uses to tell MSI, MSI-X, I/O APIC and legacy (8259)
//! interrupts apart. `struct ioapic_pin`, `struct ioapic_softc` and the `ioapic_*`
//! prototypes come with `ioapic.c` (the I/O APIC is not driven yet).
//!
//! ## Deviations
//! - The `APIC_IRQ_*` macros are `const fn`s with lower-case names.

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
