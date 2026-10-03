/*	$OpenBSD: autoconf.c,v 1.18 2026/06/23 11:45:54 kettenis Exp $	*/
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
//! - `unmap_startup` (with its `codepatch_disable`) and
//!   `cpu_identify_cleanup` are reported.
//! - `diskconf`: `NFSCLIENT` (the boot MAC's interface) is not configured, so `setroot` gets
//!   no boot device, as in C without it; `dumpconf` (`machdep.c`, crash dumps) is reported;
//!   `HIBERNATE` is not configured.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::arch::arm64::arm64::bus_dma::bus_dma_init;
use crate::arch::arm64::arm64::machdep::COLD;
use crate::kern::kern_softintr::softintr_init;
use crate::kern::subr_autoconf::config_rootfound;
use crate::machine::intr::{spl0, splhigh};
use crate::sys::device::{Device, Nam2blk};
use crate::unported;

/// `nam2blk[]`: the disk drivers' names and block majors (`findblkmajor`, `findblkname`).
/// `diskconf`: `setroot` with the boot device (none without `NFSCLIENT`).
pub fn diskconf() {
    crate::kern::subr_disk::setroot(None, 0, crate::sys::reboot::RB_USERREQ);
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

    let _ = unported!("cpu_identify_cleanup (M4-b)");

    // CRYPTO: not configured.

    COLD.store(false, Ordering::Relaxed);
    spl0();
}

// diskconf: setroot, dumpconf and the boot device come with disks (parsedisk).

/// `device_register`: nothing to note on arm64.
pub fn device_register(_dev: &Device, _aux: *mut c_void) {}
