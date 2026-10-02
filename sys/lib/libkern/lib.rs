//! Freestanding kernel C library: OpenBSD `sys/lib/libkern`.
//!
//! One module per C file (`strlcpy.c` → `strlcpy.rs`). Functions whose semantics `core` already
//! provides exactly (`memcpy`, `strlen`, `qsort`) are not ported; see `ports.toml`
//! (`skipped: provided-by-core`). This crate has no dependencies and must stay that way.

#![no_std]

#[cfg(test)]
extern crate std;
