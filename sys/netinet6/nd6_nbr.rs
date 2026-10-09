/*	$KAME: nd6_nbr.c,v 1.61 2001/02/10 16:06:14 jinmei Exp $	*/
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
/* </LICENSES> */

/* <CODE> */
//! Neighbor solicitations and advertisements, and Duplicate Address Detection:
//! `netinet6/nd6_nbr.c`.
//!
//! Upstream: sys/netinet6/nd6_nbr.c @ 3ce1f3f79392
//!
//! `nd6_ns_input` answers a solicitation for one of our addresses (or one we proxy) with an
//! advertisement and learns the solicitor's link-layer address; `nd6_na_input` completes
//! the neighbor cache entry `nd6_resolve` started (INCOMPLETE to REACHABLE or STALE) or
//! updates an existing one by the override/solicited rules of RFC 2461 7.2.5.
//! `nd6_ns_output`/`nd6_na_output` build those messages. Duplicate Address Detection
//! (RFC 2462) keeps a `dadq` per tentative address: `ip6_dad_count` solicitations from
//! `::`, one per `RETRANS_TIMER`; a solicitation or advertisement for the address from
//! someone else marks it duplicated, silence makes it usable.
//!
//! ## Deviations
//! - `struct dadq` is [`Dadq`] with `Cell` counters (net lock); its list holds `malloc`ed
//!   entries freed by `nd6_dad_reaper` as in C.
//! - The ND messages are read out of the mbuf and written into it as copies
//!   (`read_unaligned`/`write_unaligned`): packet data has no alignment guarantee. The
//!   input functions also refuse (`icp6s_tooshort`) a message shorter than its fixed part,
//!   which `icmp6_input` already guarantees; the C would read past it.
//! - The cache update half of `nd6_na_input` (from the lookup of an existing entry on) is
//!   the crate-visible `nd6_na_cache`, so host tests can run it on an entry without an inet6
//!   routing table.
//! - `nd6_ifptomac` returns the Ethernet address by value; the C returns `ifp + 1`, the
//!   `struct arpcom` address, for every type in its list: here an interface of those types
//!   that `ether_ifattach` did not attach has none. `nd6_na_output`'s `sdl0` is a
//!   `sockaddr_dl` (the C's `struct sockaddr *` checked for `AF_LINK`), and `nd6_ns_input`
//!   passes a copy of the proxy route's gateway (the C keeps the pointer after `rtfree`).
//! - `ifp->if_nd->reachable` falls back to `REACHABLE_TIME` for an interface without ND
//!   information (the C reads it unconditionally).
//! - `NCARP` (`carp_iamatch`) is not configured: each site is a comment. `DIAGNOSTIC`'s
//!   `MCLBYTES` check is under the `diagnostic` feature. `inet_ntop` is `nd6::In6Ntop`.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_timeout::{timeout_add, timeout_add_msec, timeout_del, timeout_set_proc};
use crate::kern::subr_prf::{Str, panic};
use crate::kern::uipc_mbuf::{MAX_LINKHDR, m_align, m_free, m_freem, m_gethdr};
use crate::log;
use crate::net::if_::{
    IFF_RUNNING, IFF_UP, IFNAMSIZ, if_get, if_isconnected, if_output_mq, if_put,
};
use crate::net::if_dl::{SockaddrDl, lladdr, satosdl};
use crate::net::if_types::{IFT_CARP, IFT_ETHER, IFT_IEEE1394, IFT_IEEE80211, IFT_PROPVIRTUAL};
use crate::net::if_var::{Ifaddr, Ifnet};
use crate::net::route::{
    RT_RESOLVE, RTF_ANNOUNCE, RTF_CLONED, RTF_CLONING, RTF_LLINFO, RTF_REJECT, RTM_CHGADDRATTR,
    RTM_RESOLVE, Rtentry, ifafree, ifaref, rtalloc, rtfree, rtisvalid,
};
use crate::net::rtable::rt_key;
use crate::net::rtsock::{rtm_addr, rtm_send};
use crate::netinet::icmp6::{
    Icmp6Hdr, Icmp6statCounters, ND_NA_FLAG_OVERRIDE, ND_NA_FLAG_ROUTER, ND_NA_FLAG_SOLICITED,
    ND_NEIGHBOR_ADVERT, ND_NEIGHBOR_SOLICIT, ND_OPT_SOURCE_LINKADDR, ND_OPT_TARGET_LINKADDR,
    NdNeighborAdvert, NdNeighborSolicit, NdOptHdr, icmp6stat_inc, icmp6stat_inc_hist,
};
use crate::netinet::if_ether::{ETHER_ADDR_LEN, arpcom_of};
use crate::netinet::in_::IPPROTO_ICMPV6;
use crate::netinet::ip6::{IPV6_VERSION, IPV6_VERSION_MASK, Ip6Hdr, ip6_exthdr_get};
use crate::netinet6::in6::{
    __IPV6_ADDR_INT16_MLL, __IPV6_ADDR_INT32_ONE, IN6ADDR_LINKLOCAL_ALLNODES,
    IN6ADDR_LINKLOCAL_ALLROUTERS, In6Addr, SockaddrIn6, ifatoia6, in6_are_addr_equal,
    in6_is_addr_multicast, in6_is_addr_unspecified, in6_is_scope_embed, in6ifa_ifpforlinklocal,
    in6ifa_ifpwithaddr, sin6tosa_const,
};
use crate::netinet6::in6_proto::{IP6_DAD_COUNT, IP6_DAD_PENDING, IP6_FORWARDING};
use crate::netinet6::in6_var::{IN6_IFF_ANYCAST, IN6_IFF_DUPLICATED, IN6_IFF_TENTATIVE, ifa_in6};
use crate::netinet6::ip6_output::ip6_output;
use crate::netinet6::ip6_var::{IPV6_UNSPECSRC, Ip6Moptions, mtod_ip6, mtod_ip6_store};
use crate::netinet6::nd6::{
    In6Ntop, ND6_GCTIMER, ND6_LLINFO_INCOMPLETE, ND6_LLINFO_REACHABLE, ND6_LLINFO_STALE, NdOpts,
    REACHABLE_TIME, RETRANS_TIMER, if_nd, ln_hold_total, nd6_cache_lladdr, nd6_ether_sprintf,
    nd6_llchanged, nd6_llinfo_permanent, nd6_llinfo_settimer, nd6_llstore, nd6_lookup,
    nd6_opt_lladdr, nd6_options, rt_ln,
};
use crate::netinet6::nd6_rtr::rt6_flush;
use crate::queue_adapter;
use crate::sys::endian::htons;
use crate::sys::malloc::{M_IP6NDP, M_NOWAIT, M_ZERO};
use crate::sys::mbuf::{
    M_DONTWAIT, M_EXT, M_ICMP_CSUM_OUT, M_MCAST, MHLEN, MT_DATA, Mbuf, mclget, mtod,
};
use crate::sys::queue::{ListHead, TailqEntry, TailqHead};
use crate::sys::socket::AF_LINK;
use crate::sys::syslog::{LOG_ERR, LOG_INFO};
use crate::sys::systm::{net_assert_locked, net_assert_locked_exclusive, net_lock, net_unlock};
use crate::sys::timeout::Timeout;

/// `struct dadq`: an address to run DAD on. Locks: \[N\] net lock.
pub struct Dadq {
    /// \[N\] `dad_list`: the `dadq` list.
    dad_list: TailqEntry<Dadq>,
    /// `dad_ifa`: the address, referenced.
    dad_ifa: &'static Ifaddr,
    /// \[N\] `dad_count`: max NS to send.
    dad_count: Cell<i32>,
    /// \[N\] `dad_ns_tcount`: # of trials to send NS.
    dad_ns_tcount: Cell<i32>,
    /// \[N\] `dad_ns_ocount`: NS sent so far.
    dad_ns_ocount: Cell<i32>,
    /// \[N\] `dad_ns_icount`: NS received for the address.
    dad_ns_icount: Cell<i32>,
    /// \[N\] `dad_na_icount`: NA received for the address.
    dad_na_icount: Cell<i32>,
    /// `dad_timer_ch`: the retransmission timer, then the reaper.
    dad_timer_ch: Timeout,
}

queue_adapter!(
    /// `TAILQ_HEAD(, dadq)` through `dad_list`.
    DadqList: Dadq, dad_list => TailqEntry<Dadq>
);

/// `dadq`, made `Sync`: changed and walked under the net lock.
struct DadqHead(TailqHead<DadqList>);

// SAFETY: see the type's doc.
unsafe impl Sync for DadqHead {}

/// \[N\] `dadq`: list of addresses to run DAD on.
static DADQ: DadqHead = DadqHead(TailqHead::new());

/// The interface name the C prints for an address without an interface.
const UNKNOWN_IFNAME: [u8; IFNAMSIZ] = *b"???\0\0\0\0\0\0\0\0\0\0\0\0\0";

