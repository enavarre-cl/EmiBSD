//! arm64 machine-dependent sources: OpenBSD `sys/arch/arm64/arm64/*.c` and `*.S`.
//!
//! Until milestone M2 only the bootstrap shortcuts live here: a polled PL011 console behind a
//! temporary device mapping and, under feature `qemu`, the emulator exit. Both are project
//! helpers (`ports.toml`, `[[extra]]`), not ports.

pub mod earlycons;
#[cfg(feature = "qemu")]
pub mod qemu;
