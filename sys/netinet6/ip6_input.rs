/*	$OpenBSD: ip6_input.c,v 1.301 2026/09/17 15:56:59 bluhm Exp $	*/
/*	$KAME: ip6_input.c,v 1.188 2001/03/29 05:34:31 itojun Exp $	*/
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
 * Copyright (c) 1982, 1986, 1988, 1993
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
 *	@(#)ip_input.c	8.2 (Berkeley) 1/4/94
 */
/* </LICENSES> */

/* <CODE> */
//! IPv6 input: header checks, the hop-by-hop options, local delivery through the
//! protocol switch, forwarding, extension header walking, the control messages
//! of received packets and the `net.inet6.ip6` sysctls: `netinet6/ip6_input.c`.
//!
//! Upstream: sys/netinet6/ip6_input.c @ 3ce1f3f79392
//!
//! `ipv6_input` is the entry point from the link layer (`ether_input`, `if_input_local`):
//! `ip6_input_if` checks the header (`ipv6_check`), runs pf(4), embeds the scope of
//! link-local addresses, rejects type 0 routing headers, and then delivers the packet when
//! it is for us (a multicast group joined on the interface, or a local route), or forwards it
//! (`ip6_forward`). Local delivery processes the hop-by-hop options and trims the packet to
//! its payload length (`ip6_hbhchcheck`), then walks the protocol switch with
//! `netinet/ip_input.rs`'s `ip_deliver`, queueing on `ip6intrq` for `ip6intr` the protocols
//! that need the exclusive net lock.
//!
//! ## Deviations
//! - `ip6counters` (`struct cpumem *`) is the static array of atomics [`IP6COUNTERS`]
//!   (`netinet6/ip6_var.rs`'s `ip6stat_inc` bumps it); `counters_alloc` in `ip6_init` has
//!   nothing to do. `ip6_sysctl_ip6stat` copies it into a heap buffer as the C's `malloc`.
//! - The IPv6 header in the packet is read and written as a copy (`ip6_var.rs`'s `mtod_ip6`/
//!   `mtod_ip6_store`): mbuf data has no alignment guarantee. The extension headers are read
//!   unaligned or byte by byte. The C's `goto bad`/`goto out` are labelled blocks.
//! - The C's `struct mbuf **` is `&mut Option<&'static Mbuf>`: when a helper consumed the
//!   packet (`icmp6_error`, a failed `ip6_exthdr_get`), `*mp` is `None` (the C leaves a
//!   dangling pointer its callers do not touch). `ip6_hbhchcheck`'s `int *oursp` is an
//!   `Option<&mut bool>`.
//! - `ip6_unknown_opt` returns `true` where the C returns the option length to skip (the
//!   option's length byte, which the callers read themselves) and `false` for the C's -1.
//!   `ip6_nexthdr` and `ip6_lasthdr` return `None` for the C's -1; their `nxtp` is never
//!   NULL.
//! - `ip6_savecontrol` appends to the control chain through the last message's `m_next`, as
//!   `ip_savecontrol` does; the values are handed to `sbcreatecontrol` as their bytes.
//! - `NPF` (pf(4)) is configured: `pf_test(AF_INET6, PF_IN, ..)` filters every packet and
//!   `pf_ouraddr` answers for the addresses pf redirected. `IPSEC` is configured:
//!   `ipsec_forward_check(.., AF_INET6)` checks forwarded packets.
//! - Not configured, each a comment at its site: `NCARP` (`carp_lsdrop`,
//!   `carp_strict_addr_chk`) and `MROUTING` (`ip6_mforward`, `ip6_mrouter_active`,
//!   `mrt6_init`; the `mrt` sysctls answer `EOPNOTSUPP` as the C's `#else` does).
//! - The `SMALL_KERNEL` variant of `ip6_sysctl` is not built (the kernel is `GENERIC`).

use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use alloc::vec;

use crate::kassert;
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write};
use crate::kern::kern_sysctl::{
    SYSCTL_LOCK, sysctl_bounded_arr, sysctl_int_bounded, sysctl_rdstruct,
};
use crate::kern::kern_task::task_add;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_domain::pffindproto;
use crate::kern::uipc_mbuf::{
    m_adj, m_copydata, m_free, m_freem, m_get, m_microtime, m_pullup, m_trailingspace, ml_dequeue,
    mq_delist, mq_enqueue, mq_init,
};
use crate::kern::uipc_mbuf2::{m_tag_delete, m_tag_find, m_tag_get, m_tag_prepend};
use crate::kern::uipc_socket2::sbcreatecontrol;
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_::{IFF_LOOPBACK, net_tq, niq_enqueue};
use crate::net::if_types::IFT_ENC;
use crate::net::if_var::{Ifnet, Netstack, Niqueue, niq_dequeue, sysctl_niq};
use crate::net::netisr::NETISR_IPV6;
use crate::net::pf::{pf_ouraddr, pf_test};
use crate::net::pfvar::{PF_IN, PF_PASS};
use crate::net::route::{
    RTF_LOCAL, RTGENERATION, Route, route6_mpath, rt_timer_queue_change, rtfree,
};
use crate::netinet::icmp6::{ICMP6_PARAM_PROB, ICMP6_PARAMPROB_HEADER, ICMP6_PARAMPROB_OPTION};
use crate::netinet::in_::{
    IPPROTO_AH, IPPROTO_DONE, IPPROTO_DSTOPTS, IPPROTO_ESP, IPPROTO_FRAGMENT, IPPROTO_HOPOPTS,
    IPPROTO_IPCOMP, IPPROTO_IPV6, IPPROTO_MAX, IPPROTO_NONE, IPPROTO_RAW, IPPROTO_ROUTING,
};
use crate::netinet::in_pcb::{
    IN6P_DSTOPTS, IN6P_HOPLIMIT, IN6P_HOPOPTS, IN6P_PKTINFO, IN6P_RTHDR, IN6P_TCLASS, Inpcb,
};
use crate::netinet::ip::IPQ_MAXLEN;
use crate::netinet::ip_input::ip_deliver;
use crate::netinet::ip_ipsp::IPSEC_IN_USE;
use crate::netinet::ip_var::Ipoffnxt;
use crate::netinet::ip6::{
    IP6F_OFF_MASK, IP6OPT_JUMBO, IP6OPT_JUMBO_LEN, IP6OPT_MINLEN, IP6OPT_PAD1, IP6OPT_PADN,
    IP6OPT_ROUTER_ALERT, IP6OPT_RTALERT_LEN, IP6OPT_TYPE_DISCARD, IP6OPT_TYPE_FORCEICMP,
    IP6OPT_TYPE_ICMP, IP6OPT_TYPE_SKIP, IPV6_FLOWINFO_MASK, IPV6_MAXPACKET, IPV6_VERSION,
    IPV6_VERSION_MASK, Ip6Ext, Ip6Frag, Ip6Hbh, Ip6Hdr, Ip6Rthdr, ip6_exthdr_get, ip6opt_type,
};
use crate::netinet::ipsec_input::ipsec_forward_check;
use crate::netinet6::frag6::frag6_init;
use crate::netinet6::icmp6::{ICMP6_MTUDISC_TIMEOUT_Q, icmp6_error};
use crate::netinet6::in6::{
    IPV6_DSTOPTS, IPV6_HOPLIMIT, IPV6_HOPOPTS, IPV6_PKTINFO, IPV6_RTHDR, IPV6_RTHDR_TYPE_0,
    IPV6_TCLASS, IPV6CTL_DAD_COUNT, IPV6CTL_DAD_PENDING, IPV6CTL_DEFHLIM, IPV6CTL_DEFMCASTHLIM,
    IPV6CTL_FORWARDING, IPV6CTL_HDRNESTLIMIT, IPV6CTL_IFQUEUE, IPV6CTL_MAXDYNROUTES,
    IPV6CTL_MAXFRAGPACKETS, IPV6CTL_MAXFRAGS, IPV6CTL_MCAST_PMTU, IPV6CTL_MFORWARDING,
    IPV6CTL_MRTMFC, IPV6CTL_MRTMIF, IPV6CTL_MRTPROTO, IPV6CTL_MRTSTATS, IPV6CTL_MTUDISCTIMEOUT,
    IPV6CTL_MULTIPATH, IPV6CTL_NEIGHBORGCTHRESH, IPV6CTL_SENDREDIRECTS, IPV6CTL_STATS, In6Pktinfo,
    ifatoia6, in6_are_addr_equal, in6_hasmulti, in6_is_addr_loopback, in6_is_addr_mc_intfacelocal,
    in6_is_addr_mc_linklocal, in6_is_addr_multicast, in6_is_addr_unspecified, in6_is_addr_v4compat,
    in6_is_addr_v4mapped, in6_is_scope_embed,
};
use crate::netinet6::in6_proto::{
    INET6DOMAIN, INET6SW, IP6_DAD_COUNT, IP6_DAD_PENDING, IP6_DEFHLIM, IP6_DEFMCASTHLIM,
    IP6_FORWARDING, IP6_HDRNESTLIMIT, IP6_MAXDYNROUTES, IP6_MAXFRAGPACKETS, IP6_MAXFRAGS,
    IP6_MCAST_PMTU, IP6_MFORWARDING, IP6_MTUDISC_TIMEOUT, IP6_MULTIPATH, IP6_NEIGHBORGCTHRESH,
    IP6_PROTOX, IP6_SENDREDIRECTS,
};
use crate::netinet6::in6_var::{IN6_IFF_DUPLICATED, IN6_IFF_TENTATIVE};
use crate::netinet6::ip6_forward::ip6_forward;
use crate::netinet6::ip6_output::{ip6_output, ip6_randomid_init};
use crate::netinet6::ip6_var::{
    IP6S_NCOUNTERS, IPV6_FORWARDING, IPV6_FORWARDING_IPSEC, IPV6_REDIRECT, Ip6stat,
    Ip6statCounters, ip6stat_inc, ip6stat_inc_idx, mtod_ip6, mtod_ip6_store,
};
use crate::netinet6::nd6::nd6_init;
use crate::sys::endian::{htons, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::limits::INT_MAX;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::mbuf::{
    M_BCAST, M_DONTWAIT, M_EXT, M_LOOP, M_MCAST, M_PKTHDR, MLEN, MT_DATA, Mbuf, MbufList,
    MbufQueue, PACKET_TAG_IP6_OFFNXT, PF_TAG_PROCESSED, PF_TAG_TRANSLATE_LOCALHOST, m_freemp,
    mclget, ml_empty, mtod,
};
use crate::sys::protosw::{PRC_NCMDS, Protosw};
use crate::sys::socket::{
    AF_INET6, AF_UNSPEC, PF_INET6, SCM_TIMESTAMP, SO_TIMESTAMP, SOCK_RAW, SOL_SOCKET,
};
use crate::sys::sysctl::SysctlBoundedArgs;
use crate::sys::systm::{net_lock_shared, net_unlock_shared};
use crate::sys::task::Task;

/// The size of the IPv6 header, as the C's `sizeof(struct ip6_hdr)` in offset arithmetic.
const IP6_HDR_LEN: i32 = size_of::<Ip6Hdr>() as i32;

/// `ip6intrq`.
pub static IP6INTRQ: Niqueue = Niqueue::new(IPQ_MAXLEN as u32, NETISR_IPV6);

/// `ip6counters`: the IPv6 statistics.
pub static IP6COUNTERS: [AtomicU64; IP6S_NCOUNTERS] = [const { AtomicU64::new(0) }; IP6S_NCOUNTERS];

/// `ip6send_mq`: the packets `ip6_send` queued for `ip6_send_dispatch`.
static IP6SEND_MQ: MbufQueue = MbufQueue::new(64, IPL_SOFTNET);

/// `ip6send_task`.
static IP6SEND_TASK: Task = Task::new(ip6_send_dispatch, (&raw const IP6SEND_MQ).cast_mut().cast());

/// `inet6ctlerrmap[]`: the errno of each `PRC_*` control command.
pub static INET6CTLERRMAP: [Option<Errno>; PRC_NCMDS] = [
    None,
    None,
    None,
    None,
    None,
    Some(Errno::EMSGSIZE),
    Some(Errno::EHOSTDOWN),
    Some(Errno::EHOSTUNREACH),
    Some(Errno::EHOSTUNREACH),
    Some(Errno::EHOSTUNREACH),
    Some(Errno::ECONNREFUSED),
    Some(Errno::ECONNREFUSED),
    Some(Errno::EMSGSIZE),
    Some(Errno::EHOSTUNREACH),
    None,
    None,
    None,
    None,
    None,
    None,
    Some(Errno::ENOPROTOOPT),
];

/// `ipv6ctl_vars[]`: the bounded integer sysctls of `net.inet6.ip6`.
static IPV6CTL_VARS: [SysctlBoundedArgs; 13] = [
    SysctlBoundedArgs::new(IPV6CTL_FORWARDING, &IP6_FORWARDING, 0, 2),
    SysctlBoundedArgs::new(IPV6CTL_SENDREDIRECTS, &IP6_SENDREDIRECTS, 0, 1),
    SysctlBoundedArgs::readonly(IPV6CTL_DAD_PENDING, &IP6_DAD_PENDING),
    // MROUTING: IPV6CTL_MRTPROTO, read only (ip6_mrtproto); not configured.
    SysctlBoundedArgs::new(IPV6CTL_DEFHLIM, &IP6_DEFHLIM, 0, 255),
    SysctlBoundedArgs::new(IPV6CTL_MAXFRAGPACKETS, &IP6_MAXFRAGPACKETS, 0, 1000),
    SysctlBoundedArgs::new(IPV6CTL_HDRNESTLIMIT, &IP6_HDRNESTLIMIT, 0, 100),
    SysctlBoundedArgs::new(IPV6CTL_DAD_COUNT, &IP6_DAD_COUNT, 0, 10),
    SysctlBoundedArgs::new(IPV6CTL_DEFMCASTHLIM, &IP6_DEFMCASTHLIM, 0, 255),
    SysctlBoundedArgs::new(IPV6CTL_MAXFRAGS, &IP6_MAXFRAGS, 0, 1000),
    SysctlBoundedArgs::new(IPV6CTL_MFORWARDING, &IP6_MFORWARDING, 0, 1),
    SysctlBoundedArgs::new(IPV6CTL_MCAST_PMTU, &IP6_MCAST_PMTU, 0, 1),
    SysctlBoundedArgs::new(IPV6CTL_NEIGHBORGCTHRESH, &IP6_NEIGHBORGCTHRESH, 0, 5 * 2048),
    SysctlBoundedArgs::new(IPV6CTL_MAXDYNROUTES, &IP6_MAXDYNROUTES, 0, 5 * 4096),
];

/// The index of `pr` in `inet6sw[]` (`pr - inet6sw`).
fn inet6sw_index(pr: &Protosw) -> u8 {
    match INET6SW.iter().position(|p| ptr::eq(p, pr)) {
        Some(i) => i as u8,
        None => panic(format_args!("ip6_init: protocol not in inet6sw")),
    }
}

/// `ip6_init`: IP6 initialization: fills in the IP6 protocol switch table; all protocols
/// not implemented in kernel go to the raw IP6 protocol handler.
pub fn ip6_init() {
    let Some(pr) = pffindproto(i32::from(PF_INET6), IPPROTO_RAW, SOCK_RAW) else {
        panic(format_args!("ip6_init"));
    };
    let raw = inet6sw_index(pr);
    for p in &IP6_PROTOX {
        p.store(raw, Ordering::Relaxed);
    }
    for pr in INET6DOMAIN.dom_protosw {
        if pr.pr_domain.dom_family == i32::from(PF_INET6)
            && pr.pr_protocol != 0
            && i32::from(pr.pr_protocol) != IPPROTO_RAW
            && i32::from(pr.pr_protocol) < IPPROTO_MAX
        {
            IP6_PROTOX[pr.pr_protocol as usize].store(inet6sw_index(pr), Ordering::Relaxed);
        }
    }
    ip6_randomid_init();
    nd6_init();
    frag6_init();

    mq_init(&IP6SEND_MQ, 64, IPL_SOFTNET);

    // ip6counters = counters_alloc(ip6s_ncounters): a static (the module's deviations).
    // MROUTING: mrt6_init(); not configured.
}

/// `ip6_ours`: enqueue packet for local delivery. Queuing is used as a boundary between the
/// network layer (input/forward path) running with `NET_LOCK_SHARED()` and the transport
/// layer needing it exclusively.
fn ip6_ours(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    nxt: i32,
    af: i32,
    flags: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let mut nxt = nxt;

    // ip6_hbhchcheck() may be run before, then off and nxt are set
    if *offp == 0 {
        nxt = ip6_hbhchcheck(mp, offp, None, flags);
        if nxt == IPPROTO_DONE {
            return IPPROTO_DONE;
        }
    }

    // We are already in a IPv4/IPv6 local deliver loop.
    if af != i32::from(AF_UNSPEC) {
        return nxt;
    }

    let nxt = ip_deliver(mp, offp, nxt, i32::from(AF_INET6), true, ns);
    if nxt == IPPROTO_DONE {
        return IPPROTO_DONE;
    }

    ip6_ours_enqueue(mp, offp, nxt)
}

/// `ip6_ours_enqueue`: queues a packet for `ip6intr` (protocols that need the exclusive
/// net lock), with its offset and next header in a tag when extension headers were
/// processed; returns `IPPROTO_DONE`.
pub fn ip6_ours_enqueue(mp: &mut Option<&'static Mbuf>, offp: &mut i32, nxt: i32) -> i32 {
    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };

    // save values for later, use after dequeue
    if *offp != IP6_HDR_LEN {
        // mbuf tags are expensive, but only used for header options
        let Some(mtag) = m_tag_get(
            PACKET_TAG_IP6_OFFNXT,
            size_of::<Ipoffnxt>() as i32,
            M_NOWAIT,
        ) else {
            ip6stat_inc(Ip6statCounters::Ip6sIdropped);
            m_freemp(mp);
            return IPPROTO_DONE;
        };
        let ion = Ipoffnxt {
            ion_off: *offp,
            ion_nxt: nxt,
        };
        // SAFETY: the tag has `size_of::<Ipoffnxt>()` bytes of data.
        unsafe { ptr::write_unaligned(mtag.data().cast::<Ipoffnxt>(), ion) };

        m_tag_prepend(m, mtag);
    }

    niq_enqueue(&IP6INTRQ, m);
    *mp = None;
    IPPROTO_DONE
}

