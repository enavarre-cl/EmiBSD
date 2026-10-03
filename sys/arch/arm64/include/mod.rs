//! Header ports: OpenBSD `sys/arch/arm64/include/*.h`.
//!
//! Constants, `#[repr(C)]` hardware structs and inline accessors only; never state.

pub mod _types;
pub mod armreg;
pub mod bus;
pub mod cpu;
pub mod db_machdep;
pub mod fdt;
pub mod frame;
pub mod intr;
pub mod mutex;
pub mod param;
pub mod pcb;
pub mod pmap;
pub mod proc;
pub mod pte;
pub mod reg;
pub mod timetc;
pub mod vmparam;
