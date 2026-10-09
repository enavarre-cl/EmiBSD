/* <LICENSES> */
/*-
 * Copyright (c) 1999 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Andrew Doran.
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
//! Raster operations: `dev/rasops/rasops.c` and `<dev/rasops/rasops.h>`, the text console
//! drawn in software on a linear frame buffer (`rasops(9)`).
//!
//! Upstream: sys/dev/rasops/rasops.c @ 3ce1f3f79392
//! Upstream: sys/dev/rasops/rasops.h @ 3ce1f3f79392
//!
//! A frame buffer driver fills the geometry of a [`RasopsInfo`] (depth, bits, width, height,
//! stride and, for direct colour, the channel layout) and calls [`rasops_init`]: rasops picks
//! a font from `wsfont(9)`, computes the character grid, and fills `ri_ops`, the
//! [`WsdisplayEmulops`] wsdisplay draws with. The depth-specific part (`putchar`, and the
//! faster row and column operations of some depths) is in `rasops1.rs` ... `rasops32.rs`.
//! With `RI_VCONS`, every screen [`rasops_alloc_screen`] makes keeps a backing store of
//! characters (with scrollback) and only the visible one draws; with `RI_WRONLY` and a
//! backing store, the frame buffer is never read back (copies are redrawn from the store).
//!
//! The driver's attach arguments hand wsdisplay the screen descriptor (whose `textops` is
//! `ri_ops`) and the access operations, most of which are rasops's own
//! (`rasops_alloc_screen`, `rasops_show_screen`, `rasops_getchar`, ...).
//!
//! ## Deviations
//! - Every field of [`RasopsInfo`] and [`RasopsScreen`] is a `Cell`: the C mutates them
//!   through the `void *` cookie the emulops receive while other references to the same
//!   structure are live (an operation calls another with the same cookie). The structure is
//!   zero-valid, as a driver softc must be.
//! - The emulops are `unsafe fn`s over the cookie (`docs/C_TO_RUST.md`, a table over an opaque
//!   pointer): the cookie is a [`RasopsInfo`] for the plain, `RI_WRONLY` and depth operations,
//!   a [`RasopsScreen`] for the `RI_VCONS` ones. The `ri_putchar` ... `ri_pack_attr` hooks and
//!   the `ri_ops` table have methods of the same names that call through them with the
//!   structure itself as the cookie (`ri.ri_putchar(row, ...)`), panicking with the hook's
//!   name where the C would call through NULL.
//! - The frame buffer is memory mapped `BUS_SPACE_MAP_LINEAR` (`bus_space_vaddr`): single
//!   pixels and words are written with `write_volatile`, spans are copied and cleared with
//!   `ptr::copy`/`write_bytes` where the C uses `memmove`/`memset`. The C's unrolled
//!   eight-word loops (`dp[0] = clr; ... dp[7] = clr;`) are one loop over the same words.
//! - `RASOPS_CLIPPING` and `DEBUG` are not configured; their checks are kept behind the
//!   `RASOPS_CLIPPING` and `DEBUG` constants (false). `NRASOPS_BSWAP` is 0 on amd64 and arm64
//!   (only sparc64 frame buffers set it): `slow_bcopy` and the `RI_BSWAP` paths are kept
//!   behind `NRASOPS_BSWAP` (false). `NRASOPS_ROTATION` is on (amd64's GENERIC has it through
//!   `inteldrm`), and built on arm64 too, where no driver asks for a rotated display.
//! - The depths are the ones amd64 and arm64 GENERIC configure (`rasops1`, `rasops8`,
//!   `rasops15`/`rasops16`, `rasops24`, `rasops32`, through `ssdfb`, `efifb`, `simplefb`,
//!   `udl` and the DRM drivers); `rasops4` (no amd64 or arm64 driver) is ported as well.
//! - Failures: `rasops_init` and `rasops_reconfig` return `Err(EINVAL)` where the C returns -1
//!   (no font, unsupported depth) and pass `rasops_alloc_screen`'s `ENOMEM` on. Allocation
//!   is `malloc(9)` with `M_NOWAIT`, as in C.
//! - `ri_switchtask` runs `rasops_doswitch` on `systq` with the `RasopsInfo` as its argument;
//!   `task_add` wants a `&'static Task`, so [`rasops_init`] takes a `&'static RasopsInfo`
//!   (a static or a softc, which live for good).
//! - The screen list (`ri_screens`) is a `queue.rs` list; the framebuffer claims
//!   (`rasops_framebuffers`) and the rotated fonts (`rotatedfonts`) are lists of malloc'd
//!   entries, never freed, as in C.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::dev::rasops::rasops32::rasops32_init;
use crate::dev::wscons::wsconsio::{WSDISPLAY_FONTENC_ISO, WSDISPLAY_FONTORDER_KNOWN};
use crate::dev::wscons::wsconsio::{WSDISPLAY_FONTORDER_L2R, WsdisplayFont};
use crate::dev::wscons::wsdisplayvar::{
    ShowScreenCb, WS_DEFAULT_BG, WS_DEFAULT_FG, WSATTR_BLINK, WSATTR_HILIT, WSATTR_REVERSE,
    WSATTR_UNDERLINE, WSATTR_WSCOLORS, WSSCREEN_HILIT, WSSCREEN_REVERSE, WSSCREEN_UNDERLINE,
    WSSCREEN_WSCOLORS, WsdisplayCharcell, WsdisplayEmulops, WsscreenDescr,
};
use crate::dev::wsfont::wsfont::{
    wsfont_add, wsfont_enum, wsfont_find, wsfont_init, wsfont_lock, wsfont_map_unichar,
    wsfont_rotate, wsfont_unlock,
};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_task::{task_add, task_set};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::intr::{splhigh, splx};
use crate::queue_adapter;
use crate::sys::device::Device;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_WAITOK};
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::task::{SYSTQ, Task};
use crate::sys::types::{Paddr, Psize};

/// `RASOPS_CLIPPING`: not configured.
pub(crate) const RASOPS_CLIPPING: bool = false;
/// `DEBUG`: not configured.
const DEBUG: bool = false;
/// `NRASOPS_BSWAP > 0`: no amd64 or arm64 frame buffer has the other byte order.
pub(crate) const NRASOPS_BSWAP: bool = false;

/// `RI_FULLCLEAR`: eraserows() hack to clear full screen.
pub const RI_FULLCLEAR: i32 = 0x0001;
/// `RI_FORCEMONO`: monochrome output even if we can do color.
pub const RI_FORCEMONO: i32 = 0x0002;
/// `RI_BSWAP`: framebuffer endianness doesn't match CPU.
pub const RI_BSWAP: i32 = 0x0004;
/// `RI_CURSOR`: cursor is switched on.
pub const RI_CURSOR: i32 = 0x0008;
/// `RI_CLEAR`: clear display on startup.
pub const RI_CLEAR: i32 = 0x0010;
/// `RI_CLEARMARGINS`: clear display margins on startup.
pub const RI_CLEARMARGINS: i32 = 0x0020;
/// `RI_CENTER`: center onscreen output.
pub const RI_CENTER: i32 = 0x0040;
/// `RI_CURSORCLIP`: cursor is currently clipped.
pub const RI_CURSORCLIP: i32 = 0x0080;
/// `RI_ROTATE_CW`: display is rotated, 90 deg clockwise.
pub const RI_ROTATE_CW: i32 = 0x0100;
/// `RI_ROTATE_CCW`: display is rotated, 90 deg anti-clockwise.
pub const RI_ROTATE_CCW: i32 = 0x0200;
/// `RI_CFGDONE`: rasops_reconfig() completed successfully.
pub const RI_CFGDONE: i32 = 0x0400;
/// `RI_VCONS`: virtual consoles.
pub const RI_VCONS: i32 = 0x0800;
/// `RI_WRONLY`: avoid framebuffer reads.
pub const RI_WRONLY: i32 = 0x1000;

/// `RS_SCROLLBACK_SCREENS`.
const RS_SCROLLBACK_SCREENS: i32 = 5;

/// `NORMAL_BLACK` ... `HILITE_WHITE`: the ANSI colormap (R,G,B).
const NORMAL: [u32; 8] = [
    0x000000, 0x7f0000, 0x007f00, 0x7f7f00, 0x00007f, 0x7f007f, 0x007f7f,
    0xc7c7c7, // XXX too dim?
];
/// `HILITE_*`.
const HILITE: [u32; 8] = [
    0x7f7f7f, 0xff0000, 0x00ff00, 0xffff00, 0x0000ff, 0xff00ff, 0x00ffff, 0xffffff,
];

/// `int (*ri_do_cursor)(struct rasops_info *)`.
pub type DoCursorFn = fn(ri: &RasopsInfo) -> Result<(), Errno>;

/// `struct rasops_info`: a frame buffer's text console state.
pub struct RasopsInfo {
    // These must be filled in by the caller
    /// `ri_depth`: depth in bits.
    pub ri_depth: Cell<i32>,
    /// `ri_bits`: ptr to bits.
    pub ri_bits: Cell<*mut u8>,
    /// `ri_width`: width (pels).
    pub ri_width: Cell<i32>,
    /// `ri_height`: height (pels).
    pub ri_height: Cell<i32>,
    /// `ri_stride`: stride in bytes.
    pub ri_stride: Cell<i32>,

    // These can optionally be left zeroed out. If you fill ri_font, but aren't using
    // wsfont, set ri_wsfcookie to -1.
    /// `ri_font`: the font, locked through `ri_wsfcookie`.
    pub ri_font: Cell<*mut WsdisplayFont>,
    /// `ri_wsfcookie`: wsfont cookie.
    pub ri_wsfcookie: Cell<i32>,
    /// `ri_hw`: driver private data; ignored by rasops.
    pub ri_hw: Cell<*mut c_void>,
    /// `ri_bs`: character backing store, `ri_rows * ri_cols` cells (or NULL).
    pub ri_bs: Cell<*mut WsdisplayCharcell>,
    /// `ri_crow`: cursor row.
    pub ri_crow: Cell<i32>,
    /// `ri_ccol`: cursor column.
    pub ri_ccol: Cell<i32>,
    /// `ri_flg`: various operational flags (`RI_*`).
    pub ri_flg: Cell<i32>,

    // These are optional and will default if zero. Meaningless on depths other than 15,
    // 16, 24 and 32 bits per pel. On 24 bit displays, ri_{r,g,b}num must be 8.
    /// `ri_rnum`: number of bits for red.
    pub ri_rnum: Cell<u8>,
    /// `ri_gnum`: number of bits for green.
    pub ri_gnum: Cell<u8>,
    /// `ri_bnum`: number of bits for blue.
    pub ri_bnum: Cell<u8>,
    /// `ri_rpos`: which bit red starts at.
    pub ri_rpos: Cell<u8>,
    /// `ri_gpos`: which bit green starts at.
    pub ri_gpos: Cell<u8>,
    /// `ri_bpos`: which bit blue starts at.
    pub ri_bpos: Cell<u8>,

    // These are filled in by rasops_init()
    /// `ri_emuwidth`: width we actually care about.
    pub ri_emuwidth: Cell<i32>,
    /// `ri_emuheight`: height we actually care about.
    pub ri_emuheight: Cell<i32>,
    /// `ri_emustride`: bytes per row we actually care about.
    pub ri_emustride: Cell<i32>,
    /// `ri_rows`: number of rows (characters, not pels).
    pub ri_rows: Cell<i32>,
    /// `ri_cols`: number of columns (characters, not pels).
    pub ri_cols: Cell<i32>,
    /// `ri_delta`: row delta in bytes.
    pub ri_delta: Cell<i32>,
    /// `ri_pelbytes`: bytes per pel (may be zero).
    pub ri_pelbytes: Cell<i32>,
    /// `ri_fontscale`: fontheight * fontstride.
    pub ri_fontscale: Cell<i32>,
    /// `ri_xscale`: fontwidth * pelbytes.
    pub ri_xscale: Cell<i32>,
    /// `ri_yscale`: fontheight * stride.
    pub ri_yscale: Cell<i32>,
    /// `ri_origbits`: where screen bits actually start.
    pub ri_origbits: Cell<*mut u8>,
    /// `ri_xorigin`: where ri_bits begins (x).
    pub ri_xorigin: Cell<i32>,
    /// `ri_yorigin`: where ri_bits begins (y).
    pub ri_yorigin: Cell<i32>,
    /// `ri_devcmap`: color -> framebuffer data.
    pub ri_devcmap: [Cell<i32>; 16],

    // The emulops you need to use, and the screen caps for wscons
    /// `ri_ops`.
    pub ri_ops: Cell<WsdisplayEmulops>,
    /// `ri_caps`.
    pub ri_caps: Cell<i32>,

    // Callbacks so we can share some code
    /// `ri_do_cursor`.
    pub ri_do_cursor: Cell<Option<DoCursorFn>>,
    /// `ri_updatecursor`.
    pub ri_updatecursor: Cell<Option<fn(&RasopsInfo)>>,

    /// `ri_real_ops`: used to intercept putchar to permit display rotation
    /// (`NRASOPS_ROTATION > 0`).
    pub ri_real_ops: Cell<WsdisplayEmulops>,

    /// `ri_nscreens`.
    pub ri_nscreens: Cell<i32>,
    /// `ri_screens`.
    pub ri_screens: ListHead<RsNext>,
    /// `ri_active`.
    pub ri_active: Cell<*mut RasopsScreen>,

    /// `ri_switchcb`.
    pub ri_switchcb: Cell<Option<ShowScreenCb>>,
    /// `ri_switchcbarg`.
    pub ri_switchcbarg: Cell<*mut c_void>,
    /// `ri_switchcookie`.
    pub ri_switchcookie: Cell<*mut c_void>,
    /// `ri_switchtask`.
    pub ri_switchtask: Task,

    /// `ri_putchar`.
    pub ri_putchar: Cell<Option<crate::dev::wscons::wsdisplayvar::PutcharFn>>,
    /// `ri_copycols`.
    pub ri_copycols: Cell<Option<crate::dev::wscons::wsdisplayvar::CopycolsFn>>,
    /// `ri_erasecols`.
    pub ri_erasecols: Cell<Option<crate::dev::wscons::wsdisplayvar::ErasecolsFn>>,
    /// `ri_copyrows`.
    pub ri_copyrows: Cell<Option<crate::dev::wscons::wsdisplayvar::CopyrowsFn>>,
    /// `ri_eraserows`.
    pub ri_eraserows: Cell<Option<crate::dev::wscons::wsdisplayvar::EraserowsFn>>,
    /// `ri_pack_attr`.
    pub ri_pack_attr: Cell<Option<crate::dev::wscons::wsdisplayvar::PackAttrFn>>,
}

impl RasopsInfo {
    /// A zeroed descriptor, as a driver's static or softc starts.
    pub const fn new() -> Self {
        Self {
            ri_depth: Cell::new(0),
            ri_bits: Cell::new(ptr::null_mut()),
            ri_width: Cell::new(0),
            ri_height: Cell::new(0),
            ri_stride: Cell::new(0),
            ri_font: Cell::new(ptr::null_mut()),
            ri_wsfcookie: Cell::new(0),
            ri_hw: Cell::new(ptr::null_mut()),
            ri_bs: Cell::new(ptr::null_mut()),
            ri_crow: Cell::new(0),
            ri_ccol: Cell::new(0),
            ri_flg: Cell::new(0),
            ri_rnum: Cell::new(0),
            ri_gnum: Cell::new(0),
            ri_bnum: Cell::new(0),
            ri_rpos: Cell::new(0),
            ri_gpos: Cell::new(0),
            ri_bpos: Cell::new(0),
            ri_emuwidth: Cell::new(0),
            ri_emuheight: Cell::new(0),
            ri_emustride: Cell::new(0),
            ri_rows: Cell::new(0),
            ri_cols: Cell::new(0),
            ri_delta: Cell::new(0),
            ri_pelbytes: Cell::new(0),
            ri_fontscale: Cell::new(0),
            ri_xscale: Cell::new(0),
            ri_yscale: Cell::new(0),
            ri_origbits: Cell::new(ptr::null_mut()),
            ri_xorigin: Cell::new(0),
            ri_yorigin: Cell::new(0),
            ri_devcmap: [const { Cell::new(0) }; 16],
            ri_ops: Cell::new(WsdisplayEmulops::EMPTY),
            ri_caps: Cell::new(0),
            ri_do_cursor: Cell::new(None),
            ri_updatecursor: Cell::new(None),
            ri_real_ops: Cell::new(WsdisplayEmulops::EMPTY),
            ri_nscreens: Cell::new(0),
            ri_screens: ListHead::new(),
            ri_active: Cell::new(ptr::null_mut()),
            ri_switchcb: Cell::new(None),
            ri_switchcbarg: Cell::new(ptr::null_mut()),
            ri_switchcookie: Cell::new(ptr::null_mut()),
            ri_switchtask: Task::zeroed(),
            ri_putchar: Cell::new(None),
            ri_copycols: Cell::new(None),
            ri_erasecols: Cell::new(None),
            ri_copyrows: Cell::new(None),
            ri_eraserows: Cell::new(None),
            ri_pack_attr: Cell::new(None),
        }
    }

    /// The structure as the `void *` cookie of its operations.
    pub fn cookie(&self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }

    /// `ri->ri_font`: the font rasops locked (or the driver set).
    pub fn font(&self) -> &WsdisplayFont {
        let f = self.ri_font.get();
        if f.is_null() {
            panic(format_args!("rasops: no font selected"));
        }
        // SAFETY: a font locked through wsfont (or the driver's own, which it keeps) stays
        // valid and is not modified while locked (wsfont only reverses unlocked fonts).
        unsafe { &*f }
    }

    /// The glyph rows of character `uc` (an index from `firstchar`): `ri_fontscale` bytes.
    pub fn glyph(&self, uc: u32) -> &[u8] {
        let font = self.font();
        let scale = self.ri_fontscale.get() as usize;
        let n = font.numchars.max(0) as usize;
        let uc = (uc as usize).min(n.saturating_sub(1));
        // SAFETY: a font's data holds `numchars` glyphs of `fontheight * stride` bytes
        // (`ri_fontscale`), and the index is clamped to them; the font is locked (see `font`).
        unsafe { core::slice::from_raw_parts(font.data.cast::<u8>().add(uc * scale), scale) }
    }

    /// Calls `ri_do_cursor`.
    fn ri_do_cursor(&self) -> Result<(), Errno> {
        match self.ri_do_cursor.get() {
            Some(f) => f(self),
            None => panic(format_args!("rasops: ri_do_cursor is NULL")),
        }
    }

    /// Calls `ri_putchar` with this structure as the cookie.
    pub fn ri_putchar(&self, row: i32, col: i32, uc: u32, attr: u32) -> Result<(), Errno> {
        let Some(f) = self.ri_putchar.get() else {
            panic(format_args!("rasops: ri_putchar is NULL"));
        };
        // SAFETY: rasops installs depth operations, whose cookie is the rasops_info.
        unsafe { f(self.cookie(), row, col, uc, attr) }
    }

    /// Calls `ri_copycols` with this structure as the cookie.
    pub fn ri_copycols(&self, row: i32, src: i32, dst: i32, num: i32) -> Result<(), Errno> {
        let Some(f) = self.ri_copycols.get() else {
            panic(format_args!("rasops: ri_copycols is NULL"));
        };
        // SAFETY: as in `ri_putchar`.
        unsafe { f(self.cookie(), row, src, dst, num) }
    }

    /// Calls `ri_erasecols` with this structure as the cookie.
    pub fn ri_erasecols(&self, row: i32, col: i32, num: i32, attr: u32) -> Result<(), Errno> {
        let Some(f) = self.ri_erasecols.get() else {
            panic(format_args!("rasops: ri_erasecols is NULL"));
        };
        // SAFETY: as in `ri_putchar`.
        unsafe { f(self.cookie(), row, col, num, attr) }
    }

    /// Calls `ri_copyrows` with this structure as the cookie.
    pub fn ri_copyrows(&self, src: i32, dst: i32, num: i32) -> Result<(), Errno> {
        let Some(f) = self.ri_copyrows.get() else {
            panic(format_args!("rasops: ri_copyrows is NULL"));
        };
        // SAFETY: as in `ri_putchar`.
        unsafe { f(self.cookie(), src, dst, num) }
    }

    /// Calls `ri_eraserows` with this structure as the cookie.
    pub fn ri_eraserows(&self, row: i32, num: i32, attr: u32) -> Result<(), Errno> {
        let Some(f) = self.ri_eraserows.get() else {
            panic(format_args!("rasops: ri_eraserows is NULL"));
        };
        // SAFETY: as in `ri_putchar`.
        unsafe { f(self.cookie(), row, num, attr) }
    }

    /// Calls `ri_pack_attr` with this structure as the cookie.
    pub fn ri_pack_attr(&self, fg: i32, bg: i32, flg: i32) -> Result<u32, Errno> {
        let Some(f) = self.ri_pack_attr.get() else {
            panic(format_args!("rasops: ri_pack_attr is NULL"));
        };
        // SAFETY: as in `ri_putchar`.
        unsafe { f(self.cookie(), fg, bg, flg) }
    }

    /// Calls `ri_ops.pack_attr` with `cookie` (the descriptor itself, or the active screen
    /// with `RI_VCONS`), as the drivers do before `wsdisplay_cnattach`.
    ///
    /// # Safety
    ///
    /// `cookie` is the one `ri_ops` expects: this structure, or a screen of it under
    /// `RI_VCONS`.
    pub unsafe fn ops_pack_attr(
        &self,
        cookie: *mut c_void,
        fg: i32,
        bg: i32,
        flg: i32,
    ) -> Result<u32, Errno> {
        let Some(f) = self.ri_ops.get().pack_attr else {
            panic(format_args!("rasops: pack_attr is NULL"));
        };
        // SAFETY: the caller's contract.
        unsafe { f(cookie, fg, bg, flg) }
    }

    /// `&ri->ri_ops`, for a screen descriptor's `textops`.
    pub fn ops_ptr(&self) -> *const WsdisplayEmulops {
        self.ri_ops.as_ptr()
    }

    /// Fills a screen descriptor from the configured geometry, as every raster driver does
    /// after `rasops_init` (`ncols`, `nrows`, `textops`, `fontwidth`, `fontheight`,
    /// `capabilities`).
    pub fn fill_descr(&self, descr: &mut WsscreenDescr) {
        descr.ncols = self.ri_cols.get();
        descr.nrows = self.ri_rows.get();
        descr.textops = self.ops_ptr();
        descr.fontwidth = self.font().fontwidth as i32;
        descr.fontheight = self.font().fontheight as i32;
        descr.capabilities = self.ri_caps.get();
    }

    /// `ri_bs[off]`.
    fn bs(&self, off: i32) -> WsdisplayCharcell {
        // SAFETY: `ri_bs` holds `ri_rows * ri_cols` cells and callers index inside them.
        unsafe { self.ri_bs.get().add(off as usize).read() }
    }

    /// `ri_bs[off] = cell`.
    fn set_bs(&self, off: i32, uc: u32, attr: u32) {
        // SAFETY: as in `bs`.
        unsafe {
            self.ri_bs
                .get()
                .add(off as usize)
                .write(WsdisplayCharcell { uc, attr })
        }
    }
}

impl Default for RasopsInfo {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// The `rs_next` link of `ri_screens`.
    pub RsNext: RasopsScreen, rs_next => ListEntry<RasopsScreen>
);

/// `struct rasops_screen`: a virtual screen (`RI_VCONS`) and its backing store.
pub struct RasopsScreen {
    /// `rs_next`.
    rs_next: ListEntry<RasopsScreen>,
    /// `rs_ri`.
    rs_ri: &'static RasopsInfo,
    /// `rs_bs`: `(rs_sbscreens + 1) * ri_rows * ri_cols` cells, the scrollback first.
    rs_bs: *mut WsdisplayCharcell,
    /// `rs_visible`.
    rs_visible: Cell<i32>,
    /// `rs_crow`.
    rs_crow: Cell<i32>,
    /// `rs_ccol`.
    rs_ccol: Cell<i32>,
    /// `rs_defattr`.
    rs_defattr: Cell<u32>,
    /// `rs_sbscreens`.
    rs_sbscreens: i32,
    /// `rs_dispoffset`: rs_bs index, start of our actual screen.
    rs_dispoffset: i32,
    /// `rs_visibleoffset`: rs_bs index, current scrollback screen.
    rs_visibleoffset: Cell<i32>,
}

impl RasopsScreen {
    /// `rs_bs[off]`.
    fn bs(&self, off: i32) -> WsdisplayCharcell {
        // SAFETY: `rs_bs` holds `(rs_sbscreens + 1) * ri_rows * ri_cols` cells and callers
        // index inside them.
        unsafe { self.rs_bs.add(off as usize).read() }
    }

    /// `rs_bs[off] = cell`.
    fn set_bs(&self, off: i32, uc: u32, attr: u32) {
        // SAFETY: as in `bs`.
        unsafe {
            self.rs_bs
                .add(off as usize)
                .write(WsdisplayCharcell { uc, attr })
        }
    }

    /// `memmove(&rs_bs[dst], &rs_bs[src], n cells)`.
    fn move_bs(&self, dst: i32, src: i32, n: i32) {
        if n <= 0 {
            return;
        }
        // SAFETY: both ranges lie inside the store (the callers' arithmetic is the C's);
        // `ptr::copy` allows them to overlap, as memmove does.
        unsafe {
            ptr::copy(
                self.rs_bs.add(src as usize),
                self.rs_bs.add(dst as usize),
                n as usize,
            )
        }
    }

    /// The cells of the whole store, for tests and `rasops_getchar`.
    fn ncells(&self) -> usize {
        let ri = self.rs_ri;
        ((self.rs_sbscreens + 1) * ri.ri_rows.get() * ri.ri_cols.get()) as usize
    }
}

/// `struct rotatedfont`: a font and its rotated copy (`NRASOPS_ROTATION > 0`).
struct Rotatedfont {
    /// `rf_next`.
    rf_next: *mut Rotatedfont,
    /// `rf_cookie`.
    rf_cookie: i32,
    /// `rf_rotated`.
    rf_rotated: i32,
}

/// `struct rasops_framebuffer`: a frame buffer a driver claimed.
struct RasopsFramebuffer {
    /// `rf_list`.
    rf_list: *mut RasopsFramebuffer,
    /// `rf_base`.
    rf_base: Paddr,
    /// `rf_size`.
    rf_size: Psize,
    /// `rf_dev`.
    #[allow(dead_code)] // the C records it and never reads it
    rf_dev: *const Device,
}

/// `rasops_cmap`: the ANSI colormap (R,G,B), then white, then the opposite of the first 16
/// colours for the cursor.
pub static RASOPS_CMAP: [u8; 256 * 3] = {
    /// `_C(x)`.
    const fn put(t: &mut [u8; 768], i: usize, x: u32) {
        t[i * 3] = ((x & 0xff0000) >> 16) as u8;
        t[i * 3 + 1] = ((x & 0x00ff00) >> 8) as u8;
        t[i * 3 + 2] = (x & 0x0000ff) as u8;
    }
    let mut t = [0u8; 768];
    let mut i = 0;
    while i < 8 {
        put(&mut t, i, NORMAL[i]);
        put(&mut t, 8 + i, HILITE[i]);
        // For the cursor, we need the last 16 colors to be the opposite of the first 16.
        put(&mut t, 240 + i, !HILITE[7 - i]);
        put(&mut t, 248 + i, !NORMAL[7 - i]);
        i += 1;
    }
    // Fill the intermediate space with white completely for simplicity.
    let mut i = 16;
    while i < 240 {
        put(&mut t, i, HILITE[7]);
        i += 1;
    }
    t
};

/// `rotatedfonts`: list of all rotated fonts.
static ROTATEDFONTS: AtomicPtr<Rotatedfont> = AtomicPtr::new(ptr::null_mut());

/// `rasops_framebuffers`.
static RASOPS_FRAMEBUFFERS: AtomicPtr<RasopsFramebuffer> = AtomicPtr::new(ptr::null_mut());

/// The `RasopsInfo` behind an emulop's cookie.
///
/// # Safety
///
/// `cookie` is a `RasopsInfo` that outlives the returned reference.
unsafe fn ri_of<'a>(cookie: *mut c_void) -> &'a RasopsInfo {
    // SAFETY: the caller's contract.
    unsafe { &*cookie.cast::<RasopsInfo>() }
}

/// The `RasopsScreen` behind a `RI_VCONS` emulop's cookie.
///
/// # Safety
///
/// `cookie` is a screen `rasops_alloc_screen` made and `rasops_free_screen` has not freed.
unsafe fn scr_of<'a>(cookie: *mut c_void) -> &'a RasopsScreen {
    // SAFETY: the caller's contract.
    unsafe { &*cookie.cast::<RasopsScreen>() }
}

/// A 32-bit store into the frame buffer.
///
/// # Safety
///
/// `p` is a 4-byte aligned address inside the mapped frame buffer.
pub(crate) unsafe fn fb_w32(p: *mut u8, v: i32) {
    // SAFETY: the caller's contract.
    unsafe { ptr::write_volatile(p.cast::<i32>(), v) }
}

/// A 16-bit store into the frame buffer.
///
/// # Safety
///
/// `p` is a 2-byte aligned address inside the mapped frame buffer.
pub(crate) unsafe fn fb_w16(p: *mut u8, v: i16) {
    // SAFETY: the caller's contract.
    unsafe { ptr::write_volatile(p.cast::<i16>(), v) }
}

/// A byte store into the frame buffer.
///
/// # Safety
///
/// `p` is inside the mapped frame buffer.
pub(crate) unsafe fn fb_w8(p: *mut u8, v: u8) {
    // SAFETY: the caller's contract.
    unsafe { ptr::write_volatile(p, v) }
}

/// A 32-bit load from the frame buffer.
///
/// # Safety
///
/// As for [`fb_w32`].
pub(crate) unsafe fn fb_r32(p: *const u8) -> i32 {
    // SAFETY: the caller's contract.
    unsafe { ptr::read_volatile(p.cast::<i32>()) }
}

/// A 16-bit load from the frame buffer.
///
/// # Safety
///
/// As for [`fb_w16`].
pub(crate) unsafe fn fb_r16(p: *const u8) -> i16 {
    // SAFETY: the caller's contract.
    unsafe { ptr::read_volatile(p.cast::<i16>()) }
}

/// A byte load from the frame buffer.
///
/// # Safety
///
/// As for [`fb_w8`].
pub(crate) unsafe fn fb_r8(p: *const u8) -> u8 {
    // SAFETY: the caller's contract.
    unsafe { ptr::read_volatile(p) }
}

/// `rasops_init`: initialize a `rasops_info` descriptor. The integer parameters are the
/// number of rows and columns we'd *like*.
///
/// In terms of optimization, fonts that are a multiple of 8 pixels wide work the best.
///
/// `rasops_init()` takes care of `rasops_reconfig()`. The parameters to both are the same.
/// If calling `rasops_reconfig()` to change the font and `ri_wsfcookie >= 0`, you must call
/// `wsfont_unlock()` on it, and reset it to -1 (or a new, valid cookie).
pub fn rasops_init(ri: &'static RasopsInfo, wantrows: i32, wantcols: i32) -> Result<(), Errno> {
    // Select a font if the caller doesn't care
    if ri.ri_font.get().is_null() {
        let valid = |c: Option<i32>| c.filter(|&c| c > 0);
        let mut cookie = None;

        wsfont_init();

        if ri.ri_width.get() >= 120 * 32 {
            // Screen width of at least 3840px, 32px wide font
            cookie = valid(wsfont_find(None, 32, 0, 0));
        }

        if cookie.is_none() && ri.ri_width.get() >= 120 * 16 {
            // Screen width of at least 1920px, 16px wide font
            cookie = valid(wsfont_find(None, 16, 0, 0));
        }

        if cookie.is_none() && ri.ri_width.get() > 80 * 12 {
            // Screen width larger than 960px, 12px wide font
            cookie = valid(wsfont_find(None, 12, 0, 0));
        }

        if cookie.is_none() {
            // Lower resolution, choose a 8px wide font
            cookie = valid(wsfont_find(None, 8, 0, 0));
        }

        if cookie.is_none() {
            cookie = valid(wsfont_find(None, 0, 0, 0));
        }

        let Some(mut cookie) = cookie else {
            printf(format_args!("rasops_init: font table is empty\n"));
            return Err(Errno::EINVAL);
        };

        // Pick the rotated version of this font. This will create it if necessary.
        if ri.ri_flg.get() & (RI_ROTATE_CW | RI_ROTATE_CCW) != 0 {
            rasops_rotate_font(&mut cookie, ri.ri_flg.get() & RI_ROTATE_CCW != 0);
        }

        match wsfont_lock(cookie, WSDISPLAY_FONTORDER_L2R, WSDISPLAY_FONTORDER_L2R) {
            Ok((lc, font)) if lc > 0 => ri.ri_font.set(font.as_ptr()),
            _ => {
                printf(format_args!("rasops_init: couldn't lock font\n"));
                return Err(Errno::EINVAL);
            }
        }

        ri.ri_wsfcookie.set(cookie);
    }

    // This should never happen in reality...
    if DEBUG {
        if ri.ri_bits.get() as usize & 3 != 0 {
            printf(format_args!(
                "rasops_init: bits not aligned on 32-bit boundary\n"
            ));
            return Err(Errno::EINVAL);
        }

        if ri.ri_stride.get() & 3 != 0 {
            printf(format_args!(
                "rasops_init: stride not aligned on 32-bit boundary\n"
            ));
            return Err(Errno::EINVAL);
        }
    }

    rasops_reconfig(ri, wantrows, wantcols)?;

    ri.ri_screens.init();
    ri.ri_nscreens.set(0);

    let ops = ri.ri_ops.get();
    ri.ri_putchar.set(ops.putchar);
    ri.ri_copycols.set(ops.copycols);
    ri.ri_erasecols.set(ops.erasecols);
    ri.ri_copyrows.set(ops.copyrows);
    ri.ri_eraserows.set(ops.eraserows);
    ri.ri_pack_attr.set(ops.pack_attr);

    if ri.ri_flg.get() & RI_VCONS != 0 {
        let mut cookie = ptr::null_mut();
        let (mut curx, mut cury, mut attr) = (0, 0, 0);

        // SAFETY: the access cookie of rasops's own operations is the rasops_info.
        unsafe {
            rasops_alloc_screen(
                ri.cookie(),
                ptr::null(),
                &mut cookie,
                &mut curx,
                &mut cury,
                &mut attr,
            )?
        };

        ri.ri_active.set(cookie.cast());
        // SAFETY: the screen just made.
        let scr = unsafe { scr_of(cookie) };
        // SAFETY: inside the screen's store: the visible screen follows the scrollback.
        ri.ri_bs
            .set(unsafe { scr.rs_bs.add(scr.rs_dispoffset as usize) });

        let mut ops = ri.ri_ops.get();
        ops.cursor = Some(rasops_vcons_cursor);
        ops.mapchar = Some(rasops_vcons_mapchar);
        ops.putchar = Some(rasops_vcons_putchar);
        ops.copycols = Some(rasops_vcons_copycols);
        ops.erasecols = Some(rasops_vcons_erasecols);
        ops.copyrows = Some(rasops_vcons_copyrows);
        ops.eraserows = Some(rasops_vcons_eraserows);
        ops.pack_attr = Some(rasops_vcons_pack_attr);
        ops.unpack_attr = Some(rasops_vcons_unpack_attr);
        ri.ri_ops.set(ops);
        ri.ri_do_cursor.set(Some(rasops_wronly_do_cursor));
    } else if ri.ri_flg.get() & RI_WRONLY != 0 && !ri.ri_bs.get().is_null() {
        let mut ops = ri.ri_ops.get();
        ops.putchar = Some(rasops_wronly_putchar);
        ops.copycols = Some(rasops_wronly_copycols);
        ops.erasecols = Some(rasops_wronly_erasecols);
        ops.copyrows = Some(rasops_wronly_copyrows);
        ops.eraserows = Some(rasops_wronly_eraserows);
        ri.ri_ops.set(ops);
        ri.ri_do_cursor.set(Some(rasops_wronly_do_cursor));

        if ri.ri_flg.get() & RI_CLEAR != 0 {
            let attr = ri.ri_pack_attr(0, 0, 0).unwrap_or(0);
            for i in 0..ri.ri_rows.get() * ri.ri_cols.get() {
                ri.set_bs(i, u32::from(b' '), attr);
            }
        }
    }

    task_set(&ri.ri_switchtask, rasops_doswitch, ri.cookie());

    rasops_init_devcmap(ri);
    Ok(())
}

/// `rasops_reconfig`: reconfigure (because parameters have changed in some way).
pub fn rasops_reconfig(ri: &RasopsInfo, mut wantrows: i32, mut wantcols: i32) -> Result<(), Errno> {
    let s = splhigh();

    let font = ri.font();
    if font.fontwidth > 32 || font.fontwidth < 4 {
        panic(format_args!("rasops_init: fontwidth assumptions botched!"));
    }
    let (fontwidth, fontheight) = (font.fontwidth as i32, font.fontheight as i32);

    // Need this to frob the setup below
    let bpp = if ri.ri_depth.get() == 15 {
        16
    } else {
        ri.ri_depth.get()
    };

    if ri.ri_flg.get() & RI_CFGDONE != 0 {
        ri.ri_bits.set(ri.ri_origbits.get());
    }

    // Don't care if the caller wants a hideously small console
    if wantrows < 10 {
        wantrows = 10;
    }

    if wantcols < 20 {
        wantcols = 20;
    }

    // Now constrain what they get
    ri.ri_emuwidth.set(fontwidth * wantcols);
    ri.ri_emuheight.set(fontheight * wantrows);

    if ri.ri_emuwidth.get() > ri.ri_width.get() {
        ri.ri_emuwidth.set(ri.ri_width.get());
    }

    if ri.ri_emuheight.get() > ri.ri_height.get() {
        ri.ri_emuheight.set(ri.ri_height.get());
    }

    // Reduce width until aligned on a 32-bit boundary
    while (ri.ri_emuwidth.get() * bpp) & 31 != 0 {
        ri.ri_emuwidth.set(ri.ri_emuwidth.get() - 1);
    }

    if ri.ri_flg.get() & (RI_ROTATE_CW | RI_ROTATE_CCW) != 0 {
        ri.ri_rows.set(ri.ri_emuwidth.get() / fontwidth);
        ri.ri_cols.set(ri.ri_emuheight.get() / fontheight);
    } else {
        ri.ri_cols.set(ri.ri_emuwidth.get() / fontwidth);
        ri.ri_rows.set(ri.ri_emuheight.get() / fontheight);
    }
    ri.ri_emustride.set((ri.ri_emuwidth.get() * bpp) >> 3);
    ri.ri_delta.set(ri.ri_stride.get() - ri.ri_emustride.get());
    ri.ri_ccol.set(0);
    ri.ri_crow.set(0);
    ri.ri_pelbytes.set(bpp >> 3);

    ri.ri_xscale.set((fontwidth * bpp) >> 3);
    ri.ri_yscale.set(fontheight * ri.ri_stride.get());
    ri.ri_fontscale.set(fontheight * font.stride as i32);

    if DEBUG && ri.ri_delta.get() & 3 != 0 {
        panic(format_args!(
            "rasops_init: ri_delta not aligned on 32-bit boundary"
        ));
    }

    // Clear the entire display
    if ri.ri_flg.get() & RI_CLEAR != 0 {
        // SAFETY: the frame buffer is `ri_stride * ri_height` bytes from `ri_bits` (not yet
        // centred), mapped by the driver.
        unsafe {
            ptr::write_bytes(
                ri.ri_bits.get(),
                0,
                (ri.ri_stride.get() * ri.ri_height.get()) as usize,
            )
        };
        ri.ri_flg.set(ri.ri_flg.get() & !RI_CLEARMARGINS);
    }

    // Now centre our window if needs be
    ri.ri_origbits.set(ri.ri_bits.get());

    if ri.ri_flg.get() & RI_CENTER != 0 {
        let xoff = ((((ri.ri_width.get() * bpp) >> 3) - ri.ri_emustride.get()) >> 1) & !3;
        let yoff = ((ri.ri_height.get() - ri.ri_emuheight.get()) >> 1) * ri.ri_stride.get();
        // SAFETY: both offsets stay inside the frame buffer: the emulated area is no larger
        // than the screen.
        ri.ri_bits
            .set(unsafe { ri.ri_bits.get().add((xoff + yoff) as usize) });

        let off = xoff + yoff;
        ri.ri_yorigin.set(off / ri.ri_stride.get());
        ri.ri_xorigin.set((off % ri.ri_stride.get()) * 8 / bpp);
    } else {
        ri.ri_xorigin.set(0);
        ri.ri_yorigin.set(0);
    }

    // Clear the margins
    if ri.ri_flg.get() & RI_CLEARMARGINS != 0 {
        let orig = ri.ri_origbits.get();
        let bits = ri.ri_bits.get();
        let stride = ri.ri_stride.get() as usize;
        let emustride = ri.ri_emustride.get() as usize;
        let emuheight = ri.ri_emuheight.get() as usize;
        let lead = bits as usize - orig as usize;
        // SAFETY: the margins are the frame buffer bytes around the emulated area, all
        // inside `ri_stride * ri_height` bytes from `ri_origbits`.
        unsafe {
            ptr::write_bytes(orig, 0, lead);
            for l in 0..emuheight {
                ptr::write_bytes(bits.add(emustride + l * stride), 0, stride - emustride);
            }
            let tail = bits.add(emuheight * stride);
            let end = orig.add(ri.ri_height.get() as usize * stride);
            ptr::write_bytes(tail, 0, end as usize - tail as usize);
        }
    }

    // Fill in defaults for operations set. XXX this nukes private routines used by
    // accelerated fb drivers.
    let mut ops = ri.ri_ops.get();
    ops.mapchar = Some(rasops_mapchar);
    ops.copyrows = Some(rasops_copyrows);
    ops.copycols = Some(rasops_copycols);
    ops.erasecols = Some(rasops_erasecols);
    ops.eraserows = Some(rasops_eraserows);
    ops.cursor = Some(rasops_cursor);
    ops.unpack_attr = Some(rasops_unpack_attr);
    ri.ri_do_cursor.set(Some(rasops_do_cursor));
    ri.ri_updatecursor.set(None);

    if ri.ri_depth.get() < 8 || ri.ri_flg.get() & RI_FORCEMONO != 0 {
        ops.pack_attr = Some(rasops_pack_mattr);
        ri.ri_caps.set(WSSCREEN_UNDERLINE | WSSCREEN_REVERSE);
    } else {
        ops.pack_attr = Some(rasops_pack_cattr);
        ri.ri_caps
            .set(WSSCREEN_UNDERLINE | WSSCREEN_HILIT | WSSCREEN_WSCOLORS | WSSCREEN_REVERSE);
    }
    ri.ri_ops.set(ops);

    match ri.ri_depth.get() {
        1 => crate::dev::rasops::rasops1::rasops1_init(ri),
        4 => crate::dev::rasops::rasops4::rasops4_init(ri),
        8 => crate::dev::rasops::rasops8::rasops8_init(ri),
        15 | 16 => crate::dev::rasops::rasops15::rasops15_init(ri),
        24 => crate::dev::rasops::rasops24::rasops24_init(ri),
        32 => rasops32_init(ri),
        _ => {
            ri.ri_flg.set(ri.ri_flg.get() & !RI_CFGDONE);
            splx(s);
            return Err(Errno::EINVAL);
        }
    }

    if ri.ri_flg.get() & (RI_ROTATE_CW | RI_ROTATE_CCW) != 0 {
        let real = ri.ri_ops.get();
        ri.ri_real_ops.set(real);
        let mut ops = real;
        ops.copycols = Some(rasops_copycols_rotated);
        ops.copyrows = Some(rasops_copyrows_rotated);
        ops.erasecols = Some(rasops_erasecols_rotated);
        ops.eraserows = Some(rasops_eraserows_rotated);
        ops.putchar = Some(rasops_putchar_rotated);
        ri.ri_ops.set(ops);
    }

    ri.ri_flg.set(ri.ri_flg.get() | RI_CFGDONE);
    splx(s);
    Ok(())
}

/// `rasops_mapchar`: map a character.
///
/// # Safety
///
/// `cookie` is a [`RasopsInfo`] with a font.
pub unsafe fn rasops_mapchar(cookie: *mut c_void, mut c: i32, cp: &mut u32) -> i32 {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(cookie) };

    let font = ri.font();
    if font.encoding != WSDISPLAY_FONTENC_ISO {
        match wsfont_map_unichar(font, c) {
            Some(g) => c = g,
            None => {
                *cp = u32::from(b'?');
                return 0;
            }
        }
    }

    if c < font.firstchar || c - font.firstchar >= font.numchars {
        *cp = u32::from(b'?');
        return 0;
    }

    *cp = c as u32;
    5
}

/// `rasops_pack_cattr`: pack a color attribute.
///
/// # Safety
///
/// None beyond the emulops' contract: the cookie is not used.
pub unsafe fn rasops_pack_cattr(
    _cookie: *mut c_void,
    mut fg: i32,
    mut bg: i32,
    flg: i32,
) -> Result<u32, Errno> {
    if flg & WSATTR_BLINK != 0 {
        return Err(Errno::EINVAL);
    }

    if flg & WSATTR_WSCOLORS == 0 {
        fg = WS_DEFAULT_FG;
        bg = WS_DEFAULT_BG;
    }

    if flg & WSATTR_REVERSE != 0 {
        core::mem::swap(&mut fg, &mut bg);
    }

    // Bold is only supported for ANSI colors 0 - 7.
    if flg & WSATTR_HILIT != 0 && fg < 8 {
        fg += 8;
    }

    Ok(((bg << 16) | (fg << 24) | (flg & WSATTR_UNDERLINE)) as u32)
}

/// `rasops_pack_mattr`: pack a mono attribute.
///
/// # Safety
///
/// None beyond the emulops' contract: the cookie is not used.
pub unsafe fn rasops_pack_mattr(
    _cookie: *mut c_void,
    _fg: i32,
    _bg: i32,
    flg: i32,
) -> Result<u32, Errno> {
    if flg & (WSATTR_BLINK | WSATTR_HILIT | WSATTR_WSCOLORS) != 0 {
        return Err(Errno::EINVAL);
    }

    let (mut fg, mut bg) = (1, 0);

    if flg & WSATTR_REVERSE != 0 {
        core::mem::swap(&mut fg, &mut bg);
    }

    Ok(((bg << 16) | (fg << 24) | (flg & WSATTR_UNDERLINE)) as u32)
}

/// `rasops_copyrows`: copy rows.
///
/// # Safety
///
/// `cookie` is a configured [`RasopsInfo`] whose frame buffer is mapped; the rows are on
/// the screen (without `RASOPS_CLIPPING` the C does not check them either).
pub unsafe fn rasops_copyrows(
    cookie: *mut c_void,
    mut src: i32,
    mut dst: i32,
    mut num: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(cookie) };

    if RASOPS_CLIPPING {
        if dst == src {
            return Ok(());
        }
        if src < 0 {
            num += src;
            src = 0;
        }
        if src + num > ri.ri_rows.get() {
            num = ri.ri_rows.get() - src;
        }
        if dst < 0 {
            num += dst;
            dst = 0;
        }
        if dst + num > ri.ri_rows.get() {
            num = ri.ri_rows.get() - dst;
        }
        if num <= 0 {
            return Ok(());
        }
    }

    let fh = ri.font().fontheight as i32;
    num *= fh;
    let nwords = (ri.ri_emustride.get() >> 2) as usize;
    let stride = ri.ri_stride.get() as isize;
    let bits = ri.ri_bits.get();

    // SAFETY: the rows are on the screen (the caller's contract); every word copied is
    // inside the emulated area, which is inside the frame buffer.
    unsafe {
        let (mut srp, mut drp, delta) = if dst < src {
            (
                bits.offset((src * ri.ri_yscale.get()) as isize),
                bits.offset((dst * ri.ri_yscale.get()) as isize),
                stride,
            )
        } else {
            src = fh * src + num - 1;
            dst = fh * dst + num - 1;
            (
                bits.offset(src as isize * stride),
                bits.offset(dst as isize * stride),
                -stride,
            )
        };

        while num > 0 {
            num -= 1;
            for w in 0..nwords {
                fb_w32(drp.add(w * 4), fb_r32(srp.add(w * 4)));
            }
            drp = drp.offset(delta);
            srp = srp.offset(delta);
        }
    }

    Ok(())
}

/// `rasops_copycols`: copy columns. This is slow, and hard to optimize due to alignment, and
/// the fact that we have to copy both left->right and right->left. We simply cop-out here
/// and use either memmove() or slow_bcopy(), since they handle all of these cases anyway.
///
/// # Safety
///
/// As for [`rasops_copyrows`].
pub unsafe fn rasops_copycols(
    cookie: *mut c_void,
    mut row: i32,
    mut src: i32,
    mut dst: i32,
    mut num: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(cookie) };

    if RASOPS_CLIPPING {
        if dst == src {
            return Ok(());
        }
        // Catches < 0 case too
        if row as u32 >= ri.ri_rows.get() as u32 {
            return Ok(());
        }
        if src < 0 {
            num += src;
            src = 0;
        }
        if src + num > ri.ri_cols.get() {
            num = ri.ri_cols.get() - src;
        }
        if dst < 0 {
            num += dst;
            dst = 0;
        }
        if dst + num > ri.ri_cols.get() {
            num = ri.ri_cols.get() - dst;
        }
        if num <= 0 {
            return Ok(());
        }
    }

    num *= ri.ri_xscale.get();
    row *= ri.ri_yscale.get();
    let height = ri.font().fontheight;
    let stride = ri.ri_stride.get() as usize;

    // SAFETY: the cells are on the screen (the caller's contract).
    unsafe {
        let mut sp = ri
            .ri_bits
            .get()
            .add((row + src * ri.ri_xscale.get()) as usize);
        let mut dp = ri
            .ri_bits
            .get()
            .add((row + dst * ri.ri_xscale.get()) as usize);

        for _ in 0..height {
            if NRASOPS_BSWAP && ri.ri_flg.get() & RI_BSWAP != 0 {
                slow_bcopy(sp, dp, num as usize);
            } else {
                ptr::copy(sp, dp, num as usize);
            }
            dp = dp.add(stride);
            sp = sp.add(stride);
        }
    }

    Ok(())
}

/// `rasops_cursor`: turn cursor off/on.
///
/// # Safety
///
/// As for [`rasops_copyrows`].
pub unsafe fn rasops_cursor(cookie: *mut c_void, on: i32, row: i32, col: i32) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(cookie) };

    // Turn old cursor off
    if ri.ri_flg.get() & RI_CURSOR != 0 {
        if !RASOPS_CLIPPING || ri.ri_flg.get() & RI_CURSORCLIP == 0 {
            ri.ri_do_cursor()?;
        }
        ri.ri_flg.set(ri.ri_flg.get() & !RI_CURSOR);
    }

    // Select new cursor
    if RASOPS_CLIPPING {
        ri.ri_flg.set(ri.ri_flg.get() & !RI_CURSORCLIP);

        if row < 0 || row >= ri.ri_rows.get() || col < 0 || col >= ri.ri_cols.get() {
            ri.ri_flg.set(ri.ri_flg.get() | RI_CURSORCLIP);
        }
    }
    ri.ri_crow.set(row);
    ri.ri_ccol.set(col);

    if let Some(update) = ri.ri_updatecursor.get() {
        update(ri);
    }

    if on != 0 {
        if !RASOPS_CLIPPING || ri.ri_flg.get() & RI_CURSORCLIP == 0 {
            ri.ri_do_cursor()?;
        }
        ri.ri_flg.set(ri.ri_flg.get() | RI_CURSOR);
    }

    Ok(())
}

/// `rasops_init_devcmap`: make the device colormap.
pub fn rasops_init_devcmap(ri: &RasopsInfo) {
    if ri.ri_depth.get() == 1 || ri.ri_flg.get() & RI_FORCEMONO != 0 {
        ri.ri_devcmap[0].set(0);
        for c in &ri.ri_devcmap[1..] {
            c.set(-1); // 0xffffffff
        }
        return;
    }

    match ri.ri_depth.get() {
        4 => {
            for (i, cell) in ri.ri_devcmap.iter().enumerate() {
                let c = (i | (i << 4)) as u32;
                cell.set((c | (c << 8) | (c << 16) | (c << 24)) as i32);
            }
            return;
        }
        8 => {
            for (i, cell) in ri.ri_devcmap.iter().enumerate() {
                let i = i as u32;
                cell.set((i | (i << 8) | (i << 16) | (i << 24)) as i32);
            }
            return;
        }
        _ => {}
    }

    let chan = |v: u8, num: u8, pos: u8| -> u32 {
        let v = u32::from(v);
        if num <= 8 {
            (v >> (8 - num)) << pos
        } else {
            (v << (num - 8)) << pos
        }
    };

    for (i, cell) in ri.ri_devcmap.iter().enumerate() {
        let p = &RASOPS_CMAP[i * 3..i * 3 + 3];
        let mut c = chan(p[0], ri.ri_rnum.get(), ri.ri_rpos.get());
        c |= chan(p[1], ri.ri_gnum.get(), ri.ri_gpos.get());
        c |= chan(p[2], ri.ri_bnum.get(), ri.ri_bpos.get());

        // Fill the word for generic routines, which want this
        if ri.ri_depth.get() == 24 {
            c |= (c & 0xff) << 24;
        } else if ri.ri_depth.get() <= 16 {
            c |= c << 16;
        }

        // 24bpp does bswap on the fly. {32,16,15}bpp do it here.
        let v = if !NRASOPS_BSWAP || ri.ri_flg.get() & RI_BSWAP == 0 {
            c
        } else if ri.ri_depth.get() == 32 {
            c.swap_bytes()
        } else if ri.ri_depth.get() == 16 || ri.ri_depth.get() == 15 {
            // swap16 of the low half, as the C's assignment keeps it
            u32::from((c as u16).swap_bytes())
        } else {
            c
        };
        cell.set(v as i32);
    }
}

/// `rasops_unpack_attr`: unpack a rasops attribute into `(fg, bg, underline)`.
///
/// # Safety
///
/// None beyond the emulops' contract: the cookie is not used.
pub unsafe fn rasops_unpack_attr(_cookie: *mut c_void, attr: u32) -> (i32, i32, i32) {
    (
        ((attr >> 24) & 0xf) as i32,
        ((attr >> 16) & 0xf) as i32,
        (attr & WSATTR_UNDERLINE as u32) as i32,
    )
}

/// `rasops_eraserows`: erase rows.
///
/// # Safety
///
/// As for [`rasops_copyrows`].
pub unsafe fn rasops_eraserows(
    cookie: *mut c_void,
    mut row: i32,
    mut num: i32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(cookie) };

    if RASOPS_CLIPPING {
        if row < 0 {
            num += row;
            row = 0;
        }
        if row + num > ri.ri_rows.get() {
            num = ri.ri_rows.get() - row;
        }
        if num <= 0 {
            return Ok(());
        }
    }

    let clr = ri.ri_devcmap[((attr >> 16) & 0xf) as usize].get();

    // XXX The wsdisplay_emulops interface seems a little deficient in that there is no way
    // to clear the *entire* screen. We provide a workaround here: if the entire console area
    // is being cleared, and the RI_FULLCLEAR flag is set, clear the entire display.
    let (nwords, lines, mut dp, delta) =
        if num == ri.ri_rows.get() && ri.ri_flg.get() & RI_FULLCLEAR != 0 {
            (
                (ri.ri_stride.get() >> 2) as usize,
                ri.ri_height.get(),
                ri.ri_origbits.get(),
                0,
            )
        } else {
            // SAFETY: the rows are on the screen (the caller's contract).
            let dp = unsafe { ri.ri_bits.get().add((row * ri.ri_yscale.get()) as usize) };
            (
                (ri.ri_emustride.get() >> 2) as usize,
                num * ri.font().fontheight as i32,
                dp,
                ri.ri_delta.get(),
            )
        };

    // SAFETY: each line is `nwords` words of the frame buffer, `nwords * 4 + delta` bytes
    // apart (the whole stride, or the emulated stride plus the margin).
    unsafe {
        for _ in 0..lines {
            for w in 0..nwords {
                fb_w32(dp.add(w * 4), clr);
            }
            dp = dp.add(nwords * 4).offset(delta as isize);
        }
    }

    Ok(())
}

/// `rasops_do_cursor`: actually turn the cursor on or off. This does the dirty work for
/// `rasops_cursor()`.
pub fn rasops_do_cursor(ri: &RasopsInfo) -> Result<(), Errno> {
    let (row, col) = if ri.ri_flg.get() & RI_ROTATE_CW != 0 {
        // Rotate rows/columns
        (ri.ri_ccol.get(), ri.ri_rows.get() - ri.ri_crow.get() - 1)
    } else if ri.ri_flg.get() & RI_ROTATE_CCW != 0 {
        // Rotate rows/columns
        (ri.ri_cols.get() - ri.ri_ccol.get() - 1, ri.ri_crow.get())
    } else {
        (ri.ri_crow.get(), ri.ri_ccol.get())
    };

    let xscale = ri.ri_xscale.get();
    // SAFETY: the cursor cell is on the screen (rasops_cursor's caller's contract).
    let mut rp = unsafe {
        ri.ri_bits
            .get()
            .add((row * ri.ri_yscale.get() + col * xscale) as usize)
    };
    let height = ri.font().fontheight;
    let mut slop1 = (4 - (rp as usize & 3) as i32) & 3;

    if slop1 > xscale {
        slop1 = xscale;
    }

    let slop2 = (xscale - slop1) & 3;
    let full1 = (xscale - slop1 - slop2) >> 2;

    // XXX this is stupid.. use masks instead (the C has a separate loop for the common
    // aligned case, where both slops are 0; this one does the same then)
    // SAFETY: every byte inverted is in the cursor's cell, inside the frame buffer.
    unsafe {
        for _ in 0..height {
            let mut dp = rp;
            rp = rp.add(ri.ri_stride.get() as usize);

            if slop1 & 1 != 0 {
                fb_w8(dp, !fb_r8(dp));
                dp = dp.add(1);
            }

            if slop1 & 2 != 0 {
                fb_w16(dp, !fb_r16(dp));
                dp = dp.add(2);
            }

            for _ in 0..full1 {
                fb_w32(dp, !fb_r32(dp));
                dp = dp.add(4);
            }

            if slop2 & 1 != 0 {
                fb_w8(dp, !fb_r8(dp));
                dp = dp.add(1);
            }

            if slop2 & 2 != 0 {
                fb_w16(dp, !fb_r16(dp));
            }
        }
    }

    Ok(())
}

/// `rasops_erasecols`: erase columns.
///
/// # Safety
///
/// As for [`rasops_copyrows`].
pub unsafe fn rasops_erasecols(
    cookie: *mut c_void,
    row: i32,
    mut col: i32,
    mut num: i32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(cookie) };

    if RASOPS_CLIPPING {
        if row as u32 >= ri.ri_rows.get() as u32 {
            return Ok(());
        }
        if col < 0 {
            num += col;
            col = 0;
        }
        if col + num > ri.ri_cols.get() {
            num = ri.ri_cols.get() - col;
        }
        if num <= 0 {
            return Ok(());
        }
    }

    num *= ri.ri_xscale.get();
    let stride = ri.ri_stride.get() as usize;
    let xscale = ri.ri_xscale.get();
    // SAFETY: the cells are on the screen (the caller's contract).
    let mut rp = unsafe {
        ri.ri_bits
            .get()
            .add((row * ri.ri_yscale.get() + col * xscale) as usize)
    };
    let height = ri.font().fontheight;
    let clr = ri.ri_devcmap[((attr >> 16) & 0xf) as usize].get();

    // SAFETY: every store is inside the cells being erased.
    unsafe {
        // Don't bother using the full loop for <= 32 pels
        if num <= 32 {
            if (num | xscale) & 3 == 0 {
                // Word aligned blt
                for _ in 0..height {
                    for w in 0..(num >> 2) as usize {
                        fb_w32(rp.add(w * 4), clr);
                    }
                    rp = rp.add(stride);
                }
            } else if (num | xscale) & 1 == 0 {
                // Halfword aligned blt. This is needed so the 15/16 bit ops can use this
                // function.
                for _ in 0..height {
                    for h in 0..(num >> 1) as usize {
                        fb_w16(rp.add(h * 2), clr as i16);
                    }
                    rp = rp.add(stride);
                }
            } else {
                for _ in 0..height {
                    for b in 0..num as usize {
                        fb_w8(rp.add(b), clr as u8);
                    }
                    rp = rp.add(stride);
                }
            }

            return Ok(());
        }

        let slop1 = (4 - (rp as usize & 3) as i32) & 3;
        let slop2 = (num - slop1) & 3;
        num -= slop1 + slop2;
        let nwords = (num >> 2) as usize;

        for _ in 0..height {
            let mut dp = rp;
            rp = rp.add(stride);

            // Align span to 4 bytes
            if slop1 & 1 != 0 {
                fb_w8(dp, clr as u8);
                dp = dp.add(1);
            }

            if slop1 & 2 != 0 {
                fb_w16(dp, clr as i16);
                dp = dp.add(2);
            }

            // Write 4 bytes per loop (32 bytes per loop in the C's unrolled form)
            for _ in 0..nwords {
                fb_w32(dp, clr);
                dp = dp.add(4);
            }

            // Write unaligned trailing slop
            if slop2 & 1 != 0 {
                fb_w8(dp, clr as u8);
                dp = dp.add(1);
            }

            if slop2 & 2 != 0 {
                fb_w16(dp, clr as i16);
            }
        }
    }

    Ok(())
}

/*
 * Quarter clockwise rotation routines (originally intended for the built-in Zaurus C3x00
 * display in 16bpp).
 */

