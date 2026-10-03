/*	$OpenBSD: sysctl.h,v 1.248 2026/04/16 14:47:24 deraadt Exp $	*/
/*	$NetBSD: sysctl.h,v 1.16 1996/04/09 20:55:36 cgd Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Mike Karels at Berkeley Software Design, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)sysctl.h	8.2 (Berkeley) 3/30/95
 */
/* </LICENSES> */

//! Definitions for the `sysctl(2)` call: `<sys/sysctl.h>`. The name of an object is a sequence of
//! integers, read like a path: the meaning of each component depends on its place in the
//! hierarchy. The top-level, `kern` and `hw` identifiers are here; the others are in their
//! subsystems' headers (`VM_*` in `uvm/uvmexp.rs`).
//!
//! Upstream: sys/sys/sysctl.h @ 3ce1f3f79392
//!
//! The `CTL_*_NAMES` tables are what `sysctl(8)` prints; the kernel does not read them. They are
//! kept so the header is complete, and the compile-time checks tie their lengths to the ids.
//!
//! ## Deviations
//! - `struct ctlname`'s `char *ctl_name` is `Option<&'static [u8]>`: `None` is the C's
//!   `{ 0, 0 }` first entry.
//! - `struct kinfo_proc` has an explicit `p_pad0` where the C compiler inserts four bytes of
//!   padding before `p_uru_maxrss`; the layout is the C's (checked at compile time), and the
//!   structure has no uninitialised bytes, so it can be copied out as bytes ([`SysctlPlain`]).
//! - `FILL_KPROC`, `_getcompatprio`, `PTRTOINT64` and `PR_LOCK`/`PR_UNLOCK` are expanded where
//!   their one kernel user is, `fill_kproc` in `kern/kern_sysctl.rs`: a macro over a dozen
//!   pointers is a function body in Rust.
//! - `struct ctldebug` exists only under `DEBUG_SYSCTL`, which is not configured.
//! - `int *var` of `struct sysctl_bounded_args` is `&'static AtomicI32`: the helpers read and
//!   write it with the C's atomic operations. `SYSCTL_INT_READONLY` (the C's `1,0` pair) is a
//!   tuple, and [`SysctlBoundedArgs::readonly`] builds an entry with it.
//! - The `sysctlfn` prototype is [`Sysctlfn`]: a name slice instead of `int *` plus `u_int`,
//!   user addresses as `usize` for `oldp`/`newp`, and `Result<(), Errno>`. The other
//!   prototypes are the functions in `kern/kern_sysctl.rs` and their subsystems.
//! - [`SysctlPlain`] is not OpenBSD's: it marks the structures `sysctl_rdstruct` and
//!   `sysctl_struct` may copy as bytes, which C does with a `void *` and a size.

use core::sync::atomic::AtomicI32;

use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbstat;
use crate::sys::proc::Proc;
use crate::sys::sched::Cpustats;
use crate::sys::syslimits::_MAXCOMLEN;
use crate::sys::time::{Clockinfo, Timeval};

/// `CTL_MAXNAME`: largest number of components supported.
pub const CTL_MAXNAME: usize = 12;

/// `struct ctlname`: the name and type of one level of the hierarchy, for `sysctl(8)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ctlname {
    /// `ctl_name`: subsystem name; `None` for the unused slot 0.
    pub ctl_name: Option<&'static [u8]>,
    /// `ctl_type`: type of name (`CTLTYPE_*`).
    pub ctl_type: i32,
}

impl Ctlname {
    /// `{ 0, 0 }`.
    pub const NONE: Self = Self {
        ctl_name: None,
        ctl_type: 0,
    };

    /// `{ name, type }`.
    pub const fn new(name: &'static [u8], ctl_type: i32) -> Self {
        Self {
            ctl_name: Some(name),
            ctl_type,
        }
    }
}

/// `CTLTYPE_NODE`: name is a node.
pub const CTLTYPE_NODE: i32 = 1;
/// `CTLTYPE_INT`: name describes an integer.
pub const CTLTYPE_INT: i32 = 2;
/// `CTLTYPE_STRING`: name describes a string.
pub const CTLTYPE_STRING: i32 = 3;
/// `CTLTYPE_QUAD`: name describes a 64-bit number.
pub const CTLTYPE_QUAD: i32 = 4;
/// `CTLTYPE_STRUCT`: name describes a structure.
pub const CTLTYPE_STRUCT: i32 = 5;

/// `CTL_UNSPEC`: unused.
pub const CTL_UNSPEC: i32 = 0;
/// `CTL_KERN`: "high kernel": proc, limits.
pub const CTL_KERN: i32 = 1;
/// `CTL_VM`: virtual memory.
pub const CTL_VM: i32 = 2;
// gap for CTL_FS 3
/// `CTL_NET`: network, see `socket.h`.
pub const CTL_NET: i32 = 4;
/// `CTL_DEBUG`: debugging parameters.
pub const CTL_DEBUG: i32 = 5;
/// `CTL_HW`: generic cpu/io.
pub const CTL_HW: i32 = 6;
/// `CTL_MACHDEP`: machine dependent.
pub const CTL_MACHDEP: i32 = 7;
// gap 8, was CTL_USER: removed 2013-04
/// `CTL_DDB`: DDB user interface, see `db_var.h`.
pub const CTL_DDB: i32 = 9;
/// `CTL_VFS`: VFS sysctl's.
pub const CTL_VFS: i32 = 10;
/// `CTL_MAXID`: number of valid top-level ids.
pub const CTL_MAXID: usize = 11;

/// `CTL_NAMES`.
pub const CTL_NAMES: [Ctlname; CTL_MAXID] = [
    Ctlname::NONE,
    Ctlname::new(b"kern", CTLTYPE_NODE),
    Ctlname::new(b"vm", CTLTYPE_NODE),
    Ctlname::new(b"gap", 0),
    Ctlname::new(b"net", CTLTYPE_NODE),
    Ctlname::new(b"debug", CTLTYPE_NODE),
    Ctlname::new(b"hw", CTLTYPE_NODE),
    Ctlname::new(b"machdep", CTLTYPE_NODE),
    Ctlname::new(b"gap", 0),
    Ctlname::new(b"ddb", CTLTYPE_NODE),
    Ctlname::new(b"vfs", CTLTYPE_NODE),
];

