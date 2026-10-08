/* $OpenBSD: pcdisplay_chars.c,v 1.6 2023/04/13 18:29:35 miod Exp $ */
/* $NetBSD: pcdisplay_chars.c,v 1.5 2000/06/08 07:01:19 cgd Exp $ */
/* <LICENSES> */
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
//! `pcdisplay_mapchar`: the glyph of a Unicode character in the IBM PC's code page 437, the
//! font a VGA's character generator holds after the firmware loaded it.
//!
//! Upstream: sys/dev/ic/pcdisplay_chars.c @ 3ce1f3f79392
//!
//! ASCII maps to itself; ISO 8859-1 through [`ISOMAPPINGS`]; about a hundred other
//! characters (Greek letters, arrows, box drawing, card suits) through [`UNIMAPPINGS`]; and
//! some characters CP437 lacks get a look-alike from [`REPLACEMENTS`], with a lower
//! quality. Anything else is the diamond, quality 0.
//!
//! ## Deviations
//! - The emulops cookie the C takes (and ignores) is not a parameter.
//! - The tables are the C's, entry for entry; the structures of `unimappings[]` and
//!   `replacements[]` are the private [`UniMapping`] and [`Replacement`].

use crate::dev::wscons::unicode::{
    _e00aU, _e00bU, _e00cU, _e00dU, _e00eU, _e00fU, _e007U, _e008U, _e009U,
};

/// `CONTROL`: XXX smiley.
const CONTROL: u8 = 1;
/// `NOTPRINTABLE`: diamond XXX watch out - not in ISO part!
const NOTPRINTABLE: u8 = 4;

/// An entry of `unimappings[]`.
struct UniMapping {
    /// `uni`.
    uni: u16,
    /// `ibm`.
    ibm: u8,
}

/// An entry of `replacements[]`.
struct Replacement {
    /// `uni`.
    uni: u16,
    /// `ibm`.
    ibm: u8,
    /// `quality`.
    quality: i32,
}

