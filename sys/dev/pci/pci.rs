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
 * Copyright (c) 1995, 1996 Christopher G. Demetriou.  All rights reserved.
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
//! PCI bus autoconfiguration: `dev/pci/pci.c`.
//!
//! Upstream: sys/dev/pci/pci.c @ 3ce1f3f79392
//!
//! `pci* at mainbus0` (or below a bridge) attaches with a `pcibus_attach_args`; `pciattach`
//! walks the bus three times with `pci_enumerate_bus`: to reserve the resources the firmware
//! assigned, to find the active VGA device, and to probe every function, offering each to
//! the drivers with `config_found_sm`. A function no driver takes is printed by `pciprint`
//! ("vendor 0x8086 product 0x29c0 (class bridge subclass host, rev 0x02) at pci0 dev 0
//! function 0 not configured"). The capability walkers, the power state functions and the
//! MSI-X table save/restore used across suspend live here too.
//!
//! Important note about PCI-ISA bridges: callbacks (`config_defer`) are used to configure
//! these devices so that ISA/EISA bridges can attach their child busses after PCI
//! configuration is done. This works because there can be at most one ISA/EISA bridge per
//! PCI bus and any ISA/EISA bridges must be attached to primary PCI busses (bus zero): some
//! legacy PCI devices (VGA controllers) can show up as ISA devices as well, and probing them
//! from the ISA side first would complicate or break their PCI attachment.
//!
//! ## Deviations
//! - The bus's extents come from the host bridge (M16b: arm64's `pciecam` and `acpipci`);
//!   amd64's are `None` until its `pci_init_extents` is ported, and there
//!   `pci_reserve_resources` only sizes the BARs and the ROM, as the C with NULL extents.
//!   `__sparc64__`'s exception for T5's 64-bit BARs is not compiled.
//! - `USER_PCICONF` (in GENERIC): `pciopen`, `pciclose`, `pciioctl` and `pci_vga_proc`
//!   need `sys/pciio.h` and the `cdevsw` and are not ported; `pci_disable_vga`,
//!   `pci_enable_vga`, `pci_route_vga` and `pci_unroute_vga` are.
//! - `pci_vga_pci` is an `AtomicPtr` and `pci_vga_tag` an `Option` in a `StaticCell` (both
//!   written by autoconfiguration only).
//! - Out pointers are return values: the capability walkers return `(offset, value)`.
//!   `pci_probe_device`/`pci_enumerate_bus`/`pci_find_device` take the match function as an
//!   `Option<fn(&PciAttachArgs) -> i32>` and the result slot as `Option<&mut
//!   PciAttachArgs>`.
//! - `struct pci_dev` is malloc'd as in C; a failed `M_WAITOK` allocation panics (the C
//!   cannot fail there). `pd_msix_table` is a raw pointer to the `mallocarray`'d table.
//! - `pci_vpd_read` reads `data.len()` registers (the C's `count`).
//! - `__HAVE_PCI_MSIX` is defined on both architectures, so the MSI-X versions of the table
//!   functions are the ones ported.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use libkern::StaticCell;

use crate::dev::pci::pci_map::{pci_mapreg_info, pci_mapreg_probe};
use crate::dev::pci::pci_quirks::pci_lookup_quirkdata;
use crate::dev::pci::pci_subr::pci_devinfo;
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_AMD_17_1X_XHCI_1, PCI_PRODUCT_AMD_17_1X_XHCI_2, PCI_PRODUCT_AMD_17_6X_XHCI,
    PCI_VENDOR_AMD, PCI_VENDOR_INVALID,
};
use crate::dev::pci::pcireg::*;
use crate::dev::pci::pcivar::{
    PCI_FLAGS_IO_ENABLED, PCI_FLAGS_MEM_ENABLED, PCI_FLAGS_MSI_ENABLED, PCI_QUIRK_MONOFUNCTION,
    PCI_QUIRK_MULTIFUNCTION, PCI_UNK_DEV, PCI_UNK_FUNCTION, PCIBUS_UNK_BUS, PciAttachArgs,
    PciMatchid, PciSoftc, PcibusAttachArgs, Pcireg, pcibuscf_bus, pcicf_dev, pcicf_function,
};
use crate::dev::pci::ppbreg::{
    PPB_BC_VGA_ENABLE, PPB_REG_BRIDGECONTROL, PPB_REG_BUSINFO, PPB_REG_IO_HI, PPB_REG_IOSTATUS,
    PPB_REG_MEM, PPB_REG_PREFBASE_HI32, PPB_REG_PREFLIM_HI32, PPB_REG_PREFMEM,
    ppb_businfo_secondary, ppb_businfo_subordinate,
};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::subr_autoconf::{
    config_activate_children, config_detach_children, config_found_sm,
};
use crate::kern::subr_extent::extent_alloc_region;
use crate::kern::subr_prf::{Str, panic, printf};
use crate::machine::bus::{
    BUS_SPACE_BARRIER_WRITE, BusSpaceTag, bus_space_barrier, bus_space_read_4, bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{splhigh, splx};
use crate::machine::pci_machdep::{
    PciChipsetTag, Pcitag, pci_attach_hook, pci_bus_maxdevs, pci_conf_read, pci_conf_write,
    pci_decompose_tag, pci_dev_postattach, pci_make_tag, pci_min_powerstate, pci_msix_table_map,
    pci_msix_table_unmap, pci_probe_device_hook, pci_set_powerstate_md,
};
use crate::queue_adapter;
use crate::sys::device::{
    CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_POWERDOWN, DVACT_RESUME, DVACT_SUSPEND,
    Device, UNCONF,
};
use crate::sys::errno::Errno;
use crate::sys::extent::{EX_NOWAIT, Extent};
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::queue::ListEntry;

/// `NMAPREG`: the number of base address registers of a type 0 header.
pub const NMAPREG: usize = (PCI_MAPREG_END - PCI_MAPREG_START) as usize / size_of::<Pcireg>();

/// `BUS_SPACE_MAP_PREFETCHABLE`, as `pci_mapreg_info` reports it in its flags.
const PREFETCHABLE: u32 =
    <crate::machine::Machine as crate::machine::BusSpace>::BUS_SPACE_MAP_PREFETCHABLE;

/// `struct msix_vector`: one MSI-X table entry, saved across suspend.
#[derive(Clone, Copy, Debug, Default)]
pub struct MsixVector {
    /// `mv_ma`: message address.
    pub mv_ma: u32,
    /// `mv_mau32`: message upper address.
    pub mv_mau32: u32,
    /// `mv_md`: message data.
    pub mv_md: u32,
    /// `mv_vc`: vector control.
    pub mv_vc: u32,
}

/// `struct pci_dev`: what the bus remembers about each function it probed. `#[repr(C)]` of
/// `Cell`s, allocated zeroed; the bus's lists are touched by autoconfiguration only.
#[repr(C)]
pub struct PciDev {
    /// `pd_dev`: the attached driver, if any.
    pub pd_dev: Cell<Option<NonNull<Device>>>,
    /// `pd_next`.
    pub pd_next: ListEntry<PciDev>,
    /// `pd_tag`: pci register tag.
    pub pd_tag: Cell<Pcitag>,
    /// `pd_csr`.
    pub pd_csr: Cell<Pcireg>,
    /// `pd_bhlc`.
    pub pd_bhlc: Cell<Pcireg>,
    /// `pd_int`.
    pub pd_int: Cell<Pcireg>,
    /// `pd_map`: the BARs, saved across suspend.
    pub pd_map: [Cell<Pcireg>; NMAPREG],
    /// `pd_mask`: what each BAR read back after all ones were written.
    pub pd_mask: [Cell<Pcireg>; NMAPREG],
    /// `pd_msi_mc`.
    pub pd_msi_mc: Cell<Pcireg>,
    /// `pd_msi_ma`.
    pub pd_msi_ma: Cell<Pcireg>,
    /// `pd_msi_mau32`.
    pub pd_msi_mau32: Cell<Pcireg>,
    /// `pd_msi_md`.
    pub pd_msi_md: Cell<Pcireg>,
    /// `pd_msix_mc`.
    pub pd_msix_mc: Cell<Pcireg>,
    /// `pd_msix_table`: `pci_alloc_msix_table`'s array, or null.
    pub pd_msix_table: Cell<*mut MsixVector>,
    /// `pd_pmcsr_state`.
    pub pd_pmcsr_state: Cell<i32>,
    /// `pd_vga_decode`.
    pub pd_vga_decode: Cell<i32>,
}

queue_adapter!(
    /// `LIST_ENTRY(pci_dev) pd_next`: a bus's `sc_devs`.
    pub PciDevList: PciDev, pd_next => ListEntry<PciDev>
);

/// `pci_ca`.
pub static PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PciSoftc>(),
    ca_match: Some(pcimatch),
    ca_attach: pciattach,
    ca_detach: Some(pcidetach),
    ca_activate: Some(pciactivate),
};

