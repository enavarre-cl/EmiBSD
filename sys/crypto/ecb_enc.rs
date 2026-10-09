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

/* lib/des/ecb_enc.c */

/* Copyright (C) 1995 Eric Young (eay@mincom.oz.au)
 * All rights reserved.
 *
 * This file is part of an SSL implementation written
 * by Eric Young (eay@mincom.oz.au).
 * The implementation was written so as to conform with Netscapes SSL
 * specification.  This library and applications are
 * FREE FOR COMMERCIAL AND NON-COMMERCIAL USE
 * as long as the following conditions are aheared to.
 *
 * Copyright remains Eric Young's, and as such any Copyright notices in
 * the code are not to be removed.  If this code is used in a product,
 * Eric Young should be given attribution as the author of the parts used.
 * This can be in the form of a textual message at program startup or
 * in documentation (online or textual) provided with the package.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *    This product includes software developed by Eric Young (eay@mincom.oz.au)
 *
 * THIS SOFTWARE IS PROVIDED BY ERIC YOUNG ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * The licence and distribution terms for any publically available version or
 * derivative of this code cannot be changed.  i.e. this code cannot simply be
 * copied and put under another distribution licence
 * [including the GNU Public Licence.]
 */
/* </LICENSES> */

/* <CODE> */
//! The DES block transform: the 16 rounds of one DES encryption or decryption on a block
//! already put through the initial permutation, as `des_ecb3_encrypt` calls it.
//!
//! Upstream: sys/crypto/ecb_enc.c @ 3ce1f3f79392
//! LZ: sys/crypto/ecb_enc.rs@f5985f1d055a
//!
//! ## Deviations
//! - `encrypt` is a `bool`. `DES_USE_PTR` (an alternative way of indexing `des_SPtrans`,
//!   undefined in the C) is not ported. The C's final `l = r = t = u = 0` (plain stores the
//!   compiler may drop) has no counterpart.
//!
//! ## Redesign
//! - `des_encrypt2` is the method [`DesKeySchedule::encrypt2`]: it takes the two words by value
//!   and returns them (LZ: `&mut [u32; 2]` and the schedule as an argument). The rounds walk
//!   the subkeys in pairs, forwards to encrypt and backwards to decrypt, as the C's loops over
//!   the word index do.

use super::des_locl::{DesKeySchedule, d_encrypt};

impl DesKeySchedule {
    /// `des_encrypt2`: runs the sixteen rounds on `data` (left word, right word) with this
    /// schedule; `encrypt` false walks it backwards. The result is the transformed pair.
    pub fn encrypt2(&self, data: [u32; 2], encrypt: bool) -> [u32; 2] {
        let [u, r] = data;

        // Things have been modified so that the initial rotate is done outside the loop. This
        // required the des_SPtrans values in sp.h to be rotated 1 bit to the right. One perl
        // script later and things have a 5% speed up on a sparc2. Thanks to Richard Outerbridge
        // <71755.204@CompuServe.COM> for pointing this out.
        let mut l = r.rotate_left(1);
        let mut r = u.rotate_left(1);

        let pairs = self.ks.as_chunks::<2>().0;
        if encrypt {
            for [k1, k2] in pairs {
                l ^= d_encrypt(r, k1); //  1
                r ^= d_encrypt(l, k2); //  2
            }
        } else {
            for [k15, k16] in pairs.iter().rev() {
                l ^= d_encrypt(r, k16); // 16
                r ^= d_encrypt(l, k15); // 15
            }
        }

        [l.rotate_right(1), r.rotate_right(1)]
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypting_with_the_schedule_walked_backwards_undoes_it() {
        let key = [0x13, 0x34, 0x57, 0x79, 0x9b, 0xbc, 0xdf, 0xf1];
        let ks = DesKeySchedule::new(&key).expect("unchecked key");
        let orig = [0x0123_4567u32, 0x89ab_cdef];
        let data = ks.encrypt2(orig, true);
        assert_ne!(data, orig);
        assert_eq!(ks.encrypt2(data, false), orig);
    }
}
/* </TESTS> */