/// `isomappings[128]`: the CP437 glyph of each ISO 8859-1 character from 0x80 to 0xff.
static ISOMAPPINGS: [u8; 128] = [
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    CONTROL,
    0xff,         // 0x00a0 NO-BREAK SPACE
    0xad,         // 0x00a1 INVERTED EXCLAMATION MARK
    0x9b,         // 0x00a2 CENT SIGN
    0x9c,         // 0x00a3 POUND SIGN
    NOTPRINTABLE, // 0x00a4 CURRENCY SIGN
    0x9d,         // 0x00a5 YEN SIGN
    0x7c,         // 0x00a6 BROKEN BAR
    0x15,         // 0x00a7 SECTION SIGN
    NOTPRINTABLE, // 0x00a8 DIAERESIS
    NOTPRINTABLE, // 0x00a9 COPYRIGHT SIGN
    0xa6,         // 0x00aa FEMININE ORDINAL INDICATOR
    0xae,         // 0x00ab LEFT-POINTING DOUBLE ANGLE QUOTATION MARK
    0xaa,         // 0x00ac NOT SIGN
    0xc4,         // 0x00ad SOFT HYPHEN
    NOTPRINTABLE, // 0x00ae REGISTERED SIGN
    NOTPRINTABLE, // 0x00af MACRON
    0xf8,         // 0x00b0 DEGREE SIGN
    0xf1,         // 0x00b1 PLUS-MINUS SIGN
    0xfd,         // 0x00b2 SUPERSCRIPT TWO
    NOTPRINTABLE, // 0x00b3 SUPERSCRIPT THREE
    0x27,         // 0x00b4 ACUTE ACCENT
    0xe6,         // 0x00b5 MICRO SIGN
    0x14,         // 0x00b6 PILCROW SIGN
    0xfa,         // 0x00b7 MIDDLE DOT
    NOTPRINTABLE, // 0x00b8 CEDILLA
    NOTPRINTABLE, // 0x00b9 SUPERSCRIPT ONE
    0xa7,         // 0x00ba MASCULINE ORDINAL INDICATOR
    0xaf,         // 0x00bb RIGHT-POINTING DOUBLE ANGLE QUOTATION MARK
    0xac,         // 0x00bc VULGAR FRACTION ONE QUARTER
    0xab,         // 0x00bd VULGAR FRACTION ONE HALF
    NOTPRINTABLE, // 0x00be VULGAR FRACTION THREE QUARTERS
    0xa8,         // 0x00bf INVERTED QUESTION MARK
    NOTPRINTABLE, // 0x00c0 LATIN CAPITAL LETTER A WITH GRAVE
    NOTPRINTABLE, // 0x00c1 LATIN CAPITAL LETTER A WITH ACUTE
    NOTPRINTABLE, // 0x00c2 LATIN CAPITAL LETTER A WITH CIRCUMFLEX
    NOTPRINTABLE, // 0x00c3 LATIN CAPITAL LETTER A WITH TILDE
    0x8e,         // 0x00c4 LATIN CAPITAL LETTER A WITH DIAERESIS
    0x8f,         // 0x00c5 LATIN CAPITAL LETTER A WITH RING ABOVE
    0x92,         // 0x00c6 LATIN CAPITAL LIGATURE AE
    0x80,         // 0x00c7 LATIN CAPITAL LETTER C WITH CEDILLA
    NOTPRINTABLE, // 0x00c8 LATIN CAPITAL LETTER E WITH GRAVE
    0x90,         // 0x00c9 LATIN CAPITAL LETTER E WITH ACUTE
    NOTPRINTABLE, // 0x00ca LATIN CAPITAL LETTER E WITH CIRCUMFLEX
    NOTPRINTABLE, // 0x00cb LATIN CAPITAL LETTER E WITH DIAERESIS
    NOTPRINTABLE, // 0x00cc LATIN CAPITAL LETTER I WITH GRAVE
    NOTPRINTABLE, // 0x00cd LATIN CAPITAL LETTER I WITH ACUTE
    NOTPRINTABLE, // 0x00ce LATIN CAPITAL LETTER I WITH CIRCUMFLEX
    NOTPRINTABLE, // 0x00cf LATIN CAPITAL LETTER I WITH DIAERESIS
    NOTPRINTABLE, // 0x00d0 LATIN CAPITAL LETTER ETH
    0xa5,         // 0x00d1 LATIN CAPITAL LETTER N WITH TILDE
    NOTPRINTABLE, // 0x00d2 LATIN CAPITAL LETTER O WITH GRAVE
    NOTPRINTABLE, // 0x00d3 LATIN CAPITAL LETTER O WITH ACUTE
    NOTPRINTABLE, // 0x00d4 LATIN CAPITAL LETTER O WITH CIRCUMFLEX
    NOTPRINTABLE, // 0x00d5 LATIN CAPITAL LETTER O WITH TILDE
    0x99,         // 0x00d6 LATIN CAPITAL LETTER O WITH DIAERESIS
    NOTPRINTABLE, // 0x00d7 MULTIPLICATION SIGN
    NOTPRINTABLE, // 0x00d8 LATIN CAPITAL LETTER O WITH STROKE
    NOTPRINTABLE, // 0x00d9 LATIN CAPITAL LETTER U WITH GRAVE
    NOTPRINTABLE, // 0x00da LATIN CAPITAL LETTER U WITH ACUTE
    NOTPRINTABLE, // 0x00db LATIN CAPITAL LETTER U WITH CIRCUMFLEX
    0x9a,         // 0x00dc LATIN CAPITAL LETTER U WITH DIAERESIS
    NOTPRINTABLE, // 0x00dd LATIN CAPITAL LETTER Y WITH ACUTE
    NOTPRINTABLE, // 0x00de LATIN CAPITAL LETTER THORN
    0xe1,         // 0x00df LATIN SMALL LETTER SHARP S
    0x85,         // 0x00e0 LATIN SMALL LETTER A WITH GRAVE
    0xa0,         // 0x00e1 LATIN SMALL LETTER A WITH ACUTE
    0x83,         // 0x00e2 LATIN SMALL LETTER A WITH CIRCUMFLEX
    NOTPRINTABLE, // 0x00e3 LATIN SMALL LETTER A WITH TILDE
    0x84,         // 0x00e4 LATIN SMALL LETTER A WITH DIAERESIS
    0x86,         // 0x00e5 LATIN SMALL LETTER A WITH RING ABOVE
    0x91,         // 0x00e6 LATIN SMALL LIGATURE AE
    0x87,         // 0x00e7 LATIN SMALL LETTER C WITH CEDILLA
    0x8a,         // 0x00e8 LATIN SMALL LETTER E WITH GRAVE
    0x82,         // 0x00e9 LATIN SMALL LETTER E WITH ACUTE
    0x88,         // 0x00ea LATIN SMALL LETTER E WITH CIRCUMFLEX
    0x89,         // 0x00eb LATIN SMALL LETTER E WITH DIAERESIS
    0x8d,         // 0x00ec LATIN SMALL LETTER I WITH GRAVE
    0xa1,         // 0x00ed LATIN SMALL LETTER I WITH ACUTE
    0x8c,         // 0x00ee LATIN SMALL LETTER I WITH CIRCUMFLEX
    0x8b,         // 0x00ef LATIN SMALL LETTER I WITH DIAERESIS
    NOTPRINTABLE, // 0x00f0 LATIN SMALL LETTER ETH
    0xa4,         // 0x00f1 LATIN SMALL LETTER N WITH TILDE
    0x95,         // 0x00f2 LATIN SMALL LETTER O WITH GRAVE
    0xa2,         // 0x00f3 LATIN SMALL LETTER O WITH ACUTE
    0x93,         // 0x00f4 LATIN SMALL LETTER O WITH CIRCUMFLEX
    NOTPRINTABLE, // 0x00f5 LATIN SMALL LETTER O WITH TILDE
    0x94,         // 0x00f6 LATIN SMALL LETTER O WITH DIAERESIS
    0xf6,         // 0x00f7 DIVISION SIGN
    NOTPRINTABLE, // 0x00f8 LATIN SMALL LETTER O WITH STROKE
    0x97,         // 0x00f9 LATIN SMALL LETTER U WITH GRAVE
    0xa3,         // 0x00fa LATIN SMALL LETTER U WITH ACUTE
    0x96,         // 0x00fb LATIN SMALL LETTER U WITH CIRCUMFLEX
    0x81,         // 0x00fc LATIN SMALL LETTER U WITH DIAERESIS
    NOTPRINTABLE, // 0x00fd LATIN SMALL LETTER Y WITH ACUTE
    NOTPRINTABLE, // 0x00fe LATIN SMALL LETTER THORN
    0x98,         // 0x00ff LATIN SMALL LETTER Y WITH DIAERESIS
];