/// `pci_cd`.
pub static PCI_CD: Cfdriver = Cfdriver::new(b"pci", DV_DULL, CD_COCOVM);

/// `pci_ndomains`: PCI domains handed out so far.
pub static PCI_NDOMAINS: AtomicI32 = AtomicI32::new(0);

/// `pci_vga_pci`: the bus of the VGA device that is currently active.
pub static PCI_VGA_PCI: AtomicPtr<PciSoftc> = AtomicPtr::new(ptr::null_mut());

/// `pci_vga_tag`: that VGA device.
pub static PCI_VGA_TAG: StaticCell<Option<Pcitag>> = StaticCell::new(None);

/// `pci_dopm`: put devices into their lowest power state on powerdown.
pub static PCI_DOPM: AtomicI32 = AtomicI32::new(0);

/// `(struct pci_softc *)self`.
fn softc(dev: &Device) -> &PciSoftc {
    // SAFETY: only called with devices `pci_ca` made (pci's own functions, and children of
    // a pci bus whose parent is checked to be one).
    unsafe { dev.softc::<PciSoftc>() }
}

/// `pcimatch`.
pub fn pcimatch(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();
    // SAFETY: a `pci` bus is only offered by a parent that hands over pcibus_attach_args.
    let pba = unsafe { &*aux.cast::<PcibusAttachArgs>() };

    if pba.pba_busname != cf.cf_driver.cd_name {
        return 0;
    }

    // Check the locators
    if pcibuscf_bus(cf) != PCIBUS_UNK_BUS && pcibuscf_bus(cf) != pba.pba_bus {
        return 0;
    }

    // sanity
    if pba.pba_bus < 0 || pba.pba_bus > 255 {
        return 0;
    }

    // XXX check other (hardware?) indicators

    1
}

/// `pciattach`.
pub fn pciattach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: as in `pcimatch`.
    let pba = unsafe { &*aux.cast::<PcibusAttachArgs>() };
    let sc = softc(self_);

    if let Some(parent) = parent {
        pci_attach_hook(parent, self_, pba);
    }

    printf(format_args!("\n"));

    sc.sc_devs.init();

    sc.sc_iot.set(Some(pba.pba_iot));
    sc.sc_memt.set(Some(pba.pba_memt));
    sc.sc_dmat.set(Some(pba.pba_dmat));
    sc.sc_pc.set(Some(pba.pba_pc));
    sc.sc_flags.set(pba.pba_flags);
    sc.sc_ioex.set(pba.pba_ioex);
    sc.sc_memex.set(pba.pba_memex);
    sc.sc_pmemex.set(pba.pba_pmemex);
    sc.sc_busex.set(pba.pba_busex);
    sc.sc_domain.set(pba.pba_domain);
    sc.sc_bus.set(pba.pba_bus);
    sc.sc_bridgetag.set(pba.pba_bridgetag);
    sc.sc_bridgeih.set(pba.pba_bridgeih);
    sc.sc_maxndevs.set(pci_bus_maxdevs(pba.pba_pc, pba.pba_bus));
    sc.sc_intrswiz.set(pba.pba_intrswiz);
    sc.sc_intrtag.set(pba.pba_intrtag);

    // Reserve our own bus number.
    if let Some(busex) = sc.sc_busex.get() {
        let _ = extent_alloc_region(busex, sc.sc_bus.get() as u64, 1, EX_NOWAIT);
    }

    pci_enumerate_bus(sc, Some(pci_reserve_resources), None);

    // Find the VGA device that's currently active.
    if pci_enumerate_bus(sc, Some(pci_primary_vga), None) != 0 {
        PCI_VGA_PCI.store(ptr::from_ref(sc).cast_mut(), Ordering::Relaxed);
    }

    pci_enumerate_bus(sc, None, None);
}

/// `pcidetach`.
pub fn pcidetach(self_: &Device, flags: i32) -> Result<(), Errno> {
    pci_detach_devices(softc(self_), flags)
}

/// `pciactivate`.
pub fn pciactivate(self_: &Device, act: i32) -> Result<(), Errno> {
    match act {
        DVACT_SUSPEND => {
            let rv = config_activate_children(self_, act);
            pci_suspend(softc(self_));
            rv
        }
        DVACT_RESUME => {
            pci_resume(softc(self_));
            config_activate_children(self_, act)
        }
        DVACT_POWERDOWN => {
            let rv = config_activate_children(self_, act);
            pci_powerdown(softc(self_));
            rv
        }
        _ => config_activate_children(self_, act),
    }
}

/// The MSI-X table of `pd`, `tblsz` entries long.
fn msix_table<'a>(pd: &PciDev, tblsz: usize) -> Option<&'a mut [MsixVector]> {
    let table = pd.pd_msix_table.get();
    if table.is_null() {
        return None;
    }
    // SAFETY: `pci_alloc_msix_table` made the table with `PCI_MSIX_MC_TBLSZ + 1` entries,
    // which is `tblsz`, and only the bus's suspend and resume paths touch it.
    Some(unsafe { slice::from_raw_parts_mut(table, tblsz) })
}

/// `pci_suspend`: saves the registers of the bus's type 0 functions that may get lost.
pub fn pci_suspend(sc: &PciSoftc) {
    let pc = sc.pc();
    for pd in sc.sc_devs.iter() {
        let tag = pd.pd_tag.get();
        // Only handle header type 0 here; PCI-PCI bridges and CardBus bridges need special
        // handling, which will be done in their specific drivers.
        let bhlc = pci_conf_read(pc, tag, PCI_BHLC_REG);
        if pci_hdrtype_type(bhlc) != 0 {
            continue;
        }

        // Save registers that may get lost.
        for (i, map) in pd.pd_map.iter().enumerate() {
            map.set(pci_conf_read(pc, tag, PCI_MAPREG_START + (i as i32 * 4)));
        }
        pd.pd_csr
            .set(pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG));
        pd.pd_bhlc.set(pci_conf_read(pc, tag, PCI_BHLC_REG));
        pd.pd_int.set(pci_conf_read(pc, tag, PCI_INTERRUPT_REG));

        if let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSI) {
            pd.pd_msi_ma.set(pci_conf_read(pc, tag, off + PCI_MSI_MA));
            if reg & PCI_MSI_MC_C64 != 0 {
                pd.pd_msi_mau32
                    .set(pci_conf_read(pc, tag, off + PCI_MSI_MAU32));
                pd.pd_msi_md.set(pci_conf_read(pc, tag, off + PCI_MSI_MD64));
            } else {
                pd.pd_msi_md.set(pci_conf_read(pc, tag, off + PCI_MSI_MD32));
            }
            pd.pd_msi_mc.set(reg);
        }

        if let Some(memt) = sc.sc_memt.get() {
            let mut mc = pd.pd_msix_mc.get();
            pci_suspend_msix(pc, tag, memt, &mut mc, pd);
            pd.pd_msix_mc.set(mc);
        }

        pd.pd_pmcsr_state.set(pci_get_powerstate(pc, tag));
    }
}

/// `pci_powerdown`: puts the bus's type 0 functions into their lowest power state when
/// `pci_dopm` is set.
pub fn pci_powerdown(sc: &PciSoftc) {
    let pc = sc.pc();
    for pd in sc.sc_devs.iter() {
        let tag = pd.pd_tag.get();
        // Only handle header type 0 here (see pci_suspend).
        let bhlc = pci_conf_read(pc, tag, PCI_BHLC_REG);
        if pci_hdrtype_type(bhlc) != 0 {
            continue;
        }

        if PCI_DOPM.load(Ordering::Relaxed) != 0 {
            // Place the device into the lowest possible power state.
            pci_set_powerstate(pc, tag, pci_min_powerstate(pc, tag) as i32);
        }
    }
}

