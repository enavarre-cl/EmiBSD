/*	$OpenBSD: siop_pci_common.h,v 1.7 2010/07/23 07:47:13 jsg Exp $ */
/*	$NetBSD: siop_pci_common.h,v 1.6 2005/02/27 00:27:34 perry Exp $ */
/*	$OpenBSD: siop_pci_common.c,v 1.20 2024/05/24 06:02:58 jsg Exp $ */
/*	$NetBSD: siop_pci_common.c,v 1.25 2005/06/28 00:28:42 thorpej Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2000 Manuel Bouyer.
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
 */
/*
 * Copyright (c) 2000 Manuel Bouyer.
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
 */
/* </LICENSES> */

/* <CODE> */
//! The code the siop and esiop PCI front-ends share (`<dev/pci/siop_pci_common.h>` and
//! `siop_pci_common.c`): the table of the Symbios chips the drivers know, with each one's
//! features, burst, offset and clock; the common attach, which maps the registers (memory
//! space, else I/O space) and the on-board RAM and establishes the interrupt; and the PCI
//! part of the chip reset.
//!
//! Upstream: sys/dev/pci/siop_pci_common.h @ 3ce1f3f79392, sys/dev/pci/siop_pci_common.c @ 3ce1f3f79392
//!
//! "SYM53c8xx PCI-SCSI I/O Processors driver: PCI front-end". The header's types and the
//! file's functions share this module (`siop_pci_common.h` is a prototype header of this
//! file).
//!
//! ## Deviations
//! - `siop_products[]` is [`SIOP_PRODUCTS`] without the C's all-zero terminator: the
//!   lookup walks the slice.
//! - `siop_pci_attach_common` returns `bool` (the C's 1 and 0) and takes the interrupt
//!   handler as a [`PciIntrFn`].
//! - `struct siop_pci_common_softc` is `#[repr(C)]` with its members in `Cell`s (`sc_pc` an
//!   `Option`, `sc_ih` a raw pointer, `sc_pp` an `Option` of a reference to the table),
//!   all-zero valid inside the zeroed softc.
//! - The interrupt runs on the INTx line the firmware routed, as the C does: siop asks
//!   `pci_intr_map` only (amd64 has no I/O APIC here before ACPI, M13).
//! - `SIOP_SYMLED` ("XXX Should be a devprop!") is not defined: `SF_CHIP_LED0` is never set
//!   here.
//! - No stubs: every function of the file is ported.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::ic::siopreg::{
    CTEST3_WRIE, CTEST4_BDIS, CTEST5_BBCK, DCNTL_CLSE, DMODE_BL_MASK, DMODE_BL_SHIFT, DMODE_BOF,
    DMODE_ERL, DMODE_ERMP, SIOP_CTEST3, SIOP_CTEST4, SIOP_CTEST5, SIOP_DCNTL, SIOP_DMODE,
};
use crate::dev::ic::siopvar_common::{
    SF_BUS_ULTRA, SF_BUS_ULTRA2, SF_BUS_ULTRA3, SF_BUS_WIDE, SF_CHIP_10REGS, SF_CHIP_AAIP,
    SF_CHIP_DBLR, SF_CHIP_DFBC, SF_CHIP_DT, SF_CHIP_FIFO, SF_CHIP_GEBUG, SF_CHIP_LEDC, SF_CHIP_LS,
    SF_CHIP_PF, SF_CHIP_QUAD, SF_CHIP_RAM, SF_PCI_BOF, SF_PCI_CLS, SF_PCI_RL, SF_PCI_RM,
    SF_PCI_WRI, SiopCommonSoftc,
};
use crate::dev::pci::pci_map::{pci_mapreg_map, pci_mapreg_type};
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_SYMBIOS_810, PCI_PRODUCT_SYMBIOS_815, PCI_PRODUCT_SYMBIOS_820,
    PCI_PRODUCT_SYMBIOS_825, PCI_PRODUCT_SYMBIOS_860, PCI_PRODUCT_SYMBIOS_875,
    PCI_PRODUCT_SYMBIOS_875J, PCI_PRODUCT_SYMBIOS_885, PCI_PRODUCT_SYMBIOS_895,
    PCI_PRODUCT_SYMBIOS_895A, PCI_PRODUCT_SYMBIOS_896, PCI_PRODUCT_SYMBIOS_1010,
    PCI_PRODUCT_SYMBIOS_1010_2, PCI_PRODUCT_SYMBIOS_1510D, PCI_VENDOR_SYMBIOS,
};
use crate::dev::pci::pcireg::{
    PCI_MAPREG_MEM_TYPE_32BIT, PCI_MAPREG_MEM_TYPE_64BIT, PCI_MAPREG_TYPE_IO, PCI_MAPREG_TYPE_MEM,
    pci_product, pci_revision, pci_vendor,
};
use crate::dev::pci::pcivar::PciAttachArgs;
use crate::kern::subr_prf::printf;
use crate::machine::bus::bus_space_unmap;
use crate::machine::intr::IPL_BIO;
use crate::machine::pci_machdep::{
    PciChipsetTag, PciIntrFn, Pcitag, pci_intr_disestablish, pci_intr_establish, pci_intr_map,
    pci_intr_string,
};

