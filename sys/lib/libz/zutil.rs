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

/* zutil.h -- internal interface and configuration of the compression library
 * Copyright (C) 1995-2026 Jean-loup Gailly, Mark Adler
 * For conditions of distribution and use, see copyright notice in zlib.h
 */

/* zutil.c -- target dependent utility functions for the compression library
 * Copyright (C) 1995-2026 Jean-loup Gailly
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
//! zlib's internal interface: the error messages, the common constants of deflate and inflate,
//! the build knobs of the kernel's zlib, and the utility functions of `zutil.c`
//! (`zlibVersion`, `zlibCompileFlags`, `zError`).
//!
//! Upstream: sys/lib/libz/zutil.h @ 3ce1f3f79392, sys/lib/libz/zutil.c @ 3ce1f3f79392
//!
//! **This is an altered source version, not the original zlib `zutil.h`/`zutil.c`** (zlib
//! licence, clause 2): a Rust rewrite written for EmiBSD. The original's notice is kept above.
//!
//! OpenBSD compiles the kernel's zlib with `-DSLOW -DSMALL -DNO_GZIP`
//! (`sys/lib/libz/Makefile`); [`SLOW`], [`SMALL`] and [`NO_GZIP`] carry that choice into the
//! Rust code, which tests them where the C has `#ifdef`s, so both sides of each `#ifdef` are
//! still written down.
//!
//! ## Deviations
//! - `zmemcpy`, `zmemcmp` and `zmemzero` are `copy_from_slice`, slice comparison and `fill(0)`
//!   (`HAVE_MEMCPY` is defined in the kernel build, so the C uses `memcpy` and friends too).
//! - `ZSWAP32` is `u32::swap_bytes`.
//! - `zcalloc`/`zcfree`, `ZALLOC`, `ZFREE` and `TRY_FREE` are in `zopenbsd.rs` (the kernel's
//!   `MY_ZCALLOC`): fallible allocations of typed buffers instead of `void *` and an item size.
//! - `Assert` is [`zassert!`], checked under `cfg(test)` only (the kernel build has no
//!   `ZLIB_DEBUG`, so in the C it compiles to nothing); `Trace`, `Tracev`, `Tracevv`, `Tracec`,
//!   `Tracecv`, `z_verbose` and `z_error` print only under `ZLIB_DEBUG` and have no
//!   counterpart.
//! - `z_once` (with or without atomics) is only used by `DYNAMIC_CRC_TABLE` and `BUILDFIXED`,
//!   neither of which the kernel defines; it has no counterpart.
//! - The 16-bit and non-Unix target sections (MSDOS, OS/2, Windows, Amiga, VMS, TOPS-20, ...),
//!   `F_OPEN`, `ptrdiff_t`, `local`, the `uch`/`ush`/`ulg` typedefs and `Z_U8` have no
//!   counterpart; `OS_CODE` is the Unix value, 3.

#![allow(non_snake_case)] // zlib's names are camelCase or upper case in C (zError, ERR_MSG)

use crate::zconf::{MAX_MEM_LEVEL, MAX_WBITS};
use crate::zlib::{ZLIB_VERSION, ZStream};

/// `SLOW` (`-DSLOW` in `sys/lib/libz/Makefile`): `inflate()` never calls `inflate_fast()`.
pub(crate) const SLOW: bool = true;
/// `SMALL` (`-DSMALL`): inflate's error messages are all `"error"`.
pub(crate) const SMALL: bool = true;
/// `NO_GZIP` (`-DNO_GZIP`): no gzip wrapper; `GZIP` is not defined.
pub(crate) const NO_GZIP: bool = true;

/// `DEF_WBITS`: default windowBits for decompression. `MAX_WBITS` is for compression only.
pub const DEF_WBITS: i32 = MAX_WBITS;

/// `DEF_MEM_LEVEL`: default memLevel.
pub const DEF_MEM_LEVEL: i32 = if MAX_MEM_LEVEL >= 8 { 8 } else { MAX_MEM_LEVEL };

/// `STORED_BLOCK`: the three kinds of block type.
pub const STORED_BLOCK: i32 = 0;
/// `STATIC_TREES`.
pub const STATIC_TREES: i32 = 1;
/// `DYN_TREES`.
pub const DYN_TREES: i32 = 2;

/// `MIN_MATCH`: the minimum match length.
pub const MIN_MATCH: usize = 3;
/// `MAX_MATCH`: the maximum match length.
pub const MAX_MATCH: usize = 258;

/// `PRESET_DICT`: preset dictionary flag in zlib header.
pub const PRESET_DICT: u32 = 0x20;

/// `OS_CODE`: the operating system byte of a gzip header (3, Unix).
pub const OS_CODE: i32 = 3;

/// `z_errmsg`: the messages of the return codes, indexed by `2 - zlib_error`.
#[allow(non_upper_case_globals)] // the C name
pub static z_errmsg: [&str; 10] = [
    "need dictionary",      // Z_NEED_DICT       2
    "stream end",           // Z_STREAM_END      1
    "",                     // Z_OK              0
    "file error",           // Z_ERRNO         (-1)
    "stream error",         // Z_STREAM_ERROR  (-2)
    "data error",           // Z_DATA_ERROR    (-3)
    "insufficient memory",  // Z_MEM_ERROR     (-4)
    "buffer error",         // Z_BUF_ERROR     (-5)
    "incompatible version", // Z_VERSION_ERROR (-6)
    "",
];

/// `Assert(cond, msg)`: checked in host tests; nothing in the kernel (no `ZLIB_DEBUG`).
macro_rules! zassert {
    ($cond:expr, $msg:expr) => {
        #[cfg(test)]
        {
            assert!($cond, "{}", $msg);
        }
    };
}
pub(crate) use zassert;

/// `ERR_MSG(err)`: the message of the return code `err`; codes outside `Z_VERSION_ERROR ..=
/// Z_NEED_DICT` get the empty message.
pub fn ERR_MSG(err: i32) -> &'static str {
    if !(-6..=2).contains(&err) {
        z_errmsg[9]
    } else {
        z_errmsg[(2 - err) as usize]
    }
}

/// `ERR_RETURN(strm, err)`: sets `strm.msg` to the message of `err` and returns `err`. To be
/// used only when the state is known to be valid.
pub fn ERR_RETURN(strm: &mut ZStream<'_>, err: i32) -> i32 {
    strm.msg = Some(ERR_MSG(err));
    err
}

/// `zlibVersion`: the version of the library, `ZLIB_VERSION`.
pub fn zlibVersion() -> &'static str {
    ZLIB_VERSION
}

/// `zlibCompileFlags`: the compile-time options, as zlib.h documents the bits. Bits 0..7 are
/// the sizes of `uInt`, `uLong`, pointers and `z_off_t` (each 0: 16 bits, 1: 32, 2: 64, 3:
/// other); bit 17 is `NO_GZIP`. Nothing else applies to the kernel build (no `ZLIB_DEBUG`,
/// `BUILDFIXED`, `DYNAMIC_CRC_TABLE`, `FASTEST`; `vsnprintf` is there and returns a value).
pub fn zlibCompileFlags() -> u64 {
    const fn size_code(size: usize) -> u64 {
        match size {
            2 => 0,
            4 => 1,
            8 => 2,
            _ => 3,
        }
    }
    let mut flags = 0;
    flags += size_code(size_of::<u32>()); // uInt
    flags += size_code(size_of::<u64>()) << 2; // uLong
    flags += size_code(size_of::<usize>()) << 4; // voidpf
    flags += size_code(size_of::<i64>()) << 6; // z_off_t
    if NO_GZIP {
        flags += 1 << 17;
    }
    flags
}

/// `zError`: the message of the return code `err`, exported for `compress()` and
/// `uncompress()` callers.
pub fn zError(err: i32) -> &'static str {
    ERR_MSG(err)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::zlib::{Z_BUF_ERROR, Z_NEED_DICT, Z_OK, Z_STREAM_ERROR, Z_VERSION_ERROR};

    #[test]
    fn error_messages_are_indexed_by_two_minus_code() {
        assert_eq!(zError(Z_NEED_DICT), "need dictionary");
        assert_eq!(zError(Z_OK), "");
        assert_eq!(zError(Z_STREAM_ERROR), "stream error");
        assert_eq!(zError(Z_BUF_ERROR), "buffer error");
        assert_eq!(zError(Z_VERSION_ERROR), "incompatible version");
        assert_eq!(zError(3), "");
        assert_eq!(zError(-7), "");
    }

    #[test]
    fn err_return_sets_the_message() {
        let mut strm = ZStream::new();
        assert_eq!(ERR_RETURN(&mut strm, Z_BUF_ERROR), Z_BUF_ERROR);
        assert_eq!(strm.msg, Some("buffer error"));
    }

    #[test]
    fn version_and_flags() {
        assert_eq!(zlibVersion(), "1.3.2");
        // LP64: uInt 32 bits, uLong, pointers and z_off_t 64 bits; NO_GZIP.
        assert_eq!(zlibCompileFlags(), 1 | 2 << 2 | 2 << 4 | 2 << 6 | 1 << 17);
    }
}
/* </TESTS> */
