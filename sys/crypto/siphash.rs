/* $OpenBSD: siphash.h,v 1.6 2024/09/04 07:54:52 mglocker Exp $ */
/*	$OpenBSD: siphash.c,v 1.5 2018/01/05 19:05:09 mikeb Exp $ */
/* <LICENSES> */
/*-
 * Copyright (c) 2013 Andre Oppermann <andre@FreeBSD.org>
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
 * 3. The name of the author may not be used to endorse or promote
 *    products derived from this software without specific prior written
 *    permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
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
 * $FreeBSD$
 */

/*-
 * Copyright (c) 2013 Andre Oppermann <andre@FreeBSD.org>
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
 * 3. The name of the author may not be used to endorse or promote
 *    products derived from this software without specific prior written
 *    permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
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
 */
/* </LICENSES> */

//! SipHash, a family of pseudorandom functions (keyed hashes) optimised for speed on short
//! messages, returning a 64-bit value: `SipHash24_*` for the fast and reasonably strong
//! version, `SipHash48_*` for the strong one. The kernel uses it to spread keys over hash
//! tables an attacker cannot predict (`ufs_ihash`).
//!
//! Upstream: sys/crypto/siphash.h @ 3ce1f3f79392, sys/crypto/siphash.c @ 3ce1f3f79392
//!
//! Implemented, as the C is, from the paper "SipHash: a fast short-input PRF" by Jean-Philippe
//! Aumasson and Daniel J. Bernstein.
//!
//! ## Deviations
//! - The header and the file share this module (one name, as `docs/C_TO_RUST.md` does for
//!   `.h`/`.c` pairs).
//! - `SIPHASH_CTX`/`SIPHASH_KEY` are [`SiphashCtx`]/[`SiphashKey`]; the input is a byte slice
//!   (`src`, `len` in C) and `SipHash_Final` writes into an 8-byte array.
//! - `SipHash_End` wipes the state by assigning a zeroed one through the `&mut` (a store the
//!   caller can observe, so it is not elided) instead of `explicit_bzero` over its bytes.
//! - The `SipHash24_*`/`SipHash48_*` macros are functions with the same names.

/// `SIPHASH_BLOCK_LENGTH`.
pub const SIPHASH_BLOCK_LENGTH: usize = 8;
/// `SIPHASH_KEY_LENGTH`.
pub const SIPHASH_KEY_LENGTH: usize = 16;
/// `SIPHASH_DIGEST_LENGTH`.
pub const SIPHASH_DIGEST_LENGTH: usize = 8;

/// `SIPHASH_CTX`: the state of a hash in progress.
#[derive(Clone, Copy, Default)]
pub struct SiphashCtx {
    /// `v`: the four state words.
    pub v: [u64; 4],
    /// `buf`: the bytes of the block not yet compressed.
    pub buf: [u8; SIPHASH_BLOCK_LENGTH],
    /// `bytes`: the message length so far.
    pub bytes: u32,
}

/// `SIPHASH_KEY`: the 128-bit key, two words stored little-endian.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SiphashKey {
    /// `k0`.
    pub k0: u64,
    /// `k1`.
    pub k1: u64,
}

/// `SipHash_Init`: starts a hash under `key`.
#[allow(non_snake_case)] // the C name
pub fn SipHash_Init(ctx: &mut SiphashCtx, key: &SiphashKey) {
    // lemtoh64: the key words are stored little-endian.
    let k0 = u64::from_le(key.k0);
    let k1 = u64::from_le(key.k1);

    ctx.v[0] = 0x736f_6d65_7073_6575 ^ k0;
    ctx.v[1] = 0x646f_7261_6e64_6f6d ^ k1;
    ctx.v[2] = 0x6c79_6765_6e65_7261 ^ k0;
    ctx.v[3] = 0x7465_6462_7974_6573 ^ k1;

    ctx.buf = [0; SIPHASH_BLOCK_LENGTH];
    ctx.bytes = 0;
}

