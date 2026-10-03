/*	$OpenBSD: kern_proc.c,v 1.103 2025/09/25 08:46:50 mvs Exp $	*/
/*	$NetBSD: kern_proc.c,v 1.14 1996/02/09 18:59:41 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)kern_proc.c	8.4 (Berkeley) 1/4/94
 */
/* </LICENSES> */

//! The process lists and hash tables: `kern/kern_proc.c`.
//!
//! Upstream: sys/kern/kern_proc.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports the lists, the hash tables and the pools (`procinit`),
//! `uid_find`/`uid_release`/`chgproccnt`, `inferior`, `tfind`, `tfind_user`, `prfind`,
//! `pgfind` and `zombiefind`. The process group management (`enternewpgrp`, `enterthispgrp`,
//! `leavepgrp`, `pgdelete`, `zapverauth`, `fixjobc`, `killjobc`, `orphanpg`) needs signals,
//! ttys and `sigio` (M6); `proc_printit` and the `ddb` commands come with the real ddb (M7).
//!
//! ## Deviations
//! - `uidinfolk` is an rwlock (`kern_rwlock.c`, M5-b): `uid_find` reports it; on one CPU
//!   with nothing sleeping the hash is consistent anyway.
//! - The hash tables are slices from `hashinit`; the C's `tidhash`/`pidhash`/`pgrphash`
//!   masks are `len() - 1`.

use core::ptr;

use libkern::StaticCell;

use crate::conf::param::{MAXPROCESS, MAXTHREAD};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_subr::hashinit;
use crate::kern::subr_pool::pool_init;
use crate::kern::subr_prf::panic;
use crate::machine::intr::{IPL_MPFLOOR, IPL_NONE};
use crate::sys::malloc::{M_NOWAIT, M_PROC, M_WAITOK, M_ZERO};
use crate::sys::pool::{PR_WAITOK, Pool};
use crate::sys::proc::{
    Pgrp, PgrpHash, Proc, ProcHash, ProcList, Process, ProcessHash, ProcessList, Session,
    THREAD_PID_OFFSET, Uidinfo, UidinfoHash,
};
use crate::sys::queue::ListHead;
use crate::sys::resource::Rusage;
use crate::sys::types::{Pid, Uid};
use crate::sys::ucred::Ucred;
use crate::unported;
use core::sync::atomic::Ordering;

/*
 *  Locks used to protect struct members in this file:
 *	I	immutable after creation
 *	U	uidinfolk
 */

/// A hash table from `hashinit`, set once by `procinit`.
struct HashTable<A: crate::sys::queue::ListAdapter + 'static>(
    StaticCell<Option<&'static [ListHead<A>]>>,
);

// SAFETY: written once by `procinit` on the boot CPU; the lists inside are locked as the
// C's annotations say.
unsafe impl<A: crate::sys::queue::ListAdapter + 'static> Sync for HashTable<A> {}

impl<A: crate::sys::queue::ListAdapter + 'static> HashTable<A> {
    const fn new() -> Self {
        Self(StaticCell::new(None))
    }

    /// The chain for `key` (`&tbl[key & mask]`).
    fn chain(&self, key: i64) -> &'static ListHead<A> {
        // SAFETY: set once by `procinit` before any lookup.
        let Some(tbl) = (unsafe { self.0.read() }) else {
            panic(format_args!("procinit: hash table used before procinit"));
        };
        &tbl[(key as usize) & (tbl.len() - 1)]
    }
}

/// `uidinfolk`: an rwlock (see the module's deviations).
fn uidinfolk_unported() {
    let _ = unported!("uidinfolk (rw_enter_write, kern_rwlock.c)");
}

/// \[U\] `uihashtbl`.
static UIHASHTBL: HashTable<UidinfoHash> = HashTable::new();

/// `tidhashtbl`.
static TIDHASHTBL: HashTable<ProcHash> = HashTable::new();
/// `pidhashtbl`.
static PIDHASHTBL: HashTable<ProcessHash> = HashTable::new();
/// `pgrphashtbl`.
static PGRPHASHTBL: HashTable<PgrpHash> = HashTable::new();

