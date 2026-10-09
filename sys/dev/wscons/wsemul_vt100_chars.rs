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

/*
 * Copyright (c) 1998
 *	Matthias Drochner.  All rights reserved.
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
 *
 */
/* </LICENSES> */

/* <CODE> */
//! The vt100 emulation's character sets: ISO Latin-1 supplemental, DEC special graphics,
//! DEC technical and the national replacement sets, mapped to the font's glyphs.
//!
//! Upstream: sys/dev/wscons/wsemul_vt100_chars.c @ 3ce1f3f79392
//!
//! Each set is a table of 128 glyphs the display's `mapchar` gives for the set's Unicode
//! characters; a GL or GR character indexes the table invoked there.
//!
//! ## Deviations
//! - `vt100_setnrc` returns `Err(ERANGE)` for an unknown set, `Ok` otherwise (also when
//!   there is no `nrctab`, as the C's 0).
//! - `nrcovlpos` holds `usize` indexes.

use crate::dev::wscons::unicode::*;
use crate::dev::wscons::wsemul_vt100var::WsemulVt100Emuldata;
use crate::sys::errno::Errno;

/// `decspcgr2uni`: DEC special graphics to Unicode.
#[rustfmt::skip]
pub static DECSPCGR2UNI: [u16; 128] = [
    // 0x00
    0x0000, 0x0001, 0x0002, 0x0003, 0x0004, 0x0005, 0x0006, 0x0007,
    // 0x08
    0x0008, 0x0009, 0x000a, 0x000b, 0x000c, 0x000d, 0x000e, 0x000f,
    // 0x10
    0x0010, 0x0011, 0x0012, 0x0013, 0x0014, 0x0015, 0x0016, 0x0017,
    // 0x18
    0x0018, 0x0019, 0x001a, 0x001b, 0x001c, 0x001d, 0x001e, 0x001f,
    // 0x20
    0x0020, 0x0021, 0x0022, 0x0023, 0x0024, 0x0025, 0x0026, 0x0027,
    // 0x28
    0x0028, 0x0029, 0x002a, 0x002b, 0x002c, 0x002d, 0x002e, 0x002f,
    // 0x30
    0x0030, 0x0031, 0x0032, 0x0033, 0x0034, 0x0035, 0x0036, 0x0037,
    // 0x38
    0x0038, 0x0039, 0x003a, 0x003b, 0x003c, 0x003d, 0x003e, 0x003f,
    // 0x40
    0x0040, 0x0041, 0x0042, 0x0043, 0x0044, 0x0045, 0x0046, 0x0047,
    // 0x48
    0x0048, 0x0049, 0x004a, 0x004b, 0x004c, 0x004d, 0x004e, 0x004f,
    // 0x50
    0x0050, 0x0051, 0x0052, 0x0053, 0x0054, 0x0055, 0x0056, 0x0057,
    // 0x58
    0x0058, 0x0059, 0x005a, 0x005b, 0x005c, 0x005d, 0x005e, 0x00a0,
    // 0x60
    0x25c6, 0x2592, 0x2409, 0x240c, 0x240d, 0x240a, 0x00b0, 0x00b1,
    // 0x68
    _e006U, 0x240b, 0x2518, 0x2510, 0x250c, 0x2514, 0x253c, 0x23ba,
    // 0x70
    0x23bb, 0x2500, 0x23bc, 0x23bd, 0x251c, 0x2524, 0x2534, 0x252c,
    // 0x78
    0x2502, 0x2264, 0x2265, 0x03c0, 0x2260, 0x00a3, 0x00b7, 0x007f,
];