/// `SipHash_Update`: adds `src` to the message, compressing each full block with `rc`
/// rounds.
#[allow(non_snake_case)] // the C name
pub fn SipHash_Update(ctx: &mut SiphashCtx, rc: i32, _rf: i32, src: &[u8]) {
    let mut ptr = src;
    if ptr.is_empty() {
        return;
    }

    let used = ctx.bytes as usize % SIPHASH_BLOCK_LENGTH;
    ctx.bytes = ctx.bytes.wrapping_add(ptr.len() as u32);

    if used > 0 {
        let left = SIPHASH_BLOCK_LENGTH - used;

        if ptr.len() >= left {
            ctx.buf[used..].copy_from_slice(&ptr[..left]);
            SipHash_CRounds(ctx, rc);
            ptr = &ptr[left..];
        } else {
            ctx.buf[used..used + ptr.len()].copy_from_slice(ptr);
            return;
        }
    }

    while ptr.len() >= SIPHASH_BLOCK_LENGTH {
        ctx.buf.copy_from_slice(&ptr[..SIPHASH_BLOCK_LENGTH]);
        SipHash_CRounds(ctx, rc);
        ptr = &ptr[SIPHASH_BLOCK_LENGTH..];
    }

    if !ptr.is_empty() {
        ctx.buf[..ptr.len()].copy_from_slice(ptr);
    }
}

/// `SipHash_Final`: the digest, as little-endian bytes.
#[allow(non_snake_case)] // the C name
pub fn SipHash_Final(
    dst: &mut [u8; SIPHASH_DIGEST_LENGTH],
    ctx: &mut SiphashCtx,
    rc: i32,
    rf: i32,
) {
    *dst = SipHash_End(ctx, rc, rf).to_le_bytes();
}

/// `SipHash_End`: pads the last block with the length, finalises with `rf` rounds and
/// returns the hash; the state is wiped.
#[allow(non_snake_case)] // the C name
pub fn SipHash_End(ctx: &mut SiphashCtx, rc: i32, rf: i32) -> u64 {
    let used = ctx.bytes as usize % SIPHASH_BLOCK_LENGTH;
    let left = SIPHASH_BLOCK_LENGTH - used;
    ctx.buf[used..used + left - 1].fill(0);
    ctx.buf[7] = ctx.bytes as u8;

    SipHash_CRounds(ctx, rc);
    ctx.v[2] ^= 0xff;
    SipHash_Rounds(ctx, rf);

    let r = (ctx.v[0] ^ ctx.v[1]) ^ (ctx.v[2] ^ ctx.v[3]);
    *ctx = SiphashCtx::default();
    r
}

/// `SipHash`: the hash of `src` under `key`, in one call.
#[allow(non_snake_case)] // the C name
pub fn SipHash(key: &SiphashKey, rc: i32, rf: i32, src: &[u8]) -> u64 {
    let mut ctx = SiphashCtx::default();

    SipHash_Init(&mut ctx, key);
    SipHash_Update(&mut ctx, rc, rf, src);
    SipHash_End(&mut ctx, rc, rf)
}

/// `SipHash_Rounds`: `rounds` SipRounds over the state.
#[allow(non_snake_case)] // the C name
fn SipHash_Rounds(ctx: &mut SiphashCtx, rounds: i32) {
    let v = &mut ctx.v;
    for _ in 0..rounds {
        v[0] = v[0].wrapping_add(v[1]);
        v[2] = v[2].wrapping_add(v[3]);
        v[1] = v[1].rotate_left(13);
        v[3] = v[3].rotate_left(16);

        v[1] ^= v[0];
        v[3] ^= v[2];
        v[0] = v[0].rotate_left(32);

        v[2] = v[2].wrapping_add(v[1]);
        v[0] = v[0].wrapping_add(v[3]);
        v[1] = v[1].rotate_left(17);
        v[3] = v[3].rotate_left(21);

        v[1] ^= v[2];
        v[3] ^= v[0];
        v[2] = v[2].rotate_left(32);
    }
}

/// `SipHash_CRounds`: compresses the buffered block.
#[allow(non_snake_case)] // the C name
fn SipHash_CRounds(ctx: &mut SiphashCtx, rounds: i32) {
    let m = u64::from_le_bytes(ctx.buf);

    ctx.v[3] ^= m;
    SipHash_Rounds(ctx, rounds);
    ctx.v[0] ^= m;
}

/// `SipHash24_Init`.
#[allow(non_snake_case)] // the C macro's name
pub fn SipHash24_Init(ctx: &mut SiphashCtx, key: &SiphashKey) {
    SipHash_Init(ctx, key);
}

