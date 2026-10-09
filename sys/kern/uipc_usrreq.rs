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
 *	@(#)uipc_usrreq.c	8.3 (Berkeley) 1/4/94
 */
/* </LICENSES> */

/* <CODE> */
//! The UNIX communications domain: `kern/uipc_usrreq.c`.
//!
//! Upstream: sys/kern/uipc_usrreq.c @ 3ce1f3f79392
//!
//! Local sockets: stream, sequenced packet and datagram sockets that live in the kernel
//! only. Each socket has a protocol control block ([`Unpcb`]); a connected stream socket's
//! data goes straight into its peer's receive buffer, and the sender's send buffer mirrors
//! that buffer's counts for back pressure. A socket bound to a path names a `VSOCK` vnode
//! (`uipc_bind`), which `unp_connect` finds through `namei`. `SCM_RIGHTS` control messages
//! carry descriptors: `unp_internalize` turns the sender's descriptor numbers into file
//! references (`struct fdpass`), `unp_externalize` installs them in the receiver's table,
//! and `unp_dispose`/`unp_discard` hand the ones never received to the garbage collector
//! (`unp_gc`, a task on `systqmp`), which also finds and frees cycles of sockets that are
//! only referenced from each other's buffers.
//!
//! Locks used to protect global data and struct members: \[I\] immutable after creation,
//! \[D\] `unp_df_lock`, \[G\] `unp_gc_lock`, \[M\] `unp_ino_mtx`, \[R\] `unp_rights_mtx`,
//! \[a\] atomic, \[s\] socket lock.
//!
//! ## Deviations
//! - `unp_head` and `unp_deferred` are statics wrapping their heads (the `allproc` idiom);
//!   `unp_ino`, `unp_rights`, `unp_defer` and `unp_gcing` are atomics, still changed under
//!   the locks the C names. The send and receive space tunables are `AtomicI32`s, the type
//!   `sysctl_bounded_arr` takes (`u_int` in C; the bounds keep them positive).
//! - `struct fdpass` arrays live in mbuf data: `unp_scan` hands its operations a raw pointer
//!   and a count, so `unp_discard`, `unp_remove_gcrefs` and `unp_restore_gcrefs` are
//!   `unsafe fn`s; the entries are read and written unaligned.
//! - `unp_nam2sun` returns the path length; the `struct sockaddr_un *` is the mbuf's data,
//!   which the callers read through it as the C does.
//! - `pool_get(PR_WAITOK)` and `m_getclr(M_WAITOK)` can fail here (see `subr_pool.rs`):
//!   `uipc_attach` answers `ENOBUFS` (as for `PR_NOWAIT`), `uipc_bind` `ENOBUFS`.
//!   `mallocarray(M_WAITOK)` in `unp_externalize` answers `ENOMEM`; `fdexpand` failing
//!   there (it cannot in C) ends the call with its error. `unp_discard`'s `malloc(M_WAITOK)`
//!   cannot be refused without leaking the files in flight, so its failure panics.
//! - `unp_internalize` keeps the message in a stack buffer (at most `MLEN` bytes) while it
//!   moves it into a cluster, where the C uses `malloc(M_TEMP)`.
//! - `NKCOV` is 0 (no kcov descriptors).

use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::conf::param::MAXFILES;
use crate::kassert;
use crate::kern::kern_descrip::{closef, fd_getfile, fdalloc, fdexpand, fdremove};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_pledge::{pledge_recvfd, pledge_sendfd};
use crate::kern::kern_rwlock::{rw_assert_wrlock, rw_enter_write, rw_exit_write};
use crate::kern::kern_synch::{refcnt_finalize, refcnt_init, refcnt_rele_wake, refcnt_take};
use crate::kern::kern_sysctl::{sysctl_bounded_arr, sysctl_rdint};
use crate::kern::kern_task::{SYSTQMP, task_add};
use crate::kern::kern_tc::getnanotime;
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_copym, m_freem, m_getclr, m_purge, m_trailingspace};
use crate::kern::uipc_proto::UNIXDOMAIN;
use crate::kern::uipc_socket::sofree;
use crate::kern::uipc_socket::sorwakeup;
use crate::kern::uipc_socket::sowwakeup;
use crate::kern::uipc_socket2::{
    sbappend, sbappendaddr, sbappendcontrol, sbappendrecord, soassertlocked, socantrcvmore,
    socantsendmore, soisconnected, soisdisconnected, solock, solock_pair, sonewconn, soreserve,
    sounlock, sounlock_pair,
};
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::{vattr_null, vput, vrele};
use crate::kern::vfs_vnops::vn_isunder;
use crate::kern::vfs_vops::{VOP_ABORTOP, VOP_ACCESS, VOP_CREATE, VOP_LOCK, VOP_UNLOCK};
use crate::machine::cpu::curproc;
use crate::machine::intr::IPL_SOFTNET;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::file::{
    DTYPE_KQUEUE, DTYPE_SOCKET, DTYPE_VMM, DTYPE_VNODE, FDUP_MAX_COUNT, File, fref, frele,
};
use crate::sys::filedesc::{
    UF_EXCLOSE, UF_FORKCLOSE, UF_PLEDGED, UF_PLEDGEOPEN, fdplock, fdpunlock,
};
use crate::sys::lock::LK_EXCLUSIVE;
use crate::sys::malloc::{M_TEMP, M_WAITOK};
use crate::sys::mbuf::{
    M_COPYALL, M_DONTWAIT, M_EXT, M_WAIT, MLEN, MT_CONTROL, MT_SONAME, Mbuf, mclget, mtod,
};
use crate::sys::mutex::Mutex;
use crate::sys::namei::{
    CREATE, FOLLOW, LOCKLEAF, LOCKPARENT, LOOKUP, NOFOLLOW, NiDirp, UNVEIL_CREATE, UNVEIL_WRITE,
};
use crate::sys::param::NODEV;
use crate::sys::pledge::PLEDGE_UNIX;
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::protosw::{PR_CONNREQUIRED, PrUsrreqs};
use crate::sys::queue::{ListHead, SlistEntry, SlistHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::socket::{
    AF_UNIX, Cmsghdr, MSG_CMSG_CLOEXEC, MSG_CMSG_CLOFORK, NET_UNIX_DEFERRED, NET_UNIX_INFLIGHT,
    SCM_RIGHTS, SO_ACCEPTCONN, SOCK_DGRAM, SOCK_SEQPACKET, SOCK_STREAM, SOL_SOCKET, Sockaddr,
    Sockpeercred, UNPCTL_RECVSPACE, UNPCTL_SENDSPACE, cmsg_align, cmsg_len, cmsg_space,
};
use crate::sys::socketvar::{SB_MAX, SS_CANTSENDMORE, SS_ISCONNECTED, Socket};
use crate::sys::stat::{ACCESSPERMS, Stat};
use crate::sys::sysctl::SysctlBoundedArgs;
use crate::sys::systm::{INFSLP, kernel_lock, kernel_unlock};
use crate::sys::task::Task;
use crate::sys::types::{Blksize, Socklen};
use crate::sys::un::{SUN_PATH_LEN, SockaddrUn};
use crate::sys::unpcb::{
    Fdpass, UNP_BINDING, UNP_CONNECTING, UNP_FEIDS, UNP_FEIDSBIND, UNP_GCDEAD, UnpHead, Unpcb,
    sotounpcb,
};
use crate::sys::vnode::{VBLK, VDIR, VSOCK, VWRITE, Vattr};

/// `PIPSIZ`: both send and receive buffers are allocated `PIPSIZ` bytes of buffering for
/// stream sockets, although the total for sender and receiver is actually only `PIPSIZ`.
/// Datagram sockets really use the sendspace as the maximum datagram size, and don't really
/// want to reserve the sendspace. Their recvspace should be large enough for at least one
/// max-size datagram plus address.
const PIPSIZ: i32 = 32768;

/// The size of `sun_noname`.
const SUN_NONAME_LEN: usize = size_of::<Sockaddr>();

/// `struct unp_deferral`: a set of files that were passed over a socket but were not
/// received and need to be closed, followed by its `ud_n` `struct fdpass`es.
#[repr(C)]
pub struct UnpDeferral {
    /// \[D\] `ud_link`.
    pub ud_link: SlistEntry<UnpDeferral>,
    /// \[I\] `ud_n`.
    pub ud_n: i32,
    /// \[I\] `ud_fp`: the files (a flexible array member).
    pub ud_fp: [Fdpass; 0],
}

impl UnpDeferral {
    /// `&defer->ud_fp[0]`.
    fn fps(&self) -> *mut Fdpass {
        ptr::from_ref(self)
            .cast::<u8>()
            .wrapping_add(offset_of!(UnpDeferral, ud_fp))
            .cast::<Fdpass>()
            .cast_mut()
    }

    /// The allocation size of a deferral of `n` files.
    const fn size(n: usize) -> usize {
        size_of::<UnpDeferral>() + n * size_of::<Fdpass>()
    }
}

queue_adapter!(
    /// `SLIST_HEAD(,unp_deferral)`: the deferred sets, through `ud_link`.
    pub UnpDeferrals: UnpDeferral, ud_link => SlistEntry<UnpDeferral>
);

/// `unp_head`: the list of all UNIX domain sockets, for `unp_gc()`.
pub struct UnpHeadList(pub ListHead<UnpHead>);

// SAFETY: the list is read and changed only under `unp_gc_lock` ([G]).
unsafe impl Sync for UnpHeadList {}

/// `unp_deferred`: the sets of files that were sent over sockets that are now closed.
pub struct UnpDeferredList(pub SlistHead<UnpDeferrals>);

// SAFETY: the list is read and changed only under `unp_df_lock` ([D]).
unsafe impl Sync for UnpDeferredList {}

/// `unp_df_lock`.
pub static UNP_DF_LOCK: Rwlock = Rwlock::new("unpdflk");
/// `unp_gc_lock`.
pub static UNP_GC_LOCK: Rwlock = Rwlock::new("unpgclk");

/// `unp_rights_mtx`.
pub static UNP_RIGHTS_MTX: Mutex = Mutex::new(IPL_SOFTNET);
/// `unp_ino_mtx`.
pub static UNP_INO_MTX: Mutex = Mutex::new(IPL_SOFTNET);

/// `unpcb_pool`.
pub static UNPCB_POOL: Pool = Pool::new();
/// `unp_gc_task`.
pub static UNP_GC_TASK: Task = Task::new(unp_gc, ptr::null_mut());

/// `sun_noname`: the address of an unbound UNIX domain socket.
pub static SUN_NONAME: Sockaddr = Sockaddr {
    sa_len: SUN_NONAME_LEN as u8,
    sa_family: AF_UNIX,
    sa_data: [0; 14],
};

/// \[G\] `unp_head`.
pub static UNP_HEAD: UnpHeadList = UnpHeadList(ListHead::new());
/// \[D\] `unp_deferred`.
pub static UNP_DEFERRED: UnpDeferredList = UnpDeferredList(SlistHead::new());

/// \[M\] `unp_ino`: prototype for fake inode numbers.
pub static UNP_INO: AtomicU64 = AtomicU64::new(0);
/// \[R\] `unp_rights`: file descriptors in flight.
pub static UNP_RIGHTS: AtomicI32 = AtomicI32::new(0);
/// \[G\] `unp_defer`: number of deferred fp to close by the GC task.
pub static UNP_DEFER: AtomicI32 = AtomicI32::new(0);
/// \[G\] `unp_gcing`: GC task currently running.
pub static UNP_GCING: AtomicI32 = AtomicI32::new(0);

/// `uipc_usrreqs`: the stream and sequenced packet protocols.
pub static UIPC_USRREQS: PrUsrreqs = PrUsrreqs {
    pru_attach: Some(uipc_attach),
    pru_detach: Some(uipc_detach),
    pru_bind: Some(uipc_bind),
    pru_listen: Some(uipc_listen),
    pru_connect: Some(uipc_connect),
    pru_accept: Some(uipc_accept),
    pru_disconnect: Some(uipc_disconnect),
    pru_shutdown: Some(uipc_shutdown),
    pru_rcvd: Some(uipc_rcvd),
    pru_send: Some(uipc_send),
    pru_abort: Some(uipc_abort),
    pru_sense: Some(uipc_sense),
    pru_sockaddr: Some(uipc_sockaddr),
    pru_peeraddr: Some(uipc_peeraddr),
    pru_connect2: Some(uipc_connect2),
    ..PrUsrreqs::NONE
};

/// `uipc_dgram_usrreqs`: the datagram protocol.
pub static UIPC_DGRAM_USRREQS: PrUsrreqs = PrUsrreqs {
    pru_attach: Some(uipc_attach),
    pru_detach: Some(uipc_detach),
    pru_bind: Some(uipc_bind),
    pru_listen: Some(uipc_listen),
    pru_connect: Some(uipc_connect),
    pru_disconnect: Some(uipc_disconnect),
    pru_shutdown: Some(uipc_dgram_shutdown),
    pru_send: Some(uipc_dgram_send),
    pru_sense: Some(uipc_sense),
    pru_sockaddr: Some(uipc_sockaddr),
    pru_peeraddr: Some(uipc_peeraddr),
    pru_connect2: Some(uipc_connect2),
    ..PrUsrreqs::NONE
};

/// \[a\] `unpst_sendspace`.
pub static UNPST_SENDSPACE: AtomicI32 = AtomicI32::new(PIPSIZ);
/// \[a\] `unpst_recvspace`.
pub static UNPST_RECVSPACE: AtomicI32 = AtomicI32::new(PIPSIZ);
/// \[a\] `unpsq_sendspace`.
pub static UNPSQ_SENDSPACE: AtomicI32 = AtomicI32::new(PIPSIZ);
/// \[a\] `unpsq_recvspace`.
pub static UNPSQ_RECVSPACE: AtomicI32 = AtomicI32::new(PIPSIZ);
/// \[a\] `unpdg_sendspace`: really max datagram size.
pub static UNPDG_SENDSPACE: AtomicI32 = AtomicI32::new(8192);
/// \[a\] `unpdg_recvspace`.
pub static UNPDG_RECVSPACE: AtomicI32 = AtomicI32::new(PIPSIZ);

/// `unpstctl_vars`: `net.unix.stream`.
static UNPSTCTL_VARS: [SysctlBoundedArgs; 2] = [
    SysctlBoundedArgs::new(UNPCTL_RECVSPACE, &UNPST_RECVSPACE, 0, SB_MAX as i32),
    SysctlBoundedArgs::new(UNPCTL_SENDSPACE, &UNPST_SENDSPACE, 0, SB_MAX as i32),
];
/// `unpsqctl_vars`: `net.unix.seqpacket`.
static UNPSQCTL_VARS: [SysctlBoundedArgs; 2] = [
    SysctlBoundedArgs::new(UNPCTL_RECVSPACE, &UNPSQ_RECVSPACE, 0, SB_MAX as i32),
    SysctlBoundedArgs::new(UNPCTL_SENDSPACE, &UNPSQ_SENDSPACE, 0, SB_MAX as i32),
];
/// `unpdgctl_vars`: `net.unix.dgram`.
static UNPDGCTL_VARS: [SysctlBoundedArgs; 2] = [
    SysctlBoundedArgs::new(UNPCTL_RECVSPACE, &UNPDG_RECVSPACE, 0, SB_MAX as i32),
    SysctlBoundedArgs::new(UNPCTL_SENDSPACE, &UNPDG_SENDSPACE, 0, SB_MAX as i32),
];

/// The operation `unp_scan` applies to each set of files it finds.
type UnpScanOp = unsafe fn(*mut Fdpass, usize);

/// `sotounpcb(so)` of a socket the C knows to be attached.
fn unpcb_of(so: &Socket) -> &'static Unpcb {
    match sotounpcb(so) {
        Some(unp) => unp,
        None => panic(format_args!("socket {:p}: no unpcb", so)),
    }
}