/// `rasops_rotate_font`: the cookie of the rotated version of font `cookie`, made by wsfont
/// the first time.
fn rasops_rotate_font(cookie: &mut i32, ccw: bool) {
    let mut f = ROTATEDFONTS.load(Ordering::Acquire);
    while let Some(rf) = NonNull::new(f) {
        // SAFETY: the entries are malloc'd below and never freed.
        let rf = unsafe { rf.as_ref() };
        if rf.rf_cookie == *cookie {
            *cookie = rf.rf_rotated;
            return;
        }
        f = rf.rf_next;
    }

    // We did not find a rotated version of this font. Ask the wsfont code to compute one
    // for us.
    let Some(ncookie) = wsfont_rotate(*cookie, ccw) else {
        return;
    };

    if let Some(mem) = malloc(size_of::<Rotatedfont>(), M_DEVBUF, M_WAITOK) {
        let rf = mem.cast::<Rotatedfont>().as_ptr();
        // SAFETY: a fresh allocation of one entry, malloc(9)-aligned, kept for good.
        unsafe {
            ptr::write(
                rf,
                Rotatedfont {
                    rf_next: ROTATEDFONTS.load(Ordering::Acquire),
                    rf_cookie: *cookie,
                    rf_rotated: ncookie,
                },
            )
        };
        ROTATEDFONTS.store(rf, Ordering::Release);
    }

    *cookie = ncookie;
}

