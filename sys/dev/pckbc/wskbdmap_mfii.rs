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
 * PLEASE DO NOT FORGET TO REGEN
 *	sys/dev/usb/ukbdmap.c
 * AFTER ANY CHANGES TO THIS FILE!
 */

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
//! The PS/2 keyboard layouts of `pckbd(4)`, `dev/pckbc/wskbdmap_mfii.c`: for each layout
//! `wskbd(4)` offers a keyboard on the AT keyboard controller (`KB_US`, `KB_DE`, ... and their
//! variants), the keysyms of the keys it changes, by XT scan code (`KC(n)`).
//!
//! Upstream: sys/dev/pckbc/wskbdmap_mfii.c @ 3ce1f3f79392
//! Upstream: sys/dev/pckbc/wskbdmap_mfii.h @ 3ce1f3f79392
//!
//! The header's one line, `extern const struct wscons_keydesc pckbd_keydesctab[]`, is the
//! `pub static` [`PCKBD_KEYDESCTAB`] below. This is the file `ukbdmap.c` is generated from
//! (`makemap.awk` turns the XT scan codes into USB key codes); the Rust is made from the C the
//! same way as `dev/usb/ukbdmap.rs`, keeping every line of the tables and their comments. A
//! table is a list of entries, each a key code (`KC(n)`, a keysym of group `KS_GROUP_Keycode`),
//! an optional command keysym (`KS_Cmd_*`) and up to four keysyms (normal, shifted, altgr,
//! shift-altgr); `pckbd_keydesctab` names each layout's table and the layout it is a delta
//! over, which `wskbdutil.rs`'s `wskbd_load_keymap` applies first.
//!
//! ## Deviations
//! - `WSKBD_NO_INTL_LAYOUTS` is not defined (GENERIC has every layout): the `#if` and `#endif`
//!   lines are kept as comments. So is the `#if 0` around `KC(198)` (`KS_Cmd_ResetClose`,
//!   CTL-Break): the entry is a comment, as the C does not compile it.
//! - The macro `KC(n)` is the `const fn` [`kc`], `KBD_MAP(name, base, map)` the `const fn`
//!   `kbd_map`; the tables are `&[KeysymT]` slices (`map_size` is their length) and
//!   `pckbd_keydesctab` has no `{0, 0, 0, 0}` terminator (`wsksymvar.rs`).
//! - The C's block comments are line comments.

use crate::dev::wscons::wsksymdef::*;
use crate::dev::wscons::wsksymvar::{KeysymT, WsconsKeydesc};

/// `KC(n)`: the keysym of key code `n` (`KS_KEYCODE(n)`).
const fn kc(n: u32) -> KeysymT {
    ks_keycode(n) as KeysymT
}

/// `KBD_MAP(name, base, map)`: layout `name`, a delta over `base`, with the keymap `map`.
const fn kbd_map(name: u32, base: u32, map: &'static [KeysymT]) -> WsconsKeydesc {
    WsconsKeydesc {
        name,
        base,
        map_size: map.len() as i32,
        map,
    }
}

/// `pckbd_keydesc_us`.
#[rustfmt::skip]
static PCKBD_KEYDESC_US: &[KeysymT] = &[
// pos      command            normal          shifted
    kc(1),   KS_Cmd_Debugger,   KS_Escape,
    kc(2),                      KS_1,           KS_exclam,
    kc(3),                      KS_2,           KS_at,
    kc(4),                      KS_3,           KS_numbersign,
    kc(5),                      KS_4,           KS_dollar,
    kc(6),                      KS_5,           KS_percent,
    kc(7),                      KS_6,           KS_asciicircum,
    kc(8),                      KS_7,           KS_ampersand,
    kc(9),                      KS_8,           KS_asterisk,
    kc(10),                     KS_9,           KS_parenleft,
    kc(11),                     KS_0,           KS_parenright,
    kc(12),                     KS_minus,       KS_underscore,
    kc(13),                     KS_equal,       KS_plus,
    kc(14),  KS_Cmd_ResetEmul,  KS_Delete,
    kc(15),                     KS_Tab,         KS_Backtab,
    kc(16),                     KS_q,
    kc(17),                     KS_w,
    kc(18),                     KS_e,
    kc(19),                     KS_r,
    kc(20),                     KS_t,
    kc(21),                     KS_y,
    kc(22),                     KS_u,
    kc(23),                     KS_i,
    kc(24),                     KS_o,
    kc(25),                     KS_p,
    kc(26),                     KS_bracketleft, KS_braceleft,
    kc(27),                     KS_bracketright, KS_braceright,
    kc(28),                     KS_Return,
    kc(29),  KS_Cmd1,           KS_Control_L,
    kc(30),                     KS_a,
    kc(31),                     KS_s,
    kc(32),                     KS_d,
    kc(33),                     KS_f,
    kc(34),                     KS_g,
    kc(35),                     KS_h,
    kc(36),                     KS_j,
    kc(37),                     KS_k,
    kc(38),                     KS_l,
    kc(39),                     KS_semicolon,   KS_colon,
    kc(40),                     KS_apostrophe,  KS_quotedbl,
    kc(41),                     KS_grave,       KS_asciitilde,
    kc(42),                     KS_Shift_L,
    kc(43),                     KS_backslash,   KS_bar,
    kc(44),                     KS_z,
    kc(45),                     KS_x,
    kc(46),                     KS_c,
    kc(47),                     KS_v,
    kc(48),                     KS_b,
    kc(49),                     KS_n,
    kc(50),                     KS_m,
    kc(51),                     KS_comma,       KS_less,
    kc(52),                     KS_period,      KS_greater,
    kc(53),                     KS_slash,       KS_question,
    kc(54),                     KS_Shift_R,
    kc(55),                     KS_KP_Multiply,
    kc(56),  KS_Cmd2,           KS_Alt_L,
    kc(57),                     KS_space,
    kc(58),                     KS_Caps_Lock,
    kc(59),  KS_Cmd_Screen0,    KS_f1,
    kc(60),  KS_Cmd_Screen1,    KS_f2,
    kc(61),  KS_Cmd_Screen2,    KS_f3,
    kc(62),  KS_Cmd_Screen3,    KS_f4,
    kc(63),  KS_Cmd_Screen4,    KS_f5,
    kc(64),  KS_Cmd_Screen5,    KS_f6,
    kc(65),  KS_Cmd_Screen6,    KS_f7,
    kc(66),  KS_Cmd_Screen7,    KS_f8,
    kc(67),  KS_Cmd_Screen8,    KS_f9,
    kc(68),  KS_Cmd_Screen9,    KS_f10,
    kc(69),                     KS_Num_Lock,
    kc(70),                     KS_Hold_Screen,
    kc(71),                     KS_KP_Home,     KS_KP_7,
    kc(72),                     KS_KP_Up,       KS_KP_8,
    kc(73),                     KS_KP_Prior,    KS_KP_9,
    kc(74),                     KS_KP_Subtract,
    kc(75),                     KS_KP_Left,     KS_KP_4,
    kc(76),                     KS_KP_Begin,    KS_KP_5,
    kc(77),                     KS_KP_Right,    KS_KP_6,
    kc(78),                     KS_KP_Add,
    kc(79),                     KS_KP_End,      KS_KP_1,
    kc(80),                     KS_KP_Down,     KS_KP_2,
    kc(81),                     KS_KP_Next,     KS_KP_3,
    kc(82),                     KS_KP_Insert,   KS_KP_0,
    kc(83),                     KS_KP_Delete,   KS_KP_Decimal,
    kc(87),  KS_Cmd_Screen10,   KS_f11,
    kc(88),  KS_Cmd_Screen11,   KS_f12,
    kc(91),                     KS_f13,
    kc(92),                     KS_f14,
    kc(93),                     KS_f15,
    kc(99),                     KS_f16,
    kc(100),                    KS_f17,
    kc(101),                    KS_f18,
    kc(102),                    KS_f19,
    kc(103),                    KS_f20,
    kc(104),                    KS_f21,
    kc(105),                    KS_f22,
    kc(106),                    KS_f23,
    kc(107),                    KS_f24,
    kc(127),                    KS_Pause, // Break
    kc(156),                    KS_KP_Enter,
    kc(157), KS_Cmd1,           KS_Control_R,
    kc(160),                    KS_AudioMute,
    kc(170),                    KS_Print_Screen,
    kc(174),                    KS_AudioLower,
    kc(176),                    KS_AudioRaise,
    kc(181),                    KS_KP_Divide,
    kc(183),                    KS_Print_Screen,
    kc(184), KS_Cmd2,           KS_Alt_R,       KS_Multi_key,
    // #if 0
    // kc(198),  KS_Cmd_ResetClose, // CTL-Break
    // #endif
    kc(199),                    KS_Home,
    kc(200),                    KS_Up,
    kc(201), KS_Cmd_ScrollBack, KS_Prior,
    kc(203),                    KS_Left,
    kc(205),                    KS_Right,
    kc(207),                    KS_End,
    kc(208),                    KS_Down,
    kc(209), KS_Cmd_ScrollFwd,  KS_Next,
    kc(210),                    KS_Insert,
    kc(211), KS_Cmd_KbdReset,   KS_KP_Delete,
    kc(219),                    KS_Meta_L,
    kc(220),                    KS_Meta_R,
    kc(221),                    KS_Menu,
];

// #if !defined(WSKBD_NO_INTL_LAYOUTS)