/// `dad_maxtry`: max # of *tries* to transmit DAD packet.
static DAD_MAXTRY: AtomicI32 = AtomicI32::new(15);

/// The ND message of type `T` at the start of the `len` contiguous bytes at `p`, and the
/// options after it.
///
/// # Safety
///
/// `p` points at `len` readable bytes (what `ip6_exthdr_get` returned), `len` is at least
/// `size_of::<T>()`, and `T` is a `#[repr(C)]` structure of integers.
unsafe fn nd6_msg<'a, T>(p: *mut u8, len: usize) -> (T, &'a [u8]) {
    // SAFETY: the caller's contract.
    unsafe {
        (
            ptr::read_unaligned(p.cast::<T>()),
            core::slice::from_raw_parts(p.add(size_of::<T>()), len - size_of::<T>()),
        )
    }
}

/// The link-layer address option `o` carries (`(char *)(o + 1)` and `o->nd_opt_len << 3`).
///
/// # Safety
///
/// `o` is `None` or an option `nd6_options` found in a message that stays alive and
/// unchanged while the address is used.
unsafe fn nd6_lladdr_of<'a>(o: Option<NonNull<NdOptHdr>>) -> (Option<&'a [u8]>, i32) {
    match o {
        Some(o) => {
            // SAFETY: the caller's contract.
            let (l, n) = unsafe { nd6_opt_lladdr(o) };
            (Some(l), n)
        }
        None => (None, 0),
    }
}

/// `ifp->if_nd->reachable`.
fn nd6_reachable(ifp: &Ifnet) -> u32 {
    if_nd(ifp).map_or(REACHABLE_TIME / 1000, |nd| nd.get().reachable)
}

/// `nd6_ns_input`: input a Neighbor Solicitation Message (`icmp6len` bytes at `off` of
/// `m`, consumed). Based on RFC 2461 and RFC 2462 (duplicated address detection).
pub fn nd6_ns_input(m: &'static Mbuf, off: i32, icmp6len: i32) {
    let ip6 = mtod_ip6(m);
    let mut saddr6 = ip6.ip6_src;
    let daddr6 = ip6.ip6_dst;
    let mut i_am_router = IP6_FORWARDING.load(Ordering::Relaxed) != 0;

    let Some(ifp) = if_get(m.m_pkthdr().ph_ifidx.get()) else {
        m_freem(m);
        return;
    };

    let mut mp = Some(m);
    let nd_ns = if (icmp6len as usize) < size_of::<NdNeighborSolicit>() {
        m_freem(m);
        None
    } else {
        ip6_exthdr_get(&mut mp, off, icmp6len)
    };
    let (Some(p), Some(m)) = (nd_ns, mp.filter(|_| nd_ns.is_some())) else {
        icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
        if_put(ifp);
        return;
    };
    let ip6 = mtod_ip6(m); // adjust pointer for safety
    // SAFETY: `ip6_exthdr_get` made `icmp6len` bytes contiguous at `p` (at least a
    // `nd_neighbor_solicit`, checked above); they live with `m`, freed below.
    let (nd_ns, opts) = unsafe { nd6_msg::<NdNeighborSolicit>(p, icmp6len as usize) };
    let mut taddr6 = nd_ns.nd_ns_target;

    let bad = 'out: {
        if ip6.ip6_hlim != 255 {
            break 'out true;
        }

        if in6_is_addr_unspecified(&saddr6) {
            // dst has to be solicited node multicast address.
            // don't check ifindex portion
            if !(daddr6.s6_addr16(0) == __IPV6_ADDR_INT16_MLL
                && daddr6.s6_addr32(1) == 0
                && daddr6.s6_addr32(2) == __IPV6_ADDR_INT32_ONE
                && daddr6.s6_addr8(12) == 0xff)
            {
                break 'out true;
            }
        } else {
            // Make sure the source address is from a neighbor's address.
            if !nd6_isneighbor(ifp, &saddr6) {
                break 'out true;
            }
        }

        if in6_is_addr_multicast(&taddr6) {
            break 'out true;
        }

        if in6_is_scope_embed(&taddr6) {
            taddr6.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
        }

        let mut ndopts = NdOpts::default();
        if !nd6_options(opts, &mut ndopts) {
            // nd6_options have incremented stats
            break 'out false;
        }

        // SAFETY: the option is inside the message, which lives until `m` is freed below.
        let (lladdr, lladdrlen) = unsafe { nd6_lladdr_of(ndopts.nd_opts_src_lladdr) };

        if in6_is_addr_unspecified(&ip6.ip6_src) && lladdr.is_some() {
            break 'out true;
        }

        // Attaching target link-layer address to the NA? (RFC 2461 7.2.4)
        //
        // NS IP dst is unicast/anycast			MUST NOT add
        // NS IP dst is solicited-node multicast	MUST add
        //
        // In implementation, we add target link-layer address by default. We do not add one
        // in MUST NOT cases.
        // (#if 0 in C, too much!: no tlladdr when the destination is our anycast address.)
        let tlladdr = in6_is_addr_multicast(&daddr6);

        // Target address (taddr6) must be either:
        // (1) Valid unicast/anycast address for my receiving interface,
        // (2) Unicast address for which I'm offering proxy service, or
        // (3) "tentative" address on which DAD is being performed.
        // (1) and (3) check.
        let mut ifa = in6ifa_ifpwithaddr(ifp, &taddr6).map(|ia| &ia.ia_ifa);
        // NCARP > 0: a carp interface that is not the master ignores it; not configured.

        // (2) check.
        let mut proxy = false;
        let mut proxydl: Option<SockaddrDl> = None;
        if ifa.is_none() {
            let tsin6 = SockaddrIn6::with_addr(taddr6);

            // SAFETY: a local `sockaddr_in6`.
            let rt = unsafe { rtalloc(sin6tosa_const(&tsin6), 0, m.m_pkthdr().ph_rtableid.get()) };
            if let Some(r) = rt
                && r.rt_flags.get() & RTF_ANNOUNCE != 0
                // SAFETY: a route's gateway is a readable socket address.
                && unsafe { (*r.rt_gateway.get()).sa_family } == AF_LINK
            {
                // proxy NDP for single entry
                ifa = in6ifa_ifpforlinklocal(
                    ifp,
                    IN6_IFF_TENTATIVE | IN6_IFF_DUPLICATED | IN6_IFF_ANYCAST,
                )
                .map(|ia| &ia.ia_ifa);
                if ifa.is_some() {
                    proxy = true;
                    // SAFETY: an `AF_LINK` gateway is a `sockaddr_dl`.
                    proxydl = Some(unsafe { *satosdl(r.rt_gateway.get()) });
                    i_am_router = false; // XXX
                }
            }
            rtfree(rt);
        }
        let Some(ifa) = ifa else {
            // We've got an NS packet, and we don't have that address assigned for us. We
            // MUST silently ignore it. See RFC2461 7.2.3.
            break 'out false;
        };
        let myaddr6 = ifa_in6(ifa);
        let ia6 = ifatoia6(ifa);
        let anycast = ia6.ia6_flags.get() & IN6_IFF_ANYCAST != 0;
        let tentative = ia6.ia6_flags.get() & IN6_IFF_TENTATIVE != 0;
        if ia6.ia6_flags.get() & IN6_IFF_DUPLICATED != 0 {
            break 'out false;
        }

        if lladdr.is_some() && ((i32::from(ifp.if_addrlen.get()) + 2 + 7) & !7) != lladdrlen {
            break 'out true;
        }

        if in6_are_addr_equal(&myaddr6, &saddr6) {
            log!(
                LOG_INFO,
                "nd6_ns_input: duplicate IP6 address {}\n",
                In6Ntop(saddr6)
            );
            break 'out false;
        }

        // We have neighbor solicitation packet, with target address equals to one of my
        // tentative address.
        //
        // src addr	how to process?
        // ---		---
        // multicast	of course, invalid (rejected in ip6_input)
        // unicast	somebody is doing address resolution -> ignore
        // unspec	dup address detection
        //
        // The processing is defined in RFC 2462.
        if tentative {
            // If source address is unspecified address, it is for duplicated address
            // detection.
            //
            // If not, the packet is for address resolution; silently ignore it.
            if in6_is_addr_unspecified(&saddr6) {
                nd6_dad_ns_input(Some(ifa));
            }

            break 'out false;
        }

        let flags = if anycast || proxy || !tlladdr {
            0
        } else {
            ND_NA_FLAG_OVERRIDE
        } | if i_am_router { ND_NA_FLAG_ROUTER } else { 0 };

        // If the source address is unspecified address, entries must not be created or
        // updated. It looks that sender is performing DAD. Output NA toward all-node
        // multicast address, to tell the sender that I'm using the address. S bit
        // ("solicited") must be zero.
        if in6_is_addr_unspecified(&saddr6) {
            saddr6 = IN6ADDR_LINKLOCAL_ALLNODES;
            saddr6.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
            nd6_na_output(
                ifp,
                &saddr6,
                &taddr6,
                u64::from(flags),
                tlladdr,
                proxydl.as_ref(),
            );
            break 'out false;
        }

        nd6_cache_lladdr(
            ifp,
            &saddr6,
            lladdr,
            i32::from(ND_NEIGHBOR_SOLICIT),
            0,
            i32::from(i_am_router),
        );

        nd6_na_output(
            ifp,
            &saddr6,
            &taddr6,
            u64::from(flags | ND_NA_FLAG_SOLICITED),
            tlladdr,
            proxydl.as_ref(),
        );
        false
    };

    if bad {
        icmp6stat_inc(Icmp6statCounters::Icp6sBadns);
    }
    // freeit:
    m_freem(m);
    if_put(ifp);
}

