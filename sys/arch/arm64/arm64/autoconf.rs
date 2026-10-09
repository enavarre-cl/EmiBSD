/* <LICENSES> */
/*
 * Copyright (c) 2009 Miodrag Vallat.
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
//! Setup the system to run on the current machine: `arch/arm64/arm64/autoconf.c`.
//!
//! Upstream: sys/arch/arm64/arm64/autoconf.c @ 3ce1f3f79392
//!
//! Status: `wip`. `cpu_configure` runs autoconfiguration from `config_rootfound("mainbus")`
//! (M7b), which attaches the interrupt controller and the generic timer from the device
//! tree; `device_register`, `diskconf` and `nam2blk[]` are here; `unmap_startup` waits for
//! the boot-only text. `cold` lives in `sys/systm.rs`.
//!
//! ## Deviations
//! - `unmap_startup` (with its `codepatch_disable`) is reported; `cpu_identify_cleanup`
//!   (`cpu.rs`) runs since M11a.
//! - `diskconf`: `setroot` gets no boot device unless the boot MAC address ([`BOOTMAC`],
//!   `bootmac` of `NFSCLIENT`, which the firmware hand-over would fill in and nothing does
//!   under Limine) names an interface; `dumpconf` (`machdep.c`, crash dumps) is reported;
//!   `HIBERNATE` is not configured.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

#[cfg(feature = "nfsclient")]
use libkern::StaticCell;

use crate::arch::arm64::arm64::bus_dma::bus_dma_init;
use crate::arch::arm64::arm64::cpu::cpu_identify_cleanup;
use crate::arch::arm64::arm64::machdep::COLD;
use crate::kern::kern_softintr::softintr_init;
use crate::kern::subr_autoconf::config_rootfound;
#[cfg(feature = "nfsclient")]
use crate::kern::subr_disk::parsedisk;
use crate::machine::intr::{spl0, splhigh};
#[cfg(feature = "nfsclient")]
use crate::net::if_::IFNETLIST;
#[cfg(feature = "nfsclient")]
use crate::net::if_types::IFT_ETHER;
#[cfg(feature = "nfsclient")]
use crate::netinet::if_ether::{ETHER_ADDR_LEN, arpcom_of};
use crate::sys::device::{Device, Nam2blk};
use crate::unported;

/// `bootmac`: the MAC address the machine booted from over the network, handed over by the
/// firmware (`None` under Limine).
#[cfg(feature = "nfsclient")]
pub static BOOTMAC: StaticCell<Option<[u8; ETHER_ADDR_LEN]>> = StaticCell::new(None);

/// `diskconf`: `setroot` with the boot device (the interface of the boot MAC address with
/// `NFSCLIENT`, none otherwise).
pub fn diskconf() {
    #[cfg(feature = "nfsclient")]
    let mut bootdv: Option<&'static Device> = None;
    #[cfg(not(feature = "nfsclient"))]
    let bootdv: Option<&'static Device> = None;
    let part = 0;

    #[cfg(feature = "nfsclient")]
    // SAFETY: written only while the firmware's hand-over is read, before autoconfiguration
    // ends.
    if let Some(mac) = unsafe { BOOTMAC.read() } {
        let ifp = IFNETLIST
            .0
            .iter()
            .find(|ifp| ifp.if_type.get() == IFT_ETHER && arpcom_of(ifp).ac_enaddr.get() == mac);
        if let Some(ifp) = ifp {
            let xname = ifp.if_xname.get();
            let len = xname.iter().position(|&c| c == 0).unwrap_or(xname.len());
            bootdv = parsedisk(&xname[..len], 0).map(|(dv, _)| dv);
        }
    }

    crate::kern::subr_disk::setroot(bootdv, part, crate::sys::reboot::RB_USERREQ);
    let _ = crate::unported!("dumpconf (machdep.c)");
    // HIBERNATE: not configured.
}

/// `nam2blk[]`: the disk drivers' names and block majors.
pub static NAM2BLK: [Nam2blk; 5] = [
    Nam2blk {
        name: b"wd",
        maj: 0,
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
    splhigh();

    softintr_init();
    bus_dma_init();
    #[cfg(feature = "qemu")]
    crate::kern::selftest::bus_dma_check(&crate::arch::arm64::dev::mainbus::MAINBUS_DMA_TAG);

    let _ = config_rootfound(b"mainbus", ptr::null_mut());

    let _ = unported!("unmap_startup (M6)");

    cpu_identify_cleanup();

    // CRYPTO: not configured.

    COLD.store(false, Ordering::Relaxed);
    spl0();
}

// diskconf: setroot, dumpconf and the boot device come with disks (parsedisk).

/// `device_register`: nothing to note on arm64.
pub fn device_register(_dev: &Device, _aux: *mut c_void) {}
/* </CODE> */
