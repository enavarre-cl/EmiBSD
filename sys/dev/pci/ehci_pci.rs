/*	$OpenBSD: ehci_pci.c,v 1.33 2024/05/24 06:02:53 jsg Exp $ */
/*	$NetBSD: ehci_pci.c,v 1.15 2004/04/23 21:13:06 itojun Exp $	*/
/* <LICENSES> */
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
//! ehci(4) at pci: maps the EHCI registers (BAR 0), applies the chipset quirks, establishes
//! the INTx interrupt at `IPL_USB | IPL_MPSAFE`, takes the controller from the BIOS and
//! hands it to the machine-independent driver (`dev/usb/ehci.rs`).
//!
//! Upstream: sys/dev/pci/ehci_pci.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `EHCI_DEBUG` is not in GENERIC: the `DPRINTF`s are absent.
//! - `pci_findvendor` has no names without `PCIVERBOSE`'s table (`dev/pci/pci_subr.rs`), so
//!   the root hub's vendor string is `"vendor 0x%04x"`, as the C's fallback.
//! - `pci_find_device(NULL, ehci_sb700_match)` passes a scratch copy of the attach arguments
//!   for the found function's (`pci_find_device` here takes a `&mut`); nothing reads it.
//! - The interrupt keeps the device's name (`dv_xname`) for the life of the attachment, as
//!   `xhci_pci` does (`docs/C_TO_RUST.md`, the `const char *what` an interrupt keeps).
//! - `ehci_pci_givecontroller` is under `#if 0` in C and is not ported.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::pci::pci::pci_find_device;
use crate::dev::pci::pci_map::pci_mapreg_map;
use crate::dev::pci::pci_subr::pci_findvendor;
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_ATI_SB600_EHCI, PCI_PRODUCT_ATI_SB700_EHCI, PCI_PRODUCT_ATI_SBX00_SMB,
    PCI_PRODUCT_VIATECH_VT6202, PCI_VENDOR_ATI, PCI_VENDOR_VIATECH,
};
use crate::dev::pci::pcireg::{
    PCI_CLASS_SERIALBUS, PCI_MAPREG_TYPE_MEM, PCI_SUBCLASS_SERIALBUS_USB, pci_class, pci_interface,
    pci_product, pci_revision, pci_subclass, pci_vendor,
};
use crate::dev::pci::pcivar::{PciAttachArgs, Pcireg};
use crate::dev::usb::ehci::{ehci_activate, ehci_detach, ehci_init, ehci_intr};
use crate::dev::usb::ehcireg::{
    EHCI_CAPLENGTH, EHCI_EC_LEGSUP, EHCI_HCCPARAMS, EHCI_LEGSUP_BIOSOWNED, EHCI_LEGSUP_OSOWNED,
    EHCI_USBINTR, PCI_CBMEM, PCI_INTERFACE_EHCI, PCI_USBREV, PCI_USBREV_1_0, PCI_USBREV_1_1,
    PCI_USBREV_2_0, PCI_USBREV_MASK, PCI_USBREV_PRE_1_0, ehci_eecp_id, ehci_eecp_next,
    ehci_hcc_eecp,
};
use crate::dev::usb::ehcivar::{
    EHCIF_DROPPED_INTR_WORKAROUND, EhciSoftc, eowrite2, eread1, eread4,
};
use crate::dev::usb::usb::usbctlprint;
use crate::dev::usb::usbdi::{IPL_USB, USBD_NORMAL_COMPLETION, splhardusb};
use crate::dev::usb::usbdivar::{USBREV_2_0, USBREV_UNKNOWN};
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::{printf, snprintf};
use crate::machine::bus::bus_space_unmap;
use crate::machine::cpu::delay;
use crate::machine::intr::{IPL_MPSAFE, splx};
use crate::machine::pci_machdep::{
    PciChipsetTag, Pcitag, pci_conf_read, pci_conf_write, pci_intr_disestablish,
    pci_intr_establish, pci_intr_map, pci_intr_string,
};
use crate::sys::device::{CfMatch, Cfattach, DVACT_RESUME, Device, Softc};
use crate::sys::errno::Errno;
use core::sync::atomic::Ordering;
use libkern::strlcpy;

