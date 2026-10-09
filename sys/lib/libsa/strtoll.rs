/* <LICENSES> */
/* Modified strtoll() from stdlib */
/*-
 * Copyright (c) 1992 The Regents of the University of California.
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
//! `strtoll()`: a string to a `long long`.
//!
//! Upstream: sys/lib/libsa/strtoll.c @ 3ce1f3f79392
//!
//! Ignores `locale' stuff. Assumes that the upper and lower case alphabets and digits are
//! each contiguous. Unlike libsa's `strtol`, it skips only `isspace` (a blank or a tab) in
//! front of the number; it saturates at `LLONG_MIN`/`LLONG_MAX` on overflow.
//!
//! ## Deviations
//! - The string is a byte slice, its end standing for the NUL; `endptr` is the index
//!   returned beside the value. The conversion loop is `strtol.rs`'s (`long` and `long long`
//!   are both 64-bit on amd64 and arm64).

use crate::stand::isspace;
use crate::strtol::strtonum;

/// `strtoll(nptr, &endptr, base)`: the value and the index where the number ended (0 if
/// there was none).
pub fn strtoll(nptr: &[u8], base: i32) -> (i64, usize) {
    strtonum(nptr, base, isspace)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_blanks_only() {
        assert_eq!(strtoll(b" \t0x10", 0), (16, 6));
        assert_eq!(strtoll(b"\n5", 0), (0, 0));
        assert_eq!(strtoll(b"64M", 0), (64, 2));
    }
}
/* </TESTS> */
