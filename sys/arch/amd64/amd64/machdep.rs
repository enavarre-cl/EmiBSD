/*	$OpenBSD: machdep.c,v 1.314 2026/09/28 14:14:03 deraadt Exp $	*/
/*	$NetBSD: machdep.c,v 1.3 2003/05/07 22:58:18 fvdl Exp $	*/

/*-
 * Copyright (c) 1996, 1997, 1998, 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum and by Jason R. Thorpe of the Numerical Aerospace
 * Simulation Facility, NASA Ames Research Center.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

/*-
 * Copyright (c) 1982, 1987, 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)machdep.c	7.4 (Berkeley) 6/3/91
 */

//! amd64 machine-dependent setup and shutdown: `arch/amd64/amd64/machdep.c`.
//!
//! Upstream: sys/arch/amd64/amd64/machdep.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports what the console and a panic need: the first part of
//! `init_x86_64` (message buffer, console, `boot -d`), `boot`, `delay` and the globals they
//! use (`cpureset_delay`, `lid_action`, `waittime`); M3 adds the direct map, `pmap_bootstrap`
//! and the memory clusters; M4 adds the descriptor tables (`setgate`, `unsetgate`,
//! `setregion`, `set_mem_segment`, `set_sys_segment`), the IDT (`idt`, `idt_allocmap`,
//! `cpu_init_idt`, `idt_vec_alloc`, `idt_vec_alloc_range`, `idt_vec_set`, `idt_vec_free`),
//! `x86_64_proc0_tss_ldt_init` and the GDT/TSS/IDT part of `init_x86_64`. `cpu_reset`,
//! `dumpsys`, the bootinfo parsing, `sendsig`/`setregs` and the sysctl tree arrive with M4-b
//! to M6.
//!
//! ## Deviations
//! - Limine has set up long mode, paging and the direct map before `init_x86_64` runs, so the
//!   BIOS/EFI memory-map walk and the page-table work of the C version are replaced by the
//!   boot protocol (`docs/ARCHITECTURE.md`, "Boot flow"): `pmap_direct_base` is the
//!   bootloader's higher-half direct map, and the memory clusters loaded into `uvm` are the
//!   protocol's usable regions, which already exclude the kernel, the firmware and the
//!   bootloader's own data. The ISA hole and the `avail_end` bookkeeping have nothing to do.
//! - `cpu_startup` prints the memory sizes and fills the boot CPU's TSS (`cpu_enter_pages`):
//!   `version` (generated `vers.c`), `startclocks`, `rtcinit` (M5), the exec and physio maps
//!   and `bufinit` (M6, M7), `cpu_init_extents` and `cpu_boot_mode` (M4-b) are not there yet.
//! - The IDT is a static page (`IDT`) instead of the early page `locore0.S` reserves and the
//!   page `init_x86_64` maps at `idt_vaddr`; `idt_allocmap` is an array of atomics.
//!   `cpu_init_msrs` and the `cpu_info_full_primary` initialiser are the first lines of
//!   `init_x86_64` because there is no `locore0.S` to run them earlier.
//!   `x86_64_proc0_tss_ldt_init` loads the task register only: proc0's pcb is M5. The IST
//!   stacks are filled by `cpu_enter_pages` from `cpu_startup`, as in C, so an NMI or double
//!   fault before then has no stack, as in C.
//! - The message buffer is a static area (`kern/subr_log.rs`, `init_static_msgbuf`) instead of
//!   reserved physical pages, until M3.
//! - `cninit()` is replaced by `consinit()` (`consinit.rs`): no `constab[]` yet.
//! - `delay` is `i8254_delay` directly; `delay_func`, `delay_init` and `delay_fini` (the TSC
//!   upgrade) arrive with M5.
//! - `boot`: under feature `qemu`, the wait for a key after "The operating system has halted"
//!   is the emulator exit with the failure status, which `xtask smoke` checks after a panic.
//!   `vfs_shutdown`, `resettodr`, `if_downall`, `uvm_shutdown`, `dumpsys`,
//!   `config_suspend_all`, ACPI and `cpu_reset` are reported as unported when reached.

