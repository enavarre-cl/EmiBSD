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
//! The system identifies itself as EmiBSD 7.8 (`kern.ostype`, `kern.osrelease`,
//! `kern.version`, `kern.osversion` come from `conf/vers.rs`); `kern.osrevision` stays the
//! `OpenBSD` API date of `<sys/param.h>`, which is what programs test.
//!
//! Real nodes: `kern.ostype`, `osrelease`, `osrevision`, `version`, `osversion`, `maxproc`,
//! `maxfiles`, `argmax`, `securelevel`, `hostname`, `domainname`, `hostid`, `clockrate`,
//! `posix1version`, `ngroups`, `job_control`, `saved_ids`, `boottime`, `maxpartitions`,
//! `rawpartition`, `maxthread`, `nthreads`, `fsync`, `sysvmsg`/`sysvsem`/`sysvshm` (0: not
//! configured), `msgbufsize`, `msgbuf`, `consbufsize`, `consbuf`, `cp_time`, `cp_time2`,
//! `cpustats`, `forkstat`, `ccpu`, `fscale`, `nprocs`, `allowkmem`, `splassert`, `mbstat`,
//! `proc` (`kinfo_proc`), `proc_nobroadcastkill`, `maxclusters`, `wxabort`, `consdev`,
//! `netlivelocks`, `pool_debug`, `timeout_stats`, `utc_offset`, `autoconf_serial`; `hw.machine`,
//! `ncpu`, `ncpufound`, `ncpuonline`, `byteorder`, `physmem`, `usermem`, `physmem64`,
//! `usermem64`, `pagesize`, `power`, `allowpowerdown`, `ucomnames`, `cpuspeed`, `battery.*`,
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
//!   `ENOSYS`: `ttycount` and `tty` (`tty.c`), `somaxconn`/`sominconn` (`uipc_socket.c`),
//!   `maxlocksperuid` (`vfs_lockf.c`), `stackgap_random` (`kern_exec.c` has no stack gap yet),
//!   `bufcachepercent` (`vfs_bio.c`), `file` (`kern_descrip.c`; `fill_file` is not here),
//!   `malloc` (`sysctl_malloc`), `pool` (`sysctl_dopool`), `intrcnt` and `evcount`
//!   (`evcount_sysctl`), `watchdog` (`kern_watchdog.c`), `clockintr`, `timecounter`
//!   (`sysctl_tc`), `procargs` after its checks (`uvm_io`), `proc_vmmap` after its checks
//!   (`fill_vmmap`); `hw.model`
//!   (`cpu_model`, `identcpu.c`/arm64 `cpu.c`), `disknames`/`diskstats`/`diskcount`
//!   (`subr_disk.c`), `sensors` (`kern_sensors.c`), `setperf`/`perfpolicy` (`sched_bsd.c`),
//!   `smt`/`blockcpu` (`kern_sched.c`); the top-level `net` (`net_sysctl`), `machdep`
//!   (`cpu_sysctl`) and `ddb` (`ddb_sysctl`) trees. `kern.proc_cwd` of a process without a
//!   current directory (none has one before a root file system is mounted) is `ENOENT`. `resettodr`
//!   after a new `kern.utc_offset` is reported and skipped. The tty fields of `kinfo_proc`
//!   (a controlling terminal cannot exist yet) are reported when a process would have them.
//! - Options this kernel does not configure are compiled out as in C: `DEBUG_SYSCTL`
//!   (`debug_sysctl`, `CTL_DEBUG` is `EOPNOTSUPP`), `SYSVMSG`/`SYSVSEM`/`SYSVSHM`
//!   (`sysctl_sysvipc`), `NAUDIO`/`NVIDEO`/`NDT`/`NPF`/`NUCOM` (0), `GPROF`, `WITNESS`,
//!   `PTRACE` (`kern.global_ptrace`), `KTRACE` (the trace members of `kinfo_proc` stay
//!   zero), `MULTIPROCESSOR` (`p_cpuid` stays `KI_NOCPU`). `SMALL_KERNEL` is not set.
//! - `KERNEL_LOCK` is not taken: one CPU and no kernel lock yet. `log_mtx` does not exist
//!   (`subr_log.rs`), so the message buffer header is read without it.
//! - `pledge_sysctl` (`kern_pledge.c`) is its first test: an unpledged process passes; a
//!   pledged one cannot exist yet and would be reported.
//! - `disknames`, `diskstats` and their lengths are not declared: `sysctl_diskinit` reports
//!   `subr_disk.c` before it would touch them.
//! - The morally-const values `sysctl_bounded_arr` reports (`arg_max`, `openbsd`, ...) are
//!   `AtomicI32` statics like the C's `static int`s; `ccpu` is a constant in `sched_bsd.rs`,
//!   so the table points at a read-only copy of it.
//! - `hw_vendor`, `hw_prod`, `hw_uuid`, `hw_serial`, `hw_ver`, `hw_power` and the
//!   `hw_battery_*` globals keep their lowercase C names: their uppercase spellings are the
//!   `HW_*` sysctl ids of `<sys/sysctl.h>`.
//! - `KERN_CPTIME` with no CPU online (which only the host double can produce) leaves the
//!   sums at zero instead of dividing by zero; an empty name (never from `sys_sysctl`, which
//!   wants two components) is `EINVAL`.

