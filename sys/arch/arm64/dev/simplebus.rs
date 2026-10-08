/* $OpenBSD: simplebus.c,v 1.25 2026/08/19 20:14:06 kettenis Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2016 Patrick Wildt <patrick@blueri.se>
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
//! The device tree's `simple-bus`: `arch/arm64/dev/simplebus.c`. Simplebus is a generic bus
//! with no special casings: it offers each child node to the drivers that attach at `fdt`, in
//! the three `early` passes mainbus makes, with a bus space and a DMA tag that translate the
//! child's addresses through the node's `ranges` and `dma-ranges`. The interrupt controllers
//! with child nodes (`ampintc`'s GICv2m frames) attach their children with
//! [`simplebus_attach`] too.
//!
//! Upstream: sys/arch/arm64/dev/simplebus.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `iommu_device_map` (`ofw_misc.c`) is reported, as in mainbus; without it every child
//!   keeps the bus's DMA tag (or its `dma-coherent` copy), which is what the C gets when no
//!   IOMMU claims the node.
//! - `struct fdt_attach_args` takes slices for `fa_reg` and `fa_intr`.
//! - `simplebus_bs_mmap` returns `None` for the C's `-1`, and for its `EINVAL` (a `paddr_t`
//!   holding an errno when `ranges` is malformed), which no caller could tell from an address.
//! - The softc's tags are `MaybeUninit` (`machine/simplebusvar.rs`), written once here before
//!   any child sees them; the softc lives as long as the kernel (no detach), so the children
//!   get `'static` tags.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;

use crate::arch::arm64::include::bus::{
    BUS_DMA_COHERENT, BusDmaSegment, BusDmaTag, BusDmaTagT, BusDmamap, BusSpace, BusSpaceHandle,
};
use crate::arch::arm64::include::fdt::FdtAttachArgs;
use crate::arch::arm64::include::simplebusvar::SimplebusSoftc;
use crate::dev::ofw::fdt::FdtReg;
use crate::dev::ofw::openfirm::{
    OF_child, OF_getprop, OF_getpropint, OF_getpropintarray, OF_getproplen, OF_is_compatible,
    OF_is_enabled, OF_parent, OF_peer,
};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_autoconf::config_found_sm;
use crate::kern::subr_prf::{Str, panic, printf};
use crate::machine::bus::{BusAddr, BusSize};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, CfprintT, DV_DULL, Device, QUIET, UNCONF};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::proc::Proc;
use crate::sys::types::{Off, Paddr};
use crate::unported;

/// `simplebus_ca`.
pub static SIMPLEBUS_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<SimplebusSoftc>(),
    ca_match: Some(simplebus_match),
    ca_attach: simplebus_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `simplebus_cd`.
pub static SIMPLEBUS_CD: Cfdriver = Cfdriver::new(b"simplebus", DV_DULL, 0);

/// `(struct simplebus_softc *)self`: the softc of a device `simplebus_ca` made, or of a
/// driver whose softc begins with one (`ampintc`).
fn softc(dev: &Device) -> &'static SimplebusSoftc {
    // SAFETY: only called with such a device (see above); devices on this bus are never
    // detached (no `ca_detach`), so the softc lives as long as the kernel.
    unsafe { &*ptr::from_ref(dev).cast::<SimplebusSoftc>() }
}

/// The `struct fdt_attach_args` a bus hands its children as `aux`.
fn fdt_args<'a>(aux: *mut c_void) -> &'a FdtAttachArgs<'a> {
    // SAFETY: simplebus attaches at `fdt`, whose buses (mainbus, simplebus) hand every child
    // a `FdtAttachArgs` that lives across the call.
    unsafe { &*aux.cast::<FdtAttachArgs<'a>>() }
}

/// The `simplebus_softc` a translating tag points at (`t->bus_private`, `t->_cookie`).
fn private_softc(p: *mut c_void) -> &'static SimplebusSoftc {
    // SAFETY: `simplebus_attach` points the tags' `bus_private` and `_cookie` at its softc,
    // which lives as long as the kernel; no other tag reaches these functions.
    unsafe { &*p.cast::<SimplebusSoftc>() }
}

/// A NUL-terminated name in a buffer, up to its first NUL.
fn cstr(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]
}

/// `simplebus_match`: simplebus is a generic bus with no special casings.
pub fn simplebus_match(_parent: Option<&Device>, _cfdata: &CfMatch, aux: *mut c_void) -> i32 {
    let fa = fdt_args(aux);

    if fa.fa_node == 0 {
        return 0;
    }

    // Qualcomm GENI can mostly be treated as simple-bus.
    if OF_is_compatible(fa.fa_node, b"qcom,geni-se-qup") {
        return 1;
    }

    if !OF_is_compatible(fa.fa_node, b"simple-bus") {
        return 0;
    }

    1
}

/// `malloc(len, M_TEMP, M_WAITOK)` filled with the node's 32-bit cells of `prop`.
fn prop_cells(node: i32, prop: &[u8], len: usize) -> NonNull<u32> {
    let Some(p) = malloc(len, M_TEMP, M_WAITOK) else {
        panic(format_args!("simplebus: out of memory for {len} bytes"));
    };
    let p = p.cast::<u32>();
    // SAFETY: a fresh allocation of `len` bytes (a whole number of cells) that nothing else
    // references yet.
    let cells = unsafe { slice::from_raw_parts_mut(p.as_ptr(), len / 4) };
    OF_getpropintarray(node, prop, cells);
    p
}

/// `simplebus_attach`: copies the parent's tags with translating functions and offers every
/// child node, in three `early` passes.
pub fn simplebus_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = softc(self_);
    let fa = fdt_args(aux);
    let mut name = [0u8; 32];

    sc.sc_node.set(fa.fa_node);
    sc.sc_iot.set(Some(fa.fa_iot));
    sc.sc_dmat.set(Some(fa.fa_dmat));
    sc.sc_acells
        .set(OF_getpropint(sc.sc_node.get(), b"#address-cells", fa.fa_acells as u32) as i32);
    sc.sc_scells
        .set(OF_getpropint(sc.sc_node.get(), b"#size-cells", fa.fa_scells as u32) as i32);
    sc.sc_pacells.set(fa.fa_acells);
    sc.sc_pscells.set(fa.fa_scells);

    if OF_getprop(sc.sc_node.get(), b"name", &mut name) > 0 {
        let last = name.len() - 1;
        name[last] = 0;
        printf(format_args!(": \"{}\"", Str(cstr(&name))));
    }

    printf(format_args!("\n"));

    // SAFETY: attach time, before any child exists: nothing else reads `sc_bus` yet, and it
    // is written once.
    unsafe {
        (*sc.sc_bus.get()).write(BusSpace {
            bus_private: ptr::from_ref(sc).cast_mut().cast::<c_void>(),
            _space_map: simplebus_bs_map,
            _space_mmap: simplebus_bs_mmap,
            ..*fa.fa_iot
        });
    }

    sc.sc_rangeslen
        .set(OF_getproplen(sc.sc_node.get(), b"ranges"));
    let rangeslen = sc.sc_rangeslen.get();
    if rangeslen > 0 && rangeslen % 4 == 0 {
        sc.sc_ranges
            .set(prop_cells(sc.sc_node.get(), b"ranges", rangeslen as usize).as_ptr());
    }

    // SAFETY: as for `sc_bus`.
    unsafe {
        (*sc.sc_dma.get()).write(BusDmaTag {
            _dmamap_load_buffer: simplebus_dmamap_load_buffer,
            _dmamap_load_raw: simplebus_dmamap_load_raw,
            _cookie: ptr::from_ref(sc).cast_mut().cast::<c_void>(),
            ..*fa.fa_dmat
        });
    }

    sc.sc_dmarangeslen
        .set(OF_getproplen(sc.sc_node.get(), b"dma-ranges"));
    let dmarangeslen = sc.sc_dmarangeslen.get();
    if dmarangeslen > 0 && dmarangeslen % 4 == 0 {
        sc.sc_dmaranges
            .set(prop_cells(sc.sc_node.get(), b"dma-ranges", dmarangeslen as usize).as_ptr());
    }

    // The device tree provided by the Raspberry Pi firmware lacks a "dma-ranges" option. So
    // provide the information until that gets fixed.
    if sc.sc_dmaranges.get().is_null() {
        let node = OF_parent(sc.sc_node.get());
        if OF_is_compatible(node, b"brcm,bcm2709") {
            let len = 3 * size_of::<u32>();
            sc.sc_dmarangeslen.set(len as i32);
            let Some(p) = malloc(len, M_TEMP, M_WAITOK) else {
                panic(format_args!("simplebus: out of memory for dma-ranges"));
            };
            let p = p.cast::<u32>();
            // SAFETY: a fresh allocation of three cells, kept by the softc.
            unsafe {
                p.as_ptr().write(0xc000_0000);
                p.as_ptr().add(1).write(0x0000_0000);
                p.as_ptr().add(2).write(0x3f00_0000);
            }
            sc.sc_dmaranges.set(p.as_ptr());
        }
    }

    // Scan the whole tree.
    sc.sc_early.set(2);
    while sc.sc_early.get() >= 0 {
        let mut node = OF_child(sc.sc_node.get());
        while node != 0 {
            simplebus_attach_node(self_, node);
            node = OF_peer(node);
        }
        sc.sc_early.set(sc.sc_early.get() - 1);
    }
}

/// `simplebus_submatch`: the driver's own match, in the pass its `early` locator names.
pub fn simplebus_submatch(self_: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let Some(self_) = self_ else {
        return 0;
    };
    let sc = softc(self_);
    let cf = match_.cfdata();

    if cf.cf_loc.first().copied().unwrap_or(0) == i64::from(sc.sc_early.get()) {
        return match cf.cf_attach.ca_match {
            Some(f) => f(Some(self_), match_, aux),
            None => panic(format_args!("simplebus: no match function")),
        };
    }
    0
}

/// `simplebus_print`: names a child no driver took.
pub fn simplebus_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    let fa = fdt_args(aux);
    let mut name = [0u8; 32];

    let Some(pnp) = pnp else {
        return QUIET;
    };

    if OF_getprop(fa.fa_node, b"name", &mut name) > 0 {
        let last = name.len() - 1;
        name[last] = 0;
        printf(format_args!("\"{}\"", Str(cstr(&name))));
    } else {
        printf(format_args!("node {}", fa.fa_node as u32));
    }

    printf(format_args!(" at {}", Str(pnp)));

    UNCONF
}

/// `simplebus_attach_node`: look for a driver that wants to be attached to this node.
pub fn simplebus_attach_node(self_: &Device, node: i32) {
    let sc = softc(self_);

    if OF_getproplen(node, b"compatible") <= 0 {
        return;
    }

    if !OF_is_enabled(node) {
        return;
    }

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
        let raw = prop_cells(node, b"reg", len as usize);
        // SAFETY: `prop_cells` allocated and filled `len` bytes, freed below.
        let cells = unsafe { slice::from_raw_parts(raw.as_ptr(), len as usize / 4) };

        let nreg = (len / line) as usize;
        let Some(regs) = malloc(nreg * size_of::<FdtReg>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
            panic(format_args!("simplebus_attach_node: out of memory"));
        };
        let regs = regs.cast::<FdtReg>();

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
        let Some(p) = malloc(len as usize, M_DEVBUF, M_WAITOK) else {
            panic(format_args!("simplebus_attach_node: out of memory"));
        };
        let p = p.cast::<u32>();
        let nintr = len as usize / 4;
        // SAFETY: a fresh allocation of `nintr` cells, freed below.
        let cells = unsafe { slice::from_raw_parts_mut(p.as_ptr(), nintr) };
        OF_getpropintarray(node, b"interrupts", cells);
        intr = Some((p, nintr));
    }

    // SAFETY: `simplebus_attach` filled the tags before offering any node.
    let mut dmat: BusDmaTagT = unsafe { &*ptr::from_ref(sc.dma()) };
    if OF_getproplen(node, b"dma-coherent") >= 0 {
        let Some(p) = malloc(size_of::<BusDmaTag>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
            panic(format_args!("simplebus: out of memory for a dma tag"));
        };
        let tag = p.cast::<BusDmaTag>();
        // SAFETY: a fresh allocation of a tag's size, written once before it is shared and
        // never freed, as in C: the child keeps it.
        unsafe {
            tag.write(BusDmaTag {
                _flags: dmat._flags | BUS_DMA_COHERENT,
                ..*dmat
            });
            dmat = &*tag.as_ptr();
        }
    }

    // iommu_device_map(fa.fa_node, fa.fa_dmat): ofw_misc.c, which hands back the tag
    // unchanged when no IOMMU claims the node.
    let _ = unported!("iommu_device_map (ofw_misc.c)");

    let print: Option<CfprintT> = if sc.sc_early.get() != 0 {
        None
    } else {
        Some(simplebus_print)
    };

    let child = {
        let mut fa = FdtAttachArgs {
            fa_name: b"",
            fa_node: node,
            // SAFETY: as above; the softc lives as long as the kernel.
            fa_iot: unsafe { &*ptr::from_ref(sc.bus()) },
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
        config_found_sm(
            self_,
            ptr::from_mut(&mut fa).cast(),
            print,
            Some(simplebus_submatch),
        )
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

/// The node's `ranges`, `sc_rangeslen` bytes of cells.
fn ranges(sc: &SimplebusSoftc) -> &[u32] {
    let p = sc.sc_ranges.get();
    let len = sc.sc_rangeslen.get();
    if p.is_null() || len <= 0 {
        return &[];
    }
    // SAFETY: `simplebus_attach` allocated and filled `sc_rangeslen` bytes at `sc_ranges`,
    // never freed.
    unsafe { slice::from_raw_parts(p, len as usize / 4) }
}

/// The node's `dma-ranges`, `sc_dmarangeslen` bytes of cells.
fn dmaranges(sc: &SimplebusSoftc) -> Option<&[u32]> {
    let p = sc.sc_dmaranges.get();
    if p.is_null() {
        return None;
    }
    // SAFETY: as for `ranges`.
    Some(unsafe { slice::from_raw_parts(p, sc.sc_dmarangeslen.get() as usize / 4) })
}

/// The `ranges` entry that covers `addr` (`[addr, addr + size)` when `size` is given): the
/// child bus address translated to the parent's, `None` when no entry does. The loop of
/// `simplebus_bs_map` and `simplebus_bs_mmap`.
fn simplebus_translate(sc: &SimplebusSoftc, addr: u64, size: Option<u64>) -> Option<u64> {
    let acells = sc.sc_acells.get() as usize;
    let pacells = sc.sc_pacells.get() as usize;
    let scells = sc.sc_scells.get() as usize;
    let rone = pacells + acells + scells;

    // For each range.
    for range in ranges(sc).chunks_exact(rone.max(1)) {
        // Extract from and size, so we can see if we fit.
        let mut rfrom = u64::from(range[0]);
        if acells == 2 {
            rfrom = (rfrom << 32).wrapping_add(u64::from(range[1]));
        }
        let mut rsize = u64::from(range[acells + pacells]);
        if scells == 2 {
            rsize = (rsize << 32).wrapping_add(u64::from(range[acells + pacells + 1]));
        }

        // Try next, if we're not in the range.
        let outside = match size {
            Some(size) => addr < rfrom || addr.wrapping_add(size) > rfrom.wrapping_add(rsize),
            None => addr < rfrom || addr >= rfrom.wrapping_add(rsize),
        };
        if outside {
            continue;
        }

        // All good, extract to address and translate.
        let mut rto = u64::from(range[acells]);
        if pacells > 1 {
            rto = (rto << 32).wrapping_add(u64::from(range[acells + 1]));
        }
        // Quietly drop the "flags" part of PCI addresses.
        if pacells > 2 {
            rto = (rto << 32).wrapping_add(u64::from(range[acells + 2]));
        }

        return Some(addr.wrapping_sub(rfrom).wrapping_add(rto));
    }

    None
}

/// `simplebus_bs_map`: translate memory address if needed.
///
/// # Safety
///
/// As for `machine::bus::BusSpace::bus_space_map`.
pub unsafe fn simplebus_bs_map(
    t: &'static BusSpace,
    bpa: BusAddr,
    size: BusSize,
    flag: u32,
) -> Result<BusSpaceHandle, Errno> {
    let sc = private_softc(t.bus_private);
    let Some(iot) = sc.sc_iot.get() else {
        return Err(Errno::EINVAL);
    };

    let addr = bpa as u64;
    let parent = OF_parent(sc.sc_node.get());
    if parent == 0 {
        // SAFETY: the caller's guarantee, for the same region.
        return unsafe { (iot._space_map)(iot, bpa, size, flag) };
    }

    if sc.sc_rangeslen.get() < 0 {
        return Err(Errno::EINVAL);
    }
    if sc.sc_rangeslen.get() == 0 {
        // SAFETY: as above.
        return unsafe { (iot._space_map)(iot, bpa, size, flag) };
    }

    match simplebus_translate(sc, addr, Some(size as u64)) {
        // SAFETY: the caller's region, as the parent bus sees it.
        Some(addr) => unsafe { (iot._space_map)(iot, addr as BusAddr, size, flag) },
        None => Err(Errno::ESRCH),
    }
}

/// `simplebus_bs_mmap`: as `simplebus_bs_map`, for `mmap(2)`.
pub fn simplebus_bs_mmap(
    t: &'static BusSpace,
    bpa: BusAddr,
    off: Off,
    prot: i32,
    flags: i32,
) -> Option<Paddr> {
    let sc = private_softc(t.bus_private);
    let iot = sc.sc_iot.get()?;

    let addr = bpa as u64;
    let parent = OF_parent(sc.sc_node.get());
    if parent == 0 {
        return (iot._space_mmap)(iot, bpa, off, prot, flags);
    }

    if sc.sc_rangeslen.get() < 0 {
        // The C returns EINVAL as a paddr_t here.
        return None;
    }
    if sc.sc_rangeslen.get() == 0 {
        return (iot._space_mmap)(iot, bpa, off, prot, flags);
    }

    let addr = simplebus_translate(sc, addr, None)?;
    (iot._space_mmap)(iot, addr as BusAddr, off, prot, flags)
}

/// Translates the segments `seg` of `map` through `dma-ranges`, from the parent's address
/// space into the children's. `rlen` is the loop's count of cells left, which the C does not
/// reset between segments; it is kept so.
fn simplebus_dma_translate(
    sc: &SimplebusSoftc,
    dmaranges: &[u32],
    map: &BusDmamap,
    segs: core::ops::Range<usize>,
) {
    let acells = sc.sc_acells.get() as usize;
    let pacells = sc.sc_pacells.get() as usize;
    let scells = sc.sc_scells.get() as usize;
    let rone = pacells + acells + scells;
    let mut rlen = dmaranges.len();

    // For each segment.
    for seg in segs {
        let mut s = map.seg(seg).get();
        let addr = s.ds_addr as u64;
        let size = s.ds_len as u64;

        // For each range.
        let mut off = 0;
        while rlen >= rone && rone > 0 {
            let range = &dmaranges[off..off + rone];
            // Extract from and size, so we can see if we fit.
            let mut rfrom = u64::from(range[acells]);
            if pacells > 1 {
                rfrom = (rfrom << 32).wrapping_add(u64::from(range[acells + 1]));
            }
            // Quietly drop the "flags" part of PCI addresses.
            if pacells > 2 {
                rfrom = (rfrom << 32).wrapping_add(u64::from(range[acells + 2]));
            }

            let mut rsize = u64::from(range[acells + pacells]);
            if scells == 2 {
                rsize = (rsize << 32).wrapping_add(u64::from(range[acells + pacells + 1]));
            }

            // Try next, if we're not in the range.
            if addr < rfrom || addr.wrapping_add(size) > rfrom.wrapping_add(rsize) {
                rlen -= rone;
                off += rone;
                continue;
            }

            // All good, extract to address and translate.
            let mut rto = u64::from(range[0]);
            if acells == 2 {
                rto = (rto << 32).wrapping_add(u64::from(range[1]));
            }

            s.ds_addr = (s.ds_addr as u64).wrapping_sub(rfrom).wrapping_add(rto) as BusAddr;
            map.seg(seg).set(s);
            break;
        }
    }
}

/// `simplebus_dmamap_load_buffer`: the parent's, then the new segments translated through
/// `dma-ranges`.
///
/// # Safety
///
/// As for the parent tag's `_dmamap_load_buffer`.
#[allow(clippy::too_many_arguments)] // the C's signature
pub unsafe fn simplebus_dmamap_load_buffer(
    t: BusDmaTagT,
    map: &BusDmamap,
    buf: usize,
    buflen: BusSize,
    p: Option<&Proc>,
    flags: i32,
    lastaddrp: &mut usize,
    segp: &mut i32,
    usedp: &mut i32,
    lastbouncep: &mut bool,
    first: bool,
) -> Result<(), Errno> {
    let sc = private_softc(t._cookie);
    let Some(dmat) = sc.sc_dmat.get() else {
        return Err(Errno::EINVAL);
    };
    let lastaddr = *lastaddrp;
    let mut firstseg = *segp as usize;

    let lastlen = map.seg(firstseg).get().ds_len;
    // SAFETY: the caller's guarantee, forwarded to the parent's function.
    unsafe {
        (dmat._dmamap_load_buffer)(
            dmat,
            map,
            buf,
            buflen,
            p,
            flags,
            lastaddrp,
            segp,
            usedp,
            lastbouncep,
            first,
        )
    }?;

    let Some(dmaranges) = dmaranges(sc) else {
        return Ok(());
    };

    // If we already translated the first segment, don't do it again!
    if !first && lastaddr == map.seg(firstseg).get()._ds_paddr + lastlen {
        firstseg += 1;
    }

    simplebus_dma_translate(sc, dmaranges, map, firstseg..*segp as usize + 1);
    Ok(())
}

/// `simplebus_dmamap_load_raw`: the parent's, then the segments translated through
/// `dma-ranges`.
///
/// # Safety
///
/// As for the parent tag's `_dmamap_load_raw`.
pub unsafe fn simplebus_dmamap_load_raw(
    t: BusDmaTagT,
    map: &BusDmamap,
    segs: &[BusDmaSegment],
    size: BusSize,
    flags: i32,
) -> Result<(), Errno> {
    let sc = private_softc(t._cookie);
    let Some(dmat) = sc.sc_dmat.get() else {
        return Err(Errno::EINVAL);
    };

    // SAFETY: the caller's guarantee, forwarded to the parent's function.
    unsafe { (dmat._dmamap_load_raw)(dmat, map, segs, size, flags) }?;

    let Some(dmaranges) = dmaranges(sc) else {
        return Ok(());
    };

    simplebus_dma_translate(sc, dmaranges, map, 0..map.dm_nsegs.get() as usize);
    Ok(())
}
/* </CODE> */
