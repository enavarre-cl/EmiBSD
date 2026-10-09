/* <LICENSES> */
/*
 * Copyright (c) 1996 Christopher G. Demetriou.  All rights reserved.
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
//! `ppb*`: PCI-PCI bridges (`ppb* at pci?`, `pci* at ppb?`), see `ppb(4)`.
//!
//! Upstream: sys/dev/pci/ppb.c @ 3ce1f3f79392
//!
//! A bridge is a function of class bridge, subclass PCI, header type 1. The attachment reads
//! the secondary and subordinate bus numbers the firmware gave it (or allocates them from the
//! parent's bus extent when it has none), sets up PCI Express native hot plug when the port
//! has a slot (an interrupt on presence-detect changes that rescans or detaches the bus behind
//! it), maps the four INTx pins, so the devices behind it can be routed through the bridge
//! (`pba_bridgeih`), describes the bridge's I/O, memory and prefetchable windows as extents and
//! attaches the `pci*` bus behind it. Suspend saves and resume restores the bridge's
//! configuration registers.
//!
//! ## Deviations
//! - The extents come down from the host bridge as in C (`pa_*ex`, `pba_*ex`, M16b): on
//!   amd64 mainbus and acpipci still hand down none (`pci_init_extents` is not ported), so
//!   there `ppb_alloc_busrange` is never called (a bridge the firmware left without bus
//!   numbers prints `not configured by system firmware`, as the C does without a bus extent)
//!   and `ppb_alloc_resources` returns at its `pa_memex == NULL` test; arm64's acpipci and
//!   pciecam give theirs. The bridge's softc holds the names of its extents (the C `malloc`s
//!   `PPB_EXNAMLEN` bytes).
//! - `sc_ih` holds `Option`s: `None` is a pin `pci_intr_map` could not map, which the C marks
//!   inside the handle (`pcivar.rs`).
//! - `ppbattach` writes the interrupt pins into its own copy of the attach arguments; the C
//!   writes the parent's, which nothing reads after the attach.
//! - `ppb_hotplug_insert` returns when the bus behind the bridge did not attach (`sc_psc`
//!   NULL), where the C would dereference it.

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::dev::ofw::fdt::OF_getpropintarray;
use crate::dev::pci::pci::{
    PCI_DOPM, pci_detach_devices, pci_enumerate_bus, pci_get_capability, pci_get_powerstate,
    pci_set_powerstate,
};
use crate::dev::pci::pci_map::pci_mapreg_probe;
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_INTEL_82801BA_HPB, PCI_PRODUCT_INTEL_82801BAM_HPB, PCI_PRODUCT_SUN_SIMBA,
    PCI_PRODUCT_VIATECH_VT82C586_PWR, PCI_VENDOR_INTEL, PCI_VENDOR_INVALID, PCI_VENDOR_SUN,
    PCI_VENDOR_VIATECH,
};
use crate::dev::pci::pcireg::{
    PCI_BHLC_REG, PCI_CAP_MSI, PCI_CAP_PCIEXPRESS, PCI_CLASS_BRIDGE, PCI_COMMAND_IO_ENABLE,
    PCI_COMMAND_MASTER_ENABLE, PCI_COMMAND_MEM_ENABLE, PCI_COMMAND_STATUS_REG, PCI_ID_REG,
    PCI_INTERRUPT_PIN_A, PCI_INTERRUPT_PIN_D, PCI_INTERRUPT_REG, PCI_MAPREG_END,
    PCI_MAPREG_MEM_TYPE_64BIT, PCI_MAPREG_PCB_END, PCI_MAPREG_PPB_END, PCI_MAPREG_START,
    PCI_MAPREG_TYPE_IO, PCI_MAPREG_TYPE_MEM, PCI_MSI_MA, PCI_MSI_MAU32, PCI_MSI_MC, PCI_MSI_MC_C64,
    PCI_MSI_MD32, PCI_MSI_MD64, PCI_PCIE_SLCSR, PCI_PCIE_SLCSR_HPE, PCI_PCIE_SLCSR_PDC,
    PCI_PCIE_SLCSR_PDE, PCI_PCIE_SLCSR_PDS, PCI_PCIE_XCAP_SI, PCI_ROM_ENABLE, PCI_ROM_REG,
    PCI_SUBCLASS_BRIDGE_PCI, PciIntrPin, pci_class, pci_hdrtype_type, pci_interface, pci_product,
    pci_rom_size, pci_subclass, pci_vendor,
};
use crate::dev::pci::pcivar::{
    PCI_FLAGS_MRM_OKAY, PciAttachArgs, PciSoftc, PcibusAttachArgs, Pcireg,
};
use crate::dev::pci::ppbreg::{
    PPB_INTERFACE_SUBTRACTIVE, PPB_IO_MASK, PPB_IO_MIN, PPB_IO_SHIFT, PPB_MEM_MASK, PPB_MEM_MIN,
    PPB_MEM_SHIFT, PPB_REG_BRIDGECONTROL, PPB_REG_BUSINFO, PPB_REG_IO_HI, PPB_REG_IOSTATUS,
    PPB_REG_MEM, PPB_REG_PREFBASE_HI32, PPB_REG_PREFLIM_HI32, PPB_REG_PREFMEM,
    ppb_businfo_secondary, ppb_businfo_subordinate,
};
use crate::kern::kern_task::{SYSTQ, task_add, task_set};
use crate::kern::kern_timeout::{timeout_add_sec, timeout_set};
use crate::kern::subr_autoconf::{config_activate_children, config_detach_children, config_found};
use crate::kern::subr_extent::{
    extent_alloc, extent_alloc_region, extent_alloc_subregion, extent_create, extent_destroy,
    extent_free,
};
use crate::kern::subr_prf::{Str, printf, snprintf};
use crate::machine::intr::IPL_BIO;
use crate::machine::pci_machdep::{
    PCI_IO_END, PCI_IO_START, PCI_MEM_END, PCI_MEM_START, PciChipsetTag, PciIntrHandle, Pcitag,
    pci_bus_maxdevs, pci_conf_read, pci_conf_write, pci_intr_disestablish, pci_intr_establish,
    pci_intr_map, pci_intr_map_msi, pci_intr_string, pci_make_tag, pci_min_powerstate, pcitag_node,
};
use crate::sys::device::{
    CD_COCOVM, CfMatch, Cfattach, Cfdriver, DETACH_FORCE, DV_DULL, DVACT_POWERDOWN, DVACT_RESUME,
    DVACT_SUSPEND, Device, Softc, UNCONF,
};
use crate::sys::errno::Errno;
use crate::sys::extent::{EX_CONFLICTOK, EX_FILLED, EX_NOWAIT, Extent};
use crate::sys::malloc::M_DEVBUF;
use crate::sys::systm::COLD;
use crate::sys::task::Task;
use crate::sys::timeout::Timeout;

/// `PPB_EXNAMLEN`: the size of an extent's name.
const PPB_EXNAMLEN: usize = 32;

/// `struct ppb_softc`.
#[repr(C)]
pub struct PpbSoftc {
    /// `sc_dev`: generic device glue.
    pub sc_dev: Device,
    /// `sc_pc`: our PCI chipset...
    pub sc_pc: Cell<Option<PciChipsetTag>>,
    /// `sc_tag`: ...and tag; written once, first thing in the attach, then lent to the bus
    /// behind the bridge (`pba_bridgetag`).
    pub sc_tag: UnsafeCell<Pcitag>,
    /// `sc_ih[4]`: the INTx pins' handles, written once in the attach before the bus behind
    /// the bridge (which borrows them, `pba_bridgeih`) exists.
    pub sc_ih: UnsafeCell<[Option<PciIntrHandle>; 4]>,
    /// `sc_intrhand`: the hot-plug interrupt.
    pub sc_intrhand: Cell<Option<NonNull<c_void>>>,
    /// `sc_parent_busex`.
    pub sc_parent_busex: Cell<Option<&'static Extent>>,
    /// `sc_busex`.
    pub sc_busex: Cell<Option<&'static Extent>>,
    /// `sc_ioex`.
    pub sc_ioex: Cell<Option<&'static Extent>>,
    /// `sc_memex`.
    pub sc_memex: Cell<Option<&'static Extent>>,
    /// `sc_pmemex`.
    pub sc_pmemex: Cell<Option<&'static Extent>>,
    /// `sc_psc`: the `pci*` behind the bridge.
    pub sc_psc: Cell<Option<NonNull<Device>>>,
    /// `sc_cap_off`: the PCI Express capability, when it has a slot.
    pub sc_cap_off: Cell<i32>,
    /// `sc_insert_task`.
    pub sc_insert_task: Task,
    /// `sc_rescan_task`.
    pub sc_rescan_task: Task,
    /// `sc_remove_task`.
    pub sc_remove_task: Task,
    /// `sc_to`.
    pub sc_to: Timeout,
    /// `sc_busnum`.
    pub sc_busnum: Cell<u64>,
    /// `sc_busrange`.
    pub sc_busrange: Cell<u64>,
    /// `sc_iobase`.
    pub sc_iobase: Cell<u64>,
    /// `sc_iolimit`.
    pub sc_iolimit: Cell<u64>,
    /// `sc_membase`.
    pub sc_membase: Cell<u64>,
    /// `sc_memlimit`.
    pub sc_memlimit: Cell<u64>,
    /// `sc_pmembase`.
    pub sc_pmembase: Cell<u64>,
    /// `sc_pmemlimit`.
    pub sc_pmemlimit: Cell<u64>,
    /// `sc_csr`.
    pub sc_csr: Cell<Pcireg>,
    /// `sc_bhlcr`.
    pub sc_bhlcr: Cell<Pcireg>,
    /// `sc_bir`.
    pub sc_bir: Cell<Pcireg>,
    /// `sc_bcr`.
    pub sc_bcr: Cell<Pcireg>,
    /// `sc_int`.
    pub sc_int: Cell<Pcireg>,
    /// `sc_slcsr`.
    pub sc_slcsr: Cell<Pcireg>,
    /// `sc_msi_mc`.
    pub sc_msi_mc: Cell<Pcireg>,
    /// `sc_msi_ma`.
    pub sc_msi_ma: Cell<Pcireg>,
    /// `sc_msi_mau32`.
    pub sc_msi_mau32: Cell<Pcireg>,
    /// `sc_msi_md`.
    pub sc_msi_md: Cell<Pcireg>,
    /// `sc_pmcsr_state`.
    pub sc_pmcsr_state: Cell<i32>,
    /// The name of `sc_busex` (the C `malloc`s it), written once by the attach.
    sc_busex_name: UnsafeCell<[u8; PPB_EXNAMLEN]>,
    /// The name of `sc_ioex`.
    sc_ioex_name: UnsafeCell<[u8; PPB_EXNAMLEN]>,
    /// The name of `sc_memex`.
    sc_memex_name: UnsafeCell<[u8; PPB_EXNAMLEN]>,
    /// The name of `sc_pmemex`.
    sc_pmemex_name: UnsafeCell<[u8; PPB_EXNAMLEN]>,
}

impl PpbSoftc {
    /// `sc->sc_pc`, set first thing in the attach.
    fn pc(&self) -> Option<PciChipsetTag> {
        self.sc_pc.get()
    }

    /// `sc->sc_tag`.
    fn tag(&self) -> Pcitag {
        // SAFETY: written once at the start of the attach, before any reader exists.
        unsafe { *self.sc_tag.get() }
    }

    /// One of the extent names, formatted once into its buffer:
    /// `snprintf(name, PPB_EXNAMLEN, "%s pcibus", sc->sc_dev.dv_xname)`.
    fn exname(
        cell: &'static UnsafeCell<[u8; PPB_EXNAMLEN]>,
        what: &str,
        xname: &str,
    ) -> &'static [u8] {
        // SAFETY: the attach formats each name once, before the extent that keeps it exists;
        // nothing writes it afterwards.
        let buf = unsafe { &mut *cell.get() };
        let n = snprintf(buf, format_args!("{xname} {what}")).min(buf.len() - 1);
        &buf[..n]
    }

    /// The softc behind a task's, a timeout's or an interrupt's argument.
    ///
    /// # Safety
    ///
    /// `arg` is the `PpbSoftc` `ppbattach` registered it with, which lives as long as the
    /// device (the handlers are torn down by `ppbdetach` first).
    unsafe fn from_arg(arg: *mut c_void) -> &'static PpbSoftc {
        // SAFETY: the caller's contract.
        unsafe { &*arg.cast::<PpbSoftc>() }
    }

    /// The `pci*` behind the bridge, when it attached.
    fn psc(&self) -> Option<&PciSoftc> {
        // SAFETY: `sc_psc` is the device `config_found` attached for `pba`, a `pci*`, whose
        // softc is a `PciSoftc`; it lives until `ppbdetach` detaches it.
        self.sc_psc
            .get()
            .map(|d| unsafe { d.as_ref().softc::<PciSoftc>() })
    }
}

// SAFETY: `#[repr(C)]` with the device first; the tasks and the timeout are all-zero valid
// (`sys/task.rs`, `sys/timeout.rs`), the name buffers are bytes, the pcitag is an integer (or
// a struct of integers), and every other member is a `Cell` of an integer, `None` or an array
// of `None` handles: all valid as zero bits.
unsafe impl Softc for PpbSoftc {}

/// The parent bus's extents, which the C reads from the attach arguments (`pa_busex`,
/// `pa_ioex`, `pa_memex`).
#[derive(Clone, Copy, Default)]
pub struct ParentExtents {
    /// `pa_busex`.
    pub busex: Option<&'static Extent>,
    /// `pa_ioex`.
    pub ioex: Option<&'static Extent>,
    /// `pa_memex`.
    pub memex: Option<&'static Extent>,
}

/// `ppb_ca`.
pub static PPB_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PpbSoftc>(),
    ca_match: Some(ppbmatch),
    ca_attach: ppbattach,
    ca_detach: Some(ppbdetach),
    ca_activate: Some(ppbactivate),
};

/// `ppb_cd`.
pub static PPB_CD: Cfdriver = Cfdriver::new(b"ppb", DV_DULL, CD_COCOVM);

/// `ppbmatch`: a PCI-PCI bridge, but for the VT82C586's power management function, which says
/// it is one.
pub fn ppbmatch(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    // This device is mislabeled. It is not a PCI bridge.
    if pci_vendor(pa.pa_id) == PCI_VENDOR_VIATECH
        && pci_product(pa.pa_id) == PCI_PRODUCT_VIATECH_VT82C586_PWR
    {
        return 0;
    }
    // Check the ID register to see that it's a PCI bridge. If it is, we assume that we can
    // deal with it; it _should_ work in a standardized way...
    if pci_class(pa.pa_class) == PCI_CLASS_BRIDGE
        && pci_subclass(pa.pa_class) == PCI_SUBCLASS_BRIDGE_PCI
    {
        return 1;
    }

    0
}

/// Makes one of the bridge's extents over `[start, end]`, all of it allocated but the window
/// `[base, base + size)`, named into `name`: the C's `extent_create(.., EX_FILLED)` and
/// `extent_free` pair (nothing when the name cannot be had; here it always can).
fn ppb_window_extent(
    name: &'static [u8],
    end: u64,
    base: u64,
    size: u64,
) -> Option<&'static Extent> {
    let ex = extent_create(name, 0, end, M_DEVBUF, None, EX_NOWAIT | EX_FILLED)?;
    let _ = extent_free(ex, base, size, EX_NOWAIT);
    Some(ex)
}

/// `ppbattach`.
pub fn ppbattach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `ppb_ca`, whose softc is a `PpbSoftc`; softcs are never
    // freed while the device exists, so the softc may be borrowed for 'static.
    let sc: &'static PpbSoftc = unsafe { &*ptr::from_ref(self_.softc::<PpbSoftc>()) };
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach; ppbattach changes the pins in a copy (see the deviations).
    let mut pa: PciAttachArgs = unsafe { *aux.cast::<PciAttachArgs>() };
    let pc = pa.pa_pc;
    let xname = sc.sc_dev.xname();
    let parent_ex = ParentExtents {
        busex: pa.pa_busex,
        ioex: pa.pa_ioex,
        memex: pa.pa_memex,
    };

    sc.sc_pc.set(Some(pc));
    // SAFETY: the attach's first write; nothing has borrowed the tag yet.
    unsafe { *sc.sc_tag.get() = pa.pa_tag };

    let mut busdata = pci_conf_read(pc, pa.pa_tag, PPB_REG_BUSINFO);

    // When the bus number isn't configured, try to allocate one ourselves.
    if busdata == 0
        && let Some(busex) = parent_ex.busex
    {
        ppb_alloc_busrange(sc, &pa, busex, &mut busdata);
    }

    // When the bus number still isn't set correctly, give up.
    if ppb_businfo_secondary(busdata) == 0 {
        printf(format_args!(": not configured by system firmware\n"));
        return;
    }

    // (The C's `#if 0` sanity check of the primary bus number against the tag stays out: the
    // tag cannot be decomposed portably.)

    let sec = u64::from(ppb_businfo_secondary(busdata));
    let sub = u64::from(ppb_businfo_subordinate(busdata));
    if sub > sec {
        let name = PpbSoftc::exname(&sc.sc_busex_name, "pcibus", xname);
        sc.sc_busex
            .set(ppb_window_extent(name, 0xff, sec + 1, sub - sec));
    }

    sc.sc_parent_busex.set(parent_ex.busex);
    sc.sc_busnum.set(sec);
    sc.sc_busrange.set((sub + 1).wrapping_sub(sec));

    // Check for PCI Express capabilities and setup hotplug support.
    // (`sc_cap_off` is set whenever the capability exists, slot or not, as the C's
    // `pci_get_capability(.., &sc->sc_cap_off, ..)` does; ppbactivate saves SLCSR by it.)
    let pcie = pci_get_capability(pc, pa.pa_tag, PCI_CAP_PCIEXPRESS);
    if let Some((off, _)) = pcie {
        sc.sc_cap_off.set(off);
    }
    if let Some((off, reg)) = pcie
        && reg & PCI_PCIE_XCAP_SI != 0
    {
        let arg = ptr::from_ref(sc).cast_mut().cast::<c_void>();
        task_set(&sc.sc_insert_task, ppb_hotplug_insert, arg);
        task_set(&sc.sc_rescan_task, ppb_hotplug_rescan, arg);
        task_set(&sc.sc_remove_task, ppb_hotplug_remove, arg);
        timeout_set(&sc.sc_to, ppb_hotplug_insert_finish, arg);

        // (`__i386__` maps INTx only; every other machine tries MSI first.)
        if let Some(ih) = pci_intr_map_msi(&pa).or_else(|| pci_intr_map(&pa)) {
            sc.sc_intrhand
                .set(pci_intr_establish(pc, ih, IPL_BIO, ppb_intr, arg, xname));
            if sc.sc_intrhand.get().is_some() {
                printf(format_args!(": {}", pci_intr_string(pc, ih)));

                // Enable hotplug interrupt.
                let reg = pci_conf_read(pc, pa.pa_tag, off + PCI_PCIE_SLCSR)
                    | PCI_PCIE_SLCSR_HPE
                    | PCI_PCIE_SLCSR_PDE;
                pci_conf_write(pc, pa.pa_tag, off + PCI_PCIE_SLCSR, reg);
            }
        }
    }

    printf(format_args!("\n"));

    let mut interface = pci_interface(pa.pa_class);

    // The Intel 82801BAM Hub-to-PCI can decode subtractively but doesn't advertise itself as
    // such.
    if pci_vendor(pa.pa_id) == PCI_VENDOR_INTEL
        && (pci_product(pa.pa_id) == PCI_PRODUCT_INTEL_82801BA_HPB
            || pci_product(pa.pa_id) == PCI_PRODUCT_INTEL_82801BAM_HPB)
    {
        interface = PPB_INTERFACE_SUBTRACTIVE;
    }

    if interface != PPB_INTERFACE_SUBTRACTIVE {
        ppb_alloc_resources(sc, parent_ex);
    }

    let mut ih: [Option<PciIntrHandle>; 4] = [None; 4];
    for pin in PCI_INTERRUPT_PIN_A..=PCI_INTERRUPT_PIN_D {
        pa.pa_intrpin = pin as PciIntrPin;
        pa.pa_rawintrpin = pin as PciIntrPin;
        pa.pa_intrline = 0;
        ih[(pin - PCI_INTERRUPT_PIN_A) as usize] = pci_intr_map(&pa);
    }
    // SAFETY: written once, before the bus behind the bridge (the one reader) exists.
    unsafe { *sc.sc_ih.get() = ih };

    // The UltraSPARC-IIi APB doesn't implement the standard address range registers.
    if !(pci_vendor(pa.pa_id) == PCI_VENDOR_SUN && pci_product(pa.pa_id) == PCI_PRODUCT_SUN_SIMBA) {
        ppb_windows(sc, &pa, interface);
    }

    // attach: Attach the PCI bus that hangs off of it.
    //
    // XXX Don't pass-through Memory Read Multiple. Should we?
    // XXX Consult the spec...
    let mut pba = PcibusAttachArgs {
        pba_busname: b"pci",
        pba_iot: pa.pa_iot,
        pba_memt: pa.pa_memt,
        pba_dmat: pa.pa_dmat,
        pba_pc: pc,
        pba_flags: pa.pa_flags & !PCI_FLAGS_MRM_OKAY,
        pba_busex: sc.sc_busex.get(),
        pba_ioex: sc.sc_ioex.get(),
        pba_memex: sc.sc_memex.get(),
        pba_pmemex: sc.sc_pmemex.get(),
        pba_domain: pa.pa_domain as i32,
        pba_bus: ppb_businfo_secondary(busdata) as i32,
        // SAFETY: both were written above, once, and are only read from now on.
        pba_bridgeih: Some(unsafe { &*sc.sc_ih.get() }),
        // SAFETY: as above.
        pba_bridgetag: Some(unsafe { &*sc.sc_tag.get() }),
        pba_intrswiz: pa.pa_intrswiz,
        pba_intrtag: pa.pa_intrtag,
    };

    sc.sc_psc.set(config_found(
        &sc.sc_dev,
        ptr::from_mut(&mut pba).cast(),
        Some(ppbprint),
    ));
}

/// The I/O, memory and prefetchable windows of `ppbattach`: read from the bridge, each made
/// an extent; a subtractive bridge falls back on its parent's.
fn ppb_windows(sc: &'static PpbSoftc, pa: &PciAttachArgs, interface: u32) {
    let pc = pa.pa_pc;
    let xname = sc.sc_dev.xname();

    // Figure out the I/O address range of the bridge.
    let blr = u64::from(pci_conf_read(pc, pa.pa_tag, PPB_REG_IOSTATUS));
    let mut iobase = (blr & 0x0000_00f0) << 8;
    let mut iolimit = (blr & 0x0000_f000) | 0x0000_0fff;
    let blr = u64::from(pci_conf_read(pc, pa.pa_tag, PPB_REG_IO_HI));
    iobase |= (blr & 0x0000_ffff) << 16;
    iolimit |= blr & 0xffff_0000;
    sc.sc_iobase.set(iobase);
    sc.sc_iolimit.set(iolimit);
    if iolimit > iobase {
        let name = PpbSoftc::exname(&sc.sc_ioex_name, "pciio", xname);
        sc.sc_ioex.set(ppb_window_extent(
            name,
            0xffff_ffff,
            iobase,
            iolimit - iobase + 1,
        ));
    }

    // Figure out the memory mapped I/O address range of the bridge.
    let blr = u64::from(pci_conf_read(pc, pa.pa_tag, PPB_REG_MEM));
    let membase = (blr & 0x0000_fff0) << 16;
    let memlimit = (blr & 0xfff0_0000) | 0x000f_ffff;
    sc.sc_membase.set(membase);
    sc.sc_memlimit.set(memlimit);
    if memlimit > membase {
        let name = PpbSoftc::exname(&sc.sc_memex_name, "pcimem", xname);
        sc.sc_memex.set(ppb_window_extent(
            name,
            u64::MAX,
            membase,
            memlimit - membase + 1,
        ));
    }

    // Figure out the prefetchable MMI/O address range of the bridge.
    let blr = u64::from(pci_conf_read(pc, pa.pa_tag, PPB_REG_PREFMEM));
    let mut pmembase = (blr & 0x0000_fff0) << 16;
    let mut pmemlimit = (blr & 0xfff0_0000) | 0x000f_ffff;
    // __LP64__
    let blr = u64::from(pci_conf_read(pc, pa.pa_tag, PPB_REG_PREFBASE_HI32));
    pmembase |= blr << 32;
    let blr = u64::from(pci_conf_read(pc, pa.pa_tag, PPB_REG_PREFLIM_HI32));
    pmemlimit |= blr << 32;
    sc.sc_pmembase.set(pmembase);
    sc.sc_pmemlimit.set(pmemlimit);
    if pmemlimit > pmembase {
        let name = PpbSoftc::exname(&sc.sc_pmemex_name, "pcipmem", xname);
        sc.sc_pmemex.set(ppb_window_extent(
            name,
            u64::MAX,
            pmembase,
            pmemlimit - pmembase + 1,
        ));
    }

    if interface == PPB_INTERFACE_SUBTRACTIVE {
        if sc.sc_ioex.get().is_none() {
            sc.sc_ioex.set(pa.pa_ioex);
        }
        if sc.sc_memex.get().is_none() {
            sc.sc_memex.set(pa.pa_memex);
        }
    }
}

/// `ppbdetach`.
pub fn ppbdetach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `ppb_ca`.
    let sc = unsafe { self_.softc::<PpbSoftc>() };

    if let (Some(pc), Some(ih)) = (sc.pc(), sc.sc_intrhand.get()) {
        // SAFETY: the cookie came from `pci_intr_establish` in the attach and is dropped here.
        unsafe { pci_intr_disestablish(pc, ih) };
        sc.sc_intrhand.set(None);
    }

    let rv = config_detach_children(self_, flags);

    for ex in [&sc.sc_busex, &sc.sc_ioex, &sc.sc_memex, &sc.sc_pmemex] {
        if let Some(e) = ex.take() {
            // SAFETY: the extent came from `extent_create` in the attach; its name lives in
            // the softc and nothing uses the extent afterwards.
            unsafe { extent_destroy(e) };
        }
    }

    if let Some(busex) = sc.sc_parent_busex.get()
        && sc.sc_busrange.get() > 0
    {
        let _ = extent_free(busex, sc.sc_busnum.get(), sc.sc_busrange.get(), EX_NOWAIT);
    }

    rv
}

/// `ppbactivate`: saves the bridge's registers on suspend and restores them on resume.
pub fn ppbactivate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `ppb_ca`.
    let sc = unsafe { self_.softc::<PpbSoftc>() };
    let Some(pc) = sc.pc() else {
        return config_activate_children(self_, act);
    };
    let tag = sc.tag();

    match act {
        DVACT_SUSPEND => {
            let rv = config_activate_children(self_, act);

            // Save registers that may get lost.
            sc.sc_csr
                .set(pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG));
            sc.sc_bhlcr.set(pci_conf_read(pc, tag, PCI_BHLC_REG));
            sc.sc_bir.set(pci_conf_read(pc, tag, PPB_REG_BUSINFO));
            sc.sc_bcr.set(pci_conf_read(pc, tag, PPB_REG_BRIDGECONTROL));
            sc.sc_int.set(pci_conf_read(pc, tag, PCI_INTERRUPT_REG));
            if sc.sc_cap_off.get() != 0 {
                sc.sc_slcsr
                    .set(pci_conf_read(pc, tag, sc.sc_cap_off.get() + PCI_PCIE_SLCSR));
            }

            if let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSI) {
                sc.sc_msi_ma.set(pci_conf_read(pc, tag, off + PCI_MSI_MA));
                if reg & PCI_MSI_MC_C64 != 0 {
                    sc.sc_msi_mau32
                        .set(pci_conf_read(pc, tag, off + PCI_MSI_MAU32));
                    sc.sc_msi_md.set(pci_conf_read(pc, tag, off + PCI_MSI_MD64));
                } else {
                    sc.sc_msi_md.set(pci_conf_read(pc, tag, off + PCI_MSI_MD32));
                }
                sc.sc_msi_mc.set(reg);
            }
            rv
        }
        DVACT_RESUME => {
            if PCI_DOPM.load(Ordering::Relaxed) != 0 {
                // Restore power.
                pci_set_powerstate(pc, tag, sc.sc_pmcsr_state.get());
            }

            // Restore the registers saved above.
            pci_conf_write(pc, tag, PCI_BHLC_REG, sc.sc_bhlcr.get());
            pci_conf_write(pc, tag, PPB_REG_BUSINFO, sc.sc_bir.get());
            pci_conf_write(pc, tag, PPB_REG_BRIDGECONTROL, sc.sc_bcr.get());
            pci_conf_write(pc, tag, PCI_INTERRUPT_REG, sc.sc_int.get());
            if sc.sc_cap_off.get() != 0 {
                pci_conf_write(
                    pc,
                    tag,
                    sc.sc_cap_off.get() + PCI_PCIE_SLCSR,
                    sc.sc_slcsr.get(),
                );
            }

            // Restore I/O window.
            let (iobase, iolimit) = (sc.sc_iobase.get(), sc.sc_iolimit.get());
            let blr = pci_conf_read(pc, tag, PPB_REG_IOSTATUS);
            pci_conf_write(
                pc,
                tag,
                PPB_REG_IOSTATUS,
                ppb_iostatus(blr, iobase, iolimit),
            );
            pci_conf_write(pc, tag, PPB_REG_IO_HI, ppb_io_hi(iobase, iolimit));

            // Restore memory mapped I/O window.
            let (membase, memlimit) = (sc.sc_membase.get(), sc.sc_memlimit.get());
            pci_conf_write(pc, tag, PPB_REG_MEM, ppb_mem(membase, memlimit));

            // Restore prefetchable MMI/O window.
            let (pmembase, pmemlimit) = (sc.sc_pmembase.get(), sc.sc_pmemlimit.get());
            pci_conf_write(
                pc,
                tag,
                PPB_REG_PREFMEM,
                ppb_mem(pmembase & u64::from(PPB_MEM_MASK), pmemlimit),
            );
            // __LP64__
            pci_conf_write(pc, tag, PPB_REG_PREFBASE_HI32, (pmembase >> 32) as Pcireg);
            pci_conf_write(pc, tag, PPB_REG_PREFLIM_HI32, (pmemlimit >> 32) as Pcireg);

            if let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSI) {
                pci_conf_write(pc, tag, off + PCI_MSI_MA, sc.sc_msi_ma.get());
                if reg & PCI_MSI_MC_C64 != 0 {
                    pci_conf_write(pc, tag, off + PCI_MSI_MAU32, sc.sc_msi_mau32.get());
                    pci_conf_write(pc, tag, off + PCI_MSI_MD64, sc.sc_msi_md.get());
                } else {
                    pci_conf_write(pc, tag, off + PCI_MSI_MD32, sc.sc_msi_md.get());
                }
                pci_conf_write(pc, tag, off + PCI_MSI_MC, sc.sc_msi_mc.get());
            }

            // Restore command register last to avoid exposing uninitialised windows.
            let reg = pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG);
            pci_conf_write(
                pc,
                tag,
                PCI_COMMAND_STATUS_REG,
                (reg & 0xffff_0000) | (sc.sc_csr.get() & 0x0000_ffff),
            );

            config_activate_children(self_, act)
        }
        DVACT_POWERDOWN => {
            let rv = config_activate_children(self_, act);

            if PCI_DOPM.load(Ordering::Relaxed) != 0 {
                // Place the bridge into the lowest possible power state.
                sc.sc_pmcsr_state.set(pci_get_powerstate(pc, tag));
                pci_set_powerstate(pc, tag, pci_min_powerstate(pc, tag) as i32);
            }
            rv
        }
        _ => config_activate_children(self_, act),
    }
}

/// The I/O base and limit register (`PPB_REG_IOSTATUS`) for a window, keeping the status in
/// the upper half of `blr`. As in C, the base is shifted unmasked: a window above 64 KB
/// spills its upper bits into the limit byte (the C's expression, kept).
const fn ppb_iostatus(blr: Pcireg, iobase: u64, iolimit: u64) -> Pcireg {
    (blr & 0xffff_0000) | (iolimit as Pcireg & PPB_IO_MASK) | (iobase >> PPB_IO_SHIFT) as Pcireg
}

/// The upper 16 bits of an I/O window's base and limit (`PPB_REG_IO_HI`).
const fn ppb_io_hi(iobase: u64, iolimit: u64) -> Pcireg {
    (((iobase & 0xffff_0000) >> 16) | (iolimit & 0xffff_0000)) as Pcireg
}

/// A memory window's base and limit register (`PPB_REG_MEM`, `PPB_REG_PREFMEM`).
const fn ppb_mem(base: u64, limit: u64) -> Pcireg {
    (limit as Pcireg & PPB_MEM_MASK) | (base >> PPB_MEM_SHIFT) as Pcireg
}

/// `ppb_alloc_busrange`: takes a bus range for the bridge from its parent's bus extent (the
/// device tree's `bus-range` when the node has one, else the largest of 16, 8, ... buses that
/// fits) and programs the bridge's bus numbers.
pub fn ppb_alloc_busrange(sc: &PpbSoftc, pa: &PciAttachArgs, busex: &Extent, busdata: &mut Pcireg) {
    let Some(pc) = sc.pc() else {
        return;
    };
    let mut busnum: u64 = 0;
    let mut busrange: u64 = 0;

    // __HAVE_FDT: PCITAG_NODE is 0 where there is no device tree.
    let node = pcitag_node(pa.pa_tag);
    let mut bus_range = [0u32; 2];
    if node != 0
        && OF_getpropintarray(node, b"bus-range", &mut bus_range) == size_of_val(&bus_range) as i32
    {
        let n = u64::from(bus_range[1].wrapping_sub(bus_range[0])) + 1;
        if extent_alloc_region(busex, u64::from(bus_range[0]), n, EX_NOWAIT).is_ok() {
            busnum = u64::from(bus_range[0]);
            busrange = n;
        }
    }

    if busrange == 0 {
        busrange = 16;
        while busrange > 0 {
            if let Ok(b) = extent_alloc(busex, busrange, 1, 0, 0, EX_NOWAIT) {
                busnum = b;
                break;
            }
            busrange >>= 1;
        }
    }

    if busrange > 0 {
        *busdata |= pa.pa_bus;
        *busdata |= (busnum << 8) as Pcireg;
        *busdata |= ((busnum + busrange - 1) << 16) as Pcireg;
        pci_conf_write(pc, pa.pa_tag, PPB_REG_BUSINFO, *busdata);
    }
}

/// `ppb_alloc_resources`: when there are devices behind the bridge, opens I/O and memory
/// windows for them out of the parent's extents if the firmware left none, and turns on the
/// bridge's decoding and bus mastering.
pub fn ppb_alloc_resources(sc: &PpbSoftc, ex: ParentExtents) {
    let Some(pc) = sc.pc() else {
        return;
    };
    let tag = sc.tag();
    let mut io_count = 0;
    let mut mem_count = 0;

    let Some(memex) = ex.memex else {
        return;
    };

    let busdata = pci_conf_read(pc, tag, PPB_REG_BUSINFO);
    let bus = ppb_businfo_secondary(busdata) as i32;
    if bus == 0 {
        return;
    }

    // Count number of devices. If there are no devices behind this bridge, there's no point
    // in allocating any address space.
    for dev in 0..pci_bus_maxdevs(pc, bus) {
        let dtag = pci_make_tag(pc, bus, dev, 0);
        let id = pci_conf_read(pc, dtag, PCI_ID_REG);

        if pci_vendor(id) == PCI_VENDOR_INVALID || pci_vendor(id) == 0 {
            continue;
        }

        let bhlcr = pci_conf_read(pc, dtag, PCI_BHLC_REG);
        let (reg_start, reg_end, reg_rom) = match pci_hdrtype_type(bhlcr) {
            0 => (PCI_MAPREG_START, PCI_MAPREG_END, PCI_ROM_REG),
            1 => {
                // PCI-PCI bridge
                io_count += 1;
                mem_count += 1;
                (PCI_MAPREG_START, PCI_MAPREG_PPB_END, 0) // 0x38
            }
            2 => {
                // PCI-Cardbus bridge
                io_count += 1;
                mem_count += 1;
                (PCI_MAPREG_START, PCI_MAPREG_PCB_END, 0)
            }
            _ => return,
        };

        let mut reg = reg_start;
        while reg < reg_end {
            if let Some(type_) = pci_mapreg_probe(pc, dtag, reg) {
                if type_ == PCI_MAPREG_TYPE_IO {
                    io_count += 1;
                } else {
                    mem_count += 1;
                }
                if type_ == (PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_64BIT) {
                    reg += 4;
                }
            }
            reg += 4;
        }

        if reg_rom != 0 {
            let addr = pci_conf_read(pc, dtag, reg_rom);
            pci_conf_write(pc, dtag, reg_rom, !PCI_ROM_ENABLE);
            let mask = pci_conf_read(pc, dtag, reg_rom);
            pci_conf_write(pc, dtag, reg_rom, addr);
            if pci_rom_size(mask) != 0 {
                mem_count += 1;
            }
        }
    }

    let mut csr = pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG);

    // Get the bridge in a consistent state. If memory mapped I/O or port I/O is disabled,
    // disabled the associated windows as well.
    if csr & PCI_COMMAND_MEM_ENABLE == 0 {
        pci_conf_write(pc, tag, PPB_REG_MEM, 0x0000_ffff);
        pci_conf_write(pc, tag, PPB_REG_PREFMEM, 0x0000_ffff);
        pci_conf_write(pc, tag, PPB_REG_PREFBASE_HI32, 0);
        pci_conf_write(pc, tag, PPB_REG_PREFLIM_HI32, 0);
    }
    if csr & PCI_COMMAND_IO_ENABLE == 0 {
        pci_conf_write(pc, tag, PPB_REG_IOSTATUS, 0x0000_00ff);
        pci_conf_write(pc, tag, PPB_REG_IO_HI, 0x0000_ffff);
    }

    // Allocate I/O address space if necessary.
    if io_count > 0
        && let Some(ioex) = ex.ioex
    {
        let blr = u64::from(pci_conf_read(pc, tag, PPB_REG_IOSTATUS));
        let mut iobase = (blr << PPB_IO_SHIFT) & u64::from(PPB_IO_MASK);
        let mut iolimit = (blr & u64::from(PPB_IO_MASK)) | 0x0000_0fff;
        let blr = u64::from(pci_conf_read(pc, tag, PPB_REG_IO_HI));
        iobase |= (blr & 0x0000_ffff) << 16;
        iolimit |= blr & 0xffff_0000;
        sc.sc_iobase.set(iobase);
        sc.sc_iolimit.set(iolimit);
        if iolimit < iobase || iobase == 0 {
            #[allow(clippy::unnecessary_min_or_max)] // the C's max(); PCI_IO_START is 0 on arm64
            let start = PCI_IO_START.max(ioex.ex_start);
            let end = PCI_IO_END.min(ioex.ex_end);
            let mut size: u64 = 0x2000;
            let mut base = 0;
            while size >= u64::from(PPB_IO_MIN) {
                if let Ok(b) = extent_alloc_subregion(ioex, start, end, size, size, 0, 0, 0) {
                    base = b;
                    break;
                }
                size >>= 1;
            }
            if size >= u64::from(PPB_IO_MIN) {
                sc.sc_iobase.set(base);
                sc.sc_iolimit.set(base + size - 1);
                let blr = pci_conf_read(pc, tag, PPB_REG_IOSTATUS);
                pci_conf_write(
                    pc,
                    tag,
                    PPB_REG_IOSTATUS,
                    ppb_iostatus(blr, base, base + size - 1),
                );
                pci_conf_write(pc, tag, PPB_REG_IO_HI, ppb_io_hi(base, base + size - 1));

                csr |= PCI_COMMAND_IO_ENABLE;
            }
        }
    }

    // Allocate memory mapped I/O address space if necessary.
    if mem_count > 0 {
        let blr = u64::from(pci_conf_read(pc, tag, PPB_REG_MEM));
        let membase = (blr << PPB_MEM_SHIFT) & u64::from(PPB_MEM_MASK);
        let memlimit = (blr & u64::from(PPB_MEM_MASK)) | 0x000f_ffff;
        sc.sc_membase.set(membase);
        sc.sc_memlimit.set(memlimit);
        if memlimit < membase || membase == 0 {
            #[allow(clippy::unnecessary_min_or_max)] // the C's max(); PCI_MEM_START is 0 on arm64
            let start = PCI_MEM_START.max(memex.ex_start);
            let end = PCI_MEM_END.min(memex.ex_end);
            let mut size: u64 = 0x200_0000;
            let mut base = 0;
            while size >= u64::from(PPB_MEM_MIN) {
                if let Ok(b) = extent_alloc_subregion(memex, start, end, size, size, 0, 0, 0) {
                    base = b;
                    break;
                }
                size >>= 1;
            }
            if size >= u64::from(PPB_MEM_MIN) {
                sc.sc_membase.set(base);
                sc.sc_memlimit.set(base + size - 1);
                pci_conf_write(pc, tag, PPB_REG_MEM, ppb_mem(base, base + size - 1));

                csr |= PCI_COMMAND_MEM_ENABLE;
            }
        }
    }

    // Enable bus master.
    csr |= PCI_COMMAND_MASTER_ENABLE;

    pci_conf_write(pc, tag, PCI_COMMAND_STATUS_REG, csr);
}

/// `ppb_intr`: a presence-detect change on the slot queues the insert or the remove task.
fn ppb_intr(arg: *mut c_void) -> i32 {
    // SAFETY: the interrupt was established with the bridge's softc.
    let sc = unsafe { PpbSoftc::from_arg(arg) };

    // XXX ignore hotplug events while in autoconf. On some machines with onboard re(4), we
    // get a bogus hotplug remove event when we reset that device. Ignoring that event makes
    // sure we will not try to forcibly detach re(4) when it isn't ready to deal with that.
    if COLD.load(Ordering::Relaxed) {
        return 0;
    }
    let Some(pc) = sc.pc() else {
        return 0;
    };

    let reg = pci_conf_read(pc, sc.tag(), sc.sc_cap_off.get() + PCI_PCIE_SLCSR);
    if reg & PCI_PCIE_SLCSR_PDC != 0 {
        if reg & PCI_PCIE_SLCSR_PDS != 0 {
            task_add(SYSTQ, &sc.sc_insert_task);
        } else {
            task_add(SYSTQ, &sc.sc_remove_task);
        }

        // Clear interrupts.
        pci_conf_write(pc, sc.tag(), sc.sc_cap_off.get() + PCI_PCIE_SLCSR, reg);
        return 1;
    }

    0
}

/// `ppb_hotplug_insert`: a card appeared in an empty slot; rescan once it settled.
fn ppb_hotplug_insert(xsc: *mut c_void) {
    // SAFETY: the task was set with the bridge's softc.
    let sc = unsafe { PpbSoftc::from_arg(xsc) };
    let Some(psc) = sc.psc() else {
        return;
    };

    if !psc.sc_devs.is_empty() {
        return;
    }

    // XXX Powerup the card.

    // XXX Turn on LEDs.

    // Wait a second for things to settle.
    timeout_add_sec(&sc.sc_to, 1);
}

/// `ppb_hotplug_insert_finish`.
fn ppb_hotplug_insert_finish(arg: *mut c_void) {
    // SAFETY: the timeout was set with the bridge's softc.
    let sc = unsafe { PpbSoftc::from_arg(arg) };

    task_add(SYSTQ, &sc.sc_rescan_task);
}

/// `ppb_hotplug_rescan`: probes the bus behind the bridge again.
fn ppb_hotplug_rescan(xsc: *mut c_void) {
    // SAFETY: the task was set with the bridge's softc.
    let sc = unsafe { PpbSoftc::from_arg(xsc) };

    if let Some(psc) = sc.psc() {
        pci_enumerate_bus(psc, None, None);
    }
}

/// `ppb_hotplug_remove`: the card left: detach what was behind the bridge and give its
/// windows back.
fn ppb_hotplug_remove(xsc: *mut c_void) {
    // SAFETY: the task was set with the bridge's softc.
    let sc = unsafe { PpbSoftc::from_arg(xsc) };

    if let Some(psc) = sc.psc() {
        let _ = pci_detach_devices(psc, DETACH_FORCE);

        // XXX Allocate the entire window with EX_CONFLICTOK such that we can easily free it.
        for (ex, base, limit) in [
            (&sc.sc_ioex, &sc.sc_iobase, &sc.sc_iolimit),
            (&sc.sc_memex, &sc.sc_membase, &sc.sc_memlimit),
            (&sc.sc_pmemex, &sc.sc_pmembase, &sc.sc_pmemlimit),
        ] {
            if let Some(ex) = ex.get() {
                let size = limit.get().wrapping_sub(base.get()).wrapping_add(1);
                let _ = extent_alloc_region(ex, base.get(), size, EX_NOWAIT | EX_CONFLICTOK);
                let _ = extent_free(ex, base.get(), size, EX_NOWAIT);
            }
        }
    }
}

/// `ppbprint`: only PCIs can attach to PPBs; easy.
pub fn ppbprint(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: ppbattach hands its child a `pcibus_attach_args`.
    let pba = unsafe { &*aux.cast::<PcibusAttachArgs>() };

    if let Some(pnp) = pnp {
        printf(format_args!("pci at {}", Str(pnp)));
    }
    printf(format_args!(" bus {}", pba.pba_bus));
    UNCONF
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::pci::pcireg::PCI_CLASS_MASS_STORAGE;
    use crate::sys::device::Cfdata;
    use std::boxed::Box;

    /// Attach arguments carrying only an ID and a class register.
    fn args(id: Pcireg, class: Pcireg) -> PciAttachArgs {
        PciAttachArgs {
            pa_iot: Default::default(),
            pa_memt: Default::default(),
            pa_dmat: Default::default(),
            pa_pc: Default::default(),
            pa_flags: 0,
            pa_ioex: None,
            pa_memex: None,
            pa_pmemex: None,
            pa_busex: None,
            pa_domain: 0,
            pa_bus: 0,
            pa_device: 4,
            pa_function: 0,
            pa_tag: Default::default(),
            pa_id: id,
            pa_class: class,
            pa_bridgetag: None,
            pa_bridgeih: None,
            pa_intrswiz: 0,
            pa_intrtag: Default::default(),
            pa_intrpin: 0,
            pa_intrline: 0,
            pa_rawintrpin: 0,
        }
    }

    fn matches(id: Pcireg, class: Pcireg) -> i32 {
        let cf: &'static Cfdata = Box::leak(Box::new(Cfdata::free()));
        let mut pa = args(id, class);
        ppbmatch(None, &CfMatch::Cfdata(cf), ptr::from_mut(&mut pa).cast())
    }

    #[test]
    fn matches_pci_bridges_but_the_vt82c586_power_function() {
        // Class 0x06 (bridge), subclass 0x04 (PCI): QEMU's root port and its pci-bridge.
        let bridge = 0x0604_0000;
        assert_eq!(matches(0x000c_1b36, bridge), 1);
        assert_eq!(matches(0x0001_1b36, bridge | 0x0100), 1);
        assert_eq!(matches(0x3040_1106, bridge), 0);
        assert_eq!(matches(0x1001_1af4, PCI_CLASS_MASS_STORAGE << 24), 0);
        // A host bridge (subclass 0) is not one.
        assert_eq!(matches(0x29c0_8086, 0x0600_0000), 0);
    }

    #[test]
    fn window_registers_encode_as_in_c() {
        // I/O: base 0x1000, limit 0x1fff, the status half kept.
        assert_eq!(ppb_iostatus(0x2280_0101, 0x1000, 0x1fff), 0x2280_1010);
        // The upper halves of an I/O window above 64 KB.
        assert_eq!(ppb_io_hi(0x1_2000, 0x1_ffff), 0x0001_0001);
        // Memory: 0xfe000000..0xfe1fffff.
        assert_eq!(ppb_mem(0xfe00_0000, 0xfe1f_ffff), 0xfe10_fe00);
    }
}
/* </TESTS> */
