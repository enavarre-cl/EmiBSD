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
//! The raster font list: `dev/wsfont/wsfont.c` and `<dev/wsfont/wsfont.h>`, `wsfont(9)`.
//!
//! Upstream: sys/dev/wsfont/wsfont.c @ 3ce1f3f79392
//! Upstream: sys/dev/wsfont/wsfont.h @ 3ce1f3f79392
//!
//! Fonts usable on raster frame buffers are kept in one list, the built-in ones first.
//! `wsfont_find()` can be called with any of the parameters as 0 (or `None`), meaning we
//! don't care about that aspect of the font. It returns a cookie which we can use with the
//! other functions. When more flexibility is required, `wsfont_enum()` should be used. The
//! last two parameters to `wsfont_lock()` are the bit order and byte order required
//! (`WSDISPLAY_FONTORDER_L2R` or `WSDISPLAY_FONTORDER_R2L`):
//!
//! ```text
//! let cookie = wsfont_find(None, 8, 16, 0).ok_or(...)?;     // "unable to get 8x16 font"
//! let (_, font) = wsfont_lock(cookie, WSDISPLAY_FONTORDER_L2R,
//!     WSDISPLAY_FONTORDER_R2L)?;                              // "unable to lock font"
//! ... do stuff ...
//! wsfont_unlock(cookie);
//! ```
//!
//! The built-in fonts are what the C picks without `FONT_*` options on amd64 and arm64
//! (neither `SMALL_KERNEL`): Spleen 8x16, 12x24, 16x32 and 32x64, with cookies 4, 6, 7 and 8.
//! This kernel is not `SMALL_KERNEL`, so the bit-reversal, byte-reversal and Unicode-to-IBM
//! code is in.
//!
//! ## Deviations
//! - The `FONT_*` options are not offered: GENERIC sets none, so the font list is the default
//!   set above. The other fonts (`gallant12x22`, `spleen5x8`, `spleen6x12`,
//!   `spleen8x16-ibm`) are `todo` in `ports.toml`; their entries of `builtin_fonts` come
//!   with them.
//! - The rotation code (`NRASOPS_ROTATION > 0`: `wsfont_rotate` and its helpers) is always
//!   built. amd64's GENERIC has it through `inteldrm`; arm64's does not, but a font is only
//!   rotated when a driver asks for a rotated display, so the code is inert there.
//! - The list is protected at `splhigh`, as in C (the kernel lock serialises the rest); the
//!   statics are wrappers with `unsafe impl Sync` stating that, as `kern_sensors.rs` does.
//!   An entry's `font` is a raw pointer: a built-in font is a `StaticCell` in its own module,
//!   a loaded one malloc'd memory; the cookie/lock-count discipline below is the C's.
//! - Failures: `wsfont_find`, `wsfont_map_unichar` and `wsfont_rotate` return `None` for the
//!   C's -1 (nothing found); `wsfont_add` returns `EEXIST` for a duplicate and `ENOSPC` for
//!   a full list (both -1 in C); `wsfont_lock` returns `ENOENT` for an unknown cookie and
//!   `EBUSY` when a locked font would need its order changed (-1 in C) and otherwise the new
//!   lock count with the font; `wsfont_unlock` returns `ENOENT` for an unknown cookie.
//! - `wsfont_enum` takes a closure (returning `true` to stop) instead of a callback and its
//!   `void *` argument.
//! - The glyph maps' `void *chars` with its element `width` is an enum of slices
//!   ([`GlyphChars`]).

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};

use crate::dev::wscons::wsconsio::{
    WSDISPLAY_FONTENC_IBM, WSDISPLAY_FONTENC_ISO, WSDISPLAY_MAXFONTCOUNT, WsdisplayFont,
};
use crate::dev::wsfont::spleen8x16::SPLEEN8X16;
use crate::dev::wsfont::spleen12x24::SPLEEN12X24;
use crate::dev::wsfont::spleen16x32::SPLEEN16X32;
use crate::dev::wsfont::spleen32x64::SPLEEN32X64;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::subr_prf::panic;
use crate::machine::intr::{splhigh, splx};
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::queue::{TailqEntry, TailqHead};

