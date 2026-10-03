/*	$OpenBSD: sha1.h,v 1.6 2014/11/16 17:39:09 tedu Exp $	*/
/*	$OpenBSD: sha1.c,v 1.11 2014/12/28 10:04:35 tedu Exp $	*/
/* <LICENSES> */
/*
 * SHA-1 in C
 * By Steve Reid <steve@edmweb.com>
 * 100% Public Domain
 */

/*
 * SHA-1 in C
 * By Steve Reid <steve@edmweb.com>
 * 100% Public Domain
 *
 * Test Vectors (from FIPS PUB 180-1)
 * "abc"
 *   A9993E36 4706816A BA3E2571 7850C26C 9CD0D89D
 * "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
 *   84983E44 1C3BD26E BAAE4AA1 F95129E5 E54670F1
 * A million repetitions of "a"
 *   34AA973C D4C4DAA4 F61EEB2B DBAD2731 6534016F
*/
/* </LICENSES> */

//! SHA-1 (FIPS 180-4), Steve Reid's public domain implementation: the hash of IPsec's
//! `HMAC-SHA1-96` and of `hmac.c`.
//!
//! Upstream: sys/crypto/sha1.h @ 3ce1f3f79392, sys/crypto/sha1.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header and the file share this module; `SHA1_CTX` is [`Sha1Ctx`].
//! - `SHA1Update` takes a slice. The C's `len << 3` is 32-bit arithmetic (`unsigned int`) and
//!   wraps for a message of 512 MiB or more before it is added to the 64-bit count; here the
//!   shift is done in 64 bits.
//! - `SHA1Transform` expands the message schedule into an 80-word array and runs the rounds
//!   in a loop; the C expands in place in a 16-word window with the rounds unrolled by macro
//!   (`SHA1HANDSOFF` is always on: the block is copied). Both compute FIPS 180-4's function.
//! - `SHA1Final` wipes the context by assignment (`crate::crypto::wipe`).

use super::wipe;

/// `SHA1_BLOCK_LENGTH`.
pub const SHA1_BLOCK_LENGTH: usize = 64;
/// `SHA1_DIGEST_LENGTH`.
pub const SHA1_DIGEST_LENGTH: usize = 20;

/// `SHA1_CTX`: a hash in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sha1Ctx {
    /// `state`: the five chaining words.
    pub state: [u32; 5],
    /// `count`: the message length so far, in bits.
    pub count: u64,
    /// `buffer`: the partial block.
    pub buffer: [u8; SHA1_BLOCK_LENGTH],
}

impl Default for Sha1Ctx {
    fn default() -> Self {
        Self {
            state: [0; 5],
            count: 0,
            buffer: [0; SHA1_BLOCK_LENGTH],
        }
    }
}

/// `SHA1Transform`: hash a single 512-bit block. This is the core of the algorithm.
#[allow(non_snake_case)] // the C name
pub fn SHA1Transform(state: &mut [u32; 5], buffer: &[u8; SHA1_BLOCK_LENGTH]) {
    let mut w = [0u32; 80];

    for (i, word) in buffer.as_chunks::<4>().0.iter().enumerate() {
        w[i] = u32::from_be_bytes(*word);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }

    // Copy context->state[] to working vars
    let [mut a, mut b, mut c, mut d, mut e] = *state;

    // 4 rounds of 20 operations each.
    for (i, wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => (((b & (c ^ d)) ^ d), 0x5A827999u32),
            20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
            40..=59 => ((((b | c) & d) | (b & c)), 0x8F1BBCDC),
            _ => (b ^ c ^ d, 0xCA62C1D6),
        };
        let t = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(*wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = t;
    }

    // Add the working vars back into context.state[]
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
}

/// `SHA1Init`: initialize new context.
#[allow(non_snake_case)] // the C name
pub fn SHA1Init(context: &mut Sha1Ctx) {
    // SHA1 initialization constants
    context.count = 0;
    context.state[0] = 0x67452301;
    context.state[1] = 0xEFCDAB89;
    context.state[2] = 0x98BADCFE;
    context.state[3] = 0x10325476;
    context.state[4] = 0xC3D2E1F0;
}

/// `SHA1Update`: run your data through this.
#[allow(non_snake_case)] // the C name
pub fn SHA1Update(context: &mut Sha1Ctx, data: &[u8]) {
    let len = data.len();
    let mut j = ((context.count >> 3) & 63) as usize;
    let mut i;

    context.count = context.count.wrapping_add((len as u64) << 3);
    if (j + len) > 63 {
        i = 64 - j;
        context.buffer[j..].copy_from_slice(&data[..i]);
        SHA1Transform(&mut context.state, &context.buffer);
        while i + 63 < len {
            let mut block = [0u8; SHA1_BLOCK_LENGTH];
            block.copy_from_slice(&data[i..i + 64]);
            SHA1Transform(&mut context.state, &block);
            i += 64;
        }
        j = 0;
    } else {
        i = 0;
    }
    context.buffer[j..j + len - i].copy_from_slice(&data[i..]);
}

/// `SHA1Final`: add padding and return the message digest.
#[allow(non_snake_case)] // the C name
pub fn SHA1Final(digest: &mut [u8; SHA1_DIGEST_LENGTH], context: &mut Sha1Ctx) {
    // Endian independent
    let finalcount = context.count.to_be_bytes();

    SHA1Update(context, b"\x80");
    while (context.count & 504) != 448 {
        SHA1Update(context, b"\0");
    }
    SHA1Update(context, &finalcount); // Should cause a SHA1Transform()

    for (i, d) in digest.iter_mut().enumerate() {
        *d = (context.state[i >> 2] >> ((3 - (i & 3)) * 8)) as u8;
    }
    wipe(context);
}

#[cfg(test)]
mod tests;
