//! zlib as the kernel uses it: OpenBSD `sys/lib/libz`.
//!
//! Upstream: sys/lib/libz/crc32.c @ 3ce1f3f79392
//!
//! OpenBSD builds `libz` as a library of its own, and the kernel links only the parts it calls:
//! today `crc32` (`subr_disk.c` checksums GPT headers and partition entries with it). This crate
//! ports the same subset, one module per C file (`crc32.c` → `crc32.rs`), each function
//! re-exported at the crate root. The rest of zlib (deflate, inflate, adler32, ...) is not ported.
//! This crate has no dependencies and must stay that way.
//!
//! The files here are under the zlib licence, not ISC, and are altered source versions (rewrites
//! in Rust); each file says so and carries the notice in full.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod crc32;

pub use crc32::crc32;
