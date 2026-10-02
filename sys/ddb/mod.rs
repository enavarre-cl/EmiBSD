//! The in-kernel debugger: OpenBSD `sys/ddb/`.
//!
//! Starts life as "ddb-lite" at M2 (panic backtrace), grows into a real `ddb(4)` later.
