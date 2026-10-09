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
//! ACPI: OpenBSD `sys/dev/acpi/`. M13 brings the headers, the AML interpreter (`dsdt`), the
//! core (`acpi`, acpi0), the table checksum (`acpiutil`), the timers (`acpitimer`,
//! `acpihpet`), the MADT (`acpimadt`) and the PCI interrupt routing (`acpiprt`). M14 adds
//! the MCFG (`acpimcfg`) and arm64's console UART (`pluart_acpi`). M16e adds the DMA remapping
//! units (`acpidmar`, with `amd_iommu`) the TPM (`tpm`) and the x86 processor power management (`acpicpu_x86`).

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/acpi/acpi.c
pub mod acpi;
pub mod acpicpu_x86;
pub mod acpidev;
pub mod acpidmar;
pub mod acpihpet;
pub mod acpimadt;
pub mod acpimcfg;
pub mod acpiprt;
pub mod acpireg;
pub mod acpitimer;
pub mod acpiutil;
pub mod acpivar;
pub mod amd_iommu;
pub mod amltypes;
pub mod dsdt;
pub mod ipmi_acpi;
pub mod pluart_acpi;
pub mod tpm;
/* </CODE> */
