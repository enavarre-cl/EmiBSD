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
//! arm64-only drivers: OpenBSD `sys/arch/arm64/dev/`.
//!
//! `mainbus` is the root bus (M7b), `ampintc` the GICv2 interrupt controller (M4), `agtimer`
//! the ARM generic timer (M5); M12 adds `simplebus` (the device tree's `simple-bus`, also
//! the GIC's children) and `pci_machdep` (`arch/arm64/dev/pci_machdep.c`).
//! M14 adds arm64 ACPI's `acpiiort` (the IORT) and `acpipci` (the ACPI PCI host bridges).
//! M16f adds `agintc`, the GICv3 interrupt controller with its ITS (`agintcmsi`), and `smmu`
//! (the ARM System MMU, `smmureg`/`smmuvar` its headers) with its device-tree and IORT
//! attachments `smmu_fdt` and `smmu_acpi`. The rest attach with their milestones.

pub mod acpiiort;
pub mod acpipci;
pub mod agintc;
pub mod agtimer;
pub mod ampintc;
pub mod efi_machdep;
pub mod mainbus;
pub mod pci_machdep;
pub mod simplebus;
pub mod smmu;
pub mod smmu_acpi;
pub mod smmu_fdt;
pub mod smmureg;
pub mod smmuvar;
/* </CODE> */
