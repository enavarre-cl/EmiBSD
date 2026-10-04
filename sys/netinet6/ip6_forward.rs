/*	$OpenBSD: ip6_forward.c,v 1.130 2026/06/23 15:45:00 bluhm Exp $	*/
/*	$KAME: ip6_forward.c,v 1.75 2001/06/29 12:42:13 jinmei Exp $	*/
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
/* </LICENSES> */

//! IPv6 forwarding: `netinet6/ip6_forward.c`.
//!
//! Upstream: sys/netinet6/ip6_forward.c @ 3ce1f3f79392
//!
//! `ip6_forward` decrements the hop limit, finds the route (`route6_mpath`), applies the
//! outbound IPsec policy, decides on a redirect, runs pf(4) in the forward direction and
//! sends the packet with `if_output_tso`. A copy of the start of the packet (on the stack
//! when it is small) feeds the ICMPv6 error or redirect sent back to the source.
//!
//! ## Deviations
//! - The packet is read and written as a copy of its IPv6 header (`mtod_ip6`/
//!   `mtod_ip6_store`): mbuf data has no alignment guarantee.
//! - The C's `goto reroute` is a labelled loop whose exits name the C's labels `senderr`,
//!   `icmperror` and `freecopy`; `done` is the end of a labelled block. `struct route *ro`
//!   is an `Option<&Route>`; the local `iproute` is used when it is `None`, as in C.
//! - The `IPV6_FORWARDING_IPSEC` check frees the packet before `goto senderr`; the C leaves
//!   it allocated there (a leak).
//! - `ip6_output_ipsec_lookup`'s errors (`SpdError::Drop` is the C's `-EINVAL`, silently
//!   drop) all lead to `freecopy`, as the C's `error` is not read after it.
//! - `NPF` (pf(4)) and `IPSEC` are configured.

use core::mem::size_of;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::kern::uipc_mbuf::{m_copydata, m_copym, m_freem, m_gethdr};
use crate::net::if_::{IFF_POINTOPOINT, if_get, if_output_tso, if_put};
use crate::net::if_var::Ifnet;
use crate::net::pf::pf_test;
use crate::net::pfvar::{PF_FWD, PF_PASS};
use crate::net::route::{
    RTF_DYNAMIC, RTF_GATEWAY, RTF_MODIFIED, Route, Rtentry, route6_mpath, rtfree,
};
use crate::netinet::icmp6::{
    ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_ADDR, ICMP6_DST_UNREACH_BEYONDSCOPE,
    ICMP6_DST_UNREACH_NOROUTE, ICMP6_PACKET_TOO_BIG, ICMP6_TIME_EXCEED_TRANSIT,
    ICMP6_TIME_EXCEEDED, ICMPV6_PLD_MAXLEN, ND_REDIRECT,
};
use crate::netinet::in_::{IPPROTO_ESP, IPPROTO_TCP, IPPROTO_UDP};
use crate::netinet::ip_ipsp::{IPSEC_IN_USE, Tdb, tdb_unref};
use crate::netinet::ip6::{IPV6_HLIMDEC, Ip6Hdr};
use crate::netinet::tcp::{MAX_TCPOPTLEN, Tcphdr};
use crate::netinet::udp::Udphdr;
use crate::netinet6::icmp6::{icmp6_error, icmp6_redirect_output};
use crate::netinet6::in6::{
    in6_addr2scopeid, in6_is_addr_multicast, in6_is_addr_unspecified, in6_is_scope_embed,
};
use crate::netinet6::in6_proto::IP6_SENDREDIRECTS;
use crate::netinet6::ip6_output::{ip6_output_ipsec_lookup, ip6_output_ipsec_send};
use crate::netinet6::ip6_var::{
    IPV6_FORWARDING, IPV6_FORWARDING_IPSEC, IPV6_REDIRECT, Ip6statCounters, ip6stat_inc, mtod_ip6,
    mtod_ip6_store,
};
use crate::netinet6::nd6::nd6_is_addr_neighbor;
use crate::sys::errno::Errno;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::mbuf::{
    M_BCAST, M_COPYFLAGS, M_DONTWAIT, M_MCAST, MHLEN, MT_DATA, Mbuf, PACKET_TAG_IPSEC_IN_DONE,
    PF_TAG_GENERATED, PF_TAG_REROUTE,
};
use crate::sys::socket::{AF_INET6, Sockaddr};

