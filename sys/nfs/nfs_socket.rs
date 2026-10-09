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
 * Copyright (c) 1989, 1991, 1993, 1995
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Rick Macklem at The University of Guelph.
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
 *	@(#)nfs_socket.c	8.5 (Berkeley) 3/30/95
 */
/* </LICENSES> */

/* <CODE> */
//! `nfs/nfs_socket.c`: socket operations for use by NFS. The client's RPC engine (connect and
//! reconnect, send, receive, match replies to requests, the retransmit timer, the congestion
//! window and the RTT estimator) and the server's socket side (the socket upcall, the record
//! marking of stream sockets, parsing an RPC request, the reply header, waking an nfsd).
//!
//! Upstream: sys/nfs/nfs_socket.c @ 3ce1f3f79392
//!
//! The file sits under `option NFSCLIENT` or `option NFSSERVER` (`sys/conf/files`); inside it
//! `nfs_receive`, `nfs_reply` and `nfs_request` are `#ifdef NFSCLIENT` (feature `nfsclient`)
//! and `nfsrv_rcv`, `nfsrv_getstream`, `nfsrv_dorec` and `nfsrv_wakenfsd` `#ifdef NFSSERVER`
//! (feature `nfsserver`), as in the C. Requests, mounts and server sockets are the
//! `&'static` `Cell` objects of `nfs.rs` and `nfsmount.rs`; mbufs follow `docs/C_TO_RUST.md`
//! and the reply cursor conventions of `nfsm_subs.rs`.
//!
//! ## Deviations
//! - Errors are `Result<_, Errno>`. `nfs_sigintr` returns `Err(EINTR)` for the C's nonzero;
//!   `nfs_rcvlock`'s "reply already received" is `Err(EALREADY)`, as in C. A reply status that
//!   is no errno (version 3's `NFSERR_BADHANDLE` ... `NFSERR_JUKEBOX`, 10001 and up) comes out
//!   of `nfs_request` as `EIO`, the only fallback `Errno` allows; the C hands the raw number
//!   up.
//! - `nfs_sndlock`/`nfs_sndunlock`/`nfs_rcvunlock` take the flag word as `&Cell<i32>` (the
//!   C's `int *`: `&nmp->nm_flag` or `&slp->ns_solock`); its address is the sleep channel.
//! - `MGET`/`m_copym`/`M_PREPEND`/`pool_get` with `M_WAIT`/`PR_WAITOK` can fail here (the
//!   pools cannot sleep, `subr_pool.rs`): `nfs_connect`, `nfs_receive`, `nfs_request`,
//!   `nfs_rephead` and `nfsrv_dorec` return `ENOBUFS` (freeing what the C would have
//!   consumed); a failed first send copy in `nfs_request` marks the request `R_MUSTRESEND`
//!   like a send without a socket; `nfs_realign` leaves the chain as it is when it cannot get
//!   an mbuf (the dissection views of `nfsm_subs.rs` copy bytes, so a misaligned chain is
//!   still read correctly), and ignores `m_copyback`'s error, as the C does.
//! - `nfs_rephead` returns the reply and its build cursor (`*mrq`, `*mbp`) instead of
//!   storing them, and builds the six header words with `nfsm_build` (same bytes, same
//!   `m_len`). `nfsrv_dorec` returns the descriptor (`*ndp`). `nfs_getreq` takes
//!   `has_header` as a `bool`.
//! - `nfs_request` frees the reply of a denied RPC (`MSG_DENIED`), which the C leaks.
//!   `nfsrv_rcv` frees a partial chain or address that `soreceive` hands back with an error
//!   (the C leaks them), and queues a datagram that comes without an address as a record of
//!   its own (the C dereferences the NULL address); `nfsrv_dorec` takes such a record as it
//!   takes one without `MT_SONAME`.
//! - `nfs_reply`'s, `nfs_timer`'s and `nfs_msg`'s `struct proc *` is the request's
//!   `r_procp`, read back as `Option<&Proc>`.
//! - `nfsrv_dorec` takes its descriptors from `nfs_syscalls.rs`'s `NFSRV_DESCRIPT_PL`.

use core::cell::Cell;
use core::cmp::{max, min};
use core::ffi::c_void;
use core::ptr;
#[cfg(feature = "nfsserver")]
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use crate::kern::init_main::PROC0;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
#[cfg(feature = "nfsserver")]
use crate::kern::kern_synch::wakeup_one;
use crate::kern::kern_synch::{nowake, refcnt_init, tsleep_nsec, wakeup};
use crate::kern::kern_timeout::timeout_add;
#[cfg(feature = "nfsclient")]
use crate::kern::kern_timeout::timeout_del;
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::{Str, panic, tprintf, tprintf_close, tprintf_open};
use crate::kern::uipc_mbuf::{
    MAX_HDR, m_adj, m_copyback, m_copym, m_freem, m_get, m_gethdr, m_trailingspace,
};
#[cfg(feature = "nfsclient")]
use crate::kern::uipc_mbuf::{m_calchdrlen, m_prepend};
use crate::kern::uipc_socket::{
    sobind, soclose, soconnect, socreate, soreceive, sosend, sosetopt, soshutdown,
};
use crate::kern::uipc_socket2::{solock_shared, soreserve, sosleep_nsec, sounlock_shared};
#[cfg(feature = "nfsclient")]
use crate::kern::vfs_cache::cache_purge;
#[cfg(feature = "nfsclient")]
use crate::log;
#[cfg(feature = "nfsclient")]
use crate::machine::cpu::curproc;
use crate::netinet::in_::{
    INADDR_ANY, IP_PORTRANGE, IP_PORTRANGE_DEFAULT, IP_PORTRANGE_LOW, IPPROTO_IP, IPPROTO_TCP,
    InAddr, SockaddrIn,
};
use crate::netinet::tcp::TCP_NODELAY;
use crate::nfs::nfs::{
    ND_NFSV3, NFS_DEFAULT_TIMER, NFS_GETATTR_TIMER, NFS_LOOKUP_TIMER, NFS_MAXREXMIT,
    NFS_READ_TIMER, NFS_WRITE_TIMER, NFSINT_SIGMASK, NfsReq, Nfsd, NfsrvDescript, NfssvcSock,
    R_MUSTRESEND, R_SENT, R_SOFTTERM, R_TIMING, R_TPRINTFMSG, nfs_initrtt, nfs_maxrto, nfs_minrto,
    nfsignore_soerror,
};
#[cfg(feature = "nfsclient")]
use crate::nfs::nfs::{NFS_TIMEOUTMUL, nfs_maxtimeo, nfs_mintimeo};
#[cfg(feature = "nfsserver")]
use crate::nfs::nfs::{
    NFSD_CHECKSLP, NFSD_HEAD, NFSD_HEAD_FLAG, NFSD_WAITING, SLP_DISCONN, SLP_DOREC, SLP_GETSTREAM,
    SLP_LASTFRAG, SLP_NEEDQ, SLP_VALID,
};
#[cfg(feature = "nfsclient")]
use crate::nfs::nfs_subs::NFSREQPL;
use crate::nfs::nfs_subs::{
    NFS_TICKS, NFSSTATS, NFSV3_PROCID, nfs_prog, nfsm_build, rpc_auth_unix, rpc_autherr, rpc_call,
    rpc_mismatch, rpc_msgaccepted, rpc_msgdenied, rpc_reply, rpc_vers,
};
#[cfg(feature = "nfsserver")]
use crate::nfs::nfs_syscalls::NFSRV_DESCRIPT_PL;
#[cfg(feature = "nfsclient")]
use crate::nfs::nfsm_subs::{NfsmInfo, nfsm_adv, nfsm_dissect};
use crate::nfs::nfsm_subs::{XdrIn, nfsd_adv, nfsd_dissect, nfsm_rndup};
use crate::nfs::nfsmount::NfsMount;
#[cfg(feature = "nfsclient")]
use crate::nfs::nfsmount::VFSTONFS;
#[cfg(feature = "nfsclient")]
use crate::nfs::nfsproto::NFSERR_TRYLATER;
use crate::nfs::nfsproto::{
    NFS_MAXNAMLEN, NFS_MAXPACKET, NFS_MAXPKTHDR, NFS_NPROCS, NFS_VER2, NFS_VER3, NFSERR_AUTHERR,
    NFSERR_RETVOID, NFSPROC_COMMIT, NFSPROC_NOOP, NFSPROC_NULL, NFSV2PROC_STATFS, NFSX_UNSIGNED,
};
#[cfg(feature = "nfsclient")]
use crate::nfs::rpcv2::RPCAUTH_UNIX;
use crate::nfs::rpcv2::{
    AUTH_REJECTCRED, RPC_GARBAGE, RPC_PROCUNAVAIL, RPC_PROGMISMATCH, RPC_PROGUNAVAIL, RPC_REPLYSIZ,
    RPC_VER2, RPCAUTH_MAXSIZ, RPCAUTH_UNIXGIDS,
};
use crate::nfs::xdr_subs::{fxdr_unsigned, txdr_unsigned};
use crate::sys::errno::Errno;
#[cfg(feature = "nfsclient")]
use crate::sys::mbuf::m_freemp;
use crate::sys::mbuf::{
    M_COPYALL, M_DONTWAIT, M_WAIT, MHLEN, MINCLSIZE, MT_DATA, MT_SONAME, MT_SOOPTS, Mbuf, mclget,
    mtod,
};
#[cfg(feature = "nfsclient")]
use crate::sys::mount::NFSMNT_NFSV3;
use crate::sys::mount::{
    MNAMELEN, NFSMNT_DUMBTIMR, NFSMNT_INT, NFSMNT_NOCONN, NFSMNT_RCVLOCK, NFSMNT_SNDLOCK,
    NFSMNT_SOFT, NFSMNT_WANTRCV, NFSMNT_WANTSND,
};
use crate::sys::param::{PCATCH, PSOCK, PZERO, align, aligned_pointer};
use crate::sys::pool::PR_WAITOK;
use crate::sys::proc::Proc;
use crate::sys::protosw::{PR_CONNREQUIRED, pru_send};
use crate::sys::signalvar::sigpending;
#[cfg(feature = "nfsserver")]
use crate::sys::socket::MSG_DONTWAIT;
use crate::sys::socket::{AF_INET, SHUT_RDWR, SO_KEEPALIVE, SOCK_DGRAM, SOCK_STREAM, SOL_SOCKET};
#[cfg(feature = "nfsclient")]
use crate::sys::socket::{MSG_EOR, MSG_WAITALL};
use crate::sys::socketvar::{SB_NOINTR, SS_ISCONNECTED, SS_ISCONNECTING, Socket, sbspace};
use crate::sys::syslimits::NGROUPS_MAX;
#[cfg(feature = "nfsclient")]
use crate::sys::syslog::{LOG_ERR, LOG_INFO};
use crate::sys::systm::{INFSLP, net_lock, net_unlock};
#[cfg(feature = "nfsserver")]
use crate::sys::systm::{kernel_lock, kernel_unlock};
use crate::sys::time::sec_to_nsec;
use crate::sys::ucred::Ucred;
#[cfg(feature = "nfsclient")]
use crate::sys::uio::Iovec;
use crate::sys::uio::{Uio, UioRw, UioSeg};
#[cfg(feature = "nfsclient")]
use crate::sys::vnode::Vnode;

/// `NFS_CWNDSCALE`: the scale of the congestion window and of the sent count. There is a
/// congestion window for outstanding rpcs maintained per mount point. The cwnd size is
/// adjusted in roughly the way that: Van Jacobson, Congestion avoidance and Control, In
/// "Proceedings of SIGCOMM '88". ACM, August 1988. describes for TCP. The cwnd size is chopped
/// in half on a retransmit timeout and incremented by 1/cwnd when each rpc reply is received
/// and a full cwnd of rpcs is in progress. (The sent count and cwnd are scaled for integer
/// arith.) Variants of "slow start" were tried and were found to be too much of a performance
/// hit (ave. rtt 3 times larger), I suspect due to the large rtt that nfs rpcs have.
pub const NFS_CWNDSCALE: i32 = 256;

/// `NFS_MAXCWND`: the largest congestion window.
pub const NFS_MAXCWND: i32 = NFS_CWNDSCALE * 32;

/// `nfs_backoff`: the retransmit timeout multipliers, by number of timeouts in a row.
const NFS_BACKOFF: [i32; 8] = [2, 4, 8, 16, 32, 64, 128, 256];

/// `nfs_ptimers`: the RTT estimator of each procedure.
const NFS_PTIMERS: [usize; NFS_NPROCS] = [
    NFS_DEFAULT_TIMER, // NULL
    NFS_GETATTR_TIMER, // GETATTR
    NFS_DEFAULT_TIMER, // SETATTR
    NFS_LOOKUP_TIMER,  // LOOKUP
    NFS_GETATTR_TIMER, // ACCESS
    NFS_READ_TIMER,    // READLINK
    NFS_READ_TIMER,    // READ
    NFS_WRITE_TIMER,   // WRITE
    NFS_DEFAULT_TIMER, // CREATE
    NFS_DEFAULT_TIMER, // MKDIR
    NFS_DEFAULT_TIMER, // SYMLINK
    NFS_DEFAULT_TIMER, // MKNOD
    NFS_DEFAULT_TIMER, // REMOVE
    NFS_DEFAULT_TIMER, // RMDIR
    NFS_DEFAULT_TIMER, // RENAME
    NFS_DEFAULT_TIMER, // LINK
    NFS_READ_TIMER,    // READDIR
    NFS_READ_TIMER,    // READDIRPLUS
    NFS_DEFAULT_TIMER, // FSSTAT
    NFS_DEFAULT_TIMER, // FSINFO
    NFS_DEFAULT_TIMER, // PATHCONF
    NFS_DEFAULT_TIMER, // COMMIT
    NFS_DEFAULT_TIMER, // NOOP
];

/// `nfs_realign_test`: calls of `nfs_realign`.
pub static NFS_REALIGN_TEST: AtomicU32 = AtomicU32::new(0);

/// `nfs_realign_count`: chains `nfs_realign` had to copy.
pub static NFS_REALIGN_COUNT: AtomicU32 = AtomicU32::new(0);

/// `rep->r_nmp`: the mount of a request, set by `nfs_request` before anything else reads it.
fn r_nmp(rep: &NfsReq) -> &'static NfsMount {
    match rep.r_nmp.get() {
        Some(nmp) => nmp,
        None => panic(format_args!("nfs: request {:p} has no mount", rep)),
    }
}

