/*	$OpenBSD: ip_output.c,v 1.420 2026/08/05 09:43:19 bluhm Exp $	*/
/*	$NetBSD: ip_output.c,v 1.28 1996/02/13 23:43:07 christos Exp $	*/
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
 *	@(#)ip_output.c	8.3 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! IPv4 output: `ip_output`, fragmentation, option insertion, multicast loopback and the
//! output checksums.
//!
//! Upstream: sys/netinet/ip_output.c @ 3ce1f3f79392
//!
//! `ip_output` takes a packet with a skeletal IP header (length, offset, ttl, protocol, tos,
//! source, destination), completes it, finds the route and the interface (caching the route
//! in a `struct route`) and hands the packet to `if_output_tso`, fragmenting it with
//! `ip_fragment` when it is too large for the interface.
//!
//! The socket option half (`ip_ctloutput`, the `pr_ctloutput` of UDP and, behind
//! `rip_ctloutput`, of raw IP) keeps a socket's IP options, header fields and multicast
//! options in its `inpcb`.
//!
//! Status: `ported` (M7b output path, M9a socket options).
//!
//! ## Deviations
//! - `ip_output`'s `const struct ipsec_level *seclevel` is not a parameter: IPsec is not
//!   configured and every caller passes NULL. Its `struct route *` and `struct ip_moptions *`
//!   are `Option`s; the error is a `Result`. The packet (and `opt`) are `&'static Mbuf`s.
//! - The IP header is read and written as a copy (`ip_var.rs`'s `mtod_ip`/`mtod_ip_store`),
//!   since mbuf data has no 4-byte alignment guarantee.
//! - `ip_ctloutput`'s option values are read and written unaligned in the option mbuf;
//!   `ip_pcbopts` builds the `struct ipoption` in a local buffer, large enough for what the
//!   C may write past `ipopt_list` before its final length check, and copies it into the mbuf.
//!   `ip_setmoptions`'s `malloc(M_WAITOK)` cannot fail in C: here its failure panics.
//!   `IPSEC` is not configured: the security levels answer `EOPNOTSUPP` to a set and
//!   `IPSEC_LEVEL_NONE` to a get, as the C's `#ifndef IPSEC` branches do.
//! - `struct tcphdr` (`<netinet/tcp.h>`) is not ported: the offset of `th_sum` is a constant
//!   here, and `tcpstat_inc(tcps_outswcsum)` is reported (`netinet/tcp_*.c`).
//! - `NPF` (pf(4)) is configured: `pf_test` filters the packet before it is sent, and a
//!   packet pf tagged `PF_TAG_REROUTE` reruns the route lookup (the C's `goto reroute` is a
//!   labelled loop). The `pf_test` on `enc0` in `ip_output_ipsec_send` comes with `IPSEC`.
//! - Not configured, each a comment at its site: `IPSEC` (`ip_output_ipsec_lookup`,
//!   `ip_output_ipsec_pmtu_update`, `ip_output_ipsec_send`, `ipsec_adjust_mtu`) and
//!   `MROUTING` (`ip_mforward`).
//! - `in_cksum_phdr`, `in_delayed_cksum` and `in_proto_cksum_out` write the checksum through
//!   `m_copyback` or, when it lies in the first mbuf, an unaligned store; `in_ifcap_cksum`
//!   answers `bool`.
//! - `KERNEL_LOCK()`/`KERNEL_UNLOCK()` are nothing without `MULTIPROCESSOR`.

use core::cell::Cell;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_prot::suser;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{
    MAX_LINKHDR, m_adj, m_copyback, m_copym, m_dup_pkt, m_dup_pkthdr, m_freem, m_get, m_gethdr,
    ml_enqueue, ml_init, ml_purge,
};
use crate::machine::cpu::curproc;
use crate::net::if_::{
    IFCAP_CSUM_IPv4, IFCAP_CSUM_TCPv4, IFCAP_CSUM_UDPv4, IFCAP_TSOv4, IFF_BROADCAST, IFF_LOOPBACK,
    IFF_MULTICAST, IFF_SIMPLEX, if_get, if_input_local, if_output_ml, if_output_tso, if_put,
    ifa_ifwithaddr,
};
use crate::net::if_var::Ifnet;
use crate::net::pf::pf_test;
use crate::net::pfvar::{PF_FWD, PF_OUT, PF_PASS};
use crate::net::route::{
    RT_RESOLVE, RTF_BROADCAST, RTF_GATEWAY, RTF_HOST, RTF_LOCAL, RTV_MTU, Route, route_cache,
    rtalloc, rtalloc_mpath, rtfree, rtisvalid,
};
use crate::net::rtable::{rtable_l2, rtable_loindex};
use crate::netinet::in_::{
    IN_CLASSA_NSHIFT, INADDR_ANY, INADDR_BROADCAST, IP_ADD_MEMBERSHIP, IP_AUTH_LEVEL,
    IP_DEFAULT_MULTICAST_LOOP, IP_DEFAULT_MULTICAST_TTL, IP_DROP_MEMBERSHIP, IP_ESP_NETWORK_LEVEL,
    IP_ESP_TRANS_LEVEL, IP_IPCOMP_LEVEL, IP_IPDEFTTL, IP_IPSEC_LOCAL_ID, IP_IPSEC_REMOTE_ID,
    IP_IPSECFLOWINFO, IP_MAX_MEMBERSHIPS, IP_MIN_MEMBERSHIPS, IP_MINTTL, IP_MULTICAST_IF,
    IP_MULTICAST_LOOP, IP_MULTICAST_TTL, IP_OPTIONS, IP_PIPEX, IP_PORTRANGE, IP_PORTRANGE_DEFAULT,
    IP_PORTRANGE_HIGH, IP_PORTRANGE_LOW, IP_RECVDSTADDR, IP_RECVDSTPORT, IP_RECVIF, IP_RECVOPTS,
    IP_RECVRETOPTS, IP_RECVRTABLE, IP_RECVTTL, IP_RETOPTS, IP_TOS, IP_TTL, IPPROTO_ICMP,
    IPPROTO_IP, IPPROTO_TCP, IPPROTO_UDP, IPSEC_LEVEL_NONE, InAddr, IpMreq, IpMreqn, SockaddrIn,
    in_addmulti, in_delmulti, in_hasmulti, in_ifp2ia, in_multicast, satosin, sintosa,
};
use crate::netinet::in_cksum::in_cksum;
use crate::netinet::in_pcb::{
    INP_HIGHPORT, INP_IPSECFLOWINFO, INP_LOWPORT, INP_RECVDSTADDR, INP_RECVDSTPORT, INP_RECVIF,
    INP_RECVOPTS, INP_RECVRETOPTS, INP_RECVRTABLE, INP_RECVTTL, in_pcbset_rtableid, sotoinpcb,
};
use crate::netinet::in_var::{InMulti, ifatoia};
use crate::netinet::in4_cksum::in4_cksum;
use crate::netinet::ip::{
    IP_DF, IP_MAXPACKET, IP_MF, IPOPT_EOL, IPOPT_LSRR, IPOPT_MINOFF, IPOPT_NOP, IPOPT_OFFSET,
    IPOPT_OLEN, IPOPT_OPTVAL, IPOPT_SSRR, IPVERSION, Ip, MAXTTL, ipopt_copied,
};
use crate::netinet::ip_icmp::{ICMP_CKSUM_OFFSET, icmp_mtudisc_clone};
use crate::netinet::ip_id::ip_randomid;
use crate::netinet::ip_input::IP_DEFTTL;
use crate::netinet::ip_var::{
    IP_ALLOWBROADCAST, IP_FORWARDING, IP_MTUDISC, IP_RAWOUTPUT, IpMoptions, Ipoption,
    IpstatCounters, MAX_IPOPTLEN, ipstat_add, ipstat_inc, mtod_ip, mtod_ip_store,
};
use crate::netinet::udp::Udphdr;
use crate::netinet::udp_var::{UdpstatCounters, udpstat_inc};
use crate::sys::endian::{htonl, htons, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_IPMOPTS, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{
    M_BCAST, M_DONTWAIT, M_EXT, M_ICMP_CSUM_OUT, M_IPV4_CSUM_OUT, M_MCAST, M_TCP_CSUM_OUT,
    M_TCP_TSO, M_UDP_CSUM_OUT, MT_HEADER, MT_SOOPTS, Mbuf, MbufList, PF_TAG_GENERATED,
    PF_TAG_REROUTE, m_move_hdr, ml_len, mtod,
};
use crate::sys::protosw::{PRCO_GETOPT, PRCO_SETOPT};
use crate::sys::socket::{AF_INET, SO_RTABLE};
use crate::sys::socketvar::Socket;
use crate::sys::systm::net_assert_locked;
use crate::{kassert, unported};

/// `offsetof(struct tcphdr, th_sum)` (`<netinet/tcp.h>` is not ported).
const TH_SUM_OFFSET: usize = 16;
/// `offsetof(struct udphdr, uh_sum)`.
const UH_SUM_OFFSET: usize = core::mem::offset_of!(Udphdr, uh_sum);

/// `ip_output`: IP output. The packet in mbuf chain `m` contains a skeletal IP header (with
/// len, off, ttl, proto, tos, src, dst). The mbuf chain containing the packet will be freed.
/// The mbuf `opt`, if present, will not be freed.
pub fn ip_output(
    m: &'static Mbuf,
    opt: Option<&'static Mbuf>,
    ro: Option<&Route>,
    flags: i32,
    imo: Option<&IpMoptions>,
    _ipsecflowinfo: u32,
) -> Result<(), Errno> {
    let mut m = m;
    let mut ifp: Option<&'static Ifnet> = None;
    let ml = MbufList::new();
    let mut hlen = size_of::<Ip>() as i32;
    let iproute = Route::new();
    let mut mtu: u64;

    net_assert_locked("ip_output");

    #[cfg(feature = "diagnostic")]
    if m.m_flags().get() & crate::sys::mbuf::M_PKTHDR == 0 {
        crate::kern::subr_prf::panic(format_args!("ip_output no HDR"));
    }
    if let Some(opt) = opt {
        m = ip_insertoptions(m, opt, &mut hlen);
    }

    let mut ip = mtod_ip(m);

    // Fill in IP header.
    if flags & (IP_FORWARDING | IP_RAWOUTPUT) == 0 {
        ip.set_ip_v(IPVERSION);
        ip.ip_off &= htons(IP_DF);
        ip.ip_id = htons(ip_randomid());
        ip.set_ip_hl((hlen >> 2) as u8);
        ipstat_inc(IpstatCounters::IpsLocalout);
    } else {
        hlen = i32::from(ip.ip_hl()) << 2;
    }
    mtod_ip_store(m, &ip);
    let _ = hlen; // read by the IPsec lookup, which is not configured

    let mut ro = ro.unwrap_or(&iproute);
    let error: Result<(), Errno> = 'done: {
        let bad: Result<(), Errno> = 'bad: {
            // We should not send traffic to 0/8 say both Stevens and RFCs 5735 section 3 and
            // 1122 sections 3.2.1.3 and 3.3.6.
            if (ntohl(ip.ip_dst.s_addr) >> IN_CLASSA_NSHIFT) == 0 {
                break 'bad Err(Errno::ENETUNREACH);
            }

            let orig_rtableid = m.m_pkthdr().ph_rtableid.get();
            // reroute: pf(4) asks for a new route lookup after it changed the packet.
            'reroute: loop {
                // Do a route lookup now in case we need the source address to do an SPD lookup in
                // IPsec; for most packets, the source address is set at a higher level protocol.
                // ICMPs and other packets though (e.g., traceroute) have a source address of
                // zeroes. If there is a cached route, check that it is to the same destination and
                // is still up. If not, free it and try again.
                let _ = route_cache(
                    ro,
                    &ip.ip_dst,
                    Some(&ip.ip_src),
                    m.m_pkthdr().ph_rtableid.get(),
                );
                let mut dst: *const SockaddrIn = ro.ro_dstsa().cast();

                let mcast_or_bcast =
                    in_multicast(ip.ip_dst.s_addr) || ip.ip_dst.s_addr == INADDR_BROADCAST;
                let imo_ifp = match imo {
                    Some(imo) if mcast_or_bcast => if_get(u32::from(imo.imo_ifidx)),
                    _ => None,
                };
                if let Some(i) = imo_ifp {
                    ifp = Some(i);
                    mtu = u64::from(i.if_mtu.get());
                    if ip.ip_src.s_addr == INADDR_ANY
                        && let Some(ia) = in_ifp2ia(i)
                    {
                        ip.ip_src = ia.ia_addr.get().sin_addr;
                        mtod_ip_store(m, &ip);
                    }
                } else {
                    if ro.ro_rt.get().is_none() {
                        let words = [ip.ip_src.s_addr];
                        // SAFETY: `ro_dstsa` is the `sockaddr_in` `route_cache` wrote.
                        ro.ro_rt.set(unsafe {
                            rtalloc_mpath(ro.ro_dstsa(), Some(&words), ro.ro_tableid.get() as u32)
                        });
                    }

                    let Some(rt) = ro.ro_rt.get() else {
                        ipstat_inc(IpstatCounters::IpsNoroute);
                        break 'bad Err(Errno::EHOSTUNREACH);
                    };

                    let ia = rt.rt_ifa.get().map(ifatoia);
                    ifp = if rt.rt_flags.get() & RTF_LOCAL != 0 {
                        if_get(rtable_loindex(m.m_pkthdr().ph_rtableid.get()))
                    } else {
                        if_get(rt.rt_ifidx.get())
                    };
                    // We aren't using rtisvalid() here because the UP/DOWN state machine is broken
                    // with some Ethernet drivers like em(4). As a result we might try to use an
                    // invalid cached route entry while an interface is being detached.
                    let Some(i) = ifp else {
                        ipstat_inc(IpstatCounters::IpsNoroute);
                        break 'bad Err(Errno::EHOSTUNREACH);
                    };
                    mtu = u64::from(rt.rt_mtu().load(Ordering::Relaxed));
                    if mtu == 0 {
                        mtu = u64::from(i.if_mtu.get());
                    }

                    if rt.rt_flags.get() & RTF_GATEWAY != 0 {
                        dst = satosin(rt.rt_gateway.get());
                    }

                    // Set the source IP address
                    if ip.ip_src.s_addr == INADDR_ANY
                        && let Some(ia) = ia
                    {
                        ip.ip_src = ia.ia_addr.get().sin_addr;
                        mtod_ip_store(m, &ip);
                    }
                }

                // IPSEC: ip_output_ipsec_lookup when ipsec_in_use or seclevel; not configured,
                // so there is never a tdb below.

                if in_multicast(ip.ip_dst.s_addr) || ip.ip_dst.s_addr == INADDR_BROADCAST {
                    m.m_flags().set(
                        m.m_flags().get()
                            | if ip.ip_dst.s_addr == INADDR_BROADCAST {
                                M_BCAST
                            } else {
                                M_MCAST
                            },
                    );

                    // IP destination address is multicast. Make sure "dst" still points to the
                    // address in "ro". (It may have been changed to point to a gateway address,
                    // above.)
                    dst = ro.ro_dstsa().cast();

                    // See if the caller provided any multicast options
                    ip.ip_ttl = match imo {
                        Some(imo) => imo.imo_ttl,
                        None => IP_DEFAULT_MULTICAST_TTL,
                    };
                    mtod_ip_store(m, &ip);

                    // if we don't know the outgoing ifp yet, we can't generate output
                    let Some(i) = ifp else {
                        ipstat_inc(IpstatCounters::IpsNoroute);
                        break 'bad Err(Errno::EHOSTUNREACH);
                    };

                    // Confirm that the outgoing interface supports multicast, but only if the
                    // packet actually is going out on that interface (i.e., no IPsec is applied).
                    if (m.m_flags().get() & M_MCAST != 0 && i.if_flags.get() & IFF_MULTICAST == 0)
                        || (m.m_flags().get() & M_BCAST != 0
                            && i.if_flags.get() & IFF_BROADCAST == 0)
                    {
                        ipstat_inc(IpstatCounters::IpsNoroute);
                        break 'bad Err(Errno::ENETUNREACH);
                    }

                    // If source address not specified yet, use address of outgoing interface.
                    if ip.ip_src.s_addr == INADDR_ANY
                        && let Some(ia) = in_ifp2ia(i)
                    {
                        ip.ip_src = ia.ia_addr.get().sin_addr;
                        mtod_ip_store(m, &ip);
                    }

                    if imo.is_none_or(|imo| imo.imo_loop != 0) && in_hasmulti(&ip.ip_dst, i) {
                        // If we belong to the destination multicast group on the outgoing
                        // interface, and the caller did not forbid loopback, loop back a copy.
                        // Can't defer TCP/UDP checksumming, do the computation now.
                        in_proto_cksum_out(m, None);
                        // SAFETY: `dst` is the route's destination `sockaddr_in`.
                        ip_mloopback(i, m, unsafe { &*dst });
                    }
                    // MROUTING: ip_mforward when ipmforwarding and ip_mrouter_active; not
                    // configured.

                    // Multicasts with a time-to-live of zero may be looped-back, above, but must
                    // not be transmitted on a network. Also, multicasts addressed to the loopback
                    // interface are not sent -- the above call to ip_mloopback() will loop back a
                    // copy if this host actually belongs to the destination group on the loopback
                    // interface.
                    if ip.ip_ttl == 0 || i.if_flags.get() & IFF_LOOPBACK != 0 {
                        break 'bad Ok(());
                    }
                }

                let Some(i) = ifp else {
                    break 'bad Err(Errno::EHOSTUNREACH);
                };

                // Look for broadcast address and verify user is allowed to send such a packet; if
                // the packet is going in an IPsec tunnel, skip this check.
                // SAFETY: `dst` is the route's destination or gateway `sockaddr_in`.
                let dst_addr = unsafe { (*dst).sin_addr.s_addr };
                if dst_addr == INADDR_BROADCAST
                    || ro
                        .ro_rt
                        .get()
                        .is_some_and(|rt| rt.rt_flags.get() & RTF_BROADCAST != 0)
                {
                    if i.if_flags.get() & IFF_BROADCAST == 0 {
                        break 'bad Err(Errno::EADDRNOTAVAIL);
                    }
                    if flags & IP_ALLOWBROADCAST == 0 {
                        break 'bad Err(Errno::EACCES);
                    }

                    // Don't allow broadcast messages to be fragmented
                    if u32::from(ntohs(ip.ip_len)) > i.if_mtu.get() {
                        break 'bad Err(Errno::EMSGSIZE);
                    }
                    m.m_flags().set(m.m_flags().get() | M_BCAST);
                } else {
                    m.m_flags().set(m.m_flags().get() & !M_BCAST);
                }

                // If we're doing Path MTU discovery, we need to set DF unless the route's MTU is
                // locked.
                if flags & IP_MTUDISC != 0
                    && ro
                        .ro_rt
                        .get()
                        .is_some_and(|rt| rt.rt_locks().get() & RTV_MTU == 0)
                {
                    ip.ip_off |= htons(IP_DF);
                    mtod_ip_store(m, &ip);
                }

                // IPSEC: ip_output_ipsec_send when a tdb applies; not configured.

                // Packet filter.
                let mut mp = Some(m);
                let dir = if flags & IP_FORWARDING != 0 {
                    PF_FWD
                } else {
                    PF_OUT
                };
                if pf_test(AF_INET, dir, i, &mut mp) != PF_PASS {
                    // `goto bad` frees `m` as pf_test left it: NULL when pf dropped it.
                    m_freem(mp);
                    break 'done Err(Errno::EACCES);
                }
                let Some(mm) = mp else {
                    break 'done Ok(());
                };
                m = mm;
                ip = mtod_ip(m);
                hlen = i32::from(ip.ip_hl()) << 2;
                let _ = hlen; // read by the IPsec code, which is not configured
                let pf = &m.m_pkthdr().pf;
                if pf.flags.get() & (PF_TAG_REROUTE | PF_TAG_GENERATED)
                    == (PF_TAG_REROUTE | PF_TAG_GENERATED)
                {
                    // Already rerun the route lookup, go on.
                    pf.flags
                        .set(pf.flags.get() & !(PF_TAG_GENERATED | PF_TAG_REROUTE));
                } else if pf.flags.get() & PF_TAG_REROUTE != 0 {
                    // Tag as generated to skip over pf_test on rerun.
                    pf.flags.set(pf.flags.get() | PF_TAG_GENERATED);
                    if ptr::eq(ro, &iproute) {
                        rtfree(ro.ro_rt.get());
                    }
                    // ro = NULL, then `ro = &iproute; ro->ro_rt = NULL` at reroute.
                    ro = &iproute;
                    ro.ro_rt.set(None);
                    if_put(ifp); // drop reference since target changed
                    ifp = None;
                    continue 'reroute;
                }

                // IPSEC: the IP_FORWARDING_IPSEC check; not configured.

                // If TSO or small enough for interface, can just send directly.
                let mut mp = Some(m);
                // SAFETY: `dst` is the route's destination or gateway `sockaddr_in`, readable for
                // the call.
                let error = unsafe {
                    if_output_tso(
                        i,
                        &mut mp,
                        sintosa(dst.cast_mut()),
                        ro.ro_rt.get(),
                        mtu as u32,
                    )
                };
                let Some(mm) = mp else {
                    break 'done error;
                };
                if error.is_err() {
                    break 'done error;
                }
                m = mm;

                // Too large for interface; fragment if possible. Must be able to put at least 8
                // bytes per fragment.
                let ip = mtod_ip(m);
                if ip.ip_off & htons(IP_DF) != 0 {
                    // IPSEC: ipsec_adjust_mtu when ip_mtudisc; not configured.
                    // pf changed routing table, use orig rtable for path MTU.
                    if ro.ro_tableid.get() != u64::from(orig_rtableid) {
                        rtfree(ro.ro_rt.get());
                        ro.ro_tableid.set(u64::from(orig_rtableid));
                        ro.ro_rt.set(icmp_mtudisc_clone(
                            ro.ro_dstsin().sin_addr,
                            ro.ro_tableid.get() as u32,
                            false,
                        ));
                    }

                    // This case can happen if the user changed the MTU of an interface after
                    // enabling IP on it. Because most netifs don't keep track of routes pointing
                    // to them, there is no way for one to update all its routes when the MTU is
                    // changed.
                    if let Some(rt) = ro.ro_rt.get()
                        && rtisvalid(Some(rt))
                        && rt.rt_flags.get() & RTF_HOST != 0
                        && rt.rt_locks().get() & RTV_MTU == 0
                    {
                        let rtmtu = rt.rt_mtu().load(Ordering::Relaxed);
                        if rtmtu > i.if_mtu.get() {
                            let _ = rt.rt_mtu().compare_exchange(
                                rtmtu,
                                i.if_mtu.get(),
                                Ordering::Relaxed,
                                Ordering::Relaxed,
                            );
                        }
                    }
                    ipstat_inc(IpstatCounters::IpsCantfrag);
                    break 'bad Err(Errno::EMSGSIZE);
                }

                if let Err(e) = ip_fragment(m, &ml, i, mtu) {
                    break 'done Err(e);
                }
                // SAFETY: as for `if_output_tso`.
                let sent = unsafe { if_output_ml(i, &ml, sintosa(dst.cast_mut()), ro.ro_rt.get()) };
                if let Err(e) = sent {
                    break 'done Err(e);
                }
                ipstat_inc(IpstatCounters::IpsFragmented);
                break 'done Ok(());
            }
        };
        // bad:
        m_freem(m);
        bad
    };
    // done:
    if ptr::eq(ro, &iproute) {
        rtfree(ro.ro_rt.get());
    }
    if_put(ifp);
    // IPSEC: tdb_unref(tdb); not configured.
    error
}

