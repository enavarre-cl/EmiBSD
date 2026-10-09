/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1990, 1993
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
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by the University of
 *	California, Berkeley and its contributors.
 * 4. Neither the name of the University nor the names of its contributors
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
 */
/* </LICENSES> */

/* <CODE> */
//! `putchar()`: one character to the console, with tab expansion and erase handling.
//!
//! Upstream: sys/lib/libsa/putchar.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `pch_pos`, the output column, is the atomic [`PCH_POS`].

use core::sync::atomic::{AtomicI32, Ordering};

use crate::cons::cnputc;

/// `pch_pos`: the column the next character goes to.
pub static PCH_POS: AtomicI32 = AtomicI32::new(0);

/// `putchar(c)`.
pub fn putchar(c: i32) {
    match c {
        // DEL erases
        0o177 => {
            cnputc(i32::from(b'\x08'));
            cnputc(i32::from(b' '));
            backspace();
        }
        0x08 => backspace(),
        0x09 => loop {
            cnputc(i32::from(b' '));
            if (PCH_POS.fetch_add(1, Ordering::Relaxed) + 1) % 8 == 0 {
                break;
            }
        },
        0x0a | 0x0d => {
            cnputc(c);
            PCH_POS.store(0, Ordering::Relaxed);
        }
        _ => {
            cnputc(c);
            PCH_POS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The `'\b'` case, which DEL falls into.
fn backspace() {
    cnputc(i32::from(b'\x08'));
    if PCH_POS.load(Ordering::Relaxed) != 0 {
        PCH_POS.fetch_sub(1, Ordering::Relaxed);
    }
}
/* </CODE> */
