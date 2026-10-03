/*	$OpenBSD: rtsock.c,v 1.391 2026/04/17 18:30:45 claudio Exp $	*/
/*	$NetBSD: rtsock.c,v 1.18 1996/03/29 00:32:10 cgd Exp $	*/
/* <LICENSES> */
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

//! The routing socket: the `routedomain`, its protocol table and the messages the kernel
//! announces to routing sockets (`rtm_send`, `rtm_miss`, `rtm_ifchg`, `rtm_addr`, ...).
//!
//! Upstream: sys/net/rtsock.c @ 3ce1f3f79392
//!
//! Every announcement builds a routing message in an mbuf (`rtm_msg1`) and hands it to
//! `route_input`, which copies it to each listening routing socket. When no routing socket
//! exists (`rtptable.rtp_count == 0`) the announcements return at once, as they do here
//! until sockets are ported: nothing can attach to the table yet.
//!
//! Status: `wip` (M7b): what `net/route.c`, `net/if.c` and `netinet/` need without sockets.
//!
//! ## Deviations
//! - Sockets (`<sys/socketvar.h>`) are not ported. `struct rtpcb`'s `rop_socket` is an opaque
//!   pointer and the table of routing control blocks is always empty; `route_input` keeps the
//!   C's walk over it, but delivering to a socket (the filters and `rtm_sendup`) reports
//!   itself with `unported!`. The socket side of the file is not here: `route_attach`,
//!   `route_detach`, `route_disconnect`, `route_shutdown`, `route_rcvd`, `route_send`,
//!   `route_sockaddr`, `route_peeraddr`, `route_ctloutput`, `route_output`, `rtm_output`,
//!   `rtm_getifa`, `ifa_ifwithroute`, `route_cleargateway`, `route_arp_conflict`,
//!   `rtm_senddesync(_timer)`, `rtm_sendup`, `rtm_report`, `rtm_msg2`, `rtm_xaddrs`,
//!   `rtm_validate_proposal`, `rtm_setmetrics`, `rtm_getmetrics`, `rt_setsource` and the
//!   `route_usrreqs` table. `sysctl_rtable` (the `net.route` sysctl: route dumps, interface
//!   lists, `rtstat`) reports itself until `rtm_msg2` and `struct walkarg` are ported.
//! - `routesw[]` has no `pr_ctloutput`/`pr_usrreqs` (`sys/protosw.rs`).
//! - The `rtm_*` functions take `&mut RtAddrinfo` where `rtm_msg1` records the addresses it
//!   copied (`rti_addrs`) and are `unsafe fn`s where the info's raw socket addresses are read.
//! - `BFD` is not configured: `rtm_bfd` and `RTM_BFD`'s header size are comments.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::kern::kern_rwlock::{rw_enter_read, rw_exit_read, rw_init};
use crate::kern::subr_pool::pool_init;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_copyback, m_free, m_freem, m_gethdr};
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_::{
    IFNAMSIZ, IfAnnouncemsghdr, IfIeee80211Data, IfIeee80211Msghdr, IfMsghdr, IfaMsghdr, if_get,
    if_getdata, if_put,
};
use crate::net::if_dl::sdltosa;
use crate::net::if_var::{Ifaddr, Ifnet};
use crate::net::route::{
    RTAX_DNS, RTAX_DST, RTAX_GATEWAY, RTAX_IFA, RTAX_IFP, RTAX_LABEL, RTAX_MAX, RTAX_NETMASK,
    RTF_DONE, RTF_HOST, RTM_80211INFO, RTM_DELADDR, RTM_IFANNOUNCE, RTM_IFINFO, RTM_NEWADDR,
    RTM_PROPOSAL, RTM_VERSION, RtAddrinfo, RtMsghdr, Rtentry, SockaddrRtlabel, route_init,
    rt_plen2mask, rtlabel_id2sa,
};
use crate::net::rtable::{rt_key, rtable_getsource};
use crate::queue_adapter;
use crate::sys::domain::Domain;
use crate::sys::errno::Errno;
use crate::sys::mbuf::{M_DONTWAIT, M_EXT, MCLBYTES, MHLEN, MT_DATA, Mbuf, mclget, mtod};
use crate::sys::pool::{PR_WAITOK, Pool};
use crate::sys::protosw::{PR_ADDR, PR_ATOMIC, PR_WANTRCVD, Protosw};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::socket::{AF_UNSPEC, PF_ROUTE, SOCK_RAW, Sockaddr, SockaddrStorage};
use crate::sys::timeout::Timeout;
use crate::sys::types::SaFamily;
use crate::unported;

/// `ROUTESNDQ`.
pub const ROUTESNDQ: usize = 8192;
/// `ROUTERCVQ`.
pub const ROUTERCVQ: usize = 8192;

// These flags and timeout are used for indicating to userland (via a RTM_DESYNC msg) when the
// route socket has overflowed and messages have been lost.

/// Route socket out of memory.
pub const ROUTECB_FLAG_DESYNC: u32 = 0x1;
/// Wait until socket is empty before queueing more packets.
pub const ROUTECB_FLAG_FLUSH: u32 = 0x2;

