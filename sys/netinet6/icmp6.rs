/*	$KAME: icmp6.c,v 1.217 2001/06/20 15:03:29 jinmei Exp $	*/
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
 *	@(#)ip_icmp.c	8.2 (Berkeley) 1/4/94
 */
/* </LICENSES> */

/* <CODE> */
//! ICMPv6: error generation, input processing (echo, MLD and ND dispatch, errors to
//! the upper layers), reflection, redirects, path MTU discovery and the
//! `net.inet6.icmp6` sysctls: `netinet6/icmp6.c`.
//!
//! Upstream: sys/netinet6/icmp6.c @ 3ce1f3f79392
//!
//! `icmp6_input` checks and counts a message, answers an echo request with an echo reply
//! (`icmp6_reflect` picks the source address), hands MLD, router and neighbor discovery and
//! redirect messages to their handlers on a copy of the packet, turns the errors into
//! `pr_ctlinput` calls for the protocol of the packet they quote
//! (`icmp6_notify_error`, with the path MTU learned in `icmp6_mtudisc_update`) and finally gives
//! the packet to the raw sockets (`rip6_input`).
//!
//! ## Deviations
//! - `icmp6counters` (`struct cpumem *`) is the static array of atomics [`ICMP6COUNTERS`]
//!   (`netinet/icmp6.rs`'s `icmp6stat_inc` bumps it), as `icmpcounters` is for ICMP.
//!   `icmp6_sysctl_icmp6stat` copies it into a `Vec` instead of `malloc(M_TEMP)`.
//! - `icmp6_mtudisc_callbacks` (a `LIST` of malloc'd entries) is a `Vec` of function pointers
//!   written by `icmp6_mtudisc_callback_register` at protocol initialisation only, which is
//!   why it is a `StaticCell` without a lock; registering twice is still a no-op.
//! - `icmp6_notify_error` returns `true` for the C's 0 (the packet is still ours) and `false`
//!   for -1 (it was freed). `ip6cp.ip6c_dst` and, for messages other than Packet Too Big,
//!   `ip6c_cmdarg` are NULL; the C leaves them uninitialised.
//! - `icmp6_redirect_input` frees `m` when the receiving interface is gone (the C returns
//!   without freeing it: a leak).
//! - `icmp6_redirect_output` builds the headers in a stack buffer and copies them to the mbuf
//!   in one go, instead of writing through pointers into the mbuf; the packet is the same.
//!   `icmp6_do_error` clears the embedded scope of the quoted header before prepending the
//!   new one (the C does it after, through the same memory) so it never holds a pointer into
//!   the packet across `m_prepend`.
//! - `NCARP > 0` (`carp_lsdrop` on echo requests) is not configured. The `PRC_*` dispatch
//!   `inet6sw[ip6_protox[nxt]].pr_ctlinput` goes through [`crate::netinet6::in6_proto`].
//! - `icmp6_mtudisc_timeout` is the C's: it does not tell TCP that the path MTU may have
//!   grown (the IPv4 twin does).
//! - The ICMPv6 parameter problem pointer of `icmp6_error` is whatever the caller passes.
//! - `ICMP6_FILTER` get/set (`icmp6_ctloutput`) reaches the socket's control block through
//!   `sotoinpcb`, which must accept `AF_INET6` sockets (phase 2 of the INET6 port).

use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use libkern::StaticCell;

use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write};
use crate::kern::kern_sysctl::{
    SYSCTL_LOCK, sysctl_bounded_arr, sysctl_int_bounded, sysctl_rdint, sysctl_rdstruct,
};
use crate::kern::kern_time::ppsratecheck_shared;
use crate::kern::uipc_domain::pfctlinput;
use crate::kern::uipc_mbuf::{
    m_adj, m_cat, m_copydata, m_copym, m_free, m_freem, m_gethdr, m_prepend, m_pullup, m_resethdr,
    m_trailingspace,
};
use crate::net::if_::{IFF_UP, IFXF_AUTOCONF6, if_get, if_put, ifa_ifwithaddr};
use crate::net::if_dl::{lladdr, satosdl};
use crate::net::if_var::{Ifnet, Netstack};
use crate::net::pf::pf_pkt_addr_changed;
use crate::net::route::{
    RT_RESOLVE, RTAX_DST, RTAX_GATEWAY, RTAX_LABEL, RTF_BLACKHOLE, RTF_DYNAMIC, RTF_GATEWAY,
    RTF_HOST, RTF_LLINFO, RTF_LOCAL, RTF_REJECT, RTM_ADD, RTV_MTU, RtAddrinfo, Rtentry,
    RttimerQueue, SockaddrRtlabel, rt_timer_add, rt_timer_queue_change, rt_timer_queue_count,
    rt_timer_queue_init, rtalloc, rtdeletemsg, rtfree, rtisvalid, rtlabel_id2sa, rtredirect,
    rtrequest,
};
use crate::net::rtable::rtable_getsource;
use crate::net::rtsock::rtm_send;
use crate::netinet::icmp6::{
    ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_ADDR, ICMP6_DST_UNREACH_ADMIN,
    ICMP6_DST_UNREACH_BEYONDSCOPE, ICMP6_DST_UNREACH_NOPORT, ICMP6_DST_UNREACH_NOROUTE,
    ICMP6_ECHO_REPLY, ICMP6_ECHO_REQUEST, ICMP6_PACKET_TOO_BIG, ICMP6_PARAM_PROB,
    ICMP6_PARAMPROB_HEADER, ICMP6_PARAMPROB_NEXTHEADER, ICMP6_PARAMPROB_OPTION,
    ICMP6_ROUTER_RENUMBERING, ICMP6_ROUTER_RENUMBERING_COMMAND, ICMP6_ROUTER_RENUMBERING_RESULT,
    ICMP6_TIME_EXCEED_REASSEMBLY, ICMP6_TIME_EXCEED_TRANSIT, ICMP6_TIME_EXCEEDED, ICMP6_WRUREPLY,
    ICMP6_WRUREQUEST, ICMPV6_PLD_MAXLEN, ICMPV6CTL_ERRPPSLIMIT, ICMPV6CTL_MTUDISC_HIWAT,
    ICMPV6CTL_MTUDISC_LOWAT, ICMPV6CTL_ND6_DELAY, ICMPV6CTL_ND6_MMAXTRIES, ICMPV6CTL_ND6_QUEUED,
    ICMPV6CTL_ND6_UMAXTRIES, ICMPV6CTL_REDIRTIMEOUT, ICMPV6CTL_STATS, ICP6S_NCOUNTERS, Icmp6Filter,
    Icmp6Hdr, Icmp6RouterRenum, Icmp6stat, Icmp6statCounters, MLD_LISTENER_DONE,
    MLD_LISTENER_QUERY, MLD_LISTENER_REPORT, MLD_MTRACE, MLD_MTRACE_RESP, MldHdr,
    ND_NEIGHBOR_ADVERT, ND_NEIGHBOR_SOLICIT, ND_OPT_REDIRECTED_HEADER, ND_OPT_TARGET_LINKADDR,
    ND_REDIRECT, ND_REDIRECT_ONLINK, ND_REDIRECT_ROUTER, ND_ROUTER_ADVERT, ND_ROUTER_SOLICIT,
    NdNeighborAdvert, NdNeighborSolicit, NdOptHdr, NdOptRdHdr, NdRedirect, NdRouterAdvert,
    NdRouterSolicit, icmp6stat_inc, icmp6stat_inc_hist,
};
use crate::netinet::in_::{IPPROTO_AH, IPPROTO_DSTOPTS, IPPROTO_HOPOPTS, IPPROTO_ROUTING};
use crate::netinet::in_::{IPPROTO_DONE, IPPROTO_FRAGMENT, IPPROTO_ICMPV6, IPPROTO_IPV6};
use crate::netinet::in_pcb::sotoinpcb;
use crate::netinet::ip6::{
    IP6F_OFF_MASK, IPV6_FLOWLABEL_MASK, IPV6_MMTU, IPV6_VERSION, Ip6Ext, Ip6Frag, Ip6Hdr, Ip6Rthdr,
    Ip6Rthdr0, ip6_exthdr_get,
};
use crate::netinet6::in6::{
    ICMP6_FILTER, IPV6_RTHDR_TYPE_0, In6Addr, SockaddrIn6, ifatoia6, in6_addr2scopeid,
    in6_ifawithscope, in6_is_addr_linklocal, in6_is_addr_multicast, in6_is_addr_unspecified,
    in6_is_scope_embed, in6ifa_ifpforlinklocal, sin6tosa,
};
use crate::netinet6::in6_cksum::in6_cksum;
use crate::netinet6::in6_proto::{
    ICMP6_REDIRTIMEOUT, ICMP6ERRPPSLIM, INET6SW, IP6_DEFHLIM, IP6_FORWARDING, IP6_MAXDYNROUTES,
    IP6_MTUDISC_TIMEOUT, IP6_PROTOX,
};
use crate::netinet6::in6_src::in6_embedscope;
use crate::netinet6::in6_var::{IN6_IFF_ANYCAST, IN6_IFF_DUPLICATED, IN6_IFF_TENTATIVE, ia6_in6};
use crate::netinet6::ip6_input::{ip6_lasthdr, ip6_send};
use crate::netinet6::ip6_output::ip6_output;
use crate::netinet6::ip6_var::mtod_ip6;
use crate::netinet6::ip6_var::mtod_ip6_store;
use crate::netinet6::ip6protosw::Ip6ctlparam;
use crate::netinet6::mld6::{mld6_fasttimo, mld6_init, mld6_input};
use crate::netinet6::nd6::{
    ND6_DELAY, ND6_MMAXTRIES, ND6_UMAXTRIES, NdOpts, ln_hold_total, nd6_cache_lladdr,
    nd6_is_addr_neighbor, nd6_lookup, nd6_opt_lladdr, nd6_options,
};
use crate::netinet6::nd6_nbr::{nd6_na_input, nd6_ns_input};
use crate::netinet6::nd6_rtr::nd6_rtr_cache;
use crate::netinet6::raw_ip6::rip6_input;
use crate::sys::endian::{htonl, htons, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::limits::INT_MAX;
use crate::sys::mbuf::{
    M_BCAST, M_COPYALL, M_DONTWAIT, M_EXT, M_ICMP_CSUM_OUT, M_MAXLOOP, M_MCAST, MCLBYTES, MHLEN,
    MT_HEADER, Mbuf, PF_TAG_DIVERTED, PF_TAG_GENERATED, m_freemp, m_move_pkthdr, mclget, mtod,
};
use crate::sys::protosw::{
    PRC_HOSTDEAD, PRC_MSGSIZE, PRC_NCMDS, PRC_PARAMPROB, PRC_REDIRECT_HOST, PRC_TIMXCEED_INTRANS,
    PRC_TIMXCEED_REASS, PRC_UNREACH_NET, PRC_UNREACH_PORT, PRC_UNREACH_PROTOCOL, PRCO_GETOPT,
    PRCO_SETOPT, PrCtlinputFn,
};
use crate::sys::socket::{AF_INET6, AF_LINK};
use crate::sys::socketvar::Socket;
use crate::sys::sysctl::SysctlBoundedArgs;
use crate::sys::systm::net_assert_locked;
use crate::sys::time::Timeval;

/// `void (*)(struct sockaddr_in6 *, u_int)`: a path MTU change callback
/// (`icmp6_mtudisc_callback_register`): the destination and the routing table.
pub type Icmp6MtudiscCallbackFn = fn(&SockaddrIn6, u32);

/// `icmp6counters`: the ICMPv6 statistics.
pub static ICMP6COUNTERS: [AtomicU64; ICP6S_NCOUNTERS] =
    [const { AtomicU64::new(0) }; ICP6S_NCOUNTERS];

/// `icmp6errpps_count` and `icmp6errppslim_last`.
struct Icmp6ErrPps {
    last: Timeval,
    count: i32,
}

/// `icmp6errpps_count`, `icmp6errppslim_last`: only `ppsratecheck` touches them, under its
/// mutex.
static ICMP6ERRPPS: StaticCell<Icmp6ErrPps> = StaticCell::new(Icmp6ErrPps {
    last: Timeval {
        tv_sec: 0,
        tv_usec: 0,
    },
    count: 0,
});

/// `icmp6_mtudisc_callbacks`: the callbacks to notify when Path MTU changes are made.
/// Written by `icmp6_mtudisc_callback_register` during protocol initialisation only.
static ICMP6_MTUDISC_CALLBACKS: StaticCell<Vec<Icmp6MtudiscCallbackFn>> =
    StaticCell::new(Vec::new());

/// `icmp6_mtudisc_timeout_q`: the routes cloned for path MTU discovery.
pub static ICMP6_MTUDISC_TIMEOUT_Q: RttimerQueue = RttimerQueue::new();

/// \[a\] `icmp6_mtudisc_hiwat`: XXX do these values make any sense?
pub static ICMP6_MTUDISC_HIWAT: AtomicI32 = AtomicI32::new(1280);
/// \[a\] `icmp6_mtudisc_lowat`.
pub static ICMP6_MTUDISC_LOWAT: AtomicI32 = AtomicI32::new(256);

/// `icmp6_redirect_timeout_q`: keeps track of the number of redirect routes.
pub static ICMP6_REDIRECT_TIMEOUT_Q: RttimerQueue = RttimerQueue::new();

/// `icmpv6ctl_vars[]`.
static ICMPV6CTL_VARS: [SysctlBoundedArgs; 6] = [
    SysctlBoundedArgs::new(ICMPV6CTL_ND6_DELAY, &ND6_DELAY, 0, INT_MAX),
    SysctlBoundedArgs::new(ICMPV6CTL_ND6_UMAXTRIES, &ND6_UMAXTRIES, 0, INT_MAX),
    SysctlBoundedArgs::new(ICMPV6CTL_ND6_MMAXTRIES, &ND6_MMAXTRIES, 0, INT_MAX),
    SysctlBoundedArgs::new(ICMPV6CTL_MTUDISC_HIWAT, &ICMP6_MTUDISC_HIWAT, 0, INT_MAX),
    SysctlBoundedArgs::new(ICMPV6CTL_MTUDISC_LOWAT, &ICMP6_MTUDISC_LOWAT, 0, INT_MAX),
    SysctlBoundedArgs::new(ICMPV6CTL_ERRPPSLIMIT, &ICMP6ERRPPSLIM, -1, 1000),
];

/// Copies `v` into `buf` at `at`. `T` is one of the plain-data packet structures (`#[repr(C)]`,
/// no padding, any bit pattern valid), whose bytes are the wire format.
fn put<T: Copy>(buf: &mut [u8], at: usize, v: &T) {
    let dst = &mut buf[at..at + size_of::<T>()];
    // SAFETY: `v` is a `T`, readable for `size_of::<T>()` bytes with no padding (see above);
    // `dst` is exactly that long and does not overlap a borrowed `T`.
    unsafe { ptr::copy_nonoverlapping(ptr::from_ref(v).cast::<u8>(), dst.as_mut_ptr(), dst.len()) };
}

/// Reads the `T` at `p` (packet data has no alignment guarantee).
///
/// # Safety
///
/// `p` points at `size_of::<T>()` readable bytes; `T` is plain data valid for any bit pattern.
unsafe fn read_at<T: Copy>(p: *const u8) -> T {
    // SAFETY: the caller's contract.
    unsafe { ptr::read_unaligned(p.cast::<T>()) }
}

/// `icmp6_init`: initializes MLD, the path MTU and redirect timeout queues.
pub fn icmp6_init() {
    mld6_init();
    rt_timer_queue_init(
        &ICMP6_MTUDISC_TIMEOUT_Q,
        IP6_MTUDISC_TIMEOUT.load(Ordering::Relaxed),
        Some(icmp6_mtudisc_timeout),
    );
    rt_timer_queue_init(
        &ICMP6_REDIRECT_TIMEOUT_Q,
        ICMP6_REDIRTIMEOUT.load(Ordering::Relaxed),
        None,
    );
    // icmp6counters = counters_alloc(icp6s_ncounters): a static (the module's deviations).
}

/// `icmp6_errcount`: counts an error message of `type_` and `code` that we send.
pub fn icmp6_errcount(type_: u8, code: u8) {
    use Icmp6statCounters::*;
    let mut c = Icp6sOunknown;

    match type_ {
        ICMP6_DST_UNREACH => match code {
            ICMP6_DST_UNREACH_NOROUTE => c = Icp6sOdstUnreachNoroute,
            ICMP6_DST_UNREACH_ADMIN => c = Icp6sOdstUnreachAdmin,
            ICMP6_DST_UNREACH_BEYONDSCOPE => c = Icp6sOdstUnreachBeyondscope,
            ICMP6_DST_UNREACH_ADDR => c = Icp6sOdstUnreachAddr,
            ICMP6_DST_UNREACH_NOPORT => c = Icp6sOdstUnreachNoport,
            _ => {}
        },
        ICMP6_PACKET_TOO_BIG => c = Icp6sOpacketTooBig,
        ICMP6_TIME_EXCEEDED => match code {
            ICMP6_TIME_EXCEED_TRANSIT => c = Icp6sOtimeExceedTransit,
            ICMP6_TIME_EXCEED_REASSEMBLY => c = Icp6sOtimeExceedReassembly,
            _ => {}
        },
        ICMP6_PARAM_PROB => match code {
            ICMP6_PARAMPROB_HEADER => c = Icp6sOparamprobHeader,
            ICMP6_PARAMPROB_NEXTHEADER => c = Icp6sOparamprobNextheader,
            ICMP6_PARAMPROB_OPTION => c = Icp6sOparamprobOption,
            _ => {}
        },
        ND_REDIRECT => c = Icp6sOredirect,
        _ => {}
    }

    icmp6stat_inc(c);
}

/// `icmp6_mtudisc_callback_register`: registers a function to call on path MTU changes
/// (once: registering it again does nothing).
pub fn icmp6_mtudisc_callback_register(func: Icmp6MtudiscCallbackFn) {
    // SAFETY: registrations happen while the protocols initialise, on the boot CPU, before
    // `icmp6_mtudisc_update` can run and read the list.
    let callbacks = unsafe { ICMP6_MTUDISC_CALLBACKS.get_mut() };
    if callbacks.iter().any(|f| *f as usize == func as usize) {
        return;
    }
    // LIST_INSERT_HEAD
    callbacks.insert(0, func);
}

/// `icmp6_do_error`: builds the ICMPv6 error of `type_`/`code` (`param`: pointer or
/// MTU) about packet `m`, which it consumes; `None` when no error is sent.
pub fn icmp6_do_error(m: &'static Mbuf, type_: u8, code: u8, param: i32) -> Option<&'static Mbuf> {
    icmp6stat_inc(Icmp6statCounters::Icp6sError);

    // count per-type-code statistics
    icmp6_errcount(type_, code);

    let mut m = m;
    if (m.m_len().get() as usize) < size_of::<Ip6Hdr>() {
        m = m_pullup(m, size_of::<Ip6Hdr>() as i32)?;
    }
    let oip6 = mtod_ip6(m);

    'freeit: {
        // If the destination address of the erroneous packet is a multicast address, or the
        // packet was sent using link-layer multicast, we should basically suppress sending an
        // error (RFC 2463, Section 2.4). We have two exceptions (the item e.2 in that
        // section):
        // - the Packet Too Big message can be sent for path MTU discovery.
        // - the Parameter Problem Message that can be allowed an icmp6 error in the option
        //   type field. This check has been done in ip6_unknown_opt(), so we can just check
        //   the type and code.
        if (m.m_flags().get() & (M_BCAST | M_MCAST) != 0 || in6_is_addr_multicast(&oip6.ip6_dst))
            && (type_ != ICMP6_PACKET_TOO_BIG
                && (type_ != ICMP6_PARAM_PROB || code != ICMP6_PARAMPROB_OPTION))
        {
            break 'freeit;
        }

        // RFC 2463, 2.4 (e.5): source address check.
        // XXX: the case of anycast source?
        if in6_is_addr_unspecified(&oip6.ip6_src) || in6_is_addr_multicast(&oip6.ip6_src) {
            break 'freeit;
        }

        // If we are about to send ICMPv6 against ICMPv6 error/redirect, don't do it.
        let mut nxt = -1;
        if let Some(off) = ip6_lasthdr(m, 0, IPPROTO_IPV6, &mut nxt)
            && nxt == IPPROTO_ICMPV6
        {
            let mut mp = Some(m);
            let Some(icp) = ip6_exthdr_get(&mut mp, off, size_of::<Icmp6Hdr>() as i32) else {
                icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
                return None;
            };
            // SAFETY: ip6_exthdr_get made the header's bytes readable.
            let icmp6_type = unsafe { *icp };
            if icmp6_type < ICMP6_ECHO_REQUEST || icmp6_type == ND_REDIRECT {
                // ICMPv6 error. Special case: for redirect (which is informational) we must
                // not send icmp6 error.
                icmp6stat_inc(Icmp6statCounters::Icp6sCanterror);
                break 'freeit;
            }
            // ICMPv6 informational - send the error
        }
        // non-ICMPv6 - send the error

        let oip6 = mtod_ip6(m); // adjust pointer

        // Finally, do rate limitation check.
        if icmp6_ratelimit(&oip6.ip6_src, type_, code) {
            icmp6stat_inc(Icmp6statCounters::Icp6sToofreq);
            break 'freeit;
        }

        // OK, ICMP6 can be generated.

        if m.m_pkthdr().len.get() as usize >= ICMPV6_PLD_MAXLEN {
            m_adj(m, ICMPV6_PLD_MAXLEN as i32 - m.m_pkthdr().len.get());
        }

        // The quoted packet carries no embedded scope zone; the new header does (it is only
        // an interface-local construct for routing).
        let mut quoted = oip6;
        if in6_is_scope_embed(&quoted.ip6_src) {
            quoted.ip6_src.set_s6_addr16(1, 0);
        }
        if in6_is_scope_embed(&quoted.ip6_dst) {
            quoted.ip6_dst.set_s6_addr16(1, 0);
        }
        mtod_ip6_store(m, &quoted);

        let preplen = size_of::<Ip6Hdr>() + size_of::<Icmp6Hdr>();
        let mut m = m_prepend(m, preplen as i32, M_DONTWAIT)?;
        if (m.m_len().get() as usize) < preplen {
            m = m_pullup(m, preplen as i32)?;
        }

        let mut nip6 = Ip6Hdr::zeroed();
        nip6.ip6_src = oip6.ip6_src;
        nip6.ip6_dst = oip6.ip6_dst;
        mtod_ip6_store(m, &nip6);

        let mut icmp6 = Icmp6Hdr::zeroed();
        icmp6.icmp6_type = type_;
        icmp6.icmp6_code = code;
        icmp6.set_icmp6_pptr(htonl(param as u32));
        // SAFETY: the first mbuf holds `preplen` bytes; the ICMPv6 header follows the IPv6 one.
        unsafe {
            ptr::write_unaligned(
                mtod::<u8>(m).add(size_of::<Ip6Hdr>()).cast::<Icmp6Hdr>(),
                icmp6,
            )
        };

        // icmp6_reflect() is designed to be in the input path. icmp6_error() can be called
        // from both input and output path, and if we are in output path rcvif could contain
        // bogus value. clear m->m_pkthdr.ph_ifidx for safety, we should have enough scope
        // information in ip header (nip6).
        m.m_pkthdr().ph_ifidx.set(0);

        icmp6stat_inc_hist(Icmp6statCounters::Icp6sOuthist, type_);

        return Some(m);
    }
    // freeit: if we can't tell whether or not we can generate ICMP6, free it.
    m_freem(m);
    None
}

/// `icmp6_error`: generates an error packet of `type_`/`code` in response to bad IP6 packet
/// `m` (consumed).
pub fn icmp6_error(m: &'static Mbuf, type_: u8, code: u8, param: i32) {
    if let Some(n) = icmp6_do_error(m, type_, code, param) {
        // header order: IPv6 - ICMPv6
        let mut n = Some(n);
        if icmp6_reflect(&mut n, size_of::<Ip6Hdr>(), None).is_ok()
            && let Some(n) = n
        {
            ip6_send(n);
        }
    }
}

/// What `icmp6_input` does after its message-specific processing.
enum Next {
    /// `deliver`: the message is an error for the protocol it quotes; `PRC_*` code.
    Deliver(i32),
    /// `badcode`.
    Badcode,
    /// `badlen`.
    Badlen,
    /// `break` to `raw`: give the packet to the raw sockets.
    Raw,
}

/// `icmp6_input`: processes a received ICMP6 message; ICMPv6's `pr_input`.
pub fn icmp6_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };
    let off = *offp;
    let icmp6len = m.m_pkthdr().len.get() - off;

    'freeit: {
        // Locate icmp6 structure in mbuf, and check that not corrupted and of at least
        // minimum length
        if icmp6len < size_of::<Icmp6Hdr>() as i32 {
            icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
            break 'freeit;
        }

        // calculate the checksum
        let Some(icmp6p) = ip6_exthdr_get(mp, off, size_of::<Icmp6Hdr>() as i32) else {
            icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
            return IPPROTO_DONE;
        };
        // SAFETY: ip6_exthdr_get made the ICMPv6 header's bytes readable.
        let icmp6: Icmp6Hdr = unsafe { read_at(icmp6p) };
        let icmp6_type = icmp6.icmp6_type;
        let code = icmp6.icmp6_code;

        if in6_cksum(m, IPPROTO_ICMPV6 as u8, off as u32, icmp6len as u32) != 0 {
            icmp6stat_inc(Icmp6statCounters::Icp6sChecksum);
            break 'freeit;
        }

        'raw: {
            let pf = &m.m_pkthdr().pf;
            if pf.flags.get() & PF_TAG_DIVERTED != 0 {
                match icmp6_type {
                    // These ICMP6 types map to other connections. They must be delivered to
                    // pr_ctlinput() also for diverted connections.
                    ICMP6_DST_UNREACH | ICMP6_PACKET_TOO_BIG | ICMP6_TIME_EXCEEDED
                    | ICMP6_PARAM_PROB => {
                        // Do not use the divert-to property of the TCP or UDP rule when doing
                        // the PCB lookup for the raw socket.
                        pf.flags.set(pf.flags.get() & !PF_TAG_DIVERTED);
                    }
                    _ => break 'raw,
                }
            }

            // NCARP > 0: carp_lsdrop of echo requests; not configured.
            icmp6stat_inc_hist(Icmp6statCounters::Icp6sInhist, icmp6_type);

            let next = match icmp6_type {
                ICMP6_DST_UNREACH => match code {
                    ICMP6_DST_UNREACH_NOROUTE => Next::Deliver(PRC_UNREACH_NET),
                    // is this a good code?
                    ICMP6_DST_UNREACH_ADMIN => Next::Deliver(PRC_UNREACH_PROTOCOL),
                    ICMP6_DST_UNREACH_ADDR => Next::Deliver(PRC_HOSTDEAD),
                    // I mean "source address was incorrect."
                    ICMP6_DST_UNREACH_BEYONDSCOPE => Next::Deliver(PRC_PARAMPROB),
                    ICMP6_DST_UNREACH_NOPORT => Next::Deliver(PRC_UNREACH_PORT),
                    _ => Next::Badcode,
                },

                // MTU is checked in icmp6_mtudisc_update. Updating the path MTU will be done
                // after examining intermediate extension headers.
                ICMP6_PACKET_TOO_BIG => Next::Deliver(PRC_MSGSIZE),

                ICMP6_TIME_EXCEEDED => match code {
                    ICMP6_TIME_EXCEED_TRANSIT => Next::Deliver(PRC_TIMXCEED_INTRANS),
                    ICMP6_TIME_EXCEED_REASSEMBLY => Next::Deliver(PRC_TIMXCEED_REASS),
                    _ => Next::Badcode,
                },

                ICMP6_PARAM_PROB => match code {
                    ICMP6_PARAMPROB_NEXTHEADER => Next::Deliver(PRC_UNREACH_PROTOCOL),
                    ICMP6_PARAMPROB_HEADER | ICMP6_PARAMPROB_OPTION => Next::Deliver(PRC_PARAMPROB),
                    _ => Next::Badcode,
                },

                ICMP6_ECHO_REQUEST => {
                    if code != 0 {
                        Next::Badcode
                    } else if icmp6_echo_input(m, off, icmp6) {
                        Next::Raw
                    } else {
                        // The packet went to the querier only.
                        *mp = None;
                        return IPPROTO_DONE;
                    }
                }

                ICMP6_ECHO_REPLY => {
                    if code != 0 {
                        Next::Badcode
                    } else {
                        Next::Raw
                    }
                }

                MLD_LISTENER_QUERY | MLD_LISTENER_REPORT => {
                    if icmp6len < size_of::<MldHdr>() as i32 {
                        Next::Badlen
                    } else if let Some(n) = m_copym(m, 0, M_COPYALL, M_DONTWAIT) {
                        mld6_input(n, off);
                        // m stays.
                        Next::Raw
                    } else {
                        // give up local
                        mld6_input(m, off);
                        *mp = None;
                        return IPPROTO_DONE;
                    }
                }

                MLD_LISTENER_DONE => {
                    if icmp6len < size_of::<MldHdr>() as i32 {
                        // necessary?
                        Next::Badlen
                    } else {
                        // nothing to be done in kernel
                        Next::Raw
                    }
                }

                // XXX: these two are experimental. not officially defined.
                // XXX: per-interface statistics?
                // just pass it to applications
                MLD_MTRACE_RESP | MLD_MTRACE => Next::Raw,

                // IPv6 Node Information Queries are not supported (ICMP6_FQDN_QUERY).
                ICMP6_WRUREQUEST | ICMP6_WRUREPLY => Next::Raw,

                ND_ROUTER_SOLICIT | ND_ROUTER_ADVERT => {
                    if code != 0 {
                        Next::Badcode
                    } else if (icmp6_type == ND_ROUTER_SOLICIT
                        && icmp6len < size_of::<NdRouterSolicit>() as i32)
                        || (icmp6_type == ND_ROUTER_ADVERT
                            && icmp6len < size_of::<NdRouterAdvert>() as i32)
                    {
                        Next::Badlen
                    } else if let Some(n) = m_copym(m, 0, M_COPYALL, M_DONTWAIT) {
                        nd6_rtr_cache(n, off, icmp6len, icmp6_type);
                        // m stays.
                        Next::Raw
                    } else {
                        // give up local
                        nd6_rtr_cache(m, off, icmp6len, icmp6_type);
                        *mp = None;
                        return IPPROTO_DONE;
                    }
                }

                ND_NEIGHBOR_SOLICIT => {
                    if code != 0 {
                        Next::Badcode
                    } else if icmp6len < size_of::<NdNeighborSolicit>() as i32 {
                        Next::Badlen
                    } else if let Some(n) = m_copym(m, 0, M_COPYALL, M_DONTWAIT) {
                        nd6_ns_input(n, off, icmp6len);
                        // m stays.
                        Next::Raw
                    } else {
                        // give up local
                        nd6_ns_input(m, off, icmp6len);
                        *mp = None;
                        return IPPROTO_DONE;
                    }
                }

                ND_NEIGHBOR_ADVERT => {
                    if code != 0 {
                        Next::Badcode
                    } else if icmp6len < size_of::<NdNeighborAdvert>() as i32 {
                        Next::Badlen
                    } else if let Some(n) = m_copym(m, 0, M_COPYALL, M_DONTWAIT) {
                        nd6_na_input(n, off, icmp6len);
                        // m stays.
                        Next::Raw
                    } else {
                        // give up local
                        nd6_na_input(m, off, icmp6len);
                        *mp = None;
                        return IPPROTO_DONE;
                    }
                }

                ND_REDIRECT => {
                    if code != 0 {
                        Next::Badcode
                    } else if icmp6len < size_of::<NdRedirect>() as i32 {
                        Next::Badlen
                    } else if let Some(n) = m_copym(m, 0, M_COPYALL, M_DONTWAIT) {
                        icmp6_redirect_input(n, off);
                        // m stays.
                        Next::Raw
                    } else {
                        // give up local
                        icmp6_redirect_input(m, off);
                        *mp = None;
                        return IPPROTO_DONE;
                    }
                }

                ICMP6_ROUTER_RENUMBERING => {
                    if code != ICMP6_ROUTER_RENUMBERING_COMMAND
                        && code != ICMP6_ROUTER_RENUMBERING_RESULT
                    {
                        Next::Badcode
                    } else if icmp6len < size_of::<Icmp6RouterRenum>() as i32 {
                        Next::Badlen
                    } else {
                        Next::Raw
                    }
                }

                _ => {
                    if icmp6_type < ICMP6_ECHO_REQUEST {
                        // ICMPv6 error: MUST deliver it by spec...
                        Next::Deliver(PRC_NCMDS as i32)
                    } else {
                        // ICMPv6 informational: MUST not deliver
                        Next::Raw
                    }
                }
            };

            match next {
                Next::Deliver(prc) => {
                    if !icmp6_notify_error(m, off, icmp6len, prc) {
                        // In this case, m should've been freed.
                        *mp = None;
                        return IPPROTO_DONE;
                    }
                }
                Next::Badcode => icmp6stat_inc(Icmp6statCounters::Icp6sBadcode),
                Next::Badlen => icmp6stat_inc(Icmp6statCounters::Icp6sBadlen),
                Next::Raw => {}
            }
        }
        // raw: deliver the packet to appropriate sockets
        return rip6_input(mp, offp, proto, af, ns);
    }
    // freeit:
    m_freem(m);
    *mp = None;
    IPPROTO_DONE
}