/// `pckbd_keydesc_de`.
#[rustfmt::skip]
static PCKBD_KEYDESC_DE: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(3),   KS_2,              KS_quotedbl,    KS_twosuperior,
    kc(4),   KS_3,              KS_section,     KS_threesuperior,
    kc(7),   KS_6,              KS_ampersand,
    kc(8),   KS_7,              KS_slash,       KS_braceleft,
    kc(9),   KS_8,              KS_parenleft,   KS_bracketleft,
    kc(10),  KS_9,              KS_parenright,  KS_bracketright,
    kc(11),  KS_0,              KS_equal,       KS_braceright,
    kc(12),  KS_ssharp,         KS_question,    KS_backslash,
    kc(13),  KS_dead_acute,     KS_dead_grave,
    kc(16),  KS_q,              KS_Q,           KS_at,
    kc(21),  KS_z,
    kc(26),  KS_udiaeresis,
    kc(27),  KS_plus,           KS_asterisk,    KS_dead_tilde,
    kc(39),  KS_odiaeresis,
    kc(40),  KS_adiaeresis,
    kc(41),  KS_dead_circumflex,KS_dead_abovering,
    kc(43),  KS_numbersign,     KS_apostrophe,
    kc(44),  KS_y,
    kc(50),  KS_m,              KS_M,           KS_mu,
    kc(51),  KS_comma,          KS_semicolon,
    kc(52),  KS_period,         KS_colon,
    kc(53),  KS_minus,          KS_underscore,
    kc(86),  KS_less,           KS_greater,     KS_bar,         KS_brokenbar,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_de_nodead`.
#[rustfmt::skip]
static PCKBD_KEYDESC_DE_NODEAD: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(13),  KS_apostrophe,     KS_grave,
    kc(27),  KS_plus,           KS_asterisk,    KS_asciitilde,
    kc(41),  KS_asciicircum,    KS_degree,
];

/// `pckbd_keydesc_dk`.
#[rustfmt::skip]
static PCKBD_KEYDESC_DK: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(3),   KS_2,              KS_quotedbl,    KS_at,
    kc(4),   KS_3,              KS_numbersign,  KS_sterling,
    kc(5),   KS_4,              KS_currency,    KS_dollar,
    kc(7),   KS_6,              KS_ampersand,
    kc(8),   KS_7,              KS_slash,       KS_braceleft,
    kc(9),   KS_8,              KS_parenleft,   KS_bracketleft,
    kc(10),  KS_9,              KS_parenright,  KS_bracketright,
    kc(11),  KS_0,              KS_equal,       KS_braceright,
    kc(12),  KS_plus,           KS_question,
    kc(13),  KS_dead_acute,     KS_dead_grave,  KS_bar,
    kc(26),  KS_aring,
    kc(27),  KS_dead_diaeresis, KS_dead_circumflex, KS_dead_tilde,
    kc(39),  KS_ae,
    kc(40),  KS_oslash,
    kc(41),  KS_onehalf,        KS_paragraph,
    kc(43),  KS_apostrophe,     KS_asterisk,
    kc(51),  KS_comma,          KS_semicolon,
    kc(52),  KS_period,         KS_colon,
    kc(53),  KS_minus,          KS_underscore,
    kc(86),  KS_less,           KS_greater,     KS_backslash,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_dk_nodead`.
#[rustfmt::skip]
static PCKBD_KEYDESC_DK_NODEAD: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(13),  KS_apostrophe,     KS_grave,       KS_bar,
    kc(27),  KS_diaeresis,      KS_asciicircum, KS_asciitilde,
];

/// `pckbd_keydesc_sv`.
#[rustfmt::skip]
static PCKBD_KEYDESC_SV: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(12),  KS_plus,           KS_question,    KS_backslash,
    kc(27),  KS_dead_diaeresis, KS_dead_circumflex, KS_dead_tilde,
    kc(39),  KS_odiaeresis,
    kc(40),  KS_adiaeresis,
    kc(41),  KS_section,        KS_onehalf,
    kc(86),  KS_less,           KS_greater,     KS_bar,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_sv_nodead`.
#[rustfmt::skip]
static PCKBD_KEYDESC_SV_NODEAD: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(13),  KS_apostrophe,     KS_grave,       KS_bar,
    kc(27),  KS_diaeresis,      KS_asciicircum, KS_asciitilde,
];

/// `pckbd_keydesc_no`.
#[rustfmt::skip]
static PCKBD_KEYDESC_NO: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(13),  KS_backslash,      KS_dead_grave,  KS_dead_acute,
    kc(27),  KS_dead_diaeresis, KS_dead_circumflex, KS_dead_tilde,
    kc(39),  KS_oslash,
    kc(40),  KS_ae,
    kc(41),  KS_bar,            KS_paragraph,
    kc(86),  KS_less,           KS_greater,
];

/// `pckbd_keydesc_no_nodead`.
#[rustfmt::skip]
static PCKBD_KEYDESC_NO_NODEAD: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(13),  KS_backslash,      KS_grave,       KS_acute,
    kc(27),  KS_diaeresis,      KS_asciicircum, KS_asciitilde,
];

/// `pckbd_keydesc_fr`.
#[rustfmt::skip]
static PCKBD_KEYDESC_FR: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(2),   KS_ampersand,      KS_1,
    kc(3),   KS_eacute,         KS_2,           KS_asciitilde,
    kc(4),   KS_quotedbl,       KS_3,           KS_numbersign,
    kc(5),   KS_apostrophe,     KS_4,           KS_braceleft,
    kc(6),   KS_parenleft,      KS_5,           KS_bracketleft,
    kc(7),   KS_minus,          KS_6,           KS_bar,
    kc(8),   KS_egrave,         KS_7,           KS_grave,
    kc(9),   KS_underscore,     KS_8,           KS_backslash,
    kc(10),  KS_ccedilla,       KS_9,           KS_asciicircum,
    kc(11),  KS_agrave,         KS_0,           KS_at,
    kc(12),  KS_parenright,     KS_degree,      KS_bracketright,
    kc(13),  KS_equal,          KS_plus,        KS_braceright,
    kc(16),  KS_a,
    kc(17),  KS_z,
    kc(26),  KS_dead_circumflex, KS_dead_diaeresis,
    kc(27),  KS_dollar,         KS_sterling,    KS_currency,
    kc(30),  KS_q,
    kc(39),  KS_m,
    kc(40),  KS_ugrave,         KS_percent,
    kc(41),  KS_twosuperior,
    kc(43),  KS_asterisk,       KS_mu,
    kc(44),  KS_w,
    kc(50),  KS_comma,          KS_question,
    kc(51),  KS_semicolon,      KS_period,
    kc(52),  KS_colon,          KS_slash,
    kc(53),  KS_exclam,         KS_section,
    kc(86),  KS_less,           KS_greater,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

//
// fr-dvorak-be'po layout, simplified map, per http://www.clavier-dvorak.org/
// (the complete map is still a moving target)
//
/// `pckbd_keydesc_fr_dvorak_bepo`.
#[rustfmt::skip]
static PCKBD_KEYDESC_FR_DVORAK_BEPO: &[KeysymT] = &[
    kc(2),   KS_quotedbl,       KS_1,           KS_hyphen,
    kc(3),   KS_guillemotleft,  KS_2,           KS_less,
    kc(4),   KS_guillemotright, KS_3,           KS_greater,
    kc(5),   KS_parenleft,      KS_4,           KS_bracketleft,
    kc(6),   KS_parenright,     KS_5,           KS_bracketright,
    kc(7),   KS_at,             KS_6,
    kc(8),   KS_plus,           KS_7,
    kc(9),   KS_minus,          KS_8,
    kc(10),  KS_slash,          KS_9,
    kc(11),  KS_asterisk,       KS_0,
    kc(12),  KS_equal,          KS_asciicircum,
    kc(13),  KS_percent,        KS_grave,
    kc(16),  KS_b,              KS_B,           KS_bar,
    kc(17),  KS_eacute,         KS_Eacute,      KS_dead_acute,
    kc(18),  KS_p,              KS_P,           KS_ampersand,
    kc(19),  KS_o,              KS_O,
// oe ligature
    kc(20),  KS_egrave,         KS_Egrave,      KS_dead_grave,
    kc(21),  KS_dead_circumflex,KS_exclam,
    kc(22),                     KS_v,
    kc(23),                     KS_d,
    kc(24),                     KS_l,
    kc(25),                     KS_j,
    kc(26),                     KS_z,
    kc(27),                     KS_w,
    kc(30),  KS_a,              KS_A,           KS_ae,          KS_AE,
    kc(31),  KS_u,              KS_U,           KS_ugrave,      KS_Ugrave,
    kc(32),  KS_i,              KS_I,           KS_dead_diaeresis,
    kc(33),  KS_e,              KS_E,
// euro currency
    kc(34),  KS_comma,          KS_semicolon,
    kc(35),                     KS_c,
    kc(36),                     KS_t,
    kc(37),                     KS_s,
    kc(38),                     KS_r,
    kc(39),                     KS_n,
    kc(40),                     KS_m,
    kc(41),  KS_dollar,         KS_numbersign,
    kc(43),  KS_ccedilla,       KS_Ccedilla,
    kc(44),  KS_agrave,         KS_Agrave,      KS_backslash,
    kc(45),  KS_y,              KS_Y,           KS_braceleft,
    kc(46),  KS_x,              KS_X,           KS_braceright,
    kc(47),  KS_period,         KS_colon, // ellipsis
    kc(48),  KS_k,              KS_K,           KS_asciitilde,
    kc(49),  KS_apostrophe,     KS_question,
    kc(50),  KS_q,              KS_Q,
    kc(51),  KS_g,              KS_G,           KS_mu,
    kc(52),                     KS_h,
    kc(53),                     KS_f,
    kc(57),  KS_space,          KS_nobreakspace,KS_underscore,
    kc(86),  KS_egrave,         KS_Egrave,      KS_slash,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_it`.
