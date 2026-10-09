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
//! ohci(4) at pci: maps the OHCI registers (BAR 0), quiets the controller, establishes the
//! INTx interrupt at `IPL_USB`, takes the controller from the SMM (BIOS) and, once the
//! children of the bus are attached (`config_defer`), hands it to the machine-independent
//! driver (`dev/usb/ohci.rs`).
//!
//! Upstream: sys/dev/pci/ohci_pci.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `pci_findvendor` has no names without `PCIVERBOSE`'s table (`dev/pci/pci_subr.rs`), so
//!   the root hub's vendor string is `"vendor 0x%04x"`, as the C's fallback.
//! - The interrupt keeps the device's name (`dv_xname`) for the life of the attachment, as
//!   `xhci_pci` does (`docs/C_TO_RUST.md`, the `const char *what` an interrupt keeps).
//! - `ohci_pci_attach` and `ohci_pci_attach_deferred` clear `sc_size` (and the cookie) when
//!   they unmap the registers on a failure, so a later detach does not unmap them again.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::pci::pci_map::pci_mapreg_map;
use crate::dev::pci::pci_subr::pci_findvendor;
use crate::dev::pci::pcireg::{
    PCI_CLASS_SERIALBUS, PCI_MAPREG_TYPE_MEM, PCI_SUBCLASS_SERIALBUS_USB, pci_class, pci_interface,
    pci_subclass, pci_vendor,
};
use crate::dev::pci::pcivar::PciAttachArgs;
use crate::dev::usb::ohci::{
    ohci_activate, ohci_checkrev, ohci_detach, ohci_handover, ohci_init, ohci_intr,
};
use crate::dev::usb::ohcireg::{
    OHCI_INTERRUPT_DISABLE, OHCI_INTERRUPT_ENABLE, OHCI_MIE, PCI_CBMEM, PCI_INTERFACE_OHCI,
};
use crate::dev::usb::ohcivar::OhciSoftc;
use crate::dev::usb::usb::usbctlprint;
use crate::dev::usb::usbdi::{IPL_USB, USBD_NORMAL_COMPLETION, splusb};
use crate::kern::subr_autoconf::{config_defer, config_found};
use crate::kern::subr_prf::{printf, snprintf};
use crate::machine::bus::{
    BUS_SPACE_BARRIER_READ, BUS_SPACE_BARRIER_WRITE, bus_space_barrier, bus_space_read_4,
    bus_space_unmap, bus_space_write_4,
};
use crate::machine::intr::splx;
use crate::machine::pci_machdep::{
    PciChipsetTag, pci_intr_disestablish, pci_intr_establish, pci_intr_map, pci_intr_string,
};
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};
use crate::sys::errno::Errno;
use libkern::strlcpy;

/// `struct ohci_pci_softc`.
#[repr(C)]
pub struct OhciPciSoftc {
    /// `sc`: the machine-independent softc, first.
    pub sc: OhciSoftc,
    /// `sc_pc`.
    pub sc_pc: Cell<Option<PciChipsetTag>>,
    /// `sc_ih`: interrupt vectoring.
    pub sc_ih: Cell<Option<NonNull<c_void>>>,
}

impl OhciPciSoftc {
    /// `sc->sc_pc`. Panics before the attach set it.
    fn pc(&self) -> PciChipsetTag {
        match self.sc_pc.get() {
            Some(pc) => pc,
            None => crate::kern::subr_prf::panic(format_args!("ohci_pci: no chipset tag")),
        }
    }
}

// SAFETY: `#[repr(C)]` with the `OhciSoftc` (itself headed by the device) first; the other
// members are `Cell`s of an `Option` of a tag and an `Option<NonNull>`: all valid as zero
// bits.
unsafe impl Softc for OhciPciSoftc {}

/// `ohci_pci_ca`.
pub static OHCI_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<OhciPciSoftc>(),
    ca_match: Some(ohci_pci_match),
    ca_attach: ohci_pci_attach,
    ca_detach: Some(ohci_pci_detach),
    ca_activate: Some(ohci_activate),
};

