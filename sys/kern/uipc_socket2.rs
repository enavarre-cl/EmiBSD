/*	$OpenBSD: uipc_socket2.c,v 1.186 2025/07/14 21:47:26 bluhm Exp $	*/
/*	$NetBSD: uipc_socket2.c,v 1.11 1996/02/04 02:17:55 christos Exp $	*/
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
 *	@(#)uipc_socket2.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! Primitive routines for operating on sockets and socket buffers: `kern/uipc_socket2.c`.
//!
//! Upstream: sys/kern/uipc_socket2.c @ 3ce1f3f79392
//!
//! The state routines (`soisconnecting`, `soisconnected`, `soisdisconnecting`,
//! `soisdisconnected`) move a socket through its connection states and wake whoever waits.
//! Normal sequence from the active (originating) side is that `soisconnecting()` is called
//! during processing of `connect()` call, resulting in an eventual call to `soisconnected()`
//! if/when the connection is established. When the connection is torn down
//! `soisdisconnecting()` is called during processing of `disconnect()` call, and
//! `soisdisconnected()` is called when the connection to the peer is totally severed.
//! Connectionless protocols can call `soisconnected()` and `soisdisconnected()` only,
//! bypassing the in-progress calls when setting up a "connection" takes no time.
//!
//! From the passive side, a socket is created with two queues of sockets: `so_q0` for
//! connections in progress and `so_q` for connections already made and awaiting user
//! acceptance. As a protocol is preparing incoming connections, it creates a socket
//! structure queued on `so_q0` by calling `sonewconn()`. When the connection is established,
//! `soisconnected()` is called, and transfers the socket structure to `so_q`, making it
//! available to `accept()`. If a socket is closed with sockets on either `so_q0` or `so_q`,
//! these sockets are dropped.
//!
//! The socket buffer routines keep each buffer a list of records (chained through
//! `m_nextpkt`), each record a chain of mbufs (through `m_next`): an optional address
//! (`MT_SONAME`), optional control (`MT_CONTROL`), then data. `sbappend*` add, `sbdrop*`
//! remove, `sbreserve` sets the limits `sbspace` checks against.
//!
//! The lock routines (`solock` and friends) take the net lock for the internet domains and
//! the socket's own `so_lock` for the others.
//!
//! ## Deviations
//! - `soqremque`, `sbappendaddr`, `sbappendcontrol` and `sbchecklowmem` return `bool` (the
//!   C's 1 or 0); `sbreserve` and `sbcheckreserve` return `Result` with `ENOBUFS` for the C's
//!   non-zero; `sonewconn` returns `Option`.
//! - `sbappendaddr` takes the address as its bytes (`asa.sa_len` of them, the first byte
//!   being `sa_len`), as the C copies them with `memcpy`.
//! - `sowakeup`'s `knote_locked(&sb->sb_klist, 0)` and `sonewconn`'s `klist_free`s are
//!   `kern_event.c`'s: the first is reported at its site, the frees are comments (there is no
//!   `sb_klist`, see `sys/socketvar.rs`).
//! - `SOCKBUF_DEBUG` is not configured: `sblastrecordchk`, `sblastmbufchk` and `sbcheck`
//!   are ported but, as in GENERIC, nothing calls them.
//! - `sosleep_nsec` takes the wait channel as a raw pointer, as `rwsleep_nsec` does.
//! - `sbcreatecontrol` takes the data as a byte slice (the C's pointer and size).

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_rwlock::{
    rw_assert_unlocked, rw_assert_wrlock, rw_enter, rw_enter_write, rw_exit, rw_exit_write,
    rw_status,
};
use crate::kern::kern_sig::{pgsigio, sigio_copy, sigio_free};
use crate::kern::kern_synch::{msleep_nsec, rwsleep_nsec, wakeup, wakeup_one};
use crate::kern::subr_pool::pool_put;
use crate::kern::subr_prf::{SPLASSERT_CTL, panic, printf, splassert_fail};
use crate::kern::uipc_mbuf::{m_free, m_get, m_pool_used, m_trailingspace};
use crate::kern::uipc_socket::{SOCKET_POOL, soalloc, sorele, sorwakeup, sowwakeup};
use crate::net::if_::NETLOCK;
use crate::sys::errno::Errno;
use crate::sys::mbuf::{
    M_DONTWAIT, M_EOR, M_EXT, M_PKTHDR, MAXMCLBYTES, MCLBYTES, MLEN, MSIZE, MT_CONTROL, MT_SONAME,
    Mbuf, mclget, mtod,
};
use crate::sys::mutex::mutex_assert_locked;
use crate::sys::param::{PCATCH, PSOCK};
use crate::sys::protosw::pru_attach;
use crate::sys::rwlock::{RW_INTR, RW_NOSLEEP, RW_READ, RW_WRITE};
use crate::sys::signal::SIGIO;
use crate::sys::socket::{
    Cmsghdr, PF_INET, PF_INET6, SO_ACCEPTCONN, cmsg_align, cmsg_len, cmsg_space,
};
use crate::sys::socketvar::{
    SB_ASYNC, SB_MAX, SB_NOINTR, SB_WAIT, SBL_NOINTR, SBL_WAIT, SS_CANTRCVMORE, SS_CANTSENDMORE,
    SS_ISCONNECTED, SS_ISCONNECTING, SS_ISDISCONNECTED, SS_ISDISCONNECTING, SS_NOFDREF, Sockbuf,
    Socket, sb_empty_fixup, sballoc, sbfree, sbspace_locked, soref,
};
use crate::sys::systm::{
    net_assert_locked, net_assert_locked_exclusive, net_lock, net_lock_shared, net_unlock,
    net_unlock_shared,
};
use crate::{kassert, unported};

/// \[I\] `sb_max`: patchable.
pub static SB_MAX_VAR: AtomicU64 = AtomicU64::new(SB_MAX);

/// `sbchecklowmem`'s `static int sblowmem`.
static SBLOWMEM: AtomicI32 = AtomicI32::new(0);

/// Whether `so` belongs to an internet domain, whose sockets the net lock serialises.
fn is_inet(so: &Socket) -> bool {
    let family = so.dom_family();
    family == i32::from(PF_INET) || family == i32::from(PF_INET6)
}

/// `m->m_type != MT_CONTROL && m->m_type != MT_SONAME`: `m` counts in `sb_datacc`.
fn is_data(m: &Mbuf) -> bool {
    let t = i32::from(m.m_type().get());
    t != MT_CONTROL && t != MT_SONAME
}

/// `so->so_head`, which a queued socket has.
fn head_of(so: &Socket) -> &'static Socket {
    match so.so_head.get() {
        Some(head) => head,
        None => panic(format_args!("socket {:p}: no so_head", so)),
    }
}

