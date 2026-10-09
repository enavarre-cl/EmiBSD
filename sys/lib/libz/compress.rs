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

/* compress.c -- compress a memory buffer
 * Copyright (C) 1995-2026 Jean-loup Gailly, Mark Adler
 * For conditions of distribution and use, see copyright notice in zlib.h
 */

/* zlib.h -- interface of the 'zlib' general purpose compression library
  version 1.3.2, February 17th, 2026

  Copyright (C) 1995-2026 Jean-loup Gailly and Mark Adler

  This software is provided 'as-is', without any express or implied
  warranty.  In no event will the authors be held liable for any damages
  arising from the use of this software.

  Permission is granted to anyone to use this software for any purpose,
  including commercial applications, and to alter it and redistribute it
  freely, subject to the following restrictions:

  1. The origin of this software must not be misrepresented; you must not
     claim that you wrote the original software. If you use this software
     in a product, an acknowledgment in the product documentation would be
     appreciated but is not required.
  2. Altered source versions must be plainly marked as such, and must not be
     misrepresented as being the original software.
  3. This notice may not be removed or altered from any source distribution.

  Jean-loup Gailly        Mark Adler
  jloup@gzip.org          madler@alumni.caltech.edu


  The data format used by the zlib library is described by RFCs (Request for
  Comments) 1950 to 1952 at https://datatracker.ietf.org/doc/html/rfc1950
  (zlib format), rfc1951 (deflate format) and rfc1952 (gzip format).
*/
/* </LICENSES> */

/* <CODE> */
//! Compress a memory buffer in one call: `compress`, `compress2` and `compressBound`.
//!
//! Upstream: sys/lib/libz/compress.c @ 3ce1f3f79392
//!
//! **This is an altered source version, not the original zlib `compress.c`** (zlib licence,
//! clause 2): a Rust rewrite written for EmiBSD. The original's notice is kept above in full
//! (clause 3). Do not mistake it for the zlib distribution; bugs here are not the zlib
//! authors'.
//!
//! OpenBSD's kernel compiles `compress.c` (`sys/conf/files`), though no kernel code calls it.
//!
//! ## Deviations
//! - `compress2` is the C's `compress2_z` and `compress` its `compress_z`: the output buffer is
//!   a slice whose length is the space available (the C's `*destLen` on entry), and
//!   `destLen` only receives the compressed length. The `uLong` versions, which only differ
//!   from the `z_size_t` ones on platforms where the two have different widths, are the same
//!   functions on LP64. The `NULL` checks have no counterpart (slices are never null).
//! - `compressBound` and `compressBound_z` are both kept, for a `u64` and a `usize` length.

#![allow(non_snake_case)] // zlib's names (compressBound, destLen)

use crate::deflate::{deflate, deflateEnd};
use crate::zlib::{
    Z_DEFAULT_COMPRESSION, Z_FINISH, Z_NO_FLUSH, Z_OK, Z_STREAM_END, ZStream, deflateInit,
};

/// `compress2` (`compress2_z`): compresses `source` into `dest` at `level` (as in
/// `deflateInit`) and sets `destLen` to the size of the compressed data. `dest` should be at
/// least [`compressBound`] bytes long. Returns `Z_OK` if success, `Z_MEM_ERROR` if there was
/// not enough memory, `Z_BUF_ERROR` if there was not enough room in the output buffer,
/// `Z_STREAM_ERROR` if the level parameter is invalid.
pub fn compress2(dest: &mut [u8], destLen: &mut usize, source: &[u8], level: i32) -> i32 {
    let max = u32::MAX as usize; // (uInt)-1: what one call takes at most
    let dest_size = dest.len();
    let mut left = dest_size;
    let mut sourceLen = source.len();
    *destLen = 0;

    let mut stream = ZStream::new();
    let mut err = deflateInit(&mut stream, level);
    if err != Z_OK {
        return err;
    }

    // What has not been handed to next_out and next_in yet.
    let mut dest_rest: &mut [u8] = dest;
    let mut source_rest: &[u8] = source;

    loop {
        if stream.avail_out() == 0 {
            let n = left.min(max);
            let (chunk, rest) = core::mem::take(&mut dest_rest).split_at_mut(n);
            stream.next_out = chunk;
            dest_rest = rest;
            left -= n;
        }
        if stream.avail_in() == 0 {
            let n = sourceLen.min(max);
            let (chunk, rest) = source_rest.split_at(n);
            stream.next_in = chunk;
            source_rest = rest;
            sourceLen -= n;
        }
        err = deflate(
            &mut stream,
            if sourceLen != 0 { Z_NO_FLUSH } else { Z_FINISH },
        );
        if err != Z_OK {
            break;
        }
    }

    // stream.next_out - dest
    *destLen = dest_size - left - stream.avail_out();
    deflateEnd(&mut stream);
    if err == Z_STREAM_END { Z_OK } else { err }
}

