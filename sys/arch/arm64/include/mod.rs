//! Header ports: OpenBSD `sys/arch/arm64/include/*.h`.
//!
//! Constants, `#[repr(C)]` hardware structs and inline accessors only; never state.

pub mod _types;
pub mod bus;
pub mod cpu;
pub mod frame;
pub mod param;
pub mod pmap;
pub mod vmparam;