/// The echo request case of `icmp6_input`: sends the echo reply. `true` when `m` is still ours
/// for the raw sockets, `false` when it was used up as the reply.
fn icmp6_echo_input(m: &'static Mbuf, off: i32, icmp6: Icmp6Hdr) -> bool {
    let (reply, local) = icmp6_echo_reply(m, off, icmp6);
    if let Some(n) = reply {
        ip6_send(n);
    }
    local
}

/// The echo request case of `icmp6_input` up to the sending: copies the packet `m` to send to
/// two data paths, userland socket(s) (`m`) and the querier (echo reply, `n`); returns the
/// reply, reflected and ready for `ip6_send`, and whether `m` is still ours for the raw
/// sockets (`false` when it was used up as the reply). (The C does the sending inline; the
/// split lets the host tests look at the reply.)
fn icmp6_echo_reply(m: &'static Mbuf, off: i32, icmp6: Icmp6Hdr) -> (Option<&'static Mbuf>, bool) {
    let mut local = true;
    let mut n: Option<&'static Mbuf>;
    let mut noff = off;
    let mut nicmp6: Option<*mut u8> = None;
    let mut fresh = false;
    let mut reply = None;

    'deliverecho: {
        let Some(n0) = m_copym(m, 0, M_COPYALL, M_DONTWAIT) else {
            // Give up local
            n = Some(m);
            local = false;
            break 'deliverecho;
        };
        n = Some(n0);

        // If the first mbuf is shared, or the first mbuf is too short, copy the first part of
        // the data into a fresh mbuf. Otherwise, we will wrongly overwrite both copies.
        if n0.m_flags().get() & M_EXT == 0
            && n0.m_len().get() as usize >= off as usize + size_of::<Icmp6Hdr>()
        {
            break 'deliverecho;
        }

        const MAXLEN: usize = size_of::<Ip6Hdr>() + size_of::<Icmp6Hdr>();

        // Prepare an internal mbuf. m_pullup() doesn't always copy the length we specified.
        if MAXLEN >= MCLBYTES {
            // Give up remote
            m_freem(n0);
            return (None, local);
        }
        let mut nn = m_gethdr(M_DONTWAIT, i32::from(n0.m_type().get()));
        if let Some(x) = nn
            && MAXLEN >= MHLEN
        {
            mclget(x, M_DONTWAIT);
            if x.m_flags().get() & M_EXT == 0 {
                m_free(x);
                nn = None;
            }
        }
        let Some(nn) = nn else {
            // Give up local
            m_freem(n0);
            n = Some(m);
            local = false;
            break 'deliverecho;
        };
        m_move_pkthdr(nn, n0);

        // Copy IPv6 and ICMPv6 only.
        nn.m_len().set(MAXLEN as u32);
        mtod_ip6_store(nn, &mtod_ip6(m));
        let hdr = mtod::<u8>(nn).wrapping_add(size_of::<Ip6Hdr>());
        // SAFETY: the new mbuf holds `MAXLEN` bytes at its data (set just above); the ICMPv6
        // header follows the IPv6 one.
        unsafe { ptr::write_unaligned(hdr.cast::<Icmp6Hdr>(), icmp6) };
        noff = size_of::<Ip6Hdr>() as i32;

        // Adjust mbuf. ip6_plen will be adjusted in ip6_output(). n->m_pkthdr.len ==
        // n0->m_pkthdr.len at this point.
        let len = nn.m_pkthdr().len.get() + MAXLEN as i32 - (off + size_of::<Icmp6Hdr>() as i32);
        nn.m_pkthdr().len.set(len);
        m_adj(n0, off + size_of::<Icmp6Hdr>() as i32);
        nn.m_next().set(Some(n0));

        n = Some(nn);
        nicmp6 = Some(hdr);
        fresh = true;
    }

    if !fresh {
        // deliverecho:
        nicmp6 = ip6_exthdr_get(&mut n, off, size_of::<Icmp6Hdr>() as i32);
        noff = off;
    }
    if let (Some(nn), Some(p)) = (n, nicmp6) {
        // SAFETY: `p` is the start of the ICMPv6 header in `nn` (two bytes are written).
        unsafe {
            *p = ICMP6_ECHO_REPLY;
            *p.add(1) = 0;
        }
        icmp6stat_inc(Icmp6statCounters::Icp6sReflect);
        icmp6stat_inc_hist(Icmp6statCounters::Icp6sOuthist, ICMP6_ECHO_REPLY);
        let mut nn = Some(nn);
        if icmp6_reflect(&mut nn, noff as usize, None).is_ok() {
            reply = nn;
        }
    }
    (reply, local)
}