/// `WSFONT_BUILTIN`: for `wsfont_add()`.
pub const WSFONT_BUILTIN: u16 = 0x01;
/// `WSFONT_STATIC`.
pub const WSFONT_STATIC: u16 = 0x02;

/// `struct font`: an entry of the font list.
pub struct Font {
    /// `chain`.
    chain: TailqEntry<Font>,
    /// `font`.
    font: Cell<*mut WsdisplayFont>,
    /// `lockcount`.
    lockcount: Cell<u16>,
    /// `cookie`.
    cookie: Cell<u16>,
    /// `flg`: `WSFONT_*`.
    #[allow(dead_code)] // the C sets it and never reads it
    flg: Cell<u16>,
}

impl Font {
    /// `BUILTIN_FONT(f, c)`.
    const fn builtin(font: *mut WsdisplayFont, cookie: u16) -> Self {
        Self {
            chain: TailqEntry::new(),
            font: Cell::new(font),
            lockcount: Cell::new(0),
            cookie: Cell::new(cookie),
            flg: Cell::new(WSFONT_STATIC | WSFONT_BUILTIN),
        }
    }
}

queue_adapter!(FontChain: Font, chain => TailqEntry<Font>);

/// The `fontlist` head.
pub struct Fontlist(TailqHead<FontChain>);

// SAFETY: the list and its entries change at `splhigh` (every function below raises to it
// around them), under the kernel lock, as in C.
unsafe impl Sync for Fontlist {}

/// The `builtin_fonts` array.
pub struct BuiltinFonts([Font; 4]);

// SAFETY: as for `Fontlist`: the entries are only touched at `splhigh`.
unsafe impl Sync for BuiltinFonts {}

/// One way a level-2 glyph map holds its glyph numbers (`chars` with its `width`).
#[derive(Clone, Copy)]
pub enum GlyphChars {
    /// 1-byte entries.
    W1(&'static [u8]),
    /// 2-byte entries.
    W2(&'static [u16]),
    /// 4-byte entries.
    W4(&'static [u32]),
}

/// `struct wsfont_level1_glyphmap`: the high byte of a Unicode character picks a level-2
/// table.
pub struct WsfontLevel1Glyphmap {
    /// `level2`.
    level2: &'static [Option<&'static WsfontLevel2Glyphmap>],
    /// `base`: high byte for first level2 entry.
    base: i32,
    /// `size`: number of level2 entries.
    size: i32,
}

/// `struct wsfont_level2_glyphmap`: the low byte picks the glyph.
pub struct WsfontLevel2Glyphmap {
    /// `base`: low byte for first character.
    base: i32,
    /// `size`: number of characters.
    size: i32,
    /// `chars` and `width`: the character number entries.
    chars: GlyphChars,
}

/// `fontlist`.
static FONTLIST: Fontlist = Fontlist(TailqHead::new());

/// `builtin_fonts`: our list of built-in fonts.
static BUILTIN_FONTS: BuiltinFonts = BuiltinFonts([
    Font::builtin(SPLEEN8X16.as_ptr(), 4),
    Font::builtin(SPLEEN12X24.as_ptr(), 6),
    Font::builtin(SPLEEN16X32.as_ptr(), 7),
    Font::builtin(SPLEEN32X64.as_ptr(), 8),
]);

/// `reverse`: reverse the bit order in a byte.
static REVERSE: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = (i as u8).reverse_bits();
        i += 1;
    }
    t
};

/// `again` of `wsfont_init`.
static WSFONT_INIT_AGAIN: AtomicBool = AtomicBool::new(false);

/// `cookiegen` of `wsfont_add`.
static COOKIEGEN: AtomicU16 = AtomicU16::new(666);

/*
 * Unicode to font encoding mappings
 *
 * To save memory, font encoding tables use a two level lookup. First the high byte of the
 * Unicode is used to lookup the level 2 table, then the low byte indexes that table. Level 2
 * tables that are not needed are omitted (NULL), and both level 1 and level 2 tables have
 * base and size attributes to keep their size down.
 */

