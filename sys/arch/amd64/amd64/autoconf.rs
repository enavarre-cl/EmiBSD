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

/* <CODE> */
//! Setup the system to run on the current machine: `arch/amd64/amd64/autoconf.c`.
//!
//! Upstream: sys/arch/amd64/amd64/autoconf.c @ 3ce1f3f79392
//!
//! Status: `wip`. `cold` is `sys/systm.rs`'s; `cpu_configure` runs autoconfiguration from
//! `config_rootfound("mainbus")` (M7b), `device_register`, `diskconf` and `nam2blk[]` are
//! here; `unmap_startup` waits for the boot-only text.
//!
//! ## Deviations
//! - After `config_rootfound`, `cpu_configure` does what a boot CPU attached as `CPU_ROLE_SP`
//!   (no MADT) misses from the boot processor's attach (`lapic_enable`,
//!   `lapic_calibrate_timer`), the LVT setup (`lapic_set_lvt`, which the C does after
//!   mainbus for `NIOAPIC`) and `intr_enable`; then, where the C does, `ioapic_enable`
//!   (M13). Until M13 `lapic_boot_init` ran here before mainbus; it is now `acpimadt`'s, or
//!   mainbus's without a MADT (`mainbus.rs`). `pmap_randomize`, `map_tramps`,
//!   `unmap_startup` and the random-number timeouts are reported;
//!   `mbuf_dma_64bit_enable` runs and reports the interface list it needs itself.
//! - `diskconf`: Limine is not boot(8), so there is no `bootdev` (`B_DEVMAGIC`): the boot
//!   device is unknown and `setroot` gets none, unless the PXE boot MAC address
//!   ([`BIOS_BOOTMAC`], `bios_bootmac` of `NFSCLIENT`, a boot(8) hand-over that nothing under
//!   Limine fills in) names an interface. `dkcsumattach` (`dkcsum.c`, the BIOS disk
//!   checksums) and `dumpconf` (`machdep.c`, crash dumps) are reported; `HIBERNATE` is not
//!   configured.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

#[cfg(feature = "nfsclient")]
use libkern::StaticCell;

use crate::arch::amd64::amd64::bus_dma::bus_dma_init;
use crate::arch::amd64::amd64::intr::intr_printconfig;
use crate::arch::amd64::amd64::ioapic::ioapic_enable;
use crate::arch::amd64::amd64::lapic::{lapic_calibrate_timer, lapic_enable, lapic_set_lvt};
use crate::arch::amd64::amd64::machdep::x86_64_proc0_tss_ldt_init;
use crate::arch::amd64::include::cpu::{CPUF_BSP, cpu_info_primary};
use crate::arch::amd64::include::cpufunc::{intr_enable, lcr8};
use crate::kern::subr_autoconf::config_rootfound;
#[cfg(feature = "nfsclient")]
use crate::kern::subr_disk::parsedisk;
use crate::kern::subr_prf::panic;
#[cfg(feature = "nfsclient")]
use crate::kern::subr_prf::{Str, printf};
use crate::kern::uipc_mbuf::mbuf_dma_64bit_enable;
use crate::machine::intr::spl0;
#[cfg(feature = "nfsclient")]
use crate::net::if_::IFNETLIST;
#[cfg(feature = "nfsclient")]
use crate::net::if_ethersubr::ether_sprintf;
#[cfg(feature = "nfsclient")]
use crate::net::if_types::IFT_ETHER;
#[cfg(feature = "nfsclient")]
use crate::netinet::if_ether::{ETHER_ADDR_LEN, arpcom_of};
use crate::sys::device::{Device, Nam2blk};
use crate::unported;

/// `cold`: if set, still working on cold-start.
pub use crate::sys::systm::COLD;

/// `bios_bootmac`: the MAC address the machine PXE-booted from, which boot(8) hands over
/// (`None` under Limine, which does not boot from the network).
#[cfg(feature = "nfsclient")]
pub static BIOS_BOOTMAC: StaticCell<Option<[u8; ETHER_ADDR_LEN]>> = StaticCell::new(None);

/// `diskconf`: the boot device (from boot(8)'s `bootdev`, none under Limine; the interface
/// of the PXE boot MAC address with `NFSCLIENT`) and then `setroot`.
pub fn diskconf() {
    let _ = crate::unported!("dkcsumattach (dkcsum.c)");
    // bootdev (B_DEVMAGIC) comes from boot(8): none under Limine.
    #[cfg(feature = "nfsclient")]
    let mut bootdv: Option<&'static Device> = None;
    #[cfg(not(feature = "nfsclient"))]
    let bootdv: Option<&'static Device> = None;
    let part = 0;

    #[cfg(feature = "nfsclient")]
    // SAFETY: written only while boot(8)'s hand-over is read, before autoconfiguration ends.
    if let Some(mac) = unsafe { BIOS_BOOTMAC.read() } {
        let ifp = IFNETLIST
            .0
            .iter()
            .find(|ifp| ifp.if_type.get() == IFT_ETHER && arpcom_of(ifp).ac_enaddr.get() == mac);
        let sprintf = ether_sprintf(&mac);
        if let Some(ifp) = ifp {
            let xname = ifp.if_xname.get();
            let _ = printf(format_args!(
                "PXE boot MAC address {}, interface {}\n",
                Str(&sprintf),
                Str(&xname)
            ));
            let len = xname.iter().position(|&c| c == 0).unwrap_or(xname.len());
            bootdv = parsedisk(&xname[..len], 0).map(|(dv, _)| dv);
        } else {
            let _ = printf(format_args!(
                "PXE boot MAC address {}, interface {}\n",
                Str(&sprintf),
                "unknown"
            ));
        }
    }

    crate::kern::subr_disk::setroot(bootdv, part, crate::sys::reboot::RB_USERREQ);
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
    crate::arch::amd64::amd64::machdep::map_tramps();
    bus_dma_init();
    #[cfg(feature = "qemu")]
    crate::kern::selftest::bus_dma_check(&crate::arch::amd64::pci::pci_machdep::PCI_BUS_DMA_TAG);

    if config_rootfound(b"mainbus", ptr::null_mut()).is_none() {
        panic(format_args!("configure: mainbus not configured"));
    }

    // mainbus attached cpu0 as CPU_ROLE_SP (cpu_intr_init); what the boot processor's
    // attach would add (lapic_enable, lapic_calibrate_timer) and the LVT the C programs for
    // the IOAPIC below, before the interrupts are let through. A boot processor attached as
    // CPU_ROLE_BP (MULTIPROCESSOR) has done the first two in cpu_attach already.
    lapic_enable();
    lapic_set_lvt();
    // SAFETY: the IDT, the PIC, the LAPIC and the masks are set up.
    unsafe { intr_enable() };
    if cpu_info_primary().ci_flags.load(Ordering::Relaxed) & CPUF_BSP == 0 {
        lapic_calibrate_timer(cpu_info_primary());
    }

    intr_printconfig();

    mbuf_dma_64bit_enable();

    // NIOAPIC > 0: lapic_set_lvt (done above), ioapic_enable.
    ioapic_enable();

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
/* </CODE> */