/// `unimappings[]`: Unicode characters outside ISO 8859-1 with a CP437 glyph.
static UNIMAPPINGS: [UniMapping; 108] = [
    UniMapping {
        uni: 0x0192,
        ibm: 0x9f,
    }, // LATIN SMALL LETTER F WITH HOOK
    UniMapping {
        uni: 0x0393,
        ibm: 0xe2,
    }, // GREEK CAPITAL LETTER GAMMA
    UniMapping {
        uni: 0x0398,
        ibm: 0xe9,
    }, // GREEK CAPITAL LETTER THETA
    UniMapping {
        uni: 0x03a3,
        ibm: 0xe4,
    }, // GREEK CAPITAL LETTER SIGMA
    UniMapping {
        uni: 0x03a6,
        ibm: 0xe8,
    }, // GREEK CAPITAL LETTER PHI
    UniMapping {
        uni: 0x03a9,
        ibm: 0xea,
    }, // GREEK CAPITAL LETTER OMEGA
    UniMapping {
        uni: 0x03b1,
        ibm: 0xe0,
    }, // GREEK SMALL LETTER ALPHA
    UniMapping {
        uni: 0x03b2,
        ibm: 0xe1,
    }, // GREEK SMALL LETTER BETA
    UniMapping {
        uni: 0x03b4,
        ibm: 0xeb,
    }, // GREEK SMALL LETTER DELTA
    UniMapping {
        uni: 0x03b5,
        ibm: 0xee,
    }, // GREEK SMALL LETTER EPSILON
    UniMapping {
        uni: 0x03c0,
        ibm: 0xe3,
    }, // GREEK SMALL LETTER PI
    UniMapping {
        uni: 0x03c3,
        ibm: 0xe5,
    }, // GREEK SMALL LETTER SIGMA
    UniMapping {
        uni: 0x03c4,
        ibm: 0xe7,
    }, // GREEK SMALL LETTER TAU
    UniMapping {
        uni: 0x03c6,
        ibm: 0xed,
    }, // GREEK SMALL LETTER PHI
    UniMapping {
        uni: 0x2022,
        ibm: 0x07,
    }, // BULLET
    UniMapping {
        uni: 0x203c,
        ibm: 0x13,
    }, // DOUBLE EXCLAMATION MARK
    UniMapping {
        uni: 0x207f,
        ibm: 0xfc,
    }, // SUPERSCRIPT LATIN SMALL LETTER N
    UniMapping {
        uni: 0x20a7,
        ibm: 0x9e,
    }, // PESETA SIGN
    UniMapping {
        uni: 0x2190,
        ibm: 0x1b,
    }, // LEFTWARDS ARROW
    UniMapping {
        uni: 0x2191,
        ibm: 0x18,
    }, // UPWARDS ARROW
    UniMapping {
        uni: 0x2192,
        ibm: 0x1a,
    }, // RIGHTWARDS ARROW
    UniMapping {
        uni: 0x2193,
        ibm: 0x19,
    }, // DOWNWARDS ARROW
    UniMapping {
        uni: 0x2194,
        ibm: 0x1d,
    }, // LEFT RIGHT ARROW
    UniMapping {
        uni: 0x2195,
        ibm: 0x12,
    }, // UP DOWN ARROW
    UniMapping {
        uni: 0x21a8,
        ibm: 0x17,
    }, // UP DOWN ARROW WITH BASE
    UniMapping {
        uni: 0x2212,
        ibm: 0x2d,
    }, // MINUS SIGN XXX move to more general place
    UniMapping {
        uni: 0x2215,
        ibm: 0x2f,
    }, // DIVISION SLASH XXX move to more general place
    UniMapping {
        uni: 0x2219,
        ibm: 0xf9,
    }, // BULLET OPERATOR
    UniMapping {
        uni: 0x221a,
        ibm: 0xfb,
    }, // SQUARE ROOT
    UniMapping {
        uni: 0x221e,
        ibm: 0xec,
    }, // INFINITY
    UniMapping {
        uni: 0x2229,
        ibm: 0xef,
    }, // INTERSECTION
    UniMapping {
        uni: 0x2248,
        ibm: 0xf7,
    }, // ALMOST EQUAL TO
    UniMapping {
        uni: 0x2261,
        ibm: 0xf0,
    }, // IDENTICAL TO
    UniMapping {
        uni: 0x2264,
        ibm: 0xf3,
    }, // LESS-THAN OR EQUAL TO
    UniMapping {
        uni: 0x2265,
        ibm: 0xf2,
    }, // GREATER-THAN OR EQUAL TO
    UniMapping {
        uni: 0x2302,
        ibm: 0x7f,
    }, // HOUSE
    UniMapping {
        uni: 0x2310,
        ibm: 0xa9,
    }, // REVERSED NOT SIGN
    UniMapping {
        uni: 0x2320,
        ibm: 0xf4,
    }, // TOP HALF INTEGRAL
    UniMapping {
        uni: 0x2321,
        ibm: 0xf5,
    }, // BOTTOM HALF INTEGRAL
    UniMapping {
        uni: 0x2500,
        ibm: 0xc4,
    }, // BOX DRAWINGS LIGHT HORIZONTAL
    UniMapping {
        uni: 0x2502,
        ibm: 0xb3,
    }, // BOX DRAWINGS LIGHT VERTICAL
    UniMapping {
        uni: 0x250c,
        ibm: 0xda,
    }, // BOX DRAWINGS LIGHT DOWN AND RIGHT
    UniMapping {
        uni: 0x2510,
        ibm: 0xbf,
    }, // BOX DRAWINGS LIGHT DOWN AND LEFT
    UniMapping {
        uni: 0x2514,
        ibm: 0xc0,
    }, // BOX DRAWINGS LIGHT UP AND RIGHT
    UniMapping {
        uni: 0x2518,
        ibm: 0xd9,
    }, // BOX DRAWINGS LIGHT UP AND LEFT
    UniMapping {
        uni: 0x251c,
        ibm: 0xc3,
    }, // BOX DRAWINGS LIGHT VERTICAL AND RIGHT
    UniMapping {
        uni: 0x2524,
        ibm: 0xb4,
    }, // BOX DRAWINGS LIGHT VERTICAL AND LEFT
    UniMapping {
        uni: 0x252c,
        ibm: 0xc2,
    }, // BOX DRAWINGS LIGHT DOWN AND HORIZONTAL
    UniMapping {
        uni: 0x2534,
        ibm: 0xc1,
    }, // BOX DRAWINGS LIGHT UP AND HORIZONTAL
    UniMapping {
        uni: 0x253c,
        ibm: 0xc5,
    }, // BOX DRAWINGS LIGHT VERTICAL AND HORIZONTAL
    UniMapping {
        uni: 0x2550,
        ibm: 0xcd,
    }, // BOX DRAWINGS DOUBLE HORIZONTAL
    UniMapping {
        uni: 0x2551,
        ibm: 0xba,
    }, // BOX DRAWINGS DOUBLE VERTICAL
    UniMapping {
        uni: 0x2552,
        ibm: 0xd5,
    }, // BOX DRAWINGS DOWN SINGLE AND RIGHT DOUBLE
    UniMapping {
        uni: 0x2553,
        ibm: 0xd6,
    }, // BOX DRAWINGS DOWN DOUBLE AND RIGHT SINGLE
    UniMapping {
        uni: 0x2554,
        ibm: 0xc9,
    }, // BOX DRAWINGS DOUBLE DOWN AND RIGHT
    UniMapping {
        uni: 0x2555,
        ibm: 0xb8,
    }, // BOX DRAWINGS DOWN SINGLE AND LEFT DOUBLE
    UniMapping {
        uni: 0x2556,
        ibm: 0xb7,
    }, // BOX DRAWINGS DOWN DOUBLE AND LEFT SINGLE
    UniMapping {
        uni: 0x2557,
        ibm: 0xbb,
    }, // BOW DRAWINGS DOUBLE DOWN AND LEFT
    UniMapping {
        uni: 0x2558,
        ibm: 0xd4,
    }, // BOX DRAWINGS UP SINGLE AND RIGHT DOUBLE
    UniMapping {
        uni: 0x2559,
        ibm: 0xd3,
    }, // BOX DRAWINGS UP DOUBLE AND RIGHT SINGLE
    UniMapping {
        uni: 0x255a,
        ibm: 0xc8,
    }, // BOX DRAWINGS DOUBLE UP AND RIGHT
    UniMapping {
        uni: 0x255b,
        ibm: 0xbe,
    }, // BOX DRAWINGS UP SINGLE AND LEFT DOUBLE
    UniMapping {
        uni: 0x255c,
        ibm: 0xbd,
    }, // BOX DRAWINGS UP DOUBLE AND LEFT SINGLE
    UniMapping {
        uni: 0x255d,
        ibm: 0xbc,
    }, // BOX DRAWINGS DOUBLE UP AND LEFT
    UniMapping {
        uni: 0x255e,
        ibm: 0xc6,
    }, // BOX DRAWINGS VERTICAL SINGLE AND RIGHT DOUBLE
    UniMapping {
        uni: 0x255f,
        ibm: 0xc7,
    }, // BOX DRAWINGS VERTICAL DOUBLE AND RIGHT SINGLE
    UniMapping {
        uni: 0x2560,
        ibm: 0xcc,
    }, // BOX DRAWINGS DOUBLE VERTICAL AND RIGHT
    UniMapping {
        uni: 0x2561,
        ibm: 0xb4,
    }, // BOX DRAWINGS VERTICAL SINGLE AND LEFT DOUBLE
    UniMapping {
        uni: 0x2562,
        ibm: 0xb5,
    }, // BOX DRAWINGS VERTICAL DOUBLE AND LEFT SINGLE
    UniMapping {
        uni: 0x2563,
        ibm: 0xb9,
    }, // BOX DRAWINGS DOUBLE VERTICAL AND LEFT
    UniMapping {
        uni: 0x2564,
        ibm: 0xd1,
    }, // BOX DRAWINGS DOWN SINGLE AND HORIZONTAL DOUBLE
    UniMapping {
        uni: 0x2565,
        ibm: 0xd2,
    }, // BOX DRAWINGS DOWN DOUBLE AND HORIZONTAL SINGLE
    UniMapping {
        uni: 0x2566,
        ibm: 0xcb,
    }, // BOX DRAWINGS DOUBLE DOWN AND HORIZONTAL
    UniMapping {
        uni: 0x2567,
        ibm: 0xcf,
    }, // BOX DRAWINGS UP SINGLE AND HORIZONTAL DOUBLE
    UniMapping {
        uni: 0x2568,
        ibm: 0xd0,
    }, // BOX DRAWINGS UP DOUBLE AND HORIZONTAL SINGLE
    UniMapping {
        uni: 0x2569,
        ibm: 0xca,
    }, // BOX DRAWINGS DOUBLE UP AND HORIZONTAL
    UniMapping {
        uni: 0x256a,
        ibm: 0xd8,
    }, // BOX DRAWINGS VERTICAL SINGLE AND HORIZONTAL DOUBLE
    UniMapping {
        uni: 0x256b,
        ibm: 0xd7,
    }, // BOX DRAWINGS VERTICAL DOUBLE AND HORIZONTAL SINGLE
    UniMapping {
        uni: 0x256c,
        ibm: 0xce,
    }, // BOX DRAWINGS DOUBLE VERTICAL AND HORIZONTAL
    UniMapping {
        uni: 0x2580,
        ibm: 0xdf,
    }, // UPPER HALF BLOCK
    UniMapping {
        uni: 0x2584,
        ibm: 0xdc,
    }, // LOWER HALF BLOCK
    UniMapping {
        uni: 0x2588,
        ibm: 0xdb,
    }, // FULL BLOCK
    UniMapping {
        uni: 0x258c,
        ibm: 0xdd,
    }, // LEFT HALF BLOCK
    UniMapping {
        uni: 0x2590,
        ibm: 0xde,
    }, // RIGHT HALF BLOCK
    UniMapping {
        uni: 0x2591,
        ibm: 0xb0,
    }, // LIGHT SHADE
    UniMapping {
        uni: 0x2592,
        ibm: 0xb1,
    }, // MEDIUM SHADE
    UniMapping {
        uni: 0x2593,
        ibm: 0xb2,
    }, // DARK SHADE
    UniMapping {
        uni: 0x25a0,
        ibm: 0xfe,
    }, // BLACK SQUARE
    UniMapping {
        uni: 0x25ac,
        ibm: 0x16,
    }, // BLACK RECTANGLE
    UniMapping {
        uni: 0x25b2,
        ibm: 0x1e,
    }, // BLACK UP-POINTING TRIANGLE
    UniMapping {
        uni: 0x25ba,
        ibm: 0x10,
    }, // BLACK RIGHT-POINTING POINTER
    UniMapping {
        uni: 0x25bc,
        ibm: 0x1f,
    }, // BLACK DOWN-POINTING TRIANGLE
    UniMapping {
        uni: 0x25c4,
        ibm: 0x11,
    }, // BLACK LEFT-POINTING POINTER
    UniMapping {
        uni: 0x25c6,
        ibm: 0x04,
    }, // BLACK DIAMOND
    UniMapping {
        uni: 0x25cb,
        ibm: 0x09,
    }, // WHITE CIRCLE
    UniMapping {
        uni: 0x25d8,
        ibm: 0x08,
    }, // INVERSE BULLET
    UniMapping {
        uni: 0x25d9,
        ibm: 0x0a,
    }, // INVERSE WHITE CIRCLE
    UniMapping {
        uni: 0x263a,
        ibm: 0x01,
    }, // WHITE SMILING FACE
    UniMapping {
        uni: 0x263b,
        ibm: 0x02,
    }, // BLACK SMILING FACE
    UniMapping {
        uni: 0x263c,
        ibm: 0x0f,
    }, // WHITE SUN WITH RAYS
    UniMapping {
        uni: 0x2640,
        ibm: 0x0c,
    }, // FEMALE SIGN
    UniMapping {
        uni: 0x2642,
        ibm: 0x0b,
    }, // MALE SIGN
    UniMapping {
        uni: 0x2660,
        ibm: 0x06,
    }, // BLACK SPADE SUIT
    UniMapping {
        uni: 0x2663,
        ibm: 0x05,
    }, // BLACK CLUB SUIT
    UniMapping {
        uni: 0x2665,
        ibm: 0x03,
    }, // BLACK HEART SUIT
    UniMapping {
        uni: 0x2666,
        ibm: 0x04,
    }, // BLACK DIAMOND SUIT
    UniMapping {
        uni: 0x266a,
        ibm: 0x0d,
    }, // EIGHTH NOTE
    UniMapping {
        uni: 0x266b,
        ibm: 0x0e,
    }, // BEAMED EIGHTH NOTES
];

