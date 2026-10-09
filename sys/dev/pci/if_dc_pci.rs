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
 * Copyright (c) 1997, 1998, 1999
 *	Bill Paul <wpaul@ee.columbia.edu>.  All rights reserved.
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
 *	This product includes software developed by Bill Paul.
 * 4. Neither the name of the author nor the names of any co-contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY Bill Paul AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL Bill Paul OR THE VOICES IN HIS HEAD
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 *
 * $FreeBSD: src/sys/pci/if_dc.c,v 1.5 2000/01/12 22:24:05 wpaul Exp $
 */
/* </LICENSES> */

/* <CODE> */
//! `dev/pci/if_dc_pci.c`: the PCI front-end of dc(4) (`dc* at pci?`), for the DEC/Intel
//! 21143 and its clones.
//!
//! Upstream: sys/dev/pci/if_dc_pci.c @ 3ce1f3f79392
//!
//! It matches `dc_devs[]` (and the DEC 21140, experimental, and the 21142 revision 0x21,
//! which de(4) would rather have, at priority 1), maps the registers in I/O space
//! (`DC_USEIOSPACE`: every arch but hppa), establishes the interrupt, picks the chip type
//! and its quirks from the PCI id, reads the SROM where the chip has one, decides the media
//! port and calls `dc_attach`. QEMU's `tulip` is a DEC 21142/3 (1011:0019) at revision 0.
//!
//! ## Deviations
//! - `dc_pci_ca`'s softc size is that of `struct dc_pci_softc` (the C gives `sizeof(struct
//!   dc_softc)`, so `psc_pc` and `psc_mapsize` lie past the softc's size: a C bug malloc's
//!   rounding hides).
//! - `dc_devs[]` is a slice without the `{ 0, 0 }` terminator; the match is the pure
//!   [`dc_pci_match_id`].
//! - The softc is `#[repr(C)]` with the `struct dc_softc` first, as in C; `psc_pc` is an
//!   `Option` (all-zero valid).
//! - `__sparc64__` (the OpenFirmware address and `DC_MOMENCO_BOTCH`) and `SRM_MEDIA` (alpha)
//!   are not compiled, as on amd64.
//! - `dc_pci_detach` returns `Result<(), Errno>`; the interrupt is established under the
//!   device's name, which lives as long as the softc.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::ic::dc::{
    dc_activate, dc_attach, dc_detach, dc_eeprom_width, dc_intr, dc_parse_21143_srom, dc_read_srom,
    dc_reset,
};
use crate::dev::ic::dcreg::*;
use crate::dev::pci::pci::pci_set_powerstate;
use crate::dev::pci::pci_map::pci_mapreg_map;
use crate::dev::pci::pcidevs::*;
use crate::dev::pci::pcireg::{
    PCI_MAPREG_TYPE_IO, PCI_PMCSR_STATE_D0, PCI_SUBSYS_ID_REG, pci_product, pci_revision,
    pci_vendor,
};
use crate::dev::pci::pcivar::PciAttachArgs;
use crate::kern::kern_malloc::malloc;
use crate::kern::subr_prf::{Str, panic, printf};
use crate::machine::bus::{BusSize, bus_space_unmap};
use crate::machine::intr::IPL_NET;
use crate::machine::pci_machdep::{
    PciChipsetTag, pci_conf_read, pci_conf_write, pci_intr_disestablish, pci_intr_establish,
    pci_intr_map, pci_intr_string,
};
use crate::netinet::if_ether::ETHER_MAX_DIX_LEN;
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT};

/// `struct dc_pci_softc`.
#[repr(C)]
pub struct DcPciSoftc {
    /// `psc_softc`.
    pub psc_softc: DcSoftc,
    /// `psc_pc`.
    pub psc_pc: Cell<Option<PciChipsetTag>>,
    /// `psc_mapsize`.
    pub psc_mapsize: Cell<BusSize>,
}

// SAFETY: `#[repr(C)]` with the `struct dc_softc` (a `Softc`, its device first) first; the
// other members are `Cell`s of an `Option` and a size, all-zero valid.
unsafe impl Softc for DcPciSoftc {}

/// `{ vendor, product }` of `dc_devs[]`.
const fn ty(vendor: u32, product: u32) -> DcType {
    DcType {
        dc_vid: vendor as u16,
        dc_did: product as u16,
    }
}

