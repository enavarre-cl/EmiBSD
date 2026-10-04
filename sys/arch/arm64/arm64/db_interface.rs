/*	$OpenBSD: db_interface.c,v 1.17 2025/07/22 09:20:41 kettenis Exp $	*/
/*	$NetBSD: db_interface.c,v 1.34 2003/10/26 23:11:15 chris Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 Scott K. Stevens
 *
 * Mach Operating System
 * Copyright (c) 1991,1990 Carnegie Mellon University
 * All Rights Reserved.
 *
 * Permission to use, copy, modify and distribute this software and its
 * documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND FOR
 * ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 *
 *	From: db_interface.c,v 2.4 1991/02/05 17:11:13 mrt (CMU)
 */
/* </LICENSES> */

//! Interface to new debugger: `arch/arm64/arm64/db_interface.c`.
//!
//! Upstream: sys/arch/arm64/arm64/db_interface.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `ddb_regs`, `db_ktrap` and `db_enter` (ddb-lite: the
//! trap frame is saved and `db_trap` prints where the kernel stopped); M11c the register
//! table `db_regs[]`. `db_validate_address`, `db_read_bytes`/`db_write_bytes`,
//! `db_machine_init`, the machine commands and the multiprocessor entry/exit (`db_enter_ddb`,
//! `db_startcpu`, `db_stopcpu`) come with the command loop and M11c.
//!
//! ## Deviations
//! - `db_ktrap` has no `db_recover` (`db_command.c`'s longjmp target) and no `splhigh`
//!   (M4-b, reported); `db_active` is the boolean of `init_main.rs`, not a counter.

use libkern::StaticCell;

use crate::arch::arm64::include::armreg::{
    DBG_MDSCR_KDE, DBG_MDSCR_SS, EXCP_BRK, EXCP_SOFTSTP_EL1, EXCP_WATCHPT_EL1, PSR_D, PSR_SS,
    read_specialreg, write_specialreg,
};
use crate::arch::arm64::include::db_machdep::DbRegs;
use crate::db_reg_var;
use crate::ddb::db_trap::db_trap;
use crate::ddb::db_variables::DbVariable;
use crate::dev::cons::cnpollc;
use crate::kern::init_main::DB_ACTIVE;
use crate::unported;

/// `ddb_regs`: register state. Written by `db_ktrap` on the one CPU that is in the
/// debugger, read by `db_trace` and `db_trap` while it is.
pub static DDB_REGS: StaticCell<DbRegs> = StaticCell::new(DbRegs::new());

/// `db_ktrap`: the debugger's entry from an exception of class `type_`; always returns
/// `true` (the kernel continues with `regs`).
pub fn db_ktrap(type_: i32, regs: &mut DbRegs) -> bool {
    // MULTIPROCESSOR: ddb_mp_mutex, db_enter_ddb (M5).

    match type_ {
        // breakpoint, watchpoint, single-step, keyboard interrupt
        t if t == EXCP_BRK as i32
            || t == EXCP_WATCHPT_EL1 as i32
            || t == EXCP_SOFTSTP_EL1 as i32
            || t == -1 => {}
        _ => {
            // db_recover != 0: db_error("Faulted in DDB; continuing...\n"): no command loop.
        }
    }

    // Should switch to kdb`s own stack here.

    // SAFETY: the one CPU entering the debugger writes ddb_regs; the readers (db_trace,
    // db_trap) run below, on this CPU, before it is written again.
    unsafe { DDB_REGS.write(*regs) };

    let _ = unported!("splhigh/splx around db_trap (M4-b)");
    DB_ACTIVE.store(true, core::sync::atomic::Ordering::Relaxed);
    cnpollc(true);
    db_trap(type_, 0 /* code */);
    cnpollc(false);
    DB_ACTIVE.store(false, core::sync::atomic::Ordering::Relaxed);

    // SAFETY: as above; db_trap has returned and nothing else reads ddb_regs now.
    *regs = unsafe { DDB_REGS.read() };

    // MULTIPROCESSOR: ddb_state = DDB_STATE_EXITING unless db_switch_cpu (M5).

    // Enable debug exceptions in the kernel when needed.
    let mut mdscr = read_specialreg!("mdscr_el1");
    if regs.tf_spsr as u64 & PSR_SS != 0 {
        mdscr |= DBG_MDSCR_KDE | DBG_MDSCR_SS;
        regs.tf_spsr &= !(PSR_D as isize);
    } else {
        mdscr &= !(DBG_MDSCR_KDE | DBG_MDSCR_SS);
        regs.tf_spsr |= PSR_D as isize;
    }
    // SAFETY: MDSCR_EL1 only arms or disarms software-step debug events for the kernel,
    // matching the frame about to be restored.
    unsafe { write_specialreg!("mdscr_el1", mdscr) };

    true
}

