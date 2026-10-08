/*	$OpenBSD: set_key.c,v 1.6 2025/10/27 16:56:00 tb Exp $	*/
/* <LICENSES> */
/* lib/des/set_key.c */

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

/* set_key.c v 1.4 eay 24/9/91
 * 1.4 Speed up by 400% :-)
 * 1.3 added register declarations.
 * 1.2 unrolled make_key_sched a bit more
 * 1.1 added norm_expand_bits
 * 1.0 First working version
 */
/* </LICENSES> */

/* <CODE> */
//! The DES key schedule (`des_set_key`), the odd-parity and weak-key checks, the one
//! used by `des3_setkey` in `xform.c`.
//!
//! Upstream: sys/crypto/set_key.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `des_set_key` returns `Result<(), Errno>`: `EINVAL` where the C returns -1 (parity error)
//!   or -2 (weak key), which only happens when `des_check_key` is set (it is 0, and nothing in
//!   the kernel sets it). `des_check_key` is an `AtomicI32`.
//! - The key is a `&DesCblock` and the schedule a `&mut DesKeySchedule` (32 words); the 8
//!   `HPERM_OP`/`PERM_OP` steps keep the C's operation order. The commented-out 60-operation
//!   PC1 of the C is not carried over.

use core::sync::atomic::{AtomicI32, Ordering};

use super::des_locl::{DesCblock, DesKeySchedule, ITERATIONS, c2l, perm_op};
use super::podd::ODD_PARITY;
use super::sk::DES_SKB;
use crate::sys::errno::Errno;

/// `des_check_key`: nonzero makes `des_set_key` check parity and weakness.
pub static DES_CHECK_KEY: AtomicI32 = AtomicI32::new(0);

/// `check_parity`: true when every byte of the key has odd parity.
fn check_parity(key: &DesCblock) -> bool {
    key.iter().all(|b| *b == ODD_PARITY[*b as usize])
}

/// `NUM_WEAK_KEY`.
const NUM_WEAK_KEY: usize = 16;

/// Weak and semi-weak keys as taken from
/// %A D.W. Davies
/// %A W.L. Price
/// %T Security for Computer Networks
/// %I John Wiley & Sons
/// %D 1984
/// Many thanks to smb@ulysses.att.com (Steven Bellovin) for the reference (and actual cblock
/// values).
static WEAK_KEYS: [DesCblock; NUM_WEAK_KEY] = [
    // weak keys
    [0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01],
    [0xFE, 0xFE, 0xFE, 0xFE, 0xFE, 0xFE, 0xFE, 0xFE],
    [0x1F, 0x1F, 0x1F, 0x1F, 0x0E, 0x0E, 0x0E, 0x0E],
    [0xE0, 0xE0, 0xE0, 0xE0, 0xF1, 0xF1, 0xF1, 0xF1],
    // semi-weak keys
    [0x01, 0xFE, 0x01, 0xFE, 0x01, 0xFE, 0x01, 0xFE],
    [0xFE, 0x01, 0xFE, 0x01, 0xFE, 0x01, 0xFE, 0x01],
    [0x1F, 0xE0, 0x1F, 0xE0, 0x0E, 0xF1, 0x0E, 0xF1],
    [0xE0, 0x1F, 0xE0, 0x1F, 0xF1, 0x0E, 0xF1, 0x0E],
    [0x01, 0xE0, 0x01, 0xE0, 0x01, 0xF1, 0x01, 0xF1],
    [0xE0, 0x01, 0xE0, 0x01, 0xF1, 0x01, 0xF1, 0x01],
    [0x1F, 0xFE, 0x1F, 0xFE, 0x0E, 0xFE, 0x0E, 0xFE],
    [0xFE, 0x1F, 0xFE, 0x1F, 0xFE, 0x0E, 0xFE, 0x0E],
    [0x01, 0x1F, 0x01, 0x1F, 0x01, 0x0E, 0x01, 0x0E],
    [0x1F, 0x01, 0x1F, 0x01, 0x0E, 0x01, 0x0E, 0x01],
    [0xE0, 0xFE, 0xE0, 0xFE, 0xF1, 0xFE, 0xF1, 0xFE],
    [0xFE, 0xE0, 0xFE, 0xE0, 0xFE, 0xF1, 0xFE, 0xF1],
];

/// `des_is_weak_key`: true for a weak or semi-weak key.
pub fn des_is_weak_key(key: &DesCblock) -> bool {
    WEAK_KEYS.iter().any(|w| w == key)
}

/// `HPERM_OP`.
fn hperm_op(a: &mut u32, n: i32, m: u32) {
    let sh = (16 - n) as u32;
    let t = ((*a << sh) ^ *a) & m;
    *a = *a ^ t ^ (t >> sh);
}

/// `shifts2`: the rotations of the key halves in each iteration.
static SHIFTS2: [bool; 16] = [
    false, false, true, true, true, true, true, true, false, true, true, true, true, true, true,
    false,
];