/// `ip6intr`: the IPv6 software interrupt: dequeues and processes locally delivered
/// packets. This is called with exclusive `NET_LOCK()`.
pub fn ip6intr() {
    while let Some(m) = niq_dequeue(&IP6INTRQ) {
        #[cfg(feature = "diagnostic")]
        if m.m_flags().get() & M_PKTHDR == 0 {
            panic(format_args!("ip6intr no HDR"));
        }
        let (mut off, nxt);
        if let Some(mtag) = m_tag_find(m, PACKET_TAG_IP6_OFFNXT, None) {
            // SAFETY: `ip6_ours_enqueue` wrote an `Ipoffnxt` into the tag's data.
            let ion = unsafe { ptr::read_unaligned(mtag.data().cast::<Ipoffnxt>()) };
            off = ion.ion_off;
            nxt = ion.ion_nxt;

            // SAFETY: the tag is on this packet's list.
            unsafe { m_tag_delete(m, mtag) };
        } else {
            let ip6 = mtod_ip6(m);
            off = IP6_HDR_LEN;
            nxt = i32::from(ip6.ip6_nxt);
        }

        let mut mp = Some(m);
        let nxt = ip_deliver(&mut mp, &mut off, nxt, i32::from(AF_INET6), false, None);
        kassert!(nxt == IPPROTO_DONE);
        let _ = nxt;
    }
}

