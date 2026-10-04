/* $OpenBSD: xform_ipcomp.c,v 1.8 2019/01/09 12:11:38 mpi Exp $ */

/* <LICENSES> */
/*
 * Copyright (c) 2001 Jean-Jacques Bernard-Gundol (jj@wabbitt.org)
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *   notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *   notice, this list of conditions and the following disclaimer in the
 *   documentation and/or other materials provided with the distribution.
 * 3. The name of the author may not be used to endorse or promote products
 *   derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! A wrapper around the deflate compression functions of zlib (`sys/lib/libz`) for IPComp:
//! `deflate_global` compresses or decompresses one block of data in one call.
//!
//! Upstream: sys/crypto/xform_ipcomp.c @ 3ce1f3f79392
//!
//! IPComp (RFC 2393, RFC 2394) carries raw deflate streams: no zlib header and no Adler-32.
//! Compression uses a 4 KiB window (`window_deflate`, -12), decompression accepts any window
//! (`window_inflate`, -`MAX_WBITS`). The output is gathered in up to `ZBUF - 1` buffers of
//! growing size (the first is 4 times the input when decompressing, the next ones double up
//! to 32 KiB) and then copied into one buffer of exactly the produced length.
//!
//! ## Deviations
//! - `deflate_global` returns the output as a `Vec<u8>` (its length is the C's return value)
//!   or an error where the C returns 0 with `*out` set to `NULL`: `ENOMEM` when a buffer
//!   cannot be allocated, `EINVAL` when zlib fails or the buffers run out. The caller
//!   (`swcr_compdec`) turns either into `EINVAL`, as it does with the C's 0. `decomp` is a
//!   `bool`.
//! - `struct deflate_buf` is an array of `Vec<u8>` (`out` and `size` are the vector, `flag`
//!   is "one of the first `n` buffers"); the buffers are freed when the function returns, on
//!   every path, as the C frees them by hand. The memory comes from the global allocator
//!   (malloc(9)), fallibly (`try_reserve_exact`, the C's `M_NOWAIT`); `M_CRYPTO_DATA` has no
//!   counterpart.
//! - `window_inflate` and `window_deflate` are constants: the C's are writable globals that
//!   nothing writes.

use alloc::vec::Vec;

use crate::sys::errno::Errno;
use libz::{
    MAX_WBITS, Z_DEFAULT_COMPRESSION, Z_DEFAULT_STRATEGY, Z_FINISH, Z_OK, Z_PARTIAL_FLUSH,
    Z_STREAM_END, ZStream, deflate, deflateEnd, deflateInit2, inflate, inflateEnd, inflateInit2,
};

/// `Z_METHOD`: the deflate compression method.
pub const Z_METHOD: i32 = 8;
/// `Z_MEMLEVEL`: the memory level of the compressor.
pub const Z_MEMLEVEL: i32 = 8;
/// `ZBUF`: the number of output buffers (one stays unused, as in the C).
pub const ZBUF: usize = 10;

/// `window_inflate`: raw inflate, any window size.
#[allow(non_upper_case_globals)] // the C name
pub const window_inflate: i32 = -MAX_WBITS;
/// `window_deflate`: raw deflate with a 4 KiB window.
#[allow(non_upper_case_globals)] // the C name
pub const window_deflate: i32 = -12;

/// A zero-filled buffer of `size` bytes, or `ENOMEM` (the C's `malloc` with `M_NOWAIT`
/// returning `NULL`).
fn deflate_buf_alloc(size: usize) -> Result<Vec<u8>, Errno> {
    let mut v = Vec::new();
    v.try_reserve_exact(size).map_err(|_| Errno::ENOMEM)?;
    v.resize(size, 0);
    Ok(v)
}

/// `deflate_global`: compresses (`decomp == false`) or decompresses `data` with the deflate
/// algorithm and returns the result.
pub fn deflate_global(data: &[u8], decomp: bool) -> Result<Vec<u8>, Errno> {
    let mut buf: [Vec<u8>; ZBUF] = Default::default();
    // The C's `i`: the number of buffers in use (`buf[j].flag != 0` for `j < i`).
    let mut i = 0usize;

    let mut zbuf = ZStream::new();
    zbuf.next_in = data; // data that is going to be processed, its length the total
    let mut slots = buf.iter_mut();
    let error = deflate_global_run(&mut zbuf, &mut slots, &mut i, data.len(), decomp);
    let result = zbuf.total_out as usize;
    if decomp {
        inflateEnd(&mut zbuf);
    } else {
        deflateEnd(&mut zbuf);
    }
    drop(zbuf);
    error?;

    let mut out = Vec::new();
    out.try_reserve_exact(result).map_err(|_| Errno::ENOMEM)?;
    let mut count = result;
    for b in &buf[..i] {
        // the last buffer holds the remainder
        let n = count.min(b.len());
        out.extend_from_slice(&b[..n]);
        count -= n;
    }
    Ok(out)
}

/// The body of [`deflate_global`] up to the C's `bad:` label: initialises the stream and runs
/// it to its end, handing it a new output buffer from `slots` each time it fills one.
fn deflate_global_run<'a>(
    zbuf: &mut ZStream<'a>,
    slots: &mut core::slice::IterMut<'a, Vec<u8>>,
    i: &mut usize,
    mut size: usize,
    decomp: bool,
) -> Result<(), Errno> {
    if decomp {
        // Choose a buffer with 4x the size of the input buffer for the size of the output
        // buffer in the case of decompression. If it's not sufficient, it will need to be
        // updated while the decompression is going on.
        if size < 32 * 1024 {
            size *= 4;
        }
    }
    deflate_global_next(zbuf, slots, i, size)?;

    let error = if decomp {
        inflateInit2(zbuf, window_inflate)
    } else {
        deflateInit2(
            zbuf,
            Z_DEFAULT_COMPRESSION,
            Z_METHOD,
            window_deflate,
            Z_MEMLEVEL,
            Z_DEFAULT_STRATEGY,
        )
    };
    if error != Z_OK {
        return Err(Errno::EINVAL);
    }

    loop {
        let error = if decomp {
            inflate(zbuf, Z_PARTIAL_FLUSH)
        } else {
            deflate(zbuf, Z_FINISH)
        };
        if error == Z_STREAM_END {
            return Ok(());
        }
        if error != Z_OK {
            return Err(Errno::EINVAL);
        }
        if zbuf.avail_out() == 0 && *i < ZBUF - 1 {
            // we need more output space, allocate size
            if size < 32 * 1024 {
                size *= 2;
            }
            deflate_global_next(zbuf, slots, i, size)?;
        } else {
            return Err(Errno::EINVAL); // out of buffers
        }
    }
}

/// Allocates the next output buffer, of `size` bytes, and makes it the stream's `next_out`.
fn deflate_global_next<'a>(
    zbuf: &mut ZStream<'a>,
    slots: &mut core::slice::IterMut<'a, Vec<u8>>,
    i: &mut usize,
    size: usize,
) -> Result<(), Errno> {
    let slot = slots.next().ok_or(Errno::EINVAL)?;
    *slot = deflate_buf_alloc(size)?;
    *i += 1;
    zbuf.next_out = slot.as_mut_slice();
    Ok(())
}

#[cfg(test)]
mod tests;
