/*	$OpenBSD: raw_ip6.h,v 1.4 2017/02/09 15:23:35 jca Exp $	*/
/*	$KAME: raw_ip6.h,v 1.2 2001/05/27 13:28:35 itojun Exp $	*/
/*	$OpenBSD: raw_ip6.c,v 1.195 2026/09/17 15:56:59 bluhm Exp $	*/
/*	$KAME: raw_ip6.c,v 1.69 2001/03/04 15:55:44 itojun Exp $	*/
/* <LICENSES> */
/*
 * Copyright (C) 2001 WIDE Project.
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
 *	@(#)raw_ip.c	8.2 (Berkeley) 1/4/94
 */
/* </LICENSES> */

//! Raw IPv6 sockets: the statistics of `<netinet6/raw_ip6.h>`, and `netinet6/raw_ip6.c`,
//! the raw IPv6 protocol (input to every matching socket, output with a caller-built
//! payload, the `IPV6_CHECKSUM` and `ICMP6_FILTER` options).
//!
//! Upstream: sys/netinet6/raw_ip6.h @ 3ce1f3f79392
//! Upstream: sys/netinet6/raw_ip6.c @ 3ce1f3f79392
//!
//! `raw_ip6.c` shares the module with the header. ICMPv6 statistics are counted separately
//! (`netinet/icmp6.rs`).
//!
//! A `SOCK_RAW` socket of the inet6 domain (ping6's `IPPROTO_ICMPV6` one, or any protocol
//! number) has a control block in `rawin6pcbtable`. `rip6_input`, the input of every
//! protocol without a handler of its own and the end of `icmp6_input`, hands a copy of each
//! datagram, the headers before `*offp` stripped, to every raw socket whose protocol,
//! addresses, ICMPv6 filter and (with `IPV6_CHECKSUM`) checksum match. `rip6_output`
//! prepends an IPv6 header and, for ICMPv6 or with `IPV6_CHECKSUM`, computes the checksum
//! at its offset, then sends the datagram with `ip6_output`.
//!
//! ## Deviations
//! - `enum rip6stat_counters` is [`Rip6statCounters`]; `rip6counters` (`struct cpumem *`) is
//!   the static array of atomics [`RIP6COUNTERS`]; `counters_alloc` in `rip6_init` has
//!   nothing left to do.
//! - `RIPM6CTL_NAMES` is a `Ctlname` table.
//! - The IPv6 header is read and written as a copy (`ip6_var.rs`'s `mtod_ip6`), since mbuf
//!   data need not be aligned; `rip6_sbappend` does not take the header the C passes and
//!   does not use.
//! - `rip6_output` takes the destination as a `sockaddr_in6` (the C's `struct sockaddr *`
//!   is always one; its family is still checked).
//! - `ip6counters`' `counters_enter` pair (`ip6s_delivered` down) is one atomic update.
//! - `rip6_ctlinput` reads `ip6c_src` as an optional copy.
//! - `rip6_send` without a packet (which `sosend` never does) fails with `EINVAL`, as
//!   `rip_send` does.
//! - `MROUTING` (`ip6_mroute.c`) is not configured: the `MRT6_*` options go to
//!   `ip6_ctloutput` as any other, and `rip6_detach` has no `ip6_mrouter_done`; comments at
//!   their sites. `NPF` is configured: the divert-to key and `pf_mbuf_link_inpcb`.
//!   `SMALL_KERNEL` is not set: `rip6_sysctl` is here.