/// `ipv6_input`: an IPv6 packet from `ifp` (`ether_input`, `if_input_local`).
pub fn ipv6_input(ifp: &'static Ifnet, m: &'static Mbuf, ns: Option<&Netstack>) {
    let mut off = 0;
    let mut mp = Some(m);
    let nxt = ip6_input_if(
        &mut mp,
        &mut off,
        IPPROTO_IPV6,
        i32::from(AF_UNSPEC),
        ifp,
        ns,
    );
    kassert!(nxt == IPPROTO_DONE);
    let _ = nxt;
}

/// `ipv6_check`: the header checks of `ipv6_input`: version, address spoofing and scope
/// violations; `None` when the packet was dropped (and freed).
pub fn ipv6_check(ifp: &Ifnet, m: &'static Mbuf) -> Option<&'static Mbuf> {
    let mut m = m;

    if (m.m_len().get() as usize) < size_of::<Ip6Hdr>() {
        let Some(mm) = m_pullup(m, IP6_HDR_LEN) else {
            ip6stat_inc(Ip6statCounters::Ip6sToosmall);
            return None;
        };
        m = mm;
    }

    let ip6 = mtod_ip6(m);
    let loopback = ifp.if_flags.get() & IFF_LOOPBACK != 0;

    'bad: {
        if ip6.ip6_vfc() & IPV6_VERSION_MASK != IPV6_VERSION {
            ip6stat_inc(Ip6statCounters::Ip6sBadvers);
            break 'bad;
        }

        // Check against address spoofing/corruption.
        if in6_is_addr_multicast(&ip6.ip6_src) || in6_is_addr_unspecified(&ip6.ip6_dst) {
            // XXX: "badscope" is not very suitable for a multicast source.
            ip6stat_inc(Ip6statCounters::Ip6sBadscope);
            break 'bad;
        }
        if (in6_is_addr_loopback(&ip6.ip6_src) || in6_is_addr_loopback(&ip6.ip6_dst)) && !loopback {
            ip6stat_inc(Ip6statCounters::Ip6sBadscope);
            break 'bad;
        }
        // Drop packets if interface ID portion is already filled.
        if ((in6_is_scope_embed(&ip6.ip6_src) && ip6.ip6_src.s6_addr16(1) != 0)
            || (in6_is_scope_embed(&ip6.ip6_dst) && ip6.ip6_dst.s6_addr16(1) != 0))
            && !loopback
        {
            ip6stat_inc(Ip6statCounters::Ip6sBadscope);
            break 'bad;
        }
        if in6_is_addr_mc_intfacelocal(&ip6.ip6_dst) && m.m_flags().get() & M_LOOP == 0 {
            // In this case, the packet should come from the loopback interface. However, we
            // cannot just check the if_flags, because ip6_mloopback() passes the "actual"
            // interface as the outgoing/incoming interface.
            ip6stat_inc(Ip6statCounters::Ip6sBadscope);
            break 'bad;
        }

        // The following check is not documented in specs. A malicious party may be able to
        // use IPv4 mapped addr to confuse tcp/udp stack and bypass security checks (act as
        // if it was from 127.0.0.1 by using IPv6 src ::ffff:127.0.0.1). Be cautious.
        //
        // This check chokes if we are in an SIIT cloud. As none of BSDs support IPv4-less
        // kernel compilation, we cannot support SIIT environment at all. So, it makes more
        // sense for us to reject any malicious packets for non-SIIT environment, than try to
        // do a partial support for SIIT environment.
        if in6_is_addr_v4mapped(&ip6.ip6_src) || in6_is_addr_v4mapped(&ip6.ip6_dst) {
            ip6stat_inc(Ip6statCounters::Ip6sBadscope);
            break 'bad;
        }

        // Reject packets with IPv4 compatible addresses (auto tunnel).
        //
        // The code forbids automatic tunneling as per RFC4213.
        if in6_is_addr_v4compat(&ip6.ip6_src) || in6_is_addr_v4compat(&ip6.ip6_dst) {
            ip6stat_inc(Ip6statCounters::Ip6sBadscope);
            break 'bad;
        }

        return Some(m);
    }
    // bad:
    m_freem(m);
    None
}

/// `ip6_input_if`: processes the IPv6 packet `*mp` received on `ifp` (`nxt` is
/// `IPPROTO_IPV6`, `af` `AF_INET6` or `AF_UNSPEC` for a forwarded one); returns the next
/// protocol or `IPPROTO_DONE`, `*mp` cleared when the packet was consumed.
pub fn ip6_input_if(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    nxt: i32,
    af: i32,
    ifp: &'static Ifnet,
    ns: Option<&Netstack>,
) -> i32 {
    let iproute = Route::new();
    let mut nxt = nxt;
    let mut src_scope: u16 = 0;
    let mut dst_scope: u16 = 0;
    let mut flags = 0;
    let loopback = ifp.if_flags.get() & IFF_LOOPBACK != 0;

    kassert!(*offp == 0);

    ip6stat_inc(Ip6statCounters::Ip6sTotal);
    'out: {
        'bad: {
            let Some(m0) = *mp else {
                break 'bad;
            };
            *mp = ipv6_check(ifp, m0);
            let Some(m) = *mp else {
                break 'bad;
            };

            let mut ip6 = mtod_ip6(m);

            // NCARP > 0: carp_lsdrop; not configured.
            ip6stat_inc_idx(Ip6statCounters::Ip6sNxthist, usize::from(ip6.ip6_nxt));

            // If the packet has been received on a loopback interface it can be destined to
            // any local address, not necessarily to an address configured on `ifp'.
            if loopback {
                if in6_is_scope_embed(&ip6.ip6_src) {
                    src_scope = ip6.ip6_src.s6_addr16(1);
                    ip6.ip6_src.set_s6_addr16(1, 0);
                }
                if in6_is_scope_embed(&ip6.ip6_dst) {
                    dst_scope = ip6.ip6_dst.s6_addr16(1);
                    ip6.ip6_dst.set_s6_addr16(1, 0);
                }
                mtod_ip6_store(m, &ip6);
            }

            // Packet filter
            let odst = ip6.ip6_dst;
            if pf_test(AF_INET6, PF_IN, ifp, mp) != PF_PASS {
                break 'bad;
            }
            let Some(m) = *mp else {
                break 'bad;
            };

            ip6 = mtod_ip6(m);
            if !in6_are_addr_equal(&odst, &ip6.ip6_dst) {
                flags |= IPV6_REDIRECT;
            }

            match IP6_FORWARDING.load(Ordering::Relaxed) {
                2 => flags |= IPV6_FORWARDING_IPSEC | IPV6_FORWARDING,
                1 => flags |= IPV6_FORWARDING,
                _ => {}
            }

            // Without embedded scope ID we cannot find link-local addresses in the routing
            // table.
            if loopback {
                if in6_is_scope_embed(&ip6.ip6_src) {
                    ip6.ip6_src.set_s6_addr16(1, src_scope);
                }
                if in6_is_scope_embed(&ip6.ip6_dst) {
                    ip6.ip6_dst.set_s6_addr16(1, dst_scope);
                }
            } else {
                let idx = htons(ifp.if_index.get() as u16);
                if in6_is_scope_embed(&ip6.ip6_src) {
                    ip6.ip6_src.set_s6_addr16(1, idx);
                }
                if in6_is_scope_embed(&ip6.ip6_dst) {
                    ip6.ip6_dst.set_s6_addr16(1, idx);
                }
            }
            mtod_ip6_store(m, &ip6);

            // Be more secure than RFC5095 and scan for type 0 routing headers. If pf has
            // already scanned the header chain, do not do it twice.
            if m.m_pkthdr().pf.flags.get() & PF_TAG_PROCESSED == 0 && ip6_check_rh0hdr(m, offp) {
                ip6stat_inc(Ip6statCounters::Ip6sBadoptions);
                icmp6_error(m, ICMP6_PARAM_PROB, ICMP6_PARAMPROB_HEADER, *offp);
                *mp = None;
                break 'bad;
            }

            if pf_ouraddr(m) == 1 {
                nxt = ip6_ours(mp, offp, nxt, af, flags, ns);
                break 'out;
            }

            // Multicast check
            if in6_is_addr_multicast(&ip6.ip6_dst) {
                // Make sure M_MCAST is set. It should theoretically already be there, but
                // let's play safe because upper layers check for this flag.
                m.m_flags().set(m.m_flags().get() | M_MCAST);

                // See if we belong to the destination multicast group on the arrival
                // interface.
                let ours = in6_hasmulti(&ip6.ip6_dst, ifp);

                // MROUTING: when ip6_mforwarding and ip6_mrouter_active(ifp->if_rdomain),
                // ip6_hbhchcheck and ip6_mforward (the packet is passed to the kernel-level
                // multicast forwarding function); not configured.

                if !ours {
                    ip6stat_inc(Ip6statCounters::Ip6sNotmember);
                    if !in6_is_addr_mc_linklocal(&ip6.ip6_dst) {
                        ip6stat_inc(Ip6statCounters::Ip6sCantforward);
                    }
                    break 'bad;
                }
                nxt = ip6_ours(mp, offp, nxt, af, flags, ns);
                break 'out;
            }

            // Unicast check
            let ro: &Route = match ns {
                None => &iproute,
                Some(ns) => &ns.ns_route,
            };
            let rt = route6_mpath(
                ro,
                &ip6.ip6_dst,
                Some(&ip6.ip6_src),
                m.m_pkthdr().ph_rtableid.get(),
            );

            // Accept the packet if the route to the destination is marked as local.
            if let Some(rt) = rt
                && rt.rt_flags.get() & RTF_LOCAL != 0
            {
                let ia6 = ifatoia6(rt.ifa());

                if flags & IPV6_FORWARDING == 0
                    && rt.rt_ifidx.get() != ifp.if_index.get()
                    && !(loopback
                        || ifp.if_type.get() == IFT_ENC
                        || m.m_pkthdr().pf.flags.get() & PF_TAG_TRANSLATE_LOCALHOST != 0)
                {
                    // received on wrong interface
                    // NCARP > 0: virtual IPs on carp interfaces are also checked against the
                    // parent interface (carp_strict_addr_chk); not configured.
                    ip6stat_inc(Ip6statCounters::Ip6sWrongif);
                    break 'bad;
                }
                // packets to a tentative, duplicated, or somehow invalid address must not be
                // accepted.
                if ia6.ia6_flags.get() & (IN6_IFF_TENTATIVE | IN6_IFF_DUPLICATED) != 0 {
                    break 'bad;
                }
                nxt = ip6_ours(mp, offp, nxt, af, flags, ns);
                break 'out;
            }

            // NCARP > 0: carp_lsdrop for ICMPv6; not configured.

            // Now there is no reason to process the packet if it's not our own and we're not
            // a router.
            if flags & IPV6_FORWARDING == 0 {
                ip6stat_inc(Ip6statCounters::Ip6sCantforward);
                break 'bad;
            }

            let mut ours = false;
            nxt = ip6_hbhchcheck(mp, offp, Some(&mut ours), flags);
            if nxt == IPPROTO_DONE {
                break 'out;
            }

            if ours {
                if af == i32::from(AF_UNSPEC) {
                    nxt = ip6_ours(mp, offp, nxt, af, flags, ns);
                }
                break 'out;
            }

            let Some(m) = *mp else {
                break 'bad;
            };
            if IPSEC_IN_USE.load(Ordering::Relaxed) != 0
                && ipsec_forward_check(m, *offp, i32::from(AF_INET6)).is_err()
            {
                ip6stat_inc(Ip6statCounters::Ip6sCantforward);
                break 'bad;
            }
            // Fall through, forward packet. Outbound IPsec policy checking will occur in
            // ip6_forward().

            ip6_forward(m, Some(ro), flags);
            *mp = None;
            rtfree(iproute.ro_rt.get());
            return IPPROTO_DONE;
        }
        // bad:
        nxt = IPPROTO_DONE;
        m_freemp(mp);
    }
    // out: `iproute` is the one route this function may have filled.
    rtfree(iproute.ro_rt.get());
    nxt
}

