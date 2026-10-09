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
//! The interface of zlib: the stream (`z_stream`), the gzip header (`gz_header`) and the
//! constants every caller passes or tests (`Z_OK`, `Z_FINISH`, `Z_DEFAULT_COMPRESSION`, ...).
//!
//! Upstream: sys/lib/libz/zlib.h @ 3ce1f3f79392
//! LZ: sys/lib/libz/zlib.rs@f5985f1d055a
//!
//! **This is an altered source version, not the original zlib `zlib.h`** (zlib licence, clause
//! 2): a Rust rewrite written for EmiBSD. The original's notice is kept above in full (clause 3).
//!
//! The functions zlib.h declares live with their `.c` files (`deflate.rs`, `inflate.rs`,
//! `adler32.rs`, `compress.rs`, `zutil.rs`) and are re-exported at the crate root; the
//! function-like macros of the header (`deflateInit`, `inflateInit2`, ...) are here.
//!
//! ## Deviations
//! - `next_in`/`avail_in` are one field, `next_in: &[u8]` (`avail_in` is its length), and
//!   `next_out`/`avail_out` are `next_out: &mut [u8]`; zlib advances them exactly as the C
//!   advances the pointer and decrements the count. [`ZStream::avail_in`] and
//!   [`ZStream::avail_out`] give the counts. The stream borrows the caller's buffers for `'a`.
//! - `zalloc`, `zfree` and `opaque` are gone: the kernel build always uses `zopenbsd.c`'s
//!   `zcalloc`/`zcfree`, and so does this crate (`zopenbsd.rs`: fallible allocations through
//!   the global allocator). `reserved` is gone too.
//! - `state` is a crate-private enum ([`InternalState`]) instead of an opaque pointer: a deflate
//!   stream and an inflate stream cannot be confused, which is what `deflateStateCheck` and
//!   `inflateStateCheck` check in the C.
//! - `msg` is `Option<&'static str>` (`NULL` is `None`); `total_in`/`total_out` are `u64`
//!   (`uLong` on LP64); `adler` is `u32` (an Adler-32 or CRC-32 value never exceeds 32 bits).
//! - `gz_header` keeps its fields, with owned buffers for `extra`, `name` and `comment`. The
//!   kernel builds zlib with `NO_GZIP`, so `deflateSetHeader` and `inflateGetHeader` always
//!   refuse it (`Z_STREAM_ERROR`), exactly as the C build does.
//! - The gz* file functions, `get_crc_table`, the `z_*` prefixed aliases (`Z_PREFIX_SET`) and
//!   the `*64` large-file variants are not part of the kernel's zlib (`gzguts.h` is excluded
//!   under `_KERNEL`, the rest are aliases) and have no counterpart.
//!
//! ## Redesign
//! - The return codes are a `Result`: [`ZStatus`] carries the codes that are not errors
//!   (`Z_OK`, `Z_STREAM_END`, `Z_NEED_DICT`), [`ZError`] the negative ones. A function whose
//!   only success is `Z_OK` returns `Result<(), ZError>`; one that can also report the end of
//!   the stream or a needed dictionary returns `Result<ZStatus, ZError>`. The C values are
//!   still the `Z_*` constants, and [`ZCode::code`] gives the C value of any result, for
//!   callers that keep a status integer of their own (libsa's `cread`). LZ returned the `i32`
//!   codes.
//! - The flush parameter is a [`Flush`] and the compression strategy a [`Strategy`] (the `Z_*`
//!   constants stay, as the C values), so an out-of-range flush or strategy cannot be passed.
//!   The `deflateInit*` and `inflateInit*` macros return the `Result` of the functions they
//!   call.

#![allow(non_snake_case)] // zlib's API names are camelCase in C (deflateInit2_, inflateReset2)

use alloc::vec::Vec;

