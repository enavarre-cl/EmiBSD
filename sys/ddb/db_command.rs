/*	$OpenBSD: db_command.c,v 1.104 2026/02/02 15:20:51 claudio Exp $	*/
/*	$NetBSD: db_command.c,v 1.20 1996/03/30 22:30:05 christos Exp $	*/
/*	$OpenBSD: db_command.h,v 1.35 2022/04/14 19:47:12 naddy Exp $	*/
/*	$NetBSD: db_command.h,v 1.8 1996/02/05 01:56:55 christos Exp $	*/
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
 */
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
 *	Author: David B. Golub, Carnegie Mellon University
 *	Date:	7/90
 */
/* </LICENSES> */

//! Command dispatcher of the debugger: `ddb/db_command.c` and `<ddb/db_command.h>`.
//!
//! Upstream: sys/ddb/db_command.c @ 3ce1f3f79392
//! Upstream: sys/ddb/db_command.h @ 3ce1f3f79392
//!
//! Status: `wip`. This step brings `db_error` and `db_skip_to_eol`, which the lexer needs;
//! the command tables and `db_command_loop` follow.
//!
//! ## Deviations
//! - `db_error` does not `longjmp` to `db_recover`: it returns a [`DbError`], which every
//!   function that can reach it propagates as the `Err` of a [`DbResult`] up to
//!   `db_command_loop`, where the C's `setjmp` was (`docs/C_TO_RUST.md`).

use crate::ddb::db_lex::{db_flush_lex, db_read_token, tEOL};
use crate::kern::subr_prf::db_printf;

/// "`db_error` was called": its message is printed and the lexer flushed; the command is
/// abandoned and control goes back to the command loop (C's `longjmp(db_recover)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DbError;

/// The result of a debugger function that can reach `db_error`.
pub type DbResult<T = ()> = Result<T, DbError>;

/// `db_skip_to_eol`: utility routine - discard tokens through end-of-line.
pub fn db_skip_to_eol() -> DbResult {
    loop {
        if db_read_token()? == tEOL {
            return Ok(());
        }
    }
}

/// `db_error`: prints `s` (if any), flushes the lexer and returns the [`DbError`] the caller
/// propagates back to the command loop: `return Err(db_error(Some("...")))`.
pub fn db_error(s: Option<&str>) -> DbError {
    if let Some(s) = s {
        db_printf(format_args!("{s}"));
    }
    db_flush_lex();
    DbError
}