/// `rep->r_procp`: the thread that made the request, `None` for none.
fn r_procp(rep: &NfsReq) -> Option<&Proc> {
    // SAFETY: `nfs_request` stores the requesting thread (or null) and that thread sleeps in
    // `nfs_reply` until the request is unlinked, so the pointer is null or alive while
    // anybody reaches the request.
    unsafe { rep.r_procp.get().as_ref() }
}

/// `nmp->nm_mountp->mnt_stat.f_mntfromname` for the messages.
fn nm_mntfromname(nmp: &NfsMount) -> [u8; MNAMELEN] {
    nmp.nm_mountp
        .get()
        .map_or([0; MNAMELEN], |mp| mp.mnt_stat.get().f_mntfromname)
}

/// `*mtod(m, int *) = v; m->m_len = sizeof(int)`: an option value for `sosetopt`.
fn set_mtod_int(m: &Mbuf, v: i32) {
    // SAFETY: an mbuf's storage holds `MLEN` bytes at least; the data may be unaligned.
    unsafe { mtod::<i32>(m).write_unaligned(v) };
    m.m_len().set(size_of::<i32>() as u32);
}

/// `nfs_init_rtt(nmp)`: initialize the RTT estimator state for a new mount point.
pub fn nfs_init_rtt(nmp: &NfsMount) {
    for srtt in &nmp.nm_srtt {
        srtt.set(nfs_initrtt());
    }
    for sdrtt in &nmp.nm_sdrtt {
        sdrtt.set(0);
    }
}

/// `nfs_update_rtt(rep)`: update a mount point's RTT estimator state using data from the
/// passed-in request.
///
/// Use a gain of 0.125 on the mean and a gain of 0.25 on the deviation.
///
/// NB: Since the timer resolution of `NFS_HZ` is so coarse, it can often result in
/// `r_rtt == 0`. Since `r_rtt == N` means that the actual RTT is between N + dt and N + 2 - dt
/// ticks, add 1 before calculating the update values.
pub fn nfs_update_rtt(rep: &NfsReq) {
    let mut t1 = rep.r_rtt.get() + 1;
    let index = NFS_PTIMERS[rep.r_procnum.get()] - 1;
    let nmp = r_nmp(rep);
    let srtt = &nmp.nm_srtt[index];
    let sdrtt = &nmp.nm_sdrtt[index];

    t1 -= srtt.get() >> 3;
    srtt.set(srtt.get() + t1);
    if t1 < 0 {
        t1 = -t1;
    }
    t1 -= sdrtt.get() >> 2;
    sdrtt.set(sdrtt.get() + t1);
}

/// `nfs_estimate_rto(nmp, procnum)`: estimate RTO for an NFS RPC sent via an unreliable
/// datagram.
///
/// Use the mean and mean deviation of RTT for the appropriate type of RPC for the frequent
/// RPCs and a default for the others. The justification for doing "other" this way is that
/// these RPCs happen so infrequently that timer est. would probably be stale. Also, since many
/// of these RPCs are non-idempotent, a conservative timeout is desired.
///
/// getattr, lookup - A+2D; read, write - A+4D; other - `nm_timeo`.
pub fn nfs_estimate_rto(nmp: &NfsMount, procnum: usize) -> i32 {
    let timer = NFS_PTIMERS[procnum];
    let rto = match timer {
        NFS_GETATTR_TIMER | NFS_LOOKUP_TIMER => {
            let index = timer - 1;
            ((nmp.nm_srtt[index].get() + 3) >> 2) + ((nmp.nm_sdrtt[index].get() + 1) >> 1)
        }
        NFS_READ_TIMER | NFS_WRITE_TIMER => {
            let index = timer - 1;
            ((nmp.nm_srtt[index].get() + 7) >> 3) + (nmp.nm_sdrtt[index].get() + 1)
        }
        _ => return nmp.nm_timeo.get(),
    };

    if rto < nfs_minrto() {
        nfs_minrto()
    } else if rto > nfs_maxrto() {
        nfs_maxrto()
    } else {
        rto
    }
}

/// `nfs_connect(nmp, rep)`: initialize sockets and congestion for a new NFS connection. We do
/// not free the sockaddr if error.
pub fn nfs_connect(nmp: &'static NfsMount, rep: Option<&NfsReq>) -> Result<(), Errno> {
    let sotype = nmp.nm_sotype.get();
    if !(sotype == SOCK_DGRAM || sotype == SOCK_STREAM) {
        return Err(Errno::EINVAL);
    }

    nmp.nm_so.set(None);
    // saddr = mtod(nmp->nm_nam, struct sockaddr *): only its family is needed.
    let family = nmp.nm_nam.get().map_or(0, |nam| {
        if (nam.m_len().get() as usize) < 2 {
            0
        } else {
            // SAFETY: `sa_family` is the second byte of the `sockaddr` in the name mbuf's
            // data, which holds at least two bytes (checked above).
            unsafe { mtod::<u8>(nam).add(1).read() }
        }
    });
    let so = match socreate(i32::from(family), sotype, nmp.nm_soproto.get()) {
        Ok(so) => so,
        Err(error) => {
            nfs_disconnect(nmp);
            return Err(error);
        }
    };
    nmp.nm_so.set(Some(so));

    // Allocate mbufs possibly waiting before grabbing the socket lock.
    let mut mopt = None;
    let mut nam = None;
    let alloc = 'alloc: {
        if sotype == SOCK_STREAM || family == AF_INET {
            mopt = m_get(M_WAIT, MT_SOOPTS);
            if mopt.is_none() {
                break 'alloc Err(Errno::ENOBUFS);
            }
        }
        if family == AF_INET {
            nam = m_get(M_WAIT, MT_SONAME);
            if nam.is_none() {
                break 'alloc Err(Errno::ENOBUFS);
            }
        }
        Ok(())
    };

    let error = alloc.and_then(|()| nfs_connect_so(nmp, so, rep, family, mopt, nam));

    m_freem(mopt);
    m_freem(nam);

    match error {
        Ok(()) => {
            // Initialize other non-zero congestion variables.
            nfs_init_rtt(nmp);
            nmp.nm_cwnd.set(NFS_MAXCWND / 2); // Initial send window
            nmp.nm_sent.set(0);
            nmp.nm_timeouts.set(0);
            Ok(())
        }
        Err(error) => {
            nfs_disconnect(nmp);
            Err(error)
        }
    }
}

/// The part of `nfs_connect` between the allocations and the `bad:` label: binds a reserved
/// port, connects, sets the timeouts and the buffer sizes. Returns with the socket unlocked.
fn nfs_connect_so(
    nmp: &'static NfsMount,
    so: &'static Socket,
    rep: Option<&NfsReq>,
    family: u8,
    mopt: Option<&'static Mbuf>,
    nam: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    nmp.nm_soflags.set(i32::from(so.so_proto.pr_flags));

    // Some servers require that the client port be a reserved port number. We always
    // allocate a reserved port, as this prevents filehandle disclosure through UDP port
    // capture.
    if family == AF_INET
        && let (Some(mopt), Some(nam)) = (mopt, nam)
    {
        set_mtod_int(mopt, IP_PORTRANGE_LOW);
        sosetopt(so, IPPROTO_IP, IP_PORTRANGE, Some(mopt))?;

        let sin = SockaddrIn {
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_family: AF_INET,
            sin_port: 0, // htons(0)
            sin_addr: InAddr { s_addr: INADDR_ANY },
            ..SockaddrIn::default()
        };
        // SAFETY: a fresh mbuf's data area holds far more than a `sockaddr_in`; the data may
        // be unaligned.
        unsafe { mtod::<SockaddrIn>(nam).write_unaligned(sin) };
        nam.m_len().set(u32::from(sin.sin_len));
        solock_shared(so);
        let error = sobind(so, nam, &PROC0);
        sounlock_shared(so);
        error?;

        set_mtod_int(mopt, IP_PORTRANGE_DEFAULT);
        sosetopt(so, IPPROTO_IP, IP_PORTRANGE, Some(mopt))?;
    }

    // Protocols that do not require connections may be optionally left unconnected for
    // servers that reply from a port other than NFS_PORT.
    if nmp.nm_flag.get() & NFSMNT_NOCONN != 0 {
        if nmp.nm_soflags.get() & i32::from(PR_CONNREQUIRED) != 0 {
            return Err(Errno::ENOTCONN);
        }
    } else {
        solock_shared(so);
        let connected = nfs_connect_wait(nmp, so, rep);
        sounlock_shared(so);
        connected?;
    }

    // Always set receive timeout to detect server crash and reconnect. Otherwise, we can get
    // stuck in soreceive forever.
    mtx_enter(&so.so_rcv.sb_mtx);
    so.so_rcv.sb_timeo_nsecs.set(sec_to_nsec(5));
    mtx_leave(&so.so_rcv.sb_mtx);
    mtx_enter(&so.so_snd.sb_mtx);
    if nmp.nm_flag.get() & (NFSMNT_SOFT | NFSMNT_INT) != 0 {
        so.so_snd.sb_timeo_nsecs.set(sec_to_nsec(5));
    } else {
        so.so_snd.sb_timeo_nsecs.set(INFSLP);
    }
    mtx_leave(&so.so_snd.sb_mtx);

    let (sndreserve, rcvreserve) = match nmp.nm_sotype.get() {
        SOCK_DGRAM => {
            let snd = nmp.nm_wsize.get() as usize + NFS_MAXPKTHDR;
            let rcv =
                (max(nmp.nm_rsize.get(), nmp.nm_readdirsize.get()) as usize + NFS_MAXPKTHDR) * 2;
            (snd, rcv)
        }
        SOCK_STREAM => {
            if let Some(mopt) = mopt {
                if so.so_proto.pr_flags & PR_CONNREQUIRED != 0 {
                    set_mtod_int(mopt, 1);
                    let _ = sosetopt(so, SOL_SOCKET, SO_KEEPALIVE, Some(mopt));
                }
                if i32::from(so.so_proto.pr_protocol) == IPPROTO_TCP {
                    set_mtod_int(mopt, 1);
                    let _ = sosetopt(so, IPPROTO_TCP, TCP_NODELAY, Some(mopt));
                }
            }
            let snd = (nmp.nm_wsize.get() as usize + NFS_MAXPKTHDR + size_of::<u32>()) * 2;
            let rcv = (nmp.nm_rsize.get() as usize + NFS_MAXPKTHDR + size_of::<u32>()) * 2;
            (snd, rcv)
        }
        t => panic(format_args!("nfs_connect: nm_sotype {}", t)),
    };
    solock_shared(so);
    let error = soreserve(so, sndreserve as u64, rcvreserve as u64);
    if error.is_ok() {
        mtx_enter(&so.so_rcv.sb_mtx);
        so.so_rcv.sb_flags.set(so.so_rcv.sb_flags.get() | SB_NOINTR);
        mtx_leave(&so.so_rcv.sb_mtx);
        mtx_enter(&so.so_snd.sb_mtx);
        so.so_snd.sb_flags.set(so.so_snd.sb_flags.get() | SB_NOINTR);
        mtx_leave(&so.so_snd.sb_mtx);
    }
    sounlock_shared(so);
    error
}

/// `soconnect` and the wait for the connection to complete, with the socket locked: cribbed
/// from the connect system call but with the wait timing out so that interruptible mounts
/// don't hang here for a long time.
fn nfs_connect_wait(
    nmp: &'static NfsMount,
    so: &'static Socket,
    rep: Option<&NfsReq>,
) -> Result<(), Errno> {
    let Some(srv) = nmp.nm_nam.get() else {
        panic(format_args!("nfs_connect: no server address"));
    };
    soconnect(so, srv)?;

    while so.has_state(SS_ISCONNECTING) && so.error().is_none() {
        let _ = sosleep_nsec(so, so.timeo_chan(), PSOCK, "nfscon", sec_to_nsec(2));
        if so.has_state(SS_ISCONNECTING)
            && so.error().is_none()
            && let Some(rep) = rep
            && let Err(error) = nfs_sigintr(nmp, Some(rep), r_procp(rep))
        {
            so.so_state.set(so.so_state.get() & !SS_ISCONNECTING);
            return Err(error);
        }
    }
    if let Some(error) = so.error() {
        so.set_error(None);
        return Err(error);
    }
    Ok(())
}