/// `SipHash24_Update`.
#[allow(non_snake_case)] // the C macro's name
pub fn SipHash24_Update(ctx: &mut SiphashCtx, src: &[u8]) {
    SipHash_Update(ctx, 2, 4, src);
}

/// `SipHash24_End`.
#[allow(non_snake_case)] // the C macro's name
pub fn SipHash24_End(ctx: &mut SiphashCtx) -> u64 {
    SipHash_End(ctx, 2, 4)
}

/// `SipHash24_Final`.
#[allow(non_snake_case)] // the C macro's name
pub fn SipHash24_Final(dst: &mut [u8; SIPHASH_DIGEST_LENGTH], ctx: &mut SiphashCtx) {
    SipHash_Final(dst, ctx, 2, 4);
}

/// `SipHash24`.
#[allow(non_snake_case)] // the C macro's name
pub fn SipHash24(key: &SiphashKey, src: &[u8]) -> u64 {
    SipHash(key, 2, 4, src)
}

/// `SipHash48_Init`.
#[allow(non_snake_case)] // the C macro's name
pub fn SipHash48_Init(ctx: &mut SiphashCtx, key: &SiphashKey) {
    SipHash_Init(ctx, key);
}

/// `SipHash48_Update`.
#[allow(non_snake_case)] // the C macro's name
pub fn SipHash48_Update(ctx: &mut SiphashCtx, src: &[u8]) {
    SipHash_Update(ctx, 4, 8, src);
}

/// `SipHash48_End`.
#[allow(non_snake_case)] // the C macro's name
pub fn SipHash48_End(ctx: &mut SiphashCtx) -> u64 {
    SipHash_End(ctx, 4, 8)
}

/// `SipHash48_Final`.
#[allow(non_snake_case)] // the C macro's name
pub fn SipHash48_Final(dst: &mut [u8; SIPHASH_DIGEST_LENGTH], ctx: &mut SiphashCtx) {
    SipHash_Final(dst, ctx, 4, 8);
}

/// `SipHash48`.
#[allow(non_snake_case)] // the C macro's name
pub fn SipHash48(key: &SiphashKey, src: &[u8]) -> u64 {
    SipHash(key, 4, 8, src)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key of the paper's test vectors: the bytes 0, 1, ..., 15 in memory.
    fn paper_key() -> SiphashKey {
        let mut k0 = [0u8; 8];
        let mut k1 = [0u8; 8];
        for i in 0..8 {
            k0[i] = i as u8;
            k1[i] = (i + 8) as u8;
        }
        SiphashKey {
            k0: u64::from_ne_bytes(k0),
            k1: u64::from_ne_bytes(k1),
        }
    }

    #[test]
    fn matches_the_reference_vectors() {
        let key = paper_key();
        let msg: [u8; 15] = core::array::from_fn(|i| i as u8);
        // Appendix A of the paper: SipHash-2-4 of the 15-byte message 00..0e.
        assert_eq!(SipHash24(&key, &msg), 0xa129_ca61_49be_45e5);
        // The reference implementation's vectors for the empty and the 8-byte message.
        assert_eq!(SipHash24(&key, &[]), 0x726f_db47_dd0e_0e31);
        assert_eq!(SipHash24(&key, &msg[..8]), 0x93f5_f579_9a93_2462);
    }

    #[test]
    fn incremental_updates_agree_with_one_call() {
        let key = paper_key();
        let msg: [u8; 37] = core::array::from_fn(|i| (i * 7) as u8);
        let whole = SipHash24(&key, &msg);
        for split in [0, 1, 3, 8, 9, 20, 37] {
            let mut ctx = SiphashCtx::default();
            SipHash24_Init(&mut ctx, &key);
            SipHash24_Update(&mut ctx, &msg[..split]);
            SipHash24_Update(&mut ctx, &msg[split..]);
            assert_eq!(SipHash24_End(&mut ctx), whole, "split at {split}");
        }
        let mut d = [0u8; 8];
        let mut ctx = SiphashCtx::default();
        SipHash48_Init(&mut ctx, &key);
        SipHash48_Update(&mut ctx, &msg);
        SipHash48_Final(&mut d, &mut ctx);
        assert_eq!(u64::from_le_bytes(d), SipHash48(&key, &msg));
    }
}
