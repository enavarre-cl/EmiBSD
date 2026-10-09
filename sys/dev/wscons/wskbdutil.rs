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
//! The keysym helpers of `wskbd(4)`: the compose table, the upper case of a keysym, and the
//! loading of a keyboard's keymap from its layout descriptions.
//!
//! Upstream: sys/dev/wscons/wskbdutil.c @ 3ce1f3f79392
//!
//! A layout ([`WsconsKeydesc`]) is a delta over its base: [`wskbd_load_keymap`] stacks the
//! layout and its bases, sizes the map after the largest key code any of them names, and
//! fills it from the base up, so that the layout's own entries win; [`wskbd_get_mapentry`]
//! looks one key up the same way without building a map (the console keyboard before it
//! attaches). An entry's keysyms fill the four of a [`WsconsKeymap`] ([`fillmapentry`]): one
//! keysym is both groups' plain symbol with its upper case shifted, two are plain and
//! shifted in both groups, three or four set group 2 too. Two keysyms typed after the
//! compose key, or a dead accent and a letter, become one ([`wskbd_compose_value`]).
//!
//! ## Deviations
//! - `compose_tab` is sorted at compile time ([`COMPOSE_TAB`] is the C's table and
//!   [`compose_tab_sorted`] the C's insertion sort, run by `const` evaluation): the C sorts
//!   its static table in place on the first lookup (`compose_tab_inorder`). The binary
//!   search over it is the C's.
//! - [`wskbd_init_keymap`] and [`wskbd_load_keymap`] return the `malloc`ed map as a
//!   `NonNull` and its length (the C's out-parameters); the caller frees it with
//!   `M_DEVBUF` and `maplen * size_of::<WsconsKeymap>()`, as in C. `wskbd_load_keymap`'s
//!   `EINVAL` is `Err(EINVAL)`; an `M_WAITOK` allocation does not fail (it would be
//!   `ENOMEM`).
//! - A layout table is a slice (`wsksymvar.rs`): the C's walk to the `map_size == 0`
//!   terminator is a walk over the slice, and an entry's command keysym is looked for only
//!   inside the table (the C reads one keysym past the end of a table whose last entry is
//!   a bare key code).
//! - [`fillmapentry`] takes the entry's keysyms as a slice (its length is the C's `len`).

use core::ptr::NonNull;

use crate::dev::wscons::wsksymdef::*;
use crate::dev::wscons::wsksymvar::{
    KB_HANDLEDBYWSKBD, KbdT, KeysymT, WsconsKeydesc, WsconsKeymap, WskbdMapdata,
};
use crate::kern::kern_malloc::mallocarray;
use crate::kern::subr_prf::panic;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK};

/// `struct compose_tab_s`: two keysyms and the one they compose to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComposeTabS {
    /// `elem`.
    pub elem: [KeysymT; 2],
    /// `result`.
    pub result: KeysymT,
}

/// An entry of [`COMPOSE_TAB`].
const fn ct(a: KeysymT, b: KeysymT, result: KeysymT) -> ComposeTabS {
    ComposeTabS {
        elem: [a, b],
        result,
    }
}

