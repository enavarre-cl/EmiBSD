/* $OpenBSD: vga.c,v 1.74 2021/05/27 23:24:40 cheloha Exp $ */
/* $NetBSD: vga.c,v 1.28.2.1 2000/06/30 16:27:47 simonb Exp $ */
/* <LICENSES> */
/*-
 * Copyright (c) 1999 Kazutaka YOKOTA <yokota@zodiac.mech.utsunomiya-u.ac.jp>
 * Copyright (c) 1992-1998 Søren Schmidt
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer as
 *    the first lines of this file unmodified.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHORS ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 */
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
//! `vga(4)`: the VGA in text mode, as a `wsdisplay(4)` with up to eight 256-character
//! fonts in its character generator and several virtual screens in its 32 KB of text
//! memory.
//!
//! Upstream: sys/dev/ic/vga.c @ 3ce1f3f79392
//!
//! The bus front-ends (`vga_pci.c`, `vga_isa.c`) probe with [`vga_common_probe`], which
//! looks for VGA registers at their fixed legacy addresses (I/O 0x3c0, the 6845 at 0x3d4
//! or 0x3b4, text memory at 0xb8000 or 0xb0000), and attach with [`vga_common_attach`],
//! which sets up a [`VgaConfig`] (or takes the console's, made earlier by [`vga_cnattach`])
//! and attaches the `wsdisplay` child. The screens are `struct vgascreen`s: the displayed
//! one draws in video memory and scrolls by moving the 6845's start address through the
//! text memory; the others draw in a backing store until `vga_show_screen` swaps them in.
//!
//! ## Deviations
//! - The `__alpha__` code (`vga_pick_monitor_type`, the blue background of DEC firmware,
//!   the swapped colour tables, `custom_list`) is not compiled, as on every architecture
//!   but alpha. The `#ifdef notyet` members of `struct vgafont` are not here either.
//! - `VGAFONTDEBUG` is the constant [`VGAFONTDEBUG`] (off); its messages compile.
//! - The globals `vga_console_vc` and `vga_console_screen` are statics whose interiors
//!   are written while cold by `vga_cnattach` and later under the kernel lock, as in C;
//!   `vga_console_vc` holds `None` until `vga_cnattach` made it (its bus handles are set
//!   by `vga_init`, which here builds the configuration and returns it).
//!   `vgaconsole`, `vga_console_type` and `vga_console_attached` are atomics.
//! - The console attach of OpenBSD (`wscons_machdep.c`'s `wscn_video_init` calling
//!   `vga_cnattach`) is not ported: EmiBSD's console is the serial line, as OpenBSD's
//!   with a serial console chosen by boot(8), so nothing calls [`vga_cnattach`] yet and a
//!   VGA attaches as a plain display.
//! - `vga_extended_attach` sets the attach arguments' `primary` to 0, which the C leaves
//!   uninitialised; no GENERIC line at `vga?` gives a `primary` locator, so it is never
//!   read there.
//! - The emulops and accessops are the `unsafe fn`s of `wsdisplayvar.rs`'s tables, over
//!   the C's cookies (a [`Vgascreen`] for the emulops, the [`VgaConfig`] for the
//!   accessops); their 0-or-errno results are `Result`s. `vga_ioctl` keeps the C's
//!   `ENOTTY` for an unknown command.
//! - A failed `M_WAITOK` allocation in `vga_alloc_screen` (which cannot fail in C) returns
//!   `ENOMEM`.
//! - `vga_free_screen` clears the backing-store pointer of the screen whose store it
//!   frees (the C leaves it dangling, unread); `vga_getchar` returns `None` when there is
//!   no active screen (the C dereferences NULL).
//! - The bus front-ends call `vga_pci_ioctl` (`NVGA_PCI > 0`) through `vga_pci.rs`, which
//!   is always compiled.
//! - The licence block is whole; its one Latin-1 byte (the `ø` of Søren Schmidt) is
//!   written in UTF-8, as Rust source must be.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::dev::ic::pcdisplay::{
    BG_BLACK, BG_BLUE, BG_BROWN, BG_CYAN, BG_GREEN, BG_LIGHTGREY, BG_MAGENTA, BG_RED, FG_BLACK,
    FG_BLINK, FG_BLUE, FG_BROWN, FG_CYAN, FG_GREEN, FG_INTENSE, FG_LIGHTGREY, FG_MAGENTA, FG_RED,
    FG_UNDERLINE,
};
use crate::dev::ic::pcdisplay_chars::pcdisplay_mapchar;
use crate::dev::ic::pcdisplay_subr::{
    PCDISPLAY_SOFTCURSOR, pcdisplay_copycols, pcdisplay_cursor, pcdisplay_cursor_init,
    pcdisplay_cursor_reset, pcdisplay_erasecols, pcdisplay_eraserows, pcdisplay_getchar,
    pcdisplay_putchar,
};
use crate::dev::ic::pcdisplayvar::PcdisplayHandle;
use crate::dev::ic::vga_subr::{vga_loadchars, vga_setfontset, vga_setscreentype};
use crate::dev::ic::vgareg::{VGA_ATC_DATAR, VGA_ATC_DATAW, VGA_ATC_INDEX};
use crate::dev::ic::vgareg::{VGA_DAC_DATA, VGA_DAC_MASK, VGA_DAC_READ, VGA_DAC_WRITE};
use crate::dev::ic::vgavar::{
    VGA_MAXFONT, VgaConfig, VgaHandle, Vgascreen, VgascreenList, vga_6845_read, vga_6845_write,
    vga_raw_read, vga_raw_write, vga_ts_read, vga_ts_write,
};
use crate::dev::pci::vga_pci::vga_pci_ioctl;
use crate::dev::wscons::wsconsio::{
    WSDISPLAY_BURN_VBLANK, WSDISPLAY_FONTENC_IBM, WSDISPLAY_FONTENC_ISO, WSDISPLAY_FONTORDER_L2R,
    WSDISPLAY_TYPE_PCIVGA, WSDISPLAYIO_GTYPE, WSDISPLAYIO_GVIDEO, WSDISPLAYIO_MODE_EMUL,
    WSDISPLAYIO_SMODE, WSDISPLAYIO_SVIDEO, WSFONT_NAME_SIZE, WsdisplayFont,
};
use crate::dev::wscons::wsdisplay::{
    wsdisplay_cnattach, wsemuldisplaydevprint, wsemuldisplaydevsubmatch,
};
use crate::dev::wscons::wsdisplayvar::{
    ShowScreenCb, WSATTR_BLINK, WSATTR_HILIT, WSATTR_REVERSE, WSATTR_UNDERLINE, WSATTR_WSCOLORS,
    WSCOL_BLACK, WSCOL_BLUE, WSCOL_BROWN, WSCOL_CYAN, WSCOL_GREEN, WSCOL_MAGENTA, WSCOL_RED,
    WSCOL_WHITE, WSSCREEN_BLINK, WSSCREEN_HILIT, WSSCREEN_REVERSE, WSSCREEN_UNDERLINE,
    WSSCREEN_WSCOLORS, WsdisplayAccessops, WsdisplayCharcell, WsdisplayEmulops, WsdisplayMmapFn,
    WsemuldisplaydevAttachArgs, WsscreenDescr, WsscreenList,
};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_timeout::{timeout_add, timeout_set};
use crate::kern::subr_autoconf::config_found_sm;
use crate::kern::subr_prf::{Str, panic, printf};
use crate::machine::bus::{
    BusSpaceTag, bus_space_copy_2, bus_space_map, bus_space_read_1, bus_space_read_2,
    bus_space_read_region_2, bus_space_subregion, bus_space_unmap, bus_space_write_1,
    bus_space_write_2, bus_space_write_region_2,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{splhigh, spltty, splx};
use crate::sys::device::{Cfdriver, DV_DULL, Device};
use crate::sys::errno::Errno;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_CANFAIL, M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::proc::Proc;
use crate::sys::types::Paddr;

/// `VGAFONTDEBUG`: report font selection and loading. Not defined.
const VGAFONTDEBUG: bool = false;

/// `struct vgafont`: a font in one of the character generator's slots.
pub struct Vgafont {
    /// `name`.
    pub name: [u8; WSFONT_NAME_SIZE],
    /// `height`: lines per character.
    pub height: i32,
    /// `encoding`: `WSDISPLAY_FONTENC_*`.
    pub encoding: i32,
    /// `slot`: the character generator's font slot.
    pub slot: i32,
    /// `fontdata`: the glyphs (`256 * height` bytes), NULL for the firmware's font.
    pub fontdata: *const u8,
}

impl Vgafont {
    /// The name, up to its NUL.
    pub fn name(&self) -> &[u8] {
        cstr(&self.name)
    }
}

// SAFETY: a font is written once, before it is published in `vc_fonts`, and only read
// afterwards; `fontdata` points at glyphs kept for good.
unsafe impl Sync for Vgafont {}

/// A screen descriptor in a static: written by the compiler, only read.
struct Descr(WsscreenDescr);

// SAFETY: the descriptor is constant; its `textops` points at the constant `VGA_EMULOPS`.
unsafe impl Sync for Descr {}

/// A static list of screen descriptors.
struct Scrlist<const N: usize>([*const WsscreenDescr; N]);

// SAFETY: constant pointers at constant descriptors.
unsafe impl<const N: usize> Sync for Scrlist<N> {}

/// A static `struct wsscreen_list`.
struct Screenlist(WsscreenList);

// SAFETY: constant: a count and a pointer at a constant list.
unsafe impl Sync for Screenlist {}

/// The console's `struct vga_config`, made by `vga_cnattach`.
struct ConsoleVc(core::cell::UnsafeCell<Option<VgaConfig>>);

// SAFETY: written only by `vga_cnattach`, while cold and before any other CPU runs; read
// afterwards under the kernel lock, as the C's global is.
unsafe impl Sync for ConsoleVc {}

/// The console's `struct vgascreen`.
struct ConsoleScreen(Vgascreen);

// SAFETY: a screen's cells are touched at `spltty()` under the kernel lock, as the C's
// global is.
unsafe impl Sync for ConsoleScreen {}

/// `vga_builtinfont`: the font the firmware loaded into slot 0.
static VGA_BUILTINFONT: Vgafont = Vgafont {
    name: font_name(b"builtin"),
    height: 16,
    encoding: WSDISPLAY_FONTENC_IBM,
    slot: 0,
    fontdata: ptr::null(),
};

/// `vgaconsole`.
static VGACONSOLE: AtomicI32 = AtomicI32::new(0);
/// `vga_console_type`.
static VGA_CONSOLE_TYPE: AtomicI32 = AtomicI32::new(0);
/// `vga_console_attached`.
static VGA_CONSOLE_ATTACHED: AtomicI32 = AtomicI32::new(0);
/// `vga_console_screen`.
static VGA_CONSOLE_SCREEN: ConsoleScreen = ConsoleScreen(Vgascreen::new());
/// `vga_console_vc`.
static VGA_CONSOLE_VC: ConsoleVc = ConsoleVc(core::cell::UnsafeCell::new(None));

/// `vga_emulops`.
static VGA_EMULOPS: WsdisplayEmulops = WsdisplayEmulops {
    cursor: Some(pcdisplay_cursor),
    mapchar: Some(vga_mapchar),
    putchar: Some(vga_putchar),
    copycols: Some(pcdisplay_copycols),
    erasecols: Some(pcdisplay_erasecols),
    copyrows: Some(vga_copyrows),
    eraserows: Some(pcdisplay_eraserows),
    pack_attr: Some(vga_pack_attr),
    unpack_attr: Some(vga_unpack_attr),
};

/// `fgansitopc[]`: translate WS(=ANSI) color codes to standard pc ones.
static FGANSITOPC: [u32; 8] = [
    FG_BLACK,
    FG_RED,
    FG_GREEN,
    FG_BROWN,
    FG_BLUE,
    FG_MAGENTA,
    FG_CYAN,
    FG_LIGHTGREY,
];
/// `bgansitopc[]`.
static BGANSITOPC: [u32; 8] = [
    BG_BLACK,
    BG_RED,
    BG_GREEN,
    BG_BROWN,
    BG_BLUE,
    BG_MAGENTA,
    BG_CYAN,
    BG_LIGHTGREY,
];

/// `pctoansi[]`: translate standard pc color codes to WS(=ANSI) ones.
static PCTOANSI: [i32; 8] = [
    WSCOL_BLACK,
    WSCOL_BLUE,
    WSCOL_GREEN,
    WSCOL_CYAN,
    WSCOL_RED,
    WSCOL_MAGENTA,
    WSCOL_BROWN,
    WSCOL_WHITE,
];

/// The capabilities of the colour screens.
const CAPS_COLOR: i32 = WSSCREEN_WSCOLORS | WSSCREEN_HILIT | WSSCREEN_BLINK;
/// The capabilities of the monochrome screens.
const CAPS_MONO: i32 = WSSCREEN_HILIT | WSSCREEN_UNDERLINE | WSSCREEN_BLINK | WSSCREEN_REVERSE;
/// The capabilities of the colour screens with two fonts (`bf`: no highlighting, which
/// would select the second font).
const CAPS_BF: i32 = WSSCREEN_WSCOLORS | WSSCREEN_BLINK;

/// `vga_stdscreen`.
static VGA_STDSCREEN: Descr = Descr(descr(b"80x25", 80, 25, 16, CAPS_COLOR));
/// `vga_stdscreen_mono`.
static VGA_STDSCREEN_MONO: Descr = Descr(descr(b"80x25", 80, 25, 16, CAPS_MONO));
/// `vga_stdscreen_bf`.
static VGA_STDSCREEN_BF: Descr = Descr(descr(b"80x25bf", 80, 25, 16, CAPS_BF));
/// `vga_40lscreen`.
static VGA_40LSCREEN: Descr = Descr(descr(b"80x40", 80, 40, 10, CAPS_COLOR));
/// `vga_40lscreen_mono`.
static VGA_40LSCREEN_MONO: Descr = Descr(descr(b"80x40", 80, 40, 10, CAPS_MONO));
/// `vga_40lscreen_bf`.
static VGA_40LSCREEN_BF: Descr = Descr(descr(b"80x40bf", 80, 40, 10, CAPS_BF));
/// `vga_50lscreen`.
static VGA_50LSCREEN: Descr = Descr(descr(b"80x50", 80, 50, 8, CAPS_COLOR));
/// `vga_50lscreen_mono`.
static VGA_50LSCREEN_MONO: Descr = Descr(descr(b"80x50", 80, 50, 8, CAPS_MONO));
/// `vga_50lscreen_bf`.
static VGA_50LSCREEN_BF: Descr = Descr(descr(b"80x50bf", 80, 50, 8, CAPS_BF));

/// `_vga_scrlist[]`.
static _VGA_SCRLIST: Scrlist<6> = Scrlist([
    &VGA_STDSCREEN.0,
    &VGA_STDSCREEN_BF.0,
    &VGA_40LSCREEN.0,
    &VGA_40LSCREEN_BF.0,
    &VGA_50LSCREEN.0,
    &VGA_50LSCREEN_BF.0,
    // XXX other formats, graphics screen?
]);
/// `_vga_scrlist_mono[]`.
static _VGA_SCRLIST_MONO: Scrlist<3> = Scrlist([
    &VGA_STDSCREEN_MONO.0,
    &VGA_40LSCREEN_MONO.0,
    &VGA_50LSCREEN_MONO.0,
    // XXX other formats, graphics screen?
]);

/// `vga_screenlist`.
static VGA_SCREENLIST: Screenlist = Screenlist(WsscreenList {
    nscreens: 6,
    screens: _VGA_SCRLIST.0.as_ptr(),
});
/// `vga_screenlist_mono`.
static VGA_SCREENLIST_MONO: Screenlist = Screenlist(WsscreenList {
    nscreens: 3,
    screens: _VGA_SCRLIST_MONO.0.as_ptr(),
});

/// `vga_accessops`.
pub static VGA_ACCESSOPS: WsdisplayAccessops = WsdisplayAccessops {
    ioctl: Some(vga_ioctl),
    mmap: Some(vga_mmap),
    alloc_screen: Some(vga_alloc_screen),
    free_screen: Some(vga_free_screen),
    show_screen: Some(vga_show_screen),
    load_font: Some(vga_load_font),
    list_font: Some(vga_list_font),
    scrollback: Some(vga_scrollback),
    getchar: Some(vga_getchar),
    burn_screen: Some(vga_burner),
    ..WsdisplayAccessops::EMPTY
};

/// `vga_cd`.
pub static VGA_CD: Cfdriver = Cfdriver::new(b"vga", DV_DULL, 0);

/// A screen descriptor of `vga.c`'s: 8 dots wide, drawn with [`VGA_EMULOPS`].
const fn descr(name: &[u8], ncols: i32, nrows: i32, fontheight: i32, caps: i32) -> WsscreenDescr {
    WsscreenDescr {
        ncols,
        nrows,
        textops: &VGA_EMULOPS,
        fontwidth: 8,
        fontheight,
        capabilities: caps,
        ..WsscreenDescr::new(name)
    }
}

/// A font name as the C's `char name[WSFONT_NAME_SIZE]`.
const fn font_name(name: &[u8]) -> [u8; WSFONT_NAME_SIZE] {
    let mut n = [0u8; WSFONT_NAME_SIZE];
    let mut i = 0;
    while i < name.len() && i < WSFONT_NAME_SIZE - 1 {
        n[i] = name[i];
        i += 1;
    }
    n
}

/// A C string: `s` up to its first NUL.
fn cstr(s: &[u8]) -> &[u8] {
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    &s[..n]
}

/// `!strncmp(a, b, WSFONT_NAME_SIZE)`.
fn name_eq(a: &[u8], b: &[u8]) -> bool {
    let a = cstr(a);
    let b = cstr(b);
    a[..a.len().min(WSFONT_NAME_SIZE)] == b[..b.len().min(WSFONT_NAME_SIZE)]
}

/// `VGA_SCREEN_CANTWOFONTS(type)`: a screen without highlighting can use a second font.
fn vga_screen_cantwofonts(type_: &WsscreenDescr) -> bool {
    type_.capabilities & WSSCREEN_HILIT == 0
}

/// The configuration behind an accessops cookie.
///
/// # Safety
///
/// `v` is the access cookie `vga_extended_attach` handed out: a live [`VgaConfig`].
unsafe fn vc_of<'a>(v: *mut c_void) -> &'a VgaConfig {
    // SAFETY: the caller's contract.
    unsafe { &*v.cast::<VgaConfig>() }
}

/// The screen behind an emulops cookie.
///
/// # Safety
///
/// `id` is a screen cookie of this driver's: a live [`Vgascreen`].
unsafe fn scr_of<'a>(id: *mut c_void) -> &'a Vgascreen {
    // SAFETY: the caller's contract.
    unsafe { &*id.cast::<Vgascreen>() }
}

/// `&vga_console_screen`.
fn vga_console_screen() -> &'static Vgascreen {
    &VGA_CONSOLE_SCREEN.0
}