use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use libkern::StaticCell;

use crate::arch::amd64::amd64::autoconf::COLD;
use crate::arch::amd64::amd64::consinit::consinit;
use crate::arch::amd64::amd64::cpu::{
    CPU_INFO_FULL_PRIMARY, cpu_enter_pages, cpu_info_primary_init, cpu_init_msrs,
};
use crate::arch::amd64::amd64::locore::lgdt;
use crate::arch::amd64::amd64::pmap::{PMAP_DIRECT_BASE, PMAP_DIRECT_END, pmap_bootstrap};
use crate::arch::amd64::amd64::vector::Xexceptions;
use crate::arch::amd64::include::cpu::cpu_info_primary;
use crate::arch::amd64::include::cpufunc::{lidt, lldt, ltr};
use crate::arch::amd64::include::param::PAGE_SIZE;
use crate::arch::amd64::include::segments::{
    GCODE_SEL, GDATA_SEL, GDT_SIZE, GPROC0_SEL, GUCODE_SEL, GUDATA_SEL, GateDescriptor,
    MemSegmentDescriptor, NIDT, RegionDescriptor, SDT_MEMERA, SDT_MEMRWA, SDT_SYS386IGT,
    SDT_SYS386TSS, SEL_KPL, SEL_UPL, SysSegmentDescriptor, gdt_addr_mem, gdt_addr_sys, gsel,
    gsyssel,
};
use crate::arch::amd64::include::tss::X86_64Tss;
use crate::arch::amd64::include::vmparam::VM_MAXUSER_ADDRESS;
use crate::arch::amd64::isa::clock::i8254_delay;
use crate::kassert;
use crate::kern::init_main::BOOTHOWTO;
use crate::kern::subr_log::init_static_msgbuf;
use crate::kprintf;
use crate::machine::bootinfo::{BootInfo, MemKind};
use crate::machine::db_machdep::db_enter;
use crate::machine::{Cpu, Machine};
use crate::sys::param::roundup;
use crate::sys::reboot::{
    RB_DUMP, RB_HALT, RB_KDB, RB_NOSYNC, RB_POWERDOWN, RB_RESET, RB_TIMEBAD, RB_USERREQ,
};
use crate::sys::systm::PHYSMEM;
use crate::sys::types::Paddr;
use crate::unported;
use crate::uvm::uvm_extern::UvmConstraintRange;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_page::{uvm_page_physload, uvm_setpagesize};
use crate::uvm::uvm_param::{atop, ptoa, trunc_page};

#[cfg(feature = "qemu")]
use crate::arch::amd64::amd64::qemu;
#[cfg(not(feature = "qemu"))]
use crate::dev::cons::{cngetc, cnpollc};
#[cfg(feature = "qemu")]
use crate::machine::ExitStatus;

/// `cpureset_delay`: milliseconds to wait before resetting, from the `CPURESET_DELAY` option
/// (0 when not configured).
pub static CPURESET_DELAY: AtomicI32 = AtomicI32::new(0);
/// `lid_action`: what closing the lid does (`machdep.lidaction`).
pub static LID_ACTION: AtomicI32 = AtomicI32::new(1);
/// `waittime`: set once the file systems have been synced on the way down.
static WAITTIME: AtomicI32 = AtomicI32::new(-1);
/// `isa_constraint`: what ISA DMA can reach.
pub static ISA_CONSTRAINT: UvmConstraintRange = UvmConstraintRange {
    ucr_low: Paddr::new(0),
    ucr_high: Paddr::new(0x00ff_ffff),
};
/// `dma_constraint`: what 32-bit DMA can reach.
pub static DMA_CONSTRAINT: UvmConstraintRange = UvmConstraintRange {
    ucr_low: Paddr::new(0),
    ucr_high: Paddr::new(0xffff_ffff),
};
/// `uvm_md_constraints[]`: the machine's DMA ranges.
pub static UVM_MD_CONSTRAINTS: [&UvmConstraintRange; 2] = [&ISA_CONSTRAINT, &DMA_CONSTRAINT];

