/*	$OpenBSD: cread.c,v 1.15 2016/09/18 15:14:52 jsing Exp $	*/
/*	$NetBSD: cread.c,v 1.2 1997/02/04 18:38:20 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996
 *	Matthias Drochner.  All rights reserved.
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
 *
 */

/* support for compressed bootfiles
(only read)
replaces open(), close(), read(), lseek().
original libsa open(), close(), read(), lseek() are called
as oopen(), oclose(), oread() resp. olseek().
compression parts stripped from zlib:gzio.c
*/

/* gzio.c -- IO on .gz files
 * Copyright (C) 1995-1996 Jean-loup Gailly.
 * For conditions of distribution and use, see copyright notice in zlib.h
 */
/* </LICENSES> */

//! Support for compressed boot files (read only): `open()`, `close()`, `read()` and `lseek()`
//! that inflate a gzip file transparently and pass any other file through.
//!
//! Upstream: sys/lib/libsa/cread.c @ 3ce1f3f79392
//!
//! The original libsa `open()`, `close()`, `read()` and `lseek()` are called as `oopen()`,
//! `oclose()`, `oread()` and `olseek()` (`__INTERNAL_LIBSA_CREAD`, which efiboot defines).
//! The compression parts are stripped from zlib's `gzio.c`. libz is `sys/lib/libz`, the
//! kernel's port, as the C takes `inflate.c` and friends from `${S}/lib/libz` (`.PATH`).
//!
//! ## Deviations
//! - `struct sd *ss[SOPEN_MAX]` is the [`StaticCell`] [`SS`] of boxed states; `zcalloc` and
//!   `zcfree` are libz's own (`zopenbsd.rs`, through the global allocator, which a standalone
//!   program points at libsa's `alloc`).
//! - zlib's `next_in` is kept as the indices `in_pos..in_end` of `inbuf`, so that
//!   `check_header` can step back over bytes it peeked at (`next_in--` in the C); the stream
//!   borrows `inbuf` and the caller's buffer only for the length of one `inflate()` call.
//! - The functions return `Result` (descriptor, count, offset) where the C returns -1 and
//!   sets `errno`; `errno` is still set where the C sets it.
//! - `read()` and `lseek()` on a descriptor opened for writing (no state, the C dereferences
//!   a stale or NULL `ss[fd]`) go to `oread()`/`olseek()`.

use alloc::boxed::Box;
use alloc::vec;

use libkern::staticcell::StaticCell;
use libz::{
    Z_DATA_ERROR, Z_DEFLATED, Z_ERRNO, Z_NO_FLUSH, Z_OK, Z_STREAM_END, ZStream, crc32, inflate,
    inflateEnd, inflateInit2, inflateReset,
};

use crate::close::oclose;
use crate::dev::{errno, set_errno};
use crate::hdr::types::Off;
use crate::lseek::olseek;
use crate::open::{FILES, oopen};
use crate::read::oread;
use crate::saerrno::Errno;
use crate::stand::{F_READ, SEEK_CUR, SEEK_END, SEEK_SET, SOPEN_MAX};

/// `EOF`: what `get_byte` returns at the end of the input.
const EOF: i32 = -1;

/// `Z_BUFSIZE`: the input buffer.
const Z_BUFSIZE: usize = 4096;

/// `gz_magic`: the gzip magic header.
const GZ_MAGIC: [i32; 2] = [0x1f, 0x8b];

/// `HEAD_CRC`: bit 1 set: header CRC present.
const HEAD_CRC: i32 = 0x02;
/// `EXTRA_FIELD`: bit 2 set: extra field present.
const EXTRA_FIELD: i32 = 0x04;
/// `ORIG_NAME`: bit 3 set: original file name present.
const ORIG_NAME: i32 = 0x08;
/// `COMMENT`: bit 4 set: file comment present.
const COMMENT: i32 = 0x10;
/// `RESERVED`: bits 5..7: reserved.
const RESERVED: i32 = 0xE0;

/// `DUMMYBUFSIZE`: the scratch buffer of a forward seek.
const DUMMYBUFSIZE: usize = 256;

