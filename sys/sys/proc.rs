/*	$OpenBSD: proc.h,v 1.401 2026/09/19 17:53:49 gnezdo Exp $	*/
/*	$NetBSD: proc.h,v 1.44 1996/04/22 01:23:21 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1986, 1989, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)proc.h	8.8 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! `<sys/proc.h>`: description of a process.
//!
//! These structures contain the information needed to manage a thread of control, known in
//! UN*X as a process; it has references to substructures containing descriptions of things
//! that the process uses, but may share with related processes.
//!
//! `struct process` is the higher level process containing information shared by all threads
//! in a process, while `struct proc` contains the run-time information needed by threads.
//!
//! Upstream: sys/sys/proc.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part b) ports `struct session`, `struct pgrp`, `struct
//! tusage`, `struct process` and `struct proc` with the members the scheduler, the sleep
//! queues and the kernel threads use, the `S*` states, the `P_*`/`PS_*` flags, the `FORK_*`,
//! `EXIT_*` and `SINGLE_*` constants, `struct cond`, `struct cpuset`, `struct uidinfo` and
//! `tu_enter`/`tu_leave`. The members that belong to subsystems not here yet (`ptrace`,
//! `pinsyscall`, the `vnode`s, the file descriptors) are opaque pointers
//! or left out, each named in a comment at its place. The vmspace, (since `kern_prot.c`)
//! the credentials and (since `kern_sig.c`) the signal actions are typed pointers with
//! accessors (`vmspace()`, `ucred()`, `sigacts()`); `kern_sig.c` also brought the sigio
//! lists, `p_sigstk` and `p_sigval`, and `kern_unveil.c` the unveil table (`ps_uvpaths`, a
//! raw pointer to `kern_unveil.rs`'s `Unveil` slots, with its counts and `ps_uvdone`);
//! `kern_event.c` the process's `ps_klist` and the thread's poll kqueue (`p_kq`,
//! `p_kq_serial`).
//!
//! ## Deviations
//! - Members the owning thread or a lock mutates are `Cell`s; the flag words `p_flag` and
//!   `ps_flags` (`atomic_setbits_int`) are atomics, as are `p_pctcpu`, `p_siglist` and
//!   `ps_siglist`. The two signal lists are `AtomicU32` (`sigset_t`) where the C declares
//!   them `int`; the bits are the same.
//! - `ps_comm`/`p_name` are `[u8; _MAXCOMLEN]` NUL-terminated byte arrays, as in C.
//! - `p_wmesg` is `Option<&'static str>`; the sleep messages are literals.

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64};

use crate::kassert;
use crate::kern::kern_lock::{pc_sprod_enter, pc_sprod_leave};
use crate::kern::kern_proc::SESSION_POOL;
use crate::kern::kern_prot::dorefreshcreds;
use crate::kern::kern_timeout::timeout_del;
use crate::kern::kern_unveil::Unveil;
use crate::kern::subr_pool::pool_put;
use crate::machine::Machine;
use crate::machine::cpu::{CpuInfo, MAXCPUS};
use crate::machine::intr::IPL_HIGH;
use crate::machine::proc::{MachineProc, Mdproc};
use crate::queue_adapter;
use crate::sys::event::Klist;
use crate::sys::eventvar::Kqueue;
use crate::sys::filedesc::Filedesc;
use crate::sys::mutex::Mutex;
use crate::sys::pclock::PcLock;
use crate::sys::queue::{ListEntry, ListHead, TailqEntry, TailqHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::resource::Rusage;
use crate::sys::resourcevar::Plimit;
use crate::sys::rwlock::Rwlock;
use crate::sys::siginfo::Sigval;
use crate::sys::sigio::Sigiolst;
use crate::sys::signal::{Sigaltstack, Sigset};
use crate::sys::signalvar::Sigacts;
use crate::sys::syslimits::LOGIN_NAME_MAX;
use crate::sys::time::{Timespec, Timeval};
use crate::sys::timeout::Timeout;
use crate::sys::tty::Tty;
use crate::sys::types::{Pid, Uid};
use crate::sys::ucred::Ucred;
use crate::sys::user::User;
use crate::sys::vnode::Vnode;
use crate::uvm::uvm_extern::Vmspace;

/// `_MAXCOMLEN`: the command and thread names, NUL included.
pub const _MAXCOMLEN: usize = crate::sys::syslimits::_MAXCOMLEN;

/// `struct session`: one structure allocated per session.
pub struct Session {
    /// `s_count`: ref cnt; pgrps in session.
    pub s_count: Cell<i32>,
    /// `s_leader`: session leader.
    pub s_leader: Cell<*const Process>,
    /// `s_ttyvp`: vnode of controlling terminal, holding a use count (`vref` in `vn_ioctl`'s
    /// `TIOCSCTTY`).
    pub s_ttyvp: Cell<*const Vnode>,
    /// `s_ttyp`: controlling terminal; not cleared when the session loses it, to remember
    /// that it once had one.
    pub s_ttyp: Cell<*const Tty>,
    /// `s_login`: setlogin() name.
    pub s_login: UnsafeCell<[u8; LOGIN_NAME_MAX]>,
    /// `s_verauthppid`.
    pub s_verauthppid: Cell<Pid>,
    /// `s_verauthuid`.
    pub s_verauthuid: Cell<Uid>,
    /// `s_verauthto`.
    pub s_verauthto: Timeout,
}

// SAFETY: sessions are touched under the kernel lock (one CPU here).
unsafe impl Sync for Session {}

impl Session {
    /// A zero session.
    pub const fn new() -> Self {
        Self {
            s_count: Cell::new(0),
            s_leader: Cell::new(ptr::null()),
            s_ttyvp: Cell::new(ptr::null()),
            s_ttyp: Cell::new(ptr::null()),
            s_login: UnsafeCell::new([0; LOGIN_NAME_MAX]),
            s_verauthppid: Cell::new(0),
            s_verauthuid: Cell::new(0),
            s_verauthto: Timeout::zeroed(),
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct pgrp`: one structure allocated per process group.
pub struct Pgrp {
    /// `pg_hash`: hash chain.
    pub pg_hash: ListEntry<Pgrp>,
    /// `pg_members`: pointer to pgrp members.
    pub pg_members: ListHead<ProcessPglist>,
    /// `pg_session`: pointer to session.
    pub pg_session: Cell<*const Session>,
    /// `pg_sigiolst`: list of sigio structures.
    pub pg_sigiolst: Sigiolst,
    /// `pg_id`: pgrp id.
    pub pg_id: Cell<Pid>,
    /// `pg_jobc`: # procs qualifying pgrp for job control.
    pub pg_jobc: Cell<i32>,
}

// SAFETY: process groups are touched under the kernel lock (one CPU here).
unsafe impl Sync for Pgrp {}

impl Pgrp {
    /// A zero process group.
    pub const fn new() -> Self {
        Self {
            pg_hash: ListEntry::new(),
            pg_members: ListHead::new(),
            pg_session: Cell::new(ptr::null()),
            pg_sigiolst: Sigiolst::new(),
            pg_id: Cell::new(0),
            pg_jobc: Cell::new(0),
        }
    }
}

impl Default for Pgrp {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `LIST_ENTRY(pgrp) pg_hash`: the pgrp hash chains.
    pub PgrpHash: Pgrp, pg_hash => ListEntry<Pgrp>
);

/*
 * time usage: accumulated times in ticks
 * Each thread is immediately accumulated here. For processes only the
 * time of exited threads is accumulated and to get the proper process
 * time usage tuagg_get_process() needs to be called.
 * Accounting of threads is done lockless by curproc using the tu_pcl
 * pc_lock. Code should use tu_enter() and tu_leave() for this.
 * The process ps_tu structure is locked by the ps_mtx.
 */

