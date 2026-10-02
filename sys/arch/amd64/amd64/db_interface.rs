/*	$OpenBSD: db_interface.c,v 1.40 2025/02/12 20:18:31 bluhm Exp $	*/
/*	$NetBSD: db_interface.c,v 1.1 2003/04/26 18:39:27 fvdl Exp $	*/
/* <LICENSES> */
/*
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
 *	db_interface.c,v 2.4 1991/02/05 17:11:13 mrt (CMU)
 */
/* </LICENSES> */

//! Interface to new debugger: `arch/amd64/amd64/db_interface.c`.
//!
//! Upstream: sys/arch/amd64/amd64/db_interface.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `ddb_regs`, `db_printtrap`, `db_ktrap` and `db_enter`
//! (ddb-lite: the trap frame is saved and `db_trap` prints where the kernel stopped). The
//! register table `db_regs[]`, `db_read_bytes`/`db_write_bytes`, `db_machine_init`, the
//! machine commands and the multiprocessor entry/exit (`db_enter_ddb`, `db_startcpu`,
//! `db_stopcpu`, `x86_ipi_db`) come with the command loop and M5.
//!
//! ## Deviations
//! - `db_ktrap` has no `db_recover` (`db_command.c`'s longjmp target) and no `splhigh`
//!   (M4-b, reported); `db_active` is the boolean of `init_main.rs`, not a counter.
//! - `db_panic` is 0 (`kern/subr_prf.rs`), so a fatal trap returns 0 from `db_ktrap` and
//!   `kerntrap` prints the trap and panics, which is the C's flow when ddb is told not to
//!   take panics.

use libkern::StaticCell;

use crate::arch::amd64::amd64::trap::{TRAP_TYPE, TRAP_TYPES};
use crate::arch::amd64::include::cpufunc::breakpoint;
use crate::arch::amd64::include::db_machdep::DbRegs;
use crate::arch::amd64::include::trap::{T_BPTFLT, T_NMI, T_TRCTRAP};
use crate::ddb::db_trap::db_trap;
use crate::dev::cons::cnpollc;
use crate::kern::init_main::DB_ACTIVE;
use crate::kern::subr_prf::{DB_PANIC, db_printf};
use crate::unported;

/// `ddb_regs`: register state. Written by `db_ktrap` on the one CPU that is in the
/// debugger, read by `db_trace` and `db_trap` while it is.
pub static DDB_REGS: StaticCell<DbRegs> = StaticCell::new(DbRegs {
    tf_rdi: 0,
    tf_rsi: 0,
    tf_rdx: 0,
    tf_r10: 0,
    tf_r8: 0,
    tf_r9: 0,
    tf_rcx: 0,
    tf_r11: 0,
    tf_r12: 0,
    tf_r13: 0,
    tf_r14: 0,
    tf_r15: 0,
    tf_err: 0,
    tf_rbx: 0,
    tf_rax: 0,
    tf_trapno: 0,
    tf_rbp: 0,
    tf_rip: 0,
    tf_cs: 0,
    tf_rflags: 0,
    tf_rsp: 0,
    tf_ss: 0,
});

/// `db_printtrap`: names the trap that brought the kernel into the debugger.
pub fn db_printtrap(type_: i32, code: i32) {
    db_printf(format_args!("kernel: "));
    if !(0..TRAP_TYPES).contains(&type_) {
        db_printf(format_args!("type {type_}"));
    } else {
        db_printf(format_args!("{}", TRAP_TYPE[type_ as usize]));
    }
    db_printf(format_args!(" trap, code={code:x}\n"));
}

/// `db_ktrap`: field a TRACE or BPT trap. Returns `true` when the debugger took the trap and
/// the kernel continues with `regs`, `false` when the caller should treat it as fatal.
pub fn db_ktrap(type_: i32, code: i32, regs: &mut DbRegs) -> bool {
    // wsdisplay_enter_ddb(): no wsdisplay.

    match type_ {
        // breakpoint, single_step, NMI, keyboard interrupt
        T_BPTFLT | T_TRCTRAP | T_NMI | -1 => {}
        _ => {
            if !DB_PANIC.load(core::sync::atomic::Ordering::Relaxed) {
                return false;
            }

            db_printtrap(type_, code);
            // db_recover != 0: db_error("Faulted in DDB; continuing...\n"): no command loop.
        }
    }

    // MULTIPROCESSOR: ddb_mp_mutex, db_enter_ddb (M5).

    let mut saved = *regs;
    saved.tf_cs &= 0xffff;
    saved.tf_ss &= 0xffff;
    // SAFETY: the one CPU entering the debugger writes ddb_regs; the readers (db_trace,
    // db_trap) run below, on this CPU, before it is written again.
    unsafe { DDB_REGS.write(saved) };

    let _ = unported!("splhigh/splx around db_trap (M4-b)");
    DB_ACTIVE.store(true, core::sync::atomic::Ordering::Relaxed);
    cnpollc(true);
    db_trap(type_, code);
    cnpollc(false);
    DB_ACTIVE.store(false, core::sync::atomic::Ordering::Relaxed);

    // SAFETY: as above; db_trap has returned and nothing else reads ddb_regs now.
    *regs = unsafe { DDB_REGS.read() };

    // MULTIPROCESSOR: ddb_state = DDB_STATE_EXITING unless db_switch_cpu (M5).

    true
}

/// `db_enter`: enters the debugger with a breakpoint instruction, which `kerntrap` hands to
/// `db_ktrap`.
pub fn db_enter() {
    breakpoint();
}
