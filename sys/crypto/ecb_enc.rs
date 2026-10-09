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
//!
//! ## Deviations
//! - `des_encrypt2` takes the two words as `&mut [u32; 2]` and `encrypt` as a `bool`. The loops
//!   run the sixteen rounds in pairs as the C does; `DES_USE_PTR` (an alternative way of
//!   indexing `des_SPtrans`, undefined in the C) is not ported.

use super::des_locl::{DesKeySchedule, d_encrypt};

/// `des_encrypt2`: runs the rounds on `data` (left word, right word) with the schedule `ks`;
/// `encrypt` false walks it backwards.
pub fn des_encrypt2(data: &mut [u32; 2], ks: &DesKeySchedule, encrypt: bool) {
    let u = data[0];
    let r = data[1];

    // Things have been modified so that the initial rotate is done outside the loop. This
    // required the des_SPtrans values in sp.h to be rotated 1 bit to the right. One perl script
    // later and things have a 5% speed up on a sparc2. Thanks to Richard Outerbridge
    // <71755.204@CompuServe.COM> for pointing this out.
    let mut l = r.rotate_left(1);
    let mut r = u.rotate_left(1);

    if encrypt {
        for i in (0..32).step_by(4) {
            d_encrypt(&mut l, r, ks, i); //  1
            d_encrypt(&mut r, l, ks, i + 2); //  2
        }
    } else {
        for i in (1..=30).rev().step_by(4) {
            d_encrypt(&mut l, r, ks, i); // 16
            d_encrypt(&mut r, l, ks, i - 2); // 15
        }
    }
    l = l.rotate_right(1);
    r = r.rotate_right(1);

    data[0] = l;
    data[1] = r;
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::set_key::des_set_key;

    #[test]
    fn decrypting_with_the_schedule_walked_backwards_undoes_it() {
        let key = [0x13, 0x34, 0x57, 0x79, 0x9b, 0xbc, 0xdf, 0xf1];
        let mut ks = [0u32; 32];
        assert_eq!(des_set_key(&key, &mut ks), Ok(()));
        let orig = [0x0123_4567u32, 0x89ab_cdef];
        let mut data = orig;
        des_encrypt2(&mut data, &ks, true);
        assert_ne!(data, orig);
        des_encrypt2(&mut data, &ks, false);
        assert_eq!(data, orig);
    }
}
/* </TESTS> */