/// `TU_UTICKS`: statclock hits in user mode.
pub const TU_UTICKS: usize = 0;
/// `TU_STICKS`: statclock hits in system mode.
pub const TU_STICKS: usize = 1;
/// `TU_ITICKS`: statclock hits processing intr.
pub const TU_ITICKS: usize = 2;
/// `TU_TICKS_COUNT`.
pub const TU_TICKS_COUNT: usize = 3;

/// `struct pinsyscall`: the system call pin table of a text region.
pub struct Pinsyscall {
    /// `pn_start`.
    pub pn_start: Cell<usize>,
    /// `pn_end`.
    pub pn_end: Cell<usize>,
    /// `pn_pins`: array of offsets indexed by syscall#.
    pub pn_pins: Cell<*mut u32>,
    /// `pn_npins`: number of entries in table.
    pub pn_npins: Cell<i32>,
}

impl Pinsyscall {
    /// No table.
    pub const fn new() -> Self {
        Self {
            pn_start: Cell::new(0),
            pn_end: Cell::new(0),
            pn_pins: Cell::new(core::ptr::null_mut()),
            pn_npins: Cell::new(0),
        }
    }
}

impl Default for Pinsyscall {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct tusage`.
pub struct Tusage {
    /// `tu_pcl`.
    pub tu_pcl: PcLock,
    /// `tu_ticks`: `tu_uticks`, `tu_sticks`, `tu_iticks`.
    pub tu_ticks: [Cell<u64>; TU_TICKS_COUNT],
    /// `tu_ixrss`.
    pub tu_ixrss: Cell<u64>,
    /// `tu_idrss`.
    pub tu_idrss: Cell<u64>,
    /// `tu_isrss`.
    pub tu_isrss: Cell<u64>,
    /// `tu_runtime`: realtime.
    pub tu_runtime: Cell<Timespec>,
}

// SAFETY: written by the owning thread under `tu_enter`/`tu_leave` (or under `ps_mtx` for a
// process's), read by consumers that check the generation.
unsafe impl Sync for Tusage {}

impl Tusage {
    /// All zero.
    pub const fn new() -> Self {
        Self {
            tu_pcl: PcLock::new(),
            tu_ticks: [const { Cell::new(0) }; TU_TICKS_COUNT],
            tu_ixrss: Cell::new(0),
            tu_idrss: Cell::new(0),
            tu_isrss: Cell::new(0),
            tu_runtime: Cell::new(Timespec::new(0, 0)),
        }
    }
}

impl Default for Tusage {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct process`.
///
/// Locks used to protect struct members in this file:
/// - I: immutable after creation
/// - a: atomic operations
/// - K: kernel lock
/// - m: this process' `ps_mtx`
/// - p: this process' `ps_lock`
/// - Q: kqueue_ps_list_lock
/// - R: rlimit_lock
/// - S: scheduler lock
/// - T: itimer_mtx
pub struct Process {
    /// `ps_refcnt`.
    pub ps_refcnt: Refcnt,

    /// `ps_mainproc` is the original thread in the process. It's only still special for the
    /// handling of some signal and ptrace behaviors that need to be fixed.
    pub ps_mainproc: Cell<*const Proc>,
    /// `ps_ucred`: process owner's identity; the process holds a reference.
    pub ps_ucred: Cell<*const Ucred>,

    /// `ps_list`: list of all processes.
    pub ps_list: ListEntry<Process>,
    /// \[K|m\] `ps_threads`: threads in this process.
    pub ps_threads: TailqHead<ProcThrLink>,

    /// `ps_pglist`: list of processes in pgrp.
    pub ps_pglist: ListEntry<Process>,
    /// \[K|m\] `ps_pptr`: pointer to parent process.
    pub ps_pptr: Cell<*const Process>,
    /// `ps_sibling`: list of sibling processes.
    pub ps_sibling: ListEntry<Process>,
    /// `ps_children`: pointer to list of children.
    pub ps_children: ListHead<ProcessSibling>,
    /// `ps_hash`: hash chain.
    pub ps_hash: ListEntry<Process>,

    /// `ps_orphan`: list of orphan processes. An orphan is the child that has been
    /// re-parented to the debugger as a result of attaching to it. Need to keep track of
    /// them for parent to be able to collect the exit status of what used to be children.
    pub ps_orphan: ListEntry<Process>,
    /// `ps_orphans`: pointer to list of orphans.
    pub ps_orphans: ListHead<ProcessOrphan>,

    /// `ps_sigiolst`: list of sigio structures.
    pub ps_sigiolst: Sigiolst,
    /// \[I\] `ps_sigacts`: signal actions, state.
    pub ps_sigacts: Cell<*const Sigacts>,
    /// `ps_textvp`: vnode of executable, referenced (`None` for the boot module image).
    pub ps_textvp: Cell<Option<&'static crate::sys::vnode::Vnode>>,
    /// `ps_fd`: ptr to open files structure; the process holds a reference (`fd()`).
    pub ps_fd: Cell<*const Filedesc>,
    /// `ps_vmspace`: address space.
    pub ps_vmspace: Cell<*const Vmspace>,
    /// \[I\] `ps_pid`: process identifier.
    pub ps_pid: Cell<Pid>,

    /// `ps_lock`: per-process rwlock.
    pub ps_lock: Rwlock,
    /// `ps_mtx`: per-process mutex.
    pub ps_mtx: Mutex,

    // The following fields are all zeroed upon creation in process_new (ps_startzero).
    /// \[Q,m\] `ps_klist`: knotes attached to process (locked by `ps_mtx` once
    /// `process_initialize` ran `klist_init_mutex`).
    pub ps_klist: Klist,
    /// \[a\] `ps_flags`: `PS_*` flags.
    pub ps_flags: AtomicU32,
    /// `ps_siglist`: signals pending for the process.
    pub ps_siglist: AtomicU32,

    /// \[m\] `ps_single`: thread for single-threading.
    pub ps_single: Cell<*const Proc>,
    /// \[m\] `ps_trapped`: thread trapped for ptrace.
    pub ps_trapped: Cell<*const Proc>,
    /// \[m\] `ps_suspendcnt`: number of threads to suspend.
    pub ps_suspendcnt: Cell<u32>,
    /// \[m\] `ps_exitcnt`: number of threads in exit1.
    pub ps_exitcnt: Cell<u32>,