/// `icmp6_notify_error`: finds the protocol an ICMPv6 error quotes and hands the error to its
/// `pr_ctlinput` with `code` (a `PRC_*`). `false` when `m` was freed.
fn icmp6_notify_error(m: &'static Mbuf, off: i32, icmp6len: i32, code: i32) -> bool {
    let mut mp = Some(m);

    if icmp6len < (size_of::<Icmp6Hdr>() + size_of::<Ip6Hdr>()) as i32 {
        icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
        m_freem(m);
        return false;
    }
    let Some(icmp6) = ip6_exthdr_get(
        &mut mp,
        off,
        (size_of::<Icmp6Hdr>() + size_of::<Ip6Hdr>()) as i32,
    ) else {
        icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
        return false;
    };
    // SAFETY: ip6_exthdr_get made the ICMPv6 and the quoted IPv6 header contiguous.
    let eip6: Ip6Hdr = unsafe { read_at(icmp6.add(size_of::<Icmp6Hdr>())) };

    // Detect the upper level protocol
    let mut nxt = eip6.ip6_nxt;
    let mut eoff = off + (size_of::<Icmp6Hdr>() + size_of::<Ip6Hdr>()) as i32;
    // SAFETY: as above.
    let icmp6type = unsafe { *icmp6 };
    let mut finaldst: Option<In6Addr> = None;

    // XXX: should avoid infinite loop explicitly?
    loop {
        match i32::from(nxt) {
            IPPROTO_HOPOPTS | IPPROTO_DSTOPTS | IPPROTO_AH => {
                let Some(eh) = ip6_exthdr_get(&mut mp, eoff, size_of::<Ip6Ext>() as i32) else {
                    icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
                    return false;
                };
                // SAFETY: ip6_exthdr_get made the extension header's bytes readable.
                let eh: Ip6Ext = unsafe { read_at(eh) };

                if i32::from(nxt) == IPPROTO_AH {
                    eoff += (i32::from(eh.ip6e_len) + 2) << 2;
                } else {
                    eoff += (i32::from(eh.ip6e_len) + 1) << 3;
                }
                nxt = eh.ip6e_nxt;
            }
            IPPROTO_ROUTING => {
                // When the erroneous packet contains a routing header, we should examine the
                // header to determine the final destination. Otherwise, we can't properly
                // update information that depends on the final destination (e.g. path MTU).
                let Some(rth) = ip6_exthdr_get(&mut mp, eoff, size_of::<Ip6Rthdr>() as i32) else {
                    icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
                    return false;
                };
                // SAFETY: ip6_exthdr_get made the routing header's bytes readable.
                let rth: Ip6Rthdr = unsafe { read_at(rth) };
                let rthlen = (i32::from(rth.ip6r_len) + 1) << 3;

                // XXX: currently there is no officially defined type other than type-0. Note
                // that if the segment left field is 0, all intermediate hops must have been
                // passed.
                if rth.ip6r_segleft != 0 && i32::from(rth.ip6r_type) == IPV6_RTHDR_TYPE_0 {
                    let Some(rth0) = ip6_exthdr_get(&mut mp, eoff, rthlen) else {
                        icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
                        return false;
                    };
                    // SAFETY: as above: `rthlen` bytes are readable.
                    let rth0h: Ip6Rthdr0 = unsafe { read_at(rth0) };
                    // just ignore a bogus header
                    let hops = usize::from(rth0h.ip6r0_len / 2);
                    if rth0h.ip6r0_len.is_multiple_of(2) && hops != 0 {
                        // SAFETY: the header is `rthlen` = 8 + 16 * hops bytes long, so the
                        // last of its `hops` addresses lies inside it.
                        finaldst = Some(unsafe {
                            read_at(rth0.add(size_of::<Ip6Rthdr0>() + (hops - 1) * 16))
                        });
                    }
                }
                eoff += rthlen;
                nxt = rth.ip6r_nxt;
            }
            IPPROTO_FRAGMENT => {
                let Some(fh) = ip6_exthdr_get(&mut mp, eoff, size_of::<Ip6Frag>() as i32) else {
                    icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
                    return false;
                };
                // SAFETY: ip6_exthdr_get made the fragment header's bytes readable.
                let fh: Ip6Frag = unsafe { read_at(fh) };
                // Data after a fragment header is meaningless unless it is the first
                // fragment, but we'll go to the notify label for path MTU discovery.
                if fh.ip6f_offlg & IP6F_OFF_MASK != 0 {
                    break;
                }

                eoff += size_of::<Ip6Frag>() as i32;
                nxt = fh.ip6f_nxt;
            }
            // This case includes ESP and the No Next Header. In such cases going to the
            // notify label does not have any meaning (i.e. ctlfunc will be NULL), but we go
            // anyway since we might have to update path MTU information.
            _ => break,
        }
    }

    // notify:
    let Some(icmp6) = ip6_exthdr_get(
        &mut mp,
        off,
        (size_of::<Icmp6Hdr>() + size_of::<Ip6Hdr>()) as i32,
    ) else {
        icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
        return false;
    };
    // SAFETY: as above.
    let eip6: Ip6Hdr = unsafe { read_at(icmp6.add(size_of::<Icmp6Hdr>())) };
    // SAFETY: as above.
    let icmp6h: Icmp6Hdr = unsafe { read_at(icmp6) };
    let ifidx = m.m_pkthdr().ph_ifidx.get();

    let mut icmp6dst = SockaddrIn6::zeroed();
    icmp6dst.sin6_len = size_of::<SockaddrIn6>() as u8;
    icmp6dst.sin6_family = AF_INET6;
    icmp6dst.sin6_addr = finaldst.unwrap_or(eip6.ip6_dst);
    icmp6dst.sin6_scope_id = in6_addr2scopeid(ifidx, &icmp6dst.sin6_addr) as u32;
    let mut a = icmp6dst.sin6_addr;
    if in6_embedscope(&mut a, &icmp6dst, None, None).is_err() {
        // interface went away
        m_freem(m);
        return false;
    }
    icmp6dst.sin6_addr = a;

    // retrieve parameters from the inner IPv6 header, and convert them into sockaddr
    // structures.
    let mut icmp6src = SockaddrIn6::zeroed();
    icmp6src.sin6_len = size_of::<SockaddrIn6>() as u8;
    icmp6src.sin6_family = AF_INET6;
    icmp6src.sin6_addr = eip6.ip6_src;
    icmp6src.sin6_scope_id = in6_addr2scopeid(ifidx, &icmp6src.sin6_addr) as u32;
    let mut a = icmp6src.sin6_addr;
    if in6_embedscope(&mut a, &icmp6src, None, None).is_err() {
        // interface went away
        m_freem(m);
        return false;
    }
    icmp6src.sin6_addr = a;
    icmp6src.sin6_flowinfo = eip6.ip6_flow & IPV6_FLOWLABEL_MASK;

    let mut finaldst = finaldst.unwrap_or(eip6.ip6_dst);
    let mut notifymtu = ntohl(icmp6h.icmp6_mtu());
    let mut ip6cp = Ip6ctlparam::new();
    ip6cp.ip6c_m = Some(m);
    ip6cp.ip6c_icmp6 = icmp6.cast::<Icmp6Hdr>();
    ip6cp.ip6c_ip6 = icmp6.wrapping_add(size_of::<Icmp6Hdr>()).cast::<Ip6Hdr>();
    ip6cp.ip6c_off = eoff;
    ip6cp.ip6c_finaldst = ptr::from_mut(&mut finaldst);
    ip6cp.ip6c_src = ptr::from_mut(&mut icmp6src);
    ip6cp.ip6c_nxt = nxt;
    pf_pkt_addr_changed(m);

    if icmp6type == ICMP6_PACKET_TOO_BIG {
        ip6cp.ip6c_cmdarg = ptr::from_mut(&mut notifymtu).cast::<c_void>();
    }

    let ctlfunc: Option<PrCtlinputFn> =
        INET6SW[usize::from(IP6_PROTOX[usize::from(nxt)].load(Ordering::Relaxed))].pr_ctlinput;
    if let Some(ctlfunc) = ctlfunc {
        // SAFETY: `icmp6dst` is a local `sockaddr_in6`; `ip6cp` and what it points at (the
        // message in `m`, the locals above) outlive the call.
        unsafe {
            ctlfunc(
                code,
                sin6tosa(&mut icmp6dst),
                m.m_pkthdr().ph_rtableid.get(),
                ptr::from_mut(&mut ip6cp).cast::<c_void>(),
            )
        };
    }
    true
}

