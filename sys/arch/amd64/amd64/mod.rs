//! amd64 machine-dependent sources: OpenBSD `sys/arch/amd64/amd64/*.c` and `*.S`.
//!
//! Until milestone M2 only the bootstrap shortcuts live here: a polled COM1 console and, under
//! feature `qemu`, the emulator exit. Both are project helpers (`ports.toml`, `[[extra]]`), not
//! ports.

pub mod earlycons;
#[cfg(feature = "qemu")]
pub mod qemu;