    // ps_traceflag, ps_tracevp, ps_tracecred: ktrace (M6).
    /// `ps_xexit`: exit status for wait.
    pub ps_xexit: Cell<u32>,
    /// `ps_xsig`: stopping or killing signal.
    pub ps_xsig: Cell<i32>,

    /// \[K|m\] `ps_ppid`: cached parent pid.
    pub ps_ppid: Cell<Pid>,
    // ps_ptmask, ps_ptstat: ptrace (sys_process.c, not ported).
    /// \[K|m\] `ps_opptr`: old parent during ptrace (null: ptrace is not ported, so nothing
    /// attaches).
    pub ps_opptr: Cell<*const Process>,
    /// `ps_ru`: sum of stats for dead threads (`struct rusage`, pooled).
    pub ps_ru: Cell<*const Rusage>,
    /// \[m\] `ps_tu`: accumul times of dead threads.
    pub ps_tu: Tusage,
    /// `ps_cru`: sum of stats for reaped children.
    pub ps_cru: Rusage,
    // ps_timer[3]: the interval timers (M5-b, kern_time.c).
    /// `ps_rucheck_to`: resource limit check timer.
    pub ps_rucheck_to: Timeout,
    /// `ps_nextxcpu`: when to send next SIGXCPU, in seconds of process runtime.
    pub ps_nextxcpu: Cell<i64>,

    /// `ps_wxcounter`.
    pub ps_wxcounter: Cell<u64>,

    /// `ps_uvpaths`: unveil vnodes and names (`UNVEIL_MAX_VNODES` slots from
    /// `mallocarray(M_PROC)`, NULL until the first `unveil(2)`; `kern_unveil.rs`).
    pub ps_uvpaths: Cell<*mut Unveil>,
    /// `ps_uvvcount`: count of unveil vnodes held.
    pub ps_uvvcount: Cell<isize>,
    /// `ps_uvncount`: count of unveil names allocated.
    pub ps_uvncount: Cell<usize>,
    /// `ps_uvdone`: no more unveil is permitted.
    pub ps_uvdone: Cell<i32>,
    // End area that is zeroed on creation (ps_endzero = ps_startcopy).

    // The following fields are all copied upon creation in process_new (ps_startcopy).
    /// \[m,R\] `ps_limit`: process limits.
    pub ps_limit: Cell<*const Plimit>,
    /// \[K|m\] `ps_pgrp`: pointer to process group.
    pub ps_pgrp: Cell<*const Pgrp>,

    /// `ps_comm`: command name, incl NUL.
    pub ps_comm: UnsafeCell<[u8; _MAXCOMLEN]>,

    /// `ps_strings`: user pointers to argv/env.
    pub ps_strings: Cell<usize>,
    /// `ps_auxinfo`: user pointer to auxinfo.
    pub ps_auxinfo: Cell<usize>,
    /// `ps_timekeep`: user pointer to timekeep.
    pub ps_timekeep: Cell<usize>,
    /// \[I\] `ps_sigcode`: user pointer to signal code.
    pub ps_sigcode: Cell<usize>,
    /// \[I\] `ps_sigcoderet`: user ptr to sigreturn retPC.
    pub ps_sigcoderet: Cell<usize>,
    /// \[I\] `ps_sigcookie`.
    pub ps_sigcookie: Cell<u64>,
    /// \[a\] `ps_rtableid`: process routing table/domain.
    pub ps_rtableid: AtomicU32,
    /// \[I\] `ps_iflags`: flags set at exec time.
    pub ps_iflags: Cell<u16>,
    /// `ps_nice`: process "nice" value.
    pub ps_nice: Cell<u8>,

    // ps_prof: profile arguments (M6).
    /// `ps_acflag`: accounting flags.
    pub ps_acflag: Cell<u32>,

    /// \[m\] `ps_pledge`: pledge promises; an atomic because `pledge_syscall` reads it without
    /// the lock (the C's `READ_ONCE`).
    pub ps_pledge: AtomicU64,
    /// \[m\] `ps_execpledge`: execpledge promises.
    pub ps_execpledge: Cell<u64>,

    /// \[m\] `ps_kbind_cookie`.
    pub ps_kbind_cookie: Cell<i64>,
    /// \[m\] `ps_kbind_addr`.
    pub ps_kbind_addr: Cell<usize>,
    /// `ps_pin`: static or ld.so.
    pub ps_pin: Pinsyscall,
    /// `ps_libcpin`: libc.so, from pinsyscalls(2).
    pub ps_libcpin: Pinsyscall,
    // End area that is copied on creation (ps_endcopy = ps_threadcnt).
    /// \[m\] `ps_threadcnt`: number of threads.
    pub ps_threadcnt: Cell<u32>,

    /// `ps_start`: starting uptime.
    pub ps_start: Cell<Timespec>,
    /// \[m\] `ps_realit_to`: `ITIMER_REAL` timeout.
    pub ps_realit_to: Timeout,
    /// \[m\] `ps_timer[ITIMER_REAL]`, \[T\] `ps_timer[ITIMER_VIRTUAL, ITIMER_PROF]`
    /// (`itimer_mtx`): the interval timers (kept with the other members here; the C zeroes
    /// it in `process_new` as part of the zeroed area).
    pub ps_timer: [Cell<crate::sys::_time::Itimerspec>; 3],
}

// SAFETY: the members are locked as the C's annotations say ([K] the kernel lock, [m] `ps_mtx`,
// [S] `sched_lock`, [a] atomics, [I] immutable); without `MULTIPROCESSOR` the kernel lock is
// the lack of preemption.
unsafe impl Sync for Process {}

impl Process {
    /// A zero process (what a `static struct process` holds).
    pub const fn new() -> Self {
        Self {
            ps_refcnt: Refcnt::new(),
            ps_mainproc: Cell::new(ptr::null()),
            ps_ucred: Cell::new(ptr::null()),
            ps_list: ListEntry::new(),
            ps_threads: TailqHead::new(),
            ps_pglist: ListEntry::new(),
            ps_pptr: Cell::new(ptr::null()),
            ps_sibling: ListEntry::new(),
            ps_children: ListHead::new(),
            ps_hash: ListEntry::new(),
            ps_orphan: ListEntry::new(),
            ps_orphans: ListHead::new(),
            ps_sigiolst: Sigiolst::new(),
            ps_sigacts: Cell::new(ptr::null()),
            ps_textvp: Cell::new(None),
            ps_fd: Cell::new(ptr::null()),
            ps_vmspace: Cell::new(ptr::null()),
            ps_pid: Cell::new(0),
            ps_lock: Rwlock::new("pslock"),
            ps_mtx: Mutex::new(IPL_HIGH),
            ps_klist: Klist::new(),
            ps_flags: AtomicU32::new(0),
            ps_siglist: AtomicU32::new(0),
            ps_single: Cell::new(ptr::null()),
            ps_trapped: Cell::new(ptr::null()),
            ps_suspendcnt: Cell::new(0),
            ps_exitcnt: Cell::new(0),
            ps_xexit: Cell::new(0),
            ps_xsig: Cell::new(0),
            ps_ppid: Cell::new(0),
            ps_opptr: Cell::new(ptr::null()),
            ps_ru: Cell::new(ptr::null()),
            ps_tu: Tusage::new(),
            ps_cru: Rusage::new(),
            ps_rucheck_to: Timeout::zeroed(),
            ps_nextxcpu: Cell::new(0),
            ps_wxcounter: Cell::new(0),
            ps_uvpaths: Cell::new(ptr::null_mut()),
            ps_uvvcount: Cell::new(0),
            ps_uvncount: Cell::new(0),
            ps_uvdone: Cell::new(0),
            ps_limit: Cell::new(ptr::null()),
            ps_pgrp: Cell::new(ptr::null()),
            ps_comm: UnsafeCell::new([0; _MAXCOMLEN]),
            ps_strings: Cell::new(0),
            ps_auxinfo: Cell::new(0),
            ps_timekeep: Cell::new(0),
            ps_sigcode: Cell::new(0),
            ps_sigcoderet: Cell::new(0),
            ps_sigcookie: Cell::new(0),
            ps_rtableid: AtomicU32::new(0),
            ps_iflags: Cell::new(0),
            ps_nice: Cell::new(0),
            ps_acflag: Cell::new(0),
            ps_pledge: AtomicU64::new(0),
            ps_execpledge: Cell::new(0),
            ps_kbind_cookie: Cell::new(0),
            ps_kbind_addr: Cell::new(0),
            ps_pin: Pinsyscall::new(),
            ps_libcpin: Pinsyscall::new(),
            ps_threadcnt: Cell::new(0),
            ps_start: Cell::new(Timespec::new(0, 0)),
            ps_realit_to: Timeout::zeroed(),
            ps_timer: [const { Cell::new(crate::sys::_time::Itimerspec::new()) }; 3],
        }
    }

