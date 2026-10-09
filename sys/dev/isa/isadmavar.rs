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
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center.
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
//! The ISA DMA interface: `<dev/isa/isadmavar.h>`.
//!
//! Upstream: sys/dev/isa/isadmavar.h @ 3ce1f3f79392
//!
//! The functions it declares (`isa_dmacascade`, `isa_dmamap_create`, `isa_dmastart`, ...)
//! are `isadma.c`'s, in `isadma.rs`. `__ISADMA_COMPAT` is always defined ("XXX for now..."),
//! so the old interface fd(4) still uses is here: the `isadma_*` macros, which call those
//! functions on the bus `isadmaattach` recorded (`isa_dev`) with the channel's preallocated
//! map (`BUS_DMA_BUS1` marks the call as such).
//!
//! ## Deviations
//! - The `isadma_*` compatibility macros are functions. `isadma_acquire` and `isadma_release` are not ported: they expand to
//!   `isa_dma_acquire`/`isa_dma_release`, which no file of the tree defines (no caller
//!   either), so the C compiles them only because nobody uses them.
//! - `isadma_start` takes the buffer as `*mut u8` (`caddr_t`), its length as `BusSize`.

use crate::dev::isa::isadma::{
    isa_dev, isa_dmaabort, isa_dmacascade, isa_dmadone, isa_dmafinished, isa_dmastart,
};
use crate::machine::bus::{BUS_DMA_BUS1, BUS_DMA_WAITOK, BusSize};
use crate::sys::errno::Errno;

/// `MAX_ISADMA`.
pub const MAX_ISADMA: usize = 65536;

/// `DMAMODE_WRITE`.
pub const DMAMODE_WRITE: i32 = 0;
/// `DMAMODE_READ`.
pub const DMAMODE_READ: i32 = 1;
/// `DMAMODE_LOOP`.
pub const DMAMODE_LOOP: i32 = 2;

/// `isadma_cascade(c)`: `isa_dmacascade(isa_dev, c)`.
pub fn isadma_cascade(c: i32) {
    isa_dmacascade(isa_dev(), c)
}

/// `isadma_start(a, s, c, f)`: `isa_dmastart(isa_dev, c, a, s, 0, f,
/// BUS_DMA_WAITOK|BUS_DMA_BUS1)`.
///
/// # Safety
///
/// As [`isa_dmastart`]: `a` is `s` bytes of kernel memory that stay allocated and mapped
/// until the transfer is done ([`isadma_done`]) or aborted ([`isadma_abort`]).
pub unsafe fn isadma_start(a: *mut u8, s: BusSize, c: i32, f: i32) -> Result<(), Errno> {
    // SAFETY: the caller's contract is isa_dmastart's.
    unsafe { isa_dmastart(isa_dev(), c, a, s, None, f, BUS_DMA_WAITOK | BUS_DMA_BUS1) }
}

/// `isadma_abort(c)`: `isa_dmaabort(isa_dev, c)`.
pub fn isadma_abort(c: i32) {
    isa_dmaabort(isa_dev(), c)
}

/// `isadma_finished(c)`: `isa_dmafinished(isa_dev, c)`.
pub fn isadma_finished(c: i32) -> bool {
    isa_dmafinished(isa_dev(), c)
}

/// `isadma_done(c)`: `isa_dmadone(isa_dev, c)`.
pub fn isadma_done(c: i32) {
    isa_dmadone(isa_dev(), c)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/isa/isadmavar.h");
        assert_eq!(
            crate::reftest::int(&defs, "MAX_ISADMA"),
            Some(MAX_ISADMA as i64)
        );
        for (name, v) in [
            ("DMAMODE_WRITE", DMAMODE_WRITE),
            ("DMAMODE_READ", DMAMODE_READ),
            ("DMAMODE_LOOP", DMAMODE_LOOP),
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