/// `rasops_copychar`: copy one character cell of a rotated display.
///
/// # Safety
///
/// As for [`rasops_copyrows`].
unsafe fn rasops_copychar(cookie: *mut c_void, srcrow: i32, dstrow: i32, srccol: i32, dstcol: i32) {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(cookie) };

    let r_srcrow = srccol * ri.ri_yscale.get();
    let r_dstrow = dstcol * ri.ri_yscale.get();
    let r_srccol = ri.ri_rows.get() - srcrow - 1;
    let r_dstcol = ri.ri_rows.get() - dstrow - 1;
    let height = ri.font().fontheight;
    let xscale = ri.ri_xscale.get();
    let stride = ri.ri_stride.get() as usize;

    // SAFETY: both cells are on the screen (the caller's contract).
    unsafe {
        let mut sp = ri
            .ri_bits
            .get()
            .add((r_srcrow + r_srccol * xscale) as usize);
        let mut dp = ri
            .ri_bits
            .get()
            .add((r_dstrow + r_dstcol * xscale) as usize);

        for _ in 0..height {
            if NRASOPS_BSWAP && ri.ri_flg.get() & RI_BSWAP != 0 {
                slow_bcopy(sp, dp, xscale as usize);
            } else {
                ptr::copy(sp, dp, xscale as usize);
            }
            dp = dp.add(stride);
            sp = sp.add(stride);
        }
    }
}

