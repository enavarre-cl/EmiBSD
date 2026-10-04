/*	$OpenBSD: kern_sysctl.c,v 1.497 2026/09/19 17:29:23 dgl Exp $	*/
/*	$NetBSD: kern_sysctl.c,v 1.17 1996/05/20 17:49:05 mrg Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)kern_sysctl.c	8.4 (Berkeley) 4/14/94
 */
/* </LICENSES> */

//! The `sysctl(2)` system call and the `kern` and `hw` trees: `kern/kern_sysctl.c`. Also the
//! helpers every subsystem's sysctl node is written with (`sysctl_int`, `sysctl_rdint`,
//! `sysctl_string`, `sysctl_rdstruct`, `sysctl_bounded_arr`, ...).
//!
//! Upstream: sys/kern/kern_sysctl.c @ 3ce1f3f79392
//!
//! The system identifies itself as EmiBSD 8.0 (`kern.ostype`, `kern.osrelease`,
//! `kern.version`, `kern.osversion` come from `conf/vers.rs`); `kern.osrevision` stays the
//! `OpenBSD` API date of `<sys/param.h>`, which is what programs test.
//!
//! Real nodes: `kern.ostype`, `osrelease`, `osrevision`, `version`, `osversion`, `maxproc`,
//! `maxfiles`, `argmax`, `securelevel`, `hostname`, `domainname`, `hostid`, `clockrate`,
//! `posix1version`, `ngroups`, `job_control`, `saved_ids`, `boottime`, `maxpartitions`,
//! `rawpartition`, `maxthread`, `nthreads`, `fsync`, `sysvmsg`/`sysvsem`/`sysvshm` (0: not
//! configured), `msgbufsize`, `msgbuf`, `consbufsize`, `consbuf`, `cp_time`, `cp_time2`,
//! `cpustats`, `forkstat`, `ccpu`, `fscale`, `nprocs`, `allowkmem`, `splassert`, `mbstat`,
//! `proc` (`kinfo_proc`), `file` (`kinfo_file`), `proc_nobroadcastkill`, `maxclusters`,
//! `wxabort`, `consdev`,
//! `netlivelocks`, `pool_debug`, `timeout_stats`, `utc_offset`, `autoconf_serial`; `hw.machine`,
//! `ncpu`, `ncpufound`, `ncpuonline`, `byteorder`, `physmem`, `usermem`, `physmem64`,
//! `usermem64`, `pagesize`, `power`, `allowpowerdown`, `ucomnames`, `cpuspeed`, `battery.*`,
//! `diskcount`, `disknames`, `diskstats`,
//! `vendor`/`product`/`version`/`serialno`/`uuid` (whatever the machine recorded). The
//! `vm` tree is `uvm/uvm_meter.rs`.
//!
//! ## Deviations
//! - Names are slices (`&[i32]`), `oldp`/`newp` user addresses as `usize` (0 is NULL),
//!   `oldlenp` a `&mut usize` (every caller has one: `sys_sysctl` passes its `oldlen`), and
//!   errors `Result<(), Errno>`. A string is `&[u8]` up to its first NUL, a writable string
//!   `&mut [u8]` whose length is the C's `maxlen`; a structure is its bytes
//!   ([`SysctlPlain`]). `int *valp` is `&AtomicI32`, the C's atomic operations on it are the
//!   atomic's; a C local passed by address is an `AtomicI32` read back with `into_inner`.
//! - Every node whose subsystem is not ported reports itself with `unported!` and fails with
//!   `ENOSYS`: `watchdog` (`kern_watchdog.c`), `clockintr`, `proc_vmmap` after its checks
//!   (`fill_vmmap`); `hw.model` (`cpu_model`, `identcpu.c`/arm64 `cpu.c`),
//!   `setperf`/`perfpolicy` (`sched_bsd.c`); the top-level
//!   `machdep` (`cpu_sysctl`) and `ddb` (`ddb_sysctl`) trees. `kern.proc_cwd` of a process
//!   without a current directory (none has one before a root file system is mounted) is
//!   `ENOENT`. `resettodr` after a new `kern.utc_offset` is reported and skipped.
//! - Options this kernel does not configure are compiled out as in C: `DEBUG_SYSCTL`
//!   (`debug_sysctl`, `CTL_DEBUG` is `EOPNOTSUPP`), `SYSVMSG`/`SYSVSEM`/`SYSVSHM`
//!   (`sysctl_sysvipc`), `NAUDIO`/`NVIDEO`/`NDT`/`NUCOM` (0), `GPROF`, `WITNESS`,
//!   `PTRACE` (`kern.global_ptrace`), `KTRACE` (the trace members of `kinfo_proc` stay
//!   zero). `SMALL_KERNEL` is not set.
//! - The kernel lock is taken at the C's sites (M11e). `log_mtx` does not exist
//!   (`subr_log.rs`), so the message buffer header is read without it.
//! - `kern.file`: `fill_file` fills the `AF_INET` and `AF_INET6` (feature `inet6`) control
//!   blocks and the TCP members (`fill_file_tcpcb`, zero for a control block without a
//!   `tcpcb`). `KERN_FILE_BYFILE` of sockets walks `tcbtable`, `udbtable`, `rawcbtable`,
//!   `divbtable` (`NPF`) and, with `INET6`, `tcb6table`, `udb6table`, `rawin6pcbtable` and
//!   `divb6table`. `ps_tracevp` does not exist
//!   (`KTRACE`), so no `KERN_FILE_TRACE` entry is made. The C's `FILLIT` macros are the
//!   methods of a private `FileWalk` (the C's `kf`, `dp`, `buflen`, `elem_count`,
//!   `needed`); `kf` lives in it instead of an `M_TEMP` allocation. A `copyout` error ends
//!   the walk and is returned once the references taken are dropped (the C's `break` only
//!   left the macro's `do { } while (0)`, so its walk went on and a later `copyout` could
//!   overwrite the error). `kinfo_file` is copied out as bytes, its two C padding holes
//!   being members (`sys/sysctl.rs`).
//! - `disknames`/`diskstats` are `AtomicPtr`s to their `M_SYSCTL` allocations, with their
//!   lengths in `AtomicUsize`s, all under `sysctl_disklock`. `hw.disknames` and
//!   `hw.diskstats` read them holding that lock for reading (the C reads them after
//!   `sysctl_diskinit` let go of it, under the kernel lock), and `hw.diskstats` copies at
//!   most `diskstatslen` bytes. `sysctl_diskinit` fills at most as many entries as it
//!   allocated (the C trusts `disk_count` to match the list).
//! - The morally-const values `sysctl_bounded_arr` reports (`arg_max`, `openbsd`, ...) are
//!   `AtomicI32` statics like the C's `static int`s; `ccpu` is a constant in `sched_bsd.rs`,
//!   so the table points at a read-only copy of it.
//! - `hw_vendor`, `hw_prod`, `hw_uuid`, `hw_serial`, `hw_ver`, `hw_power` and the
//!   `hw_battery_*` globals keep their lowercase C names: their uppercase spellings are the
//!   `HW_*` sysctl ids of `<sys/sysctl.h>`.
//! - `KERN_CPTIME` with no CPU online (which only the host double can produce) leaves the
//!   sums at zero instead of dividing by zero; an empty name (never from `sys_sysctl`, which
//!   wants two components) is `EINVAL`.
//! - `hw.sensors.<dev>.<type>.<numt>` with a `type` outside `enum sensor_type` finds no
//!   sensor (`ENOENT`, after the device lookup's own errors), as the C's comparison would.

use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicUsize, Ordering};

use libkern::{StaticCell, strlcpy, strnlen};