/// `dc_devs[]`: various supported device vendors/types.
pub static DC_DEVS: [DcType; 32] = [
    ty(PCI_VENDOR_DEC, PCI_PRODUCT_DEC_21140),
    ty(PCI_VENDOR_DEC, PCI_PRODUCT_DEC_21142),
    ty(PCI_VENDOR_DAVICOM, PCI_PRODUCT_DAVICOM_DM9009),
    ty(PCI_VENDOR_DAVICOM, PCI_PRODUCT_DAVICOM_DM9100),
    ty(PCI_VENDOR_DAVICOM, PCI_PRODUCT_DAVICOM_DM9102),
    ty(PCI_VENDOR_ADMTEK, PCI_PRODUCT_ADMTEK_ADM9511),
    ty(PCI_VENDOR_ADMTEK, PCI_PRODUCT_ADMTEK_ADM9513),
    ty(PCI_VENDOR_ADMTEK, PCI_PRODUCT_ADMTEK_AL981),
    ty(PCI_VENDOR_ADMTEK, PCI_PRODUCT_ADMTEK_AN983),
    ty(PCI_VENDOR_ASIX, PCI_PRODUCT_ASIX_AX88140A),
    ty(PCI_VENDOR_MACRONIX, PCI_PRODUCT_MACRONIX_MX98713),
    ty(PCI_VENDOR_MACRONIX, PCI_PRODUCT_MACRONIX_MX98715),
    ty(PCI_VENDOR_MACRONIX, PCI_PRODUCT_MACRONIX_MX98727),
    ty(PCI_VENDOR_COMPEX, PCI_PRODUCT_COMPEX_98713),
    ty(PCI_VENDOR_LITEON, PCI_PRODUCT_LITEON_PNIC),
    ty(PCI_VENDOR_LITEON, PCI_PRODUCT_LITEON_PNICII),
    ty(PCI_VENDOR_ACCTON, PCI_PRODUCT_ACCTON_EN1217),
    ty(PCI_VENDOR_ACCTON, PCI_PRODUCT_ACCTON_EN2242),
    ty(PCI_VENDOR_CONEXANT, PCI_PRODUCT_CONEXANT_RS7112),
    ty(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_21145),
    ty(PCI_VENDOR_3COM, PCI_PRODUCT_3COM_3CSHO100BTX),
    ty(PCI_VENDOR_MICROSOFT, PCI_PRODUCT_MICROSOFT_MN130),
    ty(PCI_VENDOR_XIRCOM, PCI_PRODUCT_XIRCOM_X3201_3_21143),
    ty(PCI_VENDOR_ADMTEK, PCI_PRODUCT_ADMTEK_AN985),
    ty(PCI_VENDOR_ABOCOM, PCI_PRODUCT_ABOCOM_FE2500),
    ty(PCI_VENDOR_ABOCOM, PCI_PRODUCT_ABOCOM_FE2500MX),
    ty(PCI_VENDOR_ABOCOM, PCI_PRODUCT_ABOCOM_PCM200),
    ty(PCI_VENDOR_DLINK, PCI_PRODUCT_DLINK_DRP32TXD),
    ty(PCI_VENDOR_LINKSYS, PCI_PRODUCT_LINKSYS_PCMPC200),
    ty(PCI_VENDOR_LINKSYS, PCI_PRODUCT_LINKSYS_PCM200),
    ty(PCI_VENDOR_HAWKING, PCI_PRODUCT_HAWKING_PN672TX),
    ty(PCI_VENDOR_MICROSOFT, PCI_PRODUCT_MICROSOFT_MN120),
];

/// `dc_pci_ca`.
pub static DC_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<DcPciSoftc>(),
    ca_match: Some(dc_pci_match),
    ca_attach: dc_pci_attach,
    ca_detach: Some(dc_pci_detach),
    ca_activate: Some(dc_activate),
};

/// The softc of a device made for `dc_pci_ca`.
fn dc_pci_sc(self_: &Device) -> &'static DcPciSoftc {
    // SAFETY: `self_` was made for `dc_pci_ca`, whose softc is a `DcPciSoftc`; softcs are
    // never freed while the device exists, so the softc may be borrowed for 'static.
    unsafe { &*ptr::from_ref(self_.softc::<DcPciSoftc>()) }
}