/// An `ip6_moptions` for ND output: out of `ifp`, hop limit 255, no loopback (the C
/// fills its stack copy only for a multicast destination; `ip6_output` reads it only
/// then).
fn nd6_im6o(ifp: &Ifnet, mcast: bool) -> Ip6Moptions {
    Ip6Moptions {
        im6o_memberships: ListHead::new(),
        im6o_ifidx: if mcast { ifp.if_index.get() as u16 } else { 0 },
        im6o_hlim: if mcast { 255 } else { 0 },
        im6o_loop: 0,
    }
}

/// The packet `nd6_ns_output`/`nd6_na_output` build: a header mbuf (with a cluster when
/// `max_linkhdr + maxlen` does not fit) for `ifp`'s routing domain; `None` when out of
/// mbufs.
fn nd6_getpkt(ifp: &Ifnet, maxlen: usize, func: &str) -> Option<&'static Mbuf> {
    let max_linkhdr = MAX_LINKHDR.load(Ordering::Relaxed) as usize;
    #[cfg(feature = "diagnostic")]
    if max_linkhdr + maxlen >= crate::sys::mbuf::MCLBYTES {
        crate::kern::subr_prf::printf(format_args!(
            "{}: max_linkhdr + maxlen >= MCLBYTES ({} + {} > {})\n",
            func,
            max_linkhdr,
            maxlen,
            crate::sys::mbuf::MCLBYTES
        ));
        panic(format_args!("{func}: insufficient MCLBYTES"));
    }
    let _ = func;

    let m = m_gethdr(M_DONTWAIT, MT_DATA)?;
    if max_linkhdr + maxlen >= MHLEN {
        mclget(m, M_DONTWAIT);
        if m.m_flags().get() & M_EXT == 0 {
            m_free(m);
            return None;
        }
    }
    m.m_pkthdr().ph_rtableid.set(ifp.if_rdomain.get());
    Some(m)
}

/// Appends the link-layer address option of `type_` carrying `mac` (`if_addrlen` bytes)
/// at `off` of `m`'s data; the option's length.
fn nd6_put_lladdr_opt(m: &Mbuf, off: usize, type_: u8, mac: &[u8], ifp: &Ifnet) -> usize {
    let addrlen = usize::from(ifp.if_addrlen.get());
    // 8 byte alignments...
    let optlen = (size_of::<NdOptHdr>() + addrlen + 7) & !7;
    let n = addrlen.min(mac.len());

    // SAFETY: the packet was sized (`maxlen`) for the message and an option of `optlen`
    // bytes after it.
    unsafe {
        let p = mtod::<u8>(m).add(off);
        ptr::write_bytes(p, 0, optlen);
        p.write(type_);
        p.add(1).write((optlen >> 3) as u8);
        ptr::copy_nonoverlapping(mac.as_ptr(), p.add(size_of::<NdOptHdr>()), n);
    }
    m.m_pkthdr().len.set(m.m_pkthdr().len.get() + optlen as i32);
    m.m_len().set(m.m_len().get() + optlen as u32);
    optlen
}

/// An IPv6 header for ND output: version 6, ICMPv6, hop limit 255.
fn nd6_ip6hdr() -> Ip6Hdr {
    let mut ip6 = Ip6Hdr::zeroed();
    ip6.ip6_flow = 0;
    ip6.set_ip6_vfc((ip6.ip6_vfc() & !IPV6_VERSION_MASK) | IPV6_VERSION);
    // ip6->ip6_plen will be set later
    ip6.ip6_nxt = IPPROTO_ICMPV6 as u8;
    ip6.ip6_hlim = 255;
    ip6
}

/// `nd6_ns_output`: output a Neighbor Solicitation Message. Caller specifies:
/// - ICMP6 header source IP6 address (`saddr6`, for source address determination)
/// - ND6 header target IP6 address (`taddr6`)
/// - ND6 header source datalink address
///
/// `daddr6` `None` means the solicited-node multicast address of `taddr6`; `dad`: a
/// duplicated address detection probe. Based on RFC 2461 and RFC 2462.
pub fn nd6_ns_output(
    ifp: &'static Ifnet,
    daddr6: Option<&In6Addr>,
    taddr6: &In6Addr,
    saddr6: Option<&In6Addr>,
    dad: bool,
) {
    if in6_is_addr_multicast(taddr6) {
        return;
    }

    // estimate the size of message
    let mut maxlen = size_of::<Ip6Hdr>() + size_of::<NdNeighborSolicit>();
    maxlen += (size_of::<NdOptHdr>() + usize::from(ifp.if_addrlen.get()) + 7) & !7;

    let Some(m) = nd6_getpkt(ifp, maxlen, "nd6_ns_output") else {
        return;
    };
    m.m_pkthdr().ph_ifidx.set(0);

    let mcast = daddr6.is_none_or(in6_is_addr_multicast);
    if mcast {
        m.m_flags().set(m.m_flags().get() | M_MCAST);
    }
    let im6o = nd6_im6o(ifp, mcast);

    let mut icmp6len = size_of::<NdNeighborSolicit>();
    m.m_len().set((size_of::<Ip6Hdr>() + icmp6len) as u32);
    m.m_pkthdr()
        .len
        .set((size_of::<Ip6Hdr>() + icmp6len) as i32);
    m_align(m, maxlen as i32);

    // fill neighbor solicitation packet
    let mut ip6 = nd6_ip6hdr();
    // determine the source and destination addresses
    let mut src_sa = SockaddrIn6::with_addr(In6Addr::new([0; 16]));
    let mut dst_sa = SockaddrIn6::with_addr(In6Addr::new([0; 16]));
    match daddr6 {
        Some(d) => dst_sa.sin6_addr = *d,
        None => {
            let a = &mut dst_sa.sin6_addr;
            a.set_s6_addr16(0, __IPV6_ADDR_INT16_MLL);
            a.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
            a.set_s6_addr32(1, 0);
            a.set_s6_addr32(2, __IPV6_ADDR_INT32_ONE);
            a.set_s6_addr32(3, taddr6.s6_addr32(3));
            a.s6_addr[12] = 0xff;
        }
    }
    ip6.ip6_dst = dst_sa.sin6_addr;
    if !dad {
        // RFC2461 7.2.2:
        // "If the source address of the packet prompting the solicitation is the same as
        // one of the addresses assigned to the outgoing interface, that address SHOULD be
        // placed in the IP Source Address of the outgoing solicitation. Otherwise, any one
        // of the addresses assigned to the interface should be used."
        //
        // We use the source address for the prompting packet (saddr6), if:
        // - saddr6 is given from the caller (by giving "ln"), and
        // - saddr6 belongs to the outgoing interface and
        // - if taddr is link local saddr6 must be link local as well
        // Otherwise, we perform the source address selection as usual.
        if let Some(s) = saddr6 {
            src_sa.sin6_addr = *s;
        }

        if !crate::netinet6::in6::in6_is_addr_linklocal(taddr6)
            || in6_is_addr_unspecified(&src_sa.sin6_addr)
            || crate::netinet6::in6::in6_is_addr_linklocal(&src_sa.sin6_addr)
            || in6ifa_ifpwithaddr(ifp, &src_sa.sin6_addr).is_none()
        {
            // SAFETY: a local `sockaddr_in6`.
            let rt = unsafe {
                rtalloc(
                    sin6tosa_const(&dst_sa),
                    RT_RESOLVE,
                    m.m_pkthdr().ph_rtableid.get(),
                )
            };
            let Some(r) = rt.filter(|r| rtisvalid(Some(r))) else {
                rtfree(rt);
                // bad:
                m_freem(m);
                return;
            };
            src_sa.sin6_addr = ifatoia6(r.ifa()).ia_addr.get().sin6_addr;
            rtfree(Some(r));
        }
    } else {
        // Source address for DAD packet must always be IPv6 unspecified address. (0::0)
        // We actually don't have to 0-clear the address (we did it above), but we do so
        // here explicitly to make the intention clearer.
        src_sa.sin6_addr = In6Addr::new([0; 16]);
    }
    ip6.ip6_src = src_sa.sin6_addr;

    let mut nd_ns = NdNeighborSolicit {
        nd_ns_hdr: Icmp6Hdr::zeroed(),
        nd_ns_target: *taddr6,
    };
    nd_ns.set_nd_ns_type(ND_NEIGHBOR_SOLICIT);
    nd_ns.set_nd_ns_code(0);
    nd_ns.set_nd_ns_reserved(0);

    if in6_is_scope_embed(&nd_ns.nd_ns_target) {
        nd_ns.nd_ns_target.set_s6_addr16(1, 0);
    }

    // Add source link-layer address option.
    //
    //				spec		implementation
    //				---		---
    // DAD packet			MUST NOT	do not add the option
    // there's no link layer address:
    //				impossible	do not add the option
    // there's link layer address:
    //	Multicast NS		MUST add one	add the option
    //	Unicast NS		SHOULD add one	add the option
    if !dad && let Some(mac) = nd6_ifptomac(ifp) {
        icmp6len += nd6_put_lladdr_opt(
            m,
            size_of::<Ip6Hdr>() + size_of::<NdNeighborSolicit>(),
            ND_OPT_SOURCE_LINKADDR,
            &mac,
            ifp,
        );
    }

    ip6.ip6_plen = htons(icmp6len as u16);
    nd_ns.set_nd_ns_cksum(0);
    mtod_ip6_store(m, &ip6);
    // SAFETY: the packet holds the header and the solicitation (`m_len` above); unaligned.
    unsafe {
        ptr::write_unaligned(
            mtod::<u8>(m)
                .add(size_of::<Ip6Hdr>())
                .cast::<NdNeighborSolicit>(),
            nd_ns,
        )
    };
    m.m_pkthdr()
        .csum_flags
        .set(m.m_pkthdr().csum_flags.get() | M_ICMP_CSUM_OUT);

    let _ = ip6_output(
        m,
        None,
        None,
        if dad { IPV6_UNSPECSRC } else { 0 },
        Some(&im6o),
        None,
    );
    icmp6stat_inc_hist(Icmp6statCounters::Icp6sOuthist, ND_NEIGHBOR_SOLICIT);
}