/// `pci_resume`: restores what `pci_suspend` saved.
pub fn pci_resume(sc: &PciSoftc) {
    let pc = sc.pc();
    for pd in sc.sc_devs.iter() {
        let tag = pd.pd_tag.get();
        // Only handle header type 0 here (see pci_suspend).
        let bhlc = pci_conf_read(pc, tag, PCI_BHLC_REG);
        if pci_hdrtype_type(bhlc) != 0 {
            continue;
        }

        // Restore power.
        if PCI_DOPM.load(Ordering::Relaxed) != 0 {
            pci_set_powerstate(pc, tag, pd.pd_pmcsr_state.get());
        }

        // Restore the registers saved above.
        for (i, map) in pd.pd_map.iter().enumerate() {
            pci_conf_write(pc, tag, PCI_MAPREG_START + (i as i32 * 4), map.get());
        }
        let reg = pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG);
        pci_conf_write(
            pc,
            tag,
            PCI_COMMAND_STATUS_REG,
            (reg & 0xffff_0000) | (pd.pd_csr.get() & 0x0000_ffff),
        );
        pci_conf_write(pc, tag, PCI_BHLC_REG, pd.pd_bhlc.get());
        pci_conf_write(pc, tag, PCI_INTERRUPT_REG, pd.pd_int.get());

        if let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSI) {
            pci_conf_write(pc, tag, off + PCI_MSI_MA, pd.pd_msi_ma.get());
            if reg & PCI_MSI_MC_C64 != 0 {
                pci_conf_write(pc, tag, off + PCI_MSI_MAU32, pd.pd_msi_mau32.get());
                pci_conf_write(pc, tag, off + PCI_MSI_MD64, pd.pd_msi_md.get());
            } else {
                pci_conf_write(pc, tag, off + PCI_MSI_MD32, pd.pd_msi_md.get());
            }
            pci_conf_write(pc, tag, off + PCI_MSI_MC, pd.pd_msi_mc.get());
        }

        if let Some(memt) = sc.sc_memt.get() {
            pci_resume_msix(pc, tag, memt, pd.pd_msix_mc.get(), pd);
        }
    }
}

/// `pciprint`: names a function no driver took ("... at pci0 dev 1 function 0").
pub fn pciprint(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: pci hands its children pci_attach_args.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let mut devinfo = [0u8; 256];

    if let Some(pnp) = pnp {
        pci_devinfo(pa.pa_id, pa.pa_class, true, &mut devinfo);
        printf(format_args!("{} at {}", Str(&devinfo), Str(pnp)));
    }
    printf(format_args!(
        " dev {} function {}",
        pa.pa_device, pa.pa_function
    ));
    if pnp.is_none() {
        pci_devinfo(pa.pa_id, pa.pa_class, false, &mut devinfo);
        printf(format_args!(" {}", Str(&devinfo)));
    }

    UNCONF
}

/// `pcisubmatch`: checks the `dev` and `function` locators before the driver's match.
pub fn pcisubmatch(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();
    // SAFETY: as in `pciprint`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    if pcicf_dev(cf) != PCI_UNK_DEV && pcicf_dev(cf) != pa.pa_device as i32 {
        return 0;
    }
    if pcicf_function(cf) != PCI_UNK_FUNCTION && pcicf_function(cf) != pa.pa_function as i32 {
        return 0;
    }

    cf.cf_attach
        .ca_match
        .map_or(0, |ca_match| ca_match(parent, match_, aux))
}

/// `pci_probe_device`: builds the attach arguments of the function at `tag`; with `match_`
/// it only asks the function (and stores the arguments in `pap` when it answers nonzero),
/// without it the function is recorded on the bus and offered to the drivers.
pub fn pci_probe_device(
    sc: &PciSoftc,
    tag: Pcitag,
    match_: Option<fn(&PciAttachArgs) -> i32>,
    pap: Option<&mut PciAttachArgs>,
) -> i32 {
    let pc = sc.pc();
    let mut ret = 0;

    let (bus, device, function) = pci_decompose_tag(pc, tag);

    let bhlcr = pci_conf_read(pc, tag, PCI_BHLC_REG);
    if pci_hdrtype_type(bhlcr) > 2 {
        return 0;
    }

    let id = pci_conf_read(pc, tag, PCI_ID_REG);
    let class = pci_conf_read(pc, tag, PCI_CLASS_REG);

    // Invalid vendor ID value?
    if pci_vendor(id) == PCI_VENDOR_INVALID {
        return 0;
    }
    // XXX Not invalid, but we've done this ~forever.
    if pci_vendor(id) == 0 {
        return 0;
    }

    let (Some(iot), Some(memt), Some(dmat)) = (sc.sc_iot.get(), sc.sc_memt.get(), sc.sc_dmat.get())
    else {
        panic(format_args!("pci_probe_device: bus not attached"));
    };

    let (intrswiz, intrtag) = if sc.sc_bridgetag.get().is_none() {
        (0, tag)
    } else {
        (sc.sc_intrswiz.get() + device as u32, sc.sc_intrtag.get())
    };

    let intr = pci_conf_read(pc, tag, PCI_INTERRUPT_REG);
    let pin = pci_interrupt_pin(intr);
    let intrpin = if pin == PCI_INTERRUPT_PIN_NONE {
        // no interrupt
        0
    } else {
        // swizzle it based on the number of busses we're behind and our device number.
        ((pin + intrswiz - 1) % 4) + 1 // XXX
    };

    let mut pa = PciAttachArgs {
        pa_iot: iot,
        pa_memt: memt,
        pa_dmat: dmat,
        pa_pc: pc,
        // This is a simplification of the NetBSD code. We don't support turning off I/O or
        // memory on broken hardware. <csapuntz@stanford.edu>
        pa_flags: sc.sc_flags.get() | PCI_FLAGS_IO_ENABLED | PCI_FLAGS_MEM_ENABLED,
        pa_ioex: sc.sc_ioex.get(),
        pa_memex: sc.sc_memex.get(),
        pa_pmemex: sc.sc_pmemex.get(),
        pa_busex: sc.sc_busex.get(),
        pa_domain: sc.sc_domain.get() as u32,
        pa_bus: bus as u32,
        pa_device: device as u32,
        pa_function: function as u32,
        pa_tag: tag,
        pa_id: id,
        pa_class: class,
        pa_bridgetag: sc.sc_bridgetag.get(),
        pa_bridgeih: sc.sc_bridgeih.get(),
        pa_intrswiz: intrswiz,
        pa_intrtag: intrtag,
        pa_intrpin: intrpin as PciIntrPin,
        pa_intrline: pci_interrupt_line(intr) as PciIntrLine,
        pa_rawintrpin: pin as PciIntrPin,
    };

    if let Some((off, cap)) = pci_get_ht_capability(pc, tag, PCI_HT_CAP_MSI) {
        // XXX Should we enable MSI mapping ourselves on systems that have it disabled?
        if cap & PCI_HT_MSI_ENABLED != 0 {
            let addr = if cap & PCI_HT_MSI_FIXED == 0 {
                u64::from(pci_conf_read(pc, tag, off + PCI_HT_MSI_ADDR))
                    | (u64::from(pci_conf_read(pc, tag, off + PCI_HT_MSI_ADDR_HI32)) << 32)
            } else {
                PCI_HT_MSI_FIXED_ADDR
            };

            // XXX This will fail to enable MSI on systems that don't use the canonical
            // address.
            if addr == PCI_HT_MSI_FIXED_ADDR {
                pa.pa_flags |= PCI_FLAGS_MSI_ENABLED;
            }
        }
    }

    // Give the MD code a chance to alter pci_attach_args and/or skip devices.
    if pci_probe_device_hook(pc, &mut pa) != 0 {
        return 0;
    }

    if let Some(match_) = match_ {
        ret = match_(&pa);
        if ret != 0
            && let Some(pap) = pap
        {
            *pap = pa;
        }
    } else {
        let Some(pdp) = malloc(size_of::<PciDev>(), M_DEVBUF, M_ZERO | M_WAITOK) else {
            panic(format_args!("pci_probe_device: out of memory"));
        };
        // SAFETY: a zeroed allocation of a `PciDev`, whose members are all valid as zero;
        // the bus keeps it on `sc_devs` until `pci_detach_devices` frees it.
        let pd: &'static PciDev = unsafe { &*pdp.as_ptr().cast::<PciDev>() };
        pd.pd_tag.set(tag);
        // SAFETY: a fresh element, on no list; the bus's list is touched by
        // autoconfiguration only.
        unsafe { sc.sc_devs.insert_head(pd) };

        let (reg_start, reg_end) = match pci_hdrtype_type(bhlcr) {
            0 => (PCI_MAPREG_START, PCI_MAPREG_END),
            // PCI-PCI bridge
            1 => (PCI_MAPREG_START, PCI_MAPREG_PPB_END),
            // PCI-CardBus bridge
            2 => (PCI_MAPREG_START, PCI_MAPREG_PCB_END),
            _ => return 0,
        };

        pd.pd_msix_table.set(pci_alloc_msix_table(pc, tag));

        let s = splhigh();
        let csr = pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG);
        let decode = PCI_COMMAND_IO_ENABLE | PCI_COMMAND_MEM_ENABLE;
        if csr & decode != 0 {
            pci_conf_write(pc, tag, PCI_COMMAND_STATUS_REG, csr & !decode);
        }

        for (i, reg) in (reg_start..reg_end).step_by(4).enumerate() {
            let address = pci_conf_read(pc, tag, reg);
            pci_conf_write(pc, tag, reg, 0xffff_ffff);
            pd.pd_mask[i].set(pci_conf_read(pc, tag, reg));
            pci_conf_write(pc, tag, reg, address);
        }

        if csr & decode != 0 {
            pci_conf_write(pc, tag, PCI_COMMAND_STATUS_REG, csr);
        }
        splx(s);

        if (pci_class(class) == PCI_CLASS_DISPLAY
            && pci_subclass(class) == PCI_SUBCLASS_DISPLAY_VGA)
            || (pci_class(class) == PCI_CLASS_PREHISTORIC
                && pci_subclass(class) == PCI_SUBCLASS_PREHISTORIC_VGA)
        {
            pd.pd_vga_decode.set(1);
        }

        let child = config_found_sm(
            &sc.sc_dev,
            ptr::from_mut(&mut pa).cast(),
            Some(pciprint),
            Some(pcisubmatch),
        );
        pd.pd_dev.set(child);
        if let Some(dev) = child {
            // SAFETY: a device config_found_sm just attached; it outlives this call.
            pci_dev_postattach(unsafe { dev.as_ref() }, &pa);
        }
    }

    ret
}

