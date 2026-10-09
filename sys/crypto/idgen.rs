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

/*
 * Copyright (c) 2008 Damien Miller <djm@mindrot.org>
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
/* </LICENSES> */

/* <CODE> */
//! IDGEN32: non-repeating identifiers covering an almost maximal 32-bit range. A 31-bit
//! counter, started at a random offset, goes through a keyed 31-bit Feistel permutation
//! (IDGEN32 is based on Greg Rose's public domain SKIP32); the top bit flips at every rekey
//! so that identifiers of two consecutive keys never collide. Zero is never returned.
//!
//! Upstream: sys/crypto/idgen.h @ 3ce1f3f79392, sys/crypto/idgen.c @ 3ce1f3f79392
//! LZ: sys/crypto/idgen.rs@f5985f1d055a
//!
//! ## Deviations
//! - The header and the file share this module (one name, as `docs/C_TO_RUST.md` does for
//!   `.h`/`.c` pairs).
//! - `struct idgen32_ctx` is [`Idgen32Ctx`]; `init` stores a fresh context instead of
//!   `bzero` over its bytes. The caller provides the locking, as in C.
//!
//! ## Redesign
//! - The functions over a context are methods: `idgen32_init` is [`Idgen32Ctx::init`],
//!   `idgen32` is [`Idgen32Ctx::generate`], and the private `idgen32_permute` and
//!   `idgen32_rekey` are `permute` and `rekey` (LZ: free functions with the context first).
//!   `init` stays a method on the caller's storage: the generators live in `static`s that
//!   are set up on first use.
//! - The round counter of the permutation is a `u16` and the halves are taken with
//!   `to_be_bytes`, so the `as` casts of the C's arithmetic go; the values are the same.

use crate::dev::rnd::{arc4random, arc4random_buf};
use crate::kern::kern_tc::getuptime;
use crate::sys::types::Time;

/// `IDGEN32_ROUNDS`: Feistel rounds of the permutation.
pub const IDGEN32_ROUNDS: u32 = 31;
/// `IDGEN32_KEYLEN`: key bytes.
pub const IDGEN32_KEYLEN: usize = 32;
/// `IDGEN32_REKEY_LIMIT`: identifiers handed out under one key before a rekey.
pub const IDGEN32_REKEY_LIMIT: u32 = 0x6000_0000;
/// `IDGEN32_REKEY_TIME`: seconds a key lives at most.
pub const IDGEN32_REKEY_TIME: Time = 600;

/// `struct idgen32_ctx`: the state of one generator.
#[derive(Clone, Copy, Default)]
pub struct Idgen32Ctx {
    /// `id32_counter`: identifiers handed out under the current key.
    pub id32_counter: u32,
    /// `id32_offset`: random start of the counter for this key.
    pub id32_offset: u32,
    /// `id32_hibit`: the top bit, flipped at every rekey.
    pub id32_hibit: u32,
    /// `id32_key`: the permutation key.
    pub id32_key: [u8; IDGEN32_KEYLEN],
    /// `id32_rekey_time`: uptime after which the key is replaced.
    pub id32_rekey_time: Time,
}

impl Idgen32Ctx {
    /// An all-zero context (a C `static struct idgen32_ctx`), to be set up by
    /// [`Idgen32Ctx::init`].
    pub const fn zeroed() -> Self {
        Self {
            id32_counter: 0,
            id32_offset: 0,
            id32_hibit: 0,
            id32_key: [0; IDGEN32_KEYLEN],
            id32_rekey_time: 0,
        }
    }
}