/// `replacements[]`: approximations, with the quality of each.
static REPLACEMENTS: [Replacement; 61] = [
    Replacement {
        uni: 0x00af,
        ibm: 0x2d,
        quality: 3,
    }, // MACRON -> -
    Replacement {
        uni: 0x221f,
        ibm: 0xc0,
        quality: 3,
    }, // RIGHT ANGLE -> light up and right
    Replacement {
        uni: 0x222a,
        ibm: 0x55,
        quality: 3,
    }, // UNION -> U
    Replacement {
        uni: 0x223c,
        ibm: 0x7e,
        quality: 3,
    }, // TILDE OPERATOR -> ~
    Replacement {
        uni: 0x2308,
        ibm: 0xda,
        quality: 3,
    }, // LEFT CEILING -> light down and right
    Replacement {
        uni: 0x2309,
        ibm: 0xbf,
        quality: 3,
    }, // RIGHT CEILING -> light down and left
    Replacement {
        uni: 0x230a,
        ibm: 0xc0,
        quality: 3,
    }, // LEFT FLOOR -> light up and right
    Replacement {
        uni: 0x230b,
        ibm: 0xd9,
        quality: 3,
    }, // RIGHT FLOOR -> light up and left
    Replacement {
        uni: 0x2329,
        ibm: 0x3c,
        quality: 3,
    }, // LEFT-POINTING ANGLE BRACKET -> <
    Replacement {
        uni: 0x232a,
        ibm: 0x3e,
        quality: 3,
    }, // RIGHT-POINTING ANGLE BRACKET -> >
    Replacement {
        uni: 0x2500,
        ibm: 0x2d,
        quality: 3,
    }, // scan 5 -> -
    Replacement {
        uni: 0x23bd,
        ibm: 0x5f,
        quality: 3,
    }, // scan 9 -> _
    Replacement {
        uni: _e00bU,
        ibm: 0x7b,
        quality: 3,
    }, // braceleftmid -> {
    Replacement {
        uni: _e00cU,
        ibm: 0x7d,
        quality: 3,
    }, // bracerightmid -> }
    Replacement {
        uni: _e00fU,
        ibm: 0xd9,
        quality: 3,
    }, // mirrored not sign? -> light up and left
    Replacement {
        uni: 0x00d7,
        ibm: 0x78,
        quality: 2,
    }, // MULTIPLICATION SIGN -> x
    Replacement {
        uni: 0x00d8,
        ibm: 0xe9,
        quality: 2,
    }, // LATIN CAPITAL LETTER O WITH STROKE -> Theta
    Replacement {
        uni: 0x00f8,
        ibm: 0xed,
        quality: 2,
    }, // LATIN SMALL LETTER O WITH STROKE -> phi
    Replacement {
        uni: 0x03a0,
        ibm: 0xe3,
        quality: 2,
    }, // GREEK CAPITAL LETTER PI -> pi
    Replacement {
        uni: 0x03a5,
        ibm: 0x59,
        quality: 2,
    }, // GREEK CAPITAL LETTER UPSILON -> Y
    Replacement {
        uni: 0x03b3,
        ibm: 0x59,
        quality: 2,
    }, // GREEK SMALL LETTER GAMMA -> Y
    Replacement {
        uni: 0x03b8,
        ibm: 0xe9,
        quality: 2,
    }, // GREEK SMALL LETTER THETA -> Theta
    Replacement {
        uni: 0x03bd,
        ibm: 0x76,
        quality: 2,
    }, // GREEK SMALL LETTER NU -> v
    Replacement {
        uni: 0x03c9,
        ibm: 0x77,
        quality: 2,
    }, // GREEK SMALL LETTER OMEGA -> w
    Replacement {
        uni: 0x20ac,
        ibm: 0x45,
        quality: 2,
    }, // EURO SIGN -> E
    Replacement {
        uni: 0x23bb,
        ibm: 0x2d,
        quality: 2,
    }, // scan 3 -> -
    Replacement {
        uni: 0x23bc,
        ibm: 0x2d,
        quality: 2,
    }, // scan 7 -> -
    Replacement {
        uni: _e007U,
        ibm: 0xda,
        quality: 2,
    }, // bracelefttp -> light down and right
    Replacement {
        uni: _e008U,
        ibm: 0xc0,
        quality: 2,
    }, // braceleftbt -> light up and right
    Replacement {
        uni: _e009U,
        ibm: 0xbf,
        quality: 2,
    }, // bracerighttp -> light down and left
    Replacement {
        uni: _e00aU,
        ibm: 0xd9,
        quality: 2,
    }, // bracerighrbt -> light up and left
    Replacement {
        uni: _e00dU,
        ibm: 0x3c,
        quality: 2,
    }, // inverted angle? -> <
    Replacement {
        uni: _e00eU,
        ibm: 0x3c,
        quality: 2,
    }, // angle? -> <
    Replacement {
        uni: _e00fU,
        ibm: 0xd9,
        quality: 2,
    }, // mirrored not sign? -> light up and left
    Replacement {
        uni: 0x00a9,
        ibm: 0x63,
        quality: 1,
    }, // COPYRIGHT SIGN -> c
    Replacement {
        uni: 0x00ae,
        ibm: 0x72,
        quality: 1,
    }, // REGISTERED SIGN -> r
    Replacement {
        uni: 0x00b3,
        ibm: 0x33,
        quality: 1,
    }, // SUPERSCRIPT THREE -> 3
    Replacement {
        uni: 0x00b9,
        ibm: 0x39,
        quality: 1,
    }, // SUPERSCRIPT ONE -> 1
    Replacement {
        uni: 0x00c0,
        ibm: 0x41,
        quality: 1,
    }, // LATIN CAPITAL LETTER A WITH GRAVE -> A
    Replacement {
        uni: 0x00c1,
        ibm: 0x41,
        quality: 1,
    }, // LATIN CAPITAL LETTER A WITH ACUTE -> A
    Replacement {
        uni: 0x00c2,
        ibm: 0x41,
        quality: 1,
    }, // LATIN CAPITAL LETTER A WITH CIRCUMFLEX -> A
    Replacement {
        uni: 0x00c3,
        ibm: 0x41,
        quality: 1,
    }, // LATIN CAPITAL LETTER A WITH TILDE -> A
    Replacement {
        uni: 0x00c8,
        ibm: 0x45,
        quality: 1,
    }, // LATIN CAPITAL LETTER E WITH GRAVE -> E
    Replacement {
        uni: 0x00ca,
        ibm: 0x45,
        quality: 1,
    }, // LATIN CAPITAL LETTER E WITH CIRCUMFLEX -> E
    Replacement {
        uni: 0x00cb,
        ibm: 0x45,
        quality: 1,
    }, // LATIN CAPITAL LETTER E WITH DIAERESIS -> E
    Replacement {
        uni: 0x00cc,
        ibm: 0x49,
        quality: 1,
    }, // LATIN CAPITAL LETTER I WITH GRAVE -> I
    Replacement {
        uni: 0x00cd,
        ibm: 0x49,
        quality: 1,
    }, // LATIN CAPITAL LETTER I WITH ACUTE -> I
    Replacement {
        uni: 0x00ce,
        ibm: 0x49,
        quality: 1,
    }, // LATIN CAPITAL LETTER I WITH CIRCUMFLEX -> I
    Replacement {
        uni: 0x00cf,
        ibm: 0x49,
        quality: 1,
    }, // LATIN CAPITAL LETTER I WITH DIAERESIS -> I
    Replacement {
        uni: 0x00d0,
        ibm: 0x44,
        quality: 1,
    }, // LATIN CAPITAL LETTER ETH -> D
    Replacement {
        uni: 0x00d2,
        ibm: 0x4f,
        quality: 1,
    }, // LATIN CAPITAL LETTER O WITH GRAVE -> O
    Replacement {
        uni: 0x00d3,
        ibm: 0x4f,
        quality: 1,
    }, // LATIN CAPITAL LETTER O WITH ACUTE -> O
    Replacement {
        uni: 0x00d4,
        ibm: 0x4f,
        quality: 1,
    }, // LATIN CAPITAL LETTER O WITH CIRCUMFLEX -> O
    Replacement {
        uni: 0x00d5,
        ibm: 0x4f,
        quality: 1,
    }, // LATIN CAPITAL LETTER O WITH TILDE -> O
    Replacement {
        uni: 0x00d9,
        ibm: 0x55,
        quality: 1,
    }, // LATIN CAPITAL LETTER U WITH GRAVE -> U
    Replacement {
        uni: 0x00da,
        ibm: 0x55,
        quality: 1,
    }, // LATIN CAPITAL LETTER U WITH ACUTE -> U
    Replacement {
        uni: 0x00db,
        ibm: 0x55,
        quality: 1,
    }, // LATIN CAPITAL LETTER U WITH CIRCUMFLEX -> U
    Replacement {
        uni: 0x00dd,
        ibm: 0x59,
        quality: 1,
    }, // LATIN CAPITAL LETTER Y WITH ACUTE -> Y
    Replacement {
        uni: 0x00e3,
        ibm: 0x61,
        quality: 1,
    }, // LATIN SMALL LETTER A WITH TILDE -> a
    Replacement {
        uni: 0x00f5,
        ibm: 0x6f,
        quality: 1,
    }, // LATIN SMALL LETTER O WITH TILDE -> o
    Replacement {
        uni: 0x00fd,
        ibm: 0x79,
        quality: 1,
    }, // LATIN SMALL LETTER Y WITH ACUTE -> y
];

