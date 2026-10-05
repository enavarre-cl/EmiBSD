//! arm64-only drivers: OpenBSD `sys/arch/arm64/dev/`.
//!
//! `mainbus` is the root bus (M7b), `ampintc` the GICv2 interrupt controller (M4), `agtimer`
//! the ARM generic timer (M5); M12 adds `simplebus` (the device tree's `simple-bus`, also
//! the GIC's children) and `pci_machdep` (`arch/arm64/dev/pci_machdep.c`).
//! `agintc` (GICv3) and the rest attach with their milestones.

pub mod agtimer;
pub mod ampintc;
pub mod efi_machdep;
pub mod mainbus;
pub mod pci_machdep;
pub mod simplebus;
