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
 * Copyright (c) 1996, 1998, 1999
 *	Christopher G. Demetriou.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `puc*`: the PCI "universal" communication card driver, which glues `com(4)` and `lpt(4)`
//! ports to PCI through the bridge chips that are often larger than the devices behind them
//! (`puc* at pci?`, `com* at puc?`).
//!
//! Upstream: sys/dev/pci/puc.c @ 3ce1f3f79392
//!
//! The card is found by `puc_find_description` in `PUC_DEVS` (`pucdata.rs`); the attachment maps
//! each BAR, maps the interrupt once and then hands every port, with the BAR's registers
//! sub-regioned to it, to whatever driver matches (`com_puc.rs`).
//!
//! ## Deviations
//! - `lpt(4)` is not ported, so a card's parallel ports print `lpt at puc0 port N not
//!   configured`, which is what `config_found_sm` does for a port nobody claims.
//! - The `#if NCOM > 0` branch (re-using the console's mapping when the console UART is on this
//!   card) is always present: `com(4)` is always configured.
//! - `puc_pci_intr_establish` returns, behind an Exar XR17V35x card, `real_intrhand` as the
//!   cookie, as the C does (the C casts the function pointer to `void *`; here it is the
//!   function's address as a non-null pointer): its only use is the "not NULL means
//!   established" test in the port driver.
//! - The interrupt is established under the name of the port's device (`name`), which lives as
//!   long as the port's softc; `pci_intr_establish` wants it `'static`.
//! - `puc_pci_attach`'s softc is borrowed for `'static` (softcs are never freed while their
//!   device is attached), so the interrupt argument can be the card's softc.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::ic::com::{COMCONSADDR, comconsioh, comconsiot};
use crate::dev::ic::comreg::{COM_NPORTS, UART_EXAR_INT0};
use crate::dev::pci::pci_map::{pci_mapreg_info, pci_mapreg_map, pci_mapreg_probe};
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_EXAR_XR17V352, PCI_PRODUCT_EXAR_XR17V354, PCI_VENDOR_EXAR,
};
use crate::dev::pci::pcireg::{
    PCI_BHLC_REG, PCI_MAPREG_MEM_TYPE_64BIT, PCI_MAPREG_START, PCI_SUBSYS_ID_REG, pci_hdrtype_type,
    pci_product, pci_vendor,
};
use crate::dev::pci::pcivar::PciAttachArgs;
use crate::dev::pci::pucdata::PUC_DEVS;
use crate::dev::pci::pucvar::{
    PUC_MAX_PORTS, PUC_NBARS, PucAttachArgs, PucDeviceDescription, PucSoftc, puc_is_com,
    puc_is_lpt, puc_port_bar_index,
};
use crate::kern::subr_autoconf::{config_detach, config_found_sm};
use crate::kern::subr_prf::{Str, printf};
use crate::machine::bus::{bus_space_read_1, bus_space_subregion, bus_space_unmap};
use crate::machine::pci_machdep::{
    PciChipsetTag, PciIntrFn, PciIntrHandle, PciIntrStr, pci_conf_read, pci_intr_disestablish,
    pci_intr_establish, pci_intr_map, pci_intr_string,
};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc, UNCONF};
use crate::sys::errno::Errno;

/// `struct puc_pci_softc`.
#[repr(C)]
pub struct PucPciSoftc {
    /// `sc_psc`.
    pub sc_psc: PucSoftc,
    /// `pc`.
    pub pc: Cell<Option<PciChipsetTag>>,
    /// `ih`.
    pub ih: Cell<Option<PciIntrHandle>>,
}

// SAFETY: `#[repr(C)]` with the puc softc (itself headed by the device, all-zero valid) first,
// then two `Cell`s of `Option`s of the chipset tag and the interrupt handle, valid as zero bits
// (`None`).
unsafe impl Softc for PucPciSoftc {}

/// `puc_pci_ca`.
pub static PUC_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PucPciSoftc>(),
    ca_match: Some(puc_pci_match),
    ca_attach: puc_pci_attach,
    ca_detach: Some(puc_pci_detach),
    ca_activate: None,
};

/// `puc_cd`.
pub static PUC_CD: Cfdriver = Cfdriver::new(b"puc", DV_DULL, 0);

/// `puc_pci_match`: a header type 0 function `puc_find_description` knows.
pub fn puc_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    let bhlc = pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_BHLC_REG);
    if pci_hdrtype_type(bhlc) != 0 {
        return 0;
    }

    let subsys = pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_SUBSYS_ID_REG);

    let desc = puc_find_description(
        pci_vendor(pa.pa_id) as u16,
        pci_product(pa.pa_id) as u16,
        pci_vendor(subsys) as u16,
        pci_product(subsys) as u16,
    );
    i32::from(desc.is_some())
}