/// `KERN_OSTYPE`: string: system version.
pub const KERN_OSTYPE: i32 = 1;
/// `KERN_OSRELEASE`: string: system release.
pub const KERN_OSRELEASE: i32 = 2;
/// `KERN_OSREV`: int: system revision.
pub const KERN_OSREV: i32 = 3;
/// `KERN_VERSION`: string: compile time info.
pub const KERN_VERSION: i32 = 4;
/// `KERN_MAXVNODES`: int: max vnodes.
pub const KERN_MAXVNODES: i32 = 5;
/// `KERN_MAXPROC`: int: max processes.
pub const KERN_MAXPROC: i32 = 6;
/// `KERN_MAXFILES`: int: max open files.
pub const KERN_MAXFILES: i32 = 7;
/// `KERN_ARGMAX`: int: max arguments to exec.
pub const KERN_ARGMAX: i32 = 8;
/// `KERN_SECURELVL`: int: system security level.
pub const KERN_SECURELVL: i32 = 9;
/// `KERN_HOSTNAME`: string: hostname.
pub const KERN_HOSTNAME: i32 = 10;
/// `KERN_HOSTID`: int: host identifier.
pub const KERN_HOSTID: i32 = 11;
/// `KERN_CLOCKRATE`: struct: struct clockinfo.
pub const KERN_CLOCKRATE: i32 = 12;
// was KERN_DNSJACKPORT 13, KERN_PROC 14, KERN_FILE 15
/// `KERN_PROF`: node: kernel profiling info.
pub const KERN_PROF: i32 = 16;
/// `KERN_POSIX1`: int: POSIX.1 version.
pub const KERN_POSIX1: i32 = 17;
/// `KERN_NGROUPS`: int: # of supplemental group ids.
pub const KERN_NGROUPS: i32 = 18;
/// `KERN_JOB_CONTROL`: int: is job control available.
pub const KERN_JOB_CONTROL: i32 = 19;
/// `KERN_SAVED_IDS`: int: saved set-user/group-ID.
pub const KERN_SAVED_IDS: i32 = 20;
/// `KERN_BOOTTIME`: struct: time kernel was booted.
pub const KERN_BOOTTIME: i32 = 21;
/// `KERN_DOMAINNAME`: string: (YP) domainname.
pub const KERN_DOMAINNAME: i32 = 22;
/// `KERN_MAXPARTITIONS`: int: number of partitions/disk.
pub const KERN_MAXPARTITIONS: i32 = 23;
/// `KERN_RAWPARTITION`: int: raw partition number.
pub const KERN_RAWPARTITION: i32 = 24;
/// `KERN_MAXTHREAD`: int: max threads.
pub const KERN_MAXTHREAD: i32 = 25;
/// `KERN_NTHREADS`: int: number of threads.
pub const KERN_NTHREADS: i32 = 26;
/// `KERN_OSVERSION`: string: kernel build version.
pub const KERN_OSVERSION: i32 = 27;
/// `KERN_SOMAXCONN`: int: listen queue maximum.
pub const KERN_SOMAXCONN: i32 = 28;
/// `KERN_SOMINCONN`: int: half-open controllable param.
pub const KERN_SOMINCONN: i32 = 29;
// was KERN_USERMOUNT 30, KERN_RND 31
/// `KERN_NOSUIDCOREDUMP`: int: no setuid coredumps ever.
pub const KERN_NOSUIDCOREDUMP: i32 = 32;
/// `KERN_FSYNC`: int: file synchronization support.
pub const KERN_FSYNC: i32 = 33;
/// `KERN_SYSVMSG`: int: SysV message queue support.
pub const KERN_SYSVMSG: i32 = 34;
/// `KERN_SYSVSEM`: int: SysV semaphore support.
pub const KERN_SYSVSEM: i32 = 35;
/// `KERN_SYSVSHM`: int: SysV shared memory support.
pub const KERN_SYSVSHM: i32 = 36;
// was KERN_ARND 37
/// `KERN_MSGBUFSIZE`: int: size of message buffer.
pub const KERN_MSGBUFSIZE: i32 = 38;
/// `KERN_MALLOCSTATS`: node: malloc statistics.
pub const KERN_MALLOCSTATS: i32 = 39;
/// `KERN_CPTIME`: array: cp_time.
pub const KERN_CPTIME: i32 = 40;
/// `KERN_NCHSTATS`: struct: vfs cache statistics.
pub const KERN_NCHSTATS: i32 = 41;
/// `KERN_FORKSTAT`: struct: fork statistics.
pub const KERN_FORKSTAT: i32 = 42;
// was KERN_NSELCOLL 43
/// `KERN_TTY`: node: tty information.
pub const KERN_TTY: i32 = 44;
/// `KERN_CCPU`: int: ccpu.
pub const KERN_CCPU: i32 = 45;
/// `KERN_FSCALE`: int: fscale.
pub const KERN_FSCALE: i32 = 46;
/// `KERN_NPROCS`: int: number of processes.
pub const KERN_NPROCS: i32 = 47;
/// `KERN_MSGBUF`: message buffer, `KERN_MSGBUFSIZE`.
pub const KERN_MSGBUF: i32 = 48;
/// `KERN_POOL`: struct: pool information.
pub const KERN_POOL: i32 = 49;
/// `KERN_STACKGAPRANDOM`: int: stackgap_random.
pub const KERN_STACKGAPRANDOM: i32 = 50;
/// `KERN_SYSVIPC_INFO`: struct: SysV sem/shm/msg info.
pub const KERN_SYSVIPC_INFO: i32 = 51;
/// `KERN_ALLOWKMEM`: int: allowkmem.
pub const KERN_ALLOWKMEM: i32 = 52;
/// `KERN_WITNESSWATCH`: int: witnesswatch.
pub const KERN_WITNESSWATCH: i32 = 53;
/// `KERN_SPLASSERT`: int: splassert.
pub const KERN_SPLASSERT: i32 = 54;
/// `KERN_PROC_ARGS`: node: proc args and env.
pub const KERN_PROC_ARGS: i32 = 55;
/// `KERN_NFILES`: int: number of open files.
pub const KERN_NFILES: i32 = 56;
/// `KERN_TTYCOUNT`: int: number of tty devices.
pub const KERN_TTYCOUNT: i32 = 57;
/// `KERN_NUMVNODES`: int: number of vnodes in use.
pub const KERN_NUMVNODES: i32 = 58;
/// `KERN_MBSTAT`: struct: mbuf statistics.
pub const KERN_MBSTAT: i32 = 59;
/// `KERN_WITNESS`: node: witness.
pub const KERN_WITNESS: i32 = 60;
/// `KERN_SEMINFO`: struct: SysV struct seminfo.
pub const KERN_SEMINFO: i32 = 61;
/// `KERN_SHMINFO`: struct: SysV struct shminfo.
pub const KERN_SHMINFO: i32 = 62;
/// `KERN_INTRCNT`: node: interrupt counters.
pub const KERN_INTRCNT: i32 = 63;
/// `KERN_WATCHDOG`: node: watchdog.
pub const KERN_WATCHDOG: i32 = 64;
/// `KERN_ALLOWDT`: int: allowdt.
pub const KERN_ALLOWDT: i32 = 65;
/// `KERN_PROC`: struct: process entries.
pub const KERN_PROC: i32 = 66;
/// `KERN_MAXCLUSTERS`: number of mclusters.
pub const KERN_MAXCLUSTERS: i32 = 67;
/// `KERN_EVCOUNT`: node: event counters.
pub const KERN_EVCOUNT: i32 = 68;
/// `KERN_TIMECOUNTER`: node: timecounter.
pub const KERN_TIMECOUNTER: i32 = 69;
/// `KERN_MAXLOCKSPERUID`: int: locks per uid.
pub const KERN_MAXLOCKSPERUID: i32 = 70;
/// `KERN_CPTIME2`: array: cp_time2.
pub const KERN_CPTIME2: i32 = 71;
/// `KERN_CACHEPCT`: buffer cache % of physmem.
pub const KERN_CACHEPCT: i32 = 72;
/// `KERN_FILE`: struct: file entries.
pub const KERN_FILE: i32 = 73;
/// `KERN_WXABORT`: int: w^x sigabrt & core.
pub const KERN_WXABORT: i32 = 74;
/// `KERN_CONSDEV`: dev_t: console terminal device.
pub const KERN_CONSDEV: i32 = 75;
/// `KERN_NETLIVELOCKS`: int: number of network livelocks.
pub const KERN_NETLIVELOCKS: i32 = 76;
/// `KERN_POOL_DEBUG`: int: enable pool_debug.
pub const KERN_POOL_DEBUG: i32 = 77;
/// `KERN_PROC_CWD`: node: proc cwd.
pub const KERN_PROC_CWD: i32 = 78;
/// `KERN_PROC_NOBROADCASTKILL`: node: proc no broadcast kill.
pub const KERN_PROC_NOBROADCASTKILL: i32 = 79;
/// `KERN_PROC_VMMAP`: node: proc vmmap.
pub const KERN_PROC_VMMAP: i32 = 80;
/// `KERN_GLOBAL_PTRACE`: allow ptrace globally.
pub const KERN_GLOBAL_PTRACE: i32 = 81;
/// `KERN_CONSBUFSIZE`: int: console message buffer size.
pub const KERN_CONSBUFSIZE: i32 = 82;
/// `KERN_CONSBUF`: console message buffer.
pub const KERN_CONSBUF: i32 = 83;
/// `KERN_AUDIO`: struct: audio properties.
pub const KERN_AUDIO: i32 = 84;
/// `KERN_CPUSTATS`: struct: cpu statistics.
pub const KERN_CPUSTATS: i32 = 85;
/// `KERN_PFSTATUS`: struct: pf status and stats.
pub const KERN_PFSTATUS: i32 = 86;
/// `KERN_TIMEOUT_STATS`: struct: timeout status and stats.
pub const KERN_TIMEOUT_STATS: i32 = 87;
/// `KERN_UTC_OFFSET`: int: adjust RTC time to UTC.
pub const KERN_UTC_OFFSET: i32 = 88;
/// `KERN_VIDEO`: struct: video properties.
pub const KERN_VIDEO: i32 = 89;
/// `KERN_CLOCKINTR`: node: clockintr.
pub const KERN_CLOCKINTR: i32 = 90;
/// `KERN_AUTOCONF_SERIAL`: int: kernel device tree state serial.
pub const KERN_AUTOCONF_SERIAL: i32 = 91;
/// `KERN_MAXID`: number of valid kern ids.
pub const KERN_MAXID: usize = 92;