/// `ip6_hbhchcheck`: processes the hop-by-hop options header, if any, and trims the packet
/// to its payload length; returns the next header with `*offp` after the hop-by-hop
/// options. On error frees the mbuf and returns `IPPROTO_DONE`.
fn ip6_hbhchcheck(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    oursp: Option<&mut bool>,
    flags: i32,
) -> i32 {
    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };
    let ip6 = mtod_ip6(m);
    let mut rtalert: u32 = !0;
    let nxt: u8;

    // Process Hop-by-Hop options header if it's contained. m may be modified in
    // ip6_hopopts_input(). If a JumboPayload option is included, plen will also be
    // modified.
    let mut plen = u32::from(ntohs(ip6.ip6_plen));
    *offp = IP6_HDR_LEN;
    if i32::from(ip6.ip6_nxt) == IPPROTO_HOPOPTS {
        if !ip6_hopopts_input(mp, offp, &mut plen, &mut rtalert) {
            return IPPROTO_DONE; // m have already been freed
        }

        // adjust pointer
        let Some(m) = *mp else {
            return IPPROTO_DONE;
        };
        let ip6 = mtod_ip6(m);

        // if the payload length field is 0 and the next header field indicates Hop-by-Hop
        // Options header, then a Jumbo Payload option MUST be included.
        if ip6.ip6_plen == 0 && plen == 0 {
            // Note that if a valid jumbo payload option is contained, ip6_hopopts_input()
            // must set a valid (non-zero) payload length to the variable plen.
            ip6stat_inc(Ip6statCounters::Ip6sBadoptions);
            *mp = None;
            icmp6_error(
                m,
                ICMP6_PARAM_PROB,
                ICMP6_PARAMPROB_HEADER,
                offset_of!(Ip6Hdr, ip6_plen) as i32,
            );
            return IPPROTO_DONE;
        }
        let Some(hbh) = ip6_exthdr_get(mp, IP6_HDR_LEN, size_of::<Ip6Hbh>() as i32) else {
            ip6stat_inc(Ip6statCounters::Ip6sTooshort);
            return IPPROTO_DONE;
        };
        // SAFETY: `ip6_exthdr_get` made the header contiguous at `hbh`.
        nxt = unsafe { ptr::read_unaligned(hbh.cast::<Ip6Hbh>()) }.ip6h_nxt;

        // accept the packet if a router alert option is included and we act as an IPv6
        // router.
        if rtalert != !0
            && flags & IPV6_FORWARDING != 0
            && let Some(ours) = oursp
        {
            *ours = true;
        }
    } else {
        nxt = ip6.ip6_nxt;
    }

    // Check that the amount of data in the buffers is as at least much as the IPv6 header
    // would have us expect. Trim mbufs if longer than we expect. Drop packet if shorter
    // than we expect.
    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };
    let pktlen = i64::from(m.m_pkthdr().len.get());
    let want = i64::from(IP6_HDR_LEN) + i64::from(plen);
    if pktlen - i64::from(IP6_HDR_LEN) < i64::from(plen) {
        ip6stat_inc(Ip6statCounters::Ip6sTooshort);
        m_freemp(mp);
        return IPPROTO_DONE;
    }
    if pktlen > want {
        if i64::from(m.m_len().get()) == pktlen {
            m.m_len().set(want as u32);
            m.m_pkthdr().len.set(want as i32);
        } else {
            m_adj(m, (want - pktlen) as i32);
        }
    }

    i32::from(nxt)
}

/// `ip6_check_rh0hdr`: scans the packet for an RH0 routing header (mostly stolen from pf.c's
/// `pf_test()`); `true` with `*offp` at the offending field when the packet must be
/// rejected.
fn ip6_check_rh0hdr(m: &Mbuf, offp: &mut i32) -> bool {
    let ip6 = mtod_ip6(m);
    let mut proto = i32::from(ip6.ip6_nxt);
    let mut rh_cnt = 0;

    // The header is at the start of the data.
    let mut off = IP6_HDR_LEN;
    let lim = m
        .m_pkthdr()
        .len
        .get()
        .min(i32::from(ntohs(ip6.ip6_plen)) + IP6_HDR_LEN);
    loop {
        match proto {
            IPPROTO_ROUTING => {
                if rh_cnt > 0 {
                    // more than one rh header present
                    *offp = off;
                    return true;
                }
                rh_cnt += 1;

                if off + size_of::<Ip6Rthdr>() as i32 > lim {
                    // packet to short to make sense
                    *offp = off;
                    return true;
                }

                let mut b = [0u8; size_of::<Ip6Rthdr>()];
                m_copydata(m, off, &mut b);

                if i32::from(b[offset_of!(Ip6Rthdr, ip6r_type)]) == IPV6_RTHDR_TYPE_0 {
                    *offp = off + offset_of!(Ip6Rthdr, ip6r_type) as i32;
                    return true;
                }

                off += (i32::from(b[offset_of!(Ip6Rthdr, ip6r_len)]) + 1) * 8;
                proto = i32::from(b[offset_of!(Ip6Rthdr, ip6r_nxt)]);
            }
            IPPROTO_AH | IPPROTO_HOPOPTS | IPPROTO_DSTOPTS => {
                // get next header and header length
                if off + size_of::<Ip6Ext>() as i32 > lim {
                    // Packet to short to make sense, we could reject the packet but as a
                    // router we should not do that so forward it.
                    return false;
                }

                let mut b = [0u8; size_of::<Ip6Ext>()];
                m_copydata(m, off, &mut b);

                let len = i32::from(b[offset_of!(Ip6Ext, ip6e_len)]);
                if proto == IPPROTO_AH {
                    off += (len + 2) * 4;
                } else {
                    off += (len + 1) * 8;
                }
                proto = i32::from(b[offset_of!(Ip6Ext, ip6e_nxt)]);
            }
            // IPPROTO_FRAGMENT and the rest: end of header stack
            _ => return false,
        }
    }
}

/// `ip6_hopopts_input`: Hop-by-Hop options header processing. If a valid jumbo payload
/// option is included, the real payload length will be stored in `plenp`. On error frees
/// the mbuf and returns `false` (the C's -1).
///
/// rtalertp - XXX: should be stored in a more smart way
fn ip6_hopopts_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    plenp: &mut u32,
    rtalertp: &mut u32,
) -> bool {
    let mut off = *offp;

    // validation of the length of the header
    let Some(hbh) = ip6_exthdr_get(mp, IP6_HDR_LEN, size_of::<Ip6Hbh>() as i32) else {
        ip6stat_inc(Ip6statCounters::Ip6sTooshort);
        return false;
    };
    // SAFETY: `ip6_exthdr_get` made the header contiguous at `hbh`.
    let len = unsafe { ptr::read_unaligned(hbh.cast::<Ip6Hbh>()) }.ip6h_len;
    let mut hbhlen = (i32::from(len) + 1) << 3;
    let Some(hbh) = ip6_exthdr_get(mp, IP6_HDR_LEN, hbhlen) else {
        ip6stat_inc(Ip6statCounters::Ip6sTooshort);
        return false;
    };
    off += hbhlen;
    hbhlen -= size_of::<Ip6Hbh>() as i32;

    // SAFETY: the `hbhlen` bytes of options follow the 2-byte header in the contiguous
    // region `ip6_exthdr_get` returned.
    let ok =
        unsafe { ip6_process_hopopts(mp, hbh.add(size_of::<Ip6Hbh>()), hbhlen, rtalertp, plenp) };
    if !ok {
        return false;
    }

    *offp = off;
    true
}

