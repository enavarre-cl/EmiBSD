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

/* inflate.c -- zlib decompression
 * Copyright (C) 1995-2026 Mark Adler
 * For conditions of distribution and use, see copyright notice in zlib.h
 */

/* inflate.h -- internal inflate state definition
 * Copyright (C) 1995-2019 Mark Adler
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
//! zlib decompression: the inflate state and its state machine, `inflate()`, and the
//! functions that set up, reset, prime, synchronise, copy and end an inflate stream.
//!
//! Upstream: sys/lib/libz/inflate.c @ 3ce1f3f79392, sys/lib/libz/inflate.h @ 3ce1f3f79392
//! LZ: sys/lib/libz/inflate.rs@f5985f1d055a
//!
//! **This is an altered source version, not the original zlib `inflate.c`/`inflate.h`** (zlib
//! licence, clause 2): a Rust rewrite written for EmiBSD. The original's notice is kept above
//! in full (clause 3). Do not mistake it for the zlib distribution.
//!
//! `inflate()` is a state machine: each `InflateMode` either has the input bits or output
//! space it needs to make progress, makes it and moves to the next mode, or leaves, so that
//! the next call retries the same mode. The C keeps the stream's buffers and the bit
//! accumulator in local "registers" (`LOAD()`/`RESTORE()`); here they are a `Regs` value,
//! and the C's `NEEDBITS`/`PULLBYTE`/`BITS`/`DROPBITS`/`BYTEBITS`/`INITBITS` macros are its
//! methods. A `goto inf_leave` is a `break 'inf_leave` out of the `loop { match mode { .. } }`;
//! a `case` falling through to the next is the mode being set and the loop dispatching it
//! (the one fall-through that keeps its mode, `TYPE` into `TYPEDO`, is one arm). On the way
//! out, `inflate()` updates the totals and the check value, and copies the last 32K of output
//! into the sliding window (allocated only when a stream needs more than one call), so the
//! next call can reach back into it. The flush parameter only changes the return code: with
//! `Z_FINISH`, `inflate()` returns `Z_BUF_ERROR` rather than `Z_OK` if the stream is not
//! finished. The C's change history (1.2.beta0 .. 1.2.0) is in `inflate.c`.
//!
//! The kernel's zlib is built with `-DSLOW -DSMALL -DNO_GZIP` (`SLOW`, `SMALL`,
//! `NO_GZIP`): `inflate()` never calls `inflate_fast()`, every error message is `"error"`,
//! and there is no gzip decoding. Both sides of `SLOW` and `SMALL` are written; the host tests
//! decode every stream with and without `inflate_fast()`.
//!
//! ## Deviations
//! - `GUNZIP` is not defined (`NO_GZIP`): the gzip modes (`FLAGS` .. `HCRC`, `LENGTH`) exist in
//!   `InflateMode` but nothing enters them, `windowBits` above 15 are refused as in the C
//!   build, and the header and trailer checks are the zlib ones only. A comment marks each
//!   site. `inflateGetHeader` refuses every stream (`ZError::Stream`, since `wrap` never has
//!   bit 1), as it does in the C build; the state has no `head` (it could not keep the
//!   caller's `&mut GzHeader` anyway).
//! - The state is `InternalState::Inflate` in the stream; `inflateStateCheck` checks the
//!   variant. A state made by `inflateBackInit_` is `InternalState::InflateBack`: the C's
//!   check rejects it too (that init never sets `state->strm` or `mode`). The `strm` back
//!   pointer and the `zalloc`/`zfree`/`opaque` checks have no counterpart (the state is owned
//!   by its stream; the allocator is always `zopenbsd.rs`), and neither have the `Z_NULL`
//!   checks of `strm`, `next_in` and `next_out` (slices are never null).
//! - `lencode`, `distcode` and `next` point into `codes[]` or at the fixed tables in the C;
//!   here they are a `CodeTable` selector and an index. `lens`, `work`, `codes` and the
//!   window are heap buffers from `zcalloc` (the state stays small: no 7K stack temporary).
//! - `inflate()` works on the whole output buffer it was given (`put` is an index into it),
//!   because matches, the check value and the window update read the output written earlier in
//!   the same call, which the C reaches backwards from `next_out`; the stream's `next_out` is
//!   handed back advanced on every return.
//! - `inflateGetDictionary` takes `Option<&mut [u8]>`; a buffer shorter than the window's
//!   contents gets `ZError::Buf` (the C overruns it). `inflateSetDictionary` takes a slice.
//! - `inflateCopy` reads `source` through a shared reference and copies its stream fields too,
//!   but `dest.next_out` is left empty: the output buffer is a `&mut` and cannot be shared by
//!   two streams. The caller gives `dest` its own.
//! - Not defined in the kernel build, so only the `#else` side is ported: `INFLATE_STRICT`
//!   (`dmax` is kept but never checked), `INFLATE_ALLOW_INVALID_DISTANCE_TOOFAR_ARRR`
//!   (`inflateUndermine` returns `ZError::Data` and `sane` stays true), `PKZIP_BUG_WORKAROUND`,
//!   `Z_SOLO`, `ZLIB_DEBUG` (`Trace*` dropped, `Assert` is `zassert!`). `BUILDFIXED` and
//!   `MAKEFIXED` concern `inftrees.c` (`inflate_fixed`), which this zlib's `inflate.c` calls
//!   instead of having `fixedtables()`/`makefixed()` of its own.
//!
//! ## Redesign
//! - Every function returns a `Result` instead of a zlib status code (`zlib.rs`, [`ZStatus`]
//!   and [`ZError`]): `inflate()` returns `Ok(ZStatus::Ok | StreamEnd | NeedDict)` or the
//!   error; the functions whose only success is `Z_OK` return `Result<(), ZError>`. A missing
//!   or foreign state is `Err(ZError::Stream)` everywhere, which `with_state` now returns.
//! - The values the C returns through out parameters or in-band sentinels are the `Ok` value:
//!   `inflateGetDictionary` returns the dictionary length (LZ: `dictLength: Option<&mut u32>`),
//!   `inflateSyncPoint` a `bool` (LZ: 1/0 or `Z_STREAM_ERROR` in one `i32`), `inflateMark`
//!   and `inflateCodesUsed` their value or `Err(ZError::Stream)` (LZ: the sentinels
//!   `-(1 << 16)` and `u64::MAX`, which `.unwrap_or` gives back).
//! - `flush` is a [`Flush`]; `inflateValidate`'s `check` and `inflateUndermine`'s `subvert` are
//!   `bool` (they were C ints used as flags).
//! - `updatewindow` returns `Result<(), ZError>` (`Err(ZError::Mem)` when the window cannot be
//!   allocated; LZ returned the C's `1` as `true`), `syncsearch` returns the new pattern count
//!   with the bytes read instead of updating a `&mut` count, and `inflate_table` returns the
//!   tables' end and root bits (`inftrees.rs`).
//! - `wsize`, `whave` and `wnext` are `usize`: they index the window, and the casts at every
//!   use are gone.

#![allow(non_snake_case)] // zlib's API names are camelCase in C (inflateInit2_, inflateReset2)

use alloc::vec::Vec;

use crate::adler32::adler32;
use crate::inffast::inflate_fast;
use crate::inffixed::{distfix, lenfix};
use crate::inftrees::{Code, CodeType, ENOUGH, inflate_fixed, inflate_table};
use crate::zlib::{
    Flush, GzHeader, InternalState, Z_DEFLATED, ZError, ZLIB_VERSION, ZStatus, ZStream,
};
use crate::zopenbsd::{ZBox, zcalloc, zcalloc_box, zcfree};
use crate::zutil::{DEF_WBITS, SLOW, SMALL};

/// `inflate_mode`: the possible inflate modes between `inflate()` calls. The values start at
/// 16180, as in the C, so a stray integer is unlikely to look like a mode; the order matters
/// (`inflate()` compares modes with `<`). "i" marks a mode that waits for input, "o" one
/// that waits for output space.
///
/// Transitions (most modes can also go to `BAD` or `MEM` on error):
/// - header: `HEAD` → (gzip) or (zlib) or (raw); (gzip) → `FLAGS` → `TIME` → `OS` → `EXLEN` →
///   `EXTRA` → `NAME` → `COMMENT` → `HCRC` → `TYPE`; (zlib) → `DICTID` or `TYPE`; `DICTID` →
///   `DICT` → `TYPE`; (raw) → `TYPEDO`;
/// - blocks: `TYPE` → `TYPEDO` → `STORED` or `TABLE` or `LEN_` or `CHECK`; `STORED` → `COPY_`
///   → `COPY` → `TYPE`; `TABLE` → `LENLENS` → `CODELENS` → `LEN_`; `LEN_` → `LEN`;
/// - codes: `LEN` → `LENEXT` or `LIT` or `TYPE`; `LENEXT` → `DIST` → `DISTEXT` → `MATCH` →
///   `LEN`; `LIT` → `LEN`;
/// - trailer: `CHECK` → `LENGTH` → `DONE`.
#[allow(non_camel_case_types, clippy::upper_case_acronyms)] // the C's enumerator names
#[allow(dead_code)] // the gzip modes: nothing enters them when GUNZIP is not defined
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum InflateMode {
    /// i: waiting for magic header
    HEAD = 16180,
    /// i: waiting for method and flags (gzip)
    FLAGS,
    /// i: waiting for modification time (gzip)
    TIME,
    /// i: waiting for extra flags and operating system (gzip)
    OS,
    /// i: waiting for extra length (gzip)
    EXLEN,
    /// i: waiting for extra bytes (gzip)
    EXTRA,
    /// i: waiting for end of file name (gzip)
    NAME,
    /// i: waiting for end of comment (gzip)
    COMMENT,
    /// i: waiting for header crc (gzip)
    HCRC,
    /// i: waiting for dictionary check value
    DICTID,
    /// waiting for inflateSetDictionary() call
    DICT,
    /// i: waiting for type bits, including last-flag bit
    TYPE,
    /// i: same, but skip check to exit inflate on new block
    TYPEDO,
    /// i: waiting for stored size (length and complement)
    STORED,
    /// i/o: same as COPY below, but only first time in
    COPY_,
    /// i/o: waiting for input or output to copy stored block
    COPY,
    /// i: waiting for dynamic block table lengths
    TABLE,
    /// i: waiting for code length code lengths
    LENLENS,
    /// i: waiting for length/lit and distance code lengths
    CODELENS,
    /// i: same as LEN below, but only first time in
    LEN_,
    /// i: waiting for length/lit/eob code
    LEN,
    /// i: waiting for length extra bits
    LENEXT,
    /// i: waiting for distance code
    DIST,
    /// i: waiting for distance extra bits
    DISTEXT,
    /// o: waiting for output space to copy string
    MATCH,
    /// o: waiting for output space to write literal
    LIT,
    /// i: waiting for 32-bit check value
    CHECK,
    /// i: waiting for 32-bit length (gzip)
    LENGTH,
    /// finished check, done -- remain here until reset
    DONE,
    /// got a data error -- remain here until reset
    BAD,
    /// got an inflate() memory error -- remain here until reset
    MEM,
    /// looking for synchronization bytes to restart inflate()
    SYNC,
}

/// What the C's `code const *lencode` and `*distcode` point at: one of the fixed tables of
/// `inffixed.rs`, or a table built by `inflate_table` at an offset in the state's `codes`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CodeTable {
    /// `lenfix`: the fixed literal/length table.
    LenFix,
    /// `distfix`: the fixed distance table.
    DistFix,
    /// `codes + offset`: a dynamic table in the state's code space.
    Codes(usize),
}

/// `struct inflate_state`: what inflate keeps between calls, about 300 bytes plus the heap
/// buffers (`lens`, `work` and `codes`, about 7K, and the window, up to 32K).
pub(crate) struct InflateState {
    /// `mode`: current inflate mode.
    pub(crate) mode: InflateMode,
    /// `last`: true if processing last block.
    pub(crate) last: bool,
    /// `wrap`: bit 0 true for zlib, bit 1 true for gzip, bit 2 true to validate check value.
    pub(crate) wrap: i32,
    /// `havedict`: true if dictionary provided.
    pub(crate) havedict: bool,
    /// `flags`: gzip header method and flags, 0 if zlib, or -1 if raw or no header yet.
    pub(crate) flags: i32,
    /// `dmax`: zlib header max distance (`INFLATE_STRICT`).
    pub(crate) dmax: u32,
    /// `check`: protected copy of check value.
    pub(crate) check: u32,
    /// `total`: protected copy of output count.
    pub(crate) total: u64,
    /// `wbits`: log base 2 of requested window size.
    pub(crate) wbits: u32,
    /// `wsize`: window size or zero if not using window.
    pub(crate) wsize: usize,
    /// `whave`: valid bytes in the window.
    pub(crate) whave: usize,
    /// `wnext`: window write index.
    pub(crate) wnext: usize,
    /// `window`: allocated sliding window, if needed (`1 << wbits` bytes); for
    /// `inflateBack()`, the caller's window, which is also its output buffer.
    pub(crate) window: Option<Vec<u8>>,
    /// `hold`: input bit accumulator.
    pub(crate) hold: u64,
    /// `bits`: number of bits in hold.
    pub(crate) bits: u32,
    /// `length`: literal or length of data to copy.
    pub(crate) length: u32,
    /// `offset`: distance back to copy string from.
    pub(crate) offset: u32,
    /// `extra`: extra bits needed.
    pub(crate) extra: u32,
    /// `lencode`: starting table for length/literal codes.
    pub(crate) lencode: CodeTable,
    /// `distcode`: starting table for distance codes.
    pub(crate) distcode: CodeTable,
    /// `lenbits`: index bits for lencode.
    pub(crate) lenbits: u32,
    /// `distbits`: index bits for distcode.
    pub(crate) distbits: u32,
    /// `ncode`: number of code length code lengths.
    pub(crate) ncode: u32,
    /// `nlen`: number of length code lengths.
    pub(crate) nlen: u32,
    /// `ndist`: number of distance code lengths.
    pub(crate) ndist: u32,
    /// `have`: number of code lengths in lens[].
    pub(crate) have: u32,
    /// `next`: next available space in codes[], as an index.
    pub(crate) next: usize,
    /// `lens`: temporary storage for code lengths (320).
    pub(crate) lens: Vec<u16>,
    /// `work`: work area for code table building (288).
    pub(crate) work: Vec<u16>,
    /// `codes`: space for code tables (`ENOUGH`).
    pub(crate) codes: Vec<Code>,
    /// `sane`: if false, allow invalid distance too far.
    pub(crate) sane: bool,
    /// `back`: bits back of last unprocessed length/lit.
    pub(crate) back: i32,
    /// `was`: initial length of match.
    pub(crate) was: u32,
}

impl InflateState {
    /// `ZALLOC` + `zmemzero` of a state: everything zero, no window, mode `HEAD`. `None` when
    /// the allocator has no memory.
    pub(crate) fn new() -> Option<ZBox<Self>> {
        let lens = zcalloc(320)?;
        let work = zcalloc(288)?;
        let codes = zcalloc(ENOUGH)?;
        zcalloc_box(Self {
            mode: InflateMode::HEAD,
            last: false,
            wrap: 0,
            havedict: false,
            flags: 0,
            dmax: 0,
            check: 0,
            total: 0,
            wbits: 0,
            wsize: 0,
            whave: 0,
            wnext: 0,
            window: None,
            hold: 0,
            bits: 0,
            length: 0,
            offset: 0,
            extra: 0,
            lencode: CodeTable::Codes(0),
            distcode: CodeTable::Codes(0),
            lenbits: 0,
            distbits: 0,
            ncode: 0,
            nlen: 0,
            ndist: 0,
            have: 0,
            next: 0,
            lens,
            work,
            codes,
            sane: false,
            back: 0,
            was: 0,
        })
    }

    /// The decoding table `t` selects: a fixed table, or the dynamic one in `codes`.
    pub(crate) fn table(&self, t: CodeTable) -> &[Code] {
        match t {
            CodeTable::LenFix => &lenfix,
            CodeTable::DistFix => &distfix,
            CodeTable::Codes(offset) => &self.codes[offset..],
        }
    }

    /// The `inflateCopy` copy of the state: new buffers with the same contents (of the
    /// window, its valid bytes). `None` when the allocator has no memory.
    fn try_clone(&self) -> Option<ZBox<Self>> {
        let mut lens = zcalloc(self.lens.len())?;
        lens.copy_from_slice(&self.lens);
        let mut work = zcalloc(self.work.len())?;
        work.copy_from_slice(&self.work);
        let mut codes = zcalloc(self.codes.len())?;
        codes.copy_from_slice(&self.codes);
        let window = match &self.window {
            Some(src) => {
                let mut window = zcalloc(src.len())?;
                window[..self.whave].copy_from_slice(&src[..self.whave]);
                Some(window)
            }
            None => None,
        };
        zcalloc_box(Self {
            window,
            lens,
            work,
            codes,
            ..*self
        })
    }
}

/// The local "registers" of `inflate()` and `inflateBack()`: the input and output buffers
/// with the positions in them (the C's `next`/`have` and `put`/`left`), and the bit
/// accumulator (`hold`/`bits`). `LOAD()` fills them from the stream and the state,
/// `RESTORE()` puts them back. `output` starts where the call's output starts, so the bytes
/// written earlier in the same call stay reachable at `output[..put]`.
pub(crate) struct Regs<'i, 'o> {
    /// The input; `next` is the next byte to read.
    pub(crate) input: &'i [u8],
    /// `next`: index of the next input byte.
    pub(crate) next: usize,
    /// The output; `put` is the next byte to write.
    pub(crate) output: &'o mut [u8],
    /// `put`: index of the next output byte.
    pub(crate) put: usize,
    /// `hold`: bit buffer.
    pub(crate) hold: u64,
    /// `bits`: bits in bit buffer.
    pub(crate) bits: u32,
}

impl<'a> Regs<'a, 'a> {
    /// `LOAD()`: take the stream's buffers and the state's bit accumulator.
    fn load(strm: &mut ZStream<'a>, state: &InflateState) -> Self {
        Self {
            input: strm.next_in,
            next: 0,
            output: core::mem::take(&mut strm.next_out),
            put: 0,
            hold: state.hold,
            bits: state.bits,
        }
    }

    /// `RESTORE()`: give the stream its buffers back, advanced past what was read and
    /// written, and the state its bit accumulator.
    fn restore(self, strm: &mut ZStream<'a>, state: &mut InflateState) {
        strm.next_in = &self.input[self.next..];
        strm.next_out = &mut self.output[self.put..];
        state.hold = self.hold;
        state.bits = self.bits;
    }
}

impl Regs<'_, '_> {
    /// `have`: available input.
    pub(crate) fn have(&self) -> usize {
        self.input.len() - self.next
    }

    /// `left`: available output.
    pub(crate) fn left(&self) -> usize {
        self.output.len() - self.put
    }

    /// `INITBITS()`: clear the input bit accumulator.
    pub(crate) fn init_bits(&mut self) {
        self.hold = 0;
        self.bits = 0;
    }

    /// `PULLBYTE()`: get a byte of input into the bit accumulator; `false` when there is no
    /// input available (the C's `goto inf_leave`).
    pub(crate) fn pull_byte(&mut self) -> bool {
        let Some(&b) = self.input.get(self.next) else {
            return false;
        };
        self.next += 1;
        self.hold += u64::from(b) << self.bits;
        self.bits += 8;
        true
    }

    /// `NEEDBITS(n)`: make sure there are at least `n` bits in the accumulator; `false` when
    /// the input runs out first.
    pub(crate) fn need_bits(&mut self, n: u32) -> bool {
        while self.bits < n {
            if !self.pull_byte() {
                return false;
            }
        }
        true
    }

    /// `BITS(n)`: the low `n` bits of the accumulator (`n < 16`).
    pub(crate) fn low_bits(&self, n: u32) -> u32 {
        (self.hold as u32) & ((1u32 << n) - 1)
    }

    /// `DROPBITS(n)`: remove `n` bits from the accumulator.
    pub(crate) fn drop_bits(&mut self, n: u32) {
        self.hold >>= n;
        self.bits -= n;
    }

    /// `BYTEBITS()`: remove zero to seven bits as needed to go to a byte boundary.
    pub(crate) fn byte_bits(&mut self) {
        self.hold >>= self.bits & 7;
        self.bits -= self.bits & 7;
    }
}

/// `order`: the permutation of code lengths in a dynamic block header.
#[allow(non_upper_case_globals)] // the C's name
pub(crate) static order: [u16; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// An inflate error message: `"error"` under `SMALL` (the kernel build), `long` otherwise.
pub(crate) const fn small_msg(long: &'static str) -> &'static str {
    if SMALL { "error" } else { long }
}

/// Copy `len` bytes of `output` from `from` to `*put`, front to back, and advance `*put`. The
/// source may overlap the bytes being written (a match whose distance is shorter than its
/// length repeats them), so this is a byte loop, as in the C, not a `memmove`.
pub(crate) fn copy_forward(output: &mut [u8], put: &mut usize, from: usize, len: usize) {
    for i in 0..len {
        output[*put + i] = output[from + i];
    }
    *put += len;
}

/// `inflateStateCheck`: true (the C's 1) when `strm` has no inflate state.
fn inflateStateCheck(strm: &ZStream<'_>) -> bool {
    !matches!(strm.state, InternalState::Inflate(_))
}

/// Run `f` on the stream and its inflate state, which is taken out of the stream for the
/// call and put back afterwards. `Err(ZError::Stream)` (and `f` not called) when
/// `inflateStateCheck` fails.
fn with_state<'a, R>(
    strm: &mut ZStream<'a>,
    f: impl FnOnce(&mut ZStream<'a>, &mut InflateState) -> Result<R, ZError>,
) -> Result<R, ZError> {
    match core::mem::take(&mut strm.state) {
        InternalState::Inflate(mut state) => {
            let r = f(strm, &mut state);
            strm.state = InternalState::Inflate(state);
            r
        }
        other => {
            strm.state = other;
            Err(ZError::Stream)
        }
    }
}

/// `inflateResetKeep`: reset the stream for a new stream but keep the window and its
/// contents (`inflateReset` drops them). `Err(ZError::Stream)` without an inflate state.
pub fn inflateResetKeep(strm: &mut ZStream<'_>) -> Result<(), ZError> {
    with_state(strm, |strm, state| {
        reset_keep(strm, state);
        Ok(())
    })
}

/// The body of `inflateResetKeep`, once the state is known to be valid.
fn reset_keep(strm: &mut ZStream<'_>, state: &mut InflateState) {
    strm.total_in = 0;
    strm.total_out = 0;
    state.total = 0;
    strm.msg = None;
    strm.data_type = 0;
    if state.wrap != 0 {
        // to support ill-conceived Java test suite
        strm.adler = u32::from(state.wrap & 1 != 0);
    }
    state.mode = InflateMode::HEAD;
    state.last = false;
    state.havedict = false;
    state.flags = -1;
    state.dmax = 32768;
    state.hold = 0;
    state.bits = 0;
    state.lencode = CodeTable::Codes(0);
    state.distcode = CodeTable::Codes(0);
    state.next = 0;
    state.sane = true;
    state.back = -1;
}

/// `inflateReset`: reset the stream for a new stream with the same parameters; the window
/// contents are dropped (the window memory is kept). `Err(ZError::Stream)` without an
/// inflate state.
pub fn inflateReset(strm: &mut ZStream<'_>) -> Result<(), ZError> {
    with_state(strm, |strm, state| {
        reset(strm, state);
        Ok(())
    })
}

/// The body of `inflateReset`, once the state is known to be valid.
fn reset(strm: &mut ZStream<'_>, state: &mut InflateState) {
    state.wsize = 0;
    state.whave = 0;
    state.wnext = 0;
    reset_keep(strm, state);
}

/// `inflateReset2`: [`inflateReset`] with a new `windowBits` (as for [`inflateInit2_`]); the
/// window is freed if its size changes. `Err(ZError::Stream)` for an invalid `windowBits` or
/// without an inflate state.
pub fn inflateReset2(strm: &mut ZStream<'_>, windowBits: i32) -> Result<(), ZError> {
    with_state(strm, |strm, state| reset2(strm, state, windowBits))
}

/// The body of `inflateReset2`, once the state is known to be valid.
fn reset2(strm: &mut ZStream<'_>, state: &mut InflateState, windowBits: i32) -> Result<(), ZError> {
    // extract wrap request from windowBits parameter
    let mut windowBits = windowBits;
    let wrap = if windowBits < 0 {
        if windowBits < -15 {
            return Err(ZError::Stream);
        }
        windowBits = -windowBits;
        0
    } else {
        // GUNZIP is not defined: windowBits 16 and up (gzip, or auto-detect) are not
        // masked to their low four bits, so the range check below refuses them.
        (windowBits >> 4) + 5
    };

    // set number of window bits, free window if different
    if windowBits != 0 && !(8..=15).contains(&windowBits) {
        return Err(ZError::Stream);
    }
    let wbits = windowBits.unsigned_abs(); // 0 or 8..=15
    if state.window.is_some() && state.wbits != wbits {
        zcfree(state.window.take());
    }

    // update state and reset the rest of it
    state.wrap = wrap;
    state.wbits = wbits;
    reset(strm, state);
    Ok(())
}

/// `inflateInit2_`: initialise `strm` for decompression. `windowBits` is 8..15 for a zlib
/// stream with a window of up to `1 << windowBits` bytes (0: use the size in the header),
/// or -8..-15 for raw deflate data. `version` and `stream_size` must match the library
/// (`inflateInit2` in zlib.rs passes them). Fails with `ZError::Mem`, `ZError::Version`, or
/// `ZError::Stream` for an invalid `windowBits`.
pub fn inflateInit2_(
    strm: &mut ZStream<'_>,
    windowBits: i32,
    version: &str,
    stream_size: i32,
) -> Result<(), ZError> {
    if version.as_bytes().first() != ZLIB_VERSION.as_bytes().first()
        || stream_size != size_of::<ZStream<'_>>() as i32
    {
        return Err(ZError::Version);
    }
    strm.msg = None; // in case we return an error
    let mut state = InflateState::new().ok_or(ZError::Mem)?;
    // mode is HEAD, to pass the state test in inflateReset2()
    let ret = reset2(strm, &mut state, windowBits);
    if ret.is_ok() {
        strm.state = InternalState::Inflate(state);
    } else {
        zcfree(state);
        strm.state = InternalState::None;
    }
    ret
}

/// `inflateInit_`: [`inflateInit2_`] with the default window, `DEF_WBITS`, and a zlib
/// wrapper.
pub fn inflateInit_(strm: &mut ZStream<'_>, version: &str, stream_size: i32) -> Result<(), ZError> {
    inflateInit2_(strm, DEF_WBITS, version, stream_size)
}

/// `inflatePrime`: insert `bits` bits of `value` (at most 16, and at most 32 in the
/// accumulator) into the input, as if they came before `next_in`. A negative `bits` empties
/// the accumulator. `Err(ZError::Stream)` for too many bits or without an inflate state.
pub fn inflatePrime(strm: &mut ZStream<'_>, bits: i32, value: i32) -> Result<(), ZError> {
    with_state(strm, |_, state| {
        if bits == 0 {
            return Ok(());
        }
        if bits < 0 {
            state.hold = 0;
            state.bits = 0;
            return Ok(());
        }
        let bits = bits.unsigned_abs();
        if bits > 16 || state.bits + bits > 32 {
            return Err(ZError::Stream);
        }
        let value = i64::from(value) & ((1i64 << bits) - 1); // 0..=0xffff
        state.hold += value.unsigned_abs() << state.bits;
        state.bits += bits;
        Ok(())
    })
}

/// `updatewindow`: copy the last `wsize` (normally 32K) bytes of `src`, the output just
/// written (or a dictionary), into the circular window, allocating the window first if
/// needed. `Err(ZError::Mem)` when the window cannot be allocated.
///
/// It runs only when a window is in use already, or when a call wrote output but did not
/// reach the end of the stream (or for a dictionary). Output buffers larger than 32K help:
/// only the last 32K are copied, and later distances fall within the output itself.
pub(crate) fn updatewindow(state: &mut InflateState, src: &[u8]) -> Result<(), ZError> {
    // if it hasn't been done already, allocate space for the window
    if state.window.is_none() {
        state.window = Some(zcalloc(1usize << state.wbits).ok_or(ZError::Mem)?);
    }

    // if window not in use yet, initialize
    if state.wsize == 0 {
        state.wsize = 1 << state.wbits;
        state.wnext = 0;
        state.whave = 0;
    }

    let Some(window) = state.window.as_deref_mut() else {
        return Err(ZError::Mem);
    };
    let (wsize, wnext) = (state.wsize, state.wnext);

    // copy state->wsize or less output bytes into the circular window
    let copy = src.len();
    if copy >= wsize {
        window[..wsize].copy_from_slice(&src[copy - wsize..]);
        state.wnext = 0;
        state.whave = state.wsize;
    } else {
        let dist = (wsize - wnext).min(copy);
        window[wnext..wnext + dist].copy_from_slice(&src[..dist]);
        let rest = copy - dist;
        if rest != 0 {
            window[..rest].copy_from_slice(&src[dist..]);
            state.wnext = rest;
            state.whave = state.wsize;
        } else {
            state.wnext += dist;
            if state.wnext == state.wsize {
                state.wnext = 0;
            }
            if state.whave < state.wsize {
                state.whave += dist;
            }
        }
    }
    Ok(())
}

/// `inflate`: decompress as much as possible from `next_in` into `next_out`, advancing both,
/// and return `Ok(ZStatus::Ok)` (progress made), `Ok(ZStatus::StreamEnd)` (the end of the
/// stream, check value verified), `Ok(ZStatus::NeedDict)` (call [`inflateSetDictionary`];
/// `adler` is the dictionary's id), or fail with `ZError::Data` (corrupt input; `msg` says
/// why), `ZError::Mem`, `ZError::Stream` (no inflate state), or `ZError::Buf` (no progress
/// was possible, or `flush` is `Flush::Finish` and the end was not reached). `flush` is
/// `NoFlush`, `SyncFlush`, `Finish`, or `Block` / `Trees` to stop at the next block boundary
/// / after the block header. `data_type` tells where decoding stopped: the unused bits in the
/// last input byte, plus 64 in the last block, 128 at the end of a block, 256 after a block
/// header.
pub fn inflate(strm: &mut ZStream<'_>, flush: Flush) -> Result<ZStatus, ZError> {
    inflate_impl(strm, flush, SLOW)
}

/// `inflate()` with `SLOW` as a parameter: the kernel passes `SLOW` (true), the host tests
/// both values, so that `inflate_fast()` decodes the same streams too.
pub(crate) fn inflate_impl(
    strm: &mut ZStream<'_>,
    flush: Flush,
    slow: bool,
) -> Result<ZStatus, ZError> {
    with_state(strm, |strm, state| inflate_run(strm, state, flush, slow))
}

/// The state machine of `inflate()`, once the state is known to be valid.
fn inflate_run(
    strm: &mut ZStream<'_>,
    state: &mut InflateState,
    flush: Flush,
    slow: bool,
) -> Result<ZStatus, ZError> {
    use InflateMode::*;

    if state.mode == TYPE {
        state.mode = TYPEDO; // skip check
    }
    let mut r = Regs::load(strm, state);
    let in_ = r.have(); // save starting available input
    let mut out = r.left(); // and output
    let mut ret = Ok(ZStatus::Ok);

    'inf_leave: loop {
        match state.mode {
            HEAD => {
                if state.wrap == 0 {
                    state.mode = TYPEDO;
                    continue;
                }
                if !r.need_bits(16) {
                    break 'inf_leave;
                }
                // GUNZIP is not defined: no gzip magic check (and no head->done), and a zlib
                // header is checked without asking whether wrap allows it.
                if !(u64::from(r.low_bits(8) << 8) + (r.hold >> 8)).is_multiple_of(31) {
                    strm.msg = Some(small_msg("incorrect header check"));
                    state.mode = BAD;
                    continue;
                }
                if r.low_bits(4) != Z_DEFLATED.unsigned_abs() {
                    strm.msg = Some(small_msg("unknown compression method"));
                    state.mode = BAD;
                    continue;
                }
                r.drop_bits(4);
                let len = r.low_bits(4) + 8;
                if state.wbits == 0 {
                    state.wbits = len;
                }
                if len > 15 || len > state.wbits {
                    strm.msg = Some(small_msg("invalid window size"));
                    state.mode = BAD;
                    continue;
                }
                state.dmax = 1 << len;
                state.flags = 0; // indicate zlib header
                state.check = adler32(0, None);
                strm.adler = state.check;
                state.mode = if r.hold & 0x200 != 0 { DICTID } else { TYPE };
                r.init_bits();
            }
            DICTID => {
                if !r.need_bits(32) {
                    break 'inf_leave;
                }
                state.check = (r.hold as u32).swap_bytes(); // ZSWAP32
                strm.adler = state.check;
                r.init_bits();
                state.mode = DICT;
            }
            DICT => {
                if !state.havedict {
                    r.restore(strm, state);
                    return Ok(ZStatus::NeedDict);
                }
                state.check = adler32(0, None);
                strm.adler = state.check;
                state.mode = TYPE;
            }
            TYPE | TYPEDO => {
                if state.mode == TYPE && matches!(flush, Flush::Block | Flush::Trees) {
                    break 'inf_leave;
                }
                if state.last {
                    r.byte_bits();
                    state.mode = CHECK;
                    continue;
                }
                if !r.need_bits(3) {
                    break 'inf_leave;
                }
                state.last = r.low_bits(1) != 0;
                r.drop_bits(1);
                match r.low_bits(2) {
                    0 => state.mode = STORED, // stored block
                    1 => {
                        // fixed block
                        inflate_fixed(state);
                        state.mode = LEN_; // decode codes
                        if flush == Flush::Trees {
                            r.drop_bits(2);
                            break 'inf_leave;
                        }
                    }
                    2 => state.mode = TABLE, // dynamic block
                    _ => {
                        strm.msg = Some(small_msg("invalid block type"));
                        state.mode = BAD;
                    }
                }
                r.drop_bits(2);
            }
            STORED => {
                r.byte_bits(); // go to byte boundary
                if !r.need_bits(32) {
                    break 'inf_leave;
                }
                if (r.hold & 0xffff) != ((r.hold >> 16) ^ 0xffff) {
                    strm.msg = Some(small_msg("invalid stored block lengths"));
                    state.mode = BAD;
                    continue;
                }
                state.length = (r.hold & 0xffff) as u32;
                r.init_bits();
                state.mode = COPY_;
                if flush == Flush::Trees {
                    break 'inf_leave;
                }
            }
            COPY_ => state.mode = COPY,
            COPY => {
                let copy = state.length as usize;
                if copy != 0 {
                    let copy = copy.min(r.have()).min(r.left());
                    if copy == 0 {
                        break 'inf_leave;
                    }
                    r.output[r.put..r.put + copy].copy_from_slice(&r.input[r.next..r.next + copy]);
                    r.next += copy;
                    r.put += copy;
                    state.length -= copy as u32;
                    continue;
                }
                state.mode = TYPE;
            }
            TABLE => {
                if !r.need_bits(14) {
                    break 'inf_leave;
                }
                state.nlen = r.low_bits(5) + 257;
                r.drop_bits(5);
                state.ndist = r.low_bits(5) + 1;
                r.drop_bits(5);
                state.ncode = r.low_bits(4) + 4;
                r.drop_bits(4);
                // PKZIP_BUG_WORKAROUND is not defined: the counts are checked
                if state.nlen > 286 || state.ndist > 30 {
                    strm.msg = Some(small_msg("too many length or distance symbols"));
                    state.mode = BAD;
                    continue;
                }
                state.have = 0;
                state.mode = LENLENS;
            }
            LENLENS => {
                while state.have < state.ncode {
                    if !r.need_bits(3) {
                        break 'inf_leave;
                    }
                    state.lens[order[state.have as usize] as usize] = r.low_bits(3) as u16;
                    state.have += 1;
                    r.drop_bits(3);
                }
                while state.have < 19 {
                    state.lens[order[state.have as usize] as usize] = 0;
                    state.have += 1;
                }
                state.next = 0;
                state.lencode = CodeTable::Codes(0);
                state.distcode = CodeTable::Codes(0);
                state.lenbits = 7;
                match inflate_table(
                    CodeType::CODES,
                    &state.lens[..19],
                    &mut state.codes,
                    state.next,
                    state.lenbits,
                    &mut state.work,
                ) {
                    Ok((next, bits)) => (state.next, state.lenbits) = (next, bits),
                    Err(_) => {
                        strm.msg = Some(small_msg("invalid code lengths set"));
                        state.mode = BAD;
                        continue;
                    }
                }
                state.have = 0;
                state.mode = CODELENS;
            }
            CODELENS => {
                while state.have < state.nlen + state.ndist {
                    let here = loop {
                        let here = state.table(state.lencode)[r.low_bits(state.lenbits) as usize];
                        if u32::from(here.bits) <= r.bits {
                            break here;
                        }
                        if !r.pull_byte() {
                            break 'inf_leave;
                        }
                    };
                    if here.val < 16 {
                        r.drop_bits(u32::from(here.bits));
                        state.lens[state.have as usize] = here.val;
                        state.have += 1;
                    } else {
                        let (len, copy);
                        if here.val == 16 {
                            if !r.need_bits(u32::from(here.bits) + 2) {
                                break 'inf_leave;
                            }
                            r.drop_bits(u32::from(here.bits));
                            if state.have == 0 {
                                strm.msg = Some(small_msg("invalid bit length repeat"));
                                state.mode = BAD;
                                break;
                            }
                            len = state.lens[state.have as usize - 1];
                            copy = 3 + r.low_bits(2);
                            r.drop_bits(2);
                        } else if here.val == 17 {
                            if !r.need_bits(u32::from(here.bits) + 3) {
                                break 'inf_leave;
                            }
                            r.drop_bits(u32::from(here.bits));
                            len = 0;
                            copy = 3 + r.low_bits(3);
                            r.drop_bits(3);
                        } else {
                            if !r.need_bits(u32::from(here.bits) + 7) {
                                break 'inf_leave;
                            }
                            r.drop_bits(u32::from(here.bits));
                            len = 0;
                            copy = 11 + r.low_bits(7);
                            r.drop_bits(7);
                        }
                        if state.have + copy > state.nlen + state.ndist {
                            strm.msg = Some(small_msg("invalid bit length repeat"));
                            state.mode = BAD;
                            break;
                        }
                        let have = state.have as usize;
                        state.lens[have..have + copy as usize].fill(len);
                        state.have += copy;
                    }
                }

                // handle error breaks in while
                if state.mode == BAD {
                    continue;
                }

                // check for end-of-block code (better have one)
                if state.lens[256] == 0 {
                    strm.msg = Some(small_msg("invalid code -- missing end-of-block"));
                    state.mode = BAD;
                    continue;
                }

                // build code tables -- note: do not change the lenbits or distbits values
                // here (9 and 6) without reading the comments in inftrees.rs concerning the
                // ENOUGH constants, which depend on those values
                let (nlen, ndist) = (state.nlen as usize, state.ndist as usize);
                state.next = 0;
                state.lencode = CodeTable::Codes(0);
                state.lenbits = 9;
                match inflate_table(
                    CodeType::LENS,
                    &state.lens[..nlen],
                    &mut state.codes,
                    state.next,
                    state.lenbits,
                    &mut state.work,
                ) {
                    Ok((next, bits)) => (state.next, state.lenbits) = (next, bits),
                    Err(_) => {
                        strm.msg = Some(small_msg("invalid literal/lengths set"));
                        state.mode = BAD;
                        continue;
                    }
                }
                state.distcode = CodeTable::Codes(state.next);
                state.distbits = 6;
                match inflate_table(
                    CodeType::DISTS,
                    &state.lens[nlen..nlen + ndist],
                    &mut state.codes,
                    state.next,
                    state.distbits,
                    &mut state.work,
                ) {
                    Ok((next, bits)) => (state.next, state.distbits) = (next, bits),
                    Err(_) => {
                        strm.msg = Some(small_msg("invalid distances set"));
                        state.mode = BAD;
                        continue;
                    }
                }
                state.mode = LEN_;
                if flush == Flush::Trees {
                    break 'inf_leave;
                }
            }
            LEN_ => state.mode = LEN,
            LEN => {
                // SLOW (the kernel build) never takes this path
                if !slow && r.have() >= 6 && r.left() >= 258 {
                    inflate_fast(strm, state, &mut r, out, false);
                    if state.mode == TYPE {
                        state.back = -1;
                    }
                    continue;
                }
                state.back = 0;
                let mut here = loop {
                    let here = state.table(state.lencode)[r.low_bits(state.lenbits) as usize];
                    if u32::from(here.bits) <= r.bits {
                        break here;
                    }
                    if !r.pull_byte() {
                        break 'inf_leave;
                    }
                };
                if here.op != 0 && here.op & 0xf0 == 0 {
                    let last = here;
                    let (lbits, lop) = (u32::from(last.bits), u32::from(last.op));
                    here = loop {
                        let idx = last.val as usize + (r.low_bits(lbits + lop) >> lbits) as usize;
                        let here = state.table(state.lencode)[idx];
                        if lbits + u32::from(here.bits) <= r.bits {
                            break here;
                        }
                        if !r.pull_byte() {
                            break 'inf_leave;
                        }
                    };
                    r.drop_bits(lbits);
                    state.back += lbits as i32;
                }
                r.drop_bits(u32::from(here.bits));
                state.back += i32::from(here.bits);
                state.length = u32::from(here.val);
                if here.op == 0 {
                    state.mode = LIT;
                    continue;
                }
                if here.op & 32 != 0 {
                    state.back = -1;
                    state.mode = TYPE;
                    continue;
                }
                if here.op & 64 != 0 {
                    strm.msg = Some(small_msg("invalid literal/length code"));
                    state.mode = BAD;
                    continue;
                }
                state.extra = u32::from(here.op) & 15;
                state.mode = LENEXT;
            }
            LENEXT => {
                if state.extra != 0 {
                    if !r.need_bits(state.extra) {
                        break 'inf_leave;
                    }
                    state.length += r.low_bits(state.extra);
                    r.drop_bits(state.extra);
                    state.back += state.extra as i32;
                }
                state.was = state.length;
                state.mode = DIST;
            }
            DIST => {
                let mut here = loop {
                    let here = state.table(state.distcode)[r.low_bits(state.distbits) as usize];
                    if u32::from(here.bits) <= r.bits {
                        break here;
                    }
                    if !r.pull_byte() {
                        break 'inf_leave;
                    }
                };
                if here.op & 0xf0 == 0 {
                    let last = here;
                    let (lbits, lop) = (u32::from(last.bits), u32::from(last.op));
                    here = loop {
                        let idx = last.val as usize + (r.low_bits(lbits + lop) >> lbits) as usize;
                        let here = state.table(state.distcode)[idx];
                        if lbits + u32::from(here.bits) <= r.bits {
                            break here;
                        }
                        if !r.pull_byte() {
                            break 'inf_leave;
                        }
                    };
                    r.drop_bits(lbits);
                    state.back += lbits as i32;
                }
                r.drop_bits(u32::from(here.bits));
                state.back += i32::from(here.bits);
                if here.op & 64 != 0 {
                    strm.msg = Some(small_msg("invalid distance code"));
                    state.mode = BAD;
                    continue;
                }
                state.offset = u32::from(here.val);
                state.extra = u32::from(here.op) & 15;
                state.mode = DISTEXT;
            }
            DISTEXT => {
                if state.extra != 0 {
                    if !r.need_bits(state.extra) {
                        break 'inf_leave;
                    }
                    state.offset += r.low_bits(state.extra);
                    r.drop_bits(state.extra);
                    state.back += state.extra as i32;
                }
                // INFLATE_STRICT is not defined: no check against dmax
                state.mode = MATCH;
            }
            MATCH => {
                if r.left() == 0 {
                    break 'inf_leave;
                }
                let mut copy = out - r.left(); // output written in this call
                if state.offset as usize > copy {
                    // copy from window
                    copy = state.offset as usize - copy;
                    if copy > state.whave && state.sane {
                        strm.msg = Some(small_msg("invalid distance too far back"));
                        state.mode = BAD;
                        continue;
                    }
                    // (INFLATE_ALLOW_INVALID_DISTANCE_TOOFAR_ARRR is not defined, and sane
                    // is always true without it)
                    let (wsize, wnext) = (state.wsize, state.wnext);
                    let from = if copy > wnext {
                        copy -= wnext;
                        wsize - copy
                    } else {
                        wnext - copy
                    };
                    let copy = copy.min(state.length as usize).min(r.left());
                    let window = state.window.as_deref().unwrap_or(&[]);
                    r.output[r.put..r.put + copy].copy_from_slice(&window[from..from + copy]);
                    r.put += copy;
                    state.length -= copy as u32;
                } else {
                    // copy from output
                    let from = r.put - state.offset as usize;
                    let copy = (state.length as usize).min(r.left());
                    copy_forward(r.output, &mut r.put, from, copy);
                    state.length -= copy as u32;
                }
                if state.length == 0 {
                    state.mode = LEN;
                }
            }
            LIT => {
                if r.left() == 0 {
                    break 'inf_leave;
                }
                r.output[r.put] = state.length as u8;
                r.put += 1;
                state.mode = LEN;
            }
            CHECK => {
                if state.wrap != 0 {
                    if !r.need_bits(32) {
                        break 'inf_leave;
                    }
                    out -= r.left();
                    strm.total_out += out as u64;
                    state.total += out as u64;
                    if state.wrap & 4 != 0 && out != 0 {
                        // UPDATE_CHECK: adler32() only, GUNZIP is not defined
                        state.check = adler32(state.check, Some(&r.output[r.put - out..r.put]));
                        strm.adler = state.check;
                    }
                    out = r.left();
                    if state.wrap & 4 != 0 && (r.hold as u32).swap_bytes() != state.check {
                        strm.msg = Some(small_msg("incorrect data check"));
                        state.mode = BAD;
                        continue;
                    }
                    r.init_bits();
                }
                // GUNZIP is not defined: no LENGTH (gzip trailer) state
                state.mode = DONE;
            }
            DONE => {
                ret = Ok(ZStatus::StreamEnd);
                break 'inf_leave;
            }
            BAD => {
                ret = Err(ZError::Data);
                break 'inf_leave;
            }
            MEM => {
                r.restore(strm, state);
                return Err(ZError::Mem);
            }
            // SYNC, and the gzip modes (FLAGS .. HCRC, LENGTH), which have no case of their
            // own when GUNZIP is not defined
            SYNC | FLAGS | TIME | OS | EXLEN | EXTRA | NAME | COMMENT | HCRC | LENGTH => {
                r.restore(strm, state);
                return Err(ZError::Stream);
            }
        }
    }

    // inf_leave: return from inflate(), updating the total counts and the check value. If
    // there was no progress during the inflate() call, return a buffer error. Call
    // updatewindow() to create and/or update the window state. Note: a memory error from
    // inflate() is non-recoverable.
    let written = out - r.left();
    if (state.wsize != 0
        || (written != 0 && state.mode < BAD && (state.mode < CHECK || flush != Flush::Finish)))
        && updatewindow(state, &r.output[r.put - written..r.put]).is_err()
    {
        state.mode = MEM;
        r.restore(strm, state);
        return Err(ZError::Mem);
    }
    let in_ = in_ - r.have();
    let out = out - r.left();
    strm.total_in += in_ as u64;
    strm.total_out += out as u64;
    state.total += out as u64;
    if state.wrap & 4 != 0 && out != 0 {
        state.check = adler32(state.check, Some(&r.output[r.put - out..r.put]));
        strm.adler = state.check;
    }
    r.restore(strm, state);
    strm.data_type = state.bits as i32
        + if state.last { 64 } else { 0 }
        + if state.mode == TYPE { 128 } else { 0 }
        + if state.mode == LEN_ || state.mode == COPY_ {
            256
        } else {
            0
        };
    if ((in_ == 0 && out == 0) || flush == Flush::Finish) && ret == Ok(ZStatus::Ok) {
        ret = Err(ZError::Buf);
    }
    ret
}

/// `inflateEnd`: free the stream's inflate state (and window). `Err(ZError::Stream)` without
/// an inflate state.
pub fn inflateEnd(strm: &mut ZStream<'_>) -> Result<(), ZError> {
    if inflateStateCheck(strm) {
        return Err(ZError::Stream);
    }
    zcfree(core::mem::take(&mut strm.state));
    Ok(())
}

/// `inflateGetDictionary`: copy the sliding window's contents, oldest first, to
/// `dictionary` (which needs room for up to `1 << windowBits` bytes; `None` to only get the
/// length) and return their length (the C's `dictLength`). `Err(ZError::Buf)` if
/// `dictionary` is too short, `Err(ZError::Stream)` without an inflate state.
pub fn inflateGetDictionary(
    strm: &mut ZStream<'_>,
    dictionary: Option<&mut [u8]>,
) -> Result<usize, ZError> {
    with_state(strm, |_, state| {
        // copy dictionary
        let (whave, wnext) = (state.whave, state.wnext);
        if whave != 0
            && let Some(dictionary) = dictionary
        {
            if dictionary.len() < whave {
                return Err(ZError::Buf);
            }
            let window = state.window.as_deref().unwrap_or(&[]);
            dictionary[..whave - wnext].copy_from_slice(&window[wnext..whave]);
            dictionary[whave - wnext..whave].copy_from_slice(&window[..wnext]);
        }
        Ok(whave)
    })
}

/// `inflateSetDictionary`: give the decompressor the preset dictionary, after `inflate()`
/// returned `Z_NEED_DICT` for a zlib stream (the dictionary's Adler-32 must be the id in the
/// header), or at any time for a raw stream. Fails with `ZError::Data` for the wrong
/// dictionary, `ZError::Stream` at the wrong moment or without an inflate state, `ZError::Mem`
/// if the window cannot be allocated.
pub fn inflateSetDictionary(strm: &mut ZStream<'_>, dictionary: &[u8]) -> Result<(), ZError> {
    with_state(strm, |_, state| {
        if state.wrap != 0 && state.mode != InflateMode::DICT {
            return Err(ZError::Stream);
        }

        // check for correct dictionary identifier
        if state.mode == InflateMode::DICT {
            let dictid = adler32(adler32(0, None), Some(dictionary));
            if dictid != state.check {
                return Err(ZError::Data);
            }
        }

        // copy dictionary to window using updatewindow(), which will amend the existing
        // dictionary if appropriate
        if let Err(err) = updatewindow(state, dictionary) {
            state.mode = InflateMode::MEM;
            return Err(err);
        }
        state.havedict = true;
        Ok(())
    })
}

/// `inflateGetHeader`: ask for the gzip header to be stored in `head`. Only a gzip stream
/// has one, and this zlib is built without gzip decoding, so it always fails with
/// `ZError::Stream`, as the C build does.
pub fn inflateGetHeader(strm: &mut ZStream<'_>, head: &mut GzHeader) -> Result<(), ZError> {
    with_state(strm, |_, state| {
        if state.wrap & 2 == 0 {
            return Err(ZError::Stream);
        }
        // Not reached in this build (wrap never has bit 1 without GUNZIP); the C would keep
        // `head` in the state to fill it while decoding a gzip header.
        head.done = 0;
        Ok(())
    })
}

/// `syncsearch`: search `buf` for the pattern 0, 0, 0xff, 0xff. `have` is the number of
/// pattern bytes found so far (0..3; 0 for the first call). Returns the new count and the
/// number of bytes read: if the count is 4, the pattern was found and the bytes read include
/// the pattern's last byte; otherwise all of `buf` was read, and the search can go on with
/// more data and the returned count.
fn syncsearch(have: u32, buf: &[u8]) -> (u32, usize) {
    let mut got = have;
    let mut next = 0;
    while next < buf.len() && got < 4 {
        if u32::from(buf[next]) == if got < 2 { 0 } else { 0xff } {
            got += 1;
        } else if buf[next] != 0 {
            got = 0;
        } else {
            got = 4 - got;
        }
        next += 1;
    }
    (got, next)
}

/// `inflateSync`: skip input until the 0, 0, 0xff, 0xff marker of a full flush point (an
/// empty stored block), then get ready to inflate the block after it. Returns `Z_OK` when
/// found (the check value is not verified from then on); fails with `ZError::Data` when the
/// input ran out first (call again with more), `ZError::Buf` with no input, `ZError::Stream`
/// without an inflate state. `total_in` counts the skipped bytes.
pub fn inflateSync(strm: &mut ZStream<'_>) -> Result<(), ZError> {
    with_state(strm, |strm, state| {
        if strm.avail_in() == 0 && state.bits < 8 {
            return Err(ZError::Buf);
        }

        // if first time, start search in bit buffer
        if state.mode != InflateMode::SYNC {
            state.mode = InflateMode::SYNC;
            state.hold >>= state.bits & 7;
            state.bits -= state.bits & 7;
            let mut buf = [0u8; 4]; // to restore bit buffer to byte string
            let mut len = 0;
            while state.bits >= 8 {
                buf[len] = state.hold as u8;
                len += 1;
                state.hold >>= 8;
                state.bits -= 8;
            }
            state.have = syncsearch(0, &buf[..len]).0;
        }

        // search available input
        let (have, len) = syncsearch(state.have, strm.next_in);
        state.have = have;
        strm.take_in(len);
        strm.total_in += len as u64;

        // return no joy or set up to restart inflate() on a new block
        if state.have != 4 {
            return Err(ZError::Data);
        }
        if state.flags == -1 {
            state.wrap = 0; // if no header yet, treat as raw
        } else {
            state.wrap &= !4; // no point in computing a check value now
        }
        let flags = state.flags;
        let (in_, out) = (strm.total_in, strm.total_out);
        reset(strm, state);
        strm.total_in = in_;
        strm.total_out = out;
        state.flags = flags;
        state.mode = InflateMode::TYPE;
        Ok(())
    })
}

/// `inflateSyncPoint`: true (the C's 1) if inflate is at the end of a block made by
/// `Z_SYNC_FLUSH` or `Z_FULL_FLUSH`, waiting for the length bytes of the empty stored block.
/// One PPP implementation uses it: PPP flushes with `Z_SYNC_FLUSH` but drops those length
/// bytes, and checks at the end of a packet that inflate is waiting for them.
/// `Err(ZError::Stream)` without an inflate state.
pub fn inflateSyncPoint(strm: &mut ZStream<'_>) -> Result<bool, ZError> {
    with_state(strm, |_, state| {
        Ok(state.mode == InflateMode::STORED && state.bits == 0)
    })
}

/// `inflateCopy`: make `dest` a copy of `source`, inflate state and window included. `dest`
/// gets `source`'s input and counters but an empty `next_out` (an output buffer cannot be
/// shared); its previous state, if any, is dropped. Fails with `ZError::Mem` or
/// `ZError::Stream` (no inflate state in `source`).
pub fn inflateCopy<'a>(dest: &mut ZStream<'a>, source: &ZStream<'a>) -> Result<(), ZError> {
    // check input
    let InternalState::Inflate(state) = &source.state else {
        return Err(ZError::Stream);
    };

    // allocate space and copy state
    let copy = state.try_clone().ok_or(ZError::Mem)?;
    dest.next_in = source.next_in;
    dest.total_in = source.total_in;
    dest.next_out = &mut [];
    dest.total_out = source.total_out;
    dest.msg = source.msg;
    dest.data_type = source.data_type;
    dest.adler = source.adler;
    dest.state = InternalState::Inflate(copy);
    Ok(())
}

/// `inflateUndermine`: allow (`subvert`) distances too far back, filling with zeros. Only
/// `INFLATE_ALLOW_INVALID_DISTANCE_TOOFAR_ARRR` builds allow it, and the kernel's is not one:
/// it fails with `ZError::Data` and keeps the check (`ZError::Stream` without an inflate
/// state).
pub fn inflateUndermine(strm: &mut ZStream<'_>, subvert: bool) -> Result<(), ZError> {
    with_state(strm, |_, state| {
        let _ = subvert;
        state.sane = true;
        Err(ZError::Data)
    })
}

/// `inflateValidate`: check (`check`) or ignore the zlib stream's Adler-32 trailer.
/// `Err(ZError::Stream)` without an inflate state.
pub fn inflateValidate(strm: &mut ZStream<'_>, check: bool) -> Result<(), ZError> {
    with_state(strm, |_, state| {
        if check && state.wrap != 0 {
            state.wrap |= 4;
        } else {
            state.wrap &= !4;
        }
        Ok(())
    })
}

/// `inflateMark`: where decoding stands, for random access: the upper bits are the number of
/// bits back from `next_in` where the current code starts (-1 between blocks and codes, so
/// the value is negative), the low 16 bits the bytes still to copy of a stored block or the
/// bytes already copied of a match. `Err(ZError::Stream)` without an inflate state (the C
/// returns `-(1 << 16)`).
pub fn inflateMark(strm: &mut ZStream<'_>) -> Result<i64, ZError> {
    with_state(strm, |_, state| {
        let low = match state.mode {
            InflateMode::COPY => state.length,
            InflateMode::MATCH => state.was - state.length,
            _ => 0,
        };
        // back is -1 or a small bit count: the shift cannot overflow
        Ok((i64::from(state.back) << 16) + i64::from(low))
    })
}

/// `inflateCodesUsed`: the number of entries of the code space used by the current dynamic
/// block's tables (to check `ENOUGH`). `Err(ZError::Stream)` without an inflate state (the C
/// returns `(unsigned long)-1`).
pub fn inflateCodesUsed(strm: &mut ZStream<'_>) -> Result<usize, ZError> {
    with_state(strm, |_, state| Ok(state.next))
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests of `inflate.rs` (and `inffast.rs`, which every decoding test runs too: each stream is
    // decoded with `SLOW`, the kernel's configuration, and without it).
    //
    // The streams under `sys/lib/libz/testdata/inflate_*` were made by Python's zlib (1.2.12)
    // with `sys/lib/libz/testdata/gen_inflate.py`; to regenerate them run
    // `/usr/bin/python3 sys/lib/libz/testdata/gen_inflate.py` from the repository root. The
    // RFC 1951 streams of the error and corner-case tests are built here, bit by bit.

    use super::*;
    use crate::adler32::adler32;
    use crate::zconf::MAX_WBITS;
    use crate::zlib::*;
    use std::vec;
    use std::vec::Vec;

    macro_rules! testdata {
        ($name:literal) => {
            include_bytes!(concat!("testdata/inflate_", $name))
        };
    }

    static CORPUS: &[u8] = testdata!("corpus.bin");
    static SMALL_CORPUS: &[u8] = testdata!("small.bin");

    /// Both decoders: `SLOW` (no `inflate_fast`) and fast.
    const BOTH: [bool; 2] = [true, false];

    /// The result of decoding a stream.
    struct Decoded {
        ret: Result<ZStatus, ZError>,
        out: Vec<u8>,
        total_in: u64,
        adler: u32,
        msg: Option<&'static str>,
    }

    /// Run `inflate_impl` on `strm` over `input`, giving it at most `in_chunk` new input bytes
    /// and `out_chunk` bytes of output space at a time, until it stops making progress or
    /// returns something other than `Ok(ZStatus::Ok)` (or `Err(ZError::Buf)` while there is more to give).
    /// The buffers live in a temporary stream that lends `strm`'s state.
    fn run(
        strm: &mut ZStream<'_>,
        input: &[u8],
        slow: bool,
        in_chunk: usize,
        out_chunk: usize,
        flush: Flush,
    ) -> Decoded {
        let mut buf = vec![0u8; 1 << 17];
        let mut s = ZStream::new();
        s.state = core::mem::take(&mut strm.state);
        s.total_in = strm.total_in;
        s.total_out = strm.total_out;
        s.adler = strm.adler;
        let start_out = s.total_out;
        let start_in = s.total_in;
        let mut input_rest = input;
        let mut out_rest: &mut [u8] = &mut buf;
        let mut ret;
        loop {
            if s.avail_in() == 0 && !input_rest.is_empty() {
                let n = in_chunk.min(input_rest.len());
                s.next_in = &input_rest[..n];
                input_rest = &input_rest[n..];
            }
            if s.avail_out() == 0 {
                let n = out_chunk.min(out_rest.len());
                let (head, tail) = core::mem::take(&mut out_rest).split_at_mut(n);
                s.next_out = head;
                out_rest = tail;
            }
            ret = inflate_impl(&mut s, flush, slow);
            let more = (s.avail_in() == 0 && !input_rest.is_empty())
                || (s.avail_out() == 0 && !out_rest.is_empty());
            if !(ret == Ok(ZStatus::Ok) || (ret == Err(ZError::Buf) && more)) {
                break;
            }
        }
        let written = (s.total_out - start_out) as usize;
        let unused_in = s.avail_in() + input_rest.len();
        strm.state = core::mem::take(&mut s.state);
        strm.total_in = s.total_in;
        strm.total_out = s.total_out;
        strm.adler = s.adler;
        strm.msg = s.msg;
        strm.data_type = s.data_type;
        let (total_in, adler, msg) = (s.total_in, s.adler, s.msg);
        let d = Decoded {
            ret,
            out: buf[..written].to_vec(),
            total_in,
            adler,
            msg,
        };
        assert_eq!((d.total_in - start_in) as usize, input.len() - unused_in);
        d
    }

    /// Decode `input` with a new stream of `windowBits` `wbits`.
    fn decode(
        input: &[u8],
        wbits: i32,
        slow: bool,
        in_chunk: usize,
        out_chunk: usize,
        flush: Flush,
    ) -> Decoded {
        let mut strm = ZStream::new();
        assert_eq!(inflateInit2(&mut strm, wbits), Ok(()));
        let d = run(&mut strm, input, slow, in_chunk, out_chunk, flush);
        assert_eq!(inflateEnd(&mut strm), Ok(()));
        d
    }

    /// Decode `input` in one call per buffer (all input, 128K of output).
    fn decode_all(input: &[u8], wbits: i32, slow: bool) -> Decoded {
        decode(input, wbits, slow, usize::MAX, usize::MAX, Flush::NoFlush)
    }

    /// Check that `input` decodes to `want`, with an Adler-32 check if it is a zlib stream.
    fn assert_decodes(input: &[u8], wbits: i32, want: &[u8]) {
        for slow in BOTH {
            let d = decode_all(input, wbits, slow);
            assert_eq!(d.ret, Ok(ZStatus::StreamEnd), "slow={slow} msg={:?}", d.msg);
            assert!(d.out == want, "slow={slow}: output differs");
            assert_eq!(d.total_in as usize, input.len());
            if wbits >= 0 {
                assert_eq!(d.adler, adler32(1, Some(want)));
            }
        }
    }

    /// Bits written first-bit-in-the-low-bit, as deflate packs them.
    struct Bits {
        out: Vec<u8>,
        acc: u32,
        n: u32,
    }

    impl Bits {
        fn new() -> Self {
            Self {
                out: Vec::new(),
                acc: 0,
                n: 0,
            }
        }

        fn bit(&mut self, b: u32) {
            self.acc |= (b & 1) << self.n;
            self.n += 1;
            if self.n == 8 {
                self.out.push(self.acc as u8);
                self.acc = 0;
                self.n = 0;
            }
        }

        /// A value of `n` bits, low bit first (header fields, extra bits).
        fn put(&mut self, v: u32, n: u32) {
            for i in 0..n {
                self.bit(v >> i);
            }
        }

        /// A Huffman code of `len` bits, high bit first.
        fn huff(&mut self, code: u32, len: u32) {
            for i in (0..len).rev() {
                self.bit(code >> i);
            }
        }

        /// A fixed-code literal/length symbol.
        fn fixed(&mut self, sym: u32) {
            match sym {
                0..=143 => self.huff(0x30 + sym, 8),
                144..=255 => self.huff(0x190 + sym - 144, 9),
                256..=279 => self.huff(sym - 256, 7),
                _ => self.huff(0xc0 + sym - 280, 8),
            }
        }

        /// Pad to a byte boundary.
        fn align(&mut self) {
            while self.n != 0 {
                self.bit(0);
            }
        }

        fn bytes(&mut self, b: &[u8]) {
            self.align();
            self.out.extend_from_slice(b);
        }

        fn finish(mut self) -> Vec<u8> {
            self.align();
            self.out
        }
    }

    /// A raw stored block (final) with `data`.
    fn stored(data: &[u8]) -> Vec<u8> {
        let mut b = Bits::new();
        b.put(1, 1);
        b.put(0, 2);
        let len = data.len() as u16;
        b.bytes(&len.to_le_bytes());
        b.bytes(&(!len).to_le_bytes());
        b.bytes(data);
        b.finish()
    }

    /// "aaaa" as a raw fixed block: the literal 'a' and a match of length 3, distance 1.
    fn fixed_aaaa() -> Vec<u8> {
        let mut b = Bits::new();
        b.put(1, 1); // BFINAL
        b.put(1, 2); // BTYPE fixed
        b.fixed(u32::from(b'a'));
        b.fixed(257); // length 3
        b.huff(0, 5); // distance code 0: distance 1
        b.fixed(256);
        b.finish()
    }

    /// A two-byte zlib header with compression method/info `cmf`, made to pass the check.
    fn zlib_header(cmf: u8) -> [u8; 2] {
        let flg = (31 - (u32::from(cmf) * 256) % 31) % 31;
        [cmf, flg as u8]
    }

    /// `raw` wrapped in a zlib header and an Adler-32 trailer of `data`.
    fn zlib_wrap(raw: &[u8], data: &[u8]) -> Vec<u8> {
        let mut v = zlib_header(0x78).to_vec();
        v.extend_from_slice(raw);
        v.extend_from_slice(&adler32(1, Some(data)).to_be_bytes());
        v
    }

    #[test]
    fn python_levels() {
        let streams: [&[u8]; 10] = [
            testdata!("level0.z"),
            testdata!("level1.z"),
            testdata!("level2.z"),
            testdata!("level3.z"),
            testdata!("level4.z"),
            testdata!("level5.z"),
            testdata!("level6.z"),
            testdata!("level7.z"),
            testdata!("level8.z"),
            testdata!("level9.z"),
        ];
        for s in streams {
            assert_decodes(s, 15, SMALL_CORPUS);
            assert_decodes(s, 0, SMALL_CORPUS);
        }
    }

    #[test]
    fn python_strategies_and_big_streams() {
        let streams: [&[u8]; 6] = [
            testdata!("big1.z"),
            testdata!("big6.z"),
            testdata!("filtered.z"),
            testdata!("huffman.z"),
            testdata!("rle.z"),
            testdata!("fixed.z"),
        ];
        for s in streams {
            assert_decodes(s, MAX_WBITS, CORPUS);
        }
    }

    #[test]
    fn python_window_sizes() {
        let streams: [(i32, &[u8]); 7] = [
            (9, testdata!("wbits9.z")),
            (10, testdata!("wbits10.z")),
            (11, testdata!("wbits11.z")),
            (12, testdata!("wbits12.z")),
            (13, testdata!("wbits13.z")),
            (14, testdata!("wbits14.z")),
            (15, testdata!("wbits15.z")),
        ];
        for (wbits, s) in streams {
            assert_decodes(s, wbits, CORPUS);
            assert_decodes(s, 15, CORPUS);
            assert_decodes(s, 0, CORPUS);
            // a window smaller than the header asks for is refused
            if wbits > 9 {
                for slow in BOTH {
                    let d = decode_all(s, wbits - 1, slow);
                    assert_eq!((d.ret, d.msg), (Err(ZError::Data), Some("error")));
                    assert!(d.out.is_empty());
                }
            }
        }
    }

    #[test]
    fn python_raw_streams() {
        assert_decodes(testdata!("raw15.z"), -15, CORPUS);
        assert_decodes(testdata!("raw12.z"), -12, CORPUS);
        assert_decodes(testdata!("raw12.z"), -15, CORPUS);
    }

    #[test]
    fn the_window_limits_the_distance() {
        // 5000 stored bytes, then a match 4500 back (distance code 24: 4097 + 11 extra bits)
        let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 + i / 251) as u8).collect();
        let mut s = stored(&data);
        s[0] = 0; // not last
        let mut b = Bits::new();
        b.put(1, 1);
        b.put(1, 2);
        b.fixed(257);
        b.huff(24, 5);
        b.put(4500 - 4097, 11);
        b.fixed(256);
        s.extend_from_slice(&b.finish());
        s.extend_from_slice(&[0; 8]); // so that inflate_fast may run
        let mut want = data.clone();
        want.extend_from_slice(&data[500..503]);
        for slow in BOTH {
            // one call: the match is in the output, whatever the window size
            assert_eq!(decode_all(&s, -12, slow).out, want);
            // two calls: the match must come from the window, and 4K is not enough
            let d = decode(&s, -15, slow, usize::MAX, 5000, Flush::NoFlush);
            assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
            assert!(d.out == want);
            let d = decode(&s, -12, slow, usize::MAX, 5000, Flush::NoFlush);
            assert_eq!((d.ret, d.msg), (Err(ZError::Data), Some("error")));
            assert!(d.out == data);
        }
    }

    #[test]
    fn one_byte_at_a_time() {
        for (s, wbits, want) in [
            (&testdata!("big6.z")[..], 15, CORPUS),
            (&testdata!("level0.z")[..], 15, SMALL_CORPUS),
            (&testdata!("fixed.z")[..], 15, CORPUS),
            (&testdata!("raw12.z")[..], -12, CORPUS),
        ] {
            for slow in BOTH {
                let d = decode(s, wbits, slow, 1, 1, Flush::NoFlush);
                assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
                assert!(d.out == want);
            }
        }
    }

    #[test]
    fn assorted_buffer_sizes() {
        let s = testdata!("huffman.z");
        for (in_chunk, out_chunk) in [
            (7, 13),
            (100, 300),
            (6, 258),
            (4096, 1000),
            (1, 40000),
            (40000, 1),
        ] {
            for slow in BOTH {
                let d = decode(s, 15, slow, in_chunk, out_chunk, Flush::SyncFlush);
                assert_eq!(
                    d.ret,
                    Ok(ZStatus::StreamEnd),
                    "{in_chunk}/{out_chunk} slow={slow}"
                );
                assert!(d.out == CORPUS, "{in_chunk}/{out_chunk} slow={slow}");
            }
        }
    }

    #[test]
    fn z_finish_without_a_window() {
        for slow in BOTH {
            let mut strm = ZStream::new();
            assert_eq!(inflateInit(&mut strm), Ok(()));
            let d = run(
                &mut strm,
                testdata!("big6.z"),
                slow,
                usize::MAX,
                usize::MAX,
                Flush::Finish,
            );
            assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
            assert!(d.out == CORPUS);
            // one call that reached the end with Flush::Finish never needed a window
            let InternalState::Inflate(state) = &strm.state else {
                panic!("no inflate state")
            };
            assert!(state.window.is_none());
            assert_eq!(inflateGetDictionary(&mut strm, None), Ok(0));
            assert_eq!(inflateEnd(&mut strm), Ok(()));
        }
    }

    /// The IPComp path (`xform_ipcomp.c`): raw deflate, `Flush::PartialFlush`, fresh output buffers
    /// whenever one is full (`Ok(ZStatus::Ok)` with `avail_out == 0`), until `Ok(ZStatus::StreamEnd)`.
    #[test]
    fn ipcomp_raw_partial_flush_with_fresh_buffers() {
        let input = testdata!("raw15.z");
        let mut bufs: Vec<Vec<u8>> = (0..64).map(|_| vec![0u8; 1000]).collect();
        let mut used = Vec::new();
        let mut strm = ZStream::new();
        assert_eq!(inflateInit2(&mut strm, -MAX_WBITS), Ok(()));
        strm.next_in = input;
        let mut bufs_iter = bufs.iter_mut();
        let mut full = 0;
        let ret = loop {
            let Some(buf) = bufs_iter.next() else {
                panic!("out of buffers")
            };
            strm.next_out = buf;
            let ret = inflate(&mut strm, Flush::PartialFlush);
            used.push(1000 - strm.avail_out());
            if ret == Ok(ZStatus::StreamEnd) {
                break ret;
            }
            assert_eq!(ret, Ok(ZStatus::Ok));
            // "give me more space"
            assert_eq!(strm.avail_out(), 0);
            full += 1;
        };
        assert_eq!(ret, Ok(ZStatus::StreamEnd));
        assert_eq!(strm.avail_in(), 0);
        assert_eq!(strm.total_in as usize, input.len());
        assert_eq!(strm.total_out as usize, CORPUS.len());
        assert_eq!(inflateEnd(&mut strm), Ok(()));
        assert_eq!(full, CORPUS.len() / 1000);
        let out: Vec<u8> = bufs
            .iter()
            .zip(&used)
            .flat_map(|(b, &n)| b[..n].iter().copied())
            .collect();
        assert!(out == CORPUS);
    }

    #[test]
    fn preset_dictionary() {
        let dict = testdata!("dict.bin");
        let input = testdata!("dict.z");
        for slow in BOTH {
            let mut buf = vec![0u8; 8192];
            let mut strm = ZStream::new();
            assert_eq!(inflateInit(&mut strm), Ok(()));
            // not before inflate() asks for it
            assert_eq!(inflateSetDictionary(&mut strm, dict), Err(ZError::Stream));
            strm.next_in = input;
            strm.next_out = &mut buf;
            assert_eq!(
                inflate_impl(&mut strm, Flush::NoFlush, slow),
                Ok(ZStatus::NeedDict)
            );
            assert_eq!(strm.adler, adler32(1, Some(dict)));
            assert_eq!(strm.total_out, 0);
            assert_eq!(
                inflateSetDictionary(&mut strm, &dict[1..]),
                Err(ZError::Data)
            );
            assert_eq!(inflateSetDictionary(&mut strm, dict), Ok(()));
            assert_eq!(
                inflate_impl(&mut strm, Flush::NoFlush, slow),
                Ok(ZStatus::StreamEnd)
            );
            let n = strm.total_out as usize;
            assert_eq!(strm.adler, adler32(1, Some(SMALL_CORPUS)));
            assert_eq!(inflateEnd(&mut strm), Ok(()));
            assert!(buf[..n] == *SMALL_CORPUS);

            // a raw stream takes the dictionary up front
            let mut buf = vec![0u8; 8192];
            let mut strm = ZStream::new();
            assert_eq!(inflateInit2(&mut strm, -15), Ok(()));
            assert_eq!(inflateSetDictionary(&mut strm, dict), Ok(()));
            strm.next_in = testdata!("dict_raw.z");
            strm.next_out = &mut buf;
            assert_eq!(
                inflate_impl(&mut strm, Flush::NoFlush, slow),
                Ok(ZStatus::StreamEnd)
            );
            let n = strm.total_out as usize;
            assert_eq!(inflateEnd(&mut strm), Ok(()));
            assert!(buf[..n] == *SMALL_CORPUS);
        }
    }

    /// The offsets `gen_inflate.py` wrote next to a stream.
    fn offsets(txt: &[u8]) -> Vec<usize> {
        core::str::from_utf8(txt)
            .unwrap()
            .split_whitespace()
            .map(|w| w.parse().unwrap())
            .collect()
    }

    #[test]
    fn sync_recovers_at_a_full_flush_point() {
        let stream = testdata!("fullflush.z");
        let off = offsets(testdata!("fullflush.txt"));
        let (flush1, piece2) = (off[0], off[2]);
        assert_eq!(&stream[flush1 - 4..flush1], &[0, 0, 0xff, 0xff]);
        let mut corrupt = stream.to_vec();
        corrupt[40..60].fill(0x55);
        for slow in BOTH {
            let mut buf = vec![0u8; 1 << 16];
            let buf_len = buf.len();
            let mut strm = ZStream::new();
            assert_eq!(inflateInit(&mut strm), Ok(()));
            // no input: nothing to search
            assert_eq!(inflateSync(&mut strm), Err(ZError::Buf));
            // decode the damaged start; whatever comes out is discarded
            strm.next_in = &corrupt[..flush1 - 100];
            strm.next_out = &mut buf;
            let _ = inflate_impl(&mut strm, Flush::NoFlush, slow);
            // the marker split over two inputs: the search state carries over
            strm.next_in = &corrupt[flush1 - 100..flush1 - 2];
            let before = strm.total_in;
            assert_eq!(inflateSync(&mut strm), Err(ZError::Data));
            assert_eq!(strm.total_in - before, 98);
            strm.next_in = &corrupt[flush1 - 2..];
            assert_eq!(inflateSync(&mut strm), Ok(()));
            assert_eq!(strm.avail_in(), corrupt.len() - flush1);
            let out_before = strm.total_out as usize;
            let start = buf_len - strm.avail_out();
            assert_eq!(
                inflate_impl(&mut strm, Flush::Finish, slow),
                Ok(ZStatus::StreamEnd)
            );
            let end = start + (strm.total_out as usize - out_before);
            drop(strm);
            assert!(buf[start..end] == CORPUS[piece2..]);
        }
    }

    #[test]
    fn sync_flush_point() {
        let stream = testdata!("syncflush.z");
        let off = offsets(testdata!("syncflush.txt"));
        let (marker, piece2) = (off[0], off[1]);
        for slow in BOTH {
            let mut buf = vec![0u8; 8192];
            let mut strm = ZStream::new();
            assert_eq!(inflateInit(&mut strm), Ok(()));
            // everything before the empty stored block's length bytes: all of the first piece
            // comes out, and inflate waits for the lengths (what PPP checks)
            strm.next_in = &stream[..marker];
            strm.next_out = &mut buf;
            assert_eq!(
                inflate_impl(&mut strm, Flush::SyncFlush, slow),
                Ok(ZStatus::Ok)
            );
            assert_eq!(strm.total_out as usize, piece2);
            assert_eq!(inflateSyncPoint(&mut strm), Ok(true));
            strm.next_in = &stream[marker..];
            assert_eq!(inflateSyncPoint(&mut strm), Ok(true));
            assert_eq!(
                inflate_impl(&mut strm, Flush::SyncFlush, slow),
                Ok(ZStatus::StreamEnd)
            );
            assert_eq!(inflateSyncPoint(&mut strm), Ok(false));
            let n = strm.total_out as usize;
            assert_eq!(inflateEnd(&mut strm), Ok(()));
            assert!(buf[..n] == *SMALL_CORPUS);
        }
    }

    #[test]
    fn rfc1951_hand_built_blocks() {
        // a stored block
        let s = stored(b"hello");
        assert_eq!(s, [1, 5, 0, 0xfa, 0xff, b'h', b'e', b'l', b'l', b'o']);
        assert_decodes(&s, -15, b"hello");
        assert_decodes(&zlib_wrap(&s, b"hello"), 15, b"hello");
        // a fixed-Huffman block with a literal and an overlapping match
        assert_decodes(&fixed_aaaa(), -15, b"aaaa");
        assert_decodes(&zlib_wrap(&fixed_aaaa(), b"aaaa"), 15, b"aaaa");
        // an empty final block, fixed and stored
        let mut b = Bits::new();
        b.put(1, 1);
        b.put(1, 2);
        b.fixed(256);
        let empty = b.finish();
        assert_eq!(empty, [3, 0]);
        assert_decodes(&empty, -15, b"");
        assert_decodes(&stored(b""), -15, b"");
        assert_decodes(&zlib_wrap(&empty, b""), 15, b"");
        // a non-final stored block, then a fixed one
        let mut s = stored(b"abc");
        s[0] = 0; // not last
        s.extend_from_slice(&fixed_aaaa());
        assert_decodes(&s, -15, b"abcaaaa");
        // a long fixed block: 300 'x' as a literal and matches of 258 and 41 (codes 285 and 273
        // with three extra bits, 41 = 35 + 6)
        let mut b = Bits::new();
        b.put(1, 1);
        b.put(1, 2);
        b.fixed(u32::from(b'x'));
        b.fixed(285);
        b.huff(0, 5);
        b.fixed(273);
        b.put(6, 3);
        b.huff(0, 5);
        b.fixed(256);
        let mut v = b.finish();
        v.extend_from_slice(&[0; 8]); // trailing bytes, left unused
        for slow in BOTH {
            let d = decode_all(&v, -15, slow);
            assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
            assert!(d.out == [b'x'; 300]);
            assert_eq!(d.total_in as usize, v.len() - 8);
        }
    }

    /// Decode `input` as a whole and expect a data error with the `SMALL` message.
    fn assert_data_error(input: &[u8], wbits: i32) {
        for slow in BOTH {
            let d = decode_all(input, wbits, slow);
            assert_eq!(d.ret, Err(ZError::Data), "slow={slow}");
            assert_eq!(d.msg, Some("error"));
        }
    }

    #[test]
    fn header_errors() {
        // incorrect header check
        assert_data_error(&[0x78, 0x9d, 3, 0], 15);
        // unknown compression method (9)
        let mut v = zlib_header(0x79).to_vec();
        v.extend_from_slice(&[3, 0]);
        assert_data_error(&v, 15);
        // invalid window size (CINFO 8: 64K)
        let mut v = zlib_header(0x88).to_vec();
        v.extend_from_slice(&[3, 0]);
        assert_data_error(&v, 15);
        // a wrong check value, and the same stream when the check is not validated
        let mut v = testdata!("big6.z").to_vec();
        *v.last_mut().unwrap() ^= 1;
        assert_data_error(&v, 15);
        for slow in BOTH {
            let mut strm = ZStream::new();
            assert_eq!(inflateInit(&mut strm), Ok(()));
            assert_eq!(inflateValidate(&mut strm, false), Ok(()));
            let d = run(&mut strm, &v, slow, usize::MAX, usize::MAX, Flush::NoFlush);
            assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
            assert!(d.out == CORPUS);
            assert_eq!(inflateValidate(&mut strm, true), Ok(()));
            assert_eq!(inflateEnd(&mut strm), Ok(()));
        }
    }

    #[test]
    fn block_errors() {
        // invalid block type (3)
        assert_data_error(&[0x07, 0, 0, 0, 0, 0, 0, 0], -15);
        // invalid stored block lengths
        assert_data_error(&[1, 5, 0, 0xfa, 0xfe, 1, 2, 3, 4, 5], -15);
        // a distance too far back: a match before any output (padded for inflate_fast)
        let mut b = Bits::new();
        b.put(1, 1);
        b.put(1, 2);
        b.fixed(257);
        b.huff(0, 5);
        b.fixed(256);
        let mut v = b.finish();
        v.extend_from_slice(&[0; 8]);
        assert_data_error(&v, -15);
        // too many length symbols (HLIT 30: 287)
        let mut b = Bits::new();
        b.put(1, 1);
        b.put(2, 2);
        b.put(30, 5);
        b.put(0, 5);
        b.put(0, 4);
        assert_data_error(&b.finish(), -15);
        // an over-subscribed code length code: four code lengths of one bit
        let mut b = Bits::new();
        b.put(1, 1);
        b.put(2, 2);
        b.put(0, 5);
        b.put(0, 5);
        b.put(0, 4);
        for _ in 0..4 {
            b.put(1, 3);
        }
        assert_data_error(&b.finish(), -15);
        // no end-of-block code: code length codes 0 and 17 (one bit each), every length 0
        let mut b = Bits::new();
        b.put(1, 1);
        b.put(2, 2);
        b.put(0, 5);
        b.put(0, 5);
        b.put(0, 4);
        b.put(0, 3); // 16
        b.put(1, 3); // 17
        b.put(0, 3); // 18
        b.put(1, 3); // 0
        for _ in 0..258 {
            b.huff(0, 1);
        }
        assert_data_error(&b.finish(), -15);
    }

    #[test]
    fn truncated_stream() {
        let s = testdata!("level6.z");
        let cut = &s[..s.len() - 5];
        for slow in BOTH {
            let d = decode(cut, 15, slow, usize::MAX, usize::MAX, Flush::Finish);
            assert_eq!(d.ret, Err(ZError::Buf));
            assert!(SMALL_CORPUS.starts_with(&d.out));
            // without Flush::Finish: Ok(ZStatus::Ok) while it consumes input, Err(ZError::Buf) once stuck
            let mut strm = ZStream::new();
            assert_eq!(inflateInit(&mut strm), Ok(()));
            let mut buf = vec![0u8; 8192];
            strm.next_in = cut;
            strm.next_out = &mut buf;
            assert_eq!(
                inflate_impl(&mut strm, Flush::NoFlush, slow),
                Ok(ZStatus::Ok)
            );
            assert_eq!(
                inflate_impl(&mut strm, Flush::NoFlush, slow),
                Err(ZError::Buf)
            );
            assert_eq!(inflateEnd(&mut strm), Ok(()));
        }
    }

    #[test]
    fn prime_inserts_bits() {
        let s = testdata!("raw15.z");
        for slow in BOTH {
            // the first byte primed, the rest as input
            let mut strm = ZStream::new();
            assert_eq!(inflateInit2(&mut strm, -15), Ok(()));
            assert_eq!(inflatePrime(&mut strm, 8, i32::from(s[0])), Ok(()));
            let d = run(&mut strm, &s[1..], slow, 512, 512, Flush::NoFlush);
            assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
            assert!(d.out == CORPUS);
            // two bytes primed in one call, after clearing a junk prime
            assert_eq!(inflateReset(&mut strm), Ok(()));
            assert_eq!(inflatePrime(&mut strm, 5, 0x1f), Ok(()));
            assert_eq!(inflatePrime(&mut strm, -1, 0), Ok(()));
            assert_eq!(
                inflatePrime(
                    &mut strm,
                    16,
                    i32::from(s[0]) | i32::from(s[1]) << 8 | 0x7fff_0000
                ),
                Ok(())
            );
            let d = run(
                &mut strm,
                &s[2..],
                slow,
                usize::MAX,
                usize::MAX,
                Flush::NoFlush,
            );
            assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
            assert!(d.out == CORPUS);
            // limits: 16 bits a call, 32 in all
            assert_eq!(inflateReset(&mut strm), Ok(()));
            assert_eq!(inflatePrime(&mut strm, 17, 0), Err(ZError::Stream));
            assert_eq!(inflatePrime(&mut strm, 0, 0), Ok(()));
            assert_eq!(inflatePrime(&mut strm, 16, 0), Ok(()));
            assert_eq!(inflatePrime(&mut strm, 16, 0), Ok(()));
            assert_eq!(inflatePrime(&mut strm, 1, 0), Err(ZError::Stream));
            assert_eq!(inflateEnd(&mut strm), Ok(()));
        }
    }

    #[test]
    fn mark_reports_the_position() {
        let mut strm = ZStream::new();
        assert_eq!(inflateMark(&mut strm), Err(ZError::Stream));
        // in a stored block: the bytes left to copy
        let s = stored(b"hello");
        let mut out = [0u8; 8];
        assert_eq!(inflateInit2(&mut strm, -15), Ok(()));
        assert_eq!(inflateMark(&mut strm), Ok(-(1 << 16)));
        strm.next_in = &s;
        strm.next_out = &mut out[..2];
        assert_eq!(inflate(&mut strm, Flush::NoFlush), Ok(ZStatus::Ok));
        assert_eq!(inflateMark(&mut strm), Ok(-(1 << 16) + 3));
        assert_eq!(inflateEnd(&mut strm), Ok(()));

        // in a match: the bits of the length/distance codes back, and the bytes copied so far
        let s = fixed_aaaa();
        let mut out = [0u8; 4];
        let mut strm = ZStream::new();
        assert_eq!(inflateInit2(&mut strm, -15), Ok(()));
        strm.next_in = &s;
        let (o1, rest) = out.split_at_mut(1);
        let (o2, rest) = rest.split_at_mut(1);
        let (o3, o4) = rest.split_at_mut(1);
        strm.next_out = o1;
        assert_eq!(inflate(&mut strm, Flush::NoFlush), Ok(ZStatus::Ok));
        assert_eq!(inflateMark(&mut strm), Ok(12 << 16));
        strm.next_out = o2;
        assert_eq!(inflate(&mut strm, Flush::NoFlush), Ok(ZStatus::Ok));
        assert_eq!(inflateMark(&mut strm), Ok((12 << 16) + 1));
        strm.next_out = o3;
        assert_eq!(inflate(&mut strm, Flush::NoFlush), Ok(ZStatus::Ok));
        assert_eq!(inflateMark(&mut strm), Ok((12 << 16) + 2));
        strm.next_out = o4;
        assert_eq!(inflate(&mut strm, Flush::NoFlush), Ok(ZStatus::StreamEnd));
        assert_eq!(inflateMark(&mut strm), Ok(-(1 << 16)));
        assert_eq!(inflateEnd(&mut strm), Ok(()));
        assert_eq!(&out, b"aaaa");
    }

    #[test]
    fn get_dictionary_returns_the_window() {
        // a zlib stream decoded in one call never fills the window: inflate() resets its output
        // count after the check value, so the end-of-call update sees no output (as in the C)
        let mut strm = ZStream::new();
        assert_eq!(inflateInit(&mut strm), Ok(()));
        let d = run(
            &mut strm,
            testdata!("big6.z"),
            true,
            usize::MAX,
            usize::MAX,
            Flush::NoFlush,
        );
        assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
        assert_eq!(inflateGetDictionary(&mut strm, None), Ok(0));
        assert_eq!(inflateEnd(&mut strm), Ok(()));

        for (s, wbits, out_chunk, slow) in [
            (&testdata!("raw15.z")[..], -15, usize::MAX, true),
            (&testdata!("raw15.z")[..], -15, usize::MAX, false),
            (&testdata!("raw15.z")[..], -15, 1000, true),
            (&testdata!("raw15.z")[..], -15, 777, false),
        ] {
            let mut strm = ZStream::new();
            assert_eq!(inflateInit2(&mut strm, wbits), Ok(()));
            let d = run(&mut strm, s, slow, usize::MAX, out_chunk, Flush::NoFlush);
            assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
            assert_eq!(inflateGetDictionary(&mut strm, None), Ok(32768));
            let mut dict = vec![0u8; 32768];
            assert_eq!(inflateGetDictionary(&mut strm, Some(&mut dict)), Ok(32768));
            assert!(dict == CORPUS[CORPUS.len() - 32768..]);
            let mut short = vec![0u8; 100];
            assert_eq!(
                inflateGetDictionary(&mut strm, Some(&mut short)),
                Err(ZError::Buf)
            );
            assert_eq!(inflateEnd(&mut strm), Ok(()));
        }
    }

    #[test]
    fn copy_continues_independently() {
        let s = testdata!("raw15.z");
        let half = s.len() / 2;
        for slow in BOTH {
            let mut strm = ZStream::new();
            assert_eq!(inflateInit2(&mut strm, -15), Ok(()));
            let first = run(
                &mut strm,
                &s[..half],
                slow,
                usize::MAX,
                usize::MAX,
                Flush::NoFlush,
            );
            assert_eq!(first.ret, Err(ZError::Buf)); // all input used, then no progress
            let mut dest = ZStream::new();
            assert_eq!(inflateCopy(&mut dest, &strm), Ok(()));
            assert_eq!(dest.total_in, strm.total_in);
            assert_eq!(dest.total_out, strm.total_out);
            assert_eq!(dest.avail_out(), 0);
            let a = run(
                &mut strm,
                &s[half..],
                slow,
                usize::MAX,
                usize::MAX,
                Flush::NoFlush,
            );
            let b = run(&mut dest, &s[half..], slow, 100, 100, Flush::NoFlush);
            assert_eq!(
                (a.ret, b.ret),
                (Ok(ZStatus::StreamEnd), Ok(ZStatus::StreamEnd))
            );
            assert!(a.out == b.out);
            assert!([first.out, a.out].concat() == CORPUS);
            assert_eq!(inflateEnd(&mut strm), Ok(()));
            assert_eq!(inflateEnd(&mut dest), Ok(()));
        }
        let empty = ZStream::new();
        let mut dest = ZStream::new();
        assert_eq!(inflateCopy(&mut dest, &empty), Err(ZError::Stream));
    }

    #[test]
    fn reset2_changes_the_window() {
        let mut strm = ZStream::new();
        assert_eq!(inflateInit2(&mut strm, 15), Ok(()));
        let d = run(
            &mut strm,
            testdata!("big6.z"),
            true,
            usize::MAX,
            4096,
            Flush::NoFlush,
        );
        assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
        let window_len = |strm: &ZStream<'_>| match &strm.state {
            InternalState::Inflate(state) => state.window.as_ref().map(Vec::len),
            _ => panic!("no inflate state"),
        };
        assert_eq!(window_len(&strm), Some(32768));
        // same size: the window is kept (inflateReset2 -> inflateReset)
        assert_eq!(inflateReset2(&mut strm, 15), Ok(()));
        assert_eq!(window_len(&strm), Some(32768));
        // a smaller window: freed, then allocated at the new size when needed
        assert_eq!(inflateReset2(&mut strm, 9), Ok(()));
        assert_eq!(window_len(&strm), None);
        let d = run(
            &mut strm,
            testdata!("wbits9.z"),
            true,
            usize::MAX,
            4096,
            Flush::NoFlush,
        );
        assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
        assert!(d.out == CORPUS);
        assert_eq!(window_len(&strm), Some(512));
        // raw
        assert_eq!(inflateReset2(&mut strm, -15), Ok(()));
        let d = run(
            &mut strm,
            testdata!("raw15.z"),
            false,
            usize::MAX,
            4096,
            Flush::NoFlush,
        );
        assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
        assert!(d.out == CORPUS);
        // windowBits 0: the header says
        assert_eq!(inflateReset2(&mut strm, 0), Ok(()));
        let d = run(
            &mut strm,
            testdata!("wbits12.z"),
            true,
            3000,
            3000,
            Flush::NoFlush,
        );
        assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
        assert!(d.out == CORPUS);
        assert_eq!(window_len(&strm), Some(4096));
        // refused: too small, too large, gzip (31) and auto-detect (47) without GUNZIP
        for bad in [7, 16, 31, 47, -7, -16, 1] {
            assert_eq!(inflateReset2(&mut strm, bad), Err(ZError::Stream), "{bad}");
        }
        assert_eq!(inflateEnd(&mut strm), Ok(()));
    }

    #[test]
    fn reset_keep_keeps_the_window() {
        let mut strm = ZStream::new();
        assert_eq!(inflateInit2(&mut strm, -15), Ok(()));
        let d = run(
            &mut strm,
            testdata!("raw15.z"),
            true,
            usize::MAX,
            4096,
            Flush::NoFlush,
        );
        assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
        assert_eq!(inflateResetKeep(&mut strm), Ok(()));
        assert_eq!((strm.total_in, strm.total_out, strm.msg), (0, 0, None));
        assert_eq!(inflateGetDictionary(&mut strm, None), Ok(32768));
        assert_eq!(inflateReset(&mut strm), Ok(()));
        assert_eq!(inflateGetDictionary(&mut strm, None), Ok(0));
        assert_eq!(inflateEnd(&mut strm), Ok(()));
    }

    #[test]
    fn block_flushes_and_data_type() {
        // Flush::Block stops at each block boundary; data_type says where
        let mut s = stored(b"abc");
        s[0] = 0;
        s.extend_from_slice(&fixed_aaaa());
        let mut buf = [0u8; 16];
        let mut strm = ZStream::new();
        assert_eq!(inflateInit2(&mut strm, -15), Ok(()));
        strm.next_in = &s;
        strm.next_out = &mut buf;
        assert_eq!(inflate(&mut strm, Flush::Block), Ok(ZStatus::Ok));
        assert_eq!(strm.total_out, 3);
        assert_eq!(strm.data_type & 128, 128); // at the end of a block
        assert_eq!(strm.data_type & 64, 0); // not the last
        assert_eq!(inflate(&mut strm, Flush::Trees), Ok(ZStatus::Ok));
        assert_eq!(strm.total_out, 3);
        assert_eq!(strm.data_type & 256, 256); // after the fixed block's header
        assert_eq!(strm.data_type & 64, 64); // the last block
        // the last block's end is a block boundary too
        assert_eq!(inflate(&mut strm, Flush::Block), Ok(ZStatus::Ok));
        assert_eq!(strm.total_out, 7);
        assert_eq!(strm.data_type & (64 | 128), 64 | 128);
        assert_eq!(inflate(&mut strm, Flush::Block), Ok(ZStatus::StreamEnd));
        assert_eq!(inflateEnd(&mut strm), Ok(()));
        assert_eq!(&buf[..7], b"abcaaaa");
    }

    #[test]
    fn codes_used_counts_the_dynamic_tables() {
        let mut strm = ZStream::new();
        assert_eq!(inflateCodesUsed(&mut strm), Err(ZError::Stream));
        assert_eq!(inflateInit(&mut strm), Ok(()));
        assert_eq!(inflateCodesUsed(&mut strm), Ok(0));
        let d = run(
            &mut strm,
            testdata!("big6.z"),
            true,
            2000,
            usize::MAX,
            Flush::NoFlush,
        );
        assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
        let used = inflateCodesUsed(&mut strm).unwrap();
        assert!(used > 512 && used <= ENOUGH, "{used}");
        assert_eq!(inflateEnd(&mut strm), Ok(()));
    }

    #[test]
    fn api_misuse() {
        let mut strm = ZStream::new();
        // no state
        assert_eq!(inflate(&mut strm, Flush::NoFlush), Err(ZError::Stream));
        assert_eq!(inflateEnd(&mut strm), Err(ZError::Stream));
        assert_eq!(inflateReset(&mut strm), Err(ZError::Stream));
        assert_eq!(inflateResetKeep(&mut strm), Err(ZError::Stream));
        assert_eq!(inflateReset2(&mut strm, 15), Err(ZError::Stream));
        assert_eq!(inflatePrime(&mut strm, 1, 1), Err(ZError::Stream));
        assert_eq!(inflateSetDictionary(&mut strm, b"x"), Err(ZError::Stream));
        assert_eq!(inflateGetDictionary(&mut strm, None), Err(ZError::Stream));
        assert_eq!(inflateSync(&mut strm), Err(ZError::Stream));
        assert_eq!(inflateSyncPoint(&mut strm), Err(ZError::Stream));
        assert_eq!(inflateUndermine(&mut strm, true), Err(ZError::Stream));
        assert_eq!(inflateValidate(&mut strm, true), Err(ZError::Stream));
        assert_eq!(
            inflateGetHeader(&mut strm, &mut GzHeader::default()),
            Err(ZError::Stream)
        );
        // version and size checks
        assert_eq!(
            inflateInit_(&mut strm, "2.0", size_of::<ZStream<'_>>() as i32),
            Err(ZError::Version)
        );
        assert_eq!(
            inflateInit_(&mut strm, "", size_of::<ZStream<'_>>() as i32),
            Err(ZError::Version)
        );
        assert_eq!(inflateInit_(&mut strm, "1.2.12", 8), Err(ZError::Version));
        assert_eq!(
            inflateInit_(&mut strm, "1.2.12", size_of::<ZStream<'_>>() as i32),
            Ok(())
        );
        assert_eq!(inflateEnd(&mut strm), Ok(()));
        assert_eq!(inflateEnd(&mut strm), Err(ZError::Stream));
        // gzip is not configured
        assert_eq!(inflateInit2(&mut strm, 31), Err(ZError::Stream));
        assert!(matches!(strm.state, InternalState::None));
        assert_eq!(inflateInit2(&mut strm, 15), Ok(()));
        assert_eq!(
            inflateGetHeader(&mut strm, &mut GzHeader::default()),
            Err(ZError::Stream)
        );
        // no ARRR build: undermining is refused
        assert_eq!(inflateUndermine(&mut strm, true), Err(ZError::Data));
        // a dictionary is refused in the middle of a zlib stream
        assert_eq!(inflateSetDictionary(&mut strm, b"x"), Err(ZError::Stream));
        // after an error, inflate stays in BAD until reset
        let bad = [0x78u8, 0x9d, 3, 0];
        strm.next_in = &bad;
        let mut out = [0u8; 4];
        strm.next_out = &mut out;
        assert_eq!(inflate(&mut strm, Flush::NoFlush), Err(ZError::Data));
        assert_eq!(inflate(&mut strm, Flush::NoFlush), Err(ZError::Data));
        assert_eq!(inflateReset(&mut strm), Ok(()));
        assert_eq!(strm.msg, None);
        assert_eq!(inflateEnd(&mut strm), Ok(()));
    }

    #[test]
    fn messages_follow_small() {
        const { assert!(SMALL) };
        assert_eq!(small_msg("incorrect header check"), "error");
    }

    #[test]
    fn full_flush_stream_decodes_whole() {
        assert_decodes(testdata!("fullflush.z"), 15, CORPUS);
        assert_decodes(testdata!("syncflush.z"), 15, SMALL_CORPUS);
    }

    /// Compress `data` with this crate's deflate (`level`, `windowBits`, `strategy`), flushing
    /// with `flush` after every `chunk` bytes and finishing with `Flush::Finish`.
    fn deflate_with(
        data: &[u8],
        level: i32,
        wbits: i32,
        strategy: i32,
        chunk: usize,
        flush: Flush,
    ) -> Vec<u8> {
        use crate::deflate::{deflate, deflateEnd};
        use crate::zlib::{Z_DEFLATED, deflateInit2};
        let mut out = vec![0u8; data.len() + data.len() / 2 + 1024];
        let mut strm = ZStream::new();
        assert_eq!(
            deflateInit2(&mut strm, level, Z_DEFLATED, wbits, 8, strategy),
            Z_OK
        );
        strm.next_out = &mut out;
        let pieces: Vec<&[u8]> = data.chunks(chunk).collect();
        for (i, piece) in pieces.iter().enumerate() {
            strm.next_in = piece;
            let last = i + 1 == pieces.len();
            let ret = deflate(&mut strm, if last { Z_FINISH } else { flush.code() });
            assert_eq!(ret, if last { Z_STREAM_END } else { Z_OK });
            assert_eq!(strm.avail_in(), 0);
        }
        let n = strm.total_out as usize;
        assert_eq!(deflateEnd(&mut strm), Z_OK);
        out.truncate(n);
        out
    }

    #[test]
    fn round_trips_with_this_crates_deflate() {
        use crate::zlib::{Z_DEFAULT_STRATEGY, Z_FILTERED, Z_FIXED, Z_HUFFMAN_ONLY, Z_RLE};
        for level in 0..=9 {
            let z = deflate_with(
                CORPUS,
                level,
                15,
                Z_DEFAULT_STRATEGY,
                usize::MAX,
                Flush::NoFlush,
            );
            assert_decodes(&z, 15, CORPUS);
        }
        for strategy in [Z_FILTERED, Z_HUFFMAN_ONLY, Z_RLE, Z_FIXED] {
            let z = deflate_with(CORPUS, 6, -15, strategy, usize::MAX, Flush::NoFlush);
            assert_decodes(&z, -15, CORPUS);
        }
        for wbits in [9, 12, 15] {
            let z = deflate_with(CORPUS, 9, wbits, Z_DEFAULT_STRATEGY, 5000, Flush::FullFlush);
            assert_decodes(&z, wbits, CORPUS);
            let z = deflate_with(
                CORPUS,
                9,
                -wbits,
                Z_DEFAULT_STRATEGY,
                3000,
                Flush::SyncFlush,
            );
            assert_decodes(&z, -wbits, CORPUS);
        }
    }

    #[test]
    fn round_trips_the_ipcomp_way() {
        // xform_ipcomp.c: raw deflate of a packet with Flush::Finish, raw inflate with
        // Flush::PartialFlush into fresh buffers
        use crate::zlib::Z_DEFAULT_STRATEGY;
        for packet in [&CORPUS[..1400], &CORPUS[5000..5100], CORPUS] {
            let z = deflate_with(
                packet,
                -1,
                -MAX_WBITS,
                Z_DEFAULT_STRATEGY,
                usize::MAX,
                Flush::NoFlush,
            );
            for slow in BOTH {
                let d = decode(&z, -MAX_WBITS, slow, usize::MAX, 256, Flush::PartialFlush);
                assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
                assert!(d.out == packet);
            }
        }
    }

    /// Compress `input` with this crate's deflate in one `Flush::Finish` call, with `memLevel`
    /// `mem_level` (an empty input too, which `deflate_with` has no piece for).
    fn deflate_all(input: &[u8], level: i32, wbits: i32, mem_level: i32) -> Vec<u8> {
        use crate::deflate::{deflate, deflateEnd};
        use crate::zlib::{Z_DEFAULT_STRATEGY, Z_DEFLATED, deflateInit2};
        let mut out = vec![0u8; input.len() + input.len() / 8 + 1024];
        let mut strm = ZStream::new();
        assert_eq!(
            deflateInit2(
                &mut strm,
                level,
                Z_DEFLATED,
                wbits,
                mem_level,
                Z_DEFAULT_STRATEGY
            ),
            Z_OK
        );
        strm.next_in = input;
        strm.next_out = &mut out;
        assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
        let n = strm.total_out as usize;
        assert_eq!(deflateEnd(&mut strm), Z_OK);
        out.truncate(n);
        out
    }

    #[test]
    fn round_trips_every_window_and_memory_size() {
        for wbits in 9..=15 {
            for mem_level in [1, 5, 9] {
                let z = deflate_all(CORPUS, 6, wbits, mem_level);
                assert_decodes(&z, wbits, CORPUS);
                assert_decodes(&z, 0, CORPUS);
                let raw = deflate_all(SMALL_CORPUS, 9, -wbits, mem_level);
                assert_decodes(&raw, -wbits, SMALL_CORPUS);
            }
        }
    }

    #[test]
    fn round_trips_odd_inputs() {
        let zeros = vec![0u8; 70_000];
        let noise: Vec<u8> = (0u32..50_000)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        for input in [&[][..], &[42][..], &zeros[..], &noise[..]] {
            for level in [0, 1, 6, 9] {
                let z = deflate_all(input, level, 15, 8);
                assert_decodes(&z, 15, input);
            }
        }
    }

    /// One stream written in pieces with every flush mode between them (`Flush::PartialFlush` and
    /// `Flush::Block` too) decodes whole, and with one byte of input and output at a time.
    #[test]
    fn round_trips_every_flush_mode() {
        use crate::deflate::{deflate, deflateEnd};
        use crate::zlib::deflateInit;
        let mut out = vec![0u8; CORPUS.len() * 2];
        let mut strm = ZStream::new();
        assert_eq!(deflateInit(&mut strm, 6), Z_OK);
        strm.next_out = &mut out;
        let flushes = [
            Flush::NoFlush,
            Flush::PartialFlush,
            Flush::SyncFlush,
            Flush::FullFlush,
            Flush::Block,
        ];
        for (i, piece) in CORPUS.chunks(3000).enumerate() {
            strm.next_in = piece;
            assert_eq!(deflate(&mut strm, flushes[i % flushes.len()].code()), Z_OK);
            assert_eq!(strm.avail_in(), 0);
        }
        assert_eq!(deflate(&mut strm, Z_FINISH), Z_STREAM_END);
        let n = strm.total_out as usize;
        assert_eq!(deflateEnd(&mut strm), Z_OK);
        let z = &out[..n];
        assert_decodes(z, 15, CORPUS);
        for slow in BOTH {
            let d = decode(z, 15, slow, 1, 1, Flush::NoFlush);
            assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
            assert!(d.out == CORPUS);
        }
    }

    /// `xform_ipcomp.c`'s compression side: raw deflate with `Flush::Finish` into fresh 512-byte
    /// buffers whenever one is full, then raw inflate with `Flush::PartialFlush` into 333-byte ones.
    #[test]
    fn round_trips_ipcomp_output_buffers() {
        use crate::deflate::{deflate, deflateEnd};
        use crate::zlib::{Z_DEFAULT_STRATEGY, Z_DEFLATED, deflateInit2};
        for size in [1, 100, 1000, 1400, 9000] {
            let packet = &CORPUS[..size];
            let mut bufs: Vec<Vec<u8>> = (0..40).map(|_| vec![0u8; 512]).collect();
            let mut used = Vec::new();
            let mut c = ZStream::new();
            assert_eq!(
                deflateInit2(&mut c, 6, Z_DEFLATED, -11, 8, Z_DEFAULT_STRATEGY),
                Z_OK
            );
            c.next_in = packet;
            for buf in bufs.iter_mut() {
                c.next_out = buf;
                let ret = deflate(&mut c, Z_FINISH);
                used.push(512 - c.avail_out());
                if ret == Z_STREAM_END {
                    break;
                }
                assert_eq!(ret, Z_OK);
            }
            assert_eq!(deflateEnd(&mut c), Z_OK);
            let z: Vec<u8> = bufs
                .iter()
                .zip(&used)
                .flat_map(|(b, &n)| b[..n].iter().copied())
                .collect();
            for slow in BOTH {
                let d = decode(&z, -11, slow, usize::MAX, 333, Flush::PartialFlush);
                assert_eq!(d.ret, Ok(ZStatus::StreamEnd));
                assert!(d.out == packet, "size={size} slow={slow}");
            }
        }
    }

    /// A small xorshift PRNG for the property tests (no proptest offline).
    struct XorShift(u64);

    impl XorShift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// Corrupted streams: random bytes of a valid zlib stream flipped, the stream cut short,
    /// or both. Decoding never panics, ends in one of the results a damaged stream can give
    /// (the end, a dictionary request when the header's FDICT bit was hit, a data error, or no
    /// progress when the input runs out), and the slow and fast decoders agree on the result,
    /// the output and the message.
    #[test]
    fn corrupted_streams_fail_the_same_way_in_both_decoders() {
        let good = testdata!("level6.z");
        let mut rng = XorShift(0x9e37_79b9_7f4a_7c15);
        for round in 0..300 {
            let mut s = good.to_vec();
            for _ in 0..1 + rng.below(4) {
                let at = rng.below(s.len());
                s[at] ^= 1 << rng.below(8);
            }
            if round % 3 == 0 {
                s.truncate(rng.below(s.len()));
            }
            let slow = decode_all(&s, 15, true);
            let fast = decode_all(&s, 15, false);
            assert!(
                matches!(
                    slow.ret,
                    Ok(ZStatus::StreamEnd)
                        | Ok(ZStatus::NeedDict)
                        | Err(ZError::Data)
                        | Err(ZError::Buf)
                ),
                "round {round}: {:?}",
                slow.ret
            );
            assert_eq!((slow.ret, slow.msg), (fast.ret, fast.msg), "round {round}");
            assert!(slow.out == fast.out, "round {round}");
            if slow.ret == Err(ZError::Data) {
                assert_eq!(slow.msg, Some("error"));
            }
        }
    }
}
/* </TESTS> */