#[rustfmt::skip]
static PCKBD_KEYDESC_IT: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(3),   KS_2,              KS_quotedbl,    KS_twosuperior,
    kc(4),   KS_3,              KS_sterling,    KS_threesuperior,
    kc(6),   KS_5,              KS_percent,
    kc(7),   KS_6,              KS_ampersand,
    kc(8),   KS_7,              KS_slash,
    kc(9),   KS_8,              KS_parenleft,
    kc(10),  KS_9,              KS_parenright,
    kc(11),  KS_0,              KS_equal,
    kc(12),  KS_apostrophe,     KS_question,
    kc(13),  KS_igrave,         KS_asciicircum,
    kc(26),  KS_egrave,         KS_eacute,      KS_braceleft,   KS_bracketleft,
    kc(27),  KS_plus,           KS_asterisk,    KS_braceright,  KS_bracketright,
    kc(39),  KS_ograve,         KS_Ccedilla,    KS_at,
    kc(40),  KS_agrave,         KS_degree,      KS_numbersign,
    kc(41),  KS_backslash,      KS_bar,
    kc(43),  KS_ugrave,         KS_section,
    kc(51),  KS_comma,          KS_semicolon,
    kc(52),  KS_period,         KS_colon,
    kc(53),  KS_minus,          KS_underscore,
    kc(86),  KS_less,           KS_greater,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_uk`.
#[rustfmt::skip]
static PCKBD_KEYDESC_UK: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(2),   KS_1,              KS_exclam,      KS_plusminus,   KS_exclamdown,
    kc(3),   KS_2,              KS_quotedbl,    KS_twosuperior, KS_cent,
    kc(4),   KS_3,              KS_sterling,    KS_threesuperior,
    kc(5),   KS_4,              KS_dollar,      KS_acute,       KS_currency,
    kc(6),   KS_5,              KS_percent,     KS_mu,          KS_yen,
    kc(7),   KS_6,              KS_asciicircum, KS_paragraph,
    kc(8),   KS_7,              KS_ampersand,   KS_periodcentered, KS_brokenbar,
    kc(9),   KS_8,              KS_asterisk,    KS_cedilla,     KS_ordfeminine,
    kc(10),  KS_9,              KS_parenleft,   KS_onesuperior, KS_diaeresis,
    kc(11),  KS_0,              KS_parenright,  KS_masculine,   KS_copyright,
    kc(12),  KS_minus,          KS_underscore,  KS_hyphen,      KS_ssharp,
    kc(13),  KS_equal,          KS_plus,        KS_onehalf,    KS_guillemotleft,
    kc(40),  KS_apostrophe,     KS_at,          KS_section,     KS_Agrave,
    kc(41),  KS_grave,          KS_grave,       KS_agrave,      KS_agrave,
    kc(43),  KS_numbersign,     KS_asciitilde,  KS_sterling,    KS_thorn,
    kc(86),  KS_backslash,      KS_bar,         KS_Udiaeresis,
];

/// `pckbd_keydesc_jp`.
#[rustfmt::skip]
static PCKBD_KEYDESC_JP: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(3),   KS_2,              KS_quotedbl,
    kc(7),   KS_6,              KS_ampersand,
    kc(8),   KS_7,              KS_apostrophe,
    kc(9),   KS_8,              KS_parenleft,
    kc(10),  KS_9,              KS_parenright,
    kc(11),  KS_0,
    kc(12),  KS_minus,          KS_equal,
    kc(13),  KS_asciicircum,    KS_asciitilde,
    kc(26),  KS_at,             KS_grave,
    kc(27),  KS_bracketleft,    KS_braceleft,
    kc(39),  KS_semicolon,      KS_plus,
    kc(40),  KS_colon,          KS_asterisk,
    kc(41),  KS_Zenkaku_Hankaku, // replace grave/tilde
    kc(43),  KS_bracketright,   KS_braceright,
    kc(112), KS_Hiragana_Katakana,
    kc(115), KS_backslash,      KS_underscore,
    kc(121), KS_Henkan,
    kc(123), KS_Muhenkan,
    kc(125), KS_backslash,      KS_bar,
];

/// `pckbd_keydesc_es`.
#[rustfmt::skip]
static PCKBD_KEYDESC_ES: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(2),   KS_1,              KS_exclam,      KS_bar,
    kc(3),   KS_2,              KS_quotedbl,    KS_at,
    kc(4),   KS_3,              KS_periodcentered, KS_numbersign,
    kc(5),   KS_4,              KS_dollar,      KS_asciitilde,
    kc(7),   KS_6,              KS_ampersand,
    kc(8),   KS_7,              KS_slash,
    kc(9),   KS_8,              KS_parenleft,
    kc(10),  KS_9,              KS_parenright,
    kc(11),  KS_0,              KS_equal,
    kc(12),  KS_apostrophe,     KS_question,
    kc(13),  KS_exclamdown,     KS_questiondown,
    kc(26),  KS_dead_grave,     KS_dead_circumflex, KS_bracketleft,
    kc(27),  KS_plus,           KS_asterisk,    KS_bracketright,
    kc(39),  KS_ntilde,
    kc(40),  KS_dead_acute,     KS_dead_diaeresis, KS_braceleft,
    kc(41),  KS_degree,         KS_ordfeminine, KS_backslash,
    kc(43),  KS_ccedilla,       KS_Ccedilla,    KS_braceright,
    kc(51),  KS_comma,          KS_semicolon,
    kc(52),  KS_period,         KS_colon,
    kc(53),  KS_minus,          KS_underscore,
    kc(86),  KS_less,           KS_greater,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_lt`.
#[rustfmt::skip]
static PCKBD_KEYDESC_LT: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(2),   KS_exclam,         KS_1,           KS_at,
    kc(3),   KS_minus,          KS_2,           KS_underscore,
    kc(4),   KS_slash,          KS_3,           KS_numbersign,
    kc(5),   KS_semicolon,      KS_4,           KS_dollar,
    kc(6),   KS_colon,          KS_5,           KS_paragraph,
    kc(7),   KS_comma,          KS_6,           KS_asciicircum,
    kc(8),   KS_period,         KS_7,           KS_ampersand,
    kc(9),   KS_equal,          KS_8,           KS_asterisk,
    kc(10),  KS_bracketleft,    KS_9,           KS_parenleft,
    kc(11),  KS_bracketright,   KS_0,           KS_parenright,
    kc(12),  KS_question,       KS_plus,        KS_apostrophe,
    kc(13),  KS_x,              KS_X,           KS_percent,
    kc(16),  KS_L7_aogonek,     KS_L7_Aogonek,
    kc(17),  KS_L7_zcaron,      KS_L7_Zcaron,
    kc(18),  KS_e,              KS_E,           KS_currency,
    kc(26),  KS_L7_iogonek,     KS_L7_Iogonek,  KS_braceleft,
    kc(27),  KS_w,              KS_W,           KS_braceright,
    kc(33),  KS_L7_scaron,      KS_L7_Scaron,
    kc(39),  KS_L7_uogonek,     KS_L7_Uogonek,
    kc(40),  KS_L7_edot,        KS_L7_Edot,     KS_quotedbl,
    kc(41),  KS_grave,          KS_asciitilde,
    kc(43),  KS_q,              KS_Q,           KS_bar,
    kc(45),  KS_L7_umacron,     KS_L7_Umacron,
    kc(51),  KS_L7_ccaron,      KS_L7_Ccaron,   KS_L7_dbllow9quot,
    kc(52),  KS_f,              KS_F,           KS_L7_leftdblquot,
    kc(53),  KS_L7_eogonek,     KS_L7_Eogonek,  KS_backslash,
    kc(57),  KS_space,          KS_space,       KS_nobreakspace,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_be`.
#[rustfmt::skip]
static PCKBD_KEYDESC_BE: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(2),   KS_ampersand,      KS_1,           KS_bar,
    kc(3),   KS_eacute,         KS_2,           KS_at,
    kc(4),   KS_quotedbl,       KS_3,           KS_numbersign,
    kc(5),   KS_apostrophe,     KS_4,
    kc(6),   KS_parenleft,      KS_5,
    kc(7),   KS_section,        KS_6,           KS_asciicircum,
    kc(8),   KS_egrave,         KS_7,
    kc(9),   KS_exclam,         KS_8,
    kc(10),  KS_ccedilla,       KS_9,           KS_braceleft,
    kc(11),  KS_agrave,         KS_0,           KS_braceright,
    kc(12),  KS_parenright,     KS_degree,
    kc(13),  KS_minus,          KS_underscore,
    kc(16),  KS_a,
    kc(17),  KS_z,
    kc(26),  KS_dead_circumflex, KS_dead_diaeresis, KS_bracketleft,
    kc(27),  KS_dollar,         KS_asterisk,    KS_bracketright,
    kc(30),  KS_q,
    kc(39),  KS_m,
    kc(40),  KS_ugrave,         KS_percent,     KS_acute,
    kc(41),  KS_twosuperior,    KS_threesuperior,
    kc(43),  KS_mu,             KS_sterling,    KS_grave,
    kc(44),  KS_w,
    kc(50),  KS_comma,          KS_question,
    kc(51),  KS_semicolon,      KS_period,
    kc(52),  KS_colon,          KS_slash,
    kc(53),  KS_equal,          KS_plus,        KS_asciitilde,
    kc(86),  KS_less,           KS_greater,     KS_backslash,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_us_declk`.
#[rustfmt::skip]
static PCKBD_KEYDESC_US_DECLK: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(1),      KS_grave,       KS_asciitilde, // replace escape
    kc(41),     KS_less,        KS_greater, // replace grave/tilde
    kc(143),    KS_Multi_key, // left compose
    kc(157),    KS_Multi_key, // right compose, replace right control
    kc(87),     KS_Cmd_Debugger,        KS_Escape, // replace F11
    kc(189),    KS_f13,
    kc(190),    KS_f14,
    kc(191),    KS_Help,
    kc(192),    KS_Execute,
    kc(193),    KS_f17,
    kc(183),    KS_f18,
    kc(70),     KS_f19, // replace scroll lock
    kc(127),    KS_f20, // replace break
    kc(69),     KS_KP_F1, // replace num lock
    kc(181),    KS_KP_F2, // replace divide
    kc(55),     KS_KP_F3, // replace multiply
    kc(74),     KS_KP_F4, // replace subtract