/// `curproc`, which the requests run as.
fn curproc_or_panic(func: &str) -> &'static Proc {
    match curproc() {
        Some(p) => p,
        None => panic(format_args!("{}: no curproc", func)),
    }
}

/// Whether two optional control blocks are the same one.
fn same_unp(a: Option<&Unpcb>, b: &Unpcb) -> bool {
    a.is_some_and(|a| ptr::eq(a, b))
}

/// The `cmsghdr` at the start of `m`, read unaligned; the caller checked `m` holds one.
fn cmsghdr_of(m: &Mbuf) -> Cmsghdr {
    // SAFETY: the callers check `m_len` against the header's size, or know the message (an
    // internalized `SCM_RIGHTS` message is at least `CMSG_LEN(0)` long).
    unsafe { mtod::<Cmsghdr>(m).read_unaligned() }
}

/// Writes the `cmsghdr` at the start of `m`.
fn set_cmsghdr(m: &Mbuf, cm: Cmsghdr) {
    // SAFETY: the message `m` holds starts with a header (see `cmsghdr_of`).
    unsafe { mtod::<Cmsghdr>(m).write_unaligned(cm) };
}

/// `CMSG_DATA(cm)` of the message at the start of `m`.
fn cmsg_data_of(m: &Mbuf) -> *mut u8 {
    mtod::<u8>(m).wrapping_add(cmsg_align(size_of::<Cmsghdr>()))
}

/// The `cmsg_len` of a header as the `size_t`s the C computes with.
fn cmsg_len_of(cm: &Cmsghdr) -> usize {
    cm.cmsg_len as usize
}

/// `unp_init`: the control block pool.
pub fn unp_init() {
    pool_init(
        &UNPCB_POOL,
        size_of::<Unpcb>(),
        0,
        IPL_SOFTNET,
        0,
        "unpcb",
        None,
    );
}

/// `unp_ref(unp)`.
fn unp_ref(unp: &Unpcb) {
    refcnt_take(&unp.unp_refcnt);
}

/// `unp_rele(unp)`.
fn unp_rele(unp: &Unpcb) {
    refcnt_rele_wake(&unp.unp_refcnt);
}

/// `unp_solock_peer(so)`: locks the socket `so` is connected to (in address order, which
/// may unlock `so` for a moment) and returns it; `None` if not connected.
pub fn unp_solock_peer(so: &'static Socket) -> Option<&'static Socket> {
    let unp = unpcb_of(so);

    loop {
        let unp2 = unp.unp_conn.get()?;

        let so2 = unp2.unp_socket;

        if ptr::from_ref(so) < ptr::from_ref(so2) {
            solock(so2);
        } else if ptr::from_ref(so) > ptr::from_ref(so2) {
            unp_ref(unp2);
            sounlock(so);
            solock(so2);
            solock(so);

            // Datagram socket could be reconnected due to re-lock.
            if !same_unp(unp.unp_conn.get(), unp2) {
                sounlock(so2);
                unp_rele(unp2);
                continue;
            }

            unp_rele(unp2);
        }

        return Some(so2);
    }
}

/// `uipc_setaddr(unp, nam)`: the bound address of `unp` (or `sun_noname`) into `nam`.
pub fn uipc_setaddr(unp: Option<&Unpcb>, nam: &Mbuf) {
    if let Some(addr) = unp.and_then(|unp| unp.unp_addr.get()) {
        let len = addr.m_len().get();
        nam.m_len().set(len);
        // SAFETY: both mbufs hold at least `len` bytes (`nam` is a fresh `MT_SONAME` mbuf of
        // `MLEN` bytes; a bound address is a `struct sockaddr_un`, smaller); distinct mbufs.
        unsafe { ptr::copy_nonoverlapping(mtod::<u8>(addr), mtod::<u8>(nam), len as usize) };
    } else {
        nam.m_len().set(SUN_NONAME_LEN as u32);
        // SAFETY: `nam` holds `MLEN` bytes, more than a `struct sockaddr`; `SUN_NONAME` is a
        // `#[repr(C)]` sockaddr without padding.
        unsafe { mtod::<Sockaddr>(nam).write_unaligned(SUN_NONAME) };
    }
}

/// `uipc_attach(so, proto, wait)`: a new control block for `so`, with the domain's buffer
/// sizes.
pub fn uipc_attach(so: &'static Socket, _proto: i32, wait: i32) -> Result<(), Errno> {
    if !so.so_pcb.get().is_null() {
        return Err(Errno::EISCONN);
    }
    if so.so_snd.sb_hiwat.get() == 0 || so.so_rcv.sb_hiwat.get() == 0 {
        let space = |s: &AtomicI32| s.load(Ordering::Relaxed) as u64;
        match so.so_type.get() {
            SOCK_STREAM => soreserve(so, space(&UNPST_SENDSPACE), space(&UNPST_RECVSPACE))?,
            SOCK_SEQPACKET => {
                soreserve(so, space(&UNPSQ_SENDSPACE), space(&UNPSQ_RECVSPACE))?;
            }
            SOCK_DGRAM => soreserve(so, space(&UNPDG_SENDSPACE), space(&UNPDG_RECVSPACE))?,
            _ => panic(format_args!("unp_attach")),
        }
    }
    let Some(mem) = pool_get(
        &UNPCB_POOL,
        (if wait == M_WAIT { PR_WAITOK } else { PR_NOWAIT }) | PR_ZERO,
    ) else {
        return Err(Errno::ENOBUFS);
    };
    let raw = mem.cast::<Unpcb>().as_ptr();
    // SAFETY: a fresh, suitably aligned `unpcb_pool` item, written once before anything else
    // sees it.
    unsafe { raw.write(Unpcb::new(so)) };
    // SAFETY: as above; the item stays allocated until `unp_detach` gives it back.
    let unp: &'static Unpcb = unsafe { &*raw };
    refcnt_init(&unp.unp_refcnt);
    so.so_pcb.set(raw.cast());
    unp.unp_ctime.set(getnanotime());

    rw_enter_write(&UNP_GC_LOCK);
    // SAFETY: `unp_gc_lock` is held; the control block is new and stays in place until
    // `unp_detach` unlinks it.
    unsafe { UNP_HEAD.0.insert_head(unp) };
    rw_exit_write(&UNP_GC_LOCK);

    Ok(())
}

/// `uipc_detach(so)`.
pub fn uipc_detach(so: &'static Socket) -> Result<(), Errno> {
    let Some(unp) = sotounpcb(so) else {
        return Err(Errno::EINVAL);
    };

    unp_detach(unp);

    Ok(())
}

/// `uipc_bind(so, nam, p)`: binds `so` to the path in `nam`, creating a `VSOCK` vnode.
pub fn uipc_bind(so: &'static Socket, nam: &'static Mbuf, p: &Proc) -> Result<(), Errno> {
    let unp = unpcb_of(so);

    if unp.has_flags(UNP_BINDING | UNP_CONNECTING) {
        return Err(Errno::EINVAL);
    }
    if unp.unp_vnode.get().is_some() {
        return Err(Errno::EINVAL);
    }
    let pathlen = unp_nam2sun(nam)?;

    unp.set_flags(UNP_BINDING);

    // Enforce `i_lock' -> `solock' because fifo subsystem requires it. The socket can't be
    // closed concurrently because the file descriptor reference is still held.

    sounlock(unp.unp_socket);

    let Some(nam2) = m_getclr(M_WAITOK, MT_SONAME) else {
        solock(unp.unp_socket);
        unp.clear_flags(UNP_BINDING);
        return Err(Errno::ENOBUFS);
    };

    let error: Result<(), Errno> = 'out: {
        nam2.m_len().set(size_of::<SockaddrUn>() as u32);
        // SAFETY: `nam` holds a `sockaddr_un` with a `pathlen`-byte path (`unp_nam2sun`);
        // `nam2` is a fresh zeroed mbuf of `MLEN` bytes, larger than a `sockaddr_un`.
        unsafe {
            ptr::copy_nonoverlapping(
                mtod::<u8>(nam),
                mtod::<u8>(nam2),
                SockaddrUn::PATH_OFFSET + pathlen,
            );
        }
        // No need to NUL terminate: m_getclr() returns zero'd mbufs.

        // Fixup sun_len to keep it in sync with m_len.
        // SAFETY: as above; `sun_len` is the first byte.
        unsafe { *mtod::<u8>(nam2) = nam2.m_len().get() as u8 };

        // SAFETY: the path is the `SUN_PATH_LEN` bytes after the header in `nam2`, which this
        // function holds until it either frees it or hands it to the control block.
        let path: &[u8] = unsafe {
            slice::from_raw_parts(mtod::<u8>(nam2).add(SockaddrUn::PATH_OFFSET), SUN_PATH_LEN)
        };
        let mut nd = ndinit(CREATE, NOFOLLOW | LOCKPARENT, NiDirp::Sys(path), p);
        nd.ni_pledge = PLEDGE_UNIX;
        nd.ni_unveil = UNVEIL_CREATE;

        kernel_lock();
        // SHOULD BE ABLE TO ADOPT EXISTING AND wakeup() ALA FIFO's
        if let Err(error) = namei(&mut nd) {
            m_freem(nam2);
            solock(unp.unp_socket);
            break 'out Err(error);
        }
        let Some(dvp) = nd.ni_dvp else {
            panic(format_args!("uipc_bind: namei returned no parent"));
        };
        if let Some(vp) = nd.ni_vp {
            let _ = VOP_ABORTOP(dvp, &mut nd.ni_cnd);
            if ptr::eq(dvp, vp) {
                vrele(dvp);
            } else {
                vput(dvp);
            }
            vrele(vp);
            m_freem(nam2);
            solock(unp.unp_socket);
            break 'out Err(Errno::EADDRINUSE);
        }
        let mut vattr = Vattr::new();
        vattr_null(&mut vattr);
        vattr.va_type = VSOCK;
        vattr.va_mode = ACCESSPERMS & !p.fd().fd_cmask.get();
        let error = VOP_CREATE(dvp, &mut nd.ni_vp, &mut nd.ni_cnd, &mut vattr);
        vput(dvp);
        if let Err(error) = error {
            m_freem(nam2);
            solock(unp.unp_socket);
            break 'out Err(error);
        }
        solock(unp.unp_socket);
        unp.unp_addr.set(Some(nam2));
        let Some(vp) = nd.ni_vp else {
            panic(format_args!("uipc_bind: VOP_CREATE returned no vnode"));
        };
        vp.set_v_socket(Some(unp.unp_socket));
        unp.unp_vnode.set(Some(vp));
        let cred = p.ucred();
        unp.unp_connid.set(Sockpeercred {
            uid: cred.cr_uid.get(),
            gid: cred.cr_gid.get(),
            pid: p.process().ps_pid.get(),
        });
        unp.set_flags(UNP_FEIDSBIND);
        let _ = VOP_UNLOCK(vp);
        Ok(())
    };
    // out:
    kernel_unlock();
    unp.clear_flags(UNP_BINDING);

    error
}