// IPSEC: ip_output_ipsec_lookup, ip_output_ipsec_pmtu_update and ip_output_ipsec_send; not
// configured.

/// `ip_fragment`: splits `m0` into fragments for `mtu` on `ml`, the first (trimmed) one
/// first; on failure every fragment is freed.
pub fn ip_fragment(m0: &'static Mbuf, ml: &MbufList, ifp: &Ifnet, mtu: u64) -> Result<(), Errno> {
    ml_init(ml);
    ml_enqueue(ml, m0);

    let mut ip = mtod_ip(m0);
    let hlen = i32::from(ip.ip_hl()) << 2;
    let tlen = m0.m_pkthdr().len.get();
    let mut len = ((mtu as i32) - hlen) & !7;

    let error = 'bad: {
        if len < 8 {
            break 'bad Errno::EMSGSIZE;
        }
        let firstlen = len;

        // If we are doing fragmentation, we can't defer TCP/UDP checksumming; compute the
        // checksum and clear the flag.
        in_proto_cksum_out(m0, None);

        // Loop through length of payload after first fragment, make new header and copy data
        // of each part and link onto chain.
        let mut off = hlen + firstlen;
        while off < tlen {
            let Some(m) = m_gethdr(M_DONTWAIT, MT_HEADER) else {
                break 'bad Errno::ENOBUFS;
            };
            ml_enqueue(ml, m);
            if let Err(e) = m_dup_pkthdr(m, m0, M_DONTWAIT) {
                break 'bad e;
            }
            m.m_data().set(
                m.m_data()
                    .get()
                    .wrapping_add(MAX_LINKHDR.load(Ordering::Relaxed) as usize),
            );
            let mut mhip = ip;
            let mhlen = if hlen > size_of::<Ip>() as i32 {
                // The options of the fragment, after its fixed header.
                // SAFETY: `m0`'s header with its options is contiguous; the new mbuf has
                // room for a header and 40 bytes of options after `max_linkhdr`.
                let n = unsafe { ip_optcopy(mtod::<u8>(m0), hlen as usize, mtod::<u8>(m)) };
                let mhlen = n as i32 + size_of::<Ip>() as i32;
                mhip.set_ip_hl((mhlen >> 2) as u8);
                mhlen
            } else {
                size_of::<Ip>() as i32
            };
            m.m_len().set(mhlen as u32);

            let mut moff = (((off - hlen) >> 3) as u16) + (ntohs(ip.ip_off) & !IP_MF);
            if ip.ip_off & htons(IP_MF) != 0 {
                moff |= IP_MF;
            }
            if off + len >= tlen {
                len = tlen - off;
            } else {
                moff |= IP_MF;
            }
            mhip.ip_off = htons(moff);

            m.m_pkthdr().len.set(mhlen + len);
            mhip.ip_len = htons(m.m_pkthdr().len.get() as u16);
            mtod_ip_store(m, &mhip);
            let Some(n) = m_copym(m0, off, len, M_NOWAIT) else {
                break 'bad Errno::ENOBUFS;
            };
            m.m_next().set(Some(n));

            in_hdr_cksum_out(m, Some(ifp));
            off += len;
        }

        // Update first fragment by trimming what's been copied out and updating header, then
        // send each fragment (in order).
        if hlen + firstlen < tlen {
            m_adj(m0, hlen + firstlen - tlen);
            ip.ip_off |= htons(IP_MF);
        }
        ip.ip_len = htons(m0.m_pkthdr().len.get() as u16);
        mtod_ip_store(m0, &ip);

        in_hdr_cksum_out(m0, Some(ifp));

        ipstat_add(IpstatCounters::IpsOfragments, u64::from(ml_len(ml)));
        return Ok(());
    };
    // bad:
    ipstat_inc(IpstatCounters::IpsOdropped);
    let _ = ml_purge(ml);
    Err(error)
}