/// `struct sd`: the state of one open file.
struct Sd {
    /// `stream`; its `next_in`/`next_out` are empty between calls.
    stream: ZStream<'static>,
    /// `z_err`: error code for last stream operation.
    z_err: i32,
    /// `z_eof`: set if end of input file.
    z_eof: bool,
    /// `fd`.
    fd: usize,
    /// `inbuf`: input buffer.
    inbuf: Box<[u8]>,
    /// `next_in`, as an index into `inbuf`.
    in_pos: usize,
    /// The end of the valid input in `inbuf` (`next_in + avail_in`).
    in_end: usize,
    /// `crc`: crc32 of uncompressed data.
    crc: u32,
    /// `transparent`: true if input file is not a .gz file.
    transparent: bool,
}

impl Sd {
    /// A fresh state for `fd` over `inbuf` (`bzero` and `inflateInit2(-15)`), or `None` if
    /// zlib could not set up.
    fn new(fd: usize, inbuf: Box<[u8]>) -> Option<Self> {
        let mut s = Self {
            stream: ZStream::new(),
            z_err: Z_OK,
            z_eof: false,
            fd,
            inbuf,
            in_pos: 0,
            in_end: 0,
            crc: 0,
            transparent: false,
        };
        if inflateInit2(&mut s.stream, -15) != Z_OK {
            return None;
        }
        Some(s)
    }

    /// `avail_in`.
    fn avail_in(&self) -> usize {
        self.in_end - self.in_pos
    }

    /// Refills `inbuf` from the file (`avail_in = oread(fd, inbuf, Z_BUFSIZE)`); false at the
    /// end of the input or on an error, which sets `z_eof` and, for an error, `z_err`.
    fn refill(&mut self) -> bool {
        set_errno(Errno(0));
        let n = oread(self.fd, &mut self.inbuf).unwrap_or(0);
        self.in_pos = 0;
        self.in_end = n;
        if n == 0 {
            self.z_eof = true;
            if errno() != Errno(0) {
                self.z_err = Z_ERRNO;
            }
            return false;
        }
        true
    }
}

/// `ss[SOPEN_MAX]`: the state of each open file, if it is being read.
static SS: StaticCell<[Option<Box<Sd>>; SOPEN_MAX]> = StaticCell::new([const { None }; SOPEN_MAX]);

/// The states.
///
/// # Safety
///
/// No other reference into [`SS`] may be live: each entry point takes it once (the
/// standalone programs are single-threaded, and `oread`/`olseek` do not come back here).
unsafe fn ss() -> &'static mut [Option<Box<Sd>>; SOPEN_MAX] {
    // SAFETY: the caller excludes every other reference.
    unsafe { SS.get_mut() }
}

/// `get_byte(s)`: the next input byte, or [`EOF`].
fn get_byte(s: &mut Sd) -> i32 {
    if s.z_eof {
        return EOF;
    }
    if s.avail_in() == 0 && !s.refill() {
        return EOF;
    }
    let c = s.inbuf[s.in_pos];
    s.in_pos += 1;
    i32::from(c)
}

/// `getLong(s)`: a 32-bit little-endian number from the input.
fn get_long(s: &mut Sd) -> u32 {
    let mut x = get_byte(s) as u32;
    x = x.wrapping_add((get_byte(s) as u32) << 8);
    x = x.wrapping_add((get_byte(s) as u32) << 16);
    let c = get_byte(s);
    if c == EOF {
        s.z_err = Z_DATA_ERROR;
    }
    x.wrapping_add((c as u32) << 24)
}

/// `check_header(s)`: check the gzip header of a gz file and skip it, or decide the file is
/// not compressed (`transparent`).
fn check_header(s: &mut Sd) {
    // Check the gzip magic header
    for (len, &magic) in GZ_MAGIC.iter().enumerate() {
        let c = get_byte(s);
        if c != magic {
            if len != 0 {
                s.in_pos -= 1;
            }
            if c != EOF {
                s.in_pos -= 1;
                s.transparent = true;
            }
            s.z_err = if s.avail_in() != 0 {
                Z_OK
            } else {
                Z_STREAM_END
            };
            return;
        }
    }
    let method = get_byte(s);
    let flags = get_byte(s);
    if method != Z_DEFLATED || (flags & RESERVED) != 0 {
        s.z_err = Z_DATA_ERROR;
        return;
    }

    // Discard time, xflags and OS code:
    for _ in 0..6 {
        let _ = get_byte(s);
    }

    if (flags & EXTRA_FIELD) != 0 {
        // skip the extra field
        let mut len = get_byte(s) as u32;
        len = len.wrapping_add((get_byte(s) as u32) << 8);
        // len is garbage if EOF but the loop below will quit anyway
        while len != 0 && get_byte(s) != EOF {
            len -= 1;
        }
    }
    if (flags & ORIG_NAME) != 0 {
        // skip the original file name
        loop {
            let c = get_byte(s);
            if c == 0 || c == EOF {
                break;
            }
        }
    }
    if (flags & COMMENT) != 0 {
        // skip the .gz file comment
        loop {
            let c = get_byte(s);
            if c == 0 || c == EOF {
                break;
            }
        }
    }
    if (flags & HEAD_CRC) != 0 {
        // skip the header crc
        for _ in 0..2 {
            let _ = get_byte(s);
        }
    }
    s.z_err = if s.z_eof { Z_DATA_ERROR } else { Z_OK };
}

