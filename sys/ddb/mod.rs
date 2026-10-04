//! The in-kernel debugger: OpenBSD `sys/ddb/`.
//!
//! Started life as "ddb-lite" at M2 (panic backtrace through `db_output`); M11c brings the
//! command loop (`db_command`, `db_lex`, `db_input`, `db_expr`, `db_variables`, `db_run`).
//! Most of its sources carry the Mach license (Carnegie Mellon).

pub mod db_command;
pub mod db_input;
pub mod db_lex;
pub mod db_output;
pub mod db_trap;
pub mod db_usrreq;
pub mod db_var;
