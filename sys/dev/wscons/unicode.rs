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
/* </LICENSES> */

/* <CODE> */
//! `<dev/wscons/unicode.h>`: some private character definitions for stuff not found in the
//! Unicode database, for communication between terminal emulation and graphics driver.
//!
//! Upstream: sys/dev/wscons/unicode.h @ 3ce1f3f79392
//!
//! The original file carries no licence text, only its `$OpenBSD$` and `$NetBSD$` lines.
//! The values sit in the Unicode private use area; the DEC graphics tables of
//! `wsemul_vt100_chars` map to them and a font's `mapchar` decides what to draw.
//!
//! ## Deviations
//! - The constants are `u16`, the element type of the tables that use them.

#![allow(non_upper_case_globals)] // the C names (`_e000U`), verbatim, for grep-ability

/// `_e000U`: mirrored question mark?
pub const _e000U: u16 = 0xe000;
/// `_e006U`: N/L control.
pub const _e006U: u16 = 0xe006;
/// `_e007U`: bracelefttp.
pub const _e007U: u16 = 0xe007;
/// `_e008U`: braceleftbt.
pub const _e008U: u16 = 0xe008;
/// `_e009U`: bracerighttp.
pub const _e009U: u16 = 0xe009;
/// `_e00aU`: bracerighrbt.
pub const _e00aU: u16 = 0xe00a;
/// `_e00bU`: braceleftmid.
pub const _e00bU: u16 = 0xe00b;
/// `_e00cU`: bracerightmid.
pub const _e00cU: u16 = 0xe00c;
/// `_e00dU`: inverted angle?
pub const _e00dU: u16 = 0xe00d;
/// `_e00eU`: angle?
pub const _e00eU: u16 = 0xe00e;
/// `_e00fU`: mirrored not sign?
pub const _e00fU: u16 = 0xe00f;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// Every constant against `<dev/wscons/unicode.h>`.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/wscons/unicode.h");
        crate::reftest::assert_defines!(defs;
            _e000U, _e006U, _e007U, _e008U, _e009U, _e00aU, _e00bU, _e00cU, _e00dU, _e00eU,
            _e00fU,
        );
    }
}
/* </TESTS> */