/// `ZLIB_VERSION`.
pub const ZLIB_VERSION: &str = "1.3.2";
/// `ZLIB_VERNUM`.
pub const ZLIB_VERNUM: u32 = 0x1320;
/// `ZLIB_VER_MAJOR`.
pub const ZLIB_VER_MAJOR: u32 = 1;
/// `ZLIB_VER_MINOR`.
pub const ZLIB_VER_MINOR: u32 = 3;
/// `ZLIB_VER_REVISION`.
pub const ZLIB_VER_REVISION: u32 = 2;
/// `ZLIB_VER_SUBREVISION`.
pub const ZLIB_VER_SUBREVISION: u32 = 0;

/// `Z_NO_FLUSH`: allowed flush values, see `deflate()` and `inflate()`.
pub const Z_NO_FLUSH: i32 = 0;
/// `Z_PARTIAL_FLUSH`.
pub const Z_PARTIAL_FLUSH: i32 = 1;
/// `Z_SYNC_FLUSH`.
pub const Z_SYNC_FLUSH: i32 = 2;
/// `Z_FULL_FLUSH`.
pub const Z_FULL_FLUSH: i32 = 3;
/// `Z_FINISH`.
pub const Z_FINISH: i32 = 4;
/// `Z_BLOCK`.
pub const Z_BLOCK: i32 = 5;
/// `Z_TREES`.
pub const Z_TREES: i32 = 6;

/// `Z_OK`: return codes for the compression/decompression functions. Negative values are
/// errors, positive values are used for special but normal events.
pub const Z_OK: i32 = 0;
/// `Z_STREAM_END`.
pub const Z_STREAM_END: i32 = 1;
/// `Z_NEED_DICT`.
pub const Z_NEED_DICT: i32 = 2;
/// `Z_ERRNO`.
pub const Z_ERRNO: i32 = -1;
/// `Z_STREAM_ERROR`.
pub const Z_STREAM_ERROR: i32 = -2;
/// `Z_DATA_ERROR`.
pub const Z_DATA_ERROR: i32 = -3;
/// `Z_MEM_ERROR`.
pub const Z_MEM_ERROR: i32 = -4;
/// `Z_BUF_ERROR`.
pub const Z_BUF_ERROR: i32 = -5;
/// `Z_VERSION_ERROR`.
pub const Z_VERSION_ERROR: i32 = -6;

/// `Z_NO_COMPRESSION`: compression levels.
pub const Z_NO_COMPRESSION: i32 = 0;
/// `Z_BEST_SPEED`.
pub const Z_BEST_SPEED: i32 = 1;
/// `Z_BEST_COMPRESSION`.
pub const Z_BEST_COMPRESSION: i32 = 9;
/// `Z_DEFAULT_COMPRESSION`.
pub const Z_DEFAULT_COMPRESSION: i32 = -1;

/// `Z_FILTERED`: compression strategies, see `deflateInit2()`.
pub const Z_FILTERED: i32 = 1;
/// `Z_HUFFMAN_ONLY`.
pub const Z_HUFFMAN_ONLY: i32 = 2;
/// `Z_RLE`.
pub const Z_RLE: i32 = 3;
/// `Z_FIXED`.
pub const Z_FIXED: i32 = 4;
/// `Z_DEFAULT_STRATEGY`.
pub const Z_DEFAULT_STRATEGY: i32 = 0;

/// `Z_BINARY`: possible values of the `data_type` field for `deflate()`.
pub const Z_BINARY: i32 = 0;
/// `Z_TEXT`.
pub const Z_TEXT: i32 = 1;
/// `Z_ASCII`: for compatibility with 1.2.2 and earlier.
pub const Z_ASCII: i32 = Z_TEXT;
/// `Z_UNKNOWN`.
pub const Z_UNKNOWN: i32 = 2;

/// `Z_DEFLATED`: the deflate compression method (the only one supported in this version).
pub const Z_DEFLATED: i32 = 8;

/// The return codes of zlib that are not errors: what the `Ok` of a stream function carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZStatus {
    /// `Z_OK`: the call made progress, or did what it was asked.
    Ok,
    /// `Z_STREAM_END`: the end of the stream was reached (all output produced or consumed).
    StreamEnd,
    /// `Z_NEED_DICT`: `inflate()` needs a preset dictionary to go on.
    NeedDict,
}