/// `ohci_pci_match`: any serial bus / USB / OHCI function.
pub fn ohci_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args` valid during the match.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    if pci_class(pa.pa_class) == PCI_CLASS_SERIALBUS
        && pci_subclass(pa.pa_class) == PCI_SUBCLASS_SERIALBUS_USB
        && pci_interface(pa.pa_class) == PCI_INTERFACE_OHCI
    {
        return 1;
    }

    0
}

/// `ohci_pci_attach`.
pub fn ohci_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `ohci_pci_ca`, whose softc is an `OhciPciSoftc`; attached
    // devices are not freed while the kernel uses them (detach disestablishes the interrupt
    // and detaches `usb` first).
    let sc: &'static OhciPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<OhciPciSoftc>()) };
    // SAFETY: pci attaches its children with a `pci_attach_args` valid during the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let pc = pa.pa_pc;
    let osc = &sc.sc;
    // SAFETY: the device's name lives as long as the device, and the interrupt is
    // disestablished before the device can go (`ohci_pci_detach`).
    let devname: &'static str = unsafe { &*ptr::from_ref(osc.sc_bus.bdev.xname()) };

    // Map I/O registers
    let Ok((iot, ioh, _, size)) = pci_mapreg_map(pa, PCI_CBMEM, PCI_MAPREG_TYPE_MEM, 0, 0) else {
        printf(format_args!(": can't map mem space\n"));
        return;
    };
    osc.iot.set(Some(iot));
    osc.ioh.set(Some(ioh));
    osc.sc_size.set(size);

    // Record what interrupts were enabled by SMM/BIOS.
    osc.sc_intre
        .set(bus_space_read_4(iot, ioh, OHCI_INTERRUPT_ENABLE));

    // Disable interrupts, so we don't get any spurious ones.
    bus_space_write_4(iot, ioh, OHCI_INTERRUPT_DISABLE, OHCI_MIE);

    sc.sc_pc.set(Some(pc));
    osc.sc_bus.dmatag.set(Some(pa.pa_dmat));

    bus_space_barrier(
        iot,
        ioh,
        0,
        size,
        BUS_SPACE_BARRIER_READ | BUS_SPACE_BARRIER_WRITE,
    );
    bus_space_write_4(iot, ioh, OHCI_INTERRUPT_DISABLE, OHCI_MIE);

    let s = splusb();
    let unmap = || {
        bus_space_unmap(iot, ioh, size);
        osc.sc_size.set(0);
    };
    // Map and establish the interrupt.
    let Some(ih) = pci_intr_map(pa) else {
        printf(format_args!(": couldn't map interrupt\n"));
        unmap();
        splx(s);
        return;
    };

    let intrstr = pci_intr_string(pc, ih);
    let Some(cookie) = pci_intr_establish(
        pc,
        ih,
        IPL_USB,
        ohci_intr,
        ptr::from_ref(osc).cast_mut().cast(),
        devname,
    ) else {
        printf(format_args!(": couldn't establish interrupt"));
        printf(format_args!(" at {intrstr}"));
        printf(format_args!("\n"));
        unmap();
        splx(s);
        return;
    };
    sc.sc_ih.set(Some(cookie));
    printf(format_args!(": {intrstr}, "));

    // Figure out vendor for root hub descriptor.
    let vendor = pci_findvendor(pa.pa_id);
    osc.sc_id_vendor.set(pci_vendor(pa.pa_id) as i32);
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
    osc.sc_vendor.set(buf);

    // Display revision and perform legacy emulation handover.
    if ohci_checkrev(osc) != USBD_NORMAL_COMPLETION || ohci_handover(osc) != USBD_NORMAL_COMPLETION
    {
        unmap();
        // SAFETY: the cookie came from pci_intr_establish above and is dropped here.
        unsafe { pci_intr_disestablish(sc.pc(), cookie) };
        sc.sc_ih.set(None);
        splx(s);
        return;
    }

    // Ignore interrupts for now
    osc.sc_bus.dying.set(1);

    config_defer(self_, ohci_pci_attach_deferred);

    splx(s);
}

/// `ohci_pci_attach_deferred`: initialises the controller and attaches `usb(4)`.
pub fn ohci_pci_attach_deferred(self_: &Device) {
    // SAFETY: as in `ohci_pci_attach`.
    let sc: &'static OhciPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<OhciPciSoftc>()) };
    let osc = &sc.sc;

    let s = splusb();

    osc.sc_bus.dying.set(0);

    let r = ohci_init(osc);
    if r != USBD_NORMAL_COMPLETION {
        printf(format_args!(
            "{}: init failed, error={}\n",
            osc.sc_bus.bdev.xname(),
            r as i32
        ));
        let (iot, ioh) = osc.regs();
        bus_space_unmap(iot, ioh, osc.sc_size.get());
        osc.sc_size.set(0);
        if let Some(ih) = sc.sc_ih.get() {
            // SAFETY: the cookie came from pci_intr_establish and is dropped here.
            unsafe { pci_intr_disestablish(sc.pc(), ih) };
            sc.sc_ih.set(None);
        }
        splx(s);
        return;
    }
    splx(s);

    // Attach usb device.
    config_found(
        self_,
        ptr::from_ref(&osc.sc_bus).cast_mut().cast(),
        Some(usbctlprint),
    );
}

/// `ohci_pci_detach`.
pub fn ohci_pci_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `ohci_pci_ca`.
    let sc = unsafe { self_.softc::<OhciPciSoftc>() };

    ohci_detach(self_, flags)?;

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
