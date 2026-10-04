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
pub mod crc32;
pub mod zconf;
pub mod zlib;
pub mod zopenbsd;
pub mod zutil;

pub use adler32::{adler32, adler32_combine};
pub use crc32::crc32;
pub use zconf::{MAX_MEM_LEVEL, MAX_WBITS};
pub use zlib::*;
pub use zutil::{zError, zlibCompileFlags, zlibVersion};