/// `ip_insertoptions`: inserts IP options `opt` (a `struct ipoption`) into a preformed packet.
/// Adjusts the IP destination as required for IP source routing, as indicated by a non-zero
/// `in_addr` at the start of the options; `phlen` gets the new header length.
pub fn ip_insertoptions(m: &'static Mbuf, opt: &Mbuf, phlen: &mut i32) -> &'static Mbuf {
    let mut m = m;
    // SAFETY: an options mbuf holds a `struct ipoption` of `m_len` bytes (its list is
    // `m_len - sizeof(ipopt_dst)` bytes long).
    let p: Ipoption = unsafe { ptr::read_unaligned(mtod::<Ipoption>(opt)) };
    let mut ip = mtod_ip(m);

    let optlen = opt.m_len().get() as usize - size_of::<crate::netinet::in_::InAddr>();
    if optlen + usize::from(ntohs(ip.ip_len)) > IP_MAXPACKET {
        return m; // XXX should fail
    }

    // check if options will fit to IP header
    if optlen + size_of::<Ip>() > (0x0f << 2) {
        *phlen = size_of::<Ip>() as i32;
        return m;
    }

    if p.ipopt_dst.s_addr != 0 {
        ip.ip_dst = p.ipopt_dst;
    }
    let pktdat_room = (m.m_data().get() as usize).wrapping_sub(m.m_pktdat() as usize);
    if m.m_flags().get() & M_EXT != 0 || pktdat_room < optlen {
        let Some(n) = m_gethdr(M_DONTWAIT, MT_HEADER) else {
            return m;
        };
        m_move_hdr(n, m);
        n.m_pkthdr().len.set(n.m_pkthdr().len.get() + optlen as i32);
        m.m_len().set(m.m_len().get() - size_of::<Ip>() as u32);
        m.m_data()
            .set(m.m_data().get().wrapping_add(size_of::<Ip>()));
        n.m_next().set(Some(m));
        m = n;
        m.m_len().set((optlen + size_of::<Ip>()) as u32);
        m.m_data().set(
            m.m_data()
                .get()
                .wrapping_add(MAX_LINKHDR.load(Ordering::Relaxed) as usize),
        );
    } else {
        m.m_data().set(m.m_data().get().wrapping_sub(optlen));
        m.m_len().set(m.m_len().get() + optlen as u32);
        m.m_pkthdr().len.set(m.m_pkthdr().len.get() + optlen as i32);
    }
    // The header moves to the new start (the C's memcpy/memmove of the old header).
    mtod_ip_store(m, &ip);
    // SAFETY: the first mbuf now holds the header followed by `optlen` bytes of room; the
    // options are `optlen` bytes of `p`'s list.
    unsafe {
        ptr::copy_nonoverlapping(
            p.ipopt_list.as_ptr().cast::<u8>(),
            mtod::<u8>(m).add(size_of::<Ip>()),
            optlen,
        )
    };
    *phlen = (size_of::<Ip>() + optlen) as i32;
    ip.ip_len = htons(ntohs(ip.ip_len) + optlen as u16);
    mtod_ip_store(m, &ip);
    m
}

/// `ip_optcopy`: copies the options of the header at `ip` (`hlen` bytes long) to after the
/// fixed header at `jp`, omitting those not copied during fragmentation, and pads them with
/// `IPOPT_EOL` to a multiple of 4. Returns the length of the copy.
///
/// # Safety
///
/// `ip` points at `hlen` readable bytes; `jp` has room for a fixed header and `hlen` bytes
/// after it, and does not overlap `ip`.
pub unsafe fn ip_optcopy(ip: *const u8, hlen: usize, jp: *mut u8) -> usize {
    let hdr = size_of::<Ip>();
    // SAFETY: the caller's contract.
    let cp = unsafe { core::slice::from_raw_parts(ip.add(hdr), hlen - hdr) };
    // SAFETY: the caller's contract.
    let dp = unsafe { core::slice::from_raw_parts_mut(jp.add(hdr), hlen - hdr) };
    let mut d = 0;
    let mut c = 0;
    let mut cnt = (hlen - hdr) as i32;
    while cnt > 0 {
        let opt = cp[c];
        if opt == IPOPT_EOL {
            break;
        }
        if opt == IPOPT_NOP {
            // Preserve for IP mcast tunnel's LSRR alignment.
            dp[d] = IPOPT_NOP;
            d += 1;
            cnt -= 1;
            c += 1;
            continue;
        }
        #[cfg(feature = "diagnostic")]
        if cnt < (IPOPT_OLEN + 1) as i32 {
            crate::kern::subr_prf::panic(format_args!(
                "malformed IPv4 option passed to ip_optcopy"
            ));
        }
        let mut optlen = i32::from(cp[c + IPOPT_OLEN]);
        #[cfg(feature = "diagnostic")]
        if optlen < (IPOPT_OLEN + 1) as i32 || optlen > cnt {
            crate::kern::subr_prf::panic(format_args!(
                "malformed IPv4 option passed to ip_optcopy"
            ));
        }
        // bogus lengths should have been caught by ip_dooptions
        if optlen > cnt {
            optlen = cnt;
        }
        if optlen <= 0 {
            break;
        }
        let ol = optlen as usize;
        if ipopt_copied(opt) != 0 {
            dp[d..d + ol].copy_from_slice(&cp[c..c + ol]);
            d += ol;
        }
        cnt -= optlen;
        c += ol;
    }
    let mut optlen = d;
    while optlen & 0x3 != 0 {
        dp[d] = IPOPT_EOL;
        d += 1;
        optlen += 1;
    }
    optlen
}

// ip_ctloutput, ip_pcbopts, ip_multicast_if, ip_setmoptions, ip_getmoptions: the socket
// option side, which needs the socket layer (see the module's deviations).

/// The `int` at the start of an option mbuf (read unaligned: mbuf data need not be aligned).
fn mtod_int(m: &Mbuf) -> i32 {
    // SAFETY: the callers checked `m_len` is at least `sizeof(int)`.
    unsafe { mtod::<i32>(m).read_unaligned() }
}

/// Stores `v` as an `int` option of `m`.
fn set_mtod_int(m: &Mbuf, v: i32) {
    m.m_len().set(size_of::<i32>() as u32);
    // SAFETY: an option mbuf holds `MLEN` bytes, more than an `int`.
    unsafe { mtod::<i32>(m).write_unaligned(v) };
}

/// The first `m_len` bytes of an option mbuf.
fn mtod_bytes(m: &Mbuf) -> &[u8] {
    // SAFETY: the first mbuf holds `m_len` bytes at `m_data` (an option mbuf is one mbuf),
    // read while the caller borrows it.
    unsafe { core::slice::from_raw_parts(mtod::<u8>(m), m.m_len().get() as usize) }
}

/// `ip_ctloutput`: IP socket option processing.
pub fn ip_ctloutput(
    op: i32,
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let Some(inp) = sotoinpcb(so) else {
        panic(format_args!("ip_ctloutput: socket {:p} without inpcb", so));
    };
    let Some(p) = curproc() else {
        panic(format_args!("ip_ctloutput: no curproc")); // XXX
    };
    let mut error = Ok(());

    if level != IPPROTO_IP {
        return Err(Errno::EINVAL);
    }

    let rtableid = p.process().ps_rtableid.load(Ordering::Relaxed);

    match op {
        PRCO_SETOPT => match optname {
            IP_OPTIONS => return ip_pcbopts(&inp.inp_options, m),

            IP_TOS | IP_TTL | IP_MINTTL | IP_RECVOPTS | IP_RECVRETOPTS | IP_RECVDSTADDR
            | IP_RECVIF | IP_RECVTTL | IP_RECVDSTPORT | IP_RECVRTABLE | IP_IPSECFLOWINFO => {
                let Some(m) = m.filter(|m| m.m_len().get() as usize == size_of::<i32>()) else {
                    return Err(Errno::EINVAL);
                };
                let optval = mtod_int(m);
                let optset = |bit: i32| {
                    if optval != 0 {
                        inp.set_flags(bit);
                    } else {
                        inp.clear_flags(bit);
                    }
                };
                let mut ip = inp.inp_ip.get();
                match optname {
                    IP_TOS => ip.ip_tos = optval as u8,

                    IP_TTL => {
                        if optval > 0 && optval <= i32::from(MAXTTL) {
                            ip.ip_ttl = optval as u8;
                        } else if optval == -1 {
                            ip.ip_ttl = IP_DEFTTL.load(Ordering::Relaxed) as u8;
                        } else {
                            error = Err(Errno::EINVAL);
                        }
                    }

                    IP_MINTTL => {
                        if optval >= 0 && optval <= i32::from(MAXTTL) {
                            inp.inp_ip_minttl.set(optval as u8);
                        } else {
                            error = Err(Errno::EINVAL);
                        }
                    }

                    IP_RECVOPTS => optset(INP_RECVOPTS),
                    IP_RECVRETOPTS => optset(INP_RECVRETOPTS),
                    IP_RECVDSTADDR => optset(INP_RECVDSTADDR),
                    IP_RECVIF => optset(INP_RECVIF),
                    IP_RECVTTL => optset(INP_RECVTTL),
                    IP_RECVDSTPORT => optset(INP_RECVDSTPORT),
                    IP_RECVRTABLE => optset(INP_RECVRTABLE),
                    _ => optset(INP_IPSECFLOWINFO), // IP_IPSECFLOWINFO
                }
                inp.inp_ip.set(ip);
            }

            IP_MULTICAST_IF | IP_MULTICAST_TTL | IP_MULTICAST_LOOP | IP_ADD_MEMBERSHIP
            | IP_DROP_MEMBERSHIP => {
                error = ip_setmoptions(optname, &inp.inp_moptions, m, inp.inp_rtableid.get());
            }

            IP_PORTRANGE => {
                let Some(m) = m.filter(|m| m.m_len().get() as usize == size_of::<i32>()) else {
                    return Err(Errno::EINVAL);
                };
                match mtod_int(m) {
                    IP_PORTRANGE_DEFAULT => {
                        inp.clear_flags(INP_LOWPORT);
                        inp.clear_flags(INP_HIGHPORT);
                    }

                    IP_PORTRANGE_HIGH => {
                        inp.clear_flags(INP_LOWPORT);
                        inp.set_flags(INP_HIGHPORT);
                    }

                    IP_PORTRANGE_LOW => {
                        inp.clear_flags(INP_HIGHPORT);
                        inp.set_flags(INP_LOWPORT);
                    }

                    _ => error = Err(Errno::EINVAL),
                }
            }
            // !IPSEC: the security levels cannot be set.
            IP_AUTH_LEVEL | IP_ESP_TRANS_LEVEL | IP_ESP_NETWORK_LEVEL | IP_IPCOMP_LEVEL => {
                error = Err(Errno::EOPNOTSUPP);
            }

            IP_IPSEC_LOCAL_ID | IP_IPSEC_REMOTE_ID => error = Err(Errno::EOPNOTSUPP),
            SO_RTABLE => {
                let Some(m) = m.filter(|m| m.m_len().get() as usize >= size_of::<u32>()) else {
                    return Err(Errno::EINVAL);
                };
                let rtid = mtod_int(m) as u32;
                if inp.inp_rtableid.get() != rtid {
                    // needs privileges to switch when already set
                    if rtableid != rtid && rtableid != 0 {
                        suser(p)?;
                    }
                    error = in_pcbset_rtableid(inp, rtid);
                }
            }
            IP_PIPEX => match m.filter(|m| m.m_len().get() as usize == size_of::<i32>()) {
                Some(m) => inp.inp_pipex.set(mtod_int(m)),
                None => error = Err(Errno::EINVAL),
            },

            _ => error = Err(Errno::ENOPROTOOPT),
        },

        PRCO_GETOPT => {
            // sogetopt always hands over an mbuf to fill.
            let Some(m) = m else {
                return Err(Errno::EINVAL);
            };
            match optname {
                IP_OPTIONS | IP_RETOPTS => {
                    if let Some(opts) = inp.inp_options.get() {
                        let len = opts.m_len().get();
                        m.m_len().set(len);
                        // SAFETY: both mbufs hold `len` bytes (`ip_pcbopts` made the options
                        // at most a `struct ipoption`, smaller than `MLEN`); distinct mbufs.
                        unsafe {
                            ptr::copy_nonoverlapping(mtod::<u8>(opts), mtod::<u8>(m), len as usize)
                        };
                    } else {
                        m.m_len().set(0);
                    }
                }

                IP_TOS | IP_TTL | IP_MINTTL | IP_RECVOPTS | IP_RECVRETOPTS | IP_RECVDSTADDR
                | IP_RECVIF | IP_RECVTTL | IP_RECVDSTPORT | IP_RECVRTABLE | IP_IPSECFLOWINFO
                | IP_IPDEFTTL => {
                    let optbit = |bit: i32| i32::from(inp.has_flags(bit));
                    let optval = match optname {
                        IP_TOS => i32::from(inp.inp_ip.get().ip_tos),
                        IP_TTL => i32::from(inp.inp_ip.get().ip_ttl),
                        IP_MINTTL => i32::from(inp.inp_ip_minttl.get()),
                        IP_IPDEFTTL => IP_DEFTTL.load(Ordering::Relaxed),
                        IP_RECVOPTS => optbit(INP_RECVOPTS),
                        IP_RECVRETOPTS => optbit(INP_RECVRETOPTS),
                        IP_RECVDSTADDR => optbit(INP_RECVDSTADDR),
                        IP_RECVIF => optbit(INP_RECVIF),
                        IP_RECVTTL => optbit(INP_RECVTTL),
                        IP_RECVDSTPORT => optbit(INP_RECVDSTPORT),
                        IP_RECVRTABLE => optbit(INP_RECVRTABLE),
                        _ => optbit(INP_IPSECFLOWINFO), // IP_IPSECFLOWINFO
                    };
                    set_mtod_int(m, optval);
                }

                IP_MULTICAST_IF | IP_MULTICAST_TTL | IP_MULTICAST_LOOP | IP_ADD_MEMBERSHIP
                | IP_DROP_MEMBERSHIP => error = ip_getmoptions(optname, inp.moptions(), m),

                IP_PORTRANGE => {
                    let optval = if inp.has_flags(INP_HIGHPORT) {
                        IP_PORTRANGE_HIGH
                    } else if inp.has_flags(INP_LOWPORT) {
                        IP_PORTRANGE_LOW
                    } else {
                        0
                    };
                    set_mtod_int(m, optval);
                }

                // !IPSEC: every level reads as IPSEC_LEVEL_NONE.
                IP_AUTH_LEVEL | IP_ESP_TRANS_LEVEL | IP_ESP_NETWORK_LEVEL | IP_IPCOMP_LEVEL => {
                    set_mtod_int(m, IPSEC_LEVEL_NONE);
                }
                IP_IPSEC_LOCAL_ID | IP_IPSEC_REMOTE_ID => error = Err(Errno::EOPNOTSUPP),
                SO_RTABLE => set_mtod_int(m, inp.inp_rtableid.get() as i32),
                IP_PIPEX => set_mtod_int(m, inp.inp_pipex.get()),
                _ => error = Err(Errno::ENOPROTOOPT),
            }
        }

        _ => {}
    }
    error
}

/// `ip_pcbopts`: sets up IP options in the pcb for insertion in output packets: stored in an
/// mbuf in `pcbopt`, with a pseudo-option holding the first hop if source routed.
pub fn ip_pcbopts(pcbopt: &Cell<Option<&'static Mbuf>>, m: Option<&Mbuf>) -> Result<(), Errno> {
    // turn off any old options
    m_freem(pcbopt.take());
    let Some(m) = m.filter(|m| m.m_len().get() != 0) else {
        // Only turning off any previous options.
        return Ok(());
    };

    let mlen = m.m_len().get() as usize;
    if !mlen.is_multiple_of(size_of::<i32>()) || mlen > MAX_IPOPTLEN + size_of::<InAddr>() {
        return Err(Errno::EINVAL);
    }

    // Don't sleep because NET_LOCK() is hold.
    let Some(n) = m_get(M_NOWAIT, MT_SOOPTS) else {
        return Err(Errno::ENOBUFS);
    };
    // The struct ipoption, zeroed (0 = IPOPT_EOL, needed for padding): the first-hop address
    // then the options. The list is as long as the input can fill (the C writes past
    // ipopt_list into the mbuf before the final length check).
    let mut p = [0u8; size_of::<InAddr>() + MAX_IPOPTLEN + size_of::<InAddr>()];
    let (dst, list) = p.split_at_mut(size_of::<InAddr>());
    let mut nlen = size_of::<InAddr>();

    let cp_all = mtod_bytes(m);
    let mut cp = 0usize;
    let mut off = 0usize;
    let mut cnt = mlen as i32;

    let bad = |n: &Mbuf| {
        m_freem(n);
        Err(Errno::EINVAL)
    };

    while cnt > 0 {
        let opt = cp_all[cp + IPOPT_OPTVAL];

        let mut optlen = if opt == IPOPT_NOP || opt == IPOPT_EOL {
            1
        } else {
            if cnt < (IPOPT_OLEN + 1) as i32 {
                return bad(n);
            }
            let optlen = i32::from(cp_all[cp + IPOPT_OLEN]);
            if optlen < (IPOPT_OLEN + 1) as i32 || optlen > cnt {
                return bad(n);
            }
            optlen
        };
        match opt {
            IPOPT_LSRR | IPOPT_SSRR => {
                // user process specifies route as:
                //	->A->B->C->D
                // D must be our final destination (but we can't check that since we may not
                // have connected yet). A is first hop destination, which doesn't appear in
                // actual IP option, but is stored before the options.
                if optlen < i32::from(IPOPT_MINOFF) - 1 + size_of::<InAddr>() as i32 {
                    return bad(n);
                }

                // Optlen is smaller because first address is popped. Cnt and cp will be
                // adjusted a bit later to reflect this.
                optlen -= size_of::<InAddr>() as i32;
                list[off + IPOPT_OPTVAL] = opt;
                list[off + IPOPT_OLEN] = optlen as u8;

                // Move first hop before start of options.
                dst.copy_from_slice(&cp_all[cp + IPOPT_OFFSET..cp + IPOPT_OFFSET + 4]);
                cp += size_of::<InAddr>();
                cnt -= size_of::<InAddr>() as i32;
                // Then copy rest of options
                let rest = optlen as usize - IPOPT_OFFSET;
                list[off + IPOPT_OFFSET..off + IPOPT_OFFSET + rest]
                    .copy_from_slice(&cp_all[cp + IPOPT_OFFSET..cp + IPOPT_OFFSET + rest]);
            }
            _ => {
                let l = optlen as usize;
                list[off..off + l].copy_from_slice(&cp_all[cp..cp + l]);
            }
        }
        off += optlen as usize;
        cp += optlen as usize;
        cnt -= optlen;

        if opt == IPOPT_EOL {
            break;
        }
    }
    // pad options to next word, since p was zeroed just adjust off
    off = (off + size_of::<i32>() - 1) & !(size_of::<i32>() - 1);
    nlen += off;
    if nlen > size_of::<Ipoption>() {
        return bad(n);
    }
    n.m_len().set(nlen as u32);
    // SAFETY: `n` is a fresh mbuf of `MLEN` bytes, more than a `struct ipoption`.
    unsafe { ptr::copy_nonoverlapping(p.as_ptr(), mtod::<u8>(n), size_of::<Ipoption>()) };

    pcbopt.set(Some(n));
    Ok(())
}

/// `ip_multicast_if`: the interface for the request in `mreq`: its index if given, else the
/// interface of the route to the group (no local address) or of the local address.
pub fn ip_multicast_if(mreq: &IpMreqn, rtableid: u32, ifidx: &mut u32) -> Result<(), Errno> {
    // In case userland provides the imr_ifindex use this as interface. If no interface
    // address was provided, use the interface of the route to the given multicast address.
    if mreq.imr_ifindex != 0 {
        *ifidx = mreq.imr_ifindex as u32;
    } else {
        let (addr, flags) = if mreq.imr_address.s_addr == INADDR_ANY {
            (mreq.imr_multiaddr, RT_RESOLVE)
        } else {
            (mreq.imr_address, 0)
        };
        let mut sin = SockaddrIn {
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_family: AF_INET,
            sin_addr: addr,
            ..SockaddrIn::default()
        };
        // SAFETY: a local `sockaddr_in`, read for the call.
        let rt = unsafe { rtalloc(sintosa(&mut sin), flags, rtableid) };
        if !rtisvalid(rt) || (flags == 0 && rt.is_some_and(|rt| rt.rt_flags.get() & RTF_LOCAL == 0))
        {
            rtfree(rt);
            return Err(Errno::EADDRNOTAVAIL);
        }
        *ifidx = rt.map_or(0, |rt| rt.rt_ifidx.get());
        rtfree(rt);
    }

    Ok(())
}

/// The `ip_mreqn` an `ip_mreq` or `ip_mreqn` option mbuf holds (a short one zero-extended).
fn mtod_mreqn(m: &Mbuf) -> IpMreqn {
    let mut mreqn = IpMreqn::default();
    let len = (m.m_len().get() as usize).min(size_of::<IpMreqn>());
    // SAFETY: the callers checked `m_len` is the size of an `ip_mreq` or an `ip_mreqn`; the
    // local is `#[repr(C)]` plain data.
    unsafe { ptr::copy_nonoverlapping(mtod::<u8>(m), ptr::from_mut(&mut mreqn).cast::<u8>(), len) };
    mreqn
}

/// Membership slot `i` of `imo` (of `imo_max_memberships` at `imo_membership`).
fn imo_slot(imo: &IpMoptions, i: usize) -> *mut Option<&'static InMulti> {
    imo.imo_membership.wrapping_add(i)
}

