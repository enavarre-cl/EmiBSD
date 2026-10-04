/*	$OpenBSD: in6_ifattach.h,v 1.10 2019/08/21 15:32:18 florian Exp $	*/
/*	$KAME: in6_ifattach.h,v 1.9 2000/04/12 05:35:48 itojun Exp $	*/
/*	$OpenBSD: in6_ifattach.c,v 1.128 2026/09/20 20:50:29 gnezdo Exp $	*/
/*	$KAME: in6_ifattach.c,v 1.124 2001/07/18 08:32:51 jinmei Exp $	*/
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

//! Attaching IPv6 to an interface: the interface identifier, the link-local and
//! loopback addresses: `<netinet6/in6_ifattach.h>` and `netinet6/in6_ifattach.c`.
//!
//! Upstream: sys/netinet6/in6_ifattach.c @ 3ce1f3f79392
//! Upstream: sys/netinet6/in6_ifattach.h @ 3ce1f3f79392
//!
//! The header only declares `in6_ifattach`, `in6_ifdetach` and `in6_ifattach_linklocal`;
//! they are defined here. This version of `in6_ifattach.c` has no stable-privacy (RFC 7217)
//! identifiers and no node-information group (`in6_get_soii_ifid`, `in6_nigroup`): the
//! interface identifier is the EUI-64 of a hardware address (`in6_get_hw_ifid`), borrowed
//! from another interface, or random (`in6_get_rand_ifid`).
//!
//! ## Deviations
//! - `in6_get_hw_ifid` returns `bool` (true: an identifier was made; the C's 0 / -1) and
//!   `in6_get_ifid` / `in6_get_rand_ifid` are public so the tests reach them.
//! - `ifra_name` is a copy of `if_xname` (both `IFNAMSIZ` bytes) instead of `strlcpy`.
//! - `ip6_mrouter_detach` (`MROUTING`) is not configured: a comment in `in6_ifdetach`.
//! - `in6_ifattach_linklocal` panics where the C would dereference a NULL address (the
//!   link-local address `in6_update_ifa` just configured is not there).

use core::mem::size_of;
use core::ptr;
use core::slice;

use crate::dev::rnd::arc4random_buf;
use crate::kern::subr_prf::panic;
use crate::net::if_::{
    IFF_LOOPBACK, IFF_MULTICAST, IFF_POINTOPOINT, IFNETLIST, IFXF_AUTOCONF6, IFXF_AUTOCONF6TEMP,
    if_addrhooks_run,
};
use crate::net::if_dl::lladdr;
use crate::net::if_types::{
    IFT_BRIDGE, IFT_CARP, IFT_ENC, IFT_ETHER, IFT_GIF, IFT_IEEE1394, IFT_IEEE80211, IFT_PFLOG,
    IFT_PFSYNC, IFT_WIREGUARD,
};
use crate::net::if_var::Ifnet;
use crate::net::route::{
    RTF_CLONING, RTF_CONNECTED, RTF_MPATH, rt_ifa_add, rtalloc, rtdeletemsg, rtfree,
};
use crate::net::rtable::rtable_loindex;
use crate::netinet::ip6::IPV6_MMTU;
use crate::netinet6::in6::{
    IN6ADDR_INTFACELOCAL_ALLNODES, IN6ADDR_LINKLOCAL_ALLNODES, IN6ADDR_LOOPBACK, IN6MASK64,
    IN6MASK128, In6Addr, SockaddrIn6, in6_purgeaddr, in6_update_ifa, in6if_do_dad,
    in6ifa_ifpforlinklocal, in6ifa_ifpwithaddr, sin6tosa,
};
use crate::netinet6::in6_var::{IN6_IFF_TENTATIVE, In6Aliasreq};
use crate::netinet6::nd6::{ND6_INFINITE_LIFETIME, nd6_need_cache, nd6_purge};
use crate::netinet6::nd6_nbr::nd6_dad_start;
use crate::sys::endian::{htonl, htons};
use crate::sys::errno::Errno;
use crate::sys::socket::AF_INET6;