use core::sync::atomic::{AtomicI32, Ordering};

use libkern::{StaticCell, strlcpy, strnlen};

use crate::conf::param::{FSCALE, MAXFILES, MAXPROCESS, MAXTHREAD, NMBCLUST, UTC_OFFSET};
use crate::conf::vers::{OSRELEASE, OSTYPE, OSVERSION, VERSION};
use crate::dev::cons::cn_tab;
use crate::kern::init_main::{NCPUS, NCPUSFOUND};
use crate::kern::kern_clock::sysctl_clockrate;
use crate::kern::kern_descrip::NUMFILES;
use crate::kern::kern_fork::{FORKSTAT, NPROCESSES, NTHREADS};
use crate::kern::kern_lock::{mtx_enter, mtx_leave, pc_cons_enter, pc_cons_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_proc::{ALLPROCESS, ZOMBPROCESS, prfind};
use crate::kern::kern_prot::suser;
use crate::kern::kern_resource::{calctsru, tuagg_get_proc, tuagg_get_process};
use crate::kern::kern_rwlock::{rw_enter, rw_enter_write, rw_exit_write};
use crate::kern::kern_sched::{cpu_is_online, sysctl_hwncpuonline};
use crate::kern::kern_sig::NOSUIDCOREDUMP;
use crate::kern::kern_tc::{microboottime, nanoboottime, nanotime, tc_setrealtimeclock};
use crate::kern::kern_timeout::timeout_sysctl;
use crate::kern::sched_bsd;
use crate::kern::subr_autoconf::AUTOCONF_SERIAL;
use crate::kern::subr_log::{consbufp, msgbufp};
use crate::kern::subr_pool::{POOL_DEBUG, pool_reclaim_all};
use crate::kern::subr_prf::{SPLASSERT_CTL, panic};
use crate::kern::uipc_mbuf::{MBSTAT, nmbclust_update};
use crate::kern::vfs_cache::NCHSTATS;
use crate::kern::vfs_getcwd::vfs_getcwd_common;
use crate::kern::vfs_subr::{MAXVNODES, NUMVNODES, vfs_sysctl, vref, vrele};
use crate::machine::Machine;
use crate::machine::copy::{copyin, copyout};
use crate::machine::cpu::{Cpu, CpuInfo, cpu_info_foreach, curproc};
use crate::machine::param::MachineInfo;
use crate::machine::pmap::pmap_resident_count;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_TEMP, M_WAITOK};
use crate::sys::mbuf::{MT_NTYPES, Mbstat, MbstatCounters};
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::msgbuf::{MSG_MAGIC, Msgbuf};
use crate::sys::param::{MAXPATHLEN, MAXPHYS, NODEV, OpenBSD, PAGE_SIZE};
use crate::sys::proc::{
    PS_CONTROLT, PS_EMBRYO, PS_EXITING, PS_INEXEC, PS_NOBROADCASTKILL, PS_PLEDGE, PS_SYSTEM,
    PS_ZOMBIE, Proc, ProcThrLink, Process, SDEAD, SIDL, SONPROC, SRUN, SSLEEP, SSTOP,
    THREAD_PID_OFFSET, TU_ITICKS, TU_STICKS, TU_UTICKS, Tusage,
};
use crate::sys::queue::TailqHead;
use crate::sys::resource::RLIMIT_RSS;
use crate::sys::rwlock::{RW_INTR, RW_WRITE, Rwlock};
use crate::sys::sched::{CPUSTATES, CPUSTATS_ONLINE, Cpustats};
use crate::sys::syscallargs::SysSysctlArgs;
use crate::sys::sysctl::*;
use crate::sys::syslimits;
use crate::sys::systm::{PHYSMEM, SysArgs, sysargs};
use crate::sys::time::timeradd;
use crate::sys::types::Register;
use crate::sys::unistd::_POSIX_VERSION;
use crate::sys::vnode::GETCWD_CHECK_ACCESS;
use crate::unported;
use crate::uvm::uvm_extern::Vmspace;
use crate::uvm::uvm_glue::{uvm_vslock, uvm_vsunlock};
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_map::{uvmspace_addref, uvmspace_free};
use crate::uvm::uvm_meter::uvm_sysctl;
use crate::uvm::uvm_mmap::UVM_WXABORT;
use crate::uvm::uvm_param::atop;

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

