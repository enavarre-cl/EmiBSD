//! ACPI: OpenBSD `sys/dev/acpi/`. M13 brings the headers and the AML interpreter
//! (`dsdt`); `acpi` holds only the seam of `acpi.c` the interpreter calls until that file
//! is ported.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/acpi/acpi.c
pub mod acpi;
pub mod acpidev;
pub mod acpireg;
pub mod acpivar;
pub mod amltypes;
pub mod dsdt;
