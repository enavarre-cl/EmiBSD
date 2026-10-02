//! Header ports: OpenBSD `sys/arch/arm64/include/*.h`.
//!
//! Constants, `#[repr(C)]` hardware structs and inline accessors only; never state.

pub mod _types;
pub mod cpu;
pub mod param;