/// `nd6_na_input`: neighbor advertisement input handling (`icmp6len` bytes at `off` of
/// `m`, consumed). Based on RFC 2461 and RFC 2462 (duplicated address detection).
///
/// the following items are not implemented yet:
/// - proxy advertisement delay rule (RFC2461 7.2.8, last paragraph, SHOULD)
/// - anycast advertisement delay rule (RFC2461 7.2.7, SHOULD)
pub fn nd6_na_input(m: &'static Mbuf, off: i32, icmp6len: i32) {
    let i_am_router = IP6_FORWARDING.load(Ordering::Relaxed) != 0;

    net_assert_locked_exclusive("nd6_na_input");

    let Some(ifp) = if_get(m.m_pkthdr().ph_ifidx.get()) else {
        m_freem(m);
        return;
    };
    let ip6 = mtod_ip6(m);
    let daddr6 = ip6.ip6_dst;

    if ip6.ip6_hlim != 255 {
        icmp6stat_inc(Icmp6statCounters::Icp6sBadna);
        m_freem(m);
        if_put(ifp);
        return;
    }

    let mut mp = Some(m);
    let nd_na = if (icmp6len as usize) < size_of::<NdNeighborAdvert>() {
        m_freem(m);
        None
    } else {
        ip6_exthdr_get(&mut mp, off, icmp6len)
    };
    let (Some(p), Some(m)) = (nd_na, mp.filter(|_| nd_na.is_some())) else {
        icmp6stat_inc(Icmp6statCounters::Icp6sTooshort);
        if_put(ifp);
        return;
    };
    // SAFETY: `ip6_exthdr_get` made `icmp6len` bytes contiguous at `p` (at least a
    // `nd_neighbor_advert`, checked above); they live with `m`, freed below.
    let (nd_na, opts) = unsafe { nd6_msg::<NdNeighborAdvert>(p, icmp6len as usize) };
    let mut taddr6 = nd_na.nd_na_target;
    let flags = nd_na.nd_na_flags_reserved();
    let is_router = flags & ND_NA_FLAG_ROUTER != 0;
    let is_solicited = flags & ND_NA_FLAG_SOLICITED != 0;
    let is_override = flags & ND_NA_FLAG_OVERRIDE != 0;

    let mut rt: Option<&'static Rtentry> = None;
    let bad = 'out: {
        if in6_is_scope_embed(&taddr6) {
            taddr6.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
        }

        if in6_is_addr_multicast(&taddr6) {
            break 'out true;
        }
        if is_solicited && in6_is_addr_multicast(&daddr6) {
            break 'out true;
        }

        let mut ndopts = NdOpts::default();
        if !nd6_options(opts, &mut ndopts) {
            // nd6_options have incremented stats
            break 'out false;
        }

        if in6_is_addr_multicast(&daddr6) && ndopts.nd_opts_tgt_lladdr.is_none() {
            break 'out true;
        }

        // SAFETY: the option is inside the message, which lives until `m` is freed below.
        let (lladdr, lladdrlen) = unsafe { nd6_lladdr_of(ndopts.nd_opts_tgt_lladdr) };

        let ifa = in6ifa_ifpwithaddr(ifp, &taddr6).map(|ia| &ia.ia_ifa);

        // Target address matches one of my interface address.
        //
        // If my address is tentative, this means that there's somebody already using the
        // same address as mine. This indicates DAD failure. This is defined in RFC 2462.
        //
        // Otherwise, process as defined in RFC 2461.
        if let Some(ifa) = ifa
            && ifatoia6(ifa).ia6_flags.get() & IN6_IFF_TENTATIVE != 0
        {
            if let Some(dp) = nd6_dad_find(ifa) {
                dp.dad_na_icount.set(dp.dad_na_icount.get() + 1);

                // remove the address.
                nd6_dad_duplicated(dp);
            }
            break 'out false;
        }

        if ifa.is_some() {
            // NCARP > 0: ignore NAs silently for carp addresses if we're not the CARP
            // master; not configured.
            log!(
                LOG_ERR,
                "nd6_na_input: duplicate IP6 address {}\n",
                In6Ntop(taddr6)
            );
            break 'out false;
        }

        if lladdr.is_some() && ((i32::from(ifp.if_addrlen.get()) + 2 + 7) & !7) != lladdrlen {
            break 'out true;
        }

        // Check if we already have this neighbor in our cache.
        rt = nd6_lookup(&taddr6, false, Some(ifp), ifp.if_rdomain.get());

        // If we are a router, we may create new stale cache entries upon receiving
        // Unsolicited Neighbor Advertisements.
        if rt.is_none() && i_am_router {
            rt = nd6_lookup(&taddr6, true, Some(ifp), ifp.if_rdomain.get());
            let Some(r) = rt else {
                break 'out false;
            };
            let sdl = satosdl(r.rt_gateway.get());
            let (Some(l), Some(ln)) = (lladdr, rt_ln(r)) else {
                break 'out false;
            };
            if sdl.is_null() {
                break 'out false;
            }
            // SAFETY: the gateway of a neighbor route `nd6_lookup` validated is a
            // `sockaddr_dl` with room for the interface's address.
            unsafe { nd6_llstore(l, sdl, ifp) };

            // RFC9131 6.1.1
            //
            // Routers SHOULD create a new entry for the target address with the
            // reachability state set to STALE.
            ln.ln_state.set(ND6_LLINFO_STALE);
            nd6_llinfo_settimer(ln, ND6_GCTIMER as u32);

            break 'out false;
        }

        // Host: If no neighbor cache entry is found, NA SHOULD silently be discarded.
        if let Some(r) = rt {
            nd6_na_cache(
                ifp,
                r,
                lladdr,
                is_router,
                is_solicited,
                is_override,
                i_am_router,
                &taddr6,
                &ip6.ip6_src,
            );
        }
        false
    };

    if bad {
        icmp6stat_inc(Icmp6statCounters::Icp6sBadna);
    }
    // freeit: (rt is always None on the bad paths)
    rtfree(rt);
    m_freem(m);
    if_put(ifp);
}

