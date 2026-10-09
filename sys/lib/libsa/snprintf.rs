/* <LICENSES> */
/*-
 * Copyright (c) 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *
 *	@(#)printf.c	8.1 (Berkeley) 6/11/93
 */
/* </LICENSES> */

/* <CODE> */
//! `snprintf()`: format into a buffer.
//!
//! Upstream: sys/lib/libsa/snprintf.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `snprintf(buf, len, fmt, ...)` is the [`snprintf!`](crate::snprintf!) macro over Rust
//!   format strings (see `printf.rs`); `sputchar`'s `sbuf`/`sbuf_end` globals are the state of
//!   a local writer. The result is the C's: at most `len - 1` bytes and a NUL, and the length
//!   the whole output would have had.

use core::fmt::{self, Write};

use crate::printf::latin1;

/// `sputchar()`'s state: the buffer and how far the output has gone (past the end, too).
struct Sbuf<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl Write for Sbuf<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            if self.pos < self.buf.len() {
                self.buf[self.pos] = latin1(c) as u8;
            }
            self.pos += 1;
        }
        Ok(())
    }
}

/// `snprintf(buf, len, fmt, ...)`: format into `buf`, NUL-terminated and truncated to fit.
#[macro_export]
macro_rules! snprintf {
    ($buf:expr, $($arg:tt)*) => {
        $crate::snprintf::vsnprintf($buf, format_args!($($arg)*))
    };
}

/// The body of `snprintf`: the length of the whole output, NUL excluded.
pub fn vsnprintf(buf: &mut [u8], args: fmt::Arguments<'_>) -> usize {
    let len = buf.len();
    let mut s = Sbuf { buf, pos: 0 };
    let _ = s.write_fmt(args);
    let pos = s.pos;
    if pos < len {
        s.buf[pos] = 0;
    } else if len > 0 {
        s.buf[len - 1] = 0;
    }
    pos
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    #[test]
    fn truncates_and_counts() {
        let mut buf = [0xffu8; 8];
        assert_eq!(crate::snprintf!(&mut buf, "{}:{}", "hd0a", "/bsd"), 9);
        assert_eq!(&buf, b"hd0a:/b\0");
        let mut buf = [0xffu8; 16];
        assert_eq!(crate::snprintf!(&mut buf, "{:02x}-{:3}", 10, 7), 6);
        assert_eq!(&buf[..7], b"0a-  7\0");
        assert_eq!(crate::snprintf!(&mut [], "x"), 1);
    }
}
/* </TESTS> */