/// `icmp6_mtudisc_update`: a validated (or not) Packet Too Big: updates the path MTU
/// of the destination in `ip6cp`.
pub fn icmp6_mtudisc_update(ip6cp: &Ip6ctlparam, validated: bool) {
    let Some(m) = ip6cp.ip6c_m else {
        return;
    };
    // SAFETY: `ip6c_finaldst` and `ip6c_icmp6` point at the quoted address and the message in
    // the packet `m` (`icmp6_notify_error` set them; `m` is alive for the call).
    let (dst, icmp6): (In6Addr, Icmp6Hdr) = unsafe {
        (
            read_at(ip6cp.ip6c_finaldst.cast_const().cast::<u8>()),
            read_at(ip6cp.ip6c_icmp6.cast_const().cast::<u8>()),
        )
    };
    let mtu = ntohl(icmp6.icmp6_mtu());

    if mtu < IPV6_MMTU {
        return;
    }

    // allow non-validated cases if memory is plenty, to make traffic from non-connected pcb
    // happy.
    let rtcount = rt_timer_queue_count(&ICMP6_MTUDISC_TIMEOUT_Q);
    let hiwat = ICMP6_MTUDISC_HIWAT.load(Ordering::Relaxed);
    let lowat = ICMP6_MTUDISC_LOWAT.load(Ordering::Relaxed);
    if validated {
        if rtcount > hiwat as u64 {
            return;
        }
        // else if rtcount > lowat: XXX nuke a victim, install the new one.
    } else if rtcount > lowat as u64 {
        return;
    }

    let ifidx = m.m_pkthdr().ph_ifidx.get();
    let mut sin6 = SockaddrIn6::zeroed();
    sin6.sin6_family = AF_INET6;
    sin6.sin6_len = size_of::<SockaddrIn6>() as u8;
    sin6.sin6_addr = dst;
    // XXX normally, this won't happen
    if in6_is_addr_linklocal(&dst) {
        sin6.sin6_addr.set_s6_addr16(1, htons(ifidx as u16));
    }
    sin6.sin6_scope_id = in6_addr2scopeid(ifidx, &sin6.sin6_addr) as u32;

    let rtableid = m.m_pkthdr().ph_rtableid.get();
    let rt = icmp6_mtudisc_clone(&sin6, rtableid, false);

    if let Some(rt) = rt
        && rt.rt_flags.get() & RTF_HOST != 0
        && rt.rt_locks().get() & RTV_MTU == 0
    {
        let rtmtu = rt.rt_mtu().load(Ordering::Relaxed);
        if rtmtu > mtu || rtmtu == 0 {
            let ifp = if_get(rt.rt_ifidx.get());
            if let Some(ifp) = ifp
                && mtu < ifp.if_mtu.get()
            {
                icmp6stat_inc(Icmp6statCounters::Icp6sPmtuchg);
                let _ =
                    rt.rt_mtu()
                        .compare_exchange(rtmtu, mtu, Ordering::Relaxed, Ordering::Relaxed);
            }
            if_put(ifp);
        }
    }
    rtfree(rt);

    // Notify protocols that the MTU for this destination has changed.
    // SAFETY: the list is only written at protocol initialisation.
    for func in unsafe { ICMP6_MTUDISC_CALLBACKS.get() } {
        func(&sin6, rtableid);
    }
}

/// `icmp6_reflect`: reflects the ip6 packet back to the source. `off` points to the icmp6
/// header, counted from the top of the mbuf. The source address comes from `sa` if given; the
/// packet is consumed on error.
pub fn icmp6_reflect(
    mp: &mut Option<&'static Mbuf>,
    off: usize,
    sa: Option<&SockaddrIn6>,
) -> Result<(), Errno> {
    const HDRLEN: usize = size_of::<Ip6Hdr>() + size_of::<Icmp6Hdr>();
    let Some(mut m) = *mp else {
        return Err(Errno::EHOSTUNREACH);
    };

    'bad: {
        // too short to reflect
        if off < size_of::<Ip6Hdr>() {
            break 'bad;
        }

        let loopcnt = m.m_pkthdr().ph_loopcnt.get();
        m.m_pkthdr().ph_loopcnt.set(loopcnt.wrapping_add(1));
        if loopcnt >= M_MAXLOOP {
            m_freemp(mp);
            return Err(Errno::ELOOP);
        }
        let rtableid = m.m_pkthdr().ph_rtableid.get();
        let pfflags = m.m_pkthdr().pf.flags.get();
        m_resethdr(m);
        m.m_pkthdr().ph_rtableid.set(rtableid);
        m.m_pkthdr().pf.flags.set(pfflags & PF_TAG_GENERATED);

        // If there are extra headers between IPv6 and ICMPv6, strip off that header first.
        if off > size_of::<Ip6Hdr>() {
            let l = off - size_of::<Ip6Hdr>();
            let mut nip6 = [0u8; size_of::<Ip6Hdr>()];
            m_copydata(m, 0, &mut nip6);
            m_adj(m, l as i32);
            if (m.m_len().get() as usize) < HDRLEN {
                let Some(mm) = m_pullup(m, HDRLEN as i32) else {
                    *mp = None;
                    return Err(Errno::EMSGSIZE);
                };
                m = mm;
                *mp = Some(m);
            }
            // SAFETY: the first mbuf holds at least `HDRLEN` bytes.
            unsafe { ptr::copy_nonoverlapping(nip6.as_ptr(), mtod::<u8>(m), nip6.len()) };
        } else {
            // off == sizeof(struct ip6_hdr)
            if (m.m_len().get() as usize) < HDRLEN {
                let Some(mm) = m_pullup(m, HDRLEN as i32) else {
                    *mp = None;
                    return Err(Errno::EMSGSIZE);
                };
                m = mm;
                *mp = Some(m);
            }
        }
        let mut ip6 = mtod_ip6(m);
        ip6.ip6_nxt = IPPROTO_ICMPV6 as u8;

        let t = ip6.ip6_dst;
        // ip6_input() drops a packet if its src is multicast. So, the src is never
        // multicast.
        ip6.ip6_dst = ip6.ip6_src;

        // XXX: make sure to embed scope zone information, using already embedded IDs or the
        // received interface (if any). Note that rcvif may be NULL.
        // TODO: scoped routing case (XXX).
        let mut sa6_src = SockaddrIn6::zeroed();
        sa6_src.sin6_family = AF_INET6;
        sa6_src.sin6_len = size_of::<SockaddrIn6>() as u8;
        sa6_src.sin6_addr = ip6.ip6_dst;
        let mut sa6_dst = SockaddrIn6::zeroed();
        sa6_dst.sin6_family = AF_INET6;
        sa6_dst.sin6_len = size_of::<SockaddrIn6>() as u8;
        sa6_dst.sin6_addr = t;

        let mut src: Option<In6Addr> = None;
        let mut key = match sa {
            Some(sa) => *sa,
            None => sa6_src,
        };
        if sa.is_none() {
            // If the incoming packet was addressed directly to us (i.e. unicast), use dst as
            // the src for the reply. The IN6_IFF_TENTATIVE|IN6_IFF_DUPLICATED case would be
            // VERY rare, but is possible (for example) when we encounter an error while
            // forwarding procedure destined to a duplicated address of ours.
            // SAFETY: a local `sockaddr_in6`.
            let rt = unsafe { rtalloc(sin6tosa(&mut sa6_dst), 0, rtableid) };
            if let Some(r) = rt
                && rtisvalid(Some(r))
                && r.rt_flags.get() & RTF_LOCAL != 0
                && ifatoia6(r.ifa()).ia6_flags.get()
                    & (IN6_IFF_ANYCAST | IN6_IFF_TENTATIVE | IN6_IFF_DUPLICATED)
                    == 0
            {
                src = Some(t);
            }
            rtfree(rt);
        }

        let mut rt = None;
        if src.is_none() {
            // This case matches to multicasts, our anycast, or unicasts that we do not own.
            // Select a source address based on the source address of the erroneous packet.
            // SAFETY: `key` is a local `sockaddr_in6`.
            rt = unsafe { rtalloc(sin6tosa(&mut key), RT_RESOLVE, rtableid) };
            let Some(r) = rt.filter(|r| rtisvalid(Some(r))) else {
                rtfree(rt);
                break 'bad;
            };
            let ia6 = r
                .ifa()
                .ifa_ifp
                .get()
                .and_then(|ifp| in6_ifawithscope(ifp, &t, rtableid, Some(r)));
            let mut s = match ia6 {
                Some(ia6) => ia6_in6(ia6),
                None => ia6_in6(ifatoia6(r.ifa())),
            };

            // route sourceaddr may override src address selection
            if r.rt_flags.get() & RTF_GATEWAY != 0 {
                let sourceaddr = rtable_getsource(rtableid, AF_INET6);
                if !sourceaddr.is_null() {
                    // SAFETY: a preferred source is an interface address, readable.
                    let ifa = unsafe { ifa_ifwithaddr(sourceaddr, rtableid) };
                    if let Some(ifa) = ifa
                        && ifa
                            .ifa_ifp
                            .get()
                            .is_some_and(|ifp| ifp.if_flags.get() & IFF_UP != 0)
                    {
                        // SAFETY: an `AF_INET6` source is a `sockaddr_in6`.
                        s = unsafe { read_at::<SockaddrIn6>(sourceaddr.cast::<u8>()) }.sin6_addr;
                    }
                }
            }
            src = Some(s);
        }

        ip6.ip6_src = src.unwrap_or(t);
        rtfree(rt);

        ip6.ip6_flow = 0;
        ip6.set_ip6_vfc((ip6.ip6_vfc() & !crate::netinet::ip6::IPV6_VERSION_MASK) | IPV6_VERSION);
        ip6.ip6_nxt = IPPROTO_ICMPV6 as u8;
        ip6.ip6_hlim = IP6_DEFHLIM.load(Ordering::Relaxed) as u8;
        mtod_ip6_store(m, &ip6);

        // SAFETY: the first mbuf holds `HDRLEN` bytes; the checksum is the ICMPv6 header's
        // third and fourth byte.
        unsafe {
            ptr::write_unaligned(mtod::<u8>(m).add(size_of::<Ip6Hdr>() + 2).cast::<u16>(), 0)
        };
        m.m_pkthdr().csum_flags.set(M_ICMP_CSUM_OUT);

        // XXX option handling

        m.m_flags().set(m.m_flags().get() & !(M_BCAST | M_MCAST));
        return Ok(());
    }
    // bad:
    m_freemp(mp);
    Err(Errno::EHOSTUNREACH)
}

/// `icmp6_fasttimo`: the ICMPv6 fast timeout (MLD timers).
pub fn icmp6_fasttimo() {
    mld6_fasttimo();
}

/// `icmp6_redirect_input`: processes a received redirect at offset `off` of `m` (consumed).
pub fn icmp6_redirect_input(m: &'static Mbuf, off: i32) {
    let ip6 = mtod_ip6(m);
    let mut icmp6len = i32::from(ntohs(ip6.ip6_plen));
    let i_am_router = IP6_FORWARDING.load(Ordering::Relaxed) != 0;
    let src6 = ip6.ip6_src;

    let Some(ifp) = if_get(m.m_pkthdr().ph_ifidx.get()) else {
        m_freem(m);
        return;
    };

    // `bad` and `freeit` of the C: whether the packet counts as a bad redirect.
    let bad = 'out: {
        // if we are router, we don't update route by icmp6 redirect
        if i_am_router {
            break 'out false;
        }
        if ifp.if_xflags.get() & IFXF_AUTOCONF6 == 0 {
            break 'out false;
        }

        let mut mp = Some(m);
        let Some(nd_rd) = ip6_exthdr_get(&mut mp, off, icmp6len) else {
            icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
            if_put(ifp);
            return;
        };
        // SAFETY: ip6_exthdr_get made `icmp6len` bytes (the plen of a packet that passed
        // icmp6_input's length check against a `nd_redirect`) contiguous at `nd_rd`.
        let nd_rd_hdr: NdRedirect = unsafe { read_at(nd_rd) };
        let mut redtgt6 = nd_rd_hdr.nd_rd_target;
        let mut reddst6 = nd_rd_hdr.nd_rd_dst;

        if in6_is_addr_linklocal(&redtgt6) {
            redtgt6.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
        }
        if in6_is_addr_linklocal(&reddst6) {
            reddst6.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
        }

        // validation
        if !in6_is_addr_linklocal(&src6) {
            break 'out true;
        }
        if ip6.ip6_hlim != 255 {
            break 'out true;
        }
        if in6_is_addr_multicast(&reddst6) {
            break 'out true;
        }

        {
            // ip6->ip6_src must be equal to gw for icmp6->icmp6_reddst
            let mut sin6 = SockaddrIn6::zeroed();
            sin6.sin6_family = AF_INET6;
            sin6.sin6_len = size_of::<SockaddrIn6>() as u8;
            sin6.sin6_addr = reddst6;
            // SAFETY: a local `sockaddr_in6`.
            let Some(rt) =
                (unsafe { rtalloc(sin6tosa(&mut sin6), 0, m.m_pkthdr().ph_rtableid.get()) })
            else {
                break 'out true;
            };

            let gw = rt.rt_gateway.get();
            // SAFETY: a route's gateway is NULL or a readable sockaddr.
            if gw.is_null() || unsafe { (*gw).sa_family } != AF_INET6 {
                rtfree(Some(rt));
                break 'out true;
            }

            // SAFETY: an `AF_INET6` gateway is a `sockaddr_in6`.
            let gw6 = unsafe { read_at::<SockaddrIn6>(gw.cast::<u8>()) }.sin6_addr;
            if src6 != gw6 {
                rtfree(Some(rt));
                break 'out true;
            }
            rtfree(Some(rt));
        }

        let mut is_router = false;
        let mut is_onlink = false;
        if in6_is_addr_linklocal(&redtgt6) {
            is_router = true; // router case
        }
        if redtgt6 == reddst6 {
            is_onlink = true; // on-link destination case
        }
        if !is_router && !is_onlink {
            break 'out true;
        }

        // validation passed

        icmp6len -= size_of::<NdRedirect>() as i32;
        let mut ndopts = NdOpts::default();
        // SAFETY: the options follow the message inside the `icmp6len` contiguous bytes.
        let opts = unsafe {
            core::slice::from_raw_parts(nd_rd.add(size_of::<NdRedirect>()), icmp6len as usize)
        };
        if !nd6_options(opts, &mut ndopts) {
            // nd6_options have incremented stats
            break 'out false;
        }

        let mut lladdr: Option<&[u8]> = None;
        let mut lladdrlen = 0;
        if let Some(opt) = ndopts.nd_opts_tgt_lladdr {
            // SAFETY: the option was validated by nd6_options and is inside `opts`.
            let (a, len) = unsafe { nd6_opt_lladdr(opt) };
            lladdr = Some(a);
            lladdrlen = len;
        }

        if lladdr.is_some() && ((i32::from(ifp.if_addrlen.get()) + 2 + 7) & !7) != lladdrlen {
            break 'out true;
        }

        // RFC 2461 8.3
        nd6_cache_lladdr(
            ifp,
            &redtgt6,
            lladdr,
            i32::from(ND_REDIRECT),
            i32::from(if is_onlink {
                ND_REDIRECT_ONLINK
            } else {
                ND_REDIRECT_ROUTER
            }),
            i32::from(i_am_router),
        );

        if !is_onlink {
            // better router case. perform rtredirect.
            // do not install redirect route, if the number of entries is too much (> hiwat).
            // note that, the node (= host) will work just fine even if we do not install
            // redirect route (there will be additional hops, though).
            let rtcount = rt_timer_queue_count(&ICMP6_REDIRECT_TIMEOUT_Q);
            if rtcount >= IP6_MAXDYNROUTES.load(Ordering::Relaxed) as u64 {
                break 'out false;
            }

            let mut sdst = SockaddrIn6::zeroed();
            let mut sgw = SockaddrIn6::zeroed();
            let mut ssrc = SockaddrIn6::zeroed();
            for s in [&mut sdst, &mut sgw, &mut ssrc] {
                s.sin6_family = AF_INET6;
                s.sin6_len = size_of::<SockaddrIn6>() as u8;
            }
            sgw.sin6_addr = redtgt6;
            sdst.sin6_addr = reddst6;
            ssrc.sin6_addr = src6;
            let mut newrt = None;
            // SAFETY: three local `sockaddr_in6`s.
            unsafe {
                rtredirect(
                    sin6tosa(&mut sdst),
                    sin6tosa(&mut sgw),
                    sin6tosa(&mut ssrc),
                    Some(&mut newrt),
                    m.m_pkthdr().ph_rtableid.get(),
                )
            };
            if let Some(nrt) = newrt
                && ICMP6_REDIRTIMEOUT.load(Ordering::Relaxed) > 0
            {
                let _ = rt_timer_add(
                    nrt,
                    &ICMP6_REDIRECT_TIMEOUT_Q,
                    m.m_pkthdr().ph_rtableid.get(),
                );
            }
            rtfree(newrt);
        }
        // finally update cached route in each socket via pfctlinput
        let mut sdst = SockaddrIn6::zeroed();
        sdst.sin6_family = AF_INET6;
        sdst.sin6_len = size_of::<SockaddrIn6>() as u8;
        sdst.sin6_addr = reddst6;
        // SAFETY: a local `sockaddr_in6`.
        unsafe { pfctlinput(PRC_REDIRECT_HOST, sin6tosa(&mut sdst)) };
        false
    };

    // freeit / bad:
    if_put(ifp);
    if bad {
        icmp6stat_inc(Icmp6statCounters::Icp6sBadredirect);
    }
    m_freem(m);
}

