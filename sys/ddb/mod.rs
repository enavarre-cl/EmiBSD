/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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
/* </LICENSES> */

/* <CODE> */
//! The in-kernel debugger: OpenBSD `sys/ddb/`.
//!
//! Started life as "ddb-lite" at M2 (panic backtrace through `db_output`); M11c brings the
//! command loop (`db_command`, `db_lex`, `db_input`, `db_expr`, `db_variables`, `db_run`).
//! Most of its sources carry the Mach license (Carnegie Mellon).

pub mod db_command;
pub mod db_expr;
pub mod db_input;
pub mod db_lex;
pub mod db_output;
pub mod db_run;
pub mod db_trap;
pub mod db_usrreq;
pub mod db_var;
pub mod db_variables;
/* </CODE> */