/// `compose_tab[]`, in the C's order.
#[rustfmt::skip]
pub const COMPOSE_TAB: [ComposeTabS; 134] = [
    ct(KS_plus, KS_plus, KS_numbersign),
    ct(KS_a, KS_a, KS_at),
    ct(KS_parenleft, KS_parenleft, KS_bracketleft),
    ct(KS_slash, KS_slash, KS_backslash),
    ct(KS_parenright, KS_parenright, KS_bracketright),
    ct(KS_parenleft, KS_minus, KS_braceleft),
    ct(KS_slash, KS_minus, KS_bar),
    ct(KS_parenright, KS_minus, KS_braceright),
    ct(KS_exclam, KS_exclam, KS_exclamdown),
    ct(KS_c, KS_slash, KS_cent),
    ct(KS_l, KS_minus, KS_sterling),
    ct(KS_y, KS_minus, KS_yen),
    ct(KS_s, KS_o, KS_section),
    ct(KS_x, KS_o, KS_currency),
    ct(KS_c, KS_o, KS_copyright),
    ct(KS_less, KS_less, KS_guillemotleft),
    ct(KS_greater, KS_greater, KS_guillemotright),
    ct(KS_question, KS_question, KS_questiondown),
    ct(KS_dead_acute, KS_space, KS_apostrophe),
    ct(KS_dead_grave, KS_space, KS_grave),
    ct(KS_dead_tilde, KS_space, KS_asciitilde),
    ct(KS_dead_circumflex, KS_space, KS_asciicircum),
    ct(KS_dead_diaeresis, KS_space, KS_quotedbl),
    ct(KS_dead_cedilla, KS_space, KS_comma),
    ct(KS_dead_circumflex, KS_A, KS_Acircumflex),
    ct(KS_dead_diaeresis, KS_A, KS_Adiaeresis),
    ct(KS_dead_grave, KS_A, KS_Agrave),
    ct(KS_dead_abovering, KS_A, KS_Aring),
    ct(KS_dead_tilde, KS_A, KS_Atilde),
    ct(KS_dead_cedilla, KS_C, KS_Ccedilla),
    ct(KS_dead_acute, KS_E, KS_Eacute),
    ct(KS_dead_circumflex, KS_E, KS_Ecircumflex),
    ct(KS_dead_diaeresis, KS_E, KS_Ediaeresis),
    ct(KS_dead_grave, KS_E, KS_Egrave),
    ct(KS_dead_acute, KS_I, KS_Iacute),
    ct(KS_dead_circumflex, KS_I, KS_Icircumflex),
    ct(KS_dead_diaeresis, KS_I, KS_Idiaeresis),
    ct(KS_dead_grave, KS_I, KS_Igrave),
    ct(KS_dead_tilde, KS_N, KS_Ntilde),
    ct(KS_dead_acute, KS_O, KS_Oacute),
    ct(KS_dead_circumflex, KS_O, KS_Ocircumflex),
    ct(KS_dead_diaeresis, KS_O, KS_Odiaeresis),
    ct(KS_dead_grave, KS_O, KS_Ograve),
    ct(KS_dead_tilde, KS_O, KS_Otilde),
    ct(KS_dead_acute, KS_U, KS_Uacute),
    ct(KS_dead_circumflex, KS_U, KS_Ucircumflex),
    ct(KS_dead_diaeresis, KS_U, KS_Udiaeresis),
    ct(KS_dead_grave, KS_U, KS_Ugrave),
    ct(KS_dead_acute, KS_Y, KS_Yacute),
    ct(KS_dead_acute, KS_a, KS_aacute),
    ct(KS_dead_circumflex, KS_a, KS_acircumflex),
    ct(KS_dead_diaeresis, KS_a, KS_adiaeresis),
    ct(KS_dead_grave, KS_a, KS_agrave),
    ct(KS_dead_abovering, KS_a, KS_aring),
    ct(KS_dead_tilde, KS_a, KS_atilde),
    ct(KS_dead_cedilla, KS_c, KS_ccedilla),
    ct(KS_dead_acute, KS_e, KS_eacute),
    ct(KS_dead_circumflex, KS_e, KS_ecircumflex),
    ct(KS_dead_diaeresis, KS_e, KS_ediaeresis),
    ct(KS_dead_grave, KS_e, KS_egrave),
    ct(KS_dead_acute, KS_i, KS_iacute),
    ct(KS_dead_circumflex, KS_i, KS_icircumflex),
    ct(KS_dead_diaeresis, KS_i, KS_idiaeresis),
    ct(KS_dead_grave, KS_i, KS_igrave),
    ct(KS_dead_tilde, KS_n, KS_ntilde),
    ct(KS_dead_acute, KS_o, KS_oacute),
    ct(KS_dead_circumflex, KS_o, KS_ocircumflex),
    ct(KS_dead_diaeresis, KS_o, KS_odiaeresis),
    ct(KS_dead_grave, KS_o, KS_ograve),
    ct(KS_dead_tilde, KS_o, KS_otilde),
    ct(KS_dead_acute, KS_u, KS_uacute),
    ct(KS_dead_circumflex, KS_u, KS_ucircumflex),
    ct(KS_dead_diaeresis, KS_u, KS_udiaeresis),
    ct(KS_dead_grave, KS_u, KS_ugrave),
    ct(KS_dead_acute, KS_y, KS_yacute),
    ct(KS_dead_diaeresis, KS_y, KS_ydiaeresis),
    ct(KS_quotedbl, KS_A, KS_Adiaeresis),
    ct(KS_quotedbl, KS_E, KS_Ediaeresis),
    ct(KS_quotedbl, KS_I, KS_Idiaeresis),
    ct(KS_quotedbl, KS_O, KS_Odiaeresis),
    ct(KS_quotedbl, KS_U, KS_Udiaeresis),
    ct(KS_quotedbl, KS_a, KS_adiaeresis),
    ct(KS_quotedbl, KS_e, KS_ediaeresis),
    ct(KS_quotedbl, KS_i, KS_idiaeresis),
    ct(KS_quotedbl, KS_o, KS_odiaeresis),
    ct(KS_quotedbl, KS_u, KS_udiaeresis),
    ct(KS_quotedbl, KS_y, KS_ydiaeresis),
    ct(KS_acute, KS_A, KS_Aacute),
    ct(KS_asciicircum, KS_A, KS_Acircumflex),
    ct(KS_grave, KS_A, KS_Agrave),
    ct(KS_asterisk, KS_A, KS_Aring),
    ct(KS_asciitilde, KS_A, KS_Atilde),
    ct(KS_cedilla, KS_C, KS_Ccedilla),
    ct(KS_acute, KS_E, KS_Eacute),
    ct(KS_asciicircum, KS_E, KS_Ecircumflex),
    ct(KS_grave, KS_E, KS_Egrave),
    ct(KS_acute, KS_I, KS_Iacute),
    ct(KS_asciicircum, KS_I, KS_Icircumflex),
    ct(KS_grave, KS_I, KS_Igrave),
    ct(KS_asciitilde, KS_N, KS_Ntilde),
    ct(KS_acute, KS_O, KS_Oacute),
    ct(KS_asciicircum, KS_O, KS_Ocircumflex),
    ct(KS_grave, KS_O, KS_Ograve),
    ct(KS_asciitilde, KS_O, KS_Otilde),
    ct(KS_acute, KS_U, KS_Uacute),
    ct(KS_asciicircum, KS_U, KS_Ucircumflex),
    ct(KS_grave, KS_U, KS_Ugrave),
    ct(KS_acute, KS_Y, KS_Yacute),
    ct(KS_acute, KS_a, KS_aacute),
    ct(KS_asciicircum, KS_a, KS_acircumflex),
    ct(KS_grave, KS_a, KS_agrave),
    ct(KS_asterisk, KS_a, KS_aring),
    ct(KS_asciitilde, KS_a, KS_atilde),
    ct(KS_cedilla, KS_c, KS_ccedilla),
    ct(KS_acute, KS_e, KS_eacute),
    ct(KS_asciicircum, KS_e, KS_ecircumflex),
    ct(KS_grave, KS_e, KS_egrave),
    ct(KS_acute, KS_i, KS_iacute),
    ct(KS_asciicircum, KS_i, KS_icircumflex),
    ct(KS_grave, KS_i, KS_igrave),
    ct(KS_asciitilde, KS_n, KS_ntilde),
    ct(KS_acute, KS_o, KS_oacute),
    ct(KS_asciicircum, KS_o, KS_ocircumflex),
    ct(KS_grave, KS_o, KS_ograve),
    ct(KS_asciitilde, KS_o, KS_otilde),
    ct(KS_acute, KS_u, KS_uacute),
    ct(KS_asciicircum, KS_u, KS_ucircumflex),
    ct(KS_grave, KS_u, KS_ugrave),
    ct(KS_acute, KS_y, KS_yacute),
    ct(KS_dead_caron, KS_space, KS_L2_caron),
    ct(KS_dead_caron, KS_S, KS_L2_Scaron),
    ct(KS_dead_caron, KS_Z, KS_L2_Zcaron),
    ct(KS_dead_caron, KS_s, KS_L2_scaron),
    ct(KS_dead_caron, KS_z, KS_L2_zcaron),
];