use crate::conf::param::{FSCALE, MAXFILES, MAXPROCESS, MAXTHREAD, NMBCLUST, UTC_OFFSET};
use crate::conf::vers::{OSRELEASE, OSTYPE, OSVERSION, VERSION};
use crate::dev::cons::cn_tab;
use crate::kern::init_main::{NCPUS, NCPUSFOUND};
use crate::kern::kern_clock::sysctl_clockrate;
use crate::kern::kern_descrip::{NUMFILES, fd_getfile, fd_iterfile};
use crate::kern::kern_event::fp_kqueue;
use crate::kern::kern_fork::{FORKSTAT, NPROCESSES, NTHREADS};
use crate::kern::kern_lock::{mtx_enter, mtx_leave, pc_cons_enter, pc_cons_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray, sysctl_malloc};
use crate::kern::kern_pledge::pledge_sysctl;
use crate::kern::kern_proc::{ALLPROCESS, ZOMBPROCESS, prfind};
use crate::kern::kern_prot::suser;
use crate::kern::kern_resource::{calctsru, tuagg_get_proc, tuagg_get_process};
use crate::kern::kern_rwlock::{
    rw_enter, rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write,
};
use crate::kern::kern_sched::{
    cpu_is_online, sysctl_hwblockcpu, sysctl_hwncpuonline, sysctl_hwsmt,
};
use crate::kern::kern_sensors::{sensor_find, sensordev_get};
use crate::kern::kern_sig::NOSUIDCOREDUMP;
use crate::kern::kern_synch::{refcnt_rele_wake, refcnt_take};
use crate::kern::kern_tc::{microboottime, nanoboottime, nanotime, sysctl_tc, tc_setrealtimeclock};
use crate::kern::kern_timeout::timeout_sysctl;
use crate::kern::sched_bsd;
use crate::kern::subr_autoconf::AUTOCONF_SERIAL;
use crate::kern::subr_disk::{DISK_CHANGE, DISK_COUNT, DISKLIST, duid_format, duid_iszero};
use crate::kern::subr_evcount::evcount_sysctl;
use crate::kern::subr_log::{consbufp, msgbufp};
use crate::kern::subr_pool::{POOL_DEBUG, pool_reclaim_all, sysctl_dopool};
use crate::kern::subr_prf::{SPLASSERT_CTL, panic};
use crate::kern::sys_pipe::fp_pipe;
use crate::kern::sys_socket::fp_socket;
use crate::kern::tty::{TTY_COUNT, sysctl_tty};
use crate::kern::uipc_mbuf::{MBSTAT, nmbclust_update};
use crate::kern::uipc_socket::{somaxconn, sominconn};
use crate::kern::uipc_socket2::{soassertlocked, solock_shared, sounlock_shared};
use crate::kern::vfs_bio::{BUFHIGHPAGES, bufadjust};
use crate::kern::vfs_cache::NCHSTATS;
use crate::kern::vfs_getcwd::vfs_getcwd_common;
use crate::kern::vfs_lockf::MAXLOCKSPERUID;
use crate::kern::vfs_subr::{MAXVNODES, NUMVNODES, vfs_sysctl, vref, vrele};
use crate::kern::vfs_vops::VOP_GETATTR;
use crate::machine::Machine;
use crate::machine::copy::{copyin, copyout};
use crate::machine::cpu::{Cpu, CpuInfo, cpu_info_foreach, curproc};
use crate::machine::param::MachineInfo;
use crate::machine::pmap::pmap_resident_count;
use crate::netinet::in_::IPPROTO_TCP;
use crate::netinet::in_pcb::{
    Inpcb, InpcbIterator, Inpcbtable, in_pcb_iterator, in_pcb_iterator_abort, in_pcbsolock,
    in_pcbsounlock, sotoinpcb,
};
use crate::netinet::ip_divert::DIVBTABLE;
use crate::netinet::raw_ip::RAWCBTABLE;
use crate::netinet::tcp_usrreq::TCBTABLE;
use crate::netinet::tcp_var::intotcpcb;
use crate::netinet::udp_usrreq::UDBTABLE;
use crate::sys::disk::{DS_DISKNAMELEN, Disk, Diskstats};
use crate::sys::errno::Errno;
use crate::sys::exec::PsStrings;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::file::{DTYPE_KQUEUE, DTYPE_PIPE, DTYPE_SOCKET, DTYPE_VNODE, File, frele};
use crate::sys::filedesc::{Filedesc, fdplock, fdpunlock};
use crate::sys::limits::SHRT_MAX;
use crate::sys::malloc::{M_SYSCTL, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{MT_NTYPES, Mbstat, MbstatCounters, mtod};
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::msgbuf::{MSG_MAGIC, Msgbuf};
use crate::sys::param::{MAXPATHLEN, MAXPHYS, NODEV, OpenBSD, PAGE_MASK, PAGE_SIZE};
use crate::sys::proc::{
    PS_CONTROLT, PS_EMBRYO, PS_EXITING, PS_INEXEC, PS_NOBROADCASTKILL, PS_PLEDGE, PS_SYSTEM,
    PS_ZOMBIE, Proc, ProcThrLink, Process, SDEAD, SIDL, SONPROC, SRUN, SSLEEP, SSTOP,
    THREAD_PID_OFFSET, TU_ITICKS, TU_STICKS, TU_UTICKS, Tusage,
};
use crate::sys::queue::{SlistHead, TailqHead};
use crate::sys::resource::RLIMIT_RSS;
use crate::sys::rwlock::{RW_INTR, RW_WRITE, Rwlock};
use crate::sys::sched::{CPUSTATES, CPUSTATS_ONLINE, Cpustats};
use crate::sys::sensors::{Sensor, SensorType, Sensordev};
use crate::sys::socket::{AF_INET, AF_INET6, AF_UNIX, SOCK_RAW};
use crate::sys::socketvar::{Socket, isspliced, issplicedback};
use crate::sys::syscallargs::SysSysctlArgs;
use crate::sys::sysctl::*;
use crate::sys::syslimits;
use crate::sys::systm::{
    PHYSMEM, SysArgs, kernel_assert_locked, kernel_lock, kernel_unlock, net_lock_shared,
    net_unlock_shared, sysargs,
};
use crate::sys::time::timeradd;
use crate::sys::types::{Dev, Off, Register};
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::un::SockaddrUn;
use crate::sys::unistd::_POSIX_VERSION;
use crate::sys::unpcb::{UnpRefs, sotounpcb};
use crate::sys::vnode::{GETCWD_CHECK_ACCESS, Vattr, Vnode, VnodeUn, makeimode};
use crate::unported;
use crate::uvm::uvm_extern::Vmspace;
use crate::uvm::uvm_glue::{uvm_vslock, uvm_vsunlock};
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_io::uvm_io;
use crate::uvm::uvm_km::NO_CONSTRAINT;
use crate::uvm::uvm_map::{VmMap, uvmspace_addref, uvmspace_free};
use crate::uvm::uvm_meter::uvm_sysctl;
use crate::uvm::uvm_mmap::UVM_WXABORT;
use crate::uvm::uvm_page::uvm_pagecount;
use crate::uvm::uvm_param::atop;
#[cfg(feature = "inet6")]
use crate::{
    netinet::tcp_usrreq::TCB6TABLE, netinet::udp_usrreq::UDB6TABLE,
    netinet6::ip6_divert::DIVB6TABLE, netinet6::raw_ip6::RAWIN6PCBTABLE,
};

/// `MAXPARTITIONS` (`<machine/disklabel.h>`, 16 on amd64 and arm64): number of partitions.
/// The disklabel headers are not ported.
const MAXPARTITIONS_C: i32 = 16;
/// `RAW_PART` (`<sys/disklabel.h>`): the 'c' partition.
const RAW_PART_C: i32 = 2;
/// `PLEDGE_UNVEIL` (`<sys/pledge.h>`, not ported): allow unveil().
const PLEDGE_UNVEIL: u64 = 0x0000_0010_0000_0000;
/// `BYTE_ORDER` (`<machine/endian.h>`): `LITTLE_ENDIAN` (1234) or `BIG_ENDIAN` (4321).
const BYTE_ORDER_C: i32 = if cfg!(target_endian = "little") {
    1234
} else {
    4321
};
/// `KERN_PROCSLOP`: try over estimating by 5 procs.
const KERN_PROCSLOP: usize = 5;
/// `VMMAP_MAXLEN`: arbitrary but reasonable limit for one iteration.
const VMMAP_MAXLEN: usize = MAXPHYS;

/// `int (*)(int *)`: the type of `cpu_cpuspeed`, which stores the CPU's frequency in MHz.
pub type CpuSpeedFn = fn(&mut i32) -> Result<(), Errno>;
/// `int (*)(int)`: the type of the `hw_battery_set*` hooks a battery driver provides.
pub type BatterySetFn = fn(i32) -> Result<(), Errno>;

/// `sysctl_lock`: avoids too many processes vslocking a large amount of memory at the same
/// time.
pub static SYSCTL_LOCK: Rwlock = Rwlock::new("sysctllk");
/// `sysctl_disklock`.
pub static SYSCTL_DISKLOCK: Rwlock = Rwlock::new("sysctldlk");
/// `disknames`: the `hw.disknames` string (`M_SYSCTL`), rebuilt by `sysctl_diskinit` under
/// `sysctl_disklock`; null until the first call.
static DISKNAMES: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
/// `disknameslen`: the size of the `disknames` allocation.
static DISKNAMESLEN: AtomicUsize = AtomicUsize::new(0);
/// `diskstats`: the `hw.diskstats` array (`M_SYSCTL`), under `sysctl_disklock` like
/// `disknames`.
static DISKSTATS: AtomicPtr<Diskstats> = AtomicPtr::new(ptr::null_mut());
/// `diskstatslen`: the size of the `diskstats` allocation in bytes.
static DISKSTATSLEN: AtomicUsize = AtomicUsize::new(0);

/// \[a\] `allowkmem`.
pub static ALLOWKMEM: AtomicI32 = AtomicI32::new(0);
/// `cpu_cpuspeed`: the machine's CPU frequency reader, if it has one.
pub static CPU_CPUSPEED: StaticCell<Option<CpuSpeedFn>> = StaticCell::new(None);

/// `hostname`. Protected by: `sysctl_lock` (written by `kern_sysctl_locked`).
pub static HOSTNAME: StaticCell<[u8; crate::sys::param::MAXHOSTNAMELEN]> =
    StaticCell::new([0; crate::sys::param::MAXHOSTNAMELEN]);
/// `hostnamelen`.
pub static HOSTNAMELEN: AtomicI32 = AtomicI32::new(0);
/// `domainname`. Protected by: `sysctl_lock`.
pub static DOMAINNAME: StaticCell<[u8; crate::sys::param::MAXHOSTNAMELEN]> =
    StaticCell::new([0; crate::sys::param::MAXHOSTNAMELEN]);
/// `domainnamelen`.
pub static DOMAINNAMELEN: AtomicI32 = AtomicI32::new(0);
/// `hostid`.
pub static HOSTID: AtomicI32 = AtomicI32::new(0);
/// `securelevel`: the system security level (`<sys/systm.h>` explains the levels).
pub static SECURELEVEL: AtomicI32 = AtomicI32::new(0);

/// `arg_max`: morally const, reported by `sysctl_bounded_arr`.
static ARG_MAX: AtomicI32 = AtomicI32::new(syslimits::ARG_MAX as i32);
/// `openbsd`.
static OPENBSD: AtomicI32 = AtomicI32::new(OpenBSD as i32);
/// `posix_version`.
static POSIX_VERSION: AtomicI32 = AtomicI32::new(_POSIX_VERSION as i32);
/// `ngroups_max`.
static NGROUPS_MAX: AtomicI32 = AtomicI32::new(syslimits::NGROUPS_MAX as i32);
/// `int_zero`.
static INT_ZERO: AtomicI32 = AtomicI32::new(0);
/// `int_one`.
static INT_ONE: AtomicI32 = AtomicI32::new(1);
/// `maxpartitions`.
static MAXPARTITIONS: AtomicI32 = AtomicI32::new(MAXPARTITIONS_C);
/// `raw_part`.
static RAW_PART: AtomicI32 = AtomicI32::new(RAW_PART_C);
/// `ccpu`, read-only (see the module's deviations).
static CCPU: AtomicI32 = AtomicI32::new(sched_bsd::CCPU as i32);

/// `kern_vars[]`: the `kern` integers `sysctl_bounded_arr` serves.
static KERN_VARS: [SysctlBoundedArgs; 30] = [
    SysctlBoundedArgs::readonly(KERN_OSREV, &OPENBSD),
    SysctlBoundedArgs::new(KERN_MAXVNODES, &MAXVNODES, 0, i32::MAX),
    SysctlBoundedArgs::new(KERN_MAXPROC, &MAXPROCESS, 0, i32::MAX),
    SysctlBoundedArgs::new(KERN_MAXFILES, &MAXFILES, 0, i32::MAX),
    SysctlBoundedArgs::readonly(KERN_NFILES, &NUMFILES),
    SysctlBoundedArgs::readonly(KERN_TTYCOUNT, &TTY_COUNT),
    SysctlBoundedArgs::readonly(KERN_ARGMAX, &ARG_MAX),
    SysctlBoundedArgs::readonly(KERN_POSIX1, &POSIX_VERSION),
    SysctlBoundedArgs::readonly(KERN_NGROUPS, &NGROUPS_MAX),
    SysctlBoundedArgs::readonly(KERN_JOB_CONTROL, &INT_ONE),
    SysctlBoundedArgs::readonly(KERN_SAVED_IDS, &INT_ONE),
    SysctlBoundedArgs::readonly(KERN_MAXPARTITIONS, &MAXPARTITIONS),
    SysctlBoundedArgs::readonly(KERN_RAWPARTITION, &RAW_PART),
    SysctlBoundedArgs::new(KERN_MAXTHREAD, &MAXTHREAD, 0, i32::MAX),
    SysctlBoundedArgs::readonly(KERN_NTHREADS, &NTHREADS),
    SysctlBoundedArgs::new(KERN_SOMAXCONN, &somaxconn, 0, SHRT_MAX as i32),
    SysctlBoundedArgs::new(KERN_SOMINCONN, &sominconn, 0, SHRT_MAX as i32),
    SysctlBoundedArgs::new(KERN_NOSUIDCOREDUMP, &NOSUIDCOREDUMP, 0, 3),
    SysctlBoundedArgs::readonly(KERN_FSYNC, &INT_ONE),
    // SYSVMSG, SYSVSEM, SYSVSHM: not configured.
    SysctlBoundedArgs::readonly(KERN_SYSVMSG, &INT_ZERO),
    SysctlBoundedArgs::readonly(KERN_SYSVSEM, &INT_ZERO),
    SysctlBoundedArgs::readonly(KERN_SYSVSHM, &INT_ZERO),
    SysctlBoundedArgs::readonly(KERN_FSCALE, &FSCALE),
    SysctlBoundedArgs::readonly(KERN_CCPU, &CCPU),
    SysctlBoundedArgs::readonly(KERN_NPROCS, &NPROCESSES),
    SysctlBoundedArgs::new(KERN_SPLASSERT, &SPLASSERT_CTL, 0, 3),
    SysctlBoundedArgs::new(KERN_MAXLOCKSPERUID, &MAXLOCKSPERUID, 0, i32::MAX),
    SysctlBoundedArgs::new(KERN_WXABORT, &UVM_WXABORT, 0, 1),
    SysctlBoundedArgs::readonly(KERN_NETLIVELOCKS, &INT_ZERO),
    // KERN_GLOBAL_PTRACE: PTRACE is not configured.
    SysctlBoundedArgs::readonly(KERN_AUTOCONF_SERIAL, &AUTOCONF_SERIAL),
];

/// `hw_vendor`: set by the machine when the firmware names it.
#[allow(non_upper_case_globals)] // the C's name; `HW_VENDOR` is the sysctl id
pub static hw_vendor: StaticCell<Option<&'static [u8]>> = StaticCell::new(None);
/// `hw_prod`.
#[allow(non_upper_case_globals)] // the C's name, kept beside `hw_vendor`
pub static hw_prod: StaticCell<Option<&'static [u8]>> = StaticCell::new(None);
/// `hw_uuid`.
#[allow(non_upper_case_globals)] // the C's name; `HW_UUID` is the sysctl id
pub static hw_uuid: StaticCell<Option<&'static [u8]>> = StaticCell::new(None);
/// `hw_serial`.
#[allow(non_upper_case_globals)] // the C's name, kept beside `hw_vendor`
pub static hw_serial: StaticCell<Option<&'static [u8]>> = StaticCell::new(None);
/// `hw_ver`.
#[allow(non_upper_case_globals)] // the C's name, kept beside `hw_vendor`
pub static hw_ver: StaticCell<Option<&'static [u8]>> = StaticCell::new(None);
/// `allowpowerdown`: allow the power button to shut the machine down.
pub static ALLOWPOWERDOWN: AtomicI32 = AtomicI32::new(1);
/// `hw_power`: the machine has wall power.
#[allow(non_upper_case_globals)] // the C's name; `HW_POWER` is the sysctl id
pub static hw_power: AtomicI32 = AtomicI32::new(1);

/// `byte_order`: morally const, reported by `sysctl_bounded_arr`.
static BYTE_ORDER: AtomicI32 = AtomicI32::new(BYTE_ORDER_C);

/// `hw_vars[]`.
static HW_VARS: [SysctlBoundedArgs; 6] = [
    SysctlBoundedArgs::readonly(HW_NCPU, &NCPUS),
    SysctlBoundedArgs::readonly(HW_NCPUFOUND, &NCPUSFOUND),
    SysctlBoundedArgs::readonly(HW_BYTEORDER, &BYTE_ORDER),
    SysctlBoundedArgs::readonly(HW_PAGESIZE, &UVMEXP.pagesize),
    SysctlBoundedArgs::readonly(HW_DISKCOUNT, &DISK_COUNT),
    SysctlBoundedArgs::readonly(HW_POWER, &hw_power),
];

/// `hw_battery_chargemode`.
#[allow(non_upper_case_globals)] // the C's name; `HW_BATTERY_CHARGEMODE` is the sysctl id
pub static hw_battery_chargemode: AtomicI32 = AtomicI32::new(0);
/// `hw_battery_chargestart`.
#[allow(non_upper_case_globals)] // the C's name; `HW_BATTERY_CHARGESTART` is the sysctl id
pub static hw_battery_chargestart: AtomicI32 = AtomicI32::new(0);
/// `hw_battery_chargestop`.
#[allow(non_upper_case_globals)] // the C's name; `HW_BATTERY_CHARGESTOP` is the sysctl id
pub static hw_battery_chargestop: AtomicI32 = AtomicI32::new(0);
/// `hw_battery_setchargemode`: a battery driver's setter, if one attached.
#[allow(non_upper_case_globals)] // the C's name, kept beside `hw_battery_chargemode`
pub static hw_battery_setchargemode: StaticCell<Option<BatterySetFn>> = StaticCell::new(None);
/// `hw_battery_setchargestart`.
#[allow(non_upper_case_globals)] // the C's name, kept beside `hw_battery_chargestart`
pub static hw_battery_setchargestart: StaticCell<Option<BatterySetFn>> = StaticCell::new(None);
/// `hw_battery_setchargestop`.
#[allow(non_upper_case_globals)] // the C's name, kept beside `hw_battery_chargestop`
pub static hw_battery_setchargestop: StaticCell<Option<BatterySetFn>> = StaticCell::new(None);

/// `sysctl_vslock`: takes `sysctl_lock` and wires the user buffer `[addr, addr + len)` so
/// the copies under the lock cannot sleep on a fault. On success the caller owes a
/// [`sysctl_vsunlock`].
pub fn sysctl_vslock(addr: usize, len: usize) -> Result<(), Errno> {
    rw_enter(&SYSCTL_LOCK, RW_WRITE | RW_INTR)?;
    kernel_lock();

    if addr != 0 {
        let wired = UVMEXP.wired.load(Ordering::Relaxed);
        let wiredmax = UVMEXP.wiredmax.load(Ordering::Relaxed);
        let error = if atop(len) as i64 > i64::from(wiredmax) - i64::from(wired) {
            Err(Errno::ENOMEM)
        } else {
            let Some(p) = curproc() else {
                panic(format_args!("sysctl_vslock: no curproc"));
            };
            uvm_vslock(p, addr, len, PROT_READ | PROT_WRITE)
        };
        if let Err(e) = error {
            kernel_unlock();
            rw_exit_write(&SYSCTL_LOCK);
            return Err(e);
        }
    }

    Ok(())
}

/// `sysctl_vsunlock`: undoes [`sysctl_vslock`].
pub fn sysctl_vsunlock(addr: usize, len: usize) {
    kernel_assert_locked();

    if addr != 0 {
        let Some(p) = curproc() else {
            panic(format_args!("sysctl_vsunlock: no curproc"));
        };
        uvm_vsunlock(p, addr, len);
    }
    kernel_unlock();
    rw_exit_write(&SYSCTL_LOCK);
}

/// `sysctl(2)`: reads and writes the variable `name` names. `old` (with `*oldlenp` bytes)
/// receives the old value, `new` (`newlen` bytes) is the value to set.
pub fn sys_sysctl(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSysctlArgs = sysargs(v);
    let namelen = uap.namelen.get() as usize;
    let old = uap.old.get() as usize;
    let oldlenp = uap.oldlenp.get() as usize;
    let new = uap.new.get() as usize;
    let newlen = uap.newlen.get();

    if new != 0 {
        suser(p)?;
    }
    // all top-level sysctl names are non-terminal
    if !(2..=CTL_MAXNAME).contains(&namelen) {
        return Err(Errno::EINVAL);
    }
    let mut name = [0i32; CTL_MAXNAME];
    copyin(
        uap.name.get() as usize,
        &mut name.as_bytes_mut()[..namelen * size_of::<i32>()],
    )?;
    let name = &name[..namelen];

    pledge_sysctl(p, name, new)?;

    let (dolock, f): (bool, Sysctlfn) = match name[0] {
        CTL_KERN => (false, kern_sysctl),
        CTL_HW => (false, hw_sysctl),
        CTL_NET => (false, crate::kern::uipc_domain::net_sysctl),
        CTL_VM => (true, uvm_sysctl),
        CTL_VFS => (true, vfs_sysctl),
        CTL_MACHDEP => return Err(unported!("cpu_sysctl (machdep.c)")),
        // CTL_DEBUG: DEBUG_SYSCTL is not configured.
        CTL_DDB => (false, crate::ddb::db_usrreq::ddb_sysctl),
        _ => return Err(Errno::EOPNOTSUPP),
    };

    let mut oldlen: usize = 0;
    if oldlenp != 0 {
        let mut b = [0u8; size_of::<usize>()];
        copyin(oldlenp, &mut b)?;
        oldlen = usize::from_ne_bytes(b);
    }

    let mut savelen = 0;
    if dolock {
        sysctl_vslock(old, oldlen)?;
        savelen = oldlen;
    }
    let error = f(&name[1..], old, &mut oldlen, new, newlen, p);
    if dolock {
        sysctl_vsunlock(old, savelen);
    }

    error?;
    if oldlenp != 0 {
        copyout(&oldlen.to_ne_bytes(), oldlenp)?;
    }
    Ok(())
}

/// `sysctl_bounded_arr(kern_vars, ...)`.
fn kern_vars(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    sysctl_bounded_arr(&KERN_VARS, name, oldp, oldlenp, newp, newlen)
}

/// `kern_sysctl_dirs`: the non-terminal `kern` nodes that need no lock, then the others under
/// `sysctl_lock`.
fn kern_sysctl_dirs(
    top_name: i32,
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    p: &Proc,
) -> Result<(), Errno> {
    match top_name {
        KERN_FILE => return sysctl_file(name, oldp, oldlenp, p),
        KERN_MALLOCSTATS => return sysctl_malloc(name, oldp, oldlenp, newp, newlen),
        KERN_CPTIME2 => return sysctl_cptime2(name, oldp, oldlenp, newp, newlen),
        KERN_POOL => return sysctl_dopool(name, oldp, oldlenp),
        KERN_CPUSTATS => return sysctl_cpustats(name, oldp, oldlenp, newp, newlen),
        // KERN_SYSVIPC_INFO, KERN_SEMINFO, KERN_SHMINFO: SYSV* are not configured.
        // KERN_AUDIO, KERN_VIDEO: NAUDIO and NVIDEO are 0.
        _ => {}
    }

    let savelen = *oldlenp;
    sysctl_vslock(oldp, savelen)?;
    let error = kern_sysctl_dirs_locked(top_name, name, oldp, oldlenp, newp, newlen, p);
    sysctl_vsunlock(oldp, savelen);

    error
}

/// `kern_sysctl_dirs_locked`.
fn kern_sysctl_dirs_locked(
    top_name: i32,
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    p: &Proc,
) -> Result<(), Errno> {
    match top_name {
        KERN_PROC => sysctl_doproc(name, oldp, oldlenp),
        KERN_PROC_ARGS => sysctl_proc_args(name, oldp, oldlenp, p),
        KERN_PROC_CWD => sysctl_proc_cwd(name, oldp, oldlenp, p),
        KERN_PROC_NOBROADCASTKILL => {
            sysctl_proc_nobroadcastkill(name, newp, newlen, oldp, oldlenp, p)
        }
        KERN_PROC_VMMAP => sysctl_proc_vmmap(name, oldp, oldlenp, p),
        KERN_INTRCNT => sysctl_intrcnt(name, oldp, oldlenp),
        KERN_WATCHDOG => Err(unported!("kern.watchdog: sysctl_wdog (kern_watchdog.c)")),
        KERN_EVCOUNT => evcount_sysctl(name, oldp, oldlenp, newp, newlen),
        KERN_CLOCKINTR => Err(unported!(
            "kern.clockintr: sysctl_clockintr (kern_clockintr.c)"
        )),
        KERN_TTY => sysctl_tty(name, oldp, oldlenp, newp, newlen),
        // KERN_PROF: GPROF and DDBPROF are not configured.
        KERN_TIMECOUNTER => sysctl_tc(name, oldp, oldlenp, newp, newlen),
        // KERN_WITNESSWATCH, KERN_WITNESS: WITNESS is not configured.
        _ => Err(Errno::ENOTDIR), // overloaded
    }
}

/// `kern_sysctl`: kernel related system variables.
pub fn kern_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    p: &Proc,
) -> Result<(), Errno> {
    let [top, rest @ ..] = name else {
        return Err(Errno::EINVAL);
    };

    // dispatch the non-terminal nodes first
    if !rest.is_empty() {
        return kern_sysctl_dirs(*top, rest, oldp, oldlenp, newp, newlen, p);
    }

    match *top {
        KERN_OSTYPE => return sysctl_rdstring(oldp, oldlenp, newp, OSTYPE.as_bytes()),
        KERN_OSRELEASE => return sysctl_rdstring(oldp, oldlenp, newp, OSRELEASE.as_bytes()),
        KERN_OSVERSION => return sysctl_rdstring(oldp, oldlenp, newp, OSVERSION.as_bytes()),
        KERN_VERSION => return sysctl_rdstring(oldp, oldlenp, newp, VERSION.as_bytes()),
        KERN_CONSBUF | KERN_MSGBUF => {
            if *top == KERN_CONSBUF {
                suser(p)?;
            }
            let mp = if *top == KERN_MSGBUF {
                msgbufp()
            } else {
                consbufp()
            };
            return sysctl_msgbuf(mp, oldp, oldlenp, newp);
        }
        KERN_CONSBUFSIZE | KERN_MSGBUFSIZE => {
            let mp = if *top == KERN_MSGBUFSIZE {
                msgbufp()
            } else {
                consbufp()
            };

            // deal with cases where the message buffer has become corrupted.
            let Some(mp) = mp.filter(|mp| mp.magic() == MSG_MAGIC) else {
                return Err(Errno::ENXIO);
            };
            return sysctl_rdint(oldp, oldlenp, newp, mp.bufs() as i32);
        }
        // KERN_ALLOWDT: NDT is 0.
        KERN_HOSTID => return sysctl_int(oldp, oldlenp, newp, newlen, &HOSTID),
        KERN_CLOCKRATE => return sysctl_clockrate(oldp, oldlenp, newp),
        KERN_ALLOWKMEM => {
            return sysctl_securelevel_int(oldp, oldlenp, newp, newlen, &ALLOWKMEM);
        }
        KERN_NUMVNODES => {
            // XXX numvnodes is a long
            return sysctl_rdint(
                oldp,
                oldlenp,
                newp,
                NUMVNODES.load(Ordering::Relaxed) as i32,
            );
        }
        KERN_BOOTTIME => {
            let bt = microboottime();
            return sysctl_rdstruct(oldp, oldlenp, newp, bt.as_bytes());
        }
        KERN_MAXCLUSTERS => {
            let oldval = NMBCLUST.load(Ordering::Relaxed) as i32;
            let newval = AtomicI32::new(oldval);
            let mut error = sysctl_int(oldp, oldlenp, newp, newlen, &newval);
            let newval = newval.into_inner();

            if error.is_ok() && oldval != newval {
                rw_enter_write(&SYSCTL_LOCK);
                error = nmbclust_update(i64::from(newval));
                rw_exit_write(&SYSCTL_LOCK);
            }

            return error;
        }
        KERN_MBSTAT => {
            let counters: [u64; MbstatCounters::MbsNcounters as usize] =
                core::array::from_fn(|i| MBSTAT[i].load(Ordering::Relaxed));
            let mut mbs = Mbstat::default();
            mbs.m_mtypes.copy_from_slice(&counters[..MT_NTYPES]);
            mbs.m_drops = counters[MbstatCounters::MbsDrops as usize];
            mbs.m_wait = counters[MbstatCounters::MbsWait as usize];
            mbs.m_drain = counters[MbstatCounters::MbsDrain as usize];
            mbs.m_defrag_alloc = counters[MbstatCounters::MbsDefragAlloc as usize];
            mbs.m_prepend_alloc = counters[MbstatCounters::MbsPrependAlloc as usize];
            mbs.m_pullup_alloc = counters[MbstatCounters::MbsPullupAlloc as usize];
            mbs.m_pullup_copy = counters[MbstatCounters::MbsPullupCopy as usize];
            mbs.m_pulldown_alloc = counters[MbstatCounters::MbsPulldownAlloc as usize];
            mbs.m_pulldown_copy = counters[MbstatCounters::MbsPulldownCopy as usize];
            return sysctl_rdstruct(oldp, oldlenp, newp, mbs.as_bytes());
        }
        KERN_CPTIME => {
            let mut cp_time = [0i64; CPUSTATES];
            let mut n = 0i64;

            cpu_info_foreach(&mut |ci| {
                if !cpu_is_online(ci) {
                    return;
                }

                n += 1;
                let ci_cp_time = sysctl_ci_cp_time(ci);
                for (sum, t) in cp_time.iter_mut().zip(ci_cp_time) {
                    *sum += t as i64;
                }
            });

            if n > 0 {
                for t in &mut cp_time {
                    *t /= n;
                }
            }

            return sysctl_rdstruct(oldp, oldlenp, newp, cp_time.as_bytes());
        }
        KERN_POOL_DEBUG => {
            let oldval = POOL_DEBUG.load(Ordering::Relaxed);
            let newval = AtomicI32::new(oldval);

            let error = sysctl_int(oldp, oldlenp, newp, newlen, &newval);
            let newval = newval.into_inner();
            if error.is_ok()
                && oldval != newval
                && POOL_DEBUG
                    .compare_exchange(oldval, newval, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
            {
                pool_reclaim_all();
            }

            return error;
        }
        KERN_TIMEOUT_STATS => return timeout_sysctl(oldp, oldlenp, newp, newlen),
        KERN_MAXPROC | KERN_MAXFILES | KERN_NFILES | KERN_TTYCOUNT | KERN_ARGMAX | KERN_POSIX1
        | KERN_NGROUPS | KERN_JOB_CONTROL | KERN_SAVED_IDS | KERN_FSYNC | KERN_SYSVMSG
        | KERN_SYSVSEM | KERN_SYSVSHM | KERN_SOMAXCONN | KERN_SOMINCONN | KERN_NOSUIDCOREDUMP
        | KERN_WXABORT | KERN_NETLIVELOCKS | KERN_GLOBAL_PTRACE | KERN_AUTOCONF_SERIAL
        | KERN_OSREV | KERN_MAXPARTITIONS | KERN_RAWPARTITION | KERN_MAXTHREAD | KERN_NTHREADS
        | KERN_FSCALE | KERN_CCPU | KERN_NPROCS => {
            return kern_vars(name, oldp, oldlenp, newp, newlen);
        }
        _ => {}
    }

    let savelen = *oldlenp;
    sysctl_vslock(oldp, savelen)?;
    let error = kern_sysctl_locked(name, oldp, oldlenp, newp, newlen, p);
    sysctl_vsunlock(oldp, savelen);

    error
}

/// The `KERN_MSGBUF`/`KERN_CONSBUF` case of `kern_sysctl`: the header, then the ring.
fn sysctl_msgbuf(
    mp: Option<&Msgbuf>,
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
) -> Result<(), Errno> {
    let hlen = Msgbuf::HEADER_SIZE;

    // deal with cases where the message buffer has become corrupted.
    let Some(mp) = mp.filter(|mp| mp.magic() == MSG_MAGIC) else {
        return Err(Errno::ENXIO);
    };
    if newp != 0 {
        return Err(Errno::EPERM);
    }
    let bufs = mp.bufs() as usize;
    if oldp != 0 {
        if hlen + bufs > *oldlenp {
            return Err(Errno::ENOMEM);
        }
    } else {
        return Ok(());
    }

    // mtx_enter(&log_mtx): see the module's deviations.
    let ump: [i64; 5] = [mp.magic(), mp.bufx(), mp.bufr(), mp.bufs(), mp.bufd()];

    // copy header...
    copyout(ump.as_bytes(), oldp)?;
    // ...and the data.
    let mut chunk = [0u8; 256];
    let ring = &mp.bufc()[..bufs.min(mp.bufc().len())];
    for (i, cells) in ring.chunks(chunk.len()).enumerate() {
        for (b, c) in chunk.iter_mut().zip(cells) {
            *b = c.get();
        }
        copyout(&chunk[..cells.len()], oldp + hlen + i * 256)?;
    }

    Ok(())
}

/// `kern_sysctl_locked`: the `kern` leaves served under `sysctl_lock`.
fn kern_sysctl_locked(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    p: &Proc,
) -> Result<(), Errno> {
    match name[0] {
        KERN_SECURELVL => sysctl_securelevel(oldp, oldlenp, newp, newlen, p),
        KERN_HOSTNAME => {
            // SAFETY: `sysctl_lock` is held for writing (`kern_sysctl` took it through
            // `sysctl_vslock`), which is what serialises every access to `hostname`.
            let hostname = unsafe { HOSTNAME.get_mut() };
            let error = sysctl_tstring(oldp, oldlenp, newp, newlen, hostname);
            if newp != 0 && error.is_ok() {
                HOSTNAMELEN.store(newlen as i32, Ordering::Relaxed);
            }
            error
        }
        KERN_DOMAINNAME => {
            let error = if SECURELEVEL.load(Ordering::Relaxed) >= 1
                && DOMAINNAMELEN.load(Ordering::Relaxed) != 0
                && newp != 0
            {
                Err(Errno::EPERM)
            } else {
                // SAFETY: as for `hostname`: `sysctl_lock` is held for writing.
                let domainname = unsafe { DOMAINNAME.get_mut() };
                sysctl_tstring(oldp, oldlenp, newp, newlen, domainname)
            };
            if newp != 0 && error.is_ok() {
                DOMAINNAMELEN.store(newlen as i32, Ordering::Relaxed);
            }
            error
        }
        KERN_NCHSTATS => {
            let mut bytes = [0u8; 12 * size_of::<u64>()];
            for (chunk, v) in bytes.chunks_mut(size_of::<u64>()).zip(NCHSTATS.snapshot()) {
                chunk.copy_from_slice(&v.to_ne_bytes());
            }
            sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
        }
        KERN_FORKSTAT => {
            // struct forkstat: four ints, then four uint64_t.
            let mut fs = [0u8; 48];
            let cnt = [
                FORKSTAT.cntfork.load(Ordering::Relaxed),
                FORKSTAT.cntvfork.load(Ordering::Relaxed),
                FORKSTAT.cnttfork.load(Ordering::Relaxed),
                FORKSTAT.cntkthread.load(Ordering::Relaxed),
            ];
            let siz = [
                FORKSTAT.sizfork.load(Ordering::Relaxed),
                FORKSTAT.sizvfork.load(Ordering::Relaxed),
                FORKSTAT.siztfork.load(Ordering::Relaxed),
                FORKSTAT.sizkthread.load(Ordering::Relaxed),
            ];
            fs[..16].copy_from_slice(cnt.as_bytes());
            fs[16..].copy_from_slice(siz.as_bytes());
            sysctl_rdstruct(oldp, oldlenp, newp, &fs)
        }
        KERN_STACKGAPRANDOM => {
            use crate::kern::kern_exec::stackgap_random;

            let stackgap = AtomicI32::new(stackgap_random.load(Ordering::Relaxed));
            sysctl_int(oldp, oldlenp, newp, newlen, &stackgap)?;
            let stackgap = stackgap.into_inner();
            // Safety harness.
            let alignbytes =
                <crate::machine::Machine as crate::machine::param::MachineParam>::ALIGNBYTES;
            let maxssiz = <crate::machine::Machine as crate::machine::VmParam>::MAXSSIZ;
            if (stackgap < alignbytes as i32 && stackgap != 0)
                || (stackgap.wrapping_sub(1) & stackgap) != 0
                || stackgap as i64 >= maxssiz as i64
            {
                return Err(Errno::EINVAL);
            }
            stackgap_random.store(stackgap, Ordering::Relaxed);
            Ok(())
        }
        KERN_CACHEPCT => {
            use crate::conf::param::{bufcachepercent, bufpages};

            let opct = bufcachepercent.load(Ordering::Relaxed);
            sysctl_int(oldp, oldlenp, newp, newlen, &bufcachepercent)?;
            let pct = bufcachepercent.load(Ordering::Relaxed);
            if !(5..=90).contains(&pct) {
                bufcachepercent.store(opct, Ordering::Relaxed);
                return Err(Errno::EINVAL);
            }
            let pages = uvm_pagecount(&NO_CONSTRAINT) as u64;
            if pct != opct {
                let pgs = (pct as u64 * pages / 100) as i32;
                bufadjust(i64::from(pgs)); // adjust bufpages
                // set high water mark
                BUFHIGHPAGES.store(bufpages.load(Ordering::Relaxed), Ordering::Relaxed);
            }
            Ok(())
        }
        KERN_PFSTATUS => crate::net::pf_ioctl::pf_sysctl(oldp, oldlenp, newp, newlen),
        KERN_CONSDEV => {
            let dev = cn_tab().map_or(NODEV, |cn| cn.cn_dev.get());
            sysctl_rdstruct(oldp, oldlenp, newp, dev.as_bytes())
        }
        KERN_UTC_OFFSET => sysctl_utc_offset(oldp, oldlenp, newp, newlen),
        _ => kern_vars(name, oldp, oldlenp, newp, newlen),
    }
}

/// `hw_sysctl`: hardware related system variables.
pub fn hw_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    p: &Proc,
) -> Result<(), Errno> {
    let Some(&top) = name.first() else {
        return Err(Errno::EINVAL);
    };

    // all sysctl names at this level except sensors and battery are terminal
    if top != HW_SENSORS && top != HW_BATTERY && name.len() != 1 {
        return Err(Errno::ENOTDIR); // overloaded
    }

    let physmem = PHYSMEM.load(Ordering::Relaxed) as i64;
    let wired = i64::from(UVMEXP.wired.load(Ordering::Relaxed));

    match top {
        HW_MACHINE => sysctl_rdstring(oldp, oldlenp, newp, Machine::MACHINE.as_bytes()),
        HW_MODEL => Err(unported!(
            "hw.model: cpu_model (amd64 identcpu.c, arm64 cpu.c)"
        )),
        HW_NCPUONLINE => sysctl_rdint(oldp, oldlenp, newp, sysctl_hwncpuonline() as i32),
        HW_PHYSMEM => sysctl_rdint(oldp, oldlenp, newp, (physmem * PAGE_SIZE as i64) as i32),
        HW_USERMEM => sysctl_rdint(
            oldp,
            oldlenp,
            newp,
            ((physmem - wired) * PAGE_SIZE as i64) as i32,
        ),
        HW_SENSORS => sysctl_sensors(&name[1..], oldp, oldlenp, newp, newlen),
        HW_DISKNAMES | HW_DISKSTATS | HW_CPUSPEED | HW_SETPERF | HW_PERFPOLICY | HW_BATTERY
        | HW_ALLOWPOWERDOWN | HW_UCOMNAMES | HW_SMT | HW_BLOCKCPU => {
            let savelen = *oldlenp;
            sysctl_vslock(oldp, savelen)?;
            let err = hw_sysctl_locked(name, oldp, oldlenp, newp, newlen, p);
            sysctl_vsunlock(oldp, savelen);
            err
        }
        HW_VENDOR => hw_string(&hw_vendor, oldp, oldlenp, newp),
        HW_PRODUCT => hw_string(&hw_prod, oldp, oldlenp, newp),
        HW_VERSION => hw_string(&hw_ver, oldp, oldlenp, newp),
        HW_SERIALNO => hw_string(&hw_serial, oldp, oldlenp, newp),
        HW_UUID => hw_string(&hw_uuid, oldp, oldlenp, newp),
        HW_PHYSMEM64 => sysctl_rdquad(oldp, oldlenp, newp, physmem * PAGE_SIZE as i64),
        HW_USERMEM64 => sysctl_rdquad(oldp, oldlenp, newp, (physmem - wired) * PAGE_SIZE as i64),
        _ => sysctl_bounded_arr(&HW_VARS, name, oldp, oldlenp, newp, newlen),
    }
}

