/*	$OpenBSD: gcu_var.h,v 1.1 2015/03/18 12:04:26 dlg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2009 Dariusz Swiderski <sfires@sfires.net>
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
//! The softc of gcu(4), the EP80579 (Tolapai) Global Configuration Unit
//! (`<dev/pci/gcu_var.h>`).
//!
//! Upstream: sys/dev/pci/gcu_var.h @ 3ce1f3f79392
//!
//! The whole header. em(4)'s `struct em_hw` points at it (`hw->gcu`) and `if_em_soc.c`
//! drives the GCU's MDIO registers under `mdio_mtx`. gcu(4) itself (`gcu.c`) is configured
//! only by i386 GENERIC and is not ported, so no `GcuSoftc` exists on amd64 or arm64.
//!
//! ## Deviations
//! - `#[repr(C)]` with the device first, as every softc here; `addr` and `size` are the
//!   mapped window, `tag` and `handle` the mapping.

use crate::machine::bus::{BusAddr, BusSize, BusSpaceHandle, BusSpaceTag};
use crate::sys::device::Device;
use crate::sys::mutex::Mutex;

/// `struct gcu_softc`.
#[repr(C)]
pub struct GcuSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `addr`.
    pub addr: BusAddr,
    /// `size`.
    pub size: BusSize,
    /// `tag`.
    pub tag: BusSpaceTag,
    /// `handle`.
    pub handle: BusSpaceHandle,
    /// `mdio_mtx`: serialises the MDIO command and status registers.
    pub mdio_mtx: Mutex,
}
/* </CODE> */
