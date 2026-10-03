//! Kernel configuration: OpenBSD `sys/arch/arm64/conf/`.
//!
//! `kernel.ld` is the linker script (`sys/build.rs`); `ioconf` stands in for the `ioconf.c`
//! that `config(8)` would generate from `GENERIC`.

pub mod ioconf;
