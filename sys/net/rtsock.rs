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
 * Copyright (C) 1995, 1996, 1997, and 1998 WIDE Project.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the project nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE PROJECT AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE PROJECT OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */

/*
 * Copyright (c) 1988, 1991, 1993
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
 *	@(#)rtsock.c	8.6 (Berkeley) 2/11/95
 */
/* </LICENSES> */

/* <CODE> */
//! The routing socket: the `routedomain` and its one protocol, the messages user processes
//! send to change and query the routing tables (`route_output`), the messages the kernel
//! announces to every routing socket (`rtm_send`, `rtm_miss`, `rtm_ifchg`, `rtm_addr`, ...),
//! and the `net.route` sysctl (route dumps, interface lists, `rtstat`).
//!
//! Upstream: sys/net/rtsock.c @ 3ce1f3f79392
//!
//! A routing socket is a raw socket of `PF_ROUTE` with a `struct rtpcb` as its control
//! block, kept on `rtptable`. A message written to it is a `struct rt_msghdr` followed by the
//! socket addresses its `rtm_addrs` names (`rtm_xaddrs`); `route_output` carries out the
//! request (`rtm_output`: `RTM_ADD`, `RTM_DELETE`, `RTM_CHANGE`, `RTM_GET`; also
//! `RTM_PROPOSAL` and `RTM_SOURCE`) and hands the answer to every listener (`route_input`),
//! itself included unless it cleared `SO_USELOOPBACK`. The announcements build their message
//! in an mbuf (`rtm_msg1`); the sysctl builds its messages in a walk buffer (`rtm_msg2`) and
//! copies them out one by one.
//!
//! ## Deviations
//! - `struct rtpcb`'s members the C changes under the socket lock are `Cell`s; `rop_socket`
//!   is `&'static Socket` (the socket outlives its control block, freed by `route_detach`).
//!   `rtptable.rtp_count` is an `AtomicU32`: it changes under `rtp_lk` but the announcements
//!   read it without the lock, as the C does.
//! - `route_input` reads the header fields it filters on (`rtm_type`, `rtm_tableid`,
//!   `rtm_priority`, `rtm_flags`) with `m_copydata` instead of through `mtod`: the answer to
//!   a user's message is copied back into the user's mbuf chain, which may hold the header in
//!   more than one mbuf. Its `struct socket *so0` is an `Option`.
//! - The routing messages are byte buffers: `rtm_msg2`'s `caddr_t cp` is `Option<&mut [u8]>`;
//!   `route_output` and `rtm_report` keep their message in a `malloc(M_RTABLE)` buffer
//!   (`RtmBuf`, freed with its size when dropped, always `M_ZERO` so every byte is
//!   initialised) and edit a copy of its `struct rt_msghdr` that is written back before the
//!   message goes out. `struct walkarg`'s `w_where` is a user address (0 for NULL) and its
//!   `w_tmem` an `Option<NonNull<u8>>`.
//! - The functions that read the raw socket addresses of a `struct rt_addrinfo`
//!   (`rtm_msg1`, `rtm_msg2`, `rtm_miss`, `rtm_xaddrs`, `rtm_output`, `rtm_getifa`,
//!   `ifa_ifwithroute`, `route_arp_conflict`, `rt_setsource`, `rtm_validate_proposal`) are
//!   `unsafe fn`s whose contract is that every non-NULL address is readable for its `sa_len`
//!   (`docs/C_TO_RUST.md`).
//! - `rtm_validate_proposal` returns `Err(EINVAL)` for the C's -1. `sizeof(struct
//!   sockaddr_in6)` is the local `SIZEOF_SOCKADDR_IN6`; `rtm_validate_proposal` checks it
//!   outside `INET6`, as the C does.
//! - `ifp->if_rtrequest` is an `Option`, called when set (`if_attach` always sets it).
//! - Not configured: `BFD` (`rtm_bfd`, `RTM_BFD`'s header, `RTAX_BFD`), `MPLS`
//!   (`RTAX_SRC` labels, `rt_mpls_set`/`rt_mpls_clear`); comments at their
//!   sites. `INET6`'s `AF_INET6` cases are under the `inet6` feature. `SMALL_KERNEL` is
//!   not defined, so `NET_RT_STATS` and `NET_RT_TABLE` are answered. The C's
//!   `KERNEL_LOCK()` sites are all `BFD` code.
//! - kqueue: `rtm_sendup` and `rtm_senddesync` wake the reader with `sorwakeup`, whose knote
//!   (`KNOTE(&so->so_rcv.sb_klist, 0)`) is the socket layer's, not ported yet.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_prot::suser;
use crate::kern::kern_rwlock::{
    rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write, rw_init,
};
use crate::kern::kern_synch::refcnt_read;
use crate::kern::kern_sysctl::sysctl_rdstruct;
use crate::kern::kern_tc::{gettime, getuptime};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del_barrier, timeout_set_flags};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{
    m_adj, m_copyback, m_copydata, m_copym, m_free, m_freem, m_gethdr, m_pullup,
};
use crate::kern::uipc_socket::sorwakeup;
use crate::kern::uipc_socket2::{
    sbappendaddr, soassertlocked, socantsendmore, soisconnected, soisdisconnected, solock,
    soreserve, sounlock,
};
use crate::machine::copy::copyout;
use crate::machine::cpu::curproc;
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_::{
    IF_TMPLIST_LOCK, IFF_POINTOPOINT, IFNAMSIZ, IFNETLIST, IfAnnouncemsghdr, IfIeee80211Data,
    IfIeee80211Msghdr, IfMsghdr, IfNameindexMsg, IfaMsghdr, if_get, if_getdata,
    if_group_routechange, if_put, if_ref, ifa_ifwithaddr, ifa_ifwithdstaddr, ifaof_ifpforaddr,
};
use crate::net::if_dl::{satosdl_const, sdltosa};
use crate::net::if_var::{IfTmplist, Ifaddr, Ifnet};
use crate::net::route::{
    ROUTE_FLAGFILTER, ROUTE_MSGFILTER, ROUTE_PRIOFILTER, ROUTE_TABLEFILTER, RT_RESOLVE, RTA_DNS,
    RTA_IFA, RTA_NETMASK, RTA_SEARCH, RTA_STATIC, RTABLE_ANY, RTAX_BRD, RTAX_DNS, RTAX_DST,
    RTAX_GATEWAY, RTAX_GENMASK, RTAX_IFA, RTAX_IFP, RTAX_LABEL, RTAX_MAX, RTAX_NETMASK,
    RTAX_SEARCH, RTAX_SRC, RTAX_STATIC, RTCOUNTERS, RTF_ANNOUNCE, RTF_BROADCAST, RTF_CACHED,
    RTF_CLONED, RTF_CLONING, RTF_DONE, RTF_FMASK, RTF_GATEWAY, RTF_HOST, RTF_LLINFO, RTF_LOCAL,
    RTF_MPATH, RTF_MPLS, RTF_STATIC, RTLABEL_LEN, RTM_80211INFO, RTM_ADD, RTM_BFD, RTM_CHANGE,
    RTM_DELADDR, RTM_DELETE, RTM_DESYNC, RTM_GET, RTM_IFANNOUNCE, RTM_IFINFO, RTM_INVALIDATE,
    RTM_MAXSIZE, RTM_NEWADDR, RTM_PROPOSAL, RTM_RESOLVE, RTM_SOURCE, RTM_VERSION, RTP_ANY,
    RTP_DEFAULT, RTP_LOCAL, RTP_MASK, RTP_MAX, RTP_PROPOSAL_SOLICIT, RTSEARCH_LEN, RTSTATIC_LEN,
    RTV_EXPIRE, RTV_MTU, RtAddrinfo, RtKmetrics, RtMetrics, RtMsghdr, RtTableinfo, Rtentry,
    RtstatCounters, SockaddrRtdns, SockaddrRtlabel, SockaddrRtsearch, SockaddrRtstatic, ifafree,
    ifaref, route_init, rt_if_linkstate_change, rt_plen2mask, rt_setgate, rt_timer_get_expire,
    rtalloc, rtfree, rtlabel_id2sa, rtlabel_name2id, rtlabel_unref, rtrequest, rtrequest_delete,
};
use crate::net::rtable::{
    rt_key, rt_plen, rtable_add, rtable_exists, rtable_getsource, rtable_l2, rtable_lookup,
    rtable_match, rtable_read, rtable_satoplen, rtable_setsource, rtable_walk,
};
use crate::netinet::in_::{INADDR_ANY, InAddr, SockaddrIn, satosin_const};
use crate::queue_adapter;
use crate::sys::domain::Domain;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_RTABLE, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{
    M_COPYALL, M_DONTWAIT, M_EXT, M_PKTHDR, M_WAIT, MCLBYTES, MHLEN, MSIZE, MT_DATA, Mbuf, mclget,
    mtod,
};
use crate::sys::param::{PAGE_SIZE, align};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::protosw::{
    PR_ADDR, PR_ATOMIC, PR_WANTRCVD, PRCO_GETOPT, PRCO_SETOPT, PrUsrreqs, Protosw,
};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::socket::{
    AF_INET, AF_LINK, AF_MAX, AF_ROUTE, AF_UNSPEC, NET_RT_DUMP, NET_RT_FLAGS, NET_RT_IFLIST,
    NET_RT_IFNAMES, NET_RT_SOURCE, NET_RT_STATS, NET_RT_TABLE, PF_KEY, PF_ROUTE, SO_USELOOPBACK,
    SOCK_RAW, Sockaddr, SockaddrStorage,
};
use crate::sys::socketvar::{SS_CANTRCVMORE, SS_ISCONNECTED, SS_NOFDREF, Socket, sbspace_locked};
use crate::sys::sysctl::SysctlPlain;
use crate::sys::systm::{
    net_assert_locked, net_lock, net_lock_shared, net_unlock, net_unlock_shared,
};
use crate::sys::timeout::{KCLOCK_NONE, TIMEOUT_MPSAFE, TIMEOUT_PROC, Timeout};
use crate::sys::types::SaFamily;

/// `ROUTESNDQ`.
pub const ROUTESNDQ: u64 = 8192;
/// `ROUTERCVQ`.
pub const ROUTERCVQ: u64 = 8192;

// These flags and timeout are used for indicating to userland (via a RTM_DESYNC msg) when the
// route socket has overflowed and messages have been lost.

/// Route socket out of memory.
pub const ROUTECB_FLAG_DESYNC: u32 = 0x1;
/// Wait until socket is empty before queueing more packets.
pub const ROUTECB_FLAG_FLUSH: u32 = 0x2;

/// `ROUTE_DESYNC_RESEND_TIMEOUT`: in ms.
pub const ROUTE_DESYNC_RESEND_TIMEOUT: u64 = 200;

/// `sizeof(struct sockaddr_in6)`.
const SIZEOF_SOCKADDR_IN6: usize = size_of::<crate::netinet6::in6::SockaddrIn6>();

/// `sizeof(socklen_t)`: what `rtm_xaddrs` wants left before it reads an address.
const SIZEOF_SOCKLEN_T: usize = size_of::<u32>();

/// `struct walkarg`: the state of a `net.route` sysctl walk.
#[derive(Default)]
struct Walkarg {
    /// `w_op`: the `NET_RT_*` operation.
    w_op: i32,
    /// `w_arg`: its argument (flags, priority, interface index or table id).
    w_arg: i32,
    /// `w_tmemsize`: the size of `w_tmem`.
    w_tmemsize: usize,
    /// `w_given`: the size of the user's buffer.
    w_given: usize,
    /// `w_needed`: the bytes the answer takes.
    w_needed: usize,
    /// `w_where`: where the next message goes in the user's buffer (0 for NULL).
    w_where: usize,
    /// `w_tmem`: the buffer one message is built in (`malloc(M_RTABLE)`).
    w_tmem: Option<NonNull<u8>>,
}

/// `struct rtpcb`: a routing socket's control block. The locks are the C's: \[I\] immutable
/// after creation, \[s\] solock.
pub struct Rtpcb {
    /// \[I\] `rop_socket`.
    pub rop_socket: &'static Socket,
    /// `rop_list`.
    pub rop_list: TailqEntry<Rtpcb>,
    /// `rop_timeout`: resends `RTM_DESYNC`.
    pub rop_timeout: Timeout,
    /// \[s\] `rop_msgfilter`: the message types wanted (`ROUTE_FILTER` bits), 0 for all.
    pub rop_msgfilter: Cell<u32>,
    /// \[s\] `rop_flagfilter`: messages with any of these `RTF_*` flags are not wanted.
    pub rop_flagfilter: Cell<u32>,
    /// \[s\] `rop_flags`: `ROUTECB_FLAG_*`.
    pub rop_flags: Cell<u32>,
    /// \[s\] `rop_rtableid`: the table listened to, `RTABLE_ANY` for all.
    pub rop_rtableid: Cell<u32>,
    /// \[I\] `rop_proto`: the address family listened to, `AF_UNSPEC` for all.
    pub rop_proto: u16,
    /// \[s\] `rop_priority`: the lowest priority wanted, 0 for all.
    pub rop_priority: Cell<u8>,
}

impl Rtpcb {
    /// The control block of `so` for protocol `proto`, as `route_attach` fills it in.
    fn new(so: &'static Socket, proto: u16) -> Self {
        Self {
            rop_socket: so,
            rop_list: TailqEntry::new(),
            rop_timeout: Timeout::zeroed(),
            rop_msgfilter: Cell::new(0),
            rop_flagfilter: Cell::new(0),
            rop_flags: Cell::new(0),
            rop_rtableid: Cell::new(0),
            rop_proto: proto,
            rop_priority: Cell::new(0),
        }
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(, rtpcb) rtp_list`.
    pub RtpcbList: Rtpcb, rop_list => TailqEntry<Rtpcb>
);

/// `struct rtptable`: the routing sockets. Lock order: `rtptable.rtp_lk` -> solock.
pub struct Rtptable {
    /// `rtp_list`.
    pub rtp_list: TailqHead<RtpcbList>,
    /// `rtp_lk`.
    pub rtp_lk: Rwlock,
    /// `rtp_count`.
    pub rtp_count: AtomicU32,
}

// SAFETY: the list changes under `rtp_lk`, a control block's members under its socket's
// lock; the count is atomic.
unsafe impl Sync for Rtptable {}

/// A routing message in a `malloc(len, M_RTABLE, M_ZERO)` buffer: the C's `struct rt_msghdr
/// *rtm` with its `len`, freed with that size when dropped.
struct RtmBuf {
    /// The buffer, aligned for any type (a `malloc` bucket).
    ptr: NonNull<u8>,
    /// Its size.
    len: usize,
}

impl RtmBuf {
    /// `malloc(len, M_RTABLE, flags | M_ZERO)`; a failed `M_WAITOK` allocation panics.
    fn alloc(len: usize, flags: i32) -> Self {
        match malloc(len, M_RTABLE, flags | M_ZERO) {
            Some(ptr) => Self { ptr, len },
            None => panic(format_args!("rtsock: malloc({}, M_RTABLE)", len)),
        }
    }

    /// The message's bytes.
    fn bytes(&self) -> &[u8] {
        // SAFETY: `len` bytes allocated and zeroed by `alloc`, written only as bytes since.
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    /// The message's bytes, to write.
    fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as in `bytes`; `&mut self` makes the slice the only access.
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    /// A copy of the `struct rt_msghdr` the message starts with.
    fn hdr(&self) -> RtMsghdr {
        kassert!(self.len >= size_of::<RtMsghdr>());
        // SAFETY: every message here is at least a `struct rt_msghdr` long (`route_output`
        // checks the user's, `rtm_report` builds one); its integers take any bit pattern.
        unsafe { self.ptr.as_ptr().cast::<RtMsghdr>().read_unaligned() }
    }

    /// Writes the message's `struct rt_msghdr`.
    fn set_hdr(&mut self, rtm: &RtMsghdr) {
        kassert!(self.len >= size_of::<RtMsghdr>());
        // SAFETY: as in `hdr`; `struct rt_msghdr` has no padding, so the bytes stay
        // initialised.
        unsafe { self.ptr.as_ptr().cast::<RtMsghdr>().write_unaligned(*rtm) };
    }
}

/// The header fields of a routing message `route_input` filters on.
struct RtmPeek {
    /// `rtm_type`.
    rtm_type: u8,
    /// `rtm_tableid` (`ifm_tableid`, `ifam_tableid` at the same offset).
    rtm_tableid: u16,
    /// `rtm_priority`.
    rtm_priority: u8,
    /// `rtm_flags`.
    rtm_flags: i32,
}

/// `route_src`: the source address of the messages.
pub static ROUTE_SRC: Sockaddr = Sockaddr {
    sa_len: 2,
    sa_family: PF_ROUTE,
    sa_data: [0; 14],
};

/// `rtpcb_pool`.
static RTPCB_POOL: Pool = Pool::new();
/// `rtptable`.
pub static RTPTABLE: Rtptable = Rtptable {
    rtp_list: TailqHead::new(),
    rtp_lk: Rwlock::new("rtsock"),
    rtp_count: AtomicU32::new(0),
};

/// `route_usrreqs`: the user requests of routing sockets.
pub static ROUTE_USRREQS: PrUsrreqs = PrUsrreqs {
    pru_attach: Some(route_attach),
    pru_detach: Some(route_detach),
    pru_disconnect: Some(route_disconnect),
    pru_shutdown: Some(route_shutdown),
    pru_rcvd: Some(route_rcvd),
    pru_send: Some(route_send),
    pru_sockaddr: Some(route_sockaddr),
    pru_peeraddr: Some(route_peeraddr),
    ..PrUsrreqs::NONE
};

/// `routesw[]`: the protocols of the route domain.
pub static ROUTESW: [Protosw; 1] = [Protosw {
    pr_type: SOCK_RAW as i16,
    pr_flags: PR_ATOMIC | PR_ADDR | PR_WANTRCVD,
    pr_ctloutput: Some(route_ctloutput),
    pr_usrreqs: Some(&ROUTE_USRREQS),
    pr_init: Some(route_prinit),
    pr_sysctl: Some(sysctl_rtable),
    ..Protosw::new(&ROUTEDOMAIN)
}];

/// `routedomain`.
pub static ROUTEDOMAIN: Domain = Domain {
    dom_family: PF_ROUTE as i32,
    dom_name: b"route",
    dom_init: Some(route_init),
    dom_externalize: None,
    dom_dispose: None,
    dom_protosw: &ROUTESW,
    dom_sasize: 0,
    dom_rtoffset: 0,
    dom_maxplen: 0,
};

impl Drop for RtmBuf {
    fn drop(&mut self) {
        free(self.ptr, M_RTABLE, self.len);
    }
}

/// `ROUNDUP(a)`: `a` rounded up to a multiple of `sizeof(long)`, `sizeof(long)` for 0.
const fn roundup_long(a: usize) -> usize {
    if a > 0 {
        1 + ((a - 1) | (size_of::<u64>() - 1))
    } else {
        size_of::<u64>()
    }
}

/// `sotortpcb(so)`: the control block of a routing socket, `None` once detached.
fn sotortpcb(so: &Socket) -> Option<&'static Rtpcb> {
    // SAFETY: a routing socket's `so_pcb` is NULL or the `rtpcb_pool` item `route_attach`
    // wrote, which stays allocated until `route_detach` clears the pointer.
    unsafe { so.so_pcb.get().cast::<Rtpcb>().as_ref() }
}

/// `sotortpcb(so)` where the C dereferences it without a check.
fn rtpcb_of(so: &Socket) -> &'static Rtpcb {
    match sotortpcb(so) {
        Some(rop) => rop,
        None => panic(format_args!("routing socket {:p} without rtpcb", so)),
    }
}

/// `curproc`, which the requests and the sysctl run as.
fn curproc_or_panic(func: &str) -> &'static Proc {
    match curproc() {
        Some(p) => p,
        None => panic(format_args!("{}: no curproc", func)),
    }
}