/// `puc_pci_intr_string`.
pub fn puc_pci_intr_string(paa: &PucAttachArgs) -> PciIntrStr {
    // SAFETY: `puc_pci_attach` makes `paa.puc` the card's softc, alive while its ports are.
    let sc = unsafe { &*paa.puc.cast::<PucPciSoftc>() };

    match (sc.pc.get(), sc.ih.get()) {
        (Some(pc), Some(ih)) => pci_intr_string(pc, ih),
        // Not reached: the ports are configured after `pci_intr_map` filled both.
        _ => PciIntrStr::new(format_args!("")),
    }
}

/// `puc_pci_intr_establish`.
pub fn puc_pci_intr_establish(
    paa: &PucAttachArgs,
    type_: i32,
    func: PciIntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> Option<NonNull<c_void>> {
    // SAFETY: as in `puc_pci_intr_string`.
    let sc: &'static PucPciSoftc = unsafe { &*paa.puc.cast::<PucPciSoftc>() };
    let psc = &sc.sc_psc;
    let port = &psc.sc_ports[paa.port as usize];
    let (pc, ih) = (sc.pc.get()?, sc.ih.get()?);

    if psc.sc_xr17v35x.get() {
        port.real_intrhand.set(Some(func));
        port.real_intrhand_arg.set(arg);
        if paa.port == 0 {
            let cookie = pci_intr_establish(
                pc,
                ih,
                type_,
                puc_pci_xr17v35x_intr,
                ptr::from_ref(sc).cast_mut().cast(),
                name,
            );
            port.intrhand
                .set(cookie.map_or(ptr::null_mut(), NonNull::as_ptr));
        }
        return NonNull::new(func as usize as *mut c_void);
    }

    let cookie = pci_intr_establish(pc, ih, type_, func, arg, name);
    port.intrhand
        .set(cookie.map_or(ptr::null_mut(), NonNull::as_ptr));

    cookie
}

/// `puc_pci_attach`: map the card's BARs and interrupt, then configure its ports.
pub fn puc_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `puc_pci_ca`, whose softc is a `PucPciSoftc`; softcs are
    // never freed while the device exists, so the softc may be borrowed for 'static.
    let psc: &'static PucPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<PucPciSoftc>()) };
    let sc = &psc.sc_psc;
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    let subsys = pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_SUBSYS_ID_REG);
    sc.sc_desc.set(puc_find_description(
        pci_vendor(pa.pa_id) as u16,
        pci_product(pa.pa_id) as u16,
        pci_vendor(subsys) as u16,
        pci_product(subsys) as u16,
    ));

    if pci_vendor(pa.pa_id) == PCI_VENDOR_EXAR
        && (pci_product(pa.pa_id) == PCI_PRODUCT_EXAR_XR17V352
            || pci_product(pa.pa_id) == PCI_PRODUCT_EXAR_XR17V354)
    {
        sc.sc_xr17v35x.set(true);
    }

    // puc_pci_match found the description.
    let Some(desc) = sc.sc_desc.get() else {
        return;
    };
    puc_print_ports(desc);

    let mut i = 0;
    while i < PUC_NBARS {
        let bm = &sc.sc_bar_mappings[i];

        bm.mapped.set(false);
        let bar = PCI_MAPREG_START + 4 * i as i32;
        let Some(type_) = pci_mapreg_probe(pa.pa_pc, pa.pa_tag, bar) else {
            i += 1;
            continue;
        };

        let mapped = pci_mapreg_map(pa, bar, type_, 0, 0);
        bm.mapped.set(mapped.is_ok());
        if let Ok((t, h, a, s)) = mapped {
            bm.t.set(Some(t));
            bm.h.set(Some(h));
            bm.a.set(a);
            bm.s.set(s);
            if type_ == PCI_MAPREG_MEM_TYPE_64BIT {
                i += 1;
            }
            i += 1;
            continue;
        }

        // If a port on this card is used as serial console, mapping the associated BAR will
        // fail because the bus space is already mapped.  In that case, we try to re-use the
        // already existing mapping.  Unfortunately this means that if a BAR is used to
        // support multiple ports, only the first port will work.
        if let Ok((a, _, _)) = pci_mapreg_info(pa.pa_pc, pa.pa_tag, bar, type_) {
            bm.a.set(a);
            if Some(pa.pa_iot) == comconsiot()
                && a == COMCONSADDR.load(core::sync::atomic::Ordering::Relaxed)
            {
                bm.t.set(comconsiot());
                bm.h.set(comconsioh());
                bm.s.set(COM_NPORTS);
                bm.mapped.set(true);
                if type_ == PCI_MAPREG_MEM_TYPE_64BIT {
                    i += 1;
                }
                i += 1;
                continue;
            }
        }

        printf(format_args!(
            "{}: couldn't map BAR at offset 0x{:x}\n",
            sc.sc_dev.xname(),
            bar
        ));
        i += 1;
    }

    // Map interrupt.
    psc.pc.set(Some(pa.pa_pc));
    let Some(ih) = pci_intr_map(pa) else {
        printf(format_args!(
            "{}: couldn't map interrupt\n",
            sc.sc_dev.xname()
        ));
        return;
    };
    psc.ih.set(Some(ih));

    let mut paa = PucAttachArgs {
        port: 0,
        type_: 0,
        puc: ptr::from_ref(psc).cast(),
        a: 0,
        t: None,
        h: None,
        intr_string: puc_pci_intr_string,
        intr_establish: puc_pci_intr_establish,
    };

    puc_common_attach(sc, &mut paa);
}

