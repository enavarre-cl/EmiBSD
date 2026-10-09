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

/*
 * Copyright (c) 2001, 2002 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net).
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
//! xhci(4) at pci: maps the xHCI registers (BAR 0), establishes the interrupt (MSI-X, then
//! MSI, then INTx, at `IPL_USB | IPL_MPSAFE`), takes the controller from the BIOS and hands
//! it to the machine-independent driver (`dev/usb/xhci.rs`).
//!
//! Upstream: sys/dev/pci/xhci_pci.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `XHCI_DEBUG` is not in GENERIC: the `DPRINTF`s of `xhci_pci_port_route` and
//!   `xhci_pci_takecontroller` are absent.
//! - `pci_findvendor` has no names without `PCIVERBOSE`'s table (`dev/pci/pci_subr.rs`), so
//!   the root hub's vendor string is `"vendor 0x%04x"`, as the C's fallback.
//! - The interrupt keeps the device's name (`dv_xname`) for the life of the attachment, as
//!   `com_isa` does (`docs/C_TO_RUST.md`, the `const char *what` an interrupt keeps).
//! - `xhci_pci_port_route` returns nothing (the C's constant 0 is never read).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::pci::pci::{PCI_DOPM, pci_set_powerstate};
use crate::dev::pci::pci_map::{pci_mapreg_map, pci_mapreg_type};
use crate::dev::pci::pci_subr::pci_findvendor;
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_FRESCO_FL1000, PCI_PRODUCT_FRESCO_FL1400, PCI_VENDOR_AMD, PCI_VENDOR_FRESCO,
    PCI_VENDOR_INTEL,
};
use crate::dev::pci::pcireg::{
    PCI_CLASS_SERIALBUS, PCI_SUBCLASS_SERIALBUS_USB, pci_class, pci_interface, pci_product,
    pci_subclass, pci_vendor,
};
use crate::dev::pci::pcivar::{PCI_FLAGS_MSI_ENABLED, PciAttachArgs, Pcireg};
use crate::dev::usb::usb::usbctlprint;
use crate::dev::usb::usbdi::IPL_USB;
use crate::dev::usb::xhci::{xhci_activate, xhci_config, xhci_detach, xhci_init, xhci_intr};
use crate::dev::usb::xhcireg::{
    PCI_CBMEM, PCI_INTERFACE_XHCI, PCI_XHCI_INTEL_USB3_PSSEN, PCI_XHCI_INTEL_USB3PRM,
    PCI_XHCI_INTEL_XUSB2PR, PCI_XHCI_INTEL_XUSB2PRM, XHCI_HCCPARAMS, XHCI_ID_USB_LEGACY,
    XHCI_XECP_BIOS_SEM, XHCI_XECP_OS_SEM, xhci_hcc_xecp, xhci_xecp_id, xhci_xecp_next,
};
use crate::dev::usb::xhcivar::{XHCI_NOCSS, XhciSoftc, xread1, xread4, xwrite1};
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::{Str, printf, snprintf};
use crate::machine::bus::bus_space_unmap;
use crate::machine::cpu::delay;
use crate::machine::intr::IPL_MPSAFE;
use crate::machine::pci_machdep::{
    PciChipsetTag, Pcitag, pci_conf_read, pci_conf_write, pci_intr_disestablish,
    pci_intr_establish, pci_intr_map, pci_intr_map_msi, pci_intr_map_msix, pci_intr_string,
    pci_min_powerstate,
};
use crate::sys::device::{CfMatch, Cfattach, DVACT_POWERDOWN, DVACT_RESUME, Device, Softc};
use crate::sys::errno::Errno;
use core::sync::atomic::Ordering;
use libkern::strlcpy;

/// `struct xhci_pci_softc`.
#[repr(C)]
pub struct XhciPciSoftc {
    /// `sc`: the machine-independent softc, first.
    pub sc: XhciSoftc,
    /// `sc_pc`.
    pub sc_pc: Cell<Option<PciChipsetTag>>,
    /// `sc_tag`.
    pub sc_tag: Cell<Pcitag>,
    /// `sc_id`.
    pub sc_id: Cell<Pcireg>,
    /// `sc_ih`: interrupt vectoring.
    pub sc_ih: Cell<Option<NonNull<c_void>>>,
}

impl XhciPciSoftc {
    /// `psc->sc_pc`. Panics before the attach set it.
    fn pc(&self) -> PciChipsetTag {
        match self.sc_pc.get() {
            Some(pc) => pc,
            None => crate::kern::subr_prf::panic(format_args!("xhci_pci: no chipset tag")),
        }
    }
}

// SAFETY: `#[repr(C)]` with the `XhciSoftc` (itself headed by the device) first; the other
// members are `Cell`s of an `Option` of a tag, a `pcitag_t` and a `pcireg_t` (integers) and
// an `Option<NonNull>`: all valid as zero bits.
unsafe impl Softc for XhciPciSoftc {}

/// `xhci_pci_ca`.
pub static XHCI_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<XhciPciSoftc>(),
    ca_match: Some(xhci_pci_match),
    ca_attach: xhci_pci_attach,
    ca_detach: Some(xhci_pci_detach),
    ca_activate: Some(xhci_pci_activate),
};

/// `xhci_pci_match`: any serial bus / USB / xHCI function.
pub fn xhci_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args` valid during the match.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    if pci_class(pa.pa_class) == PCI_CLASS_SERIALBUS
        && pci_subclass(pa.pa_class) == PCI_SUBCLASS_SERIALBUS_USB
        && pci_interface(pa.pa_class) == PCI_INTERFACE_XHCI
    {
        return 1;
    }

    0
}

/// `xhci_pci_port_route`: switches the ports the BIOS lets the OS route from EHCI to xHCI
/// (Intel).
fn xhci_pci_port_route(psc: &XhciPciSoftc) {
    let pc = psc.pc();
    let tag = psc.sc_tag.get();

    // Check USB3 Port Routing Mask register that indicates the ports can be changed from OS,
    // and turn on by USB3 Port SS Enable register.
    let val = pci_conf_read(pc, tag, PCI_XHCI_INTEL_USB3PRM);

    pci_conf_write(pc, tag, PCI_XHCI_INTEL_USB3_PSSEN, val);
    let _ = pci_conf_read(pc, tag, PCI_XHCI_INTEL_USB3_PSSEN);

    // Check USB2 Port Routing Mask register that indicates the USB2.0 ports to be controlled
    // by xHCI HC, and switch them to xHCI HC.
    let val = pci_conf_read(pc, tag, PCI_XHCI_INTEL_XUSB2PRM);

    pci_conf_write(pc, tag, PCI_XHCI_INTEL_XUSB2PR, val);
    let _ = pci_conf_read(pc, tag, PCI_XHCI_INTEL_XUSB2PR);
}

/// `xhci_pci_attach`.
pub fn xhci_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `xhci_pci_ca`, whose softc is an `XhciPciSoftc`; attached
    // devices are not freed while the kernel uses them (detach disestablishes the interrupt
    // and detaches `usb` first).
    let psc: &'static XhciPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<XhciPciSoftc>()) };
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach and lets them change it (`pa_flags` below), as the C does.
    let pa = unsafe { &mut *aux.cast::<PciAttachArgs>() };
    let sc = &psc.sc;

    let reg = pci_mapreg_type(pa.pa_pc, pa.pa_tag, PCI_CBMEM);
    let Ok((iot, ioh, _, size)) = pci_mapreg_map(pa, PCI_CBMEM, reg, 0, 0) else {
        printf(format_args!(": can't map mem space\n"));
        return;
    };
    sc.iot.set(Some(iot));
    sc.ioh.set(Some(ioh));
    sc.sc_size.set(size);

    psc.sc_pc.set(Some(pa.pa_pc));
    psc.sc_tag.set(pa.pa_tag);
    psc.sc_id.set(pa.pa_id);
    sc.sc_bus.dmatag.set(Some(pa.pa_dmat));

    // Handle quirks
    match pci_vendor(pa.pa_id) {
        PCI_VENDOR_FRESCO => {
            // FL1000 / FL1400 claim MSI support but do not support MSI
            if pci_product(pa.pa_id) == PCI_PRODUCT_FRESCO_FL1000
                || pci_product(pa.pa_id) == PCI_PRODUCT_FRESCO_FL1400
            {
                pa.pa_flags &= !PCI_FLAGS_MSI_ENABLED;
            }
        }
        PCI_VENDOR_AMD => sc.sc_flags.set(sc.sc_flags.get() | XHCI_NOCSS),
        _ => {}
    }

    'unmap_ret: {
        // Map and establish the interrupt.
        let Some(ih) = pci_intr_map_msix(pa, 0)
            .or_else(|| pci_intr_map_msi(pa))
            .or_else(|| pci_intr_map(pa))
        else {
            printf(format_args!(": couldn't map interrupt\n"));
            break 'unmap_ret;
        };
        let intrstr = pci_intr_string(pa.pa_pc, ih);

        // SAFETY: the device's name lives as long as the device, and the interrupt is
        // disestablished before the device can go (`xhci_pci_detach`).
        let name: &'static str = unsafe { &*ptr::from_ref(sc.sc_bus.bdev.xname()) };
        let Some(cookie) = pci_intr_establish(
            pa.pa_pc,
            ih,
            IPL_USB | IPL_MPSAFE,
            xhci_intr,
            ptr::from_ref(sc).cast_mut().cast(),
            name,
        ) else {
            printf(format_args!(": couldn't establish interrupt"));
            printf(format_args!(" at {intrstr}"));
            printf(format_args!("\n"));
            break 'unmap_ret;
        };
        psc.sc_ih.set(Some(cookie));
        printf(format_args!(": {intrstr}"));

        // Figure out vendor for root hub descriptor.
        let vendor = pci_findvendor(pa.pa_id);
        sc.sc_id_vendor.set(pci_vendor(pa.pa_id) as i32);
        let mut buf = [0u8; 16];
        match vendor {
            Some(v) => {
                strlcpy(&mut buf, v);
            }
            None => {
                snprintf(
                    &mut buf,
                    format_args!("vendor 0x{:04x}", pci_vendor(pa.pa_id)),
                );
            }
        }
        sc.sc_vendor.set(buf);

        xhci_pci_takecontroller(psc, false);

        if let Err(error) = xhci_init(sc) {
            printf(format_args!(
                "{}: init failed, error={}\n",
                sc.sc_bus.bdev.xname(),
                error as i32
            ));
            // disestablish_ret:
            // SAFETY: the cookie came from pci_intr_establish above and is dropped here.
            unsafe { pci_intr_disestablish(psc.pc(), cookie) };
            psc.sc_ih.set(None);
            break 'unmap_ret;
        }

        if pci_vendor(psc.sc_id.get()) == PCI_VENDOR_INTEL {
            xhci_pci_port_route(psc);
        }

        // Attach usb device.
        config_found(
            self_,
            ptr::from_ref(&sc.sc_bus).cast_mut().cast(),
            Some(usbctlprint),
        );

        // Now that the stack is ready, config' the HC and enable interrupts.
        xhci_config(sc);

        return;
    }

    // unmap_ret:
    bus_space_unmap(iot, ioh, size);
}

/// `xhci_pci_detach`.
pub fn xhci_pci_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `xhci_pci_ca`.
    let psc = unsafe { self_.softc::<XhciPciSoftc>() };

    xhci_detach(self_, flags)?;
    if let Some(ih) = psc.sc_ih.get() {
        // SAFETY: the cookie came from pci_intr_establish and is dropped here.
        unsafe { pci_intr_disestablish(psc.pc(), ih) };
        psc.sc_ih.set(None);
    }
    if psc.sc.sc_size.get() != 0 {
        let (iot, ioh) = psc.sc.regs();
        bus_space_unmap(iot, ioh, psc.sc.sc_size.get());
        psc.sc.sc_size.set(0);
    }
    Ok(())
}

/// `xhci_pci_activate`.
pub fn xhci_pci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `xhci_pci_ca`.
    let psc = unsafe { self_.softc::<XhciPciSoftc>() };

    if act == DVACT_RESUME && pci_vendor(psc.sc_id.get()) == PCI_VENDOR_INTEL {
        xhci_pci_port_route(psc);
    }

    let rc = xhci_activate(self_, act);

    if act == DVACT_POWERDOWN && PCI_DOPM.load(Ordering::Relaxed) != 0 {
        // Put the controller into its minimal power state now such that we can tell its
        // companion USB4 controller to go to sleep.
        let pc = psc.pc();
        let tag = psc.sc_tag.get();
        pci_set_powerstate(pc, tag, pci_min_powerstate(pc, tag) as i32);
    }

    rc
}

/// `xhci_pci_takecontroller`: synchronises with the BIOS if it owns the controller (the USB
/// legacy support capability's semaphores).
pub fn xhci_pci_takecontroller(psc: &XhciPciSoftc, silent: bool) {
    let sc = &psc.sc;

    let cparams = xread4(sc, XHCI_HCCPARAMS);
    if cparams == 0xffffffff {
        return;
    }

    let mut eec: u32 = u32::MAX;

    // Synchronise with the BIOS if it owns the controller.
    let mut xecp = (xhci_hcc_xecp(cparams) << 2) as usize;
    while xecp != 0 && xhci_xecp_next(eec) != 0 {
        eec = xread4(sc, xecp);
        if eec == 0xffffffff {
            return;
        }
        if xhci_xecp_id(eec) == XHCI_ID_USB_LEGACY {
            let mut bios_sem = xread1(sc, xecp + XHCI_XECP_BIOS_SEM);
            if bios_sem != 0 {
                xwrite1(sc, xecp + XHCI_XECP_OS_SEM, 1);
                for _ in 0..5000 {
                    bios_sem = xread1(sc, xecp + XHCI_XECP_BIOS_SEM);
                    if bios_sem == 0 {
                        break;
                    }
                    delay(1000);
                }
                if !silent && bios_sem != 0 {
                    printf(format_args!(
                        "{}: timed out waiting for BIOS\n",
                        Str(sc.sc_bus.bdev.xname().as_bytes())
                    ));
                }
            }
        }
        xecp += (xhci_xecp_next(eec) << 2) as usize;
    }
}
/* </CODE> */
