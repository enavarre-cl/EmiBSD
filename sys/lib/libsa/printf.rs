/*	$OpenBSD: printf.c,v 1.29 2019/05/11 16:56:47 deraadt Exp $	*/
/*	$NetBSD: printf.c,v 1.10 1996/11/30 04:19:21 gwr Exp $	*/
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

/*
 * Scaled down version of printf(3).
 *
 * One additional format:
 *
 * The format %b is supported to decode error registers.
 * Its usage is:
 *
 *	printf("reg=%b\n", regval, "<base><arg>*");
 *
 * where <base> is the output base expressed as a control character, e.g.
 * \10 gives octal; \20 gives hex.  Each arg is a sequence of characters,
 * the first of which gives the bit number to be inspected (origin 1), and
 * the next characters (up to a control character, i.e. a character <= 32),
 * give the name of the register.  Thus:
 *
 *	printf("reg=%b\n", 3, "\10\2BITTWO\1BITONE\n");
 *
 * would produce output:
 *
 *	reg=3<BITTWO,BITONE>
 */
/* </LICENSES> */

//! `printf()` on the console, and `twiddle()`, the spinning progress mark.
//!
//! Upstream: sys/lib/libsa/printf.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `printf(fmt, ...)` is the [`printf!`](crate::printf!) macro over Rust format strings
//!   (`docs/C_TO_RUST.md`: printf(9) becomes `kprintf!`; libsa does the same). `kdoprnt`'s
//!   conversions map to format specs: `%d`/`%u`/`%ld` to `{}`, `%x`/`%lx`/`%llx` to `{:x}`,
//!   `%08lx` to `{:08x}`, `%c` to a `char`, `%s` of a NUL-terminated byte string to
//!   [`Str`]. `%b` (a value and its bit names) has no caller in the boot programs ported
//!   here; when one comes it gets a `Display` helper as the kernel's `Bitmask`.
//! - `kdoprnt`, `kprintn` and `kprintn64` are `core::fmt`; the output still goes byte by byte
//!   through `putchar()`, so the console sees what the C sends (characters above U+00FF,
//!   which byte strings never produce, become `?`).

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::putchar::putchar;

/// `hexdig`: the digits of `kprintn`.
pub const HEXDIG: &[u8; 16] = b"0123456789abcdef";

/// A `fmt::Write` that sends every character to `putchar()`.
pub struct Putchar;

impl Write for Putchar {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            putchar(latin1(c));
        }
        Ok(())
    }
}

/// The byte a character stands for: the byte strings [`Str`] prints come back as the bytes
/// they were (Latin-1), anything else is `?`.
pub fn latin1(c: char) -> i32 {
    let v = u32::from(c);
    if v <= 0xff { v as i32 } else { i32::from(b'?') }
}

/// `%s` of a C string: the bytes up to the first NUL (or the end of the slice).
pub struct Str<'a>(pub &'a [u8]);

impl fmt::Display for Str<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &b in self.0.iter().take_while(|&&b| b != 0) {
            f.write_char(char::from(b))?;
        }
        Ok(())
    }
}

/// `printf(fmt, ...)`: format on the console.
#[macro_export]
macro_rules! printf {
    ($($arg:tt)*) => {
        $crate::printf::vprintf(format_args!($($arg)*))
    };
}

/// `vprintf(fmt, ap)`.
pub fn vprintf(args: fmt::Arguments<'_>) {
    let _ = Putchar.write_fmt(args);
}

/// `donottwiddle`: no progress marks when set.
pub static DONOTTWIDDLE: AtomicBool = AtomicBool::new(false);

/// `twiddle()`'s `static int pos`.
static TWIDDLE_POS: AtomicUsize = AtomicUsize::new(0);

/// `twiddle()`: print the next of `|/-\` and back up over it.
pub fn twiddle() {
    if !DONOTTWIDDLE.load(Ordering::Relaxed) {
        let pos = TWIDDLE_POS.fetch_add(1, Ordering::Relaxed);
        putchar(i32::from(b"|/-\\"[pos & 3]));
        putchar(i32::from(b'\x08'));
    }
}
