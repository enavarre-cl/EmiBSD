//! ACPI: OpenBSD `sys/dev/acpi/`. M13 brings the headers, the AML interpreter (`dsdt`), the
//! core (`acpi`, acpi0), the table checksum (`acpiutil`), the timers (`acpitimer`,
//! `acpihpet`), the MADT (`acpimadt`) and the PCI interrupt routing (`acpiprt`).

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/acpi/acpi.c
pub mod acpi;
pub mod acpidev;
pub mod acpihpet;
pub mod acpimadt;
pub mod acpiprt;
pub mod acpireg;
pub mod acpitimer;
pub mod acpiutil;
pub mod acpivar;
pub mod amltypes;
pub mod dsdt;
