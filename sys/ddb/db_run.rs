/*	$OpenBSD: db_run.c,v 1.33 2025/07/22 09:09:50 kettenis Exp $	*/
/*	$NetBSD: db_run.c,v 1.8 1996/02/05 01:57:12 christos Exp $	*/
/*	$OpenBSD: db_run.h,v 1.12 2019/11/06 07:30:08 mpi Exp $	*/
/*	$NetBSD: db_run.h,v 1.3 1996/02/05 01:57:14 christos Exp $	*/
/* <LICENSES> */
/*
 * Mach Operating System
 * Copyright (c) 1993,1992,1991,1990 Carnegie Mellon University
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
 * any improvements or extensions that they make and grant Carnegie Mellon
 * the rights to redistribute these changes.
 *
 * 	Author: David B. Golub, Carnegie Mellon University
 *	Date:	7/90
 */
/* </LICENSES> */

/* <CODE> */
//! Commands to run the stopped kernel: `ddb/db_run.c` and `<ddb/db_run.h>`.
//!
//! Upstream: sys/ddb/db_run.c @ 3ce1f3f79392
//! Upstream: sys/ddb/db_run.h @ 3ce1f3f79392
//!
//! `continue`, `step`, `until` and `next`/`match` set a run mode and end the command loop.
//! `db_restart_at_pc` then arms the hardware single step (or the breakpoints) before the
//! kernel resumes, and `db_stop_at_pc`, on the next debugger trap, decides from the mode
//! whether to stop there or to continue silently.
//!
//! ## Deviations
//! - `db_regs_t *regs` is always `&ddb_regs` (`db_trap` is the only caller): the machine's
//!   `pc_regs`, `set_pc_regs`, `fixup_pc_after_break`, `db_set_single_step` and
//!   `db_clear_single_step` work on it.
//! - Breakpoints (`db_break.c`: `db_find_breakpoint`, `db_set_breakpoints`,
//!   `db_clear_breakpoints`) and watchpoints (`db_watch.c`: `db_set_watchpoints`,
//!   `db_clear_watchpoints`) are not ported: visible stubs with an empty table, so no
//!   breakpoint is ever found, set or cleared.
//! - `db_get_value` (`db_access.c`) is not ported: `STEP_RETURN` and `STEP_CALLT` (`next`,
//!   `match`, `until`), which read the instruction at the PC, stop at the first instruction
//!   instead. `db_restart_at_pc`'s read of the instruction is only used by `SOFTWARE_SSTEP`,
//!   so it only counts it. `db_print_loc_and_inst` (`db_examine.c`) prints the address.
//! - `SOFTWARE_SSTEP` (software single step with temporary breakpoints) is not defined for
//!   amd64 or arm64; its code is not compiled here either.
//! - The run state is atomics; ddb runs on one CPU at a time.

use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::ddb::db_command::{DB_CMD_LOOP_DONE, DbResult, db_get_value, db_print_loc_and_inst};
use crate::kern::subr_prf::db_printf;
use crate::machine::db_machdep::{
    DbExpr, db_clear_single_step, db_set_single_step, fixup_pc_after_break, inst_call, inst_return,
    inst_trap_return, pc_regs, set_pc_regs,
};
use crate::unported;

/// `STEP_NONE`.
const STEP_NONE: i32 = 0;
/// `STEP_ONCE`.
const STEP_ONCE: i32 = 1;
/// `STEP_RETURN`.
const STEP_RETURN: i32 = 2;
/// `STEP_CALLT`.
const STEP_CALLT: i32 = 3;
/// `STEP_CONTINUE`.
const STEP_CONTINUE: i32 = 4;
/// `STEP_INVISIBLE`.
const STEP_INVISIBLE: i32 = 5;
/// `STEP_COUNT`.
const STEP_COUNT: i32 = 6;

/// `db_inst_count`: instructions executed since the last run command.
pub static DB_INST_COUNT: AtomicI32 = AtomicI32::new(0);
/// `db_run_mode`.
static DB_RUN_MODE: AtomicI32 = AtomicI32::new(STEP_NONE);
/// `db_sstep_print`.
static DB_SSTEP_PRINT: AtomicBool = AtomicBool::new(false);
/// `db_loop_count`.
static DB_LOOP_COUNT: AtomicI32 = AtomicI32::new(0);
/// `db_call_depth`.
static DB_CALL_DEPTH: AtomicI32 = AtomicI32::new(0);