/// `CTL_KERN_NAMES`.
pub const CTL_KERN_NAMES: [Ctlname; KERN_MAXID] = [
    Ctlname::NONE,
    Ctlname::new(b"ostype", CTLTYPE_STRING),
    Ctlname::new(b"osrelease", CTLTYPE_STRING),
    Ctlname::new(b"osrevision", CTLTYPE_INT),
    Ctlname::new(b"version", CTLTYPE_STRING),
    Ctlname::new(b"maxvnodes", CTLTYPE_INT),
    Ctlname::new(b"maxproc", CTLTYPE_INT),
    Ctlname::new(b"maxfiles", CTLTYPE_INT),
    Ctlname::new(b"argmax", CTLTYPE_INT),
    Ctlname::new(b"securelevel", CTLTYPE_INT),
    Ctlname::new(b"hostname", CTLTYPE_STRING),
    Ctlname::new(b"hostid", CTLTYPE_INT),
    Ctlname::new(b"clockrate", CTLTYPE_STRUCT),
    Ctlname::new(b"gap", 0),
    Ctlname::new(b"gap", 0),
    Ctlname::new(b"gap", 0),
    Ctlname::new(b"profiling", CTLTYPE_NODE),
    Ctlname::new(b"posix1version", CTLTYPE_INT),
    Ctlname::new(b"ngroups", CTLTYPE_INT),
    Ctlname::new(b"job_control", CTLTYPE_INT),
    Ctlname::new(b"saved_ids", CTLTYPE_INT),
    Ctlname::new(b"boottime", CTLTYPE_STRUCT),
    Ctlname::new(b"domainname", CTLTYPE_STRING),
    Ctlname::new(b"maxpartitions", CTLTYPE_INT),
    Ctlname::new(b"rawpartition", CTLTYPE_INT),
    Ctlname::new(b"maxthread", CTLTYPE_INT),
    Ctlname::new(b"nthreads", CTLTYPE_INT),
    Ctlname::new(b"osversion", CTLTYPE_STRING),
    Ctlname::new(b"somaxconn", CTLTYPE_INT),
    Ctlname::new(b"sominconn", CTLTYPE_INT),
    Ctlname::new(b"gap", 0),
    Ctlname::new(b"gap", 0),
    Ctlname::new(b"nosuidcoredump", CTLTYPE_INT),
    Ctlname::new(b"fsync", CTLTYPE_INT),
    Ctlname::new(b"sysvmsg", CTLTYPE_INT),
    Ctlname::new(b"sysvsem", CTLTYPE_INT),
    Ctlname::new(b"sysvshm", CTLTYPE_INT),
    Ctlname::new(b"gap", 0),
    Ctlname::new(b"msgbufsize", CTLTYPE_INT),
    Ctlname::new(b"malloc", CTLTYPE_NODE),
    Ctlname::new(b"cp_time", CTLTYPE_STRUCT),
    Ctlname::new(b"nchstats", CTLTYPE_STRUCT),
    Ctlname::new(b"forkstat", CTLTYPE_STRUCT),
    Ctlname::new(b"gap", 0),
    Ctlname::new(b"tty", CTLTYPE_NODE),
    Ctlname::new(b"ccpu", CTLTYPE_INT),
    Ctlname::new(b"fscale", CTLTYPE_INT),
    Ctlname::new(b"nprocs", CTLTYPE_INT),
    Ctlname::new(b"msgbuf", CTLTYPE_STRUCT),
    Ctlname::new(b"pool", CTLTYPE_NODE),
    Ctlname::new(b"stackgap_random", CTLTYPE_INT),
    Ctlname::new(b"sysvipc_info", CTLTYPE_INT),
    Ctlname::new(b"allowkmem", CTLTYPE_INT),
    Ctlname::new(b"witnesswatch", CTLTYPE_INT),
    Ctlname::new(b"splassert", CTLTYPE_INT),
    Ctlname::new(b"procargs", CTLTYPE_NODE),
    Ctlname::new(b"nfiles", CTLTYPE_INT),
    Ctlname::new(b"ttycount", CTLTYPE_INT),
    Ctlname::new(b"numvnodes", CTLTYPE_INT),
    Ctlname::new(b"mbstat", CTLTYPE_STRUCT),
    Ctlname::new(b"witness", CTLTYPE_NODE),
    Ctlname::new(b"seminfo", CTLTYPE_STRUCT),
    Ctlname::new(b"shminfo", CTLTYPE_STRUCT),
    Ctlname::new(b"intrcnt", CTLTYPE_NODE),
    Ctlname::new(b"watchdog", CTLTYPE_NODE),
    Ctlname::new(b"allowdt", CTLTYPE_INT),
    Ctlname::new(b"proc", CTLTYPE_STRUCT),
    Ctlname::new(b"maxclusters", CTLTYPE_INT),
    Ctlname::new(b"evcount", CTLTYPE_NODE),
    Ctlname::new(b"timecounter", CTLTYPE_NODE),
    Ctlname::new(b"maxlocksperuid", CTLTYPE_INT),
    Ctlname::new(b"cp_time2", CTLTYPE_STRUCT),
    Ctlname::new(b"bufcachepercent", CTLTYPE_INT),
    Ctlname::new(b"file", CTLTYPE_STRUCT),
    Ctlname::new(b"wxabort", CTLTYPE_INT),
    Ctlname::new(b"consdev", CTLTYPE_STRUCT),
    Ctlname::new(b"netlivelocks", CTLTYPE_INT),
    Ctlname::new(b"pool_debug", CTLTYPE_INT),
    Ctlname::new(b"proc_cwd", CTLTYPE_NODE),
    Ctlname::new(b"proc_nobroadcastkill", CTLTYPE_NODE),
    Ctlname::new(b"proc_vmmap", CTLTYPE_NODE),
    Ctlname::new(b"global_ptrace", CTLTYPE_INT),
    Ctlname::new(b"consbufsize", CTLTYPE_INT),
    Ctlname::new(b"consbuf", CTLTYPE_STRUCT),
    Ctlname::new(b"audio", CTLTYPE_STRUCT),
    Ctlname::new(b"cpustats", CTLTYPE_STRUCT),
    Ctlname::new(b"pfstatus", CTLTYPE_STRUCT),
    Ctlname::new(b"timeout_stats", CTLTYPE_STRUCT),
    Ctlname::new(b"utc_offset", CTLTYPE_INT),
    Ctlname::new(b"video", CTLTYPE_STRUCT),
    Ctlname::new(b"clockintr", CTLTYPE_NODE),
    Ctlname::new(b"autoconf_serial", CTLTYPE_INT),
];

/// `KERN_PROC_ALL`: everything but kernel threads.
pub const KERN_PROC_ALL: i32 = 0;
/// `KERN_PROC_PID`: by process id.
pub const KERN_PROC_PID: i32 = 1;
/// `KERN_PROC_PGRP`: by process group id.
pub const KERN_PROC_PGRP: i32 = 2;
/// `KERN_PROC_SESSION`: by session of pid.
pub const KERN_PROC_SESSION: i32 = 3;
/// `KERN_PROC_TTY`: by controlling tty.
pub const KERN_PROC_TTY: i32 = 4;
/// `KERN_PROC_UID`: by effective uid.
pub const KERN_PROC_UID: i32 = 5;
/// `KERN_PROC_RUID`: by real uid.
pub const KERN_PROC_RUID: i32 = 6;
/// `KERN_PROC_KTHREAD`: also return kernel threads.
pub const KERN_PROC_KTHREAD: i32 = 7;
/// `KERN_PROC_SHOW_THREADS`: also return normal threads.
pub const KERN_PROC_SHOW_THREADS: i32 = 0x4000_0000;