/// `ip6_process_hopopts`: searches the `hbhlen` bytes of hop-by-hop options at `opthead`
/// and processes each option (router alert into `rtalertp`, jumbo payload length into
/// `plenp`); `true` on success (the C's 0), `false` when the packet was dropped or an
/// ICMPv6 error sent (the C's -1, `*mp` cleared).
///
/// This function is separate from `ip6_hopopts_input()` in order to handle a case where
/// the sending node itself process its hop-by-hop options header. In such a case, the
/// function is called from `ip6_output()`. The hop-by-hop header is located right after the
/// IPv6 header (RFC2460 p7).
///
/// # Safety
///
/// `opthead` points at `hbhlen` readable bytes of options inside `*mp`'s first
/// mbuf (contiguous).
pub unsafe fn ip6_process_hopopts(
    mp: &mut Option<&'static Mbuf>,
    opthead: *const u8,
    hbhlen: i32,
    rtalertp: &mut u32,
    plenp: &mut u32,
) -> bool {
    const ERROFF: i32 = IP6_HDR_LEN + size_of::<Ip6Hbh>() as i32;
    let mut hbhlen = hbhlen;
    // SAFETY: the caller guarantees `hbhlen` readable bytes at `opthead`.
    let opt = unsafe { slice::from_raw_parts(opthead, hbhlen.max(0) as usize) };
    let mut i = 0usize;

    while hbhlen > 0 {
        let optlen: i32;
        match opt[i] {
            IP6OPT_PAD1 => optlen = 1,
            IP6OPT_PADN => {
                if hbhlen < IP6OPT_MINLEN as i32 {
                    ip6stat_inc(Ip6statCounters::Ip6sToosmall);
                    m_freemp(mp);
                    return false;
                }
                optlen = i32::from(opt[i + 1]) + 2;
            }
            IP6OPT_ROUTER_ALERT => {
                // XXX may need check for alignment
                if hbhlen < IP6OPT_RTALERT_LEN as i32 {
                    ip6stat_inc(Ip6statCounters::Ip6sToosmall);
                    m_freemp(mp);
                    return false;
                }
                if usize::from(opt[i + 1]) != IP6OPT_RTALERT_LEN - 2 {
                    // XXX stat
                    if let Some(m) = mp.take() {
                        icmp6_error(
                            m,
                            ICMP6_PARAM_PROB,
                            ICMP6_PARAMPROB_HEADER,
                            ERROFF + i as i32 + 1,
                        );
                    }
                    return false;
                }
                optlen = IP6OPT_RTALERT_LEN as i32;
                *rtalertp = u32::from(u16::from_be_bytes([opt[i + 2], opt[i + 3]]));
            }
            IP6OPT_JUMBO => {
                // XXX may need check for alignment
                if hbhlen < IP6OPT_JUMBO_LEN as i32 {
                    ip6stat_inc(Ip6statCounters::Ip6sToosmall);
                    m_freemp(mp);
                    return false;
                }
                if usize::from(opt[i + 1]) != IP6OPT_JUMBO_LEN - 2 {
                    // XXX stat
                    if let Some(m) = mp.take() {
                        icmp6_error(
                            m,
                            ICMP6_PARAM_PROB,
                            ICMP6_PARAMPROB_HEADER,
                            ERROFF + i as i32 + 1,
                        );
                    }
                    return false;
                }
                optlen = IP6OPT_JUMBO_LEN as i32;

                // IPv6 packets that have non 0 payload length must not contain a jumbo
                // payload option.
                let Some(m) = *mp else {
                    return false;
                };
                if mtod_ip6(m).ip6_plen != 0 {
                    ip6stat_inc(Ip6statCounters::Ip6sBadoptions);
                    *mp = None;
                    icmp6_error(
                        m,
                        ICMP6_PARAM_PROB,
                        ICMP6_PARAMPROB_HEADER,
                        ERROFF + i as i32,
                    );
                    return false;
                }

                // We may see jumbolen in unaligned location, so we'd need to perform
                // memcpy().
                let jumboplen =
                    u32::from_be_bytes([opt[i + 2], opt[i + 3], opt[i + 4], opt[i + 5]]);

                // if there are multiple jumbo payload options, *plenp will be non-zero and
                // the packet will be rejected. the behavior may need some debate in ipngwg -
                // multiple options does not make sense, however, there's no explicit mention
                // in specification.
                if *plenp != 0 {
                    ip6stat_inc(Ip6statCounters::Ip6sBadoptions);
                    *mp = None;
                    icmp6_error(
                        m,
                        ICMP6_PARAM_PROB,
                        ICMP6_PARAMPROB_HEADER,
                        ERROFF + i as i32 + 2,
                    );
                    return false;
                }

                // jumbo payload length must be larger than 65535.
                if jumboplen as usize <= IPV6_MAXPACKET {
                    ip6stat_inc(Ip6statCounters::Ip6sBadoptions);
                    *mp = None;
                    icmp6_error(
                        m,
                        ICMP6_PARAM_PROB,
                        ICMP6_PARAMPROB_HEADER,
                        ERROFF + i as i32 + 2,
                    );
                    return false;
                }
                *plenp = jumboplen;
            }
            // unknown option
            _ => {
                if hbhlen < IP6OPT_MINLEN as i32 {
                    ip6stat_inc(Ip6statCounters::Ip6sToosmall);
                    m_freemp(mp);
                    return false;
                }
                let len = i32::from(opt[i + 1]);
                // SAFETY: `i + 1` is inside the options (checked above).
                if !unsafe { ip6_unknown_opt(mp, opthead.add(i), ERROFF + i as i32) } {
                    return false;
                }
                optlen = len + 2;
            }
        }
        hbhlen -= optlen;
        i += optlen as usize;
    }

    true
}

/// `ip6_unknown_opt`: unknown option processing: handles the option at `optp` by its
/// action bits. `off` is the offset from the IPv6 header to the option, which allows
/// returning an ICMPv6 error even if the IPv6 header and the option header are not
/// continuous. `true` to skip the option (the C returns its length, `optp[1]`), `false`
/// when the packet was dropped or an ICMPv6 error sent (the C's -1, `*mp` cleared).
///
/// # Safety
///
/// `optp` points at the option inside `*mp`'s first mbuf (at least two readable
/// bytes).
pub unsafe fn ip6_unknown_opt(mp: &mut Option<&'static Mbuf>, optp: *const u8, off: i32) -> bool {
    // SAFETY: the caller guarantees the option's type byte is readable.
    let ty = unsafe { *optp };

    match ip6opt_type(ty) {
        // ignore the option
        IP6OPT_TYPE_SKIP => true,
        // silently discard
        IP6OPT_TYPE_DISCARD => {
            m_freemp(mp);
            false
        }
        // send ICMP even if multicasted
        IP6OPT_TYPE_FORCEICMP => {
            ip6stat_inc(Ip6statCounters::Ip6sBadoptions);
            if let Some(m) = mp.take() {
                icmp6_error(m, ICMP6_PARAM_PROB, ICMP6_PARAMPROB_OPTION, off);
            }
            false
        }
        // send ICMP if not multicasted
        IP6OPT_TYPE_ICMP => {
            ip6stat_inc(Ip6statCounters::Ip6sBadoptions);
            if let Some(m) = mp.take() {
                let ip6 = mtod_ip6(m);
                if in6_is_addr_multicast(&ip6.ip6_dst)
                    || m.m_flags().get() & (M_BCAST | M_MCAST) != 0
                {
                    m_freem(m);
                } else {
                    icmp6_error(m, ICMP6_PARAM_PROB, ICMP6_PARAMPROB_OPTION, off);
                }
            }
            false
        }
        _ => {
            m_freemp(mp); // XXX: NOTREACHED
            false
        }
    }
}

/// The bytes of a plain-data `#[repr(C)]` value without padding.
fn pod_bytes<T: Copy>(v: &T) -> &[u8] {
    // SAFETY: the callers pass `#[repr(C)]` structures and integers without padding
    // (`timeval`, `in6_pktinfo`, `int`), whose bytes are all initialised.
    unsafe { slice::from_raw_parts(ptr::from_ref(v).cast::<u8>(), size_of::<T>()) }
}

/// The `m_len` bytes of data of the single mbuf `n`.
fn mbuf_bytes(n: &Mbuf) -> &[u8] {
    // SAFETY: an mbuf's `m_len` bytes at `m_data` are initialised data of that mbuf.
    unsafe { slice::from_raw_parts(mtod::<u8>(n), n.m_len().get() as usize) }
}