/// `rasops_putchar_rotated`.
///
/// # Safety
///
/// As for [`rasops_copyrows`].
pub unsafe fn rasops_putchar_rotated(
    cookie: *mut c_void,
    mut row: i32,
    mut col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(cookie) };

    if ri.ri_flg.get() & RI_ROTATE_CW != 0 {
        row = ri.ri_rows.get() - row - 1;
    } else {
        col = ri.ri_cols.get() - col - 1;
    }

    // Do rotated char sans (side)underline
    let Some(putchar) = ri.ri_real_ops.get().putchar else {
        panic(format_args!("rasops: ri_real_ops.putchar is NULL"));
    };
    // SAFETY: the real operations are the depth's, over the same rasops_info.
    unsafe { putchar(cookie, col, row, uc, attr & !(WSATTR_UNDERLINE as u32))? };

    // Do rotated underline
    let font = ri.font();
    // SAFETY: the cell is on the screen (the caller's contract).
    let mut rp = unsafe {
        ri.ri_bits
            .get()
            .add((col * ri.ri_yscale.get() + row * ri.ri_xscale.get()) as usize)
    };
    if ri.ri_flg.get() & RI_ROTATE_CCW != 0 {
        // SAFETY: the last pel of the cell's first line.
        rp = unsafe { rp.add(((font.fontwidth as i32 - 1) * ri.ri_pelbytes.get()) as usize) };
    }
    let height = font.fontheight;

    // XXX this assumes 16-bit color depth
    if attr & WSATTR_UNDERLINE as u32 != 0 {
        let c = ri.ri_devcmap[((attr >> 24) & 0xf) as usize].get() as i16;

        for _ in 0..height {
            // SAFETY: one pel per line of the cell.
            unsafe {
                fb_w16(rp, c);
                rp = rp.add(ri.ri_stride.get() as usize);
            }
        }
    }

    Ok(())
}

