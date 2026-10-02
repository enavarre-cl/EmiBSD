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
//! use (`cpureset_delay`, `lid_action`, `waittime`). `cpu_startup`, the descriptor tables,
//! `cpu_reset`, `dumpsys`, the bootinfo parsing, `sendsig`/`setregs` and the sysctl tree arrive
//! with M3 to M5.
//!
//! ## Deviations
//! - Limine has set up long mode, paging and the direct map before `init_x86_64` runs, so the
//!   BIOS/EFI memory-map walk and the page-table work of the C version are replaced by the
//!   boot protocol (`docs/ARCHITECTURE.md`, "Boot flow"): `pmap_direct_base` is the
//!   bootloader's higher-half direct map, and the memory clusters loaded into `uvm` are the
//!   protocol's usable regions, which already exclude the kernel, the firmware and the
//!   bootloader's own data. The ISA hole and the `avail_end` bookkeeping have nothing to do.
//! - `cpu_startup` prints the memory sizes only: `version` (generated `vers.c`),
//!   `startclocks`, `rtcinit` (M5), the exec and physio maps and `bufinit` (M6, M7),
//!   `cpu_init_idt` and `cpu_boot_mode` (M4) are not there yet.
//! - The message buffer is a static area (`kern/subr_log.rs`, `init_static_msgbuf`) instead of
//!   reserved physical pages, until M3.
//! - `cninit()` is replaced by `consinit()` (`consinit.rs`): no `constab[]` yet.
//! - `delay` is `i8254_delay` directly; `delay_func`, `delay_init` and `delay_fini` (the TSC
//!   upgrade) arrive with M5.
//! - `boot`: under feature `qemu`, the wait for a key after "The operating system has halted"
//!   is the emulator exit with the failure status, which `xtask smoke` checks after a panic.
//!   `vfs_shutdown`, `resettodr`, `if_downall`, `uvm_shutdown`, `dumpsys`,
//!   `config_suspend_all`, ACPI and `cpu_reset` are reported as unported when reached.

use core::sync::atomic::{AtomicI32, Ordering};

use crate::arch::amd64::amd64::autoconf::COLD;
use crate::arch::amd64::amd64::consinit::consinit;
use crate::arch::amd64::amd64::pmap::{PMAP_DIRECT_BASE, PMAP_DIRECT_END, pmap_bootstrap};
use crate::arch::amd64::include::param::PAGE_SIZE;
use crate::arch::amd64::isa::clock::i8254_delay;
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

    // The rest of init_x86_64 (descriptor tables, cpu_init_idt, the trampolines, ...)
    // arrives with M4 and M5; db_machine_init() and ddb_init() with M4.
    if BOOTHOWTO.load(Ordering::Relaxed) & RB_KDB != 0 {
        db_enter();
    }
    Ok(())
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

    // cpu_init_idt, cpu_boot_mode, the ISA DMA bounce pages: M4 and M6.
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