/// `idgen32_ftable`: the SKIP32 F table.
static IDGEN32_FTABLE: [u8; 256] = [
    0xa3, 0xd7, 0x09, 0x83, 0xf8, 0x48, 0xf6, 0xf4, //
    0xb3, 0x21, 0x15, 0x78, 0x99, 0xb1, 0xaf, 0xf9, //
    0xe7, 0x2d, 0x4d, 0x8a, 0xce, 0x4c, 0xca, 0x2e, //
    0x52, 0x95, 0xd9, 0x1e, 0x4e, 0x38, 0x44, 0x28, //
    0x0a, 0xdf, 0x02, 0xa0, 0x17, 0xf1, 0x60, 0x68, //
    0x12, 0xb7, 0x7a, 0xc3, 0xe9, 0xfa, 0x3d, 0x53, //
    0x96, 0x84, 0x6b, 0xba, 0xf2, 0x63, 0x9a, 0x19, //
    0x7c, 0xae, 0xe5, 0xf5, 0xf7, 0x16, 0x6a, 0xa2, //
    0x39, 0xb6, 0x7b, 0x0f, 0xc1, 0x93, 0x81, 0x1b, //
    0xee, 0xb4, 0x1a, 0xea, 0xd0, 0x91, 0x2f, 0xb8, //
    0x55, 0xb9, 0xda, 0x85, 0x3f, 0x41, 0xbf, 0xe0, //
    0x5a, 0x58, 0x80, 0x5f, 0x66, 0x0b, 0xd8, 0x90, //
    0x35, 0xd5, 0xc0, 0xa7, 0x33, 0x06, 0x65, 0x69, //
    0x45, 0x00, 0x94, 0x56, 0x6d, 0x98, 0x9b, 0x76, //
    0x97, 0xfc, 0xb2, 0xc2, 0xb0, 0xfe, 0xdb, 0x20, //
    0xe1, 0xeb, 0xd6, 0xe4, 0xdd, 0x47, 0x4a, 0x1d, //
    0x42, 0xed, 0x9e, 0x6e, 0x49, 0x3c, 0xcd, 0x43, //
    0x27, 0xd2, 0x07, 0xd4, 0xde, 0xc7, 0x67, 0x18, //
    0x89, 0xcb, 0x30, 0x1f, 0x8d, 0xc6, 0x8f, 0xaa, //
    0xc8, 0x74, 0xdc, 0xc9, 0x5d, 0x5c, 0x31, 0xa4, //
    0x70, 0x88, 0x61, 0x2c, 0x9f, 0x0d, 0x2b, 0x87, //
    0x50, 0x82, 0x54, 0x64, 0x26, 0x7d, 0x03, 0x40, //
    0x34, 0x4b, 0x1c, 0x73, 0xd1, 0xc4, 0xfd, 0x3b, //
    0xcc, 0xfb, 0x7f, 0xab, 0xe6, 0x3e, 0x5b, 0xa5, //
    0xad, 0x04, 0x23, 0x9c, 0x14, 0x51, 0x22, 0xf0, //
    0x29, 0x79, 0x71, 0x7e, 0xff, 0x8c, 0x0e, 0xe2, //
    0x0c, 0xef, 0xbc, 0x72, 0x75, 0x6f, 0x37, 0xa1, //
    0xec, 0xd3, 0x8e, 0x62, 0x8b, 0x86, 0x10, 0xe8, //
    0x08, 0x77, 0x11, 0xbe, 0x92, 0x4f, 0x24, 0xc5, //
    0x32, 0x36, 0x9d, 0xcf, 0xf3, 0xa6, 0xbb, 0xac, //
    0x5e, 0x6c, 0xa9, 0x13, 0x57, 0x25, 0xb5, 0xe3, //
    0xbd, 0xa8, 0x3a, 0x01, 0x05, 0x59, 0x2a, 0x46, //
];

/// `idgen32_g`: the keyed G function of round `k` on the 16-bit word `w`.
fn idgen32_g(key: &[u8; IDGEN32_KEYLEN], k: u16, w: u16) -> u16 {
    let o = usize::from(k) * 4;
    let kb = |i: usize| key[(o + i) & (IDGEN32_KEYLEN - 1)];

    let [g1, g2] = w.to_be_bytes();

    let g3 = IDGEN32_FTABLE[usize::from(g2 ^ kb(0))] ^ g1;
    let g4 = IDGEN32_FTABLE[usize::from(g3 ^ kb(1))] ^ g2;
    let g5 = IDGEN32_FTABLE[usize::from(g4 ^ kb(2))] ^ g3;
    let g6 = IDGEN32_FTABLE[usize::from(g5 ^ kb(3))] ^ g4;

    u16::from_be_bytes([g5, g6])
}