/// `pci_detach_devices`: detaches the bus's children and forgets its functions.
pub fn pci_detach_devices(sc: &PciSoftc, flags: i32) -> Result<(), Errno> {
    config_detach_children(&sc.sc_dev, flags)?;

    let pc = sc.pc();
    while let Some(pd) = sc.sc_devs.first() {
        // SAFETY: `pd` is on the bus's list.
        unsafe { crate::sys::queue::ListHead::<PciDevList>::remove(pd) };
        pci_free_msix_table(pc, pd.pd_tag.get(), pd.pd_msix_table.get());
        free(
            NonNull::from(pd).cast::<u8>(),
            M_DEVBUF,
            size_of::<PciDev>(),
        );
    }
    sc.sc_devs.init();

    Ok(())
}

/// `pci_get_capability`: the offset and first register of capability `capid`, if the
/// function has it.
pub fn pci_get_capability(pc: PciChipsetTag, tag: Pcitag, capid: i32) -> Option<(i32, Pcireg)> {
    let reg = pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG);
    if reg & PCI_STATUS_CAPLIST_SUPPORT == 0 {
        return None;
    }

    // Determine the Capability List Pointer register to start with.
    let reg = pci_conf_read(pc, tag, PCI_BHLC_REG);
    let ofs = match pci_hdrtype_type(reg) {
        // standard device header, PCI-PCI bridge header
        0 | 1 => PCI_CAPLISTPTR_REG,
        // PCI-CardBus bridge header
        2 => PCI_CARDBUS_CAPLISTPTR_REG,
        _ => return None,
    };

    let mut ofs = pci_caplist_ptr(pci_conf_read(pc, tag, ofs));
    while ofs != 0 {
        // Some devices, like parts of the NVIDIA C51 chipset, have a broken Capabilities
        // List. So we need to do a sanity check here.
        if ofs & 3 != 0 || ofs < 0x40 {
            return None;
        }
        let reg = pci_conf_read(pc, tag, ofs as i32);
        if pci_caplist_cap(reg) == capid as u32 {
            return Some((ofs as i32, reg));
        }
        ofs = pci_caplist_next(reg);
    }

    None
}

/// `pci_get_ht_capability`: the offset and register of HyperTransport capability `capid`.
pub fn pci_get_ht_capability(pc: PciChipsetTag, tag: Pcitag, capid: i32) -> Option<(i32, Pcireg)> {
    let (mut ofs, _) = pci_get_capability(pc, tag, PCI_CAP_HT)?;

    while ofs != 0 {
        #[cfg(feature = "diagnostic")]
        if ofs & 3 != 0 || ofs < 0x40 {
            panic(format_args!("pci_get_ht_capability"));
        }
        let reg = pci_conf_read(pc, tag, ofs);
        if pci_ht_cap(reg) == capid as u32 {
            return Some((ofs, reg));
        }
        ofs = pci_caplist_next(reg) as i32;
    }

    None
}

/// `pci_get_ext_capability`: the offset and register of PCI Express extended capability
/// `capid`.
pub fn pci_get_ext_capability(pc: PciChipsetTag, tag: Pcitag, capid: i32) -> Option<(i32, Pcireg)> {
    // Make sure this is a PCI Express device.
    pci_get_capability(pc, tag, PCI_CAP_PCIEXPRESS)?;

    // Scan PCI Express extended capabilities.
    let mut ofs = PCI_PCIE_ECAP;
    while ofs != 0 {
        #[cfg(feature = "diagnostic")]
        if ofs & 3 != 0 || ofs < PCI_PCIE_ECAP {
            panic(format_args!("pci_get_ext_capability"));
        }
        let reg = pci_conf_read(pc, tag, ofs);
        if pci_pcie_ecap_id(reg) == capid as u32 {
            return Some((ofs, reg));
        }
        ofs = pci_pcie_ecap_next(reg) as i32;
    }

    None
}

/// `pci_requester_id`: the function's bus/device/function number as a PCI Express
/// requester sees it.
pub fn pci_requester_id(pc: PciChipsetTag, tag: Pcitag) -> u16 {
    let (bus, dev, func) = pci_decompose_tag(pc, tag);
    ((bus << 8) | (dev << 3) | func) as u16
}

/// `pci_find_device`: asks every function of every PCI bus; stores the attach arguments of
/// the first that `match_` accepts in `pa`.
pub fn pci_find_device(pa: &mut PciAttachArgs, match_: fn(&PciAttachArgs) -> i32) -> i32 {
    for i in 0..PCI_CD.cd_ndevs.get() {
        if let Some(pcidev) = PCI_CD.cd_dev(i) {
            // SAFETY: an attached unit of pci_cd, alive while it is in cd_devs.
            let sc = softc(unsafe { pcidev.as_ref() });
            if pci_enumerate_bus(sc, Some(match_), Some(&mut *pa)) != 0 {
                return 1;
            }
        }
    }
    0
}

/// `pci_get_powerstate`: the function's power state (`PCI_PMCSR_STATE_D*`).
pub fn pci_get_powerstate(pc: PciChipsetTag, tag: Pcitag) -> i32 {
    if let Some((offset, _)) = pci_get_capability(pc, tag, PCI_CAP_PWRMGMT) {
        let reg = pci_conf_read(pc, tag, offset + PCI_PMCSR);
        return (reg & PCI_PMCSR_STATE_MASK) as i32;
    }
    PCI_PMCSR_STATE_D0 as i32
}