/// The bytes of `route_src`.
fn route_src() -> &'static [u8] {
    // SAFETY: `struct sockaddr` is `#[repr(C)]` bytes without padding, and a static; its
    // `sa_len` (2) is within it.
    unsafe {
        slice::from_raw_parts(
            ptr::from_ref(&ROUTE_SRC).cast::<u8>(),
            usize::from(ROUTE_SRC.sa_len),
        )
    }
}

/// `ifp->if_rtrequest(ifp, req, rt)`.
fn if_rtrequest(ifp: &'static Ifnet, req: u8, rt: Option<&'static Rtentry>) {
    if let Some(rtrequest) = ifp.if_rtrequest.get() {
        rtrequest(ifp, i32::from(req), rt);
    }
}

/// The family of the socket address at `sa`, `AF_UNSPEC` for NULL.
///
/// # Safety
///
/// `sa` is NULL or points at a readable socket address.
unsafe fn sa_family(sa: *const Sockaddr) -> SaFamily {
    if sa.is_null() {
        AF_UNSPEC
    } else {
        // SAFETY: the caller's contract.
        unsafe { (*sa).sa_family }
    }
}

/// The `sa_len` of the socket address at `sa`.
///
/// # Safety
///
/// `sa` points at a readable socket address.
unsafe fn sa_len(sa: *const Sockaddr) -> usize {
    // SAFETY: the caller's contract.
    usize::from(unsafe { (*sa).sa_len })
}

/// `route_prinit`: the routing control block table.
pub fn route_prinit() {
    rw_init(&RTPTABLE.rtp_lk, "rtsock");
    RTPTABLE.rtp_list.init();
    pool_init(
        &RTPCB_POOL,
        size_of::<Rtpcb>(),
        0,
        IPL_SOFTNET,
        PR_WAITOK,
        "rtpcb",
        None,
    );
}

/// `route_attach(so, proto, wait)`: a control block for the new routing socket `so`,
/// listening to address family `proto` (0 for all) in the process's routing table.
pub fn route_attach(so: &'static Socket, proto: i32, wait: i32) -> Result<(), Errno> {
    soassertlocked(so);

    soreserve(so, ROUTESNDQ, ROUTERCVQ)?;
    // use the rawcb but allocate a rtpcb, this code does not care about the additional
    // fields and works directly on the raw socket.
    let Some(mem) = pool_get(
        &RTPCB_POOL,
        (if wait == M_WAIT { PR_WAITOK } else { PR_NOWAIT }) | PR_ZERO,
    ) else {
        return Err(Errno::ENOBUFS);
    };
    let raw = mem.cast::<Rtpcb>().as_ptr();
    // SAFETY: a fresh, suitably aligned `rtpcb_pool` item of `size_of::<Rtpcb>()` bytes,
    // written once before anything else sees it.
    unsafe { raw.write(Rtpcb::new(so, proto as u16)) };
    // SAFETY: as above; the item stays allocated until `route_detach` gives it back.
    let rop: &'static Rtpcb = unsafe { &*raw };
    so.so_pcb.set(raw.cast());
    // Init the timeout structure
    timeout_set_flags(
        &rop.rop_timeout,
        rtm_senddesync_timer,
        ptr::from_ref(so).cast_mut().cast(),
        KCLOCK_NONE,
        TIMEOUT_PROC | TIMEOUT_MPSAFE,
    );

    let p = curproc_or_panic("route_attach");
    rop.rop_rtableid
        .set(p.process().ps_rtableid.load(Ordering::Relaxed));

    soisconnected(so);
    so.so_options.set(so.so_options.get() | SO_USELOOPBACK);

    // Give up solock before taking rtp_lk for the lock ordering.
    sounlock(so);

    rw_enter_write(&RTPTABLE.rtp_lk);
    // SAFETY: `rtp_lk` is held; the control block is new and stays in place until
    // `route_detach` unlinks it.
    unsafe { RTPTABLE.rtp_list.insert_tail(rop) };
    RTPTABLE.rtp_count.fetch_add(1, Ordering::Relaxed);
    rw_exit_write(&RTPTABLE.rtp_lk);

    solock(so);

    Ok(())
}

/// `route_detach(so)`: unlinks and frees the control block of `so`.
pub fn route_detach(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);

    let Some(rop) = sotortpcb(so) else {
        return Err(Errno::EINVAL);
    };

    // Give up solock before taking rtp_lk for the lock ordering.
    sounlock(so);

    rw_enter_write(&RTPTABLE.rtp_lk);
    RTPTABLE.rtp_count.fetch_sub(1, Ordering::Relaxed);
    // SAFETY: `rtp_lk` is held; `route_attach` linked the control block.
    unsafe { RTPTABLE.rtp_list.remove(rop) };
    rw_exit_write(&RTPTABLE.rtp_lk);

    // wait for all references to drop
    timeout_del_barrier(&rop.rop_timeout);

    solock(so);

    so.so_pcb.set(ptr::null_mut());
    kassert!(!so.has_state(SS_NOFDREF));
    pool_put(&RTPCB_POOL, NonNull::from(rop).cast());

    Ok(())
}

/// `route_disconnect(so)`.
pub fn route_disconnect(so: &'static Socket) -> Result<(), Errno> {
    soisdisconnected(so);
    Ok(())
}

/// `route_shutdown(so)`.
pub fn route_shutdown(so: &'static Socket) -> Result<(), Errno> {
    socantsendmore(so);
    Ok(())
}

/// `route_rcvd(so)`: the reader took data; a flushed socket takes messages again once its
/// buffer is empty.
pub fn route_rcvd(so: &'static Socket) {
    let rop = rtpcb_of(so);

    soassertlocked(so);

    // If we are in a FLUSH state, check if the buffer is empty so that we can clear the flag.

    mtx_enter(&so.so_rcv.sb_mtx);
    if rop.rop_flags.get() & ROUTECB_FLAG_FLUSH != 0
        && sbspace_locked(&so.so_rcv) == so.so_rcv.sb_hiwat.get() as i64
    {
        rop.rop_flags.set(rop.rop_flags.get() & !ROUTECB_FLAG_FLUSH);
    }
    mtx_leave(&so.so_rcv.sb_mtx);
}

/// `route_send(so, m, nam, control)`: a message written to the socket.
pub fn route_send(
    so: &'static Socket,
    m: Option<&'static Mbuf>,
    nam: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let mut m = m;

    soassertlocked(so);

    let error = if control.is_some_and(|c| c.m_len().get() != 0) {
        Err(Errno::EOPNOTSUPP)
    } else if nam.is_some() {
        Err(Errno::EISCONN)
    } else {
        let error = route_output(m, so);
        m = None;
        error
    };

    m_freem(control);
    m_freem(m);

    error
}

/// `route_sockaddr(so, nam)`: a routing socket has no address.
pub fn route_sockaddr(_so: &'static Socket, _nam: &'static Mbuf) -> Result<(), Errno> {
    Err(Errno::EINVAL)
}

/// `route_peeraddr(so, nam)`: minimal support, just implement a fake peer address.
pub fn route_peeraddr(_so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    let src = route_src();
    // SAFETY: `nam` is an address mbuf of `MLEN` bytes, more than the two of `route_src`.
    unsafe { ptr::copy_nonoverlapping(src.as_ptr(), mtod::<u8>(nam), src.len()) };
    nam.m_len().set(src.len() as u32);
    Ok(())
}

/// `route_ctloutput(op, so, level, optname, m)`: the routing socket options
/// (`ROUTE_MSGFILTER`, `ROUTE_TABLEFILTER`, `ROUTE_PRIOFILTER`, `ROUTE_FLAGFILTER`), each an
/// `unsigned int`.
pub fn route_ctloutput(
    op: i32,
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let rop = rtpcb_of(so);
    let mut error = Ok(());

    if level != i32::from(AF_ROUTE) {
        return Err(Errno::EINVAL);
    }

    // The option's `unsigned int`, when `m` holds exactly one.
    let uint_of = |m: Option<&'static Mbuf>| -> Option<u32> {
        let m = m.filter(|m| m.m_len().get() as usize == size_of::<u32>())?;
        // SAFETY: the mbuf holds the four bytes (checked); maybe unaligned.
        Some(unsafe { mtod::<u32>(m).read_unaligned() })
    };

    match op {
        PRCO_SETOPT => match optname {
            ROUTE_MSGFILTER => match uint_of(m) {
                Some(v) => rop.rop_msgfilter.set(v),
                None => error = Err(Errno::EINVAL),
            },
            ROUTE_TABLEFILTER => match uint_of(m) {
                None => error = Err(Errno::EINVAL),
                Some(tid) => {
                    if tid != RTABLE_ANY && !rtable_exists(tid) {
                        error = Err(Errno::ENOENT);
                    } else {
                        rop.rop_rtableid.set(tid);
                    }
                }
            },
            ROUTE_PRIOFILTER => match uint_of(m) {
                None => error = Err(Errno::EINVAL),
                Some(prio) => {
                    if prio > u32::from(RTP_MAX) {
                        error = Err(Errno::EINVAL);
                    } else {
                        rop.rop_priority.set(prio as u8);
                    }
                }
            },
            ROUTE_FLAGFILTER => match uint_of(m) {
                Some(v) => rop.rop_flagfilter.set(v),
                None => error = Err(Errno::EINVAL),
            },
            _ => error = Err(Errno::ENOPROTOOPT),
        },
        PRCO_GETOPT => {
            let value = match optname {
                ROUTE_MSGFILTER => Some(rop.rop_msgfilter.get()),
                ROUTE_TABLEFILTER => Some(rop.rop_rtableid.get()),
                ROUTE_PRIOFILTER => Some(u32::from(rop.rop_priority.get())),
                ROUTE_FLAGFILTER => Some(rop.rop_flagfilter.get()),
                _ => None,
            };
            match (value, m) {
                (Some(v), Some(m)) => {
                    m.m_len().set(size_of::<u32>() as u32);
                    // SAFETY: an option mbuf holds `MLEN` bytes, more than four; maybe
                    // unaligned.
                    unsafe { mtod::<u32>(m).write_unaligned(v) };
                }
                (Some(_), None) => panic(format_args!("route_ctloutput: no option mbuf")),
                (None, _) => error = Err(Errno::ENOPROTOOPT),
            }
        }
        _ => {}
    }
    error
}

/// `rtm_senddesync_timer(xso)`: the timeout that retries `RTM_DESYNC`; `xso` is the socket.
pub fn rtm_senddesync_timer(xso: *mut c_void) {
    // SAFETY: `route_attach` armed the timeout with its socket, and `route_detach` waits for
    // it (`timeout_del_barrier`) before the control block, then the socket, go away.
    let so: &'static Socket = unsafe { &*xso.cast::<Socket>() };

    solock(so);
    rtm_senddesync(so);
    sounlock(so);
}

/// `rtm_senddesync(so)`: tells a socket that lost messages so with `RTM_DESYNC`, or retries
/// later.
pub fn rtm_senddesync(so: &'static Socket) {
    let rop = rtpcb_of(so);

    soassertlocked(so);

    // Dying socket is disconnected by upper layer and there is no reason to send packet.
    // Also we shouldn't reschedule timeout(9), otherwise timeout_del_barrier(9) can't help
    // us.
    if !so.has_state(SS_ISCONNECTED) || so.so_rcv.has_state(SS_CANTRCVMORE) {
        return;
    }

    // If we are in a DESYNC state, try to send a RTM_DESYNC packet
    if rop.rop_flags.get() & ROUTECB_FLAG_DESYNC == 0 {
        return;
    }

    // If we fail to alloc memory or if sbappendaddr() fails, re-add timeout and try again.
    // SAFETY: no addresses.
    if let Some(desync_mbuf) = unsafe { rtm_msg1(RTM_DESYNC, None) } {
        mtx_enter(&so.so_rcv.sb_mtx);
        let ret = sbappendaddr(&so.so_rcv, route_src(), Some(desync_mbuf), None);
        mtx_leave(&so.so_rcv.sb_mtx);

        if ret {
            rop.rop_flags
                .set(rop.rop_flags.get() & !ROUTECB_FLAG_DESYNC);
            sorwakeup(rop.rop_socket);
            return;
        }
        m_freem(desync_mbuf);
    }
    // Re-add timeout to try sending msg again
    timeout_add_msec(&rop.rop_timeout, ROUTE_DESYNC_RESEND_TIMEOUT);
}

/// The fields of the routing message `m` that `route_input` filters on (see the
/// deviations); a short message reads as zeros past its end.
fn rtm_peek(m: &Mbuf) -> RtmPeek {
    let mut hdr = [0u8; offset_of!(RtMsghdr, rtm_fmask)];
    let n = (m.m_pkthdr().len.get().max(0) as usize).min(hdr.len());
    m_copydata(m, 0, &mut hdr[..n]);

    let u16_at = |o: usize| u16::from_ne_bytes([hdr[o], hdr[o + 1]]);
    let i32_at = |o: usize| i32::from_ne_bytes([hdr[o], hdr[o + 1], hdr[o + 2], hdr[o + 3]]);
    RtmPeek {
        rtm_type: hdr[offset_of!(RtMsghdr, rtm_type)],
        rtm_tableid: u16_at(offset_of!(RtMsghdr, rtm_tableid)),
        rtm_priority: hdr[offset_of!(RtMsghdr, rtm_priority)],
        rtm_flags: i32_at(offset_of!(RtMsghdr, rtm_flags)),
    }
}

/// `route_input`: hands routing message `m0` to every routing socket that wants it (all
/// sockets bound to `sa_family`, or every one for `AF_UNSPEC`; `so0`, which sent it, only
/// with `SO_USELOOPBACK`), then frees it.
pub fn route_input(m0: &'static Mbuf, so0: Option<&'static Socket>, sa_family: SaFamily) {
    let m = m0;

    // ensure that we can access the rtm_type via mtod()
    if (m.m_len().get() as usize) < offset_of!(RtMsghdr, rtm_type) + 1 {
        m_freem(m);
        return;
    }
    let rtm = rtm_peek(m);

    rw_enter_read(&RTPTABLE.rtp_lk);
    for rop in RTPTABLE.rtp_list.iter() {
        // If route socket is bound to an address family only send messages that match the
        // address family. Address family agnostic messages are always sent.
        if sa_family != AF_UNSPEC
            && rop.rop_proto != u16::from(AF_UNSPEC)
            && rop.rop_proto != u16::from(sa_family)
        {
            continue;
        }

        let so = rop.rop_socket;
        solock(so);

        'next: {
            // Check to see if we don't want our own messages and if we can receive anything.
            if (so0.is_some_and(|s0| ptr::eq(s0, so)) && !so.has_options(SO_USELOOPBACK))
                || !so.has_state(SS_ISCONNECTED)
                || so.so_rcv.has_state(SS_CANTRCVMORE)
            {
                break 'next;
            }

            // filter messages that the process does not want
            // but RTM_DESYNC can't be filtered
            if rtm.rtm_type != RTM_DESYNC {
                let msgfilter = rop.rop_msgfilter.get();
                if msgfilter != 0 && msgfilter & 1u32.wrapping_shl(u32::from(rtm.rtm_type)) == 0 {
                    break 'next;
                }
                if rop.rop_flagfilter.get() & rtm.rtm_flags as u32 != 0 {
                    break 'next;
                }
            }
            match rtm.rtm_type {
                RTM_IFANNOUNCE | RTM_DESYNC => {
                    // no tableid
                }
                RTM_RESOLVE | RTM_NEWADDR | RTM_DELADDR | RTM_IFINFO | RTM_80211INFO | RTM_BFD => {
                    // check against rdomain id
                    let tid = rop.rop_rtableid.get();
                    if tid != RTABLE_ANY && rtable_l2(tid) != u32::from(rtm.rtm_tableid) {
                        break 'next;
                    }
                }
                _ => {
                    let prio = rop.rop_priority.get();
                    if prio != 0 && prio < rtm.rtm_priority {
                        break 'next;
                    }
                    // check against rtable id
                    let tid = rop.rop_rtableid.get();
                    if tid != RTABLE_ANY && tid != u32::from(rtm.rtm_tableid) {
                        break 'next;
                    }
                }
            }

            // Check to see if the flush flag is set. If so, don't queue any more messages
            // until the flag is cleared.
            if rop.rop_flags.get() & ROUTECB_FLAG_FLUSH != 0 {
                break 'next;
            }

            let _ = rtm_sendup(so, m);
        }
        sounlock(so);
    }
    rw_exit_read(&RTPTABLE.rtp_lk);

    m_freem(m);
}