/// `ibm437_chars_0`: the IBM 437 maps.
#[rustfmt::skip]
static IBM437_CHARS_0: [u8; 256] = [
     0,  1,  2,  3,  4,  5,  6,  7,  8,  9, 10, 11, 12, 13, 14, 15,
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
    32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47,
    48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63,
    64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79,
    80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95,
    96, 97, 98, 99, 100,101,102,103,104,105,106,107,108,109,110,111,
    112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127,
     0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
     0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
    255,173,155,156, 0, 157, 0,  0,  0,  0, 166,174,170, 0,  0,  0,
     0, 241,253, 0,  0,  0,  0, 249, 0,  0, 167,175,172,171, 0, 168,
     0,  0,  0,  0, 142,143,146,128, 0, 144, 0,  0,  0,  0,  0,  0,
     0, 165, 0,  0,  0,  0, 153, 0,  0,  0,  0,  0, 154, 0,  0,  0,
    133,160,131, 0, 132,134,145,135,138,130,136,137,141,161,140,139,
     0, 164,149,162,147, 0, 148,246, 0, 151,163,150,129, 0,  0, 152,
];
/// `ibm437_chars_1`.
static IBM437_CHARS_1: [u8; 1] = [159];
/// `ibm437_chars_3`.
#[rustfmt::skip]
static IBM437_CHARS_3: [u8; 50] = [
    226, 0,  0,  0,  0, 233, 0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
    228, 0,  0, 232, 0,  0, 234, 0,  0,  0,  0,  0,  0,  0, 224,225,
     0, 235,238, 0,  0,  0,  0,  0,  0, 230, 0,  0,  0, 227, 0,  0,
    229,231,
];
/// `ibm437_chars_32`.
#[rustfmt::skip]
static IBM437_CHARS_32: [u8; 41] = [
    252, 0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
     0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
     0,  0,  0,  0,  0,  0,  0,  0, 158,
];
/// `ibm437_chars_34`.
#[rustfmt::skip]
static IBM437_CHARS_34: [u8; 97] = [
    237, 0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
     0,  0,  0, 248,250,251, 0,  0,  0, 236, 0,  0,  0,  0,  0,  0,
     0,  0,  0,  0, 239, 0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
     0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
     0,  0,  0, 247, 0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
     0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,240,  0,  0,243,
    242,
];
/// `ibm437_chars_35`.
#[rustfmt::skip]
static IBM437_CHARS_35: [u8; 18] = [
    169, 0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
    244,245,
];
/// `ibm437_chars_37`.
#[rustfmt::skip]
static IBM437_CHARS_37: [u8; 161] = [
    196,205,179,186, 0,  0,  0,  0,  0,  0,  0,  0, 218,213,214,201,
    191,184,183,187,192,212,211,200,217,190,189,188,195,198, 0,  0,
    199, 0,  0, 204,180,181, 0,  0, 182, 0,  0, 185,194, 0,  0, 209,
    210, 0,  0, 203,193, 0,  0, 207,208, 0,  0, 202,197, 0,  0, 216,
     0,  0, 215, 0,  0,  0,  0,  0,  0,  0,  0, 206, 0,  0,  0,  0,
     0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
     0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
     0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
    223, 0,  0,  0, 220, 0,  0,  0, 219, 0,  0,  0, 221, 0,  0,  0,
    222,176,177,178, 0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,
    254,
];

/// `ibm437_level2_0`.
static IBM437_LEVEL2_0: WsfontLevel2Glyphmap = WsfontLevel2Glyphmap {
    base: 0,
    size: 256,
    chars: GlyphChars::W1(&IBM437_CHARS_0),
};
/// `ibm437_level2_1`.
static IBM437_LEVEL2_1: WsfontLevel2Glyphmap = WsfontLevel2Glyphmap {
    base: 146,
    size: 1,
    chars: GlyphChars::W1(&IBM437_CHARS_1),
};
/// `ibm437_level2_3`.
static IBM437_LEVEL2_3: WsfontLevel2Glyphmap = WsfontLevel2Glyphmap {
    base: 147,
    size: 50,
    chars: GlyphChars::W1(&IBM437_CHARS_3),
};
/// `ibm437_level2_32`.
static IBM437_LEVEL2_32: WsfontLevel2Glyphmap = WsfontLevel2Glyphmap {
    base: 127,
    size: 41,
    chars: GlyphChars::W1(&IBM437_CHARS_32),
};
/// `ibm437_level2_34`.
static IBM437_LEVEL2_34: WsfontLevel2Glyphmap = WsfontLevel2Glyphmap {
    base: 5,
    size: 97,
    chars: GlyphChars::W1(&IBM437_CHARS_34),
};
/// `ibm437_level2_35`.
static IBM437_LEVEL2_35: WsfontLevel2Glyphmap = WsfontLevel2Glyphmap {
    base: 16,
    size: 18,
    chars: GlyphChars::W1(&IBM437_CHARS_35),
};
/// `ibm437_level2_37`.
static IBM437_LEVEL2_37: WsfontLevel2Glyphmap = WsfontLevel2Glyphmap {
    base: 0,
    size: 161,
    chars: GlyphChars::W1(&IBM437_CHARS_37),
};