/// The `HW_VENDOR`.. `HW_UUID` cases of `hw_sysctl`: the string, or `EOPNOTSUPP` when the
/// machine recorded none.
fn hw_string(
    var: &StaticCell<Option<&'static [u8]>>,
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
) -> Result<(), Errno> {
    // SAFETY: the machine writes these once, at attach time on the boot CPU, before any
    // process can call sysctl(2).
    match unsafe { var.read() } {
        Some(s) => sysctl_rdstring(oldp, oldlenp, newp, s),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `hw_sysctl_locked`: the `hw` nodes served under `sysctl_lock`.
fn hw_sysctl_locked(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    p: &Proc,
) -> Result<(), Errno> {
    match name[0] {
        HW_DISKNAMES => {
            sysctl_diskinit(false, p)?;
            rw_enter_read(&SYSCTL_DISKLOCK);
            let names = match NonNull::new(DISKNAMES.load(Ordering::Relaxed)) {
                // SAFETY: the `disknameslen` bytes `sysctl_diskinit` allocated, which it
                // replaces only under the lock taken for reading here.
                Some(n) => unsafe {
                    core::slice::from_raw_parts(n.as_ptr(), DISKNAMESLEN.load(Ordering::Relaxed))
                },
                None => &[],
            };
            let err = sysctl_rdstring(oldp, oldlenp, newp, names);
            rw_exit_read(&SYSCTL_DISKLOCK);
            err
        }
        HW_DISKSTATS => {
            sysctl_diskinit(true, p)?;
            rw_enter_read(&SYSCTL_DISKLOCK);
            let len = (DISK_COUNT.load(Ordering::Relaxed).max(0) as usize * size_of::<Diskstats>())
                .min(DISKSTATSLEN.load(Ordering::Relaxed));
            let stats = match NonNull::new(DISKSTATS.load(Ordering::Relaxed)) {
                // SAFETY: `len` bytes of the `diskstatslen`-byte array `sysctl_diskinit`
                // allocated and filled (zeroed, padding a member: every byte initialised),
                // which it replaces only under the lock taken for reading here.
                Some(sdk) => unsafe { core::slice::from_raw_parts(sdk.as_ptr().cast::<u8>(), len) },
                None => &[],
            };
            let err = sysctl_rdstruct(oldp, oldlenp, newp, stats);
            rw_exit_read(&SYSCTL_DISKLOCK);
            err
        }
        HW_CPUSPEED => {
            // SAFETY: the machine sets `cpu_cpuspeed` while attaching its CPUs, before any
            // process runs.
            let Some(cpu_cpuspeed) = (unsafe { CPU_CPUSPEED.read() }) else {
                return Err(Errno::EOPNOTSUPP);
            };
            let mut cpuspeed = 0;
            cpu_cpuspeed(&mut cpuspeed)?;
            sysctl_rdint(oldp, oldlenp, newp, cpuspeed)
        }
        HW_SETPERF => Err(unported!("hw.setperf: sysctl_hwsetperf (sched_bsd.c)")),
        HW_PERFPOLICY => Err(unported!(
            "hw.perfpolicy: sysctl_hwperfpolicy (sched_bsd.c)"
        )),
        HW_ALLOWPOWERDOWN => sysctl_securelevel_int(oldp, oldlenp, newp, newlen, &ALLOWPOWERDOWN),
        HW_UCOMNAMES => {
            // NUCOM is 0: sysctl_ucominit is not called.
            sysctl_rdstring(oldp, oldlenp, newp, b"")
        }
        HW_SMT => sysctl_hwsmt(oldp, oldlenp, newp, newlen),
        HW_BLOCKCPU => sysctl_hwblockcpu(oldp, oldlenp, newp, newlen),
        HW_BATTERY => sysctl_hwbattery(&name[1..], oldp, oldlenp, newp, newlen),
        _ => Err(Errno::EOPNOTSUPP),
    }
}

/// The common body of `sysctl_hwchargemode`, `sysctl_hwchargestart` and
/// `sysctl_hwchargestop`.
#[allow(clippy::too_many_arguments)] // the C's arguments plus the variable, setter and bounds
fn sysctl_hwcharge(
    var: &AtomicI32,
    setter: &StaticCell<Option<BatterySetFn>>,
    minimum: i32,
    maximum: i32,
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    // SAFETY: a battery driver sets its setter at attach time, before any process runs.
    let Some(set) = (unsafe { setter.read() }) else {
        return Err(Errno::EOPNOTSUPP);
    };
    let value = AtomicI32::new(var.load(Ordering::Relaxed));

    sysctl_int_bounded(oldp, oldlenp, newp, newlen, &value, minimum, maximum)?;

    if newp != 0 {
        set(value.into_inner())?;
    }
    Ok(())
}

/// `sysctl_hwchargemode`.
pub fn sysctl_hwchargemode(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    sysctl_hwcharge(
        &hw_battery_chargemode,
        &hw_battery_setchargemode,
        -1,
        1,
        oldp,
        oldlenp,
        newp,
        newlen,
    )
}

/// `sysctl_hwchargestart`.
pub fn sysctl_hwchargestart(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    sysctl_hwcharge(
        &hw_battery_chargestart,
        &hw_battery_setchargestart,
        0,
        100,
        oldp,
        oldlenp,
        newp,
        newlen,
    )
}

/// `sysctl_hwchargestop`.
pub fn sysctl_hwchargestop(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    sysctl_hwcharge(
        &hw_battery_chargestop,
        &hw_battery_setchargestop,
        0,
        100,
        oldp,
        oldlenp,
        newp,
        newlen,
    )
}

/// `sysctl_hwbattery`: the `hw.battery` node.
pub fn sysctl_hwbattery(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let [mib] = name else {
        return Err(Errno::ENOTDIR);
    };

    match *mib {
        HW_BATTERY_CHARGEMODE => sysctl_hwchargemode(oldp, oldlenp, newp, newlen),
        HW_BATTERY_CHARGESTART => sysctl_hwchargestart(oldp, oldlenp, newp, newlen),
        HW_BATTERY_CHARGESTOP => sysctl_hwchargestop(oldp, oldlenp, newp, newlen),
        _ => Err(Errno::EOPNOTSUPP),
    }
}

/// `sysctl_int_lower`: reads, or writes that lower the value.
pub fn sysctl_int_lower(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    valp: &AtomicI32,
) -> Result<(), Errno> {
    if oldp != 0 && *oldlenp < size_of::<i32>() {
        return Err(Errno::ENOMEM);
    }
    if newp != 0 && newlen != size_of::<i32>() {
        return Err(Errno::EINVAL);
    }
    *oldlenp = size_of::<i32>();

    if newp != 0 {
        let mut b = [0u8; 4];
        copyin(newp, &mut b)?;
        let newval = i32::from_ne_bytes(b);
        let mut oldval = valp.load(Ordering::Relaxed);
        loop {
            if (oldval as u32) < (newval as u32) {
                return Err(Errno::EPERM); // do not allow raising
            }
            match valp.compare_exchange(oldval, newval, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(current) => oldval = current,
            }
        }

        if oldp != 0 {
            // new value has been set although user gets error
            copyout(&oldval.to_ne_bytes(), oldp)?;
        }
    } else if oldp != 0 {
        let oldval = valp.load(Ordering::Relaxed);

        copyout(&oldval.to_ne_bytes(), oldp)?;
    }

    Ok(())
}

/// `sysctl_int`: validates parameters and gets the old / sets the new value of an
/// integer-valued sysctl.
pub fn sysctl_int(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    valp: &AtomicI32,
) -> Result<(), Errno> {
    sysctl_int_bounded(oldp, oldlenp, newp, newlen, valp, i32::MIN, i32::MAX)
}

/// `sysctl_rdint`: as [`sysctl_int`], but read-only.
pub fn sysctl_rdint(oldp: usize, oldlenp: &mut usize, newp: usize, val: i32) -> Result<(), Errno> {
    if oldp != 0 && *oldlenp < size_of::<i32>() {
        return Err(Errno::ENOMEM);
    }
    if newp != 0 {
        return Err(Errno::EPERM);
    }
    *oldlenp = size_of::<i32>();
    if oldp != 0 {
        copyout(&val.to_ne_bytes(), oldp)?;
    }
    Ok(())
}

/// `sysctl_securelevel`: `kern.securelevel`, which only init (pid 1) may lower once raised.
fn sysctl_securelevel(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    p: &Proc,
) -> Result<(), Errno> {
    if oldp != 0 && *oldlenp < size_of::<i32>() {
        return Err(Errno::ENOMEM);
    }
    if newp != 0 && newlen != size_of::<i32>() {
        return Err(Errno::EINVAL);
    }
    *oldlenp = size_of::<i32>();

    if newp != 0 {
        let mut b = [0u8; 4];
        copyin(newp, &mut b)?;
        let newval = i32::from_ne_bytes(b);
        let mut oldval = SECURELEVEL.load(Ordering::Relaxed);
        loop {
            if (oldval > 0 || newval < -1) && newval < oldval && p.process().ps_pid.get() != 1 {
                return Err(Errno::EPERM);
            }
            match SECURELEVEL.compare_exchange(oldval, newval, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(current) => oldval = current,
            }
        }

        if oldp != 0 {
            // new value has been set although user gets error
            copyout(&oldval.to_ne_bytes(), oldp)?;
        }
    } else if oldp != 0 {
        let oldval = SECURELEVEL.load(Ordering::Relaxed);

        copyout(&oldval.to_ne_bytes(), oldp)?;
    }

    Ok(())
}

/// `sysctl_securelevel_int`: [`sysctl_rdint`] or [`sysctl_int`] according to
/// `securelevel`.
pub fn sysctl_securelevel_int(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    valp: &AtomicI32,
) -> Result<(), Errno> {
    if SECURELEVEL.load(Ordering::Relaxed) > 0 {
        return sysctl_rdint(oldp, oldlenp, newp, valp.load(Ordering::Relaxed));
    }
    sysctl_int(oldp, oldlenp, newp, newlen, valp)
}

/// `sysctl_int_bounded`: read-only or bounded integer values. `minimum > maximum` makes the
/// value read-only; both bounds are inclusive.
pub fn sysctl_int_bounded(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    valp: &AtomicI32,
    minimum: i32,
    maximum: i32,
) -> Result<(), Errno> {
    // read only
    if newp != 0 && minimum > maximum {
        return Err(Errno::EPERM);
    }

    if oldp != 0 && *oldlenp < size_of::<i32>() {
        return Err(Errno::ENOMEM);
    }
    if newp != 0 && newlen != size_of::<i32>() {
        return Err(Errno::EINVAL);
    }
    *oldlenp = size_of::<i32>();

    // copyin() may sleep, call it first
    let mut newval = 0;
    if newp != 0 {
        let mut b = [0u8; 4];
        copyin(newp, &mut b)?;
        newval = i32::from_ne_bytes(b);
        // outside limits
        if newval < minimum || maximum < newval {
            return Err(Errno::EINVAL);
        }
    }
    if oldp != 0 {
        let oldval = if newp != 0 {
            valp.swap(newval, Ordering::Relaxed)
        } else {
            valp.load(Ordering::Relaxed)
        };
        // new value has been set although user gets error
        copyout(&oldval.to_ne_bytes(), oldp)?;
    } else if newp != 0 {
        valp.store(newval, Ordering::Relaxed);
    }

    Ok(())
}

/// `sysctl_bounded_arr`: an array of read-only or bounded integer values, looked up by the
/// one remaining name component.
pub fn sysctl_bounded_arr(
    valpp: &[SysctlBoundedArgs],
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let [mib] = name else {
        return Err(Errno::ENOTDIR);
    };
    match valpp.iter().find(|v| v.mib == *mib) {
        Some(v) => sysctl_int_bounded(oldp, oldlenp, newp, newlen, v.var, v.minimum, v.maximum),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `sysctl_rdquad`: validates parameters and gets the old value of a 64-bit sysctl.
pub fn sysctl_rdquad(oldp: usize, oldlenp: &mut usize, newp: usize, val: i64) -> Result<(), Errno> {
    if oldp != 0 && *oldlenp < size_of::<i64>() {
        return Err(Errno::ENOMEM);
    }
    if newp != 0 {
        return Err(Errno::EPERM);
    }
    *oldlenp = size_of::<i64>();
    if oldp != 0 {
        copyout(&val.to_ne_bytes(), oldp)?;
    }
    Ok(())
}

/// `sysctl_string`: validates parameters and gets the old / sets the new value of a
/// string-valued sysctl. `str` is the variable's whole buffer (`maxlen` in C).
pub fn sysctl_string(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    str: &mut [u8],
) -> Result<(), Errno> {
    sysctl__string(oldp, oldlenp, newp, newlen, str, false)
}

/// `sysctl_tstring`: as [`sysctl_string`], but a short `old` buffer gets a truncated,
/// NUL-terminated copy instead of `ENOMEM`.
pub fn sysctl_tstring(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    str: &mut [u8],
) -> Result<(), Errno> {
    sysctl__string(oldp, oldlenp, newp, newlen, str, true)
}

/// `sysctl__string`: the body of [`sysctl_string`] and [`sysctl_tstring`].
#[allow(non_snake_case)] // the C's name, with its double underscore
pub fn sysctl__string(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    str: &mut [u8],
    trunc: bool,
) -> Result<(), Errno> {
    let maxlen = str.len();
    let slen = strnlen(str, maxlen);
    let mut len = slen + 1;
    if oldp != 0 && *oldlenp < len && (!trunc || *oldlenp == 0) {
        return Err(Errno::ENOMEM);
    }
    if newp != 0 && newlen >= maxlen {
        return Err(Errno::EINVAL);
    }
    let mut error = Ok(());
    if oldp != 0 {
        if trunc && *oldlenp < len {
            len = *oldlenp;
            error = copyout(&str[..len - 1], oldp);
            if error.is_ok() {
                error = copyout(&[0], oldp + len - 1);
            }
        } else {
            error = copyout_cstr(&str[..slen], oldp);
        }
    }
    *oldlenp = len;
    if error.is_ok() && newp != 0 {
        error = copyin(newp, &mut str[..newlen]);
        str[newlen] = 0;
    }
    error
}

/// `copyout(str, oldp, strlen(str) + 1)`: the bytes, then the NUL.
fn copyout_cstr(s: &[u8], uaddr: usize) -> Result<(), Errno> {
    copyout(s, uaddr)?;
    copyout(&[0], uaddr + s.len())
}

/// `sysctl_rdstring`: as [`sysctl_string`], but read-only. `str` ends at its first NUL or
/// at its end.
pub fn sysctl_rdstring(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    str: &[u8],
) -> Result<(), Errno> {
    let slen = strnlen(str, str.len());
    let len = slen + 1;
    if oldp != 0 && *oldlenp < len {
        return Err(Errno::ENOMEM);
    }
    if newp != 0 {
        return Err(Errno::EPERM);
    }
    *oldlenp = len;
    if oldp != 0 {
        copyout_cstr(&str[..slen], oldp)?;
    }
    Ok(())
}

/// `sysctl_struct`: validates parameters and gets the old / sets the new value of a
/// structure-valued sysctl. `sp` is the structure's bytes.
pub fn sysctl_struct(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    sp: &mut [u8],
) -> Result<(), Errno> {
    let len = sp.len();

    if oldp != 0 && *oldlenp < len {
        return Err(Errno::ENOMEM);
    }
    if newp != 0 && newlen > len {
        return Err(Errno::EINVAL);
    }
    let mut error = Ok(());
    if oldp != 0 {
        *oldlenp = len;
        error = copyout(sp, oldp);
    }
    if error.is_ok() && newp != 0 {
        error = copyin(newp, sp);
    }
    error
}

/// `sysctl_rdstruct`: validates parameters and gets the old value of a structure-valued
/// sysctl.
pub fn sysctl_rdstruct(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    sp: &[u8],
) -> Result<(), Errno> {
    let len = sp.len();

    if oldp != 0 && *oldlenp < len {
        return Err(Errno::ENOMEM);
    }
    if newp != 0 {
        return Err(Errno::EPERM);
    }
    *oldlenp = len;
    if oldp != 0 {
        copyout(sp, oldp)?;
    }
    Ok(())
}

/// `PTRTOINT64(vp->v_un.vu_socket)`: whatever pointer the vnode's union holds.
fn vnode_un_ptr(un: VnodeUn) -> u64 {
    match un {
        VnodeUn::None => 0,
        VnodeUn::Mountedhere(mp) => ptrtoint64(mp),
        VnodeUn::Socket(so) => ptrtoint64(so),
        VnodeUn::Specinfo(si) => ptrtoint64(si),
        VnodeUn::Fifoinfo(fi) => ptrtoint64(fi),
    }
}

/// The `memcpy(kf->unp_path, un->sun_path, un->sun_len - offsetof(...))` of `fill_file`:
/// `addr` is the bytes of a bound `struct sockaddr_un` (its mbuf's `m_len`). The copy is
/// bounded by both buffers.
fn unp_path_copy(dst: &mut [u8; KI_UNPPATHLEN], addr: &[u8]) {
    let off = SockaddrUn::PATH_OFFSET;
    let Some(&sun_len) = addr.first() else {
        return;
    };
    let len = usize::from(sun_len)
        .saturating_sub(off)
        .min(addr.len().saturating_sub(off))
        .min(KI_UNPPATHLEN);
    dst[..len].copy_from_slice(&addr[off..off + len]);
}

/// `fill_file`: one `kinfo_file` for the file `fp`, or (without a file) for the vnode `vp`
/// a process uses as its text, current or root directory (`fd` is then `KERN_FILE_TEXT`,
/// ...), or for the socket `so` of a protocol control block table (locked by the caller).
/// `fdp` and `pr` are the descriptor table and the process the descriptor is found through,
/// for `KERN_FILE_BYPID`/`KERN_FILE_BYUID`.
#[allow(clippy::too_many_arguments)] // the C's arguments
pub fn fill_file(
    kf: &mut KinfoFile,
    fp: Option<&File>,
    fdp: Option<&Filedesc>,
    fd: i32,
    vp: Option<&'static Vnode>,
    pr: Option<&Process>,
    p: &Proc,
    so: Option<&Socket>,
    show_pointers: bool,
) {
    *kf = KinfoFile::zeroed();

    kf.fd_fd = fd; // might not really be an fd

    if let Some(fp) = fp {
        // SAFETY: an open file holds a reference to its credentials from `fnew` until
        // `fdrop`, and the caller holds a reference to the file.
        let cred = unsafe { &*fp.f_cred.get() };
        if show_pointers {
            kf.f_fileaddr = ptrtoint64(fp);
        }
        kf.f_flag = fp.f_flag.load(Ordering::Relaxed);
        kf.f_iflags = fp.f_iflags.load(Ordering::Relaxed);
        kf.f_type = fp.f_type.get() as u32;
        kf.f_count = fp.f_count.load(Ordering::Relaxed);
        if show_pointers {
            kf.f_ucred = ptrtoint64(fp.f_cred.get());
        }
        kf.f_uid = cred.cr_uid.get();
        kf.f_gid = cred.cr_gid.get();
        if show_pointers {
            kf.f_ops = fp.f_ops.get().map_or(0, |ops| ptrtoint64(ops));
        }
        if show_pointers {
            kf.f_data = ptrtoint64(fp.f_data.get());
        }
        kf.f_usecount = 0;

        if suser(p).is_ok() || p.ucred().cr_uid.get() == cred.cr_uid.get() {
            mtx_enter(&fp.f_mtx);
            kf.f_offset = fp.f_offset.get() as u64;
            kf.f_rxfer = fp.f_rxfer.get();
            kf.f_rwfer = fp.f_wxfer.get();
            kf.f_seek = fp.f_seek.get();
            kf.f_rbytes = fp.f_rbytes.get();
            kf.f_wbytes = fp.f_wbytes.get();
            mtx_leave(&fp.f_mtx);
        } else {
            kf.f_offset = -1i64 as u64;
        }
    } else if vp.is_some() {
        // fake it
        kf.f_type = DTYPE_VNODE as u32;
        kf.f_flag = FREAD as u32;
        if fd == KERN_FILE_TRACE {
            kf.f_flag |= FWRITE as u32;
        }
    } else if so.is_some() {
        // fake it
        kf.f_type = DTYPE_SOCKET as u32;
    }

    // information about the object associated with this file
    match (kf.f_type as i32, fp) {
        (DTYPE_VNODE, _) => {
            let vp = match (fp, vp) {
                (Some(fp), _) => Some(fp.vnode()),
                (None, vp) => vp,
            };
            if let Some(vp) = vp {
                fill_file_vnode(kf, vp, p, show_pointers);
            }
        }
        (DTYPE_SOCKET, _) => {
            // if so is passed as parameter it is already locked
            let (so, locked) = match (so, fp) {
                (Some(so), _) => (Some(so), false),
                (None, Some(fp)) => {
                    let so = fp_socket(fp);
                    solock_shared(so);
                    (Some(so), true)
                }
                (None, None) => (None, false),
            };
            if let Some(so) = so {
                fill_file_socket(kf, so, show_pointers);
                if locked {
                    sounlock_shared(so);
                }
            }
        }
        (DTYPE_PIPE, Some(fp)) => {
            let pipe = fp_pipe(fp);
            if show_pointers {
                kf.pipe_peer = ptrtoint64(pipe.pipe_peer.get());
            }
            kf.pipe_state = pipe.pipe_state.get();
        }
        (DTYPE_KQUEUE, Some(fp)) => {
            let kqi = fp_kqueue(fp);
            kf.kq_count = kqi.kq_count.get() as u32;
            kf.kq_state = kqi.kq_state.get() as u32;
        }
        _ => {}
    }

    // per-process information for KERN_FILE_BY[PU]ID
    if let Some(pr) = pr {
        let cred = pr.ucred();
        kf.p_pid = pr.ps_pid.get() as u32;
        kf.p_uid = cred.cr_uid.get();
        kf.p_gid = cred.cr_gid.get();
        kf.p_tid = -1i32 as u32;
        strlcpy(&mut kf.p_comm, pr.comm());
    }
    if let Some(fdp) = fdp {
        fdplock(fdp);
        kf.fd_ofileflags = u32::from(fdp.ofileflags(fd as usize));
        fdpunlock(fdp);
    }
}

/// The `DTYPE_VNODE` case of `fill_file`.
fn fill_file_vnode(kf: &mut KinfoFile, vp: &'static Vnode, p: &Proc, show_pointers: bool) {
    if show_pointers {
        kf.v_un = vnode_un_ptr(vp.v_un.get());
    }
    kf.v_type = vp.v_type.get() as u32;
    kf.v_tag = vp.v_tag.get() as u32;
    kf.v_flag = vp.v_flag.get();
    if show_pointers {
        kf.v_data = ptrtoint64(vp.v_data.get());
    }
    if show_pointers {
        kf.v_mount = vp.v_mount.get().map_or(0, |mp| ptrtoint64(mp));
    }
    if let Some(mp) = vp.v_mount.get() {
        let (name, len) = mp.mntonname();
        strlcpy(&mut kf.f_mntonname, &name[..len]);
    }

    let mut va = Vattr::new();
    if VOP_GETATTR(vp, &mut va, p.p_ucred.get(), p).is_ok() {
        kf.va_fileid = va.va_fileid;
        kf.va_mode = makeimode(va.va_type, va.va_mode);
        kf.va_size = va.va_size;
        kf.va_rdev = va.va_rdev as u32;
        kf.va_fsid = (va.va_fsid & 0xffff_ffff) as u32;
        kf.va_nlink = va.va_nlink;
    }
}

/// The `DTYPE_SOCKET` case of `fill_file`, the socket locked.
fn fill_file_socket(kf: &mut KinfoFile, so: &Socket, show_pointers: bool) {
    kf.so_type = so.so_type.get() as u32;
    kf.so_state = so.so_state.get() | so.so_snd.sb_state.get() | so.so_rcv.sb_state.get();
    kf.so_pcb = if show_pointers {
        ptrtoint64(so.so_pcb.get())
    } else {
        -1i64 as u64
    };
    kf.so_protocol = so.so_proto.pr_protocol as u32;
    kf.so_family = so.dom_family() as u32;
    kf.so_rcv_cc = so.so_rcv.sb_cc.get();
    kf.so_snd_cc = so.so_snd.sb_cc.get();
    if isspliced(so) {
        if let Some(sp) = so.so_sp.get() {
            if show_pointers {
                kf.so_splice = sp.ssp_socket.get().map_or(0, |s| ptrtoint64(s));
            }
            kf.so_splicelen = sp.ssp_len.get();
        }
    } else if issplicedback(so) {
        kf.so_splicelen = -1;
    }
    if so.so_pcb.get().is_null() {
        return;
    }
    let family = so.dom_family();
    if family == i32::from(AF_INET) {
        let Some(inpcb) = sotoinpcb(so) else {
            return;
        };
        soassertlocked(so);
        if show_pointers {
            kf.inp_ppcb = ptrtoint64(inpcb.inp_ppcb.get());
        }
        kf.inp_lport = u32::from(inpcb.inp_lport.get());
        kf.inp_laddru[0] = inpcb.inp_laddr.get().s_addr;
        kf.inp_fport = u32::from(inpcb.inp_fport.get());
        kf.inp_faddru[0] = inpcb.inp_faddr.get().s_addr;
        kf.inp_rtableid = inpcb.inp_rtableid.get();
        if so.so_type.get() == SOCK_RAW {
            kf.inp_proto = u32::from(inpcb.inp_ip.get().ip_p);
        }
        if i32::from(so.so_proto.pr_protocol) == IPPROTO_TCP {
            fill_file_tcpcb(kf, inpcb);
        }
    } else if family == i32::from(AF_INET6) {
        let Some(inpcb) = sotoinpcb(so) else {
            return;
        };
        soassertlocked(so);
        if show_pointers {
            kf.inp_ppcb = ptrtoint64(inpcb.inp_ppcb.get());
        }
        kf.inp_lport = u32::from(inpcb.inp_lport.get());
        let laddr6 = inpcb.inp_laddr6.get();
        for (i, w) in kf.inp_laddru.iter_mut().enumerate() {
            *w = laddr6.s6_addr32(i);
        }
        kf.inp_fport = u32::from(inpcb.inp_fport.get());
        let faddr6 = inpcb.inp_faddr6.get();
        for (i, w) in kf.inp_faddru.iter_mut().enumerate() {
            *w = faddr6.s6_addr32(i);
        }
        kf.inp_rtableid = inpcb.inp_rtableid.get();
        if so.so_type.get() == SOCK_RAW {
            kf.inp_proto = u32::from(inpcb.inp_ipv6.get().ip6_nxt);
        }
        if i32::from(so.so_proto.pr_protocol) == IPPROTO_TCP {
            fill_file_tcpcb(kf, inpcb);
        }
    } else if family == i32::from(AF_UNIX) {
        let Some(unpcb) = sotounpcb(so) else {
            return;
        };
        kf.f_msgcount = unpcb.unp_msgcount.get() as u32;
        if show_pointers {
            kf.unp_conn = unpcb.unp_conn.get().map_or(0, |u| ptrtoint64(u));
            kf.unp_refs = unpcb.unp_refs.first().map_or(0, |u| ptrtoint64(u));
            kf.unp_nextref = SlistHead::<UnpRefs>::next(unpcb).map_or(0, |u| ptrtoint64(u));
            kf.v_un = unpcb.unp_vnode.get().map_or(0, |v| ptrtoint64(v));
            kf.unp_addr = unpcb.unp_addr.get().map_or(0, |m| ptrtoint64(m));
        }
        if let Some(m) = unpcb.unp_addr.get() {
            // SAFETY: a bound address mbuf holds `m_len` bytes of `struct sockaddr_un` and
            // lives while the control block is bound; the caller holds the socket lock.
            let addr = unsafe {
                core::slice::from_raw_parts(mtod::<u8>(m).cast_const(), m.m_len().get() as usize)
            };
            unp_path_copy(&mut kf.unp_path, addr);
        }
    }
}

/// The TCP members of `kinfo_file` (`fill_file`'s `intotcpcb(inpcb)` block, shared by the
/// `AF_INET` and `AF_INET6` cases); a control block without a `tcpcb` (the C would
/// dereference NULL) leaves them zero.
fn fill_file_tcpcb(kf: &mut KinfoFile, inpcb: &Inpcb) {
    let Some(tcpcb) = intotcpcb(inpcb) else {
        return;
    };
    kf.t_rcv_wnd = tcpcb.rcv_wnd.get();
    kf.t_snd_wnd = tcpcb.snd_wnd.get();
    kf.t_snd_cwnd = tcpcb.snd_cwnd.get();
    kf.t_state = tcpcb.t_state.get() as u32;
}

/// The output of a `sysctl_file` walk: the C's `kf`, `dp`, `buflen`, `elem_count` and
/// `needed`, which its `FILLIT` and `FILLINPTABLE` macros advance.
struct FileWalk<'a> {
    kf: KinfoFile,
    p: &'a Proc,
    show_pointers: bool,
    dp: usize,
    buflen: usize,
    elem_size: usize,
    elem_count: usize,
    outsize: usize,
    needed: usize,
}

impl FileWalk<'_> {
    /// Whether the buffer has room for one more element.
    fn room(&self) -> bool {
        self.buflen >= self.elem_size && self.elem_count > 0
    }

    /// Copies the filled `kf` out and advances past its element.
    fn put(&mut self) -> Result<(), Errno> {
        copyout(&self.kf.as_bytes()[..self.outsize], self.dp)?;
        self.dp += self.elem_size;
        self.buflen -= self.elem_size;
        self.elem_count -= 1;
        Ok(())
    }

    /// `FILLIT(fp, fdp, i, vp, pr)`.
    fn fillit(
        &mut self,
        fp: Option<&File>,
        fdp: Option<&Filedesc>,
        i: i32,
        vp: Option<&'static Vnode>,
        pr: Option<&Process>,
    ) -> Result<(), Errno> {
        if self.room() {
            fill_file(
                &mut self.kf,
                fp,
                fdp,
                i,
                vp,
                pr,
                self.p,
                None,
                self.show_pointers,
            );
            self.put()?;
        }
        self.needed += self.elem_size;
        Ok(())
    }

    /// `FILLINPTABLE(table)`: the sockets of a protocol control block table, which also
    /// holds the closed connections no file refers to any more.
    fn fillinptable(&mut self, table: &Inpcbtable) -> Result<(), Errno> {
        let iter = InpcbIterator::new();
        let mut inp: Option<&'static Inpcb> = None;
        let mut error = Ok(());

        mtx_enter(&table.inpt_mtx);
        loop {
            // SAFETY: the table mutex is held; `iter` stays in place on this frame and serves
            // this walk only, until the iterator returns None or the walk is aborted below.
            inp = unsafe { in_pcb_iterator(table, inp, &iter) };
            let Some(i) = inp else {
                break;
            };
            if self.room() {
                mtx_leave(&table.inpt_mtx);
                net_lock_shared();
                let Some(so) = in_pcbsolock(i) else {
                    net_unlock_shared();
                    mtx_enter(&table.inpt_mtx);
                    continue;
                };
                fill_file(
                    &mut self.kf,
                    None,
                    None,
                    0,
                    None,
                    None,
                    self.p,
                    Some(so),
                    self.show_pointers,
                );
                in_pcbsounlock(Some(i), Some(so));
                net_unlock_shared();
                let r = copyout(&self.kf.as_bytes()[..self.outsize], self.dp);
                mtx_enter(&table.inpt_mtx);
                if let Err(e) = r {
                    // SAFETY: the mutex is held and `iter` is the iterator of this walk,
                    // whose last answer was `i`.
                    unsafe { in_pcb_iterator_abort(table, Some(i), &iter) };
                    error = Err(e);
                    break;
                }
                self.dp += self.elem_size;
                self.buflen -= self.elem_size;
                self.elem_count -= 1;
            }
            self.needed += self.elem_size;
        }
        mtx_leave(&table.inpt_mtx);
        error
    }

    /// The `KERN_FILE_BYPID`/`KERN_FILE_BYUID` body for one process: its text vnode (when
    /// `text`), current and root directories, then every open descriptor.
    fn fill_process(&mut self, pr: &Process, text: bool) -> Result<(), Errno> {
        let fdp = pr.fd();
        if text && let Some(vp) = pr.ps_textvp.get() {
            self.fillit(None, None, KERN_FILE_TEXT, Some(vp), Some(pr))?;
        }
        if let Some(vp) = fdp.fd_cdir.get() {
            self.fillit(None, None, KERN_FILE_CDIR, Some(vp), Some(pr))?;
        }
        if let Some(vp) = fdp.fd_rdir.get() {
            self.fillit(None, None, KERN_FILE_RDIR, Some(vp), Some(pr))?;
        }
        // ps_tracevp (KERN_FILE_TRACE) does not exist: KTRACE is not configured.
        let mut i = 0;
        while i < fdp.fd_nfiles.get() {
            if let Some(fp) = fd_getfile(fdp, i) {
                let r = self.fillit(Some(fp), Some(fdp), i, None, Some(pr));
                let _ = frele(fp, self.p);
                r?;
            }
            i += 1;
        }
        Ok(())
    }
}

/// Whether `sysctl_file`'s process walks skip `pr`: system, exiting, embryonic and undead
/// processes.
fn file_skips(pr: &Process) -> bool {
    pr.ps_flags.load(Ordering::Relaxed) & (PS_SYSTEM | PS_EMBRYO | PS_EXITING) != 0
}

/// `sysctl_file`: get file structures (`kern.file`; `name` is op, arg, element size,
/// element count).
pub fn sysctl_file(name: &[i32], where_: usize, sizep: &mut usize, p: &Proc) -> Result<(), Errno> {
    if name.len() > 4 {
        return Err(Errno::ENOTDIR);
    }
    if name.len() < 4 || name[2] < 0 || name[2] as usize > size_of::<KinfoFile>() {
        return Err(Errno::EINVAL);
    }

    let op = name[0];
    let arg = name[1];
    let elem_size = name[2] as usize;
    // As the C's size_t: a negative count is no limit.
    let elem_count = name[3] as usize;

    if elem_size < 1 {
        return Err(Errno::EINVAL);
    }

    let mut w = FileWalk {
        kf: KinfoFile::zeroed(),
        p,
        show_pointers: curproc().is_some_and(|cp| suser(cp).is_ok()),
        dp: where_,
        buflen: if where_ != 0 { *sizep } else { 0 },
        elem_size,
        elem_count,
        outsize: size_of::<KinfoFile>().min(elem_size),
        needed: 0,
    };

    match op {
        KERN_FILE_BYFILE => {
            // use the inp-tables to pick up closed connections, too
            if arg == DTYPE_SOCKET {
                w.fillinptable(&TCBTABLE)?;
                #[cfg(feature = "inet6")]
                w.fillinptable(&TCB6TABLE)?;
                w.fillinptable(&UDBTABLE)?;
                #[cfg(feature = "inet6")]
                w.fillinptable(&UDB6TABLE)?;
                w.fillinptable(&RAWCBTABLE)?;
                #[cfg(feature = "inet6")]
                w.fillinptable(&RAWIN6PCBTABLE)?;
                // NPF > 0
                w.fillinptable(&DIVBTABLE)?;
                #[cfg(feature = "inet6")]
                w.fillinptable(&DIVB6TABLE)?;
            }
            let mut fp = None;
            loop {
                fp = fd_iterfile(fp, p);
                let Some(f) = fp else {
                    break;
                };
                if arg != 0 && f.f_type.get() != arg {
                    continue;
                }
                let skip = arg == DTYPE_SOCKET && {
                    let af = fp_socket(f).dom_family();
                    af == i32::from(AF_INET) || af == i32::from(AF_INET6)
                };
                if !skip {
                    kernel_lock();
                    let r = w.fillit(Some(f), None, 0, None, None);
                    kernel_unlock();
                    if let Err(e) = r {
                        let _ = frele(f, p);
                        return Err(e);
                    }
                }
            }
        }
        KERN_FILE_BYPID => {
            // A arg of -1 indicates all processes
            if arg < -1 {
                return Err(Errno::EINVAL);
            }
            let mut matched = false;
            kernel_lock();
            let r = 'scan: {
                for pr in ALLPROCESS.0.iter() {
                    if file_skips(pr) {
                        continue;
                    }
                    if arg >= 0 && pr.ps_pid.get() != arg {
                        // not the pid we are looking for
                        continue;
                    }

                    refcnt_take(&pr.ps_refcnt);
                    matched = true;
                    let r = w.fill_process(pr, true);
                    refcnt_rele_wake(&pr.ps_refcnt);
                    if r.is_err() {
                        break 'scan r;
                    }

                    // pid is unique, stop searching
                    if arg >= 0 {
                        break;
                    }
                }
                Ok(())
            };
            kernel_unlock();
            r?;
            if !matched {
                return Err(Errno::ESRCH);
            }
        }
        KERN_FILE_BYUID => {
            kernel_lock();
            let r = 'scan: {
                for pr in ALLPROCESS.0.iter() {
                    if file_skips(pr) {
                        continue;
                    }
                    if arg >= 0 && pr.ucred().cr_uid.get() != arg as u32 {
                        // not the uid we are looking for
                        continue;
                    }

                    refcnt_take(&pr.ps_refcnt);
                    let r = w.fill_process(pr, false);
                    refcnt_rele_wake(&pr.ps_refcnt);
                    if r.is_err() {
                        break 'scan r;
                    }
                }
                Ok(())
            };
            kernel_unlock();
            r?;
        }
        _ => return Err(Errno::EINVAL),
    }

    let mut needed = w.needed;
    let mut error = Ok(());
    if where_ == 0 {
        needed += KERN_FILESLOP * elem_size;
    } else if *sizep < needed {
        error = Err(Errno::ENOMEM);
    }
    *sizep = needed;
    error
}