/// The group of membership `i` of `imo`, one of the first `imo_num_memberships` (in use).
fn imo_member(imo: &IpMoptions, i: usize) -> Option<&'static InMulti> {
    kassert!(i < usize::from(imo.imo_num_memberships));
    // SAFETY: a slot in use of the vector `ip_setmoptions` allocated.
    unsafe { *imo_slot(imo, i) }
}

/// `ip_setmoptions`: sets the IP multicast options in response to user setsockopt().
pub fn ip_setmoptions(
    optname: i32,
    imop: &Cell<Option<NonNull<IpMoptions>>>,
    m: Option<&Mbuf>,
    rtableid: u32,
) -> Result<(), Errno> {
    let mut error = Ok(());

    let imo_ptr = match imop.get() {
        Some(imo) => imo,
        None => {
            // No multicast option buffer attached to the pcb; allocate one and initialize to
            // default values.
            let (Some(imo), Some(immp)) = (
                malloc(size_of::<IpMoptions>(), M_IPMOPTS, M_WAITOK | M_ZERO),
                mallocarray(
                    usize::from(IP_MIN_MEMBERSHIPS),
                    size_of::<Option<&InMulti>>(),
                    M_IPMOPTS,
                    M_WAITOK | M_ZERO,
                ),
            ) else {
                panic(format_args!("ip_setmoptions: malloc(M_WAITOK)"));
            };
            let imo = imo.cast::<IpMoptions>();
            // SAFETY: a fresh allocation of the structure's size, written once.
            unsafe {
                imo.as_ptr().write(IpMoptions {
                    imo_membership: immp.cast::<Option<&'static InMulti>>().as_ptr(),
                    imo_ifidx: 0,
                    imo_ttl: IP_DEFAULT_MULTICAST_TTL,
                    imo_loop: IP_DEFAULT_MULTICAST_LOOP,
                    imo_num_memberships: 0,
                    imo_max_memberships: IP_MIN_MEMBERSHIPS,
                })
            };
            imop.set(Some(imo));
            imo
        }
    };
    // SAFETY: the options are the control block's own allocation, changed here and in
    // ip_freemoptions only, under the exclusive net lock the socket option calls hold.
    let imo = unsafe { &mut *imo_ptr.as_ptr() };

    'out: {
        match optname {
            IP_MULTICAST_IF => {
                // Select the interface for outgoing multicast packets.
                let Some(m) = m else {
                    error = Err(Errno::EINVAL);
                    break 'out;
                };
                let len = m.m_len().get() as usize;
                let addr = if len == size_of::<InAddr>() {
                    // SAFETY: the mbuf holds an `in_addr`.
                    unsafe { mtod::<InAddr>(m).read_unaligned() }
                } else if len == size_of::<IpMreq>() || len == size_of::<IpMreqn>() {
                    let mreqn = mtod_mreqn(m);

                    // If an interface index is given use this index to set the imo_ifidx but
                    // check first that the interface actually exists. In the other case just
                    // set the addr to the imr_address and fall through to the regular code.
                    if mreqn.imr_ifindex != 0 {
                        let ifp = if_get(mreqn.imr_ifindex as u32);
                        match ifp {
                            Some(ifp) if ifp.if_rdomain.get() == rtable_l2(rtableid) => {
                                imo.imo_ifidx = ifp.if_index.get() as u16;
                            }
                            _ => error = Err(Errno::EADDRNOTAVAIL),
                        }
                        if_put(ifp);
                        break 'out;
                    }
                    mreqn.imr_address
                } else {
                    error = Err(Errno::EINVAL);
                    break 'out;
                };
                // INADDR_ANY is used to remove a previous selection. When no interface is
                // selected, a default one is chosen every time a multicast packet is sent.
                if addr.s_addr == INADDR_ANY {
                    imo.imo_ifidx = 0;
                    break 'out;
                }
                // The selected interface is identified by its local IP address. Find the
                // interface and confirm that it supports multicasting.
                let mut sin = SockaddrIn {
                    sin_len: size_of::<SockaddrIn>() as u8,
                    sin_family: AF_INET,
                    sin_addr: addr,
                    ..SockaddrIn::default()
                };
                // SAFETY: a local `sockaddr_in`, read for the call.
                let ifa = unsafe { ifa_ifwithaddr(sintosa(&mut sin), rtableid) };
                let ifp = ifa.map(ifatoia).and_then(|ia| ia.ia_ifp().get());
                match ifp {
                    Some(ifp) if ifp.if_flags.get() & IFF_MULTICAST != 0 => {
                        imo.imo_ifidx = ifp.if_index.get() as u16;
                    }
                    _ => error = Err(Errno::EADDRNOTAVAIL),
                }
            }

            IP_MULTICAST_TTL => {
                // Set the IP time-to-live for outgoing multicast packets.
                match m.filter(|m| m.m_len().get() == 1) {
                    Some(m) => imo.imo_ttl = mtod_bytes(m)[0],
                    None => error = Err(Errno::EINVAL),
                }
            }

            IP_MULTICAST_LOOP => {
                // Set the loopback flag for outgoing multicast packets. Must be zero or one.
                match m.filter(|m| m.m_len().get() == 1).map(|m| mtod_bytes(m)[0]) {
                    Some(lp) if lp <= 1 => imo.imo_loop = lp,
                    _ => error = Err(Errno::EINVAL),
                }
            }

            IP_ADD_MEMBERSHIP => {
                // Add a multicast group membership. Group must be a valid IP multicast
                // address.
                let Some(m) = m.filter(|m| {
                    let len = m.m_len().get() as usize;
                    len == size_of::<IpMreq>() || len == size_of::<IpMreqn>()
                }) else {
                    error = Err(Errno::EINVAL);
                    break 'out;
                };
                let mreqn = mtod_mreqn(m);
                if !in_multicast(mreqn.imr_multiaddr.s_addr) {
                    error = Err(Errno::EINVAL);
                    break 'out;
                }

                let mut ifidx = 0;
                if let Err(e) = ip_multicast_if(&mreqn, rtableid, &mut ifidx) {
                    error = Err(e);
                    break 'out;
                }

                // See if we found an interface, and confirm that it supports multicast.
                let ifp = if_get(ifidx);
                let Some(ifp) = ifp.filter(|ifp| {
                    ifp.if_rdomain.get() == rtable_l2(rtableid)
                        && ifp.if_flags.get() & IFF_MULTICAST != 0
                }) else {
                    error = Err(Errno::EADDRNOTAVAIL);
                    if_put(ifp);
                    break 'out;
                };

                // See if the membership already exists or if all the membership slots are
                // full.
                let num = usize::from(imo.imo_num_memberships);
                let i = (0..num)
                    .find(|&i| {
                        imo_member(imo, i).is_some_and(|inm| {
                            inm.inm_ifidx().get() == ifidx
                                && inm.inm_addr().s_addr == mreqn.imr_multiaddr.s_addr
                        })
                    })
                    .unwrap_or(num);
                if i < num {
                    error = Err(Errno::EADDRINUSE);
                    if_put(Some(ifp));
                    break 'out;
                }
                if imo.imo_num_memberships == imo.imo_max_memberships {
                    // Resize the vector to next power-of-two minus 1. If the size would exceed
                    // the maximum then we know we've really run out of entries. Otherwise, we
                    // reallocate the vector.
                    let mut nmships = None;
                    let omships = imo.imo_membership;
                    let oldmax = usize::from(imo.imo_max_memberships);
                    let newmax = ((oldmax + 1) * 2) - 1;
                    if newmax <= usize::from(IP_MAX_MEMBERSHIPS) {
                        nmships = mallocarray(
                            newmax,
                            size_of::<Option<&InMulti>>(),
                            M_IPMOPTS,
                            M_NOWAIT | M_ZERO,
                        );
                        if let Some(n) = nmships {
                            let n = n.cast::<Option<&'static InMulti>>().as_ptr();
                            // SAFETY: the old vector has `oldmax` slots, the new one more;
                            // distinct allocations.
                            unsafe { ptr::copy_nonoverlapping(omships, n, oldmax) };
                            if let Some(o) = NonNull::new(omships) {
                                free(o.cast(), M_IPMOPTS, size_of::<Option<&InMulti>>() * oldmax);
                            }
                            imo.imo_membership = n;
                            imo.imo_max_memberships = newmax as u16;
                        }
                    }
                    if nmships.is_none() {
                        error = Err(Errno::ENOBUFS);
                        if_put(Some(ifp));
                        break 'out;
                    }
                }
                // Everything looks good; add a new record to the multicast address list for
                // the given interface.
                let Some(inm) = in_addmulti(&mreqn.imr_multiaddr, ifp) else {
                    error = Err(Errno::ENOBUFS);
                    if_put(Some(ifp));
                    break 'out;
                };
                // SAFETY: `i` (== imo_num_memberships) is below imo_max_memberships: a slot
                // of the vector.
                unsafe { *imo_slot(imo, i) = Some(inm) };
                imo.imo_num_memberships += 1;
                if_put(Some(ifp));
            }

            IP_DROP_MEMBERSHIP => {
                // Drop a multicast group membership. Group must be a valid IP multicast
                // address.
                let Some(m) = m.filter(|m| {
                    let len = m.m_len().get() as usize;
                    len == size_of::<IpMreq>() || len == size_of::<IpMreqn>()
                }) else {
                    error = Err(Errno::EINVAL);
                    break 'out;
                };
                let mreqn = mtod_mreqn(m);
                if !in_multicast(mreqn.imr_multiaddr.s_addr) {
                    error = Err(Errno::EINVAL);
                    break 'out;
                }

                // If an interface address was specified, get a pointer to its ifnet
                // structure.
                let mut ifidx = 0;
                if let Err(e) = ip_multicast_if(&mreqn, rtableid, &mut ifidx) {
                    error = Err(e);
                    break 'out;
                }

                // Find the membership in the membership array.
                let num = usize::from(imo.imo_num_memberships);
                let Some(i) = (0..num).find(|&i| {
                    imo_member(imo, i).is_some_and(|inm| {
                        (ifidx == 0 || inm.inm_ifidx().get() == ifidx)
                            && inm.inm_addr().s_addr == mreqn.imr_multiaddr.s_addr
                    })
                }) else {
                    error = Err(Errno::EADDRNOTAVAIL);
                    break 'out;
                };
                // Give up the multicast address record to which the membership points.
                if let Some(inm) = imo_member(imo, i) {
                    in_delmulti(inm);
                }
                // Remove the gap in the membership array.
                for j in i + 1..num {
                    // SAFETY: `j - 1` and `j` are slots in use.
                    unsafe { *imo_slot(imo, j - 1) = *imo_slot(imo, j) };
                }
                imo.imo_num_memberships -= 1;
            }

            _ => error = Err(Errno::EOPNOTSUPP),
        }
    }

    // If all options have default values, no need to keep the data.
    if imo.imo_ifidx == 0
        && imo.imo_ttl == IP_DEFAULT_MULTICAST_TTL
        && imo.imo_loop == IP_DEFAULT_MULTICAST_LOOP
        && imo.imo_num_memberships == 0
    {
        if let Some(mships) = NonNull::new(imo.imo_membership) {
            free(
                mships.cast(),
                M_IPMOPTS,
                usize::from(imo.imo_max_memberships) * size_of::<Option<&InMulti>>(),
            );
        }
        free(imo_ptr.cast(), M_IPMOPTS, size_of::<IpMoptions>());
        imop.set(None);
    }

    error
}

