/*	$OpenBSD: pcivar.h,v 1.81 2025/06/29 19:32:08 miod Exp $	*/
/*	$NetBSD: pcivar.h,v 1.23 1997/06/06 23:48:05 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 Christopher G. Demetriou.  All rights reserved.
 * Copyright (c) 1994 Charles Hannum.  All rights reserved.
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
 *	This product includes software developed by Charles Hannum.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
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
//! `<dev/pci/pcivar.h>`: definitions for PCI autoconfiguration, the types and functions
//! used for PCI configuration. Some of this information is machine-specific, and is provided
//! by `<machine/pci_machdep.h>` (`machine::pci_machdep`).
//!
//! Upstream: sys/dev/pci/pcivar.h @ 3ce1f3f79392
//!
//! The functions it declares live where the C defines them: `pci.rs` (`pci_probe_device`,
//! `pci_get_capability`, `pci_find_device`, ...), `pci_map.rs` (`pci_mapreg_*`),
//! `pci_subr.rs` (`pci_devinfo`, `pci_findvendor`, `pci_findproduct`) and `pci_quirks.rs`
//! (`pci_lookup_quirkdata`).
//!
//! ## Deviations
//! - The extent members (`pba_ioex`, `pba_memex`, `pba_pmemex`, `pba_busex`, the `pa_*ex`
//!   and `sc_*ex` ones) are `Option<&'static Extent>` (M16b): the host bridges that make
//!   extents (arm64's `pciecam` and `acpipci`) hand them down; amd64's are `None` until
//!   its `pci_init_extents` is ported, and every C test of them then takes the NULL branch.
//! - `pba_bridgetag`/`pa_bridgetag` are `Option<&'static Pcitag>` and the bridge's
//!   interrupt handles (`pba_bridgeih`, an array of four) an `Option` of a slice.
//! - The locator macros (`cf->pcibuscf_bus`, `cf->pcicf_dev`, `cf->pcicf_function`) are
//!   functions of the `cfdata`; a missing locator reads as its wildcard.
//! - [`PciSoftc`] holds its members in `Cell`s (all-zero valid, `config_make_softc`
//!   allocates it zeroed); the tags are `Option`s until `pciattach` sets them.

use core::cell::Cell;

use crate::dev::pci::pci::PciDevList;
use crate::dev::pci::pcireg::{PciIntrLine, PciIntrPin, PciProductId, PciVendorId};
use crate::machine::bus::{BusDmaTag, BusSpaceTag};
use crate::machine::pci_machdep::{PciChipsetTag, PciIntrHandle, Pcitag};
use crate::sys::device::{Cfdata, Device, Softc};
use crate::sys::extent::Extent;
use crate::sys::queue::ListHead;

/// `pcireg_t`: configuration space register XXX.
pub type Pcireg = u32;

/// `PCI_PWR_D0`: power management (PCI 2.2).
pub const PCI_PWR_D0: i32 = 0;
/// `PCI_PWR_D1`.
pub const PCI_PWR_D1: i32 = 1;
/// `PCI_PWR_D2`.
pub const PCI_PWR_D2: i32 = 2;
/// `PCI_PWR_D3`.
pub const PCI_PWR_D3: i32 = 3;

/// `struct pci_matchid`: a vendor and product a driver matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciMatchid {
    /// `pm_vid`.
    pub pm_vid: PciVendorId,
    /// `pm_pid`.
    pub pm_pid: PciProductId,
}

/// `struct pcibus_attach_args`: PCI bus attach arguments. `#[repr(C)]` with the bus name
/// first, which the parent's print function reads.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PcibusAttachArgs {
    /// `pba_busname`: XXX should be common.
    pub pba_busname: &'static [u8],
    /// `pba_iot`: pci i/o space tag.
    pub pba_iot: BusSpaceTag,
    /// `pba_memt`: pci mem space tag.
    pub pba_memt: BusSpaceTag,
    /// `pba_dmat`: DMA tag.
    pub pba_dmat: BusDmaTag,
    /// `pba_pc`.
    pub pba_pc: PciChipsetTag,
    /// `pba_flags`: flags; see below.
    pub pba_flags: i32,
    /// `pba_ioex`: the bus's I/O space extent, if any.
    pub pba_ioex: Option<&'static Extent>,
    /// `pba_memex`: the bus's memory space extent.
    pub pba_memex: Option<&'static Extent>,
    /// `pba_pmemex`: the bus's prefetchable memory space extent.
    pub pba_pmemex: Option<&'static Extent>,
    /// `pba_busex`: the bus numbers' extent.
    pub pba_busex: Option<&'static Extent>,
    /// `pba_domain`: PCI domain.
    pub pba_domain: i32,
    /// `pba_bus`: PCI bus number.
    pub pba_bus: i32,
    /// `pba_bridgetag`: the pcitag of our parent bridge. If there is no parent bridge, then
    /// we assume we are a root bus.
    pub pba_bridgetag: Option<&'static Pcitag>,
    /// `pba_bridgeih`: the parent bridge's interrupt handles.
    pub pba_bridgeih: Option<&'static [PciIntrHandle]>,
    /// `pba_intrswiz`: how to swizzle pins (secondary busses only).
    pub pba_intrswiz: u32,
    /// `pba_intrtag`: intr. appears to come from here (secondary busses only).
    pub pba_intrtag: Pcitag,
}

/// `struct pci_attach_args`: PCI device attach arguments.
#[derive(Clone, Copy)]
pub struct PciAttachArgs {
    /// `pa_iot`: pci i/o space tag.
    pub pa_iot: BusSpaceTag,
    /// `pa_memt`: pci mem space tag.
    pub pa_memt: BusSpaceTag,
    /// `pa_dmat`: DMA tag.
    pub pa_dmat: BusDmaTag,
    /// `pa_pc`.
    pub pa_pc: PciChipsetTag,
    /// `pa_flags`: flags; see below.
    pub pa_flags: i32,
    /// `pa_ioex`: the bus's I/O space extent, if any.
    pub pa_ioex: Option<&'static Extent>,
    /// `pa_memex`.
    pub pa_memex: Option<&'static Extent>,
    /// `pa_pmemex`.
    pub pa_pmemex: Option<&'static Extent>,
    /// `pa_busex`.
    pub pa_busex: Option<&'static Extent>,
    /// `pa_domain`.
    pub pa_domain: u32,
    /// `pa_bus`.
    pub pa_bus: u32,
    /// `pa_device`.
    pub pa_device: u32,
    /// `pa_function`.
    pub pa_function: u32,
    /// `pa_tag`.
    pub pa_tag: Pcitag,
    /// `pa_id`.
    pub pa_id: Pcireg,
    /// `pa_class`.
    pub pa_class: Pcireg,
    /// `pa_bridgetag`.
    pub pa_bridgetag: Option<&'static Pcitag>,
    /// `pa_bridgeih`.
    pub pa_bridgeih: Option<&'static [PciIntrHandle]>,
    /// `pa_intrswiz`: how to swizzle pins if ppb.
    pub pa_intrswiz: u32,
    /// `pa_intrtag`: intr. appears to come from here.
    pub pa_intrtag: Pcitag,
    /// `pa_intrpin`: intr. appears on this pin.
    pub pa_intrpin: PciIntrPin,
    /// `pa_intrline`: intr. routing information, on systems whose firmware puts the right
    /// routing data into the line register in configuration space.
    pub pa_intrline: PciIntrLine,
    /// `pa_rawintrpin`: unswizzled pin.
    pub pa_rawintrpin: PciIntrPin,
}

/// `PCI_FLAGS_IO_ENABLED`: I/O space is enabled (OpenBSD doesn't actually use these flags
/// yet).
pub const PCI_FLAGS_IO_ENABLED: i32 = 0x01;
/// `PCI_FLAGS_MEM_ENABLED`: memory space is enabled.
pub const PCI_FLAGS_MEM_ENABLED: i32 = 0x02;
/// `PCI_FLAGS_MRL_OKAY`: Memory Read Line okay.
pub const PCI_FLAGS_MRL_OKAY: i32 = 0x04;
/// `PCI_FLAGS_MRM_OKAY`: Memory Read Multiple okay.
pub const PCI_FLAGS_MRM_OKAY: i32 = 0x08;
/// `PCI_FLAGS_MWI_OKAY`: Memory Write and Invalidate okay.
pub const PCI_FLAGS_MWI_OKAY: i32 = 0x10;
/// `PCI_FLAGS_MSI_ENABLED`: Message Signaled Interrupt enabled.
pub const PCI_FLAGS_MSI_ENABLED: i32 = 0x20;
/// `PCI_FLAGS_MSIVEC_ENABLED`: Multiple Message Capability enabled.
pub const PCI_FLAGS_MSIVEC_ENABLED: i32 = 0x40;

/// `struct pci_quirkdata`.
#[derive(Clone, Copy, Debug)]
pub struct PciQuirkdata {
    /// `vendor`: Vendor ID.
    pub vendor: u32,
    /// `product`: Product ID.
    pub product: u32,
    /// `quirks`: quirks; see below.
    pub quirks: i32,
}

/// `PCI_QUIRK_MULTIFUNCTION`: probe all eight functions whatever the header says.
pub const PCI_QUIRK_MULTIFUNCTION: i32 = 1;
/// `PCI_QUIRK_MONOFUNCTION`: probe function 0 only.
pub const PCI_QUIRK_MONOFUNCTION: i32 = 2;

/// `struct pci_softc`: a PCI bus.
#[repr(C)]
pub struct PciSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_memt`.
    pub sc_memt: Cell<Option<BusSpaceTag>>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_pc`.
    pub sc_pc: Cell<Option<PciChipsetTag>>,
    /// `sc_flags`.
    pub sc_flags: Cell<i32>,
    /// `sc_ioex`.
    pub sc_ioex: Cell<Option<&'static Extent>>,
    /// `sc_memex`.
    pub sc_memex: Cell<Option<&'static Extent>>,
    /// `sc_pmemex`.
    pub sc_pmemex: Cell<Option<&'static Extent>>,
    /// `sc_busex`.
    pub sc_busex: Cell<Option<&'static Extent>>,
    /// `sc_devs`: the functions found on the bus (`LIST_HEAD(, pci_dev)`).
    pub sc_devs: ListHead<PciDevList>,
    /// `sc_domain`.
    pub sc_domain: Cell<i32>,
    /// `sc_bus`.
    pub sc_bus: Cell<i32>,
    /// `sc_maxndevs`.
    pub sc_maxndevs: Cell<i32>,
    /// `sc_bridgetag`.
    pub sc_bridgetag: Cell<Option<&'static Pcitag>>,
    /// `sc_bridgeih`.
    pub sc_bridgeih: Cell<Option<&'static [PciIntrHandle]>>,
    /// `sc_intrswiz`.
    pub sc_intrswiz: Cell<u32>,
    /// `sc_intrtag`.
    pub sc_intrtag: Cell<Pcitag>,
}

impl PciSoftc {
    /// `sc->sc_pc`, set by `pciattach` before any device is probed.
    pub fn pc(&self) -> PciChipsetTag {
        match self.sc_pc.get() {
            Some(pc) => pc,
            None => crate::kern::subr_prf::panic(format_args!("pci: no chipset tag")),
        }
    }
}

// SAFETY: `#[repr(C)]` with the device first; every other member is a `Cell` of an integer,
// an `Option` of a tag or reference, or a pcitag (an integer or a struct of integers), and
// the list head is a null pointer when zero: all valid as zero bits.
unsafe impl Softc for PciSoftc {}

/// `pcibuscf_bus`: the `bus` locator of a device attaching to `pcibus` (`cf_loc[0]`).
pub fn pcibuscf_bus(cf: &Cfdata) -> i32 {
    cf.cf_loc.first().map_or(PCIBUS_UNK_BUS, |&l| l as i32)
}

/// `PCIBUS_UNK_BUS`: wildcarded 'bus'.
pub const PCIBUS_UNK_BUS: i32 = -1;

/// `pcicf_dev`: the `dev` locator of a PCI device (`cf_loc[0]`).
pub fn pcicf_dev(cf: &Cfdata) -> i32 {
    cf.cf_loc.first().map_or(PCI_UNK_DEV, |&l| l as i32)
}

/// `PCI_UNK_DEV`: wildcarded 'dev'.
pub const PCI_UNK_DEV: i32 = -1;

/// `pcicf_function`: the `function` locator of a PCI device (`cf_loc[1]`).
pub fn pcicf_function(cf: &Cfdata) -> i32 {
    cf.cf_loc.get(1).map_or(PCI_UNK_FUNCTION, |&l| l as i32)
}

/// `PCI_UNK_FUNCTION`: wildcarded 'function'.
pub const PCI_UNK_FUNCTION: i32 = -1;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pcivar.h");
        for (name, value) in [
            ("PCI_PWR_D3", PCI_PWR_D3),
            ("PCI_FLAGS_IO_ENABLED", PCI_FLAGS_IO_ENABLED),
            ("PCI_FLAGS_MEM_ENABLED", PCI_FLAGS_MEM_ENABLED),
            ("PCI_FLAGS_MSI_ENABLED", PCI_FLAGS_MSI_ENABLED),
            ("PCI_FLAGS_MSIVEC_ENABLED", PCI_FLAGS_MSIVEC_ENABLED),
            ("PCI_QUIRK_MULTIFUNCTION", PCI_QUIRK_MULTIFUNCTION),
            ("PCI_QUIRK_MONOFUNCTION", PCI_QUIRK_MONOFUNCTION),
            ("PCIBUS_UNK_BUS", PCIBUS_UNK_BUS),
            ("PCI_UNK_DEV", PCI_UNK_DEV),
            ("PCI_UNK_FUNCTION", PCI_UNK_FUNCTION),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
    }
}
/* </TESTS> */
