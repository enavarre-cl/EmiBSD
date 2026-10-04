/*	$OpenBSD: in6_src.c,v 1.104 2025/07/18 08:39:14 mvs Exp $	*/
/*	$KAME: in6_src.c,v 1.36 2001/02/06 04:08:17 itojun Exp $	*/
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
 * Copyright (c) 1982, 1986, 1991, 1993
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
 *	@(#)in_pcb.c	8.2 (Berkeley) 1/4/94
 */
/* </LICENSES> */

//! IPv6 source address and route selection, the hop limit of a pcb, and the
//! embedding of scope zone ids in addresses: `netinet6/in6_src.c`.
//!
//! Upstream: sys/netinet6/in6_src.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `in6_pcbselsrc` and `in6_selectsrc` hand the address back by value through `in6src`
//!   (`&mut In6Addr`), where the C returns a pointer to it (into the pcb, the packet info or
//!   the interface address); `in6_embedscope` and `in6_recoverscope` already did.
//! - `in6_selectif` is file-local in C and here; its `struct ifnet **retifp` is the
//!   returned `Option<&'static Ifnet>` (a reference from `if_get`, released with `if_put`).
//! - The packet info and the multicast options of a pcb are plain structures owned by the
//!   socket, reached through the `NonNull`s the pcb keeps (`ip6po_pktinfo`,
//!   `inp_moptions6`); `in6_pcbselsrc` still overrides `pi->ipi6_addr` in place, as the C
//!   does.
//! - `in6_selectsrc`'s route-based fallback (`route6_mpath` and the route's interface
//!   address) is in `in6_pcbselsrc`, as in the C; a route whose interface address is not
//!   an `AF_INET6` one is treated like a route without one (the C reinterprets the
//!   structure).

use core::ptr;

use crate::net::if_::{IFF_UP, if_get, if_put, ifa_ifwithaddr};
use crate::net::if_var::Ifnet;
use crate::net::route::{
    RTF_BLACKHOLE, RTF_GATEWAY, RTF_HOST, RTF_LOCAL, RTF_REJECT, Route, Rtentry, route6_mpath,
};
use crate::net::rtable::rtable_getsource;
use crate::netinet::in_pcb::Inpcb;
use crate::netinet6::in6::In6Addr;
use crate::netinet6::in6::{
    In6Pktinfo, SockaddrIn6, ifatoia6, in6_ifawithscope, in6_is_addr_linklocal,
    in6_is_addr_mc_intfacelocal, in6_is_addr_mc_linklocal, in6_is_addr_multicast,
    in6_is_addr_unspecified, in6_is_scope_embed, satosin6_const, sin6tosa_const,
};
use crate::netinet6::in6_proto::IP6_DEFHLIM;
use crate::netinet6::in6_var::{IN6_IFF_ANYCAST, IN6_IFF_DUPLICATED, IN6_IFF_TENTATIVE, ia6_in6};
use crate::netinet6::ip6_var::{Ip6Moptions, Ip6Pktopts};
use crate::sys::endian::{htons, ntohs};
use crate::sys::errno::Errno;
use crate::sys::socket::AF_INET6;
use core::sync::atomic::Ordering;

/// The packet info of `opts`, if any, as the C's `opts->ip6po_pktinfo` pointer.
fn pktinfo(opts: Option<&Ip6Pktopts>) -> Option<ptr::NonNull<In6Pktinfo>> {
    opts.and_then(|o| o.ip6po_pktinfo)
}

/// `in6_pcbselsrc`: return an IPv6 address, which is the most appropriate for a given
/// destination and pcb (stored in `in6src`). We need the additional `opts` parameter because
/// the values set at pcb level can be overridden via cmsg.
pub fn in6_pcbselsrc(
    in6src: &mut In6Addr,
    dstsock: &SockaddrIn6,
    inp: &Inpcb,
    opts: Option<&Ip6Pktopts>,
) -> Result<(), Errno> {
    let dst = &dstsock.sin6_addr;
    let laddr = inp.inp_laddr6.get();
    // SAFETY: the pcb's multicast options are a plain structure owned by the socket, valid
    // while the pcb is (the socket lock).
    let mopts: Option<&Ip6Moptions> = inp.inp_moptions6.get().map(|p| unsafe { &*p.as_ptr() });
    let rtableid = inp.inp_rtableid.get();
    let pi = pktinfo(opts);

    // If the source address is explicitly specified by the caller, check if the requested
    // source address is indeed a unicast address assigned to the node, and can be used as the
    // packet's source address. If everything is okay, use the address as source.
    if let Some(pi) = pi
        // SAFETY: the packet info is a plain structure owned by the socket (or the sticky
        // options), valid for the call.
        && !in6_is_addr_unspecified(unsafe { &(*pi.as_ptr()).ipi6_addr })
    {
        // get the outgoing interface
        let ifp = in6_selectif(dst, opts, mopts, &inp.inp_route, rtableid)?;

        let mut sa6 = SockaddrIn6::zeroed();
        sa6.sin6_family = AF_INET6;
        sa6.sin6_len = core::mem::size_of::<SockaddrIn6>() as u8;
        // SAFETY: as above.
        sa6.sin6_addr = unsafe { (*pi.as_ptr()).ipi6_addr };

        if let Some(ifp) = ifp
            && in6_is_scope_embed(&sa6.sin6_addr)
        {
            sa6.sin6_addr
                .set_s6_addr16(1, htons(ifp.if_index.get() as u16));
        }
        if_put(ifp); // put reference from in6_selectif

        // SAFETY: `sa6` is a socket address of its `sin6_len` bytes, live for the call.
        let ia6 = unsafe { ifa_ifwithaddr(sin6tosa_const(&sa6), rtableid) }.map(ifatoia6);
        let Some(ia6) = ia6 else {
            return Err(Errno::EADDRNOTAVAIL);
        };
        if ia6.ia6_flags.get() & (IN6_IFF_ANYCAST | IN6_IFF_TENTATIVE | IN6_IFF_DUPLICATED) != 0 {
            return Err(Errno::EADDRNOTAVAIL);
        }

        // XXX: this overrides pi
        // SAFETY: as above; the address is written, nothing else of the structure.
        unsafe { (*pi.as_ptr()).ipi6_addr = sa6.sin6_addr };

        *in6src = sa6.sin6_addr;
        return Ok(());
    }

    // If the source address is not specified but the socket(if any) is already bound, use the
    // bound address.
    if !in6_is_addr_unspecified(&laddr) {
        *in6src = laddr;
        return Ok(());
    }

    // If the caller doesn't specify the source address but the outgoing interface, use an
    // address associated with the interface.
    if let Some(pi) = pi
        // SAFETY: as above.
        && unsafe { (*pi.as_ptr()).ipi6_ifindex } != 0
    {
        // SAFETY: as above.
        let Some(ifp) = if_get(unsafe { (*pi.as_ptr()).ipi6_ifindex }) else {
            return Err(Errno::ENXIO); // XXX: better error?
        };

        let ia6 = in6_ifawithscope(ifp, dst, rtableid, None);
        if_put(ifp);

        let Some(ia6) = ia6 else {
            return Err(Errno::EADDRNOTAVAIL);
        };

        *in6src = ia6_in6(ia6);
        return Ok(());
    }

    match in6_selectsrc(in6src, dstsock, mopts, rtableid) {
        Err(Errno::EADDRNOTAVAIL) => {}
        error => return error,
    }

    // If route is known or can be allocated now, our src addr is taken from the i/f, else
    // punt.
    let rt = route6_mpath(&inp.inp_route, dst, None, rtableid);

    // in_pcbconnect() checks out IFF_LOOPBACK to skip using the address. But we don't know
    // why it does so. It is necessary to ensure the scope even for lo0 so doesn't check out
    // IFF_LOOPBACK.

    let mut ia6 = None;
    if let Some(rt) = rt {
        if let Some(ifp) = if_get(rt.rt_ifidx.get()) {
            ia6 = in6_ifawithscope(ifp, dst, rtableid, Some(rt));
            if_put(ifp);
        }
        if ia6.is_none() {
            // xxx scope error ?
            ia6 = rt
                .rt_ifa
                .get()
                // SAFETY: an interface address's `ifa_addr` is readable (`ifa_add`'s
                // contract).
                .filter(|ifa| unsafe { (*ifa.ifa_addr.get()).sa_family } == AF_INET6)
                .map(ifatoia6);
        }
    }

    // Use preferred source address if :
    // - destination is not onlink
    // - preferred source address is set
    // - output interface is UP
    if let Some(rt) = rt
        && rt.rt_flags.get() & RTF_GATEWAY != 0
    {
        let ip6_source = rtable_getsource(rtableid, AF_INET6);
        if !ip6_source.is_null() {
            // SAFETY: a table's preferred source is a readable socket address.
            let ifa = unsafe { ifa_ifwithaddr(ip6_source, rtableid) };
            if let Some(ifa) = ifa
                && ifa
                    .ifa_ifp
                    .get()
                    .is_some_and(|ifp| ifp.if_flags.get() & IFF_UP != 0)
            {
                // SAFETY: an `AF_INET6` source is a `sockaddr_in6`; read unaligned because
                // the generic structure has a smaller alignment.
                *in6src = unsafe { ptr::read_unaligned(satosin6_const(ip6_source)) }.sin6_addr;
                return Ok(());
            }
        }
    }

    let Some(ia6) = ia6 else {
        return Err(Errno::EHOSTUNREACH); // no route
    };

    *in6src = ia6_in6(ia6);
    Ok(())
}

/// `in6_selectsrc`: return an IPv6 address, which is the most appropriate for a given
/// destination and multicast options (stored in `in6src`). If necessary, this function
/// lookups the routing table and returns an entry to the caller for later use.
pub fn in6_selectsrc(
    in6src: &mut In6Addr,
    dstsock: &SockaddrIn6,
    mopts: Option<&Ip6Moptions>,
    rtableid: u32,
) -> Result<(), Errno> {
    let dst = &dstsock.sin6_addr;

    // If the destination address is a link-local unicast address or a link/interface-local
    // multicast address, and if the outgoing interface is specified by the sin6_scope_id
    // filed, use an address associated with the interface.
    // XXX: We're now trying to define more specific semantics of sin6_scope_id field, so this
    //      part will be rewritten in the near future.
    if (in6_is_addr_linklocal(dst)
        || in6_is_addr_mc_linklocal(dst)
        || in6_is_addr_mc_intfacelocal(dst))
        && dstsock.sin6_scope_id != 0
    {
        let Some(ifp) = if_get(dstsock.sin6_scope_id) else {
            return Err(Errno::ENXIO); // XXX: better error?
        };

        let ia6 = in6_ifawithscope(ifp, dst, rtableid, None);
        if_put(ifp);

        let Some(ia6) = ia6 else {
            return Err(Errno::EADDRNOTAVAIL);
        };

        *in6src = ia6_in6(ia6);
        return Ok(());
    }

    // If the destination address is a multicast address and the outgoing interface for the
    // address is specified by the caller, use an address associated with the interface. Even
    // if the outgoing interface is not specified, we also choose a loopback interface as the
    // outgoing interface.
    if in6_is_addr_multicast(dst) {
        let mut ifp = mopts.and_then(|m| if_get(u32::from(m.im6o_ifidx)));

        if ifp.is_none() && dstsock.sin6_scope_id != 0 {
            ifp = if_get(u32::from(htons(dstsock.sin6_scope_id as u16)));
        }

        if let Some(ifp) = ifp {
            let ia6 = in6_ifawithscope(ifp, dst, rtableid, None);
            if_put(ifp);

            let Some(ia6) = ia6 else {
                return Err(Errno::EADDRNOTAVAIL);
            };

            *in6src = ia6_in6(ia6);
            return Ok(());
        }
    }

    Err(Errno::EADDRNOTAVAIL)
}

/// `in6_selectroute`: the route to `dst`, from the cache `ro` if valid, else a newly
/// allocated one; `None` when there is none or it conflicts with the interface of the
/// packet info of `opts`.
pub fn in6_selectroute(
    dst: &In6Addr,
    opts: Option<&Ip6Pktopts>,
    ro: &Route,
    rtableid: u32,
) -> Option<&'static Rtentry> {
    // Use a cached route if it exists and is valid, else try to allocate a new one.
    let rt = route6_mpath(ro, dst, None, rtableid);

    // Check if the outgoing interface conflicts with the interface specified by ipi6_ifindex
    // (if specified). Note that loopback interface is always okay. (this may happen when we
    // are sending a packet to one of our own addresses.)
    if let Some(pi) = pktinfo(opts) {
        // SAFETY: the packet info is a plain structure owned by the socket, valid for the
        // call.
        let ifindex = unsafe { (*pi.as_ptr()).ipi6_ifindex };
        if ifindex != 0
            && let Some(rt) = rt
            && rt.rt_flags.get() & RTF_LOCAL == 0
            && rt.rt_ifidx.get() != ifindex
        {
            return None;
        }
    }

    rt
}