/// `soisconnecting(so)`: `so` is in process of connecting to its peer.
pub fn soisconnecting(so: &Socket) {
    soassertlocked(so);
    so.clear_state(SS_ISCONNECTED | SS_ISDISCONNECTING);
    so.set_state(SS_ISCONNECTING);
}

/// `soisconnected(so)`: `so` is connected; a connection on a listener's `so_q0` moves to its
/// `so_q`, where `accept(2)` finds it.
pub fn soisconnected(so: &'static Socket) {
    let head = so.so_head.get();

    soassertlocked(so);
    so.clear_state(SS_ISCONNECTING | SS_ISDISCONNECTING);
    so.set_state(SS_ISCONNECTED);

    if let Some(head) = head
        && so.onq_is(&head.so_q0)
    {
        let _ = soref(Some(head));
        sounlock(so);
        solock(head);
        solock(so);

        if !so.onq_is(&head.so_q0) {
            sounlock(head);
            sorele(head);
            return;
        }

        soqremque(so, 0);
        soqinsque(head, so, 1);
        sorwakeup(head);
        wakeup_one(head.timeo_chan());

        sounlock(head);
        sorele(head);
    } else {
        wakeup(so.timeo_chan());
        sorwakeup(so);
        sowwakeup(so);
    }
}

/// `soisdisconnecting(so)`: `so` is in process of disconnecting: it can neither send nor
/// receive any more.
pub fn soisdisconnecting(so: &'static Socket) {
    soassertlocked(so);
    so.clear_state(SS_ISCONNECTING);
    so.set_state(SS_ISDISCONNECTING);

    mtx_enter(&so.so_rcv.sb_mtx);
    so.so_rcv.set_state(SS_CANTRCVMORE);
    mtx_leave(&so.so_rcv.sb_mtx);

    mtx_enter(&so.so_snd.sb_mtx);
    so.so_snd.set_state(SS_CANTSENDMORE);
    mtx_leave(&so.so_snd.sb_mtx);

    wakeup(so.timeo_chan());
    sowwakeup(so);
    sorwakeup(so);
}

/// `soisdisconnected(so)`: the connection to the peer is totally severed.
pub fn soisdisconnected(so: &'static Socket) {
    soassertlocked(so);

    mtx_enter(&so.so_rcv.sb_mtx);
    so.so_rcv.set_state(SS_CANTRCVMORE);
    mtx_leave(&so.so_rcv.sb_mtx);

    mtx_enter(&so.so_snd.sb_mtx);
    so.so_snd.set_state(SS_CANTSENDMORE);
    mtx_leave(&so.so_snd.sb_mtx);

    so.clear_state(SS_ISCONNECTING | SS_ISCONNECTED | SS_ISDISCONNECTING);
    so.set_state(SS_ISDISCONNECTED);

    wakeup(so.timeo_chan());
    sowwakeup(so);
    sorwakeup(so);
}

/// When an attempt at a new connection is noted on a socket which accepts connections,
/// `sonewconn` is called. If the connection is possible (subject to space constraints, etc.)
/// then we allocate a new structure, properly linked into the data structure of the original
/// socket, and return this, locked. `connstatus` may be 0 or `SS_ISCONNECTED`.
pub fn sonewconn(head: &'static Socket, connstatus: u32, wait: i32) -> Option<&'static Socket> {
    let soqueue = if connstatus != 0 { 1 } else { 0 };

    soassertlocked(head);

    if m_pool_used() > 95 {
        return None;
    }
    if i32::from(head.so_qlen.get()) + i32::from(head.so_q0len.get())
        > i32::from(head.so_qlimit.get()) * 3
    {
        return None;
    }
    let so = soalloc(head.so_proto, wait)?;
    so.so_type.set(head.so_type.get());
    so.so_options.set(head.so_options.get() & !SO_ACCEPTCONN);
    so.so_linger.set(head.so_linger.get());
    so.so_state.set(head.so_state.get() | SS_NOFDREF);
    so.so_timeo.set(head.so_timeo.get());
    so.so_euid.set(head.so_euid.get());
    so.so_ruid.set(head.so_ruid.get());
    so.so_egid.set(head.so_egid.get());
    so.so_rgid.set(head.so_rgid.get());
    so.so_cpid.set(head.so_cpid.get());

    // Lock order will be `head' -> `so' while these sockets are linked.
    solock_nonet(so);

    'fail: {
        // Inherit watermarks but those may get clamped in low mem situations.
        if soreserve(so, head.so_snd.sb_hiwat.get(), head.so_rcv.sb_hiwat.get()).is_err() {
            break 'fail;
        }

        mtx_enter(&head.so_snd.sb_mtx);
        so.so_snd.sb_wat.set(head.so_snd.sb_wat.get());
        so.so_snd.sb_lowat.set(head.so_snd.sb_lowat.get());
        so.so_snd
            .sb_timeo_nsecs
            .set(head.so_snd.sb_timeo_nsecs.get());
        mtx_leave(&head.so_snd.sb_mtx);

        mtx_enter(&head.so_rcv.sb_mtx);
        so.so_rcv.sb_wat.set(head.so_rcv.sb_wat.get());
        so.so_rcv.sb_lowat.set(head.so_rcv.sb_lowat.get());
        so.so_rcv
            .sb_timeo_nsecs
            .set(head.so_rcv.sb_timeo_nsecs.get());
        mtx_leave(&head.so_rcv.sb_mtx);

        sigio_copy(&so.so_sigio, &head.so_sigio);

        soqinsque(head, so, soqueue);
        if pru_attach(so, 0, wait).is_err() {
            soqremque(so, soqueue);
            break 'fail;
        }
        if connstatus != 0 {
            so.set_state(connstatus);
            sorwakeup(head);
            wakeup(head.timeo_chan());
        }

        return Some(so);
    }

    // fail:
    sounlock_nonet(so);
    sigio_free(&so.so_sigio);
    // klist_free(&so->so_rcv.sb_klist), klist_free(&so->so_snd.sb_klist): kern_event.c.
    pool_put(&SOCKET_POOL, NonNull::from(so).cast());

    None
}

