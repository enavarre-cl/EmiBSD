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
//! The vt100 emulation's keyboard: the byte sequences of the function, editing, cursor and
//! keypad keys, and of the characters (UTF-8 or the layout's 8-bit charset).
//!
//! Upstream: sys/dev/wscons/wsemul_vt100_keys.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `wsemul_vt100_translate` writes a character into the caller's buffer and returns the
//!   bytes (borrowed from the buffer or a static string), where the C fills the state's
//!   `translatebuf` and returns a count and a pointer (`wsemulvar`'s `translate`).

use core::ffi::c_void;

use crate::dev::wscons::wsemul_subr::wsemul_utf8_translate;
use crate::dev::wscons::wsemul_vt100var::{
    VTFL_APPLCURSOR, VTFL_APPLKEYPAD, VTFL_UTF8, WsemulVt100Emuldata,
};
use crate::dev::wscons::wsemulvar::WSEMUL_TRANSLATE_SIZE;
use crate::dev::wscons::wsksymdef::*;
use crate::dev::wscons::wsksymvar::{KbdT, KeysymT};

/// `vt100_fkeys`: F1 to F24 (F1-F5 normally don't send codes; F11 is ESC, F12 BS and F13
/// LF on a VT100; F15 is help and F16 do).
static VT100_FKEYS: [&[u8]; 24] = [
    b"\x1b[11~", // F1
    b"\x1b[12~",
    b"\x1b[13~",
    b"\x1b[14~",
    b"\x1b[15~", // F5
    b"\x1b[17~", // F6
    b"\x1b[18~",
    b"\x1b[19~",
    b"\x1b[20~",
    b"\x1b[21~",
    b"\x1b[23~", // VT100: ESC
    b"\x1b[24~", // VT100: BS
    b"\x1b[25~", // VT100: LF
    b"\x1b[26~",
    b"\x1b[28~", // help
    b"\x1b[29~", // do
    b"\x1b[31~",
    b"\x1b[32~",
    b"\x1b[33~",
    b"\x1b[34~", // F20
    b"\x1b[35~",
    b"\x1b[36~",
    b"\x1b[37~",
    b"\x1b[38~",
];

/// `vt100_pfkeys`: PF1 to PF4.
static VT100_PFKEYS: [&[u8]; 4] = [
    b"\x1bOP", // PF1
    b"\x1bOQ", b"\x1bOR", b"\x1bOS", // PF4
];

/// `vt100_numpad`: the keypad digits in application mode.
static VT100_NUMPAD: [&[u8]; 10] = [
    b"\x1bOp", // KP 0
    b"\x1bOq", // KP 1
    b"\x1bOr", // KP 2
    b"\x1bOs", // KP 3
    b"\x1bOt", // KP 4
    b"\x1bOu", // KP 5
    b"\x1bOv", // KP 6
    b"\x1bOw", // KP 7
    b"\x1bOx", // KP 8
    b"\x1bOy", // KP 9
];