/// `puc_common_attach`: configure each port of the card.
pub fn puc_common_attach(sc: &PucSoftc, paa: &mut PucAttachArgs) {
    let Some(desc) = sc.sc_desc.get() else {
        return;
    };

    // Configure each port.
    for i in 0..PUC_MAX_PORTS {
        let port = &desc.ports[i];

        if port.type_ == 0 {
            // neither com or lpt
            continue;
        }
        // make sure the base address register is mapped
        let bar = puc_port_bar_index(i32::from(port.bar));
        let bm = &sc.sc_bar_mappings[bar];
        if !bm.mapped.get() {
            printf(format_args!(
                "{}: {} port uses unmapped BAR (0x{:x})\n",
                sc.sc_dev.xname(),
                puc_port_type_name(i32::from(port.type_)).unwrap_or("(null)"),
                port.bar
            ));
            continue;
        }

        // set up to configure the child device
        paa.port = i as i32;
        paa.a = bm.a.get();
        paa.t = bm.t.get();

        paa.type_ = i32::from(port.type_);

        let (Some(t), Some(h)) = (bm.t.get(), bm.h.get()) else {
            continue;
        };
        let offset = usize::from(port.offset);
        let sub = if offset >= bm.s.get() {
            None
        } else {
            bus_space_subregion(t, h, offset, bm.s.get() - offset).ok()
        };
        let Some(sub) = sub else {
            printf(format_args!(
                "{}: couldn't get subregion for port {}\n",
                sc.sc_dev.xname(),
                i
            ));
            continue;
        };
        paa.h = Some(sub);

        // and configure it
        sc.sc_ports[i].dev.set(config_found_sm(
            &sc.sc_dev,
            ptr::from_mut(paa).cast(),
            Some(puc_print),
            Some(puc_submatch),
        ));
    }
}

/// `puc_pci_detach`: detach the ports, release their interrupts and unmap the BARs.
pub fn puc_pci_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `puc_pci_ca`; softcs are never freed while the device
    // exists.
    let sc: &'static PucPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<PucPciSoftc>()) };
    let psc = &sc.sc_psc;

    for i in (0..PUC_MAX_PORTS).rev() {
        let port = &psc.sc_ports[i];

        if let (Some(pc), Some(ih)) = (sc.pc.get(), NonNull::new(port.intrhand.get())) {
            // SAFETY: the cookie `puc_pci_intr_establish` stored for this port, dropped here.
            unsafe { pci_intr_disestablish(pc, ih) };
        }
        if let Some(dev) = port.dev.get() {
            // SAFETY: the port's device is attached (it is `sc_ports[i].dev`) and not used
            // after the detach.
            unsafe { config_detach(dev, flags)? };
        }
    }

    for bm in psc.sc_bar_mappings.iter().rev() {
        if let (true, Some(t), Some(h)) = (bm.mapped.get(), bm.t.get(), bm.h.get()) {
            bus_space_unmap(t, h, bm.s.get());
        }
    }

    Ok(())
}

/// `puc_print`.
pub fn puc_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: `puc_common_attach` hands its children a `puc_attach_args`.
    let paa = unsafe { &*aux.cast::<PucAttachArgs>() };

    if let Some(pnp) = pnp {
        printf(format_args!(
            "{} at {}",
            puc_port_type_name(paa.type_).unwrap_or("(null)"),
            Str(pnp)
        ));
    }
    printf(format_args!(" port {}", paa.port));
    UNCONF
}

/// `puc_submatch`: checks the `port` locator before the driver's match.
pub fn puc_submatch(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();
    // SAFETY: as in `puc_print`.
    let aa = unsafe { &*aux.cast::<PucAttachArgs>() };

    // `puc*`'s one locator, `port` (`files.pci`: `device puc {[port = -1]}`).
    let port = cf.cf_loc.first().map_or(-1, |&l| l as i32);
    if port != -1 && port != aa.port {
        return 0;
    }
    match cf.cf_attach.ca_match {
        Some(ca_match) => ca_match(parent, match_, aux),
        None => 0,
    }
}