/// `rasops_erasecols_rotated`.
///
/// # Safety
///
/// As for [`rasops_copyrows`].
pub unsafe fn rasops_erasecols_rotated(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    num: i32,
    attr: u32,
) -> Result<(), Errno> {
    for i in col..col + num {
        // SAFETY: the caller's contract.
        unsafe { rasops_putchar_rotated(cookie, row, i, u32::from(b' '), attr)? };
    }

    Ok(())
}

/// `rasops_copyrows_rotated`. XXX: these could likely be optimised somewhat.
///
/// # Safety
///
/// As for [`rasops_copyrows`].
pub unsafe fn rasops_copyrows_rotated(
    cookie: *mut c_void,
    src: i32,
    dst: i32,
    num: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let cols = unsafe { ri_of(cookie) }.ri_cols.get();

    let mut copy = |roff: i32| {
        for col in 0..cols {
            // SAFETY: the caller's contract.
            unsafe { rasops_copychar(cookie, src + roff, dst + roff, col, col) };
        }
    };
    if src > dst {
        (0..num).for_each(&mut copy);
    } else {
        (0..num).rev().for_each(&mut copy);
    }

    Ok(())
}

/// `rasops_copycols_rotated`.
///
/// # Safety
///
/// As for [`rasops_copyrows`].
pub unsafe fn rasops_copycols_rotated(
    cookie: *mut c_void,
    row: i32,
    src: i32,
    dst: i32,
    num: i32,
) -> Result<(), Errno> {
    let copy = |coff: i32| {
        // SAFETY: the caller's contract.
        unsafe { rasops_copychar(cookie, row, row, src + coff, dst + coff) };
    };
    if src > dst {
        (0..num).for_each(copy);
    } else {
        (0..num).rev().for_each(copy);
    }

    Ok(())
}

/// `rasops_eraserows_rotated`.
///
/// # Safety
///
/// As for [`rasops_copyrows`].
pub unsafe fn rasops_eraserows_rotated(
    cookie: *mut c_void,
    row: i32,
    num: i32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let cols = unsafe { ri_of(cookie) }.ri_cols.get();

    for rn in row..row + num {
        for col in 0..cols {
            // SAFETY: the caller's contract.
            unsafe { rasops_putchar_rotated(cookie, rn, col, u32::from(b' '), attr)? };
        }
    }

    Ok(())
}

