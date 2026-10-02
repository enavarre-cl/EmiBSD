//! arm64 (aarch64) machine-dependent code: OpenBSD `sys/arch/arm64/`.
//!
//! Layout follows OpenBSD: `arm64/` for `.c`/`.S` ports (`locore`, `machdep`, `pmap`, `trap`),
//! `include/` for header ports, `dev/` for arch-only drivers (GIC, generic timer),
//! `conf/kernel.ld` for the linker script.

/// The arm64 implementation of the machine interface.
pub struct Machine;

impl crate::machine::api::MachineInfo for Machine {
    const MACHINE: &'static str = "arm64";
}