use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_sysctl::sysctl_rdstruct;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_adj, m_copym, m_freem, m_prepend, m_pullup};
use crate::kern::uipc_mbuf2::m_pulldown;
use crate::kern::uipc_socket::sorwakeup;
use crate::kern::uipc_socket2::{
    sbappendaddr, soassertlocked, socantsendmore, soisconnected, soisdisconnected, soreserve,
};
use crate::net::if_var::Netstack;
use crate::net::pf::{pf_find_divert, pf_mbuf_link_inpcb};
use crate::net::pfvar::{PF_DIVERT_REPLY, PF_DIVERT_TO};
use crate::net::rtable::rtable_l2;
use crate::netinet::icmp6::{
    ICMP6_PARAM_PROB, ICMP6_PARAMPROB_NEXTHEADER, Icmp6Filter, Icmp6Hdr, Icmp6statCounters,
    icmp6stat_inc_hist,
};
use crate::netinet::in_::{IPPROTO_DONE, IPPROTO_ICMPV6, IPPROTO_IPV6, IPPROTO_MAX, IPPROTO_NONE};
use crate::netinet::in_pcb::{
    IN6P_CONTROLOPTS, IN6P_MINMTU, INP_IPV6, InpNotifyFn, Inpcb, InpcbIterator, Inpcbtable,
    in_pcb_iterator, in_pcballoc, in_pcbdetach, in_pcbinit, in_pcbref, in_pcbrtchange, in_pcbunref,
    sotoinpcb,
};
use crate::netinet::ip6::{
    IPV6_FLOWINFO_MASK, IPV6_VERSION, IPV6_VERSION_MASK, Ip6Hdr, ip6_exthdr_get,
};
use crate::netinet6::icmp6::{icmp6_ctloutput, icmp6_error, icmp6_mtudisc_update};
use crate::netinet6::in6::{
    IN6ADDR_ANY, IPV6_CHECKSUM, In6Addr, SA6_ANY, SockaddrIn6, in6_are_addr_equal, in6_control,
    in6_is_addr_unspecified, in6_is_addr_v4mapped, in6_nam2sin6, satosin6_const,
};
use crate::netinet6::in6_cksum::in6_cksum;
use crate::netinet6::in6_pcb::{
    in6_pcbaddrisavail, in6_pcblookup, in6_pcbnotify, in6_peeraddr, in6_sockaddr, inp_moptions6,
    inp_outputopts6,
};
use crate::netinet6::in6_proto::{RIP6_RECVSPACE, RIP6_SENDSPACE};
use crate::netinet6::in6_src::{in6_embedscope, in6_pcbselsrc, in6_recoverscope, in6_selecthlim};
use crate::netinet6::ip6_input::{INET6CTLERRMAP, IP6COUNTERS, ip6_get_prevhdr, ip6_savecontrol};
use crate::netinet6::ip6_output::{
    ip6_clearpktopts, ip6_ctloutput, ip6_output, ip6_raw_ctloutput, ip6_setpktopts,
};
use crate::netinet6::ip6_var::{
    IPV6_MINMTU, Ip6Pktopts, Ip6statCounters, mtod_ip6, mtod_ip6_store,
};
use crate::netinet6::ip6protosw::Ip6ctlparam;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_PCB, M_WAITOK};
use crate::sys::mbuf::{M_COPYALL, M_DONTWAIT, M_MCAST, M_WAIT, Mbuf, PF_TAG_DIVERTED, mtod};
use crate::sys::proc::Proc;
use crate::sys::protosw::{PRC_HOSTDEAD, PRC_MSGSIZE, PRC_NCMDS, PrUsrreqs, prc_is_redirect};
use crate::sys::socket::{AF_INET6, Sockaddr};
use crate::sys::socketvar::{SS_CANTRCVMORE, SS_ISCONNECTED, SS_PRIV, Socket};
use crate::sys::sysctl::{CTLTYPE_NODE, Ctlname};

/// RIP6 stats.
pub const RIPV6CTL_STATS: i32 = 1;
/// `RIPV6CTL_MAXID`.
pub const RIPV6CTL_MAXID: i32 = 2;

/// `RIPM6CTL_NAMES`.
pub const RIPM6CTL_NAMES: [Ctlname; RIPV6CTL_MAXID as usize] =
    [Ctlname::NONE, Ctlname::new(b"stats", CTLTYPE_NODE)];

/// `struct rip6stat`: raw IPv6 statistics, as `sysctl(2)` returns them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rip6stat {
    /// Total input packets.
    pub rip6s_ipackets: u64,
    /// Input checksum computations.
    pub rip6s_isum: u64,
    /// Of above, checksum error.
    pub rip6s_badsum: u64,
    /// No matching socket.
    pub rip6s_nosock: u64,
    /// Of above, arrived as multicast.
    pub rip6s_nosockmcast: u64,
    /// Not delivered, input socket full.
    pub rip6s_fullsock: u64,
    /// Total output packets.
    pub rip6s_opackets: u64,
}

/// `enum rip6stat_counters`: the indices of [`RIP6COUNTERS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Rip6statCounters {
    /// `rip6s_ipackets`.
    Rip6sIpackets,
    /// `rip6s_isum`.
    Rip6sIsum,
    /// `rip6s_badsum`.
    Rip6sBadsum,
    /// `rip6s_nosock`.
    Rip6sNosock,
    /// `rip6s_nosockmcast`.
    Rip6sNosockmcast,
    /// `rip6s_fullsock`.
    Rip6sFullsock,
    /// `rip6s_opackets`.
    Rip6sOpackets,
    /// `rip6s_ncounters`.
    Rip6sNcounters,
}

/// The number of counters (`rip6s_ncounters`).
pub const RIP6S_NCOUNTERS: usize = Rip6statCounters::Rip6sNcounters as usize;

