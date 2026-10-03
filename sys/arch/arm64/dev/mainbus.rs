/* $OpenBSD: mainbus.c,v 1.37 2026/06/22 12:20:52 deraadt Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2016 Patrick Wildt <patrick@blueri.se>
 * Copyright (c) 2017 Mark Kettenis <kettenis@openbsd.org>
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

//! The arm64 root bus: `arch/arm64/dev/mainbus.c`. Mainbus takes care of FDT and non-FDT
//! machines: it starts the interrupt controllers and the generic timer's delay, then offers
//! every node of the device tree to the drivers that attach at `fdt` (PSCI first, the CPUs,
//! `/firmware`, `/reserved-memory`, then the root's children in three passes, `early 2`,
//! `early 1` and the rest), and leaves the framebuffers in `/chosen` for after the root file
//! system is mounted.
//!
//! Upstream: sys/arch/arm64/dev/mainbus.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `iommu_device_map` (`ofw_misc.c`) is reported; without it every node keeps mainbus's
//!   tag or its `dma-coherent` copy, which is what the C gets when no IOMMU claims the node.
//! - `mainbus_dma_tag` is not placed in `.rodata` by hand: a Rust `static` without interior
//!   mutability already is read-only.
//! - `thermal_init` (`ofw_thermal.c`) is reported.
//! - `struct fdt_attach_args` cannot carry a null bus space tag, so the `efi` and `apm`
//!   arguments, which the C zeroes but for the name, carry mainbus's bus space and DMA tags.
//! - `cf_loc[0]` (the `early` locator) of an entry without locators reads as 0, the
//!   locator's default.
//! - A node whose `reg` lines would be zero cells long (`#address-cells` and `#size-cells`
//!   both 0) gets no `fa_reg`, where the C would divide by zero.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::Ordering;

use libkern::strlcpy;

use crate::arch::arm64::arm64::bus_dma::{
    _dmamap_create, _dmamap_destroy, _dmamap_load, _dmamap_load_buffer, _dmamap_load_mbuf,
    _dmamap_load_raw, _dmamap_load_uio, _dmamap_sync, _dmamap_unload, _dmamem_alloc,
    _dmamem_alloc_range, _dmamem_free, _dmamem_map, _dmamem_mmap, _dmamem_unmap,
};
use crate::arch::arm64::arm64::bus_space::ARM64_BS_TAG;
use crate::arch::arm64::arm64::intr::arm_intr_init_fdt;
use crate::arch::arm64::dev::agtimer::agtimer_init;
use crate::arch::arm64::include::armreg::{MPIDR_AFF, read_specialreg};
use crate::arch::arm64::include::bus::{self, BUS_DMA_COHERENT};
use crate::arch::arm64::include::fdt::FdtAttachArgs;
use crate::dev::ofw::fdt::FdtReg;
use crate::dev::ofw::openfirm::{
    OF_child, OF_finddevice, OF_getprop, OF_getpropint, OF_getpropintarray, OF_getproplen,
    OF_is_compatible, OF_is_enabled, OF_peer,
};
use crate::kern::init_main::NCPUSFOUND;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_sysctl::{hw_prod, hw_serial};
use crate::kern::subr_autoconf::{config_found, config_found_sm, config_mountroot};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::machine::bus::{BusDmaTag, BusSpaceTag};
use crate::sys::device::{
    CfMatch, Cfattach, Cfdriver, CfmatchT, CfprintT, DV_DULL, Device, QUIET, Softc, UNCONF,
};
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_TEMP, M_WAITOK, M_ZERO};
use crate::unported;

/// `struct mainbus_softc`.
#[repr(C)]
pub struct MainbusSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_node`: the root node.
    pub sc_node: Cell<i32>,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_acells`: the `#address-cells` of the nodes being attached.
    pub sc_acells: Cell<i32>,
    /// `sc_scells`: their `#size-cells`.
    pub sc_scells: Cell<i32>,
    /// `sc_ranges`: the root's `ranges`, `sc_rangeslen` bytes (`malloc`ed, `M_TEMP`).
    pub sc_ranges: Cell<*mut u32>,
    /// `sc_rangeslen`.
    pub sc_rangeslen: Cell<i32>,
    /// `sc_early`: the `early` locator of the pass under way.
    pub sc_early: Cell<i32>,
    /// `sc_early_nodes`: the nodes attached in an early pass, 0-terminated.
    pub sc_early_nodes: [Cell<i32>; 64],
}

impl MainbusSoftc {
    /// `sc->sc_iot`, set by `mainbus_attach` before any child is offered.
    fn iot(&self) -> BusSpaceTag {
        match self.sc_iot.get() {
            Some(iot) => iot,
            None => panic(format_args!("mainbus: no bus space")),
        }
    }

    /// `sc->sc_dmat`, set by `mainbus_attach` before any child is offered.
    fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(dmat) => dmat,
            None => panic(format_args!("mainbus: no dma tag")),
        }
    }
}

// SAFETY: `#[repr(C)]`, the device first; every other field is a `Cell` of an integer, a
// raw pointer or an `Option` of a reference, all valid as zero.
unsafe impl Softc for MainbusSoftc {}

/// `mainbus_ca`.
pub static MAINBUS_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<MainbusSoftc>(),
    ca_match: Some(mainbus_match),
    ca_attach: mainbus_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `mainbus_cd`.
pub static MAINBUS_CD: Cfdriver = Cfdriver::new(b"mainbus", DV_DULL, 0);

/// `mainbus_dma_tag`: the generic `bus_dma` functions, not cache-coherent.
pub static MAINBUS_DMA_TAG: bus::BusDmaTag = bus::BusDmaTag {
    _cookie: ptr::null_mut(),
    _flags: 0,
    _dmamap_create,
    _dmamap_destroy,
    _dmamap_load,
    _dmamap_load_mbuf,
    _dmamap_load_uio,
    _dmamap_load_raw,
    _dmamap_load_buffer,
    _dmamap_unload,
    _dmamap_sync,
    _dmamem_alloc,
    _dmamem_alloc_range,
    _dmamem_free,
    _dmamem_map,
    _dmamem_unmap,
    _dmamem_mmap,
    _dma_mask: 0,
};

/// `(struct mainbus_softc *)self`.
fn softc(dev: &Device) -> &MainbusSoftc {
    // SAFETY: only mainbus's own functions call this, with the device `mainbus_ca` made.
    unsafe { dev.softc::<MainbusSoftc>() }
}

/// The `struct fdt_attach_args` a mainbus child gets as `aux`.
fn fdt_args<'a>(aux: *mut c_void) -> &'a FdtAttachArgs<'a> {
    // SAFETY: mainbus hands every child a `FdtAttachArgs` that lives across the call.
    unsafe { &*aux.cast::<FdtAttachArgs<'a>>() }
}

/// A C string in a buffer, up to its first NUL.
fn cstr(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]
}

/// `mainbus_match`: mainbus takes care of FDT and non-FDT machines, so we always attach.
pub fn mainbus_match(_parent: Option<&Device>, _cfdata: &CfMatch, _aux: *mut c_void) -> i32 {
    1
}

/// `s = malloc(len, M_DEVBUF, M_NOWAIT); if (s) strlcpy(s, prop, len);`: a copy of a
/// device-tree string that lives as long as the kernel, or `None` when memory is short.
fn kept_string(prop: &[u8], len: usize) -> Option<&'static [u8]> {
    let buf = malloc(len, M_DEVBUF, M_NOWAIT)?;
    // SAFETY: malloc returned `len` bytes that nothing else references; they are never
    // freed, as the C never frees hw_prod and hw_serial.
    let dst: &'static mut [u8] = unsafe { slice::from_raw_parts_mut(buf.as_ptr(), len) };
    strlcpy(dst, prop);
    Some(dst)
}

/// `mainbus_attach`.
pub fn mainbus_attach(_parent: Option<&Device>, self_: &Device, _aux: *mut c_void) {
    let sc = softc(self_);
    let mut prop = [0u8; 128];

    arm_intr_init_fdt();
    agtimer_init();

    sc.sc_node.set(OF_peer(0));
    sc.sc_iot.set(Some(&ARM64_BS_TAG));
    sc.sc_dmat.set(Some(&MAINBUS_DMA_TAG));
    sc.sc_acells
        .set(OF_getpropint(OF_peer(0), b"#address-cells", 1) as i32);
    sc.sc_scells
        .set(OF_getpropint(OF_peer(0), b"#size-cells", 1) as i32);

    let len = OF_getprop(sc.sc_node.get(), b"model", &mut prop);
    if len > 0 {
        printf(format_args!(": {}\n", Str(&prop)));
        // SAFETY: mainbus attaches once, on the boot CPU, before any process can read
        // hw_prod through sysctl(2).
        unsafe { *hw_prod.get_mut() = kept_string(&prop, len as usize) };
    } else {
        printf(format_args!(": unknown model\n"));
    }

    let len = OF_getprop(sc.sc_node.get(), b"serial-number", &mut prop);
    if len > 0 {
        // SAFETY: as for hw_prod.
        unsafe { *hw_serial.get_mut() = kept_string(&prop, len as usize) };
    }

    mainbus_attach_psci(self_);
    mainbus_attach_efi(self_);

    // Attach primary CPU first.
    mainbus_attach_cpus(self_, mainbus_match_primary);

    // Attach secondary CPUs.
    mainbus_attach_cpus(self_, mainbus_match_secondary);

    mainbus_attach_firmware(self_);
    mainbus_attach_resvmem(self_);

    sc.sc_rangeslen.set(OF_getproplen(OF_peer(0), b"ranges"));
    let rangeslen = sc.sc_rangeslen.get();
    if rangeslen > 0 && rangeslen % 4 == 0 {
        let ranges = cells_alloc(rangeslen as usize, M_TEMP);
        // SAFETY: a fresh allocation of `rangeslen` bytes, kept as `sc_ranges`.
        let cells = unsafe { slice::from_raw_parts_mut(ranges.as_ptr(), rangeslen as usize / 4) };
        OF_getpropintarray(OF_peer(0), b"ranges", cells);
        sc.sc_ranges.set(ranges.as_ptr());
    }

    mainbus_attach_apm(self_);

    // Scan the whole tree.
    sc.sc_early.set(2);
    while sc.sc_early.get() >= 0 {
        let mut node = OF_child(sc.sc_node.get());
        while node != 0 {
            mainbus_attach_node(self_, node, None);
            node = OF_peer(node);
        }
        sc.sc_early.set(sc.sc_early.get() - 1);
    }
    sc.sc_early.set(0);

    // Delay attaching the framebuffer to give other drivers a chance to claim it.
    config_mountroot(self_, mainbus_attach_framebuffer);

    let _ = unported!("thermal_init (ofw_thermal.c)");
}

/// `mainbus_print`: names a node no driver took, unless it is one nobody expects a driver
/// for.
pub fn mainbus_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    let fa = fdt_args(aux);
    let mut buf = [0u8; 32];

    let Some(pnp) = pnp else {
        return QUIET;
    };

    if !OF_is_enabled(fa.fa_node) {
        return QUIET;
    }

    if OF_getprop(fa.fa_node, b"name", &mut buf) > 0 {
        let last = buf.len() - 1;
        buf[last] = 0;
        let name = cstr(&buf);
        if name == b"aliases"
            || name == b"chosen"
            || name == b"cpus"
            || name == b"memory"
            || name == b"reserved-memory"
            || name == b"thermal-zones"
            || name.starts_with(b"__")
        {
            return QUIET;
        }
        printf(format_args!("\"{}\"", Str(name)));
    } else {
        printf(format_args!("node {}", fa.fa_node as u32));
    }

    printf(format_args!(" at {}", Str(pnp)));

    UNCONF
}

/// `malloc(len, type, M_WAITOK)` of 32-bit cells.
fn cells_alloc(len: usize, type_: i32) -> NonNull<u32> {
    match malloc(len, type_, M_WAITOK) {
        Some(p) => p.cast(),
        None => panic(format_args!("mainbus: out of memory for {len} bytes")),
    }
}

/// `mainbus_attach_node`: look for a driver that wants to be attached to this node.
pub fn mainbus_attach_node(self_: &Device, node: i32, submatch: Option<CfmatchT>) {
    let sc = softc(self_);

    // Skip if already attached early.
    for slot in &sc.sc_early_nodes {
        if slot.get() == node {
            return;
        }
        if slot.get() == 0 {
            break;
        }
    }

    let acells = sc.sc_acells.get();
    let scells = sc.sc_scells.get();

    let mut reg: Option<(NonNull<FdtReg>, usize)> = None;
    let len = OF_getproplen(node, b"reg");
    let line = (acells + scells) * 4;
    if len > 0 && line > 0 && len % line == 0 {
        let raw = cells_alloc(len as usize, M_TEMP);
        // SAFETY: a fresh allocation of `len` bytes, freed below.
        let cells = unsafe { slice::from_raw_parts_mut(raw.as_ptr(), len as usize / 4) };
        OF_getpropintarray(node, b"reg", cells);

        let nreg = (len / line) as usize;
        let regs = match malloc(nreg * size_of::<FdtReg>(), M_DEVBUF, M_WAITOK) {
            Some(p) => p.cast::<FdtReg>(),
            None => panic(format_args!("mainbus_attach_node: out of memory")),
        };

        let mut cell = 0;
        for i in 0..nreg {
            let mut r = FdtReg::default();
            if acells >= 1 {
                r.addr = u64::from(cells[cell]);
            }
            if acells == 2 {
                r.addr <<= 32;
                r.addr |= u64::from(cells[cell + 1]);
            }
            cell += acells as usize;
            if scells >= 1 {
                r.size = u64::from(cells[cell]);
            }
            if scells == 2 {
                r.size <<= 32;
                r.size |= u64::from(cells[cell + 1]);
            }
            cell += scells as usize;
            // SAFETY: `regs` has room for `nreg` entries.
            unsafe { regs.add(i).write(r) };
        }

        free(raw.cast(), M_TEMP, len as usize);
        reg = Some((regs, nreg));
    }

    let mut intr: Option<(NonNull<u32>, usize)> = None;
    let len = OF_getproplen(node, b"interrupts");
    if len > 0 && len % 4 == 0 {
        let p = cells_alloc(len as usize, M_DEVBUF);
        let nintr = len as usize / 4;
        // SAFETY: a fresh allocation of `nintr` cells, freed below.
        let cells = unsafe { slice::from_raw_parts_mut(p.as_ptr(), nintr) };
        OF_getpropintarray(node, b"interrupts", cells);
        intr = Some((p, nintr));
    }

    let mut dmat: BusDmaTag = sc.dmat();
    if OF_getproplen(node, b"dma-coherent") >= 0 {
        let Some(p) = malloc(size_of::<bus::BusDmaTag>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
            panic(format_args!("mainbus: out of memory for a dma tag"));
        };
        let tag = p.cast::<bus::BusDmaTag>();
        // SAFETY: a fresh allocation of a tag's size, written once before it is shared and
        // never freed, as in C: the child keeps it.
        unsafe {
            tag.write(bus::BusDmaTag {
                _flags: dmat._flags | BUS_DMA_COHERENT,
                ..*dmat
            });
            dmat = &*tag.as_ptr();
        }
    }

    // iommu_device_map(fa.fa_node, fa.fa_dmat): ofw_misc.c, which hands back the tag
    // unchanged when no IOMMU claims the node.
    let _ = unported!("iommu_device_map (ofw_misc.c)");

    let print: Option<CfprintT> = if submatch.is_none() && sc.sc_early.get() == 0 {
        Some(mainbus_print)
    } else {
        None
    };
    let submatch = submatch.unwrap_or(mainbus_match_status);

    let child = {
        let mut fa = FdtAttachArgs {
            fa_name: b"",
            fa_node: node,
            fa_iot: sc.iot(),
            fa_dmat: dmat,
            // SAFETY: `nreg` entries written above, alive until the frees below.
            fa_reg: reg.map_or(&[], |(p, n)| unsafe {
                slice::from_raw_parts(p.as_ptr(), n)
            }),
            // SAFETY: `nintr` cells read above, alive until the frees below.
            fa_intr: intr.map_or(&[], |(p, n)| unsafe {
                slice::from_raw_parts(p.as_ptr(), n)
            }),
            fa_acells: acells,
            fa_scells: scells,
        };
        config_found_sm(self_, ptr::from_mut(&mut fa).cast(), print, Some(submatch))
    };

    // Record nodes that we attach early.
    if child.is_some() && sc.sc_early.get() != 0 {
        for slot in &sc.sc_early_nodes {
            if slot.get() != 0 {
                continue;
            }
            slot.set(node);
            break;
        }
    }

    if let Some((p, n)) = reg {
        free(p.cast(), M_DEVBUF, n * size_of::<FdtReg>());
    }
    if let Some((p, n)) = intr {
        free(p.cast(), M_DEVBUF, n * size_of::<u32>());
    }
}

/// `mainbus_match_status`: an enabled node, offered to the drivers whose `early` locator is
/// the pass under way.
pub fn mainbus_match_status(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let Some(parent) = parent else {
        return 0;
    };
    let sc = softc(parent);
    let fa = fdt_args(aux);
    let cf = match_.cfdata();

    if !OF_is_enabled(fa.fa_node) {
        return 0;
    }

    if cf.cf_loc.first().copied().unwrap_or(0) == i64::from(sc.sc_early.get()) {
        return ca_match(cf.cf_attach.ca_match, Some(parent), match_, aux);
    }

    0
}

/// `(*cf->cf_attach->ca_match)(parent, match, aux)`.
fn ca_match(
    f: Option<CfmatchT>,
    parent: Option<&Device>,
    match_: &CfMatch,
    aux: *mut c_void,
) -> i32 {
    match f {
        Some(f) => f(parent, match_, aux),
        None => panic(format_args!("mainbus: no match function")),
    }
}

/// `mainbus_attach_cpus`: offers the children of `/cpus`, counting `ncpusfound`.
pub fn mainbus_attach_cpus(self_: &Device, match_: CfmatchT) {
    let sc = softc(self_);
    let mut node = OF_finddevice(b"/cpus");
    let mut buf = [0u8; 32];

    if node == -1 {
        return;
    }

    let acells = sc.sc_acells.get();
    let scells = sc.sc_scells.get();
    sc.sc_acells
        .set(OF_getpropint(node, b"#address-cells", 2) as i32);
    sc.sc_scells
        .set(OF_getpropint(node, b"#size-cells", 0) as i32);

    NCPUSFOUND.store(0, Ordering::Relaxed);
    node = OF_child(node);
    while node != 0 {
        if OF_getprop(node, b"device_type", &mut buf) > 0 && cstr(&buf) == b"cpu" {
            NCPUSFOUND.fetch_add(1, Ordering::Relaxed);
        }

        mainbus_attach_node(self_, node, Some(match_));
        node = OF_peer(node);
    }

    sc.sc_acells.set(acells);
    sc.sc_scells.set(scells);
}

/// `mainbus_match_primary`: the CPU node of the CPU we are running on.
pub fn mainbus_match_primary(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let fa = fdt_args(aux);
    let cf = match_.cfdata();
    let mpidr = read_specialreg!("mpidr_el1");

    if fa.fa_reg.is_empty() || fa.fa_reg[0].addr != (mpidr & MPIDR_AFF) {
        return 0;
    }

    ca_match(cf.cf_attach.ca_match, parent, match_, aux)
}

/// `mainbus_match_secondary`: the other CPU nodes.
pub fn mainbus_match_secondary(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let fa = fdt_args(aux);
    let cf = match_.cfdata();
    let mpidr = read_specialreg!("mpidr_el1");

    if fa.fa_reg.is_empty() || fa.fa_reg[0].addr == (mpidr & MPIDR_AFF) {
        return 0;
    }

    ca_match(cf.cf_attach.ca_match, parent, match_, aux)
}

/// `mainbus_attach_psci`: PSCI attaches first, in an early pass of its own.
pub fn mainbus_attach_psci(self_: &Device) {
    let sc = softc(self_);
    let node = OF_finddevice(b"/psci");

    if node == -1 {
        return;
    }

    sc.sc_early.set(1);
    mainbus_attach_node(self_, node, None);
    sc.sc_early.set(0);
}

/// `mainbus_attach_efi`: the EFI runtime services, when the bootloader passed their table.
pub fn mainbus_attach_efi(self_: &Device) {
    let sc = softc(self_);
    let node = OF_finddevice(b"/chosen");

    if node == -1 || OF_getproplen(node, b"openbsd,uefi-system-table") <= 0 {
        return;
    }

    let mut fa = FdtAttachArgs {
        fa_name: b"efi",
        fa_node: 0,
        fa_iot: sc.iot(),
        fa_dmat: sc.dmat(),
        fa_reg: &[],
        fa_intr: &[],
        fa_acells: 0,
        fa_scells: 0,
    };
    let _ = config_found(self_, ptr::from_mut(&mut fa).cast(), None);
}

/// `mainbus_attach_apm`.
pub fn mainbus_attach_apm(self_: &Device) {
    let sc = softc(self_);
    let mut fa = FdtAttachArgs {
        fa_name: b"apm",
        fa_node: 0,
        fa_iot: sc.iot(),
        fa_dmat: sc.dmat(),
        fa_reg: &[],
        fa_intr: &[],
        fa_acells: 0,
        fa_scells: 0,
    };

    let _ = config_found(self_, ptr::from_mut(&mut fa).cast(), None);
}

/// `mainbus_attach_framebuffer`: the `simple-framebuffer` nodes of `/chosen`.
pub fn mainbus_attach_framebuffer(self_: &Device) {
    let sc = softc(self_);
    let mut node = OF_finddevice(b"/chosen");

    if node == -1 {
        return;
    }

    // On some systems, such as the Raspberry Pi 5B, /chosen has its own #address-cells and
    // #size-cells that differ from the root node.
    let acells = sc.sc_acells.get();
    let scells = sc.sc_scells.get();
    sc.sc_acells
        .set(OF_getpropint(node, b"#address-cells", acells as u32) as i32);
    sc.sc_scells
        .set(OF_getpropint(node, b"#size-cells", scells as u32) as i32);

    node = OF_child(node);
    while node != 0 {
        if OF_is_compatible(node, b"simple-framebuffer") {
            mainbus_attach_node(self_, node, None);
        }
        node = OF_peer(node);
    }

    sc.sc_acells.set(acells);
    sc.sc_scells.set(scells);
}

/// `mainbus_attach_firmware`: the children of `/firmware`.
pub fn mainbus_attach_firmware(self_: &Device) {
    let mut node = OF_finddevice(b"/firmware");

    if node == -1 {
        return;
    }

    node = OF_child(node);
    while node != 0 {
        mainbus_attach_node(self_, node, None);
        node = OF_peer(node);
    }
}

/// `mainbus_attach_resvmem`: the children of `/reserved-memory`.
pub fn mainbus_attach_resvmem(self_: &Device) {
    let mut node = OF_finddevice(b"/reserved-memory");

    if node == -1 {
        return;
    }

    node = OF_child(node);
    while node != 0 {
        mainbus_attach_node(self_, node, None);
        node = OF_peer(node);
    }
}
