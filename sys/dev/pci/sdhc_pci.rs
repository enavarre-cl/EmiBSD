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
 * Copyright (c) 2006 Uwe Stuehler <uwe@openbsd.org>
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
//! `sdhc* at pci?`: the PCI front-end of sdhc(4): matches the SD host controller class
//! (and two Ricoh parts), applies the vendor fixups (TI, ENE, Ricoh, Genesys Logic GL9755),
//! establishes the interrupt (MSI, else INTx) and maps one standard register set per slot,
//! each handed to `sdhc_host_found`.
//!
//! Upstream: sys/dev/pci/sdhc_pci.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The softc begins with the `struct sdhc_softc`, as in C; `sc_pc` is an `Option` (set by
//!   attach), `sc_ih` the interrupt handle or null.
//! - The GL9755's `cd-inverted` and `wp-inverted` come from the device tree where there is
//!   one (`__HAVE_FDT`: `PCITAG_NODE` is 0 without it, as in ppb.rs).
//! - The interrupt is established with the device's name (`DEVNAME(sc)`), which lives as
//!   long as the softc.
//! - No stubs: every function of the file is ported.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::dev::ofw::fdt::OF_getpropbool;
use crate::dev::pci::pci_map::{pci_mapreg_map, pci_mapreg_probe};
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_ENE_SDCARD, PCI_PRODUCT_GENESYS_GL9755, PCI_PRODUCT_REALTEK_RTS5209,
    PCI_PRODUCT_RICOH_R5U822, PCI_PRODUCT_RICOH_R5U823, PCI_PRODUCT_TI_PCI7XX1_FLASH,
    PCI_PRODUCT_TI_PCI7XX1_SD, PCI_VENDOR_ENE, PCI_VENDOR_GENESYS, PCI_VENDOR_REALTEK,
    PCI_VENDOR_RICOH, PCI_VENDOR_TI,
};
use crate::dev::pci::pcireg::{
    PCI_CLASS_SYSTEM, PCI_ID_REG, PCI_MAPREG_MEM_TYPE_64BIT, PCI_MAPREG_TYPE_IO,
    PCI_SUBCLASS_SYSTEM_SDHC, pci_class, pci_interface, pci_product, pci_subclass, pci_vendor,
};
use crate::dev::pci::pcivar::{PciAttachArgs, Pcireg};
use crate::dev::sdmmc::sdhc::{sdhc_activate, sdhc_host_found, sdhc_intr, sdhc_write_2};
use crate::dev::sdmmc::sdhcreg::{
    SDHC_64BIT_DMA_SUPP, SDHC_CLOCK_CTL, SDHC_DDR50_SUPP, SDHC_PCI_BAR_END, SDHC_PCI_BAR_START,
    SDHC_PCI_INTERFACE_DMA,
};
use crate::dev::sdmmc::sdhcvar::{SDHC_F_32BIT_ACCESS, SDHC_F_NOPWR0, SdhcSoftc};
use crate::dev::sdmmc::sdmmcvar::IPL_SDMMC;
use crate::kern::kern_malloc::mallocarray;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::cpu::delay;
use crate::machine::pci_machdep::{
    PciChipsetTag, Pcitag, pci_conf_read, pci_conf_write, pci_intr_establish, pci_intr_map,
    pci_intr_map_msi, pci_intr_string, pci_make_tag, pcitag_node,
};
use crate::sys::device::{CfMatch, Cfattach, DVACT_RESUME, DVACT_SUSPEND, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK};

/// `SDHC_PCI_CONF_SLOT_INFO`: 8-bit PCI configuration register that tells us how many slots
/// there are and which BAR entry corresponds to the first slot.
pub const SDHC_PCI_CONF_SLOT_INFO: i32 = 0x40;

/* TI specific register */

/// `SDHC_PCI_GENERAL_CTL`.
pub const SDHC_PCI_GENERAL_CTL: i32 = 0x4c;
/// `MMC_SD_DIS`.
pub const MMC_SD_DIS: Pcireg = 0x02;

