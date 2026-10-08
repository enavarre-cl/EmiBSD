/*	$OpenBSD: getsn.c,v 1.7 2018/04/25 11:15:58 dlg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 Theo de Raadt
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `getsn`: read a line from the console, with echo and simple editing.
//!
//! Upstream: sys/lib/libkern/getsn.c @ 3ce1f3f79392
//!
//! The kernel's few interactive prompts read their answers with it: `boot -c`'s `UKC>`
//! (`kern/subr_userconf.c`) and the root device question of `boot -a`. Each key is read with
//! `cngetc` and echoed; backspace and DEL erase the last character, `^U` the whole line, a tab
//! is a space, and a key that does not fit or is a control character rings the bell. Return or
//! newline ends the line, which is NUL-terminated in the buffer.
//!
//! ## Deviations
//! - This crate is a leaf (it cannot call into `bsd`), so the console is the [`GetsnCons`]
//!   argument: its `cngetc` is the C's `cngetc()` and its `cnputs` the C's `printf` of the
//!   echo, the bell and the erase sequence.
//! - The buffer is a slice (`size` is its length). A zero-length buffer cannot hold even the
//!   terminating NUL: every key rings the bell and the line ends without storing anything,
//!   where the C would write one byte past it.

/// The console `getsn` reads keys from and echoes to.
pub trait GetsnCons {
    /// `cngetc()`: the next key, waiting for it.
    fn cngetc(&mut self) -> i32;

    /// `printf("%s", s)`: writes the bytes to the console.
    fn cnputs(&mut self, s: &[u8]);
}

/// `getsn(cp, size)`: reads a line into `cp`, NUL-terminated, echoing on `cons`; returns its
/// length without the NUL.
pub fn getsn(cp: &mut [u8], cons: &mut impl GetsnCons) -> usize {
    let mut len = 0usize;

    loop {
        let mut c = cons.cngetc();
        match c {
            0x0a | 0x0d => {
                // '\n', '\r'
                cons.cnputs(b"\n");
                if let Some(end) = cp.get_mut(len) {
                    *end = 0;
                }
                return len;
            }
            0x08 | 0x7f => {
                // '\b', DEL
                if len > 0 {
                    cons.cnputs(b"\x08 \x08");
                    len -= 1;
                }
            }
            0x15 => {
                // 'u' & 037: ^U kills the line
                while len > 0 {
                    cons.cnputs(b"\x08 \x08");
                    len -= 1;
                }
            }
            _ => {
                if c == 0x09 {
                    c = i32::from(b' ');
                }
                if len + 1 >= cp.len() || c < i32::from(b' ') {
                    cons.cnputs(b"\x07");
                    continue;
                }
                // `printf("%c", c)` and `*lp++ = c` keep the low byte, as `char` does.
                let b = c as u8;
                cons.cnputs(&[b]);
                cp[len] = b;
                len += 1;
            }
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    struct Script {
        keys: Vec<i32>,
        next: usize,
        out: Vec<u8>,
    }

    impl Script {
        fn new(keys: &[u8]) -> Self {
            Self {
                keys: keys.iter().map(|&k| i32::from(k)).collect(),
                next: 0,
                out: Vec::new(),
            }
        }
    }

    impl GetsnCons for Script {
        fn cngetc(&mut self) -> i32 {
            let k = self.keys[self.next];
            self.next += 1;
            k
        }

        fn cnputs(&mut self, s: &[u8]) {
            self.out.extend_from_slice(s);
        }
    }

    fn run(keys: &[u8], size: usize) -> (usize, Vec<u8>, Vec<u8>) {
        let mut buf = std::vec![0xaau8; size];
        let mut s = Script::new(keys);
        let n = getsn(&mut buf, &mut s);
        (n, buf, s.out)
    }

    #[test]
    fn reads_a_line_with_echo() {
        let (n, buf, out) = run(b"list\r", 40);
        assert_eq!(n, 4);
        assert_eq!(&buf[..5], b"list\0");
        assert_eq!(out, b"list\n");
        let (n, buf, _) = run(b"q\n", 40);
        assert_eq!((n, &buf[..2]), (1, &b"q\0"[..]));
    }

    #[test]
    fn edits_backspace_del_and_kill() {
        let (n, buf, out) = run(b"ab\x08c\r", 40);
        assert_eq!((n, &buf[..3]), (2, &b"ac\0"[..]));
        assert_eq!(out, b"ab\x08 \x08c\n");
        let (n, buf, _) = run(b"xy\x7f\x7f\x7fz\n", 40);
        assert_eq!((n, &buf[..2]), (1, &b"z\0"[..]));
        let (n, buf, out) = run(b"abc\x15d\r", 40);
        assert_eq!((n, &buf[..2]), (1, &b"d\0"[..]));
        assert_eq!(out, b"abc\x08 \x08\x08 \x08\x08 \x08d\n");
    }

    #[test]
    fn tab_is_a_space_and_controls_ring_the_bell() {
        let (n, buf, out) = run(b"a\tb\x01\r", 40);
        assert_eq!((n, &buf[..4]), (3, &b"a b\0"[..]));
        assert_eq!(out, b"a b\x07\n");
    }

    #[test]
    fn a_full_buffer_rings_the_bell_and_keeps_room_for_the_nul() {
        // size 4: three characters and the NUL.
        let (n, buf, out) = run(b"abcde\r", 4);
        assert_eq!((n, &buf[..]), (3, &b"abc\0"[..]));
        assert_eq!(out, b"abc\x07\x07\n");
        // size 1: only the NUL fits.
        let (n, buf, out) = run(b"a\r", 1);
        assert_eq!((n, &buf[..]), (0, &b"\0"[..]));
        assert_eq!(out, b"\x07\n");
        // size 0: nothing fits.
        let (n, _, out) = run(b"a\r", 0);
        assert_eq!(n, 0);
        assert_eq!(out, b"\x07\n");
    }
}
/* </TESTS> */