    /// `ps_comm` as a byte string up to its NUL.
    pub fn comm(&self) -> &[u8] {
        // SAFETY: written at creation and by `setproctitle`-like paths under the kernel
        // lock; read for printing.
        let comm = unsafe { &*self.ps_comm.get() };
        let n = comm.iter().position(|&c| c == 0).unwrap_or(comm.len());
        &comm[..n]
    }

    /// `pr->ps_vmspace`: the address space, which the process holds a reference to from
    /// `fork1` (or `main` for process 0) until the reaper's `uvm_exit`.
    pub fn vmspace(&self) -> &'static Vmspace {
        let vm = self.ps_vmspace.get();
        kassert!(!vm.is_null());
        // SAFETY: non-null while the process holds its reference; `uvmspace_free` runs only
        // after `uvm_exit` cleared the pointer.
        unsafe { &*vm }
    }

    /// `pr->ps_ucred`: the process's credentials, which it holds a reference to from
    /// `process_initialize` until `process_zap`.
    pub fn ucred(&self) -> &'static Ucred {
        let cr = self.ps_ucred.get();
        kassert!(!cr.is_null());
        // SAFETY: non-null while the process holds its reference; the set*id calls and exec
        // replace the pointer under the kernel lock and drop the old reference after, so a
        // credential read here stays allocated for as long as the caller runs without
        // sleeping.
        unsafe { &*cr }
    }

    /// `strlcpy(pr->ps_comm, name, sizeof pr->ps_comm)`.
    pub fn set_comm(&self, name: &[u8]) {
        // SAFETY: as for `comm`.
        let comm = unsafe { &mut *self.ps_comm.get() };
        let n = name.len().min(comm.len() - 1);
        comm[..n].copy_from_slice(&name[..n]);
        comm[n..].fill(0);
    }

    /// `pr->ps_sigacts`: the process's signal actions, which it holds from `process_new`
    /// (`sigacts0` for process 0) until `process_zap`.
    pub fn sigacts(&self) -> &'static Sigacts {
        let ps = self.ps_sigacts.get();
        kassert!(!ps.is_null());
        // SAFETY: non-null while the process exists: `sigactsinit` set it before the process
        // was visible and `sigactsfree` runs only in `process_zap`.
        unsafe { &*ps }
    }

    /// `ps_session`: `ps_pgrp->pg_session`.
    pub fn session(&self) -> *const Session {
        // SAFETY: a process's pgrp is set before the process is visible and stays valid
        // while the process is in it.
        unsafe { self.ps_pgrp.get().as_ref() }.map_or(ptr::null(), |pg| pg.pg_session.get())
    }

    /// `ps_pgid`: `ps_pgrp->pg_id`.
    pub fn pgid(&self) -> Pid {
        // SAFETY: as for `session`.
        unsafe { self.ps_pgrp.get().as_ref() }.map_or(0, |pg| pg.pg_id.get())
    }
}

impl Default for Process {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `LIST_ENTRY(process) ps_list`: `allprocess`/`zombprocess`.
    pub ProcessList: Process, ps_list => ListEntry<Process>
);
queue_adapter!(
    /// `LIST_ENTRY(process) ps_pglist`: a pgrp's members.
    pub ProcessPglist: Process, ps_pglist => ListEntry<Process>
);
queue_adapter!(
    /// `LIST_ENTRY(process) ps_sibling`: a parent's children.
    pub ProcessSibling: Process, ps_sibling => ListEntry<Process>
);
queue_adapter!(
    /// `LIST_ENTRY(process) ps_hash`: the pid hash chains.
    pub ProcessHash: Process, ps_hash => ListEntry<Process>
);
queue_adapter!(
    /// `LIST_ENTRY(process) ps_orphan`: a debugger's orphans.
    pub ProcessOrphan: Process, ps_orphan => ListEntry<Process>
);

/*
 * These flags are kept in ps_flags.
 *
 * When adding a new flag, carefully consider whether it should be
 * added to PS_FLAGS_INHERITED_ON_FORK.
 */