// keypad is numbers only - no num lock
    kc(71),     KS_KP_7,
    kc(72),     KS_KP_8,
    kc(73),     KS_KP_9,
    kc(75),     KS_KP_4,
    kc(76),     KS_KP_5,
    kc(77),     KS_KP_6,
    kc(79),     KS_KP_1,
    kc(80),     KS_KP_2,
    kc(81),     KS_KP_3,
    kc(82),     KS_KP_0,
    kc(83),     KS_KP_Decimal,

    kc(206),    KS_KP_Subtract,
    kc(78),     KS_KP_Separator, // replace add
    kc(199),    KS_Find, // replace home
    kc(207),    KS_Select, // replace end
];

/// `pckbd_keydesc_us_dvorak`.
#[rustfmt::skip]
static PCKBD_KEYDESC_US_DVORAK: &[KeysymT] = &[
// pos      command            normal          shifted
    kc(12),                     KS_bracketleft, KS_braceleft,
    kc(13),                     KS_bracketright, KS_braceright,
    kc(16),                     KS_apostrophe, KS_quotedbl,
    kc(17),                     KS_comma, KS_less,
    kc(18),                     KS_period, KS_greater,
    kc(19),                     KS_p,
    kc(20),                     KS_y,
    kc(21),                     KS_f,
    kc(22),                     KS_g,
    kc(23),                     KS_c,
    kc(24),                     KS_r,
    kc(25),                     KS_l,
    kc(26),                     KS_slash, KS_question,
    kc(27),                     KS_equal, KS_plus,
    kc(31),                     KS_o,
    kc(32),                     KS_e,
    kc(33),                     KS_u,
    kc(34),                     KS_i,
    kc(35),                     KS_d,
    kc(36),                     KS_h,
    kc(37),                     KS_t,
    kc(38),                     KS_n,
    kc(39),                     KS_s,
    kc(40),                     KS_minus, KS_underscore,
    kc(44),                     KS_semicolon, KS_colon,
    kc(45),                     KS_q,
    kc(46),                     KS_j,
    kc(47),                     KS_k,
    kc(48),                     KS_x,
    kc(49),                     KS_b,
    kc(51),                     KS_w,
    kc(52),                     KS_v,
    kc(53),                     KS_z,
];

/// `pckbd_keydesc_us_colemak`.
#[rustfmt::skip]
static PCKBD_KEYDESC_US_COLEMAK: &[KeysymT] = &[
// pos      command            normal          shifted
    kc(18),                     KS_f,
    kc(19),                     KS_p,
    kc(20),                     KS_g,
    kc(21),                     KS_j,
    kc(22),                     KS_l,
    kc(23),                     KS_u,
    kc(24),                     KS_y,
    kc(25),                     KS_semicolon,   KS_colon,
    kc(31),                     KS_r,
    kc(32),                     KS_s,
    kc(33),                     KS_t,
    kc(34),                     KS_d,
    kc(36),                     KS_n,
    kc(37),                     KS_e,
    kc(38),                     KS_i,           KS_I,
    kc(39),                     KS_o,
    kc(49),                     KS_k,
];

/// `pckbd_keydesc_swapctrlcaps`.
#[rustfmt::skip]
static PCKBD_KEYDESC_SWAPCTRLCAPS: &[KeysymT] = &[
// pos      command            normal          shifted
    kc(29),                     KS_Caps_Lock,
    kc(58),  KS_Cmd1,           KS_Control_L,
];

/// `pckbd_keydesc_ru`.
#[rustfmt::skip]
static PCKBD_KEYDESC_RU: &[KeysymT] = &[
// pos      normal             shifted         altgr                   shift-altgr
    kc(7),   KS_6,              KS_asciicircum, KS_6,                   KS_comma,
    kc(8),   KS_7,              KS_ampersand,   KS_7,                   KS_period,
    kc(16),  KS_q,              KS_Q,           KS_Cyrillic_ishort,     KS_Cyrillic_ISHORT,
    kc(17),  KS_w,              KS_W,           KS_Cyrillic_tse,        KS_Cyrillic_TSE,
    kc(18),  KS_e,              KS_E,           KS_Cyrillic_u,          KS_Cyrillic_U,
    kc(19),  KS_r,              KS_R,           KS_Cyrillic_ka,         KS_Cyrillic_KA,
    kc(20),  KS_t,              KS_T,           KS_Cyrillic_ie,         KS_Cyrillic_IE,
    kc(21),  KS_y,              KS_Y,           KS_Cyrillic_en,         KS_Cyrillic_EN,
    kc(22),  KS_u,              KS_U,           KS_Cyrillic_ge,         KS_Cyrillic_GE,
    kc(23),  KS_i,              KS_I,           KS_Cyrillic_sha,        KS_Cyrillic_SHA,
    kc(24),  KS_o,              KS_O,           KS_Cyrillic_scha,       KS_Cyrillic_SCHA,
    kc(25),  KS_p,              KS_P,           KS_Cyrillic_ze,         KS_Cyrillic_ZE,
    kc(26),  KS_bracketleft,    KS_braceleft,   KS_Cyrillic_ha,         KS_Cyrillic_HA,
    kc(27),  KS_bracketright,   KS_braceright,  KS_Cyrillic_hsighn,     KS_Cyrillic_HSIGHN,
    kc(30),  KS_a,              KS_A,           KS_Cyrillic_ef,         KS_Cyrillic_EF,
    kc(31),  KS_s,              KS_S,           KS_Cyrillic_yeru,       KS_Cyrillic_YERU,
    kc(32),  KS_d,              KS_D,           KS_Cyrillic_ve,         KS_Cyrillic_VE,
    kc(33),  KS_f,              KS_F,           KS_Cyrillic_a,          KS_Cyrillic_A,
    kc(34),  KS_g,              KS_G,           KS_Cyrillic_pe,         KS_Cyrillic_PE,
    kc(35),  KS_h,              KS_H,           KS_Cyrillic_er,         KS_Cyrillic_ER,
    kc(36),  KS_j,              KS_J,           KS_Cyrillic_o,          KS_Cyrillic_O,
    kc(37),  KS_k,              KS_K,           KS_Cyrillic_el,         KS_Cyrillic_EL,
    kc(38),  KS_l,              KS_L,           KS_Cyrillic_de,         KS_Cyrillic_DE,
    kc(39),  KS_semicolon,      KS_colon,       KS_Cyrillic_zhe,        KS_Cyrillic_ZHE,
    kc(40),  KS_apostrophe,     KS_quotedbl,    KS_Cyrillic_e,          KS_Cyrillic_E,
    kc(44),  KS_z,              KS_Z,           KS_Cyrillic_ya,         KS_Cyrillic_YA,
    kc(45),  KS_x,              KS_X,           KS_Cyrillic_che,        KS_Cyrillic_CHE,
    kc(46),  KS_c,              KS_C,           KS_Cyrillic_es,         KS_Cyrillic_ES,
    kc(47),  KS_v,              KS_V,           KS_Cyrillic_em,         KS_Cyrillic_EM,
    kc(48),  KS_b,              KS_B,           KS_Cyrillic_i,          KS_Cyrillic_I,
    kc(49),  KS_n,              KS_N,           KS_Cyrillic_te,         KS_Cyrillic_TE,
    kc(50),  KS_m,              KS_M,           KS_Cyrillic_ssighn,     KS_Cyrillic_SSIGHN,
    kc(51),  KS_comma,          KS_less,        KS_Cyrillic_be,         KS_Cyrillic_BE,
    kc(52),  KS_period,         KS_greater,     KS_Cyrillic_yu,         KS_Cyrillic_YU,
    kc(53),  KS_slash,          KS_question,    KS_Cyrillic_yo,         KS_Cyrillic_YO,
    kc(184), KS_Mode_switch,   KS_Multi_key,
];