/// `open(fname, mode)`: [`oopen`], plus a decompression state for a file opened to read.
pub fn open(fname: &[u8], mode: i32) -> Result<usize, Errno> {
    let fd = oopen(fname, mode)?;
    if mode != 0 {
        // compression only for read
        return Ok(fd);
    }

    let mut s = match Sd::new(fd, vec![0u8; Z_BUFSIZE].into_boxed_slice()) {
        Some(s) => Box::new(s),
        None => {
            let _ = oclose(fd);
            return Err(Errno::ENOMEM);
        }
    };
    check_header(&mut s); // skip the .gz header
    // SAFETY: entry point; the only reference into SS here.
    unsafe { ss()[fd] = Some(s) };
    Ok(fd)
}

/// `close(fd)`.
pub fn close(fd: usize) -> Result<(), Errno> {
    if fd >= SOPEN_MAX {
        set_errno(Errno::EBADF);
        return Err(Errno::EBADF);
    }
    // SAFETY: a copy of one field; no reference into FILES outlives the expression.
    let flags = unsafe { FILES.get()[fd].f_flags };
    if (flags & F_READ) == 0 {
        return oclose(fd);
    }

    // SAFETY: entry point; the only reference into SS here.
    if let Some(mut s) = unsafe { ss()[fd].take() } {
        inflateEnd(&mut s.stream);
    }

    oclose(fd)
}

/// `read(fd, buf, len)`: decompress (or copy) up to `buf.len()` bytes.
pub fn read(fd: usize, buf: &mut [u8]) -> Result<usize, Errno> {
    if fd >= SOPEN_MAX {
        set_errno(Errno::EBADF);
        return Err(Errno::EBADF);
    }
    // SAFETY: entry point; `oread`, the only libsa routine called below, does not touch SS.
    let Some(s) = (unsafe { ss()[fd].as_deref_mut() }) else {
        return oread(fd, buf);
    };
    let len = buf.len();

    if s.z_err == Z_DATA_ERROR || s.z_err == Z_ERRNO {
        return Err(errno());
    }
    if s.z_err == Z_STREAM_END {
        return Ok(0); // EOF
    }

    let mut out = 0usize; // next_out - buf
    let mut start = 0usize; // starting point for crc computation

    while out < len {
        if s.transparent {
            // Copy first the lookahead bytes:
            let n = s.avail_in().min(len - out);
            if n > 0 {
                buf[out..out + n].copy_from_slice(&s.inbuf[s.in_pos..s.in_pos + n]);
                out += n;
                s.in_pos += n;
            }
            if out < len {
                set_errno(Errno(0));
                match oread(fd, &mut buf[out..]) {
                    Ok(0) | Err(_) => {
                        s.z_eof = true;
                        if errno() != Errno(0) {
                            s.z_err = Z_ERRNO;
                        }
                    }
                    Ok(n) => out += n,
                }
            }
            s.stream.total_in += out as u64;
            s.stream.total_out += out as u64;
            if out == 0 {
                s.z_eof = true;
            }
            return Ok(out);
        }

        if s.avail_in() == 0 && !s.z_eof && !s.refill() && s.z_err == Z_ERRNO {
            break;
        }

        let (consumed, produced) = {
            let input = &s.inbuf[s.in_pos..s.in_end];
            let output = &mut buf[out..];
            let (ilen, olen) = (input.len(), output.len());
            // SAFETY: the stream keeps these borrows only until the two lines after
            // `inflate`, which put empty slices back; `inbuf` and `buf` outlive the call and
            // nothing else touches them meanwhile.
            unsafe {
                s.stream.next_in = core::mem::transmute::<&[u8], &'static [u8]>(input);
                s.stream.next_out = core::mem::transmute::<&mut [u8], &'static mut [u8]>(output);
            }
            s.z_err = inflate(&mut s.stream, Z_NO_FLUSH);
            let left = (s.stream.avail_in(), s.stream.avail_out());
            s.stream.next_in = &[];
            s.stream.next_out = &mut [];
            (ilen - left.0, olen - left.1)
        };
        s.in_pos += consumed;
        out += produced;

        if s.z_err == Z_STREAM_END {
            // Check CRC and original size
            s.crc = crc32(s.crc, &buf[start..out]);
            start = out;

            if get_long(s) != s.crc {
                s.z_err = Z_DATA_ERROR;
            } else {
                let _ = get_long(s);

                // The uncompressed length returned by above getlong() may be different from
                // s->stream.total_out in case of concatenated .gz files. Check for such files:
                check_header(s);
                if s.z_err == Z_OK {
                    let total_in = s.stream.total_in;
                    let total_out = s.stream.total_out;

                    inflateReset(&mut s.stream);
                    s.stream.total_in = total_in;
                    s.stream.total_out = total_out;
                    s.crc = crc32(0, &[]);
                }
            }
        }
        if s.z_err != Z_OK || s.z_eof {
            break;
        }
    }
    s.crc = crc32(s.crc, &buf[start..out]);

    Ok(out)
}

