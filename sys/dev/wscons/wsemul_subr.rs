/*	$OpenBSD: wsemul_subr.c,v 1.2 2023/03/06 17:14:44 miod Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2007, 2013 Miodrag Vallat.
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice, this permission notice, and the disclaimer below
 * appear in all copies.
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
 * Part of the UTF-8 state machine logic borrowed from citrus_utf8.c
 * under the following licence:
 */
/*-
 * Copyright (c) 2002-2004 Tim J. Robbins
 * All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

//! Helpers shared by the terminal emulations: UTF-8 decoding of the output stream, and
//! keysym to byte sequence translation (UTF-8, or the 8-bit charset the keyboard layout
//! suggests).
//!
//! Upstream: sys/dev/wscons/wsemul_subr.c @ 3ce1f3f79392
//!
//! Part of the UTF-8 state machine logic is borrowed from `citrus_utf8.c` (the second
//! licence block above).
//!
//! ## Deviations
//! - `wsemul_getchar` takes the input as a slice it advances (the C's `inbuf`/`inlen`
//!   pair) and returns `Err(EAGAIN)` when the input ended before a complete character,
//!   `Err(EILSEQ)` when the last bytes were an ill-formed sequence, where the C returns
//!   those numbers.
//! - The translation functions write into the caller's [`WSEMUL_TRANSLATE_SIZE`]-byte
//!   buffer and return the length as `usize`.
//! - The `#ifdef HAVE_UTF8_SUPPORT` branches test the constant of `wscons_features`.

use crate::dev::wscons::wscons_features::HAVE_UTF8_SUPPORT;
use crate::dev::wscons::wsemulvar::{WSEMUL_TRANSLATE_SIZE, WsemulInputstate};
use crate::dev::wscons::wsksymdef::*;
use crate::dev::wscons::wsksymvar::KbdT;
use crate::sys::errno::Errno;

/// What one byte did to the character being decoded.
enum Step {
    /// More bytes are needed.
    Next,
    /// A character is complete.
    Done,
    /// The sequence is ill-formed.
    Invalid,
}

/// `cyrillic_to_koi8`: Unicode Cyrillic to KOI8 translation table (starts at U+0400), from
/// RFC 2319.
#[rustfmt::skip]
pub static CYRILLIC_TO_KOI8: [u8; 96] = [
    // U+0400
    0x00, 0xb3, 0x00, 0x00, 0xb4, 0x00, 0xb6, 0xb7,
    // U+0408
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // U+0410
    0xe1, 0xe2, 0xf7, 0xe7, 0xe4, 0xe5, 0xf6, 0xfa,
    // U+0418
    0xe9, 0xea, 0xeb, 0xec, 0xed, 0xee, 0xef, 0xf0,
    // U+0420
    0xf2, 0xf3, 0xf4, 0xf5, 0xe6, 0xe8, 0xe3, 0xfe,
    // U+0428
    0xfb, 0xfd, 0xff, 0xf9, 0xf8, 0xfc, 0xe0, 0xf1,
    // U+0430
    0xc1, 0xc2, 0xd7, 0xc7, 0xc4, 0xc5, 0xd6, 0xda,
    // U+0438
    0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf, 0xd0,
    // U+0440
    0xd2, 0xd3, 0xd4, 0xd5, 0xc6, 0xc8, 0xc3, 0xde,
    // U+0448
    0xdb, 0xdd, 0xdf, 0xd9, 0xd8, 0xdc, 0xc0, 0xd1,
    // U+0450
    0x00, 0xa3, 0x00, 0x00, 0xa4, 0x00, 0xa6, 0xa7,
    // U+0458
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// `unicode_to_latin2`: Europe to Latin-2 translation table (starts at U+0100).
#[rustfmt::skip]
pub static UNICODE_TO_LATIN2: [u8; 128] = [
    // U+0100
    0x00, 0x00, 0xc3, 0xe3, 0xa1, 0xb1, 0xc6, 0xe6,
    // U+0108
    0x00, 0x00, 0x00, 0x00, 0xc8, 0xe8, 0xcf, 0xef,
    // U+0110
    0xd0, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // U+0118
    0xca, 0xea, 0xcc, 0xec, 0x00, 0x00, 0x00, 0x00,
    // U+0120
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // U+0128
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // U+0130
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // U+0138
    0x00, 0xc5, 0xe5, 0x00, 0x00, 0xa5, 0xb5, 0x00,
    // U+0140
    0x00, 0xa3, 0xb3, 0xd1, 0xf1, 0x00, 0x00, 0xd2,
    // U+0148
    0xf2, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // U+0150
    0xd5, 0xf5, 0x00, 0x00, 0xc0, 0xe0, 0x00, 0x00,
    // U+0158
    0xd8, 0xf8, 0xa6, 0xb6, 0x00, 0x00, 0xaa, 0xba,
    // U+0160
    0xa9, 0xb9, 0xde, 0xfe, 0xab, 0xbb, 0x00, 0x00,
    // U+0168
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xd9, 0xf9,
    // U+0170
    0xdb, 0xfb, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // U+0178
    0x00, 0xac, 0xbc, 0xaf, 0xbf, 0xae, 0xbe, 0x00,
];

/// `unicode_to_latin7`: Baltic to Latin-7 translation table (starts at U+0100).
#[rustfmt::skip]
pub static UNICODE_TO_LATIN7: [u8; 128] = [
    // U+0100
    0xc2, 0xe2, 0x00, 0x00, 0xc0, 0xe0, 0xc3, 0xe3,
    // U+0108
    0x00, 0x00, 0x00, 0x00, 0xc8, 0xe8, 0x00, 0x00,
    // U+0110
    0x00, 0x00, 0xc7, 0xe7, 0x00, 0x00, 0xcb, 0xeb,
    // U+0118
    0xc6, 0xe6, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // U+0120
    0x00, 0x00, 0xcc, 0xec, 0x00, 0x00, 0x00, 0x00,
    // U+0128
    0x00, 0x00, 0xce, 0xee, 0x00, 0x00, 0xc1, 0xe1,
    // U+0130
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xcd, 0xed,
    // U+0138
    0x00, 0x00, 0x00, 0xcf, 0xef, 0x00, 0x00, 0x00,
    // U+0140
    0x00, 0xd9, 0xf9, 0xd1, 0xf1, 0xd2, 0xf2, 0x00,
    // U+0148
    0x00, 0x00, 0x00, 0x00, 0xd4, 0xf4, 0x00, 0x00,
    // U+0150
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xaa, 0xba,
    // U+0158
    0x00, 0x00, 0xda, 0xfa, 0x00, 0x00, 0x00, 0x00,
    // U+0160
    0xd0, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // U+0168
    0x00, 0x00, 0xdb, 0xfb, 0x00, 0x00, 0x00, 0x00,
    // U+0170
    0x00, 0x00, 0xd8, 0xf8, 0x00, 0x00, 0x00, 0x00,
    // U+0178
    0x00, 0xca, 0xea, 0xdd, 0xfd, 0xde, 0xfe, 0x00,
];

/// `wsemul_getchar`: get characters from an input stream and update the input state.
/// Processing stops when the stream is empty (`Err(EAGAIN)`, or `Err(EILSEQ)` when an
/// ill-formed sequence was the last thing seen), or a complete character sequence has been
/// recognized, in which case it returns `Ok` with the character in `state.inchar`.
pub fn wsemul_getchar(
    inbuf: &mut &[u8],
    state: &mut WsemulInputstate,
    allow_utf8: bool,
) -> Result<(), Errno> {
    let Some((&first, rest)) = inbuf.split_first() else {
        return Err(Errno::EAGAIN);
    };

    // If we do not allow multibyte sequences, process as quickly as possible.
    if !HAVE_UTF8_SUPPORT || !allow_utf8 {
        state.inchar = u32::from(first);
        state.mbleft = 0;
        *inbuf = rest;
        return Ok(());
    }

    let mut rc = Err(Errno::EAGAIN);
    let mut tmpchar = state.inchar;
    let mut lbound = state.lbound;
    let mut mbleft = state.mbleft;
    let mut buf = *inbuf;

    while let Some((&b, rest)) = buf.split_first() {
        buf = rest;
        let frag = u32::from(b);

        let step = if mbleft != 0 {
            // We are in the middle of a multibyte sequence: try to complete it.
            if frag & 0xc0 != 0x80 {
                Step::Invalid
            } else {
                tmpchar = (tmpchar << 6) | (frag & 0x3f);
                mbleft -= 1;
                if mbleft != 0 {
                    Step::Next
                } else if tmpchar < lbound
                    || (0xd800..0xe000).contains(&tmpchar)
                    || tmpchar >= 0x110000
                {
                    Step::Invalid
                } else {
                    Step::Done
                }
            }
        } else if frag & 0x80 == 0 {
            // A 7-bit character.
            tmpchar = frag;
            Step::Done
        } else {
            // The start of a new multibyte sequence, if well-formed.
            let start = if frag & 0xe0 == 0xc0 {
                Some((frag & 0x1f, 1, 0x80))
            } else if frag & 0xf0 == 0xe0 {
                Some((frag & 0x0f, 2, 0x800))
            } else if frag & 0xf8 == 0xf0 {
                Some((frag & 0x07, 3, 0x10000))
            } else {
                None
            };
            match start {
                Some((bits, left, bound)) => {
                    tmpchar = bits;
                    mbleft = left;
                    lbound = bound;
                    state.lbound = lbound;
                    Step::Next
                }
                None => Step::Invalid,
            }
        };

        match step {
            Step::Next => {}
            Step::Done => {
                rc = Ok(());
                break;
            }
            Step::Invalid => {
                // Abort the ill-formed sequence and continue.
                mbleft = 0;
                tmpchar = 0;
                rc = Err(Errno::EILSEQ);
            }
        }
    }

    state.inchar = tmpchar;
    state.mbleft = mbleft;
    *inbuf = buf;
    rc
}

/// `wsemul_local_translate`: keysym to local 8-bit charset sequence translation function.
/// The keyboard layout is used as a hint to decide which latin charset to assume. Always
/// one byte.
#[allow(non_upper_case_globals)] // the keysym names (`KS_Help`), verbatim, as patterns
pub fn wsemul_local_translate(
    unisym: u32,
    layout: KbdT,
    out: &mut [u8; WSEMUL_TRANSLATE_SIZE],
) -> usize {
    let mut unisym = unisym;
    let enc = kb_encoding(layout);
    // Each arm below covers 128 code points under 0x10000, so `unisym as u16` is exact
    // where it is compared with the keysyms.
    match unisym >> 7 {
        0x01 => {
            // U+0080
            if matches!(enc, KB_LT | KB_LV) {
                match unisym as u16 {
                    KS_L7_AE => unisym = 0xaf,
                    KS_L7_Ostroke => unisym = 0xa8,
                    KS_L7_ae => unisym = 0xbf,
                    KS_L7_ostroke => unisym = 0xb8,
                    _ => {}
                }
            }
        }
        0x02 => match enc {
            // U+0100
            KB_LT | KB_LV => {
                let t = UNICODE_TO_LATIN7[(unisym - 0x100) as usize];
                if t != 0 {
                    unisym = u32::from(t);
                }
            }
            KB_TR => match unisym as u16 {
                KS_L5_Gbreve => unisym = 0xd0,
                KS_L5_gbreve => unisym = 0xf0,
                KS_L5_Idotabove => unisym = 0xdd,
                KS_L5_idotless => unisym = 0xfd,
                KS_L5_Scedilla => unisym = 0xde,
                KS_L5_scedilla => unisym = 0xfe,
                _ => {}
            },
            KB_PL | KB_SI => {
                let t = UNICODE_TO_LATIN2[(unisym - 0x100) as usize];
                if t != 0 {
                    unisym = u32::from(t);
                }
            }
            _ => {}
        },
        0x05 => {
            // U+0280
            if matches!(enc, KB_PL | KB_SI) {
                match unisym as u16 {
                    KS_L2_caron => unisym = 0xb7,
                    KS_L2_breve => unisym = 0xa2,
                    KS_L2_dotabove => unisym = 0xff,
                    KS_L2_ogonek => unisym = 0xb2,
                    KS_L2_dblacute => unisym = 0xbd,
                    _ => {}
                }
            }
        }
        0x08 => {
            // U+0400
            if let Some(&t) = CYRILLIC_TO_KOI8.get((unisym - 0x400) as usize)
                && t != 0
            {
                unisym = u32::from(t);
            }
        }
        0x09 => {
            // U+0480
            if unisym == u32::from(KS_Cyrillic_GHEUKR) {
                unisym = 0xbd; // ukrainian GHE
            } else if unisym == u32::from(KS_Cyrillic_gheukr) {
                unisym = 0xad; // ukrainian ghe
            }
        }
        0x40 => {
            // U+2000
            if matches!(enc, KB_LT | KB_LV) {
                match unisym as u16 {
                    KS_L7_rightsnglquot => unisym = 0xff,
                    KS_L7_leftdblquot => unisym = 0xb4,
                    KS_L7_rightdblquot => unisym = 0xa1,
                    KS_L7_dbllow9quot => unisym = 0xa5,
                    _ => {}
                }
            }
        }
        _ => {}
    }

    out[0] = (unisym & 0xff) as u8;
    1
}

/// `wsemul_utf8_translate`: keysym to UTF-8 sequence translation function; the length
/// written to `out`, 0 for a surrogate or a code point past U+10FFFF. Without
/// `allow_utf8`, [`wsemul_local_translate`].
pub fn wsemul_utf8_translate(
    unisym: u32,
    layout: KbdT,
    out: &mut [u8; WSEMUL_TRANSLATE_SIZE],
    allow_utf8: bool,
) -> usize {
    if !HAVE_UTF8_SUPPORT || !allow_utf8 {
        return wsemul_local_translate(unisym, layout, out);
    }

    if unisym < 0x80 {
        // Fast path for plain ASCII characters.
        out[0] = unisym as u8;
        return 1;
    }

    let (headpat, length) = if unisym < 0x800 {
        (0xc0, 2)
    } else if unisym < 0x10000 {
        if (0xd800..0xe000).contains(&unisym) {
            return 0;
        }
        (0xe0, 3)
    } else {
        if unisym >= 0x110000 {
            return 0;
        }
        (0xf0, 4)
    };

    let mut unisym = unisym;
    for pos in (1..length).rev() {
        out[pos] = 0x80 | (unisym & 0x3f) as u8;
        unisym >>= 6;
    }
    out[0] = headpat | unisym as u8;

    length
}

#[cfg(test)]
mod tests;