/// `puc_find_description`: the card with these vendor, product, subsystem vendor and subsystem
/// product IDs.
pub fn puc_find_description(
    vend: u16,
    prod: u16,
    svend: u16,
    sprod: u16,
) -> Option<&'static PucDeviceDescription> {
    PUC_DEVS.iter().find(|d| {
        (vend & d.rmask[0]) == d.rval[0]
            && (prod & d.rmask[1]) == d.rval[1]
            && (svend & d.rmask[2]) == d.rval[2]
            && (sprod & d.rmask[3]) == d.rval[3]
    })
}

/// `puc_port_type_name`.
pub fn puc_port_type_name(type_: i32) -> Option<&'static str> {
    if puc_is_com(type_) {
        return Some("com");
    }
    if puc_is_lpt(type_) {
        return Some("lpt");
    }
    None
}

/// `puc_print_ports`: the `: ports: 2 com, 1 lpt` of the attach line.
pub fn puc_print_ports(desc: &PucDeviceDescription) {
    let (mut ncom, mut nlpt) = (0, 0);

    printf(format_args!(": ports: "));
    for port in &desc.ports {
        if puc_is_com(i32::from(port.type_)) {
            ncom += 1;
        } else if puc_is_lpt(i32::from(port.type_)) {
            nlpt += 1;
        }
    }
    if ncom != 0 {
        printf(format_args!("{} com", ncom));
    }
    if nlpt != 0 {
        if ncom != 0 {
            printf(format_args!(", "));
        }
        printf(format_args!("{} lpt", nlpt));
    }
    printf(format_args!("\n"));
}

/// `puc_pci_xr17v35x_intr`: the shared interrupt of an Exar XR17V35x card, which calls the
/// handler of each port that has one pending.
pub fn puc_pci_xr17v35x_intr(arg: *mut c_void) -> i32 {
    // SAFETY: the argument `puc_pci_intr_establish` gave `pci_intr_establish`: the card's
    // softc, alive while the interrupt is established.
    let sc = unsafe { &*arg.cast::<PucPciSoftc>() };
    let psc = &sc.sc_psc;

    let bm = &psc.sc_bar_mappings[0];
    let (Some(t), Some(h)) = (bm.t.get(), bm.h.get()) else {
        return 1;
    };
    let ports = bus_space_read_1(t, h, UART_EXAR_INT0);

    for i in 0..8 {
        if let (true, Some(real)) = (ports & (1 << i) != 0, psc.sc_ports[i].real_intrhand.get()) {
            real(psc.sc_ports[i].real_intrhand_arg.get());
        }
    }

    1
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::pci::pcidevs::{PCI_PRODUCT_REDHAT_SERIAL4, PCI_VENDOR_REDHAT};
    use crate::dev::pci::pucvar::PUC_PORT_COM;

    #[test]
    fn finds_qemus_pci_serial_cards() {
        // 1b36:0002, 0003, 0004: one, two and four 16550s in one I/O BAR, 8 bytes apart.
        let one = puc_find_description(0x1b36, 0x0002, 0x1af4, 0x1100).expect("pci-serial");
        assert_eq!(one.ports.iter().filter(|p| p.type_ != 0).count(), 1);
        assert_eq!(
            (one.ports[0].type_, one.ports[0].bar, one.ports[0].offset),
            (PUC_PORT_COM as u8, 0x10, 0)
        );

        let four = puc_find_description(
            PCI_VENDOR_REDHAT as u16,
            PCI_PRODUCT_REDHAT_SERIAL4 as u16,
            0,
            0,
        )
        .expect("pci-serial-4x");
        let offsets: [u16; 4] = core::array::from_fn(|i| four.ports[i].offset);
        assert_eq!(offsets, [0, 8, 0x10, 0x18]);
        assert_eq!(four.ports.iter().filter(|p| p.type_ != 0).count(), 4);
    }

    #[test]
    fn masks_leave_out_the_subsystem_ids_that_are_zero() {
        // Red Hat's entries ignore the subsystem, MosChip's MCS9865 matches it exactly.
        assert!(puc_find_description(0x1b36, 0x0002, 0xdead, 0xbeef).is_some());
        assert!(puc_find_description(0x1b36, 0x0001, 0, 0).is_none());
        assert!(puc_find_description(0x5372, 0x6873, 0x1000, 0x0004).is_some());
        assert!(puc_find_description(0x5372, 0x6873, 0x1000, 0x0003).is_none());
    }

    #[test]
    fn port_type_names() {
        assert_eq!(puc_port_type_name(PUC_PORT_COM), Some("com"));
        assert_eq!(puc_port_type_name(1), Some("lpt"));
    }
}
/* </TESTS> */