/// `EHCI_SBx00_WORKAROUND_REG`.
const EHCI_SBX00_WORKAROUND_REG: i32 = 0x50;
/// `EHCI_SBx00_WORKAROUND_ENABLE`.
const EHCI_SBX00_WORKAROUND_ENABLE: Pcireg = 1 << 3;
/// `EHCI_VT6202_WORKAROUND_REG`.
const EHCI_VT6202_WORKAROUND_REG: i32 = 0x48;

/// `struct ehci_pci_softc`.
#[repr(C)]
pub struct EhciPciSoftc {
    /// `sc`: the machine-independent softc, first.
    pub sc: EhciSoftc,
    /// `sc_pc`.
    pub sc_pc: Cell<Option<PciChipsetTag>>,
    /// `sc_tag`.
    pub sc_tag: Cell<Pcitag>,
    /// `sc_ih`: interrupt vectoring.
    pub sc_ih: Cell<Option<NonNull<c_void>>>,
}

impl EhciPciSoftc {
    /// `sc->sc_pc`. Panics before the attach set it.
    fn pc(&self) -> PciChipsetTag {
        match self.sc_pc.get() {
            Some(pc) => pc,
            None => crate::kern::subr_prf::panic(format_args!("ehci_pci: no chipset tag")),
        }
    }
}

// SAFETY: `#[repr(C)]` with the `EhciSoftc` (itself headed by the device) first; the other
// members are `Cell`s of an `Option` of a tag, a `pcitag_t` (an integer) and an
// `Option<NonNull>`: all valid as zero bits.
unsafe impl Softc for EhciPciSoftc {}

/// `ehci_pci_ca`.
pub static EHCI_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<EhciPciSoftc>(),
    ca_match: Some(ehci_pci_match),
    ca_attach: ehci_pci_attach,
    ca_detach: Some(ehci_pci_detach),
    ca_activate: Some(ehci_pci_activate),
};