/* RICOH specific registers */

/// `SDHC_PCI_MODE_KEY`.
pub const SDHC_PCI_MODE_KEY: i32 = 0xf9;
/// `SDHC_PCI_MODE`.
pub const SDHC_PCI_MODE: i32 = 0x150;
/// `SDHC_PCI_MODE_SD20`.
pub const SDHC_PCI_MODE_SD20: u8 = 0x10;
/// `SDHC_PCI_BASE_FREQ_KEY`.
pub const SDHC_PCI_BASE_FREQ_KEY: i32 = 0xfc;
/// `SDHC_PCI_BASE_FREQ`.
pub const SDHC_PCI_BASE_FREQ: i32 = 0xe1;

/* Genesys Logic GL9755 */

/// `GL9755_PECONF`.
pub const GL9755_PECONF: i32 = 0x044;
/// `GL9755_PECONF_LFCLK`.
pub const GL9755_PECONF_LFCLK: Pcireg = 0x7 << 12;
/// `GL9755_PECONF_DMACLK`.
pub const GL9755_PECONF_DMACLK: Pcireg = 1 << 29;
/// `GL9755_PECONF_INVERT_CD`.
pub const GL9755_PECONF_INVERT_CD: Pcireg = 1 << 30;
/// `GL9755_PECONF_INVERT_WP`.
pub const GL9755_PECONF_INVERT_WP: Pcireg = 1 << 31;
/// `GL9755_PLL`.
pub const GL9755_PLL: i32 = 0x064;
/// `GL9755_PLL_LDIV_MASK`.
pub const GL9755_PLL_LDIV_MASK: Pcireg = 0x3ff;
/// `GL9755_PLL_LDIV_SHIFT`.
pub const GL9755_PLL_LDIV_SHIFT: u32 = 0;
/// `GL9755_PLL_PDIV_MASK`.
pub const GL9755_PLL_PDIV_MASK: Pcireg = 0x7 << 12;
/// `GL9755_PLL_PDIV_SHIFT`.
pub const GL9755_PLL_PDIV_SHIFT: u32 = 12;
/// `GL9755_PLL_DIR`.
pub const GL9755_PLL_DIR: Pcireg = 1 << 15;
/// `GL9755_PLL_SSC_STEP_MASK`.
pub const GL9755_PLL_SSC_STEP_MASK: Pcireg = 0x1f << 24;
/// `GL9755_PLL_SSC_STEP_SHIFT`.
pub const GL9755_PLL_SSC_STEP_SHIFT: u32 = 24;
/// `GL9755_PLL_SSC_EN`.
pub const GL9755_PLL_SSC_EN: Pcireg = 1 << 31;
/// `GL9755_PLLSSC`.
pub const GL9755_PLLSSC: i32 = 0x068;
/// `GL9755_PLLSSC_PPM_MASK`.
pub const GL9755_PLLSSC_PPM_MASK: Pcireg = 0xffff;
/// `GL9755_PLLSSC_PPM_SHIFT`.
pub const GL9755_PLLSSC_PPM_SHIFT: u32 = 0;
/// `GL9755_SERDES`.
pub const GL9755_SERDES: i32 = 0x070;
/// `GL9755_SERDES_SCP_DIS`.
pub const GL9755_SERDES_SCP_DIS: Pcireg = 1 << 19;
/// `GL9755_MISC`.
pub const GL9755_MISC: i32 = 0x078;
/// `GL9755_MISC_SSC_OFF`.
pub const GL9755_MISC_SSC_OFF: Pcireg = 1 << 26;
/// `GL9755_WT`.
pub const GL9755_WT: i32 = 0x800;
/// `GL9755_WT_EN`.
pub const GL9755_WT_EN: Pcireg = 1 << 0;

