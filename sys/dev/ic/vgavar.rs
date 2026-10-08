/* $OpenBSD: vgavar.h,v 1.13 2015/07/26 03:17:07 miod Exp $ */
/* $NetBSD: vgavar.h,v 1.4 2000/06/17 07:11:50 soda Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 1995, 1996 Carnegie-Mellon University.
 * All rights reserved.
 *
 * Author: Chris G. Demetriou
 *
 * Permission to use, copy, modify and distribute this software and
 * its documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND
 * FOR ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/ic/vgavar.h>`: the state of a VGA: its bus handles ([`VgaHandle`]), its screens
//! ([`Vgascreen`]) and the configuration that holds them ([`VgaConfig`]), with the accessors
//! of the VGA's indexed register files.
//!
//! Upstream: sys/dev/ic/vgavar.h @ 3ce1f3f79392
//!
//! The attribute controller, the timing sequencer and the graphics data controller are
//! each an index port and a data port in the VGA's I/O range at 0x3c0; the 6845 CRT
//! controller is reached through `pcdisplayvar.h`'s handle. The `vga_*_read`/`vga_*_write`
//! macros take a register by its field name in the `vgareg.h` structures, as the C's do.
//!
//! ## Deviations
//! - `struct vgafont` is opaque here, as in C; it is `vga.rs`'s [`Vgafont`].
//! - [`VgaHandle`] is plain data, set once by `vga_init` and only read afterwards; the
//!   `vh_iot`/`vh_memt`/`vh_ioh_6845`/`vh_memh` macros are its methods.
//! - [`Vgascreen`] and the mutable members of [`VgaConfig`] are `Cell`s: the emulops,
//!   accessops and the switch timeout reach them through the C's `void *` cookies, at
//!   `spltty()` under the kernel lock as in C. The palette is 768 byte cells.
//! - The `__alpha__` members (`custom_scr`, `custom_scrlist`, `custom_list`) are not here:
//!   no alpha port, as on every other architecture.
//! - `vga_pci_ioctl`'s prototype (`NVGA_PCI > 0`) is `vga_pci.rs`'s function.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::dev::ic::pcdisplayvar::{PcdisplayHandle, Pcdisplayscreen};
use crate::dev::ic::vga::Vgafont;
use crate::dev::ic::vgareg::{
    VGA_ATC_DATAR, VGA_ATC_DATAW, VGA_ATC_INDEX, VGA_GDC_DATA, VGA_GDC_INDEX, VGA_TS_DATA,
    VGA_TS_INDEX,
};
use crate::dev::wscons::wsdisplayvar::{ShowScreenCb, WsdisplayMmapFn, WsscreenDescr};
use crate::machine::bus::{BusSpaceHandle, BusSpaceTag, bus_space_read_1, bus_space_write_1};
use crate::sys::device::Device;
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::timeout::Timeout;

/// `VGA_MAXFONT`.
pub const VGA_MAXFONT: usize = 8;

/// `struct vga_handle`.
#[derive(Clone, Copy)]
pub struct VgaHandle {
    /// `vh_ph`: the 6845 and text memory handles.
    pub vh_ph: PcdisplayHandle,
    /// `vh_ioh_vga`: the VGA's I/O ports at 0x3c0.
    pub vh_ioh_vga: BusSpaceHandle,
    /// `vh_allmemh`: the whole 128 KB window at 0xa0000.
    pub vh_allmemh: BusSpaceHandle,
    /// `vh_mono`: monochrome (the 6845 at 0x3b0, text at 0xb0000).
    pub vh_mono: i32,
}

impl VgaHandle {
    /// `vh_iot`.
    pub fn vh_iot(&self) -> BusSpaceTag {
        self.vh_ph.ph_iot
    }

    /// `vh_memt`.
    pub fn vh_memt(&self) -> BusSpaceTag {
        self.vh_ph.ph_memt
    }

    /// `vh_ioh_6845`.
    pub fn vh_ioh_6845(&self) -> BusSpaceHandle {
        self.vh_ph.ph_ioh_6845
    }

    /// `vh_memh`: the text memory (32 KB at 0xb8000, or 0xb0000 when mono).
    pub fn vh_memh(&self) -> BusSpaceHandle {
        self.vh_ph.ph_memh
    }
}

/// `struct vgascreen`: a screen, the cookie of `vga.c`'s emulops. `#[repr(C)]` with the
/// pcdisplay screen first, which the `pcdisplay_*` emulops see.
#[repr(C)]
pub struct Vgascreen {
    /// `pcs`.
    pub pcs: Pcdisplayscreen,
    /// `next`: `LIST_ENTRY(vgascreen)` on the configuration's `screens`.
    pub next: ListEntry<Vgascreen>,

    // videostate
    /// `cfg`.
    pub cfg: Cell<*const VgaConfig>,
    // font data
    /// `fontset1`.
    pub fontset1: Cell<*const Vgafont>,
    /// `fontset2`.
    pub fontset2: Cell<*const Vgafont>,

    /// `mindispoffset`.
    pub mindispoffset: Cell<i32>,
    /// `maxdispoffset`.
    pub maxdispoffset: Cell<i32>,
    /// `vga_rollover`.
    pub vga_rollover: Cell<i32>,
}

impl Vgascreen {
    /// A screen with every member zero.
    pub const fn new() -> Self {
        Self {
            pcs: Pcdisplayscreen::new(),
            next: ListEntry::new(),
            cfg: Cell::new(ptr::null()),
            fontset1: Cell::new(ptr::null()),
            fontset2: Cell::new(ptr::null()),
            mindispoffset: Cell::new(0),
            maxdispoffset: Cell::new(0),
            vga_rollover: Cell::new(0),
        }
    }

    /// `scr->cfg`.
    pub fn cfg(&self) -> &VgaConfig {
        // SAFETY: `vga_init_screen` points `cfg` at the configuration before the screen is
        // handed out, and a configuration outlives its screens.
        unsafe { &*self.cfg.get() }
    }

    /// `scr->fontset1`.
    pub fn fontset1(&self) -> Option<&Vgafont> {
        // SAFETY: fonts are `vga.c`'s builtin one or ones `vga_load_font` allocated, never
        // freed.
        unsafe { self.fontset1.get().as_ref() }
    }

    /// `scr->fontset2`.
    pub fn fontset2(&self) -> Option<&Vgafont> {
        // SAFETY: as in `fontset1`.
        unsafe { self.fontset2.get().as_ref() }
    }
}

impl Default for Vgascreen {
    fn default() -> Self {
        Self::new()
    }
}

crate::queue_adapter!(
    /// `LIST_ENTRY(vgascreen) next`: a configuration's `screens`.
    pub VgascreenList: Vgascreen, next => ListEntry<Vgascreen>
);

/// `struct vga_config`.
pub struct VgaConfig {
    /// `hdl`.
    pub hdl: VgaHandle,

    /// `vc_softc`.
    pub vc_softc: Cell<*const Device>,
    /// `vc_type`: `WSDISPLAY_TYPE_*`.
    pub vc_type: Cell<i32>,
    /// `nscreens`.
    pub nscreens: Cell<i32>,
    /// `screens`.
    pub screens: ListHead<VgascreenList>,
    /// `active`: current display.
    pub active: Cell<*const Vgascreen>,
    /// `currenttype`.
    pub currenttype: Cell<*const WsscreenDescr>,
    /// `currentfontset1`.
    pub currentfontset1: Cell<i32>,
    /// `currentfontset2`.
    pub currentfontset2: Cell<i32>,

    /// `vc_fonts[VGA_MAXFONT]`.
    pub vc_fonts: [Cell<*const Vgafont>; VGA_MAXFONT],
    /// `vc_palette[256 * 3]`.
    pub vc_palette: [Cell<u8>; 256 * 3],

    /// `wantedscreen`.
    pub wantedscreen: Cell<*const Vgascreen>,
    /// `switchcb`.
    pub switchcb: Cell<Option<ShowScreenCb>>,
    /// `switchcbarg`.
    pub switchcbarg: Cell<*mut c_void>,

    /// `vc_mmap`.
    pub vc_mmap: Cell<Option<WsdisplayMmapFn>>,

    /// `vc_switch_timeout`.
    pub vc_switch_timeout: Timeout,
}

impl VgaConfig {
    /// A configuration over `hdl` with every other member zero (the C's `M_ZERO` or static
    /// one, before `vga_init` fills it).
    pub fn new(hdl: VgaHandle) -> Self {
        Self {
            hdl,
            vc_softc: Cell::new(ptr::null()),
            vc_type: Cell::new(0),
            nscreens: Cell::new(0),
            screens: ListHead::new(),
            active: Cell::new(ptr::null()),
            currenttype: Cell::new(ptr::null()),
            currentfontset1: Cell::new(0),
            currentfontset2: Cell::new(0),
            vc_fonts: [const { Cell::new(ptr::null()) }; VGA_MAXFONT],
            vc_palette: [const { Cell::new(0) }; 256 * 3],
            wantedscreen: Cell::new(ptr::null()),
            switchcb: Cell::new(None),
            switchcbarg: Cell::new(ptr::null_mut()),
            vc_mmap: Cell::new(None),
            vc_switch_timeout: Timeout::zeroed(),
        }
    }

    /// `vc->active`.
    pub fn active(&self) -> Option<&Vgascreen> {
        // SAFETY: `active` is one of this configuration's screens or NULL; a screen is
        // unset as active before it is freed (`vga_free_screen`).
        unsafe { self.active.get().as_ref() }
    }

    /// `vc->vc_fonts[slot]`.
    pub fn font(&self, slot: usize) -> Option<&Vgafont> {
        // SAFETY: as in `Vgascreen::fontset1`.
        self.vc_fonts
            .get(slot)
            .and_then(|f| unsafe { f.get().as_ref() })
    }
}

/// `vga_raw_read(vh, reg)`: port `reg` of the VGA's I/O range.
#[inline]
pub fn vga_raw_read(vh: &VgaHandle, reg: usize) -> u8 {
    bus_space_read_1(vh.vh_iot(), vh.vh_ioh_vga, reg)
}

/// `vga_raw_write(vh, reg, value)`.
#[inline]
pub fn vga_raw_write(vh: &VgaHandle, reg: usize, value: u8) {
    bus_space_write_1(vh.vh_iot(), vh.vh_ioh_vga, reg, value)
}

/// `vga_enable(vh)`: set the attribute controller's palette address source bit, which
/// turns the display back on.
#[inline]
pub fn vga_enable(vh: &VgaHandle) {
    vga_raw_write(vh, 0, 0x20);
}

/// `_vga_attr_read`: attribute controller register `reg`.
#[inline]
pub fn _vga_attr_read(vh: &VgaHandle, reg: usize) -> u8 {
    // reset state
    let _ = bus_space_read_1(vh.vh_iot(), vh.vh_ioh_6845(), 10);

    vga_raw_write(vh, VGA_ATC_INDEX, reg as u8);
    let res = vga_raw_read(vh, VGA_ATC_DATAR);

    // reset state XXX unneeded?
    let _ = bus_space_read_1(vh.vh_iot(), vh.vh_ioh_6845(), 10);

    vga_enable(vh);

    res
}

/// `_vga_attr_write`.
#[inline]
pub fn _vga_attr_write(vh: &VgaHandle, reg: usize, val: u8) {
    // reset state
    let _ = bus_space_read_1(vh.vh_iot(), vh.vh_ioh_6845(), 10);

    vga_raw_write(vh, VGA_ATC_INDEX, reg as u8);
    vga_raw_write(vh, VGA_ATC_DATAW, val);

    // reset state XXX unneeded?
    let _ = bus_space_read_1(vh.vh_iot(), vh.vh_ioh_6845(), 10);

    vga_enable(vh);
}

/// `_vga_ts_read`: timing sequencer register `reg`.
#[inline]
pub fn _vga_ts_read(vh: &VgaHandle, reg: usize) -> u8 {
    vga_raw_write(vh, VGA_TS_INDEX, reg as u8);
    vga_raw_read(vh, VGA_TS_DATA)
}

/// `_vga_ts_write`.
#[inline]
pub fn _vga_ts_write(vh: &VgaHandle, reg: usize, val: u8) {
    vga_raw_write(vh, VGA_TS_INDEX, reg as u8);
    vga_raw_write(vh, VGA_TS_DATA, val);
}

/// `_vga_gdc_read`: graphics data controller register `reg`.
#[inline]
pub fn _vga_gdc_read(vh: &VgaHandle, reg: usize) -> u8 {
    vga_raw_write(vh, VGA_GDC_INDEX, reg as u8);
    vga_raw_read(vh, VGA_GDC_DATA)
}

/// `_vga_gdc_write`.
#[inline]
pub fn _vga_gdc_write(vh: &VgaHandle, reg: usize, val: u8) {
    vga_raw_write(vh, VGA_GDC_INDEX, reg as u8);
    vga_raw_write(vh, VGA_GDC_DATA, val);
}

/// `vga_attr_read(vh, reg)`: the attribute register named by a field of `struct
/// reg_vgaattr`.
#[allow(unused_macros)] // the C defines it; no ported code reads one by name
macro_rules! vga_attr_read {
    ($vh:expr, $reg:ident) => {
        $crate::dev::ic::vgavar::_vga_attr_read(
            $vh,
            ::core::mem::offset_of!($crate::dev::ic::vgareg::RegVgaattr, $reg),
        )
    };
}
#[allow(unused_imports)] // as above
pub(crate) use vga_attr_read;

/// `vga_attr_write(vh, reg, val)`.
macro_rules! vga_attr_write {
    ($vh:expr, $reg:ident, $val:expr) => {
        $crate::dev::ic::vgavar::_vga_attr_write(
            $vh,
            ::core::mem::offset_of!($crate::dev::ic::vgareg::RegVgaattr, $reg),
            $val,
        )
    };
}
pub(crate) use vga_attr_write;

/// `vga_ts_read(vh, reg)`: the sequencer register named by a field of `struct reg_vgats`.
macro_rules! vga_ts_read {
    ($vh:expr, $reg:ident) => {
        $crate::dev::ic::vgavar::_vga_ts_read(
            $vh,
            ::core::mem::offset_of!($crate::dev::ic::vgareg::RegVgats, $reg),
        )
    };
}
pub(crate) use vga_ts_read;

/// `vga_ts_write(vh, reg, val)`.
macro_rules! vga_ts_write {
    ($vh:expr, $reg:ident, $val:expr) => {
        $crate::dev::ic::vgavar::_vga_ts_write(
            $vh,
            ::core::mem::offset_of!($crate::dev::ic::vgareg::RegVgats, $reg),
            $val,
        )
    };
}
pub(crate) use vga_ts_write;

/// `vga_gdc_read(vh, reg)`: the graphics controller register named by a field of `struct
/// reg_vgagdc`.
#[allow(unused_macros)] // the C defines it; no ported code reads one by name
macro_rules! vga_gdc_read {
    ($vh:expr, $reg:ident) => {
        $crate::dev::ic::vgavar::_vga_gdc_read(
            $vh,
            ::core::mem::offset_of!($crate::dev::ic::vgareg::RegVgagdc, $reg),
        )
    };
}
#[allow(unused_imports)] // as above
pub(crate) use vga_gdc_read;

/// `vga_gdc_write(vh, reg, val)`.
macro_rules! vga_gdc_write {
    ($vh:expr, $reg:ident, $val:expr) => {
        $crate::dev::ic::vgavar::_vga_gdc_write(
            $vh,
            ::core::mem::offset_of!($crate::dev::ic::vgareg::RegVgagdc, $reg),
            $val,
        )
    };
}
pub(crate) use vga_gdc_write;

/// `vga_6845_read(vh, reg)`: the 6845 register named by a field of `struct reg_mc6845`.
macro_rules! vga_6845_read {
    ($vh:expr, $reg:ident) => {
        $crate::dev::ic::pcdisplayvar::pcdisplay_6845_read!(&($vh).vh_ph, $reg)
    };
}
pub(crate) use vga_6845_read;

/// `vga_6845_write(vh, reg, val)`.
macro_rules! vga_6845_write {
    ($vh:expr, $reg:ident, $val:expr) => {
        $crate::dev::ic::pcdisplayvar::pcdisplay_6845_write!(&($vh).vh_ph, $reg, $val)
    };
}
pub(crate) use vga_6845_write;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_screen_is_unattached() {
        let scr = Vgascreen::new();
        assert!(scr.cfg.get().is_null());
        assert!(scr.fontset1().is_none());
        assert_eq!(scr.pcs.active.get(), 0);
    }
}
/* </TESTS> */
