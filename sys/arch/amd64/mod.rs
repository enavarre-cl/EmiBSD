//! amd64 (x86_64) machine-dependent code: OpenBSD `sys/arch/amd64/`.
//!
//! Layout follows OpenBSD: `amd64/` for `.c`/`.S` ports (`locore`, `machdep`, `pmap`, `trap`),
//! `include/` for header ports, `conf/kernel.ld` for the linker script.

/// The amd64 implementation of the machine interface.
pub struct Machine;

impl crate::machine::api::MachineInfo for Machine {
    const MACHINE: &'static str = "amd64";
}