/// `struct sdhc_pci_softc`.
#[repr(C)]
pub struct SdhcPciSoftc {
    /// `sc`.
    pub sc: SdhcSoftc,
    /// `sc_pc`.
    pub sc_pc: Cell<Option<PciChipsetTag>>,
    /// `sc_tag`.
    pub sc_tag: Cell<Pcitag>,
    /// `sc_id`.
    pub sc_id: Cell<Pcireg>,
    /// `sc_ih`: the interrupt handle, or null.
    pub sc_ih: Cell<*mut c_void>,
    /// `sc_capmask`.
    pub sc_capmask: Cell<u32>,
    /// `sc_capmask2`.
    pub sc_capmask2: Cell<u32>,
}

impl SdhcPciSoftc {
    /// `sc->sc_pc`, which attach sets first.
    fn pc(&self) -> PciChipsetTag {
        match self.sc_pc.get() {
            Some(pc) => pc,
            None => panic(format_args!("sdhc_pci: no chipset tag")),
        }
    }
}

// SAFETY: `#[repr(C)]` with the sdhc softc (itself headed by the device, all-zero valid)
// first, then `Cell`s of an `Option` of the chipset tag, the PCI tag and id (integers), a
// raw pointer and integers, valid as zero bits.
unsafe impl Softc for SdhcPciSoftc {}

/// `sdhc_pci_ca`.
pub static SDHC_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<SdhcPciSoftc>(),
    ca_match: Some(sdhc_pci_match),
    ca_attach: sdhc_pci_attach,
    ca_detach: None,
    ca_activate: Some(sdhc_pci_activate),
};

/// `SDHC_PCI_NUM_SLOTS(info)`.
pub const fn sdhc_pci_num_slots(info: Pcireg) -> i32 {
    (((info >> 4) & 0x7) + 1) as i32
}

/// `SDHC_PCI_FIRST_BAR(info)`.
pub const fn sdhc_pci_first_bar(info: Pcireg) -> i32 {
    (info & 0x7) as i32
}

/// `(struct sdhc_pci_softc *)self` for a device made for `sdhc_pci_ca`.
fn sdhc_pci_softc(self_: &Device) -> &'static SdhcPciSoftc {
    // SAFETY: only devices made for `sdhc_pci_ca` reach the attachment's functions, and
    // their softc is an `SdhcPciSoftc`; softcs are never freed while the device exists.
    unsafe { &*ptr::from_ref(self_.softc::<SdhcPciSoftc>()) }
}

/// `sdhc_pci_match`.
pub fn sdhc_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    sdhc_pci_match_ids(pa.pa_id, pa.pa_class)
}

/// The test of `sdhc_pci_match` on the function's ID and class registers.
pub fn sdhc_pci_match_ids(id: Pcireg, class: Pcireg) -> i32 {
    // The Realtek RTS5209 is supported by rtsx(4). Usually the device class for these is
    // UNDEFINED but there are RTS5209 devices which are advertising an SYSTEM/SDHC device
    // class in addition to a separate device advertising the UNDEFINED class. Such devices
    // are not compatible with sdhc(4), so ignore them.
    if pci_vendor(id) == PCI_VENDOR_REALTEK && pci_product(id) == PCI_PRODUCT_REALTEK_RTS5209 {
        return 0;
    }

    if pci_class(class) == PCI_CLASS_SYSTEM && pci_subclass(class) == PCI_SUBCLASS_SYSTEM_SDHC {
        return 1;
    }

    if pci_vendor(id) == PCI_VENDOR_RICOH
        && (pci_product(id) == PCI_PRODUCT_RICOH_R5U822
            || pci_product(id) == PCI_PRODUCT_RICOH_R5U823)
    {
        return 1;
    }

    0
}

/// Whether the function is one of the Ricoh controllers that need `sdhc_ricohfix`.
fn sdhc_pci_is_ricoh(id: Pcireg) -> bool {
    pci_vendor(id) == PCI_VENDOR_RICOH
        && (pci_product(id) == PCI_PRODUCT_RICOH_R5U822
            || pci_product(id) == PCI_PRODUCT_RICOH_R5U823)
}