/// `pcdisplay_mapchar`: the CP437 index of Unicode character `uni` in `*index`; returns the
/// quality of the mapping (5 exact, lower for a look-alike, 0 for none).
pub fn pcdisplay_mapchar(uni: i32, index: &mut u32) -> i32 {
    if uni < 128 {
        *index = uni as u32;
        return 5;
    } else if uni < 256 && ISOMAPPINGS[(uni - 128) as usize] != NOTPRINTABLE {
        *index = u32::from(ISOMAPPINGS[(uni - 128) as usize]);
        return 5;
    }

    if let Some(m) = UNIMAPPINGS.iter().find(|m| uni == i32::from(m.uni)) {
        *index = u32::from(m.ibm);
        return 5;
    }

    if let Some(r) = REPLACEMENTS.iter().find(|r| uni == i32::from(r.uni)) {
        *index = u32::from(r.ibm);
        return r.quality;
    }

    *index = u32::from(NOTPRINTABLE);
    0
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    fn map(uni: i32) -> (u32, i32) {
        let mut i = 0;
        let q = pcdisplay_mapchar(uni, &mut i);
        (i, q)
    }

    #[test]
    fn ascii_iso_unicode_replacements_and_misses() {
        assert_eq!(map(i32::from(b'A')), (0x41, 5));
        assert_eq!(map(0x00e9), (0x82, 5)); // e acute
        assert_eq!(map(0x00a4), (4, 0)); // currency sign: not printable, no replacement
        assert_eq!(map(0x00a9), (0x63, 1)); // copyright sign -> c
        assert_eq!(map(0x2500), (0xc4, 5)); // box drawing: the exact glyph wins
        assert_eq!(map(0x20ac), (0x45, 2)); // euro -> E
        assert_eq!(map(i32::from(_e00bU)), (0x7b, 3));
        assert_eq!(map(0x1f600), (u32::from(NOTPRINTABLE), 0));
        assert_eq!(map(0x0085), (u32::from(CONTROL), 5));
    }
}
/* </TESTS> */