/// `in6_selectif`: the outgoing interface for `dst` (with a reference from `if_get`, which
/// the caller releases with `if_put`; the C's `*retifp`, which stays NULL, `Ok(None)`, when
/// the route's interface is gone).
fn in6_selectif(
    dst: &In6Addr,
    opts: Option<&Ip6Pktopts>,
    mopts: Option<&Ip6Moptions>,
    ro: &Route,
    rtableid: u32,
) -> Result<Option<&'static Ifnet>, Errno> {
    // If the caller specify the outgoing interface explicitly, use it.
    if let Some(pi) = pktinfo(opts) {
        // SAFETY: the packet info is a plain structure owned by the socket, valid for the
        // call.
        let ifindex = unsafe { (*pi.as_ptr()).ipi6_ifindex };
        if ifindex != 0
            && let Some(ifp) = if_get(ifindex)
        {
            return Ok(Some(ifp));
        }
    }

    // If the destination address is a multicast address and the outgoing interface for the
    // address is specified by the caller, use it.
    if in6_is_addr_multicast(dst)
        && let Some(mopts) = mopts
        && let Some(ifp) = if_get(u32::from(mopts.im6o_ifidx))
    {
        return Ok(Some(ifp));
    }

    let Some(rt) = in6_selectroute(dst, opts, ro, rtableid) else {
        return Err(Errno::EHOSTUNREACH);
    };

    // do not use a rejected or black hole route.
    // XXX: this check should be done in the L2 output routine. However, if we skipped this
    // check here, we'd see the following scenario:
    // - install a rejected route for a scoped address prefix (like fe80::/10)
    // - send a packet to a destination that matches the scoped prefix, with ambiguity about
    //   the scope zone.
    // - pick the outgoing interface from the route, and disambiguate the scope zone with the
    //   interface.
    // - ip6_output() would try to get another route with the "new" destination, which may be
    //   valid.
    // - we'd see no error on output.
    // Although this may not be very harmful, it should still be confusing. We thus reject the
    // case here.
    if rt.rt_flags.get() & (RTF_REJECT | RTF_BLACKHOLE) != 0 {
        return Err(if rt.rt_flags.get() & RTF_HOST != 0 {
            Errno::EHOSTUNREACH
        } else {
            Errno::ENETUNREACH
        });
    }

    Ok(if_get(rt.rt_ifidx.get()))
}