/// `uipc_listen(so)`: only a bound socket listens.
pub fn uipc_listen(so: &'static Socket) -> Result<(), Errno> {
    let unp = unpcb_of(so);

    if unp.has_flags(UNP_BINDING | UNP_CONNECTING) {
        return Err(Errno::EINVAL);
    }
    if unp.unp_vnode.get().is_none() {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

/// `uipc_connect(so, nam)`.
pub fn uipc_connect(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    unp_connect(so, nam, curproc_or_panic("uipc_connect"))
}

/// `uipc_accept(so, nam)`: pass back name of connected socket, if it was bound and we are
/// still connected (our peer may have closed already!).
pub fn uipc_accept(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    let unp = unpcb_of(so);

    let so2 = unp_solock_peer(so);
    uipc_setaddr(unp.unp_conn.get(), nam);

    if let Some(so2) = so2
        && !ptr::eq(so2, so)
    {
        sounlock(so2);
    }
    Ok(())
}

/// `uipc_disconnect(so)`.
pub fn uipc_disconnect(so: &'static Socket) -> Result<(), Errno> {
    let unp = unpcb_of(so);

    unp_disconnect(unp);
    Ok(())
}

/// `uipc_shutdown(so)`: no more sending; the peer can receive no more.
pub fn uipc_shutdown(so: &'static Socket) -> Result<(), Errno> {
    let unp = unpcb_of(so);

    socantsendmore(so);

    if let Some(conn) = unp.unp_conn.get() {
        let so2 = conn.unp_socket;
        socantrcvmore(so2);
    }

    Ok(())
}

/// `uipc_dgram_shutdown(so)`.
pub fn uipc_dgram_shutdown(so: &'static Socket) -> Result<(), Errno> {
    socantsendmore(so);
    Ok(())
}

/// `uipc_rcvd(so)`: data was taken off `so`'s receive buffer: adjust backpressure on sender
/// and wakeup any waiting to write.
pub fn uipc_rcvd(so: &'static Socket) {
    let unp = unpcb_of(so);

    let Some(conn) = unp.unp_conn.get() else {
        return;
    };
    let so2 = conn.unp_socket;

    // Adjust backpressure on sender and wakeup any waiting to write.
    mtx_enter(&so.so_rcv.sb_mtx);
    mtx_enter(&so2.so_snd.sb_mtx);
    so2.so_snd.sb_mbcnt.set(so.so_rcv.sb_mbcnt.get());
    so2.so_snd.sb_cc.set(so.so_rcv.sb_cc.get());
    mtx_leave(&so2.so_snd.sb_mtx);
    mtx_leave(&so.so_rcv.sb_mtx);
    sowwakeup(so2);
}

/// `uipc_send(so, m, nam, control)`: the stream and sequenced packet send: straight into
/// the peer's receive buffer.
pub fn uipc_send(
    so: &'static Socket,
    m: Option<&'static Mbuf>,
    _nam: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let unp = unpcb_of(so);
    let mut m = m;
    let mut control = control;

    let error: Result<(), Errno> = 'out: {
        if let Some(c) = control {
            sounlock(so);
            let error = unp_internalize(c, curproc_or_panic("uipc_send"));
            solock(so);
            if let Err(e) = error {
                break 'out Err(e);
            }
        }

        let error: Result<(), Errno> = 'dispose: {
            // We hold both solock() and `sb_mtx' mutex while modifying SS_CANTSENDMORE flag.
            // solock() is enough to check it.
            if so.so_snd.has_state(SS_CANTSENDMORE) {
                break 'dispose Err(Errno::EPIPE);
            }
            let Some(conn) = unp.unp_conn.get() else {
                break 'dispose Err(Errno::ENOTCONN);
            };

            let so2 = conn.unp_socket;

            // Send to paired receive port, and then raise send buffer counts to maintain
            // backpressure. Wake up readers.
            //
            // sbappend*() should be serialized together with so_snd modification.
            mtx_enter(&so2.so_rcv.sb_mtx);
            mtx_enter(&so.so_snd.sb_mtx);
            if let Some(c) = control {
                if sbappendcontrol(&so2.so_rcv, m, c) {
                    control = None;
                } else {
                    mtx_leave(&so.so_snd.sb_mtx);
                    mtx_leave(&so2.so_rcv.sb_mtx);
                    break 'dispose Err(Errno::ENOBUFS);
                }
            } else if so.so_type.get() == SOCK_SEQPACKET {
                sbappendrecord(&so2.so_rcv, m);
            } else {
                sbappend(&so2.so_rcv, m);
            }
            so.so_snd.sb_mbcnt.set(so2.so_rcv.sb_mbcnt.get());
            so.so_snd.sb_cc.set(so2.so_rcv.sb_cc.get());
            let dowakeup = so2.so_rcv.sb_cc.get() > 0;
            mtx_leave(&so.so_snd.sb_mtx);
            mtx_leave(&so2.so_rcv.sb_mtx);

            if dowakeup {
                sorwakeup(so2);
            }

            m = None;
            Ok(())
        };
        // dispose:
        // we need to undo unp_internalize in case of errors
        if let Some(c) = control
            && error.is_err()
        {
            unp_dispose(Some(c));
        }
        error
    };
    // out:
    m_freem(control);
    m_freem(m);

    error
}

/// The bytes of the address `unp` sends from: its bound address, or `sun_noname`.
fn unp_from(unp: &Unpcb) -> &'static [u8] {
    match unp.unp_addr.get() {
        // SAFETY: a bound address mbuf holds `m_len` bytes of `struct sockaddr_un` and lives
        // until the control block is freed, after every send through it.
        Some(addr) => unsafe {
            slice::from_raw_parts(mtod::<u8>(addr), addr.m_len().get() as usize)
        },
        // SAFETY: `SUN_NONAME` is a `#[repr(C)]` sockaddr of `SUN_NONAME_LEN` bytes without
        // padding, a static.
        None => unsafe {
            slice::from_raw_parts(ptr::from_ref(&SUN_NONAME).cast::<u8>(), SUN_NONAME_LEN)
        },
    }
}

/// `uipc_dgram_send(so, m, nam, control)`: a datagram into the receive buffer of the socket
/// `so` is connected to, or of the one bound to `nam` (connected for the call).
pub fn uipc_dgram_send(
    so: &'static Socket,
    m: Option<&'static Mbuf>,
    nam: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let unp = unpcb_of(so);
    let mut m = m;
    let mut control = control;

    let error: Result<(), Errno> = 'out: {
        if let Some(c) = control {
            sounlock(so);
            let error = unp_internalize(c, curproc_or_panic("uipc_dgram_send"));
            solock(so);
            if let Err(e) = error {
                break 'out Err(e);
            }
        }

        let error: Result<(), Errno> = 'dispose: {
            if let Some(nam) = nam {
                if unp.unp_conn.get().is_some() {
                    break 'dispose Err(Errno::EISCONN);
                }
                if let Err(e) = unp_connect(so, nam, curproc_or_panic("uipc_dgram_send")) {
                    break 'dispose Err(e);
                }
            }

            let Some(conn) = unp.unp_conn.get() else {
                break 'dispose Err(if nam.is_some() {
                    Errno::ECONNREFUSED
                } else {
                    Errno::ENOTCONN
                });
            };

            let so2 = conn.unp_socket;

            let from = unp_from(unp);

            let mut error = Ok(());
            let mut dowakeup = false;
            mtx_enter(&so2.so_rcv.sb_mtx);
            if sbappendaddr(&so2.so_rcv, from, m, control) {
                dowakeup = true;
                m = None;
                control = None;
            } else {
                error = Err(Errno::ENOBUFS);
            }
            mtx_leave(&so2.so_rcv.sb_mtx);

            if dowakeup {
                sorwakeup(so2);
            }
            if nam.is_some() {
                unp_disconnect(unp);
            }
            error
        };
        // dispose:
        // we need to undo unp_internalize in case of errors
        if let Some(c) = control
            && error.is_err()
        {
            unp_dispose(Some(c));
        }
        error
    };
    // out:
    m_freem(control);
    m_freem(m);

    error
}

/// `uipc_abort(so)`: an unaccepted connection goes away.
pub fn uipc_abort(so: &'static Socket) {
    let unp = unpcb_of(so);

    unp_detach(unp);
    sofree(so, true);
}

/// `uipc_sense(so, sb)`: `fstat(2)` of a UNIX domain socket: a fake inode number and the
/// creation time.
pub fn uipc_sense(so: &'static Socket, sb: &mut Stat) -> Result<(), Errno> {
    let unp = unpcb_of(so);

    sb.st_blksize = so.so_snd.sb_hiwat.get() as Blksize;
    sb.st_dev = NODEV;
    if unp.unp_ino.get() == 0 {
        mtx_enter(&UNP_INO_MTX);
        let mut ino = UNP_INO.load(Ordering::Relaxed).wrapping_add(1);
        if ino == 0 {
            ino = 1;
        }
        UNP_INO.store(ino, Ordering::Relaxed);
        unp.unp_ino.set(ino);
        mtx_leave(&UNP_INO_MTX);
    }
    let ctime = unp.unp_ctime.get();
    sb.st_atim = ctime;
    sb.st_mtim = ctime;
    sb.st_ctim = ctime;
    sb.st_ino = unp.unp_ino.get();

    Ok(())
}

/// `uipc_sockaddr(so, nam)`.
pub fn uipc_sockaddr(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    let unp = unpcb_of(so);

    uipc_setaddr(Some(unp), nam);
    Ok(())
}

/// `uipc_peeraddr(so, nam)`.
pub fn uipc_peeraddr(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    let unp = unpcb_of(so);

    let so2 = unp_solock_peer(so);
    uipc_setaddr(unp.unp_conn.get(), nam);
    if let Some(so2) = so2
        && !ptr::eq(so2, so)
    {
        sounlock(so2);
    }
    Ok(())
}

/// `uipc_connect2(so, so2)`: `socketpair(2)`: connects the two and records the creator as
/// both peers' credentials.
pub fn uipc_connect2(so: &'static Socket, so2: &'static Socket) -> Result<(), Errno> {
    let unp = unpcb_of(so);

    unp_connect2(so, so2)?;

    let p = curproc_or_panic("uipc_connect2");
    let cred = p.ucred();
    let connid = Sockpeercred {
        uid: cred.cr_uid.get(),
        gid: cred.cr_gid.get(),
        pid: p.process().ps_pid.get(),
    };
    unp.unp_connid.set(connid);
    unp.set_flags(UNP_FEIDS);
    let unp2 = unpcb_of(so2);
    unp2.unp_connid.set(connid);
    unp2.set_flags(UNP_FEIDS);

    Ok(())
}

