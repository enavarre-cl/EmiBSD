//! amd64 machine-dependent sources: OpenBSD `sys/arch/amd64/amd64/*.c` and `*.S`.
//!
//! `machdep` (boot, delay, the early init, the descriptor tables), `cpu` (the per-CPU pages
//! and MSRs), `locore` and `vector` (the assembly glue and the exception stubs), `trap`,
//! `bus_space`, `consinit`, `autoconf` (`cold`), `db_trace` and `db_interface` (ddb-lite)
//! are partial ports; `qemu` is the emulator exit under feature `qemu`, a project helper
//! (`ports.toml`, `[[extra]]`).

pub mod autoconf;
pub mod bus_space;
pub mod consinit;
pub mod cpu;
pub mod db_interface;
pub mod db_trace;
pub mod i8259;
pub mod intr;
pub mod lapic;
pub mod locore;
pub mod machdep;
pub mod pmap;
#[cfg(feature = "qemu")]
pub mod qemu;
pub mod spl;
pub mod trap;
pub mod vector;
pub mod vm_machdep;
