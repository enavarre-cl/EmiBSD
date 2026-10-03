/*	$OpenBSD: uipc_socket.c,v 1.389 2026/06/11 19:21:51 bluhm Exp $	*/
/*	$NetBSD: uipc_socket.c,v 1.21 1996/02/04 02:17:52 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1988, 1990, 1993
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
 *	@(#)uipc_socket.c	8.3 (Berkeley) 4/15/94
 */
/* </LICENSES> */

//! Socket operation routines: `kern/uipc_socket.c`.
//!
//! Upstream: sys/kern/uipc_socket.c @ 3ce1f3f79392
//!
//! These routines are called by the routines in `sys_socket.c` or from a system process,
//! and implement the semantics of socket operations by switching out to the protocol
//! specific routines (`pr_usrreqs`). `socreate` allocates a socket from `socket_pool` and
//! attaches it to its protocol; `soclose` and `sofree` take it apart again, the last
//! reference (`sorele`) giving it back to the pool. `sosend` copies user data into mbufs and
//! hands them to the protocol; `soreceive` takes records off the receive buffer (address,
//! control, data) and copies them out. Socket splicing (`SOCKET_SPLICE`, configured in
//! GENERIC) moves data from one socket's receive buffer to another's send buffer in the
//! kernel: `sosplice`, `somove`, `sounsplice`.
//!
//! ## Deviations
//! - `struct socket **aso` and the other out-parameters are return values: `socreate`
//!   returns the socket, `soalloc` an `Option`, `m_getuio` the mbuf chain, `somove` a
//!   `bool` (the C's "1 continue").
//! - `pool_get(PR_WAITOK)` and `m_get(M_WAIT)` can fail here (the pool cannot sleep for
//!   memory yet, see `subr_pool.rs`), where the C never does: `socreate`, `m_getuio`,
//!   `soreceive`'s out-of-band mbuf and `sosplice` answer `ENOBUFS`.
//! - `soreceive`'s `struct mbuf **` arguments (`paddr`, `mp0`, `controlp`) are
//!   `Option<&mut Option<&'static Mbuf>>`; the C's walking `mp = &m->m_next` is the private
//!   `MbufTail`, which remembers the last mbuf of the chain being built.
//! - `struct filterops soread_filtops`, `sowrite_filtops` and `soexcept_filtops` wait for
//!   `<sys/event.h>`'s kernel half (`kern_event.c`, not ported): `soo_kqfilter` reports
//!   itself. The filters are ported over [`SoKnote`], the knote fields they read and write,
//!   with the socket as an argument (the C reads `kn->kn_fp->f_data`); `filt_sordetach`,
//!   `filt_sowdetach` and the `*modify`/`*process` routines report `klist_remove`,
//!   `knote_modify` and `knote_process`. `soalloc`'s `klist_init_mutex` and `sorele`'s
//!   `klist_free` are comments; `sohasoutofband`'s `knote` is reported.
//! - `somaxconn`, `sominconn` keep their C names (lowercase statics), beside the
//!   `SOMAXCONN` constant of `<sys/socket.h>`.
//! - `WITNESS` is not configured (`soalloc`'s `inet46` lock name); `DIAGNOSTIC`'s panics are
//!   under feature `diagnostic`.
//! - `sosplice` compares the protocols' `pru_send` with `ptr::fn_addr_eq`, as the C compares
//!   the function pointers.
//! - The `DDB` printers `sobuf_print` and `so_print` take a `&Socket`/`&Sockbuf` and the
//!   `db_printf`-like `PrFn`.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kern::kern_lock::{mtx_enter, mtx_init_flags, mtx_leave};
use crate::kern::kern_prot::suser;
use crate::kern::kern_rwlock::{rw_init, rw_init_flags_trace};
use crate::kern::kern_sig::{pgsigio, sigio_free};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::{refcnt_init_trace, refcnt_rele};
use crate::kern::kern_task::{task_add, task_del, task_set, taskq_barrier};
use crate::kern::kern_timeout::{
    timeout_add_nsec, timeout_barrier, timeout_del, timeout_set_flags,
};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_domain::{pffindproto, pffindtype};
use crate::kern::uipc_mbuf::{
    MAX_HDR, m_adj, m_align, m_copym, m_free, m_freem, m_get, m_gethdr, m_purge, m_resethdr,
    m_split,
};
use crate::kern::uipc_socket2::{
    sbcheckreserve, sbdroprecord, sblock, sbrelease, sbreserve, sbunlock, sbwait, soassertlocked,
    soassertlocked_readonly, socantrcvmore, solock, solock_nonet, solock_pair, solock_persocket,
    solock_shared, soqremque, sosleep_nsec, sounlock, sounlock_nonet, sounlock_pair,
    sounlock_shared, sowakeup,
};
use crate::kern::uipc_syscalls::getsock;
use crate::machine::cpu::curproc;
use crate::machine::db_machdep::PrFn;
use crate::machine::intr::{IPL_MPFLOOR, IPL_SOFTNET};
use crate::net::if_::net_tq;
use crate::sys::errno::Errno;
use crate::sys::event::{__EV_HUP, __EV_POLL, __EV_SELECT, EV_EOF, Knote, NOTE_LOWAT, NOTE_OOB};
use crate::sys::file::{File, frele};
use crate::sys::limits::SHRT_MAX;
use crate::sys::mbuf::{
    M_BCAST, M_DONTWAIT, M_EOR, M_EXT, M_LOOP, M_MAXLOOP, M_MCAST, M_PKTHDR, M_WAIT, M_ZEROIZE,
    MAXMCLBYTES, MCLBYTES, MHLEN, MINCLSIZE, MLEN, MT_CONTROL, MT_DATA, MT_HEADER, MT_OOBDATA,
    MT_SONAME, Mbuf, mclgetl, mtod,
};
use crate::sys::param::{PCATCH, PSOCK};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::protosw::{
    PR_ABRTACPTDIS, PR_ADDR, PR_ATOMIC, PR_CONNREQUIRED, PR_RIGHTS, PR_SPLICE, PR_WANTRCVD,
    PRCO_GETOPT, PRCO_SETOPT, Protosw, pru_abort, pru_accept, pru_attach, pru_bind, pru_connect,
    pru_connect2, pru_detach, pru_disconnect, pru_flowid, pru_listen, pru_rcvd, pru_rcvoob,
    pru_send, pru_sendoob, pru_shutdown,
};
use crate::sys::refcnt::DT_REFCNT_IDX_SOCKET;
use crate::sys::rwlock::{DT_RWLOCK_IDX_SOLOCK, RWL_DUPOK};
use crate::sys::signal::SIGURG;
use crate::sys::socket::{
    AF_UNIX, Cmsghdr, Linger, MSG_BCAST, MSG_DONTWAIT, MSG_EOR, MSG_MCAST, MSG_OOB, MSG_PEEK,
    MSG_TRUNC, MSG_WAITALL, SCM_RIGHTS, SHUT_RD, SHUT_RDWR, SHUT_WR, SO_ACCEPTCONN, SO_BINDANY,
    SO_BROADCAST, SO_DEBUG, SO_DOMAIN, SO_DONTROUTE, SO_ERROR, SO_KEEPALIVE, SO_LINGER,
    SO_OOBINLINE, SO_PEERCRED, SO_PROTOCOL, SO_RCVBUF, SO_RCVLOWAT, SO_RCVTIMEO, SO_REUSEADDR,
    SO_REUSEPORT, SO_RTABLE, SO_SNDBUF, SO_SNDLOWAT, SO_SNDTIMEO, SO_SPLICE, SO_TIMESTAMP, SO_TYPE,
    SO_USELOOPBACK, SO_ZEROIZE, SOCK_SEQPACKET, SOCK_STREAM, SOL_SOCKET, SOMAXCONN, Sockpeercred,
    Splice, cmsg_align, cmsg_space,
};
use crate::sys::socketvar::{
    SB_SPLICE, SBL_NOINTR, SBL_WAIT, SS_CANTRCVMORE, SS_CANTSENDMORE, SS_ISCONNECTED,
    SS_ISCONNECTING, SS_ISDISCONNECTED, SS_ISDISCONNECTING, SS_ISSENDING, SS_NOFDREF, SS_PRIV,
    SS_RCVATMARK, Sockbuf, Socket, SoqHead, Sosplice, isspliced, issplicedback, sb_empty_fixup,
    sbassertlocked, sbfree, sbspace_locked, soreadable, soref, sosendallatonce, sowriteable,
};

use crate::sys::systm::INFSLP;
use crate::sys::time::{Timeval, nsec_to_timeval, sec_to_nsec, timeval_to_nsec};
use crate::sys::timeout::{KCLOCK_NONE, TIMEOUT_MPSAFE, TIMEOUT_PROC};
use crate::sys::types::{Off, Socklen};
use crate::sys::uio::Uio;
use crate::sys::unpcb::{Fdpass, UNP_FEIDS, sotounpcb};
use crate::{kassert, unported};

/// `SOMINCONN`: the smallest backlog `listen(2)` grants.
pub const SOMINCONN: i32 = 80;

/// `SOSP_FREEING_READ`: `sounsplice` must not wake the source, it is being freed.
const SOSP_FREEING_READ: i32 = 1;
/// `SOSP_FREEING_WRITE`: `sounsplice` must not wake the drain, it is being freed.
const SOSP_FREEING_WRITE: i32 = 2;

/// The knote fields the socket filters read and write, which `kern_event.c` will hand them
/// through `struct knote` (see the module's deviations). Not a C type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SoKnote {
    /// `kn_flags` (`EV_*`, `__EV_*`).
    pub kn_flags: u16,
    /// `kn_sfflags`: the `NOTE_*` filter flags the user asked for.
    pub kn_sfflags: u32,
    /// `kn_sdata`: the user's data (`NOTE_LOWAT`'s mark).
    pub kn_sdata: i64,
    /// `kn_fflags`: the filter flags returned.
    pub kn_fflags: u32,
    /// `kn_data`: the data returned.
    pub kn_data: i64,
}

/// An `struct mbuf **mp` the C walks down a chain it builds (`*mp = m; mp = &m->m_next`):
/// the chain's head and its last mbuf so far.
struct MbufTail<'a> {
    /// What the caller's `struct mbuf **` points at.
    head: &'a mut Option<&'static Mbuf>,
    /// The mbuf whose `m_next` the pointer now designates (`None`: the head itself).
    last: Option<&'static Mbuf>,
}

impl<'a> MbufTail<'a> {
    /// `mp = mp0`.
    fn new(head: &'a mut Option<&'static Mbuf>) -> Self {
        Self { head, last: None }
    }

    /// `*mp`.
    fn get(&self) -> Option<&'static Mbuf> {
        match self.last {
            Some(last) => last.m_next().get(),
            None => *self.head,
        }
    }

    /// `*mp = m`.
    fn set(&mut self, m: Option<&'static Mbuf>) {
        match self.last {
            Some(last) => last.m_next().set(m),
            None => *self.head = m,
        }
    }

    /// `mp = &(*mp)->m_next`; nothing when `*mp` is NULL.
    fn advance(&mut self) {
        if let Some(cur) = self.get() {
            self.last = Some(cur);
        }
    }
}

/// \[a\] `somaxconn`: the largest backlog `listen(2)` grants (`kern.somaxconn`).
#[allow(non_upper_case_globals)] // the C's name; SOMAXCONN is the constant
pub static somaxconn: AtomicI32 = AtomicI32::new(SOMAXCONN);
/// \[a\] `sominconn`: the smallest (`kern.sominconn`).
#[allow(non_upper_case_globals)] // the C's name; SOMINCONN is the constant
pub static sominconn: AtomicI32 = AtomicI32::new(SOMINCONN);

/// `socket_pool`.
pub static SOCKET_POOL: Pool = Pool::new();
/// `sosplice_pool`.
pub static SOSPLICE_POOL: Pool = Pool::new();

/// A queued socket: the first of `q`. Queued sockets are pool items that stay allocated
/// while they are queued.
fn soq_first(q: &SoqHead) -> Option<&'static Socket> {
    // SAFETY: a socket on a connection queue is a live `socket_pool` item; `soqremque`
    // takes it off before it can be freed. The caller holds the listening socket's lock.
    q.first().map(|so| unsafe { &*ptr::from_ref(so) })
}

/// `so->so_sp`, which a spliced (or once spliced) socket has.
fn so_sp(so: &Socket) -> &'static Sosplice {
    match so.so_sp.get() {
        Some(sp) => sp,
        None => panic(format_args!("socket {:p}: no so_sp", so)),
    }
}

/// `curproc`, which the socket calls that the C runs as `curproc` always have.
fn curproc_or_panic(func: &str) -> &'static crate::sys::proc::Proc {
    match curproc() {
        Some(p) => p,
        None => panic(format_args!("{}: no curproc", func)),
    }
}

/// The `len` bytes of `m`'s data from `off` on.
///
/// # Safety
///
/// `off + len` is at most `m->m_len`, and nothing else touches those bytes while the slice
/// lives (the caller holds the buffer's `sblock`, or owns the mbuf).
unsafe fn mdata<'a>(m: &Mbuf, off: usize, len: usize) -> &'a mut [u8] {
    // SAFETY: the caller's contract: the range lies in the mbuf's data.
    unsafe { slice::from_raw_parts_mut(mtod::<u8>(m).add(off), len) }
}

/// `*mtod(m, int *)`; the caller checked that `m` holds an `int`.
fn mtod_int(m: &Mbuf) -> i32 {
    // SAFETY: `m_len >= sizeof(int)` (the callers check it); the data may be unaligned.
    unsafe { mtod::<i32>(m).read_unaligned() }
}

