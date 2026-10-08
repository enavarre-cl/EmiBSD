/*	$OpenBSD: socketvar.h,v 1.161 2026/06/11 12:50:52 bluhm Exp $	*/
/*	$NetBSD: socketvar.h,v 1.18 1996/02/09 18:25:38 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1990, 1993
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
 *	@(#)socketvar.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

/* <CODE> */
//! Kernel structure per socket and per socket buffer: `<sys/socketvar.h>`.
//!
//! Upstream: sys/sys/socketvar.h @ 3ce1f3f79392
//!
//! A [`Socket`] is a `socket_pool` item (`kern/uipc_socket.rs`) handed around as
//! `&'static Socket`, the `struct file *` idiom: valid from `soalloc` until the last
//! reference goes in `sorele`. Its two [`Sockbuf`]s hold the send and receive queues as
//! chains of records of mbufs. The header's inline functions (`soref`, `sbspace`,
//! `soreadable`, `sballoc`, ...) are here; the prototypes are their functions in
//! `kern/uipc_socket.rs`, `kern/uipc_socket2.rs`, `kern/sys_socket.rs` and
//! `kern/uipc_syscalls.rs`.
//!
//! Locks used to protect global data and struct members: \[I\] immutable after creation,
//! \[a\] atomic, \[mr\] `sb_mtx` of `so_rcv`, \[ms\] `sb_mtx` of `so_snd`, \[m\] `sb_mtx`,
//! \[br\] `sblock()` of `so_rcv`, \[bs\] `sblock()` of `so_snd`, \[s\] `solock()`.
//!
//! ## Deviations
//! - The members the C changes through a `struct socket *` under one of the locks above are
//!   `Cell`s; `so_error` (\[a\], `READ_ONCE`/`WRITE_ONCE`) is an `AtomicU32` holding the errno
//!   number (0 for none). Pointers to other sockets (`so_head`, `ssp_socket`, `ssp_soback`)
//!   are `Option<&'static Socket>`; `so_pcb` stays the C's untyped pointer, which each
//!   domain casts back (`sotounpcb`).
//! - `so_type` and `so_options` are `i32`, the `int` the `<sys/socket.h>` constants are
//!   (`short` in C); `sb_state` is a `u32` like `so_state`, whose `SS_*` bits it shares.
//! - `so_proto` is set by `soalloc`, whose two callers (`socreate`, `sonewconn`) assign the
//!   same protocol right after the allocation in C.
//! - `so_onq` is the address of the head's `so_q0` or `so_q` (`*const SoqHead`), compared
//!   with those addresses as the C compares the pointers.
//! - The `sb_startzero`/`sb_endzero` `memset` is [`Sockbuf::zero_counts`].
//! - `SOCKBUF_DEBUG` (`SBLASTRECORDCHK`, `SBLASTMBUFCHK`, `SBCHECK`) is not configured, as
//!   in GENERIC: the checks are `uipc_socket2.rs`'s functions, called by nobody.
//! - `socket_pool` and `sb_max` are defined in `kern/uipc_socket.rs` and
//!   `kern/uipc_socket2.rs`, where the C defines them.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_rwlock::rw_assert_wrlock;
use crate::kern::kern_synch::refcnt_take;
use crate::kern::uipc_socket2::{sbmtxassertlocked, soassertlocked_readonly};
use crate::machine::intr::IPL_MPFLOOR;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::event::{Klist, klist_empty};
use crate::sys::mbuf::{M_EXT, MSIZE, MT_CONTROL, MT_SONAME, Mbuf};
use crate::sys::mutex::Mutex;
use crate::sys::protosw::{PR_ATOMIC, PR_CONNREQUIRED, Protosw};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::rwlock::Rwlock;
use crate::sys::sigio::SigioRef;
use crate::sys::task::{Task, Taskq};
use crate::sys::time::Timeval;
use crate::sys::timeout::Timeout;
use crate::sys::types::{Gid, Off, Pid, Uid};