/// `&vga_console_vc`, once `vga_cnattach` made it.
fn vga_console_vc() -> Option<&'static VgaConfig> {
    // SAFETY: see `ConsoleVc`: written only while cold by `vga_cnattach`.
    unsafe { (*VGA_CONSOLE_VC.0.get()).as_ref() }
}

/// `vga_common_probe`: whether a VGA answers at the legacy addresses of `iot` and `memt`.
pub fn vga_common_probe(iot: BusSpaceTag, memt: BusSpaceTag) -> bool {
    // SAFETY: the VGA's fixed legacy I/O range, reserved for it by the PC architecture.
    let Ok(ioh_vga) = (unsafe { bus_space_map(iot, 0x3c0, 0x10, 0) }) else {
        return false;
    };

    // read "misc output register"
    let regval = bus_space_read_1(iot, ioh_vga, 0xc);
    let mono = regval & 1 == 0;

    // SAFETY: the 6845's fixed legacy I/O range (mono or colour), as above.
    let Ok(ioh_6845) = (unsafe { bus_space_map(iot, if mono { 0x3b0 } else { 0x3d0 }, 0x10, 0) })
    else {
        bus_space_unmap(iot, ioh_vga, 0x10);
        return false;
    };

    // SAFETY: the legacy video memory window at 0xa0000, as above.
    let Ok(memh) = (unsafe { bus_space_map(memt, 0xa0000, 0x20000, 0) }) else {
        bus_space_unmap(iot, ioh_vga, 0x10);
        bus_space_unmap(iot, ioh_6845, 0x10);
        return false;
    };

    let probe = || {
        let dispoffset = if mono { 0x10000 } else { 0x18000 };

        let vgadata = bus_space_read_2(memt, memh, dispoffset);
        bus_space_write_2(memt, memh, dispoffset, 0xa55a);
        if bus_space_read_2(memt, memh, dispoffset) != 0xa55a {
            return false;
        }
        bus_space_write_2(memt, memh, dispoffset, vgadata);

        // check if this is really a VGA (try to write "Color Select" register as XFree86
        // does)
        // XXX check before if at least EGA?

        // reset state
        let _ = bus_space_read_1(iot, ioh_6845, 10);
        bus_space_write_1(iot, ioh_vga, VGA_ATC_INDEX, 20 | 0x20); // colselect | enable
        let regval = bus_space_read_1(iot, ioh_vga, VGA_ATC_DATAR);
        // toggle the implemented bits
        bus_space_write_1(iot, ioh_vga, VGA_ATC_DATAW, regval ^ 0x0f);
        bus_space_write_1(iot, ioh_vga, VGA_ATC_INDEX, 20 | 0x20);
        // read back
        if bus_space_read_1(iot, ioh_vga, VGA_ATC_DATAR) != (regval ^ 0x0f) {
            return false;
        }
        // restore contents
        bus_space_write_1(iot, ioh_vga, VGA_ATC_DATAW, regval);

        true
    };
    let rv = probe();

    bus_space_unmap(iot, ioh_vga, 0x10);
    bus_space_unmap(iot, ioh_6845, 0x10);
    bus_space_unmap(memt, memh, 0x20000);

    rv
}