/// `db_find_breakpoint(pc)` (`db_break.c`): whether a breakpoint is set at `pc`. Not ported:
/// the table is empty.
fn db_find_breakpoint(pc: usize) -> bool {
    let _ = pc;
    let _ = unported!("db_find_breakpoint (db_break.c)");
    false
}

/// `db_set_breakpoints` (`db_break.c`): not ported, the table is empty.
fn db_set_breakpoints() {
    let _ = unported!("db_set_breakpoints (db_break.c)");
}

/// `db_clear_breakpoints` (`db_break.c`): not ported, the table is empty.
fn db_clear_breakpoints() {
    let _ = unported!("db_clear_breakpoints (db_break.c)");
}

/// `db_set_watchpoints` (`db_watch.c`): not ported, the table is empty.
fn db_set_watchpoints() {
    let _ = unported!("db_set_watchpoints (db_watch.c)");
}

/// `db_clear_watchpoints` (`db_watch.c`): not ported, the table is empty.
fn db_clear_watchpoints() {
    let _ = unported!("db_clear_watchpoints (db_watch.c)");
}

/// The instruction at `pc`, or `None` when it cannot be read (`db_get_value` is not ported).
fn db_get_inst(pc: usize) -> Option<DbExpr> {
    let r: DbResult<DbExpr> = db_get_value(pc, size_of::<i32>(), false);
    r.ok()
}

/// `db_stop_at_pc`: whether the debugger stops at the trap in `ddb_regs`. `is_breakpoint`
/// says whether it was a breakpoint trap, and on return whether it was one of ddb's.
pub fn db_stop_at_pc(is_breakpoint: &mut bool) -> bool {
    db_clear_breakpoints();
    db_clear_watchpoints();
    let old_pc = pc_regs();
    let mut pc = old_pc;

    if *is_breakpoint {
        // Breakpoint trap. Fix up the PC if the machine requires it.
        fixup_pc_after_break();
        pc = pc_regs();
    }

    // Now check for a breakpoint at this address.
    if db_find_breakpoint(pc) {
        // db_break.c's per-breakpoint count would decide here; there are none.
        db_clear_single_step();
        *is_breakpoint = true;
        return true; // stop here
    } else if *is_breakpoint {
        set_pc_regs(old_pc);
    }
    db_clear_single_step();

    *is_breakpoint = false;

    let mode = DB_RUN_MODE.load(Ordering::Relaxed);
    if mode == STEP_INVISIBLE {
        DB_RUN_MODE.store(STEP_CONTINUE, Ordering::Relaxed);
        return false; // continue
    }
    if mode == STEP_COUNT {
        return false; // continue
    }
    if mode == STEP_ONCE && DB_LOOP_COUNT.fetch_sub(1, Ordering::Relaxed) - 1 > 0 {
        if DB_SSTEP_PRINT.load(Ordering::Relaxed) {
            db_printf(format_args!("\t\t"));
            db_print_loc_and_inst(pc);
            db_printf(format_args!("\n"));
        }
        return false; // continue
    }
    if mode == STEP_RETURN
        && let Some(ins) = db_get_inst(pc)
    {
        // continue until matching return
        if !inst_trap_return(ins)
            && (!inst_return(ins) || DB_CALL_DEPTH.fetch_sub(1, Ordering::Relaxed) - 1 != 0)
        {
            if DB_SSTEP_PRINT.load(Ordering::Relaxed) && (inst_call(ins) || inst_return(ins)) {
                db_printf(format_args!(
                    "[after {:6}]     ",
                    DB_INST_COUNT.load(Ordering::Relaxed)
                ));
                for _ in 1..DB_CALL_DEPTH.load(Ordering::Relaxed) {
                    db_printf(format_args!("  "));
                }
                db_print_loc_and_inst(pc);
                db_printf(format_args!("\n"));
            }
            if inst_call(ins) {
                DB_CALL_DEPTH.fetch_add(1, Ordering::Relaxed);
            }
            return false; // continue
        }
    }
    if mode == STEP_CALLT
        && let Some(ins) = db_get_inst(pc)
    {
        // continue until call or return
        if !inst_call(ins) && !inst_return(ins) && !inst_trap_return(ins) {
            return false; // continue
        }
    }
    DB_RUN_MODE.store(STEP_NONE, Ordering::Relaxed);
    true
}