/// `rtm_sendup(so, m0)`: appends a copy of `m0` to the receive buffer of `so`; when it does
/// not fit, the socket is flagged desynchronised and flushed (`ENOBUFS`).
pub fn rtm_sendup(so: &'static Socket, m0: &'static Mbuf) -> Result<(), Errno> {
    let rop = rtpcb_of(so);

    soassertlocked(so);

    let Some(m) = m_copym(m0, 0, M_COPYALL, M_NOWAIT) else {
        return Err(Errno::ENOMEM);
    };

    mtx_enter(&so.so_rcv.sb_mtx);
    let send_desync = sbspace_locked(&so.so_rcv) < (2 * MSIZE) as i64
        || !sbappendaddr(&so.so_rcv, route_src(), Some(m), None);
    mtx_leave(&so.so_rcv.sb_mtx);

    if send_desync {
        // Flag socket as desync'ed and flush required
        rop.rop_flags
            .set(rop.rop_flags.get() | ROUTECB_FLAG_DESYNC | ROUTECB_FLAG_FLUSH);
        rtm_senddesync(so);
        m_freem(m);
        return Err(Errno::ENOBUFS);
    }

    sorwakeup(so);
    Ok(())
}

/// `rtm_report(rt, type, seq, tableid)`: the answer to a request about route `rt`: a
/// `struct rt_msghdr` of `type_` with the route's addresses, metrics and flags.
fn rtm_report(rt: &'static Rtentry, type_: u8, seq: i32, tableid: u32) -> RtmBuf {
    let mut info = RtAddrinfo::new();
    let mut sa_rl = SockaddrRtlabel::default();
    let mut sa_mask = SockaddrStorage::zeroed();

    info.rti_info[RTAX_DST] = rt_key(rt);
    info.rti_info[RTAX_GATEWAY] = rt.rt_gateway.get();
    info.rti_info[RTAX_NETMASK] = rt_plen2mask(rt, &mut sa_mask);
    info.rti_info[RTAX_LABEL] = rtlabel_id2sa(rt.rt_labelid.get(), &mut sa_rl);
    // BFD: RTAX_BFD (bfd2sa), not configured.
    // MPLS: RTAX_SRC (the route's label) and rti_mpls, not configured.
    let ifp = if_get(rt.rt_ifidx.get());
    if let Some(ifp) = ifp {
        info.rti_info[RTAX_IFP] = sdltosa(ifp.if_sadl.get());
        // SAFETY: a route's key is readable.
        let family = unsafe { sa_family(info.rti_info[RTAX_DST]) };
        info.rti_info[RTAX_IFA] = rtable_getsource(tableid, family);
        if info.rti_info[RTAX_IFA].is_null() {
            info.rti_info[RTAX_IFA] = rt.ifa().ifa_addr.get();
        }
        if ifp.if_flags.get() & IFF_POINTOPOINT != 0 {
            info.rti_info[RTAX_BRD] = rt.ifa().ifa_dstaddr.get();
        }
    }
    if_put(ifp);
    // RTAX_GENMASK, RTAX_AUTHOR, RTAX_SRCMASK ignored

    // build new route message
    // SAFETY: the route's, the interface's and the local addresses, alive for the calls.
    let len = unsafe { rtm_msg2(type_, RTM_VERSION, &mut info, None, None) };
    let mut buf = RtmBuf::alloc(len, M_WAITOK);

    // SAFETY: as above.
    unsafe { rtm_msg2(type_, RTM_VERSION, &mut info, Some(buf.bytes_mut()), None) };
    let mut rtm = buf.hdr();
    rtm.rtm_type = type_;
    rtm.rtm_index = rt.rt_ifidx.get() as u16;
    rtm.rtm_tableid = tableid as u16;
    rtm.rtm_priority = rt.rt_priority.get() & RTP_MASK;
    rtm.rtm_flags = rt.rt_flags.get() as i32;
    rtm.rtm_pid = curproc_or_panic("rtm_report").process().ps_pid.get();
    rtm.rtm_seq = seq;
    rtm_getmetrics(rt, &mut rtm.rtm_rmx);
    rtm.rtm_addrs = info.rti_addrs;
    // MPLS: rtm_mpls, not configured.
    buf.set_hdr(&rtm);
    buf
}

/// `route_output(m, so)`: a request written to routing socket `so`, answered to the
/// listeners.
pub fn route_output(m: Option<&'static Mbuf>, so: &'static Socket) -> Result<(), Errno> {
    let Some(mut m) = m else {
        return Err(Errno::ENOBUFS);
    };
    if (m.m_len().get() as usize) < size_of::<i32>() {
        match m_pullup(m, size_of::<i32>() as i32) {
            Some(n) => m = n,
            None => return Err(Errno::ENOBUFS),
        }
    }
    if m.m_flags().get() & M_PKTHDR == 0 {
        panic(format_args!("route_output"));
    }

    let useloopback = so.has_options(SO_USELOOPBACK);

    // The socket can't be closed concurrently because the file descriptor reference is
    // still held.

    sounlock(so);
    let error = route_output_unlocked(m, so, useloopback);
    solock(so);

    error
}

/// The body of `route_output` between `sounlock` and `solock`: consumes `m`.
fn route_output_unlocked(
    m: &'static Mbuf,
    so: &'static Socket,
    useloopback: bool,
) -> Result<(), Errno> {
    // fail: the message buffer is freed when it goes out of scope.
    let fail = |m: &'static Mbuf, error: Errno| -> Result<(), Errno> {
        m_freem(m);
        Err(error)
    };

    let mut len = m.m_pkthdr().len.get().max(0) as usize;
    if len < offset_of!(RtMsghdr, rtm_hdrlen) + size_of::<u16>() {
        return fail(m, Errno::EINVAL);
    }
    let mut head = [0u8; offset_of!(RtMsghdr, rtm_type)];
    m_copydata(m, 0, &mut head);
    if len != usize::from(u16::from_ne_bytes([head[0], head[1]])) {
        return fail(m, Errno::EINVAL);
    }
    let vers = head[offset_of!(RtMsghdr, rtm_version)];
    let mut buf = match vers {
        RTM_VERSION => {
            if len < size_of::<RtMsghdr>() {
                return fail(m, Errno::EINVAL);
            }
            if len > RTM_MAXSIZE {
                return fail(m, Errno::EMSGSIZE);
            }
            let mut buf = RtmBuf::alloc(len, M_WAITOK);
            m_copydata(m, 0, buf.bytes_mut());
            buf
        }
        _ => return fail(m, Errno::EPROTONOSUPPORT),
    };
    let mut rtm = buf.hdr();

    // Verify that the caller is sending an appropriate message early
    match rtm.rtm_type {
        RTM_ADD | RTM_DELETE | RTM_GET | RTM_CHANGE | RTM_PROPOSAL | RTM_SOURCE => {}
        _ => return fail(m, Errno::EOPNOTSUPP),
    }
    // Verify that the header length is valid. All messages from userland start with a
    // struct rt_msghdr.
    if rtm.rtm_hdrlen == 0 {
        // old client
        rtm.rtm_hdrlen = size_of::<RtMsghdr>() as u16;
    }
    let hdrlen = usize::from(rtm.rtm_hdrlen);
    if hdrlen < size_of::<RtMsghdr>() || len < hdrlen {
        return fail(m, Errno::EINVAL);
    }

    let p = curproc_or_panic("route_output");
    rtm.rtm_pid = p.process().ps_pid.get();

    // Verify that the caller has the appropriate privilege; RTM_GET is the only operation
    // the non-superuser is allowed.
    if rtm.rtm_type != RTM_GET && suser(p).is_err() {
        return fail(m, Errno::EACCES);
    }
    let tableid = u32::from(rtm.rtm_tableid);
    if !rtable_exists(tableid) {
        if rtm.rtm_type == RTM_ADD {
            if let Err(e) = rtable_add(tableid) {
                return fail(m, e);
            }
        } else {
            return fail(m, Errno::EINVAL);
        }
    }

    // Do not let userland play with kernel-only flags.
    if (rtm.rtm_flags as u32) & (RTF_LOCAL | RTF_BROADCAST) != 0 {
        return fail(m, Errno::EINVAL);
    }

    // make sure that kernel-only bits are not set
    rtm.rtm_priority &= RTP_MASK;
    rtm.rtm_flags &= !((RTF_DONE | RTF_CLONED | RTF_CACHED) as i32);
    rtm.rtm_fmask &= RTF_FMASK as i32;

    let prio = if rtm.rtm_priority != 0 {
        if rtm.rtm_priority > RTP_MAX || rtm.rtm_priority == RTP_LOCAL {
            return fail(m, Errno::EINVAL);
        }
        rtm.rtm_priority
    } else if rtm.rtm_type != RTM_ADD {
        RTP_ANY
    } else if (rtm.rtm_flags as u32) & RTF_STATIC != 0 {
        0
    } else {
        RTP_DEFAULT
    };

    let mut info = RtAddrinfo::new();
    info.rti_addrs = rtm.rtm_addrs;
    let base = buf.ptr.as_ptr();
    // SAFETY: `hdrlen <= len`, both within the buffer; the addresses stay in it.
    if let Err(e) = unsafe { rtm_xaddrs(base.add(hdrlen), base.add(len), &mut info) } {
        return fail(m, e);
    }

    info.rti_flags = rtm.rtm_flags as u32;

    let dst = info.rti_info[RTAX_DST];
    let gate = info.rti_info[RTAX_GATEWAY];
    // SAFETY: `rtm_xaddrs` checked every address lies within the message buffer.
    let (dst_family, gate_family) = unsafe { (sa_family(dst), sa_family(gate)) };
    if rtm.rtm_type != RTM_SOURCE
        && rtm.rtm_type != RTM_PROPOSAL
        && (dst.is_null()
            || dst_family >= AF_MAX
            || (!gate.is_null() && gate_family >= AF_MAX)
            || !info.rti_info[RTAX_GENMASK].is_null())
    {
        return fail(m, Errno::EINVAL);
    }
    // MPLS: info.rti_mpls = rtm->rtm_mpls, not configured.

    if !gate.is_null() && gate_family == AF_LINK && info.rti_flags & RTF_CLONING == 0 {
        info.rti_flags |= RTF_LLINFO;
    }

    let af = dst_family;

    let mut rt: Option<&'static Rtentry> = None;
    let mut error = Ok(());
    if rtm.rtm_type == RTM_PROPOSAL {
        // Validate RTM_PROPOSAL and pass it along or error out.
        // SAFETY: as above.
        if unsafe { rtm_validate_proposal(&info) }.is_err() {
            return fail(m, Errno::EINVAL);
        }
        // If this is a solicitation proposal forward request to all interfaces. Most
        // handlers will ignore it but at least umb(4) will send a response to this event.
        if rtm.rtm_priority == RTP_PROPOSAL_SOLICIT {
            net_lock();
            for ifp in IFNETLIST.0.iter() {
                if_rtrequest(ifp, RTM_PROPOSAL, None);
            }
            net_unlock();
        }
    } else if rtm.rtm_type == RTM_SOURCE {
        if info.rti_info[RTAX_IFA].is_null() {
            return fail(m, Errno::EINVAL);
        }
        net_lock();
        // SAFETY: as above.
        let e = unsafe { rt_setsource(tableid, info.rti_info[RTAX_IFA]) };
        net_unlock();
        if let Err(e) = e {
            return fail(m, e);
        }
    } else {
        // SAFETY: as above.
        error = unsafe { rtm_output(&mut rtm, &mut rt, &mut info, prio, tableid) };
        if error.is_ok() {
            let type_ = rtm.rtm_type;
            let seq = rtm.rtm_seq;
            let Some(r) = rt else {
                panic(format_args!("route_output: no route"));
            };
            net_lock_shared();
            buf = rtm_report(r, type_, seq, tableid);
            net_unlock_shared();
            rtm = buf.hdr();
            len = usize::from(rtm.rtm_msglen);
        }
    }

    rtfree(rt);
    match error {
        Err(e) => rtm.rtm_errno = e as i32,
        Ok(()) => rtm.rtm_flags |= RTF_DONE as i32,
    }
    buf.set_hdr(&rtm);

    // Check to see if we don't want our own messages.
    if !useloopback && RTPTABLE.rtp_count.load(Ordering::Relaxed) == 0 {
        // no other listener and no loopback of messages
        m_freem(m);
        return error;
    }
    let m = if m_copyback(m, 0, &buf.bytes()[..len], M_NOWAIT).is_err() {
        m_freem(m);
        None
    } else {
        let pktlen = m.m_pkthdr().len.get();
        if pktlen as usize > len {
            m_adj(m, len as i32 - pktlen);
        }
        Some(m)
    };
    drop(buf);
    if let Some(m) = m {
        route_input(m, Some(so), af);
    }

    error
}

