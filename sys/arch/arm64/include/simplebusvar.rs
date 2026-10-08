/* $OpenBSD: simplebusvar.h,v 1.2 2026/06/22 07:54:19 deraadt Exp $ */
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
//! arm64 `<machine/simplebusvar.h>`: the softc of a `simple-bus` node, which the interrupt
//! controllers that have children (`ampintc`'s GICv2m frames) embed first.
//!
//! Upstream: sys/arch/arm64/include/simplebusvar.h @ 3ce1f3f79392
//!
//! `extern void simplebus_attach(...)` is `arch/arm64/dev/simplebus.rs`.
//!
//! ## Deviations
//! - `sc_bus` and `sc_dma`, the copies of the parent's tags that the children get, are
//!   `MaybeUninit` behind an `UnsafeCell`: the softc is zeroed memory, which is no valid table
//!   of function pointers, and `simplebus_attach` writes them once before any child sees
//!   them ([`SimplebusSoftc::bus`], [`SimplebusSoftc::dma`]).
//! - The tags and the range arrays are `Cell`s (`Option` for the tags, raw pointers for the
//!   `malloc`ed `int *` arrays), written by `simplebus_attach`.

use core::cell::{Cell, UnsafeCell};
use core::mem::MaybeUninit;

use crate::arch::arm64::include::bus::{BusDmaTag, BusDmaTagT, BusSpace};
use crate::sys::device::{Device, Softc};

/// `struct simplebus_softc`.
#[repr(C)]
pub struct SimplebusSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_node`.
    pub sc_node: Cell<i32>,
    /// `sc_iot`: the parent's bus space.
    pub sc_iot: Cell<Option<&'static BusSpace>>,
    /// `sc_dmat`: the parent's DMA tag.
    pub sc_dmat: Cell<Option<BusDmaTagT>>,
    /// `sc_acells`: the children's `#address-cells`.
    pub sc_acells: Cell<i32>,
    /// `sc_scells`: the children's `#size-cells`.
    pub sc_scells: Cell<i32>,
    /// `sc_pacells`: the parent's `#address-cells`.
    pub sc_pacells: Cell<i32>,
    /// `sc_pscells`: the parent's `#size-cells`.
    pub sc_pscells: Cell<i32>,
    /// `sc_bus`: the children's bus space, a copy of `sc_iot` translating through `ranges`.
    pub sc_bus: UnsafeCell<MaybeUninit<BusSpace>>,
    /// `sc_dma`: the children's DMA tag, a copy of `sc_dmat` translating through
    /// `dma-ranges`.
    pub sc_dma: UnsafeCell<MaybeUninit<BusDmaTag>>,
    /// `sc_ranges`: the node's `ranges` cells, `sc_rangeslen` bytes (`malloc`ed, `M_TEMP`).
    pub sc_ranges: Cell<*mut u32>,
    /// `sc_rangeslen`: in bytes; negative when the node has no `ranges`.
    pub sc_rangeslen: Cell<i32>,
    /// `sc_dmaranges`: the node's `dma-ranges` cells, `sc_dmarangeslen` bytes.
    pub sc_dmaranges: Cell<*mut u32>,
    /// `sc_dmarangeslen`.
    pub sc_dmarangeslen: Cell<i32>,
    /// `sc_early`: the `early` locator of the pass under way.
    pub sc_early: Cell<i32>,
    /// `sc_early_nodes`: the nodes attached in an early pass, 0-terminated.
    pub sc_early_nodes: [Cell<i32>; 64],
}

impl SimplebusSoftc {
    /// `&sc->sc_bus`, once `simplebus_attach` has filled it.
    ///
    /// # Safety
    ///
    /// `simplebus_attach` has written `sc_bus`.
    pub unsafe fn bus(&self) -> &BusSpace {
        // SAFETY: the caller's guarantee; it is never written again.
        unsafe { (*self.sc_bus.get()).assume_init_ref() }
    }

    /// `&sc->sc_dma`, once `simplebus_attach` has filled it.
    ///
    /// # Safety
    ///
    /// `simplebus_attach` has written `sc_dma`.
    pub unsafe fn dma(&self) -> &BusDmaTag {
        // SAFETY: the caller's guarantee; it is never written again.
        unsafe { (*self.sc_dma.get()).assume_init_ref() }
    }
}

// SAFETY: `#[repr(C)]` with the device first; every other member is a `Cell` of an integer,
// a raw pointer or an `Option` of a reference, or `MaybeUninit`: all valid as zero bits.
unsafe impl Softc for SimplebusSoftc {}
/* </CODE> */
