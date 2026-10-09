/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
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
//! `if_vmxreg`) VMware's VMXNET3 NIC (`vmx* at pci?`, M13); `ichiic` (with `ichreg`) and `piixpm` (with
//! `piixreg`) the ICH and PIIX4 SMBus controllers (`ichiic* at pci?`, `piixpm* at pci?`, M16e); `ehci_pci` the EHCI front-end
//! (`ehci* at pci?`, M16b); `uhci_pci` the UHCI front-end (`uhci* at pci?`, M16b);
//! `ohci_pci` the OHCI front-end (`ohci* at pci?`, M16b). The machine side
//! (configuration access, tags, interrupts) is `machine::pci_machdep`.

pub mod ahci_pci;
pub mod auich;
pub mod azalia;
pub mod azalia_codec;
pub mod eap;
pub mod eapreg;
pub mod ehci_pci;
pub mod gcu_reg;
pub mod gcu_var;
pub mod ichiic;
pub mod ichreg;
pub mod if_dc_pci;
pub mod if_em;
pub mod if_em_hw;
pub mod if_em_osdep;
pub mod if_em_soc;
pub mod if_fxp_pci;
pub mod if_ne_pci;
pub mod if_pcn;
pub mod if_re_pci;
pub mod if_vmx;
pub mod if_vmxreg;
pub mod nvme_pci;
pub mod ohci_pci;
#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/pci/pci.c
pub mod pci;
pub mod pci_map;
pub mod pci_quirks;
pub mod pci_subr;
pub mod pcidevs;
pub mod pcireg;
pub mod pcivar;
pub mod piixpm;
pub mod piixreg;
pub mod ppb;
pub mod ppbreg;
pub mod puc;
pub mod pucdata;
pub mod pucvar;
pub mod siop_pci;
pub mod siop_pci_common;
pub mod uhci_pci;
pub mod vga_pci;
pub mod vga_pcivar;
pub mod virtio_pci;
pub mod virtio_pcireg;
pub mod xhci_pci;
/* </CODE> */