/// `des_set_key`: computes the key schedule of the 8-byte `key`. When `des_check_key` is set
/// an error for a key with a parity error (-1 in the C) or a weak key (-2).
pub fn des_set_key(key: &DesCblock, schedule: &mut DesKeySchedule) -> Result<(), Errno> {
    if DES_CHECK_KEY.load(Ordering::Relaxed) != 0 && (!check_parity(key) || des_is_weak_key(key)) {
        return Err(Errno::EINVAL);
    }

    let mut c = c2l(&key[0..]);
    let mut d = c2l(&key[4..]);

    // I now do it in 47 simple operations :-) Thanks to John Fletcher
    // (john_fletcher@lccmail.ocf.llnl.gov) for the inspiration. :-)
    perm_op(&mut d, &mut c, 4, 0x0f0f0f0f);
    hperm_op(&mut c, -2, 0xcccc0000);
    hperm_op(&mut d, -2, 0xcccc0000);
    perm_op(&mut d, &mut c, 1, 0x55555555);
    perm_op(&mut c, &mut d, 8, 0x00ff00ff);
    perm_op(&mut d, &mut c, 1, 0x55555555);
    d = ((d & 0x000000ff) << 16)
        | (d & 0x0000ff00)
        | ((d & 0x00ff0000) >> 16)
        | ((c & 0xf0000000) >> 4);
    c &= 0x0fffffff;

    for i in 0..ITERATIONS {
        if SHIFTS2[i] {
            c = (c >> 2) | (c << 26);
            d = (d >> 2) | (d << 26);
        } else {
            c = (c >> 1) | (c << 27);
            d = (d >> 1) | (d << 27);
        }
        c &= 0x0fffffff;
        d &= 0x0fffffff;
        // could be a few less shifts but I am to lazy at this point in time to investigate
        let mut s = DES_SKB[0][(c & 0x3f) as usize]
            | DES_SKB[1][(((c >> 6) & 0x03) | ((c >> 7) & 0x3c)) as usize]
            | DES_SKB[2][(((c >> 13) & 0x0f) | ((c >> 14) & 0x30)) as usize]
            | DES_SKB[3][(((c >> 20) & 0x01) | ((c >> 21) & 0x06) | ((c >> 22) & 0x38)) as usize];
        let t = DES_SKB[4][(d & 0x3f) as usize]
            | DES_SKB[5][(((d >> 7) & 0x03) | ((d >> 8) & 0x3c)) as usize]
            | DES_SKB[6][((d >> 15) & 0x3f) as usize]
            | DES_SKB[7][(((d >> 21) & 0x0f) | ((d >> 22) & 0x30)) as usize];

        // table contained 0213 4657
        schedule[2 * i] = (t << 16) | (s & 0x0000ffff);
        s = (s >> 16) | (t & 0xffff0000);

        s = s.rotate_left(4);
        schedule[2 * i + 1] = s;
    }
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odd_parity_table_and_check() {
        for x in 0..=255u8 {
            let y = ODD_PARITY[x as usize];
            assert_eq!(y.count_ones() % 2, 1, "{x:#x}");
            assert_eq!(y & 0xfe, x & 0xfe, "{x:#x}");
        }
        assert!(check_parity(&[0x01; 8]));
        assert!(check_parity(&[
            0x13, 0x34, 0x57, 0x79, 0x9b, 0xbc, 0xdf, 0xf1
        ]));
        assert!(!check_parity(&[0x00; 8]));
        assert!(!check_parity(&[
            0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x03
        ]));
    }

    #[test]
    fn weak_and_semi_weak_keys() {
        for k in WEAK_KEYS.iter() {
            assert!(des_is_weak_key(k));
            // Weak keys have odd parity, so only the weakness test can reject them.
            assert!(check_parity(k));
        }
        assert!(!des_is_weak_key(&[
            0x13, 0x34, 0x57, 0x79, 0x9b, 0xbc, 0xdf, 0xf1
        ]));
    }

    #[test]
    fn des_set_key_checks_only_when_asked() {
        let mut ks = [0u32; 32];
        let weak = [0x01u8; 8];
        let bad_parity = [0x00u8; 8];
        assert_eq!(des_set_key(&weak, &mut ks), Ok(()));
        assert_eq!(des_set_key(&bad_parity, &mut ks), Ok(()));
        DES_CHECK_KEY.store(1, Ordering::Relaxed);
        let (a, b) = (
            des_set_key(&weak, &mut ks),
            des_set_key(&bad_parity, &mut ks),
        );
        DES_CHECK_KEY.store(0, Ordering::Relaxed);
        assert_eq!((a, b), (Err(Errno::EINVAL), Err(Errno::EINVAL)));
    }

    #[test]
    fn the_schedule_of_the_fips_example_key() {
        // The first subkey of key 133457799BBCDFF1 is 000110 110000 001011 101111 111111 000111
        // 000001 110010 (the classic worked example), spread over two words as the tables
        // arrange it; the sixteenth is the other end of the schedule. Check them through the
        // cipher in `ecb3_enc`'s tests; here, that the schedule is deterministic and fills
        // every word.
        let mut a = [0u32; 32];
        let mut b = [0xffff_ffffu32; 32];
        let k = [0x13, 0x34, 0x57, 0x79, 0x9b, 0xbc, 0xdf, 0xf1];
        assert_eq!(des_set_key(&k, &mut a), Ok(()));
        assert_eq!(des_set_key(&k, &mut b), Ok(()));
        assert_eq!(a, b);
        assert!(a.iter().any(|w| *w != 0));
    }
}
/* </TESTS> */