/// `*mtod(m, int *) = v`; `m` holds at least an `int` of storage.
fn set_mtod_int(m: &Mbuf, v: i32) {
    // SAFETY: an mbuf's storage holds `MLEN` bytes at least; the data may be unaligned.
    unsafe { mtod::<i32>(m).write_unaligned(v) };
}

/// The `cmsg_type` of the control message at the start of `m`, 0 if `m` is too short.
fn cmsg_type_of(m: &Mbuf) -> i32 {
    if (m.m_len().get() as usize) < size_of::<Cmsghdr>() {
        return 0;
    }
    // SAFETY: `m` holds a whole header (checked above); it may be unaligned.
    unsafe { mtod::<Cmsghdr>(m).read_unaligned().cmsg_type }
}

/// `soinit`: the socket and splice pools.
pub fn soinit() {
    pool_init(
        &SOCKET_POOL,
        size_of::<Socket>(),
        0,
        IPL_SOFTNET,
        0,
        "sockpl",
        None,
    );
    pool_init(
        &SOSPLICE_POOL,
        size_of::<Sosplice>(),
        0,
        IPL_SOFTNET,
        0,
        "sosppl",
        None,
    );
}

/// `soalloc(prp, wait)`: a zeroed socket of protocol `prp` with its locks and lists
/// initialised and one reference, or `None` when the pool is empty.
pub fn soalloc(prp: &'static Protosw, wait: i32) -> Option<&'static Socket> {
    let dp = prp.pr_domain;
    let dom_name = core::str::from_utf8(dp.dom_name).unwrap_or("socket");

    let mem = pool_get(
        &SOCKET_POOL,
        (if wait == M_WAIT { PR_WAITOK } else { PR_NOWAIT }) | PR_ZERO,
    )?;
    let raw = mem.cast::<Socket>().as_ptr();
    // SAFETY: a fresh, suitably aligned `socket_pool` item of `size_of::<Socket>()` bytes,
    // written once before anything else sees it.
    unsafe { raw.write(Socket::new(prp)) };
    // SAFETY: as above; the item stays allocated until `sorele` (or `sonewconn`'s failure
    // path) gives it back.
    let so: &'static Socket = unsafe { &*raw };

    // WITNESS: not configured.

    refcnt_init_trace(&so.so_refcnt, DT_REFCNT_IDX_SOCKET);
    rw_init_flags_trace(&so.so_lock, dom_name, RWL_DUPOK, DT_RWLOCK_IDX_SOLOCK);
    rw_init(&so.so_rcv.sb_lock, "sbufrcv");
    rw_init(&so.so_snd.sb_lock, "sbufsnd");
    mtx_init_flags(&so.so_rcv.sb_mtx, IPL_MPFLOOR, Some("sbrcv"), 0);
    mtx_init_flags(&so.so_snd.sb_mtx, IPL_MPFLOOR, Some("sbsnd"), 0);
    // klist_init_mutex(&so->so_rcv.sb_klist, ...), the same for so_snd: kern_event.c.
    crate::sys::sigio::sigio_init(&so.so_sigio);
    so.so_q0.init();
    so.so_q.init();

    Some(so)
}

/// `socreate(dom, aso, type, proto)`: a new socket of domain `dom`, type `type_` and
/// protocol `proto` (0: the domain's first of that type), attached to its protocol.
pub fn socreate(dom: i32, type_: i32, proto: i32) -> Result<&'static Socket, Errno> {
    let p = curproc_or_panic("socreate"); // XXX

    let prp = if proto != 0 {
        pffindproto(dom, proto, type_)
    } else {
        pffindtype(dom, type_)
    };
    let Some(prp) = prp.filter(|prp| prp.pr_usrreqs.is_some()) else {
        return Err(Errno::EPROTONOSUPPORT);
    };
    if i32::from(prp.pr_type) != type_ {
        return Err(Errno::EPROTOTYPE);
    }
    let Some(so) = soalloc(prp, M_WAIT) else {
        return Err(Errno::ENOBUFS);
    };
    so.so_type.set(type_);
    if suser(p).is_ok() {
        so.so_state.set(SS_PRIV);
    }
    let cred = p.ucred();
    so.so_ruid.set(cred.cr_ruid.get());
    so.so_euid.set(cred.cr_uid.get());
    so.so_rgid.set(cred.cr_rgid.get());
    so.so_egid.set(cred.cr_gid.get());
    so.so_cpid.set(p.process().ps_pid.get());
    so.so_snd.sb_timeo_nsecs.set(INFSLP);
    so.so_rcv.sb_timeo_nsecs.set(INFSLP);

    solock_shared(so);
    if let Err(error) = pru_attach(so, proto, M_WAIT) {
        so.set_state(SS_NOFDREF);
        // sofree() calls sounlock().
        sofree(so, false);
        return Err(error);
    }
    sounlock_shared(so);
    Ok(so)
}

/// `sobind(so, nam, p)`: binds `so` to the address in `nam`.
pub fn sobind(
    so: &'static Socket,
    nam: &'static Mbuf,
    p: &crate::sys::proc::Proc,
) -> Result<(), Errno> {
    soassertlocked(so);
    pru_bind(so, nam, p)
}

/// `solisten(so, backlog)`: makes `so` accept connections, at most `backlog` of them
/// queued (clamped to `[sominconn, somaxconn]`).
pub fn solisten(so: &'static Socket, backlog: i32) -> Result<(), Errno> {
    let somaxconn_local = somaxconn.load(Ordering::Relaxed);
    let sominconn_local = sominconn.load(Ordering::Relaxed);
    let mut backlog = backlog;

    match so.so_type.get() {
        SOCK_STREAM | SOCK_SEQPACKET => {}
        _ => return Err(Errno::EOPNOTSUPP),
    }

    soassertlocked(so);

    if so.has_state(SS_ISCONNECTED | SS_ISCONNECTING | SS_ISDISCONNECTING) {
        return Err(Errno::EINVAL);
    }
    if isspliced(so) || issplicedback(so) {
        return Err(Errno::EOPNOTSUPP);
    }
    pru_listen(so)?;
    if so.so_q.first().is_none() {
        so.so_options.set(so.so_options.get() | SO_ACCEPTCONN);
    }
    if backlog < 0 || backlog > somaxconn_local {
        backlog = somaxconn_local;
    }
    if backlog < sominconn_local {
        backlog = sominconn_local;
    }
    so.so_qlimit.set(backlog as i16);
    Ok(())
}

/// `sorele(so)`: drops a reference; the last one frees the socket, its buffers and the
/// rights still in its receive buffer.
pub fn sorele(so: &'static Socket) {
    if !refcnt_rele(&so.so_refcnt) {
        return;
    }

    sigio_free(&so.so_sigio);
    // klist_free(&so->so_rcv.sb_klist), klist_free(&so->so_snd.sb_klist): kern_event.c.

    mtx_enter(&so.so_snd.sb_mtx);
    sbrelease(&so.so_snd);
    mtx_leave(&so.so_snd.sb_mtx);

    if so.pr_flags(PR_RIGHTS)
        && let Some(dispose) = so.so_proto.pr_domain.dom_dispose
    {
        dispose(so.so_rcv.sb_mb.get());
    }
    m_purge(so.so_rcv.sb_mb.get());

    if let Some(sp) = so.so_sp.get() {
        pool_put(&SOSPLICE_POOL, NonNull::from(sp).cast());
    }
    pool_put(&SOCKET_POOL, NonNull::from(so).cast());
}

/// `sofree(so, keep_lock)`: frees a socket that has neither a protocol control block nor a
/// file reference, taking it off its listener's partial queue first. Unlocks `so` unless
/// `keep_lock`.
pub fn sofree(so: &'static Socket, keep_lock: bool) {
    let persocket = solock_persocket(so);

    soassertlocked(so);

    if !so.so_pcb.get().is_null() || !so.has_state(SS_NOFDREF) {
        if !keep_lock {
            sounlock_shared(so);
        }
        return;
    }
    if let Some(head) = so.so_head.get() {
        // We must not decommission a socket that's on the accept(2) queue. If we do, then
        // accept(2) may hang after select(2) indicated that the listening socket was ready.
        if so.onq_is(&head.so_q) {
            if !keep_lock {
                sounlock_shared(so);
            }
            return;
        }

        if persocket {
            let _ = soref(Some(head));
            sounlock(so);
            solock(head);
            solock(so);

            if !so.onq_is(&head.so_q0) {
                sounlock(so);
                sounlock(head);
                sorele(head);
                return;
            }
        }

        soqremque(so, 0);

        if persocket {
            sounlock(head);
            sorele(head);
        }
    }

    if !keep_lock {
        sounlock_shared(so);
    }
    sorele(so);
}

/// `solinger_nsec(so)`: how long `soclose` lingers.
fn solinger_nsec(so: &Socket) -> u64 {
    if so.so_linger.get() == 0 {
        return INFSLP;
    }

    sec_to_nsec(so.so_linger.get() as u64)
}

/// Close a socket on last file table reference removal. Initiate disconnect if connected.
/// Free socket when disconnect complete.
pub fn soclose(so: &'static Socket, flags: i32) -> Result<(), Errno> {
    let mut error = Ok(());

    solock_shared(so);
    // Revoke async IO early. There is a final revocation in sofree().
    sigio_free(&so.so_sigio);
    'discard: {
        'drop: {
            if so.has_state(SS_ISCONNECTED) {
                if so.so_pcb.get().is_null() {
                    break 'discard;
                }
                if !so.has_state(SS_ISDISCONNECTING)
                    && let Err(e) = sodisconnect(so)
                {
                    error = Err(e);
                    break 'drop;
                }
                if so.has_options(SO_LINGER) {
                    if so.has_state(SS_ISDISCONNECTING) && flags & MSG_DONTWAIT != 0 {
                        break 'drop;
                    }
                    while so.has_state(SS_ISCONNECTED) {
                        if let Err(e) = sosleep_nsec(
                            so,
                            so.timeo_chan(),
                            PSOCK | PCATCH,
                            "netcls",
                            solinger_nsec(so),
                        ) {
                            error = Err(e);
                            break;
                        }
                    }
                }
            }
        }
        // drop:
        if !so.so_pcb.get().is_null() {
            let error2 = pru_detach(so);
            if error.is_ok() {
                error = error2;
            }
        }
        if so.has_options(SO_ACCEPTCONN) {
            let persocket = solock_persocket(so);

            while let Some(so2) = soq_first(&so.so_q0) {
                let _ = soref(Some(so2));
                solock(so2);
                let _ = soqremque(so2, 0);
                sounlock(so);
                soabort(so2);
                sounlock(so2);
                sorele(so2);
                solock(so);
            }
            while let Some(so2) = soq_first(&so.so_q) {
                let _ = soref(Some(so2));
                solock_nonet(so2);
                let _ = soqremque(so2, 1);
                if persocket {
                    sounlock(so);
                }
                soabort(so2);
                sounlock_nonet(so2);
                sorele(so2);
                if persocket {
                    solock(so);
                }
            }
        }
    }
    // discard:
    if let Some(sp) = so.so_sp.get() {
        sounlock_shared(so);
        // Concurrent sounsplice() locks `sb_mtx' mutexes on both `so_snd' and `so_rcv'
        // before unsplice sockets.
        mtx_enter(&so.so_snd.sb_mtx);
        let soback = soref(sp.ssp_soback.get());
        mtx_leave(&so.so_snd.sb_mtx);

        if let Some(soback) = soback {
            // `so' can be only unspliced, and never spliced again. Thus if issplicedback(so)
            // check is positive, socket is still spliced and `ssp_soback' points to the same
            // socket that `soback'.
            let _ = sblock(&soback.so_rcv, SBL_WAIT | SBL_NOINTR);
            if issplicedback(so)
                && let Some(back) = sp.ssp_soback.get()
            {
                let mut freeing = SOSP_FREEING_WRITE;

                if ptr::eq(back, so) {
                    freeing |= SOSP_FREEING_READ;
                }
                sounsplice(back, so, freeing);
            }
            sbunlock(&soback.so_rcv);
        }

        // notsplicedback:
        let _ = sblock(&so.so_rcv, SBL_WAIT | SBL_NOINTR);
        if isspliced(so)
            && let Some(drain) = sp.ssp_socket.get()
        {
            let mut freeing = SOSP_FREEING_READ;

            if ptr::eq(so, drain) {
                freeing |= SOSP_FREEING_WRITE;
            }
            sounsplice(so, drain, freeing);
        }
        sbunlock(&so.so_rcv);

        timeout_barrier(&sp.ssp_idleto);
        let spq = sp.ssp_queue.get();
        if let Some(spq) = spq {
            taskq_barrier(spq);
        }
        if let Some(soback) = soback {
            let spqback = soback.so_sp.get().and_then(|bsp| bsp.ssp_queue.get());
            if let Some(spqback) = spqback
                && !spq.is_some_and(|spq| ptr::eq(spq, spqback))
            {
                taskq_barrier(spqback);
            }
            sorele(soback);
        }

        solock_shared(so);
    }

    if so.has_state(SS_NOFDREF) {
        panic(format_args!(
            "soclose NOFDREF: so {:p}, so_type {}",
            so,
            so.so_type.get()
        ));
    }
    so.set_state(SS_NOFDREF);

    // sofree() calls sounlock().
    sofree(so, false);
    error
}

/// `soabort(so)`: `pru_abort`.
pub fn soabort(so: &'static Socket) {
    soassertlocked(so);
    pru_abort(so);
}