/// `vga_selectfont`: the fonts of `scr`, `name1` (`None` or empty: the first one found) in
/// the first slot and, for a screen that can use two fonts, `name2` in the second. We want
/// at least ASCII 32..127 be present in the first font slot.
pub fn vga_selectfont(
    vc: &VgaConfig,
    scr: &Vgascreen,
    name1: Option<&[u8]>,
    name2: Option<&[u8]>,
) -> Result<(), Errno> {
    let type_ = scr.pcs.type_();
    let name1 = name1.map(cstr).filter(|n| !n.is_empty());
    let name2 = name2.map(cstr).filter(|n| !n.is_empty());
    let mut f1: Option<&Vgafont> = None;
    let mut f2: Option<&Vgafont> = None;

    for i in 0..VGA_MAXFONT {
        let Some(f) = vc.font(i) else {
            continue;
        };
        if f.height != type_.fontheight {
            continue;
        }
        if f1.is_none() && name1.is_none_or(|n| name_eq(n, &f.name)) {
            f1 = Some(f);
            continue;
        }
        if f2.is_none()
            && vga_screen_cantwofonts(type_)
            && name2.is_none_or(|n| name_eq(n, &f.name))
        {
            f2 = Some(f);
            continue;
        }
    }

    // The request fails if no primary font was found, or if a second font was requested
    // but not found.
    if let Some(f1) = f1
        && (name2.is_none() || f2.is_some())
    {
        if VGAFONTDEBUG
            && (!ptr::eq(scr, vga_console_screen())
                || VGA_CONSOLE_ATTACHED.load(Ordering::Relaxed) != 0)
        {
            printf(format_args!(
                "vga ({}): font1={} (slot {})",
                Str(type_.name()),
                Str(f1.name()),
                f1.slot
            ));
            if let Some(f2) = f2 {
                printf(format_args!(
                    ", font2={} (slot {})",
                    Str(f2.name()),
                    f2.slot
                ));
            }
            printf(format_args!("\n"));
        }
        scr.fontset1.set(f1);
        scr.fontset2.set(f2.map_or(ptr::null(), ptr::from_ref));
        return Ok(());
    }
    Err(Errno::ENXIO)
}

/// `vga_init_screen`: `scr` as a screen of type `type_` on `vc`, put on its list;
/// `existing` for the first screen, which keeps what the display shows (cursor and start
/// address). Returns the default attribute.
pub fn vga_init_screen(
    vc: &VgaConfig,
    scr: &Vgascreen,
    type_: &WsscreenDescr,
    existing: bool,
) -> u32 {
    scr.cfg.set(vc);
    scr.pcs
        .hdl
        .set(ptr::from_ref::<PcdisplayHandle>(&vc.hdl.vh_ph));
    scr.pcs.type_.set(type_);
    scr.pcs.active.set(0);
    scr.mindispoffset.set(0);
    scr.maxdispoffset
        .set(0x8000 - type_.nrows * type_.ncols * 2);

    let mut cpos;
    if existing {
        cpos = i32::from(vga_6845_read!(&vc.hdl, cursorh)) << 8;
        cpos |= i32::from(vga_6845_read!(&vc.hdl, cursorl));

        // make sure we have a valid cursor position
        if cpos < 0 || cpos >= type_.nrows * type_.ncols {
            cpos = 0;
        }

        let mut dispoffset = i32::from(vga_6845_read!(&vc.hdl, startadrh)) << 9;
        dispoffset |= i32::from(vga_6845_read!(&vc.hdl, startadrl)) << 1;

        // make sure we have a valid memory offset
        if dispoffset < scr.mindispoffset.get() || dispoffset > scr.maxdispoffset.get() {
            dispoffset = scr.mindispoffset.get();
        }
        scr.pcs.dispoffset.set(dispoffset);
    } else {
        cpos = 0;
        scr.pcs.dispoffset.set(scr.mindispoffset.get());
    }
    scr.pcs.visibleoffset.set(scr.pcs.dispoffset.get());
    scr.vga_rollover.set(0);

    scr.pcs.vc_crow.set(cpos / type_.ncols);
    scr.pcs.vc_ccol.set(cpos % type_.ncols);
    pcdisplay_cursor_init(&scr.pcs, existing);

    // SAFETY: `scr` is a screen of this driver's, set up above.
    let res = unsafe { vga_pack_attr(ptr::from_ref(scr).cast_mut().cast(), 0, 0, 0) };
    #[cfg(feature = "diagnostic")]
    if res.is_err() {
        panic(format_args!("vga_init_screen: attribute botch"));
    }
    let attr = res.unwrap_or(0);

    scr.pcs.mem.set(ptr::null_mut());

    scr.fontset1.set(ptr::null());
    scr.fontset2.set(ptr::null());
    if vga_selectfont(vc, scr, None, None).is_err() {
        if ptr::eq(scr, vga_console_screen()) {
            panic(format_args!("vga_init_screen: no font"));
        } else {
            printf(format_args!("vga_init_screen: no font\n"));
        }
    }

    vc.nscreens.set(vc.nscreens.get() + 1);
    // SAFETY: `scr` is in no list (a fresh screen, or the console's set up once), lives
    // until `vga_free_screen` unlinks it, and the configuration does not move once it has
    // screens (malloc'd or static).
    unsafe { vc.screens.insert_head(scr) };

    attr
}

/// `vga_init`: map the VGA's registers and text memory on `iot` and `memt` and make its
/// configuration, with the firmware's font in slot 0 and the palette saved.
pub fn vga_init(iot: BusSpaceTag, memt: BusSpaceTag) -> VgaConfig {
    // SAFETY: the VGA's fixed legacy I/O range, which the caller probed or knows is there.
    let Ok(vh_ioh_vga) = (unsafe { bus_space_map(iot, 0x3c0, 0x10, 0) }) else {
        panic(format_args!("vga_common_setup: can't map vga i/o"));
    };

    // read "misc output register"
    let mor = bus_space_read_1(iot, vh_ioh_vga, 0xc);
    let vh_mono = i32::from(mor & 1 == 0);

    // SAFETY: the 6845's fixed legacy I/O range, as above.
    let Ok(vh_ioh_6845) =
        (unsafe { bus_space_map(iot, if vh_mono != 0 { 0x3b0 } else { 0x3d0 }, 0x10, 0) })
    else {
        panic(format_args!("vga_common_setup: can't map 6845 i/o"));
    };

    // SAFETY: the legacy video memory window at 0xa0000, as above.
    let Ok(vh_allmemh) = (unsafe { bus_space_map(memt, 0xa0000, 0x20000, 0) }) else {
        panic(format_args!("vga_common_setup: can't map mem space"));
    };

    let Ok(vh_memh) = bus_space_subregion(
        memt,
        vh_allmemh,
        if vh_mono != 0 { 0x10000 } else { 0x18000 },
        0x8000,
    ) else {
        panic(format_args!("vga_common_setup: mem subrange failed"));
    };

    let vc = VgaConfig::new(VgaHandle {
        vh_ph: PcdisplayHandle {
            ph_iot: iot,
            ph_memt: memt,
            ph_ioh_6845: vh_ioh_6845,
            ph_memh: vh_memh,
        },
        vh_ioh_vga,
        vh_allmemh,
        vh_mono,
    });

    vc.nscreens.set(0);
    vc.screens.init();
    vc.active.set(ptr::null());
    vc.currenttype.set(if vh_mono != 0 {
        &VGA_STDSCREEN_MONO.0
    } else {
        &VGA_STDSCREEN.0
    });

    vc.vc_fonts[0].set(&VGA_BUILTINFONT);
    for f in &vc.vc_fonts[1..] {
        f.set(ptr::null());
    }

    vc.currentfontset1.set(0);
    vc.currentfontset2.set(0);

    vga_save_palette(&vc);

    vc
}