/// `rtm_output(rtm, prt, info, prio, tableid)`: carries out `RTM_ADD`, `RTM_DELETE`,
/// `RTM_CHANGE` or `RTM_GET` for the request `rtm` whose addresses `info` holds; the route
/// acted on goes to `prt` (referenced).
///
/// # Safety
///
/// Every non-NULL `rti_info[]` address is readable for its `sa_len` bytes, as `rtm_xaddrs`
/// leaves them; `RTAX_DST` is not NULL.
unsafe fn rtm_output(
    rtm: &mut RtMsghdr,
    prt: &mut Option<&'static Rtentry>,
    info: &mut RtAddrinfo,
    prio: u8,
    tableid: u32,
) -> Result<(), Errno> {
    let mut rt = *prt;
    let mut ifp: Option<&'static Ifnet> = None;
    let mut error = Ok(());

    let dst = info.rti_info[RTAX_DST];
    let mask = info.rti_info[RTAX_NETMASK];

    match rtm.rtm_type {
        RTM_ADD => 'add: {
            let gate = info.rti_info[RTAX_GATEWAY];
            if gate.is_null() {
                error = Err(Errno::EINVAL);
                break 'add;
            }

            // SAFETY: the caller's contract.
            rt = unsafe { rtable_match(tableid, dst, None) };
            // SAFETY: as above.
            if let Err(e) = unsafe { route_arp_conflict(rt, info) } {
                rtfree(rt);
                rt = None;
                error = Err(e);
                break 'add;
            }

            // We cannot go through a delete/create/insert cycle for cached route because
            // this can lead to races in the receive path. Instead we update the L2 cache.
            if let Some(r) = rt
                && r.rt_flags.get() & RTF_CACHED != 0
            {
                ifp = if_get(r.rt_ifidx.get());
                let Some(i) = ifp else {
                    rtfree(rt);
                    rt = None;
                    error = Err(Errno::ESRCH);
                    break 'add;
                };

                // goto change
                // SAFETY: as above.
                error = unsafe { rtm_output_change(rtm, r, i, info, false, tableid) };
                break 'add;
            }

            rtfree(rt);
            rt = None;

            net_lock();
            // SAFETY: as above.
            if let Err(e) = unsafe { rtm_getifa(info, tableid) } {
                net_unlock();
                error = Err(e);
                break 'add;
            }
            // SAFETY: as above.
            error = unsafe { rtrequest(RTM_ADD, info, prio, Some(&mut rt), tableid) };
            net_unlock();
            if error.is_ok()
                && let Some(r) = rt
            {
                rtm_setmetrics(rtm.rtm_inits, &rtm.rtm_rmx, &r.rt_rmx);
            }
        }
        RTM_DELETE => 'delete: {
            // SAFETY: the caller's contract.
            rt = unsafe { rtable_lookup(tableid, dst, mask, info.rti_info[RTAX_GATEWAY], prio) };
            let Some(r) = rt else {
                error = Err(Errno::ESRCH);
                break 'delete;
            };

            // If we got multipath routes, we require users to specify a matching gateway.
            if r.rt_flags.get() & RTF_MPATH != 0 && info.rti_info[RTAX_GATEWAY].is_null() {
                error = Err(Errno::ESRCH);
                break 'delete;
            }

            ifp = if_get(r.rt_ifidx.get());
            let Some(i) = ifp else {
                rtfree(rt);
                rt = None;
                error = Err(Errno::ESRCH);
                break 'delete;
            };

            // Invalidate the cache of automagically created and referenced L2 entries to
            // make sure that ``rt_gwroute'' pointer stays valid for other CPUs.
            if r.rt_flags.get() & RTF_CACHED != 0 {
                net_lock();
                if_rtrequest(i, RTM_INVALIDATE, Some(r));
                // Reset the MTU of the gateway route.
                // SAFETY: a route's key is readable.
                let family = unsafe { sa_family(rt_key(r)) };
                let _ = rtable_walk(tableid, family, None, |e, id| route_cleargateway(e, r, id));
                net_unlock();
                break 'delete;
            }

            // Make sure that local routes are only modified by the kernel.
            if r.rt_flags.get() & (RTF_LOCAL | RTF_BROADCAST) != 0 {
                error = Err(Errno::EINVAL);
                break 'delete;
            }

            rtfree(rt);
            rt = None;

            net_lock();
            // SAFETY: the caller's contract.
            error = unsafe { rtrequest_delete(info, prio, i, Some(&mut rt), tableid) };
            net_unlock();
        }
        RTM_CHANGE => 'change: {
            let gate = info.rti_info[RTAX_GATEWAY];
            // SAFETY: the caller's contract.
            rt = unsafe { rtable_lookup(tableid, dst, mask, gate, prio) };
            // If we got multipath routes, we require users to specify a matching gateway.
            if let Some(r) = rt
                && r.rt_flags.get() & RTF_MPATH != 0
                && gate.is_null()
            {
                rtfree(rt);
                rt = None;
            }

            // If RTAX_GATEWAY is the argument we're trying to change, try to find a
            // compatible route.
            if rt.is_none() && !gate.is_null() {
                // SAFETY: the caller's contract.
                rt = unsafe { rtable_lookup(tableid, dst, mask, ptr::null(), prio) };
                // Ensure we don't pick a multipath one.
                if let Some(r) = rt
                    && r.rt_flags.get() & RTF_MPATH != 0
                {
                    rtfree(rt);
                    rt = None;
                }
            }

            let Some(r) = rt else {
                error = Err(Errno::ESRCH);
                break 'change;
            };

            // Make sure that local routes are only modified by the kernel.
            if r.rt_flags.get() & (RTF_LOCAL | RTF_BROADCAST) != 0 {
                error = Err(Errno::EINVAL);
                break 'change;
            }

            ifp = if_get(r.rt_ifidx.get());
            let Some(i) = ifp else {
                rtfree(rt);
                rt = None;
                error = Err(Errno::ESRCH);
                break 'change;
            };

            // RTM_CHANGE needs a perfect match.
            // SAFETY: the caller's contract.
            let plen = unsafe { rtable_satoplen(sa_family(dst), mask) };
            if !plen.is_ok_and(|plen| rt_plen(r) == plen as i32) {
                error = Err(Errno::ESRCH);
                break 'change;
            }

            let mut newgate = false;
            if !gate.is_null() {
                let rgate = r.rt_gateway.get();
                // SAFETY: the caller's contract for `gate`; a route's gateway is readable
                // for its `sa_len`, and the comparison stops at the lengths when they differ.
                if rgate.is_null() || unsafe { !sa_bytes_equal(rgate, gate) } {
                    newgate = true;
                }
            }
            // Check reachable gateway before changing the route. New gateway could require
            // new ifaddr, ifp; flags may also be different; ifp may be specified by ll
            // sockaddr when protocol address is ambiguous.
            if newgate || !info.rti_info[RTAX_IFP].is_null() || !info.rti_info[RTAX_IFA].is_null() {
                net_lock();
                // SAFETY: the caller's contract.
                if let Err(e) = unsafe { rtm_getifa(info, tableid) } {
                    net_unlock();
                    error = Err(e);
                    break 'change;
                }
                let Some(ifa) = info.rti_ifa else {
                    panic(format_args!("rtm_output: rtm_getifa without address"));
                };
                if !r.rt_ifa.get().is_some_and(|a| ptr::eq(a, ifa)) {
                    if_rtrequest(i, RTM_DELETE, Some(r));
                    ifafree(r.ifa());

                    r.rt_ifa.set(Some(ifaref(ifa)));
                    let Some(nifp) = ifa.ifa_ifp.get() else {
                        panic(format_args!("rtm_output: address without interface"));
                    };
                    r.rt_ifidx.set(nifp.if_index.get());
                    // recheck link state after ifp change
                    let _ = rt_if_linkstate_change(r, nifp, tableid);
                }
                net_unlock();
            }
            // SAFETY: the caller's contract.
            error = unsafe { rtm_output_change(rtm, r, i, info, newgate, tableid) };
        }
        RTM_GET => {
            // SAFETY: the caller's contract.
            rt = unsafe { rtable_lookup(tableid, dst, mask, info.rti_info[RTAX_GATEWAY], prio) };
            if rt.is_none() {
                error = Err(Errno::ESRCH);
            }
        }
        _ => {}
    }

    if_put(ifp);
    *prt = rt;
    error
}

/// The `change:` part of `rtm_output`'s `RTM_CHANGE`, which `RTM_ADD` of a cached route
/// jumps to: the new gateway, flags, metrics and label of `rt` on interface `ifp`.
///
/// # Safety
///
/// As for [`rtm_output`].
unsafe fn rtm_output_change(
    rtm: &mut RtMsghdr,
    rt: &'static Rtentry,
    ifp: &'static Ifnet,
    info: &mut RtAddrinfo,
    newgate: bool,
    tableid: u32,
) -> Result<(), Errno> {
    let gate = info.rti_info[RTAX_GATEWAY];
    if !gate.is_null() {
        // When updating the gateway, make sure it is valid.
        let rgate = rt.rt_gateway.get();
        kassert!(newgate || !rgate.is_null());
        // SAFETY: the caller's contract for `gate`; a route's gateway is readable.
        if !newgate && unsafe { sa_family(rgate) != sa_family(gate) } {
            return Err(Errno::EINVAL);
        }

        net_lock();
        // SAFETY: the caller's contract.
        let error = unsafe { rt_setgate(rt, gate, tableid) };
        net_unlock();
        error?;
    }
    // MPLS: rt_mpls_set (RTF_MPLS) or rt_mpls_clear (newgate or RTF_MPLS in rtm_fmask), not
    // configured.
    // BFD: bfdset (RTF_BFD) or bfdclear (RTF_BFD in rtm_fmask only), not configured.

    net_lock();
    // Hack to allow some flags to be toggled
    if rtm.rtm_fmask != 0 {
        // MPLS flag it is set by rt_mpls_set()
        rtm.rtm_fmask &= !(RTF_MPLS as i32);
        rtm.rtm_flags &= !(RTF_MPLS as i32);
        let fmask = rtm.rtm_fmask as u32;
        rt.rt_flags
            .set((rt.rt_flags.get() & !fmask) | (rtm.rtm_flags as u32 & fmask));
    }
    rtm_setmetrics(rtm.rtm_inits, &rtm.rtm_rmx, &rt.rt_rmx);

    if_rtrequest(ifp, RTM_ADD, Some(rt));

    let label = info.rti_info[RTAX_LABEL];
    if !label.is_null() {
        // SAFETY: `rtm_xaddrs` checked the label holds a whole `struct sockaddr_rtlabel`.
        let rtlabel =
            unsafe { ptr::addr_of!((*label.cast::<SockaddrRtlabel>()).sr_label).read_unaligned() };
        rtlabel_unref(rt.rt_labelid.get());
        rt.rt_labelid.set(rtlabel_name2id(&rtlabel));
    }
    // SAFETY: the caller's contract.
    unsafe { if_group_routechange(info.rti_info[RTAX_DST], info.rti_info[RTAX_NETMASK]) };
    let locks = rt.rt_locks();
    locks.set(locks.get() & !rtm.rtm_inits);
    locks.set(locks.get() | (rtm.rtm_inits & rtm.rtm_rmx.rmx_locks));
    net_unlock();

    Ok(())
}

/// `bcmp(a, b, b->sa_len) == 0`: whether two socket addresses have the same bytes.
///
/// # Safety
///
/// Both point at readable socket addresses of their `sa_len` bytes.
unsafe fn sa_bytes_equal(a: *const Sockaddr, b: *const Sockaddr) -> bool {
    // SAFETY: the caller's contract.
    let (alen, blen) = unsafe { (sa_len(a), sa_len(b)) };
    if alen != blen {
        // The first byte, sa_len, already differs.
        return false;
    }
    // SAFETY: the caller's contract; both are `blen` bytes long.
    unsafe {
        slice::from_raw_parts(a.cast::<u8>(), blen) == slice::from_raw_parts(b.cast::<u8>(), blen)
    }
}

/// `ifa_ifwithroute(flags, dst, gateway, rtableid)`: the interface address a new route to
/// `dst` through `gateway` should use.
///
/// # Safety
///
/// `dst` and `gateway` point at readable socket addresses of their `sa_len` bytes.
pub unsafe fn ifa_ifwithroute(
    flags: u32,
    dst: *const Sockaddr,
    gateway: *const Sockaddr,
    rtableid: u32,
) -> Option<&'static Ifaddr> {
    let mut ifa;

    if flags & RTF_GATEWAY == 0 {
        // If we are adding a route to an interface, and the interface is a pt to pt link we
        // should search for the destination as our clue to the interface. Otherwise we can
        // use the local address.
        ifa = None;
        if flags & RTF_HOST != 0 {
            // SAFETY: the caller's contract.
            ifa = unsafe { ifa_ifwithdstaddr(dst, rtableid) };
        }
        if ifa.is_none() {
            // SAFETY: the caller's contract.
            ifa = unsafe { ifa_ifwithaddr(gateway, rtableid) };
        }
    } else {
        // If we are adding a route to a remote net or host, the gateway may still be on the
        // other end of a pt to pt link.
        // SAFETY: the caller's contract.
        ifa = unsafe { ifa_ifwithdstaddr(gateway, rtableid) };
    }
    if ifa.is_none() {
        // SAFETY: the caller's contract.
        if unsafe { sa_family(gateway) } == AF_LINK {
            let sdl = satosdl_const(gateway);
            // SAFETY: an `AF_LINK` address holds at least `sdl_index` (`rtm_xaddrs` or the
            // kernel's own `sockaddr_dl`).
            let index = unsafe { ptr::addr_of!((*sdl).sdl_index).read_unaligned() };
            let ifp = if_get(u32::from(index));
            if let Some(ifp) = ifp {
                // SAFETY: the caller's contract.
                ifa = unsafe { ifaof_ifpforaddr(dst, ifp) };
            }
            if_put(ifp);
        } else {
            // SAFETY: the caller's contract.
            let rt = unsafe { rtalloc(gateway, RT_RESOLVE, rtable_l2(rtableid)) };
            if let Some(r) = rt {
                ifa = r.rt_ifa.get();
            }
            rtfree(rt);
        }
    }
    let ifa = ifa?;
    // SAFETY: an interface address is readable; the caller's contract for `dst`.
    if unsafe { sa_family(ifa.ifa_addr.get()) != sa_family(dst) } {
        let oifa = ifa;
        let Some(ifp) = ifa.ifa_ifp.get() else {
            return Some(oifa);
        };
        // SAFETY: the caller's contract.
        return Some(unsafe { ifaof_ifpforaddr(dst, ifp) }.unwrap_or(oifa));
    }
    Some(ifa)
}

/// `rtm_getifa(info, rtid)`: picks the interface address of a new or changed route into
/// `info.rti_ifa`; `ENETUNREACH` when there is none. The address is alive only while the
/// net lock is held.
///
/// # Safety
///
/// Every non-NULL `rti_info[]` address is readable for its `sa_len` bytes.
unsafe fn rtm_getifa(info: &mut RtAddrinfo, rtid: u32) -> Result<(), Errno> {
    let mut ifp = None;

    // The "returned" `ifa' is guaranteed to be alive only if the NET_LOCK() is held.
    net_assert_locked("rtm_getifa");

    // ifp may be specified by sockaddr_dl when protocol address is ambiguous
    if !info.rti_info[RTAX_IFP].is_null() {
        let sdl = satosdl_const(info.rti_info[RTAX_IFP]);
        // SAFETY: `rtm_xaddrs` checked `RTAX_IFP` is `AF_LINK` with at least four bytes,
        // which hold `sdl_index`.
        let index = unsafe { ptr::addr_of!((*sdl).sdl_index).read_unaligned() };
        ifp = if_get(u32::from(index));
    }

    // If the destination is a PF_KEY address, we'll look for the existence of a encap
    // interface number or address in the options list of the gateway. By default, we'll
    // return enc0.
    let dst = info.rti_info[RTAX_DST];
    // SAFETY: a non-null `RTAX_DST` is a socket address (the caller's contract).
    if !dst.is_null() && unsafe { (*dst).sa_family } == PF_KEY {
        info.rti_ifa = crate::net::if_enc::enc_getifa(rtid, 0);
    }

    if info.rti_ifa.is_none() && !info.rti_info[RTAX_IFA].is_null() {
        // SAFETY: the caller's contract.
        info.rti_ifa = unsafe { ifa_ifwithaddr(info.rti_info[RTAX_IFA], rtid) };
    }

    if info.rti_ifa.is_none() {
        let mut sa = info.rti_info[RTAX_IFA];
        if sa.is_null() {
            sa = info.rti_info[RTAX_GATEWAY];
            if sa.is_null() {
                sa = info.rti_info[RTAX_DST];
            }
        }

        if !sa.is_null()
            && let Some(i) = ifp
        {
            // SAFETY: the caller's contract.
            info.rti_ifa = unsafe { ifaof_ifpforaddr(sa, i) };
        } else if !info.rti_info[RTAX_DST].is_null() && !info.rti_info[RTAX_GATEWAY].is_null() {
            // SAFETY: the caller's contract.
            info.rti_ifa = unsafe {
                ifa_ifwithroute(
                    info.rti_flags,
                    info.rti_info[RTAX_DST],
                    info.rti_info[RTAX_GATEWAY],
                    rtid,
                )
            };
        } else if !sa.is_null() {
            // SAFETY: the caller's contract.
            info.rti_ifa = unsafe { ifa_ifwithroute(info.rti_flags, sa, sa, rtid) };
        }
    }

    if_put(ifp);

    if info.rti_ifa.is_none() {
        return Err(Errno::ENETUNREACH);
    }

    Ok(())
}

/// `route_cleargateway(rt, arg, rtableid)`: resets the MTU of a gateway route whose next
/// hop is `nhrt`.
pub fn route_cleargateway(rt: &Rtentry, nhrt: &Rtentry, _rtableid: u32) -> Result<(), Errno> {
    if rt.rt_flags.get() & RTF_GATEWAY != 0
        && rt.rt_gwroute.get().is_some_and(|g| ptr::eq(g, nhrt))
        && rt.rt_locks().get() & RTV_MTU == 0
    {
        rt.rt_mtu().store(0, Ordering::Relaxed);
    }

    Ok(())
}

/// `route_arp_conflict(rt, info)`: checks that a user request to insert an ARP entry does
/// not conflict with existing ones. Only two entries are allowed for a given IP address: a
/// private one (priv) and a public one (pub).
///
/// # Safety
///
/// `rti_info[RTAX_DST]` points at a readable socket address.
unsafe fn route_arp_conflict(rt: Option<&Rtentry>, info: &mut RtAddrinfo) -> Result<(), Errno> {
    let proxy = info.rti_flags & RTF_ANNOUNCE;

    // SAFETY: the caller's contract.
    if info.rti_flags & RTF_LLINFO == 0 || unsafe { sa_family(info.rti_info[RTAX_DST]) } != AF_INET
    {
        return Ok(());
    }

    let Some(rt) = rt.filter(|rt| rt.rt_flags.get() & RTF_LLINFO != 0) else {
        return Ok(());
    };

    // If the entry is cached, it can be updated.
    if rt.rt_flags.get() & RTF_CACHED != 0 {
        return Ok(());
    }

    // Same destination, not cached and both "priv" or "pub" conflict. If a second entry
    // exists, it always conflict.
    if rt.rt_flags.get() & RTF_ANNOUNCE == proxy || rt.rt_flags.get() & RTF_MPATH != 0 {
        return Err(Errno::EEXIST);
    }

    // No conflict but an entry exist so we need to force mpath.
    info.rti_flags |= RTF_MPATH;
    Ok(())
}

/// `rtm_setmetrics(which, in, out)`: the metrics of a request (`rtm_inits` names them) into
/// a route; the expiry goes from wall clock to uptime.
pub fn rtm_setmetrics(which: u32, in_: &RtMetrics, out: &RtKmetrics) {
    if which & RTV_MTU != 0 {
        out.rmx_mtu.store(in_.rmx_mtu, Ordering::Relaxed);
    }
    if which & RTV_EXPIRE != 0 {
        let mut expire = in_.rmx_expire;
        if expire != 0 {
            expire -= gettime();
            expire += getuptime();
        }

        out.rmx_expire.set(expire);
    }
}

/// `rtm_getmetrics(rt, out)`: the metrics of route `rt` for userland; the expiry goes from
/// uptime to wall clock.
pub fn rtm_getmetrics(rt: &Rtentry, out: &mut RtMetrics) {
    let in_ = &rt.rt_rmx;

    let mut expire = in_.rmx_expire.get();
    if expire == 0 {
        expire = rt_timer_get_expire(rt);
    }
    if expire != 0 {
        expire -= getuptime();
        expire += gettime();
    }

    *out = RtMetrics::default();
    out.rmx_locks = in_.rmx_locks.get();
    out.rmx_mtu = in_.rmx_mtu.load(Ordering::Relaxed);
    out.rmx_expire = expire;
    out.rmx_pksent = in_.rmx_pksent.get();
}

