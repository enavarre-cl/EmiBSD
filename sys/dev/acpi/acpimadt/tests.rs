//! Host tests of `acpimadt.rs`: the MADT walk and its validation.

use std::vec;
use std::vec::Vec;

use super::*;

/// A MADT header (44 bytes with the local APIC address and flags) followed by `entries`.
fn madt(entries: &[&[u8]]) -> Vec<u8> {
    let mut b = vec![0u8; size_of::<AcpiMadt>()];
    b[..4].copy_from_slice(MADT_SIG);
    for e in entries {
        b.extend_from_slice(e);
    }
    let len = b.len() as u32;
    b[4..8].copy_from_slice(&len.to_le_bytes());
    b
}

/// QEMU's entries: a local APIC, an I/O APIC, an override of IRQ 0 and a LAPIC NMI.
const LAPIC0: [u8; 8] = [ACPI_MADT_LAPIC, 8, 0, 0, 1, 0, 0, 0];
const IOAPIC0: [u8; 12] = [ACPI_MADT_IOAPIC, 12, 0, 0, 0, 0, 0xc0, 0xfe, 0, 0, 0, 0];
const OVERRIDE0: [u8; 10] = [ACPI_MADT_OVERRIDE, 10, 0, 0, 2, 0, 0, 0, 0, 0];
const LAPIC_NMI: [u8; 6] = [ACPI_MADT_LAPIC_NMI, 6, 0xff, 0, 0, 1];

#[test]
fn entry_sizes_are_the_c_ones() {
    assert_eq!(size_of::<AcpiMadt>(), 44);
    assert_eq!(size_of::<AcpiMadtLapic>(), 8);
    assert_eq!(size_of::<AcpiMadtIoapic>(), 12);
    assert_eq!(size_of::<AcpiMadtOverride>(), 10);
    assert_eq!(size_of::<AcpiMadtLapicNmi>(), 6);
    assert_eq!(size_of::<AcpiMadtX2apic>(), 16);
}

#[test]
fn a_well_formed_table_validates_and_walks() {
    let b = madt(&[&LAPIC0, &IOAPIC0, &OVERRIDE0, &LAPIC_NMI]);
    assert!(acpimadt_validate(&b));
    let types: Vec<u8> = madt_entries(&b).map(|(_, t, _)| t).collect();
    assert_eq!(
        types,
        [
            ACPI_MADT_LAPIC,
            ACPI_MADT_IOAPIC,
            ACPI_MADT_OVERRIDE,
            ACPI_MADT_LAPIC_NMI
        ]
    );
    let io = read_at::<AcpiMadtIoapic>(&b, 44 + 8).map(|e| e.address);
    assert_eq!(io, Some(0xfec0_0000));
}

#[test]
fn bad_lengths_are_refused() {
    // A local APIC entry with the I/O APIC's length.
    let mut bad = LAPIC0;
    bad[1] = 12;
    assert!(!acpimadt_validate(&madt(&[&bad, &[0; 4]])));
    // An entry shorter than its header.
    assert!(!acpimadt_validate(&madt(&[&[ACPI_MADT_NMI, 1]])));
    // An entry running past the table.
    let b = madt(&[&LAPIC0]);
    assert!(!acpimadt_validate(&b[..b.len() - 1]));
    // An unknown (OEM) type is only bounds-checked.
    assert!(acpimadt_validate(&madt(&[&[ACPI_MADT_OEM_RSVD, 3, 0]])));
}
