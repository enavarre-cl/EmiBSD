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
/* </LICENSES> */

/* <CODE> */
//! zlib as the kernel uses it: OpenBSD `sys/lib/libz`.
//!
//! Upstream: sys/lib/libz @ 3ce1f3f79392
//!
//! OpenBSD builds `libz` as a library of its own (`sys/lib/libz/Makefile`, with `-DSLOW -DSMALL
//! -DNO_GZIP`) and the kernel links what `sys/conf/files` lists: `crc32.c` always (`subr_disk.c`
//! checksums GPT headers with it), the rest for `ipsec`, `crypto`, `ppp_deflate` and `ddb`
//! (IPComp's `deflate_global`, `cryptosoft.c`, `ppp-deflate.c`, CTF sections). This crate ports
//! the same files, one module per C file (`deflate.c` and `deflate.h` → `deflate.rs`;
//! `zlib.h` → `zlib.rs`), the public functions re-exported at the crate root.
//!
//! The files here are under the zlib licence (`zopenbsd.c` is ISC), and are altered source
//! versions (rewrites in Rust); each file says so and carries the notice in full. This crate
//! depends on nothing but `alloc` and must stay that way.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod adler32;
pub mod compress;
pub mod crc32;
pub mod deflate;
pub mod infback;
mod inffast;
mod inffixed;
pub mod inflate;
mod inftrees;
mod trees;
pub mod zconf;
pub mod zlib;
pub mod zopenbsd;
pub mod zutil;

pub use adler32::{adler32, adler32_combine};
pub use compress::{compress, compress2, compressBound, compressBound_z};
pub use crc32::crc32;
pub use deflate::{
    deflate, deflateBound, deflateBound_z, deflateCopy, deflateEnd, deflateGetDictionary,
    deflateInit_, deflateInit2_, deflateParams, deflatePending, deflatePrime, deflateReset,
    deflateResetKeep, deflateSetDictionary, deflateSetHeader, deflateTune, deflateUsed,
};
pub use infback::{inflateBack, inflateBackEnd, inflateBackInit_};
pub use inflate::{
    inflate, inflateCodesUsed, inflateCopy, inflateEnd, inflateGetDictionary, inflateGetHeader,
    inflateInit_, inflateInit2_, inflateMark, inflatePrime, inflateReset, inflateReset2,
    inflateResetKeep, inflateSetDictionary, inflateSync, inflateSyncPoint, inflateUndermine,
    inflateValidate,
};
pub use zconf::{MAX_MEM_LEVEL, MAX_WBITS};
pub use zlib::*;
pub use zutil::{zError, zlibCompileFlags, zlibVersion};
/* </CODE> */
