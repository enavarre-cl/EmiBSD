/*	$OpenBSD: autoconf.c,v 1.61 2026/06/23 14:40:40 bluhm Exp $	*/
/*	$NetBSD: autoconf.c,v 1.1 2003/04/26 18:39:26 fvdl Exp $	*/
/* <LICENSES> */
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
 *	@(#)autoconf.c	7.1 (Berkeley) 5/9/91
 */
/* </LICENSES> */

//! Setup the system to run on the current machine: `arch/amd64/amd64/autoconf.c`.
//!
//! Upstream: sys/arch/amd64/amd64/autoconf.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 needs only `cold`, which `boot(9)` consults; M4 adds
//! `cpu_configure` as far as the interrupts go. `diskconf`, `device_register` and the
//! root-device search arrive with autoconfiguration (M5).
//!
//! ## Deviations
//! - `cpu_configure` has no `config_rootfound("mainbus")`: what the `cpu0` attach would do
//!   for the interrupts (`cpu_intr_init` from `cpu_attach`, `intr_enable` from `cpu_init`) is
//!   done here directly, as is the LAPIC setup `mpbios`/`acpimadt` would trigger
//!   (`lapic_boot_init` at the architectural base, `lapic_enable`, `lapic_set_lvt`) and the
//!   timer calibration of the boot CPU's `cpu_attach` (`lapic_calibrate_timer`);
//!   `pmap_randomize`, `map_tramps`, `bus_dma_init`,
//!   `mbuf_dma_64bit_enable`, `unmap_startup` and the random-number timeouts are reported.

use core::sync::atomic::Ordering;

use crate::arch::amd64::amd64::intr::{cpu_intr_init, intr_printconfig};
use crate::arch::amd64::amd64::lapic::{
    lapic_boot_init, lapic_calibrate_timer, lapic_enable, lapic_set_lvt,
};
use crate::arch::amd64::amd64::machdep::x86_64_proc0_tss_ldt_init;
use crate::arch::amd64::include::cpu::cpu_info_primary;
use crate::arch::amd64::include::cpufunc::{intr_enable, lcr8};
use crate::arch::amd64::include::i82489reg::LAPIC_BASE;
use crate::machine::intr::spl0;
use crate::sys::types::Paddr;
use crate::unported;

/// `cold`: if set, still working on cold-start.
pub use crate::sys::systm::COLD;

/// `cpu_configure`: determine i/o configuration for a machine.
pub fn cpu_configure() {
    x86_64_proc0_tss_ldt_init();

    let _ = unported!("pmap_randomize (M6)");
    let _ = unported!("map_tramps (M6)");
    let _ = unported!("bus_dma_init (M7)");

    // config_rootfound("mainbus", NULL): autoconfiguration (M5). Of what it would attach:
    // mpbios/acpimadt find the LAPIC and call lapic_boot_init; the cpu0 attach does
    // lapic_enable, cpu_intr_init (cpu_attach), lapic_set_lvt and intr_enable (cpu_init).
    // TODO(M5): the LAPIC base comes from the MADT or the MP tables; this is the
    // architectural default.
    lapic_boot_init(Paddr::new(LAPIC_BASE));
    lapic_enable();
    cpu_intr_init(cpu_info_primary());
    lapic_set_lvt();
    // SAFETY: the IDT, the PIC and the masks are set up.
    unsafe { intr_enable() };
    // cpu_attach, CPU_ROLE_BP: calibrate the LAPIC timer against the i8254 (M5).
    lapic_calibrate_timer(cpu_info_primary());

    intr_printconfig();

    let _ = unported!("mbuf_dma_64bit_enable (M7)");

    // NIOAPIC > 0: lapic_set_lvt, ioapic_enable (M5).

    let _ = unported!("unmap_startup (M6)");

    // SAFETY: 0 lets every interrupt through, the boot value.
    unsafe { lcr8(0) };
    spl0();
    COLD.store(false, Ordering::Relaxed);

    // The viac3_rnd and rdrand timeouts: M5.
}