/// `ip6_savecontrol`: creates the "control" list for this pcb: appends to `*mp` the control
/// messages `inp` asked for about the received packet `m`.
///
/// The routine will be called from upper layer handlers like `udp_input()`.
pub fn ip6_savecontrol(inp: &Inpcb, m: &Mbuf, mp: &mut Option<&'static Mbuf>) {
    let ip6 = mtod_ip6(m);
    // Where *mp points: `mp` itself, then the m_next of the last message.
    let mut tail: Option<&'static Mbuf> = None;
    let mut put = |n: Option<&'static Mbuf>| {
        match tail {
            None => *mp = n,
            Some(t) => t.m_next().set(n),
        }
        if n.is_some() {
            tail = n;
        }
    };

    if inp.socket().has_options(SO_TIMESTAMP) {
        let tv = m_microtime(m);
        put(sbcreatecontrol(pod_bytes(&tv), SCM_TIMESTAMP, SOL_SOCKET));
    }

    // RFC 2292 sec. 5
    if inp.has_flags(IN6P_PKTINFO) {
        let mut pi6 = In6Pktinfo {
            ipi6_addr: ip6.ip6_dst,
            ipi6_ifindex: m.m_pkthdr().ph_ifidx.get(),
        };
        if in6_is_scope_embed(&pi6.ipi6_addr) {
            pi6.ipi6_addr.set_s6_addr16(1, 0);
        }
        put(sbcreatecontrol(pod_bytes(&pi6), IPV6_PKTINFO, IPPROTO_IPV6));
    }

    if inp.has_flags(IN6P_HOPLIMIT) {
        let hlim: i32 = i32::from(ip6.ip6_hlim);
        put(sbcreatecontrol(
            pod_bytes(&hlim),
            IPV6_HOPLIMIT,
            IPPROTO_IPV6,
        ));
    }

    if inp.has_flags(IN6P_TCLASS) {
        let flowinfo = ntohl(ip6.ip6_flow & IPV6_FLOWINFO_MASK) >> 20;
        let tclass: i32 = (flowinfo & 0xff) as i32;
        put(sbcreatecontrol(
            pod_bytes(&tclass),
            IPV6_TCLASS,
            IPPROTO_IPV6,
        ));
    }

    // IPV6_HOPOPTS socket option. Recall that we required super-user privilege for the
    // option (see ip6_ctloutput), but it might be too strict, since there might be some
    // hop-by-hop options which can be returned to normal user. See also RFC 2292 section 6
    // (or RFC 3542 section 8).
    //
    // Check if a hop-by-hop options header is contained in the received packet, and if so,
    // store the options as ancillary data. Note that a hop-by-hop options header must be just
    // after the IPv6 header, which is assured through the IPv6 input processing.
    if inp.has_flags(IN6P_HOPOPTS) && i32::from(ip6.ip6_nxt) == IPPROTO_HOPOPTS {
        let Some(ext) = ip6_pullexthdr(m, size_of::<Ip6Hdr>(), i32::from(ip6.ip6_nxt)) else {
            ip6stat_inc(Ip6statCounters::Ip6sTooshort);
            return;
        };
        let hbh = mbuf_bytes(ext);
        let hbhlen = (usize::from(hbh[offset_of!(Ip6Hbh, ip6h_len)]) + 1) << 3;
        if hbhlen != hbh.len() {
            m_freem(ext);
            ip6stat_inc(Ip6statCounters::Ip6sTooshort);
            return;
        }

        // XXX: We copy the whole header even if a jumbo payload option is included, the
        // option which is to be removed before returning according to RFC2292. Note: this
        // constraint is removed in RFC3542.
        put(sbcreatecontrol(hbh, IPV6_HOPOPTS, IPPROTO_IPV6));
        m_freem(ext);
    }

    // IPV6_DSTOPTS and IPV6_RTHDR socket options
    if inp.has_flags(IN6P_RTHDR | IN6P_DSTOPTS) {
        let mut nxt = i32::from(ip6.ip6_nxt);
        let mut off = size_of::<Ip6Hdr>();

        // Search for destination options headers or routing header(s) through the header
        // chain, and stores each header as ancillary data. Note that the order of the
        // headers remains in the chain of ancillary data. (Is explicit loop prevention
        // necessary?) If it is not an extension header, don't try to pull it from the
        // chain: the loop's condition.
        while let IPPROTO_DSTOPTS | IPPROTO_ROUTING | IPPROTO_HOPOPTS | IPPROTO_AH = nxt {
            let Some(ext) = ip6_pullexthdr(m, off, nxt) else {
                ip6stat_inc(Ip6statCounters::Ip6sTooshort);
                return;
            };
            let ip6e = mbuf_bytes(ext);
            let len = usize::from(ip6e[offset_of!(Ip6Ext, ip6e_len)]);
            let elen = if nxt == IPPROTO_AH {
                (len + 2) << 2
            } else {
                (len + 1) << 3
            };
            if elen != ip6e.len() {
                m_freem(ext);
                ip6stat_inc(Ip6statCounters::Ip6sTooshort);
                return;
            }

            match nxt {
                IPPROTO_DSTOPTS if inp.has_flags(IN6P_DSTOPTS) => {
                    put(sbcreatecontrol(ip6e, IPV6_DSTOPTS, IPPROTO_IPV6));
                }
                IPPROTO_ROUTING if inp.has_flags(IN6P_RTHDR) => {
                    put(sbcreatecontrol(ip6e, IPV6_RTHDR, IPPROTO_IPV6));
                }
                // IPPROTO_HOPOPTS, IPPROTO_AH (is it possible?), or not asked for
                _ => {}
            }

            // proceed with the next header.
            off += elen;
            nxt = i32::from(ip6e[offset_of!(Ip6Ext, ip6e_nxt)]);
            m_freem(ext);
        }
    }
}

/// `ip6_pullexthdr`: pulls the single extension header of type `nxt` at `off` from the
/// mbuf chain; returns a single mbuf that contains the result, or `None` on error.
fn ip6_pullexthdr(m: &Mbuf, off: usize, nxt: i32) -> Option<&'static Mbuf> {
    #[cfg(feature = "diagnostic")]
    match nxt {
        IPPROTO_DSTOPTS | IPPROTO_ROUTING | IPPROTO_HOPOPTS | IPPROTO_AH => {}
        _ => {
            crate::kern::subr_prf::printf(format_args!("ip6_pullexthdr: invalid nxt={nxt}\n"));
        }
    }

    let pktlen = m.m_pkthdr().len.get().max(0) as usize;
    if off + size_of::<Ip6Ext>() > pktlen {
        return None;
    }

    let mut ip6e = [0u8; size_of::<Ip6Ext>()];
    m_copydata(m, off as i32, &mut ip6e);
    let len = usize::from(ip6e[offset_of!(Ip6Ext, ip6e_len)]);
    let elen = if nxt == IPPROTO_AH {
        (len + 2) << 2
    } else {
        (len + 1) << 3
    };

    if off + elen > pktlen {
        return None;
    }

    let mut n = m_get(M_DONTWAIT, MT_DATA);
    if let Some(nn) = n
        && elen >= MLEN
    {
        mclget(nn, M_DONTWAIT);
        if nn.m_flags().get() & M_EXT == 0 {
            m_free(nn);
            n = None;
        }
    }
    let Some(n) = n else {
        ip6stat_inc(Ip6statCounters::Ip6sIdropped);
        return None;
    };

    n.m_len().set(0);
    if elen as i32 >= m_trailingspace(n) {
        m_free(n);
        return None;
    }

    // SAFETY: `n` has more than `elen` bytes of trailing space at its data (checked above).
    let dst = unsafe { slice::from_raw_parts_mut(mtod::<u8>(n), elen) };
    m_copydata(m, off as i32, dst);
    n.m_len().set(elen as u32);
    Some(n)
}

/// `ip6_get_prevhdr`: the offset of the next header field that precedes offset `off` (the
/// field of the header before the one being processed).
pub fn ip6_get_prevhdr(m: &Mbuf, off: i32) -> i32 {
    if off == IP6_HDR_LEN {
        return offset_of!(Ip6Hdr, ip6_nxt) as i32;
    } else if off < IP6_HDR_LEN {
        panic(format_args!(
            "ip6_get_prevhdr: off < sizeof(struct ip6_hdr)"
        ));
    }

    let ip6 = mtod_ip6(m);
    let mut nxt = i32::from(ip6.ip6_nxt);
    let mut len = IP6_HDR_LEN;
    let mut nlen = 0;
    while len < off {
        let mut ip6e = [0u8; size_of::<Ip6Ext>()];
        m_copydata(m, len, &mut ip6e);
        let elen = i32::from(ip6e[offset_of!(Ip6Ext, ip6e_len)]);

        nlen = match nxt {
            IPPROTO_FRAGMENT => size_of::<Ip6Frag>() as i32,
            IPPROTO_AH => (elen + 2) << 2,
            _ => (elen + 1) << 3,
        };
        len += nlen;
        nxt = i32::from(ip6e[offset_of!(Ip6Ext, ip6e_nxt)]);
    }

    len - nlen
}

/// `ip6_nexthdr`: gets the offset of the header after the one of protocol `proto` at `off`,
/// its protocol stored in `nxtp`; `m` is retained. `None` where the C returns -1.
pub fn ip6_nexthdr(m: &Mbuf, off: i32, proto: i32, nxtp: &mut i32) -> Option<i32> {
    let pktlen = m.m_pkthdr().len.get();
    if m.m_flags().get() & M_PKTHDR == 0 || pktlen < off {
        return None;
    }

    match proto {
        IPPROTO_IPV6 => {
            if pktlen < off + IP6_HDR_LEN {
                return None;
            }
            let mut ip6 = [0u8; size_of::<Ip6Hdr>()];
            m_copydata(m, off, &mut ip6);
            *nxtp = i32::from(ip6[offset_of!(Ip6Hdr, ip6_nxt)]);
            Some(off + IP6_HDR_LEN)
        }
        IPPROTO_FRAGMENT => {
            // terminate parsing if it is not the first fragment, it does not make sense to
            // parse through it.
            if pktlen < off + size_of::<Ip6Frag>() as i32 {
                return None;
            }
            let mut fh = [0u8; size_of::<Ip6Frag>()];
            m_copydata(m, off, &mut fh);
            let o = offset_of!(Ip6Frag, ip6f_offlg);
            if u16::from_ne_bytes([fh[o], fh[o + 1]]) & IP6F_OFF_MASK != 0 {
                return None;
            }
            *nxtp = i32::from(fh[offset_of!(Ip6Frag, ip6f_nxt)]);
            Some(off + size_of::<Ip6Frag>() as i32)
        }
        IPPROTO_AH | IPPROTO_HOPOPTS | IPPROTO_ROUTING | IPPROTO_DSTOPTS => {
            if pktlen < off + size_of::<Ip6Ext>() as i32 {
                return None;
            }
            let mut ip6e = [0u8; size_of::<Ip6Ext>()];
            m_copydata(m, off, &mut ip6e);
            *nxtp = i32::from(ip6e[offset_of!(Ip6Ext, ip6e_nxt)]);
            let len = i32::from(ip6e[offset_of!(Ip6Ext, ip6e_len)]);
            let off = if proto == IPPROTO_AH {
                off + ((len + 2) << 2)
            } else {
                off + ((len + 1) << 3)
            };
            if pktlen < off {
                return None;
            }
            Some(off)
        }
        // give up
        IPPROTO_NONE | IPPROTO_ESP | IPPROTO_IPCOMP => None,
        _ => None,
    }
}

