//! arm64 machine-dependent sources: OpenBSD `sys/arch/arm64/arm64/*.c` and `*.S`.
//!
//! `machdep` (boot, the early init, `consinit`), `intr` (`delay`), `bus_space`, `db_trace` and
//! `db_interface` (ddb-lite) are partial ports; `qemu` is the emulator exit under feature
//! `qemu`, a project helper (`ports.toml`, `[[extra]]`).

pub mod bus_space;
pub mod db_interface;
pub mod db_trace;
pub mod intr;
pub mod machdep;
#[cfg(feature = "qemu")]
pub mod qemu;
