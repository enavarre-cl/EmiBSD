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
//! Machine-independent kernel core: OpenBSD `sys/kern/*.c`.
//!
//! Scheduler, processes, synchronisation, VFS glue, syscalls, `printf(9)`/`panic(9)`.
//! `unported` is the project's visible-stub helper, `rust_alloc` the `GlobalAlloc` over
//! `malloc(9)` and `selftest` the boot-time checks under feature `qemu` (`ports.toml`,
//! `[[extra]]`).

pub mod clock_subr;
pub mod exec_elf;
pub mod exec_script;
pub mod exec_subr;
pub mod init_main;
pub mod init_sysent;
pub mod kern_acct;
pub mod kern_bufq;
pub mod kern_clock;
pub mod kern_clockintr;
pub mod kern_descrip;
pub mod kern_event;
pub mod kern_exec;
pub mod kern_exit;
pub mod kern_fork;
pub mod kern_intrmap;
pub mod kern_kthread;
pub mod kern_lock;
pub mod kern_malloc;
pub mod kern_physio;
pub mod kern_pledge;
pub mod kern_proc;
pub mod kern_prot;
pub mod kern_resource;
pub mod kern_rwlock;
pub mod kern_sched;
pub mod kern_sensors;
pub mod kern_sig;
pub mod kern_smr;
pub mod kern_softintr;
pub mod kern_subr;
pub mod kern_synch;
pub mod kern_sysctl;
pub mod kern_task;
pub mod kern_tc;
pub mod kern_time;
pub mod kern_timeout;
pub mod kern_unveil;
pub mod kern_watchdog;
pub mod kern_xxx;
#[cfg(feature = "alloc")]
pub mod rust_alloc;
pub mod sched_bsd;
#[cfg(feature = "qemu")]
pub mod selftest;
pub mod spec_vnops;
pub mod subr_autoconf;
pub mod subr_disk;
pub mod subr_evcount;
pub mod subr_extent;
pub mod subr_log;
pub mod subr_percpu;
pub mod subr_pool;
pub mod subr_prf;
pub mod subr_prof;
pub mod subr_tree;
#[cfg(feature = "boot_config")]
pub mod subr_userconf;
pub mod subr_xxx;
pub mod sys_futex;
pub mod sys_generic;
pub mod sys_pipe;
pub mod sys_socket;
pub mod syscalls;
pub mod tty;
pub mod tty_conf;
pub mod tty_pty;
pub mod tty_subr;
pub mod tty_tty;
pub mod uipc_domain;
pub mod uipc_mbuf;
pub mod uipc_mbuf2;
pub mod uipc_proto;
pub mod uipc_socket;
pub mod uipc_socket2;
pub mod uipc_syscalls;
pub mod uipc_usrreq;
pub mod unported;
pub mod vfs_bio;
pub mod vfs_biomem;
pub mod vfs_cache;
pub mod vfs_default;
pub mod vfs_getcwd;
pub mod vfs_init;
pub mod vfs_lockf;
pub mod vfs_lookup;
pub mod vfs_subr;
pub mod vfs_sync;
pub mod vfs_syscalls;
pub mod vfs_vnops;
pub mod vfs_vops;
/* </CODE> */