/// `KERN_SYSVIPC_MSG_INFO`: msginfo and msqid_ds.
pub const KERN_SYSVIPC_MSG_INFO: i32 = 1;
/// `KERN_SYSVIPC_SEM_INFO`: seminfo and semid_ds.
pub const KERN_SYSVIPC_SEM_INFO: i32 = 2;
/// `KERN_SYSVIPC_SHM_INFO`: shminfo and shmid_ds.
pub const KERN_SYSVIPC_SHM_INFO: i32 = 3;

/// `KERN_PROC_ARGV`.
pub const KERN_PROC_ARGV: i32 = 1;
/// `KERN_PROC_NARGV`.
pub const KERN_PROC_NARGV: i32 = 2;
/// `KERN_PROC_ENV`.
pub const KERN_PROC_ENV: i32 = 3;
/// `KERN_PROC_NENV`.
pub const KERN_PROC_NENV: i32 = 4;

/// `KERN_AUDIO_RECORD`.
pub const KERN_AUDIO_RECORD: i32 = 1;
/// `KERN_AUDIO_KBDCONTROL`.
pub const KERN_AUDIO_KBDCONTROL: i32 = 2;
/// `KERN_AUDIO_MAXID`.
pub const KERN_AUDIO_MAXID: usize = 3;

/// `CTL_KERN_AUDIO_NAMES`.
pub const CTL_KERN_AUDIO_NAMES: [Ctlname; KERN_AUDIO_MAXID] = [
    Ctlname::NONE,
    Ctlname::new(b"record", CTLTYPE_INT),
    Ctlname::new(b"kbdcontrol", CTLTYPE_INT),
];

/// `KERN_VIDEO_RECORD`.
pub const KERN_VIDEO_RECORD: i32 = 1;
/// `KERN_VIDEO_MAXID`.
pub const KERN_VIDEO_MAXID: usize = 2;

/// `CTL_KERN_VIDEO_NAMES`.
pub const CTL_KERN_VIDEO_NAMES: [Ctlname; KERN_VIDEO_MAXID] =
    [Ctlname::NONE, Ctlname::new(b"record", CTLTYPE_INT)];

/// `KERN_WITNESS_WATCH`: int: operating mode.
pub const KERN_WITNESS_WATCH: i32 = 1;
/// `KERN_WITNESS_LOCKTRACE`: int: stack trace saving mode.
pub const KERN_WITNESS_LOCKTRACE: i32 = 2;
/// `KERN_WITNESS_MAXID`.
pub const KERN_WITNESS_MAXID: usize = 3;

/// `CTL_KERN_WITNESS_NAMES`.
pub const CTL_KERN_WITNESS_NAMES: [Ctlname; KERN_WITNESS_MAXID] = [
    Ctlname::NONE,
    Ctlname::new(b"watch", CTLTYPE_INT),
    Ctlname::new(b"locktrace", CTLTYPE_INT),
];

/// `KI_NGROUPS`.
pub const KI_NGROUPS: usize = 16;
/// `KI_MAXCOMLEN`: includes NUL.
pub const KI_MAXCOMLEN: usize = _MAXCOMLEN;
/// `KI_WMESGLEN`.
pub const KI_WMESGLEN: usize = 8;
/// `KI_MAXLOGNAME`.
pub const KI_MAXLOGNAME: usize = 32;
/// `KI_EMULNAMELEN`.
pub const KI_EMULNAMELEN: usize = 8;

/// `KI_NOCPU`.
pub const KI_NOCPU: u64 = !0;

/// `struct kinfo_proc`: what the `KERN_PROC` subtypes return, one per process (or thread).
/// Relatively fixed size, 8 byte alignment, new members only at the end, for binary
/// compatibility. It is ABI (`ps(1)`, `top(1)`, `libkvm`): `#[repr(C)]`, and the layout is
/// checked at compile time.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(non_snake_case)] // `p__pgid`: the C's name, double underscore and all
pub struct KinfoProc {
    /// PTR: linked run/sleep queue.
    pub p_forw: u64,
    /// PTR.
    pub p_back: u64,
    /// PTR: address of proc.
    pub p_paddr: u64,

    /// PTR: Kernel virtual addr of u-area.
    pub p_addr: u64,
    /// PTR: Ptr to open files structure.
    pub p_fd: u64,
    /// Unused, always zero.
    pub p_stats: u64,
    /// PTR: Process limits.
    pub p_limit: u64,
    /// PTR: Address space.
    pub p_vmspace: u64,
    /// PTR: Signal actions, state.
    pub p_sigacts: u64,
    /// PTR: session pointer.
    pub p_sess: u64,
    /// PTR: tty session pointer.
    pub p_tsess: u64,
    /// PTR: Exit information. XXX
    pub p_ru: u64,

    /// LONG: extra kinfo_proc flags (`EPROC_*`).
    pub p_eflag: i32,
    /// Unused, always zero.
    pub p_exitsig: i32,
    /// INT: `P_*` flags.
    pub p_flag: i32,

    /// PID_T: Process identifier.
    pub p_pid: i32,
    /// PID_T: Parent process id.
    pub p_ppid: i32,
    /// PID_T: session id.
    pub p_sid: i32,
    /// PID_T: process group id (`<sys/proc.h>` hijacks `p_pgid`).
    pub p__pgid: i32,
    /// PID_T: tty process group id.
    pub p_tpgid: i32,

    /// UID_T: effective user id.
    pub p_uid: u32,
    /// UID_T: real user id.
    pub p_ruid: u32,
    /// GID_T: effective group id.
    pub p_gid: u32,
    /// GID_T: real group id.
    pub p_rgid: u32,

    /// GID_T: groups.
    pub p_groups: [u32; KI_NGROUPS],
    /// SHORT: number of groups.
    pub p_ngroups: i16,

    /// SHORT: job control counter.
    pub p_jobc: i16,
    /// DEV_T: controlling tty dev.
    pub p_tdev: u32,

    /// U_INT: Time averaged value of `p_cpticks`.
    pub p_estcpu: u32,
    /// STRUCT TIMEVAL: Real time.
    pub p_rtime_sec: u32,
    /// STRUCT TIMEVAL: Real time.
    pub p_rtime_usec: u32,
    /// INT: Ticks of cpu time.
    pub p_cpticks: i32,
    /// FIXPT_T: %cpu for this process.
    pub p_pctcpu: u32,
    /// Unused, always zero.
    pub p_swtime: u32,
    /// U_INT: Time since last blocked.
    pub p_slptime: u32,
    /// INT: `PSCHED_*` flags.
    pub p_schedflags: i32,

    /// U_QUAD_T: Statclock hits in user mode.
    pub p_uticks: u64,
    /// U_QUAD_T: Statclock hits in system mode.
    pub p_sticks: u64,
    /// U_QUAD_T: Statclock hits processing intr.
    pub p_iticks: u64,

    /// PTR: Trace to vnode or file.
    pub p_tracep: u64,
    /// INT: Kernel trace points.
    pub p_traceflag: i32,

    /// INT: If non-zero, don't swap.
    pub p_holdcnt: i32,

    /// INT: Signals arrived but not delivered.
    pub p_siglist: i32,
    /// SIGSET_T: Current signal mask.
    pub p_sigmask: u32,
    /// SIGSET_T: Signals being ignored.
    pub p_sigignore: u32,
    /// SIGSET_T: Signals being caught by user.
    pub p_sigcatch: u32,

    /// CHAR: S* process status (from LWP).
    pub p_stat: i8,
    /// U_CHAR: Process priority.
    pub p_priority: u8,
    /// U_CHAR: User-priority based on `p_estcpu` and `ps_nice`.
    pub p_usrpri: u8,
    /// U_CHAR: Process "nice" value.
    pub p_nice: u8,

    /// U_SHORT: Exit status for wait; also stop signal.
    pub p_xstat: u16,
    /// U_SHORT: unused.
    pub p_spare: u16,

    /// Command name.
    pub p_comm: [u8; KI_MAXCOMLEN],

    /// wchan message.
    pub p_wmesg: [u8; KI_WMESGLEN],
    /// PTR: sleep address.
    pub p_wchan: u64,

    /// `setlogin()` name.
    pub p_login: [u8; KI_MAXLOGNAME],

    /// SEGSZ_T: current resident set size in pages.
    pub p_vm_rssize: i32,
    /// SEGSZ_T: text size (pages).
    pub p_vm_tsize: i32,
    /// SEGSZ_T: data size (pages).
    pub p_vm_dsize: i32,
    /// SEGSZ_T: stack size (pages).
    pub p_vm_ssize: i32,