/// `pckbd_keydesc_ua`.
#[rustfmt::skip]
static PCKBD_KEYDESC_UA: &[KeysymT] = &[
// pos      normal             shifted         altgr                   shift-altgr
    kc(7),   KS_6,              KS_asciicircum, KS_6,                   KS_comma,
    kc(8),   KS_7,              KS_ampersand,   KS_7,                   KS_period,
    kc(12),   KS_minus,         KS_underscore,  KS_Cyrillic_iukr,                       KS_Cyrillic_IUKR,
    kc(13),   KS_equal,         KS_plus,        KS_Cyrillic_yeukr,                      KS_Cyrillic_YEUKR,
    kc(16),  KS_q,              KS_Q,           KS_Cyrillic_ishort,     KS_Cyrillic_ISHORT,
    kc(17),  KS_w,              KS_W,           KS_Cyrillic_tse,        KS_Cyrillic_TSE,
    kc(18),  KS_e,              KS_E,           KS_Cyrillic_u,          KS_Cyrillic_U,
    kc(19),  KS_r,              KS_R,           KS_Cyrillic_ka,         KS_Cyrillic_KA,
    kc(20),  KS_t,              KS_T,           KS_Cyrillic_ie,         KS_Cyrillic_IE,
    kc(21),  KS_y,              KS_Y,           KS_Cyrillic_en,         KS_Cyrillic_EN,
    kc(22),  KS_u,              KS_U,           KS_Cyrillic_ge,         KS_Cyrillic_GE,
    kc(23),  KS_i,              KS_I,           KS_Cyrillic_sha,        KS_Cyrillic_SHA,
    kc(24),  KS_o,              KS_O,           KS_Cyrillic_scha,       KS_Cyrillic_SCHA,
    kc(25),  KS_p,              KS_P,           KS_Cyrillic_ze,         KS_Cyrillic_ZE,
    kc(26),  KS_bracketleft,    KS_braceleft,   KS_Cyrillic_ha,         KS_Cyrillic_HA,
    kc(27),  KS_bracketright,   KS_braceright,  KS_Cyrillic_hsighn,     KS_Cyrillic_HSIGHN,
    kc(30),  KS_a,              KS_A,           KS_Cyrillic_ef,         KS_Cyrillic_EF,
    kc(31),  KS_s,              KS_S,           KS_Cyrillic_yeru,       KS_Cyrillic_YERU,
    kc(32),  KS_d,              KS_D,           KS_Cyrillic_ve,         KS_Cyrillic_VE,
    kc(33),  KS_f,              KS_F,           KS_Cyrillic_a,          KS_Cyrillic_A,
    kc(34),  KS_g,              KS_G,           KS_Cyrillic_pe,         KS_Cyrillic_PE,
    kc(35),  KS_h,              KS_H,           KS_Cyrillic_er,         KS_Cyrillic_ER,
    kc(36),  KS_j,              KS_J,           KS_Cyrillic_o,          KS_Cyrillic_O,
    kc(37),  KS_k,              KS_K,           KS_Cyrillic_el,         KS_Cyrillic_EL,
    kc(38),  KS_l,              KS_L,           KS_Cyrillic_de,         KS_Cyrillic_DE,
    kc(39),  KS_semicolon,      KS_colon,       KS_Cyrillic_zhe,        KS_Cyrillic_ZHE,
    kc(40),  KS_apostrophe,     KS_quotedbl,    KS_Cyrillic_e,          KS_Cyrillic_E,
    kc(41),  KS_grave,  KS_asciitilde,  KS_Cyrillic_gheukr,             KS_Cyrillic_GHEUKR,
    kc(43),  KS_backslash,      KS_bar, KS_Cyrillic_yi,         KS_Cyrillic_YI,
    kc(44),  KS_z,              KS_Z,           KS_Cyrillic_ya,         KS_Cyrillic_YA,
    kc(45),  KS_x,              KS_X,           KS_Cyrillic_che,        KS_Cyrillic_CHE,
    kc(46),  KS_c,              KS_C,           KS_Cyrillic_es,         KS_Cyrillic_ES,
    kc(47),  KS_v,              KS_V,           KS_Cyrillic_em,         KS_Cyrillic_EM,
    kc(48),  KS_b,              KS_B,           KS_Cyrillic_i,          KS_Cyrillic_I,
    kc(49),  KS_n,              KS_N,           KS_Cyrillic_te,         KS_Cyrillic_TE,
    kc(50),  KS_m,              KS_M,           KS_Cyrillic_ssighn,     KS_Cyrillic_SSIGHN,
    kc(51),  KS_comma,          KS_less,        KS_Cyrillic_be,         KS_Cyrillic_BE,
    kc(52),  KS_period,         KS_greater,     KS_Cyrillic_yu,         KS_Cyrillic_YU,
    kc(53),  KS_slash,          KS_question,    KS_Cyrillic_yo,         KS_Cyrillic_YO,
    kc(184), KS_Mode_switch,   KS_Multi_key,
];

/// `pckbd_keydesc_sg`.
#[rustfmt::skip]
static PCKBD_KEYDESC_SG: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(2),   KS_1,              KS_plus,        KS_bar,
    kc(3),   KS_2,              KS_quotedbl,    KS_at,
    kc(4),   KS_3,              KS_asterisk,    KS_numbersign,
    kc(5),   KS_4,              KS_ccedilla,
    kc(7),   KS_6,              KS_ampersand,   KS_notsign,
    kc(8),   KS_7,              KS_slash,       KS_brokenbar,
    kc(9),   KS_8,              KS_parenleft,   KS_cent,
    kc(10),  KS_9,              KS_parenright,
    kc(11),  KS_0,              KS_equal,
    kc(12),  KS_apostrophe,     KS_question,    KS_dead_acute,
    kc(13),  KS_dead_circumflex,KS_dead_grave,  KS_dead_tilde,
    kc(18),  KS_e,              KS_E,           KS_currency,
    kc(21),  KS_z,
    kc(26),  KS_udiaeresis,     KS_egrave,      KS_bracketleft,
    kc(27),  KS_dead_diaeresis, KS_exclam,      KS_bracketright,
    kc(39),  KS_odiaeresis,     KS_eacute,
    kc(40),  KS_adiaeresis,     KS_agrave,      KS_braceleft,
    kc(41),  KS_section,        KS_degree,      KS_dead_abovering,
    kc(43),  KS_dollar,         KS_sterling,    KS_braceright,
    kc(44),  KS_y,
    kc(51),  KS_comma,          KS_semicolon,
    kc(52),  KS_period,         KS_colon,
    kc(53),  KS_minus,          KS_underscore,
    kc(86),  KS_less,           KS_greater,     KS_backslash,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_sg_nodead`.
#[rustfmt::skip]
static PCKBD_KEYDESC_SG_NODEAD: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(12),  KS_apostrophe,     KS_question,    KS_acute,
    kc(13),  KS_asciicircum,    KS_grave,       KS_asciitilde,
    kc(27),  KS_diaeresis,      KS_exclam,      KS_bracketright
];

/// `pckbd_keydesc_sf`.
#[rustfmt::skip]
static PCKBD_KEYDESC_SF: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(26),  KS_egrave,         KS_udiaeresis,  KS_bracketleft,
    kc(39),  KS_eacute,         KS_odiaeresis,
    kc(40),  KS_agrave,         KS_adiaeresis,  KS_braceleft
];

/// `pckbd_keydesc_pt`.
#[rustfmt::skip]
static PCKBD_KEYDESC_PT: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(3),   KS_2,              KS_quotedbl,    KS_at,
    kc(4),   KS_3,              KS_numbersign,  KS_sterling,
    kc(7),   KS_6,              KS_ampersand,
    kc(8),   KS_7,              KS_slash,       KS_braceleft,
    kc(9),   KS_8,              KS_parenleft,   KS_bracketleft,
    kc(10),  KS_9,              KS_parenright,  KS_bracketright,
    kc(11),  KS_0,              KS_equal,       KS_braceright,
    kc(12),  KS_apostrophe,     KS_question,
    kc(13),  KS_less,           KS_greater,
    kc(26),  KS_plus,           KS_asterisk,
    kc(27),  KS_dead_acute,     KS_dead_grave,
    kc(39),  KS_ccedilla,       KS_Ccedilla,
    kc(40),  KS_masculine,      KS_ordfeminine,
    kc(41),  KS_backslash,      KS_bar,
    kc(43),  KS_dead_tilde,     KS_dead_circumflex,
    kc(51),  KS_comma,          KS_semicolon,
    kc(52),  KS_period,         KS_colon,
    kc(53),  KS_minus,          KS_underscore,
    kc(86),  KS_less,           KS_greater,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_la`.
#[rustfmt::skip]
static PCKBD_KEYDESC_LA: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(2),   KS_1,              KS_exclam,
    kc(3),   KS_2,              KS_quotedbl,
    kc(4),   KS_3,              KS_numbersign,
    kc(7),   KS_6,              KS_ampersand,
    kc(8),   KS_7,              KS_slash,
    kc(9),   KS_8,              KS_parenleft,
    kc(10),  KS_9,              KS_parenright,
    kc(11),  KS_0,              KS_equal,
    kc(12),  KS_apostrophe,     KS_question,    KS_backslash,
    kc(13),  KS_questiondown,   KS_exclamdown,
    kc(16),  KS_q,              KS_Q,           KS_at,
    kc(26),  KS_dead_acute,     KS_dead_diaeresis,
    kc(27),  KS_plus,           KS_asterisk,    KS_asciitilde,
    kc(39),  KS_ntilde,
    kc(40),  KS_braceleft,      KS_bracketleft, KS_dead_circumflex,
    kc(41),  KS_bar,            KS_degree,      KS_notsign,
    kc(43),  KS_braceright,     KS_bracketright,KS_dead_grave,
    kc(51),  KS_comma,          KS_semicolon,
    kc(52),  KS_period,         KS_colon,
    kc(53),  KS_minus,          KS_underscore,
    kc(86),  KS_less,           KS_greater,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_br`.
#[rustfmt::skip]
static PCKBD_KEYDESC_BR: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(2),   KS_1,              KS_exclam,      KS_onesuperior,
    kc(3),   KS_2,              KS_at,          KS_twosuperior,
    kc(4),   KS_3,              KS_numbersign,  KS_threesuperior,
    kc(5),   KS_4,              KS_dollar,      KS_sterling,
    kc(6),   KS_5,              KS_percent,     KS_cent,
    kc(7),   KS_6,              KS_dead_diaeresis,      KS_notsign,
    kc(13),  KS_equal,          KS_plus,        KS_section,
    kc(26),  KS_dead_acute,     KS_dead_grave,
    kc(27),  KS_bracketleft,    KS_braceleft,   KS_ordfeminine,
    kc(39),  KS_ccedilla,       KS_Ccedilla,
    kc(40),  KS_dead_tilde,     KS_dead_circumflex,
    kc(41),  KS_apostrophe,     KS_quotedbl,
    kc(43),  KS_bracketright,   KS_braceright,  KS_masculine,
    kc(53),  KS_semicolon,      KS_colon,
    kc(83),  KS_KP_Delete,      KS_KP_Decimal,
    kc(86),  KS_backslash,      KS_bar,
    kc(115), KS_slash,          KS_question,    KS_degree,
];

/// `pckbd_keydesc_tr`.
#[rustfmt::skip]
static PCKBD_KEYDESC_TR: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(3),   KS_2,              KS_apostrophe,  KS_sterling,
    kc(4),   KS_3,              KS_asciicircum, KS_numbersign,
    kc(5),   KS_4,              KS_plus,        KS_dollar,
    kc(6),   KS_5,              KS_percent,     KS_onehalf,
    kc(7),   KS_6,              KS_ampersand,
    kc(8),   KS_7,              KS_slash,       KS_braceleft,
    kc(9),   KS_8,              KS_parenleft,   KS_bracketleft,
    kc(10),  KS_9,              KS_parenright,  KS_bracketright,
    kc(11),  KS_0,              KS_equal,       KS_braceright,
    kc(12),  KS_asterisk,       KS_question,    KS_backslash,
    kc(13),  KS_minus,          KS_underscore,
    kc(16),  KS_q,              KS_Q,           KS_at,
    kc(23),  KS_L5_idotless,    KS_I,
    kc(26),  KS_L5_gbreve,      KS_L5_Gbreve,   KS_dead_diaeresis,
    kc(27),  KS_udiaeresis,     KS_Udiaeresis,  KS_asciitilde,
    kc(39),  KS_L5_scedilla,    KS_L5_Scedilla, KS_dead_acute,
    kc(40),  KS_i,              KS_L5_Idotabove,
    kc(41),  KS_quotedbl,       KS_eacute,
    kc(43),  KS_comma,          KS_semicolon,   KS_dead_grave,
    kc(51),  KS_odiaeresis,     KS_Odiaeresis,
    kc(52),  KS_ccedilla,       KS_Ccedilla,
    kc(53),  KS_period, KS_colon,
    kc(86),  KS_less,           KS_greater,     KS_bar,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_tr_nodead`.