/// `nfs_reconnect(rep)`: called when a connection is broken on a reliable protocol: clean up
/// the old socket, `nfs_connect()` again, set `R_MUSTRESEND` for all outstanding requests on
/// mount point. If this fails the mount point is DEAD! nb: Must be called with the
/// `nfs_sndlock()` set on the mount point.
pub fn nfs_reconnect(rep: &NfsReq) -> Result<(), Errno> {
    let nmp = r_nmp(rep);

    nfs_disconnect(nmp);
    loop {
        match nfs_connect(nmp, Some(rep)) {
            Ok(()) => break,
            Err(Errno::EINTR | Errno::ERESTART) => return Err(Errno::EINTR),
            Err(_) => {
                let _ = tsleep_nsec(nowake(), PSOCK, "nfsrecon", sec_to_nsec(1));
            }
        }
    }

    // Loop through outstanding request list and fix up all requests on old socket.
    for rp in nmp.nm_reqsq.iter() {
        rp.r_flags.set(rp.r_flags.get() | R_MUSTRESEND);
        rp.r_rexmit.set(0);
    }
    Ok(())
}

/// `nfs_disconnect(nmp)`: NFS disconnect. Clean up and unlink.
pub fn nfs_disconnect(nmp: &NfsMount) {
    if let Some(so) = nmp.nm_so.take() {
        let _ = soshutdown(so, SHUT_RDWR);
        let _ = soclose(so, 0);
    }
}

/// `nfs_send(so, nam, top, rep)`: this is the nfs send routine. For connection based socket
/// types, it must be called with an `nfs_sndlock()` on the socket. `rep == None` indicates
/// that it has been called from a server.
///
/// For the client side: return `EINTR` if the RPC is terminated, 0 otherwise; set
/// `R_MUSTRESEND` if the send fails for any reason; do any cleanup required by recoverable
/// socket errors (???).
///
/// For the server side: return `EINTR` or `ERESTART` if interrupted by a signal; return
/// `EPIPE` if a connection is lost for connection based sockets (TCP...); do any cleanup
/// required by recoverable socket errors (???).
pub fn nfs_send(
    so: &'static Socket,
    nam: Option<&'static Mbuf>,
    top: &'static Mbuf,
    rep: Option<&NfsReq>,
) -> Result<(), Errno> {
    let (so, soflags) = match rep {
        Some(rep) => {
            if rep.r_flags.get() & R_SOFTTERM != 0 {
                m_freem(top);
                return Err(Errno::EINTR);
            }
            let nmp = r_nmp(rep);
            let Some(so) = nmp.nm_so.get() else {
                rep.r_flags.set(rep.r_flags.get() | R_MUSTRESEND);
                m_freem(top);
                return Ok(());
            };
            rep.r_flags.set(rep.r_flags.get() & !R_MUSTRESEND);
            (so, nmp.nm_soflags.get())
        }
        None => (so, i32::from(so.so_proto.pr_flags)),
    };
    let sendnam = if soflags & i32::from(PR_CONNREQUIRED) != 0 || so.has_state(SS_ISCONNECTED) {
        None
    } else {
        nam
    };
    let flags = 0;

    let Err(mut error) = sosend(so, sendnam, None, Some(top), None, flags) else {
        return Ok(());
    };
    if let Some(rep) = rep {
        // Deal with errors for the client side.
        if rep.r_flags.get() & R_SOFTTERM != 0 {
            error = Errno::EINTR;
        } else {
            rep.r_flags.set(rep.r_flags.get() | R_MUSTRESEND);
        }
    }

    // Handle any recoverable (soft) socket errors here. (???)
    if error != Errno::EINTR
        && error != Errno::ERESTART
        && error != Errno::EWOULDBLOCK
        && error != Errno::EPIPE
    {
        return Ok(());
    }
    Err(error)
}

/// `nfs_receive(rep, aname, mp)`: receive a Sun RPC Request/Reply. For `SOCK_DGRAM`, the
/// work is all done by `soreceive()`, but for `SOCK_STREAM` we must deal with the Record Mark
/// and consolidate the data into a new mbuf list. nb: Sometimes TCP passes the data up to
/// `soreceive()` in long lists of small mbufs. For `SOCK_STREAM` we must be very careful to
/// read an entire record once we have read any of it, even if the system call has been
/// interrupted.
#[cfg(feature = "nfsclient")]
pub fn nfs_receive(
    rep: &NfsReq,
    aname: &mut Option<&'static Mbuf>,
    mp: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let p = curproc(); // XXX

    // Set up arguments for soreceive().
    *mp = None;
    *aname = None;
    let nmp = r_nmp(rep);
    let sotype = nmp.nm_sotype.get();

    let mut error: Result<(), Errno>;
    // For reliable protocols, lock against other senders/receivers in case a reconnect is
    // necessary. For SOCK_STREAM, first get the Record Mark to find out how much more there
    // is to get. We must lock the socket against other receivers until we have an entire rpc
    // request/reply.
    if sotype != SOCK_DGRAM {
        nfs_sndlock(&nmp.nm_flag, Some(rep))?;
        'tryagain: loop {
            // Check for fatal errors and resending request.
            //
            // Ugh: If a reconnect attempt just happened, nm_so would have changed. NULL
            // indicates a failed attempt that has essentially shut down this mount point.
            if rep.r_mrep.get().is_some() || rep.r_flags.get() & R_SOFTTERM != 0 {
                nfs_sndunlock(&nmp.nm_flag);
                return Err(Errno::EINTR);
            }
            let Some(so) = nmp.nm_so.get() else {
                if let Err(e) = nfs_reconnect(rep) {
                    nfs_sndunlock(&nmp.nm_flag);
                    return Err(e);
                }
                continue 'tryagain;
            };
            while rep.r_flags.get() & R_MUSTRESEND != 0 {
                let Some(m) = rep
                    .r_mreq
                    .get()
                    .and_then(|r| m_copym(r, 0, M_COPYALL, M_WAIT))
                else {
                    nfs_sndunlock(&nmp.nm_flag);
                    return Err(Errno::ENOBUFS);
                };
                NFSSTATS.rpcretries.fetch_add(1, Relaxed);
                rep.r_rtt.set(0);
                rep.r_flags.set(rep.r_flags.get() & !R_TIMING);
                if let Err(e) = nfs_send(so, nmp.nm_nam.get(), m, Some(rep)) {
                    if e == Errno::EINTR || e == Errno::ERESTART {
                        nfs_sndunlock(&nmp.nm_flag);
                        return Err(e);
                    }
                    if let Err(e) = nfs_reconnect(rep) {
                        nfs_sndunlock(&nmp.nm_flag);
                        return Err(e);
                    }
                    continue 'tryagain;
                }
            }
            nfs_sndunlock(&nmp.nm_flag);
            error = 'errout: {
                if sotype == SOCK_STREAM {
                    let mut len_buf = [0u8; NFSX_UNSIGNED];
                    let mut aio = [Iovec {
                        iov_base: len_buf.as_mut_ptr().cast(),
                        iov_len: NFSX_UNSIGNED,
                    }];
                    let mut auio = Uio {
                        uio_iov: &mut aio,
                        uio_offset: 0,
                        uio_resid: NFSX_UNSIGNED,
                        uio_segflg: UioSeg::UIO_SYSSPACE,
                        uio_rw: UioRw::UIO_READ,
                        uio_procp: p,
                    };
                    let mut error;
                    loop {
                        let mut rcvflg = MSG_WAITALL;
                        error = soreceive(so, None, &mut auio, None, None, Some(&mut rcvflg), 0);
                        if error == Err(Errno::EWOULDBLOCK) {
                            if rep.r_flags.get() & R_SOFTTERM != 0 {
                                return Err(Errno::EINTR);
                            }
                            // looks like the server died after it received the request,
                            // make sure that we will retransmit and we don't get stuck here
                            // forever.
                            if rep.r_rexmit.get() >= nmp.nm_retry.get() {
                                NFSSTATS.rpctimeouts.fetch_add(1, Relaxed);
                                error = Err(Errno::EPIPE);
                            }
                        }
                        if error != Err(Errno::EWOULDBLOCK) {
                            break;
                        }
                    }
                    if error.is_ok() && auio.uio_resid > 0 {
                        log!(
                            LOG_INFO,
                            "short receive ({}/{}) from nfs server {}\n",
                            NFSX_UNSIGNED - auio.uio_resid,
                            NFSX_UNSIGNED,
                            Str(&nm_mntfromname(nmp))
                        );
                        error = Err(Errno::EPIPE);
                    }
                    if error.is_err() {
                        break 'errout error;
                    }

                    let len = u32::from_be_bytes(len_buf) & !0x8000_0000;
                    // This is SERIOUS! We are out of sync with the sender and forcing a
                    // disconnect/reconnect is all I can do.
                    if len as usize > NFS_MAXPACKET {
                        log!(
                            LOG_ERR,
                            "{} ({}) from nfs server {}\n",
                            "impossible packet length",
                            len,
                            Str(&nm_mntfromname(nmp))
                        );
                        break 'errout Err(Errno::EFBIG);
                    }
                    auio.uio_resid = len as usize;
                    loop {
                        let mut rcvflg = MSG_WAITALL;
                        error = soreceive(
                            so,
                            None,
                            &mut auio,
                            Some(&mut *mp),
                            None,
                            Some(&mut rcvflg),
                            0,
                        );
                        if !matches!(
                            error,
                            Err(Errno::EWOULDBLOCK | Errno::EINTR | Errno::ERESTART)
                        ) {
                            break;
                        }
                    }
                    if error.is_ok() && auio.uio_resid > 0 {
                        log!(
                            LOG_INFO,
                            "short receive ({}/{}) from nfs server {}\n",
                            len as usize - auio.uio_resid,
                            len,
                            Str(&nm_mntfromname(nmp))
                        );
                        error = Err(Errno::EPIPE);
                    }
                    error
                } else {
                    // NB: Since uio_resid is big, MSG_WAITALL is ignored and soreceive() will
                    // return when it has either a control msg or a data msg. We have no use
                    // for control msg., but must grab them and then throw them away so we know
                    // what is going on.
                    let mut auio = Uio {
                        uio_iov: &mut [],
                        uio_offset: 0,
                        uio_resid: 100_000_000, // Anything Big
                        uio_segflg: UioSeg::UIO_SYSSPACE,
                        uio_rw: UioRw::UIO_READ,
                        uio_procp: p,
                    };
                    let mut error;
                    let mut rcvflg;
                    loop {
                        rcvflg = 0;
                        let mut control = None;
                        error = soreceive(
                            so,
                            None,
                            &mut auio,
                            Some(&mut *mp),
                            Some(&mut control),
                            Some(&mut rcvflg),
                            0,
                        );
                        let had_control = control.is_some();
                        m_freem(control);
                        if error == Err(Errno::EWOULDBLOCK) && rep.r_flags.get() & R_SOFTTERM != 0 {
                            return Err(Errno::EINTR);
                        }
                        if !(error == Err(Errno::EWOULDBLOCK)
                            || (error.is_ok() && mp.is_none() && had_control))
                        {
                            break;
                        }
                    }
                    if rcvflg & MSG_EOR == 0 {
                        crate::kprintf!("Egad!!\n");
                    }
                    if error.is_ok() && mp.is_none() {
                        error = Err(Errno::EPIPE);
                    }
                    error
                }
            };
            // errout:
            if let Err(e) = error
                && e != Errno::EINTR
                && e != Errno::ERESTART
            {
                m_freemp(mp);
                if e != Errno::EPIPE {
                    log!(
                        LOG_INFO,
                        "receive error {} from nfs server {}\n",
                        e.as_i32(),
                        Str(&nm_mntfromname(nmp))
                    );
                }
                error = nfs_sndlock(&nmp.nm_flag, Some(rep));
                if error.is_ok() {
                    error = nfs_reconnect(rep);
                    if error.is_ok() {
                        continue 'tryagain;
                    }
                    nfs_sndunlock(&nmp.nm_flag);
                }
            }
            break;
        }
    } else {
        let Some(so) = nmp.nm_so.get() else {
            return Err(Errno::EACCES);
        };
        let getnam = !so.has_state(SS_ISCONNECTED);
        let mut auio = Uio {
            uio_iov: &mut [],
            uio_offset: 0,
            uio_resid: 1_000_000,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: p,
        };
        loop {
            let mut rcvflg = 0;
            let paddr = if getnam { Some(&mut *aname) } else { None };
            error = soreceive(
                so,
                paddr,
                &mut auio,
                Some(&mut *mp),
                None,
                Some(&mut rcvflg),
                0,
            );
            if error == Err(Errno::EWOULDBLOCK) && rep.r_flags.get() & R_SOFTTERM != 0 {
                return Err(Errno::EINTR);
            }
            if error != Err(Errno::EWOULDBLOCK) {
                break;
            }
        }
    }
    if error.is_err() {
        m_freemp(mp);
    }
    // Search for any mbufs that are not a multiple of 4 bytes long or with m_data not
    // longword aligned. These could cause pointer alignment problems, so copy them to well
    // aligned mbufs.
    nfs_realign(mp, 5 * NFSX_UNSIGNED);
    error
}

