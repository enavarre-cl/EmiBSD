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
//! Device-tree attachments of the generic drivers: OpenBSD `sys/dev/fdt/`.
//!
//! `pluart_fdt` finds the console PL011 (M4); the rest attach with autoconfiguration:
//! `virtio_mmio` is the virtio transport of the `virtio,mmio` nodes (`virtio* at fdt?`);
//! `psci` (with `pscivar`) is PSCI, `psci* at fdt? early 1` (M13); `simplefb` the
//! firmware's frame buffer, `simplefb* at fdt?` (M13); `pciecam` is the generic ECAM PCIe host bridge (`pciecam* at fdt?`, M12), which
//! `files.arm64` lists: it is compiled where cfg `machine_pci_chipset` is set (`sys/build.rs`);
//! `plgpio` is the PL061 GPIO controller (`plgpio* at fdt? early 1`) and `gpiokeys` the keys
//! on GPIO pins (`gpiokeys* at fdt?`, M16f).
//! `ipmi_fdt` is ipmi(4) on an `ipmi-kcs` node (`ipmi* at fdt?`, M16e).

pub mod gpiokeys;
pub mod ipmi_fdt;
#[cfg(machine_pci_chipset)]
pub mod pciecam;
pub mod plgpio;
pub mod plrtc;
pub mod pluart_fdt;
pub mod psci;
pub mod pscivar;
pub mod simplefb;
pub mod virtio_mmio;
/* </CODE> */
