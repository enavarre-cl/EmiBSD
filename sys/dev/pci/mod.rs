//! The PCI bus: OpenBSD `sys/dev/pci/`.
//!
//! `pcireg`, `pcivar`, `ppbreg` and `pcidevs` are the headers; `pci` the bus driver
//! (`pci* at mainbus0`), `pci_map` the BAR decoding and mapping, `pci_subr` the attach-line
//! descriptions and `pci_quirks` the multi/mono-function quirk table; `virtio_pci` (with
//! `virtio_pcireg`) is the virtio transport (`virtio* at pci?`), `nvme_pci` the NVM
//! Express front-end (`nvme* at pci?`), `ahci_pci` the AHCI SATA front-end (`ahci* at
//! pci?`), `siop_pci` (with `siop_pci_common`) the Symbios SCSI front-end (`siop* at
//! pci?`) (M13); `xhci_pci` the xHCI front-end (`xhci* at
//! pci?`), `auich` the Intel ICH AC'97 audio controller (`auich* at pci?`), `azalia` (with
//! `azalia_codec`) the HD Audio controller (`azalia* at pci?`) (M12); `puc` (with `pucvar`
//! and `pucdata`) the "universal" communication card driver (`puc* at pci?`, M13); `if_vmx` (with
//! `if_vmxreg`) VMware's VMXNET3 NIC (`vmx* at pci?`, M13). The machine side
//! (configuration access, tags, interrupts) is `machine::pci_machdep`.

pub mod ahci_pci;
pub mod auich;
pub mod azalia;
pub mod azalia_codec;
pub mod gcu_reg;
pub mod gcu_var;
pub mod if_em;
pub mod if_em_hw;
pub mod if_em_osdep;
pub mod if_em_soc;
pub mod if_re_pci;
pub mod if_vmx;
pub mod if_vmxreg;
pub mod nvme_pci;
#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/pci/pci.c
pub mod pci;
pub mod pci_map;
pub mod pci_quirks;
pub mod pci_subr;
pub mod pcidevs;
pub mod pcireg;
pub mod pcivar;
pub mod ppbreg;
pub mod puc;
pub mod pucdata;
pub mod pucvar;
pub mod siop_pci;
pub mod siop_pci_common;
pub mod vga_pci;
pub mod vga_pcivar;
pub mod virtio_pci;
pub mod virtio_pcireg;
pub mod xhci_pci;
