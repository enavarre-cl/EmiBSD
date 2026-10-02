/*	$OpenBSD: db_interface.c,v 1.40 2025/02/12 20:18:31 bluhm Exp $	*/
/*	$NetBSD: db_interface.c,v 1.1 2003/04/26 18:39:27 fvdl Exp $	*/

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

//! Interface to new debugger: `arch/amd64/amd64/db_interface.c`.
//!
//! Upstream: sys/arch/amd64/amd64/db_interface.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 has `db_enter` only. `db_ktrap`, `db_read_bytes`,
//! `db_write_bytes`, `db_machine_init` and the multiprocessor entry/exit arrive with the trap
//! handlers (M4).
//!
//! ## Deviations
//! - `db_enter` is `breakpoint()` (`int3`) in C, which lands in `db_ktrap` through the IDT.
//!   There is no IDT before M4, so entering the debugger is a panic with a message that says so:
//!   the kernel still stops, prints the stack trace and halts, which is what "ddb-lite" offers.

/// `db_enter`: enters the debugger; in ddb-lite, panics (see the module's deviations).
#[allow(clippy::panic)] // the only way to stop the kernel until db_ktrap exists (M4)
pub fn db_enter() {
    panic!("db_enter: no debugger loop yet, breakpoint traps arrive with milestone M4");
}