/// `dectech2uni`: DEC technical to Unicode.
#[rustfmt::skip]
pub static DECTECH2UNI: [u16; 128] = [
    // 0x00
    0x0000, 0x0001, 0x0002, 0x0003, 0x0004, 0x0005, 0x0006, 0x0007,
    // 0x08
    0x0008, 0x0009, 0x000a, 0x000b, 0x000c, 0x000d, 0x000e, 0x000f,
    // 0x10
    0x0010, 0x0011, 0x0012, 0x0013, 0x0014, 0x0015, 0x0016, 0x0017,
    // 0x18
    0x0018, 0x0019, 0x001a, 0x001b, 0x001c, 0x001d, 0x001e, 0x001f,
    // 0x20
    0x0020, 0x221a, 0x250c, 0x2212, 0x2320, 0x2321, 0x2502, 0x2308,
    // 0x28
    0x230a, 0x2309, 0x230b, _e007U, _e008U, _e009U, _e00aU, _e00bU,
    // 0x30
    _e00cU, _e00dU, _e00eU, 0x2216, 0x2215, 0x00ac, _e00fU, 0x232a,
    // 0x38
    _e000U, _e000U, _e000U, _e000U, 0x2264, 0x2260, 0x2265, 0x222b,
    // 0x40
    0x2234, 0x221d, 0x221e, 0x00f7, 0x2206, 0x2207, 0x03a6, 0x0393,
    // 0x48
    0x223c, 0x2243, 0x0398, 0x00d7, 0x039b, 0x21d4, 0x21d2, 0x2261,
    // 0x50
    0x03a0, 0x03a8, _e000U, 0x03a3, _e000U, _e000U, 0x221a, 0x03a9,
    // 0x58
    0x039e, 0x03a5, 0x2282, 0x2283, 0x2229, 0x222a, 0x2227, 0x2228,
    // 0x60
    0x00ac, 0x03b1, 0x03b2, 0x03c7, 0x03b4, 0x03b5, 0x03c6, 0x03b3,
    // 0x68
    0x03b7, 0x03b9, 0x03b8, 0x03ba, 0x03bb, _e000U, 0x03bd, 0x2202,
    // 0x70
    0x03c0, 0x03c8, 0x03c1, 0x03c3, 0x03c4, _e000U, 0x0192, 0x03c9,
    // 0x78
    0x03be, 0x03c5, 0x03b6, 0x2190, 0x2191, 0x2192, 0x2193, 0x007f,
];

/// `nrcovlpos`: the positions a national replacement set replaces: `#@[\]^_`{|}~`.
static NRCOVLPOS: [usize; 12] = [
    0x23, 0x40, 0x5b, 0x5c, 0x5d, 0x5e, // #@[\]^
    0x5f, 0x60, 0x7b, 0x7c, 0x7d, 0x7e, // _`{|}~
];

/// `nrctable`: the characters of each national replacement set at `nrcovlpos`.
#[rustfmt::skip]
static NRCTABLE: [[u16; 12]; 12] = [
    // british
    [0x00a3, 0x0040, 0x005b, 0x005c, 0x005d, 0x005e,
     0x005f, 0x0060, 0x007b, 0x007c, 0x007d, 0x007e],
    // dutch
    [0x00a3, 0x00be, 0x00dc, 0x00ab, 0x007c, 0x005e,
     0x005f, 0x0060, 0x00a8, 0x0192, 0x00bc, 0x00b4],
    // finnish
    [0x0023, 0x0040, 0x00c4, 0x00d6, 0x00c5, 0x00dc,
     0x005f, 0x00e9, 0x00e4, 0x00f6, 0x00e5, 0x00fc],
    // french
    [0x00a3, 0x00e0, 0x00b0, 0x00e7, 0x00a7, 0x005e,
     0x005f, 0x0060, 0x00e9, 0x00f9, 0x00e8, 0x00a8],
    // french canadian
    [0x0023, 0x00e0, 0x00e2, 0x00e7, 0x00ea, 0x00ee,
     0x005f, 0x00f4, 0x00e9, 0x00f9, 0x00e8, 0x00fb],
    // german
    [0x0023, 0x00a7, 0x00c4, 0x00d6, 0x00dc, 0x005e,
     0x005f, 0x0060, 0x00e4, 0x00f6, 0x00fc, 0x00df],
    // italian
    [0x00a3, 0x00a7, 0x00b0, 0x00e7, 0x00e9, 0x005e,
     0x005f, 0x00f9, 0x00e0, 0x00f2, 0x00e8, 0x00ec],
    // norwegian /danish
    [0x0023, 0x0040, 0x00c6, 0x00d8, 0x00c5, 0x005e,
     0x005f, 0x0060, 0x00e6, 0x00f8, 0x00e5, 0x007e],
    // portuguese
    [0x0023, 0x0040, 0x00c3, 0x00c7, 0x00d5, 0x005e,
     0x005f, 0x0060, 0x00e3, 0x00e7, 0x00f5, 0x007e],
    // spanish
    [0x00a3, 0x00a7, 0x00a1, 0x00d1, 0x00bf, 0x005e,
     0x005f, 0x0060, 0x0060, 0x00b0, 0x00f1, 0x00e7],
    // swedish
    [0x0023, 0x00c9, 0x00c4, 0x00d6, 0x00c5, 0x00dc,
     0x005f, 0x00e9, 0x00e4, 0x00f6, 0x00e5, 0x00fc],
    // swiss
    [0x00f9, 0x00e0, 0x00e9, 0x00e7, 0x00ea, 0x00ee,
     0x00e8, 0x00f4, 0x00e4, 0x00f6, 0x00fc, 0x00fb],
];