/// `struct siop_product_desc`: structure describing each chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SiopProductDesc {
    /// `product`.
    pub product: u32,
    /// `revision`.
    pub revision: i32,
    /// `features`: features are defined in siopvar.h (`SF_*`).
    pub features: i32,
    /// `maxburst`.
    pub maxburst: u8,
    /// `maxoff`: maximum supported offset.
    pub maxoff: u8,
    /// `clock_div`: clock divider to use for async. logic.
    pub clock_div: u8,
    /// `clock_period`: clock period (ns * 10).
    pub clock_period: u8,
    /// `ram_size`: size of RAM, if appropriate.
    pub ram_size: i32,
}

/// `struct siop_pci_common_softc`: driver internal state.
#[repr(C)]
pub struct SiopPciCommonSoftc {
    /// `sc_pc`: PCI registers info.
    pub sc_pc: Cell<Option<PciChipsetTag>>,
    /// `sc_tag`.
    pub sc_tag: Cell<Pcitag>,
    /// `sc_ih`: PCI interrupt handle.
    pub sc_ih: Cell<*mut c_void>,
    /// `sc_pp`: Adapter description.
    pub sc_pp: Cell<Option<&'static SiopProductDesc>>,
}

/// One row of [`SIOP_PRODUCTS`].
#[allow(clippy::too_many_arguments)] // the C's initialiser
const fn desc(
    product: u32,
    revision: i32,
    features: i32,
    maxburst: u8,
    maxoff: u8,
    clock_div: u8,
    clock_period: u8,
    ram_size: i32,
) -> SiopProductDesc {
    SiopProductDesc {
        product,
        revision,
        features,
        maxburst,
        maxoff,
        clock_div,
        clock_period,
        ram_size,
    }
}

