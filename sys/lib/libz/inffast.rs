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

/* inffast.c -- fast decoding
 * Copyright (C) 1995-2026 Mark Adler
 * For conditions of distribution and use, see copyright notice in zlib.h
 */

/* inffast.h -- header to use inffast.c
 * Copyright (C) 1995-2003, 2010 Mark Adler
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
//! Fast decoding: `inflate_fast` decodes literals and length/distance pairs of a block
//! without checking for input or output at each step, while there is enough of both.
//!
//! Upstream: sys/lib/libz/inffast.c @ 3ce1f3f79392, sys/lib/libz/inffast.h @ 3ce1f3f79392
//!
//! **This is an altered source version, not the original zlib `inffast.c`/`inffast.h`** (zlib
//! licence, clause 2): a Rust rewrite written for EmiBSD. The original's notice is kept above
//! in full (clause 3). Do not mistake it for the zlib distribution.
//!
//! A length/distance pair takes at most 48 bits of input (15 for the length code, 5 extra,
//! 15 for the distance code, 13 extra), so with 6 bytes of input available no step needs to
//! check for more; and it writes at most 258 bytes, so with 258 bytes of output space none
//! needs to check for room. With large buffers (say 16K in, 64K out) more than 95% of the
//! inflate time is spent here.
//!
//! The kernel's zlib is built with `SLOW`, so neither `inflate()` nor `inflateBack()` calls
//! this; the host tests run both decoders on the same streams.
//!
//! ## Deviations
//! - The C takes the stream and reads the buffers from it, finding the start of this
//!   `inflate()` call's output as `next_out - (start - avail_out)`; here the caller passes its
//!   `Regs` (buffers, positions, bit accumulator), whose output starts at the call's start.
//!   `start` is still passed, and `beg` computed from it.
//! - `from` (where a match is copied from) is an index plus a flag saying whether it is in the
//!   window or in the output. `window_in_output` is for `inflateBack()`, whose window is its
//!   output buffer: window offsets then index the output.
//! - The unrolled copy loops (three bytes per turn) are one copy of `len` bytes; a copy from
//!   the output goes byte by byte, front to back, because the source may overlap what is being
//!   written (a distance shorter than the length repeats bytes).
//! - `ASMINF` (an assembler `inflate_fast`), `INFLATE_STRICT` and
//!   `INFLATE_ALLOW_INVALID_DISTANCE_TOOFAR_ARRR` are not defined in the kernel build; only the
//!   C code compiled without them is ported. `Tracevv` is dropped (no `ZLIB_DEBUG`).

use crate::inflate::{InflateMode, InflateState, Regs, copy_forward, small_msg};
use crate::zlib::ZStream;
use crate::zutil::zassert;

/// Copy `len` bytes to `output[*out..]` from `from` in `window`, or, when `window` is `None`,
/// from `output[from..]`, front to back (the regions may overlap, as a match needs).
fn copy_bytes(output: &mut [u8], out: &mut usize, window: Option<&[u8]>, from: usize, len: usize) {
    match window {
        Some(window) => {
            output[*out..*out + len].copy_from_slice(&window[from..from + len]);
            *out += len;
        }
        None => copy_forward(output, out, from, len),
    }
}

/// `inflate_fast`: decode literal, length and distance codes and write out the resulting
/// literals and matches until either not enough input or output is available, an
/// end-of-block is reached, or a data error is found.
///
/// Entry assumptions: `state.mode` is `LEN`; at least 6 bytes of input and 258 of output are
/// available in `r`; `start` (the output space at the start of the `inflate()` call, the
/// window size for `inflateBack()`) is at least the space left; fewer than 8 bits are in the
/// accumulator. On return, `state.mode` is `LEN` (out of input or output), `TYPE` (end of
/// block) or `BAD` (data error, `strm.msg` set).
pub(crate) fn inflate_fast(
    strm: &mut ZStream<'_>,
    state: &mut InflateState,
    r: &mut Regs<'_, '_>,
    start: usize,
    window_in_output: bool,
) {
    zassert!(
        state.mode == InflateMode::LEN,
        "inflate_fast: mode is not LEN"
    );
    zassert!(
        r.have() >= 6 && r.left() >= 258,
        "inflate_fast: buffers too short"
    );

    // copy state to local variables
    let input = r.input;
    #[cfg(test)]
    let entry = r.next; // for the check below
    let mut in_ = r.next; // local next_in
    let last = input.len() - 5; // have enough input while in_ < last
    let left = r.left();
    let output = &mut *r.output;
    let mut out = r.put; // local next_out
    let beg = out - (start - left); // inflate()'s initial next_out
    let end = output.len() - 257; // while out < end, enough space available
    let wsize = state.wsize as usize; // window size or zero if not using window
    let whave = state.whave as usize; // valid bytes in the window
    let wnext = state.wnext as usize; // window write index
    let window: Option<&[u8]> = if window_in_output {
        None
    } else {
        Some(state.window.as_deref().unwrap_or(&[]))
    };
    let mut hold = r.hold; // local hold
    let mut bits = r.bits; // local bits
    let lcode = state.table(state.lencode); // local lencode
    let dcode = state.table(state.distcode); // local distcode
    let lmask: u64 = (1 << state.lenbits) - 1; // mask for first level of length codes
    let dmask: u64 = (1 << state.distbits) - 1; // mask for first level of distance codes
    let mut mode = None; // the mode to leave in, when not LEN
    let mut msg = None;

    // decode literals and length/distances until end-of-block or not enough input data or
    // output space
    'decode: loop {
        if bits < 15 {
            hold += u64::from(input[in_]) << bits;
            in_ += 1;
            bits += 8;
            hold += u64::from(input[in_]) << bits;
            in_ += 1;
            bits += 8;
        }
        let mut here = lcode[(hold & lmask) as usize];
        // dolen:
        loop {
            let op = u32::from(here.bits);
            hold >>= op;
            bits -= op;
            let op = u32::from(here.op);
            if op == 0 {
                // literal
                output[out] = here.val as u8;
                out += 1;
                break;
            }
            if op & 16 != 0 {
                // length base
                let mut len = usize::from(here.val);
                let op = op & 15; // number of extra bits
                if op != 0 {
                    if bits < op {
                        hold += u64::from(input[in_]) << bits;
                        in_ += 1;
                        bits += 8;
                    }
                    len += (hold as usize) & ((1 << op) - 1);
                    hold >>= op;
                    bits -= op;
                }
                if bits < 15 {
                    hold += u64::from(input[in_]) << bits;
                    in_ += 1;
                    bits += 8;
                    hold += u64::from(input[in_]) << bits;
                    in_ += 1;
                    bits += 8;
                }
                let mut here = dcode[(hold & dmask) as usize];
                // dodist:
                loop {
                    let op = u32::from(here.bits);
                    hold >>= op;
                    bits -= op;
                    let op = u32::from(here.op);
                    if op & 16 != 0 {
                        // distance base
                        let mut dist = usize::from(here.val);
                        let op = op & 15; // number of extra bits
                        if bits < op {
                            hold += u64::from(input[in_]) << bits;
                            in_ += 1;
                            bits += 8;
                            if bits < op {
                                hold += u64::from(input[in_]) << bits;
                                in_ += 1;
                                bits += 8;
                            }
                        }
                        dist += (hold as usize) & ((1 << op) - 1);
                        // (INFLATE_STRICT is not defined: no check against dmax)
                        hold >>= op;
                        bits -= op;
                        let op = out - beg; // max distance in output
                        if dist > op {
                            // see if copy from window
                            let mut op = dist - op; // distance back in window
                            if op > whave && state.sane {
                                msg = Some(small_msg("invalid distance too far back"));
                                mode = Some(InflateMode::BAD);
                                break 'decode;
                            }
                            // (INFLATE_ALLOW_INVALID_DISTANCE_TOOFAR_ARRR is not defined)
                            let mut from; // where to copy match from
                            let mut from_window = true;
                            if wnext == 0 {
                                // very common case
                                from = wsize - op;
                                if op < len {
                                    // some from window
                                    len -= op;
                                    copy_bytes(output, &mut out, window, from, op);
                                    from = out - dist; // rest from output
                                    from_window = false;
                                }
                            } else if wnext < op {
                                // wrap around window
                                from = wsize + wnext - op;
                                op -= wnext;
                                if op < len {
                                    // some from end of window
                                    len -= op;
                                    copy_bytes(output, &mut out, window, from, op);
                                    from = 0;
                                    if wnext < len {
                                        // some from start of window
                                        len -= wnext;
                                        copy_bytes(output, &mut out, window, from, wnext);
                                        from = out - dist; // rest from output
                                        from_window = false;
                                    }
                                }
                            } else {
                                // contiguous in window
                                from = wnext - op;
                                if op < len {
                                    // some from window
                                    len -= op;
                                    copy_bytes(output, &mut out, window, from, op);
                                    from = out - dist; // rest from output
                                    from_window = false;
                                }
                            }
                            let src = if from_window { window } else { None };
                            copy_bytes(output, &mut out, src, from, len);
                        } else {
                            // copy direct from output (the minimum length is three)
                            let from = out - dist;
                            copy_bytes(output, &mut out, None, from, len);
                        }
                        break;
                    } else if op & 64 == 0 {
                        // 2nd level distance code
                        here = dcode[usize::from(here.val) + (hold as usize & ((1 << op) - 1))];
                        continue; // goto dodist
                    } else {
                        msg = Some(small_msg("invalid distance code"));
                        mode = Some(InflateMode::BAD);
                        break 'decode;
                    }
                }
                break;
            } else if op & 64 == 0 {
                // 2nd level length code
                here = lcode[usize::from(here.val) + (hold as usize & ((1 << op) - 1))];
                continue; // goto dolen
            } else if op & 32 != 0 {
                // end-of-block
                mode = Some(InflateMode::TYPE);
                break 'decode;
            } else {
                msg = Some(small_msg("invalid literal/length code"));
                mode = Some(InflateMode::BAD);
                break 'decode;
            }
        }
        if !(in_ < last && out < end) {
            break;
        }
    }

    // return unused bytes (on entry, bits < 8, so in_ won't go too far back; when inflate()
    // left 8 or more bits in LEN, it was because the next code is longer than that, and that
    // code is consumed here first)
    let len = (bits >> 3) as usize;
    in_ -= len;
    zassert!(in_ >= entry, "inflate_fast: returned input it did not read");
    bits -= (len as u32) << 3;
    hold &= (1 << bits) - 1;

    // update state and return
    r.next = in_;
    r.put = out;
    r.hold = hold;
    r.bits = bits;
    if let Some(mode) = mode {
        state.mode = mode;
    }
    if msg.is_some() {
        strm.msg = msg;
    }
}
/* </CODE> */