/// `icmp6_redirect_output`: sends a redirect about forwarded packet `m0` (consumed) along
/// route `rt`.
pub fn icmp6_redirect_output(m0: &'static Mbuf, rt: &'static Rtentry) {
    let i_am_router = IP6_FORWARDING.load(Ordering::Relaxed) != 0;

    icmp6_errcount(ND_REDIRECT, 0);

    let mut ifp: Option<&'static Ifnet> = None;
    let mut m: Option<&'static Mbuf> = None;
    let mut m0 = Some(m0);

    'fail: {
        // if we are not router, we don't send icmp6 redirect
        if !i_am_router {
            break 'fail;
        }

        // sanity check
        let Some(m0r) = m0 else {
            break 'fail;
        };
        if !rtisvalid(Some(rt)) {
            break 'fail;
        }

        ifp = if_get(rt.rt_ifidx.get());
        let Some(ifp) = ifp else {
            break 'fail;
        };

        // Address check: the source address must identify a neighbor, and the destination
        // address must not be a multicast address [RFC 2461, sec 8.2]
        let sip6 = mtod_ip6(m0r);
        let mut src_sa = SockaddrIn6::zeroed();
        src_sa.sin6_family = AF_INET6;
        src_sa.sin6_len = size_of::<SockaddrIn6>() as u8;
        src_sa.sin6_addr = sip6.ip6_src;
        // we don't currently use sin6_scope_id, but eventually use it
        src_sa.sin6_scope_id = in6_addr2scopeid(ifp.if_index.get(), &sip6.ip6_src) as u32;
        if !nd6_is_addr_neighbor(&src_sa, ifp) {
            break 'fail;
        }
        if in6_is_addr_multicast(&sip6.ip6_dst) {
            break 'fail; // what should we do here?
        }

        // rate limit
        if icmp6_ratelimit(&sip6.ip6_src, ND_REDIRECT, 0) {
            break 'fail;
        }

        // Since we are going to append up to 1280 bytes (= IPV6_MMTU), we almost always ask
        // for an mbuf cluster for simplicity. (MHLEN < IPV6_MMTU is almost always true)
        const _: () = assert!((IPV6_MMTU as usize) < MCLBYTES);
        m = m_gethdr(M_DONTWAIT, MT_HEADER);
        if let Some(x) = m
            && IPV6_MMTU as usize >= MHLEN
        {
            mclget(x, M_DONTWAIT);
        }
        let Some(mm) = m else {
            break 'fail;
        };
        mm.m_pkthdr().ph_ifidx.set(0);
        mm.m_len().set(0);
        let maxlen = (IPV6_MMTU as usize).min(m_trailingspace(mm) as usize);
        // just for safety
        if maxlen
            < size_of::<Ip6Hdr>()
                + size_of::<Icmp6Hdr>()
                + ((size_of::<NdOptHdr>() + usize::from(ifp.if_addrlen.get()) + 7) & !7)
        {
            break 'fail;
        }

        // get ip6 linklocal address for ifp(my outgoing interface).
        let Some(ia6) = in6ifa_ifpforlinklocal(
            ifp,
            IN6_IFF_TENTATIVE | IN6_IFF_DUPLICATED | IN6_IFF_ANYCAST,
        ) else {
            break 'fail;
        };
        let ifp_ll6 = ia6_in6(ia6);

        // get ip6 linklocal address for the router.
        let mut gateway_ll6: Option<In6Addr> = None;
        let gw = rt.rt_gateway.get();
        if !gw.is_null() && rt.rt_flags.get() & RTF_GATEWAY != 0 {
            // SAFETY: a route's gateway is a readable sockaddr; an inet6 gateway route's is a
            // `sockaddr_in6`.
            let a = unsafe { read_at::<SockaddrIn6>(gw.cast::<u8>()) }.sin6_addr;
            if in6_is_addr_linklocal(&a) {
                gateway_ll6 = Some(a);
            }
        }

        // ip6
        let mut ip6 = Ip6Hdr::zeroed();
        ip6.set_ip6_vfc(IPV6_VERSION);
        // ip6->ip6_plen will be set later
        ip6.ip6_nxt = IPPROTO_ICMPV6 as u8;
        ip6.ip6_hlim = 255;
        // ip6->ip6_src must be linklocal addr for my outgoing if.
        ip6.ip6_src = ifp_ll6;
        ip6.ip6_dst = sip6.ip6_src;

        // ND Redirect
        let mut nd_rd = NdRedirect {
            nd_rd_hdr: Icmp6Hdr::zeroed(),
            nd_rd_target: In6Addr::default(),
            nd_rd_dst: In6Addr::default(),
        };
        nd_rd.set_nd_rd_type(ND_REDIRECT);
        nd_rd.set_nd_rd_code(0);
        nd_rd.set_nd_rd_reserved(0);
        let nexthop;
        if rt.rt_flags.get() & RTF_GATEWAY != 0 {
            // nd_rd->nd_rd_target must be a link-local address in better router cases.
            let Some(nh) = gateway_ll6 else {
                break 'fail;
            };
            nexthop = nh;
            nd_rd.nd_rd_target = nh;
            nd_rd.nd_rd_dst = sip6.ip6_dst;
        } else {
            // make sure redtgt == reddst
            nexthop = sip6.ip6_dst;
            nd_rd.nd_rd_target = sip6.ip6_dst;
            nd_rd.nd_rd_dst = sip6.ip6_dst;
        }

        // The message up to the redirected header option, built here and copied to the mbuf
        // in one go. The target link-layer option is at most 8 * ((2 + 255 + 7) / 8) = 264
        // bytes.
        let mut buf = [0u8; size_of::<Ip6Hdr>() + size_of::<NdRedirect>() + 264 + 8];
        let mut p = size_of::<Ip6Hdr>() + size_of::<NdRedirect>();

        // target lladdr option
        let addrlen = usize::from(ifp.if_addrlen.get());
        let len = (size_of::<NdOptHdr>() + addrlen + 7) & !7; // round by 8
        // safety check
        if len + p <= maxlen {
            let nrt = nd6_lookup(&nexthop, false, Some(ifp), ifp.if_rdomain.get());
            if let Some(nrt) = nrt
                && nrt.rt_flags.get() & (RTF_GATEWAY | RTF_LLINFO) == RTF_LLINFO
                && !nrt.rt_gateway.get().is_null()
            {
                let sdl = satosdl(nrt.rt_gateway.get());
                // SAFETY: a route's gateway is a readable sockaddr; a link-layer one is a
                // `sockaddr_dl`.
                let (family, alen) = unsafe { ((*sdl).sdl_family, (*sdl).sdl_alen) };
                if family == AF_LINK && alen != 0 {
                    let hdr = NdOptHdr {
                        nd_opt_type: ND_OPT_TARGET_LINKADDR,
                        nd_opt_len: (len >> 3) as u8,
                    };
                    put(&mut buf, p, &hdr);
                    // SAFETY: LLADDR(sdl) holds the link-layer address, `if_addrlen` bytes.
                    let src = unsafe { core::slice::from_raw_parts(lladdr(sdl), addrlen) };
                    let at = p + size_of::<NdOptHdr>();
                    buf[at..at + addrlen].copy_from_slice(src);
                    p += len;
                }
            }
            rtfree(nrt);
        }

        mm.m_pkthdr().len.set(p as i32);
        mm.m_len().set(p as u32);

        // just to be safe
        let mut attach_m0 = false;
        if p <= maxlen {
            // redirected header option
            //
            // compute the maximum size for icmp6 redirect header option.
            // XXX room for auth header?
            let mut len = (maxlen - p) & !7;

            // Redirected header option spec (RFC2461 4.6.3) talks nothing about
            // padding/truncate rule for the original IP packet. From the discussion on
            // IPv6imp in Feb 1999, the consensus was:
            // - "attach as much as possible" is the goal
            // - pad if not aligned (original size can be guessed by original ip6 header)
            // Following code adds the padding if it is simple enough, and truncates if not.
            let hdrlen = size_of::<NdOptRdHdr>();
            let pl = m0r.m_pkthdr().len.get();
            if ((len - hdrlen) as i64) < i64::from(pl) {
                // not enough room, truncate
                m_adj(m0r, (len - hdrlen) as i32 - pl);
            } else {
                // enough room, truncate if not aligned. we don't pad here for simplicity.
                let extra = pl % 8;
                if extra != 0 {
                    // truncate
                    m_adj(m0r, -extra);
                }
                len = m0r.m_pkthdr().len.get() as usize + hdrlen;
            }

            let nd_opt_rh = NdOptRdHdr {
                nd_opt_rh_type: ND_OPT_REDIRECTED_HEADER,
                nd_opt_rh_len: (len >> 3) as u8,
                ..NdOptRdHdr::default()
            };
            put(&mut buf, p, &nd_opt_rh);
            p += hdrlen;

            // connect m0 to m
            mm.m_pkthdr().len.set(p as i32 + m0r.m_pkthdr().len.get());
            attach_m0 = true;
        }
        mm.m_len().set(p as u32);

        // The scope zones of link-local addresses are meaningful only inside the node.
        if in6_is_addr_linklocal(&ip6.ip6_src) {
            ip6.ip6_src.set_s6_addr16(1, 0);
        }
        if in6_is_addr_linklocal(&ip6.ip6_dst) {
            ip6.ip6_dst.set_s6_addr16(1, 0);
        }
        if in6_is_addr_linklocal(&nd_rd.nd_rd_target) {
            nd_rd.nd_rd_target.set_s6_addr16(1, 0);
        }
        if in6_is_addr_linklocal(&nd_rd.nd_rd_dst) {
            nd_rd.nd_rd_dst.set_s6_addr16(1, 0);
        }

        ip6.ip6_plen = htons((mm.m_pkthdr().len.get() as usize - size_of::<Ip6Hdr>()) as u16);

        // nd_rd->nd_rd_cksum = 0: the message was built zeroed.
        put(&mut buf, 0, &ip6);
        put(&mut buf, size_of::<Ip6Hdr>(), &nd_rd);
        // SAFETY: `mm` is a fresh mbuf with a cluster, `maxlen` bytes at its data, and
        // `p <= maxlen` (the lladdr option is only added when it fits; the redirected header
        // option only when `p <= maxlen`).
        unsafe { ptr::copy_nonoverlapping(buf.as_ptr(), mtod::<u8>(mm), p) };
        if attach_m0 {
            m_cat(mm, m0.take());
        }
        // noredhdropt:
        m_freem(m0.take());
        mm.m_pkthdr().csum_flags.set(M_ICMP_CSUM_OUT);

        // send the packet to outside...
        let _ = ip6_output(mm, None, None, 0, None, None);

        icmp6stat_inc_hist(Icmp6statCounters::Icp6sOuthist, ND_REDIRECT);

        if_put(Some(ifp));
        return;
    }
    // fail:
    if_put(ifp);
    m_freem(m);
    m_freem(m0);
}

/// `icmp6_ctloutput`: ICMPv6 socket option processing (`ICMP6_FILTER`).
pub fn icmp6_ctloutput(
    op: i32,
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let inp = sotoinpcb(so);

    if level != IPPROTO_ICMPV6 {
        return Err(Errno::EINVAL);
    }

    match op {
        PRCO_SETOPT => match optname {
            ICMP6_FILTER => {
                let Some(m) = m else {
                    return Err(Errno::EMSGSIZE);
                };
                if m.m_len().get() as usize != size_of::<Icmp6Filter>() {
                    return Err(Errno::EMSGSIZE);
                }
                let Some(filt) = inp.and_then(|inp| inp.inp_icmp6filt.get()) else {
                    return Err(Errno::EINVAL);
                };
                // SAFETY: the mbuf holds a whole filter at its data; the pcb's filter is the
                // `Icmp6Filter` allocated at attach time, owned by the socket.
                unsafe {
                    ptr::copy_nonoverlapping(
                        mtod::<u8>(m).cast_const(),
                        filt.as_ptr().cast::<u8>(),
                        size_of::<Icmp6Filter>(),
                    )
                };
                Ok(())
            }
            _ => Err(Errno::ENOPROTOOPT),
        },
        PRCO_GETOPT => match optname {
            ICMP6_FILTER => {
                let Some(filt) = inp.and_then(|inp| inp.inp_icmp6filt.get()) else {
                    return Err(Errno::EINVAL);
                };
                let Some(m) = m else {
                    return Err(Errno::EINVAL);
                };
                m.m_len().set(size_of::<Icmp6Filter>() as u32);
                // SAFETY: the pcb's filter is a whole `Icmp6Filter`; the reply mbuf has room
                // for it (`sogetopt` provides an mbuf with MLEN bytes).
                unsafe {
                    ptr::copy_nonoverlapping(
                        filt.as_ptr().cast::<u8>().cast_const(),
                        mtod::<u8>(m),
                        size_of::<Icmp6Filter>(),
                    )
                };
                Ok(())
            }
            _ => Err(Errno::ENOPROTOOPT),
        },
        _ => Ok(()),
    }
}

/// `icmp6_ratelimit`: performs the rate limit check. `false` if it is okay to send the icmp6
/// packet, `true` if the router SHOULD NOT send this icmp6 packet due to rate limitation.
///
/// XXX per-destination/type check necessary? `dst`, `type_` and `code` are not used at this
/// moment.
pub fn icmp6_ratelimit(_dst: &In6Addr, _type: u8, _code: u8) -> bool {
    let limit = ICMP6ERRPPSLIM.load(Ordering::Relaxed);
    let pps = ICMP6ERRPPS.as_ptr();
    // PPS limit
    // SAFETY: the counters live forever and are touched only here, through
    // `ppsratecheck_shared`, which dereferences them only inside `ppsratecheck_mtx`.
    let ok = unsafe { ppsratecheck_shared(&raw mut (*pps).last, &raw mut (*pps).count, limit) };
    if !ok {
        return true; // The packet is subject to rate limit
    }
    false // okay to send
}