/// `vga_common_attach`.
pub fn vga_common_attach(
    self_: &Device,
    iot: BusSpaceTag,
    memt: BusSpaceTag,
    type_: i32,
) -> Option<&'static VgaConfig> {
    vga_extended_attach(self_, iot, memt, type_, None)
}

/// `vga_extended_attach`: the configuration (the console's, or a new one) and the
/// `wsdisplay` child; `map` is the bus front-end's `mmap`.
pub fn vga_extended_attach(
    self_: &Device,
    iot: BusSpaceTag,
    memt: BusSpaceTag,
    type_: i32,
    map: Option<WsdisplayMmapFn>,
) -> Option<&'static VgaConfig> {
    let console = vga_is_console(iot, type_);
    if console {
        VGA_CONSOLE_ATTACHED.store(1, Ordering::Relaxed);
    }

    if type_ == -1 {
        return None;
    }

    let vc: &'static VgaConfig = match vga_console_vc().filter(|_| console) {
        Some(vc) => vc,
        None => {
            let p = malloc(size_of::<VgaConfig>(), M_DEVBUF, M_NOWAIT | M_ZERO)?
                .cast::<VgaConfig>()
                .as_ptr();
            let vc = vga_init(iot, memt);
            // SAFETY: a fresh allocation of one `struct vga_config`, malloc(9)-aligned and
            // kept for good, as in C.
            unsafe {
                ptr::write(p, vc);
                &*p
            }
        }
    };

    vc.vc_softc.set(self_);
    vc.vc_type.set(type_);
    vc.vc_mmap.set(map);

    let mut aa = WsemuldisplaydevAttachArgs {
        console: i32::from(console),
        primary: 0,
        scrdata: if vc.hdl.vh_mono != 0 {
            &VGA_SCREENLIST_MONO.0
        } else {
            &VGA_SCREENLIST.0
        },
        accessops: &VGA_ACCESSOPS,
        accesscookie: ptr::from_ref(vc).cast_mut().cast(),
        defaultscreens: 0,
    };

    let _ = config_found_sm(
        self_,
        ptr::from_mut(&mut aa).cast(),
        Some(wsemuldisplaydevprint),
        Some(wsemuldisplaydevsubmatch),
    );

    Some(vc)
}

/// `vga_cnattach`: the VGA on `iot`/`memt` as the console, after probing it when `check`.
pub fn vga_cnattach(
    iot: BusSpaceTag,
    memt: BusSpaceTag,
    type_: i32,
    check: bool,
) -> Result<(), Errno> {
    if check && !vga_common_probe(iot, memt) {
        return Err(Errno::ENXIO);
    }

    // set up bus-independent VGA configuration
    let vc = vga_init(iot, memt);
    // SAFETY: see `ConsoleVc`: cold, before anything else reads the console configuration.
    let vc: &'static VgaConfig = unsafe {
        let slot = &mut *VGA_CONSOLE_VC.0.get();
        *slot = Some(vc);
        match slot.as_ref() {
            Some(vc) => vc,
            None => return Err(Errno::ENXIO),
        }
    };
    // SAFETY: `currenttype` is one of this driver's static descriptors (`vga_init`).
    let scr: &'static WsscreenDescr = unsafe { &*vc.currenttype.get() };
    let console = vga_console_screen();
    let defattr = vga_init_screen(vc, console, scr, true);

    console.pcs.active.set(1);
    vc.active.set(console);

    // SAFETY: the console screen is this driver's emulops cookie for good.
    unsafe {
        wsdisplay_cnattach(
            scr,
            ptr::from_ref(console).cast_mut().cast(),
            console.pcs.vc_ccol.get(),
            console.pcs.vc_crow.get(),
            defattr,
        )
    };

    VGACONSOLE.store(1, Ordering::Relaxed);
    VGA_CONSOLE_TYPE.store(type_, Ordering::Relaxed);
    Ok(())
}

/// `vga_is_console`: whether the console made by `vga_cnattach` is the VGA on `iot` of
/// bus type `type_`, and not attached yet.
pub fn vga_is_console(iot: BusSpaceTag, type_: i32) -> bool {
    let console_type = VGA_CONSOLE_TYPE.load(Ordering::Relaxed);
    VGACONSOLE.load(Ordering::Relaxed) != 0
        && VGA_CONSOLE_ATTACHED.load(Ordering::Relaxed) == 0
        && vga_console_vc().is_some_and(|vc| iot == vc.hdl.vh_iot())
        && (console_type == -1 || type_ == console_type)
}

/// `vga_ioctl`.
///
/// # Safety
///
/// `v` is the access cookie (see [`vc_of`]); `data` is the kernel copy of the argument.
pub unsafe fn vga_ioctl(
    v: *mut c_void,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno> {
    // SAFETY: the caller's contract.
    let vc = unsafe { vc_of(v) };

    // NVGA_PCI > 0
    if vc.vc_type.get() == WSDISPLAY_TYPE_PCIVGA as i32 {
        match vga_pci_ioctl(v, cmd, data, flag, p) {
            Err(Errno::ENOTTY) => {}
            r => return r,
        }
    }

    match cmd {
        WSDISPLAYIO_GTYPE => {
            ioctl_ret(data, &(vc.vc_type.get() as u32));
            // XXX should get detailed hardware information here
        }

        WSDISPLAYIO_SMODE => {
            let mode: u32 = ioctl_arg(data);
            if mode == WSDISPLAYIO_MODE_EMUL {
                vga_restore_fonts(vc);
                vga_restore_palette(vc);
            }
        }

        WSDISPLAYIO_GVIDEO | WSDISPLAYIO_SVIDEO => {}

        // WSDISPLAYIO_GINFO, WSDISPLAYIO_GETCMAP, WSDISPLAYIO_PUTCMAP, WSDISPLAYIO_GCURPOS,
        // WSDISPLAYIO_SCURPOS, WSDISPLAYIO_GCURMAX, WSDISPLAYIO_GCURSOR,
        // WSDISPLAYIO_SCURSOR, and the rest:
        _ => {
            // NONE of these operations are by the generic VGA driver.
            return Err(Errno::ENOTTY);
        }
    }

    Ok(true)
}

/// `vga_mmap`: the bus front-end's `mmap`, if it has one.
///
/// # Safety
///
/// `v` is the access cookie (see [`vc_of`]).
pub unsafe fn vga_mmap(v: *mut c_void, offset: i64, prot: i32) -> Option<Paddr> {
    // SAFETY: the caller's contract.
    let vc = unsafe { vc_of(v) };

    let map = vc.vc_mmap.get()?;
    // SAFETY: the front-end's `mmap` takes the configuration it attached with.
    unsafe { map(v, offset, prot) }
}

/// `vga_alloc_screen`: a new screen of type `type_`.
///
/// # Safety
///
/// `v` is the access cookie (see [`vc_of`]); `type_` is one of the descriptors of the
/// screen list this driver attached with.
pub unsafe fn vga_alloc_screen(
    v: *mut c_void,
    type_: *const WsscreenDescr,
    cookiep: &mut *mut c_void,
    curxp: &mut i32,
    curyp: &mut i32,
    defattrp: &mut u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let vc = unsafe { vc_of(v) };
    // SAFETY: the caller's contract: a static descriptor of this driver's.
    let type_: &'static WsscreenDescr = unsafe { &*type_ };

    if vc.nscreens.get() == 1 {
        // When allocating the second screen, get backing store for the first one too.
        // XXX We could be more clever and use video RAM.
        if let Some(scr) = vc.screens.first() {
            let t = scr.pcs.type_();
            let mem = mallocarray(t.ncols as usize, t.nrows as usize * 2, M_DEVBUF, M_WAITOK)
                .ok_or(Errno::ENOMEM)?;
            scr.pcs.mem.set(mem.cast::<u16>().as_ptr());
        }
    }

    let p = malloc(size_of::<Vgascreen>(), M_DEVBUF, M_WAITOK)
        .ok_or(Errno::ENOMEM)?
        .cast::<Vgascreen>()
        .as_ptr();
    // SAFETY: a fresh allocation of one `struct vgascreen`, malloc(9)-aligned; it lives
    // until `vga_free_screen`.
    let scr: &'static Vgascreen = unsafe {
        ptr::write(p, Vgascreen::new());
        &*p
    };
    *defattrp = vga_init_screen(vc, scr, type_, vc.nscreens.get() == 0);

    if vc.nscreens.get() == 1 {
        scr.pcs.active.set(1);
        vc.active.set(scr);
        vc.currenttype.set(type_);
    } else {
        let mem = mallocarray(
            type_.ncols as usize,
            type_.nrows as usize * 2,
            M_DEVBUF,
            M_WAITOK,
        )
        .ok_or(Errno::ENOMEM)?;
        scr.pcs.mem.set(mem.cast::<u16>().as_ptr());
        // SAFETY: `scr` is a screen of this driver's.
        let _ = unsafe { pcdisplay_eraserows(p.cast(), 0, type_.nrows, *defattrp) };
    }

    *cookiep = p.cast();
    *curxp = scr.pcs.vc_ccol.get();
    *curyp = scr.pcs.vc_crow.get();

    Ok(())
}