/// `in6_selecthlim`: the hop limit for packets of `inp`: its `inp_hops` if set, else
/// `ip6_defhlim`.
pub fn in6_selecthlim(inp: &Inpcb) -> i32 {
    if inp.inp_hops.get() >= 0 {
        return inp.inp_hops.get();
    }

    IP6_DEFHLIM.load(Ordering::Relaxed)
}

/// `in6_embedscope`: generate kernel-internal form (scopeid embedded into `s6_addr16[1]`) of
/// the address of `sin6` in `in6`. If the address scope of is link-local, embed the interface
/// index in the address. The routine determines our precedence between advanced API
/// scope/interface specification and basic API specification (the interface of the packet
/// info of `outputopts6`, the multicast interface of `moptions6`, `sin6_scope_id`).
///
/// This function should be nuked in the future, when we get rid of embedded scopeid thing.
///
/// XXX actually, it is over-specification to return ifp against sin6_scope_id. there can be
/// multiple interfaces that belong to a particular scope zone (in specification, we have 1:N
/// mapping between a scope zone and interfaces). we may want to change the function to return
/// something other than ifp.
pub fn in6_embedscope(
    in6: &mut In6Addr,
    sin6: &SockaddrIn6,
    outputopts6: Option<&Ip6Pktopts>,
    moptions6: Option<&Ip6Moptions>,
) -> Result<(), Errno> {
    *in6 = sin6.sin6_addr;

    // don't try to read sin6->sin6_addr beyond here, since the caller may ask us to overwrite
    // existing sockaddr_in6

    if in6_is_scope_embed(in6) {
        // KAME assumption: link id == interface id

        // SAFETY: the packet info is a plain structure owned by the socket, valid for the
        // call.
        let pi_ifindex =
            pktinfo(outputopts6).map_or(0, |pi| unsafe { (*pi.as_ptr()).ipi6_ifindex });
        let scopeid = if pi_ifindex != 0 {
            pi_ifindex
        } else if let Some(m) = moptions6
            && in6_is_addr_multicast(in6)
            && m.im6o_ifidx != 0
        {
            u32::from(m.im6o_ifidx)
        } else {
            sin6.sin6_scope_id
        };

        if scopeid != 0 {
            let Some(ifp) = if_get(scopeid) else {
                return Err(Errno::ENXIO); // XXX EINVAL?
            };
            // XXX assignment to 16bit from 32bit variable
            in6.set_s6_addr16(1, htons((scopeid & 0xffff) as u16));
            if_put(ifp);
        }
    }

    Ok(())
}

/// `in6_recoverscope`: generate standard `sockaddr_in6` from embedded form; touches
/// `sin6_addr` and `sin6_scope_id` only.
///
/// This function should be nuked in the future, when we get rid of embedded scopeid thing.
pub fn in6_recoverscope(sin6: &mut SockaddrIn6, in6: &In6Addr) {
    sin6.sin6_addr = *in6;

    // don't try to read *in6 beyond here, since the caller may ask us to overwrite existing
    // sockaddr_in6

    sin6.sin6_scope_id = 0;
    if in6_is_scope_embed(in6) {
        // KAME assumption: link id == interface id
        let scopeid = u32::from(ntohs(sin6.sin6_addr.s6_addr16(1)));
        if scopeid != 0 {
            sin6.sin6_addr.set_s6_addr16(1, 0);
            sin6.sin6_scope_id = scopeid;
        }
    }
}

/// `in6_clearscope`: just clear the embedded scope identifier.
pub fn in6_clearscope(addr: &mut In6Addr) {
    if in6_is_scope_embed(addr) {
        addr.set_s6_addr16(1, 0);
    }
}

#[cfg(test)]
mod tests;