impl ZStatus {
    /// The C value of the status (`Z_OK`, `Z_STREAM_END`, `Z_NEED_DICT`).
    pub const fn code(self) -> i32 {
        match self {
            Self::Ok => Z_OK,
            Self::StreamEnd => Z_STREAM_END,
            Self::NeedDict => Z_NEED_DICT,
        }
    }
}

/// The error codes of zlib (negative in C): what the `Err` of a stream function carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZError {
    /// `Z_ERRNO`: a file error. Only the gz* file functions return it; the kernel's zlib never
    /// does, but its message exists (`zError`) and libsa's `cread` reports it.
    Errno,
    /// `Z_STREAM_ERROR`: the stream state is inconsistent, or a parameter is invalid.
    Stream,
    /// `Z_DATA_ERROR`: the input data was corrupted (or a dictionary is wrong).
    Data,
    /// `Z_MEM_ERROR`: not enough memory.
    Mem,
    /// `Z_BUF_ERROR`: no progress was possible, or the output space was too short.
    Buf,
    /// `Z_VERSION_ERROR`: the library version is incompatible with the caller's.
    Version,
}

impl ZError {
    /// The C value of the error (`Z_ERRNO` .. `Z_VERSION_ERROR`).
    pub const fn code(self) -> i32 {
        match self {
            Self::Errno => Z_ERRNO,
            Self::Stream => Z_STREAM_ERROR,
            Self::Data => Z_DATA_ERROR,
            Self::Mem => Z_MEM_ERROR,
            Self::Buf => Z_BUF_ERROR,
            Self::Version => Z_VERSION_ERROR,
        }
    }
}

/// The flush parameter of `deflate()` and `inflate()` (the C's `Z_NO_FLUSH` .. `Z_TREES`).
/// The order is the C values' order, which the compressor compares.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Flush {
    /// `Z_NO_FLUSH`: no flush, the library decides how much to buffer.
    NoFlush,
    /// `Z_PARTIAL_FLUSH`: flush to a byte boundary with an empty static block (deflate).
    PartialFlush,
    /// `Z_SYNC_FLUSH`: flush all output to a byte boundary with an empty stored block.
    SyncFlush,
    /// `Z_FULL_FLUSH`: a sync flush that also forgets the history (deflate).
    FullFlush,
    /// `Z_FINISH`: process all input and finish the stream.
    Finish,
    /// `Z_BLOCK`: stop at the end of the current block.
    Block,
    /// `Z_TREES`: like `Z_BLOCK`, and stop after a block header too (inflate only).
    Trees,
}

impl Flush {
    /// The C value of the flush mode (`Z_NO_FLUSH` .. `Z_TREES`).
    pub const fn code(self) -> i32 {
        match self {
            Self::NoFlush => Z_NO_FLUSH,
            Self::PartialFlush => Z_PARTIAL_FLUSH,
            Self::SyncFlush => Z_SYNC_FLUSH,
            Self::FullFlush => Z_FULL_FLUSH,
            Self::Finish => Z_FINISH,
            Self::Block => Z_BLOCK,
            Self::Trees => Z_TREES,
        }
    }
}

impl TryFrom<i32> for Flush {
    type Error = ZError;

    /// The flush mode of a C value; `ZError::Stream` for a value outside `Z_NO_FLUSH ..=
    /// Z_TREES`, as `deflate()` and `inflate()` treat one.
    fn try_from(code: i32) -> Result<Self, ZError> {
        Ok(match code {
            Z_NO_FLUSH => Self::NoFlush,
            Z_PARTIAL_FLUSH => Self::PartialFlush,
            Z_SYNC_FLUSH => Self::SyncFlush,
            Z_FULL_FLUSH => Self::FullFlush,
            Z_FINISH => Self::Finish,
            Z_BLOCK => Self::Block,
            Z_TREES => Self::Trees,
            _ => return Err(ZError::Stream),
        })
    }
}

