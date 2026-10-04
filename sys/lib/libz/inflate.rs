/* <LICENSES> */
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

//! zlib decompression: the inflate state and its state machine, `inflate()`, and the
//! functions that set up, reset, prime, synchronise, copy and end an inflate stream.
//!
//! Upstream: sys/lib/libz/inflate.c @ 3ce1f3f79392, sys/lib/libz/inflate.h @ 3ce1f3f79392
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
//!   site. `inflateGetHeader` refuses every stream (`Z_STREAM_ERROR`, since `wrap` never has
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
//!   contents gets `Z_BUF_ERROR` (the C overruns it). `inflateSetDictionary` takes a slice.
//! - `inflateCopy` reads `source` through a shared reference and copies its stream fields too,
//!   but `dest.next_out` is left empty: the output buffer is a `&mut` and cannot be shared by
//!   two streams. The caller gives `dest` its own.
//! - Not defined in the kernel build, so only the `#else` side is ported: `INFLATE_STRICT`
//!   (`dmax` is kept but never checked), `INFLATE_ALLOW_INVALID_DISTANCE_TOOFAR_ARRR`
//!   (`inflateUndermine` returns `Z_DATA_ERROR` and `sane` stays true), `PKZIP_BUG_WORKAROUND`,
//!   `Z_SOLO`, `ZLIB_DEBUG` (`Trace*` dropped, `Assert` is `zassert!`). `BUILDFIXED` and
//!   `MAKEFIXED` concern `inftrees.c` (`inflate_fixed`), which this zlib's `inflate.c` calls
//!   instead of having `fixedtables()`/`makefixed()` of its own.