    /// CHAR: following `p_u*` members from struct user are valid (64 bits for alignment).
    pub p_uvalid: i64,
    /// STRUCT TIMEVAL: starting time.
    pub p_ustart_sec: u64,
    /// STRUCT TIMEVAL: starting time.
    pub p_ustart_usec: u32,

    /// STRUCT TIMEVAL: user time.
    pub p_uutime_sec: u32,
    /// STRUCT TIMEVAL: user time.
    pub p_uutime_usec: u32,
    /// STRUCT TIMEVAL: system time.
    pub p_ustime_sec: u32,
    /// STRUCT TIMEVAL: system time.
    pub p_ustime_usec: u32,

    /// The four bytes of padding the C compiler puts here to align `p_uru_maxrss`.
    pub p_pad0: u32,

    /// LONG: max resident set size.
    pub p_uru_maxrss: u64,
    /// LONG: integral shared memory size.
    pub p_uru_ixrss: u64,
    /// LONG: integral unshared data ".
    pub p_uru_idrss: u64,
    /// LONG: integral unshared stack ".
    pub p_uru_isrss: u64,
    /// LONG: page reclaims.
    pub p_uru_minflt: u64,
    /// LONG: page faults.
    pub p_uru_majflt: u64,
    /// LONG: swaps.
    pub p_uru_nswap: u64,
    /// LONG: block input operations.
    pub p_uru_inblock: u64,
    /// LONG: block output operations.
    pub p_uru_oublock: u64,
    /// LONG: messages sent.
    pub p_uru_msgsnd: u64,
    /// LONG: messages received.
    pub p_uru_msgrcv: u64,
    /// LONG: signals received.
    pub p_uru_nsignals: u64,
    /// LONG: voluntary context switches.
    pub p_uru_nvcsw: u64,
    /// LONG: involuntary ".
    pub p_uru_nivcsw: u64,

    /// STRUCT TIMEVAL: child u+s time.
    pub p_uctime_sec: u32,
    /// STRUCT TIMEVAL: child u+s time.
    pub p_uctime_usec: u32,
    /// UINT: `PS_*` flags on the process.
    pub p_psflags: u32,
    /// UINT: Accounting flags.
    pub p_acflag: u32,
    /// UID_T: saved user id.
    pub p_svuid: u32,
    /// GID_T: saved group id.
    pub p_svgid: u32,
    /// Syscall emulation name.
    pub p_emul: [u8; KI_EMULNAMELEN],
    /// RLIM_T: soft limit for rss.
    pub p_rlim_rss_cur: u64,
    /// LONG: CPU id.
    pub p_cpuid: u64,
    /// VSIZE_T: virtual size.
    pub p_vm_map_size: u64,
    /// PID_T: Thread identifier.
    pub p_tid: i32,
    /// U_INT: Routing table identifier.
    pub p_rtableid: u32,

    /// U_INT64_T: Pledge flags.
    pub p_pledge: u64,
    /// Thread name.
    pub p_name: [u8; KI_MAXCOMLEN],
}

impl KinfoProc {
    /// `memset(kp, 0, sizeof(*kp))`.
    pub fn zeroed() -> Self {
        // SAFETY: every member is an integer or an array of integers, for which all-zero
        // bytes are a valid value.
        unsafe { core::mem::MaybeUninit::<Self>::zeroed().assume_init() }
    }
}

/// `EPROC_CTTY`: controlling tty vnode active.
pub const EPROC_CTTY: i32 = 0x01;
/// `EPROC_SLEADER`: session leader.
pub const EPROC_SLEADER: i32 = 0x02;
/// `EPROC_UNVEIL`: has unveil settings.
pub const EPROC_UNVEIL: i32 = 0x04;
/// `EPROC_LKUNVEIL`: unveil is locked.
pub const EPROC_LKUNVEIL: i32 = 0x08;

/// `struct kinfo_vmentry`: a VM address range, matching `struct vm_map_entry`, for debuggers.
/// To iterate entries, set the last `kve_end` as the base address into `kve_start`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KinfoVmentry {
    /// `vaddr_t`.
    pub kve_start: u64,
    /// `vaddr_t`.
    pub kve_end: u64,
    /// `vsize_t`.
    pub kve_guard: u64,
    /// `vsize_t`.
    pub kve_fspace: u64,
    /// `vsize_t`.
    pub kve_fspace_augment: u64,
    /// `voff_t`.
    pub kve_offset: u64,
    /// `kve_wired_count`.
    pub kve_wired_count: i32,
    /// `kve_etype`.
    pub kve_etype: i32,
    /// `kve_protection`.
    pub kve_protection: i32,
    /// `kve_max_protection`.
    pub kve_max_protection: i32,
    /// `kve_advice`.
    pub kve_advice: i32,
    /// `kve_inheritance`.
    pub kve_inheritance: i32,
    /// `u_int8_t`.
    pub kve_flags: u8,
}

/// `KVE_ET_OBJ` (keep in sync with `UVM_ET_*`).
pub const KVE_ET_OBJ: i32 = 0x0000_0001;
/// `KVE_ET_SUBMAP`.
pub const KVE_ET_SUBMAP: i32 = 0x0000_0002;
/// `KVE_ET_COPYONWRITE`.
pub const KVE_ET_COPYONWRITE: i32 = 0x0000_0004;
/// `KVE_ET_NEEDSCOPY`.
pub const KVE_ET_NEEDSCOPY: i32 = 0x0000_0008;
/// `KVE_ET_HOLE`.
pub const KVE_ET_HOLE: i32 = 0x0000_0010;
/// `KVE_ET_NOFAULT`.
pub const KVE_ET_NOFAULT: i32 = 0x0000_0020;
/// `KVE_ET_STACK`.
pub const KVE_ET_STACK: i32 = 0x0000_0040;
/// `KVE_ET_WC`.
pub const KVE_ET_WC: i32 = 0x0000_0080;
/// `KVE_ET_CONCEAL`.
pub const KVE_ET_CONCEAL: i32 = 0x0000_0100;
/// `KVE_ET_SYSCALL`.
pub const KVE_ET_SYSCALL: i32 = 0x0000_0200;
/// `KVE_ET_FREEMAPPED`.
pub const KVE_ET_FREEMAPPED: i32 = 0x0000_0800;

/// `KVE_PROT_NONE`.
pub const KVE_PROT_NONE: i32 = 0x0000_0000;
/// `KVE_PROT_READ`.
pub const KVE_PROT_READ: i32 = 0x0000_0001;
/// `KVE_PROT_WRITE`.
pub const KVE_PROT_WRITE: i32 = 0x0000_0002;
/// `KVE_PROT_EXEC`.
pub const KVE_PROT_EXEC: i32 = 0x0000_0004;

/// `KVE_ADV_NORMAL`.
pub const KVE_ADV_NORMAL: i32 = 0x0000_0000;
/// `KVE_ADV_RANDOM`.
pub const KVE_ADV_RANDOM: i32 = 0x0000_0001;
/// `KVE_ADV_SEQUENTIAL`.
pub const KVE_ADV_SEQUENTIAL: i32 = 0x0000_0002;

/// `KVE_INH_SHARE`.
pub const KVE_INH_SHARE: i32 = 0x0000_0000;
/// `KVE_INH_COPY`.
pub const KVE_INH_COPY: i32 = 0x0000_0010;
/// `KVE_INH_NONE`.
pub const KVE_INH_NONE: i32 = 0x0000_0020;
/// `KVE_INH_ZERO`.
pub const KVE_INH_ZERO: i32 = 0x0000_0030;

/// `KVE_F_STATIC`.
pub const KVE_F_STATIC: u8 = 0x01;
/// `KVE_F_KMEM`.
pub const KVE_F_KMEM: u8 = 0x02;

/// `KERN_FILE_BYFILE`.
pub const KERN_FILE_BYFILE: i32 = 1;
/// `KERN_FILE_BYPID`.
pub const KERN_FILE_BYPID: i32 = 2;
/// `KERN_FILE_BYUID`.
pub const KERN_FILE_BYUID: i32 = 3;
/// `KERN_FILESLOP`.
pub const KERN_FILESLOP: usize = 10;

