/*	$OpenBSD: md5.h,v 1.3 2014/11/16 17:39:09 tedu Exp $	*/
/*	$OpenBSD: md5.c,v 1.4 2014/12/28 10:04:35 tedu Exp $	*/
/* <LICENSES> */
/*
 * This code implements the MD5 message-digest algorithm.
 * The algorithm is due to Ron Rivest.  This code was
 * written by Colin Plumb in 1993, no copyright is claimed.
 * This code is in the public domain; do with it what you wish.
 *
 * Equivalent code is available from RSA Data Security, Inc.
 * This code has been tested against that, and is equivalent,
 * except that you don't need to include two pages of legalese
 * with every copy.
 */

/*
 * This code implements the MD5 message-digest algorithm.
 * The algorithm is due to Ron Rivest.	This code was
 * written by Colin Plumb in 1993, no copyright is claimed.
 * This code is in the public domain; do with it what you wish.
 *
 * Equivalent code is available from RSA Data Security, Inc.
 * This code has been tested against that, and is equivalent,
 * except that you don't need to include two pages of legalese
 * with every copy.
 *
 * To compute the message digest of a chunk of bytes, declare an
 * MD5Context structure, pass it to MD5Init, call MD5Update as
 * needed on buffers full of bytes, and then call MD5Final, which
 * will fill a supplied 16-byte array with the digest.
 */
/* </LICENSES> */

//! MD5 (RFC 1321), Colin Plumb's public domain implementation: the hash of IPsec's
//! `HMAC-MD5-96` and of the TCP MD5 signature option.
//!
//! Upstream: sys/crypto/md5.h @ 3ce1f3f79392, sys/crypto/md5.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header and the file share this module; `MD5_CTX` (`struct MD5Context`) is [`Md5Ctx`].
//! - `MD5Update` takes a slice; `MD5Final` wipes the context by assignment
//!   (`crate::crypto::wipe`).
//! - `MD5Transform` keeps the C's step order but states the four round functions and the
//!   per-round constants and shifts as tables read by a loop, where the C writes sixty-four
//!   `MD5STEP` lines. The words are always read little-endian (the C copies on little-endian
//!   machines and assembles bytes otherwise).

use super::wipe;

/// `MD5_BLOCK_LENGTH`.
pub const MD5_BLOCK_LENGTH: usize = 64;
/// `MD5_DIGEST_LENGTH`.
pub const MD5_DIGEST_LENGTH: usize = 16;

/// `MD5_CTX`: a hash in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Md5Ctx {
    /// `state`: the four chaining words.
    pub state: [u32; 4],
    /// `count`: number of bits, mod 2^64.
    pub count: u64,
    /// `buffer`: input buffer.
    pub buffer: [u8; MD5_BLOCK_LENGTH],
}

impl Default for Md5Ctx {
    fn default() -> Self {
        Self {
            state: [0; 4],
            count: 0,
            buffer: [0; MD5_BLOCK_LENGTH],
        }
    }
}

/// `PADDING`: a 1 bit and zeros.
static PADDING: [u8; MD5_BLOCK_LENGTH] = {
    let mut p = [0u8; MD5_BLOCK_LENGTH];
    p[0] = 0x80;
    p
};

/// The sine-derived addend of each of the sixty-four steps.
const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

/// The left rotation of each of the sixty-four steps.
const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// `MD5Init`: start MD5 accumulation. Set bit count to 0 and buffer to mysterious
/// initialization constants.
#[allow(non_snake_case)] // the C name
pub fn MD5Init(ctx: &mut Md5Ctx) {
    ctx.count = 0;
    ctx.state[0] = 0x67452301;
    ctx.state[1] = 0xefcdab89;
    ctx.state[2] = 0x98badcfe;
    ctx.state[3] = 0x10325476;
}