/// `icmp6_mtudisc_clone`: a host route to `dst` to hold a learned path MTU (cloned from the
/// network route if needed), with a timer on `icmp6_mtudisc_timeout_q`; referenced. `ipsec`:
/// for an IPsec SA's destination.
pub fn icmp6_mtudisc_clone(
    dst: &SockaddrIn6,
    rtableid: u32,
    ipsec: bool,
) -> Option<&'static Rtentry> {
    let mut dst = *dst;
    // SAFETY: a local `sockaddr_in6`.
    let mut rt = unsafe { rtalloc(sin6tosa(&mut dst), RT_RESOLVE, rtableid) };

    let ok = 'bad: {
        // Check if the route is actually usable
        let Some(r) = rt.filter(|r| rtisvalid(Some(r))) else {
            break 'bad false;
        };
        // IPsec needs the route only for PMTU, it can use reject for that
        if !ipsec && r.rt_flags.get() & (RTF_REJECT | RTF_BLACKHOLE) != 0 {
            break 'bad false;
        }

        // No PMTU for local routes and permanent neighbors, ARP and NDP use the same expire
        // timer as the route.
        if r.rt_flags.get() & RTF_LOCAL != 0
            || (r.rt_flags.get() & RTF_LLINFO != 0 && r.rt_expire().get() == 0)
        {
            break 'bad false;
        }

        // If we didn't get a host route, allocate one
        let mut r = r;
        if r.rt_flags.get() & RTF_HOST == 0 {
            let mut info = RtAddrinfo::new();
            let mut sa_rl = SockaddrRtlabel::default();

            info.rti_ifa = r.rt_ifa.get();
            info.rti_flags = RTF_GATEWAY | RTF_HOST | RTF_DYNAMIC;
            info.rti_info[RTAX_DST] = sin6tosa(&mut dst);
            info.rti_info[RTAX_GATEWAY] = r.rt_gateway.get();
            info.rti_info[RTAX_LABEL] = rtlabel_id2sa(r.rt_labelid.get(), &mut sa_rl);

            let mut nrt = None;
            // SAFETY: a local `sockaddr_in6`, the route's gateway and a local label.
            if unsafe {
                rtrequest(
                    RTM_ADD,
                    &mut info,
                    r.rt_priority.get(),
                    Some(&mut nrt),
                    rtableid,
                )
            }
            .is_err()
            {
                break 'bad false;
            }
            let Some(n) = nrt else {
                break 'bad false;
            };
            n.rt_rmx.assign(&r.rt_rmx);
            rtfree(Some(r));
            r = n;
            rt = Some(n);
            rtm_send(r, RTM_ADD, 0, rtableid);
        }
        if rt_timer_add(r, &ICMP6_MTUDISC_TIMEOUT_Q, rtableid).is_err() {
            break 'bad false;
        }

        true
    };
    if ok {
        return rt;
    }
    // bad:
    rtfree(rt);
    None
}

/// `icmp6_mtudisc_timeout`: a path MTU timer fired: delete the dynamic host route or forget
/// the learned MTU.
pub fn icmp6_mtudisc_timeout(rt: &'static Rtentry, rtableid: u32) {
    net_assert_locked("icmp6_mtudisc_timeout");

    let Some(ifp) = if_get(rt.rt_ifidx.get()) else {
        return;
    };

    if rt.rt_flags.get() & (RTF_DYNAMIC | RTF_HOST) == (RTF_DYNAMIC | RTF_HOST) {
        let _ = rtdeletemsg(rt, ifp, rtableid);
    } else if rt.rt_locks().get() & RTV_MTU == 0 {
        rt.rt_mtu().store(0, Ordering::Relaxed);
    }

    if_put(Some(ifp));
}

/// `icmp6_sysctl_icmp6stat`: `net.inet6.icmp6.icmp6stats`, the counters as a `struct
/// icmp6stat`.
fn icmp6_sysctl_icmp6stat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    const _: () = assert!(size_of::<Icmp6stat>() == ICP6S_NCOUNTERS * size_of::<u64>());
    let mut bytes = Vec::with_capacity(ICP6S_NCOUNTERS * size_of::<u64>());
    for c in &ICMP6COUNTERS {
        bytes.extend_from_slice(&c.load(Ordering::Relaxed).to_ne_bytes());
    }

    sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
}