/// `ibm437_level1`.
static IBM437_LEVEL1: [Option<&WsfontLevel2Glyphmap>; 38] = [
    Some(&IBM437_LEVEL2_0),
    Some(&IBM437_LEVEL2_1),
    None,
    Some(&IBM437_LEVEL2_3),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    Some(&IBM437_LEVEL2_32),
    None,
    Some(&IBM437_LEVEL2_34),
    Some(&IBM437_LEVEL2_35),
    None,
    Some(&IBM437_LEVEL2_37),
];

/// `encodings`: indexed by `WSDISPLAY_FONTENC_*`.
static ENCODINGS: [WsfontLevel1Glyphmap; 2] = [
    // WSDISPLAY_FONTENC_ISO
    WsfontLevel1Glyphmap {
        level2: &[],
        base: 0,
        size: 0,
    },
    // WSDISPLAY_FONTENC_IBM
    WsfontLevel1Glyphmap {
        level2: &IBM437_LEVEL1,
        base: 0,
        size: IBM437_LEVEL1.len() as i32,
    },
];

/// `wsfont_revbit`: reverse the bit order of a font.
///
/// # Safety
///
/// `font` is valid, unlocked, and its `data` holds its glyphs.
unsafe fn wsfont_revbit(font: *mut WsdisplayFont) {
    // SAFETY: the caller's contract.
    let font = unsafe { &mut *font };
    let len = font.stride as usize * font.numchars as usize * font.fontheight as usize;
    // SAFETY: the glyphs are `len` bytes; nobody holds the font locked, so nobody reads them.
    let p = unsafe { core::slice::from_raw_parts_mut(font.data.cast::<u8>(), len) };

    for b in p {
        *b = REVERSE[*b as usize];
    }
}

/// `wsfont_revbyte`: reverse the byte order of a font.
///
/// # Safety
///
/// As for [`wsfont_revbit`].
unsafe fn wsfont_revbyte(font: *mut WsdisplayFont) {
    // SAFETY: the caller's contract.
    let font = unsafe { &mut *font };
    if font.stride == 1 {
        return;
    }

    let stride = font.stride as usize;
    let nr = font.numchars as usize * font.fontheight as usize;
    // SAFETY: as in `wsfont_revbit`.
    let rp = unsafe { core::slice::from_raw_parts_mut(font.data.cast::<u8>(), nr * stride) };

    for row in rp.chunks_exact_mut(stride) {
        row.reverse();
    }
}

/// `wsfont_enum`: enumerate the list of fonts; `cb` returns `true` to stop.
pub fn wsfont_enum(mut cb: impl FnMut(&WsdisplayFont) -> bool) {
    let s = splhigh();

    for ent in FONTLIST.0.iter() {
        // SAFETY: a listed font stays valid while listed; at splhigh nobody changes it.
        if cb(unsafe { &*ent.font.get() }) {
            break;
        }
    }

    splx(s);
}

