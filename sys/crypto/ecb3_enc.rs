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

/* lib/des/ecb3_enc.c */

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
//! Triple DES (EDE, three keys) of one 8-byte block: `des3_encrypt` and `des3_decrypt` of
//! `xform.c`.
//!
//! Upstream: sys/crypto/ecb3_enc.c @ 3ce1f3f79392
//! LZ: sys/crypto/ecb3_enc.rs@f5985f1d055a
//!
//! ## Deviations
//! - `encrypt` is a `bool`.
//!
//! ## Redesign
//! - [`des_ecb3_encrypt`] returns the output block (LZ: an `&mut DesCblock` out parameter; the
//!   C's `input` and `output` may be one buffer, which a returned value makes moot).

use super::des_locl::{DesCblock, DesKeySchedule, c2l, fp, ip, l2c};

/// `des_ecb3_encrypt`: the encryption (or decryption) of one block under the three key
/// schedules: the DES transform with `ks1`, the opposite with `ks2`, the first again with
/// `ks3`.
pub fn des_ecb3_encrypt(
    input: &DesCblock,
    ks1: &DesKeySchedule,
    ks2: &DesKeySchedule,
    ks3: &DesKeySchedule,
    encrypt: bool,
) -> DesCblock {
    let [l0, l1] = c2l(input);
    let (l0, l1) = ip(l0, l1);
    let mut ll = [l0, l1];
    ll = ks1.encrypt2(ll, encrypt);
    ll = ks2.encrypt2(ll, !encrypt);
    ll = ks3.encrypt2(ll, encrypt);
    let [l0, l1] = ll;
    let (l1, l0) = fp(l1, l0);
    l2c([l0, l1])
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests for DES and triple DES: the classic DES example (key 133457799BBCDFF1),
    // the FIPS 81 example and the first NIST SP 800-20 variable-plaintext vectors, each as EDE
    // with one key thrice; three-key vectors computed with `openssl`'s `des-ede3`; the
    // decryption order `xform.c` uses (the schedules reversed, `encrypt` false); a property
    // test of the round trip over random keys and blocks; and a reference-backed comparison of
    // the tables with the C files'.

    use super::*;
    use crate::crypto::podd::ODD_PARITY;
    use crate::crypto::testutil::{c_table, hex, hexn};

    extern crate std;
    use std::vec::Vec;

    fn schedule(key: &str) -> DesKeySchedule {
        let k: DesCblock = hexn(key);
        match DesKeySchedule::new(&k) {
            Ok(ks) => ks,
            Err(e) => panic!("key {key}: {e:?}"),
        }
    }

    fn ede3(blk: &[u8], k1: &str, k2: &str, k3: &str, encrypt: bool) -> Vec<u8> {
        let (s1, s2, s3) = (schedule(k1), schedule(k2), schedule(k3));
        let mut input: DesCblock = [0; 8];
        input.copy_from_slice(blk);
        if encrypt {
            des_ecb3_encrypt(&input, &s1, &s2, &s3, true).to_vec()
        } else {
            // des3_decrypt: the schedules in the opposite order, encrypt false.
            des_ecb3_encrypt(&input, &s3, &s2, &s1, false).to_vec()
        }
    }

    #[test]
    fn des_known_answers_as_ede_with_one_key() {
        let cases = [
            // The textbook example.
            ("133457799bbcdff1", "0123456789abcdef", "85e813540f0ab405"),
            // FIPS 81, appendix B: "Now is t" under 0123456789abcdef.
            ("0123456789abcdef", "4e6f772069732074", "3fa40e8a984d4815"),
            // NIST SP 800-20, variable plaintext, key 0101010101010101.
            ("0101010101010101", "8000000000000000", "95f8a5e5dd31d900"),
            ("0101010101010101", "4000000000000000", "dd7f121ca5015619"),
            ("0101010101010101", "2000000000000000", "2e8653104f3834ea"),
        ];
        for (k, pt, ct) in cases {
            assert_eq!(ede3(&hex(pt), k, k, k, true), hex(ct), "key {k} pt {pt}");
            assert_eq!(ede3(&hex(ct), k, k, k, false), hex(pt), "key {k} ct {ct}");
        }
    }

    const K1: &str = "0123456789abcdef";
    const K2: &str = "23456789abcdef01";
    const K3: &str = "456789abcdef0123";

    #[test]
    fn three_key_vectors() {
        assert_eq!(
            ede3(&hex("6bc1bee22e409f96"), K1, K2, K3, true),
            hex("714772f339841d34")
        );
        // "Now is the time " in two blocks.
        assert_eq!(ede3(b"Now is t", K1, K2, K3, true), hex("314f8327fa7a09a8"));
        assert_eq!(ede3(b"he time ", K1, K2, K3, true), hex("4362760cc13ba7da"));
        for ct in ["714772f339841d34", "314f8327fa7a09a8", "4362760cc13ba7da"] {
            let pt = ede3(&hex(ct), K1, K2, K3, false);
            assert_eq!(ede3(&pt, K1, K2, K3, true), hex(ct));
        }
        assert_eq!(
            ede3(&hex("714772f339841d34"), K1, K2, K3, false),
            hex("6bc1bee22e409f96")
        );
    }

    #[test]
    fn encrypt_flag_false_is_the_inverse_with_the_same_schedule_order() {
        let (s1, s2, s3) = (schedule(K1), schedule(K2), schedule(K3));
        let pt: DesCblock = hexn("0011223344556677");
        let ct = des_ecb3_encrypt(&pt, &s1, &s2, &s3, true);
        // Decryption runs ks1 backwards, ks2 forwards, ks3 backwards: undoing ks3, ks2, ks1 needs
        // the order reversed, as xform.c passes them.
        assert_eq!(des_ecb3_encrypt(&ct, &s3, &s2, &s1, false), pt);
    }

    /// xorshift64: the property tests' generator.
    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    #[test]
    fn round_trips_over_random_keys_and_blocks() {
        let mut st = 0x9e37_79b9_7f4a_7c15u64;
        // Keys with odd parity, as real DES keys have (`des_check_key` would accept them; a
        // weak key is a 2^-52 event).
        let key = |st: &mut u64| {
            let k = next(st).to_le_bytes().map(|b| ODD_PARITY[usize::from(b)]);
            DesKeySchedule::new(&k).expect("odd parity")
        };
        for _ in 0..200 {
            let (s1, s2, s3) = (key(&mut st), key(&mut st), key(&mut st));
            let pt = next(&mut st).to_le_bytes();
            let ct = des_ecb3_encrypt(&pt, &s1, &s2, &s3, true);
            assert_eq!(des_ecb3_encrypt(&ct, &s3, &s2, &s1, false), pt);
            // One key thrice is single DES, and the rounds alone invert each other.
            let one = des_ecb3_encrypt(&pt, &s1, &s1, &s1, true);
            assert_eq!(des_ecb3_encrypt(&one, &s1, &s1, &s1, false), pt);
            let w = [next(&mut st) as u32, next(&mut st) as u32];
            assert_eq!(s2.encrypt2(s2.encrypt2(w, true), false), w);
        }
    }

    #[test]
    #[ignore = "reads the C tables from $OPENBSD_SRC (just test-ref)"]
    fn tables_match_the_c_files() {
        use crate::crypto::sk::DES_SKB;
        use crate::crypto::spr::DES_SPTRANS;

        let skb: Vec<u64> = DES_SKB.iter().flatten().map(|w| u64::from(*w)).collect();
        assert_eq!(c_table("sys/crypto/sk.h", "des_skb"), skb);
        let sp: Vec<u64> = DES_SPTRANS
            .iter()
            .flatten()
            .map(|w| u64::from(*w))
            .collect();
        assert_eq!(c_table("sys/crypto/spr.h", "des_SPtrans"), sp);
        let odd: Vec<u64> = ODD_PARITY.iter().map(|b| u64::from(*b)).collect();
        assert_eq!(c_table("sys/crypto/podd.h", "odd_parity"), odd);
    }
}
/* </TESTS> */