/// `sysctl_doproc`: the `kern.proc` node, an array of `kinfo_proc` (`name` is op, arg,
/// element size, element count).
pub fn sysctl_doproc(name: &[i32], where_: usize, sizep: &mut usize) -> Result<(), Errno> {
    let mut dp = where_;
    let mut buflen = if where_ != 0 { *sizep } else { 0 };
    let mut needed: usize = 0;

    let [op, arg, elem_size, elem_count] = *name else {
        return Err(Errno::EINVAL);
    };
    if elem_size <= 0 || elem_count < 0 || elem_size as usize > size_of::<KinfoProc>() {
        return Err(Errno::EINVAL);
    }
    let elem_size = elem_size as usize;
    let mut elem_count = elem_count;

    let dothreads = op & KERN_PROC_SHOW_THREADS != 0;
    let op = op & !KERN_PROC_SHOW_THREADS;

    let show_pointers = curproc().is_some_and(|cp| suser(cp).is_ok());

    let mut kproc = KinfoProc::zeroed();

    for list in [&ALLPROCESS, &ZOMBPROCESS] {
        for pr in list.0.iter() {
            // XXX skip processes in the middle of being created or zapped
            if pr.ps_pgrp.get().is_null() {
                continue;
            }

            // Skip embryonic processes.
            let flags = pr.ps_flags.load(Ordering::Relaxed);
            if flags & PS_EMBRYO != 0 {
                continue;
            }

            if !doproc_matches(pr, op, arg)? {
                continue;
            }

            if buflen >= elem_size && elem_count > 0 {
                fill_kproc(pr, &mut kproc, None, show_pointers);
                copyout(&kproc.as_bytes()[..elem_size], dp)?;
                dp += elem_size;
                buflen -= elem_size;
                elem_count -= 1;
            }
            needed += elem_size;

            // Skip per-thread entries if not required by op
            if !dothreads {
                continue;
            }

            let mut q = pr.ps_threads.first();
            while let Some(p) = q {
                if buflen >= elem_size && elem_count > 0 {
                    fill_kproc(pr, &mut kproc, Some(p), show_pointers);
                    copyout(&kproc.as_bytes()[..elem_size], dp)?;
                    dp += elem_size;
                    buflen -= elem_size;
                    elem_count -= 1;
                }
                needed += elem_size;
                q = TailqHead::<ProcThrLink>::next(p);
            }
        }
    }

    if where_ != 0 {
        *sizep = dp - where_;
        if needed > *sizep {
            return Err(Errno::ENOMEM);
        }
    } else {
        needed += KERN_PROCSLOP * elem_size;
        *sizep = needed;
    }
    Ok(())
}

