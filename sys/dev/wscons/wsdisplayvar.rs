/* <LICENSES> */
/*
 * Copyright (c) 1996, 1997 Christopher G. Demetriou.  All rights reserved.
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
//! `<dev/wscons/wsdisplayvar.h>`: the interface between display drivers and
//! `wsdisplay(4)`.
//!
//! Upstream: sys/dev/wscons/wsdisplayvar.h @ 3ce1f3f79392
//!
//! A display driver (`efifb(4)`, `simplefb`, `vga(4)`, ...) attaches a `wsdisplay` child with
//! a [`WsemuldisplaydevAttachArgs`]: the screen types it offers ([`WsscreenList`] of
//! [`WsscreenDescr`], each with the [`WsdisplayEmulops`] a terminal emulation draws with), its
//! [`WsdisplayAccessops`] (screens, fonts, ioctls, `mmap`) and the cookie they are called
//! with. Raster drivers fill all of this from rasops (`dev/rasops`).
//!
//! ## Deviations
//! - The operation tables are structures of `Option<unsafe fn>` pointers (NULL is `None`)
//!   over the C's `void *` cookie, as `docs/C_TO_RUST.md` has it for a table over an opaque
//!   pointer only the table's owner understands: each operation is `unsafe`, its contract
//!   that the cookie is the one the driver paired with the table (the screen cookie its
//!   `alloc_screen` returned for the emulops, the access cookie of the attach arguments for
//!   the accessops). The tables are `Copy`, so a driver can keep one in a `Cell`.
//! - Errors: the emulops and accessops that return 0 or an errno return
//!   `Result<(), Errno>` (`pack_attr` returns the packed attribute in the `Ok`);
//!   `mapchar` keeps its quality value and out-parameter; `unpack_attr` returns
//!   `(fg, bg, underline)`; `ioctl` returns `Ok(true)` where the C returns 0, `Ok(false)`
//!   where it returns -1 (not the driver's), or the errno, as `wskbdvar.rs`'s does; `mmap`
//!   returns `None` where the C returns -1; `getchar` returns the cell, `None` where the C
//!   returns non-zero.
//! - `textops`, `screens` and `scrdata` stay raw pointers: the C points them into the
//!   driver's softc or its `rasops_info` (whose operation table rasops rewrites when it
//!   reconfigures), and wsdisplay reads through them for the life of the screen.
//! - `WS_DEFAULT_FG`/`WS_DEFAULT_BG` are the non-sparc64 values (white on black).
//! - The prototypes of `wsdisplay_cnattach`, `wsemuldisplaydevprint`,
//!   `wsemuldisplaydevsubmatch`, `wsdisplay_cnputc`, the `wsscreen_*_sync` and
//!   `wsdisplay_*` functions are `wsdisplay.c`'s, defined in `wsdisplay.rs`, with the opaque
//!   `struct wsdisplay_softc` and `struct wsscreen`. The `ws_get_param`/`ws_set_param` hooks
//!   `wsdisplay.c` defines are here, as the [`WS_GET_PARAM`]/[`WS_SET_PARAM`] cells the
//!   display drivers and `wsdisplay.rs` read; nothing sets them yet.
//! - The `wsemuldisplaydevcf_*` and `wsdisplaydevcf_mux` locator macros are functions of the
//!   `cfdata`; a missing locator reads as the default of `sys/conf/files`'s
//!   `define wsemuldisplaydev {[console = -1], [primary = -1], [mux = 1]}` and
//!   `define wsdisplaydev {[mux = 1]}`.

use core::ffi::c_void;
use core::ptr;

use libkern::StaticCell;

use crate::dev::wscons::wsconsio::{WSSCREEN_NAME_SIZE, WsdisplayFont, WsdisplayParam};
use crate::sys::device::Cfdata;
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::types::Paddr;

/// `WSCOL_BLACK`: fg / bg values, identical to ANSI terminal color codes.
pub const WSCOL_BLACK: i32 = 0;
/// `WSCOL_RED`.
pub const WSCOL_RED: i32 = 1;
/// `WSCOL_GREEN`.
pub const WSCOL_GREEN: i32 = 2;
/// `WSCOL_BROWN`.
pub const WSCOL_BROWN: i32 = 3;
/// `WSCOL_BLUE`.
pub const WSCOL_BLUE: i32 = 4;
/// `WSCOL_MAGENTA`.
pub const WSCOL_MAGENTA: i32 = 5;
/// `WSCOL_CYAN`.
pub const WSCOL_CYAN: i32 = 6;
/// `WSCOL_WHITE`.
pub const WSCOL_WHITE: i32 = 7;

/// `WS_DEFAULT_FG`: white on black, except on Sun hardware (black on white there, to match
/// the firmware console).
pub const WS_DEFAULT_FG: i32 = WSCOL_WHITE;
/// `WS_DEFAULT_BG`.
pub const WS_DEFAULT_BG: i32 = WSCOL_BLACK;

/// `WSDISPLAY_MAXSCREEN`.
pub const WSDISPLAY_MAXSCREEN: i32 = 12;

/// `WSATTR_REVERSE`: flag values of an attribute.
pub const WSATTR_REVERSE: i32 = 1;
/// `WSATTR_HILIT`.
pub const WSATTR_HILIT: i32 = 2;
/// `WSATTR_BLINK`.
pub const WSATTR_BLINK: i32 = 4;
/// `WSATTR_UNDERLINE`.
pub const WSATTR_UNDERLINE: i32 = 8;
/// `WSATTR_WSCOLORS`.
pub const WSATTR_WSCOLORS: i32 = 16;

/// `cursor`: turn the cursor off or on at (`row`, `col`).
pub type CursorFn = unsafe fn(c: *mut c_void, on: i32, row: i32, col: i32) -> Result<(), Errno>;
/// `mapchar`: the glyph for character `ch` in `*cp`; returns the quality of the mapping (0
/// for a substitute).
pub type MapcharFn = unsafe fn(c: *mut c_void, ch: i32, cp: &mut u32) -> i32;
/// `putchar`: draw `uc` with attribute `attr` at (`row`, `col`).
pub type PutcharFn =
    unsafe fn(c: *mut c_void, row: i32, col: i32, uc: u32, attr: u32) -> Result<(), Errno>;
/// `copycols`: copy `ncols` cells of `row` from `srccol` to `dstcol`.
pub type CopycolsFn =
    unsafe fn(c: *mut c_void, row: i32, srccol: i32, dstcol: i32, ncols: i32) -> Result<(), Errno>;
/// `erasecols`: erase `ncols` cells of `row` from `startcol` with attribute `attr`.
pub type ErasecolsFn =
    unsafe fn(c: *mut c_void, row: i32, startcol: i32, ncols: i32, attr: u32) -> Result<(), Errno>;
/// `copyrows`: copy `nrows` rows from `srcrow` to `dstrow`.
pub type CopyrowsFn =
    unsafe fn(c: *mut c_void, srcrow: i32, dstrow: i32, nrows: i32) -> Result<(), Errno>;
/// `eraserows`: erase `nrows` rows from `row` with attribute `attr`.
pub type EraserowsFn =
    unsafe fn(c: *mut c_void, row: i32, nrows: i32, attr: u32) -> Result<(), Errno>;
/// `pack_attr`: the attribute for colors `fg`/`bg` and `WSATTR_*` `flags`.
pub type PackAttrFn = unsafe fn(c: *mut c_void, fg: i32, bg: i32, flags: i32) -> Result<u32, Errno>;
/// `unpack_attr`: `(fg, bg, underline)` of an attribute.
pub type UnpackAttrFn = unsafe fn(c: *mut c_void, attr: u32) -> (i32, i32, i32);

/// `struct wsdisplay_emulops`: emulation functions, for displays that can support glass-tty
/// terminal emulations. These are character oriented, with row and column numbers starting
/// at zero in the upper left hand corner of the screen.
///
/// These are used only when emulating a terminal. Therefore, displays drivers which cannot
/// emulate terminals do not have to provide them.
///
/// There is a "void *" cookie provided by the display driver associated with these
/// functions, which is passed to them when they are invoked: calling one is `unsafe`, with
/// the contract that the cookie is the one the driver paired with this table.
#[derive(Clone, Copy, Default)]
pub struct WsdisplayEmulops {
    /// `cursor`.
    pub cursor: Option<CursorFn>,
    /// `mapchar`.
    pub mapchar: Option<MapcharFn>,
    /// `putchar`.
    pub putchar: Option<PutcharFn>,
    /// `copycols`.
    pub copycols: Option<CopycolsFn>,
    /// `erasecols`.
    pub erasecols: Option<ErasecolsFn>,
    /// `copyrows`.
    pub copyrows: Option<CopyrowsFn>,
    /// `eraserows`.
    pub eraserows: Option<EraserowsFn>,
    /// `pack_attr`.
    pub pack_attr: Option<PackAttrFn>,
    /// `unpack_attr`.
    pub unpack_attr: Option<UnpackAttrFn>,
}

impl WsdisplayEmulops {
    /// A table with every operation NULL.
    pub const EMPTY: Self = Self {
        cursor: None,
        mapchar: None,
        putchar: None,
        copycols: None,
        erasecols: None,
        copyrows: None,
        eraserows: None,
        pack_attr: None,
        unpack_attr: None,
    };
}

/// `WSSCREEN_WSCOLORS`: minimal color capability.
pub const WSSCREEN_WSCOLORS: i32 = 1;
/// `WSSCREEN_REVERSE`: can display reversed.
pub const WSSCREEN_REVERSE: i32 = 2;
/// `WSSCREEN_HILIT`: can highlight (however).
pub const WSSCREEN_HILIT: i32 = 4;
/// `WSSCREEN_BLINK`: can blink.
pub const WSSCREEN_BLINK: i32 = 8;
/// `WSSCREEN_UNDERLINE`: can underline.
pub const WSSCREEN_UNDERLINE: i32 = 16;

/// `struct wsscreen_descr`: a screen type a display offers.
#[derive(Clone, Copy)]
pub struct WsscreenDescr {
    /// `name`.
    pub name: [u8; WSSCREEN_NAME_SIZE],
    /// `ncols`.
    pub ncols: i32,
    /// `nrows`.
    pub nrows: i32,
    /// `textops`: the emulops (inside the driver's `rasops_info` for raster displays).
    pub textops: *const WsdisplayEmulops,
    /// `fontwidth`.
    pub fontwidth: i32,
    /// `fontheight`.
    pub fontheight: i32,
    /// `capabilities`: `WSSCREEN_*`.
    pub capabilities: i32,
}

impl WsscreenDescr {
    /// `{ "name" }`: a descriptor with only its name set (the driver fills the rest when it
    /// knows its font and geometry). A name longer than `WSSCREEN_NAME_SIZE - 1` is cut.
    pub const fn new(name: &[u8]) -> Self {
        let mut n = [0u8; WSSCREEN_NAME_SIZE];
        let mut i = 0;
        while i < name.len() && i < WSSCREEN_NAME_SIZE - 1 {
            n[i] = name[i];
            i += 1;
        }
        Self {
            name: n,
            ncols: 0,
            nrows: 0,
            textops: ptr::null(),
            fontwidth: 0,
            fontheight: 0,
            capabilities: 0,
        }
    }

    /// The name, up to its NUL.
    pub fn name(&self) -> &[u8] {
        let len = self
            .name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(self.name.len());
        &self.name[..len]
    }
}

// SAFETY: a descriptor is plain data plus a pointer at the driver's operation table, which
// lives as long as the driver; it is written while cold and only read afterwards.
unsafe impl Send for WsscreenDescr {}

/// `struct wsdisplay_charcell`: character cell description (for emulation mode).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsdisplayCharcell {
    /// `uc`.
    pub uc: u32,
    /// `attr`.
    pub attr: u32,
}

/// `ioctl`: `Ok(true)` handled, `Ok(false)` not the driver's (the C's -1). The thread is
/// `None` where wsdisplay passes NULL (the bell task, `wsdisplay_param`).
pub type WsdisplayIoctlFn = unsafe fn(
    v: *mut c_void,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno>;
/// `mmap`: the physical address (with `PMAP_*` flags) of offset `off`, `None` for the C's -1.
pub type WsdisplayMmapFn = unsafe fn(v: *mut c_void, off: i64, prot: i32) -> Option<Paddr>;
/// `alloc_screen`: a new screen of type `type_`; its cookie, cursor and default attribute.
pub type AllocScreenFn = unsafe fn(
    v: *mut c_void,
    type_: *const WsscreenDescr,
    cookiep: &mut *mut c_void,
    curxp: &mut i32,
    curyp: &mut i32,
    attrp: &mut u32,
) -> Result<(), Errno>;
/// `free_screen`.
pub type FreeScreenFn = unsafe fn(v: *mut c_void, cookie: *mut c_void);
/// The completion callback of `show_screen`.
pub type ShowScreenCb = fn(arg: *mut c_void, error: i32, waitok: i32);
/// `show_screen`: switch to screen `cookie`; `Err(EAGAIN)` when `cb` will be called later.
pub type ShowScreenFn = unsafe fn(
    v: *mut c_void,
    cookie: *mut c_void,
    waitok: i32,
    cb: Option<ShowScreenCb>,
    cbarg: *mut c_void,
) -> Result<(), Errno>;
/// `load_font`.
pub type LoadFontFn =
    unsafe fn(v: *mut c_void, cookie: *mut c_void, font: &mut WsdisplayFont) -> Result<(), Errno>;
/// `list_font`.
pub type ListFontFn = unsafe fn(v: *mut c_void, font: &mut WsdisplayFont) -> Result<(), Errno>;
/// `scrollback`.
pub type ScrollbackFn = unsafe fn(v: *mut c_void, cookie: *mut c_void, lines: i32);
/// `getchar`: the cell at (`row`, `col`) of the active screen.
pub type GetcharFn = unsafe fn(v: *mut c_void, row: i32, col: i32) -> Option<WsdisplayCharcell>;
/// `burn_screen`.
pub type BurnScreenFn = unsafe fn(v: *mut c_void, on: u32, flags: u32);
/// `pollc`.
pub type PollcFn = unsafe fn(v: *mut c_void, on: i32);
/// `enter_ddb`.
pub type EnterDdbFn = unsafe fn(v: *mut c_void, cookie: *mut c_void);

/// `struct wsdisplay_accessops`: display access functions, invoked by user-land programs
/// which require direct device access, such as X11.
///
/// There is a "void *" cookie provided by the display driver associated with these
/// functions, which is passed to them when they are invoked: calling one is `unsafe`, with
/// the contract that the cookie is the attach arguments' `accesscookie`.
#[derive(Clone, Copy, Default)]
pub struct WsdisplayAccessops {
    /// `ioctl`.
    pub ioctl: Option<WsdisplayIoctlFn>,
    /// `mmap`.
    pub mmap: Option<WsdisplayMmapFn>,
    /// `alloc_screen`.
    pub alloc_screen: Option<AllocScreenFn>,
    /// `free_screen`.
    pub free_screen: Option<FreeScreenFn>,
    /// `show_screen`.
    pub show_screen: Option<ShowScreenFn>,
    /// `load_font`.
    pub load_font: Option<LoadFontFn>,
    /// `list_font`.
    pub list_font: Option<ListFontFn>,
    /// `scrollback`.
    pub scrollback: Option<ScrollbackFn>,
    /// `getchar`.
    pub getchar: Option<GetcharFn>,
    /// `burn_screen`.
    pub burn_screen: Option<BurnScreenFn>,
    /// `pollc`.
    pub pollc: Option<PollcFn>,
    /// `enter_ddb`.
    pub enter_ddb: Option<EnterDdbFn>,
}

impl WsdisplayAccessops {
    /// A table with every operation NULL (`.member = ...` initialisers leave the rest so).
    pub const EMPTY: Self = Self {
        ioctl: None,
        mmap: None,
        alloc_screen: None,
        free_screen: None,
        show_screen: None,
        load_font: None,
        list_font: None,
        scrollback: None,
        getchar: None,
        burn_screen: None,
        pollc: None,
        enter_ddb: None,
    };
}

/// `struct wsscreen_list`: passed to wscons by the video driver to tell about its
/// capabilities.
#[derive(Clone, Copy)]
pub struct WsscreenList {
    /// `nscreens`.
    pub nscreens: i32,
    /// `screens`: `nscreens` descriptor pointers.
    pub screens: *const *const WsscreenDescr,
}

impl WsscreenList {
    /// The descriptor pointers.
    ///
    /// # Safety
    ///
    /// `screens` points to `nscreens` pointers that stay valid for the returned lifetime (a
    /// driver's static list, or one in its softc).
    pub unsafe fn screens<'a>(&self) -> &'a [*const WsscreenDescr] {
        if self.screens.is_null() || self.nscreens <= 0 {
            return &[];
        }
        // SAFETY: the caller's contract.
        unsafe { core::slice::from_raw_parts(self.screens, self.nscreens as usize) }
    }
}

// SAFETY: a list is a count and a pointer at descriptors that live as long as the driver;
// it is written while cold and only read afterwards.
unsafe impl Send for WsscreenList {}

/// `struct wsemuldisplaydev_attach_args`: attachment information provided by
/// wsemuldisplaydev devices when attaching wsdisplay units.
#[derive(Clone, Copy)]
pub struct WsemuldisplaydevAttachArgs {
    /// `console`: is it console?
    pub console: i32,
    /// `primary`: is it primary?
    pub primary: i32,
    /// `scrdata`: screen cfg info.
    pub scrdata: *const WsscreenList,
    /// `accessops`: access ops.
    pub accessops: &'static WsdisplayAccessops,
    /// `accesscookie`: access cookie.
    pub accesscookie: *mut c_void,
    /// `defaultscreens`: screens to create.
    pub defaultscreens: u32,
}

/// `WSEMULDISPLAYDEVCF_CONSOLE`: the index of the `console` locator.
pub const WSEMULDISPLAYDEVCF_CONSOLE: usize = 0;
/// `WSEMULDISPLAYDEVCF_CONSOLE_UNK`.
pub const WSEMULDISPLAYDEVCF_CONSOLE_UNK: i64 = -1;
/// `WSEMULDISPLAYDEVCF_PRIMARY`: the index of the `primary` locator.
pub const WSEMULDISPLAYDEVCF_PRIMARY: usize = 1;
/// `WSEMULDISPLAYDEVCF_PRIMARY_UNK`.
pub const WSEMULDISPLAYDEVCF_PRIMARY_UNK: i64 = -1;
/// `WSEMULDISPLAYDEVCF_MUX`: the index of the `mux` locator.
pub const WSEMULDISPLAYDEVCF_MUX: usize = 2;
/// `WSDISPLAYDEVCF_MUX`: the index of `wsdisplaydev`'s `mux` locator.
pub const WSDISPLAYDEVCF_MUX: usize = 0;

/// `wsemuldisplaydevcf_console`: spec'd as console?
pub fn wsemuldisplaydevcf_console(cf: &Cfdata) -> i64 {
    cf.cf_loc
        .get(WSEMULDISPLAYDEVCF_CONSOLE)
        .copied()
        .unwrap_or(WSEMULDISPLAYDEVCF_CONSOLE_UNK)
}

/// `wsemuldisplaydevcf_primary`: spec'd as primary?
pub fn wsemuldisplaydevcf_primary(cf: &Cfdata) -> i64 {
    cf.cf_loc
        .get(WSEMULDISPLAYDEVCF_PRIMARY)
        .copied()
        .unwrap_or(WSEMULDISPLAYDEVCF_PRIMARY_UNK)
}

/// `wsemuldisplaydevcf_mux`.
pub fn wsemuldisplaydevcf_mux(cf: &Cfdata) -> i64 {
    cf.cf_loc.get(WSEMULDISPLAYDEVCF_MUX).copied().unwrap_or(1)
}

/// `wsdisplaydevcf_mux`.
pub fn wsdisplaydevcf_mux(cf: &Cfdata) -> i64 {
    cf.cf_loc.get(WSDISPLAYDEVCF_MUX).copied().unwrap_or(1)
}

/// `struct wscons_syncops`: what a compatibility layer (`wsdisplay_compat_usl`) hooks into
/// screen switching.
#[derive(Clone, Copy)]
pub struct WsconsSyncops {
    /// `detach`.
    pub detach: unsafe fn(*mut c_void, i32, Option<ShowScreenCb>, *mut c_void) -> i32,
    /// `attach`.
    pub attach: unsafe fn(*mut c_void, i32, Option<ShowScreenCb>, *mut c_void) -> i32,
    /// `check`.
    pub check: unsafe fn(*mut c_void) -> i32,
    /// `destroy`.
    pub destroy: unsafe fn(*mut c_void),
}

/// `WSDISPLAY_NULLSCREEN`.
pub const WSDISPLAY_NULLSCREEN: i32 = -1;

/// `int (*)(struct wsdisplay_param *)`: the type of `ws_get_param`/`ws_set_param`, 0 or an
/// errno, -1 when the parameter is not the hook's.
pub type WsParamFn = fn(&mut WsdisplayParam) -> i32;

/// `ws_get_param`: the hook a backlight or brightness driver installs, read by the display
/// drivers' `WSDISPLAYIO_GETPARAM` (defined by `wsdisplay.c`).
pub static WS_GET_PARAM: StaticCell<Option<WsParamFn>> = StaticCell::new(None);
/// `ws_set_param`: as [`WS_GET_PARAM`], for `WSDISPLAYIO_SETPARAM`.
pub static WS_SET_PARAM: StaticCell<Option<WsParamFn>> = StaticCell::new(None);

/// `ws_get_param`, read: written while cold by the driver that provides it, read after.
pub fn ws_get_param() -> Option<WsParamFn> {
    // SAFETY: written only while cold, before any display ioctl can run (see the static).
    unsafe { WS_GET_PARAM.read() }
}

/// `ws_set_param`, read: as [`ws_get_param`].
pub fn ws_set_param() -> Option<WsParamFn> {
    // SAFETY: as in `ws_get_param`.
    unsafe { WS_SET_PARAM.read() }
}

/// `WSDISPLAY_SCROLL_BACKWARD`: for use by wskbd.
pub const WSDISPLAY_SCROLL_BACKWARD: i32 = 0;
/// `WSDISPLAY_SCROLL_FORWARD`.
pub const WSDISPLAY_SCROLL_FORWARD: i32 = 1;
/// `WSDISPLAY_SCROLL_RESET`.
pub const WSDISPLAY_SCROLL_RESET: i32 = 2;

/// `WSDISPLAY_DEFBURNOUT_MSEC`: screen burner, disabled.
pub const WSDISPLAY_DEFBURNOUT_MSEC: i32 = 0;
/// `WSDISPLAY_DEFBURNIN_MSEC`: milliseconds.
pub const WSDISPLAY_DEFBURNIN_MSEC: i32 = 250;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_descriptor_names() {
        let d = WsscreenDescr::new(b"std");
        assert_eq!(d.name(), b"std");
        let long = WsscreenDescr::new(b"a-very-long-screen-name");
        assert_eq!(long.name().len(), WSSCREEN_NAME_SIZE - 1);
        assert!(d.textops.is_null());
    }

    /// Every constant against `<dev/wscons/wsdisplayvar.h>`.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/wscons/wsdisplayvar.h");
        crate::reftest::assert_defines!(defs;
            WSDISPLAY_MAXSCREEN,
            WSCOL_BLACK,
            WSCOL_RED,
            WSCOL_GREEN,
            WSCOL_BROWN,
            WSCOL_BLUE,
            WSCOL_MAGENTA,
            WSCOL_CYAN,
            WSCOL_WHITE,
            WSATTR_REVERSE,
            WSATTR_HILIT,
            WSATTR_BLINK,
            WSATTR_UNDERLINE,
            WSATTR_WSCOLORS,
            WSSCREEN_WSCOLORS,
            WSSCREEN_REVERSE,
            WSSCREEN_HILIT,
            WSSCREEN_BLINK,
            WSSCREEN_UNDERLINE,
            WSDISPLAY_NULLSCREEN,
            WSDISPLAY_SCROLL_BACKWARD,
            WSDISPLAY_SCROLL_FORWARD,
            WSDISPLAY_SCROLL_RESET,
            WSDISPLAY_DEFBURNOUT_MSEC,
            WSDISPLAY_DEFBURNIN_MSEC,
        );
    }
}
/* </TESTS> */