/// `rtm_xaddrs(cp, cplim, rtinfo)`: parses the address bits of `rti_addrs`, splits the
/// address storage between `cp` and `cplim` in chunks and sets the info pointers, then
/// checks each address's family and size against its role.
///
/// # Safety
///
/// `cp..cplim` is a readable range of one buffer that outlives the use of `rti_info[]`.
pub unsafe fn rtm_xaddrs(
    cp: *const u8,
    cplim: *const u8,
    rtinfo: &mut RtAddrinfo,
) -> Result<(), Errno> {
    let mut cp = cp as usize;
    let cplim = cplim as usize;

    // Parse address bits, split address storage in chunks, and set info pointers. Use sa_len
    // for traversing the memory and check that we stay within in the limit.
    rtinfo.rti_info = [ptr::null(); RTAX_MAX];
    for i in 0..i32::BITS as usize {
        if (rtinfo.rti_addrs as u32) & (1u32 << i) == 0 {
            continue;
        }
        if i >= RTAX_MAX || cp + SIZEOF_SOCKLEN_T > cplim {
            return Err(Errno::EINVAL);
        }
        let sa = cp as *const Sockaddr;
        // SAFETY: at least `sizeof(socklen_t)` bytes are left (checked), so `sa_len` is
        // readable.
        let len = unsafe { sa_len(sa) };
        if cp + len > cplim {
            return Err(Errno::EINVAL);
        }
        rtinfo.rti_info[i] = sa;
        cp += roundup_long(len);
    }
    // Check that the address family is suitable for the route address type. Check that each
    // address has a size that fits its family and its length is within the size. Strings
    // within addresses must be NUL terminated.
    for i in 0..RTAX_MAX {
        let sa = rtinfo.rti_info[i];
        if sa.is_null() {
            continue;
        }
        // SAFETY: the first loop checked every address lies within the buffer.
        let (family, salen) = unsafe { (sa_family(sa), sa_len(sa)) };
        let mut maxlen = 0;
        let mut size = 0;
        match i {
            RTAX_DST | RTAX_GATEWAY | RTAX_SRC => match family {
                AF_INET => size = size_of::<SockaddrIn>(),
                AF_LINK => size = size_of::<crate::net::if_dl::SockaddrDl>(),
                #[cfg(feature = "inet6")]
                crate::sys::socket::AF_INET6 => size = SIZEOF_SOCKADDR_IN6,
                // MPLS: sizeof(struct sockaddr_mpls), not configured.
                _ => {}
            },
            RTAX_IFP => {
                if family != AF_LINK {
                    return Err(Errno::EAFNOSUPPORT);
                }
                // XXX Should be sizeof(struct sockaddr_dl), but route(8) has a bug and
                // provides less memory. arp(8) has another bug and uses sizeof pointer.
                size = 4;
            }
            RTAX_IFA => match family {
                AF_INET => size = size_of::<SockaddrIn>(),
                #[cfg(feature = "inet6")]
                crate::sys::socket::AF_INET6 => size = SIZEOF_SOCKADDR_IN6,
                _ => return Err(Errno::EAFNOSUPPORT),
            },
            RTAX_LABEL => {
                if family != AF_UNSPEC {
                    return Err(Errno::EAFNOSUPPORT);
                }
                maxlen = RTLABEL_LEN;
                size = size_of::<SockaddrRtlabel>();
            }
            // BFD: RTAX_BFD, not configured.
            RTAX_DNS => {
                // more validation in rtm_validate_proposal
                if salen > size_of::<SockaddrRtdns>() {
                    return Err(Errno::EINVAL);
                }
                if salen < offset_of!(SockaddrRtdns, sr_dns) {
                    return Err(Errno::EINVAL);
                }
                match family {
                    AF_INET => {}
                    #[cfg(feature = "inet6")]
                    crate::sys::socket::AF_INET6 => {}
                    _ => return Err(Errno::EAFNOSUPPORT),
                }
            }
            RTAX_STATIC => {
                match family {
                    AF_INET => {}
                    #[cfg(feature = "inet6")]
                    crate::sys::socket::AF_INET6 => {}
                    _ => return Err(Errno::EAFNOSUPPORT),
                }
                maxlen = RTSTATIC_LEN;
                size = size_of::<SockaddrRtstatic>();
            }
            RTAX_SEARCH => {
                if family != AF_UNSPEC {
                    return Err(Errno::EAFNOSUPPORT);
                }
                maxlen = RTSEARCH_LEN;
                size = size_of::<SockaddrRtsearch>();
            }
            _ => {}
        }
        // memory for the full struct must be provided
        if size != 0 && salen < size {
            return Err(Errno::EINVAL);
        }
        if maxlen != 0 {
            // this should not happen
            if 2 + maxlen > size {
                return Err(Errno::EINVAL);
            }
            // strings must be NUL terminated within the struct
            // SAFETY: `sa_len >= size >= 2 + maxlen` (checked), so the `maxlen` bytes after
            // the two-byte header lie within the address.
            let data = unsafe { slice::from_raw_parts(sa.cast::<u8>().add(2), maxlen) };
            let len = data.iter().position(|&c| c == 0).unwrap_or(maxlen);
            if len >= maxlen || 2 + len >= salen {
                return Err(Errno::EINVAL);
            }
            // The C breaks out of the loop here: the addresses after the first string
            // address are not checked.
            break;
        }
    }
    Ok(())
}

/// `rtm_msg1`: a routing message of `type_` in a new mbuf: the header for its type (zeroed)
/// followed by the addresses of `rtinfo`, each rounded up to a long, which it records in
/// `rti_addrs`.
///
/// # Safety
///
/// Every non-NULL `rti_info[]` address is readable for its `sa_len` bytes.
pub unsafe fn rtm_msg1(type_: u8, rtinfo: Option<&mut RtAddrinfo>) -> Option<&'static Mbuf> {
    let hlen = match type_ {
        RTM_DELADDR | RTM_NEWADDR => size_of::<IfaMsghdr>(),
        RTM_IFINFO => size_of::<IfMsghdr>(),
        RTM_IFANNOUNCE => size_of::<IfAnnouncemsghdr>(),
        // BFD: RTM_BFD's struct bfd_msghdr, not configured.
        RTM_80211INFO => size_of::<IfIeee80211Msghdr>(),
        _ => size_of::<RtMsghdr>(),
    };
    let info: &[*const Sockaddr] = match &rtinfo {
        Some(i) => &i.rti_info,
        None => &[],
    };
    let mut len = hlen;
    for &sa in info.iter().take(RTAX_MAX) {
        if sa.is_null() {
            continue;
        }
        // SAFETY: the caller's contract.
        len += roundup_long(unsafe { sa_len(sa) });
    }
    if len > MCLBYTES {
        panic(format_args!("rtm_msg1"));
    }
    let mut m = m_gethdr(M_DONTWAIT, MT_DATA);
    if let Some(mm) = m
        && len > MHLEN
    {
        mclget(mm, M_DONTWAIT);
        if mm.m_flags().get() & M_EXT == 0 {
            m_free(mm);
            m = None;
        }
    }
    let m = m?;
    m.m_pkthdr().len.set(len as i32);
    m.m_len().set(len as u32);
    m.m_pkthdr().ph_ifidx.set(0);
    let rtm = mtod::<u8>(m);
    // SAFETY: the mbuf's data area holds `len` bytes (MHLEN or a cluster).
    unsafe { ptr::write_bytes(rtm, 0, len) };
    let mut addrs = 0;
    let mut off = hlen;
    for (i, &sa) in info.iter().enumerate().take(RTAX_MAX) {
        if sa.is_null() {
            continue;
        }
        addrs |= 1 << i;
        // SAFETY: the caller's contract.
        let salen = unsafe { sa_len(sa) };
        let dlen = roundup_long(salen);
        // SAFETY: as above.
        let bytes = unsafe { slice::from_raw_parts(sa.cast::<u8>(), salen) };
        if m_copyback(m, off as i32, bytes, M_DONTWAIT).is_err() {
            m_freem(m);
            return None;
        }
        off += dlen;
    }
    if let Some(i) = rtinfo {
        i.rti_addrs |= addrs;
    }
    let rtm = mtod::<RtMsghdr>(m);
    // SAFETY: the data area starts with `hlen` bytes, at least the common header fields of
    // every message type, and is aligned for it (a fresh mbuf's data).
    unsafe {
        (*rtm).rtm_msglen = off as u16;
        (*rtm).rtm_hdrlen = hlen as u16;
        (*rtm).rtm_version = RTM_VERSION;
        (*rtm).rtm_type = type_;
    }
    Some(m)
}

/// `rtm_msg2(type, vers, rtinfo, cp, w)`: the length of a routing message of `type_` with
/// the addresses of `rtinfo` (recorded in `rti_addrs`), aligned. With `cp` the message is
/// built there; with a walk and no `cp`, the length is added to `w_needed` and, when the
/// message fits the user's buffer, it is built in the walk's `w_tmem` (grown as needed).
///
/// # Safety
///
/// Every non-NULL `rti_info[]` address is readable for its `sa_len` bytes; `cp`, when given,
/// holds the length this returns.
unsafe fn rtm_msg2(
    type_: u8,
    _vers: u8,
    rtinfo: &mut RtAddrinfo,
    cp: Option<&mut [u8]>,
    mut w: Option<&mut Walkarg>,
) -> usize {
    let mut cp = cp;
    let mut second_time = false;

    rtinfo.rti_addrs = 0;
    loop {
        // again:
        let hlen = match type_ {
            RTM_DELADDR | RTM_NEWADDR => size_of::<IfaMsghdr>(),
            RTM_IFINFO => size_of::<IfMsghdr>(),
            _ => size_of::<RtMsghdr>(),
        };
        let mut len = hlen;
        for i in 0..RTAX_MAX {
            let sa = rtinfo.rti_info[i];
            if sa.is_null() {
                continue;
            }
            rtinfo.rti_addrs |= 1 << i;
            // SAFETY: the caller's contract.
            let salen = unsafe { sa_len(sa) };
            let dlen = roundup_long(salen);
            if let Some(buf) = cp.as_deref_mut() {
                // SAFETY: the caller's contract.
                let bytes = unsafe { slice::from_raw_parts(sa.cast::<u8>(), salen) };
                buf[len..len + salen].copy_from_slice(bytes);
                buf[len + salen..len + dlen].fill(0);
            }
            len += dlen;
        }
        // align message length to the next natural boundary
        let len = align(len);
        if cp.is_none()
            && !second_time
            && let Some(w) = w.as_deref_mut()
        {
            w.w_needed += len;
            if w.w_needed <= w.w_given && w.w_where != 0 {
                if w.w_tmemsize < len {
                    if let Some(t) = w.w_tmem.take() {
                        free(t, M_RTABLE, w.w_tmemsize);
                    }
                    w.w_tmem = malloc(len, M_RTABLE, M_NOWAIT | M_ZERO);
                    if w.w_tmem.is_some() {
                        w.w_tmemsize = len;
                    }
                }
                if let Some(t) = w.w_tmem {
                    // SAFETY: the walk's buffer holds `w_tmemsize >= len` bytes, zeroed when
                    // allocated and written only as bytes since; nothing else borrows it.
                    cp = Some(unsafe { slice::from_raw_parts_mut(t.as_ptr(), len) });
                    second_time = true;
                    continue;
                } else {
                    w.w_where = 0;
                }
            }
        }
        if let Some(buf) = cp {
            if w.is_some() {
                // clear the message header
                buf[..hlen].fill(0);
            }

            let msglen = offset_of!(RtMsghdr, rtm_msglen);
            buf[msglen..msglen + 2].copy_from_slice(&(len as u16).to_ne_bytes());
            buf[offset_of!(RtMsghdr, rtm_version)] = RTM_VERSION;
            buf[offset_of!(RtMsghdr, rtm_type)] = type_;
            let hdrlen = offset_of!(RtMsghdr, rtm_hdrlen);
            buf[hdrlen..hdrlen + 2].copy_from_slice(&(hlen as u16).to_ne_bytes());
        }
        return len;
    }
}

/// `rtm_send`: announces route `rt` with message `cmd`.
pub fn rtm_send(rt: &Rtentry, cmd: u8, error: i32, rtableid: u32) {
    let mut info = RtAddrinfo::new();
    let mut sa_rl = SockaddrRtlabel::default();
    let mut sa_mask = SockaddrStorage::zeroed();

    info.rti_info[RTAX_DST] = rt_key(rt);
    info.rti_info[RTAX_GATEWAY] = rt.rt_gateway.get();
    if rt.rt_flags.get() & RTF_HOST == 0 {
        info.rti_info[RTAX_NETMASK] = rt_plen2mask(rt, &mut sa_mask);
    }
    info.rti_info[RTAX_LABEL] = rtlabel_id2sa(rt.rt_labelid.get(), &mut sa_rl);
    let ifp = if_get(rt.rt_ifidx.get());
    if let Some(ifp) = ifp {
        info.rti_info[RTAX_IFP] = sdltosa(ifp.if_sadl.get());
        // SAFETY: a route's key is readable.
        let family = unsafe { sa_family(info.rti_info[RTAX_DST]) };
        info.rti_info[RTAX_IFA] = rtable_getsource(rtableid, family);
        if info.rti_info[RTAX_IFA].is_null() {
            info.rti_info[RTAX_IFA] = rt.ifa().ifa_addr.get();
        }
    }

    // SAFETY: the route's, the interface's and the local addresses, alive for the call.
    unsafe {
        rtm_miss(
            cmd,
            &mut info,
            rt.rt_flags.get(),
            rt.rt_priority.get(),
            rt.rt_ifidx.get(),
            error,
            rtableid,
        )
    };
    if_put(ifp);
}

/// `rtm_miss`: generates a routing message indicating that a redirect has occurred, a routing
/// lookup has failed, or that a protocol has detected timeouts to a particular destination.
///
/// # Safety
///
/// As for [`rtm_msg1`].
pub unsafe fn rtm_miss(
    type_: u8,
    rtinfo: &mut RtAddrinfo,
    flags: u32,
    prio: u8,
    ifidx: u32,
    error: i32,
    tableid: u32,
) {
    let sa = rtinfo.rti_info[RTAX_DST];

    if RTPTABLE.rtp_count.load(Ordering::Relaxed) == 0 {
        return;
    }
    // SAFETY: the caller's contract.
    let Some(m) = (unsafe { rtm_msg1(type_, Some(rtinfo)) }) else {
        return;
    };
    let rtm = mtod::<RtMsghdr>(m);
    // SAFETY: `rtm_msg1` made a message with a `struct rt_msghdr`.
    unsafe {
        (*rtm).rtm_flags = (RTF_DONE | flags) as i32;
        (*rtm).rtm_priority = prio;
        (*rtm).rtm_errno = error;
        (*rtm).rtm_tableid = tableid as u16;
        (*rtm).rtm_addrs = rtinfo.rti_addrs;
        (*rtm).rtm_index = ifidx as u16;
    }
    // SAFETY: the caller's contract.
    route_input(m, None, unsafe { sa_family(sa) });
}

/// `rtm_ifchg`: generates a routing message indicating that the status of a network
/// interface has changed.
pub fn rtm_ifchg(ifp: &Ifnet) {
    let mut info = RtAddrinfo::new();

    if RTPTABLE.rtp_count.load(Ordering::Relaxed) == 0 {
        return;
    }
    info.rti_info[RTAX_IFP] = sdltosa(ifp.if_sadl.get());
    // SAFETY: the interface's link address is readable.
    let Some(m) = (unsafe { rtm_msg1(RTM_IFINFO, Some(&mut info)) }) else {
        return;
    };
    // SAFETY: `rtm_msg1` made a message with a `struct if_msghdr`, aligned (a fresh mbuf);
    // the fields are assigned one by one, so the padding bytes stay zero.
    let ifm = unsafe { &mut *mtod::<IfMsghdr>(m) };
    ifm.ifm_index = ifp.if_index.get() as u16;
    ifm.ifm_tableid = ifp.if_rdomain.get() as u16;
    ifm.ifm_flags = ifp.if_flags.get();
    ifm.ifm_xflags = ifp.if_xflags.get();
    if_getdata(ifp, &mut ifm.ifm_data);
    ifm.ifm_addrs = info.rti_addrs;
    route_input(m, None, AF_UNSPEC);
}

/// `rtm_addr`: generates a routing message indicating that a network interface has had
/// address `ifa` associated with it (`RTM_NEWADDR`) or removed (`RTM_DELADDR`).
pub fn rtm_addr(cmd: u8, ifa: &Ifaddr) {
    let Some(ifp) = ifa.ifa_ifp.get() else {
        return;
    };
    let mut info = RtAddrinfo::new();

    if RTPTABLE.rtp_count.load(Ordering::Relaxed) == 0 {
        return;
    }

    info.rti_info[RTAX_IFA] = ifa.ifa_addr.get();
    info.rti_info[RTAX_IFP] = sdltosa(ifp.if_sadl.get());
    info.rti_info[RTAX_NETMASK] = ifa.ifa_netmask.get();
    info.rti_info[RTAX_BRD] = ifa.ifa_dstaddr.get();
    // SAFETY: the address's and the interface's socket addresses are readable.
    let Some(m) = (unsafe { rtm_msg1(cmd, Some(&mut info)) }) else {
        return;
    };
    // SAFETY: `rtm_msg1` made a message with a `struct ifa_msghdr`, aligned.
    let ifam = unsafe { &mut *mtod::<IfaMsghdr>(m) };
    ifam.ifam_index = ifp.if_index.get() as u16;
    ifam.ifam_metric = ifa.ifa_metric.get();
    ifam.ifam_flags = ifa.ifa_flags.get() as i32;
    ifam.ifam_addrs = info.rti_addrs;
    ifam.ifam_tableid = ifp.if_rdomain.get() as u16;

    // SAFETY: an interface address's `ifa_addr` is NULL or a readable socket address.
    route_input(m, None, unsafe { sa_family(ifa.ifa_addr.get()) });
}