/// `COMPOSE_SIZE`.
const COMPOSE_SIZE: usize = COMPOSE_TAB.len();

/// `compose_tab`, sorted as the C's first `wskbd_compose_value` sorts it.
static COMPOSE_TAB_SORTED: [ComposeTabS; COMPOSE_SIZE] = compose_tab_sorted(COMPOSE_TAB);

/// `latin1_to_upper[]`: the upper case of each Latin-1 letter, 0 for the rest.
#[rustfmt::skip]
static LATIN1_TO_UPPER: [u8; 256] = [
//      0  8  1  9  2  a  3  b  4  c  5  d  6  e  7  f
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 0
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 0
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 1
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 1
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 2
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 2
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 3
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 3
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 4
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 4
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 5
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 5
    0x00, b'A', b'B', b'C', b'D', b'E', b'F', b'G',         // 6
    b'H', b'I', b'J', b'K', b'L', b'M', b'N', b'O',         // 6
    b'P', b'Q', b'R', b'S', b'T', b'U', b'V', b'W',         // 7
    b'X', b'Y', b'Z', 0x00, 0x00, 0x00, 0x00, 0x00,         // 7
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 8
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 8
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 9
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // 9
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // a
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // a
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // b
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // b
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // c
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // c
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // d
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,         // d
    0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,         // e
    0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf,         // e
    0xd0, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0x00,         // f
    0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0xdd, 0xde, 0x00,         // f
];