/// The interrupt descriptor table, one page (the C's `early_idt` in `locore0.S`, later a
/// page `init_x86_64` maps at `idt_vaddr`).
#[repr(C, align(4096))]
pub struct Idt(pub [GateDescriptor; NIDT]);

/// `idt`: the interrupt descriptor table. Written by `init_x86_64` and `idt_vec_set` on the
/// boot CPU; the CPUs read it.
pub static IDT: StaticCell<Idt> = StaticCell::new(Idt([const { GateDescriptor::zeroed() }; NIDT]));

/// `idt_allocmap[]`: which vectors are taken.
pub static IDT_ALLOCMAP: [AtomicBool; NIDT] = [const { AtomicBool::new(false) }; NIDT];
/// The direct map covers at least this much, by the boot protocol's guarantee.
const DIRECT_MAP_MIN_SIZE: usize = 4 << 30;

/// `init_x86_64`: the first C of the kernel, called from `locore` with the machine as the
/// bootloader left it. Here: the direct map, the message buffer, the console, the pmap
/// bootstrap, the physical memory and the `boot -d` hook.
///
/// # Safety
///
/// Call once, on the boot CPU, before anything else runs, with `boot` describing the loaded
/// image.
pub unsafe fn init_x86_64(boot: &BootInfo) -> Result<(), &'static str> {
    // The C's locore0.S points GS.base at cpu_info_primary before init_x86_64, whose first
    // call is cpu_init_msrs; here the static initialiser of cpu_info_full_primary runs first,
    // then the MSRs, so curcpu() works from this point on.
    cpu_info_primary_init();
    let ci = cpu_info_primary();
    // SAFETY: the boot CPU, once, before anything reads curcpu().
    unsafe { cpu_init_msrs(ci) };

    // The direct map is the bootloader's (see the module's deviations); the C derives
    // pmap_direct_base from L4_SLOT_DIRECT here.
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

    // cpu_init_early_vctrap and the early PTE pages: M4 and the page tables.

    init_static_msgbuf();
    consinit(); // cninit() in C

    // The memory map is the bootloader's, already flensed (see the module's deviations).
    let avail_end = regions
        .iter()
        .filter(|r| r.kind == MemKind::Usable)
        .map(|r| r.base.as_usize() + r.length.as_usize())
        .max()
        .unwrap_or(0);

    // Call pmap initialization to make new kernel address space.
    // SAFETY: once, on the boot CPU, with the direct map set above and paging on.
    let first_avail = unsafe { pmap_bootstrap(Paddr::new(0), Paddr::new(trunc_page(avail_end))) };

    // Now, load the memory clusters (which have already been flensed) into the VM system.
    for r in regions.iter().filter(|r| r.kind == MemKind::Usable) {
        let seg_start = r.base.as_usize().max(first_avail.as_usize());
        let seg_end = r.base.as_usize() + r.length.as_usize();

        if seg_start > seg_end {
            continue;
        }
        if seg_end - seg_start < PAGE_SIZE {
            continue;
        }

        PHYSMEM.fetch_add(atop(r.length.as_usize()), Ordering::Relaxed);

        uvm_page_physload(
            atop(seg_start),
            atop(seg_end),
            atop(seg_start),
            atop(seg_end),
            0,
        );
    }
    // The memory between the ISA hole and the kernel, and the message buffer pages: the map
    // has no hole to load around, and the message buffer is static (M2).

    uvm_setpagesize();

    // The idt_vaddr/idt_paddr page: the IDT is a static page here (see the module's
    // deviations). proc0paddr, the lapic page and the trampoline pages: M5, M4-b, M6.

    ci.ci_tss.set(CPU_INFO_FULL_PRIMARY.cif_tss.get());
    ci.ci_gdt
        .set(CPU_INFO_FULL_PRIMARY.cif_gdt.get().cast::<u8>());

    // make gdt gates and memory segments
    // SAFETY: the boot CPU's GDT, written once here before lgdt loads it.
    let gdt = unsafe { &mut *CPU_INFO_FULL_PRIMARY.cif_gdt.get() };
    set_mem_segment(
        &mut gdt[gdt_addr_mem(GCODE_SEL)],
        0,
        0xfffff,
        SDT_MEMERA,
        SEL_KPL,
        true,
        false,
        true,
    );
    set_mem_segment(
        &mut gdt[gdt_addr_mem(GDATA_SEL)],
        0,
        0xfffff,
        SDT_MEMRWA,
        SEL_KPL,
        true,
        false,
        true,
    );
    set_mem_segment(
        &mut gdt[gdt_addr_mem(GUDATA_SEL)],
        0,
        atop(VM_MAXUSER_ADDRESS) - 1,
        SDT_MEMRWA,
        SEL_UPL,
        true,
        false,
        true,
    );
    set_mem_segment(
        &mut gdt[gdt_addr_mem(GUCODE_SEL)],
        0,
        atop(VM_MAXUSER_ADDRESS) - 1,
        SDT_MEMERA,
        SEL_UPL,
        true,
        false,
        true,
    );

    // make ldt memory segments: no LDT (M6).

    let sys = gdt_addr_sys(GPROC0_SEL);
    set_sys_segment(
        &mut gdt[sys..sys + 2],
        CPU_INFO_FULL_PRIMARY.cif_tss.get() as usize,
        size_of::<X86_64Tss>() - 1,
        SDT_SYS386TSS,
        SEL_KPL,
        false,
    );

    // exceptions
    // SAFETY: the boot CPU's IDT, written here before cpu_init_idt loads it.
    let idt = unsafe { IDT.get_mut() };
    for x in 0..32usize {
        // trap2 == NMI, trap8 == double fault
        let ist = match x {
            2 => 2,
            8 => 1,
            _ => 0,
        };
        // SAFETY: Xexceptions is vector.S's table of the 32 exception entry points.
        let func = unsafe { Xexceptions[x] } as usize;
        setgate(
            &mut idt.0[x],
            func,
            ist,
            SDT_SYS386IGT,
            if x == 3 { SEL_UPL } else { SEL_KPL },
            gsel(GCODE_SEL, SEL_KPL),
        );
        IDT_ALLOCMAP[x].store(true, Ordering::Relaxed);
    }

    let mut region = RegionDescriptor {
        rd_limit: 0,
        rd_base: 0,
    };
    setregion(
        &mut region,
        CPU_INFO_FULL_PRIMARY.cif_gdt.get() as usize,
        (GDT_SIZE - 1) as u16,
    );
    // SAFETY: the GDT filled above has valid 64-bit kernel code and data segments at
    // GCODE_SEL/GDATA_SEL and lives forever.
    unsafe { lgdt(&region) };
    cpu_init_idt();

    // intr_default_setup(): M4-b. fpuinit: M6.

    x86_64_proc0_tss_ldt_init();

    // cpu_init(&cpu_info_primary), the ACPI/MP tables, the memory-map and -b/-c handling of
    // the bootinfo: M4-b and M5; db_machine_init() and ddb_init() with the command loop.
    if BOOTHOWTO.load(Ordering::Relaxed) & RB_KDB != 0 {
        db_enter();
    }
    Ok(())
}