/// `lseek(fd, offset, where)`: on a compressed file, only forwards (by reading) or back to
/// the start (by starting over); `SEEK_END` is refused.
pub fn lseek(fd: usize, offset: Off, whence: i32) -> Result<Off, Errno> {
    if fd >= SOPEN_MAX {
        set_errno(Errno::EBADF);
        return Err(Errno::EBADF);
    }
    // SAFETY: a copy of one field; no reference into FILES outlives the expression.
    let flags = unsafe { FILES.get()[fd].f_flags };
    if (flags & F_READ) == 0 {
        return olseek(fd, offset, whence);
    }

    // SAFETY: entry point; `olseek` does not touch SS, and the forward seek's `read` takes
    // SS again only after this reference's last use.
    let Some(s) = (unsafe { ss()[fd].as_deref_mut() }) else {
        return olseek(fd, offset, whence);
    };

    if s.transparent {
        let res = olseek(fd, offset, whence);
        if res.is_ok() {
            // make sure the lookahead buffer is invalid
            s.in_pos = s.in_end;
        }
        return res;
    }

    let mut offset = offset;
    match whence {
        SEEK_CUR | SEEK_SET => {
            if whence == SEEK_CUR {
                offset += s.stream.total_out as Off;
            }

            // if seek backwards, simply start from the beginning
            if offset < s.stream.total_out as Off {
                olseek(fd, 0, SEEK_SET)?;
                // ??? perhaps fallback to close / open

                inflateEnd(&mut s.stream);

                // don't allocate again; this resets total_out to 0!
                let inbuf = core::mem::take(&mut s.inbuf);
                match Sd::new(fd, inbuf) {
                    Some(fresh) => *s = fresh,
                    None => return Err(Errno::ENOMEM),
                }
                check_header(s); // skip the .gz header
            }

            // to seek forwards, throw away data
            let mut toskip = offset - s.stream.total_out as Off;
            while toskip > 0 {
                let mut dummybuf = [0u8; DUMMYBUFSIZE];
                let len = (toskip as usize).min(DUMMYBUFSIZE);
                match read(fd, &mut dummybuf[..len]) {
                    Ok(0) => {
                        set_errno(Errno::EINVAL);
                        return Err(Errno::EINVAL);
                    }
                    Ok(n) => toskip -= n as Off,
                    Err(e) => return Err(e),
                }
            }
            Ok(offset)
        }
        SEEK_END => {
            set_errno(Errno::EOFFSET);
            Err(Errno::EOFFSET)
        }
        _ => {
            set_errno(Errno::EINVAL);
            Err(Errno::EINVAL)
        }
    }
}