/// `kern_vars[]`: the `kern` integers `sysctl_bounded_arr` serves. The ones whose variable
/// lives in an unported file are reported by [`kern_vars`] instead.
static KERN_VARS: [SysctlBoundedArgs; 26] = [
    SysctlBoundedArgs::readonly(KERN_OSREV, &OPENBSD),
    SysctlBoundedArgs::new(KERN_MAXVNODES, &MAXVNODES, 0, i32::MAX),
    SysctlBoundedArgs::new(KERN_MAXPROC, &MAXPROCESS, 0, i32::MAX),
    SysctlBoundedArgs::new(KERN_MAXFILES, &MAXFILES, 0, i32::MAX),
    SysctlBoundedArgs::readonly(KERN_NFILES, &NUMFILES),
    // KERN_TTYCOUNT: tty_count (tty.c).
    SysctlBoundedArgs::readonly(KERN_ARGMAX, &ARG_MAX),
    SysctlBoundedArgs::readonly(KERN_POSIX1, &POSIX_VERSION),
    SysctlBoundedArgs::readonly(KERN_NGROUPS, &NGROUPS_MAX),
    SysctlBoundedArgs::readonly(KERN_JOB_CONTROL, &INT_ONE),
    SysctlBoundedArgs::readonly(KERN_SAVED_IDS, &INT_ONE),
    SysctlBoundedArgs::readonly(KERN_MAXPARTITIONS, &MAXPARTITIONS),
    SysctlBoundedArgs::readonly(KERN_RAWPARTITION, &RAW_PART),
    SysctlBoundedArgs::new(KERN_MAXTHREAD, &MAXTHREAD, 0, i32::MAX),
    SysctlBoundedArgs::readonly(KERN_NTHREADS, &NTHREADS),
    // KERN_SOMAXCONN, KERN_SOMINCONN: somaxconn, sominconn (uipc_socket.c).
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
    // KERN_MAXLOCKSPERUID: maxlocksperuid (vfs_lockf.c).
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

/// `hw_vars[]`. `HW_DISKCOUNT` (`disk_count`, `subr_disk.c`) is reported by `hw_sysctl`.
static HW_VARS: [SysctlBoundedArgs; 5] = [
    SysctlBoundedArgs::readonly(HW_NCPU, &NCPUS),
    SysctlBoundedArgs::readonly(HW_NCPUFOUND, &NCPUSFOUND),
    SysctlBoundedArgs::readonly(HW_BYTEORDER, &BYTE_ORDER),
    SysctlBoundedArgs::readonly(HW_PAGESIZE, &UVMEXP.pagesize),
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
    // KERNEL_LOCK(): one CPU, no kernel lock yet.

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
            // KERNEL_UNLOCK()
            rw_exit_write(&SYSCTL_LOCK);
            return Err(e);
        }
    }

    Ok(())
}