/// `soaccept(so, nam)`: a connection taken off its listener's queue gets a file reference
/// and the peer's address in `nam`.
pub fn soaccept(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    soassertlocked(so);

    if !so.has_state(SS_NOFDREF) {
        panic(format_args!(
            "soaccept !NOFDREF: so {:p}, so_type {}",
            so,
            so.so_type.get()
        ));
    }
    so.clear_state(SS_NOFDREF);
    if !so.has_state(SS_ISDISCONNECTED) || !so.pr_flags(PR_ABRTACPTDIS) {
        pru_accept(so, nam)
    } else {
        Err(Errno::ECONNABORTED)
    }
}

/// `soconnect(so, nam)`: connects `so` to the address in `nam`.
pub fn soconnect(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    soassertlocked(so);

    if so.has_options(SO_ACCEPTCONN) {
        return Err(Errno::EOPNOTSUPP);
    }
    // If protocol is connection-based, can only connect once. Otherwise, if connected, try
    // to disconnect first. This allows user to disconnect by connecting to, e.g., a null
    // address.
    if so.has_state(SS_ISCONNECTED | SS_ISCONNECTING)
        && (so.pr_flags(PR_CONNREQUIRED) || sodisconnect(so).is_err())
    {
        Err(Errno::EISCONN)
    } else {
        pru_connect(so, nam)
    }
}

/// `soconnect2(so1, so2)`: connects two sockets of one protocol to each other.
pub fn soconnect2(so1: &'static Socket, so2: &'static Socket) -> Result<(), Errno> {
    solock_pair(so1, so2);
    let error = pru_connect2(so1, so2);
    sounlock_pair(so1, so2);

    error
}

/// `sodisconnect(so)`: starts disconnecting a connected socket.
pub fn sodisconnect(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);

    if !so.has_state(SS_ISCONNECTED) {
        return Err(Errno::ENOTCONN);
    }
    if so.has_state(SS_ISDISCONNECTING) {
        return Err(Errno::EALREADY);
    }
    pru_disconnect(so)
}

/// `SBLOCKWAIT(f)`.
const fn sblockwait(f: i32) -> i32 {
    if f & MSG_DONTWAIT != 0 { 0 } else { SBL_WAIT }
}

/// Send on a socket. If send must go all at once and message is larger than send buffering,
/// then hard error. Lock against other senders. If must go all at once and not enough room
/// now, then inform user that this would block and do nothing. Otherwise, if nonblocking,
/// send as much as possible. The data to be sent is described by `uio` if `Some`, otherwise
/// by the mbuf chain `top` (which must be `None` if `uio` is not). Data provided in mbuf
/// chain must be small enough to send all at once.
///
/// Returns an error on error, timeout or signal; callers must check for short counts if
/// `EINTR`/`ERESTART` are returned. Data and control buffers are freed on return.
pub fn sosend(
    so: &'static Socket,
    addr: Option<&'static Mbuf>,
    uio: Option<&mut Uio<'_>>,
    top: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
    flags: i32,
) -> Result<(), Errno> {
    let mut uio = uio;
    let mut top = top;
    let mut control = control;
    let mut clen: i64 = 0;
    let atomic = sosendallatonce(so) || top.is_some();

    let mut resid: usize = match &uio {
        Some(uio) => uio.uio_resid,
        None => top.map_or(0, |top| top.m_pkthdr().len.get() as usize),
    };
    // MSG_EOR on a SOCK_STREAM socket is invalid.
    if so.so_type.get() == SOCK_STREAM && flags & MSG_EOR != 0 {
        m_freem(top);
        m_freem(control);
        return Err(Errno::EINVAL);
    }
    if let Some(uio) = &uio
        && let Some(p) = uio.uio_procp
    {
        p.p_ru.ru_msgsnd.set(p.p_ru.ru_msgsnd.get() + 1);
    }
    if let Some(c) = control {
        // In theory clen should be unsigned (since control->m_len is). However, space must
        // be signed, as it might be less than 0 if we over-committed, and we must use a
        // signed comparison of space and clen.
        clen = i64::from(c.m_len().get());
        // reserve extra space for AF_UNIX's internalize
        let hdr = cmsg_align(size_of::<Cmsghdr>()) as i64;
        if so.dom_family() == i32::from(AF_UNIX) && clen >= hdr && cmsg_type_of(c) == SCM_RIGHTS {
            clen =
                cmsg_space((clen - hdr) as usize * (size_of::<Fdpass>() / size_of::<i32>())) as i64;
        }
    }

    let error: Result<(), Errno> = 'out: {
        'restart: loop {
            if let Err(e) = sblock(&so.so_snd, sblockwait(flags)) {
                break 'out Err(e);
            }
            mtx_enter(&so.so_snd.sb_mtx);
            so.so_snd.set_state(SS_ISSENDING);
            let error: Result<(), Errno> = 'release: {
                loop {
                    if so.so_snd.has_state(SS_CANTSENDMORE) {
                        break 'release Err(Errno::EPIPE);
                    }
                    if let Some(e) = so.error() {
                        so.set_error(None);
                        break 'release Err(e);
                    }
                    if !so.has_state(SS_ISCONNECTED) {
                        if so.pr_flags(PR_CONNREQUIRED) {
                            if !(resid == 0 && clen != 0) {
                                break 'release Err(Errno::ENOTCONN);
                            }
                        } else if addr.is_none() {
                            break 'release Err(Errno::EDESTADDRREQ);
                        }
                    }
                    let mut space = sbspace_locked(&so.so_snd);
                    if flags & MSG_OOB != 0 {
                        space += 1024;
                    }
                    let hiwat = so.so_snd.sb_hiwat.get() as i64;
                    if so.dom_family() == i32::from(AF_UNIX) {
                        if atomic && resid as i64 > hiwat {
                            break 'release Err(Errno::EMSGSIZE);
                        }
                    } else if clen > hiwat || (atomic && resid as i64 > hiwat - clen) {
                        break 'release Err(Errno::EMSGSIZE);
                    }
                    if space < clen
                        || (space - clen < resid as i64
                            && (atomic || space < so.so_snd.sb_lowat.get()))
                    {
                        if flags & MSG_DONTWAIT != 0 {
                            break 'release Err(Errno::EWOULDBLOCK);
                        }
                        sbunlock(&so.so_snd);
                        let error = sbwait(&so.so_snd);
                        so.so_snd.clear_state(SS_ISSENDING);
                        mtx_leave(&so.so_snd.sb_mtx);
                        if let Err(e) = error {
                            break 'out Err(e);
                        }
                        continue 'restart;
                    }
                    space -= clen;
                    loop {
                        match uio.as_deref_mut() {
                            None => {
                                // Data is prepackaged in "top".
                                resid = 0;
                                if flags & MSG_EOR != 0
                                    && let Some(top) = top
                                {
                                    top.m_flags().set(top.m_flags().get() | M_EOR);
                                }
                            }
                            Some(uio) => {
                                mtx_leave(&so.so_snd.sb_mtx);
                                let chain = m_getuio(atomic, space, uio);
                                mtx_enter(&so.so_snd.sb_mtx);
                                let t = match chain {
                                    Ok(t) => t,
                                    Err(e) => break 'release Err(e),
                                };
                                top = Some(t);
                                space -= i64::from(t.m_pkthdr().len.get());
                                resid = uio.uio_resid;
                                if flags & MSG_EOR != 0 {
                                    t.m_flags().set(t.m_flags().get() | M_EOR);
                                }
                            }
                        }
                        if resid == 0 {
                            so.so_snd.clear_state(SS_ISSENDING);
                        }
                        if let Some(top) = top
                            && so.has_options(SO_ZEROIZE)
                        {
                            top.m_flags().set(top.m_flags().get() | M_ZEROIZE);
                        }
                        mtx_leave(&so.so_snd.sb_mtx);
                        solock_shared(so);
                        let error = if flags & MSG_OOB != 0 {
                            pru_sendoob(so, top, addr, control)
                        } else {
                            pru_send(so, top, addr, control)
                        };
                        sounlock_shared(so);
                        mtx_enter(&so.so_snd.sb_mtx);
                        clen = 0;
                        control = None;
                        top = None;
                        if let Err(e) = error {
                            break 'release Err(e);
                        }
                        if !(resid != 0 && space > 0) {
                            break;
                        }
                    }
                    if resid == 0 {
                        break;
                    }
                }
                Ok(())
            };
            // release:
            so.so_snd.clear_state(SS_ISSENDING);
            mtx_leave(&so.so_snd.sb_mtx);
            sbunlock(&so.so_snd);
            break 'out error;
        }
    };
    // out:
    m_freem(top);
    m_freem(control);
    error
}

