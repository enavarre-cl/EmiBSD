//! arm64-only drivers: OpenBSD `sys/arch/arm64/dev/`.
//!
//! `ampintc` is the GICv2 interrupt controller (M4). `agintc` (GICv3), `agtimer` and the
//! rest attach with their milestones.

pub mod ampintc;
