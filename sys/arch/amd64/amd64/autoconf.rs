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
//! Status: `wip`. `cold` is `sys/systm.rs`'s; `cpu_configure` runs autoconfiguration from
//! `config_rootfound("mainbus")` (M7b), `device_register`, `diskconf` and `nam2blk[]` are
//! here; `unmap_startup` waits for the boot-only text.
//!
//! ## Deviations
//! - What `bios0`/`acpi0` (the MADT) or `mpbios0` would do for the interrupts while mainbus
//!   attaches is done by `cpu_configure` around `config_rootfound`: `lapic_boot_init` at the
//!   architectural base before it; and after it, because mainbus attaches the boot CPU as
//!   `CPU_ROLE_SP` without those tables, what the boot processor's attach would add
//!   (`lapic_enable`, `lapic_calibrate_timer`), the LVT setup (`lapic_set_lvt`, which the C
//!   does after mainbus for `NIOAPIC`) and `intr_enable`. `pmap_randomize`, `map_tramps`,
//!   `ioapic_enable`, `unmap_startup` and the random-number timeouts are reported;
//!   `mbuf_dma_64bit_enable` runs and reports the interface list it needs itself.
//! - `diskconf`: Limine is not boot(8), so there is no `bootdev` (`B_DEVMAGIC`) and no
//!   `bios_bootmac` (`NFSCLIENT` is not configured either): the boot device is unknown and
//!   `setroot` gets none. `dkcsumattach` (`dkcsum.c`, the BIOS disk checksums) and
//!   `dumpconf` (`machdep.c`, crash dumps) are reported; `HIBERNATE` is not configured.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::arch::amd64::amd64::bus_dma::bus_dma_init;
use crate::arch::amd64::amd64::intr::intr_printconfig;
use crate::arch::amd64::amd64::lapic::{
    lapic_boot_init, lapic_calibrate_timer, lapic_enable, lapic_set_lvt,
};
use crate::arch::amd64::amd64::machdep::x86_64_proc0_tss_ldt_init;
use crate::arch::amd64::include::cpu::cpu_info_primary;
use crate::arch::amd64::include::cpufunc::{intr_enable, lcr8};
use crate::arch::amd64::include::i82489reg::LAPIC_BASE;
use crate::kern::subr_autoconf::config_rootfound;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::mbuf_dma_64bit_enable;
use crate::machine::intr::spl0;
use crate::sys::device::{Device, Nam2blk};
use crate::sys::types::Paddr;
use crate::unported;

/// `cold`: if set, still working on cold-start.
pub use crate::sys::systm::COLD;

/// `diskconf`: the boot device (from boot(8)'s `bootdev`, none under Limine) and then
/// `setroot`.
pub fn diskconf() {
    let _ = crate::unported!("dkcsumattach (dkcsum.c)");
    // bootdev (B_DEVMAGIC) and bios_bootmac come from boot(8): none under Limine.
    crate::kern::subr_disk::setroot(None, 0, crate::sys::reboot::RB_USERREQ);
    let _ = crate::unported!("dumpconf (machdep.c)");
    // HIBERNATE: not configured.
}

/// `nam2blk[]`: the disk drivers' names and block majors (`findblkmajor`, `findblkname`).
pub static NAM2BLK: [Nam2blk; 6] = [
    Nam2blk {
        name: b"wd",
        maj: 0,
    },
    Nam2blk {
        name: b"fd",
        maj: 2,
    },
    Nam2blk {
        name: b"sd",
        maj: 4,
    },
    Nam2blk {
        name: b"cd",
        maj: 6,
    },
    Nam2blk {
        name: b"vnd",
        maj: 14,
    },
    Nam2blk {
        name: b"rd",
        maj: 17,
    },
];

/// `cpu_configure`: determine i/o configuration for a machine.
pub fn cpu_configure() {
    x86_64_proc0_tss_ldt_init();

    let _ = unported!("pmap_randomize (M6)");
    let _ = unported!("map_tramps (M6)");
    bus_dma_init();
    #[cfg(feature = "qemu")]
    crate::kern::selftest::bus_dma_check(&crate::arch::amd64::pci::pci_machdep::PCI_BUS_DMA_TAG);

    // What acpimadt (or mpbios) does before attaching the CPUs: find the LAPIC.
    // TODO(M7b): the LAPIC base comes from the MADT or the MP tables; this is the
    // architectural default.
    lapic_boot_init(Paddr::new(LAPIC_BASE));

    if config_rootfound(b"mainbus", ptr::null_mut()).is_none() {
        panic(format_args!("configure: mainbus not configured"));
    }

    // mainbus attached cpu0 as CPU_ROLE_SP (cpu_intr_init); what the boot processor's
    // attach would add (lapic_enable, lapic_calibrate_timer) and the LVT the C programs for
    // the IOAPIC below, before the interrupts are let through.
    lapic_enable();
    lapic_set_lvt();
    // SAFETY: the IDT, the PIC, the LAPIC and the masks are set up.
    unsafe { intr_enable() };
    lapic_calibrate_timer(cpu_info_primary());

    intr_printconfig();

    mbuf_dma_64bit_enable();

    // NIOAPIC > 0: lapic_set_lvt (done above), ioapic_enable.
    let _ = unported!("ioapic_enable (ioapic.c)");

    let _ = unported!("unmap_startup (M6)");

    // SAFETY: 0 lets every interrupt through, the boot value.
    unsafe { lcr8(0) };
    spl0();
    COLD.store(false, Ordering::Relaxed);

    // At this point the RNG is running, and if FSXR is set we can use it. Here we setup a
    // periodic timeout to collect the data: the viac3_rnd and rdrand timeouts.
    let _ = unported!("viac3_rnd/rdrand timeouts (identcpu.c)");
    // CRYPTO: not configured.
}

/// `device_register`: nothing to note on amd64.
pub fn device_register(_dev: &Device, _aux: *mut c_void) {}

// diskconf: setroot, dumpconf and the boot device come with disks (dkcsumattach, parsedisk).