/// `pci_set_powerstate`: puts the function into `state`; returns the state it was in.
pub fn pci_set_powerstate(pc: PciChipsetTag, tag: Pcitag, state: i32) -> i32 {
    let mut ostate = state;
    let mut d3_delay = 10 * 1000;

    // Some AMD Ryzen xHCI controllers need a bit more time to wake up.
    let id = pci_conf_read(pc, tag, PCI_ID_REG);
    if pci_vendor(id) == PCI_VENDOR_AMD {
        match pci_product(id) {
            PCI_PRODUCT_AMD_17_1X_XHCI_1
            | PCI_PRODUCT_AMD_17_1X_XHCI_2
            | PCI_PRODUCT_AMD_17_6X_XHCI => d3_delay = 20 * 1000,
            _ => {}
        }
    }

    // Warn the firmware that we are going to put the device into the given state.
    pci_set_powerstate_md(pc, tag, state, 1);

    if let Some((offset, _)) = pci_get_capability(pc, tag, PCI_CAP_PWRMGMT) {
        if state == PCI_PMCSR_STATE_D3 as i32 {
            // The PCI Power Management spec says we should disable I/O and memory space as
            // well as bus mastering before we place the device into D3.
            let mut reg = pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG);
            reg &= !PCI_COMMAND_IO_ENABLE;
            reg &= !PCI_COMMAND_MEM_ENABLE;
            reg &= !PCI_COMMAND_MASTER_ENABLE;
            pci_conf_write(pc, tag, PCI_COMMAND_STATUS_REG, reg);
        }
        let reg = pci_conf_read(pc, tag, offset + PCI_PMCSR);
        if (reg & PCI_PMCSR_STATE_MASK) as i32 != state {
            ostate = (reg & PCI_PMCSR_STATE_MASK) as i32;

            pci_conf_write(
                pc,
                tag,
                offset + PCI_PMCSR,
                (reg & !PCI_PMCSR_STATE_MASK) | state as u32,
            );
            if state == PCI_PMCSR_STATE_D3 as i32 || ostate == PCI_PMCSR_STATE_D3 as i32 {
                delay(d3_delay);
            }
        }
    }

    // Warn the firmware that the device is now in the given state.
    pci_set_powerstate_md(pc, tag, state, 0);

    ostate
}

/// `pci_enumerate_bus`: generic PCI bus enumeration routine. Probes every function of every
/// device on the bus; with `match_`, stops at the first nonzero answer and returns it.
pub fn pci_enumerate_bus(
    sc: &PciSoftc,
    match_: Option<fn(&PciAttachArgs) -> i32>,
    mut pap: Option<&mut PciAttachArgs>,
) -> i32 {
    let pc = sc.pc();
    let mut maxndevs = sc.sc_maxndevs.get();

    // PCIe downstream ports and root ports should only forward configuration requests for
    // device number 0. However, not all hardware implements this correctly, and some
    // devices will respond to other device numbers making the device show up 32 times.
    // Prevent this by only scanning a single device.
    if let Some(bridgetag) = sc.sc_bridgetag.get()
        && let Some((_, cap)) = pci_get_capability(pc, *bridgetag, PCI_CAP_PCIEXPRESS)
    {
        match pci_pcie_xcap_type(cap) {
            PCI_PCIE_XCAP_TYPE_RP | PCI_PCIE_XCAP_TYPE_DOWN | PCI_PCIE_XCAP_TYPE_PCI2PCIE => {
                maxndevs = 1;
            }
            _ => {}
        }
    }

    for device in 0..maxndevs {
        let tag = pci_make_tag(pc, sc.sc_bus.get(), device, 0);

        let bhlcr = pci_conf_read(pc, tag, PCI_BHLC_REG);
        if pci_hdrtype_type(bhlcr) > 2 {
            continue;
        }

        let id = pci_conf_read(pc, tag, PCI_ID_REG);

        // Invalid vendor ID value?
        if pci_vendor(id) == PCI_VENDOR_INVALID {
            continue;
        }
        // XXX Not invalid, but we've done this ~forever.
        if pci_vendor(id) == 0 {
            continue;
        }

        let qd = pci_lookup_quirkdata(pci_vendor(id), pci_product(id));

        let nfunctions = match qd {
            Some(qd) if qd.quirks & PCI_QUIRK_MULTIFUNCTION != 0 => 8,
            Some(qd) if qd.quirks & PCI_QUIRK_MONOFUNCTION != 0 => 1,
            _ => {
                if pci_hdrtype_multifn(bhlcr) {
                    8
                } else {
                    1
                }
            }
        };

        for function in 0..nfunctions {
            let tag = pci_make_tag(pc, sc.sc_bus.get(), device, function);
            let ret = pci_probe_device(sc, tag, match_, pap.as_deref_mut());
            if match_.is_some() && ret != 0 {
                return ret;
            }
        }
    }

    0
}