/// `ip_getmoptions`: returns the IP multicast options in response to user getsockopt().
pub fn ip_getmoptions(optname: i32, imo: Option<&IpMoptions>, m: &Mbuf) -> Result<(), Errno> {
    match optname {
        IP_MULTICAST_IF => {
            m.m_len().set(size_of::<InAddr>() as u32);
            let ifp = imo.and_then(|imo| if_get(u32::from(imo.imo_ifidx)));
            let addr = match ifp {
                None => INADDR_ANY,
                Some(ifp) => {
                    let a =
                        in_ifp2ia(ifp).map_or(INADDR_ANY, |ia| ia.ia_addr.get().sin_addr.s_addr);
                    if_put(Some(ifp));
                    a
                }
            };
            // SAFETY: an option mbuf holds `MLEN` bytes, more than an `in_addr`.
            unsafe { mtod::<InAddr>(m).write_unaligned(InAddr { s_addr: addr }) };
            Ok(())
        }

        IP_MULTICAST_TTL => {
            m.m_len().set(1);
            let ttl = imo.map_or(IP_DEFAULT_MULTICAST_TTL, |imo| imo.imo_ttl);
            // SAFETY: an option mbuf holds at least one byte.
            unsafe { mtod::<u8>(m).write(ttl) };
            Ok(())
        }

        IP_MULTICAST_LOOP => {
            m.m_len().set(1);
            let lp = imo.map_or(IP_DEFAULT_MULTICAST_LOOP, |imo| imo.imo_loop);
            // SAFETY: as above.
            unsafe { mtod::<u8>(m).write(lp) };
            Ok(())
        }

        _ => Err(Errno::EOPNOTSUPP),
    }
}

