//! The ACPI table handlers on synthetic tables like QEMU's `virt`, each run on a copy of
//! the template blob.

use super::*;
use crate::dt_blob::DT_BLOB_TEMPLATE;
use std::vec::Vec;

/// The bytes of a plain-old-data structure.
fn bytes<T: Pod>(v: &T) -> Vec<u8> {
    // SAFETY: `T` is packed plain data: its bytes are initialized.
    unsafe { core::slice::from_raw_parts((v as *const T).cast::<u8>(), core::mem::size_of::<T>()) }
        .to_vec()
}

/// A table header.
fn header(sig: &[u8; 4], length: usize, revision: u8) -> AcpiTableHeader {
    // SAFETY: `Pod`: all zeros is a value.
    let mut h: AcpiTableHeader = read(&[], 0);
    h.signature = *sig;
    h.length = length as u32;
    h.revision = revision;
    h
}

/// A tree on a copy of the template.
fn template() -> (Vec<u8>, Fdt) {
    let mut b = DT_BLOB_TEMPLATE.to_vec();
    let mut t = Fdt::new();
    // SAFETY: `b` is a whole blob only `t` uses; its heap buffer does not move.
    assert_ne!(unsafe { t.init(b.as_mut_ptr()) }, 0);
    (b, t)
}

/// The property `p` of the node at `path`.
fn prop<'a>(t: &'a Fdt, path: &[u8], p: &[u8]) -> Option<&'a [u8]> {
    t.property(t.find_node(path)?, p)
}

fn be32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

#[test]
fn fadt_enables_psci_over_hvc() {
    let (_b, mut t) = template();
    let mut s = AcpiState::new();
    let mut fadt: AcpiFadt = read(&[], 0);
    fadt.hdr = header(FADT_SIG, 268, 6);
    fadt.arm_boot_arch = FADT_PSCI_COMPLIANT | FADT_PSCI_USE_HVC;
    efi_acpi_fadt(&mut t, &mut s, &bytes(&fadt));
    assert!(s.psci);
    assert_eq!(prop(&t, b"/psci", b"status"), Some(&b"okay\0"[..]));
    assert_eq!(prop(&t, b"/psci", b"method"), Some(&b"hvc\0"[..]));
    // ACPI 5.0 has no PSCI flags
    let (_b, mut t) = template();
    let mut s = AcpiState::new();
    fadt.hdr.revision = 4;
    efi_acpi_fadt(&mut t, &mut s, &bytes(&fadt));
    assert_eq!(prop(&t, b"/psci", b"status"), Some(&b"disabled\0"[..]));
}

#[test]
fn gtdt_gives_the_timer_ppis() {
    let (_b, mut t) = template();
    let mut g: AcpiGtdt = read(&[], 0);
    g.hdr = header(GTDT_SIG, 104, 3);
    g.sec_el1_interrupt = 29;
    g.nonsec_el1_interrupt = 30;
    g.virt_el1_interrupt = 27;
    g.nonsec_el2_interrupt = 26;
    g.virt_el2_interrupt = 28;
    g.nonsec_el1_flags = ACPI_GTDT_TIMER_POLARITY_LOW;
    efi_acpi_gtdt(&mut t, &bytes(&g));
    assert_eq!(
        prop(&t, b"/timer", b"interrupts"),
        Some(&be32s(&[1, 13, 4, 1, 14, 8, 1, 11, 4, 1, 10, 4, 1, 12, 4])[..])
    );
    assert_eq!(
        prop(&t, b"/timer", b"interrupt-names"),
        Some(&b"sec-phys\0phys\0virt\0hyp-phys\0hyp-virt\0"[..])
    );
    assert_eq!(prop(&t, b"/timer", b"status"), Some(&b"okay\0"[..]));
}

