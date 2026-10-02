/*	$OpenBSD: db_trap.c,v 1.30 2019/11/06 07:30:08 mpi Exp $	*/
/*	$NetBSD: db_trap.c,v 1.9 1996/02/05 01:57:18 christos Exp $	*/

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

//! Trap entry point to kernel debugger: `ddb/db_trap.c`.
//!
//! Upstream: sys/ddb/db_trap.c @ 3ce1f3f79392
//!
//! Status: `wip` (ddb-lite). The per-arch `db_ktrap` lands here with the trap frame saved in
//! `ddb_regs`; `db_trap` says where the kernel stopped, prints the stack trace the user would
//! ask for and returns, which continues the kernel as the `c` command would.
//!
//! ## Deviations
//! - `db_stop_at_pc`/`db_restart_at_pc` (`db_run.c`: breakpoints, watchpoints, single
//!   stepping, `db_inst_count`) are not here: the kernel always stops, and a breakpoint trap
//!   that is not one of ddb's own (there are none) is reported as "Stopped at", as the C
//!   does after `db_find_breakpoint` fails. `db_command_loop` (`db_command.c`) is reported as
//!   unported; the `trace` the C's panic path runs by itself is printed in every case.
//! - `db_print_loc_and_inst` (`db_sym.c`, the symbol table and the disassembler) prints the
//!   address. `db_show_all_procs` waits for processes (M5); the "ddb.html" notice, which asks
//!   for an OpenBSD bug report, is not printed by this kernel.

use crate::ddb::db_output::db_print_position;
use crate::kern::subr_prf::{db_printf, panicstr};
use crate::machine::db_machdep::{db_stack_trace_print, pc_regs};
use crate::unported;

/// `db_trap`: the debugger's entry from a trap of `type_` with `code`, with the registers in
/// `ddb_regs`.
pub fn db_trap(type_: i32, code: i32) {
    fn pr(args: core::fmt::Arguments<'_>) {
        db_printf(args);
    }

    // bkpt = IS_BREAKPOINT_TRAP(type, code); watchpt = IS_WATCHPOINT_TRAP(type, code):
    // db_stop_at_pc clears `bkpt` when the breakpoint is not one of ddb's (see the module's
    // deviations), and ddb sets none, so the stop is always reported as "Stopped at".
    let _ = (type_, code);
    let bkpt = false;
    let watchpt = false;

    // if (db_stop_at_pc(&ddb_regs, &bkpt)): always.
    if bkpt {
        db_printf(format_args!("Breakpoint at\t"));
    } else if watchpt {
        db_printf(format_args!("Watchpoint at\t"));
    } else {
        db_printf(format_args!("Stopped at\t"));
    }
    let db_dot = pc_regs();
    // db_print_loc_and_inst(db_dot)
    db_printf(format_args!("{db_dot:#x}\n"));
    if panicstr() {
        // show on-proc threads: db_show_all_procs(0, 0, 0, "o") (M5)
    }
    // then the backtrace (the C prints it when panicstr != NULL; ddb-lite always)
    db_stack_trace_print(db_dot, false, 14 /* arbitrary */, b"", pr);
    if db_print_position() != 0 {
        db_printf(format_args!("\n"));
    }
    let _ = unported!("db_command_loop (ddb-lite continues as `c` would)");
    // db_restart_at_pc(&ddb_regs, watchpt): db_run.c.
}