/// `nd6_na_input` from the neighbor cache entry `rt` of the target `taddr6` on: updates it
/// from an advertisement (`is_router`, `is_solicited`, `is_override` flags, target
/// link-layer address `lladdr`) sent from `src`, and sends what it holds once usable.
#[allow(clippy::too_many_arguments)] // the locals of the C function the code came from
pub(crate) fn nd6_na_cache(
    ifp: &'static Ifnet,
    rt: &'static Rtentry,
    lladdr: Option<&[u8]>,
    is_router: bool,
    is_solicited: bool,
    is_override: bool,
    i_am_router: bool,
    taddr6: &In6Addr,
    src: &In6Addr,
) {
    let Some(ln) = rt_ln(rt) else {
        return;
    };
    let sdl = satosdl(rt.rt_gateway.get());
    if sdl.is_null() {
        return;
    }

    if ln.ln_state.get() == ND6_LLINFO_INCOMPLETE {
        // If the link-layer has address, and no lladdr option came, discard the packet.
        if ifp.if_addrlen.get() != 0 && lladdr.is_none() {
            return;
        }

        // Record link-layer address, and update the state.
        // SAFETY: the gateway of a neighbor route is a `sockaddr_dl` with room for the
        // interface's address (`nd6_lookup` validated it).
        unsafe { nd6_llstore(lladdr.unwrap_or(&[]), sdl, ifp) };
        if is_solicited {
            ln.ln_state.set(ND6_LLINFO_REACHABLE);
            // Notify userland that a new ND entry is reachable.
            rtm_send(rt, RTM_RESOLVE, 0, ifp.if_rdomain.get());
            if !nd6_llinfo_permanent(ln) {
                nd6_llinfo_settimer(ln, nd6_reachable(ifp));
            }
        } else {
            ln.ln_state.set(ND6_LLINFO_STALE);
            nd6_llinfo_settimer(ln, ND6_GCTIMER as u32);
        }
        ln.ln_router.set(i16::from(is_router));
        if is_router {
            // This means a router's state has changed from non-reachable to probably
            // reachable, and might affect the status of associated prefixes..
            if rt.rt_flags.get() & RTF_LLINFO == 0 {
                return; // ln is gone
            }
        }
    } else {
        // Check if the link-layer address has changed or not.
        let llchange = match lladdr {
            None => false,
            // SAFETY: as above.
            Some(l) => unsafe { (*sdl).sdl_alen == 0 || nd6_llchanged(l, sdl, ifp) },
        };

        // This is VERY complex.  Look at it with care.
        //
        // override solicit lladdr llchange	action
        //					(L: record lladdr)
        //
        //	0	0	n	--	(2c)
        //	0	0	y	n	(2b) L
        //	0	0	y	y	(1)    REACHABLE->STALE
        //	0	1	n	--	(2c)   *->REACHABLE
        //	0	1	y	n	(2b) L *->REACHABLE
        //	0	1	y	y	(1)    REACHABLE->STALE
        //	1	0	n	--	(2a)
        //	1	0	y	n	(2a) L
        //	1	0	y	y	(2a) L *->STALE
        //	1	1	n	--	(2a)   *->REACHABLE
        //	1	1	y	n	(2a) L *->REACHABLE
        //	1	1	y	y	(2a) L *->REACHABLE
        if !is_override && lladdr.is_some() && llchange {
            // (1)
            // If state is REACHABLE, make it STALE. no other updates should be done.
            if ln.ln_state.get() == ND6_LLINFO_REACHABLE {
                ln.ln_state.set(ND6_LLINFO_STALE);
                nd6_llinfo_settimer(ln, ND6_GCTIMER as u32);
            }
            return;
        } else if is_override                                 // (2a)
            || (!is_override && lladdr.is_some() && !llchange) // (2b)
            || lladdr.is_none()
        // (2c)
        {
            // Update link-local address, if any.
            if llchange && let Some(l) = lladdr {
                let s = nd6_ether_sprintf(l);
                log!(
                    LOG_INFO,
                    "ndp info overwritten for {} by {} on {}\n",
                    In6Ntop(*taddr6),
                    Str(&s),
                    Str(&ifp.if_xname.get())
                );
            }
            if let Some(l) = lladdr {
                // SAFETY: as above.
                unsafe { nd6_llstore(l, sdl, ifp) };
            }

            // If solicited, make the state REACHABLE. If not solicited and the link-layer
            // address was changed, make it STALE.
            if is_solicited {
                ln.ln_state.set(ND6_LLINFO_REACHABLE);
                if !nd6_llinfo_permanent(ln) {
                    nd6_llinfo_settimer(ln, nd6_reachable(ifp));
                }
            } else if lladdr.is_some() && llchange {
                ln.ln_state.set(ND6_LLINFO_STALE);
                nd6_llinfo_settimer(ln, ND6_GCTIMER as u32);
            }
        }

        if ln.ln_router.get() != 0 && !is_router && !i_am_router {
            // The neighbor may be used as a next hop for some destinations (e.g. redirect
            // case). So we must call rt6_flush explicitly.
            let _ = rt6_flush(src, ifp);
        }
        ln.ln_router.set(i16::from(is_router));
    }
    rt.rt_flags.set(rt.rt_flags.get() & !RTF_REJECT);
    ln.ln_asked.set(0);
    // SAFETY: the route's key is its readable `sockaddr_in6`.
    let _ = unsafe { if_output_mq(ifp, &ln.ln_mq, &ln_hold_total, rt_key(rt), Some(rt)) };
}

/// `nd6_na_output`: neighbor advertisement output handling. Based on RFC 2461.
///
/// `tlladdr`: include the target link-layer address; `sdl0`: the `sockaddr_dl` of a
/// proxy NA, or `None`.
///
/// the following items are not implemented yet:
/// - proxy advertisement delay rule (RFC2461 7.2.8, last paragraph, SHOULD)
/// - anycast advertisement delay rule (RFC2461 7.2.7, SHOULD)
pub fn nd6_na_output(
    ifp: &'static Ifnet,
    daddr6: &In6Addr,
    taddr6: &In6Addr,
    flags: u64,
    tlladdr: bool,
    sdl0: Option<&SockaddrDl>,
) {
    let mut flags = flags;

    // NCARP > 0: do not send NAs for carp addresses if we're not the CARP master; not
    // configured.

    // estimate the size of message
    let mut maxlen = size_of::<Ip6Hdr>() + size_of::<NdNeighborAdvert>();
    maxlen += (size_of::<NdOptHdr>() + usize::from(ifp.if_addrlen.get()) + 7) & !7;

    let Some(m) = nd6_getpkt(ifp, maxlen, "nd6_na_output") else {
        return;
    };

    let mcast = in6_is_addr_multicast(daddr6);
    if mcast {
        m.m_flags().set(m.m_flags().get() | M_MCAST);
    }
    let im6o = nd6_im6o(ifp, mcast);

    let mut icmp6len = size_of::<NdNeighborAdvert>();
    m.m_len().set((size_of::<Ip6Hdr>() + icmp6len) as u32);
    m.m_pkthdr()
        .len
        .set((size_of::<Ip6Hdr>() + icmp6len) as i32);
    m_align(m, maxlen as i32);

    // fill neighbor advertisement packet
    let mut ip6 = nd6_ip6hdr();
    let mut dst_sa = SockaddrIn6::with_addr(*daddr6);
    if in6_is_addr_unspecified(daddr6) {
        // reply to DAD
        let a = &mut dst_sa.sin6_addr;
        a.set_s6_addr16(0, __IPV6_ADDR_INT16_MLL);
        a.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
        a.set_s6_addr32(1, 0);
        a.set_s6_addr32(2, 0);
        a.set_s6_addr32(3, __IPV6_ADDR_INT32_ONE);

        flags &= !u64::from(ND_NA_FLAG_SOLICITED);
    }
    ip6.ip6_dst = dst_sa.sin6_addr;

    // Select a source whose scope is the same as that of the dest.
    // SAFETY: a local `sockaddr_in6`.
    let rt = unsafe { rtalloc(sin6tosa_const(&dst_sa), RT_RESOLVE, ifp.if_rdomain.get()) };
    let Some(r) = rt.filter(|r| rtisvalid(Some(r))) else {
        rtfree(rt);
        // bad:
        m_freem(m);
        return;
    };
    ip6.ip6_src = ifatoia6(r.ifa()).ia_addr.get().sin6_addr;
    rtfree(Some(r));

    let mut nd_na = NdNeighborAdvert {
        nd_na_hdr: Icmp6Hdr::zeroed(),
        nd_na_target: *taddr6,
    };
    nd_na.set_nd_na_type(ND_NEIGHBOR_ADVERT);
    nd_na.set_nd_na_code(0);
    if in6_is_scope_embed(&nd_na.nd_na_target) {
        nd_na.nd_na_target.set_s6_addr16(1, 0);
    }

    // "tlladdr" indicates NS's condition for adding tlladdr or not. see nd6_ns_input() for
    // details. Basically, if NS packet is sent to unicast/anycast addr, target lladdr
    // option SHOULD NOT be included.
    let ifmac: [u8; ETHER_ADDR_LEN];
    let mut mac: Option<&[u8]> = None;
    if tlladdr {
        // sdl0 != NULL indicates proxy NA. If we do proxy, use lladdr in sdl0. If we are
        // not proxying (sending NA for my address) use lladdr configured for the interface.
        match sdl0 {
            None => {
                if let Some(a) = nd6_ifptomac(ifp) {
                    ifmac = a;
                    mac = Some(&ifmac);
                }
            }
            Some(sdl) if sdl.sdl_family == AF_LINK => {
                if sdl.sdl_alen == ifp.if_addrlen.get() {
                    // SAFETY: a `sockaddr_dl` holds `sdl_alen` address bytes after its
                    // name, inside its data.
                    mac = Some(unsafe {
                        core::slice::from_raw_parts(
                            lladdr(ptr::from_ref(sdl).cast_mut()),
                            usize::from(sdl.sdl_alen),
                        )
                    });
                }
            }
            Some(_) => {}
        }
    }
    match mac {
        Some(mac) if tlladdr => {
            icmp6len += nd6_put_lladdr_opt(
                m,
                size_of::<Ip6Hdr>() + size_of::<NdNeighborAdvert>(),
                ND_OPT_TARGET_LINKADDR,
                mac,
                ifp,
            );
        }
        _ => flags &= !u64::from(ND_NA_FLAG_OVERRIDE),
    }

    ip6.ip6_plen = htons(icmp6len as u16);
    nd_na.set_nd_na_flags_reserved(flags as u32);
    nd_na.set_nd_na_cksum(0);
    mtod_ip6_store(m, &ip6);
    // SAFETY: the packet holds the header and the advertisement (`m_len` above);
    // unaligned.
    unsafe {
        ptr::write_unaligned(
            mtod::<u8>(m)
                .add(size_of::<Ip6Hdr>())
                .cast::<NdNeighborAdvert>(),
            nd_na,
        )
    };
    m.m_pkthdr()
        .csum_flags
        .set(m.m_pkthdr().csum_flags.get() | M_ICMP_CSUM_OUT);

    let _ = ip6_output(m, None, None, 0, Some(&im6o), None);
    icmp6stat_inc_hist(Icmp6statCounters::Icp6sOuthist, ND_NEIGHBOR_ADVERT);
}