/// `slow_bcopy`: strictly byte-only bcopy() version, to be used with `RI_BSWAP`, as the
/// regular bcopy() may want to optimize things by doing larger-than-byte reads or write.
/// This may confuse things if src and dst have different alignments.
///
/// # Safety
///
/// `s` and `d` are `len` bytes of the frame buffer each.
unsafe fn slow_bcopy(s: *const u8, d: *mut u8, len: usize) {
    // SAFETY: the caller's contract; byte by byte, in the direction that survives overlap.
    unsafe {
        if d as usize <= s as usize {
            for i in 0..len {
                fb_w8(d.add(i), fb_r8(s.add(i)));
            }
        } else {
            for i in (0..len).rev() {
                fb_w8(d.add(i), fb_r8(s.add(i)));
            }
        }
    }
}

/// `rasops_alloc_screen`: a new virtual screen with its backing store (and scrollback),
/// filled with blanks (or with the current contents for the first, visible one).
///
/// # Safety
///
/// `v` is a configured `&'static` [`RasopsInfo`] (the access cookie rasops's drivers pass).
pub unsafe fn rasops_alloc_screen(
    v: *mut c_void,
    _type: *const WsscreenDescr,
    cookiep: &mut *mut c_void,
    curxp: &mut i32,
    curyp: &mut i32,
    attrp: &mut u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri: &'static RasopsInfo = unsafe { ri_of(v) };

    let mem = malloc(size_of::<RasopsScreen>(), M_DEVBUF, M_NOWAIT).ok_or(Errno::ENOMEM)?;

    let sbscreens = RS_SCROLLBACK_SCREENS;
    let (rows, cols) = (ri.ri_rows.get(), ri.ri_cols.get());
    let cells = (rows * (sbscreens + 1)) as usize;
    let Some(bs) = mallocarray(
        cells,
        cols as usize * size_of::<WsdisplayCharcell>(),
        M_DEVBUF,
        M_NOWAIT,
    ) else {
        free(mem, M_DEVBUF, size_of::<RasopsScreen>());
        return Err(Errno::ENOMEM);
    };
    let dispoffset = rows * sbscreens * cols;

    *curxp = 0;
    *curyp = 0;
    *attrp = ri.ri_pack_attr(0, 0, 0)?;

    let scr_ptr = mem.cast::<RasopsScreen>().as_ptr();
    // SAFETY: a fresh allocation of one screen, malloc(9)-aligned; screens live until
    // rasops_free_screen.
    unsafe {
        ptr::write(
            scr_ptr,
            RasopsScreen {
                rs_next: ListEntry::new(),
                rs_ri: ri,
                rs_bs: bs.cast::<WsdisplayCharcell>().as_ptr(),
                rs_visible: Cell::new(i32::from(ri.ri_nscreens.get() == 0)),
                rs_crow: Cell::new(-1),
                rs_ccol: Cell::new(-1),
                rs_defattr: Cell::new(*attrp),
                rs_sbscreens: sbscreens,
                rs_dispoffset: dispoffset,
                rs_visibleoffset: Cell::new(dispoffset),
            },
        )
    };
    // SAFETY: just written.
    let scr = unsafe { &*scr_ptr };
    *cookiep = scr_ptr.cast();

    for i in 0..dispoffset {
        scr.set_bs(i, u32::from(b' '), scr.rs_defattr.get());
    }

    if !ri.ri_bs.get().is_null() && scr.rs_visible.get() != 0 {
        // SAFETY: `ri_bs` holds the screen's `rows * cols` cells; the store has room for
        // them after the scrollback; the two allocations do not overlap.
        unsafe {
            ptr::copy_nonoverlapping(
                ri.ri_bs.get(),
                scr.rs_bs.add(dispoffset as usize),
                (rows * cols) as usize,
            )
        };
    } else {
        for i in 0..rows * cols {
            scr.set_bs(dispoffset + i, u32::from(b' '), scr.rs_defattr.get());
        }
    }

    // SAFETY: the screen stays in place until rasops_free_screen unlinks it.
    unsafe { ri.ri_screens.insert_head(scr) };
    ri.ri_nscreens.set(ri.ri_nscreens.get() + 1);

    Ok(())
}

/// `rasops_free_screen`.
///
/// # Safety
///
/// `v` is the [`RasopsInfo`] and `cookie` a screen of it that is not the active one and is
/// not used again.
pub unsafe fn rasops_free_screen(v: *mut c_void, cookie: *mut c_void) {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(v) };
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(cookie) };
    let ncells = scr.ncells();

    // SAFETY: the screen is linked (rasops_alloc_screen) and is unlinked once.
    unsafe { ListHead::<RsNext>::remove(scr) };
    ri.ri_nscreens.set(ri.ri_nscreens.get() - 1);

    if let Some(bs) = NonNull::new(scr.rs_bs.cast::<u8>()) {
        free(bs, M_DEVBUF, ncells * size_of::<WsdisplayCharcell>());
    }
    if let Some(p) = NonNull::new(cookie.cast::<u8>()) {
        free(p, M_DEVBUF, size_of::<RasopsScreen>());
    }
}

/// `rasops_show_screen`: switch to screen `cookie`, at once or (with `cb`) from a task;
/// `Err(EAGAIN)` when the task will do it.
///
/// # Safety
///
/// As for [`rasops_free_screen`], except that `cookie` stays in use.
pub unsafe fn rasops_show_screen(
    v: *mut c_void,
    cookie: *mut c_void,
    _waitok: i32,
    cb: Option<ShowScreenCb>,
    cbarg: *mut c_void,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract; a rasops_info set up by rasops_init is 'static.
    let ri: &'static RasopsInfo = unsafe { ri_of(v) };

    ri.ri_switchcookie.set(cookie);
    if let Some(cb) = cb {
        ri.ri_switchcb.set(Some(cb));
        ri.ri_switchcbarg.set(cbarg);
        task_add(SYSTQ, &ri.ri_switchtask);
        return Err(Errno::EAGAIN);
    }

    rasops_doswitch(v);
    Ok(())
}

/// `rasops_doswitch`: make `ri_switchcookie` the visible screen (the task of
/// `rasops_show_screen`).
fn rasops_doswitch(v: *mut c_void) {
    // SAFETY: the task's argument and show_screen's cookie are the rasops_info.
    let ri = unsafe { ri_of(v) };
    // SAFETY: the switch cookie is a live screen of `ri` (rasops_show_screen's contract).
    let scr = unsafe { scr_of(ri.ri_switchcookie.get()) };

    // SAFETY: `v` is the rasops_info.
    let _ = unsafe { rasops_cursor(v, 0, 0, 0) };
    // SAFETY: the active screen is live.
    unsafe { scr_of(ri.ri_active.get().cast()) }
        .rs_visible
        .set(0);
    let _ = ri.ri_eraserows(0, ri.ri_rows.get(), scr.rs_defattr.get());
    ri.ri_active.set(ptr::from_ref(scr).cast_mut());
    // SAFETY: inside the screen's store, as in rasops_init.
    ri.ri_bs
        .set(unsafe { scr.rs_bs.add(scr.rs_dispoffset as usize) });
    scr.rs_visible.set(1);
    scr.rs_visibleoffset.set(scr.rs_dispoffset);
    let cols = scr.rs_ri.ri_cols.get();
    for row in 0..ri.ri_rows.get() {
        for col in 0..ri.ri_cols.get() {
            let cell = scr.bs(row * cols + col + scr.rs_visibleoffset.get());
            let _ = ri.ri_putchar(row, col, cell.uc, cell.attr);
        }
    }
    if scr.rs_crow.get() != -1 {
        // SAFETY: `v` is the rasops_info.
        let _ = unsafe { rasops_cursor(v, 1, scr.rs_crow.get(), scr.rs_ccol.get()) };
    }

    if let Some(cb) = ri.ri_switchcb.get() {
        cb(ri.ri_switchcbarg.get(), 0, 0);
        ri.ri_switchcb.set(None);
        ri.ri_switchcbarg.set(ptr::null_mut());
    }
}

/// `rasops_getchar`: the cell at (`row`, `col`) of the active screen.
///
/// # Safety
///
/// `v` is a [`RasopsInfo`]; the cell is on the screen.
pub unsafe fn rasops_getchar(v: *mut c_void, row: i32, col: i32) -> Option<WsdisplayCharcell> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(v) };
    let scr = ri.ri_active.get();

    if scr.is_null() {
        return None;
    }
    // SAFETY: the active screen is live.
    let scr = unsafe { &*scr };
    if scr.rs_bs.is_null() {
        return None;
    }

    Some(scr.bs(row * ri.ri_cols.get() + col + scr.rs_dispoffset))
}

/// `rasops_vcons_cursor`.
///
/// # Safety
///
/// `cookie` is a live [`RasopsScreen`].
pub unsafe fn rasops_vcons_cursor(
    cookie: *mut c_void,
    on: i32,
    row: i32,
    col: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(cookie) };

    scr.rs_crow.set(if on != 0 { row } else { -1 });
    scr.rs_ccol.set(if on != 0 { col } else { -1 });

    if scr.rs_visible.get() == 0 {
        return Ok(());
    }

    // SAFETY: the screen's rasops_info.
    unsafe { rasops_cursor(scr.rs_ri.cookie(), on, row, col) }
}

/// `rasops_vcons_mapchar`.
///
/// # Safety
///
/// As for [`rasops_vcons_cursor`].
pub unsafe fn rasops_vcons_mapchar(cookie: *mut c_void, c: i32, cp: &mut u32) -> i32 {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(cookie) };

    // SAFETY: the screen's rasops_info.
    unsafe { rasops_mapchar(scr.rs_ri.cookie(), c, cp) }
}

/// `rasops_vcons_putchar`.
///
/// # Safety
///
/// As for [`rasops_vcons_cursor`]; the cell is on the screen.
pub unsafe fn rasops_vcons_putchar(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(cookie) };
    let off = row * scr.rs_ri.ri_cols.get() + col + scr.rs_dispoffset;

    if scr.rs_visible.get() != 0 && scr.rs_visibleoffset.get() != scr.rs_dispoffset {
        // SAFETY: the screen and its rasops_info.
        unsafe { rasops_scrollback(scr.rs_ri.cookie(), cookie, 0) };
    }

    scr.set_bs(off, uc, attr);

    if scr.rs_visible.get() == 0 {
        return Ok(());
    }

    scr.rs_ri.ri_putchar(row, col, uc, attr)
}

/// `rasops_vcons_copycols`.
///
/// # Safety
///
/// As for [`rasops_vcons_putchar`].
pub unsafe fn rasops_vcons_copycols(
    cookie: *mut c_void,
    row: i32,
    src: i32,
    dst: i32,
    num: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(cookie) };
    let ri = scr.rs_ri;
    let cols = ri.ri_cols.get();

    scr.move_bs(
        row * cols + dst + scr.rs_dispoffset,
        row * cols + src + scr.rs_dispoffset,
        num,
    );

    if scr.rs_visible.get() == 0 {
        return Ok(());
    }

    if ri.ri_flg.get() & RI_WRONLY == 0 {
        return ri.ri_copycols(row, src, dst, num);
    }

    for col in dst..dst + num {
        let cell = scr.bs(row * cols + col + scr.rs_dispoffset);
        ri.ri_putchar(row, col, cell.uc, cell.attr)?;
    }

    Ok(())
}

/// `rasops_vcons_erasecols`.
///
/// # Safety
///
/// As for [`rasops_vcons_putchar`].
pub unsafe fn rasops_vcons_erasecols(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    num: i32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(cookie) };
    let cols = scr.rs_ri.ri_cols.get();

    for i in 0..num {
        scr.set_bs(
            row * cols + col + i + scr.rs_dispoffset,
            u32::from(b' '),
            attr,
        );
    }

    if scr.rs_visible.get() == 0 {
        return Ok(());
    }

    scr.rs_ri.ri_erasecols(row, col, num, attr)
}

/// `rasops_vcons_copyrows`.
///
/// # Safety
///
/// As for [`rasops_vcons_putchar`].
pub unsafe fn rasops_vcons_copyrows(
    cookie: *mut c_void,
    src: i32,
    dst: i32,
    num: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(cookie) };
    let ri = scr.rs_ri;
    let cols = ri.ri_cols.get();

    // update the scrollback buffer if the entire screen is moving
    if dst == 0 && src + num == ri.ri_rows.get() && scr.rs_sbscreens > 0 {
        scr.move_bs(dst, src * cols, ri.ri_rows.get() * scr.rs_sbscreens * cols);
    }

    // copy everything
    if ri.ri_flg.get() & RI_WRONLY == 0 || scr.rs_visible.get() == 0 {
        scr.move_bs(
            dst * cols + scr.rs_dispoffset,
            src * cols + scr.rs_dispoffset,
            num * cols,
        );

        if scr.rs_visible.get() == 0 {
            return Ok(());
        }

        return ri.ri_copyrows(src, dst, num);
    }

    // smart update, only redraw characters that are different
    let srcofs = (src - dst) * cols;

    for mv in 0..num {
        let row = if srcofs > 0 {
            dst + mv
        } else {
            dst + num - 1 - mv
        };
        for col in 0..cols {
            let off = row * cols + col + scr.rs_dispoffset;
            let new = scr.bs(off + srcofs);

            if scr.bs(off) == new {
                continue;
            }
            scr.set_bs(off, new.uc, new.attr);
            ri.ri_putchar(row, col, new.uc, new.attr)?;
        }
    }

    Ok(())
}