/// `ip6_lasthdr`: gets the offset of the last header in the chain (the upper-layer
/// protocol), its protocol stored in `nxtp`; `m` is kept untainted. `None` where the C
/// returns -1 (an invalid chain).
pub fn ip6_lasthdr(m: &Mbuf, off: i32, proto: i32, nxtp: &mut i32) -> Option<i32> {
    let mut off = off;
    let mut proto = proto;

    loop {
        let Some(newoff) = ip6_nexthdr(m, off, proto, nxtp) else {
            return Some(off);
        };
        if newoff < off {
            return None; // invalid
        } else if newoff == off {
            return Some(newoff);
        }

        off = newoff;
        proto = *nxtp;
    }
}

/// `ip6_sysctl_ip6stat`: `net.inet6.ip6.stats`, the counters as a `struct ip6stat`.
fn ip6_sysctl_ip6stat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    const _: () = assert!(size_of::<Ip6stat>() == IP6S_NCOUNTERS * size_of::<u64>());

    let mut ip6stat = vec![0u8; size_of::<Ip6stat>()];
    let (words, _) = ip6stat.as_chunks_mut::<{ size_of::<u64>() }>();
    for (w, c) in words.iter_mut().zip(IP6COUNTERS.iter()) {
        *w = c.load(Ordering::Relaxed).to_ne_bytes();
    }
    sysctl_rdstruct(oldp, oldlenp, newp, &ip6stat)
}

/// `ip6_sysctl`: system control for IP6, the `net.inet6.ip6` sysctls.
pub fn ip6_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    // Almost all sysctl names at this level are terminal.
    let Some(&name0) = name.first() else {
        return Err(Errno::ENOTDIR);
    };
    if name.len() != 1 && name0 != IPV6CTL_IFQUEUE {
        return Err(Errno::ENOTDIR);
    }

    match name0 {
        IPV6CTL_STATS => ip6_sysctl_ip6stat(oldp, oldlenp, newp),
        // !MROUTING:
        IPV6CTL_MRTSTATS | IPV6CTL_MRTPROTO | IPV6CTL_MRTMIF | IPV6CTL_MRTMFC => {
            Err(Errno::EOPNOTSUPP)
        }
        IPV6CTL_MTUDISCTIMEOUT => {
            let oldval = IP6_MTUDISC_TIMEOUT.load(Ordering::Relaxed);
            let newval = AtomicI32::new(oldval);
            let error = sysctl_int_bounded(oldp, oldlenp, newp, newlen, &newval, 0, INT_MAX);
            let newval = newval.into_inner();
            if error.is_ok() && oldval != newval {
                rw_enter_write(&SYSCTL_LOCK);
                IP6_MTUDISC_TIMEOUT.store(newval, Ordering::Relaxed);
                rt_timer_queue_change(&ICMP6_MTUDISC_TIMEOUT_Q, newval);
                rw_exit_write(&SYSCTL_LOCK);
            }

            error
        }
        IPV6CTL_IFQUEUE => sysctl_niq(&name[1..], oldp, oldlenp, newp, newlen, &IP6INTRQ),
        IPV6CTL_MULTIPATH => {
            let oldval = IP6_MULTIPATH.load(Ordering::Relaxed);
            let newval = AtomicI32::new(oldval);
            let error = sysctl_int_bounded(oldp, oldlenp, newp, newlen, &newval, 0, 1);
            let newval = newval.into_inner();
            if error.is_ok() && oldval != newval {
                IP6_MULTIPATH.store(newval, Ordering::Relaxed);
                core::sync::atomic::fence(Ordering::Release);
                RTGENERATION.fetch_add(1, Ordering::Relaxed);
            }

            error
        }
        _ => sysctl_bounded_arr(&IPV6CTL_VARS, name, oldp, oldlenp, newp, newlen),
    }
}

/// `ip6_send_dispatch`: `ip6send_task`: sends the packets queued on `xmq` with
/// `ip6_output`.
fn ip6_send_dispatch(xmq: *mut c_void) {
    // SAFETY: the task is initialised with its static queue as the argument.
    let mq = unsafe { &*xmq.cast::<MbufQueue>() };
    let ml = MbufList::new();

    mq_delist(mq, &ml);
    if ml_empty(&ml) {
        return;
    }

    net_lock_shared();
    while let Some(m) = ml_dequeue(&ml) {
        let _ = ip6_output(m, None, None, 0, None, None);
    }
    net_unlock_shared();
}