/// The `switch (op)` of `sysctl_doproc`: whether `pr` is selected; `EINVAL` for an unknown
/// op.
fn doproc_matches(pr: &Process, op: i32, arg: i32) -> Result<bool, Errno> {
    // SAFETY: a process in a list has its pgrp and session until process_zap, and the
    // caller skipped the ones without a pgrp.
    let sess = unsafe { pr.session().as_ref() };
    Ok(match op {
        // could do this with just a lookup
        KERN_PROC_PID => pr.ps_pid.get() == arg,
        // could do this by traversing pgrp
        KERN_PROC_PGRP => pr.pgid() == arg,
        KERN_PROC_SESSION => {
            // SAFETY: a session leader stays allocated while the session exists.
            let leader = sess.and_then(|s| unsafe { s.s_leader.get().as_ref() });
            leader.is_some_and(|l| l.ps_pid.get() == arg)
        }
        KERN_PROC_TTY => {
            if pr.ps_flags.load(Ordering::Relaxed) & PS_CONTROLT == 0
                || sess.is_none_or(|s| s.s_ttyp.get().is_null())
            {
                false
            } else {
                // SAFETY: a controlling tty belongs to its driver's softc, which keeps it
                // until the device detaches (`ttyfree`); the C reads it the same way.
                sess.and_then(|s| unsafe { s.s_ttyp.get().as_ref() })
                    .is_some_and(|tp| tp.t_dev.get() == arg as Dev)
            }
        }
        KERN_PROC_UID => pr.ucred().cr_uid.get() == arg as u32,
        KERN_PROC_RUID => pr.ucred().cr_ruid.get() == arg as u32,
        KERN_PROC_ALL => pr.ps_flags.load(Ordering::Relaxed) & PS_SYSTEM == 0,
        // no filtering
        KERN_PROC_KTHREAD => true,
        _ => return Err(Errno::EINVAL),
    })
}

