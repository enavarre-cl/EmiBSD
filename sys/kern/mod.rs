//! Machine-independent kernel core: OpenBSD `sys/kern/*.c`.
//!
//! Scheduler, processes, synchronisation, VFS glue, syscalls, `printf(9)`/`panic(9)`.

pub mod subr_tree;
