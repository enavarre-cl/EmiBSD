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

/*-
 * Copyright (c) 1997 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center.
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! isadma(4): the ISA on-board DMA controller, `dev/isa/isadma.c`.
//!
//! Upstream: sys/dev/isa/isadma.c @ 3ce1f3f79392
//!
//! `isadma0 at isa?` attaches to the ISA bus, which mapped the two 8237s and their page
//! registers (`isa.c`'s `isaattach`). It records the bus for the `isadma_*` compatibility
//! interface (`isa_dev`) and creates one map per channel, 64 KB for channels 0..3 and 128 KB
//! for 4..7, on the bus's DMA tag, with their bounce pages below 16 MB allocated at once
//! (`BUS_DMA_24BIT|BUS_DMA_ALLOCNOW`, amd64's `isa_bus_dma_tag`). A transfer
//! (`isa_dmastart`) loads the channel's map with the buffer, syncs it, programs the
//! channel's mode, page, address and count and unmasks it; `isa_dmadone` checks the
//! terminal count, syncs and unloads; `isa_dmacount` reads the residue.
//!
//! ## Deviations
//! - `isadmaattach` and `isadma_ca` exist where cfg `machine_x86` is set (amd64): its map
//!   flags name `BUS_DMA_24BIT`, a flag of x86's `<machine/bus.h>` alone (isadma is only
//!   configured on machines with an ISA bus); the rest compiles everywhere.
//! - `isa_dev` is an `AtomicPtr` and `isadma_dmam[8]` an array of `AtomicPtr`s to maps,
//!   set once by `isadmaattach`; `isa_mem_head` is an `AtomicPtr` too, though the list is
//!   walked and changed as in the C, without a lock of its own (its callers hold the kernel
//!   lock).
//! - The functions that "goto lose" print what the C prints and panic with the C's
//!   message. `isa_dmamap_create`, `isa_dmamem_alloc` and `isa_dmamem_map` return
//!   `Result` (the address or mapping as the value), `isa_dmafinished` and
//!   `isa_drq_isfree` a `bool`. `isa_dmastart` and `isa_dmamem_unmap`/`_free` are
//!   `unsafe`: the buffer or memory must stay valid as `bus_dma(9)` requires.
//! - `isa_dmamap_destroy` clears the channel's `sc_dmamaps` slot (the C leaves the freed
//!   map's pointer there). A channel without a map panics where the C would dereference
//!   NULL (`isa_dmaabort`, `isa_dmadone`, `isa_dmamap_destroy`).
//! - `ISADMA_DEBUG` is not defined: its prints and the `isa_dmastart_afterload` labels are
//!   not compiled.

#[cfg(machine_x86)]
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::dev::isa::isadmareg::{
    DMA1_FFC, DMA1_MODE, DMA1_SMSK, DMA1_SR, DMA2_FFC, DMA2_MODE, DMA2_SMSK, DMA2_SR,
    DMA37MD_CASCADE, DMA37MD_LOOP, DMA37MD_READ, DMA37MD_SINGLE, DMA37MD_WRITE, DMA37SM_CLEAR,
    DMA37SM_SET, dma1_chn, dma2_chn,
};
use crate::dev::isa::isadmavar::DMAMODE_READ;
#[cfg(machine_x86)]
use crate::dev::isa::isavar::IsaAttachArgs;
use crate::dev::isa::isavar::{ISA_DRQ_ALLOC, ISA_DRQ_FREE, ISA_DRQ_ISFREE, IsaSoftc};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::{panic, printf};
#[cfg(machine_x86)]
use crate::machine::bus::BUS_DMA_ALLOCNOW;
use crate::machine::bus::{
    BUS_DMA_BUS1, BUS_DMA_NOWAIT, BUS_DMA_WAITOK, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE,
    BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE, BusAddr, BusDmaSegment, BusDmaTag, BusDmamap,
    BusSize, BusSpaceHandle, BusSpaceTag, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load,
    bus_dmamap_sync, bus_dmamap_unload, bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_map,
    bus_dmamem_unmap, bus_space_read_1, bus_space_write_1,
};
#[cfg(machine_x86)]
use crate::machine::x86::BUS_DMA_24BIT;
use crate::sys::device::Device;
#[cfg(machine_x86)]
use crate::sys::device::{CD_INDIRECT, CfMatch, Cfattach, Cfdriver, DV_DULL};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::param::NBPG;
use crate::sys::proc::Proc;
use crate::uvm::uvm_param::round_page;

/// `struct isa_mem`: one `isa_malloc` allocation, used by `isa_free`.
struct IsaMem {
    /// `isadev`.
    isadev: *const Device,
    /// `chan`.
    chan: i32,
    /// `size`.
    size: BusSize,
    /// `addr`.
    addr: BusAddr,
    /// `kva`.
    kva: *mut u8,
    /// `next`.
    next: *mut IsaMem,
}

/// `isa_dev`: the bus `isadmaattach` recorded for the `isadma_*` compatibility interface.
/// XXX ugly, but will go away soon...
pub static ISA_DEV: AtomicPtr<Device> = AtomicPtr::new(ptr::null_mut());

/// `isadma_dmam[8]`: the compatibility interface's per-channel maps.
static ISADMA_DMAM: [AtomicPtr<BusDmamap>; 8] = [const { AtomicPtr::new(ptr::null_mut()) }; 8];

/// `isa_mem_head`: used by `isa_malloc()`.
///
/// Invariant: read and written only by `isa_malloc` and `isa_free`, whose callers (ISA
/// drivers) hold the kernel lock, as in the C.
static ISA_MEM_HEAD: AtomicPtr<IsaMem> = AtomicPtr::new(ptr::null_mut());

/// `dmapageport`: high byte of DMA address is stored in this DMAPG register for the Nth DMA
/// channel.
static DMAPAGEPORT: [[usize; 4]; 2] = [[0x7, 0x3, 0x1, 0x2], [0xf, 0xb, 0x9, 0xa]];

/// `dmamode`: the 8237 mode of each `DMAMODE_*` combination.
static DMAMODE: [u8; 4] = [
    DMA37MD_READ | DMA37MD_SINGLE,
    DMA37MD_WRITE | DMA37MD_SINGLE,
    DMA37MD_READ | DMA37MD_SINGLE | DMA37MD_LOOP,
    DMA37MD_WRITE | DMA37MD_SINGLE | DMA37MD_LOOP,
];

/// `isadma_ca`.
#[cfg(machine_x86)]
pub static ISADMA_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<Device>(),
    ca_match: Some(isadmamatch),
    ca_attach: isadmaattach,
    ca_detach: None,
    ca_activate: None,
};

/// `isadma_cd`.
#[cfg(machine_x86)]
pub static ISADMA_CD: Cfdriver = Cfdriver::new(b"isadma", DV_DULL, CD_INDIRECT);

/// `isa_dev`, set by `isadmaattach`. Panics before, where the C would dereference NULL.
pub fn isa_dev() -> &'static Device {
    let dev = ISA_DEV.load(Ordering::Acquire);
    if dev.is_null() {
        panic(format_args!("isadma: no isadma0"));
    }
    // SAFETY: the isa bus `isadmaattach` recorded; autoconf never frees it.
    unsafe { &*dev }
}

/// `(struct isa_softc *)isadev`.
fn isa_sc(isadev: &Device) -> &IsaSoftc {
    // SAFETY: every caller passes the isa bus (`isa_dev`, or a driver's `ia_isa`), whose
    // softc is an `IsaSoftc`.
    unsafe { isadev.softc::<IsaSoftc>() }
}

/// The bus's i/o tag and the handle of `which` of its DMA register blocks (`sc_dma1h`,
/// `sc_dma2h`, `sc_dmapgh`), mapped by `isaattach`.
fn regs(
    sc: &IsaSoftc,
    which: &core::cell::Cell<Option<BusSpaceHandle>>,
) -> (BusSpaceTag, BusSpaceHandle) {
    match (sc.sc_iot.get(), which.get()) {
        (Some(iot), Some(h)) => (iot, h),
        _ => panic(format_args!(
            "{}: DMA registers not mapped",
            sc.sc_dev.xname()
        )),
    }
}

/// `sc->sc_dmat`, set by `isaattach` from the bus's attach arguments.
fn dmat(sc: &IsaSoftc) -> BusDmaTag {
    match sc.sc_dmat.get() {
        Some(t) => t,
        None => panic(format_args!("{}: no DMA tag", sc.sc_dev.xname())),
    }
}

/// `seg.ds_addr = addr; seg.ds_len = size;`: a segment with the public members set.
#[allow(clippy::needless_update)] // the host double's segment has no other members
fn segment(addr: BusAddr, size: BusSize) -> BusDmaSegment {
    BusDmaSegment {
        ds_addr: addr,
        ds_len: size,
        ..Default::default()
    }
}

/// `isadmamatch`.
#[cfg(machine_x86)]
pub fn isadmamatch(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: isa's children are probed with an `isa_attach_args`.
    let ia = unsafe { &mut *aux.cast::<IsaAttachArgs>() };

    // Sure we exist
    ia.set_ia_iosize(0);
    1
}

/// `isadmaattach`.
#[cfg(machine_x86)]
pub fn isadmaattach(parent: Option<&Device>, _self: &Device, _aux: *mut c_void) {
    let Some(parent) = parent else {
        panic(format_args!("isadmaattach: no parent"));
    };
    let sc = isa_sc(parent);

    // XXX ugly, but will go away soon...
    ISA_DEV.store(ptr::from_ref(parent).cast_mut(), Ordering::Release);

    for (i, dmam) in ISADMA_DMAM.iter().enumerate() {
        let sz = if i & 4 != 0 { 1 << 17 } else { 1 << 16 };
        match bus_dmamap_create(
            dmat(sc),
            sz,
            1,
            sz,
            sz,
            BUS_DMA_24BIT | BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW,
        ) {
            Ok(map) => dmam.store(ptr::from_ref(map).cast_mut(), Ordering::Release),
            Err(_) => panic(format_args!("isadmaattach: can not create DMA map")),
        }
    }

    // XXX I'd like to map the DMA ports here, see isa.c why not...

    printf(format_args!("\n"));
}

/// `isa_dmaunmask`.
fn isa_dmaunmask(sc: &IsaSoftc, chan: i32) {
    let ochan = (chan & 3) as u8;

    // set dma channel mode, and set dma channel mode
    if chan & 4 == 0 {
        let (iot, h) = regs(sc, &sc.sc_dma1h);
        bus_space_write_1(iot, h, DMA1_SMSK, ochan | DMA37SM_CLEAR);
    } else {
        let (iot, h) = regs(sc, &sc.sc_dma2h);
        bus_space_write_1(iot, h, DMA2_SMSK, ochan | DMA37SM_CLEAR);
    }
}

/// `isa_dmamask`.
fn isa_dmamask(sc: &IsaSoftc, chan: i32) {
    let ochan = (chan & 3) as u8;

    // set dma channel mode, and set dma channel mode
    if chan & 4 == 0 {
        let (iot, h) = regs(sc, &sc.sc_dma1h);
        bus_space_write_1(iot, h, DMA1_SMSK, ochan | DMA37SM_SET);
        bus_space_write_1(iot, h, DMA1_FFC, 0);
    } else {
        let (iot, h) = regs(sc, &sc.sc_dma2h);
        bus_space_write_1(iot, h, DMA2_SMSK, ochan | DMA37SM_SET);
        bus_space_write_1(iot, h, DMA2_FFC, 0);
    }
}

/// `isa_dmacascade()`: program 8237 DMA controller channel to accept external dma control
/// by a board.
pub fn isa_dmacascade(isadev: &Device, chan: i32) {
    let sc = isa_sc(isadev);
    let ochan = (chan & 3) as u8;

    if !(0..=7).contains(&chan) {
        printf(format_args!("{}: bogus drq {}\n", sc.sc_dev.xname(), chan));
        panic(format_args!("isa_dmacascade"));
    }

    if !ISA_DRQ_ISFREE(sc, chan) {
        printf(format_args!(
            "{}: DRQ {} is not free\n",
            sc.sc_dev.xname(),
            chan
        ));
        panic(format_args!("isa_dmacascade"));
    }

    ISA_DRQ_ALLOC(sc, chan);

    // set dma channel mode, and set dma channel mode
    if chan & 4 == 0 {
        let (iot, h) = regs(sc, &sc.sc_dma1h);
        bus_space_write_1(iot, h, DMA1_MODE, ochan | DMA37MD_CASCADE);
    } else {
        let (iot, h) = regs(sc, &sc.sc_dma2h);
        bus_space_write_1(iot, h, DMA2_MODE, ochan | DMA37MD_CASCADE);
    }

    isa_dmaunmask(sc, chan);
}

/// `isa_dmamap_create`: a map for `chan` on the bus's DMA tag, the channel taken.
pub fn isa_dmamap_create(
    isadev: &Device,
    chan: i32,
    size: BusSize,
    flags: i32,
) -> Result<(), Errno> {
    let sc = isa_sc(isadev);

    if !(0..=7).contains(&chan) {
        printf(format_args!("{}: bogus drq {}\n", sc.sc_dev.xname(), chan));
        panic(format_args!("isa_dmamap_create"));
    }

    let maxsize: BusSize = if chan & 4 != 0 { 1 << 17 } else { 1 << 16 };

    if size > maxsize {
        return Err(Errno::EINVAL);
    }

    if !ISA_DRQ_ISFREE(sc, chan) {
        printf(format_args!(
            "{}: drq {} is not free\n",
            sc.sc_dev.xname(),
            chan
        ));
        panic(format_args!("isa_dmamap_create"));
    }

    ISA_DRQ_ALLOC(sc, chan);

    let map = bus_dmamap_create(dmat(sc), size, 1, size, maxsize, flags)?;
    sc.sc_dmamaps[chan as usize].set(Some(map));
    Ok(())
}

/// `isa_dmamap_destroy`.
pub fn isa_dmamap_destroy(isadev: &Device, chan: i32) {
    let sc = isa_sc(isadev);

    if !(0..=7).contains(&chan) {
        printf(format_args!("{}: bogus drq {}\n", sc.sc_dev.xname(), chan));
        panic(format_args!("isa_dmamap_destroy"));
    }

    if ISA_DRQ_ISFREE(sc, chan) {
        printf(format_args!(
            "{}: drq {} is already free\n",
            sc.sc_dev.xname(),
            chan
        ));
        panic(format_args!("isa_dmamap_destroy"));
    }

    ISA_DRQ_FREE(sc, chan);

    let Some(map) = sc.sc_dmamaps[chan as usize].take() else {
        panic(format_args!(
            "isa_dmamap_destroy: no DMA map for chan {}",
            chan
        ));
    };
    // SAFETY: the channel's map `isa_dmamap_create` made; its DRQ is free now, so no
    // transfer uses it any more, and `take` left no reference to it in the softc.
    unsafe { bus_dmamap_destroy(dmat(sc), NonNull::from(map)) };
}

/// `isa_dmastart()`: program 8237 DMA controller channel and set it in motion.
///
/// # Safety
///
/// `addr` is `nbytes` bytes of memory, in `p`'s address space (the kernel's when `None`),
/// that stay allocated and mapped until the transfer is done ([`isa_dmadone`]) or aborted
/// ([`isa_dmaabort`]): `bus_dmamap_load(9)`'s contract.
pub unsafe fn isa_dmastart(
    isadev: &Device,
    chan: i32,
    addr: *mut u8,
    nbytes: BusSize,
    p: Option<&Proc>,
    flags: i32,
    busdmaflags: i32,
) -> Result<(), Errno> {
    let sc = isa_sc(isadev);
    let ochan = chan & 3;
    // __ISADMA_COMPAT
    let compat = busdmaflags & BUS_DMA_BUS1 != 0;
    let busdmaflags = busdmaflags & !BUS_DMA_BUS1;

    if !(0..=7).contains(&chan) {
        printf(format_args!("{}: bogus drq {}\n", sc.sc_dev.xname(), chan));
        panic(format_args!("isa_dmastart"));
    }

    if chan & 4 != 0 {
        if nbytes > (1 << 17) || nbytes & 1 != 0 || addr as usize & 1 != 0 {
            printf(format_args!(
                "{}: drq {}, nbytes 0x{:x}, addr {:p}\n",
                sc.sc_dev.xname(),
                chan,
                nbytes,
                addr
            ));
            panic(format_args!("isa_dmastart"));
        }
    } else if nbytes > (1 << 16) {
        printf(format_args!(
            "{}: drq {}, nbytes 0x{:x}\n",
            sc.sc_dev.xname(),
            chan,
            nbytes
        ));
        panic(format_args!("isa_dmastart"));
    }

    let dmam = match sc.sc_dmamaps[chan as usize].get() {
        Some(dmam) => dmam,
        None => {
            let compat_map = ISADMA_DMAM[chan as usize].load(Ordering::Acquire);
            if !compat || compat_map.is_null() {
                panic(format_args!("isa_dmastart: no DMA map for chan {}", chan));
            }
            // SAFETY: a map `isadmaattach` created and never destroys.
            let dmam: &'static BusDmamap = unsafe { &*compat_map };
            sc.sc_dmamaps[chan as usize].set(Some(dmam));
            dmam
        }
    };

    // SAFETY: the caller's contract.
    unsafe { bus_dmamap_load(dmat(sc), dmam, addr, nbytes, p, busdmaflags) }?;

    if flags & DMAMODE_READ != 0 {
        bus_dmamap_sync(
            dmat(sc),
            dmam,
            0,
            dmam.dm_mapsize.get(),
            BUS_DMASYNC_PREREAD,
        );
        sc.sc_dmareads.set(sc.sc_dmareads.get() | (1 << chan));
    } else {
        bus_dmamap_sync(
            dmat(sc),
            dmam,
            0,
            dmam.dm_mapsize.get(),
            BUS_DMASYNC_PREWRITE,
        );
        sc.sc_dmareads.set(sc.sc_dmareads.get() & !(1 << chan));
    }

    let mut dmaaddr = dmam.dm_segs()[0].get().ds_addr;

    sc.sc_dmalength[chan as usize].set(nbytes);

    isa_dmamask(sc, chan);
    sc.sc_dmafinished
        .set(sc.sc_dmafinished.get() & !(1 << chan));

    let (iot, pgh) = regs(sc, &sc.sc_dmapgh);
    let mode = (ochan as u8) | DMAMODE[flags as usize];
    if chan & 4 == 0 {
        let (_, h) = regs(sc, &sc.sc_dma1h);
        // set dma channel mode
        bus_space_write_1(iot, h, DMA1_MODE, mode);

        // send start address
        let waport = dma1_chn(ochan as usize);
        bus_space_write_1(
            iot,
            pgh,
            DMAPAGEPORT[0][ochan as usize],
            ((dmaaddr >> 16) & 0xff) as u8,
        );
        bus_space_write_1(iot, h, waport, (dmaaddr & 0xff) as u8);
        bus_space_write_1(iot, h, waport, ((dmaaddr >> 8) & 0xff) as u8);

        // send count
        let nbytes = nbytes.wrapping_sub(1);
        bus_space_write_1(iot, h, waport + 1, (nbytes & 0xff) as u8);
        bus_space_write_1(iot, h, waport + 1, ((nbytes >> 8) & 0xff) as u8);
    } else {
        let (_, h) = regs(sc, &sc.sc_dma2h);
        // set dma channel mode
        bus_space_write_1(iot, h, DMA2_MODE, mode);

        // send start address
        let waport = dma2_chn(ochan as usize);
        bus_space_write_1(
            iot,
            pgh,
            DMAPAGEPORT[1][ochan as usize],
            ((dmaaddr >> 16) & 0xff) as u8,
        );
        dmaaddr >>= 1;
        bus_space_write_1(iot, h, waport, (dmaaddr & 0xff) as u8);
        bus_space_write_1(iot, h, waport, ((dmaaddr >> 8) & 0xff) as u8);

        // send count
        let nbytes = (nbytes >> 1).wrapping_sub(1);
        bus_space_write_1(iot, h, waport + 2, (nbytes & 0xff) as u8);
        bus_space_write_1(iot, h, waport + 2, ((nbytes >> 8) & 0xff) as u8);
    }

    isa_dmaunmask(sc, chan);
    Ok(())
}

/// `isa_dmaabort`.
pub fn isa_dmaabort(isadev: &Device, chan: i32) {
    let sc = isa_sc(isadev);

    if !(0..=7).contains(&chan) {
        panic(format_args!(
            "isa_dmaabort: {}: bogus drq {}",
            sc.sc_dev.xname(),
            chan
        ));
    }

    isa_dmamask(sc, chan);
    match sc.sc_dmamaps[chan as usize].get() {
        Some(map) => bus_dmamap_unload(dmat(sc), map),
        None => panic(format_args!("isa_dmaabort: no DMA map for chan {}", chan)),
    }
    sc.sc_dmareads.set(sc.sc_dmareads.get() & !(1 << chan));
}

/// `isa_dmacount`: the bytes the channel has left to transfer.
pub fn isa_dmacount(isadev: &Device, chan: i32) -> BusSize {
    let sc = isa_sc(isadev);
    let ochan = (chan & 3) as usize;

    if !(0..=7).contains(&chan) {
        panic(format_args!(
            "isa_dmacount: {}: bogus drq {}",
            sc.sc_dev.xname(),
            chan
        ));
    }

    isa_dmamask(sc, chan);

    // We have to shift the byte count by 1. If we're in auto-initialize mode, the count may
    // have wrapped around to the initial value. We can't use the TC bit to check for this
    // case, so instead we compare against the original byte count. If we're not in
    // auto-initialize mode, then the count will wrap to -1, so we also handle that case.
    let mut nbytes: BusSize;
    if chan & 4 == 0 {
        let (iot, h) = regs(sc, &sc.sc_dma1h);
        let waport = dma1_chn(ochan);
        nbytes = BusSize::from(bus_space_read_1(iot, h, waport + 1)) + 1;
        nbytes += BusSize::from(bus_space_read_1(iot, h, waport + 1)) << 8;
        nbytes &= 0xffff;
    } else {
        let (iot, h) = regs(sc, &sc.sc_dma2h);
        let waport = dma2_chn(ochan);
        nbytes = BusSize::from(bus_space_read_1(iot, h, waport + 2)) + 1;
        nbytes += BusSize::from(bus_space_read_1(iot, h, waport + 2)) << 8;
        nbytes <<= 1;
        nbytes &= 0x1ffff;
    }

    if nbytes == sc.sc_dmalength[chan as usize].get() {
        nbytes = 0;
    }

    isa_dmaunmask(sc, chan);
    nbytes
}

/// `isa_dmafinished`: whether the channel reached its terminal count.
pub fn isa_dmafinished(isadev: &Device, chan: i32) -> bool {
    let sc = isa_sc(isadev);

    if !(0..=7).contains(&chan) {
        panic(format_args!(
            "isa_dmafinished: {}: bogus drq {}",
            sc.sc_dev.xname(),
            chan
        ));
    }

    // check that the terminal count was reached
    if chan & 4 == 0 {
        let (iot, h) = regs(sc, &sc.sc_dma1h);
        let sr = i32::from(bus_space_read_1(iot, h, DMA1_SR)) & 0x0f;
        sc.sc_dmafinished.set(sc.sc_dmafinished.get() | sr);
    } else {
        let (iot, h) = regs(sc, &sc.sc_dma2h);
        let sr = i32::from(bus_space_read_1(iot, h, DMA2_SR)) & 0x0f;
        sc.sc_dmafinished.set(sc.sc_dmafinished.get() | (sr << 4));
    }

    sc.sc_dmafinished.get() & (1 << chan) != 0
}

/// `isa_dmadone`.
pub fn isa_dmadone(isadev: &Device, chan: i32) {
    let sc = isa_sc(isadev);

    if !(0..=7).contains(&chan) {
        panic(format_args!(
            "isa_dmadone: {}: bogus drq {}",
            sc.sc_dev.xname(),
            chan
        ));
    }

    let Some(dmam) = sc.sc_dmamaps[chan as usize].get() else {
        panic(format_args!("isa_dmadone: no DMA map for chan {}", chan));
    };

    isa_dmamask(sc, chan);

    if !isa_dmafinished(isadev, chan) {
        printf(format_args!(
            "{}: isa_dmadone: channel {} not finished\n",
            sc.sc_dev.xname(),
            chan
        ));
    }

    bus_dmamap_sync(
        dmat(sc),
        dmam,
        0,
        dmam.dm_mapsize.get(),
        if sc.sc_dmareads.get() & (1 << chan) != 0 {
            BUS_DMASYNC_POSTREAD
        } else {
            BUS_DMASYNC_POSTWRITE
        },
    );

    bus_dmamap_unload(dmat(sc), dmam);
    sc.sc_dmareads.set(sc.sc_dmareads.get() & !(1 << chan));
}

/// `isa_dmamem_alloc`: DMA-safe memory for `chan`, its bus address as the value.
pub fn isa_dmamem_alloc(
    isadev: &Device,
    chan: i32,
    size: BusSize,
    flags: i32,
) -> Result<BusAddr, Errno> {
    let sc = isa_sc(isadev);

    if !(0..=7).contains(&chan) {
        panic(format_args!(
            "isa_dmamem_alloc: {}: bogus drq {}",
            sc.sc_dev.xname(),
            chan
        ));
    }

    let boundary: BusSize = if chan & 4 != 0 { 1 << 17 } else { 1 << 16 };

    let size = round_page(size);

    let mut seg = [BusDmaSegment::default()];
    bus_dmamem_alloc(dmat(sc), size, NBPG, boundary, &mut seg, flags)?;

    Ok(seg[0].ds_addr)
}

/// `isa_dmamem_free`.
///
/// # Safety
///
/// `addr` and `size` are an allocation of [`isa_dmamem_alloc`], unmapped and unused.
pub unsafe fn isa_dmamem_free(isadev: &Device, chan: i32, addr: BusAddr, size: BusSize) {
    let sc = isa_sc(isadev);

    if !(0..=7).contains(&chan) {
        panic(format_args!(
            "isa_dmamem_free: {}: bogus drq {}",
            sc.sc_dev.xname(),
            chan
        ));
    }

    let seg = segment(addr, size);

    // SAFETY: the caller's guarantee.
    unsafe { bus_dmamem_free(dmat(sc), &[seg]) };
}

/// `isa_dmamem_map`: maps DMA memory of [`isa_dmamem_alloc`] in the kernel.
pub fn isa_dmamem_map(
    isadev: &Device,
    chan: i32,
    addr: BusAddr,
    size: BusSize,
    flags: i32,
) -> Result<NonNull<u8>, Errno> {
    let sc = isa_sc(isadev);

    if !(0..=7).contains(&chan) {
        panic(format_args!(
            "isa_dmamem_map: {}: bogus drq {}",
            sc.sc_dev.xname(),
            chan
        ));
    }

    let mut seg = segment(addr, size);

    bus_dmamem_map(dmat(sc), core::slice::from_mut(&mut seg), size, flags)
}

/// `isa_dmamem_unmap`.
///
/// # Safety
///
/// `kva` and `size` are a mapping of [`isa_dmamem_map`] nobody uses any more.
pub unsafe fn isa_dmamem_unmap(isadev: &Device, chan: i32, kva: NonNull<u8>, size: usize) {
    let sc = isa_sc(isadev);

    if !(0..=7).contains(&chan) {
        panic(format_args!(
            "isa_dmamem_unmap: {}: bogus drq {}",
            sc.sc_dev.xname(),
            chan
        ));
    }

    // SAFETY: the caller's guarantee.
    unsafe { bus_dmamem_unmap(dmat(sc), kva, size) };
}

/// `isa_drq_isfree`.
pub fn isa_drq_isfree(isadev: &Device, chan: i32) -> bool {
    let sc = isa_sc(isadev);
    if !(0..=7).contains(&chan) {
        panic(format_args!(
            "isa_drq_isfree: {}: bogus drq {}",
            sc.sc_dev.xname(),
            chan
        ));
    }
    ISA_DRQ_ISFREE(sc, chan)
}

/// `isa_malloc`: `size` bytes of DMA-safe memory for `chan`, mapped; `None` on failure.
pub fn isa_malloc(
    isadev: &Device,
    chan: i32,
    size: usize,
    pool: i32,
    flags: i32,
) -> Option<NonNull<u8>> {
    let bflags = if flags & M_NOWAIT != 0 {
        BUS_DMA_NOWAIT
    } else {
        BUS_DMA_WAITOK
    };

    let addr = isa_dmamem_alloc(isadev, chan, size, bflags).ok()?;
    let kva = match isa_dmamem_map(isadev, chan, addr, size, bflags) {
        Ok(kva) => kva,
        Err(_) => {
            // SAFETY: the allocation just made, never mapped.
            unsafe { isa_dmamem_free(isadev, chan, addr, size) };
            return None;
        }
    };
    let Some(m) = malloc(size_of::<IsaMem>(), pool, flags) else {
        // SAFETY: the mapping and allocation just made, handed to nobody.
        unsafe {
            isa_dmamem_unmap(isadev, chan, kva, size);
            isa_dmamem_free(isadev, chan, addr, size);
        }
        return None;
    };
    let m = m.cast::<IsaMem>();
    // SAFETY: `m` is a fresh allocation the size of an `IsaMem`, written whole before it
    // is linked (under the kernel lock: `ISA_MEM_HEAD`'s invariant).
    unsafe {
        m.as_ptr().write(IsaMem {
            isadev: isadev as *const Device,
            chan,
            size,
            addr,
            kva: kva.as_ptr(),
            next: ISA_MEM_HEAD.load(Ordering::Relaxed),
        });
    }
    ISA_MEM_HEAD.store(m.as_ptr(), Ordering::Relaxed);
    Some(kva)
}

/// `isa_free`: gives back memory of [`isa_malloc`].
///
/// # Safety
///
/// Nobody uses the memory at `addr` any more.
pub unsafe fn isa_free(addr: *mut u8, pool: i32) {
    // SAFETY: `ISA_MEM_HEAD`'s invariant (the kernel lock); the list holds the `IsaMem`s
    // `isa_malloc` linked, each alive until unlinked here.
    unsafe {
        let mut mp: *mut *mut IsaMem = ISA_MEM_HEAD.as_ptr();
        while !(*mp).is_null() && (**mp).kva != addr {
            mp = &raw mut (**mp).next;
        }
        let m = *mp;
        if m.is_null() {
            printf(format_args!("isa_free: freeing unallocated memory\n"));
            return;
        }
        *mp = (*m).next;
        let isadev = &*(*m).isadev;
        if let Some(kva) = NonNull::new(addr) {
            isa_dmamem_unmap(isadev, (*m).chan, kva, (*m).size);
        }
        isa_dmamem_free(isadev, (*m).chan, (*m).addr, (*m).size);
        free(NonNull::new_unchecked(m).cast(), pool, 0);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_ports_and_modes() {
        // Channel 2 (the floppy's) has its page register at 0x81 (IO_DMAPG + 1).
        assert_eq!(DMAPAGEPORT[0][2], 0x1);
        // A read from the device into memory is the 8237's "write" transfer.
        assert_eq!(
            DMAMODE[DMAMODE_READ as usize],
            DMA37MD_WRITE | DMA37MD_SINGLE
        );
        assert_eq!(DMAMODE[0], DMA37MD_READ | DMA37MD_SINGLE);
    }
}
/* </TESTS> */
