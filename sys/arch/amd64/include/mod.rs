//! Header ports: OpenBSD `sys/arch/amd64/include/*.h`.
//!
//! Constants, `#[repr(C)]` hardware structs and inline accessors only; never state.

pub mod _types;
pub mod cpu;
pub mod cpu_full;
pub mod cpufunc;
pub mod db_machdep;
pub mod frame;
pub mod intrdefs;
pub mod param;
pub mod pio;
pub mod pmap;
pub mod psl;
pub mod pte;
pub mod segments;
pub mod specialreg;
pub mod trap;
pub mod tss;
pub mod vmparam;