/// `wsfont_rotate_cw`: rotate the font a bit at a time.
fn wsfont_rotate_cw(font: &WsdisplayFont, glyphs: &[u8], newbits: &mut [u8], newstride: usize) {
    let stride = font.stride as usize;
    let (fw, fh) = (font.fontwidth as usize, font.fontheight as usize);

    for n in 0..font.numchars as usize {
        let ch = &glyphs[n * stride * fh..];

        for r in 0..fh {
            for b in 0..fw {
                let rb = ch[stride * r + b / 8];
                if rb & (0x80 >> (b % 8)) != 0 {
                    let rrb = newstride - 1 - r / 8 + n * newstride * fw + newstride * b;
                    newbits[rrb] |= 1 << (r % 8);
                }
            }
        }
    }
}

/// `wsfont_rotate_ccw`: rotate the font a bit at a time.
fn wsfont_rotate_ccw(font: &WsdisplayFont, glyphs: &[u8], newbits: &mut [u8], newstride: usize) {
    let stride = font.stride as usize;
    let (fw, fh) = (font.fontwidth as usize, font.fontheight as usize);

    for n in 0..font.numchars as usize {
        let ch = &glyphs[n * stride * fh..];

        for r in 0..fh {
            for b in 0..fw {
                let bb = fw - 1 - b;
                let rb = ch[stride * r + b / 8];
                if rb & (0x80 >> (b % 8)) != 0 {
                    let rrb = r / 8 + n * newstride * fw + newstride * bb;
                    newbits[rrb] |= 1 << (7 - (r % 8));
                }
            }
        }
    }
}

/// `wsfont_rotate_internal`: a rotated copy of `font`, added to the list; `None` if the list
/// already has it.
///
/// # Safety
///
/// `font` is a listed font whose glyphs are valid.
unsafe fn wsfont_rotate_internal(
    font: *mut WsdisplayFont,
    ccw: bool,
) -> Option<*mut WsdisplayFont> {
    // SAFETY: the caller's contract.
    let font = unsafe { &*font };

    // Duplicate the existing font...
    let mem = malloc(size_of::<WsdisplayFont>(), M_DEVBUF, M_WAITOK)?;
    let newfont = mem.cast::<WsdisplayFont>().as_ptr();
    let mut copy = *font;
    copy.cookie = ptr::null_mut();

    // Allocate a buffer big enough for the rotated font.
    let newstride = (font.fontheight as usize).div_ceil(8);
    let nbytes = font.numchars as usize * newstride * font.fontwidth as usize;
    let newbits = mallocarray(
        font.numchars as usize,
        newstride * font.fontwidth as usize,
        M_DEVBUF,
        M_WAITOK | M_ZERO,
    )?;
    // SAFETY: a fresh, zeroed allocation of `nbytes` bytes, ours alone.
    let bits = unsafe { core::slice::from_raw_parts_mut(newbits.as_ptr(), nbytes) };
    let glen = font.stride as usize * font.numchars as usize * font.fontheight as usize;
    // SAFETY: a listed font's glyphs are `glen` bytes (the caller's contract).
    let glyphs = unsafe { core::slice::from_raw_parts(font.data.cast::<u8>(), glen) };

    if ccw {
        wsfont_rotate_ccw(font, glyphs, bits, newstride);
    } else {
        wsfont_rotate_cw(font, glyphs, bits, newstride);
    }

    copy.data = newbits.as_ptr().cast();

    // Update font sizes.
    copy.stride = newstride as u32;
    copy.fontwidth = font.fontheight;
    copy.fontheight = font.fontwidth;
    // SAFETY: a fresh allocation of one font, malloc(9)-aligned.
    unsafe { ptr::write(newfont, copy) };

    // SAFETY: the new font and its glyphs are kept for good once listed.
    if unsafe { wsfont_add(newfont, false) }.is_err() {
        // If we seem to have rotated this font already, drop the new one...
        free(newbits, M_DEVBUF, nbytes);
        free(mem, M_DEVBUF, size_of::<WsdisplayFont>());
        return None;
    }

    Some(newfont)
}

/// `wsfont_rotate`: the cookie of a rotated copy of font `cookie` (made and listed now).
pub fn wsfont_rotate(cookie: i32, ccw: bool) -> Option<i32> {
    let s = splhigh();
    let origfont = wsfont_find0(cookie);
    splx(s);

    let origfont = origfont?;
    // SAFETY: a listed font with its glyphs.
    let font = unsafe { wsfont_rotate_internal(origfont.font.get(), ccw) }?;

    // SAFETY: the font was just listed and stays so.
    let font = unsafe { &*font };
    wsfont_find(
        Some(font.name()),
        font.fontwidth as i32,
        font.fontheight as i32,
        font.stride as i32,
    )
}

