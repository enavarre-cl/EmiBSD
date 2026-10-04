//! arm64-only drivers: OpenBSD `sys/arch/arm64/dev/`.
//!
//! `mainbus` is the root bus (M7b), `ampintc` the GICv2 interrupt controller (M4), `agtimer`
//! the ARM generic timer (M5).
//! `agintc` (GICv3) and the rest attach with their milestones.

pub mod agtimer;
pub mod ampintc;
pub mod efi_machdep;
pub mod mainbus;