/// `EUI64_GBIT`: the group bit of an EUI-64.
const EUI64_GBIT: u8 = 0x01;
/// `EUI64_UBIT`: the universal/local bit of an EUI-64.
const EUI64_UBIT: u8 = 0x02;

/// `EUI64_TO_IFID(in6)`: converts the EUI-64 in the low half of `in6` into an IPv6
/// interface identifier (flips the universal/local bit).
fn eui64_to_ifid(in6: &mut In6Addr) {
    in6.s6_addr[8] ^= EUI64_UBIT;
}

/// `EUI64_GROUP(in6)`: the group bit of the EUI-64 in `in6`.
fn eui64_group(in6: &In6Addr) -> bool {
    in6.s6_addr[8] & EUI64_GBIT != 0
}

/// `in6_get_rand_ifid`: generate a random interface identifier; the upper 64 bits of `in6`
/// are preserved.
pub fn in6_get_rand_ifid(ifp: &Ifnet, in6: &mut In6Addr) {
    let _ = ifp;
    arc4random_buf(&mut in6.s6_addr[8..16]);

    // make sure to set "u" bit to local, and "g" bit to individual.
    in6.s6_addr[8] &= !EUI64_GBIT; // g bit to "individual"
    in6.s6_addr[8] |= EUI64_UBIT; // u bit to "local"

    // convert EUI64 into IPv6 interface identifier
    eui64_to_ifid(in6);
}

/// `in6_get_hw_ifid`: get the interface identifier of the interface from its hardware
/// address; the upper 64 bits of `in6` are preserved. `false` (the C's -1) when the
/// interface has no usable one.
pub fn in6_get_hw_ifid(ifp: &Ifnet, in6: &mut In6Addr) -> bool {
    const ALLZERO: [u8; 8] = [0; 8];
    const ALLONE: [u8; 8] = [0xff; 8];

    let sdl = ifp.if_sadl.get();
    // SAFETY: a non-NULL `if_sadl` is the interface's link-level address.
    if sdl.is_null() || unsafe { (*sdl).sdl_alen } == 0 {
        return false;
    }

    // SAFETY: `sdl` is the interface's link-level address, as above.
    let mut addrlen = usize::from(unsafe { (*sdl).sdl_alen });
    // SAFETY: `sdl` is readable and the link-level address, `sdl_alen` bytes after the name
    // inside the allocation `if_alloc_sadl` made, is readable for its length.
    let all = unsafe { slice::from_raw_parts(lladdr(sdl), addrlen) };

    if matches!(ifp.if_type.get(), IFT_IEEE1394 | IFT_IEEE80211) {
        // IEEE1394 uses 16byte length address starting with EUI64
        addrlen = addrlen.min(8);
    }
    let addr = &all[..addrlen];

    // get EUI64
    match ifp.if_type.get() {
        // IEEE802/EUI64 cases - what others?
        IFT_ETHER | IFT_CARP | IFT_IEEE1394 | IFT_IEEE80211 => {
            // look at IEEE802/EUI64 only
            if addrlen != 8 && addrlen != 6 {
                return false;
            }

            // check for invalid MAC address - on bsdi, we see it a lot since wildboar
            // configures all-zero MAC on pccard before card insertion.
            if addr == &ALLZERO[..addrlen] {
                return false;
            }
            if addr == &ALLONE[..addrlen] {
                return false;
            }

            // make EUI64 address
            if addrlen == 8 {
                in6.s6_addr[8..16].copy_from_slice(addr);
            } else if addrlen == 6 {
                in6.s6_addr[8] = addr[0];
                in6.s6_addr[9] = addr[1];
                in6.s6_addr[10] = addr[2];
                in6.s6_addr[11] = 0xff;
                in6.s6_addr[12] = 0xfe;
                in6.s6_addr[13] = addr[3];
                in6.s6_addr[14] = addr[4];
                in6.s6_addr[15] = addr[5];
            }
        }

        IFT_GIF => {
            // RFC2893 says: "SHOULD use IPv4 address as ifid source". however, IPv4 address
            // is not very suitable as unique identifier source (can be renumbered). we
            // don't do this.
            return false;
        }

        _ => return false,
    }

    // sanity check: g bit must not indicate "group"
    if eui64_group(in6) {
        return false;
    }

    // convert EUI64 into IPv6 interface identifier
    eui64_to_ifid(in6);

    // sanity check: ifid must not be all zero, avoid conflict with subnet router anycast
    if in6.s6_addr[8] & !(EUI64_GBIT | EUI64_UBIT) == 0x00 && in6.s6_addr[9..16] == ALLZERO[..7] {
        return false;
    }

    true
}

