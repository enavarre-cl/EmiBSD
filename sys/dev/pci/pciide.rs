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
 * Copyright (c) 1999, 2000, 2001 Manuel Bouyer.
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
 *
 */

/*
 * Copyright (c) 1996, 1998 Christopher G. Demetriou.  All rights reserved.
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
//! PCI IDE controller driver, pciide(4): the PCI front-end of the IDE channel core
//! (`wdc.c`), with the chip-specific code of every controller of its table.
//!
//! Upstream: sys/dev/pci/pciide.c @ 3ce1f3f79392
//!
//! Author: Christopher G. Demetriou, March 2, 1998 (derived from NetBSD
//! sys/dev/pci/ppb.c, revision 1.16).
//!
//! See "PCI IDE Controller Specification, Revision 1.0 3/4/94" and "Programming Interface
//! for Bus Master IDE Controller, Revision 1.0 5/16/94" from the PCI SIG.
//!
//! `pciide_attach` looks the controller up in the vendor/product table and calls its
//! `chip_map`, which maps the bus master DMA registers, sets the controller's capabilities
//! and mode hook, maps each channel (compatibility mode: the legacy ports and ISA IRQ 14/15
//! through `machine::pciide_machdep`; native mode: the BARs and the PCI interrupt) and
//! attaches it with `wdcattach`. The `*_setup_channel` hooks program the chip's timing
//! registers for the modes `wdc.c` chose; `pciide_dma_init`/`start`/`finish` drive the bus
//! master DMA engine with a per-drive descriptor table.
//!
//! ## Deviations
//! - Every chip's `chip_map` gets the softc as `&'static PciideSoftc` (softcs are never
//!   freed while their device exists), so channels can be handed to `wdcattach`. The hooks
//!   `wdc.c` calls with a `ChannelSoftc` find their `PciideChannel` by address among the
//!   softc's channels ([`pciide_cp`]); the softc is recovered from the `WdcSoftc` only after
//!   checking that the device is a pciide one ([`pciide_sc`]).
//! - Functions returning a C truth value (`pciide_mapregs_compat`, `pciide_mapregs_native`,
//!   `pciide_chansetup`, `pciide_chan_candisable`) return `bool`; `hw_ok` stays an `int`.
//! - `struct pciide_vendor_desc`'s product pointer and count are one slice.
//! - Comparisons of `sc_pp->chip_map` with a chip's function (`pciide_activate`) use
//!   `core::ptr::fn_addr_eq`.
//! - `WDCDEBUG_PRINT` is `wdcdebug_print!` on `wdcdebug_pciide_mask` (feature `wdcdebug`);
//!   `DIAGNOSTIC` is feature `diagnostic`.
//! - `pciide_attach`'s debug line prints the chipset tag's address and the PCI tag as its
//!   (bus, device, function): the machine's tag types are not integers everywhere.
//! - `BUS_DMA_RAW` is not defined by any EmiBSD machine: 0, as the C's fallback.
//! - The `gcsc_chip_map` row (`__i386__` only) of the National table is not compiled, as
//!   on amd64; nor are the `__sparc64__` reset of `cmd0643_9_setup_channel` and the code
//!   under the options no kernel defines (`PCIIDE_AMD756_ENABLEDMA`,
//!   `PCIIDE_CMD0646U_ENABLEUDMA`, `PCIIDE_I31244_DISABLEDMA`) or `#if 0`.
//! - A chip's private structure (`sc_cookie`: `PciideSatalink`, `PciideCy`, `PciideSis`,
//!   `PciidePdcsata`, `PciideSvwsata`) is `malloc`ed and written whole
//!   (`pciide_alloc_cookie`); a failed allocation panics where the C would dereference
//!   NULL. Its accessors (`satalink`, `pciide_cy`, ...) check the chip's `chip_map` before
//!   casting the cookie.
//! - `sis_hostbr_type_match`, which `pci_find_device`'s match function sets, is an
//!   `AtomicUsize` index into the table (0 is NULL); `pci_find_device(NULL, ..)` passes a
//!   scratch copy of the attach arguments.
//! - `intrstr ? intrstr : "unknown interrupt"`: `pci_intr_string` always returns a string.
//!   A Promise SATA channel's `name` is `None` (the C's NULL), printed as "".
//! - Kept as in the C (`docs/EXTERNAL_BUGS.md` candidates, listed in the port's handoff):
//!   `pciide_unmapregs_compat` unmaps the control register through the command block's
//!   handle; `cmd680_setup_channel` writes both bytes of a DMA/PIO timing to the same
//!   register; `hpt_chip_map` resets `compatchan` to 0 for every channel, so the HPT366's
//!   second function maps channel 0's compatibility addresses; `cy693_setup_channel` stores
//!   -1 in `DMA_mode`; `rdc_setup_channel` clears the other channel's timings, and neither
//!   sets the DMA status bits nor prints the modes.

#![allow(non_upper_case_globals)] // the C's patchable globals (`pciide_skip_ata`) and masks

use core::cell::Cell;
use core::cmp::min;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use crate::dev::ata::atavar::{
    AtaDriveDatas, DRIVE, DRIVE_ATA, DRIVE_ATAPI, DRIVE_DMA, DRIVE_UDMA,
};
use crate::dev::ata::satareg::*;
use crate::dev::ic::wdc::{
    wdc_alloc_queue, wdc_do_reset, wdc_drvp_chp, wdc_free_queue, wdc_print_current_modes,
    wdcattach, wdcdebug_print, wdcdetach, wdcintr, wdcprobe,
};
use crate::dev::ic::wdcreg::WDSD_IBM;
use crate::dev::ic::wdcvar::{
    _WDC_AUX, ChannelSoftc, WDC_CAPABILITY_DATA16, WDC_CAPABILITY_DATA32, WDC_CAPABILITY_DMA,
    WDC_CAPABILITY_IRQACK, WDC_CAPABILITY_MODE, WDC_CAPABILITY_NO_ATAPI_DMA, WDC_CAPABILITY_SATA,
    WDC_CAPABILITY_UDMA, WDC_DMA_LBA48, WDC_DMA_READ, WDC_DMAST_ERR, WDC_DMAST_NOIRQ,
    WDC_DMAST_UNDER, WDC_NREG, WDC_QUIRK_NOATA, WDC_QUIRK_NOATAPI, WDC_QUIRK_NOSHORTDMA,
    WDCF_DMA_BEFORE_CMD, WdcRegs, WdcSoftc, wdr_command, wdr_cyl_hi, wdr_cyl_lo, wdr_error,
    wdr_features, wdr_sdh, wdr_seccnt, wdr_sector, wdr_status,
};
use crate::dev::pci::cy82c693::{cy82c693_init, cy82c693_write};
use crate::dev::pci::pci::pci_find_device;
use crate::dev::pci::pci_map::{pci_mapreg_info, pci_mapreg_map, pci_mapreg_type};
use crate::dev::pci::pcidevs::*;
use crate::dev::pci::pciide_acard_reg::*;
use crate::dev::pci::pciide_acer_reg::*;
use crate::dev::pci::pciide_amd_reg::*;
use crate::dev::pci::pciide_apollo_reg::*;
use crate::dev::pci::pciide_cmd_reg::*;
use crate::dev::pci::pciide_cy693_reg::*;
use crate::dev::pci::pciide_hpt_reg::*;
use crate::dev::pci::pciide_ite_reg::*;
use crate::dev::pci::pciide_ixp_reg::*;
use crate::dev::pci::pciide_jmicron_reg::*;
use crate::dev::pci::pciide_natsemi_reg::*;
use crate::dev::pci::pciide_nforce_reg::*;
use crate::dev::pci::pciide_pdc202xx_reg::*;
use crate::dev::pci::pciide_piix_reg::*;
use crate::dev::pci::pciide_rdc_reg::*;
use crate::dev::pci::pciide_sii3112_reg::*;
use crate::dev::pci::pciide_sis_reg::*;
use crate::dev::pci::pciide_svwsata_reg::*;
use crate::dev::pci::pciidereg::idedma_ctl as idedma_ctl_reg;
use crate::dev::pci::pciidereg::*;
use crate::dev::pci::pciidevar::{PciideChannel, PciideDmaMaps, PciideSoftc};
use crate::dev::pci::pcireg::{
    PCI_BHLC_REG, PCI_CACHELINE_MASK, PCI_CACHELINE_SHIFT, PCI_CLASS_BRIDGE,
    PCI_CLASS_MASS_STORAGE, PCI_CLASS_REG, PCI_COMMAND_IO_ENABLE, PCI_COMMAND_MASTER_ENABLE,
    PCI_COMMAND_STATUS_REG, PCI_ID_REG, PCI_INTERFACE_SHIFT, PCI_MAPREG_END,
    PCI_MAPREG_MEM_TYPE_32BIT, PCI_MAPREG_START, PCI_MAPREG_TYPE_IO, PCI_MAPREG_TYPE_MEM,
    PCI_SUBCLASS_MASS_STORAGE_IDE, PCI_SUBCLASS_MASS_STORAGE_MISC, PCI_SUBCLASS_MASS_STORAGE_RAID,
    PCI_SUBCLASS_MASS_STORAGE_SATA, PCI_SUBSYS_ID_REG, pci_class, pci_interface, pci_product,
    pci_revision, pci_subclass, pci_vendor,
};
use crate::dev::pci::pcivar::{PciAttachArgs, Pcireg};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_autoconf::config_activate_children;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_ALLOCNOW, BUS_DMA_COHERENT, BUS_DMA_NOWAIT, BUS_DMASYNC_POSTREAD,
    BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE, BusDmaSegment, BusDmamap,
    BusSize, BusSpaceHandle, BusSpaceTag, bus_dmamap_create, bus_dmamap_load, bus_dmamap_sync,
    bus_dmamap_unload, bus_dmamem_alloc, bus_dmamem_map, bus_space_map, bus_space_read_1,
    bus_space_read_2, bus_space_read_4, bus_space_subregion, bus_space_unmap, bus_space_write_1,
    bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{IPL_BIO, splbio, splx};
use crate::machine::pci_machdep::{
    PciChipsetTag, Pcitag, pci_conf_read, pci_conf_write, pci_decompose_tag, pci_intr_disestablish,
    pci_intr_establish, pci_intr_map, pci_intr_string, pci_make_tag,
};
use crate::machine::pciide_machdep::{
    pciide_machdep_compat_intr_disestablish, pciide_machdep_compat_intr_establish,
};
use crate::sys::device::{
    CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_RESUME, DVACT_SUSPEND, Device,
};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};
use crate::sys::param::PAGE_SIZE;

/// `DEBUG_DMA`.
const DEBUG_DMA: i32 = 0x01;
/// `DEBUG_XFERS`.
const DEBUG_XFERS: i32 = 0x02;
/// `DEBUG_FUNCS`.
#[allow(dead_code)] // the C defines it; no message of this file uses it
const DEBUG_FUNCS: i32 = 0x08;
/// `DEBUG_PROBE`.
const DEBUG_PROBE: i32 = 0x10;

/// `WDCDEBUG_PCIIDE_MASK`.
#[cfg(feature = "wdcdebug")]
const WDCDEBUG_PCIIDE_MASK: i32 = 0x00;

/// `IDE_PCI_CLASS_OVERRIDE`: accept even if class != pciide.
pub const IDE_PCI_CLASS_OVERRIDE: u16 = 0x0001;
/// `IDE_16BIT_IOSPACE`: I/O space BARS ignore upper word.
pub const IDE_16BIT_IOSPACE: u16 = 0x0002;

/// `PCIIDE_OPTIONS_DMA`: options passed via the 'flags' config keyword.
pub const PCIIDE_OPTIONS_DMA: i32 = 0x01;

/// A chip's `chip_map`: map and setup chip, probe drives.
pub type PciideChipMap = fn(&'static PciideSoftc, &PciAttachArgs);

/// `struct pciide_product_desc`.
pub struct PciideProductDesc {
    /// `ide_product`.
    pub ide_product: u32,
    /// `ide_flags` (`IDE_*`).
    pub ide_flags: u16,
    /// `chip_map`: map and setup chip, probe drives.
    pub chip_map: PciideChipMap,
}

/// `struct pciide_vendor_desc`.
pub struct PciideVendorDesc {
    /// `ide_vendor`.
    pub ide_vendor: u32,
    /// `ide_products` and `ide_nproducts`.
    pub ide_products: &'static [PciideProductDesc],
}

/// `wdcdebug_pciide_mask`.
#[cfg(feature = "wdcdebug")]
pub static wdcdebug_pciide_mask: AtomicI32 = AtomicI32::new(WDCDEBUG_PCIIDE_MASK);

/// `pciide_skip_ata`: patchable; skip attaching ATA disks.
pub static pciide_skip_ata: AtomicI32 = AtomicI32::new(0);
/// `pciide_skip_atapi`: patchable; skip attaching ATAPI devices.
pub static pciide_skip_atapi: AtomicI32 = AtomicI32::new(0);

/// `default_product_desc`: default product description for devices not known from this
/// controller.
static DEFAULT_PRODUCT_DESC: PciideProductDesc = PciideProductDesc {
    ide_product: 0, // Generic PCI IDE controller
    ide_flags: 0,
    chip_map: default_chip_map,
};

/// `pciide_intel_products`.
static PCIIDE_INTEL_PRODUCTS: [PciideProductDesc; 95] = [
    // Intel 31244 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_31244,
        ide_flags: 0,
        chip_map: artisea_chip_map,
    },
    // Intel 82092AA IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82092AA,
        ide_flags: 0,
        chip_map: default_chip_map,
    },
    // Intel 82371FB IDE (PIIX)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82371FB_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82371FB IDE (PIIX)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82371FB_ISA,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82372FB IDE (PIIX4)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82372FB_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82371SB IDE (PIIX3)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82371SB_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82371AB IDE (PIIX4)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82371AB_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82371MX IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82371MX,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82440MX IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82440MX_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82451NX (PIIX4) IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82451NX,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801AA IDE (ICH)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801AA_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801AB IDE (ICH0)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801AB_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801BAM IDE (ICH2)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801BAM_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801BA IDE (ICH2)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801BA_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801CAM IDE (ICH3)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801CAM_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801CA IDE (ICH3)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801CA_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801DB IDE (ICH4)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801DB_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801DBL IDE (ICH4-L)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801DBL_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801DBM IDE (ICH4-M)
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801DBM_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801EB/ER (ICH5/5R) IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801EB_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801EB (ICH5) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801EB_SATA,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801ER (ICH5R) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801ER_SATA,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 6300ESB IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_6300ESB_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 6300ESB SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_6300ESB_SATA,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 6300ESB SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_6300ESB_SATA2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 6321ESB IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_6321ESB_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801FB (ICH6) IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801FB_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801FBM (ICH6M) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801FBM_SATA,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801FB (ICH6) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801FB_SATA,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801FR (ICH6R) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801FR_SATA,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801GB (ICH7) IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801GB_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801GB (ICH7) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801GB_SATA,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801GR (ICH7R) AHCI
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801GR_AHCI,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801GR (ICH7R) RAID
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801GR_RAID,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801GBM (ICH7M) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801GBM_SATA,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801GBM (ICH7M) AHCI
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801GBM_AHCI,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801GHM (ICH7M DH) RAID
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801GHM_RAID,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801H (ICH8) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801H_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801H (ICH8) AHCI
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801H_AHCI_6P,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801H (ICH8) RAID
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801H_RAID,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801H (ICH8) AHCI
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801H_AHCI_4P,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801H (ICH8) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801H_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801HBM (ICH8M) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801HBM_SATA,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801HBM (ICH8M) AHCI
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801HBM_AHCI,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801HBM (ICH8M) RAID
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801HBM_RAID,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801HBM (ICH8M) IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801HBM_IDE,
        ide_flags: 0,
        chip_map: piix_chip_map,
    },
    // Intel 82801I (ICH9) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801I_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801I (ICH9) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801I_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801I (ICH9) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801I_SATA_3,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801I (ICH9) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801I_SATA_4,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801I (ICH9M) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801I_SATA_5,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801I (ICH9M) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801I_SATA_6,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801JD (ICH10) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801JD_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801JD (ICH10) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801JD_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801JI (ICH10) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801JI_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 82801JI (ICH10) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_82801JI_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 6321ESB SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_6321ESB_SATA,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 3400 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_3400_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 3400 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_3400_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 3400 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_3400_SATA_3,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 3400 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_3400_SATA_4,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 3400 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_3400_SATA_5,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 3400 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_3400_SATA_6,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel C600 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_C600_SATA,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel C610 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_C610_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel C610 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_C610_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel C610 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_C610_SATA_3,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 6 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_6SERIES_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 6 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_6SERIES_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 6 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_6SERIES_SATA_3,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 6 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_6SERIES_SATA_4,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 7 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_7SERIES_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 7 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_7SERIES_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 7 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_7SERIES_SATA_3,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 7 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_7SERIES_SATA_4,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 8 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_8SERIES_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 8 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_8SERIES_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 8 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_8SERIES_SATA_3,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 8 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_8SERIES_SATA_4,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 8 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_8SERIES_LP_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 8 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_8SERIES_LP_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 8 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_8SERIES_LP_SATA_3,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 8 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_8SERIES_LP_SATA_4,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 9 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_9SERIES_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel 9 Series SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_9SERIES_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel Atom C2000 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_ATOMC2000_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel Atom C2000 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_ATOMC2000_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel Atom C2000 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_ATOMC2000_SATA_3,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel Atom C2000 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_ATOMC2000_SATA_4,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel Baytrail SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_BAYTRAIL_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel Baytrail SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_BAYTRAIL_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel EP80579 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_EP80579_SATA,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel DH8900 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_DH8900_SATA_1,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel DH8900 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_DH8900_SATA_2,
        ide_flags: 0,
        chip_map: piixsata_chip_map,
    },
    // Intel SCH IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_INTEL_SCH_IDE,
        ide_flags: 0,
        chip_map: sch_chip_map,
    },
];

/// `pciide_amd_products`.
static PCIIDE_AMD_PRODUCTS: [PciideProductDesc; 6] = [
    // AMD 756
    PciideProductDesc {
        ide_product: PCI_PRODUCT_AMD_PBC756_IDE,
        ide_flags: 0,
        chip_map: amd756_chip_map,
    },
    // AMD 766
    PciideProductDesc {
        ide_product: PCI_PRODUCT_AMD_766_IDE,
        ide_flags: 0,
        chip_map: amd756_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_AMD_PBC768_IDE,
        ide_flags: 0,
        chip_map: amd756_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_AMD_8111_IDE,
        ide_flags: 0,
        chip_map: amd756_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_AMD_CS5536_IDE,
        ide_flags: 0,
        chip_map: amd756_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_AMD_HUDSON2_IDE,
        ide_flags: 0,
        chip_map: ixp_chip_map,
    },
];

/// `pciide_cmd_products`.
static PCIIDE_CMD_PRODUCTS: [PciideProductDesc; 10] = [
    // CMD Technology PCI0640
    PciideProductDesc {
        ide_product: PCI_PRODUCT_CMDTECH_640,
        ide_flags: 0,
        chip_map: cmd_chip_map,
    },
    // CMD Technology PCI0643
    PciideProductDesc {
        ide_product: PCI_PRODUCT_CMDTECH_643,
        ide_flags: 0,
        chip_map: cmd0643_9_chip_map,
    },
    // CMD Technology PCI0646
    PciideProductDesc {
        ide_product: PCI_PRODUCT_CMDTECH_646,
        ide_flags: 0,
        chip_map: cmd0643_9_chip_map,
    },
    // CMD Technology PCI0648
    PciideProductDesc {
        ide_product: PCI_PRODUCT_CMDTECH_648,
        ide_flags: 0,
        chip_map: cmd0643_9_chip_map,
    },
    // CMD Technology PCI0649
    PciideProductDesc {
        ide_product: PCI_PRODUCT_CMDTECH_649,
        ide_flags: 0,
        chip_map: cmd0643_9_chip_map,
    },
    // CMD Technology PCI0680
    PciideProductDesc {
        ide_product: PCI_PRODUCT_CMDTECH_680,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: cmd680_chip_map,
    },
    // SiI3112 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_CMDTECH_3112,
        ide_flags: 0,
        chip_map: sii3112_chip_map,
    },
    // SiI3512 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_CMDTECH_3512,
        ide_flags: 0,
        chip_map: sii3112_chip_map,
    },
    // Adaptec AAR-1210SA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_CMDTECH_AAR_1210SA,
        ide_flags: 0,
        chip_map: sii3112_chip_map,
    },
    // SiI3114 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_CMDTECH_3114,
        ide_flags: 0,
        chip_map: sii3114_chip_map,
    },
];

/// `pciide_via_products`.
static PCIIDE_VIA_PRODUCTS: [PciideProductDesc; 16] = [
    // VIA VT82C416 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VT82C416,
        ide_flags: 0,
        chip_map: apollo_chip_map,
    },
    // VIA VT82C571 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VT82C571,
        ide_flags: 0,
        chip_map: apollo_chip_map,
    },
    // VIA VT6410 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VT6410,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: apollo_chip_map,
    },
    // VIA VT6415 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VT6415,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: apollo_chip_map,
    },
    // VIA CX700 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_CX700_IDE,
        ide_flags: 0,
        chip_map: apollo_chip_map,
    },
    // VIA VX700 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VX700_IDE,
        ide_flags: 0,
        chip_map: apollo_chip_map,
    },
    // VIA VX855 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VX855_IDE,
        ide_flags: 0,
        chip_map: apollo_chip_map,
    },
    // VIA VX900 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VX900_IDE,
        ide_flags: 0,
        chip_map: apollo_chip_map,
    },
    // VIA VT6420 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VT6420_SATA,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    // VIA VT6421 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VT6421_SATA,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    // VIA VT8237A SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VT8237A_SATA,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    // VIA VT8237A SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VT8237A_SATA_2,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    // VIA VT8237S SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VT8237S_SATA,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    // VIA VT8251 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VT8251_SATA,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    // VIA VT8251(CE) SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VT8251_SATA_2,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    // VIA VT8261 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_VIATECH_VT8261_SATA,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
];

/// `pciide_cypress_products`.
static PCIIDE_CYPRESS_PRODUCTS: [PciideProductDesc; 1] = [
    // Contaq CY82C693 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_CONTAQ_82C693,
        ide_flags: IDE_16BIT_IOSPACE,
        chip_map: cy693_chip_map,
    },
];

/// `pciide_sis_products`.
static PCIIDE_SIS_PRODUCTS: [PciideProductDesc; 5] = [
    // SIS 5513 EIDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_SIS_5513,
        ide_flags: 0,
        chip_map: sis_chip_map,
    },
    // SIS 180 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_SIS_180,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    // SIS 181 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_SIS_181,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    // SIS 182 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_SIS_182,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    // SIS 1183 SATA
    PciideProductDesc {
        ide_product: PCI_PRODUCT_SIS_1183,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
];

/// `pciide_natsemi_products`.
static PCIIDE_NATSEMI_PRODUCTS: [PciideProductDesc; 2] = [
    // National Semi PC87415 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NS_PC87415,
        ide_flags: 0,
        chip_map: natsemi_chip_map,
    },
    // National Semi SCx200 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NS_SCX200_IDE,
        ide_flags: 0,
        chip_map: ns_scx200_chip_map,
    },
];

/// `pciide_acer_products`.
static PCIIDE_ACER_PRODUCTS: [PciideProductDesc; 1] = [
    // Acer Labs M5229 UDMA IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ALI_M5229,
        ide_flags: 0,
        chip_map: acer_chip_map,
    },
];

/// `pciide_triones_products`.
static PCIIDE_TRIONES_PRODUCTS: [PciideProductDesc; 5] = [
    // Highpoint HPT36x/37x IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_TRIONES_HPT366,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: hpt_chip_map,
    },
    // Highpoint HPT372A IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_TRIONES_HPT372A,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: hpt_chip_map,
    },
    // Highpoint HPT302 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_TRIONES_HPT302,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: hpt_chip_map,
    },
    // Highpoint HPT371 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_TRIONES_HPT371,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: hpt_chip_map,
    },
    // Highpoint HPT374 IDE
    PciideProductDesc {
        ide_product: PCI_PRODUCT_TRIONES_HPT374,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: hpt_chip_map,
    },
];

/// `pciide_promise_products`.
static PCIIDE_PROMISE_PRODUCTS: [PciideProductDesc; 29] = [
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20246,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdc202xx_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20262,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdc202xx_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20265,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdc202xx_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20267,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdc202xx_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20268,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdc202xx_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20268R,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdc202xx_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20269,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdc202xx_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20271,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdc202xx_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20275,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdc202xx_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20276,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdc202xx_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20277,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdc202xx_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20318,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20319,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20371,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20375,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20376,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20377,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20378,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20379,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC40518,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC40519,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC40718,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC40719,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC40779,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20571,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20575,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20579,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20771,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_PROMISE_PDC20775,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: pdcsata_chip_map,
    },
];

/// `pciide_acard_products`.
static PCIIDE_ACARD_PRODUCTS: [PciideProductDesc; 5] = [
    // Acard ATP850U Ultra33 Controller
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ACARD_ATP850U,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: acard_chip_map,
    },
    // Acard ATP860 Ultra66 Controller
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ACARD_ATP860,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: acard_chip_map,
    },
    // Acard ATP860-A Ultra66 Controller
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ACARD_ATP860A,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: acard_chip_map,
    },
    // Acard ATP865-A Ultra133 Controller
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ACARD_ATP865A,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: acard_chip_map,
    },
    // Acard ATP865-R Ultra133 Controller
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ACARD_ATP865R,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: acard_chip_map,
    },
];

/// `pciide_serverworks_products`.
static PCIIDE_SERVERWORKS_PRODUCTS: [PciideProductDesc; 10] = [
    PciideProductDesc {
        ide_product: PCI_PRODUCT_RCC_OSB4_IDE,
        ide_flags: 0,
        chip_map: serverworks_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_RCC_CSB5_IDE,
        ide_flags: 0,
        chip_map: serverworks_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_RCC_CSB6_IDE,
        ide_flags: 0,
        chip_map: serverworks_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_RCC_CSB6_RAID_IDE,
        ide_flags: 0,
        chip_map: serverworks_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_RCC_HT_1000_IDE,
        ide_flags: 0,
        chip_map: serverworks_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_RCC_K2_SATA,
        ide_flags: 0,
        chip_map: svwsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_RCC_FRODO4_SATA,
        ide_flags: 0,
        chip_map: svwsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_RCC_FRODO8_SATA,
        ide_flags: 0,
        chip_map: svwsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_RCC_HT_1000_SATA_1,
        ide_flags: 0,
        chip_map: svwsata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_RCC_HT_1000_SATA_2,
        ide_flags: 0,
        chip_map: svwsata_chip_map,
    },
];

/// `pciide_nvidia_products`.
static PCIIDE_NVIDIA_PRODUCTS: [PciideProductDesc; 45] = [
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_NFORCE_IDE,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_NFORCE2_IDE,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_NFORCE2_400_IDE,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_NFORCE3_IDE,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_NFORCE3_250_IDE,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_NFORCE4_ATA133,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP04_IDE,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP51_IDE,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP55_IDE,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP61_IDE,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP65_IDE,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP67_IDE,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP73_IDE,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP77_IDE,
        ide_flags: 0,
        chip_map: nforce_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_NFORCE2_400_SATA,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_NFORCE3_250_SATA,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_NFORCE3_250_SATA2,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_NFORCE4_SATA1,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_NFORCE4_SATA2,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP04_SATA,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP04_SATA2,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP51_SATA,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP51_SATA2,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP55_SATA,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP55_SATA2,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP61_SATA,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP61_SATA2,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP61_SATA3,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP65_SATA_1,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP65_SATA_2,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP65_SATA_3,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP65_SATA_4,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP67_SATA_1,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP67_SATA_2,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP67_SATA_3,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP67_SATA_4,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP77_SATA_1,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP79_SATA_1,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP79_SATA_2,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP79_SATA_3,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP79_SATA_4,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP89_SATA_1,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP89_SATA_2,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP89_SATA_3,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_NVIDIA_MCP89_SATA_4,
        ide_flags: 0,
        chip_map: sata_chip_map,
    },
];

/// `pciide_ite_products`.
static PCIIDE_ITE_PRODUCTS: [PciideProductDesc; 2] = [
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ITEXPRESS_IT8211F,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: ite_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ITEXPRESS_IT8212F,
        ide_flags: IDE_PCI_CLASS_OVERRIDE,
        chip_map: ite_chip_map,
    },
];

/// `pciide_ati_products`.
static PCIIDE_ATI_PRODUCTS: [PciideProductDesc; 8] = [
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ATI_SB200_IDE,
        ide_flags: 0,
        chip_map: ixp_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ATI_SB300_IDE,
        ide_flags: 0,
        chip_map: ixp_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ATI_SB400_IDE,
        ide_flags: 0,
        chip_map: ixp_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ATI_SB600_IDE,
        ide_flags: 0,
        chip_map: ixp_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ATI_SB700_IDE,
        ide_flags: 0,
        chip_map: ixp_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ATI_SB300_SATA,
        ide_flags: 0,
        chip_map: sii3112_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ATI_SB400_SATA_1,
        ide_flags: 0,
        chip_map: sii3112_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_ATI_SB400_SATA_2,
        ide_flags: 0,
        chip_map: sii3112_chip_map,
    },
];

/// `pciide_jmicron_products`.
static PCIIDE_JMICRON_PRODUCTS: [PciideProductDesc; 5] = [
    PciideProductDesc {
        ide_product: PCI_PRODUCT_JMICRON_JMB361,
        ide_flags: 0,
        chip_map: jmicron_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_JMICRON_JMB363,
        ide_flags: 0,
        chip_map: jmicron_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_JMICRON_JMB365,
        ide_flags: 0,
        chip_map: jmicron_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_JMICRON_JMB366,
        ide_flags: 0,
        chip_map: jmicron_chip_map,
    },
    PciideProductDesc {
        ide_product: PCI_PRODUCT_JMICRON_JMB368,
        ide_flags: 0,
        chip_map: jmicron_chip_map,
    },
];

/// `pciide_phison_products`.
static PCIIDE_PHISON_PRODUCTS: [PciideProductDesc; 1] = [PciideProductDesc {
    ide_product: PCI_PRODUCT_PHISON_PS5000,
    ide_flags: 0,
    chip_map: phison_chip_map,
}];

/// `pciide_rdc_products`.
static PCIIDE_RDC_PRODUCTS: [PciideProductDesc; 1] = [PciideProductDesc {
    ide_product: PCI_PRODUCT_RDC_R1012_IDE,
    ide_flags: 0,
    chip_map: rdc_chip_map,
}];

/// `pciide_vendors`.
static PCIIDE_VENDORS: [PciideVendorDesc; 18] = [
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_INTEL,
        ide_products: &PCIIDE_INTEL_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_AMD,
        ide_products: &PCIIDE_AMD_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_CMDTECH,
        ide_products: &PCIIDE_CMD_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_VIATECH,
        ide_products: &PCIIDE_VIA_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_CONTAQ,
        ide_products: &PCIIDE_CYPRESS_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_SIS,
        ide_products: &PCIIDE_SIS_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_NS,
        ide_products: &PCIIDE_NATSEMI_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_ALI,
        ide_products: &PCIIDE_ACER_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_TRIONES,
        ide_products: &PCIIDE_TRIONES_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_ACARD,
        ide_products: &PCIIDE_ACARD_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_RCC,
        ide_products: &PCIIDE_SERVERWORKS_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_PROMISE,
        ide_products: &PCIIDE_PROMISE_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_NVIDIA,
        ide_products: &PCIIDE_NVIDIA_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_ITEXPRESS,
        ide_products: &PCIIDE_ITE_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_ATI,
        ide_products: &PCIIDE_ATI_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_JMICRON,
        ide_products: &PCIIDE_JMICRON_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_PHISON,
        ide_products: &PCIIDE_PHISON_PRODUCTS,
    },
    PciideVendorDesc {
        ide_vendor: PCI_VENDOR_RDC,
        ide_products: &PCIIDE_RDC_PRODUCTS,
    },
];

/// `pciide_pci_ca`.
pub static PCIIDE_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PciideSoftc>(),
    ca_match: Some(pciide_match),
    ca_attach: pciide_attach,
    ca_detach: Some(pciide_detach),
    ca_activate: Some(pciide_activate),
};

/// `pciide_jmb_ca` (`pciide* at jmb?`; jmb(4) is not ported, so nothing attaches with it).
pub static PCIIDE_JMB_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PciideSoftc>(),
    ca_match: Some(pciide_match),
    ca_attach: pciide_attach,
    ca_detach: Some(pciide_detach),
    ca_activate: Some(pciide_activate),
};

/// `pciide_cd`.
pub static PCIIDE_CD: Cfdriver = Cfdriver::new(b"pciide", DV_DULL, 0);

/// `(struct pciide_softc *)chp->wdc`: the pciide softc whose `sc_wdcdev` is `wdc`. Panics
/// if `wdc` is not a pciide controller's.
pub fn pciide_sc(wdc: &WdcSoftc) -> &PciideSoftc {
    let ca = wdc.sc_dev.cfdata().cf_attach;
    if !ptr::eq(ca, &PCIIDE_PCI_CA) && !ptr::eq(ca, &PCIIDE_JMB_CA) {
        panic(format_args!(
            "{}: not a pciide controller",
            wdc.sc_dev.xname()
        ));
    }
    // SAFETY: the device was made for one of pciide's attachments, whose softc is a
    // `PciideSoftc`: `#[repr(C)]` with `sc_wdcdev` first, so both share an address, and the
    // allocation holds the whole softc for as long as `wdc` is borrowed.
    unsafe { &*ptr::from_ref(wdc).cast::<PciideSoftc>() }
}

/// `(struct pciide_channel *)chp`: the pciide channel whose `wdc_channel` is `chp`.
pub fn pciide_cp(chp: &ChannelSoftc) -> &PciideChannel {
    let sc = pciide_sc(chp.wdc());
    match sc
        .pciide_channels
        .iter()
        .find(|cp| ptr::eq(&cp.wdc_channel, chp))
    {
        Some(cp) => cp,
        None => panic(format_args!("{}: foreign channel", sc.xname())),
    }
}

/// `pciide_pci_read`: an 8-bit PCI configuration register.
pub fn pciide_pci_read(pc: PciChipsetTag, pa: Pcitag, reg: i32) -> u8 {
    ((pci_conf_read(pc, pa, reg & !0x03) >> ((reg & 0x03) * 8)) & 0xff) as u8
}

/// `pciide_pci_write`.
pub fn pciide_pci_write(pc: PciChipsetTag, pa: Pcitag, reg: i32, val: u8) {
    let mut pcival = pci_conf_read(pc, pa, reg & !0x03);
    pcival &= !(0xff << ((reg & 0x03) * 8));
    pcival |= u32::from(val) << ((reg & 0x03) * 8);
    pci_conf_write(pc, pa, reg & !0x03, pcival);
}

/// `pciide_lookup_product`.
pub fn pciide_lookup_product(id: u32) -> Option<&'static PciideProductDesc> {
    let vp = PCIIDE_VENDORS
        .iter()
        .find(|vp| pci_vendor(id) == vp.ide_vendor)?;

    vp.ide_products
        .iter()
        .find(|pp| pci_product(id) == pp.ide_product)
}

/// `pciide_match`.
pub fn pciide_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    // Some IDE controllers have severe bugs when used in PCI mode. We punt and attach them
    // to the ISA bus instead.
    if pci_vendor(pa.pa_id) == PCI_VENDOR_PCTECH
        && pci_product(pa.pa_id) == PCI_PRODUCT_PCTECH_RZ1000
    {
        return 0;
    }

    // Some controllers (e.g. promise Ultra-33) don't claim to be PCI IDE controllers. Let
    // see if we can deal with it anyway.
    let pp = pciide_lookup_product(pa.pa_id);
    if pp.is_some_and(|pp| pp.ide_flags & IDE_PCI_CLASS_OVERRIDE != 0) {
        return 1;
    }

    // Check the ID register to see that it's a PCI IDE controller. If it is, we assume
    // that we can deal with it; it _should_ work in a standardized way...
    if pci_class(pa.pa_class) == PCI_CLASS_MASS_STORAGE {
        match pci_subclass(pa.pa_class) {
            PCI_SUBCLASS_MASS_STORAGE_IDE => return 1,
            // We only match these if we know they have a match, as we may not support
            // native interfaces on them.
            PCI_SUBCLASS_MASS_STORAGE_SATA
            | PCI_SUBCLASS_MASS_STORAGE_RAID
            | PCI_SUBCLASS_MASS_STORAGE_MISC => return i32::from(pp.is_some()),
            _ => {}
        }
    }

    0
}

/// `pciide_attach`.
pub fn pciide_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for a pciide attachment, whose softc is a `PciideSoftc`;
    // softcs are never freed while the device exists, so it may be borrowed for 'static.
    let sc: &'static PciideSoftc = unsafe { &*ptr::from_ref(self_.softc::<PciideSoftc>()) };
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    sc.sc_pp.set(Some(
        pciide_lookup_product(pa.pa_id).unwrap_or(&DEFAULT_PRODUCT_DESC),
    ));
    sc.sc_rev.set(pci_revision(pa.pa_class) as i32);

    sc.sc_pc.set(Some(pa.pa_pc));
    sc.sc_tag.set(pa.pa_tag);

    // Set up DMA defaults; these might be adjusted by chip_map.
    sc.sc_dma_maxsegsz.set(IDEDMA_BYTE_COUNT_MAX);
    sc.sc_dma_boundary.set(IDEDMA_BYTE_COUNT_ALIGN);

    sc.sc_dmacmd_read.set(Some(pciide_dmacmd_read));
    sc.sc_dmacmd_write.set(Some(pciide_dmacmd_write));
    sc.sc_dmactl_read.set(Some(pciide_dmactl_read));
    sc.sc_dmactl_write.set(Some(pciide_dmactl_write));
    sc.sc_dmatbl_write.set(Some(pciide_dmatbl_write));

    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        " sc_pc={:p}, sc_tag={:?}, pa_class=0x{:x}\n",
        sc.sc_pc.as_ptr(),
        pci_decompose_tag(pa.pa_pc, sc.sc_tag.get()),
        pa.pa_class
    );

    if pciide_skip_ata.load(Ordering::Relaxed) != 0 {
        sc.sc_wdcdev
            .quirks
            .set(sc.sc_wdcdev.quirks.get() | WDC_QUIRK_NOATA);
    }
    if pciide_skip_atapi.load(Ordering::Relaxed) != 0 {
        sc.sc_wdcdev
            .quirks
            .set(sc.sc_wdcdev.quirks.get() | WDC_QUIRK_NOATAPI);
    }

    (sc.pp().chip_map)(sc, pa);

    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "pciide: command/status register=0x{:x}\n",
        pci_conf_read(sc.pc(), sc.tag(), PCI_COMMAND_STATUS_REG)
    );
}

/// `pciide_detach`.
pub fn pciide_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for a pciide attachment; softcs are never freed while the
    // device exists.
    let sc: &'static PciideSoftc = unsafe { &*ptr::from_ref(self_.softc::<PciideSoftc>()) };
    match sc.chip_unmap.get() {
        None => panic(format_args!("unmap not yet implemented for this chipset")),
        Some(unmap) => unmap(sc, flags),
    }

    Ok(())
}

/// `pciide_activate`.
pub fn pciide_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for a pciide attachment; softcs are never freed while the
    // device exists.
    let sc: &'static PciideSoftc = unsafe { &*ptr::from_ref(self_.softc::<PciideSoftc>()) };
    let is = |f: PciideChipMap| ptr::fn_addr_eq(sc.pp().chip_map, f);
    let (pc, tag) = (sc.pc(), sc.tag());
    let rv;

    match act {
        DVACT_SUSPEND => {
            rv = config_activate_children(self_, act);

            for (i, save) in sc.sc_save.iter().enumerate() {
                save.set(pci_conf_read(
                    pc,
                    tag,
                    PCI_MAPREG_END + 0x18 + (i as i32 * 4),
                ));
            }

            if is(sch_chip_map) {
                sc.sc_save2[0].set(pci_conf_read(pc, tag, SCH_D0TIM));
                sc.sc_save2[1].set(pci_conf_read(pc, tag, SCH_D1TIM));
            } else if is(piixsata_chip_map) {
                sc.sc_save2[0].set(pciide_pci_read(pc, tag, ICH5_SATA_MAP).into());
                sc.sc_save2[1].set(pciide_pci_read(pc, tag, ICH5_SATA_PI).into());
                sc.sc_save2[2].set(pciide_pci_read(pc, tag, ICH_SATA_PCS).into());
            } else if is(sii3112_chip_map) {
                sc.sc_save2[0].set(pci_conf_read(pc, tag, SII3112_SCS_CMD));
                sc.sc_save2[1].set(pci_conf_read(pc, tag, SII3112_PCI_CFGCTL));
            } else if is(ite_chip_map) {
                sc.sc_save2[0].set(pci_conf_read(pc, tag, it_tim(0)));
            } else if is(nforce_chip_map) {
                sc.sc_save2[0].set(pci_conf_read(pc, tag, NFORCE_PIODMATIM));
                sc.sc_save2[1].set(pci_conf_read(pc, tag, NFORCE_PIOTIM));
                sc.sc_save2[2].set(pci_conf_read(pc, tag, NFORCE_UDMATIM));
            }
        }
        DVACT_RESUME => {
            for (i, save) in sc.sc_save.iter().enumerate() {
                pci_conf_write(pc, tag, PCI_MAPREG_END + 0x18 + (i as i32 * 4), save.get());
            }

            if is(default_chip_map)
                || is(sata_chip_map)
                || is(piix_chip_map)
                || is(amd756_chip_map)
                || is(phison_chip_map)
                || is(rdc_chip_map)
                || is(ixp_chip_map)
                || is(acard_chip_map)
                || is(apollo_chip_map)
                || is(sis_chip_map)
            {
                // nothing to restore -- uses only 0x40 - 0x56
            } else if is(sch_chip_map) {
                pci_conf_write(pc, tag, SCH_D0TIM, sc.sc_save2[0].get());
                pci_conf_write(pc, tag, SCH_D1TIM, sc.sc_save2[1].get());
            } else if is(piixsata_chip_map) {
                pciide_pci_write(pc, tag, ICH5_SATA_MAP, sc.sc_save2[0].get() as u8);
                pciide_pci_write(pc, tag, ICH5_SATA_PI, sc.sc_save2[1].get() as u8);
                pciide_pci_write(pc, tag, ICH_SATA_PCS, sc.sc_save2[2].get() as u8);
            } else if is(sii3112_chip_map) {
                pci_conf_write(pc, tag, SII3112_SCS_CMD, sc.sc_save2[0].get());
                delay(50 * 1000);
                pci_conf_write(pc, tag, SII3112_PCI_CFGCTL, sc.sc_save2[1].get());
                delay(50 * 1000);
            } else if is(ite_chip_map) {
                pci_conf_write(pc, tag, it_tim(0), sc.sc_save2[0].get());
            } else if is(nforce_chip_map) {
                pci_conf_write(pc, tag, NFORCE_PIODMATIM, sc.sc_save2[0].get());
                pci_conf_write(pc, tag, NFORCE_PIOTIM, sc.sc_save2[1].get());
                pci_conf_write(pc, tag, NFORCE_UDMATIM, sc.sc_save2[2].get());
            } else {
                printf(format_args!(
                    "{}: restore for unknown chip map {:x}\n",
                    sc.xname(),
                    sc.pp().ide_product
                ));
            }

            rv = config_activate_children(self_, act);
        }
        _ => {
            rv = config_activate_children(self_, act);
        }
    }
    rv
}

/// `pciide_mapregs_compat`.
pub fn pciide_mapregs_compat(
    pa: &PciAttachArgs,
    cp: &PciideChannel,
    compatchan: i32,
    cmdsizep: &mut BusSize,
    ctlsizep: &mut BusSize,
) -> bool {
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let wdc_cp = &cp.wdc_channel;

    cp.compat.set(1);
    *cmdsizep = PCIIDE_COMPAT_CMD_SIZE;
    *ctlsizep = PCIIDE_COMPAT_CTL_SIZE;

    let csr = pci_conf_read(sc.pc(), sc.tag(), PCI_COMMAND_STATUS_REG);
    pci_conf_write(
        sc.pc(),
        sc.tag(),
        PCI_COMMAND_STATUS_REG,
        csr | PCI_COMMAND_IO_ENABLE | PCI_COMMAND_MASTER_ENABLE,
    );

    wdc_cp.cmd_iot.set(Some(pa.pa_iot));

    // SAFETY: the legacy command block of this compatibility channel, which the controller
    // decodes for the channel.
    match unsafe {
        bus_space_map(
            pa.pa_iot,
            pciide_compat_cmd_base(compatchan),
            PCIIDE_COMPAT_CMD_SIZE,
            0,
        )
    } {
        Ok(h) => wdc_cp.cmd_ioh.set(Some(h)),
        Err(_) => {
            printf(format_args!(
                "{}: couldn't map {} cmd regs\n",
                sc.xname(),
                cp.name()
            ));
            return false;
        }
    }

    wdc_cp.ctl_iot.set(Some(pa.pa_iot));

    // SAFETY: the legacy control register of the same channel.
    match unsafe {
        bus_space_map(
            pa.pa_iot,
            pciide_compat_ctl_base(compatchan),
            PCIIDE_COMPAT_CTL_SIZE,
            0,
        )
    } {
        Ok(h) => wdc_cp.ctl_ioh.set(Some(h)),
        Err(_) => {
            printf(format_args!(
                "{}: couldn't map {} ctl regs\n",
                sc.xname(),
                cp.name()
            ));
            bus_space_unmap(wdc_cp.cmd_iot(), wdc_cp.cmd_ioh(), PCIIDE_COMPAT_CMD_SIZE);
            return false;
        }
    }
    wdc_cp.cmd_iosz.set(*cmdsizep);
    wdc_cp.ctl_iosz.set(*ctlsizep);

    true
}

/// `pciide_unmapregs_compat`.
///
/// Like the C, the control registers are unmapped through the command block's handle
/// (`wdc_cp->cmd_ioh`), not `ctl_ioh`.
pub fn pciide_unmapregs_compat(sc: &PciideSoftc, cp: &PciideChannel) -> i32 {
    let wdc_cp = &cp.wdc_channel;

    bus_space_unmap(wdc_cp.cmd_iot(), wdc_cp.cmd_ioh(), wdc_cp.cmd_iosz.get());
    bus_space_unmap(wdc_cp.ctl_iot(), wdc_cp.cmd_ioh(), wdc_cp.ctl_iosz.get());

    if let Some(ih) = NonNull::new(sc.sc_pci_ih.replace(ptr::null_mut())) {
        // SAFETY: `sc_pci_ih` holds a handle this driver established, dropped here.
        unsafe { pciide_machdep_compat_intr_disestablish(sc.pc(), ih) };
    }

    0
}

/// `pciide_mapregs_native`.
pub fn pciide_mapregs_native(
    pa: &PciAttachArgs,
    cp: &'static PciideChannel,
    cmdsizep: &mut BusSize,
    ctlsizep: &mut BusSize,
    pci_intr: fn(*mut c_void) -> i32,
) -> bool {
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let wdc_cp = &cp.wdc_channel;

    cp.compat.set(0);

    if sc.sc_pci_ih.get().is_null() {
        let Some(intrhandle) = pci_intr_map(pa) else {
            printf(format_args!(
                "{}: couldn't map native-PCI interrupt\n",
                sc.xname()
            ));
            return false;
        };
        let intrstr = pci_intr_string(pa.pa_pc, intrhandle);
        let ih = pci_intr_establish(
            pa.pa_pc,
            intrhandle,
            IPL_BIO,
            pci_intr,
            ptr::from_ref(sc).cast_mut().cast(),
            sc.sc_wdcdev.sc_dev.xname(),
        );
        match ih {
            Some(ih) => {
                sc.sc_pci_ih.set(ih.as_ptr());
                printf(format_args!(
                    "{}: using {} for native-PCI interrupt\n",
                    sc.xname(),
                    intrstr
                ));
            }
            None => {
                printf(format_args!(
                    "{}: couldn't establish native-PCI interrupt",
                    sc.xname()
                ));
                printf(format_args!(" at {}", intrstr));
                printf(format_args!("\n"));
                return false;
            }
        }
    }
    cp.ih.set(sc.sc_pci_ih.get());
    sc.sc_pc.set(Some(pa.pa_pc));

    let channel = wdc_cp.channel.get();
    let maptype = pci_mapreg_type(pa.pa_pc, pa.pa_tag, pciide_reg_cmd_base(channel));
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "{}: {} cmd regs mapping: {}\n",
        sc.xname(),
        cp.name(),
        if maptype == PCI_MAPREG_TYPE_IO {
            "I/O"
        } else {
            "memory"
        }
    );
    match pci_mapreg_map(pa, pciide_reg_cmd_base(channel), maptype, 0, 0) {
        Ok((iot, ioh, _, size)) => {
            wdc_cp.cmd_iot.set(Some(iot));
            wdc_cp.cmd_ioh.set(Some(ioh));
            *cmdsizep = size;
        }
        Err(_) => {
            printf(format_args!(
                "{}: couldn't map {} cmd regs\n",
                sc.xname(),
                cp.name()
            ));
            return false;
        }
    }

    let maptype = pci_mapreg_type(pa.pa_pc, pa.pa_tag, pciide_reg_ctl_base(channel));
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "{}: {} ctl regs mapping: {}\n",
        sc.xname(),
        cp.name(),
        if maptype == PCI_MAPREG_TYPE_IO {
            "I/O"
        } else {
            "memory"
        }
    );
    match pci_mapreg_map(pa, pciide_reg_ctl_base(channel), maptype, 0, 0) {
        Ok((iot, ioh, _, size)) => {
            wdc_cp.ctl_iot.set(Some(iot));
            cp.ctl_baseioh.set(Some(ioh));
            *ctlsizep = size;
        }
        Err(_) => {
            printf(format_args!(
                "{}: couldn't map {} ctl regs\n",
                sc.xname(),
                cp.name()
            ));
            bus_space_unmap(wdc_cp.cmd_iot(), wdc_cp.cmd_ioh(), *cmdsizep);
            return false;
        }
    }
    let Some(ctl_baseioh) = cp.ctl_baseioh.get() else {
        return false;
    };
    // In native mode, 4 bytes of I/O space are mapped for the control register, the
    // control register is at offset 2. Pass the generic code a handle for only one byte at
    // the right offset.
    match bus_space_subregion(wdc_cp.ctl_iot(), ctl_baseioh, 2, 1) {
        Ok(h) => wdc_cp.ctl_ioh.set(Some(h)),
        Err(_) => {
            printf(format_args!(
                "{}: unable to subregion {} ctl regs\n",
                sc.xname(),
                cp.name()
            ));
            bus_space_unmap(wdc_cp.cmd_iot(), wdc_cp.cmd_ioh(), *cmdsizep);
            // The C passes cmd_iot for the control block here.
            bus_space_unmap(wdc_cp.cmd_iot(), ctl_baseioh, *ctlsizep);
            return false;
        }
    }
    wdc_cp.cmd_iosz.set(*cmdsizep);
    wdc_cp.ctl_iosz.set(*ctlsizep);

    true
}

/// `pciide_unmapregs_native`.
pub fn pciide_unmapregs_native(sc: &PciideSoftc, cp: &PciideChannel) -> i32 {
    let wdc_cp = &cp.wdc_channel;

    bus_space_unmap(wdc_cp.cmd_iot(), wdc_cp.cmd_ioh(), wdc_cp.cmd_iosz.get());

    // Unmap the whole control space, not just the sub-region
    if let Some(h) = cp.ctl_baseioh.get() {
        bus_space_unmap(wdc_cp.ctl_iot(), h, wdc_cp.ctl_iosz.get());
    }

    if let Some(ih) = NonNull::new(sc.sc_pci_ih.replace(ptr::null_mut())) {
        // SAFETY: `sc_pci_ih` holds the PCI interrupt this driver established, dropped here.
        unsafe { pci_intr_disestablish(sc.pc(), ih) };
    }

    0
}

/// `pciide_mapreg_dma`.
pub fn pciide_mapreg_dma(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    // Map DMA registers
    //
    // Note that sc_dma_ok is the right variable to test to see if DMA can be done. If the
    // interface doesn't support DMA, sc_dma_ok will never be non-zero. If the DMA regs
    // couldn't be mapped, it'll be zero. I.e., sc_dma_ok will only be non-zero if the
    // interface supports DMA and the registers could be mapped.
    //
    // XXX Note that despite the fact that the Bus Master IDE specs say that "The bus master
    // IDE function uses 16 bytes of IO space", some controllers (at least the United
    // Microelectronics UM8886BF) place it in memory space.

    let maptype = pci_mapreg_type(pa.pa_pc, pa.pa_tag, PCIIDE_REG_BUS_MASTER_DMA);

    let map = match maptype {
        PCI_MAPREG_TYPE_IO => {
            match pci_mapreg_info(
                pa.pa_pc,
                pa.pa_tag,
                PCIIDE_REG_BUS_MASTER_DMA,
                PCI_MAPREG_TYPE_IO,
            ) {
                Err(_) => {
                    sc.sc_dma_ok.set(0);
                    printf(format_args!(", unused (couldn't query registers)"));
                    false
                }
                Ok((addr, _, _)) => {
                    sc.sc_dma_ok.set(1);
                    if sc.pp().ide_flags & IDE_16BIT_IOSPACE != 0 && addr >= 0x10000 {
                        sc.sc_dma_ok.set(0);
                        printf(format_args!(
                            ", unused (registers at unsafe address {:#x})",
                            addr
                        ));
                        false
                    } else {
                        // FALLTHROUGH
                        true
                    }
                }
            }
        }
        PCI_MAPREG_MEM_TYPE_32BIT => true,
        _ => {
            sc.sc_dma_ok.set(0);
            printf(format_args!(", (unsupported maptype 0x{:x})", maptype));
            false
        }
    };
    if !map {
        return;
    }

    let r = pci_mapreg_map(pa, PCIIDE_REG_BUS_MASTER_DMA, maptype, 0, 0);
    sc.sc_dmat.set(Some(pa.pa_dmat));
    match r {
        Err(_) => {
            sc.sc_dma_ok.set(0);
            printf(format_args!(", unused (couldn't map registers)"));
        }
        Ok((iot, ioh, _, size)) => {
            sc.sc_dma_ok.set(1);
            sc.sc_dma_iot.set(Some(iot));
            sc.sc_dma_ioh.set(Some(ioh));
            sc.sc_dma_iosz.set(size);
            sc.sc_wdcdev
                .dma_arg
                .set(ptr::from_ref(sc).cast_mut().cast());
            sc.sc_wdcdev.dma_init.set(Some(pciide_dma_init));
            sc.sc_wdcdev.dma_start.set(Some(pciide_dma_start));
            sc.sc_wdcdev.dma_finish.set(Some(pciide_dma_finish));
        }
    }
}

/// `pciide_unmapreg_dma`.
pub fn pciide_unmapreg_dma(sc: &PciideSoftc) {
    bus_space_unmap(sc.dma_iot(), sc.dma_ioh(), sc.sc_dma_iosz.get());
}

/// `pciide_intr_flag`: 1 if the channel's DMA engine raised the interrupt, 0 if it did not,
/// -1 when no DMA is in progress (the interrupt may be the channel's).
pub fn pciide_intr_flag(cp: &PciideChannel) -> i32 {
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let chan = cp.wdc_channel.channel.get();

    if cp.dma_in_progress.get() != 0 {
        // Check the status register
        let mut retry = 10;
        while retry > 0 {
            let status = sc.dmactl_read(chan);
            if status & IDEDMA_CTL_INTR != 0 {
                break;
            }
            delay(5);
            retry -= 1;
        }

        // Not for us.
        if retry == 0 {
            return 0;
        }

        return 1;
    }

    -1
}

/// `pciide_compat_intr`: the interrupt handler of a compatibility channel.
pub fn pciide_compat_intr(arg: *mut c_void) -> i32 {
    // SAFETY: `pciide_map_compat_intr` establishes this handler with its channel, which
    // lives in the softc.
    let cp = unsafe { &*arg.cast_const().cast::<PciideChannel>() };

    if pciide_intr_flag(cp) == 0 {
        return 0;
    }

    // should only be called for a compat channel
    #[cfg(feature = "diagnostic")]
    if cp.compat.get() == 0 {
        panic(format_args!(
            "pciide compat intr called for non-compat chan {:p}",
            cp
        ));
    }
    wdcintr(ptr::from_ref(&cp.wdc_channel).cast_mut().cast())
}

/// `pciide_pci_intr`: the native-PCI interrupt handler, for every native channel.
pub fn pciide_pci_intr(arg: *mut c_void) -> i32 {
    // SAFETY: `pciide_mapregs_native` establishes this handler with the softc.
    let sc = unsafe { &*arg.cast_const().cast::<PciideSoftc>() };

    let mut rv = 0;
    for i in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(i);
        let wdc_cp = &cp.wdc_channel;

        // If a compat channel skip.
        if cp.compat.get() != 0 {
            continue;
        }

        if cp.hw_ok.get() == 0 {
            continue;
        }

        if pciide_intr_flag(cp) == 0 {
            continue;
        }

        let crv = wdcintr(ptr::from_ref(wdc_cp).cast_mut().cast());
        if crv == 0 {
            // leave rv alone
        } else if crv == 1 {
            rv = 1; // claim the intr
        } else if rv == 0 {
            // crv should be -1 in this case; if we've done no better, take it
            rv = crv;
        }
    }
    rv
}

/// `pciide_dmacmd_read`.
pub fn pciide_dmacmd_read(sc: &PciideSoftc, chan: i32) -> u8 {
    bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), idedma_cmd(chan))
}

/// `pciide_dmacmd_write`.
pub fn pciide_dmacmd_write(sc: &PciideSoftc, chan: i32, val: u8) {
    bus_space_write_1(sc.dma_iot(), sc.dma_ioh(), idedma_cmd(chan), val);
}

/// `pciide_dmactl_read`.
pub fn pciide_dmactl_read(sc: &PciideSoftc, chan: i32) -> u8 {
    bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), idedma_ctl(chan))
}

/// `pciide_dmactl_write`.
pub fn pciide_dmactl_write(sc: &PciideSoftc, chan: i32, val: u8) {
    bus_space_write_1(sc.dma_iot(), sc.dma_ioh(), idedma_ctl(chan), val);
}

/// `pciide_dmatbl_write`.
pub fn pciide_dmatbl_write(sc: &PciideSoftc, chan: i32, val: u32) {
    bus_space_write_4(sc.dma_iot(), sc.dma_ioh(), idedma_tbl(chan), val);
}

/// `pciide_channel_dma_setup`.
pub fn pciide_channel_dma_setup(cp: &PciideChannel) {
    let sc = pciide_sc(cp.wdc_channel.wdc());

    for drive in 0..2 {
        let drvp = &cp.wdc_channel.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        // setup DMA if needed
        if (!drvp.isset(DRIVE_DMA) && !drvp.isset(DRIVE_UDMA)) || sc.sc_dma_ok.get() == 0 {
            drvp.clr(DRIVE_DMA | DRIVE_UDMA);
            continue;
        }
        if pciide_dma_table_setup(sc, cp.wdc_channel.channel.get(), drive).is_err() {
            // Abort DMA setup
            drvp.clr(DRIVE_DMA | DRIVE_UDMA);
            continue;
        }
    }
}

/// `pciide_dma_table_setup`: allocates and maps the descriptor table of `drive` and its
/// transfer map, once.
pub fn pciide_dma_table_setup(sc: &PciideSoftc, channel: i32, drive: i32) -> Result<(), Errno> {
    let dma_table_size: BusSize = size_of::<IdedmaTable>() * NIDEDMA_TABLES;
    let dma_maps = sc.channel(channel).dma_maps(drive);
    let dmat = sc.dmat();

    // If table was already allocated, just return
    if !dma_maps.dma_table.get().is_null() {
        return Ok(());
    }

    // Allocate memory for the DMA tables and map it
    let mut seg = [BusDmaSegment::default()];
    let rseg = match bus_dmamem_alloc(
        dmat,
        dma_table_size,
        IDEDMA_TBL_ALIGN,
        IDEDMA_TBL_ALIGN,
        &mut seg,
        BUS_DMA_NOWAIT,
    ) {
        Ok(rseg) => rseg,
        Err(error) => {
            printf(format_args!(
                "{}:{}: unable to allocate table DMA for drive {}, error={}\n",
                sc.xname(),
                channel,
                drive,
                error as i32
            ));
            return Err(error);
        }
    };

    match bus_dmamem_map(
        dmat,
        &mut seg[..rseg],
        dma_table_size,
        BUS_DMA_NOWAIT | BUS_DMA_COHERENT,
    ) {
        Ok(kva) => dma_maps.dma_table.set(kva.as_ptr().cast()),
        Err(error) => {
            printf(format_args!(
                "{}:{}: unable to map table DMA fordrive {}, error={}\n",
                sc.xname(),
                channel,
                drive,
                error as i32
            ));
            return Err(error);
        }
    }

    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "pciide_dma_table_setup: table at {:p} len {}, phy 0x{:x}\n",
        dma_maps.dma_table.get(),
        dma_table_size,
        seg[0].ds_addr
    );

    // Create and load table DMA map for this disk
    let table_map = match bus_dmamap_create(
        dmat,
        dma_table_size,
        1,
        dma_table_size,
        IDEDMA_TBL_ALIGN,
        BUS_DMA_NOWAIT,
    ) {
        Ok(map) => map,
        Err(error) => {
            printf(format_args!(
                "{}:{}: unable to create table DMA map for drive {}, error={}\n",
                sc.xname(),
                channel,
                drive,
                error as i32
            ));
            return Err(error);
        }
    };
    dma_maps.dmamap_table.set(Some(table_map));
    // SAFETY: the table was just mapped, `dma_table_size` bytes, and is never freed while
    // the controller exists.
    if let Err(error) = unsafe {
        bus_dmamap_load(
            dmat,
            table_map,
            dma_maps.dma_table.get().cast(),
            dma_table_size,
            None,
            BUS_DMA_NOWAIT,
        )
    } {
        printf(format_args!(
            "{}:{}: unable to load table DMA map for drive {}, error={}\n",
            sc.xname(),
            channel,
            drive,
            error as i32
        ));
        return Err(error);
    }
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "pciide_dma_table_setup: phy addr of table 0x{:x}\n",
        table_map.dm_segs()[0].get().ds_addr
    );
    // Create a xfer DMA map for this drive
    match bus_dmamap_create(
        dmat,
        IDEDMA_BYTE_COUNT_MAX,
        NIDEDMA_TABLES as i32,
        sc.sc_dma_maxsegsz.get(),
        sc.sc_dma_boundary.get(),
        BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW,
    ) {
        Ok(map) => dma_maps.dmamap_xfer.set(Some(map)),
        Err(error) => {
            printf(format_args!(
                "{}:{}: unable to create xfer DMA map for drive {}, error={}\n",
                sc.xname(),
                channel,
                drive,
                error as i32
            ));
            return Err(error);
        }
    }
    Ok(())
}

/// `BUS_DMA_RAW`: no EmiBSD machine defines it, so the C's fallback.
const BUS_DMA_RAW: i32 = 0;

/// `dma_maps->dmamap_table` and `dmamap_xfer`, which `pciide_dma_table_setup` created
/// before the drive was marked DMA-capable.
fn pciide_dma_maps(
    sc: &PciideSoftc,
    dma_maps: &PciideDmaMaps,
) -> (&'static BusDmamap, &'static BusDmamap) {
    match (dma_maps.dmamap_table.get(), dma_maps.dmamap_xfer.get()) {
        (Some(t), Some(x)) => (t, x),
        _ => panic(format_args!("{}: DMA maps not set up", sc.xname())),
    }
}

/// `pciide_dma_init`: loads the transfer map with `databuf` and fills the descriptor
/// table; `Err` (the load's error) makes the caller fall back to PIO.
///
/// # Safety
///
/// `v` is the softc `pciide_mapreg_dma` installed as `dma_arg`; `databuf` points to
/// `datalen` bytes that stay valid until `pciide_dma_finish` returns.
pub unsafe fn pciide_dma_init(
    v: *mut c_void,
    channel: i32,
    drive: i32,
    databuf: *mut u8,
    datalen: usize,
    flags: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*v.cast_const().cast::<PciideSoftc>() };
    let cp = sc.channel(channel);
    let dma_maps = cp.dma_maps(drive);
    let (dmamap_table, dmamap_xfer) = pciide_dma_maps(sc, dma_maps);
    let dmat = sc.dmat();

    // SAFETY: the caller's guarantee on `databuf`.
    if let Err(error) = unsafe {
        bus_dmamap_load(
            dmat,
            dmamap_xfer,
            databuf,
            datalen,
            None,
            BUS_DMA_NOWAIT | BUS_DMA_RAW,
        )
    } {
        printf(format_args!(
            "{}:{}: unable to load xfer DMA map for drive {}, error={}\n",
            sc.xname(),
            channel,
            drive,
            error as i32
        ));
        return Err(error);
    }

    bus_dmamap_sync(
        dmat,
        dmamap_xfer,
        0,
        dmamap_xfer.dm_mapsize.get(),
        if flags & WDC_DMA_READ != 0 {
            BUS_DMASYNC_PREREAD
        } else {
            BUS_DMASYNC_PREWRITE
        },
    );

    let table = dma_maps.dma_table.get();
    let nsegs = dmamap_xfer.dm_nsegs.get() as usize;
    for (seg, s) in dmamap_xfer.dm_segs()[..nsegs].iter().enumerate() {
        let s = s.get();
        // A segment must not cross a 64k boundary
        #[cfg(feature = "diagnostic")]
        {
            let phys = s.ds_addr as u64;
            let len = s.ds_len as u64;
            let mask = !u64::from(IDEDMA_BYTE_COUNT_MASK);
            if (phys & mask) != ((phys + len - 1) & mask) {
                printf(format_args!(
                    "pciide_dma: segment {} physical addr 0x{:x} len 0x{:x} not properly aligned\n",
                    seg, phys, len
                ));
                panic(format_args!("pciide_dma: buf align"));
            }
        }
        let entry = IdedmaTable {
            base_addr: (s.ds_addr as u32).to_le(),
            byte_count: (s.ds_len as u32 & IDEDMA_BYTE_COUNT_MASK).to_le(),
        };
        // SAFETY: the table holds NIDEDMA_TABLES descriptors and the transfer map was
        // created with at most that many segments.
        unsafe { table.add(seg).write(entry) };
        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_DMA,
            "\t seg {} len {} addr 0x{:x}\n",
            seg,
            u32::from_le(entry.byte_count),
            u32::from_le(entry.base_addr)
        );
    }
    if nsegs > 0 {
        // SAFETY: as above; the last descriptor written.
        unsafe {
            let last = table.add(nsegs - 1);
            (*last).byte_count |= IDEDMA_BYTE_COUNT_EOT.to_le();
        }
    }

    bus_dmamap_sync(
        dmat,
        dmamap_table,
        0,
        dmamap_table.dm_mapsize.get(),
        BUS_DMASYNC_PREWRITE,
    );

    let table_addr = dmamap_table.dm_segs()[0].get().ds_addr;
    // Maps are ready. Start DMA function
    #[cfg(feature = "diagnostic")]
    if table_addr as u64 & !u64::from(IDEDMA_TBL_MASK) != 0 {
        printf(format_args!(
            "pciide_dma_init: addr 0x{:x} not properly aligned\n",
            table_addr
        ));
        panic(format_args!("pciide_dma_init: table align"));
    }

    // Clear status bits
    sc.dmactl_write(channel, sc.dmactl_read(channel));
    // Write table addr
    sc.dmatbl_write(channel, table_addr as u32);
    // set read/write
    sc.dmacmd_write(
        channel,
        (if flags & WDC_DMA_READ != 0 {
            IDEDMA_CMD_WRITE
        } else {
            0
        }) | cp.idedma_cmd.get(),
    );
    // remember flags
    dma_maps.dma_flags.set(flags);
    Ok(())
}

/// `pciide_dma_start`.
///
/// # Safety
///
/// `v` is the softc `pciide_mapreg_dma` installed as `dma_arg`.
pub unsafe fn pciide_dma_start(v: *mut c_void, channel: i32, _drive: i32) {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*v.cast_const().cast::<PciideSoftc>() };

    wdcdebug_print!(wdcdebug_pciide_mask, DEBUG_XFERS, "pciide_dma_start\n");
    sc.dmacmd_write(channel, sc.dmacmd_read(channel) | IDEDMA_CMD_START);

    sc.channel(channel).dma_in_progress.set(1);
}

/// `pciide_dma_finish`: stops the engine and returns the `WDC_DMAST_*` status (0xff for a
/// controller that went away).
///
/// # Safety
///
/// `v` is the softc `pciide_mapreg_dma` installed as `dma_arg`.
pub unsafe fn pciide_dma_finish(v: *mut c_void, channel: i32, drive: i32, force: i32) -> i32 {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*v.cast_const().cast::<PciideSoftc>() };
    let cp = sc.channel(channel);
    let dma_maps = cp.dma_maps(drive);
    let mut error = 0;

    let status = sc.dmactl_read(channel);
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_XFERS,
        "pciide_dma_finish: status 0x{:x}\n",
        status
    );
    if status == 0xff {
        return i32::from(status);
    }

    'done: {
        if force == 0 && status & IDEDMA_CTL_INTR == 0 {
            error = WDC_DMAST_NOIRQ;
            break 'done;
        }

        let (_, dmamap_xfer) = pciide_dma_maps(sc, dma_maps);
        let read = dma_maps.dma_flags.get() & WDC_DMA_READ != 0;

        // stop DMA channel
        sc.dmacmd_write(
            channel,
            (if read { 0x00 } else { IDEDMA_CMD_WRITE }) | cp.idedma_cmd.get(),
        );

        // Unload the map of the data buffer
        bus_dmamap_sync(
            sc.dmat(),
            dmamap_xfer,
            0,
            dmamap_xfer.dm_mapsize.get(),
            if read {
                BUS_DMASYNC_POSTREAD
            } else {
                BUS_DMASYNC_POSTWRITE
            },
        );
        bus_dmamap_unload(sc.dmat(), dmamap_xfer);

        // Clear status bits
        sc.dmactl_write(channel, status);

        if status & IDEDMA_CTL_ERR != 0 {
            printf(format_args!(
                "{}:{}:{}: bus-master DMA error: status=0x{:x}\n",
                sc.xname(),
                channel,
                drive,
                status
            ));
            error |= WDC_DMAST_ERR;
        }

        if status & IDEDMA_CTL_INTR == 0 {
            printf(format_args!(
                "{}:{}:{}: bus-master DMA error: missing interrupt, status=0x{:x}\n",
                sc.xname(),
                channel,
                drive,
                status
            ));
            error |= WDC_DMAST_NOIRQ;
        }

        if status & IDEDMA_CTL_ACT != 0 {
            // data underrun, may be a valid condition for ATAPI
            error |= WDC_DMAST_UNDER;
        }
    }

    // done:
    cp.dma_in_progress.set(0);
    error
}

/// `pciide_irqack`: clear status bits in IDE DMA registers.
pub fn pciide_irqack(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let chan = chp.channel.get();

    // clear status bits in IDE DMA registers
    sc.dmactl_write(chan, sc.dmactl_read(chan));
}

/// `pciide_chansetup`: some common code used by several chip_map.
pub fn pciide_chansetup(sc: &'static PciideSoftc, channel: i32, _interface: Pcireg) -> bool {
    let cp = sc.channel(channel);
    sc.wdc_chanarray[channel as usize].set(Some(NonNull::from(&cp.wdc_channel)));
    cp.name.set(Some(pciide_channel_name(channel)));
    cp.wdc_channel.channel.set(channel);
    cp.wdc_channel.wdc.set(Some(NonNull::from(&sc.sc_wdcdev)));
    cp.wdc_channel.ch_queue.set(wdc_alloc_queue());
    if cp.wdc_channel.ch_queue.get().is_none() {
        printf(format_args!(
            "{}: {} cannot allocate channel queue",
            sc.xname(),
            cp.name()
        ));
        return false;
    }
    cp.hw_ok.set(1);

    true
}

/// `pciide_chanfree`.
pub fn pciide_chanfree(sc: &PciideSoftc, channel: i32) {
    let cp = sc.channel(channel);
    if let Some(q) = cp.wdc_channel.ch_queue.get() {
        // SAFETY: the queue `pciide_chansetup` allocated; its channel is detached.
        unsafe { wdc_free_queue(q) };
    }
}

/// `pciide_mapchan`: some common code used by several chip channel_map.
pub fn pciide_mapchan(
    pa: &PciAttachArgs,
    cp: &'static PciideChannel,
    interface: Pcireg,
    cmdsizep: &mut BusSize,
    ctlsizep: &mut BusSize,
    pci_intr: fn(*mut c_void) -> i32,
) {
    let wdc_cp = &cp.wdc_channel;

    let ok = if interface & pciide_interface_pci(wdc_cp.channel.get()) != 0 {
        pciide_mapregs_native(pa, cp, cmdsizep, ctlsizep, pci_intr)
    } else {
        pciide_mapregs_compat(pa, cp, wdc_cp.channel.get(), cmdsizep, ctlsizep)
    };
    cp.hw_ok.set(i32::from(ok));
    if cp.hw_ok.get() == 0 {
        return;
    }
    wdc_cp.data32iot.set(wdc_cp.cmd_iot.get());
    wdc_cp.data32ioh.set(wdc_cp.cmd_ioh.get());
    wdcattach(wdc_cp);
}

/// `pciide_unmap_chan`.
pub fn pciide_unmap_chan(sc: &PciideSoftc, cp: &PciideChannel, flags: i32) {
    let wdc_cp = &cp.wdc_channel;

    let _ = wdcdetach(wdc_cp, flags);

    if cp.compat.get() != 0 {
        pciide_unmapregs_compat(sc, cp);
    } else {
        pciide_unmapregs_native(sc, cp);
    }
}

/// `pciide_chan_candisable`: generic code to call to know if a channel can be disabled.
/// True if channel can be disabled, false if not.
pub fn pciide_chan_candisable(cp: &PciideChannel) -> bool {
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let wdc_cp = &cp.wdc_channel;

    if !wdc_cp.ch_drive[0].isset(DRIVE) && !wdc_cp.ch_drive[1].isset(DRIVE) {
        printf(format_args!(
            "{}: {} disabled (no drives)\n",
            sc.xname(),
            cp.name()
        ));
        cp.hw_ok.set(0);
        return true;
    }
    false
}

/// `pciide_map_compat_intr`: generic code to map the compat intr if hw_ok=1 and it is a
/// compat channel. Set hw_ok=0 on failure.
pub fn pciide_map_compat_intr(
    pa: &PciAttachArgs,
    cp: &'static PciideChannel,
    compatchan: i32,
    interface: Pcireg,
) {
    let sc: &'static PciideSoftc = pciide_sc_static(cp);
    let wdc_cp = &cp.wdc_channel;

    if interface & pciide_interface_pci(wdc_cp.channel.get()) != 0 {
        return;
    }

    cp.compat.set(1);
    let ih = pciide_machdep_compat_intr_establish(
        &sc.sc_wdcdev.sc_dev,
        pa,
        compatchan,
        pciide_compat_intr,
        ptr::from_ref(cp).cast_mut().cast(),
    );
    cp.ih.set(ih.map_or(ptr::null_mut(), |ih| ih.as_ptr()));
    if ih.is_none() {
        printf(format_args!(
            "{}: no compatibility interrupt for use by {}\n",
            sc.xname(),
            cp.name()
        ));
        cp.hw_ok.set(0);
    }
}

/// `pciide_sc` for a channel borrowed for 'static.
fn pciide_sc_static(cp: &'static PciideChannel) -> &'static PciideSoftc {
    pciide_sc(cp.wdc_channel.wdc())
}

/// `pciide_unmap_compat_intr`: generic code to unmap the compat intr if hw_ok=1 and it is a
/// compat channel.
pub fn pciide_unmap_compat_intr(
    pa: &PciAttachArgs,
    cp: &PciideChannel,
    _compatchan: i32,
    interface: Pcireg,
) {
    let wdc_cp = &cp.wdc_channel;

    if interface & pciide_interface_pci(wdc_cp.channel.get()) != 0 {
        return;
    }

    if let Some(ih) = NonNull::new(cp.ih.get()) {
        // SAFETY: the handle `pciide_map_compat_intr` established for this channel.
        unsafe { pciide_machdep_compat_intr_disestablish(pa.pa_pc, ih) };
    }
}

/// `pciide_print_channels`.
pub fn pciide_print_channels(nchannels: i32, interface: Pcireg) {
    for i in 0..nchannels {
        printf(format_args!(
            ", {} {} to {}",
            pciide_channel_name(i),
            if interface & pciide_interface_settable(i) != 0 {
                "configured"
            } else {
                "wired"
            },
            if interface & pciide_interface_pci(i) != 0 {
                "native-PCI"
            } else {
                "compatibility"
            }
        ));
    }

    printf(format_args!("\n"));
}

/// `pciide_print_modes`.
pub fn pciide_print_modes(cp: &PciideChannel) {
    wdc_print_current_modes(&cp.wdc_channel);
}

/// `default_chip_map`.
pub fn default_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let interface = pci_interface(pa.pa_class);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    if interface & PCIIDE_INTERFACE_BUS_MASTER_DMA != 0 {
        printf(format_args!(": DMA"));
        if ptr::eq(sc.pp(), &DEFAULT_PRODUCT_DESC)
            && sc.sc_wdcdev.sc_dev.cfdata().cf_flags & PCIIDE_OPTIONS_DMA == 0
        {
            printf(format_args!(" (unsupported)"));
            sc.sc_dma_ok.set(0);
        } else {
            pciide_mapreg_dma(sc, pa);
            if sc.sc_dma_ok.get() != 0 {
                printf(format_args!(", (partial support)"));
            }
        }
    } else {
        printf(format_args!(": no DMA"));
        sc.sc_dma_ok.set(0);
    }
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(0);
    sc.sc_wdcdev.DMA_cap.set(0);
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);
    wdc_cap_set(sc, WDC_CAPABILITY_DATA16);

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }
        let ok = if interface & pciide_interface_pci(channel) != 0 {
            pciide_mapregs_native(pa, cp, &mut cmdsize, &mut ctlsize, pciide_pci_intr)
        } else {
            pciide_mapregs_compat(pa, cp, channel, &mut cmdsize, &mut ctlsize)
        };
        cp.hw_ok.set(i32::from(ok));
        if cp.hw_ok.get() == 0 {
            continue;
        }
        // Check to see if something appears to be there.
        let mut failreason = None;
        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        'next: {
            if wdcprobe(&cp.wdc_channel) == 0 {
                failreason = Some("not responding; disabled or no drives?");
                break 'next;
            }
            // Now, make sure it's actually attributable to this PCI IDE channel by trying
            // to access the channel again while the PCI IDE controller's I/O space is
            // disabled. (If the channel no longer appears to be there, it belongs to this
            // controller.) YUCK!
            let csr = pci_conf_read(sc.pc(), sc.tag(), PCI_COMMAND_STATUS_REG);
            pci_conf_write(
                sc.pc(),
                sc.tag(),
                PCI_COMMAND_STATUS_REG,
                csr & !PCI_COMMAND_IO_ENABLE,
            );
            if wdcprobe(&cp.wdc_channel) != 0 {
                failreason = Some("other hardware responding at addresses");
            }
            pci_conf_write(sc.pc(), sc.tag(), PCI_COMMAND_STATUS_REG, csr);
        }
        // next:
        if let Some(failreason) = failreason {
            printf(format_args!(
                "{}: {} ignored ({})\n",
                sc.xname(),
                cp.name(),
                failreason
            ));
            cp.hw_ok.set(0);
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            bus_space_unmap(cp.wdc_channel.cmd_iot(), cp.wdc_channel.cmd_ioh(), cmdsize);
            if interface & pciide_interface_pci(channel) != 0 {
                if let Some(h) = cp.ctl_baseioh.get() {
                    bus_space_unmap(cp.wdc_channel.ctl_iot(), h, ctlsize);
                }
            } else {
                bus_space_unmap(cp.wdc_channel.ctl_iot(), cp.wdc_channel.ctl_ioh(), ctlsize);
            }
        }
        if cp.hw_ok.get() != 0 {
            cp.wdc_channel.data32iot.set(cp.wdc_channel.cmd_iot.get());
            cp.wdc_channel.data32ioh.set(cp.wdc_channel.cmd_ioh.get());
            wdcattach(&cp.wdc_channel);
        }
    }

    if sc.sc_dma_ok.get() == 0 {
        return;
    }

    // Allocate DMA maps
    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let mut idedma_ctl: u8 = 0;
        let cp = sc.channel(channel);
        for drive in 0..2 {
            let drvp = &cp.wdc_channel.ch_drive[drive as usize];
            // If no drive, skip
            if !drvp.isset(DRIVE) {
                continue;
            }
            if !drvp.isset(DRIVE_DMA) {
                continue;
            }
            if pciide_dma_table_setup(sc, channel, drive).is_err() {
                // Abort DMA setup
                printf(format_args!(
                    "{}:{}:{}: cannot allocate DMA maps, using PIO transfers\n",
                    sc.xname(),
                    channel,
                    drive
                ));
                drvp.clr(DRIVE_DMA);
            }
            printf(format_args!(
                "{}:{}:{}: using DMA data transfers\n",
                sc.xname(),
                channel,
                drive
            ));
            idedma_ctl |= idedma_ctl_drv_dma(drive);
        }
        if idedma_ctl != 0 {
            // Add software bits in status register
            sc.dmactl_write(channel, idedma_ctl);
        }
    }
}

/// `sc->sc_wdcdev.cap |= f`.
fn wdc_cap_set(sc: &PciideSoftc, f: i32) {
    sc.sc_wdcdev.cap.set(sc.sc_wdcdev.cap.get() | f);
}

/// `default_chip_unmap`.
pub fn default_chip_unmap(sc: &PciideSoftc, flags: i32) {
    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        pciide_unmap_chan(sc, cp, flags);
        pciide_chanfree(sc, channel);
    }

    pciide_unmapreg_dma(sc);

    if let Some(cookie) = NonNull::new(sc.sc_cookie.get().cast::<u8>()) {
        free(cookie, M_DEVBUF, sc.sc_cookielen.get());
    }
}

/// `sata_chip_map`.
pub fn sata_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let mut interface = pci_interface(pa.pa_class);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    if interface == 0 {
        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_PROBE,
            "sata_chip_map interface == 0\n"
        );
        interface =
            PCIIDE_INTERFACE_BUS_MASTER_DMA | pciide_interface_pci(0) | pciide_interface_pci(1);
    }

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);
    printf(format_args!("\n"));

    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(
            sc,
            WDC_CAPABILITY_UDMA | WDC_CAPABILITY_DMA | WDC_CAPABILITY_IRQACK,
        );
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.UDMA_cap.set(6);

    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);
    wdc_cap_set(
        sc,
        WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE | WDC_CAPABILITY_SATA,
    );
    sc.sc_wdcdev.set_modes.set(Some(sata_setup_channel));
    sc.chip_unmap.set(Some(default_chip_unmap));

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }
        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            pciide_pci_intr,
        );
        sata_setup_channel(&cp.wdc_channel);
    }
}

/// `sata_setup_channel`.
pub fn sata_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());

    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    let mut idedma_ctl: u32 = 0;

    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        if drvp.isset(DRIVE_UDMA) {
            // use Ultra/DMA
            drvp.clr(DRIVE_DMA);
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        } else if drvp.isset(DRIVE_DMA) {
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        }
    }

    // Nothing to do to setup modes; it is meaningless in S-ATA (but many S-ATA drives still
    // want to get the SET_FEATURE command).
    if idedma_ctl != 0 {
        // Add software bits in status register
        sc.dmactl_write(chp.channel.get(), idedma_ctl as u8);
    }
    pciide_print_modes(cp);
}

/// `piix_timing_debug`.
pub fn piix_timing_debug(sc: &PciideSoftc) {
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "piix_setup_chip: idetim=0x{:x}",
        pci_conf_read(sc.pc(), sc.tag(), PIIX_IDETIM)
    );
    let prod = sc.pp().ide_product;
    if prod != PCI_PRODUCT_INTEL_82371FB_IDE && prod != PCI_PRODUCT_INTEL_82371FB_ISA {
        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_PROBE,
            ", sidetim=0x{:x}",
            pci_conf_read(sc.pc(), sc.tag(), PIIX_SIDETIM)
        );
        if sc.sc_wdcdev.has(WDC_CAPABILITY_UDMA) {
            wdcdebug_print!(
                wdcdebug_pciide_mask,
                DEBUG_PROBE,
                ", udmareg 0x{:x}",
                pci_conf_read(sc.pc(), sc.tag(), PIIX_UDMAREG)
            );
        }
        if matches!(
            prod,
            PCI_PRODUCT_INTEL_6300ESB_IDE
                | PCI_PRODUCT_INTEL_6321ESB_IDE
                | PCI_PRODUCT_INTEL_82801AA_IDE
                | PCI_PRODUCT_INTEL_82801AB_IDE
                | PCI_PRODUCT_INTEL_82801BAM_IDE
                | PCI_PRODUCT_INTEL_82801BA_IDE
                | PCI_PRODUCT_INTEL_82801CAM_IDE
                | PCI_PRODUCT_INTEL_82801CA_IDE
                | PCI_PRODUCT_INTEL_82801DB_IDE
                | PCI_PRODUCT_INTEL_82801DBL_IDE
                | PCI_PRODUCT_INTEL_82801DBM_IDE
                | PCI_PRODUCT_INTEL_82801EB_IDE
                | PCI_PRODUCT_INTEL_82801FB_IDE
                | PCI_PRODUCT_INTEL_82801GB_IDE
                | PCI_PRODUCT_INTEL_82801HBM_IDE
                | PCI_PRODUCT_INTEL_82372FB_IDE
        ) {
            wdcdebug_print!(
                wdcdebug_pciide_mask,
                DEBUG_PROBE,
                ", IDE_CONTROL 0x{:x}",
                pci_conf_read(sc.pc(), sc.tag(), PIIX_CONFIG)
            );
        }
    }
    wdcdebug_print!(wdcdebug_pciide_mask, DEBUG_PROBE, "\n");
}

/// `piix_chip_map`.
pub fn piix_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    let interface = pci_interface(pa.pa_class);

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);
    wdc_cap_set(
        sc,
        WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE,
    );
    let prod = sc.pp().ide_product;
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
        if matches!(
            prod,
            PCI_PRODUCT_INTEL_6300ESB_IDE
                | PCI_PRODUCT_INTEL_6321ESB_IDE
                | PCI_PRODUCT_INTEL_82371AB_IDE
                | PCI_PRODUCT_INTEL_82372FB_IDE
                | PCI_PRODUCT_INTEL_82440MX_IDE
                | PCI_PRODUCT_INTEL_82451NX
                | PCI_PRODUCT_INTEL_82801AA_IDE
                | PCI_PRODUCT_INTEL_82801AB_IDE
                | PCI_PRODUCT_INTEL_82801BAM_IDE
                | PCI_PRODUCT_INTEL_82801BA_IDE
                | PCI_PRODUCT_INTEL_82801CAM_IDE
                | PCI_PRODUCT_INTEL_82801CA_IDE
                | PCI_PRODUCT_INTEL_82801DB_IDE
                | PCI_PRODUCT_INTEL_82801DBL_IDE
                | PCI_PRODUCT_INTEL_82801DBM_IDE
                | PCI_PRODUCT_INTEL_82801EB_IDE
                | PCI_PRODUCT_INTEL_82801FB_IDE
                | PCI_PRODUCT_INTEL_82801GB_IDE
                | PCI_PRODUCT_INTEL_82801HBM_IDE
        ) {
            wdc_cap_set(sc, WDC_CAPABILITY_UDMA);
        }
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.UDMA_cap.set(match prod {
        PCI_PRODUCT_INTEL_82801AA_IDE | PCI_PRODUCT_INTEL_82372FB_IDE => 4,
        PCI_PRODUCT_INTEL_6300ESB_IDE
        | PCI_PRODUCT_INTEL_6321ESB_IDE
        | PCI_PRODUCT_INTEL_82801BAM_IDE
        | PCI_PRODUCT_INTEL_82801BA_IDE
        | PCI_PRODUCT_INTEL_82801CAM_IDE
        | PCI_PRODUCT_INTEL_82801CA_IDE
        | PCI_PRODUCT_INTEL_82801DB_IDE
        | PCI_PRODUCT_INTEL_82801DBL_IDE
        | PCI_PRODUCT_INTEL_82801DBM_IDE
        | PCI_PRODUCT_INTEL_82801EB_IDE
        | PCI_PRODUCT_INTEL_82801FB_IDE
        | PCI_PRODUCT_INTEL_82801GB_IDE
        | PCI_PRODUCT_INTEL_82801HBM_IDE => 5,
        _ => 2,
    });

    if prod == PCI_PRODUCT_INTEL_82371FB_IDE || prod == PCI_PRODUCT_INTEL_82371FB_ISA {
        sc.sc_wdcdev.set_modes.set(Some(piix_setup_channel));
    } else {
        sc.sc_wdcdev.set_modes.set(Some(piix3_4_setup_channel));
    }
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    piix_timing_debug(sc);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);

        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }
        let mut idetim = pci_conf_read(sc.pc(), sc.tag(), PIIX_IDETIM);
        if piix_idetim_read(idetim, channel) & PIIX_IDETIM_IDE == 0 {
            printf(format_args!(
                "{}: {} ignored (disabled)\n",
                sc.xname(),
                cp.name()
            ));
            cp.hw_ok.set(0);
            continue;
        }
        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        'next: {
            pciide_mapchan(
                pa,
                cp,
                interface,
                &mut cmdsize,
                &mut ctlsize,
                pciide_pci_intr,
            );
            if cp.hw_ok.get() == 0 {
                break 'next;
            }
            if pciide_chan_candisable(cp) {
                idetim = piix_idetim_clear(idetim, PIIX_IDETIM_IDE, channel);
                pci_conf_write(sc.pc(), sc.tag(), PIIX_IDETIM, idetim);
            }
            if cp.hw_ok.get() == 0 {
                break 'next;
            }
            if let Some(set_modes) = sc.sc_wdcdev.set_modes.get() {
                set_modes(&cp.wdc_channel);
            }
        }
        // next:
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
        }
    }

    piix_timing_debug(sc);
}

/// `piixsata_chip_map`.
pub fn piixsata_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let mut interface = pci_interface(pa.pa_class);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);

    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(
            sc,
            WDC_CAPABILITY_UDMA | WDC_CAPABILITY_DMA | WDC_CAPABILITY_IRQACK,
        );
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
        sc.sc_wdcdev.DMA_cap.set(2);
        sc.sc_wdcdev.UDMA_cap.set(6);
    }
    sc.sc_wdcdev.PIO_cap.set(4);

    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);
    wdc_cap_set(
        sc,
        WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE | WDC_CAPABILITY_SATA,
    );
    sc.sc_wdcdev.set_modes.set(Some(sata_setup_channel));

    let ich: u8 = match sc.pp().ide_product {
        PCI_PRODUCT_INTEL_6300ESB_SATA
        | PCI_PRODUCT_INTEL_6300ESB_SATA2
        | PCI_PRODUCT_INTEL_82801EB_SATA
        | PCI_PRODUCT_INTEL_82801ER_SATA => 5,
        PCI_PRODUCT_INTEL_82801FB_SATA
        | PCI_PRODUCT_INTEL_82801FR_SATA
        | PCI_PRODUCT_INTEL_82801FBM_SATA => 6,
        _ => 7,
    };

    // Put the SATA portion of controllers that don't operate in combined mode into native
    // PCI modes so the maximum number of devices can be used. Intel calls this "enhanced
    // mode"
    if ich == 5 {
        let mut reg = pciide_pci_read(sc.pc(), sc.tag(), ICH5_SATA_MAP);
        if u32::from(reg) & ICH5_SATA_MAP_COMBINED == 0 {
            reg = pciide_pci_read(pa.pa_pc, pa.pa_tag, ICH5_SATA_PI);
            reg |= (ICH5_SATA_PI_PRI_NATIVE | ICH5_SATA_PI_SEC_NATIVE) as u8;
            pciide_pci_write(pa.pa_pc, pa.pa_tag, ICH5_SATA_PI, reg);
            interface |= pciide_interface_pci(0) | pciide_interface_pci(1);
        }
    } else {
        let mut reg =
            pciide_pci_read(sc.pc(), sc.tag(), ICH5_SATA_MAP) & ICH6_SATA_MAP_CMB_MASK as u8;
        if u32::from(reg) != ICH6_SATA_MAP_CMB_PRI && u32::from(reg) != ICH6_SATA_MAP_CMB_SEC {
            reg = pciide_pci_read(pa.pa_pc, pa.pa_tag, ICH5_SATA_PI);
            reg |= (ICH5_SATA_PI_PRI_NATIVE | ICH5_SATA_PI_SEC_NATIVE) as u8;

            pciide_pci_write(pa.pa_pc, pa.pa_tag, ICH5_SATA_PI, reg);
            interface |= pciide_interface_pci(0) | pciide_interface_pci(1);

            // Ask for SATA IDE Mode, we don't need to do this for the combined mode case
            // as combined mode is only allowed in IDE Mode
            if ich >= 7 {
                reg = pciide_pci_read(sc.pc(), sc.tag(), ICH5_SATA_MAP)
                    & !(ICH7_SATA_MAP_SMS_MASK as u8);
                pciide_pci_write(pa.pa_pc, pa.pa_tag, ICH5_SATA_MAP, reg);
            }
        }
    }

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }

        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }

        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            pciide_pci_intr,
        );
        if cp.hw_ok.get() != 0
            && let Some(set_modes) = sc.sc_wdcdev.set_modes.get()
        {
            set_modes(&cp.wdc_channel);
        }

        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
        }
    }
}

/// `piix_setup_channel` (PIIX: one timing for both drives of a channel).
pub fn piix_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let drvp = &cp.wdc_channel.ch_drive;
    let channel = chp.channel.get();
    let mut mode = [0u8; 2];

    let oidetim = pci_conf_read(sc.pc(), sc.tag(), PIIX_IDETIM);
    let mut idetim = piix_idetim_clear(oidetim, 0xffff, channel);
    let mut idedma_ctl: u32 = 0;

    // set up new idetim: Enable IDE registers decode
    idetim = piix_idetim_set(idetim, PIIX_IDETIM_IDE, channel);

    // setup DMA
    pciide_channel_dma_setup(cp);

    // Here we have to mess up with drives mode: PIIX can't have different timings for
    // master and slave drives. We need to find the best combination.
    'ok: {
        // If both drives supports DMA, take the lower mode
        if drvp[0].isset(DRIVE_DMA) && drvp[1].isset(DRIVE_DMA) {
            let m = min(drvp[0].DMA_mode.get(), drvp[1].DMA_mode.get());
            mode = [m, m];
            drvp[0].DMA_mode.set(mode[0]);
            drvp[1].DMA_mode.set(mode[1]);
            break 'ok;
        }
        // If only one drive supports DMA, use its mode, and put the other one in PIO mode 0
        // if mode not compatible
        if drvp[0].isset(DRIVE_DMA) {
            mode[0] = drvp[0].DMA_mode.get();
            mode[1] = drvp[1].PIO_mode.get();
            if PIIX_ISP_PIO[mode[1] as usize] != PIIX_ISP_DMA[mode[0] as usize]
                || PIIX_RTC_PIO[mode[1] as usize] != PIIX_RTC_DMA[mode[0] as usize]
            {
                mode[1] = 0;
                drvp[1].PIO_mode.set(0);
            }
            break 'ok;
        }
        if drvp[1].isset(DRIVE_DMA) {
            mode[1] = drvp[1].DMA_mode.get();
            mode[0] = drvp[0].PIO_mode.get();
            if PIIX_ISP_PIO[mode[0] as usize] != PIIX_ISP_DMA[mode[1] as usize]
                || PIIX_RTC_PIO[mode[0] as usize] != PIIX_RTC_DMA[mode[1] as usize]
            {
                mode[0] = 0;
                drvp[0].PIO_mode.set(0);
            }
            break 'ok;
        }
        // If both drives are not DMA, takes the lower mode, unless one of them is PIO mode
        // < 2
        if drvp[0].PIO_mode.get() < 2 {
            mode[0] = 0;
            drvp[0].PIO_mode.set(0);
            mode[1] = drvp[1].PIO_mode.get();
        } else if drvp[1].PIO_mode.get() < 2 {
            mode[1] = 0;
            drvp[1].PIO_mode.set(0);
            mode[0] = drvp[0].PIO_mode.get();
        } else {
            let m = min(drvp[1].PIO_mode.get(), drvp[0].PIO_mode.get());
            mode = [m, m];
            drvp[0].PIO_mode.set(mode[0]);
            drvp[1].PIO_mode.set(mode[1]);
        }
    }
    // ok: The modes are setup
    'end: {
        for drive in 0..2 {
            if drvp[drive].isset(DRIVE_DMA) {
                idetim |= piix_setup_idetim_timings(mode[drive], 1, channel);
                break 'end;
            }
        }
        // If we are there, none of the drives are DMA
        if mode[0] >= 2 {
            idetim |= piix_setup_idetim_timings(mode[0], 0, channel);
        } else {
            idetim |= piix_setup_idetim_timings(mode[1], 0, channel);
        }
    }
    // end: timing mode is now set up in the controller. Enable it per-drive
    for (drive, d) in drvp.iter().enumerate() {
        // If no drive, skip
        if !d.isset(DRIVE) {
            continue;
        }
        idetim |= piix_setup_idetim_drvs(d);
        if d.isset(DRIVE_DMA) {
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive as i32));
        }
    }
    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }
    pci_conf_write(sc.pc(), sc.tag(), PIIX_IDETIM, idetim);
    pciide_print_modes(cp);
}

/// `piix3_4_setup_channel` (PIIX3, PIIX4 and the ICHs: slave timings, UDMA).
pub fn piix3_4_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    let prod = sc.pp().ide_product;

    let oidetim = pci_conf_read(sc.pc(), sc.tag(), PIIX_IDETIM);
    let mut sidetim = pci_conf_read(sc.pc(), sc.tag(), PIIX_SIDETIM);
    let mut udmareg = pci_conf_read(sc.pc(), sc.tag(), PIIX_UDMAREG);
    let mut ideconf = pci_conf_read(sc.pc(), sc.tag(), PIIX_CONFIG);
    let mut idetim = piix_idetim_clear(oidetim, 0xffff, channel);
    sidetim &= !(piix_sidetim_isp_mask(channel) | piix_sidetim_rtc_mask(channel));

    let mut idedma_ctl: u32 = 0;
    // If channel disabled, no need to go further
    if piix_idetim_read(oidetim, channel) & PIIX_IDETIM_IDE == 0 {
        return;
    }
    // set up new idetim: Enable IDE registers decode
    idetim = piix_idetim_set(idetim, PIIX_IDETIM_IDE, channel);

    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    for drive in 0..2 {
        udmareg &= !(piix_udmactl_drv_en(channel, drive) | piix_udmatim_set(0x3, channel, drive));
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        if drvp.isset(DRIVE_DMA) || drvp.isset(DRIVE_UDMA) {
            if matches!(
                prod,
                PCI_PRODUCT_INTEL_6300ESB_IDE
                    | PCI_PRODUCT_INTEL_6321ESB_IDE
                    | PCI_PRODUCT_INTEL_82801AA_IDE
                    | PCI_PRODUCT_INTEL_82801AB_IDE
                    | PCI_PRODUCT_INTEL_82801BAM_IDE
                    | PCI_PRODUCT_INTEL_82801BA_IDE
                    | PCI_PRODUCT_INTEL_82801CAM_IDE
                    | PCI_PRODUCT_INTEL_82801CA_IDE
                    | PCI_PRODUCT_INTEL_82801DB_IDE
                    | PCI_PRODUCT_INTEL_82801DBL_IDE
                    | PCI_PRODUCT_INTEL_82801DBM_IDE
                    | PCI_PRODUCT_INTEL_82801EB_IDE
                    | PCI_PRODUCT_INTEL_82801FB_IDE
                    | PCI_PRODUCT_INTEL_82801GB_IDE
                    | PCI_PRODUCT_INTEL_82801HBM_IDE
                    | PCI_PRODUCT_INTEL_82372FB_IDE
            ) {
                ideconf |= PIIX_CONFIG_PINGPONG;
            }
            if matches!(
                prod,
                PCI_PRODUCT_INTEL_6300ESB_IDE
                    | PCI_PRODUCT_INTEL_6321ESB_IDE
                    | PCI_PRODUCT_INTEL_82801BAM_IDE
                    | PCI_PRODUCT_INTEL_82801BA_IDE
                    | PCI_PRODUCT_INTEL_82801CAM_IDE
                    | PCI_PRODUCT_INTEL_82801CA_IDE
                    | PCI_PRODUCT_INTEL_82801DB_IDE
                    | PCI_PRODUCT_INTEL_82801DBL_IDE
                    | PCI_PRODUCT_INTEL_82801DBM_IDE
                    | PCI_PRODUCT_INTEL_82801EB_IDE
                    | PCI_PRODUCT_INTEL_82801FB_IDE
                    | PCI_PRODUCT_INTEL_82801GB_IDE
                    | PCI_PRODUCT_INTEL_82801HBM_IDE
            ) {
                // setup Ultra/100
                if drvp.UDMA_mode.get() > 2 && ideconf & piix_config_cr(channel, drive) == 0 {
                    drvp.UDMA_mode.set(2);
                }
                if drvp.UDMA_mode.get() > 4 {
                    ideconf |= piix_config_udma100(channel, drive);
                } else {
                    ideconf &= !piix_config_udma100(channel, drive);
                    if drvp.UDMA_mode.get() > 2 {
                        ideconf |= piix_config_udma66(channel, drive);
                    } else {
                        ideconf &= !piix_config_udma66(channel, drive);
                    }
                }
            }
            if prod == PCI_PRODUCT_INTEL_82801AA_IDE || prod == PCI_PRODUCT_INTEL_82372FB_IDE {
                // setup Ultra/66
                if drvp.UDMA_mode.get() > 2 && ideconf & piix_config_cr(channel, drive) == 0 {
                    drvp.UDMA_mode.set(2);
                }
                if drvp.UDMA_mode.get() > 2 {
                    ideconf |= piix_config_udma66(channel, drive);
                } else {
                    ideconf &= !piix_config_udma66(channel, drive);
                }
            }

            if chp.wdc().has(WDC_CAPABILITY_UDMA) && drvp.isset(DRIVE_UDMA) {
                // use Ultra/DMA
                drvp.clr(DRIVE_DMA);
                udmareg |= piix_udmactl_drv_en(channel, drive);
                udmareg |= piix_udmatim_set(
                    u32::from(PIIX4_SCT_UDMA[drvp.UDMA_mode.get() as usize]),
                    channel,
                    drive,
                );
            } else {
                // use Multiword DMA
                drvp.clr(DRIVE_UDMA);
                if drive == 0 {
                    idetim |= piix_setup_idetim_timings(drvp.DMA_mode.get(), 1, channel);
                } else {
                    sidetim |= piix_setup_sidetim_timings(drvp.DMA_mode.get(), 1, channel);
                    idetim = piix_idetim_set(idetim, PIIX_IDETIM_SITRE, channel);
                }
            }
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        }

        // pio: use PIO mode
        idetim |= piix_setup_idetim_drvs(drvp);
        if drive == 0 {
            idetim |= piix_setup_idetim_timings(drvp.PIO_mode.get(), 0, channel);
        } else {
            sidetim |= piix_setup_sidetim_timings(drvp.PIO_mode.get(), 0, channel);
            idetim = piix_idetim_set(idetim, PIIX_IDETIM_SITRE, channel);
        }
    }
    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }
    pci_conf_write(sc.pc(), sc.tag(), PIIX_IDETIM, idetim);
    pci_conf_write(sc.pc(), sc.tag(), PIIX_SIDETIM, sidetim);
    pci_conf_write(sc.pc(), sc.tag(), PIIX_UDMAREG, udmareg);
    pci_conf_write(sc.pc(), sc.tag(), PIIX_CONFIG, ideconf);
    pciide_print_modes(cp);
}

/// `piix_setup_idetim_timings`: setup ISP and RTC fields, based on mode.
pub fn piix_setup_idetim_timings(mode: u8, dma: u8, channel: i32) -> u32 {
    let mode = mode as usize;
    if dma != 0 {
        piix_idetim_set(
            0,
            piix_idetim_isp_set(PIIX_ISP_DMA[mode].into())
                | piix_idetim_rtc_set(PIIX_RTC_DMA[mode].into()),
            channel,
        )
    } else {
        piix_idetim_set(
            0,
            piix_idetim_isp_set(PIIX_ISP_PIO[mode].into())
                | piix_idetim_rtc_set(PIIX_RTC_PIO[mode].into()),
            channel,
        )
    }
}

/// `piix_setup_idetim_drvs`: setup DTE, PPE, IE and TIME field based on PIO mode.
pub fn piix_setup_idetim_drvs(drvp: &AtaDriveDatas) -> u32 {
    let mut ret = 0;
    let chp = wdc_drvp_chp(drvp);
    let channel = chp.channel.get();
    let drive = i32::from(drvp.drive.get());

    // If drive is using UDMA, timing setup is independent so just check DMA and PIO here.
    if drvp.isset(DRIVE_DMA) {
        // if mode = DMA mode 0, use compatible timings
        if drvp.isset(DRIVE_DMA) && drvp.DMA_mode.get() == 0 {
            drvp.PIO_mode.set(0);
            return ret;
        }
        ret = piix_idetim_set(ret, piix_idetim_time(drive), channel);
        // PIO and DMA timings are the same, use fast timings for PIO too, else use compat
        // timings.
        let (pio, dma) = (drvp.PIO_mode.get() as usize, drvp.DMA_mode.get() as usize);
        if PIIX_ISP_PIO[pio] != PIIX_ISP_DMA[dma] || PIIX_RTC_PIO[pio] != PIIX_RTC_DMA[dma] {
            drvp.PIO_mode.set(0);
        }
        // if PIO mode <= 2, use compat timings for PIO
        if drvp.PIO_mode.get() <= 2 {
            ret = piix_idetim_set(ret, piix_idetim_dte(drive), channel);
            return ret;
        }
    }

    // Now setup PIO modes. If mode < 2, use compat timings. Else enable fast timings.
    // Enable IORDY and prefetch/post if PIO mode >= 3.

    if drvp.PIO_mode.get() < 2 {
        return ret;
    }

    ret = piix_idetim_set(ret, piix_idetim_time(drive), channel);
    if drvp.PIO_mode.get() >= 3 {
        ret = piix_idetim_set(ret, piix_idetim_ie(drive), channel);
        ret = piix_idetim_set(ret, piix_idetim_ppe(drive), channel);
    }
    ret
}

/// `piix_setup_sidetim_timings`: setup values in SIDETIM registers, based on mode.
pub fn piix_setup_sidetim_timings(mode: u8, dma: u8, channel: i32) -> u32 {
    let mode = mode as usize;
    if dma != 0 {
        piix_sidetim_isp_set(PIIX_ISP_DMA[mode].into(), channel)
            | piix_sidetim_rtc_set(PIIX_RTC_DMA[mode].into(), channel)
    } else {
        piix_sidetim_isp_set(PIIX_ISP_PIO[mode].into(), channel)
            | piix_sidetim_rtc_set(PIIX_RTC_PIO[mode].into(), channel)
    }
}

/// `amd756_chip_map`.
pub fn amd756_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let interface = pci_interface(pa.pa_class);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);
    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.UDMA_cap.set(match sc.pp().ide_product {
        PCI_PRODUCT_AMD_8111_IDE => 6,
        PCI_PRODUCT_AMD_766_IDE | PCI_PRODUCT_AMD_PBC768_IDE => 5,
        _ => 4,
    });
    sc.sc_wdcdev.set_modes.set(Some(amd756_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);
    let mut chanenable = pci_conf_read(sc.pc(), sc.tag(), AMD756_CHANSTATUS_EN);

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }

        if chanenable & amd756_chan_en(channel) == 0 {
            printf(format_args!(
                "{}: {} ignored (disabled)\n",
                sc.xname(),
                cp.name()
            ));
            cp.hw_ok.set(0);
            continue;
        }
        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }

        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            pciide_pci_intr,
        );

        if pciide_chan_candisable(cp) {
            chanenable &= !amd756_chan_en(channel);
        }
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }

        amd756_setup_channel(&cp.wdc_channel);
    }
    pci_conf_write(sc.pc(), sc.tag(), AMD756_CHANSTATUS_EN, chanenable);
}

/// `amd756_setup_channel` (`PCIIDE_AMD756_ENABLEDMA` is not defined: the revision D2
/// workaround is compiled).
pub fn amd756_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    // !PCIIDE_AMD756_ENABLEDMA
    let product = sc.pp().ide_product;
    let rev = sc.sc_rev.get();

    let mut idedma_ctl: u8 = 0;
    let mut datatim_reg = pci_conf_read(sc.pc(), sc.tag(), AMD756_DATATIM);
    let mut udmatim_reg = pci_conf_read(sc.pc(), sc.tag(), AMD756_UDMA);
    datatim_reg &= !amd756_datatim_mask(channel);
    udmatim_reg &= !amd756_udma_mask(channel);
    let chanenable = pci_conf_read(sc.pc(), sc.tag(), AMD756_CHANSTATUS_EN);

    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        // add timing values, setup DMA if needed
        let mut mode = 'pio: {
            if !drvp.isset(DRIVE_DMA) && !drvp.isset(DRIVE_UDMA) {
                break 'pio drvp.PIO_mode.get();
            }
            let mode;
            if chp.wdc().has(WDC_CAPABILITY_UDMA) && drvp.isset(DRIVE_UDMA) {
                // use Ultra/DMA
                drvp.clr(DRIVE_DMA);

                // Check cable
                if chanenable & amd756_cable(channel, drive) == 0 && drvp.UDMA_mode.get() > 2 {
                    wdcdebug_print!(
                        wdcdebug_pciide_mask,
                        DEBUG_PROBE,
                        "{}({}:{}:{}): 80-wire cable not detected\n",
                        drvp.name(),
                        sc.xname(),
                        channel,
                        drive
                    );
                    drvp.UDMA_mode.set(2);
                }

                udmatim_reg |= amd756_udma_en(channel, drive)
                    | amd756_udma_en_mth(channel, drive)
                    | amd756_udma_time(
                        channel,
                        drive,
                        AMD756_UDMA_TIM[drvp.UDMA_mode.get() as usize].into(),
                    );
                // can use PIO timings, MW DMA unused
                mode = drvp.PIO_mode.get();
            } else {
                // use Multiword DMA, but only if revision is OK
                drvp.clr(DRIVE_UDMA);
                // The workaround doesn't seem to be necessary with all drives, so it can be
                // disabled by PCIIDE_AMD756_ENABLEDMA. It causes a hard hang if triggered.
                if amd756_chiprev_disabledma(product, rev) {
                    printf(format_args!(
                        "{}:{}:{}: multi-word DMA disabled due to chip revision\n",
                        sc.xname(),
                        channel,
                        drive
                    ));
                    drvp.clr(DRIVE_DMA);
                    break 'pio drvp.PIO_mode.get();
                }
                // mode = min(pio, dma+2)
                if drvp.PIO_mode.get() <= drvp.DMA_mode.get() + 2 {
                    mode = drvp.PIO_mode.get();
                } else {
                    mode = drvp.DMA_mode.get() + 2;
                }
            }
            idedma_ctl |= idedma_ctl_drv_dma(drive);
            mode
        };

        // pio: setup PIO mode
        if mode <= 2 {
            drvp.DMA_mode.set(0);
            drvp.PIO_mode.set(0);
            mode = 0;
        } else {
            drvp.PIO_mode.set(mode);
            drvp.DMA_mode.set(mode - 2);
        }
        datatim_reg |= amd756_datatim_pulse(channel, drive, AMD756_PIO_SET[mode as usize].into())
            | amd756_datatim_recov(channel, drive, AMD756_PIO_REC[mode as usize].into());
    }
    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl,
        );
    }
    pciide_print_modes(cp);
    pci_conf_write(sc.pc(), sc.tag(), AMD756_DATATIM, datatim_reg);
    pci_conf_write(sc.pc(), sc.tag(), AMD756_UDMA, udmatim_reg);
}

/// `apollo_chip_map`.
pub fn apollo_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let mut no_ideconf = false;
    let mut ideconf: u32 = 0;
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    // Fake interface since VT6410 is claimed to be a ``RAID'' device.
    let interface = if pci_subclass(pa.pa_class) == PCI_SUBCLASS_MASS_STORAGE_IDE {
        pci_interface(pa.pa_class)
    } else {
        PCIIDE_INTERFACE_BUS_MASTER_DMA | pciide_interface_pci(0) | pciide_interface_pci(1)
    };

    let product = pci_product(pa.pa_id);
    if matches!(
        product,
        PCI_PRODUCT_VIATECH_VT6410
            | PCI_PRODUCT_VIATECH_VT6415
            | PCI_PRODUCT_VIATECH_CX700_IDE
            | PCI_PRODUCT_VIATECH_VX700_IDE
            | PCI_PRODUCT_VIATECH_VX855_IDE
            | PCI_PRODUCT_VIATECH_VX900_IDE
    ) {
        if matches!(
            product,
            PCI_PRODUCT_VIATECH_VT6410 | PCI_PRODUCT_VIATECH_VT6415
        ) {
            no_ideconf = true;
            // FALLTHROUGH
        }
        printf(format_args!(": ATA133"));
        sc.sc_wdcdev.UDMA_cap.set(6);
    } else {
        // Determine the DMA capabilities by looking at the ISA bridge.
        let mut tag = pci_make_tag(pa.pa_pc, pa.pa_bus as i32, pa.pa_device as i32, 0);
        let mut id = pci_conf_read(sc.pc(), tag, PCI_ID_REG);
        let mut class = pci_conf_read(sc.pc(), tag, PCI_CLASS_REG);

        // XXX On the VT8237, the ISA bridge is on a different device.
        if pci_class(class) != PCI_CLASS_BRIDGE && pa.pa_device == 15 {
            tag = pci_make_tag(pa.pa_pc, pa.pa_bus as i32, 17, 0);
            id = pci_conf_read(sc.pc(), tag, PCI_ID_REG);
            class = pci_conf_read(sc.pc(), tag, PCI_CLASS_REG);
        }

        let (msg, cap) = match pci_product(id) {
            PCI_PRODUCT_VIATECH_VT82C586_ISA => {
                if pci_revision(class) >= 0x02 {
                    (": ATA33", 2)
                } else {
                    (": DMA", 0)
                }
            }
            PCI_PRODUCT_VIATECH_VT82C596A => {
                if pci_revision(class) >= 0x12 {
                    (": ATA66", 4)
                } else {
                    (": ATA33", 2)
                }
            }
            PCI_PRODUCT_VIATECH_VT82C686A_ISA => {
                if pci_revision(class) >= 0x40 {
                    (": ATA100", 5)
                } else {
                    (": ATA66", 4)
                }
            }
            PCI_PRODUCT_VIATECH_VT8231_ISA
            | PCI_PRODUCT_VIATECH_VT8233_ISA
            | PCI_PRODUCT_VIATECH_VT8233C_ISA => (": ATA100", 5),
            PCI_PRODUCT_VIATECH_VT8233A_ISA
            | PCI_PRODUCT_VIATECH_VT8235_ISA
            | PCI_PRODUCT_VIATECH_VT8237_ISA
            | PCI_PRODUCT_VIATECH_VT8237A_ISA
            | PCI_PRODUCT_VIATECH_VT8237S_ISA
            | PCI_PRODUCT_VIATECH_VT8251_ISA
            | PCI_PRODUCT_VIATECH_VT8261_ISA => (": ATA133", 6),
            _ => (": DMA", 0),
        };
        printf(format_args!("{}", msg));
        sc.sc_wdcdev.UDMA_cap.set(cap);
    }

    pciide_mapreg_dma(sc, pa);
    wdc_cap_set(
        sc,
        WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE,
    );
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
        if sc.sc_wdcdev.UDMA_cap.get() > 0 {
            wdc_cap_set(sc, WDC_CAPABILITY_UDMA);
        }
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.set_modes.set(Some(apollo_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "apollo_chip_map: old APO_IDECONF=0x{:x}, APO_CTLMISC=0x{:x}, APO_DATATIM=0x{:x}, APO_UDMA=0x{:x}\n",
        pci_conf_read(sc.pc(), sc.tag(), APO_IDECONF),
        pci_conf_read(sc.pc(), sc.tag(), APO_CTLMISC),
        pci_conf_read(sc.pc(), sc.tag(), APO_DATATIM),
        pci_conf_read(sc.pc(), sc.tag(), APO_UDMA)
    );

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }

        if !no_ideconf {
            ideconf = pci_conf_read(sc.pc(), sc.tag(), APO_IDECONF);
            if ideconf & apo_ideconf_en(channel) == 0 {
                printf(format_args!(
                    "{}: {} ignored (disabled)\n",
                    sc.xname(),
                    cp.name()
                ));
                cp.hw_ok.set(0);
                continue;
            }
        }
        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }

        'next: {
            pciide_mapchan(
                pa,
                cp,
                interface,
                &mut cmdsize,
                &mut ctlsize,
                pciide_pci_intr,
            );
            if cp.hw_ok.get() == 0 {
                break 'next;
            }
            if pciide_chan_candisable(cp) && !no_ideconf {
                ideconf &= !apo_ideconf_en(channel);
                pci_conf_write(sc.pc(), sc.tag(), APO_IDECONF, ideconf);
            }

            if cp.hw_ok.get() == 0 {
                break 'next;
            }
            apollo_setup_channel(&sc.channel(channel).wdc_channel);
        }
        // next:
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
        }
    }
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "apollo_chip_map: APO_DATATIM=0x{:x}, APO_UDMA=0x{:x}\n",
        pci_conf_read(sc.pc(), sc.tag(), APO_DATATIM),
        pci_conf_read(sc.pc(), sc.tag(), APO_UDMA)
    );
}

/// `apollo_setup_channel`.
pub fn apollo_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();

    let mut idedma_ctl: u8 = 0;
    let mut datatim_reg = pci_conf_read(sc.pc(), sc.tag(), APO_DATATIM);
    let mut udmatim_reg = pci_conf_read(sc.pc(), sc.tag(), APO_UDMA);
    datatim_reg &= !apo_datatim_mask(channel);
    udmatim_reg &= !apo_udma_mask(channel);

    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    // We can't mix Ultra/33 and Ultra/66 on the same channel, so downgrade to Ultra/33 if
    // needed
    let d = &chp.ch_drive;
    if d[0].isset(DRIVE_UDMA) && d[1].isset(DRIVE_UDMA) {
        // both drives UDMA
        if d[0].UDMA_mode.get() > 2 && d[1].UDMA_mode.get() <= 2 {
            // drive 0 Ultra/66, drive 1 Ultra/33
            d[0].UDMA_mode.set(2);
        } else if d[1].UDMA_mode.get() > 2 && d[0].UDMA_mode.get() <= 2 {
            // drive 1 Ultra/66, drive 0 Ultra/33
            d[1].UDMA_mode.set(2);
        }
    }

    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        // add timing values, setup DMA if needed
        let mut mode = 'pio: {
            if !drvp.isset(DRIVE_DMA) && !drvp.isset(DRIVE_UDMA) {
                break 'pio drvp.PIO_mode.get();
            }
            let mode;
            if chp.wdc().has(WDC_CAPABILITY_UDMA) && drvp.isset(DRIVE_UDMA) {
                // use Ultra/DMA
                drvp.clr(DRIVE_DMA);
                udmatim_reg |= apo_udma_en(channel, drive) | apo_udma_en_mth(channel, drive);
                let udma = drvp.UDMA_mode.get() as usize;
                match sc.sc_wdcdev.UDMA_cap.get() {
                    6 => {
                        udmatim_reg |=
                            apo_udma_time(channel, drive, APOLLO_UDMA133_TIM[udma].into());
                    }
                    5 => {
                        // 686b
                        udmatim_reg |=
                            apo_udma_time(channel, drive, APOLLO_UDMA100_TIM[udma].into());
                    }
                    4 => {
                        // 596b or 686a
                        udmatim_reg |= apo_udma_clk66(channel);
                        udmatim_reg |=
                            apo_udma_time(channel, drive, APOLLO_UDMA66_TIM[udma].into());
                    }
                    _ => {
                        // 596a or 586b
                        udmatim_reg |=
                            apo_udma_time(channel, drive, APOLLO_UDMA33_TIM[udma].into());
                    }
                }
                // can use PIO timings, MW DMA unused
                mode = drvp.PIO_mode.get();
            } else {
                // use Multiword DMA
                drvp.clr(DRIVE_UDMA);
                // mode = min(pio, dma+2)
                if drvp.PIO_mode.get() <= drvp.DMA_mode.get() + 2 {
                    mode = drvp.PIO_mode.get();
                } else {
                    mode = drvp.DMA_mode.get() + 2;
                }
            }
            idedma_ctl |= idedma_ctl_drv_dma(drive);
            mode
        };

        // pio: setup PIO mode
        if mode <= 2 {
            drvp.DMA_mode.set(0);
            drvp.PIO_mode.set(0);
            mode = 0;
        } else {
            drvp.PIO_mode.set(mode);
            drvp.DMA_mode.set(mode - 2);
        }
        datatim_reg |= apo_datatim_pulse(channel, drive, APOLLO_PIO_SET[mode as usize].into())
            | apo_datatim_recov(channel, drive, APOLLO_PIO_REC[mode as usize].into());
    }
    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl,
        );
    }
    pciide_print_modes(cp);
    pci_conf_write(sc.pc(), sc.tag(), APO_DATATIM, datatim_reg);
    pci_conf_write(sc.pc(), sc.tag(), APO_UDMA, udmatim_reg);
}

/// The interface of a CMD 0643-0649 or 0680: the 0648/0649 can be told to identify as a RAID
/// controller, in which case it is faked (`cmd_channel_map`, `cmd0643_9_chip_map`).
fn cmd_interface(pa: &PciAttachArgs) -> Pcireg {
    if pci_subclass(pa.pa_class) != PCI_SUBCLASS_MASS_STORAGE_IDE {
        let mut interface = pciide_interface_settable(0) | pciide_interface_settable(1);
        if u32::from(pciide_pci_read(pa.pa_pc, pa.pa_tag, CMD_CONF)) & CMD_CONF_DSA1 != 0 {
            interface |= pciide_interface_pci(0) | pciide_interface_pci(1);
        }
        interface
    } else {
        pci_interface(pa.pa_class)
    }
}

/// `cmd_channel_map`.
pub fn cmd_channel_map(pa: &PciAttachArgs, sc: &'static PciideSoftc, channel: i32) {
    let cp = sc.channel(channel);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;
    let mut ctrl = pciide_pci_read(sc.pc(), sc.tag(), CMD_CTRL);

    // The 0648/0649 can be told to identify as a RAID controller. In this case, we have to
    // fake interface
    let interface = cmd_interface(pa);

    sc.wdc_chanarray[channel as usize].set(Some(NonNull::from(&cp.wdc_channel)));
    cp.name.set(Some(pciide_channel_name(channel)));
    cp.wdc_channel.channel.set(channel);
    cp.wdc_channel.wdc.set(Some(NonNull::from(&sc.sc_wdcdev)));

    // Older CMD64X doesn't have independent channels
    let one_channel = sc.pp().ide_product != PCI_PRODUCT_CMDTECH_649;

    if channel > 0 && one_channel {
        cp.wdc_channel
            .ch_queue
            .set(sc.channel(0).wdc_channel.ch_queue.get());
    } else {
        cp.wdc_channel.ch_queue.set(wdc_alloc_queue());
    }
    if cp.wdc_channel.ch_queue.get().is_none() {
        printf(format_args!(
            "{}: {} cannot allocate channel queue",
            sc.xname(),
            cp.name()
        ));
        return;
    }

    // with a CMD PCI64x, if we get here, the first channel is enabled: there's no way to
    // disable the first channel without disabling the whole device
    if channel != 0 && u32::from(ctrl) & CMD_CTRL_2PORT == 0 {
        printf(format_args!(
            "{}: {} ignored (disabled)\n",
            sc.xname(),
            cp.name()
        ));
        cp.hw_ok.set(0);
        return;
    }
    cp.hw_ok.set(1);
    pciide_map_compat_intr(pa, cp, channel, interface);
    if cp.hw_ok.get() == 0 {
        return;
    }
    pciide_mapchan(pa, cp, interface, &mut cmdsize, &mut ctlsize, cmd_pci_intr);
    if cp.hw_ok.get() == 0 {
        pciide_unmap_compat_intr(pa, cp, channel, interface);
        return;
    }
    if pciide_chan_candisable(cp) && channel == 1 {
        ctrl &= !(CMD_CTRL_2PORT as u8);
        pciide_pci_write(pa.pa_pc, pa.pa_tag, CMD_CTRL, ctrl);
        pciide_unmap_compat_intr(pa, cp, channel, interface);
    }
}

/// `cmd_pci_intr`.
pub fn cmd_pci_intr(arg: *mut c_void) -> i32 {
    // SAFETY: `pciide_mapregs_native` establishes this handler with the softc.
    let sc = unsafe { &*arg.cast_const().cast::<PciideSoftc>() };

    let mut rv = 0;
    let priirq = u32::from(pciide_pci_read(sc.pc(), sc.tag(), CMD_CONF));
    let secirq = u32::from(pciide_pci_read(sc.pc(), sc.tag(), CMD_ARTTIM23));
    for i in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(i);
        let wdc_cp = &cp.wdc_channel;
        // If a compat channel skip.
        if cp.compat.get() != 0 {
            continue;
        }
        if (i == 0 && priirq & CMD_CONF_DRV0_INTR != 0)
            || (i == 1 && secirq & CMD_ARTTIM23_IRQ != 0)
        {
            let crv = wdcintr(ptr::from_ref(wdc_cp).cast_mut().cast());
            if crv == 0 {
                // #if 0: printf("%s:%d: bogus intr\n", ...)
            } else {
                rv = 1;
            }
        }
    }
    rv
}

/// `cmd_chip_map`.
pub fn cmd_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let interface = pci_interface(pa.pa_class);

    printf(format_args!(": no DMA"));
    sc.sc_dma_ok.set(0);

    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);
    sc.sc_wdcdev.cap.set(WDC_CAPABILITY_DATA16);

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        cmd_channel_map(pa, sc, channel);
    }
}

/// `cmd0643_9_chip_map` (`PCIIDE_CMD0646U_ENABLEUDMA` is not defined: the 646U's UDMA stays
/// off).
pub fn cmd0643_9_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let rev = sc.sc_rev.get();

    // The 0648/0649 can be told to identify as a RAID controller. In this case, we have to
    // fake interface
    let interface = cmd_interface(pa);

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);
    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_IRQACK);
        match sc.pp().ide_product {
            PCI_PRODUCT_CMDTECH_649 => {
                wdc_cap_set(sc, WDC_CAPABILITY_UDMA);
                sc.sc_wdcdev.UDMA_cap.set(5);
                sc.sc_wdcdev.irqack.set(Some(cmd646_9_irqack));
            }
            PCI_PRODUCT_CMDTECH_648 => {
                wdc_cap_set(sc, WDC_CAPABILITY_UDMA);
                sc.sc_wdcdev.UDMA_cap.set(4);
                sc.sc_wdcdev.irqack.set(Some(cmd646_9_irqack));
            }
            PCI_PRODUCT_CMDTECH_646 => {
                if rev >= CMD0646U2_REV as i32 {
                    wdc_cap_set(sc, WDC_CAPABILITY_UDMA);
                    sc.sc_wdcdev.UDMA_cap.set(2);
                } else if rev >= CMD0646U_REV as i32 {
                    // Linux's driver claims that the 646U is broken with UDMA. Only enable
                    // it if we know what we're doing (PCIIDE_CMD0646U_ENABLEUDMA, not
                    // defined).
                    // explicitly disable UDMA
                    pciide_pci_write(sc.pc(), sc.tag(), cmd_udmatim(0), 0);
                    pciide_pci_write(sc.pc(), sc.tag(), cmd_udmatim(1), 0);
                }
                sc.sc_wdcdev.irqack.set(Some(cmd646_9_irqack));
            }
            _ => sc.sc_wdcdev.irqack.set(Some(pciide_irqack)),
        }
    }

    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.set_modes.set(Some(cmd0643_9_setup_channel));

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "cmd0643_9_chip_map: old timings reg 0x{:x} 0x{:x}\n",
        pci_conf_read(sc.pc(), sc.tag(), 0x54),
        pci_conf_read(sc.pc(), sc.tag(), 0x58)
    );
    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        cmd_channel_map(pa, sc, channel);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        cmd0643_9_setup_channel(&cp.wdc_channel);
    }
    // note - this also makes sure we clear the irq disable and reset bits
    pciide_pci_write(sc.pc(), sc.tag(), CMD_DMA_MODE, CMD_DMA_MULTIPLE as u8);
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "cmd0643_9_chip_map: timings reg now 0x{:x} 0x{:x}\n",
        pci_conf_read(sc.pc(), sc.tag(), 0x54),
        pci_conf_read(sc.pc(), sc.tag(), 0x58)
    );
}

/// `cmd0643_9_setup_channel` (the `__sparc64__` reset at the end is not compiled).
pub fn cmd0643_9_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();

    let mut idedma_ctl: u32 = 0;
    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        // add timing values, setup DMA if needed
        let mut tim = CMD0643_9_DATA_TIM_PIO[drvp.PIO_mode.get() as usize];
        if drvp.isset(DRIVE_DMA | DRIVE_UDMA) {
            if drvp.isset(DRIVE_UDMA) {
                // UltraDMA on a 646U2, 0648 or 0649
                drvp.clr(DRIVE_DMA);
                let mut udma_reg =
                    u32::from(pciide_pci_read(sc.pc(), sc.tag(), cmd_udmatim(channel)));
                if drvp.UDMA_mode.get() > 2
                    && u32::from(pciide_pci_read(sc.pc(), sc.tag(), CMD_BICSR))
                        & cmd_bicsr_80(channel)
                        == 0
                {
                    wdcdebug_print!(
                        wdcdebug_pciide_mask,
                        DEBUG_PROBE,
                        "{}({}:{}:{}): 80-wire cable not detected\n",
                        drvp.name(),
                        sc.xname(),
                        channel,
                        drive
                    );
                    drvp.UDMA_mode.set(2);
                }
                if drvp.UDMA_mode.get() > 2 {
                    udma_reg &= !cmd_udmatim_udma33(drive);
                } else if sc.sc_wdcdev.UDMA_cap.get() > 2 {
                    udma_reg |= cmd_udmatim_udma33(drive);
                }
                udma_reg |= cmd_udmatim_udma(drive);
                udma_reg &= !(CMD_UDMATIM_TIM_MASK << cmd_udmatim_tim_off(drive));
                udma_reg |= u32::from(CMD0646_9_TIM_UDMA[drvp.UDMA_mode.get() as usize])
                    << cmd_udmatim_tim_off(drive);
                pciide_pci_write(sc.pc(), sc.tag(), cmd_udmatim(channel), udma_reg as u8);
            } else {
                // use Multiword DMA. Timings will be used for both PIO and DMA, so adjust
                // DMA mode if needed if we have a 0646U2/8/9, turn off UDMA
                if sc.sc_wdcdev.has(WDC_CAPABILITY_UDMA) {
                    let mut udma_reg =
                        u32::from(pciide_pci_read(sc.pc(), sc.tag(), cmd_udmatim(channel)));
                    udma_reg &= !cmd_udmatim_udma(drive);
                    pciide_pci_write(sc.pc(), sc.tag(), cmd_udmatim(channel), udma_reg as u8);
                }
                if drvp.PIO_mode.get() >= 3 && drvp.DMA_mode.get() + 2 > drvp.PIO_mode.get() {
                    drvp.DMA_mode.set(drvp.PIO_mode.get() - 2);
                }
                tim = CMD0643_9_DATA_TIM_DMA[drvp.DMA_mode.get() as usize];
            }
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        }
        pciide_pci_write(sc.pc(), sc.tag(), cmd_data_tim(channel, drive), tim);
    }
    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }
    pciide_print_modes(cp);
}

/// `cmd646_9_irqack`.
pub fn cmd646_9_irqack(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());

    if chp.channel.get() == 0 {
        let priirq = pciide_pci_read(sc.pc(), sc.tag(), CMD_CONF);
        pciide_pci_write(sc.pc(), sc.tag(), CMD_CONF, priirq);
    } else {
        let secirq = pciide_pci_read(sc.pc(), sc.tag(), CMD_ARTTIM23);
        pciide_pci_write(sc.pc(), sc.tag(), CMD_ARTTIM23, secirq);
    }
    pciide_irqack(chp);
}

/// `cmd680_chip_map`.
pub fn cmd680_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    printf(format_args!(
        "\n{}: bus-master DMA support present",
        sc.xname()
    ));
    pciide_mapreg_dma(sc, pa);
    printf(format_args!("\n"));
    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_IRQACK);
        wdc_cap_set(sc, WDC_CAPABILITY_UDMA);
        sc.sc_wdcdev.UDMA_cap.set(6);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }

    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.set_modes.set(Some(cmd680_setup_channel));

    pciide_pci_write(sc.pc(), sc.tag(), 0x80, 0x00);
    pciide_pci_write(sc.pc(), sc.tag(), 0x84, 0x00);
    pciide_pci_write(
        sc.pc(),
        sc.tag(),
        0x8a,
        pciide_pci_read(sc.pc(), sc.tag(), 0x8a) | 0x01,
    );
    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        cmd680_channel_map(pa, sc, channel);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        cmd680_setup_channel(&cp.wdc_channel);
    }
}

/// `cmd680_channel_map`.
pub fn cmd680_channel_map(pa: &PciAttachArgs, sc: &'static PciideSoftc, channel: i32) {
    let cp = sc.channel(channel);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;
    const INIT_VAL: [u8; 14] = [
        0x8a, 0x32, 0x8a, 0x32, 0x8a, 0x32, 0x92, 0x43, 0x92, 0x43, 0x09, 0x40, 0x09, 0x40,
    ];

    let interface = if pci_subclass(pa.pa_class) != PCI_SUBCLASS_MASS_STORAGE_IDE {
        pciide_interface_settable(0)
            | pciide_interface_settable(1)
            | pciide_interface_pci(0)
            | pciide_interface_pci(1)
    } else {
        pci_interface(pa.pa_class)
    };

    sc.wdc_chanarray[channel as usize].set(Some(NonNull::from(&cp.wdc_channel)));
    cp.name.set(Some(pciide_channel_name(channel)));
    cp.wdc_channel.channel.set(channel);
    cp.wdc_channel.wdc.set(Some(NonNull::from(&sc.sc_wdcdev)));

    cp.wdc_channel.ch_queue.set(wdc_alloc_queue());
    if cp.wdc_channel.ch_queue.get().is_none() {
        printf(format_args!(
            "{} {}: cannot allocate channel queue",
            sc.xname(),
            cp.name()
        ));
        return;
    }

    // XXX
    let reg = 0xa2 + channel * 16;
    for (i, v) in INIT_VAL.iter().enumerate() {
        pciide_pci_write(sc.pc(), sc.tag(), reg + i as i32, *v);
    }

    printf(format_args!(
        "{}: {} {} to {} mode\n",
        sc.xname(),
        cp.name(),
        if interface & pciide_interface_settable(channel) != 0 {
            "configured"
        } else {
            "wired"
        },
        if interface & pciide_interface_pci(channel) != 0 {
            "native-PCI"
        } else {
            "compatibility"
        }
    ));

    pciide_mapchan(
        pa,
        cp,
        interface,
        &mut cmdsize,
        &mut ctlsize,
        pciide_pci_intr,
    );
    if cp.hw_ok.get() == 0 {
        return;
    }
    pciide_map_compat_intr(pa, cp, channel, interface);
}

/// `cmd680_setup_channel`.
///
/// Like the C, the multiword DMA and PIO timings write both bytes of their 16-bit value to
/// the same register `off` (the high byte last).
pub fn cmd680_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let pc = sc.pc();
    let pa = sc.tag();
    let channel = chp.channel.get();
    const UDMA2_TBL: [u8; 7] = [0x0f, 0x0b, 0x07, 0x06, 0x03, 0x02, 0x01];
    const UDMA_TBL: [u8; 7] = [0x0c, 0x07, 0x05, 0x04, 0x02, 0x01, 0x00];
    const DMA_TBL: [u16; 3] = [0x2208, 0x10c2, 0x10c1];
    const PIO_TBL: [u16; 5] = [0x328a, 0x2283, 0x1104, 0x10c3, 0x10c1];

    let mut idedma_ctl: u32 = 0;
    pciide_channel_dma_setup(cp);
    let mut mode = pciide_pci_read(pc, pa, 0x80 + channel * 4);

    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        mode &= !(0x03u8 << (drive * 4));
        if drvp.isset(DRIVE_UDMA) {
            drvp.clr(DRIVE_DMA);
            let off = 0xa0 + channel * 16;
            if drvp.UDMA_mode.get() > 2 && pciide_pci_read(pc, pa, off) & 0x01 == 0 {
                drvp.UDMA_mode.set(2);
            }
            let mut scsc = pciide_pci_read(pc, pa, 0x8a);
            if drvp.UDMA_mode.get() == 6 && scsc & 0x30 == 0 {
                pciide_pci_write(pc, pa, 0x8a, scsc | 0x01);
                scsc = pciide_pci_read(pc, pa, 0x8a);
                if scsc & 0x30 == 0 {
                    drvp.UDMA_mode.set(5);
                }
            }
            mode |= 0x03 << (drive * 4);
            let off = 0xac + channel * 16 + drive * 2;
            let mut val = u16::from(pciide_pci_read(pc, pa, off)) & !0x3f;
            if scsc & 0x30 != 0 {
                val |= u16::from(UDMA2_TBL[drvp.UDMA_mode.get() as usize]);
            } else {
                val |= u16::from(UDMA_TBL[drvp.UDMA_mode.get() as usize]);
            }
            pciide_pci_write(pc, pa, off, val as u8);
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        } else if drvp.isset(DRIVE_DMA) {
            mode |= 0x02 << (drive * 4);
            let off = 0xa8 + channel * 16 + drive * 2;
            let val = DMA_TBL[drvp.DMA_mode.get() as usize];
            pciide_pci_write(pc, pa, off, (val & 0xff) as u8);
            pciide_pci_write(pc, pa, off, (val >> 8) as u8);
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        } else {
            mode |= 0x01 << (drive * 4);
            let off = 0xa4 + channel * 16 + drive * 2;
            let val = PIO_TBL[drvp.PIO_mode.get() as usize];
            pciide_pci_write(pc, pa, off, (val & 0xff) as u8);
            pciide_pci_write(pc, pa, off, (val >> 8) as u8);
        }
    }

    pciide_pci_write(pc, pa, 0x80 + channel * 4, mode);
    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }
    pciide_print_modes(cp);
}

/// `sii_fixup_cacheline`: when the Silicon Image 3112 retries a PCI memory read command, it
/// may retry it as a memory read multiple command under some circumstances. This can
/// totally confuse some PCI controllers, so ensure that it will never do this by making sure
/// that the Read Threshold (FIFO Read Request Control) field of the FIFO Valid Byte Count
/// and Control registers for both channels (BA5 offset 0x40 and 0x44) are set to be at
/// least as large as the cacheline size register.
pub fn sii_fixup_cacheline(sc: &PciideSoftc, pa: &PciAttachArgs) {
    let mut cls = pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_BHLC_REG);
    cls = (cls >> PCI_CACHELINE_SHIFT) & PCI_CACHELINE_MASK;
    cls *= 4;
    if cls > 224 {
        cls = pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_BHLC_REG);
        cls &= !(PCI_CACHELINE_MASK << PCI_CACHELINE_SHIFT);
        cls |= (224 / 4) << PCI_CACHELINE_SHIFT;
        pci_conf_write(pa.pa_pc, pa.pa_tag, PCI_BHLC_REG, cls);
        cls = 224;
    }
    if cls < 32 {
        cls = 32;
    }
    cls = cls.div_ceil(32);
    let reg40 = ba5_read_4(sc, 0x40);
    let reg44 = ba5_read_4(sc, 0x44);
    if (reg40 & 0x7) < cls {
        ba5_write_4(sc, 0x40, (reg40 & !0x07) | cls);
    }
    if (reg44 & 0x7) < cls {
        ba5_write_4(sc, 0x44, (reg44 & !0x07) | cls);
    }
}

/// `sc->sc_cookie = malloc(sizeof(*sl), M_DEVBUF, M_NOWAIT | M_ZERO)` for a chip's private
/// structure, initialised to `init`. The C dereferences a failed allocation; here it panics.
fn pciide_alloc_cookie<T>(sc: &PciideSoftc, init: T) -> &T {
    sc.sc_cookielen.set(size_of::<T>());
    let Some(mem) = malloc(size_of::<T>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        panic(format_args!("{}: no memory for chip data", sc.xname()));
    };
    let mem = mem.cast::<T>();
    // SAFETY: a fresh allocation of `size_of::<T>()` bytes, malloc-aligned (`T` holds only
    // integers, pointers and cells of them), written whole before any use.
    unsafe { mem.as_ptr().write(init) };
    sc.sc_cookie.set(mem.as_ptr().cast());
    // SAFETY: just initialised; freed only by `default_chip_unmap` on detach.
    unsafe { mem.as_ref() }
}

/// `SII3112_RESET_BITS`.
const SII3112_RESET_BITS: u32 = SCS_CMD_PBM_RESET
    | SCS_CMD_ARB_RESET
    | SCS_CMD_FF1_RESET
    | SCS_CMD_FF0_RESET
    | SCS_CMD_IDE1_RESET
    | SCS_CMD_IDE0_RESET;

/// `sii3112_chip_map`.
pub fn sii3112_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    // Allocate memory for private data
    let sl = pciide_alloc_cookie(sc, PciideSatalink::new());

    sc.chip_unmap.set(Some(default_chip_unmap));

    // Reset everything and then unblock all of the interrupts.
    let scs_cmd = pci_conf_read(pa.pa_pc, pa.pa_tag, SII3112_SCS_CMD);
    pci_conf_write(
        pa.pa_pc,
        pa.pa_tag,
        SII3112_SCS_CMD,
        scs_cmd | SII3112_RESET_BITS,
    );
    delay(50 * 1000);
    pci_conf_write(
        pa.pa_pc,
        pa.pa_tag,
        SII3112_SCS_CMD,
        scs_cmd & SCS_CMD_BA5_EN,
    );
    delay(50 * 1000);

    if scs_cmd & SCS_CMD_BA5_EN != 0 {
        match pci_mapreg_map(
            pa,
            PCI_MAPREG_START + 0x14,
            PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_32BIT,
            0,
            0,
        ) {
            Err(_) => {
                printf(format_args!(": unable to map BA5 register space\n"));
            }
            Ok((st, sh, _, _)) => {
                sl.ba5_st.set(Some(st));
                sl.ba5_sh.set(Some(sh));
                sl.ba5_en.set(1);
            }
        }
    } else {
        let cfgctl = pci_conf_read(pa.pa_pc, pa.pa_tag, SII3112_PCI_CFGCTL);
        pci_conf_write(
            pa.pa_pc,
            pa.pa_tag,
            SII3112_PCI_CFGCTL,
            cfgctl | CFGCTL_BA5INDEN,
        );
    }

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);
    printf(format_args!("\n"));

    // Rev. <= 0x01 of the 3112 have a bug that can cause data corruption if DMA transfers
    // cross an 8K boundary. This is apparently hard to tickle, but we'll go ahead and play
    // it safe.
    if sc.sc_rev.get() <= 0x01 {
        sc.sc_dma_maxsegsz.set(8192);
        sc.sc_dma_boundary.set(8192);
    }

    sii_fixup_cacheline(sc, pa);

    wdc_cap_set(sc, WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32);
    sc.sc_wdcdev.PIO_cap.set(4);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
        sc.sc_wdcdev.DMA_cap.set(2);
        sc.sc_wdcdev.UDMA_cap.set(6);
    }
    sc.sc_wdcdev.set_modes.set(Some(sii3112_setup_channel));

    // We can use SControl and SStatus to probe for drives.
    sc.sc_wdcdev.drv_probe.set(Some(sii3112_drv_probe));

    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);

    // The 3112 either identifies itself as a RAID storage device or a Misc storage device.
    // Fake up the interface bits for what our driver expects.
    let interface = if pci_subclass(pa.pa_class) == PCI_SUBCLASS_MASS_STORAGE_IDE {
        pci_interface(pa.pa_class)
    } else {
        PCIIDE_INTERFACE_BUS_MASTER_DMA | pciide_interface_pci(0) | pciide_interface_pci(1)
    };

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }
        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            pciide_pci_intr,
        );
        if cp.hw_ok.get() == 0 {
            continue;
        }
        if let Some(set_modes) = sc.sc_wdcdev.set_modes.get() {
            set_modes(&cp.wdc_channel);
        }
    }
}

/// `sii3112_setup_channel`.
pub fn sii3112_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();

    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    let mut idedma_ctl: u32 = 0;
    let mut dtm: u32 = 0;

    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        if drvp.isset(DRIVE_UDMA) {
            // use Ultra/DMA
            drvp.clr(DRIVE_DMA);
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            dtm |= DTM_IDEx_DMA;
        } else if drvp.isset(DRIVE_DMA) {
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            dtm |= DTM_IDEx_DMA;
        } else {
            dtm |= DTM_IDEx_PIO;
        }
    }

    // Nothing to do to setup modes; it is meaningless in S-ATA (but many S-ATA drives still
    // want to get the SET_FEATURE command).
    if idedma_ctl != 0 {
        // Add software bits in status register
        sc.dmactl_write(channel, idedma_ctl as u8);
    }
    ba5_write_4(sc, SATALINK_BA5_REGMAP[channel as usize].ba5_IDE_DTM, dtm);
    pciide_print_modes(cp);
}

/// `sii3112_drv_probe`: probe the drive through SControl and SStatus.
pub fn sii3112_drv_probe(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    let regmap = &SATALINK_BA5_REGMAP[channel as usize];

    // The 3112 is a 2-port part, and only has one drive per channel (each port emulates a
    // master drive).
    //
    // The 3114 is similar, but has 4 channels.

    // Request communication initialization sequence, any speed. Performing this is the
    // equivalent of an ATA Reset.
    let mut scontrol = SControl_DET_INIT | SControl_SPD_ANY;

    // XXX We don't yet support SATA power management; disable all power management state
    // transitions.
    scontrol |= SControl_IPM_NONE;

    ba5_write_4(sc, regmap.ba5_SControl, scontrol);
    delay(50 * 1000);
    scontrol &= !SControl_DET_INIT;
    ba5_write_4(sc, regmap.ba5_SControl, scontrol);
    delay(50 * 1000);

    let sstatus = ba5_read_4(sc, regmap.ba5_SStatus);
    // #if 0: printf("%s: port %d: SStatus=0x%08x, SControl=0x%08x\n", ...)
    match sstatus & SStatus_DET_mask {
        SStatus_DET_NODEV => {
            // No device; be silent.
        }
        SStatus_DET_DEV_NE => {
            printf(format_args!(
                "{}: port {}: device connected, but communication not established\n",
                sc.xname(),
                channel
            ));
        }
        SStatus_DET_OFFLINE => {
            printf(format_args!(
                "{}: port {}: PHY offline\n",
                sc.xname(),
                channel
            ));
        }
        SStatus_DET_DEV => {
            // XXX ATAPI detection doesn't currently work. Don't XXX know why. But, it's not
            // like the standard method XXX can detect an ATAPI device connected via a
            // SATA/PATA XXX bridge, so at least this is no worse. --thorpej
            if chp._vtbl.get().is_some() {
                chp.write_reg(wdr_sdh, WDSD_IBM);
            } else {
                bus_space_write_1(chp.cmd_iot(), chp.cmd_ioh(), wdr_sdh.offset(), WDSD_IBM);
            }
            delay(10); // 400ns delay
            // Save register contents.
            let (scnt, sn, cl, ch);
            if chp._vtbl.get().is_some() {
                scnt = chp.read_reg(wdr_seccnt);
                sn = chp.read_reg(wdr_sector);
                cl = chp.read_reg(wdr_cyl_lo);
                ch = chp.read_reg(wdr_cyl_hi);
            } else {
                let (t, h) = (chp.cmd_iot(), chp.cmd_ioh());
                scnt = bus_space_read_1(t, h, wdr_seccnt.offset());
                sn = bus_space_read_1(t, h, wdr_sector.offset());
                cl = bus_space_read_1(t, h, wdr_cyl_lo.offset());
                ch = bus_space_read_1(t, h, wdr_cyl_hi.offset());
            }
            // #if 0: printf("%s: port %d: scnt=0x%x sn=0x%x cl=0x%x ch=0x%x\n", ...)
            let _ = (scnt, sn);
            // scnt and sn are supposed to be 0x1 for ATAPI, but in some cases we get wrong
            // values here, so ignore it.
            let s = splbio();
            if cl == 0x14 && ch == 0xeb {
                chp.ch_drive[0].set(DRIVE_ATAPI);
            } else {
                chp.ch_drive[0].set(DRIVE_ATA);
            }
            splx(s);

            printf(format_args!("{}: port {}", sc.xname(), channel));
            match (sstatus & SStatus_SPD_mask) >> SStatus_SPD_shift {
                1 => {
                    printf(format_args!(": 1.5Gb/s"));
                }
                2 => {
                    printf(format_args!(": 3.0Gb/s"));
                }
                _ => {}
            }
            printf(format_args!("\n"));
        }
        _ => {
            printf(format_args!(
                "{}: port {}: unknown SStatus: 0x{:08x}\n",
                sc.xname(),
                channel,
                sstatus
            ));
        }
    }
}

/// `SII3114_RESET_BITS`.
const SII3114_RESET_BITS: u32 = SCS_CMD_PBM_RESET
    | SCS_CMD_ARB_RESET
    | SCS_CMD_FF1_RESET
    | SCS_CMD_FF0_RESET
    | SCS_CMD_FF3_RESET
    | SCS_CMD_FF2_RESET
    | SCS_CMD_IDE1_RESET
    | SCS_CMD_IDE0_RESET
    | SCS_CMD_IDE3_RESET
    | SCS_CMD_IDE2_RESET;

/// `sii3114_chip_map`.
pub fn sii3114_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    // Allocate memory for private data
    let sl = pciide_alloc_cookie(sc, PciideSatalink::new());

    // Reset everything and then unblock all of the interrupts.
    let scs_cmd = pci_conf_read(pa.pa_pc, pa.pa_tag, SII3112_SCS_CMD);
    pci_conf_write(
        pa.pa_pc,
        pa.pa_tag,
        SII3112_SCS_CMD,
        scs_cmd | SII3114_RESET_BITS,
    );
    delay(50 * 1000);
    pci_conf_write(
        pa.pa_pc,
        pa.pa_tag,
        SII3112_SCS_CMD,
        scs_cmd & SCS_CMD_M66EN,
    );
    delay(50 * 1000);

    // On the 3114, the BA5 register space is always enabled. In order to use the 3114 in
    // any sane way, we must use this BA5 register space, and so we consider it an error if
    // we cannot map it.
    //
    // As a consequence of using BA5, our register mapping is different from a normal PCI
    // IDE controller's, and so we are unable to use most of the common PCI IDE register
    // mapping functions.
    match pci_mapreg_map(
        pa,
        PCI_MAPREG_START + 0x14,
        PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_32BIT,
        0,
        0,
    ) {
        Err(_) => {
            printf(format_args!(": unable to map BA5 register space\n"));
            return;
        }
        Ok((st, sh, _, _)) => {
            sl.ba5_st.set(Some(st));
            sl.ba5_sh.set(Some(sh));
        }
    }
    sl.ba5_en.set(1);

    // Set the Interrupt Steering bit in the IDEDMA_CMD register of channel 2. This is
    // required at all times for proper operation when using the BA5 register space
    // (otherwise interrupts from all 4 channels won't work).
    ba5_write_4(
        sc,
        SATALINK_BA5_REGMAP[2].ba5_IDEDMA_CMD,
        IDEDMA_CMD_INT_STEER,
    );

    printf(format_args!(": DMA"));
    sii3114_mapreg_dma(sc, pa);
    printf(format_args!("\n"));

    sii_fixup_cacheline(sc, pa);

    wdc_cap_set(sc, WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32);
    sc.sc_wdcdev.PIO_cap.set(4);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
        sc.sc_wdcdev.DMA_cap.set(2);
        sc.sc_wdcdev.UDMA_cap.set(6);
    }
    sc.sc_wdcdev.set_modes.set(Some(sii3112_setup_channel));

    // We can use SControl and SStatus to probe for drives.
    sc.sc_wdcdev.drv_probe.set(Some(sii3112_drv_probe));

    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(4);

    // Map and establish the interrupt handler.
    let Some(intrhandle) = pci_intr_map(pa) else {
        printf(format_args!(
            "{}: couldn't map native-PCI interrupt\n",
            sc.xname()
        ));
        return;
    };
    let intrstr = pci_intr_string(pa.pa_pc, intrhandle);
    let ih = pci_intr_establish(
        pa.pa_pc,
        intrhandle,
        IPL_BIO,
        // XXX
        pciide_pci_intr,
        ptr::from_ref(sc).cast_mut().cast(),
        sc.sc_wdcdev.sc_dev.xname(),
    );
    sc.sc_pci_ih
        .set(ih.map_or(ptr::null_mut(), |ih| ih.as_ptr()));
    if ih.is_some() {
        printf(format_args!(
            "{}: using {} for native-PCI interrupt\n",
            sc.xname(),
            intrstr
        ));
    } else {
        printf(format_args!(
            "{}: couldn't establish native-PCI interrupt",
            sc.xname()
        ));
        printf(format_args!(" at {}", intrstr));
        printf(format_args!("\n"));
        return;
    }

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !sii3114_chansetup(sc, channel) {
            continue;
        }
        sii3114_mapchan(cp);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        if let Some(set_modes) = sc.sc_wdcdev.set_modes.get() {
            set_modes(&cp.wdc_channel);
        }
    }
}

/// `sii3114_mapreg_dma`: the channels' DMA registers are subregions of BA5.
pub fn sii3114_mapreg_dma(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let sl = satalink(sc);

    sc.sc_wdcdev
        .dma_arg
        .set(ptr::from_ref(sc).cast_mut().cast());
    sc.sc_wdcdev.dma_init.set(Some(pciide_dma_init));
    sc.sc_wdcdev.dma_start.set(Some(pciide_dma_start));
    sc.sc_wdcdev.dma_finish.set(Some(pciide_dma_finish));

    // Slice off a subregion of BA5 for each of the channel's DMA registers.

    sc.sc_dma_iot.set(sl.ba5_st.get());
    for (regmap, regs) in SATALINK_BA5_REGMAP.iter().zip(&sl.regs) {
        for reg in 0..IDEDMA_NREGS {
            let mut size: BusSize = 4;
            if size > IDEDMA_SCH_OFFSET - reg {
                size = IDEDMA_SCH_OFFSET - reg;
            }
            let off = regmap.ba5_IDEDMA_CMD + reg;
            match bus_space_subregion(sl.ba5_st(), sl.ba5_sh(), off, size) {
                Ok(h) => regs.dma_iohs[reg].set(Some(h)),
                Err(_) => {
                    sc.sc_dma_ok.set(0);
                    printf(format_args!(
                        ": can't subregion offset {} size {}",
                        off, size
                    ));
                    return;
                }
            }
        }
    }

    sc.sc_dmacmd_read.set(Some(sii3114_dmacmd_read));
    sc.sc_dmacmd_write.set(Some(sii3114_dmacmd_write));
    sc.sc_dmactl_read.set(Some(sii3114_dmactl_read));
    sc.sc_dmactl_write.set(Some(sii3114_dmactl_write));
    sc.sc_dmatbl_write.set(Some(sii3114_dmatbl_write));

    // DMA registers all set up!
    sc.sc_dmat.set(Some(pa.pa_dmat));
    sc.sc_dma_ok.set(1);
}

/// `sii3114_chansetup`.
pub fn sii3114_chansetup(sc: &'static PciideSoftc, channel: i32) -> bool {
    const CHANNEL_NAMES: [&str; 4] = ["port 0", "port 1", "port 2", "port 3"];
    let cp = sc.channel(channel);

    sc.wdc_chanarray[channel as usize].set(Some(NonNull::from(&cp.wdc_channel)));

    // We must always keep the Interrupt Steering bit set in channel 2's IDEDMA_CMD
    // register.
    if channel == 2 {
        cp.idedma_cmd.set(IDEDMA_CMD_INT_STEER as u8);
    }

    cp.name.set(Some(CHANNEL_NAMES[channel as usize]));
    cp.wdc_channel.channel.set(channel);
    cp.wdc_channel.wdc.set(Some(NonNull::from(&sc.sc_wdcdev)));
    cp.wdc_channel.ch_queue.set(wdc_alloc_queue());
    if cp.wdc_channel.ch_queue.get().is_none() {
        printf(format_args!(
            "{} {} channel: cannot allocate channel queue",
            sc.xname(),
            cp.name()
        ));
        return false;
    }
    true
}

/// `sii3114_mapchan`: the channel's task file registers are subregions of BA5.
pub fn sii3114_mapchan(cp: &'static PciideChannel) {
    let wdc_cp = &cp.wdc_channel;
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let sl = satalink(sc);
    let chan = wdc_cp.channel.get() as usize;
    let regs = &sl.regs[chan];

    cp.hw_ok.set(0);
    cp.compat.set(0);
    cp.ih.set(sc.sc_pci_ih.get());

    regs.cmd_iot.set(sl.ba5_st.get());
    match bus_space_subregion(
        sl.ba5_st(),
        sl.ba5_sh(),
        SATALINK_BA5_REGMAP[chan].ba5_IDE_TF0,
        9,
    ) {
        Ok(h) => regs.cmd_baseioh.set(Some(h)),
        Err(_) => {
            printf(format_args!(
                "{}: couldn't subregion {} cmd base\n",
                sc.xname(),
                cp.name()
            ));
            return;
        }
    }

    regs.ctl_iot.set(sl.ba5_st.get());
    match bus_space_subregion(
        sl.ba5_st(),
        sl.ba5_sh(),
        SATALINK_BA5_REGMAP[chan].ba5_IDE_TF8,
        1,
    ) {
        Ok(h) => cp.ctl_baseioh.set(Some(h)),
        Err(_) => {
            printf(format_args!(
                "{}: couldn't subregion {} ctl base\n",
                sc.xname(),
                cp.name()
            ));
            return;
        }
    }
    regs.ctl_ioh.set(cp.ctl_baseioh.get());

    let Some(baseioh) = regs.cmd_baseioh.get() else {
        return;
    };
    for i in 0..WDC_NREG as usize {
        match bus_space_subregion(sl.ba5_st(), baseioh, i, if i == 0 { 4 } else { 1 }) {
            Ok(h) => regs.cmd_iohs[i].set(Some(h)),
            Err(_) => {
                printf(format_args!(
                    "{}: couldn't subregion {} channel cmd regs\n",
                    sc.xname(),
                    cp.name()
                ));
                return;
            }
        }
    }
    regs.cmd_iohs[wdr_status.offset()].set(regs.cmd_iohs[wdr_command.offset()].get());
    regs.cmd_iohs[wdr_features.offset()].set(regs.cmd_iohs[wdr_error.offset()].get());
    wdc_cp.cmd_iot.set(regs.cmd_iot.get());
    wdc_cp.data32iot.set(regs.cmd_iot.get());
    wdc_cp.cmd_ioh.set(regs.cmd_iohs[0].get());
    wdc_cp.data32ioh.set(regs.cmd_iohs[0].get());
    wdc_cp._vtbl.set(Some(&WDC_SII3114_VTBL));
    wdcattach(wdc_cp);
    cp.hw_ok.set(1);
}

/// A handle of the SiI3114's per-register map. Panics where the C would use an unmapped
/// one.
fn sl_ioh(h: &Cell<Option<BusSpaceHandle>>) -> BusSpaceHandle {
    match h.get() {
        Some(h) => h,
        None => panic(format_args!("sii3114: register not mapped")),
    }
}

/// A tag of the SiI3114's channel registers.
fn sl_iot(t: &Cell<Option<BusSpaceTag>>) -> BusSpaceTag {
    match t.get() {
        Some(t) => t,
        None => panic(format_args!("sii3114: register space not mapped")),
    }
}

/// `sii3114_read_reg`.
pub fn sii3114_read_reg(chp: &ChannelSoftc, reg: WdcRegs) -> u8 {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let regs = &satalink(sc).regs[chp.channel.get() as usize];

    if reg.isset(_WDC_AUX) {
        bus_space_read_1(sl_iot(&regs.ctl_iot), sl_ioh(&regs.ctl_ioh), reg.offset())
    } else {
        bus_space_read_1(
            sl_iot(&regs.cmd_iot),
            sl_ioh(&regs.cmd_iohs[reg.offset()]),
            0,
        )
    }
}

/// `sii3114_write_reg`.
pub fn sii3114_write_reg(chp: &ChannelSoftc, reg: WdcRegs, val: u8) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let regs = &satalink(sc).regs[chp.channel.get() as usize];

    if reg.isset(_WDC_AUX) {
        bus_space_write_1(
            sl_iot(&regs.ctl_iot),
            sl_ioh(&regs.ctl_ioh),
            reg.offset(),
            val,
        );
    } else {
        bus_space_write_1(
            sl_iot(&regs.cmd_iot),
            sl_ioh(&regs.cmd_iohs[reg.offset()]),
            0,
            val,
        );
    }
}

/// `sii3114_dmacmd_read`.
pub fn sii3114_dmacmd_read(sc: &PciideSoftc, chan: i32) -> u8 {
    let sl = satalink(sc);

    bus_space_read_1(
        sc.dma_iot(),
        sl_ioh(&sl.regs[chan as usize].dma_iohs[idedma_cmd(0)]),
        0,
    )
}

/// `sii3114_dmacmd_write`.
pub fn sii3114_dmacmd_write(sc: &PciideSoftc, chan: i32, val: u8) {
    let sl = satalink(sc);

    bus_space_write_1(
        sc.dma_iot(),
        sl_ioh(&sl.regs[chan as usize].dma_iohs[idedma_cmd(0)]),
        0,
        val,
    );
}

/// `sii3114_dmactl_read`.
pub fn sii3114_dmactl_read(sc: &PciideSoftc, chan: i32) -> u8 {
    let sl = satalink(sc);

    bus_space_read_1(
        sc.dma_iot(),
        sl_ioh(&sl.regs[chan as usize].dma_iohs[idedma_ctl_reg(0)]),
        0,
    )
}

/// `sii3114_dmactl_write`.
pub fn sii3114_dmactl_write(sc: &PciideSoftc, chan: i32, val: u8) {
    let sl = satalink(sc);

    bus_space_write_1(
        sc.dma_iot(),
        sl_ioh(&sl.regs[chan as usize].dma_iohs[idedma_ctl_reg(0)]),
        0,
        val,
    );
}

/// `sii3114_dmatbl_write`.
pub fn sii3114_dmatbl_write(sc: &PciideSoftc, chan: i32, val: u32) {
    let sl = satalink(sc);

    bus_space_write_4(
        sc.dma_iot(),
        sl_ioh(&sl.regs[chan as usize].dma_iohs[idedma_tbl(0)]),
        0,
        val,
    );
}

/// `(struct pciide_cy *)sc->sc_cookie`. Panics if `sc` is not a CY82C693.
fn pciide_cy(sc: &PciideSoftc) -> &PciideCy {
    if !ptr::fn_addr_eq(sc.pp().chip_map, cy693_chip_map as PciideChipMap) {
        panic(format_args!("{}: not a CY82C693", sc.xname()));
    }
    // SAFETY: `cy693_chip_map` sets `sc_cookie` to a `PciideCy` before its hooks run.
    unsafe { sc.cookie::<PciideCy>() }
}

/// `cy693_chip_map`.
pub fn cy693_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let interface = pci_interface(pa.pa_class);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    // Allocate memory for private data
    let cy = pciide_alloc_cookie(
        sc,
        PciideCy {
            cy_handle: Cell::new(None),
            cy_compatchan: Cell::new(0),
        },
    );

    // this chip has 2 PCI IDE functions, one for primary and one for secondary. So we need
    // to call pciide_mapregs_compat() with the real channel
    if pa.pa_function == 1 {
        cy.cy_compatchan.set(0);
    } else if pa.pa_function == 2 {
        cy.cy_compatchan.set(1);
    } else {
        printf(format_args!(
            ": unexpected PCI function {}\n",
            pa.pa_function
        ));
        return;
    }

    if interface & PCIIDE_INTERFACE_BUS_MASTER_DMA != 0 {
        printf(format_args!(": DMA"));
        pciide_mapreg_dma(sc, pa);
    } else {
        printf(format_args!(": no DMA"));
        sc.sc_dma_ok.set(0);
    }

    cy.cy_handle.set(cy82c693_init(pa.pa_iot));
    if cy.cy_handle.get().is_none() {
        printf(format_args!(", (unable to map ctl registers)"));
        sc.sc_dma_ok.set(0);
    }

    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.set_modes.set(Some(cy693_setup_channel));

    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(1);

    // Only one channel for this chip; if we are here it's enabled
    let cp = sc.channel(0);
    sc.wdc_chanarray[0].set(Some(NonNull::from(&cp.wdc_channel)));
    cp.name.set(Some(pciide_channel_name(0)));
    cp.wdc_channel.channel.set(0);
    cp.wdc_channel.wdc.set(Some(NonNull::from(&sc.sc_wdcdev)));
    cp.wdc_channel.ch_queue.set(wdc_alloc_queue());
    if cp.wdc_channel.ch_queue.get().is_none() {
        printf(format_args!(": cannot allocate channel queue\n"));
        return;
    }
    printf(format_args!(
        ", {} {} to ",
        pciide_channel_name(0),
        if interface & pciide_interface_settable(0) != 0 {
            "configured"
        } else {
            "wired"
        }
    ));
    let ok = if interface & pciide_interface_pci(0) != 0 {
        printf(format_args!("native-PCI\n"));
        pciide_mapregs_native(pa, cp, &mut cmdsize, &mut ctlsize, pciide_pci_intr)
    } else {
        printf(format_args!("compatibility\n"));
        pciide_mapregs_compat(pa, cp, cy.cy_compatchan.get(), &mut cmdsize, &mut ctlsize)
    };
    cp.hw_ok.set(i32::from(ok));

    cp.wdc_channel.data32iot.set(cp.wdc_channel.cmd_iot.get());
    cp.wdc_channel.data32ioh.set(cp.wdc_channel.cmd_ioh.get());
    pciide_map_compat_intr(pa, cp, cy.cy_compatchan.get(), interface);
    if cp.hw_ok.get() == 0 {
        return;
    }
    wdcattach(&cp.wdc_channel);
    if pciide_chan_candisable(cp) {
        pci_conf_write(sc.pc(), sc.tag(), PCI_COMMAND_STATUS_REG, 0);
    }
    if cp.hw_ok.get() == 0 {
        pciide_unmap_compat_intr(pa, cp, cy.cy_compatchan.get(), interface);
        return;
    }

    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "cy693_chip_map: old timings reg 0x{:x}\n",
        pci_conf_read(sc.pc(), sc.tag(), CY_CMD_CTRL)
    );
    cy693_setup_channel(&cp.wdc_channel);
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "cy693_chip_map: new timings reg 0x{:x}\n",
        pci_conf_read(sc.pc(), sc.tag(), CY_CMD_CTRL)
    );
}

/// `cy693_setup_channel`. Like the C, a channel without DMA drives stores `dma_mode` -1 in
/// both drives' `u_int8_t DMA_mode` (255).
pub fn cy693_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let cy = pciide_cy(sc);
    let mut dma_mode: i32 = -1;

    let mut cy_cmd_ctrl: u32 = 0;
    let mut idedma_ctl: u32 = 0;

    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        // add timing values, setup DMA if needed
        if drvp.isset(DRIVE_DMA) {
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            // use Multiword DMA
            if dma_mode == -1 || dma_mode > i32::from(drvp.DMA_mode.get()) {
                dma_mode = i32::from(drvp.DMA_mode.get());
            }
        }
        let pio = drvp.PIO_mode.get() as usize;
        cy_cmd_ctrl |= u32::from(CY_PIO_PULSE[pio]) << cy_cmd_ctrl_iow_pulse_off(drive);
        cy_cmd_ctrl |= u32::from(CY_PIO_REC[pio]) << cy_cmd_ctrl_iow_rec_off(drive);
        cy_cmd_ctrl |= u32::from(CY_PIO_PULSE[pio]) << cy_cmd_ctrl_ior_pulse_off(drive);
        cy_cmd_ctrl |= u32::from(CY_PIO_REC[pio]) << cy_cmd_ctrl_ior_rec_off(drive);
    }
    pci_conf_write(sc.pc(), sc.tag(), CY_CMD_CTRL, cy_cmd_ctrl);
    chp.ch_drive[0].DMA_mode.set(dma_mode as u8);
    chp.ch_drive[1].DMA_mode.set(dma_mode as u8);

    if dma_mode == -1 {
        dma_mode = 0;
    }

    if let Some(handle) = cy.cy_handle.get() {
        // Note: `multiple' is implied.
        cy82c693_write(
            handle,
            if cy.cy_compatchan.get() == 0 {
                CY_DMA_IDX_PRIMARY as u8
            } else {
                CY_DMA_IDX_SECONDARY as u8
            },
            dma_mode as u8,
        );
    }

    pciide_print_modes(cp);

    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(chp.channel.get()),
            idedma_ctl as u8,
        );
    }
}

/// `SIS_TYPE_NOUDMA`.
const SIS_TYPE_NOUDMA: u8 = 0;
/// `SIS_TYPE_66`.
const SIS_TYPE_66: u8 = 1;
/// `SIS_TYPE_100OLD`.
const SIS_TYPE_100OLD: u8 = 2;
/// `SIS_TYPE_100NEW`.
const SIS_TYPE_100NEW: u8 = 3;
/// `SIS_TYPE_133OLD`.
const SIS_TYPE_133OLD: u8 = 4;
/// `SIS_TYPE_133NEW`.
const SIS_TYPE_133NEW: u8 = 5;
/// `SIS_TYPE_SOUTH`.
const SIS_TYPE_SOUTH: u8 = 6;

/// `struct sis_hostbr_type`.
pub struct SisHostbrType {
    /// `id`.
    pub id: u16,
    /// `rev`.
    pub rev: u8,
    /// `udma_mode`.
    pub udma_mode: u8,
    /// `name`.
    pub name: &'static str,
    /// `type` (`SIS_TYPE_*`).
    pub type_: u8,
}

const fn sis_hostbr(
    id: u32,
    rev: u8,
    udma_mode: u8,
    name: &'static str,
    type_: u8,
) -> SisHostbrType {
    SisHostbrType {
        id: id as u16,
        rev,
        udma_mode,
        name,
        type_,
    }
}

/// `sis_hostbr_type[]`: most infos here are from sos@freebsd.org.
static SIS_HOSTBR_TYPE: [SisHostbrType; 37] = [
    sis_hostbr(PCI_PRODUCT_SIS_530, 0x00, 4, "530", SIS_TYPE_66),
    // #if 0: controllers associated to a rev 0x2 530 Host to PCI Bridge have problems with
    // UDMA (info provided by Christos): {PCI_PRODUCT_SIS_530, 0x02, 0, "530 (buggy)",
    // SIS_TYPE_NOUDMA}
    sis_hostbr(PCI_PRODUCT_SIS_540, 0x00, 4, "540", SIS_TYPE_66),
    sis_hostbr(PCI_PRODUCT_SIS_550, 0x00, 4, "550", SIS_TYPE_66),
    sis_hostbr(PCI_PRODUCT_SIS_620, 0x00, 4, "620", SIS_TYPE_66),
    sis_hostbr(PCI_PRODUCT_SIS_630, 0x00, 4, "630", SIS_TYPE_66),
    sis_hostbr(PCI_PRODUCT_SIS_630, 0x30, 5, "630S", SIS_TYPE_100NEW),
    sis_hostbr(PCI_PRODUCT_SIS_633, 0x00, 5, "633", SIS_TYPE_100NEW),
    sis_hostbr(PCI_PRODUCT_SIS_635, 0x00, 5, "635", SIS_TYPE_100NEW),
    sis_hostbr(PCI_PRODUCT_SIS_640, 0x00, 4, "640", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_645, 0x00, 6, "645", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_646, 0x00, 6, "645DX", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_648, 0x00, 6, "648", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_650, 0x00, 6, "650", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_651, 0x00, 6, "651", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_652, 0x00, 6, "652", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_655, 0x00, 6, "655", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_658, 0x00, 6, "658", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_661, 0x00, 6, "661", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_730, 0x00, 5, "730", SIS_TYPE_100OLD),
    sis_hostbr(PCI_PRODUCT_SIS_733, 0x00, 5, "733", SIS_TYPE_100NEW),
    sis_hostbr(PCI_PRODUCT_SIS_735, 0x00, 5, "735", SIS_TYPE_100NEW),
    sis_hostbr(PCI_PRODUCT_SIS_740, 0x00, 5, "740", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_741, 0x00, 6, "741", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_745, 0x00, 5, "745", SIS_TYPE_100NEW),
    sis_hostbr(PCI_PRODUCT_SIS_746, 0x00, 6, "746", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_748, 0x00, 6, "748", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_750, 0x00, 6, "750", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_751, 0x00, 6, "751", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_752, 0x00, 6, "752", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_755, 0x00, 6, "755", SIS_TYPE_SOUTH),
    sis_hostbr(PCI_PRODUCT_SIS_760, 0x00, 6, "760", SIS_TYPE_SOUTH),
    // From sos@freebsd.org: the 0x961 ID will never be found in real world
    // {PCI_PRODUCT_SIS_961, 0x00, 6, "961", SIS_TYPE_133NEW},
    sis_hostbr(PCI_PRODUCT_SIS_962, 0x00, 6, "962", SIS_TYPE_133NEW),
    sis_hostbr(PCI_PRODUCT_SIS_963, 0x00, 6, "963", SIS_TYPE_133NEW),
    sis_hostbr(PCI_PRODUCT_SIS_964, 0x00, 6, "964", SIS_TYPE_133NEW),
    sis_hostbr(PCI_PRODUCT_SIS_965, 0x00, 6, "965", SIS_TYPE_133NEW),
    sis_hostbr(PCI_PRODUCT_SIS_966, 0x00, 6, "966", SIS_TYPE_133NEW),
    sis_hostbr(PCI_PRODUCT_SIS_968, 0x00, 6, "968", SIS_TYPE_133NEW),
];

/// `sis_hostbr_type_match`: the index of the last `SIS_HOSTBR_TYPE` row `sis_hostbr_match`
/// accepted, plus one; 0 is NULL.
static SIS_HOSTBR_TYPE_MATCH: AtomicUsize = AtomicUsize::new(0);

/// `sis_hostbr_type_match`.
fn sis_hostbr_type_match() -> Option<&'static SisHostbrType> {
    match SIS_HOSTBR_TYPE_MATCH.load(Ordering::Relaxed) {
        0 => None,
        i => SIS_HOSTBR_TYPE.get(i - 1),
    }
}

/// `sis_hostbr_match`: a SiS host bridge of the table (the last row whose id matches and
/// whose revision is at most the bridge's).
pub fn sis_hostbr_match(pa: &PciAttachArgs) -> i32 {
    if pci_vendor(pa.pa_id) != PCI_VENDOR_SIS {
        return 0;
    }
    SIS_HOSTBR_TYPE_MATCH.store(0, Ordering::Relaxed);
    for (i, t) in SIS_HOSTBR_TYPE.iter().enumerate() {
        if pci_product(pa.pa_id) == u32::from(t.id) && pci_revision(pa.pa_class) >= u32::from(t.rev)
        {
            SIS_HOSTBR_TYPE_MATCH.store(i + 1, Ordering::Relaxed);
        }
    }
    i32::from(sis_hostbr_type_match().is_some())
}

/// `sis_south_match`.
pub fn sis_south_match(pa: &PciAttachArgs) -> i32 {
    i32::from(
        pci_vendor(pa.pa_id) == PCI_VENDOR_SIS
            && pci_product(pa.pa_id) == PCI_PRODUCT_SIS_85C503
            && pci_revision(pa.pa_class) >= 0x10,
    )
}

/// `(struct pciide_sis *)sc->sc_cookie`. Panics if `sc` is not a SiS controller.
fn pciide_sis(sc: &PciideSoftc) -> &PciideSis {
    if !ptr::fn_addr_eq(sc.pp().chip_map, sis_chip_map as PciideChipMap) {
        panic(format_args!("{}: not a SiS controller", sc.xname()));
    }
    // SAFETY: `sis_chip_map` sets `sc_cookie` to a `PciideSis` before its hooks run.
    unsafe { sc.cookie::<PciideSis>() }
}

/// `sis_chip_map`.
pub fn sis_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let mut sis_ctr0 = pciide_pci_read(sc.pc(), sc.tag(), SIS_CTRL0);
    let interface = pci_interface(pa.pa_class);
    let rev = sc.sc_rev.get();
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    // Allocate memory for private data
    let sis = pciide_alloc_cookie(
        sc,
        PciideSis {
            sis_type: Cell::new(0),
        },
    );

    // pci_find_device(NULL, ...): the found device's arguments land in a scratch copy.
    let mut found = *pa;
    pci_find_device(&mut found, sis_hostbr_match);

    if let Some(m) = sis_hostbr_type_match() {
        if m.type_ == SIS_TYPE_SOUTH {
            pciide_pci_write(
                sc.pc(),
                sc.tag(),
                SIS_REG_57,
                pciide_pci_read(sc.pc(), sc.tag(), SIS_REG_57) & 0x7f,
            );
            if sc.pp().ide_product == SIS_PRODUCT_5518 {
                sis.sis_type.set(SIS_TYPE_133NEW);
                sc.sc_wdcdev.UDMA_cap.set(m.udma_mode);
            } else if pci_find_device(&mut found, sis_south_match) != 0 {
                sis.sis_type.set(SIS_TYPE_133OLD);
                sc.sc_wdcdev.UDMA_cap.set(m.udma_mode);
            } else {
                sis.sis_type.set(SIS_TYPE_100NEW);
                sc.sc_wdcdev.UDMA_cap.set(m.udma_mode);
            }
        } else {
            sis.sis_type.set(m.type_);
            sc.sc_wdcdev.UDMA_cap.set(m.udma_mode);
        }
        printf(format_args!(": {}", m.name));
    } else {
        printf(format_args!(": 5597/5598"));
        if rev >= 0xd0 {
            sc.sc_wdcdev.UDMA_cap.set(2);
            sis.sis_type.set(SIS_TYPE_66);
        } else {
            sc.sc_wdcdev.UDMA_cap.set(0);
            sis.sis_type.set(SIS_TYPE_NOUDMA);
        }
    }

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);

    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
        if sis.sis_type.get() >= SIS_TYPE_66 {
            wdc_cap_set(sc, WDC_CAPABILITY_UDMA);
        }
    }

    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);

    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);
    let (pc, tag) = (sc.pc(), sc.tag());
    match sis.sis_type.get() {
        SIS_TYPE_NOUDMA | SIS_TYPE_66 | SIS_TYPE_100OLD => {
            sc.sc_wdcdev.set_modes.set(Some(sis_setup_channel));
            pciide_pci_write(
                pc,
                tag,
                SIS_MISC,
                pciide_pci_read(pc, tag, SIS_MISC)
                    | (SIS_MISC_TIM_SEL | SIS_MISC_FIFO_SIZE | SIS_MISC_GTC) as u8,
            );
        }
        SIS_TYPE_100NEW | SIS_TYPE_133OLD => {
            sc.sc_wdcdev.set_modes.set(Some(sis_setup_channel));
            pciide_pci_write(
                pc,
                tag,
                SIS_REG_49,
                pciide_pci_read(pc, tag, SIS_REG_49) | 0x01,
            );
        }
        SIS_TYPE_133NEW => {
            sc.sc_wdcdev.set_modes.set(Some(sis96x_setup_channel));
            pciide_pci_write(
                pc,
                tag,
                SIS_REG_50,
                pciide_pci_read(pc, tag, SIS_REG_50) & 0xf7,
            );
            pciide_pci_write(
                pc,
                tag,
                SIS_REG_52,
                pciide_pci_read(pc, tag, SIS_REG_52) & 0xf7,
            );
        }
        _ => {}
    }

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }
        if (channel == 0 && u32::from(sis_ctr0) & SIS_CTRL0_CHAN0_EN == 0)
            || (channel == 1 && u32::from(sis_ctr0) & SIS_CTRL0_CHAN1_EN == 0)
        {
            printf(format_args!(
                "{}: {} ignored (disabled)\n",
                sc.xname(),
                cp.name()
            ));
            cp.hw_ok.set(0);
            continue;
        }
        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            pciide_pci_intr,
        );
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }
        if pciide_chan_candisable(cp) {
            if channel == 0 {
                sis_ctr0 &= !(SIS_CTRL0_CHAN0_EN as u8);
            } else {
                sis_ctr0 &= !(SIS_CTRL0_CHAN1_EN as u8);
            }
            pciide_pci_write(sc.pc(), sc.tag(), SIS_CTRL0, sis_ctr0);
        }
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }
        if let Some(set_modes) = sc.sc_wdcdev.set_modes.get() {
            set_modes(&cp.wdc_channel);
        }
    }
}

/// `sis96x_setup_channel`.
pub fn sis96x_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();

    let mut sis_tim: u32 = 0;
    let mut idedma_ctl: u32 = 0;
    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    for drive in 0..2 {
        let regtim = sis_tim133(
            pciide_pci_read(sc.pc(), sc.tag(), SIS_REG_57).into(),
            channel,
            drive,
        );
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        // add timing values, setup DMA if needed
        if drvp.isset(DRIVE_UDMA) {
            // use Ultra/DMA
            drvp.clr(DRIVE_DMA);
            if u32::from(pciide_pci_read(sc.pc(), sc.tag(), sis96x_reg_cbl(channel)))
                & SIS96x_REG_CBL_33
                != 0
                && drvp.UDMA_mode.get() > 2
            {
                drvp.UDMA_mode.set(2);
            }
            sis_tim |= SIS_UDMA133NEW_TIM[drvp.UDMA_mode.get() as usize];
            sis_tim |= SIS_PIO133NEW_TIM[drvp.PIO_mode.get() as usize];
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        } else if drvp.isset(DRIVE_DMA) {
            // use Multiword DMA. Timings will be used for both PIO and DMA, so adjust DMA
            // mode if needed
            if drvp.PIO_mode.get() > drvp.DMA_mode.get() + 2 {
                drvp.PIO_mode.set(drvp.DMA_mode.get() + 2);
            }
            if drvp.DMA_mode.get() + 2 > drvp.PIO_mode.get() {
                drvp.DMA_mode.set(if drvp.PIO_mode.get() > 2 {
                    drvp.PIO_mode.get() - 2
                } else {
                    0
                });
            }
            sis_tim |= SIS_DMA133NEW_TIM[drvp.DMA_mode.get() as usize];
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        } else {
            sis_tim |= SIS_PIO133NEW_TIM[drvp.PIO_mode.get() as usize];
        }
        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_PROBE,
            "sis96x_setup_channel: new timings reg for channel {} drive {}: 0x{:x} (reg 0x{:x})\n",
            channel,
            drive,
            sis_tim,
            regtim
        );
        pci_conf_write(sc.pc(), sc.tag(), regtim, sis_tim);
    }
    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }
    pciide_print_modes(cp);
}

/// `sis_setup_channel`.
pub fn sis_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let sis = pciide_sis(sc);
    let channel = chp.channel.get();

    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "sis_setup_channel: old timings reg for channel {} 0x{:x}\n",
        channel,
        pci_conf_read(sc.pc(), sc.tag(), sis_tim(channel))
    );
    let mut sis_tim_v: u32 = 0;
    let mut idedma_ctl: u32 = 0;
    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        // add timing values, setup DMA if needed
        'pio: {
            if !drvp.isset(DRIVE_DMA) && !drvp.isset(DRIVE_UDMA) {
                break 'pio;
            }

            if drvp.isset(DRIVE_UDMA) {
                // use Ultra/DMA
                drvp.clr(DRIVE_DMA);
                if u32::from(pciide_pci_read(sc.pc(), sc.tag(), SIS_REG_CBL))
                    & sis_reg_cbl_33(channel)
                    != 0
                    && drvp.UDMA_mode.get() > 2
                {
                    drvp.UDMA_mode.set(2);
                }
                let udma = drvp.UDMA_mode.get() as usize;
                match sis.sis_type.get() {
                    SIS_TYPE_66 | SIS_TYPE_100OLD => {
                        sis_tim_v |=
                            u32::from(SIS_UDMA66_TIM[udma]) << sis_tim66_udma_time_off(drive);
                    }
                    SIS_TYPE_100NEW => {
                        sis_tim_v |=
                            u32::from(SIS_UDMA100NEW_TIM[udma]) << sis_tim100_udma_time_off(drive);
                    }
                    SIS_TYPE_133OLD => {
                        sis_tim_v |=
                            u32::from(SIS_UDMA133OLD_TIM[udma]) << sis_tim100_udma_time_off(drive);
                    }
                    t => {
                        printf(format_args!("unknown SiS IDE type {}\n", t));
                    }
                }
            } else {
                // use Multiword DMA. Timings will be used for both PIO and DMA, so adjust
                // DMA mode if needed
                if drvp.PIO_mode.get() > drvp.DMA_mode.get() + 2 {
                    drvp.PIO_mode.set(drvp.DMA_mode.get() + 2);
                }
                if drvp.DMA_mode.get() + 2 > drvp.PIO_mode.get() {
                    drvp.DMA_mode.set(if drvp.PIO_mode.get() > 2 {
                        drvp.PIO_mode.get() - 2
                    } else {
                        0
                    });
                }
                if drvp.DMA_mode.get() == 0 {
                    drvp.PIO_mode.set(0);
                }
            }
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        }
        // pio:
        let pio = drvp.PIO_mode.get() as usize;
        match sis.sis_type.get() {
            SIS_TYPE_NOUDMA | SIS_TYPE_66 | SIS_TYPE_100OLD => {
                sis_tim_v |= u32::from(SIS_PIO_ACT[pio]) << sis_tim66_act_off(drive);
                sis_tim_v |= u32::from(SIS_PIO_REC[pio]) << sis_tim66_rec_off(drive);
            }
            SIS_TYPE_100NEW | SIS_TYPE_133OLD => {
                sis_tim_v |= u32::from(SIS_PIO_ACT[pio]) << sis_tim100_act_off(drive);
                sis_tim_v |= u32::from(SIS_PIO_REC[pio]) << sis_tim100_rec_off(drive);
            }
            t => {
                printf(format_args!("unknown SiS IDE type {}\n", t));
            }
        }
    }
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "sis_setup_channel: new timings reg for channel {} 0x{:x}\n",
        channel,
        sis_tim_v
    );
    pci_conf_write(sc.pc(), sc.tag(), sis_tim(channel), sis_tim_v);
    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }
    pciide_print_modes(cp);
}

/// `natsemi_chip_map`.
pub fn natsemi_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;
    let (pc, tag) = (sc.pc(), sc.tag());

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);
    sc.sc_wdcdev.cap.set(WDC_CAPABILITY_DATA16);

    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(natsemi_irqack));
    }

    pciide_pci_write(pc, tag, NATSEMI_CCBT, 0xb7);

    // Mask off interrupts from both channels, appropriate channel(s) will be unmasked later.
    pciide_pci_write(
        pc,
        tag,
        NATSEMI_CTRL2,
        pciide_pci_read(pc, tag, NATSEMI_CTRL2) | (natsemi_chmask(0) | natsemi_chmask(1)) as u8,
    );

    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.set_modes.set(Some(natsemi_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);

    let mut interface = pci_interface(pci_conf_read(pc, tag, PCI_CLASS_REG));
    interface &= !PCIIDE_CHANSTATUS_EN; // Reserved on PC87415
    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    // If we're in PCIIDE mode, unmask INTA, otherwise mask it.
    let mut ctl = u32::from(pciide_pci_read(pc, tag, NATSEMI_CTRL1));
    if interface & (pciide_interface_pci(0) | pciide_interface_pci(1)) != 0 {
        ctl &= !NATSEMI_CTRL1_INTAMASK;
    } else {
        ctl |= NATSEMI_CTRL1_INTAMASK;
    }
    pciide_pci_write(pc, tag, NATSEMI_CTRL1, ctl as u8);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }

        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }

        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            natsemi_pci_intr,
        );
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }
        natsemi_setup_channel(&cp.wdc_channel);
    }
}

/// `natsemi_setup_channel`.
pub fn natsemi_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    let (pc, tag) = (sc.pc(), sc.tag());
    let mut ndrives = 0;
    let mut idedma_ctl: u32 = 0;

    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }

        ndrives += 1;
        // add timing values, setup DMA if needed
        let tim: u8 = if !drvp.isset(DRIVE_DMA) {
            let pio = drvp.PIO_mode.get() as usize;
            NATSEMI_PIO_PULSE[pio] | (NATSEMI_PIO_RECOVER[pio] << 4)
        } else {
            // use Multiword DMA. Timings will be used for both PIO and DMA, so adjust DMA
            // mode if needed
            if drvp.PIO_mode.get() >= 3 && drvp.DMA_mode.get() + 2 > drvp.PIO_mode.get() {
                drvp.DMA_mode.set(drvp.PIO_mode.get() - 2);
            }
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            let dma = drvp.DMA_mode.get() as usize;
            NATSEMI_DMA_PULSE[dma] | (NATSEMI_DMA_RECOVER[dma] << 4)
        };

        pciide_pci_write(pc, tag, natsemi_rtreg(channel, drive), tim);
        pciide_pci_write(pc, tag, natsemi_wtreg(channel, drive), tim);
    }
    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }
    if ndrives > 0 {
        // Unmask the channel if at least one drive is found
        pciide_pci_write(
            pc,
            tag,
            NATSEMI_CTRL2,
            pciide_pci_read(pc, tag, NATSEMI_CTRL2) & !(natsemi_chmask(channel) as u8),
        );
    }

    pciide_print_modes(cp);

    // Go ahead and ack interrupts generated during probe.
    bus_space_write_1(
        sc.dma_iot(),
        sc.dma_ioh(),
        idedma_ctl_reg(channel),
        bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), idedma_ctl_reg(channel)),
    );
}

/// `natsemi_irqack`: the "clear" bits are in the wrong register *sigh*.
pub fn natsemi_irqack(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();

    // The "clear" bits are in the wrong register *sigh*
    let mut clr = bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), idedma_cmd(channel));
    clr |= bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), idedma_ctl_reg(channel))
        & (IDEDMA_CTL_ERR | IDEDMA_CTL_INTR);
    bus_space_write_1(sc.dma_iot(), sc.dma_ioh(), idedma_cmd(channel), clr);
}

/// `natsemi_pci_intr`.
pub fn natsemi_pci_intr(arg: *mut c_void) -> i32 {
    // SAFETY: `pciide_mapregs_native` establishes this handler with the softc.
    let sc = unsafe { &*arg.cast_const().cast::<PciideSoftc>() };

    let mut rv = 0;
    let msk = u32::from(pciide_pci_read(sc.pc(), sc.tag(), NATSEMI_CTRL2));
    for i in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(i);
        let wdc_cp = &cp.wdc_channel;

        // If a compat channel skip.
        if cp.compat.get() != 0 {
            continue;
        }

        // If this channel is masked, skip it.
        if msk & natsemi_chmask(i) != 0 {
            continue;
        }

        if pciide_intr_flag(cp) == 0 {
            continue;
        }

        let crv = wdcintr(ptr::from_ref(wdc_cp).cast_mut().cast());
        if crv == 0 {
            // leave rv alone
        } else if crv == 1 {
            rv = 1; // claim the intr
        } else if rv == 0 {
            // crv should be -1 in this case; if we've done no better, take it
            rv = crv;
        }
    }
    rv
}

/// `ns_scx200_chip_map`.
pub fn ns_scx200_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let interface = pci_interface(pa.pa_class);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);

    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.UDMA_cap.set(2);

    sc.sc_wdcdev.set_modes.set(Some(ns_scx200_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);

    // Soekris net4801 errata 0003:
    //
    // The SC1100 built in busmaster IDE controller is pretty standard, but have two bugs:
    // data transfers need to be dword aligned and it cannot do an exact 64Kbyte data
    // transfer.
    //
    // Assume that reducing maximum segment size by one page will be enough, and restrict
    // boundary too for extra certainty.
    if sc.pp().ide_product == PCI_PRODUCT_NS_SCX200_IDE {
        sc.sc_dma_maxsegsz.set(IDEDMA_BYTE_COUNT_MAX - PAGE_SIZE);
        sc.sc_dma_boundary.set(IDEDMA_BYTE_COUNT_MAX - PAGE_SIZE);
    }

    // This chip seems to be unable to do one-sector transfers using DMA.
    sc.sc_wdcdev.quirks.set(WDC_QUIRK_NOSHORTDMA);

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }
        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            pciide_pci_intr,
        );
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }
        if let Some(set_modes) = sc.sc_wdcdev.set_modes.get() {
            set_modes(&cp.wdc_channel);
        }
    }
}

/// `ns_scx200_setup_channel`.
pub fn ns_scx200_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    let (pc, tag) = (sc.pc(), sc.tag());

    // Setup DMA if needed
    pciide_channel_dma_setup(cp);

    let mut idedma_ctl: u32 = 0;

    let pioformat =
        ((pci_conf_read(pc, tag, scx200_tim_dma(0, 0)) >> SCx200_PIOFORMAT_SHIFT) & 0x01) as usize;
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "{}: pio format {}\n",
        "ns_scx200_setup_channel",
        pioformat
    );

    // Per channel settings
    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];

        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }

        let mut piotim = pci_conf_read(pc, tag, scx200_tim_pio(channel, drive));
        let mut dmatim = pci_conf_read(pc, tag, scx200_tim_dma(channel, drive));
        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_PROBE,
            "{}:{}:{}: piotim=0x{:x}, dmatim=0x{:x}\n",
            sc.xname(),
            channel,
            drive,
            piotim,
            dmatim
        );

        let mode;
        if chp.wdc().has(WDC_CAPABILITY_UDMA) && drvp.isset(DRIVE_UDMA) {
            // Setup UltraDMA mode
            drvp.clr(DRIVE_DMA);
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            dmatim = SCX200_UDMA33[drvp.UDMA_mode.get() as usize];
            mode = drvp.PIO_mode.get();
        } else if chp.wdc().has(WDC_CAPABILITY_DMA) && drvp.isset(DRIVE_DMA) {
            // Setup multiword DMA mode
            drvp.clr(DRIVE_UDMA);
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            dmatim = SCX200_DMA33[drvp.DMA_mode.get() as usize];

            // mode = min(pio, dma + 2)
            if drvp.PIO_mode.get() <= drvp.DMA_mode.get() + 2 {
                mode = drvp.PIO_mode.get();
            } else {
                mode = drvp.DMA_mode.get() + 2;
            }
        } else {
            mode = drvp.PIO_mode.get();
        }

        // Setup PIO mode
        drvp.PIO_mode.set(mode);
        if mode < 2 {
            drvp.DMA_mode.set(0);
        } else {
            drvp.DMA_mode.set(mode - 2);
        }

        piotim = SCX200_PIO33[pioformat][drvp.PIO_mode.get() as usize];

        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_PROBE,
            "{}:{}:{}: new piotim=0x{:x}, dmatim=0x{:x}\n",
            sc.xname(),
            channel,
            drive,
            piotim,
            dmatim
        );

        pci_conf_write(pc, tag, scx200_tim_pio(channel, drive), piotim);
        pci_conf_write(pc, tag, scx200_tim_dma(channel, drive), dmatim);
    }

    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }

    pciide_print_modes(cp);
}

/// `acer_chip_map`.
pub fn acer_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;
    let rev = sc.sc_rev.get();
    let (pc, tag) = (sc.pc(), sc.tag());

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);
    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);

    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA);
        if rev >= 0x20 {
            wdc_cap_set(sc, WDC_CAPABILITY_UDMA);
            if rev >= 0xC4 {
                sc.sc_wdcdev.UDMA_cap.set(5);
            } else if rev >= 0xC2 {
                sc.sc_wdcdev.UDMA_cap.set(4);
            } else {
                sc.sc_wdcdev.UDMA_cap.set(2);
            }
        }
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
        if rev <= 0xC4 {
            sc.sc_wdcdev.dma_init.set(Some(acer_dma_init));
        }
    }

    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.set_modes.set(Some(acer_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);

    pciide_pci_write(
        pc,
        tag,
        ACER_CDRC,
        (pciide_pci_read(pc, tag, ACER_CDRC) | ACER_CDRC_DMA_EN as u8)
            & !(ACER_CDRC_FIFO_DISABLE as u8),
    );

    // Enable "microsoft register bits" R/W.
    pciide_pci_write(
        pc,
        tag,
        ACER_CCAR3,
        pciide_pci_read(pc, tag, ACER_CCAR3) | ACER_CCAR3_PI as u8,
    );
    pciide_pci_write(
        pc,
        tag,
        ACER_CCAR1,
        pciide_pci_read(pc, tag, ACER_CCAR1)
            & !((ACER_CHANSTATUS_RO | pciide_chan_ro(0) | pciide_chan_ro(1)) as u8),
    );
    pciide_pci_write(
        pc,
        tag,
        ACER_CCAR2,
        pciide_pci_read(pc, tag, ACER_CCAR2) & !(ACER_CHANSTATUSREGS_RO as u8),
    );
    let mut cr = pci_conf_read(pc, tag, PCI_CLASS_REG);
    cr |= PCIIDE_CHANSTATUS_EN << PCI_INTERFACE_SHIFT;
    pci_conf_write(pc, tag, PCI_CLASS_REG, cr);
    // Don't use cr, re-read the real register content instead
    let interface = pci_interface(pci_conf_read(pc, tag, PCI_CLASS_REG));

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    // From linux: enable "Cable Detection"
    if rev >= 0xC2 {
        pciide_pci_write(
            pc,
            tag,
            ACER_0x4B,
            pciide_pci_read(pc, tag, ACER_0x4B) | ACER_0x4B_CDETECT as u8,
        );
    }

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }
        if interface & pciide_chan_en(channel) == 0 {
            printf(format_args!(
                "{}: {} ignored (disabled)\n",
                sc.xname(),
                cp.name()
            ));
            cp.hw_ok.set(0);
            continue;
        }
        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            if rev >= 0xC2 {
                pciide_pci_intr
            } else {
                acer_pci_intr
            },
        );
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }
        if pciide_chan_candisable(cp) {
            cr &= !(pciide_chan_en(channel) << PCI_INTERFACE_SHIFT);
            pci_conf_write(pc, tag, PCI_CLASS_REG, cr);
        }
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }
        acer_setup_channel(&cp.wdc_channel);
    }
}

/// `acer_setup_channel`.
pub fn acer_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    let (pc, tag) = (sc.pc(), sc.tag());

    let mut idedma_ctl: u32 = 0;
    let mut acer_fifo_udma = pci_conf_read(pc, tag, ACER_FTH_UDMA);
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "acer_setup_channel: old fifo/udma reg 0x{:x}\n",
        acer_fifo_udma
    );
    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    let d = &chp.ch_drive;
    if (d[0].drive_flags.get() | d[1].drive_flags.get()) & DRIVE_UDMA != 0 {
        // check 80 pins cable
        if u32::from(pciide_pci_read(pc, tag, ACER_0x4A)) & acer_0x4a_80pin(channel) != 0 {
            wdcdebug_print!(
                wdcdebug_pciide_mask,
                DEBUG_PROBE,
                "{}:{}: 80-wire cable not detected\n",
                sc.xname(),
                channel
            );
            if d[0].UDMA_mode.get() > 2 {
                d[0].UDMA_mode.set(2);
            }
            if d[1].UDMA_mode.get() > 2 {
                d[1].UDMA_mode.set(2);
            }
        }
    }

    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_PROBE,
            "acer_setup_channel: old timings reg for channel {} drive {} 0x{:x}\n",
            channel,
            drive,
            pciide_pci_read(pc, tag, acer_idetim(channel, drive))
        );
        // clear FIFO/DMA mode
        acer_fifo_udma &= !(acer_fth_opl(channel, drive, 0x3)
            | acer_udma_en(channel, drive)
            | acer_udma_tim(channel, drive, 0x7));

        // add timing values, setup DMA if needed
        'pio: {
            if !drvp.isset(DRIVE_DMA) && !drvp.isset(DRIVE_UDMA) {
                acer_fifo_udma |= acer_fth_opl(channel, drive, 0x1);
                break 'pio;
            }

            acer_fifo_udma |= acer_fth_opl(channel, drive, 0x2);
            if drvp.isset(DRIVE_UDMA) {
                // use Ultra/DMA
                drvp.clr(DRIVE_DMA);
                acer_fifo_udma |= acer_udma_en(channel, drive);
                acer_fifo_udma |= acer_udma_tim(
                    channel,
                    drive,
                    ACER_UDMA[drvp.UDMA_mode.get() as usize].into(),
                );
                // XXX disable if one drive < UDMA3 ?
                if drvp.UDMA_mode.get() >= 3 {
                    pciide_pci_write(
                        pc,
                        tag,
                        ACER_0x4B,
                        pciide_pci_read(pc, tag, ACER_0x4B) | ACER_0x4B_UDMA66 as u8,
                    );
                }
            } else {
                // use Multiword DMA. Timings will be used for both PIO and DMA, so adjust
                // DMA mode if needed
                if drvp.PIO_mode.get() > drvp.DMA_mode.get() + 2 {
                    drvp.PIO_mode.set(drvp.DMA_mode.get() + 2);
                }
                if drvp.DMA_mode.get() + 2 > drvp.PIO_mode.get() {
                    drvp.DMA_mode.set(if drvp.PIO_mode.get() > 2 {
                        drvp.PIO_mode.get() - 2
                    } else {
                        0
                    });
                }
                if drvp.DMA_mode.get() == 0 {
                    drvp.PIO_mode.set(0);
                }
            }
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        }
        // pio:
        pciide_pci_write(
            pc,
            tag,
            acer_idetim(channel, drive),
            ACER_PIO[drvp.PIO_mode.get() as usize],
        );
    }
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "acer_setup_channel: new fifo/udma reg 0x{:x}\n",
        acer_fifo_udma
    );
    pci_conf_write(pc, tag, ACER_FTH_UDMA, acer_fifo_udma);
    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }
    pciide_print_modes(cp);
}

/// `acer_pci_intr`.
pub fn acer_pci_intr(arg: *mut c_void) -> i32 {
    // SAFETY: `pciide_mapregs_native` establishes this handler with the softc.
    let sc = unsafe { &*arg.cast_const().cast::<PciideSoftc>() };

    let mut rv = 0;
    let chids = u32::from(pciide_pci_read(sc.pc(), sc.tag(), ACER_CHIDS));
    for i in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(i);
        let wdc_cp = &cp.wdc_channel;
        // If a compat channel skip.
        if cp.compat.get() != 0 {
            continue;
        }
        if chids & acer_chids_int(i) != 0 {
            let crv = wdcintr(ptr::from_ref(wdc_cp).cast_mut().cast());
            if crv == 0 {
                printf(format_args!("{}:{}: bogus intr\n", sc.xname(), i));
            } else {
                rv = 1;
            }
        }
    }
    rv
}

/// `acer_dma_init`: use PIO for LBA48 transfers.
///
/// # Safety
///
/// As for [`pciide_dma_init`].
pub unsafe fn acer_dma_init(
    v: *mut c_void,
    channel: i32,
    drive: i32,
    databuf: *mut u8,
    datalen: usize,
    flags: i32,
) -> Result<(), Errno> {
    // Use PIO for LBA48 transfers.
    if flags & WDC_DMA_LBA48 != 0 {
        return Err(Errno::EINVAL);
    }

    // SAFETY: forwarded; the caller's guarantee.
    unsafe { pciide_dma_init(v, channel, drive, databuf, datalen, flags) }
}

/// A HighPoint chip with both channels on one function and the HPT370 and later registers.
fn hpt_is_370_or_later(sc: &PciideSoftc) -> bool {
    let prod = sc.pp().ide_product;
    let revision = sc.sc_rev.get();
    (prod == PCI_PRODUCT_TRIONES_HPT366
        && (revision == HPT370_REV as i32
            || revision == HPT370A_REV as i32
            || revision == HPT372_REV as i32))
        || prod == PCI_PRODUCT_TRIONES_HPT372A
        || prod == PCI_PRODUCT_TRIONES_HPT302
        || prod == PCI_PRODUCT_TRIONES_HPT371
        || prod == PCI_PRODUCT_TRIONES_HPT374
}

/// `hpt_chip_map`.
///
/// Like the C, the loop over the channels sets `compatchan` to 0 before using it, so the
/// HPT366's second function (`pa_function` 1) maps compatibility channel 0's addresses.
pub fn hpt_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;
    let revision = sc.sc_rev.get();
    let prod = sc.pp().ide_product;

    // when the chip is in native mode it identifies itself as a 'misc mass storage'. Fake
    // interface in this case.
    let interface = if pci_subclass(pa.pa_class) == PCI_SUBCLASS_MASS_STORAGE_IDE {
        pci_interface(pa.pa_class)
    } else {
        let mut interface = PCIIDE_INTERFACE_BUS_MASTER_DMA | pciide_interface_pci(0);
        if hpt_is_370_or_later(sc) {
            interface |= pciide_interface_pci(1);
        }
        interface
    };

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);
    printf(format_args!("\n"));
    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);

    sc.sc_wdcdev.set_modes.set(Some(hpt_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    if prod == PCI_PRODUCT_TRIONES_HPT366 && revision == HPT366_REV as i32 {
        sc.sc_wdcdev.UDMA_cap.set(4);
        // The 366 has 2 PCI IDE functions, one for primary and one for secondary. So we
        // need to call pciide_mapregs_compat() with the real channel (the loop below
        // resets it, as in the C).
        if pa.pa_function != 0 && pa.pa_function != 1 {
            printf(format_args!(
                "{}: unexpected PCI function {}\n",
                sc.xname(),
                pa.pa_function
            ));
            return;
        }
        sc.sc_wdcdev.nchannels.set(1);
    } else {
        sc.sc_wdcdev.nchannels.set(2);
        if prod == PCI_PRODUCT_TRIONES_HPT372A
            || prod == PCI_PRODUCT_TRIONES_HPT302
            || prod == PCI_PRODUCT_TRIONES_HPT371
            || prod == PCI_PRODUCT_TRIONES_HPT374
        {
            sc.sc_wdcdev.UDMA_cap.set(6);
        } else if prod == PCI_PRODUCT_TRIONES_HPT366 {
            if revision == HPT372_REV as i32 {
                sc.sc_wdcdev.UDMA_cap.set(6);
            } else {
                sc.sc_wdcdev.UDMA_cap.set(5);
            }
        }
    }
    for i in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(i);
        let mut compatchan = 0;
        if sc.sc_wdcdev.nchannels.get() > 1 {
            compatchan = i;
            if u32::from(pciide_pci_read(sc.pc(), sc.tag(), hpt370_ctrl1(i))) & HPT370_CTRL1_EN == 0
            {
                printf(format_args!(
                    "{}: {} ignored (disabled)\n",
                    sc.xname(),
                    cp.name()
                ));
                cp.hw_ok.set(0);
                continue;
            }
        }
        if !pciide_chansetup(sc, i, interface) {
            continue;
        }
        let ok = if interface & pciide_interface_pci(i) != 0 {
            pciide_mapregs_native(pa, cp, &mut cmdsize, &mut ctlsize, hpt_pci_intr)
        } else {
            pciide_mapregs_compat(pa, cp, compatchan, &mut cmdsize, &mut ctlsize)
        };
        cp.hw_ok.set(i32::from(ok));
        if cp.hw_ok.get() == 0 {
            return;
        }
        cp.wdc_channel.data32iot.set(cp.wdc_channel.cmd_iot.get());
        cp.wdc_channel.data32ioh.set(cp.wdc_channel.cmd_ioh.get());
        wdcattach(&cp.wdc_channel);
        hpt_setup_channel(&cp.wdc_channel);
    }
    let (pc, tag) = (sc.pc(), sc.tag());
    if hpt_is_370_or_later(sc) {
        // Turn off fast interrupts
        for ch in 0..2 {
            pciide_pci_write(
                pc,
                tag,
                hpt370_ctrl2(ch),
                pciide_pci_read(pc, tag, hpt370_ctrl2(ch))
                    & !((HPT370_CTRL2_FASTIRQ | HPT370_CTRL2_HIRQ) as u8),
            );
        }

        // HPT370 and higher has a bit to disable interrupts, make sure to clear it
        pciide_pci_write(
            pc,
            tag,
            HPT_CSEL,
            pciide_pci_read(pc, tag, HPT_CSEL) & !(HPT_CSEL_IRQDIS as u8),
        );
    }
    // set clocks, etc (mandatory on 372/4, optional otherwise)
    if prod == PCI_PRODUCT_TRIONES_HPT372A
        || prod == PCI_PRODUCT_TRIONES_HPT302
        || prod == PCI_PRODUCT_TRIONES_HPT371
        || prod == PCI_PRODUCT_TRIONES_HPT374
        || (prod == PCI_PRODUCT_TRIONES_HPT366 && revision == HPT372_REV as i32)
    {
        pciide_pci_write(
            pc,
            tag,
            HPT_SC2,
            (pciide_pci_read(pc, tag, HPT_SC2) & HPT_SC2_MAEN as u8) | HPT_SC2_OSC_EN as u8,
        );
    }
}

/// `hpt_setup_channel`.
pub fn hpt_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    let revision = sc.sc_rev.get();
    let (pc, tag) = (sc.pc(), sc.tag());

    let cable = u32::from(pciide_pci_read(pc, tag, HPT_CSEL));

    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    let mut idedma_ctl: u32 = 0;

    'end: {
        let (tim_pio, tim_dma, tim_udma): (&[u32], &[u32], &[u32]) = match sc.pp().ide_product {
            PCI_PRODUCT_TRIONES_HPT366 => {
                if revision == HPT370_REV as i32 || revision == HPT370A_REV as i32 {
                    (&HPT370_PIO, &HPT370_DMA, &HPT370_UDMA)
                } else if revision == HPT372_REV as i32 {
                    (&HPT372_PIO, &HPT372_DMA, &HPT372_UDMA)
                } else {
                    (&HPT366_PIO, &HPT366_DMA, &HPT366_UDMA)
                }
            }
            PCI_PRODUCT_TRIONES_HPT372A
            | PCI_PRODUCT_TRIONES_HPT302
            | PCI_PRODUCT_TRIONES_HPT371 => (&HPT372_PIO, &HPT372_DMA, &HPT372_UDMA),
            PCI_PRODUCT_TRIONES_HPT374 => (&HPT374_PIO, &HPT374_DMA, &HPT374_UDMA),
            _ => {
                printf(format_args!("{}: no known timing values\n", sc.xname()));
                break 'end;
            }
        };

        // Per drive settings
        for drive in 0..2 {
            let drvp = &chp.ch_drive[drive as usize];
            // If no drive, skip
            if !drvp.isset(DRIVE) {
                continue;
            }
            let before = pci_conf_read(pc, tag, hpt_idetim(channel, drive));

            // add timing values, setup DMA if needed
            let after;
            if drvp.isset(DRIVE_UDMA) {
                // use Ultra/DMA
                drvp.clr(DRIVE_DMA);
                if cable & hpt_csel_cblid(channel) != 0 && drvp.UDMA_mode.get() > 2 {
                    wdcdebug_print!(
                        wdcdebug_pciide_mask,
                        DEBUG_PROBE,
                        "{}({}:{}:{}): 80-wire cable not detected\n",
                        drvp.name(),
                        sc.xname(),
                        channel,
                        drive
                    );
                    drvp.UDMA_mode.set(2);
                }
                after = tim_udma[drvp.UDMA_mode.get() as usize];
                idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            } else if drvp.isset(DRIVE_DMA) {
                // use Multiword DMA. Timings will be used for both PIO and DMA, so adjust
                // DMA mode if needed
                if drvp.PIO_mode.get() >= 3 && drvp.DMA_mode.get() + 2 > drvp.PIO_mode.get() {
                    drvp.DMA_mode.set(drvp.PIO_mode.get() - 2);
                }
                after = tim_dma[drvp.DMA_mode.get() as usize];
                idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            } else {
                // PIO only
                after = tim_pio[drvp.PIO_mode.get() as usize];
            }
            pci_conf_write(pc, tag, hpt_idetim(channel, drive), after);
            wdcdebug_print!(
                wdcdebug_pciide_mask,
                DEBUG_PROBE,
                "{}: bus speed register set to 0x{:08x} (BIOS 0x{:08x})\n",
                sc.xname(),
                after,
                before
            );
        }
    }
    // end:
    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }
    pciide_print_modes(cp);
}

/// `hpt_pci_intr`.
pub fn hpt_pci_intr(arg: *mut c_void) -> i32 {
    // SAFETY: `pciide_mapregs_native` establishes this handler with the softc.
    let sc = unsafe { &*arg.cast_const().cast::<PciideSoftc>() };
    let mut rv = 0;

    for i in 0..sc.sc_wdcdev.nchannels.get() {
        let dmastat = bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), idedma_ctl_reg(i));
        if dmastat & (IDEDMA_CTL_ACT | IDEDMA_CTL_INTR) != IDEDMA_CTL_INTR {
            continue;
        }
        let cp = sc.channel(i);
        let wdc_cp = &cp.wdc_channel;
        let crv = wdcintr(ptr::from_ref(wdc_cp).cast_mut().cast());
        if crv == 0 {
            printf(format_args!("{}:{}: bogus intr\n", sc.xname(), i));
            bus_space_write_1(sc.dma_iot(), sc.dma_ioh(), idedma_ctl_reg(i), dmastat);
        } else {
            rv = 1;
        }
    }
    rv
}

/// `PDC_IS_262(sc)`.
#[allow(non_snake_case)] // the C macro's name
fn PDC_IS_262(sc: &PciideSoftc) -> bool {
    matches!(
        sc.pp().ide_product,
        PCI_PRODUCT_PROMISE_PDC20262 | PCI_PRODUCT_PROMISE_PDC20265 | PCI_PRODUCT_PROMISE_PDC20267
    )
}

/// `PDC_IS_265(sc)`.
#[allow(non_snake_case)] // the C macro's name
fn PDC_IS_265(sc: &PciideSoftc) -> bool {
    matches!(
        sc.pp().ide_product,
        PCI_PRODUCT_PROMISE_PDC20265
            | PCI_PRODUCT_PROMISE_PDC20267
            | PCI_PRODUCT_PROMISE_PDC20268
            | PCI_PRODUCT_PROMISE_PDC20268R
            | PCI_PRODUCT_PROMISE_PDC20269
            | PCI_PRODUCT_PROMISE_PDC20271
            | PCI_PRODUCT_PROMISE_PDC20275
            | PCI_PRODUCT_PROMISE_PDC20276
            | PCI_PRODUCT_PROMISE_PDC20277
    )
}

/// `PDC_IS_268(sc)`.
#[allow(non_snake_case)] // the C macro's name
fn PDC_IS_268(sc: &PciideSoftc) -> bool {
    matches!(
        sc.pp().ide_product,
        PCI_PRODUCT_PROMISE_PDC20268
            | PCI_PRODUCT_PROMISE_PDC20268R
            | PCI_PRODUCT_PROMISE_PDC20269
            | PCI_PRODUCT_PROMISE_PDC20271
            | PCI_PRODUCT_PROMISE_PDC20275
            | PCI_PRODUCT_PROMISE_PDC20276
            | PCI_PRODUCT_PROMISE_PDC20277
    )
}

/// `PDC_IS_269(sc)`.
#[allow(non_snake_case)] // the C macro's name
fn PDC_IS_269(sc: &PciideSoftc) -> bool {
    matches!(
        sc.pp().ide_product,
        PCI_PRODUCT_PROMISE_PDC20269
            | PCI_PRODUCT_PROMISE_PDC20271
            | PCI_PRODUCT_PROMISE_PDC20275
            | PCI_PRODUCT_PROMISE_PDC20276
            | PCI_PRODUCT_PROMISE_PDC20277
    )
}

/// `pdc268_config_read`: an indexed register of a PDC20268-and-later channel.
pub fn pdc268_config_read(chp: &ChannelSoftc, index: u8) -> u8 {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();

    bus_space_write_1(sc.dma_iot(), sc.dma_ioh(), pdc268_index(channel), index);
    bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), pdc268_data(channel))
}

/// `pdc202xx_chip_map`.
pub fn pdc202xx_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;
    let (pc, tag) = (sc.pc(), sc.tag());
    let mut st: Pcireg = 0;

    if !PDC_IS_268(sc) {
        st = pci_conf_read(pc, tag, PDC2xx_STATE);
        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_PROBE,
            "pdc202xx_setup_chip: controller state 0x{:x}\n",
            st
        );
    }

    // turn off  RAID mode
    if !PDC_IS_268(sc) {
        st &= !PDC2xx_STATE_IDERAID;
    }

    // can't rely on the PCI_CLASS_REG content if the chip was in raid mode. We have to fake
    // interface
    let mut interface = pciide_interface_settable(0) | pciide_interface_settable(1);
    if PDC_IS_268(sc) || st & PDC2xx_STATE_NATIVE != 0 {
        interface |= pciide_interface_pci(0) | pciide_interface_pci(1);
    }

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);

    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.pp().ide_product == PCI_PRODUCT_PROMISE_PDC20246 || PDC_IS_262(sc) {
        wdc_cap_set(sc, WDC_CAPABILITY_NO_ATAPI_DMA);
    }
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    if PDC_IS_269(sc) {
        sc.sc_wdcdev.UDMA_cap.set(6);
    } else if PDC_IS_265(sc) {
        sc.sc_wdcdev.UDMA_cap.set(5);
    } else if PDC_IS_262(sc) {
        sc.sc_wdcdev.UDMA_cap.set(4);
    } else {
        sc.sc_wdcdev.UDMA_cap.set(2);
    }
    sc.sc_wdcdev.set_modes.set(Some(if PDC_IS_268(sc) {
        pdc20268_setup_channel
    } else {
        pdc202xx_setup_channel
    }));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);

    if PDC_IS_262(sc) {
        sc.sc_wdcdev.dma_start.set(Some(pdc20262_dma_start));
        sc.sc_wdcdev.dma_finish.set(Some(pdc20262_dma_finish));
    }

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);
    if !PDC_IS_268(sc) {
        // setup failsafe defaults
        let mut mode: u32 = 0;
        mode = pdc2xx_tim_set_pa(mode, PDC2XX_PA[0].into());
        mode = pdc2xx_tim_set_pb(mode, PDC2XX_PB[0].into());
        mode = pdc2xx_tim_set_mb(mode, PDC2XX_DMA_MB[0].into());
        mode = pdc2xx_tim_set_mc(mode, PDC2XX_DMA_MC[0].into());
        for channel in 0..sc.sc_wdcdev.nchannels.get() {
            wdcdebug_print!(
                wdcdebug_pciide_mask,
                DEBUG_PROBE,
                "pdc202xx_setup_chip: channel {} drive 0 initial timings  0x{:x}, now 0x{:x}\n",
                channel,
                pci_conf_read(pc, tag, pdc2xx_tim(channel, 0)),
                mode | PDC2xx_TIM_IORDYp
            );
            pci_conf_write(pc, tag, pdc2xx_tim(channel, 0), mode | PDC2xx_TIM_IORDYp);
            wdcdebug_print!(
                wdcdebug_pciide_mask,
                DEBUG_PROBE,
                "pdc202xx_setup_chip: channel {} drive 1 initial timings  0x{:x}, now 0x{:x}\n",
                channel,
                pci_conf_read(pc, tag, pdc2xx_tim(channel, 1)),
                mode
            );
            pci_conf_write(pc, tag, pdc2xx_tim(channel, 1), mode);
        }

        let mut mode = PDC2xx_SCR_DMA;
        if PDC_IS_262(sc) {
            mode = pdc2xx_scr_set_gen(mode, PDC262_SCR_GEN_LAT);
        } else {
            // the BIOS set it up this way
            mode = pdc2xx_scr_set_gen(mode, 0x1);
        }
        mode = pdc2xx_scr_set_i2c(mode, 0x3); // ditto
        mode = pdc2xx_scr_set_poll(mode, 0x1); // ditto
        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_PROBE,
            "pdc202xx_setup_chip: initial SCR  0x{:x}, now 0x{:x}\n",
            bus_space_read_4(sc.dma_iot(), sc.dma_ioh(), PDC2xx_SCR),
            mode
        );
        bus_space_write_4(sc.dma_iot(), sc.dma_ioh(), PDC2xx_SCR, mode);

        // controller initial state register is OK even without BIOS
        // Set DMA mode to IDE DMA compatibility
        let mode = bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), PDC2xx_PM);
        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_PROBE,
            "pdc202xx_setup_chip: primary mode 0x{:x}",
            mode
        );
        bus_space_write_1(sc.dma_iot(), sc.dma_ioh(), PDC2xx_PM, mode | 0x1);
        let mode = bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), PDC2xx_SM);
        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_PROBE,
            ", secondary mode 0x{:x}\n",
            mode
        );
        bus_space_write_1(sc.dma_iot(), sc.dma_ioh(), PDC2xx_SM, mode | 0x1);
    }

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }
        let state_en = if PDC_IS_262(sc) {
            pdc262_state_en(channel)
        } else {
            pdc246_state_en(channel)
        };
        if !PDC_IS_268(sc) && st & state_en == 0 {
            printf(format_args!(
                "{}: {} ignored (disabled)\n",
                sc.xname(),
                cp.name()
            ));
            cp.hw_ok.set(0);
            continue;
        }
        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        if PDC_IS_265(sc) {
            pciide_mapchan(
                pa,
                cp,
                interface,
                &mut cmdsize,
                &mut ctlsize,
                pdc20265_pci_intr,
            );
        } else {
            pciide_mapchan(
                pa,
                cp,
                interface,
                &mut cmdsize,
                &mut ctlsize,
                pdc202xx_pci_intr,
            );
        }
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }
        if !PDC_IS_268(sc) && pciide_chan_candisable(cp) {
            st &= !state_en;
            pciide_unmap_compat_intr(pa, cp, channel, interface);
        }
        if PDC_IS_268(sc) {
            pdc20268_setup_channel(&cp.wdc_channel);
        } else {
            pdc202xx_setup_channel(&cp.wdc_channel);
        }
    }
    if !PDC_IS_268(sc) {
        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_PROBE,
            "pdc202xx_setup_chip: new controller state 0x{:x}\n",
            st
        );
        pci_conf_write(pc, tag, PDC2xx_STATE, st);
    }
}

/// `pdc202xx_setup_channel`.
pub fn pdc202xx_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    let (pc, tag) = (sc.pc(), sc.tag());
    let d = &chp.ch_drive;

    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    let mut idedma_ctl: u32 = 0;
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "pdc202xx_setup_channel {}: scr 0x{:x}\n",
        sc.xname(),
        bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), PDC262_U66)
    );

    // Per channel settings
    if PDC_IS_262(sc) {
        let udma_above = |i: usize, m: u8| d[i].isset(DRIVE_UDMA) && d[i].UDMA_mode.get() > m;
        let udma_upto = |i: usize, m: u8| d[i].isset(DRIVE_UDMA) && d[i].UDMA_mode.get() <= m;
        let mut scr = u32::from(bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), PDC262_U66));
        let st = pci_conf_read(pc, tag, PDC2xx_STATE);
        // Check cable
        if st & pdc262_state_80p(channel) != 0 && (udma_above(0, 2) || udma_above(1, 2)) {
            wdcdebug_print!(
                wdcdebug_pciide_mask,
                DEBUG_PROBE,
                "{}:{}: 80-wire cable not detected\n",
                sc.xname(),
                channel
            );
            if d[0].UDMA_mode.get() > 2 {
                d[0].UDMA_mode.set(2);
            }
            if d[1].UDMA_mode.get() > 2 {
                d[1].UDMA_mode.set(2);
            }
        }
        // Trim UDMA mode
        if udma_upto(0, 2) || udma_upto(1, 2) {
            if d[0].UDMA_mode.get() > 2 {
                d[0].UDMA_mode.set(2);
            }
            if d[1].UDMA_mode.get() > 2 {
                d[1].UDMA_mode.set(2);
            }
        }
        // Set U66 if needed
        if udma_above(0, 2) || udma_above(1, 2) {
            scr |= pdc262_u66_en(channel);
        } else {
            scr &= !pdc262_u66_en(channel);
        }
        bus_space_write_1(sc.dma_iot(), sc.dma_ioh(), PDC262_U66, scr as u8);
        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_PROBE,
            "pdc202xx_setup_channel {}:{}: ATAPI 0x{:x}\n",
            sc.xname(),
            channel,
            bus_space_read_4(sc.dma_iot(), sc.dma_ioh(), pdc262_atapi(channel))
        );
        if d[0].isset(DRIVE_ATAPI) || d[1].isset(DRIVE_ATAPI) {
            let atapi =
                if (d[0].isset(DRIVE_UDMA) && !d[1].isset(DRIVE_UDMA) && d[1].isset(DRIVE_DMA))
                    || (d[1].isset(DRIVE_UDMA) && !d[0].isset(DRIVE_UDMA) && d[0].isset(DRIVE_DMA))
                {
                    0
                } else {
                    PDC262_ATAPI_UDMA
                };
            bus_space_write_4(sc.dma_iot(), sc.dma_ioh(), pdc262_atapi(channel), atapi);
        }
    }
    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        let mut mode: u32 = 0;
        if drvp.isset(DRIVE_UDMA) {
            // use Ultra/DMA
            drvp.clr(DRIVE_DMA);
            let udma = drvp.UDMA_mode.get() as usize;
            mode = pdc2xx_tim_set_mb(mode, PDC2XX_UDMA_MB[udma].into());
            mode = pdc2xx_tim_set_mc(mode, PDC2XX_UDMA_MC[udma].into());
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        } else if drvp.isset(DRIVE_DMA) {
            let dma = drvp.DMA_mode.get() as usize;
            mode = pdc2xx_tim_set_mb(mode, PDC2XX_DMA_MB[dma].into());
            mode = pdc2xx_tim_set_mc(mode, PDC2XX_DMA_MC[dma].into());
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        } else {
            mode = pdc2xx_tim_set_mb(mode, PDC2XX_DMA_MB[0].into());
            mode = pdc2xx_tim_set_mc(mode, PDC2XX_DMA_MC[0].into());
        }
        let pio = drvp.PIO_mode.get() as usize;
        mode = pdc2xx_tim_set_pa(mode, PDC2XX_PA[pio].into());
        mode = pdc2xx_tim_set_pb(mode, PDC2XX_PB[pio].into());
        if drvp.isset(DRIVE_ATA) {
            mode |= PDC2xx_TIM_PRE;
        }
        mode |= PDC2xx_TIM_SYNC | PDC2xx_TIM_ERRDY;
        if drvp.PIO_mode.get() >= 3 {
            mode |= PDC2xx_TIM_IORDY;
            if drive == 0 {
                mode |= PDC2xx_TIM_IORDYp;
            }
        }
        wdcdebug_print!(
            wdcdebug_pciide_mask,
            DEBUG_PROBE,
            "pdc202xx_setup_channel: {}:{}:{} timings 0x{:x}\n",
            sc.xname(),
            channel,
            drive,
            mode
        );
        pci_conf_write(pc, tag, pdc2xx_tim(channel, drive), mode);
    }
    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }
    pciide_print_modes(cp);
}

/// `pdc20268_setup_channel`.
pub fn pdc20268_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();

    // check 80 pins cable
    let cable = u32::from(pdc268_config_read(chp, 0x0b)) & PDC268_CABLE;

    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    let mut idedma_ctl: u32 = 0;

    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        if drvp.isset(DRIVE_UDMA) {
            // use Ultra/DMA
            drvp.clr(DRIVE_DMA);
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            if cable != 0 && drvp.UDMA_mode.get() > 2 {
                wdcdebug_print!(
                    wdcdebug_pciide_mask,
                    DEBUG_PROBE,
                    "{}({}:{}:{}): 80-wire cable not detected\n",
                    drvp.name(),
                    sc.xname(),
                    channel,
                    drive
                );
                drvp.UDMA_mode.set(2);
            }
        } else if drvp.isset(DRIVE_DMA) {
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        }
    }
    // nothing to do to setup modes, the controller snoop SET_FEATURE cmd
    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }
    pciide_print_modes(cp);
}

/// `pdc202xx_pci_intr`.
pub fn pdc202xx_pci_intr(arg: *mut c_void) -> i32 {
    // SAFETY: `pciide_mapregs_native` establishes this handler with the softc.
    let sc = unsafe { &*arg.cast_const().cast::<PciideSoftc>() };

    let mut rv = 0;
    let scr = bus_space_read_4(sc.dma_iot(), sc.dma_ioh(), PDC2xx_SCR);
    for i in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(i);
        let wdc_cp = &cp.wdc_channel;
        // If a compat channel skip.
        if cp.compat.get() != 0 {
            continue;
        }
        if scr & pdc2xx_scr_int(i) != 0 {
            let crv = wdcintr(ptr::from_ref(wdc_cp).cast_mut().cast());
            if crv == 0 {
                printf(format_args!(
                    "{}:{}: bogus intr (reg 0x{:x})\n",
                    sc.xname(),
                    i,
                    scr
                ));
            } else {
                rv = 1;
            }
        }
    }
    rv
}

/// `pdc20265_pci_intr`.
pub fn pdc20265_pci_intr(arg: *mut c_void) -> i32 {
    // SAFETY: `pciide_mapregs_native` establishes this handler with the softc.
    let sc = unsafe { &*arg.cast_const().cast::<PciideSoftc>() };

    let mut rv = 0;
    for i in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(i);
        let wdc_cp = &cp.wdc_channel;
        // If a compat channel skip.
        if cp.compat.get() != 0 {
            continue;
        }

        // In case of shared IRQ check that the interrupt was actually generated by this
        // channel. Only check the channel that is enabled.
        if cp.hw_ok.get() != 0
            && PDC_IS_268(sc)
            && u32::from(pdc268_config_read(wdc_cp, 0x0b)) & PDC268_INTR == 0
        {
            continue;
        }

        // The Ultra/100 seems to assert PDC2xx_SCR_INT * spuriously, however it asserts INT
        // in IDEDMA_CTL even for non-DMA ops. So use it instead (requires 2 reg reads
        // instead of 1, but we can't do it another way).
        let dmastat = bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), idedma_ctl_reg(i));
        if dmastat & IDEDMA_CTL_INTR == 0 {
            continue;
        }

        let crv = wdcintr(ptr::from_ref(wdc_cp).cast_mut().cast());
        if crv == 0 {
            printf(format_args!("{}:{}: bogus intr\n", sc.xname(), i));
        } else {
            rv = 1;
        }
    }
    rv
}

/// `pdc20262_dma_start`.
///
/// # Safety
///
/// As for [`pciide_dma_start`].
pub unsafe fn pdc20262_dma_start(v: *mut c_void, channel: i32, drive: i32) {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*v.cast_const().cast::<PciideSoftc>() };
    let dma_maps = sc.channel(channel).dma_maps(drive);

    if dma_maps.dma_flags.get() & WDC_DMA_LBA48 != 0 {
        let clock = bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), PDC262_U66);
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            PDC262_U66,
            clock | pdc262_u66_en(channel) as u8,
        );
        let (_, dmamap_xfer) = pciide_dma_maps(sc, dma_maps);
        let mut count = (dmamap_xfer.dm_mapsize.get() >> 1) as u32;
        count |= if dma_maps.dma_flags.get() & WDC_DMA_READ != 0 {
            PDC262_ATAPI_LBA48_READ
        } else {
            PDC262_ATAPI_LBA48_WRITE
        };
        bus_space_write_4(sc.dma_iot(), sc.dma_ioh(), pdc262_atapi(channel), count);
    }

    // SAFETY: forwarded.
    unsafe { pciide_dma_start(v, channel, drive) };
}

/// `pdc20262_dma_finish`.
///
/// # Safety
///
/// As for [`pciide_dma_finish`].
pub unsafe fn pdc20262_dma_finish(v: *mut c_void, channel: i32, drive: i32, force: i32) -> i32 {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*v.cast_const().cast::<PciideSoftc>() };
    let dma_maps = sc.channel(channel).dma_maps(drive);

    if dma_maps.dma_flags.get() & WDC_DMA_LBA48 != 0 {
        let clock = bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), PDC262_U66);
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            PDC262_U66,
            clock & !(pdc262_u66_en(channel) as u8),
        );
        bus_space_write_4(sc.dma_iot(), sc.dma_ioh(), pdc262_atapi(channel), 0);
    }

    // SAFETY: forwarded.
    unsafe { pciide_dma_finish(v, channel, drive, force) }
}

/// `(struct pciide_pdcsata *)sc->sc_cookie`. Panics if `sc` is not a Promise SATA
/// controller.
fn pdcsata(sc: &PciideSoftc) -> &PciidePdcsata {
    if !ptr::fn_addr_eq(sc.pp().chip_map, pdcsata_chip_map as PciideChipMap) {
        panic(format_args!(
            "{}: not a Promise SATA controller",
            sc.xname()
        ));
    }
    // SAFETY: `pdcsata_chip_map` sets `sc_cookie` to a `PciidePdcsata` before anything
    // reaches its hooks.
    unsafe { sc.cookie::<PciidePdcsata>() }
}

/// A handle of a Promise SATA channel's register map. Panics where the C would use an
/// unmapped one.
fn ps_ioh(h: &Cell<Option<BusSpaceHandle>>) -> BusSpaceHandle {
    match h.get() {
        Some(h) => h,
        None => panic(format_args!("pdcsata: register not mapped")),
    }
}

/// `pdcsata_chip_map`: Promise SATA controllers have 3 or 4 channels, the usual IDE
/// registers are mapped in I/O space, with offsets.
pub fn pdcsata_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    // Allocate memory for private data
    let ps = pciide_alloc_cookie(sc, PciidePdcsata::new());

    // Promise SATA controllers have 3 or 4 channels, the usual IDE registers are mapped in
    // I/O space, with offsets.
    let Some(intrhandle) = pci_intr_map(pa) else {
        printf(format_args!(": couldn't map interrupt\n"));
        return;
    };
    let intrstr = pci_intr_string(pa.pa_pc, intrhandle);

    let handler: fn(*mut c_void) -> i32 = match sc.pp().ide_product {
        PCI_PRODUCT_PROMISE_PDC40518
        | PCI_PRODUCT_PROMISE_PDC40519
        | PCI_PRODUCT_PROMISE_PDC40718
        | PCI_PRODUCT_PROMISE_PDC40719
        | PCI_PRODUCT_PROMISE_PDC40779
        | PCI_PRODUCT_PROMISE_PDC20571
        | PCI_PRODUCT_PROMISE_PDC20575
        | PCI_PRODUCT_PROMISE_PDC20579
        | PCI_PRODUCT_PROMISE_PDC20771
        | PCI_PRODUCT_PROMISE_PDC20775 => pdc205xx_pci_intr,
        // PDC20318, PDC20319, PDC20371, PDC20375..PDC20379 and any other
        _ => pdc203xx_pci_intr,
    };
    let ih = pci_intr_establish(
        pa.pa_pc,
        intrhandle,
        IPL_BIO,
        handler,
        ptr::from_ref(sc).cast_mut().cast(),
        sc.sc_wdcdev.sc_dev.xname(),
    );
    sc.sc_pci_ih
        .set(ih.map_or(ptr::null_mut(), |ih| ih.as_ptr()));

    let Some(pci_ih) = ih else {
        printf(format_args!(": couldn't establish native-PCI interrupt"));
        printf(format_args!(" at {}", intrstr));
        printf(format_args!("\n"));
        return;
    };

    let dmasize = match pci_mapreg_map(
        pa,
        PCIIDE_REG_BUS_MASTER_DMA,
        PCI_MAPREG_MEM_TYPE_32BIT,
        0,
        0,
    ) {
        Ok((iot, ioh, _, size)) => {
            sc.sc_dma_ok.set(1);
            sc.sc_dma_iot.set(Some(iot));
            sc.sc_dma_ioh.set(Some(ioh));
            size
        }
        Err(_) => {
            sc.sc_dma_ok.set(0);
            printf(format_args!(": couldn't map bus-master DMA registers\n"));
            // SAFETY: the handle established above, dropped here.
            unsafe { pci_intr_disestablish(pa.pa_pc, pci_ih) };
            return;
        }
    };

    sc.sc_dmat.set(Some(pa.pa_dmat));

    match pci_mapreg_map(
        pa,
        PDC203xx_BAR_IDEREGS as i32,
        PCI_MAPREG_MEM_TYPE_32BIT,
        0,
        0,
    ) {
        Ok((st, sh, _, _)) => {
            ps.ba5_st.set(Some(st));
            ps.ba5_sh.set(Some(sh));
        }
        Err(_) => {
            printf(format_args!(": couldn't map IDE registers\n"));
            bus_space_unmap(sc.dma_iot(), sc.dma_ioh(), dmasize);
            // SAFETY: the handle established above, dropped here.
            unsafe { pci_intr_disestablish(pa.pa_pc, pci_ih) };
            return;
        }
    }

    printf(format_args!(": DMA\n"));

    sc.sc_wdcdev.cap.set(WDC_CAPABILITY_DATA16);
    wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
    wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
    sc.sc_wdcdev.irqack.set(Some(pdc203xx_irqack));
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.UDMA_cap.set(6);
    sc.sc_wdcdev.set_modes.set(Some(pdc203xx_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));

    let (st, sh) = (ps.ba5_st(), ps.ba5_sh());
    match sc.pp().ide_product {
        PCI_PRODUCT_PROMISE_PDC40518
        | PCI_PRODUCT_PROMISE_PDC40519
        | PCI_PRODUCT_PROMISE_PDC40718
        | PCI_PRODUCT_PROMISE_PDC40719
        | PCI_PRODUCT_PROMISE_PDC40779
        | PCI_PRODUCT_PROMISE_PDC20571 => {
            bus_space_write_4(st, sh, 0x60, 0x00ff00ff);
            sc.sc_wdcdev.nchannels.set(PDC40718_NCHANNELS);

            sc.sc_wdcdev.reset.set(Some(pdc205xx_do_reset));
            sc.sc_wdcdev.drv_probe.set(Some(pdc205xx_drv_probe));
        }
        PCI_PRODUCT_PROMISE_PDC20575
        | PCI_PRODUCT_PROMISE_PDC20579
        | PCI_PRODUCT_PROMISE_PDC20771
        | PCI_PRODUCT_PROMISE_PDC20775 => {
            bus_space_write_4(st, sh, 0x60, 0x00ff00ff);
            sc.sc_wdcdev.nchannels.set(PDC20575_NCHANNELS);

            sc.sc_wdcdev.reset.set(Some(pdc205xx_do_reset));
            sc.sc_wdcdev.drv_probe.set(Some(pdc205xx_drv_probe));
        }
        // PDC20318, PDC20319, PDC20371, PDC20375..PDC20379 and any other
        _ => {
            bus_space_write_4(st, sh, 0x06c, 0x00ff0033);
            sc.sc_wdcdev
                .nchannels
                .set(if bus_space_read_4(st, sh, 0x48) & 0x02 != 0 {
                    PDC203xx_NCHANNELS
                } else {
                    3
                });
        }
    }

    sc.sc_wdcdev
        .dma_arg
        .set(ptr::from_ref(sc).cast_mut().cast());
    sc.sc_wdcdev.dma_init.set(Some(pciide_dma_init));
    sc.sc_wdcdev.dma_start.set(Some(pdc203xx_dma_start));
    sc.sc_wdcdev.dma_finish.set(Some(pdc203xx_dma_finish));

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        sc.wdc_chanarray[channel as usize].set(Some(NonNull::from(&cp.wdc_channel)));

        cp.ih.set(sc.sc_pci_ih.get());
        cp.name.set(None);
        cp.wdc_channel.channel.set(channel);
        cp.wdc_channel.wdc.set(Some(NonNull::from(&sc.sc_wdcdev)));
        cp.wdc_channel.ch_queue.set(wdc_alloc_queue());
        if cp.wdc_channel.ch_queue.get().is_none() {
            printf(format_args!(
                "{}: channel {}: cannot allocate channel queue\n",
                sc.xname(),
                channel
            ));
            continue;
        }
        let wdc_cp = &cp.wdc_channel;
        let regs = &ps.regs[channel as usize];
        let chan_off = (channel as BusSize) << 7;

        regs.ctl_iot.set(Some(st));
        regs.cmd_iot.set(Some(st));

        match bus_space_subregion(st, sh, 0x0238 + chan_off, 1) {
            Ok(h) => regs.ctl_ioh.set(Some(h)),
            Err(_) => {
                printf(format_args!(
                    "{}: couldn't map channel {} ctl regs\n",
                    sc.xname(),
                    channel
                ));
                continue;
            }
        }
        let mut mapped = true;
        for i in 0..WDC_NREG as usize {
            match bus_space_subregion(
                st,
                sh,
                0x0200 + (i << 2) + chan_off,
                if i == 0 { 4 } else { 1 },
            ) {
                Ok(h) => regs.cmd_iohs[i].set(Some(h)),
                Err(_) => {
                    printf(format_args!(
                        "{}: couldn't map channel {} cmd regs\n",
                        sc.xname(),
                        channel
                    ));
                    mapped = false;
                    break;
                }
            }
        }
        if !mapped {
            // goto loop_end
            continue;
        }
        regs.cmd_iohs[wdr_status.offset()].set(regs.cmd_iohs[wdr_command.offset()].get());
        regs.cmd_iohs[wdr_features.offset()].set(regs.cmd_iohs[wdr_error.offset()].get());
        wdc_cp.cmd_iot.set(regs.cmd_iot.get());
        wdc_cp.data32iot.set(regs.cmd_iot.get());
        wdc_cp.cmd_ioh.set(regs.cmd_iohs[0].get());
        wdc_cp.data32ioh.set(regs.cmd_iohs[0].get());
        wdc_cp._vtbl.set(Some(&WDC_PDC203XX_VTBL));

        // Subregion de busmaster registers. They're spread all over the controller's
        // register space :(. They are also 4 bytes sized, with some specific extensions in
        // the extra bits. It also seems that the IDEDMA_CTL register isn't available.
        match bus_space_subregion(st, sh, 0x260 + chan_off, 1) {
            Ok(h) => regs.dma_iohs[idedma_cmd(0)].set(Some(h)),
            Err(_) => {
                printf(format_args!(
                    "{} channel {}: can't subregion DMA registers\n",
                    sc.xname(),
                    channel
                ));
                continue;
            }
        }
        match bus_space_subregion(st, sh, 0x244 + chan_off, 4) {
            Ok(h) => regs.dma_iohs[idedma_tbl(0)].set(Some(h)),
            Err(_) => {
                printf(format_args!(
                    "{} channel {}: can't subregion DMA registers\n",
                    sc.xname(),
                    channel
                ));
                continue;
            }
        }

        wdcattach(wdc_cp);
        let cmd = ps_ioh(&regs.dma_iohs[idedma_cmd(0)]);
        bus_space_write_4(
            sc.dma_iot(),
            cmd,
            0,
            (bus_space_read_4(sc.dma_iot(), cmd, 0) & !0x00003f9f) | (channel as u32 + 1),
        );
        bus_space_write_4(st, sh, ((channel as BusSize) + 1) << 2, 0x00000001);

        pdc203xx_setup_channel(&cp.wdc_channel);
        // loop_end:
    }

    printf(format_args!(
        "{}: using {} for native-PCI interrupt\n",
        sc.xname(),
        intrstr
    ));
}

/// `pdc203xx_setup_channel`.
pub fn pdc203xx_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);

    pciide_channel_dma_setup(cp);

    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive];
        if !drvp.isset(DRIVE) {
            continue;
        }
        if drvp.isset(DRIVE_UDMA) {
            let s = splbio();
            drvp.clr(DRIVE_DMA);
            splx(s);
        }
    }
    pciide_print_modes(cp);
}

/// `pdc203xx_pci_intr`.
pub fn pdc203xx_pci_intr(arg: *mut c_void) -> i32 {
    // SAFETY: `pdcsata_chip_map` establishes this handler with the softc.
    let sc = unsafe { &*arg.cast_const().cast::<PciideSoftc>() };
    let ps = pdcsata(sc);

    let mut rv = 0;
    let scr = bus_space_read_4(ps.ba5_st(), ps.ba5_sh(), 0x00040);

    for i in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(i);
        let wdc_cp = &cp.wdc_channel;
        if scr & (1 << (i + 1)) != 0 {
            let crv = wdcintr(ptr::from_ref(wdc_cp).cast_mut().cast());
            if crv == 0 {
                printf(format_args!(
                    "{}:{}: bogus intr (reg 0x{:x})\n",
                    sc.xname(),
                    i,
                    scr
                ));
            } else {
                rv = 1;
            }
        }
    }

    rv
}

/// `pdc205xx_pci_intr`.
pub fn pdc205xx_pci_intr(arg: *mut c_void) -> i32 {
    // SAFETY: `pdcsata_chip_map` establishes this handler with the softc.
    let sc = unsafe { &*arg.cast_const().cast::<PciideSoftc>() };
    let ps = pdcsata(sc);
    let (st, sh) = (ps.ba5_st(), ps.ba5_sh());

    let mut rv = 0;
    let scr = bus_space_read_4(st, sh, 0x40);
    bus_space_write_4(st, sh, 0x40, scr & 0x0000ffff);

    let status = bus_space_read_4(st, sh, 0x60);
    bus_space_write_4(st, sh, 0x60, status & 0x000000ff);

    for i in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(i);
        let wdc_cp = &cp.wdc_channel;
        if scr & (1 << (i + 1)) != 0 {
            let crv = wdcintr(ptr::from_ref(wdc_cp).cast_mut().cast());
            if crv == 0 {
                printf(format_args!(
                    "{}:{}: bogus intr (reg 0x{:x})\n",
                    sc.xname(),
                    i,
                    scr
                ));
            } else {
                rv = 1;
            }
        }
    }
    rv
}

/// `pdc203xx_irqack`.
pub fn pdc203xx_irqack(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let ps = pdcsata(sc);
    let chan = chp.channel.get();
    let cmd = ps_ioh(&ps.regs[chan as usize].dma_iohs[idedma_cmd(0)]);

    bus_space_write_4(
        sc.dma_iot(),
        cmd,
        0,
        (bus_space_read_4(sc.dma_iot(), cmd, 0) & !0x00003f9f) | (chan as u32 + 1),
    );
    bus_space_write_4(
        ps.ba5_st(),
        ps.ba5_sh(),
        ((chan as BusSize) + 1) << 2,
        0x00000001,
    );
}

/// `pdc203xx_dma_start`.
///
/// # Safety
///
/// `v` is the softc `pdcsata_chip_map` installed as `dma_arg`.
pub unsafe fn pdc203xx_dma_start(v: *mut c_void, channel: i32, drive: i32) {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*v.cast_const().cast::<PciideSoftc>() };
    let cp = sc.channel(channel);
    let dma_maps = cp.dma_maps(drive);
    let ps = pdcsata(sc);
    let regs = &ps.regs[channel as usize];
    let (dmamap_table, _) = pciide_dma_maps(sc, dma_maps);

    // Write table address
    bus_space_write_4(
        sc.dma_iot(),
        ps_ioh(&regs.dma_iohs[idedma_tbl(0)]),
        0,
        dmamap_table.dm_segs()[0].get().ds_addr as u32,
    );

    // Start DMA engine
    let cmd = ps_ioh(&regs.dma_iohs[idedma_cmd(0)]);
    bus_space_write_4(
        sc.dma_iot(),
        cmd,
        0,
        (bus_space_read_4(sc.dma_iot(), cmd, 0) & !0xc0)
            | if dma_maps.dma_flags.get() & WDC_DMA_READ != 0 {
                0x80
            } else {
                0xc0
            },
    );
}

/// `pdc203xx_dma_finish`.
///
/// # Safety
///
/// `v` is the softc `pdcsata_chip_map` installed as `dma_arg`.
pub unsafe fn pdc203xx_dma_finish(v: *mut c_void, channel: i32, drive: i32, _force: i32) -> i32 {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*v.cast_const().cast::<PciideSoftc>() };
    let cp = sc.channel(channel);
    let dma_maps = cp.dma_maps(drive);
    let ps = pdcsata(sc);
    let (_, dmamap_xfer) = pciide_dma_maps(sc, dma_maps);

    // Stop DMA channel
    let cmd = ps_ioh(&ps.regs[channel as usize].dma_iohs[idedma_cmd(0)]);
    bus_space_write_4(
        sc.dma_iot(),
        cmd,
        0,
        bus_space_read_4(sc.dma_iot(), cmd, 0) & !0x80,
    );

    // Unload the map of the data buffer
    bus_dmamap_sync(
        sc.dmat(),
        dmamap_xfer,
        0,
        dmamap_xfer.dm_mapsize.get(),
        if dma_maps.dma_flags.get() & WDC_DMA_READ != 0 {
            BUS_DMASYNC_POSTREAD
        } else {
            BUS_DMASYNC_POSTWRITE
        },
    );
    bus_dmamap_unload(sc.dmat(), dmamap_xfer);

    0
}

/// A tag of a Promise SATA channel's register map.
fn ps_iot(t: &Cell<Option<BusSpaceTag>>) -> BusSpaceTag {
    match t.get() {
        Some(t) => t,
        None => panic(format_args!("pdcsata: register space not mapped")),
    }
}

/// `pdc203xx_read_reg`.
pub fn pdc203xx_read_reg(chp: &ChannelSoftc, reg: WdcRegs) -> u8 {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let regs = &pdcsata(sc).regs[chp.channel.get() as usize];

    if reg.isset(_WDC_AUX) {
        bus_space_read_1(ps_iot(&regs.ctl_iot), ps_ioh(&regs.ctl_ioh), reg.offset())
    } else {
        bus_space_read_1(
            ps_iot(&regs.cmd_iot),
            ps_ioh(&regs.cmd_iohs[reg.offset()]),
            0,
        )
    }
}

/// `pdc203xx_write_reg`.
pub fn pdc203xx_write_reg(chp: &ChannelSoftc, reg: WdcRegs, val: u8) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let regs = &pdcsata(sc).regs[chp.channel.get() as usize];

    if reg.isset(_WDC_AUX) {
        bus_space_write_1(
            ps_iot(&regs.ctl_iot),
            ps_ioh(&regs.ctl_ioh),
            reg.offset(),
            val,
        );
    } else {
        bus_space_write_1(
            ps_iot(&regs.cmd_iot),
            ps_ioh(&regs.cmd_iohs[reg.offset()]),
            0,
            val,
        );
    }
}

/// `pdc205xx_do_reset`.
pub fn pdc205xx_do_reset(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let ps = pdcsata(sc);
    let channel = chp.channel.get();

    wdc_do_reset(chp);

    // reset SATA
    let mut scontrol = SControl_DET_INIT | SControl_SPD_ANY | SControl_IPM_NONE;
    scontrol_write(ps, channel, scontrol);
    delay(50 * 1000);

    scontrol &= !SControl_DET_INIT;
    scontrol_write(ps, channel, scontrol);
    delay(50 * 1000);
}

/// `pdc205xx_drv_probe`.
pub fn pdc205xx_drv_probe(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let ps = pdcsata(sc);
    let channel = chp.channel.get();

    scontrol_write(ps, channel, 0);
    delay(50 * 1000);

    let mut scontrol = SControl_DET_INIT | SControl_SPD_ANY | SControl_IPM_NONE;
    scontrol_write(ps, channel, scontrol);
    delay(50 * 1000);

    scontrol &= !SControl_DET_INIT;
    scontrol_write(ps, channel, scontrol);
    delay(50 * 1000);

    let sstatus = sstatus_read(ps, channel);

    match sstatus & SStatus_DET_mask {
        SStatus_DET_NODEV => {
            // No Device; be silent.
        }
        SStatus_DET_DEV_NE => {
            printf(format_args!(
                "{}: port {}: device connected, but communication not established\n",
                sc.xname(),
                channel
            ));
        }
        SStatus_DET_OFFLINE => {
            printf(format_args!(
                "{}: port {}: PHY offline\n",
                sc.xname(),
                channel
            ));
        }
        SStatus_DET_DEV => {
            let iohs = &ps.regs[channel as usize].cmd_iohs;
            let iot = chp.cmd_iot();
            bus_space_write_1(iot, ps_ioh(&iohs[wdr_sdh.0 as usize]), 0, WDSD_IBM);
            delay(10); // 400ns delay
            let scnt = bus_space_read_2(iot, ps_ioh(&iohs[wdr_seccnt.0 as usize]), 0);
            let sn = bus_space_read_2(iot, ps_ioh(&iohs[wdr_sector.0 as usize]), 0);
            let cl = bus_space_read_2(iot, ps_ioh(&iohs[wdr_cyl_lo.0 as usize]), 0);
            let ch = bus_space_read_2(iot, ps_ioh(&iohs[wdr_cyl_hi.0 as usize]), 0);
            // #if 0: printf("%s: port %d: scnt=0x%x sn=0x%x cl=0x%x ch=0x%x\n", ...)
            let _ = (scnt, sn);
            // scnt and sn are supposed to be 0x1 for ATAPI, but in some cases we get wrong
            // values here, so ignore it.
            let s = splbio();
            if cl == 0x14 && ch == 0xeb {
                chp.ch_drive[0].set(DRIVE_ATAPI);
            } else {
                chp.ch_drive[0].set(DRIVE_ATA);
            }
            splx(s);
            // #if 0: printf("%s: port %d", ...) and the link speed
        }
        _ => {
            printf(format_args!(
                "{}: port {}: unknown SStatus: 0x{:08x}\n",
                sc.xname(),
                channel,
                sstatus
            ));
        }
    }
}

/// `serverworks_chip_map`.
pub fn serverworks_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let interface = pci_interface(pa.pa_class);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);
    printf(format_args!("\n"));
    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);

    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    match sc.pp().ide_product {
        PCI_PRODUCT_RCC_OSB4_IDE => sc.sc_wdcdev.UDMA_cap.set(2),
        PCI_PRODUCT_RCC_CSB5_IDE => {
            if sc.sc_rev.get() < 0x92 {
                sc.sc_wdcdev.UDMA_cap.set(4);
            } else {
                sc.sc_wdcdev.UDMA_cap.set(5);
            }
        }
        PCI_PRODUCT_RCC_CSB6_IDE => sc.sc_wdcdev.UDMA_cap.set(4),
        PCI_PRODUCT_RCC_CSB6_RAID_IDE | PCI_PRODUCT_RCC_HT_1000_IDE => {
            sc.sc_wdcdev.UDMA_cap.set(5);
        }
        _ => {}
    }

    sc.sc_wdcdev.set_modes.set(Some(serverworks_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev
        .nchannels
        .set(if sc.pp().ide_product == PCI_PRODUCT_RCC_CSB6_IDE {
            1
        } else {
            2
        });

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }
        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            serverworks_pci_intr,
        );
        if cp.hw_ok.get() == 0 {
            return;
        }
        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            return;
        }
        serverworks_setup_channel(&cp.wdc_channel);
    }

    let pcib_tag = pci_make_tag(pa.pa_pc, pa.pa_bus as i32, pa.pa_device as i32, 0);
    pci_conf_write(
        pa.pa_pc,
        pcib_tag,
        0x64,
        (pci_conf_read(pa.pa_pc, pcib_tag, 0x64) & !0x2000) | 0x4000,
    );
}

/// `serverworks_setup_channel`.
pub fn serverworks_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    let (pc, tag) = (sc.pc(), sc.tag());
    const PIO_MODES: [u8; 5] = [0x5d, 0x47, 0x34, 0x22, 0x20];
    const DMA_MODES: [u8; 3] = [0x77, 0x21, 0x20];

    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    let mut pio_time = pci_conf_read(pc, tag, 0x40);
    let mut dma_time = pci_conf_read(pc, tag, 0x44);
    let mut pio_mode = pci_conf_read(pc, tag, 0x48);
    let mut udma_mode = pci_conf_read(pc, tag, 0x54);

    pio_time &= !(0xffff << (16 * channel));
    dma_time &= !(0xffff << (16 * channel));
    pio_mode &= !(0xff << (8 * channel + 16));
    udma_mode &= !(0xff << (8 * channel + 16));
    udma_mode &= !(3 << (2 * channel));

    let mut idedma_ctl: u32 = 0;

    // Per drive settings
    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        let unit = drive + 2 * channel;
        // add timing values, setup DMA if needed
        pio_time |= u32::from(PIO_MODES[drvp.PIO_mode.get() as usize]) << (8 * (unit ^ 1));
        pio_mode |= u32::from(drvp.PIO_mode.get()) << (4 * unit + 16);
        if chp.wdc().has(WDC_CAPABILITY_UDMA) && drvp.isset(DRIVE_UDMA) {
            // use Ultra/DMA, check for 80-pin cable
            if sc.sc_rev.get() <= 0x92
                && drvp.UDMA_mode.get() > 2
                && pci_product(pci_conf_read(pc, tag, PCI_SUBSYS_ID_REG)) & (1 << (14 + channel))
                    == 0
            {
                wdcdebug_print!(
                    wdcdebug_pciide_mask,
                    DEBUG_PROBE,
                    "{}({}:{}:{}): 80-wire cable not detected\n",
                    drvp.name(),
                    sc.xname(),
                    channel,
                    drive
                );
                drvp.UDMA_mode.set(2);
            }
            dma_time |= u32::from(DMA_MODES[drvp.DMA_mode.get() as usize]) << (8 * (unit ^ 1));
            udma_mode |= u32::from(drvp.UDMA_mode.get()) << (4 * unit + 16);
            udma_mode |= 1 << unit;
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        } else if chp.wdc().has(WDC_CAPABILITY_DMA) && drvp.isset(DRIVE_DMA) {
            // use Multiword DMA
            drvp.clr(DRIVE_UDMA);
            dma_time |= u32::from(DMA_MODES[drvp.DMA_mode.get() as usize]) << (8 * (unit ^ 1));
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        } else {
            // PIO only
            drvp.clr(DRIVE_UDMA | DRIVE_DMA);
        }
    }

    pci_conf_write(pc, tag, 0x40, pio_time);
    pci_conf_write(pc, tag, 0x44, dma_time);
    if sc.pp().ide_product != PCI_PRODUCT_RCC_OSB4_IDE {
        pci_conf_write(pc, tag, 0x48, pio_mode);
    }
    pci_conf_write(pc, tag, 0x54, udma_mode);

    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }
    pciide_print_modes(cp);
}

/// `serverworks_pci_intr`.
pub fn serverworks_pci_intr(arg: *mut c_void) -> i32 {
    // SAFETY: `pciide_mapregs_native` establishes this handler with the softc.
    let sc = unsafe { &*arg.cast_const().cast::<PciideSoftc>() };
    let mut rv = 0;

    for i in 0..sc.sc_wdcdev.nchannels.get() {
        let dmastat = bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), idedma_ctl_reg(i));
        if dmastat & (IDEDMA_CTL_ACT | IDEDMA_CTL_INTR) != IDEDMA_CTL_INTR {
            continue;
        }
        let cp = sc.channel(i);
        let wdc_cp = &cp.wdc_channel;
        let crv = wdcintr(ptr::from_ref(wdc_cp).cast_mut().cast());
        if crv == 0 {
            printf(format_args!("{}:{}: bogus intr\n", sc.xname(), i));
            bus_space_write_1(sc.dma_iot(), sc.dma_ioh(), idedma_ctl_reg(i), dmastat);
        } else {
            rv = 1;
        }
    }
    rv
}

/// `(struct pciide_svwsata *)sc->sc_cookie`. Panics if `sc` is not a ServerWorks SATA
/// controller.
fn svwsata(sc: &PciideSoftc) -> &PciideSvwsata {
    if !ptr::fn_addr_eq(sc.pp().chip_map, svwsata_chip_map as PciideChipMap) {
        panic(format_args!(
            "{}: not a ServerWorks SATA controller",
            sc.xname()
        ));
    }
    // SAFETY: `svwsata_chip_map` sets `sc_cookie` to a `PciideSvwsata` before anything
    // reaches its hooks.
    unsafe { sc.cookie::<PciideSvwsata>() }
}

/// `svwsata_chip_map`.
pub fn svwsata_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    // Allocate memory for private data
    let ss = pciide_alloc_cookie(sc, PciideSvwsata::new());

    // The 4-port version has a dummy second function.
    if pci_conf_read(sc.pc(), sc.tag(), PCI_MAPREG_START + 0x14) == 0 {
        printf(format_args!("\n"));
        return;
    }

    match pci_mapreg_map(
        pa,
        PCI_MAPREG_START + 0x14,
        PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_32BIT,
        0,
        0,
    ) {
        Ok((st, sh, _, _)) => {
            ss.ba5_st.set(Some(st));
            ss.ba5_sh.set(Some(sh));
        }
        Err(_) => {
            printf(format_args!(": unable to map BA5 register space\n"));
            return;
        }
    }

    printf(format_args!(": DMA"));
    svwsata_mapreg_dma(sc, pa);
    printf(format_args!("\n"));

    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(
            sc,
            WDC_CAPABILITY_UDMA | WDC_CAPABILITY_DMA | WDC_CAPABILITY_IRQACK,
        );
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.UDMA_cap.set(6);

    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(4);
    wdc_cap_set(
        sc,
        WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE | WDC_CAPABILITY_SATA,
    );
    sc.sc_wdcdev.set_modes.set(Some(sata_setup_channel));

    // We can use SControl and SStatus to probe for drives.
    sc.sc_wdcdev.drv_probe.set(Some(svwsata_drv_probe));

    // Map and establish the interrupt handler.
    let Some(intrhandle) = pci_intr_map(pa) else {
        printf(format_args!(
            "{}: couldn't map native-PCI interrupt\n",
            sc.xname()
        ));
        return;
    };
    let intrstr = pci_intr_string(pa.pa_pc, intrhandle);
    let ih = pci_intr_establish(
        pa.pa_pc,
        intrhandle,
        IPL_BIO,
        pciide_pci_intr,
        ptr::from_ref(sc).cast_mut().cast(),
        sc.sc_wdcdev.sc_dev.xname(),
    );
    sc.sc_pci_ih
        .set(ih.map_or(ptr::null_mut(), |ih| ih.as_ptr()));
    if ih.is_some() {
        printf(format_args!(
            "{}: using {} for native-PCI interrupt\n",
            sc.xname(),
            intrstr
        ));
    } else {
        printf(format_args!(
            "{}: couldn't establish native-PCI interrupt",
            sc.xname()
        ));
        printf(format_args!(" at {}", intrstr));
        printf(format_args!("\n"));
        return;
    }

    if sc.pp().ide_product == PCI_PRODUCT_RCC_K2_SATA {
        let (st, sh) = (ss.ba5_st(), ss.ba5_sh());
        bus_space_write_4(
            st,
            sh,
            SVWSATA_SICR1,
            bus_space_read_4(st, sh, SVWSATA_SICR1) & !0x00040000,
        );
        bus_space_write_4(st, sh, SVWSATA_SIM, 0);
    }

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, 0) {
            continue;
        }
        svwsata_mapchan(cp);
        sata_setup_channel(&cp.wdc_channel);
    }
}

/// `svwsata_mapreg_dma`.
pub fn svwsata_mapreg_dma(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let ss = svwsata(sc);

    sc.sc_wdcdev
        .dma_arg
        .set(ptr::from_ref(sc).cast_mut().cast());
    sc.sc_wdcdev.dma_init.set(Some(pciide_dma_init));
    sc.sc_wdcdev.dma_start.set(Some(pciide_dma_start));
    sc.sc_wdcdev.dma_finish.set(Some(pciide_dma_finish));

    // XXX
    sc.sc_dma_iot.set(ss.ba5_st.get());
    sc.sc_dma_ioh.set(ss.ba5_sh.get());

    sc.sc_dmacmd_read.set(Some(svwsata_dmacmd_read));
    sc.sc_dmacmd_write.set(Some(svwsata_dmacmd_write));
    sc.sc_dmactl_read.set(Some(svwsata_dmactl_read));
    sc.sc_dmactl_write.set(Some(svwsata_dmactl_write));
    sc.sc_dmatbl_write.set(Some(svwsata_dmatbl_write));

    // DMA registers all set up!
    sc.sc_dmat.set(Some(pa.pa_dmat));
    sc.sc_dma_ok.set(1);
}

/// `(chan << 8) + SVWSATA_DMA + reg`.
const fn svwsata_dma_reg(chan: i32, reg: BusSize) -> BusSize {
    ((chan as BusSize) << 8) + SVWSATA_DMA + reg
}

/// `svwsata_dmacmd_read`.
pub fn svwsata_dmacmd_read(sc: &PciideSoftc, chan: i32) -> u8 {
    bus_space_read_1(
        sc.dma_iot(),
        sc.dma_ioh(),
        svwsata_dma_reg(chan, idedma_cmd(0)),
    )
}

/// `svwsata_dmacmd_write`.
pub fn svwsata_dmacmd_write(sc: &PciideSoftc, chan: i32, val: u8) {
    bus_space_write_1(
        sc.dma_iot(),
        sc.dma_ioh(),
        svwsata_dma_reg(chan, idedma_cmd(0)),
        val,
    );
}

/// `svwsata_dmactl_read`.
pub fn svwsata_dmactl_read(sc: &PciideSoftc, chan: i32) -> u8 {
    bus_space_read_1(
        sc.dma_iot(),
        sc.dma_ioh(),
        svwsata_dma_reg(chan, idedma_ctl_reg(0)),
    )
}

/// `svwsata_dmactl_write`.
pub fn svwsata_dmactl_write(sc: &PciideSoftc, chan: i32, val: u8) {
    bus_space_write_1(
        sc.dma_iot(),
        sc.dma_ioh(),
        svwsata_dma_reg(chan, idedma_ctl_reg(0)),
        val,
    );
}

/// `svwsata_dmatbl_write`.
pub fn svwsata_dmatbl_write(sc: &PciideSoftc, chan: i32, val: u32) {
    bus_space_write_4(
        sc.dma_iot(),
        sc.dma_ioh(),
        svwsata_dma_reg(chan, idedma_tbl(0)),
        val,
    );
}

/// `svwsata_mapchan`.
pub fn svwsata_mapchan(cp: &'static PciideChannel) {
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let wdc_cp = &cp.wdc_channel;
    let ss = svwsata(sc);
    let chan_off = (wdc_cp.channel.get() as BusSize) << 8;

    cp.compat.set(0);
    cp.ih.set(sc.sc_pci_ih.get());

    match bus_space_subregion(
        ss.ba5_st(),
        ss.ba5_sh(),
        chan_off + SVWSATA_TF0 as BusSize,
        (SVWSATA_TF8 - SVWSATA_TF0) as BusSize,
    ) {
        Ok(h) => wdc_cp.cmd_ioh.set(Some(h)),
        Err(_) => {
            printf(format_args!(
                "{}: couldn't map {} cmd regs\n",
                sc.xname(),
                cp.name()
            ));
            return;
        }
    }
    match bus_space_subregion(
        ss.ba5_st(),
        ss.ba5_sh(),
        chan_off + SVWSATA_TF8 as BusSize,
        4,
    ) {
        Ok(h) => wdc_cp.ctl_ioh.set(Some(h)),
        Err(_) => {
            printf(format_args!(
                "{}: couldn't map {} ctl regs\n",
                sc.xname(),
                cp.name()
            ));
            return;
        }
    }
    wdc_cp.cmd_iot.set(ss.ba5_st.get());
    wdc_cp.ctl_iot.set(ss.ba5_st.get());
    wdc_cp._vtbl.set(Some(&WDC_SVWSATA_VTBL));
    wdc_cp.set(WDCF_DMA_BEFORE_CMD);
    wdcattach(wdc_cp);
}

/// `svwsata_drv_probe`.
pub fn svwsata_drv_probe(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let ss = svwsata(sc);
    let channel = chp.channel.get();
    let chan_off = (channel as BusSize) << 8;
    let (st, sh) = (ss.ba5_st(), ss.ba5_sh());

    // Request communication initialization sequence, any speed. Performing this is the
    // equivalent of an ATA Reset.
    let mut scontrol = SControl_DET_INIT | SControl_SPD_ANY;

    // XXX We don't yet support SATA power management; disable all power management state
    // transitions.
    scontrol |= SControl_IPM_NONE;

    bus_space_write_4(st, sh, chan_off + SVWSATA_SCONTROL, scontrol);
    delay(50 * 1000);
    scontrol &= !SControl_DET_INIT;
    bus_space_write_4(st, sh, chan_off + SVWSATA_SCONTROL, scontrol);
    delay(100 * 1000);

    let sstatus = bus_space_read_4(st, sh, chan_off + SVWSATA_SSTATUS);
    // #if 0: printf("%s: port %d: SStatus=0x%08x, SControl=0x%08x\n", ...)
    match sstatus & SStatus_DET_mask {
        SStatus_DET_NODEV => {
            // No device; be silent.
        }
        SStatus_DET_DEV_NE => {
            printf(format_args!(
                "{}: port {}: device connected, but communication not established\n",
                sc.xname(),
                channel
            ));
        }
        SStatus_DET_OFFLINE => {
            printf(format_args!(
                "{}: port {}: PHY offline\n",
                sc.xname(),
                channel
            ));
        }
        SStatus_DET_DEV => {
            // XXX ATAPI detection doesn't currently work. Don't XXX know why. But, it's not
            // like the standard method XXX can detect an ATAPI device connected via a
            // SATA/PATA XXX bridge, so at least this is no worse. --thorpej
            if chp._vtbl.get().is_some() {
                chp.write_reg(wdr_sdh, WDSD_IBM);
            } else {
                bus_space_write_1(chp.cmd_iot(), chp.cmd_ioh(), wdr_sdh.offset(), WDSD_IBM);
            }
            delay(10); // 400ns delay
            // Save register contents.
            let (scnt, sn, cl, ch);
            if chp._vtbl.get().is_some() {
                scnt = chp.read_reg(wdr_seccnt);
                sn = chp.read_reg(wdr_sector);
                cl = chp.read_reg(wdr_cyl_lo);
                ch = chp.read_reg(wdr_cyl_hi);
            } else {
                let (t, h) = (chp.cmd_iot(), chp.cmd_ioh());
                scnt = bus_space_read_1(t, h, wdr_seccnt.offset());
                sn = bus_space_read_1(t, h, wdr_sector.offset());
                cl = bus_space_read_1(t, h, wdr_cyl_lo.offset());
                ch = bus_space_read_1(t, h, wdr_cyl_hi.offset());
            }
            // #if 0: printf("%s: port %d: scnt=0x%x sn=0x%x cl=0x%x ch=0x%x\n", ...)
            let _ = (scnt, sn);
            // scnt and sn are supposed to be 0x1 for ATAPI, but in some cases we get wrong
            // values here, so ignore it.
            let s = splbio();
            if cl == 0x14 && ch == 0xeb {
                chp.ch_drive[0].set(DRIVE_ATAPI);
            } else {
                chp.ch_drive[0].set(DRIVE_ATA);
            }
            splx(s);

            printf(format_args!("{}: port {}", sc.xname(), channel));
            match (sstatus & SStatus_SPD_mask) >> SStatus_SPD_shift {
                1 => {
                    printf(format_args!(": 1.5Gb/s"));
                }
                2 => {
                    printf(format_args!(": 3.0Gb/s"));
                }
                _ => {}
            }
            printf(format_args!("\n"));
        }
        _ => {
            printf(format_args!(
                "{}: port {}: unknown SStatus: 0x{:08x}\n",
                sc.xname(),
                channel,
                sstatus
            ));
        }
    }
}

/// `svwsata_read_reg`: the task file registers are 32 bits apart.
pub fn svwsata_read_reg(chp: &ChannelSoftc, reg: WdcRegs) -> u8 {
    if reg.isset(_WDC_AUX) {
        bus_space_read_4(chp.ctl_iot(), chp.ctl_ioh(), reg.offset() << 2) as u8
    } else {
        bus_space_read_4(chp.cmd_iot(), chp.cmd_ioh(), reg.offset() << 2) as u8
    }
}

/// `svwsata_write_reg`.
pub fn svwsata_write_reg(chp: &ChannelSoftc, reg: WdcRegs, val: u8) {
    if reg.isset(_WDC_AUX) {
        bus_space_write_4(chp.ctl_iot(), chp.ctl_ioh(), reg.offset() << 2, val.into());
    } else {
        bus_space_write_4(chp.cmd_iot(), chp.cmd_ioh(), reg.offset() << 2, val.into());
    }
}

/// `svwsata_lba48_write_reg`.
pub fn svwsata_lba48_write_reg(chp: &ChannelSoftc, reg: WdcRegs, val: u16) {
    if reg.isset(_WDC_AUX) {
        bus_space_write_4(chp.ctl_iot(), chp.ctl_ioh(), reg.offset() << 2, val.into());
    } else {
        bus_space_write_4(chp.cmd_iot(), chp.cmd_ioh(), reg.offset() << 2, val.into());
    }
}

/// `ACARD_IS_850(sc)`.
#[allow(non_snake_case)] // the C macro's name
fn ACARD_IS_850(sc: &PciideSoftc) -> bool {
    sc.pp().ide_product == PCI_PRODUCT_ACARD_ATP850U
}

/// `acard_chip_map`.
pub fn acard_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    // when the chip is in native mode it identifies itself as a 'misc mass storage'. Fake
    // interface in this case.
    let interface = if pci_subclass(pa.pa_class) == PCI_SUBCLASS_MASS_STORAGE_IDE {
        pci_interface(pa.pa_class)
    } else {
        PCIIDE_INTERFACE_BUS_MASTER_DMA | pciide_interface_pci(0) | pciide_interface_pci(1)
    };

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);
    printf(format_args!("\n"));
    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);

    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    match sc.pp().ide_product {
        PCI_PRODUCT_ACARD_ATP850U => sc.sc_wdcdev.UDMA_cap.set(2),
        PCI_PRODUCT_ACARD_ATP860 | PCI_PRODUCT_ACARD_ATP860A => sc.sc_wdcdev.UDMA_cap.set(4),
        PCI_PRODUCT_ACARD_ATP865A | PCI_PRODUCT_ACARD_ATP865R => sc.sc_wdcdev.UDMA_cap.set(6),
        _ => {}
    }

    sc.sc_wdcdev.set_modes.set(Some(acard_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(2);

    for i in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(i);
        if !pciide_chansetup(sc, i, interface) {
            continue;
        }
        let ok = if interface & pciide_interface_pci(i) != 0 {
            pciide_mapregs_native(pa, cp, &mut cmdsize, &mut ctlsize, pciide_pci_intr)
        } else {
            pciide_mapregs_compat(pa, cp, i, &mut cmdsize, &mut ctlsize)
        };
        cp.hw_ok.set(i32::from(ok));
        if cp.hw_ok.get() == 0 {
            return;
        }
        cp.wdc_channel.data32iot.set(cp.wdc_channel.cmd_iot.get());
        cp.wdc_channel.data32ioh.set(cp.wdc_channel.cmd_ioh.get());
        wdcattach(&cp.wdc_channel);
        acard_setup_channel(&cp.wdc_channel);
    }
    if !ACARD_IS_850(sc) {
        let mut reg = pci_conf_read(sc.pc(), sc.tag(), ATP8x0_CTRL);
        reg &= !ATP860_CTRL_INT;
        pci_conf_write(sc.pc(), sc.tag(), ATP8x0_CTRL, reg);
    }
}

/// `acard_setup_channel`.
pub fn acard_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    let (pc, tag) = (sc.pc(), sc.tag());
    let is850 = ACARD_IS_850(sc);

    // setup DMA if needed
    pciide_channel_dma_setup(cp);

    let mut idetime;
    let mut udma_mode;
    if is850 {
        idetime = 0;
        udma_mode = pci_conf_read(pc, tag, ATP850_UDMA);
        udma_mode &= !atp850_udma_mask(channel);
    } else {
        idetime = pci_conf_read(pc, tag, ATP860_IDETIME);
        idetime &= !atp860_settime_mask(channel);
        udma_mode = pci_conf_read(pc, tag, ATP860_UDMA);
        udma_mode &= !atp860_udma_mask(channel);
    }
    let settime = |drive: i32, act: u8, rec: u8| {
        if is850 {
            atp850_settime(drive, act.into(), rec.into())
        } else {
            atp860_settime(channel, drive, act.into(), rec.into())
        }
    };

    let mut idedma_ctl: u32 = 0;

    // Per drive settings
    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];
        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        // add timing values, setup DMA if needed
        if chp.wdc().has(WDC_CAPABILITY_UDMA) && drvp.isset(DRIVE_UDMA) {
            // use Ultra/DMA
            let udma = drvp.UDMA_mode.get() as usize;
            idetime |= settime(drive, ACARD_ACT_UDMA[udma], ACARD_REC_UDMA[udma]);
            if is850 {
                udma_mode |= atp850_udma_mode(channel, drive, ACARD_UDMA_CONF[udma].into());
            } else {
                udma_mode |= atp860_udma_mode(channel, drive, ACARD_UDMA_CONF[udma].into());
            }
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        } else if chp.wdc().has(WDC_CAPABILITY_DMA) && drvp.isset(DRIVE_DMA) {
            // use Multiword DMA
            drvp.clr(DRIVE_UDMA);
            let dma = drvp.DMA_mode.get() as usize;
            idetime |= settime(drive, ACARD_ACT_DMA[dma], ACARD_REC_DMA[dma]);
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
        } else {
            // PIO only
            drvp.clr(DRIVE_UDMA | DRIVE_DMA);
            let pio = drvp.PIO_mode.get() as usize;
            idetime |= settime(drive, ACARD_ACT_PIO[pio], ACARD_REC_PIO[pio]);
            pci_conf_write(
                pc,
                tag,
                ATP8x0_CTRL,
                pci_conf_read(pc, tag, ATP8x0_CTRL) | atp8x0_ctrl_en(channel),
            );
        }
    }

    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }
    pciide_print_modes(cp);

    if is850 {
        pci_conf_write(pc, tag, atp850_idetime(channel), idetime);
        pci_conf_write(pc, tag, ATP850_UDMA, udma_mode);
    } else {
        pci_conf_write(pc, tag, ATP860_IDETIME, idetime);
        pci_conf_write(pc, tag, ATP860_UDMA, udma_mode);
    }
}

/// `nforce_chip_map`.
pub fn nforce_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let interface = pci_interface(pa.pa_class);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    let mut conf = pci_conf_read(sc.pc(), sc.tag(), NFORCE_CONF);
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "{}: conf register 0x{:x}\n",
        sc.xname(),
        conf
    );

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);

    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev
        .UDMA_cap
        .set(if sc.pp().ide_product == PCI_PRODUCT_NVIDIA_NFORCE_IDE {
            5
        } else {
            6
        });
    sc.sc_wdcdev.set_modes.set(Some(nforce_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);

        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }

        if conf & nforce_chan_en(channel) == 0 {
            printf(format_args!(
                "{}: {} ignored (disabled)\n",
                sc.xname(),
                cp.name()
            ));
            cp.hw_ok.set(0);
            continue;
        }

        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            nforce_pci_intr,
        );
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }

        if pciide_chan_candisable(cp) {
            conf &= !nforce_chan_en(channel);
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }

        if let Some(set_modes) = sc.sc_wdcdev.set_modes.get() {
            set_modes(&cp.wdc_channel);
        }
    }
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "{}: new conf register 0x{:x}\n",
        sc.xname(),
        conf
    );
    pci_conf_write(sc.pc(), sc.tag(), NFORCE_CONF, conf);
}

/// `nforce_setup_channel`.
pub fn nforce_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    let (pc, tag) = (sc.pc(), sc.tag());

    let conf = pci_conf_read(pc, tag, NFORCE_CONF);
    let mut piodmatim = pci_conf_read(pc, tag, NFORCE_PIODMATIM);
    let piotim = pci_conf_read(pc, tag, NFORCE_PIOTIM);
    let mut udmatim = pci_conf_read(pc, tag, NFORCE_UDMATIM);
    let _ = conf;
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "{}: {} old timing values: piodmatim=0x{:x}, piotim=0x{:x}, udmatim=0x{:x}\n",
        sc.xname(),
        cp.name(),
        piodmatim,
        piotim,
        udmatim
    );

    // Setup DMA if needed
    pciide_channel_dma_setup(cp);

    // Clear all bits for this channel
    let mut idedma_ctl: u32 = 0;
    piodmatim &= !nforce_piodmatim_mask(channel);
    udmatim &= !nforce_udmatim_mask(channel);

    // Per channel settings
    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];

        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }

        let mut mode = 'pio: {
            let mode;
            if chp.wdc().has(WDC_CAPABILITY_UDMA) && drvp.isset(DRIVE_UDMA) {
                // Setup UltraDMA mode
                drvp.clr(DRIVE_DMA);

                udmatim |= nforce_udmatim_set(
                    channel,
                    drive,
                    NFORCE_UDMA[drvp.UDMA_mode.get() as usize].into(),
                ) | nforce_udma_en(channel, drive)
                    | nforce_udma_enm(channel, drive);

                mode = drvp.PIO_mode.get();
            } else if chp.wdc().has(WDC_CAPABILITY_DMA) && drvp.isset(DRIVE_DMA) {
                // Setup multiword DMA mode
                drvp.clr(DRIVE_UDMA);

                // mode = min(pio, dma + 2)
                if drvp.PIO_mode.get() <= drvp.DMA_mode.get() + 2 {
                    mode = drvp.PIO_mode.get();
                } else {
                    mode = drvp.DMA_mode.get() + 2;
                }
            } else {
                break 'pio drvp.PIO_mode.get();
            }
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            mode
        };

        // pio: Setup PIO mode
        if mode <= 2 {
            drvp.DMA_mode.set(0);
            drvp.PIO_mode.set(0);
            mode = 0;
        } else {
            drvp.PIO_mode.set(mode);
            drvp.DMA_mode.set(mode - 2);
        }
        piodmatim |= nforce_piodmatim_set(channel, drive, NFORCE_PIO[mode as usize].into());
    }

    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }

    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "{}: {} new timing values: piodmatim=0x{:x}, piotim=0x{:x}, udmatim=0x{:x}\n",
        sc.xname(),
        cp.name(),
        piodmatim,
        piotim,
        udmatim
    );
    pci_conf_write(pc, tag, NFORCE_PIODMATIM, piodmatim);
    pci_conf_write(pc, tag, NFORCE_UDMATIM, udmatim);

    pciide_print_modes(cp);
}

/// `nforce_pci_intr`.
pub fn nforce_pci_intr(arg: *mut c_void) -> i32 {
    // SAFETY: `pciide_mapregs_native` establishes this handler with the softc.
    let sc = unsafe { &*arg.cast_const().cast::<PciideSoftc>() };

    let mut rv = 0;
    for i in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(i);
        let wdc_cp = &cp.wdc_channel;

        // Skip compat channel
        if cp.compat.get() != 0 {
            continue;
        }

        let dmastat = bus_space_read_1(sc.dma_iot(), sc.dma_ioh(), idedma_ctl_reg(i));
        if dmastat & IDEDMA_CTL_INTR == 0 {
            continue;
        }

        let crv = wdcintr(ptr::from_ref(wdc_cp).cast_mut().cast());
        if crv == 0 {
            printf(format_args!("{}:{}: bogus intr\n", sc.xname(), i));
        } else {
            rv = 1;
        }
    }
    rv
}

/// `artisea_chip_map` (`PCIIDE_I31244_DISABLEDMA` is not defined: DMA is mapped on every
/// revision).
pub fn artisea_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);
    printf(format_args!("\n"));

    // XXX Configure LEDs to show activity.

    wdc_cap_set(
        sc,
        WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE | WDC_CAPABILITY_SATA,
    );
    sc.sc_wdcdev.PIO_cap.set(4);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
        sc.sc_wdcdev.DMA_cap.set(2);
        sc.sc_wdcdev.UDMA_cap.set(6);
    }
    sc.sc_wdcdev.set_modes.set(Some(sata_setup_channel));

    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);

    let interface = pci_interface(pa.pa_class);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }
        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            pciide_pci_intr,
        );
        if cp.hw_ok.get() == 0 {
            continue;
        }
        pciide_map_compat_intr(pa, cp, channel, interface);
        sata_setup_channel(&cp.wdc_channel);
    }
}

/// `ite_chip_map`.
pub fn ite_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;
    let (pc, tag) = (sc.pc(), sc.tag());

    // Fake interface since IT8212F is claimed to be a ``RAID'' device.
    let interface =
        PCIIDE_INTERFACE_BUS_MASTER_DMA | pciide_interface_pci(0) | pciide_interface_pci(1);

    let cfg = pci_conf_read(pc, tag, IT_CFG);
    let mut modectl = pci_conf_read(pc, tag, IT_MODE);
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "{}: cfg=0x{:x}, modectl=0x{:x}\n",
        sc.xname(),
        cfg & IT_CFG_MASK,
        modectl & IT_MODE_MASK
    );

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);

    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.UDMA_cap.set(6);

    sc.sc_wdcdev.set_modes.set(Some(ite_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    // Disable RAID
    modectl &= !IT_MODE_RAID1;
    // Disable CPU firmware mode
    modectl &= !IT_MODE_CPU;

    pci_conf_write(pc, tag, IT_MODE, modectl);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);

        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }
        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            pciide_pci_intr,
        );
        if let Some(set_modes) = sc.sc_wdcdev.set_modes.get() {
            set_modes(&cp.wdc_channel);
        }
    }

    // Re-read configuration registers after channels setup
    let cfg = pci_conf_read(pc, tag, IT_CFG);
    let modectl = pci_conf_read(pc, tag, IT_MODE);
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "{}: cfg=0x{:x}, modectl=0x{:x}\n",
        sc.xname(),
        cfg & IT_CFG_MASK,
        modectl & IT_MODE_MASK
    );
}

/// `ite_setup_channel` (the `#if 0` cable check, which works only in CPU firmware mode, is
/// not compiled, as in the C).
pub fn ite_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    let (pc, tag) = (sc.pc(), sc.tag());

    let mut cfg = pci_conf_read(pc, tag, IT_CFG);
    let mut modectl = pci_conf_read(pc, tag, IT_MODE);
    let mut tim = pci_conf_read(pc, tag, it_tim(channel));
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "{}:{}: tim=0x{:x}\n",
        sc.xname(),
        channel,
        tim
    );

    // Setup DMA if needed
    pciide_channel_dma_setup(cp);

    // Clear all bits for this channel
    let mut idedma_ctl: u32 = 0;

    // Per channel settings
    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];

        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }

        let mut mode = 'pio: {
            let mode;
            if chp.wdc().has(WDC_CAPABILITY_UDMA) && drvp.isset(DRIVE_UDMA) {
                // Setup UltraDMA mode
                drvp.clr(DRIVE_DMA);
                modectl &= !it_mode_dma(channel, drive);

                // #if 0: Check cable, works only in CPU firmware mode

                if drvp.UDMA_mode.get() >= 5 {
                    tim |= it_tim_udma5(drive);
                } else {
                    tim &= !it_tim_udma5(drive);
                }

                mode = drvp.PIO_mode.get();
            } else if chp.wdc().has(WDC_CAPABILITY_DMA) && drvp.isset(DRIVE_DMA) {
                // Setup multiword DMA mode
                drvp.clr(DRIVE_UDMA);
                modectl |= it_mode_dma(channel, drive);

                // mode = min(pio, dma + 2)
                if drvp.PIO_mode.get() <= drvp.DMA_mode.get() + 2 {
                    mode = drvp.PIO_mode.get();
                } else {
                    mode = drvp.DMA_mode.get() + 2;
                }
            } else {
                break 'pio drvp.PIO_mode.get();
            }
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            mode
        };

        // pio: Setup PIO mode
        if mode <= 2 {
            drvp.DMA_mode.set(0);
            drvp.PIO_mode.set(0);
            mode = 0;
        } else {
            drvp.PIO_mode.set(mode);
            drvp.DMA_mode.set(mode - 2);
        }
        let _ = mode;

        // Enable IORDY if PIO mode >= 3
        if drvp.PIO_mode.get() >= 3 {
            cfg |= it_cfg_iordy(channel);
        }
    }

    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "{}: tim=0x{:x}\n",
        sc.xname(),
        tim
    );

    pci_conf_write(pc, tag, IT_CFG, cfg);
    pci_conf_write(pc, tag, IT_MODE, modectl);
    pci_conf_write(pc, tag, it_tim(channel), tim);

    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }

    pciide_print_modes(cp);
}

/// `ixp_chip_map`.
pub fn ixp_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let interface = pci_interface(pa.pa_class);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);

    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.UDMA_cap.set(6);

    sc.sc_wdcdev.set_modes.set(Some(ixp_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);
        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }
        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            pciide_pci_intr,
        );
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }
        if let Some(set_modes) = sc.sc_wdcdev.set_modes.get() {
            set_modes(&cp.wdc_channel);
        }
    }
}

/// `ixp_setup_channel`.
pub fn ixp_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    let (pc, tag) = (sc.pc(), sc.tag());

    let mut pio_timing = pci_conf_read(pc, tag, IXP_PIO_TIMING);
    let mut pio = pci_conf_read(pc, tag, IXP_PIO_CTL);
    let mut mdma_timing = pci_conf_read(pc, tag, IXP_MDMA_TIMING);
    let mut udma = pci_conf_read(pc, tag, IXP_UDMA_CTL);

    // Setup DMA if needed
    pciide_channel_dma_setup(cp);

    let mut idedma_ctl: u32 = 0;

    // Per channel settings
    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];

        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }
        let mode;
        if chp.wdc().has(WDC_CAPABILITY_UDMA) && drvp.isset(DRIVE_UDMA) {
            // Setup UltraDMA mode
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            ixp_udma_enable(&mut udma, channel, drive);
            ixp_set_mode(&mut udma, channel, drive, drvp.UDMA_mode.get().into());
            mode = drvp.PIO_mode.get();
        } else if chp.wdc().has(WDC_CAPABILITY_DMA) && drvp.isset(DRIVE_DMA) {
            // Setup multiword DMA mode
            drvp.clr(DRIVE_UDMA);
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            ixp_udma_disable(&mut udma, channel, drive);
            ixp_set_timing(
                &mut mdma_timing,
                channel,
                drive,
                IXP_MDMA_TIMINGS[drvp.DMA_mode.get() as usize].into(),
            );

            // mode = min(pio, dma + 2)
            if drvp.PIO_mode.get() <= drvp.DMA_mode.get() + 2 {
                mode = drvp.PIO_mode.get();
            } else {
                mode = drvp.DMA_mode.get() + 2;
            }
        } else {
            mode = drvp.PIO_mode.get();
        }

        // Setup PIO mode
        drvp.PIO_mode.set(mode);
        if mode < 2 {
            drvp.DMA_mode.set(0);
        } else {
            drvp.DMA_mode.set(mode - 2);
        }
        // Set PIO mode and timings. Linux driver avoids PIO mode 1, let's do it too.
        if drvp.PIO_mode.get() == 1 {
            drvp.PIO_mode.set(0);
        }

        ixp_set_mode(&mut pio, channel, drive, drvp.PIO_mode.get().into());
        ixp_set_timing(
            &mut pio_timing,
            channel,
            drive,
            IXP_PIO_TIMINGS[drvp.PIO_mode.get() as usize].into(),
        );
    }

    pci_conf_write(pc, tag, IXP_UDMA_CTL, udma);
    pci_conf_write(pc, tag, IXP_MDMA_TIMING, mdma_timing);
    pci_conf_write(pc, tag, IXP_PIO_CTL, pio);
    pci_conf_write(pc, tag, IXP_PIO_TIMING, pio_timing);

    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }

    pciide_print_modes(cp);
}

/// The "setup PIO mode" tail most `*_setup_channel`s share: below PIO 3 both modes go to
/// 0, else the multiword DMA mode follows the PIO mode.
fn pciide_setup_pio_mode(drvp: &AtaDriveDatas, mode: u8) {
    if mode <= 2 {
        drvp.DMA_mode.set(0);
        drvp.PIO_mode.set(0);
    } else {
        drvp.PIO_mode.set(mode);
        drvp.DMA_mode.set(mode - 2);
    }
}

/// `jmicron_chip_map` (the `#if 0` check of `JMICRON_CHAN_EN` is not compiled, as in the C).
pub fn jmicron_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let interface = pci_interface(pa.pa_class);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    let mut conf = pci_conf_read(sc.pc(), sc.tag(), JMICRON_CONF);
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "{}: conf register 0x{:x}\n",
        sc.xname(),
        conf
    );

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);

    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.UDMA_cap.set(6);
    sc.sc_wdcdev.set_modes.set(Some(jmicron_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);

        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }

        // #if 0: if ((conf & JMICRON_CHAN_EN(channel)) == 0) { ... ignored (disabled) }

        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            pciide_pci_intr,
        );
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }

        if pciide_chan_candisable(cp) {
            conf &= !jmicron_chan_en(channel);
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }

        if let Some(set_modes) = sc.sc_wdcdev.set_modes.get() {
            set_modes(&cp.wdc_channel);
        }
    }
    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "{}: new conf register 0x{:x}\n",
        sc.xname(),
        conf
    );
    pci_conf_write(sc.pc(), sc.tag(), JMICRON_CONF, conf);
}

/// `jmicron_setup_channel`.
pub fn jmicron_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();

    let conf = pci_conf_read(sc.pc(), sc.tag(), JMICRON_CONF);

    // Setup DMA if needed
    pciide_channel_dma_setup(cp);

    // Clear all bits for this channel
    let mut idedma_ctl: u32 = 0;

    // Per channel settings
    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];

        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }

        let mode = 'pio: {
            let mode;
            if chp.wdc().has(WDC_CAPABILITY_UDMA) && drvp.isset(DRIVE_UDMA) {
                // Setup UltraDMA mode
                drvp.clr(DRIVE_DMA);

                // see if cable is up to scratch
                if conf & JMICRON_CONF_40PIN != 0 && drvp.UDMA_mode.get() > 2 {
                    drvp.UDMA_mode.set(2);
                }

                mode = drvp.PIO_mode.get();
            } else if chp.wdc().has(WDC_CAPABILITY_DMA) && drvp.isset(DRIVE_DMA) {
                // Setup multiword DMA mode
                drvp.clr(DRIVE_UDMA);

                // mode = min(pio, dma + 2)
                if drvp.PIO_mode.get() <= drvp.DMA_mode.get() + 2 {
                    mode = drvp.PIO_mode.get();
                } else {
                    mode = drvp.DMA_mode.get() + 2;
                }
            } else {
                break 'pio drvp.PIO_mode.get();
            }
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            mode
        };

        // pio: Setup PIO mode
        pciide_setup_pio_mode(drvp, mode);
    }

    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }

    pciide_print_modes(cp);
}

/// `phison_chip_map`.
pub fn phison_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let interface = pci_interface(pa.pa_class);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    sc.chip_unmap.set(Some(default_chip_unmap));

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);

    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.UDMA_cap.set(5);
    sc.sc_wdcdev.set_modes.set(Some(phison_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(1);

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);

        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }

        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            pciide_pci_intr,
        );
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }

        if let Some(set_modes) = sc.sc_wdcdev.set_modes.get() {
            set_modes(&cp.wdc_channel);
        }
    }
}

/// `phison_setup_channel`.
pub fn phison_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();

    // Setup DMA if needed
    pciide_channel_dma_setup(cp);

    // Clear all bits for this channel
    let mut idedma_ctl: u32 = 0;

    // Per channel settings
    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];

        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }

        let mode = 'pio: {
            let mode;
            if chp.wdc().has(WDC_CAPABILITY_UDMA) && drvp.isset(DRIVE_UDMA) {
                // Setup UltraDMA mode
                drvp.clr(DRIVE_DMA);
                mode = drvp.PIO_mode.get();
            } else if chp.wdc().has(WDC_CAPABILITY_DMA) && drvp.isset(DRIVE_DMA) {
                // Setup multiword DMA mode
                drvp.clr(DRIVE_UDMA);

                // mode = min(pio, dma + 2)
                if drvp.PIO_mode.get() <= drvp.DMA_mode.get() + 2 {
                    mode = drvp.PIO_mode.get();
                } else {
                    mode = drvp.DMA_mode.get() + 2;
                }
            } else {
                break 'pio drvp.PIO_mode.get();
            }
            idedma_ctl |= u32::from(idedma_ctl_drv_dma(drive));
            mode
        };

        // pio: Setup PIO mode
        pciide_setup_pio_mode(drvp, mode);
    }

    if idedma_ctl != 0 {
        // Add software bits in status register
        bus_space_write_1(
            sc.dma_iot(),
            sc.dma_ioh(),
            idedma_ctl_reg(channel),
            idedma_ctl as u8,
        );
    }

    pciide_print_modes(cp);
}

/// `sch_chip_map`.
pub fn sch_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let interface = pci_interface(pa.pa_class);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);

    sc.sc_wdcdev
        .cap
        .set(WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32 | WDC_CAPABILITY_MODE);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(sc, WDC_CAPABILITY_DMA | WDC_CAPABILITY_UDMA);
        wdc_cap_set(sc, WDC_CAPABILITY_IRQACK);
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.UDMA_cap.set(5);
    sc.sc_wdcdev.set_modes.set(Some(sch_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(1);

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);

        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }

        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        pciide_mapchan(
            pa,
            cp,
            interface,
            &mut cmdsize,
            &mut ctlsize,
            pciide_pci_intr,
        );
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
            continue;
        }

        if let Some(set_modes) = sc.sc_wdcdev.set_modes.get() {
            set_modes(&cp.wdc_channel);
        }
    }
}

/// `sch_setup_channel`.
pub fn sch_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());

    // Setup DMA if needed
    pciide_channel_dma_setup(cp);

    // Per channel settings
    for drive in 0..2 {
        let drvp = &chp.ch_drive[drive as usize];

        // If no drive, skip
        if !drvp.isset(DRIVE) {
            continue;
        }

        let timaddr = if drive == 0 { SCH_D0TIM } else { SCH_D1TIM };
        let mut tim = pci_conf_read(sc.pc(), sc.tag(), timaddr);
        tim &= !SCH_TIM_MASK;

        let mode;
        if chp.wdc().has(WDC_CAPABILITY_UDMA) && drvp.isset(DRIVE_UDMA) {
            // Setup UltraDMA mode
            drvp.clr(DRIVE_DMA);

            mode = drvp.PIO_mode.get();
            tim |= (u32::from(drvp.UDMA_mode.get()) << 16) | SCH_TIM_SYNCDMA;
        } else if chp.wdc().has(WDC_CAPABILITY_DMA) && drvp.isset(DRIVE_DMA) {
            // Setup multiword DMA mode
            drvp.clr(DRIVE_UDMA);

            tim &= !SCH_TIM_SYNCDMA;

            // mode = min(pio, dma + 2)
            if drvp.PIO_mode.get() <= drvp.DMA_mode.get() + 2 {
                mode = drvp.PIO_mode.get();
            } else {
                mode = drvp.DMA_mode.get() + 2;
            }
        } else {
            mode = drvp.PIO_mode.get();
            // goto pio
        }

        // pio: Setup PIO mode
        pciide_setup_pio_mode(drvp, mode);
        tim |= (u32::from(drvp.DMA_mode.get()) << 8) | u32::from(drvp.PIO_mode.get());
        pci_conf_write(sc.pc(), sc.tag(), timaddr, tim);
    }

    pciide_print_modes(cp);
}

/// `rdc_chip_map`.
pub fn rdc_chip_map(sc: &'static PciideSoftc, pa: &PciAttachArgs) {
    let interface = pci_interface(pa.pa_class);
    let mut cmdsize: BusSize = 0;
    let mut ctlsize: BusSize = 0;
    let (pc, tag) = (sc.pc(), sc.tag());

    printf(format_args!(": DMA"));
    pciide_mapreg_dma(sc, pa);
    wdc_cap_set(sc, WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32);
    if sc.sc_dma_ok.get() != 0 {
        wdc_cap_set(
            sc,
            WDC_CAPABILITY_UDMA | WDC_CAPABILITY_DMA | WDC_CAPABILITY_IRQACK,
        );
        sc.sc_wdcdev.irqack.set(Some(pciide_irqack));
        sc.sc_wdcdev.dma_init.set(Some(pciide_dma_init));
    }
    sc.sc_wdcdev.PIO_cap.set(4);
    sc.sc_wdcdev.DMA_cap.set(2);
    sc.sc_wdcdev.UDMA_cap.set(5);
    sc.sc_wdcdev.set_modes.set(Some(rdc_setup_channel));
    sc.sc_wdcdev.channels.set(Some(&sc.wdc_chanarray[..]));
    sc.sc_wdcdev.nchannels.set(PCIIDE_NUM_CHANNELS);

    pciide_print_channels(sc.sc_wdcdev.nchannels.get(), interface);

    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "rdc_chip_map: old PATR=0x{:x}, PSD1ATR=0x{:x}, UDCCR=0x{:x}, IIOCR=0x{:x}\n",
        pci_conf_read(pc, tag, RDCIDE_PATR),
        pci_conf_read(pc, tag, RDCIDE_PSD1ATR),
        pci_conf_read(pc, tag, RDCIDE_UDCCR),
        pci_conf_read(pc, tag, RDCIDE_IIOCR)
    );

    for channel in 0..sc.sc_wdcdev.nchannels.get() {
        let cp = sc.channel(channel);

        if !pciide_chansetup(sc, channel, interface) {
            continue;
        }
        let mut patr = pci_conf_read(pc, tag, RDCIDE_PATR);
        if patr & rdcide_patr_en(channel) == 0 {
            printf(format_args!(
                "{}: {} ignored (disabled)\n",
                sc.xname(),
                cp.name()
            ));
            cp.hw_ok.set(0);
            continue;
        }
        pciide_map_compat_intr(pa, cp, channel, interface);
        if cp.hw_ok.get() == 0 {
            continue;
        }
        'next: {
            pciide_mapchan(
                pa,
                cp,
                interface,
                &mut cmdsize,
                &mut ctlsize,
                pciide_pci_intr,
            );
            if cp.hw_ok.get() == 0 {
                break 'next;
            }
            if pciide_chan_candisable(cp) {
                patr &= !rdcide_patr_en(channel);
                pci_conf_write(pc, tag, RDCIDE_PATR, patr);
            }
            if cp.hw_ok.get() == 0 {
                break 'next;
            }
            if let Some(set_modes) = sc.sc_wdcdev.set_modes.get() {
                set_modes(&cp.wdc_channel);
            }
        }
        // next:
        if cp.hw_ok.get() == 0 {
            pciide_unmap_compat_intr(pa, cp, channel, interface);
        }
    }

    wdcdebug_print!(
        wdcdebug_pciide_mask,
        DEBUG_PROBE,
        "rdc_chip_map: PATR=0x{:x}, PSD1ATR=0x{:x}, UDCCR=0x{:x}, IIOCR=0x{:x}\n",
        pci_conf_read(pc, tag, RDCIDE_PATR),
        pci_conf_read(pc, tag, RDCIDE_PSD1ATR),
        pci_conf_read(pc, tag, RDCIDE_UDCCR),
        pci_conf_read(pc, tag, RDCIDE_IIOCR)
    );
}

/// `rdc_setup_channel`. Like the C, it neither sets the DMA status register's software bits
/// nor prints the modes, and clearing the modes keeps only both channels' enable bits of
/// PATR (the other channel's timings are cleared too).
pub fn rdc_setup_channel(chp: &ChannelSoftc) {
    let cp = pciide_cp(chp);
    let sc = pciide_sc(cp.wdc_channel.wdc());
    let channel = chp.channel.get();
    let (pc, tag) = (sc.pc(), sc.tag());

    let mut patr = pci_conf_read(pc, tag, RDCIDE_PATR);
    let mut psd1atr = pci_conf_read(pc, tag, RDCIDE_PSD1ATR);
    let mut udccr = pci_conf_read(pc, tag, RDCIDE_UDCCR);
    let mut iiocr = pci_conf_read(pc, tag, RDCIDE_IIOCR);

    // setup DMA
    pciide_channel_dma_setup(cp);

    // clear modes
    patr &= rdcide_patr_en(0) | rdcide_patr_en(1);
    psd1atr &= !rdcide_psd1atr_setup_mask(channel);
    psd1atr &= !rdcide_psd1atr_hold_mask(channel);
    for drive in 0..2 {
        udccr &= !rdcide_udccr_en(channel, drive);
        udccr &= !rdcide_udccr_tim_mask(channel, drive);
        iiocr &= !rdcide_iiocr_clk_mask(channel, drive);
    }
    // now setup modes
    for drive in 0..2 {
        let drvp = &cp.wdc_channel.ch_drive[drive as usize];
        if !drvp.isset(DRIVE) {
            continue;
        }
        if drvp.isset(DRIVE_ATAPI) {
            patr |= rdcide_patr_ata(channel, drive);
        }
        let pio = drvp.PIO_mode.get() as usize;
        if drive == 0 {
            patr |= rdcide_patr_setup(RDCIDE_SETUP[pio].into(), channel);
            patr |= rdcide_patr_hold(RDCIDE_HOLD[pio].into(), channel);
        } else {
            patr |= rdcide_patr_dev1_ten(channel);
            psd1atr |= rdcide_psd1atr_setup(RDCIDE_SETUP[pio].into(), channel);
            psd1atr |= rdcide_psd1atr_hold(RDCIDE_HOLD[pio].into(), channel);
        }
        if drvp.PIO_mode.get() > 0 {
            patr |= rdcide_patr_ftim(channel, drive);
            patr |= rdcide_patr_iordy(channel, drive);
        }
        if drvp.isset(DRIVE_DMA) {
            patr |= rdcide_patr_dmaen(channel, drive);
        }
        if !drvp.isset(DRIVE_UDMA) {
            continue;
        }

        if iiocr & rdcide_iiocr_cable(channel, drive) == 0 && drvp.UDMA_mode.get() > 2 {
            drvp.UDMA_mode.set(2);
        }
        let udma = drvp.UDMA_mode.get() as usize;
        udccr |= rdcide_udccr_en(channel, drive);
        udccr |= rdcide_udccr_tim(RDCIDE_UDMATIM[udma].into(), channel, drive);
        iiocr |= rdcide_iiocr_clk(RDCIDE_UDMACLK[udma], channel, drive);
    }

    pci_conf_write(pc, tag, RDCIDE_PATR, patr);
    pci_conf_write(pc, tag, RDCIDE_PSD1ATR, psd1atr);
    pci_conf_write(pc, tag, RDCIDE_UDCCR, udccr);
    pci_conf_write(pc, tag, RDCIDE_IIOCR, iiocr);
}

/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::pci::pcireg::pci_id_code;

    fn map_of(vendor: u32, product: u32) -> Option<PciideChipMap> {
        pciide_lookup_product(pci_id_code(vendor, product)).map(|pp| pp.chip_map)
    }

    #[test]
    fn lookup_finds_the_chip_map() {
        // QEMU's `pc` PIIX3 IDE function (8086:7010) and `piix4-ide` (8086:7111).
        let piix3 = map_of(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82371SB_IDE);
        assert!(piix3.is_some_and(|m| ptr::fn_addr_eq(m, piix_chip_map as PciideChipMap)));
        let piix4 = map_of(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82371AB_IDE);
        assert!(piix4.is_some_and(|m| ptr::fn_addr_eq(m, piix_chip_map as PciideChipMap)));
        let ich5 = map_of(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801EB_SATA);
        assert!(ich5.is_some_and(|m| ptr::fn_addr_eq(m, piixsata_chip_map as PciideChipMap)));
        let k2 = map_of(PCI_VENDOR_RCC, PCI_PRODUCT_RCC_K2_SATA);
        assert!(k2.is_some_and(|m| ptr::fn_addr_eq(m, svwsata_chip_map as PciideChipMap)));
        let jmb = map_of(PCI_VENDOR_JMICRON, PCI_PRODUCT_JMICRON_JMB361);
        assert!(jmb.is_some_and(|m| ptr::fn_addr_eq(m, jmicron_chip_map as PciideChipMap)));
    }

    #[test]
    fn lookup_misses() {
        // An Intel product the table does not list, a vendor it does not list, and the
        // CS5535 row, which only i386 compiles.
        assert!(map_of(PCI_VENDOR_INTEL, 0xffff).is_none());
        assert!(map_of(PCI_VENDOR_QUMRANET, 0x1001).is_none());
        assert!(map_of(PCI_VENDOR_NS, PCI_PRODUCT_NS_CS5535_IDE).is_none());
    }

    #[test]
    fn class_override_rows() {
        // The Promise Ultra-33 does not claim to be an IDE controller; its row says so.
        let pp = pciide_lookup_product(pci_id_code(
            PCI_VENDOR_PROMISE,
            PCI_PRODUCT_PROMISE_PDC20246,
        ));
        assert!(pp.is_some_and(|pp| pp.ide_flags & IDE_PCI_CLASS_OVERRIDE != 0));
        let pp =
            pciide_lookup_product(pci_id_code(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82371SB_IDE));
        assert!(pp.is_some_and(|pp| pp.ide_flags == 0));
    }

    #[test]
    fn table_size() {
        // 248 rows in the C, less the __i386__ CS5535 one; 18 vendors.
        assert_eq!(PCIIDE_VENDORS.len(), 18);
        let rows: usize = PCIIDE_VENDORS.iter().map(|v| v.ide_products.len()).sum();
        assert_eq!(rows, 247);
    }

    #[test]
    fn piix_timings() {
        // PIO 4: ISP 2, RTC 3; channel 1 is the upper half of IDETIM.
        assert_eq!(piix_setup_idetim_timings(4, 0, 0), 0x2300);
        assert_eq!(piix_setup_idetim_timings(4, 0, 1), 0x2300 << 16);
        // multiword DMA 2: ISP 2, RTC 3; PIO 0: compatible timings, 0.
        assert_eq!(piix_setup_idetim_timings(2, 1, 0), 0x2300);
        assert_eq!(piix_setup_idetim_timings(0, 0, 0), 0);
        // the slave timings of channel 1, DMA 2: ISP 2 << 6, RTC 3 << 4.
        assert_eq!(piix_setup_sidetim_timings(2, 1, 1), 0xb0);
        assert_eq!(piix_setup_sidetim_timings(3, 0, 0), (2 << 2) | 1);
    }

    #[test]
    fn sis_host_bridges() {
        // The last matching row wins: a rev 0x30 630 is the 630S.
        assert_eq!(SIS_HOSTBR_TYPE.len(), 37);
        let row = SIS_HOSTBR_TYPE
            .iter()
            .rfind(|t| u32::from(t.id) == PCI_PRODUCT_SIS_630 && 0x30 >= t.rev);
        assert!(row.is_some_and(|t| t.name == "630S" && t.type_ == SIS_TYPE_100NEW));
    }
}
/* </TESTS> */