/// `compress` (`compress_z`): [`compress2`] at `Z_DEFAULT_COMPRESSION`.
pub fn compress(dest: &mut [u8], destLen: &mut usize, source: &[u8]) -> i32 {
    compress2(dest, destLen, source, Z_DEFAULT_COMPRESSION)
}

/// `compressBound_z`: an upper bound on the compressed size after [`compress`] or
/// [`compress2`] on `sourceLen` bytes. If the default memLevel or windowBits for deflateInit()
/// is changed, then this function needs to be updated.
pub fn compressBound_z(sourceLen: usize) -> usize {
    let bound = sourceLen
        .wrapping_add(sourceLen >> 12)
        .wrapping_add(sourceLen >> 14)
        .wrapping_add(sourceLen >> 25)
        .wrapping_add(13);
    if bound < sourceLen { usize::MAX } else { bound }
}

/// `compressBound`: [`compressBound_z`] for a `uLong` length.
pub fn compressBound(sourceLen: u64) -> u64 {
    let Ok(len) = usize::try_from(sourceLen) else {
        return u64::MAX;
    };
    u64::try_from(compressBound_z(len)).unwrap_or(u64::MAX)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::zlib::{Z_BUF_ERROR, Z_STREAM_ERROR};

    const TEXT: &[u8] = include_bytes!("testdata/deflate_text.txt");
    /// zlib 1.2.12, level 9, of the test text (`compress2(.., 9)` makes the same stream).
    const ZLIB_TEXT_L9: &[u8] = include_bytes!("testdata/deflate_zlib_text_l9.bin");

    #[test]
    fn compress2_makes_a_zlib_stream() {
        let mut dest = std::vec![0u8; compressBound_z(TEXT.len())];
        let mut len = 0;
        assert_eq!(compress2(&mut dest, &mut len, TEXT, 9), Z_OK);
        assert_eq!(&dest[..len], ZLIB_TEXT_L9);
        // the default level, an empty input (python3: zlib.compress(b"") == 78 9c 03 00 00 00 00 01)
        assert_eq!(compress(&mut dest, &mut len, b""), Z_OK);
        assert_eq!(
            &dest[..len],
            &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]
        );
    }

    #[test]
    fn errors() {
        let mut dest = [0u8; 16];
        let mut len = 99;
        assert_eq!(compress2(&mut dest, &mut len, TEXT, 10), Z_STREAM_ERROR);
        assert_eq!(len, 0);
        assert_eq!(compress(&mut dest, &mut len, TEXT), Z_BUF_ERROR);
        assert_eq!(len, 16);
        assert_eq!(compress(&mut [], &mut len, TEXT), Z_BUF_ERROR);
        assert_eq!(len, 0);
    }

    #[test]
    fn bounds() {
        assert_eq!(compressBound_z(0), 13);
        assert_eq!(compressBound_z(100_000), 100_000 + 24 + 6 + 13);
        assert_eq!(compressBound_z(usize::MAX), usize::MAX);
        assert_eq!(compressBound(1 << 20), (1 << 20) + 256 + 64 + 13);
    }
}
/* </TESTS> */