/// `nfs_reply(myrep)`: implement receipt of reply on a socket. We must search through the
/// list of received datagrams matching them with outstanding requests using the xid, until
/// ours is found.
#[cfg(feature = "nfsclient")]
pub fn nfs_reply(myrep: &NfsReq) -> Result<(), Errno> {
    let nmp = r_nmp(myrep);

    // Loop around until we get our own reply.
    loop {
        // Lock against other receivers so that I don't get stuck in sbwait() after someone
        // else has received my reply for me. Also necessary for connection based protocols
        // to avoid race conditions during a reconnect.
        match nfs_rcvlock(myrep) {
            Ok(()) => {}
            Err(Errno::EALREADY) => return Ok(()),
            Err(error) => return Err(error),
        }

        // Get the next Rpc reply off the socket.
        let mut nam = None;
        let mut info = NfsmInfo::new();
        let received = nfs_receive(myrep, &mut nam, &mut info.nmi_mrep);
        nfs_rcvunlock(&nmp.nm_flag);
        if let Err(error) = received {
            // Ignore routing errors on connectionless protocols??
            if nfsignore_soerror(nmp.nm_soflags.get(), error) {
                if let Some(so) = nmp.nm_so.get() {
                    so.set_error(None);
                }
                continue;
            }
            return Err(error);
        }
        m_freem(nam);

        // Get the xid and check that it is an rpc reply.
        info.dissect_from(info.nmi_mrep);
        let Ok(tl) = nfsm_dissect(&mut info, 2 * NFSX_UNSIGNED) else {
            continue; // nfsmout
        };
        let rxid = tl.get(0);
        if tl.get(1) != rpc_reply {
            NFSSTATS.rpcinvalid.fetch_add(1, Relaxed);
            m_freem(info.nmi_mrep.take());
            continue; // nfsmout
        }

        // Loop through the request list to match up the reply. Iff no match, just drop the
        // datagram.
        let mut found = None;
        for rep in nmp.nm_reqsq.iter() {
            if rep.r_mrep.get().is_none() && rxid == rep.r_xid.get() {
                // Found it..
                rep.r_mrep.set(info.nmi_mrep);
                rep.r_md.set(info.nmi_md);
                rep.r_dpos.set(info.nmi_dpos);

                // Update congestion window. Do the additive increase of one rpc/rtt.
                if nmp.nm_cwnd.get() <= nmp.nm_sent.get() {
                    let cwnd = nmp.nm_cwnd.get();
                    let cwnd = cwnd + (NFS_CWNDSCALE * NFS_CWNDSCALE + (cwnd >> 1)) / cwnd;
                    nmp.nm_cwnd.set(min(cwnd, NFS_MAXCWND));
                }
                rep.r_flags.set(rep.r_flags.get() & !R_SENT);
                nmp.nm_sent.set(nmp.nm_sent.get() - NFS_CWNDSCALE);

                if rep.r_flags.get() & R_TIMING != 0 {
                    nfs_update_rtt(rep);
                }

                nmp.nm_timeouts.set(0);
                found = Some(rep);
                break;
            }
        }
        // If not matched to a request, drop it. If it's mine, get out.
        match found {
            None => {
                NFSSTATS.rpcunexpected.fetch_add(1, Relaxed);
                m_freem(info.nmi_mrep.take());
            }
            Some(rep) if ptr::eq(rep, myrep) => {
                if rep.r_mrep.get().is_none() {
                    panic(format_args!("nfsreply nil"));
                }
                return Ok(());
            }
            Some(_) => {}
        }
    }
}

/// An NFS status from a reply as the `Errno` `nfs_request` returns: the status itself when it
/// is an errno number (every version 2 status is), `EIO` otherwise.
#[cfg(feature = "nfsclient")]
fn nfs_status_errno(status: i32) -> Errno {
    Errno::from_raw(status).unwrap_or(Errno::EIO)
}

/// `nfs_request(vp, procnum, infop)`: goes something like this: fill in request struct;
/// links it into list; calls `nfs_send()` for first transmit; calls `nfs_receive()` to get
/// reply; break down rpc header and return with nfs reply pointed to by `mrep` or error.
/// nb: always frees up mreq mbuf list.
///
/// On success and on an NFS error status the reply and its cursor are in
/// `infop.nmi_mrep`/`nmi_md`/`nmi_dpos` (the caller frees the reply); on other errors
/// `infop.nmi_mrep` is `None`.
#[cfg(feature = "nfsclient")]
pub fn nfs_request(
    vp: &'static Vnode,
    procnum: usize,
    infop: &mut NfsmInfo<'_>,
) -> Result<(), Errno> {
    let Some(item) = pool_get(&NFSREQPL, PR_WAITOK) else {
        m_freem(infop.nmi_mreq.take());
        return Err(Errno::ENOBUFS);
    };
    let rp = item.cast::<NfsReq>().as_ptr();
    // SAFETY: a fresh `nfsreqpl` item, sized and aligned for a `struct nfsreq` by `nfs_init`;
    // it stays put until the `pool_put` below, after it has left `nm_reqsq`.
    let rep: &'static NfsReq = unsafe {
        rp.write(NfsReq::new());
        &*rp
    };
    let Some(mount) = vp.v_mount.get() else {
        panic(format_args!("nfs_request: vnode {:p} has no mount", vp));
    };
    rep.r_nmp.set(Some(VFSTONFS(mount)));
    rep.r_vp.set(Some(vp));
    rep.r_procp
        .set(infop.nmi_procp.map_or(ptr::null(), ptr::from_ref));
    rep.r_procnum.set(procnum);

    // empty mbuf for AUTH_UNIX header
    let Some(head) = m_gethdr(M_WAIT, MT_DATA) else {
        m_freem(infop.nmi_mreq.take());
        pool_put(&NFSREQPL, item);
        return Err(Errno::ENOBUFS);
    };
    head.m_next().set(infop.nmi_mreq);
    head.m_len().set(0);
    m_calchdrlen(head);
    rep.r_mreq.set(Some(head));

    let mut trylater_delay = nfs_mintimeo();

    let nmp = r_nmp(rep);

    // Get the RPC header with authorization.
    // SAFETY: the caller puts in `nmi_cred` the credential of the RPC, alive for the call
    // (`nfsm_subs.rs`).
    let Some(cred) = (unsafe { infop.nmi_cred.as_ref() }) else {
        panic(format_args!("nfs_request: no credential"));
    };
    crate::nfs::nfs_subs::nfsm_rpchead(rep, cred, RPCAUTH_UNIX);
    let mut m = head;

    // For stream protocols, insert a Sun RPC Record Mark. (As in C, `m` is what the first
    // send copies and `r_mreq` what retransmits copy and the end frees: the same mbuf, since
    // `nfsm_rpchead` `m_align`s the header and leaves the leading space the mark goes in.)
    if nmp.nm_sotype.get() == SOCK_STREAM {
        let Some(mm) = m_prepend(m, NFSX_UNSIGNED as i32, M_WAIT) else {
            // m_prepend freed the whole request.
            infop.nmi_mreq = None;
            rep.r_mreq.set(None);
            pool_put(&NFSREQPL, item);
            return Err(Errno::ENOBUFS);
        };
        m = mm;
        let mark = 0x8000_0000 | (m.m_pkthdr().len.get() as u32).wrapping_sub(NFSX_UNSIGNED as u32);
        // SAFETY: `m_prepend` made the first `NFSX_UNSIGNED` bytes of `m` its data; they may
        // be unaligned.
        unsafe { mtod::<u32>(m).write_unaligned(mark.to_be()) };
    }

    let result = 'nfsmout1: loop {
        // tryagain:
        rep.r_rtt.set(0);
        rep.r_rexmit.set(0);
        if NFS_PTIMERS[rep.r_procnum.get()] != NFS_DEFAULT_TIMER {
            rep.r_flags.set(R_TIMING);
        } else {
            rep.r_flags.set(0);
        }
        rep.r_mrep.set(None);

        // Do the client side RPC.
        NFSSTATS.rpcrequests.fetch_add(1, Relaxed);
        // Chain request into list of outstanding requests. Be sure to put it LAST so timer
        // finds oldest requests first.
        if nmp.nm_reqsq.is_empty() {
            timeout_add(&nmp.nm_rtimeout, NFS_TICKS.load(Relaxed));
        }
        // SAFETY: `rep` is in no queue (fresh, or unlinked below before a retry) and stays in
        // place until the unlink below.
        unsafe { nmp.nm_reqsq.insert_tail(rep) };

        // If backing off another request or avoiding congestion, don't send this one now but
        // let timer do it. If not timing a request, do it now.
        let mut error = Ok(());
        if let Some(so) = nmp.nm_so.get()
            && (nmp.nm_sotype.get() != SOCK_DGRAM
                || nmp.nm_flag.get() & NFSMNT_DUMBTIMR != 0
                || nmp.nm_sent.get() < nmp.nm_cwnd.get())
        {
            let connreq = nmp.nm_soflags.get() & i32::from(PR_CONNREQUIRED) != 0;
            if connreq {
                error = nfs_sndlock(&nmp.nm_flag, Some(rep));
            }
            if error.is_ok() {
                match m_copym(m, 0, M_COPYALL, M_WAIT) {
                    Some(copy) => error = nfs_send(so, nmp.nm_nam.get(), copy, Some(rep)),
                    None => rep.r_flags.set(rep.r_flags.get() | R_MUSTRESEND),
                }
                if connreq {
                    nfs_sndunlock(&nmp.nm_flag);
                }
            }
            if error.is_ok() && rep.r_flags.get() & R_MUSTRESEND == 0 {
                nmp.nm_sent.set(nmp.nm_sent.get() + NFS_CWNDSCALE);
                rep.r_flags.set(rep.r_flags.get() | R_SENT);
            }
        } else {
            rep.r_rtt.set(-1);
        }

        // Wait for the reply from our send or the timer's.
        if error.is_ok() || error == Err(Errno::EPIPE) {
            error = nfs_reply(rep);
        }

        // RPC done, unlink the request.
        // SAFETY: `rep` was inserted above and nothing else unlinks it.
        unsafe { nmp.nm_reqsq.remove(rep) };
        if nmp.nm_reqsq.is_empty() {
            timeout_del(&nmp.nm_rtimeout);
        }

        // Decrement the outstanding request count.
        if rep.r_flags.get() & R_SENT != 0 {
            rep.r_flags.set(rep.r_flags.get() & !R_SENT); // paranoia
            nmp.nm_sent.set(nmp.nm_sent.get() - NFS_CWNDSCALE);
        }

        // If there was a successful reply and a tprintf msg. tprintf a response.
        if error.is_ok() && rep.r_flags.get() & R_TPRINTFMSG != 0 {
            nfs_msg(rep, "is alive again");
        }
        let mut info = NfsmInfo::new();
        info.nmi_mrep = rep.r_mrep.get();
        info.nmi_md = rep.r_md.get();
        info.nmi_dpos = rep.r_dpos.get();
        if let Err(error) = error {
            infop.nmi_mrep = None;
            break 'nfsmout1 Err(error);
        }

        // break down the rpc header and check if ok
        let error: Result<(), Errno> = 'nfsmout: {
            let tl = match nfsm_dissect(&mut info, 3 * NFSX_UNSIGNED) {
                Ok(tl) => [tl.get(0), tl.get(1), tl.get(2)],
                Err(error) => break 'nfsmout Err(error),
            };
            if tl[0] == rpc_msgdenied {
                let error = if tl[1] == rpc_mismatch {
                    Errno::EOPNOTSUPP
                } else {
                    Errno::EACCES // Should be EAUTH.
                };
                m_freem(info.nmi_mrep.take());
                infop.nmi_mrep = None;
                break 'nfsmout1 Err(error);
            }

            // Since we only support RPCAUTH_UNIX atm we step over the reply verifier type,
            // and in the (error) case that there really is any data in it, we advance over
            // it.
            let i = fxdr_unsigned(tl[2]) as i32; // tl[1]: verifier type
            if i > 0 {
                // Should not happen
                if let Err(error) = nfsm_adv(&mut info, nfsm_rndup(i as usize)) {
                    break 'nfsmout Err(error);
                }
            }

            let accepted = match nfsm_dissect(&mut info, NFSX_UNSIGNED) {
                Ok(tl) => tl.get(0),
                Err(error) => break 'nfsmout Err(error),
            };
            // 0 == ok
            if accepted == 0 {
                let status = match nfsm_dissect(&mut info, NFSX_UNSIGNED) {
                    Ok(tl) => tl.get(0),
                    Err(error) => break 'nfsmout Err(error),
                };
                if status != 0 {
                    let status = fxdr_unsigned(status) as i32;
                    if nmp.nm_flag.get() & NFSMNT_NFSV3 != 0 && status == NFSERR_TRYLATER {
                        m_freem(info.nmi_mrep.take());
                        let _ = tsleep_nsec(
                            nowake(),
                            PSOCK,
                            "nfsretry",
                            sec_to_nsec(trylater_delay as u64),
                        );
                        trylater_delay *= NFS_TIMEOUTMUL;
                        if trylater_delay > nfs_maxtimeo() {
                            trylater_delay = nfs_maxtimeo();
                        }

                        continue 'nfsmout1; // tryagain
                    }

                    // If the File Handle was stale, invalidate the lookup cache, just in
                    // case.
                    if status == Errno::ESTALE.as_i32()
                        && let Some(vp) = rep.r_vp.get()
                    {
                        cache_purge(vp);
                    }
                    break 'nfsmout Err(nfs_status_errno(status));
                }
                break 'nfsmout Ok(());
            }

            Err(Errno::EPROTONOSUPPORT)
        };

        // nfsmout:
        infop.nmi_mrep = info.nmi_mrep;
        infop.nmi_md = info.nmi_md;
        infop.nmi_dpos = info.nmi_dpos;
        break error;
    };

    // nfsmout1:
    m_freem(rep.r_mreq.get());
    pool_put(&NFSREQPL, item);
    result
}