/// `_getcompatprio` (`<sys/sysctl.h>`): the priority `ps(1)` shows.
fn getcompatprio(p: &Proc) -> u8 {
    match p.p_stat.get() {
        SRUN => p.p_runpri.get(),
        SSLEEP => p.p_slppri.get(),
        _ => p.p_usrpri.get(),
    }
}

/// `PTRTOINT64` (`<sys/sysctl.h>`).
fn ptrtoint64<T>(x: *const T) -> u64 {
    x as usize as u64
}

/// `W_EXITCODE` (`<sys/wait.h>`, not ported).
const fn w_exitcode(ret: u32, sig: i32) -> u32 {
    (ret << 8) | sig as u32
}

/// `fill_kproc`: fills in a `kinfo_proc` for process `pr`, or for its thread `p`.
pub fn fill_kproc(pr: &Process, ki: &mut KinfoProc, p: Option<&Proc>, show_pointers: bool) {
    let flags = pr.ps_flags.load(Ordering::Relaxed);
    // SAFETY: the callers skip processes without a pgrp; pgrp and session stay allocated
    // while the process is in them.
    let (pg, s) = unsafe { (&*pr.ps_pgrp.get(), &*pr.session()) };

    // exiting/zombie process might no longer have VM space.
    let mut vm: Option<&'static Vmspace> = None;
    if flags & PS_EXITING == 0 {
        // SAFETY: a process that is not exiting holds its vmspace reference.
        vm = unsafe { pr.ps_vmspace.get().as_ref() };
        if let Some(vm) = vm {
            uvmspace_addref(vm);
        }
    }

    let tu = Tusage::new();
    let isthread = p.is_some();
    let p: &Proc = match p {
        Some(p) => {
            tuagg_get_proc(&tu, p);
            p
        }
        None => {
            // SAFETY: a process keeps its main thread until process_zap. XXX, as in C.
            let Some(mainp) = (unsafe { pr.ps_mainproc.get().as_ref() }) else {
                panic(format_args!("fill_kproc: process without a main thread"));
            };
            tuagg_get_process(&tu, pr);
            mainp
        }
    };
    let uc = pr.ucred();

    // FILL_KPROC(ki, strlcpy, p, pr, pr->ps_ucred, pr->ps_pgrp, p, pr, s, vm,
    //     pr->ps_limit, pr->ps_sigacts, &tu, isthread, show_pointers)
    *ki = KinfoProc::zeroed();

    if show_pointers {
        ki.p_paddr = ptrtoint64(p);
        ki.p_fd = ptrtoint64(pr.ps_fd.get());
        ki.p_limit = ptrtoint64(pr.ps_limit.get());
        ki.p_vmspace = ptrtoint64(pr.ps_vmspace.get());
        ki.p_sigacts = ptrtoint64(pr.ps_sigacts.get());
        ki.p_sess = ptrtoint64(pg.pg_session.get());
        ki.p_ru = ptrtoint64(pr.ps_ru.get());
    }
    ki.p_stats = 0;
    ki.p_exitsig = 0;
    ki.p_flag = p.p_flag.load(Ordering::Relaxed);
    ki.p_pid = pr.ps_pid.get();
    ki.p_psflags = flags;

    ki.p__pgid = pg.pg_id.get();

    ki.p_uid = uc.cr_uid.get();
    ki.p_ruid = uc.cr_ruid.get();
    ki.p_gid = uc.cr_gid.get();
    ki.p_rgid = uc.cr_rgid.get();
    ki.p_svuid = uc.cr_svuid.get();
    ki.p_svgid = uc.cr_svgid.get();

    for (dst, g) in ki.p_groups.iter_mut().zip(&uc.cr_groups) {
        *dst = g.get();
    }
    ki.p_ngroups = uc.cr_ngroups.get();

    ki.p_jobc = pg.pg_jobc.get() as i16;

    ki.p_estcpu = p.p_estcpu.get();
    if isthread {
        ki.p_tid = p.p_tid.get() + THREAD_PID_OFFSET;
        strlcpy(&mut ki.p_name, p.name());
    } else {
        ki.p_tid = -1;
    }
    let runtime = tu.tu_runtime.get();
    ki.p_rtime_sec = runtime.tv_sec as u32;
    ki.p_rtime_usec = (runtime.tv_nsec / 1000) as u32;
    ki.p_uticks = tu.tu_ticks[TU_UTICKS].get();
    ki.p_sticks = tu.tu_ticks[TU_STICKS].get();
    ki.p_iticks = tu.tu_ticks[TU_ITICKS].get();
    ki.p_cpticks = p.p_cpticks.get() as i32;

    // p_tracep, p_traceflag: KTRACE is not configured.

    ki.p_siglist =
        (p.p_siglist.load(Ordering::Relaxed) | pr.ps_siglist.load(Ordering::Relaxed)) as i32;
    ki.p_sigmask = p.p_sigmask.get();

    mtx_enter(&pr.ps_mtx); // PR_LOCK(pr)
    ki.p_ppid = pr.ps_ppid.get();
    // SAFETY: a live process's sigacts stays allocated until process_zap; ps_mtx is held.
    if let Some(sa) = unsafe { pr.ps_sigacts.get().as_ref() } {
        ki.p_sigignore = sa.ps_sigignore.get();
        ki.p_sigcatch = sa.ps_sigcatch.get();
    }

    // SAFETY: a process holds a reference to its limits; ps_mtx keeps them from changing.
    if let Some(lim) = unsafe { pr.ps_limit.get().as_ref() } {
        ki.p_rlim_rss_cur = lim.pl_rlimit[RLIMIT_RSS].get().rlim_cur;
    }
    mtx_leave(&pr.ps_mtx); // PR_UNLOCK(pr)

    ki.p_stat = p.p_stat.get() as i8;
    ki.p_nice = pr.ps_nice.get();

    ki.p_xstat = w_exitcode(pr.ps_xexit.get(), pr.ps_xsig.get()) as u16;
    ki.p_acflag = pr.ps_acflag.get();
    ki.p_pledge = pr.ps_pledge.get();

    strlcpy(&mut ki.p_emul, b"native");
    strlcpy(&mut ki.p_comm, pr.comm());
    // SAFETY: s_login is written by setlogin(2) under the kernel lock; read for copying.
    strlcpy(&mut ki.p_login, unsafe { &*s.s_login.get() });

    if !s.s_ttyvp.get().is_null() {
        ki.p_eflag |= EPROC_CTTY;
    }
    if !pr.ps_uvpaths.get().is_null() {
        ki.p_eflag |= EPROC_UNVEIL;
    }
    if pr.ps_uvdone.get() != 0
        || (flags & PS_PLEDGE != 0 && pr.ps_pledge.get() & PLEDGE_UNVEIL == 0)
    {
        ki.p_eflag |= EPROC_LKUNVEIL;
    }

    if flags & (PS_EMBRYO | PS_ZOMBIE) == 0 {
        if let Some(vm) = vm {
            ki.p_vm_rssize = vm.vm_rssize.get();
            ki.p_vm_tsize = vm.vm_tsize.get();
            ki.p_vm_dsize = vm.vm_dused.get();
            ki.p_vm_ssize = vm.vm_ssize.get();
        }
        ki.p_stat = p.p_stat.get() as i8;
        ki.p_slptime = p.p_slptime.get();
        ki.p_holdcnt = 1;
        ki.p_priority = getcompatprio(p);
        ki.p_usrpri = p.p_usrpri.get();
        if !p.p_wchan.get().is_null()
            && let Some(wmesg) = p.p_wmesg.get()
        {
            strlcpy(&mut ki.p_wmesg, wmesg.as_bytes());
        }
        if show_pointers {
            ki.p_wchan = ptrtoint64(p.p_wchan.get());
            ki.p_addr = ptrtoint64(p.p_addr.get());
        }
    }

    if flags & PS_ZOMBIE == 0 {
        ki.p_uvalid = 1;

        let ru = &p.p_ru;
        ki.p_uru_maxrss = ru.ru_maxrss.get() as u64;
        ki.p_uru_ixrss = ru.ru_ixrss.get() as u64;
        ki.p_uru_idrss = ru.ru_idrss.get() as u64;
        ki.p_uru_isrss = ru.ru_isrss.get() as u64;
        ki.p_uru_minflt = ru.ru_minflt.get() as u64;
        ki.p_uru_majflt = ru.ru_majflt.get() as u64;
        ki.p_uru_nswap = ru.ru_nswap.get() as u64;
        ki.p_uru_inblock = ru.ru_inblock.get() as u64;
        ki.p_uru_oublock = ru.ru_oublock.get() as u64;
        ki.p_uru_msgsnd = ru.ru_msgsnd.get() as u64;
        ki.p_uru_msgrcv = ru.ru_msgrcv.get() as u64;
        ki.p_uru_nsignals = ru.ru_nsignals.get() as u64;
        ki.p_uru_nvcsw = ru.ru_nvcsw.get() as u64;
        ki.p_uru_nivcsw = ru.ru_nivcsw.get() as u64;

        let tv = timeradd(&pr.ps_cru.ru_utime.get(), &pr.ps_cru.ru_stime.get());
        ki.p_uctime_sec = tv.tv_sec as u32;
        ki.p_uctime_usec = tv.tv_usec as u32;
    }

    ki.p_cpuid = KI_NOCPU;
    ki.p_rtableid = pr.ps_rtableid.load(Ordering::Relaxed);
    // end of FILL_KPROC

    // stuff that's too painful to generalize into the macros
    // SAFETY: a session leader stays allocated while the session exists.
    if let Some(leader) = unsafe { s.s_leader.get().as_ref() } {
        ki.p_sid = leader.ps_pid.get();
    }

    // SAFETY: as in `KERN_PROC_TTY`: the driver keeps the tty until it detaches.
    if let Some(tp) = unsafe { s.s_ttyp.get().as_ref() }
        && flags & PS_CONTROLT != 0
    {
        ki.p_tdev = tp.t_dev.get() as u32;
        ki.p_tpgid = tp.pgrp().map_or(-1, |pg| pg.pg_id.get());
        ki.p_tsess = tp.t_session.get() as u64;
    } else {
        ki.p_tdev = NODEV as u32;
        ki.p_tpgid = -1;
    }

    // fixups that can only be done in the kernel
    if flags & PS_EXITING == 0 {
        if flags & PS_EMBRYO == 0
            && let Some(vm) = vm
        {
            ki.p_vm_rssize = pmap_resident_count(vm.vm_map.pmap()) as i32;
        }
        let (ut, st, _) = calctsru(&tu);
        ki.p_uutime_sec = ut.tv_sec as u32;
        ki.p_uutime_usec = (ut.tv_nsec / 1000) as u32;
        ki.p_ustime_sec = st.tv_sec as u32;
        ki.p_ustime_usec = (st.tv_nsec / 1000) as u32;

        // Convert starting uptime to a starting UTC time.
        let booted = nanoboottime();
        let utc = crate::sys::time::timespecadd(&booted, &pr.ps_start.get());
        ki.p_ustart_sec = utc.tv_sec as u64;
        ki.p_ustart_usec = (utc.tv_nsec / 1000) as u32;

        #[cfg(feature = "multiprocessor")]
        if let Some(ci) = p.cpu() {
            ki.p_cpuid = u64::from(Machine::cpu_info_unit(ci));
        }
    }

    if let Some(vm) = vm {
        uvmspace_free(vm);
    }

    // get %cpu and schedule state: just one thread or sum of all?
    if isthread {
        ki.p_pctcpu = p.p_pctcpu.load(Ordering::Relaxed);
        ki.p_stat = p.p_stat.get() as i8;
    } else {
        ki.p_pctcpu = 0;
        let mut stat = if flags & PS_EXITING != 0 { SDEAD } else { SIDL };
        let mut q = pr.ps_threads.first();
        while let Some(t) = q {
            ki.p_pctcpu += t.p_pctcpu.load(Ordering::Relaxed);
            // find best state: ONPROC > RUN > STOP > SLEEP > ..
            let ts = t.p_stat.get();
            if ts == SONPROC || stat == SONPROC {
                stat = SONPROC;
            } else if ts == SRUN || stat == SRUN {
                stat = SRUN;
            } else if ts == SSTOP || stat == SSTOP {
                stat = SSTOP;
            } else if ts == SSLEEP {
                stat = SSLEEP;
            }
            q = TailqHead::<ProcThrLink>::next(t);
        }
        ki.p_stat = stat as i8;
    }
}

