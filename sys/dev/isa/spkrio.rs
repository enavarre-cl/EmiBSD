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
//! `<dev/isa/spkrio.h>`: spkr(4)'s ioctls.
//!
//! Upstream: sys/dev/isa/spkrio.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `tone_t` is [`ToneT`].

use crate::machine::copy::AbiPod;
use crate::sys::ioccom::{_io, _iow};

/// `tone_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToneT {
    /// `frequency`: in hertz.
    pub frequency: i32,
    /// `duration`: in 1/100ths of a second (the C's comment; spkr(4) takes milliseconds).
    pub duration: i32,
}

// SAFETY: two `int`s, no padding; any bit pattern is a valid value.
unsafe impl AbiPod for ToneT {}

/// `SPKRTONE`: emit tone.
pub const SPKRTONE: u64 = _iow::<ToneT>(b'S', 1);
/// `SPKRTUNE`: emit tone sequence.
pub const SPKRTUNE: u64 = _io(b'S', 2);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers() {
        // _IOW('S', 1, tone_t): IOC_IN | (8 << 16) | ('S' << 8) | 1.
        assert_eq!(SPKRTONE, 0x8008_5301);
        // _IO('S', 2): IOC_VOID | ('S' << 8) | 2.
        assert_eq!(SPKRTUNE, 0x2000_5302);
        assert_eq!(size_of::<ToneT>(), 8);
    }
}
/* </TESTS> */