/// `rawin6pcbtable`.
pub static RAWIN6PCBTABLE: Inpcbtable = Inpcbtable::new();

/// `rip6counters`: the raw IPv6 statistics.
pub static RIP6COUNTERS: [AtomicU64; RIP6S_NCOUNTERS] =
    [const { AtomicU64::new(0) }; RIP6S_NCOUNTERS];

/// `rip6_usrreqs`.
pub static RIP6_USRREQS: PrUsrreqs = PrUsrreqs {
    pru_attach: Some(rip6_attach),
    pru_detach: Some(rip6_detach),
    pru_bind: Some(rip6_bind),
    pru_connect: Some(rip6_connect),
    pru_disconnect: Some(rip6_disconnect),
    pru_shutdown: Some(rip6_shutdown),
    pru_send: Some(rip6_send),
    pru_control: Some(in6_control),
    pru_sockaddr: Some(in6_sockaddr),
    pru_peeraddr: Some(in6_peeraddr),
    ..PrUsrreqs::NONE
};

/// `rip6stat_inc(c)`.
pub fn rip6stat_inc(c: Rip6statCounters) {
    RIP6COUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// The bytes of a `sockaddr_in6`.
fn sin6_bytes(sin6: &SockaddrIn6) -> &[u8] {
    // SAFETY: `SockaddrIn6` is `#[repr(C)]` without padding: its bytes are initialised.
    unsafe { slice::from_raw_parts(ptr::from_ref(sin6).cast::<u8>(), size_of::<SockaddrIn6>()) }
}

/// `sotoinpcb(so)` of a raw socket the C knows to be attached.
fn inpcb_of(so: &Socket) -> &'static Inpcb {
    match sotoinpcb(so) {
        Some(inp) => inp,
        None => panic(format_args!("raw6 socket {:p}: no inpcb", so)),
    }
}

/// The `sockaddr_in6` of an address mbuf, read out of it.
fn nam_sin6(nam: &Mbuf) -> Result<SockaddrIn6, Errno> {
    let sin6 = in6_nam2sin6(nam)?;
    // SAFETY: `in6_nam2sin6` checked the mbuf holds a whole `sockaddr_in6`; read unaligned.
    Ok(unsafe { sin6.read_unaligned() })
}

/// `rip6_init`: initializes the raw connection block table.
pub fn rip6_init() {
    in_pcbinit(&RAWIN6PCBTABLE, 1);
    // rip6counters = counters_alloc(rip6s_ncounters): a static array here.
}