/// `sysctl_vsunlock`: undoes [`sysctl_vslock`].
pub fn sysctl_vsunlock(addr: usize, len: usize) {
    // KERNEL_ASSERT_LOCKED(): no kernel lock yet.

    if addr != 0 {
        let Some(p) = curproc() else {
            panic(format_args!("sysctl_vsunlock: no curproc"));
        };
        uvm_vsunlock(p, addr, len);
    }
    // KERNEL_UNLOCK()
    rw_exit_write(&SYSCTL_LOCK);
}

/// `pledge_sysctl` (`kern_pledge.c`): see the module's deviations.
fn pledge_sysctl(p: &Proc, mib: &[i32], new: usize) -> Result<(), Errno> {
    let _ = (mib, new);
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }
    Err(unported!("pledge_sysctl (kern_pledge.c)"))
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
        CTL_NET => return Err(unported!("net_sysctl (uipc_domain.c)")),
        CTL_VM => (true, uvm_sysctl),
        CTL_VFS => (true, vfs_sysctl),
        CTL_MACHDEP => return Err(unported!("cpu_sysctl (machdep.c)")),
        // CTL_DEBUG: DEBUG_SYSCTL is not configured.
        CTL_DDB => return Err(unported!("ddb_sysctl (db_usrreq.c)")),
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