/// `nd6_ifptomac`: the link-layer address of `ifp` for ND options (the `struct arpcom`
/// interfaces of the C's list).
pub fn nd6_ifptomac(ifp: &Ifnet) -> Option<[u8; ETHER_ADDR_LEN]> {
    match ifp.if_type.get() {
        IFT_ETHER | IFT_IEEE1394 | IFT_PROPVIRTUAL | IFT_CARP | IFT_IEEE80211
            if ifp.is_arpcom() =>
        {
            Some(arpcom_of(ifp).ac_enaddr.get())
        }
        _ => None,
    }
}

/// `nd6_dad_find`: the DAD state of `ifa`, if DAD runs on it.
fn nd6_dad_find(ifa: &Ifaddr) -> Option<&'static Dadq> {
    DADQ.0
        .iter()
        .find(|dp| ptr::eq(dp.dad_ifa, ifa))
        // SAFETY: an entry on `dadq` lives until `nd6_dad_reaper` frees it, after its
        // removal (under the net lock, as the callers hold).
        .map(|dp| unsafe { &*ptr::from_ref(dp) })
}

/// `nd6_dad_destroy`: takes `dp` off the list and frees it from a timeout.
fn nd6_dad_destroy(dp: &'static Dadq) {
    // SAFETY: `dp` is on `dadq`, under the net lock.
    unsafe { DADQ.0.remove(dp) };
    IP6_DAD_PENDING.fetch_sub(1, Ordering::Relaxed);
    timeout_set_proc(
        &dp.dad_timer_ch,
        nd6_dad_reaper,
        ptr::from_ref(dp).cast_mut().cast(),
    );
    let _ = timeout_add(&dp.dad_timer_ch, 0);
}

/// `nd6_dad_reaper`: drops the address reference of a destroyed `dadq` and frees it.
fn nd6_dad_reaper(arg: *mut c_void) {
    // SAFETY: `nd6_dad_destroy` armed the timeout with the `dadq`, which is off the list
    // and owned by this timeout alone now.
    let dp = unsafe { &*arg.cast::<Dadq>() };

    ifafree(dp.dad_ifa);
    // `nd6_dad_start`'s allocation; nothing refers to it any more (a `Dadq` owns no
    // resources besides the address reference just dropped).
    free(NonNull::from(dp).cast(), M_IP6NDP, size_of::<Dadq>());
}

/// `nd6_dad_starttimer`: the next DAD step in `RETRANS_TIMER` milliseconds.
fn nd6_dad_starttimer(dp: &Dadq) {
    timeout_set_proc(
        &dp.dad_timer_ch,
        nd6_dad_timer,
        ptr::from_ref(dp.dad_ifa).cast_mut().cast(),
    );
    let _ = timeout_add_msec(&dp.dad_timer_ch, u64::from(RETRANS_TIMER));
}

/// `nd6_dad_stoptimer`.
fn nd6_dad_stoptimer(dp: &Dadq) {
    let _ = timeout_del(&dp.dad_timer_ch);
}

/// `nd6_dad_start`: start Duplicated Address Detection (DAD) for specified interface
/// address.
pub fn nd6_dad_start(ifa: &'static Ifaddr) {
    let ia6 = ifatoia6(ifa);
    let ip6_dad_count_local = IP6_DAD_COUNT.load(Ordering::Relaxed);

    net_assert_locked("nd6_dad_start");

    // If we don't need DAD, don't do it. There are several cases:
    // - DAD is disabled (ip6_dad_count == 0)
    // - the interface address is anycast
    crate::kassert!(ia6.ia6_flags.get() & IN6_IFF_TENTATIVE != 0);
    if ia6.ia6_flags.get() & IN6_IFF_ANYCAST != 0 || ip6_dad_count_local == 0 {
        ia6.ia6_flags.set(ia6.ia6_flags.get() & !IN6_IFF_TENTATIVE);

        rtm_addr(RTM_CHGADDRATTR, ifa);

        return;
    }

    // DAD already in progress
    if nd6_dad_find(ifa).is_some() {
        return;
    }

    let Some(mem) = malloc(size_of::<Dadq>(), M_IP6NDP, M_NOWAIT | M_ZERO) else {
        log!(
            LOG_ERR,
            "nd6_dad_start: memory allocation failed for {}({})\n",
            In6Ntop(ia6.ia_addr.get().sin6_addr),
            Str(&ifa
                .ifa_ifp
                .get()
                .map_or(UNKNOWN_IFNAME, |ifp| ifp.if_xname.get()))
        );
        return;
    };
    let p = mem.cast::<Dadq>().as_ptr();
    // Send NS packet for DAD, ip6_dad_count times. Note that we must delay the first
    // transmission, if this is the first packet to be sent from the interface after
    // interface (re)initialization.
    // SAFETY: a fresh allocation of `size_of::<Dadq>()` bytes, aligned by `malloc`,
    // written whole; it lives until `nd6_dad_reaper`.
    let dp: &'static Dadq = unsafe {
        p.write(Dadq {
            dad_list: TailqEntry::new(),
            dad_ifa: ifaref(ifa),
            dad_count: Cell::new(ip6_dad_count_local),
            dad_ns_tcount: Cell::new(0),
            dad_ns_ocount: Cell::new(0),
            dad_ns_icount: Cell::new(0),
            dad_na_icount: Cell::new(0),
            dad_timer_ch: Timeout::new(nd6_dad_timer, ptr::null_mut()),
        });
        &*p
    };

    // SAFETY: `dp` is on no list; `dadq` is changed under the net lock.
    unsafe { DADQ.0.insert_tail(dp) };
    IP6_DAD_PENDING.fetch_add(1, Ordering::Relaxed);

    nd6_dad_ns_output(dp, ifa);
    nd6_dad_starttimer(dp);
}

/// `nd6_dad_stop`: terminate DAD unconditionally. used for address removals.
pub fn nd6_dad_stop(ifa: &'static Ifaddr) {
    let Some(dp) = nd6_dad_find(ifa) else {
        // DAD wasn't started yet
        return;
    };

    nd6_dad_stoptimer(dp);
    nd6_dad_destroy(dp);
}

