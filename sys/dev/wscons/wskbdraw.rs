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
 * Copyright (c) 2005, Miodrag Vallat
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
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 * ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/wscons/wskbdraw.h>`: US keyboard XT scancodes, the raw codes `wskbd(4)`'s keyboard
//! drivers hand to `wsdisplay(4)` in raw mode (`WSDISPLAY_COMPAT_RAWKBD`); the names match
//! `KS_xxx` symbols whenever possible.
//!
//! Upstream: sys/dev/wscons/wskbdraw.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Each `RAWKEY_*` is a `u8` constant. `hidkbd.rs`'s `HIDKBD_TRTAB`, generated from
//!   `hidkbd.c`'s table before this header was ported, spells the same codes as numbers.
//! - The C's block comments between the groups are line comments.

#![allow(non_upper_case_globals)] // the C names (`RAWKEY_a`), verbatim, for grep-ability

/// `RAWKEY_Null`.
pub const RAWKEY_Null: u8 = 0x00;
// These names match KS_xxx symbols whenever possible

/// `RAWKEY_Escape`.
pub const RAWKEY_Escape: u8 = 0x01;
/// `RAWKEY_1`.
pub const RAWKEY_1: u8 = 0x02;
/// `RAWKEY_2`.
pub const RAWKEY_2: u8 = 0x03;
/// `RAWKEY_3`.
pub const RAWKEY_3: u8 = 0x04;
/// `RAWKEY_4`.
pub const RAWKEY_4: u8 = 0x05;
/// `RAWKEY_5`.
pub const RAWKEY_5: u8 = 0x06;
/// `RAWKEY_6`.
pub const RAWKEY_6: u8 = 0x07;
/// `RAWKEY_7`.
pub const RAWKEY_7: u8 = 0x08;
/// `RAWKEY_8`.
pub const RAWKEY_8: u8 = 0x09;
/// `RAWKEY_9`.
pub const RAWKEY_9: u8 = 0x0a;
/// `RAWKEY_0`.
pub const RAWKEY_0: u8 = 0x0b;
/// `RAWKEY_minus`.
pub const RAWKEY_minus: u8 = 0x0c;
/// `RAWKEY_equal`.
pub const RAWKEY_equal: u8 = 0x0d;
/// `RAWKEY_Tab`.
pub const RAWKEY_Tab: u8 = 0x0f;
/// `RAWKEY_q`.
pub const RAWKEY_q: u8 = 0x10;
/// `RAWKEY_w`.
pub const RAWKEY_w: u8 = 0x11;
/// `RAWKEY_e`.
pub const RAWKEY_e: u8 = 0x12;
/// `RAWKEY_r`.
pub const RAWKEY_r: u8 = 0x13;
/// `RAWKEY_t`.
pub const RAWKEY_t: u8 = 0x14;
/// `RAWKEY_y`.
pub const RAWKEY_y: u8 = 0x15;
/// `RAWKEY_u`.
pub const RAWKEY_u: u8 = 0x16;
/// `RAWKEY_i`.
pub const RAWKEY_i: u8 = 0x17;
/// `RAWKEY_o`.
pub const RAWKEY_o: u8 = 0x18;
/// `RAWKEY_p`.
pub const RAWKEY_p: u8 = 0x19;
/// `RAWKEY_bracketleft`.
pub const RAWKEY_bracketleft: u8 = 0x1a;
/// `RAWKEY_bracketright`.
pub const RAWKEY_bracketright: u8 = 0x1b;
/// `RAWKEY_Return`.
pub const RAWKEY_Return: u8 = 0x1c;
/// `RAWKEY_Control_L`.
pub const RAWKEY_Control_L: u8 = 0x1d;
/// `RAWKEY_a`.
pub const RAWKEY_a: u8 = 0x1e;
/// `RAWKEY_s`.
pub const RAWKEY_s: u8 = 0x1f;
/// `RAWKEY_d`.
pub const RAWKEY_d: u8 = 0x20;
/// `RAWKEY_f`.
pub const RAWKEY_f: u8 = 0x21;
/// `RAWKEY_g`.
pub const RAWKEY_g: u8 = 0x22;
/// `RAWKEY_h`.
pub const RAWKEY_h: u8 = 0x23;
/// `RAWKEY_j`.
pub const RAWKEY_j: u8 = 0x24;
/// `RAWKEY_k`.
pub const RAWKEY_k: u8 = 0x25;
/// `RAWKEY_l`.
pub const RAWKEY_l: u8 = 0x26;
/// `RAWKEY_semicolon`.
pub const RAWKEY_semicolon: u8 = 0x27;
/// `RAWKEY_apostrophe`.
pub const RAWKEY_apostrophe: u8 = 0x28;
/// `RAWKEY_grave`.
pub const RAWKEY_grave: u8 = 0x29;
/// `RAWKEY_Shift_L`.
pub const RAWKEY_Shift_L: u8 = 0x2a;
/// `RAWKEY_backslash`.
pub const RAWKEY_backslash: u8 = 0x2b;
/// `RAWKEY_z`.
pub const RAWKEY_z: u8 = 0x2c;
/// `RAWKEY_x`.
pub const RAWKEY_x: u8 = 0x2d;
/// `RAWKEY_c`.
pub const RAWKEY_c: u8 = 0x2e;
/// `RAWKEY_v`.
pub const RAWKEY_v: u8 = 0x2f;
/// `RAWKEY_b`.
pub const RAWKEY_b: u8 = 0x30;
/// `RAWKEY_n`.
pub const RAWKEY_n: u8 = 0x31;
/// `RAWKEY_m`.
pub const RAWKEY_m: u8 = 0x32;
/// `RAWKEY_comma`.
pub const RAWKEY_comma: u8 = 0x33;
/// `RAWKEY_period`.
pub const RAWKEY_period: u8 = 0x34;
/// `RAWKEY_slash`.
pub const RAWKEY_slash: u8 = 0x35;
/// `RAWKEY_Shift_R`.
pub const RAWKEY_Shift_R: u8 = 0x36;
/// `RAWKEY_KP_Multiply`.
pub const RAWKEY_KP_Multiply: u8 = 0x37;
/// `RAWKEY_Alt_L`.
pub const RAWKEY_Alt_L: u8 = 0x38;
/// `RAWKEY_space`.
pub const RAWKEY_space: u8 = 0x39;
/// `RAWKEY_Caps_Lock`.
pub const RAWKEY_Caps_Lock: u8 = 0x3a;
/// `RAWKEY_f1`.
pub const RAWKEY_f1: u8 = 0x3b;
/// `RAWKEY_f2`.
pub const RAWKEY_f2: u8 = 0x3c;
/// `RAWKEY_f3`.
pub const RAWKEY_f3: u8 = 0x3d;
/// `RAWKEY_f4`.
pub const RAWKEY_f4: u8 = 0x3e;
/// `RAWKEY_f5`.
pub const RAWKEY_f5: u8 = 0x3f;
/// `RAWKEY_f6`.
pub const RAWKEY_f6: u8 = 0x40;
/// `RAWKEY_f7`.
pub const RAWKEY_f7: u8 = 0x41;
/// `RAWKEY_f8`.
pub const RAWKEY_f8: u8 = 0x42;
/// `RAWKEY_f9`.
pub const RAWKEY_f9: u8 = 0x43;
/// `RAWKEY_f10`.
pub const RAWKEY_f10: u8 = 0x44;
/// `RAWKEY_Num_Lock`.
pub const RAWKEY_Num_Lock: u8 = 0x45;
/// `RAWKEY_Hold_Screen`: Scroll Lock
pub const RAWKEY_Hold_Screen: u8 = 0x46;
/// `RAWKEY_KP_Home`.
pub const RAWKEY_KP_Home: u8 = 0x47;
/// `RAWKEY_KP_Up`.
pub const RAWKEY_KP_Up: u8 = 0x48;
/// `RAWKEY_KP_Prior`.
pub const RAWKEY_KP_Prior: u8 = 0x49;
/// `RAWKEY_KP_Subtract`.
pub const RAWKEY_KP_Subtract: u8 = 0x4a;
/// `RAWKEY_KP_Left`.
pub const RAWKEY_KP_Left: u8 = 0x4b;
/// `RAWKEY_KP_Begin`.
pub const RAWKEY_KP_Begin: u8 = 0x4c;
/// `RAWKEY_KP_Right`.
pub const RAWKEY_KP_Right: u8 = 0x4d;
/// `RAWKEY_KP_Add`.
pub const RAWKEY_KP_Add: u8 = 0x4e;
/// `RAWKEY_KP_End`.
pub const RAWKEY_KP_End: u8 = 0x4f;
/// `RAWKEY_KP_Down`.
pub const RAWKEY_KP_Down: u8 = 0x50;
/// `RAWKEY_KP_Next`.
pub const RAWKEY_KP_Next: u8 = 0x51;
/// `RAWKEY_KP_Insert`.
pub const RAWKEY_KP_Insert: u8 = 0x52;
/// `RAWKEY_KP_Delete`.
pub const RAWKEY_KP_Delete: u8 = 0x53;
/// `RAWKEY_less`: < > on European keyboards
pub const RAWKEY_less: u8 = 0x56;
/// `RAWKEY_f11`.
pub const RAWKEY_f11: u8 = 0x57;
/// `RAWKEY_f12`.
pub const RAWKEY_f12: u8 = 0x58;
/// `RAWKEY_Pause`.
pub const RAWKEY_Pause: u8 = 0x6a;
/// `RAWKEY_Meta_L`.
pub const RAWKEY_Meta_L: u8 = 0x73;
/// `RAWKEY_Meta_R`.
pub const RAWKEY_Meta_R: u8 = 0x74;
/// `RAWKEY_KP_Equal`.
pub const RAWKEY_KP_Equal: u8 = 0x76;
/// `RAWKEY_KP_Enter`.
pub const RAWKEY_KP_Enter: u8 = 0x9c;
/// `RAWKEY_Control_R`.
pub const RAWKEY_Control_R: u8 = 0x9d;
/// `RAWKEY_KP_Divide`.
pub const RAWKEY_KP_Divide: u8 = 0xb5;
/// `RAWKEY_Print_Screen`.
pub const RAWKEY_Print_Screen: u8 = 0xb7;
/// `RAWKEY_Alt_R`.
pub const RAWKEY_Alt_R: u8 = 0xb8;
/// `RAWKEY_Home`.
pub const RAWKEY_Home: u8 = 0xc7;
/// `RAWKEY_Up`.
pub const RAWKEY_Up: u8 = 0xc8;
/// `RAWKEY_Prior`.
pub const RAWKEY_Prior: u8 = 0xc9;
/// `RAWKEY_Left`.
pub const RAWKEY_Left: u8 = 0xcb;
/// `RAWKEY_Right`.
pub const RAWKEY_Right: u8 = 0xcd;
/// `RAWKEY_End`.
pub const RAWKEY_End: u8 = 0xcf;
/// `RAWKEY_Down`.
pub const RAWKEY_Down: u8 = 0xd0;
/// `RAWKEY_Next`.
pub const RAWKEY_Next: u8 = 0xd1;
/// `RAWKEY_Insert`.
pub const RAWKEY_Insert: u8 = 0xd2;
/// `RAWKEY_Delete`.
pub const RAWKEY_Delete: u8 = 0xd3;
/// `RAWKEY_Begin`.
pub const RAWKEY_Begin: u8 = 0x5d;
/// `RAWKEY_Menu`.
pub const RAWKEY_Menu: u8 = 0x6d;
/// `RAWKEY_Compose`.
pub const RAWKEY_Compose: u8 = 0x72;
// The following keys have no KS_xxx equivalents

/// `RAWKEY_BackSpace`.
pub const RAWKEY_BackSpace: u8 = 0x0e;
/// `RAWKEY_SysReq`.
pub const RAWKEY_SysReq: u8 = 0x54;
/// `RAWKEY_Power`.
pub const RAWKEY_Power: u8 = 0x84;
/// `RAWKEY_AudioMute`.
pub const RAWKEY_AudioMute: u8 = 0x85;
/// `RAWKEY_AudioLower`.
pub const RAWKEY_AudioLower: u8 = 0x86;
/// `RAWKEY_AudioRaise`.
pub const RAWKEY_AudioRaise: u8 = 0x87;
/// `RAWKEY_Help`.
pub const RAWKEY_Help: u8 = 0x88;
/// `RAWKEY_L1`: Stop
pub const RAWKEY_L1: u8 = 0x89;
/// `RAWKEY_L2`: Again
pub const RAWKEY_L2: u8 = 0x8a;
/// `RAWKEY_L3`: Props
pub const RAWKEY_L3: u8 = 0x8b;
/// `RAWKEY_L4`: Undo
pub const RAWKEY_L4: u8 = 0x8c;
/// `RAWKEY_L5`: Front
pub const RAWKEY_L5: u8 = 0x8d;
/// `RAWKEY_L6`: Copy
pub const RAWKEY_L6: u8 = 0x8e;
/// `RAWKEY_L7`: Open
pub const RAWKEY_L7: u8 = 0x8f;
/// `RAWKEY_L8`: Paste
pub const RAWKEY_L8: u8 = 0x90;
/// `RAWKEY_L9`: Find
pub const RAWKEY_L9: u8 = 0x91;
/// `RAWKEY_L10`: Cut
pub const RAWKEY_L10: u8 = 0x92;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest::{assert_complete, assert_defines};

    /// Every scancode against `<dev/wscons/wskbdraw.h>`, and no scancode left out.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn scancodes_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/wscons/wskbdraw.h");
        let ours = assert_defines!(defs;
            RAWKEY_Null, RAWKEY_Escape, RAWKEY_1, RAWKEY_2,
            RAWKEY_3, RAWKEY_4, RAWKEY_5, RAWKEY_6,
            RAWKEY_7, RAWKEY_8, RAWKEY_9, RAWKEY_0,
            RAWKEY_minus, RAWKEY_equal, RAWKEY_Tab, RAWKEY_q,
            RAWKEY_w, RAWKEY_e, RAWKEY_r, RAWKEY_t,
            RAWKEY_y, RAWKEY_u, RAWKEY_i, RAWKEY_o,
            RAWKEY_p, RAWKEY_bracketleft, RAWKEY_bracketright, RAWKEY_Return,
            RAWKEY_Control_L, RAWKEY_a, RAWKEY_s, RAWKEY_d,
            RAWKEY_f, RAWKEY_g, RAWKEY_h, RAWKEY_j,
            RAWKEY_k, RAWKEY_l, RAWKEY_semicolon, RAWKEY_apostrophe,
            RAWKEY_grave, RAWKEY_Shift_L, RAWKEY_backslash, RAWKEY_z,
            RAWKEY_x, RAWKEY_c, RAWKEY_v, RAWKEY_b,
            RAWKEY_n, RAWKEY_m, RAWKEY_comma, RAWKEY_period,
            RAWKEY_slash, RAWKEY_Shift_R, RAWKEY_KP_Multiply, RAWKEY_Alt_L,
            RAWKEY_space, RAWKEY_Caps_Lock, RAWKEY_f1, RAWKEY_f2,
            RAWKEY_f3, RAWKEY_f4, RAWKEY_f5, RAWKEY_f6,
            RAWKEY_f7, RAWKEY_f8, RAWKEY_f9, RAWKEY_f10,
            RAWKEY_Num_Lock, RAWKEY_Hold_Screen, RAWKEY_KP_Home, RAWKEY_KP_Up,
            RAWKEY_KP_Prior, RAWKEY_KP_Subtract, RAWKEY_KP_Left, RAWKEY_KP_Begin,
            RAWKEY_KP_Right, RAWKEY_KP_Add, RAWKEY_KP_End, RAWKEY_KP_Down,
            RAWKEY_KP_Next, RAWKEY_KP_Insert, RAWKEY_KP_Delete, RAWKEY_less,
            RAWKEY_f11, RAWKEY_f12, RAWKEY_Pause, RAWKEY_Meta_L,
            RAWKEY_Meta_R, RAWKEY_KP_Equal, RAWKEY_KP_Enter, RAWKEY_Control_R,
            RAWKEY_KP_Divide, RAWKEY_Print_Screen, RAWKEY_Alt_R, RAWKEY_Home,
            RAWKEY_Up, RAWKEY_Prior, RAWKEY_Left, RAWKEY_Right,
            RAWKEY_End, RAWKEY_Down, RAWKEY_Next, RAWKEY_Insert,
            RAWKEY_Delete, RAWKEY_Begin, RAWKEY_Menu, RAWKEY_Compose,
            RAWKEY_BackSpace, RAWKEY_SysReq, RAWKEY_Power, RAWKEY_AudioMute,
            RAWKEY_AudioLower, RAWKEY_AudioRaise, RAWKEY_Help, RAWKEY_L1,
            RAWKEY_L2, RAWKEY_L3, RAWKEY_L4, RAWKEY_L5,
            RAWKEY_L6, RAWKEY_L7, RAWKEY_L8, RAWKEY_L9,
            RAWKEY_L10,
        );
        assert_complete(&defs, "RAWKEY_", &ours);
    }
}
/* </TESTS> */