/// `m_getuio(mp, atomic, space, uio)`: copies at most `space` bytes of `uio` into a new
/// packet (a chain of mbufs and clusters); a datagram's first mbuf leaves room for the
/// protocol headers.
pub fn m_getuio(atomic: bool, space: i64, uio: &mut Uio<'_>) -> Result<&'static Mbuf, Errno> {
    let mut top: Option<&'static Mbuf> = None;
    let mut last: Option<&'static Mbuf> = None;
    let mut resid = uio.uio_resid as u64;
    let mut space = space;
    let max_hdr = MAX_HDR.load(Ordering::Relaxed) as u64;

    loop {
        let (m, mut mlen) = if top.is_none() {
            (m_gethdr(M_WAIT, MT_DATA), MHLEN as u64)
        } else {
            (m_get(M_WAIT, MT_DATA), MLEN as u64)
        };
        let Some(m) = m else {
            m_freem(top);
            return Err(Errno::ENOBUFS);
        };
        // chain mbuf together
        match last {
            Some(last) => last.m_next().set(Some(m)),
            None => top = Some(m),
        }
        last = Some(m);
        let head = top.unwrap_or(m);

        resid = resid.min(space as u64);
        let mut nopages = true;
        let mut len = 0;
        if resid >= MINCLSIZE as u64 {
            let _ = mclgetl(m, M_DONTWAIT, resid.min(MAXMCLBYTES as u64) as u32);
            if m.m_flags().get() & M_EXT == 0 {
                let _ = mclgetl(m, M_DONTWAIT, MCLBYTES as u32);
            }
            if m.m_flags().get() & M_EXT != 0 {
                nopages = false;
                mlen = u64::from(m.m_ext().ext_size.get());
                len = mlen.min(resid);
                // For datagram protocols, leave room for protocol headers in first mbuf.
                if atomic && ptr::eq(m, head) && len < mlen.wrapping_sub(max_hdr) {
                    m.m_data()
                        .set(m.m_data().get().wrapping_add(max_hdr as usize));
                }
            }
        }
        if nopages {
            len = mlen.min(resid);
            // For datagram protocols, leave room for protocol headers in first mbuf.
            if atomic && ptr::eq(m, head) && len < mlen.wrapping_sub(max_hdr) {
                m_align(m, len as i32);
            }
        }

        // SAFETY: `len` is at most the room after `m_data` (`mlen`, less what `m_align` or
        // the header adjustment skipped, which only happens when `len` still fits); the mbuf
        // is ours alone until it is chained into a socket buffer.
        let buf = unsafe { mdata(m, 0, len as usize) };
        if let Err(error) = uiomove(buf, uio) {
            m_freem(top);
            return Err(error);
        }

        // adjust counters
        resid = uio.uio_resid as u64;
        space -= len as i64;
        m.m_len().set(len as u32);
        head.m_pkthdr()
            .len
            .set(head.m_pkthdr().len.get() + len as i32);

        // Is there more space and more data?
        if !(space > 0 && resid > 0) {
            break;
        }
    }

    Ok(top.unwrap_or_else(|| panic(format_args!("m_getuio: no mbuf"))))
}

/// Following replacement or removal of the first mbuf on the first mbuf chain of a socket
/// buffer, push necessary state changes back into the socket buffer so that other consumers
/// see the values consistently. `nextrecord` is the callers locally stored value of the
/// original value of `sb->sb_mb->m_nextpkt` which must be restored when the lead mbuf
/// changes. NOTE: `nextrecord` may be `None`.
pub fn sbsync(sb: &Sockbuf, nextrecord: Option<&'static Mbuf>) {
    // First, update for the new value of nextrecord. If necessary, make it the first record.
    match sb.sb_mb.get() {
        Some(mb) => mb.m_nextpkt().set(nextrecord),
        None => sb.sb_mb.set(nextrecord),
    }

    // Now update any dependent socket buffer fields to reflect the new state. This is an
    // inline of SB_EMPTY_FIXUP, with the addition of a second clause that takes care of the
    // case where sb_mb has been updated, but remains the last record.
    match sb.sb_mb.get() {
        None => {
            sb.sb_mbtail.set(None);
            sb.sb_lastrecord.set(None);
        }
        Some(mb) if mb.m_nextpkt().get().is_none() => sb.sb_lastrecord.set(Some(mb)),
        Some(_) => {}
    }
}

/// Implement receive operations on a socket. We depend on the way that records are added to
/// the sockbuf by `sbappend*`. In particular, each record (mbufs linked through `m_next`)
/// must begin with an address if the protocol so specifies, followed by an optional mbuf or
/// mbufs containing ancillary data, and then zero or more mbufs of data. In order to avoid
/// blocking network for the entire time here, we release the `solock()` while doing the
/// actual copy to user space. Although the sockbuf is locked, new data may still be
/// appended, and thus we must maintain consistency of the sockbuf during that time.
///
/// The caller may receive the data as a single mbuf chain by supplying an `mp0` for use in
/// returning the chain. The uio is then used only for the count in `uio_resid`.
pub fn soreceive(
    so: &'static Socket,
    paddr: Option<&mut Option<&'static Mbuf>>,
    uio: &mut Uio<'_>,
    mp0: Option<&mut Option<&'static Mbuf>>,
    controlp: Option<&mut Option<&'static Mbuf>>,
    flagsp: Option<&mut i32>,
    controllen: Socklen,
) -> Result<(), Errno> {
    let pr = so.so_proto;
    let mut orig_resid = uio.uio_resid;
    let mut uio_error: Result<(), Errno> = Ok(());

    let mut paddr = paddr;
    let mut flagsp = flagsp;
    let mut mp = mp0.map(MbufTail::new);
    if let Some(paddr) = paddr.as_deref_mut() {
        *paddr = None;
    }
    let mut controlp = controlp.map(|c| {
        *c = None;
        MbufTail::new(c)
    });
    let mut flags = match flagsp.as_deref() {
        Some(f) => *f & !MSG_EOR,
        None => 0,
    };
    if flags & MSG_OOB != 0 {
        let Some(m) = m_get(M_WAIT, MT_DATA) else {
            return Err(Errno::ENOBUFS);
        };
        solock_shared(so);
        let mut error = pru_rcvoob(so, m, flags & MSG_PEEK);
        sounlock_shared(so);
        let mut m = Some(m);
        if error.is_ok() {
            while let Some(mm) = m {
                let len = uio.uio_resid.min(mm.m_len().get() as usize);
                // SAFETY: `len` is at most the mbuf's length; the mbuf is ours.
                error = uiomove(unsafe { mdata(mm, 0, len) }, uio);
                m = m_free(mm);
                if !(uio.uio_resid != 0 && error.is_ok()) {
                    break;
                }
            }
        }
        // bad:
        m_freem(m);
        return error;
    }
    if let Some(mp) = mp.as_mut() {
        mp.set(None);
    }

    'restart: loop {
        sblock(&so.so_rcv, sblockwait(flags))?;
        mtx_enter(&so.so_rcv.sb_mtx);

        let mut m = so.so_rcv.sb_mb.get();
        if isspliced(so) {
            m = None;
        }
        let mut error: Result<(), Errno> = Ok(());

        let result: Result<(), Errno> = 'release: {
            // If we have less data than requested, block awaiting more (subject to any
            // timeout) if:
            //   1. the current count is less than the low water mark,
            //   2. MSG_WAITALL is set, and it is possible to do the entire receive operation
            //      at once if we block (resid <= hiwat), or
            //   3. MSG_DONTWAIT is not set.
            // If MSG_WAITALL is set but resid is larger than the receive buffer, we have to
            // do the receive in sections, and thus risk returning a short count if a timeout
            // or signal occurs after we start.
            let sb_cc = so.so_rcv.sb_cc.get();
            let must_wait = match m {
                None => true,
                Some(mm) => {
                    (flags & MSG_DONTWAIT == 0 && (sb_cc as usize) < uio.uio_resid)
                        && ((sb_cc as i64) < so.so_rcv.sb_lowat.get()
                            || (flags & MSG_WAITALL != 0
                                && uio.uio_resid as u64 <= so.so_rcv.sb_hiwat.get()))
                        && mm.m_nextpkt().get().is_none()
                        && pr.pr_flags & PR_ATOMIC == 0
                }
            };
            if must_wait {
                'dontblock: {
                    if cfg!(feature = "diagnostic") && m.is_none() && sb_cc != 0 && !isspliced(so) {
                        panic(format_args!(
                            "receive 1: so {:p}, so_type {}, sb_cc {}",
                            so,
                            so.so_type.get(),
                            sb_cc
                        ));
                    }
                    if let Some(error2) = so.error() {
                        if m.is_some() {
                            break 'dontblock;
                        }
                        if flags & MSG_PEEK == 0 {
                            so.set_error(None);
                        }
                        break 'release Err(error2);
                    }
                    if so.so_rcv.has_state(SS_CANTRCVMORE) {
                        if m.is_some() {
                            break 'dontblock;
                        } else if so.so_rcv.sb_cc.get() == 0 {
                            break 'release Ok(());
                        }
                    }
                    while let Some(mm) = m {
                        if i32::from(mm.m_type().get()) == MT_OOBDATA
                            || mm.m_flags().get() & M_EOR != 0
                        {
                            m = so.so_rcv.sb_mb.get();
                            break 'dontblock;
                        }
                        m = mm.m_next().get();
                    }
                    if !so.has_state(SS_ISCONNECTED | SS_ISCONNECTING)
                        && so.pr_flags(PR_CONNREQUIRED)
                    {
                        break 'release Err(Errno::ENOTCONN);
                    }
                    if uio.uio_resid == 0 && controlp.is_none() {
                        break 'release Ok(());
                    }
                    if flags & MSG_DONTWAIT != 0 {
                        break 'release Err(Errno::EWOULDBLOCK);
                    }

                    sbunlock(&so.so_rcv);
                    let error = sbwait(&so.so_rcv);
                    mtx_leave(&so.so_rcv.sb_mtx);
                    error?;
                    continue 'restart;
                }
            }
            // dontblock:
            // On entry here, m points to the first record of the socket buffer. From this
            // point onward, we maintain 'nextrecord' as a cache of the pointer to the next
            // record in the socket buffer. We must keep the various socket buffer pointers
            // and local stack versions of the pointers in sync, pushing out modifications
            // before operations that may sleep, and re-reading them afterwards.
            //
            // Otherwise, we will race with the network stack appending new data or records
            // onto the socket buffer by using inconsistent/stale versions of the field,
            // possibly resulting in socket buffer corruption.
            if let Some(p) = uio.uio_procp {
                p.p_ru.ru_msgrcv.set(p.p_ru.ru_msgrcv.get() + 1);
            }
            let Some(first) = m else {
                panic(format_args!("soreceive: no record"));
            };
            kassert!(so.so_rcv.sb_mb.get().is_some_and(|mb| ptr::eq(mb, first)));
            let mut nextrecord = first.m_nextpkt().get();
            if pr.pr_flags & PR_ADDR != 0 {
                if cfg!(feature = "diagnostic") && i32::from(first.m_type().get()) != MT_SONAME {
                    panic(format_args!(
                        "receive 1a: so {:p}, so_type {}, m {:p}, m_type {}",
                        so,
                        so.so_type.get(),
                        first,
                        first.m_type().get()
                    ));
                }
                orig_resid = 0;
                if flags & MSG_PEEK != 0 {
                    if let Some(paddr) = paddr.as_deref_mut() {
                        *paddr = m_copym(first, 0, first.m_len().get() as i32, M_DONTWAIT);
                    }
                    m = first.m_next().get();
                } else {
                    sbfree(&so.so_rcv, first);
                    if let Some(paddr) = paddr.as_deref_mut() {
                        *paddr = Some(first);
                        so.so_rcv.sb_mb.set(first.m_next().get());
                        first.m_next().set(None);
                        m = so.so_rcv.sb_mb.get();
                    } else {
                        so.so_rcv.sb_mb.set(m_free(first));
                        m = so.so_rcv.sb_mb.get();
                    }
                    sbsync(&so.so_rcv, nextrecord);
                }
            }
            while let Some(mm) = m
                && i32::from(mm.m_type().get()) == MT_CONTROL
                && error.is_ok()
            {
                let mut skip = false;
                if flags & MSG_PEEK != 0 {
                    if cmsg_type_of(mm) == SCM_RIGHTS {
                        // don't leak internalized SCM_RIGHTS msgs
                        skip = true;
                    } else if let Some(controlp) = controlp.as_mut() {
                        controlp.set(m_copym(mm, 0, mm.m_len().get() as i32, M_DONTWAIT));
                    }
                    m = mm.m_next().get();
                } else {
                    sbfree(&so.so_rcv, mm);
                    so.so_rcv.sb_mb.set(mm.m_next().get());
                    mm.m_nextpkt().set(None);
                    mm.m_next().set(None);
                    let cm = mm;
                    m = so.so_rcv.sb_mb.get();
                    sbsync(&so.so_rcv, nextrecord);
                    if let Some(controlp) = controlp.as_mut() {
                        if let Some(externalize) = pr.pr_domain.dom_externalize {
                            mtx_leave(&so.so_rcv.sb_mtx);
                            error = externalize(cm, controllen, flags);
                            mtx_enter(&so.so_rcv.sb_mtx);
                        }
                        controlp.set(Some(cm));
                    } else {
                        // Dispose of any SCM_RIGHTS message that went through the read path
                        // rather than recv.
                        if let Some(dispose) = pr.pr_domain.dom_dispose {
                            mtx_leave(&so.so_rcv.sb_mtx);
                            dispose(Some(cm));
                            mtx_enter(&so.so_rcv.sb_mtx);
                        }
                        m_free(cm);
                    }
                }
                if m.is_some() {
                    nextrecord = so.so_rcv.sb_mb.get().and_then(|mb| mb.m_nextpkt().get());
                } else {
                    nextrecord = so.so_rcv.sb_mb.get();
                }
                if let Some(controlp) = controlp.as_mut()
                    && !skip
                {
                    controlp.advance();
                }
                orig_resid = 0;
            }

            // If m is non-NULL, we have some data to read.
            let mut type_ = 0;
            if let Some(mm) = m {
                type_ = i32::from(mm.m_type().get());
                if type_ == MT_OOBDATA {
                    flags |= MSG_OOB;
                }
                if mm.m_flags().get() & M_BCAST != 0 {
                    flags |= MSG_BCAST;
                }
                if mm.m_flags().get() & M_MCAST != 0 {
                    flags |= MSG_MCAST;
                }
            }

            let mut moff: u64 = 0;
            let mut offset: u64 = 0;
            while let Some(mm) = m
                && uio.uio_resid > 0
                && error.is_ok()
            {
                let mtype = i32::from(mm.m_type().get());
                if mtype == MT_OOBDATA {
                    if type_ != MT_OOBDATA {
                        break;
                    }
                } else if type_ == MT_OOBDATA {
                    break;
                } else if mtype == MT_CONTROL {
                    // If there is more than one control message in the stream, we do a short
                    // read. Next can be received or disposed by another system call.
                    break;
                } else if cfg!(feature = "diagnostic") && mtype != MT_DATA && mtype != MT_HEADER {
                    panic(format_args!(
                        "receive 3: so {:p}, so_type {}, m {:p}, m_type {}",
                        so,
                        so.so_type.get(),
                        mm,
                        mtype
                    ));
                }
                so.so_rcv.clear_state(SS_RCVATMARK);
                let mut len = uio.uio_resid as u64;
                let oobmark = so.so_oobmark.get();
                if oobmark != 0 && len > oobmark - offset {
                    len = oobmark - offset;
                }
                let mlen = u64::from(mm.m_len().get());
                if len > mlen - moff {
                    len = mlen - moff;
                }
                // If mp is set, just pass back the mbufs. Otherwise copy them out via the
                // uio, then free. Sockbuf must be consistent here (points to current mbuf,
                // it points to next record) when we drop priority; we must note any
                // additions to the sockbuf when we block interrupts again.
                if mp.is_none() && uio_error.is_ok() {
                    let resid = uio.uio_resid;
                    mtx_leave(&so.so_rcv.sb_mtx);
                    // SAFETY: `moff + len` is at most the mbuf's length (clamped above); the
                    // receive buffer's `sblock` keeps the record in place.
                    uio_error = uiomove(unsafe { mdata(mm, moff as usize, len as usize) }, uio);
                    mtx_enter(&so.so_rcv.sb_mtx);
                    if uio_error.is_err() {
                        uio.uio_resid = resid - len as usize;
                    }
                } else {
                    uio.uio_resid -= len as usize;
                }
                if len == mlen - moff {
                    if mm.m_flags().get() & M_EOR != 0 {
                        flags |= MSG_EOR;
                    }
                    if flags & MSG_PEEK != 0 {
                        m = mm.m_next().get();
                        moff = 0;
                        orig_resid = 0;
                    } else {
                        nextrecord = mm.m_nextpkt().get();
                        sbfree(&so.so_rcv, mm);
                        if let Some(mp) = mp.as_mut() {
                            mp.set(Some(mm));
                            mp.advance();
                            m = mm.m_next().get();
                            so.so_rcv.sb_mb.set(m);
                            mp.set(None);
                        } else {
                            so.so_rcv.sb_mb.set(m_free(mm));
                            m = so.so_rcv.sb_mb.get();
                        }
                        // If m != NULL, we also know that so->so_rcv.sb_mb != NULL.
                        kassert!(match (so.so_rcv.sb_mb.get(), m) {
                            (Some(a), Some(b)) => ptr::eq(a, b),
                            (None, None) => true,
                            _ => false,
                        });
                        match m {
                            Some(mm) => {
                                mm.m_nextpkt().set(nextrecord);
                                if nextrecord.is_none() {
                                    so.so_rcv.sb_lastrecord.set(Some(mm));
                                }
                            }
                            None => {
                                so.so_rcv.sb_mb.set(nextrecord);
                                sb_empty_fixup(&so.so_rcv);
                            }
                        }
                    }
                } else if flags & MSG_PEEK != 0 {
                    moff += len;
                    orig_resid = 0;
                } else {
                    if let Some(mp) = mp.as_mut() {
                        mtx_leave(&so.so_rcv.sb_mtx);
                        mp.set(m_copym(mm, 0, len as i32, M_WAIT));
                        mtx_enter(&so.so_rcv.sb_mtx);
                    }
                    mm.m_data()
                        .set(mm.m_data().get().wrapping_add(len as usize));
                    mm.m_len().set(mm.m_len().get() - len as u32);
                    so.so_rcv.sb_cc.set(so.so_rcv.sb_cc.get() - len);
                    so.so_rcv.sb_datacc.set(so.so_rcv.sb_datacc.get() - len);
                }
                if so.so_oobmark.get() != 0 {
                    if flags & MSG_PEEK == 0 {
                        so.so_oobmark.set(so.so_oobmark.get() - len);
                        if so.so_oobmark.get() == 0 {
                            so.so_rcv.set_state(SS_RCVATMARK);
                            break;
                        }
                    } else {
                        offset += len;
                        if offset == so.so_oobmark.get() {
                            break;
                        }
                    }
                }
                if flags & MSG_EOR != 0 {
                    break;
                }
                // If the MSG_WAITALL flag is set (for non-atomic socket), we must not quit
                // until "uio->uio_resid == 0" or an error termination. If a signal/timeout
                // occurs, return with a short count but without error. Keep sockbuf locked
                // against other readers.
                while flags & MSG_WAITALL != 0
                    && m.is_none()
                    && uio.uio_resid > 0
                    && !sosendallatonce(so)
                    && nextrecord.is_none()
                {
                    if so.so_rcv.has_state(SS_CANTRCVMORE) || so.error().is_some() {
                        break;
                    }
                    if sbwait(&so.so_rcv).is_err() {
                        mtx_leave(&so.so_rcv.sb_mtx);
                        sbunlock(&so.so_rcv);
                        return Ok(());
                    }
                    m = so.so_rcv.sb_mb.get();
                    if let Some(mm) = m {
                        nextrecord = mm.m_nextpkt().get();
                    }
                }
            }

            if m.is_some() && pr.pr_flags & PR_ATOMIC != 0 {
                flags |= MSG_TRUNC;
                if flags & MSG_PEEK == 0 {
                    sbdroprecord(&so.so_rcv);
                }
            }
            if flags & MSG_PEEK == 0 {
                if m.is_none() {
                    // First part is an inline SB_EMPTY_FIXUP(). Second part makes sure
                    // sb_lastrecord is up-to-date if there is still data in the socket
                    // buffer.
                    so.so_rcv.sb_mb.set(nextrecord);
                    match nextrecord {
                        None => {
                            so.so_rcv.sb_mbtail.set(None);
                            so.so_rcv.sb_lastrecord.set(None);
                        }
                        Some(nr) if nr.m_nextpkt().get().is_none() => {
                            so.so_rcv.sb_lastrecord.set(Some(nr));
                        }
                        Some(_) => {}
                    }
                }
                if pr.pr_flags & PR_WANTRCVD != 0 {
                    mtx_leave(&so.so_rcv.sb_mtx);
                    solock_shared(so);
                    pru_rcvd(so);
                    sounlock_shared(so);
                    mtx_enter(&so.so_rcv.sb_mtx);
                }
            }
            if orig_resid == uio.uio_resid
                && orig_resid != 0
                && flags & MSG_EOR == 0
                && !so.so_rcv.has_state(SS_CANTRCVMORE)
            {
                mtx_leave(&so.so_rcv.sb_mtx);
                sbunlock(&so.so_rcv);
                continue 'restart;
            }

            if uio_error.is_err() {
                error = uio_error;
            }

            if let Some(flagsp) = flagsp.as_deref_mut() {
                *flagsp |= flags;
            }
            error
        };
        // release:
        mtx_leave(&so.so_rcv.sb_mtx);
        sbunlock(&so.so_rcv);
        return result;
    }
}

