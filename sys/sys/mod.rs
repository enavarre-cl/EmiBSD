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

#[forbid(unsafe_code)]
pub mod _time;
#[forbid(unsafe_code)]
pub mod acct;
pub mod ataio;
pub mod audioio;
pub mod buf;
pub mod cdio;
pub mod clockintr;
#[forbid(unsafe_code)]
pub mod conf;
pub mod device;
#[forbid(unsafe_code)]
pub mod dirent;
pub mod disk;
pub mod disklabel;
#[forbid(unsafe_code)]
pub mod dkio;
#[forbid(unsafe_code)]
pub mod domain;
#[forbid(unsafe_code)]
pub mod endian;
#[forbid(unsafe_code)]
pub mod errno;
pub mod evcount;
pub mod event;
pub mod eventvar;
#[forbid(unsafe_code)]
pub mod exec;
#[forbid(unsafe_code)]
pub mod exec_elf;
#[forbid(unsafe_code)]
pub mod exec_script;
#[forbid(unsafe_code)]
pub mod extent;
#[forbid(unsafe_code)]
pub mod fcntl;
#[forbid(unsafe_code)]
pub mod file;
pub mod filedesc;
#[forbid(unsafe_code)]
pub mod filio;
#[cfg(feature = "fuse")]
pub mod fusebuf;
#[forbid(unsafe_code)]
pub mod futex;
pub mod gpio;
#[forbid(unsafe_code)]
pub mod intrmap;
#[forbid(unsafe_code)]
pub mod ioccom;
pub mod ioctl;
#[forbid(unsafe_code)]
pub mod kernel;
#[forbid(unsafe_code)]
pub mod limits;
#[forbid(unsafe_code)]
pub mod lock;
#[forbid(unsafe_code)]
pub mod lockf;
pub mod malloc;
pub mod mbuf;
#[forbid(unsafe_code)]
pub mod mman;
pub mod mount;
pub mod mplock;
pub mod msgbuf;
#[forbid(unsafe_code)]
pub mod mtio;
pub mod mutex;
pub mod namei;
#[forbid(unsafe_code)]
pub mod param;
#[forbid(unsafe_code)]
pub mod pclock;
pub mod percpu;
pub mod pipe;
#[forbid(unsafe_code)]
pub mod pledge;
pub mod poll;
pub mod pool;
pub mod proc;
pub mod protosw;
pub mod queue;
#[forbid(unsafe_code)]
pub mod reboot;
pub mod refcnt;
pub mod resource;
pub mod resourcevar;
pub mod rwlock;
pub mod sched;
pub mod scsiio;
#[forbid(unsafe_code)]
pub mod select;
#[forbid(unsafe_code)]
pub mod selinfo;
pub mod sensors;
pub mod siginfo;
pub mod sigio;
pub mod signal;
pub mod signalvar;
pub mod smr;
pub mod socket;
#[forbid(unsafe_code)]
pub mod socketvar;
#[forbid(unsafe_code)]
pub mod sockio;
#[forbid(unsafe_code)]
pub mod softintr;
#[forbid(unsafe_code)]
pub mod specdev;
#[forbid(unsafe_code)]
pub mod stat;
#[forbid(unsafe_code)]
pub mod swap;
#[forbid(unsafe_code)]
pub mod syscall;
pub mod syscall_mi;
pub mod syscallargs;
pub mod sysctl;
#[forbid(unsafe_code)]
pub mod syslimits;
#[forbid(unsafe_code)]
pub mod syslog;
pub mod systm;
pub mod task;
pub mod termios;
pub mod time;
pub mod timeout;
pub mod timetc;
#[forbid(unsafe_code)]
pub mod tprintf;
pub mod tree;
pub mod tty;
pub mod ttycom;
#[forbid(unsafe_code)]
pub mod ttydefaults;
#[forbid(unsafe_code)]
pub mod types;
pub mod ucred;
#[forbid(unsafe_code)]
pub mod uio;
#[forbid(unsafe_code)]
pub mod un;
#[forbid(unsafe_code)]
pub mod unistd;
pub mod unpcb;
#[forbid(unsafe_code)]
pub mod user;
#[forbid(unsafe_code)]
pub mod uuid;
#[forbid(unsafe_code)]
pub mod vmmeter;
pub mod vnode;
#[forbid(unsafe_code)]
pub mod wait;
/* </CODE> */