/// `sdhc_pci_attach`.
pub fn sdhc_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = sdhc_pci_softc(self_);
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    sc.sc_pc.set(Some(pa.pa_pc));
    sc.sc_tag.set(pa.pa_tag);
    sc.sc_id.set(pa.pa_id);

    // Some TI controllers needs special treatment.
    if pci_vendor(pa.pa_id) == PCI_VENDOR_TI
        && pci_product(pa.pa_id) == PCI_PRODUCT_TI_PCI7XX1_SD
        && pa.pa_function == 4
    {
        sdhc_takecontroller(pa);
    }

    // ENE controllers break if set to 0V bus power.
    if pci_vendor(pa.pa_id) == PCI_VENDOR_ENE && pci_product(pa.pa_id) == PCI_PRODUCT_ENE_SDCARD {
        sc.sc.sc_flags.set(sc.sc.sc_flags.get() | SDHC_F_NOPWR0);
    }

    // Some RICOH controllers need to be bumped into the right mode.
    if sdhc_pci_is_ricoh(pa.pa_id) {
        sdhc_ricohfix(sc);
    }

    // Genesys Logic controllers need special handling.
    if pci_vendor(pa.pa_id) == PCI_VENDOR_GENESYS
        && pci_product(pa.pa_id) == PCI_PRODUCT_GENESYS_GL9755
    {
        sdhc_gl9755_init(sc);
    }

    let Some(ih) = pci_intr_map_msi(pa).or_else(|| pci_intr_map(pa)) else {
        printf(format_args!(": can't map interrupt\n"));
        return;
    };

    let intrstr = pci_intr_string(pa.pa_pc, ih);
    let Some(cookie) = pci_intr_establish(
        pa.pa_pc,
        ih,
        IPL_SDMMC,
        sdhc_intr,
        ptr::from_ref(&sc.sc).cast_mut().cast(),
        sc.sc.sc_dev.xname(),
    ) else {
        printf(format_args!(": can't establish interrupt\n"));
        return;
    };
    sc.sc_ih.set(cookie.as_ptr());
    printf(format_args!(": {}\n", intrstr));

    // Enable use of DMA if supported by the interface.
    let usedma = pci_interface(pa.pa_class) == SDHC_PCI_INTERFACE_DMA;
    sc.sc.sc_dmat.set(Some(pa.pa_dmat));

    // Map and attach all hosts supported by the host controller.
    let slotinfo = pci_conf_read(pa.pa_pc, pa.pa_tag, SDHC_PCI_CONF_SLOT_INFO);
    let mut nslots = sdhc_pci_num_slots(slotinfo);

    // Allocate an array big enough to hold all the possible hosts
    let Some(hosts) = mallocarray(
        nslots as usize,
        size_of::<*mut c_void>(),
        M_DEVBUF,
        M_WAITOK,
    ) else {
        panic(format_args!("sdhc_pci_attach: M_WAITOK allocation failed"));
    };
    sc.sc.sc_host.set(hosts.cast().as_ptr());

    let mut reg = SDHC_PCI_BAR_START + sdhc_pci_first_bar(slotinfo) * 4;
    while reg < SDHC_PCI_BAR_END && nslots > 0 {
        let Some(type_) = pci_mapreg_probe(pa.pa_pc, pa.pa_tag, reg) else {
            break;
        };

        let mapped = if type_ == PCI_MAPREG_TYPE_IO {
            None
        } else {
            pci_mapreg_map(pa, reg, type_, 0, 0).ok()
        };
        let Some((iot, ioh, _base, size)) = mapped else {
            printf(format_args!(
                "{} at 0x{:x}: can't map registers\n",
                sc.sc.sc_dev.xname(),
                reg
            ));
            break;
        };

        let capmask = (u64::from(sc.sc_capmask2.get()) << 32) | u64::from(sc.sc_capmask.get());
        if sdhc_host_found(&sc.sc, iot, ioh, size, usedma, capmask, 0).is_err() {
            printf(format_args!(
                "{} at 0x{:x}: can't initialize host\n",
                sc.sc.sc_dev.xname(),
                reg
            ));
        }

        if type_ & PCI_MAPREG_MEM_TYPE_64BIT != 0 {
            reg += 4;
        }
        reg += 4;
        nslots -= 1;
    }
}

