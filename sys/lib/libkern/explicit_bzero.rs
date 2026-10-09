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
//! LZ: sys/lib/libkern/explicit_bzero.rs@f5985f1d055a
//!
//! The C version calls `memset` and then a weak, empty `__explicit_bzero_hook` so that the
//! optimizer cannot prove the stores dead. Rust has no weak symbols on stable, so each byte is
//! stored with `write_volatile`, which the optimizer must keep.
//!
//! ## Deviations
//! - No `__explicit_bzero_hook`; volatile stores give the same guarantee.
//! - Takes a slice; the length is `buf.len()`.
//!
//! ## Redesign
//! - The stores stay volatile, one per byte, and a `compiler_fence(SeqCst)` follows the loop so
//!   the compiler cannot move later code (freeing the buffer, reusing the allocation) ahead of the
//!   zeroing; the C gets the same ordering from its opaque hook call. `core::hint::black_box` is
//!   not used: it is documented as best effort, not a guarantee.
//! - The SAFETY argument is spelled out (the only `unsafe` here is the volatile store).
//! - New tests: every length 0..=64 at every alignment offset, and that neighbours are untouched.

/// Overwrites `buf` with zeros. The stores are volatile, so they survive even when the buffer is
/// never read again (the usual case for a secret about to be freed).
pub fn explicit_bzero(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        // SAFETY: `b` comes from `iter_mut` over a `&mut [u8]`, so it is a non-null, aligned,
        // initialised and exclusively borrowed `u8` for this write (the invariant of `&mut u8`,
        // established by the slice's owner and upheld by the borrow checker); `u8` has no drop
        // glue and every bit pattern is valid, so overwriting it cannot violate any invariant.
        // The store is volatile because the compiler may not elide or merge it, which is the
        // property this function exists for.
        unsafe { core::ptr::write_volatile(b, 0) };
    }
    // Keep later code (a free, a reuse of the memory) from moving above the stores.
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
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

    #[test]
    fn every_window_is_zeroed_and_nothing_else() {
        for start in 0..8usize {
            for len in 0..=64usize {
                let mut buf = [0xa5u8; 80];
                explicit_bzero(&mut buf[start..start + len]);
                for (i, &b) in buf.iter().enumerate() {
                    let inside = i >= start && i < start + len;
                    assert_eq!(
                        b,
                        if inside { 0 } else { 0xa5 },
                        "start {start} len {len} i {i}"
                    );
                }
            }
        }
    }
}
/* </TESTS> */
