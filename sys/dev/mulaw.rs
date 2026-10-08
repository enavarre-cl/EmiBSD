/*	$OpenBSD: mulaw.h,v 1.16 2015/06/25 06:43:46 ratchov Exp $ */
/*	$NetBSD: mulaw.h,v 1.11 1999/11/01 18:12:19 augustss Exp $	*/
/*	$OpenBSD: mulaw.c,v 1.18 2015/06/25 06:43:46 ratchov Exp $ */
/*	$NetBSD: mulaw.c,v 1.15 2001/01/18 20:28:20 jdolecek Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1996 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by John T. Kohl.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/*
 * Copyright (c) 1991-1993 Regents of the University of California.
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
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by the Computer Systems
 *	Engineering Group at Lawrence Berkeley Laboratory.
 * 4. Neither the name of the University nor of the Laboratory may be used
 *    to endorse or promote products derived from this software without
 *    specific prior written permission.
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
 */
/* </LICENSES> */

/* <CODE> */
//! `dev/mulaw.c` and `<dev/mulaw.h>`: conversions between ITU G.711 mu-law and signed
//! linear samples, for audio(4)'s encoding emulation.
//!
//! Upstream: sys/dev/mulaw.c @ 3ce1f3f79392
//! Upstream: sys/dev/mulaw.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The two tables (`mulawtolin16`, `lintomulaw`) are computed at compile time from the
//!   G.711 definition (`docs/C_TO_RUST.md`, constant tables): [`mulaw_decode`] and
//!   [`linear_to_mulaw`]. `just test-ref` compares all 768 bytes with the C tables.
//! - The `(u_char *p, int cc)` pairs are byte slices. The 24-bit conversions work on the
//!   slice's 4-byte groups in native byte order, as the C's `int *` walk does, without
//!   requiring the slice to be `int`-aligned; a trailing partial group is left alone, as
//!   the C's `(cc -= 4) >= 0` loop leaves it.

/// G.711 mu-law to a 16-bit signed linear sample.
pub const fn mulaw_decode(mu: u8) -> i16 {
    // The code word is stored inverted: sign, 3-bit segment, 4-bit step.
    let u = !mu;
    let mut t: i32 = (((u & 0x0f) as i32) << 3) + 0x84;
    t <<= (u & 0x70) >> 4;
    if u & 0x80 != 0 {
        (0x84 - t) as i16
    } else {
        (t - 0x84) as i16
    }
}

/// A 16-bit signed linear sample to G.711 mu-law, rounding toward the segment's lower step
/// and clipping at the format's maximum.
pub const fn linear_to_mulaw(sample: i16) -> u8 {
    // Upper ends of the eight segments, in 14-bit magnitudes.
    const SEG_UEND: [i32; 8] = [0x3f, 0x7f, 0xff, 0x1ff, 0x3ff, 0x7ff, 0xfff, 0x1fff];
    const CLIP: i32 = 8159;
    const BIAS: i32 = 0x84 >> 2;

    let mut pcm = (sample as i32) >> 2;
    let mask: u8 = if pcm < 0 {
        pcm = -pcm;
        0x7f
    } else {
        0xff
    };
    if pcm > CLIP {
        pcm = CLIP;
    }
    pcm += BIAS;

    let mut seg = 0;
    while seg < SEG_UEND.len() && pcm > SEG_UEND[seg] {
        seg += 1;
    }
    if seg >= SEG_UEND.len() {
        return 0x7f ^ mask;
    }
    let uval = ((seg as u8) << 4) | (((pcm >> (seg + 1)) & 0x0f) as u8);
    uval ^ mask
}

/// Builds `mulawtolin16`: an (8 bit) mu-law value to a 16 bit value, unsigned (sign bit
/// flipped) and big-endian, as an array of two bytes for easier access to the individual
/// bytes.
const fn build_mulawtolin16() -> [[u8; 2]; 256] {
    let mut t = [[0u8; 2]; 256];
    let mut i = 0;
    while i < 256 {
        let v = (mulaw_decode(i as u8) as u16) ^ 0x8000;
        t[i] = v.to_be_bytes();
        i += 1;
    }
    t
}

/// Builds `lintomulaw`: an 8-bit unsigned linear value (the high byte of a 16-bit sample,
/// sign bit flipped) to mu-law.
const fn build_lintomulaw() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = linear_to_mulaw(((i as i32 - 128) * 256) as i16);
        i += 1;
    }
    t
}

/// `mulawtolin16`.
static MULAWTOLIN16: [[u8; 2]; 256] = build_mulawtolin16();

/// `lintomulaw`.
static LINTOMULAW: [u8; 256] = build_lintomulaw();

/// `mulaw_to_slinear8`: 8-bit mu-law to 8-bit signed linear, in place. The 16 bit table
/// serves for 8 bits too.
pub fn mulaw_to_slinear8(p: &mut [u8]) {
    for b in p {
        *b = MULAWTOLIN16[usize::from(*b)][0] ^ 0x80;
    }
}

/// `slinear8_to_mulaw`: 8-bit signed linear to 8-bit mu-law, in place.
pub fn slinear8_to_mulaw(p: &mut [u8]) {
    for b in p {
        *b = LINTOMULAW[usize::from(*b ^ 0x80)];
    }
}