/// `ip_freemoptions`: frees a set of multicast options and leaves its groups.
///
/// # Safety
///
/// `imo` was allocated by `ip_setmoptions` (`malloc(M_IPMOPTS)`), with
/// `imo_max_memberships` slots at `imo_membership`, and is not used afterwards.
pub unsafe fn ip_freemoptions(imo: Option<NonNull<IpMoptions>>) {
    let Some(imo) = imo else {
        return;
    };
    // SAFETY: the caller's contract.
    let o = unsafe { imo.as_ref() };
    for i in 0..usize::from(o.imo_num_memberships) {
        // SAFETY: the first `imo_num_memberships` slots hold joined groups.
        if let Some(inm) = unsafe { *o.imo_membership.add(i) } {
            crate::netinet::in_::in_delmulti(inm);
        }
    }
    if let Some(m) = NonNull::new(o.imo_membership) {
        free(
            m.cast(),
            M_IPMOPTS,
            usize::from(o.imo_max_memberships)
                * size_of::<Option<&crate::netinet::in_var::InMulti>>(),
        );
    }
    free(imo.cast(), M_IPMOPTS, size_of::<IpMoptions>());
}

/// `ip_mloopback`: loops back a copy of an IP multicast packet to the input queue of `ifp`
/// (called from `ip_output`).
pub fn ip_mloopback(ifp: &'static Ifnet, m: &Mbuf, dst: &SockaddrIn) {
    if let Some(copym) = m_dup_pkt(m, MAX_LINKHDR.load(Ordering::Relaxed) as u32, M_DONTWAIT) {
        // We don't bother to fragment if the IP length is greater than the interface's MTU.
        // Can this possibly matter?
        in_hdr_cksum_out(copym, None);
        let _ = if_input_local(ifp, copym, dst.sin_family, None);
    }
}

