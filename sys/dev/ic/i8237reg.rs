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
//! The Intel 8237 DMA controller's mode and mask bits: `<dev/ic/i8237reg.h>`.
//!
//! Upstream: sys/dev/ic/i8237reg.h @ 3ce1f3f79392
//!
//! `isadma.c` writes them to the PC's two cascaded 8237s (`isadmareg.h`).

/// `DMA37MD_DEMAND`: demand mode.
pub const DMA37MD_DEMAND: u8 = 0x00;
/// `DMA37MD_WRITE`: read the device, write memory operation.
pub const DMA37MD_WRITE: u8 = 0x04;
/// `DMA37MD_READ`: write the device, read memory operation.
pub const DMA37MD_READ: u8 = 0x08;
/// `DMA37MD_LOOP`: auto-initialize mode.
pub const DMA37MD_LOOP: u8 = 0x10;
/// `DMA37MD_SINGLE`: single pass mode.
pub const DMA37MD_SINGLE: u8 = 0x40;
/// `DMA37MD_CASCADE`: cascade mode.
pub const DMA37MD_CASCADE: u8 = 0xc0;

/// `DMA37SM_CLEAR`: clear mask bit.
pub const DMA37SM_CLEAR: u8 = 0x00;
/// `DMA37SM_SET`: set mask bit.
pub const DMA37SM_SET: u8 = 0x04;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/i8237reg.h");
        for (name, v) in [
            ("DMA37MD_DEMAND", DMA37MD_DEMAND),
            ("DMA37MD_WRITE", DMA37MD_WRITE),
            ("DMA37MD_READ", DMA37MD_READ),
            ("DMA37MD_LOOP", DMA37MD_LOOP),
            ("DMA37MD_SINGLE", DMA37MD_SINGLE),
            ("DMA37MD_CASCADE", DMA37MD_CASCADE),
            ("DMA37SM_CLEAR", DMA37SM_CLEAR),
            ("DMA37SM_SET", DMA37SM_SET),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(v)),
                "{name}"
            );
        }
    }
}
/* </TESTS> */
