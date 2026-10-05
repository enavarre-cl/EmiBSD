//! amd64 PCI glue: OpenBSD `sys/arch/amd64/pci/`. `pci_machdep` is configuration space
//! access, tags, interrupt mapping and `pci_bus_dma_tag`; `acpipci` the host bridges ACPI
//! describes (M13); the host bridge (`pchb`) and the ISA bridge (`pcib`) come with their
//! drivers.

pub mod acpipci;
pub mod pci_machdep;