/// `PS_CONTROLT`: has a controlling terminal.
pub const PS_CONTROLT: u32 = 0x0000_0001;
/// `PS_EXEC`: process called exec.
pub const PS_EXEC: u32 = 0x0000_0002;
/// `PS_INEXEC`: process is doing an exec right now.
pub const PS_INEXEC: u32 = 0x0000_0004;
/// `PS_EXITING`: process is exiting.
pub const PS_EXITING: u32 = 0x0000_0008;
/// `PS_SUGID`: had set id privs since last exec.
pub const PS_SUGID: u32 = 0x0000_0010;
/// `PS_SUGIDEXEC`: last execve() was set[ug]id.
pub const PS_SUGIDEXEC: u32 = 0x0000_0020;
/// `PS_PPWAIT`: parent waits for exec/exit.
pub const PS_PPWAIT: u32 = 0x0000_0040;
/// `PS_ISPWAIT`: is parent of PPWAIT child.
pub const PS_ISPWAIT: u32 = 0x0000_0080;
/// `PS_PROFIL`: has started profiling.
pub const PS_PROFIL: u32 = 0x0000_0100;
/// `PS_TRACED`: being ptraced.
pub const PS_TRACED: u32 = 0x0000_0200;
/// `PS_WAITED`: stopped proc was waited for.
pub const PS_WAITED: u32 = 0x0000_0400;
/// `PS_COREDUMP`: busy coredumping.
pub const PS_COREDUMP: u32 = 0x0000_0800;
/// `PS_SINGLEEXIT`: other threads must die.
pub const PS_SINGLEEXIT: u32 = 0x0000_1000;
/// `PS_SINGLEUNWIND`: other threads must unwind.
pub const PS_SINGLEUNWIND: u32 = 0x0000_2000;
/// `PS_NOZOMBIE`: no signal or zombie at exit.
pub const PS_NOZOMBIE: u32 = 0x0000_4000;
/// `PS_STOPPING`: just stopped, need sig to parent.
pub const PS_STOPPING: u32 = 0x0000_8000;
/// `PS_SYSTEM`: no sigs, stats or swapping.
pub const PS_SYSTEM: u32 = 0x0001_0000;
/// `PS_EMBRYO`: new process, not yet fledged.
pub const PS_EMBRYO: u32 = 0x0002_0000;
/// `PS_ZOMBIE`: dead and ready to be waited for.
pub const PS_ZOMBIE: u32 = 0x0004_0000;
/// `PS_NOBROADCASTKILL`: process excluded from kill -1.
pub const PS_NOBROADCASTKILL: u32 = 0x0008_0000;
/// `PS_PLEDGE`: has called pledge(2).
pub const PS_PLEDGE: u32 = 0x0010_0000;
/// `PS_EXECPLEDGE`: has exec pledges.
pub const PS_EXECPLEDGE: u32 = 0x0040_0000;
/// `PSI_WXNEEDED`: process allowed to violate W^X (`ps_iflags`).
pub const PSI_WXNEEDED: u16 = 0x0001;
/// `PSI_NOBTCFI`: no Branch Target CFI.
pub const PSI_NOBTCFI: u16 = 0x0002;
/// `PSI_PROFILE`: linked with -pg: allow profile(2).
pub const PSI_PROFILE: u16 = 0x0004;

/// `BOGO_PC`: an address that can't be in userspace or kernelspace (`ps_kbind_addr`).
pub const BOGO_PC: usize = usize::MAX;

/// `PS_ORPHAN`: process is on an orphan list.
pub const PS_ORPHAN: u32 = 0x0080_0000;
/// `PS_CHROOT`: process is chrooted.
pub const PS_CHROOT: u32 = 0x0100_0000;
/// `PS_ITIMER`: virtual interval timers running.
pub const PS_ITIMER: u32 = 0x0400_0000;
/// `PS_WAITEVENT`: wait(2) event pending.
pub const PS_WAITEVENT: u32 = 0x1000_0000;
/// `PS_CONTINUED`: continued proc not yet waited for.
pub const PS_CONTINUED: u32 = 0x2000_0000;
/// `PS_STOPPED`: stopped process.
pub const PS_STOPPED: u32 = 0x4000_0000;
/// `PS_TRAPPED`: stopped due to tracing event.
pub const PS_TRAPPED: u32 = 0x8000_0000;

/// `PS_BITS`: the `%b` format of `ps_flags`.
pub const PS_BITS: &[u8] = b"\x20\x01CONTROLT\x02EXEC\x03INEXEC\x04EXITING\x05SUGID\x06SUGIDEXEC\x07PPWAIT\x08ISPWAIT\x09PROFIL\x0aTRACED\x0bWAITED\x0cCOREDUMP\x0dSINGLEEXIT\x0eSINGLEUNWIND\x0fNOZOMBIE\x10STOPPING\x11SYSTEM\x12EMBRYO\x13ZOMBIE\x14NOBROADCASTKILL\x15PLEDGE\x17EXECPLEDGE\x18ORPHAN\x19CHROOT\x1bITIMER\x1dWAITEVENT\x1eCONTINUED\x1fSTOPPED\x20TRAPPED";

/// `PS_FLAGS_INHERITED_ON_FORK`.
pub const PS_FLAGS_INHERITED_ON_FORK: u32 =
    PS_SUGID | PS_SUGIDEXEC | PS_PLEDGE | PS_EXECPLEDGE | PS_CHROOT;

/// `struct p_inentry`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PInentry {
    /// `ie_serial`.
    pub ie_serial: u64,
    /// `ie_start`.
    pub ie_start: usize,
    /// `ie_end`.
    pub ie_end: usize,
}

/// `struct proc`: a thread.
///
/// Locks used to protect struct members in this file:
/// - I: immutable after creation
/// - S: scheduler lock
/// - U: uidinfolk
/// - l: read only reference, see `lim_read_enter()`
/// - o: owned (modified only) by this thread
/// - m: this proc's `p->p_p->ps_mtx`
pub struct Proc {
    /// \[S\] `p_runq`: current run/sleep queue.
    pub p_runq: TailqEntry<Proc>,
    /// `p_list`: list of all threads.
    pub p_list: ListEntry<Proc>,

    /// \[I\] `p_p`: the process of this thread.
    pub p_p: Cell<*const Process>,
    /// \[K|m\] `p_thr_link`: threads in a process linkage.
    pub p_thr_link: TailqEntry<Proc>,

    // substructures:
    /// `p_fd`: copy of `p_p->ps_fd` (`fd()`).
    pub p_fd: Cell<*const Filedesc>,
    /// \[I\] `p_vmspace`: copy of `p_p->ps_vmspace`.
    pub p_vmspace: Cell<*const Vmspace>,
    /// \[o\] `p_spinentry`: cache for SP check.
    pub p_spinentry: Cell<PInentry>,

    /// `p_flag`: `P_*` flags.
    pub p_flag: AtomicI32,
    /// \[S\] `p_stat`: `S*` process status.
    pub p_stat: Cell<u8>,
    /// \[S\] `p_runpri`: runqueue priority.
    pub p_runpri: Cell<u8>,
    /// `p_descfd`: if not 255, fdesc permits this fd.
    pub p_descfd: Cell<u8>,

    /// `p_tid`: thread identifier.
    pub p_tid: Cell<Pid>,
    /// `p_hash`: hash chain.
    pub p_hash: ListEntry<Proc>,

    // The following fields are all zeroed upon creation in fork (p_startzero = p_dupfd).
    /// `p_dupfd`: sideways return value from filedescopen. XXX
    pub p_dupfd: Cell<i32>,