/// `sdhc_pci_activate`.
pub fn sdhc_pci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    let sc = sdhc_pci_softc(self_);

    match act {
        DVACT_SUSPEND => sdhc_activate(self_, act),
        DVACT_RESUME => {
            // Some RICOH controllers need to be bumped into the right mode.
            if sdhc_pci_is_ricoh(sc.sc_id.get()) {
                sdhc_ricohfix(sc);
            }
            sdhc_activate(self_, act)
        }
        _ => sdhc_activate(self_, act),
    }
}

/// `sdhc_takecontroller`: disables MMC/SD on the TI flash media controller (function 3) so
/// the SD host takes over.
pub fn sdhc_takecontroller(pa: &PciAttachArgs) {
    // Look at func 3 for the flash device
    let tag = pci_make_tag(pa.pa_pc, pa.pa_bus as i32, pa.pa_device as i32, 3);
    let id = pci_conf_read(pa.pa_pc, tag, PCI_ID_REG);
    if pci_product(id) != PCI_PRODUCT_TI_PCI7XX1_FLASH {
        return;
    }

    // Disable MMC/SD on the flash media controller so the SD host takes over.
    let mut reg = pci_conf_read(pa.pa_pc, tag, SDHC_PCI_GENERAL_CTL);
    reg |= MMC_SD_DIS;
    pci_conf_write(pa.pa_pc, tag, SDHC_PCI_GENERAL_CTL, reg);
}

/// `sdhc_ricohfix`: enables SD 2.0 mode and lowers the base clock to 50 MHz.
pub fn sdhc_ricohfix(sc: &SdhcPciSoftc) {
    let (pc, tag) = (sc.pc(), sc.sc_tag.get());

    // Enable SD2.0 mode.
    sdhc_pci_conf_write(pc, tag, SDHC_PCI_MODE_KEY, 0xfc);
    sdhc_pci_conf_write(pc, tag, SDHC_PCI_MODE, SDHC_PCI_MODE_SD20);
    sdhc_pci_conf_write(pc, tag, SDHC_PCI_MODE_KEY, 0x00);

    // Some SD/MMC cards don't work with the default base clock frequency of 200MHz. Lower
    // it to 50Hz.
    sdhc_pci_conf_write(pc, tag, SDHC_PCI_BASE_FREQ_KEY, 0x01);
    sdhc_pci_conf_write(pc, tag, SDHC_PCI_BASE_FREQ, 50);
    sdhc_pci_conf_write(pc, tag, SDHC_PCI_BASE_FREQ_KEY, 0x00);
}

/// `sdhc_pci_conf_write`: writes one byte of configuration space (read-modify-write of its
/// dword).
pub fn sdhc_pci_conf_write(pc: PciChipsetTag, tag: Pcitag, reg: i32, val: u8) {
    let tmp = pci_conf_read(pc, tag, reg & !0x3);
    pci_conf_write(pc, tag, reg & !0x3, sdhc_pci_conf_merge(tmp, reg, val));
}

/// The dword `sdhc_pci_conf_write` writes back: `tmp` with byte `reg & 3` replaced by
/// `val`.
pub const fn sdhc_pci_conf_merge(tmp: Pcireg, reg: i32, val: u8) -> Pcireg {
    let shift = ((reg & 0x3) * 8) as u32;
    (tmp & !(0xff << shift)) | ((val as Pcireg) << shift)
}

