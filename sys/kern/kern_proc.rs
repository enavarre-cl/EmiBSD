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

/* <CODE> */
//! The process lists and hash tables: `kern/kern_proc.c`.
//!
//! Upstream: sys/kern/kern_proc.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports the lists, the hash tables and the pools (`procinit`),
//! `uid_find`/`uid_release`/`chgproccnt`, `inferior`, `tfind`, `tfind_user`, `prfind`,
//! `pgfind` and `zombiefind`; the tty layer (M8) brings the process group management
//! (`enternewpgrp`, `enterthispgrp`, `leavepgrp`, `pgdelete`, `zapverauth`, `fixjobc`,
//! `killjobc`, `orphanpg`). `proc_printit` and the `ddb` commands come with the real ddb;
//! `pgrpdump` is `DEBUG`.
//!
//! ## Deviations
//! - `enternewpgrp` takes the raw `pgrp_pool`/`session_pool` items its callers got (the C
//!   passes the uninitialised `pool_get` memory too) and writes a zeroed `Pgrp`/`Session`
//!   into them before filling them; the session's `s_verauth*` start at 0 where the C
//!   leaves the pool's old bytes.
//! - `zapverauth` takes the `void *` of its timeout, as the C; a `&Session` caller casts.
//! - `uidinfolk` is an rwlock (`kern_rwlock.c`, M5-b): `uid_find` reports it; on one CPU
//!   with nothing sleeping the hash is consistent anyway.
//! - The hash tables are slices from `hashinit`; the C's `tidhash`/`pidhash`/`pgrphash`
//!   masks are `len() - 1`.

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use libkern::StaticCell;