/// `rasops_vcons_eraserows`.
///
/// # Safety
///
/// As for [`rasops_vcons_putchar`].
pub unsafe fn rasops_vcons_eraserows(
    cookie: *mut c_void,
    row: i32,
    num: i32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(cookie) };
    let cols = scr.rs_ri.ri_cols.get();

    for i in 0..num * cols {
        scr.set_bs(row * cols + i + scr.rs_dispoffset, u32::from(b' '), attr);
    }

    if scr.rs_visible.get() == 0 {
        return Ok(());
    }

    scr.rs_ri.ri_eraserows(row, num, attr)
}

/// `rasops_vcons_pack_attr`.
///
/// # Safety
///
/// As for [`rasops_vcons_cursor`].
pub unsafe fn rasops_vcons_pack_attr(
    cookie: *mut c_void,
    fg: i32,
    bg: i32,
    flg: i32,
) -> Result<u32, Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(cookie) };

    scr.rs_ri.ri_pack_attr(fg, bg, flg)
}

/// `rasops_vcons_unpack_attr`.
///
/// # Safety
///
/// As for [`rasops_vcons_cursor`].
pub unsafe fn rasops_vcons_unpack_attr(cookie: *mut c_void, attr: u32) -> (i32, i32, i32) {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(cookie) };

    // SAFETY: the screen's rasops_info (the cookie is not used anyway).
    unsafe { rasops_unpack_attr(scr.rs_ri.cookie(), attr) }
}

/// `rasops_wronly_putchar`.
///
/// # Safety
///
/// `cookie` is a configured [`RasopsInfo`] with a backing store; the cell is on the screen.
pub unsafe fn rasops_wronly_putchar(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(cookie) };

    ri.set_bs(row * ri.ri_cols.get() + col, uc, attr);

    ri.ri_putchar(row, col, uc, attr)
}

/// `rasops_wronly_copycols`.
///
/// # Safety
///
/// As for [`rasops_wronly_putchar`].
pub unsafe fn rasops_wronly_copycols(
    cookie: *mut c_void,
    row: i32,
    src: i32,
    dst: i32,
    num: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(cookie) };
    let cols = ri.ri_cols.get();

    if num > 0 {
        // SAFETY: both ranges are cells of `row` in the store; `ptr::copy` allows overlap.
        unsafe {
            ptr::copy(
                ri.ri_bs.get().add((row * cols + src) as usize),
                ri.ri_bs.get().add((row * cols + dst) as usize),
                num as usize,
            )
        };
    }

    for col in dst..dst + num {
        let cell = ri.bs(row * cols + col);
        ri.ri_putchar(row, col, cell.uc, cell.attr)?;
    }

    Ok(())
}

/// `rasops_wronly_erasecols`.
///
/// # Safety
///
/// As for [`rasops_wronly_putchar`].
pub unsafe fn rasops_wronly_erasecols(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    num: i32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(cookie) };
    let cols = ri.ri_cols.get();

    for i in 0..num {
        ri.set_bs(row * cols + col + i, u32::from(b' '), attr);
    }

    ri.ri_erasecols(row, col, num, attr)
}

/// `rasops_wronly_copyrows`.
///
/// # Safety
///
/// As for [`rasops_wronly_putchar`].
pub unsafe fn rasops_wronly_copyrows(
    cookie: *mut c_void,
    src: i32,
    dst: i32,
    num: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(cookie) };
    let cols = ri.ri_cols.get();

    if num > 0 {
        // SAFETY: both ranges are whole rows of the store; `ptr::copy` allows overlap.
        unsafe {
            ptr::copy(
                ri.ri_bs.get().add((src * cols) as usize),
                ri.ri_bs.get().add((dst * cols) as usize),
                (num * cols) as usize,
            )
        };
    }

    for row in dst..dst + num {
        for col in 0..cols {
            let cell = ri.bs(row * cols + col);
            ri.ri_putchar(row, col, cell.uc, cell.attr)?;
        }
    }

    Ok(())
}

/// `rasops_wronly_eraserows`.
///
/// # Safety
///
/// As for [`rasops_wronly_putchar`].
pub unsafe fn rasops_wronly_eraserows(
    cookie: *mut c_void,
    row: i32,
    num: i32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(cookie) };
    let cols = ri.ri_cols.get();

    for i in 0..num * cols {
        ri.set_bs(row * cols + i, u32::from(b' '), attr);
    }

    ri.ri_eraserows(row, num, attr)
}

/// `rasops_wronly_do_cursor`: redraw the cursor's cell from the backing store, with the
/// colours swapped when the cursor is being turned on.
pub fn rasops_wronly_do_cursor(ri: &RasopsInfo) -> Result<(), Errno> {
    let off = ri.ri_crow.get() * ri.ri_cols.get() + ri.ri_ccol.get();

    let cell = ri.bs(off);
    let uc = cell.uc;
    let mut attr = cell.attr;

    if ri.ri_flg.get() & RI_CURSOR == 0 {
        let fg = (attr >> 24) & 0xf;
        let bg = (attr >> 16) & 0xf;
        attr &= !0x0fff_0000;
        attr |= (fg << 16) | (bg << 24);
    }

    ri.ri_putchar(ri.ri_crow.get(), ri.ri_ccol.get(), uc, attr)
}

/*
 * Font management.
 *
 * Fonts usable on raster frame buffers are managed by wsfont, and are not tied to any
 * particular display.
 */

/// `rasops_add_font`.
///
/// # Safety
///
/// `font` stays valid for good once added (wsfont copies the structure, not the glyphs).
unsafe fn rasops_add_font(ri: &RasopsInfo, font: &mut WsdisplayFont) -> Result<(), Errno> {
    // only accept matching metrics
    if font.fontwidth != ri.font().fontwidth || font.fontheight != ri.font().fontheight {
        return Err(Errno::EINVAL);
    }

    // for raster consoles, only accept ISO Latin-1 or Unicode encoding
    if font.encoding != WSDISPLAY_FONTENC_ISO {
        return Err(Errno::EINVAL);
    }

    // SAFETY: the caller's contract; with `copy` wsfont keeps its own copy of the structure.
    if unsafe { wsfont_add(font, true) }.is_err() {
        return Err(Errno::EEXIST); // name collision
    }

    font.index = -1; // do not store in wsdisplay_softc

    Ok(())
}

/// `rasops_use_font`.
fn rasops_use_font(ri: &RasopsInfo, font: &WsdisplayFont) -> Result<(), Errno> {
    // allow an empty font name to revert to the initial font choice
    let name = font.name();
    let name = if name.is_empty() { None } else { Some(name) };

    let Some(wsfcookie) = wsfont_find(
        name,
        ri.font().fontwidth as i32,
        ri.font().fontheight as i32,
        0,
    ) else {
        return match wsfont_find(name, 0, 0, 0) {
            None => Err(Errno::ENOENT), // font exist, but different metrics
            Some(_) => Err(Errno::EINVAL),
        };
    };
    let Ok((_, wsf)) = wsfont_lock(
        wsfcookie,
        WSDISPLAY_FONTORDER_KNOWN,
        WSDISPLAY_FONTORDER_KNOWN,
    ) else {
        return Err(Errno::EINVAL);
    };

    let _ = wsfont_unlock(ri.ri_wsfcookie.get());
    ri.ri_wsfcookie.set(wsfcookie);
    ri.ri_font.set(wsf.as_ptr());
    ri.ri_fontscale
        .set(ri.font().fontheight as i32 * ri.font().stride as i32);

    Ok(())
}

/// `rasops_load_font`: for now, we want to only allow loading fonts of the same metrics as
/// the currently in-use font. This requires the rasops struct to have been correctly
/// configured, and a font to have been selected.
///
/// # Safety
///
/// `v` is a [`RasopsInfo`]; a font with `data` is the ioctl's kernel copy, kept for good.
pub unsafe fn rasops_load_font(
    v: *mut c_void,
    _cookie: *mut c_void,
    font: &mut WsdisplayFont,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(v) };

    if ri.ri_flg.get() & RI_CFGDONE == 0 || ri.ri_font.get().is_null() {
        return Err(Errno::EINVAL);
    }

    if !font.data.is_null() {
        // SAFETY: the caller's contract.
        unsafe { rasops_add_font(ri, font) }
    } else {
        rasops_use_font(ri, font)
    }
}

/// `rasops_list_font`: font number `font.index` of wsfont's list.
///
/// # Safety
///
/// `v` is a [`RasopsInfo`].
pub unsafe fn rasops_list_font(v: *mut c_void, font: &mut WsdisplayFont) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(v) };

    if ri.ri_flg.get() & RI_CFGDONE == 0 || ri.ri_font.get().is_null() {
        return Err(Errno::EINVAL);
    }

    if font.index < 0 {
        return Err(Errno::EINVAL);
    }

    // rasops_list_font_cb, with `struct rasops_list_font_ctx` as the closure's captures
    let mut cnt = font.index;
    let mut found = None;
    wsfont_enum(|f| {
        if cnt == 0 {
            found = Some(*f);
            return true;
        }
        cnt -= 1;
        false
    });

    let Some(f) = found else {
        return Err(Errno::EINVAL);
    };

    let idx = font.index;
    *font = f; // struct copy
    font.index = idx;
    // don't leak kernel pointers
    font.cookie = ptr::null_mut();
    font.data = ptr::null_mut();
    Ok(())
}

/// `rasops_scrollback`: show the screen `lines` rows back in its scrollback (0: the live
/// screen).
///
/// # Safety
///
/// `v` is the [`RasopsInfo`] and `cookie` a live screen of it.
pub unsafe fn rasops_scrollback(v: *mut c_void, cookie: *mut c_void, lines: i32) {
    // SAFETY: the caller's contract.
    let ri = unsafe { ri_of(v) };
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(cookie) };

    let oldvoff = scr.rs_visibleoffset.get();

    if lines == 0 {
        scr.rs_visibleoffset.set(scr.rs_dispoffset);
    } else {
        let off =
            (scr.rs_visibleoffset.get() + lines * ri.ri_cols.get()).clamp(0, scr.rs_dispoffset);
        scr.rs_visibleoffset.set(off);
    }

    if scr.rs_visibleoffset.get() == oldvoff {
        return;
    }

    // SAFETY: `v` is the rasops_info.
    let _ = unsafe { rasops_cursor(v, 0, 0, 0) };
    let _ = ri.ri_eraserows(0, ri.ri_rows.get(), scr.rs_defattr.get());
    let cols = scr.rs_ri.ri_cols.get();
    for row in 0..ri.ri_rows.get() {
        for col in 0..ri.ri_cols.get() {
            let cell = scr.bs(row * cols + col + scr.rs_visibleoffset.get());
            let _ = ri.ri_putchar(row, col, cell.uc, cell.attr);
        }
    }

    if scr.rs_crow.get() != -1 && scr.rs_visibleoffset.get() == scr.rs_dispoffset {
        // SAFETY: `v` is the rasops_info.
        let _ = unsafe { rasops_cursor(v, 1, scr.rs_crow.get(), scr.rs_ccol.get()) };
    }
}

/// `rasops_claim_framebuffer`: record that `dev` drives the frame buffer at `base`, so the
/// generic frame buffer drivers (`simplefb`) leave it alone.
pub fn rasops_claim_framebuffer(base: Paddr, size: Psize, dev: Option<&Device>) {
    let Some(mem) = malloc(size_of::<RasopsFramebuffer>(), M_DEVBUF, M_WAITOK) else {
        return;
    };
    let rf = mem.cast::<RasopsFramebuffer>().as_ptr();
    // SAFETY: a fresh allocation of one entry, malloc(9)-aligned, kept for good.
    unsafe {
        ptr::write(
            rf,
            RasopsFramebuffer {
                rf_list: RASOPS_FRAMEBUFFERS.load(Ordering::Acquire),
                rf_base: base,
                rf_size: size,
                rf_dev: dev.map_or(ptr::null(), ptr::from_ref),
            },
        )
    };

    RASOPS_FRAMEBUFFERS.store(rf, Ordering::Release);
}