/// `in6_get_ifid`: get interface identifier for the specified interface. If it is not
/// available on `ifp0`, borrow interface identifier from other information sources.
pub fn in6_get_ifid(ifp0: &Ifnet, in6: &mut In6Addr) {
    // first, try to get it from the interface itself
    if in6_get_hw_ifid(ifp0, in6) {
        return;
    }

    crate::sys::systm::net_assert_locked("in6_get_ifid");

    // next, try to get it from some other hardware interface
    for ifp in IFNETLIST.0.iter() {
        if ptr::eq(ifp, ifp0) {
            continue;
        }
        if in6_get_hw_ifid(ifp, in6) {
            return;
        }
    }

    // last resort: get from random number source
    in6_get_rand_ifid(ifp0, in6);
}

/// `in6_ifattach_linklocal`: configures the link-local address of `ifp`. `ifid` is used as
/// the EUI-64 if given, overriding the other EUI-64 sources (`None`: derived from the
/// hardware address, borrowed or random).
pub fn in6_ifattach_linklocal(ifp: &'static Ifnet, ifid: Option<&In6Addr>) -> Result<(), Errno> {
    let mut ifra = In6Aliasreq::zeroed();

    crate::sys::systm::net_assert_locked("in6_ifattach_linklocal");

    // configure link-local address.
    ifra.ifra_name = ifp.if_xname.get();
    let ifidx = ifp.if_index.get() as u16;
    {
        let a = ifra.ifra_addr_mut();
        a.sin6_family = AF_INET6;
        a.sin6_len = size_of::<SockaddrIn6>() as u8;
        a.sin6_addr.set_s6_addr16(0, htons(0xfe80));
        a.sin6_addr.set_s6_addr16(1, htons(ifidx));
        a.sin6_addr.set_s6_addr32(1, 0);
        if ifp.if_flags.get() & IFF_LOOPBACK != 0 {
            a.sin6_addr.set_s6_addr32(2, 0);
            a.sin6_addr.set_s6_addr32(3, htonl(1));
        } else if let Some(ifid) = ifid {
            a.sin6_addr = *ifid;
            a.sin6_addr.set_s6_addr16(0, htons(0xfe80));
            a.sin6_addr.set_s6_addr16(1, htons(ifidx));
            a.sin6_addr.set_s6_addr32(1, 0);

            // RFC5072: Use negotiated P2P ifid as-is.
            if ifp.if_flags.get() & IFF_POINTOPOINT == 0 {
                a.sin6_addr.s6_addr[8] &= !EUI64_GBIT;
                a.sin6_addr.s6_addr[8] |= EUI64_UBIT;
            }
        } else {
            in6_get_ifid(ifp, &mut a.sin6_addr);
        }
    }

    ifra.ifra_prefixmask.sin6_len = size_of::<SockaddrIn6>() as u8;
    ifra.ifra_prefixmask.sin6_family = AF_INET6;
    ifra.ifra_prefixmask.sin6_addr = IN6MASK64;
    // link-local addresses should NEVER expire.
    ifra.ifra_lifetime.ia6t_vltime = ND6_INFINITE_LIFETIME;
    ifra.ifra_lifetime.ia6t_pltime = ND6_INFINITE_LIFETIME;

    // XXX: Some P2P interfaces seem not to send packets just after becoming up, so we skip
    // p2p interfaces for safety.
    if in6if_do_dad(ifp) && ifp.if_flags.get() & IFF_POINTOPOINT == 0 {
        ifra.ifra_flags |= IN6_IFF_TENTATIVE;
    }

    in6_update_ifa(ifp, &ifra, in6ifa_ifpforlinklocal(ifp, 0))?;

    let Some(ia6) = in6ifa_ifpforlinklocal(ifp, 0) else {
        panic(format_args!(
            "in6_ifattach_linklocal: no link-local address after in6_update_ifa"
        ));
    };

    // Perform DAD, if needed.
    if ia6.ia6_flags.get() & IN6_IFF_TENTATIVE != 0 {
        nd6_dad_start(&ia6.ia_ifa);
    }

    if ifp.if_flags.get() & IFF_LOOPBACK != 0 {
        if_addrhooks_run(ifp);
        return Ok(()); // No need to install a connected route.
    }

    let mut flags = RTF_CONNECTED | RTF_MPATH;
    if ifp.if_flags.get() & IFF_POINTOPOINT == 0 {
        flags |= RTF_CLONING;
    }

    // SAFETY: the address's own socket address.
    let error = unsafe {
        rt_ifa_add(
            &ia6.ia_ifa,
            flags,
            ia6.ia_ifa.ifa_addr.get(),
            ifp.if_rdomain.get(),
        )
    };
    if let Err(e) = error {
        in6_purgeaddr(&ia6.ia_ifa);
        return Err(e);
    }
    if_addrhooks_run(ifp);

    Ok(())
}