/// `rip6_input`: raw IPv6's `pr_input`: hands a copy of datagram `*mp`, without the headers
/// before `*offp`, to every raw socket that matches it. Without one, ICMPv6 and "no next
/// header" are dropped and any other protocol is answered with a parameter problem pointing
/// at the next header field.
pub fn rip6_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let _ = ns;
    let iter = InpcbIterator::new();
    let mut inp: Option<&'static Inpcb> = None;
    let mut last: Option<&'static Inpcb> = None;
    let mut type_: u8 = 0;

    kassert!(af == i32::from(AF_INET6));

    if proto == IPPROTO_ICMPV6 {
        let Some(icmp6) = ip6_exthdr_get(mp, *offp, size_of::<Icmp6Hdr>() as i32) else {
            return IPPROTO_DONE;
        };
        // SAFETY: `ip6_exthdr_get` made the ICMPv6 header's bytes contiguous at `icmp6`; its
        // type is the first byte.
        type_ = unsafe { icmp6.read() };
    } else {
        rip6stat_inc(Rip6statCounters::Rip6sIpackets);
    }
    let Some(m) = mp.take() else {
        return IPPROTO_DONE;
    };
    let ip6 = mtod_ip6(m);

    // KAME hack: recover scopeid
    let mut rip6src = SockaddrIn6::with_addr(IN6ADDR_ANY);
    in6_recoverscope(&mut rip6src, &ip6.ip6_src);

    let mut key = ip6.ip6_dst;
    if m.m_pkthdr().pf.flags.get() & PF_TAG_DIVERTED != 0 {
        let divert = pf_find_divert(m);
        kassert!(divert.is_some());
        if let Some(divert) = divert {
            match divert.type_ {
                PF_DIVERT_TO => key = In6Addr::new(divert.addr.addr8),
                PF_DIVERT_REPLY => {}
                t => panic(format_args!(
                    "rip6_input: unknown divert type {t}, mbuf {m:p}"
                )),
            }
        }
    }
    mtx_enter(&RAWIN6PCBTABLE.inpt_mtx);
    // SAFETY: the table mutex is held around every call; `iter` lives on this frame until the
    // walk ends with `None`.
    while let Some(i) = unsafe { in_pcb_iterator(&RAWIN6PCBTABLE, inp, &iter) } {
        inp = Some(i);
        kassert!(i.has_flags(INP_IPV6));

        // Packet must not be inserted after disconnected wakeup call. To avoid race, check
        // again when holding receive buffer mutex.
        if i.socket().so_rcv.has_state(SS_CANTRCVMORE) {
            continue;
        }
        if rtable_l2(i.inp_rtableid.get()) != rtable_l2(m.m_pkthdr().ph_rtableid.get()) {
            continue;
        }

        let nxt = i.inp_ipv6.get().ip6_nxt;
        if (nxt != 0 || proto == IPPROTO_ICMPV6) && i32::from(nxt) != proto {
            continue;
        }
        if !in6_is_addr_unspecified(&i.inp_laddr6.get())
            && !in6_are_addr_equal(&i.inp_laddr6.get(), &key)
        {
            continue;
        }
        if !in6_is_addr_unspecified(&i.inp_faddr6.get())
            && !in6_are_addr_equal(&i.inp_faddr6.get(), &ip6.ip6_src)
        {
            continue;
        }
        if proto == IPPROTO_ICMPV6
            && let Some(filt) = i.inp_icmp6filt.get()
        {
            // SAFETY: the filter is `rip6_attach`'s allocation, freed only by `rip6_detach`
            // after the control block left this table's walks.
            if unsafe { filt.as_ref() }.willblock(type_) {
                continue;
            }
        }
        let cksum6 = i.inp_cksum6.get();
        if proto != IPPROTO_ICMPV6 && cksum6 != -1 {
            rip6stat_inc(Rip6statCounters::Rip6sIsum);
            // Although in6_cksum() does not need the position of the checksum field for
            // verification, enforce that it is located within the packet. Userland has
            // given a checksum offset, a packet too short for that is invalid. Avoid
            // overflow with user supplied offset.
            let len = m.m_pkthdr().len.get();
            if len < *offp + 2
                || len - *offp - 2 < cksum6
                || in6_cksum(m, proto as u8, *offp as u32, (len - *offp) as u32) != 0
            {
                rip6stat_inc(Rip6statCounters::Rip6sBadsum);
                continue;
            }
        }

        if let Some(l) = last {
            mtx_leave(&RAWIN6PCBTABLE.inpt_mtx);

            if let Some(n) = m_copym(m, 0, M_COPYALL, M_DONTWAIT) {
                rip6_sbappend(l, n, *offp, &rip6src);
            }
            in_pcbunref(Some(l));

            mtx_enter(&RAWIN6PCBTABLE.inpt_mtx);
        }
        last = in_pcbref(Some(i));
    }
    mtx_leave(&RAWIN6PCBTABLE.inpt_mtx);

    let Some(last) = last else {
        if proto != IPPROTO_ICMPV6 {
            rip6stat_inc(Rip6statCounters::Rip6sNosock);
            if m.m_flags().get() & M_MCAST != 0 {
                rip6stat_inc(Rip6statCounters::Rip6sNosockmcast);
            }
        }
        if proto == IPPROTO_NONE || proto == IPPROTO_ICMPV6 {
            m_freem(m);
        } else {
            let prvnxt = ip6_get_prevhdr(m, *offp);

            icmp6_error(m, ICMP6_PARAM_PROB, ICMP6_PARAMPROB_NEXTHEADER, prvnxt);
        }
        IP6COUNTERS[Ip6statCounters::Ip6sDelivered as usize].fetch_sub(1, Ordering::Relaxed);

        return IPPROTO_DONE;
    };

    rip6_sbappend(last, m, *offp, &rip6src);
    in_pcbunref(Some(last));

    IPPROTO_DONE
}

/// `rip6_sbappend`: queues datagram `m` from `rip6src`, its first `hlen` bytes (the IPv6
/// and intermediate headers) stripped, with the control messages its socket asked for, on
/// the receive buffer of `inp`'s socket.
pub fn rip6_sbappend(inp: &Inpcb, m: &'static Mbuf, hlen: i32, rip6src: &SockaddrIn6) {
    let so = inp.socket();
    let mut opts = None;
    let mut ret = false;

    if inp.has_flags(IN6P_CONTROLOPTS) {
        ip6_savecontrol(inp, m, &mut opts);
    }
    // strip intermediate headers
    m_adj(m, hlen);

    mtx_enter(&so.so_rcv.sb_mtx);
    if !so.so_rcv.has_state(SS_CANTRCVMORE) {
        ret = sbappendaddr(&so.so_rcv, sin6_bytes(rip6src), Some(m), opts);
    }
    mtx_leave(&so.so_rcv.sb_mtx);

    if !ret {
        m_freem(m);
        m_freem(opts);
        rip6stat_inc(Rip6statCounters::Rip6sFullsock);
    } else {
        sorwakeup(so);
    }
}