/// `rtm_ifannounce`: generates a routing message indicating a network interface's arrival
/// or departure (`what`).
pub fn rtm_ifannounce(ifp: &Ifnet, what: u16) {
    if RTPTABLE.rtp_count.load(Ordering::Relaxed) == 0 {
        return;
    }
    // SAFETY: no addresses.
    let Some(m) = (unsafe { rtm_msg1(RTM_IFANNOUNCE, None) }) else {
        return;
    };
    // SAFETY: `rtm_msg1` made a message with a `struct if_announcemsghdr`, aligned.
    let ifan = unsafe { &mut *mtod::<IfAnnouncemsghdr>(m) };
    ifan.ifan_index = ifp.if_index.get() as u16;
    let xname: [u8; IFNAMSIZ] = ifp.if_xname.get();
    libkern::strlcpy(&mut ifan.ifan_name, &xname);
    ifan.ifan_what = what;
    route_input(m, None, AF_UNSPEC);
}

// BFD: rtm_bfd, not configured.

/// `rtm_80211info`: generates a routing message indicating the state of an ieee80211
/// interface.
pub fn rtm_80211info(ifp: &Ifnet, ifie: &IfIeee80211Data) {
    if RTPTABLE.rtp_count.load(Ordering::Relaxed) == 0 {
        return;
    }
    // SAFETY: no addresses.
    let Some(m) = (unsafe { rtm_msg1(RTM_80211INFO, None) }) else {
        return;
    };
    // SAFETY: `rtm_msg1` made a message with a `struct if_ieee80211_msghdr`, aligned.
    let ifim = unsafe { &mut *mtod::<IfIeee80211Msghdr>(m) };
    ifim.ifim_index = ifp.if_index.get() as u16;
    ifim.ifim_tableid = ifp.if_rdomain.get() as u16;

    ifim.ifim_ifie = *ifie;
    route_input(m, None, AF_UNSPEC);
}

/// `rtm_proposal`: generates a routing message with the address selection proposal from an
/// interface. Unlike the other announcements it is built even without listeners, as in C.
///
/// # Safety
///
/// As for [`rtm_msg1`]; `rti_info[RTAX_DNS]` is not NULL.
pub unsafe fn rtm_proposal(ifp: &Ifnet, rtinfo: &mut RtAddrinfo, flags: u32, prio: u8) {
    // SAFETY: the caller's contract.
    let Some(m) = (unsafe { rtm_msg1(RTM_PROPOSAL, Some(rtinfo)) }) else {
        return;
    };
    let rtm = mtod::<RtMsghdr>(m);
    // SAFETY: `rtm_msg1` made a message with a `struct rt_msghdr`.
    unsafe {
        (*rtm).rtm_flags = (RTF_DONE | flags) as i32;
        (*rtm).rtm_priority = prio;
        (*rtm).rtm_tableid = ifp.if_rdomain.get() as u16;
        (*rtm).rtm_index = ifp.if_index.get() as u16;
        (*rtm).rtm_addrs = rtinfo.rti_addrs;
    }

    // SAFETY: the caller's contract.
    let family = unsafe { (*rtinfo.rti_info[RTAX_DNS]).sa_family };
    route_input(m, None, family);
}

/// `(T *)w->w_tmem`: the message header at the start of the walk's buffer.
///
/// # Safety
///
/// `t` is the walk's `w_tmem`, holding at least `size_of::<T>()` initialised bytes; `T` is a
/// `#[repr(C)]` message header of integers (any bit pattern valid); nothing else borrows the
/// buffer while the reference lives. A `malloc` bucket is aligned for any `T`.
unsafe fn tmem_hdr<'a, T>(t: NonNull<u8>) -> &'a mut T {
    // SAFETY: the caller's contract.
    unsafe { &mut *t.as_ptr().cast::<T>() }
}

/// The first `len` bytes of the walk's buffer.
///
/// # Safety
///
/// `t` is the walk's `w_tmem`, holding at least `len` initialised bytes.
unsafe fn tmem_bytes<'a>(t: NonNull<u8>, len: usize) -> &'a [u8] {
    // SAFETY: the caller's contract.
    unsafe { slice::from_raw_parts(t.as_ptr(), len) }
}

/// `sysctl_dumpentry(rt, v, id)`: one route of a `NET_RT_DUMP` or `NET_RT_FLAGS` walk, as
/// an `RTM_GET` message.
fn sysctl_dumpentry(rt: &Rtentry, w: &mut Walkarg, id: u32) -> Result<(), Errno> {
    let mut error = Ok(());
    let mut info = RtAddrinfo::new();
    let mut sa_rl = SockaddrRtlabel::default();
    let mut sa_mask = SockaddrStorage::zeroed();

    if w.w_op == NET_RT_FLAGS && rt.rt_flags.get() & (w.w_arg as u32) == 0 {
        return Ok(());
    }
    if w.w_op == NET_RT_DUMP && w.w_arg != 0 {
        let rtprio = rt.rt_priority.get() & RTP_MASK;
        if w.w_arg < 0 {
            let prio = (w.w_arg.wrapping_neg() & i32::from(RTP_MASK)) as u8;
            // Show all routes that are not this priority
            if prio == rtprio {
                return Ok(());
            }
        } else {
            let prio = (w.w_arg & i32::from(RTP_MASK)) as u8;
            if prio != rtprio && prio != RTP_ANY {
                return Ok(());
            }
        }
    }
    info.rti_info[RTAX_DST] = rt_key(rt);
    info.rti_info[RTAX_GATEWAY] = rt.rt_gateway.get();
    info.rti_info[RTAX_NETMASK] = rt_plen2mask(rt, &mut sa_mask);
    let ifp = if_get(rt.rt_ifidx.get());
    if let Some(ifp) = ifp {
        info.rti_info[RTAX_IFP] = sdltosa(ifp.if_sadl.get());
        // SAFETY: a route's key is readable.
        let family = unsafe { sa_family(info.rti_info[RTAX_DST]) };
        info.rti_info[RTAX_IFA] = rtable_getsource(id, family);
        if info.rti_info[RTAX_IFA].is_null() {
            info.rti_info[RTAX_IFA] = rt.ifa().ifa_addr.get();
        }
        if ifp.if_flags.get() & IFF_POINTOPOINT != 0 {
            info.rti_info[RTAX_BRD] = rt.ifa().ifa_dstaddr.get();
        }
    }
    if_put(ifp);
    info.rti_info[RTAX_LABEL] = rtlabel_id2sa(rt.rt_labelid.get(), &mut sa_rl);
    // BFD: RTAX_BFD (bfd2sa), not configured.
    // MPLS: RTAX_SRC (the route's label) and rti_mpls, not configured.

    // SAFETY: the route's, the interface's and the local addresses, alive for the call.
    let size = unsafe { rtm_msg2(RTM_GET, RTM_VERSION, &mut info, None, Some(&mut *w)) };
    if w.w_where != 0
        && let Some(t) = w.w_tmem
        && w.w_needed <= w.w_given
    {
        // SAFETY: `rtm_msg2` built the message in `w_tmem`, at least a `struct rt_msghdr`.
        let rtm = unsafe { tmem_hdr::<RtMsghdr>(t) };

        rtm.rtm_pid = curproc_or_panic("sysctl_dumpentry").process().ps_pid.get();
        rtm.rtm_flags = (RTF_DONE | rt.rt_flags.get()) as i32;
        rtm.rtm_priority = rt.rt_priority.get() & RTP_MASK;
        rtm_getmetrics(rt, &mut rtm.rtm_rmx);
        // Do not account the routing table's reference.
        rtm.rtm_rmx.rmx_refcnt = refcnt_read(&rt.rt_refcnt).wrapping_sub(1);
        rtm.rtm_index = rt.rt_ifidx.get() as u16;
        rtm.rtm_addrs = info.rti_addrs;
        rtm.rtm_tableid = id as u16;
        // MPLS: rtm_mpls, not configured.
        // SAFETY: the message's `size` bytes, all initialised.
        error = copyout(unsafe { tmem_bytes(t, size) }, w.w_where);
        if error.is_err() {
            w.w_where = 0;
        } else {
            w.w_where += size;
        }
    }
    error
}

/// `sysctl_rtable_rtstat(oldp, oldlenp, newp)`: `net.route.0.0.stats`, `struct rtstat`.
fn sysctl_rtable_rtstat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    // counters_read(rtcounters): one word of struct rtstat per counter.
    let words: [u32; RtstatCounters::RtsNcounters as usize] =
        core::array::from_fn(|i| RTCOUNTERS[i].load(Ordering::Relaxed) as u32);

    sysctl_rdstruct(oldp, oldlenp, newp, words.as_bytes())
}

/// `sysctl_iflist(af, w)`: the interfaces (`RTM_IFINFO`) each followed by its addresses of
/// family `af` (all for 0, `RTM_NEWADDR`); only interface `w_arg` when it is not 0.
fn sysctl_iflist(af: u8, w: &mut Walkarg) -> Result<(), Errno> {
    let mut info = RtAddrinfo::new();

    for ifp in IFNETLIST.0.iter() {
        if w.w_arg != 0 && w.w_arg as u32 != ifp.if_index.get() {
            continue;
        }
        // Copy the link-layer address first
        info.rti_info[RTAX_IFP] = sdltosa(ifp.if_sadl.get());
        // SAFETY: the interface's link address, alive for the call.
        let len = unsafe { rtm_msg2(RTM_IFINFO, RTM_VERSION, &mut info, None, Some(&mut *w)) };
        if w.w_where != 0
            && let Some(t) = w.w_tmem
            && w.w_needed <= w.w_given
        {
            // SAFETY: `rtm_msg2` built the message in `w_tmem`, at least a `struct
            // if_msghdr`; the fields are assigned one by one, so the padding stays zero.
            let ifm = unsafe { tmem_hdr::<IfMsghdr>(t) };
            ifm.ifm_index = ifp.if_index.get() as u16;
            ifm.ifm_tableid = ifp.if_rdomain.get() as u16;
            ifm.ifm_flags = ifp.if_flags.get();
            if_getdata(ifp, &mut ifm.ifm_data);
            ifm.ifm_addrs = info.rti_addrs;
            // SAFETY: the message's `len` bytes, all initialised.
            copyout(unsafe { tmem_bytes(t, len) }, w.w_where)?;
            w.w_where += len;
        }
        info.rti_info[RTAX_IFP] = ptr::null();
        for ifa in ifp.if_addrlist.iter() {
            // SAFETY: an interface address's `ifa_addr` is a readable socket address.
            let family = unsafe { sa_family(ifa.ifa_addr.get()) };
            kassert!(family != AF_LINK);
            if af != 0 && af != family {
                continue;
            }
            info.rti_info[RTAX_IFA] = ifa.ifa_addr.get();
            info.rti_info[RTAX_NETMASK] = ifa.ifa_netmask.get();
            info.rti_info[RTAX_BRD] = ifa.ifa_dstaddr.get();
            // SAFETY: the address's socket addresses, alive for the call.
            let len = unsafe { rtm_msg2(RTM_NEWADDR, RTM_VERSION, &mut info, None, Some(&mut *w)) };
            if w.w_where != 0
                && let Some(t) = w.w_tmem
                && w.w_needed <= w.w_given
            {
                // SAFETY: `rtm_msg2` built the message in `w_tmem`, at least a `struct
                // ifa_msghdr`.
                let ifam = unsafe { tmem_hdr::<IfaMsghdr>(t) };
                let Some(aifp) = ifa.ifa_ifp.get() else {
                    panic(format_args!("sysctl_iflist: address without interface"));
                };
                ifam.ifam_index = aifp.if_index.get() as u16;
                ifam.ifam_flags = ifa.ifa_flags.get() as i32;
                ifam.ifam_metric = ifa.ifa_metric.get();
                ifam.ifam_addrs = info.rti_addrs;
                // SAFETY: the message's `len` bytes, all initialised.
                copyout(unsafe { tmem_bytes(t, len) }, w.w_where)?;
                w.w_where += len;
            }
        }
        info.rti_info[RTAX_IFA] = ptr::null();
        info.rti_info[RTAX_NETMASK] = ptr::null();
        info.rti_info[RTAX_BRD] = ptr::null();
    }
    Ok(())
}

/// `sysctl_ifnames(w)`: a `struct if_nameindex_msg` per interface (only interface `w_arg`
/// when it is not 0).
fn sysctl_ifnames(w: &mut Walkarg) -> Result<(), Errno> {
    let if_tmplist: TailqHead<IfTmplist> = TailqHead::new();
    let mut error = Ok(());

    rw_enter_write(&IF_TMPLIST_LOCK);
    net_lock_shared();
    // XXX ignore tableid for now
    for ifp in IFNETLIST.0.iter() {
        if w.w_arg != 0 && w.w_arg as u32 != ifp.if_index.get() {
            continue;
        }
        let _ = if_ref(ifp);
        // SAFETY: `if_tmplist` is free under `IF_TMPLIST_LOCK`; the reference keeps the
        // interface alive until it is taken off below.
        unsafe { if_tmplist.insert_tail(ifp) };
    }
    net_unlock_shared();

    const SIZE: usize = size_of::<IfNameindexMsg>();
    for ifp in if_tmplist.iter() {
        w.w_needed += SIZE;
        if w.w_where != 0 && w.w_needed <= w.w_given {
            // memset(&ifn, 0, sizeof(ifn)), then the index and the name.
            let mut ifn = [0u8; SIZE];
            let idx = offset_of!(IfNameindexMsg, if_index);
            ifn[idx..idx + 4].copy_from_slice(&ifp.if_index.get().to_ne_bytes());
            let name = offset_of!(IfNameindexMsg, if_name);
            let xname: [u8; IFNAMSIZ] = ifp.if_xname.get();
            libkern::strlcpy(&mut ifn[name..name + IFNAMSIZ], &xname);
            error = copyout(&ifn, w.w_where);
            if error.is_err() {
                break;
            }
            w.w_where += SIZE;
        }
    }

    while let Some(ifp) = if_tmplist.first() {
        // SAFETY: the first element of the temporary list.
        unsafe { if_tmplist.remove(ifp) };
        if_put(ifp);
    }
    rw_exit_write(&IF_TMPLIST_LOCK);

    error
}

/// `sysctl_source(af, tableid, w)`: the preferred source address of `af` in `tableid`, if
/// one is set.
fn sysctl_source(af: SaFamily, tableid: u32, w: &mut Walkarg) -> Result<(), Errno> {
    // union { struct sockaddr_in in; struct sockaddr_in6 in6; } buf
    let mut buf = [0u8; SIZEOF_SOCKADDR_IN6];
    let mut size = 0;

    net_lock_shared();
    let mut sa = rtable_getsource(tableid, af);
    if !sa.is_null() {
        // SAFETY: a preferred source is an interface's address, readable.
        match unsafe { sa_family(sa) } {
            AF_INET => size = size_of::<SockaddrIn>(),
            #[cfg(feature = "inet6")]
            crate::sys::socket::AF_INET6 => size = SIZEOF_SOCKADDR_IN6,
            _ => sa = ptr::null(),
        }
    }
    if !sa.is_null() {
        // SAFETY: an `AF_INET` (`AF_INET6`) interface address is a whole `struct sockaddr_in`
        // (`sockaddr_in6`) of `size` bytes.
        buf[..size].copy_from_slice(unsafe { slice::from_raw_parts(sa.cast::<u8>(), size) });
    }
    net_unlock_shared();

    if !sa.is_null() {
        w.w_needed += size;
        if w.w_where != 0 && w.w_needed <= w.w_given {
            copyout(&buf[..size], w.w_where)?;
            w.w_where += size;
        }
    }
    Ok(())
}