/// `siop_products[]`: list (array, really :) of chips we know how to handle.
pub static SIOP_PRODUCTS: [SiopProductDesc; 18] = [
    desc(
        PCI_PRODUCT_SYMBIOS_810,
        0x00,
        SF_PCI_RL | SF_CHIP_LS,
        4,
        8,
        3,
        250,
        0,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_810,
        0x10,
        SF_PCI_RL | SF_PCI_BOF | SF_CHIP_PF | SF_CHIP_LS,
        4,
        8,
        3,
        250,
        0,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_815,
        0x00,
        SF_PCI_RL | SF_PCI_BOF,
        4,
        8,
        3,
        250,
        0,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_820,
        0x00,
        SF_PCI_RL | SF_CHIP_LS | SF_BUS_WIDE,
        4,
        8,
        3,
        250,
        0,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_825,
        0x00,
        SF_PCI_RL | SF_PCI_BOF | SF_BUS_WIDE,
        4,
        8,
        3,
        250,
        0,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_825,
        0x10,
        SF_PCI_RL
            | SF_PCI_CLS
            | SF_PCI_WRI
            | SF_PCI_RM
            | SF_CHIP_FIFO
            | SF_CHIP_PF
            | SF_CHIP_RAM
            | SF_CHIP_LS
            | SF_CHIP_10REGS
            | SF_BUS_WIDE,
        7,
        8,
        3,
        250,
        4096,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_860,
        0x00,
        SF_PCI_RL | SF_PCI_CLS | SF_PCI_WRI | SF_PCI_RM | SF_CHIP_PF | SF_CHIP_LS | SF_BUS_ULTRA,
        4,
        8,
        5,
        125,
        0,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_875,
        0x00,
        SF_PCI_RL
            | SF_PCI_CLS
            | SF_PCI_WRI
            | SF_PCI_RM
            | SF_CHIP_FIFO
            | SF_CHIP_PF
            | SF_CHIP_RAM
            | SF_CHIP_LS
            | SF_CHIP_10REGS
            | SF_BUS_ULTRA
            | SF_BUS_WIDE,
        7,
        16,
        5,
        125,
        4096,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_875,
        0x02,
        SF_PCI_RL
            | SF_PCI_CLS
            | SF_PCI_WRI
            | SF_PCI_RM
            | SF_CHIP_FIFO
            | SF_CHIP_PF
            | SF_CHIP_RAM
            | SF_CHIP_DBLR
            | SF_CHIP_LS
            | SF_CHIP_10REGS
            | SF_BUS_ULTRA
            | SF_BUS_WIDE,
        7,
        16,
        5,
        125,
        4096,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_875J,
        0x00,
        SF_PCI_RL
            | SF_PCI_CLS
            | SF_PCI_WRI
            | SF_PCI_RM
            | SF_CHIP_FIFO
            | SF_CHIP_PF
            | SF_CHIP_RAM
            | SF_CHIP_DBLR
            | SF_CHIP_LS
            | SF_CHIP_10REGS
            | SF_BUS_ULTRA
            | SF_BUS_WIDE,
        7,
        16,
        5,
        125,
        4096,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_885,
        0x00,
        SF_PCI_RL
            | SF_PCI_CLS
            | SF_PCI_WRI
            | SF_PCI_RM
            | SF_CHIP_FIFO
            | SF_CHIP_PF
            | SF_CHIP_RAM
            | SF_CHIP_DBLR
            | SF_CHIP_LS
            | SF_CHIP_10REGS
            | SF_BUS_ULTRA
            | SF_BUS_WIDE,
        7,
        16,
        5,
        125,
        4096,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_895,
        0x00,
        SF_PCI_RL
            | SF_PCI_CLS
            | SF_PCI_WRI
            | SF_PCI_RM
            | SF_CHIP_FIFO
            | SF_CHIP_PF
            | SF_CHIP_RAM
            | SF_CHIP_QUAD
            | SF_CHIP_LS
            | SF_CHIP_10REGS
            | SF_BUS_ULTRA2
            | SF_BUS_WIDE,
        7,
        31,
        7,
        62,
        4096,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_896,
        0x00,
        SF_PCI_RL
            | SF_PCI_CLS
            | SF_PCI_WRI
            | SF_PCI_RM
            | SF_CHIP_LEDC
            | SF_CHIP_FIFO
            | SF_CHIP_PF
            | SF_CHIP_RAM
            | SF_CHIP_QUAD
            | SF_CHIP_LS
            | SF_CHIP_10REGS
            | SF_BUS_ULTRA2
            | SF_BUS_WIDE,
        7,
        31,
        7,
        62,
        8192,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_895A,
        0x00,
        SF_PCI_RL
            | SF_PCI_CLS
            | SF_PCI_WRI
            | SF_PCI_RM
            | SF_CHIP_LEDC
            | SF_CHIP_FIFO
            | SF_CHIP_PF
            | SF_CHIP_RAM
            | SF_CHIP_QUAD
            | SF_CHIP_LS
            | SF_CHIP_10REGS
            | SF_BUS_ULTRA2
            | SF_BUS_WIDE,
        7,
        31,
        7,
        62,
        8192,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_1010,
        0x00,
        SF_PCI_RL
            | SF_PCI_CLS
            | SF_PCI_WRI
            | SF_PCI_RM
            | SF_CHIP_LEDC
            | SF_CHIP_FIFO
            | SF_CHIP_PF
            | SF_CHIP_RAM
            | SF_CHIP_LS
            | SF_CHIP_10REGS
            | SF_CHIP_DFBC
            | SF_CHIP_DBLR
            | SF_CHIP_GEBUG
            | SF_BUS_ULTRA3
            | SF_BUS_WIDE,
        7,
        31,
        0,
        62,
        8192,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_1010,
        0x01,
        SF_PCI_RL
            | SF_PCI_CLS
            | SF_PCI_WRI
            | SF_PCI_RM
            | SF_CHIP_LEDC
            | SF_CHIP_FIFO
            | SF_CHIP_PF
            | SF_CHIP_RAM
            | SF_CHIP_LS
            | SF_CHIP_10REGS
            | SF_CHIP_DFBC
            | SF_CHIP_DBLR
            | SF_CHIP_DT
            | SF_CHIP_GEBUG
            | SF_BUS_ULTRA3
            | SF_BUS_WIDE,
        7,
        62,
        0,
        62,
        8192,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_1010_2,
        0x00,
        SF_PCI_RL
            | SF_PCI_CLS
            | SF_PCI_WRI
            | SF_PCI_RM
            | SF_CHIP_LEDC
            | SF_CHIP_FIFO
            | SF_CHIP_PF
            | SF_CHIP_RAM
            | SF_CHIP_LS
            | SF_CHIP_10REGS
            | SF_CHIP_DFBC
            | SF_CHIP_DBLR
            | SF_CHIP_DT
            | SF_CHIP_AAIP
            | SF_BUS_ULTRA3
            | SF_BUS_WIDE,
        7,
        62,
        0,
        62,
        8192,
    ),
    desc(
        PCI_PRODUCT_SYMBIOS_1510D,
        0x00,
        SF_PCI_RL
            | SF_PCI_CLS
            | SF_PCI_WRI
            | SF_PCI_RM
            | SF_CHIP_FIFO
            | SF_CHIP_PF
            | SF_CHIP_RAM
            | SF_CHIP_QUAD
            | SF_CHIP_LS
            | SF_CHIP_10REGS
            | SF_BUS_ULTRA2
            | SF_BUS_WIDE,
        7,
        31,
        7,
        62,
        4096,
    ),
];

