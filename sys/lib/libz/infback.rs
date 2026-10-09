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

/* infback.c -- inflate using a call-back interface
 * Copyright (C) 1995-2026 Mark Adler
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
//! Inflate through a call-back interface: `inflateBack()` decodes a raw deflate stream,
//! pulling input from one closure and pushing output to another, with the caller's window
//! as the output buffer, so no output is copied to a separate window.
//!
//! Upstream: sys/lib/libz/infback.c @ 3ce1f3f79392
//! LZ: sys/lib/libz/infback.rs@f5985f1d055a
//!
//! **This is an altered source version, not the original zlib `infback.c`** (zlib licence,
//! clause 2): a Rust rewrite written for EmiBSD. The original's notice is kept above in full
//! (clause 3). Do not mistake it for the zlib distribution.
//!
//! The decoder is largely the one of `inflate.c`, without the zlib wrapper and the modes that
//! only exist so that `inflate()` can return in the middle of a block: `inflateBack()` runs to
//! the end of the stream (or an error) in one call. `in_` is asked for more input when the
//! current input runs out; `out` gets the window each time it is full, and what is left in it
//! at the end. OpenBSD's kernel compiles this file (`sys/conf/files`), though no kernel code
//! calls it.
//!
//! ## Deviations
//! - `in_func`/`out_func` and their `void *` descriptors are closures: `in_()` returns the
//!   next input (an empty slice is the C's 0, no more input), `out(buf)` returns whether it
//!   wrote `buf` (the C's 0). The input must live as long as the stream (`'a`), because the unused part is
//!   left in `next_in` on return.
//! - `inflateBackInit_` takes the window as an owned buffer of at least `1 << windowBits`
//!   bytes, kept in the state until `inflateBackEnd`, which frees it with the state (the C
//!   keeps a pointer to the caller's memory and leaves freeing it to the caller). A shorter
//!   buffer is refused like the C's `window == Z_NULL`.
//! - The state is `InternalState::InflateBack`: `inflate()` and the other `inflate*`
//!   functions refuse it, as the C's `inflateStateCheck` does (this init sets neither
//!   `state->strm` nor `mode`), and `inflateBack()` accepts only it (the C would run on any
//!   non-null state, even one without a window). `inflateBackEnd` frees an `Inflate` state too.
//! - After a failing `in_()`, the C sets `next_in` to `Z_NULL` so the caller can tell an input
//!   failure from an output one; here `next_in` is empty either way, and the caller's closures
//!   know which of them failed.
//! - `SLOW` (the kernel build) keeps `inflate_fast()` out; as in `inflate.rs`, a crate-private
//!   entry point takes `slow` so the host tests run both decoders. `SMALL` messages, and only
//!   the C compiled without `PKZIP_BUG_WORKAROUND`, `Z_SOLO` and `ZLIB_DEBUG`, as in
//!   `inflate.rs`.
//!
//! ## Redesign
//! - `inflateBackInit_`, `inflateBack` and `inflateBackEnd` return a `Result` (`zlib.rs`):
//!   `inflateBack` gives `Ok(ZStatus::StreamEnd)` or the error, the other two `Ok(())`.
//! - The output closure returns `bool`, true when it wrote the buffer: LZ kept the C's
//!   `int`, zero for success.
//! - `inflate_table` returns the tables' end and root bits (`inftrees.rs`); the window
//!   positions are `usize` (`inflate.rs`).

#![allow(non_snake_case)] // zlib's API names are camelCase in C (inflateBackInit_)

use alloc::vec::Vec;

use crate::inffast::inflate_fast;
use crate::inflate::{CodeTable, InflateMode, InflateState, Regs, copy_forward, order, small_msg};
use crate::inftrees::{CodeType, inflate_fixed, inflate_table};
use crate::zlib::{InternalState, ZError, ZLIB_VERSION, ZStatus, ZStream};
use crate::zopenbsd::zcfree;
use crate::zutil::SLOW;

/// `inflateBackInit_`: initialise `strm` for [`inflateBack`], with a window of `1 <<
/// windowBits` bytes (`windowBits` in 8..15) that is also the output buffer. `version` and
/// `stream_size` must match the library (`inflateBackInit` in zlib.rs passes them). Fails
/// with `ZError::Mem`, `ZError::Version`, or `ZError::Stream` for a bad `windowBits` or a
/// window too short.
pub fn inflateBackInit_(
    strm: &mut ZStream<'_>,
    windowBits: i32,
    window: Vec<u8>,
    version: &str,
    stream_size: i32,
) -> Result<(), ZError> {
    if version.as_bytes().first() != ZLIB_VERSION.as_bytes().first()
        || stream_size != size_of::<ZStream<'_>>() as i32
    {
        return Err(ZError::Version);
    }
    if !(8..=15).contains(&windowBits) || window.len() < 1 << windowBits {
        return Err(ZError::Stream);
    }
    strm.msg = None; // in case we return an error
    let mut state = InflateState::new().ok_or(ZError::Mem)?;
    state.dmax = 32768;
    state.wbits = windowBits.unsigned_abs();
    state.wsize = 1 << windowBits;
    state.window = Some(window);
    state.wnext = 0;
    state.whave = 0;
    state.sane = true;
    strm.state = InternalState::InflateBack(state);
    Ok(())
}

/// `PULL()`: make sure some input is available, asking `in_` for more if needed; `false`
/// when it has none (the C's `Z_BUF_ERROR` return).
fn pull<'a>(r: &mut Regs<'a, '_>, in_: &mut impl FnMut() -> &'a [u8]) -> bool {
    if r.have() == 0 {
        r.input = in_();
        r.next = 0;
        if r.input.is_empty() {
            return false;
        }
    }
    true
}

/// `PULLBYTE()`: get a byte of input into the bit accumulator; `false` when there is none.
fn pull_byte<'a>(r: &mut Regs<'a, '_>, in_: &mut impl FnMut() -> &'a [u8]) -> bool {
    pull(r, in_) && r.pull_byte()
}

/// `NEEDBITS(n)`: make sure there are at least `n` bits in the accumulator; `false` when the
/// input runs out.
fn need_bits<'a>(r: &mut Regs<'a, '_>, in_: &mut impl FnMut() -> &'a [u8], n: u32) -> bool {
    while r.bits < n {
        if !pull_byte(r, in_) {
            return false;
        }
    }
    true
}

/// `ROOM()`: make sure some output space is available, by writing out the window if it is
/// full; `false` when `out` fails (the C's `Z_BUF_ERROR` return).
fn room(
    r: &mut Regs<'_, '_>,
    state: &mut InflateState,
    out: &mut impl FnMut(&[u8]) -> bool,
) -> bool {
    if r.left() == 0 {
        r.put = 0;
        state.whave = state.wsize;
        if !out(&r.output[..]) {
            return false;
        }
    }
    true
}

/// `inflateBack`: decode a raw deflate stream from `strm.next_in` and then `in_()`, writing
/// it through `out()`, which returns true when it wrote the buffer. Returns
/// `Ok(ZStatus::StreamEnd)` at the end of the stream, or fails with `ZError::Buf` if `in_()`
/// had no more input or `out()` failed, `ZError::Data` for corrupt data (`msg` says why),
/// `ZError::Stream` if `strm` was not set up by [`inflateBackInit_`]. On return, `next_in`
/// holds the input not used.
///
/// The caller must not change the input before `in_` is called again or `inflateBack()`
/// returns; the window is the state's own while it runs.
pub fn inflateBack<'a>(
    strm: &mut ZStream<'a>,
    in_: impl FnMut() -> &'a [u8],
    out: impl FnMut(&[u8]) -> bool,
) -> Result<ZStatus, ZError> {
    inflate_back_impl(strm, in_, out, SLOW)
}

/// `inflateBack()` with `SLOW` as a parameter (the kernel passes `SLOW`; the host tests
/// both values).
pub(crate) fn inflate_back_impl<'a>(
    strm: &mut ZStream<'a>,
    mut in_: impl FnMut() -> &'a [u8],
    mut out: impl FnMut(&[u8]) -> bool,
    slow: bool,
) -> Result<ZStatus, ZError> {
    // Check that the strm exists and that the state was initialized
    let mut state = match core::mem::take(&mut strm.state) {
        InternalState::InflateBack(state) => state,
        other => {
            strm.state = other;
            return Err(ZError::Stream);
        }
    };
    let ret = back_run(strm, &mut state, &mut in_, &mut out, slow);
    strm.state = InternalState::InflateBack(state);
    ret
}

/// The body of `inflateBack()`, once the state is known to be valid.
fn back_run<'a>(
    strm: &mut ZStream<'a>,
    state: &mut InflateState,
    in_: &mut impl FnMut() -> &'a [u8],
    out: &mut impl FnMut(&[u8]) -> bool,
    slow: bool,
) -> Result<ZStatus, ZError> {
    use InflateMode::*;

    let wsize = state.wsize;
    let Some(mut window) = state.window.take() else {
        return Err(ZError::Stream);
    };

    // Reset the state
    strm.msg = None;
    state.mode = TYPE;
    state.last = false;
    state.whave = 0;
    let mut r = Regs {
        input: strm.next_in,
        next: 0,
        output: &mut window[..wsize],
        put: 0,
        hold: 0,
        bits: 0,
    };
    let mut ret;

    // Inflate until end of block marked as last
    'inf_leave: loop {
        match state.mode {
            TYPE => {
                // determine and dispatch block type
                if state.last {
                    r.byte_bits();
                    state.mode = DONE;
                    continue;
                }
                if !need_bits(&mut r, in_, 3) {
                    ret = Err(ZError::Buf);
                    break 'inf_leave;
                }
                state.last = r.low_bits(1) != 0;
                r.drop_bits(1);
                match r.low_bits(2) {
                    0 => state.mode = STORED, // stored block
                    1 => {
                        // fixed block
                        inflate_fixed(state);
                        state.mode = LEN; // decode codes
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
                // get and verify stored block length
                r.byte_bits(); // go to byte boundary
                if !need_bits(&mut r, in_, 32) {
                    ret = Err(ZError::Buf);
                    break 'inf_leave;
                }
                if (r.hold & 0xffff) != ((r.hold >> 16) ^ 0xffff) {
                    strm.msg = Some(small_msg("invalid stored block lengths"));
                    state.mode = BAD;
                    continue;
                }
                state.length = (r.hold & 0xffff) as u32;
                r.init_bits();

                // copy stored block from input to output
                while state.length != 0 {
                    if !pull(&mut r, in_) || !room(&mut r, state, out) {
                        ret = Err(ZError::Buf);
                        break 'inf_leave;
                    }
                    let copy = (state.length as usize).min(r.have()).min(r.left());
                    r.output[r.put..r.put + copy].copy_from_slice(&r.input[r.next..r.next + copy]);
                    r.next += copy;
                    r.put += copy;
                    state.length -= copy as u32;
                }
                state.mode = TYPE;
            }
            TABLE => {
                // get dynamic table entries descriptor
                if !need_bits(&mut r, in_, 14) {
                    ret = Err(ZError::Buf);
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

                // get code length code lengths (not a typo)
                state.have = 0;
                while state.have < state.ncode {
                    if !need_bits(&mut r, in_, 3) {
                        ret = Err(ZError::Buf);
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

                // get length and distance code code lengths
                state.have = 0;
                while state.have < state.nlen + state.ndist {
                    let here = loop {
                        let here = state.table(state.lencode)[r.low_bits(state.lenbits) as usize];
                        if u32::from(here.bits) <= r.bits {
                            break here;
                        }
                        if !pull_byte(&mut r, in_) {
                            ret = Err(ZError::Buf);
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
                            if !need_bits(&mut r, in_, u32::from(here.bits) + 2) {
                                ret = Err(ZError::Buf);
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
                            if !need_bits(&mut r, in_, u32::from(here.bits) + 3) {
                                ret = Err(ZError::Buf);
                                break 'inf_leave;
                            }
                            r.drop_bits(u32::from(here.bits));
                            len = 0;
                            copy = 3 + r.low_bits(3);
                            r.drop_bits(3);
                        } else {
                            if !need_bits(&mut r, in_, u32::from(here.bits) + 7) {
                                ret = Err(ZError::Buf);
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
                state.mode = LEN;
            }
            LEN => {
                // use inflate_fast() if we have enough input and output (never under SLOW,
                // the kernel build)
                if !slow && r.have() >= 6 && r.left() >= 258 {
                    if state.whave < state.wsize {
                        state.whave = state.wsize - r.left();
                    }
                    inflate_fast(strm, state, &mut r, wsize, true);
                    continue;
                }

                // get a literal, length, or end-of-block code
                let mut here = loop {
                    let here = state.table(state.lencode)[r.low_bits(state.lenbits) as usize];
                    if u32::from(here.bits) <= r.bits {
                        break here;
                    }
                    if !pull_byte(&mut r, in_) {
                        ret = Err(ZError::Buf);
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
                        if !pull_byte(&mut r, in_) {
                            ret = Err(ZError::Buf);
                            break 'inf_leave;
                        }
                    };
                    r.drop_bits(lbits);
                }
                r.drop_bits(u32::from(here.bits));
                state.length = u32::from(here.val);

                // process literal
                if here.op == 0 {
                    if !room(&mut r, state, out) {
                        ret = Err(ZError::Buf);
                        break 'inf_leave;
                    }
                    r.output[r.put] = state.length as u8;
                    r.put += 1;
                    state.mode = LEN;
                    continue;
                }

                // process end of block
                if here.op & 32 != 0 {
                    state.mode = TYPE;
                    continue;
                }

                // invalid code
                if here.op & 64 != 0 {
                    strm.msg = Some(small_msg("invalid literal/length code"));
                    state.mode = BAD;
                    continue;
                }

                // length code -- get extra bits, if any
                state.extra = u32::from(here.op) & 15;
                if state.extra != 0 {
                    if !need_bits(&mut r, in_, state.extra) {
                        ret = Err(ZError::Buf);
                        break 'inf_leave;
                    }
                    state.length += r.low_bits(state.extra);
                    r.drop_bits(state.extra);
                }

                // get distance code
                let mut here = loop {
                    let here = state.table(state.distcode)[r.low_bits(state.distbits) as usize];
                    if u32::from(here.bits) <= r.bits {
                        break here;
                    }
                    if !pull_byte(&mut r, in_) {
                        ret = Err(ZError::Buf);
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
                        if !pull_byte(&mut r, in_) {
                            ret = Err(ZError::Buf);
                            break 'inf_leave;
                        }
                    };
                    r.drop_bits(lbits);
                }
                r.drop_bits(u32::from(here.bits));
                if here.op & 64 != 0 {
                    strm.msg = Some(small_msg("invalid distance code"));
                    state.mode = BAD;
                    continue;
                }
                state.offset = u32::from(here.val);

                // get distance extra bits, if any
                state.extra = u32::from(here.op) & 15;
                if state.extra != 0 {
                    if !need_bits(&mut r, in_, state.extra) {
                        ret = Err(ZError::Buf);
                        break 'inf_leave;
                    }
                    state.offset += r.low_bits(state.extra);
                    r.drop_bits(state.extra);
                }
                let reach = if state.whave < state.wsize {
                    r.left()
                } else {
                    0
                };
                if state.offset as usize > wsize - reach {
                    strm.msg = Some(small_msg("invalid distance too far back"));
                    state.mode = BAD;
                    continue;
                }

                // copy match from window to output
                loop {
                    if !room(&mut r, state, out) {
                        ret = Err(ZError::Buf);
                        break 'inf_leave;
                    }
                    let mut copy = wsize - state.offset as usize;
                    let from;
                    if copy < r.left() {
                        from = r.put + copy;
                        copy = r.left() - copy;
                    } else {
                        from = r.put - state.offset as usize;
                        copy = r.left();
                    }
                    let copy = copy.min(state.length as usize);
                    state.length -= copy as u32;
                    copy_forward(r.output, &mut r.put, from, copy);
                    if state.length == 0 {
                        break;
                    }
                }
            }
            DONE => {
                // inflate stream terminated properly
                ret = Ok(ZStatus::StreamEnd);
                break 'inf_leave;
            }
            BAD => {
                ret = Err(ZError::Data);
                break 'inf_leave;
            }
            _ => {
                // can't happen
                ret = Err(ZError::Stream);
                break 'inf_leave;
            }
        }
    }

    // Write leftover output and return unused input
    if r.put > 0 && !out(&r.output[..r.put]) && ret == Ok(ZStatus::StreamEnd) {
        ret = Err(ZError::Buf);
    }
    strm.next_in = &r.input[r.next..];
    state.window = Some(window);
    ret
}

/// `inflateBackEnd`: free the state (and the window) made by [`inflateBackInit_`].
/// `Err(ZError::Stream)` if `strm` has no inflate state.
pub fn inflateBackEnd(strm: &mut ZStream<'_>) -> Result<(), ZError> {
    match core::mem::take(&mut strm.state) {
        InternalState::InflateBack(state) | InternalState::Inflate(state) => {
            zcfree(state);
            Ok(())
        }
        other => {
            strm.state = other;
            Err(ZError::Stream)
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests of `infback.rs`: raw deflate streams of `sys/lib/libz/testdata` (made by
    // `testdata/gen_inflate.py`, see `inflate.rs`) decoded through closures, with and
    // without `inflate_fast()`.

    use super::*;
    use crate::inflate::{inflate, inflateEnd};
    use crate::zlib::{Flush, inflateBackInit, inflateInit2};
    use std::vec;
    use std::vec::Vec;

    static CORPUS: &[u8] = include_bytes!("testdata/inflate_corpus.bin");
    static RAW15: &[u8] = include_bytes!("testdata/inflate_raw15.z");
    static RAW12: &[u8] = include_bytes!("testdata/inflate_raw12.z");

    /// Decode `input` with a `1 << wbits` window, `strm.next_in` holding its first `first`
    /// bytes and `in_` handing out the rest `chunk` bytes at a time. Returns the return code,
    /// the output, the sizes `out` was called with and the unused input.
    fn back(
        input: &[u8],
        wbits: i32,
        first: usize,
        chunk: usize,
        slow: bool,
    ) -> (Result<ZStatus, ZError>, Vec<u8>, Vec<usize>, usize) {
        let mut strm = ZStream::new();
        assert_eq!(
            inflateBackInit(&mut strm, wbits, vec![0u8; 1 << wbits]),
            Ok(())
        );
        strm.next_in = &input[..first];
        let mut rest = &input[first..];
        let mut out = Vec::new();
        let mut sizes = Vec::new();
        let ret = inflate_back_impl(
            &mut strm,
            || {
                let (head, tail) = rest.split_at(chunk.min(rest.len()));
                rest = tail;
                head
            },
            |buf| {
                out.extend_from_slice(buf);
                sizes.push(buf.len());
                true
            },
            slow,
        );
        let unused = strm.avail_in() + rest.len();
        assert_eq!(inflateBackEnd(&mut strm), Ok(()));
        (ret, out, sizes, unused)
    }

    #[test]
    fn decodes_through_closures() {
        for slow in [true, false] {
            for (input, wbits) in [(RAW15, 15), (RAW12, 12), (RAW12, 15)] {
                for (first, chunk) in [(0, 1), (0, 1000), (100, 7), (input.len(), 1)] {
                    let (ret, out, sizes, unused) = back(input, wbits, first, chunk, slow);
                    assert_eq!(
                        ret,
                        Ok(ZStatus::StreamEnd),
                        "slow={slow} wbits={wbits} {first}/{chunk}"
                    );
                    assert!(out == CORPUS);
                    assert_eq!(unused, 0);
                    // the window goes out whole each time it is full, then what is left
                    let wsize = 1 << wbits;
                    assert!(sizes[..sizes.len() - 1].iter().all(|&n| n == wsize));
                    assert_eq!(sizes.len(), CORPUS.len().div_ceil(wsize));
                }
            }
        }
    }

    #[test]
    fn leaves_unused_input_in_next_in() {
        let mut input = RAW15.to_vec();
        input.extend_from_slice(b"trailer");
        let mut strm = ZStream::new();
        assert_eq!(inflateBackInit(&mut strm, 15, vec![0u8; 1 << 15]), Ok(()));
        strm.next_in = &input;
        let mut out = Vec::new();
        let ret = inflateBack(
            &mut strm,
            || &[],
            |buf| {
                out.extend_from_slice(buf);
                true
            },
        );
        assert_eq!(ret, Ok(ZStatus::StreamEnd));
        assert_eq!(strm.next_in, b"trailer");
        assert_eq!(inflateBackEnd(&mut strm), Ok(()));
        assert!(out == CORPUS);
    }

    #[test]
    fn input_and_output_failures() {
        for slow in [true, false] {
            // in() runs dry: Err(ZError::Buf), what was decoded is still written out
            let (ret, out, _, _) = back(&RAW15[..RAW15.len() / 2], 15, 0, 100, slow);
            assert_eq!(ret, Err(ZError::Buf));
            assert!(!out.is_empty() && CORPUS.starts_with(&out));

            // out() fails: Err(ZError::Buf)
            let mut strm = ZStream::new();
            assert_eq!(inflateBackInit(&mut strm, 9, vec![0u8; 512]), Ok(()));
            strm.next_in = RAW12;
            let mut calls = 0;
            let ret = inflate_back_impl(
                &mut strm,
                || &[],
                |_| {
                    calls += 1;
                    false
                },
                slow,
            );
            assert_eq!((ret, calls), (Err(ZError::Buf), 1));
            assert_eq!(inflateBackEnd(&mut strm), Ok(()));
        }
    }

    #[test]
    fn data_errors() {
        for slow in [true, false] {
            // invalid block type
            let bad = [0x07u8, 0, 0, 0, 0, 0, 0, 0];
            let mut strm = ZStream::new();
            assert_eq!(inflateBackInit(&mut strm, 15, vec![0u8; 1 << 15]), Ok(()));
            strm.next_in = &bad;
            let ret = inflate_back_impl(&mut strm, || &[], |_| true, slow);
            assert_eq!((ret, strm.msg), (Err(ZError::Data), Some("error")));
            // a distance too far back: a match before any output (fixed block: 1, 01, length
            // code 257 = 0000001, distance code 0 = 00000), padded for inflate_fast
            let far = [0x03u8, 0x02, 0, 0, 0, 0, 0, 0, 0, 0];
            strm.next_in = &far;
            let ret = inflate_back_impl(&mut strm, || &[], |_| true, slow);
            assert_eq!((ret, strm.msg), (Err(ZError::Data), Some("error")));
            assert_eq!(inflateBackEnd(&mut strm), Ok(()));
        }
    }

    #[test]
    fn init_and_state_checks() {
        let mut strm = ZStream::new();
        // no state
        assert_eq!(
            inflateBack(&mut strm, || &[], |_| true),
            Err(ZError::Stream)
        );
        assert_eq!(inflateBackEnd(&mut strm), Err(ZError::Stream));
        // bad window
        assert_eq!(
            inflateBackInit(&mut strm, 7, vec![0u8; 128]),
            Err(ZError::Stream)
        );
        assert_eq!(
            inflateBackInit(&mut strm, 16, vec![0u8; 1 << 16]),
            Err(ZError::Stream)
        );
        assert_eq!(
            inflateBackInit(&mut strm, 15, vec![0u8; 1000]),
            Err(ZError::Stream)
        );
        assert_eq!(
            inflateBackInit_(
                &mut strm,
                15,
                vec![0u8; 1 << 15],
                "0.9",
                size_of::<ZStream<'_>>() as i32
            ),
            Err(ZError::Version)
        );
        assert_eq!(
            inflateBackInit_(&mut strm, 15, vec![0u8; 1 << 15], "1.3.2", 3),
            Err(ZError::Version)
        );
        // a call-back state is not an inflate() state, and the other way round
        assert_eq!(inflateBackInit(&mut strm, 15, vec![0u8; 1 << 15]), Ok(()));
        assert_eq!(inflate(&mut strm, Flush::NoFlush), Err(ZError::Stream));
        assert_eq!(inflateEnd(&mut strm), Err(ZError::Stream));
        assert_eq!(inflateBackEnd(&mut strm), Ok(()));
        assert_eq!(inflateInit2(&mut strm, -15), Ok(()));
        assert_eq!(
            inflateBack(&mut strm, || &[], |_| true),
            Err(ZError::Stream)
        );
        // inflateBackEnd frees an inflate() state too, as the C does
        assert_eq!(inflateBackEnd(&mut strm), Ok(()));
        assert_eq!(inflateEnd(&mut strm), Err(ZError::Stream));
    }

    #[test]
    fn reused_for_several_streams() {
        let mut strm = ZStream::new();
        assert_eq!(inflateBackInit(&mut strm, 15, vec![0u8; 1 << 15]), Ok(()));
        for input in [RAW15, RAW12] {
            strm.next_in = input;
            let mut out = Vec::new();
            let ret = inflateBack(
                &mut strm,
                || &[],
                |buf| {
                    out.extend_from_slice(buf);
                    true
                },
            );
            assert_eq!(ret, Ok(ZStatus::StreamEnd));
            assert!(out == CORPUS);
        }
        assert_eq!(inflateBackEnd(&mut strm), Ok(()));
    }
}
/* </TESTS> */
