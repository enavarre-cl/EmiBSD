/* <LICENSES> */
/*-
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Frank van der Linden.
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
//! amd64 `<machine/i82489var.h>`: software definitions belonging to Local APIC driver.
//!
//! Upstream: sys/arch/amd64/include/i82489var.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports the vector numbers; `local_apic`, `lapic_tpr`, the
//! `lapic_readreg`/`lapic_writereg` pointers and the functions it declares are
//! `amd64/lapic.rs`. The IPI and timer stubs (`Xintr_lapic_*`, `Xipi_*`) come with M5.

/// `LAPIC_SPURIOUS_VECTOR`: "spurious interrupt vector"; vector used by interrupt which was
/// aborted because the CPU masked it after it happened but before it was delivered.. "Oh,
/// sorry, i caught you at a bad time". Low-order 4 bits must be all ones.
pub const LAPIC_SPURIOUS_VECTOR: i32 = 0xef;

/// `LAPIC_IPI_VECTOR`: vector used for inter-processor interrupts.
pub const LAPIC_IPI_VECTOR: i32 = 0xe0;

/// `LAPIC_IPI_OFFSET`: we take 0xf0-0xfe for fast IPI handlers.
pub const LAPIC_IPI_OFFSET: i32 = 0xf0;
/// `LAPIC_IPI_INVLTLB`.
pub const LAPIC_IPI_INVLTLB: i32 = LAPIC_IPI_OFFSET;
/// `LAPIC_IPI_INVLPG`.
pub const LAPIC_IPI_INVLPG: i32 = LAPIC_IPI_OFFSET + 1;
/// `LAPIC_IPI_INVLRANGE`.
pub const LAPIC_IPI_INVLRANGE: i32 = LAPIC_IPI_OFFSET + 2;
/// `LAPIC_IPI_INVEPT`.
pub const LAPIC_IPI_INVEPT: i32 = LAPIC_IPI_OFFSET + 3;

/// `LAPIC_TIMER_VECTOR`: vector used for local apic timer interrupts.
pub const LAPIC_TIMER_VECTOR: i32 = 0xc0;

/// `LAPIC_XEN_VECTOR`: vector used for Xen HVM Event Channel Interrupts.
pub const LAPIC_XEN_VECTOR: i32 = 0x70;

/// `LAPIC_HYPERV_VECTOR`: vector used for Hyper-V Interrupts.
pub const LAPIC_HYPERV_VECTOR: i32 = 0x71;
/* </CODE> */
