//! amd64 machine-dependent sources: OpenBSD `sys/arch/amd64/amd64/*.c` and `*.S`.
//!
//! `machdep` (boot, delay, the early init), `bus_space`, `consinit`, `autoconf` (`cold`),
//! `db_trace` and `db_interface` (ddb-lite) are partial ports; `qemu` is the emulator exit
//! under feature `qemu`, a project helper (`ports.toml`, `[[extra]]`).

pub mod autoconf;
pub mod bus_space;
pub mod consinit;
pub mod db_interface;
pub mod db_trace;
pub mod machdep;
#[cfg(feature = "qemu")]
pub mod qemu;