/// `rip6_ctlinput`: raw IPv6's `pr_ctlinput`: a path MTU change (`PRC_MSGSIZE`) updates
/// the route if a raw socket matches the quoted packet, then every matching socket is
/// notified (`in6_pcbnotify`).
///
/// # Safety
///
/// `sa` points at a readable socket address of its `sa_len` bytes; `d` is NULL or
/// the `Ip6ctlparam` of the ICMPv6 error, valid for the call.
pub unsafe fn rip6_ctlinput(cmd: i32, sa: *const Sockaddr, rdomain: u32, d: *mut c_void) {
    let mut d = d;
    let mut notify: InpNotifyFn = in_pcbrtchange;

    // SAFETY: the caller's contract: `sa` is readable, its length and family at least.
    let (family, len) = unsafe { ((*sa).sa_family, (*sa).sa_len) };
    if family != AF_INET6 || usize::from(len) != size_of::<SockaddrIn6>() {
        return;
    }
    // SAFETY: as above; it holds a whole `sockaddr_in6` (checked), read unaligned.
    let sa6 = unsafe { satosin6_const(sa).read_unaligned() };

    if cmd as u32 >= PRC_NCMDS as u32 {
        return;
    }
    if prc_is_redirect(cmd) {
        notify = in_pcbrtchange;
        d = ptr::null_mut();
    } else if cmd == PRC_HOSTDEAD {
        d = ptr::null_mut();
    } else if cmd == PRC_MSGSIZE {
        // special code is present, see below
    } else if INET6CTLERRMAP[cmd as usize].is_none() {
        return;
    }

    // if the parameter is from icmp6, decode it.
    let ip6cp: Option<&Ip6ctlparam> = if d.is_null() {
        None
    } else {
        // SAFETY: the caller's contract: a non-NULL `d` is the ICMPv6 error's parameter.
        Some(unsafe { &*d.cast::<Ip6ctlparam>() })
    };
    let (ip6, cmdarg, sa6_src, nxt) = match ip6cp {
        Some(ip6cp) => (
            ip6cp.ip6c_ip6,
            ip6cp.ip6c_cmdarg,
            // SAFETY: `icmp6_notify_error` points `ip6c_src` at its source address, valid
            // for the call; read unaligned.
            (!ip6cp.ip6c_src.is_null()).then(|| unsafe { ip6cp.ip6c_src.read_unaligned() }),
            i32::from(ip6cp.ip6c_nxt),
        ),
        None => (ptr::null_mut(), ptr::null_mut(), Some(SA6_ANY), -1),
    };

    if let Some(ip6cp) = ip6cp
        && !ip6.is_null()
        && cmd == PRC_MSGSIZE
    {
        // Check to see if we have a valid raw IPv6 socket corresponding to the address in
        // the ICMPv6 message payload, and the protocol (ip6_nxt) meets the socket.
        // XXX chase extension headers, or pass final nxt value from icmp6_notify_error()
        let src = sa6_src.map_or(IN6ADDR_ANY, |s| s.sin6_addr);
        let inp = in6_pcblookup(&RAWIN6PCBTABLE, &sa6.sin6_addr, 0, &src, 0, rdomain);

        let valid = inp.is_some_and(|i| {
            let n = i.inp_ipv6.get().ip6_nxt;
            n != 0 && i32::from(n) == nxt
        });

        // Depending on the value of "valid" and routing table size (mtudisc_{hi,lo}wat), we
        // will:
        // - recalculate the new MTU and create the corresponding routing entry, or
        // - ignore the MTU change notification.
        icmp6_mtudisc_update(ip6cp, valid);
        in_pcbunref(inp);

        // regardless of if we called icmp6_mtudisc_update(), we need to call
        // in6_pcbnotify(), to notify path MTU change to the userland (2292bis-02), because
        // some unconnected sockets may share the same destination and want to know the
        // path MTU.
    }

    in6_pcbnotify(
        &RAWIN6PCBTABLE,
        &sa6,
        0,
        sa6_src.as_ref(),
        0,
        rdomain,
        cmd,
        cmdarg,
        Some(notify),
    );
}