impl Idgen32Ctx {
    /// `idgen32_permute`: the keyed permutation of the 31-bit value `in_`: a 15-bit left half
    /// and a 16-bit right half.
    fn permute(&self, in_: u32) -> u32 {
        let [h0, h1, l0, l1] = in_.to_be_bytes();
        let mut wl = u16::from_be_bytes([h0, h1]) & 0x7fff;
        let mut wr = u16::from_be_bytes([l0, l1]);
        let mut r: u16 = 0;

        // Doubled up rounds, with an odd round at the end to swap.
        for _ in 0..IDGEN32_ROUNDS / 2 {
            wr ^= idgen32_g(&self.id32_key, r, wl) ^ r;
            r += 1;
            wl ^= (idgen32_g(&self.id32_key, r, wr) ^ r) & 0x7fff;
            r += 1;
        }
        wr ^= idgen32_g(&self.id32_key, r, wl) ^ r;

        (u32::from(wl) << 16) | u32::from(wr)
    }

    /// `idgen32_rekey`: a new key, offset and deadline; the top bit flips.
    fn rekey(&mut self) {
        self.id32_counter = 0;
        self.id32_hibit ^= 0x8000_0000;
        self.id32_offset = arc4random();
        arc4random_buf(&mut self.id32_key);
        self.id32_rekey_time = getuptime() + IDGEN32_REKEY_TIME;
    }

    /// `idgen32_init`: a random top bit and a first key.
    pub fn init(&mut self) {
        *self = Idgen32Ctx::zeroed();
        self.id32_hibit = arc4random() & 0x8000_0000;
        self.rekey();
    }

    /// `idgen32`: the next identifier, never zero.
    pub fn generate(&mut self) -> u32 {
        loop {
            // Rekey a little early to avoid "card counting" attack.
            if self.id32_counter > IDGEN32_REKEY_LIMIT || self.id32_rekey_time < getuptime() {
                self.rekey();
            }
            let in_ = self.id32_offset.wrapping_add(self.id32_counter) & 0x7fff_ffff;
            self.id32_counter = self.id32_counter.wrapping_add(1);
            let ret = self.id32_hibit | self.permute(in_);
            // Zero IDs are often special, so avoid.
            if ret != 0 {
                return ret;
            }
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn idgen32_ids_distinct_nonzero() {
        let mut ctx = Idgen32Ctx::zeroed();
        ctx.init();
        let hibit = ctx.id32_hibit;
        let mut seen = HashSet::new();
        for _ in 0..8192 {
            let id = ctx.generate();
            assert_ne!(id, 0);
            assert_eq!(id & 0x8000_0000, hibit);
            assert!(seen.insert(id), "id {id:#x} repeated within one key");
        }
    }

    #[test]
    fn idgen32_permute_is_injective_on_a_range() {
        let mut ctx = Idgen32Ctx::zeroed();
        arc4random_buf(&mut ctx.id32_key);
        let out: HashSet<u32> = (0..65536u32).map(|i| ctx.permute(i)).collect();
        assert_eq!(out.len(), 65536);
        assert!(out.iter().all(|&v| v <= 0x7fff_ffff));
    }

    #[test]
    fn idgen32_rekey_flips_hibit() {
        let mut ctx = Idgen32Ctx::zeroed();
        ctx.init();
        let hibit = ctx.id32_hibit;
        ctx.id32_counter = IDGEN32_REKEY_LIMIT + 1;
        let id = ctx.generate();
        assert_eq!(id & 0x8000_0000, hibit ^ 0x8000_0000);
        assert_eq!(ctx.id32_counter, 1);
    }

    #[test]
    fn permute_stays_in_31_bits_and_is_injective_under_random_keys() {
        let mut st = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            st ^= st << 13;
            st ^= st >> 7;
            st ^= st << 17;
            st
        };
        for _ in 0..4 {
            let mut ctx = Idgen32Ctx::zeroed();
            ctx.id32_key.iter_mut().for_each(|b| *b = next() as u8);
            let ins: HashSet<u32> = (0..20000).map(|_| next() as u32 & 0x7fff_ffff).collect();
            let outs: HashSet<u32> = ins.iter().map(|i| ctx.permute(*i)).collect();
            assert_eq!(ins.len(), outs.len());
            assert!(outs.iter().all(|o| *o < 0x8000_0000));
        }
    }
}
/* </TESTS> */