/// `vga_free_screen`.
///
/// # Safety
///
/// `cookie` is a screen `vga_alloc_screen` made, not freed yet and not used afterwards.
pub unsafe fn vga_free_screen(_v: *mut c_void, cookie: *mut c_void) {
    // SAFETY: the caller's contract.
    let vs = unsafe { scr_of(cookie) };
    let vc = vs.cfg();

    // SAFETY: the screen is on its configuration's list (`vga_init_screen`).
    unsafe { crate::sys::queue::ListHead::<VgascreenList>::remove(vs) };
    vc.nscreens.set(vc.nscreens.get() - 1);
    let was_active = ptr::eq(vc.active.get(), vs);
    if !ptr::eq(vs, vga_console_screen()) {
        // deallocating the one but last screen removes backing store for the last one
        if vc.nscreens.get() == 1
            && let Some(last) = vc.screens.first()
            && let Some(mem) = NonNull::new(last.pcs.mem.get())
        {
            free(mem.cast(), M_DEVBUF, 0);
            last.pcs.mem.set(ptr::null_mut());
        }

        // Last screen has no backing store
        if vc.nscreens.get() != 0
            && let Some(mem) = NonNull::new(vs.pcs.mem.get())
        {
            free(mem.cast(), M_DEVBUF, 0);
        }

        if let Some(p) = NonNull::new(cookie) {
            free(p.cast(), M_DEVBUF, size_of::<Vgascreen>());
        }
    } else {
        panic(format_args!("vga_free_screen: console"));
    }

    if was_active {
        vc.active.set(ptr::null());
    }
}

/// `vga_setfont`: load the font slots of `scr` into the character generator's selection.
pub fn vga_setfont(vc: &VgaConfig, scr: &Vgascreen) {
    let fontslot1 = scr.fontset1().map_or(0, |f| f.slot);
    let fontslot2 = scr.fontset2().map_or(fontslot1, |f| f.slot);
    if vc.currentfontset1.get() != fontslot1 || vc.currentfontset2.get() != fontslot2 {
        vga_setfontset(&vc.hdl, fontslot1, fontslot2);
        vc.currentfontset1.set(fontslot1);
        vc.currentfontset2.set(fontslot2);
    }
}

/// `vga_show_screen`: switch to screen `cookie`, now or (with `cb`) from a timeout.
///
/// # Safety
///
/// `cookie` is a screen of this driver's (see [`scr_of`]).
pub unsafe fn vga_show_screen(
    _v: *mut c_void,
    cookie: *mut c_void,
    _waitok: i32,
    cb: Option<ShowScreenCb>,
    cbarg: *mut c_void,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(cookie) };
    let vc = scr.cfg();

    let oldscr = vc.active.get(); // can be NULL!
    if ptr::eq(scr, oldscr) {
        return Ok(());
    }

    vc.wantedscreen.set(scr);
    vc.switchcb.set(cb);
    vc.switchcbarg.set(cbarg);
    if cb.is_some() {
        let arg = ptr::from_ref(vc).cast_mut().cast();
        timeout_set(&vc.vc_switch_timeout, vga_doswitch, arg);
        timeout_add(&vc.vc_switch_timeout, 0);
        return Err(Errno::EAGAIN);
    }

    vga_doswitch(ptr::from_ref(vc).cast_mut().cast());
    Ok(())
}

/// `vga_doswitch`: the switch `vga_show_screen` asked for: save the old screen's cells to
/// its backing store, program the new screen's type, fonts and palette, and copy its cells
/// in.
fn vga_doswitch(arg: *mut c_void) {
    // SAFETY: `vga_show_screen` passes (or sets the timeout up with) its configuration,
    // which lives for good.
    let vc = unsafe { vc_of(arg) };
    let vh = &vc.hdl;

    // SAFETY: `wantedscreen` is a screen of this configuration's, set by `vga_show_screen`.
    let Some(scr) = (unsafe { vc.wantedscreen.get().as_ref() }) else {
        printf(format_args!("vga_doswitch: disappeared\n"));
        if let Some(cb) = vc.switchcb.get() {
            cb(vc.switchcbarg.get(), Errno::EIO as i32, 0);
        }
        return;
    };

    let type_ = scr.pcs.type_();
    let oldscr = vc.active(); // can be NULL!
    if oldscr.is_some_and(|o| ptr::eq(o, scr)) {
        return;
    }
    let s = spltty();
    #[cfg(feature = "diagnostic")]
    {
        if let Some(oldscr) = oldscr {
            if oldscr.pcs.active.get() == 0 {
                panic(format_args!("vga_show_screen: not active"));
            }
            if !ptr::eq(oldscr.pcs.type_.get(), vc.currenttype.get()) {
                panic(format_args!("vga_show_screen: bad type"));
            }
        }
        if scr.pcs.active.get() != 0 {
            panic(format_args!("vga_show_screen: active"));
        }
    }

    scr.vga_rollover.set(0);

    if let Some(oldscr) = oldscr {
        let oldtype = oldscr.pcs.type_();

        oldscr.pcs.active.set(0);
        let n = (oldtype.ncols * oldtype.nrows) as usize;
        let mem = oldscr.pcs.mem();
        bus_space_read_region_2(
            vh.vh_memt(),
            vh.vh_memh(),
            oldscr.pcs.dispoffset.get() as usize,
            &mem[..n.min(mem.len())],
        );
    }

    if !ptr::eq(vc.currenttype.get(), type_) {
        vga_setscreentype(vh, type_);
        vc.currenttype.set(type_);
    }

    vga_restore_fonts(vc);
    vga_setfont(vc, scr);
    vga_restore_palette(vc);

    scr.pcs.dispoffset.set(scr.mindispoffset.get());
    scr.pcs.visibleoffset.set(scr.pcs.dispoffset.get());
    if oldscr.is_none_or(|o| scr.pcs.dispoffset.get() != o.pcs.dispoffset.get()) {
        vga_6845_write!(vh, startadrh, (scr.pcs.dispoffset.get() >> 9) as u8);
        vga_6845_write!(vh, startadrl, (scr.pcs.dispoffset.get() >> 1) as u8);
    }

    let n = (type_.ncols * type_.nrows) as usize;
    let mem = scr.pcs.mem();
    bus_space_write_region_2(
        vh.vh_memt(),
        vh.vh_memh(),
        scr.pcs.dispoffset.get() as usize,
        &mem[..n.min(mem.len())],
    );
    scr.pcs.active.set(1);
    splx(s);

    vc.active.set(scr);

    pcdisplay_cursor_reset(&scr.pcs);
    let cookie = ptr::from_ref(scr).cast_mut().cast();
    // SAFETY: `scr` is a screen of this driver's.
    let _ = unsafe {
        pcdisplay_cursor(
            cookie,
            scr.pcs.cursoron.get(),
            scr.pcs.vc_crow.get(),
            scr.pcs.vc_ccol.get(),
        )
    };

    vc.wantedscreen.set(ptr::null());
    if let Some(cb) = vc.switchcb.get() {
        cb(vc.switchcbarg.get(), 0, 0);
    }
}

/// `vga_load_font`: with glyphs, load them into a free (or the requested) slot; without,
/// select fonts by name (`"name1,name2"`) for screen `cookie`.
///
/// # Safety
///
/// `v` is the access cookie (see [`vc_of`]); `cookie` is NULL or a screen of this
/// driver's; a non-NULL `data.data` points at `256 * fontheight` bytes of glyphs the
/// kernel keeps for good (wsdisplay's `WSDISPLAYIO_LDFONT` buffer).
pub unsafe fn vga_load_font(
    v: *mut c_void,
    cookie: *mut c_void,
    data: &mut WsdisplayFont,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let vc = unsafe { vc_of(v) };

    if data.data.is_null() {
        // SAFETY: the caller's contract.
        let Some(scr) = (unsafe { cookie.cast::<Vgascreen>().as_ref() }) else {
            return Err(Errno::EINVAL);
        };

        // "name1,name2": cut the name at the comma, in place, as the C does.
        let len = cstr(&data.name).len();
        let comma = data.name[..len].iter().position(|&c| c == b',');
        let split = match comma {
            Some(i) => {
                data.name[i] = 0;
                i + 1
            }
            None => len,
        };
        let (name1, name2) = data.name.split_at(split);
        let res = vga_selectfont(vc, scr, Some(name1), Some(name2));
        if res.is_ok() {
            vga_setfont(vc, scr);
        }
        return res;
    }

    if data.fontwidth != 8 || data.stride != 1 {
        return Err(Errno::EINVAL); // XXX 1 byte per line
    }
    if data.firstchar != 0 || data.numchars != 256 {
        return Err(Errno::EINVAL);
    }

    let slot = if data.index < 0 {
        (0..VGA_MAXFONT)
            .find(|&s| vc.vc_fonts[s].get().is_null())
            .unwrap_or(VGA_MAXFONT)
    } else {
        data.index as usize
    };

    if slot >= VGA_MAXFONT {
        return Err(Errno::ENOSPC);
    }

    if !vc.vc_fonts[slot].get().is_null() {
        return Err(Errno::EEXIST);
    }
    let Some(p) = malloc(size_of::<Vgafont>(), M_DEVBUF, M_WAITOK | M_CANFAIL) else {
        return Err(Errno::ENOMEM);
    };
    let p = p.cast::<Vgafont>().as_ptr();
    let mut name = [0u8; WSFONT_NAME_SIZE];
    let n = cstr(&data.name).len().min(WSFONT_NAME_SIZE - 1);
    name[..n].copy_from_slice(&data.name[..n]); // strlcpy
    let height = data.fontheight as i32;
    if VGAFONTDEBUG {
        printf(format_args!(
            "vga: load {} (8x{}, enc {}) font to slot {}\n",
            Str(cstr(&name)),
            height,
            data.encoding,
            slot
        ));
    }
    let fontdata = data.data.cast::<u8>().cast_const();
    // SAFETY: the caller's contract: `256 * fontheight` bytes, kept for good.
    let glyphs = unsafe { core::slice::from_raw_parts(fontdata, 256 * data.fontheight as usize) };
    vga_loadchars(&vc.hdl, slot as i32, 0, 256, height, glyphs);
    // SAFETY: a fresh allocation of one `struct vgafont`, malloc(9)-aligned, kept for good
    // in `vc_fonts`.
    unsafe {
        ptr::write(
            p,
            Vgafont {
                name,
                height,
                encoding: data.encoding,
                slot: slot as i32,
                fontdata,
            },
        )
    };
    vc.vc_fonts[slot].set(p);
    data.cookie = p.cast();
    data.index = slot as i32;

    Ok(())
}

