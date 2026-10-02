//! Machine-independent kernel core: OpenBSD `sys/kern/*.c`.
//!
//! Scheduler, processes, synchronisation, VFS glue, syscalls, `printf(9)`/`panic(9)`.
//! `unported` is the project's visible-stub helper, `rust_alloc` the `GlobalAlloc` over
//! `malloc(9)` and `selftest` the boot-time checks under feature `qemu` (`ports.toml`,
//! `[[extra]]`).

pub mod init_main;
pub mod kern_lock;
pub mod kern_malloc;
pub mod kern_softintr;
pub mod kern_synch;
pub mod kern_xxx;
#[cfg(feature = "alloc")]
pub mod rust_alloc;
#[cfg(feature = "qemu")]
pub mod selftest;
pub mod subr_evcount;
pub mod subr_log;
pub mod subr_pool;
pub mod subr_prf;
pub mod subr_tree;
pub mod unported;
