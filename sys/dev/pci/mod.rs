//! The PCI bus: OpenBSD `sys/dev/pci/`.
//!
//! `pcireg`, `pcivar`, `ppbreg` and `pcidevs` are the headers; `pci` the bus driver
//! (`pci* at mainbus0`), `pci_map` the BAR decoding and mapping, `pci_subr` the attach-line
//! descriptions and `pci_quirks` the multi/mono-function quirk table; `virtio_pci` (with
//! `virtio_pcireg`) is the virtio transport (`virtio* at pci?`). The machine side
//! (configuration access, tags, interrupts) is `machine::pci_machdep`.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/pci/pci.c
pub mod pci;
pub mod pci_map;
pub mod pci_quirks;
pub mod pci_subr;
pub mod pcidevs;
pub mod pcireg;
pub mod pcivar;
pub mod ppbreg;
pub mod virtio_pci;
pub mod virtio_pcireg;