    // scheduling
    /// \[o\] `p_cpticks`: ticks of cpu time.
    pub p_cpticks: Cell<u32>,
    /// \[K\] `p_cpticks2`: last times ticks.
    pub p_cpticks2: Cell<u32>,
    /// \[S\] `p_wchan`: sleep address.
    pub p_wchan: Cell<*const c_void>,
    /// `p_sleep_to`: timeout for tsleep().
    pub p_sleep_to: Timeout,
    /// \[S\] `p_wmesg`: reason for sleep.
    pub p_wmesg: Cell<Option<&'static str>>,
    /// \[a\] `p_pctcpu` (volatile): %cpu for this thread.
    pub p_pctcpu: AtomicU32,
    /// \[S\] `p_slptime`: time since last blocked.
    pub p_slptime: Cell<u32>,
    /// \[S\] `p_cpu` (volatile): CPU we're running on.
    pub p_cpu: Cell<*const CpuInfo>,

    /// `p_ru`: statistics.
    pub p_ru: Rusage,
    /// \[o\] `p_tu`: accumulated times.
    pub p_tu: Tusage,

    /// \[l\] `p_limit`: read ref. of `p_p->ps_limit`.
    pub p_limit: Cell<*const Plimit>,
    // p_kd: kcov device handle; p_sleeplocks: WITNESS (not configured).
    /// \[o\] `p_kq`: for select/poll (`kqpoll_init`), null until the first call; the
    /// thread holds a reference (`p.kq()`).
    pub p_kq: Cell<*const Kqueue>,
    /// \[o\] `p_kq_serial`: for select/poll, the first serial of the next call.
    pub p_kq_serial: Cell<u64>,
    /// \[a\] `p_siglist`: signals arrived & not delivered.
    pub p_siglist: AtomicU32,

    // End area that is zeroed on creation (p_endzero = p_startcopy).

    // The following fields are all copied upon creation in fork (p_startcopy = p_sigmask).
    /// \[o\] `p_sigmask`: current signal mask.
    pub p_sigmask: Cell<Sigset>,

    /// `p_name`: thread name, incl NUL.
    pub p_name: UnsafeCell<[u8; _MAXCOMLEN]>,
    /// \[S\] `p_slppri`: sleeping priority.
    pub p_slppri: Cell<u8>,
    /// \[S\] `p_usrpri`: priority based on `p_estcpu` & `ps_nice`.
    pub p_usrpri: Cell<u8>,
    /// \[S\] `p_estcpu`: time averaged val of `p_cpticks`.
    pub p_estcpu: Cell<u32>,
    /// `p_pledge_syscall`: cache of current syscall.
    pub p_pledge_syscall: Cell<i32>,
    /// \[o\] `p_pledge`: copy of `p_p->ps_pledge`.
    pub p_pledge: Cell<u64>,

    /// \[o\] `p_ucred`: cached credentials; the thread holds a reference.
    pub p_ucred: Cell<*const Ucred>,
    /// `p_sigstk`: sp & on stack state variable.
    pub p_sigstk: Cell<Sigaltstack>,
    /// `p_prof_addr`: tmp storage for profiling addr until AST.
    pub p_prof_addr: Cell<u64>,
    /// `p_prof_ticks`: tmp storage for profiling ticks until AST.
    pub p_prof_ticks: Cell<u64>,

    // End area that is copied on creation (p_endcopy = p_addr).
    /// `p_addr`: kernel virtual addr of u-area.
    pub p_addr: Cell<*const User>,
    /// `p_md`: any machine-dependent fields.
    pub p_md: Mdproc,

    /// \[o\] `p_oldmask`: saved mask from before sigpause.
    pub p_oldmask: Cell<Sigset>,
    /// `p_sisig`: for core dump/debugger XXX.
    pub p_sisig: Cell<i32>,
    /// `p_sigval`: for core dump/debugger XXX.
    pub p_sigval: Cell<Sigval>,
    /// `p_sitrapno`: for core dump/debugger XXX.
    pub p_sitrapno: Cell<u64>,
    /// `p_sicode`: for core dump/debugger XXX.
    pub p_sicode: Cell<i32>,
}

// SAFETY: the members are locked as the C's annotations say ([S] `sched_lock`, [o] the CPU
// running the thread, [K] the kernel lock, [a] atomics); without `MULTIPROCESSOR` the kernel
// lock is the lack of preemption.
unsafe impl Sync for Proc {}

impl Proc {
    /// A zero thread (what `static struct proc proc0` holds).
    pub const fn new() -> Self {
        Self {
            p_runq: TailqEntry::new(),
            p_list: ListEntry::new(),
            p_p: Cell::new(ptr::null()),
            p_thr_link: TailqEntry::new(),
            p_fd: Cell::new(ptr::null()),
            p_vmspace: Cell::new(ptr::null()),
            p_spinentry: Cell::new(PInentry {
                ie_serial: 0,
                ie_start: 0,
                ie_end: 0,
            }),
            p_flag: AtomicI32::new(0),
            p_stat: Cell::new(0),
            p_runpri: Cell::new(0),
            p_descfd: Cell::new(0),
            p_tid: Cell::new(0),
            p_hash: ListEntry::new(),
            p_dupfd: Cell::new(0),
            p_cpticks: Cell::new(0),
            p_cpticks2: Cell::new(0),
            p_wchan: Cell::new(ptr::null()),
            p_sleep_to: Timeout::zeroed(),
            p_wmesg: Cell::new(None),
            p_pctcpu: AtomicU32::new(0),
            p_slptime: Cell::new(0),
            p_cpu: Cell::new(ptr::null()),
            p_ru: Rusage::new(),
            p_tu: Tusage::new(),
            p_limit: Cell::new(ptr::null()),
            p_kq: Cell::new(ptr::null()),
            p_kq_serial: Cell::new(0),
            p_siglist: AtomicU32::new(0),
            p_sigmask: Cell::new(0),
            p_name: UnsafeCell::new([0; _MAXCOMLEN]),
            p_slppri: Cell::new(0),
            p_usrpri: Cell::new(0),
            p_estcpu: Cell::new(0),
            p_pledge_syscall: Cell::new(0),
            p_pledge: Cell::new(0),
            p_ucred: Cell::new(ptr::null()),
            p_sigstk: Cell::new(Sigaltstack {
                ss_sp: 0,
                ss_size: 0,
                ss_flags: 0,
                _pad: 0,
            }),
            p_prof_addr: Cell::new(0),
            p_prof_ticks: Cell::new(0),
            p_addr: Cell::new(ptr::null()),
            p_md: <Machine as MachineProc>::MDPROC_INIT,
            p_oldmask: Cell::new(0),
            p_sisig: Cell::new(0),
            p_sigval: Cell::new(Sigval::from_ptr(0)),
            p_sitrapno: Cell::new(0),
            p_sicode: Cell::new(0),
        }
    }

