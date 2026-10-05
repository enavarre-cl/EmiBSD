/*	$OpenBSD: arc4.c,v 1.1 2019/10/29 02:51:17 deraadt Exp $	*/
/*	$OpenBSD: arc4.h,v 1.1 2019/10/29 02:51:17 deraadt Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2003 Markus Friedl <markus@openbsd.org>
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
 * Copyright (c) 2003 Markus Friedl <markus@openbsd.org>
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

//! RC4, which boot(8) keys with the random seed and `loadfile` uses to fill the kernel's
//! `PT_OPENBSD_RANDOMIZE` segment.
//!
//! Upstream: sys/lib/libsa/arc4.c @ 3ce1f3f79392, sys/lib/libsa/arc4.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `rc4_crypt` is `#ifdef notneeded` in the C and is not ported.
//! - `struct rc4_ctx randomctx` is defined in `stand/boot/boot.c`; here it is [`RANDOMCTX`],
//!   beside its type, because libsa's `loadfile` reads it and libsa cannot see the boot
//!   program's crate. boot(8) keys it.

use libkern::staticcell::StaticCell;

/// `RC4STATE`.
pub const RC4STATE: usize = 256;
/// `RC4KEYLEN`.
pub const RC4KEYLEN: usize = 16;

/// `struct rc4_ctx`.
#[derive(Clone, Copy)]
pub struct Rc4Ctx {
    /// `x`.
    pub x: u8,
    /// `y`.
    pub y: u8,
    /// `state`.
    pub state: [u8; RC4STATE],
}

impl Rc4Ctx {
    /// A context before `rc4_keysetup`.
    pub const fn new() -> Self {
        Self {
            x: 0,
            y: 0,
            state: [0; RC4STATE],
        }
    }

    /// The `RC4SWAP(x, y)` macro.
    fn swap(&mut self, x: usize, y: usize) {
        self.state.swap(x, y);
    }

    /// One step of the generator: the next state indices.
    fn step(&mut self) {
        self.x = self.x.wrapping_add(1);
        self.y = self.state[usize::from(self.x)].wrapping_add(self.y);
        self.swap(usize::from(self.x), usize::from(self.y));
    }
}

impl Default for Rc4Ctx {
    fn default() -> Self {
        Self::new()
    }
}

/// `randomctx`: boot(8)'s generator.
pub static RANDOMCTX: StaticCell<Rc4Ctx> = StaticCell::new(Rc4Ctx::new());

/// `rc4_keysetup(ctx, key, klen)`.
pub fn rc4_keysetup(ctx: &mut Rc4Ctx, key: &[u8]) {
    for (i, s) in ctx.state.iter_mut().enumerate() {
        *s = i as u8;
    }
    let mut x = 0usize;
    let mut y: u8 = 0;
    for i in 0..RC4STATE {
        y = key[x].wrapping_add(ctx.state[i]).wrapping_add(y);
        ctx.swap(i, usize::from(y));
        x = (x + 1) % key.len();
    }
    ctx.x = 0;
    ctx.y = 0;
}

/// `rc4_getbytes(ctx, dst, len)`: fill `dst` with the key stream.
pub fn rc4_getbytes(ctx: &mut Rc4Ctx, dst: &mut [u8]) {
    for d in dst {
        ctx.step();
        let k = ctx.state[usize::from(ctx.x)].wrapping_add(ctx.state[usize::from(ctx.y)]);
        *d = ctx.state[usize::from(k)];
    }
}

/// `rc4_skip(ctx, len)`: discard `len` bytes of the key stream.
pub fn rc4_skip(ctx: &mut Rc4Ctx, len: u32) {
    for _ in 0..len {
        ctx.step();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_stream_of_the_classic_vector() {
        // RC4("Key") encrypts "Plaintext" to BB F3 16 E8 D9 40 AF 0A D3.
        let mut ctx = Rc4Ctx::new();
        rc4_keysetup(&mut ctx, b"Key");
        let mut ks = [0u8; 9];
        rc4_getbytes(&mut ctx, &mut ks);
        let ct: [u8; 9] = core::array::from_fn(|i| ks[i] ^ b"Plaintext"[i]);
        assert_eq!(ct, [0xbb, 0xf3, 0x16, 0xe8, 0xd9, 0x40, 0xaf, 0x0a, 0xd3]);

        let mut a = Rc4Ctx::new();
        rc4_keysetup(&mut a, b"Key");
        rc4_skip(&mut a, 4);
        let mut rest = [0u8; 5];
        rc4_getbytes(&mut a, &mut rest);
        assert_eq!(rest, ks[4..]);
    }
}
