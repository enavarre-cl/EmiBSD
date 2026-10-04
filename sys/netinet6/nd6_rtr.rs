/*	$OpenBSD: nd6_rtr.c,v 1.176 2025/07/08 00:47:41 jsg Exp $	*/
/*	$KAME: nd6_rtr.c,v 1.97 2001/02/07 11:09:13 itojun Exp $	*/
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

//! Router advertisements and solicitations seen by a host, and route flushing:
//! `netinet6/nd6_rtr.c`.
//!
//! Upstream: sys/netinet6/nd6_rtr.c @ 3ce1f3f79392
//!
//! The kernel no longer processes router advertisements (slaacd(8) does, from a raw
//! socket); what is left is caching the link-layer address a router solicitation or
//! advertisement carries (`nd6_rtr_cache`), and `rt6_flush`, which removes the host routes
//! through a router that stopped being one.
//!
//! ## Deviations
//! - The message is read out of the mbuf as a copy; a message shorter than its fixed part
//!   (which `icmp6_input` already refuses) counts as `icp6s_tooshort` instead of being read
//!   past.
//! - `rt6_deleteroute` is the closure `rtable_walk` takes (a `void *` argument in C); its
//!   `EEXIST` is the walk's error.

use core::mem::size_of;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::kassert;
use crate::kern::uipc_mbuf::m_freem;
use crate::net::if_::{if_get, if_put};
use crate::net::if_var::Ifnet;
use crate::net::route::{
    RTAX_DST, RTAX_GATEWAY, RTAX_NETMASK, RTF_HOST, RTF_STATIC, RTP_ANY, RtAddrinfo, Rtentry,
    rt_plen2mask, rtfree, rtrequest_delete,
};
use crate::net::rtable::{rt_key, rtable_walk};
use crate::netinet::icmp6::{
    Icmp6statCounters, ND_ROUTER_ADVERT, ND_ROUTER_SOLICIT, NdRouterAdvert, NdRouterSolicit,
    icmp6stat_inc,
};
use crate::netinet::ip6::ip6_exthdr_get;
use crate::netinet6::in6::{
    In6Addr, in6_are_addr_equal, in6_is_addr_linklocal, in6_is_addr_unspecified, satosin6_const,
};
use crate::netinet6::in6_proto::IP6_FORWARDING;
use crate::netinet6::ip6_var::mtod_ip6;
use crate::netinet6::nd6::{NdOpts, nd6_cache_lladdr, nd6_opt_lladdr, nd6_options};
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::socket::{AF_INET6, SockaddrStorage};
use crate::sys::systm::{kernel_lock, kernel_unlock, net_assert_locked};

/// `nd6_rtr_cache`: process Source Link-layer Address Options from Router Solicitation /
/// Advertisement Messages of `icmp6_type` (`icmp6len` bytes at `off` of `m`, consumed).
pub fn nd6_rtr_cache(m: &'static Mbuf, off: i32, icmp6len: i32, icmp6_type: u8) {
    let ip6 = mtod_ip6(m);
    let saddr6 = ip6.ip6_src;
    let i_am_router = IP6_FORWARDING.load(Ordering::Relaxed) != 0;

    kassert!(icmp6_type == ND_ROUTER_SOLICIT || icmp6_type == ND_ROUTER_ADVERT);

    let mut mp = Some(m);
    let bad = 'out: {
        // Sanity checks
        if ip6.ip6_hlim != 255 {
            break 'out true;
        }

        let fixed = if icmp6_type == ND_ROUTER_SOLICIT {
            // Don't update the neighbor cache, if src = ::. This indicates that the src has
            // no IP address assigned yet.
            if in6_is_addr_unspecified(&saddr6) {
                break 'out false;
            }
            size_of::<NdRouterSolicit>()
        } else {
            if !in6_is_addr_linklocal(&saddr6) {
                break 'out true;
            }
            size_of::<NdRouterAdvert>()
        };

        let p = if (icmp6len as usize) < fixed {
            None
        } else {
            ip6_exthdr_get(&mut mp, off, icmp6len)
        };
        let Some(p) = p else {
            if (icmp6len as usize) < fixed {
                m_freem(m);
            }
            icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
            return;
        };

        // SAFETY: `ip6_exthdr_get` made `icmp6len` bytes (at least the fixed part, checked
        // above) contiguous at `p`; they live with the mbuf, freed below.
        let opts = unsafe { core::slice::from_raw_parts(p.add(fixed), icmp6len as usize - fixed) };
        let mut ndopts = NdOpts::default();
        if !nd6_options(opts, &mut ndopts) {
            // nd6_options have incremented stats
            break 'out false;
        }

        let (lladdr, lladdrlen) = match ndopts.nd_opts_src_lladdr {
            Some(o) => {
                // SAFETY: `nd6_options` found the option inside the message, alive until the
                // mbuf is freed below.
                let (l, n) = unsafe { nd6_opt_lladdr(o) };
                (Some(l), n)
            }
            None => (None, 0),
        };

        let Some(m) = mp else {
            return;
        };
        let Some(ifp) = if_get(m.m_pkthdr().ph_ifidx.get()) else {
            break 'out false;
        };

        if lladdr.is_some() && ((i32::from(ifp.if_addrlen.get()) + 2 + 7) & !7) != lladdrlen {
            if_put(ifp);
            break 'out true;
        }

        nd6_cache_lladdr(
            ifp,
            &saddr6,
            lladdr,
            i32::from(icmp6_type),
            0,
            i32::from(i_am_router),
        );
        if_put(ifp);
        false
    };

    if bad {
        icmp6stat_inc(if icmp6_type == ND_ROUTER_SOLICIT {
            Icmp6statCounters::Icp6sBadrs
        } else {
            Icmp6statCounters::Icp6sBadra
        });
    }
    // freeit:
    m_freem(mp);
}