/// `pci_reserve_resources`: claims in the bus's extents the address ranges the firmware
/// assigned to a function's BARs, expansion ROM and (for a bridge) windows and bus range;
/// a BAR, ROM or window that conflicts with an earlier claim is cleared. Without extents
/// (amd64 for now) it only sizes the BARs and the ROM.
pub fn pci_reserve_resources(pa: &PciAttachArgs) -> i32 {
    let pc = pa.pa_pc;
    let tag = pa.pa_tag;

    let (bus, dev, func) = pci_decompose_tag(pc, tag);

    let bhlc = pci_conf_read(pc, tag, PCI_BHLC_REG);
    let (reg_start, reg_end, reg_rom) = match pci_hdrtype_type(bhlc) {
        0 => (PCI_MAPREG_START, PCI_MAPREG_END, PCI_ROM_REG),
        // PCI-PCI bridge
        1 => (PCI_MAPREG_START, PCI_MAPREG_PPB_END, 0), // 0x38
        // PCI-CardBus bridge
        2 => (PCI_MAPREG_START, PCI_MAPREG_PCB_END, 0),
        _ => return 0,
    };

    // `extent_alloc_region(ex, base, size, EX_NOWAIT)` failed (nonzero in C).
    let taken =
        |ex: &Extent, base: u64, size: u64| extent_alloc_region(ex, base, size, EX_NOWAIT).is_err();

    let csr = pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG);
    let mut reg = reg_start;
    while reg < reg_end {
        let Some(type_) = pci_mapreg_probe(pc, tag, reg) else {
            reg += 4;
            continue;
        };

        let Ok((base, size, flags)) = pci_mapreg_info(pc, tag, reg, type_) else {
            reg += 4;
            continue;
        };

        if base == 0 {
            reg += 4;
            continue;
        }
        let (base, size) = (base as u64, size as u64);

        match type_ {
            t if t == PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_32BIT
                || t == PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_64BIT =>
            {
                let prefetchable = flags as u32 & PREFETCHABLE != 0;
                let in_pmem = prefetchable && pa.pa_pmemex.is_some_and(|ex| !taken(ex, base, size));
                // (__sparc64__'s T5 exception is not compiled.)
                if !in_pmem && pa.pa_memex.is_some_and(|ex| taken(ex, base, size)) {
                    if csr & PCI_COMMAND_MEM_ENABLE != 0 {
                        printf(format_args!(
                            "{bus}:{dev}:{func}: mem address conflict 0x{base:x}/0x{size:x}\n"
                        ));
                    }
                    pci_conf_write(pc, tag, reg, 0);
                    if type_ & PCI_MAPREG_MEM_TYPE_64BIT != 0 {
                        pci_conf_write(pc, tag, reg + 4, 0);
                    }
                }
            }
            PCI_MAPREG_TYPE_IO if pa.pa_ioex.is_some_and(|ex| taken(ex, base, size)) => {
                if csr & PCI_COMMAND_IO_ENABLE != 0 {
                    printf(format_args!(
                        "{bus}:{dev}:{func}: io address conflict 0x{base:x}/0x{size:x}\n"
                    ));
                }
                pci_conf_write(pc, tag, reg, 0);
            }
            _ => {}
        }

        if type_ & PCI_MAPREG_MEM_TYPE_64BIT != 0 {
            reg += 4;
        }
        reg += 4;
    }

    if reg_rom != 0 {
        let s = splhigh();
        let addr = pci_conf_read(pc, tag, PCI_ROM_REG);
        pci_conf_write(pc, tag, PCI_ROM_REG, !PCI_ROM_ENABLE);
        let mask = pci_conf_read(pc, tag, PCI_ROM_REG);
        pci_conf_write(pc, tag, PCI_ROM_REG, addr);
        splx(s);

        let base = u64::from(pci_rom_addr(addr));
        let size = u64::from(pci_rom_size(mask));
        if base != 0
            && size != 0
            && pa.pa_pmemex.is_some_and(|ex| taken(ex, base, size))
            && pa.pa_memex.is_some_and(|ex| taken(ex, base, size))
        {
            if addr & PCI_ROM_ENABLE != 0 {
                printf(format_args!(
                    "{bus}:{dev}:{func}: rom address conflict 0x{base:x}/0x{size:x}\n"
                ));
            }
            pci_conf_write(pc, tag, PCI_ROM_REG, 0);
        }
    }

    if pci_hdrtype_type(bhlc) != 1 {
        return 0;
    }

    let window = |base: u64, limit: u64| if limit > base { limit - base + 1 } else { 0 };

    // Figure out the I/O address range of the bridge.
    let mut blr = pci_conf_read(pc, tag, PPB_REG_IOSTATUS);
    let mut base = u64::from(blr & 0x000000f0) << 8;
    let mut limit = u64::from(blr & 0x000f000) | 0x00000fff;
    blr = pci_conf_read(pc, tag, PPB_REG_IO_HI);
    base |= u64::from(blr & 0x0000ffff) << 16;
    limit |= u64::from(blr & 0xffff0000);
    let size = window(base, limit);
    if let Some(ex) = pa.pa_ioex
        && base > 0
        && size > 0
        && taken(ex, base, size)
    {
        printf(format_args!(
            "{bus}:{dev}:{func}: bridge io address conflict 0x{base:x}/0x{size:x}\n"
        ));
        // The C rewrites PPB_REG_IOSTATUS from the PPB_REG_IO_HI value it last read.
        blr &= 0xffff0000;
        blr |= 0x000000f0;
        pci_conf_write(pc, tag, PPB_REG_IOSTATUS, blr);
    }

    // Figure out the memory mapped I/O address range of the bridge.
    let blr = pci_conf_read(pc, tag, PPB_REG_MEM);
    let base = u64::from(blr & 0x0000fff0) << 16;
    let limit = u64::from(blr & 0xfff00000) | 0x000fffff;
    let size = window(base, limit);
    if let Some(ex) = pa.pa_memex
        && base > 0
        && size > 0
        && taken(ex, base, size)
    {
        printf(format_args!(
            "{bus}:{dev}:{func}: bridge mem address conflict 0x{base:x}/0x{size:x}\n"
        ));
        pci_conf_write(pc, tag, PPB_REG_MEM, 0x0000fff0);
    }

    // Figure out the prefetchable memory address range of the bridge.
    let blr = pci_conf_read(pc, tag, PPB_REG_PREFMEM);
    let mut base = u64::from(blr & 0x0000fff0) << 16;
    let mut limit = u64::from(blr & 0xfff00000) | 0x000fffff;
    // __LP64__
    base |= u64::from(pci_conf_read(pc, pa.pa_tag, PPB_REG_PREFBASE_HI32)) << 32;
    limit |= u64::from(pci_conf_read(pc, pa.pa_tag, PPB_REG_PREFLIM_HI32)) << 32;
    let size = window(base, limit);
    let ex = if pa.pa_pmemex.is_some() && base > 0 && size > 0 {
        pa.pa_pmemex
    } else if pa.pa_memex.is_some() && base > 0 && size > 0 {
        pa.pa_memex
    } else {
        None
    };
    if let Some(ex) = ex
        && taken(ex, base, size)
    {
        printf(format_args!(
            "{bus}:{dev}:{func}: bridge mem address conflict 0x{base:x}/0x{size:x}\n"
        ));
        pci_conf_write(pc, tag, PPB_REG_PREFMEM, 0x0000fff0);
    }

    // Figure out the bus range handled by the bridge.
    let bir = pci_conf_read(pc, tag, PPB_REG_BUSINFO);
    let sec = ppb_businfo_secondary(bir);
    let sub = ppb_businfo_subordinate(bir);
    if let Some(ex) = pa.pa_busex
        && sub >= sec
        && sub > 0
        && taken(ex, u64::from(sec), u64::from(sub - sec + 1))
    {
        printf(format_args!(
            "{bus}:{dev}:{func}: bridge bus conflict {sec}-{sub}\n"
        ));
    }

    0
}

/*
 * Vital Product Data (PCI 2.2)
 */

/// `pci_vpd_read`: reads `data.len()` words of Vital Product Data from `offset`.
pub fn pci_vpd_read(
    pc: PciChipsetTag,
    tag: Pcitag,
    offset: i32,
    data: &mut [Pcireg],
) -> Result<(), Errno> {
    let count = data.len() as i32;
    if offset + count >= PCI_VPD_ADDRESS_MASK as i32 {
        return Err(Errno::EINVAL);
    }

    let (ofs, mut reg) = pci_get_capability(pc, tag, PCI_CAP_VPD).ok_or(Errno::ENXIO)?;

    let mut offset = offset;
    for word in data.iter_mut() {
        reg &= 0x0000_ffff;
        reg &= !PCI_VPD_OPFLAG;
        reg |= pci_vpd_address(offset as u32);
        pci_conf_write(pc, tag, ofs, reg);

        // PCI 2.2 does not specify how long we should poll for completion nor whether the
        // operation can fail.
        let mut j = 0;
        loop {
            if j == 20 {
                return Err(Errno::EIO);
            }
            j += 1;
            delay(4);
            reg = pci_conf_read(pc, tag, ofs);
            if reg & PCI_VPD_OPFLAG != 0 {
                break;
            }
        }
        *word = pci_conf_read(pc, tag, pci_vpd_datareg(ofs));
        offset += size_of::<Pcireg>() as i32;
    }

    Ok(())
}

/// `pci_matchbyid`: 1 if the device is one of `ids`.
pub fn pci_matchbyid(pa: &PciAttachArgs, ids: &[PciMatchid]) -> i32 {
    let found = ids.iter().any(|pm| {
        pci_vendor(pa.pa_id) == u32::from(pm.pm_vid)
            && pci_product(pa.pa_id) == u32::from(pm.pm_pid)
    });
    i32::from(found)
}

/// `pci_disable_legacy_vga`: the device (or the PCI ancestor it hangs from) no longer
/// decodes the legacy VGA ranges.
pub fn pci_disable_legacy_vga(dev: &Device) {
    // XXX Until we attach the drm drivers directly to pci.
    let mut dev = dev;
    while let Some(parent) = dev.parent()
        && !ptr::eq(parent.cfdata().cf_driver, &PCI_CD)
    {
        dev = parent;
    }

    let Some(parent) = dev.parent() else {
        return;
    };
    let pci = softc(parent);
    for pd in pci.sc_devs.iter() {
        if pd.pd_dev.get().is_some_and(|d| ptr::eq(d.as_ptr(), dev)) {
            pd.pd_vga_decode.set(0);
            break;
        }
    }
}

/// `pci_disable_vga` (`USER_PCICONF`): stops the function decoding I/O and memory.
pub fn pci_disable_vga(pc: PciChipsetTag, tag: Pcitag) {
    let mut csr = pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG);
    csr &= !(PCI_COMMAND_IO_ENABLE | PCI_COMMAND_MEM_ENABLE);
    pci_conf_write(pc, tag, PCI_COMMAND_STATUS_REG, csr);
}

/// `pci_enable_vga` (`USER_PCICONF`).
pub fn pci_enable_vga(pc: PciChipsetTag, tag: Pcitag) {
    let mut csr = pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG);
    csr |= PCI_COMMAND_IO_ENABLE | PCI_COMMAND_MEM_ENABLE;
    pci_conf_write(pc, tag, PCI_COMMAND_STATUS_REG, csr);
}

/// The PCI bus above the bridge a secondary bus hangs from
/// (`sc->sc_dev.dv_parent->dv_parent`).
fn grandparent_bus(sc: &PciSoftc) -> Option<&PciSoftc> {
    sc.sc_dev.parent()?.parent().map(softc)
}