/// `sysctl_rtable`: the `net.route` sysctl: `name` is the address family (0 for all), the
/// `NET_RT_*` operation, its argument and, optionally, the routing table.
pub fn sysctl_rtable(
    name: &[i32],
    where_: usize,
    given: &mut usize,
    new: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    let mut error = Err(Errno::EINVAL);

    if new != 0 {
        return Err(Errno::EPERM);
    }
    if name.len() < 3 || name.len() > 4 {
        return Err(Errno::EINVAL);
    }
    let af = name[0] as u8;
    let mut w = Walkarg {
        w_where: where_,
        w_given: *given,
        w_op: name[1],
        w_arg: name[2],
        ..Walkarg::default()
    };

    let mut tableid = if name.len() == 4 {
        let tableid = name[3] as u32;
        if !rtable_exists(tableid) {
            return Err(Errno::ENOENT);
        }
        tableid
    } else {
        curproc_or_panic("sysctl_rtable")
            .process()
            .ps_rtableid
            .load(Ordering::Relaxed)
    };

    match w.w_op {
        NET_RT_DUMP | NET_RT_FLAGS => {
            net_lock_shared();
            for i in 1..=AF_MAX {
                if af != 0 && af != i {
                    continue;
                }

                error = match rtable_read(tableid, i, |rt, id| sysctl_dumpentry(rt, &mut w, id)) {
                    Err(Errno::EAFNOSUPPORT) => Ok(()),
                    e => e,
                };
                if error.is_err() {
                    break;
                }
            }
            net_unlock_shared();
        }
        NET_RT_STATS => return sysctl_rtable_rtstat(where_, given, new),
        NET_RT_TABLE => {
            tableid = w.w_arg as u32;
            if rtable_exists(tableid) {
                let tableinfo = RtTableinfo {
                    rti_tableid: tableid as u16,
                    rti_domainid: rtable_l2(tableid) as u16,
                };
                return sysctl_rdstruct(where_, given, new, tableinfo.as_bytes());
            } else {
                return Err(Errno::ENOENT);
            }
        }
        NET_RT_SOURCE => {
            tableid = w.w_arg as u32;
            if !rtable_exists(tableid) {
                return Err(Errno::ENOENT);
            }
            for i in 1..=AF_MAX {
                if af != 0 && af != i {
                    continue;
                }

                error = match sysctl_source(i, tableid, &mut w) {
                    Err(Errno::EAFNOSUPPORT) => Ok(()),
                    e => e,
                };
                if error.is_err() {
                    break;
                }
            }
        }
        NET_RT_IFLIST => {
            net_lock_shared();
            error = sysctl_iflist(af, &mut w);
            net_unlock_shared();
        }
        NET_RT_IFNAMES => error = sysctl_ifnames(&mut w),
        _ => {}
    }
    if let Some(t) = w.w_tmem.take() {
        free(t, M_RTABLE, w.w_tmemsize);
    }
    if where_ != 0 {
        *given = w.w_where.wrapping_sub(where_);
        if w.w_needed > w.w_given {
            return Err(Errno::ENOMEM);
        }
    } else if w.w_needed == 0 {
        *given = 0;
    } else {
        let n = w.w_needed + (w.w_needed / 10).max(1024);
        *given = n.div_ceil(PAGE_SIZE) * PAGE_SIZE;
    }
    error
}

/// `rtm_validate_proposal(info)`: the addresses of an `RTM_PROPOSAL` are only a netmask, an
/// interface address, DNS servers, static routes and a search path, each well formed;
/// `EINVAL` otherwise.
///
/// # Safety
///
/// Every non-NULL `rti_info[]` address is readable for its `sa_len` bytes.
unsafe fn rtm_validate_proposal(info: &RtAddrinfo) -> Result<(), Errno> {
    let bad = Err(Errno::EINVAL);

    if info.rti_addrs & !(RTA_NETMASK | RTA_IFA | RTA_DNS | RTA_STATIC | RTA_SEARCH) != 0 {
        return bad;
    }

    // An address of family AF_INET or AF_INET6, of exactly its size.
    let inet_sized = |sa: *const Sockaddr| -> Result<(), Errno> {
        if sa.is_null() {
            return Err(Errno::EINVAL);
        }
        // SAFETY: the caller's contract.
        let (family, len) = unsafe { (sa_family(sa), sa_len(sa)) };
        match family {
            AF_INET if len == size_of::<SockaddrIn>() => Ok(()),
            crate::sys::socket::AF_INET6 if len == SIZEOF_SOCKADDR_IN6 => Ok(()),
            _ => Err(Errno::EINVAL),
        }
    };

    if info.rti_addrs & RTA_NETMASK != 0 {
        inet_sized(info.rti_info[RTAX_NETMASK])?;
    }

    if info.rti_addrs & RTA_IFA != 0 {
        inet_sized(info.rti_info[RTAX_IFA])?;
    }

    if info.rti_addrs & RTA_DNS != 0 {
        let rtdns = info.rti_info[RTAX_DNS];
        if rtdns.is_null() {
            return bad;
        }
        // SAFETY: the caller's contract.
        let (family, len) = unsafe { (sa_family(rtdns), sa_len(rtdns)) };
        if len > size_of::<SockaddrRtdns>() {
            return bad;
        }
        if len < offset_of!(SockaddrRtdns, sr_dns) {
            return bad;
        }
        match family {
            AF_INET => {
                if (len - offset_of!(SockaddrRtdns, sr_dns)) % size_of::<InAddr>() != 0 {
                    return bad;
                }
            }
            #[cfg(feature = "inet6")]
            crate::sys::socket::AF_INET6 => {
                if (len - offset_of!(SockaddrRtdns, sr_dns))
                    % size_of::<crate::netinet6::in6::In6Addr>()
                    != 0
                {
                    return bad;
                }
            }
            _ => return bad,
        }
    }

    if info.rti_addrs & RTA_STATIC != 0 {
        let rtstatic = info.rti_info[RTAX_STATIC];
        if rtstatic.is_null() {
            return bad;
        }
        // SAFETY: the caller's contract.
        let len = unsafe { sa_len(rtstatic) };
        if len > size_of::<SockaddrRtstatic>() {
            return bad;
        }
        if len <= offset_of!(SockaddrRtstatic, sr_static) {
            return bad;
        }
    }

    if info.rti_addrs & RTA_SEARCH != 0 {
        let rtsearch = info.rti_info[RTAX_SEARCH];
        if rtsearch.is_null() {
            return bad;
        }
        // SAFETY: the caller's contract.
        let len = unsafe { sa_len(rtsearch) };
        if len > size_of::<SockaddrRtsearch>() {
            return bad;
        }
        if len <= offset_of!(SockaddrRtsearch, sr_search) {
            return bad;
        }
    }

    Ok(())
}

/// `rt_setsource(rtableid, src)`: the preferred source address of `src`'s family in table
/// `rtableid`; the unspecified address returns to automatic selection. `EINVAL` when no
/// interface of the table has `src`.
///
/// # Safety
///
/// `src` points at a readable socket address of its `sa_len` bytes.
unsafe fn rt_setsource(rtableid: u32, src: *const Sockaddr) -> Result<(), Errno> {
    // If source address is 0.0.0.0 or :: use automatic source selection
    // SAFETY: the caller's contract.
    match unsafe { sa_family(src) } {
        AF_INET => {
            // SAFETY: `rtm_xaddrs` checked an `AF_INET` `RTAX_IFA` holds a whole `struct
            // sockaddr_in`; maybe unaligned.
            let addr =
                unsafe { ptr::addr_of!((*satosin_const(src)).sin_addr.s_addr).read_unaligned() };
            if addr == INADDR_ANY {
                let _ = rtable_setsource(rtableid, AF_INET, ptr::null());
                return Ok(());
            }
        }
        #[cfg(feature = "inet6")]
        crate::sys::socket::AF_INET6 => {
            // SAFETY: `rtm_xaddrs` checked an `AF_INET6` `RTAX_IFA` holds a whole `struct
            // sockaddr_in6`; maybe unaligned.
            let sin6 = unsafe { ptr::read_unaligned(crate::netinet6::in6::satosin6_const(src)) };
            if crate::netinet6::in6::in6_is_addr_unspecified(&sin6.sin6_addr) {
                let _ = rtable_setsource(rtableid, crate::sys::socket::AF_INET6, ptr::null());
                return Ok(());
            }
        }
        _ => return Err(Errno::EAFNOSUPPORT),
    }

    // Check if source address is assigned to an interface in the same rdomain
    // SAFETY: the caller's contract.
    let Some(ifa) = (unsafe { ifa_ifwithaddr(src, rtableid) }) else {
        return Err(Errno::EINVAL);
    };

    // SAFETY: as above.
    rtable_setsource(rtableid, unsafe { sa_family(src) }, ifa.ifa_addr.get())
}

