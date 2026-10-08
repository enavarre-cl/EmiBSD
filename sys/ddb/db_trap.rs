/*	$OpenBSD: db_trap.c,v 1.30 2019/11/06 07:30:08 mpi Exp $	*/
/*	$NetBSD: db_trap.c,v 1.9 1996/02/05 01:57:18 christos Exp $	*/
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
//! Trap entry point to kernel debugger: `ddb/db_trap.c`.
//!
//! Upstream: sys/ddb/db_trap.c @ 3ce1f3f79392
//!
//! The per-arch `db_ktrap` lands here with the trap frame saved in `ddb_regs`. `db_trap` asks
//! `db_stop_at_pc` whether to stop (a run command may want to go on silently), says where the
//! kernel stopped, prints the stack trace when the kernel panicked, and runs the command
//! loop; `db_restart_at_pc` then arms the run mode before the trap handler resumes.
//!
//! ## Deviations
//! - `db_print_loc_and_inst` (`db_examine.c`, the symbol table and the disassembler) prints
//!   the address (`db_command.rs`'s stand-in). `db_show_all_procs(0, 0, 0, "o")`, which lists
//!   the threads on a CPU after a panic, is `kern_proc.c`'s and a visible stub.
//! - The "ddb.html" notice, which asks for an OpenBSD bug report, is not printed by this
//!   kernel; `ddb_msg_shown` is still set, so the thread list prints once.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::ddb::db_command::{DB_DOT, db_command_loop, db_print_loc_and_inst, db_show_all_procs};
use crate::ddb::db_output::db_print_position;
use crate::ddb::db_run::{DB_INST_COUNT, db_restart_at_pc, db_stop_at_pc};
use crate::kern::subr_prf::{db_printf, panicstr};
use crate::machine::db_machdep::{
    db_stack_trace_print, is_breakpoint_trap, is_watchpoint_trap, pc_regs,
};

/// `ddb_msg_shown`: the thread list (and, in OpenBSD, the bug-report notice) was printed
/// after a panic.
static DDB_MSG_SHOWN: AtomicBool = AtomicBool::new(false);

/// `db_trap`: the debugger's entry from a trap of `type_` with `code`, with the registers in
/// `ddb_regs`.
pub fn db_trap(type_: i32, code: i32) {
    fn pr(args: core::fmt::Arguments<'_>) {
        db_printf(args);
    }

    let mut bkpt = is_breakpoint_trap(type_, code);
    let watchpt = is_watchpoint_trap(type_, code);

    if db_stop_at_pc(&mut bkpt) {
        let inst_count = DB_INST_COUNT.load(Ordering::Relaxed);
        if inst_count != 0 {
            db_printf(format_args!("After {inst_count} instructions\n"));
        }
        if bkpt {
            db_printf(format_args!("Breakpoint at\t"));
        } else if watchpt {
            db_printf(format_args!("Watchpoint at\t"));
        } else {
            db_printf(format_args!("Stopped at\t"));
        }
        let db_dot = pc_regs();
        DB_DOT.store(db_dot, Ordering::Relaxed);
        db_print_loc_and_inst(db_dot);

        if panicstr() {
            if !DDB_MSG_SHOWN.load(Ordering::Relaxed) {
                // show on-proc threads
                let _ = db_show_all_procs(0, false, 0, b"o");
            }
            // then the backtrace
            db_stack_trace_print(db_dot, false, 14 /* arbitrary */, b"", pr);

            if db_print_position() != 0 {
                db_printf(format_args!("\n"));
            }
            // The ddb.html notice: not printed (see the deviations).
            DDB_MSG_SHOWN.store(true, Ordering::Relaxed);
        }

        db_command_loop();
    }

    db_restart_at_pc(watchpt);
}
/* </CODE> */