/// `pci_route_vga` (`USER_PCICONF`): routes the VGA ranges through every bridge above
/// `sc`.
pub fn pci_route_vga(sc: &PciSoftc) {
    let Some(bridgetag) = sc.sc_bridgetag.get() else {
        return;
    };
    let pc = sc.pc();

    let mut bc = pci_conf_read(pc, *bridgetag, PPB_REG_BRIDGECONTROL);
    bc |= PPB_BC_VGA_ENABLE;
    pci_conf_write(pc, *bridgetag, PPB_REG_BRIDGECONTROL, bc);

    if let Some(up) = grandparent_bus(sc) {
        pci_route_vga(up);
    }
}

/// `pci_unroute_vga` (`USER_PCICONF`).
pub fn pci_unroute_vga(sc: &PciSoftc) {
    let Some(bridgetag) = sc.sc_bridgetag.get() else {
        return;
    };
    let pc = sc.pc();

    let mut bc = pci_conf_read(pc, *bridgetag, PPB_REG_BRIDGECONTROL);
    bc &= !PPB_BC_VGA_ENABLE;
    pci_conf_write(pc, *bridgetag, PPB_REG_BRIDGECONTROL, bc);

    if let Some(up) = grandparent_bus(sc) {
        pci_unroute_vga(up);
    }
}

/// `pci_primary_vga`: 1 for the VGA device that decodes I/O and memory (the one the
/// console uses), whose tag it records.
pub fn pci_primary_vga(pa: &PciAttachArgs) -> i32 {
    // XXX For now, only handle the first PCI domain.
    if pa.pa_domain != 0 {
        return 0;
    }

    if (pci_class(pa.pa_class) != PCI_CLASS_DISPLAY
        || pci_subclass(pa.pa_class) != PCI_SUBCLASS_DISPLAY_VGA)
        && (pci_class(pa.pa_class) != PCI_CLASS_PREHISTORIC
            || pci_subclass(pa.pa_class) != PCI_SUBCLASS_PREHISTORIC_VGA)
    {
        return 0;
    }

    let decode = PCI_COMMAND_IO_ENABLE | PCI_COMMAND_MEM_ENABLE;
    if pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_COMMAND_STATUS_REG) & decode != decode {
        return 0;
    }

    // SAFETY: written by autoconfiguration only, under the kernel lock; nothing holds a
    // reference.
    unsafe { PCI_VGA_TAG.write(Some(pa.pa_tag)) };

    1
}

