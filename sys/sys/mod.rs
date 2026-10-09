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
//! Kernel-wide types and constants: OpenBSD `sys/sys/*.h`.
//!
//! Each header becomes one module here (`errno.h` → `errno.rs`, `proc.h` → `proc.rs`). Functions
//! that the corresponding `.c` file implements live in that file's module (`kern/`), as `impl`
//! blocks on the types defined here.

pub mod _time;
pub mod acct;
pub mod ataio;
pub mod audioio;
pub mod buf;
pub mod cdio;
pub mod clockintr;
pub mod conf;
pub mod device;
pub mod dirent;
pub mod disk;
pub mod disklabel;
pub mod dkio;
pub mod domain;
pub mod endian;
pub mod errno;
pub mod evcount;
pub mod event;
pub mod eventvar;
pub mod exec;
pub mod exec_elf;
pub mod exec_script;
pub mod extent;
pub mod fcntl;
pub mod file;
pub mod filedesc;
pub mod filio;
#[cfg(feature = "fuse")]
pub mod fusebuf;
pub mod futex;
pub mod gpio;
pub mod intrmap;
pub mod ioccom;
pub mod ioctl;
pub mod kernel;
pub mod limits;
pub mod lock;
pub mod lockf;
pub mod malloc;
pub mod mbuf;
pub mod mman;
pub mod mount;
pub mod mplock;
pub mod msgbuf;
pub mod mtio;
pub mod mutex;
pub mod namei;
pub mod param;
pub mod pclock;
pub mod percpu;
pub mod pipe;
pub mod pledge;
pub mod poll;
pub mod pool;
pub mod proc;
pub mod protosw;
pub mod queue;
pub mod reboot;
pub mod refcnt;
pub mod resource;
pub mod resourcevar;
pub mod rwlock;
pub mod sched;
pub mod scsiio;
pub mod select;
pub mod selinfo;
pub mod sensors;
pub mod siginfo;
pub mod sigio;
pub mod signal;
pub mod signalvar;
pub mod smr;
pub mod socket;
pub mod socketvar;
pub mod sockio;
pub mod softintr;
pub mod specdev;
pub mod stat;
pub mod swap;
pub mod syscall;
pub mod syscall_mi;
pub mod syscallargs;
pub mod sysctl;
pub mod syslimits;
pub mod syslog;
pub mod systm;
pub mod task;
pub mod termios;
pub mod time;
pub mod timeout;
pub mod timetc;
pub mod tprintf;
pub mod tree;
pub mod tty;
pub mod ttycom;
pub mod ttydefaults;
pub mod types;
pub mod ucred;
pub mod uio;
pub mod un;
pub mod unistd;
pub mod unpcb;
pub mod user;
pub mod uuid;
pub mod vmmeter;
pub mod vnode;
pub mod wait;
/* </CODE> */
