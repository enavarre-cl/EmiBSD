/*	$OpenBSD: pci_machdep.c,v 1.7 2024/07/05 22:53:57 patrick Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2019 Mark Kettenis <kettenis@openbsd.org>
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

//! The arm64 PCI functions every host bridge shares: `arch/arm64/dev/pci_machdep.c`. MSI and
//! MSI-X are programmed here in the function's capability and table (the address and data
//! come from the bridge's MSI controller), and the `_pci_intr_map_msi*` functions are what
//! the host bridges put in their chipset's `pc_intr_map_msi*` members.
//!
//! Upstream: sys/arch/arm64/dev/pci_machdep.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `pci_intr_enable_msivec` returns `true` where the C returns 1 (it cannot); the
//!   `_pci_intr_map_msi*` functions return the handle, `None` for the C's `-1`;
//!   `pci_msix_table_map` returns the handle, or the errno of `pci_mapreg_info` or
//!   `bus_space_map` where the C returns `-1` (as amd64's does).
//! - The "no msi/msix capability" and "cannot map registers" panics keep the C's text with
//!   the function name the C's `__func__` gives.

use crate::arch::arm64::include::pci_machdep::{
    PCI_MSI, PCI_MSIX, PciChipsetTag, PciIntrHandle, Pcitag,
};
use crate::dev::pci::pci::pci_get_capability;
use crate::dev::pci::pci_map::{pci_mapreg_assign, pci_mapreg_info, pci_mapreg_type};
use crate::dev::pci::pcireg::{
    PCI_CAP_MSI, PCI_CAP_MSIX, PCI_MAPREG_START, PCI_MSI_MA, PCI_MSI_MAU32, PCI_MSI_MC_C64,
    PCI_MSI_MC_MMC_MASK, PCI_MSI_MC_MMC_SHIFT, PCI_MSI_MC_MME_MASK, PCI_MSI_MC_MME_SHIFT,
    PCI_MSI_MC_MSIE, PCI_MSI_MD32, PCI_MSI_MD64, PCI_MSIX_MC_MSIXE, PCI_MSIX_TABLE,
    PCI_MSIX_TABLE_BIR, PCI_MSIX_TABLE_OFF, PCI_MSIX_VC_MASK, pci_msix_ma, pci_msix_mau32,
    pci_msix_mc_tblsz, pci_msix_md, pci_msix_vc,
};
use crate::dev::pci::pcivar::{PCI_FLAGS_MSI_ENABLED, PCI_FLAGS_MSIVEC_ENABLED, PciAttachArgs};
use crate::kassert;
use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BUS_SPACE_BARRIER_WRITE, BusAddr, BusSpaceHandle, BusSpaceTag, bus_space_barrier,
    bus_space_map, bus_space_read_4, bus_space_unmap, bus_space_write_4,
};
use crate::machine::pci_machdep::{pci_conf_read, pci_conf_write};
use crate::sys::errno::Errno;

/// `pci_intr_enable_msivec`: enables `num_vec` MSI vectors of the function (its Multiple
/// Message Enable); `true` when the bus or the function cannot.
pub fn pci_intr_enable_msivec(pa: &PciAttachArgs, num_vec: i32) -> bool {
    let pc = pa.pa_pc;
    let tag = pa.pa_tag;

    if pa.pa_flags & PCI_FLAGS_MSIVEC_ENABLED == 0 {
        return true;
    }
    let Some((off, mut reg)) = pci_get_capability(pc, tag, PCI_CAP_MSI) else {
        return true;
    };

    let mmc = (reg & PCI_MSI_MC_MMC_MASK) >> PCI_MSI_MC_MMC_SHIFT;
    if num_vec > (1 << mmc) {
        return true;
    }

    let mut mme = (reg & PCI_MSI_MC_MME_MASK) >> PCI_MSI_MC_MME_SHIFT;
    while (1 << mme) < num_vec {
        mme += 1;
    }
    reg &= !PCI_MSI_MC_MME_MASK;
    reg |= mme << PCI_MSI_MC_MME_SHIFT;
    pci_conf_write(pc, tag, off, reg);

    false
}

/// `pci_msi_enable`: points the function's MSI at `addr` with `data` (its low bits left to
/// the vectors the function has enabled) and enables it.
pub fn pci_msi_enable(pc: PciChipsetTag, tag: Pcitag, addr: BusAddr, data: u32) {
    let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSI) else {
        panic(format_args!("pci_msi_enable: no msi capability"));
    };

    let mme = (reg & PCI_MSI_MC_MME_MASK) >> PCI_MSI_MC_MME_SHIFT;
    let data = data & !((1u32 << mme) - 1);

    if reg & PCI_MSI_MC_C64 != 0 {
        pci_conf_write(pc, tag, off + PCI_MSI_MA, addr as u32);
        pci_conf_write(pc, tag, off + PCI_MSI_MAU32, (addr as u64 >> 32) as u32);
        pci_conf_write(pc, tag, off + PCI_MSI_MD64, data);
    } else {
        pci_conf_write(pc, tag, off + PCI_MSI_MA, addr as u32);
        pci_conf_write(pc, tag, off + PCI_MSI_MD32, data);
    }
    pci_conf_write(pc, tag, off, reg | PCI_MSI_MC_MSIE);
}

/// `pci_msix_table_map`: maps the function's MSI-X table, which lives in one of its BARs.
pub fn pci_msix_table_map(
    pc: PciChipsetTag,
    tag: Pcitag,
    memt: BusSpaceTag,
) -> Result<BusSpaceHandle, Errno> {
    let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        panic(format_args!("pci_msix_table_map: no msix capability"));
    };

    let table = pci_conf_read(pc, tag, off + PCI_MSIX_TABLE);
    let bir = table & PCI_MSIX_TABLE_BIR;
    let offset = (table & PCI_MSIX_TABLE_OFF) as BusAddr;
    let tblsz = pci_msix_mc_tblsz(reg) as usize + 1;

    let bir = PCI_MAPREG_START + bir as i32 * 4;
    let type_ = pci_mapreg_type(pc, tag, bir);
    let (base, _, _) = pci_mapreg_info(pc, tag, bir, type_)?;
    // SAFETY: the table lives in the function's own BAR, as its MSI-X capability says.
    unsafe { bus_space_map(memt, base + offset, tblsz * 16, 0) }
}

/// `pci_msix_table_unmap`.
pub fn pci_msix_table_unmap(
    pc: PciChipsetTag,
    tag: Pcitag,
    memt: BusSpaceTag,
    memh: BusSpaceHandle,
) {
    let Some((_, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        panic(format_args!("pci_msix_table_unmap: no msix capability"));
    };

    let tblsz = pci_msix_mc_tblsz(reg) as usize + 1;
    bus_space_unmap(memt, memh, tblsz * 16);
}

/// `pci_msix_enable`: points MSI-X table entry `vec` at `addr` with `data`, unmasks it and
/// enables MSI-X.
pub fn pci_msix_enable(
    pc: PciChipsetTag,
    tag: Pcitag,
    memt: BusSpaceTag,
    vec: i32,
    addr: BusAddr,
    data: u32,
) {
    let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        panic(format_args!("pci_msix_enable: no msix capability"));
    };

    kassert!(vec as u32 <= pci_msix_mc_tblsz(reg));

    let Ok(memh) = pci_msix_table_map(pc, tag, memt) else {
        panic(format_args!("pci_msix_enable: cannot map registers"));
    };

    let vec = vec as usize;
    bus_space_write_4(memt, memh, pci_msix_ma(vec), addr as u32);
    bus_space_write_4(memt, memh, pci_msix_mau32(vec), (addr as u64 >> 32) as u32);
    bus_space_write_4(memt, memh, pci_msix_md(vec), data);
    bus_space_barrier(memt, memh, pci_msix_ma(vec), 16, BUS_SPACE_BARRIER_WRITE);
    let ctrl = bus_space_read_4(memt, memh, pci_msix_vc(vec));
    bus_space_write_4(memt, memh, pci_msix_vc(vec), ctrl & !PCI_MSIX_VC_MASK);

    pci_msix_table_unmap(pc, tag, memt, memh);

    pci_conf_write(pc, tag, off, reg | PCI_MSIX_MC_MSIXE);
}

/// `_pci_intr_map_msi`: the function's MSI, when the bus allows MSI and the function has
/// the capability.
pub fn _pci_intr_map_msi(pa: &PciAttachArgs) -> Option<PciIntrHandle> {
    let pc = pa.pa_pc;
    let tag = pa.pa_tag;

    if pa.pa_flags & PCI_FLAGS_MSI_ENABLED == 0 {
        return None;
    }
    pci_get_capability(pc, tag, PCI_CAP_MSI)?;

    Some(PciIntrHandle {
        ih_pc: pa.pa_pc,
        ih_tag: pa.pa_tag,
        ih_intrpin: 0,
        ih_type: PCI_MSI,
        ih_dmat: pa.pa_dmat,
    })
}

/// `_pci_intr_map_msivec`: MSI vector `vec`, among those `pci_intr_enable_msivec` enabled.
pub fn _pci_intr_map_msivec(pa: &PciAttachArgs, vec: i32) -> Option<PciIntrHandle> {
    let pc = pa.pa_pc;
    let tag = pa.pa_tag;

    if pa.pa_flags & PCI_FLAGS_MSIVEC_ENABLED == 0 {
        return None;
    }
    let (_, reg) = pci_get_capability(pc, tag, PCI_CAP_MSI)?;

    let mme = (reg & PCI_MSI_MC_MME_MASK) >> PCI_MSI_MC_MME_SHIFT;
    if vec >= (1 << mme) {
        return None;
    }

    Some(PciIntrHandle {
        ih_pc: pa.pa_pc,
        ih_tag: pa.pa_tag,
        ih_intrpin: vec,
        ih_type: PCI_MSI,
        ih_dmat: pa.pa_dmat,
    })
}

/// `_pci_intr_map_msix`: MSI-X table entry `vec`; the BAR holding the table must be
/// assignable.
pub fn _pci_intr_map_msix(pa: &PciAttachArgs, vec: i32) -> Option<PciIntrHandle> {
    let pc = pa.pa_pc;
    let tag = pa.pa_tag;

    if pa.pa_flags & PCI_FLAGS_MSI_ENABLED == 0 {
        return None;
    }
    let (off, reg) = pci_get_capability(pc, tag, PCI_CAP_MSIX)?;

    if vec as u32 > pci_msix_mc_tblsz(reg) {
        return None;
    }

    let table = pci_conf_read(pc, tag, off + PCI_MSIX_TABLE);
    let bir = PCI_MAPREG_START + (table & PCI_MSIX_TABLE_BIR) as i32 * 4;
    let type_ = pci_mapreg_type(pc, tag, bir);
    pci_mapreg_assign(pa, bir, type_).ok()?;

    Some(PciIntrHandle {
        ih_pc: pa.pa_pc,
        ih_tag: pa.pa_tag,
        ih_intrpin: vec,
        ih_type: PCI_MSIX,
        ih_dmat: pa.pa_dmat,
    })
}