/// `rip6_output`: generates an IPv6 header and passes the packet to `ip6_output`, with the
/// options the user may have set up with control calls (`control`, else the socket's
/// sticky ones). ICMPv6, and any protocol with `IPV6_CHECKSUM`, get their checksum here.
/// Consumes `m` and `control`.
pub fn rip6_output(
    m: &'static Mbuf,
    so: &'static Socket,
    dstaddr: &SockaddrIn6,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let inp = inpcb_of(so);
    let plen = m.m_pkthdr().len.get() as u32;
    let mut opt = Ip6Pktopts::default();
    let proto = i32::from(so.so_proto.pr_protocol);
    let mut m: Option<&'static Mbuf> = Some(m);

    let priv_ = so.has_state(SS_PRIV);

    let result: Result<(), Errno> = 'freectl: {
        let error: Errno = 'bad: {
            let optp: Option<&Ip6Pktopts> = if let Some(control) = control {
                if let Err(e) =
                    ip6_setpktopts(control, &mut opt, inp_outputopts6(inp), priv_, proto)
                {
                    break 'bad e;
                }
                Some(&opt)
            } else {
                inp_outputopts6(inp)
            };

            if dstaddr.sin6_family != AF_INET6 {
                break 'bad Errno::EAFNOSUPPORT;
            }
            let dst = dstaddr.sin6_addr;
            if in6_is_addr_v4mapped(&dst) {
                break 'bad Errno::EADDRNOTAVAIL;
            }

            // For an ICMPv6 packet, we should know its type and code to update statistics.
            let mut type_: u8 = 0;
            if proto == IPPROTO_ICMPV6 {
                let Some(m0) = m else {
                    break 'bad Errno::ENOBUFS;
                };
                if (m0.m_len().get() as usize) < size_of::<Icmp6Hdr>() {
                    m = m_pullup(m0, size_of::<Icmp6Hdr>() as i32);
                }
                let Some(m0) = m else {
                    break 'bad Errno::ENOBUFS;
                };
                // SAFETY: the first mbuf holds the whole ICMPv6 header (pulled up); its type
                // is the first byte.
                type_ = unsafe { mtod::<u8>(m0).read() };
            }

            let Some(m0) = m else {
                break 'bad Errno::ENOBUFS;
            };
            m = m_prepend(m0, size_of::<Ip6Hdr>() as i32, M_DONTWAIT);
            let Some(mm) = m else {
                break 'bad Errno::ENOBUFS;
            };
            let mut ip6 = mtod_ip6(mm);

            // Next header might not be ICMP6 but use its pseudo header anyway.
            ip6.ip6_dst = dst;

            // KAME hack: embed scopeid
            if in6_embedscope(&mut ip6.ip6_dst, dstaddr, optp, inp_moptions6(inp)).is_err() {
                break 'bad Errno::EINVAL;
            }

            // Source address selection.
            let mut in6a = In6Addr::default();
            if let Err(e) = in6_pcbselsrc(&mut in6a, dstaddr, inp, optp) {
                break 'bad e;
            }
            ip6.ip6_src = in6a;

            ip6.ip6_flow = inp.inp_flowinfo() & IPV6_FLOWINFO_MASK;
            ip6.set_ip6_vfc((ip6.ip6_vfc() & !IPV6_VERSION_MASK) | IPV6_VERSION);
            // ip6_plen will be filled in ip6_output.
            ip6.ip6_nxt = inp.inp_ipv6.get().ip6_nxt;
            ip6.ip6_hlim = in6_selecthlim(inp) as u8;
            mtod_ip6_store(mm, &ip6);

            let cksum6 = inp.inp_cksum6.get();
            if proto == IPPROTO_ICMPV6 || cksum6 != -1 {
                // compute checksum
                let off = if proto == IPPROTO_ICMPV6 {
                    offset_of!(Icmp6Hdr, icmp6_cksum) as i32
                } else {
                    cksum6
                };
                if plen < 2 || i64::from(plen) - 2 < i64::from(off) {
                    break 'bad Errno::EINVAL;
                }
                let off = off + size_of::<Ip6Hdr>() as i32;

                let mut sumoff = 0;
                let Some(n) = m_pulldown(mm, off, size_of::<u16>() as i32, Some(&mut sumoff))
                else {
                    m = None;
                    break 'bad Errno::ENOBUFS;
                };
                // SAFETY: `m_pulldown` made the two checksum bytes contiguous in `n` at
                // `sumoff`; written unaligned.
                let sump = unsafe { mtod::<u8>(n).add(sumoff as usize).cast::<u16>() };
                // SAFETY: as above.
                unsafe { sump.write_unaligned(0) };
                let sum = in6_cksum(mm, ip6.ip6_nxt, size_of::<Ip6Hdr>() as u32, plen);
                // SAFETY: as above.
                unsafe { sump.write_unaligned(sum) };
            }

            let mut flags = 0;
            if inp.has_flags(IN6P_MINMTU) {
                flags |= IPV6_MINMTU;
            }

            // force routing table
            mm.m_pkthdr().ph_rtableid.set(inp.inp_rtableid.get());

            if inp.socket().has_state(SS_ISCONNECTED) && proto != IPPROTO_ICMPV6 {
                pf_mbuf_link_inpcb(mm, Some(inp));
            }

            let error = ip6_output(
                mm,
                optp,
                Some(&inp.inp_route),
                flags,
                inp_moptions6(inp),
                Some(&inp.inp_seclevel.get()),
            );
            if proto == IPPROTO_ICMPV6 {
                icmp6stat_inc_hist(Icmp6statCounters::Icp6sOuthist, type_);
            } else {
                rip6stat_inc(Rip6statCounters::Rip6sOpackets);
            }

            break 'freectl error;
        };

        // bad:
        m_freem(m);
        Err(error)
    };

    // freectl:
    if let Some(control) = control {
        ip6_clearpktopts(&mut opt, -1);
        m_freem(control);
    }
    result
}