/// `uipc_sysctl`: `net.unix`: the buffer sizes per type, the descriptors in flight and the
/// deferred ones. All sysctl names at this level are terminal.
pub fn uipc_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let mut valp = &UNP_DEFER;

    let Some(&name0) = name.first() else {
        return Err(Errno::EISDIR);
    };
    match name0 {
        SOCK_STREAM => {
            if name.len() != 2 {
                return Err(Errno::ENOTDIR);
            }
            sysctl_bounded_arr(&UNPSTCTL_VARS, &name[1..], oldp, oldlenp, newp, newlen)
        }
        SOCK_SEQPACKET => {
            if name.len() != 2 {
                return Err(Errno::ENOTDIR);
            }
            sysctl_bounded_arr(&UNPSQCTL_VARS, &name[1..], oldp, oldlenp, newp, newlen)
        }
        SOCK_DGRAM => {
            if name.len() != 2 {
                return Err(Errno::ENOTDIR);
            }
            sysctl_bounded_arr(&UNPDGCTL_VARS, &name[1..], oldp, oldlenp, newp, newlen)
        }
        NET_UNIX_INFLIGHT | NET_UNIX_DEFERRED => {
            if name0 == NET_UNIX_INFLIGHT {
                valp = &UNP_RIGHTS;
            }
            if name.len() != 1 {
                return Err(Errno::ENOTDIR);
            }
            sysctl_rdint(oldp, oldlenp, newp, valp.load(Ordering::Relaxed))
        }
        _ => Err(Errno::ENOPROTOOPT),
    }
}

/// `unp_detach(unp)`: takes the control block apart: the bound vnode, the connection, the
/// sockets connected to it; frees it once nobody holds a reference.
pub fn unp_detach(unp: &'static Unpcb) {
    let so = unp.unp_socket;
    let vp = unp.unp_vnode.get();

    unp.unp_vnode.set(None);

    rw_enter_write(&UNP_GC_LOCK);
    // SAFETY: `uipc_attach` linked the control block; `unp_gc_lock` is held.
    unsafe { ListHead::<UnpHead>::remove(unp) };
    rw_exit_write(&UNP_GC_LOCK);

    if let Some(vp) = vp {
        // Enforce `i_lock' -> solock() lock order.
        sounlock(so);
        let _ = VOP_LOCK(vp, LK_EXCLUSIVE);
        vp.set_v_socket(None);

        kernel_lock();
        vput(vp);
        kernel_unlock();
        solock(so);
    }

    if unp.unp_conn.get().is_some() {
        // Datagram socket could be connected to itself. Such socket will be disconnected
        // here.
        unp_disconnect(unp);
    }

    while let Some(unp2) = unp.unp_refs.first() {
        // SAFETY: a control block on `unp_refs` is a live pool item until it disconnects,
        // which takes it off the list under this socket's lock.
        let unp2: &'static Unpcb = unsafe { &*ptr::from_ref(unp2) };
        let so2 = unp2.unp_socket;

        if ptr::from_ref(so) < ptr::from_ref(so2) {
            solock(so2);
        } else {
            unp_ref(unp2);
            sounlock(so);
            solock(so2);
            solock(so);

            if !same_unp(unp2.unp_conn.get(), unp) {
                // `unp2' was disconnected due to re-lock.
                sounlock(so2);
                unp_rele(unp2);
                continue;
            }

            unp_rele(unp2);
        }

        unp2.unp_conn.set(None);
        // SAFETY: `unp2` is on `unp_refs` (it is connected to `unp`); both sockets are
        // locked.
        unsafe { unp.unp_refs.remove(unp2) };
        so2.set_error(Some(Errno::ECONNRESET));
        so2.clear_state(SS_ISCONNECTED);

        sounlock(so2);
    }

    sounlock(so);
    refcnt_finalize(&unp.unp_refcnt, "unpfinal");
    solock(so);

    soisdisconnected(so);
    so.so_pcb.set(ptr::null_mut());
    m_freem(unp.unp_addr.get());
    pool_put(&UNPCB_POOL, NonNull::from(unp).cast());
    if UNP_RIGHTS.load(Ordering::Relaxed) != 0 {
        task_add(SYSTQMP, &UNP_GC_TASK);
    }
}

/// `unp_connect(so, nam, p)`: connects `so` to the socket bound to the path in `nam`: for a
/// connection-oriented socket, a new socket from the listener's `sonewconn`.
pub fn unp_connect(so: &'static Socket, nam: &'static Mbuf, p: &Proc) -> Result<(), Errno> {
    let unp = unpcb_of(so);
    if unp.has_flags(UNP_BINDING | UNP_CONNECTING) {
        return Err(Errno::EISCONN);
    }
    unp_nam2sun(nam)?;

    // SAFETY: `unp_nam2sun` checked that `nam` holds a `sockaddr_un` of `m_len` bytes whose
    // path is NUL-terminated within them; `nam` is the caller's for the call.
    let path: &[u8] = unsafe {
        slice::from_raw_parts(
            mtod::<u8>(nam).add(SockaddrUn::PATH_OFFSET),
            nam.m_len().get() as usize - SockaddrUn::PATH_OFFSET,
        )
    };
    let mut nd = ndinit(LOOKUP, FOLLOW | LOCKLEAF, NiDirp::Sys(path), p);
    nd.ni_pledge = PLEDGE_UNIX;
    nd.ni_unveil = UNVEIL_WRITE;

    unp.set_flags(UNP_CONNECTING);

    // Enforce `i_lock' -> `solock' because fifo subsystem requires it. The socket can't be
    // closed concurrently because the file descriptor reference is still held.

    sounlock(so);

    kernel_lock();
    let mut error: Result<(), Errno> = 'unlock: {
        if let Err(e) = namei(&mut nd) {
            break 'unlock Err(e);
        }
        let Some(vp) = nd.ni_vp else {
            panic(format_args!("unp_connect: namei returned no vnode"));
        };
        let error: Result<(), Errno> = 'put: {
            if vp.v_type.get() != VSOCK {
                break 'put Err(Errno::ENOTSOCK);
            }
            if let Err(e) = VOP_ACCESS(vp, VWRITE, p.p_ucred.get(), p) {
                break 'put Err(e);
            }
            let Some(so2) = vp.v_socket() else {
                break 'put Err(Errno::ECONNREFUSED);
            };
            if so.so_type.get() != so2.so_type.get() {
                break 'put Err(Errno::EPROTOTYPE);
            }

            let so2 = if so.pr_flags(PR_CONNREQUIRED) {
                solock(so2);

                let so3 = if so2.has_options(SO_ACCEPTCONN) {
                    sonewconn(so2, 0, M_WAIT)
                } else {
                    None
                };
                let Some(so3) = so3 else {
                    sounlock(so2);
                    break 'put Err(Errno::ECONNREFUSED);
                };

                // Since `so2' is protected by vnode(9) lock, `so3' can't be PRU_ABORT'ed
                // here.
                sounlock(so2);
                sounlock(so3);
                solock_pair(so, so3);

                let unp2 = unpcb_of(so2);
                let unp3 = unpcb_of(so3);

                // `unp_addr', `unp_connid' and 'UNP_FEIDSBIND' flag are immutable since we
                // set them in uipc_bind().
                if let Some(addr) = unp2.unp_addr.get() {
                    unp3.unp_addr.set(m_copym(addr, 0, M_COPYALL, M_DONTWAIT));
                }
                let cred = p.ucred();
                unp3.unp_connid.set(Sockpeercred {
                    uid: cred.cr_uid.get(),
                    gid: cred.cr_gid.get(),
                    pid: p.process().ps_pid.get(),
                });
                unp3.set_flags(UNP_FEIDS);

                if unp2.has_flags(UNP_FEIDSBIND) {
                    unp.unp_connid.set(unp2.unp_connid.get());
                    unp.set_flags(UNP_FEIDS);
                }

                so3
            } else {
                solock_pair(so, so2);
                so2
            };

            let error = unp_connect2(so, so2);

            // `so2' can't be PRU_ABORT'ed concurrently
            sounlock_pair(so, so2);
            error
        };
        // put:
        vput(vp);
        error
    };
    // unlock:
    kernel_unlock();
    solock(so);
    unp.clear_flags(UNP_CONNECTING);

    // The peer socket could be closed by concurrent thread when `so' and `vp' are unlocked.
    if error.is_ok() && unp.unp_conn.get().is_none() {
        error = Err(Errno::ECONNREFUSED);
    }

    error
}

/// `unp_connect2(so, so2)`: links two locked sockets of one type: a datagram socket refers
/// to its peer, stream peers refer to each other.
pub fn unp_connect2(so: &'static Socket, so2: &'static Socket) -> Result<(), Errno> {
    let unp = unpcb_of(so);

    soassertlocked(so);
    soassertlocked(so2);

    if so2.so_type.get() != so.so_type.get() {
        return Err(Errno::EPROTOTYPE);
    }
    let unp2 = unpcb_of(so2);
    unp.unp_conn.set(Some(unp2));
    match so.so_type.get() {
        SOCK_DGRAM => {
            // SAFETY: both sockets are locked; `unp` is a live pool item that leaves the list
            // when it disconnects (`unp_disconnect`, also run by `unp_detach`).
            unsafe { unp2.unp_refs.insert_head(unp) };
            soisconnected(so);
        }

        SOCK_STREAM | SOCK_SEQPACKET => {
            unp2.unp_conn.set(Some(unp));
            soisconnected(so);
            soisconnected(so2);
        }

        _ => panic(format_args!("unp_connect2")),
    }
    Ok(())
}

/// `unp_disconnect(unp)`: undoes `unp_connect2`.
pub fn unp_disconnect(unp: &'static Unpcb) {
    let Some(so2) = unp_solock_peer(unp.unp_socket) else {
        return;
    };

    let Some(unp2) = unp.unp_conn.get() else {
        panic(format_args!("unp_disconnect: not connected"));
    };
    unp.unp_conn.set(None);

    match unp.unp_socket.so_type.get() {
        SOCK_DGRAM => {
            // SAFETY: a connected datagram control block is on its peer's `unp_refs`
            // (`unp_connect2`); both sockets are locked.
            unsafe { unp2.unp_refs.remove(unp) };
            unp.unp_socket.clear_state(SS_ISCONNECTED);
        }

        SOCK_STREAM | SOCK_SEQPACKET => {
            unp.unp_socket.so_snd.sb_mbcnt.set(0);
            unp.unp_socket.so_snd.sb_cc.set(0);
            soisdisconnected(unp.unp_socket);
            unp2.unp_conn.set(None);
            unp2.unp_socket.so_snd.sb_mbcnt.set(0);
            unp2.unp_socket.so_snd.sb_cc.set(0);
            soisdisconnected(unp2.unp_socket);
        }

        _ => {}
    }

    if !ptr::eq(so2, unp.unp_socket) {
        sounlock(so2);
    }
}

/// `fptounp(fp)`: the control block of a UNIX domain socket file, `None` for any other.
fn fptounp(fp: &File) -> Option<&'static Unpcb> {
    if fp.f_type.get() != DTYPE_SOCKET {
        return None;
    }
    // SAFETY: a socket file's `f_data` is its socket, or NULL once closed.
    let so = unsafe { fp.f_data.get().cast::<Socket>().cast_const().as_ref() }?;
    if !ptr::eq(so.so_proto.pr_domain, &UNIXDOMAIN) {
        return None;
    }
    sotounpcb(so)
}

/// The file of an in-flight `struct fdpass`.
///
/// # Safety
///
/// `rp` points at a readable (maybe unaligned) `struct fdpass` whose `fp` is a file the
/// message holds a reference to.
unsafe fn fdpass_file(rp: *const Fdpass) -> &'static File {
    // SAFETY: the caller's contract.
    let fp = unsafe { rp.read_unaligned() }.fp;
    // SAFETY: as above: a referenced file is a live `file_pool` item.
    match unsafe { fp.as_ref() } {
        Some(fp) => fp,
        None => panic(format_args!("fdpass {:p}: no file", rp)),
    }
}