/// `KERN_FILE_TEXT`.
pub const KERN_FILE_TEXT: i32 = -1;
/// `KERN_FILE_CDIR`.
pub const KERN_FILE_CDIR: i32 = -2;
/// `KERN_FILE_RDIR`.
pub const KERN_FILE_RDIR: i32 = -3;
/// `KERN_FILE_TRACE`.
pub const KERN_FILE_TRACE: i32 = -4;

/// `KI_MNAMELEN`: rounded up from 90.
pub const KI_MNAMELEN: usize = 96;
/// `KI_UNPPATHLEN`.
pub const KI_UNPPATHLEN: usize = 104;

/// `struct kinfo_file`: what `kern.file` returns, immune to 32/64 bit emulation issues. The
/// order differs slightly from the real `struct file`, and some fields come from other
/// structures (`struct vnode`, `struct proc`) to make the information more useful.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KinfoFile {
    /// PTR: address of struct file.
    pub f_fileaddr: u64,
    /// UINT: flags (see `fcntl.h`).
    pub f_flag: u32,
    /// UINT: internal flags.
    pub f_iflags: u32,
    /// INT: descriptor type.
    pub f_type: u32,
    /// UINT: reference count.
    pub f_count: u32,
    /// UINT: references from msg queue.
    pub f_msgcount: u32,
    /// INT: number active users.
    pub f_usecount: u32,
    /// PTR: creds for descriptor.
    pub f_ucred: u64,
    /// UID_T: descriptor credentials.
    pub f_uid: u32,
    /// GID_T: descriptor credentials.
    pub f_gid: u32,
    /// PTR: address of fileops.
    pub f_ops: u64,
    /// OFF_T: offset.
    pub f_offset: u64,
    /// PTR: descriptor data.
    pub f_data: u64,
    /// UINT64: number of read xfers.
    pub f_rxfer: u64,
    /// UINT64: number of write xfers.
    pub f_rwfer: u64,
    /// UINT64: number of seek operations.
    pub f_seek: u64,
    /// UINT64: total bytes read.
    pub f_rbytes: u64,
    /// UINT64: total bytes written.
    pub f_wbytes: u64,

    /// PTR: socket, specinfo, etc.
    pub v_un: u64,
    /// ENUM: vnode type.
    pub v_type: u32,
    /// ENUM: type of underlying data.
    pub v_tag: u32,
    /// UINT: vnode flags.
    pub v_flag: u32,
    /// DEV_T: raw device.
    pub va_rdev: u32,
    /// PTR: private data for fs.
    pub v_data: u64,
    /// PTR: mount info for fs.
    pub v_mount: u64,
    /// LONG: file id.
    pub va_fileid: u64,
    /// UINT64_T: file size in bytes.
    pub va_size: u64,
    /// MODE_T: file access mode and type.
    pub va_mode: u32,
    /// DEV_T: filesystem device.
    pub va_fsid: u32,
    /// The mount point of the file's file system.
    pub f_mntonname: [u8; KI_MNAMELEN],

    /// SHORT: socket type.
    pub so_type: u32,
    /// SHORT: socket state.
    pub so_state: u32,
    /// PTR: socket pcb; for non-root: -1 if not NULL.
    pub so_pcb: u64,
    /// SHORT: socket protocol type.
    pub so_protocol: u32,
    /// INT: socket domain family.
    pub so_family: u32,
    /// PTR: pointer to per-protocol pcb.
    pub inp_ppcb: u64,
    /// SHORT: local inet port.
    pub inp_lport: u32,
    /// STRUCT: local inet addr.
    pub inp_laddru: [u32; 4],
    /// SHORT: foreign inet port.
    pub inp_fport: u32,
    /// STRUCT: foreign inet addr.
    pub inp_faddru: [u32; 4],
    /// PTR: connected socket cntrl block.
    pub unp_conn: u64,

    /// PTR: link with other direction.
    pub pipe_peer: u64,
    /// UINT: pipe status info.
    pub pipe_state: u32,

    /// INT: number of pending events.
    pub kq_count: u32,
    /// INT: kqueue status information.
    pub kq_state: u32,

    /// INT: unused.
    pub __unused1: u32,

    /// PID_T: process id.
    pub p_pid: u32,
    /// INT: descriptor number.
    pub fd_fd: i32,
    /// CHAR: open file flags.
    pub fd_ofileflags: u32,
    /// UID_T: process credentials.
    pub p_uid: u32,
    /// GID_T: process credentials.
    pub p_gid: u32,
    /// PID_T: thread id.
    pub p_tid: u32,
    /// Command name.
    pub p_comm: [u8; KI_MAXCOMLEN],

    /// UINT: Routing table identifier.
    pub inp_rtableid: u32,
    /// PTR: `f_data` of spliced socket.
    pub so_splice: u64,
    /// OFF_T: already spliced count or -1 if this is target of splice.
    pub so_splicelen: i64,
    /// LONG: chars in receive buf.
    pub so_rcv_cc: u64,
    /// LONG: chars in send buf.
    pub so_snd_cc: u64,
    /// PTR: connected sockets.
    pub unp_refs: u64,
    /// PTR: link to next connected socket.
    pub unp_nextref: u64,
    /// PTR: address of the socket address.
    pub unp_addr: u64,
    /// The socket's path.
    pub unp_path: [u8; KI_UNPPATHLEN],
    /// CHAR: raw protocol id.
    pub inp_proto: u32,
    /// SHORT: tcp state.
    pub t_state: u32,
    /// ULONG: tcp receive window.
    pub t_rcv_wnd: u64,
    /// ULONG: tcp send window.
    pub t_snd_wnd: u64,
    /// ULONG: congestion-controlled win.
    pub t_snd_cwnd: u64,

    /// NLINK_T: number of references to file.
    pub va_nlink: u32,
}

/// `KERN_INTRCNT_NUM`: int: # intrcnt.
pub const KERN_INTRCNT_NUM: i32 = 1;
/// `KERN_INTRCNT_CNT`: node: intrcnt.
pub const KERN_INTRCNT_CNT: i32 = 2;
/// `KERN_INTRCNT_NAME`: node: names.
pub const KERN_INTRCNT_NAME: i32 = 3;
/// `KERN_INTRCNT_VECTOR`: node: interrupt vector #.
pub const KERN_INTRCNT_VECTOR: i32 = 4;
/// `KERN_INTRCNT_MAXID`.
pub const KERN_INTRCNT_MAXID: usize = 5;

/// `CTL_KERN_INTRCNT_NAMES` (four entries, as in C: the vector node has no name).
pub const CTL_KERN_INTRCNT_NAMES: [Ctlname; 4] = [
    Ctlname::NONE,
    Ctlname::new(b"nintrcnt", CTLTYPE_INT),
    Ctlname::new(b"intrcnt", CTLTYPE_NODE),
    Ctlname::new(b"intrname", CTLTYPE_NODE),
];

/// `KERN_WATCHDOG_PERIOD`: int: watchdog period.
pub const KERN_WATCHDOG_PERIOD: i32 = 1;
/// `KERN_WATCHDOG_AUTO`: int: automatic tickle.
pub const KERN_WATCHDOG_AUTO: i32 = 2;
/// `KERN_WATCHDOG_MAXID`.
pub const KERN_WATCHDOG_MAXID: usize = 3;

/// `CTL_KERN_WATCHDOG_NAMES`.
pub const CTL_KERN_WATCHDOG_NAMES: [Ctlname; KERN_WATCHDOG_MAXID] = [
    Ctlname::NONE,
    Ctlname::new(b"period", CTLTYPE_INT),
    Ctlname::new(b"auto", CTLTYPE_INT),
];

/// `KERN_TIMECOUNTER_TICK`: int: number of revolutions.
pub const KERN_TIMECOUNTER_TICK: i32 = 1;
/// `KERN_TIMECOUNTER_TIMESTEPWARNINGS`: int: log a warning when time changes.
pub const KERN_TIMECOUNTER_TIMESTEPWARNINGS: i32 = 2;
/// `KERN_TIMECOUNTER_HARDWARE`: string: tick hardware used.
pub const KERN_TIMECOUNTER_HARDWARE: i32 = 3;
/// `KERN_TIMECOUNTER_CHOICE`: string: tick hardware used.
pub const KERN_TIMECOUNTER_CHOICE: i32 = 4;
/// `KERN_TIMECOUNTER_MAXID`.
pub const KERN_TIMECOUNTER_MAXID: usize = 5;