/// `soqinsque(head, so, q)`: queues `so` on `head`'s partial (`q` 0) or complete queue.
pub fn soqinsque(head: &'static Socket, so: &'static Socket, q: i32) {
    soassertlocked(head);
    soassertlocked(so);

    kassert!(so.so_onq.get().is_null());

    so.so_head.set(Some(head));
    let onq = if q == 0 {
        head.so_q0len.set(head.so_q0len.get() + 1);
        &head.so_q0
    } else {
        head.so_qlen.set(head.so_qlen.get() + 1);
        &head.so_q
    };
    so.so_onq.set(onq);
    // SAFETY: both sockets are locked; `so` is a pool item that stays in place while it is
    // queued (`soqremque` unlinks it before it can be freed) and is on no other queue.
    unsafe { onq.insert_tail(so) };
}

/// `soqremque(so, q)`: takes `so` off its head's partial (`q` 0) or complete queue; `false`
/// if it is not on that queue.
pub fn soqremque(so: &'static Socket, q: i32) -> bool {
    let head = head_of(so);

    soassertlocked(so);
    soassertlocked(head);

    let onq = if q == 0 {
        if !so.onq_is(&head.so_q0) {
            return false;
        }
        head.so_q0len.set(head.so_q0len.get() - 1);
        &head.so_q0
    } else {
        if !so.onq_is(&head.so_q) {
            return false;
        }
        head.so_qlen.set(head.so_qlen.get() - 1);
        &head.so_q
    };
    // SAFETY: `so` is on `onq` (checked above); both sockets are locked.
    unsafe { onq.remove(so) };
    so.so_onq.set(ptr::null());
    so.so_head.set(None);
    true
}

/// `socantsendmore(so)` indicates that no more data will be sent on the socket; it would
/// normally be applied to a socket when the user informs the system that no more data is to
/// be sent, by the protocol code (in case `PRU_SHUTDOWN`).
pub fn socantsendmore(so: &'static Socket) {
    soassertlocked(so);
    mtx_enter(&so.so_snd.sb_mtx);
    so.so_snd.set_state(SS_CANTSENDMORE);
    mtx_leave(&so.so_snd.sb_mtx);
    sowwakeup(so);
}

/// `socantrcvmore(so)` indicates that no more data will be received, and will normally be
/// applied to the socket by a protocol when it detects that the peer will send no more
/// data. Data queued for reading in the socket may yet be read.
pub fn socantrcvmore(so: &'static Socket) {
    mtx_enter(&so.so_rcv.sb_mtx);
    so.so_rcv.set_state(SS_CANTRCVMORE);
    mtx_leave(&so.so_rcv.sb_mtx);
    sorwakeup(so);
}

/// `solock(so)`: the net lock for the internet domains, the socket's lock for the others.
pub fn solock(so: &Socket) {
    if is_inet(so) {
        net_lock();
    } else {
        rw_enter_write(&so.so_lock);
    }
}

/// `solock_shared(so)`: the shared net lock (internet domains) and the socket's lock.
pub fn solock_shared(so: &Socket) {
    if is_inet(so) {
        net_lock_shared();
    }
    rw_enter_write(&so.so_lock);
}

/// `solock_nonet(so)`: the socket's lock; an internet socket's caller holds the net lock.
pub fn solock_nonet(so: &Socket) {
    if is_inet(so) {
        net_assert_locked("solock_nonet");
    }
    rw_enter_write(&so.so_lock);
}

/// `solock_persocket(so)`: whether `so` has a lock of its own (it is not an internet one).
pub fn solock_persocket(so: &Socket) -> bool {
    !is_inet(so)
}

/// `solock_pair(so1, so2)`: locks two sockets of one type, in address order.
pub fn solock_pair(so1: &Socket, so2: &Socket) {
    kassert!(so1.so_type.get() == so2.so_type.get());

    if is_inet(so1) {
        net_lock_shared();
    }
    if ptr::eq(so1, so2) {
        rw_enter_write(&so1.so_lock);
    } else if ptr::from_ref(so1) < ptr::from_ref(so2) {
        rw_enter_write(&so1.so_lock);
        rw_enter_write(&so2.so_lock);
    } else {
        rw_enter_write(&so2.so_lock);
        rw_enter_write(&so1.so_lock);
    }
}