/// `in6_ifattach_loopback`: assigns `::1` to a loopback interface.
fn in6_ifattach_loopback(ifp: &'static Ifnet) -> Result<(), Errno> {
    let in6 = IN6ADDR_LOOPBACK;
    let mut ifra = In6Aliasreq::zeroed();

    crate::kassert!(ifp.if_flags.get() & IFF_LOOPBACK != 0);

    if in6ifa_ifpwithaddr(ifp, &in6).is_some() {
        return Ok(());
    }

    ifra.ifra_name = ifp.if_xname.get();
    ifra.ifra_prefixmask.sin6_len = size_of::<SockaddrIn6>() as u8;
    ifra.ifra_prefixmask.sin6_family = AF_INET6;
    ifra.ifra_prefixmask.sin6_addr = IN6MASK128;

    // Always initialize ia_dstaddr (= broadcast address) to loopback address. Follows IPv4
    // practice - see in_ifinit().
    ifra.ifra_dstaddr.sin6_len = size_of::<SockaddrIn6>() as u8;
    ifra.ifra_dstaddr.sin6_family = AF_INET6;
    ifra.ifra_dstaddr.sin6_addr = IN6ADDR_LOOPBACK;

    let a = ifra.ifra_addr_mut();
    a.sin6_len = size_of::<SockaddrIn6>() as u8;
    a.sin6_family = AF_INET6;
    a.sin6_addr = IN6ADDR_LOOPBACK;

    // the loopback  address should NEVER expire.
    ifra.ifra_lifetime.ia6t_vltime = ND6_INFINITE_LIFETIME;
    ifra.ifra_lifetime.ia6t_pltime = ND6_INFINITE_LIFETIME;

    // We are sure that this is a newly assigned address, so we can set NULL to the 3rd arg.
    in6_update_ifa(ifp, &ifra, None)
}