/// `wsemul_vt100_translate`: the bytes keysym `in_` sends; empty for a keysym that sends
/// nothing.
///
/// # Safety
///
/// `cookie` came from this emulation's `cnattach` or `attach` and is not detached; no call
/// changing it runs at the same time.
#[allow(non_upper_case_globals)] // the keysym names (`KS_Help`), verbatim, as patterns
pub unsafe fn wsemul_vt100_translate(
    cookie: *mut c_void,
    layout: KbdT,
    in_: KeysymT,
    buf: &mut [u8; WSEMUL_TRANSLATE_SIZE],
) -> &[u8] {
    // SAFETY: the caller's contract.
    let edp = unsafe { &*cookie.cast::<WsemulVt100Emuldata>() };
    let k = u32::from(in_);

    if ks_group(k) == KS_GROUP_Ascii {
        let n = wsemul_utf8_translate(ks_value(k), layout, buf, edp.flags & VTFL_UTF8 != 0);
        return &buf[..n];
    }

    if (KS_f1..=KS_f24).contains(&in_) {
        return VT100_FKEYS[usize::from(in_ - KS_f1)];
    }
    if (KS_F1..=KS_F24).contains(&in_) {
        return VT100_FKEYS[usize::from(in_ - KS_F1)];
    }
    if (KS_KP_F1..=KS_KP_F4).contains(&in_) {
        return VT100_PFKEYS[usize::from(in_ - KS_KP_F1)];
    }
    if edp.flags & VTFL_APPLKEYPAD != 0 {
        if (KS_KP_0..=KS_KP_9).contains(&in_) {
            return VT100_NUMPAD[usize::from(in_ - KS_KP_0)];
        }
        match in_ {
            KS_KP_Tab => return b"\x1bOI",
            KS_KP_Enter => return b"\x1bOM",
            KS_KP_Multiply => return b"\x1bOj",
            KS_KP_Add => return b"\x1bOk",
            KS_KP_Separator => return b"\x1bOl",
            KS_KP_Subtract => return b"\x1bOm",
            KS_KP_Decimal => return b"\x1bOn",
            KS_KP_Divide => return b"\x1bOo",
            _ => {}
        }
    } else if in_ & 0x80 == 0 {
        buf[0] = (in_ & 0xff) as u8; // turn into ASCII
        return &buf[..1];
    }

    let applcursor = edp.flags & VTFL_APPLCURSOR != 0;
    match in_ {
        KS_Help => VT100_FKEYS[15 - 1],
        KS_Execute => VT100_FKEYS[16 - 1], // "Do"
        KS_Find => b"\x1b[1~",
        KS_Insert | KS_KP_Insert => b"\x1b[2~",
        KS_KP_Delete => b"\x1b[3~",
        KS_Select => b"\x1b[4~",
        KS_Prior | KS_KP_Prior => b"\x1b[5~",
        KS_Next | KS_KP_Next => b"\x1b[6~",
        KS_Backtab => b"\x1b[Z",
        KS_Home | KS_KP_Home => b"\x1b[7~",
        KS_End | KS_KP_End => b"\x1b[8~",
        KS_Up | KS_KP_Up => {
            if applcursor {
                b"\x1bOA"
            } else {
                b"\x1b[A"
            }
        }
        KS_Down | KS_KP_Down => {
            if applcursor {
                b"\x1bOB"
            } else {
                b"\x1b[B"
            }
        }
        KS_Left | KS_KP_Left => {
            if applcursor {
                b"\x1bOD"
            } else {
                b"\x1b[D"
            }
        }
        KS_Right | KS_KP_Right => {
            if applcursor {
                b"\x1bOC"
            } else {
                b"\x1b[C"
            }
        }
        _ => &buf[..0],
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::wscons::wsemul_vt100var::VTFL_APPLKEYPAD;

    fn tr(flags: i32, k: KeysymT) -> std::vec::Vec<u8> {
        let mut e = WsemulVt100Emuldata::ZERO;
        e.flags = flags;
        let mut buf = [0u8; WSEMUL_TRANSLATE_SIZE];
        // SAFETY: `e` is a live state nothing else uses.
        unsafe { wsemul_vt100_translate((&raw mut e).cast(), KB_US, k, &mut buf) }.to_vec()
    }

    #[test]
    fn keys_send_vt220_sequences() {
        assert_eq!(tr(0, KS_a), b"a");
        assert_eq!(tr(VTFL_UTF8, 0xe9), "é".as_bytes());
        assert_eq!(tr(0, 0xe9), b"\xe9");
        assert_eq!(tr(0, KS_f1), b"\x1b[11~");
        assert_eq!(tr(0, KS_F12), b"\x1b[24~");
        assert_eq!(tr(0, KS_KP_F2), b"\x1bOQ");
        assert_eq!(tr(0, KS_KP_7), b"7");
        assert_eq!(tr(VTFL_APPLKEYPAD, KS_KP_7), b"\x1bOw");
        assert_eq!(tr(VTFL_APPLKEYPAD, KS_KP_Enter), b"\x1bOM");
        assert_eq!(tr(0, KS_Help), b"\x1b[28~");
        assert_eq!(tr(0, KS_KP_Home), b"\x1b[7~");
        assert_eq!(tr(0, KS_Up), b"\x1b[A");
        assert_eq!(tr(VTFL_APPLCURSOR, KS_Up), b"\x1bOA");
        // Outside application keypad mode a keysym without bit 7 becomes its low byte, as
        // in C; in it, an unknown keysym sends nothing.
        assert_eq!(tr(0, KS_Shift_L), [(KS_Shift_L & 0xff) as u8]);
        assert_eq!(tr(VTFL_APPLKEYPAD, KS_Shift_L), b"");
    }
}
/* </TESTS> */