/// Where `ip6_forward` goes once the packet left its hands: the C's labels.
enum Exit {
    /// `senderr`: the send failed or the packet was consumed; maybe an ICMPv6 message.
    Senderr,
    /// `icmperror`: send the ICMPv6 error of `type`/`code`.
    Icmperror,
    /// `freecopy`: drop the copy.
    Freecopy,
}

/// `ip6_forward`: forwards a packet. If some error occurs return the sender an icmp packet.
/// Note we can't always generate a meaningful icmp message because icmp doesn't have a large
/// enough repertoire of codes and types.
///
/// If not forwarding, just drop the packet. This could be confusing if ip6_forwarding was
/// zero but some routing protocol was advancing us as a gateway to somewhere. However, we
/// must let the routing protocol deal with that. Consumes the packet.
pub fn ip6_forward(m: &'static Mbuf, ro: Option<&Route>, flags: i32) {
    let mut m = m;
    let mut ip6 = mtod_ip6(m);
    let iproute = Route::new();
    let mut ro = ro;
    let mut ifp: Option<&'static Ifnet> = None;
    let rtableid = m.m_pkthdr().ph_rtableid.get();
    let ifidx = m.m_pkthdr().ph_ifidx.get();
    let loopcnt = m.m_pkthdr().ph_loopcnt.get();
    let mut icmp_buf = [0u8; MHLEN];
    let mut flags = flags;
    let mut error: Result<(), Errno> = Ok(());
    let mut type_: u8 = 0;
    let mut code: u8 = 0;
    let mut destmtu: i32 = 0;
    let mut rt: Option<&'static Rtentry> = None;
    let mut tdb: Option<&'static Tdb> = None;

    'done: {
        // Do not forward packets to multicast destination (should be handled by
        // ip6_mforward(). Do not forward packets with unspecified source. It was discussed in
        // July 2000, on ipngwg mailing list.
        if m.m_flags().get() & (M_BCAST | M_MCAST) != 0
            || in6_is_addr_multicast(&ip6.ip6_dst)
            || in6_is_addr_unspecified(&ip6.ip6_src)
        {
            ip6stat_inc(Ip6statCounters::Ip6sCantforward);
            m_freem(m);
            break 'done;
        }

        if ip6.ip6_hlim <= IPV6_HLIMDEC {
            icmp6_error(m, ICMP6_TIME_EXCEEDED, ICMP6_TIME_EXCEED_TRANSIT, 0);
            break 'done;
        }
        ip6.ip6_hlim -= IPV6_HLIMDEC;
        mtod_ip6_store(m, &ip6);

        // Save at most ICMPV6_PLD_MAXLEN (= the min IPv6 MTU - size of IPv6 + ICMPv6
        // headers) bytes of the packet in case we need to generate an ICMP6 message to the
        // src. Thanks to M_EXT, in most cases copy will not occur. For small packets copy
        // original onto stack instead of mbuf.
        //
        // For final protocol header like TCP or UDP, full header chain in ICMP6 packet is not
        // necessary. In this case only copy small part of original packet and save it on
        // stack instead of mbuf. Although this violates RFC 4443 2.4. (c), it avoids
        // additional mbuf allocations. Also pf nat and rdr do not affect the shared mbuf
        // cluster.
        //
        // It is important to save it before IPsec processing as IPsec processing may modify
        // the mbuf.
        let mut icmp_len: usize = match i32::from(ip6.ip6_nxt) {
            IPPROTO_TCP => size_of::<Ip6Hdr>() + size_of::<Tcphdr>() + MAX_TCPOPTLEN,
            IPPROTO_UDP => size_of::<Ip6Hdr>() + size_of::<Udphdr>(),
            IPPROTO_ESP => size_of::<Ip6Hdr>() + 2 * size_of::<u32>(),
            _ => ICMPV6_PLD_MAXLEN,
        };
        let pktlen = m.m_pkthdr().len.get().max(0) as usize;
        if icmp_len > pktlen {
            icmp_len = pktlen;
        }
        let mut mflags = 0;
        let mut pfflags = 0;
        let mut mcopy: Option<&'static Mbuf>;
        if icmp_len <= icmp_buf.len() {
            mflags = m.m_flags().get();
            pfflags = m.m_pkthdr().pf.flags.get();
            m_copydata(m, 0, &mut icmp_buf[..icmp_len]);
            mcopy = None;
        } else {
            mcopy = m_copym(m, 0, icmp_len as i32, M_NOWAIT);
            icmp_len = 0;
        }
        let icmp_buf = icmp_buf;
        // The copy on the stack as a packet, for the ICMPv6 message.
        let stack_copy = || -> Option<&'static Mbuf> {
            let mc = m_gethdr(M_DONTWAIT, MT_DATA)?;
            mc.m_len().set(icmp_len as u32);
            mc.m_pkthdr().len.set(icmp_len as i32);
            mc.m_flags()
                .set(mc.m_flags().get() | (mflags & M_COPYFLAGS));
            mc.m_pkthdr().ph_rtableid.set(rtableid);
            mc.m_pkthdr().ph_ifidx.set(ifidx);
            mc.m_pkthdr().ph_loopcnt.set(loopcnt);
            let pf = &mc.m_pkthdr().pf.flags;
            pf.set(pf.get() | (pfflags & PF_TAG_GENERATED));
            // SAFETY: a fresh packet header mbuf has MHLEN bytes at its data, and `icmp_len`
            // is at most MHLEN here.
            unsafe { ptr::copy_nonoverlapping(icmp_buf.as_ptr(), mc.m_data().get(), icmp_len) };
            Some(mc)
        };

        let orig_rtableid = m.m_pkthdr().ph_rtableid.get();

        let exit = 'reroute: loop {
            if IPSEC_IN_USE.load(Ordering::Relaxed) != 0 {
                match ip6_output_ipsec_lookup(m, None) {
                    Ok(t) => tdb = t,
                    Err(_) => {
                        // -EINVAL is used to indicate that the packet should be silently
                        // dropped, typically because we've asked key management for an SA.
                        m_freem(m);
                        break 'reroute Exit::Freecopy;
                    }
                }
            }

            let r: &Route = match ro {
                Some(r) => r,
                None => {
                    iproute.ro_rt.set(None);
                    ro = Some(&iproute);
                    &iproute
                }
            };
            rt = route6_mpath(
                r,
                &ip6.ip6_dst,
                Some(&ip6.ip6_src),
                m.m_pkthdr().ph_rtableid.get(),
            );
            let Some(rtv) = rt else {
                ip6stat_inc(Ip6statCounters::Ip6sNoroute);
                type_ = ICMP6_DST_UNREACH;
                code = ICMP6_DST_UNREACH_NOROUTE;
                m_freem(m);
                break 'reroute Exit::Icmperror;
            };
            let mut dst: *const Sockaddr = r.ro_dstsa();

            // Scope check: if a packet can't be delivered to its destination for the reason
            // that the destination is beyond the scope of the source address, discard the
            // packet and return an icmp6 destination unreachable error with Code 2 (beyond
            // scope of source address). [draft-ietf-ipngwg-icmp-v3-00.txt, Section 3.1]
            if in6_addr2scopeid(ifidx, &ip6.ip6_src)
                != in6_addr2scopeid(rtv.rt_ifidx.get(), &ip6.ip6_src)
            {
                ip6stat_inc(Ip6statCounters::Ip6sCantforward);
                ip6stat_inc(Ip6statCounters::Ip6sBadscope);
                type_ = ICMP6_DST_UNREACH;
                code = ICMP6_DST_UNREACH_BEYONDSCOPE;
                m_freem(m);
                break 'reroute Exit::Icmperror;
            }

            // Check if the packet needs encapsulation. ipsp_process_packet will never come
            // back to here.
            if let Some(t) = tdb {
                // Callee frees mbuf
                error = ip6_output_ipsec_send(t, m, Some(r), orig_rtableid, true);
                rt = r.ro_rt.get();
                if error.is_err() {
                    break 'reroute Exit::Senderr;
                }
                break 'reroute Exit::Freecopy;
            }

            if rtv.rt_flags.get() & RTF_GATEWAY != 0 {
                dst = rtv.rt_gateway.get();
            }

            // If we are to forward the packet using the same interface as one we got the
            // packet from, perhaps we should send a redirect to sender to shortcut a hop.
            // Only send redirect if source is sending directly to us, and if packet was not
            // source routed (or has any options). Also, don't send redirect if forwarding
            // using a route modified by a redirect.
            ifp = if_get(rtv.rt_ifidx.get());
            let Some(i) = ifp else {
                m_freem(m);
                break 'reroute Exit::Freecopy;
            };
            if rtv.rt_ifidx.get() == ifidx
                && rtv.rt_flags.get() & (RTF_DYNAMIC | RTF_MODIFIED) == 0
                && flags & IPV6_REDIRECT == 0
                && IP6_SENDREDIRECTS.load(Ordering::Relaxed) != 0
            {
                if i.if_flags.get() & IFF_POINTOPOINT != 0
                    && nd6_is_addr_neighbor(&r.ro_dstsin6(), i)
                {
                    // If the incoming interface is equal to the outgoing one, the link
                    // attached to the interface is point-to-point, and the IPv6 destination
                    // is regarded as on-link on the link, then it will be highly probable
                    // that the destination address does not exist on the link and that the
                    // packet is going to loop. Thus, we immediately drop the packet and send
                    // an ICMPv6 error message. For other routing loops, we dare to let the
                    // packet go to the loop, so that a remote diagnosing host can detect the
                    // loop by traceroute. type/code is based on suggestion by Rich Draves.
                    // not sure if it is the best pick.
                    type_ = ICMP6_DST_UNREACH;
                    code = ICMP6_DST_UNREACH_ADDR;
                    m_freem(m);
                    break 'reroute Exit::Icmperror;
                }
                type_ = ND_REDIRECT;
            }

            // Fake scoped addresses. Note that even link-local source or destination can
            // appear, if the originating node just sends the packet to us (without address
            // resolution for the destination). Since both icmp6_error and
            // icmp6_redirect_output fill the embedded link identifiers, we can do this stuff
            // after making a copy for returning an error.
            if in6_is_scope_embed(&ip6.ip6_src) {
                ip6.ip6_src.set_s6_addr16(1, 0);
            }
            if in6_is_scope_embed(&ip6.ip6_dst) {
                ip6.ip6_dst.set_s6_addr16(1, 0);
            }
            mtod_ip6_store(m, &ip6);

            // Packet filter
            let mut mp = Some(m);
            if pf_test(AF_INET6, PF_FWD, i, &mut mp) != PF_PASS {
                m_freem(mp);
                break 'reroute Exit::Senderr;
            }
            let Some(mm) = mp else {
                break 'reroute Exit::Senderr;
            };
            m = mm;
            ip6 = mtod_ip6(m);
            let pf = &m.m_pkthdr().pf.flags;
            if pf.get() & (PF_TAG_REROUTE | PF_TAG_GENERATED) == (PF_TAG_REROUTE | PF_TAG_GENERATED)
            {
                // already rerun the route lookup, go on
                pf.set(pf.get() & !(PF_TAG_GENERATED | PF_TAG_REROUTE));
            } else if pf.get() & PF_TAG_REROUTE != 0 {
                // tag as generated to skip over pf_test on rerun
                pf.set(pf.get() | PF_TAG_GENERATED);
                flags |= IPV6_REDIRECT;
                if ptr::eq(r, &iproute) {
                    rtfree(iproute.ro_rt.get());
                    iproute.ro_rt.set(None);
                }
                ro = None;
                if_put(ifp);
                ifp = None;
                continue 'reroute;
            }

            if flags & IPV6_FORWARDING != 0
                && flags & IPV6_FORWARDING_IPSEC != 0
                && m.m_pkthdr().ph_tagsset.get() & PACKET_TAG_IPSEC_IN_DONE == 0
            {
                error = Err(Errno::EHOSTUNREACH);
                // The C keeps the packet here (see the module's deviations).
                m_freem(m);
                break 'reroute Exit::Senderr;
            }

            let mut mp = Some(m);
            // SAFETY: `dst` is the route's destination `sockaddr_in6` (in `r`, alive for this
            // call) or its gateway, readable while the route is held.
            error = unsafe { if_output_tso(i, &mut mp, dst, rt, i.if_mtu.get()) };
            if error.is_err() {
                ip6stat_inc(Ip6statCounters::Ip6sCantforward);
            } else if mp.is_none() {
                ip6stat_inc(Ip6statCounters::Ip6sForward);
            }
            if error.is_err() || mp.is_none() {
                break 'reroute Exit::Senderr;
            }

            type_ = ICMP6_PACKET_TOO_BIG;
            destmtu = i.if_mtu.get() as i32;
            m_freem(mp);
            break 'reroute Exit::Icmperror;
        };

        let exit = match exit {
            Exit::Senderr => {
                if mcopy.is_none() && icmp_len == 0 {
                    break 'done;
                }

                match error {
                    Ok(()) => {
                        if type_ == ND_REDIRECT {
                            if icmp_len != 0 {
                                mcopy = stack_copy();
                                if mcopy.is_none() {
                                    break 'done;
                                }
                            }
                            if let Some(mc) = mcopy {
                                match rt {
                                    Some(rt) => {
                                        icmp6_redirect_output(mc, rt);
                                        ip6stat_inc(Ip6statCounters::Ip6sRedirectsent);
                                    }
                                    // A sent packet had a route.
                                    None => {
                                        m_freem(mc);
                                    }
                                }
                            }
                            break 'done;
                        }
                        Exit::Freecopy
                    }
                    Err(Errno::EMSGSIZE) => {
                        type_ = ICMP6_PACKET_TOO_BIG;
                        if let Some(rt) = rt {
                            let rtmtu = rt.rt_mtu().load(Ordering::Relaxed);
                            if rtmtu != 0 {
                                destmtu = rtmtu as i32;
                            } else {
                                let destifp = if_get(rt.rt_ifidx.get());
                                if let Some(d) = destifp {
                                    destmtu = d.if_mtu.get() as i32;
                                }
                                if_put(destifp);
                            }
                        }
                        ip6stat_inc(Ip6statCounters::Ip6sCantfrag);
                        if destmtu == 0 {
                            Exit::Freecopy
                        } else {
                            Exit::Icmperror
                        }
                    }
                    // pf(4) blocked the packet. There is no need to send an ICMP packet back
                    // since pf(4) takes care of it.
                    Err(Errno::EACCES) => Exit::Freecopy,
                    // Tell source to slow down like source quench in IP?
                    Err(Errno::ENOBUFS) => Exit::Freecopy,
                    // ENETUNREACH (shouldn't happen, checked above), EHOSTUNREACH, ENETDOWN,
                    // EHOSTDOWN, default:
                    Err(_) => {
                        type_ = ICMP6_DST_UNREACH;
                        code = ICMP6_DST_UNREACH_ADDR;
                        Exit::Icmperror
                    }
                }
            }
            other => other,
        };

        match exit {
            Exit::Icmperror => {
                if icmp_len != 0 {
                    mcopy = stack_copy();
                    if mcopy.is_none() {
                        break 'done;
                    }
                }
                if let Some(mc) = mcopy {
                    icmp6_error(mc, type_, code, destmtu);
                }
            }
            // freecopy:
            _ => {
                m_freem(mcopy);
            }
        }
    }
    // done:
    if ro.is_some_and(|r| ptr::eq(r, &iproute)) {
        rtfree(iproute.ro_rt.get());
    }
    if_put(ifp);
    tdb_unref(tdb);
}

// The stack copy holds the headers of a TCP segment (the C's CTASSERT on `icmp_buf`).
const _: () = assert!(size_of::<Ip6Hdr>() + size_of::<Tcphdr>() + MAX_TCPOPTLEN <= MHLEN);