    /// `p->p_vmspace`: the thread's copy of its process's address space pointer.
    pub fn vmspace(&self) -> &'static Vmspace {
        let vm = self.p_vmspace.get();
        kassert!(!vm.is_null());
        // SAFETY: as for `Process::vmspace`; the reaper nulls the thread's copy after the
        // thread is dead.
        unsafe { &*vm }
    }

    /// `p->p_ucred`: the thread's cached credentials, which it holds a reference to from
    /// `thread_new` (or `main` for `proc0`) until `proc_free`.
    pub fn ucred(&self) -> &'static Ucred {
        let cr = self.p_ucred.get();
        kassert!(!cr.is_null());
        // SAFETY: non-null while the thread holds its reference; only the thread itself
        // replaces it (`dorefreshcreds`, exec), dropping the old reference afterwards.
        unsafe { &*cr }
    }

    /// `p->p_p`: the thread's process.
    pub fn process(&self) -> &Process {
        // SAFETY: `p_p` is set before the thread is visible and the process outlives its
        // threads.
        unsafe { &*self.p_p.get() }
    }

    /// `p->p_addr->u_pcb`: the thread's process control block.
    pub fn pcb(&self) -> &crate::machine::proc::Pcb {
        // SAFETY: `p_addr` names the thread's u-area, alive while the thread is.
        unsafe { &(*self.p_addr.get()).u_pcb }
    }

    /// `p->p_cpu`.
    pub fn cpu(&self) -> Option<&'static CpuInfo> {
        // SAFETY: `p_cpu` names a static `cpu_info`.
        unsafe { self.p_cpu.get().as_ref() }
    }

    /// `p_name` as a byte string up to its NUL.
    pub fn name(&self) -> &[u8] {
        // SAFETY: written at creation and by `setthrname` under the kernel lock.
        let name = unsafe { &*self.p_name.get() };
        let n = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        &name[..n]
    }

    /// `strlcpy(p->p_name, name, sizeof p->p_name)` (`memset` when `name` is empty).
    pub fn set_name(&self, name: &[u8]) {
        // SAFETY: as for `name`: the thread writes its own name under the kernel lock.
        let buf = unsafe { &mut *self.p_name.get() };
        let n = name.len().min(buf.len() - 1);
        buf[..n].copy_from_slice(&name[..n]);
        buf[n..].fill(0);
    }
}