/// `rasops_check_framebuffer`: whether a driver claimed the frame buffer at `base`.
pub fn rasops_check_framebuffer(base: Paddr) -> bool {
    let mut rf = RASOPS_FRAMEBUFFERS.load(Ordering::Acquire);
    while let Some(p) = NonNull::new(rf) {
        // SAFETY: the entries are never freed.
        let f = unsafe { p.as_ref() };
        let b = f.rf_base.as_usize();
        if base.as_usize() >= b && base.as_usize() < b + f.rf_size.as_usize() {
            return true;
        }
        rf = f.rf_list;
    }

    false
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use std::boxed::Box;
    use std::vec;

    use crate::dev::wscons::wsdisplayvar::{WSCOL_BLUE, WSCOL_WHITE};
    use crate::kern::subr_pool::tests::setup_real_memory;

    /// A frame buffer for the tests: leaked memory the descriptor points into.
    pub(crate) struct TestFb {
        ptr: *mut i32,
        len: usize,
    }

    impl TestFb {
        /// The frame buffer's words as they are now.
        pub(crate) fn words(&self) -> &[i32] {
            // SAFETY: the leaked allocation of `len` words; read after the writes are done.
            unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
        }
    }

    /// A descriptor for a `width` x `height` frame buffer of `depth` bits (stride = width *
    /// bytes per pixel), filled with a pattern so clears are visible, configured by
    /// `rasops_init` with `flags`.
    pub(crate) fn test_ri_flags(
        depth: i32,
        width: i32,
        height: i32,
        flags: i32,
    ) -> (&'static RasopsInfo, TestFb) {
        let bpp = if depth == 15 { 16 } else { depth };
        let stride = width * bpp / 8;
        let len = (stride * height / 4) as usize;
        let fb = Box::leak(vec![0x5a5a_5a5ai32; len].into_boxed_slice());
        let ptr = fb.as_mut_ptr();
        let ri: &'static RasopsInfo = Box::leak(Box::new(RasopsInfo::new()));
        ri.ri_depth.set(depth);
        ri.ri_bits.set(ptr.cast());
        ri.ri_width.set(width);
        ri.ri_height.set(height);
        ri.ri_stride.set(stride);
        ri.ri_flg.set(flags);
        rasops_init(ri, 50, 160).expect("rasops_init");
        (ri, TestFb { ptr, len })
    }

    /// [`test_ri_flags`] with no flags.
    pub(crate) fn test_ri(depth: i32, width: i32, height: i32) -> (&'static RasopsInfo, TestFb) {
        test_ri_flags(depth, width, height, 0)
    }

    #[test]
    fn colormap() {
        assert_eq!(&RASOPS_CMAP[0..3], &[0, 0, 0]);
        assert_eq!(&RASOPS_CMAP[3..6], &[0x7f, 0, 0]);
        assert_eq!(&RASOPS_CMAP[7 * 3..8 * 3], &[0xc7, 0xc7, 0xc7]);
        assert_eq!(&RASOPS_CMAP[15 * 3..16 * 3], &[0xff, 0xff, 0xff]);
        assert_eq!(&RASOPS_CMAP[100 * 3..101 * 3], &[0xff, 0xff, 0xff]);
        // The last 16 are the first 16 inverted, in reverse order.
        assert_eq!(&RASOPS_CMAP[240 * 3..241 * 3], &[0, 0, 0]); // ~HILITE_WHITE
        assert_eq!(&RASOPS_CMAP[255 * 3..256 * 3], &[0xff, 0xff, 0xff]); // ~NORMAL_BLACK
        assert_eq!(&RASOPS_CMAP[247 * 3..248 * 3], &[0x80, 0x80, 0x80]); // ~HILITE_BLACK
    }

    #[test]
    fn attributes() {
        let n = ptr::null_mut();
        // SAFETY: the attribute packers do not use the cookie.
        unsafe {
            assert_eq!(rasops_pack_cattr(n, 1, 2, 0), Ok(0x0700_0000));
            assert_eq!(rasops_pack_cattr(n, 1, 2, WSATTR_WSCOLORS), Ok(0x0102_0000));
            assert_eq!(
                rasops_pack_cattr(n, 1, 2, WSATTR_WSCOLORS | WSATTR_REVERSE | WSATTR_HILIT),
                Ok(0x0a01_0000)
            );
            assert_eq!(rasops_pack_cattr(n, 1, 2, WSATTR_BLINK), Err(Errno::EINVAL));
            assert_eq!(
                rasops_pack_mattr(n, 5, 5, WSATTR_REVERSE | WSATTR_UNDERLINE),
                Ok(0x0001_0008)
            );
            assert_eq!(rasops_pack_mattr(n, 5, 5, WSATTR_HILIT), Err(Errno::EINVAL));
            assert_eq!(rasops_unpack_attr(n, 0x0a01_0008), (10, 1, 8));
        }
    }

    #[test]
    fn geometry_and_font_choice() {
        // 640 pixels wide: the 8-pixel font, an 80x30 grid.
        let (ri, _fb) = test_ri(32, 640, 480);
        assert_eq!((ri.font().fontwidth, ri.font().fontheight), (8, 16));
        assert_eq!((ri.ri_cols.get(), ri.ri_rows.get()), (80, 30));
        assert_eq!(ri.ri_xscale.get(), 32);
        assert_eq!(ri.ri_yscale.get(), 16 * 640 * 4);
        assert_eq!(ri.ri_flg.get() & RI_CFGDONE, RI_CFGDONE);

        // 1280x800, centred: the 12x24 font, 106 columns (wantcols 160 is cut to the width),
        // 33 rows. The emulated height is the whole screen (wantrows 50 is cut to it), so the
        // grid is not moved down: the 8 leftover lines are at the bottom.
        let (ri, _fb) = test_ri_flags(32, 1280, 800, RI_CENTER);
        assert_eq!((ri.font().fontwidth, ri.font().fontheight), (12, 24));
        assert_eq!((ri.ri_cols.get(), ri.ri_rows.get()), (106, 33));
        assert_eq!(ri.ri_emuwidth.get(), 1280);
        assert_eq!(ri.ri_xorigin.get(), 0);
        assert_eq!(ri.ri_emuheight.get(), 800);
        assert_eq!(ri.ri_yorigin.get(), 0);

        // 1000x700 centred with the 12x24 font and an 80x25 wish: 960x600, centred.
        let ri: &'static RasopsInfo = Box::leak(Box::new(RasopsInfo::new()));
        let fb = Box::leak(vec![0i32; 1000 * 700].into_boxed_slice());
        ri.ri_depth.set(32);
        ri.ri_bits.set(fb.as_mut_ptr().cast());
        ri.ri_width.set(1000);
        ri.ri_height.set(700);
        ri.ri_stride.set(4000);
        ri.ri_flg.set(RI_CENTER);
        rasops_init(ri, 25, 80).expect("rasops_init");
        assert_eq!((ri.ri_cols.get(), ri.ri_rows.get()), (80, 25));
        assert_eq!((ri.ri_xorigin.get(), ri.ri_yorigin.get()), (20, 50));

        // 1920 wide: the 16-pixel font.
        let (ri, _fb) = test_ri(32, 1920, 64 * 10);
        assert_eq!(ri.font().fontwidth, 16);

        // An unsupported depth.
        let ri: &'static RasopsInfo = Box::leak(Box::new(RasopsInfo::new()));
        let fb = Box::leak(vec![0u8; 64 * 64 * 2].into_boxed_slice());
        ri.ri_depth.set(2);
        ri.ri_bits.set(fb.as_mut_ptr());
        ri.ri_width.set(64);
        ri.ri_height.set(64);
        ri.ri_stride.set(16);
        assert_eq!(rasops_init(ri, 10, 20), Err(Errno::EINVAL));
    }

    #[test]
    fn devcmap_follows_the_channel_layout() {
        // Red in the high byte, blue in the low one (the GOP's BGRX order).
        let ri: &'static RasopsInfo = Box::leak(Box::new(RasopsInfo::new()));
        ri.ri_depth.set(32);
        ri.ri_rnum.set(8);
        ri.ri_rpos.set(16);
        ri.ri_gnum.set(8);
        ri.ri_gpos.set(8);
        ri.ri_bnum.set(8);
        ri.ri_bpos.set(0);
        rasops_init_devcmap(ri);
        assert_eq!(ri.ri_devcmap[0].get(), 0);
        assert_eq!(ri.ri_devcmap[1].get(), 0x7f_0000);
        assert_eq!(ri.ri_devcmap[4].get(), 0x7f);
        assert_eq!(ri.ri_devcmap[15].get(), 0xff_ffff);

        // 16 bits, r5g6b5: the value is doubled into both halves of the word.
        ri.ri_depth.set(16);
        ri.ri_rnum.set(5);
        ri.ri_rpos.set(11);
        ri.ri_gnum.set(6);
        ri.ri_gpos.set(5);
        ri.ri_bnum.set(5);
        ri.ri_bpos.set(0);
        rasops_init_devcmap(ri);
        assert_eq!(ri.ri_devcmap[9].get() as u32, 0xf800_f800);

        // Monochrome.
        ri.ri_depth.set(1);
        rasops_init_devcmap(ri);
        assert_eq!(ri.ri_devcmap[0].get(), 0);
        assert_eq!(ri.ri_devcmap[3].get(), -1);
    }

    #[test]
    fn erase_and_copy_on_the_frame_buffer() {
        let (ri, fb) = test_ri(32, 640, 480);
        let blue = ri.ri_devcmap[WSCOL_BLUE as usize].get();
        let attr = ri
            .ri_pack_attr(WSCOL_WHITE, WSCOL_BLUE, WSATTR_WSCOLORS)
            .expect("packs");
        let stride = 640;
        let cell = |w: &[i32], row: usize, col: usize| w[row * 16 * stride + col * 8];

        // Rows 2 and 3 cleared to blue; row 1 and 4 untouched.
        ri.ri_eraserows(2, 2, attr).expect("eraserows");
        let w = fb.words();
        assert_eq!(cell(w, 2, 0), blue);
        assert_eq!(w[(4 * 16 - 1) * stride + 639], blue);
        assert_eq!(cell(w, 1, 0), 0x5a5a_5a5a);
        assert_eq!(cell(w, 4, 0), 0x5a5a_5a5a);

        // Columns 5..8 of row 6.
        ri.ri_erasecols(6, 5, 3, attr).expect("erasecols");
        let w = fb.words();
        assert_eq!(cell(w, 6, 5), blue);
        assert_eq!(w[(6 * 16 + 15) * stride + 8 * 8 - 1], blue);
        assert_eq!(cell(w, 6, 4), 0x5a5a_5a5a);
        assert_eq!(cell(w, 6, 8), 0x5a5a_5a5a);

        // Row 2 (blue) copied down onto row 10, then column 5 of row 6 onto column 20.
        ri.ri_copyrows(2, 10, 1).expect("copyrows");
        ri.ri_copycols(6, 5, 20, 1).expect("copycols");
        let w = fb.words();
        assert_eq!(cell(w, 10, 0), blue);
        assert_eq!(cell(w, 6, 20), blue);
        assert_eq!(cell(w, 6, 21), 0x5a5a_5a5a);

        // The cursor inverts its cell, twice restores it.
        // SAFETY: the test descriptor.
        unsafe { rasops_cursor(ri.cookie(), 1, 10, 0) }.expect("cursor on");
        assert_eq!(cell(fb.words(), 10, 0), !blue);
        // SAFETY: as above.
        unsafe { rasops_cursor(ri.cookie(), 0, 10, 0) }.expect("cursor off");
        assert_eq!(cell(fb.words(), 10, 0), blue);
    }

    #[test]
    fn virtual_screens_keep_a_backing_store() {
        let _g = setup_real_memory();
        let (ri, fb) = test_ri_flags(32, 640, 480, RI_VCONS | RI_WRONLY | RI_CLEAR);
        assert_eq!(ri.ri_nscreens.get(), 1);
        let scr = ri.ri_active.get().cast::<c_void>();
        assert!(!scr.is_null());
        let ops = ri.ri_ops.get();
        let putchar = ops.putchar.expect("vcons putchar");
        let copyrows = ops.copyrows.expect("vcons copyrows");
        let white = ri.ri_devcmap[7].get();

        // SAFETY: the active screen and the descriptor rasops_init made.
        unsafe {
            let attr = (ops.pack_attr.expect("pack"))(scr, 0, 0, 0).expect("packs");
            putchar(scr, 3, 4, u32::from(b'X'), attr).expect("putchar");
            let c = rasops_getchar(ri.cookie(), 3, 4).expect("a cell");
            assert_eq!(c.uc, u32::from(b'X'));
            // The glyph is on the frame buffer: some pixel of the cell is white.
            let w = fb.words();
            let lit = (0..16).any(|y| (0..8).any(|x| w[(3 * 16 + y) * 640 + 4 * 8 + x] == white));
            assert!(lit);

            // Scrolling the whole screen up a row moves the cell and fills the scrollback.
            copyrows(scr, 1, 0, 29).expect("copyrows");
            assert_eq!(
                rasops_getchar(ri.cookie(), 2, 4).map(|c| c.uc),
                Some(u32::from(b'X'))
            );
            assert_eq!(
                rasops_getchar(ri.cookie(), 3, 4).map(|c| c.uc),
                Some(u32::from(b' '))
            );

            // A second screen is not visible and draws nothing.
            let (mut cookie, mut x, mut y, mut a) = (ptr::null_mut(), 0, 0, 0);
            rasops_alloc_screen(
                ri.cookie(),
                ptr::null(),
                &mut cookie,
                &mut x,
                &mut y,
                &mut a,
            )
            .expect("second screen");
            assert_eq!(ri.ri_nscreens.get(), 2);
            let before = fb.words().to_vec();
            putchar(cookie, 0, 0, u32::from(b'Y'), a).expect("putchar");
            assert_eq!(fb.words(), &before[..]);

            // Showing it redraws from its store.
            rasops_show_screen(ri.cookie(), cookie, 0, None, ptr::null_mut()).expect("switch");
            assert_eq!(
                rasops_getchar(ri.cookie(), 0, 0).map(|c| c.uc),
                Some(u32::from(b'Y'))
            );
            rasops_show_screen(ri.cookie(), scr, 0, None, ptr::null_mut()).expect("switch back");
            rasops_free_screen(ri.cookie(), cookie);
            assert_eq!(ri.ri_nscreens.get(), 1);
        }
    }

    #[test]
    fn mapchar_and_font_listing() {
        let (ri, _fb) = test_ri(32, 640, 480);
        let mut c = 0;
        // SAFETY: the test descriptor.
        unsafe {
            assert_eq!(rasops_mapchar(ri.cookie(), 'A' as i32, &mut c), 5);
            assert_eq!(c, 'A' as u32);
            assert_eq!(rasops_mapchar(ri.cookie(), 0x10, &mut c), 0);
            assert_eq!(c, '?' as u32);
            assert_eq!(rasops_mapchar(ri.cookie(), 0x263a, &mut c), 0);

            let mut f = WsdisplayFont::zeroed();
            f.index = 1;
            rasops_list_font(ri.cookie(), &mut f).expect("second font");
            assert_eq!(f.name(), b"Spleen 12x24");
            assert!(f.data.is_null() && f.cookie.is_null());
            f.index = 50;
            assert_eq!(rasops_list_font(ri.cookie(), &mut f), Err(Errno::EINVAL));
        }
    }

    #[test]
    fn framebuffer_claims() {
        let _g = setup_real_memory();
        assert!(!rasops_check_framebuffer(Paddr::new(0x7000_1000)));
        rasops_claim_framebuffer(Paddr::new(0x7000_0000), Psize::new(0x10_0000), None);
        assert!(rasops_check_framebuffer(Paddr::new(0x7000_1000)));
        assert!(!rasops_check_framebuffer(Paddr::new(0x7010_0000)));
    }
}
/* </TESTS> */
