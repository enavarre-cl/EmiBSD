/* <LICENSES> */
/*
 * Copyright (c) 2018 Mark Kettenis <kettenis@openbsd.org>
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
//! A device tree from the ACPI tables, for firmware that has none (ACPI-only arm64 machines,
//! QEMU's `virt` with ACPI): the template blob (`dt_blob.rs`) gets the PSCI method from the
//! FADT, the timer interrupts from the GTDT, the CPUs and the interrupt controller from the
//! MADT, the serial port from the SPCR (or the DBG2), and the RSDP's address.
//!
//! Upstream: sys/arch/arm64/stand/efiboot/efiacpi.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The tables are read as byte slices of their own length (`hdr.length`), the structures
//!   copied out of them (`#[repr(C, packed)]`, as the C's `__packed` ones) with zeros past
//!   the table's end, where the C reads on.
//! - The handlers take the tree they edit (`Fdt`) and the C's file statics (`psci`, the GIC
//!   values, `serial`, the two `phandle` counters), gathered in [`AcpiState`]; `efi_acpi`
//!   runs them on the global tree and state, the tests on their own.
//! - The rest of the work, after the RSDP is found in the system table, is
//!   [`efi_acpi_rsdp`].
//! - A node the template lacks (`None`) is not edited; the C passes NULL on.

use core::sync::atomic::Ordering;

use efi::include::efi::ACPI_20_TABLE_GUID;
use libkern::staticcell::StaticCell;
use libsa::cons::cnset;
use libsa::printf;
use libsa::snprintf;

use crate::dt_blob::dt_blob_start;
use crate::efiboot::{DMA_CONSTRAINT, st, ttydev};
use crate::fdt::{Fdt, FdtNode, fdt_finalize, fdt_init, fdt_with_tree};

/// `RSDP_SIG`.
const RSDP_SIG: &[u8; 8] = b"RSD PTR ";
/// `FADT_SIG`, `GTDT_SIG`, `MADT_SIG`, `SPCR_SIG`, `DBG2_SIG`.
const FADT_SIG: &[u8; 4] = b"FACP";
const GTDT_SIG: &[u8; 4] = b"GTDT";
const MADT_SIG: &[u8; 4] = b"APIC";
const SPCR_SIG: &[u8; 4] = b"SPCR";
const DBG2_SIG: &[u8; 4] = b"DBG2";

/// `GAS_SYSTEM_MEMORY`.
const GAS_SYSTEM_MEMORY: u8 = 0;
/// `GAS_ACCESS_BYTE`.. `GAS_ACCESS_QWORD`.
const GAS_ACCESS_BYTE: u8 = 1;
const GAS_ACCESS_WORD: u8 = 2;
const GAS_ACCESS_DWORD: u8 = 3;
const GAS_ACCESS_QWORD: u8 = 4;

/// `FADT_PSCI_COMPLIANT`: PSCI is implemented.
const FADT_PSCI_COMPLIANT: u16 = 0x0001;
/// `FADT_PSCI_USE_HVC`: HVC used as PSCI conduit.
const FADT_PSCI_USE_HVC: u16 = 0x0002;

/// `ACPI_GTDT_TIMER_TRIGGER_EDGE`, `ACPI_GTDT_TIMER_POLARITY_LOW`.
const ACPI_GTDT_TIMER_TRIGGER_EDGE: u32 = 0x1;
const ACPI_GTDT_TIMER_POLARITY_LOW: u32 = 0x2;

/// The MADT entry types: `ACPI_MADT_GICC`.. `ACPI_MADT_GIC_ITS`.
const ACPI_MADT_GICC: u8 = 11;
const ACPI_MADT_GICD: u8 = 12;
const ACPI_MADT_GIC_MSI: u8 = 13;
const ACPI_MADT_GICR: u8 = 14;
const ACPI_MADT_GIC_ITS: u8 = 15;
/// `ACPI_PROC_ENABLE`.
const ACPI_PROC_ENABLE: u32 = 0x0000_0001;
/// `ACPI_MADT_GIC_MSI_SPI_SELECT`.
const ACPI_MADT_GIC_MSI_SPI_SELECT: u32 = 0x0000_0001;

/// `SPCR_16550`, `SPCR_16450`, `SPCR_ARM_PL011`, `SPCR_ARM_SBSA`.
const SPCR_16550: u8 = 0;
const SPCR_16450: u8 = 1;
const SPCR_ARM_PL011: u8 = 3;
const SPCR_ARM_SBSA: u8 = 14;

/// `DBG2_SERIAL`; `DBG2_16550`, `DBG2_16450`, `DBG2_ARM_PL011`, `DBG2_ARM_SBSA`.
const DBG2_SERIAL: u16 = 0x8000;
const DBG2_16550: u16 = 0x0000;
const DBG2_16450: u16 = 0x0001;
const DBG2_ARM_PL011: u16 = 0x0003;
const DBG2_ARM_SBSA: u16 = 0x000e;

/// A plain-old-data ACPI structure: every bit pattern is a valid value.
///
/// # Safety
///
/// The implementing type is `#[repr(C, packed)]` and made of integers and arrays of them.
unsafe trait Pod: Copy {}

/// `struct acpi_rsdp` (with `struct acpi_rsdp1`).
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiRsdp {
    signature: [u8; 8],
    checksum: u8,
    oemid: [u8; 6],
    revision: u8,
    rsdt: u32,
    rsdp_length: u32,
    rsdp_xsdt: u64,
    rsdp_extchecksum: u8,
    rsdp_reserved: [u8; 3],
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiRsdp {}

/// `struct acpi_table_header`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiTableHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oemid: [u8; 6],
    oemtableid: [u8; 8],
    oemrevision: u32,
    aslcompilerid: [u8; 4],
    aslcompilerrevision: u32,
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiTableHeader {}

/// `struct acpi_gas`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiGas {
    address_space_id: u8,
    register_bit_width: u8,
    register_bit_offset: u8,
    access_size: u8,
    address: u64,
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiGas {}

/// `struct acpi_fadt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiFadt {
    hdr: AcpiTableHeader,
    firmware_ctl: u32,
    dsdt: u32,
    int_model: u8,
    pm_profile: u8,
    sci_int: u16,
    smi_cmd: u32,
    acpi_enable: u8,
    acpi_disable: u8,
    s4bios_req: u8,
    pstate_cnt: u8,
    pm1a_evt_blk: u32,
    pm1b_evt_blk: u32,
    pm1a_cnt_blk: u32,
    pm1b_cnt_blk: u32,
    pm2_cnt_blk: u32,
    pm_tmr_blk: u32,
    gpe0_blk: u32,
    gpe1_blk: u32,
    pm1_evt_len: u8,
    pm1_cnt_len: u8,
    pm2_cnt_len: u8,
    pm_tmr_len: u8,
    gpe0_blk_len: u8,
    gpe1_blk_len: u8,
    gpe1_base: u8,
    cst_cnt: u8,
    p_lvl2_lat: u16,
    p_lvl3_lat: u16,
    flush_size: u16,
    flush_stride: u16,
    duty_offset: u8,
    duty_width: u8,
    day_alrm: u8,
    mon_alrm: u8,
    century: u8,
    iapc_boot_arch: u16,
    reserved1: u8,
    flags: u32,
    reset_reg: AcpiGas,
    reset_value: u8,
    arm_boot_arch: u16,
    reserved2: u8,
    x_firmware_ctl: u64,
    x_dsdt: u64,
    x_pm1a_evt_blk: AcpiGas,
    x_pm1b_evt_blk: AcpiGas,
    x_pm1a_cnt_blk: AcpiGas,
    x_pm1b_cnt_blk: AcpiGas,
    x_pm2_cnt_blk: AcpiGas,
    x_pm_tmr_blk: AcpiGas,
    x_gpe0_blk: AcpiGas,
    x_gpe1_blk: AcpiGas,
    sleep_control_reg: AcpiGas,
    sleep_status_reg: AcpiGas,
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiFadt {}

/// `struct acpi_gtdt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiGtdt {
    hdr: AcpiTableHeader,
    cnt_ctrl_base: u64,
    reserved: u32,
    sec_el1_interrupt: u32,
    sec_el1_flags: u32,
    nonsec_el1_interrupt: u32,
    nonsec_el1_flags: u32,
    virt_el1_interrupt: u32,
    virt_el1_flags: u32,
    nonsec_el2_interrupt: u32,
    nonsec_el2_flags: u32,
    cnt_read_base: u64,
    platform_timer_count: u32,
    platform_timer_offset: u32,
    virt_el2_interrupt: u32,
    virt_el2_flags: u32,
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiGtdt {}

/// `struct acpi_madt`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiMadt {
    hdr: AcpiTableHeader,
    local_apic_address: u32,
    flags: u32,
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiMadt {}

/// `struct acpi_madt_gicc`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiMadtGicc {
    apic_type: u8,
    length: u8,
    reserved1: u16,
    gic_id: u32,
    acpi_proc_uid: u32,
    flags: u32,
    parking_protocol_version: u32,
    performance_interrupt: u32,
    parked_address: u64,
    base_address: u64,
    gicv_base_address: u64,
    gich_base_address: u64,
    maintenance_interrupt: u32,
    gicr_base_address: u64,
    mpidr: u64,
    efficiency_class: u8,
    reserved2: [u8; 3],
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiMadtGicc {}

/// `struct acpi_madt_gicd`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiMadtGicd {
    apic_type: u8,
    length: u8,
    reserved1: u16,
    gic_id: u32,
    base_address: u64,
    interrupt_base: u32,
    version: u8,
    reserved2: [u8; 3],
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiMadtGicd {}

/// `struct acpi_madt_gic_msi`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiMadtGicMsi {
    apic_type: u8,
    length: u8,
    reserved1: u16,
    msi_frame_id: u32,
    base_address: u64,
    flags: u32,
    spi_count: u16,
    spi_base: u16,
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiMadtGicMsi {}

/// `struct acpi_madt_gicr`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiMadtGicr {
    apic_type: u8,
    length: u8,
    reserved1: u16,
    discovery_base_address: u64,
    discovery_length: u32,
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiMadtGicr {}

/// `struct acpi_madt_gic_its`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiMadtGicIts {
    apic_type: u8,
    length: u8,
    reserved1: u16,
    gic_its_id: u32,
    base_address: u64,
    reserved2: u32,
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiMadtGicIts {}

/// `struct acpi_spcr`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiSpcr {
    hdr: AcpiTableHeader,
    interface_type: u8,
    reserved1: [u8; 3],
    base_address: AcpiGas,
    interrupt_type: u8,
    irq: u8,
    gsiv: u32,
    baud_rate: u8,
    parity: u8,
    stop_bits: u8,
    flow_control: u8,
    terminal_type: u8,
    reserved2: u8,
    pci_device_id: u16,
    pci_vendor_id: u16,
    pci_bus: u8,
    pci_device: u8,
    pci_function: u8,
    pci_flags: u32,
    pci_segment: u8,
    reserved3: u32,
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiSpcr {}

/// `struct acpi_dbg2`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiDbg2 {
    hdr: AcpiTableHeader,
    info_offset: u32,
    info_count: u32,
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiDbg2 {}

/// `struct acpi_dbg2_info`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // the C's layout: every member, read or not
struct AcpiDbg2Info {
    revision: u8,
    length: u16,
    num_address: u8,
    name_length: u16,
    name_offset: u16,
    oem_data_length: u16,
    oem_data_offset: u16,
    port_type: u16,
    port_subtype: u16,
    reserved: u16,
    base_address_offset: u16,
    address_size_offset: u16,
}
// SAFETY: packed integers.
unsafe impl Pod for AcpiDbg2Info {}

/// The C's file statics: what the MADT entries leave for the interrupt controller node,
/// whether a serial port was found, and the two `phandle` counters.
pub struct AcpiState {
    /// `psci`.
    psci: bool,
    /// `gic_version`.
    gic_version: u8,
    /// `gicc_base`.
    gicc_base: u64,
    /// `gicd_base`.
    gicd_base: u64,
    /// `gicr_base`.
    gicr_base: u64,
    /// `gicr_size`.
    gicr_size: u64,
    /// `gicr_stride`.
    gicr_stride: u64,
    /// `serial`.
    serial: bool,
    /// `efi_acpi_madt_gic_msi`'s `phandle`.
    msi_phandle: u32,
    /// `efi_acpi_madt_gic_its`'s `phandle`.
    its_phandle: u32,
}

impl AcpiState {
    /// The statics' initial values.
    pub const fn new() -> Self {
        Self {
            psci: false,
            gic_version: 0,
            gicc_base: 0,
            gicd_base: 0,
            gicr_base: 0,
            gicr_size: 0,
            gicr_stride: 0,
            serial: false,
            msi_phandle: 2,
            its_phandle: 2,
        }
    }
}

impl Default for AcpiState {
    fn default() -> Self {
        Self::new()
    }
}

/// The file statics of `efi_acpi`.
static ACPI_STATE: StaticCell<AcpiState> = StaticCell::new(AcpiState::new());

/// A `T` read from `b` at `off`, zeros past the end of `b`.
fn read<T: Pod>(b: &[u8], off: usize) -> T {
    let mut buf = [0u8; 512];
    let n = core::mem::size_of::<T>().min(buf.len());
    if off < b.len() {
        let m = n.min(b.len() - off);
        buf[..m].copy_from_slice(&b[off..off + m]);
    }
    // SAFETY: `T` is `Pod` (any bytes are a value), at most 512 bytes (asserted below for
    // the largest), and read unaligned.
    unsafe { buf.as_ptr().cast::<T>().read_unaligned() }
}

/// `fdt_node_add_string_property(n, p, s)`.
fn add_string(t: &mut Fdt, n: Option<FdtNode>, p: &[u8], s: &[u8]) {
    if let Some(n) = n {
        let mut v = alloc::vec::Vec::with_capacity(s.len() + 1);
        v.extend_from_slice(s);
        v.push(0);
        t.add_property(n, p, &v);
    }
}

/// `fdt_node_set_string_property(n, p, s)`.
fn set_string(t: &mut Fdt, n: Option<FdtNode>, p: &[u8], s: &[u8]) {
    if let Some(n) = n {
        let mut v = alloc::vec::Vec::with_capacity(s.len() + 1);
        v.extend_from_slice(s);
        v.push(0);
        t.set_property(n, p, &v);
    }
}

/// `fdt_node_add_property(n, p, data, len)` on an optional node.
fn add_prop(t: &mut Fdt, n: Option<FdtNode>, p: &[u8], data: &[u8]) {
    if let Some(n) = n {
        t.add_property(n, p, data);
    }
}

/// `fdt_node_set_property(n, p, data, len)` on an optional node.
fn set_prop(t: &mut Fdt, n: Option<FdtNode>, p: &[u8], data: &[u8]) {
    if let Some(n) = n {
        t.set_property(n, p, data);
    }
}

/// Big-endian 64-bit words as bytes (`htobe64` into a `uint64_t[]`).
fn be64s<const N: usize>(w: [u64; N]) -> alloc::vec::Vec<u8> {
    w.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// `efi_acpi_fadt(hdr)`: PSCI, and its conduit.
fn efi_acpi_fadt(t: &mut Fdt, s: &mut AcpiState, hdr: &[u8]) {
    let fadt: AcpiFadt = read(hdr, 0);

    // The PSCI flags were introduced in ACPI 5.1. The relevant field is set to zero for
    // ACPU 5.0.
    if fadt.hdr.revision < 5 {
        return;
    }

    let arm_boot_arch = fadt.arm_boot_arch;
    s.psci = arm_boot_arch & FADT_PSCI_COMPLIANT != 0;

    let node = t.find_node(b"/psci");
    if arm_boot_arch & FADT_PSCI_COMPLIANT != 0 {
        set_string(t, node, b"status", b"okay");
    }
    if arm_boot_arch & FADT_PSCI_USE_HVC != 0 {
        set_string(t, node, b"method", b"hvc");
    }
}

/// `efi_acpi_gtdt(hdr)`: the timer's interrupts (all PPIs).
fn efi_acpi_gtdt(t: &mut Fdt, hdr: &[u8]) {
    let gtdt: AcpiGtdt = read(hdr, 0);
    const MAP: [u32; 4] = [0x4, 0x1, 0x8, 0x2];
    let mask = ACPI_GTDT_TIMER_TRIGGER_EDGE | ACPI_GTDT_TIMER_POLARITY_LOW;
    let interrupt_names = b"sec-phys\0phys\0virt\0hyp-phys\0hyp-virt\0";
    let mut interrupts = [0u32; 15];

    let node = t.find_node(b"/timer");

    // All interrupts are supposed to be PPIs.
    let ppi = |irq: u32, flags: u32| [1, irq.wrapping_sub(16), MAP[(flags & mask) as usize]];
    interrupts[0..3].copy_from_slice(&ppi(gtdt.sec_el1_interrupt, gtdt.sec_el1_flags));
    interrupts[3..6].copy_from_slice(&ppi(gtdt.nonsec_el1_interrupt, gtdt.nonsec_el1_flags));
    interrupts[6..9].copy_from_slice(&ppi(gtdt.virt_el1_interrupt, gtdt.virt_el1_flags));
    interrupts[9..12].copy_from_slice(&ppi(gtdt.nonsec_el2_interrupt, gtdt.nonsec_el2_flags));
    let mut len = 12;

    let (revision, length, virt_el2) =
        (gtdt.hdr.revision, gtdt.hdr.length, gtdt.virt_el2_interrupt);
    if revision > 2 && length >= 104 && virt_el2 > 0 {
        interrupts[12..15].copy_from_slice(&ppi(virt_el2, gtdt.virt_el2_flags));
        len = 15;

        set_prop(t, node, b"interrupt-names", interrupt_names);
    }

    let bytes: alloc::vec::Vec<u8> = interrupts[..len]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();
    set_prop(t, node, b"interrupts", &bytes);
    set_string(t, node, b"status", b"okay");
}

/// `efi_acpi_madt_gicc(gicc)`: a CPU node; and the redistributors' base, size and stride
/// when they are given per CPU.
fn efi_acpi_madt_gicc(t: &mut Fdt, s: &mut AcpiState, e: &[u8]) {
    let gicc: AcpiMadtGicc = read(e, 0);

    // Skip disabled CPUs.
    if gicc.flags & ACPI_PROC_ENABLE == 0 {
        return;
    }

    // MPIDR field was introduced in ACPI 5.1. Fall back on the ACPI Processor UID on ACPI
    // 5.0.
    let mpidr = if gicc.length >= 76 {
        gicc.mpidr
    } else {
        u64::from(gicc.acpi_proc_uid)
    };

    let mut name = [0u8; 32];
    let n = snprintf!(&mut name, "cpu@{:x}", mpidr).min(31);
    let reg = mpidr.to_be_bytes();

    // Create "cpu" node.
    let node = t.find_node(b"/cpus");
    let child = node.and_then(|p| t.add_node(p, &name[..n]));
    add_string(t, child, b"device_type", b"cpu");
    add_string(t, child, b"compatible", b"arm,armv8");
    add_prop(t, child, b"reg", &reg);
    if gicc.parking_protocol_version == 0 || s.psci {
        add_string(t, child, b"enable-method", b"psci");
    }

    // Stash GIC information.
    s.gicc_base = gicc.base_address;

    // The redistributor base address may be specified per-CPU. In that case we will need
    // to reconstruct the base, size and stride to use for the redistributor registers.
    let gicr_base_address = gicc.gicr_base_address;
    if gicr_base_address > 0 {
        if s.gicr_base > 0 {
            let size: u32 = if gicr_base_address < s.gicr_base {
                (s.gicr_base - gicr_base_address) as u32
            } else {
                (gicr_base_address - s.gicr_base) as u32
            };
            if s.gicr_stride == 0 || u64::from(size) < s.gicr_stride {
                s.gicr_stride = u64::from(size);
            }
            if s.gicr_size == 0 || u64::from(size) > s.gicr_size {
                s.gicr_size = u64::from(size);
            }
            s.gicr_base = s.gicr_base.min(gicr_base_address);
        } else {
            s.gicr_base = gicr_base_address;
            s.gicr_size = 0x20000;
        }
    }
}

/// `efi_acpi_madt_gicd(gicd)`.
fn efi_acpi_madt_gicd(s: &mut AcpiState, e: &[u8]) {
    let gicd: AcpiMadtGicd = read(e, 0);

    // Stash GIC information.
    s.gic_version = gicd.version;
    s.gicd_base = gicd.base_address;
}

/// `efi_acpi_madt_gic_msi(msi)`: a GICv2m frame node.
fn efi_acpi_madt_gic_msi(t: &mut Fdt, s: &mut AcpiState, e: &[u8]) {
    let msi: AcpiMadtGicMsi = read(e, 0);
    let base_address = msi.base_address;

    let mut name = [0u8; 32];
    let n = snprintf!(&mut name, "v2m@{:x}", base_address).min(31);
    let reg = be64s([base_address, 0x1000]);

    // Create "v2m" node.
    let node = t.find_node(b"/interrupt-controller");
    let child = node.and_then(|p| t.add_node(p, &name[..n]));
    add_string(t, child, b"compatible", b"arm,gic-v2m-frame");
    add_prop(t, child, b"msi-controller", &[]);
    add_prop(t, child, b"reg", &reg);
    if msi.flags & ACPI_MADT_GIC_MSI_SPI_SELECT != 0 {
        let spi_base = u32::from(msi.spi_base).to_be_bytes();
        let spi_count = u32::from(msi.spi_count).to_be_bytes();

        add_prop(t, child, b"arm,msi-base-spi", &spi_base);
        add_prop(t, child, b"arm,msi-num-spis", &spi_count);
    }
    // the C passes the counter in host byte order, not htobe32()'d
    add_prop(t, child, b"phandle", &s.msi_phandle.to_ne_bytes());
    s.msi_phandle += 1;
}

/// `efi_acpi_madt_gicr(gicr)`.
fn efi_acpi_madt_gicr(s: &mut AcpiState, e: &[u8]) {
    let gicr: AcpiMadtGicr = read(e, 0);

    // Stash GIC information.
    s.gicr_base = gicr.discovery_base_address;
    s.gicr_size = u64::from(gicr.discovery_length);
}

/// `efi_acpi_madt_gic_its(its)`: an ITS node.
fn efi_acpi_madt_gic_its(t: &mut Fdt, s: &mut AcpiState, e: &[u8]) {
    let its: AcpiMadtGicIts = read(e, 0);
    let base_address = its.base_address;

    let mut name = [0u8; 32];
    let n = snprintf!(&mut name, "gic-its@{:x}", base_address).min(31);
    let reg = be64s([base_address, 0x20000]);
    let its_id = its.gic_its_id.to_be_bytes();

    // Create "gic-its" node.
    let node = t.find_node(b"/interrupt-controller");
    let child = node.and_then(|p| t.add_node(p, &name[..n]));
    add_string(t, child, b"compatible", b"arm,gic-v3-its");
    add_prop(t, child, b"msi-controller", &[]);
    add_prop(t, child, b"reg", &reg);
    // the C passes the counter in host byte order, not htobe32()'d
    add_prop(t, child, b"phandle", &s.its_phandle.to_ne_bytes());
    add_prop(t, child, b"openbsd,gic-its-id", &its_id);
    s.its_phandle += 1;
}

/// `efi_acpi_madt(hdr)`: the CPUs and the interrupt controller.
fn efi_acpi_madt(t: &mut Fdt, s: &mut AcpiState, hdr: &[u8]) {
    let madt: AcpiMadt = read(hdr, 0);

    // GIC support was introduced in ACPI 5.0.
    if madt.hdr.revision < 3 {
        return;
    }

    let length = madt.hdr.length as usize;
    let mut addr = core::mem::size_of::<AcpiMadt>();
    while addr < length {
        let len = usize::from(hdr.get(addr + 1).copied().unwrap_or(0));

        if len < 2 {
            return;
        }

        if addr + len > length {
            return;
        }

        let e = hdr.get(addr..addr + len).unwrap_or(&[]);
        match hdr.get(addr).copied().unwrap_or(0) {
            ACPI_MADT_GICC => efi_acpi_madt_gicc(t, s, e),
            ACPI_MADT_GICD => efi_acpi_madt_gicd(s, e),
            ACPI_MADT_GIC_MSI => efi_acpi_madt_gic_msi(t, s, e),
            ACPI_MADT_GICR => efi_acpi_madt_gicr(s, e),
            ACPI_MADT_GIC_ITS => efi_acpi_madt_gic_its(t, s, e),
            _ => {}
        }

        addr += len;
    }

    // Now that we've collected all the necessary information, fix up the
    // "interrupt-controller" node.

    let (compat, reg): (&[u8], [u64; 4]) = match s.gic_version {
        // ACPI 5.0 doesn't provide a version; assume GICv2
        0 | 2 => (b"arm,gic-400", [s.gicd_base, 0x1000, s.gicc_base, 0x100]),
        // GICv3 and GICv4
        3 | 4 => (
            b"arm,gic-v3",
            [
                s.gicd_base,
                0x10000,
                s.gicr_base,
                s.gicr_size.wrapping_add(s.gicr_stride),
            ],
        ),
        _ => return,
    };

    // Update "interrupt-controller" node.
    let node = t.find_node(b"/interrupt-controller");
    set_string(t, node, b"compatible", compat);
    set_prop(t, node, b"reg", &be64s(reg));
    if s.gicr_stride > 0 {
        add_prop(
            t,
            node,
            b"redistributor-stride",
            &s.gicr_stride.to_be_bytes(),
        );
    }
    set_string(t, node, b"status", b"okay");
}

/// `efi_acpi_serial(compat, base_address, address_size)`: the serial node.
fn efi_acpi_serial(t: &mut Fdt, compat: &[u8], base_address: AcpiGas, address_size: u32) {
    // No idea how to support anything else on ARM.
    if base_address.address_space_id != GAS_SYSTEM_MEMORY {
        return;
    }

    let reg_io_width: u32 = match base_address.access_size {
        GAS_ACCESS_BYTE => 1,
        GAS_ACCESS_WORD => 2,
        GAS_ACCESS_DWORD => 4,
        GAS_ACCESS_QWORD => 8,
        _ => return,
    };

    let mut reg_shift: u32 = 0;
    if base_address.register_bit_width > 8 {
        reg_shift = 1;
    }
    if base_address.register_bit_width > 16 {
        reg_shift = 2;
    }
    if base_address.register_bit_width > 32 {
        reg_shift = 3;
    }
    // The C keeps htobe32() of each in a uint64_t and passes its 8 bytes.
    let reg_io_width = u64::from(reg_io_width.to_be()).to_ne_bytes();
    let reg_shift = u64::from(reg_shift.to_be()).to_ne_bytes();

    // Update "serial" node.
    let node = t.find_node(b"/serial");
    set_string(t, node, b"compatible", compat);
    if compat == b"snps,dw-apb-uart" {
        add_prop(t, node, b"reg-shift", &reg_shift);
        add_prop(t, node, b"reg-io-width", &reg_io_width);
    }
    let address = base_address.address;
    set_prop(t, node, b"reg", &be64s([address, u64::from(address_size)]));
}

/// `efi_acpi_spcr(hdr)`: the console's serial port.
fn efi_acpi_spcr(t: &mut Fdt, s: &mut AcpiState, hdr: &[u8]) {
    let spcr: AcpiSpcr = read(hdr, 0);

    // Minimal revision required by Server Base Boot Requirements is 2.
    if spcr.hdr.revision < 2 {
        return;
    }

    match spcr.interface_type {
        SPCR_16550 | SPCR_16450 => {
            efi_acpi_serial(t, b"snps,dw-apb-uart", spcr.base_address, 0x100);
        }
        SPCR_ARM_PL011 | SPCR_ARM_SBSA => {
            efi_acpi_serial(t, b"arm,pl011", spcr.base_address, 0x1000);
        }
        _ => return,
    }
    s.serial = true;
}

/// `efi_acpi_dbg2(hdr)`: a serial debug port, when the SPCR named none.
fn efi_acpi_dbg2(t: &mut Fdt, hdr: &[u8]) {
    let dbg2: AcpiDbg2 = read(hdr, 0);
    let info_at = dbg2.info_offset as usize;
    let info: AcpiDbg2Info = read(hdr, info_at);

    // Only looking for serial ports.
    if info.port_type != DBG2_SERIAL {
        return;
    }

    let base_address: AcpiGas = read(hdr, info_at + usize::from(info.base_address_offset));
    let size_at = info_at + usize::from(info.address_size_offset);
    let mut size = [0u8; 4];
    for (i, b) in size.iter_mut().enumerate() {
        *b = hdr.get(size_at + i).copied().unwrap_or(0);
    }
    let address_size = u32::from_le_bytes(size);

    match info.port_subtype {
        DBG2_16550 | DBG2_16450 => {
            efi_acpi_serial(t, b"snps,dw-apb-uart", base_address, address_size);
        }
        DBG2_ARM_PL011 | DBG2_ARM_SBSA => {
            efi_acpi_serial(t, b"arm,pl011", base_address, address_size);
        }
        _ => {}
    }
}

/// `efi_acpi()`: the template blob filled in from the ACPI tables, or null if there are
/// none (no ACPI 2.0 RSDP, or no tables).
pub fn efi_acpi() -> *mut u8 {
    let st = st();
    let mut rsdp: *const u8 = core::ptr::null();

    // We'll never see ACPI 1.0 tables on ARM.
    for i in 0..st.NumberOfTableEntries {
        // SAFETY: the firmware's configuration table has NumberOfTableEntries entries.
        let ct = unsafe { &*st.ConfigurationTable.add(i) };
        if ct.VendorGuid == ACPI_20_TABLE_GUID {
            rsdp = ct.VendorTable.cast();
        }
    }

    if rsdp.is_null() {
        return core::ptr::null_mut();
    }

    // SAFETY: the firmware's RSDP, and its tables, mapped one to one by UEFI.
    unsafe { efi_acpi_rsdp(rsdp) }
}

/// The work of `efi_acpi` from the RSDP on: fill the template blob in from the tables the
/// XSDT names; the blob, or null.
///
/// # Safety
///
/// `rsdp` points at an ACPI 2.0 RSDP whose XSDT and tables are readable at the addresses
/// they give, for their `length` bytes.
pub unsafe fn efi_acpi_rsdp(rsdp_ptr: *const u8) -> *mut u8 {
    // SAFETY: the caller's contract: an RSDP.
    let rsdp: AcpiRsdp = read(unsafe { core::slice::from_raw_parts(rsdp_ptr, 36) }, 0);

    let revision = rsdp.revision;
    if rsdp.signature != *RSDP_SIG || revision < 2 {
        return core::ptr::null_mut();
    }

    let xsdt_ptr = rsdp.rsdp_xsdt as usize as *const u8;
    // SAFETY: the caller's contract: the XSDT's header, then its `length` bytes.
    let xhdr: AcpiTableHeader = read(unsafe { core::slice::from_raw_parts(xsdt_ptr, 36) }, 0);
    let len = xhdr.length as usize;
    // SAFETY: as above.
    let xsdt = unsafe { core::slice::from_raw_parts(xsdt_ptr, len.max(36)) };
    let ntables = len.saturating_sub(core::mem::size_of::<AcpiTableHeader>()) / 8;
    if ntables == 0 {
        return core::ptr::null_mut();
    }

    let fdt = dt_blob_start();
    // SAFETY: the template blob, a static only this code uses (efi_acpi runs once per boot
    // attempt, and nothing holds the tree across it).
    if unsafe { fdt_init(fdt) } == 0 {
        return core::ptr::null_mut();
    }

    let table = |i: usize| -> &'static [u8] {
        let mut a = [0u8; 8];
        a.copy_from_slice(&xsdt[36 + 8 * i..44 + 8 * i]);
        let p = u64::from_le_bytes(a) as usize as *const u8;
        // SAFETY: the caller's contract: each table the XSDT names, header then `length`.
        let h: AcpiTableHeader = read(unsafe { core::slice::from_raw_parts(p, 36) }, 0);
        // SAFETY: as above.
        unsafe { core::slice::from_raw_parts(p, (h.length as usize).max(36)) }
    };

    // SAFETY: single-threaded; the only reference to the state.
    let s = unsafe { ACPI_STATE.get_mut() };
    fdt_with_tree(|t| {
        for i in 0..ntables {
            let hdr = table(i);
            let sig = &hdr[..4];
            printf!(
                "{}{}{}{} ",
                char::from(sig[0]),
                char::from(sig[1]),
                char::from(sig[2]),
                char::from(sig[3])
            );
            if sig == FADT_SIG {
                efi_acpi_fadt(t, s, hdr);
            }
            if sig == GTDT_SIG {
                efi_acpi_gtdt(t, hdr);
            }
            if sig == MADT_SIG {
                efi_acpi_madt(t, s, hdr);
            }
            if sig == SPCR_SIG {
                efi_acpi_spcr(t, s, hdr);
            }
        }
        printf!("\n");

        for i in 0..ntables {
            let hdr = table(i);
            if &hdr[..4] == DBG2_SIG && !s.serial {
                efi_acpi_dbg2(t, hdr);
            }
        }

        let reg = be64s([rsdp_ptr as u64, u64::from(rsdp.rsdp_length)]);

        // Update "acpi" node.
        let node = t.find_node(b"/acpi");
        set_prop(t, node, b"reg", &reg);
    });

    // Use framebuffer if SPCR is absent or unusable.
    if !s.serial {
        cnset(ttydev(b"fb0"));
    }

    // Raspberry Pi 4 is "special".
    if xhdr.oemid == *b"RPIFDN" && xhdr.oemtableid[..4] == *b"RPI4" {
        DMA_CONSTRAINT[1].store(0x3bff_ffffu64.to_be(), Ordering::Relaxed);
    }

    fdt_finalize();

    fdt
}

const _: () = {
    assert!(core::mem::size_of::<AcpiRsdp>() == 36);
    assert!(core::mem::size_of::<AcpiTableHeader>() == 36);
    assert!(core::mem::size_of::<AcpiGas>() == 12);
    assert!(core::mem::size_of::<AcpiFadt>() == 268);
    assert!(core::mem::size_of::<AcpiGtdt>() == 104);
    assert!(core::mem::size_of::<AcpiMadt>() == 44);
    assert!(core::mem::size_of::<AcpiMadtGicc>() == 80);
    assert!(core::mem::size_of::<AcpiMadtGicd>() == 24);
    assert!(core::mem::size_of::<AcpiMadtGicMsi>() == 24);
    assert!(core::mem::size_of::<AcpiMadtGicr>() == 16);
    assert!(core::mem::size_of::<AcpiMadtGicIts>() == 20);
    assert!(core::mem::size_of::<AcpiSpcr>() == 80);
    assert!(core::mem::size_of::<AcpiDbg2>() == 44);
    assert!(core::mem::size_of::<AcpiDbg2Info>() == 22);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // The ACPI table handlers on synthetic tables like QEMU's `virt`, each run on a copy of
    // the template blob.

    use super::*;
    use crate::dt_blob::DT_BLOB_TEMPLATE;
    use std::vec::Vec;

    /// The bytes of a plain-old-data structure.
    fn bytes<T: Pod>(v: &T) -> Vec<u8> {
        // SAFETY: `T` is packed plain data: its bytes are initialized.
        unsafe {
            core::slice::from_raw_parts((v as *const T).cast::<u8>(), core::mem::size_of::<T>())
        }
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
}
/* </TESTS> */