/// `wsfont_init`: initialize list with `WSFONT_BUILTIN` fonts.
pub fn wsfont_init() {
    if WSFONT_INIT_AGAIN.swap(true, Ordering::Relaxed) {
        return;
    }

    FONTLIST.0.init();

    for ent in &BUILTIN_FONTS.0 {
        // SAFETY: the built-in entries are statics, linked once, never unlinked.
        unsafe { FONTLIST.0.insert_tail(ent) };
    }
}

/// `wsfont_find0`: find a font by cookie. Called at splhigh.
fn wsfont_find0(cookie: i32) -> Option<&'static Font> {
    FONTLIST
        .0
        .iter()
        .find(|ent| i32::from(ent.cookie.get()) == cookie)
}

/// `wsfont_find`: find a font; 0 (or `None`) matches anything.
pub fn wsfont_find(name: Option<&[u8]>, width: i32, height: i32, stride: i32) -> Option<i32> {
    let s = splhigh();

    for ent in FONTLIST.0.iter() {
        // SAFETY: a listed font stays valid while listed; read at splhigh.
        let font = unsafe { &*ent.font.get() };

        if height != 0 && font.fontheight as i32 != height {
            continue;
        }

        if width != 0 && font.fontwidth as i32 != width {
            continue;
        }

        if stride != 0 && font.stride as i32 != stride {
            continue;
        }

        if let Some(name) = name
            && font.name() != name
        {
            continue;
        }

        splx(s);
        return Some(i32::from(ent.cookie.get()));
    }

    splx(s);
    None
}

/// `wsfont_add`: add a font to the list. With `copy` (a `WSDISPLAYIO_LDFONT`), the entry
/// gets a copy of the `wsdisplay_font` structure, but not of its glyphs.
///
/// # Safety
///
/// `font` is valid; its glyphs (and, without `copy`, the structure itself) stay valid for
/// as long as the font is listed, which is for good.
pub unsafe fn wsfont_add(font: *mut WsdisplayFont, copy: bool) -> Result<(), Errno> {
    let s = splhigh();
    // SAFETY: the caller's contract.
    let f = unsafe { &*font };

    // Don't allow exact duplicates
    if wsfont_find(
        Some(f.name()),
        f.fontwidth as i32,
        f.fontheight as i32,
        f.stride as i32,
    )
    .is_some()
    {
        splx(s);
        return Err(Errno::EEXIST);
    }

    let fontc = FONTLIST.0.iter().count() as i32;

    if fontc >= WSDISPLAY_MAXFONTCOUNT {
        splx(s);
        return Err(Errno::ENOSPC);
    }

    let Some(mem) = malloc(size_of::<Font>(), M_DEVBUF, M_WAITOK) else {
        splx(s);
        return Err(Errno::ENOMEM);
    };

    let cookie = COOKIEGEN.fetch_add(1, Ordering::Relaxed);

    // If we are coming from a WSDISPLAYIO_LDFONT ioctl, we need to make a copy of the
    // wsdisplay_font struct, but not of font->bits.
    let (fontp, flg) = if copy {
        let Some(fm) = malloc(size_of::<WsdisplayFont>(), M_DEVBUF, M_WAITOK) else {
            free(mem, M_DEVBUF, size_of::<Font>());
            splx(s);
            return Err(Errno::ENOMEM);
        };
        let fp = fm.cast::<WsdisplayFont>().as_ptr();
        // SAFETY: a fresh allocation of one font, malloc(9)-aligned.
        unsafe { ptr::write(fp, *f) };
        (fp, 0)
    } else {
        (font, WSFONT_STATIC)
    };

    let ent = mem.cast::<Font>().as_ptr();
    // SAFETY: a fresh allocation of one entry, malloc(9)-aligned.
    unsafe {
        ptr::write(
            ent,
            Font {
                chain: TailqEntry::new(),
                font: Cell::new(fontp),
                lockcount: Cell::new(0),
                cookie: Cell::new(cookie),
                flg: Cell::new(flg),
            },
        )
    };

    // Now link into the list and return
    // SAFETY: the entry is never freed or unlinked (fonts are not removed).
    unsafe { FONTLIST.0.insert_tail(&*ent) };
    splx(s);
    Ok(())
}