/// `SB_MAX`: default for max chars in sockbuf.
pub const SB_MAX: u64 = 2 * 1024 * 1024;
/// `SB_WAIT`: someone is waiting for data/space.
pub const SB_WAIT: i16 = 0x0001;
/// `SB_ASYNC`: ASYNC I/O, need signals.
pub const SB_ASYNC: i16 = 0x0002;
/// `SB_SPLICE`: buffer is splice source or drain.
pub const SB_SPLICE: i16 = 0x0004;
/// `SB_NOINTR`: operations not interruptible.
pub const SB_NOINTR: i16 = 0x0008;

// Socket state bits.
//
// NOTE: The following states should be used with corresponding socket's buffer `sb_state'
// only: SS_CANTSENDMORE with `so_snd', SS_ISSENDING with `so_snd', SS_CANTRCVMORE with
// `so_rcv', SS_RCVATMARK with `so_rcv'.

/// `SS_NOFDREF`: no file table ref any more.
pub const SS_NOFDREF: u32 = 0x001;
/// `SS_ISCONNECTED`: socket connected to a peer.
pub const SS_ISCONNECTED: u32 = 0x002;
/// `SS_ISCONNECTING`: in process of connecting to peer.
pub const SS_ISCONNECTING: u32 = 0x004;
/// `SS_ISDISCONNECTING`: in process of disconnecting.
pub const SS_ISDISCONNECTING: u32 = 0x008;
/// `SS_CANTSENDMORE`: can't send more data to peer.
pub const SS_CANTSENDMORE: u32 = 0x010;
/// `SS_CANTRCVMORE`: can't receive more data from peer.
pub const SS_CANTRCVMORE: u32 = 0x020;
/// `SS_RCVATMARK`: at mark on input.
pub const SS_RCVATMARK: u32 = 0x040;
/// `SS_ISDISCONNECTED`: socket disconnected from peer.
pub const SS_ISDISCONNECTED: u32 = 0x800;

/// `SS_PRIV`: privileged for broadcast, raw...
pub const SS_PRIV: u32 = 0x080;
/// `SS_CONNECTOUT`: connect, not accept, at this end.
pub const SS_CONNECTOUT: u32 = 0x1000;
/// `SS_ISSENDING`: hint for lower layer.
pub const SS_ISSENDING: u32 = 0x2000;
/// `SS_DNS`: created using `SOCK_DNS` socket(2).
pub const SS_DNS: u32 = 0x4000;
/// `SS_YP`: created using ypconnect(2).
pub const SS_YP: u32 = 0x8000;

// Flags to sblock()

/// `SBL_WAIT`: wait if lock not immediately available.
pub const SBL_WAIT: i32 = 0x01;
/// `SBL_NOINTR`: enforce non-interruptible sleep.
pub const SBL_NOINTR: i32 = 0x02;

/// `struct sosplice`: variables for socket splicing, allocated only when needed
/// (`sosplice_pool`).
pub struct Sosplice {
    /// \[mr ms\] `ssp_socket`: send data to drain socket.
    pub ssp_socket: Cell<Option<&'static Socket>>,
    /// \[mr ms\] `ssp_soback`: back ref to source socket.
    pub ssp_soback: Cell<Option<&'static Socket>>,
    /// \[mr\] `ssp_len`: number of bytes spliced.
    pub ssp_len: Cell<Off>,
    /// \[I\] `ssp_max`: maximum number of bytes.
    pub ssp_max: Cell<Off>,
    /// \[I\] `ssp_idletv`: idle timeout.
    pub ssp_idletv: Cell<Timeval>,
    /// `ssp_idleto`.
    pub ssp_idleto: Timeout,
    /// `ssp_task`: task for somove.
    pub ssp_task: Task,
    /// \[I\] `ssp_queue`: softnet queue where we add.
    pub ssp_queue: Cell<Option<&'static Taskq>>,
}