#[rustfmt::skip]
static PCKBD_KEYDESC_TR_NODEAD: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(26),  KS_L5_gbreve,      KS_L5_Gbreve,
    kc(39),  KS_L5_scedilla,    KS_L5_Scedilla, KS_apostrophe,
    kc(43),  KS_comma,          KS_semicolon,   KS_grave,
];

/// `pckbd_keydesc_pl`.
#[rustfmt::skip]
static PCKBD_KEYDESC_PL: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(18),  KS_e,              KS_E,           KS_L2_eogonek,  KS_L2_Eogonek,
    kc(24),  KS_o,              KS_O,           KS_oacute,      KS_Oacute,
    kc(30),  KS_a,              KS_A,           KS_L2_aogonek,  KS_L2_Aogonek,
    kc(31),  KS_s,              KS_S,           KS_L2_sacute,   KS_L2_Sacute,
    kc(38),  KS_l,              KS_L,           KS_L2_lstroke,  KS_L2_Lstroke,
    kc(44),  KS_z,              KS_Z,           KS_L2_zdotabove,KS_L2_Zdotabove,
    kc(45),  KS_x,              KS_X,           KS_L2_zacute,   KS_L2_Zacute,
    kc(46),  KS_c,              KS_C,           KS_L2_cacute,   KS_L2_Cacute,
    kc(49),  KS_n,              KS_N,           KS_L2_nacute,   KS_L2_Nacute,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_hu`.
#[rustfmt::skip]
static PCKBD_KEYDESC_HU: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(2),   KS_1,              KS_apostrophe,  KS_asciitilde,
    kc(3),   KS_2,              KS_quotedbl,
    kc(4),   KS_3,              KS_plus,        KS_asciicircum,
    kc(5),   KS_4,              KS_exclam,
    kc(6),   KS_5,              KS_percent,
    kc(7),   KS_6,              KS_slash,
    kc(8),   KS_7,              KS_equal,KS_grave,
    kc(9),   KS_8,              KS_parenleft,
    kc(10),  KS_9,              KS_parenright,  KS_acute,
    kc(11),  KS_odiaeresis,     KS_Odiaeresis,
    kc(12),  KS_udiaeresis,     KS_Udiaeresis,
    kc(13),  KS_oacute,         KS_Oacute,
    kc(16),  KS_q,              KS_Q,           KS_backslash,
    kc(17),  KS_w,              KS_W,KS_bar,
    kc(21),  KS_z,              KS_Z,
    kc(23),  KS_i,              KS_I,           KS_iacute,
    kc(26),  KS_odoubleacute,   KS_Odoubleacute,        KS_division,
    kc(27),  KS_uacute,         KS_Uacute,      KS_multiply,
    kc(33),  KS_f,              KS_F,           KS_bracketleft,
    kc(34),  KS_g,              KS_G,           KS_bracketright,
    kc(36),  KS_j,              KS_J,           KS_iacute,
    kc(39),  KS_eacute,         KS_Eacute,      KS_dollar,
    kc(40),  KS_aacute,         KS_Aacute,      KS_ssharp,
    kc(41),  KS_0,              KS_section,
    kc(43),  KS_udoubleacute,   KS_Udoubleacute,        KS_currency,
    kc(44),  KS_y,              KS_Y,           KS_greater,
    kc(45),  KS_x,              KS_X,           KS_numbersign,
    kc(46),  KS_c,              KS_C,           KS_ampersand,
    kc(47),  KS_v,              KS_V,           KS_at,
    kc(48),  KS_b,              KS_B,           KS_braceleft,
    kc(49),  KS_n,              KS_N,           KS_braceright,
    kc(51),  KS_comma,          KS_question,    KS_semicolon,
    kc(52),  KS_period,         KS_colon,
    kc(53),  KS_minus,          KS_underscore,  KS_asterisk,
    kc(86),  KS_iacute,         KS_Iacute,      KS_less,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_si`.
#[rustfmt::skip]
static PCKBD_KEYDESC_SI: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(2),   KS_1,              KS_exclam,      KS_asciitilde,
    kc(3),   KS_2,              KS_quotedbl,    KS_L2_caron,
    kc(4),   KS_3,              KS_numbersign,  KS_asciicircum,
    kc(5),   KS_4,              KS_dollar,      KS_L2_breve,
    kc(6),   KS_5,              KS_percent,     KS_degree,
    kc(7),   KS_6,              KS_ampersand,   KS_L2_ogonek,
    kc(8),   KS_7,              KS_slash,       KS_grave,
    kc(9),   KS_8,              KS_parenleft,   KS_L2_dotabove,
    kc(10),  KS_9,              KS_parenright,  KS_acute,
    kc(11),  KS_0,              KS_equal,       KS_L2_dblacute,
    kc(12),  KS_apostrophe,     KS_question,    KS_diaeresis,
    kc(13),  KS_plus,           KS_asterisk,    KS_cedilla,
    kc(16),  KS_q,              KS_Q,           KS_backslash,
    kc(17),  KS_w,              KS_W,           KS_bar,
    kc(21),  KS_z,              KS_Z,
    kc(26),  KS_L2_scaron,      KS_L2_Scaron,   KS_division,
    kc(27),  KS_L2_dstroke,     KS_L2_Dstroke,  KS_multiply,
    kc(33),  KS_f,              KS_F,           KS_bracketleft,
    kc(34),  KS_g,              KS_G,           KS_bracketright,
    kc(37),  KS_k,              KS_K,           KS_L2_lstroke,
    kc(38),  KS_l,              KS_L,           KS_L2_Lstroke,
    kc(39),  KS_L2_ccaron,      KS_L2_Ccaron,
    kc(40),  KS_L2_cacute,      KS_L2_Cacute,   KS_ssharp,
    kc(41),  KS_cedilla,        KS_diaeresis,
    kc(43),  KS_L2_zcaron,      KS_L2_Zcaron,   KS_currency,
    kc(44),  KS_y,              KS_Y,
    kc(47),  KS_v,              KS_V,           KS_at,
    kc(48),  KS_b,              KS_B,           KS_braceleft,
    kc(49),  KS_n,              KS_N,           KS_braceright,
    kc(50),  KS_m,              KS_M,           KS_section,
    kc(51),  KS_comma,          KS_semicolon,   KS_less,
    kc(52),  KS_period,         KS_colon,       KS_greater,
    kc(53),  KS_minus,          KS_underscore,
    kc(86),  KS_less,           KS_greater,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_cf`.
#[rustfmt::skip]
static PCKBD_KEYDESC_CF: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
        kc(2),   KS_1,          KS_exclam,      KS_plusminus,
        kc(3),   KS_2,          KS_quotedbl,    KS_at,
        kc(4),   KS_3,          KS_slash,       KS_sterling,
        kc(5),   KS_4,          KS_dollar,      KS_cent,
        kc(6),   KS_5,          KS_percent,     KS_diaeresis,
        kc(7),   KS_6,          KS_question,    KS_macron,
        kc(8),   KS_7,          KS_ampersand,   KS_brokenbar,
        kc(9),   KS_8,          KS_asterisk,    KS_twosuperior,
        kc(10),  KS_9,          KS_parenleft,   KS_threesuperior,
        kc(11),  KS_0,          KS_parenright,  KS_onequarter,
        kc(12),  KS_minus,      KS_underscore,  KS_onehalf,
        kc(13),  KS_equal,      KS_plus,        KS_threequarters,
        kc(24),  KS_o,          KS_O,           KS_section,
        kc(25),  KS_p,          KS_P,           KS_paragraph,
        kc(26),  KS_dead_circumflex,KS_dead_circumflex, KS_bracketleft,
        kc(27),  KS_dead_cedilla,KS_dead_diaeresis, KS_bracketright,
        kc(39),  KS_semicolon,  KS_colon,       KS_asciitilde,
        kc(40),  KS_dead_grave, KS_dead_grave,  KS_braceleft,
        kc(41),  KS_numbersign, KS_bar,         KS_backslash,
        kc(43),  KS_less,       KS_greater,     KS_braceright,
        kc(50),  KS_m,          KS_M,           KS_mu,
        kc(51),  KS_comma,      KS_apostrophe,  KS_hyphen,
        kc(52),  KS_period,     KS_period,
        kc(53),  KS_eacute,     KS_Eacute,      KS_dead_acute,
        kc(86),  KS_guillemotleft,KS_guillemotright, KS_degree,
        kc(184), KS_Mode_switch,KS_Multi_key,
];

/// `pckbd_keydesc_cf_nodead`.
#[rustfmt::skip]
static PCKBD_KEYDESC_CF_NODEAD: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
        kc(26),  KS_asciicircum,KS_asciicircum, KS_bracketleft,
        kc(27),  KS_cedilla,    KS_diaeresis,   KS_bracketright,
        kc(40),  KS_grave,      KS_grave,       KS_braceleft,
        kc(53),  KS_eacute,     KS_Eacute,      KS_acute,
];

/// `pckbd_keydesc_lv`.
#[rustfmt::skip]
static PCKBD_KEYDESC_LV: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(18),  KS_e,              KS_E,           KS_L7_emacron,  KS_L7_Emacron,
    kc(22),  KS_u,              KS_U,           KS_L7_umacron,  KS_L7_Umacron,
    kc(23),  KS_i,              KS_I,           KS_L7_imacron,  KS_L7_Imacron,
    kc(24),  KS_o,              KS_O,           KS_L7_omacron,  KS_L7_Omacron,
    kc(30),  KS_a,              KS_A,           KS_L7_amacron,  KS_L7_Amacron,
    kc(31),  KS_s,              KS_S,           KS_L7_scaron,   KS_L7_Scaron,
    kc(34),  KS_g,              KS_G,           KS_L7_gcedilla, KS_L7_Gcedilla,
    kc(37),  KS_k,              KS_K,           KS_L7_kcedilla, KS_L7_Kcedilla,
    kc(38),  KS_l,              KS_L,           KS_L7_lcedilla, KS_L7_Lcedilla,
    kc(44),  KS_z,              KS_Z,           KS_L7_zcaron,   KS_L7_Zcaron,
    kc(46),  KS_c,              KS_C,           KS_L7_ccaron,   KS_L7_Ccaron,
    kc(49),  KS_n,              KS_N,           KS_L7_ncedilla, KS_L7_Ncedilla,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_nl`.