impl Default for Proc {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_ENTRY(proc) p_runq`: the run and sleep queues.
    pub ProcRunq: Proc, p_runq => TailqEntry<Proc>
);
queue_adapter!(
    /// `LIST_ENTRY(proc) p_list`: `allproc`.
    pub ProcList: Proc, p_list => ListEntry<Proc>
);
queue_adapter!(
    /// `TAILQ_ENTRY(proc) p_thr_link`: a process's threads.
    pub ProcThrLink: Proc, p_thr_link => TailqEntry<Proc>
);
queue_adapter!(
    /// `LIST_ENTRY(proc) p_hash`: the tid hash chains.
    pub ProcHash: Proc, p_hash => ListEntry<Proc>
);

/* Status values. */

/// `SIDL`: thread being created by fork.
pub const SIDL: u8 = 1;
/// `SRUN`: currently runnable.
pub const SRUN: u8 = 2;
/// `SSLEEP`: sleeping on an address.
pub const SSLEEP: u8 = 3;
/// `SSTOP`: debugging or suspension.
pub const SSTOP: u8 = 4;
/// `SZOMB`: unused.
pub const SZOMB: u8 = 5;
/// `SDEAD`: thread is almost gone.
pub const SDEAD: u8 = 6;
/// `SONPROC`: thread is currently on a CPU.
pub const SONPROC: u8 = 7;

/// `P_HASSIBLING(p)`.
pub fn p_hassibling(p: &Proc) -> bool {
    p.process().ps_threadcnt.get() > 1
}

/*
 * These flags are per-thread and kept in p_flag
 */

/// `P_INKTR`: in a ktrace op, don't recurse.
pub const P_INKTR: i32 = 0x0000_0001;
/// `P_PROFPEND`: SIGPROF needs to be posted.
pub const P_PROFPEND: i32 = 0x0000_0002;
/// `P_ALRMPEND`: SIGVTALRM needs to be posted.
pub const P_ALRMPEND: i32 = 0x0000_0004;
/// `P_SIGSUSPEND`: need to restore before-suspend mask.
pub const P_SIGSUSPEND: i32 = 0x0000_0008;
/// `P_CANTSLEEP`: insomniac thread.
pub const P_CANTSLEEP: i32 = 0x0000_0010;
/// `P_INSCHED`: switching scheduler state.
pub const P_INSCHED: i32 = 0x0000_0020;
/// `P_SINTR`: sleep is interruptible.
pub const P_SINTR: i32 = 0x0000_0080;
/// `P_SYSTEM`: no sigs, stats or swapping.
pub const P_SYSTEM: i32 = 0x0000_0200;
/// `P_TIMEOUT`: timing out during sleep.
pub const P_TIMEOUT: i32 = 0x0000_0400;
/// `P_TIMEOUTRAN`: timeout handler has finished.
pub const P_TIMEOUTRAN: i32 = 0x0000_0800;
/// `P_TRACESINGLE`: ptrace: keep single threaded.
pub const P_TRACESINGLE: i32 = 0x0000_1000;
/// `P_WEXIT`: working on exiting.
pub const P_WEXIT: i32 = 0x0000_2000;
/// `P_OWEUPC`: owe proc an addupc() at next ast.
pub const P_OWEUPC: i32 = 0x0000_8000;
/// `P_SUSPSINGLE`: need to stop for single threading.
pub const P_SUSPSINGLE: i32 = 0x0008_0000;
/// `P_THREAD`: only a thread, not a real process.
pub const P_THREAD: i32 = 0x0400_0000;
/// `P_SUSPSIG`: stopped from signal.
pub const P_SUSPSIG: i32 = 0x0800_0000;
/// `P_CPUPEG`: do not move to another cpu.
pub const P_CPUPEG: i32 = 0x4000_0000;

/// `P_BITS`: the `%b` format of `p_flag`.
pub const P_BITS: &[u8] = b"\x20\x01INKTR\x02PROFPEND\x03ALRMPEND\x04SIGSUSPEND\x05CANTSLEEP\x06INSCHED\x08SINTR\x0aSYSTEM\x0bTIMEOUT\x0cTIMEOUTRAN\x0dTRACESINGLE\x0eWEXIT\x10OWEUPC\x14SUSPSINGLE\x1bTHREAD\x1cSUSPSIG\x1fCPUPEG";

/// `THREAD_PID_OFFSET`.
pub const THREAD_PID_OFFSET: Pid = 100_000;

/// `struct uidinfo`.
pub struct Uidinfo {
    /// \[U\] `ui_hash`.
    pub ui_hash: ListEntry<Uidinfo>,
    /// \[I\] `ui_uid`.
    pub ui_uid: Cell<Uid>,
    /// \[U\] `ui_proccnt`: proc structs.
    pub ui_proccnt: Cell<i64>,
    /// \[U\] `ui_lockcnt`: lockf structs.
    pub ui_lockcnt: Cell<i64>,
}

// SAFETY: touched under `uidinfolk`.
unsafe impl Sync for Uidinfo {}

impl Uidinfo {
    /// A zero entry.
    pub const fn new() -> Self {
        Self {
            ui_hash: ListEntry::new(),
            ui_uid: Cell::new(0),
            ui_proccnt: Cell::new(0),
            ui_lockcnt: Cell::new(0),
        }
    }
}

impl Default for Uidinfo {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `LIST_ENTRY(uidinfo) ui_hash`: the uid hash chains.
    pub UidinfoHash: Uidinfo, ui_hash => ListEntry<Uidinfo>
);

/*
 * We use process IDs <= PID_MAX; PID_MAX + 1 must also fit in a pid_t,
 * as it is used to represent "no process group".
 * We set PID_MAX to 99999 to keep it in 5 columns in ps
 * When exposed to userspace, thread IDs have THREAD_PID_OFFSET
 * added to keep them from overlapping the PID range.  For them,
 * we use a * a (0 .. 2^n] range for cheapness, picking 'n' such
 * that 2^n + THREAD_PID_OFFSET and THREAD_PID_OFFSET have
 * the same number of columns when printed.
 */

/// `PID_MAX`.
pub const PID_MAX: Pid = 99_999;
/// `TID_MASK`.
pub const TID_MASK: Pid = 0x7ffff;

/// `NO_PID`.
pub const NO_PID: Pid = PID_MAX + 1;

/// `SESSHOLD(s)`: takes a reference on a session.
pub fn sesshold(s: &Session) {
    s.s_count.set(s.s_count.get() + 1);
}

/// `SESSRELE(s)`: drops a reference on a session, which goes back to `session_pool` with
/// the last one (`session0` never loses its own). The caller does not use `s` afterwards.
pub fn sessrele(s: &'static Session) {
    let count = s.s_count.get() - 1;
    s.s_count.set(count);
    if count == 0 {
        timeout_del(&s.s_verauthto);
        pool_put(&SESSION_POOL, ptr::NonNull::from(s).cast());
    }
}

/// `SESS_LEADER(pr)`.
pub fn sess_leader(pr: &Process) -> bool {
    // SAFETY: a process's session outlives its membership.
    unsafe { pr.session().as_ref() }.is_some_and(|s| ptr::eq(s.s_leader.get(), pr))
}

/*
 * Flags to fork1().
 */

/// `FORK_FORK`.
pub const FORK_FORK: i32 = 0x0000_0001;
/// `FORK_VFORK`.
pub const FORK_VFORK: i32 = 0x0000_0002;
/// `FORK_IDLE`.
pub const FORK_IDLE: i32 = 0x0000_0004;
/// `FORK_PPWAIT`.
pub const FORK_PPWAIT: i32 = 0x0000_0008;
/// `FORK_SHAREFILES`.
pub const FORK_SHAREFILES: i32 = 0x0000_0010;
/// `FORK_SYSTEM`.
pub const FORK_SYSTEM: i32 = 0x0000_0020;
/// `FORK_NOZOMBIE`.
pub const FORK_NOZOMBIE: i32 = 0x0000_0040;
/// `FORK_SHAREVM`.
pub const FORK_SHAREVM: i32 = 0x0000_0080;
/// `FORK_PTRACE`.
pub const FORK_PTRACE: i32 = 0x0000_0400;

/// `EXIT_NORMAL`.
pub const EXIT_NORMAL: i32 = 0x0000_0001;
/// `EXIT_THREAD`.
pub const EXIT_THREAD: i32 = 0x0000_0002;
/// `EXIT_THREAD_NOCHECK`.
pub const EXIT_THREAD_NOCHECK: i32 = 0x0000_0003;

/// `SINGLE_SUSPEND`: other threads to stop wherever they are.
pub const SINGLE_SUSPEND: i32 = 0x01;
/// `SINGLE_UNWIND`: other threads to unwind and stop.
pub const SINGLE_UNWIND: i32 = 0x02;
/// `SINGLE_EXIT`: other threads to unwind and then exit.
pub const SINGLE_EXIT: i32 = 0x03;
/// `SINGLE_MASK`.
pub const SINGLE_MASK: i32 = 0x0f;
/// `SINGLE_DEEP`: call is in deep (extra flag for `single_thread_set`).
pub const SINGLE_DEEP: i32 = 0x10;

/// `struct cond`.
pub struct Cond {
    /// \[a\] `c_wait`: initialized and waiting.
    pub c_wait: AtomicU32,
}

impl Cond {
    /// `COND_INITIALIZER()`.
    pub const fn new() -> Self {
        Self {
            c_wait: AtomicU32::new(1),
        }
    }
}

impl Default for Cond {
    fn default() -> Self {
        Self::new()
    }
}

/*
 * functions to handle sets of cpus.
 *
 * For now we keep the cpus in ints so that we can use the generic
 * atomic ops.
 */

/// `CPUSET_ASIZE(x)`.
pub const fn cpuset_asize(x: u32) -> usize {
    ((x as usize) - 1) / 32 + 1
}

/// `CPUSET_SSIZE`.
pub const CPUSET_SSIZE: usize = cpuset_asize(MAXCPUS);

/// `struct cpuset`.
pub struct Cpuset {
    /// `cs_set`.
    pub cs_set: [AtomicU32; CPUSET_SSIZE],
}

impl Cpuset {
    /// An empty set.
    pub const fn new() -> Self {
        Self {
            cs_set: [const { AtomicU32::new(0) }; CPUSET_SSIZE],
        }
    }
}

impl Default for Cpuset {
    fn default() -> Self {
        Self::new()
    }
}

/// `tu_enter(tu)`.
#[inline]
pub fn tu_enter(tu: &Tusage) -> u32 {
    pc_sprod_enter(&tu.tu_pcl)
}

/// `tu_leave(tu, gen)`.
#[inline]
pub fn tu_leave(tu: &Tusage, generation: u32) {
    pc_sprod_leave(&tu.tu_pcl, generation)
}

/// `refreshcreds(p)`: refresh the thread's cache of the process's creds. This is an unlocked
/// access to `ps_ucred`, but the result is benign.
#[inline]
pub fn refreshcreds(p: &Proc) {
    let pr = p.process();

    if !ptr::eq(pr.ps_ucred.get(), p.p_ucred.get()) {
        dorefreshcreds(pr, p);
    }
}

/// A `Timeval`-typed helper the resource code shares: the zero interval.
pub const ZERO_TIMEVAL: Timeval = Timeval::new(0, 0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comm_and_name_stop_at_the_nul() {
        let pr = Process::new();
        pr.set_comm(b"swapper");
        assert_eq!(pr.comm(), b"swapper");
        pr.set_comm(b"a very long command name that does not fit");
        assert_eq!(pr.comm().len(), _MAXCOMLEN - 1);
        let p = Proc::new();
        assert_eq!(p.name(), b"");
        assert_eq!(p.p_stat.get(), 0);
        assert_eq!(cpuset_asize(1), 1);
        assert_eq!(cpuset_asize(33), 2);
    }
}