/// `vt100_initchartables`: (re)compute the character tables from the display's font.
pub fn vt100_initchartables(edp: &mut WsemulVt100Emuldata) {
    let eo = edp.emulops;

    if let Some(t) = &mut edp.isolatin1tab {
        for (i, g) in t.iter_mut().enumerate().take(128) {
            eo.mapchar(128 + i as i32, g);
        }
    }

    if let Some(t) = &mut edp.decgraphtab {
        for (g, &u) in t.iter_mut().zip(DECSPCGR2UNI.iter()) {
            eo.mapchar(i32::from(u), g);
        }
    }

    if let Some(t) = &mut edp.dectechtab {
        for (g, &u) in t.iter_mut().zip(DECTECH2UNI.iter()) {
            eo.mapchar(i32::from(u), g);
        }
    }

    let _ = vt100_setnrc(edp, 0);
}

/// `vt100_setnrc`: make `nrctab` national replacement set `nrc` (0 british to 11 swiss).
pub fn vt100_setnrc(edp: &mut WsemulVt100Emuldata, nrc: i32) -> Result<(), Errno> {
    let eo = edp.emulops;
    let Some(t) = &mut edp.nrctab else {
        return Ok(());
    };

    let Some(set) = usize::try_from(nrc).ok().and_then(|n| NRCTABLE.get(n)) else {
        return Err(Errno::ERANGE);
    };

    for (i, g) in t.iter_mut().enumerate().take(128) {
        eo.mapchar(i as i32, g);
    }
    for (&pos, &u) in NRCOVLPOS.iter().zip(set.iter()) {
        eo.mapchar(i32::from(u), &mut t[pos]);
    }

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// The tables against `wsemul_vt100_chars.c`'s.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn tables_match_the_c_file() {
        use std::string::String;
        let path = crate::reftest::openbsd_src().join("sys/dev/wscons/wsemul_vt100_chars.c");
        let text = std::fs::read_to_string(path).unwrap();
        let defs = crate::reftest::defines("sys/dev/wscons/unicode.h");
        let value = |t: &str| match crate::reftest::int(&defs, t) {
            Some(v) => v as u16,
            None => u16::from_str_radix(t.trim_start_matches("0x"), 16).unwrap(),
        };
        let table = |name: &str| -> std::vec::Vec<u16> {
            let start = text.find(&std::format!("{name}[")).unwrap();
            let body = &text[start..];
            let body = &body[body.find('{').unwrap() + 1..body.find("};").unwrap()];
            // Drop the comments, which also open lines (`/* 2 */ 0x0020, ...`).
            let mut code = String::new();
            let mut rest = body;
            while let Some(i) = rest.find("/*") {
                code.push_str(&rest[..i]);
                rest = &rest[i + rest[i..].find("*/").unwrap() + 2..];
            }
            code.push_str(rest);
            code.split(',')
                .map(|t| {
                    t.trim()
                        .trim_start_matches('{')
                        .trim_end_matches('}')
                        .trim()
                })
                .filter(|t| !t.is_empty())
                .map(value)
                .collect()
        };
        assert_eq!(table("decspcgr2uni"), DECSPCGR2UNI);
        assert_eq!(table("dectech2uni"), DECTECH2UNI);
        assert_eq!(table("nrctable"), NRCTABLE.concat());
    }
}
/* </TESTS> */