#[rustfmt::skip]
static PCKBD_KEYDESC_NL: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(2),   KS_1,              KS_exclam,      KS_onesuperior,
    kc(3),   KS_2,              KS_quotedbl,    KS_twosuperior,
    kc(4),   KS_3,              KS_numbersign,  KS_threesuperior,
    kc(5),   KS_4,              KS_dollar,      KS_onequarter,
    kc(6),   KS_5,              KS_percent,     KS_onehalf,
    kc(7),   KS_6,              KS_ampersand,   KS_threequarters,
    kc(8),   KS_7,              KS_underscore,  KS_sterling,
    kc(9),   KS_8,              KS_parenleft,   KS_braceleft,
    kc(10),  KS_9,              KS_parenright,  KS_braceright,
    kc(11),  KS_0,              KS_apostrophe,
    kc(12),  KS_slash,          KS_question,    KS_backslash,
    kc(13),  KS_degree,         KS_dead_tilde,  KS_dead_cedilla,
    kc(19),  KS_r,              KS_R,           KS_paragraph,
    kc(26),  KS_dead_diaeresis, KS_dead_circumflex,
    kc(27),  KS_asterisk,       KS_bar,
    kc(31),  KS_s,              KS_S,           KS_ssharp,
    kc(39),  KS_plus,           KS_plusminus,
    kc(40),  KS_dead_acute,     KS_dead_grave,
    kc(41),  KS_at,             KS_section,     KS_notsign,
    kc(43),  KS_less,           KS_greater,
    kc(44),  KS_z,              KS_Z,           KS_guillemotleft,
    kc(45),  KS_x,              KS_X,           KS_guillemotright,
    kc(46),  KS_c,              KS_C,           KS_cent,
    kc(50),  KS_m,              KS_M,           KS_mu,
    kc(51),  KS_comma,          KS_semicolon,
    kc(52),  KS_period,         KS_colon,       KS_periodcentered,
    kc(53),  KS_minus,          KS_equal,
    kc(86),  KS_bracketright,   KS_bracketleft, KS_brokenbar,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_nl_nodead`.
#[rustfmt::skip]
static PCKBD_KEYDESC_NL_NODEAD: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(13),  KS_degree,         KS_asciitilde,  KS_cedilla,
    kc(26),  KS_quotedbl,       KS_asciicircum,
    kc(40),  KS_apostrophe,     KS_grave,
];

/// `pckbd_keydesc_is`.
#[rustfmt::skip]
static PCKBD_KEYDESC_IS: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(3),   KS_2,              KS_quotedbl,
    kc(7),   KS_6,              KS_ampersand,
    kc(8),   KS_7,              KS_slash,       KS_braceleft,
    kc(9),   KS_8,              KS_parenleft,   KS_bracketleft,
    kc(10),  KS_9,              KS_parenright,  KS_bracketright,
    kc(11),  KS_0,              KS_equal,       KS_braceright,
    kc(12),  KS_odiaeresis,     KS_Odiaeresis,  KS_backslash,
    kc(13),  KS_minus,          KS_underscore,
    kc(16),  KS_q,              KS_Q,           KS_at,
    kc(18),  KS_e,              KS_E,
// euro currency
    kc(26),  KS_eth,
    kc(27),  KS_apostrophe,     KS_question,    KS_asciitilde,
    kc(39),  KS_ae,
    kc(40),  KS_dead_acute,     KS_dead_diaeresis, KS_dead_circumflex,
    kc(41),  KS_degree,         KS_diaeresis,
    kc(43),  KS_plus,           KS_asterisk,    KS_grave,
    kc(51),  KS_comma,          KS_semicolon,
    kc(52),  KS_period,         KS_colon,
    kc(53),  KS_thorn,
    kc(86),  KS_less,           KS_greater,     KS_bar,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_is_nodead`.
#[rustfmt::skip]
static PCKBD_KEYDESC_IS_NODEAD: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(40),  KS_acute,          KS_diaeresis,   KS_asciicircum,
];

/// `pckbd_keydesc_ee`.
#[rustfmt::skip]
static PCKBD_KEYDESC_EE: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(3),   KS_2,              KS_quotedbl,    KS_at,
    kc(4),   KS_3,              KS_numbersign,  KS_sterling,
    kc(5),   KS_4,              KS_currency,    KS_dollar,
    kc(7),   KS_6,              KS_ampersand,
    kc(8),   KS_7,              KS_slash,       KS_braceleft,
    kc(9),   KS_8,              KS_parenleft,   KS_bracketleft,
    kc(10),  KS_9,              KS_parenright,  KS_bracketright,
    kc(11),  KS_0,              KS_equal,       KS_braceright,
    kc(12),  KS_plus,           KS_question,    KS_backslash,
    kc(13),  KS_dead_acute,     KS_dead_grave,
    kc(26),  KS_udiaeresis,
    kc(27),  KS_otilde,         KS_Otilde,      KS_section,
    kc(31),  KS_s,              KS_S,           KS_L2_scaron,   KS_L2_Scaron,
    kc(39),  KS_odiaeresis,
    kc(40),  KS_adiaeresis,     KS_Adiaeresis,  KS_dead_circumflex,
    kc(41),  KS_dead_caron,     KS_dead_tilde,
    kc(43),  KS_apostrophe,     KS_asterisk,    KS_onehalf,
    kc(44),  KS_z,              KS_Z,           KS_L2_zcaron,   KS_L2_Zcaron,
    kc(51),  KS_comma,          KS_semicolon,
    kc(52),  KS_period,         KS_colon,
    kc(53),  KS_minus,          KS_underscore,
    kc(86),  KS_less,           KS_greater,     KS_bar,         KS_brokenbar,
    kc(184), KS_Mode_switch,    KS_Multi_key,
];

/// `pckbd_keydesc_ee_nodead`.
#[rustfmt::skip]
static PCKBD_KEYDESC_EE_NODEAD: &[KeysymT] = &[
// pos      normal             shifted         altgr           shift-altgr
    kc(13),  KS_apostrophe,     KS_grave,
    kc(40),  KS_adiaeresis,     KS_Adiaeresis,  KS_asciicircum,
    kc(41),  KS_L2_caron,       KS_asciitilde,
];

// #endif  /* WSKBD_NO_INTL_LAYOUTS */


