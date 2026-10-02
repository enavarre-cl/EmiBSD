//! Header ports: OpenBSD `sys/arch/amd64/include/*.h`.
//!
//! Constants, `#[repr(C)]` hardware structs and inline accessors only; never state.

pub mod _types;
pub mod cpufunc;
pub mod frame;
pub mod param;
pub mod pio;
pub mod vmparam;
