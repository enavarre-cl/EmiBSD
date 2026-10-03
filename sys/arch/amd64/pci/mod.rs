//! amd64 PCI glue: OpenBSD `sys/arch/amd64/pci/`. `pci_machdep` is configuration space
//! access, tags, interrupt mapping and `pci_bus_dma_tag`; the host bridge (`pchb`), the ISA
//! bridge (`pcib`) and `acpipci` come with their drivers.

pub mod pci_machdep;