/// `ip6_send`: queues `m` for output by `ip6_send_dispatch` (from contexts that cannot
/// call `ip6_output` directly).
pub fn ip6_send(m: &'static Mbuf) {
    mq_enqueue(&IP6SEND_MQ, m);
    if let Some(tq) = net_tq(0) {
        task_add(tq, &IP6SEND_TASK);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for IPv6 input on synthetic packets: the header checks of `ipv6_check`, the
    // payload length checks and trimming of `ip6_hbhchcheck`, the hop-by-hop option walk of
    // `ip6_process_hopopts` and the actions of `ip6_unknown_opt`, the extension header chain
    // walkers (`ip6_nexthdr`, `ip6_lasthdr`, `ip6_get_prevhdr`) and the type 0 routing header
    // scan.

    use std::vec::Vec;

    use super::*;
    use crate::net::if_::tests::{setup_net, test_ifnet, test_packet};
    use crate::netinet::in_::IPPROTO_UDP;

    /// An IPv6 header (`nxt`, payload length `plen`, `src` and `dst`) followed by `rest`.
    fn packet(nxt: i32, plen: u16, src: [u8; 16], dst: [u8; 16], rest: &[u8]) -> &'static Mbuf {
        let mut ip6 = Ip6Hdr::zeroed();
        ip6.set_ip6_vfc(IPV6_VERSION);
        ip6.ip6_plen = htons(plen);
        ip6.ip6_nxt = nxt as u8;
        ip6.ip6_hlim = 64;
        ip6.ip6_src = crate::netinet6::in6::In6Addr::new(src);
        ip6.ip6_dst = crate::netinet6::in6::In6Addr::new(dst);
        let mut b: Vec<u8> = pod_bytes(&ip6).to_vec();
        b.extend_from_slice(rest);
        test_packet(&b)
    }

    /// fd00::`last`.
    fn ula(last: u8) -> [u8; 16] {
        let mut a = [0u8; 16];
        a[0] = 0xfd;
        a[15] = last;
        a
    }

    fn stat(c: Ip6statCounters) -> u64 {
        IP6COUNTERS[c as usize].load(Ordering::Relaxed)
    }

    #[test]
    fn ipv6_check_rejects_bad_headers() {
        let _g = setup_net();
        let ifp = test_ifnet(b"tv6c0");

        let good = packet(IPPROTO_UDP, 0, ula(1), ula(2), &[]);
        let m = ipv6_check(ifp, good).expect("a good header passes");
        m_freem(m);

        // Version 4 in the version field.
        let badvers = stat(Ip6statCounters::Ip6sBadvers);
        let m = packet(IPPROTO_UDP, 0, ula(1), ula(2), &[]);
        let mut ip6 = mtod_ip6(m);
        ip6.set_ip6_vfc(0x40);
        mtod_ip6_store(m, &ip6);
        assert!(ipv6_check(ifp, m).is_none());
        assert_eq!(stat(Ip6statCounters::Ip6sBadvers), badvers + 1);

        let mut mcast = [0u8; 16];
        mcast[0] = 0xff;
        mcast[1] = 0x02;
        mcast[15] = 1;
        let mut loopback = [0u8; 16];
        loopback[15] = 1;
        let mut v4mapped = [0u8; 16];
        v4mapped[10] = 0xff;
        v4mapped[11] = 0xff;
        v4mapped[12..].copy_from_slice(&[10, 0, 0, 1]);
        let mut v4compat = [0u8; 16];
        v4compat[12..].copy_from_slice(&[10, 0, 0, 1]);
        let mut embedded = [0u8; 16];
        embedded[0] = 0xfe;
        embedded[1] = 0x80;
        embedded[3] = 7;
        embedded[15] = 1;
        for (src, dst) in [
            (mcast, ula(2)),    // a multicast source
            (ula(1), [0; 16]),  // an unspecified destination
            (loopback, ula(2)), // ::1 on a non-loopback interface
            (ula(1), v4mapped), // an IPv4 mapped destination
            (v4compat, ula(2)), // an IPv4 compatible source
            (embedded, ula(2)), // a scope already embedded
        ] {
            let badscope = stat(Ip6statCounters::Ip6sBadscope);
            assert!(ipv6_check(ifp, packet(IPPROTO_UDP, 0, src, dst, &[])).is_none());
            assert_eq!(stat(Ip6statCounters::Ip6sBadscope), badscope + 1);
        }

        // A packet shorter than the header.
        let toosmall = stat(Ip6statCounters::Ip6sToosmall);
        assert!(ipv6_check(ifp, test_packet(&[0x60; 20])).is_none());
        assert_eq!(stat(Ip6statCounters::Ip6sToosmall), toosmall + 1);
    }

    #[test]
    fn the_payload_length_is_checked_and_the_packet_trimmed() {
        let _g = setup_net();

        // The header announces 16 bytes, only 8 follow.
        let tooshort = stat(Ip6statCounters::Ip6sTooshort);
        let mut mp = Some(packet(IPPROTO_UDP, 16, ula(1), ula(2), &[0; 8]));
        let mut off = 0;
        assert_eq!(ip6_hbhchcheck(&mut mp, &mut off, None, 0), IPPROTO_DONE);
        assert!(mp.is_none());
        assert_eq!(stat(Ip6statCounters::Ip6sTooshort), tooshort + 1);

        // The header announces 8 bytes, 24 follow (link-layer padding): trimmed to 48.
        let mut mp = Some(packet(IPPROTO_UDP, 8, ula(1), ula(2), &[0; 24]));
        let mut off = 0;
        assert_eq!(ip6_hbhchcheck(&mut mp, &mut off, None, 0), IPPROTO_UDP);
        assert_eq!(off, 40);
        let m = mp.expect("kept");
        assert_eq!(m.m_len().get(), 48);
        assert_eq!(m.m_pkthdr().len.get(), 48);
        m_freem(m);
    }

    #[test]
    fn hop_by_hop_options_are_walked() {
        let _g = setup_net();

        // Hop-by-hop header (next UDP, 8 bytes): Pad1, router alert (MLD 0x0001), PadN of 0.
        #[rustfmt::skip]
    let hbh = [
        IPPROTO_UDP as u8, 0,
        IP6OPT_PAD1,
        IP6OPT_ROUTER_ALERT, 2, 0x00, 0x01,
        IP6OPT_PAD1,
    ];
        let mut mp = Some(packet(IPPROTO_HOPOPTS, 8, ula(1), ula(2), &hbh));
        let mut off = 0;
        let mut ours = false;
        let nxt = ip6_hbhchcheck(&mut mp, &mut off, Some(&mut ours), IPV6_FORWARDING);
        assert_eq!(nxt, IPPROTO_UDP);
        assert_eq!(off, 48);
        assert!(ours, "a router alert makes a router accept the packet");
        m_freem(mp.take());

        // An unknown option whose action is "skip" (type 0x1e) between PadN options.
        let opts = [0x1e, 2, 0xaa, 0xbb, IP6OPT_PADN, 0];
        let m = test_packet(&opts);
        let mut mp = Some(m);
        let mut rtalert = !0;
        let mut plen = 0;
        // SAFETY: the six option bytes are in the mbuf.
        let ok = unsafe { ip6_process_hopopts(&mut mp, mtod::<u8>(m), 6, &mut rtalert, &mut plen) };
        assert!(ok);
        assert_eq!(rtalert, !0);
        assert_eq!(plen, 0);
        m_freem(mp.take());

        // The same option with the "discard" action (0x5e): the packet is dropped silently.
        let m = test_packet(&[0x5e, 2, 0xaa, 0xbb]);
        let mut mp = Some(m);
        // SAFETY: the four option bytes are in the mbuf.
        let ok = unsafe { ip6_process_hopopts(&mut mp, mtod::<u8>(m), 4, &mut rtalert, &mut plen) };
        assert!(!ok);
        assert!(mp.is_none());

        // A PadN with a single byte left is too small.
        let toosmall = stat(Ip6statCounters::Ip6sToosmall);
        let m = test_packet(&[IP6OPT_PAD1, IP6OPT_PADN]);
        let mut mp = Some(m);
        // SAFETY: the two option bytes are in the mbuf.
        let ok = unsafe { ip6_process_hopopts(&mut mp, mtod::<u8>(m), 2, &mut rtalert, &mut plen) };
        assert!(!ok);
        assert!(mp.is_none());
        assert_eq!(stat(Ip6statCounters::Ip6sToosmall), toosmall + 1);
    }

    #[test]
    fn unknown_option_actions() {
        let _g = setup_net();
        let m = test_packet(&[0x1e, 0]);
        let mut mp = Some(m);
        // SAFETY: the option's two bytes are in the mbuf.
        assert!(unsafe { ip6_unknown_opt(&mut mp, mtod::<u8>(m), 42) });
        assert!(mp.is_some());
        m_freem(mp.take());

        let m = test_packet(&[0x7f, 0]);
        let mut mp = Some(m);
        // SAFETY: as above.
        assert!(!unsafe { ip6_unknown_opt(&mut mp, mtod::<u8>(m), 42) });
        assert!(mp.is_none());
    }

    /// An IPv6 header, a hop-by-hop header (8), destination options (16), a fragment header
    /// (`offlg`) and 8 bytes of UDP.
    fn chain(offlg: u16) -> &'static Mbuf {
        let mut rest = Vec::new();
        rest.extend_from_slice(&[IPPROTO_DSTOPTS as u8, 0, 1, 4, 0, 0, 0, 0]);
        rest.extend_from_slice(&[IPPROTO_FRAGMENT as u8, 1, 1, 12]);
        rest.extend_from_slice(&[0; 12]);
        rest.extend_from_slice(&[IPPROTO_UDP as u8, 0]);
        rest.extend_from_slice(&offlg.to_be_bytes());
        rest.extend_from_slice(&[0, 0, 0, 7]);
        rest.extend_from_slice(&[0; 8]);
        packet(IPPROTO_HOPOPTS, rest.len() as u16, ula(1), ula(2), &rest)
    }

    #[test]
    fn the_extension_header_chain_is_walked() {
        let _g = setup_net();

        let m = chain(0x0001); // first fragment, more fragments
        let mut nxt = -1;
        assert_eq!(ip6_nexthdr(m, 0, IPPROTO_IPV6, &mut nxt), Some(40));
        assert_eq!(nxt, IPPROTO_HOPOPTS);
        assert_eq!(ip6_nexthdr(m, 40, IPPROTO_HOPOPTS, &mut nxt), Some(48));
        assert_eq!(nxt, IPPROTO_DSTOPTS);
        assert_eq!(ip6_lasthdr(m, 0, IPPROTO_IPV6, &mut nxt), Some(72));
        assert_eq!(nxt, IPPROTO_UDP);
        assert_eq!(ip6_get_prevhdr(m, 40), offset_of!(Ip6Hdr, ip6_nxt) as i32);
        assert_eq!(ip6_get_prevhdr(m, 64), 48);
        assert_eq!(ip6_get_prevhdr(m, 72), 64);
        m_freem(m);

        // A later fragment: the walk stops at the fragment header.
        let m = chain(0x0040); // byte offset 64
        let mut nxt = -1;
        assert_eq!(ip6_lasthdr(m, 0, IPPROTO_IPV6, &mut nxt), Some(64));
        assert_eq!(nxt, IPPROTO_FRAGMENT);
        // An ESP header is never walked into.
        assert_eq!(ip6_nexthdr(m, 64, IPPROTO_ESP, &mut nxt), None);
        // A header past the end of the packet.
        assert_eq!(ip6_nexthdr(m, 1000, IPPROTO_HOPOPTS, &mut nxt), None);
        m_freem(m);

        // AH counts its length in 4-byte words, plus 2.
        let ah = [IPPROTO_UDP as u8, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let m = packet(IPPROTO_AH, ah.len() as u16, ula(1), ula(2), &ah);
        let mut nxt = -1;
        assert_eq!(ip6_lasthdr(m, 0, IPPROTO_IPV6, &mut nxt), Some(52));
        assert_eq!(nxt, IPPROTO_UDP);
        m_freem(m);
    }

    #[test]
    fn type_0_routing_headers_are_rejected() {
        let _g = setup_net();
        let rthdr = |ty: u8, nxt: i32| [nxt as u8, 0, ty, 0, 0, 0, 0, 0];

        let mut off = 0;
        let m = packet(IPPROTO_ROUTING, 8, ula(1), ula(2), &rthdr(0, IPPROTO_UDP));
        assert!(ip6_check_rh0hdr(m, &mut off));
        assert_eq!(off, 40 + offset_of!(Ip6Rthdr, ip6r_type) as i32);
        m_freem(m);

        let mut off = 0;
        let m = packet(IPPROTO_ROUTING, 8, ula(1), ula(2), &rthdr(2, IPPROTO_UDP));
        assert!(!ip6_check_rh0hdr(m, &mut off));
        m_freem(m);

        // Two routing headers, behind a destination options header.
        let mut rest = vec![IPPROTO_ROUTING as u8, 0, 1, 4, 0, 0, 0, 0];
        rest.extend_from_slice(&rthdr(2, IPPROTO_ROUTING));
        rest.extend_from_slice(&rthdr(2, IPPROTO_UDP));
        let mut off = 0;
        let m = packet(IPPROTO_DSTOPTS, rest.len() as u16, ula(1), ula(2), &rest);
        assert!(ip6_check_rh0hdr(m, &mut off));
        assert_eq!(off, 56);
        m_freem(m);
    }

    #[test]
    fn deep_names_are_not_directories() {
        let mut len = 0;
        assert_eq!(
            ip6_sysctl(&[IPV6CTL_DEFHLIM, 1], 0, &mut len, 0, 0),
            Err(Errno::ENOTDIR)
        );
        assert_eq!(ip6_sysctl(&[], 0, &mut len, 0, 0), Err(Errno::ENOTDIR));
    }
}
/* </TESTS> */