/// `sysctl_proc_args`: `kern.procargs.<pid>.<op>`: the victim's argument or environment
/// strings (or their count), read from its address space with `uvm_io`.
pub fn sysctl_proc_args(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    cp: &Proc,
) -> Result<(), Errno> {
    if name.len() > 2 {
        return Err(Errno::ENOTDIR);
    }
    let [pid, op] = *name else {
        return Err(Errno::EINVAL);
    };

    match op {
        KERN_PROC_ARGV | KERN_PROC_NARGV | KERN_PROC_ENV | KERN_PROC_NENV => {}
        _ => return Err(Errno::EOPNOTSUPP),
    }

    let Some(vpr) = prfind(pid) else {
        return Err(Errno::ESRCH);
    };

    if oldp == 0 {
        *oldlenp = if op == KERN_PROC_NARGV || op == KERN_PROC_NENV {
            size_of::<i32>()
        } else {
            syslimits::ARG_MAX // XXX XXX XXX
        };
        return Ok(());
    }

    let flags = vpr.ps_flags.load(Ordering::Relaxed);
    // Either system process or exiting/zombie
    if flags & (PS_SYSTEM | PS_EXITING) != 0 {
        return Err(Errno::EINVAL);
    }

    // Execing - danger.
    if flags & PS_INEXEC != 0 {
        return Err(Errno::EBUSY);
    }

    // Only owner or root can get env
    if (op == KERN_PROC_NENV || op == KERN_PROC_ENV)
        && vpr.ucred().cr_uid.get() != cp.ucred().cr_uid.get()
    {
        suser(cp)?;
    }

    let ps_strings = vpr.ps_strings.get();
    let vm = vpr.vmspace();
    uvmspace_addref(vm);

    let Some(mem) = malloc(PAGE_SIZE, M_TEMP, M_WAITOK) else {
        uvmspace_free(vm);
        return Err(Errno::ENOMEM);
    };
    // SAFETY: a fresh PAGE_SIZE allocation, freed below and not shared; uvm_io writes it
    // before it is read.
    let buf = unsafe { core::slice::from_raw_parts_mut(mem.as_ptr(), PAGE_SIZE) };

    let error = proc_args_copy(&vm.vm_map, ps_strings, op, oldp, oldlenp, buf, cp);

    uvmspace_free(vm);
    free(mem, M_TEMP, PAGE_SIZE);
    error
}