#![allow(non_snake_case)] // zlib's API names are camelCase in C (inflateInit2_, inflateReset2)

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::adler32::adler32;
use crate::inffast::inflate_fast;
use crate::inffixed::{distfix, lenfix};
use crate::inftrees::{Code, CodeType, ENOUGH, inflate_fixed, inflate_table};
use crate::zlib::{
    GzHeader, InternalState, Z_BLOCK, Z_BUF_ERROR, Z_DATA_ERROR, Z_DEFLATED, Z_FINISH, Z_MEM_ERROR,
    Z_NEED_DICT, Z_OK, Z_STREAM_END, Z_STREAM_ERROR, Z_TREES, Z_VERSION_ERROR, ZLIB_VERSION,
    ZStream,
};
use crate::zopenbsd::{zcalloc, zcalloc_box, zcfree};
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
    pub(crate) wsize: u32,
    /// `whave`: valid bytes in the window.
    pub(crate) whave: u32,
    /// `wnext`: window write index.
    pub(crate) wnext: u32,
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
    pub(crate) fn new() -> Option<Box<Self>> {
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
    fn try_clone(&self) -> Option<Box<Self>> {
        let mut lens = zcalloc(self.lens.len())?;
        lens.copy_from_slice(&self.lens);
        let mut work = zcalloc(self.work.len())?;
        work.copy_from_slice(&self.work);
        let mut codes = zcalloc(self.codes.len())?;
        codes.copy_from_slice(&self.codes);
        let window = match &self.window {
            Some(src) => {
                let mut window = zcalloc(src.len())?;
                let whave = self.whave as usize;
                window[..whave].copy_from_slice(&src[..whave]);
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
/// call and put back afterwards. `None` (and `f` not called) when `inflateStateCheck` fails.
fn with_state<'a, R>(
    strm: &mut ZStream<'a>,
    f: impl FnOnce(&mut ZStream<'a>, &mut InflateState) -> R,
) -> Option<R> {
    match core::mem::take(&mut strm.state) {
        InternalState::Inflate(mut state) => {
            let r = f(strm, &mut state);
            strm.state = InternalState::Inflate(state);
            Some(r)
        }
        other => {
            strm.state = other;
            None
        }
    }
}

/// `inflateResetKeep`: reset the stream for a new stream but keep the window and its
/// contents (`inflateReset` drops them). Returns `Z_STREAM_ERROR` without an inflate state.
pub fn inflateResetKeep(strm: &mut ZStream<'_>) -> i32 {
    with_state(strm, |strm, state| {
        reset_keep(strm, state);
        Z_OK
    })
    .unwrap_or(Z_STREAM_ERROR)
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
        strm.adler = (state.wrap & 1) as u32;
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
/// contents are dropped (the window memory is kept). Returns `Z_STREAM_ERROR` without an
/// inflate state.
pub fn inflateReset(strm: &mut ZStream<'_>) -> i32 {
    with_state(strm, |strm, state| {
        reset(strm, state);
        Z_OK
    })
    .unwrap_or(Z_STREAM_ERROR)
}

/// The body of `inflateReset`, once the state is known to be valid.
fn reset(strm: &mut ZStream<'_>, state: &mut InflateState) {
    state.wsize = 0;
    state.whave = 0;
    state.wnext = 0;
    reset_keep(strm, state);
}

/// `inflateReset2`: [`inflateReset`] with a new `windowBits` (as for [`inflateInit2_`]); the
/// window is freed if its size changes. Returns `Z_STREAM_ERROR` for an invalid `windowBits`
/// or without an inflate state.
pub fn inflateReset2(strm: &mut ZStream<'_>, windowBits: i32) -> i32 {
    with_state(strm, |strm, state| reset2(strm, state, windowBits)).unwrap_or(Z_STREAM_ERROR)
}

/// The body of `inflateReset2`, once the state is known to be valid.
fn reset2(strm: &mut ZStream<'_>, state: &mut InflateState, windowBits: i32) -> i32 {
    // extract wrap request from windowBits parameter
    let mut windowBits = windowBits;
    let wrap = if windowBits < 0 {
        if windowBits < -15 {
            return Z_STREAM_ERROR;
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
        return Z_STREAM_ERROR;
    }
    if state.window.is_some() && state.wbits != windowBits as u32 {
        zcfree(state.window.take());
    }

    // update state and reset the rest of it
    state.wrap = wrap;
    state.wbits = windowBits as u32;
    reset(strm, state);
    Z_OK
}

/// `inflateInit2_`: initialise `strm` for decompression. `windowBits` is 8..15 for a zlib
/// stream with a window of up to `1 << windowBits` bytes (0: use the size in the header),
/// or -8..-15 for raw deflate data. `version` and `stream_size` must match the library
/// (`inflateInit2` in zlib.rs passes them). Returns `Z_OK`, `Z_MEM_ERROR`,
/// `Z_VERSION_ERROR`, or `Z_STREAM_ERROR` for an invalid `windowBits`.
pub fn inflateInit2_(
    strm: &mut ZStream<'_>,
    windowBits: i32,
    version: &str,
    stream_size: i32,
) -> i32 {
    if version.as_bytes().first() != ZLIB_VERSION.as_bytes().first()
        || stream_size != size_of::<ZStream<'_>>() as i32
    {
        return Z_VERSION_ERROR;
    }
    strm.msg = None; // in case we return an error
    let Some(mut state) = InflateState::new() else {
        return Z_MEM_ERROR;
    };
    // mode is HEAD, to pass the state test in inflateReset2()
    let ret = reset2(strm, &mut state, windowBits);
    if ret == Z_OK {
        strm.state = InternalState::Inflate(state);
    } else {
        zcfree(state);
        strm.state = InternalState::None;
    }
    ret
}

/// `inflateInit_`: [`inflateInit2_`] with the default window, `DEF_WBITS`, and a zlib
/// wrapper.
pub fn inflateInit_(strm: &mut ZStream<'_>, version: &str, stream_size: i32) -> i32 {
    inflateInit2_(strm, DEF_WBITS, version, stream_size)
}

/// `inflatePrime`: insert `bits` bits of `value` (at most 16, and at most 32 in the
/// accumulator) into the input, as if they came before `next_in`. A negative `bits` empties
/// the accumulator. Returns `Z_STREAM_ERROR` for too many bits or without an inflate state.
pub fn inflatePrime(strm: &mut ZStream<'_>, bits: i32, value: i32) -> i32 {
    with_state(strm, |_, state| {
        if bits == 0 {
            return Z_OK;
        }
        if bits < 0 {
            state.hold = 0;
            state.bits = 0;
            return Z_OK;
        }
        if bits > 16 || state.bits + bits as u32 > 32 {
            return Z_STREAM_ERROR;
        }
        let value = i64::from(value) & ((1i64 << bits) - 1);
        state.hold += (value as u64) << state.bits;
        state.bits += bits as u32;
        Z_OK
    })
    .unwrap_or(Z_STREAM_ERROR)
}

/// `updatewindow`: copy the last `wsize` (normally 32K) bytes of `src`, the output just
/// written (or a dictionary), into the circular window, allocating the window first if
/// needed. Returns true (the C's 1) when the window cannot be allocated.
///
/// It runs only when a window is in use already, or when a call wrote output but did not
/// reach the end of the stream (or for a dictionary). Output buffers larger than 32K help:
/// only the last 32K are copied, and later distances fall within the output itself.
pub(crate) fn updatewindow(state: &mut InflateState, src: &[u8]) -> bool {
    // if it hasn't been done already, allocate space for the window
    if state.window.is_none() {
        state.window = zcalloc(1usize << state.wbits);
        if state.window.is_none() {
            return true;
        }
    }

    // if window not in use yet, initialize
    if state.wsize == 0 {
        state.wsize = 1 << state.wbits;
        state.wnext = 0;
        state.whave = 0;
    }

    let Some(window) = state.window.as_deref_mut() else {
        return true;
    };
    let wsize = state.wsize as usize;
    let wnext = state.wnext as usize;

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
            state.wnext = rest as u32;
            state.whave = state.wsize;
        } else {
            state.wnext += dist as u32;
            if state.wnext == state.wsize {
                state.wnext = 0;
            }
            if state.whave < state.wsize {
                state.whave += dist as u32;
            }
        }
    }
    false
}

/// `inflate`: decompress as much as possible from `next_in` into `next_out`, advancing both,
/// and return `Z_OK` (progress made), `Z_STREAM_END` (the end of the stream, check value
/// verified), `Z_NEED_DICT` (call [`inflateSetDictionary`]; `adler` is the dictionary's id),
/// `Z_DATA_ERROR` (corrupt input; `msg` says why), `Z_MEM_ERROR`, `Z_STREAM_ERROR` (no inflate
/// state), or `Z_BUF_ERROR` (no progress was possible, or `flush` is `Z_FINISH` and the end
/// was not reached). `flush` is `Z_NO_FLUSH`, `Z_SYNC_FLUSH`, `Z_FINISH`, or `Z_BLOCK` /
/// `Z_TREES` to stop at the next block boundary / after the block header. `data_type` tells
/// where decoding stopped: the unused bits in the last input byte, plus 64 in the last block,
/// 128 at the end of a block, 256 after a block header.
pub fn inflate(strm: &mut ZStream<'_>, flush: i32) -> i32 {
    inflate_impl(strm, flush, SLOW)
}

/// `inflate()` with `SLOW` as a parameter: the kernel passes `SLOW` (true), the host tests
/// both values, so that `inflate_fast()` decodes the same streams too.
pub(crate) fn inflate_impl(strm: &mut ZStream<'_>, flush: i32, slow: bool) -> i32 {
    with_state(strm, |strm, state| inflate_run(strm, state, flush, slow)).unwrap_or(Z_STREAM_ERROR)
}

/// The state machine of `inflate()`, once the state is known to be valid.
fn inflate_run(strm: &mut ZStream<'_>, state: &mut InflateState, flush: i32, slow: bool) -> i32 {
    use InflateMode::*;

    if state.mode == TYPE {
        state.mode = TYPEDO; // skip check
    }
    let mut r = Regs::load(strm, state);
    let in_ = r.have(); // save starting available input
    let mut out = r.left(); // and output
    let mut ret = Z_OK;

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
                if r.low_bits(4) != Z_DEFLATED as u32 {
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
                    return Z_NEED_DICT;
                }
                state.check = adler32(0, None);
                strm.adler = state.check;
                state.mode = TYPE;
            }
            TYPE | TYPEDO => {
                if state.mode == TYPE && (flush == Z_BLOCK || flush == Z_TREES) {
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
                        if flush == Z_TREES {
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
                if flush == Z_TREES {
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
                let rc = inflate_table(
                    CodeType::CODES,
                    &state.lens[..19],
                    &mut state.codes,
                    &mut state.next,
                    &mut state.lenbits,
                    &mut state.work,
                );
                if rc != 0 {
                    strm.msg = Some(small_msg("invalid code lengths set"));
                    state.mode = BAD;
                    continue;
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
                let rc = inflate_table(
                    CodeType::LENS,
                    &state.lens[..nlen],
                    &mut state.codes,
                    &mut state.next,
                    &mut state.lenbits,
                    &mut state.work,
                );
                if rc != 0 {
                    strm.msg = Some(small_msg("invalid literal/lengths set"));
                    state.mode = BAD;
                    continue;
                }
                state.distcode = CodeTable::Codes(state.next);
                state.distbits = 6;
                let rc = inflate_table(
                    CodeType::DISTS,
                    &state.lens[nlen..nlen + ndist],
                    &mut state.codes,
                    &mut state.next,
                    &mut state.distbits,
                    &mut state.work,
                );
                if rc != 0 {
                    strm.msg = Some(small_msg("invalid distances set"));
                    state.mode = BAD;
                    continue;
                }
                state.mode = LEN_;
                if flush == Z_TREES {
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
                    if copy > state.whave as usize && state.sane {
                        strm.msg = Some(small_msg("invalid distance too far back"));
                        state.mode = BAD;
                        continue;
                    }
                    // (INFLATE_ALLOW_INVALID_DISTANCE_TOOFAR_ARRR is not defined, and sane
                    // is always true without it)
                    let (wsize, wnext) = (state.wsize as usize, state.wnext as usize);
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
                ret = Z_STREAM_END;
                break 'inf_leave;
            }
            BAD => {
                ret = Z_DATA_ERROR;
                break 'inf_leave;
            }
            MEM => {
                r.restore(strm, state);
                return Z_MEM_ERROR;
            }
            // SYNC, and the gzip modes (FLAGS .. HCRC, LENGTH), which have no case of their
            // own when GUNZIP is not defined
            SYNC | FLAGS | TIME | OS | EXLEN | EXTRA | NAME | COMMENT | HCRC | LENGTH => {
                r.restore(strm, state);
                return Z_STREAM_ERROR;
            }
        }
    }

    // inf_leave: return from inflate(), updating the total counts and the check value. If
    // there was no progress during the inflate() call, return a buffer error. Call
    // updatewindow() to create and/or update the window state. Note: a memory error from
    // inflate() is non-recoverable.
    let written = out - r.left();
    if (state.wsize != 0
        || (written != 0 && state.mode < BAD && (state.mode < CHECK || flush != Z_FINISH)))
        && updatewindow(state, &r.output[r.put - written..r.put])
    {
        state.mode = MEM;
        r.restore(strm, state);
        return Z_MEM_ERROR;
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
    if ((in_ == 0 && out == 0) || flush == Z_FINISH) && ret == Z_OK {
        ret = Z_BUF_ERROR;
    }
    ret
}

/// `inflateEnd`: free the stream's inflate state (and window). Returns `Z_STREAM_ERROR`
/// without an inflate state.
pub fn inflateEnd(strm: &mut ZStream<'_>) -> i32 {
    if inflateStateCheck(strm) {
        return Z_STREAM_ERROR;
    }
    zcfree(core::mem::take(&mut strm.state));
    Z_OK
}

/// `inflateGetDictionary`: copy the sliding window's contents, oldest first, to
/// `dictionary` (which needs room for up to `1 << windowBits` bytes; `None` to only get the
/// length) and store their length in `dictLength`. Returns `Z_BUF_ERROR` if `dictionary` is
/// too short, `Z_STREAM_ERROR` without an inflate state.
pub fn inflateGetDictionary(
    strm: &mut ZStream<'_>,
    dictionary: Option<&mut [u8]>,
    dictLength: Option<&mut u32>,
) -> i32 {
    with_state(strm, |_, state| {
        // copy dictionary
        let (whave, wnext) = (state.whave as usize, state.wnext as usize);
        if whave != 0
            && let Some(dictionary) = dictionary
        {
            if dictionary.len() < whave {
                return Z_BUF_ERROR;
            }
            let window = state.window.as_deref().unwrap_or(&[]);
            dictionary[..whave - wnext].copy_from_slice(&window[wnext..whave]);
            dictionary[whave - wnext..whave].copy_from_slice(&window[..wnext]);
        }
        if let Some(dictLength) = dictLength {
            *dictLength = state.whave;
        }
        Z_OK
    })
    .unwrap_or(Z_STREAM_ERROR)
}

/// `inflateSetDictionary`: give the decompressor the preset dictionary, after `inflate()`
/// returned `Z_NEED_DICT` for a zlib stream (the dictionary's Adler-32 must be the id in the
/// header), or at any time for a raw stream. Returns `Z_DATA_ERROR` for the wrong
/// dictionary, `Z_STREAM_ERROR` at the wrong moment or without an inflate state,
/// `Z_MEM_ERROR` if the window cannot be allocated.
pub fn inflateSetDictionary(strm: &mut ZStream<'_>, dictionary: &[u8]) -> i32 {
    with_state(strm, |_, state| {
        if state.wrap != 0 && state.mode != InflateMode::DICT {
            return Z_STREAM_ERROR;
        }

        // check for correct dictionary identifier
        if state.mode == InflateMode::DICT {
            let dictid = adler32(adler32(0, None), Some(dictionary));
            if dictid != state.check {
                return Z_DATA_ERROR;
            }
        }

        // copy dictionary to window using updatewindow(), which will amend the existing
        // dictionary if appropriate
        if updatewindow(state, dictionary) {
            state.mode = InflateMode::MEM;
            return Z_MEM_ERROR;
        }
        state.havedict = true;
        Z_OK
    })
    .unwrap_or(Z_STREAM_ERROR)
}

/// `inflateGetHeader`: ask for the gzip header to be stored in `head`. Only a gzip stream
/// has one, and this zlib is built without gzip decoding, so it always returns
/// `Z_STREAM_ERROR`, as the C build does.
pub fn inflateGetHeader(strm: &mut ZStream<'_>, head: &mut GzHeader) -> i32 {
    with_state(strm, |_, state| {
        if state.wrap & 2 == 0 {
            return Z_STREAM_ERROR;
        }
        // Not reached in this build (wrap never has bit 1 without GUNZIP); the C would keep
        // `head` in the state to fill it while decoding a gzip header.
        head.done = 0;
        Z_OK
    })
    .unwrap_or(Z_STREAM_ERROR)
}

/// `syncsearch`: search `buf` for the pattern 0, 0, 0xff, 0xff. `*have` is the number of
/// pattern bytes found so far (0..3; 0 for the first call) and is updated. If it reaches 4,
/// the pattern was found and the return value is the number of bytes read, including the
/// pattern's last byte; otherwise the return value is `buf.len()`, and the search can go on
/// with more data and the same `*have`.
fn syncsearch(have: &mut u32, buf: &[u8]) -> usize {
    let mut got = *have;
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
    *have = got;
    next
}

/// `inflateSync`: skip input until the 0, 0, 0xff, 0xff marker of a full flush point (an
/// empty stored block), then get ready to inflate the block after it. Returns `Z_OK` when
/// found (the check value is not verified from then on), `Z_DATA_ERROR` when the input ran
/// out first (call again with more), `Z_BUF_ERROR` with no input, `Z_STREAM_ERROR` without an
/// inflate state. `total_in` counts the skipped bytes.
pub fn inflateSync(strm: &mut ZStream<'_>) -> i32 {
    with_state(strm, |strm, state| {
        if strm.avail_in() == 0 && state.bits < 8 {
            return Z_BUF_ERROR;
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
            state.have = 0;
            syncsearch(&mut state.have, &buf[..len]);
        }

        // search available input
        let len = syncsearch(&mut state.have, strm.next_in);
        strm.take_in(len);
        strm.total_in += len as u64;

        // return no joy or set up to restart inflate() on a new block
        if state.have != 4 {
            return Z_DATA_ERROR;
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
        Z_OK
    })
    .unwrap_or(Z_STREAM_ERROR)
}

/// `inflateSyncPoint`: 1 if inflate is at the end of a block made by `Z_SYNC_FLUSH` or
/// `Z_FULL_FLUSH`, waiting for the length bytes of the empty stored block, else 0. One PPP
/// implementation uses it: PPP flushes with `Z_SYNC_FLUSH` but drops those length bytes, and
/// checks at the end of a packet that inflate is waiting for them. `Z_STREAM_ERROR` without
/// an inflate state.
pub fn inflateSyncPoint(strm: &mut ZStream<'_>) -> i32 {
    with_state(strm, |_, state| {
        i32::from(state.mode == InflateMode::STORED && state.bits == 0)
    })
    .unwrap_or(Z_STREAM_ERROR)
}

/// `inflateCopy`: make `dest` a copy of `source`, inflate state and window included. `dest`
/// gets `source`'s input and counters but an empty `next_out` (an output buffer cannot be
/// shared); its previous state, if any, is dropped. Returns `Z_MEM_ERROR` or
/// `Z_STREAM_ERROR` (no inflate state in `source`).
pub fn inflateCopy<'a>(dest: &mut ZStream<'a>, source: &ZStream<'a>) -> i32 {
    // check input
    let InternalState::Inflate(state) = &source.state else {
        return Z_STREAM_ERROR;
    };

    // allocate space and copy state
    let Some(copy) = state.try_clone() else {
        return Z_MEM_ERROR;
    };
    dest.next_in = source.next_in;
    dest.total_in = source.total_in;
    dest.next_out = &mut [];
    dest.total_out = source.total_out;
    dest.msg = source.msg;
    dest.data_type = source.data_type;
    dest.adler = source.adler;
    dest.state = InternalState::Inflate(copy);
    Z_OK
}

/// `inflateUndermine`: allow (`subvert != 0`) distances too far back, filling with zeros.
/// Only `INFLATE_ALLOW_INVALID_DISTANCE_TOOFAR_ARRR` builds allow it, and the kernel's is not
/// one: it returns `Z_DATA_ERROR` and keeps the check (`Z_STREAM_ERROR` without an inflate
/// state).
pub fn inflateUndermine(strm: &mut ZStream<'_>, subvert: i32) -> i32 {
    with_state(strm, |_, state| {
        let _ = subvert;
        state.sane = true;
        Z_DATA_ERROR
    })
    .unwrap_or(Z_STREAM_ERROR)
}

/// `inflateValidate`: check (`check != 0`) or ignore the zlib stream's Adler-32 trailer.
/// Returns `Z_STREAM_ERROR` without an inflate state.
pub fn inflateValidate(strm: &mut ZStream<'_>, check: i32) -> i32 {
    with_state(strm, |_, state| {
        if check != 0 && state.wrap != 0 {
            state.wrap |= 4;
        } else {
            state.wrap &= !4;
        }
        Z_OK
    })
    .unwrap_or(Z_STREAM_ERROR)
}

/// `inflateMark`: where decoding stands, for random access: the upper bits are the number of
/// bits back from `next_in` where the current code starts (-1 between blocks and codes, so
/// the value is negative), the low 16 bits the bytes still to copy of a stored block or the
/// bytes already copied of a match. `-(1 << 16)` without an inflate state.
pub fn inflateMark(strm: &mut ZStream<'_>) -> i64 {
    with_state(strm, |_, state| {
        let low = match state.mode {
            InflateMode::COPY => state.length,
            InflateMode::MATCH => state.was - state.length,
            _ => 0,
        };
        ((i64::from(state.back) as u64) << 16) as i64 + i64::from(low)
    })
    .unwrap_or(-(1 << 16))
}

/// `inflateCodesUsed`: the number of entries of the code space used by the current dynamic
/// block's tables (to check `ENOUGH`). `u64::MAX` (the C's `(unsigned long)-1`) without an
/// inflate state.
pub fn inflateCodesUsed(strm: &mut ZStream<'_>) -> u64 {
    with_state(strm, |_, state| state.next as u64).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
