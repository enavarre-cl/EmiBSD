/*	$OpenBSD: acpireg.h,v 1.65 2026/03/27 03:56:15 hshoexer Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2005 Thorsten Lockert <tholo@sigmasoft.com>
 * Copyright (c) 2005 Marco Peereboom <marco@openbsd.org>
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
//! `<dev/acpi/acpireg.h>`: ACPI table structures and register definitions.
//!
//! Upstream: sys/dev/acpi/acpireg.h @ 3ce1f3f79392
//!
//! The firmware tables (RSDP, RSDT/XSDT, FADT, MADT, HPET, MCFG, DMAR, IVRS, IORT, ...), the
//! generic address structure, the PM1/PM2 register bits, the operation-region address space
//! identifiers, the sleeping and device power states and the ACPI device identifiers.
//!
//! ## Deviations
//! - Firmware tables are `__packed` in C, so every structure here is `#[repr(C, packed)]`
//!   (alignment 1) and the C size is asserted at compile time. Members must be read by value
//!   (copy out of the packed structure), never through a reference. `struct acpi_lpit_entry`
//!   is not `__packed` in C and is plain `#[repr(C)]`; its `AcpiGas` members have alignment 1,
//!   so the layout is the same as the C's.
//! - Flexible tails keep the C's spelling: `aml[1]`, `table_offsets[1]`, `ec_id[1]` and
//!   `acpi_proc_uid_string[1]` are one-element arrays (the C `sizeof` includes that element),
//!   `its_ids[]` and `device_object_name[]` are zero-length arrays (`[T; 0]`); the data after
//!   the header is read from the table's bytes.
//! - The `#define rsdp_signature rsdp1.signature` and `#define hdr_signature hdr.signature`
//!   shorthands (`rsdp_*`, `hdr_*`) are not defined; the code names the member
//!   (`rsdp.rsdp1.signature`, `fadt.hdr.signature`).
//! - The `*_SIG` strings are byte arrays (`&[u8; 4]`, `RSDP_SIG` is `&[u8; 8]`), the form they
//!   are compared in; `ACPI_DEV_*` are `&str`. `char` members are `u8`.
//! - Anonymous members of unions (`struct { type; length; }` in `union acpidmar_entry` and
//!   `union acpi_ivrs_entry`, the `struct`s of `union acpi_ivhd_entry`) become named structures
//!   (`AcpidmarEntryHdr` as member `hdr`, `AcpiIvrsEntryHdr` as member `hdr`,
//!   `AcpiIvhdEntryResvd` ... `AcpiIvhdEntrySpecial` as the members of the same names). The
//!   unions are `#[repr(C, packed)]`; reading a member is `unsafe` and sound only when the
//!   entry type byte says so (callers' `// SAFETY:`).
//! - `GAS_*` (address space ids and access sizes) and `ACPI_OPREG_*` are `i32`: the C passes
//!   them as `int`. Register-bit constants take the width of the register they test
//!   (`ACPI_PM1_*` are `u16`, `FADT_*` flags `u32`, `FADT_LEGACY_DEVICES` .. `FADT_NO_MSI`
//!   `u16`, the `iapc_boot_arch` bits). `ACPI_STATE_*` are `u8`.
//! - Function-like macros are snake-case `const fn`s (`ACPI_PCI_SEG(addr)` is
//!   `acpi_pci_seg`), with the C casts as the return type.
//! - `ACPI_MADT_GICC` has no member in `union acpi_madt_entry` in C either; `AcpiMadtGicc` is
//!   defined as a structure only.

use core::mem::size_of;

/// `RSDP_SIG`.
pub const RSDP_SIG: &[u8; 8] = b"RSD PTR ";
/// `RSDT_SIG`.
pub const RSDT_SIG: &[u8; 4] = b"RSDT";
/// `XSDT_SIG`.
pub const XSDT_SIG: &[u8; 4] = b"XSDT";
/// `GAS_SYSTEM_MEMORY`.
pub const GAS_SYSTEM_MEMORY: i32 = 0;
/// `GAS_SYSTEM_IOSPACE`.
pub const GAS_SYSTEM_IOSPACE: i32 = 1;
/// `GAS_PCI_CFG_SPACE`.
pub const GAS_PCI_CFG_SPACE: i32 = 2;
/// `GAS_EMBEDDED`.
pub const GAS_EMBEDDED: i32 = 3;
/// `GAS_SMBUS`.
pub const GAS_SMBUS: i32 = 4;
/// `GAS_FUNCTIONAL_FIXED`.
pub const GAS_FUNCTIONAL_FIXED: i32 = 127;
/// `GAS_ACCESS_UNDEFINED`.
pub const GAS_ACCESS_UNDEFINED: i32 = 0;
/// `GAS_ACCESS_BYTE`.
pub const GAS_ACCESS_BYTE: i32 = 1;
/// `GAS_ACCESS_WORD`.
pub const GAS_ACCESS_WORD: i32 = 2;
/// `GAS_ACCESS_DWORD`.
pub const GAS_ACCESS_DWORD: i32 = 3;
/// `GAS_ACCESS_QWORD`.
pub const GAS_ACCESS_QWORD: i32 = 4;
/// `FADT_SIG`.
pub const FADT_SIG: &[u8; 4] = b"FACP";
/// `FADT_INT_DUAL_PIC`.
pub const FADT_INT_DUAL_PIC: u8 = 0;
/// `FADT_INT_MULTI_APIC`.
pub const FADT_INT_MULTI_APIC: u8 = 1;
/// `FADT_PM_UNSPEC`.
pub const FADT_PM_UNSPEC: u8 = 0;
/// `FADT_PM_DESKTOP`.
pub const FADT_PM_DESKTOP: u8 = 1;
/// `FADT_PM_MOBILE`.
pub const FADT_PM_MOBILE: u8 = 2;
/// `FADT_PM_WORKSTATION`.
pub const FADT_PM_WORKSTATION: u8 = 3;
/// `FADT_PM_ENT_SERVER`.
pub const FADT_PM_ENT_SERVER: u8 = 4;
/// `FADT_PM_SOHO_SERVER`.
pub const FADT_PM_SOHO_SERVER: u8 = 5;
/// `FADT_PM_APPLIANCE`.
pub const FADT_PM_APPLIANCE: u8 = 6;
/// `FADT_PM_PERF_SERVER`.
pub const FADT_PM_PERF_SERVER: u8 = 7;
/// `FADT_LEGACY_DEVICES`: Legacy devices supported.
pub const FADT_LEGACY_DEVICES: u16 = 0x0001;
/// `FADT_i8042`: Keyboard controller present.
#[allow(non_upper_case_globals)] // the C spelling
pub const FADT_i8042: u16 = 0x0002;
/// `FADT_NO_VGA`: Do not probe VGA.
pub const FADT_NO_VGA: u16 = 0x0004;
/// `FADT_NO_MSI`: Do not enable MSI.
pub const FADT_NO_MSI: u16 = 0x0008;
/// `FADT_WBINVD`.
pub const FADT_WBINVD: u32 = 0x00000001;
/// `FADT_WBINVD_FLUSH`.
pub const FADT_WBINVD_FLUSH: u32 = 0x00000002;
/// `FADT_PROC_C1`.
pub const FADT_PROC_C1: u32 = 0x00000004;
/// `FADT_P_LVL2_UP`.
pub const FADT_P_LVL2_UP: u32 = 0x00000008;
/// `FADT_PWR_BUTTON`.
pub const FADT_PWR_BUTTON: u32 = 0x00000010;
/// `FADT_SLP_BUTTON`.
pub const FADT_SLP_BUTTON: u32 = 0x00000020;
/// `FADT_FIX_RTC`.
pub const FADT_FIX_RTC: u32 = 0x00000040;
/// `FADT_RTC_S4`.
pub const FADT_RTC_S4: u32 = 0x00000080;
/// `FADT_TMR_VAL_EXT`.
pub const FADT_TMR_VAL_EXT: u32 = 0x00000100;
/// `FADT_DCK_CAP`.
pub const FADT_DCK_CAP: u32 = 0x00000200;
/// `FADT_RESET_REG_SUP`.
pub const FADT_RESET_REG_SUP: u32 = 0x00000400;
/// `FADT_SEALED_CASE`.
pub const FADT_SEALED_CASE: u32 = 0x00000800;
/// `FADT_HEADLESS`.
pub const FADT_HEADLESS: u32 = 0x00001000;
/// `FADT_CPU_SW_SLP`.
pub const FADT_CPU_SW_SLP: u32 = 0x00002000;
/// `FADT_PCI_EXP_WAK`.
pub const FADT_PCI_EXP_WAK: u32 = 0x00004000;
/// `FADT_USE_PLATFORM_CLOCK`.
pub const FADT_USE_PLATFORM_CLOCK: u32 = 0x00008000;
/// `FADT_S4_RTC_STS_VALID`.
pub const FADT_S4_RTC_STS_VALID: u32 = 0x00010000;
/// `FADT_REMOTE_POWER_ON_CAPABLE`.
pub const FADT_REMOTE_POWER_ON_CAPABLE: u32 = 0x00020000;
/// `FADT_FORCE_APIC_CLUSTER_MODEL`.
pub const FADT_FORCE_APIC_CLUSTER_MODEL: u32 = 0x00040000;
/// `FADT_FORCE_APIC_PHYS_DEST_MODE`.
pub const FADT_FORCE_APIC_PHYS_DEST_MODE: u32 = 0x00080000;
/// `FADT_HW_REDUCED_ACPI`.
pub const FADT_HW_REDUCED_ACPI: u32 = 0x00100000;
/// `FADT_POWER_S0_IDLE_CAPABLE`.
pub const FADT_POWER_S0_IDLE_CAPABLE: u32 = 0x00200000;
/// `DSDT_SIG`.
pub const DSDT_SIG: &[u8; 4] = b"DSDT";
/// `SSDT_SIG`.
pub const SSDT_SIG: &[u8; 4] = b"SSDT";
/// `PSDT_SIG`.
pub const PSDT_SIG: &[u8; 4] = b"PSDT";
/// `MADT_SIG`.
pub const MADT_SIG: &[u8; 4] = b"APIC";
/// `ACPI_APIC_PCAT_COMPAT`.
pub const ACPI_APIC_PCAT_COMPAT: u32 = 0x00000001;
/// `ACPI_MADT_LAPIC`.
pub const ACPI_MADT_LAPIC: u8 = 0;
/// `ACPI_PROC_ENABLE`.
pub const ACPI_PROC_ENABLE: u32 = 0x00000001;
/// `ACPI_MADT_IOAPIC`.
pub const ACPI_MADT_IOAPIC: u8 = 1;
/// `ACPI_MADT_OVERRIDE`.
pub const ACPI_MADT_OVERRIDE: u8 = 2;
/// `ACPI_OVERRIDE_BUS_ISA`.
pub const ACPI_OVERRIDE_BUS_ISA: u8 = 0;
/// `ACPI_OVERRIDE_POLARITY_BITS`.
pub const ACPI_OVERRIDE_POLARITY_BITS: u16 = 0x3;
/// `ACPI_OVERRIDE_POLARITY_BUS`.
pub const ACPI_OVERRIDE_POLARITY_BUS: u16 = 0x0;
/// `ACPI_OVERRIDE_POLARITY_HIGH`.
pub const ACPI_OVERRIDE_POLARITY_HIGH: u16 = 0x1;
/// `ACPI_OVERRIDE_POLARITY_LOW`.
pub const ACPI_OVERRIDE_POLARITY_LOW: u16 = 0x3;
/// `ACPI_OVERRIDE_TRIGGER_BITS`.
pub const ACPI_OVERRIDE_TRIGGER_BITS: u16 = 0xc;
/// `ACPI_OVERRIDE_TRIGGER_BUS`.
pub const ACPI_OVERRIDE_TRIGGER_BUS: u16 = 0x0;
/// `ACPI_OVERRIDE_TRIGGER_EDGE`.
pub const ACPI_OVERRIDE_TRIGGER_EDGE: u16 = 0x4;
/// `ACPI_OVERRIDE_TRIGGER_LEVEL`.
pub const ACPI_OVERRIDE_TRIGGER_LEVEL: u16 = 0xc;
/// `ACPI_MADT_NMI`.
pub const ACPI_MADT_NMI: u8 = 3;
/// `ACPI_MADT_LAPIC_NMI`.
pub const ACPI_MADT_LAPIC_NMI: u8 = 4;
/// `ACPI_MADT_LAPIC_OVERRIDE`.
pub const ACPI_MADT_LAPIC_OVERRIDE: u8 = 5;
/// `ACPI_MADT_IO_SAPIC`.
pub const ACPI_MADT_IO_SAPIC: u8 = 6;
/// `ACPI_MADT_LOCAL_SAPIC`.
pub const ACPI_MADT_LOCAL_SAPIC: u8 = 7;
/// `ACPI_MADT_PLATFORM_INT`.
pub const ACPI_MADT_PLATFORM_INT: u8 = 8;
/// `ACPI_MADT_PLATFORM_PMI`.
pub const ACPI_MADT_PLATFORM_PMI: u8 = 1;
/// `ACPI_MADT_PLATFORM_INIT`.
pub const ACPI_MADT_PLATFORM_INIT: u8 = 2;
/// `ACPI_MADT_PLATFORM_CORR_ERROR`.
pub const ACPI_MADT_PLATFORM_CORR_ERROR: u8 = 3;
/// `ACPI_MADT_PLATFORM_CPEI`.
pub const ACPI_MADT_PLATFORM_CPEI: u32 = 0x00000001;
/// `ACPI_MADT_X2APIC`.
pub const ACPI_MADT_X2APIC: u8 = 9;
/// `ACPI_MADT_X2APIC_NMI`.
pub const ACPI_MADT_X2APIC_NMI: u8 = 10;
/// `ACPI_MADT_GICC`.
pub const ACPI_MADT_GICC: u8 = 11;
/// `ACPI_MADT_OEM_RSVD`.
pub const ACPI_MADT_OEM_RSVD: u8 = 128;
/// `SBST_SIG`.
pub const SBST_SIG: &[u8; 4] = b"SBST";
/// `ECDT_SIG`.
pub const ECDT_SIG: &[u8; 4] = b"ECDT";
/// `SRAT_SIG`.
pub const SRAT_SIG: &[u8; 4] = b"SRAT";
/// `SLIT_SIG`.
pub const SLIT_SIG: &[u8; 4] = b"SLIT";
/// `HPET_SIG`.
pub const HPET_SIG: &[u8; 4] = b"HPET";
/// `MCFG_SIG`.
pub const MCFG_SIG: &[u8; 4] = b"MCFG";
/// `SPCR_SIG`.
pub const SPCR_SIG: &[u8; 4] = b"SPCR";
/// `SPCR_16550`.
pub const SPCR_16550: u8 = 0;
/// `SPCR_16450`.
pub const SPCR_16450: u8 = 1;
/// `SPCR_ARM_PL011`.
pub const SPCR_ARM_PL011: u8 = 3;
/// `SPCR_ARM_SBSA`.
pub const SPCR_ARM_SBSA: u8 = 14;
/// `FACS_SIG`.
pub const FACS_SIG: &[u8; 4] = b"FACS";
/// `FACS_LOCK_PENDING`.
pub const FACS_LOCK_PENDING: u32 = 0x00000001;
/// `FACS_LOCK_OWNED`.
pub const FACS_LOCK_OWNED: u32 = 0x00000002;
/// `FACS_S4BIOS_F`: S4BIOS_REQ supported.
pub const FACS_S4BIOS_F: u32 = 0x00000001;
/// `TPM2_SIG`.
pub const TPM2_SIG: &[u8; 4] = b"TPM2";
/// `LPIT_SIG`.
pub const LPIT_SIG: &[u8; 4] = b"LPIT";
/// `LPIT_DISABLED`.
pub const LPIT_DISABLED: u32 = 1 << 0;
/// `LPIT_COUNTER_NOT_AVAILABLE`.
pub const LPIT_COUNTER_NOT_AVAILABLE: u32 = 1 << 1;
/// `DMAR_ENDPOINT`.
pub const DMAR_ENDPOINT: u8 = 0x1;
/// `DMAR_BRIDGE`.
pub const DMAR_BRIDGE: u8 = 0x2;
/// `DMAR_IOAPIC`.
pub const DMAR_IOAPIC: u8 = 0x3;
/// `DMAR_HPET`.
pub const DMAR_HPET: u8 = 0x4;
/// `DMAR_DRHD`.
pub const DMAR_DRHD: u16 = 0x0;
/// `DMAR_RMRR`.
pub const DMAR_RMRR: u16 = 0x1;
/// `DMAR_ATSR`.
pub const DMAR_ATSR: u16 = 0x2;
/// `DMAR_RHSA`.
pub const DMAR_RHSA: u16 = 0x3;
/// `DMAR_SIG`.
pub const DMAR_SIG: &[u8; 4] = b"DMAR";
/// `IVHD_RESVD`.
pub const IVHD_RESVD: u8 = 0;
/// `IVHD_ALL`.
pub const IVHD_ALL: u8 = 1;
/// `IVHD_SEL`.
pub const IVHD_SEL: u8 = 2;
/// `IVHD_SOR`.
pub const IVHD_SOR: u8 = 3;
/// `IVHD_EOR`.
pub const IVHD_EOR: u8 = 4;
/// `IVHD_ALIAS_SEL`.
pub const IVHD_ALIAS_SEL: u8 = 66;
/// `IVHD_ALIAS_SOR`.
pub const IVHD_ALIAS_SOR: u8 = 67;
/// `IVHD_EXT_SEL`.
pub const IVHD_EXT_SEL: u8 = 70;
/// `IVHD_EXT_SOR`.
pub const IVHD_EXT_SOR: u8 = 71;
/// `IVHD_SPECIAL`.
pub const IVHD_SPECIAL: u8 = 72;
/// `IVHD_ATS_DIS`.
pub const IVHD_ATS_DIS: u32 = 1 << 31;
/// `IVHD_IOAPIC`.
pub const IVHD_IOAPIC: u8 = 0x01;
/// `IVHD_HPET`.
pub const IVHD_HPET: u8 = 0x02;
/// `IVMD_EXCLRANGE`.
pub const IVMD_EXCLRANGE: u8 = 1 << 3;
/// `IVMD_IW`.
pub const IVMD_IW: u8 = 1 << 2;
/// `IVMD_IR`.
pub const IVMD_IR: u8 = 1 << 1;
/// `IVMD_UNITY`.
pub const IVMD_UNITY: u8 = 1 << 0;
/// `IVHD_PPRSUP`.
pub const IVHD_PPRSUP: u8 = 1 << 7;
/// `IVHD_PREFSUP`.
pub const IVHD_PREFSUP: u8 = 1 << 6;
/// `IVHD_COHERENT`.
pub const IVHD_COHERENT: u8 = 1 << 5;
/// `IVHD_IOTLB`.
pub const IVHD_IOTLB: u8 = 1 << 4;
/// `IVHD_ISOC`.
pub const IVHD_ISOC: u8 = 1 << 3;
/// `IVHD_RESPASSPW`.
pub const IVHD_RESPASSPW: u8 = 1 << 2;
/// `IVHD_PASSPW`.
pub const IVHD_PASSPW: u8 = 1 << 1;
/// `IVHD_HTTUNEN`.
pub const IVHD_HTTUNEN: u8 = 1 << 0;
/// `IVHD_UNITID_SHIFT`.
pub const IVHD_UNITID_SHIFT: u32 = 8;
/// `IVHD_UNITID_MASK`.
pub const IVHD_UNITID_MASK: u16 = 0x1F;
/// `IVHD_MSINUM_SHIFT`.
pub const IVHD_MSINUM_SHIFT: u32 = 0;
/// `IVHD_MSINUM_MASK`.
pub const IVHD_MSINUM_MASK: u16 = 0x1F;
/// `IVRS_IVHD`.
pub const IVRS_IVHD: u8 = 0x10;
/// `IVRS_IVHD_EXT`.
pub const IVRS_IVHD_EXT: u8 = 0x11;
/// `IVRS_IVMD_ALL`.
pub const IVRS_IVMD_ALL: u8 = 0x20;
/// `IVRS_IVMD_SPECIFIED`.
pub const IVRS_IVMD_SPECIFIED: u8 = 0x21;
/// `IVRS_IVMD_RANGE`.
pub const IVRS_IVMD_RANGE: u8 = 0x22;
/// `IVRS_SIG`.
pub const IVRS_SIG: &[u8; 4] = b"IVRS";
/// `IVRS_ATSRNG`.
pub const IVRS_ATSRNG: u32 = 1 << 22;
/// `IVRS_VASIZE_SHIFT`.
pub const IVRS_VASIZE_SHIFT: u32 = 15;
/// `IVRS_VASIZE_MASK`.
pub const IVRS_VASIZE_MASK: u32 = 0x7F;
/// `IVRS_PASIZE_SHIFT`.
pub const IVRS_PASIZE_SHIFT: u32 = 8;
/// `IVRS_PASIZE_MASK`.
pub const IVRS_PASIZE_MASK: u32 = 0x7F;
/// `IORT_SIG`.
pub const IORT_SIG: &[u8; 4] = b"IORT";
/// `ACPI_IORT_ITS`.
pub const ACPI_IORT_ITS: u8 = 0;
/// `ACPI_IORT_NAMED_COMPONENT`.
pub const ACPI_IORT_NAMED_COMPONENT: u8 = 1;
/// `ACPI_IORT_ROOT_COMPLEX`.
pub const ACPI_IORT_ROOT_COMPLEX: u8 = 2;
/// `ACPI_IORT_SMMU`.
pub const ACPI_IORT_SMMU: u8 = 3;
/// `ACPI_IORT_SMMU_V3`.
pub const ACPI_IORT_SMMU_V3: u8 = 4;
/// `ACPI_IORT_SMMU_V1`.
pub const ACPI_IORT_SMMU_V1: u32 = 0;
/// `ACPI_IORT_SMMU_V2`.
pub const ACPI_IORT_SMMU_V2: u32 = 1;
/// `ACPI_IORT_SMMU_CORELINK_MMU400`.
pub const ACPI_IORT_SMMU_CORELINK_MMU400: u32 = 2;
/// `ACPI_IORT_SMMU_CORELINK_MMU500`.
pub const ACPI_IORT_SMMU_CORELINK_MMU500: u32 = 3;
/// `ACPI_IORT_SMMU_CORELINK_MMU401`.
pub const ACPI_IORT_SMMU_CORELINK_MMU401: u32 = 4;
/// `ACPI_IORT_SMMU_CAVIUM_THUNDERX`.
pub const ACPI_IORT_SMMU_CAVIUM_THUNDERX: u32 = 5;
/// `ACPI_IORT_SMMU_DVM`.
pub const ACPI_IORT_SMMU_DVM: u32 = 0x00000001;
/// `ACPI_IORT_SMMU_COHERENT`.
pub const ACPI_IORT_SMMU_COHERENT: u32 = 0x00000002;
/// `ACPI_IORT_SMMU_INTR_EDGE`.
pub const ACPI_IORT_SMMU_INTR_EDGE: u32 = 1 << 0;
/// `ACPI_IORT_SMMU_V3_PROX_DOM_VALID`.
pub const ACPI_IORT_SMMU_V3_PROX_DOM_VALID: u32 = 1 << 3;
/// `ACPI_IORT_SMMU_V3_DEVID_MAP_VALID`.
pub const ACPI_IORT_SMMU_V3_DEVID_MAP_VALID: u32 = 1 << 4;
/// `ACPI_IORT_SMMU_V3_GENERIC`.
pub const ACPI_IORT_SMMU_V3_GENERIC: u32 = 0;
/// `ACPI_IORT_SMMU_V3_HISILICON_HI161X`.
pub const ACPI_IORT_SMMU_V3_HISILICON_HI161X: u32 = 1;
/// `ACPI_IORT_SMMU_V3_CAVIUM_CN99X`.
pub const ACPI_IORT_SMMU_V3_CAVIUM_CN99X: u32 = 2;
/// `ACPI_IORT_MAPPING_SINGLE`.
pub const ACPI_IORT_MAPPING_SINGLE: u32 = 0x00000001;
/// `ACPI_FREQUENCY`: Per ACPI spec.
pub const ACPI_FREQUENCY: u32 = 3579545;
/// `ACPI_PM1_STATUS`.
pub const ACPI_PM1_STATUS: u8 = 0x00;
/// `ACPI_PM1_TMR_STS`.
pub const ACPI_PM1_TMR_STS: u16 = 0x0001;
/// `ACPI_PM1_BM_STS`.
pub const ACPI_PM1_BM_STS: u16 = 0x0010;
/// `ACPI_PM1_GBL_STS`.
pub const ACPI_PM1_GBL_STS: u16 = 0x0020;
/// `ACPI_PM1_PWRBTN_STS`.
pub const ACPI_PM1_PWRBTN_STS: u16 = 0x0100;
/// `ACPI_PM1_SLPBTN_STS`.
pub const ACPI_PM1_SLPBTN_STS: u16 = 0x0200;
/// `ACPI_PM1_RTC_STS`.
pub const ACPI_PM1_RTC_STS: u16 = 0x0400;
/// `ACPI_PM1_PCIEXP_WAKE_STS`.
pub const ACPI_PM1_PCIEXP_WAKE_STS: u16 = 0x4000;
/// `ACPI_PM1_WAK_STS`.
pub const ACPI_PM1_WAK_STS: u16 = 0x8000;
/// `ACPI_PM1_ALL_STS`.
pub const ACPI_PM1_ALL_STS: u16 = ACPI_PM1_TMR_STS
    | ACPI_PM1_BM_STS
    | ACPI_PM1_GBL_STS
    | ACPI_PM1_PWRBTN_STS
    | ACPI_PM1_SLPBTN_STS
    | ACPI_PM1_RTC_STS
    | ACPI_PM1_PCIEXP_WAKE_STS
    | ACPI_PM1_WAK_STS;
/// `ACPI_PM1_ENABLE`.
pub const ACPI_PM1_ENABLE: u8 = 0x02;
/// `ACPI_PM1_TMR_EN`.
pub const ACPI_PM1_TMR_EN: u16 = 0x0001;
/// `ACPI_PM1_GBL_EN`.
pub const ACPI_PM1_GBL_EN: u16 = 0x0020;
/// `ACPI_PM1_PWRBTN_EN`.
pub const ACPI_PM1_PWRBTN_EN: u16 = 0x0100;
/// `ACPI_PM1_SLPBTN_EN`.
pub const ACPI_PM1_SLPBTN_EN: u16 = 0x0200;
/// `ACPI_PM1_RTC_EN`.
pub const ACPI_PM1_RTC_EN: u16 = 0x0400;
/// `ACPI_PM1_PCIEXP_WAKE_DIS`.
pub const ACPI_PM1_PCIEXP_WAKE_DIS: u16 = 0x4000;
/// `ACPI_PM1_CONTROL`.
pub const ACPI_PM1_CONTROL: u8 = 0x00;
/// `ACPI_PM1_SCI_EN`.
pub const ACPI_PM1_SCI_EN: u16 = 0x0001;
/// `ACPI_PM1_BM_RLD`.
pub const ACPI_PM1_BM_RLD: u16 = 0x0002;
/// `ACPI_PM1_GBL_RLS`.
pub const ACPI_PM1_GBL_RLS: u16 = 0x0004;
/// `ACPI_PM1_SLP_TYPX_MASK`.
pub const ACPI_PM1_SLP_TYPX_MASK: u16 = 0x1c00;
/// `ACPI_PM1_SLP_EN`.
pub const ACPI_PM1_SLP_EN: u16 = 0x2000;
/// `ACPI_PM2_CONTROL`.
pub const ACPI_PM2_CONTROL: u8 = 0x06;
/// `ACPI_PM2_ARB_DIS`.
pub const ACPI_PM2_ARB_DIS: u16 = 0x0001;
/// `ACPI_OPREG_SYSMEM`: SystemMemory.
pub const ACPI_OPREG_SYSMEM: i32 = 0;
/// `ACPI_OPREG_SYSIO`: SystemIO.
pub const ACPI_OPREG_SYSIO: i32 = 1;
/// `ACPI_OPREG_PCICFG`: PCI_Config.
pub const ACPI_OPREG_PCICFG: i32 = 2;
/// `ACPI_OPREG_EC`: EmbeddedControl.
pub const ACPI_OPREG_EC: i32 = 3;
/// `ACPI_OPREG_SMBUS`: SMBus.
pub const ACPI_OPREG_SMBUS: i32 = 4;
/// `ACPI_OPREG_CMOS`: CMOS.
pub const ACPI_OPREG_CMOS: i32 = 5;
/// `ACPI_OPREG_PCIBAR`: PCIBARTarget.
pub const ACPI_OPREG_PCIBAR: i32 = 6;
/// `ACPI_OPREG_IPMI`: IPMI.
pub const ACPI_OPREG_IPMI: i32 = 7;
/// `ACPI_OPREG_GPIO`: GeneralPurposeIO.
pub const ACPI_OPREG_GPIO: i32 = 8;
/// `ACPI_OPREG_GSB`: GenericSerialBus.
pub const ACPI_OPREG_GSB: i32 = 9;
/// `ACPI_STATE_S0`.
pub const ACPI_STATE_S0: u8 = 0;
/// `ACPI_STATE_S1`.
pub const ACPI_STATE_S1: u8 = 1;
/// `ACPI_STATE_S2`.
pub const ACPI_STATE_S2: u8 = 2;
/// `ACPI_STATE_S3`.
pub const ACPI_STATE_S3: u8 = 3;
/// `ACPI_STATE_S4`.
pub const ACPI_STATE_S4: u8 = 4;
/// `ACPI_STATE_S5`.
pub const ACPI_STATE_S5: u8 = 5;
/// `ACPI_STATE_D0`.
pub const ACPI_STATE_D0: u8 = 0;
/// `ACPI_STATE_D1`.
pub const ACPI_STATE_D1: u8 = 1;
/// `ACPI_STATE_D2`.
pub const ACPI_STATE_D2: u8 = 2;
/// `ACPI_STATE_D3`.
pub const ACPI_STATE_D3: u8 = 3;
/// `ACPI_DEV_TIM`: System timer.
pub const ACPI_DEV_TIM: &str = "PNP0100";
/// `ACPI_DEV_ACPI`: ACPI device.
pub const ACPI_DEV_ACPI: &str = "PNP0C08";
/// `ACPI_DEV_PCIB`: PCI bus.
pub const ACPI_DEV_PCIB: &str = "PNP0A03";
/// `ACPI_DEV_GISAB`: Generic ISA Bus.
pub const ACPI_DEV_GISAB: &str = "PNP0A05";
/// `ACPI_DEV_EIOB`: Extended I/O Bus.
pub const ACPI_DEV_EIOB: &str = "PNP0A06";
/// `ACPI_DEV_PCIEB`: PCIe bus.
pub const ACPI_DEV_PCIEB: &str = "PNP0A08";
/// `ACPI_DEV_MR`: Motherboard resources.
pub const ACPI_DEV_MR: &str = "PNP0C02";
/// `ACPI_DEV_NPROC`: Numeric data processor.
pub const ACPI_DEV_NPROC: &str = "PNP0C04";
/// `ACPI_DEV_CS`: ACPI-Compliant System.
pub const ACPI_DEV_CS: &str = "PNP0C08";
/// `ACPI_DEV_ECD`: Embedded Controller Device.
pub const ACPI_DEV_ECD: &str = "PNP0C09";
/// `ACPI_DEV_CMB`: Control Method Battery.
pub const ACPI_DEV_CMB: &str = "PNP0C0A";
/// `ACPI_DEV_FAN`: Fan Device.
pub const ACPI_DEV_FAN: &str = "PNP0C0B";
/// `ACPI_DEV_PBD`: Power Button Device.
pub const ACPI_DEV_PBD: &str = "PNP0C0C";
/// `ACPI_DEV_LD`: Lid Device.
pub const ACPI_DEV_LD: &str = "PNP0C0D";
/// `ACPI_DEV_SBD`: Sleep Button Device.
pub const ACPI_DEV_SBD: &str = "PNP0C0E";
/// `ACPI_DEV_PILD`: PCI Interrupt Link Device.
pub const ACPI_DEV_PILD: &str = "PNP0C0F";
/// `ACPI_DEV_MEMD`: Memory Device.
pub const ACPI_DEV_MEMD: &str = "PNP0C80";
/// `ACPI_DEV_MOUSE`: PS/2 Mouse.
pub const ACPI_DEV_MOUSE: &str = "PNP0F13";
/// `ACPI_DEV_SHC`: SMBus 1.0 Host Controller.
pub const ACPI_DEV_SHC: &str = "ACPI0001";
/// `ACPI_DEV_SBS`: Smart Battery Subsystem.
pub const ACPI_DEV_SBS: &str = "ACPI0002";
/// `ACPI_DEV_AC`: AC Device.
pub const ACPI_DEV_AC: &str = "ACPI0003";
/// `ACPI_DEV_MD`: Module Device.
pub const ACPI_DEV_MD: &str = "ACPI0004";
/// `ACPI_DEV_SMBUS`: SMBus 2.0 Host Controller.
pub const ACPI_DEV_SMBUS: &str = "ACPI0005";
/// `ACPI_DEV_GBD`: GPE Block Device.
pub const ACPI_DEV_GBD: &str = "ACPI0006";
/// `ACPI_DEV_PD`: Processor Device.
pub const ACPI_DEV_PD: &str = "ACPI0007";
/// `ACPI_DEV_ALSD`: Ambient Light Sensor Device.
pub const ACPI_DEV_ALSD: &str = "ACPI0008";
/// `ACPI_DEV_IOXA`: IO x APIC Device.
pub const ACPI_DEV_IOXA: &str = "ACPI0009";
/// `ACPI_DEV_IOA`: IO APIC Device.
pub const ACPI_DEV_IOA: &str = "ACPI000A";
/// `ACPI_DEV_IOSA`: IO SAPIC Device.
pub const ACPI_DEV_IOSA: &str = "ACPI000B";
/// `ACPI_DEV_THZ`: Thermal Zone.
pub const ACPI_DEV_THZ: &str = "THERMALZONE";
/// `ACPI_DEV_FFB`: Fixed Feature Button.
pub const ACPI_DEV_FFB: &str = "FIXEDBUTTON";
/// `ACPI_DEV_IPMI`: IPMI.
pub const ACPI_DEV_IPMI: &str = "IPI0001";

/// `ACPI_ADR_PCIDEV(addr)`: the PCI device of an `_ADR` value (the C does not parenthesise
/// `addr`; the argument here is one value).
pub const fn acpi_adr_pcidev(addr: u64) -> u16 {
    (addr >> 16) as u16
}

/// `ACPI_ADR_PCIFUN(addr)`: the PCI function of an `_ADR` value.
pub const fn acpi_adr_pcifun(addr: u64) -> u16 {
    (addr & 0xFFFF) as u16
}

/// `ACPI_PCI_SEG(addr)`: the PCI segment of an operation-region address.
pub const fn acpi_pci_seg(addr: u64) -> u16 {
    (addr >> 48) as u16
}

/// `ACPI_PCI_BUS(addr)`: the PCI bus of an operation-region address.
pub const fn acpi_pci_bus(addr: u64) -> u8 {
    (addr >> 40) as u8
}

/// `ACPI_PCI_DEV(addr)`: the PCI device of an operation-region address.
pub const fn acpi_pci_dev(addr: u64) -> u8 {
    (addr >> 32) as u8
}

/// `ACPI_PCI_FN(addr)`: the PCI function of an operation-region address.
pub const fn acpi_pci_fn(addr: u64) -> u16 {
    (addr >> 16) as u16
}

/// `ACPI_PCI_REG(addr)`: the PCI register of an operation-region address.
pub const fn acpi_pci_reg(addr: u64) -> u16 {
    addr as u16
}

/// `ACPI_PM1_SLP_TYPX(x)`: the SLP_TYP field of a PM1 control register for sleep type `x`.
pub const fn acpi_pm1_slp_typx(x: u16) -> u16 {
    x << 10
}

/// `ACPI_IORT_SMMU_V3_COHACC_OVERRIDE(x)`: the COHACC override field of the `flags` of an
/// SMMUv3 node.
pub const fn acpi_iort_smmu_v3_cohacc_override(x: u32) -> u32 {
    x & 0x1
}

/// `ACPI_IORT_SMMU_V3_HTTU_OVERRIDE(x)`: the HTTU override field of the `flags` of an SMMUv3
/// node.
pub const fn acpi_iort_smmu_v3_httu_override(x: u32) -> u32 {
    (x >> 1) & 0x3
}

/// `struct acpi_rsdp1`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiRsdp1 {
    /// `signature`.
    pub signature: [u8; 8],
    /// `checksum`: make sum == 0.
    pub checksum: u8,
    /// `oemid`.
    pub oemid: [u8; 6],
    /// `revision`: 0 for 1, 2 for 2.
    pub revision: u8,
    /// `rsdt`: physical.
    pub rsdt: u32,
}

/// `struct acpi_rsdp`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiRsdp {
    /// `rsdp1`.
    pub rsdp1: AcpiRsdp1,
    /// `rsdp_length`: length of rsdp.
    pub rsdp_length: u32,
    /// `rsdp_xsdt`: physical.
    pub rsdp_xsdt: u64,
    /// `rsdp_extchecksum`: entire table.
    pub rsdp_extchecksum: u8,
    /// `rsdp_reserved`: must be zero.
    pub rsdp_reserved: [u8; 3],
}

/// `struct acpi_table_header`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiTableHeader {
    /// `signature`.
    pub signature: [u8; 4],
    /// `length`.
    pub length: u32,
    /// `revision`.
    pub revision: u8,
    /// `checksum`.
    pub checksum: u8,
    /// `oemid`.
    pub oemid: [u8; 6],
    /// `oemtableid`.
    pub oemtableid: [u8; 8],
    /// `oemrevision`.
    pub oemrevision: u32,
    /// `aslcompilerid`.
    pub aslcompilerid: [u8; 4],
    /// `aslcompilerrevision`.
    pub aslcompilerrevision: u32,
}

/// `struct acpi_rsdt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiRsdt {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `table_offsets`.
    pub table_offsets: [u32; 1],
}

/// `struct acpi_xsdt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiXsdt {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `table_offsets`.
    pub table_offsets: [u64; 1],
}

/// `struct acpi_gas`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiGas {
    /// `address_space_id`.
    pub address_space_id: u8,
    /// `register_bit_width`.
    pub register_bit_width: u8,
    /// `register_bit_offset`.
    pub register_bit_offset: u8,
    /// `access_size`.
    pub access_size: u8,
    /// `address`.
    pub address: u64,
}

/// `struct acpi_fadt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiFadt {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `firmware_ctl`: phys addr FACS.
    pub firmware_ctl: u32,
    /// `dsdt`: phys addr DSDT.
    pub dsdt: u32,
    /// `int_model`: interrupt model (hdr_revision < 3).
    pub int_model: u8,
    /// `pm_profile`: power mgmt profile.
    pub pm_profile: u8,
    /// `sci_int`: SCI interrupt.
    pub sci_int: u16,
    /// `smi_cmd`: SMI command port.
    pub smi_cmd: u32,
    /// `acpi_enable`: value to enable.
    pub acpi_enable: u8,
    /// `acpi_disable`: value to disable.
    pub acpi_disable: u8,
    /// `s4bios_req`: value for S4.
    pub s4bios_req: u8,
    /// `pstate_cnt`: value for performance (hdr_revision > 2).
    pub pstate_cnt: u8,
    /// `pm1a_evt_blk`: power management 1a.
    pub pm1a_evt_blk: u32,
    /// `pm1b_evt_blk`: power management 1b.
    pub pm1b_evt_blk: u32,
    /// `pm1a_cnt_blk`: pm control 1a.
    pub pm1a_cnt_blk: u32,
    /// `pm1b_cnt_blk`: pm control 1b.
    pub pm1b_cnt_blk: u32,
    /// `pm2_cnt_blk`: pm control 2.
    pub pm2_cnt_blk: u32,
    /// `pm_tmr_blk`.
    pub pm_tmr_blk: u32,
    /// `gpe0_blk`.
    pub gpe0_blk: u32,
    /// `gpe1_blk`.
    pub gpe1_blk: u32,
    /// `pm1_evt_len`.
    pub pm1_evt_len: u8,
    /// `pm1_cnt_len`.
    pub pm1_cnt_len: u8,
    /// `pm2_cnt_len`.
    pub pm2_cnt_len: u8,
    /// `pm_tmr_len`.
    pub pm_tmr_len: u8,
    /// `gpe0_blk_len`.
    pub gpe0_blk_len: u8,
    /// `gpe1_blk_len`.
    pub gpe1_blk_len: u8,
    /// `gpe1_base`.
    pub gpe1_base: u8,
    /// `cst_cnt`: (hdr_revision > 2).
    pub cst_cnt: u8,
    /// `p_lvl2_lat`.
    pub p_lvl2_lat: u16,
    /// `p_lvl3_lat`.
    pub p_lvl3_lat: u16,
    /// `flush_size`.
    pub flush_size: u16,
    /// `flush_stride`.
    pub flush_stride: u16,
    /// `duty_offset`.
    pub duty_offset: u8,
    /// `duty_width`.
    pub duty_width: u8,
    /// `day_alrm`.
    pub day_alrm: u8,
    /// `mon_alrm`.
    pub mon_alrm: u8,
    /// `century`.
    pub century: u8,
    /// `iapc_boot_arch`: (hdr_revision > 2).
    pub iapc_boot_arch: u16,
    /// `reserved1`.
    pub reserved1: u8,
    /// `flags`.
    pub flags: u32,
    /// `reset_reg`.
    pub reset_reg: AcpiGas,
    /// `reset_value`.
    pub reset_value: u8,
    /// `reserved2a`.
    pub reserved2a: u8,
    /// `reserved2b`.
    pub reserved2b: u8,
    /// `fadt_minor`.
    pub fadt_minor: u8,
    /// `x_firmware_ctl`.
    pub x_firmware_ctl: u64,
    /// `x_dsdt`.
    pub x_dsdt: u64,
    /// `x_pm1a_evt_blk`.
    pub x_pm1a_evt_blk: AcpiGas,
    /// `x_pm1b_evt_blk`.
    pub x_pm1b_evt_blk: AcpiGas,
    /// `x_pm1a_cnt_blk`.
    pub x_pm1a_cnt_blk: AcpiGas,
    /// `x_pm1b_cnt_blk`.
    pub x_pm1b_cnt_blk: AcpiGas,
    /// `x_pm2_cnt_blk`.
    pub x_pm2_cnt_blk: AcpiGas,
    /// `x_pm_tmr_blk`.
    pub x_pm_tmr_blk: AcpiGas,
    /// `x_gpe0_blk`.
    pub x_gpe0_blk: AcpiGas,
    /// `x_gpe1_blk`.
    pub x_gpe1_blk: AcpiGas,
    /// `sleep_control_reg`.
    pub sleep_control_reg: AcpiGas,
    /// `sleep_status_reg`.
    pub sleep_status_reg: AcpiGas,
}

/// `struct acpi_dsdt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiDsdt {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `aml`.
    pub aml: [u8; 1],
}

/// `struct acpi_ssdt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiSsdt {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `aml`.
    pub aml: [u8; 1],
}

/// `struct acpi_psdt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiPsdt {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
}

/// `struct acpi_madt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMadt {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `local_apic_address`.
    pub local_apic_address: u32,
    /// `flags`.
    pub flags: u32,
}

/// `struct acpi_madt_lapic`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMadtLapic {
    /// `apic_type`.
    pub apic_type: u8,
    /// `length`.
    pub length: u8,
    /// `acpi_proc_id`.
    pub acpi_proc_id: u8,
    /// `apic_id`.
    pub apic_id: u8,
    /// `flags`.
    pub flags: u32,
}

/// `struct acpi_madt_ioapic`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMadtIoapic {
    /// `apic_type`.
    pub apic_type: u8,
    /// `length`.
    pub length: u8,
    /// `acpi_ioapic_id`.
    pub acpi_ioapic_id: u8,
    /// `reserved`.
    pub reserved: u8,
    /// `address`.
    pub address: u32,
    /// `global_int_base`.
    pub global_int_base: u32,
}

/// `struct acpi_madt_override`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMadtOverride {
    /// `apic_type`.
    pub apic_type: u8,
    /// `length`.
    pub length: u8,
    /// `bus`.
    pub bus: u8,
    /// `source`.
    pub source: u8,
    /// `global_int`.
    pub global_int: u32,
    /// `flags`.
    pub flags: u16,
}

/// `struct acpi_madt_nmi`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMadtNmi {
    /// `apic_type`.
    pub apic_type: u8,
    /// `length`.
    pub length: u8,
    /// `flags`: Same flags as acpi_madt_override.
    pub flags: u16,
    /// `global_int`.
    pub global_int: u32,
}

/// `struct acpi_madt_lapic_nmi`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMadtLapicNmi {
    /// `apic_type`.
    pub apic_type: u8,
    /// `length`.
    pub length: u8,
    /// `acpi_proc_id`.
    pub acpi_proc_id: u8,
    /// `flags`: Same flags as acpi_madt_override.
    pub flags: u16,
    /// `local_apic_lint`.
    pub local_apic_lint: u8,
}

/// `struct acpi_madt_lapic_override`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMadtLapicOverride {
    /// `apic_type`.
    pub apic_type: u8,
    /// `length`.
    pub length: u8,
    /// `reserved`.
    pub reserved: u16,
    /// `lapic_address`.
    pub lapic_address: u64,
}

/// `struct acpi_madt_io_sapic`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMadtIoSapic {
    /// `apic_type`.
    pub apic_type: u8,
    /// `length`.
    pub length: u8,
    /// `iosapic_id`.
    pub iosapic_id: u8,
    /// `reserved`.
    pub reserved: u8,
    /// `global_int_base`.
    pub global_int_base: u32,
    /// `iosapic_address`.
    pub iosapic_address: u64,
}

/// `struct acpi_madt_local_sapic`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMadtLocalSapic {
    /// `apic_type`.
    pub apic_type: u8,
    /// `length`.
    pub length: u8,
    /// `acpi_proc_id`.
    pub acpi_proc_id: u8,
    /// `local_sapic_id`.
    pub local_sapic_id: u8,
    /// `local_sapic_eid`.
    pub local_sapic_eid: u8,
    /// `reserved`.
    pub reserved: [u8; 3],
    /// `flags`: Same flags as acpi_madt_lapic.
    pub flags: u32,
    /// `acpi_proc_uid`.
    pub acpi_proc_uid: u32,
    /// `acpi_proc_uid_string`.
    pub acpi_proc_uid_string: [u8; 1],
}

/// `struct acpi_madt_platform_int`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMadtPlatformInt {
    /// `apic_type`.
    pub apic_type: u8,
    /// `length`.
    pub length: u8,
    /// `flags`: Same flags as acpi_madt_override.
    pub flags: u16,
    /// `int_type`.
    pub int_type: u8,
    /// `proc_id`.
    pub proc_id: u8,
    /// `proc_eid`.
    pub proc_eid: u8,
    /// `io_sapic_vec`.
    pub io_sapic_vec: u8,
    /// `global_int`.
    pub global_int: u32,
    /// `platform_int_flags`.
    pub platform_int_flags: u32,
}

/// `struct acpi_madt_x2apic`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMadtX2apic {
    /// `apic_type`.
    pub apic_type: u8,
    /// `length`.
    pub length: u8,
    /// `reserved`.
    pub reserved: [u8; 2],
    /// `apic_id`.
    pub apic_id: u32,
    /// `flags`: Same flags as acpi_madt_lapic.
    pub flags: u32,
    /// `acpi_proc_uid`.
    pub acpi_proc_uid: u32,
}

/// `struct acpi_madt_x2apic_nmi`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMadtX2apicNmi {
    /// `apic_type`.
    pub apic_type: u8,
    /// `length`.
    pub length: u8,
    /// `flags`: Same flags as acpi_madt_override.
    pub flags: u16,
    /// `apic_proc_uid`.
    pub apic_proc_uid: u32,
    /// `local_x2apic_lint`.
    pub local_x2apic_lint: u8,
    /// `reserved`.
    pub reserved: [u8; 3],
}

/// `struct acpi_madt_gicc`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMadtGicc {
    /// `apic_type`.
    pub apic_type: u8,
    /// `length`.
    pub length: u8,
    /// `reserved1`.
    pub reserved1: u16,
    /// `gic_id`.
    pub gic_id: u32,
    /// `acpi_proc_uid`.
    pub acpi_proc_uid: u32,
    /// `flags`.
    pub flags: u32,
    /// `parking_protocol_version`.
    pub parking_protocol_version: u32,
    /// `performance_interrupt`.
    pub performance_interrupt: u32,
    /// `parked_address`.
    pub parked_address: u64,
    /// `base_address`.
    pub base_address: u64,
    /// `gicv_base_address`.
    pub gicv_base_address: u64,
    /// `gich_base_address`.
    pub gich_base_address: u64,
    /// `maintenance_interrupt`.
    pub maintenance_interrupt: u32,
    /// `gicr_base_address`.
    pub gicr_base_address: u64,
    /// `mpidr`.
    pub mpidr: u64,
    /// `efficiency_class`.
    pub efficiency_class: u8,
    /// `reserved2`.
    pub reserved2: [u8; 3],
}

/// `union acpi_madt_entry`: the interrupt controller structure types a MADT can hold,
/// told apart by `apic_type` (the first byte of each member).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub union AcpiMadtEntry {
    /// `madt_lapic`.
    pub madt_lapic: AcpiMadtLapic,
    /// `madt_ioapic`.
    pub madt_ioapic: AcpiMadtIoapic,
    /// `madt_override`.
    pub madt_override: AcpiMadtOverride,
    /// `madt_nmi`.
    pub madt_nmi: AcpiMadtNmi,
    /// `madt_lapic_nmi`.
    pub madt_lapic_nmi: AcpiMadtLapicNmi,
    /// `madt_lapic_override`.
    pub madt_lapic_override: AcpiMadtLapicOverride,
    /// `madt_io_sapic`.
    pub madt_io_sapic: AcpiMadtIoSapic,
    /// `madt_local_sapic`.
    pub madt_local_sapic: AcpiMadtLocalSapic,
    /// `madt_platform_int`.
    pub madt_platform_int: AcpiMadtPlatformInt,
    /// `madt_x2apic`.
    pub madt_x2apic: AcpiMadtX2apic,
    /// `madt_x2apic_nmi`.
    pub madt_x2apic_nmi: AcpiMadtX2apicNmi,
}

/// `struct acpi_sbst`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiSbst {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `warning_energy_level`.
    pub warning_energy_level: u32,
    /// `low_energy_level`.
    pub low_energy_level: u32,
    /// `critical_energy_level`.
    pub critical_energy_level: u32,
}

/// `struct acpi_ecdt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiEcdt {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `ec_control`.
    pub ec_control: AcpiGas,
    /// `ec_data`.
    pub ec_data: AcpiGas,
    /// `uid`.
    pub uid: u32,
    /// `gpe_bit`.
    pub gpe_bit: u8,
    /// `ec_id`.
    pub ec_id: [u8; 1],
}

/// `struct acpi_srat`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiSrat {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `reserved1`.
    pub reserved1: u32,
    /// `reserved2`.
    pub reserved2: u64,
}

/// `struct acpi_slit`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiSlit {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `number_of_localities`.
    pub number_of_localities: u64,
}

/// `struct acpi_hpet`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiHpet {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `event_timer_block_id`.
    pub event_timer_block_id: u32,
    /// `base_address`.
    pub base_address: AcpiGas,
    /// `hpet_number`.
    pub hpet_number: u8,
    /// `main_counter_min_clock_tick`.
    pub main_counter_min_clock_tick: u16,
    /// `page_protection`.
    pub page_protection: u8,
}

/// `struct acpi_mcfg`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMcfg {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `reserved`.
    pub reserved: [u8; 8],
}

/// `struct acpi_mcfg_entry`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiMcfgEntry {
    /// `base_address`.
    pub base_address: u64,
    /// `segment`.
    pub segment: u16,
    /// `min_bus_number`.
    pub min_bus_number: u8,
    /// `max_bus_number`.
    pub max_bus_number: u8,
    /// `reserved1`.
    pub reserved1: u32,
}

/// `struct acpi_spcr`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiSpcr {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `interface_type`.
    pub interface_type: u8,
    /// `reserved1`.
    pub reserved1: [u8; 3],
    /// `base_address`.
    pub base_address: AcpiGas,
    /// `interrupt_type`.
    pub interrupt_type: u8,
    /// `irq`.
    pub irq: u8,
    /// `gsiv`.
    pub gsiv: u32,
    /// `baud_rate`.
    pub baud_rate: u8,
    /// `parity`.
    pub parity: u8,
    /// `stop_bits`.
    pub stop_bits: u8,
    /// `flow_control`.
    pub flow_control: u8,
    /// `terminal_type`.
    pub terminal_type: u8,
    /// `reserved2`.
    pub reserved2: u8,
    /// `pci_device_id`.
    pub pci_device_id: u16,
    /// `pci_vendor_id`.
    pub pci_vendor_id: u16,
    /// `pci_bus`.
    pub pci_bus: u8,
    /// `pci_device`.
    pub pci_device: u8,
    /// `pci_function`.
    pub pci_function: u8,
    /// `pci_flags`.
    pub pci_flags: u32,
    /// `pci_segment`.
    pub pci_segment: u8,
    /// `reserved3`.
    pub reserved3: u32,
}

/// `struct acpi_facs`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiFacs {
    /// `signature`.
    pub signature: [u8; 4],
    /// `length`.
    pub length: u32,
    /// `hardware_signature`.
    pub hardware_signature: u32,
    /// `wakeup_vector`.
    pub wakeup_vector: u32,
    /// `global_lock`.
    pub global_lock: u32,
    /// `flags`.
    pub flags: u32,
    /// `x_wakeup_vector`.
    pub x_wakeup_vector: u64,
    /// `version`.
    pub version: u8,
    /// `reserved`.
    pub reserved: [u8; 31],
}

/// `struct acpi_tpm2`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiTpm2 {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `reserved`.
    pub reserved: u32,
    /// `control_addr`.
    pub control_addr: u64,
    /// `start_method`.
    pub start_method: u32,
}

/// `struct acpi_lpit`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiLpit {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
}

/// `struct acpi_lpit_entry`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AcpiLpitEntry {
    /// `type`.
    pub r#type: u32,
    /// `length`.
    pub length: u32,
    /// `uid`.
    pub uid: u16,
    /// `reserved`.
    pub reserved: u16,
    /// `flags`.
    pub flags: u32,
    /// `entry_trigger`.
    pub entry_trigger: AcpiGas,
    /// `residency`.
    pub residency: u32,
    /// `latency`.
    pub latency: u32,
    /// `residency_counter`.
    pub residency_counter: AcpiGas,
    /// `residency_frequency`.
    pub residency_frequency: u64,
}

/// `struct acpidmar_devpath`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpidmarDevpath {
    /// `device`.
    pub device: u8,
    /// `function`.
    pub function: u8,
}

/// `struct acpidmar_devscope`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpidmarDevscope {
    /// `type`.
    pub r#type: u8,
    /// `length`.
    pub length: u8,
    /// `reserved`.
    pub reserved: u16,
    /// `enumid`.
    pub enumid: u8,
    /// `bus`.
    pub bus: u8,
}

/// `struct acpidmar_drhd`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpidmarDrhd {
    /// `type`.
    pub r#type: u16,
    /// `length`.
    pub length: u16,
    /// `flags`.
    pub flags: u8,
    /// `reserved`.
    pub reserved: u8,
    /// `segment`.
    pub segment: u16,
    /// `address`.
    pub address: u64,
}

/// `struct acpidmar_rmrr`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpidmarRmrr {
    /// `type`.
    pub r#type: u16,
    /// `length`.
    pub length: u16,
    /// `reserved`.
    pub reserved: u16,
    /// `segment`.
    pub segment: u16,
    /// `base`.
    pub base: u64,
    /// `limit`.
    pub limit: u64,
}

/// `struct acpidmar_atsr`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpidmarAtsr {
    /// `type`.
    pub r#type: u16,
    /// `length`.
    pub length: u16,
    /// `flags`.
    pub flags: u8,
    /// `reserved`.
    pub reserved: u8,
    /// `segment`.
    pub segment: u16,
}

/// The anonymous `struct { type; length; }` member of `union acpidmar_entry`, named
/// `hdr` here (Rust has no anonymous members).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpidmarEntryHdr {
    /// `type`: `DMAR_DRHD`, `DMAR_RMRR`, `DMAR_ATSR` or `DMAR_RHSA`.
    pub r#type: u16,
    /// `length`.
    pub length: u16,
}

/// `union acpidmar_entry`: a remapping structure of the DMAR table; `hdr` is the C's
/// anonymous `type`/`length` member.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub union AcpidmarEntry {
    /// The C's anonymous `struct { type; length; }`.
    pub hdr: AcpidmarEntryHdr,
    /// `drhd`.
    pub drhd: AcpidmarDrhd,
    /// `rmrr`.
    pub rmrr: AcpidmarRmrr,
    /// `atsr`.
    pub atsr: AcpidmarAtsr,
}

/// `struct acpi_dmar`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiDmar {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `haw`.
    pub haw: u8,
    /// `flags`.
    pub flags: u8,
    /// `reserved`.
    pub reserved: [u8; 10],
}

/// The `resvd` member of `union acpi_ivhd_entry` (an anonymous `struct` in C).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIvhdEntryResvd {
    /// `type`.
    pub r#type: u8,
    /// `resvd`.
    pub resvd: [u8; 3],
}

/// The `all` member of `union acpi_ivhd_entry` (an anonymous `struct` in C).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIvhdEntryAll {
    /// `type`.
    pub r#type: u8,
    /// `resvd`.
    pub resvd: u16,
    /// `data`.
    pub data: u8,
}

/// The `sel` member of `union acpi_ivhd_entry` (an anonymous `struct` in C).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIvhdEntrySel {
    /// `type`.
    pub r#type: u8,
    /// `devid`.
    pub devid: u16,
    /// `data`.
    pub data: u8,
}

/// The `sor` member of `union acpi_ivhd_entry` (an anonymous `struct` in C).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIvhdEntrySor {
    /// `type`.
    pub r#type: u8,
    /// `devid`.
    pub devid: u16,
    /// `data`.
    pub data: u8,
}

/// The `eor` member of `union acpi_ivhd_entry` (an anonymous `struct` in C).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIvhdEntryEor {
    /// `type`.
    pub r#type: u8,
    /// `devid`.
    pub devid: u16,
    /// `resvd`.
    pub resvd: u8,
}

/// The `alias` member of `union acpi_ivhd_entry` (an anonymous `struct` in C).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIvhdEntryAlias {
    /// `type`.
    pub r#type: u8,
    /// `devid`.
    pub devid: u16,
    /// `data`.
    pub data: u8,
    /// `resvd1`.
    pub resvd1: u8,
    /// `srcid`.
    pub srcid: u16,
    /// `resvd2`.
    pub resvd2: u8,
}

/// The `ext` member of `union acpi_ivhd_entry` (an anonymous `struct` in C).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIvhdEntryExt {
    /// `type`.
    pub r#type: u8,
    /// `devid`.
    pub devid: u16,
    /// `data`.
    pub data: u8,
    /// `extdata`.
    pub extdata: u32,
}

/// The `special` member of `union acpi_ivhd_entry` (an anonymous `struct` in C).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIvhdEntrySpecial {
    /// `type`.
    pub r#type: u8,
    /// `resvd`.
    pub resvd: u16,
    /// `data`.
    pub data: u8,
    /// `handle`.
    pub handle: u8,
    /// `devid`.
    pub devid: u16,
    /// `variety`.
    pub variety: u8,
}

/// `union acpi_ivhd_entry`: a device entry of an IVHD block; `type` tells the members apart.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub union AcpiIvhdEntry {
    /// `type`: `IVHD_RESVD` ... `IVHD_SPECIAL`.
    pub r#type: u8,
    /// `resvd`.
    pub resvd: AcpiIvhdEntryResvd,
    /// `all`.
    pub all: AcpiIvhdEntryAll,
    /// `sel`.
    pub sel: AcpiIvhdEntrySel,
    /// `sor`.
    pub sor: AcpiIvhdEntrySor,
    /// `eor`.
    pub eor: AcpiIvhdEntryEor,
    /// `alias`.
    pub alias: AcpiIvhdEntryAlias,
    /// `ext`.
    pub ext: AcpiIvhdEntryExt,
    /// `special`.
    pub special: AcpiIvhdEntrySpecial,
}

/// `struct acpi_ivmd`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIvmd {
    /// `type`.
    pub r#type: u8,
    /// `flags`.
    pub flags: u8,
    /// `length`.
    pub length: u16,
    /// `devid`.
    pub devid: u16,
    /// `auxdata`.
    pub auxdata: u16,
    /// `reserved`.
    pub reserved: [u8; 8],
    /// `start_address`.
    pub start_address: u64,
    /// `block_length`.
    pub block_length: u64,
}

/// `struct acpi_ivhd`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIvhd {
    /// `type`.
    pub r#type: u8,
    /// `flags`.
    pub flags: u8,
    /// `length`.
    pub length: u16,
    /// `devid`.
    pub devid: u16,
    /// `cap`.
    pub cap: u16,
    /// `address`.
    pub address: u64,
    /// `segment`.
    pub segment: u16,
    /// `info`.
    pub info: u16,
    /// `feature`.
    pub feature: u32,
}

/// `struct acpi_ivhd_ext`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIvhdExt {
    /// `type`.
    pub r#type: u8,
    /// `flags`.
    pub flags: u8,
    /// `length`.
    pub length: u16,
    /// `devid`.
    pub devid: u16,
    /// `cap`.
    pub cap: u16,
    /// `address`.
    pub address: u64,
    /// `segment`.
    pub segment: u16,
    /// `info`.
    pub info: u16,
    /// `attrib`.
    pub attrib: u32,
    /// `efr`.
    pub efr: u64,
    /// `reserved`.
    pub reserved: [u8; 8],
}

/// The anonymous `struct { type; flags; length; }` member of `union acpi_ivrs_entry`,
/// named `hdr` here (Rust has no anonymous members).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIvrsEntryHdr {
    /// `type`: `IVRS_IVHD`, `IVRS_IVHD_EXT`, `IVRS_IVMD_ALL`, `IVRS_IVMD_SPECIFIED` or
    /// `IVRS_IVMD_RANGE`.
    pub r#type: u8,
    /// `flags`.
    pub flags: u8,
    /// `length`.
    pub length: u16,
}

/// `union acpi_ivrs_entry`: an IVRS entry; `hdr` is the C's anonymous `type`/`flags`/`length`
/// member.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub union AcpiIvrsEntry {
    /// The C's anonymous `struct { type; flags; length; }`.
    pub hdr: AcpiIvrsEntryHdr,
    /// `ivhd`.
    pub ivhd: AcpiIvhd,
    /// `ivhd_ext`.
    pub ivhd_ext: AcpiIvhdExt,
    /// `ivmd`.
    pub ivmd: AcpiIvmd,
}

/// `struct acpi_ivrs`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIvrs {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `ivinfo`.
    pub ivinfo: u32,
    /// `reserved`.
    pub reserved: [u8; 8],
}

/// `struct acpi_iort`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIort {
    /// `hdr`.
    pub hdr: AcpiTableHeader,
    /// `number_of_nodes`.
    pub number_of_nodes: u32,
    /// `offset`.
    pub offset: u32,
    /// `reserved`.
    pub reserved: u32,
}

/// `struct acpi_iort_node`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIortNode {
    /// `type`.
    pub r#type: u8,
    /// `length`.
    pub length: u16,
    /// `revision`.
    pub revision: u8,
    /// `reserved1`.
    pub reserved1: u32,
    /// `number_of_mappings`.
    pub number_of_mappings: u32,
    /// `mapping_offset`.
    pub mapping_offset: u32,
}

/// `struct acpi_iort_its_node`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIortItsNode {
    /// `number_of_itss`.
    pub number_of_itss: u32,
    /// `its_ids`.
    pub its_ids: [u32; 0],
}

/// `struct acpi_iort_nc_node`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIortNcNode {
    /// `node_flags`.
    pub node_flags: u32,
    /// `memory_access_properties`.
    pub memory_access_properties: u64,
    /// `device_memory_address_size_limit`.
    pub device_memory_address_size_limit: u8,
    /// `device_object_name`.
    pub device_object_name: [u8; 0],
}

/// `struct acpi_iort_rc_node`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIortRcNode {
    /// `memory_access_properties`.
    pub memory_access_properties: u64,
    /// `ats_attributes`.
    pub ats_attributes: u32,
    /// `segment`.
    pub segment: u32,
    /// `memory_address_size_limit`.
    pub memory_address_size_limit: u8,
    /// `reserved2`.
    pub reserved2: [u8; 3],
}

/// `struct acpi_iort_smmu_node`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIortSmmuNode {
    /// `base_address`.
    pub base_address: u64,
    /// `span`.
    pub span: u64,
    /// `model`.
    pub model: u32,
    /// `flags`.
    pub flags: u32,
    /// `global_interrupt_offset`.
    pub global_interrupt_offset: u32,
    /// `number_of_context_interrupts`.
    pub number_of_context_interrupts: u32,
    /// `context_interrupt_offset`.
    pub context_interrupt_offset: u32,
    /// `number_of_pmu_interrupts`.
    pub number_of_pmu_interrupts: u32,
    /// `pmu_interrupt_offset`.
    pub pmu_interrupt_offset: u32,
}

/// `struct acpi_iort_smmu_global_interrupt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIortSmmuGlobalInterrupt {
    /// `nsgirpt_gsiv`.
    pub nsgirpt_gsiv: u32,
    /// `nsgirpt_flags`.
    pub nsgirpt_flags: u32,
    /// `nscfgirpt_gsiv`.
    pub nscfgirpt_gsiv: u32,
    /// `nscfgirpt_flags`.
    pub nscfgirpt_flags: u32,
}

/// `struct acpi_iort_smmu_context_interrupt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIortSmmuContextInterrupt {
    /// `gsiv`.
    pub gsiv: u32,
    /// `flags`.
    pub flags: u32,
}

/// `struct acpi_iort_smmu_pmu_interrupt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIortSmmuPmuInterrupt {
    /// `gsiv`.
    pub gsiv: u32,
    /// `flags`.
    pub flags: u32,
}

/// `struct acpi_iort_smmu_v3_node`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIortSmmuV3Node {
    /// `base_address`.
    pub base_address: u64,
    /// `flags`.
    pub flags: u32,
    /// `reserved`.
    pub reserved: u32,
    /// `vatos_address`.
    pub vatos_address: u64,
    /// `model`.
    pub model: u32,
    /// `event`.
    pub event: u32,
    /// `pri`.
    pub pri: u32,
    /// `gerr`.
    pub gerr: u32,
    /// `sync`.
    pub sync: u32,
    /// `proximity_domain`.
    pub proximity_domain: u32,
    /// `deviceid_mapping_index`.
    pub deviceid_mapping_index: u32,
}

/// `struct acpi_iort_mapping`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AcpiIortMapping {
    /// `input_base`.
    pub input_base: u32,
    /// `number_of_ids`.
    pub number_of_ids: u32,
    /// `output_base`.
    pub output_base: u32,
    /// `output_reference`.
    pub output_reference: u32,
    /// `flags`.
    pub flags: u32,
}

const _: () = {
    assert!(size_of::<AcpiRsdp1>() == 20);
    assert!(size_of::<AcpiRsdp>() == 36);
    assert!(size_of::<AcpiTableHeader>() == 36);
    assert!(size_of::<AcpiRsdt>() == 40);
    assert!(size_of::<AcpiXsdt>() == 44);
    assert!(size_of::<AcpiGas>() == 12);
    assert!(size_of::<AcpiFadt>() == 268);
    assert!(size_of::<AcpiDsdt>() == 37);
    assert!(size_of::<AcpiSsdt>() == 37);
    assert!(size_of::<AcpiPsdt>() == 36);
    assert!(size_of::<AcpiMadt>() == 44);
    assert!(size_of::<AcpiMadtLapic>() == 8);
    assert!(size_of::<AcpiMadtIoapic>() == 12);
    assert!(size_of::<AcpiMadtOverride>() == 10);
    assert!(size_of::<AcpiMadtNmi>() == 8);
    assert!(size_of::<AcpiMadtLapicNmi>() == 6);
    assert!(size_of::<AcpiMadtLapicOverride>() == 12);
    assert!(size_of::<AcpiMadtIoSapic>() == 16);
    assert!(size_of::<AcpiMadtLocalSapic>() == 17);
    assert!(size_of::<AcpiMadtPlatformInt>() == 16);
    assert!(size_of::<AcpiMadtX2apic>() == 16);
    assert!(size_of::<AcpiMadtX2apicNmi>() == 12);
    assert!(size_of::<AcpiMadtGicc>() == 80);
    assert!(size_of::<AcpiMadtEntry>() == 17);
    assert!(size_of::<AcpiSbst>() == 48);
    assert!(size_of::<AcpiEcdt>() == 66);
    assert!(size_of::<AcpiSrat>() == 48);
    assert!(size_of::<AcpiSlit>() == 44);
    assert!(size_of::<AcpiHpet>() == 56);
    assert!(size_of::<AcpiMcfg>() == 44);
    assert!(size_of::<AcpiMcfgEntry>() == 16);
    assert!(size_of::<AcpiSpcr>() == 80);
    assert!(size_of::<AcpiFacs>() == 64);
    assert!(size_of::<AcpiTpm2>() == 52);
    assert!(size_of::<AcpiLpit>() == 36);
    assert!(size_of::<AcpiLpitEntry>() == 56);
    assert!(size_of::<AcpidmarDevpath>() == 2);
    assert!(size_of::<AcpidmarDevscope>() == 6);
    assert!(size_of::<AcpidmarDrhd>() == 16);
    assert!(size_of::<AcpidmarRmrr>() == 24);
    assert!(size_of::<AcpidmarAtsr>() == 8);
    assert!(size_of::<AcpidmarEntry>() == 24);
    assert!(size_of::<AcpiDmar>() == 48);
    assert!(size_of::<AcpiIvhdEntryResvd>() == 4);
    assert!(size_of::<AcpiIvhdEntryAll>() == 4);
    assert!(size_of::<AcpiIvhdEntrySel>() == 4);
    assert!(size_of::<AcpiIvhdEntrySor>() == 4);
    assert!(size_of::<AcpiIvhdEntryEor>() == 4);
    assert!(size_of::<AcpiIvhdEntryAlias>() == 8);
    assert!(size_of::<AcpiIvhdEntryExt>() == 8);
    assert!(size_of::<AcpiIvhdEntrySpecial>() == 8);
    assert!(size_of::<AcpiIvhdEntry>() == 8);
    assert!(size_of::<AcpiIvmd>() == 32);
    assert!(size_of::<AcpiIvhd>() == 24);
    assert!(size_of::<AcpiIvhdExt>() == 40);
    assert!(size_of::<AcpiIvrsEntry>() == 40);
    assert!(size_of::<AcpiIvrs>() == 48);
    assert!(size_of::<AcpiIort>() == 48);
    assert!(size_of::<AcpiIortNode>() == 16);
    assert!(size_of::<AcpiIortItsNode>() == 4);
    assert!(size_of::<AcpiIortNcNode>() == 13);
    assert!(size_of::<AcpiIortRcNode>() == 20);
    assert!(size_of::<AcpiIortSmmuNode>() == 44);
    assert!(size_of::<AcpiIortSmmuGlobalInterrupt>() == 16);
    assert!(size_of::<AcpiIortSmmuContextInterrupt>() == 8);
    assert!(size_of::<AcpiIortSmmuPmuInterrupt>() == 8);
    assert!(size_of::<AcpiIortSmmuV3Node>() == 52);
    assert!(size_of::<AcpiIortMapping>() == 20);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests for `acpireg`.

    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/acpi/acpireg.h");
        // Not compared (the helper reads one line of `#define`): the string constants (`*_SIG`,
        // `ACPI_DEV_*`) and the function-like macros.
        crate::reftest::assert_defines!(defs;
        GAS_SYSTEM_MEMORY, GAS_SYSTEM_IOSPACE, GAS_PCI_CFG_SPACE, GAS_EMBEDDED, GAS_SMBUS,
        GAS_FUNCTIONAL_FIXED, GAS_ACCESS_UNDEFINED, GAS_ACCESS_BYTE, GAS_ACCESS_WORD,
        GAS_ACCESS_DWORD, GAS_ACCESS_QWORD, FADT_INT_DUAL_PIC, FADT_INT_MULTI_APIC, FADT_PM_UNSPEC,
        FADT_PM_DESKTOP, FADT_PM_MOBILE, FADT_PM_WORKSTATION, FADT_PM_ENT_SERVER,
        FADT_PM_SOHO_SERVER, FADT_PM_APPLIANCE, FADT_PM_PERF_SERVER, FADT_LEGACY_DEVICES,
        FADT_i8042, FADT_NO_VGA, FADT_NO_MSI, FADT_WBINVD, FADT_WBINVD_FLUSH, FADT_PROC_C1,
        FADT_P_LVL2_UP, FADT_PWR_BUTTON, FADT_SLP_BUTTON, FADT_FIX_RTC, FADT_RTC_S4,
        FADT_TMR_VAL_EXT, FADT_DCK_CAP, FADT_RESET_REG_SUP, FADT_SEALED_CASE, FADT_HEADLESS,
        FADT_CPU_SW_SLP, FADT_PCI_EXP_WAK, FADT_USE_PLATFORM_CLOCK, FADT_S4_RTC_STS_VALID,
        FADT_REMOTE_POWER_ON_CAPABLE, FADT_FORCE_APIC_CLUSTER_MODEL, FADT_FORCE_APIC_PHYS_DEST_MODE,
        FADT_HW_REDUCED_ACPI, FADT_POWER_S0_IDLE_CAPABLE, ACPI_APIC_PCAT_COMPAT, ACPI_MADT_LAPIC,
        ACPI_PROC_ENABLE, ACPI_MADT_IOAPIC, ACPI_MADT_OVERRIDE, ACPI_OVERRIDE_BUS_ISA,
        ACPI_OVERRIDE_POLARITY_BITS, ACPI_OVERRIDE_POLARITY_BUS, ACPI_OVERRIDE_POLARITY_HIGH,
        ACPI_OVERRIDE_POLARITY_LOW, ACPI_OVERRIDE_TRIGGER_BITS, ACPI_OVERRIDE_TRIGGER_BUS,
        ACPI_OVERRIDE_TRIGGER_EDGE, ACPI_OVERRIDE_TRIGGER_LEVEL, ACPI_MADT_NMI, ACPI_MADT_LAPIC_NMI,
        ACPI_MADT_LAPIC_OVERRIDE, ACPI_MADT_IO_SAPIC, ACPI_MADT_LOCAL_SAPIC, ACPI_MADT_PLATFORM_INT,
        ACPI_MADT_PLATFORM_PMI, ACPI_MADT_PLATFORM_INIT, ACPI_MADT_PLATFORM_CORR_ERROR,
        ACPI_MADT_PLATFORM_CPEI, ACPI_MADT_X2APIC, ACPI_MADT_X2APIC_NMI, ACPI_MADT_GICC,
        ACPI_MADT_OEM_RSVD, SPCR_16550, SPCR_16450, SPCR_ARM_PL011, SPCR_ARM_SBSA,
        FACS_LOCK_PENDING, FACS_LOCK_OWNED, FACS_S4BIOS_F, LPIT_DISABLED,
        LPIT_COUNTER_NOT_AVAILABLE, DMAR_ENDPOINT, DMAR_BRIDGE, DMAR_IOAPIC, DMAR_HPET, DMAR_DRHD,
        DMAR_RMRR, DMAR_ATSR, DMAR_RHSA, IVHD_RESVD, IVHD_ALL, IVHD_SEL, IVHD_SOR, IVHD_EOR,
        IVHD_ALIAS_SEL, IVHD_ALIAS_SOR, IVHD_EXT_SEL, IVHD_EXT_SOR, IVHD_SPECIAL, IVHD_ATS_DIS,
        IVHD_IOAPIC, IVHD_HPET, IVMD_EXCLRANGE, IVMD_IW, IVMD_IR, IVMD_UNITY, IVHD_PPRSUP,
        IVHD_PREFSUP, IVHD_COHERENT, IVHD_IOTLB, IVHD_ISOC, IVHD_RESPASSPW, IVHD_PASSPW,
        IVHD_HTTUNEN, IVHD_UNITID_SHIFT, IVHD_UNITID_MASK, IVHD_MSINUM_SHIFT, IVHD_MSINUM_MASK,
        IVRS_IVHD, IVRS_IVHD_EXT, IVRS_IVMD_ALL, IVRS_IVMD_SPECIFIED, IVRS_IVMD_RANGE, IVRS_ATSRNG,
        IVRS_VASIZE_SHIFT, IVRS_VASIZE_MASK, IVRS_PASIZE_SHIFT, IVRS_PASIZE_MASK, ACPI_IORT_ITS,
        ACPI_IORT_NAMED_COMPONENT, ACPI_IORT_ROOT_COMPLEX, ACPI_IORT_SMMU, ACPI_IORT_SMMU_V3,
        ACPI_IORT_SMMU_V1, ACPI_IORT_SMMU_V2, ACPI_IORT_SMMU_CORELINK_MMU400,
        ACPI_IORT_SMMU_CORELINK_MMU500, ACPI_IORT_SMMU_CORELINK_MMU401,
        ACPI_IORT_SMMU_CAVIUM_THUNDERX, ACPI_IORT_SMMU_DVM, ACPI_IORT_SMMU_COHERENT,
        ACPI_IORT_SMMU_INTR_EDGE, ACPI_IORT_SMMU_V3_PROX_DOM_VALID,
        ACPI_IORT_SMMU_V3_DEVID_MAP_VALID, ACPI_IORT_SMMU_V3_GENERIC,
        ACPI_IORT_SMMU_V3_HISILICON_HI161X, ACPI_IORT_SMMU_V3_CAVIUM_CN99X,
        ACPI_IORT_MAPPING_SINGLE, ACPI_FREQUENCY, ACPI_PM1_STATUS, ACPI_PM1_TMR_STS,
        ACPI_PM1_BM_STS, ACPI_PM1_GBL_STS, ACPI_PM1_PWRBTN_STS, ACPI_PM1_SLPBTN_STS,
        ACPI_PM1_RTC_STS, ACPI_PM1_PCIEXP_WAKE_STS, ACPI_PM1_WAK_STS, ACPI_PM1_ENABLE,
        ACPI_PM1_TMR_EN, ACPI_PM1_GBL_EN, ACPI_PM1_PWRBTN_EN, ACPI_PM1_SLPBTN_EN, ACPI_PM1_RTC_EN,
        ACPI_PM1_PCIEXP_WAKE_DIS, ACPI_PM1_CONTROL, ACPI_PM1_SCI_EN, ACPI_PM1_BM_RLD,
        ACPI_PM1_GBL_RLS, ACPI_PM1_SLP_TYPX_MASK, ACPI_PM1_SLP_EN, ACPI_PM2_CONTROL,
        ACPI_PM2_ARB_DIS, ACPI_OPREG_SYSMEM, ACPI_OPREG_SYSIO, ACPI_OPREG_PCICFG, ACPI_OPREG_EC,
        ACPI_OPREG_SMBUS, ACPI_OPREG_CMOS, ACPI_OPREG_PCIBAR, ACPI_OPREG_IPMI, ACPI_OPREG_GPIO,
        ACPI_OPREG_GSB, ACPI_STATE_S0, ACPI_STATE_S1, ACPI_STATE_S2, ACPI_STATE_S3, ACPI_STATE_S4,
        ACPI_STATE_S5, ACPI_STATE_D0, ACPI_STATE_D1, ACPI_STATE_D2, ACPI_STATE_D3,
        );
    }

    #[test]
    fn table_sizes_are_the_c_sizes() {
        assert_eq!(size_of::<AcpiTableHeader>(), 36);
        assert_eq!(size_of::<AcpiGas>(), 12);
        assert_eq!(size_of::<AcpiFacs>(), 64);
        assert_eq!(size_of::<AcpiFadt>(), 268);
        assert_eq!(size_of::<AcpiRsdp>(), 36);
        assert_eq!(size_of::<AcpiLpitEntry>(), 56);
    }

    #[test]
    fn field_offsets() {
        use core::mem::offset_of;
        assert_eq!(offset_of!(AcpiFadt, sci_int), 46);
        assert_eq!(offset_of!(AcpiFadt, flags), 112);
        assert_eq!(offset_of!(AcpiFadt, reset_reg), 116);
        assert_eq!(offset_of!(AcpiFadt, x_dsdt), 140);
        assert_eq!(offset_of!(AcpiFacs, x_wakeup_vector), 24);
        assert_eq!(offset_of!(AcpiRsdp, rsdp_xsdt), 24);
    }

    #[test]
    fn signatures() {
        assert_eq!(RSDP_SIG, b"RSD PTR ");
        assert_eq!(FADT_SIG, b"FACP");
        assert_eq!(MADT_SIG, b"APIC");
    }

    #[test]
    fn address_macros() {
        let addr = 0x0123_0456_0007_0008u64;
        assert_eq!(acpi_pci_seg(addr), 0x0123);
        assert_eq!(acpi_pci_bus(addr), 0x04);
        assert_eq!(acpi_pci_dev(addr), 0x56);
        assert_eq!(acpi_pci_fn(addr), 0x0007);
        assert_eq!(acpi_pci_reg(addr), 0x0008);
        assert_eq!(acpi_adr_pcidev(0x001f_0003), 0x1f);
        assert_eq!(acpi_adr_pcifun(0x001f_0003), 3);
        assert_eq!(acpi_pm1_slp_typx(5), 0x1400);
        assert_eq!(acpi_pm1_slp_typx(7) & !ACPI_PM1_SLP_TYPX_MASK, 0);
    }
}
/* </TESTS> */
