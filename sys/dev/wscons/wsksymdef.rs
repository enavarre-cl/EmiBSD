/* <LICENSES> */
/*-
 * Copyright (c) 1997 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Juergen Hannken-Illjes.
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
//! `<dev/wscons/wsksymdef.h>`: the keysyms of wscons and the keyboard layout codes.
//!
//! Upstream: sys/dev/wscons/wsksymdef.h @ 3ce1f3f79392
//!
//! A keysym is a 16-bit Unicode code point, with the special symbols in the private area
//! (0xe000 to 0xf8ff): the modifiers (`KS_Shift_L`, ...), the keypad, the function keys, the
//! commands (`KS_Cmd_Screen1`, ...) and `KS_voidSymbol`. A keyboard layout is a `kbd_t`: an
//! 8-bit encoding (`KB_US`, `KB_DE`, ...) with a 24-bit variant (`KB_NODEAD`, `KB_APPLE`, ...).
//!
//! ## Deviations
//! - Every `KS_*` keysym is a `u16` (a `keysym_t`); the `KS_GROUP_*` codes and
//!   `KS_NUMKEYCODES`, which the C uses as `int`s, and the `KB_*` layouts (`kbd_t`) are `u32`.
//! - The function-like macros are the lowercase `const fn`s [`ks_keycode`], [`ks_group`],
//!   [`ks_value`], [`kb_encoding`] and [`kb_variant`]; they take the keysym or layout as a
//!   `u32`, as C's integer promotion would.
//! - `KB_ENCTAB` and `KB_VARTAB`, the initialisers of userland's name tables, are constant
//!   arrays of `(value, name)` pairs.

#![allow(non_upper_case_globals)] // the keysym names (`KS_BackSpace`), verbatim, for grep-ability

/// `KS_BackSpace`.
pub const KS_BackSpace: u16 = 0x08;
/// `KS_Tab`.
pub const KS_Tab: u16 = 0x09;
/// `KS_Linefeed`.
pub const KS_Linefeed: u16 = 0x0a;
/// `KS_Clear`.
pub const KS_Clear: u16 = 0x0b;
/// `KS_Return`.
pub const KS_Return: u16 = 0x0d;
/// `KS_Escape`.
pub const KS_Escape: u16 = 0x1b;
/// `KS_space`.
pub const KS_space: u16 = 0x20;
/// `KS_exclam`.
pub const KS_exclam: u16 = 0x21;
/// `KS_quotedbl`.
pub const KS_quotedbl: u16 = 0x22;
/// `KS_numbersign`.
pub const KS_numbersign: u16 = 0x23;
/// `KS_dollar`.
pub const KS_dollar: u16 = 0x24;
/// `KS_percent`.
pub const KS_percent: u16 = 0x25;
/// `KS_ampersand`.
pub const KS_ampersand: u16 = 0x26;
/// `KS_apostrophe`.
pub const KS_apostrophe: u16 = 0x27;
/// `KS_parenleft`.
pub const KS_parenleft: u16 = 0x28;
/// `KS_parenright`.
pub const KS_parenright: u16 = 0x29;
/// `KS_asterisk`.
pub const KS_asterisk: u16 = 0x2a;
/// `KS_plus`.
pub const KS_plus: u16 = 0x2b;
/// `KS_comma`.
pub const KS_comma: u16 = 0x2c;
/// `KS_minus`.
pub const KS_minus: u16 = 0x2d;
/// `KS_period`.
pub const KS_period: u16 = 0x2e;
/// `KS_slash`.
pub const KS_slash: u16 = 0x2f;
/// `KS_0`.
pub const KS_0: u16 = 0x30;
/// `KS_1`.
pub const KS_1: u16 = 0x31;
/// `KS_2`.
pub const KS_2: u16 = 0x32;
/// `KS_3`.
pub const KS_3: u16 = 0x33;
/// `KS_4`.
pub const KS_4: u16 = 0x34;
/// `KS_5`.
pub const KS_5: u16 = 0x35;
/// `KS_6`.
pub const KS_6: u16 = 0x36;
/// `KS_7`.
pub const KS_7: u16 = 0x37;
/// `KS_8`.
pub const KS_8: u16 = 0x38;
/// `KS_9`.
pub const KS_9: u16 = 0x39;
/// `KS_colon`.
pub const KS_colon: u16 = 0x3a;
/// `KS_semicolon`.
pub const KS_semicolon: u16 = 0x3b;
/// `KS_less`.
pub const KS_less: u16 = 0x3c;
/// `KS_equal`.
pub const KS_equal: u16 = 0x3d;
/// `KS_greater`.
pub const KS_greater: u16 = 0x3e;
/// `KS_question`.
pub const KS_question: u16 = 0x3f;
/// `KS_at`.
pub const KS_at: u16 = 0x40;
/// `KS_A`.
pub const KS_A: u16 = 0x41;
/// `KS_B`.
pub const KS_B: u16 = 0x42;
/// `KS_C`.
pub const KS_C: u16 = 0x43;
/// `KS_D`.
pub const KS_D: u16 = 0x44;
/// `KS_E`.
pub const KS_E: u16 = 0x45;
/// `KS_F`.
pub const KS_F: u16 = 0x46;
/// `KS_G`.
pub const KS_G: u16 = 0x47;
/// `KS_H`.
pub const KS_H: u16 = 0x48;
/// `KS_I`.
pub const KS_I: u16 = 0x49;
/// `KS_J`.
pub const KS_J: u16 = 0x4a;
/// `KS_K`.
pub const KS_K: u16 = 0x4b;
/// `KS_L`.
pub const KS_L: u16 = 0x4c;
/// `KS_M`.
pub const KS_M: u16 = 0x4d;
/// `KS_N`.
pub const KS_N: u16 = 0x4e;
/// `KS_O`.
pub const KS_O: u16 = 0x4f;
/// `KS_P`.
pub const KS_P: u16 = 0x50;
/// `KS_Q`.
pub const KS_Q: u16 = 0x51;
/// `KS_R`.
pub const KS_R: u16 = 0x52;
/// `KS_S`.
pub const KS_S: u16 = 0x53;
/// `KS_T`.
pub const KS_T: u16 = 0x54;
/// `KS_U`.
pub const KS_U: u16 = 0x55;
/// `KS_V`.
pub const KS_V: u16 = 0x56;
/// `KS_W`.
pub const KS_W: u16 = 0x57;
/// `KS_X`.
pub const KS_X: u16 = 0x58;
/// `KS_Y`.
pub const KS_Y: u16 = 0x59;
/// `KS_Z`.
pub const KS_Z: u16 = 0x5a;
/// `KS_bracketleft`.
pub const KS_bracketleft: u16 = 0x5b;
/// `KS_backslash`.
pub const KS_backslash: u16 = 0x5c;
/// `KS_bracketright`.
pub const KS_bracketright: u16 = 0x5d;
/// `KS_asciicircum`.
pub const KS_asciicircum: u16 = 0x5e;
/// `KS_underscore`.
pub const KS_underscore: u16 = 0x5f;
/// `KS_grave`.
pub const KS_grave: u16 = 0x60;
/// `KS_a`.
pub const KS_a: u16 = 0x61;
/// `KS_b`.
pub const KS_b: u16 = 0x62;
/// `KS_c`.
pub const KS_c: u16 = 0x63;
/// `KS_d`.
pub const KS_d: u16 = 0x64;
/// `KS_e`.
pub const KS_e: u16 = 0x65;
/// `KS_f`.
pub const KS_f: u16 = 0x66;
/// `KS_g`.
pub const KS_g: u16 = 0x67;
/// `KS_h`.
pub const KS_h: u16 = 0x68;
/// `KS_i`.
pub const KS_i: u16 = 0x69;
/// `KS_j`.
pub const KS_j: u16 = 0x6a;
/// `KS_k`.
pub const KS_k: u16 = 0x6b;
/// `KS_l`.
pub const KS_l: u16 = 0x6c;
/// `KS_m`.
pub const KS_m: u16 = 0x6d;
/// `KS_n`.
pub const KS_n: u16 = 0x6e;
/// `KS_o`.
pub const KS_o: u16 = 0x6f;
/// `KS_p`.
pub const KS_p: u16 = 0x70;
/// `KS_q`.
pub const KS_q: u16 = 0x71;
/// `KS_r`.
pub const KS_r: u16 = 0x72;
/// `KS_s`.
pub const KS_s: u16 = 0x73;
/// `KS_t`.
pub const KS_t: u16 = 0x74;
/// `KS_u`.
pub const KS_u: u16 = 0x75;
/// `KS_v`.
pub const KS_v: u16 = 0x76;
/// `KS_w`.
pub const KS_w: u16 = 0x77;
/// `KS_x`.
pub const KS_x: u16 = 0x78;
/// `KS_y`.
pub const KS_y: u16 = 0x79;
/// `KS_z`.
pub const KS_z: u16 = 0x7a;
/// `KS_braceleft`.
pub const KS_braceleft: u16 = 0x7b;
/// `KS_bar`.
pub const KS_bar: u16 = 0x7c;
/// `KS_braceright`.
pub const KS_braceright: u16 = 0x7d;
/// `KS_asciitilde`.
pub const KS_asciitilde: u16 = 0x7e;
/// `KS_Delete`.
pub const KS_Delete: u16 = 0x7f;
/// `KS_nobreakspace`.
pub const KS_nobreakspace: u16 = 0xa0;
/// `KS_exclamdown`.
pub const KS_exclamdown: u16 = 0xa1;
/// `KS_cent`.
pub const KS_cent: u16 = 0xa2;
/// `KS_sterling`.
pub const KS_sterling: u16 = 0xa3;
/// `KS_currency`.
pub const KS_currency: u16 = 0xa4;
/// `KS_yen`.
pub const KS_yen: u16 = 0xa5;
/// `KS_brokenbar`.
pub const KS_brokenbar: u16 = 0xa6;
/// `KS_section`.
pub const KS_section: u16 = 0xa7;
/// `KS_diaeresis`.
pub const KS_diaeresis: u16 = 0xa8;
/// `KS_copyright`.
pub const KS_copyright: u16 = 0xa9;
/// `KS_ordfeminine`.
pub const KS_ordfeminine: u16 = 0xaa;
/// `KS_guillemotleft`.
pub const KS_guillemotleft: u16 = 0xab;
/// `KS_notsign`.
pub const KS_notsign: u16 = 0xac;
/// `KS_hyphen`.
pub const KS_hyphen: u16 = 0xad;
/// `KS_registered`.
pub const KS_registered: u16 = 0xae;
/// `KS_macron`.
pub const KS_macron: u16 = 0xaf;
/// `KS_degree`.
pub const KS_degree: u16 = 0xb0;
/// `KS_plusminus`.
pub const KS_plusminus: u16 = 0xb1;
/// `KS_twosuperior`.
pub const KS_twosuperior: u16 = 0xb2;
/// `KS_threesuperior`.
pub const KS_threesuperior: u16 = 0xb3;
/// `KS_acute`.
pub const KS_acute: u16 = 0xb4;
/// `KS_mu`.
pub const KS_mu: u16 = 0xb5;
/// `KS_paragraph`.
pub const KS_paragraph: u16 = 0xb6;
/// `KS_periodcentered`.
pub const KS_periodcentered: u16 = 0xb7;
/// `KS_cedilla`.
pub const KS_cedilla: u16 = 0xb8;
/// `KS_onesuperior`.
pub const KS_onesuperior: u16 = 0xb9;
/// `KS_masculine`.
pub const KS_masculine: u16 = 0xba;
/// `KS_guillemotright`.
pub const KS_guillemotright: u16 = 0xbb;
/// `KS_onequarter`.
pub const KS_onequarter: u16 = 0xbc;
/// `KS_onehalf`.
pub const KS_onehalf: u16 = 0xbd;
/// `KS_threequarters`.
pub const KS_threequarters: u16 = 0xbe;
/// `KS_questiondown`.
pub const KS_questiondown: u16 = 0xbf;
/// `KS_Agrave`.
pub const KS_Agrave: u16 = 0xc0;
/// `KS_Aacute`.
pub const KS_Aacute: u16 = 0xc1;
/// `KS_Acircumflex`.
pub const KS_Acircumflex: u16 = 0xc2;
/// `KS_Atilde`.
pub const KS_Atilde: u16 = 0xc3;
/// `KS_Adiaeresis`.
pub const KS_Adiaeresis: u16 = 0xc4;
/// `KS_Aring`.
pub const KS_Aring: u16 = 0xc5;
/// `KS_AE`.
pub const KS_AE: u16 = 0xc6;
/// `KS_Ccedilla`.
pub const KS_Ccedilla: u16 = 0xc7;
/// `KS_Egrave`.
pub const KS_Egrave: u16 = 0xc8;
/// `KS_Eacute`.
pub const KS_Eacute: u16 = 0xc9;
/// `KS_Ecircumflex`.
pub const KS_Ecircumflex: u16 = 0xca;
/// `KS_Ediaeresis`.
pub const KS_Ediaeresis: u16 = 0xcb;
/// `KS_Igrave`.
pub const KS_Igrave: u16 = 0xcc;
/// `KS_Iacute`.
pub const KS_Iacute: u16 = 0xcd;
/// `KS_Icircumflex`.
pub const KS_Icircumflex: u16 = 0xce;
/// `KS_Idiaeresis`.
pub const KS_Idiaeresis: u16 = 0xcf;
/// `KS_ETH`.
pub const KS_ETH: u16 = 0xd0;
/// `KS_Ntilde`.
pub const KS_Ntilde: u16 = 0xd1;
/// `KS_Ograve`.
pub const KS_Ograve: u16 = 0xd2;
/// `KS_Oacute`.
pub const KS_Oacute: u16 = 0xd3;
/// `KS_Ocircumflex`.
pub const KS_Ocircumflex: u16 = 0xd4;
/// `KS_Otilde`.
pub const KS_Otilde: u16 = 0xd5;
/// `KS_Odiaeresis`.
pub const KS_Odiaeresis: u16 = 0xd6;
/// `KS_multiply`.
pub const KS_multiply: u16 = 0xd7;
/// `KS_Ooblique`.
pub const KS_Ooblique: u16 = 0xd8;
/// `KS_Ugrave`.
pub const KS_Ugrave: u16 = 0xd9;
/// `KS_Uacute`.
pub const KS_Uacute: u16 = 0xda;
/// `KS_Ucircumflex`.
pub const KS_Ucircumflex: u16 = 0xdb;
/// `KS_Udiaeresis`.
pub const KS_Udiaeresis: u16 = 0xdc;
/// `KS_Yacute`.
pub const KS_Yacute: u16 = 0xdd;
/// `KS_THORN`.
pub const KS_THORN: u16 = 0xde;
/// `KS_ssharp`.
pub const KS_ssharp: u16 = 0xdf;
/// `KS_agrave`.
pub const KS_agrave: u16 = 0xe0;
/// `KS_aacute`.
pub const KS_aacute: u16 = 0xe1;
/// `KS_acircumflex`.
pub const KS_acircumflex: u16 = 0xe2;
/// `KS_atilde`.
pub const KS_atilde: u16 = 0xe3;
/// `KS_adiaeresis`.
pub const KS_adiaeresis: u16 = 0xe4;
/// `KS_aring`.
pub const KS_aring: u16 = 0xe5;
/// `KS_ae`.
pub const KS_ae: u16 = 0xe6;
/// `KS_ccedilla`.
pub const KS_ccedilla: u16 = 0xe7;
/// `KS_egrave`.
pub const KS_egrave: u16 = 0xe8;
/// `KS_eacute`.
pub const KS_eacute: u16 = 0xe9;
/// `KS_ecircumflex`.
pub const KS_ecircumflex: u16 = 0xea;
/// `KS_ediaeresis`.
pub const KS_ediaeresis: u16 = 0xeb;
/// `KS_igrave`.
pub const KS_igrave: u16 = 0xec;
/// `KS_iacute`.
pub const KS_iacute: u16 = 0xed;
/// `KS_icircumflex`.
pub const KS_icircumflex: u16 = 0xee;
/// `KS_idiaeresis`.
pub const KS_idiaeresis: u16 = 0xef;
/// `KS_eth`.
pub const KS_eth: u16 = 0xf0;
/// `KS_ntilde`.
pub const KS_ntilde: u16 = 0xf1;
/// `KS_ograve`.
pub const KS_ograve: u16 = 0xf2;
/// `KS_oacute`.
pub const KS_oacute: u16 = 0xf3;
/// `KS_ocircumflex`.
pub const KS_ocircumflex: u16 = 0xf4;
/// `KS_otilde`.
pub const KS_otilde: u16 = 0xf5;
/// `KS_odiaeresis`.
pub const KS_odiaeresis: u16 = 0xf6;
/// `KS_division`.
pub const KS_division: u16 = 0xf7;
/// `KS_oslash`.
pub const KS_oslash: u16 = 0xf8;
/// `KS_ugrave`.
pub const KS_ugrave: u16 = 0xf9;
/// `KS_uacute`.
pub const KS_uacute: u16 = 0xfa;
/// `KS_ucircumflex`.
pub const KS_ucircumflex: u16 = 0xfb;
/// `KS_udiaeresis`.
pub const KS_udiaeresis: u16 = 0xfc;
/// `KS_yacute`.
pub const KS_yacute: u16 = 0xfd;
/// `KS_thorn`.
pub const KS_thorn: u16 = 0xfe;
/// `KS_ydiaeresis`.
pub const KS_ydiaeresis: u16 = 0xff;
/// `KS_Odoubleacute`.
pub const KS_Odoubleacute: u16 = 0x0150;
/// `KS_odoubleacute`.
pub const KS_odoubleacute: u16 = 0x0151;
/// `KS_Udoubleacute`.
pub const KS_Udoubleacute: u16 = 0x0170;
/// `KS_udoubleacute`.
pub const KS_udoubleacute: u16 = 0x0171;
/// `KS_dead_grave`.
pub const KS_dead_grave: u16 = 0x0300;
/// `KS_dead_acute`.
pub const KS_dead_acute: u16 = 0x0301;
/// `KS_dead_circumflex`.
pub const KS_dead_circumflex: u16 = 0x0302;
/// `KS_dead_tilde`.
pub const KS_dead_tilde: u16 = 0x0303;
/// `KS_dead_diaeresis`.
pub const KS_dead_diaeresis: u16 = 0x0308;
/// `KS_dead_abovering`.
pub const KS_dead_abovering: u16 = 0x030a;
/// `KS_dead_cedilla`.
pub const KS_dead_cedilla: u16 = 0x0327;
/// `KS_dead_caron`.
pub const KS_dead_caron: u16 = 0x0328;
/// `KS_Cyrillic_YO`.
pub const KS_Cyrillic_YO: u16 = 0x0401;
/// `KS_Cyrillic_YEUKR`.
pub const KS_Cyrillic_YEUKR: u16 = 0x0404;
/// `KS_Cyrillic_IUKR`.
pub const KS_Cyrillic_IUKR: u16 = 0x0406;
/// `KS_Cyrillic_YI`.
pub const KS_Cyrillic_YI: u16 = 0x0407;
/// `KS_Cyrillic_A`.
pub const KS_Cyrillic_A: u16 = 0x0410;
/// `KS_Cyrillic_BE`.
pub const KS_Cyrillic_BE: u16 = 0x0411;
/// `KS_Cyrillic_VE`.
pub const KS_Cyrillic_VE: u16 = 0x0412;
/// `KS_Cyrillic_GE`.
pub const KS_Cyrillic_GE: u16 = 0x0413;
/// `KS_Cyrillic_DE`.
pub const KS_Cyrillic_DE: u16 = 0x0414;
/// `KS_Cyrillic_IE`.
pub const KS_Cyrillic_IE: u16 = 0x0415;
/// `KS_Cyrillic_ZHE`.
pub const KS_Cyrillic_ZHE: u16 = 0x0416;
/// `KS_Cyrillic_ZE`.
pub const KS_Cyrillic_ZE: u16 = 0x0417;
/// `KS_Cyrillic_I`.
pub const KS_Cyrillic_I: u16 = 0x0418;
/// `KS_Cyrillic_ISHORT`.
pub const KS_Cyrillic_ISHORT: u16 = 0x0419;
/// `KS_Cyrillic_KA`.
pub const KS_Cyrillic_KA: u16 = 0x041a;
/// `KS_Cyrillic_EL`.
pub const KS_Cyrillic_EL: u16 = 0x041b;
/// `KS_Cyrillic_EM`.
pub const KS_Cyrillic_EM: u16 = 0x041c;
/// `KS_Cyrillic_EN`.
pub const KS_Cyrillic_EN: u16 = 0x041d;
/// `KS_Cyrillic_O`.
pub const KS_Cyrillic_O: u16 = 0x041e;
/// `KS_Cyrillic_PE`.
pub const KS_Cyrillic_PE: u16 = 0x041f;
/// `KS_Cyrillic_ER`.
pub const KS_Cyrillic_ER: u16 = 0x0420;
/// `KS_Cyrillic_ES`.
pub const KS_Cyrillic_ES: u16 = 0x0421;
/// `KS_Cyrillic_TE`.
pub const KS_Cyrillic_TE: u16 = 0x0422;
/// `KS_Cyrillic_U`.
pub const KS_Cyrillic_U: u16 = 0x0423;
/// `KS_Cyrillic_EF`.
pub const KS_Cyrillic_EF: u16 = 0x0424;
/// `KS_Cyrillic_HA`.
pub const KS_Cyrillic_HA: u16 = 0x0425;
/// `KS_Cyrillic_TSE`.
pub const KS_Cyrillic_TSE: u16 = 0x0426;
/// `KS_Cyrillic_CHE`.
pub const KS_Cyrillic_CHE: u16 = 0x0427;
/// `KS_Cyrillic_SHA`.
pub const KS_Cyrillic_SHA: u16 = 0x0428;
/// `KS_Cyrillic_SCHA`.
pub const KS_Cyrillic_SCHA: u16 = 0x0429;
/// `KS_Cyrillic_HSIGHN`.
pub const KS_Cyrillic_HSIGHN: u16 = 0x042a;
/// `KS_Cyrillic_YERU`.
pub const KS_Cyrillic_YERU: u16 = 0x042b;
/// `KS_Cyrillic_SSIGHN`.
pub const KS_Cyrillic_SSIGHN: u16 = 0x042c;
/// `KS_Cyrillic_E`.
pub const KS_Cyrillic_E: u16 = 0x042d;
/// `KS_Cyrillic_YU`.
pub const KS_Cyrillic_YU: u16 = 0x042e;
/// `KS_Cyrillic_YA`.
pub const KS_Cyrillic_YA: u16 = 0x042f;
/// `KS_Cyrillic_a`.
pub const KS_Cyrillic_a: u16 = 0x0430;
/// `KS_Cyrillic_be`.
pub const KS_Cyrillic_be: u16 = 0x0431;
/// `KS_Cyrillic_ve`.
pub const KS_Cyrillic_ve: u16 = 0x0432;
/// `KS_Cyrillic_ge`.
pub const KS_Cyrillic_ge: u16 = 0x0433;
/// `KS_Cyrillic_de`.
pub const KS_Cyrillic_de: u16 = 0x0434;
/// `KS_Cyrillic_ie`.
pub const KS_Cyrillic_ie: u16 = 0x0435;
/// `KS_Cyrillic_zhe`.
pub const KS_Cyrillic_zhe: u16 = 0x0436;
/// `KS_Cyrillic_ze`.
pub const KS_Cyrillic_ze: u16 = 0x0437;
/// `KS_Cyrillic_i`.
pub const KS_Cyrillic_i: u16 = 0x0438;
/// `KS_Cyrillic_ishort`.
pub const KS_Cyrillic_ishort: u16 = 0x0439;
/// `KS_Cyrillic_ka`.
pub const KS_Cyrillic_ka: u16 = 0x043a;
/// `KS_Cyrillic_el`.
pub const KS_Cyrillic_el: u16 = 0x043b;
/// `KS_Cyrillic_em`.
pub const KS_Cyrillic_em: u16 = 0x043c;
/// `KS_Cyrillic_en`.
pub const KS_Cyrillic_en: u16 = 0x043d;
/// `KS_Cyrillic_o`.
pub const KS_Cyrillic_o: u16 = 0x043e;
/// `KS_Cyrillic_pe`.
pub const KS_Cyrillic_pe: u16 = 0x043f;
/// `KS_Cyrillic_er`.
pub const KS_Cyrillic_er: u16 = 0x0440;
/// `KS_Cyrillic_es`.
pub const KS_Cyrillic_es: u16 = 0x0441;
/// `KS_Cyrillic_te`.
pub const KS_Cyrillic_te: u16 = 0x0442;
/// `KS_Cyrillic_u`.
pub const KS_Cyrillic_u: u16 = 0x0443;
/// `KS_Cyrillic_ef`.
pub const KS_Cyrillic_ef: u16 = 0x0444;
/// `KS_Cyrillic_ha`.
pub const KS_Cyrillic_ha: u16 = 0x0445;
/// `KS_Cyrillic_tse`.
pub const KS_Cyrillic_tse: u16 = 0x0446;
/// `KS_Cyrillic_che`.
pub const KS_Cyrillic_che: u16 = 0x0447;
/// `KS_Cyrillic_sha`.
pub const KS_Cyrillic_sha: u16 = 0x0448;
/// `KS_Cyrillic_scha`.
pub const KS_Cyrillic_scha: u16 = 0x0449;
/// `KS_Cyrillic_hsighn`.
pub const KS_Cyrillic_hsighn: u16 = 0x044a;
/// `KS_Cyrillic_yeru`.
pub const KS_Cyrillic_yeru: u16 = 0x044b;
/// `KS_Cyrillic_ssighn`.
pub const KS_Cyrillic_ssighn: u16 = 0x044c;
/// `KS_Cyrillic_e`.
pub const KS_Cyrillic_e: u16 = 0x044d;
/// `KS_Cyrillic_yu`.
pub const KS_Cyrillic_yu: u16 = 0x044e;
/// `KS_Cyrillic_ya`.
pub const KS_Cyrillic_ya: u16 = 0x044f;
/// `KS_Cyrillic_yo`.
pub const KS_Cyrillic_yo: u16 = 0x0451;
/// `KS_Cyrillic_yeukr`.
pub const KS_Cyrillic_yeukr: u16 = 0x0454;
/// `KS_Cyrillic_iukr`.
pub const KS_Cyrillic_iukr: u16 = 0x0456;
/// `KS_Cyrillic_yi`.
pub const KS_Cyrillic_yi: u16 = 0x0457;
/// `KS_Cyrillic_GHEUKR`.
pub const KS_Cyrillic_GHEUKR: u16 = 0x0490;
/// `KS_Cyrillic_gheukr`.
pub const KS_Cyrillic_gheukr: u16 = 0x0491;
/// `KS_L2_Abreve`.
pub const KS_L2_Abreve: u16 = 0x0102;
/// `KS_L2_abreve`.
pub const KS_L2_abreve: u16 = 0x0103;
/// `KS_L2_Aogonek`.
pub const KS_L2_Aogonek: u16 = 0x0104;
/// `KS_L2_aogonek`.
pub const KS_L2_aogonek: u16 = 0x0105;
/// `KS_L2_Cacute`.
pub const KS_L2_Cacute: u16 = 0x0106;
/// `KS_L2_cacute`.
pub const KS_L2_cacute: u16 = 0x0107;
/// `KS_L2_Ccaron`.
pub const KS_L2_Ccaron: u16 = 0x010c;
/// `KS_L2_ccaron`.
pub const KS_L2_ccaron: u16 = 0x010d;
/// `KS_L2_Dcaron`.
pub const KS_L2_Dcaron: u16 = 0x010e;
/// `KS_L2_dcaron`.
pub const KS_L2_dcaron: u16 = 0x010f;
/// `KS_L2_Dstroke`.
pub const KS_L2_Dstroke: u16 = 0x0110;
/// `KS_L2_dstroke`.
pub const KS_L2_dstroke: u16 = 0x0111;
/// `KS_L2_Eogonek`.
pub const KS_L2_Eogonek: u16 = 0x0118;
/// `KS_L2_eogonek`.
pub const KS_L2_eogonek: u16 = 0x0119;
/// `KS_L2_Ecaron`.
pub const KS_L2_Ecaron: u16 = 0x011a;
/// `KS_L2_ecaron`.
pub const KS_L2_ecaron: u16 = 0x011b;
/// `KS_L2_Lacute`.
pub const KS_L2_Lacute: u16 = 0x0139;
/// `KS_L2_lacute`.
pub const KS_L2_lacute: u16 = 0x013a;
/// `KS_L2_Lcaron`.
pub const KS_L2_Lcaron: u16 = 0x013d;
/// `KS_L2_lcaron`.
pub const KS_L2_lcaron: u16 = 0x013e;
/// `KS_L2_Lstroke`.
pub const KS_L2_Lstroke: u16 = 0x0141;
/// `KS_L2_lstroke`.
pub const KS_L2_lstroke: u16 = 0x0142;
/// `KS_L2_Nacute`.
pub const KS_L2_Nacute: u16 = 0x0143;
/// `KS_L2_nacute`.
pub const KS_L2_nacute: u16 = 0x0144;
/// `KS_L2_Ncaron`.
pub const KS_L2_Ncaron: u16 = 0x0147;
/// `KS_L2_Odoubleacute`.
pub const KS_L2_Odoubleacute: u16 = 0x0150;
/// `KS_L2_odoubleacute`.
pub const KS_L2_odoubleacute: u16 = 0x0151;
/// `KS_L2_Racute`.
pub const KS_L2_Racute: u16 = 0x0154;
/// `KS_L2_racute`.
pub const KS_L2_racute: u16 = 0x0155;
/// `KS_L2_Rcaron`.
pub const KS_L2_Rcaron: u16 = 0x0158;
/// `KS_L2_rcaron`.
pub const KS_L2_rcaron: u16 = 0x0159;
/// `KS_L2_Sacute`.
pub const KS_L2_Sacute: u16 = 0x015a;
/// `KS_L2_sacute`.
pub const KS_L2_sacute: u16 = 0x015b;
/// `KS_L2_Scedilla`.
pub const KS_L2_Scedilla: u16 = 0x015e;
/// `KS_L2_scedilla`.
pub const KS_L2_scedilla: u16 = 0x015f;
/// `KS_L2_Scaron`.
pub const KS_L2_Scaron: u16 = 0x0160;
/// `KS_L2_scaron`.
pub const KS_L2_scaron: u16 = 0x0161;
/// `KS_L2_Tcedilla`.
pub const KS_L2_Tcedilla: u16 = 0x0162;
/// `KS_L2_tcedilla`.
pub const KS_L2_tcedilla: u16 = 0x0163;
/// `KS_L2_Tcaron`.
pub const KS_L2_Tcaron: u16 = 0x0164;
/// `KS_L2_tcaron`.
pub const KS_L2_tcaron: u16 = 0x0165;
/// `KS_L2_Uring`.
pub const KS_L2_Uring: u16 = 0x016e;
/// `KS_L2_uring`.
pub const KS_L2_uring: u16 = 0x016f;
/// `KS_L2_Udoubleacute`.
pub const KS_L2_Udoubleacute: u16 = 0x0170;
/// `KS_L2_udoubleacute`.
pub const KS_L2_udoubleacute: u16 = 0x0171;
/// `KS_L2_Zacute`.
pub const KS_L2_Zacute: u16 = 0x0179;
/// `KS_L2_zacute`.
pub const KS_L2_zacute: u16 = 0x017a;
/// `KS_L2_Zdotabove`.
pub const KS_L2_Zdotabove: u16 = 0x017b;
/// `KS_L2_zdotabove`.
pub const KS_L2_zdotabove: u16 = 0x017c;
/// `KS_L2_Zcaron`.
pub const KS_L2_Zcaron: u16 = 0x017d;
/// `KS_L2_zcaron`.
pub const KS_L2_zcaron: u16 = 0x017e;
/// `KS_L2_caron`.
pub const KS_L2_caron: u16 = 0x02c7;
/// `KS_L2_breve`.
pub const KS_L2_breve: u16 = 0x02d8;
/// `KS_L2_dotabove`.
pub const KS_L2_dotabove: u16 = 0x02d9;
/// `KS_L2_ogonek`.
pub const KS_L2_ogonek: u16 = 0x02db;
/// `KS_L2_dblacute`.
pub const KS_L2_dblacute: u16 = 0x02dd;
/// `KS_L5_Gbreve`.
pub const KS_L5_Gbreve: u16 = 0x011e;
/// `KS_L5_gbreve`.
pub const KS_L5_gbreve: u16 = 0x011f;
/// `KS_L5_Idotabove`.
pub const KS_L5_Idotabove: u16 = 0x0130;
/// `KS_L5_idotless`.
pub const KS_L5_idotless: u16 = 0x0131;
/// `KS_L5_Scedilla`.
pub const KS_L5_Scedilla: u16 = 0x015e;
/// `KS_L5_scedilla`.
pub const KS_L5_scedilla: u16 = 0x015f;
/// `KS_L7_rightdblquot`.
pub const KS_L7_rightdblquot: u16 = 0x201d;
/// `KS_L7_dbllow9quot`.
pub const KS_L7_dbllow9quot: u16 = 0x201e;
/// `KS_L7_Ostroke`.
pub const KS_L7_Ostroke: u16 = 0x00d8;
/// `KS_L7_Rcedilla`.
pub const KS_L7_Rcedilla: u16 = 0x0156;
/// `KS_L7_AE`.
pub const KS_L7_AE: u16 = 0x00c0;
/// `KS_L7_leftdblquot`.
pub const KS_L7_leftdblquot: u16 = 0x201c;
/// `KS_L7_ostroke`.
pub const KS_L7_ostroke: u16 = 0x00f8;
/// `KS_L7_rcedilla`.
pub const KS_L7_rcedilla: u16 = 0x0157;
/// `KS_L7_ae`.
pub const KS_L7_ae: u16 = 0x00e6;
/// `KS_L7_Aogonek`.
pub const KS_L7_Aogonek: u16 = 0x0104;
/// `KS_L7_Iogonek`.
pub const KS_L7_Iogonek: u16 = 0x012e;
/// `KS_L7_Amacron`.
pub const KS_L7_Amacron: u16 = 0x0100;
/// `KS_L7_Cacute`.
pub const KS_L7_Cacute: u16 = 0x0106;
/// `KS_L7_Eogonek`.
pub const KS_L7_Eogonek: u16 = 0x0118;
/// `KS_L7_Emacron`.
pub const KS_L7_Emacron: u16 = 0x0112;
/// `KS_L7_Ccaron`.
pub const KS_L7_Ccaron: u16 = 0x010c;
/// `KS_L7_Zacute`.
pub const KS_L7_Zacute: u16 = 0x0179;
/// `KS_L7_Edot`.
pub const KS_L7_Edot: u16 = 0x0116;
/// `KS_L7_Gcedilla`.
pub const KS_L7_Gcedilla: u16 = 0x0122;
/// `KS_L7_Kcedilla`.
pub const KS_L7_Kcedilla: u16 = 0x0136;
/// `KS_L7_Imacron`.
pub const KS_L7_Imacron: u16 = 0x012a;
/// `KS_L7_Lcedilla`.
pub const KS_L7_Lcedilla: u16 = 0x013b;
/// `KS_L7_Scaron`.
pub const KS_L7_Scaron: u16 = 0x0160;
/// `KS_L7_Nacute`.
pub const KS_L7_Nacute: u16 = 0x0143;
/// `KS_L7_Ncedilla`.
pub const KS_L7_Ncedilla: u16 = 0x0145;
/// `KS_L7_Omacron`.
pub const KS_L7_Omacron: u16 = 0x014c;
/// `KS_L7_Uogonek`.
pub const KS_L7_Uogonek: u16 = 0x0172;
/// `KS_L7_Lstroke`.
pub const KS_L7_Lstroke: u16 = 0x0141;
/// `KS_L7_Sacute`.
pub const KS_L7_Sacute: u16 = 0x015a;
/// `KS_L7_Umacron`.
pub const KS_L7_Umacron: u16 = 0x016a;
/// `KS_L7_Zdot`.
pub const KS_L7_Zdot: u16 = 0x017b;
/// `KS_L7_Zcaron`.
pub const KS_L7_Zcaron: u16 = 0x017d;
/// `KS_L7_aogonek`.
pub const KS_L7_aogonek: u16 = 0x0105;
/// `KS_L7_iogonek`.
pub const KS_L7_iogonek: u16 = 0x012f;
/// `KS_L7_amacron`.
pub const KS_L7_amacron: u16 = 0x0101;
/// `KS_L7_cacute`.
pub const KS_L7_cacute: u16 = 0x0107;
/// `KS_L7_eogonek`.
pub const KS_L7_eogonek: u16 = 0x0119;
/// `KS_L7_emacron`.
pub const KS_L7_emacron: u16 = 0x0113;
/// `KS_L7_ccaron`.
pub const KS_L7_ccaron: u16 = 0x010d;
/// `KS_L7_zacute`.
pub const KS_L7_zacute: u16 = 0x017a;
/// `KS_L7_edot`.
pub const KS_L7_edot: u16 = 0x0117;
/// `KS_L7_gcedilla`.
pub const KS_L7_gcedilla: u16 = 0x0123;
/// `KS_L7_kcedilla`.
pub const KS_L7_kcedilla: u16 = 0x0137;
/// `KS_L7_imacron`.
pub const KS_L7_imacron: u16 = 0x012b;
/// `KS_L7_lcedilla`.
pub const KS_L7_lcedilla: u16 = 0x013c;
/// `KS_L7_scaron`.
pub const KS_L7_scaron: u16 = 0x0161;
/// `KS_L7_nacute`.
pub const KS_L7_nacute: u16 = 0x0144;
/// `KS_L7_ncedilla`.
pub const KS_L7_ncedilla: u16 = 0x0146;
/// `KS_L7_omacron`.
pub const KS_L7_omacron: u16 = 0x014d;
/// `KS_L7_uogonek`.
pub const KS_L7_uogonek: u16 = 0x0173;
/// `KS_L7_lstroke`.
pub const KS_L7_lstroke: u16 = 0x0142;
/// `KS_L7_sacute`.
pub const KS_L7_sacute: u16 = 0x015b;
/// `KS_L7_umacron`.
pub const KS_L7_umacron: u16 = 0x016b;
/// `KS_L7_zdot`.
pub const KS_L7_zdot: u16 = 0x017c;
/// `KS_L7_zcaron`.
pub const KS_L7_zcaron: u16 = 0x017e;
/// `KS_L7_rightsnglquot`.
pub const KS_L7_rightsnglquot: u16 = 0x2019;
/// `KS_Shift_L`.
pub const KS_Shift_L: u16 = 0xf101;
/// `KS_Shift_R`.
pub const KS_Shift_R: u16 = 0xf102;
/// `KS_Control_L`.
pub const KS_Control_L: u16 = 0xf103;
/// `KS_Control_R`.
pub const KS_Control_R: u16 = 0xf104;
/// `KS_Caps_Lock`.
pub const KS_Caps_Lock: u16 = 0xf105;
/// `KS_Shift_Lock`.
pub const KS_Shift_Lock: u16 = 0xf106;
/// `KS_Alt_L`.
pub const KS_Alt_L: u16 = 0xf107;
/// `KS_Alt_R`.
pub const KS_Alt_R: u16 = 0xf108;
/// `KS_Multi_key`.
pub const KS_Multi_key: u16 = 0xf109;
/// `KS_Mode_switch`.
pub const KS_Mode_switch: u16 = 0xf10a;
/// `KS_Num_Lock`.
pub const KS_Num_Lock: u16 = 0xf10b;
/// `KS_Hold_Screen`.
pub const KS_Hold_Screen: u16 = 0xf10c;
/// `KS_Cmd`.
pub const KS_Cmd: u16 = 0xf10d;
/// `KS_Cmd1`.
pub const KS_Cmd1: u16 = 0xf10e;
/// `KS_Cmd2`.
pub const KS_Cmd2: u16 = 0xf10f;
/// `KS_Meta_L`.
pub const KS_Meta_L: u16 = 0xf110;
/// `KS_Meta_R`.
pub const KS_Meta_R: u16 = 0xf111;
/// `KS_Zenkaku_Hankaku`: Zenkaku/Hankaku toggle.
pub const KS_Zenkaku_Hankaku: u16 = 0xf112;
/// `KS_Hiragana_Katakana`: Hiragana/Katakana toggle.
pub const KS_Hiragana_Katakana: u16 = 0xf113;
/// `KS_Henkan_Mode`: Start/Stop Conversion.
pub const KS_Henkan_Mode: u16 = 0xf114;
/// `KS_Henkan`: Alias for Henkan_Mode.
pub const KS_Henkan: u16 = 0xf115;
/// `KS_Muhenkan`: Cancel Conversion.
pub const KS_Muhenkan: u16 = 0xf116;
/// `KS_Mode_Lock`.
pub const KS_Mode_Lock: u16 = 0xf117;
/// `KS_KP_F1`.
pub const KS_KP_F1: u16 = 0xf291;
/// `KS_KP_F2`.
pub const KS_KP_F2: u16 = 0xf292;
/// `KS_KP_F3`.
pub const KS_KP_F3: u16 = 0xf293;
/// `KS_KP_F4`.
pub const KS_KP_F4: u16 = 0xf294;
/// `KS_KP_Home`.
pub const KS_KP_Home: u16 = 0xf295;
/// `KS_KP_Left`.
pub const KS_KP_Left: u16 = 0xf296;
/// `KS_KP_Up`.
pub const KS_KP_Up: u16 = 0xf297;
/// `KS_KP_Right`.
pub const KS_KP_Right: u16 = 0xf298;
/// `KS_KP_Down`.
pub const KS_KP_Down: u16 = 0xf299;
/// `KS_KP_Prior`.
pub const KS_KP_Prior: u16 = 0xf29a;
/// `KS_KP_Next`.
pub const KS_KP_Next: u16 = 0xf29b;
/// `KS_KP_End`.
pub const KS_KP_End: u16 = 0xf29c;
/// `KS_KP_Begin`.
pub const KS_KP_Begin: u16 = 0xf29d;
/// `KS_KP_Insert`.
pub const KS_KP_Insert: u16 = 0xf29e;
/// `KS_KP_Delete`.
pub const KS_KP_Delete: u16 = 0xf29f;
/// `KS_KP_Space`.
pub const KS_KP_Space: u16 = 0xf220;
/// `KS_KP_Tab`.
pub const KS_KP_Tab: u16 = 0xf209;
/// `KS_KP_Enter`.
pub const KS_KP_Enter: u16 = 0xf20d;
/// `KS_KP_Equal`.
pub const KS_KP_Equal: u16 = 0xf23d;
/// `KS_KP_Numbersign`.
pub const KS_KP_Numbersign: u16 = 0xf223;
/// `KS_KP_Multiply`.
pub const KS_KP_Multiply: u16 = 0xf22a;
/// `KS_KP_Add`.
pub const KS_KP_Add: u16 = 0xf22b;
/// `KS_KP_Separator`.
pub const KS_KP_Separator: u16 = 0xf22c;
/// `KS_KP_Subtract`.
pub const KS_KP_Subtract: u16 = 0xf22d;
/// `KS_KP_Decimal`.
pub const KS_KP_Decimal: u16 = 0xf22e;
/// `KS_KP_Divide`.
pub const KS_KP_Divide: u16 = 0xf22f;
/// `KS_KP_0`.
pub const KS_KP_0: u16 = 0xf230;
/// `KS_KP_1`.
pub const KS_KP_1: u16 = 0xf231;
/// `KS_KP_2`.
pub const KS_KP_2: u16 = 0xf232;
/// `KS_KP_3`.
pub const KS_KP_3: u16 = 0xf233;
/// `KS_KP_4`.
pub const KS_KP_4: u16 = 0xf234;
/// `KS_KP_5`.
pub const KS_KP_5: u16 = 0xf235;
/// `KS_KP_6`.
pub const KS_KP_6: u16 = 0xf236;
/// `KS_KP_7`.
pub const KS_KP_7: u16 = 0xf237;
/// `KS_KP_8`.
pub const KS_KP_8: u16 = 0xf238;
/// `KS_KP_9`.
pub const KS_KP_9: u16 = 0xf239;
/// `KS_f1`.
pub const KS_f1: u16 = 0xf300;
/// `KS_f2`.
pub const KS_f2: u16 = 0xf301;
/// `KS_f3`.
pub const KS_f3: u16 = 0xf302;
/// `KS_f4`.
pub const KS_f4: u16 = 0xf303;
/// `KS_f5`.
pub const KS_f5: u16 = 0xf304;
/// `KS_f6`.
pub const KS_f6: u16 = 0xf305;
/// `KS_f7`.
pub const KS_f7: u16 = 0xf306;
/// `KS_f8`.
pub const KS_f8: u16 = 0xf307;
/// `KS_f9`.
pub const KS_f9: u16 = 0xf308;
/// `KS_f10`.
pub const KS_f10: u16 = 0xf309;
/// `KS_f11`.
pub const KS_f11: u16 = 0xf30a;
/// `KS_f12`.
pub const KS_f12: u16 = 0xf30b;
/// `KS_f13`.
pub const KS_f13: u16 = 0xf30c;
/// `KS_f14`.
pub const KS_f14: u16 = 0xf30d;
/// `KS_f15`.
pub const KS_f15: u16 = 0xf30e;
/// `KS_f16`.
pub const KS_f16: u16 = 0xf30f;
/// `KS_f17`.
pub const KS_f17: u16 = 0xf310;
/// `KS_f18`.
pub const KS_f18: u16 = 0xf311;
/// `KS_f19`.
pub const KS_f19: u16 = 0xf312;
/// `KS_f20`.
pub const KS_f20: u16 = 0xf313;
/// `KS_f21`.
pub const KS_f21: u16 = 0xf314;
/// `KS_f22`.
pub const KS_f22: u16 = 0xf315;
/// `KS_f23`.
pub const KS_f23: u16 = 0xf316;
/// `KS_f24`.
pub const KS_f24: u16 = 0xf317;
/// `KS_F1`.
pub const KS_F1: u16 = 0xf340;
/// `KS_F2`.
pub const KS_F2: u16 = 0xf341;
/// `KS_F3`.
pub const KS_F3: u16 = 0xf342;
/// `KS_F4`.
pub const KS_F4: u16 = 0xf343;
/// `KS_F5`.
pub const KS_F5: u16 = 0xf344;
/// `KS_F6`.
pub const KS_F6: u16 = 0xf345;
/// `KS_F7`.
pub const KS_F7: u16 = 0xf346;
/// `KS_F8`.
pub const KS_F8: u16 = 0xf347;
/// `KS_F9`.
pub const KS_F9: u16 = 0xf348;
/// `KS_F10`.
pub const KS_F10: u16 = 0xf349;
/// `KS_F11`.
pub const KS_F11: u16 = 0xf34a;
/// `KS_F12`.
pub const KS_F12: u16 = 0xf34b;
/// `KS_F13`.
pub const KS_F13: u16 = 0xf34c;
/// `KS_F14`.
pub const KS_F14: u16 = 0xf34d;
/// `KS_F15`.
pub const KS_F15: u16 = 0xf34e;
/// `KS_F16`.
pub const KS_F16: u16 = 0xf34f;
/// `KS_F17`.
pub const KS_F17: u16 = 0xf350;
/// `KS_F18`.
pub const KS_F18: u16 = 0xf351;
/// `KS_F19`.
pub const KS_F19: u16 = 0xf352;
/// `KS_F20`.
pub const KS_F20: u16 = 0xf353;
/// `KS_F21`.
pub const KS_F21: u16 = 0xf354;
/// `KS_F22`.
pub const KS_F22: u16 = 0xf355;
/// `KS_F23`.
pub const KS_F23: u16 = 0xf356;
/// `KS_F24`.
pub const KS_F24: u16 = 0xf357;
/// `KS_Home`.
pub const KS_Home: u16 = 0xf381;
/// `KS_Prior`.
pub const KS_Prior: u16 = 0xf382;
/// `KS_Next`.
pub const KS_Next: u16 = 0xf383;
/// `KS_Up`.
pub const KS_Up: u16 = 0xf384;
/// `KS_Down`.
pub const KS_Down: u16 = 0xf385;
/// `KS_Left`.
pub const KS_Left: u16 = 0xf386;
/// `KS_Right`.
pub const KS_Right: u16 = 0xf387;
/// `KS_End`.
pub const KS_End: u16 = 0xf388;
/// `KS_Insert`.
pub const KS_Insert: u16 = 0xf389;
/// `KS_Help`.
pub const KS_Help: u16 = 0xf38a;
/// `KS_Execute`.
pub const KS_Execute: u16 = 0xf38b;
/// `KS_Find`.
pub const KS_Find: u16 = 0xf38c;
/// `KS_Select`.
pub const KS_Select: u16 = 0xf38d;
/// `KS_Again`.
pub const KS_Again: u16 = 0xf38e;
/// `KS_Props`.
pub const KS_Props: u16 = 0xf38f;
/// `KS_Undo`.
pub const KS_Undo: u16 = 0xf390;
/// `KS_Front`.
pub const KS_Front: u16 = 0xf391;
/// `KS_Copy`.
pub const KS_Copy: u16 = 0xf392;
/// `KS_Open`.
pub const KS_Open: u16 = 0xf393;
/// `KS_Paste`.
pub const KS_Paste: u16 = 0xf394;
/// `KS_Cut`.
pub const KS_Cut: u16 = 0xf395;
/// `KS_Backtab`.
pub const KS_Backtab: u16 = 0xf396;
/// `KS_Menu`.
pub const KS_Menu: u16 = 0xf3c0;
/// `KS_Pause`.
pub const KS_Pause: u16 = 0xf3c1;
/// `KS_Print_Screen`.
pub const KS_Print_Screen: u16 = 0xf3c2;
/// `KS_AudioMute`.
pub const KS_AudioMute: u16 = 0xf3d1;
/// `KS_AudioLower`.
pub const KS_AudioLower: u16 = 0xf3d2;
/// `KS_AudioRaise`.
pub const KS_AudioRaise: u16 = 0xf3d3;
/// `KS_Cmd_Screen0`.
pub const KS_Cmd_Screen0: u16 = 0xf400;
/// `KS_Cmd_Screen1`.
pub const KS_Cmd_Screen1: u16 = 0xf401;
/// `KS_Cmd_Screen2`.
pub const KS_Cmd_Screen2: u16 = 0xf402;
/// `KS_Cmd_Screen3`.
pub const KS_Cmd_Screen3: u16 = 0xf403;
/// `KS_Cmd_Screen4`.
pub const KS_Cmd_Screen4: u16 = 0xf404;
/// `KS_Cmd_Screen5`.
pub const KS_Cmd_Screen5: u16 = 0xf405;
/// `KS_Cmd_Screen6`.
pub const KS_Cmd_Screen6: u16 = 0xf406;
/// `KS_Cmd_Screen7`.
pub const KS_Cmd_Screen7: u16 = 0xf407;
/// `KS_Cmd_Screen8`.
pub const KS_Cmd_Screen8: u16 = 0xf408;
/// `KS_Cmd_Screen9`.
pub const KS_Cmd_Screen9: u16 = 0xf409;
/// `KS_Cmd_Screen10`.
pub const KS_Cmd_Screen10: u16 = 0xf40a;
/// `KS_Cmd_Screen11`.
pub const KS_Cmd_Screen11: u16 = 0xf40b;
/// `KS_Cmd_Debugger`.
pub const KS_Cmd_Debugger: u16 = 0xf420;
/// `KS_Cmd_ResetEmul`.
pub const KS_Cmd_ResetEmul: u16 = 0xf421;
/// `KS_Cmd_ResetClose`.
pub const KS_Cmd_ResetClose: u16 = 0xf422;
/// `KS_Cmd_BacklightOn`.
pub const KS_Cmd_BacklightOn: u16 = 0xf423;
/// `KS_Cmd_BacklightOff`.
pub const KS_Cmd_BacklightOff: u16 = 0xf424;
/// `KS_Cmd_BacklightToggle`.
pub const KS_Cmd_BacklightToggle: u16 = 0xf425;
/// `KS_Cmd_BrightnessUp`.
pub const KS_Cmd_BrightnessUp: u16 = 0xf426;
/// `KS_Cmd_BrightnessDown`.
pub const KS_Cmd_BrightnessDown: u16 = 0xf427;
/// `KS_Cmd_BrightnessRotate`.
pub const KS_Cmd_BrightnessRotate: u16 = 0xf428;
/// `KS_Cmd_ContrastUp`.
pub const KS_Cmd_ContrastUp: u16 = 0xf429;
/// `KS_Cmd_ContrastDown`.
pub const KS_Cmd_ContrastDown: u16 = 0xf42a;
/// `KS_Cmd_ContrastRotate`.
pub const KS_Cmd_ContrastRotate: u16 = 0xf42b;
/// `KS_Cmd_ScrollBack`.
pub const KS_Cmd_ScrollBack: u16 = 0xf42c;
/// `KS_Cmd_ScrollFwd`.
pub const KS_Cmd_ScrollFwd: u16 = 0xf42d;
/// `KS_Cmd_KbdReset`.
pub const KS_Cmd_KbdReset: u16 = 0xf42e;
/// `KS_Cmd_Sleep`.
pub const KS_Cmd_Sleep: u16 = 0xf42f;
/// `KS_Cmd_KbdBacklightToggle`.
pub const KS_Cmd_KbdBacklightToggle: u16 = 0xf430;
/// `KS_Cmd_KbdBacklightUp`.
pub const KS_Cmd_KbdBacklightUp: u16 = 0xf431;
/// `KS_Cmd_KbdBacklightDown`.
pub const KS_Cmd_KbdBacklightDown: u16 = 0xf432;
/// `KS_voidSymbol`.
pub const KS_voidSymbol: u16 = 0xf500;
/// `KS_GROUP_Mod`.
pub const KS_GROUP_Mod: u32 = 0xf100;
/// `KS_GROUP_Keypad`.
pub const KS_GROUP_Keypad: u32 = 0xf200;
/// `KS_GROUP_Function`.
pub const KS_GROUP_Function: u32 = 0xf300;
/// `KS_GROUP_Command`.
pub const KS_GROUP_Command: u32 = 0xf400;
/// `KS_GROUP_Internal`.
pub const KS_GROUP_Internal: u32 = 0xf500;
/// `KS_GROUP_Dead`: not encoded in keysym.
pub const KS_GROUP_Dead: u32 = 0xf801;
/// `KS_GROUP_Ascii`: not encoded in keysym.
pub const KS_GROUP_Ascii: u32 = 0xf802;
/// `KS_GROUP_Keycode`: not encoded in keysym.
pub const KS_GROUP_Keycode: u32 = 0xf803;
/// `KS_NUMKEYCODES`.
pub const KS_NUMKEYCODES: u32 = 0x1000;
/// `KB_NONE`.
pub const KB_NONE: u32 = 0x0000;
/// `KB_USER`.
pub const KB_USER: u32 = 0x0100;
/// `KB_US`.
pub const KB_US: u32 = 0x0200;
/// `KB_DE`.
pub const KB_DE: u32 = 0x0300;
/// `KB_DK`.
pub const KB_DK: u32 = 0x0400;
/// `KB_IT`.
pub const KB_IT: u32 = 0x0500;
/// `KB_FR`.
pub const KB_FR: u32 = 0x0600;
/// `KB_UK`.
pub const KB_UK: u32 = 0x0700;
/// `KB_JP`.
pub const KB_JP: u32 = 0x0800;
/// `KB_SV`.
pub const KB_SV: u32 = 0x0900;
/// `KB_NO`.
pub const KB_NO: u32 = 0x0a00;
/// `KB_ES`.
pub const KB_ES: u32 = 0x0b00;
/// `KB_HU`.
pub const KB_HU: u32 = 0x0c00;
/// `KB_BE`.
pub const KB_BE: u32 = 0x0d00;
/// `KB_RU`.
pub const KB_RU: u32 = 0x0e00;
/// `KB_SG`.
pub const KB_SG: u32 = 0x0f00;
/// `KB_SF`.
pub const KB_SF: u32 = 0x1000;
/// `KB_PT`.
pub const KB_PT: u32 = 0x1100;
/// `KB_UA`.
pub const KB_UA: u32 = 0x1200;
/// `KB_LT`.
pub const KB_LT: u32 = 0x1300;
/// `KB_LA`.
pub const KB_LA: u32 = 0x1400;
/// `KB_BR`.
pub const KB_BR: u32 = 0x1500;
/// `KB_NL`.
pub const KB_NL: u32 = 0x1600;
/// `KB_TR`.
pub const KB_TR: u32 = 0x1700;
/// `KB_PL`.
pub const KB_PL: u32 = 0x1800;
/// `KB_SI`.
pub const KB_SI: u32 = 0x1900;
/// `KB_CF`.
pub const KB_CF: u32 = 0x1a00;
/// `KB_LV`.
pub const KB_LV: u32 = 0x1b00;
/// `KB_IS`.
pub const KB_IS: u32 = 0x1c00;
/// `KB_EE`.
pub const KB_EE: u32 = 0x1d00;
/// `KB_NODEAD`: disable dead accents.
pub const KB_NODEAD: u32 = 0x00000001;
/// `KB_DECLK`: DEC LKnnn layout.
pub const KB_DECLK: u32 = 0x00000002;
/// `KB_LK401`: DEC LK401 instead LK201.
pub const KB_LK401: u32 = 0x00000004;
/// `KB_SWAPCTRLCAPS`: swap Left-Control and Caps-Lock.
pub const KB_SWAPCTRLCAPS: u32 = 0x00000008;
/// `KB_DVORAK`: Dvorak layout.
pub const KB_DVORAK: u32 = 0x00000010;
/// `KB_METAESC`: generate ESC prefix on ALT-key.
pub const KB_METAESC: u32 = 0x00000020;
/// `KB_NOENCODING`: no encodings available.
pub const KB_NOENCODING: u32 = 0x00000080;
/// `KB_APPLE`: Apple specific layout.
pub const KB_APPLE: u32 = 0x00010000;
/// `KB_COLEMAK`: Colemak layout.
pub const KB_COLEMAK: u32 = 0x02000000;
/// `KB_DEFAULT`: (attach-only) default layout.
pub const KB_DEFAULT: u32 = 0x80000000;

/// `KS_KEYCODE(v)`: the keysym that stands for the raw key code `v`.
pub const fn ks_keycode(v: u32) -> u32 {
    v | 0xe000
}

/// `KS_GROUP(k)`: the group of keysym `k`: dead accents, ASCII, key codes, or one of the
/// private groups (modifiers, keypad, function, command, internal).
pub const fn ks_group(k: u32) -> u32 {
    if k >= 0x0300 && k < 0x0370 {
        KS_GROUP_Dead
    } else if k & 0xf000 == 0xe000 {
        KS_GROUP_Keycode
    } else if k & 0xf800 == 0xf000 {
        k & 0xff00
    } else {
        KS_GROUP_Ascii
    }
}

/// `KS_VALUE(k)`: the value of keysym `k` within its group.
pub const fn ks_value(k: u32) -> u32 {
    if k & 0xf000 == 0xe000 {
        k & 0x0fff
    } else if k & 0xf800 == 0xf000 {
        k & 0x00ff
    } else {
        k
    }
}

/// `KB_ENCODING(e)`: the encoding of layout `e`.
pub const fn kb_encoding(e: u32) -> u32 {
    e & 0x0000_ff00
}

/// `KB_VARIANT(e)`: the variant bits of layout `e`.
pub const fn kb_variant(e: u32) -> u32 {
    e & 0xffff_00ff
}

/// `KB_ENCTAB`: the encodings and their names, as userland tables them.
pub const KB_ENCTAB: [(u32, &str); 29] = [
    (KB_USER, "user"),
    (KB_US, "us"),
    (KB_DE, "de"),
    (KB_DK, "dk"),
    (KB_IT, "it"),
    (KB_FR, "fr"),
    (KB_UK, "uk"),
    (KB_JP, "jp"),
    (KB_SV, "sv"),
    (KB_NO, "no"),
    (KB_ES, "es"),
    (KB_HU, "hu"),
    (KB_BE, "be"),
    (KB_RU, "ru"),
    (KB_UA, "ua"),
    (KB_SG, "sg"),
    (KB_SF, "sf"),
    (KB_PT, "pt"),
    (KB_LT, "lt"),
    (KB_LA, "la"),
    (KB_BR, "br"),
    (KB_NL, "nl"),
    (KB_TR, "tr"),
    (KB_PL, "pl"),
    (KB_SI, "si"),
    (KB_CF, "cf"),
    (KB_LV, "lv"),
    (KB_IS, "is"),
    (KB_EE, "ee"),
];

/// `KB_VARTAB`: the variants and their names, as userland tables them.
pub const KB_VARTAB: [(u32, &str); 9] = [
    (KB_NODEAD, "nodead"),
    (KB_DECLK, "declk"),
    (KB_LK401, "lk401"),
    (KB_SWAPCTRLCAPS, "swapctrlcaps"),
    (KB_DVORAK, "dvorak"),
    (KB_METAESC, "metaesc"),
    (KB_NOENCODING, "noencoding"),
    (KB_APPLE, "apple"),
    (KB_COLEMAK, "colemak"),
];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_and_values() {
        assert_eq!(ks_group(u32::from(KS_a)), KS_GROUP_Ascii);
        assert_eq!(ks_group(0x0301), KS_GROUP_Dead);
        assert_eq!(ks_group(ks_keycode(0x35)), KS_GROUP_Keycode);
        assert_eq!(ks_value(ks_keycode(0x35)), 0x35);
        assert_eq!(ks_group(u32::from(KS_Shift_L)), KS_GROUP_Mod);
        assert_eq!(KS_Shift_L, 0xf101);
        assert_eq!(ks_value(u32::from(KS_Shift_L)), 0x01);
        assert_eq!(ks_group(u32::from(KS_voidSymbol)), KS_GROUP_Internal);
        assert_eq!(ks_value(u32::from(KS_a)), u32::from(KS_a));
        assert_eq!(ks_group(u32::from(KS_Cmd_Screen1)), KS_GROUP_Command);
    }

    #[test]
    fn layout_encoding_and_variant_split() {
        let l = KB_DE | KB_NODEAD | KB_APPLE;
        assert_eq!(kb_encoding(l), KB_DE);
        assert_eq!(kb_variant(l), KB_NODEAD | KB_APPLE);
    }

    #[test]
    fn name_tables_cover_the_layouts() {
        assert_eq!(KB_ENCTAB[0], (KB_USER, "user"));
        assert!(KB_ENCTAB.contains(&(KB_US, "us")));
        assert!(KB_VARTAB.contains(&(KB_DVORAK, "dvorak")));
    }

    /// Every constant against `<dev/wscons/wsksymdef.h>`.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/wscons/wsksymdef.h");
        let mut names = crate::reftest::assert_defines!(defs;
            KS_BackSpace,
            KS_Tab,
            KS_Linefeed,
            KS_Clear,
            KS_Return,
            KS_Escape,
            KS_space,
            KS_exclam,
            KS_quotedbl,
            KS_numbersign,
            KS_dollar,
            KS_percent,
            KS_ampersand,
            KS_apostrophe,
            KS_parenleft,
            KS_parenright,
            KS_asterisk,
            KS_plus,
            KS_comma,
            KS_minus,
            KS_period,
            KS_slash,
            KS_0,
            KS_1,
            KS_2,
            KS_3,
            KS_4,
            KS_5,
            KS_6,
            KS_7,
            KS_8,
            KS_9,
            KS_colon,
            KS_semicolon,
            KS_less,
            KS_equal,
            KS_greater,
            KS_question,
            KS_at,
            KS_A,
            KS_B,
            KS_C,
            KS_D,
            KS_E,
            KS_F,
            KS_G,
            KS_H,
            KS_I,
            KS_J,
            KS_K,
            KS_L,
            KS_M,
            KS_N,
            KS_O,
            KS_P,
            KS_Q,
            KS_R,
            KS_S,
            KS_T,
            KS_U,
            KS_V,
            KS_W,
            KS_X,
            KS_Y,
            KS_Z,
            KS_bracketleft,
            KS_backslash,
            KS_bracketright,
            KS_asciicircum,
            KS_underscore,
            KS_grave,
            KS_a,
            KS_b,
            KS_c,
            KS_d,
            KS_e,
            KS_f,
            KS_g,
            KS_h,
            KS_i,
            KS_j,
            KS_k,
            KS_l,
            KS_m,
            KS_n,
            KS_o,
            KS_p,
            KS_q,
            KS_r,
            KS_s,
            KS_t,
            KS_u,
            KS_v,
            KS_w,
            KS_x,
            KS_y,
            KS_z,
            KS_braceleft,
            KS_bar,
            KS_braceright,
            KS_asciitilde,
            KS_Delete,
            KS_nobreakspace,
            KS_exclamdown,
            KS_cent,
            KS_sterling,
            KS_currency,
            KS_yen,
            KS_brokenbar,
            KS_section,
            KS_diaeresis,
            KS_copyright,
            KS_ordfeminine,
            KS_guillemotleft,
            KS_notsign,
            KS_hyphen,
            KS_registered,
            KS_macron,
            KS_degree,
            KS_plusminus,
            KS_twosuperior,
            KS_threesuperior,
            KS_acute,
            KS_mu,
            KS_paragraph,
            KS_periodcentered,
            KS_cedilla,
            KS_onesuperior,
            KS_masculine,
            KS_guillemotright,
            KS_onequarter,
            KS_onehalf,
            KS_threequarters,
            KS_questiondown,
            KS_Agrave,
            KS_Aacute,
            KS_Acircumflex,
            KS_Atilde,
            KS_Adiaeresis,
            KS_Aring,
            KS_AE,
            KS_Ccedilla,
            KS_Egrave,
            KS_Eacute,
            KS_Ecircumflex,
            KS_Ediaeresis,
            KS_Igrave,
            KS_Iacute,
            KS_Icircumflex,
            KS_Idiaeresis,
            KS_ETH,
            KS_Ntilde,
            KS_Ograve,
            KS_Oacute,
            KS_Ocircumflex,
            KS_Otilde,
            KS_Odiaeresis,
            KS_multiply,
            KS_Ooblique,
            KS_Ugrave,
            KS_Uacute,
            KS_Ucircumflex,
            KS_Udiaeresis,
            KS_Yacute,
            KS_THORN,
            KS_ssharp,
            KS_agrave,
            KS_aacute,
            KS_acircumflex,
            KS_atilde,
            KS_adiaeresis,
            KS_aring,
            KS_ae,
            KS_ccedilla,
            KS_egrave,
            KS_eacute,
            KS_ecircumflex,
            KS_ediaeresis,
            KS_igrave,
            KS_iacute,
            KS_icircumflex,
            KS_idiaeresis,
            KS_eth,
            KS_ntilde,
            KS_ograve,
            KS_oacute,
            KS_ocircumflex,
            KS_otilde,
            KS_odiaeresis,
            KS_division,
            KS_oslash,
            KS_ugrave,
            KS_uacute,
            KS_ucircumflex,
            KS_udiaeresis,
            KS_yacute,
            KS_thorn,
            KS_ydiaeresis,
            KS_Odoubleacute,
            KS_odoubleacute,
            KS_Udoubleacute,
            KS_udoubleacute,
            KS_dead_grave,
            KS_dead_acute,
            KS_dead_circumflex,
            KS_dead_tilde,
            KS_dead_diaeresis,
            KS_dead_abovering,
            KS_dead_cedilla,
            KS_dead_caron,
            KS_Cyrillic_YO,
            KS_Cyrillic_YEUKR,
            KS_Cyrillic_IUKR,
            KS_Cyrillic_YI,
            KS_Cyrillic_A,
            KS_Cyrillic_BE,
            KS_Cyrillic_VE,
            KS_Cyrillic_GE,
            KS_Cyrillic_DE,
            KS_Cyrillic_IE,
            KS_Cyrillic_ZHE,
            KS_Cyrillic_ZE,
            KS_Cyrillic_I,
            KS_Cyrillic_ISHORT,
            KS_Cyrillic_KA,
            KS_Cyrillic_EL,
            KS_Cyrillic_EM,
            KS_Cyrillic_EN,
            KS_Cyrillic_O,
            KS_Cyrillic_PE,
            KS_Cyrillic_ER,
            KS_Cyrillic_ES,
            KS_Cyrillic_TE,
            KS_Cyrillic_U,
            KS_Cyrillic_EF,
            KS_Cyrillic_HA,
            KS_Cyrillic_TSE,
            KS_Cyrillic_CHE,
            KS_Cyrillic_SHA,
            KS_Cyrillic_SCHA,
            KS_Cyrillic_HSIGHN,
            KS_Cyrillic_YERU,
            KS_Cyrillic_SSIGHN,
            KS_Cyrillic_E,
            KS_Cyrillic_YU,
            KS_Cyrillic_YA,
            KS_Cyrillic_a,
            KS_Cyrillic_be,
            KS_Cyrillic_ve,
            KS_Cyrillic_ge,
            KS_Cyrillic_de,
            KS_Cyrillic_ie,
            KS_Cyrillic_zhe,
            KS_Cyrillic_ze,
            KS_Cyrillic_i,
            KS_Cyrillic_ishort,
            KS_Cyrillic_ka,
            KS_Cyrillic_el,
            KS_Cyrillic_em,
            KS_Cyrillic_en,
            KS_Cyrillic_o,
            KS_Cyrillic_pe,
            KS_Cyrillic_er,
            KS_Cyrillic_es,
            KS_Cyrillic_te,
            KS_Cyrillic_u,
            KS_Cyrillic_ef,
            KS_Cyrillic_ha,
            KS_Cyrillic_tse,
            KS_Cyrillic_che,
            KS_Cyrillic_sha,
            KS_Cyrillic_scha,
            KS_Cyrillic_hsighn,
            KS_Cyrillic_yeru,
            KS_Cyrillic_ssighn,
            KS_Cyrillic_e,
            KS_Cyrillic_yu,
            KS_Cyrillic_ya,
            KS_Cyrillic_yo,
            KS_Cyrillic_yeukr,
            KS_Cyrillic_iukr,
            KS_Cyrillic_yi,
            KS_Cyrillic_GHEUKR,
            KS_Cyrillic_gheukr,
            KS_L2_Abreve,
            KS_L2_abreve,
            KS_L2_Aogonek,
            KS_L2_aogonek,
            KS_L2_Cacute,
            KS_L2_cacute,
            KS_L2_Ccaron,
            KS_L2_ccaron,
            KS_L2_Dcaron,
            KS_L2_dcaron,
            KS_L2_Dstroke,
            KS_L2_dstroke,
            KS_L2_Eogonek,
            KS_L2_eogonek,
            KS_L2_Ecaron,
            KS_L2_ecaron,
            KS_L2_Lacute,
            KS_L2_lacute,
            KS_L2_Lcaron,
            KS_L2_lcaron,
            KS_L2_Lstroke,
            KS_L2_lstroke,
            KS_L2_Nacute,
            KS_L2_nacute,
            KS_L2_Ncaron,
            KS_L2_Odoubleacute,
            KS_L2_odoubleacute,
            KS_L2_Racute,
            KS_L2_racute,
            KS_L2_Rcaron,
            KS_L2_rcaron,
            KS_L2_Sacute,
            KS_L2_sacute,
            KS_L2_Scedilla,
            KS_L2_scedilla,
            KS_L2_Scaron,
            KS_L2_scaron,
            KS_L2_Tcedilla,
            KS_L2_tcedilla,
            KS_L2_Tcaron,
            KS_L2_tcaron,
            KS_L2_Uring,
            KS_L2_uring,
            KS_L2_Udoubleacute,
            KS_L2_udoubleacute,
            KS_L2_Zacute,
            KS_L2_zacute,
            KS_L2_Zdotabove,
            KS_L2_zdotabove,
            KS_L2_Zcaron,
            KS_L2_zcaron,
            KS_L2_caron,
            KS_L2_breve,
            KS_L2_dotabove,
            KS_L2_ogonek,
            KS_L2_dblacute,
            KS_L5_Gbreve,
            KS_L5_gbreve,
            KS_L5_Idotabove,
            KS_L5_idotless,
            KS_L5_Scedilla,
            KS_L5_scedilla,
            KS_L7_rightdblquot,
            KS_L7_dbllow9quot,
            KS_L7_Ostroke,
            KS_L7_Rcedilla,
            KS_L7_AE,
            KS_L7_leftdblquot,
            KS_L7_ostroke,
            KS_L7_rcedilla,
            KS_L7_ae,
            KS_L7_Aogonek,
            KS_L7_Iogonek,
            KS_L7_Amacron,
            KS_L7_Cacute,
            KS_L7_Eogonek,
            KS_L7_Emacron,
            KS_L7_Ccaron,
            KS_L7_Zacute,
            KS_L7_Edot,
            KS_L7_Gcedilla,
            KS_L7_Kcedilla,
            KS_L7_Imacron,
            KS_L7_Lcedilla,
            KS_L7_Scaron,
            KS_L7_Nacute,
            KS_L7_Ncedilla,
            KS_L7_Omacron,
            KS_L7_Uogonek,
            KS_L7_Lstroke,
            KS_L7_Sacute,
            KS_L7_Umacron,
            KS_L7_Zdot,
            KS_L7_Zcaron,
            KS_L7_aogonek,
            KS_L7_iogonek,
            KS_L7_amacron,
            KS_L7_cacute,
            KS_L7_eogonek,
            KS_L7_emacron,
            KS_L7_ccaron,
            KS_L7_zacute,
            KS_L7_edot,
            KS_L7_gcedilla,
            KS_L7_kcedilla,
            KS_L7_imacron,
            KS_L7_lcedilla,
            KS_L7_scaron,
            KS_L7_nacute,
            KS_L7_ncedilla,
            KS_L7_omacron,
            KS_L7_uogonek,
            KS_L7_lstroke,
            KS_L7_sacute,
            KS_L7_umacron,
            KS_L7_zdot,
            KS_L7_zcaron,
            KS_L7_rightsnglquot,
            KS_Shift_L,
            KS_Shift_R,
            KS_Control_L,
            KS_Control_R,
            KS_Caps_Lock,
            KS_Shift_Lock,
            KS_Alt_L,
            KS_Alt_R,
            KS_Multi_key,
            KS_Mode_switch,
            KS_Num_Lock,
            KS_Hold_Screen,
            KS_Cmd,
            KS_Cmd1,
            KS_Cmd2,
            KS_Meta_L,
            KS_Meta_R,
            KS_Zenkaku_Hankaku,
            KS_Hiragana_Katakana,
            KS_Henkan_Mode,
            KS_Henkan,
            KS_Muhenkan,
            KS_Mode_Lock,
            KS_KP_F1,
            KS_KP_F2,
            KS_KP_F3,
            KS_KP_F4,
            KS_KP_Home,
            KS_KP_Left,
            KS_KP_Up,
            KS_KP_Right,
            KS_KP_Down,
            KS_KP_Prior,
            KS_KP_Next,
            KS_KP_End,
            KS_KP_Begin,
            KS_KP_Insert,
            KS_KP_Delete,
            KS_KP_Space,
            KS_KP_Tab,
            KS_KP_Enter,
            KS_KP_Equal,
            KS_KP_Numbersign,
            KS_KP_Multiply,
            KS_KP_Add,
            KS_KP_Separator,
            KS_KP_Subtract,
            KS_KP_Decimal,
            KS_KP_Divide,
            KS_KP_0,
            KS_KP_1,
            KS_KP_2,
            KS_KP_3,
            KS_KP_4,
            KS_KP_5,
            KS_KP_6,
            KS_KP_7,
            KS_KP_8,
            KS_KP_9,
            KS_f1,
            KS_f2,
            KS_f3,
            KS_f4,
            KS_f5,
            KS_f6,
            KS_f7,
            KS_f8,
            KS_f9,
            KS_f10,
            KS_f11,
            KS_f12,
            KS_f13,
            KS_f14,
            KS_f15,
            KS_f16,
            KS_f17,
            KS_f18,
            KS_f19,
            KS_f20,
            KS_f21,
            KS_f22,
            KS_f23,
            KS_f24,
            KS_F1,
            KS_F2,
            KS_F3,
            KS_F4,
            KS_F5,
            KS_F6,
            KS_F7,
            KS_F8,
            KS_F9,
            KS_F10,
            KS_F11,
            KS_F12,
            KS_F13,
            KS_F14,
            KS_F15,
            KS_F16,
            KS_F17,
            KS_F18,
            KS_F19,
            KS_F20,
            KS_F21,
            KS_F22,
            KS_F23,
            KS_F24,
            KS_Home,
            KS_Prior,
            KS_Next,
            KS_Up,
            KS_Down,
            KS_Left,
            KS_Right,
            KS_End,
            KS_Insert,
            KS_Help,
            KS_Execute,
            KS_Find,
            KS_Select,
            KS_Again,
            KS_Props,
            KS_Undo,
            KS_Front,
            KS_Copy,
            KS_Open,
            KS_Paste,
            KS_Cut,
            KS_Backtab,
            KS_Menu,
            KS_Pause,
            KS_Print_Screen,
            KS_AudioMute,
            KS_AudioLower,
            KS_AudioRaise,
            KS_Cmd_Screen0,
            KS_Cmd_Screen1,
            KS_Cmd_Screen2,
            KS_Cmd_Screen3,
            KS_Cmd_Screen4,
            KS_Cmd_Screen5,
            KS_Cmd_Screen6,
            KS_Cmd_Screen7,
            KS_Cmd_Screen8,
            KS_Cmd_Screen9,
            KS_Cmd_Screen10,
            KS_Cmd_Screen11,
            KS_Cmd_Debugger,
            KS_Cmd_ResetEmul,
            KS_Cmd_ResetClose,
            KS_Cmd_BacklightOn,
            KS_Cmd_BacklightOff,
            KS_Cmd_BacklightToggle,
            KS_Cmd_BrightnessUp,
            KS_Cmd_BrightnessDown,
            KS_Cmd_BrightnessRotate,
            KS_Cmd_ContrastUp,
            KS_Cmd_ContrastDown,
            KS_Cmd_ContrastRotate,
            KS_Cmd_ScrollBack,
            KS_Cmd_ScrollFwd,
            KS_Cmd_KbdReset,
            KS_Cmd_Sleep,
            KS_Cmd_KbdBacklightToggle,
            KS_Cmd_KbdBacklightUp,
            KS_Cmd_KbdBacklightDown,
            KS_voidSymbol,
            KS_GROUP_Mod,
            KS_GROUP_Keypad,
            KS_GROUP_Function,
            KS_GROUP_Command,
            KS_GROUP_Internal,
            KS_GROUP_Dead,
            KS_GROUP_Ascii,
            KS_GROUP_Keycode,
            KS_NUMKEYCODES,
            KB_NONE,
            KB_USER,
            KB_US,
            KB_DE,
            KB_DK,
            KB_IT,
            KB_FR,
            KB_UK,
            KB_JP,
            KB_SV,
            KB_NO,
            KB_ES,
            KB_HU,
            KB_BE,
            KB_RU,
            KB_SG,
            KB_SF,
            KB_PT,
            KB_UA,
            KB_LT,
            KB_LA,
            KB_BR,
            KB_NL,
            KB_TR,
            KB_PL,
            KB_SI,
            KB_CF,
            KB_LV,
            KB_IS,
            KB_EE,
            KB_NODEAD,
            KB_DECLK,
            KB_LK401,
            KB_SWAPCTRLCAPS,
            KB_DVORAK,
            KB_METAESC,
            KB_NOENCODING,
            KB_APPLE,
            KB_COLEMAK,
            KB_DEFAULT,
        );
        names.extend(["KB_ENCTAB", "KB_VARTAB"]);
        crate::reftest::assert_complete(&defs, "K", &names);
    }
}
/* </TESTS> */