/// `nd6_dad_timer`: one DAD step: another solicitation, or the verdict.
fn nd6_dad_timer(arg: *mut c_void) {
    // SAFETY: `nd6_dad_starttimer` armed the timeout with the address, referenced by its
    // `dadq` until the reaper runs.
    let ifa: &'static Ifaddr = unsafe { &*arg.cast::<Ifaddr>() };

    net_lock();

    'done: {
        let ia6 = ifatoia6(ifa);
        let taddr6 = ia6.ia_addr.get().sin6_addr;
        let ifname = ifa
            .ifa_ifp
            .get()
            .map_or(UNKNOWN_IFNAME, |ifp| ifp.if_xname.get());
        let Some(dp) = nd6_dad_find(ifa) else {
            log!(LOG_ERR, "nd6_dad_timer: DAD structure not found\n");
            break 'done;
        };
        if ia6.ia6_flags.get() & IN6_IFF_DUPLICATED != 0 {
            log!(
                LOG_ERR,
                "nd6_dad_timer: called with duplicated address {}({})\n",
                In6Ntop(taddr6),
                Str(&ifname)
            );
            break 'done;
        }
        if ia6.ia6_flags.get() & IN6_IFF_TENTATIVE == 0 {
            log!(
                LOG_ERR,
                "nd6_dad_timer: called with non-tentative address {}({})\n",
                In6Ntop(taddr6),
                Str(&ifname)
            );
            break 'done;
        }

        // timeouted with IFF_{RUNNING,UP} check
        if dp.dad_ns_tcount.get() > DAD_MAXTRY.load(Ordering::Relaxed) {
            nd6_dad_destroy(dp);
            break 'done;
        }

        // Need more checks?
        if dp.dad_ns_ocount.get() < dp.dad_count.get() {
            // We have more NS to go. Send NS packet for DAD.
            nd6_dad_ns_output(dp, ifa);
            nd6_dad_starttimer(dp);
        } else {
            // We have transmitted sufficient number of DAD packets.
            if dp.dad_na_icount.get() != 0 || dp.dad_ns_icount.get() != 0 {
                // dp will be freed in nd6_dad_duplicated()
                nd6_dad_duplicated(dp);
            } else {
                // We are done with DAD. No NA came, no NS came.
                ia6.ia6_flags.set(ia6.ia6_flags.get() & !IN6_IFF_TENTATIVE);

                rtm_addr(RTM_CHGADDRATTR, ifa);

                if let Some(ifp) = ifa.ifa_ifp.get() {
                    let mut daddr6 = IN6ADDR_LINKLOCAL_ALLROUTERS;
                    daddr6.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
                    // RFC9131 - inform routers about our new address
                    nd6_na_output(ifp, &daddr6, &taddr6, 0, true, None);
                }

                nd6_dad_destroy(dp);
            }
        }
    }

    net_unlock();
}

/// `nd6_dad_duplicated`: DAD found the address in use: mark it duplicated and stop.
fn nd6_dad_duplicated(dp: &'static Dadq) {
    let ia6 = ifatoia6(dp.dad_ifa);
    let ifname = ia6
        .ia_ifp()
        .get()
        .map_or(UNKNOWN_IFNAME, |ifp| ifp.if_xname.get());
    let addr = ia6.ia_addr.get().sin6_addr;

    log!(
        LOG_ERR,
        "{}: DAD detected duplicate IPv6 address {}: NS in/out={}/{}, NA in={}\n",
        Str(&ifname),
        In6Ntop(addr),
        dp.dad_ns_icount.get(),
        dp.dad_ns_ocount.get(),
        dp.dad_na_icount.get()
    );

    ia6.ia6_flags.set(ia6.ia6_flags.get() & !IN6_IFF_TENTATIVE);
    ia6.ia6_flags.set(ia6.ia6_flags.get() | IN6_IFF_DUPLICATED);

    // We are done with DAD, with duplicated address found. (failure)
    nd6_dad_stoptimer(dp);

    log!(
        LOG_ERR,
        "{}: DAD complete for {} - duplicate found\n",
        Str(&ifname),
        In6Ntop(addr)
    );
    log!(LOG_ERR, "{}: manual intervention required\n", Str(&ifname));

    rtm_addr(RTM_CHGADDRATTR, dp.dad_ifa);

    nd6_dad_destroy(dp);
}

/// `nd6_dad_ns_output`: one DAD solicitation for `ifa`, if its interface is up and
/// running.
fn nd6_dad_ns_output(dp: &Dadq, ifa: &Ifaddr) {
    let ia6 = ifatoia6(ifa);
    let Some(ifp) = ifa.ifa_ifp.get() else {
        return;
    };

    dp.dad_ns_tcount.set(dp.dad_ns_tcount.get() + 1);
    if ifp.if_flags.get() & IFF_UP == 0 {
        return;
    }
    if ifp.if_flags.get() & IFF_RUNNING == 0 {
        return;
    }

    dp.dad_ns_ocount.set(dp.dad_ns_ocount.get() + 1);
    nd6_ns_output(ifp, None, &ia6.ia_addr.get().sin6_addr, None, true);
}

/// `nd6_dad_ns_input`: a DAD solicitation from someone else for our tentative `ifa`.
fn nd6_dad_ns_input(ifa: Option<&Ifaddr>) {
    let Some(ifa) = ifa else {
        panic(format_args!("nd6_dad_ns_input: ifa == NULL"));
    };

    let Some(dp) = nd6_dad_find(ifa) else {
        log!(LOG_ERR, "nd6_dad_ns_input: DAD structure not found\n");
        return;
    };

    // if I'm yet to start DAD, someone else started using this address first. I have a
    // duplicate and you win.
    // XXX more checks for loopback situation - see nd6_dad_timer too
    if dp.dad_ns_ocount.get() == 0 {
        // dp will be freed in nd6_dad_duplicated()
        nd6_dad_duplicated(dp);
    } else {
        // not sure if I got a duplicate. increment ns count and see what happens.
        dp.dad_ns_icount.set(dp.dad_ns_icount.get() + 1);
    }
}

