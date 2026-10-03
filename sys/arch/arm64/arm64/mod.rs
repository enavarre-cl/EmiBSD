//! arm64 machine-dependent sources: OpenBSD `sys/arch/arm64/arm64/*.c` and `*.S`.
//!
//! `machdep` (boot, the early init, `consinit`), `exception` (the vector table) and `trap`,
//! `intr` (`delay`, the IRQ/FIQ entry), `bus_space`, `db_trace` and `db_interface`
//! (ddb-lite) are partial ports; `qemu` is the emulator exit under feature `qemu`, a project
//! helper (`ports.toml`, `[[extra]]`).

pub mod ast;
pub mod autoconf;
pub mod bus_space;
pub mod copy;
pub mod copystr;
pub mod cpufunc;
pub mod cpuswitch;
pub mod db_interface;
pub mod db_trace;
pub mod exception;
pub mod fpu;
pub mod intr;
pub mod machdep;
pub mod pmap;
#[cfg(feature = "qemu")]
pub mod qemu;
pub mod syscall;
pub mod trap;
pub mod vm_machdep;