/// `unp_externalize(rights, controllen, flags)`: installs the files of an internalized
/// `SCM_RIGHTS` message in the receiving process's table and rewrites the message as their
/// descriptor numbers; on failure the files are discarded.
pub fn unp_externalize(
    rights: &'static Mbuf,
    controllen: Socklen,
    flags: i32,
) -> Result<(), Errno> {
    let p = curproc_or_panic("unp_externalize"); // XXX
    let mut cm = cmsghdr_of(rights);
    let fdp = p.fd();
    let hdr = cmsg_align(size_of::<Cmsghdr>());

    // This code only works because SCM_RIGHTS is the only supported control message type
    // on unix sockets. Enforce this here.
    if cm.cmsg_type != SCM_RIGHTS || cm.cmsg_level != SOL_SOCKET {
        return Err(Errno::EINVAL);
    }

    let nfds = cmsg_len_of(&cm).saturating_sub(hdr) / size_of::<Fdpass>();
    let controllen = (controllen as usize).saturating_sub(hdr);
    let rp0 = cmsg_data_of(rights).cast::<Fdpass>();
    let mut fds: Option<NonNull<u8>> = None;

    let error: Result<(), Errno> = 'out: {
        if nfds > controllen / size_of::<i32>() {
            break 'out Err(Errno::EMSGSIZE);
        }

        // Make sure the recipient should be able to see the descriptors..

        // fdp->fd_rdir requires KERNEL_LOCK()
        kernel_lock();

        let mut error = Ok(());
        for i in 0..nfds {
            // SAFETY: the message holds `nfds` entries after its header.
            let fp = unsafe { fdpass_file(rp0.wrapping_add(i)) };
            if let Err(e) = pledge_recvfd(p, fp) {
                error = Err(e);
                break;
            }

            // No to block devices. If passing a directory, make sure that it is underneath
            // the root.
            if let Some(rdir) = fdp.fd_rdir.get()
                && fp.f_type.get() == DTYPE_VNODE
            {
                let vp = fp.vnode();

                if vp.v_type.get() == VBLK || (vp.v_type.get() == VDIR && !vn_isunder(vp, rdir, p))
                {
                    error = Err(Errno::EPERM);
                    break;
                }
            }
        }

        kernel_unlock();

        if let Err(e) = error {
            break 'out Err(e);
        }

        let fdv: &mut [i32] = if nfds > 0 {
            let Some(mem) = mallocarray(nfds, size_of::<i32>(), M_TEMP, M_WAITOK) else {
                break 'out Err(Errno::ENOMEM);
            };
            fds = Some(mem);
            // SAFETY: a fresh allocation of `nfds` ints, ours until the `free` below;
            // zeroed before the slice is made.
            unsafe {
                mem.as_ptr().write_bytes(0, nfds * size_of::<i32>());
                slice::from_raw_parts_mut(mem.as_ptr().cast::<i32>(), nfds)
            }
        } else {
            &mut []
        };

        fdplock(fdp);
        // restart:
        // First loop -- allocate file descriptor table slots for the new descriptors.
        'restart: loop {
            for i in 0..nfds {
                let fd = match fdalloc(p, 0) {
                    Ok(fd) => fd,
                    Err(e) => {
                        // Back out what we've done so far.
                        for &fd in fdv[..i].iter().rev() {
                            fdremove(fdp, fd);
                        }

                        if e == Errno::ENOSPC {
                            match fdexpand(p) {
                                Ok(()) => continue 'restart,
                                Err(e) => {
                                    fdpunlock(fdp);
                                    break 'out Err(e);
                                }
                            }
                        }

                        fdpunlock(fdp);

                        // This is the error that has historically been returned, and some
                        // callers may expect it.

                        break 'out Err(Errno::EMSGSIZE);
                    }
                };
                fdv[i] = fd;

                // Make the slot reference the descriptor so that fdalloc() works properly..
                // We finalize it all in the loop below.
                // SAFETY: as above.
                let rp = unsafe { rp0.wrapping_add(i).read_unaligned() };
                // SAFETY: as above.
                let fp = unsafe { fdpass_file(rp0.wrapping_add(i)) };
                mtx_enter(&fdp.fd_fplock);
                kassert!(fdp.ofile(fd as usize).is_none());
                fdp.set_ofile(fd as usize, Some(fp));
                mtx_leave(&fdp.fd_fplock);

                let mut ofl = rp.flags as u8 & UF_PLEDGED;
                if flags & MSG_CMSG_CLOEXEC != 0 {
                    ofl |= UF_EXCLOSE;
                }
                if flags & MSG_CMSG_CLOFORK != 0 {
                    ofl |= UF_FORKCLOSE;
                }
                fdp.set_ofileflags(fd as usize, ofl);
            }
            break;
        }

        // Keep `fdp' locked to prevent concurrent close() of just inserted descriptors. Such
        // descriptors could have the only `f_count' reference which is now shared between
        // control message and `fdp'.

        // Now that adding them has succeeded, update all of the descriptor passing state.
        for i in 0..nfds {
            // SAFETY: as above.
            let fp = unsafe { fdpass_file(rp0.wrapping_add(i)) };
            if let Some(unp) = fptounp(fp) {
                rw_enter_write(&UNP_GC_LOCK);
                unp.unp_msgcount.set(unp.unp_msgcount.get() - 1);
                rw_exit_write(&UNP_GC_LOCK);
            }
        }
        fdpunlock(fdp);

        mtx_enter(&UNP_RIGHTS_MTX);
        UNP_RIGHTS.fetch_sub(nfds as i32, Ordering::Relaxed);
        mtx_leave(&UNP_RIGHTS_MTX);

        // Copy temporary array to message and adjust length, in case of transition from
        // large struct file pointers to ints.
        for (i, &fd) in fdv.iter().enumerate() {
            // SAFETY: the ints go where the larger `fdpass` entries were, inside the message.
            unsafe {
                cmsg_data_of(rights)
                    .cast::<i32>()
                    .add(i)
                    .write_unaligned(fd)
            };
        }
        cm.cmsg_len = cmsg_len(nfds * size_of::<i32>()) as Socklen;
        set_cmsghdr(rights, cm);
        rights.m_len().set(cmsg_len(nfds * size_of::<i32>()) as u32);
        Ok(())
    };
    // out:
    if let Some(mem) = fds {
        free(mem, M_TEMP, nfds * size_of::<i32>());
    }

    if error.is_err() && nfds > 0 {
        // No lock required. We are the only `cm' holder.
        // SAFETY: the message's `nfds` entries, which this call owns.
        unsafe { unp_discard(rp0, nfds) };
    }

    error
}

/// `unp_internalize(control, p)`: turns the descriptor numbers of an `SCM_RIGHTS` message
/// into references to their files (`struct fdpass`), growing the mbuf into a cluster when
/// the larger entries do not fit.
pub fn unp_internalize(control: &'static Mbuf, p: &Proc) -> Result<(), Errno> {
    let fdp = p.fd();
    let hdr = cmsg_align(size_of::<Cmsghdr>());

    // Check for two potential msg_controllen values because IETF stuck their nose in a place
    // it does not belong.
    if (control.m_len().get() as usize) < cmsg_len(0) {
        return Err(Errno::EINVAL);
    }
    let mut cm = cmsghdr_of(control);
    if cmsg_len_of(&cm) < cmsg_len(0) {
        return Err(Errno::EINVAL);
    }
    let m_len = control.m_len().get() as usize;
    if cm.cmsg_type != SCM_RIGHTS
        || cm.cmsg_level != SOL_SOCKET
        || !(cmsg_len_of(&cm) == m_len || m_len == cmsg_align(cmsg_len_of(&cm)))
    {
        return Err(Errno::EINVAL);
    }
    let nfds = (cmsg_len_of(&cm) - hdr) / size_of::<i32>();

    mtx_enter(&UNP_RIGHTS_MTX);
    if UNP_RIGHTS.load(Ordering::Relaxed) as usize + nfds
        > (MAXFILES.load(Ordering::Relaxed) / 10) as usize
    {
        mtx_leave(&UNP_RIGHTS_MTX);
        return Err(Errno::EMFILE);
    }
    UNP_RIGHTS.fetch_add(nfds as i32, Ordering::Relaxed);
    mtx_leave(&UNP_RIGHTS_MTX);

    let error: Result<(), Errno> = 'nospace: {
        // Make sure we have room for the struct file pointers
        // morespace:
        loop {
            let neededspace =
                cmsg_space(nfds * size_of::<Fdpass>()) as i64 - i64::from(control.m_len().get());
            if neededspace <= i64::from(m_trailingspace(control)) {
                break;
            }
            // if we already have a cluster, the message is just too big
            if control.m_flags().get() & M_EXT != 0 {
                break 'nospace Err(Errno::E2BIG);
            }

            // copy cmsg data temporarily out of the mbuf
            let len = control.m_len().get() as usize;
            let mut tmp = [0u8; MLEN];
            // SAFETY: a plain mbuf holds at most `MLEN` bytes, `len` of them data.
            unsafe { ptr::copy_nonoverlapping(mtod::<u8>(control), tmp.as_mut_ptr(), len) };

            // allocate a cluster and try again
            mclget(control, M_WAIT);
            if control.m_flags().get() & M_EXT == 0 {
                break 'nospace Err(Errno::ENOBUFS); // allocation failed
            }

            // copy the data back into the cluster
            // SAFETY: the cluster holds at least `MCLBYTES` bytes, more than `len`.
            unsafe { ptr::copy_nonoverlapping(tmp.as_ptr(), mtod::<u8>(control), len) };
        }

        // adjust message & mbuf to note amount of space actually used.
        cm.cmsg_len = cmsg_len(nfds * size_of::<Fdpass>()) as Socklen;
        set_cmsghdr(control, cm);
        control
            .m_len()
            .set(cmsg_space(nfds * size_of::<Fdpass>()) as u32);

        let ip0 = cmsg_data_of(control).cast::<i32>();
        let rp0 = cmsg_data_of(control).cast::<Fdpass>();
        fdplock(fdp);
        // The entries are filled from the last one down: a `struct fdpass` written at index
        // j covers the ints at 4j and above, which have been read by then.
        let mut i = 0;
        let mut failed: Option<(Errno, Option<&'static File>)> = None;
        while i < nfds {
            let j = nfds - 1 - i;
            // SAFETY: the ints of the message are inside the (grown) mbuf.
            let fd = unsafe { ip0.add(j).read_unaligned() };
            let Some(fp) = fd_getfile(fdp, fd) else {
                failed = Some((Errno::EBADF, None));
                break;
            };
            if fp.f_count.load(Ordering::SeqCst) >= FDUP_MAX_COUNT {
                failed = Some((Errno::EDEADLK, Some(fp)));
                break;
            } else if fdp.ofileflags(fd as usize) & UF_PLEDGEOPEN != 0 {
                failed = Some((Errno::EPERM, Some(fp)));
                break;
            }
            if let Err(e) = pledge_sendfd(p, fp) {
                failed = Some((e, Some(fp)));
                break;
            }

            // kqueue and vmm descriptors cannot be copied
            if fp.f_type.get() == DTYPE_KQUEUE || fp.f_type.get() == DTYPE_VMM {
                failed = Some((Errno::EINVAL, Some(fp)));
                break;
            }
            // NKCOV: kcov(4) is not configured.
            let entry = Fdpass {
                fp: ptr::from_ref(fp),
                flags: i32::from(fdp.ofileflags(fd as usize) & UF_PLEDGED),
            };
            // SAFETY: the entry lies inside the mbuf, grown to `CMSG_SPACE` of them.
            unsafe { rp0.add(j).write_unaligned(entry) };
            if let Some(unp) = fptounp(fp) {
                rw_enter_write(&UNP_GC_LOCK);
                unp.unp_msgcount.set(unp.unp_msgcount.get() + 1);
                unp.unp_file.set(Some(fp));
                rw_exit_write(&UNP_GC_LOCK);
            }
            i += 1;
        }
        fdpunlock(fdp);
        let Some((error, fp)) = failed else {
            return Ok(());
        };
        // fail:
        if let Some(fp) = fp {
            let _ = frele(fp, p);
        }
        // Back out what we just did.
        for j in (nfds - i)..nfds {
            // SAFETY: the entries filled above, which hold their files' references.
            let fp = unsafe { fdpass_file(rp0.add(j)) };
            if let Some(unp) = fptounp(fp) {
                rw_enter_write(&UNP_GC_LOCK);
                unp.unp_msgcount.set(unp.unp_msgcount.get() - 1);
                rw_exit_write(&UNP_GC_LOCK);
            }
            let _ = frele(fp, p);
        }
        Err(error)
    };

    // nospace:
    mtx_enter(&UNP_RIGHTS_MTX);
    UNP_RIGHTS.fetch_sub(nfds as i32, Ordering::Relaxed);
    mtx_leave(&UNP_RIGHTS_MTX);

    error
}