/// `ROUTE_DESYNC_RESEND_TIMEOUT`: in ms.
pub const ROUTE_DESYNC_RESEND_TIMEOUT: u64 = 200;

/// `struct rtpcb`: a routing socket's control block. The locks are the C's: \[I\] immutable
/// after creation, \[s\] solock.
pub struct Rtpcb {
    /// \[I\] `rop_socket`: the socket (`struct socket *`, opaque until sockets are ported).
    pub rop_socket: *const c_void,
    /// `rop_list`.
    pub rop_list: TailqEntry<Rtpcb>,
    /// `rop_timeout`.
    pub rop_timeout: Timeout,
    /// \[s\] `rop_msgfilter`.
    pub rop_msgfilter: u32,
    /// \[s\] `rop_flagfilter`.
    pub rop_flagfilter: u32,
    /// \[s\] `rop_flags`.
    pub rop_flags: u32,
    /// \[s\] `rop_rtableid`.
    pub rop_rtableid: u32,
    /// \[I\] `rop_proto`.
    pub rop_proto: u16,
    /// \[s\] `rop_priority`.
    pub rop_priority: u8,
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

// SAFETY: the list changes under `rtp_lk`; the count is atomic.
unsafe impl Sync for Rtptable {}

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

/// `routesw[]`: the protocols of the route domain.
pub static ROUTESW: [Protosw; 1] = [Protosw {
    pr_type: SOCK_RAW as i16,
    pr_flags: PR_ATOMIC | PR_ADDR | PR_WANTRCVD,
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

/// `ROUNDUP(a)`: `a` rounded up to a multiple of `sizeof(long)`, `sizeof(long)` for 0.
const fn roundup_long(a: usize) -> usize {
    if a > 0 {
        1 + ((a - 1) | (size_of::<u64>() - 1))
    } else {
        size_of::<u64>()
    }
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

/// `route_input`: hands routing message `m0` to every routing socket that wants it (all
/// sockets bound to `sa_family`, or every one for `AF_UNSPEC`), then frees it.
pub fn route_input(m0: &'static Mbuf, _so0: *const c_void, _sa_family: SaFamily) {
    let m = m0;

    // ensure that we can access the rtm_type via mtod()
    if (m.m_len().get() as usize) < core::mem::offset_of!(RtMsghdr, rtm_type) + 1 {
        m_freem(m);
        return;
    }

    rw_enter_read(&RTPTABLE.rtp_lk);
    for _rop in RTPTABLE.rtp_list.iter() {
        // The family, message, flag, table and priority filters, the flush flag and
        // rtm_sendup all work on the rop's struct socket: the socket layer is not ported, and
        // nothing can put a control block on this list yet.
        let _ = unported!("route_input: routing sockets (struct socket)");
    }
    rw_exit_read(&RTPTABLE.rtp_lk);

    m_freem(m);
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
        len += roundup_long(usize::from(unsafe { (*sa).sa_len }));
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
        let salen = usize::from(unsafe { (*sa).sa_len });
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
        let family = unsafe { (*info.rti_info[RTAX_DST]).sa_family };
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
    let family = if sa.is_null() {
        AF_UNSPEC
    } else {
        // SAFETY: the caller's contract.
        unsafe { (*sa).sa_family }
    };
    route_input(m, ptr::null(), family);
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
    // SAFETY: `rtm_msg1` made a message with a `struct if_msghdr`, aligned (a fresh mbuf).
    let ifm = unsafe { &mut *mtod::<IfMsghdr>(m) };
    ifm.ifm_index = ifp.if_index.get() as u16;
    ifm.ifm_tableid = ifp.if_rdomain.get() as u16;
    ifm.ifm_flags = ifp.if_flags.get();
    ifm.ifm_xflags = ifp.if_xflags.get();
    if_getdata(ifp, &mut ifm.ifm_data);
    ifm.ifm_addrs = info.rti_addrs;
    route_input(m, ptr::null(), AF_UNSPEC);
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
    info.rti_info[crate::net::route::RTAX_BRD] = ifa.ifa_dstaddr.get();
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

    let addr = ifa.ifa_addr.get();
    let family = if addr.is_null() {
        AF_UNSPEC
    } else {
        // SAFETY: an interface address's `ifa_addr` is a readable socket address.
        unsafe { (*addr).sa_family }
    };
    route_input(m, ptr::null(), family);
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
    route_input(m, ptr::null(), AF_UNSPEC);
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
    route_input(m, ptr::null(), AF_UNSPEC);
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
    route_input(m, ptr::null(), family);
}

/// `sysctl_rtable`: the `net.route` sysctl (route dumps, interface lists, `rtstat`, the
/// table ids); not ported yet (see the module's deviations).
pub fn sysctl_rtable(
    _name: &[i32],
    _oldp: usize,
    _oldlenp: &mut usize,
    _newp: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    Err(unported!(
        "sysctl_rtable (net/rtsock.c: rtm_msg2, struct walkarg)"
    ))
}