/// `dc_pci_match`'s decision for a vendor, product and revision: 1 for the DEC 21140
/// (support for it is experimental; by default de(4) takes it) and for the 21142 revision
/// 0x21 (which doesn't seem to work so well with dc, so de(4), matching at 2, handles it),
/// 3 for the chips of `dc_devs[]`, else 0.
pub fn dc_pci_match_id(vendor: u32, product: u32, revision: u32) -> i32 {
    if vendor == PCI_VENDOR_DEC && product == PCI_PRODUCT_DEC_21140 {
        return 1;
    }

    if vendor == PCI_VENDOR_DEC && product == PCI_PRODUCT_DEC_21142 && revision == 0x21 {
        return 1;
    }

    if DC_DEVS
        .iter()
        .any(|t| vendor == u32::from(t.dc_vid) && product == u32::from(t.dc_did))
    {
        return 3;
    }

    0
}

/// `dc_pci_match`: probe for a 21143 or clone chip.
pub fn dc_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    dc_pci_match_id(
        pci_vendor(pa.pa_id),
        pci_product(pa.pa_id),
        pci_revision(pa.pa_class),
    )
}

/// The chip type and quirks of a PCI vendor and product (the switch of `dc_pci_attach`);
/// `false` when it is none dc knows. The PNIC's receive buffer and the SROM reads are the
/// caller's.
fn dc_pci_chip(sc: &DcSoftc, pa: &PciAttachArgs) -> bool {
    let pc = pa.pa_pc;
    let vendor = pci_vendor(pa.pa_id);
    let product = pci_product(pa.pa_id);
    let rev = sc.dc_revision.get();
    let mut found = false;

    match vendor {
        PCI_VENDOR_DEC => {
            if product == PCI_PRODUCT_DEC_21140 || product == PCI_PRODUCT_DEC_21142 {
                found = true;
                sc.dc_type.set(DC_TYPE_21143);
                sc.set_flags(DC_TX_POLL | DC_TX_USE_TX_INTR);
                sc.set_flags(DC_REDUCED_MII_POLL);
                dc_read_srom(sc, sc.dc_romwidth.get());
            }
        }
        PCI_VENDOR_INTEL => {
            if product == PCI_PRODUCT_INTEL_21145 {
                found = true;
                sc.dc_type.set(DC_TYPE_21145);
                sc.set_flags(DC_TX_POLL | DC_TX_USE_TX_INTR);
                sc.set_flags(DC_REDUCED_MII_POLL);
                dc_read_srom(sc, sc.dc_romwidth.get());
            }
        }
        PCI_VENDOR_DAVICOM => {
            if product == PCI_PRODUCT_DAVICOM_DM9100
                || product == PCI_PRODUCT_DAVICOM_DM9102
                || product == PCI_PRODUCT_DAVICOM_DM9009
            {
                found = true;
                sc.dc_type.set(DC_TYPE_DM9102);
                sc.set_flags(DC_TX_COALESCE | DC_TX_INTR_ALWAYS);
                sc.set_flags(DC_REDUCED_MII_POLL | DC_TX_STORENFWD);
                sc.set_flags(DC_TX_ALIGN);
                sc.dc_pmode.set(DC_PMODE_MII);

                // Increase the latency timer value.
                let mut command = pci_conf_read(pc, pa.pa_tag, DC_PCI_CFLT as i32);
                command &= 0xFFFF00FF;
                command |= 0x00008000;
                pci_conf_write(pc, pa.pa_tag, DC_PCI_CFLT as i32, command);
            }
        }
        PCI_VENDOR_ADMTEK | PCI_VENDOR_3COM | PCI_VENDOR_MICROSOFT => {
            if product == PCI_PRODUCT_ADMTEK_AL981 {
                found = true;
                sc.dc_type.set(DC_TYPE_AL981);
                sc.set_flags(DC_TX_USE_TX_INTR);
                sc.set_flags(DC_TX_ADMTEK_WAR);
                sc.dc_pmode.set(DC_PMODE_MII);
                dc_read_srom(sc, sc.dc_romwidth.get());
            }
            if product == PCI_PRODUCT_ADMTEK_ADM9511
                || product == PCI_PRODUCT_ADMTEK_ADM9513
                || product == PCI_PRODUCT_ADMTEK_AN983
                || product == PCI_PRODUCT_3COM_3CSHO100BTX
                || product == PCI_PRODUCT_MICROSOFT_MN130
            {
                found = true;
                sc.dc_type.set(DC_TYPE_AN983);
                sc.set_flags(DC_TX_USE_TX_INTR);
                sc.set_flags(DC_TX_ADMTEK_WAR);
                sc.set_flags(DC_64BIT_HASH);
                sc.dc_pmode.set(DC_PMODE_MII);
                // Don't read SROM for - auto-loaded on reset
            }
        }
        PCI_VENDOR_MACRONIX | PCI_VENDOR_ACCTON => {
            if product == PCI_PRODUCT_ACCTON_EN2242 {
                found = true;
                sc.dc_type.set(DC_TYPE_AN983);
                sc.set_flags(DC_TX_USE_TX_INTR);
                sc.set_flags(DC_TX_ADMTEK_WAR);
                sc.set_flags(DC_64BIT_HASH);
                sc.dc_pmode.set(DC_PMODE_MII);
                // Don't read SROM for - auto-loaded on reset
            }
            if product == PCI_PRODUCT_MACRONIX_MX98713 {
                found = true;
                if rev < DC_REVISION_98713A {
                    sc.dc_type.set(DC_TYPE_98713);
                }
                if rev >= DC_REVISION_98713A {
                    sc.dc_type.set(DC_TYPE_98713A);
                    sc.set_flags(DC_21143_NWAY);
                }
                sc.set_flags(DC_REDUCED_MII_POLL);
                sc.set_flags(DC_TX_POLL | DC_TX_USE_TX_INTR);
            }
            if product == PCI_PRODUCT_MACRONIX_MX98715 || product == PCI_PRODUCT_ACCTON_EN1217 {
                found = true;
                if (DC_REVISION_98715AEC_C..DC_REVISION_98725).contains(&rev) {
                    sc.set_flags(DC_128BIT_HASH);
                }
                sc.dc_type.set(DC_TYPE_987x5);
                sc.set_flags(DC_TX_POLL | DC_TX_USE_TX_INTR);
                sc.set_flags(DC_REDUCED_MII_POLL | DC_21143_NWAY);
            }
            if product == PCI_PRODUCT_MACRONIX_MX98727 {
                found = true;
                sc.dc_type.set(DC_TYPE_987x5);
                sc.set_flags(DC_TX_POLL | DC_TX_USE_TX_INTR);
                sc.set_flags(DC_REDUCED_MII_POLL | DC_21143_NWAY);
            }
        }
        PCI_VENDOR_COMPEX => {
            if product == PCI_PRODUCT_COMPEX_98713 {
                found = true;
                if rev < DC_REVISION_98713A {
                    sc.dc_type.set(DC_TYPE_98713);
                    sc.set_flags(DC_REDUCED_MII_POLL);
                }
                if rev >= DC_REVISION_98713A {
                    sc.dc_type.set(DC_TYPE_98713A);
                }
                sc.set_flags(DC_TX_POLL | DC_TX_USE_TX_INTR);
            }
        }
        PCI_VENDOR_LITEON => {
            if product == PCI_PRODUCT_LITEON_PNICII {
                found = true;
                sc.dc_type.set(DC_TYPE_PNICII);
                sc.set_flags(DC_TX_POLL | DC_TX_USE_TX_INTR);
                sc.set_flags(DC_REDUCED_MII_POLL | DC_21143_NWAY);
                sc.set_flags(DC_128BIT_HASH);
            }
            if product == PCI_PRODUCT_LITEON_PNIC {
                found = true;
                sc.dc_type.set(DC_TYPE_PNIC);
                sc.set_flags(DC_TX_STORENFWD | DC_TX_INTR_ALWAYS);
                sc.set_flags(DC_PNIC_RX_BUG_WAR);
                let Some(buf) = malloc(ETHER_MAX_DIX_LEN * 5, M_DEVBUF, M_NOWAIT) else {
                    panic(format_args!("dc_pci_attach"));
                };
                sc.dc_pnic_rx_buf.set(buf.as_ptr());
                if rev < DC_REVISION_82C169 {
                    sc.dc_pmode.set(DC_PMODE_SYM);
                }
            }
        }
        PCI_VENDOR_ASIX => {
            if product == PCI_PRODUCT_ASIX_AX88140A {
                found = true;
                sc.dc_type.set(DC_TYPE_ASIX);
                sc.set_flags(DC_TX_USE_TX_INTR | DC_TX_INTR_FIRSTFRAG);
                sc.set_flags(DC_REDUCED_MII_POLL);
                sc.dc_pmode.set(DC_PMODE_MII);
            }
        }
        PCI_VENDOR_CONEXANT => {
            if product == PCI_PRODUCT_CONEXANT_RS7112 {
                found = true;
                sc.dc_type.set(DC_TYPE_CONEXANT);
                sc.set_flags(DC_TX_INTR_ALWAYS);
                sc.set_flags(DC_REDUCED_MII_POLL);
                sc.dc_pmode.set(DC_PMODE_MII);
                dc_read_srom(sc, sc.dc_romwidth.get());
            }
        }
        PCI_VENDOR_XIRCOM if product == PCI_PRODUCT_XIRCOM_X3201_3_21143 => {
            found = true;
            sc.dc_type.set(DC_TYPE_XIRCOM);
            sc.set_flags(DC_TX_INTR_ALWAYS);
            sc.set_flags(DC_TX_COALESCE);
            sc.set_flags(DC_TX_ALIGN);
            sc.dc_pmode.set(DC_PMODE_MII);
        }
        _ => {}
    }

    found
}

