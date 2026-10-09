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
 * Public domain.
 * Written by Matthew Dempsky.
 */
/* </LICENSES> */

/* <CODE> */
//! `explicit_bzero(3)`: zero a buffer in a way the compiler cannot remove.
//!
//! Upstream: sys/lib/libkern/explicit_bzero.c @ 3ce1f3f79392
//!
//! The C version calls `memset` and then a weak, empty `__explicit_bzero_hook` so that the
//! optimizer cannot prove the stores dead. Rust has no weak symbols on stable, so each byte is
//! stored with `write_volatile`, which the optimizer must keep.
//!
//! ## Deviations
//! - No `__explicit_bzero_hook`; volatile stores give the same guarantee.
//! - Takes a slice; the length is `buf.len()`.

/// Overwrites `buf` with zeros. The stores are volatile, so they survive even when the buffer is
/// never read again (the usual case for a secret about to be freed).
pub fn explicit_bzero(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        // SAFETY: `b` is a valid, aligned, exclusively borrowed `u8` for the duration of the
        // write; a volatile store through it is sound.
        unsafe { core::ptr::write_volatile(b, 0) };
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zeroes_everything() {
        let mut secret = *b"hunter2";
        explicit_bzero(&mut secret);
        assert_eq!(secret, [0; 7]);

        let mut empty: [u8; 0] = [];
        explicit_bzero(&mut empty);

        let mut partial = [0xffu8; 8];
        explicit_bzero(&mut partial[2..5]);
        assert_eq!(partial, [0xff, 0xff, 0, 0, 0, 0xff, 0xff, 0xff]);
    }
}
/* </TESTS> */
