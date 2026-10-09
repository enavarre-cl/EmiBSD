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

/* public domain */
/* </LICENSES> */

/* <CODE> */
//! `<dev/wscons/wscons_features.h>`: the logic used to enable several optional features of
//! the wscons framework.
//!
//! Upstream: sys/dev/wscons/wscons_features.h @ 3ce1f3f79392
//!
//! The C defines each `HAVE_*` macro in a kernel built without `SMALL_KERNEL` (every
//! `GENERIC`; the installation `RAMDISK`s build with `SMALL_KERNEL` and get none of them):
//!
//! - `HAVE_WSMOUSED_SUPPORT`: support for `wsmoused(8)`;
//! - `HAVE_BURNER_SUPPORT`: screen blanking, controlled by `wsconsctl(8)`;
//! - `HAVE_SCROLLBACK_SUPPORT`: xterm-like shift-PgUp scrollback if the underlying
//!   wsdisplay supports it;
//! - `HAVE_JUMP_SCROLL`: jump scroll in the textmode emulation code;
//! - `HAVE_UTF8_SUPPORT`: UTF-8 mode and escape sequences in the textmode emulation code;
//! - `HAVE_RESTARTABLE_EMULOPS`: the C comment says defining it disables most of the
//!   restartable emulops code (for kernels whose display drivers never fail an emulop, i.e.
//!   no `udl(4)`); the code does the opposite: defined, `WSEMULOP` keeps the counts that let
//!   an output resume after a failed emulop, and undefined, it ignores emulop failures;
//! - `HAVE_DOUBLE_WIDTH_HEIGHT`: the escape sequences for double width and height
//!   characters.
//!
//! ## Deviations
//! - Each macro is a `bool` constant, `true` as in a `GENERIC` kernel: EmiBSD builds no
//!   `SMALL_KERNEL`. The code they guard tests them with a plain `if`, so both sides of every
//!   `#ifdef` are compiled and type-checked, and the optimiser removes the dead side.

/// `HAVE_WSMOUSED_SUPPORT`.
pub const HAVE_WSMOUSED_SUPPORT: bool = true;
/// `HAVE_BURNER_SUPPORT`.
pub const HAVE_BURNER_SUPPORT: bool = true;
/// `HAVE_SCROLLBACK_SUPPORT`.
pub const HAVE_SCROLLBACK_SUPPORT: bool = true;
/// `HAVE_JUMP_SCROLL`.
pub const HAVE_JUMP_SCROLL: bool = true;
/// `HAVE_UTF8_SUPPORT`.
pub const HAVE_UTF8_SUPPORT: bool = true;
/// `HAVE_RESTARTABLE_EMULOPS`.
pub const HAVE_RESTARTABLE_EMULOPS: bool = true;
/// `HAVE_DOUBLE_WIDTH_HEIGHT`.
pub const HAVE_DOUBLE_WIDTH_HEIGHT: bool = true;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// The C defines every feature outside `SMALL_KERNEL`; a feature added upstream fails.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn every_feature_of_the_c_header() {
        let path = crate::reftest::openbsd_src().join("sys/dev/wscons/wscons_features.h");
        let text = std::fs::read_to_string(path).unwrap();
        let ours = [
            ("HAVE_WSMOUSED_SUPPORT", HAVE_WSMOUSED_SUPPORT),
            ("HAVE_BURNER_SUPPORT", HAVE_BURNER_SUPPORT),
            ("HAVE_SCROLLBACK_SUPPORT", HAVE_SCROLLBACK_SUPPORT),
            ("HAVE_JUMP_SCROLL", HAVE_JUMP_SCROLL),
            ("HAVE_UTF8_SUPPORT", HAVE_UTF8_SUPPORT),
            ("HAVE_RESTARTABLE_EMULOPS", HAVE_RESTARTABLE_EMULOPS),
            ("HAVE_DOUBLE_WIDTH_HEIGHT", HAVE_DOUBLE_WIDTH_HEIGHT),
        ];
        let defined: std::vec::Vec<&str> = text
            .lines()
            .filter_map(|l| l.trim_start().strip_prefix("#define"))
            .map(str::trim)
            .collect();
        assert_eq!(defined.len(), ours.len());
        for (name, on) in ours {
            assert!(on && defined.contains(&name), "{name}");
        }
    }
}
/* </TESTS> */