/// `sdhc_gl9755_wt_enable`.
pub fn sdhc_gl9755_wt_enable(sc: &SdhcPciSoftc) {
    let (pc, tag) = (sc.pc(), sc.sc_tag.get());
    let mut reg = pci_conf_read(pc, tag, GL9755_WT);
    reg |= GL9755_WT_EN;
    pci_conf_write(pc, tag, GL9755_WT, reg);
}

/// `sdhc_gl9755_wt_disable`.
pub fn sdhc_gl9755_wt_disable(sc: &SdhcPciSoftc) {
    let (pc, tag) = (sc.pc(), sc.sc_tag.get());
    let mut reg = pci_conf_read(pc, tag, GL9755_WT);
    reg &= !GL9755_WT_EN;
    pci_conf_write(pc, tag, GL9755_WT, reg);
}

/// `sdhc_gl9755_bus_clock_pre`: reprograms the PLL (and spread spectrum) for `freq`.
pub fn sdhc_gl9755_bus_clock_pre(ssc: &SdhcSoftc, freq: i32, _timing: i32) {
    // SAFETY: only `sdhc_gl9755_init` installs this hook, on an `sdhc_pci_softc`, which is
    // `#[repr(C)]` with its `sdhc_softc` first.
    let sc = unsafe { &*ptr::from_ref(ssc).cast::<SdhcPciSoftc>() };
    let (pc, tag) = (sc.pc(), sc.sc_tag.get());

    sdhc_gl9755_wt_enable(sc);

    // Disable SSC.
    let mut pll = pci_conf_read(pc, tag, GL9755_PLL);
    pll &= !(GL9755_PLL_DIR | GL9755_PLL_SSC_EN);
    pci_conf_write(pc, tag, GL9755_PLL, pll);

    sdhc_gl9755_wt_disable(sc);

    let (step, ppm, ldiv, pdiv, dir): (Pcireg, Pcireg, Pcireg, Pcireg, bool) = match freq {
        208000 => (0xf, 0x5a1d, 0x246, 0x0, true),
        100000 => (0xe, 0x51ec, 0x244, 0x1, true),
        50000 => (0xe, 0x51ec, 0x244, 0x3, true),
        _ => return,
    };

    // Disable the clock here before we start changing the PLL. It will be disabled again
    // by our caller, but that should be a no-op.
    if let Some(hp) = sc.sc.host(0) {
        sdhc_write_2(hp, SDHC_CLOCK_CTL, 0);
    }

    sdhc_gl9755_wt_enable(sc);

    // Set SSC.
    let misc = pci_conf_read(pc, tag, GL9755_MISC);
    let enable = misc & GL9755_MISC_SSC_OFF == 0;
    let mut pll = pci_conf_read(pc, tag, GL9755_PLL);
    let mut pllssc = pci_conf_read(pc, tag, GL9755_PLLSSC);
    pll &= !(GL9755_PLL_SSC_STEP_MASK | GL9755_PLL_SSC_EN);
    pll |= step << GL9755_PLL_SSC_STEP_SHIFT;
    pll |= if enable { GL9755_PLL_SSC_EN } else { 0 };
    pllssc &= !GL9755_PLLSSC_PPM_MASK;
    pllssc |= ppm << GL9755_PLLSSC_PPM_SHIFT;
    pci_conf_write(pc, tag, GL9755_PLLSSC, pllssc);
    pci_conf_write(pc, tag, GL9755_PLL, pll);

    // Set PLL.
    let mut pll = pci_conf_read(pc, tag, GL9755_PLL);
    pll &= !(GL9755_PLL_LDIV_MASK | GL9755_PLL_PDIV_MASK);
    pll &= !GL9755_PLL_DIR;
    pll |= ldiv << GL9755_PLL_LDIV_SHIFT;
    pll |= pdiv << GL9755_PLL_PDIV_SHIFT;
    pll |= if dir { GL9755_PLL_DIR } else { 0 };
    pci_conf_write(pc, tag, GL9755_PLL, pll);

    sdhc_gl9755_wt_disable(sc);

    delay(1000);
}