/// `nfs_rephead(siz, nd, slp, err, mrq, mbp)`: generate the rpc reply header; `siz` is used
/// to decide if adding a cluster is worthwhile. `err` is an errno number or an NFS status,
/// with the `NFSERR_AUTHERR`/`NFSERR_RETVOID` marks. Returns the reply (`*mrq`) and the build
/// cursor at its end (`*mbp`).
pub fn nfs_rephead(
    siz: usize,
    nd: &NfsrvDescript,
    _slp: Option<&NfssvcSock>,
    err: i32,
) -> Result<(&'static Mbuf, &'static Mbuf), Errno> {
    let Some(mreq) = m_gethdr(M_WAIT, MT_DATA) else {
        return Err(Errno::ENOBUFS);
    };
    let mut mb = mreq;
    // If this is a big reply, use a cluster else try and leave leading space for the lower
    // level headers.
    let siz = siz + RPC_REPLYSIZ;
    let max_hdr = MAX_HDR.load(Relaxed);
    if siz as i64 >= MHLEN as i64 - i64::from(max_hdr) {
        mclget(mreq, M_WAIT);
    } else {
        mreq.m_data()
            .set(mreq.m_data().get().wrapping_add(max_hdr as usize));
    }
    mreq.m_len().set(0);

    // The six header words (the sixth dropped for an authentication error) and the words
    // that follow them.
    let mut hdr = [0u32; 6];
    let mut extra = [0u32; 2];
    let mut nextra = 0;
    hdr[0] = txdr_unsigned(nd.nd_retxid);
    hdr[1] = rpc_reply;
    let autherr = err & NFSERR_AUTHERR != 0;
    if err == Errno::ERPCMISMATCH.as_i32() || autherr {
        hdr[2] = rpc_msgdenied;
        if autherr {
            hdr[3] = rpc_autherr;
            hdr[4] = txdr_unsigned((err & !NFSERR_AUTHERR) as u32);
        } else {
            hdr[3] = rpc_mismatch;
            hdr[4] = txdr_unsigned(RPC_VER2);
            hdr[5] = txdr_unsigned(RPC_VER2);
        }
    } else {
        hdr[2] = rpc_msgaccepted;

        // AUTH_UNIX requires RPCAUTH_NULL.
        hdr[3] = 0;
        hdr[4] = 0;

        if err == Errno::EPROGUNAVAIL.as_i32() {
            hdr[5] = txdr_unsigned(RPC_PROGUNAVAIL);
        } else if err == Errno::EPROGMISMATCH.as_i32() {
            hdr[5] = txdr_unsigned(RPC_PROGMISMATCH);
            extra = [txdr_unsigned(NFS_VER2), txdr_unsigned(NFS_VER3)];
            nextra = 2;
        } else if err == Errno::EPROCUNAVAIL.as_i32() {
            hdr[5] = txdr_unsigned(RPC_PROCUNAVAIL);
        } else if err == Errno::EBADRPC.as_i32() {
            hdr[5] = txdr_unsigned(RPC_GARBAGE);
        } else {
            hdr[5] = 0;
            if err != NFSERR_RETVOID {
                if err != 0 {
                    extra[0] = txdr_unsigned(crate::nfs::nfs_subs::nfsrv_errmap(nd, err) as u32);
                }
                nextra = 1;
            }
        }
    }

    let mut tl = nfsm_build(&mut mb, 6 * NFSX_UNSIGNED);
    for w in hdr {
        tl.put(w);
    }
    if autherr {
        mreq.m_len().set(mreq.m_len().get() - NFSX_UNSIGNED as u32);
    }
    if nextra > 0 {
        let mut tl = nfsm_build(&mut mb, nextra * NFSX_UNSIGNED);
        for &w in &extra[..nextra] {
            tl.put(w);
        }
    }

    if err != 0 && err != NFSERR_RETVOID {
        NFSSTATS.srvrpc_errs.fetch_add(1, Relaxed);
    }
    Ok((mreq, mb))
}

/// `nfs_timer(arg)`: nfs timer routine. Scan the nfsreq list and retransmit any requests that
/// have timed out. `arg` is the `struct nfsmount` whose `nm_rtimeout` this is
/// (`nfs_vfsops.c`'s `timeout_set_proc`).
pub fn nfs_timer(arg: *mut c_void) {
    // SAFETY: `arg` is the mount `nfs_vfsops` registered with its own `nm_rtimeout`; the
    // mount outlives the timeout, which `nfs_unmount` deletes before freeing it.
    let nmp: &NfsMount = unsafe { &*arg.cast::<NfsMount>() };

    net_lock();
    for rep in nmp.nm_reqsq.iter() {
        if rep.r_mrep.get().is_some() || rep.r_flags.get() & R_SOFTTERM != 0 {
            continue;
        }
        if nfs_sigintr(nmp, Some(rep), r_procp(rep)).is_err() {
            rep.r_flags.set(rep.r_flags.get() | R_SOFTTERM);
            continue;
        }
        if rep.r_rtt.get() >= 0 {
            rep.r_rtt.set(rep.r_rtt.get() + 1);
            let mut timeo = if nmp.nm_flag.get() & NFSMNT_DUMBTIMR != 0 {
                nmp.nm_timeo.get()
            } else {
                nfs_estimate_rto(nmp, rep.r_procnum.get())
            };
            if nmp.nm_timeouts.get() > 0 {
                timeo *= NFS_BACKOFF[nmp.nm_timeouts.get() as usize - 1];
            }
            if rep.r_rtt.get() <= timeo {
                continue;
            }
            if (nmp.nm_timeouts.get() as usize) < NFS_BACKOFF.len() {
                nmp.nm_timeouts.set(nmp.nm_timeouts.get() + 1);
            }
        }

        // Check for server not responding.
        if rep.r_flags.get() & R_TPRINTFMSG == 0 && rep.r_rexmit.get() > 4 {
            nfs_msg(rep, "not responding");
            rep.r_flags.set(rep.r_flags.get() | R_TPRINTFMSG);
        }
        if rep.r_rexmit.get() >= nmp.nm_retry.get() {
            // too many
            NFSSTATS.rpctimeouts.fetch_add(1, Relaxed);
            rep.r_flags.set(rep.r_flags.get() | R_SOFTTERM);
            continue;
        }
        if nmp.nm_sotype.get() != SOCK_DGRAM {
            rep.r_rexmit.set(min(rep.r_rexmit.get() + 1, NFS_MAXREXMIT));
            continue;
        }

        let Some(so) = nmp.nm_so.get() else {
            continue;
        };

        // If there is enough space and the window allows.. Resend it. Set r_rtt to -1 in case
        // we fail to send it now.
        rep.r_rtt.set(-1);
        let Some(mreq) = rep.r_mreq.get() else {
            continue;
        };
        if sbspace(&so.so_snd) >= i64::from(mreq.m_pkthdr().len.get())
            && (nmp.nm_flag.get() & NFSMNT_DUMBTIMR != 0
                || rep.r_flags.get() & R_SENT != 0
                || nmp.nm_sent.get() < nmp.nm_cwnd.get())
            && let Some(m) = m_copym(mreq, 0, M_COPYALL, M_DONTWAIT)
        {
            let error = if nmp.nm_flag.get() & NFSMNT_NOCONN == 0 {
                pru_send(so, Some(m), None, None)
            } else {
                pru_send(so, Some(m), nmp.nm_nam.get(), None)
            };
            if let Err(error) = error {
                if nfsignore_soerror(nmp.nm_soflags.get(), error) {
                    so.set_error(None);
                }
            } else {
                // Iff first send, start timing else turn timing off, backoff timer and divide
                // congestion window by 2.
                if rep.r_flags.get() & R_SENT != 0 {
                    rep.r_flags.set(rep.r_flags.get() & !R_TIMING);
                    rep.r_rexmit.set(min(rep.r_rexmit.get() + 1, NFS_MAXREXMIT));
                    nmp.nm_cwnd.set(max(nmp.nm_cwnd.get() >> 1, NFS_CWNDSCALE));
                    NFSSTATS.rpcretries.fetch_add(1, Relaxed);
                } else {
                    rep.r_flags.set(rep.r_flags.get() | R_SENT);
                    nmp.nm_sent.set(nmp.nm_sent.get() + NFS_CWNDSCALE);
                }
                rep.r_rtt.set(0);
            }
        }
    }
    net_unlock();
    timeout_add(&nmp.nm_rtimeout, NFS_TICKS.load(Relaxed));
}

/// `nfs_sigintr(nmp, rep, p)`: test for a termination condition pending on the process. This
/// is used for `NFSMNT_INT` mounts. `Err(EINTR)` when the RPC must stop.
pub fn nfs_sigintr(nmp: &NfsMount, rep: Option<&NfsReq>, p: Option<&Proc>) -> Result<(), Errno> {
    if let Some(rep) = rep
        && rep.r_flags.get() & R_SOFTTERM != 0
    {
        return Err(Errno::EINTR);
    }
    if nmp.nm_flag.get() & NFSMNT_INT == 0 {
        return Ok(());
    }
    if let Some(p) = p
        && sigpending(p) & !p.process().sigacts().ps_sigignore.get() & NFSINT_SIGMASK != 0
    {
        return Err(Errno::EINTR);
    }
    Ok(())
}

/// `nfs_sndlock(flagp, rep)`: lock a socket against others. Necessary for STREAM sockets to
/// ensure you get an entire rpc request/reply and also to avoid race conditions between the
/// processes with nfs requests in progress when a reconnect is necessary. `flagp` is
/// `&nmp->nm_flag` or `&slp->ns_solock`.
pub fn nfs_sndlock(flagp: &Cell<i32>, rep: Option<&NfsReq>) -> Result<(), Errno> {
    let mut slptimeo = INFSLP;
    let mut slpflag = 0;
    let mut p = None;

    if let Some(rep) = rep {
        p = r_procp(rep);
        if r_nmp(rep).nm_flag.get() & NFSMNT_INT != 0 {
            slpflag = PCATCH;
        }
    }
    while flagp.get() & NFSMNT_SNDLOCK != 0 {
        if let Some(rep) = rep
            && nfs_sigintr(r_nmp(rep), Some(rep), p).is_err()
        {
            return Err(Errno::EINTR);
        }
        flagp.set(flagp.get() | NFSMNT_WANTSND);
        let _ = tsleep_nsec(
            ptr::from_ref(flagp),
            slpflag | (PZERO - 1),
            "nfsndlck",
            slptimeo,
        );
        if slpflag == PCATCH {
            slpflag = 0;
            slptimeo = sec_to_nsec(2);
        }
    }
    flagp.set(flagp.get() | NFSMNT_SNDLOCK);
    Ok(())
}

/// `nfs_sndunlock(flagp)`: unlock the stream socket for others.
pub fn nfs_sndunlock(flagp: &Cell<i32>) {
    if flagp.get() & NFSMNT_SNDLOCK == 0 {
        panic(format_args!("nfs sndunlock"));
    }
    flagp.set(flagp.get() & !NFSMNT_SNDLOCK);
    if flagp.get() & NFSMNT_WANTSND != 0 {
        flagp.set(flagp.get() & !NFSMNT_WANTSND);
        wakeup(ptr::from_ref(flagp));
    }
}

/// `nfs_rcvlock(rep)`: lock the mount's socket against other receivers. `Err(EALREADY)` when
/// our reply has been received while we were sleeping.
pub fn nfs_rcvlock(rep: &NfsReq) -> Result<(), Errno> {
    let mut slptimeo = INFSLP;
    let nmp = r_nmp(rep);
    let flagp = &nmp.nm_flag;
    let mut slpflag = if flagp.get() & NFSMNT_INT != 0 {
        PCATCH
    } else {
        0
    };

    while flagp.get() & NFSMNT_RCVLOCK != 0 {
        if nfs_sigintr(nmp, Some(rep), r_procp(rep)).is_err() {
            return Err(Errno::EINTR);
        }
        flagp.set(flagp.get() | NFSMNT_WANTRCV);
        let _ = tsleep_nsec(
            ptr::from_ref(flagp),
            slpflag | (PZERO - 1),
            "nfsrcvlk",
            slptimeo,
        );
        if rep.r_mrep.get().is_some() {
            // Don't take the lock if our reply has been received while we where sleeping.
            return Err(Errno::EALREADY);
        }
        if slpflag == PCATCH {
            slpflag = 0;
            slptimeo = sec_to_nsec(2);
        }
    }
    flagp.set(flagp.get() | NFSMNT_RCVLOCK);
    Ok(())
}

/// `nfs_rcvunlock(flagp)`: unlock the stream socket for others.
pub fn nfs_rcvunlock(flagp: &Cell<i32>) {
    if flagp.get() & NFSMNT_RCVLOCK == 0 {
        panic(format_args!("nfs rcvunlock"));
    }
    flagp.set(flagp.get() & !NFSMNT_RCVLOCK);
    if flagp.get() & NFSMNT_WANTRCV != 0 {
        flagp.set(flagp.get() & !NFSMNT_WANTRCV);
        wakeup(ptr::from_ref(flagp));
    }
}