/// `in_hdr_cksum_out`: the IP header checksum, by the interface if it can or in software.
pub fn in_hdr_cksum_out(m: &Mbuf, ifp: Option<&Ifnet>) {
    let mut ip = mtod_ip(m);

    ip.ip_sum = 0;
    if in_ifcap_cksum(m, ifp, IFCAP_CSUM_IPv4) {
        mtod_ip_store(m, &ip);
        m.m_pkthdr()
            .csum_flags
            .set(m.m_pkthdr().csum_flags.get() | M_IPV4_CSUM_OUT);
    } else {
        ipstat_inc(IpstatCounters::IpsOutswcsum);
        mtod_ip_store(m, &ip);
        ip.ip_sum = in_cksum(m, i32::from(ip.ip_hl()) << 2);
        mtod_ip_store(m, &ip);
        m.m_pkthdr()
            .csum_flags
            .set(m.m_pkthdr().csum_flags.get() & !M_IPV4_CSUM_OUT);
    }
}

/// `in_cksum_phdr`: computes the significant parts of the IPv4 checksum pseudo-header for use
/// in a delayed TCP/UDP checksum calculation.
fn in_cksum_phdr(src: u32, dst: u32, lenproto: u32) -> u16 {
    let mut sum: u32 = lenproto
        .wrapping_add(u32::from((src >> 16) as u16))
        .wrapping_add(u32::from(src as u16))
        .wrapping_add(u32::from((dst >> 16) as u16))
        .wrapping_add(u32::from(dst as u16));

    sum = u32::from((sum >> 16) as u16) + u32::from(sum as u16);

    if sum > 0xffff {
        sum -= 0xffff;
    }

    sum as u16
}

