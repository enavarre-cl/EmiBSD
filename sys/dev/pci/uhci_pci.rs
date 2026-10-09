/* <LICENSES> */
/*
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net) at
 * Carlstedt Research & Technology.
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
//! uhci(4) at pci: maps the UHCI I/O registers (`PCI_CBIO`), establishes the INTx interrupt
//! at `IPL_USB`, sets the legacy support register and, once autoconf has attached the
//! other devices (`config_defer`), hands the controller to the machine-independent driver
//! (`dev/usb/uhci.rs`) and attaches `usb(4)`.
//!
//! Upstream: sys/dev/pci/uhci_pci.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `pci_findvendor` has no names without `PCIVERBOSE`'s table (`dev/pci/pci_subr.rs`), so
//!   the root hub's vendor string is `"vendor 0x%04x"`, as the C's fallback.
//! - The interrupt keeps the device's name (`dv_xname`) for the life of the attachment, as
//!   `xhci_pci` and `ehci_pci` do (`docs/C_TO_RUST.md`, the `const char *what` an interrupt
//!   keeps).
//! - `uhci_pci_attach_deferred`'s failure path clears `sc_ih` after disestablishing the
//!   interrupt, so that a later `uhci_pci_detach` does not disestablish it a second time (the
//!   C leaves the stale cookie, a double free).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::pci::pci_map::pci_mapreg_map;
use crate::dev::pci::pci_subr::pci_findvendor;
use crate::dev::pci::pcireg::{
    PCI_CLASS_SERIALBUS, PCI_MAPREG_TYPE_IO, PCI_SUBCLASS_SERIALBUS_USB, pci_class, pci_interface,
    pci_subclass, pci_vendor,
};
use crate::dev::pci::pcivar::PciAttachArgs;
use crate::dev::usb::uhci::{uhci_activate, uhci_detach, uhci_init, uhci_intr, uhci_run};
use crate::dev::usb::uhcireg::{
    PCI_CBIO, PCI_INTERFACE_UHCI, PCI_LEGSUP, PCI_LEGSUP_USBPIRQDEN, PCI_USBREV, PCI_USBREV_1_0,
    PCI_USBREV_1_1, PCI_USBREV_MASK, PCI_USBREV_PRE_1_0, UHCI_INTR,
};
use crate::dev::usb::uhcivar::UhciSoftc;
use crate::dev::usb::usb::usbctlprint;
use crate::dev::usb::usbdi::{IPL_USB, USBD_NORMAL_COMPLETION, splhardusb};
use crate::dev::usb::usbdivar::{USBREV_1_0, USBREV_1_1, USBREV_PRE_1_0, USBREV_UNKNOWN};
use crate::kern::subr_autoconf::{config_defer, config_found};
use crate::kern::subr_prf::{printf, snprintf};
use crate::machine::bus::{
    BUS_SPACE_BARRIER_READ, BUS_SPACE_BARRIER_WRITE, bus_space_barrier, bus_space_unmap,
    bus_space_write_2,
};
use crate::machine::intr::splx;
use crate::machine::pci_machdep::{
    PciChipsetTag, Pcitag, pci_conf_read, pci_conf_write, pci_intr_disestablish,
    pci_intr_establish, pci_intr_map, pci_intr_string,
};
use crate::sys::device::{CfMatch, Cfattach, DVACT_RESUME, Device, Softc};
use crate::sys::errno::Errno;
use libkern::strlcpy;

/// `struct uhci_pci_softc`.
#[repr(C)]
pub struct UhciPciSoftc {
    /// `sc`: the machine-independent softc, first.
    pub sc: UhciSoftc,
    /// `sc_pc`.
    pub sc_pc: Cell<Option<PciChipsetTag>>,
    /// `sc_tag`.
    pub sc_tag: Cell<Pcitag>,
    /// `sc_ih`: interrupt vectoring.
    pub sc_ih: Cell<Option<NonNull<c_void>>>,
}

impl UhciPciSoftc {
    /// `sc->sc_pc`. Panics before the attach set it.
    fn pc(&self) -> PciChipsetTag {
        match self.sc_pc.get() {
            Some(pc) => pc,
            None => crate::kern::subr_prf::panic(format_args!("uhci_pci: no chipset tag")),
        }
    }
}

// SAFETY: `#[repr(C)]` with the `UhciSoftc` (itself headed by the device) first; the other
// members are `Cell`s of an `Option` of a tag, a `pcitag_t` (an integer) and an
// `Option<NonNull>`: all valid as zero bits.
unsafe impl Softc for UhciPciSoftc {}

/// `uhci_pci_ca`.
pub static UHCI_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UhciPciSoftc>(),
    ca_match: Some(uhci_pci_match),
    ca_attach: uhci_pci_attach,
    ca_detach: Some(uhci_pci_detach),
    ca_activate: Some(uhci_pci_activate),
};

/// `uhci_pci_match`: any serial bus / USB / UHCI function.
pub fn uhci_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args` valid during the match.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    if pci_class(pa.pa_class) == PCI_CLASS_SERIALBUS
        && pci_subclass(pa.pa_class) == PCI_SUBCLASS_SERIALBUS_USB
        && pci_interface(pa.pa_class) == PCI_INTERFACE_UHCI
    {
        return 1;
    }

    0
}

/// `uhci_pci_activate`: on resume, set legacy support attribute and enable intrs.
pub fn uhci_pci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `uhci_pci_ca`.
    let sc = unsafe { self_.softc::<UhciPciSoftc>() };

    if sc.sc.sc_size.get() == 0 {
        return Ok(());
    }

    // On resume, set legacy support attribute and enable intrs
    if act == DVACT_RESUME {
        pci_conf_write(sc.pc(), sc.sc_tag.get(), PCI_LEGSUP, PCI_LEGSUP_USBPIRQDEN);
        let (iot, ioh) = sc.sc.regs();
        bus_space_barrier(
            iot,
            ioh,
            0,
            sc.sc.sc_size.get(),
            BUS_SPACE_BARRIER_READ | BUS_SPACE_BARRIER_WRITE,
        );
        bus_space_write_2(iot, ioh, UHCI_INTR, 0);
    }

    uhci_activate(self_, act)
}

/// `uhci_pci_attach`.
pub fn uhci_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `uhci_pci_ca`, whose softc is a `UhciPciSoftc`; attached
    // devices are not freed while the kernel uses them (detach disestablishes the interrupt
    // and detaches `usb` first).
    let sc: &'static UhciPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<UhciPciSoftc>()) };
    // SAFETY: pci attaches its children with a `pci_attach_args` valid during the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let pc = pa.pa_pc;
    let tag = pa.pa_tag;
    let usc = &sc.sc;
    // SAFETY: the device's name lives as long as the device, and the interrupt is
    // disestablished before the device can go (`uhci_pci_detach`).
    let devname: &'static str = unsafe { &*ptr::from_ref(usc.sc_bus.bdev.xname()) };

    // Map I/O registers
    let Ok((iot, ioh, _, size)) = pci_mapreg_map(pa, PCI_CBIO, PCI_MAPREG_TYPE_IO, 0, 0) else {
        printf(format_args!(": can't map i/o space\n"));
        return;
    };
    usc.iot.set(Some(iot));
    usc.ioh.set(Some(ioh));
    usc.sc_size.set(size);

    // Disable interrupts, so we don't get any spurious ones.
    let s = splhardusb();
    bus_space_write_2(iot, ioh, UHCI_INTR, 0);

    sc.sc_pc.set(Some(pc));
    sc.sc_tag.set(tag);
    usc.sc_bus.dmatag.set(Some(pa.pa_dmat));

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
            IPL_USB,
            uhci_intr,
            ptr::from_ref(usc).cast_mut().cast(),
            devname,
        ) else {
            printf(format_args!(": couldn't establish interrupt"));
            printf(format_args!(" at {intrstr}"));
            printf(format_args!("\n"));
            break 'unmap_ret;
        };
        sc.sc_ih.set(Some(cookie));
        printf(format_args!(": {intrstr}\n"));

        // Set LEGSUP register to its default value.
        pci_conf_write(pc, tag, PCI_LEGSUP, PCI_LEGSUP_USBPIRQDEN);

        usc.sc_bus
            .usbrev
            .set(match pci_conf_read(pc, tag, PCI_USBREV) & PCI_USBREV_MASK {
                PCI_USBREV_PRE_1_0 => USBREV_PRE_1_0,
                PCI_USBREV_1_0 => USBREV_1_0,
                PCI_USBREV_1_1 => USBREV_1_1,
                _ => USBREV_UNKNOWN,
            });

        let _ = uhci_run(usc, 0); // stop the controller
        // disable interrupts
        bus_space_barrier(
            iot,
            ioh,
            0,
            size,
            BUS_SPACE_BARRIER_READ | BUS_SPACE_BARRIER_WRITE,
        );
        bus_space_write_2(iot, ioh, UHCI_INTR, 0);

        // Figure out vendor for root hub descriptor.
        let vendor = pci_findvendor(pa.pa_id);
        usc.sc_id_vendor.set(pci_vendor(pa.pa_id) as i32);
        let mut buf = [0u8; 32];
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
        usc.sc_vendor.set(buf);

        config_defer(self_, uhci_pci_attach_deferred);

        // Ignore interrupts for now
        usc.sc_bus.dying.set(1);

        splx(s);

        return;
    }

    // unmap_ret:
    bus_space_unmap(iot, ioh, size);
    usc.sc_size.set(0);
    splx(s);
}

/// `uhci_pci_attach_deferred`: initialises the controller and attaches `usb(4)` once the
/// other devices are attached.
pub fn uhci_pci_attach_deferred(self_: &Device) {
    // SAFETY: as in `uhci_pci_attach`: the device was made for `uhci_pci_ca` and lives as
    // long as the controller.
    let sc: &'static UhciPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<UhciPciSoftc>()) };
    let usc = &sc.sc;
    let devname = usc.sc_bus.bdev.xname();

    let s = splhardusb();

    usc.sc_bus.dying.set(0);
    let r = uhci_init(usc);
    if r != USBD_NORMAL_COMPLETION {
        printf(format_args!("{devname}: init failed, error={}\n", r as i32));
        // unmap_ret:
        let (iot, ioh) = usc.regs();
        bus_space_unmap(iot, ioh, usc.sc_size.get());
        if let Some(ih) = sc.sc_ih.get() {
            // SAFETY: the cookie came from pci_intr_establish in the attach and is dropped
            // here.
            unsafe { pci_intr_disestablish(sc.pc(), ih) };
        }
        sc.sc_ih.set(None);
        usc.sc_size.set(0);
        splx(s);
        return;
    }
    splx(s);

    // Attach usb device.
    config_found(
        self_,
        ptr::from_ref(&usc.sc_bus).cast_mut().cast(),
        Some(usbctlprint),
    );
}

/// `uhci_pci_detach`.
pub fn uhci_pci_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `uhci_pci_ca`.
    let sc = unsafe { self_.softc::<UhciPciSoftc>() };

    uhci_detach(self_, flags)?;
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
/* </CODE> */
