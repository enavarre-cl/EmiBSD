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
//! Keyboard definitions: the PS/2 keyboard's commands and responses,
//! `<dev/pckbc/pckbdreg.h>`.
//!
//! Upstream: sys/dev/pckbc/pckbdreg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - None: the values are `u8`, the bytes on the wire.

/// `KBC_RESET`: reset the keyboard.
pub const KBC_RESET: u8 = 0xFF;
/// `KBC_RESEND`: request the keyboard resend the last byte.
pub const KBC_RESEND: u8 = 0xFE;
/// `KBC_SETDEFAULT`: resets keyboard to its power-on defaults.
pub const KBC_SETDEFAULT: u8 = 0xF6;
/// `KBC_DISABLE`: as per `KBC_SETDEFAULT`, but also disable key scanning.
pub const KBC_DISABLE: u8 = 0xF5;
/// `KBC_ENABLE`: enable key scanning.
pub const KBC_ENABLE: u8 = 0xF4;
/// `KBC_TYPEMATIC`: set typematic rate and delay.
pub const KBC_TYPEMATIC: u8 = 0xF3;
/// `KBC_GETID`: get keyboard ID (not supported on AT kbd).
pub const KBC_GETID: u8 = 0xF2;
/// `KBC_SETTABLE`: set scancode translation table.
pub const KBC_SETTABLE: u8 = 0xF0;
/// `KBC_MODEIND`: set mode indicators (i.e. LEDs).
pub const KBC_MODEIND: u8 = 0xED;
/// `KBC_ECHO`: request an echo from the keyboard.
pub const KBC_ECHO: u8 = 0xEE;

/// `KBR_EXTENDED0`: extended key sequence.
pub const KBR_EXTENDED0: u8 = 0xE0;
/// `KBR_EXTENDED1`: extended key sequence.
pub const KBR_EXTENDED1: u8 = 0xE1;
/// `KBR_RESEND`: needs resend of command.
pub const KBR_RESEND: u8 = 0xFE;
/// `KBR_ACK`: received a valid command.
pub const KBR_ACK: u8 = 0xFA;
/// `KBR_OVERRUN`: flooded.
pub const KBR_OVERRUN: u8 = 0x00;
/// `KBR_FAILURE`: diagnostic failure.
pub const KBR_FAILURE: u8 = 0xFD;
/// `KBR_BREAK`: break code prefix - sent on key release.
pub const KBR_BREAK: u8 = 0xF0;
/// `KBR_RSTDONE`: reset complete.
pub const KBR_RSTDONE: u8 = 0xAA;
/// `KBR_ECHO`: echo response.
pub const KBR_ECHO: u8 = 0xEE;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resend_is_the_same_byte_both_ways() {
        assert_eq!(KBC_RESEND, KBR_RESEND);
        assert_eq!(KBC_ECHO, KBR_ECHO);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/pckbc/pckbdreg.h");
        let ours: &[(&str, u8)] = &[
            ("KBC_RESET", KBC_RESET),
            ("KBC_RESEND", KBC_RESEND),
            ("KBC_SETDEFAULT", KBC_SETDEFAULT),
            ("KBC_DISABLE", KBC_DISABLE),
            ("KBC_ENABLE", KBC_ENABLE),
            ("KBC_TYPEMATIC", KBC_TYPEMATIC),
            ("KBC_GETID", KBC_GETID),
            ("KBC_SETTABLE", KBC_SETTABLE),
            ("KBC_MODEIND", KBC_MODEIND),
            ("KBC_ECHO", KBC_ECHO),
            ("KBR_EXTENDED0", KBR_EXTENDED0),
            ("KBR_EXTENDED1", KBR_EXTENDED1),
            ("KBR_RESEND", KBR_RESEND),
            ("KBR_ACK", KBR_ACK),
            ("KBR_OVERRUN", KBR_OVERRUN),
            ("KBR_FAILURE", KBR_FAILURE),
            ("KBR_BREAK", KBR_BREAK),
            ("KBR_RSTDONE", KBR_RSTDONE),
            ("KBR_ECHO", KBR_ECHO),
        ];
        for (name, value) in ours {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(*value)),
                "{name}"
            );
        }
    }
}
/* </TESTS> */