/// `sounlock(so)`: undoes `solock`.
pub fn sounlock(so: &Socket) {
    if is_inet(so) {
        net_unlock();
    } else {
        rw_exit_write(&so.so_lock);
    }
}

/// `sounlock_shared(so)`: undoes `solock_shared`.
pub fn sounlock_shared(so: &Socket) {
    if is_inet(so) {
        net_unlock_shared();
    }
    rw_exit_write(&so.so_lock);
}

/// `sounlock_nonet(so)`: undoes `solock_nonet`.
pub fn sounlock_nonet(so: &Socket) {
    rw_exit_write(&so.so_lock);
}

/// `sounlock_pair(so1, so2)`: undoes `solock_pair`.
pub fn sounlock_pair(so1: &Socket, so2: &Socket) {
    if is_inet(so1) {
        net_unlock_shared();
    }
    if ptr::eq(so1, so2) {
        rw_exit_write(&so1.so_lock);
    } else if ptr::from_ref(so1) < ptr::from_ref(so2) {
        rw_exit_write(&so2.so_lock);
        rw_exit_write(&so1.so_lock);
    } else {
        rw_exit_write(&so1.so_lock);
        rw_exit_write(&so2.so_lock);
    }
}

/// `soassertlocked_readonly(so)`: enough of the socket's locks are held to read its state.
pub fn soassertlocked_readonly(so: &Socket) {
    if is_inet(so) {
        net_assert_locked("soassertlocked_readonly");
    } else {
        rw_assert_wrlock(&so.so_lock);
    }
}

/// `soassertlocked(so)`: the socket's locks are held to change its state.
pub fn soassertlocked(so: &Socket) {
    if is_inet(so) {
        if rw_status(&NETLOCK) == RW_READ {
            net_assert_locked("soassertlocked");

            if SPLASSERT_CTL.load(Ordering::Relaxed) > 0 && rw_status(&so.so_lock) != RW_WRITE {
                splassert_fail(0, RW_WRITE, "soassertlocked");
            }
        } else {
            net_assert_locked_exclusive("soassertlocked");
        }
    } else {
        rw_assert_wrlock(&so.so_lock);
    }
}

/// `sosleep_nsec(so, ident, prio, wmesg, nsecs)`: sleeps on `ident`, releasing the lock
/// that serialises `so` while asleep.
pub fn sosleep_nsec(
    so: &Socket,
    ident: *const c_void,
    prio: i32,
    wmesg: &'static str,
    nsecs: u64,
) -> Result<(), Errno> {
    if is_inet(so) {
        if rw_status(&NETLOCK) == RW_READ {
            rw_exit_write(&so.so_lock);
        }
        let ret = rwsleep_nsec(ident, &NETLOCK, prio, wmesg, nsecs);
        if rw_status(&NETLOCK) == RW_READ {
            rw_enter_write(&so.so_lock);
        }
        ret
    } else {
        rwsleep_nsec(ident, &so.so_lock, prio, wmesg, nsecs)
    }
}

/// `sbmtxassertlocked(sb)`: the buffer's mutex is held.
pub fn sbmtxassertlocked(sb: &Sockbuf) {
    mutex_assert_locked(&sb.sb_mtx, "sbmtxassertlocked");
}

/// Wait for data to arrive at/drain from a socket buffer. Called and returns with the
/// buffer's mutex held.
pub fn sbwait(sb: &Sockbuf) -> Result<(), Errno> {
    let prio = if sb.has_flags(SB_NOINTR) {
        PSOCK
    } else {
        PSOCK | PCATCH
    };

    mutex_assert_locked(&sb.sb_mtx, "sbwait");

    sb.set_flags(SB_WAIT);
    msleep_nsec(
        sb.cc_chan(),
        &sb.sb_mtx,
        prio,
        "sbwait",
        sb.sb_timeo_nsecs.get(),
    )
}

/// `sblock(sb, flags)`: serialises the readers (or writers) of `sb`; `EWOULDBLOCK` without
/// `SBL_WAIT` when another holds it.
pub fn sblock(sb: &Sockbuf, flags: i32) -> Result<(), Errno> {
    let mut rwflags = RW_WRITE;

    if !(flags & SBL_NOINTR != 0 || sb.has_flags(SB_NOINTR)) {
        rwflags |= RW_INTR;
    }
    if flags & SBL_WAIT == 0 {
        rwflags |= RW_NOSLEEP;
    }

    rw_enter(&sb.sb_lock, rwflags).map_err(|error| {
        if error == Errno::EBUSY {
            Errno::EWOULDBLOCK
        } else {
            error
        }
    })
}

/// `sbunlock(sb)`: undoes `sblock`.
pub fn sbunlock(sb: &Sockbuf) {
    rw_exit(&sb.sb_lock);
}

/// Wakeup processes waiting on a socket buffer. Do asynchronous notification via `SIGIO` if
/// the socket buffer has the `SB_ASYNC` flag set.
pub fn sowakeup(so: &Socket, sb: &Sockbuf) {
    let mut dowakeup = false;
    let mut dopgsigio = false;

    mtx_enter(&sb.sb_mtx);
    if sb.has_flags(SB_WAIT) {
        sb.clear_flags(SB_WAIT);
        dowakeup = true;
    }
    if sb.has_flags(SB_ASYNC) {
        dopgsigio = true;
    }

    // knote_locked(&sb->sb_klist, 0): the kqueue, select and poll waiters (kern_event.c).
    let _ = unported!("sowakeup: knote_locked (kern_event.c)");
    mtx_leave(&sb.sb_mtx);

    if dowakeup {
        wakeup(sb.cc_chan());
    }

    if dopgsigio {
        pgsigio(&so.so_sigio, SIGIO, false);
    }
}