/// `CTL_KERN_TIMECOUNTER_NAMES`.
pub const CTL_KERN_TIMECOUNTER_NAMES: [Ctlname; KERN_TIMECOUNTER_MAXID] = [
    Ctlname::NONE,
    Ctlname::new(b"tick", CTLTYPE_INT),
    Ctlname::new(b"timestepwarnings", CTLTYPE_INT),
    Ctlname::new(b"hardware", CTLTYPE_STRING),
    Ctlname::new(b"choice", CTLTYPE_STRING),
];

/// `KERN_CLOCKINTR_STATS`: struct: stats.
pub const KERN_CLOCKINTR_STATS: i32 = 1;
/// `KERN_CLOCKINTR_MAXID`.
pub const KERN_CLOCKINTR_MAXID: usize = 2;

/// `CTL_KERN_CLOCKINTR_NAMES`.
pub const CTL_KERN_CLOCKINTR_NAMES: [Ctlname; KERN_CLOCKINTR_MAXID] =
    [Ctlname::NONE, Ctlname::new(b"stats", CTLTYPE_STRUCT)];

/// `HW_MACHINE`: string: machine class.
pub const HW_MACHINE: i32 = 1;
/// `HW_MODEL`: string: specific machine model.
pub const HW_MODEL: i32 = 2;
/// `HW_NCPU`: int: number of configured cpus.
pub const HW_NCPU: i32 = 3;
/// `HW_BYTEORDER`: int: machine byte order.
pub const HW_BYTEORDER: i32 = 4;
/// `HW_PHYSMEM`: int: total memory.
pub const HW_PHYSMEM: i32 = 5;
/// `HW_USERMEM`: int: non-kernel memory.
pub const HW_USERMEM: i32 = 6;
/// `HW_PAGESIZE`: int: software page size.
pub const HW_PAGESIZE: i32 = 7;
/// `HW_DISKNAMES`: strings: disk drive names.
pub const HW_DISKNAMES: i32 = 8;
/// `HW_DISKSTATS`: struct: diskstats[].
pub const HW_DISKSTATS: i32 = 9;
/// `HW_DISKCOUNT`: int: number of disks.
pub const HW_DISKCOUNT: i32 = 10;
/// `HW_SENSORS`: node: hardware monitors.
pub const HW_SENSORS: i32 = 11;
/// `HW_CPUSPEED`: get CPU frequency.
pub const HW_CPUSPEED: i32 = 12;
/// `HW_SETPERF`: set CPU performance %.
pub const HW_SETPERF: i32 = 13;
/// `HW_VENDOR`: string: vendor name.
pub const HW_VENDOR: i32 = 14;
/// `HW_PRODUCT`: string: product name.
pub const HW_PRODUCT: i32 = 15;
/// `HW_VERSION`: string: hardware version.
pub const HW_VERSION: i32 = 16;
/// `HW_SERIALNO`: string: hardware serial number.
pub const HW_SERIALNO: i32 = 17;
/// `HW_UUID`: string: universal unique id.
pub const HW_UUID: i32 = 18;
/// `HW_PHYSMEM64`: quad: total memory.
pub const HW_PHYSMEM64: i32 = 19;
/// `HW_USERMEM64`: quad: non-kernel memory.
pub const HW_USERMEM64: i32 = 20;
/// `HW_NCPUFOUND`: int: number of cpus found.
pub const HW_NCPUFOUND: i32 = 21;
/// `HW_ALLOWPOWERDOWN`: allow power button shutdown.
pub const HW_ALLOWPOWERDOWN: i32 = 22;
/// `HW_PERFPOLICY`: set performance policy.
pub const HW_PERFPOLICY: i32 = 23;
/// `HW_SMT`: int: enable SMT/HT/CMT.
pub const HW_SMT: i32 = 24;
/// `HW_NCPUONLINE`: int: number of cpus being used.
pub const HW_NCPUONLINE: i32 = 25;
/// `HW_POWER`: int: machine has wall-power.
pub const HW_POWER: i32 = 26;
/// `HW_BATTERY`: node: battery.
pub const HW_BATTERY: i32 = 27;
/// `HW_UCOMNAMES`: strings: ucom names.
pub const HW_UCOMNAMES: i32 = 28;
/// `HW_BLOCKCPU`: string: cpu types to block.
pub const HW_BLOCKCPU: i32 = 29;
/// `HW_MAXID`: number of valid hw ids.
pub const HW_MAXID: usize = 30;

/// `CTL_HW_NAMES`.
pub const CTL_HW_NAMES: [Ctlname; HW_MAXID] = [
    Ctlname::NONE,
    Ctlname::new(b"machine", CTLTYPE_STRING),
    Ctlname::new(b"model", CTLTYPE_STRING),
    Ctlname::new(b"ncpu", CTLTYPE_INT),
    Ctlname::new(b"byteorder", CTLTYPE_INT),
    Ctlname::new(b"gap", 0),
    Ctlname::new(b"gap", 0),
    Ctlname::new(b"pagesize", CTLTYPE_INT),
    Ctlname::new(b"disknames", CTLTYPE_STRING),
    Ctlname::new(b"diskstats", CTLTYPE_STRUCT),
    Ctlname::new(b"diskcount", CTLTYPE_INT),
    Ctlname::new(b"sensors", CTLTYPE_NODE),
    Ctlname::new(b"cpuspeed", CTLTYPE_INT),
    Ctlname::new(b"setperf", CTLTYPE_INT),
    Ctlname::new(b"vendor", CTLTYPE_STRING),
    Ctlname::new(b"product", CTLTYPE_STRING),
    Ctlname::new(b"version", CTLTYPE_STRING),
    Ctlname::new(b"serialno", CTLTYPE_STRING),
    Ctlname::new(b"uuid", CTLTYPE_STRING),
    Ctlname::new(b"physmem", CTLTYPE_QUAD),
    Ctlname::new(b"usermem", CTLTYPE_QUAD),
    Ctlname::new(b"ncpufound", CTLTYPE_INT),
    Ctlname::new(b"allowpowerdown", CTLTYPE_INT),
    Ctlname::new(b"perfpolicy", CTLTYPE_STRING),
    Ctlname::new(b"smt", CTLTYPE_INT),
    Ctlname::new(b"ncpuonline", CTLTYPE_INT),
    Ctlname::new(b"power", CTLTYPE_INT),
    Ctlname::new(b"battery", CTLTYPE_NODE),
    Ctlname::new(b"ucomnames", CTLTYPE_STRING),
    Ctlname::new(b"blockcpu", CTLTYPE_STRING),
];

/// `HW_BATTERY_CHARGEMODE`: int: battery charging mode.
pub const HW_BATTERY_CHARGEMODE: i32 = 1;
/// `HW_BATTERY_CHARGESTART`: int: battery start charge percent.
pub const HW_BATTERY_CHARGESTART: i32 = 2;
/// `HW_BATTERY_CHARGESTOP`: int: battery stop charge percent.
pub const HW_BATTERY_CHARGESTOP: i32 = 3;
/// `HW_BATTERY_MAXID`.
pub const HW_BATTERY_MAXID: usize = 4;

/// `CTL_HW_BATTERY_NAMES`.
pub const CTL_HW_BATTERY_NAMES: [Ctlname; HW_BATTERY_MAXID] = [
    Ctlname::NONE,
    Ctlname::new(b"chargemode", CTLTYPE_INT),
    Ctlname::new(b"chargestart", CTLTYPE_INT),
    Ctlname::new(b"chargestop", CTLTYPE_INT),
];

/// `CTL_DEBUG_NAME`: string: variable name.
pub const CTL_DEBUG_NAME: i32 = 0;
/// `CTL_DEBUG_VALUE`: int: variable value.
pub const CTL_DEBUG_VALUE: i32 = 1;
/// `CTL_DEBUG_MAXID`.
pub const CTL_DEBUG_MAXID: usize = 20;

/// `SYSCTL_INT_READONLY`: the special case `minimum, maximum` marker of
/// [`SysctlBoundedArgs`].
pub const SYSCTL_INT_READONLY: (i32, i32) = (1, 0);

