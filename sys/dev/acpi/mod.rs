//! ACPI: OpenBSD `sys/dev/acpi/`. M13 brings the headers, the AML interpreter (`dsdt`), the
//! core (`acpi`, acpi0) and the table checksum (`acpiutil`).

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/acpi/acpi.c
pub mod acpi;
pub mod acpidev;
pub mod acpireg;
pub mod acpiutil;
pub mod acpivar;
pub mod amltypes;
pub mod dsdt;