use crate::conf::param::{MAXPROCESS, MAXTHREAD};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_sig::{pgsignal, prsignal, sigio_freelist};
use crate::kern::kern_subr::hashinit;
use crate::kern::kern_timeout::timeout_set;
use crate::kern::subr_pool::{pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::tty::ttywait;
use crate::kern::vfs_subr::vrele;
use crate::kern::vfs_vops::VOP_REVOKE;
use crate::machine::intr::{IPL_MPFLOOR, IPL_NONE};
use crate::sys::malloc::{M_NOWAIT, M_PROC, M_WAITOK, M_ZERO};
use crate::sys::pool::{PR_WAITOK, Pool};
use crate::sys::proc::{
    PS_CONTROLT, PS_STOPPED, PS_ZOMBIE, Pgrp, PgrpHash, Proc, ProcHash, ProcList, Process,
    ProcessHash, ProcessList, ProcessPglist, Session, THREAD_PID_OFFSET, Uidinfo, UidinfoHash,
    sess_leader, sessrele,
};
use crate::sys::queue::ListHead;
use crate::sys::resource::Rusage;
use crate::sys::rwlock::Rwlock;
use crate::sys::signal::{SIGCONT, SIGHUP};
use crate::sys::types::{Pid, Uid};
use crate::sys::ucred::Ucred;
use crate::sys::vnode::REVOKEALL;
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

/// `uidinfolk`: the lock over the uid hash table.
static UIDINFOLK: Rwlock = Rwlock::new("uidinfo");

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

    rw_init(&UIDINFOLK, "uidinfo");

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
    rw_enter_write(&UIDINFOLK);
    if let Some(uip) = uipp.iter().find(|u| u.ui_uid.get() == uid) {
        return uip;
    }
    rw_exit_write(&UIDINFOLK);
    let Some(nuip) = malloc(size_of::<Uidinfo>(), M_PROC, M_WAITOK | M_ZERO) else {
        panic(format_args!("uid_find: no memory"));
    };
    let nuip = nuip.cast::<Uidinfo>();
    // SAFETY: a fresh allocation, written once before it is linked.
    unsafe { nuip.as_ptr().write(Uidinfo::new()) };
    // SAFETY: as above; the entry lives forever once linked.
    let nuip: &'static Uidinfo = unsafe { nuip.as_ref() };
    rw_enter_write(&UIDINFOLK);
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
    rw_exit_write(&UIDINFOLK);
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

/// `enternewpgrp`: move process to a new process group. If a session is provided then it's
/// a new session to contain this process group; otherwise the process is staying within its
/// existing session.
///
/// `pgrp` (and `newsess`) are fresh `pgrp_pool` (`session_pool`) items this call takes
/// over.
pub fn enternewpgrp(pr: &Process, pgrp: NonNull<u8>, newsess: Option<NonNull<u8>>) {
    #[cfg(feature = "diagnostic")]
    if sess_leader(pr) {
        panic(format_args!(
            "enternewpgrp: session leader attempted setpgrp"
        ));
    }

    let pgrp = pgrp.cast::<Pgrp>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Pgrp>()` bytes, written once
    // before anything else sees it; it lives until `pgdelete` puts it back.
    let pgrp: &'static Pgrp = unsafe {
        pgrp.as_ptr().write(Pgrp::new());
        pgrp.as_ref()
    };

    if let Some(newsess) = newsess {
        // New session. Initialize it completely
        let newsess = newsess.cast::<Session>();
        // SAFETY: as for the group: a fresh `session_pool` item, freed by `SESSRELE`.
        let newsess: &'static Session = unsafe {
            newsess.as_ptr().write(Session::new());
            newsess.as_ref()
        };
        timeout_set(
            &newsess.s_verauthto,
            zapverauth,
            ptr::from_ref(newsess).cast_mut().cast::<c_void>(),
        );
        newsess.s_leader.set(pr);
        newsess.s_count.set(1);
        newsess.s_ttyvp.set(ptr::null());
        newsess.s_ttyp.set(ptr::null());
        // SAFETY: the old session lives while `pr` is in it; s_login is written by
        // setlogin(2) under the kernel lock, and the two sessions are distinct.
        if let Some(old) = unsafe { pr.session().as_ref() } {
            // SAFETY: as above.
            unsafe { *newsess.s_login.get() = *old.s_login.get() };
        }
        pr.ps_flags.fetch_and(!PS_CONTROLT, Ordering::SeqCst);
        pgrp.pg_session.set(newsess);
        #[cfg(feature = "diagnostic")]
        if !crate::machine::cpu::curproc().is_some_and(|p| ptr::eq(p.process(), pr)) {
            panic(format_args!("enternewpgrp: mksession but not curproc"));
        }
    } else {
        pgrp.pg_session.set(pr.session());
        // SAFETY: as above.
        let sess = unsafe { &*pgrp.pg_session.get() };
        sess.s_count.set(sess.s_count.get() + 1);
    }
    pgrp.pg_id.set(pr.ps_pid.get());
    pgrp.pg_members.init();
    pgrp.pg_sigiolst.init();
    // SAFETY: a new group in no chain yet; the hash is the kernel lock's.
    unsafe { pgrphash(pr.ps_pid.get()).insert_head(pgrp) };
    pgrp.pg_jobc.set(0);

    enterthispgrp(pr, pgrp);
}

/// `enterthispgrp`: move process to an existing process group.
pub fn enterthispgrp(pr: &Process, pgrp: &'static Pgrp) {
    // SAFETY: a process is in a group from `process_new` until `process_zap`.
    let savepgrp: &'static Pgrp = unsafe { &*pr.ps_pgrp.get() };

    // Adjust eligibility of affected pgrps to participate in job control. Increment
    // eligibility counts before decrementing, otherwise we could reach 0 spuriously during
    // the first call.
    fixjobc(pr, pgrp, true);
    fixjobc(pr, savepgrp, false);

    // SAFETY: `pr` is on its old group's member list; it joins the new one's.
    unsafe { ListHead::<ProcessPglist>::remove(pr) };
    mtx_enter(&pr.ps_mtx);
    pr.ps_pgrp.set(pgrp);
    mtx_leave(&pr.ps_mtx);
    // SAFETY: as above.
    unsafe { pgrp.pg_members.insert_head(pr) };
    if savepgrp.pg_members.is_empty() {
        pgdelete(savepgrp);
    }
}

/// `leavepgrp`: remove process from process group.
pub fn leavepgrp(pr: &Process) {
    // SAFETY: as for `enterthispgrp`.
    let savepgrp: &'static Pgrp = unsafe { &*pr.ps_pgrp.get() };

    // SAFETY: the group's session lives while the group does.
    let sess = unsafe { &*savepgrp.pg_session.get() };
    if sess.s_verauthppid.get() == pr.ps_pid.get() {
        zapverauth(ptr::from_ref(sess).cast_mut().cast::<c_void>());
    }
    mtx_enter(&pr.ps_mtx);
    pr.ps_pgrp.set(ptr::null());
    mtx_leave(&pr.ps_mtx);
    // SAFETY: `pr` is on the group's member list.
    unsafe { ListHead::<ProcessPglist>::remove(pr) };
    if savepgrp.pg_members.is_empty() {
        pgdelete(savepgrp);
    }
}

/// `pgdelete`: delete a process group.
pub fn pgdelete(pgrp: &'static Pgrp) {
    sigio_freelist(&pgrp.pg_sigiolst);

    // SAFETY: the group's session lives while the group does.
    let sess: &'static Session = unsafe { &*pgrp.pg_session.get() };
    // SAFETY: a session's terminal is freed only by its driver's detach, after `ttyclose`
    // dropped the session.
    if let Some(tp) = unsafe { sess.s_ttyp.get().as_ref() }
        && ptr::eq(tp.t_pgrp.get(), pgrp)
    {
        tp.t_pgrp.set(ptr::null());
    }
    // SAFETY: the group is on its hash chain since `enternewpgrp`.
    unsafe { ListHead::<PgrpHash>::remove(pgrp) };
    sessrele(sess);
    pool_put(&PGRP_POOL, NonNull::from(pgrp).cast());
}

/// `zapverauth`: forget a session's verified authentication (the `s_verauthto` timeout).
pub fn zapverauth(v: *mut c_void) {
    // SAFETY: the argument is a live session: the timeout's (set by `enternewpgrp`, deleted
    // by `SESSRELE` before the session is freed) or a caller's.
    let sess = unsafe { &*v.cast::<Session>() };
    sess.s_verauthuid.set(0);
    sess.s_verauthppid.set(0);
}

/// `fixjobc`: adjust pgrp jobc counters when specified process changes process group.
///
/// We count the number of processes in each process group that "qualify" the group for
/// terminal job control (those with a parent in a different process group of the same
/// session). If that count reaches zero, the process group becomes orphaned. Check both the
/// specified process' process group and that of its children. `entering` false: `pr` is
/// leaving specified group; true: `pr` is entering specified group. XXX need proctree lock
pub fn fixjobc(pr: &Process, pgrp: &Pgrp, entering: bool) {
    let mysession = pgrp.pg_session.get();

    // Check pr's parent to see whether pr qualifies its own process group; if so, adjust
    // count for pr's process group.
    // SAFETY: a parent outlives its children's group changes; process 0 has none.
    let parent = unsafe { pr.ps_pptr.get().as_ref() };
    // SAFETY: a live process is in a live group.
    let hispgrp = parent.and_then(|pp| unsafe { pp.ps_pgrp.get().as_ref() });
    if let Some(hispgrp) = hispgrp
        && !ptr::eq(hispgrp, pgrp)
        && ptr::eq(hispgrp.pg_session.get(), mysession)
    {
        if entering {
            pgrp.pg_jobc.set(pgrp.pg_jobc.get() + 1);
        } else {
            pgrp.pg_jobc.set(pgrp.pg_jobc.get() - 1);
            if pgrp.pg_jobc.get() == 0 {
                orphanpg(pgrp);
            }
        }
    }

    // Check this process' children to see whether they qualify their process groups; if
    // so, adjust counts for children's process groups.
    for child in pr.ps_children.iter() {
        // SAFETY: as above.
        let Some(hispgrp) = (unsafe { child.ps_pgrp.get().as_ref() }) else {
            continue;
        };
        if !ptr::eq(hispgrp, pgrp)
            && ptr::eq(hispgrp.pg_session.get(), mysession)
            && child.ps_flags.load(Ordering::Relaxed) & PS_ZOMBIE == 0
        {
            if entering {
                hispgrp.pg_jobc.set(hispgrp.pg_jobc.get() + 1);
            } else {
                hispgrp.pg_jobc.set(hispgrp.pg_jobc.get() - 1);
                if hispgrp.pg_jobc.get() == 0 {
                    orphanpg(hispgrp);
                }
            }
        }
    }
}

/// `killjobc`: a process exits: as a controlling process, hang up its terminal's
/// foreground group and revoke the terminal; then leave job control.
pub fn killjobc(pr: &Process) {
    if sess_leader(pr) {
        // SAFETY: the leader's session lives while the leader is in it.
        let sp = unsafe { &*pr.session() };

        if !sp.s_ttyvp.get().is_null() {
            // Controlling process. Signal foreground pgrp, drain controlling terminal and
            // revoke access to controlling terminal.
            // SAFETY: a session's terminal stays allocated while the session refers to it
            // (see `pgdelete`).
            if let Some(tp) = unsafe { sp.s_ttyp.get().as_ref() }
                && ptr::eq(tp.t_session.get(), sp)
            {
                if let Some(pg) = tp.pgrp() {
                    pgsignal(Some(pg), SIGHUP, true);
                }
                let _ = ttywait(tp);
                // The tty could have been revoked if we blocked.
                // SAFETY: `s_ttyvp` holds a use count on its vnode (never freed).
                if let Some(vp) = unsafe { sp.s_ttyvp.get().as_ref() } {
                    let _ = VOP_REVOKE(vp, REVOKEALL);
                }
            }
            let ovp = sp.s_ttyvp.get();
            sp.s_ttyvp.set(ptr::null());
            // SAFETY: as above.
            if let Some(ovp) = unsafe { ovp.as_ref() } {
                vrele(ovp);
            }
            // s_ttyp is not zero'd; we use this to indicate that the session once had a
            // controlling terminal. (for logging and informational purposes)
        }
        sp.s_leader.set(ptr::null());
    }
    // SAFETY: an exiting process is still in its group (`leavepgrp` comes in process_zap).
    let pgrp = unsafe { &*pr.ps_pgrp.get() };
    fixjobc(pr, pgrp, false);
}

/// `orphanpg`: a process group has become orphaned; if there are any stopped processes in
/// the group, hang-up all process in that group.
fn orphanpg(pg: &Pgrp) {
    if pg
        .pg_members
        .iter()
        .any(|pr| pr.ps_flags.load(Ordering::Relaxed) & PS_STOPPED != 0)
    {
        for pr in pg.pg_members.iter() {
            prsignal(pr, SIGHUP);
            prsignal(pr, SIGCONT);
        }
    }
}

// proc_printit, db_kill_cmd, db_stop_cmd, db_show_all_procs: the real ddb. pgrpdump: DEBUG.
/* </CODE> */
