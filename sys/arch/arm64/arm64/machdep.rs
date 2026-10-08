/* $OpenBSD: machdep.c,v 1.101 2026/09/06 18:25:22 mglocker Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2014 Patrick Wildt <patrick@blueri.se>
 * Copyright (c) 2021 Mark Kettenis <kettenis@openbsd.org>
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
//! arm64 machine-dependent setup and shutdown: `arch/arm64/arm64/machdep.c`.
//!
//! Upstream: sys/arch/arm64/arm64/machdep.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports what the console and a panic need: the part of `initarm`
//! that brings up the message buffer and the console, `consinit`, `boot`, `cold`, `waittime`,
//! `cpuresetfn` and `powerdownfn`; M3 adds the memory setup and `pmap_bootstrap`; M4 adds
//! `cpu_info_primary`, the per-CPU pointer and vector table setup of `initarm`, `fdt_init`
//! of the bootloader's device tree, `stdout_node`/`stdout_speed`, `fdt_find_cons` and the
//! console's receive interrupt.
//! `cpu_info[]`, the FDT setup, `dumpsys`, `sendsig`/`setregs`, the sysctl tree and the
//! bootstrap KVA helpers arrive with M4-b to M6. M11a adds `cpu_info[]`,
//! `cpu_idle_cycle_fcn`, `need_resched`'s `cpu_kick`, the `MULTIPROCESSOR` `signotify`
//! (`aston` and `cpu_unidle(p->p_cpu)`) and keeps the boot protocol's processors
//! (`BOOT_MP`) for `cpu_start_secondary`; `cpu_unidle` moved to `cpu.rs`, where the C has it.
//!
//! ## Deviations
//! - `cpu_sysctl` (M13, `machine::cpu::cpu_sysctl`) is ported whole. `CPU_LED_BLINK` does not
//!   call `blink_led_timeout` when it turns on: with no `blink_led` registered (the LED
//!   drivers and `blink_led_register` are not ported) the C returns from it at once.
//!   `lid_action` is a static here that nothing reads until `aplsmc` is ported.
//! - Limine has set up EL1, the MMU and the direct map before `initarm` runs, so the C's
//!   page-table and memory-map work is replaced by the boot protocol (`docs/ARCHITECTURE.md`).
//!   What Limine does not map is device memory: `initarm` installs one 1 GiB identity block of
//!   Device-nGnRnE memory in `TTBR0_EL1` (the lower half, which the protocol leaves to the
//!   kernel), through `MAIR_EL1` attribute 2, so `bus_space` can reach the PL011. `pmap` (M3)
//!   replaces it.
//! - The physical memory handed to `uvm` is the boot protocol's usable regions, which already
//!   exclude the kernel, the device tree, the initrd and the bootloader's own data: the
//!   `memreg_add`/`memreg_remove` bookkeeping, the EFI memory map walk, `pmap_avail_fixup`
//!   and `pmap_physload_avail` have nothing left to do, and the direct map `pmap` uses is the
//!   bootloader's (`arm64/pmap.rs`, deviations). The memory is loaded before `pmap_bootstrap`
//!   (after it in the C) because `pmap_bootstrap` steals its tables from `vm_physmem[]`.
//! - The message buffer is a static area (`kern/subr_log.rs`, `init_static_msgbuf`) instead of
//!   reserved physical pages, until M3.
//! - `cpu_startup` prints the memory sizes, makes the exec and physio maps and sets up the
//!   buffer cache (`bufinit`); `cpu_init_extents` and `cpu_init_idt` are not there yet.
//! - `initarm` sets `VBAR_EL1` itself (the C's `locore.S` does, before `initarm`) and sets
//!   `tpidr_el1` first thing instead of after the pmap bootstrap, so `curcpu()` and the
//!   exception vectors work for everything that follows; `x18` is not loaded, as it is a
//!   general register here (`arm64/exception.rs`, deviations).
//! - `consinit` runs `pluart_init_cons` only: the other `*_init_cons` are drivers for hardware
//!   QEMU does not have (`deferred-driver`). The console's receive interrupt is
//!   `pluart_fdt_attach`'s since M8 (`pluart* at fdt?`).
//! - `boot`: under feature `qemu`, the wait for a key after "The operating system has halted"
//!   is the emulator exit with the failure status, which `xtask smoke` checks after a panic.
//!   `resettodr`, `if_downall`, `uvm_shutdown`, `dumpsys` and `config_suspend_all` are
//!   reported as unported when reached (`vfs_shutdown` is real since M14; with no thread on
//!   the CPU, which the C cannot have there, it is skipped).
//! - The UEFI system table and memory map come from the boot protocol (`BootInfo`) instead
//!   of efiboot's `openbsd,uefi-*` properties in `/chosen`; the system table's address is
//!   kept in `SYSTEM_TABLE` (a local in C) for `mainbus` and `efi_attach`, and the memory
//!   map is relocated into a static buffer (`MMAP`) instead of stolen pages.
//! - The bootargs parsing (`-a -c -d -s`) is `BootInfo::boothowto` in `sys/machine/bootinfo.rs`,
//!   because the Limine command line serves both architectures.
//! - boot(8)'s entry (M14, `locore0.rs`): `getbootinfo` is the half of `initarm` that reads
//!   efiboot's `/chosen` and the memory (`collect_kernel_args`, `process_kernel_args`,
//!   `memreg_add`, `memreg_remove`, the EFI memory map walk, `/reserved-memory`, the 64 MB
//!   block), into the `BootInfo` the rest of `initarm` takes from either entry. The device
//!   tree and the EFI memory map are read in place through `locore0.S`'s direct map (efiboot
//!   leaves them in loader data, which is never given to uvm) instead of being mapped at the
//!   first free KVA and copied to stolen pages; the direct map gains the gigabytes of RAM
//!   there (`bootarg_direct_map`, not in the C; memory it cannot reach, past 512 GB, is
//!   listed as reserved). `openbsd,dma-constraint` is read but `dma_constraint` is a
//!   constant (every address): a narrower one is reported by `initarm`. The processors come
//!   from `/cpus` and start by psci(4)'s `psci_cpu_on` (`bootarg_mp_start`).
//! - `cpu_info[]` holds `CiPtr`s; `cpu_idle_cycle_fcn` is a `StaticCell` written at attach
//!   time; `BOOT_MP` (`MULTIPROCESSOR`) has no C counterpart: PSCI or a spin table in the C
//!   (`cpu.rs`, deviations).

use core::arch::asm;
use core::cell::UnsafeCell;
use core::ffi::CStr;
use core::ptr::{self, NonNull, addr_of, addr_of_mut};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

use libkern::StaticCell;

use crate::arch::arm64::arm64::autoconf::BOOTMAC;
use crate::arch::arm64::arm64::bus_space::ARM64_BS_TAG;
use crate::arch::arm64::arm64::cpu;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::cpu::AP_TTBR1;
use crate::arch::arm64::arm64::cpu::cpu_kick;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::cpu::cpu_unidle;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::cpufunc::cpu_dcache_wb_range;
use crate::arch::arm64::arm64::cpufunc::cpu_wfi;
use crate::arch::arm64::arm64::cpuswitch::cpu_switchto_asm;
use crate::arch::arm64::arm64::exception::exception_vectors_addr;
use crate::arch::arm64::arm64::fpu::{fpu_drop, fpu_save};
use crate::arch::arm64::arm64::intr::delay;
use crate::arch::arm64::arm64::locore0::DIRECT_BASE;
use crate::arch::arm64::arm64::pmap::{
    PMAP_DIRECT_BASE, PMAP_DIRECT_END, pmap_bootstrap, pmap_growkernel,
};
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::include::armreg::{MPIDR_AFF, read_specialreg};
use crate::arch::arm64::include::armreg::{PSR_DIT, PSR_M_EL0t};
use crate::arch::arm64::include::bootconfig::Arm64Bootparams;
use crate::arch::arm64::include::cpu::{
    CPU_COMPATIBLE, CPU_ID_AA64ISAR0, CPU_ID_AA64ISAR1, CPU_ID_AA64ISAR2, CPU_ID_AA64MMFR0,
    CPU_ID_AA64MMFR1, CPU_ID_AA64MMFR2, CPU_ID_AA64PFR0, CPU_ID_AA64PFR1, CPU_ID_AA64SMFR0,
    CPU_ID_AA64ZFR0, CPU_LED_BLINK, CPU_LIDACTION, CiPtr, CpuInfo, MAXCPUS, curcpu,
    disable_irq_daif, enable_irq_daif,
};
use crate::arch::arm64::include::frame::Trapframe;
use crate::arch::arm64::include::param::{PAGE_MASK, PAGE_SIZE};
use crate::arch::arm64::include::pcb::{PCB_FPU, PCB_SVE};
use crate::arch::arm64::include::pte::{
    ATTR_AF, ATTR_GP, ATTR_PXN, ATTR_UXN, L1_BLOCK, L1_SHIFT, L2_SIZE, Ln_ENTRIES, PTE_ATTR_WB,
    SH_INNER, attr_idx, attr_sh,
};
use crate::arch::arm64::include::reg::Fpreg;
use crate::arch::arm64::include::vmparam::{VM_MIN_KERNEL_ADDRESS, VM_PHYS_SIZE, VM_PHYSSEG_MAX};
use crate::conf::vers::VERSION;
use crate::dev::efi::efi::{
    EFI_MEMORY_DESCRIPTOR_VERSION, EfiACPIMemoryNVS, EfiACPIReclaimMemory, EfiBootServicesCode,
    EfiBootServicesData, EfiConventionalMemory, EfiLoaderCode, EfiLoaderData, EfiMemoryDescriptor,
    EfiRuntimeServicesCode, EfiRuntimeServicesData, EfiUnusableMemory,
};
use crate::dev::fdt::pluart_fdt::pluart_init_cons;
#[cfg(feature = "multiprocessor")]
use crate::dev::fdt::psci::psci_cpu_on;
#[cfg(feature = "multiprocessor")]
use crate::dev::fdt::pscivar::PSCI_SUCCESS;
use crate::dev::fdt::simplefb::simplefb_init_cons;
#[cfg(feature = "multiprocessor")]
use crate::dev::ofw::fdt::fdt_node_property_int;
use crate::dev::ofw::fdt::{
    FdtNode, FdtReg, fdt_child_node, fdt_find_node, fdt_get_reg, fdt_get_size, fdt_init,
    fdt_is_compatible, fdt_next_node, fdt_node_property,
};
use crate::dev::ofw::openfirm::{OF_finddevice, OF_getprop, OF_getproplen};
use crate::dev::softraid::{SR_BOOTKEY, SR_BOOTUUID};
use crate::dev::softraidvar::{SR_CRYPTO_MAXKEYBYTES, SR_UUID_MAX, SrUuid};
use crate::kern::init_main::{BOOTHOWTO, PROC0};
use crate::kern::kern_malloc::{kmeminit_nkmempages, nkmempages};
use crate::kern::kern_sysctl::{sysctl_bounded_arr, sysctl_int, sysctl_rdquad, sysctl_rdstring};
use crate::kern::subr_log::init_static_msgbuf;
use crate::kern::vfs_bio::bufinit;
use crate::kprintf;
#[cfg(feature = "multiprocessor")]
use crate::machine::bootinfo::BootCpu;
use crate::machine::bootinfo::BootMp;
use crate::machine::bootinfo::{BootInfo, EfiMemmap, MAX_MODULES, MemKind, MemMap, MemRegion};
use crate::machine::db_machdep::{db_enter, db_machine_init};
use crate::machine::{Cpu, Machine};
use crate::netinet::if_ether::ETHER_ADDR_LEN;
use crate::sys::errno::Errno;
use crate::sys::exec::{EXEC_NOBTCFI, ExecPackage, PsStrings};
use crate::sys::param::{NCARGS, roundup};
use crate::sys::proc::Proc;
use crate::sys::reboot::{
    RB_DUMP, RB_HALT, RB_KDB, RB_NOSYNC, RB_POWERDOWN, RB_RESET, RB_TIMEBAD, RB_USERREQ,
};
use crate::sys::sysctl::SysctlBoundedArgs;
use crate::sys::systm::PHYSMEM;
use crate::sys::types::{Paddr, Psize, Register, Vaddr};
use crate::sys::user::{Uarea, User};
use crate::unported;
use crate::uvm::uvm_extern::{EXEC_MAP, PHYS_MAP, UvmConstraintRange};
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_km::{kernel_map, kernel_map_min, uvm_km_suballoc};
use crate::uvm::uvm_map::VM_MAP_PAGEABLE;
use crate::uvm::uvm_page::{VmPage, uvm_page_physload, uvm_setpagesize};
use crate::uvm::uvm_param::{atop, ptoa, round_page, trunc_page};
use libkern::explicit_bzero::explicit_bzero;

#[cfg(feature = "qemu")]
use crate::arch::arm64::arm64::qemu;
#[cfg(not(feature = "qemu"))]
use crate::dev::cons::cngetc;
#[cfg(feature = "qemu")]
use crate::machine::ExitStatus;

/// Size of the bootstrap device map: the first GiB of physical space, identity-mapped as
/// device memory by [`initarm`] (see the module's deviations).
pub const BOOTSTRAP_DEVICE_MAP_SIZE: usize = 1 << 30;

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

/// One translation table: 512 descriptors, 4 KiB aligned.
#[repr(C, align(4096))]
struct PageTable([u64; 512]);

/// The two tables of the bootstrap lower-half map: level 0, then level 1.
struct BootstrapTables(UnsafeCell<[PageTable; 2]>);

// SAFETY: written exactly once by `initarm`, on the boot CPU, before any other code runs; from
// then on only the MMU reads them.
unsafe impl Sync for BootstrapTables {}

/// `dma_constraint`: every address, until the device tree narrows it
/// (`openbsd,dma-constraint`, M4).
pub static DMA_CONSTRAINT: UvmConstraintRange = UvmConstraintRange {
    ucr_low: Paddr::new(0),
    ucr_high: Paddr::new(usize::MAX),
};
/// `uvm_md_constraints[]`: the machine's DMA ranges.
pub static UVM_MD_CONSTRAINTS: [&UvmConstraintRange; 1] = [&DMA_CONSTRAINT];
/// The direct map covers at least this much, by the boot protocol's guarantee.
const DIRECT_MAP_MIN_SIZE: usize = 4 << 30;
/// `cpu_info_primary`: the boot CPU's `cpu_info`; `cpu_attach` (M4-b) fills in what
/// `initarm` does not (`ci_cpuid`, `ci_mpidr`, the flags).
pub static CPU_INFO_PRIMARY: CpuInfo = CpuInfo::new();

/// `cpu_info[MAXCPUS]`: every attached CPU by unit, `cpu_info_primary` first.
pub static CPU_INFO: [CiPtr<CpuInfo>; MAXCPUS as usize] = {
    let mut a = [const { CiPtr::null() }; MAXCPUS as usize];
    a[0] = CiPtr::new(ptr::from_ref(&CPU_INFO_PRIMARY));
    a
};

/// The processors from the boot protocol (`MULTIPROCESSOR`), which `initarm` keeps for
/// `cpu_start_secondary` (`cpu.rs`): written once by `initarm` on the boot CPU.
#[cfg(feature = "multiprocessor")]
pub static BOOT_MP: StaticCell<Option<BootMp>> = StaticCell::new(None);

/// `proc0paddr`: proc0's u-area (its pcb; the boot stack is Limine's).
pub static PROC0_UAREA: Uarea = Uarea::new();

/// `cpu_info_list` (`cpu.c`): the attached CPUs, linked through `ci_next`, headed by the
/// boot CPU.
pub fn cpu_info_list() -> &'static CpuInfo {
    &CPU_INFO_PRIMARY
}

/// `proc0paddr`: proc0's `struct user`, at the bottom of its u-area.
pub fn proc0paddr() -> &'static User {
    &PROC0_UAREA.u
}
/// `proc0tf`: dummy trapframe for proc0.
static PROC0TF: StaticCell<Trapframe> = StaticCell::new(Trapframe::new());

/// `cold`: if set, still working on cold-start.
pub use crate::sys::systm::COLD;
/// `waittime`: set once the file systems have been synced on the way down.
/// `lid_action`: what closing the lid does (`machdep.lidaction`).
pub static LID_ACTION: AtomicI32 = AtomicI32::new(1);
/// `led_blink`: blink the LEDs (`machdep.led_blink`).
pub static LED_BLINK: AtomicI32 = AtomicI32::new(1);
static WAITTIME: AtomicI32 = AtomicI32::new(-1);
/// `cpuresetfn`: the platform's reset hook, registered by its driver.
pub static CPURESETFN: StaticCell<Option<fn()>> = StaticCell::new(None);
/// `powerdownfn`: the platform's power-off hook, registered by its driver.
pub static POWERDOWNFN: StaticCell<Option<fn()>> = StaticCell::new(None);
/// Room for the relocated UEFI memory map (QEMU's has about 60 descriptors of 48 bytes).
const MMAP_MAX: usize = 16 * 1024;
/// `mmap`: the UEFI memory map, relocated by `initarm` (see the module's deviations).
static MMAP: StaticCell<[u8; MMAP_MAX]> = StaticCell::new([0; MMAP_MAX]);
/// `mmap_size`: bytes of `mmap` in use; 0 without UEFI.
pub static MMAP_SIZE: AtomicU32 = AtomicU32::new(0);
/// `mmap_desc_size`: the distance between two descriptors of `mmap`.
pub static MMAP_DESC_SIZE: AtomicU32 = AtomicU32::new(0);
/// `mmap_desc_ver`: the descriptors' version.
pub static MMAP_DESC_VER: AtomicU32 = AtomicU32::new(0);
/// `system_table`: the UEFI system table's physical address, 0 without UEFI (a local of
/// `initarm` in C, read again from `/chosen` by `mainbus` and `efi_attach`).
pub static SYSTEM_TABLE: AtomicU64 = AtomicU64::new(0);

/// `mmap`, `mmap_size` bytes long: the relocated UEFI memory map (empty without UEFI).
pub fn efi_mmap() -> &'static [u8] {
    let size = (MMAP_SIZE.load(Ordering::Acquire) as usize).min(MMAP_MAX);
    // SAFETY: `initarm` writes the buffer once, on the boot CPU, before publishing
    // `MMAP_SIZE`; it is only read afterwards.
    unsafe { &MMAP.get()[..size] }
}

/// The bootstrap device map's tables.
static TABLES: BootstrapTables =
    BootstrapTables(UnsafeCell::new([PageTable([0; 512]), PageTable([0; 512])]));

/// Installs the bootstrap device map (see the module's deviations).
///
/// # Safety
///
/// Call once, on the boot CPU, in the state the Limine protocol specifies at entry (MMU on,
/// `TTBR0_EL1` unused, `TCR_EL1` as guaranteed for base revision 6), with `boot` describing the
/// loaded image, so the tables' physical addresses can be computed.
unsafe fn bootstrap_device_map(boot: &BootInfo) -> Result<(), &'static str> {
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

    // Level 1 entry 0: a 1 GiB device block at physical 0. Level 0 entry 0: the level 1 table.
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

/// `initarm`: the first C of the kernel, called from `locore` with the machine as the
/// bootloader left it. Here: the bootstrap device map, the message buffer, the console, the
/// pmap bootstrap, the physical memory and the `boot -d` hook.
///
/// # Safety
///
/// Call once, on the boot CPU, before anything else runs, with `boot` describing the loaded
/// image.
pub unsafe fn initarm(boot: &BootInfo) -> Result<(), &'static str> {
    // locore.S points VBAR_EL1 at exception_vectors before initarm, and initarm sets
    // tpidr_el1 (and x18, the C's curcpu register) to cpu_info_primary. Here both come first,
    // so a fault anywhere below lands in do_el1h_sync and curcpu() works.
    CPU_INFO_PRIMARY
        .ci_self
        .set(ptr::from_ref(&CPU_INFO_PRIMARY));
    // SAFETY: system register writes that install this kernel's per-CPU pointer and vector
    // table on the boot CPU, before any exception can be taken. The SPSel switch keeps the
    // stack pointer's value, so the compiler's view of the stack is unchanged.
    unsafe {
        // The kernel runs on SP_EL1 (the "EL1h" vectors), as the C does; the boot protocol
        // may have entered with SPSel = 0, whose vectors are empty.
        asm!(
            "mov {tmp}, sp",
            "msr spsel, #1",
            "mov sp, {tmp}",
            "isb",
            tmp = out(reg) _,
            options(nomem, nostack, preserves_flags)
        );
        asm!(
            "msr tpidr_el1, {}",
            in(reg) ptr::from_ref(&CPU_INFO_PRIMARY) as usize,
            options(nomem, nostack, preserves_flags)
        );
        asm!(
            "msr vbar_el1, {}",
            "isb",
            in(reg) exception_vectors_addr(),
            options(nostack, preserves_flags)
        );
    }

    // The processors and the way to start them (MULTIPROCESSOR, cpu_start_secondary).
    // SAFETY: once, on the boot CPU, before any reader (cpu_attach runs much later).
    #[cfg(feature = "multiprocessor")]
    unsafe {
        BOOT_MP.write(boot.mp)
    };

    // The FDT, memory-map and page-table work of the C happens in the boot protocol; the
    // device map below stands in for `pmap_bootstrap_bs_map` (see the module's deviations).
    // SAFETY: forwarded from the caller.
    unsafe { bootstrap_device_map(boot)? };

    // The device tree Limine hands over (the C's `config` from the bootloader).
    if fdt_init(boot.dtb.map_or(ptr::null(), |p| p.as_ptr())) == 0 {
        return Err("fdt_init: no device tree");
    }

    init_static_msgbuf();
    consinit();

    // `openbsd,sr-bootuuid` and `openbsd,sr-bootkey` (efiboot's softraid boot volume and
    // key, copied into `sr_bootuuid`/`sr_bootkey` under NSOFTRAID): boot(8)'s entry copies
    // them (`getbootinfo`); under Limine no loader sets them and `dev/softraid.rs`'s
    // `SR_BOOTUUID`/`SR_BOOTKEY` stay zero.
    // `openbsd,dma-constraint`: `dma_constraint` is a constant here (every address, what
    // efiboot passes on QEMU); a narrower one is reported (see the module's deviations).
    // SAFETY: written by `getbootinfo` before `initarm`, on this CPU.
    if let Some((low, high)) = unsafe { BOOT_DMA_CONSTRAINT.read() }
        && (low != 0 || high != u64::MAX)
    {
        kprintf!(
            "initarm: dma constraint {:#x}-{:#x} not applied\n",
            low,
            high
        );
        let _ = unported!("dma_constraint from openbsd,dma-constraint");
    }
    // The UEFI system table and memory map efiboot puts in /chosen (see the module's
    // deviations).
    if let Some(st) = boot.efi_system_table {
        SYSTEM_TABLE.store(st.as_usize() as u64, Ordering::Relaxed);
    }

    // Relocate the EFI memory map too.
    if let Some(m) = boot.efi_memmap {
        let len = m.map.len();
        if len <= MMAP_MAX {
            // SAFETY: once, on the boot CPU, before anything reads `MMAP` (`efi_mmap`
            // reads at most `MMAP_SIZE` bytes, published below).
            unsafe { MMAP.get_mut()[..len].copy_from_slice(m.map) };
            MMAP_DESC_SIZE.store(m.desc_size, Ordering::Relaxed);
            MMAP_DESC_VER.store(m.desc_ver, Ordering::Relaxed);
            MMAP_SIZE.store(len as u32, Ordering::Release);
        } else {
            kprintf!("initarm: EFI memory map of {} bytes not relocated\n", len);
        }
    }

    // The direct map is the bootloader's (see `arm64/pmap.rs`).
    let regions = boot.memmap.regions();
    let map_end = regions
        .iter()
        .map(|r| r.base.as_usize() + r.length.as_usize())
        .max()
        .unwrap_or(0);
    PMAP_DIRECT_BASE.store(boot.hhdm_offset, Ordering::Relaxed);
    PMAP_DIRECT_END.store(
        boot.hhdm_offset + roundup(map_end, 1 << 30).max(DIRECT_MAP_MIN_SIZE),
        Ordering::Relaxed,
    );

    let usable = || regions.iter().filter(|r| r.kind == MemKind::Usable);

    UVMEXP.pagesize.store(PAGE_SIZE as i32, Ordering::Relaxed);
    uvm_setpagesize();

    // Make all physical memory available to UVM (pmap_physload_avail and the EFI memory map
    // loop of the C); before pmap_bootstrap, which steals from it (see `arm64/pmap.rs`).
    for r in usable() {
        if r.length.as_usize() < PAGE_SIZE {
            kprintf!(" skipped - too small\n");
            continue;
        }
        let start = round_page(r.base.as_usize());
        let end = trunc_page(r.base.as_usize() + r.length.as_usize());
        if end <= start {
            continue;
        }
        uvm_page_physload(atop(start), atop(end), atop(start), atop(end), 0);
        PHYSMEM.fetch_add(atop(end - start), Ordering::Relaxed);
    }

    let ram_start = usable().map(|r| r.base.as_usize()).min().unwrap_or(0);
    let ram_end = usable()
        .map(|r| r.base.as_usize() + r.length.as_usize())
        .max()
        .unwrap_or(0);
    // SAFETY: once, on the boot CPU, with the direct map set above, the memory loaded and the
    // MMU on.
    let _vstart = unsafe { pmap_bootstrap(Paddr::new(ram_start), Paddr::new(ram_end)) };

    // pmap_avail_fixup: nothing to fix up, the map is the bootloader's.

    // Make sure that we have enough KVA to initialize UVM. In particular, we need enough KVA
    // to be able to allocate the vm_page structures and nkmempages for malloc(9).
    kmeminit_nkmempages();
    let nkmempages = nkmempages();
    pmap_growkernel(Vaddr::new(
        VM_MIN_KERNEL_ADDRESS
            + 1024 * 1024 * 1024
            + PHYSMEM.load(Ordering::Relaxed) * size_of::<VmPage>()
            + ptoa(nkmempages),
    ));

    // The rest of initarm (cpu_init, the FDT, the console from the device tree, ...) arrives
    // with M4 and M5.
    db_machine_init();

    // Firmware doesn't load symbols: ddb_init() (db_sym.c, db_elf.c) is not ported.

    if BOOTHOWTO.load(Ordering::Relaxed) & RB_KDB != 0 {
        db_enter();
    }
    Ok(())
}

/// The most usable ranges `getbootinfo` collects (`memreg[VM_PHYSSEG_MAX]` in C).
const NMEMREG: usize = VM_PHYSSEG_MAX;

/// `memreg[]`/`nmemreg`: the usable physical memory boot(8)'s entry gathers, a local of
/// `getbootinfo` here (file statics in C).
struct Memreg {
    reg: [FdtReg; NMEMREG],
    n: usize,
}

/// `bootargs`: the kernel's copy of `/chosen`'s `bootargs` (boot(8)'s entry; Limine's
/// command line is its own).
static BOOTARGS: StaticCell<[u8; 256]> = StaticCell::new([0; 256]);
/// boot(8)'s entry: the kernel image's physical minus virtual address (`kern_delta`), for
/// the physical address of `cpu_hatch_secondary`; 0 under Limine.
pub static KERN_DELTA: AtomicU64 = AtomicU64::new(0);
/// `openbsd,dma-constraint` as boot(8) passed it, `(low, high)`; `initarm` reports a
/// narrower one (see the module's deviations).
static BOOT_DMA_CONSTRAINT: StaticCell<Option<(u64, u64)>> = StaticCell::new(None);

/// The processors `/cpus` lists (boot(8)'s entry, `MULTIPROCESSOR`): their `reg`, the MPIDR
/// affinity.
#[cfg(feature = "multiprocessor")]
static BOOTARG_CPUS: StaticCell<[u64; MAXCPUS as usize]> = StaticCell::new([0; MAXCPUS as usize]);
/// How many of [`BOOTARG_CPUS`] are filled.
#[cfg(feature = "multiprocessor")]
static BOOTARG_NCPUS: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" {
    /// `_start` (`locore0.S`): the first instruction of the kernel image.
    static _start: [u8; 0];
    /// `esym` (`locore.S`): the end of the symbols boot(8) loaded, a virtual address.
    static esym: u64;
    /// `_end` (`conf/kernel.ld`): the end of the image's BSS.
    static _end: [u8; 0];
}

/// boot(8)'s entry (`Cpu::getbootinfo`): the half of `initarm` that reads what efiboot
/// passed, the `arm64_bootparams` `locore0.S` built (`arg`) and the device tree's `/chosen`
/// (`bootargs`, `openbsd,boothowto`, `openbsd,bootduid`, `openbsd,bootmac`, the softraid
/// boot volume and key, the UEFI memory map and system table, `openbsd,dma-constraint`),
/// and the physical memory: the EFI memory map's conventional and boot services memory (the
/// device tree's `/memory` without one), less `/reserved-memory`'s `no-map` regions and
/// the kernel's 64 MB block, whose part after the kernel and its symbols is usable too
/// (`pmap_physload_avail` in C). The direct map `locore0.S` began covers that memory
/// afterwards (see the module's deviations).
///
/// # Safety
///
/// Once, on the boot CPU, from `bootarg_main` with the pointer `locore0.S` passed, before
/// anything else.
pub unsafe fn getbootinfo(arg: usize) -> Result<BootInfo, &'static str> {
    // SAFETY: locore0.S built the parameters on the boot stack, below the caller's frame.
    let abp = unsafe { &*(arg as *const Arm64Bootparams) };
    let kernbase = ptr::addr_of!(_start) as usize & !PAGE_MASK;
    let kvo = abp.kern_delta as usize;
    KERN_DELTA.store(abp.kern_delta, Ordering::Relaxed);
    let kernel_phys = kernbase.wrapping_add(kvo);

    // The bootloader has loaded us into a 64MB block.
    let memstart = kernel_phys & !(L2_SIZE - 1);
    let memend = memstart + 64 * 1024 * 1024;

    // The FDT, through the direct map (the C maps it at the first free KVA).
    let config = abp.arg2 as u64;
    // SAFETY: once, on the boot CPU, before anything reads the direct map's table.
    if config == 0 || !unsafe { bootarg_direct_map(config, config + 1) } {
        return Err("getbootinfo: no FDT");
    }
    let fdt = (DIRECT_BASE + config as usize) as *const u8;
    let size = fdt_get_size(fdt);
    // SAFETY: as above.
    if size == 0 || !unsafe { bootarg_direct_map(config, config + size as u64) } {
        return Err("getbootinfo: no FDT");
    }
    if fdt_init(fdt) == 0 {
        return Err("getbootinfo: corrupt FDT");
    }

    let mut howto = 0;
    let mut duid = None;
    let mut mmap_start = 0u64;
    let mut mmap_size = 0u32;
    let mut mmap_desc_size = 0u32;
    let mut mmap_desc_ver = 0u32;
    let mut system_table = 0u64;
    let node = fdt_find_node(b"/chosen");
    if !node.is_null() {
        let be32 = |p: &[u8]| u32::from_be_bytes([p[0], p[1], p[2], p[3]]);
        let be64 = |p: &[u8]| u64::from_be_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]]);
        let prop = |name: &[u8]| fdt_node_property(node, name).unwrap_or(&[]);

        let p = prop(b"bootargs");
        if !p.is_empty() {
            collect_kernel_args(p);
        }
        let p = prop(b"openbsd,boothowto");
        if p.len() == 4 {
            howto = be32(p) as i32;
        }
        let p = prop(b"openbsd,bootduid");
        if p.len() == 8 {
            let mut d = [0u8; 8];
            d.copy_from_slice(p);
            duid = Some(d);
        }
        let p = prop(b"openbsd,bootmac");
        if p.len() == ETHER_ADDR_LEN {
            let mut lladdr = [0u8; ETHER_ADDR_LEN];
            lladdr.copy_from_slice(p);
            // SAFETY: the boot CPU, before autoconfiguration reads it.
            unsafe { BOOTMAC.write(Some(lladdr)) };
        }
        let p = prop(b"openbsd,sr-bootuuid");
        if p.len() == size_of::<SrUuid>() {
            let mut uuid = SrUuid {
                sui_id: [0; SR_UUID_MAX],
            };
            uuid.sui_id.copy_from_slice(p);
            // SAFETY: the boot CPU, before softraid reads it (NSOFTRAID > 0: softraid0 is
            // configured).
            unsafe { SR_BOOTUUID.write(uuid) };
        }
        // SAFETY: the property lies in the device tree efiboot allocated, mapped
        // read-write by the direct map; nothing holds a reference into it but `p`, which is
        // not used again.
        unsafe { bootarg_bzero(p) };
        let p = prop(b"openbsd,sr-bootkey");
        if p.len() == SR_CRYPTO_MAXKEYBYTES {
            // SAFETY: as for the UUID.
            unsafe { SR_BOOTKEY.get_mut().copy_from_slice(p) };
        }
        // SAFETY: as above.
        unsafe { bootarg_bzero(p) };

        let p = prop(b"openbsd,uefi-mmap-start");
        if p.len() == 8 {
            mmap_start = be64(p);
        }
        let p = prop(b"openbsd,uefi-mmap-size");
        if p.len() == 4 {
            mmap_size = be32(p);
        }
        let p = prop(b"openbsd,uefi-mmap-desc-size");
        if p.len() == 4 {
            mmap_desc_size = be32(p);
        }
        let p = prop(b"openbsd,uefi-mmap-desc-ver");
        if p.len() == 4 {
            mmap_desc_ver = be32(p);
        }
        let p = prop(b"openbsd,uefi-system-table");
        if p.len() == 8 {
            system_table = be64(p);
        }
        let p = prop(b"openbsd,dma-constraint");
        if p.len() == 16 {
            // SAFETY: the boot CPU, before `initarm` reads it.
            unsafe { BOOT_DMA_CONSTRAINT.write(Some((be64(p), be64(&p[8..])))) };
        }
    }

    let cmdline = process_kernel_args();

    // The UEFI memory map, read in place through the direct map (`initarm` relocates it).
    let mut efi_memmap = None;
    let mapped = mmap_start != 0
        && mmap_size != 0
        // SAFETY: as above.
        && unsafe { bootarg_direct_map(mmap_start, mmap_start + u64::from(mmap_size)) };
    if mapped {
        // SAFETY: efiboot's memory map, `mmap_size` bytes in loader data the kernel never
        // hands to uvm, mapped by the direct map for the kernel's lifetime.
        let map = unsafe {
            core::slice::from_raw_parts(
                (DIRECT_BASE + mmap_start as usize) as *const u8,
                mmap_size as usize,
            )
        };
        efi_memmap = Some(EfiMemmap {
            map,
            desc_size: mmap_desc_size,
            desc_ver: mmap_desc_ver,
        });
    }

    // Make all other physical memory available to UVM.
    let mut memreg = Memreg {
        reg: [FdtReg::default(); NMEMREG],
        n: 0,
    };
    let descs = || {
        efi_memmap.iter().flat_map(|m| {
            let n = if m.desc_size == 0 {
                0
            } else {
                m.map.len() / m.desc_size as usize
            };
            (0..n).map(move |i| {
                // SAFETY: descriptor `i` lies inside the map, `desc_size` bytes apart; the
                // read is unaligned on purpose.
                unsafe {
                    ptr::read_unaligned(
                        m.map
                            .as_ptr()
                            .add(i * m.desc_size as usize)
                            .cast::<EfiMemoryDescriptor>(),
                    )
                }
            })
        })
    };
    if efi_memmap.is_some()
        && mmap_desc_ver == EFI_MEMORY_DESCRIPTOR_VERSION
        && mmap_desc_size as usize >= size_of::<EfiMemoryDescriptor>()
    {
        // Load all memory marked as EfiConventionalMemory, EfiBootServicesCode or
        // EfiBootServicesData. The initial 64MB memory block should be marked as
        // EfiLoaderData so it won't be added here.
        for desc in descs() {
            if desc.Type == EfiConventionalMemory
                || desc.Type == EfiBootServicesCode
                || desc.Type == EfiBootServicesData
            {
                memreg_add(
                    &mut memreg,
                    &FdtReg {
                        addr: desc.PhysicalStart,
                        size: ptoa(desc.NumberOfPages as usize) as u64,
                    },
                );
            }
        }
    } else {
        let node = fdt_find_node(b"/memory");
        if node.is_null() {
            return Err("getbootinfo: no memory specified");
        }
        let mut i = 0;
        while memreg.n < NMEMREG {
            let mut reg = FdtReg::default();
            if fdt_get_reg(node, i, &mut reg).is_err() {
                break;
            }
            i += 1;
            if reg.size == 0 {
                continue;
            }
            memreg_add(&mut memreg, &reg);
        }
    }

    // Remove reserved memory.
    let node = fdt_find_node(b"/reserved-memory");
    if !node.is_null() {
        let mut node = fdt_child_node(node);
        while !node.is_null() {
            let mut reg = FdtReg::default();
            if fdt_node_property(node, b"no-map").is_some()
                && fdt_get_reg(node, 0, &mut reg).is_ok()
                && reg.size != 0
            {
                memreg_remove(&mut memreg, &reg);
            }
            node = fdt_next_node(node);
        }
    }

    // Remove the initial 64MB block.
    memreg_remove(
        &mut memreg,
        &FdtReg {
            addr: memstart as u64,
            size: (memend - memstart) as u64,
        },
    );

    // The block: the kernel, its symbols and locore0.S's tables, then free memory.
    // SAFETY: `esym` is locore.S's word, written by locore0.S before any Rust ran.
    let esym_va = unsafe { ptr::addr_of!(esym).read() } as usize;
    let image_end = esym_va.max(ptr::addr_of!(_end) as usize);
    let kernel_end = round_page(image_end)
        .wrapping_add(kvo)
        .clamp(memstart, memend);

    let mut memmap = MemMap::new();
    let mut push = |base: u64, end: u64, kind: MemKind| {
        end <= base
            || memmap.push(MemRegion {
                base: Paddr::new(base as usize),
                length: Psize::new((end - base) as usize),
                kind,
            })
    };
    let mut ok = push(
        memstart as u64,
        kernel_end as u64,
        MemKind::KernelAndModules,
    );
    // SAFETY: as above.
    if unsafe { bootarg_direct_map(kernel_end as u64, memend as u64) } {
        ok &= push(kernel_end as u64, memend as u64, MemKind::Usable);
    }
    for r in &memreg.reg[..memreg.n] {
        // Memory the direct map cannot reach stays out of uvm's way (Reserved).
        // SAFETY: as above.
        let kind = if unsafe { bootarg_direct_map(r.addr, r.addr + r.size) } {
            MemKind::Usable
        } else {
            MemKind::Reserved
        };
        ok &= push(r.addr, r.addr + r.size, kind);
    }
    // What uvm never gets, for the map's sake: the firmware's and efiboot's own memory.
    for desc in descs() {
        let t = desc.Type;
        let kind =
            if t == EfiConventionalMemory || t == EfiBootServicesCode || t == EfiBootServicesData {
                continue;
            } else if t == EfiLoaderCode || t == EfiLoaderData {
                MemKind::BootloaderReclaimable
            } else if t == EfiRuntimeServicesCode || t == EfiRuntimeServicesData {
                MemKind::ReservedMapped
            } else if t == EfiACPIReclaimMemory {
                MemKind::AcpiReclaimable
            } else if t == EfiACPIMemoryNVS {
                MemKind::AcpiNvs
            } else if t == EfiUnusableMemory {
                MemKind::BadMemory
            } else {
                MemKind::Reserved
            };
        let base = desc.PhysicalStart;
        let end = base + ptoa(desc.NumberOfPages as usize) as u64;
        // The kernel's block is listed above.
        if base < memend as u64 && end > memstart as u64 {
            continue;
        }
        ok &= push(base, end, kind);
    }
    if !ok {
        return Err("getbootinfo: too many memory regions");
    }

    Ok(BootInfo {
        bootloader_name: c"boot(8)",
        bootloader_version: c"efiboot",
        cmdline,
        hhdm_offset: DIRECT_BASE,
        kernel_phys: Paddr::new(kernel_phys),
        kernel_virt: Vaddr::new(kernbase),
        rsdp: None,
        dtb: NonNull::new(fdt.cast_mut()),
        memmap,
        efi_system_table: (system_table != 0).then(|| Paddr::new(system_table as usize)),
        efi_memmap,
        modules: [None; MAX_MODULES],
        mp: bootarg_mp(),
        howto,
        duid,
        // efiboot's efi_framebuffer() already put the GOP frame buffer in the tree as
        // /chosen/framebuffer, where simplefb finds it, as in C; the Limine entry's copy of
        // that node (stand/fdtfb.rs) is not needed here.
        framebuffer: None,
    })
}

/// Not in the C: maps the gigabytes of `[start, end)` in `locore0.S`'s direct map (1 GB
/// blocks of write-back memory, as `locore0.S` maps the kernel's and the device tree's) where
/// they are not mapped yet. False when part of the range is beyond the 512 GB its one table
/// covers.
///
/// # Safety
///
/// The boot CPU alone, during boot(8)'s entry: nothing else writes the table.
unsafe fn bootarg_direct_map(start: u64, end: u64) -> bool {
    let table: usize;
    // SAFETY: computes the address of locore.S's `pagetable_l1_direct`, no memory access.
    unsafe {
        asm!(
            "adrp {t}, pagetable_l1_direct",
            "add {t}, {t}, :lo12:pagetable_l1_direct",
            t = out(reg) table,
            options(nomem, nostack, preserves_flags)
        );
    }
    let table = ptr::with_exposed_provenance_mut::<u64>(table);
    if end <= start {
        return true;
    }
    let first = (start >> L1_SHIFT) as usize;
    let last = ((end - 1) >> L1_SHIFT) as usize;
    if last >= Ln_ENTRIES {
        return false;
    }
    for idx in first..=last {
        // SAFETY: `idx < Ln_ENTRIES`, inside the page-sized table the kernel image holds and
        // maps; the caller guarantees nothing else writes it.
        unsafe {
            let e = table.add(idx);
            if ptr::read_volatile(e) & DESC_VALID == 0 {
                let desc = ((idx as u64) << L1_SHIFT)
                    | L1_BLOCK
                    | ATTR_AF
                    | attr_sh(SH_INNER)
                    | ATTR_UXN
                    | ATTR_PXN
                    | attr_idx(PTE_ATTR_WB);
                ptr::write_volatile(e, desc);
            }
        }
    }
    // SAFETY: barriers: the new entries are visible to the table walker before the next
    // access through them (they replace invalid entries, so no TLB entry is stale).
    unsafe { asm!("dsb ishst", "isb", options(nostack, preserves_flags)) };
    true
}

/// `explicit_bzero(prop, len)` on a property of the device tree.
///
/// # Safety
///
/// `p` is a property of the boot(8) device tree, writable through the direct map, and no
/// other reference to its bytes is used afterwards.
unsafe fn bootarg_bzero(p: &[u8]) {
    if p.is_empty() {
        return;
    }
    // SAFETY: the caller's guarantee; the bytes are plain memory.
    explicit_bzero(unsafe { core::slice::from_raw_parts_mut(p.as_ptr().cast_mut(), p.len()) });
}

/// The processors and the way to start them under boot(8): `/cpus`'s `cpu` nodes and
/// `/psci` (`MULTIPROCESSOR`; `None` without PSCI).
fn bootarg_mp() -> Option<BootMp> {
    #[cfg(feature = "multiprocessor")]
    {
        let psci = fdt_find_node(b"/psci");
        if psci.is_null() {
            return None;
        }
        // psci(4) (`dev/fdt/psci.rs`) attaches before the processors (`mainbus_attach_psci`)
        // and makes the CPU_ON call; without a `/psci` node there is no way to start them.

        let cpus = fdt_find_node(b"/cpus");
        if cpus.is_null() {
            return None;
        }
        let acells = fdt_node_property_int(cpus, b"#address-cells").unwrap_or(2) as usize;
        // SAFETY: the boot CPU, before any reader (`BootMp` is handed out below).
        let list = unsafe { BOOTARG_CPUS.get_mut() };
        let mut n = 0;
        let mut node = fdt_child_node(cpus);
        while !node.is_null() && n < list.len() {
            let is_cpu = fdt_node_property(node, b"device_type")
                .is_some_and(|t| t.starts_with(b"cpu\0") || t == b"cpu");
            if let Some(reg) = fdt_node_property(node, b"reg")
                && is_cpu
                && (acells == 1 || acells == 2)
                && reg.len() >= 4 * acells
            {
                let mut hwid = 0u64;
                for c in reg[..4 * acells].chunks(4) {
                    hwid = (hwid << 32) | u64::from(u32::from_be_bytes([c[0], c[1], c[2], c[3]]));
                }
                list[n] = hwid;
                n += 1;
            }
            node = fdt_next_node(node);
        }
        if n == 0 {
            return None;
        }
        BOOTARG_NCPUS.store(n as u32, Ordering::Relaxed);
        Some(BootMp {
            bsp_hwid: read_specialreg!("mpidr_el1") & MPIDR_AFF,
            ncpus: n,
            cpu: bootarg_mp_cpu,
            start: bootarg_mp_start,
        })
    }
    #[cfg(not(feature = "multiprocessor"))]
    None
}

/// [`BootMp::cpu`] under boot(8): processor `i` of `/cpus`.
#[cfg(feature = "multiprocessor")]
fn bootarg_mp_cpu(i: usize) -> BootCpu {
    let n = BOOTARG_NCPUS.load(Ordering::Relaxed) as usize;
    // SAFETY: written once by `bootarg_mp` before the `BootMp` existed; read-only since.
    let list = unsafe { BOOTARG_CPUS.read() };
    BootCpu {
        processor_id: i as u32,
        hwid: if i < n { list[i] } else { u64::MAX },
    }
}

/// [`BootMp::start`] under boot(8): PSCI `CPU_ON` of processor `i` at `locore.S`'s
/// `cpu_hatch_secondary` (its physical address), with `arg` (its `cpu_info`) as the
/// context (`cpu_start_secondary`'s `psci_cpu_on` in C).
///
/// # Safety
///
/// As [`BootMp::start`] states.
#[cfg(feature = "multiprocessor")]
unsafe fn bootarg_mp_start(i: usize, arg: usize) {
    unsafe extern "C" {
        /// `cpu_hatch_secondary` (`locore.S`).
        fn cpu_hatch_secondary();
    }
    let mpidr = bootarg_mp_cpu(i).hwid;
    // The processor reads the kernel's TTBR1_EL1 with its MMU and caches off.
    cpu_dcache_wb_range(ptr::addr_of!(AP_TTBR1) as usize, size_of::<u64>());
    let entry = (cpu_hatch_secondary as *const () as usize as u64)
        .wrapping_add(KERN_DELTA.load(Ordering::Relaxed));
    // `cpu_start_secondary`'s call: psci(4)'s `psci_cpu_on`, through the conduit its attach
    // read from `/psci`.
    let ret = psci_cpu_on(mpidr, entry, arg as u64);
    if ret != PSCI_SUCCESS {
        kprintf!(" psci: CPU_ON {:#x} failed: {}", mpidr, ret);
    }
}

/// `collect_kernel_args`: make a local copy of the bootargs.
fn collect_kernel_args(args: &[u8]) {
    // SAFETY: the boot CPU, during boot(8)'s entry, before anything reads `BOOTARGS`.
    let buf = unsafe { BOOTARGS.get_mut() };
    let len = args.iter().position(|&c| c == 0).unwrap_or(args.len());
    let n = len.min(buf.len() - 1);
    buf[..n].copy_from_slice(&args[..n]);
    buf[n] = 0;
}

/// `process_kernel_args`: skips the kernel image's name in `bootargs` and returns the rest,
/// `boot_args` (the flags themselves are `BootInfo::boothowto`'s; the C parses them here,
/// and `start_kernel` prints the `bootargs:` line).
fn process_kernel_args() -> &'static CStr {
    // SAFETY: written by `collect_kernel_args` on this CPU, read-only from here on.
    let buf: &'static [u8; 256] = unsafe { BOOTARGS.get() };
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len() - 1);
    // Skip the kernel image filename.
    let mut cp = buf[..len].iter().position(|&c| c == b' ').unwrap_or(len);
    while cp < len && buf[cp] == b' ' {
        cp += 1;
    }
    CStr::from_bytes_until_nul(&buf[cp..]).unwrap_or(c"")
}

/// `memreg_add`: adds a range, merged with an adjacent one when it can be.
fn memreg_add(m: &mut Memreg, reg: &FdtReg) {
    for r in &mut m.reg[..m.n] {
        if reg.addr == r.addr + r.size {
            r.size += reg.size;
            return;
        }
        if reg.addr + reg.size == r.addr {
            r.addr = reg.addr;
            r.size += reg.size;
            return;
        }
    }
    if m.n >= NMEMREG {
        return;
    }
    m.reg[m.n] = *reg;
    m.n += 1;
}

/// `memreg_remove`: removes a range, splitting the one it falls inside.
fn memreg_remove(m: &mut Memreg, reg: &FdtReg) {
    let start = reg.addr;
    let end = reg.addr + reg.size;

    let mut i = 0;
    while i < m.n {
        let mut memstart = m.reg[i].addr;
        let mut memend = m.reg[i].addr + m.reg[i].size;

        if end <= memstart || start >= memend {
            i += 1;
            continue;
        }

        if start <= memstart {
            memstart = end.min(memend);
        }
        if end >= memend {
            memend = start.max(memstart);
        }

        if start > memstart && end < memend {
            if m.n < NMEMREG {
                m.reg[m.n] = FdtReg {
                    addr: end,
                    size: memend - end,
                };
                m.n += 1;
            }
            memend = start;
        }
        m.reg[i].addr = memstart;
        m.reg[i].size = memend - memstart;
        i += 1;
    }

    // Remove empty slots.
    let mut i = m.n;
    while i > 0 {
        i -= 1;
        if m.reg[i].size == 0 {
            m.reg.copy_within(i + 1..m.n, i);
            m.n -= 1;
        }
    }
}

/// `setregs`: clear registers on exec: `p` returns to EL0 at the entry point with the stack
/// at `stack`.
pub fn setregs(p: &Proc, pack: &ExecPackage<'_>, stack: Vaddr, _arginfo: &PsStrings) {
    let pm = p.vmspace().vm_map.pmap();
    let pcb = p.pcb();
    let tf = pcb.pcb_tf.get();

    pm.pm_guarded.set(if pack.ep_flags & EXEC_NOBTCFI != 0 {
        0
    } else {
        ATTR_GP
    });

    // pm_apiakey/apdakey/apibkey/apdbkey/apgakey and pmap_setpauthkeys: pointer
    // authentication (M7; QEMU's default virt CPU has none).

    // If we were using the FPU, forget about it.
    // SAFETY: the thread's own pcb, with no reference to the FP state alive.
    unsafe { ptr::write_bytes(pcb.pcb_fpstate.get().cast::<u8>(), 0, size_of::<Fpreg>()) };
    pcb.pcb_flags
        .set(pcb.pcb_flags.get() & !(PCB_FPU | PCB_SVE));
    fpu_drop();

    // SAFETY: `pcb_tf` is the thread's trap frame at the top of its u-area (`cpu_fork`),
    // which only this thread writes, with no reference to it alive here.
    unsafe {
        tf.write(Trapframe::default());
        (*tf).tf_sp = stack.as_usize() as Register;
        (*tf).tf_lr = pack.ep_entry as Register;
        (*tf).tf_elr = pack.ep_entry as Register; // ???
        (*tf).tf_spsr = (PSR_M_EL0t | PSR_DIT) as Register;
    }
}

/// `cpu_startup`: machine-dependent startup code (see the module's deviations).
pub fn cpu_startup() {
    PROC0.p_addr.set(proc0paddr());

    // The message buffer mapping and initmsgbuf: the message buffer is static (M2).

    // Identify ourselves for the msgbuf (everything printed earlier will not be buffered).
    kprintf!("{}", VERSION);

    let physmem = PHYSMEM.load(Ordering::Relaxed);
    kprintf!(
        "real mem  = {} ({}MB)\n",
        ptoa(physmem),
        ptoa(physmem) / 1024 / 1024
    );

    // Allocate a submap for exec arguments. This map effectively limits the number of
    // processes exec'ing at any time.
    let mut minaddr = kernel_map_min().as_usize();
    let mut maxaddr = 0;
    let exec_map = uvm_km_suballoc(
        kernel_map(),
        &mut minaddr,
        &mut maxaddr,
        16 * NCARGS,
        VM_MAP_PAGEABLE,
        false,
        None,
    );
    EXEC_MAP.store(ptr::from_ref(exec_map).cast_mut(), Ordering::Release);

    // Allocate a submap for physio
    let phys_map = uvm_km_suballoc(
        kernel_map(),
        &mut minaddr,
        &mut maxaddr,
        VM_PHYS_SIZE,
        0,
        false,
        None,
    );
    PHYS_MAP.store(ptr::from_ref(phys_map).cast_mut(), Ordering::Release);

    // Set up buffers, so they can be used to read disk labels.
    bufinit();

    let free = UVMEXP.free.load(Ordering::Relaxed).max(0) as usize;
    kprintf!(
        "avail mem = {} ({}MB)\n",
        ptoa(free),
        ptoa(free) / 1024 / 1024
    );

    let curpcb = &proc0paddr().u_pcb;
    CPU_INFO_PRIMARY.ci_curpcb.set(curpcb);
    curpcb.pcb_flags.set(0);
    curpcb.pcb_tf.set(PROC0TF.as_ptr());

    // sched_blockcpu = CPUTYP_L: __HAVE_CPU_TOPOLOGY (M5-b2). boothowto & RB_CONFIG,
    // HIBERNATE: not configured.
}

/// `consinit`: attaches the console, once.
pub fn consinit() {
    static CONSINIT_CALLED: AtomicBool = AtomicBool::new(false);

    if CONSINIT_CALLED.swap(true, Ordering::Relaxed) {
        return;
    }

    // amluart, cduart, com_fdt, exuart, imxuart, mvuart and qcuart consoles: hardware QEMU
    // virt does not have (deferred drivers).
    pluart_init_cons();
    // The frame buffer is the console only when /chosen's stdout-path names it; QEMU's
    // names the PL011, so this returns at once and the display attaches as a plain one.
    simplefb_init_cons(&ARM64_BS_TAG);
}

/// `stdout_node`: the console's device tree node.
pub static STDOUT_NODE: AtomicI32 = AtomicI32::new(0);
/// `stdout_speed`: the speed `stdout-path` asked for, if any.
pub static STDOUT_SPEED: AtomicI32 = AtomicI32::new(0);

/// `fdt_find_cons`: the node of the console `/chosen`'s `stdout-path` (or the `serial0`
/// alias) names, if it is compatible with `name`.
pub fn fdt_find_cons(name: &[u8]) -> FdtNode {
    let mut alias: &[u8] = b"serial0";
    let mut buf = [0u8; 128];
    let mut stdout: Option<&[u8]> = None;

    // First check if "stdout-path" is set.
    let node = fdt_find_node(b"/chosen");
    if !node.is_null()
        && let Some(prop) = fdt_node_property(node, b"stdout-path")
        && !prop.is_empty()
    {
        let mut path = &prop[..prop.iter().position(|&c| c == 0).unwrap_or(prop.len())];
        if let Some(colon) = path.iter().position(|&c| c == b':') {
            let n = colon.min(buf.len() - 1);
            buf[..n].copy_from_slice(&path[..n]);
            let speed = &path[colon + 1..];
            STDOUT_SPEED.store(atoi(speed), Ordering::Relaxed);
            path = &buf[..n];
        }
        if path.first() != Some(&b'/') {
            // It's an alias.
            alias = path;
        } else {
            stdout = Some(path);
        }
    }

    // Perform alias lookup if necessary.
    let alias_buf;
    if stdout.is_none() {
        let node = fdt_find_node(b"/aliases");
        if !node.is_null()
            && let Some(prop) = fdt_node_property(node, alias)
        {
            alias_buf = prop;
            stdout = Some(
                &alias_buf[..alias_buf
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(alias_buf.len())],
            );
        }
    }

    // Lookup the physical address of the interface.
    if let Some(stdout) = stdout {
        let node = fdt_find_node(stdout);
        if !node.is_null() && fdt_is_compatible(node, name) {
            STDOUT_NODE.store(OF_finddevice(stdout), Ordering::Relaxed);
            return node;
        }
    }
    ptr::null()
}

/// `atoi` of the speed after the colon of `stdout-path`.
fn atoi(s: &[u8]) -> i32 {
    let mut n: i32 = 0;
    for &c in s {
        if !c.is_ascii_digit() {
            break;
        }
        n = n.wrapping_mul(10).wrapping_add(i32::from(c - b'0'));
    }
    n
}

/// `need_resched`: asks `ci` to reschedule.
pub fn need_resched(ci: &CpuInfo) {
    ci.ci_want_resched.store(1, Ordering::Relaxed);

    // There's a risk we'll be called before the idle threads start
    // SAFETY: `ci_curproc` names a thread on the CPU, hence alive (a thread is freed only
    // after it switched away for good).
    if let Some(p) = unsafe { ci.ci_curproc.get().as_ref() } {
        aston(p);
        cpu_kick(ci);
    }
}

/// `aston(p)`: `p->p_md.md_astpending = 1`.
pub fn aston(p: &Proc) {
    p.p_md.md_astpending.store(1, Ordering::Relaxed);
}

/// `setsoftast()`: `aston(curcpu()->ci_curproc)`.
pub fn setsoftast() {
    // SAFETY: `ci_curproc` names the thread on this CPU, hence alive.
    if let Some(p) = unsafe { curcpu().ci_curproc.get().as_ref() } {
        aston(p);
    }
}

/// `signotify(p)` (`<machine/cpu.h>`): notify the current process (p) that it has a signal
/// pending, process as soon as possible. With `MULTIPROCESSOR`, `aston(p)` and
/// `cpu_unidle(p->p_cpu)`; without, `setsoftast()`, which posts the AST to the thread on this
/// CPU.
pub fn signotify(p: &Proc) {
    #[cfg(feature = "multiprocessor")]
    {
        aston(p);
        // SAFETY: `p_cpu` names a CPU's `cpu_info`, which lives forever.
        if let Some(ci) = unsafe { p.p_cpu.get().as_ref() } {
            cpu_unidle(ci);
        }
    }
    #[cfg(not(feature = "multiprocessor"))]
    {
        let _ = p;
        setsoftast();
    }
}

/// `clear_resched(ci)`.
pub fn clear_resched(ci: &CpuInfo) {
    ci.ci_want_resched.store(0, Ordering::Relaxed);
}

/// `cpu_idle_enter`: the idle thread checks the run queues with interrupts masked, so an
/// interrupt between the check and the `wfi` wakes the `wfi`.
pub fn cpu_idle_enter() {
    disable_irq_daif();
}

/// `cpu_idle_cycle_fcn`: how the idle loop waits (`cpu_wfi`, or `cpu_psci_idle_cycle` when
/// the device tree has a PSCI idle state); set at attach time before the idle loops run.
pub static CPU_IDLE_CYCLE_FCN: StaticCell<fn()> = StaticCell::new(cpu_wfi);

/// `cpu_idle_cycle`: `(*cpu_idle_cycle_fcn)()`, then let the pending interrupt in and mask
/// again for the next check.
pub fn cpu_idle_cycle() {
    // SAFETY: written only at attach time on the boot CPU, before any idle loop runs.
    let f = unsafe { CPU_IDLE_CYCLE_FCN.read() };
    f();
    // SAFETY: the idle thread runs at IPL_NONE with nothing held: interrupts may come in.
    unsafe { enable_irq_daif() };
    disable_irq_daif();
}

/// `cpu_idle_leave`.
pub fn cpu_idle_leave() {
    // SAFETY: as for `cpu_idle_cycle`.
    unsafe { enable_irq_daif() };
}

/// `cpu_switchto(old, new)`: drops `old`'s FPU state (saving it first if it was in use) and
/// switches (`cpuswitch.S`).
///
/// # Safety
///
/// As `machine::cpu::Cpu::cpu_switchto`: the scheduler lock is held, `new` is runnable and
/// off every queue, `old` (when given) is the running thread.
pub unsafe fn cpu_switchto(old: Option<&Proc>, new: &Proc) {
    if let Some(old) = old {
        let pcb = old.pcb();

        if pcb.pcb_flags.get() & PCB_FPU != 0 {
            fpu_save(old);
        }

        fpu_drop();
    }

    // SAFETY: forwarded from the caller; the assembly only touches the pcbs, the stacks and
    // the per-CPU pointers.
    unsafe {
        cpu_switchto_asm(
            old.map_or(ptr::null(), |p| ptr::from_ref(p).cast()),
            ptr::from_ref(new).cast(),
        )
    };
}

/// `boot(9)`: halts or reboots according to `howto`.
pub fn boot(howto: i32) -> ! {
    let mut howto = howto;

    if howto & RB_RESET == 0 {
        if COLD.load(Ordering::Relaxed) {
            if howto & RB_USERREQ == 0 {
                howto |= RB_HALT;
            }
        } else {
            BOOTHOWTO.store(howto, Ordering::Relaxed);
            if howto & RB_NOSYNC == 0 && WAITTIME.load(Ordering::Relaxed) < 0 {
                WAITTIME.store(0, Ordering::Relaxed);
                if let Some(p) = crate::machine::cpu::curproc() {
                    crate::kern::vfs_subr::vfs_shutdown(p);
                }

                if howto & RB_TIMEBAD == 0 {
                    let _ = unported!("resettodr");
                } else {
                    kprintf!("WARNING: not updating battery clock\n");
                }
            }
            let _ = unported!("if_downall");

            let _ = unported!("uvm_shutdown");
            // splhigh(): M4.
            COLD.store(true, Ordering::Relaxed);

            if howto & RB_DUMP != 0 {
                let _ = unported!("dumpsys");
            }
        }

        // haltsys:
        let _ = unported!("config_suspend_all (DVACT_POWERDOWN)");

        if howto & RB_HALT != 0 {
            if howto & RB_POWERDOWN != 0 {
                kprintf!("\nAttempting to power down...\n");
                delay(500_000);
                // SAFETY: registered by a driver on the boot CPU during autoconfiguration; only
                // read here.
                if let Some(powerdown) = unsafe { POWERDOWNFN.read() } {
                    powerdown();
                }
            }

            kprintf!("\n");
            kprintf!("The operating system has halted.\n");
            kprintf!("Please press any key to reboot.\n\n");
            #[cfg(feature = "qemu")]
            {
                qemu::exit(ExitStatus::Failure)
            }
            #[cfg(not(feature = "qemu"))]
            {
                cngetc();
            }
        }
    }

    // doreset:
    kprintf!("rebooting...\n");
    delay(500_000);
    // SAFETY: as for `POWERDOWNFN`.
    if let Some(cpureset) = unsafe { CPURESETFN.read() } {
        cpureset();
    }
    kprintf!("reboot failed; spinning\n");
    Machine::halt()
}

/// `cpuctl_vars[]`: the `machdep` integers `sysctl_bounded_arr` serves.
static CPUCTL_VARS: [SysctlBoundedArgs; 1] =
    [SysctlBoundedArgs::new(CPU_LIDACTION, &LID_ACTION, 0, 2)];

/// `cpu_sysctl`: machine dependent system variables.
pub fn cpu_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    // all sysctl names at this level are terminal
    let [first] = *name else {
        return Err(Errno::ENOTDIR); // overloaded
    };

    let quad = |oldlenp: &mut usize, v: &AtomicU64| {
        sysctl_rdquad(oldp, oldlenp, newp, v.load(Ordering::Relaxed) as i64)
    };
    match first {
        CPU_COMPATIBLE => {
            let node = OF_finddevice(b"/");
            let len = OF_getproplen(node, b"compatible");
            if len <= 0 {
                return Err(Errno::EOPNOTSUPP);
            }
            let mut compatible = alloc::vec![0u8; len as usize];
            OF_getprop(node, b"compatible", &mut compatible);
            compatible[len as usize - 1] = 0;
            sysctl_rdstring(oldp, oldlenp, newp, &compatible)
        }
        CPU_ID_AA64ISAR0 => quad(oldlenp, &cpu::CPU_ID_AA64ISAR0),
        CPU_ID_AA64ISAR1 => quad(oldlenp, &cpu::CPU_ID_AA64ISAR1),
        CPU_ID_AA64ISAR2 => quad(oldlenp, &cpu::CPU_ID_AA64ISAR2),
        CPU_ID_AA64PFR0 => quad(oldlenp, &cpu::CPU_ID_AA64PFR0),
        CPU_ID_AA64PFR1 => quad(oldlenp, &cpu::CPU_ID_AA64PFR1),
        CPU_ID_AA64MMFR0 => quad(oldlenp, &cpu::CPU_ID_AA64MMFR0),
        CPU_ID_AA64MMFR1 => quad(oldlenp, &cpu::CPU_ID_AA64MMFR1),
        CPU_ID_AA64MMFR2 => quad(oldlenp, &cpu::CPU_ID_AA64MMFR2),
        CPU_ID_AA64SMFR0 => sysctl_rdquad(oldp, oldlenp, newp, 0),
        CPU_ID_AA64ZFR0 => quad(oldlenp, &cpu::CPU_ID_AA64ZFR0),
        CPU_LED_BLINK => {
            let error = sysctl_int(oldp, oldlenp, newp, newlen, &LED_BLINK);
            // If we were false and are now true, the C starts the timer
            // (blink_led_timeout); with no blink_led registered (blink_led_register and
            // the LED drivers are not ported) that function returns at once.
            error
        }
        _ => sysctl_bounded_arr(&CPUCTL_VARS, name, oldp, oldlenp, newp, newlen),
    }
}
/* </CODE> */
