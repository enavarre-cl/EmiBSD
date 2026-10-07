/* $OpenBSD: ascii.h,v 1.6 2017/05/27 12:00:28 fcambus Exp $ */
/* $NetBSD: ascii.h,v 1.3 1998/06/20 19:11:04 drochner Exp $ */
/* <LICENSES> */
/* </LICENSES> */

//! `<dev/wscons/ascii.h>`: the ASCII control characters the terminal emulations act on.
//!
//! Upstream: sys/dev/wscons/ascii.h @ 3ce1f3f79392
//!
//! The original file carries no licence text, only its `$OpenBSD$` and `$NetBSD$` lines.
//!
//! ## Deviations
//! - The constants are `u32`, the type of the decoded character
//!   (`struct wsemul_inputstate`'s `inchar`) they are compared with.

/// `ASCII_NUL`: nul.
pub const ASCII_NUL: u32 = 0x00;
/// `ASCII_BEL`: bell.
pub const ASCII_BEL: u32 = 0x07;
/// `ASCII_BS`: backspace.
pub const ASCII_BS: u32 = 0x08;
/// `ASCII_HT`: horizontal tab.
pub const ASCII_HT: u32 = 0x09;
/// `ASCII_LF`: line feed.
pub const ASCII_LF: u32 = 0x0a;
/// `ASCII_VT`: vertical tab.
pub const ASCII_VT: u32 = 0x0b;
/// `ASCII_FF`: form feed.
pub const ASCII_FF: u32 = 0x0c;
/// `ASCII_CR`: carriage return.
pub const ASCII_CR: u32 = 0x0d;
/// `ASCII_SO`: shift out.
pub const ASCII_SO: u32 = 0x0e;
/// `ASCII_SI`: shift in.
pub const ASCII_SI: u32 = 0x0f;
/// `ASCII_CAN`: cancel.
pub const ASCII_CAN: u32 = 0x18;
/// `ASCII_SUB`: substitute.
pub const ASCII_SUB: u32 = 0x1a;
/// `ASCII_ESC`: escape.
pub const ASCII_ESC: u32 = 0x1b;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every constant against `<dev/wscons/ascii.h>`.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/wscons/ascii.h");
        crate::reftest::assert_defines!(defs;
            ASCII_NUL, ASCII_BEL, ASCII_BS, ASCII_HT, ASCII_LF, ASCII_VT, ASCII_FF, ASCII_CR,
            ASCII_SO, ASCII_SI, ASCII_CAN, ASCII_SUB, ASCII_ESC,
        );
    }
}
