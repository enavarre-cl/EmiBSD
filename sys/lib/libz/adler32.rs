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

/* adler32.c -- compute the Adler-32 checksum of a data stream
 * Copyright (C) 1995-2011, 2016 Mark Adler
 * For conditions of distribution and use, see copyright notice in zlib.h
 */

/* zlib.h -- interface of the 'zlib' general purpose compression library
  version 1.3.2, February 17th, 2026

  Copyright (C) 1995-2026 Jean-loup Gailly and Mark Adler

  This software is provided 'as-is', without any express or implied
  warranty.  In no event will the authors be held liable for any damages
  arising from the use of this software.

  Permission is granted to anyone to use this software for any purpose,
  including commercial applications, and to alter it and redistribute it
  freely, subject to the following restrictions:

  1. The origin of this software must not be misrepresented; you must not
     claim that you wrote the original software. If you use this software
     in a product, an acknowledgment in the product documentation would be
     appreciated but is not required.
  2. Altered source versions must be plainly marked as such, and must not be
     misrepresented as being the original software.
  3. This notice may not be removed or altered from any source distribution.

  Jean-loup Gailly        Mark Adler
  jloup@gzip.org          madler@alumni.caltech.edu


  The data format used by the zlib library is described by RFCs (Request for
  Comments) 1950 to 1952 at https://datatracker.ietf.org/doc/html/rfc1950
  (zlib format), rfc1951 (deflate format) and rfc1952 (gzip format).
*/
/* </LICENSES> */

/* <CODE> */
//! The Adler-32 checksum (RFC 1950, section 8.2): the check value of a zlib stream.
//!
//! Upstream: sys/lib/libz/adler32.c @ 3ce1f3f79392
//!
//! **This is an altered source version, not the original zlib `adler32.c`** (zlib licence,
//! clause 2): a Rust rewrite written for EmiBSD. The original's notice is kept above in full.
//!
//! The two sums are kept modulo `BASE`, the largest prime below 65536. As in the C, the
//! modulo is taken once per `NMAX` bytes, the longest run whose sums cannot overflow 32 bits,
//! and lengths of 1 and below 16 take short paths.
//!
//! ## Deviations
//! - `buf` is `Option<&[u8]>`: `None` is the C's `Z_NULL`, which returns the initial value 1
//!   (`adler32(0, None)`); a slice carries its length, so `adler32` and `adler32_z` are one
//!   function.
//! - `NO_DIVIDE` (the shift-and-subtract `MOD`) is not defined in the kernel build; `MOD`,
//!   `MOD28` and `MOD63` are the `%` the C uses then.
//! - `adler32_combine` and `adler32_combine64` are one function over an `i64` length
//!   (`z_off_t` and `z_off64_t` are both 64 bits on LP64); `adler32_combine_` is its body.

/// `BASE`: largest prime smaller than 65536.
const BASE: u32 = 65521;
/// `NMAX`: the largest n such that `255n(n+1)/2 + (n+1)(BASE-1) <= 2^32-1`.
const NMAX: usize = 5552;

/// `adler32` (and `adler32_z`): update the running Adler-32 `adler` with `buf` and return it.
/// `buf == None` returns the required initial value, 1.
pub fn adler32(adler: u32, buf: Option<&[u8]>) -> u32 {
    // split Adler-32 into component sums
    let mut sum2 = (adler >> 16) & 0xffff;
    let mut adler = adler & 0xffff;

    // initial Adler-32 value
    let Some(buf) = buf else {
        return 1;
    };

    // in case user likes doing a byte at a time, keep it fast
    if let [b] = buf {
        adler += u32::from(*b);
        if adler >= BASE {
            adler -= BASE;
        }
        sum2 += adler;
        if sum2 >= BASE {
            sum2 -= BASE;
        }
        return adler | (sum2 << 16);
    }

    // in case short lengths are provided, keep it somewhat fast
    if buf.len() < 16 {
        for &b in buf {
            adler += u32::from(b);
            sum2 += adler;
        }
        if adler >= BASE {
            adler -= BASE;
        }
        sum2 %= BASE; // only added so many BASE's
        return adler | (sum2 << 16);
    }

    // do length NMAX blocks -- requires just one modulo operation per block; the remaining
    // bytes (less than NMAX) need one more
    for block in buf.chunks(NMAX) {
        for &b in block {
            adler += u32::from(b);
            sum2 += adler;
        }
        adler %= BASE;
        sum2 %= BASE;
    }

    // return recombined sums
    adler | (sum2 << 16)
}