#[test]
fn madt_makes_cpus_and_a_gicv3() {
    let (mut b, mut t) = template();
    let mut s = AcpiState::new();
    let mut madt = bytes(&{
        let mut m: AcpiMadt = read(&[], 0);
        m.hdr = header(MADT_SIG, 0, 5);
        m
    });
    let mut gicd: AcpiMadtGicd = read(&[], 0);
    gicd.apic_type = ACPI_MADT_GICD;
    gicd.length = 24;
    gicd.base_address = 0x0800_0000;
    gicd.version = 3;
    madt.extend(bytes(&gicd));
    let mut gicr: AcpiMadtGicr = read(&[], 0);
    gicr.apic_type = ACPI_MADT_GICR;
    gicr.length = 16;
    gicr.discovery_base_address = 0x080a_0000;
    gicr.discovery_length = 0x00f6_0000;
    madt.extend(bytes(&gicr));
    for (mpidr, flags) in [(0u64, ACPI_PROC_ENABLE), (1, ACPI_PROC_ENABLE), (2, 0)] {
        let mut c: AcpiMadtGicc = read(&[], 0);
        c.apic_type = ACPI_MADT_GICC;
        c.length = 80;
        c.flags = flags;
        c.mpidr = mpidr;
        madt.extend(bytes(&c));
    }
    let mut its: AcpiMadtGicIts = read(&[], 0);
    its.apic_type = ACPI_MADT_GIC_ITS;
    its.length = 20;
    its.base_address = 0x0808_0000;
    madt.extend(bytes(&its));
    let len = madt.len() as u32;
    madt[4..8].copy_from_slice(&len.to_le_bytes());

    efi_acpi_madt(&mut t, &mut s, &madt);
    t.finalize();

    let mut t2 = Fdt::new();
    // SAFETY: the blob the first tree wrote, used by this one only.
    assert_ne!(unsafe { t2.init(b.as_mut_ptr()) }, 0);
    let t = t2;
    assert!(t.find_node(b"/cpus/cpu@0").is_some());
    assert_eq!(
        prop(&t, b"/cpus/cpu@1", b"reg"),
        Some(&1u64.to_be_bytes()[..])
    );
    assert!(t.find_node(b"/cpus/cpu@2").is_none());
    assert_eq!(
        prop(&t, b"/cpus/cpu@0", b"enable-method"),
        Some(&b"psci\0"[..])
    );
    assert_eq!(
        prop(&t, b"/interrupt-controller", b"compatible"),
        Some(&b"arm,gic-v3\0"[..])
    );
    let reg: Vec<u8> = [0x0800_0000u64, 0x10000, 0x080a_0000, 0x00f6_0000]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();
    assert_eq!(prop(&t, b"/interrupt-controller", b"reg"), Some(&reg[..]));
    assert_eq!(
        prop(&t, b"/interrupt-controller/gic-its@8080000", b"phandle"),
        Some(&2u32.to_ne_bytes()[..])
    );
    assert_eq!(s.its_phandle, 3);
}

#[test]
fn spcr_and_dbg2_name_the_serial_port() {
    let (_b, mut t) = template();
    let mut s = AcpiState::new();
    let mut spcr: AcpiSpcr = read(&[], 0);
    spcr.hdr = header(SPCR_SIG, 80, 2);
    spcr.interface_type = SPCR_ARM_PL011;
    spcr.base_address.access_size = GAS_ACCESS_DWORD;
    spcr.base_address.register_bit_width = 32;
    spcr.base_address.address = 0x0900_0000;
    efi_acpi_spcr(&mut t, &mut s, &bytes(&spcr));
    assert!(s.serial);
    assert_eq!(
        prop(&t, b"/serial", b"compatible"),
        Some(&b"arm,pl011\0"[..])
    );
    let reg: Vec<u8> = [0x0900_0000u64, 0x1000]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();
    assert_eq!(prop(&t, b"/serial", b"reg"), Some(&reg[..]));

    // A 16550 in the DBG2: reg-shift 2, reg-io-width 4 (each as 8 bytes, as the C)
    let (_b, mut t) = template();
    let mut d = bytes(&{
        let mut d: AcpiDbg2 = read(&[], 0);
        d.hdr = header(DBG2_SIG, 0, 0);
        d.info_offset = 44;
        d.info_count = 1;
        d
    });
    let mut info: AcpiDbg2Info = read(&[], 0);
    info.port_type = DBG2_SERIAL;
    info.port_subtype = DBG2_16550;
    info.base_address_offset = 22;
    info.address_size_offset = 34;
    d.extend(bytes(&info));
    let mut gas: AcpiGas = read(&[], 0);
    gas.access_size = GAS_ACCESS_DWORD;
    gas.register_bit_width = 32;
    gas.address = 0xfe21_5040;
    d.extend(bytes(&gas));
    d.extend(0x100u32.to_le_bytes());
    efi_acpi_dbg2(&mut t, &d);
    assert_eq!(
        prop(&t, b"/serial", b"compatible"),
        Some(&b"snps,dw-apb-uart\0"[..])
    );
    assert_eq!(
        prop(&t, b"/serial", b"reg-shift"),
        Some(&u64::from(2u32.to_be()).to_ne_bytes()[..])
    );
    assert_eq!(
        prop(&t, b"/serial", b"reg-io-width"),
        Some(&u64::from(4u32.to_be()).to_ne_bytes()[..])
    );
}