/// `ALIGNED_POINTER(x, void *)`, the alignment `nfs_realign` wants for `m_data` and `m_len`.
fn aligned_void_pointer(x: usize) -> bool {
    aligned_pointer::<*const c_void>(x)
}

/// `nfs_realign_fixup(m, n, off)`: auxiliary routine to align the length of mbuf copies made
/// with `m_copyback()`.
pub fn nfs_realign_fixup(m: &'static Mbuf, n: &'static Mbuf, off: &mut u32) {
    realign_fixup(m, n, off, aligned_void_pointer);
}

/// `nfs_realign_fixup` with the alignment test as a parameter (host tests use a strict one).
fn realign_fixup(m: &'static Mbuf, n: &'static Mbuf, off: &mut u32, aligned: fn(usize) -> bool) {
    // The maximum number of bytes that m_copyback() places in a mbuf is always an aligned
    // quantity, so realign happens at the chain's tail.
    let mut n = n;
    while let Some(next) = n.m_next().get() {
        n = next;
    }

    // Pad from the next elements in the source chain. Loop until the destination chain is
    // aligned, or the end of the source is reached.
    let mut m = m;
    loop {
        let Some(next) = m.m_next().get() else {
            return;
        };
        m = next;

        let nlen = n.m_len().get() as usize;
        let padding = min(align(nlen) - nlen, m.m_len().get() as usize);
        if padding > m_trailingspace(n) as usize {
            panic(format_args!("nfs_realign_fixup: no memory to pad to"));
        }

        // SAFETY: `padding` bytes are `m`'s data (at most its `m_len`) and fit in `n`'s
        // trailing space (checked above); the two mbufs are different storage.
        unsafe { ptr::copy_nonoverlapping(mtod::<u8>(m), mtod::<u8>(n).add(nlen), padding) };

        n.m_len().set((nlen + padding) as u32);
        m_adj(m, padding as i32);
        *off += padding as u32;

        if aligned(n.m_len().get() as usize) {
            break;
        }
    }
}

/// The C's `ALIGN_POINTER(n)` of `nfs_realign`, kept as written:
/// `(u_int)(((n) + sizeof(void *)) & ~sizeof(void *))`.
const fn align_pointer(n: usize) -> usize {
    ((n + size_of::<*const c_void>()) & !size_of::<*const c_void>()) as u32 as usize
}

/// `nfs_realign(pm, hsiz)`: the NFS RPC parsing code uses the data address and the length of
/// mbuf structures to calculate on-memory addresses. This function makes sure these
/// parameters are correctly aligned. (`hsiz` is unused, as in C.)
pub fn nfs_realign(pm: &mut Option<&'static Mbuf>, _hsiz: usize) {
    realign(pm, aligned_void_pointer);
}

/// `nfs_realign` with the alignment test as a parameter (host tests use a strict one).
fn realign(pm: &mut Option<&'static Mbuf>, aligned: fn(usize) -> bool) {
    let mut prev: Option<&'static Mbuf> = None; // the mbuf whose m_next is *pm; None: pm
    let mut cur = *pm;
    let mut n = None;
    let mut off: u32 = 0;

    NFS_REALIGN_TEST.fetch_add(1, Relaxed);
    while let Some(m) = cur {
        if !aligned(m.m_data().get().addr()) || !aligned(m.m_len().get() as usize) {
            let Some(nm) = m_get(M_WAIT, MT_DATA) else {
                return;
            };
            if align_pointer(m.m_len().get() as usize) >= MINCLSIZE {
                mclget(nm, M_WAIT);
            }
            nm.m_len().set(0);
            n = Some(nm);
            break;
        }
        prev = Some(m);
        cur = m.m_next().get();
    }
    // If n is non-NULL, loop on m copying data, then replace the portion of the chain that
    // had to be realigned.
    let Some(n) = n else {
        return;
    };
    NFS_REALIGN_COUNT.fetch_add(1, Relaxed);
    while let Some(m) = cur {
        let len = m.m_len().get() as usize;
        // SAFETY: the `m_len` bytes at `m_data` are `m`'s data, which nothing writes while
        // they are copied out.
        let data = unsafe { core::slice::from_raw_parts(mtod::<u8>(m), len) };
        let _ = m_copyback(n, off as i32, data, M_WAIT);

        // If an unaligned amount of memory was copied, fix up the last mbuf created by
        // m_copyback().
        if !aligned(len) {
            realign_fixup(m, n, &mut off, aligned);
        }

        off += len as u32;
        cur = m.m_next().get();
    }
    match prev {
        None => {
            m_freem(pm.take());
            *pm = Some(n);
        }
        Some(prev) => {
            m_freem(prev.m_next().take());
            prev.m_next().set(Some(n));
        }
    }
}

/// The raw words of a dissected view, copied out (`tl[0..N]`).
fn words<const N: usize>(tl: &XdrIn<'_>) -> [u32; N] {
    core::array::from_fn(|i| tl.get(i))
}

/// `nfs_getreq(nd, nfsd, has_header)`: parse an RPC request: verify it, fill in the cred
/// struct. A request the server must answer with an RPC error comes back `Ok` with
/// `nd_repstat` set and `nd_procnum` `NFSPROC_NOOP`; a malformed one frees the request and
/// fails with `EBADRPC`.
pub fn nfs_getreq(
    nd: &mut NfsrvDescript,
    _nfsd: Option<&Nfsd>,
    has_header: bool,
) -> Result<(), Errno> {
    let mut w = [0u32; 10];
    let mut i;
    if has_header {
        w = words::<10>(&nfsd_dissect(nd, 10 * NFSX_UNSIGNED)?);
        nd.nd_retxid = fxdr_unsigned(w[0]);
        if w[1] != rpc_call {
            m_freem(nd.nd_mrep.take());
            return Err(Errno::EBADRPC);
        }
        i = 2;
    } else {
        let tl = words::<8>(&nfsd_dissect(nd, 8 * NFSX_UNSIGNED)?);
        w[..8].copy_from_slice(&tl);
        i = 0;
    }
    nd.nd_repstat = 0;
    nd.nd_flag = 0;
    if w[i] != rpc_vers {
        nd.nd_repstat = Errno::ERPCMISMATCH.as_i32();
        nd.nd_procnum = NFSPROC_NOOP;
        return Ok(());
    }
    i += 1;
    if w[i] != nfs_prog {
        nd.nd_repstat = Errno::EPROGUNAVAIL.as_i32();
        nd.nd_procnum = NFSPROC_NOOP;
        return Ok(());
    }
    i += 1;
    let nfsvers = fxdr_unsigned(w[i]);
    i += 1;
    if nfsvers != NFS_VER2 && nfsvers != NFS_VER3 {
        nd.nd_repstat = Errno::EPROGMISMATCH.as_i32();
        nd.nd_procnum = NFSPROC_NOOP;
        return Ok(());
    }
    if nfsvers == NFS_VER3 {
        nd.nd_flag = ND_NFSV3;
    }
    nd.nd_procnum = fxdr_unsigned(w[i]) as usize;
    i += 1;
    if nd.nd_procnum == NFSPROC_NULL {
        return Ok(());
    }
    if nd.nd_procnum >= NFS_NPROCS
        || nd.nd_procnum > NFSPROC_COMMIT
        || (nd.nd_flag == 0 && nd.nd_procnum > NFSV2PROC_STATFS)
    {
        nd.nd_repstat = Errno::EPROCUNAVAIL.as_i32();
        nd.nd_procnum = NFSPROC_NOOP;
        return Ok(());
    }
    if nd.nd_flag & ND_NFSV3 == 0 {
        nd.nd_procnum = NFSV3_PROCID[nd.nd_procnum];
    }
    let auth_type = w[i];
    i += 1;
    let len = fxdr_unsigned(w[i]) as i32;
    if len < 0 || len as usize > RPCAUTH_MAXSIZ {
        m_freem(nd.nd_mrep.take());
        return Err(Errno::EBADRPC);
    }

    // Handle auth_unix
    if auth_type == rpc_auth_unix {
        // w[i + 1]: the stamp; w[i + 2]: the length of the machine name.
        let len = fxdr_unsigned(w[i + 2]) as i32;
        if len < 0 || len as usize > NFS_MAXNAMLEN {
            m_freem(nd.nd_mrep.take());
            return Err(Errno::EBADRPC);
        }
        nfsd_adv(nd, nfsm_rndup(len as usize))?;
        let tl = words::<3>(&nfsd_dissect(nd, 3 * NFSX_UNSIGNED)?);
        nd.nd_cr = Ucred::new();
        refcnt_init(&nd.nd_cr.cr_refcnt);
        nd.nd_cr.cr_uid.set(fxdr_unsigned(tl[0]));
        nd.nd_cr.cr_gid.set(fxdr_unsigned(tl[1]));
        let len = fxdr_unsigned(tl[2]) as i32;
        if len < 0 || len as usize > RPCAUTH_UNIXGIDS {
            m_freem(nd.nd_mrep.take());
            return Err(Errno::EBADRPC);
        }
        let len = len as usize;
        let tl: [u32; RPCAUTH_UNIXGIDS + 2] = {
            let tl = nfsd_dissect(nd, (len + 2) * NFSX_UNSIGNED)?;
            core::array::from_fn(|j| if j < len + 2 { tl.get(j) } else { 0 })
        };
        for (group, w) in nd.nd_cr.cr_groups.iter().zip(&tl[..len]) {
            group.set(fxdr_unsigned(*w)); // the groups past NGROUPS_MAX are skipped
        }
        nd.nd_cr.cr_ngroups.set(min(len, NGROUPS_MAX) as i16);
        // tl[len]: the verifier's flavor; tl[len + 1]: its length.
        let len = fxdr_unsigned(tl[len + 1]) as i32;
        if len < 0 || len as usize > RPCAUTH_MAXSIZ {
            m_freem(nd.nd_mrep.take());
            return Err(Errno::EBADRPC);
        }
        if len > 0 {
            nfsd_adv(nd, nfsm_rndup(len as usize))?;
        }
    } else {
        nd.nd_repstat = NFSERR_AUTHERR | AUTH_REJECTCRED as i32;
        nd.nd_procnum = NFSPROC_NOOP;
        return Ok(());
    }

    Ok(())
}

/// `nfs_msg(rep, msg)`: tell the requesting thread's terminal (and the log) about the
/// server.
pub fn nfs_msg(rep: &NfsReq, msg: &str) {
    let tpr = match r_procp(rep) {
        Some(p) => tprintf_open(p),
        None => None,
    };

    tprintf(
        tpr,
        format_args!("nfs server {}: {}\n", Str(&nm_mntfromname(r_nmp(rep))), msg),
    );
    tprintf_close(tpr);
}

/// `nfsrv_rcv(so, arg, waitflag)`: socket upcall routine for the nfsd sockets. The `arg` is
/// a pointer to the `struct nfssvc_sock`. Essentially do as much as possible non-blocking,
/// else punt and it will be called with `M_WAIT` from an nfsd.
#[cfg(feature = "nfsserver")]
pub fn nfsrv_rcv(so: &'static Socket, arg: *mut c_void, waitflag: i32) {
    // SAFETY: `arg` is the `nfssvc_sock` that `nfssvc_addsock` registered with this socket
    // (or the nfsd passes); it is freed only by `nfsrv_slpderef` after `nfsrv_zapsock`
    // cleared the upcall and while no nfsd holds a reference.
    let slp: &'static NfssvcSock = unsafe { &*arg.cast::<NfssvcSock>() };

    kernel_lock();

    if slp.ns_flag.get() & SLP_VALID == 0 {
        kernel_unlock(); // out
        return;
    }

    'dorecs: {
        // Defer soreceive() to an nfsd.
        if waitflag == M_DONTWAIT {
            slp.ns_flag.set(slp.ns_flag.get() | SLP_NEEDQ);
            break 'dorecs;
        }

        if so.so_type.get() == SOCK_STREAM {
            // Do soreceive().
            let mut auio = Uio {
                uio_iov: &mut [],
                uio_offset: 0,
                uio_resid: 1_000_000_000,
                uio_segflg: UioSeg::UIO_SYSSPACE,
                uio_rw: UioRw::UIO_READ,
                uio_procp: None,
            };
            let mut flags = MSG_DONTWAIT;
            let mut mp = None;
            let error = soreceive(
                so,
                None,
                &mut auio,
                Some(&mut mp),
                None,
                Some(&mut flags),
                0,
            );
            let m = match (error, mp) {
                (Ok(()), Some(m)) => m,
                (error, mp) => {
                    m_freem(mp);
                    if error == Err(Errno::EWOULDBLOCK) {
                        slp.ns_flag.set(slp.ns_flag.get() | SLP_NEEDQ);
                    } else {
                        slp.ns_flag.set(slp.ns_flag.get() | SLP_DISCONN);
                    }
                    break 'dorecs;
                }
            };
            let received = (1_000_000_000 - auio.uio_resid) as i32;
            if let Some(rawend) = slp.ns_rawend.get() {
                rawend.m_next().set(Some(m));
                slp.ns_cc.set(slp.ns_cc.get() + received);
            } else {
                slp.ns_raw.set(Some(m));
                slp.ns_cc.set(received);
            }
            let mut last = m;
            while let Some(next) = last.m_next().get() {
                last = next;
            }
            slp.ns_rawend.set(Some(last));

            // Now try and parse record(s) out of the raw stream data.
            if let Err(error) = nfsrv_getstream(slp, waitflag) {
                if error == Errno::EPERM {
                    slp.ns_flag.set(slp.ns_flag.get() | SLP_DISCONN);
                } else {
                    slp.ns_flag.set(slp.ns_flag.get() | SLP_NEEDQ);
                }
            }
        } else {
            loop {
                let mut auio = Uio {
                    uio_iov: &mut [],
                    uio_offset: 0,
                    uio_resid: 1_000_000_000,
                    uio_segflg: UioSeg::UIO_SYSSPACE,
                    uio_rw: UioRw::UIO_READ,
                    uio_procp: None,
                };
                let mut flags = MSG_DONTWAIT;
                let mut nam = None;
                let mut mp = None;
                let error = soreceive(
                    so,
                    Some(&mut nam),
                    &mut auio,
                    Some(&mut mp),
                    None,
                    Some(&mut flags),
                    0,
                );
                if let Some(data) = mp {
                    let m = match nam {
                        Some(nam) => {
                            nam.m_next().set(Some(data));
                            nam
                        }
                        None => data,
                    };
                    if let Some(recend) = slp.ns_recend.get() {
                        recend.m_nextpkt().set(Some(m));
                    } else {
                        slp.ns_rec.set(Some(m));
                    }
                    slp.ns_recend.set(Some(m));
                    m.m_nextpkt().set(None);
                } else {
                    m_freem(nam);
                }
                if let Err(error) = error
                    && so.so_proto.pr_flags & PR_CONNREQUIRED != 0
                    && error != Errno::EWOULDBLOCK
                {
                    slp.ns_flag.set(slp.ns_flag.get() | SLP_DISCONN);
                    break 'dorecs;
                }
                if mp.is_none() {
                    break;
                }
            }
        }
    }

    // Now try and process the request records, non-blocking.
    // dorecs:
    if waitflag == M_DONTWAIT
        && (slp.ns_rec.get().is_some() || slp.ns_flag.get() & (SLP_NEEDQ | SLP_DISCONN) != 0)
    {
        nfsrv_wakenfsd(slp);
    }

    // out:
    kernel_unlock();
}