/// `x86_64_proc0_tss_ldt_init`: loads the boot CPU's task register and clears the LDT.
pub fn x86_64_proc0_tss_ldt_init() {
    // cpu_info_primary.ci_curpcb = pcb = &proc0.p_addr->u_pcb; pcb_fsbase, pcb_kstack and
    // proc0.p_md.md_regs: proc0 arrives with M5.
    let _ = unported!("proc0's pcb (x86_64_proc0_tss_ldt_init)");

    // SAFETY: GPROC0_SEL holds the available TSS descriptor init_x86_64 set, loaded once;
    // selector 0 means no LDT.
    unsafe {
        ltr(gsyssel(GPROC0_SEL, SEL_KPL));
        lldt(0);
    }
}

/// `cpu_startup`: machine-dependent startup code (see the module's deviations).
pub fn cpu_startup() {
    // msgbuf_vaddr / initmsgbuf: the message buffer is static (M2). version, startclocks,
    // rtcinit: M5.

    let physmem = PHYSMEM.load(Ordering::Relaxed);
    kprintf!(
        "real mem = {} ({}MB)\n",
        ptoa(physmem),
        ptoa(physmem) / 1024 / 1024
    );

    // exec_map, cpu_init_extents, the physio map, bufinit: M6 and M7.

    let free = UVMEXP.free.load(Ordering::Relaxed).max(0) as usize;
    kprintf!(
        "avail mem = {} ({}MB)\n",
        ptoa(free),
        ptoa(free) / 1024 / 1024
    );

    // cpu_boot_mode, the ISA DMA bounce pages, the microcode and TSX setup,
    // enter_shared_special_pages (the u-k maps): M4-b and M6.

    // initialize CPU0's TSS and GDT and put them in the u-k maps
    // SAFETY: once, on the boot CPU, for its own pages (the TSS is loaded; the CPU reads it
    // on the next privilege or stack switch).
    unsafe { cpu_enter_pages(&CPU_INFO_FULL_PRIMARY) };
}

