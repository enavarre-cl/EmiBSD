/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989 Regents of the University of California.
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
 *
 *	@(#)ufs_subr.c	7.13 (Berkeley) 6/28/90
 */
/* </LICENSES> */

/* <CODE> */
//! `skpc`: skip the leading bytes of a string that equal a given byte.
//!
//! Upstream: sys/lib/libkern/skpc.c @ 3ce1f3f79392
//!
//! The fast file system's inode allocator uses it to skip the all-used bytes (`0xff`) of a
//! cylinder group's inode map (`ffs_nodealloccg`).
//!
//! ## Deviations
//! - The string is a slice (`size` is its length); `mask` is the byte itself, as the C's
//!   `(u_char)mask` cast makes it.

/// The number of bytes left in `cp` from the first byte that is not `mask`; 0 when every
/// byte is `mask`.
pub fn skpc(mask: u8, cp: &[u8]) -> usize {
    match cp.iter().position(|&c| c != mask) {
        Some(i) => cp.len() - i,
        None => 0,
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_from_the_first_other_byte_to_the_end() {
        assert_eq!(skpc(0xff, &[]), 0);
        assert_eq!(skpc(0xff, &[0xff, 0xff]), 0);
        assert_eq!(skpc(0xff, &[0xff, 0x7f, 0xff]), 2);
        assert_eq!(skpc(0, &[1, 0]), 2);
    }
}
/* </TESTS> */