// Socket buffer (struct sockbuf) utility routines.
//
// Each socket contains two socket buffers: one for sending data and one for receiving data.
// Each buffer contains a queue of mbufs, information about the number of mbufs and amount of
// data in the queue, and other fields allowing select() statements and notification on data
// availability to be implemented.
//
// Data stored in a socket buffer is maintained as a list of records. Each record is a list of
// mbufs chained together with the m_next field. Records are chained together with the
// m_nextpkt field. The upper level routine soreceive() expects the following conventions to
// be observed when placing information in the receive buffer:
//
// 1. If the protocol requires each message be preceded by the sender's name, then a record
//    containing that name must be present before any associated data (mbuf's must be of type
//    MT_SONAME).
// 2. If the protocol supports the exchange of ``access rights'' (really just additional data
//    associated with the message), and there are ``rights'' to be received, then a record
//    containing this data should be present (mbuf's must be of type MT_CONTROL).
// 3. If a name or rights record exists, then it must be followed by a data record, perhaps
//    of zero length.
//
// Before using a new socket structure it is first necessary to reserve buffer space to the
// socket, by calling sbreserve(). This should commit some of the available buffer space in
// the system buffer pool for the socket (currently, it does nothing but enforce limits). The
// space should be released by calling sbrelease() when the socket is destroyed.

/// `soreserve(so, sndcc, rcvcc)`: reserves `sndcc` bytes of send buffer and `rcvcc` of
/// receive buffer and sets their low-water marks; `ENOBUFS` past `sb_max`.
pub fn soreserve(so: &Socket, sndcc: u64, rcvcc: u64) -> Result<(), Errno> {
    soassertlocked(so);

    mtx_enter(&so.so_rcv.sb_mtx);
    mtx_enter(&so.so_snd.sb_mtx);
    'bad: {
        if sbreserve(&so.so_snd, sndcc).is_err() {
            break 'bad;
        }
        so.so_snd.sb_wat.set(sndcc);
        if so.so_snd.sb_lowat.get() == 0 {
            so.so_snd.sb_lowat.set(MCLBYTES as i64);
        }
        if so.so_snd.sb_lowat.get() > so.so_snd.sb_hiwat.get() as i64 {
            so.so_snd.sb_lowat.set(so.so_snd.sb_hiwat.get() as i64);
        }
        if sbreserve(&so.so_rcv, rcvcc).is_err() {
            // bad2:
            sbrelease(&so.so_snd);
            break 'bad;
        }
        so.so_rcv.sb_wat.set(rcvcc);
        if so.so_rcv.sb_lowat.get() == 0 {
            so.so_rcv.sb_lowat.set(1);
        }
        mtx_leave(&so.so_snd.sb_mtx);
        mtx_leave(&so.so_rcv.sb_mtx);

        return Ok(());
    }
    // bad:
    mtx_leave(&so.so_snd.sb_mtx);
    mtx_leave(&so.so_rcv.sb_mtx);
    Err(Errno::ENOBUFS)
}

/// Allot mbufs to a sockbuf. Attempt to scale `mbmax` so that `mbcnt` doesn't become
/// limiting if buffering efficiency is near the normal case.
pub fn sbreserve(sb: &Sockbuf, cc: u64) -> Result<(), Errno> {
    sbmtxassertlocked(sb);

    if cc == 0 || cc > SB_MAX_VAR.load(Ordering::Relaxed) {
        return Err(Errno::ENOBUFS);
    }
    sb.sb_hiwat.set(cc);
    sb.sb_mbmax.set((3 * MAXMCLBYTES as u64).max(cc * 8));
    if sb.sb_lowat.get() > sb.sb_hiwat.get() as i64 {
        sb.sb_lowat.set(sb.sb_hiwat.get() as i64);
    }
    Ok(())
}

/// In low memory situation, do not accept any greater than normal request.
pub fn sbcheckreserve(cnt: u64, defcnt: u64) -> Result<(), Errno> {
    if cnt > defcnt && sbchecklowmem() {
        return Err(Errno::ENOBUFS);
    }
    Ok(())
}

/// `sbchecklowmem()`: whether the mbuf pools are short of memory (with hysteresis between
/// 60 and 80 percent used).
pub fn sbchecklowmem() -> bool {
    // m_pool_used() is thread safe. Global variable sblowmem is updated by multiple CPUs, but
    // most times with the same value. And even if the value is not correct for a short time,
    // it does not matter.
    let used = m_pool_used();
    if used < 60 {
        SBLOWMEM.store(0, Ordering::Relaxed);
    } else if used > 80 {
        SBLOWMEM.store(1, Ordering::Relaxed);
    }

    SBLOWMEM.load(Ordering::Relaxed) != 0
}

/// Free mbufs held by a socket, and reserved mbuf space.
pub fn sbrelease(sb: &Sockbuf) {
    sbflush(sb);
    sb.sb_hiwat.set(0);
    sb.sb_mbmax.set(0);
}

// Routines to add and remove data from an mbuf queue.
//
// The routines sbappend() or sbappendrecord() are normally called to append new mbufs to a
// socket buffer, after checking that adequate space is available, comparing the function
// sbspace() with the amount of data to be added. sbappendrecord() differs from sbappend() in
// that data supplied is treated as the beginning of a new record. To place a sender's
// address, optional access rights, and data in a socket receive buffer, sbappendaddr() should
// be used. To place access rights and data in a socket receive buffer, sbappendrights()
// should be used. In either case, the new data begins a new record. Note that unlike
// sbappend() and sbappendrecord(), these routines check for the caller that there will be
// enough space to store the data. Each fails if there is not enough space, or if it cannot
// find mbufs to store additional information in.
//
// Reliable protocols may use the socket send buffer to hold data awaiting acknowledgement.
// Data is normally copied from a socket send buffer in a protocol with m_copym for output to
// a peer, and then removing the data from the socket buffer with sbdrop() or sbdroprecord()
// when the data is acknowledged by the peer.