/// `rip6_ctloutput`: raw IPv6 socket option processing: `IPV6_CHECKSUM` (and
/// `ICMP6_FILTER` at the ICMPv6 level), then `ip6_ctloutput`.
pub fn rip6_ctloutput(
    op: i32,
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    match level {
        IPPROTO_IPV6 => match optname {
            // MROUTING: MRT6_INIT, MRT6_DONE, MRT6_ADD_MIF, MRT6_DEL_MIF, MRT6_ADD_MFC and
            // MRT6_DEL_MFC through ip6_mrouter_set / ip6_mrouter_get; not configured.
            IPV6_CHECKSUM => ip6_raw_ctloutput(op, so, level, optname, m),
            _ => ip6_ctloutput(op, so, level, optname, m),
        },

        // XXX: is it better to call icmp6_ctloutput() directly from protosw?
        IPPROTO_ICMPV6 => icmp6_ctloutput(op, so, level, optname, m),

        _ => Err(Errno::EINVAL),
    }
}

/// `rip6_attach`: a control block for a privileged raw IPv6 socket of protocol `proto`,
/// without checksum offset and with an ICMPv6 filter that passes everything.
pub fn rip6_attach(so: &'static Socket, proto: i32, wait: i32) -> Result<(), Errno> {
    if !so.so_pcb.get().is_null() {
        panic(format_args!("rip6_attach"));
    }
    if !so.has_state(SS_PRIV) {
        return Err(Errno::EACCES);
    }
    if !(0..IPPROTO_MAX).contains(&proto) {
        return Err(Errno::EPROTONOSUPPORT);
    }

    soreserve(so, RIP6_SENDSPACE, RIP6_RECVSPACE)?;
    in_pcballoc(so, &RAWIN6PCBTABLE, wait)?;

    let inp = inpcb_of(so);
    let mut ip6 = inp.inp_ipv6.get();
    ip6.ip6_nxt = proto as u8;
    inp.inp_ipv6.set(ip6);
    inp.inp_cksum6.set(-1);

    let Some(filt) = malloc(
        size_of::<Icmp6Filter>(),
        M_PCB,
        if wait == M_WAIT { M_WAITOK } else { M_NOWAIT },
    ) else {
        in_pcbdetach(inp);
        return Err(Errno::ENOMEM);
    };
    let filt = filt.cast::<Icmp6Filter>();
    let mut passall = Icmp6Filter::default();
    passall.setpassall();
    // SAFETY: a fresh `malloc(9)` allocation of a filter's size and alignment, written once
    // before the control block publishes it.
    unsafe { filt.as_ptr().write(passall) };
    inp.inp_icmp6filt.set(Some(filt));

    Ok(())
}

/// `rip6_detach`: frees the ICMPv6 filter and the control block.
pub fn rip6_detach(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);

    let Some(inp) = sotoinpcb(so) else {
        panic(format_args!("rip6_detach"));
    };
    // MROUTING: ip6_mrouter_done(so); not configured.
    if let Some(filt) = inp.inp_icmp6filt.take() {
        free(filt.cast(), M_PCB, size_of::<Icmp6Filter>());
    }

    in_pcbdetach(inp);

    Ok(())
}

