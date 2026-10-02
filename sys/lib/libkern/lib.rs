//! Freestanding kernel C library: OpenBSD `sys/lib/libkern`.
//!
//! One module per C file (`strlcpy.c` → `strlcpy.rs`), each function re-exported at the crate
//! root so callers write `libkern::strlcpy(..)`. Functions whose semantics `core` already
//! provides exactly (`memcpy`, `strlen`, `qsort`) are not ported; see `ports.toml`
//! (`skipped: provided-by-core`). This crate has no dependencies and must stay that way.
//!
//! String arguments are byte slices: a C string ends at its first NUL or at the end of the
//! slice, whichever comes first, and a destination's size is its slice length.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod crc32c;
pub mod explicit_bzero;
pub mod strlcat;
pub mod strlcpy;
pub mod strnlen;
pub mod timingsafe_bcmp;

pub use crc32c::crc32c;
pub use explicit_bzero::explicit_bzero;
pub use strlcat::strlcat;
pub use strlcpy::strlcpy;
pub use strnlen::strnlen;
pub use timingsafe_bcmp::timingsafe_bcmp;
