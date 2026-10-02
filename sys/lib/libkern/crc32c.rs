/*	$OpenBSD: crc32c.h,v 1.1 2025/11/01 15:46:40 kettenis Exp $	*/
/*	$NetBSD: crc16.h,v 1.3 2020/04/16 23:29:53 rin Exp $	*/

/*-
 * Copyright (c) 2006 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Steve C. Woodford.
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

//! CRC-32C (Castagnoli, polynomial `0x1edc6f41`), one byte at a time through a 256-entry table.
//!
//! Upstream: sys/lib/libkern/crc32c.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The lookup table is computed at compile time from the reflected polynomial (`0x82f63b78`)
//!   instead of being spelled out; `just test-ref` checks all 256 entries against the header.
//! - `crc32c_byte` is private: the header exposes it only as an inline helper of `crc32c`.

/// The CRC-32C polynomial `0x1edc6f41`, bit-reflected for the right-shifting table algorithm.
const CRC32C_POLY_REFLECTED: u32 = 0x82f6_3b78;

/// `crc32c_lookup`: CRC table for CRC-32C.
const CRC32C_LOOKUP: [u32; 256] = build_table();

const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < table.len() {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ CRC32C_POLY_REFLECTED
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// `crc32c_byte`: folds one byte into the running (inverted) CRC.
const fn crc32c_byte(ccrc: u32, b: u8) -> u32 {
    CRC32C_LOOKUP[((ccrc ^ b as u32) & 0xff) as usize] ^ (ccrc >> 8)
}

/// The CRC-32C of `data`, continuing from `crc` (pass `0` to start). The running value is
/// inverted on entry and on exit, so feeding consecutive chunks through successive calls yields
/// the CRC of their concatenation.
pub fn crc32c(crc: u32, data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff ^ crc;
    for &b in data {
        crc = crc32c_byte(crc, b);
    }
    crc ^ 0xffff_ffff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_spot_checks() {
        assert_eq!(CRC32C_LOOKUP[0], 0x0000_0000);
        assert_eq!(CRC32C_LOOKUP[1], 0xf26b_8303);
        assert_eq!(CRC32C_LOOKUP[2], 0xe13b_70f7);
        assert_eq!(CRC32C_LOOKUP[128], 0x82f6_3b78);
        assert_eq!(CRC32C_LOOKUP[255], 0xad7d_5351);
    }

    #[test]
    fn known_vectors() {
        // The standard CRC-32C check value.
        assert_eq!(crc32c(0, b"123456789"), 0xe306_9283);
        assert_eq!(crc32c(0, b""), 0);
        assert_eq!(crc32c(0, &[0u8; 32]), 0x8a91_36aa);
        assert_eq!(crc32c(0, &[0xffu8; 32]), 0x62a8_ab43);
    }

    #[test]
    fn chaining_chunks_equals_one_call() {
        let whole = b"The quick brown fox jumps over the lazy dog";
        let (a, b) = whole.split_at(17);
        assert_eq!(crc32c(crc32c(0, a), b), crc32c(0, whole));
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn table_matches_the_c_header() {
        let dir = std::path::PathBuf::from(
            std::env::var_os("OPENBSD_SRC").expect("OPENBSD_SRC is not set; run `just test-ref`"),
        );
        // A relative $OPENBSD_SRC is taken from the workspace root, three levels up.
        let dir = if dir.is_absolute() {
            dir
        } else {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../..")
                .join(dir)
        };
        let path = dir.join("sys/lib/libkern/crc32c.h");
        let text = std::fs::read_to_string(&path).expect("read crc32c.h");
        let start = text.find("crc32c_lookup[] = {").expect("table start");
        let end = text[start..].find("};").expect("table end") + start;
        let values: std::vec::Vec<u32> = text[start..end]
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter_map(|tok| tok.strip_prefix("0x"))
            .map(|hex| u32::from_str_radix(hex, 16).expect("hex entry"))
            .collect();
        assert_eq!(values.len(), 256);
        assert_eq!(values[..], CRC32C_LOOKUP[..]);
    }
}