/// `struct sysctl_bounded_args`: an exported sysctl variable with valid bounds. Both bounds
/// are inclusive to allow the full range of values.
#[derive(Clone, Copy, Debug)]
pub struct SysctlBoundedArgs {
    /// `mib`: identifier shared with userspace as a `CTL_` define.
    pub mib: i32,
    /// `var`: never NULL.
    pub var: &'static AtomicI32,
    /// `minimum`: checking is disabled if `minimum == maximum`.
    pub minimum: i32,
    /// `maximum`: read-only variable if `minimum > maximum`.
    pub maximum: i32,
}

impl SysctlBoundedArgs {
    /// `{ mib, var, minimum, maximum }`.
    pub const fn new(mib: i32, var: &'static AtomicI32, minimum: i32, maximum: i32) -> Self {
        Self {
            mib,
            var,
            minimum,
            maximum,
        }
    }

    /// `{ mib, var, SYSCTL_INT_READONLY }`.
    pub const fn readonly(mib: i32, var: &'static AtomicI32) -> Self {
        Self::new(mib, var, SYSCTL_INT_READONLY.0, SYSCTL_INT_READONLY.1)
    }
}

/// `sysctlfn`: the internal sysctl function calling convention,
/// `(*sysctlfn)(name, oldp, oldlenp, newp, newlen, p)`. `name` is the rest of the name from
/// the component to be interpreted on (the C's `name` and `namelen`); `oldp` and `newp` are
/// user addresses, 0 for the C's NULL.
pub type Sysctlfn = fn(&[i32], usize, &mut usize, usize, usize, &Proc) -> Result<(), Errno>;

/// A structure that `sysctl_rdstruct` and `sysctl_struct` may copy as raw bytes (not
/// OpenBSD's: C passes a `void *` and a size).
///
/// # Safety
///
/// The implementor is `#[repr(C)]` (or a primitive, or an array of such), has no padding
/// bytes, and every bit pattern is a valid value of it.
pub unsafe trait SysctlPlain: Sized {
    /// The structure's bytes, as `copyout` reads them.
    fn as_bytes(&self) -> &[u8] {
        // SAFETY: the trait's contract: no padding, so every byte is initialised, and the
        // slice borrows `self` for its lifetime.
        unsafe {
            core::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                core::mem::size_of::<Self>(),
            )
        }
    }

    /// The structure's bytes, as `copyin` writes them.
    fn as_bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: the trait's contract: every bit pattern is a valid value, and the slice
        // borrows `self` exclusively for its lifetime.
        unsafe {
            core::slice::from_raw_parts_mut(
                (self as *mut Self).cast::<u8>(),
                core::mem::size_of::<Self>(),
            )
        }
    }
}

// SAFETY: primitives have no padding and every bit pattern is valid.
unsafe impl SysctlPlain for i32 {}
// SAFETY: as for `i32`.
unsafe impl SysctlPlain for u32 {}
// SAFETY: as for `i32`.
unsafe impl SysctlPlain for i64 {}
// SAFETY: as for `i32`.
unsafe impl SysctlPlain for u64 {}
// SAFETY: an array of padding-free, any-bit-pattern elements is laid out without gaps.
unsafe impl<T: SysctlPlain, const N: usize> SysctlPlain for [T; N] {}
// SAFETY: `#[repr(C)]`, two 64-bit words (`time_t`, `suseconds_t` is a `long`), no padding.
unsafe impl SysctlPlain for Timeval {}
// SAFETY: `#[repr(C)]`, four `i32`s, no padding.
unsafe impl SysctlPlain for Clockinfo {}
// SAFETY: `#[repr(C)]` integers and byte arrays, the one C padding hole made a member
// (`p_pad0`); the compile-time checks below pin the size and the offsets.
unsafe impl SysctlPlain for KinfoProc {}
// SAFETY: `#[repr(C)]`, seven `u64`s, no padding.
unsafe impl SysctlPlain for Cpustats {}
// SAFETY: `#[repr(C)]`, `u64`s only, no padding.
unsafe impl SysctlPlain for Mbstat {}

const _: () = {
    use core::mem::{offset_of, size_of};
    // The C layout (amd64 and arm64 are both LP64): sizes and a few offsets.
    assert!(size_of::<KinfoProc>() == 648);
    assert!(offset_of!(KinfoProc, p_ngroups) == 208);
    assert!(offset_of!(KinfoProc, p_tdev) == 212);
    assert!(offset_of!(KinfoProc, p_stat) == 304);
    assert!(offset_of!(KinfoProc, p_comm) == 312);
    assert!(offset_of!(KinfoProc, p_login) == 352);
    assert!(offset_of!(KinfoProc, p_uvalid) == 400);
    assert!(offset_of!(KinfoProc, p_ustart_usec) == 416);
    assert!(offset_of!(KinfoProc, p_uru_maxrss) == 440);
    assert!(offset_of!(KinfoProc, p_emul) == 576);
    assert!(offset_of!(KinfoProc, p_name) == 624);
    assert!(size_of::<KinfoFile>() == 632);
    assert!(size_of::<KinfoVmentry>() == 80);
    assert!(size_of::<Timeval>() == 16);
    assert!(size_of::<Clockinfo>() == 16);
    // The name tables cover every id.
    assert!(CTL_NAMES.len() == CTL_VFS as usize + 1);
    assert!(CTL_KERN_NAMES.len() == KERN_AUTOCONF_SERIAL as usize + 1);
    assert!(CTL_HW_NAMES.len() == HW_BLOCKCPU as usize + 1);
};

#[cfg(test)]
mod tests {
    use super::*;

    /// `(name, value)` pairs for the reference check.
    macro_rules! defs {
        ($($name:ident),* $(,)?) => { [$((stringify!($name), $name as i64)),*] };
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/sysctl.h");
        let ours = defs![
            CTL_MAXNAME,
            CTLTYPE_NODE,
            CTLTYPE_INT,
            CTLTYPE_STRING,
            CTLTYPE_QUAD,
            CTLTYPE_STRUCT,
            CTL_KERN,
            CTL_VM,
            CTL_NET,
            CTL_DEBUG,
            CTL_HW,
            CTL_MACHDEP,
            CTL_DDB,
            CTL_VFS,
            CTL_MAXID,
            KERN_OSTYPE,
            KERN_OSRELEASE,
            KERN_OSREV,
            KERN_VERSION,
            KERN_MAXPROC,
            KERN_ARGMAX,
            KERN_SECURELVL,
            KERN_HOSTNAME,
            KERN_HOSTID,
            KERN_CLOCKRATE,
            KERN_BOOTTIME,
            KERN_DOMAINNAME,
            KERN_OSVERSION,
            KERN_MSGBUFSIZE,
            KERN_CPTIME,
            KERN_FORKSTAT,
            KERN_NPROCS,
            KERN_MSGBUF,
            KERN_PROC_ARGS,
            KERN_MBSTAT,
            KERN_PROC,
            KERN_CPTIME2,
            KERN_FILE,
            KERN_CONSDEV,
            KERN_POOL_DEBUG,
            KERN_PROC_VMMAP,
            KERN_CONSBUF,
            KERN_CPUSTATS,
            KERN_TIMEOUT_STATS,
            KERN_UTC_OFFSET,
            KERN_CLOCKINTR,
            KERN_AUTOCONF_SERIAL,
            KERN_MAXID,
            KERN_PROC_KTHREAD,
            KERN_PROC_SHOW_THREADS,
            KERN_PROC_NENV,
            KI_NGROUPS,
            KI_WMESGLEN,
            KI_MAXLOGNAME,
            KI_EMULNAMELEN,
            KI_MNAMELEN,
            KI_UNPPATHLEN,
            KERN_FILESLOP,
            HW_MACHINE,
            HW_MODEL,
            HW_NCPU,
            HW_PAGESIZE,
            HW_PHYSMEM64,
            HW_NCPUFOUND,
            HW_NCPUONLINE,
            HW_BLOCKCPU,
            HW_MAXID,
            HW_BATTERY_CHARGESTOP,
            HW_BATTERY_MAXID,
            CTL_DEBUG_MAXID,
            EPROC_LKUNVEIL,
            KVE_ET_FREEMAPPED,
            KVE_INH_ZERO,
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