/// `nfsrv_getstream(slp, waitflag)`: try and extract an RPC request from the mbuf data list
/// received on a stream socket. The `waitflag` argument indicates whether or not it can
/// sleep. `EPERM` for a record too long to be NFS, `EWOULDBLOCK` when an mbuf is short.
#[cfg(feature = "nfsserver")]
pub fn nfsrv_getstream(slp: &NfssvcSock, waitflag: i32) -> Result<(), Errno> {
    if slp.ns_flag.get() & SLP_GETSTREAM != 0 {
        return Ok(());
    }
    slp.ns_flag.set(slp.ns_flag.get() | SLP_GETSTREAM);
    let done = |r: Result<(), Errno>| {
        slp.ns_flag.set(slp.ns_flag.get() & !SLP_GETSTREAM);
        r
    };
    loop {
        if slp.ns_reclen.get() == 0 {
            if (slp.ns_cc.get() as usize) < NFSX_UNSIGNED {
                return done(Ok(()));
            }
            let Some(mut m) = slp.ns_raw.get() else {
                panic(format_args!(
                    "nfsrv_getstream: {} bytes, no data",
                    slp.ns_cc.get()
                ));
            };
            let mut recmark = [0u8; NFSX_UNSIGNED];
            if m.m_len().get() as usize >= NFSX_UNSIGNED {
                // SAFETY: the first `NFSX_UNSIGNED` bytes of `m`'s data (checked above).
                unsafe {
                    ptr::copy_nonoverlapping(mtod::<u8>(m), recmark.as_mut_ptr(), NFSX_UNSIGNED)
                };
                m.m_data().set(m.m_data().get().wrapping_add(NFSX_UNSIGNED));
                m.m_len().set(m.m_len().get() - NFSX_UNSIGNED as u32);
            } else {
                for b in &mut recmark {
                    while m.m_len().get() == 0 {
                        let Some(next) = m.m_next().get() else {
                            panic(format_args!("nfsrv_getstream: short record mark"));
                        };
                        m = next;
                    }
                    // SAFETY: `m` has at least one byte of data (the loop above).
                    *b = unsafe { mtod::<u8>(m).read() };
                    m.m_data().set(m.m_data().get().wrapping_add(1));
                    m.m_len().set(m.m_len().get() - 1);
                }
            }
            slp.ns_cc.set(slp.ns_cc.get() - NFSX_UNSIGNED as i32);
            let recmark = u32::from_be_bytes(recmark);
            slp.ns_reclen.set((recmark & !0x8000_0000) as i32);
            if recmark & 0x8000_0000 != 0 {
                slp.ns_flag.set(slp.ns_flag.get() | SLP_LASTFRAG);
            } else {
                slp.ns_flag.set(slp.ns_flag.get() & !SLP_LASTFRAG);
            }
            if slp.ns_reclen.get() as usize > NFS_MAXPACKET {
                return done(Err(Errno::EPERM));
            }
        }

        // Now get the record part.
        let mut recm = None;
        let cc = slp.ns_cc.get();
        let reclen = slp.ns_reclen.get();
        if cc == reclen {
            recm = slp.ns_raw.get();
            slp.ns_raw.set(None);
            slp.ns_rawend.set(None);
            slp.ns_cc.set(0);
            slp.ns_reclen.set(0);
        } else if cc > reclen {
            let mut len = 0;
            let mut m = slp.ns_raw.get();
            let mut om: Option<&'static Mbuf> = None;
            while len < reclen {
                let Some(mm) = m else {
                    panic(format_args!("nfsrv_getstream: {} of {} bytes", len, reclen));
                };
                let mlen = mm.m_len().get() as i32;
                if len + mlen > reclen {
                    let Some(m2) = m_copym(mm, 0, reclen - len, waitflag) else {
                        return done(Err(Errno::EWOULDBLOCK));
                    };
                    if let Some(om) = om {
                        om.m_next().set(Some(m2));
                        recm = slp.ns_raw.get();
                    } else {
                        recm = Some(m2);
                    }
                    mm.m_data()
                        .set(mm.m_data().get().wrapping_add((reclen - len) as usize));
                    mm.m_len().set((mlen - (reclen - len)) as u32);
                    len = reclen;
                } else if len + mlen == reclen {
                    om = Some(mm);
                    len += mlen;
                    m = mm.m_next().get();
                    recm = slp.ns_raw.get();
                    mm.m_next().set(None);
                } else {
                    om = Some(mm);
                    len += mlen;
                    m = mm.m_next().get();
                }
            }
            slp.ns_raw.set(m);
            slp.ns_cc.set(cc - len);
            slp.ns_reclen.set(0);
        } else {
            return done(Ok(()));
        }

        // Accumulate the fragments into a record.
        match slp.ns_frag.get() {
            None => slp.ns_frag.set(recm),
            Some(first) => {
                let mut last = first;
                while let Some(next) = last.m_next().get() {
                    last = next;
                }
                last.m_next().set(recm);
            }
        }
        if slp.ns_flag.get() & SLP_LASTFRAG != 0 {
            if let Some(recend) = slp.ns_recend.get() {
                recend.m_nextpkt().set(slp.ns_frag.get());
            } else {
                slp.ns_rec.set(slp.ns_frag.get());
            }
            slp.ns_recend.set(slp.ns_frag.get());
            slp.ns_frag.set(None);
        }
    }
}

/// `nfsrv_dorec(slp, nfsd, ndp)`: parse an RPC header. Dequeues the socket's next record into
/// a new `nfsrv_descript_pl` descriptor, which becomes `nfsd->nfsd_nd` and is returned
/// (`*ndp`); `ENOBUFS` when there is no record.
#[cfg(feature = "nfsserver")]
pub fn nfsrv_dorec(slp: &NfssvcSock, nfsd: &Nfsd) -> Result<NonNull<NfsrvDescript>, Errno> {
    if slp.ns_flag.get() & SLP_VALID == 0 {
        return Err(Errno::ENOBUFS);
    }
    let Some(m) = slp.ns_rec.get() else {
        return Err(Errno::ENOBUFS);
    };
    slp.ns_rec.set(m.m_nextpkt().get());
    if slp.ns_rec.get().is_some() {
        m.m_nextpkt().set(None);
    } else {
        slp.ns_recend.set(None);
    }
    let (nam, m) = if i32::from(m.m_type().get()) == MT_SONAME {
        (Some(m), m.m_next().take())
    } else {
        (None, Some(m))
    };
    let Some(item) = pool_get(&NFSRV_DESCRIPT_PL, PR_WAITOK) else {
        m_freem(nam);
        m_freem(m);
        return Err(Errno::ENOBUFS);
    };
    let ndp = item.cast::<NfsrvDescript>();
    // SAFETY: a fresh `nfsrv_descript_pl` item, sized and aligned for a `struct
    // nfsrv_descript`; the nfsd owns it until it puts it back.
    let nd: &mut NfsrvDescript = unsafe {
        ndp.as_ptr().write(NfsrvDescript::new());
        &mut *ndp.as_ptr()
    };
    let mut m = m;
    nfs_realign(&mut m, 10 * NFSX_UNSIGNED);
    nd.nd_md = m;
    nd.nd_mrep = m;
    nd.nd_nam2 = nam;
    nd.nd_dpos = m.map_or(ptr::null_mut(), mtod::<u8>);
    if let Err(error) = nfs_getreq(nd, Some(nfsd), true) {
        m_freem(nam);
        pool_put(&NFSRV_DESCRIPT_PL, item);
        return Err(error);
    }
    nfsd.nfsd_nd.set(Some(ndp));
    Ok(ndp)
}