/// `MD5Update`: update context to reflect the concatenation of another buffer full of bytes.
#[allow(non_snake_case)] // the C name
pub fn MD5Update(ctx: &mut Md5Ctx, input: &[u8]) {
    let mut input = input;

    // Check how many bytes we already have and how many more we need.
    let mut have = ((ctx.count >> 3) & (MD5_BLOCK_LENGTH as u64 - 1)) as usize;
    let need = MD5_BLOCK_LENGTH - have;

    // Update bitcount
    ctx.count = ctx.count.wrapping_add((input.len() as u64) << 3);

    if input.len() >= need {
        if have != 0 {
            ctx.buffer[have..].copy_from_slice(&input[..need]);
            MD5Transform(&mut ctx.state, &ctx.buffer);
            input = &input[need..];
            have = 0;
        }

        // Process data in MD5_BLOCK_LENGTH-byte chunks.
        while input.len() >= MD5_BLOCK_LENGTH {
            let mut block = [0u8; MD5_BLOCK_LENGTH];
            block.copy_from_slice(&input[..MD5_BLOCK_LENGTH]);
            MD5Transform(&mut ctx.state, &block);
            input = &input[MD5_BLOCK_LENGTH..];
        }
    }

    // Handle any remaining bytes of data.
    if !input.is_empty() {
        ctx.buffer[have..have + input.len()].copy_from_slice(input);
    }
}

/// `MD5Final`: final wrapup - pad to 64-byte boundary with the bit pattern 1 0* (64-bit count
/// of bits processed, MSB-first); the context is wiped.
#[allow(non_snake_case)] // the C name
pub fn MD5Final(digest: &mut [u8; MD5_DIGEST_LENGTH], ctx: &mut Md5Ctx) {
    // Convert count to 8 bytes in little endian order.
    let count = ctx.count.to_le_bytes();

    // Pad out to 56 mod 64.
    let mut padlen = MD5_BLOCK_LENGTH - ((ctx.count >> 3) & (MD5_BLOCK_LENGTH as u64 - 1)) as usize;
    if padlen < 1 + 8 {
        padlen += MD5_BLOCK_LENGTH;
    }
    MD5Update(ctx, &PADDING[..padlen - 8]); // padlen - 8 <= 64
    MD5Update(ctx, &count);

    for i in 0..4 {
        digest[i * 4..i * 4 + 4].copy_from_slice(&ctx.state[i].to_le_bytes());
    }
    wipe(ctx); // in case it's sensitive
}

/// The round function of step `i` (the C's `F1` to `F4`): `F1` is optimized somewhat.
fn md5_f(i: usize, x: u32, y: u32, z: u32) -> u32 {
    match i / 16 {
        0 => z ^ (x & (y ^ z)),
        1 => y ^ (z & (x ^ y)),
        2 => x ^ y ^ z,
        _ => y ^ (x | !z),
    }
}

/// The index of the message word step `i` reads.
fn md5_g(i: usize) -> usize {
    match i / 16 {
        0 => i,
        1 => (5 * i + 1) % 16,
        2 => (3 * i + 5) % 16,
        _ => (7 * i) % 16,
    }
}

/// `MD5Transform`: the core of the MD5 algorithm, this alters an existing MD5 hash to reflect
/// the addition of 16 longwords of new data. `MD5Update` blocks the data and converts bytes
/// into longwords for this routine.
#[allow(non_snake_case)] // the C name
pub fn MD5Transform(state: &mut [u32; 4], block: &[u8; MD5_BLOCK_LENGTH]) {
    let mut input = [0u32; MD5_BLOCK_LENGTH / 4];

    for (i, w) in block.as_chunks::<4>().0.iter().enumerate() {
        input[i] = u32::from_le_bytes(*w);
    }

    let [mut a, mut b, mut c, mut d] = *state;

    // The C rotates the roles of a, b, c and d between steps (MD5STEP(f, a, b, c, d, ..), then
    // (f, d, a, b, c, ..)); here the values move instead.
    for i in 0..64 {
        let f = md5_f(i, b, c, d)
            .wrapping_add(a)
            .wrapping_add(input[md5_g(i)])
            .wrapping_add(MD5_K[i]);
        a = d;
        d = c;
        c = b;
        b = b.wrapping_add(f.rotate_left(MD5_S[i]));
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
}

#[cfg(test)]
mod tests;