/// `dc_pci_attach`: attach the interface. Allocate softc structures, do ifmedia setup and
/// ethernet/BPF attach.
pub fn dc_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let psc = dc_pci_sc(self_);
    let sc: &'static DcSoftc = &psc.psc_softc;
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let pc = pa.pa_pc;

    psc.psc_pc.set(Some(pa.pa_pc));
    sc.sc_dmat.set(Some(pa.pa_dmat));

    pci_set_powerstate(pa.pa_pc, pa.pa_tag, PCI_PMCSR_STATE_D0 as i32);

    sc.dc_csid
        .set(pci_conf_read(pc, pa.pa_tag, PCI_SUBSYS_ID_REG));

    // Map control/status registers (DC_USEIOSPACE).
    let Ok((t, h, _base, size)) = pci_mapreg_map(pa, DC_PCI_CFBIO as i32, PCI_MAPREG_TYPE_IO, 0, 0)
    else {
        printf(format_args!(": can't map i/o space\n"));
        return;
    };
    sc.dc_btag.set(Some(t));
    sc.dc_bhandle.set(Some(h));
    psc.psc_mapsize.set(size);
    let fail_1 = || bus_space_unmap(t, h, size);

    // Allocate interrupt
    let Some(ih) = pci_intr_map(pa) else {
        printf(format_args!(": couldn't map interrupt\n"));
        fail_1();
        return;
    };
    let intrstr = pci_intr_string(pc, ih);
    let Some(cookie) = pci_intr_establish(
        pc,
        ih,
        IPL_NET,
        dc_intr,
        ptr::from_ref(sc).cast_mut().cast(),
        sc.devname(),
    ) else {
        printf(format_args!(
            ": couldn't establish interrupt at {}\n",
            Str(intrstr.as_bytes())
        ));
        fail_1();
        return;
    };
    sc.sc_ih.set(cookie.as_ptr());
    printf(format_args!(": {}", Str(intrstr.as_bytes())));

    // Need this info to decide on a chip type.
    sc.dc_revision.set(pci_revision(pa.pa_class));

    // Get the eeprom width, if possible
    let vendor = pci_vendor(pa.pa_id);
    let product = pci_product(pa.pa_id);
    if vendor == PCI_VENDOR_LITEON && product == PCI_PRODUCT_LITEON_PNIC {
        // PNIC has non-standard eeprom
    } else if vendor == PCI_VENDOR_XIRCOM && product == PCI_PRODUCT_XIRCOM_X3201_3_21143 {
        // XIRCOM has non-standard eeprom
    } else {
        dc_eeprom_width(sc);
    }

    if !dc_pci_chip(sc, pa) {
        // This shouldn't happen if probe has done its job...
        printf(format_args!(
            ": unknown device: {:x}:{:x}\n",
            vendor, product
        ));
        // fail_2:
        sc.sc_ih.set(ptr::null_mut());
        // SAFETY: the handler established above; its pointer is gone from the softc.
        unsafe { pci_intr_disestablish(pc, cookie) };
        fail_1();
        return;
    }

    // Save the cache line size.
    if sc.dc_is_davicom() {
        sc.dc_cachesize.set(0);
    } else {
        sc.dc_cachesize
            .set((pci_conf_read(pc, pa.pa_tag, DC_PCI_CFLT as i32) & 0xFF) as u8);
    }

    // Reset the adapter.
    dc_reset(sc);

    // Take 21143 out of snooze mode
    if sc.dc_is_intel() || sc.dc_is_xircom() {
        let mut command = pci_conf_read(pc, pa.pa_tag, DC_PCI_CFDD as i32);
        command &= !(DC_CFDD_SNOOZE_MODE | DC_CFDD_SLEEP_MODE);
        pci_conf_write(pc, pa.pa_tag, DC_PCI_CFDD as i32, command);
    }

    // If we discover later (in dc_attach) that we have an MII with no PHY, we need to have
    // the 21143 drive the LEDs. Except there are some systems like the NEC VersaPro
    // NoteBook PC which have no LEDs, and twiddling these bits has adverse effects on them.
    // (I.e. you suddenly can't get a link.)
    //
    // If mii_attach() returns an error, we leave the DC_TULIP_LEDS bit set, else we clear
    // it. Since our dc(4) driver is split into bus-dependent and bus-independent parts, we
    // must do set this bit here while we are able to do PCI configuration reads.
    if sc.dc_is_intel() && pci_conf_read(pc, pa.pa_tag, DC_PCI_CSID as i32) != 0x80281033 {
        sc.set_flags(DC_TULIP_LEDS);
    }

    // Try to learn something about the supported media. We know that ASIX and ADMtek and
    // Davicom devices will *always* be using MII media, so that's a no-brainer. The tricky
    // ones are the Macronix/PNIC II and the Intel 21143.
    if sc.dc_is_intel() {
        dc_parse_21143_srom(sc);
    } else if sc.dc_is_macronix() || sc.dc_is_pnicii() {
        if sc.dc_type.get() == DC_TYPE_98713 {
            sc.dc_pmode.set(DC_PMODE_MII);
        } else {
            sc.dc_pmode.set(DC_PMODE_SYM);
        }
    } else if sc.dc_pmode.get() == 0 {
        sc.dc_pmode.set(DC_PMODE_MII);
    }

    dc_attach(sc);
}