/// `sysctl_bounded_arr(kern_vars, ...)`, with the variables of unported files reported.
fn kern_vars(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    if let [mib] = name {
        match *mib {
            KERN_TTYCOUNT => return Err(unported!("kern.ttycount: tty_count (tty.c)")),
            KERN_SOMAXCONN | KERN_SOMINCONN => {
                return Err(unported!("kern.somaxconn: somaxconn (uipc_socket.c)"));
            }
            KERN_MAXLOCKSPERUID => {
                return Err(unported!(
                    "kern.maxlocksperuid: maxlocksperuid (vfs_lockf.c)"
                ));
            }
            _ => {}
        }
    }
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
        KERN_MALLOCSTATS => return Err(unported!("kern.malloc: sysctl_malloc (kern_malloc.c)")),
        KERN_CPTIME2 => return sysctl_cptime2(name, oldp, oldlenp, newp, newlen),
        KERN_POOL => return Err(unported!("kern.pool: sysctl_dopool (subr_pool.c)")),
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
        KERN_EVCOUNT => Err(unported!("kern.evcount: evcount_sysctl (subr_evcount.c)")),
        KERN_CLOCKINTR => Err(unported!(
            "kern.clockintr: sysctl_clockintr (kern_clockintr.c)"
        )),
        KERN_TTY => Err(unported!("kern.tty: sysctl_tty (tty.c)")),
        // KERN_PROF: GPROF and DDBPROF are not configured.
        KERN_TIMECOUNTER => Err(unported!("kern.timecounter: sysctl_tc (kern_tc.c)")),
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
        KERN_STACKGAPRANDOM => Err(unported!(
            "kern.stackgap_random: stackgap_random (kern_exec.c)"
        )),
        KERN_CACHEPCT => Err(unported!("kern.bufcachepercent: bufadjust (vfs_bio.c)")),
        // KERN_PFSTATUS: NPF is 0.
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
        HW_DISKCOUNT => Err(unported!("hw.diskcount: disk_count (subr_disk.c)")),
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
        HW_DISKNAMES | HW_DISKSTATS => {
            sysctl_diskinit(name[0] == HW_DISKSTATS, p)?;
            // disknames, diskstats: sysctl_diskinit reported subr_disk.c above.
            Err(Errno::ENOSYS)
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
        HW_SMT => Err(unported!("hw.smt: sysctl_hwsmt (kern_sched.c)")),
        HW_BLOCKCPU => Err(unported!("hw.blockcpu: sysctl_hwblockcpu (kern_sched.c)")),
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

/// `sysctl_file`: get file structures (`kern.file`). The argument checks are the C's; the
/// walk needs `struct file` (`kern_descrip.c`) and is reported.
pub fn sysctl_file(name: &[i32], where_: usize, sizep: &mut usize, p: &Proc) -> Result<(), Errno> {
    let _ = (where_, sizep, p);

    if name.len() > 4 {
        return Err(Errno::ENOTDIR);
    }
    if name.len() < 4 || name[2] < 0 || name[2] as usize > size_of::<KinfoFile>() {
        return Err(Errno::EINVAL);
    }
    if name[2] < 1 {
        return Err(Errno::EINVAL);
    }

    Err(unported!(
        "kern.file: fill_file, fd_iterfile (kern_descrip.c)"
    ))
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
                let _ = unported!("kern.proc: KERN_PROC_TTY t_dev (tty.c)");
                false
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
    // ps_uvpaths, ps_uvdone: unveil (kern_unveil.c) is not ported, so no process has any.
    if flags & PS_PLEDGE != 0 && pr.ps_pledge.get() & PLEDGE_UNVEIL == 0 {
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

    if flags & PS_CONTROLT != 0 && !s.s_ttyp.get().is_null() {
        // p_tdev, p_tpgid, p_tsess: struct tty is tty.c's.
        let _ = unported!("kinfo_proc: the controlling tty (tty.c)");
        ki.p_tdev = NODEV as u32;
        ki.p_tpgid = -1;
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

        // MULTIPROCESSOR is not configured: p_cpuid stays KI_NOCPU.
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

/// `sysctl_proc_args`: `kern.procargs.<pid>.<op>`. The checks are the C's; reading the
/// victim's `ps_strings` and strings needs `uvm_io` and is reported.
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

    Err(unported!("kern.procargs: uvm_io (uvm_io.c)"))
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

/// `sysctl_diskinit`: initialises `disknames`/`diskstats` for export by sysctl (`update`:
/// only refresh the statistics). The disk list is `subr_disk.c`'s and is reported.
pub fn sysctl_diskinit(update: bool, p: &Proc) -> Result<(), Errno> {
    let _ = (update, p);
    // KERNEL_ASSERT_LOCKED(): no kernel lock yet.

    rw_enter(&SYSCTL_DISKLOCK, RW_WRITE | RW_INTR)?;
    let error = Err(unported!(
        "sysctl_diskinit: disklist, disk_change (subr_disk.c)"
    ));
    rw_exit_write(&SYSCTL_DISKLOCK);
    error
}

/// `sysctl_intrcnt`: `kern.intrcnt`, served by `evcount_sysctl` (reported).
pub fn sysctl_intrcnt(name: &[i32], oldp: usize, oldlenp: &mut usize) -> Result<(), Errno> {
    let _ = (name, oldp, oldlenp);
    Err(unported!("kern.intrcnt: evcount_sysctl (subr_evcount.c)"))
}

/// `sysctl_sensors`: `hw.sensors`. The name checks are the C's; the sensor list is
/// `kern_sensors.c`'s and is reported.
pub fn sysctl_sensors(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let _ = (oldp, oldlenp, newp, newlen);

    if name.len() != 1 && name.len() != 3 {
        return Err(Errno::ENOTDIR);
    }

    Err(unported!(
        "hw.sensors: sensordev_get, sensor_find (kern_sensors.c)"
    ))
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
            *t = c.get();
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
