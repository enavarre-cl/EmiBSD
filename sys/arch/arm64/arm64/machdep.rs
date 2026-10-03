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
//! bootstrap KVA helpers arrive with M4-b to M6.
//!
//! ## Deviations
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
//! - `cpu_startup` prints the memory sizes and sets up the buffer cache (`bufinit`): the exec
//!   and physio maps (M6, M7), `cpu_init_extents` and `cpu_init_idt` are not there yet.
//! - `initarm` sets `VBAR_EL1` itself (the C's `locore.S` does, before `initarm`) and sets
//!   `tpidr_el1` first thing instead of after the pmap bootstrap, so `curcpu()` and the
//!   exception vectors work for everything that follows; `x18` is not loaded, as it is a
//!   general register here (`arm64/exception.rs`, deviations).
//! - `consinit` runs `pluart_init_cons` only: the other `*_init_cons` are drivers for hardware
//!   QEMU does not have (`deferred-driver`). The console's receive interrupt is
//!   `pluart_fdt_attach`'s since M8 (`pluart* at fdt?`).
//! - `boot`: under feature `qemu`, the wait for a key after "The operating system has halted"
//!   is the emulator exit with the failure status, which `xtask smoke` checks after a panic.
//!   `vfs_shutdown`, `resettodr`, `if_downall`, `uvm_shutdown`, `dumpsys` and
//!   `config_suspend_all` are reported as unported when reached.
//! - The bootargs parsing (`-a -c -d -s`) is `BootInfo::boothowto` in `sys/machine/bootinfo.rs`,
//!   because the Limine command line serves both architectures.

use core::arch::asm;
use core::cell::UnsafeCell;
use core::ptr::{self, addr_of, addr_of_mut};
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use libkern::StaticCell;

use crate::arch::arm64::arm64::cpufunc::cpu_wfi;
use crate::arch::arm64::arm64::cpuswitch::cpu_switchto_asm;
use crate::arch::arm64::arm64::exception::exception_vectors_addr;
use crate::arch::arm64::arm64::fpu::{fpu_drop, fpu_save};
use crate::arch::arm64::arm64::intr::delay;
use crate::arch::arm64::arm64::pmap::{
    PMAP_DIRECT_BASE, PMAP_DIRECT_END, pmap_bootstrap, pmap_growkernel,
};
use crate::arch::arm64::include::armreg::{PSR_DIT, PSR_M_EL0t};
use crate::arch::arm64::include::cpu::{CpuInfo, curcpu, disable_irq_daif, enable_irq_daif};
use crate::arch::arm64::include::frame::Trapframe;
use crate::arch::arm64::include::param::PAGE_SIZE;
use crate::arch::arm64::include::pcb::{PCB_FPU, PCB_SVE};
use crate::arch::arm64::include::pte::ATTR_GP;
use crate::arch::arm64::include::reg::Fpreg;
use crate::arch::arm64::include::vmparam::VM_MIN_KERNEL_ADDRESS;
use crate::conf::vers::VERSION;
use crate::dev::fdt::pluart_fdt::pluart_init_cons;
use crate::dev::ofw::fdt::{
    FdtNode, fdt_find_node, fdt_init, fdt_is_compatible, fdt_node_property,
};
use crate::dev::ofw::openfirm::OF_finddevice;
use crate::kern::init_main::{BOOTHOWTO, PROC0};
use crate::kern::kern_malloc::{kmeminit_nkmempages, nkmempages};
use crate::kern::subr_log::init_static_msgbuf;
use crate::kern::vfs_bio::bufinit;
use crate::kprintf;
use crate::machine::bootinfo::{BootInfo, MemKind};
use crate::machine::db_machdep::db_enter;
use crate::machine::{Cpu, Machine};
use crate::sys::exec::{EXEC_NOBTCFI, ExecPackage, PsStrings};
use crate::sys::param::{NCARGS, roundup};
use crate::sys::proc::Proc;
use crate::sys::reboot::{
    RB_DUMP, RB_HALT, RB_KDB, RB_NOSYNC, RB_POWERDOWN, RB_RESET, RB_TIMEBAD, RB_USERREQ,
};
use crate::sys::systm::PHYSMEM;
use crate::sys::types::{Paddr, Register, Vaddr};
use crate::sys::user::{Uarea, User};
use crate::unported;
use crate::uvm::uvm_extern::{EXEC_MAP, UvmConstraintRange};
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_km::{kernel_map, kernel_map_min, uvm_km_suballoc};
use crate::uvm::uvm_map::VM_MAP_PAGEABLE;
use crate::uvm::uvm_page::{VmPage, uvm_page_physload, uvm_setpagesize};
use crate::uvm::uvm_param::{atop, ptoa, round_page, trunc_page};

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

/// `proc0paddr`: proc0's u-area (its pcb; the boot stack is Limine's).
pub static PROC0_UAREA: Uarea = Uarea::new();

/// `proc0paddr`: proc0's `struct user`, at the bottom of its u-area.
pub fn proc0paddr() -> &'static User {
    &PROC0_UAREA.u
}
/// `proc0tf`: dummy trapframe for proc0.
static PROC0TF: StaticCell<Trapframe> = StaticCell::new(Trapframe::new());

/// `cold`: if set, still working on cold-start.
pub use crate::sys::systm::COLD;
/// `waittime`: set once the file systems have been synced on the way down.
static WAITTIME: AtomicI32 = AtomicI32::new(-1);
/// `cpuresetfn`: the platform's reset hook, registered by its driver.
pub static CPURESETFN: StaticCell<Option<fn()>> = StaticCell::new(None);
/// `powerdownfn`: the platform's power-off hook, registered by its driver.
pub static POWERDOWNFN: StaticCell<Option<fn()>> = StaticCell::new(None);
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
    // with M4 and M5; db_machine_init() and ddb_init() with M4.
    if BOOTHOWTO.load(Ordering::Relaxed) & RB_KDB != 0 {
        db_enter();
    }
    Ok(())
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

    // The physio map (phys_map): with physio.

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

    // amluart, cduart, com_fdt, exuart, imxuart, mvuart, qcuart and simplefb consoles:
    // hardware QEMU virt does not have (deferred drivers).
    pluart_init_cons();
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
    ci.ci_want_resched.set(1);

    // There's a risk we'll be called before the idle threads start
    // SAFETY: `ci_curproc` names a thread on the CPU, hence alive.
    if let Some(p) = unsafe { ci.ci_curproc.get().as_ref() } {
        aston(p);
        // cpu_kick(ci): MULTIPROCESSOR.
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
/// pending, process as soon as possible. Without `MULTIPROCESSOR` it is `setsoftast()`, which
/// posts the AST to the thread on this CPU.
pub fn signotify(_p: &Proc) {
    setsoftast();
}

/// `clear_resched(ci)`.
pub fn clear_resched(ci: &CpuInfo) {
    ci.ci_want_resched.set(0);
}

/// `cpu_unidle(ci)`: with `MULTIPROCESSOR` an IPI; on one CPU the idle loop sees the run
/// queue itself.
pub fn cpu_unidle(_ci: &CpuInfo) {}

/// `cpu_idle_enter`: the idle thread checks the run queues with interrupts masked, so an
/// interrupt between the check and the `wfi` wakes the `wfi`.
pub fn cpu_idle_enter() {
    disable_irq_daif();
}

/// `cpu_idle_cycle`: `(*cpu_idle_cycle_fcn)()` (`cpu_wfi` until a driver installs another),
/// then let the pending interrupt in and mask again for the next check.
pub fn cpu_idle_cycle() {
    cpu_wfi();
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
                let _ = unported!("vfs_shutdown");

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