/// `db_restart_at_pc`: arms the run mode before the kernel resumes from `ddb_regs`.
pub fn db_restart_at_pc(watchpt: bool) {
    let pc = pc_regs();
    let mode = DB_RUN_MODE.load(Ordering::Relaxed);

    if mode == STEP_COUNT || mode == STEP_RETURN || mode == STEP_CALLT {
        // We are about to execute this instruction, so count it now.
        DB_INST_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    if mode == STEP_CONTINUE {
        if watchpt || db_find_breakpoint(pc) {
            // Step over breakpoint/watchpoint.
            DB_RUN_MODE.store(STEP_INVISIBLE, Ordering::Relaxed);
            db_set_single_step();
        } else {
            db_set_breakpoints();
            db_set_watchpoints();
        }
    } else if mode != STEP_NONE {
        db_set_single_step();
    }
}

/// `db_single_step`: steps over the instruction at a breakpoint before continuing.
pub fn db_single_step() {
    if DB_RUN_MODE.load(Ordering::Relaxed) == STEP_CONTINUE {
        DB_RUN_MODE.store(STEP_INVISIBLE, Ordering::Relaxed);
        db_set_single_step();
    }
}

/// `db_single_step_cmd`: `step[/p] [,count]`, single-step.
pub fn db_single_step_cmd(
    _addr: DbExpr,
    _have_addr: bool,
    count: DbExpr,
    modif: &[u8],
) -> DbResult {
    let count = if count == -1 { 1 } else { count };
    let print = modif.first() == Some(&b'p');

    DB_RUN_MODE.store(STEP_ONCE, Ordering::Relaxed);
    DB_LOOP_COUNT.store(count as i32, Ordering::Relaxed);
    DB_SSTEP_PRINT.store(print, Ordering::Relaxed);
    DB_INST_COUNT.store(0, Ordering::Relaxed);

    DB_CMD_LOOP_DONE.store(true, Ordering::Relaxed);
    Ok(())
}

/// `db_trace_until_call_cmd`: `until[/p]`, trace and print until call/return.
pub fn db_trace_until_call_cmd(
    _addr: DbExpr,
    _have_addr: bool,
    _count: DbExpr,
    modif: &[u8],
) -> DbResult {
    let print = modif.first() == Some(&b'p');

    DB_RUN_MODE.store(STEP_CALLT, Ordering::Relaxed);
    DB_SSTEP_PRINT.store(print, Ordering::Relaxed);
    DB_INST_COUNT.store(0, Ordering::Relaxed);

    DB_CMD_LOOP_DONE.store(true, Ordering::Relaxed);
    Ok(())
}

/// `db_trace_until_matching_cmd`: `next[/p]` and `match[/p]`, until the matching return.
pub fn db_trace_until_matching_cmd(
    _addr: DbExpr,
    _have_addr: bool,
    _count: DbExpr,
    modif: &[u8],
) -> DbResult {
    let print = modif.first() == Some(&b'p');

    DB_RUN_MODE.store(STEP_RETURN, Ordering::Relaxed);
    DB_CALL_DEPTH.store(1, Ordering::Relaxed);
    DB_SSTEP_PRINT.store(print, Ordering::Relaxed);
    DB_INST_COUNT.store(0, Ordering::Relaxed);

    DB_CMD_LOOP_DONE.store(true, Ordering::Relaxed);
    Ok(())
}

/// `db_continue_cmd`: `continue[/c]`, the `c` modifier counts instructions.
pub fn db_continue_cmd(_addr: DbExpr, _have_addr: bool, _count: DbExpr, modif: &[u8]) -> DbResult {
    if modif.first() == Some(&b'c') {
        DB_RUN_MODE.store(STEP_COUNT, Ordering::Relaxed);
    } else {
        DB_RUN_MODE.store(STEP_CONTINUE, Ordering::Relaxed);
    }
    DB_INST_COUNT.store(0, Ordering::Relaxed);

    DB_CMD_LOOP_DONE.store(true, Ordering::Relaxed);
    Ok(())
}
/* </CODE> */