// CTASSERT(sizeof(rtstat) == (nitems(counters) * sizeof(uint32_t)))
const _: () = assert!(
    size_of::<crate::net::route::Rtstat>()
        == RtstatCounters::RtsNcounters as usize * size_of::<u32>()
);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the routing socket: the message layout `rtm_msg2` builds, the address
    // parsing and checks of `rtm_xaddrs`, the `net.route` sysctl (route dumps, interface lists
    // and names, table information, statistics) over a test Ethernet interface with an
    // address, and a routing socket end to end (`socreate`, `sosend` of `RTM_GET`, `RTM_ADD`
    // and `RTM_DELETE`, the answers read back with `soreceive`, the socket options, `soclose`).
    //
    // The tests run as a thread with root credentials made `curproc` (the requests check
    // `suser` and stamp `rtm_pid`); they clear `curproc` before they return.

    use std::boxed::Box;
    use std::sync::MutexGuard;
    use std::vec;
    use std::vec::Vec;

    use super::*;
    use crate::kern::kern_proc::procinit;
    use crate::kern::kern_prot::{crget, crhold};
    use crate::kern::uipc_mbuf::m_get;
    use crate::kern::uipc_socket::{soclose, socreate, soinit, soreceive, sosend};
    use crate::machine::Machine;
    use crate::machine::cpu::Cpu;
    use crate::net::route::{
        RTA_BRD, RTA_DST, RTA_GATEWAY, RTA_IFA, RTA_IFP, RTA_LABEL, RTA_NETMASK,
    };
    use crate::netinet::in_::sintosa;
    use crate::netinet::ip_input::tests::{configure, sin, test_ether, unconfigure};
    use crate::sys::mbuf::MT_SOOPTS;
    use crate::sys::proc::Process;
    use crate::sys::socket::MSG_DONTWAIT;
    use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};

    type Guards = (MutexGuard<'static, ()>, MutexGuard<'static, ()>);

    /// The pid the test thread's process has.
    const PID: i32 = 42;

    /// The network setup (memory, mbufs, a routing table, IPv4) with an empty interface list,
    /// the socket and routing control block pools, and a root thread as `curproc`.
    fn setup() -> Guards {
        let g = crate::netinet::ip_input::tests::setup();
        // Interfaces and interface groups of earlier tests live in the memory just reset.
        IFNETLIST.0.init();
        crate::net::if_::IFG_HEAD.0.init();
        procinit();
        soinit();
        route_prinit();
        RTPTABLE.rtp_count.store(0, Ordering::Relaxed);

        let pr: &'static Process = Box::leak(Box::new(Process::new()));
        let p: &'static Proc = Box::leak(Box::new(Proc::new()));
        p.p_p.set(pr);
        pr.ps_mainproc.set(p);
        pr.ps_pid.set(PID);
        let cr = crget();
        p.p_ucred.set(cr);
        pr.ps_ucred.set(crhold(cr));
        Machine::set_curproc(Machine::curcpu(), p);
        g
    }

    /// Undoes what outlives the reset memory: `curproc`.
    fn teardown() {
        Machine::set_curproc(Machine::curcpu(), ptr::null());
    }

    /// The `struct rt_msghdr` at `off` in `buf`; zeros past the end of a shorter message (the
    /// interface messages share only the first fields).
    fn hdr_at(buf: &[u8], off: usize) -> RtMsghdr {
        let mut h = [0u8; size_of::<RtMsghdr>()];
        let n = (buf.len() - off).min(h.len());
        h[..n].copy_from_slice(&buf[off..off + n]);
        // SAFETY: a local array of the header's size; integers only, read unaligned.
        unsafe { h.as_ptr().cast::<RtMsghdr>().read_unaligned() }
    }

    /// The bytes of a `T` (a `#[repr(C)]` value without padding).
    fn bytes_of<T>(v: &T) -> &[u8] {
        // SAFETY: the callers pass socket addresses and message headers without padding.
        unsafe { slice::from_raw_parts(ptr::from_ref(v).cast::<u8>(), size_of::<T>()) }
    }

    /// The IPv4 address of the `sockaddr_in` at `off` in `buf`.
    fn in_at(buf: &[u8], off: usize) -> [u8; 4] {
        let a = off + offset_of!(SockaddrIn, sin_addr);
        [buf[a], buf[a + 1], buf[a + 2], buf[a + 3]]
    }

    /// The messages of a sysctl answer: (offset, header) of each, by `rtm_msglen`.
    fn messages(buf: &[u8]) -> Vec<(usize, RtMsghdr)> {
        let mut v = Vec::new();
        let mut off = 0;
        while off < buf.len() {
            let h = hdr_at(buf, off);
            assert_eq!(h.rtm_version, RTM_VERSION);
            assert!(h.rtm_msglen as usize >= usize::from(h.rtm_hdrlen));
            v.push((off, h));
            off += usize::from(h.rtm_msglen);
        }
        assert_eq!(off, buf.len(), "messages end at the answer's end");
        v
    }

    /// `sysctl(name)` the way libc does it: the size first, then the answer.
    fn sysctl_answer(name: &[i32]) -> Vec<u8> {
        let mut size = 0;
        sysctl_rtable(name, 0, &mut size, 0, 0).expect("size estimate");
        let mut buf = vec![0u8; size];
        let mut len = size;
        sysctl_rtable(name, buf.as_mut_ptr() as usize, &mut len, 0, 0).expect("answer");
        buf.truncate(len);
        buf
    }

    #[test]
    fn rtm_msg2_lays_out_the_header_and_the_rounded_addresses() {
        let mut dst = sin([10, 0, 2, 0]);
        let mut mask = sin([255, 255, 255, 0]);
        mask.sin_len = 7; // trimmed, as rt_plen2mask leaves it
        let mut info = RtAddrinfo::new();
        info.rti_info[RTAX_DST] = sintosa(&mut dst);
        info.rti_info[RTAX_NETMASK] = sintosa(&mut mask);

        // SAFETY: local addresses.
        let len = unsafe { rtm_msg2(RTM_GET, RTM_VERSION, &mut info, None, None) };
        assert_eq!(len, size_of::<RtMsghdr>() + 16 + 8);
        assert_eq!(info.rti_addrs, RTA_DST | RTA_NETMASK);

        let mut buf = vec![0xffu8; len];
        // SAFETY: local addresses; the buffer holds `len` bytes.
        let again = unsafe { rtm_msg2(RTM_GET, RTM_VERSION, &mut info, Some(&mut buf), None) };
        assert_eq!(again, len);
        let h = hdr_at(&buf, 0);
        assert_eq!(usize::from(h.rtm_msglen), len);
        assert_eq!(h.rtm_version, RTM_VERSION);
        assert_eq!(h.rtm_type, RTM_GET);
        assert_eq!(usize::from(h.rtm_hdrlen), size_of::<RtMsghdr>());
        // Without a walk the rest of the caller's header is left alone.
        assert_eq!(buf[offset_of!(RtMsghdr, rtm_index)], 0xff);
        let a = size_of::<RtMsghdr>();
        assert_eq!(&buf[a..a + 16], bytes_of(&dst));
        assert_eq!(&buf[a + 16..a + 23], &bytes_of(&mask)[..7]);
        assert_eq!(buf[a + 23], 0, "rounded up with zeros");

        // The interface messages have their own headers.
        let mut info = RtAddrinfo::new();
        // SAFETY: no addresses.
        assert_eq!(
            unsafe { rtm_msg2(RTM_IFINFO, RTM_VERSION, &mut info, None, None) },
            size_of::<IfMsghdr>()
        );
        // SAFETY: no addresses.
        assert_eq!(
            unsafe { rtm_msg2(RTM_NEWADDR, RTM_VERSION, &mut info, None, None) },
            size_of::<IfaMsghdr>()
        );
        assert_eq!(info.rti_addrs, 0);
    }

    #[test]
    fn rtm_xaddrs_splits_and_checks_the_addresses() {
        let dst = sin([10, 0, 2, 0]);
        let gw = sin([10, 0, 2, 2]);
        let mut label = SockaddrRtlabel {
            sr_len: size_of::<SockaddrRtlabel>() as u8,
            sr_family: AF_UNSPEC,
            sr_label: [0; RTLABEL_LEN],
        };
        label.sr_label[..4].copy_from_slice(b"blue");
        let mut buf = [0u64; 16];
        // SAFETY: a local buffer of 128 bytes, viewed as bytes.
        let bytes = unsafe { slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), 128) };
        bytes[..16].copy_from_slice(bytes_of(&dst));
        bytes[16..32].copy_from_slice(bytes_of(&gw));
        bytes[32..32 + size_of::<SockaddrRtlabel>()].copy_from_slice(bytes_of(&label));
        let end = 32 + roundup_long(size_of::<SockaddrRtlabel>());
        let base = bytes.as_ptr();

        let parse = |addrs: i32, len: usize| {
            let mut info = RtAddrinfo::new();
            info.rti_addrs = addrs;
            // SAFETY: within the local buffer.
            let r = unsafe { rtm_xaddrs(base, base.add(len), &mut info) };
            (r, info)
        };

        let (r, info) = parse(RTA_DST | RTA_GATEWAY | RTA_LABEL, end);
        assert_eq!(r, Ok(()));
        assert_eq!(info.rti_info[RTAX_DST], base.cast());
        // SAFETY: within the local buffer.
        assert_eq!(info.rti_info[RTAX_GATEWAY], unsafe { base.add(16) }.cast());
        // SAFETY: within the local buffer.
        assert_eq!(info.rti_info[RTAX_LABEL], unsafe { base.add(32) }.cast());
        assert!(info.rti_info[RTAX_NETMASK].is_null());

        // An address past the end, a bit past RTAX_MAX.
        assert_eq!(parse(RTA_DST | RTA_GATEWAY, 24).0, Err(Errno::EINVAL));
        assert_eq!(parse(RTA_DST | (1 << 20), end).0, Err(Errno::EINVAL));
        // A sockaddr_in as the interface (which must be AF_LINK) or as a label.
        assert_eq!(parse(RTA_DST | RTA_IFP, end).0, Err(Errno::EAFNOSUPPORT));
        assert_eq!(parse(RTA_LABEL, end).0, Err(Errno::EAFNOSUPPORT));

        // A short sockaddr_in as the destination.
        bytes[0] = 8;
        assert_eq!(parse(RTA_DST, end).0, Err(Errno::EINVAL));
        bytes[0] = 16;

        // A label without its NUL.
        bytes[34..34 + RTLABEL_LEN].fill(b'x');
        assert_eq!(
            parse(RTA_DST | RTA_GATEWAY | RTA_LABEL, end).0,
            Err(Errno::EINVAL)
        );
    }

    #[test]
    fn sysctl_dumps_the_routes_of_an_interface() {
        let _g = setup();
        let ifp = test_ether();
        configure(ifp, [10, 0, 2, 15], [255, 255, 255, 0]);

        // The size estimate: what is needed plus a tenth (at least 1024), in pages.
        let mut size = 0;
        sysctl_rtable(&[0, NET_RT_DUMP, 0, 0], 0, &mut size, 0, 0).expect("estimate");
        assert!(size > 0 && size % PAGE_SIZE == 0);

        let buf = sysctl_answer(&[0, NET_RT_DUMP, 0, 0]);
        let msgs = messages(&buf);
        let mut seen = Vec::new();
        for &(off, h) in &msgs {
            assert_eq!(h.rtm_type, RTM_GET);
            assert_eq!(usize::from(h.rtm_hdrlen), size_of::<RtMsghdr>());
            assert_ne!(h.rtm_flags as u32 & RTF_DONE, 0);
            assert_eq!(h.rtm_pid, PID);
            assert_eq!(u32::from(h.rtm_index), ifp.if_index.get());
            assert_eq!(h.rtm_tableid, 0);
            let want = RTA_DST | RTA_GATEWAY | RTA_NETMASK | RTA_IFP | RTA_IFA;
            assert_eq!(h.rtm_addrs & want, want, "addresses of {h:?}");
            seen.push((
                in_at(&buf, off + usize::from(h.rtm_hdrlen)),
                h.rtm_flags as u32,
            ));
        }
        let has = |a: [u8; 4], flag: u32| seen.iter().any(|&(d, f)| d == a && f & flag != 0);
        assert!(has([10, 0, 2, 15], RTF_LOCAL), "local route: {seen:?}");
        assert!(has([10, 0, 2, 0], RTF_CLONING), "prefix route: {seen:?}");
        assert!(
            has([10, 0, 2, 255], RTF_BROADCAST),
            "broadcast route: {seen:?}"
        );

        // NET_RT_FLAGS keeps the routes with one of the flags.
        let local = sysctl_answer(&[0, NET_RT_FLAGS, RTF_LOCAL as i32, 0]);
        let local = messages(&local);
        assert_eq!(local.len(), 1);
        // A priority filter: none of these is static.
        let stat = sysctl_answer(&[0, NET_RT_DUMP, i32::from(crate::net::route::RTP_STATIC), 0]);
        assert!(stat.is_empty());

        // A buffer too small for the first message: nothing copied, ENOMEM.
        let mut small = [0u8; 50];
        let mut len = small.len();
        assert_eq!(
            sysctl_rtable(
                &[0, NET_RT_DUMP, 0, 0],
                small.as_mut_ptr() as usize,
                &mut len,
                0,
                0
            ),
            Err(Errno::ENOMEM)
        );
        assert_eq!(len, 0);

        // The argument checks.
        let mut len = 0;
        assert_eq!(
            sysctl_rtable(&[0, NET_RT_DUMP, 0, 0], 0, &mut len, 8, 4),
            Err(Errno::EPERM)
        );
        assert_eq!(
            sysctl_rtable(&[0, NET_RT_DUMP], 0, &mut len, 0, 0),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            sysctl_rtable(&[0, NET_RT_DUMP, 0, 9], 0, &mut len, 0, 0),
            Err(Errno::ENOENT)
        );
        assert_eq!(
            sysctl_rtable(&[0, 99, 0, 0], 0, &mut len, 0, 0),
            Err(Errno::EINVAL)
        );

        unconfigure(ifp, [10, 0, 2, 15]);
        teardown();
    }

    #[test]
    fn sysctl_lists_interfaces_their_addresses_and_names() {
        let _g = setup();
        let ifp = test_ether();
        configure(ifp, [10, 0, 2, 15], [255, 255, 255, 0]);
        let index = ifp.if_index.get() as i32;

        // NET_RT_IFLIST: the interface, then its address.
        let buf = sysctl_answer(&[0, NET_RT_IFLIST, index]);
        let msgs = messages(&buf);
        assert_eq!(msgs.len(), 2, "{msgs:?}");
        let (off, h) = msgs[0];
        assert_eq!(h.rtm_type, RTM_IFINFO);
        assert_eq!(usize::from(h.rtm_hdrlen), size_of::<IfMsghdr>());
        // SAFETY: an if_msghdr at the start of the answer; integers only.
        let ifm = unsafe { buf.as_ptr().add(off).cast::<IfMsghdr>().read_unaligned() };
        assert_eq!(i32::from(ifm.ifm_index), index);
        assert_eq!(ifm.ifm_addrs, RTA_IFP);
        assert_eq!(ifm.ifm_flags, ifp.if_flags.get());
        assert_eq!(ifm.ifm_data.ifi_mtu, ifp.if_mtu.get());
        let sdl = off + size_of::<IfMsghdr>();
        assert_eq!(buf[sdl + 1], AF_LINK);
        let nlen = usize::from(buf[sdl + offset_of!(crate::net::if_dl::SockaddrDl, sdl_nlen)]);
        let name = sdl + offset_of!(crate::net::if_dl::SockaddrDl, sdl_data);
        assert_eq!(&buf[name..name + nlen], b"tvio0");

        let (off, h) = msgs[1];
        assert_eq!(h.rtm_type, RTM_NEWADDR);
        assert_eq!(usize::from(h.rtm_hdrlen), size_of::<IfaMsghdr>());
        // SAFETY: an ifa_msghdr; integers only.
        let ifam = unsafe { buf.as_ptr().add(off).cast::<IfaMsghdr>().read_unaligned() };
        assert_eq!(i32::from(ifam.ifam_index), index);
        assert_eq!(ifam.ifam_addrs, RTA_IFA | RTA_NETMASK | RTA_BRD);
        // RTAX_NETMASK comes first, then RTAX_IFA.
        let mask_len = usize::from(buf[off + size_of::<IfaMsghdr>()]);
        let ifa = off + size_of::<IfaMsghdr>() + roundup_long(mask_len);
        assert_eq!(buf[ifa + 1], AF_INET);
        assert_eq!(in_at(&buf, ifa), [10, 0, 2, 15]);

        // Only the link message for a family without addresses.
        let link = sysctl_answer(&[
            i32::from(crate::sys::socket::AF_INET6),
            NET_RT_IFLIST,
            index,
        ]);
        assert_eq!(messages(&link).len(), 1);

        // NET_RT_IFNAMES: the index and the name.
        let names = sysctl_answer(&[0, NET_RT_IFNAMES, index]);
        assert_eq!(names.len(), size_of::<IfNameindexMsg>());
        assert_eq!(names[..4], (index as u32).to_ne_bytes());
        assert_eq!(&names[4..10], b"tvio0\0");

        // NET_RT_TABLE: table 0 in routing domain 0; table 9 does not exist.
        let mut info = [0xffu8; 4];
        let mut len = info.len();
        sysctl_rtable(
            &[0, NET_RT_TABLE, 0],
            info.as_mut_ptr() as usize,
            &mut len,
            0,
            0,
        )
        .expect("table 0");
        assert_eq!((len, info), (4, [0; 4]));
        assert_eq!(
            sysctl_rtable(&[0, NET_RT_TABLE, 9], 0, &mut len, 0, 0),
            Err(Errno::ENOENT)
        );

        // NET_RT_STATS: struct rtstat, one word per counter.
        RTCOUNTERS[RtstatCounters::RtsUnreach as usize].store(3, Ordering::Relaxed);
        let mut stats = [0u32; RtstatCounters::RtsNcounters as usize];
        let mut len = size_of::<crate::net::route::Rtstat>();
        sysctl_rtable(
            &[0, NET_RT_STATS, 0],
            stats.as_mut_ptr() as usize,
            &mut len,
            0,
            0,
        )
        .expect("stats");
        assert_eq!(stats[RtstatCounters::RtsUnreach as usize], 3);
        RTCOUNTERS[RtstatCounters::RtsUnreach as usize].store(0, Ordering::Relaxed);

        // NET_RT_SOURCE: no preferred source is set.
        let mut len = 0;
        sysctl_rtable(&[0, NET_RT_SOURCE, 0], 0, &mut len, 0, 0).expect("source");
        assert_eq!(len, 0);

        unconfigure(ifp, [10, 0, 2, 15]);
        teardown();
    }

    /// `sosend` of `bytes` from kernel space.
    fn send(so: &'static Socket, bytes: &[u8]) -> Result<(), Errno> {
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
        sosend(so, None, Some(&mut uio), None, None, 0)
    }

    /// One message read without waiting, `EWOULDBLOCK` when there is none.
    fn recv(so: &'static Socket) -> Result<Vec<u8>, Errno> {
        let mut buf = vec![0u8; 1024];
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
        let mut flags = MSG_DONTWAIT;
        soreceive(so, None, &mut uio, None, None, Some(&mut flags), 0)?;
        let n = len - uio.uio_resid;
        buf.truncate(n);
        Ok(buf)
    }

    /// A request: a `struct rt_msghdr` of `type_` and `seq` with `addrs` after it.
    fn request(type_: u8, seq: i32, flags: u32, addrs: &[(i32, SockaddrIn)]) -> Vec<u8> {
        let mut rtm = RtMsghdr {
            rtm_version: RTM_VERSION,
            rtm_type: type_,
            rtm_hdrlen: size_of::<RtMsghdr>() as u16,
            rtm_seq: seq,
            rtm_flags: flags as i32,
            ..RtMsghdr::default()
        };
        let mut v = bytes_of(&rtm).to_vec();
        for (bit, sa) in addrs {
            rtm.rtm_addrs |= bit;
            v.extend_from_slice(bytes_of(sa));
        }
        rtm.rtm_msglen = v.len() as u16;
        v[..size_of::<RtMsghdr>()].copy_from_slice(bytes_of(&rtm));
        v
    }

    /// The answer to request `seq`, skipping the kernel's own announcements.
    fn answer(so: &'static Socket, seq: i32) -> (Vec<u8>, RtMsghdr) {
        loop {
            let m = recv(so).expect("an answer");
            let h = hdr_at(&m, 0);
            if h.rtm_seq == seq && h.rtm_pid == PID {
                return (m, h);
            }
        }
    }

    /// An option mbuf holding `v`.
    fn opt(v: u32) -> &'static Mbuf {
        let m = m_get(M_DONTWAIT, MT_SOOPTS).expect("mbuf");
        m.m_len().set(4);
        // SAFETY: a fresh mbuf holds MLEN bytes.
        unsafe { mtod::<u32>(m).write_unaligned(v) };
        m
    }

    #[test]
    fn a_routing_socket_gets_adds_and_deletes_routes() {
        let _g = setup();
        let ifp = test_ether();
        configure(ifp, [10, 0, 2, 15], [255, 255, 255, 0]);

        let so = socreate(i32::from(PF_ROUTE), SOCK_RAW, 0).expect("socket(AF_ROUTE)");
        assert_eq!(RTPTABLE.rtp_count.load(Ordering::Relaxed), 1);
        assert!(so.has_state(SS_ISCONNECTED) && so.has_options(SO_USELOOPBACK));

        // RTM_GET of an address of the subnet: the /24, with its netmask.
        send(
            so,
            &request(RTM_GET, 1, 0, &[(RTA_DST, sin([10, 0, 2, 77]))]),
        )
        .expect("RTM_GET");
        let (m, h) = answer(so, 1);
        assert_eq!(h.rtm_type, RTM_GET);
        assert_eq!(h.rtm_errno, 0);
        assert_ne!(h.rtm_flags as u32 & RTF_DONE, 0);
        assert_eq!(u32::from(h.rtm_index), ifp.if_index.get());
        assert_ne!(h.rtm_addrs & RTA_NETMASK, 0);
        assert_eq!(h.rtm_addrs & RTA_LABEL, 0);
        assert_eq!(in_at(&m, size_of::<RtMsghdr>()), [10, 0, 2, 0]);

        // RTM_ADD of a default route through 10.0.2.2.
        let default = [
            (RTA_DST, sin([0, 0, 0, 0])),
            (RTA_GATEWAY, sin([10, 0, 2, 2])),
            (RTA_NETMASK, sin([0, 0, 0, 0])),
        ];
        let flags = crate::net::route::RTF_UP | RTF_GATEWAY | RTF_STATIC;
        send(so, &request(RTM_ADD, 2, flags, &default)).expect("RTM_ADD");
        let (_, h) = answer(so, 2);
        assert_eq!((h.rtm_type, h.rtm_errno), (RTM_ADD, 0));
        assert_ne!(h.rtm_flags as u32 & RTF_GATEWAY, 0);

        // An address off the subnet now goes through it.
        send(so, &request(RTM_GET, 3, 0, &[(RTA_DST, sin([8, 8, 8, 8]))])).expect("RTM_GET");
        let (m, h) = answer(so, 3);
        assert_eq!(in_at(&m, size_of::<RtMsghdr>()), [0, 0, 0, 0]);
        assert_eq!(
            in_at(&m, size_of::<RtMsghdr>() + 16),
            [10, 0, 2, 2],
            "gateway"
        );
        assert_eq!(h.rtm_addrs & RTA_GATEWAY, RTA_GATEWAY);
        // The sysctl dump shows it.
        let dump = sysctl_answer(&[0, NET_RT_DUMP, 0, 0]);
        assert!(messages(&dump).iter().any(|&(off, h)| in_at(
            &dump,
            off + usize::from(h.rtm_hdrlen)
        ) == [0; 4]
            && h.rtm_flags as u32 & RTF_STATIC != 0));

        // RTM_DELETE, then RTM_GET fails with ESRCH, which the answer carries too.
        send(so, &request(RTM_DELETE, 4, 0, &default)).expect("RTM_DELETE");
        let (_, h) = answer(so, 4);
        assert_eq!((h.rtm_type, h.rtm_errno), (RTM_DELETE, 0));
        // As in C, deleting the default route leaves the egress group until the next rebuild;
        // rebuild it now (empty) so no group of this test outlives its memory.
        net_lock();
        crate::net::if_::if_group_egress_build().expect("egress");
        net_unlock();
        assert!(
            crate::net::if_::IFG_HEAD
                .0
                .iter()
                .all(|g| !g.ifg_group.starts_with(b"egress"))
        );
        let get = request(RTM_GET, 5, 0, &[(RTA_DST, sin([8, 8, 8, 8]))]);
        assert_eq!(send(so, &get), Err(Errno::ESRCH));
        let (_, h) = answer(so, 5);
        assert_eq!(h.rtm_errno, Errno::ESRCH as i32);
        assert_eq!(h.rtm_flags as u32 & RTF_DONE, 0);

        // Malformed requests.
        let mut bad = get.clone();
        bad[offset_of!(RtMsghdr, rtm_version)] = 4;
        assert_eq!(send(so, &bad), Err(Errno::EPROTONOSUPPORT));
        let mut bad = get.clone();
        bad[offset_of!(RtMsghdr, rtm_type)] = RTM_IFINFO;
        assert_eq!(send(so, &bad), Err(Errno::EOPNOTSUPP));
        let mut bad = get.clone();
        bad[0] += 1;
        assert_eq!(
            send(so, &bad),
            Err(Errno::EINVAL),
            "rtm_msglen is not the length"
        );
        let local = request(RTM_ADD, 6, RTF_LOCAL, &default);
        assert_eq!(send(so, &local), Err(Errno::EINVAL), "a kernel-only flag");
        assert_eq!(
            recv(so),
            Err(Errno::EWOULDBLOCK),
            "no answers to malformed requests"
        );

        // The options: a message filter that only lets RTM_ADD through.
        let level = i32::from(AF_ROUTE);
        route_ctloutput(
            PRCO_SETOPT,
            so,
            level,
            ROUTE_MSGFILTER,
            Some(opt(1 << RTM_ADD)),
        )
        .expect("ROUTE_MSGFILTER");
        let m = opt(0);
        route_ctloutput(PRCO_GETOPT, so, level, ROUTE_MSGFILTER, Some(m)).expect("get");
        // SAFETY: the option mbuf holds four bytes.
        assert_eq!(unsafe { mtod::<u32>(m).read_unaligned() }, 1 << RTM_ADD);
        m_freem(m);
        send(
            so,
            &request(RTM_GET, 7, 0, &[(RTA_DST, sin([10, 0, 2, 77]))]),
        )
        .expect("RTM_GET");
        assert_eq!(recv(so), Err(Errno::EWOULDBLOCK), "filtered out");
        assert_eq!(
            route_ctloutput(PRCO_SETOPT, so, level, ROUTE_TABLEFILTER, Some(opt(9))),
            Err(Errno::ENOENT)
        );
        assert_eq!(
            route_ctloutput(PRCO_SETOPT, so, level, ROUTE_PRIOFILTER, Some(opt(64))),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            route_ctloutput(PRCO_SETOPT, so, level, ROUTE_FLAGFILTER, None),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            route_ctloutput(PRCO_SETOPT, so, level, 99, Some(opt(0))),
            Err(Errno::ENOPROTOOPT)
        );
        assert_eq!(
            route_ctloutput(PRCO_SETOPT, so, 0, ROUTE_MSGFILTER, None),
            Err(Errno::EINVAL)
        );

        soclose(so, 0).expect("close");
        assert_eq!(RTPTABLE.rtp_count.load(Ordering::Relaxed), 0);

        unconfigure(ifp, [10, 0, 2, 15]);
        teardown();
    }
}
/* </TESTS> */