/// `unp_gc(arg)`: the garbage collector task: closes the deferred files, then finds the
/// sockets referenced only by in-flight messages in other such sockets' buffers and
/// discards those buffers.
pub fn unp_gc(_arg: *mut c_void) {
    rw_enter_write(&UNP_GC_LOCK);
    'unlock: {
        if UNP_GCING.load(Ordering::Relaxed) != 0 {
            break 'unlock;
        }
        UNP_GCING.store(1, Ordering::Relaxed);
        rw_exit_write(&UNP_GC_LOCK);

        rw_enter_write(&UNP_DF_LOCK);
        // close any fds on the deferred list
        while let Some(defer) = UNP_DEFERRED.0.first() {
            // SAFETY: a deferral on the list is a `malloc`ed set nothing else refers to once
            // it is taken off (under `unp_df_lock`, held).
            let defer: &UnpDeferral = unsafe { &*ptr::from_ref(defer) };
            // SAFETY: as above.
            unsafe { UNP_DEFERRED.0.remove_head() };
            rw_exit_write(&UNP_DF_LOCK);
            let n = defer.ud_n as usize;
            for i in 0..n {
                // SAFETY: the deferral holds `ud_n` entries after its header.
                let rp = unsafe { defer.fps().add(i).read_unaligned() };
                // SAFETY: a non-NULL entry is a file the deferral holds a reference to.
                let Some(fp) = (unsafe { rp.fp.as_ref() }) else {
                    continue;
                };
                if let Some(unp) = fptounp(fp) {
                    rw_enter_write(&UNP_GC_LOCK);
                    unp.unp_msgcount.set(unp.unp_msgcount.get() - 1);
                    rw_exit_write(&UNP_GC_LOCK);
                }
                mtx_enter(&UNP_RIGHTS_MTX);
                UNP_RIGHTS.fetch_sub(1, Ordering::Relaxed);
                mtx_leave(&UNP_RIGHTS_MTX);
                // closef() expects a refcount of 2
                fref(fp);
                let _ = closef(fp, None::<&Proc>);
            }
            free(NonNull::from(defer).cast(), M_TEMP, UnpDeferral::size(n));
            rw_enter_write(&UNP_DF_LOCK);
        }
        rw_exit_write(&UNP_DF_LOCK);

        let mut nunref = 0;

        rw_enter_write(&UNP_GC_LOCK);

        // Determine sockets which may be prospectively dead. Such sockets have their
        // `unp_msgcount' equal to the `f_count'. If `unp_msgcount' is 0, the socket has not
        // been passed and can't be unreferenced.
        for unp in UNP_HEAD.0.iter() {
            unp.unp_gcflags.set(0);

            if unp.unp_msgcount.get() == 0 {
                continue;
            }
            let Some(fp) = unp.unp_file.get() else {
                continue;
            };
            if i64::from(fp.f_count.load(Ordering::SeqCst)) == unp.unp_msgcount.get() {
                unp.unp_gcflags.set(unp.unp_gcflags.get() | UNP_GCDEAD);
                unp.unp_gcrefs.set(unp.unp_msgcount.get());
                nunref += 1;
            }
        }

        // Scan all sockets previously marked as dead. Remove the `unp_gcrefs' reference each
        // socket holds on any dead socket in its buffer.
        for unp in UNP_HEAD.0.iter() {
            if unp.unp_gcflags.get() & UNP_GCDEAD == 0 {
                continue;
            }
            let so = unp.unp_socket;
            mtx_enter(&so.so_rcv.sb_mtx);
            unp_scan(so.so_rcv.sb_mb.get(), unp_remove_gcrefs);
            mtx_leave(&so.so_rcv.sb_mtx);
        }

        // If the dead socket has `unp_gcrefs' reference counter greater than 0, it can't be
        // unreferenced. Mark it as alive and increment the `unp_gcrefs' reference for each
        // dead socket within its buffer. Repeat this until we have no new alive sockets
        // found.
        loop {
            UNP_DEFER.store(0, Ordering::Relaxed);

            for unp in UNP_HEAD.0.iter() {
                if unp.unp_gcflags.get() & UNP_GCDEAD == 0 {
                    continue;
                }
                if unp.unp_gcrefs.get() == 0 {
                    continue;
                }

                unp.unp_gcflags.set(unp.unp_gcflags.get() & !UNP_GCDEAD);

                let so = unp.unp_socket;
                mtx_enter(&so.so_rcv.sb_mtx);
                unp_scan(so.so_rcv.sb_mb.get(), unp_restore_gcrefs);
                mtx_leave(&so.so_rcv.sb_mtx);

                kassert!(nunref > 0);
                nunref -= 1;
            }
            if UNP_DEFER.load(Ordering::Relaxed) <= 0 {
                break;
            }
        }

        // If there are any unreferenced sockets, then for each dispose of files in its
        // receive buffer and then close it.
        if nunref != 0 {
            for unp in UNP_HEAD.0.iter() {
                if unp.unp_gcflags.get() & UNP_GCDEAD != 0 {
                    let sb = &unp.unp_socket.so_rcv;

                    // This socket could still be connected and if so it's `so_rcv' is still
                    // accessible by concurrent PRU_SEND thread.

                    mtx_enter(&sb.sb_mtx);
                    let m = sb.sb_mb.get();
                    sb.zero_counts();
                    sb.sb_timeo_nsecs.set(INFSLP);
                    mtx_leave(&sb.sb_mtx);

                    unp_scan(m, unp_discard);
                    m_purge(m);
                }
            }
        }

        UNP_GCING.store(0, Ordering::Relaxed);
    }
    // unlock:
    rw_exit_write(&UNP_GC_LOCK);
}

/// `unp_dispose(m)`: discards the files in the internalized rights of the chain `m`.
pub fn unp_dispose(m: Option<&'static Mbuf>) {
    if m.is_some() {
        unp_scan(m, unp_discard);
    }
}

/// `unp_scan(m0, op)`: applies `op` to the files of every `SCM_RIGHTS` message in the
/// records of `m0`.
pub fn unp_scan(m0: Option<&'static Mbuf>, op: UnpScanOp) {
    let mut m0 = m0;

    while let Some(rec) = m0 {
        let mut m = Some(rec);
        while let Some(mm) = m {
            if i32::from(mm.m_type().get()) == MT_CONTROL
                && mm.m_len().get() as usize >= size_of::<Cmsghdr>()
            {
                let cm = cmsghdr_of(mm);
                if cm.cmsg_level != SOL_SOCKET || cm.cmsg_type != SCM_RIGHTS {
                    m = mm.m_next().get();
                    continue;
                }
                let qfds = cmsg_len_of(&cm).saturating_sub(cmsg_align(size_of::<Cmsghdr>()))
                    / size_of::<Fdpass>();
                if qfds > 0 {
                    let rp = cmsg_data_of(mm).cast::<Fdpass>();
                    // SAFETY: an internalized message holds `qfds` entries after its header,
                    // and the caller holds the buffer (its mutex, or the only reference).
                    unsafe { op(rp, qfds) };
                }
                break; // XXX, but saves time
            }
            m = mm.m_next().get();
        }
        m0 = rec.m_nextpkt().get();
    }
}

/// `unp_discard(rp, nfds)`: hands the `nfds` files at `rp` to the garbage collector task to
/// close, and clears the entries.
///
/// # Safety
///
/// `rp` points at `nfds` (maybe unaligned) entries whose file references the caller owns
/// and gives up.
pub unsafe fn unp_discard(rp: *mut Fdpass, nfds: usize) {
    // copy the file pointers to a deferral structure
    let size = UnpDeferral::size(nfds);
    let Some(mem) = malloc(size, M_TEMP, M_WAITOK) else {
        panic(format_args!("unp_discard: no memory for {} files", nfds));
    };
    let defer = mem.cast::<UnpDeferral>().as_ptr();
    // SAFETY: a fresh allocation of `size` bytes, `malloc`-aligned (8 at least, the
    // structure's alignment); the header is written before the entries.
    unsafe {
        defer.write(UnpDeferral {
            ud_link: SlistEntry::new(),
            ud_n: nfds as i32,
            ud_fp: [],
        });
    }
    // SAFETY: as above; the deferral is ours until it is on the list.
    let defer: &'static UnpDeferral = unsafe { &*defer };
    // SAFETY: the caller's `nfds` entries and the deferral's room for them do not overlap.
    unsafe {
        ptr::copy_nonoverlapping(
            rp.cast::<u8>(),
            defer.fps().cast::<u8>(),
            nfds * size_of::<Fdpass>(),
        );
        ptr::write_bytes(rp.cast::<u8>(), 0, nfds * size_of::<Fdpass>());
    }

    rw_enter_write(&UNP_DF_LOCK);
    // SAFETY: `unp_df_lock` is held; the deferral stays in place until `unp_gc` frees it.
    unsafe { UNP_DEFERRED.0.insert_head(defer) };
    rw_exit_write(&UNP_DF_LOCK);

    task_add(SYSTQMP, &UNP_GC_TASK);
}

/// `unp_remove_gcrefs(rp, nfds)`: a dead socket's buffer holds these files: they do not
/// keep the dead sockets among them alive.
///
/// # Safety
///
/// `rp` points at `nfds` readable (maybe unaligned) entries.
pub unsafe fn unp_remove_gcrefs(rp: *mut Fdpass, nfds: usize) {
    rw_assert_wrlock(&UNP_GC_LOCK);

    for i in 0..nfds {
        // SAFETY: the caller's contract.
        let fp = unsafe { rp.add(i).read_unaligned() }.fp;
        // SAFETY: a non-NULL entry is a file the message holds a reference to.
        let Some(fp) = (unsafe { fp.as_ref() }) else {
            continue;
        };
        let Some(unp) = fptounp(fp) else {
            continue;
        };
        if unp.unp_gcflags.get() & UNP_GCDEAD != 0 {
            kassert!(unp.unp_gcrefs.get() > 0);
            unp.unp_gcrefs.set(unp.unp_gcrefs.get() - 1);
        }
    }
}