/// `wsfont_lock`: lock a given font and return the new lock count with the font. This fails
/// if the cookie is invalid, or if the font is already locked and the bit/byte order
/// requested by the caller differs.
pub fn wsfont_lock(
    cookie: i32,
    bitorder: i32,
    byteorder: i32,
) -> Result<(i32, NonNull<WsdisplayFont>), Errno> {
    let s = splhigh();

    let Some(ent) = wsfont_find0(cookie) else {
        splx(s);
        return Err(Errno::ENOENT);
    };
    let font = ent.font.get();

    // SAFETY: a listed font stays valid; read and changed at splhigh, and changed only
    // while nobody holds it locked (checked just before).
    unsafe {
        if bitorder != 0 && bitorder != (*font).bitorder {
            if ent.lockcount.get() != 0 {
                splx(s);
                return Err(Errno::EBUSY);
            }
            wsfont_revbit(font);
            (*font).bitorder = bitorder;
        }

        if byteorder != 0 && byteorder != (*font).byteorder {
            if ent.lockcount.get() != 0 {
                splx(s);
                return Err(Errno::EBUSY);
            }
            wsfont_revbyte(font);
            (*font).byteorder = byteorder;
        }
    }

    let lc = ent.lockcount.get() + 1;
    ent.lockcount.set(lc);
    splx(s);
    NonNull::new(font)
        .map(|f| (i32::from(lc), f))
        .ok_or(Errno::ENOENT)
}

/// `wsfont_unlock`: unlock a given font and return the new lock count.
pub fn wsfont_unlock(cookie: i32) -> Result<i32, Errno> {
    let s = splhigh();

    let Some(ent) = wsfont_find0(cookie) else {
        splx(s);
        return Err(Errno::ENOENT);
    };
    if ent.lockcount.get() == 0 {
        panic(format_args!("wsfont_unlock: font not locked"));
    }
    let lc = ent.lockcount.get() - 1;
    ent.lockcount.set(lc);

    splx(s);
    Ok(i32::from(lc))
}

/// `wsfont_map_unichar`: remap Unicode character to glyph; `None` when the font has none.
pub fn wsfont_map_unichar(font: &WsdisplayFont, c: i32) -> Option<i32> {
    if font.encoding == WSDISPLAY_FONTENC_ISO {
        return Some(c);
    }

    if font.encoding >= 0 && (font.encoding as usize) < ENCODINGS.len() {
        let mut hi = c >> 8;
        let mut lo = c & 255;
        let map1 = &ENCODINGS[font.encoding as usize];

        hi -= map1.base;

        if hi >= 0
            && hi < map1.size
            && let Some(map2) = map1.level2[hi as usize]
        {
            lo -= map2.base;

            if lo >= 0 && lo < map2.size {
                let lo = lo as usize;
                let c = match map2.chars {
                    GlyphChars::W1(t) => i32::from(t[lo]),
                    GlyphChars::W2(t) => i32::from(t[lo]),
                    GlyphChars::W4(t) => t[lo] as i32,
                };

                if c != 0 || lo == 0 {
                    return Some(c);
                }
            }
        }
    }

    None
}

