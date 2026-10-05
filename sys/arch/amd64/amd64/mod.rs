//! amd64 machine-dependent sources: OpenBSD `sys/arch/amd64/amd64/*.c` and `*.S`.
//!
//! `machdep` (boot, delay, the early init, the descriptor tables), `cpu` (the per-CPU pages
//! and MSRs), `locore` and `vector` (the assembly glue and the exception stubs), `trap`,
//! `bus_space`, `consinit`, `autoconf` (`cold`), `db_trace` and `db_interface` (ddb-lite)
//! are partial ports; `qemu` is the emulator exit under feature `qemu`, a project helper
//! (`ports.toml`, `[[extra]]`). `bus_dma` (M7b) is a whole port.

pub mod acpi_machdep;
pub mod autoconf;
pub mod bios;
pub mod bus_dma;
pub mod bus_space;
pub mod conf;
pub mod consinit;
pub mod copy;
pub mod cpu;
pub mod db_interface;
pub mod db_trace;
pub mod disksubr;
pub mod fpu;
#[cfg(feature = "multiprocessor")]
pub mod gdt;
pub mod i8259;
pub mod identcpu;
pub mod intr;
#[cfg(feature = "multiprocessor")]
pub mod ipi;
#[cfg(feature = "multiprocessor")]
pub mod ipifuncs;
pub mod lapic;
pub mod locore;
pub mod machdep;
pub mod mainbus;
pub mod mem;
pub mod pmap;
#[cfg(feature = "qemu")]
pub mod qemu;
pub mod spl;
pub mod trap;
pub mod tsc;
pub mod vector;
pub mod vm_machdep;