/// `adler32_combine` (and `adler32_combine64`, `adler32_combine_`): the Adler-32 of two
/// sequences concatenated, from the Adler-32 of each and the length of the second. A negative
/// `len2` returns `0xffffffff`, an invalid Adler-32, as a clue for debugging.
pub fn adler32_combine(adler1: u32, adler2: u32, len2: i64) -> u32 {
    // for negative len, return invalid adler32 as a clue for debugging
    if len2 < 0 {
        return 0xffff_ffff;
    }

    // the derivation of this formula is left as an exercise for the reader
    let rem = (len2 % i64::from(BASE)) as u32;
    let mut sum1 = adler1 & 0xffff;
    let mut sum2 = rem * sum1;
    sum2 %= BASE;
    sum1 += (adler2 & 0xffff) + BASE - 1;
    sum2 += ((adler1 >> 16) & 0xffff) + ((adler2 >> 16) & 0xffff) + BASE - rem;
    if sum1 >= BASE {
        sum1 -= BASE;
    }
    if sum1 >= BASE {
        sum1 -= BASE;
    }
    if sum2 >= BASE << 1 {
        sum2 -= BASE << 1;
    }
    if sum2 >= BASE {
        sum2 -= BASE;
    }
    sum1 | (sum2 << 16)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    /// The definition, RFC 1950 section 8.2, one byte at a time with a modulo each step.
    fn reference(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &x in data {
            a = (a + u32::from(x)) % BASE;
            b = (b + a) % BASE;
        }
        (b << 16) | a
    }

    #[test]
    fn known_vectors() {
        let cases: &[(&[u8], u32)] = &[
            (b"", 0x0000_0001),
            (b"a", 0x0062_0062),
            (b"abc", 0x024d_0127),
            (b"Wikipedia", 0x11e6_0398),
            (b"123456789", 0x091e_01de),
            (b"The quick brown fox jumps over the lazy dog", 0x5bdc_0fda),
        ];
        for &(input, want) in cases {
            assert_eq!(
                adler32(1, Some(input)),
                want,
                "{:?}",
                core::str::from_utf8(input)
            );
        }
    }

    #[test]
    fn null_buffer_is_the_initial_value() {
        assert_eq!(adler32(0, None), 1);
        assert_eq!(adler32(0x1234_5678, None), 1);
    }

    #[test]
    fn long_buffers_match_the_definition() {
        // All 0xff bytes is the worst case for overflow; lengths around NMAX and its multiples.
        for len in [15, 16, 17, NMAX - 1, NMAX, NMAX + 1, 3 * NMAX + 7, 100_000] {
            let ff = std::vec![0xffu8; len];
            assert_eq!(adler32(1, Some(&ff)), reference(&ff), "0xff * {len}");
            let ramp: Vec<u8> = (0..len).map(|i| (i * 7 + 3) as u8).collect();
            assert_eq!(adler32(1, Some(&ramp)), reference(&ramp), "ramp * {len}");
        }
    }

    #[test]
    fn chaining_and_combining() {
        let data: Vec<u8> = (0..20_000u32).map(|i| (i % 251) as u8).collect();
        let whole = adler32(1, Some(&data));
        for split in [0, 1, 15, 16, 5551, 5552, 5553, 19_999, 20_000] {
            let (a, b) = data.split_at(split);
            let chained = adler32(adler32(1, Some(a)), Some(b));
            assert_eq!(chained, whole, "chained at {split}");
            let combined =
                adler32_combine(adler32(1, Some(a)), adler32(1, Some(b)), b.len() as i64);
            assert_eq!(combined, whole, "combined at {split}");
        }
        assert_eq!(adler32_combine(1, 1, -1), 0xffff_ffff);
    }
}
/* </TESTS> */