/// Whether two optional mbufs are the same one.
fn same(a: Option<&Mbuf>, b: Option<&Mbuf>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `sblastrecordchk(sb, where)` (`SOCKBUF_DEBUG`): `sb_lastrecord` is the last record.
pub fn sblastrecordchk(sb: &Sockbuf, where_: &str) {
    let mut m = sb.sb_mb.get();

    while let Some(next) = m.and_then(|mm| mm.m_nextpkt().get()) {
        m = Some(next);
    }

    if !same(m, sb.sb_lastrecord.get()) {
        printf(format_args!(
            "sblastrecordchk: sb_mb {:?} sb_lastrecord {:?} last {:?}\n",
            sb.sb_mb.get().map(ptr::from_ref),
            sb.sb_lastrecord.get().map(ptr::from_ref),
            m.map(ptr::from_ref)
        ));
        printf(format_args!("packet chain:\n"));
        let mut m = sb.sb_mb.get();
        while let Some(mm) = m {
            printf(format_args!("\t{:p}\n", mm));
            m = mm.m_nextpkt().get();
        }
        panic(format_args!("sblastrecordchk from {}", where_));
    }
}

/// `sblastmbufchk(sb, where)` (`SOCKBUF_DEBUG`): `sb_mbtail` is the last mbuf of the last
/// record.
pub fn sblastmbufchk(sb: &Sockbuf, where_: &str) {
    let mut m = sb.sb_mb.get();

    while let Some(next) = m.and_then(|mm| mm.m_nextpkt().get()) {
        m = Some(next);
    }

    while let Some(next) = m.and_then(|mm| mm.m_next().get()) {
        m = Some(next);
    }

    if !same(m, sb.sb_mbtail.get()) {
        printf(format_args!(
            "sblastmbufchk: sb_mb {:?} sb_mbtail {:?} last {:?}\n",
            sb.sb_mb.get().map(ptr::from_ref),
            sb.sb_mbtail.get().map(ptr::from_ref),
            m.map(ptr::from_ref)
        ));
        printf(format_args!("packet tree:\n"));
        let mut m = sb.sb_mb.get();
        while let Some(mm) = m {
            printf(format_args!("\t"));
            let mut n = Some(mm);
            while let Some(nn) = n {
                printf(format_args!("{:p} ", nn));
                n = nn.m_next().get();
            }
            printf(format_args!("\n"));
            m = mm.m_nextpkt().get();
        }
        panic(format_args!("sblastmbufchk from {}", where_));
    }
}

/// `SBLINKRECORD(sb, m0)`: links the record `m0` after the last one.
fn sblinkrecord(sb: &Sockbuf, m0: &'static Mbuf) {
    match sb.sb_lastrecord.get() {
        Some(last) => last.m_nextpkt().set(Some(m0)),
        None => sb.sb_mb.set(Some(m0)),
    }
    sb.sb_lastrecord.set(Some(m0));
}

/// Append mbuf chain `m` to the last record in the socket buffer `sb`. The additional space
/// associated the mbuf chain is recorded in `sb`. Empty mbufs are discarded and mbufs are
/// compacted where possible.
pub fn sbappend(sb: &Sockbuf, m: Option<&'static Mbuf>) {
    let Some(m) = m else {
        return;
    };

    sbmtxassertlocked(sb);

    let mut n = sb.sb_lastrecord.get();
    if let Some(mut nn) = n {
        // XXX Would like to simply use sb_mbtail here, but XXX I need to verify that I won't
        // miss an EOR that XXX way.
        loop {
            if nn.m_flags().get() & M_EOR != 0 {
                sbappendrecord(sb, Some(m)); // XXXXXX!!!!
                return;
            }
            match nn.m_next().get() {
                Some(next) => nn = next,
                None => break,
            }
        }
        n = Some(nn);
    } else {
        // If this is the first record in the socket buffer, it's also the last record.
        sb.sb_lastrecord.set(Some(m));
    }
    sbcompress(sb, Some(m), n);
}

/// This version of `sbappend()` should only be used when the caller absolutely knows that
/// there will never be more than one record in the socket buffer, that is, a stream protocol
/// (such as TCP).
pub fn sbappendstream(sb: &Sockbuf, m: &'static Mbuf) {
    sbmtxassertlocked(sb);
    crate::kdassert!(m.m_nextpkt().get().is_none());
    kassert!(same(sb.sb_mb.get(), sb.sb_lastrecord.get()));

    sbcompress(sb, Some(m), sb.sb_mbtail.get());

    sb.sb_lastrecord.set(sb.sb_mb.get());
}

/// `sbcheck(so, sb)` (`SOCKBUF_DEBUG`): the counters match the chain.
pub fn sbcheck(_so: &Socket, sb: &Sockbuf) {
    let mut len: u64 = 0;
    let mut mbcnt: u64 = 0;

    let mut m = sb.sb_mb.get();
    while let Some(mm) = m {
        let mut n = Some(mm);
        while let Some(nn) = n {
            len += u64::from(nn.m_len().get());
            mbcnt += MSIZE as u64;
            if nn.m_flags().get() & M_EXT != 0 {
                mbcnt += u64::from(nn.m_ext().ext_size.get());
            }
            if !ptr::eq(mm, nn) && nn.m_nextpkt().get().is_some() {
                panic(format_args!("sbcheck nextpkt"));
            }
            n = nn.m_next().get();
        }
        m = mm.m_nextpkt().get();
    }
    if len != sb.sb_cc.get() || mbcnt != sb.sb_mbcnt.get() {
        printf(format_args!(
            "cc {} != {} || mbcnt {} != {}\n",
            len,
            sb.sb_cc.get(),
            mbcnt,
            sb.sb_mbcnt.get()
        ));
        panic(format_args!("sbcheck"));
    }
}

/// As above, except the mbuf chain begins a new record.
pub fn sbappendrecord(sb: &Sockbuf, m0: Option<&'static Mbuf>) {
    sbmtxassertlocked(sb);

    let Some(m0) = m0 else {
        return;
    };

    // Put the first mbuf on the queue. Note this permits zero length records.
    sballoc(sb, m0);
    sblinkrecord(sb, m0);
    let m = m0.m_next().get();
    m0.m_next().set(None);
    if let Some(m) = m
        && m0.m_flags().get() & M_EOR != 0
    {
        m0.m_flags().set(m0.m_flags().get() & !M_EOR);
        m.m_flags().set(m.m_flags().get() | M_EOR);
    }
    sbcompress(sb, m, Some(m0));
}

/// Append address and data, and optionally, control (ancillary) data to the receive queue of
/// a socket. If present, `m0` must include a packet header with total length. Returns
/// `false` if no space in sockbuf or insufficient mbufs. `asa` is the address's bytes, at
/// least `sa_len` (its first byte) of them.
pub fn sbappendaddr(
    sb: &Sockbuf,
    asa: &[u8],
    m0: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> bool {
    let sa_len = usize::from(asa.first().copied().unwrap_or(0));
    let mut space = sa_len as i64;

    sbmtxassertlocked(sb);

    if let Some(m0) = m0 {
        if m0.m_flags().get() & M_PKTHDR == 0 {
            panic(format_args!("sbappendaddr"));
        }
        space += i64::from(m0.m_pkthdr().len.get());
    }
    // keep pointer to last control buf
    let mut n = control;
    while let Some(nn) = n {
        space += i64::from(nn.m_len().get());
        if nn.m_next().get().is_none() {
            break;
        }
        n = nn.m_next().get();
    }
    if space > sbspace_locked(sb) {
        return false;
    }
    if sa_len > MLEN {
        return false;
    }
    let Some(addr) = asa.get(..sa_len) else {
        return false;
    };
    let Some(m) = m_get(M_DONTWAIT, MT_SONAME) else {
        return false;
    };
    m.m_len().set(sa_len as u32);
    // SAFETY: a fresh mbuf's data area holds `MLEN` bytes, at least `sa_len` (checked).
    unsafe { ptr::copy_nonoverlapping(addr.as_ptr(), mtod::<u8>(m), sa_len) };
    let control = match n {
        Some(n) => {
            n.m_next().set(m0); // concatenate data to control
            control
        }
        None => m0,
    };
    m.m_next().set(control);

    let mut n = m;
    while let Some(next) = n.m_next().get() {
        sballoc(sb, n);
        n = next;
    }
    sballoc(sb, n);
    let nlast = n;
    sblinkrecord(sb, m);

    sb.sb_mbtail.set(Some(nlast));

    true
}

/// Append control and data to the receive queue of a socket as a new record; `false` if
/// there is not enough space.
pub fn sbappendcontrol(sb: &Sockbuf, m0: Option<&'static Mbuf>, control: &'static Mbuf) -> bool {
    let mut space: i64 = 0;
    let mut eor: u16 = 0;

    sbmtxassertlocked(sb);

    let mut m = control;
    loop {
        space += i64::from(m.m_len().get());
        match m.m_next().get() {
            Some(next) => m = next,
            None => break,
        }
    }
    let n = m; // save pointer to last control buffer
    let mut mm = m0;
    while let Some(x) = mm {
        space += i64::from(x.m_len().get());
        eor |= x.m_flags().get() & M_EOR;
        if eor != 0 {
            if x.m_next().get().is_none() {
                x.m_flags().set(x.m_flags().get() | M_EOR);
            } else {
                x.m_flags().set(x.m_flags().get() & !M_EOR);
            }
        }
        mm = x.m_next().get();
    }
    if space > sbspace_locked(sb) {
        return false;
    }
    n.m_next().set(m0); // concatenate data to control

    let mut m = control;
    while let Some(next) = m.m_next().get() {
        sballoc(sb, m);
        m = next;
    }
    sballoc(sb, m);
    let mlast = m;
    sblinkrecord(sb, control);

    sb.sb_mbtail.set(Some(mlast));

    true
}

/// Compress mbuf chain `m` into the socket buffer `sb` following mbuf `n`. If `n` is null,
/// the buffer is presumed empty.
pub fn sbcompress(sb: &Sockbuf, m: Option<&'static Mbuf>, n: Option<&'static Mbuf>) {
    let mut eor: u16 = 0;
    let mut m = m;
    let mut n = n;

    while let Some(mm) = m {
        eor |= mm.m_flags().get() & M_EOR;
        if mm.m_len().get() == 0
            && (eor == 0
                || mm
                    .m_next()
                    .get()
                    .or(n)
                    .is_some_and(|o| o.m_type().get() == mm.m_type().get()))
        {
            if same(sb.sb_lastrecord.get(), Some(mm)) {
                sb.sb_lastrecord.set(mm.m_next().get());
            }
            m = m_free(mm);
            continue;
        }
        if let Some(nn) = n
            && nn.m_flags().get() & M_EOR == 0
            // m_trailingspace() checks buffer writeability
            && mm.m_len().get()
                <= (if nn.m_flags().get() & M_EXT != 0 {
                    nn.m_ext().ext_size.get()
                } else {
                    MCLBYTES as u32
                }) / 4 // XXX Don't copy too much
            && mm.m_len().get() as i32 <= m_trailingspace(nn)
            && nn.m_type().get() == mm.m_type().get()
        {
            let len = mm.m_len().get();
            // SAFETY: `nn` has at least `len` writable bytes after its data
            // (`m_trailingspace`), and `mm` holds `len` bytes of data; two distinct mbufs.
            unsafe {
                ptr::copy_nonoverlapping(
                    mtod::<u8>(mm),
                    mtod::<u8>(nn).add(nn.m_len().get() as usize),
                    len as usize,
                );
            }
            nn.m_len().set(nn.m_len().get() + len);
            sb.sb_cc.set(sb.sb_cc.get() + u64::from(len));
            if is_data(mm) {
                sb.sb_datacc.set(sb.sb_datacc.get() + u64::from(len));
            }
            m = m_free(mm);
            continue;
        }
        match n {
            Some(nn) => nn.m_next().set(Some(mm)),
            None => sb.sb_mb.set(Some(mm)),
        }
        sb.sb_mbtail.set(Some(mm));
        sballoc(sb, mm);
        n = Some(mm);
        mm.m_flags().set(mm.m_flags().get() & !M_EOR);
        m = mm.m_next().get();
        mm.m_next().set(None);
    }
    if eor != 0 {
        match n {
            Some(nn) => nn.m_flags().set(nn.m_flags().get() | eor),
            None => {
                printf(format_args!("semi-panic: sbcompress"));
            }
        }
    }
}

/// Free all mbufs in a sockbuf. Check that all resources are reclaimed.
pub fn sbflush(sb: &Sockbuf) {
    rw_assert_unlocked(&sb.sb_lock);

    while sb.sb_mbcnt.get() != 0 {
        sbdrop(sb, sb.sb_cc.get() as i32);
    }

    kassert!(sb.sb_cc.get() == 0);
    kassert!(sb.sb_datacc.get() == 0);
    kassert!(sb.sb_mb.get().is_none());
    kassert!(sb.sb_mbtail.get().is_none());
    kassert!(sb.sb_lastrecord.get().is_none());
}

/// Drop data from (the front of) a sockbuf.
pub fn sbdrop(sb: &Sockbuf, len: i32) {
    let mut len = len;

    sbmtxassertlocked(sb);

    let mut m = sb.sb_mb.get();
    let mut next = m.and_then(|m| m.m_nextpkt().get());
    while len > 0 {
        let Some(mm) = m else {
            let Some(nx) = next else {
                panic(format_args!("sbdrop"));
            };
            m = Some(nx);
            next = nx.m_nextpkt().get();
            continue;
        };
        if mm.m_len().get() as i32 > len {
            mm.m_len().set(mm.m_len().get() - len as u32);
            mm.m_data()
                .set(mm.m_data().get().wrapping_add(len as usize));
            sb.sb_cc.set(sb.sb_cc.get() - len as u64);
            if is_data(mm) {
                sb.sb_datacc.set(sb.sb_datacc.get() - len as u64);
            }
            break;
        }
        len -= mm.m_len().get() as i32;
        sbfree(sb, mm);
        m = m_free(mm);
    }
    while let Some(mm) = m
        && mm.m_len().get() == 0
    {
        sbfree(sb, mm);
        m = m_free(mm);
    }
    match m {
        Some(mm) => {
            sb.sb_mb.set(Some(mm));
            mm.m_nextpkt().set(next);
        }
        None => sb.sb_mb.set(next),
    }
    // First part is an inline SB_EMPTY_FIXUP(). Second part makes sure sb_lastrecord is
    // up-to-date if we dropped part of the last record.
    match sb.sb_mb.get() {
        None => {
            sb.sb_mbtail.set(None);
            sb.sb_lastrecord.set(None);
        }
        Some(m) if m.m_nextpkt().get().is_none() => sb.sb_lastrecord.set(Some(m)),
        Some(_) => {}
    }
}

/// Drop a record off the front of a sockbuf and move the next record to the front.
pub fn sbdroprecord(sb: &Sockbuf) {
    if let Some(m) = sb.sb_mb.get() {
        sb.sb_mb.set(m.m_nextpkt().get());
        let mut m = Some(m);
        while let Some(mm) = m {
            sbfree(sb, mm);
            m = m_free(mm);
        }
    }
    sb_empty_fixup(sb);
}

/// Create a "control" mbuf containing the specified data with the specified type for
/// presentation on a socket buffer.
pub fn sbcreatecontrol(p: &[u8], type_: i32, level: i32) -> Option<&'static Mbuf> {
    let size = p.len();

    if cmsg_space(size) > MCLBYTES {
        printf(format_args!(
            "sbcreatecontrol: message too large {}\n",
            size
        ));
        return None;
    }

    let m = m_get(M_DONTWAIT, MT_CONTROL)?;
    if cmsg_space(size) > MLEN {
        mclget(m, M_DONTWAIT);
        if m.m_flags().get() & M_EXT == 0 {
            m_free(m);
            return None;
        }
    }
    let cp = mtod::<u8>(m);
    // SAFETY: the mbuf (or its cluster) holds at least `CMSG_SPACE(size)` bytes (checked
    // above); the header is written unaligned, the data after `CMSG_ALIGN(sizeof(cmsghdr))`.
    unsafe {
        ptr::write_bytes(cp, 0, cmsg_space(size));
        ptr::copy_nonoverlapping(p.as_ptr(), cp.add(cmsg_align(size_of::<Cmsghdr>())), size);
        cp.cast::<Cmsghdr>().write_unaligned(Cmsghdr {
            cmsg_len: cmsg_len(size) as u32,
            cmsg_level: level,
            cmsg_type: type_,
        });
    }
    m.m_len().set(cmsg_space(size) as u32);
    Some(m)
}

#[cfg(test)]
mod tests;
