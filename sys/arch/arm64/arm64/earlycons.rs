//! Milestone M0 bootstrap console: the PL011 of QEMU's `virt` machine, polled, through a
//! one-entry `TTBR0_EL1` device mapping that this module installs.
//!
//! Not an OpenBSD file. OpenBSD drives this UART with `pluart(4)` (`dev/ic/pluart.c`), finds it
//! in the device tree (`dev/ofw/fdt.c`) and maps it with `pmap_kenter_pa`; those arrive with
//! milestones M2 to M4 and replace this module.
//!
//! Why a mapping of its own: the Limine protocol maps RAM into the higher half but not device
//! memory, and leaves `TTBR0_EL1` (the lower half) to the kernel. So the first GiB of physical
//! space is identity-mapped here as one Device-nGnRnE block, through `MAIR_EL1` attribute 2,
//! which the bootloader guarantees unused. The tables are static, in `.bss`.

use core::arch::asm;
use core::cell::UnsafeCell;
use core::ptr::{self, addr_of, addr_of_mut};

use crate::machine::bootinfo::BootInfo;
use crate::sys::types::Vaddr;

/// PL011 base on QEMU `virt`. TODO(M4): from the device tree.
const PL011_BASE: usize = 0x0900_0000;
/// Data register.
const UARTDR: usize = 0x000;
/// Flag register.
const UARTFR: usize = 0x018;
/// UARTFR: transmit FIFO full.
const FR_TXFF: u32 = 1 << 5;

/// One translation table: 512 descriptors, 4 KiB aligned.
#[repr(C, align(4096))]
struct PageTable([u64; 512]);

/// The two tables of the temporary lower-half map: level 0, then level 1.
struct EarlyTables(UnsafeCell<[PageTable; 2]>);

// SAFETY: written exactly once by `init`, on the boot CPU, before any other code runs; from then
// on only the MMU reads them.
unsafe impl Sync for EarlyTables {}

static TABLES: EarlyTables =
    EarlyTables(UnsafeCell::new([PageTable([0; 512]), PageTable([0; 512])]));

// Descriptor bits (Armv8-A VMSA, 4 KiB granule).
/// Any descriptor: valid.
const DESC_VALID: u64 = 1 << 0;
/// Level 0 to 2: points to a next-level table (clear: a block).
const DESC_TABLE: u64 = 1 << 1;
/// Block: access flag, set so the first access does not fault.
const BLOCK_AF: u64 = 1 << 10;
/// Block: `MAIR_EL1` attribute index 2.
const BLOCK_ATTR_INDEX_2: u64 = 2 << 2;
/// Block: privileged execute-never.
const BLOCK_PXN: u64 = 1 << 53;
/// Block: unprivileged execute-never.
const BLOCK_UXN: u64 = 1 << 54;

/// `MAIR_EL1` attribute 2 field.
const MAIR_ATTR2_MASK: u64 = 0xff << 16;
/// Attribute encoding for Device-nGnRnE memory.
const MAIR_ATTR2_DEVICE_NGNRNE: u64 = 0x00 << 16;

/// `TCR_EL1.T0SZ` field.
const TCR_T0SZ_MASK: u64 = 0x3f;
/// `T0SZ` under 4-level paging (48-bit lower half): the walk starts at level 0.
const T0SZ_4LEVEL: u64 = 16;

/// Installs the device mapping the console needs. The PL011 itself is left as the firmware
/// programmed it.
///
/// # Safety
///
/// Call once, on the boot CPU, in the state the Limine protocol specifies at entry (MMU on,
/// `TTBR0_EL1` unused, `TCR_EL1` as guaranteed for base revision 6), with `boot` describing the
/// loaded image, so the tables' physical addresses can be computed.
pub unsafe fn init(boot: &BootInfo) -> Result<(), &'static str> {
    let tcr: u64;
    // SAFETY: reading a system register has no side effects.
    unsafe {
        asm!("mrs {}, tcr_el1", out(reg) tcr, options(nomem, nostack, preserves_flags));
    }
    if tcr & TCR_T0SZ_MASK != T0SZ_4LEVEL {
        return Err("TTBR0_EL1 walk is not 4-level (TCR_EL1.T0SZ != 16)");
    }

    let tables = TABLES.0.get();
    // SAFETY: `tables` points to the static; `addr_of!` takes addresses without creating
    // references to the cell's contents.
    let (l0_virt, l1_virt) = unsafe {
        (
            Vaddr::new(addr_of!((*tables)[0]) as usize),
            Vaddr::new(addr_of!((*tables)[1]) as usize),
        )
    };
    let l0_phys = boot.kernel_virt_to_phys(l0_virt).as_usize() as u64;
    let l1_phys = boot.kernel_virt_to_phys(l1_virt).as_usize() as u64;

    // Level 1 entry 0: a 1 GiB device block at physical 0 (the PL011 is at 0x0900_0000).
    // Level 0 entry 0: the level 1 table.
    let block = BLOCK_AF | BLOCK_ATTR_INDEX_2 | BLOCK_PXN | BLOCK_UXN | DESC_VALID;
    let table = l1_phys | DESC_TABLE | DESC_VALID;
    // SAFETY: the caller guarantees this runs once before anything else; the stores go to the
    // static tables, volatile so they are complete before the barrier below.
    unsafe {
        ptr::write_volatile(addr_of_mut!((*tables)[1].0[0]), block);
        ptr::write_volatile(addr_of_mut!((*tables)[0].0[0]), table);
    }

    let mair: u64;
    // SAFETY: the table writes are made visible to the walker (dsb), attribute 2 of MAIR_EL1 is
    // set to Device-nGnRnE (unused by the bootloader by protocol guarantee), then TTBR0_EL1 is
    // pointed at the level 0 table and the lower-half TLB entries are invalidated. Nothing used
    // the lower half before, so no live translation changes under running code.
    unsafe {
        asm!("dsb ishst", options(nostack, preserves_flags));
        asm!("mrs {}, mair_el1", out(reg) mair, options(nomem, nostack, preserves_flags));
        let mair = (mair & !MAIR_ATTR2_MASK) | MAIR_ATTR2_DEVICE_NGNRNE;
        asm!("msr mair_el1, {}", in(reg) mair, options(nostack, preserves_flags));
        asm!(
            "msr ttbr0_el1, {}",
            "isb",
            "tlbi vmalle1",
            "dsb ish",
            "isb",
            in(reg) l0_phys,
            options(nostack, preserves_flags)
        );
    }
    Ok(())
}

/// Writes one byte, after waiting for room in the transmit FIFO. Faults if [`init`] did not run.
pub fn putc(c: u8) {
    let fr = (PL011_BASE + UARTFR) as *const u32;
    let dr = (PL011_BASE + UARTDR) as *mut u32;
    // SAFETY: `init` mapped the first GiB of the lower half as device memory, which holds the
    // PL011; these are volatile accesses to its documented registers.
    unsafe {
        while ptr::read_volatile(fr) & FR_TXFF != 0 {
            core::hint::spin_loop();
        }
        ptr::write_volatile(dr, u32::from(c));
    }
}