/// `mulaw24_to_slinear24`: 24-bit mu-law (the code in bits 16..24 of each native `int`) to
/// 24-bit signed linear (each native `int` a sign-extended 24-bit sample whose low 8 bits
/// are zero), in place.
pub fn mulaw24_to_slinear24(p: &mut [u8]) {
    for q in p.as_chunks_mut::<4>().0 {
        let w = i32::from_ne_bytes(*q);
        let s = ((w >> 16) & 0xff) as usize;
        let hi = i32::from((MULAWTOLIN16[s][0] ^ 0x80) as i8);
        let lo = i32::from(MULAWTOLIN16[s][1]);
        *q = ((hi << 16) | (lo << 8)).to_ne_bytes();
    }
}

/// `slinear24_to_mulaw24`: 24-bit signed linear to 24-bit mu-law (the code in bits 16..24
/// of each native `int`), in place.
pub fn slinear24_to_mulaw24(p: &mut [u8]) {
    for q in p.as_chunks_mut::<4>().0 {
        let w = u32::from_ne_bytes(*q);
        let idx = (((w >> 16) & 0xff) ^ 0x80) as usize;
        *q = (u32::from(LINTOMULAW[idx]) << 16).to_ne_bytes();
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use super::*;

    #[test]
    fn table_ends_match_the_c() {
        assert_eq!(MULAWTOLIN16[0], [0x02, 0x84]);
        assert_eq!(MULAWTOLIN16[127], [0x80, 0x00]);
        assert_eq!(MULAWTOLIN16[128], [0xfd, 0x7c]);
        assert_eq!(MULAWTOLIN16[255], [0x80, 0x00]);
        assert_eq!(LINTOMULAW[..8], [0, 0, 0, 0, 0, 1, 1, 1]);
        assert_eq!(LINTOMULAW[127], 0x67);
        assert_eq!(LINTOMULAW[128], 0xff);
        assert_eq!(LINTOMULAW[255], 0x80);
    }

    #[test]
    fn mulaw8_round_trips() {
        // 8-bit linear keeps only the high byte of the 16-bit sample, so a mu-law code comes
        // back as a neighbour at most 288 (16-bit) away, a little more than one 8-bit step.
        let mut codes: Vec<u8> = (0..=255).collect();
        let orig = codes.clone();
        mulaw_to_slinear8(&mut codes);
        slinear8_to_mulaw(&mut codes);
        for (&o, &c) in orig.iter().zip(&codes) {
            let d = (i32::from(mulaw_decode(o)) - i32::from(mulaw_decode(c))).abs();
            assert!(d <= 288, "code {o:#x} -> {c:#x}");
        }
        // Linear 8-bit samples come back within 2.
        let mut lin: Vec<u8> = (0..=255).collect();
        slinear8_to_mulaw(&mut lin);
        mulaw_to_slinear8(&mut lin);
        for (i, &l) in lin.iter().enumerate() {
            let want = i as u8 as i8;
            let got = l as i8;
            assert!(
                (i32::from(want) - i32::from(got)).abs() <= 2,
                "{want} -> {got}"
            );
        }
    }

    #[test]
    fn mulaw24_matches_the_8_bit_tables() {
        let mut buf: Vec<u8> = Vec::new();
        for code in 0u8..=255 {
            buf.extend_from_slice(&(u32::from(code) << 16).to_ne_bytes());
        }
        buf.push(0xaa); // a partial group is left alone
        mulaw24_to_slinear24(&mut buf);
        for (code, q) in buf.chunks_exact(4).enumerate() {
            let w = i32::from_ne_bytes([q[0], q[1], q[2], q[3]]);
            let lin = i32::from(mulaw_decode(code as u8));
            assert_eq!(w, lin << 8, "code {code:#x}");
        }
        assert_eq!(buf[1024], 0xaa);
        slinear24_to_mulaw24(&mut buf);
        for (code, q) in buf.chunks_exact(4).enumerate() {
            let w = u32::from_ne_bytes([q[0], q[1], q[2], q[3]]);
            let back = (w >> 16) as u8;
            let mut one = [code as u8];
            mulaw_to_slinear8(&mut one);
            slinear8_to_mulaw(&mut one);
            assert_eq!(back, one[0], "code {code:#x}");
            assert_eq!(w & 0xff00_ffff, 0);
        }
    }

    /// The hex bytes of a C table, between `name` and the next `};`.
    fn c_table(src: &str, name: &str) -> Vec<u8> {
        let start = src.find(name).expect("table");
        let body = &src[start..];
        let body = &body[body.find('=').expect("initialiser")..body.find("};").expect("end")];
        body.split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| t.starts_with("0x"))
            .map(|t| u8::from_str_radix(&t[2..], 16).expect("hex"))
            .collect()
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn tables_match_the_c() {
        let path = crate::reftest::openbsd_src().join("sys/dev/mulaw.c");
        let src = std::fs::read_to_string(path).expect("mulaw.c");
        let lin16 = c_table(&src, "mulawtolin16[256][2]");
        assert_eq!(lin16.len(), 512);
        for (i, pair) in lin16.chunks_exact(2).enumerate() {
            assert_eq!(MULAWTOLIN16[i], [pair[0], pair[1]], "mulawtolin16[{i}]");
        }
        let tomu = c_table(&src, "lintomulaw[256]");
        assert_eq!(tomu.len(), 256);
        assert_eq!(&LINTOMULAW[..], &tomu[..]);
    }
}
/* </TESTS> */