/// `nfsrv_wakenfsd(slp)`: search for a sleeping nfsd and wake it up. SIDE EFFECT: If none
/// found, set `NFSD_CHECKSLP` flag, so that one of the running nfsds will go look for the work
/// in the nfssvc_sock list.
#[cfg(feature = "nfsserver")]
pub fn nfsrv_wakenfsd(slp: &'static NfssvcSock) {
    if slp.ns_flag.get() & SLP_VALID == 0 {
        return;
    }

    for nfsd in NFSD_HEAD.0.iter() {
        if nfsd.nfsd_flag.get() & NFSD_WAITING != 0 {
            nfsd.nfsd_flag.set(nfsd.nfsd_flag.get() & !NFSD_WAITING);
            if nfsd.nfsd_slp.get().is_some() {
                panic(format_args!("nfsd wakeup"));
            }
            slp.ns_sref.set(slp.ns_sref.get() + 1);
            nfsd.nfsd_slp.set(Some(slp));
            wakeup_one(ptr::from_ref(nfsd));
            return;
        }
    }

    slp.ns_flag.set(slp.ns_flag.get() | SLP_DOREC);
    NFSD_HEAD_FLAG.fetch_or(NFSD_CHECKSLP, Relaxed);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `nfs_socket.c`: the RTT estimator, the socket flag locks, `nfs_sigintr`,
    // `nfs_realign` on a misaligned chain, the reply header of `nfs_rephead`, the request parse
    // of `nfs_getreq` and the record marking of `nfsrv_getstream`.

    use std::boxed::Box;
    use std::vec::Vec;

    use super::*;
    use crate::kern::uipc_mbuf::tests::setup;
    use crate::nfs::nfs_subs::tests::{bytes, chain, lens};
    use crate::nfs::nfsproto::{NFSPROC_GETATTR, NFSPROC_READ, NFSPROC_SETATTR};
    use crate::nfs::rpcv2::AUTH_BADCRED;
    use crate::sys::param::ALIGNBYTES;

    /// A mount that lives for the rest of the test run.
    fn leak_mount() -> &'static NfsMount {
        Box::leak(Box::new(NfsMount::new()))
    }

    /// A request of procedure `procnum` on `nmp`.
    fn leak_req(nmp: &'static NfsMount, procnum: usize) -> &'static NfsReq {
        let rep = Box::leak(Box::new(NfsReq::new()));
        rep.r_nmp.set(Some(nmp));
        rep.r_procnum.set(procnum);
        rep
    }

    /// The XDR words `w` as wire bytes.
    fn wire(w: &[u32]) -> Vec<u8> {
        w.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    #[test]
    fn rtt_estimator_follows_the_c_arithmetic() {
        let _g = setup();
        let ticks = NFS_TICKS.swap(1, Relaxed);

        let nmp = leak_mount();
        nfs_init_rtt(nmp);
        assert!(nmp.nm_srtt.iter().all(|t| t.get() == nfs_initrtt()));
        assert!(nmp.nm_sdrtt.iter().all(|t| t.get() == 0));

        // GETATTR uses timer 1, index 0: A + 2D.
        nmp.nm_srtt[0].set(800);
        nmp.nm_sdrtt[0].set(80);
        let rep = leak_req(nmp, NFSPROC_GETATTR);
        rep.r_rtt.set(99);
        nfs_update_rtt(rep);
        // t1 = 100 - 800/8 = 0; srtt stays; t1 = 0 - 80/4 = -20.
        assert_eq!(nmp.nm_srtt[0].get(), 800);
        assert_eq!(nmp.nm_sdrtt[0].get(), 60);
        let rto = nfs_estimate_rto(nmp, NFSPROC_GETATTR);
        assert_eq!(rto, (803 >> 2) + (61 >> 1));
        assert!(rto > nfs_minrto() && rto < nfs_maxrto());

        // A negative error is folded: t1 = 1 - 800/8 = -99, srtt 701, |t1| - 60/4 = 84.
        rep.r_rtt.set(0);
        nfs_update_rtt(rep);
        assert_eq!(nmp.nm_srtt[0].get(), 701);
        assert_eq!(nmp.nm_sdrtt[0].get(), 144);

        // READ (timer 3, index 2): A + 4D, clamped to NFS_MAXRTO.
        nmp.nm_srtt[2].set(1 << 20);
        assert_eq!(nfs_estimate_rto(nmp, NFSPROC_READ), nfs_maxrto());
        nmp.nm_srtt[2].set(0);
        nmp.nm_sdrtt[2].set(0);
        assert_eq!(nfs_estimate_rto(nmp, NFSPROC_READ), nfs_minrto());

        // Other procedures take the mount's timeout as it is.
        nmp.nm_timeo.set(7);
        assert_eq!(nfs_estimate_rto(nmp, NFSPROC_SETATTR), 7);

        NFS_TICKS.store(ticks, Relaxed);
    }

    #[test]
    fn flag_locks_and_interrupts() {
        let nmp = leak_mount();
        let rep = leak_req(nmp, NFSPROC_GETATTR);

        nfs_sndlock(&nmp.nm_flag, Some(rep)).expect("free lock");
        assert_ne!(nmp.nm_flag.get() & NFSMNT_SNDLOCK, 0);
        nfs_sndunlock(&nmp.nm_flag);
        assert_eq!(nmp.nm_flag.get() & NFSMNT_SNDLOCK, 0);

        nfs_rcvlock(rep).expect("free lock");
        assert_ne!(nmp.nm_flag.get() & NFSMNT_RCVLOCK, 0);
        nfs_rcvunlock(&nmp.nm_flag);
        assert_eq!(nmp.nm_flag.get() & NFSMNT_RCVLOCK, 0);

        // The server's lock word works the same way without a request.
        let solock = Cell::new(0);
        nfs_sndlock(&solock, None).expect("free lock");
        assert_eq!(solock.get(), NFSMNT_SNDLOCK);
        nfs_sndunlock(&solock);
        assert_eq!(solock.get(), 0);

        assert_eq!(nfs_sigintr(nmp, Some(rep), None), Ok(()));
        nmp.nm_flag.set(NFSMNT_INT);
        assert_eq!(nfs_sigintr(nmp, Some(rep), None), Ok(()));
        rep.r_flags.set(R_SOFTTERM);
        assert_eq!(nfs_sigintr(nmp, Some(rep), None), Err(Errno::EINTR));
        // A terminated request cannot take a busy lock either.
        nmp.nm_flag.set(NFSMNT_INT | NFSMNT_SNDLOCK);
        assert_eq!(nfs_sndlock(&nmp.nm_flag, Some(rep)), Err(Errno::EINTR));
    }

    /// Eight-byte alignment, the host's `ALIGNED_POINTER(x, void *)` on the real machines.
    fn strict(x: usize) -> bool {
        x % 8 == 0
    }

    #[test]
    fn realign_copies_a_misaligned_chain() {
        let _g = setup();
        assert_eq!(ALIGNBYTES, 7);

        let data: Vec<u8> = (0..29).collect();
        // An aligned first mbuf is kept; the misaligned rest is copied.
        let head = chain(&[&data[..8], &data[8..18], &data[18..21], &data[21..]]);
        let second = head.m_next().get();
        let mut pm = Some(head);
        let before = NFS_REALIGN_COUNT.load(Relaxed);
        realign(&mut pm, strict);
        assert_eq!(NFS_REALIGN_COUNT.load(Relaxed), before + 1);
        let head2 = pm.expect("a chain");
        assert!(ptr::eq(head2, head));
        assert!(!ptr::eq(
            head.m_next().get().expect("rest"),
            second.expect("old rest")
        ));
        assert_eq!(bytes(head2), data);
        let l = lens(head2);
        assert!(l[..l.len() - 1].iter().all(|&n| strict(n)), "{l:?}");
        m_freem(head2);

        // An aligned chain is left alone.
        let head = chain(&[&data[..8], &data[8..24]]);
        let mut pm = Some(head);
        let before = NFS_REALIGN_COUNT.load(Relaxed);
        realign(&mut pm, strict);
        assert_eq!(NFS_REALIGN_COUNT.load(Relaxed), before);
        assert_eq!(lens(pm.expect("chain")), [8, 16]);
        m_freem(pm);

        // A misaligned first mbuf replaces the head.
        let head = chain(&[&data[..5], &data[5..]]);
        let mut pm = Some(head);
        realign(&mut pm, strict);
        let head2 = pm.expect("a chain");
        assert!(!ptr::eq(head2, head));
        assert_eq!(bytes(head2), data);
        m_freem(head2);
    }

    #[test]
    fn rephead_builds_the_reply_header() {
        let _g = setup();
        let mut nd = NfsrvDescript::new();
        nd.nd_retxid = 0x1234;
        let xid = 0x1234u32;

        let reply = |nd: &NfsrvDescript, err: i32| -> Vec<u8> {
            let (mreq, _mb) = nfs_rephead(0, nd, None, err).expect("reply");
            let b = bytes(mreq);
            m_freem(mreq);
            b
        };

        // Success: accepted, null verifier, status 0, then the NFS status word 0.
        assert_eq!(reply(&nd, 0), wire(&[xid, 1, 0, 0, 0, 0, 0]));
        // A void reply has no NFS status word.
        assert_eq!(reply(&nd, NFSERR_RETVOID), wire(&[xid, 1, 0, 0, 0, 0]));
        assert_eq!(
            reply(&nd, Errno::EPROGUNAVAIL.as_i32()),
            wire(&[xid, 1, 0, 0, 0, 1])
        );
        assert_eq!(
            reply(&nd, Errno::EPROGMISMATCH.as_i32()),
            wire(&[xid, 1, 0, 0, 0, 2, 2, 3])
        );
        assert_eq!(
            reply(&nd, Errno::EPROCUNAVAIL.as_i32()),
            wire(&[xid, 1, 0, 0, 0, 3])
        );
        assert_eq!(
            reply(&nd, Errno::EBADRPC.as_i32()),
            wire(&[xid, 1, 0, 0, 0, 4])
        );
        // Denied: RPC version mismatch (low and high version), or an authentication error.
        assert_eq!(
            reply(&nd, Errno::ERPCMISMATCH.as_i32()),
            wire(&[xid, 1, 1, 0, 2, 2])
        );
        assert_eq!(
            reply(&nd, NFSERR_AUTHERR | AUTH_BADCRED as i32),
            wire(&[xid, 1, 1, 1, 1])
        );
    }

    /// A version `vers` call of procedure `proc_` with an AUTH_UNIX credential of uid 1000,
    /// gid 10 and `groups`, as an mbuf chain cut into `cut`-byte mbufs.
    fn call(vers: u32, proc_: u32, groups: &[u32], cut: usize) -> &'static Mbuf {
        let mut w = std::vec![77, 0, 2, 100_003, vers, proc_, 1];
        let cred_len = 5 * 4 + 8 + 4 * groups.len() as u32;
        w.extend([cred_len, 0, 5]);
        let mut b = wire(&w);
        b.extend(b"host\0\0\0\0");
        let mut w = std::vec![1000, 10, groups.len() as u32];
        w.extend(groups);
        w.extend([0, 0]); // AUTH_NULL verifier
        b.extend(wire(&w));
        let parts: Vec<&[u8]> = b.chunks(cut).collect();
        // `chain` copies the parts, so the vector may go.
        chain(&parts)
    }

    /// A descriptor dissecting `m` from its start.
    fn descript(m: &'static Mbuf) -> NfsrvDescript {
        let mut nd = NfsrvDescript::new();
        nd.nd_mrep = Some(m);
        nd.nd_md = Some(m);
        nd.nd_dpos = mtod::<u8>(m);
        nd
    }

    #[test]
    fn getreq_parses_an_auth_unix_call() {
        let _g = setup();

        let mut nd = descript(call(3, 1, &[20, 30], 12));
        nfs_getreq(&mut nd, None, true).expect("a call");
        assert_eq!(nd.nd_retxid, 77);
        assert_eq!(nd.nd_repstat, 0);
        assert_eq!(nd.nd_flag, ND_NFSV3);
        assert_eq!(nd.nd_procnum, NFSPROC_GETATTR);
        assert_eq!(nd.nd_cr.cr_uid.get(), 1000);
        assert_eq!(nd.nd_cr.cr_gid.get(), 10);
        assert_eq!(nd.nd_cr.cr_ngroups.get(), 2);
        assert_eq!(nd.nd_cr.cr_groups[1].get(), 30);
        m_freem(nd.nd_mrep);

        // Version 2 procedure numbers are mapped to version 3's (v2 READ is 6, v3 READ 6;
        // v2 STATFS 17 is v3 FSSTAT 18).
        let mut nd = descript(call(2, 17, &[], 7));
        nfs_getreq(&mut nd, None, true).expect("a call");
        assert_eq!(nd.nd_flag, 0);
        assert_eq!(nd.nd_procnum, NFSV3_PROCID[17]);
        m_freem(nd.nd_mrep);

        // RPC errors are reported in nd_repstat with the NOOP procedure.
        let mut nd = descript(call(4, 1, &[], 40));
        nfs_getreq(&mut nd, None, true).expect("a call");
        assert_eq!(nd.nd_repstat, Errno::EPROGMISMATCH.as_i32());
        assert_eq!(nd.nd_procnum, NFSPROC_NOOP);
        m_freem(nd.nd_mrep);

        let mut nd = descript(call(2, 18, &[], 40));
        nfs_getreq(&mut nd, None, true).expect("a call");
        assert_eq!(nd.nd_repstat, Errno::EPROCUNAVAIL.as_i32());
        m_freem(nd.nd_mrep);

        // A reply instead of a call is garbage; the request is freed.
        let m = call(3, 1, &[], 40);
        // SAFETY: the second word of the first mbuf's data (40 bytes long).
        unsafe { mtod::<u8>(m).add(7).write(1) };
        let mut nd = descript(m);
        assert_eq!(nfs_getreq(&mut nd, None, true), Err(Errno::EBADRPC));
        assert!(nd.nd_mrep.is_none());
    }

    #[cfg(feature = "nfsserver")]
    mod stream {
        use super::*;

        /// A server socket that lives for the rest of the test run.
        fn leak_slp() -> &'static NfssvcSock {
            Box::leak(Box::new(NfssvcSock::new()))
        }

        /// The records queued on `slp`, each as bytes.
        fn records(slp: &NfssvcSock) -> Vec<Vec<u8>> {
            let mut v = Vec::new();
            let mut r = slp.ns_rec.get();
            while let Some(m) = r {
                v.push(bytes(m));
                r = m.m_nextpkt().get();
            }
            v
        }

        #[test]
        fn getstream_splits_records_and_joins_fragments() {
            let _g = setup();
            let rec1: Vec<u8> = (1..=8).collect();
            let frag_a: Vec<u8> = (11..=14).collect();
            let frag_b: Vec<u8> = (15..=18).collect();
            let mut s = Vec::new();
            s.extend(0x8000_0008u32.to_be_bytes());
            s.extend(&rec1);
            s.extend(4u32.to_be_bytes()); // not the last fragment
            s.extend(&frag_a);
            s.extend(0x8000_0004u32.to_be_bytes());
            s.extend(&frag_b);
            s.extend(0x8000_000cu32.to_be_bytes());
            s.extend([21, 22, 23, 24, 25]); // 5 of 12 bytes so far

            let parts: Vec<&[u8]> = s.chunks(3).collect();
            let raw = chain(&parts);
            let slp = leak_slp();
            slp.ns_raw.set(Some(raw));
            let mut last = raw;
            while let Some(n) = last.m_next().get() {
                last = n;
            }
            slp.ns_rawend.set(Some(last));
            slp.ns_cc.set(s.len() as i32);

            assert_eq!(nfsrv_getstream(slp, M_WAIT), Ok(()));
            assert_eq!(slp.ns_flag.get() & SLP_GETSTREAM, 0);
            let mut rec2 = frag_a.clone();
            rec2.extend(&frag_b);
            assert_eq!(records(slp), [rec1, rec2]);
            assert!(slp.ns_frag.get().is_none());
            assert_eq!(slp.ns_reclen.get(), 12);
            assert_eq!(slp.ns_cc.get(), 5);
            assert_eq!(
                bytes(slp.ns_raw.get().expect("partial record")),
                [21, 22, 23, 24, 25]
            );
            assert_ne!(slp.ns_flag.get() & SLP_LASTFRAG, 0);

            // The record mark of a record too big for NFS disconnects.
            let big = (0x8000_0000u32 | (NFS_MAXPACKET as u32 + 1)).to_be_bytes();
            let slp = leak_slp();
            slp.ns_raw.set(Some(chain(&[&big[..2], &big[2..]])));
            slp.ns_cc.set(4);
            assert_eq!(nfsrv_getstream(slp, M_WAIT), Err(Errno::EPERM));
            assert_eq!(slp.ns_flag.get() & SLP_GETSTREAM, 0);
        }
    }
}
/* </TESTS> */