/// A list head made `Sync`: the process lists are touched under the kernel lock.
pub struct ProcessListHead(pub ListHead<ProcessList>);
// SAFETY: see the type's doc.
unsafe impl Sync for ProcessListHead {}

/// `allproc`'s head, made `Sync`.
pub struct ProcListHead(pub ListHead<ProcList>);
// SAFETY: see `ProcessListHead`.
unsafe impl Sync for ProcListHead {}

/// `allprocess`: list of all processes.
pub static ALLPROCESS: ProcessListHead = ProcessListHead(ListHead::new());
/// `zombprocess`: list of zombie processes.
pub static ZOMBPROCESS: ProcessListHead = ProcessListHead(ListHead::new());
/// `allproc`: list of all threads.
pub static ALLPROC: ProcListHead = ProcListHead(ListHead::new());

/// `proc_pool`.
pub static PROC_POOL: Pool = Pool::new();
/// `process_pool`.
pub static PROCESS_POOL: Pool = Pool::new();
/// `rusage_pool`.
pub static RUSAGE_POOL: Pool = Pool::new();
/// `ucred_pool`.
pub static UCRED_POOL: Pool = Pool::new();
/// `pgrp_pool`.
pub static PGRP_POOL: Pool = Pool::new();
/// `session_pool`.
pub static SESSION_POOL: Pool = Pool::new();

/// `TIDHASH(tid)`.
pub fn tidhash(tid: Pid) -> &'static ListHead<ProcHash> {
    TIDHASHTBL.chain(i64::from(tid))
}

/// `PIDHASH(pid)`.
pub fn pidhash(pid: Pid) -> &'static ListHead<ProcessHash> {
    PIDHASHTBL.chain(i64::from(pid))
}

/// `PGRPHASH(pgid)`.
pub fn pgrphash(pgid: Pid) -> &'static ListHead<PgrpHash> {
    PGRPHASHTBL.chain(i64::from(pgid))
}

/// `UIHASH(uid)`.
fn uihash(uid: Uid) -> &'static ListHead<UidinfoHash> {
    UIHASHTBL.chain(i64::from(uid))
}

/// `procinit`: initialize global process hashing structures.
pub fn procinit() {
    ALLPROCESS.0.init();
    ZOMBPROCESS.0.init();
    ALLPROC.0.init();

    // rw_init(&uidinfolk, "uidinfo"): see the module's deviations.

    let maxthread = MAXTHREAD.load(Ordering::Relaxed);
    let maxprocess = MAXPROCESS.load(Ordering::Relaxed);
    let tid = hashinit::<ProcHash>(maxthread / 4, M_PROC, M_NOWAIT);
    let pid = hashinit::<ProcessHash>(maxprocess / 4, M_PROC, M_NOWAIT);
    let pgrp = hashinit::<PgrpHash>(maxprocess / 4, M_PROC, M_NOWAIT);
    let ui = hashinit::<UidinfoHash>(maxprocess / 16, M_PROC, M_NOWAIT);
    let (Some(tid), Some(pid), Some(pgrp), Some(ui)) = (tid, pid, pgrp, ui) else {
        panic(format_args!("procinit: malloc"));
    };
    // SAFETY: once, on the boot CPU, before any lookup.
    unsafe {
        TIDHASHTBL.0.write(Some(tid));
        PIDHASHTBL.0.write(Some(pid));
        PGRPHASHTBL.0.write(Some(pgrp));
        UIHASHTBL.0.write(Some(ui));
    }

    pool_init(
        &PROC_POOL,
        size_of::<Proc>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "procpl",
        None,
    );
    pool_init(
        &PROCESS_POOL,
        size_of::<Process>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "processpl",
        None,
    );
    pool_init(
        &RUSAGE_POOL,
        size_of::<Rusage>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "zombiepl",
        None,
    );
    pool_init(
        &UCRED_POOL,
        size_of::<Ucred>(),
        0,
        IPL_MPFLOOR,
        0,
        "ucredpl",
        None,
    );
    pool_init(
        &PGRP_POOL,
        size_of::<Pgrp>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "pgrppl",
        None,
    );
    pool_init(
        &SESSION_POOL,
        size_of::<Session>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "sessionpl",
        None,
    );
}