/// `compose_tab_cmp`: order by the first keysym, then the second.
const fn compose_tab_cmp(i: &ComposeTabS, j: &ComposeTabS) -> i32 {
    if i.elem[0] == j.elem[0] {
        i.elem[1] as i32 - j.elem[1] as i32
    } else {
        i.elem[0] as i32 - j.elem[0] as i32
    }
}

/// The C's insertion sort of `compose_tab` (the first `wskbd_compose_value`), as a `const
/// fn`.
const fn compose_tab_sorted(mut tab: [ComposeTabS; COMPOSE_SIZE]) -> [ComposeTabS; COMPOSE_SIZE] {
    let mut i = 1;
    while i < COMPOSE_SIZE {
        let v = tab[i];
        // find correct slot, moving others up
        let mut j = i;
        while j > 0 && compose_tab_cmp(&v, &tab[j - 1]) < 0 {
            tab[j] = tab[j - 1];
            j -= 1;
        }
        tab[j] = v;
        i += 1;
    }
    tab
}

/// `wskbd_compose_value`: the keysym `compose_buf`'s two compose to, `KS_voidSymbol` if
/// none.
pub fn wskbd_compose_value(compose_buf: &[KeysymT; 2]) -> KeysymT {
    let tab = &COMPOSE_TAB_SORTED;

    let mut j = 0;
    let mut i = COMPOSE_SIZE;
    while i != 0 {
        let e = &tab[j + i / 2];
        let r = if e.elem[0] == compose_buf[0] {
            if e.elem[1] == compose_buf[1] {
                return e.result;
            }
            e.elem[1] < compose_buf[1]
        } else {
            e.elem[0] < compose_buf[0]
        };
        if r {
            j += i / 2 + 1;
            i -= 1;
        }
        i /= 2;
    }

    KS_voidSymbol
}

/// `ksym_upcase`: the upper case of a keysym: `KS_F*` for `KS_f*`, the capital of a Latin-1
/// letter, itself otherwise.
pub fn ksym_upcase(ksym: KeysymT) -> KeysymT {
    if (KS_f1..=KS_f20).contains(&ksym) {
        return KS_F1 - KS_f1 + ksym;
    }

    if ks_group(u32::from(ksym)) == KS_GROUP_Ascii
        && ksym <= 0xff
        && LATIN1_TO_UPPER[ksym as usize] != 0x00
    {
        return KeysymT::from(LATIN1_TO_UPPER[ksym as usize]);
    }

    ksym
}