/// `struct internal_state`: what `z_stream.state` points at. Each variant is the private state
/// of one kind of stream; `None` is the C's `Z_NULL` (not initialised, or ended).
#[derive(Default)]
pub(crate) enum InternalState {
    /// No state: before an init function or after an end function.
    #[default]
    None,
    /// A compression stream (`deflate.rs`).
    Deflate(crate::zopenbsd::ZBox<crate::deflate::DeflateState>),
    /// A decompression stream (`inflate.rs`).
    Inflate(crate::zopenbsd::ZBox<crate::inflate::InflateState>),
    /// A call-back decompression stream (`infback.rs`). The C's `inflateStateCheck` rejects
    /// it, so it is not an `Inflate`.
    InflateBack(crate::zopenbsd::ZBox<crate::inflate::InflateState>),
}

/// `struct z_stream_s` (`z_stream`): a compression or decompression stream.
///
/// The application fills `next_in` and `next_out` before each call, and must refill `next_in`
/// when it has been consumed and give a new `next_out` when it is full. All other fields are
/// set by the library and must not be changed by the application.
pub struct ZStream<'a> {
    /// `next_in` and `avail_in`: the input not consumed yet.
    pub next_in: &'a [u8],
    /// `total_in`: total number of input bytes read so far.
    pub total_in: u64,
    /// `next_out` and `avail_out`: the output space not filled yet.
    pub next_out: &'a mut [u8],
    /// `total_out`: total number of bytes output so far.
    pub total_out: u64,
    /// `msg`: last error message, `None` if no error.
    pub msg: Option<&'static str>,
    /// `state`: not visible by applications.
    pub(crate) state: InternalState,
    /// `data_type`: best guess about the data type (binary or text) for deflate, or the
    /// decoding state for inflate.
    pub data_type: i32,
    /// `adler`: Adler-32 or CRC-32 value of the uncompressed data.
    pub adler: u32,
}

impl<'a> ZStream<'a> {
    /// A stream with no buffers and no state, as the C's `bzero(&zbuf, sizeof(z_stream))`
    /// leaves it before an init function.
    pub const fn new() -> Self {
        Self {
            next_in: &[],
            total_in: 0,
            next_out: &mut [],
            total_out: 0,
            msg: None,
            state: InternalState::None,
            data_type: 0,
            adler: 0,
        }
    }

    /// `avail_in`: number of bytes available at `next_in`.
    pub const fn avail_in(&self) -> usize {
        self.next_in.len()
    }

    /// `avail_out`: remaining free space at `next_out`.
    pub const fn avail_out(&self) -> usize {
        self.next_out.len()
    }

    /// Consumes `n` bytes of `next_in` (`next_in += n; avail_in -= n`) and returns them.
    /// `n` must not exceed `avail_in`.
    pub(crate) fn take_in(&mut self, n: usize) -> &'a [u8] {
        let (taken, rest) = self.next_in.split_at(n);
        self.next_in = rest;
        taken
    }

    /// Copies `src` to the front of `next_out` and advances it (`zmemcpy(strm->next_out, src,
    /// len); next_out += len; avail_out -= len`). `src` must fit in `avail_out`.
    pub(crate) fn put_out(&mut self, src: &[u8]) {
        let out = core::mem::take(&mut self.next_out);
        let (head, rest) = out.split_at_mut(src.len());
        head.copy_from_slice(src);
        self.next_out = rest;
    }
}