/// `sdhc_gl9755_init`.
pub fn sdhc_gl9755_init(sc: &SdhcPciSoftc) {
    let (pc, tag) = (sc.pc(), sc.sc_tag.get());

    sdhc_gl9755_wt_enable(sc);

    let mut reg = pci_conf_read(pc, tag, GL9755_PECONF);
    reg &= !GL9755_PECONF_LFCLK;
    reg &= !GL9755_PECONF_DMACLK;
    // __HAVE_FDT: PCITAG_NODE is 0 where there is no device tree.
    let node = pcitag_node(tag);
    if node != 0 {
        if OF_getpropbool(node, b"cd-inverted") {
            reg |= GL9755_PECONF_INVERT_CD;
        }
        if OF_getpropbool(node, b"wp-inverted") {
            reg |= GL9755_PECONF_INVERT_WP;
        }
    }
    pci_conf_write(pc, tag, GL9755_PECONF, reg);

    // Enable short circuit protection.
    let mut reg = pci_conf_read(pc, tag, GL9755_SERDES);
    reg &= !GL9755_SERDES_SCP_DIS;
    pci_conf_write(pc, tag, GL9755_SERDES, reg);

    sdhc_gl9755_wt_disable(sc);

    sc.sc.sc_bus_clock_pre.set(Some(sdhc_gl9755_bus_clock_pre));

    // We need to use 32-bit register access on Apple Silicon. This shouldn't hurt on other
    // platforms.
    sc.sc
        .sc_flags
        .set(sc.sc.sc_flags.get() | SDHC_F_32BIT_ACCESS);

    // V3-compatible 64-bit DMA isn't supported.
    sc.sc_capmask.set(sc.sc_capmask.get() | SDHC_64BIT_DMA_SUPP);

    // DDR50 is apparently broked on this controller.
    sc.sc_capmask2.set(sc.sc_capmask2.get() | SDHC_DDR50_SUPP);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::pci::pcidevs::PCI_VENDOR_QUMRANET;

    #[test]
    fn qemus_sdhci_pci_matches() {
        // QEMU's sdhci-pci: Red Hat 1b36:0007, class 08 05 01 (system, SDHC, DMA).
        let id = (0x0007 << 16) | 0x1b36;
        let class = (0x08 << 24) | (0x05 << 16) | (0x01 << 8);
        assert_eq!(sdhc_pci_match_ids(id, class), 1);
        assert_eq!(pci_interface(class), SDHC_PCI_INTERFACE_DMA);
        // The RTS5209 is refused even with the SDHC class.
        let rts = (PCI_PRODUCT_REALTEK_RTS5209 << 16) | PCI_VENDOR_REALTEK;
        assert_eq!(sdhc_pci_match_ids(rts, class), 0);
        // A Ricoh R5U822 matches whatever its class; a virtio disk does not.
        let ricoh = (PCI_PRODUCT_RICOH_R5U822 << 16) | PCI_VENDOR_RICOH;
        assert_eq!(sdhc_pci_match_ids(ricoh, 0), 1);
        assert_eq!(
            sdhc_pci_match_ids((0x1001 << 16) | PCI_VENDOR_QUMRANET, 0x0100_0000),
            0
        );
    }

    #[test]
    fn slot_info_and_byte_writes() {
        // One slot, first BAR 0 (QEMU); 6 slots from BAR 2.
        assert_eq!((sdhc_pci_num_slots(0x00), sdhc_pci_first_bar(0x00)), (1, 0));
        assert_eq!((sdhc_pci_num_slots(0x52), sdhc_pci_first_bar(0x52)), (6, 2));
        assert_eq!(sdhc_pci_conf_merge(0x1122_3344, 0xf9, 0xfc), 0x1122_fc44);
        assert_eq!(sdhc_pci_conf_merge(0x1122_3344, 0x150, 0x10), 0x1122_3310);
        assert_eq!(sdhc_pci_conf_merge(0x1122_3344, 0xe1 + 2, 50), 0x3222_3344);
    }
}
/* </TESTS> */