/// `fillmapentry`: the groups of `mapentry` from an entry's keysyms `kp` (0 to 4 of them).
pub fn fillmapentry(kp: &[KeysymT], mapentry: &mut WsconsKeymap) {
    match kp.len() {
        0 => {
            mapentry.group1[0] = KS_voidSymbol;
            mapentry.group1[1] = KS_voidSymbol;
            mapentry.group2[0] = KS_voidSymbol;
            mapentry.group2[1] = KS_voidSymbol;
        }
        1 => {
            mapentry.group1[0] = kp[0];
            mapentry.group1[1] = ksym_upcase(kp[0]);
            mapentry.group2[0] = mapentry.group1[0];
            mapentry.group2[1] = mapentry.group1[1];
        }
        2 => {
            mapentry.group1[0] = kp[0];
            mapentry.group1[1] = kp[1];
            mapentry.group2[0] = mapentry.group1[0];
            mapentry.group2[1] = mapentry.group1[1];
        }
        3 => {
            mapentry.group1[0] = kp[0];
            mapentry.group1[1] = kp[1];
            mapentry.group2[0] = kp[2];
            mapentry.group2[1] = ksym_upcase(kp[2]);
        }
        4 => {
            mapentry.group1[0] = kp[0];
            mapentry.group1[1] = kp[1];
            mapentry.group2[0] = kp[2];
            mapentry.group2[1] = kp[3];
        }
        _ => {}
    }
}

/// Whether `ks` is an entry's command keysym (`KS_GROUP_Command`, `KS_Cmd`, `KS_Cmd1` or
/// `KS_Cmd2`).
fn is_command(ks: KeysymT) -> bool {
    ks_group(u32::from(ks)) == KS_GROUP_Command || ks == KS_Cmd || ks == KS_Cmd1 || ks == KS_Cmd2
}

/// The number of keysyms from `kp`'s start up to the next key code (or the end).
fn entry_len(kp: &[KeysymT]) -> usize {
    kp.iter()
        .position(|&k| ks_group(u32::from(k)) == KS_GROUP_Keycode)
        .unwrap_or(kp.len())
}