impl Default for ZStream<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct gz_header_s` (`gz_header`): gzip header information passed to and from zlib
/// routines. See RFC 1952 for more details on the meanings of these fields.
#[derive(Default)]
pub struct GzHeader {
    /// `text`: true if compressed data believed to be text.
    pub text: i32,
    /// `time`: modification time.
    pub time: u64,
    /// `xflags`: extra flags (not used when writing a gzip file).
    pub xflags: i32,
    /// `os`: operating system.
    pub os: i32,
    /// `extra` and `extra_len`: the extra field, `None` if none.
    pub extra: Option<Vec<u8>>,
    /// `extra_max`: space at `extra` (only when reading header).
    pub extra_max: u32,
    /// `name`: zero-terminated file name, `None` if none.
    pub name: Option<Vec<u8>>,
    /// `name_max`: space at `name` (only when reading header).
    pub name_max: u32,
    /// `comment`: zero-terminated comment, `None` if none.
    pub comment: Option<Vec<u8>>,
    /// `comm_max`: space at `comment` (only when reading header).
    pub comm_max: u32,
    /// `hcrc`: true if there was or will be a header crc.
    pub hcrc: i32,
    /// `done`: true when done reading gzip header (not used when writing a gzip file).
    pub done: i32,
}

/// The compression strategy of `deflateInit2()` and `deflateParams()` (the C's
/// `Z_DEFAULT_STRATEGY` .. `Z_FIXED`). The order is the C values' order, which `deflate()`
/// compares.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Strategy {
    /// `Z_DEFAULT_STRATEGY`: for normal data.
    Default,
    /// `Z_FILTERED`: for data produced by a filter (small values, somewhat random): fewer
    /// short matches.
    Filtered,
    /// `Z_HUFFMAN_ONLY`: Huffman coding only, no string matches.
    HuffmanOnly,
    /// `Z_RLE`: matches of distance one only (run-length encoding).
    Rle,
    /// `Z_FIXED`: no dynamic Huffman codes.
    Fixed,
}

impl Strategy {
    /// The C value of the strategy (`Z_DEFAULT_STRATEGY` .. `Z_FIXED`).
    pub const fn code(self) -> i32 {
        match self {
            Self::Default => Z_DEFAULT_STRATEGY,
            Self::Filtered => Z_FILTERED,
            Self::HuffmanOnly => Z_HUFFMAN_ONLY,
            Self::Rle => Z_RLE,
            Self::Fixed => Z_FIXED,
        }
    }
}

impl TryFrom<i32> for Strategy {
    type Error = ZError;

    /// The strategy of a C value; `ZError::Stream` for a value outside `Z_DEFAULT_STRATEGY ..=
    /// Z_FIXED`, as `deflateInit2()` and `deflateParams()` refuse one.
    fn try_from(code: i32) -> Result<Self, ZError> {
        Ok(match code {
            Z_DEFAULT_STRATEGY => Self::Default,
            Z_FILTERED => Self::Filtered,
            Z_HUFFMAN_ONLY => Self::HuffmanOnly,
            Z_RLE => Self::Rle,
            Z_FIXED => Self::Fixed,
            _ => return Err(ZError::Stream),
        })
    }
}

/// The C return code of a zlib result: the value the C function would have returned.
pub trait ZCode {
    /// The C value: `Z_OK`, `Z_STREAM_END`, `Z_NEED_DICT` or a negative error code.
    fn code(&self) -> i32;
}

impl ZCode for Result<ZStatus, ZError> {
    fn code(&self) -> i32 {
        match self {
            Ok(status) => status.code(),
            Err(err) => err.code(),
        }
    }
}

impl ZCode for Result<(), ZError> {
    fn code(&self) -> i32 {
        match self {
            Ok(()) => Z_OK,
            Err(err) => err.code(),
        }
    }
}

