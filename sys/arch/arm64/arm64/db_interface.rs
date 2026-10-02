/*	$OpenBSD: db_interface.c,v 1.17 2025/07/22 09:20:41 kettenis Exp $	*/
/*	$NetBSD: db_interface.c,v 1.34 2003/10/26 23:11:15 chris Exp $	*/

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

//! Interface to new debugger: `arch/arm64/arm64/db_interface.c`.
//!
//! Upstream: sys/arch/arm64/arm64/db_interface.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 has `db_enter` only. `db_ktrap`, `db_read_bytes`,
//! `db_write_bytes`, `db_machine_init` and the multiprocessor entry/exit arrive with the
//! exception vectors (M4).
//!
//! ## Deviations
//! - `db_enter` is a `brk #0xf000` in C, which lands in `db_ktrap` through `VBAR_EL1`. There
//!   are no exception vectors before M4, so entering the debugger is a panic with a message
//!   that says so: the kernel still stops, prints the stack trace and halts.

/// `db_enter`: enters the debugger; in ddb-lite, panics (see the module's deviations).
#[allow(clippy::panic)] // the only way to stop the kernel until db_ktrap exists (M4)
pub fn db_enter() {
    panic!("db_enter: no debugger loop yet, breakpoint traps arrive with milestone M4");
}
