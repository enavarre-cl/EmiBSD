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

/* Modified strtol() from stdlib */
/*-
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `strtol()`: a string to a `long`.
//!
//! Upstream: sys/lib/libsa/strtol.c @ 3ce1f3f79392
//!
//! Ignores `locale' stuff. Assumes that the upper and lower case alphabets and digits are
//! each contiguous. libsa's version skips every control character and blank in front of the
//! number (not only `isspace`), and saturates at `LONG_MIN`/`LONG_MAX` on overflow.
//!
//! ## Deviations
//! - The string is a byte slice, its end standing for the NUL; `endptr` is the index
//!   returned beside the value (the C's `*endptr - nptr`).

/// `strtol(nptr, &endptr, base)`: the value and the index where the number ended (0 if
/// there was none).
pub fn strtol(nptr: &[u8], base: i32) -> (i64, usize) {
    strtonum(nptr, base, |c| c <= b' ' || c >= 0x7f)
}

/// The body of `strtol` and `strtoll`, which differ only in what they skip in front of the
/// number (`skip`); `long` and `long long` are both 64-bit here.
pub(crate) fn strtonum(nptr: &[u8], base: i32, skip: fn(u8) -> bool) -> (i64, usize) {
    let at = |i: usize| nptr.get(i).copied().unwrap_or(0);
    let mut base = i64::from(base);

    // Skip white space and pick up leading +/- sign if any. If base is 0, allow 0x for hex
    // and 0 for octal, else assume decimal; if base is already 16, allow 0x.
    let mut s = 0;
    let mut c;
    loop {
        c = at(s);
        s += 1;
        if !skip(c) || s > nptr.len() {
            break;
        }
    }
    let neg = c == b'-';
    if neg || c == b'+' {
        c = at(s);
        s += 1;
    }
    if (base == 0 || base == 16) && c == b'0' && (at(s) == b'x' || at(s) == b'X') {
        c = at(s + 1);
        s += 2;
        base = 16;
    }
    if base == 0 {
        base = if c == b'0' { 8 } else { 10 };
    }

    // Compute the cutoff value between legal numbers and illegal numbers: the largest legal
    // value divided by the base, and the last digit allowed after it.
    let (mut cutoff, mut cutlim) = if neg {
        (i64::MIN / base, i64::MIN % base)
    } else {
        (i64::MAX / base, i64::MAX % base)
    };
    if neg {
        if cutlim > 0 {
            cutlim -= base;
            cutoff += 1;
        }
        cutlim = -cutlim;
    }
    let mut acc: i64 = 0;
    let mut any = 0;
    loop {
        let d = match c {
            b'0'..=b'9' => i64::from(c - b'0'),
            b'A'..=b'Z' => i64::from(c - b'A') + 10,
            b'a'..=b'z' => i64::from(c - b'a') + 10,
            _ => break,
        };
        if d >= base {
            break;
        }
        if any >= 0 {
            if neg {
                if acc < cutoff || (acc == cutoff && d > cutlim) {
                    any = -1;
                    acc = i64::MIN;
                } else {
                    any = 1;
                    acc = acc * base - d;
                }
            } else if acc > cutoff || (acc == cutoff && d > cutlim) {
                any = -1;
                acc = i64::MAX;
            } else {
                any = 1;
                acc = acc * base + d;
            }
        }
        c = at(s);
        s += 1;
    }
    (acc, if any != 0 { s - 1 } else { 0 })
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bases_signs_and_ends() {
        assert_eq!(strtol(b"115200", 0), (115_200, 6));
        assert_eq!(strtol(b"0x3f8", 0), (0x3f8, 5));
        assert_eq!(strtol(b"010", 0), (8, 3));
        assert_eq!(strtol(b"  -42k", 10), (-42, 5));
        assert_eq!(strtol(b"zz", 10), (0, 0));
        assert_eq!(strtol(b"99999999999999999999", 10), (i64::MAX, 20));
        assert_eq!(strtol(b"-99999999999999999999", 10), (i64::MIN, 21));
    }
}
/* </TESTS> */