/// `in6_ifattach`: attaches IPv6 to `ifp`: the loopback address on the default loopback
/// interface of its routing domain and the link-local address.
///
/// XXX multiple loopback interface needs more care. for instance, nodelocal address needs to
/// be configured onto only one of them. XXX multiple link-local address case.
pub fn in6_ifattach(ifp: &'static Ifnet) -> Result<(), Errno> {
    // some of the interfaces are inherently not IPv6 capable
    match ifp.if_type.get() {
        IFT_BRIDGE | IFT_ENC | IFT_PFLOG | IFT_PFSYNC => return Ok(()),
        _ => {}
    }

    // if link mtu is too small, don't try to configure IPv6. remember there could be some
    // link-layer that has special fragmentation logic.
    if ifp.if_mtu.get() < IPV6_MMTU {
        return Err(Errno::EINVAL);
    }

    if nd6_need_cache(ifp) && ifp.if_flags.get() & IFF_MULTICAST == 0 {
        return Err(Errno::EINVAL);
    }

    // Assign loopback address if this lo(4) interface is the default for its rdomain.
    if ifp.if_flags.get() & IFF_LOOPBACK != 0
        && ifp.if_index.get() == rtable_loindex(ifp.if_rdomain.get())
    {
        in6_ifattach_loopback(ifp)?;
    }

    if ifp.if_type.get() == IFT_WIREGUARD {
        return Ok(());
    }

    // Assign a link-local address, if there's none.
    if in6ifa_ifpforlinklocal(ifp, 0).is_none() && in6_ifattach_linklocal(ifp, None).is_err() {
        // failed to assign linklocal address. bark?
    }

    Ok(())
}

/// Removes the route to the all-nodes group `addr` (with the interface index embedded) if
/// it belongs to `ifp`: the two identical blocks of `in6_ifdetach`.
fn in6_ifdetach_route(ifp: &'static Ifnet, allnodes: &In6Addr) {
    let mut sin6 = SockaddrIn6::zeroed();
    sin6.sin6_len = size_of::<SockaddrIn6>() as u8;
    sin6.sin6_family = AF_INET6;
    sin6.sin6_addr = *allnodes;
    sin6.sin6_addr
        .set_s6_addr16(1, htons(ifp.if_index.get() as u16));
    // SAFETY: `sin6` is a socket address of its `sin6_len` bytes, live for the call.
    let rt = unsafe { rtalloc(sin6tosa(&mut sin6), 0, ifp.if_rdomain.get()) };
    if let Some(rt) = rt
        && rt.rt_ifidx.get() == ifp.if_index.get()
    {
        let _ = rtdeletemsg(rt, ifp, ifp.if_rdomain.get());
    }
    rtfree(rt);
}

/// `in6_ifdetach`: removes every IPv6 address, route and membership of `ifp`.
///
/// NOTE: `in6_ifdetach()` does not support loopback if at this moment.
pub fn in6_ifdetach(ifp: &'static Ifnet) {
    // MROUTING: ip6_mrouter_detach(ifp) removes the ip6_mrouter stuff; not configured.

    // nuke any of IPv6 addresses we have
    for ifa in ifp.if_addrlist.iter() {
        // SAFETY: an interface address's `ifa_addr` is readable (`ifa_add`'s contract).
        if unsafe { (*ifa.ifa_addr.get()).sa_family } != AF_INET6 {
            continue;
        }
        // SAFETY: an address on the list lives until `ifa_del` and its last `ifafree`; the
        // list's iterator has already read the next entry (`TAILQ_FOREACH_SAFE`).
        in6_purgeaddr(unsafe { &*ptr::from_ref(ifa) });
        if_addrhooks_run(ifp);
    }

    // Remove neighbor management table. Must be called after purging addresses.
    nd6_purge(ifp);

    // remove route to interface local allnodes multicast (ff01::1)
    in6_ifdetach_route(ifp, &IN6ADDR_INTFACELOCAL_ALLNODES);

    // remove route to link-local allnodes multicast (ff02::1)
    in6_ifdetach_route(ifp, &IN6ADDR_LINKLOCAL_ALLNODES);

    if ifp.if_xflags.get() & (IFXF_AUTOCONF6 | IFXF_AUTOCONF6TEMP) != 0 {
        ifp.if_xflags
            .set(ifp.if_xflags.get() & !(IFXF_AUTOCONF6 | IFXF_AUTOCONF6TEMP));
    }
}

#[cfg(test)]
mod tests;