/// `pci_alloc_msix_table` (`__HAVE_PCI_MSIX`): room to save the function's MSI-X table, or
/// null when it has none.
pub fn pci_alloc_msix_table(pc: PciChipsetTag, tag: Pcitag) -> *mut MsixVector {
    let Some((_, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        return ptr::null_mut();
    };

    let tblsz = pci_msix_mc_tblsz(reg) as usize + 1;
    let Some(p) = mallocarray(tblsz, size_of::<MsixVector>(), M_DEVBUF, M_WAITOK) else {
        panic(format_args!("pci_alloc_msix_table: out of memory"));
    };
    let table = p.as_ptr().cast::<MsixVector>();
    for i in 0..tblsz {
        // SAFETY: `tblsz` entries were just allocated.
        unsafe { table.add(i).write(MsixVector::default()) };
    }
    table
}

/// `pci_free_msix_table` (`__HAVE_PCI_MSIX`).
pub fn pci_free_msix_table(pc: PciChipsetTag, tag: Pcitag, table: *mut MsixVector) {
    let Some((_, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        return;
    };
    let Some(table) = NonNull::new(table) else {
        return;
    };

    let tblsz = pci_msix_mc_tblsz(reg) as usize + 1;
    free(
        table.cast::<u8>(),
        M_DEVBUF,
        tblsz * size_of::<MsixVector>(),
    );
}

/// `pci_suspend_msix` (`__HAVE_PCI_MSIX`): saves the MSI-X table and control register.
pub fn pci_suspend_msix(
    pc: PciChipsetTag,
    tag: Pcitag,
    memt: BusSpaceTag,
    mc: &mut Pcireg,
    pd: &PciDev,
) {
    let Some((_, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        return;
    };

    let tblsz = pci_msix_mc_tblsz(reg) as usize + 1;
    let Some(table) = msix_table(pd, tblsz) else {
        panic(format_args!("pci_suspend_msix: no table"));
    };

    let Ok(memh) = pci_msix_table_map(pc, tag, memt) else {
        return;
    };

    for (i, v) in table.iter_mut().enumerate() {
        v.mv_ma = bus_space_read_4(memt, memh, pci_msix_ma(i));
        v.mv_mau32 = bus_space_read_4(memt, memh, pci_msix_mau32(i));
        v.mv_md = bus_space_read_4(memt, memh, pci_msix_md(i));
        v.mv_vc = bus_space_read_4(memt, memh, pci_msix_vc(i));
    }

    pci_msix_table_unmap(pc, tag, memt, memh);

    *mc = reg;
}

/// `pci_resume_msix` (`__HAVE_PCI_MSIX`): restores what `pci_suspend_msix` saved.
pub fn pci_resume_msix(pc: PciChipsetTag, tag: Pcitag, memt: BusSpaceTag, mc: Pcireg, pd: &PciDev) {
    let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        return;
    };

    let tblsz = pci_msix_mc_tblsz(reg) as usize + 1;
    let Some(table) = msix_table(pd, tblsz) else {
        panic(format_args!("pci_resume_msix: no table"));
    };

    let Ok(memh) = pci_msix_table_map(pc, tag, memt) else {
        return;
    };

    for (i, v) in table.iter().enumerate() {
        bus_space_write_4(memt, memh, pci_msix_ma(i), v.mv_ma);
        bus_space_write_4(memt, memh, pci_msix_mau32(i), v.mv_mau32);
        bus_space_write_4(memt, memh, pci_msix_md(i), v.mv_md);
        bus_space_barrier(memt, memh, pci_msix_ma(i), 16, BUS_SPACE_BARRIER_WRITE);
        bus_space_write_4(memt, memh, pci_msix_vc(i), v.mv_vc);
        bus_space_barrier(memt, memh, pci_msix_vc(i), 4, BUS_SPACE_BARRIER_WRITE);
    }

    pci_msix_table_unmap(pc, tag, memt, memh);

    pci_conf_write(pc, tag, off, mc);
}

/// `pci_intr_msix_count` (`__HAVE_PCI_MSIX`): how many MSI-X vectors the device has, 0
/// when MSI cannot be used.
pub fn pci_intr_msix_count(pa: &PciAttachArgs) -> i32 {
    if pa.pa_flags & PCI_FLAGS_MSI_ENABLED == 0 {
        return 0;
    }

    let Some((_, reg)) = pci_get_capability(pa.pa_pc, pa.pa_tag, PCI_CAP_MSIX) else {
        return 0;
    };

    pci_msix_mc_tblsz(reg) as i32 + 1
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of the capability walkers, the bus enumeration and the power state functions,
    // over the fake configuration space of `pci_map`'s tests.

    use std::boxed::Box;
    use std::vec::Vec;

    use super::*;
    use crate::dev::pci::pci_map::tests::{FakePci, attach_args, pc, with_fake};
    use crate::dev::pci::pcidevs::{PCI_PRODUCT_CIRRUS_CL_PD6729, PCI_VENDOR_CIRRUS};

    /// A function with a capability list: power management at 0x40, MSI at 0x50 (64-bit), MSI-X
    /// at 0x60 with an 8-entry table.
    fn with_caps() -> FakePci {
        let mut fake = FakePci::default();
        let f = fake.add(0, 2, 0, pci_id_code(0x1af4, 0x1000), 0x0200_0000);
        f.regs[1] = PCI_STATUS_CAPLIST_SUPPORT;
        f.regs[(PCI_CAPLISTPTR_REG / 4) as usize] = 0x40;
        f.regs[0x40 / 4] = 0x5000 | PCI_CAP_PWRMGMT as u32;
        f.regs[0x44 / 4] = PCI_PMCSR_STATE_D0;
        f.regs[0x50 / 4] = PCI_MSI_MC_C64 | 0x6000 | PCI_CAP_MSI as u32;
        f.regs[0x60 / 4] = (7 << PCI_MSIX_MC_TBLSZ_SHIFT) | PCI_CAP_MSIX as u32;
        fake
    }

    #[test]
    fn capabilities_are_found_by_walking_the_list() {
        with_fake(with_caps(), |shared| {
            let tag = pci_make_tag(pc(), 0, 2, 0);
            assert_eq!(
                pci_get_capability(pc(), tag, PCI_CAP_MSI).map(|(o, _)| o),
                Some(0x50)
            );
            let (off, reg) = pci_get_capability(pc(), tag, PCI_CAP_MSIX).unwrap();
            assert_eq!(off, 0x60);
            assert_eq!(pci_msix_mc_tblsz(reg), 7);
            assert!(pci_get_capability(pc(), tag, PCI_CAP_VPD).is_none());
            assert!(pci_get_ht_capability(pc(), tag, PCI_HT_CAP_MSI).is_none());
            assert!(pci_get_ext_capability(pc(), tag, 1).is_none());

            // MSI-X entries come from the capability, when MSI is allowed on the bus.
            let mut pa = attach_args(0, 2, 0);
            assert_eq!(pci_intr_msix_count(&pa), 0);
            pa.pa_flags |= PCI_FLAGS_MSI_ENABLED;
            assert_eq!(pci_intr_msix_count(&pa), 8);

            // A broken list (a pointer below 0x40) ends the walk.
            shared
                .lock()
                .unwrap()
                .funcs
                .get_mut(&(0, 2, 0))
                .unwrap()
                .regs[0x40 / 4] = 0x2000 | PCI_CAP_PWRMGMT as u32;
            assert!(pci_get_capability(pc(), tag, PCI_CAP_MSI).is_none());

            // No capability list at all.
            shared
                .lock()
                .unwrap()
                .funcs
                .get_mut(&(0, 2, 0))
                .unwrap()
                .regs[1] = 0;
            assert!(pci_get_capability(pc(), tag, PCI_CAP_PWRMGMT).is_none());
        });
    }

    #[test]
    fn power_states() {
        with_fake(with_caps(), |shared| {
            let tag = pci_make_tag(pc(), 0, 2, 0);
            shared
                .lock()
                .unwrap()
                .funcs
                .get_mut(&(0, 2, 0))
                .unwrap()
                .regs[1] |= 0x7;
            assert_eq!(pci_get_powerstate(pc(), tag), PCI_PMCSR_STATE_D0 as i32);
            assert_eq!(
                pci_set_powerstate(pc(), tag, PCI_PMCSR_STATE_D3 as i32),
                PCI_PMCSR_STATE_D0 as i32
            );
            assert_eq!(pci_get_powerstate(pc(), tag), PCI_PMCSR_STATE_D3 as i32);
            // Going to D3 turned off decoding and bus mastering.
            let csr = shared.lock().unwrap().funcs[&(0, 2, 0)].regs[1];
            assert_eq!(csr & 0x7, 0);
        });
    }

    /// The functions `pci_enumerate_bus` probes, recorded by its match function.
    static SEEN: std::sync::Mutex<Vec<(u32, u32)>> = std::sync::Mutex::new(Vec::new());

    fn record(pa: &PciAttachArgs) -> i32 {
        SEEN.lock().unwrap().push((pa.pa_device, pa.pa_function));
        0
    }

    fn first(_pa: &PciAttachArgs) -> i32 {
        1
    }

    fn find_storage(pa: &PciAttachArgs) -> i32 {
        i32::from(pci_class(pa.pa_class) == PCI_CLASS_MASS_STORAGE)
    }

    /// A bus PciSoftc for the tests (zeroed, then given its tags), leaked.
    fn test_bus() -> &'static PciSoftc {
        // SAFETY: all-zero is a valid PciSoftc (its Softc contract).
        let sc: &'static PciSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed::<PciSoftc>() }));
        sc.sc_devs.init();
        sc.sc_iot.set(Some(Default::default()));
        sc.sc_memt.set(Some(Default::default()));
        sc.sc_dmat.set(Some(Default::default()));
        sc.sc_pc.set(Some(pc()));
        sc.sc_maxndevs.set(32);
        sc
    }

    #[test]
    fn enumeration_follows_headers_and_quirks() {
        let mut fake = FakePci::default();
        // dev 0: a single-function host bridge.
        fake.add(0, 0, 0, pci_id_code(0x8086, 0x29c0), 0x0600_0000);
        // dev 1: multi-function, functions 0 and 2 present.
        fake.add(0, 1, 0, pci_id_code(0x8086, 0x2918), 0x0601_0000)
            .hdrtype(0x80);
        fake.add(0, 1, 2, pci_id_code(0x8086, 0x2922), 0x0106_0100);
        // dev 4: says multi-function but is a MONOFUNCTION quirk; function 1 must not be seen.
        fake.add(
            0,
            4,
            0,
            pci_id_code(PCI_VENDOR_CIRRUS, PCI_PRODUCT_CIRRUS_CL_PD6729),
            0x0605_0000,
        )
        .hdrtype(0x80);
        fake.add(0, 4, 1, pci_id_code(0x1234, 0x5678), 0);
        // dev 5: vendor 0, skipped.
        fake.add(0, 5, 0, 0, 0);
        // dev 6: header type 3, skipped.
        fake.add(0, 6, 0, pci_id_code(0x1234, 0x1), 0).hdrtype(3);

        with_fake(fake, |_| {
            let sc = test_bus();
            SEEN.lock().unwrap().clear();
            assert_eq!(pci_enumerate_bus(sc, Some(record), None), 0);
            assert_eq!(SEEN.lock().unwrap()[..], [(0, 0), (1, 0), (1, 2), (4, 0)]);

            // A match function that answers stops the walk and fills the arguments.
            let mut pa = attach_args(0, 0, 0);
            assert_eq!(pci_enumerate_bus(sc, Some(find_storage), Some(&mut pa)), 1);
            assert_eq!((pa.pa_device, pa.pa_function), (1, 2));
            assert_eq!(pci_subclass(pa.pa_class), PCI_SUBCLASS_MASS_STORAGE_SATA);
            assert_eq!(pa.pa_flags & PCI_FLAGS_MEM_ENABLED, PCI_FLAGS_MEM_ENABLED);

            assert_eq!(pci_requester_id(pc(), pa.pa_tag), (1 << 3) | 2);
            let ids = [PciMatchid {
                pm_vid: 0x8086,
                pm_pid: 0x2922,
            }];
            assert_eq!(pci_matchbyid(&pa, &ids), 1);
            assert_eq!(pci_matchbyid(&attach_args(0, 0, 0), &ids), 0);
        });
    }

    #[test]
    fn interrupt_pins_are_swizzled_behind_a_bridge() {
        let mut fake = FakePci::default();
        fake.add(1, 3, 0, pci_id_code(0x1af4, 0x1000), 0).regs[(PCI_INTERRUPT_REG / 4) as usize] =
            (PCI_INTERRUPT_PIN_A << PCI_INTERRUPT_PIN_SHIFT) | 11;
        with_fake(fake, |_| {
            let sc = test_bus();
            sc.sc_bus.set(1);
            static BRIDGE: HostTag = HostTag::new();
            sc.sc_bridgetag.set(Some(BRIDGE.get()));
            sc.sc_intrswiz.set(1);
            let mut pa = attach_args(0, 0, 0);
            assert_eq!(pci_enumerate_bus(sc, Some(first), Some(&mut pa)), 1);
            // Pin A of device 3 behind a bridge with swizzle 1: ((1 + 4 - 1) % 4) + 1 = pin A.
            assert_eq!(pa.pa_rawintrpin, PCI_INTERRUPT_PIN_A as u8);
            assert_eq!(pa.pa_intrswiz, 4);
            assert_eq!(pa.pa_intrpin, 1);
            assert_eq!(pa.pa_intrline, 11);
        });
    }

    /// A `'static` pcitag for a bridge.
    struct HostTag(std::sync::OnceLock<Pcitag>);

    impl HostTag {
        const fn new() -> Self {
            Self(std::sync::OnceLock::new())
        }

        fn get(&'static self) -> &'static Pcitag {
            self.0.get_or_init(|| pci_make_tag(pc(), 0, 1, 0))
        }
    }
}
/* </TESTS> */