/// `soshutdown(so, how)`: `SHUT_RD` flushes the receive side, `SHUT_WR` tells the protocol
/// no more data will be sent, `SHUT_RDWR` both.
pub fn soshutdown(so: &'static Socket, how: i32) -> Result<(), Errno> {
    let shut_wr = |so: &'static Socket| {
        solock_shared(so);
        let error = pru_shutdown(so);
        sounlock_shared(so);
        error
    };

    match how {
        SHUT_RD => {
            sorflush(so);
            Ok(())
        }
        SHUT_RDWR => {
            sorflush(so);
            shut_wr(so)
        }
        SHUT_WR => shut_wr(so),
        _ => Err(Errno::EINVAL),
    }
}

/// `sorflush(so)`: no more data will be received: empty the receive buffer, disposing of
/// the rights in it.
pub fn sorflush(so: &'static Socket) {
    let sb = &so.so_rcv;
    let pr = so.so_proto;

    let error = sblock(sb, SBL_WAIT | SBL_NOINTR);
    // with SBL_WAIT and SLB_NOINTR sblock() must not fail
    kassert!(error.is_ok());

    solock_shared(so);
    socantrcvmore(so);
    sounlock_shared(so);
    mtx_enter(&sb.sb_mtx);
    let m = sb.sb_mb.get();
    sb.zero_counts();
    sb.sb_timeo_nsecs.set(INFSLP);
    mtx_leave(&sb.sb_mtx);
    sbunlock(sb);

    if pr.pr_flags & PR_RIGHTS != 0
        && let Some(dispose) = pr.pr_domain.dom_dispose
    {
        dispose(m);
    }
    m_purge(m);
}