/// `siop_lookup_product`: the table's row for the PCI ID `id` at revision `rev`: the
/// product's row with the highest revision not above `rev`.
pub fn siop_lookup_product(id: u32, rev: i32) -> Option<&'static SiopProductDesc> {
    let mut rp: Option<&'static SiopProductDesc> = None;

    if pci_vendor(id) != PCI_VENDOR_SYMBIOS {
        return None;
    }

    for pp in SIOP_PRODUCTS.iter() {
        if pci_product(id) == pp.product
            && pp.revision <= rev
            && rp.is_none_or(|r| pp.revision > r.revision)
        {
            rp = Some(pp);
        }
    }
    rp
}

/// `siop_pci_attach_common`: copies the chip's description into the common softc, maps
/// its registers (memory space, else I/O space) and on-board RAM and establishes `intr`;
/// false (with the message printed) when the adapter cannot be used.
pub fn siop_pci_attach_common(
    pci_sc: &SiopPciCommonSoftc,
    siop_sc: &'static SiopCommonSoftc,
    pa: &PciAttachArgs,
    intr: PciIntrFn,
) -> bool {
    let pc = pa.pa_pc;
    let tag = pa.pa_tag;

    let Some(pp) = siop_lookup_product(pa.pa_id, pci_revision(pa.pa_class) as i32) else {
        printf(format_args!(": broken match/attach!\n"));
        return false;
    };
    pci_sc.sc_pp.set(Some(pp));
    // copy interesting infos about the chip
    siop_sc.features.set(pp.features);
    siop_sc.maxburst.set(i32::from(pp.maxburst));
    siop_sc.maxoff.set(i32::from(pp.maxoff));
    siop_sc.clock_div.set(i32::from(pp.clock_div));
    siop_sc.clock_period.set(i32::from(pp.clock_period));
    siop_sc.ram_size.set(pp.ram_size);

    siop_sc.sc_reset.set(Some(siop_pci_reset));
    pci_sc.sc_pc.set(Some(pc));
    pci_sc.sc_tag.set(tag);
    siop_sc.sc_dmat.set(Some(pa.pa_dmat));

    let memtype = pci_mapreg_type(pa.pa_pc, pa.pa_tag, 0x14);
    let mem = if memtype == PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_32BIT
        || memtype == PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_64BIT
    {
        pci_mapreg_map(pa, 0x14, memtype, 0, 0).ok()
    } else {
        None
    };

    let io = pci_mapreg_map(pa, 0x10, PCI_MAPREG_TYPE_IO, 0, 0).ok();

    if let Some((memt, memh, memaddr, _)) = mem {
        siop_sc.sc_rt.set(Some(memt));
        siop_sc.sc_rh.set(Some(memh));
        siop_sc.sc_raddr.set(memaddr);
    } else if let Some((iot, ioh, ioaddr, _)) = io {
        siop_sc.sc_rt.set(Some(iot));
        siop_sc.sc_rh.set(Some(ioh));
        siop_sc.sc_raddr.set(ioaddr);
    } else {
        printf(format_args!(": unable to map device registers\n"));
        return false;
    }

    let attached = 'out: {
        let Some(intrhandle) = pci_intr_map(pa) else {
            printf(format_args!(": couldn't map interrupt\n"));
            break 'out false;
        };
        let intrstr = pci_intr_string(pa.pa_pc, intrhandle);
        match pci_intr_establish(
            pa.pa_pc,
            intrhandle,
            IPL_BIO,
            intr,
            ptr::from_ref(siop_sc).cast_mut().cast(),
            siop_sc.sc_dev.xname(),
        ) {
            Some(ih) => {
                pci_sc.sc_ih.set(ih.as_ptr());
                printf(format_args!(": {intrstr}"));
            }
            None => {
                printf(format_args!(
                    ": couldn't establish interrupt at {intrstr}\n"
                ));
                break 'out false;
            }
        }

        if siop_sc.features.get() & SF_CHIP_RAM != 0 {
            let bar = if memtype == PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_32BIT {
                0x18
            } else if memtype == PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_64BIT {
                0x1c
            } else {
                printf(format_args!(": invalid memory type {memtype}\n"));
                break 'out false;
            };
            match pci_mapreg_map(pa, bar, memtype, 0, 0) {
                Ok((ramt, ramh, ramaddr, ramsize)) => {
                    siop_sc.sc_ramt.set(Some(ramt));
                    siop_sc.sc_ramh.set(Some(ramh));
                    siop_sc.sc_scriptaddr.set(ramaddr);
                    printf(format_args!(", using {}K of on-board RAM", ramsize / 1024));
                }
                Err(_) => {
                    printf(format_args!(", can't map on-board RAM"));
                    siop_sc.features.set(siop_sc.features.get() & !SF_CHIP_RAM);
                }
            }
        }

        printf(format_args!("\n"));

        true
    };
    if attached {
        return true;
    }

    // out:
    if let Some(ih) = NonNull::new(pci_sc.sc_ih.replace(ptr::null_mut())) {
        // SAFETY: the handle established above, dropped here.
        unsafe { pci_intr_disestablish(pa.pa_pc, ih) };
    }
    if let Some((iot, ioh, _, iosize)) = io {
        bus_space_unmap(iot, ioh, iosize);
    }
    if let Some((memt, memh, _, memsize)) = mem {
        bus_space_unmap(memt, memh, memsize);
    }
    false
}