/// `vga_list_font`: the font in slot `data.index`.
///
/// # Safety
///
/// `v` is the access cookie (see [`vc_of`]).
pub unsafe fn vga_list_font(v: *mut c_void, data: &mut WsdisplayFont) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let vc = unsafe { vc_of(v) };

    if data.index < 0 || data.index as usize >= VGA_MAXFONT {
        return Err(Errno::EINVAL);
    }

    let Some(f) = vc.font(data.index as usize) else {
        return Err(Errno::EINVAL);
    };

    data.name = [0; WSFONT_NAME_SIZE];
    let n = f.name().len().min(WSFONT_NAME_SIZE - 1);
    data.name[..n].copy_from_slice(&f.name()[..n]); // strlcpy
    data.firstchar = 0;
    data.numchars = 256;
    data.encoding = f.encoding;
    data.fontwidth = 8;
    data.fontheight = f.height as u32;
    data.stride = 1;
    data.bitorder = WSDISPLAY_FONTORDER_L2R;
    data.byteorder = WSDISPLAY_FONTORDER_L2R;

    Ok(())
}

/// `vga_scrollback`: show `lines` rows further back (negative) or forward in the text
/// memory the screen scrolled through; 0 returns to the bottom.
///
/// # Safety
///
/// `v` is the access cookie (see [`vc_of`]) and `cookie` a screen of this driver's.
pub unsafe fn vga_scrollback(v: *mut c_void, cookie: *mut c_void, lines: i32) {
    // SAFETY: the caller's contract.
    let vc = unsafe { vc_of(v) };
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(cookie) };
    let vh = &vc.hdl;

    if lines == 0 {
        if scr.pcs.visibleoffset.get() == scr.pcs.dispoffset.get() {
            return;
        }

        scr.pcs.visibleoffset.set(scr.pcs.dispoffset.get()); // reset
    } else {
        let t = scr.pcs.type_();
        let margin = t.ncols * 2;

        let vga_scr_end = scr.pcs.dispoffset.get() + t.ncols * t.nrows * 2;
        let (ul, we) = if scr.vga_rollover.get() > vga_scr_end + margin {
            (vga_scr_end, scr.vga_rollover.get() + t.ncols * 2)
        } else {
            (0, 0x8000)
        };
        let mut p = (scr.pcs.visibleoffset.get() - ul + we) % we + lines * (t.ncols * 2);
        let st = (scr.pcs.dispoffset.get() - ul + we) % we;
        if p < margin {
            p = 0;
        }
        if p > st - margin {
            p = st;
        }
        scr.pcs.visibleoffset.set((p + ul) % we);
    }

    // update visible position
    vga_6845_write!(vh, startadrh, (scr.pcs.visibleoffset.get() >> 9) as u8);
    vga_6845_write!(vh, startadrl, (scr.pcs.visibleoffset.get() >> 1) as u8);
}

/// `vga_pack_attr`: the cell attribute of colours `fg`/`bg` and `WSATTR_*` `flags`.
///
/// # Safety
///
/// `id` is a screen of this driver's (see [`scr_of`]).
pub unsafe fn vga_pack_attr(id: *mut c_void, fg: i32, bg: i32, flags: i32) -> Result<u32, Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(id) };
    let vc = scr.cfg();
    let mut attr;

    if vc.hdl.vh_mono != 0 {
        if flags & WSATTR_WSCOLORS != 0 {
            return Err(Errno::EINVAL);
        }
        attr = if flags & WSATTR_REVERSE != 0 {
            0x70
        } else {
            0x07
        };
        if flags & WSATTR_UNDERLINE != 0 {
            attr |= FG_UNDERLINE;
        }
        if flags & WSATTR_HILIT != 0 {
            attr |= FG_INTENSE;
        }
    } else {
        if flags & (WSATTR_UNDERLINE | WSATTR_REVERSE) != 0 {
            return Err(Errno::EINVAL);
        }
        attr = if flags & WSATTR_WSCOLORS != 0 {
            FGANSITOPC[(fg & 7) as usize] | BGANSITOPC[(bg & 7) as usize]
        } else {
            7
        };
        if flags & WSATTR_HILIT != 0 || fg & 8 != 0 || bg & 8 != 0 {
            attr += 8;
        }
    }
    if flags & WSATTR_BLINK != 0 {
        attr |= FG_BLINK;
    }
    Ok(attr)
}

/// `vga_unpack_attr`: `(fg, bg, underline)` of a cell attribute.
///
/// # Safety
///
/// `id` is a screen of this driver's (see [`scr_of`]).
pub unsafe fn vga_unpack_attr(id: *mut c_void, attr: u32) -> (i32, i32, i32) {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(id) };
    let vc = scr.cfg();
    let (mut fg, bg, ul);

    if vc.hdl.vh_mono != 0 {
        fg = if attr & 0x07 == 0x07 {
            WSCOL_WHITE
        } else {
            WSCOL_BLACK
        };
        bg = if attr & 0x70 != 0 {
            WSCOL_WHITE
        } else {
            WSCOL_BLACK
        };
        ul = i32::from(fg != WSCOL_WHITE && attr & 0x01 != 0);
    } else {
        fg = PCTOANSI[(attr & 0x07) as usize];
        bg = PCTOANSI[((attr & 0x70) >> 4) as usize];
        ul = 0;
    }
    if attr & FG_INTENSE != 0 {
        fg += 8;
    }
    (fg, bg, ul)
}

/// `vga_copyrows`: as `pcdisplay_copyrows`, but a scroll of the whole displayed screen
/// moves the 6845's start address instead of the cells, and wraps to the start of the
/// text memory when it reaches the end.
///
/// # Safety
///
/// `id` is a screen of this driver's (see [`scr_of`]).
pub unsafe fn vga_copyrows(
    id: *mut c_void,
    srcrow: i32,
    dstrow: i32,
    nrows: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(id) };
    let ph = scr.pcs.hdl();
    let (memt, memh) = (ph.ph_memt, ph.ph_memh);
    let ncols = scr.pcs.type_().ncols;

    let srcoff = srcrow * ncols;
    let dstoff = dstrow * ncols;

    let s = spltty();
    if scr.pcs.active.get() != 0 {
        if dstrow == 0 && (srcrow + nrows == scr.pcs.type_().nrows) {
            let cursoron = scr.pcs.cursoron.get();
            if PCDISPLAY_SOFTCURSOR && cursoron != 0 {
                // NOTE this assumes pcdisplay_cursor() never fails
                // SAFETY: the caller's contract.
                let _ = unsafe {
                    pcdisplay_cursor(id, 0, scr.pcs.vc_crow.get(), scr.pcs.vc_ccol.get())
                };
            }
            // scroll up whole screen
            if scr.pcs.dispoffset.get() + srcrow * ncols * 2 <= scr.maxdispoffset.get() {
                scr.pcs
                    .dispoffset
                    .set(scr.pcs.dispoffset.get() + srcrow * ncols * 2);
            } else {
                bus_space_copy_2(
                    memt,
                    memh,
                    (scr.pcs.dispoffset.get() + srcoff * 2) as usize,
                    memh,
                    scr.mindispoffset.get() as usize,
                    (nrows * ncols) as usize,
                );
                scr.vga_rollover.set(scr.pcs.dispoffset.get());
                scr.pcs.dispoffset.set(scr.mindispoffset.get());
            }
            scr.pcs.visibleoffset.set(scr.pcs.dispoffset.get());
            let vh = &scr.cfg().hdl;
            vga_6845_write!(vh, startadrh, (scr.pcs.dispoffset.get() >> 9) as u8);
            vga_6845_write!(vh, startadrl, (scr.pcs.dispoffset.get() >> 1) as u8);
            if PCDISPLAY_SOFTCURSOR && cursoron != 0 {
                // NOTE this assumes pcdisplay_cursor() never fails
                // SAFETY: the caller's contract.
                let _ = unsafe {
                    pcdisplay_cursor(id, 1, scr.pcs.vc_crow.get(), scr.pcs.vc_ccol.get())
                };
            }
        } else {
            bus_space_copy_2(
                memt,
                memh,
                (scr.pcs.dispoffset.get() + srcoff * 2) as usize,
                memh,
                (scr.pcs.dispoffset.get() + dstoff * 2) as usize,
                (nrows * ncols) as usize,
            );
        }
    } else {
        let mem = scr.pcs.mem();
        let (src, dst, n) = (srcoff as usize, dstoff as usize, (nrows * ncols) as usize);
        // bcopy: an overlapping move
        if src >= dst {
            for i in 0..n {
                if let (Some(s), Some(d)) = (mem.get(src + i), mem.get(dst + i)) {
                    d.set(s.get());
                }
            }
        } else {
            for i in (0..n).rev() {
                if let (Some(s), Some(d)) = (mem.get(src + i), mem.get(dst + i)) {
                    d.set(s.get());
                }
            }
        }
    }
    splx(s);

    Ok(())
}