/// `dc_pci_detach`.
pub fn dc_pci_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let psc = dc_pci_sc(self_);
    let sc: &'static DcSoftc = &psc.psc_softc;

    if let (Some(cookie), Some(pc)) = (NonNull::new(sc.sc_ih.get()), psc.psc_pc.get()) {
        sc.sc_ih.set(ptr::null_mut());
        // SAFETY: the handler dc_pci_attach established; its pointer is gone from the softc.
        unsafe { pci_intr_disestablish(pc, cookie) };
    }
    dc_detach(sc)?;
    let (t, h) = sc.regs();
    bus_space_unmap(t, h, psc.psc_mapsize.get());

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_priorities() {
        // QEMU's tulip: a 21142/3 at revision 0.
        assert_eq!(dc_pci_match_id(PCI_VENDOR_DEC, PCI_PRODUCT_DEC_21142, 0), 3);
        // The revision de(4) keeps, and the experimental 21140.
        assert_eq!(
            dc_pci_match_id(PCI_VENDOR_DEC, PCI_PRODUCT_DEC_21142, 0x21),
            1
        );
        assert_eq!(dc_pci_match_id(PCI_VENDOR_DEC, PCI_PRODUCT_DEC_21140, 0), 1);
        assert_eq!(
            dc_pci_match_id(PCI_VENDOR_MICROSOFT, PCI_PRODUCT_MICROSOFT_MN120, 0),
            3
        );
        // Not a tulip.
        assert_eq!(dc_pci_match_id(PCI_VENDOR_INTEL, 0x100e, 3), 0);
    }
}
/* </TESTS> */