/// `rip6_bind`: a local address of ours (or the wildcard) for `so`. Local ports are
/// nonsensical for raw sockets.
pub fn rip6_bind(so: &'static Socket, nam: &'static Mbuf, p: &Proc) -> Result<(), Errno> {
    let inp = inpcb_of(so);

    soassertlocked(so);

    let mut addr = nam_sin6(nam)?;

    // Make sure to not enter in_pcblookup_local(), local ports are non-sensical for raw
    // sockets.
    addr.sin6_port = 0;

    in6_pcbaddrisavail(inp, &mut addr, 0, p)?;

    mtx_enter(&RAWIN6PCBTABLE.inpt_mtx);
    inp.inp_laddr6.set(addr.sin6_addr);
    mtx_leave(&RAWIN6PCBTABLE.inpt_mtx);

    Ok(())
}

/// `rip6_connect`: the foreign address of `so`, and a source address for it.
pub fn rip6_connect(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    let inp = inpcb_of(so);

    soassertlocked(so);

    let addr = nam_sin6(nam)?;

    // Source address selection. XXX: need pcblookup?
    let mut in6a = In6Addr::default();
    in6_pcbselsrc(&mut in6a, &addr, inp, inp_outputopts6(inp))?;

    mtx_enter(&RAWIN6PCBTABLE.inpt_mtx);
    inp.inp_laddr6.set(in6a);
    inp.inp_faddr6.set(addr.sin6_addr);
    mtx_leave(&RAWIN6PCBTABLE.inpt_mtx);
    soisconnected(so);

    Ok(())
}

/// `rip6_disconnect`.
pub fn rip6_disconnect(so: &'static Socket) -> Result<(), Errno> {
    let inp = inpcb_of(so);

    soassertlocked(so);

    if !so.has_state(SS_ISCONNECTED) {
        return Err(Errno::ENOTCONN);
    }

    soisdisconnected(so);
    mtx_enter(&RAWIN6PCBTABLE.inpt_mtx);
    inp.inp_faddr6.set(IN6ADDR_ANY);
    mtx_leave(&RAWIN6PCBTABLE.inpt_mtx);

    Ok(())
}

/// `rip6_shutdown`: marks the connection as being incapable of further input.
pub fn rip6_shutdown(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);
    socantsendmore(so);

    Ok(())
}

/// `rip6_send`: ships a packet out to the connected address or `nam`; `rip6_output`
/// handles any messaging necessary.
pub fn rip6_send(
    so: &'static Socket,
    m: Option<&'static Mbuf>,
    nam: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let inp = inpcb_of(so);

    soassertlocked(so);

    // always copy sockaddr to avoid overwrites
    let mut dst = SockaddrIn6::with_addr(IN6ADDR_ANY);
    let error = 'out: {
        if so.has_state(SS_ISCONNECTED) {
            if nam.is_some() {
                break 'out Errno::EISCONN;
            }
            dst.sin6_addr = inp.inp_faddr6.get();
        } else {
            let Some(nam) = nam else {
                break 'out Errno::ENOTCONN;
            };
            match nam_sin6(nam) {
                Ok(addr6) => {
                    dst.sin6_addr = addr6.sin6_addr;
                    dst.sin6_scope_id = addr6.sin6_scope_id;
                }
                Err(e) => break 'out e,
            }
        }
        // sosend always hands over a packet (a pkthdr mbuf, empty or not).
        let Some(m) = m else {
            break 'out Errno::EINVAL;
        };
        return rip6_output(m, so, &dst, control);
    };

    m_freem(control);
    m_freem(m);

    Err(error)
}

/// `rip6_sysctl_rip6stat`: `net.inet6.ip6.rip6.stats`, the counters as a `struct rip6stat`.
fn rip6_sysctl_rip6stat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    let mut bytes = [0u8; RIP6S_NCOUNTERS * size_of::<u64>()];
    for (i, c) in RIP6COUNTERS.iter().enumerate() {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&c.load(Ordering::Relaxed).to_ne_bytes());
    }

    sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
}

/// `rip6_sysctl`: the raw IPv6 sysctls (`RIPV6CTL_STATS`).
pub fn rip6_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let _ = newlen;
    // All sysctl names at this level are terminal.
    let [n] = name else {
        return Err(Errno::ENOTDIR);
    };

    match *n {
        RIPV6CTL_STATS => rip6_sysctl_rip6stat(oldp, oldlenp, newp),
        _ => Err(Errno::EOPNOTSUPP),
    }
}

// The counters are the statistics' words (the `CTASSERT` of `rip6_sysctl_rip6stat`).
const _: () = assert!(size_of::<Rip6stat>() == RIP6S_NCOUNTERS * size_of::<u64>());

#[cfg(test)]
mod tests;