/// Writes a 16-bit checksum at byte `offset` of the packet.
fn cksum_store(m: &Mbuf, offset: usize, csum: u16) {
    if offset + size_of::<u16>() > m.m_len().get() as usize {
        let _ = m_copyback(m, offset as i32, &csum.to_ne_bytes(), M_NOWAIT);
    } else {
        // SAFETY: the first mbuf holds the two bytes at `offset`.
        unsafe { ptr::write_unaligned(mtod::<u8>(m).add(offset).cast::<u16>(), csum) };
    }
}

/// `in_delayed_cksum`: processes a delayed payload checksum calculation.
pub fn in_delayed_cksum(m: &Mbuf) {
    let ip = mtod_ip(m);
    let mut offset = usize::from(ip.ip_hl()) << 2;
    let mut csum = in4_cksum(m, 0, offset as i32, m.m_pkthdr().len.get() - offset as i32);
    if csum == 0 && i32::from(ip.ip_p) == IPPROTO_UDP {
        csum = 0xffff;
    }

    match i32::from(ip.ip_p) {
        IPPROTO_TCP => offset += TH_SUM_OFFSET,
        IPPROTO_UDP => offset += UH_SUM_OFFSET,
        IPPROTO_ICMP => offset += ICMP_CKSUM_OFFSET,
        _ => return,
    }

    cksum_store(m, offset, csum);
}

/// `in_proto_cksum_out`: the transport checksum of an outgoing packet: the pseudo header for
/// the hardware, or the whole checksum in software when the interface cannot.
pub fn in_proto_cksum_out(m: &Mbuf, ifp: Option<&Ifnet>) {
    let ip = mtod_ip(m);
    let flags = m.m_pkthdr().csum_flags.get();

    // some hw and in_delayed_cksum need the pseudo header cksum
    if flags & (M_TCP_CSUM_OUT | M_UDP_CSUM_OUT | M_ICMP_CSUM_OUT) != 0 {
        let mut csum: u16 = 0;
        let mut offset = usize::from(ip.ip_hl()) << 2;
        if flags & M_TCP_TSO != 0 && in_ifcap_cksum(m, ifp, IFCAP_TSOv4) {
            csum = in_cksum_phdr(
                ip.ip_src.s_addr,
                ip.ip_dst.s_addr,
                htonl(u32::from(ip.ip_p)),
            );
        } else if flags & (M_TCP_CSUM_OUT | M_UDP_CSUM_OUT) != 0 {
            csum = in_cksum_phdr(
                ip.ip_src.s_addr,
                ip.ip_dst.s_addr,
                htonl(u32::from(ntohs(ip.ip_len)) - offset as u32 + u32::from(ip.ip_p)),
            );
        }
        match i32::from(ip.ip_p) {
            IPPROTO_TCP => offset += TH_SUM_OFFSET,
            IPPROTO_UDP => offset += UH_SUM_OFFSET,
            IPPROTO_ICMP => offset += ICMP_CKSUM_OFFSET,
            _ => {}
        }
        cksum_store(m, offset, csum);
    }

    let flags = m.m_pkthdr().csum_flags.get();
    if flags & M_TCP_CSUM_OUT != 0 {
        if !in_ifcap_cksum(m, ifp, IFCAP_CSUM_TCPv4) || ip.ip_hl() != 5 {
            let _ = unported!("tcpstat_inc (netinet/tcp_*.c)");
            in_delayed_cksum(m);
            m.m_pkthdr().csum_flags.set(flags & !M_TCP_CSUM_OUT); // Clear
        }
    } else if flags & M_UDP_CSUM_OUT != 0 {
        if !in_ifcap_cksum(m, ifp, IFCAP_CSUM_UDPv4) || ip.ip_hl() != 5 {
            udpstat_inc(UdpstatCounters::UdpsOutswcsum);
            in_delayed_cksum(m);
            m.m_pkthdr().csum_flags.set(flags & !M_UDP_CSUM_OUT); // Clear
        }
    } else if flags & M_ICMP_CSUM_OUT != 0 {
        in_delayed_cksum(m);
        m.m_pkthdr().csum_flags.set(flags & !M_ICMP_CSUM_OUT); // Clear
    }
}

/// `in_ifcap_cksum`: whether `ifp` computes checksum `ifcap` for `m` in hardware.
pub fn in_ifcap_cksum(m: &Mbuf, ifp: Option<&Ifnet>, ifcap: u32) -> bool {
    let Some(ifp) = ifp else {
        return false;
    };
    if ifp.if_capabilities.get() & ifcap == 0 || ifp.if_bridgeidx.get() != 0 {
        return false;
    }
    // Simplex interface sends packet back without hardware cksum. Keep this check in sync
    // with the condition where ether_resolve() calls if_input_local().
    if m.m_flags().get() & M_BCAST != 0
        && ifp.if_flags.get() & IFF_SIMPLEX != 0
        && m.m_pkthdr().pf.routed.get() == 0
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests;