/// `uid_find`: this returns with `uidinfolk` held: caller must call `uid_release()` after
/// making whatever change they needed.
pub fn uid_find(uid: Uid) -> &'static Uidinfo {
    let uipp = uihash(uid);
    uidinfolk_unported(); // rw_enter_write(&uidinfolk)
    if let Some(uip) = uipp.iter().find(|u| u.ui_uid.get() == uid) {
        return uip;
    }
    // rw_exit_write(&uidinfolk)
    let Some(nuip) = malloc(size_of::<Uidinfo>(), M_PROC, M_WAITOK | M_ZERO) else {
        panic(format_args!("uid_find: no memory"));
    };
    let nuip = nuip.cast::<Uidinfo>();
    // SAFETY: a fresh allocation, written once before it is linked.
    unsafe { nuip.as_ptr().write(Uidinfo::new()) };
    // SAFETY: as above; the entry lives forever once linked.
    let nuip: &'static Uidinfo = unsafe { nuip.as_ref() };
    // rw_enter_write(&uidinfolk)
    if let Some(uip) = uipp.iter().find(|u| u.ui_uid.get() == uid) {
        // `nuip` was allocated above and is in no list.
        free(
            ptr::NonNull::from(nuip).cast::<u8>(),
            M_PROC,
            size_of::<Uidinfo>(),
        );
        return uip;
    }
    nuip.ui_uid.set(uid);
    // SAFETY: `nuip` is in no list and lives forever.
    unsafe { uipp.insert_head(nuip) };

    nuip
}

/// `uid_release`.
pub fn uid_release(_uip: &Uidinfo) {
    // rw_exit_write(&uidinfolk)
}

/// `chgproccnt`: change the count associated with number of threads a given user is using.
pub fn chgproccnt(uid: Uid, diff: i64) -> i64 {
    let uip = uid_find(uid);
    let count = uip.ui_proccnt.get() + diff;
    uip.ui_proccnt.set(count);
    uid_release(uip);
    if count < 0 {
        panic(format_args!("chgproccnt: procs < 0"));
    }
    count
}

/// `inferior`: is `pr` an inferior of `parent`?
pub fn inferior(pr: &Process, parent: &Process) -> bool {
    let mut pr: *const Process = pr;
    while !ptr::eq(pr, parent) {
        // SAFETY: the parent chain ends at process 0 or 1, which live forever; every process
        // on it is alive while its children are.
        let p = unsafe { &*pr };
        if p.ps_pid.get() == 0 || p.ps_pid.get() == 1 {
            return false;
        }
        pr = p.ps_pptr.get();
    }
    true
}

/// `tfind`: locate a proc (thread) by number.
pub fn tfind(tid: Pid) -> Option<&'static Proc> {
    tidhash(tid).iter().find(|p| p.p_tid.get() == tid)
}

/// `tfind_user`: locate a thread by userspace id, from a given process.
pub fn tfind_user(tid: Pid, pr: &Process) -> Option<&'static Proc> {
    if tid < THREAD_PID_OFFSET {
        return None;
    }
    let p = tfind(tid - THREAD_PID_OFFSET)?;

    // verify we found a thread in the correct process
    if !ptr::eq(p.p_p.get(), pr) {
        return None;
    }
    Some(p)
}

/// `prfind`: locate a process by number.
pub fn prfind(pid: Pid) -> Option<&'static Process> {
    pidhash(pid).iter().find(|pr| pr.ps_pid.get() == pid)
}

/// `pgfind`: locate a process group by number.
pub fn pgfind(pgid: Pid) -> Option<&'static Pgrp> {
    pgrphash(pgid).iter().find(|pg| pg.pg_id.get() == pgid)
}

/// `zombiefind`: locate a zombie process.
pub fn zombiefind(pid: Pid) -> Option<&'static Process> {
    ZOMBPROCESS.0.iter().find(|pr| pr.ps_pid.get() == pid)
}

// enternewpgrp, enterthispgrp, leavepgrp, pgdelete, zapverauth, fixjobc, killjobc,
// orphanpg: signals, ttys and sigio (M6). proc_printit, db_kill_cmd, db_stop_cmd,
// db_show_all_procs: the real ddb (M7). pgrpdump: DEBUG.