/// `rt6_flush`: delete all the routing table entries that use the specified gateway.
/// XXX: this function causes search through all entries of routing table, so it shouldn't
/// be called when acting as a router. The gateway must already contain KAME's hack for
/// link-local scope.
pub fn rt6_flush(gateway: &In6Addr, ifp: &Ifnet) -> Result<(), Errno> {
    net_assert_locked("rt6_flush");

    // We'll care only link-local addresses
    if !in6_is_addr_linklocal(gateway) {
        return Ok(());
    }

    kassert!(gateway.s6_addr16(1) != 0);

    loop {
        let mut rt: Option<&'static Rtentry> = None;
        let mut error = rtable_walk(ifp.if_rdomain.get(), AF_INET6, Some(&mut rt), |rt, id| {
            rt6_deleteroute(rt, gateway, id)
        });
        if let Some(r) = rt
            && error == Err(Errno::EEXIST)
        {
            let mut info = RtAddrinfo::new();
            let mut sa_mask = SockaddrStorage::zeroed();
            info.rti_flags = r.rt_flags.get();
            info.rti_info[RTAX_DST] = rt_key(r);
            info.rti_info[RTAX_GATEWAY] = r.rt_gateway.get();
            info.rti_info[RTAX_NETMASK] = rt_plen2mask(r, &mut sa_mask);
            kernel_lock();
            // SAFETY: the route's key, gateway and the local netmask are readable socket
            // addresses.
            error =
                unsafe { rtrequest_delete(&mut info, RTP_ANY, ifp, None, ifp.if_rdomain.get()) };
            kernel_unlock();
            if error.is_ok() {
                error = Err(Errno::EAGAIN);
            }
        }
        rtfree(rt);
        if error != Err(Errno::EAGAIN) {
            return error;
        }
    }
}

/// `rt6_deleteroute`: whether `rt` is a host route through `gate` that `rt6_flush` should
/// delete (`Err(EEXIST)` stops the walk at it).
fn rt6_deleteroute(rt: &Rtentry, gate: &In6Addr, _id: u32) -> Result<(), Errno> {
    let g = rt.rt_gateway.get();
    // SAFETY: a route's gateway is NULL or a readable socket address.
    if g.is_null() || unsafe { (*g).sa_family } != AF_INET6 {
        return Ok(());
    }

    // SAFETY: an `AF_INET6` gateway is a `sockaddr_in6`, at any alignment.
    let ga = unsafe { ptr::read_unaligned(satosin6_const(g)) }.sin6_addr;
    if !in6_are_addr_equal(gate, &ga) {
        return Ok(());
    }

    // Do not delete a static route.
    // XXX: this seems to be a bit ad-hoc. Should we consider the 'cloned' bit instead?
    if rt.rt_flags.get() & RTF_STATIC != 0 {
        return Ok(());
    }

    // We delete only host route. This means, in particular, we don't delete default route.
    if rt.rt_flags.get() & RTF_HOST == 0 {
        return Ok(());
    }

    Err(Errno::EEXIST)
}
