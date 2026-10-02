/* $OpenBSD: machdep.c,v 1.101 2026/09/06 18:25:22 mglocker Exp $ */
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

//! arm64 machine-dependent setup and shutdown: `arch/arm64/arm64/machdep.c`.
//!
//! Upstream: sys/arch/arm64/arm64/machdep.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports what the console and a panic need: the part of `initarm`
//! that brings up the message buffer and the console, `consinit`, `boot`, `cold`, `waittime`,
//! `cpuresetfn` and `powerdownfn`. `cpu_startup`, the FDT and memory setup, `dumpsys`,
//! `sendsig`/`setregs`, the sysctl tree and the bootstrap KVA helpers arrive with M3 to M5.
//!
//! ## Deviations
//! - Limine has set up EL1, the MMU and the direct map before `initarm` runs, so the C's
//!   page-table and memory-map work is replaced by the boot protocol (`docs/ARCHITECTURE.md`).
//!   What Limine does not map is device memory: `initarm` installs one 1 GiB identity block of
//!   Device-nGnRnE memory in `TTBR0_EL1` (the lower half, which the protocol leaves to the
//!   kernel), through `MAIR_EL1` attribute 2, so `bus_space` can reach the PL011. `pmap` (M3)
//!   replaces it.
//! - The message buffer is a static area (`kern/subr_log.rs`, `init_static_msgbuf`) instead of
//!   reserved physical pages, until M3.
//! - `consinit` attaches the PL011 at QEMU `virt`'s address directly: `pluart_init_cons`
//!   (`dev/fdt/pluart_fdt.c`) needs the device tree (M4), and the other `*_init_cons` are
//!   drivers for hardware QEMU does not have (`deferred-driver`).
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

use crate::arch::arm64::arm64::bus_space::FDT_CONS_BS_TAG;
use crate::arch::arm64::arm64::intr::delay;
use crate::dev::ic::pluart::pluartcnattach;
use crate::kern::init_main::BOOTHOWTO;
use crate::kern::subr_log::init_static_msgbuf;
use crate::kprintf;
use crate::machine::bootinfo::BootInfo;
use crate::machine::bus::BusAddr;
use crate::machine::db_machdep::db_enter;
use crate::machine::{Cpu, Machine};
use crate::sys::reboot::{
    RB_DUMP, RB_HALT, RB_KDB, RB_NOSYNC, RB_POWERDOWN, RB_RESET, RB_TIMEBAD, RB_USERREQ,
};
use crate::sys::termios::B115200;
use crate::sys::ttydefaults::TTYDEF_CFLAG;
use crate::sys::types::Vaddr;
use crate::unported;

#[cfg(feature = "qemu")]
use crate::arch::arm64::arm64::qemu;
#[cfg(not(feature = "qemu"))]
use crate::dev::cons::cngetc;
#[cfg(feature = "qemu")]
use crate::machine::ExitStatus;

/// Size of the bootstrap device map: the first GiB of physical space, identity-mapped as
/// device memory by [`initarm`] (see the module's deviations).
pub const BOOTSTRAP_DEVICE_MAP_SIZE: usize = 1 << 30;

/// The PL011 of QEMU's `virt` machine. TODO(M4): `fdt_find_cons("arm,pl011")`.
const QEMU_VIRT_PL011: BusAddr = 0x0900_0000;

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

/// `cold`: if set, still working on cold-start.
pub static COLD: AtomicBool = AtomicBool::new(true);
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
/// bootloader left it. Here: the bootstrap device map, the message buffer, the console and the
/// `boot -d` hook.
///
/// # Safety
///
/// Call once, on the boot CPU, before anything else runs, with `boot` describing the loaded
/// image.
pub unsafe fn initarm(boot: &BootInfo) -> Result<(), &'static str> {
    // The FDT, memory-map and page-table work of the C happens in the boot protocol; the
    // device map below stands in for `pmap_bootstrap_bs_map` (see the module's deviations).
    // SAFETY: forwarded from the caller.
    unsafe { bootstrap_device_map(boot)? };
    init_static_msgbuf();
    consinit();
    // The rest of initarm (cpu_init, pmap_bootstrap, uvm_setpagesize, ...) arrives with M3 and
    // M4; db_machine_init() and ddb_init() with M4.
    if BOOTHOWTO.load(Ordering::Relaxed) & RB_KDB != 0 {
        db_enter();
    }
    Ok(())
}

/// `consinit`: attaches the console, once.
pub fn consinit() {
    static CONSINIT_CALLED: AtomicBool = AtomicBool::new(false);

    if CONSINIT_CALLED.swap(true, Ordering::Relaxed) {
        return;
    }

    // amluart, cduart, com_fdt, exuart, imxuart, mvuart, qcuart and simplefb consoles: hardware
    // QEMU virt does not have (deferred drivers). pluart_init_cons (dev/fdt/pluart_fdt.c)
    // needs the device tree: M4.
    // SAFETY: QEMU virt's PL011 is at this address and nothing else drives it.
    let attached = unsafe {
        pluartcnattach(
            FDT_CONS_BS_TAG,
            QEMU_VIRT_PL011,
            B115200 as i32,
            TTYDEF_CFLAG,
        )
    };
    // A failure leaves the kernel without a console; there is nowhere to report it.
    let _ = attached;
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