/// `icmp6_sysctl`: the `net.inet6.icmp6` sysctls.
pub fn icmp6_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    // All sysctl names at this level are terminal.
    let [n] = name else {
        return Err(Errno::ENOTDIR);
    };

    match *n {
        ICMPV6CTL_REDIRTIMEOUT => {
            let oldval = ICMP6_REDIRTIMEOUT.load(Ordering::Relaxed);
            let newval = AtomicI32::new(oldval);
            let error = sysctl_int_bounded(oldp, oldlenp, newp, newlen, &newval, 0, INT_MAX);
            let newval = newval.into_inner();
            if error.is_ok() && oldval != newval {
                rw_enter_write(&SYSCTL_LOCK);
                ICMP6_REDIRTIMEOUT.store(newval, Ordering::Relaxed);
                rt_timer_queue_change(&ICMP6_REDIRECT_TIMEOUT_Q, newval);
                rw_exit_write(&SYSCTL_LOCK);
            }

            error
        }
        ICMPV6CTL_STATS => icmp6_sysctl_icmp6stat(oldp, oldlenp, newp),

        ICMPV6CTL_ND6_QUEUED => sysctl_rdint(
            oldp,
            oldlenp,
            newp,
            ln_hold_total.load(Ordering::Relaxed) as i32,
        ),

        _ => sysctl_bounded_arr(&ICMPV6CTL_VARS, name, oldp, oldlenp, newp, newlen),
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for ICMPv6 on synthetic packets: the echo reply (`icmp6_echo_reply`, with the
    // copy and the fresh-mbuf paths), `icmp6_error`'s rules and the message it builds,
    // `icmp6_reflect`, the checks of `icmp6_input`, the redirect validation, path MTU discovery
    // and the sysctls.

    use std::sync::MutexGuard;
    use std::vec::Vec;

    use super::*;
    use crate::kern::uipc_mbuf::{m_copydata, m_freem, m_get};
    use crate::net::if_::ifaof_ifpforaddr;
    use crate::net::if_::tests::test_packet;
    use crate::net::route::{
        RTAX_DST, RTAX_GATEWAY, RTAX_NETMASK, RTF_CLONING, RTF_CONNECTED, RTF_MPATH, RTF_STATIC,
        RtAddrinfo, rt_ifa_add,
    };
    use crate::netinet::icmp6::{ICMP6_PACKET_TOO_BIG, Icmp6stat};
    use crate::netinet::ip_input::tests::{bytes, setup as setup_ip};
    use crate::netinet::ip6::IPV6_VERSION;
    use crate::netinet6::in6::tests::{a6, test_ia6, test_if};
    use crate::netinet6::in6::{IN6ADDR_ANY, IN6ADDR_LINKLOCAL_ALLNODES};
    use crate::netinet6::nd6::tests::{OURS6, PEER6, icmp6stat};
    use crate::netinet6::nd6::{nd6_ifattach, nd6_init};
    use crate::sys::endian::htonl;
    use crate::sys::mbuf::MT_DATA;
    use crate::sys::systm::{net_lock, net_unlock};
    use std::ptr::NonNull;

    const IPPROTO_UDP: u8 = 17;

    /// The ICMPv6 and ND state, and an interface with `OURS6`/64, its connected route, and a
    /// link-local address; no duplicate address detection runs: the addresses are usable.
    ///
    /// The interface's MTU is 0: `ip6_output` drops (EMSGSIZE) what it is given, because
    /// `if_output_tso` has no AF_INET6 case yet (phase 2 of the INET6 port) and would panic.
    fn net6() -> (
        (MutexGuard<'static, ()>, MutexGuard<'static, ()>),
        &'static Ifnet,
    ) {
        let g = setup_ip();
        nd6_init();
        icmp6_init();
        reset_ratelimit();
        let ifp = test_if(b"tic0");
        nd6_ifattach(ifp);
        let ia = test_ia6(ifp, OURS6, 0);
        let mut ll = a6("fe80::5054:ff:fe12:3456");
        ll.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
        test_ia6(ifp, ll, 0);
        // `in6_update_ifa` installs the connected route of the prefix.
        net_lock();
        // SAFETY: the address's own socket address; the address is leaked.
        unsafe {
            rt_ifa_add(
                &ia.ia_ifa,
                RTF_CLONING | RTF_CONNECTED | RTF_MPATH,
                ia.ia_ifa.ifa_addr.get(),
                0,
            )
        }
        .expect("connected route");
        net_unlock();
        ifp.if_mtu.set(0);
        (g, ifp)
    }

    /// Forgets the rate limit's history and restores its default.
    fn reset_ratelimit() {
        ICMP6ERRPPSLIM.store(100, Ordering::Relaxed);
        set_ratelimit_window(0);
    }

    /// Starts the rate limit's window at second `sec`. The host clock of the tests stands at 0,
    /// which `ppsratecheck` reads as "never checked" and resets the count on every call; a window
    /// that started "in the future" is the way to count.
    fn set_ratelimit_window(sec: i64) {
        // SAFETY: the tests run one at a time under the network test lock.
        let pps = unsafe { ICMP6ERRPPS.get_mut() };
        pps.last = Timeval {
            tv_sec: sec,
            tv_usec: 0,
        };
        pps.count = 0;
    }

    /// The value of the histogram entry `hist + t`.
    fn hist(hist: Icmp6statCounters, t: u8) -> u64 {
        ICMP6COUNTERS[hist as usize + usize::from(t)].load(Ordering::Relaxed)
    }

    /// The bytes of an IPv6 header.
    fn ip6_bytes(src: In6Addr, dst: In6Addr, nxt: u8, hlim: u8, plen: usize) -> Vec<u8> {
        let mut ip6 = Ip6Hdr::zeroed();
        ip6.set_ip6_vfc(IPV6_VERSION);
        ip6.ip6_plen = htons(plen as u16);
        ip6.ip6_nxt = nxt;
        ip6.ip6_hlim = hlim;
        ip6.ip6_src = src;
        ip6.ip6_dst = dst;
        let mut b = std::vec![0u8; 40];
        put(&mut b, 0, &ip6);
        b
    }

    /// A received packet of `b`, from `ifp`.
    fn received(ifp: &Ifnet, b: &[u8]) -> &'static Mbuf {
        let m = test_packet(b);
        m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
        m
    }

    /// An ICMPv6 message from `src` to `dst`: `fixed` is type, code, checksum and the 4 bytes of
    /// the header's last word, `data` what follows. The checksum is filled in unless `bad_sum`.
    fn icmp6_packet(
        ifp: &Ifnet,
        src: In6Addr,
        dst: In6Addr,
        hlim: u8,
        fixed: [u8; 8],
        data: &[u8],
        bad_sum: bool,
    ) -> &'static Mbuf {
        let len = 8 + data.len();
        let mut b = ip6_bytes(src, dst, IPPROTO_ICMPV6 as u8, hlim, len);
        b.extend_from_slice(&fixed);
        b.extend_from_slice(data);
        let m = received(ifp, &b);
        if bad_sum {
            b[42..44].copy_from_slice(&0xdeadu16.to_ne_bytes());
        } else {
            let sum = in6_cksum(m, IPPROTO_ICMPV6 as u8, 40, len as u32);
            b[42..44].copy_from_slice(&sum.to_ne_bytes());
        }
        m_freem(m);
        received(ifp, &b)
    }

    /// The echo request the tests send: id 0x1234, sequence 7, 12 bytes of data.
    const ECHO_DATA: &[u8] = b"ping6-data!!";

    fn echo_request(ifp: &Ifnet, src: In6Addr, dst: In6Addr) -> &'static Mbuf {
        icmp6_packet(
            ifp,
            src,
            dst,
            64,
            [ICMP6_ECHO_REQUEST, 0, 0, 0, 0x12, 0x34, 0, 7],
            ECHO_DATA,
            false,
        )
    }

    /// The ICMPv6 header of the packet `b` at offset 40.
    fn header_at_40(b: &[u8]) -> Icmp6Hdr {
        // SAFETY: `b` holds at least 48 bytes; `Icmp6Hdr` is plain data.
        unsafe { ptr::read_unaligned(b[40..].as_ptr().cast::<Icmp6Hdr>()) }
    }

    /// The IPv6 header at the start of `b`.
    fn ip6_of(b: &[u8]) -> Ip6Hdr {
        // SAFETY: `b` holds at least 40 bytes; `Ip6Hdr` is plain data.
        unsafe { ptr::read_unaligned(b.as_ptr().cast::<Ip6Hdr>()) }
    }

    /// A packet of `parts` in a chain of mbufs, one part each.
    fn chain(parts: &[&[u8]]) -> &'static Mbuf {
        let total: usize = parts.iter().map(|p| p.len()).sum();
        let head = test_packet(parts[0]);
        let mut last = head;
        for p in &parts[1..] {
            let m = m_get(M_DONTWAIT, MT_DATA).expect("mbuf");
            // SAFETY: a fresh mbuf has MLEN bytes at m_data; the parts are small.
            unsafe { ptr::copy_nonoverlapping(p.as_ptr(), m.m_data().get(), p.len()) };
            m.m_len().set(p.len() as u32);
            last.m_next().set(Some(m));
            last = m;
        }
        head.m_pkthdr().len.set(total as i32);
        head
    }

    #[test]
    fn echo_request_is_answered_with_an_echo_reply() {
        let (_g, ifp) = net6();
        let m = echo_request(ifp, PEER6, OURS6);
        let req = bytes(m);
        let hdr = header_at_40(&req);

        let (reply, local) = icmp6_echo_reply(m, 40, hdr);
        assert!(local, "the request is kept for the raw sockets");
        let reply = reply.expect("a reply");
        let r = bytes(reply);

        // Addresses swapped, the reply comes from the address that was asked.
        let ip6 = ip6_of(&r);
        assert_eq!((ip6.ip6_src, ip6.ip6_dst), (OURS6, PEER6));
        assert_eq!(ip6.ip6_vfc(), IPV6_VERSION);
        assert_eq!(ip6.ip6_nxt, IPPROTO_ICMPV6 as u8);
        assert_eq!(i32::from(ip6.ip6_hlim), IP6_DEFHLIM.load(Ordering::Relaxed));
        assert_eq!(ip6.ip6_flow & !htonl(0xf000_0000), 0, "no flow label");
        // Echo reply, code 0, checksum left for ip6_output, id, sequence and data as they came.
        assert_eq!((r[40], r[41]), (ICMP6_ECHO_REPLY, 0));
        assert_eq!(r[42..44], [0, 0]);
        assert_eq!(r[44..], req[44..]);
        assert_ne!(
            reply.m_pkthdr().csum_flags.get() & M_ICMP_CSUM_OUT,
            0,
            "ip6_output has the checksum to make"
        );
        // The request itself is untouched.
        let after = bytes(m);
        assert_eq!(after, req);
        assert_eq!(after[40], ICMP6_ECHO_REQUEST);

        m_freem(reply);
        m_freem(m);
    }

    #[test]
    fn echo_reply_of_a_split_packet_gets_its_own_header_mbuf() {
        let (_g, ifp) = net6();
        let full = echo_request(ifp, PEER6, OURS6);
        let req = bytes(full);
        m_freem(full);

        // The IPv6 header alone in the first mbuf: the ICMPv6 header is not in it, so the reply
        // is built in a fresh mbuf in front of the data.
        let m = chain(&[&req[..40], &req[40..]]);
        m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
        let (reply, local) = icmp6_echo_reply(m, 40, header_at_40(&req));
        assert!(local);
        let reply = reply.expect("a reply");
        let r = bytes(reply);
        assert_eq!(r.len(), req.len());
        let ip6 = ip6_of(&r);
        assert_eq!((ip6.ip6_src, ip6.ip6_dst), (OURS6, PEER6));
        assert_eq!((r[40], r[41], r[42], r[43]), (ICMP6_ECHO_REPLY, 0, 0, 0));
        assert_eq!(r[44..], req[44..]);
        // The request still has its data, and its type.
        let kept = bytes(m);
        assert_eq!(kept, req);

        m_freem(reply);
        m_freem(m);
    }

    #[test]
    fn echo_request_to_a_stranger_is_not_answered_without_a_route() {
        let (_g, ifp) = net6();
        // The peer is on a prefix nobody routes: no reply, the packet is dropped.
        let m = echo_request(ifp, a6("2001:db8::5"), OURS6);
        let req = bytes(m);
        let (reply, local) = icmp6_echo_reply(m, 40, header_at_40(&req));
        assert!(reply.is_none());
        assert!(local);
        m_freem(m);
    }

    #[test]
    fn icmp6_input_counts_and_answers_an_echo_request() {
        let (_g, ifp) = net6();
        let inreq = hist(Icmp6statCounters::Icp6sInhist, ICMP6_ECHO_REQUEST);
        let outrep = hist(Icmp6statCounters::Icp6sOuthist, ICMP6_ECHO_REPLY);
        let reflect = icmp6stat(Icmp6statCounters::Icp6sReflect);

        let mut mp = Some(echo_request(ifp, PEER6, OURS6));
        let mut off = 40;
        let r = icmp6_input(&mut mp, &mut off, IPPROTO_ICMPV6, 10, None);
        let _ = r;
        assert_eq!(
            hist(Icmp6statCounters::Icp6sInhist, ICMP6_ECHO_REQUEST),
            inreq + 1
        );
        assert_eq!(
            hist(Icmp6statCounters::Icp6sOuthist, ICMP6_ECHO_REPLY),
            outrep + 1
        );
        assert_eq!(icmp6stat(Icmp6statCounters::Icp6sReflect), reflect + 1);
    }

    #[test]
    fn icmp6_input_drops_what_is_malformed() {
        let (_g, ifp) = net6();
        let case = |m: &'static Mbuf| {
            let mut mp = Some(m);
            let mut off = 40;
            let r = icmp6_input(&mut mp, &mut off, IPPROTO_ICMPV6, 10, None);
            (r, mp.is_none())
        };

        // Shorter than an ICMPv6 header.
        let before = icmp6stat(Icmp6statCounters::Icp6sTooshort);
        let mut b = ip6_bytes(PEER6, OURS6, IPPROTO_ICMPV6 as u8, 64, 4);
        b.extend_from_slice(&[ICMP6_ECHO_REQUEST, 0, 0, 0]);
        assert_eq!(case(received(ifp, &b)), (IPPROTO_DONE, true));
        assert_eq!(icmp6stat(Icmp6statCounters::Icp6sTooshort), before + 1);

        // A wrong checksum.
        let before = icmp6stat(Icmp6statCounters::Icp6sChecksum);
        let m = icmp6_packet(
            ifp,
            PEER6,
            OURS6,
            64,
            [ICMP6_ECHO_REQUEST, 0, 0, 0, 0, 1, 0, 1],
            &[],
            true,
        );
        assert_eq!(case(m), (IPPROTO_DONE, true));
        assert_eq!(icmp6stat(Icmp6statCounters::Icp6sChecksum), before + 1);

        // A code the type does not have, and messages shorter than their type needs.
        let msgs: [(u8, u8, usize, Icmp6statCounters); 6] = [
            (ICMP6_ECHO_REQUEST, 1, 0, Icmp6statCounters::Icp6sBadcode),
            (ICMP6_ECHO_REPLY, 3, 0, Icmp6statCounters::Icp6sBadcode),
            (MLD_LISTENER_QUERY, 0, 0, Icmp6statCounters::Icp6sBadlen),
            (ND_NEIGHBOR_SOLICIT, 0, 4, Icmp6statCounters::Icp6sBadlen),
            (ND_ROUTER_ADVERT, 0, 4, Icmp6statCounters::Icp6sBadlen),
            (ND_REDIRECT, 0, 8, Icmp6statCounters::Icp6sBadlen),
        ];
        for (ty, code, extra, counter) in msgs {
            let before = icmp6stat(counter);
            let m = icmp6_packet(
                ifp,
                PEER6,
                OURS6,
                255,
                [ty, code, 0, 0, 0, 0, 0, 0],
                &std::vec![0u8; extra],
                false,
            );
            let mut mp = Some(m);
            let mut off = 40;
            let _ = icmp6_input(&mut mp, &mut off, IPPROTO_ICMPV6, 10, None);
            assert_eq!(icmp6stat(counter), before + 1, "type {ty} code {code}");
        }
    }

    /// A UDP packet from `src` to `dst`, 20 bytes of payload, received on `ifp`.
    fn udp_packet(ifp: &Ifnet, src: In6Addr, dst: In6Addr) -> (&'static Mbuf, Vec<u8>) {
        let mut b = ip6_bytes(src, dst, IPPROTO_UDP, 64, 20);
        b.extend_from_slice(&[0x5a; 20]);
        (received(ifp, &b), b)
    }

    #[test]
    fn error_message_quotes_the_packet_and_counts_its_kind() {
        let (_g, ifp) = net6();
        let (m, orig) = udp_packet(ifp, PEER6, OURS6);
        let errors = icmp6stat(Icmp6statCounters::Icp6sError);
        let noport = icmp6stat(Icmp6statCounters::Icp6sOdstUnreachNoport);
        let out = hist(Icmp6statCounters::Icp6sOuthist, ICMP6_DST_UNREACH);

        let e =
            icmp6_do_error(m, ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_NOPORT, 0).expect("an error");
        let b = bytes(e);
        assert_eq!(b.len(), 40 + 8 + orig.len());
        let ip6 = ip6_of(&b);
        assert_eq!(
            (ip6.ip6_src, ip6.ip6_dst),
            (PEER6, OURS6),
            "reflected later"
        );
        assert_eq!(
            (b[40], b[41], b[44..48].to_vec()),
            (ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_NOPORT, std::vec![0; 4])
        );
        assert_eq!(b[48..], orig[..], "the offending packet follows");
        assert_eq!(e.m_pkthdr().ph_ifidx.get(), 0);
        assert_eq!(icmp6stat(Icmp6statCounters::Icp6sError), errors + 1);
        assert_eq!(
            icmp6stat(Icmp6statCounters::Icp6sOdstUnreachNoport),
            noport + 1
        );
        assert_eq!(
            hist(Icmp6statCounters::Icp6sOuthist, ICMP6_DST_UNREACH),
            out + 1
        );

        // The reflected error goes back to the sender, from us.
        let mut mp = Some(e);
        icmp6_reflect(&mut mp, 40, None).expect("reflect");
        let b = bytes(mp.expect("packet"));
        let ip6 = ip6_of(&b);
        assert_eq!((ip6.ip6_src, ip6.ip6_dst), (OURS6, PEER6));
        assert_eq!(b[48..], orig[..]);
    }

    #[test]
    fn error_message_carries_the_pointer_or_mtu() {
        let (_g, ifp) = net6();
        let (m, _) = udp_packet(ifp, PEER6, OURS6);
        let e = icmp6_do_error(m, ICMP6_PACKET_TOO_BIG, 0, 1400).expect("an error");
        let b = bytes(e);
        assert_eq!(b[44..48], 1400u32.to_be_bytes());
        assert_eq!(header_at_40(&b).icmp6_mtu(), htonl(1400));
        m_freem(e);

        let (m, _) = udp_packet(ifp, PEER6, OURS6);
        let e = icmp6_do_error(m, ICMP6_PARAM_PROB, ICMP6_PARAMPROB_HEADER, 6).expect("an error");
        assert_eq!(bytes(e)[44..48], 6u32.to_be_bytes());
        m_freem(e);
    }

    #[test]
    fn no_error_about_an_error() {
        let (_g, ifp) = net6();
        let canterror = icmp6stat(Icmp6statCounters::Icp6sCanterror);
        // ICMPv6 errors (any type below the echo request) and redirects: no error about them.
        for ty in [
            ICMP6_DST_UNREACH,
            ICMP6_PACKET_TOO_BIG,
            ICMP6_TIME_EXCEEDED,
            ND_REDIRECT,
        ] {
            let m = icmp6_packet(
                ifp,
                PEER6,
                OURS6,
                64,
                [ty, 0, 0, 0, 0, 0, 0, 0],
                &[0; 8],
                false,
            );
            assert!(
                icmp6_do_error(m, ICMP6_DST_UNREACH, 0, 0).is_none(),
                "type {ty}"
            );
        }
        assert_eq!(icmp6stat(Icmp6statCounters::Icp6sCanterror), canterror + 4);

        // An echo request is informational: it gets its error.
        let m = echo_request(ifp, PEER6, OURS6);
        let e = icmp6_do_error(m, ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_ADDR, 0).expect("an error");
        m_freem(e);
    }

    #[test]
    fn errors_about_multicast_and_unspecified_sources_are_suppressed() {
        let (_g, ifp) = net6();
        let mcast = a6("ff02::1:ff00:5");
        // To a multicast destination: no error, except Packet Too Big and a Parameter Problem
        // about an option.
        let (m, _) = udp_packet(ifp, PEER6, mcast);
        assert!(icmp6_do_error(m, ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_NOPORT, 0).is_none());
        let (m, _) = udp_packet(ifp, PEER6, mcast);
        assert!(icmp6_do_error(m, ICMP6_TIME_EXCEEDED, 0, 0).is_none());
        let (m, _) = udp_packet(ifp, PEER6, mcast);
        let e = icmp6_do_error(m, ICMP6_PACKET_TOO_BIG, 0, 1280).expect("too big is sent");
        m_freem(e);
        let (m, _) = udp_packet(ifp, PEER6, mcast);
        let e = icmp6_do_error(m, ICMP6_PARAM_PROB, ICMP6_PARAMPROB_OPTION, 40).expect("option");
        m_freem(e);
        let (m, _) = udp_packet(ifp, PEER6, mcast);
        assert!(icmp6_do_error(m, ICMP6_PARAM_PROB, ICMP6_PARAMPROB_HEADER, 40).is_none());

        // Link-layer multicast or broadcast.
        let (m, _) = udp_packet(ifp, PEER6, OURS6);
        m.m_flags().set(m.m_flags().get() | M_MCAST);
        assert!(icmp6_do_error(m, ICMP6_DST_UNREACH, 0, 0).is_none());

        // The sender is the unspecified address or a multicast group: nobody to tell.
        let (m, _) = udp_packet(ifp, IN6ADDR_ANY, OURS6);
        assert!(icmp6_do_error(m, ICMP6_DST_UNREACH, 0, 0).is_none());
        let (m, _) = udp_packet(ifp, mcast, OURS6);
        assert!(icmp6_do_error(m, ICMP6_DST_UNREACH, 0, 0).is_none());
    }

    #[test]
    fn errors_are_rate_limited() {
        let (_g, ifp) = net6();
        ICMP6ERRPPSLIM.store(2, Ordering::Relaxed);
        set_ratelimit_window(1);
        let toofreq = icmp6stat(Icmp6statCounters::Icp6sToofreq);
        let mut sent = 0;
        for _ in 0..5 {
            let (m, _) = udp_packet(ifp, PEER6, OURS6);
            if let Some(e) = icmp6_do_error(m, ICMP6_DST_UNREACH, 0, 0) {
                sent += 1;
                m_freem(e);
            }
        }
        assert_eq!(sent, 2, "two per second");
        assert_eq!(icmp6stat(Icmp6statCounters::Icp6sToofreq), toofreq + 3);

        // A negative limit: none.
        reset_ratelimit();
        ICMP6ERRPPSLIM.store(-1, Ordering::Relaxed);
        for _ in 0..5 {
            let (m, _) = udp_packet(ifp, PEER6, OURS6);
            m_freem(icmp6_do_error(m, ICMP6_DST_UNREACH, 0, 0).expect("unlimited"));
        }
        reset_ratelimit();
    }

    #[test]
    fn a_long_packet_is_truncated_to_the_minimum_mtu() {
        let (_g, ifp) = net6();
        let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
        mclget(m, M_DONTWAIT);
        let mut b = ip6_bytes(PEER6, OURS6, IPPROTO_UDP, 64, 1460);
        b.extend_from_slice(&[0x33; 1460]);
        // SAFETY: the cluster holds 2048 bytes.
        unsafe { ptr::copy_nonoverlapping(b.as_ptr(), m.m_data().get(), b.len()) };
        m.m_len().set(b.len() as u32);
        m.m_pkthdr().len.set(b.len() as i32);
        m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());

        let e =
            icmp6_do_error(m, ICMP6_TIME_EXCEEDED, ICMP6_TIME_EXCEED_TRANSIT, 0).expect("error");
        assert_eq!(e.m_pkthdr().len.get() as usize, IPV6_MMTU as usize);
        assert_eq!(ICMPV6_PLD_MAXLEN + 48, IPV6_MMTU as usize);
        m_freem(e);
    }

    #[test]
    fn error_message_does_not_quote_the_scope_zone() {
        let (_g, ifp) = net6();
        let mut src = a6("fe80::5054:ff:fe00:1");
        src.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
        let (m, _) = udp_packet(ifp, src, OURS6);
        let e = icmp6_do_error(m, ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_NOPORT, 0).expect("error");
        let b = bytes(e);
        // The new header keeps the zone (it routes the reply); the quoted one has it cleared.
        assert_eq!(ip6_of(&b).ip6_dst, OURS6);
        assert_eq!(
            ip6_of(&b).ip6_src.s6_addr16(1),
            htons(ifp.if_index.get() as u16)
        );
        assert_eq!(ip6_of(&b[48..]).ip6_src.s6_addr16(1), 0);
        m_freem(e);
    }

    #[test]
    fn reflect_refuses_what_cannot_be_reflected() {
        let (_g, ifp) = net6();

        // Too short to hold an IPv6 header before the ICMPv6 one.
        let m = echo_request(ifp, PEER6, OURS6);
        let mut mp = Some(m);
        assert_eq!(icmp6_reflect(&mut mp, 20, None), Err(Errno::EHOSTUNREACH));
        assert!(mp.is_none());

        // A packet that has been looping.
        let m = echo_request(ifp, PEER6, OURS6);
        m.m_pkthdr().ph_loopcnt.set(M_MAXLOOP);
        let mut mp = Some(m);
        assert_eq!(icmp6_reflect(&mut mp, 40, None), Err(Errno::ELOOP));
        assert!(mp.is_none());

        // No route back to the sender.
        let m = echo_request(ifp, a6("2001:db8::5"), OURS6);
        let mut mp = Some(m);
        assert_eq!(icmp6_reflect(&mut mp, 40, None), Err(Errno::EHOSTUNREACH));
        assert!(mp.is_none());
    }

    #[test]
    fn reflect_strips_extension_headers_and_picks_the_source() {
        let (_g, ifp) = net6();
        // An echo request that arrived with a destination options header (8 bytes of padding).
        let req = echo_request(ifp, PEER6, OURS6);
        let b = bytes(req);
        m_freem(req);
        let mut with_ext = b[..40].to_vec();
        with_ext[6] = 60; // next header: destination options
        with_ext.extend_from_slice(&[IPPROTO_ICMPV6 as u8, 0, 1, 4, 0, 0, 0, 0]);
        with_ext.extend_from_slice(&b[40..]);
        let m = received(ifp, &with_ext);

        let mut mp = Some(m);
        icmp6_reflect(&mut mp, 48, None).expect("reflect");
        let r = bytes(mp.expect("packet"));
        assert_eq!(r.len(), b.len(), "the extension header is gone");
        let ip6 = ip6_of(&r);
        assert_eq!(ip6.ip6_nxt, IPPROTO_ICMPV6 as u8);
        assert_eq!((ip6.ip6_src, ip6.ip6_dst), (OURS6, PEER6));
        assert_eq!(r[40..42], b[40..42]);
        assert_eq!(r[42..44], [0, 0], "the checksum is made by ip6_output");
        assert_eq!(r[44..], b[44..]);

        // Sent to a multicast group (a ping of ff02::1): the source is an address of ours, and the
        // answer goes to the sender.
        let m = echo_request(ifp, PEER6, IN6ADDR_LINKLOCAL_ALLNODES);
        let mut mp = Some(m);
        icmp6_reflect(&mut mp, 40, None).expect("reflect");
        let r = bytes(mp.expect("packet"));
        let ip6 = ip6_of(&r);
        assert_eq!(ip6.ip6_dst, PEER6);
        assert!(!in6_is_addr_unspecified(&ip6.ip6_src) && !in6_is_addr_multicast(&ip6.ip6_src));
        assert!(
            in6ifa_ifpwithaddr_any(ifp, &ip6.ip6_src),
            "the source is one of the interface's addresses"
        );

        // The source can be given: the route to it picks the interface and the address.
        let m = echo_request(ifp, PEER6, OURS6);
        let sa = SockaddrIn6::with_addr(PEER6);
        let mut mp = Some(m);
        icmp6_reflect(&mut mp, 40, Some(&sa)).expect("reflect with sa");
        m_freem(mp.expect("packet"));
    }

    /// Whether `ifp` has the IPv6 address `a`, whatever its flags.
    fn in6ifa_ifpwithaddr_any(ifp: &Ifnet, a: &In6Addr) -> bool {
        crate::netinet6::in6::in6ifa_ifpwithaddr(ifp, a).is_some()
    }

    #[test]
    fn redirects_are_validated() {
        let (_g, ifp) = net6();
        let bad = || icmp6stat(Icmp6statCounters::Icp6sBadredirect);
        let ll = |last: u16| {
            let mut a = a6("fe80::");
            a.set_s6_addr16(7, htons(last));
            a
        };
        let dst = a6("2001:db8::99");
        let redirect = |src: In6Addr, hlim: u8, target: In6Addr, redirected: In6Addr| {
            let mut data = target.s6_addr.to_vec();
            data.extend_from_slice(&redirected.s6_addr);
            // The redirect: the 8 bytes of the header, target and destination, no options.
            let m = icmp6_packet(
                ifp,
                src,
                OURS6,
                hlim,
                [ND_REDIRECT, 0, 0, 0, 0, 0, 0, 0],
                &data,
                false,
            );
            m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
            m
        };

        // The interface does not take redirects (no autoconf): ignored without a count.
        let before = bad();
        icmp6_redirect_input(redirect(ll(1), 255, ll(1), dst), 40);
        assert_eq!(bad(), before);

        // Taking them: each rule is checked.
        ifp.if_xflags.set(ifp.if_xflags.get() | IFXF_AUTOCONF6);
        let cases: [(&str, In6Addr, u8, In6Addr, In6Addr); 5] = [
            ("source not link-local", PEER6, 255, ll(1), dst),
            ("hop limit not 255", ll(1), 64, ll(1), dst),
            ("multicast destination", ll(1), 255, ll(1), a6("ff02::1")),
            (
                "no route to the destination's gateway",
                ll(1),
                255,
                ll(1),
                dst,
            ),
            (
                "target neither a router nor the destination",
                ll(1),
                255,
                PEER6,
                dst,
            ),
        ];
        for (what, src, hlim, target, redirected) in cases {
            let before = bad();
            icmp6_redirect_input(redirect(src, hlim, target, redirected), 40);
            assert_eq!(bad(), before + 1, "{what}");
        }

        // A router takes no redirects.
        IP6_FORWARDING.store(1, Ordering::Relaxed);
        let before = bad();
        icmp6_redirect_input(redirect(PEER6, 255, ll(1), dst), 40);
        IP6_FORWARDING.store(0, Ordering::Relaxed);
        assert_eq!(bad(), before);
        ifp.if_xflags.set(ifp.if_xflags.get() & !IFXF_AUTOCONF6);
    }

    #[test]
    fn a_host_sends_no_redirects() {
        let (_g, ifp) = net6();
        let errors = hist(Icmp6statCounters::Icp6sOuthist, ND_REDIRECT);
        let count = icmp6stat(Icmp6statCounters::Icp6sOredirect);
        let (m, _) = udp_packet(ifp, PEER6, a6("fd00:77::99"));
        // SAFETY: a local `sockaddr_in6`.
        let rt = unsafe {
            let mut dst = SockaddrIn6::with_addr(a6("fd00:77::99"));
            rtalloc(sin6tosa(&mut dst), RT_RESOLVE, 0)
        };
        let rt = rt.expect("the connected route");
        icmp6_redirect_output(m, rt);
        // Counted as an attempt, not sent.
        assert_eq!(icmp6stat(Icmp6statCounters::Icp6sOredirect), count + 1);
        assert_eq!(hist(Icmp6statCounters::Icp6sOuthist, ND_REDIRECT), errors);
        rtfree(Some(rt));
    }

    /// `route add -inet6 default <gw>`: the route and the next hop it caches.
    fn add_default_route(ifp: &'static Ifnet, gw: In6Addr) {
        let mut dst = SockaddrIn6::with_addr(IN6ADDR_ANY);
        let mut mask = SockaddrIn6::with_addr(IN6ADDR_ANY);
        let mut gate = SockaddrIn6::with_addr(gw);
        for s in [&mut dst, &mut mask, &mut gate] {
            s.sin6_len = size_of::<SockaddrIn6>() as u8;
        }
        let mut info = RtAddrinfo::new();
        info.rti_info[RTAX_DST] = sin6tosa(&mut dst);
        info.rti_info[RTAX_NETMASK] = sin6tosa(&mut mask);
        info.rti_info[RTAX_GATEWAY] = sin6tosa(&mut gate);
        info.rti_flags = RTF_GATEWAY | RTF_STATIC;
        // SAFETY: a local `sockaddr_in6`.
        info.rti_ifa = unsafe { ifaof_ifpforaddr(sin6tosa(&mut gate), ifp) };
        net_lock();
        let mut rt = None;
        // SAFETY: the addresses are locals.
        unsafe { rtrequest(RTM_ADD, &mut info, 0, Some(&mut rt), 0) }.expect("default route");
        rtfree(rt);
        net_unlock();
    }

    /// The ICMPv6 control parameters about packet `m`: Packet Too Big with `mtu`, quoting a UDP
    /// packet to `dst`.
    struct TooBig {
        hdr: Icmp6Hdr,
        dst: In6Addr,
    }

    impl TooBig {
        fn new(mtu: u32, dst: In6Addr) -> Self {
            let mut hdr = Icmp6Hdr::zeroed();
            hdr.icmp6_type = ICMP6_PACKET_TOO_BIG;
            hdr.set_icmp6_mtu(htonl(mtu));
            Self { hdr, dst }
        }

        fn param(&mut self, m: &'static Mbuf) -> Ip6ctlparam {
            let mut p = Ip6ctlparam::new();
            p.ip6c_m = Some(m);
            p.ip6c_icmp6 = ptr::from_mut(&mut self.hdr);
            p.ip6c_finaldst = ptr::from_mut(&mut self.dst);
            p
        }
    }

    static MTU_NOTIFIED: std::sync::Mutex<Vec<(In6Addr, u32)>> = std::sync::Mutex::new(Vec::new());

    fn record_mtu_change(dst: &SockaddrIn6, rtableid: u32) {
        MTU_NOTIFIED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((dst.sin6_addr, rtableid));
    }

    #[test]
    fn path_mtu_discovery_clones_a_host_route_and_lowers_its_mtu() {
        let (_g, ifp) = net6();
        icmp6_mtudisc_callback_register(record_mtu_change);
        icmp6_mtudisc_callback_register(record_mtu_change); // once
        MTU_NOTIFIED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        // A destination beyond the router: the route to it is cloned from the default route.
        add_default_route(ifp, PEER6);
        // A path MTU below the link's: nothing is sent here, the MTU only bounds the learned one.
        ifp.if_mtu.set(1500);
        let dst = a6("2001:db8::99");
        let (m, _) = udp_packet(ifp, PEER6, dst);
        let pmtuchg = icmp6stat(Icmp6statCounters::Icp6sPmtuchg);
        net_lock();

        // Below the minimum MTU: ignored.
        let mut tb = TooBig::new(1200, dst);
        icmp6_mtudisc_update(&tb.param(m), true);
        assert_eq!(icmp6stat(Icmp6statCounters::Icp6sPmtuchg), pmtuchg);
        assert!(
            MTU_NOTIFIED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
        );

        // A real one: the route holds the MTU; the callbacks hear of it.
        let mut tb = TooBig::new(1400, dst);
        icmp6_mtudisc_update(&tb.param(m), true);
        assert_eq!(icmp6stat(Icmp6statCounters::Icp6sPmtuchg), pmtuchg + 1);
        let sin6 = SockaddrIn6::with_addr(dst);
        let rt = icmp6_mtudisc_clone(&sin6, 0, false).expect("the host route");
        assert_ne!(rt.rt_flags.get() & RTF_HOST, 0);
        assert_eq!(rt.rt_mtu().load(Ordering::Relaxed), 1400);
        assert_eq!(rt_timer_queue_count(&ICMP6_MTUDISC_TIMEOUT_Q), 1);
        assert_eq!(
            *MTU_NOTIFIED.lock().unwrap_or_else(|e| e.into_inner()),
            std::vec![(dst, 0)]
        );

        // A larger one does not raise it, a smaller one lowers it.
        let mut tb = TooBig::new(1450, dst);
        icmp6_mtudisc_update(&tb.param(m), true);
        assert_eq!(rt.rt_mtu().load(Ordering::Relaxed), 1400);
        let mut tb = TooBig::new(1300, dst);
        icmp6_mtudisc_update(&tb.param(m), true);
        assert_eq!(rt.rt_mtu().load(Ordering::Relaxed), 1300);

        // The timer fires: the dynamic host route goes away.
        icmp6_mtudisc_timeout(rt, 0);
        rtfree(Some(rt));
        let again = icmp6_mtudisc_clone(&sin6, 0, false).expect("cloned afresh");
        assert_eq!(again.rt_mtu().load(Ordering::Relaxed), 0);
        rtfree(Some(again));
        net_unlock();
        m_freem(m);
    }

    #[test]
    fn unvalidated_mtu_changes_need_room_in_the_table() {
        let (_g, ifp) = net6();
        let dst = a6("fd00:77::98");
        let (m, _) = udp_packet(ifp, PEER6, dst);
        net_lock();
        // No room (lowat 0 with one route cloned already): an unvalidated report is ignored.
        ICMP6_MTUDISC_LOWAT.store(0, Ordering::Relaxed);
        let mut tb = TooBig::new(1400, dst);
        let before = rt_timer_queue_count(&ICMP6_MTUDISC_TIMEOUT_Q);
        icmp6_mtudisc_update(&tb.param(m), false);
        let after = rt_timer_queue_count(&ICMP6_MTUDISC_TIMEOUT_Q);
        ICMP6_MTUDISC_LOWAT.store(256, Ordering::Relaxed);
        assert!(before == 0 || after == before);
        net_unlock();
        m_freem(m);
    }

    #[test]
    fn the_icmp6_filter_is_a_socket_option() {
        use crate::kern::uipc_socket::{soclose, socreate};
        use crate::sys::socket::SOCK_RAW;
        let (_g, _t, _p) = crate::netinet::in_pcb::tests::setup();
        crate::netinet::raw_ip::rip_init();
        // A raw socket of the inet domain stands in for a raw ICMPv6 socket (no `AF_INET6`
        // socket is possible before `rip6_attach`): the option code only needs the pcb.
        let so = socreate(i32::from(crate::sys::socket::AF_INET), SOCK_RAW, 1).expect("socket");
        let inp = sotoinpcb(so).expect("pcb");
        let filt: &'static mut Icmp6Filter =
            std::boxed::Box::leak(std::boxed::Box::new(Icmp6Filter { icmp6_filt: [0; 8] }));
        inp.inp_icmp6filt.set(NonNull::new(filt));

        // Wrong level, wrong option.
        assert_eq!(
            icmp6_ctloutput(PRCO_SETOPT, so, 0, ICMP6_FILTER, None),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            icmp6_ctloutput(PRCO_SETOPT, so, IPPROTO_ICMPV6, 99, None),
            Err(Errno::ENOPROTOOPT)
        );
        // The wrong size of filter.
        let m = m_get(M_DONTWAIT, MT_DATA).expect("mbuf");
        m.m_len().set(8);
        assert_eq!(
            icmp6_ctloutput(PRCO_SETOPT, so, IPPROTO_ICMPV6, ICMP6_FILTER, Some(m)),
            Err(Errno::EMSGSIZE)
        );

        // Set a filter that blocks everything but echo replies, and read it back.
        let mut want = Icmp6Filter { icmp6_filt: [0; 8] };
        want.setblockall();
        want.setpass(ICMP6_ECHO_REPLY);
        m.m_len().set(size_of::<Icmp6Filter>() as u32);
        // SAFETY: an mbuf holds MLEN bytes, more than a filter.
        unsafe {
            ptr::copy_nonoverlapping(
                ptr::from_ref(&want).cast::<u8>(),
                mtod::<u8>(m),
                size_of::<Icmp6Filter>(),
            )
        };
        icmp6_ctloutput(PRCO_SETOPT, so, IPPROTO_ICMPV6, ICMP6_FILTER, Some(m)).expect("set");
        assert!(filt.willpass(ICMP6_ECHO_REPLY) && filt.willblock(ICMP6_ECHO_REQUEST));

        let out = m_get(M_DONTWAIT, MT_DATA).expect("mbuf");
        icmp6_ctloutput(PRCO_GETOPT, so, IPPROTO_ICMPV6, ICMP6_FILTER, Some(out)).expect("get");
        assert_eq!(out.m_len().get() as usize, size_of::<Icmp6Filter>());
        assert_eq!(bytes_of(out), bytes_of_filter(&want));

        // No filter, no option.
        inp.inp_icmp6filt.set(None);
        assert_eq!(
            icmp6_ctloutput(PRCO_GETOPT, so, IPPROTO_ICMPV6, ICMP6_FILTER, Some(out)),
            Err(Errno::EINVAL)
        );
        m_freem(m);
        m_freem(out);
        soclose(so, 0).expect("close");
        crate::netinet::in_pcb::tests::teardown();
    }

    fn bytes_of(m: &Mbuf) -> Vec<u8> {
        let mut v = std::vec![0u8; m.m_len().get() as usize];
        m_copydata(m, 0, &mut v);
        v
    }

    fn bytes_of_filter(f: &Icmp6Filter) -> Vec<u8> {
        f.icmp6_filt.iter().flat_map(|w| w.to_ne_bytes()).collect()
    }

    /// A sysctl read of the integer `mib`.
    fn sysctl_get(mib: i32) -> i32 {
        let mut v = 0i32;
        let mut len = size_of::<i32>();
        // SAFETY: a destination of the right size; `copyout` on the host copies there.
        let r = unsafe { sysctl_read_int(mib, ptr::from_mut(&mut v) as usize, &mut len) };
        r.expect("sysctl");
        v
    }

    /// Reads the integer sysctl `mib` into the user address `oldp`.
    ///
    /// # Safety
    ///
    /// `oldp` is the address of a writable `i32`.
    unsafe fn sysctl_read_int(mib: i32, oldp: usize, len: &mut usize) -> Result<(), Errno> {
        icmp6_sysctl(&[mib], oldp, len, 0, 0)
    }

    #[test]
    fn the_sysctls_read_and_write_the_variables() {
        let (_g, _ifp) = net6();
        assert_eq!(
            sysctl_get(ICMPV6CTL_ERRPPSLIMIT),
            ICMP6ERRPPSLIM.load(Ordering::Relaxed)
        );
        assert_eq!(
            sysctl_get(ICMPV6CTL_REDIRTIMEOUT),
            ICMP6_REDIRTIMEOUT.load(Ordering::Relaxed)
        );
        assert_eq!(sysctl_get(ICMPV6CTL_ND6_QUEUED), 0);
        assert_eq!(
            sysctl_get(ICMPV6CTL_MTUDISC_HIWAT),
            ICMP6_MTUDISC_HIWAT.load(Ordering::Relaxed)
        );

        // The error rate has bounds (-1..=1000).
        let mut len = size_of::<i32>();
        let mut new = 5000i32;
        assert_eq!(
            icmp6_sysctl(
                &[ICMPV6CTL_ERRPPSLIMIT],
                0,
                &mut len,
                ptr::from_mut(&mut new) as usize,
                size_of::<i32>()
            ),
            Err(Errno::EINVAL)
        );
        // Names with more than one level, and unknown ones.
        assert_eq!(
            icmp6_sysctl(&[ICMPV6CTL_STATS, 1], 0, &mut len, 0, 0),
            Err(Errno::ENOTDIR)
        );
        assert!(icmp6_sysctl(&[999], 0, &mut len, 0, 0).is_err());

        // The statistics are a `struct icmp6stat`.
        let mut size = 0usize;
        icmp6_sysctl(&[ICMPV6CTL_STATS], 0, &mut size, 0, 0).expect("size of the statistics");
        assert_eq!(size, size_of::<Icmp6stat>());
    }
}
/* </TESTS> */