/// The body of `sysctl_proc_args` past its checks (its `goto out` paths are the `?`s; the
/// caller drops the vmspace reference and the buffer): reads the victim's `ps_strings`, then
/// lays out an `argv`-style array of `cnt` pointers, a NULL and the strings in the reader's
/// buffer at `oldp`.
fn proc_args_copy(
    map: &VmMap,
    ps_strings: usize,
    op: i32,
    oldp: usize,
    oldlenp: &mut usize,
    buf: &mut [u8],
    cp: &Proc,
) -> Result<(), Errno> {
    const PTR: usize = size_of::<usize>();

    // Reads `dst.len()` bytes of the victim's address space at `va`.
    let victim_read = |va: usize, dst: &mut [u8]| -> Result<(), Errno> {
        let mut iov = [Iovec {
            iov_base: dst.as_mut_ptr().cast(),
            iov_len: dst.len(),
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: va as Off,
            uio_resid: dst.len(),
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: Some(cp),
        };
        uvm_io(map, &mut uio, 0)
    };

    let mut pss = [0u8; size_of::<PsStrings>()];
    victim_read(ps_strings, &mut pss)?;
    let word = |b: &[u8]| usize::from_ne_bytes(b[..PTR].try_into().unwrap_or([0; PTR]));
    let int = |b: &[u8]| i32::from_ne_bytes(b[..4].try_into().unwrap_or([0; 4]));
    // struct ps_strings: ps_argvstr, ps_nargvstr, ps_envstr, ps_nenvstr (PsStrings::to_bytes).
    let (argvstr, nargvstr) = (word(&pss[0..]), int(&pss[PTR..]));
    let (envstr, nenvstr) = (word(&pss[2 * PTR..]), int(&pss[3 * PTR..]));

    if op == KERN_PROC_NARGV {
        return sysctl_rdint(oldp, oldlenp, 0, nargvstr);
    }
    if op == KERN_PROC_NENV {
        return sysctl_rdint(oldp, oldlenp, 0, nenvstr);
    }

    let (cnt, mut vargv) = if op == KERN_PROC_ARGV {
        (nargvstr, argvstr)
    } else {
        (nenvstr, envstr)
    };

    // Clamp to avoid overflow, using ARG_MAX is only an approximation. It is not possible
    // to execve() with this many elements, so this only happens if a process has changed
    // its strings. A hard cap, so not ENOMEM: the caller cannot retry. (The C's count is
    // unsigned: a negative one is over the cap too.)
    if cnt < 0 || cnt as usize > syslimits::ARG_MAX {
        return Err(Errno::EINVAL);
    }
    let mut cnt = cnt as usize;

    // -1 to have space for a terminating NUL
    let limit = oldlenp.wrapping_sub(1);
    *oldlenp = 0;

    // *oldlenp: bytes copied out into the reader's buffer; limit: the most allowed there;
    // rarg: where the next string goes in the reader's buffer; rargv: where the next rarg
    // pointer goes; vargv: where the next argument pointer is read in the victim.
    let mut rargv = oldp;
    // space for cnt pointers and a NULL
    let mut rarg = rargv + (cnt + 1) * PTR;
    *oldlenp += (cnt + 1) * PTR;

    while cnt > 0 && *oldlenp < limit {
        // Write to the reader's argv.
        copyout(&rarg.to_ne_bytes(), rargv)?;

        // Read the victim's argv.
        let mut vargb = [0u8; PTR];
        victim_read(vargv, &mut vargb)?;
        let mut varg = usize::from_ne_bytes(vargb);
        if varg == 0 {
            break;
        }

        // Read the victim's string a page at a time, so as not to cross a page boundary
        // too much and return an error.
        loop {
            let len = PAGE_SIZE - (varg & PAGE_MASK);
            victim_read(varg, &mut buf[..len])?;
            let vstrlen = buf[..len].iter().position(|&c| c == 0).unwrap_or(len);

            // Don't overflow the reader's buffer.
            if *oldlenp + vstrlen + 1 >= limit {
                return Err(Errno::ENOMEM);
            }
            copyout(&buf[..vstrlen], rarg)?;
            *oldlenp += vstrlen;
            rarg += vstrlen;

            // The string didn't end in this page?
            if vstrlen == len {
                varg += vstrlen;
                continue;
            }
            break;
        }

        // End of string. Terminate it with a NUL.
        copyout(&[0], rarg)?;
        *oldlenp += 1;
        rarg += 1;

        vargv += PTR;
        rargv += PTR;
        cnt -= 1;
    }

    if *oldlenp >= limit {
        return Err(Errno::ENOMEM);
    }

    // Write the terminating null.
    copyout(&0usize.to_ne_bytes(), rargv)
}

/// `sysctl_proc_cwd`: `kern.proc_cwd.<pid>`, the process's current directory.
pub fn sysctl_proc_cwd(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    cp: &Proc,
) -> Result<(), Errno> {
    if name.len() > 1 {
        return Err(Errno::ENOTDIR);
    }
    let [pid] = *name else {
        return Err(Errno::EINVAL);
    };

    let Some(findpr) = prfind(pid) else {
        return Err(Errno::ESRCH);
    };

    if oldp == 0 {
        *oldlenp = MAXPATHLEN * 4;
        return Ok(());
    }

    // Either system process or exiting/zombie
    if findpr.ps_flags.load(Ordering::Relaxed) & (PS_SYSTEM | PS_EXITING) != 0 {
        return Err(Errno::EINVAL);
    }

    // Only owner or root can get cwd
    if findpr.ucred().cr_uid.get() != cp.ucred().cr_uid.get() {
        suser(cp)?;
    }

    let mut len = *oldlenp;
    if len > MAXPATHLEN * 4 {
        len = MAXPATHLEN * 4;
    } else if len < 2 {
        return Err(Errno::ERANGE);
    }
    *oldlenp = 0;

    // snag a reference to the vnode before we can sleep
    let Some(vp) = findpr.fd().fd_cdir.get() else {
        // No current directory before a root file system is mounted.
        return Err(Errno::ENOENT);
    };
    vref(vp);

    let Some(mem) = malloc(len, M_TEMP, M_WAITOK) else {
        vrele(vp);
        return Err(Errno::ENOMEM);
    };
    // SAFETY: a fresh `len`-byte allocation, freed below; every byte is written before it is
    // read (the path is built backwards from the NUL).
    let path = unsafe { core::slice::from_raw_parts_mut(mem.as_ptr(), len) };

    let mut bp = len - 1;
    path[bp] = 0;

    // Same as sys__getcwd
    let mut error = vfs_getcwd_common(
        vp,
        None,
        Some((&mut *path, &mut bp)),
        (len / 2) as i32,
        GETCWD_CHECK_ACCESS,
        cp,
    );
    if error.is_ok() {
        let lenused = len - bp;
        *oldlenp = lenused;
        error = copyout(&path[bp..], oldp);
    }

    vrele(vp);
    free(mem, M_TEMP, len);

    error
}

/// `sysctl_proc_nobroadcastkill`: `kern.proc_nobroadcastkill.<pid>`, the process's
/// `PS_NOBROADCASTKILL` flag.
pub fn sysctl_proc_nobroadcastkill(
    name: &[i32],
    newp: usize,
    newlen: usize,
    oldp: usize,
    oldlenp: &mut usize,
    cp: &Proc,
) -> Result<(), Errno> {
    if name.len() > 1 {
        return Err(Errno::ENOTDIR);
    }
    let [pid] = *name else {
        return Err(Errno::EINVAL);
    };

    let Some(findpr) = prfind(pid) else {
        return Err(Errno::ESRCH);
    };

    // Either system process or exiting/zombie
    if findpr.ps_flags.load(Ordering::Relaxed) & (PS_SYSTEM | PS_EXITING) != 0 {
        return Err(Errno::EINVAL);
    }

    // Only root can change PS_NOBROADCASTKILL
    if newp != 0 {
        suser(cp)?;
    }

    // get the PS_NOBROADCASTKILL flag
    let flag = AtomicI32::new(i32::from(
        findpr.ps_flags.load(Ordering::Relaxed) & PS_NOBROADCASTKILL != 0,
    ));

    let error = sysctl_int(oldp, oldlenp, newp, newlen, &flag);
    if error.is_ok() && newp != 0 {
        if flag.into_inner() != 0 {
            findpr
                .ps_flags
                .fetch_or(PS_NOBROADCASTKILL, Ordering::Relaxed);
        } else {
            findpr
                .ps_flags
                .fetch_and(!PS_NOBROADCASTKILL, Ordering::Relaxed);
        }
    }

    error
}

/// `sysctl_proc_vmmap`: `kern.proc_vmmap.<pid>`, the address space as `kinfo_vmentry`s.
/// The checks are the C's; `fill_vmmap` is reported.
pub fn sysctl_proc_vmmap(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    cp: &Proc,
) -> Result<(), Errno> {
    if name.len() > 1 {
        return Err(Errno::ENOTDIR);
    }
    let [pid] = *name else {
        return Err(Errno::EINVAL);
    };

    // Provide max buffer length as hint (oldlenp is never NULL here).
    if oldp == 0 {
        *oldlenp = VMMAP_MAXLEN;
        return Ok(());
    }

    if pid == cp.process().ps_pid.get() {
        // Self process mapping.
    } else if pid > 0 {
        let Some(findpr) = prfind(pid) else {
            return Err(Errno::ESRCH);
        };

        // Either system process or exiting/zombie
        if findpr.ps_flags.load(Ordering::Relaxed) & (PS_SYSTEM | PS_EXITING) != 0 {
            return Err(Errno::EINVAL);
        }

        // XXX Allow only root for now
        suser(cp)?;
    } else {
        // Only root can get kernel_map
        suser(cp)?;
    }

    // Check the given size.
    let oldlen = *oldlenp;
    if oldlen == 0 || !oldlen.is_multiple_of(size_of::<KinfoVmentry>()) {
        return Err(Errno::EINVAL);
    }

    // Deny huge allocation.
    if oldlen > VMMAP_MAXLEN {
        return Err(Errno::EINVAL);
    }

    // Iterate from the given address passed as the first element's kve_start via oldp.
    let mut start = [0u8; 8];
    copyin(oldp, &mut start)?;

    Err(unported!("kern.proc_vmmap: fill_vmmap (uvm_glue.c)"))
}

/// One `snprintf(disknames + l, disknameslen - l, "%s:%s,", name, duid)` of
/// `sysctl_diskinit`, then `l += strlen(disknames + l)`: appends at `l` what fits of the
/// entry, NUL-terminated, and returns the new `l`.
fn diskname_append(buf: &mut [u8], l: usize, name: &[u8], duid: &[u8]) -> usize {
    let Some(avail) = buf.len().checked_sub(l).filter(|&a| a > 0) else {
        return l;
    };
    let mut n = 0;
    for &c in name.iter().chain(b":").chain(duid).chain(b",") {
        if n + 1 >= avail {
            break;
        }
        buf[l + n] = c;
        n += 1;
    }
    buf[l + n] = 0;
    l + n
}

/// The per-disk copy of `sysctl_diskinit`: `dk`'s name and statistics into `sdk`.
fn disk_stats_copy(sdk: &mut Diskstats, dk: &Disk) {
    strlcpy(&mut sdk.ds_name, &dk.dk_name.get());
    mtx_enter(&dk.dk_mtx);
    sdk.ds_busy = dk.dk_busy.get();
    sdk.ds_rxfer = dk.dk_rxfer.get();
    sdk.ds_wxfer = dk.dk_wxfer.get();
    sdk.ds_seek = dk.dk_seek.get();
    sdk.ds_rbytes = dk.dk_rbytes.get();
    sdk.ds_wbytes = dk.dk_wbytes.get();
    sdk.ds_attachtime = dk.dk_attachtime.get();
    sdk.ds_timestamp = dk.dk_timestamp.get();
    sdk.ds_time = dk.dk_time.get();
    mtx_leave(&dk.dk_mtx);
}

/// `diskstats` as a slice of its `diskstatslen / sizeof(struct diskstats)` entries.
///
/// # Safety
///
/// `sysctl_disklock` is held for writing, so no other slice of the array is live and
/// `sysctl_diskinit` cannot replace it meanwhile.
#[allow(clippy::mut_from_ref)] // the C's global array, guarded by sysctl_disklock
unsafe fn diskstats_mut() -> &'static mut [Diskstats] {
    let Some(sdk) = NonNull::new(DISKSTATS.load(Ordering::Relaxed)) else {
        return &mut [];
    };
    let n = DISKSTATSLEN.load(Ordering::Relaxed) / size_of::<Diskstats>();
    // SAFETY: `sysctl_diskinit` allocated `n` zeroed entries (all-zero is a valid
    // `Diskstats`) and frees them only under the lock the caller holds.
    unsafe { core::slice::from_raw_parts_mut(sdk.as_ptr(), n) }
}

/// `sysctl_diskinit`: initialises `disknames`/`diskstats` for export by sysctl. If `update`
/// is set, then we simply update the disk statistics information.
pub fn sysctl_diskinit(update: bool, _p: &Proc) -> Result<(), Errno> {
    kernel_assert_locked();

    rw_enter(&SYSCTL_DISKLOCK, RW_WRITE | RW_INTR)?;

    let mut changed = false;

    // Run in a loop, disks may change while malloc sleeps.
    while DISK_CHANGE.load(Ordering::Relaxed) != 0 {
        DISK_CHANGE.store(0, Ordering::Relaxed);

        let mut tlen = 0;
        for dk in DISKLIST.0.iter() {
            tlen += strnlen(&dk.dk_name.get(), DS_DISKNAMELEN);
            tlen += 18; // label uid + separators
        }
        tlen += 1;
        // disk_count may change when malloc sleeps
        let count = DISK_COUNT.load(Ordering::Relaxed).max(0) as usize;

        // The sysctl_disklock ensures that no other process can allocate disknames and
        // diskstats while our malloc sleeps.
        if let Some(names) = NonNull::new(DISKNAMES.swap(ptr::null_mut(), Ordering::Relaxed)) {
            free(names, M_SYSCTL, DISKNAMESLEN.load(Ordering::Relaxed));
        }
        if let Some(stats) = NonNull::new(DISKSTATS.swap(ptr::null_mut(), Ordering::Relaxed)) {
            free(stats.cast(), M_SYSCTL, DISKSTATSLEN.load(Ordering::Relaxed));
        }
        DISKNAMESLEN.store(0, Ordering::Relaxed);
        DISKSTATSLEN.store(0, Ordering::Relaxed);
        if let Some(stats) = mallocarray(count, size_of::<Diskstats>(), M_SYSCTL, M_WAITOK | M_ZERO)
        {
            DISKSTATS.store(stats.as_ptr().cast(), Ordering::Relaxed);
            DISKSTATSLEN.store(count * size_of::<Diskstats>(), Ordering::Relaxed);
        }
        if let Some(names) = malloc(tlen, M_SYSCTL, M_WAITOK | M_ZERO) {
            // disknames[0] = '\0': M_ZERO.
            DISKNAMES.store(names.as_ptr(), Ordering::Relaxed);
            DISKNAMESLEN.store(tlen, Ordering::Relaxed);
        }
        changed = true;
    }

    // SAFETY: SYSCTL_DISKLOCK is held for writing.
    let stats = unsafe { diskstats_mut() };
    if changed {
        let names = match NonNull::new(DISKNAMES.load(Ordering::Relaxed)) {
            // SAFETY: the `disknameslen` bytes allocated above; only this lock's holder
            // touches them.
            Some(n) => unsafe {
                core::slice::from_raw_parts_mut(n.as_ptr(), DISKNAMESLEN.load(Ordering::Relaxed))
            },
            None => &mut [],
        };
        let mut l = 0;
        let mut sdk = stats.iter_mut();
        for dk in DISKLIST.0.iter() {
            let label_uid = dk.label().map(|lp| lp.d_uid).filter(|u| !duid_iszero(u));
            let duid = label_uid.map(|u| duid_format(&u));
            let name = dk.dk_name.get();
            l = diskname_append(
                names,
                l,
                &name[..strnlen(&name, DS_DISKNAMELEN)],
                duid.as_ref().map_or(&[][..], |d| &d[..]),
            );
            if let Some(sdk) = sdk.next() {
                disk_stats_copy(sdk, dk);
            }
        }

        // Eliminate trailing comma
        if l != 0 {
            names[l - 1] = 0;
        }
    } else if update {
        // Just update, number of drives hasn't changed
        for (sdk, dk) in stats.iter_mut().zip(DISKLIST.0.iter()) {
            disk_stats_copy(sdk, dk);
        }
    }
    rw_exit_write(&SYSCTL_DISKLOCK);
    Ok(())
}

/// `sysctl_intrcnt`: `kern.intrcnt`, served by `evcount_sysctl`.
pub fn sysctl_intrcnt(name: &[i32], oldp: usize, oldlenp: &mut usize) -> Result<(), Errno> {
    evcount_sysctl(name, oldp, oldlenp, 0, 0)
}

/// `sysctl_sensors`: `hw.sensors.<dev>` (a `struct sensordev`) and
/// `hw.sensors.<dev>.<type>.<numt>` (a `struct sensor`), copies without the kernel pointers.
pub fn sysctl_sensors(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let _ = newlen;

    if name.len() != 1 && name.len() != 3 {
        return Err(Errno::ENOTDIR);
    }

    let dev = name[0];
    if name.len() == 1 {
        kernel_lock();
        let ksd = match sensordev_get(dev) {
            Ok(ksd) => ksd,
            Err(e) => {
                kernel_unlock();
                return Err(e);
            }
        };

        // Grab a copy, to clear the kernel pointers
        let mut usd = Sensordev {
            num: ksd.num.get(),
            maxnumt: ksd.maxnumt.get(),
            sensors_count: ksd.sensors_count.get(),
            ..Sensordev::default()
        };
        strlcpy(&mut usd.xname, &ksd.xname.get());
        kernel_unlock();

        return sysctl_rdstruct(oldp, oldlenp, newp, usd.as_bytes());
    }

    // `(enum sensor_type)name[1]`: a value outside the enum matches no sensor.
    let r#type = SensorType::from_i32(name[1]);
    let numt = name[2];

    kernel_lock();
    let ks = match r#type {
        Some(t) => sensor_find(dev, t, numt),
        None => sensordev_get(dev).and(Err(Errno::ENOENT)),
    };
    let ks = match ks {
        Ok(ks) => ks,
        Err(e) => {
            kernel_unlock();
            return Err(e);
        }
    };

    // Grab a copy, to clear the kernel pointers
    let us = Sensor {
        desc: ks.desc.get(),
        tv: ks.tv.get(),
        value: ks.value.get(),
        r#type: ks.r#type.get() as i32,
        status: ks.status.get() as i32,
        numt: ks.numt.get(),
        flags: ks.flags.get(),
    };
    kernel_unlock();

    sysctl_rdstruct(oldp, oldlenp, newp, us.as_bytes())
}

/// `sysctl_cpustats`: `kern.cpustats.<cpu>`.
pub fn sysctl_cpustats(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let _ = newlen;
    let [unit] = *name else {
        return Err(Errno::ENOTDIR);
    };

    let Some(ci) = cpu_by_unit(unit) else {
        return Err(Errno::ENOENT);
    };

    let mut cs = Cpustats {
        cs_time: sysctl_ci_cp_time(ci),
        cs_flags: 0,
    };
    if cpu_is_online(ci) {
        cs.cs_flags |= CPUSTATS_ONLINE;
    }

    sysctl_rdstruct(oldp, oldlenp, newp, cs.as_bytes())
}

/// The `CPU_INFO_FOREACH` search of `sysctl_cpustats` and `sysctl_cptime2`.
fn cpu_by_unit(unit: i32) -> Option<&'static CpuInfo> {
    let mut found = None;
    cpu_info_foreach(&mut |ci| {
        if found.is_none() && i64::from(unit) == i64::from(Machine::cpu_info_unit(ci)) {
            found = Some(ci);
        }
    });
    found
}

/// `sysctl_ci_cp_time`: a consistent copy of a CPU's `spc_cp_time`.
fn sysctl_ci_cp_time(ci: &CpuInfo) -> [u64; CPUSTATES] {
    let spc = Machine::ci_schedstate(ci);
    let mut generation = 0;
    let mut cp_time = [0u64; CPUSTATES];

    pc_cons_enter(&spc.spc_cp_time_lock, &mut generation);
    loop {
        for (t, c) in cp_time.iter_mut().zip(&spc.spc_cp_time) {
            *t = c.load(Ordering::Relaxed);
        }
        if !pc_cons_leave(&spc.spc_cp_time_lock, &mut generation) {
            break;
        }
    }
    cp_time
}

/// `sysctl_cptime2`: `kern.cp_time2.<cpu>`.
pub fn sysctl_cptime2(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let _ = newlen;
    let [unit] = *name else {
        return Err(Errno::ENOTDIR);
    };

    let Some(ci) = cpu_by_unit(unit) else {
        return Err(Errno::ENOENT);
    };

    let cp_time = sysctl_ci_cp_time(ci);

    sysctl_rdstruct(oldp, oldlenp, newp, cp_time.as_bytes())
}

/// `sysctl_utc_offset`: `kern.utc_offset`, in minutes; a change steps the real-time clock
/// by the difference.
pub fn sysctl_utc_offset(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let old_offset_minutes = UTC_OFFSET.load(Ordering::Relaxed) / 60; // seconds -> minutes
    let new_offset_minutes = AtomicI32::new(old_offset_minutes);
    sysctl_securelevel_int(oldp, oldlenp, newp, newlen, &new_offset_minutes)?;
    let new_offset_minutes = new_offset_minutes.into_inner();
    if !(-24 * 60..=24 * 60).contains(&new_offset_minutes) {
        return Err(Errno::EINVAL);
    }
    if new_offset_minutes == old_offset_minutes {
        return Ok(());
    }

    UTC_OFFSET.store(new_offset_minutes * 60, Ordering::Relaxed); // minutes -> seconds
    let adjustment_seconds = (new_offset_minutes - old_offset_minutes) * 60;

    let now = nanotime();
    let mut adjusted = now;
    adjusted.tv_sec -= i64::from(adjustment_seconds);
    tc_setrealtimeclock(&adjusted);
    let _ = unported!("kern.utc_offset: resettodr (kern_time.c)");

    Ok(())
}

#[cfg(test)]
mod tests;