/// `pckbd_keydesctab`: the layouts of a PS/2 keyboard, each the keymap of an encoding or a
/// variant over its base (the C's `{0, 0, 0, 0}` terminator is the end of the slice).
pub static PCKBD_KEYDESCTAB: [WsconsKeydesc; 51] = [
    kbd_map(KB_US, 0, PCKBD_KEYDESC_US),
    // #if !defined(WSKBD_NO_INTL_LAYOUTS)
    kbd_map(KB_DE, KB_US, PCKBD_KEYDESC_DE),
    kbd_map(KB_DE | KB_NODEAD, KB_DE, PCKBD_KEYDESC_DE_NODEAD),
    kbd_map(KB_FR, KB_US, PCKBD_KEYDESC_FR),
    kbd_map(KB_FR | KB_DVORAK, KB_US, PCKBD_KEYDESC_FR_DVORAK_BEPO),
    kbd_map(KB_DK, KB_US, PCKBD_KEYDESC_DK),
    kbd_map(KB_DK | KB_NODEAD, KB_DK, PCKBD_KEYDESC_DK_NODEAD),
    kbd_map(KB_IT, KB_US, PCKBD_KEYDESC_IT),
    kbd_map(KB_UK, KB_US, PCKBD_KEYDESC_UK),
    kbd_map(KB_JP, KB_US, PCKBD_KEYDESC_JP),
    kbd_map(KB_SV, KB_DK, PCKBD_KEYDESC_SV),
    kbd_map(KB_SV | KB_NODEAD, KB_SV, PCKBD_KEYDESC_SV_NODEAD),
    kbd_map(KB_NO, KB_DK, PCKBD_KEYDESC_NO),
    kbd_map(KB_NO | KB_NODEAD, KB_NO, PCKBD_KEYDESC_NO_NODEAD),
    kbd_map(KB_US | KB_DECLK, KB_US, PCKBD_KEYDESC_US_DECLK),
    kbd_map(KB_US | KB_DVORAK, KB_US, PCKBD_KEYDESC_US_DVORAK),
    kbd_map(KB_US | KB_COLEMAK, KB_US, PCKBD_KEYDESC_US_COLEMAK),
    kbd_map(KB_US | KB_SWAPCTRLCAPS, KB_US, PCKBD_KEYDESC_SWAPCTRLCAPS),
    kbd_map(KB_UK | KB_SWAPCTRLCAPS, KB_UK, PCKBD_KEYDESC_SWAPCTRLCAPS),
    kbd_map(KB_JP | KB_SWAPCTRLCAPS, KB_JP, PCKBD_KEYDESC_SWAPCTRLCAPS),
    kbd_map(KB_FR | KB_SWAPCTRLCAPS, KB_FR, PCKBD_KEYDESC_SWAPCTRLCAPS),
    kbd_map(KB_FR | KB_DVORAK | KB_SWAPCTRLCAPS, KB_FR | KB_DVORAK, PCKBD_KEYDESC_SWAPCTRLCAPS),
    kbd_map(KB_BE | KB_SWAPCTRLCAPS, KB_BE, PCKBD_KEYDESC_SWAPCTRLCAPS),
    kbd_map(KB_US | KB_DVORAK | KB_SWAPCTRLCAPS, KB_US | KB_DVORAK, PCKBD_KEYDESC_SWAPCTRLCAPS),
    kbd_map(KB_US | KB_COLEMAK | KB_SWAPCTRLCAPS, KB_US | KB_COLEMAK, PCKBD_KEYDESC_SWAPCTRLCAPS),
    kbd_map(KB_ES, KB_US, PCKBD_KEYDESC_ES),
    kbd_map(KB_BE, KB_US, PCKBD_KEYDESC_BE),
    kbd_map(KB_RU, KB_US, PCKBD_KEYDESC_RU),
    kbd_map(KB_UA, KB_US, PCKBD_KEYDESC_UA),
    kbd_map(KB_SG, KB_US, PCKBD_KEYDESC_SG),
    kbd_map(KB_SG | KB_NODEAD, KB_SG, PCKBD_KEYDESC_SG_NODEAD),
    kbd_map(KB_SF, KB_SG, PCKBD_KEYDESC_SF),
    kbd_map(KB_SF | KB_NODEAD, KB_SF, PCKBD_KEYDESC_SG_NODEAD),
    kbd_map(KB_PT, KB_US, PCKBD_KEYDESC_PT),
    kbd_map(KB_LT, KB_US, PCKBD_KEYDESC_LT),
    kbd_map(KB_LA, KB_US, PCKBD_KEYDESC_LA),
    kbd_map(KB_BR, KB_US, PCKBD_KEYDESC_BR),
    kbd_map(KB_TR, KB_US, PCKBD_KEYDESC_TR),
    kbd_map(KB_TR | KB_NODEAD, KB_TR, PCKBD_KEYDESC_TR_NODEAD),
    kbd_map(KB_PL, KB_US, PCKBD_KEYDESC_PL),
    kbd_map(KB_HU, KB_US, PCKBD_KEYDESC_HU),
    kbd_map(KB_SI, KB_US, PCKBD_KEYDESC_SI),
    kbd_map(KB_CF, KB_US, PCKBD_KEYDESC_CF),
    kbd_map(KB_CF | KB_NODEAD, KB_CF, PCKBD_KEYDESC_CF_NODEAD),
    kbd_map(KB_LV, KB_US, PCKBD_KEYDESC_LV),
    kbd_map(KB_NL, KB_US, PCKBD_KEYDESC_NL),
    kbd_map(KB_NL | KB_NODEAD, KB_NL, PCKBD_KEYDESC_NL_NODEAD),
    kbd_map(KB_IS, KB_US, PCKBD_KEYDESC_IS),
    kbd_map(KB_IS | KB_NODEAD, KB_IS, PCKBD_KEYDESC_IS_NODEAD),
    kbd_map(KB_EE, KB_US, PCKBD_KEYDESC_EE),
    kbd_map(KB_EE | KB_NODEAD, KB_EE, PCKBD_KEYDESC_EE_NODEAD),
    // #endif  /* WSKBD_NO_INTL_LAYOUTS */
];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
// Host tests of the layout tables: their shape, and (reference-backed) every keysym of every
// table against the C.

use std::collections::BTreeMap;
use std::string::{String, ToString};
use std::vec::Vec;

use super::*;

/// Whether `k` is a key code (`KS_GROUP_Keycode`).
fn is_kc(k: KeysymT) -> bool {
    ks_group(u32::from(k)) == KS_GROUP_Keycode
}

#[test]
fn tables_are_entries_of_a_key_code_and_at_most_four_keysyms() {
    for kd in &PCKBD_KEYDESCTAB {
        assert_eq!(kd.map_size as usize, kd.map.len());
        assert!(kd.map_size > 0);
        assert!(is_kc(kd.map[0]), "layout {:#x} starts with a key code", kd.name);
        let mut n = 0;
        for &k in &kd.map[1..] {
            if is_kc(k) {
                n = 0;
            } else {
                n += 1;
                assert!(n <= 5, "layout {:#x}: command + four keysyms at most", kd.name);
            }
        }
    }
}

#[test]
fn every_base_is_a_layout_and_names_are_unique() {
    let names: Vec<u32> = PCKBD_KEYDESCTAB.iter().map(|k| k.name).collect();
    for (i, kd) in PCKBD_KEYDESCTAB.iter().enumerate() {
        assert!(!names[..i].contains(&kd.name), "{:#x} twice", kd.name);
        assert!(kd.base == 0 || names.contains(&kd.base), "{:#x}", kd.base);
    }
    assert_eq!(PCKBD_KEYDESCTAB[0].name, KB_US);
    assert_eq!(PCKBD_KEYDESCTAB[0].base, 0);
    assert_eq!(kc(4), 0xe004);
}

/// The C preprocessor lines of a body dropped, and the `#if 0` ... `#endif` block with them
/// (`WSKBD_NO_INTL_LAYOUTS` is not defined, so only the `#if 0` removes anything).
fn live_lines(body: &str) -> String {
    let mut out = String::new();
    let mut dead = false;
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("#if 0") {
            dead = true;
        } else if t.starts_with('#') {
            dead = dead && !t.starts_with("#endif");
        } else if !dead {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The tokens of a C initialiser body: `KC(n)` and keysym names, comments and preprocessor
/// lines dropped.
fn c_tokens(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let live = live_lines(body);
    let mut s = &live[..];
    while let Some(i) = s.find("/*") {
        let j = s[i..].find("*/").map_or(s.len(), |j| i + j + 2);
        out.extend(tokens_of(&s[..i]));
        s = &s[j..];
    }
    out.extend(tokens_of(s));
    out
}

fn tokens_of(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect()
}

/// Every table and the layout list against `wskbdmap_mfii.c`: the same keysyms, keysym by keysym,
/// with `KS_*` and `KB_*` evaluated from `wsksymdef.h`.
#[test]
#[ignore = "needs OPENBSD_SRC"]
fn tables_match_the_c() {
    let path = crate::reftest::openbsd_src().join("sys/dev/pckbc/wskbdmap_mfii.c");
    let text = std::fs::read_to_string(path).unwrap();
    let defs = crate::reftest::defines("sys/dev/wscons/wsksymdef.h");
    let val = |tok: &str| -> u32 {
        if let Some(n) = tok.strip_prefix("KC(").and_then(|t| t.strip_suffix(')')) {
            return ks_keycode(n.parse().unwrap());
        }
        let mut v = 0;
        for part in tok.split('|') {
            let part = part.trim();
            v |= if part == "0" {
                0
            } else {
                crate::reftest::int(&defs, part).unwrap_or_else(|| panic!("{part}")) as u32
            };
        }
        v
    };

    // The arrays, by name.
    let mut tables: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut rest = &text[..];
    while let Some(i) = rest.find("static const keysym_t ") {
        let after = &rest[i + "static const keysym_t ".len()..];
        let name = &after[..after.find('[').unwrap()];
        let open = after.find('{').unwrap();
        let close = after.find("};").unwrap();
        let body = &after[open + 1..close];
        tables.insert(name.into(), c_tokens(body).iter().map(|t| val(t)).collect());
        rest = &after[close..];
    }
    assert_eq!(tables.len(), 43);

    // The layout list.
    let start = text.find("pckbd_keydesctab[] = {").unwrap();
    let list = &text[start..start + text[start..].find("};").unwrap()];
    let mut entries = Vec::new();
    for chunk in list.split("KBD_MAP(").skip(1) {
        let args = &chunk[..chunk.find(')').unwrap()];
        let args: Vec<&str> = args.split(',').map(str::trim).collect();
        entries.push((val(args[0]), val(args[1]), args[2].to_string()));
    }
    assert_eq!(entries.len(), PCKBD_KEYDESCTAB.len());
    for ((name, base, map), kd) in entries.iter().zip(&PCKBD_KEYDESCTAB) {
        assert_eq!((kd.name, kd.base), (*name, *base), "{map}");
        let ours: Vec<u32> = kd.map.iter().map(|&k| u32::from(k)).collect();
        assert_eq!(&ours, &tables[map], "{map}");
    }
}
}
/* </TESTS> */