/// `boot(9)`: halts or reboots according to `howto`.
pub fn boot(howto: i32) -> ! {
    let mut howto = howto;

    // NACPI > 0: acpi_softc->sc_state = ACPI_STATE_S5 on RB_POWERDOWN (M4+).

    if howto & RB_POWERDOWN != 0 {
        LID_ACTION.store(0, Ordering::Relaxed);
    }

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

        // MULTIPROCESSOR: x86_broadcast_ipi(X86_IPI_HALT): not configured.

        if howto & RB_HALT != 0 {
            // NACPI > 0 && !SMALL_KERNEL: delay(500000) and acpi_powerdown() (M4+).
            kprintf!("\n");
            kprintf!("The operating system has halted.\n");
            kprintf!("Please press any key to reboot.\n\n");
            #[cfg(feature = "qemu")]
            {
                qemu::exit(ExitStatus::Failure)
            }
            #[cfg(not(feature = "qemu"))]
            {
                cnpollc(true); // for proper keyboard command handling
                cngetc();
                cnpollc(false);
            }
        }
    }

    // doreset:
    kprintf!("rebooting...\n");
    let d = CPURESET_DELAY.load(Ordering::Relaxed);
    if d > 0 {
        delay((d * 1000) as u32);
    }
    cpu_reset()
}

/// `cpu_reset`: resets the CPU; until the descriptor tables are ported (M4), parks it.
pub fn cpu_reset() -> ! {
    let _ = unported!("cpu_reset");
    Machine::halt()
}

/// `delay(9)`: busy-waits `usec` microseconds (`delay_func`, see the module's deviations).
pub fn delay(usec: u32) {
    i8254_delay(usec.min(i32::MAX as u32) as i32);
}

/// `setgate`: fills an interrupt or trap gate for `func` with `ist`, `type_`, `dpl` and the
/// code selector `sel`.
pub fn setgate(gd: &mut GateDescriptor, func: usize, ist: u8, type_: u8, dpl: u16, sel: u16) {
    *gd = GateDescriptor::pack(func as u64, sel, ist, type_, dpl as u8, true);
}

/// `unsetgate`: clears a gate.
pub fn unsetgate(gd: &mut GateDescriptor) {
    *gd = GateDescriptor::zeroed();
}