/// `deflateInit(strm, level)`: the zlib.h macro, [`deflateInit_`](crate::deflate::deflateInit_)
/// with this library's version and stream size.
pub fn deflateInit(strm: &mut ZStream<'_>, level: i32) -> Result<(), ZError> {
    crate::deflate::deflateInit_(strm, level, ZLIB_VERSION, size_of::<ZStream<'_>>() as i32)
}

/// `deflateInit2(strm, level, method, windowBits, memLevel, strategy)`: the zlib.h macro,
/// [`deflateInit2_`](crate::deflate::deflateInit2_) with this library's version and stream
/// size.
pub fn deflateInit2(
    strm: &mut ZStream<'_>,
    level: i32,
    method: i32,
    windowBits: i32,
    memLevel: i32,
    strategy: Strategy,
) -> Result<(), ZError> {
    crate::deflate::deflateInit2_(
        strm,
        level,
        method,
        windowBits,
        memLevel,
        strategy,
        ZLIB_VERSION,
        size_of::<ZStream<'_>>() as i32,
    )
}

/// `inflateInit(strm)`: the zlib.h macro, [`inflateInit_`](crate::inflate::inflateInit_) with
/// this library's version and stream size.
pub fn inflateInit(strm: &mut ZStream<'_>) -> Result<(), ZError> {
    crate::inflate::inflateInit_(strm, ZLIB_VERSION, size_of::<ZStream<'_>>() as i32)
}

/// `inflateInit2(strm, windowBits)`: the zlib.h macro,
/// [`inflateInit2_`](crate::inflate::inflateInit2_) with this library's version and stream
/// size.
pub fn inflateInit2(strm: &mut ZStream<'_>, windowBits: i32) -> Result<(), ZError> {
    crate::inflate::inflateInit2_(
        strm,
        windowBits,
        ZLIB_VERSION,
        size_of::<ZStream<'_>>() as i32,
    )
}

/// `inflateBackInit(strm, windowBits, window)`: the zlib.h macro,
/// [`inflateBackInit_`](crate::infback::inflateBackInit_) with this library's version and
/// stream size.
pub fn inflateBackInit(
    strm: &mut ZStream<'_>,
    windowBits: i32,
    window: Vec<u8>,
) -> Result<(), ZError> {
    crate::infback::inflateBackInit_(
        strm,
        windowBits,
        window,
        ZLIB_VERSION,
        size_of::<ZStream<'_>>() as i32,
    )
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// Every status, error and flush mode against the value zlib.h gives it.
    #[test]
    fn codes_are_zlib_h_values() {
        for (status, code) in [
            (ZStatus::Ok, 0),
            (ZStatus::StreamEnd, 1),
            (ZStatus::NeedDict, 2),
        ] {
            assert_eq!(status.code(), code);
            assert_eq!(Ok::<ZStatus, ZError>(status).code(), code);
        }
        for (err, code) in [
            (ZError::Errno, -1),
            (ZError::Stream, -2),
            (ZError::Data, -3),
            (ZError::Mem, -4),
            (ZError::Buf, -5),
            (ZError::Version, -6),
        ] {
            assert_eq!(err.code(), code);
            assert_eq!(Err::<ZStatus, ZError>(err).code(), code);
            assert_eq!(Err::<(), ZError>(err).code(), code);
        }
        assert_eq!(Ok::<(), ZError>(()).code(), Z_OK);
        let flushes = [
            Flush::NoFlush,
            Flush::PartialFlush,
            Flush::SyncFlush,
            Flush::FullFlush,
            Flush::Finish,
            Flush::Block,
            Flush::Trees,
        ];
        for (code, flush) in flushes.into_iter().enumerate() {
            assert_eq!(usize::try_from(flush.code()), Ok(code));
            assert_eq!(Flush::try_from(flush.code()), Ok(flush));
        }
        assert_eq!(Flush::try_from(-1), Err(ZError::Stream));
        assert_eq!(Flush::try_from(7), Err(ZError::Stream));
        // the order deflate compares is the order of the C values
        assert!(flushes.windows(2).all(|w| w[0] < w[1]));
        let strategies = [
            Strategy::Default,
            Strategy::Filtered,
            Strategy::HuffmanOnly,
            Strategy::Rle,
            Strategy::Fixed,
        ];
        for (code, strategy) in strategies.into_iter().enumerate() {
            assert_eq!(usize::try_from(strategy.code()), Ok(code));
            assert_eq!(Strategy::try_from(strategy.code()), Ok(strategy));
        }
        assert_eq!(Strategy::try_from(-1), Err(ZError::Stream));
        assert_eq!(Strategy::try_from(5), Err(ZError::Stream));
        assert!(strategies.windows(2).all(|w| w[0] < w[1]));
    }
}
/* </TESTS> */
