//! The in-kernel debugger: OpenBSD `sys/ddb/`.
//!
//! Starts life as "ddb-lite" at M2 (panic backtrace through `db_output`), grows into a real
//! `ddb(4)` later. Its sources carry the Mach license (Carnegie Mellon).

pub mod db_output;
pub mod db_trap;
pub mod db_usrreq;