/// `_vga_mapchar`: the glyph of `uni` in `font`'s encoding.
fn _vga_mapchar(font: &Vgafont, uni: i32, index: &mut u32) -> i32 {
    match font.encoding {
        WSDISPLAY_FONTENC_ISO => {
            if uni < 256 {
                *index = uni as u32;
                5
            } else {
                *index = u32::from(b'?');
                0
            }
        }
        WSDISPLAY_FONTENC_IBM => pcdisplay_mapchar(uni, index),
        _ => {
            if VGAFONTDEBUG {
                printf(format_args!("_vga_mapchar: encoding={}\n", font.encoding));
            }
            *index = u32::from(b'?');
            0
        }
    }
}

/// `vga_mapchar`: the glyph of `uni` in the screen's first font or, when it maps better,
/// its second (attribute bit 3 selects it).
///
/// # Safety
///
/// `id` is a screen of this driver's (see [`scr_of`]).
pub unsafe fn vga_mapchar(id: *mut c_void, uni: i32, index: &mut u32) -> i32 {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(id) };

    let mut res1 = 0;
    let mut idx1 = u32::from(b' '); // space
    if let Some(f) = scr.fontset1() {
        res1 = _vga_mapchar(f, uni, &mut idx1);
    }
    let mut res2 = -1;
    let mut idx2 = 0;
    if let Some(f) = scr.fontset2() {
        crate::kassert!(vga_screen_cantwofonts(scr.pcs.type_()));
        res2 = _vga_mapchar(f, uni, &mut idx2);
    }
    if res2 >= res1 {
        *index = idx2 | 0x0800; // attribute bit 3
        return res2;
    }
    *index = idx1;
    res1
}

/// `vga_putchar`: back to the bottom of the screen if it was scrolled back, then
/// `pcdisplay_putchar`.
///
/// # Safety
///
/// `c` is a screen of this driver's (see [`scr_of`]).
pub unsafe fn vga_putchar(
    c: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(c) };

    let s = spltty();
    if scr.pcs.active.get() != 0 && scr.pcs.visibleoffset.get() != scr.pcs.dispoffset.get() {
        let vc = ptr::from_ref(scr.cfg()).cast_mut().cast();
        // SAFETY: the screen's configuration is the access cookie, and `c` the screen.
        unsafe { vga_scrollback(vc, c, 0) };
    }
    // SAFETY: the caller's contract.
    let rc = unsafe { pcdisplay_putchar(c, row, col, uc, attr) };
    splx(s);

    rc
}

/// `vga_burner`: blank (`on` 0) or unblank the display; with `WSDISPLAY_BURN_VBLANK`, also
/// stop the vertical sync.
///
/// # Safety
///
/// `v` is the access cookie (see [`vc_of`]).
pub unsafe fn vga_burner(v: *mut c_void, on: u32, flags: u32) {
    // SAFETY: the caller's contract.
    let vc = unsafe { vc_of(v) };
    let vh = &vc.hdl;

    let s = splhigh();
    vga_ts_write!(vh, syncreset, 0x01);
    if on != 0 {
        vga_ts_write!(vh, mode, vga_ts_read!(vh, mode) & !0x20);
        let r = vga_6845_read!(vh, mode) | 0x80;
        delay(10000);
        vga_6845_write!(vh, mode, r);
    } else {
        vga_ts_write!(vh, mode, vga_ts_read!(vh, mode) | 0x20);
        if flags & WSDISPLAY_BURN_VBLANK != 0 {
            let r = vga_6845_read!(vh, mode) & !0x80;
            delay(10000);
            vga_6845_write!(vh, mode, r);
        }
    }
    vga_ts_write!(vh, syncreset, 0x03);
    splx(s);
}

/// `vga_getchar`: the cell at (`row`, `col`) of the active screen.
///
/// # Safety
///
/// `c` is the access cookie (see [`vc_of`]).
pub unsafe fn vga_getchar(c: *mut c_void, row: i32, col: i32) -> Option<WsdisplayCharcell> {
    // SAFETY: the caller's contract.
    let vc = unsafe { vc_of(c) };

    let active = vc.active.get();
    if active.is_null() {
        return None;
    }
    // SAFETY: the active screen is a screen of this driver's.
    Some(unsafe { pcdisplay_getchar(active.cast_mut().cast(), row, col) })
}

/// `vga_save_palette`: read the DAC's 256 colours into `vc_palette`.
pub fn vga_save_palette(vc: &VgaConfig) {
    let vh = &vc.hdl;

    if vh.vh_mono != 0 {
        return;
    }

    vga_raw_write(vh, VGA_DAC_MASK, 0xff);
    vga_raw_write(vh, VGA_DAC_READ, 0x00);
    for c in &vc.vc_palette {
        c.set(vga_raw_read(vh, VGA_DAC_DATA));
    }
}

/// `vga_restore_palette`: write `vc_palette` back to the DAC.
pub fn vga_restore_palette(vc: &VgaConfig) {
    let vh = &vc.hdl;

    if vh.vh_mono != 0 {
        return;
    }

    vga_raw_write(vh, VGA_DAC_MASK, 0xff);
    vga_raw_write(vh, VGA_DAC_WRITE, 0x00);
    for c in &vc.vc_palette {
        vga_raw_write(vh, VGA_DAC_DATA, c.get());
    }
}