/// `siop_pci_reset`: the PCI part of the chip reset (the softc's `sc_reset`): burst
/// length, read line/multiple, cache line size and write-and-invalidate.
pub fn siop_pci_reset(sc: &SiopCommonSoftc) {
    let features = sc.features.get();

    let mut dmode = sc.read_1(SIOP_DMODE);
    if features & SF_PCI_RL != 0 {
        dmode |= DMODE_ERL;
    }
    if features & SF_PCI_RM != 0 {
        dmode |= DMODE_ERMP;
    }
    if features & SF_PCI_BOF != 0 {
        dmode |= DMODE_BOF;
    }
    if features & SF_PCI_CLS != 0 {
        sc.write_1(SIOP_DCNTL, sc.read_1(SIOP_DCNTL) | DCNTL_CLSE);
    }
    if features & SF_PCI_WRI != 0 {
        sc.write_1(SIOP_CTEST3, sc.read_1(SIOP_CTEST3) | CTEST3_WRIE);
    }
    let maxburst = sc.maxburst.get();
    if maxburst != 0 {
        let mut ctest5 = sc.read_1(SIOP_CTEST5);
        sc.write_1(SIOP_CTEST4, sc.read_1(SIOP_CTEST4) & !CTEST4_BDIS);
        dmode &= !DMODE_BL_MASK;
        dmode |= (((maxburst - 1) << DMODE_BL_SHIFT) as u8) & DMODE_BL_MASK;
        ctest5 &= !CTEST5_BBCK;
        ctest5 |= ((maxburst - 1) as u8) & CTEST5_BBCK;
        sc.write_1(SIOP_CTEST5, ctest5);
    } else {
        sc.write_1(SIOP_CTEST4, sc.read_1(SIOP_CTEST4) | CTEST4_BDIS);
    }
    sc.write_1(SIOP_DMODE, dmode);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_takes_the_highest_revision_not_above_the_chip_s() {
        // QEMU's lsi53c895a: 1000:0012, revision 0.
        let pp = siop_lookup_product(0x0012_1000, 0).unwrap();
        assert_eq!(pp.product, PCI_PRODUCT_SYMBIOS_895A);
        assert_eq!((pp.maxoff, pp.clock_period, pp.ram_size), (31, 62, 8192));
        assert_ne!(pp.features & SF_CHIP_RAM, 0);
        // The 810 at revision 0x12 takes the 0x10 row, at 0x02 the 0x00 row.
        assert_eq!(
            siop_lookup_product(0x0001_1000, 0x12).unwrap().revision,
            0x10
        );
        assert_eq!(
            siop_lookup_product(0x0001_1000, 0x02).unwrap().revision,
            0x00
        );
        // The 1010-33 rev 0 has no DT; rev 1 has.
        assert_eq!(
            siop_lookup_product(0x0020_1000, 0).unwrap().features & SF_CHIP_DT,
            0
        );
        assert_ne!(
            siop_lookup_product(0x0020_1000, 1).unwrap().features & SF_CHIP_DT,
            0
        );
        // Another vendor, or an unknown product, is not a siop.
        assert!(siop_lookup_product(0x0012_1001, 0).is_none());
        assert!(siop_lookup_product(0x0030_1000, 0).is_none());
    }
}
/* </TESTS> */