/// `unp_restore_gcrefs(rp, nfds)`: a live socket's buffer holds these files: the dead
/// sockets among them are alive after all.
///
/// # Safety
///
/// `rp` points at `nfds` readable (maybe unaligned) entries.
pub unsafe fn unp_restore_gcrefs(rp: *mut Fdpass, nfds: usize) {
    rw_assert_wrlock(&UNP_GC_LOCK);

    for i in 0..nfds {
        // SAFETY: the caller's contract.
        let fp = unsafe { rp.add(i).read_unaligned() }.fp;
        // SAFETY: a non-NULL entry is a file the message holds a reference to.
        let Some(fp) = (unsafe { fp.as_ref() }) else {
            continue;
        };
        let Some(unp) = fptounp(fp) else {
            continue;
        };
        if unp.unp_gcflags.get() & UNP_GCDEAD != 0 {
            unp.unp_gcrefs.set(unp.unp_gcrefs.get() + 1);
            UNP_DEFER.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// `unp_nam2sun(nam, sun, pathlen)`: checks that `nam` holds a UNIX domain address whose
/// path is NUL-terminated (adding the NUL when there is room) and returns the path's
/// length. The `struct sockaddr_un` is `nam`'s data.
pub fn unp_nam2sun(nam: &'static Mbuf) -> Result<usize, Errno> {
    let m_len = nam.m_len().get() as usize;

    if m_len < offset_of!(Sockaddr, sa_data) {
        return Err(Errno::EINVAL);
    }
    // SAFETY: `nam` holds at least `sa_len` and `sa_family` (checked above).
    let (sa_len, sa_family) = unsafe { (*mtod::<u8>(nam), *mtod::<u8>(nam).add(1)) };
    if sa_family != AF_UNIX {
        return Err(Errno::EAFNOSUPPORT);
    }
    if usize::from(sa_len) != m_len {
        return Err(Errno::EINVAL);
    }
    if usize::from(sa_len) > size_of::<SockaddrUn>() {
        return Err(Errno::EINVAL);
    }

    // ensure that sun_path is NUL terminated and fits
    let size = usize::from(sa_len) - SockaddrUn::PATH_OFFSET;
    let path = mtod::<u8>(nam).wrapping_add(SockaddrUn::PATH_OFFSET);
    // SAFETY: the `size` path bytes are inside the mbuf's `m_len` bytes.
    let len = unsafe { slice::from_raw_parts(path, size) }
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(size);
    if len == SUN_PATH_LEN {
        return Err(Errno::EINVAL);
    }
    if len == size {
        if m_trailingspace(nam) == 0 {
            return Err(Errno::EINVAL);
        }
        nam.m_len().set(nam.m_len().get() + 1);
        // SAFETY: the mbuf has room after its data (`m_trailingspace`), which the NUL takes;
        // `sun_len` is the first byte.
        unsafe {
            *mtod::<u8>(nam) += 1;
            *path.add(len) = 0;
        }
    }

    Ok(len)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for the UNIX domain and the socket layer above it, end to end through the
    // kernel functions: stream, datagram and sequenced packet pairs (`socreate`,
    // `soconnect2`, `sosend`, `soreceive`, `soshutdown`, `soclose`), the accept queue
    // (`sonewconn`, `soisconnected`, `soqremque`, `soaccept`, a listener closed with a pending
    // connection), the paths through `namei` on the test file system (`uipc_bind`,
    // `unp_connect`, `unp_nam2sun`), and descriptor passing through the system calls
    // (`socketpair`, `sendmsg`, `recvmsg` with `SCM_RIGHTS`), with the rights of a closed
    // receiver and a cycle of sockets collected by `unp_gc`.
    //
    // The tests run as the thread `vfs_subr`'s setup builds, made `curproc` (the socket layer
    // reads its credentials and descriptor table); they clear `curproc` and dequeue the
    // collector's task before they return, since the task queue outlives the reset memory.

    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq, vec};

    use super::*;
    use crate::kern::kern_descrip::sys_close;
    use crate::kern::kern_task::task_del;
    use crate::kern::sys_generic::{sys_read, sys_write};
    use crate::kern::uipc_mbuf::m_get;
    use crate::kern::uipc_mbuf::tests::mbinit_again;
    use crate::kern::uipc_socket::{
        soaccept, sobind, soclose, soconnect, soconnect2, socreate, sogetopt, soinit, soreceive,
        sosend, soshutdown,
    };
    use crate::kern::uipc_socket2::{
        solock_nonet, solock_shared, soqremque, sounlock_nonet, sounlock_shared,
    };
    use crate::kern::uipc_syscalls::{sys_recvmsg, sys_sendmsg, sys_socketpair};
    use crate::machine::Machine;
    use crate::machine::cpu::Cpu;
    use crate::sys::mbuf::MT_SOOPTS;
    use crate::sys::resource::{RLIM_INFINITY, RLIMIT_NOFILE, Rlimit};
    use crate::sys::resourcevar::Plimit;
    use crate::sys::socket::{
        MSG_DONTWAIT, MSG_NOSIGNAL, MSG_TRUNC, Msghdr, SHUT_WR, SO_PEERCRED, SO_TYPE,
    };
    use crate::sys::socketvar::{SS_CANTRCVMORE, SS_ISCONNECTED, SS_NOFDREF};
    use crate::sys::systm::SysArgs;
    use crate::sys::types::Register;
    use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};

    /// The vfs setup (memory, pools, a thread with a descriptor table), mbufs, the socket and
    /// control block pools, empty global lists; the thread is `curproc`.
    pub(crate) fn setup() -> (MutexGuard<'static, ()>, &'static Proc) {
        let (g, p) = crate::kern::vfs_subr::tests::setup();
        mbinit_again();
        soinit();
        unp_init();
        UNP_HEAD.0.init();
        UNP_DEFERRED.0.init();
        UNP_RIGHTS.store(0, Ordering::Relaxed);
        UNP_DEFER.store(0, Ordering::Relaxed);
        UNP_GCING.store(0, Ordering::Relaxed);
        Machine::set_curproc(Machine::curcpu(), p);
        // fdalloc reads RLIMIT_NOFILE; no other limit matters here.
        let limit: &'static Plimit = std::boxed::Box::leak(std::boxed::Box::new(Plimit::new()));
        for l in &limit.pl_rlimit {
            l.set(Rlimit {
                rlim_cur: RLIM_INFINITY,
                rlim_max: RLIM_INFINITY,
            });
        }
        limit.pl_rlimit[RLIMIT_NOFILE].set(Rlimit {
            rlim_cur: 64,
            rlim_max: 64,
        });
        p.process().ps_limit.set(limit);
        p.p_limit.set(limit);
        (g, p)
    }

    /// Undoes what outlives the reset memory: the collector's task on `systqmp`, `curproc`.
    pub(crate) fn teardown() {
        let _ = task_del(SYSTQMP, &UNP_GC_TASK);
        UNP_HEAD.0.init();
        UNP_DEFERRED.0.init();
        Machine::set_curproc(Machine::curcpu(), ptr::null());
    }

    /// `sosend` of `bytes` from kernel space; the bytes taken.
    pub(crate) fn send(so: &'static Socket, bytes: &[u8], flags: i32) -> Result<usize, Errno> {
        let mut iov = [Iovec {
            iov_base: bytes.as_ptr().cast_mut().cast(),
            iov_len: bytes.len(),
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: bytes.len(),
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_WRITE,
            uio_procp: None,
        };
        sosend(so, None, Some(&mut uio), None, None, flags)?;
        Ok(bytes.len() - uio.uio_resid)
    }

    /// `soreceive` into `buf` in kernel space; the bytes read and the flags returned.
    pub(crate) fn recv(
        so: &'static Socket,
        buf: &mut [u8],
        flags: i32,
    ) -> Result<(usize, i32), Errno> {
        let len = buf.len();
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: len,
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: len,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let mut flags = flags;
        soreceive(so, None, &mut uio, None, None, Some(&mut flags), 0)?;
        Ok((len - uio.uio_resid, flags & !MSG_DONTWAIT))
    }

    /// Two connected sockets of `type_`, as `socketpair(2)` makes them.
    fn pair(type_: i32) -> (&'static Socket, &'static Socket) {
        let a = socreate(i32::from(AF_UNIX), type_, 0).expect("socreate");
        let b = socreate(i32::from(AF_UNIX), type_, 0).expect("socreate");
        assert_eq!(soconnect2(a, b), Ok(()));
        if type_ == SOCK_DGRAM {
            assert_eq!(soconnect2(b, a), Ok(()));
        }
        (a, b)
    }

    /// An `MT_SONAME` mbuf holding `sun_len`, `AF_UNIX` and `path` (no NUL added).
    fn sun(path: &[u8]) -> &'static Mbuf {
        let m = m_get(M_WAIT, MT_SONAME).expect("an mbuf");
        let len = SockaddrUn::PATH_OFFSET + path.len();
        // SAFETY: a fresh mbuf of `MLEN` bytes; the tests' paths fit.
        unsafe {
            *mtod::<u8>(m) = len as u8;
            *mtod::<u8>(m).add(1) = AF_UNIX;
            ptr::copy_nonoverlapping(path.as_ptr(), mtod::<u8>(m).add(2), path.len());
        }
        m.m_len().set(len as u32);
        m
    }

    #[test]
    fn stream_pair_moves_bytes_both_ways_and_shuts_down() {
        let (_g, p) = setup();
        let (a, b) = pair(SOCK_STREAM);
        assert!(a.has_state(SS_ISCONNECTED) && b.has_state(SS_ISCONNECTED));
        let mut buf = [0u8; 16];

        assert_eq!(send(a, b"hello", 0), Ok(5));
        // The sender's buffer mirrors the receiver's, for back pressure.
        assert_eq!(b.so_rcv.sb_cc.get(), 5);
        assert_eq!(a.so_snd.sb_cc.get(), 5);
        assert_eq!(recv(b, &mut buf[..2], 0), Ok((2, 0)));
        assert_eq!(&buf[..2], b"he");
        assert_eq!(a.so_snd.sb_cc.get(), 3);
        assert_eq!(recv(b, &mut buf, 0), Ok((3, 0)));
        assert_eq!(&buf[..3], b"llo");
        assert_eq!(a.so_snd.sb_cc.get(), 0);
        assert_eq!(recv(b, &mut buf, MSG_DONTWAIT), Err(Errno::EWOULDBLOCK));

        assert_eq!(send(b, b"world", 0), Ok(5));
        assert_eq!(recv(a, &mut buf, 0), Ok((5, 0)));
        assert_eq!(&buf[..5], b"world");

        // socketpair(2) records the creator as both peers.
        let m = m_get(M_WAIT, MT_SOOPTS).expect("an mbuf");
        assert_eq!(sogetopt(a, SOL_SOCKET, SO_PEERCRED, m), Ok(()));
        // SAFETY: sogetopt stored the credentials.
        let cred = unsafe { mtod::<Sockpeercred>(m).read_unaligned() };
        assert_eq!(cred.uid, p.ucred().cr_uid.get());
        assert_eq!(sogetopt(b, SOL_SOCKET, SO_TYPE, m), Ok(()));
        // SAFETY: an int.
        assert_eq!(unsafe { mtod::<i32>(m).read_unaligned() }, SOCK_STREAM);
        m_freem(Some(m));

        // Shut down a's sending side: b reads EOF, a cannot send, b still can.
        assert_eq!(soshutdown(a, SHUT_WR), Ok(()));
        assert!(b.so_rcv.has_state(SS_CANTRCVMORE));
        assert_eq!(recv(b, &mut buf, 0), Ok((0, 0)));
        assert_eq!(send(a, b"x", MSG_NOSIGNAL), Err(Errno::EPIPE));
        assert_eq!(send(b, b"!", 0), Ok(1));
        assert_eq!(recv(a, &mut buf, 0), Ok((1, 0)));

        // Closing a disconnects b.
        assert_eq!(soclose(a, 0), Ok(()));
        assert!(!b.has_state(SS_ISCONNECTED));
        assert_eq!(send(b, b"?", MSG_NOSIGNAL), Err(Errno::EPIPE));
        assert_eq!(recv(b, &mut buf, 0), Ok((0, 0)));
        assert_eq!(soclose(b, 0), Ok(()));
        assert!(UNP_HEAD.0.first().is_none());
        teardown();
    }

    #[test]
    fn datagrams_keep_their_boundaries_and_sender() {
        let (_g, _p) = setup();
        let (a, b) = pair(SOCK_DGRAM);
        let mut buf = [0u8; 16];

        assert_eq!(send(a, b"first", 0), Ok(5));
        assert_eq!(send(a, b"second", 0), Ok(6));

        // One record per read, with the sender's address (unbound: sun_noname).
        let mut from = None;
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: 16,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let mut flags = 0;
        assert_eq!(
            soreceive(
                b,
                Some(&mut from),
                &mut uio,
                None,
                None,
                Some(&mut flags),
                0
            ),
            Ok(())
        );
        assert_eq!(16 - uio.uio_resid, 5);
        assert_eq!(&buf[..5], b"first");
        let from = from.expect("an address");
        assert_eq!(from.m_len().get() as usize, SUN_NONAME_LEN);
        // SAFETY: a socket address.
        assert_eq!(
            unsafe { mtod::<Sockaddr>(from).read_unaligned() },
            SUN_NONAME
        );
        m_freem(Some(from));

        // A short read truncates the datagram and drops the rest of it.
        assert_eq!(recv(b, &mut buf[..3], 0), Ok((3, MSG_TRUNC)));
        assert_eq!(&buf[..3], b"sec");
        assert_eq!(recv(b, &mut buf, MSG_DONTWAIT), Err(Errno::EWOULDBLOCK));

        // The pair is connected both ways.
        assert_eq!(send(b, b"back", 0), Ok(4));
        assert_eq!(recv(a, &mut buf, 0), Ok((4, 0)));
        assert_eq!(soclose(a, 0), Ok(()));
        assert_eq!(soclose(b, 0), Ok(()));
        teardown();
    }

    #[test]
    fn seqpacket_keeps_records() {
        let (_g, _p) = setup();
        let (a, b) = pair(SOCK_SEQPACKET);
        let mut buf = [0u8; 16];

        assert_eq!(send(a, b"ab", 0), Ok(2));
        assert_eq!(send(a, b"cd", 0), Ok(2));
        assert_eq!(recv(b, &mut buf, 0), Ok((2, 0)));
        assert_eq!(&buf[..2], b"ab");
        assert_eq!(recv(b, &mut buf, 0), Ok((2, 0)));
        assert_eq!(&buf[..2], b"cd");
        assert_eq!(soclose(a, 0), Ok(()));
        assert_eq!(soclose(b, 0), Ok(()));
        teardown();
    }

    /// Connects `client` to the listener `l` as `unp_connect` does once `namei` found it: a new
    /// socket from `sonewconn`, connected with `unp_connect2`.
    fn connect_to(client: &'static Socket, l: &'static Socket) -> &'static Socket {
        solock(l);
        let so3 = sonewconn(l, 0, M_WAIT).expect("sonewconn");
        sounlock(l);
        sounlock(so3);
        solock_pair(client, so3);
        assert_eq!(unp_connect2(client, so3), Ok(()));
        sounlock_pair(client, so3);
        so3
    }

    #[test]
    fn the_accept_queue() {
        let (_g, _p) = setup();
        let l = socreate(i32::from(AF_UNIX), SOCK_STREAM, 0).expect("socreate");

        // Only a bound socket listens.
        solock_shared(l);
        assert_eq!(crate::kern::uipc_socket::solisten(l, 5), Err(Errno::EINVAL));
        sounlock_shared(l);
        l.so_options.set(l.so_options.get() | SO_ACCEPTCONN);
        l.so_qlimit.set(5);

        let client = socreate(i32::from(AF_UNIX), SOCK_STREAM, 0).expect("socreate");
        let so3 = connect_to(client, l);
        // soisconnected moved the new socket from the partial queue to the complete one.
        assert_eq!((l.so_q0len.get(), l.so_qlen.get()), (0, 1));
        assert!(so3.has_state(SS_NOFDREF | SS_ISCONNECTED));

        // accept(2): off the queue, a file reference, the peer's name.
        solock_shared(l);
        let first = l.so_q.first().expect("a connection");
        assert!(ptr::eq(first, so3));
        solock_nonet(so3);
        assert!(soqremque(so3, 1));
        sounlock_nonet(l);
        let nam = m_get(M_WAIT, MT_SONAME).expect("an mbuf");
        assert_eq!(soaccept(so3, nam), Ok(()));
        sounlock_shared(so3);
        assert_eq!(nam.m_len().get() as usize, SUN_NONAME_LEN);
        m_freem(Some(nam));
        assert!(!so3.has_state(SS_NOFDREF));

        let mut buf = [0u8; 8];
        assert_eq!(send(client, b"ping", 0), Ok(4));
        assert_eq!(recv(so3, &mut buf, 0), Ok((4, 0)));
        assert_eq!(send(so3, b"pong", 0), Ok(4));
        assert_eq!(recv(client, &mut buf, 0), Ok((4, 0)));
        assert_eq!(&buf[..4], b"pong");

        // A connection still on the queue is aborted with its listener.
        let client2 = socreate(i32::from(AF_UNIX), SOCK_STREAM, 0).expect("socreate");
        let _pending = connect_to(client2, l);
        assert_eq!(l.so_qlen.get(), 1);
        assert_eq!(soclose(l, 0), Ok(()));
        assert!(!client2.has_state(SS_ISCONNECTED));
        assert_eq!(recv(client2, &mut buf, 0), Ok((0, 0)));

        for so in [client, so3, client2] {
            assert_eq!(soclose(so, 0), Ok(()));
        }
        assert!(UNP_HEAD.0.first().is_none());
        teardown();
    }

    #[test]
    fn bind_and_connect_go_through_namei() {
        let (_g, p) = setup();
        let _mp = crate::kern::vfs_subr::tests::testfs::mount_root(p);
        let so = socreate(i32::from(AF_UNIX), SOCK_STREAM, 0).expect("socreate");

        let bind = |path: &[u8]| {
            let nam = sun(path);
            solock_shared(so);
            let error = sobind(so, nam, p);
            sounlock_shared(so);
            m_freem(Some(nam));
            error
        };
        let connect = |path: &[u8]| {
            let nam = sun(path);
            solock(so);
            let error = soconnect(so, nam);
            sounlock(so);
            m_freem(Some(nam));
            error
        };

        // An existing name is in use; unp_nam2sun adds the missing NUL.
        assert_eq!(bind(b"/a/b"), Err(Errno::EADDRINUSE));
        assert_eq!(connect(b"/a/b"), Err(Errno::ENOTSOCK));
        assert_eq!(connect(b"/nonexistent"), Err(Errno::ENOENT));
        // A path that fills sun_path leaves no room for the NUL.
        assert_eq!(bind(&[b'x'; SUN_PATH_LEN]), Err(Errno::EINVAL));
        let inet = sun(b"/a/b");
        // SAFETY: the family byte of the address.
        unsafe { *mtod::<u8>(inet).add(1) = 2 };
        assert_eq!(unp_nam2sun(inet), Err(Errno::EAFNOSUPPORT));
        m_freem(Some(inet));
        assert!(sotounpcb(so).is_some_and(|unp| unp.unp_vnode.get().is_none()));

        assert_eq!(soclose(so, 0), Ok(()));
        teardown();
    }

    /// A system call's argument block from its arguments.
    fn args(a: &[usize]) -> SysArgs {
        let mut v: SysArgs = [0; 6];
        for (slot, &x) in v.iter_mut().zip(a) {
            *slot = x as Register;
        }
        v
    }

    /// `socketpair(AF_UNIX, SOCK_STREAM, 0, sv)`.
    pub(crate) fn socketpair(p: &Proc) -> [i32; 2] {
        let mut sv = [-1i32; 2];
        let mut retval = [0; 2];
        let v = args(&[
            usize::from(AF_UNIX),
            SOCK_STREAM as usize,
            0,
            sv.as_mut_ptr() as usize,
        ]);
        assert_eq!(sys_socketpair(p, &v, &mut retval), Ok(()));
        sv
    }

    /// `sendmsg(s, msg, 0)` of one byte with `fd` in an `SCM_RIGHTS` message.
    fn send_fd(p: &Proc, s: i32, fd: i32) {
        let byte = [b'x'];
        let mut iov = [Iovec {
            iov_base: byte.as_ptr().cast_mut().cast(),
            iov_len: 1,
        }];
        let mut cbuf = vec![0u8; cmsg_space(4)];
        let cm = Cmsghdr {
            cmsg_len: cmsg_len(4) as Socklen,
            cmsg_level: SOL_SOCKET,
            cmsg_type: SCM_RIGHTS,
        };
        // SAFETY: the buffer holds a header and an int.
        unsafe {
            cbuf.as_mut_ptr().cast::<Cmsghdr>().write_unaligned(cm);
            cbuf.as_mut_ptr().add(16).cast::<i32>().write_unaligned(fd);
        }
        let msg = Msghdr {
            msg_name: ptr::null_mut(),
            msg_namelen: 0,
            msg_iov: iov.as_mut_ptr().cast(),
            msg_iovlen: 1,
            msg_control: cbuf.as_mut_ptr().cast(),
            msg_controllen: cbuf.len() as Socklen,
            msg_flags: 0,
        };
        let mut retval = [0; 2];
        let v = args(&[s as usize, ptr::from_ref(&msg) as usize, 0]);
        assert_eq!(sys_sendmsg(p, &v, &mut retval), Ok(()));
        assert_eq!(retval[0], 1);
    }

    /// `close(fd)`.
    pub(crate) fn close(p: &Proc, fd: i32) {
        let mut retval = [0; 2];
        assert_eq!(sys_close(p, &args(&[fd as usize]), &mut retval), Ok(()));
    }

    #[test]
    fn descriptors_pass_through_scm_rights() {
        let (_g, p) = setup();
        let [a0, a1] = socketpair(p);
        let [b0, b1] = socketpair(p);
        let fp_b0 = crate::kern::kern_descrip::fd_getfile(p.fd(), b0).expect("b0");

        send_fd(p, a0, b0);
        assert_eq!(UNP_RIGHTS.load(Ordering::Relaxed), 1);

        let mut byte = [0u8; 1];
        let mut iov = [Iovec {
            iov_base: byte.as_mut_ptr().cast(),
            iov_len: 1,
        }];
        let mut cbuf = vec![0u8; 64];
        let mut msg = Msghdr {
            msg_name: ptr::null_mut(),
            msg_namelen: 0,
            msg_iov: iov.as_mut_ptr().cast(),
            msg_iovlen: 1,
            msg_control: cbuf.as_mut_ptr().cast(),
            msg_controllen: cbuf.len() as Socklen,
            msg_flags: 0,
        };
        let mut retval = [0; 2];
        let v = args(&[a1 as usize, ptr::from_mut(&mut msg) as usize, 0]);
        assert_eq!(sys_recvmsg(p, &v, &mut retval), Ok(()));
        assert_eq!(retval[0], 1);
        assert_eq!(byte, [b'x']);
        assert_eq!(msg.msg_controllen as usize, cmsg_len(4));
        // SAFETY: recvmsg copied a header and an int out.
        let (cm, newfd) = unsafe {
            (
                cbuf.as_ptr().cast::<Cmsghdr>().read_unaligned(),
                cbuf.as_ptr().add(16).cast::<i32>().read_unaligned(),
            )
        };
        assert_eq!((cm.cmsg_level, cm.cmsg_type), (SOL_SOCKET, SCM_RIGHTS));
        assert_eq!(UNP_RIGHTS.load(Ordering::Relaxed), 0);
        let fp_new = crate::kern::kern_descrip::fd_getfile(p.fd(), newfd).expect("the new fd");
        assert!(ptr::eq(fp_new, fp_b0));
        let _ = frele(fp_new, p);
        let _ = frele(fp_b0, p);

        // The new descriptor is b0: what it writes, b1 reads.
        let mut retval = [0; 2];
        let w = args(&[newfd as usize, b"via".as_ptr() as usize, 3]);
        assert_eq!(sys_write(p, &w, &mut retval), Ok(()));
        let mut got = [0u8; 8];
        let r = args(&[b1 as usize, got.as_mut_ptr() as usize, got.len()]);
        assert_eq!(sys_read(p, &r, &mut retval), Ok(()));
        assert_eq!(&got[..retval[0] as usize], b"via");

        for fd in [a0, a1, b0, b1, newfd] {
            close(p, fd);
        }
        assert!(UNP_HEAD.0.first().is_none());
        teardown();
    }

    #[test]
    fn rights_never_received_are_collected() {
        let (_g, p) = setup();
        let [a0, a1] = socketpair(p);
        let [b0, b1] = socketpair(p);
        let fp_b0 = crate::kern::kern_descrip::fd_getfile(p.fd(), b0).expect("b0");
        let count = |fp: &File| fp.f_count.load(Ordering::SeqCst);
        // The table's reference and ours.
        assert_eq!(count(fp_b0), 2);

        send_fd(p, a0, b0);
        assert_eq!(count(fp_b0), 3);

        // The receiver goes away with the message in its buffer: the file is handed to the
        // collector, which closes the message's reference.
        close(p, a1);
        assert!(UNP_DEFERRED.0.first().is_some());
        unp_gc(ptr::null_mut());
        assert!(UNP_DEFERRED.0.first().is_none());
        assert_eq!(count(fp_b0), 2);
        assert_eq!(UNP_RIGHTS.load(Ordering::Relaxed), 0);
        let _ = frele(fp_b0, p);

        for fd in [a0, b0, b1] {
            close(p, fd);
        }
        assert!(UNP_HEAD.0.first().is_none());
        teardown();
    }

    #[test]
    fn a_cycle_of_sockets_is_collected() {
        let (_g, p) = setup();
        let [a0, a1] = socketpair(p);

        // a1's own descriptor goes into a1's receive buffer; then both descriptors close. a1
        // is now referenced only by the message in its own buffer.
        send_fd(p, a0, a1);
        close(p, a0);
        close(p, a1);
        let live: Vec<_> = UNP_HEAD.0.iter().collect();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].unp_msgcount.get(), 1);

        // The first pass finds the cycle and discards the buffer; the second closes the file.
        unp_gc(ptr::null_mut());
        assert!(UNP_DEFERRED.0.first().is_some());
        unp_gc(ptr::null_mut());
        assert!(UNP_HEAD.0.first().is_none());
        assert_eq!(UNP_RIGHTS.load(Ordering::Relaxed), 0);
        teardown();
    }
}
/* </TESTS> */