/// `nd6_isneighbor`: check whether `addr` is a neighbor address connected to `ifp`.
fn nd6_isneighbor(ifp: &Ifnet, addr: &In6Addr) -> bool {
    let tableid = ifp.if_rdomain.get();
    let sin6 = SockaddrIn6::with_addr(*addr);

    // SAFETY: a local `sockaddr_in6`.
    let rt = unsafe { rtalloc(sin6tosa_const(&sin6), 0, tableid) };

    let rv = rt.is_some_and(|r| {
        rtisvalid(Some(r))
            && r.rt_flags.get() & (RTF_CLONING | RTF_CLONED) != 0
            && if_isconnected(ifp, r.rt_ifidx.get())
    });

    rtfree(rt);
    rv
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for neighbor solicitation/advertisement input on crafted packets (the checks
    // before any table lookup) and for Duplicate Address Detection's counting.

    use std::vec::Vec;

    use super::*;
    use crate::kern::uipc_mbuf::m_freem;
    use crate::net::if_::tests::test_packet;
    use crate::netinet6::in6::IN6ADDR_ANY;
    use crate::netinet6::in6_var::In6Ifaddr;
    use crate::netinet6::nd6::tests::{OURS6, PEER6, fake_ifa6, icmp6stat, nd6_setup};

    /// An ICMPv6 ND packet from `src` to `dst` with hop limit `hlim`: the fixed part `msg`
    /// (type, code, checksum, reserved/flags, target) and the options `opts`.
    fn nd_packet(
        ifp: &Ifnet,
        src: In6Addr,
        dst: In6Addr,
        hlim: u8,
        msg: &[u8],
        opts: &[u8],
    ) -> (&'static Mbuf, i32) {
        let mut ip6 = Ip6Hdr::zeroed();
        ip6.set_ip6_vfc(IPV6_VERSION);
        ip6.ip6_plen = htons((msg.len() + opts.len()) as u16);
        ip6.ip6_nxt = IPPROTO_ICMPV6 as u8;
        ip6.ip6_hlim = hlim;
        ip6.ip6_src = src;
        ip6.ip6_dst = dst;
        // SAFETY: `Ip6Hdr` is 40 bytes of integers without padding.
        let hdr: [u8; 40] = unsafe { core::mem::transmute(ip6) };
        let mut b: Vec<u8> = hdr.to_vec();
        b.extend_from_slice(msg);
        b.extend_from_slice(opts);
        let m = test_packet(&b);
        m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
        (m, (msg.len() + opts.len()) as i32)
    }

    /// The fixed part of a solicitation (`flags` 0) or advertisement for `target`.
    fn nd_msg(type_: u8, flags: u32, target: In6Addr) -> Vec<u8> {
        let mut b = std::vec![type_, 0, 0, 0];
        b.extend_from_slice(&flags.to_ne_bytes());
        b.extend_from_slice(&target.s6_addr);
        b
    }

    /// `ff02::1:ff00:1`, the solicited-node group of `OURS6`.
    fn solicited_node() -> In6Addr {
        let mut a = In6Addr::new([0xff, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0xff, 0, 0, 1]);
        a.s6_addr[13..].copy_from_slice(&OURS6.s6_addr[13..]);
        a
    }

    /// A crafted solicitation: source, destination, hop limit, fixed part, options.
    type NsCase<'a> = (In6Addr, In6Addr, u8, Vec<u8>, &'a [u8]);

    #[test]
    fn bad_solicitations_are_counted_and_dropped() {
        let (_g, ifp) = nd6_setup();
        let badns = || icmp6stat(Icmp6statCounters::Icp6sBadns);
        let badopt = || icmp6stat(Icmp6statCounters::Icp6sNdBadopt);
        let ns = nd_msg(ND_NEIGHBOR_SOLICIT, 0, OURS6);
        let lla = [ND_OPT_SOURCE_LINKADDR, 1, 0x52, 0x55, 0x0a, 0, 2, 2];

        let cases: [NsCase<'_>; 4] = [
            // the hop limit must be 255
            (IN6ADDR_ANY, solicited_node(), 254, ns.clone(), &[]),
            // from ::, to something other than a solicited-node group
            (IN6ADDR_ANY, OURS6, 255, ns.clone(), &[]),
            // a multicast target
            (
                IN6ADDR_ANY,
                solicited_node(),
                255,
                nd_msg(ND_NEIGHBOR_SOLICIT, 0, solicited_node()),
                &[],
            ),
            // a DAD probe (from ::) must not carry a source link-layer address
            (IN6ADDR_ANY, solicited_node(), 255, ns.clone(), &lla),
        ];
        for (i, (src, dst, hlim, msg, opts)) in cases.iter().enumerate() {
            let before = badns();
            let (m, len) = nd_packet(ifp, *src, *dst, *hlim, msg, opts);
            nd6_ns_input(m, 40, len);
            assert_eq!(badns(), before + 1, "case {i}");
        }

        // A zero-length option: dropped by nd6_options, counted there.
        let (before, before_opt) = (badns(), badopt());
        let (m, len) = nd_packet(
            ifp,
            IN6ADDR_ANY,
            solicited_node(),
            255,
            &ns,
            &[1, 0, 0, 0, 0, 0, 0, 0],
        );
        nd6_ns_input(m, 40, len);
        assert_eq!((badns(), badopt()), (before, before_opt + 1));

        // Shorter than a solicitation.
        let before = icmp6stat(Icmp6statCounters::Icp6sTooshort);
        let (m, _) = nd_packet(ifp, IN6ADDR_ANY, solicited_node(), 255, &ns[..20], &[]);
        nd6_ns_input(m, 40, 20);
        assert_eq!(icmp6stat(Icmp6statCounters::Icp6sTooshort), before + 1);
    }

    #[test]
    fn bad_advertisements_are_counted_and_dropped() {
        let (_g, ifp) = nd6_setup();
        let badna = || icmp6stat(Icmp6statCounters::Icp6sBadna);
        let all_nodes = IN6ADDR_LINKLOCAL_ALLNODES;
        let tlla = [ND_OPT_TARGET_LINKADDR, 1, 0x52, 0x55, 0x0a, 0, 2, 2];

        let cases: [(In6Addr, u8, Vec<u8>, &[u8]); 4] = [
            // the hop limit must be 255
            (
                OURS6,
                64,
                nd_msg(ND_NEIGHBOR_ADVERT, ND_NA_FLAG_SOLICITED, PEER6),
                &tlla,
            ),
            // a multicast target
            (OURS6, 255, nd_msg(ND_NEIGHBOR_ADVERT, 0, all_nodes), &tlla),
            // solicited, to a multicast group
            (
                all_nodes,
                255,
                nd_msg(ND_NEIGHBOR_ADVERT, ND_NA_FLAG_SOLICITED, PEER6),
                &tlla,
            ),
            // to a multicast group without the target link-layer address
            (
                all_nodes,
                255,
                nd_msg(ND_NEIGHBOR_ADVERT, ND_NA_FLAG_OVERRIDE, PEER6),
                &[],
            ),
        ];
        net_lock();
        for (i, (dst, hlim, msg, opts)) in cases.iter().enumerate() {
            let before = badna();
            let (m, len) = nd_packet(ifp, PEER6, *dst, *hlim, msg, opts);
            nd6_na_input(m, 40, len);
            assert_eq!(badna(), before + 1, "case {i}");
        }

        // A good unsolicited advertisement for a neighbor we have no entry for (a host): ignored,
        // not counted as bad.
        let before = badna();
        let (m, len) = nd_packet(
            ifp,
            PEER6,
            all_nodes,
            255,
            &nd_msg(ND_NEIGHBOR_ADVERT, ND_NA_FLAG_OVERRIDE, PEER6),
            &tlla,
        );
        nd6_na_input(m, 40, len);
        assert_eq!(badna(), before);
        net_unlock();
    }

    /// A tentative address `OURS6` on `ifp`, and the DAD state `nd6_dad_start` made for it.
    fn start_dad(ifp: &'static Ifnet) -> (&'static In6Ifaddr, &'static Dadq) {
        let ia = fake_ifa6(ifp, OURS6);
        ia.ia6_flags.set(IN6_IFF_TENTATIVE);
        net_lock();
        nd6_dad_start(&ia.ia_ifa);
        net_unlock();
        let dp = nd6_dad_find(&ia.ia_ifa).expect("dadq");
        (ia, dp)
    }

    /// Frees a `dadq` that `nd6_dad_destroy` left to its reaper (no softclock runs here).
    fn reap(dp: &'static Dadq) {
        let _ = timeout_del(&dp.dad_timer_ch);
        nd6_dad_reaper(ptr::from_ref(dp).cast_mut().cast());
    }

    #[test]
    fn dad_without_an_answer_makes_the_address_usable() {
        let (_g, ifp) = nd6_setup();
        let pending = IP6_DAD_PENDING.load(Ordering::Relaxed);
        let (ia, dp) = start_dad(ifp);
        assert_eq!(IP6_DAD_PENDING.load(Ordering::Relaxed), pending + 1);
        assert_eq!(dp.dad_count.get(), IP6_DAD_COUNT.load(Ordering::Relaxed));
        assert_eq!((dp.dad_ns_tcount.get(), dp.dad_ns_ocount.get()), (1, 1));
        assert_eq!(
            ia.ia_ifa.ifa_refcnt.r_refs.load(Ordering::Relaxed),
            2,
            "dadq's reference"
        );

        // A second start for the same address does nothing.
        net_lock();
        nd6_dad_start(&ia.ia_ifa);
        net_unlock();
        assert_eq!(IP6_DAD_PENDING.load(Ordering::Relaxed), pending + 1);

        nd6_dad_timer(ptr::from_ref(&ia.ia_ifa).cast_mut().cast());
        assert_eq!(
            ia.ia6_flags.get() & (IN6_IFF_TENTATIVE | IN6_IFF_DUPLICATED),
            0
        );
        assert!(nd6_dad_find(&ia.ia_ifa).is_none());
        assert_eq!(IP6_DAD_PENDING.load(Ordering::Relaxed), pending);
        reap(dp);
        assert_eq!(ia.ia_ifa.ifa_refcnt.r_refs.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_dad_probe_from_someone_else_marks_the_address_duplicated() {
        let (_g, ifp) = nd6_setup();
        let (ia, dp) = start_dad(ifp);

        // Another node probes for the same address after our probe: counted, decided at the
        // timer.
        net_lock();
        nd6_dad_ns_input(Some(&ia.ia_ifa));
        net_unlock();
        assert_eq!(dp.dad_ns_icount.get(), 1);
        assert_ne!(ia.ia6_flags.get() & IN6_IFF_TENTATIVE, 0);
        nd6_dad_timer(ptr::from_ref(&ia.ia_ifa).cast_mut().cast());
        assert_eq!(ia.ia6_flags.get() & IN6_IFF_TENTATIVE, 0);
        assert_ne!(ia.ia6_flags.get() & IN6_IFF_DUPLICATED, 0);
        assert!(nd6_dad_find(&ia.ia_ifa).is_none());
        reap(dp);

        // Down interface: no probe goes out (only the try counts); a probe from someone else
        // then wins at once.
        ifp.if_flags.set(ifp.if_flags.get() & !IFF_UP);
        let (ia, dp) = start_dad(ifp);
        assert_eq!((dp.dad_ns_tcount.get(), dp.dad_ns_ocount.get()), (1, 0));
        net_lock();
        nd6_dad_ns_input(Some(&ia.ia_ifa));
        net_unlock();
        assert_ne!(ia.ia6_flags.get() & IN6_IFF_DUPLICATED, 0);
        assert!(nd6_dad_find(&ia.ia_ifa).is_none());
        reap(dp);

        // An anycast address skips DAD.
        let ia = fake_ifa6(ifp, PEER6);
        ia.ia6_flags.set(IN6_IFF_TENTATIVE | IN6_IFF_ANYCAST);
        net_lock();
        nd6_dad_start(&ia.ia_ifa);
        net_unlock();
        assert_eq!(ia.ia6_flags.get(), IN6_IFF_ANYCAST);
        assert!(nd6_dad_find(&ia.ia_ifa).is_none());
    }

    #[test]
    fn nd6_ifptomac_is_the_ethernet_address() {
        let (_g, ifp) = nd6_setup();
        assert_eq!(
            nd6_ifptomac(ifp),
            Some(crate::netinet::ip_input::tests::OURS)
        );
        let m = nd_packet(ifp, OURS6, PEER6, 255, &[], &[]).0;
        m_freem(m);
    }
}
/* </TESTS> */
