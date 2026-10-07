/*	$OpenBSD: if_re_pci.c,v 1.59 2024/08/31 16:23:09 deraadt Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2005 Peter Valchev <pvalchev@openbsd.org>
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

//! `dev/pci/if_re_pci.c`: the PCI front-end of re(4) (`re* at pci?`): PCI/CardBus front-end
//! for the Realtek 8169 and the 8139C+.
//!
//! Upstream: sys/dev/pci/if_re_pci.c @ 3ce1f3f79392
//!
//! It matches `re_pci_devices[]`, the Linksys EG1032 by its subsystem id, and the RT8139 at
//! PCI revision 0x20 (an 8139C+; rl(4) takes it only at revision 0x10). QEMU's `rtl8139` is
//! such an 8139C+.
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the `struct rl_softc` first (its device first), as in C;
//!   `sc_pc` is an `Option` (all-zero valid).
//! - `SMALL_KERNEL` is not defined: the PMCSR write for wake on LAN is here.
//! - `re_pci_detach` and `re_pci_activate` return `Result<(), Errno>`.
//! - The interrupt is established under the device's name, which lives as long as the softc.
//! - When `re_attach` fails, `sc_ih` is cleared as the interrupt is disestablished (the C
//!   leaves the stale pointer).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::dev::ic::re::{re_attach, re_detach, re_init, re_intr, re_stop};
use crate::dev::ic::rtl81x9reg::{
    RL_CFG2, RL_CFG2_MSI, RL_EE_MODE, RL_EECMD, RL_EEMODE_OFF, RL_FLAG_MSI, RL_FLAG_PCIE,
    RL_PCI_LOIO, RL_PCI_LOMEM, RL_PCI_LOMEM64, RL_PCI_PMCSR, RL_PME_EN, RlSoftc,
};
use crate::dev::pci::pci::{pci_get_capability, pci_matchbyid, pci_set_powerstate};
use crate::dev::pci::pci_map::pci_mapreg_map;
use crate::dev::pci::pcidevs::*;
use crate::dev::pci::pcireg::{
    PCI_CAP_PCIEXPRESS, PCI_MAPREG_MEM_TYPE_32BIT, PCI_MAPREG_MEM_TYPE_64BIT, PCI_MAPREG_TYPE_IO,
    PCI_MAPREG_TYPE_MEM, PCI_PCIE_LCSR, PCI_PCIE_LCSR_ASPM_L0S, PCI_PCIE_LCSR_ASPM_L1,
    PCI_PCIE_LCSR_ECPM, PCI_PMCSR_STATE_D0, PCI_SUBSYS_ID_REG, PciProductId, PciVendorId,
    pci_product, pci_revision, pci_vendor,
};
use crate::dev::pci::pcivar::{PciAttachArgs, PciMatchid};
use crate::kern::subr_prf::{Str, printf};
use crate::machine::bus::{BusSize, bus_space_unmap};
use crate::machine::intr::{IPL_MPSAFE, IPL_NET};
use crate::machine::pci_machdep::{
    PciChipsetTag, Pcitag, pci_conf_read, pci_conf_write, pci_intr_disestablish,
    pci_intr_establish, pci_intr_map, pci_intr_map_msi, pci_intr_string,
};
use crate::net::if_::{IFF_RUNNING, IFF_UP};
use crate::sys::device::{CfMatch, Cfattach, DVACT_RESUME, DVACT_SUSPEND, Device, Softc};
use crate::sys::errno::Errno;

/// `RE_LINKSYS_EG1032_SUBID`.
pub const RE_LINKSYS_EG1032_SUBID: u32 = 0x00241737;

/// `struct re_pci_softc`.
#[repr(C)]
pub struct RePciSoftc {
    /// `sc_rl`: general.
    pub sc_rl: RlSoftc,

    /// `sc_pc`: PCI-specific data.
    pub sc_pc: Cell<Option<PciChipsetTag>>,
    /// `sc_pcitag`.
    pub sc_pcitag: Cell<Pcitag>,

    /// `sc_iosize`.
    pub sc_iosize: Cell<BusSize>,
}

// SAFETY: `#[repr(C)]` with the `struct rl_softc` (a `Softc`, its device first) first; the
// other members are `Cell`s of an `Option`, a tag and a size, all-zero valid.
unsafe impl Softc for RePciSoftc {}

/// `{ vendor, product }` of `re_pci_devices[]`.
const fn id(vendor: u32, product: u32) -> PciMatchid {
    PciMatchid {
        pm_vid: vendor as PciVendorId,
        pm_pid: product as PciProductId,
    }
}

/// `re_pci_devices[]`.
pub static RE_PCI_DEVICES: [PciMatchid; 12] = [
    id(PCI_VENDOR_COREGA, PCI_PRODUCT_COREGA_CGLAPCIGT),
    id(PCI_VENDOR_DLINK, PCI_PRODUCT_DLINK_DGE528T),
    id(PCI_VENDOR_DLINK, PCI_PRODUCT_DLINK_DGE530T_C1),
    id(PCI_VENDOR_REALTEK, PCI_PRODUCT_REALTEK_E2500V2),
    id(PCI_VENDOR_REALTEK, PCI_PRODUCT_REALTEK_E2600),
    id(PCI_VENDOR_REALTEK, PCI_PRODUCT_REALTEK_RT8101E),
    id(PCI_VENDOR_REALTEK, PCI_PRODUCT_REALTEK_RT8168),
    id(PCI_VENDOR_REALTEK, PCI_PRODUCT_REALTEK_RT8168_2),
    id(PCI_VENDOR_REALTEK, PCI_PRODUCT_REALTEK_RT8169),
    id(PCI_VENDOR_REALTEK, PCI_PRODUCT_REALTEK_RT8169SC),
    id(PCI_VENDOR_TTTECH, PCI_PRODUCT_TTTECH_MC322),
    id(PCI_VENDOR_USR2, PCI_PRODUCT_USR2_USR997902),
];

/// `re_pci_ca`: PCI autoconfig definitions.
pub static RE_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<RePciSoftc>(),
    ca_match: Some(re_pci_probe),
    ca_attach: re_pci_attach,
    ca_detach: Some(re_pci_detach),
    ca_activate: Some(re_pci_activate),
};

/// The softc of a device made for `re_pci_ca`.
fn re_pci_sc(self_: &Device) -> &'static RePciSoftc {
    // SAFETY: `self_` was made for `re_pci_ca`, whose softc is a `RePciSoftc`; softcs are
    // never freed while the device exists, so the softc may be borrowed for 'static.
    unsafe { &*ptr::from_ref(self_.softc::<RePciSoftc>()) }
}

/// Whether a function with id `pa_id`, class `pa_class` and subsystem id `subid` is one
/// `re_pci_probe` takes beside `re_pci_devices[]`: the C+ mode 8139 (revision 0x20) and the
/// Linksys EG1032 with its own subsystem id.
pub fn re_pci_probe_quirk(pa_id: u32, pa_class: u32, subid: u32) -> bool {
    // C+ mode 8139's
    if pci_vendor(pa_id) == PCI_VENDOR_REALTEK
        && pci_product(pa_id) == PCI_PRODUCT_REALTEK_RT8139
        && pci_revision(pa_class) == 0x20
    {
        return true;
    }

    pci_vendor(pa_id) == PCI_VENDOR_LINKSYS
        && pci_product(pa_id) == PCI_PRODUCT_LINKSYS_EG1032
        && subid == RE_LINKSYS_EG1032_SUBID
}

/// `re_pci_probe`: probe for a Realtek 8169/8110 chip. Check the PCI vendor and device IDs
/// against our list and return a device name if we find a match.
pub fn re_pci_probe(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let pc = pa.pa_pc;

    let subid = pci_conf_read(pc, pa.pa_tag, PCI_SUBSYS_ID_REG);

    if re_pci_probe_quirk(pa.pa_id, pa.pa_class, subid) {
        return 1;
    }

    pci_matchbyid(pa, &RE_PCI_DEVICES)
}

/// `re_pci_attach`: PCI-specific attach routine.
pub fn re_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let psc = re_pci_sc(self_);
    let sc: &'static RlSoftc = &psc.sc_rl;
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let pc = pa.pa_pc;

    pci_set_powerstate(pa.pa_pc, pa.pa_tag, PCI_PMCSR_STATE_D0 as i32);

    // SMALL_KERNEL is not defined: enable power management for wake on lan.
    pci_conf_write(pc, pa.pa_tag, RL_PCI_PMCSR as i32, RL_PME_EN);

    // Map control/status registers.
    let mapped = pci_mapreg_map(
        pa,
        RL_PCI_LOMEM64 as i32,
        PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_64BIT,
        0,
        0,
    )
    .or_else(|_| {
        pci_mapreg_map(
            pa,
            RL_PCI_LOMEM as i32,
            PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_32BIT,
            0,
            0,
        )
    })
    .or_else(|_| pci_mapreg_map(pa, RL_PCI_LOIO as i32, PCI_MAPREG_TYPE_IO, 0, 0));
    let Ok((t, h, _base, iosize)) = mapped else {
        printf(format_args!(": can't map mem or i/o space\n"));
        return;
    };
    sc.rl_btag.set(Some(t));
    sc.rl_bhandle.set(Some(h));
    psc.sc_iosize.set(iosize);

    // Allocate interrupt
    let ih = match pci_intr_map_msi(pa) {
        Some(ih) => {
            sc.set_flags(RL_FLAG_MSI);
            ih
        }
        None => {
            let Some(ih) = pci_intr_map(pa) else {
                printf(format_args!(": couldn't map interrupt\n"));
                return;
            };
            ih
        }
    };
    let intrstr = pci_intr_string(pc, ih);
    let Some(cookie) = pci_intr_establish(
        pc,
        ih,
        IPL_NET | IPL_MPSAFE,
        re_intr,
        ptr::from_ref(sc).cast_mut().cast(),
        sc.devname(),
    ) else {
        printf(format_args!(
            ": couldn't establish interrupt at {}",
            Str(intrstr.as_bytes())
        ));
        return;
    };
    sc.sc_ih.set(cookie.as_ptr());

    sc.sc_dmat.set(Some(pa.pa_dmat));
    psc.sc_pc.set(Some(pc));
    psc.sc_pcitag.set(pa.pa_tag);

    // PCI Express check.
    if let Some((offset, _)) = pci_get_capability(pc, pa.pa_tag, PCI_CAP_PCIEXPRESS) {
        // Disable PCIe ASPM and ECPM.
        let mut reg = pci_conf_read(pc, pa.pa_tag, offset + PCI_PCIE_LCSR);
        reg &= !(PCI_PCIE_LCSR_ASPM_L0S | PCI_PCIE_LCSR_ASPM_L1 | PCI_PCIE_LCSR_ECPM);
        pci_conf_write(pc, pa.pa_tag, offset + PCI_PCIE_LCSR, reg);
        sc.set_flags(RL_FLAG_PCIE);
    }

    if !(pci_vendor(pa.pa_id) == PCI_VENDOR_REALTEK
        && pci_product(pa.pa_id) == PCI_PRODUCT_REALTEK_RT8139)
    {
        sc.csr_write_1(RL_EECMD, RL_EE_MODE);
        let mut cfg = sc.csr_read_1(RL_CFG2);
        if sc.flags() & RL_FLAG_MSI != 0 {
            cfg |= RL_CFG2_MSI;
            sc.csr_write_1(RL_CFG2, cfg);
        } else if cfg & RL_CFG2_MSI != 0 {
            cfg &= !RL_CFG2_MSI;
            sc.csr_write_1(RL_CFG2, cfg);
        }
        sc.csr_write_1(RL_EECMD, RL_EEMODE_OFF);
    }

    sc.sc_product.set(pci_product(pa.pa_id) as u16);

    // Call bus-independent attach routine
    if re_attach(sc, intrstr.as_bytes()).is_err() {
        sc.sc_ih.set(ptr::null_mut());
        // SAFETY: the handler established above; its pointer is gone from the softc.
        unsafe { pci_intr_disestablish(pc, cookie) };
        bus_space_unmap(t, h, iosize);
    }
}

/// `re_pci_detach`.
pub fn re_pci_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let psc = re_pci_sc(self_);
    let sc: &'static RlSoftc = &psc.sc_rl;

    re_detach(sc);

    // Disable interrupts
    if let (Some(cookie), Some(pc)) = (ptr::NonNull::new(sc.sc_ih.get()), psc.sc_pc.get()) {
        sc.sc_ih.set(ptr::null_mut());
        // SAFETY: the handler re_pci_attach established; its pointer is gone from the
        // softc, and re_detach stopped the interface.
        unsafe { pci_intr_disestablish(pc, cookie) };
    }

    // Free pci resources
    let (t, h) = sc.regs();
    bus_space_unmap(t, h, psc.sc_iosize.get());

    Ok(())
}

/// `re_pci_activate`.
pub fn re_pci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    let psc = re_pci_sc(self_);
    let ifp = &psc.sc_rl.sc_arpcom.ac_if;

    match act {
        DVACT_SUSPEND if ifp.if_flags.get() & IFF_RUNNING != 0 => re_stop(ifp),
        DVACT_RESUME if ifp.if_flags.get() & IFF_UP != 0 => {
            let _ = re_init(ifp);
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_8139cplus_is_taken_at_revision_0x20_only() {
        let id = (PCI_PRODUCT_REALTEK_RT8139 << 16) | PCI_VENDOR_REALTEK;
        assert!(re_pci_probe_quirk(id, 0x0200_0020, 0));
        assert!(!re_pci_probe_quirk(id, 0x0200_0010, 0));
        let eg = (PCI_PRODUCT_LINKSYS_EG1032 << 16) | PCI_VENDOR_LINKSYS;
        assert!(re_pci_probe_quirk(eg, 0, RE_LINKSYS_EG1032_SUBID));
        assert!(!re_pci_probe_quirk(eg, 0, 0));
    }
}