/// `setregion`: fills the `lgdt`/`lidt` operand.
pub fn setregion(rd: &mut RegionDescriptor, base: usize, limit: u16) {
    rd.rd_limit = limit;
    rd.rd_base = base as u64;
}

/// `set_mem_segment`: fills a memory segment descriptor. Note that the base and limit fields
/// are ignored in long mode.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn set_mem_segment(
    sd: &mut u64,
    base: usize,
    limit: usize,
    type_: u8,
    dpl: u16,
    gran: bool,
    def32: bool,
    is64: bool,
) {
    *sd = MemSegmentDescriptor::pack(
        base as u64,
        limit as u32,
        type_,
        dpl as u8,
        true,
        false,
        is64,
        def32,
        gran,
    )
    .0;
}

/// `set_sys_segment`: fills a 16-byte system segment descriptor (`sd[0]`, `sd[1]`).
pub fn set_sys_segment(sd: &mut [u64], base: usize, limit: usize, type_: u8, dpl: u16, gran: bool) {
    let d = SysSegmentDescriptor::pack(base as u64, limit as u32, type_, dpl as u8, true, gran);
    sd[0] = d.lo;
    sd[1] = d.hi;
}

/// `cpu_init_idt`: loads this CPU's IDT register with the kernel's table.
pub fn cpu_init_idt() {
    let mut region = RegionDescriptor {
        rd_limit: 0,
        rd_base: 0,
    };
    setregion(
        &mut region,
        IDT.as_ptr() as usize,
        (NIDT * size_of::<GateDescriptor>() - 1) as u16,
    );
    // SAFETY: the IDT is a static page whose gates init_x86_64 filled; it lives forever.
    unsafe { lidt(&region) };
}

/// `idt_vec_alloc`: takes the first free vector in `low..=high`, or 0 if none.
pub fn idt_vec_alloc(low: i32, high: i32) -> i32 {
    for vec in low..=high {
        if !IDT_ALLOCMAP[vec as usize].swap(true, Ordering::Relaxed) {
            return vec;
        }
    }
    0
}

/// `idt_vec_alloc_range`: takes `num` (a power of two) aligned consecutive free vectors in
/// `low..=high`, or 0 if none.
pub fn idt_vec_alloc_range(low: i32, high: i32, num: i32) -> i32 {
    kassert!(num > 0 && num & (num - 1) == 0);
    let low = (low + num - 1) & !(num - 1);
    let high = ((high + 1) & !(num - 1)) - 1;

    let mut vec = low;
    while vec <= high {
        let free = (0..num).all(|i| !IDT_ALLOCMAP[(vec + i) as usize].load(Ordering::Relaxed));
        if free {
            for i in 0..num {
                IDT_ALLOCMAP[(vec + i) as usize].store(true, Ordering::Relaxed);
            }
            return vec;
        }
        vec += num;
    }
    0
}

/// `idt_vec_set`: points an allocated vector at `function`.
pub fn idt_vec_set(vec: i32, function: usize) {
    // Vector should be allocated, so no locking needed.
    kassert!(IDT_ALLOCMAP[vec as usize].load(Ordering::Relaxed));
    // SAFETY: the vector is allocated, so nothing else writes its gate; the CPU reads the
    // table on the next interrupt.
    let idt = unsafe { IDT.get_mut() };
    setgate(
        &mut idt.0[vec as usize],
        function,
        0,
        SDT_SYS386IGT,
        SEL_KPL,
        gsel(GCODE_SEL, SEL_KPL),
    );
}

/// `idt_vec_free`: clears a vector's gate and frees it.
pub fn idt_vec_free(vec: i32) {
    // SAFETY: as for `idt_vec_set`; the caller no longer expects the vector to fire.
    let idt = unsafe { IDT.get_mut() };
    unsetgate(&mut idt.0[vec as usize]);
    IDT_ALLOCMAP[vec as usize].store(false, Ordering::Relaxed);
}