/// `db_enter`: enters the debugger with `brk #0xf000`, which `do_el1h_sync` hands to
/// `db_ktrap` and then steps over.
pub fn db_enter() {
    // SAFETY: a breakpoint instruction; the exception handler returns past it.
    unsafe { core::arch::asm!("brk #0xf000", options(nomem, nostack, preserves_flags)) };
}

/// `db_regs[]`: the registers of `ddb_regs` as debugger variables. Each reads and writes its
/// field of the trap frame (`db_reg_var!`) where C points `valuep` at it; `x30` is `tf_lr`
/// (the C's `tf_x[30]` is one past the end of `tf_x`, which holds x0 to x29).
pub static DB_REGS: [DbVariable; 35] = [
    db_reg_var!(DDB_REGS, "x0", tf_x[0]),
    db_reg_var!(DDB_REGS, "x1", tf_x[1]),
    db_reg_var!(DDB_REGS, "x2", tf_x[2]),
    db_reg_var!(DDB_REGS, "x3", tf_x[3]),
    db_reg_var!(DDB_REGS, "x4", tf_x[4]),
    db_reg_var!(DDB_REGS, "x5", tf_x[5]),
    db_reg_var!(DDB_REGS, "x6", tf_x[6]),
    db_reg_var!(DDB_REGS, "x7", tf_x[7]),
    db_reg_var!(DDB_REGS, "x8", tf_x[8]),
    db_reg_var!(DDB_REGS, "x9", tf_x[9]),
    db_reg_var!(DDB_REGS, "x10", tf_x[10]),
    db_reg_var!(DDB_REGS, "x11", tf_x[11]),
    db_reg_var!(DDB_REGS, "x12", tf_x[12]),
    db_reg_var!(DDB_REGS, "x13", tf_x[13]),
    db_reg_var!(DDB_REGS, "x14", tf_x[14]),
    db_reg_var!(DDB_REGS, "x15", tf_x[15]),
    db_reg_var!(DDB_REGS, "x16", tf_x[16]),
    db_reg_var!(DDB_REGS, "x17", tf_x[17]),
    db_reg_var!(DDB_REGS, "x18", tf_x[18]),
    db_reg_var!(DDB_REGS, "x19", tf_x[19]),
    db_reg_var!(DDB_REGS, "x20", tf_x[20]),
    db_reg_var!(DDB_REGS, "x21", tf_x[21]),
    db_reg_var!(DDB_REGS, "x22", tf_x[22]),
    db_reg_var!(DDB_REGS, "x23", tf_x[23]),
    db_reg_var!(DDB_REGS, "x24", tf_x[24]),
    db_reg_var!(DDB_REGS, "x25", tf_x[25]),
    db_reg_var!(DDB_REGS, "x26", tf_x[26]),
    db_reg_var!(DDB_REGS, "x27", tf_x[27]),
    db_reg_var!(DDB_REGS, "x28", tf_x[28]),
    db_reg_var!(DDB_REGS, "x29", tf_x[29]),
    db_reg_var!(DDB_REGS, "x30", tf_lr),
    db_reg_var!(DDB_REGS, "sp", tf_sp),
    db_reg_var!(DDB_REGS, "spsr", tf_spsr),
    db_reg_var!(DDB_REGS, "elr", tf_elr),
    db_reg_var!(DDB_REGS, "lr", tf_lr),
];
