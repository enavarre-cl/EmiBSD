//! Machine-independent kernel core: OpenBSD `sys/kern/*.c`.
//!
//! Scheduler, processes, synchronisation, VFS glue, syscalls, `printf(9)`/`panic(9)`.
//! `unported` is the project's visible-stub helper (`ports.toml`, `[[extra]]`).

pub mod init_main;
pub mod kern_xxx;
pub mod subr_log;
pub mod subr_prf;
pub mod subr_tree;
pub mod unported;