/// `ehci_pci_match`: any serial bus / USB / EHCI function.
pub fn ehci_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args` valid during the match.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    if pci_class(pa.pa_class) == PCI_CLASS_SERIALBUS
        && pci_subclass(pa.pa_class) == PCI_SUBCLASS_SERIALBUS_USB
        && pci_interface(pa.pa_class) == PCI_INTERFACE_EHCI
    {
        return 1;
    }

    0
}

/// `ehci_pci_attach`.
pub fn ehci_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `ehci_pci_ca`, whose softc is an `EhciPciSoftc`; attached
    // devices are not freed while the kernel uses them (detach disestablishes the interrupt
    // and detaches `usb` first).
    let sc: &'static EhciPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<EhciPciSoftc>()) };
    // SAFETY: pci attaches its children with a `pci_attach_args` valid during the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let pc = pa.pa_pc;
    let tag = pa.pa_tag;
    let esc = &sc.sc;
    // SAFETY: the device's name lives as long as the device, and the interrupt is
    // disestablished before the device can go (`ehci_pci_detach`).
    let devname: &'static str = unsafe { &*ptr::from_ref(esc.sc_bus.bdev.xname()) };

    // Map I/O registers
    let Ok((iot, ioh, _, size)) = pci_mapreg_map(pa, PCI_CBMEM, PCI_MAPREG_TYPE_MEM, 0, 0) else {
        printf(format_args!(": can't map mem space\n"));
        return;
    };
    esc.iot.set(Some(iot));
    esc.ioh.set(Some(ioh));
    esc.sc_size.set(size);

    sc.sc_pc.set(Some(pc));
    sc.sc_tag.set(tag);
    esc.sc_bus.dmatag.set(Some(pa.pa_dmat));

    // Disable interrupts, so we don't get any spurious ones.
    let s = splhardusb();
    esc.sc_offs
        .store(usize::from(eread1(esc, EHCI_CAPLENGTH)), Ordering::Relaxed);
    eowrite2(esc, EHCI_USBINTR, 0);

    // Handle quirks
    match pci_vendor(pa.pa_id) {
        PCI_VENDOR_ATI => {
            let mut found = *pa;
            if pci_product(pa.pa_id) == PCI_PRODUCT_ATI_SB600_EHCI
                || (pci_product(pa.pa_id) == PCI_PRODUCT_ATI_SB700_EHCI
                    && pci_find_device(&mut found, ehci_sb700_match) != 0)
            {
                // apply the ATI SB600/SB700 workaround
                let value = pci_conf_read(sc.pc(), sc.sc_tag.get(), EHCI_SBX00_WORKAROUND_REG);
                pci_conf_write(
                    sc.pc(),
                    sc.sc_tag.get(),
                    EHCI_SBX00_WORKAROUND_REG,
                    value | EHCI_SBX00_WORKAROUND_ENABLE,
                );
            }
        }
        PCI_VENDOR_VIATECH
            if pci_product(pa.pa_id) == PCI_PRODUCT_VIATECH_VT6202
                && (pci_revision(pa.pa_class) & 0xf0) == 0x60 =>
        {
            // The VT6202 defaults to a 1 usec EHCI sleep time which hogs the PCI bus
            // *badly*. Setting bit 5 of the register makes that sleep time use the
            // conventional 10 usec.
            let value = pci_conf_read(sc.pc(), sc.sc_tag.get(), EHCI_VT6202_WORKAROUND_REG);
            pci_conf_write(
                sc.pc(),
                sc.sc_tag.get(),
                EHCI_VT6202_WORKAROUND_REG,
                value | 0x20000000,
            );
        }
        _ => {}
    }

    'unmap_ret: {
        // Map and establish the interrupt.
        let Some(ih) = pci_intr_map(pa) else {
            printf(format_args!(": couldn't map interrupt\n"));
            break 'unmap_ret;
        };
        let intrstr = pci_intr_string(pc, ih);
        let Some(cookie) = pci_intr_establish(
            pc,
            ih,
            IPL_USB | IPL_MPSAFE,
            ehci_intr,
            ptr::from_ref(esc).cast_mut().cast(),
            devname,
        ) else {
            printf(format_args!(": couldn't establish interrupt"));
            printf(format_args!(" at {intrstr}"));
            printf(format_args!("\n"));
            break 'unmap_ret;
        };
        sc.sc_ih.set(Some(cookie));
        printf(format_args!(": {intrstr}\n"));

        'disestablish_ret: {
            match pci_conf_read(pc, tag, PCI_USBREV) & PCI_USBREV_MASK {
                PCI_USBREV_PRE_1_0 | PCI_USBREV_1_0 | PCI_USBREV_1_1 => {
                    esc.sc_bus.usbrev.set(USBREV_UNKNOWN);
                    printf(format_args!("{devname}: pre-2.0 USB rev\n"));
                    break 'disestablish_ret;
                }
                PCI_USBREV_2_0 => esc.sc_bus.usbrev.set(USBREV_2_0),
                _ => esc.sc_bus.usbrev.set(USBREV_UNKNOWN),
            }

            // Figure out vendor for root hub descriptor.
            let vendor = pci_findvendor(pa.pa_id);
            esc.sc_id_vendor.set(pci_vendor(pa.pa_id) as i32);
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
            esc.sc_vendor.set(buf);

            // Enable workaround for dropped interrupts as required
            match esc.sc_id_vendor.get() as u32 {
                PCI_VENDOR_ATI | PCI_VENDOR_VIATECH => {
                    esc.sc_flags
                        .fetch_or(EHCIF_DROPPED_INTR_WORKAROUND, Ordering::Relaxed);
                }
                _ => {}
            }

            ehci_pci_takecontroller(sc, false);
            let r = ehci_init(esc);
            if r != USBD_NORMAL_COMPLETION {
                printf(format_args!("{devname}: init failed, error={}\n", r as i32));
                break 'disestablish_ret;
            }

            // Attach usb device.
            config_found(
                self_,
                ptr::from_ref(&esc.sc_bus).cast_mut().cast(),
                Some(usbctlprint),
            );
            splx(s);
            return;
        }

        // disestablish_ret:
        // SAFETY: the cookie came from pci_intr_establish above and is dropped here.
        unsafe { pci_intr_disestablish(sc.pc(), cookie) };
        sc.sc_ih.set(None);
    }

    // unmap_ret:
    bus_space_unmap(iot, ioh, size);
    esc.sc_size.set(0);
    splx(s);
}

/// `ehci_pci_activate`.
pub fn ehci_pci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `ehci_pci_ca`.
    let sc = unsafe { self_.softc::<EhciPciSoftc>() };

    if sc.sc.sc_size.get() == 0 {
        return Ok(());
    }

    if act == DVACT_RESUME {
        ehci_pci_takecontroller(sc, true);
    }

    ehci_activate(self_, act)
}

/// `ehci_pci_detach`.
pub fn ehci_pci_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `ehci_pci_ca`.
    let sc = unsafe { self_.softc::<EhciPciSoftc>() };

    ehci_detach(self_, flags)?;
    if let Some(ih) = sc.sc_ih.get() {
        // SAFETY: the cookie came from pci_intr_establish and is dropped here.
        unsafe { pci_intr_disestablish(sc.pc(), ih) };
        sc.sc_ih.set(None);
    }
    if sc.sc.sc_size.get() != 0 {
        let (iot, ioh) = sc.sc.regs();
        bus_space_unmap(iot, ioh, sc.sc.sc_size.get());
        sc.sc.sc_size.set(0);
    }
    Ok(())
}

/// `ehci_pci_takecontroller`: synchronises with the BIOS if it owns the controller (the
/// legacy support capability's semaphores, in PCI configuration space).
pub fn ehci_pci_takecontroller(sc: &EhciPciSoftc, silent: bool) {
    let pc = sc.pc();
    let tag = sc.sc_tag.get();

    let cparams = eread4(&sc.sc, EHCI_HCCPARAMS);
    // Synchronise with the BIOS if it owns the controller.
    let mut eecp = ehci_hcc_eecp(cparams);
    while eecp != 0 {
        let eec = pci_conf_read(pc, tag, eecp as i32);
        let next = ehci_eecp_next(eec);
        if ehci_eecp_id(eec) == EHCI_EC_LEGSUP {
            let mut legsup = eec;
            if legsup & EHCI_LEGSUP_BIOSOWNED != 0 {
                pci_conf_write(pc, tag, eecp as i32, legsup | EHCI_LEGSUP_OSOWNED);
                for _ in 0..5000 {
                    legsup = pci_conf_read(pc, tag, eecp as i32);
                    if legsup & EHCI_LEGSUP_BIOSOWNED == 0 {
                        break;
                    }
                    delay(1000);
                }
                if !silent && legsup & EHCI_LEGSUP_BIOSOWNED != 0 {
                    printf(format_args!(
                        "{}: timed out waiting for BIOS\n",
                        sc.sc.sc_bus.bdev.xname()
                    ));
                }
            }
        }
        eecp = next;
    }
}

/// `ehci_sb700_match`: the ATI SBx00 SMBus revisions whose SB700 EHCI needs the workaround.
pub fn ehci_sb700_match(pa: &PciAttachArgs) -> i32 {
    if pci_vendor(pa.pa_id) == PCI_VENDOR_ATI
        && pci_product(pa.pa_id) == PCI_PRODUCT_ATI_SBX00_SMB
        && (pci_revision(pa.pa_class) == 0x3a || pci_revision(pa.pa_class) == 0x3b)
    {
        return 1;
    }

    0
}
/* </CODE> */