/// A fresh `sosplice_pool` item, or `None` when the pool is empty.
fn sosplice_alloc(so: &'static Socket) -> Option<&'static Sosplice> {
    let mem = pool_get(&SOSPLICE_POOL, PR_WAITOK | PR_ZERO)?;
    let raw = mem.cast::<Sosplice>().as_ptr();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Sosplice>()` bytes, written
    // once before anything else sees it.
    unsafe { raw.write(Sosplice::new()) };
    // SAFETY: as above; it stays allocated until `sorele` gives it back with its socket.
    let sp: &'static Sosplice = unsafe { &*raw };
    let arg = ptr::from_ref(so).cast_mut().cast::<c_void>();
    timeout_set_flags(
        &sp.ssp_idleto,
        soidle,
        arg,
        KCLOCK_NONE,
        TIMEOUT_PROC | TIMEOUT_MPSAFE,
    );
    task_set(&sp.ssp_task, sotask, arg);
    Some(sp)
}

/// `sosplice(so, fd, max, tv)`: splices `so`'s received data into the socket `fd` (at most
/// `max` bytes, unspliced after `tv` idle); `fd` < 0 unsplices.
pub fn sosplice(so: &'static Socket, fd: i32, max: Off, tv: Option<&Timeval>) -> Result<(), Errno> {
    if !so.pr_flags(PR_SPLICE) {
        return Err(Errno::EPROTONOSUPPORT);
    }
    if max != 0 && max < 0 {
        return Err(Errno::EINVAL);
    }
    if let Some(tv) = tv
        && (tv.tv_sec < 0 || !tv.is_valid())
    {
        return Err(Errno::EINVAL);
    }

    // If no fd is given, unsplice by removing existing link.
    if fd < 0 {
        sblock(&so.so_rcv, SBL_WAIT)?;
        if let Err(error) = sblock(&so.so_snd, SBL_WAIT) {
            sbunlock(&so.so_rcv);
            return Err(error);
        }
        let error = match so.so_sp.get().and_then(|sp| sp.ssp_socket.get()) {
            Some(drain) => {
                sounsplice(so, drain, 0);
                Ok(())
            }
            None => Err(Errno::EPROTO),
        };
        sbunlock(&so.so_snd);
        sbunlock(&so.so_rcv);
        return error;
    }

    // Find sosp, the drain socket where data will be spliced into.
    let p = curproc_or_panic("sosplice");
    let fp = getsock(p, fd)?;
    let sosp = crate::kern::sys_socket::fp_socket(fp);

    let error: Result<(), Errno> = 'frele: {
        let send = |s: &Socket| s.so_proto.pr_usrreqs.and_then(|u| u.pru_send);
        let same_send = match (send(sosp), send(so)) {
            (Some(a), Some(b)) => ptr::fn_addr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same_send {
            break 'frele Err(Errno::EPROTONOSUPPORT);
        }

        if let Err(e) = sblock(&so.so_rcv, SBL_WAIT) {
            break 'frele Err(e);
        }
        if let Err(e) = sblock(&sosp.so_snd, SBL_WAIT) {
            sbunlock(&so.so_rcv);
            break 'frele Err(e);
        }
        solock_pair(so, sosp);

        let error: Result<(), Errno> = 'release: {
            if so.has_options(SO_ACCEPTCONN) || sosp.has_options(SO_ACCEPTCONN) {
                break 'release Err(Errno::EOPNOTSUPP);
            }
            if !so.has_state(SS_ISCONNECTED | SS_ISCONNECTING) && so.pr_flags(PR_CONNREQUIRED) {
                break 'release Err(Errno::ENOTCONN);
            }
            if !sosp.has_state(SS_ISCONNECTED | SS_ISCONNECTING) {
                break 'release Err(Errno::ENOTCONN);
            }
            if so.so_sp.get().is_none() {
                let Some(sp) = sosplice_alloc(so) else {
                    break 'release Err(Errno::ENOBUFS);
                };
                so.so_sp.set(Some(sp));
            }
            if sosp.so_sp.get().is_none() {
                let Some(sp) = sosplice_alloc(sosp) else {
                    break 'release Err(Errno::ENOBUFS);
                };
                sosp.so_sp.set(Some(sp));
            }
            let sp = so_sp(so);
            let sosp_sp = so_sp(sosp);
            if sp.ssp_socket.get().is_some() || sosp_sp.ssp_soback.get().is_some() {
                break 'release Err(Errno::EBUSY);
            }

            sp.ssp_len.set(0);
            sp.ssp_max.set(max);
            match tv {
                Some(tv) => sp.ssp_idletv.set(*tv),
                None => sp.ssp_idletv.set(Timeval::new(0, 0)),
            }

            // To prevent sorwakeup() calling somove() before this somove() has finished, the
            // socket buffers are not marked as spliced yet.

            // Splice so and sosp together.
            mtx_enter(&so.so_rcv.sb_mtx);
            mtx_enter(&sosp.so_snd.sb_mtx);
            sp.ssp_socket.set(soref(Some(sosp)));
            sosp_sp.ssp_soback.set(soref(Some(so)));
            sp.ssp_queue.set(net_tq(pru_flowid(so) as u32));
            mtx_leave(&sosp.so_snd.sb_mtx);
            mtx_leave(&so.so_rcv.sb_mtx);

            sounlock_pair(so, sosp);
            sbunlock(&sosp.so_snd);

            if somove(so, M_WAIT) {
                mtx_enter(&so.so_rcv.sb_mtx);
                mtx_enter(&sosp.so_snd.sb_mtx);
                so.so_rcv.set_flags(SB_SPLICE);
                sosp.so_snd.set_flags(SB_SPLICE);
                mtx_leave(&sosp.so_snd.sb_mtx);
                mtx_leave(&so.so_rcv.sb_mtx);
            }

            sbunlock(&so.so_rcv);
            let _ = frele(fp, p);
            return Ok(());
        };
        // release:
        sounlock_pair(so, sosp);
        sbunlock(&sosp.so_snd);
        sbunlock(&so.so_rcv);
        error
    };
    // frele:
    let _ = frele(fp, p);
    error
}

/// `sounsplice(so, sosp, freeing)`: dissolves the splice of `so` into `sosp`, waking the
/// sockets that are not being freed (`SOSP_FREEING_*`).
pub fn sounsplice(so: &'static Socket, sosp: &'static Socket, freeing: i32) {
    sbassertlocked(&so.so_rcv);

    let sp = so_sp(so);
    let sosp_sp = so_sp(sosp);

    mtx_enter(&so.so_rcv.sb_mtx);
    mtx_enter(&sosp.so_snd.sb_mtx);
    so.so_rcv.clear_flags(SB_SPLICE);
    sosp.so_snd.clear_flags(SB_SPLICE);
    timeout_del(&sp.ssp_idleto);
    if let Some(q) = sp.ssp_queue.get() {
        task_del(q, &sp.ssp_task);
    }
    kassert!(sp.ssp_socket.get().is_some_and(|s| ptr::eq(s, sosp)));
    kassert!(sosp_sp.ssp_soback.get().is_some_and(|s| ptr::eq(s, so)));
    sp.ssp_socket.set(None);
    sosp_sp.ssp_soback.set(None);
    mtx_leave(&sosp.so_snd.sb_mtx);
    mtx_leave(&so.so_rcv.sb_mtx);

    // Do not wakeup a socket that is about to be freed.
    if freeing & SOSP_FREEING_READ == 0 {
        solock_shared(so);
        mtx_enter(&so.so_rcv.sb_mtx);
        let readable = so.so_qlen.get() != 0 || soreadable(so);
        mtx_leave(&so.so_rcv.sb_mtx);
        if readable {
            sorwakeup(so);
        }
        sounlock_shared(so);
    }
    if freeing & SOSP_FREEING_WRITE == 0 {
        solock_shared(sosp);
        if sowriteable(sosp) {
            sowwakeup(sosp);
        }
        sounlock_shared(sosp);
    }

    sorele(sosp);
    sorele(so);
}

/// `soidle(arg)`: the splice idle timeout of the socket `arg`: unsplice with `ETIMEDOUT`.
pub fn soidle(arg: *mut c_void) {
    // SAFETY: `sosplice_alloc` armed the timeout with its socket's address, and `soclose`
    // waits for it (`timeout_barrier`) before the socket can go.
    let so: &'static Socket = unsafe { &*arg.cast::<Socket>() };

    let _ = sblock(&so.so_rcv, SBL_WAIT | SBL_NOINTR);
    if so.so_rcv.has_flags(SB_SPLICE)
        && let Some(drain) = so_sp(so).ssp_socket.get()
    {
        so.set_error(Some(Errno::ETIMEDOUT));
        sounsplice(so, drain, 0);
    }
    sbunlock(&so.so_rcv);
}

/// `sotask(arg)`: the splice task of the socket `arg`: move what arrived.
pub fn sotask(arg: *mut c_void) {
    // SAFETY: as in `soidle`: the task's socket outlives it (`taskq_barrier` in `soclose`).
    let so: &'static Socket = unsafe { &*arg.cast::<Socket>() };

    let _ = sblock(&so.so_rcv, SBL_WAIT | SBL_NOINTR);
    if so.so_rcv.has_flags(SB_SPLICE) {
        let _ = somove(so, M_DONTWAIT);
    }
    sbunlock(&so.so_rcv);
}

/// Move data from receive buffer of spliced source socket to send buffer of drain socket.
/// Try to move as much as possible in one big chunk. It is a TCP only implementation.
/// Returns `false` when splicing has been finished, `true` to continue.
pub fn somove(so: &'static Socket, wait: i32) -> bool {
    let sp = so_sp(so);
    let Some(sosp) = sp.ssp_socket.get() else {
        panic(format_args!("somove: so {:p} not spliced", so));
    };
    let mut error: Option<Errno>;
    let mut maxreached = false;
    let mut unsplice = false;

    sbassertlocked(&so.so_rcv);

    if so.pr_flags(PR_WANTRCVD) {
        let _ = sblock(&so.so_snd, SBL_WAIT | SBL_NOINTR);
    }

    mtx_enter(&so.so_rcv.sb_mtx);
    mtx_enter(&sosp.so_snd.sb_mtx);

    // nextpkt:
    'release: loop {
        if let Some(e) = so.error() {
            error = Some(e);
            break 'release;
        }
        if sosp.so_snd.has_state(SS_CANTSENDMORE) {
            error = Some(Errno::EPIPE);
            break 'release;
        }

        error = sosp.error();
        if let Some(e) = error {
            if e != Errno::ETIMEDOUT && e != Errno::EFBIG && e != Errno::ELOOP {
                break 'release;
            }
            error = None;
        }
        if !sosp.has_state(SS_ISCONNECTED) {
            break 'release;
        }

        // Calculate how many bytes can be copied now.
        let mut len = so.so_rcv.sb_datacc.get();
        if sp.ssp_max.get() != 0 {
            kassert!(sp.ssp_len.get() < sp.ssp_max.get());
            if sp.ssp_max.get() <= sp.ssp_len.get() + len as Off {
                len = (sp.ssp_max.get() - sp.ssp_len.get()) as u64;
                maxreached = true;
            }
        }
        let mut space = sbspace_locked(&sosp.so_snd);
        let oobmark = so.so_oobmark.get();
        if oobmark != 0 && oobmark < len && oobmark < (space + 1024) as u64 {
            space += 1024;
        }
        if space <= 0 {
            maxreached = false;
            break 'release;
        }
        if (space as u64) < len {
            maxreached = false;
            if space < sosp.so_snd.sb_lowat.get() {
                break 'release;
            }
            len = space as u64;
        }
        sosp.so_snd.set_state(SS_ISSENDING);

        let Some(first) = so.so_rcv.sb_mb.get() else {
            break 'release;
        };
        let nextrecord = first.m_nextpkt().get();

        // Drop address and control information not used with splicing.
        let mut m = Some(first);
        if so.pr_flags(PR_ADDR) {
            if cfg!(feature = "diagnostic") && i32::from(first.m_type().get()) != MT_SONAME {
                panic(format_args!(
                    "somove soname: so {:p}, so_type {}, m {:p}, m_type {}",
                    so,
                    so.so_type.get(),
                    first,
                    first.m_type().get()
                ));
            }
            m = first.m_next().get();
        }
        while let Some(mm) = m
            && i32::from(mm.m_type().get()) == MT_CONTROL
        {
            m = mm.m_next().get();
        }
        let Some(data) = m else {
            sbdroprecord(&so.so_rcv);
            if so.pr_flags(PR_WANTRCVD) {
                mtx_leave(&sosp.so_snd.sb_mtx);
                mtx_leave(&so.so_rcv.sb_mtx);
                solock_shared(so);
                pru_rcvd(so);
                sounlock_shared(so);
                mtx_enter(&so.so_rcv.sb_mtx);
                mtx_enter(&sosp.so_snd.sb_mtx);
            }
            continue 'release; // goto nextpkt
        };

        // By splicing sockets connected to localhost, userland might create a loop. Dissolve
        // splicing with error if loop is detected by counter.
        let dflags = data.m_flags().get();
        if dflags & M_PKTHDR != 0
            && (data.m_pkthdr().ph_loopcnt.get() >= M_MAXLOOP
                || (dflags & M_LOOP != 0 && dflags & (M_BCAST | M_MCAST) != 0))
        {
            error = Some(Errno::ELOOP);
            break 'release;
        }

        if so.pr_flags(PR_ATOMIC) {
            if dflags & M_PKTHDR == 0 {
                panic(format_args!(
                    "somove !PKTHDR: so {:p}, so_type {}, m {:p}, m_type {}",
                    so,
                    so.so_type.get(),
                    data,
                    data.m_type().get()
                ));
            }
            let pktlen = data.m_pkthdr().len.get() as u64;
            if sosp.so_snd.sb_hiwat.get() < pktlen {
                error = Some(Errno::EMSGSIZE);
                break 'release;
            }
            if len < pktlen {
                break 'release;
            }
            if pktlen < len {
                maxreached = false;
                len = pktlen;
            }
            // Throw away the name mbuf after it has been assured that the whole first record
            // can be processed.
            if let Some(name) = so.so_rcv.sb_mb.get() {
                sbfree(&so.so_rcv, name);
                so.so_rcv.sb_mb.set(m_free(name));
                sbsync(&so.so_rcv, nextrecord);
            }
        }
        // Throw away the control mbufs after it has been assured that the whole first record
        // can be processed.
        let mut mm = so.so_rcv.sb_mb.get();
        while let Some(c) = mm
            && i32::from(c.m_type().get()) == MT_CONTROL
        {
            sbfree(&so.so_rcv, c);
            so.so_rcv.sb_mb.set(m_free(c));
            mm = so.so_rcv.sb_mb.get();
            sbsync(&so.so_rcv, nextrecord);
        }

        // Take at most len mbufs out of receive buffer: `mp` walks from the local chain head
        // (`at` None) down the m_next links (`at` the mbuf whose link it is).
        let mut head = mm;
        let mut at: Option<&'static Mbuf> = None;
        let link = |at: Option<&'static Mbuf>, head: &Option<&'static Mbuf>| match at {
            Some(a) => a.m_next().get(),
            None => *head,
        };
        let setlink = |at: Option<&'static Mbuf>,
                       head: &mut Option<&'static Mbuf>,
                       v: Option<&'static Mbuf>| match at {
            Some(a) => a.m_next().set(v),
            None => *head = v,
        };
        let mut off: u64 = 0;
        while off <= len
            && let Some(cur) = link(at, &head)
        {
            let size = len - off;

            if cfg!(feature = "diagnostic") {
                let t = i32::from(cur.m_type().get());
                if t != MT_DATA && t != MT_HEADER {
                    panic(format_args!(
                        "somove type: so {:p}, so_type {}, m {:p}, m_type {}",
                        so,
                        so.so_type.get(),
                        cur,
                        t
                    ));
                }
            }
            let taken = if u64::from(cur.m_len().get()) > size {
                // Move only a partial mbuf at maximum splice length or if the drain buffer is
                // too small for this large mbuf.
                if !maxreached && sosp.so_snd.sb_datacc.get() > 0 {
                    len -= size;
                    break;
                }
                if wait == M_WAIT {
                    mtx_leave(&sosp.so_snd.sb_mtx);
                    mtx_leave(&so.so_rcv.sb_mtx);
                }
                let copy = so
                    .so_rcv
                    .sb_mb
                    .get()
                    .and_then(|mb| m_copym(mb, 0, size as i32, wait));
                if wait == M_WAIT {
                    mtx_enter(&so.so_rcv.sb_mtx);
                    mtx_enter(&sosp.so_snd.sb_mtx);
                }
                setlink(at, &mut head, copy);
                let Some(copy) = copy else {
                    len -= size;
                    break;
                };
                if let Some(mb) = so.so_rcv.sb_mb.get() {
                    mb.m_data()
                        .set(mb.m_data().get().wrapping_add(size as usize));
                    mb.m_len().set(mb.m_len().get() - size as u32);
                }
                so.so_rcv.sb_cc.set(so.so_rcv.sb_cc.get() - size);
                so.so_rcv.sb_datacc.set(so.so_rcv.sb_datacc.get() - size);
                copy
            } else {
                let Some(mb) = so.so_rcv.sb_mb.get() else {
                    break;
                };
                setlink(at, &mut head, Some(mb));
                sbfree(&so.so_rcv, mb);
                so.so_rcv.sb_mb.set(mb.m_next().get());
                sbsync(&so.so_rcv, nextrecord);
                mb
            };
            off += u64::from(taken.m_len().get());
            at = Some(taken);
        }
        setlink(at, &mut head, None);

        let Some(mut m) = head else {
            break 'release;
        };
        m.m_nextpkt().set(None);
        if m.m_flags().get() & M_PKTHDR != 0 {
            m_resethdr(m);
            m.m_pkthdr().len.set(len as i32);
        }

        // Receive buffer did shrink by len bytes, adjust oob.
        let mut rcvstate = so.so_rcv.sb_state.get();
        so.so_rcv.clear_state(SS_RCVATMARK);
        let mut oobmark = so.so_oobmark.get();
        so.so_oobmark.set(oobmark.saturating_sub(len));
        if oobmark != 0 {
            if oobmark == len {
                so.so_rcv.set_state(SS_RCVATMARK);
            }
            if oobmark >= len {
                oobmark = 0;
            }
        }

        // Send window update to source peer as receive buffer has changed.
        if so.pr_flags(PR_WANTRCVD) {
            mtx_leave(&sosp.so_snd.sb_mtx);
            mtx_leave(&so.so_rcv.sb_mtx);
            solock_shared(so);
            pru_rcvd(so);
            sounlock_shared(so);
            mtx_enter(&so.so_rcv.sb_mtx);
            mtx_enter(&sosp.so_snd.sb_mtx);
        }

        // Handle oob data. If any malloc fails, ignore error. TCP urgent data is not very
        // reliable anyway.
        while (rcvstate & SS_RCVATMARK != 0 || oobmark != 0) && so.has_options(SO_OOBINLINE) {
            let mut o: Option<&'static Mbuf> = None;

            mtx_leave(&sosp.so_snd.sb_mtx);
            mtx_leave(&so.so_rcv.sb_mtx);

            if rcvstate & SS_RCVATMARK != 0 {
                o = m_get(wait, MT_DATA);
                rcvstate &= !SS_RCVATMARK;
            } else if oobmark != 0 {
                o = m_split(m, oobmark as i32, wait);
                if let Some(oo) = o {
                    if m.m_flags().get() & M_PKTHDR != 0 {
                        let lc = &m.m_pkthdr().ph_loopcnt;
                        lc.set(lc.get().wrapping_add(1));
                    }
                    solock_shared(sosp);
                    let e = pru_send(sosp, Some(m), None, None);
                    sounlock_shared(sosp);

                    if let Err(e) = e {
                        mtx_enter(&so.so_rcv.sb_mtx);
                        mtx_enter(&sosp.so_snd.sb_mtx);
                        error = Some(if sosp.so_snd.has_state(SS_CANTSENDMORE) {
                            Errno::EPIPE
                        } else {
                            e
                        });
                        m_freem(oo);
                        break 'release;
                    }
                    len -= oobmark;
                    sp.ssp_len.set(sp.ssp_len.get() + oobmark as Off);
                    m = oo;
                    o = m_get(wait, MT_DATA);
                }
                oobmark = 0;
            }
            if let Some(o) = o {
                o.m_len().set(1);
                // SAFETY: `o` is a fresh mbuf with room for a byte; `m` holds data.
                unsafe { *mtod::<u8>(o) = *mtod::<u8>(m) };

                solock_shared(sosp);
                let e = pru_sendoob(sosp, Some(o), None, None);
                sounlock_shared(sosp);

                if let Err(e) = e {
                    mtx_enter(&so.so_rcv.sb_mtx);
                    mtx_enter(&sosp.so_snd.sb_mtx);
                    error = Some(if sosp.so_snd.has_state(SS_CANTSENDMORE) {
                        Errno::EPIPE
                    } else {
                        e
                    });
                    m_freem(m);
                    break 'release;
                }
                len -= 1;
                sp.ssp_len.set(sp.ssp_len.get() + 1);
                if oobmark != 0 {
                    oobmark -= 1;
                    if oobmark == 0 {
                        rcvstate |= SS_RCVATMARK;
                    }
                }
                m_adj(m, 1);
            }

            mtx_enter(&so.so_rcv.sb_mtx);
            mtx_enter(&sosp.so_snd.sb_mtx);
        }

        // Append all remaining data to drain socket.
        if so.so_rcv.sb_cc.get() == 0 || maxreached {
            sosp.so_snd.clear_state(SS_ISSENDING);
        }

        mtx_leave(&sosp.so_snd.sb_mtx);
        mtx_leave(&so.so_rcv.sb_mtx);
        if m.m_flags().get() & M_PKTHDR != 0 {
            let lc = &m.m_pkthdr().ph_loopcnt;
            lc.set(lc.get().wrapping_add(1));
        }
        solock_shared(sosp);
        let e = pru_send(sosp, Some(m), None, None);
        sounlock_shared(sosp);
        mtx_enter(&so.so_rcv.sb_mtx);
        mtx_enter(&sosp.so_snd.sb_mtx);

        if let Err(e) = e {
            error = Some(
                if sosp.so_snd.has_state(SS_CANTSENDMORE) || sosp.so_pcb.get().is_null() {
                    Errno::EPIPE
                } else {
                    e
                },
            );
            break 'release;
        }
        sp.ssp_len.set(sp.ssp_len.get() + len as Off);

        // Move several packets if possible.
        if !maxreached && nextrecord.is_some() {
            continue 'release; // goto nextpkt
        }
        break 'release;
    }

    // release:
    sosp.so_snd.clear_state(SS_ISSENDING);

    if error.is_none() && maxreached && sp.ssp_max.get() == sp.ssp_len.get() {
        error = Some(Errno::EFBIG);
    }
    if error.is_some() {
        so.set_error(error);
    }

    if (so.so_rcv.has_state(SS_CANTRCVMORE) && so.so_rcv.sb_cc.get() == 0)
        || sosp.so_snd.has_state(SS_CANTSENDMORE)
        || maxreached
        || error.is_some()
    {
        unsplice = true;
    }

    mtx_leave(&sosp.so_snd.sb_mtx);
    mtx_leave(&so.so_rcv.sb_mtx);

    if so.pr_flags(PR_WANTRCVD) {
        sbunlock(&so.so_snd);
    }

    if unsplice {
        sounsplice(so, sosp, 0);
        return false;
    }
    let idletv = sp.ssp_idletv.get();
    if idletv.is_set() {
        timeout_add_nsec(&sp.ssp_idleto, timeval_to_nsec(&idletv));
    }
    true
}

/// `sorwakeup(so)`: data arrived on `so`: run a splice's task, or wake the readers and the
/// upcall.
pub fn sorwakeup(so: &'static Socket) {
    if so.pr_flags(PR_SPLICE) {
        mtx_enter(&so.so_rcv.sb_mtx);
        if so.so_rcv.has_flags(SB_SPLICE) {
            let sp = so_sp(so);
            if let Some(q) = sp.ssp_queue.get() {
                task_add(q, &sp.ssp_task);
            }
        }
        if isspliced(so) {
            mtx_leave(&so.so_rcv.sb_mtx);
            return;
        }
        mtx_leave(&so.so_rcv.sb_mtx);
    }
    sowakeup(so, &so.so_rcv);
    if let Some(upcall) = so.so_upcall.get() {
        upcall(so, so.so_upcallarg.get(), M_DONTWAIT);
    }
}

/// `sowwakeup(so)`: space appeared on `so`: run the source's splice task, or wake the
/// writers.
pub fn sowwakeup(so: &'static Socket) {
    if so.pr_flags(PR_SPLICE) {
        mtx_enter(&so.so_snd.sb_mtx);
        if so.so_snd.has_flags(SB_SPLICE)
            && let Some(back) = so_sp(so).ssp_soback.get()
        {
            let bsp = so_sp(back);
            if let Some(q) = bsp.ssp_queue.get() {
                task_add(q, &bsp.ssp_task);
            }
        }
        if issplicedback(so) {
            mtx_leave(&so.so_snd.sb_mtx);
            return;
        }
        mtx_leave(&so.so_snd.sb_mtx);
    }
    sowakeup(so, &so.so_snd);
}

/// `sosetopt(so, level, optname, m)`: sets a socket option (`SOL_SOCKET`) or hands it to
/// the protocol; `m` holds the value.
pub fn sosetopt(
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let mlen = |m: Option<&Mbuf>| m.map(|m| m.m_len().get() as usize);

    if level != SOL_SOCKET {
        if let Some(ctloutput) = so.so_proto.pr_ctloutput {
            solock(so);
            let error = ctloutput(PRCO_SETOPT, so, level, optname, m);
            sounlock(so);
            return error;
        }
        return Err(Errno::ENOPROTOOPT);
    }

    match optname {
        SO_LINGER => {
            let Some(m) = m.filter(|m| m.m_len().get() as usize == size_of::<Linger>()) else {
                return Err(Errno::EINVAL);
            };
            // SAFETY: the mbuf holds a whole `struct linger` (checked); maybe unaligned.
            let l = unsafe { mtod::<Linger>(m).read_unaligned() };
            if l.l_linger < 0 || l.l_linger > i32::from(SHRT_MAX) {
                return Err(Errno::EINVAL);
            }

            solock(so);
            so.so_linger.set(l.l_linger as i16);
            if mtod_int(m) != 0 {
                so.so_options.set(so.so_options.get() | optname);
            } else {
                so.so_options.set(so.so_options.get() & !optname);
            }
            sounlock(so);

            Ok(())
        }
        SO_BINDANY | SO_DEBUG | SO_KEEPALIVE | SO_USELOOPBACK | SO_BROADCAST | SO_REUSEADDR
        | SO_REUSEPORT | SO_OOBINLINE | SO_TIMESTAMP | SO_ZEROIZE => {
            if optname == SO_BINDANY {
                suser(curproc_or_panic("sosetopt"))?; // XXX
            }
            let Some(m) = m.filter(|_| mlen(m).is_some_and(|l| l >= size_of::<i32>())) else {
                return Err(Errno::EINVAL);
            };

            solock(so);
            if mtod_int(m) != 0 {
                so.so_options.set(so.so_options.get() | optname);
            } else {
                so.so_options.set(so.so_options.get() & !optname);
            }
            sounlock(so);

            Ok(())
        }
        SO_DONTROUTE => {
            let Some(m) = m.filter(|_| mlen(m).is_some_and(|l| l >= size_of::<i32>())) else {
                return Err(Errno::EINVAL);
            };
            if mtod_int(m) != 0 {
                return Err(Errno::EOPNOTSUPP);
            }
            Ok(())
        }
        SO_SNDBUF | SO_RCVBUF | SO_SNDLOWAT | SO_RCVLOWAT => {
            let sb = if optname == SO_SNDBUF || optname == SO_SNDLOWAT {
                &so.so_snd
            } else {
                &so.so_rcv
            };

            let Some(m) = m.filter(|_| mlen(m).is_some_and(|l| l >= size_of::<i32>())) else {
                return Err(Errno::EINVAL);
            };
            let mut cnt = mtod_int(m) as i64 as u64;
            if (cnt as i64) <= 0 {
                cnt = 1;
            }

            let mut error = Ok(());
            mtx_enter(&sb.sb_mtx);
            match optname {
                SO_SNDBUF | SO_RCVBUF => {
                    if sb.has_state(SS_CANTSENDMORE | SS_CANTRCVMORE) {
                        error = Err(Errno::EINVAL);
                    } else if sbcheckreserve(cnt, sb.sb_wat.get()).is_err()
                        || sbreserve(sb, cnt).is_err()
                    {
                        error = Err(Errno::ENOBUFS);
                    } else {
                        sb.sb_wat.set(cnt);
                    }
                }
                _ => {
                    sb.sb_lowat.set(if cnt > sb.sb_hiwat.get() {
                        sb.sb_hiwat.get() as i64
                    } else {
                        cnt as i64
                    });
                }
            }
            mtx_leave(&sb.sb_mtx);

            error
        }
        SO_SNDTIMEO | SO_RCVTIMEO => {
            let sb = if optname == SO_SNDTIMEO {
                &so.so_snd
            } else {
                &so.so_rcv
            };

            let Some(m) = m.filter(|_| mlen(m).is_some_and(|l| l >= size_of::<Timeval>())) else {
                return Err(Errno::EINVAL);
            };
            // SAFETY: the mbuf holds a whole `struct timeval` (checked); maybe unaligned.
            let tv = unsafe { mtod::<Timeval>(m).read_unaligned() };
            if !tv.is_valid() {
                return Err(Errno::EINVAL);
            }
            let mut nsecs = timeval_to_nsec(&tv);
            if nsecs == u64::MAX {
                return Err(Errno::EDOM);
            }
            if nsecs == 0 {
                nsecs = INFSLP;
            }

            mtx_enter(&sb.sb_mtx);
            sb.sb_timeo_nsecs.set(nsecs);
            mtx_leave(&sb.sb_mtx);
            Ok(())
        }
        SO_RTABLE => {
            let dom = so.so_proto.pr_domain;
            match (dom.dom_protosw.first(), so.so_proto.pr_ctloutput) {
                (Some(first), Some(ctloutput)) => {
                    let level = i32::from(first.pr_protocol);
                    solock(so);
                    let error = ctloutput(PRCO_SETOPT, so, level, optname, m);
                    sounlock(so);
                    error
                }
                _ => Err(Errno::ENOPROTOOPT),
            }
        }
        SO_SPLICE => match m {
            None => sosplice(so, -1, 0, None),
            Some(m) if (m.m_len().get() as usize) < size_of::<i32>() => Err(Errno::EINVAL),
            Some(m) if (m.m_len().get() as usize) < size_of::<Splice>() => {
                sosplice(so, mtod_int(m), 0, None)
            }
            Some(m) => {
                // SAFETY: the mbuf holds a whole `struct splice` (checked); maybe unaligned.
                let sp = unsafe { mtod::<Splice>(m).read_unaligned() };
                sosplice(so, sp.sp_fd, sp.sp_max, Some(&sp.sp_idle))
            }
        },
        _ => Err(Errno::ENOPROTOOPT),
    }
}

/// `sogetopt(so, level, optname, m)`: reads a socket option (`SOL_SOCKET`) into `m`, or asks
/// the protocol.
pub fn sogetopt(
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: &'static Mbuf,
) -> Result<(), Errno> {
    if level != SOL_SOCKET {
        let Some(ctloutput) = so.so_proto.pr_ctloutput else {
            return Err(Errno::ENOPROTOOPT);
        };
        m.m_len().set(0);

        solock(so);
        let error = ctloutput(PRCO_GETOPT, so, level, optname, Some(m));
        sounlock(so);
        return error;
    }

    m.m_len().set(size_of::<i32>() as u32);

    match optname {
        SO_LINGER => {
            m.m_len().set(size_of::<Linger>() as u32);
            solock_shared(so);
            let l = Linger {
                l_onoff: so.so_options.get() & SO_LINGER,
                l_linger: i32::from(so.so_linger.get()),
            };
            sounlock_shared(so);
            // SAFETY: an mbuf's storage holds `MLEN` bytes, more than a `struct linger`.
            unsafe { mtod::<Linger>(m).write_unaligned(l) };
        }
        SO_BINDANY | SO_USELOOPBACK | SO_DEBUG | SO_KEEPALIVE | SO_REUSEADDR | SO_REUSEPORT
        | SO_BROADCAST | SO_OOBINLINE | SO_ACCEPTCONN | SO_TIMESTAMP | SO_ZEROIZE => {
            set_mtod_int(m, so.so_options.get() & optname);
        }
        SO_DONTROUTE => set_mtod_int(m, 0),
        SO_TYPE => set_mtod_int(m, so.so_type.get()),
        SO_ERROR => {
            solock(so);
            set_mtod_int(m, so.so_error.load(Ordering::Relaxed) as i32);
            so.set_error(None);
            sounlock(so);
        }
        SO_DOMAIN => set_mtod_int(m, so.dom_family()),
        SO_PROTOCOL => set_mtod_int(m, i32::from(so.so_proto.pr_protocol)),
        SO_SNDBUF => set_mtod_int(m, so.so_snd.sb_hiwat.get() as i32),
        SO_RCVBUF => set_mtod_int(m, so.so_rcv.sb_hiwat.get() as i32),
        SO_SNDLOWAT => set_mtod_int(m, so.so_snd.sb_lowat.get() as i32),
        SO_RCVLOWAT => set_mtod_int(m, so.so_rcv.sb_lowat.get() as i32),
        SO_SNDTIMEO | SO_RCVTIMEO => {
            let sb = if optname == SO_SNDTIMEO {
                &so.so_snd
            } else {
                &so.so_rcv
            };

            mtx_enter(&sb.sb_mtx);
            let nsecs = sb.sb_timeo_nsecs.get();
            mtx_leave(&sb.sb_mtx);

            m.m_len().set(size_of::<Timeval>() as u32);
            let mut tv = Timeval::new(0, 0);
            if nsecs != INFSLP {
                tv = nsec_to_timeval(nsecs);
            }
            // SAFETY: an mbuf's storage holds `MLEN` bytes, more than a `struct timeval`.
            unsafe { mtod::<Timeval>(m).write_unaligned(tv) };
        }
        SO_RTABLE => {
            let dom = so.so_proto.pr_domain;
            match (dom.dom_protosw.first(), so.so_proto.pr_ctloutput) {
                (Some(first), Some(ctloutput)) => {
                    let level = i32::from(first.pr_protocol);
                    solock(so);
                    let error = ctloutput(PRCO_GETOPT, so, level, optname, Some(m));
                    sounlock(so);
                    error?;
                }
                _ => return Err(Errno::ENOPROTOOPT),
            }
        }
        SO_SPLICE => {
            m.m_len().set(size_of::<Off>() as u32);
            solock_shared(so);
            let len: Off = so.so_sp.get().map_or(0, |sp| sp.ssp_len.get());
            sounlock_shared(so);
            // SAFETY: an mbuf's storage holds `MLEN` bytes, more than an `off_t`.
            unsafe { mtod::<Off>(m).write_unaligned(len) };
        }
        SO_PEERCRED => {
            if i32::from(so.so_proto.pr_protocol) != i32::from(AF_UNIX) {
                return Err(Errno::EOPNOTSUPP);
            }
            let unp = sotounpcb(so);

            solock(so);
            if let Some(unp) = unp
                && unp.has_flags(UNP_FEIDS)
            {
                m.m_len().set(size_of::<Sockpeercred>() as u32);
                // SAFETY: an mbuf's storage holds `MLEN` bytes, more than the credentials.
                unsafe { mtod::<Sockpeercred>(m).write_unaligned(unp.unp_connid.get()) };
                sounlock(so);
                return Ok(());
            }
            sounlock(so);

            return Err(Errno::ENOTCONN);
        }
        _ => return Err(Errno::ENOPROTOOPT),
    }
    Ok(())
}

/// `sohasoutofband(so)`: out-of-band data arrived: `SIGURG` and the except knotes.
pub fn sohasoutofband(so: &Socket) {
    pgsigio(&so.so_sigio, SIGURG, false);
    // knote(&so->so_rcv.sb_klist, 0): kern_event.c.
    let _ = unported!("sohasoutofband: knote (kern_event.c)");
}

/// `fo_kqfilter` of a socket: attaches a read, write or except knote.
pub fn soo_kqfilter(_fp: &File, _kn: &Knote) -> Result<(), Errno> {
    // switch (kn->kn_filter): EVFILT_READ gets soread_filtops on so_rcv, EVFILT_WRITE
    // sowrite_filtops on so_snd, EVFILT_EXCEPT soexcept_filtops on so_rcv, others EINVAL;
    // then klist_insert(&sb->sb_klist, kn): struct knote (kern_event.c).
    Err(unported!("soo_kqfilter: struct knote (kern_event.c)"))
}

/// `filt_sordetach(kn)`: unhooks the knote from `so_rcv`'s list.
pub fn filt_sordetach(_so: &Socket) {
    // klist_remove(&so->so_rcv.sb_klist, kn): kern_event.c.
    let _ = unported!("filt_sordetach: klist_remove (kern_event.c)");
}

/// `filt_soread(kn, hint)` for the socket of the knote's file: readable data, or a listener
/// with connections to accept.
pub fn filt_soread(so: &Socket, kn: &mut SoKnote, _hint: i64) -> bool {
    let state = so.so_state.get();
    let error = so.so_error.load(Ordering::Relaxed);

    crate::sys::mutex::mutex_assert_locked(&so.so_rcv.sb_mtx, "filt_soread");

    if so.has_options(SO_ACCEPTCONN) {
        let qlen = so.so_qlen.get();

        soassertlocked_readonly(so);

        kn.kn_data = i64::from(qlen);
        let mut rv = kn.kn_data != 0;

        if kn.kn_flags & (__EV_POLL | __EV_SELECT) != 0 {
            if state & SS_ISDISCONNECTED != 0 {
                kn.kn_flags |= __EV_HUP;
                rv = true;
            } else {
                rv = qlen != 0 || soreadable(so);
            }
        }

        return rv;
    }

    kn.kn_data = so.so_rcv.sb_cc.get() as i64;
    if isspliced(so) {
        false
    } else if so.so_rcv.has_state(SS_CANTRCVMORE) {
        kn.kn_flags |= EV_EOF;
        if kn.kn_flags & __EV_POLL != 0 && state & SS_ISDISCONNECTED != 0 {
            kn.kn_flags |= __EV_HUP;
        }
        kn.kn_fflags = error;
        true
    } else if error != 0 {
        true
    } else if kn.kn_sfflags & NOTE_LOWAT != 0 {
        kn.kn_data >= kn.kn_sdata
    } else {
        kn.kn_data >= so.so_rcv.sb_lowat.get()
    }
}

/// `filt_sowdetach(kn)`: unhooks the knote from `so_snd`'s list.
pub fn filt_sowdetach(_so: &Socket) {
    // klist_remove(&so->so_snd.sb_klist, kn): kern_event.c.
    let _ = unported!("filt_sowdetach: klist_remove (kern_event.c)");
}

/// `filt_sowrite(kn, hint)` for the socket of the knote's file: room to send.
pub fn filt_sowrite(so: &Socket, kn: &mut SoKnote, _hint: i64) -> bool {
    let state = so.so_state.get();
    let error = so.so_error.load(Ordering::Relaxed);

    crate::sys::mutex::mutex_assert_locked(&so.so_snd.sb_mtx, "filt_sowrite");

    kn.kn_data = sbspace_locked(&so.so_snd);
    if so.so_snd.has_state(SS_CANTSENDMORE) {
        kn.kn_flags |= EV_EOF;
        if kn.kn_flags & __EV_POLL != 0 && state & SS_ISDISCONNECTED != 0 {
            kn.kn_flags |= __EV_HUP;
        }
        kn.kn_fflags = error;
        true
    } else if error != 0 {
        true
    } else if state & SS_ISCONNECTED == 0 && so.pr_flags(PR_CONNREQUIRED) {
        false
    } else if kn.kn_sfflags & NOTE_LOWAT != 0 {
        kn.kn_data >= kn.kn_sdata
    } else {
        kn.kn_data >= so.so_snd.sb_lowat.get()
    }
}

/// `filt_soexcept(kn, hint)` for the socket of the knote's file: out-of-band data, or a
/// hang-up for poll.
pub fn filt_soexcept(so: &Socket, kn: &mut SoKnote, _hint: i64) -> bool {
    let mut rv = false;

    crate::sys::mutex::mutex_assert_locked(&so.so_rcv.sb_mtx, "filt_soexcept");

    if isspliced(so) {
        rv = false;
    } else if kn.kn_sfflags & NOTE_OOB != 0
        && (so.so_oobmark.get() != 0 || so.so_rcv.has_state(SS_RCVATMARK))
    {
        kn.kn_fflags |= NOTE_OOB;
        kn.kn_data -= so.so_oobmark.get() as i64;
        rv = true;
    }

    if kn.kn_flags & __EV_POLL != 0 {
        let state = so.so_state.get();

        if state & SS_ISDISCONNECTED != 0 {
            kn.kn_flags |= __EV_HUP;
            rv = true;
        }
    }

    rv
}

/// `filt_sowmodify(kev, kn)`: `knote_modify` under `so_snd`'s mutex.
pub fn filt_sowmodify(so: &Socket) -> bool {
    mtx_enter(&so.so_snd.sb_mtx);
    let _ = unported!("filt_sowmodify: knote_modify (kern_event.c)");
    mtx_leave(&so.so_snd.sb_mtx);

    false
}

/// `filt_sowprocess(kn, kev)`: `knote_process` under `so_snd`'s mutex.
pub fn filt_sowprocess(so: &Socket) -> bool {
    mtx_enter(&so.so_snd.sb_mtx);
    let _ = unported!("filt_sowprocess: knote_process (kern_event.c)");
    mtx_leave(&so.so_snd.sb_mtx);

    false
}

/// `filt_sormodify(kev, kn)`: `knote_modify` under `so_rcv`'s mutex (and the socket lock
/// for a protocol that wants `PRU_RCVD`).
pub fn filt_sormodify(so: &Socket) -> bool {
    if so.pr_flags(PR_WANTRCVD) {
        solock_shared(so);
    }
    mtx_enter(&so.so_rcv.sb_mtx);
    let _ = unported!("filt_sormodify: knote_modify (kern_event.c)");
    mtx_leave(&so.so_rcv.sb_mtx);
    if so.pr_flags(PR_WANTRCVD) {
        sounlock_shared(so);
    }

    false
}

/// `filt_sorprocess(kn, kev)`: `knote_process` under `so_rcv`'s mutex (and the socket lock
/// for a protocol that wants `PRU_RCVD`).
pub fn filt_sorprocess(so: &Socket) -> bool {
    if so.pr_flags(PR_WANTRCVD) {
        solock_shared(so);
    }
    mtx_enter(&so.so_rcv.sb_mtx);
    let _ = unported!("filt_sorprocess: knote_process (kern_event.c)");
    mtx_leave(&so.so_rcv.sb_mtx);
    if so.pr_flags(PR_WANTRCVD) {
        sounlock_shared(so);
    }

    false
}

/// `filt_soemodify(kev, kn)`: `knote_modify` under `so_rcv`'s mutex.
pub fn filt_soemodify(so: &Socket) -> bool {
    mtx_enter(&so.so_rcv.sb_mtx);
    let _ = unported!("filt_soemodify: knote_modify (kern_event.c)");
    mtx_leave(&so.so_rcv.sb_mtx);

    false
}

/// `filt_soeprocess(kn, kev)`: `knote_process` under `so_rcv`'s mutex.
pub fn filt_soeprocess(so: &Socket) -> bool {
    mtx_enter(&so.so_rcv.sb_mtx);
    let _ = unported!("filt_soeprocess: knote_process (kern_event.c)");
    mtx_leave(&so.so_rcv.sb_mtx);

    false
}

/// An optional mbuf as a pointer, for the printers.
fn mptr(m: Option<&Mbuf>) -> *const Mbuf {
    m.map_or(ptr::null(), ptr::from_ref)
}

/// `sobuf_print(sb, pr)` (`DDB`).
pub fn sobuf_print(sb: &Sockbuf, pr: PrFn) {
    pr(format_args!("\tsb_cc: {}\n", sb.sb_cc.get()));
    pr(format_args!("\tsb_datacc: {}\n", sb.sb_datacc.get()));
    pr(format_args!("\tsb_hiwat: {}\n", sb.sb_hiwat.get()));
    pr(format_args!("\tsb_wat: {}\n", sb.sb_wat.get()));
    pr(format_args!("\tsb_mbcnt: {}\n", sb.sb_mbcnt.get()));
    pr(format_args!("\tsb_mbmax: {}\n", sb.sb_mbmax.get()));
    pr(format_args!("\tsb_lowat: {}\n", sb.sb_lowat.get()));
    pr(format_args!("\tsb_mb: {:p}\n", mptr(sb.sb_mb.get())));
    pr(format_args!(
        "\tsb_mbtail: {:p}\n",
        mptr(sb.sb_mbtail.get())
    ));
    pr(format_args!(
        "\tsb_lastrecord: {:p}\n",
        mptr(sb.sb_lastrecord.get())
    ));
    pr(format_args!("\tsb_flags: {:04x}\n", sb.sb_flags.get()));
    pr(format_args!("\tsb_state: {:04x}\n", sb.sb_state.get()));
    pr(format_args!(
        "\tsb_timeo_nsecs: {}\n",
        sb.sb_timeo_nsecs.get()
    ));
}

/// `so_print(v, pr)` (`DDB`): the socket's state.
pub fn so_print(so: &Socket, pr: PrFn) {
    let sock = |s: Option<&'static Socket>| s.map_or(ptr::null(), ptr::from_ref);

    pr(format_args!("socket {:p}\n", so));
    pr(format_args!("so_type: {}\n", so.so_type.get()));
    pr(format_args!("so_options: 0x{:04x}\n", so.so_options.get())); // %b
    pr(format_args!("so_linger: {}\n", so.so_linger.get()));
    pr(format_args!("so_state: 0x{:04x}\n", so.so_state.get()));
    pr(format_args!("so_pcb: {:p}\n", so.so_pcb.get()));
    pr(format_args!("so_proto: {:p}\n", so.so_proto));
    pr(format_args!(
        "so_sigio: {:p}\n",
        so.so_sigio.sir_sigio.get()
    ));

    pr(format_args!("so_head: {:p}\n", sock(so.so_head.get())));
    pr(format_args!("so_onq: {:p}\n", so.so_onq.get()));
    pr(format_args!(
        "so_q0: @{:p} first: {:p}\n",
        &so.so_q0,
        sock(soq_first(&so.so_q0))
    ));
    pr(format_args!(
        "so_q: @{:p} first: {:p}\n",
        &so.so_q,
        sock(soq_first(&so.so_q))
    ));
    pr(format_args!(
        "so_eq: next: {:p}\n",
        SoqHead::next(so).map_or(ptr::null(), ptr::from_ref)
    ));
    pr(format_args!("so_q0len: {}\n", so.so_q0len.get()));
    pr(format_args!("so_qlen: {}\n", so.so_qlen.get()));
    pr(format_args!("so_qlimit: {}\n", so.so_qlimit.get()));
    pr(format_args!("so_timeo: {}\n", so.so_timeo.get()));
    pr(format_args!("so_obmark: {}\n", so.so_oobmark.get()));

    pr(format_args!(
        "so_sp: {:p}\n",
        so.so_sp.get().map_or(ptr::null(), ptr::from_ref)
    ));
    if let Some(sp) = so.so_sp.get() {
        let tv = sp.ssp_idletv.get();
        pr(format_args!(
            "\tssp_socket: {:p}\n",
            sock(sp.ssp_socket.get())
        ));
        pr(format_args!(
            "\tssp_soback: {:p}\n",
            sock(sp.ssp_soback.get())
        ));
        pr(format_args!("\tssp_len: {}\n", sp.ssp_len.get()));
        pr(format_args!("\tssp_max: {}\n", sp.ssp_max.get()));
        pr(format_args!("\tssp_idletv: {} {}\n", tv.tv_sec, tv.tv_usec));
        pr(format_args!(
            "\tssp_idleto: {}pending (@{})\n",
            if crate::sys::timeout::timeout_pending(&sp.ssp_idleto) {
                ""
            } else {
                "not "
            },
            sp.ssp_idleto.to_time.get()
        ));
    }

    pr(format_args!("so_rcv:\n"));
    sobuf_print(&so.so_rcv, pr);
    pr(format_args!("so_snd:\n"));
    sobuf_print(&so.so_snd, pr);

    pr(format_args!(
        "so_upcall: {:p} so_upcallarg: {:p}\n",
        so.so_upcall.get().map_or(ptr::null(), |f| f as *const ()),
        so.so_upcallarg.get()
    ));

    pr(format_args!(
        "so_euid: {} so_ruid: {}\n",
        so.so_euid.get(),
        so.so_ruid.get()
    ));
    pr(format_args!(
        "so_egid: {} so_rgid: {}\n",
        so.so_egid.get(),
        so.so_rgid.get()
    ));
    pr(format_args!("so_cpid: {}\n", so.so_cpid.get()));
}
