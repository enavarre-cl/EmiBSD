/* <CODE> */
//! arm64-only drivers: OpenBSD `sys/arch/arm64/dev/`.
//!
//! `mainbus` is the root bus (M7b), `ampintc` the GICv2 interrupt controller (M4), `agtimer`
//! the ARM generic timer (M5); M12 adds `simplebus` (the device tree's `simple-bus`, also
//! the GIC's children) and `pci_machdep` (`arch/arm64/dev/pci_machdep.c`).
//! M14 adds arm64 ACPI's `acpiiort` (the IORT) and `acpipci` (the ACPI PCI host bridges).
//! M16f adds `smmu` (the ARM System MMU, `smmureg`/`smmuvar` its headers) with its device-tree
//! and IORT attachments `smmu_fdt` and `smmu_acpi`.
//! `agintc` (GICv3) and the rest attach with their milestones.

pub mod acpiiort;
pub mod acpipci;
pub mod agtimer;
pub mod ampintc;
pub mod efi_machdep;
pub mod mainbus;
pub mod pci_machdep;
pub mod simplebus;
pub mod smmu;
pub mod smmureg;
pub mod smmuvar;
/* </CODE> */
