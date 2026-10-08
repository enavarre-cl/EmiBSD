/*	$OpenBSD: pcdisplay.h,v 1.2 2001/02/02 20:25:39 aaron Exp $ */
/*	$NetBSD: pcdisplay.h,v 1.1 1998/05/14 23:11:03 drochner Exp $	*/
/* <LICENSES> */
/* </LICENSES> */

//! `<dev/ic/pcdisplay.h>`: IBM PC display definitions, the colour and monochrome attribute
//! bits of a text-mode character cell.
//!
//! Upstream: sys/dev/ic/pcdisplay.h @ 3ce1f3f79392
//!
//! The original file carries no licence text, only its `$OpenBSD$` and `$NetBSD$` lines
//! (kept above).
//!
//! ## Deviations
//! - None: every define is here, as a `u32` (the attribute type of the emulops).

// Color attributes for foreground text

/// `FG_BLACK`.
pub const FG_BLACK: u32 = 0;
/// `FG_BLUE`.
pub const FG_BLUE: u32 = 1;
/// `FG_GREEN`.
pub const FG_GREEN: u32 = 2;
/// `FG_CYAN`.
pub const FG_CYAN: u32 = 3;
/// `FG_RED`.
pub const FG_RED: u32 = 4;
/// `FG_MAGENTA`.
pub const FG_MAGENTA: u32 = 5;
/// `FG_BROWN`.
pub const FG_BROWN: u32 = 6;
/// `FG_LIGHTGREY`.
pub const FG_LIGHTGREY: u32 = 7;
/// `FG_DARKGREY`.
pub const FG_DARKGREY: u32 = 8;
/// `FG_LIGHTBLUE`.
pub const FG_LIGHTBLUE: u32 = 9;
/// `FG_LIGHTGREEN`.
pub const FG_LIGHTGREEN: u32 = 10;
/// `FG_LIGHTCYAN`.
pub const FG_LIGHTCYAN: u32 = 11;
/// `FG_LIGHTRED`.
pub const FG_LIGHTRED: u32 = 12;
/// `FG_LIGHTMAGENTA`.
pub const FG_LIGHTMAGENTA: u32 = 13;
/// `FG_YELLOW`.
pub const FG_YELLOW: u32 = 14;
/// `FG_WHITE`.
pub const FG_WHITE: u32 = 15;
/// `FG_BLINK`.
pub const FG_BLINK: u32 = 0x80;
/// `FG_MASK`.
pub const FG_MASK: u32 = 0x8f;

// Color attributes for text background

/// `BG_BLACK`.
pub const BG_BLACK: u32 = 0x00;
/// `BG_BLUE`.
pub const BG_BLUE: u32 = 0x10;
/// `BG_GREEN`.
pub const BG_GREEN: u32 = 0x20;
/// `BG_CYAN`.
pub const BG_CYAN: u32 = 0x30;
/// `BG_RED`.
pub const BG_RED: u32 = 0x40;
/// `BG_MAGENTA`.
pub const BG_MAGENTA: u32 = 0x50;
/// `BG_BROWN`.
pub const BG_BROWN: u32 = 0x60;
/// `BG_LIGHTGREY`.
pub const BG_LIGHTGREY: u32 = 0x70;
/// `BG_MASK`.
pub const BG_MASK: u32 = 0x70;

// Monochrome attributes for foreground text

/// `FG_UNDERLINE`.
pub const FG_UNDERLINE: u32 = 0x01;
/// `FG_INTENSE`.
pub const FG_INTENSE: u32 = 0x08;

// Monochrome attributes for text background

/// `BG_INTENSE`.
pub const BG_INTENSE: u32 = 0x10;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every define against `<dev/ic/pcdisplay.h>`.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/pcdisplay.h");
        let names = crate::reftest::assert_defines!(defs;
            FG_BLACK, FG_BLUE, FG_GREEN, FG_CYAN, FG_RED, FG_MAGENTA, FG_BROWN, FG_LIGHTGREY,
            FG_DARKGREY, FG_LIGHTBLUE, FG_LIGHTGREEN, FG_LIGHTCYAN, FG_LIGHTRED,
            FG_LIGHTMAGENTA, FG_YELLOW, FG_WHITE, FG_BLINK, FG_MASK, BG_BLACK, BG_BLUE,
            BG_GREEN, BG_CYAN, BG_RED, BG_MAGENTA, BG_BROWN, BG_LIGHTGREY, BG_MASK,
            FG_UNDERLINE, FG_INTENSE, BG_INTENSE,
        );
        crate::reftest::assert_complete(&defs, "FG_", &names);
        crate::reftest::assert_complete(&defs, "BG_", &names);
    }
}