const _: () = assert!(WSDISPLAY_FONTENC_IBM as usize == 1);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    use crate::dev::wscons::wsconsio::WSDISPLAY_FONTORDER_L2R;

    #[test]
    fn builtin_fonts_are_found() {
        wsfont_init();
        wsfont_init(); // idempotent

        assert_eq!(wsfont_find(None, 8, 16, 0), Some(4));
        assert_eq!(wsfont_find(None, 12, 0, 0), Some(6));
        assert_eq!(wsfont_find(None, 16, 0, 0), Some(7));
        assert_eq!(wsfont_find(None, 32, 0, 0), Some(8));
        assert_eq!(
            wsfont_find(None, 0, 0, 0),
            Some(4),
            "the first font of the list"
        );
        assert_eq!(wsfont_find(Some(b"Spleen 12x24"), 0, 0, 2), Some(6));
        assert_eq!(wsfont_find(Some(b"Spleen 12x24"), 8, 0, 0), None);
        assert_eq!(wsfont_find(None, 5, 8, 0), None);

        let mut names = std::vec::Vec::new();
        wsfont_enum(|f| {
            names.push(f.name().to_vec());
            false
        });
        assert!(names.starts_with(&[b"Spleen 8x16".to_vec(), b"Spleen 12x24".to_vec()]));
    }

    #[test]
    fn lock_and_unlock() {
        wsfont_init();

        let (lc, font) = wsfont_lock(4, WSDISPLAY_FONTORDER_L2R, WSDISPLAY_FONTORDER_L2R)
            .expect("spleen 8x16 locks");
        assert!(lc >= 1);
        // SAFETY: a locked built-in font.
        let font = unsafe { font.as_ref() };
        assert_eq!((font.fontwidth, font.fontheight, font.stride), (8, 16, 1));
        assert_eq!(font.firstchar, 32);
        assert_eq!(font.numchars, 224);
        // 'A' is glyph 33; its fourth row has its two top pixels in the middle.
        // SAFETY: the glyphs are numchars * fontheight bytes.
        let glyphs = unsafe { core::slice::from_raw_parts(font.data.cast::<u8>(), 224 * 16) };
        assert_ne!(glyphs[33 * 16..34 * 16].iter().fold(0, |a, &b| a | b), 0);
        assert_eq!(glyphs[..16], [0; 16], "the space is blank");

        assert_eq!(wsfont_unlock(4), Ok(lc - 1));
        assert_eq!(wsfont_lock(99, 0, 0).err(), Some(Errno::ENOENT));
        assert_eq!(wsfont_unlock(99), Err(Errno::ENOENT));
    }

    #[test]
    fn unicode_maps() {
        let mut font = WsdisplayFont::zeroed();
        font.encoding = WSDISPLAY_FONTENC_ISO;
        assert_eq!(wsfont_map_unichar(&font, 0x263a), Some(0x263a));

        font.encoding = WSDISPLAY_FONTENC_IBM;
        assert_eq!(wsfont_map_unichar(&font, 'A' as i32), Some(65));
        assert_eq!(wsfont_map_unichar(&font, 0xe9), Some(130)); // e acute
        assert_eq!(wsfont_map_unichar(&font, 0x2500), Some(196)); // box drawing
        assert_eq!(wsfont_map_unichar(&font, 0x3a3), Some(228)); // capital sigma
        assert_eq!(wsfont_map_unichar(&font, 0), Some(0), "lo == 0 maps to 0");
        assert_eq!(wsfont_map_unichar(&font, 0x80), None);
        assert_eq!(wsfont_map_unichar(&font, 0x263a), None);

        font.encoding = 7;
        assert_eq!(wsfont_map_unichar(&font, 'A' as i32), None);
    }

    #[test]
    fn rotation_turns_rows_into_columns() {
        let mut font = WsdisplayFont::zeroed();
        font.numchars = 1;
        font.fontwidth = 8;
        font.fontheight = 2;
        font.stride = 1;
        // Row 0 has the leftmost pixel set, row 1 the rightmost.
        let glyphs = [0x80u8, 0x01];
        let newstride = 1;
        let mut cw = [0u8; 8];
        wsfont_rotate_cw(&font, &glyphs, &mut cw, newstride);
        // Clockwise: column b of the original becomes row b, row r becomes column (h - 1 - r).
        assert_eq!(cw[0], 0x01);
        assert_eq!(cw[7], 0x02);
        let mut ccw = [0u8; 8];
        wsfont_rotate_ccw(&font, &glyphs, &mut ccw, newstride);
        assert_eq!(ccw[7], 0x80);
        assert_eq!(ccw[0], 0x40);
    }

    #[test]
    fn bit_reversal_table() {
        assert_eq!(REVERSE[0x01], 0x80);
        assert_eq!(REVERSE[0xc0], 0x03);
        assert_eq!(REVERSE[0xff], 0xff);
    }
}
/* </TESTS> */