impl Sosplice {
    /// A zeroed `struct sosplice`, as `pool_get(PR_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            ssp_socket: Cell::new(None),
            ssp_soback: Cell::new(None),
            ssp_len: Cell::new(0),
            ssp_max: Cell::new(0),
            ssp_idletv: Cell::new(Timeval::new(0, 0)),
            ssp_idleto: Timeout::zeroed(),
            ssp_task: Task::zeroed(),
            ssp_queue: Cell::new(None),
        }
    }
}

impl Default for Sosplice {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct sockbuf`: variables for socket buffering.
pub struct Sockbuf {
    /// `sb_lock`: serialises the readers or the writers of the buffer (`sblock`).
    pub sb_lock: Rwlock,
    /// `sb_mtx`.
    pub sb_mtx: Mutex,
    // The following fields are all zeroed on flush (sb_startzero).
    /// \[m\] `sb_cc`: actual chars in buffer.
    pub sb_cc: Cell<u64>,
    /// \[m\] `sb_datacc`: data only chars in buffer.
    pub sb_datacc: Cell<u64>,
    /// \[m\] `sb_hiwat`: max actual char count.
    pub sb_hiwat: Cell<u64>,
    /// \[m\] `sb_wat`: default watermark.
    pub sb_wat: Cell<u64>,
    /// \[m\] `sb_mbcnt`: chars of mbufs used.
    pub sb_mbcnt: Cell<u64>,
    /// \[m\] `sb_mbmax`: max chars of mbufs to use.
    pub sb_mbmax: Cell<u64>,
    /// \[m\] `sb_lowat`: low water mark.
    pub sb_lowat: Cell<i64>,
    /// \[m\] `sb_mb`: the mbuf chain.
    pub sb_mb: Cell<Option<&'static Mbuf>>,
    /// \[m\] `sb_mbtail`: the last mbuf in the chain.
    pub sb_mbtail: Cell<Option<&'static Mbuf>>,
    /// \[m\] `sb_lastrecord`: first mbuf of last record in socket buffer.
    pub sb_lastrecord: Cell<Option<&'static Mbuf>>,
    /// \[m\] `sb_flags`: flags, see `SB_*` (end of the area zeroed on flush).
    pub sb_flags: Cell<i16>,
    /// \[m\] `sb_state`: socket state on sockbuf (`SS_*`).
    pub sb_state: Cell<u32>,
    /// \[m\] `sb_timeo_nsecs`: timeout for read/write.
    pub sb_timeo_nsecs: Cell<u64>,
    /// \[m\] `sb_klist`: list of knotes.
    pub sb_klist: Klist,
}

impl Sockbuf {
    /// A zeroed `struct sockbuf`; `soalloc` names its locks.
    pub const fn new() -> Self {
        Self {
            sb_lock: Rwlock::new("sbuf"),
            sb_mtx: Mutex::new(IPL_MPFLOOR),
            sb_cc: Cell::new(0),
            sb_datacc: Cell::new(0),
            sb_hiwat: Cell::new(0),
            sb_wat: Cell::new(0),
            sb_mbcnt: Cell::new(0),
            sb_mbmax: Cell::new(0),
            sb_lowat: Cell::new(0),
            sb_mb: Cell::new(None),
            sb_mbtail: Cell::new(None),
            sb_lastrecord: Cell::new(None),
            sb_flags: Cell::new(0),
            sb_state: Cell::new(0),
            sb_timeo_nsecs: Cell::new(0),
            sb_klist: Klist::new(),
        }
    }

    /// `memset(&sb->sb_startzero, 0, &sb->sb_endzero - &sb->sb_startzero)`: clears the
    /// counters, the limits and the chain, from `sb_cc` up to (not including) `sb_flags`.
    pub fn zero_counts(&self) {
        self.sb_cc.set(0);
        self.sb_datacc.set(0);
        self.sb_hiwat.set(0);
        self.sb_wat.set(0);
        self.sb_mbcnt.set(0);
        self.sb_mbmax.set(0);
        self.sb_lowat.set(0);
        self.sb_mb.set(None);
        self.sb_mbtail.set(None);
        self.sb_lastrecord.set(None);
    }

    /// `sb->sb_state & bits`.
    pub fn has_state(&self, bits: u32) -> bool {
        self.sb_state.get() & bits != 0
    }

    /// `sb->sb_state |= bits`.
    pub fn set_state(&self, bits: u32) {
        self.sb_state.set(self.sb_state.get() | bits);
    }

    /// `sb->sb_state &= ~bits`.
    pub fn clear_state(&self, bits: u32) {
        self.sb_state.set(self.sb_state.get() & !bits);
    }

    /// `sb->sb_flags & bits`.
    pub fn has_flags(&self, bits: i16) -> bool {
        self.sb_flags.get() & bits != 0
    }

    /// `sb->sb_flags |= bits`.
    pub fn set_flags(&self, bits: i16) {
        self.sb_flags.set(self.sb_flags.get() | bits);
    }

    /// `sb->sb_flags &= ~bits`.
    pub fn clear_flags(&self, bits: i16) {
        self.sb_flags.set(self.sb_flags.get() & !bits);
    }

    /// The wait channel of `sb->sb_cc` (`sbwait` and `sowakeup`).
    pub fn cc_chan(&self) -> *const c_void {
        ptr::from_ref(&self.sb_cc).cast()
    }
}

impl Default for Sockbuf {
    fn default() -> Self {
        Self::new()
    }
}

/// `TAILQ_HEAD(soqhead, socket)`: a listening socket's queue of partial (`so_q0`) or
/// complete (`so_q`) connections, through `so_qe`.
pub type SoqHead = TailqHead<SoQe>;

queue_adapter!(
    /// The adapter of `struct soqhead`: sockets through their `so_qe`.
    pub SoQe: Socket, so_qe => TailqEntry<Socket>
);

/// `so_upcall`: a protocol's or the kernel's hook run by `sorwakeup`.
pub type SoUpcall = fn(&'static Socket, *mut c_void, i32);

/// `struct socket`: kernel structure per socket. Contains send and receive buffer queues,
/// handle on protocol and pointer to protocol private data and error information.
pub struct Socket {
    /// \[I\] `so_proto`: protocol handle.
    pub so_proto: &'static Protosw,
    /// `so_lock`: this socket lock.
    pub so_lock: Rwlock,
    /// `so_refcnt`: references to this socket.
    pub so_refcnt: Refcnt,
    /// \[s\] `so_pcb`: protocol control block.
    pub so_pcb: Cell<*mut c_void>,
    /// \[s\] `so_state`: internal state flags `SS_*`.
    pub so_state: Cell<u32>,
    /// \[I\] `so_type`: generic type, see `socket.h`.
    pub so_type: Cell<i32>,
    /// \[s\] `so_options`: from socket call, see `socket.h`.
    pub so_options: Cell<i32>,
    /// \[s\] `so_linger`: time to linger while closing.
    pub so_linger: Cell<i16>,
    // Variables for connection queueing. Socket where accepts occur is so_head in all
    // subsidiary sockets. If so_head is 0, socket is not related to an accept. For head
    // socket so_q0 queues partially completed connections, while so_q is a queue of
    // connections ready to be accepted. If a connection is aborted and it has so_head set,
    // then it has to be pulled out of either so_q0 or so_q. We allow connections to queue
    // up based on current queue lengths and limit on number of queued connections for this
    // socket.
    //
    // Connections queue relies on both socket locks of listening and unaccepted sockets.
    // Socket lock of listening socket should be always taken first.
    /// \[s\] `so_head`: back pointer to accept socket.
    pub so_head: Cell<Option<&'static Socket>>,
    /// \[s\] `so_onq`: queue (q or q0) that we're on.
    pub so_onq: Cell<*const SoqHead>,
    /// \[s\] `so_q0`: queue of partial connections.
    pub so_q0: SoqHead,
    /// \[s\] `so_q`: queue of incoming connections.
    pub so_q: SoqHead,
    /// `so_sigio`: async I/O registration.
    pub so_sigio: SigioRef,
    /// \[s\] `so_qe`: our queue entry (q or q0).
    pub so_qe: TailqEntry<Socket>,
    /// \[s\] `so_q0len`: partials on `so_q0`.
    pub so_q0len: Cell<i16>,
    /// \[s\] `so_qlen`: number of connections on `so_q`.
    pub so_qlen: Cell<i16>,
    /// \[s\] `so_qlimit`: max number queued connections.
    pub so_qlimit: Cell<i16>,
    /// \[s\] `so_timeo`: connection timeout (and the channel connection waits sleep on).
    pub so_timeo: Cell<i16>,
    /// \[mr\] `so_oobmark`: chars to oob mark.
    pub so_oobmark: Cell<u64>,
    /// \[a\] `so_error`: error affecting connection (an errno number, 0 for none).
    pub so_error: AtomicU32,
    /// \[s br\] `so_sp`: splice state, allocated only when needed.
    pub so_sp: Cell<Option<&'static Sosplice>>,
    /// `so_rcv`: the receive buffer.
    pub so_rcv: Sockbuf,
    /// `so_snd`: the send buffer.
    pub so_snd: Sockbuf,
    /// \[s\] `so_upcall`.
    pub so_upcall: Cell<Option<SoUpcall>>,
    /// \[s\] `so_upcallarg`: arg for above.
    pub so_upcallarg: Cell<*mut c_void>,
    /// \[I\] `so_euid`: who opened the socket.
    pub so_euid: Cell<Uid>,
    /// \[I\] `so_ruid`.
    pub so_ruid: Cell<Uid>,
    /// \[I\] `so_egid`.
    pub so_egid: Cell<Gid>,
    /// \[I\] `so_rgid`.
    pub so_rgid: Cell<Gid>,
    /// \[I\] `so_cpid`: pid of process that opened socket.
    pub so_cpid: Cell<Pid>,
}

impl Socket {
    /// A zeroed socket of protocol `prp`, as `pool_get(PR_ZERO)` and the protocol assignment
    /// leave it; `soalloc` initialises its locks and lists.
    pub const fn new(prp: &'static Protosw) -> Self {
        Self {
            so_proto: prp,
            so_lock: Rwlock::new("socket"),
            so_refcnt: Refcnt::new(),
            so_pcb: Cell::new(ptr::null_mut()),
            so_state: Cell::new(0),
            so_type: Cell::new(0),
            so_options: Cell::new(0),
            so_linger: Cell::new(0),
            so_head: Cell::new(None),
            so_onq: Cell::new(ptr::null()),
            so_q0: TailqHead::new(),
            so_q: TailqHead::new(),
            so_sigio: SigioRef::new(),
            so_qe: TailqEntry::new(),
            so_q0len: Cell::new(0),
            so_qlen: Cell::new(0),
            so_qlimit: Cell::new(0),
            so_timeo: Cell::new(0),
            so_oobmark: Cell::new(0),
            so_error: AtomicU32::new(0),
            so_sp: Cell::new(None),
            so_rcv: Sockbuf::new(),
            so_snd: Sockbuf::new(),
            so_upcall: Cell::new(None),
            so_upcallarg: Cell::new(ptr::null_mut()),
            so_euid: Cell::new(0),
            so_ruid: Cell::new(0),
            so_egid: Cell::new(0),
            so_rgid: Cell::new(0),
            so_cpid: Cell::new(0),
        }
    }

    /// `so->so_state & bits`.
    pub fn has_state(&self, bits: u32) -> bool {
        self.so_state.get() & bits != 0
    }

    /// `so->so_state |= bits`.
    pub fn set_state(&self, bits: u32) {
        self.so_state.set(self.so_state.get() | bits);
    }

    /// `so->so_state &= ~bits`.
    pub fn clear_state(&self, bits: u32) {
        self.so_state.set(self.so_state.get() & !bits);
    }

    /// `so->so_options & bits`.
    pub fn has_options(&self, bits: i32) -> bool {
        self.so_options.get() & bits != 0
    }

    /// `so->so_proto->pr_flags & flags`.
    pub fn pr_flags(&self, flags: i16) -> bool {
        self.so_proto.pr_flags & flags != 0
    }

    /// `so->so_proto->pr_domain->dom_family`.
    pub fn dom_family(&self) -> i32 {
        self.so_proto.pr_domain.dom_family
    }

    /// `READ_ONCE(so->so_error)`.
    pub fn error(&self) -> Option<Errno> {
        Errno::from_raw(self.so_error.load(Ordering::Relaxed) as i32)
    }

    /// `WRITE_ONCE(so->so_error, error)`; `None` clears it.
    pub fn set_error(&self, error: Option<Errno>) {
        self.so_error
            .store(error.map_or(0, |e| e.as_i32() as u32), Ordering::Relaxed);
    }

    /// `so->so_onq == q`.
    pub fn onq_is(&self, q: &SoqHead) -> bool {
        ptr::eq(self.so_onq.get(), q)
    }

    /// The wait channel of `&so->so_timeo` (connection, accept and close waits).
    pub fn timeo_chan(&self) -> *const c_void {
        ptr::from_ref(&self.so_timeo).cast()
    }
}

/// `soref(so)`: takes a reference to `so`; returns it, or `None` for `None`.
pub fn soref(so: Option<&'static Socket>) -> Option<&'static Socket> {
    let so = so?;
    refcnt_take(&so.so_refcnt);
    Some(so)
}

// Macros for sockets and socket buffering.

/// `isspliced(so)`: `so` sends its received data to a drain socket.
pub fn isspliced(so: &Socket) -> bool {
    so.so_sp
        .get()
        .is_some_and(|sp| sp.ssp_socket.get().is_some())
}

/// `issplicedback(so)`: `so` is the drain of a spliced source socket.
pub fn issplicedback(so: &Socket) -> bool {
    so.so_sp
        .get()
        .is_some_and(|sp| sp.ssp_soback.get().is_some())
}

/// `sb_notify(sb)`: do we need to notify the other side when I/O is possible?
pub fn sb_notify(sb: &Sockbuf) -> bool {
    mtx_enter(&sb.sb_mtx);
    let rv = sb.has_flags(SB_WAIT | SB_ASYNC | SB_SPLICE) || !klist_empty(&sb.sb_klist);
    mtx_leave(&sb.sb_mtx);

    rv
}

/// `sbspace_locked(sb)`: how much space is there in a socket buffer (`so->so_snd` or
/// `so->so_rcv`)? This is problematical if the fields are unsigned, as the space might still
/// be negative (`cc > hiwat` or `mbcnt > mbmax`): the differences wrap and read as negative
/// `long`s, as in C.
pub fn sbspace_locked(sb: &Sockbuf) -> i64 {
    sbmtxassertlocked(sb);

    let cc = sb.sb_hiwat.get().wrapping_sub(sb.sb_cc.get()) as i64;
    let mb = sb.sb_mbmax.get().wrapping_sub(sb.sb_mbcnt.get()) as i64;
    cc.min(mb)
}

/// `sbspace(sb)`: `sbspace_locked` under the buffer's mutex.
pub fn sbspace(sb: &Sockbuf) -> i64 {
    mtx_enter(&sb.sb_mtx);
    let ret = sbspace_locked(sb);
    mtx_leave(&sb.sb_mtx);

    ret
}

/// `sosendallatonce(so)`: do we have to send all at once on a socket?
pub fn sosendallatonce(so: &Socket) -> bool {
    so.pr_flags(PR_ATOMIC)
}

/// `soissending(so)`: are we sending on this socket?
pub fn soissending(so: &Socket) -> bool {
    so.so_snd.has_state(SS_ISSENDING)
}

/// `soreadable(so)`: can we read something from `so`?
pub fn soreadable(so: &Socket) -> bool {
    soassertlocked_readonly(so);
    if isspliced(so) {
        return false;
    }
    so.so_rcv.has_state(SS_CANTRCVMORE)
        || so.error().is_some()
        || so.so_rcv.sb_cc.get() as i64 >= so.so_rcv.sb_lowat.get()
}

/// `sowriteable(so)`: can we write something to `so`?
pub fn sowriteable(so: &Socket) -> bool {
    soassertlocked_readonly(so);
    (sbspace(&so.so_snd) >= so.so_snd.sb_lowat.get()
        && (so.has_state(SS_ISCONNECTED) || !so.pr_flags(PR_CONNREQUIRED)))
        || so.so_snd.has_state(SS_CANTSENDMORE)
        || so.error().is_some()
}

/// Whether `m` counts in `sb_datacc`: it is neither an address nor control.
fn is_data(m: &Mbuf) -> bool {
    let t = i32::from(m.m_type().get());
    t != MT_CONTROL && t != MT_SONAME
}

/// The storage `m` takes: `MSIZE`, plus its cluster.
fn mbcnt(m: &Mbuf) -> u64 {
    let mut n = MSIZE as u64;
    if m.m_flags().get() & M_EXT != 0 {
        n += u64::from(m.m_ext().ext_size.get());
    }
    n
}

/// `sballoc(sb, m)`: adjust counters in `sb` reflecting allocation of `m`.
pub fn sballoc(sb: &Sockbuf, m: &Mbuf) {
    let len = u64::from(m.m_len().get());
    sb.sb_cc.set(sb.sb_cc.get().wrapping_add(len));
    if is_data(m) {
        sb.sb_datacc.set(sb.sb_datacc.get().wrapping_add(len));
    }
    sb.sb_mbcnt.set(sb.sb_mbcnt.get().wrapping_add(mbcnt(m)));
}

/// `sbfree(sb, m)`: adjust counters in `sb` reflecting freeing of `m`.
pub fn sbfree(sb: &Sockbuf, m: &Mbuf) {
    let len = u64::from(m.m_len().get());
    sb.sb_cc.set(sb.sb_cc.get().wrapping_sub(len));
    if is_data(m) {
        sb.sb_datacc.set(sb.sb_datacc.get().wrapping_sub(len));
    }
    sb.sb_mbcnt.set(sb.sb_mbcnt.get().wrapping_sub(mbcnt(m)));
}

/// `sbassertlocked(sb)`: the caller holds `sblock(sb)`.
pub fn sbassertlocked(sb: &Sockbuf) {
    rw_assert_wrlock(&sb.sb_lock);
}

/// `SB_EMPTY_FIXUP(sb)`: an empty chain has no tail and no last record.
pub fn sb_empty_fixup(sb: &Sockbuf) {
    if sb.sb_mb.get().is_none() {
        sb.sb_mbtail.set(None);
        sb.sb_lastrecord.set(None);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest::{assert_complete, assert_defines};

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/socketvar.h");
        let sb = assert_defines!(defs; SB_MAX, SB_WAIT, SB_ASYNC, SB_SPLICE, SB_NOINTR);
        assert_complete(&defs, "SB_", &sb);
        let ss = assert_defines!(defs;
            SS_NOFDREF, SS_ISCONNECTED, SS_ISCONNECTING, SS_ISDISCONNECTING,
            SS_CANTSENDMORE, SS_CANTRCVMORE, SS_RCVATMARK, SS_ISDISCONNECTED, SS_PRIV,
            SS_CONNECTOUT, SS_ISSENDING, SS_DNS, SS_YP);
        assert_complete(&defs, "SS_", &ss);
        let sbl = assert_defines!(defs; SBL_WAIT, SBL_NOINTR);
        assert_complete(&defs, "SBL_", &sbl);
    }
}
/* </TESTS> */