/// `vga_restore_fonts`: load every font with glyphs back into its slot.
pub fn vga_restore_fonts(vc: &VgaConfig) {
    for slot in 0..VGA_MAXFONT {
        let Some(f) = vc.font(slot) else {
            continue;
        };
        if f.fontdata.is_null() {
            continue;
        }

        // SAFETY: a loaded font's glyphs are `256 * height` bytes kept for good
        // (`vga_load_font`).
        let glyphs =
            unsafe { core::slice::from_raw_parts(f.fontdata, 256 * f.height.max(0) as usize) };
        vga_loadchars(&vc.hdl, slot as i32, 0, 256, f.height, glyphs);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of `vga.c`: attributes, character mapping and font selection, screens and
    // their backing stores, scrollback arithmetic, fonts and ioctls. The host's bus space
    // reads zeros (so `vga_init` sees a monochrome adapter) and drops writes.

    use std::boxed::Box;
    use std::vec;

    use super::*;
    use crate::machine::bus::BusSpaceTag;

    /// A configuration as `vga_init` makes it, monochrome or (with its handle reused) colour,
    /// in a box that stays in place.
    fn config(mono: bool) -> Box<VgaConfig> {
        let t = BusSpaceTag::default();
        let mut hdl = vga_init(t, t).hdl;
        assert_eq!(hdl.vh_mono, 1); // the host reads 0 from the misc output register
        hdl.vh_mono = i32::from(mono);
        let vc = Box::new(VgaConfig::new(hdl));
        vc.currenttype.set(if mono {
            &VGA_STDSCREEN_MONO.0
        } else {
            &VGA_STDSCREEN.0
        });
        vc.vc_fonts[0].set(&VGA_BUILTINFONT);
        vc
    }

    fn cookie<T>(x: &T) -> *mut c_void {
        ptr::from_ref(x).cast_mut().cast()
    }

    /// A screen of `type_` on `vc` (not on the configuration's list: a test's own).
    fn screen(vc: &VgaConfig, type_: &WsscreenDescr) -> Box<Vgascreen> {
        let scr = Box::new(Vgascreen::new());
        scr.cfg.set(vc);
        scr.pcs.hdl.set(&vc.hdl.vh_ph);
        scr.pcs.type_.set(type_);
        scr
    }

    #[test]
    fn colour_attributes_pack_and_unpack() {
        let vc = config(false);
        let scr = screen(&vc, &VGA_STDSCREEN.0);
        let id = cookie(&*scr);
        // SAFETY: `id` is a vga screen in all the calls below.
        unsafe {
            assert_eq!(vga_pack_attr(id, 0, 0, 0), Ok(7));
            let a = vga_pack_attr(id, WSCOL_RED, WSCOL_BLUE, WSATTR_WSCOLORS).unwrap();
            assert_eq!(a, FG_RED | BG_BLUE);
            assert_eq!(vga_unpack_attr(id, a), (WSCOL_RED, WSCOL_BLUE, 0));
            let a = vga_pack_attr(id, WSCOL_GREEN, 0, WSATTR_WSCOLORS | WSATTR_HILIT).unwrap();
            assert_eq!(vga_unpack_attr(id, a), (WSCOL_GREEN + 8, WSCOL_BLACK, 0));
            assert_eq!(vga_pack_attr(id, 0, 0, WSATTR_BLINK), Ok(7 | FG_BLINK));
            assert_eq!(vga_pack_attr(id, 0, 0, WSATTR_REVERSE), Err(Errno::EINVAL));
        }
    }

    #[test]
    fn mono_attributes_pack_and_unpack() {
        let vc = config(true);
        let scr = screen(&vc, &VGA_STDSCREEN_MONO.0);
        let id = cookie(&*scr);
        // SAFETY: `id` is a vga screen in all the calls below.
        unsafe {
            assert_eq!(vga_pack_attr(id, 0, 0, WSATTR_WSCOLORS), Err(Errno::EINVAL));
            assert_eq!(vga_pack_attr(id, 0, 0, WSATTR_REVERSE), Ok(0x70));
            let a = vga_pack_attr(id, 0, 0, WSATTR_UNDERLINE | WSATTR_HILIT).unwrap();
            assert_eq!(a, 0x07 | FG_UNDERLINE | FG_INTENSE);
            assert_eq!(vga_unpack_attr(id, 0x70), (WSCOL_BLACK, WSCOL_WHITE, 0));
            assert_eq!(vga_unpack_attr(id, 0x01), (WSCOL_BLACK, WSCOL_BLACK, 1));
        }
    }

    #[test]
    fn fonts_are_selected_by_height_and_name_and_map_characters() {
        let vc = config(false);
        let scr = screen(&vc, &VGA_STDSCREEN.0);
        assert_eq!(vga_selectfont(&vc, &scr, None, None), Ok(()));
        assert!(ptr::eq(scr.fontset1.get(), &VGA_BUILTINFONT));
        assert!(scr.fontset2().is_none());
        // no 10-line font for the 80x40 screens
        let scr40 = screen(&vc, &VGA_40LSCREEN.0);
        assert_eq!(vga_selectfont(&vc, &scr40, None, None), Err(Errno::ENXIO));
        // a second font only on a screen without highlighting
        let iso = Box::new(Vgafont {
            name: font_name(b"iso"),
            height: 16,
            encoding: WSDISPLAY_FONTENC_ISO,
            slot: 1,
            fontdata: ptr::null(),
        });
        vc.vc_fonts[1].set(&*iso);
        assert_eq!(
            vga_selectfont(&vc, &scr, None, Some(b"iso\0")),
            Err(Errno::ENXIO)
        );
        let bf = screen(&vc, &VGA_STDSCREEN_BF.0);
        assert_eq!(
            vga_selectfont(&vc, &bf, Some(b"builtin"), Some(b"iso")),
            Ok(())
        );
        assert!(ptr::eq(bf.fontset2.get(), &*iso));

        let mut i = 0;
        // SAFETY: vga screens.
        unsafe {
            // the builtin (IBM) font maps 0xe9 to its CP437 glyph; the ISO one as is, and as
            // well, so the second font wins with attribute bit 3
            assert_eq!(vga_mapchar(cookie(&*scr), 0xe9, &mut i), 5);
            assert_eq!(i, 0x82);
            assert_eq!(vga_mapchar(cookie(&*bf), 0xe9, &mut i), 5);
            assert_eq!(i, 0xe9 | 0x0800);
            // only the IBM font has a box drawing glyph
            assert_eq!(vga_mapchar(cookie(&*bf), 0x2500, &mut i), 5);
            assert_eq!(i, 0xc4);
        }
    }

    #[test]
    fn second_screen_gets_backing_store_and_free_returns_it() {
        let _g = crate::kern::subr_pool::tests::setup_real_memory();
        let vc = config(false);
        let v = cookie(&*vc);
        let (mut c1, mut c2) = (ptr::null_mut(), ptr::null_mut());
        let (mut x, mut y, mut attr) = (0, 0, 0);
        // SAFETY: `v` is a configuration and the types are its descriptors.
        unsafe {
            vga_alloc_screen(v, &VGA_STDSCREEN.0, &mut c1, &mut x, &mut y, &mut attr).unwrap();
            let s1 = &*c1.cast::<Vgascreen>();
            assert_eq!(s1.pcs.active.get(), 1);
            assert!(s1.pcs.mem.get().is_null());
            assert!(ptr::eq(vc.active.get(), s1));
            assert_eq!(attr, 7);

            vga_alloc_screen(v, &VGA_STDSCREEN.0, &mut c2, &mut x, &mut y, &mut attr).unwrap();
            let s2 = &*c2.cast::<Vgascreen>();
            assert_eq!(vc.nscreens.get(), 2);
            assert_eq!(s2.pcs.active.get(), 0);
            assert_eq!(s1.pcs.mem().len(), 80 * 25);
            assert!(s2.pcs.mem().iter().all(|c| c.get() == 0x0720));

            // the inactive screen scrolls in its backing store
            s2.pcs.mem()[80].set(0x0741);
            vga_copyrows(c2, 1, 0, 24).unwrap();
            assert_eq!(s2.pcs.mem()[0].get(), 0x0741);

            vga_free_screen(v, c2);
            assert_eq!(vc.nscreens.get(), 1);
            assert!(s1.pcs.mem.get().is_null());
            assert!(ptr::eq(vc.screens.first().unwrap(), s1));
            assert!(vga_getchar(v, 0, 0).is_some());
        }
    }

    #[test]
    fn scrollback_moves_the_visible_offset_within_the_text_memory() {
        let vc = config(false);
        let scr = screen(&vc, &VGA_STDSCREEN.0);
        scr.maxdispoffset.set(0x8000 - 80 * 25 * 2);
        scr.pcs.dispoffset.set(160 * 30);
        scr.pcs.visibleoffset.set(160 * 30);
        // SAFETY: a configuration and its screen.
        unsafe {
            vga_scrollback(cookie(&*vc), cookie(&*scr), -10);
            assert_eq!(scr.pcs.visibleoffset.get(), 160 * 20);
            // never past the start of the memory
            vga_scrollback(cookie(&*vc), cookie(&*scr), -100);
            assert_eq!(scr.pcs.visibleoffset.get(), 0);
            // nor past the bottom
            vga_scrollback(cookie(&*vc), cookie(&*scr), 100);
            assert_eq!(scr.pcs.visibleoffset.get(), 160 * 30);
            vga_scrollback(cookie(&*vc), cookie(&*scr), -1);
            vga_scrollback(cookie(&*vc), cookie(&*scr), 0);
            assert_eq!(scr.pcs.visibleoffset.get(), scr.pcs.dispoffset.get());
        }
    }

    #[test]
    fn fonts_load_into_free_slots_and_list() {
        let _g = crate::kern::subr_pool::tests::setup_real_memory();
        let vc = config(false);
        let v = cookie(&*vc);
        let glyphs = vec![0u8; 256 * 8].leak();
        let mut f = WsdisplayFont::zeroed();
        f.name[..4].copy_from_slice(b"tiny");
        f.index = -1;
        f.numchars = 256;
        f.fontwidth = 8;
        f.fontheight = 8;
        f.stride = 1;
        f.encoding = WSDISPLAY_FONTENC_IBM;
        f.data = glyphs.as_mut_ptr().cast();
        // SAFETY: `v` is a configuration; the glyphs are leaked, kept for good.
        unsafe {
            assert_eq!(vga_load_font(v, ptr::null_mut(), &mut f), Ok(()));
            assert_eq!(f.index, 1);
            f.index = 1;
            assert_eq!(
                vga_load_font(v, ptr::null_mut(), &mut f),
                Err(Errno::EEXIST)
            );
            f.stride = 2;
            assert_eq!(
                vga_load_font(v, ptr::null_mut(), &mut f),
                Err(Errno::EINVAL)
            );

            let mut l = WsdisplayFont::zeroed();
            l.index = 1;
            assert_eq!(vga_list_font(v, &mut l), Ok(()));
            assert_eq!(cstr(&l.name), b"tiny");
            assert_eq!((l.fontheight, l.numchars, l.stride), (8, 256, 1));
            l.index = 2;
            assert_eq!(vga_list_font(v, &mut l), Err(Errno::EINVAL));

            // select it by name for an 80x50 screen (8 lines)
            let scr = screen(&vc, &VGA_50LSCREEN_BF.0);
            let mut u = WsdisplayFont::zeroed();
            u.name[..7].copy_from_slice(b"tiny,xx");
            assert_eq!(vga_load_font(v, cookie(&*scr), &mut u), Err(Errno::ENXIO));
            u.name = [0; WSFONT_NAME_SIZE];
            u.name[..4].copy_from_slice(b"tiny");
            assert_eq!(vga_load_font(v, cookie(&*scr), &mut u), Ok(()));
            assert_eq!(scr.fontset1().map(|f| f.slot), Some(1));
        }
    }

    #[test]
    fn ioctls_of_the_generic_vga() {
        let vc = config(false);
        vc.vc_type.set(WSDISPLAY_TYPE_PCIVGA as i32);
        let v = cookie(&*vc);
        let mut data = [0u8; 4];
        // SAFETY: `v` is a configuration; `data` holds an int.
        unsafe {
            assert_eq!(
                vga_ioctl(v, WSDISPLAYIO_GTYPE, &mut data, 0, None),
                Ok(true)
            );
            assert_eq!(u32::from_ne_bytes(data), WSDISPLAY_TYPE_PCIVGA);
            assert_eq!(
                vga_ioctl(
                    v,
                    crate::dev::wscons::wsconsio::WSDISPLAYIO_GINFO,
                    &mut data,
                    0,
                    None
                ),
                Err(Errno::ENOTTY)
            );
            assert_eq!(vga_mmap(v, 0, 0), None);
            assert!(vga_getchar(v, 0, 0).is_none());
        }
        assert!(!vga_is_console(
            BusSpaceTag::default(),
            WSDISPLAY_TYPE_PCIVGA as i32
        ));
    }
}
/* </TESTS> */