/// The description of layout `cur` among `keydesc` (the C's walk to the terminator).
fn find_keydesc(keydesc: &'static [WsconsKeydesc], cur: KbdT) -> Option<&'static WsconsKeydesc> {
    keydesc
        .iter()
        .take_while(|mp| mp.map_size > 0)
        .find(|mp| mp.name == cur)
}

/// `wskbd_get_mapentry`: the keymap entry of key code `kc` in `mapdata`'s layout (and its
/// bases), without building the map.
pub fn wskbd_get_mapentry(mapdata: &WskbdMapdata, kc: i32, mapentry: &mut WsconsKeymap) {
    mapentry.command = KS_voidSymbol;
    mapentry.group1[0] = KS_voidSymbol;
    mapentry.group1[1] = KS_voidSymbol;
    mapentry.group2[0] = KS_voidSymbol;
    mapentry.group2[1] = KS_voidSymbol;

    let mut cur = mapdata.layout() & !KB_HANDLEDBYWSKBD;
    while cur != 0 {
        // If map not found, return
        let Some(mp) = find_keydesc(mapdata.keydesc, cur) else {
            return;
        };

        let map = mp.map;
        let mut k = 0;
        while k < map.len() {
            let ksg = ks_group(u32::from(map[k]));
            if ksg == KS_GROUP_Keycode && ks_value(u32::from(map[k])) == kc as u32 {
                // First skip keycode and possible command
                k += 1;
                if let Some(&c) = map.get(k)
                    && is_command(c)
                {
                    mapentry.command = c;
                    k += 1;
                }

                let rest = &map[k.min(map.len())..];
                let l = entry_len(rest);
                if l > 4 {
                    panic(format_args!(
                        "wskbd_get_mapentry: {}({}): bad entry",
                        mp.name,
                        rest.first().copied().unwrap_or(0)
                    ));
                }
                fillmapentry(&rest[..l], mapentry);
                return;
            }
            k += 1;
        }

        cur = mp.base;
    }
}

/// `wskbd_init_keymap`: a map of `maplen` entries, every keysym `KS_voidSymbol`.
pub fn wskbd_init_keymap(maplen: usize) -> Result<NonNull<WsconsKeymap>, Errno> {
    let map = mallocarray(maplen, size_of::<WsconsKeymap>(), M_DEVBUF, M_WAITOK)
        .ok_or(Errno::ENOMEM)?
        .cast::<WsconsKeymap>();
    let void = WsconsKeymap {
        command: KS_voidSymbol,
        group1: [KS_voidSymbol; 2],
        group2: [KS_voidSymbol; 2],
    };
    for i in 0..maplen {
        // SAFETY: a fresh allocation of `maplen` keymap entries.
        unsafe { map.as_ptr().add(i).write(void) };
    }
    Ok(map)
}

/// `wskbd_load_keymap`: the map of `layout` built from `mapdata`'s descriptions: the map and
/// its length, `EINVAL` if a layout of the chain is missing.
pub fn wskbd_load_keymap(
    mapdata: &WskbdMapdata,
    layout: KbdT,
) -> Result<(NonNull<WsconsKeymap>, usize), Errno> {
    let mut stack: [Option<&'static WsconsKeydesc>; 10] = [None; 10];
    let mut stack_ptr = 0;

    let mut cur = layout & !KB_HANDLEDBYWSKBD;
    while cur != 0 {
        let mp = find_keydesc(mapdata.keydesc, cur);

        if stack_ptr == stack.len() {
            panic(format_args!(
                "wskbd_load_keymap: {}: recursion too deep",
                mapdata.layout()
            ));
        }
        let Some(mp) = mp else {
            return Err(Errno::EINVAL);
        };

        stack[stack_ptr] = Some(mp);
        cur = mp.base;
        stack_ptr += 1;
    }
    let stack = &stack[..stack_ptr];

    let mut i = 0;
    for mp in stack.iter().rev().flatten() {
        for &k in mp.map {
            let ksg = ks_group(u32::from(k));
            if ksg == KS_GROUP_Keycode && ks_value(u32::from(k)) as usize > i {
                i = ks_value(u32::from(k)) as usize;
            }
        }
    }

    let maplen = i + 1;
    let map = wskbd_init_keymap(maplen)?;
    // SAFETY: the fresh map of `maplen` entries, ours until returned.
    let entries = unsafe { core::slice::from_raw_parts_mut(map.as_ptr(), maplen) };

    for mp in stack.iter().rev().flatten() {
        let m = mp.map;
        let mut k = 0;
        while k < m.len() {
            let ksg = ks_group(u32::from(m[k]));
            if ksg != KS_GROUP_Keycode {
                panic(format_args!(
                    "wskbd_load_keymap: {}({}): bad entry",
                    mp.name, m[k]
                ));
            }

            let kc = ks_value(u32::from(m[k])) as usize;
            k += 1;

            if let Some(&c) = m.get(k)
                && is_command(c)
            {
                entries[kc].command = c;
                k += 1;
            }

            let rest = &m[k.min(m.len())..];
            let l = entry_len(rest);

            if l > 4 {
                panic(format_args!(
                    "wskbd_load_keymap: {}({}): bad entry",
                    mp.name,
                    rest.first().copied().unwrap_or(0)
                ));
            }

            fillmapentry(&rest[..l], &mut entries[kc]);
            k += l;
        }
    }

    Ok((map, maplen))
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of the keysym helpers: the upper case, the compose table, the map entries, and
    // the keymaps of `ukbdmap.rs`'s layouts built both ways (`wskbd_load_keymap` against
    // `wskbd_get_mapentry` for every key of every layout).

    use super::*;
    use crate::dev::usb::ukbdmap::UKBD_KEYDESCTAB;
    use crate::kern::kern_malloc::free;
    use crate::kern::subr_pool::tests::setup_real_memory;

    /// The entries of a map `wskbd_load_keymap` built, freed after `f`.
    fn with_map(layout: KbdT, f: impl FnOnce(&[WsconsKeymap])) {
        let md = WskbdMapdata::new(&UKBD_KEYDESCTAB, layout);
        let (map, len) = wskbd_load_keymap(&md, layout).unwrap();
        // SAFETY: the map `wskbd_load_keymap` made, `len` entries.
        f(unsafe { core::slice::from_raw_parts(map.as_ptr(), len) });
        free(map.cast(), M_DEVBUF, len * size_of::<WsconsKeymap>());
    }

    #[test]
    fn upcase_of_letters_and_function_keys() {
        assert_eq!(ksym_upcase(KS_a), KS_A);
        assert_eq!(ksym_upcase(KS_z), KS_Z);
        assert_eq!(ksym_upcase(KS_agrave), KS_Agrave);
        assert_eq!(ksym_upcase(KS_thorn), KS_THORN);
        assert_eq!(
            ksym_upcase(KS_ydiaeresis),
            KS_ydiaeresis,
            "no Latin-1 capital"
        );
        assert_eq!(ksym_upcase(KS_division), KS_division);
        assert_eq!(ksym_upcase(KS_1), KS_1);
        assert_eq!(ksym_upcase(KS_A), KS_A);
        assert_eq!(ksym_upcase(KS_f1), KS_F1);
        assert_eq!(ksym_upcase(KS_f20), KS_F20);
        assert_eq!(ksym_upcase(KS_Return), KS_Return);
    }

    #[test]
    fn compose_table_is_sorted_and_every_pair_is_found() {
        let sorted = &COMPOSE_TAB_SORTED;
        for w in sorted.windows(2) {
            assert!(compose_tab_cmp(&w[0], &w[1]) <= 0, "{w:?}");
        }
        for e in &COMPOSE_TAB {
            assert!(sorted.contains(e));
            assert_eq!(wskbd_compose_value(&e.elem), e.result, "{e:?}");
        }
        assert_eq!(wskbd_compose_value(&[KS_a, KS_a]), KS_at);
        assert_eq!(wskbd_compose_value(&[KS_dead_acute, KS_e]), KS_eacute);
        assert_eq!(wskbd_compose_value(&[KS_dead_caron, KS_s]), KS_L2_scaron);
        assert_eq!(wskbd_compose_value(&[KS_e, KS_dead_acute]), KS_voidSymbol);
        assert_eq!(wskbd_compose_value(&[KS_x, KS_x]), KS_voidSymbol);
    }

    #[test]
    fn fillmapentry_by_number_of_keysyms() {
        let mut m = WsconsKeymap::default();
        fillmapentry(&[], &mut m);
        assert_eq!(
            (m.group1, m.group2),
            ([KS_voidSymbol; 2], [KS_voidSymbol; 2])
        );
        fillmapentry(&[KS_b], &mut m);
        assert_eq!((m.group1, m.group2), ([KS_b, KS_B], [KS_b, KS_B]));
        fillmapentry(&[KS_1, KS_exclam], &mut m);
        assert_eq!((m.group1, m.group2), ([KS_1, KS_exclam], [KS_1, KS_exclam]));
        fillmapentry(&[KS_q, KS_Q, KS_at], &mut m);
        assert_eq!((m.group1, m.group2), ([KS_q, KS_Q], [KS_at, KS_at]));
        fillmapentry(&[KS_e, KS_E, KS_currency, KS_cent], &mut m);
        assert_eq!((m.group1, m.group2), ([KS_e, KS_E], [KS_currency, KS_cent]));
    }

    #[test]
    fn us_keymap() {
        let _g = setup_real_memory();
        with_map(KB_US | KB_DEFAULT, |map| {
            assert_eq!(map.len(), 237, "up to KC(236)");
            assert_eq!(map[4].group1, [KS_a, KS_A]);
            assert_eq!(map[4].command, KS_voidSymbol);
            assert_eq!(map[30].group1, [KS_1, KS_exclam]);
            assert_eq!(map[40].group1[0], KS_Return);
            assert_eq!(map[41].command, KS_Cmd_Debugger);
            assert_eq!(map[41].group1[0], KS_Escape);
            assert_eq!(map[58].command, KS_Cmd_Screen0);
            assert_eq!(map[58].group1, [KS_f1, KS_F1]);
            assert_eq!(map[89].group1, [KS_KP_End, KS_KP_1]);
            assert_eq!(map[224].command, KS_Cmd1);
            assert_eq!(map[224].group1[0], KS_Control_L);
            assert_eq!(map[0].group1, [KS_voidSymbol; 2], "an unused code");
        });
    }

    #[test]
    fn german_keymap_is_a_delta_over_us() {
        let _g = setup_real_memory();
        with_map(KB_DE, |map| {
            assert_eq!(map[28].group1, [KS_z, KS_Z], "y and z swap");
            assert_eq!(map[29].group1, [KS_y, KS_Y]);
            assert_eq!(map[4].group1, [KS_a, KS_A], "from the base");
            assert_eq!(map[20].group2, [KS_at, KS_at]);
            assert_eq!(map[46].group1, [KS_dead_acute, KS_dead_grave]);
        });
        // A variant over a variant: de.nodead over de over us.
        with_map(KB_DE | KB_NODEAD, |map| {
            assert_eq!(map[28].group1, [KS_z, KS_Z]);
            assert_ne!(map[46].group1[0], KS_dead_acute);
        });
    }

    /// Whether a layer of `layout`'s chain has two entries for key `kc`: the lookup takes the
    /// first, the load the last (`fr.apple` redefines key 37), in C too.
    fn defined_twice(layout: KbdT, kc: u32) -> bool {
        let mut cur = layout;
        while cur != 0 {
            let Some(mp) = find_keydesc(&UKBD_KEYDESCTAB, cur) else {
                return false;
            };
            let code = ks_keycode(kc) as KeysymT;
            if mp.map.iter().filter(|&&k| k == code).count() > 1 {
                return true;
            }
            cur = mp.base;
        }
        false
    }

    #[test]
    fn every_layout_loads_and_matches_the_lookup_of_each_key() {
        // The lookup stops at the first layer that has the key, the load fills each layer over
        // the one below: a command of the base stays in the loaded map where a layer redefines
        // the key without one, and a key a layer has twice is its first entry for the lookup,
        // its last for the load (the C's two functions differ there too).
        let _g = setup_real_memory();
        for kd in &UKBD_KEYDESCTAB {
            with_map(kd.name, |map| {
                let md = WskbdMapdata::new(&UKBD_KEYDESCTAB, kd.name);
                for (kc, want) in map.iter().enumerate() {
                    if defined_twice(kd.name, kc as u32) {
                        continue;
                    }
                    let mut got = WsconsKeymap::default();
                    wskbd_get_mapentry(&md, kc as i32, &mut got);
                    assert_eq!(
                        (got.group1, got.group2),
                        (want.group1, want.group2),
                        "layout {:#x} key {kc}",
                        kd.name
                    );
                    if got.command != KS_voidSymbol {
                        assert_eq!(got.command, want.command, "layout {:#x} key {kc}", kd.name);
                    }
                }
            });
        }
    }

    #[test]
    fn unknown_layout_is_einval() {
        let _g = setup_real_memory();
        let md = WskbdMapdata::new(&UKBD_KEYDESCTAB, KB_US);
        assert_eq!(
            wskbd_load_keymap(&md, KB_HU | KB_APPLE).err(),
            Some(Errno::EINVAL)
        );
        let mut e = WsconsKeymap::default();
        let md = WskbdMapdata::new(&UKBD_KEYDESCTAB, KB_HU | KB_APPLE);
        wskbd_get_mapentry(&md, 4, &mut e);
        assert_eq!(e.group1, [KS_voidSymbol; 2]);
    }

    #[test]
    fn init_keymap_is_void() {
        let _g = setup_real_memory();
        let map = wskbd_init_keymap(3).unwrap();
        // SAFETY: three entries, just made.
        let m = unsafe { core::slice::from_raw_parts(map.as_ptr(), 3) };
        for e in m {
            assert_eq!(e.command, KS_voidSymbol);
            assert_eq!(e.group2, [KS_voidSymbol; 2]);
        }
        free(map.cast(), M_DEVBUF, 3 * size_of::<WsconsKeymap>());
    }

    /// The compose table against the C's: same pairs, same results, same order.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn compose_table_matches_the_c() {
        let path = crate::reftest::openbsd_src().join("sys/dev/wscons/wskbdutil.c");
        let text = std::fs::read_to_string(path).unwrap();
        let defs = crate::reftest::defines("sys/dev/wscons/wsksymdef.h");
        let ks = |name: &str| crate::reftest::int(&defs, name).unwrap() as KeysymT;
        let mut c = std::vec::Vec::new();
        for line in text.lines() {
            let l = line.trim();
            let Some(rest) = l.strip_prefix("{ {") else {
                continue;
            };
            let names: std::vec::Vec<&str> = rest
                .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                .filter(|s| s.starts_with("KS_"))
                .collect();
            assert_eq!(names.len(), 3, "{l}");
            c.push(ct(ks(names[0]), ks(names[1]), ks(names[2])));
        }
        assert_eq!(&c[..], &COMPOSE_TAB[..]);
    }
}
/* </TESTS> */
