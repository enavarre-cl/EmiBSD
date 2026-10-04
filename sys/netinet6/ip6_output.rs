/*	$OpenBSD: ip6_output.c,v 1.309 2026/09/17 15:56:59 bluhm Exp $	*/
/*	$KAME: ip6_output.c,v 1.172 2001/03/25 09:55:56 itojun Exp $	*/
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

//! IPv6 output: `ip6_output` (extension headers, the route and path MTU, multicast loopback,
//! pf(4), IPsec and fragmentation), the IPv6-level socket options, the sticky and ancillary
//! packet options of RFC 3542, the IPv6 multicast options, the output checksums and the
//! fragment identifiers: `netinet6/ip6_output.c`.
//!
//! Upstream: sys/netinet6/ip6_output.c @ 3ce1f3f79392
//!
//! `ip6_output` takes a packet with a skeletal IPv6 header (flow, length, next header, hop
//! limit, source, destination), adds the extension headers of the packet options (hop-by-hop,
//! destination options before and after the routing header) in their own mbufs after the
//! header, finds the route and the interface (caching the route in a `struct route`), and
//! hands the packet to `if_output_tso`, fragmenting it with `ip6_fragment` when it is larger
//! than the path MTU.
//!
//! ## Deviations
//! - `ip6_output`'s `struct route *`, `struct ip6_pktopts *`, `struct ip6_moptions *` and
//!   `const struct ipsec_level *` are `Option`s; the error is a `Result`. The C's `goto
//!   freehdrs`/`bad`/`done` are nested labelled blocks, `reroute` a labelled loop.
//! - The IPv6 header is read and written as a copy (`ip6_var.rs`'s `mtod_ip6`/
//!   `mtod_ip6_store`): mbuf data has no alignment guarantee. The C's `nexthdrp` (the next
//!   header byte of the previous header) is the header copy or the first byte of an extension
//!   header mbuf. `ip6_insertfraghdr` returns the fragment header's address instead of
//!   writing it through `frghdrp`; `ip6_fragment` writes the header unaligned.
//! - `ip6_id_ctx` is a `StaticCell` under a private mutex, `ip6_id_mtx` (the C calls
//!   `idgen32` unlocked; here `ip6_randomid` can run on several CPUs under the shared net
//!   lock).
//! - The socket option values are byte slices (`u_char *buf, int len` in C) read with
//!   `from_ne_bytes`; the option mbufs are read and written unaligned. `ip6_setpktopts`
//!   walks its control mbuf by offsets. `ip6_pcbopt` and `ip6_setmoptions` take the pcb's
//!   `Cell` (the C's `struct ip6_pktopts **`/`struct ip6_moptions **`); their `malloc
//!   (M_WAITOK)` that cannot fail in C panics here if it does.
//! - `ip6_setmoptions`' `suser(curproc)` counts a missing `curproc` as unprivileged.
//! - `in6_proto_cksum_out` stores no pseudo-header checksum when `ip6_lasthdr` finds the
//!   chain invalid (the C would write at offset -1 plus the field offset).
//! - `NPF` (pf(4)) is configured: `pf_test(AF_INET6, ...)` filters the packet, and a
//!   packet pf tagged `PF_TAG_REROUTE` reruns the route lookup. `IPSEC` is configured: the
//!   SPD lookup, the loop detection, the path MTU update and `ipsp_process_packet` run with
//!   `AF_INET6`; their IPv6 halves in `netinet/ip_spd.rs` and `netinet/ipsec_output.rs` are
//!   still comments there (phase 2 of the INET6 port), so an IPv6 flow finds no policy yet.
//! - Not configured, a comment at its site: `MROUTING` (`ip6_mforward`).
//! - `KERNEL_LOCK()`/`KERNEL_UNLOCK()` are nothing without `MULTIPROCESSOR`.
//! - `ip6_optlen` and `ip6_copypktopts` of other BSDs do not exist in this OpenBSD:
//!   `copypktopts` is file-local here as in C.

use core::cell::Cell;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::crypto::idgen::{Idgen32Ctx, idgen32, idgen32_init};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_prot::suser;
use crate::kern::kern_tc::gettime;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{
    MAX_LINKHDR, m_align, m_copyback, m_copym, m_dup_pkthdr, m_free, m_freem, m_get, m_gethdr,
    m_pullup, m_trailingspace, ml_dequeue, ml_enqueue, ml_init, ml_purge,
};
use crate::kern::uipc_mbuf2::{m_tag_first, m_tag_next};
use crate::machine::cpu::curproc;
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_::{
    IFCAP_CSUM_TCPv6, IFCAP_CSUM_UDPv6, IFCAP_TSOv6, IFF_LOOPBACK, IFF_MULTICAST, if_get,
    if_input_local, if_output_ml, if_output_tso, if_put,
};
use crate::net::if_enc::enc_getif;
use crate::net::if_var::Ifnet;
use crate::net::pf::pf_test;
use crate::net::pfvar::{PF_FWD, PF_OUT, PF_PASS};
use crate::net::route::{
    RT_RESOLVE, RTF_GATEWAY, RTF_HOST, RTF_LOCAL, RTV_MTU, Route, Rtentry, route6_cache, rtalloc,
    rtfree, rtisvalid,
};
use crate::net::rtable::{rtable_l2, rtable_loindex};
use crate::netinet::icmp6::Icmp6Hdr;
use crate::netinet::in_::{
    IPPROTO_DSTOPTS, IPPROTO_FRAGMENT, IPPROTO_HOPOPTS, IPPROTO_ICMPV6, IPPROTO_IPV6, IPPROTO_TCP,
    IPPROTO_UDP, IPSEC_AUTH_LEVEL_DEFAULT, IPSEC_ESP_NETWORK_LEVEL_DEFAULT,
    IPSEC_ESP_TRANS_LEVEL_DEFAULT, IPSEC_IPCOMP_LEVEL_DEFAULT, IPSEC_LEVEL_BYPASS,
    IPSEC_LEVEL_UNIQUE,
};
use crate::netinet::in_pcb::{
    IN6P_AUTOFLOWLABEL, IN6P_DSTOPTS, IN6P_HIGHPORT, IN6P_HOPLIMIT, IN6P_HOPOPTS, IN6P_LOWPORT,
    IN6P_MTU, IN6P_PKTINFO, IN6P_RECVDSTPORT, IN6P_RTHDR, IN6P_TCLASS, in_pcbset_rtableid,
    sotoinpcb,
};
use crate::netinet::ip_input::ip_mtudisc;
use crate::netinet::ip_ipsp::{
    IPSEC_IN_USE, IPSP_DF_INHERIT, IPSP_DIRECTION_OUT, IpsecCounters, IpsecLevel, Tdb, TdbCounters,
    TdbIdent, ipsecstat_inc, tdb_unref, tdbstat_inc,
};
use crate::netinet::ip_output::in_ifcap_cksum;
use crate::netinet::ip_spd::{SpdError, ipsp_spd_lookup};
use crate::netinet::ip6::{
    IP6F_MORE_FRAG, IP6OPT_JUMBO, IP6OPT_PADN, IPV6_MAXPACKET, IPV6_MMTU, Ip6Dest, Ip6Ext, Ip6Frag,
    Ip6Hbh, Ip6Hdr,
};
use crate::netinet::ipsec_output::{ipsec_adjust_mtu, ipsp_process_packet};
use crate::netinet::tcp::Tcphdr;
use crate::netinet::tcp_output::tcp_softtso_chop;
use crate::netinet::tcp_var::{TcpstatCounters, tcpstat_inc};
use crate::netinet::udp::Udphdr;
use crate::netinet::udp_var::{UdpstatCounters, udpstat_inc};
use crate::netinet6::icmp6::icmp6_mtudisc_clone;
use crate::netinet6::in6::{
    IPSEC6_OUTSA, IPV6_AUTH_LEVEL, IPV6_AUTOFLOWLABEL, IPV6_CHECKSUM, IPV6_DEFAULT_MULTICAST_LOOP,
    IPV6_DONTFRAG, IPV6_DSTOPTS, IPV6_ESP_NETWORK_LEVEL, IPV6_ESP_TRANS_LEVEL, IPV6_HOPLIMIT,
    IPV6_HOPOPTS, IPV6_IPCOMP_LEVEL, IPV6_JOIN_GROUP, IPV6_LEAVE_GROUP, IPV6_MINHOPCOUNT,
    IPV6_MULTICAST_HOPS, IPV6_MULTICAST_IF, IPV6_MULTICAST_LOOP, IPV6_PATHMTU, IPV6_PIPEX,
    IPV6_PKTINFO, IPV6_PORTRANGE, IPV6_PORTRANGE_DEFAULT, IPV6_PORTRANGE_HIGH, IPV6_PORTRANGE_LOW,
    IPV6_RECVDSTOPTS, IPV6_RECVDSTPORT, IPV6_RECVHOPLIMIT, IPV6_RECVHOPOPTS, IPV6_RECVPATHMTU,
    IPV6_RECVPKTINFO, IPV6_RECVRTHDR, IPV6_RECVTCLASS, IPV6_RTHDR, IPV6_RTHDRDSTOPTS, IPV6_TCLASS,
    IPV6_UNICAST_HOPS, IPV6_USE_MIN_MTU, IPV6_V6ONLY, In6Addr, In6Pktinfo, Ip6Mtuinfo, Ipv6Mreq,
    SockaddrIn6, in6_addr2scopeid, in6_are_addr_equal, in6_hasmulti, in6_is_addr_mc_intfacelocal,
    in6_is_addr_mc_linklocal, in6_is_addr_multicast, in6_is_addr_unspecified, in6_is_scope_embed,
    in6_joingroup, in6_leavegroup, satosin6, sin6tosa_const,
};
use crate::netinet6::in6_cksum::in6_cksum;
use crate::netinet6::in6_pcb::in6_pcbrtentry;
use crate::netinet6::in6_proto::IP6_DEFMCASTHLIM;
use crate::netinet6::in6_src::{in6_embedscope, in6_selectroute};
use crate::netinet6::in6_var::{In6MultiMship, In6MultiMshipList};
use crate::netinet6::ip6_input::{ip6_lasthdr, ip6_process_hopopts};
use crate::netinet6::ip6_var::{
    IP6PO_DONTFRAG, IP6PO_MINMTU_ALL, IP6PO_MINMTU_DISABLE, IP6PO_MINMTU_MCASTONLY,
    IPV6_FORWARDING, IPV6_FORWARDING_IPSEC, IPV6_MINMTU, IPV6_UNSPECSRC, Ip6Moptions, Ip6Pktopts,
    Ip6statCounters, ip6stat_add, ip6stat_inc, mtod_ip6, mtod_ip6_store,
};
use crate::sys::endian::{htonl, htons, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_IP6OPT, M_IPMOPTS, M_NOWAIT, M_WAITOK};
use crate::sys::mbuf::{
    M_BCAST, M_COPYALL, M_DONTWAIT, M_EXT, M_ICMP_CSUM_OUT, M_IPV6_DF_OUT, M_MCAST, M_TCP_CSUM_OUT,
    M_TCP_TSO, M_UDP_CSUM_OUT, M_WAIT, MCLBYTES, MLEN, MT_DATA, MT_HEADER, Mbuf, MbufList,
    PACKET_TAG_IPSEC_IN_DONE, PACKET_TAG_IPSEC_OUT_DONE, PF_TAG_GENERATED, PF_TAG_REROUTE,
    m_move_pkthdr, mclget, ml_len, mtod,
};
use crate::sys::mutex::Mutex;
use crate::sys::protosw::{PRCO_GETOPT, PRCO_SETOPT};
use crate::sys::queue::ListHead;
use crate::sys::socket::{AF_INET6, Cmsghdr, SO_RTABLE, cmsg_align, cmsg_len};
use crate::sys::socketvar::{SS_ISCONNECTED, SS_PRIV, Socket};
use libkern::StaticCell;

/// `JUMBOOPTLEN`: length of the jumbo payload option and its padding.
const JUMBOOPTLEN: usize = 8;

/// `struct ip6_exthdrs`: the IPv6 header and the extension header mbufs of a packet being
/// built by `ip6_output`.
#[derive(Clone, Copy, Default)]
struct Ip6Exthdrs {
    /// `ip6e_ip6`: the mbuf holding the IPv6 header.
    ip6e_ip6: Option<&'static Mbuf>,
    /// `ip6e_hbh`: the hop-by-hop options header.
    ip6e_hbh: Option<&'static Mbuf>,
    /// `ip6e_dest1`: the destination options header before a routing header.
    ip6e_dest1: Option<&'static Mbuf>,
    /// `ip6e_dest2`: the destination options header after a routing header.
    ip6e_dest2: Option<&'static Mbuf>,
}

/// `ip6_id_mtx` (not in C, see the deviations): guards [`IP6_ID_CTX`].
static IP6_ID_MTX: Mutex = Mutex::new(IPL_SOFTNET);

/// `ip6_id_ctx`: context for non-repeating IDs; touched only under [`IP6_ID_MTX`].
static IP6_ID_CTX: StaticCell<Idgen32Ctx> = StaticCell::new(Idgen32Ctx::zeroed());

/// The first byte of `m`'s data: the next header field of an extension header.
fn mtod_byte(m: &Mbuf) -> u8 {
    // SAFETY: an extension header mbuf holds at least 8 bytes (`(ip6e_len + 1) << 3`).
    unsafe { mtod::<u8>(m).read() }
}

/// Stores `v` as the first byte of `m`'s data.
fn set_mtod_byte(m: &Mbuf, v: u8) {
    // SAFETY: as in `mtod_byte`.
    unsafe { mtod::<u8>(m).write(v) };
}

/// `MAKE_EXTHDR(hp, mp)`: copies the extension header at `hp` (if any) into a new mbuf.
///
/// # Safety
///
/// `hp` is `None` or points to a whole extension header, `(ip6e_len + 1) << 3` bytes.
unsafe fn make_exthdr(
    hp: Option<NonNull<u8>>,
    mp: &mut Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let Some(hp) = hp else {
        return Ok(());
    };
    // SAFETY: the caller's contract: an extension header starts with `struct ip6_ext`.
    let eh = unsafe { hp.cast::<Ip6Ext>().as_ptr().read_unaligned() };
    let hlen = (i32::from(eh.ip6e_len) + 1) << 3;
    // SAFETY: the caller's contract covers the `hlen` bytes.
    let hdr = unsafe { core::slice::from_raw_parts(hp.as_ptr(), hlen as usize) };
    ip6_copyexthdr(mp, Some(hdr), hlen)
}

/// `ip6_output`: IP6 output. The packet in mbuf chain `m` contains a skeletal IP6 header
/// (with pri, len, nxt, hlim, src, dst). This function may modify ver and hlim only. The
/// mbuf chain containing the packet will be freed. The options `opt`, if present, will not
/// be freed.
pub fn ip6_output(
    m0: &'static Mbuf,
    opt: Option<&Ip6Pktopts>,
    ro: Option<&Route>,
    flags: i32,
    im6o: Option<&Ip6Moptions>,
    seclevel: Option<&IpsecLevel>,
) -> Result<(), Errno> {
    // `m` is what `bad` frees; `mm` the packet while there is one.
    let mut m: Option<&'static Mbuf> = Some(m0);
    let mut mm: &'static Mbuf = m0;
    let mut ifp: Option<&'static Ifnet> = None;
    let ml = MbufList::new();
    let iproute = Route::new();
    let mut ro: Option<&Route> = ro;
    let mut ro_pmtu: Option<&Route> = None;
    let mut rt: Option<&'static Rtentry> = None;
    let mut src_scope: u16 = 0;
    let mut dst_scope: u16 = 0;
    let mut exthdrs = Ip6Exthdrs::default();
    let mut hdrsplit = false;
    let sproto: u8 = 0; // the C never sets it
    let mut tdb: Option<&'static Tdb> = None;

    let mut ip6 = mtod_ip6(m0);
    let mut finaldst = ip6.ip6_dst;

    let error: Result<(), Errno> = 'done: {
        let bad: Result<(), Errno> = 'bad: {
            let freehdrs: Result<(), Errno> = 'freehdrs: {
                if let Some(opt) = opt {
                    // Hop-by-Hop options header
                    // SAFETY: the options hold whole extension headers (`ip6_setpktopt`
                    // checked their lengths).
                    if let Err(e) = unsafe {
                        make_exthdr(opt.ip6po_hbh.map(NonNull::cast), &mut exthdrs.ip6e_hbh)
                    } {
                        break 'freehdrs Err(e);
                    }
                    // Destination options header(1st part)
                    // SAFETY: as above.
                    if let Err(e) = unsafe {
                        make_exthdr(opt.ip6po_dest1.map(NonNull::cast), &mut exthdrs.ip6e_dest1)
                    } {
                        break 'freehdrs Err(e);
                    }
                    // Destination options header(2nd part)
                    // SAFETY: as above.
                    if let Err(e) = unsafe {
                        make_exthdr(opt.ip6po_dest2.map(NonNull::cast), &mut exthdrs.ip6e_dest2)
                    } {
                        break 'freehdrs Err(e);
                    }
                }

                if IPSEC_IN_USE.load(Ordering::Relaxed) != 0 || seclevel.is_some() {
                    match ip6_output_ipsec_lookup(mm, seclevel) {
                        // -EINVAL: the packet should be silently dropped, typically because
                        // we've asked key management for an SA.
                        Err(SpdError::Drop) => break 'freehdrs Ok(()),
                        Err(SpdError::Errno(e)) => break 'freehdrs Err(e),
                        Ok(t) => tdb = t,
                    }
                }

                // Calculate the total length of the extension header chain. Keep the length
                // of the unfragmentable part for fragmentation.
                let mut optlen: u32 = 0;
                if let Some(h) = exthdrs.ip6e_hbh {
                    optlen += h.m_len().get();
                }
                if let Some(h) = exthdrs.ip6e_dest1 {
                    optlen += h.m_len().get();
                }
                let unfragpartlen = optlen + size_of::<Ip6Hdr>() as u32;
                // NOTE: we don't add AH/ESP length here. do that later.
                if let Some(h) = exthdrs.ip6e_dest2 {
                    optlen += h.m_len().get();
                }

                // If we need IPsec, or there is at least one extension header, separate IP6
                // header from the payload.
                if (sproto != 0 || optlen != 0) && !hdrsplit {
                    if let Err(e) = ip6_splithdr(mm, &mut exthdrs) {
                        m = None;
                        break 'freehdrs Err(e);
                    }
                    let Some(h) = exthdrs.ip6e_ip6 else {
                        break 'freehdrs Err(Errno::ENOBUFS);
                    };
                    mm = h;
                    m = Some(mm);
                    hdrsplit = true;
                }

                // adjust pointer
                ip6 = mtod_ip6(mm);

                // adjust mbuf packet header length
                let pkthdr = mm.m_pkthdr();
                pkthdr.len.set(pkthdr.len.get() + optlen as i32);
                let plen = (pkthdr.len.get() as u32).wrapping_sub(size_of::<Ip6Hdr>() as u32);

                // If this is a jumbo payload, insert a jumbo payload option.
                if plen as usize > IPV6_MAXPACKET {
                    if !hdrsplit {
                        if let Err(e) = ip6_splithdr(mm, &mut exthdrs) {
                            m = None;
                            break 'freehdrs Err(e);
                        }
                        let Some(h) = exthdrs.ip6e_ip6 else {
                            break 'freehdrs Err(Errno::ENOBUFS);
                        };
                        mm = h;
                        m = Some(mm);
                        hdrsplit = true;
                    }
                    // adjust pointer
                    ip6 = mtod_ip6(mm);
                    if let Err(e) = ip6_insert_jumboopt(&mut exthdrs, plen) {
                        break 'freehdrs Err(e);
                    }
                    ip6.ip6_plen = 0;
                } else {
                    ip6.ip6_plen = htons(plen as u16);
                }

                // Concatenate headers and fill in next header fields. Here we have, on "m"
                //	IPv6 payload
                // and we insert headers accordingly. Finally, we should be getting:
                //	IPv6 hbh dest1 rthdr ah* [esp* dest2 payload]
                //
                // During the header composing process, "m" points to IPv6 header. "mprev"
                // points to an extension header prior to esp.
                {
                    // `nexthdrp`: `None` is the header's `ip6_nxt`, `Some(h)` the first byte of
                    // extension header `h`.
                    let mut nexthdrp: Option<&'static Mbuf> = None;
                    let mut mprev = mm;

                    // We treat dest2 specially. This makes IPsec processing much easier. The
                    // goal here is to make mprev point the mbuf prior to dest2.
                    //
                    // result: IPv6 dest2 payload
                    // m and mprev will point to IPv6 header.
                    if let Some(d2) = exthdrs.ip6e_dest2 {
                        if !hdrsplit {
                            panic(format_args!("ip6_output: assumption failed: hdr not split"));
                        }
                        d2.m_next().set(mm.m_next().get());
                        mm.m_next().set(Some(d2));
                        set_mtod_byte(d2, ip6.ip6_nxt);
                        ip6.ip6_nxt = IPPROTO_DSTOPTS as u8;
                    }

                    // MAKE_CHAIN. result: IPv6 hbh dest1 rthdr dest2 payload
                    // m will point to IPv6 header. mprev will point to the extension header
                    // prior to dest2 (rthdr in the above case).
                    for (eh, proto) in [
                        (exthdrs.ip6e_hbh, IPPROTO_HOPOPTS),
                        (exthdrs.ip6e_dest1, IPPROTO_DSTOPTS),
                    ] {
                        let Some(eh) = eh else {
                            continue;
                        };
                        if !hdrsplit {
                            panic(format_args!("assumption failed: hdr not split"));
                        }
                        match nexthdrp {
                            None => {
                                set_mtod_byte(eh, ip6.ip6_nxt);
                                ip6.ip6_nxt = proto as u8;
                            }
                            Some(p) => {
                                set_mtod_byte(eh, mtod_byte(p));
                                set_mtod_byte(p, proto as u8);
                            }
                        }
                        nexthdrp = Some(eh);
                        eh.m_next().set(mprev.m_next().get());
                        mprev.m_next().set(Some(eh));
                        mprev = eh;
                    }
                }
                mtod_ip6_store(mm, &ip6);

                // Source address validation
                if flags & IPV6_UNSPECSRC == 0 && in6_is_addr_unspecified(&ip6.ip6_src) {
                    // XXX: we can probably assume validation in the caller, but we
                    // explicitly check the address here for safety.
                    ip6stat_inc(Ip6statCounters::Ip6sBadscope);
                    break 'bad Err(Errno::EOPNOTSUPP);
                }
                if in6_is_addr_multicast(&ip6.ip6_src) {
                    ip6stat_inc(Ip6statCounters::Ip6sBadscope);
                    break 'bad Err(Errno::EOPNOTSUPP);
                }

                ip6stat_inc(Ip6statCounters::Ip6sLocalout);

                // Route packet.
                let orig_rtableid = mm.m_pkthdr().ph_rtableid.get();
                // reroute: pf(4) asks for a new route lookup after it changed the packet.
                'reroute: loop {
                    // initialize cached route
                    let r: &Route = match ro {
                        Some(r) => r,
                        None => {
                            iproute.ro_rt.set(None);
                            &iproute
                        }
                    };
                    ro = Some(r);
                    ro_pmtu = Some(r);
                    let mut dst: *const SockaddrIn6 = r.ro_dstsa().cast();

                    // If specified, try to fill in the traffic class field. Do not override if
                    // a non-zero value is already set. We check the diffserv field and the ecn
                    // field separately.
                    if let Some(opt) = opt
                        && opt.ip6po_tclass >= 0
                    {
                        let mut mask = 0;
                        if ip6.ip6_flow & htonl(0xfc << 20) == 0 {
                            mask |= 0xfc;
                        }
                        if ip6.ip6_flow & htonl(0x03 << 20) == 0 {
                            mask |= 0x03;
                        }
                        if mask != 0 {
                            ip6.ip6_flow |= htonl(((opt.ip6po_tclass & mask) as u32) << 20);
                        }
                    }

                    // fill in or override the hop limit field, if necessary.
                    if let Some(opt) = opt
                        && opt.ip6po_hlim != -1
                    {
                        ip6.ip6_hlim = (opt.ip6po_hlim & 0xff) as u8;
                    } else if in6_is_addr_multicast(&ip6.ip6_dst) {
                        ip6.ip6_hlim = match im6o {
                            Some(im6o) => im6o.im6o_hlim,
                            None => IP6_DEFMCASTHLIM.load(Ordering::Relaxed) as u8,
                        };
                    }
                    mtod_ip6_store(mm, &ip6);

                    if let Some(t) = tdb {
                        // XXX what should we do if ip6_hlim == 0 and the packet gets
                        // tunneled? Callee frees mbuf.
                        break 'done ip6_output_ipsec_send(t, mm, Some(r), orig_rtableid, false);
                    }

                    if in6_is_addr_multicast(&ip6.ip6_dst) {
                        // If the caller specify the outgoing interface explicitly, use it.
                        if let Some(opt) = opt
                            && let Some(pi) = opt.ip6po_pktinfo
                        {
                            // SAFETY: the packet info is the options' own allocation
                            // (`ip6_setpktopt`), alive while the options are.
                            ifp = if_get(unsafe { pi.as_ptr().read() }.ipi6_ifindex);
                        }

                        if ifp.is_none()
                            && let Some(im6o) = im6o
                        {
                            ifp = if_get(u32::from(im6o.im6o_ifidx));
                        }
                    }

                    let i: &'static Ifnet = if let Some(i) = ifp {
                        let _ =
                            route6_cache(r, &ip6.ip6_dst, None, mm.m_pkthdr().ph_rtableid.get());
                        i
                    } else {
                        rt = in6_selectroute(&ip6.ip6_dst, opt, r, mm.m_pkthdr().ph_rtableid.get());
                        let Some(rte) = rt else {
                            ip6stat_inc(Ip6statCounters::Ip6sNoroute);
                            break 'bad Err(Errno::EHOSTUNREACH);
                        };
                        ifp = if rte.rt_flags.get() & RTF_LOCAL != 0 {
                            if_get(rtable_loindex(mm.m_pkthdr().ph_rtableid.get()))
                        } else {
                            if_get(rte.rt_ifidx.get())
                        };
                        // We aren't using rtisvalid() here because the UP/DOWN state machine
                        // is broken with some Ethernet drivers like em(4). As a result we
                        // might try to use an invalid cached route entry while an interface
                        // is being detached.
                        let Some(i) = ifp else {
                            ip6stat_inc(Ip6statCounters::Ip6sNoroute);
                            break 'bad Err(Errno::EHOSTUNREACH);
                        };
                        i
                    };

                    if let Some(rte) = rt
                        && rte.rt_flags.get() & RTF_GATEWAY != 0
                        && !in6_is_addr_multicast(&ip6.ip6_dst)
                    {
                        dst = satosin6(rte.rt_gateway.get()).cast_const();
                    }

                    if !in6_is_addr_multicast(&ip6.ip6_dst) {
                        // Unicast
                        mm.m_flags().set(mm.m_flags().get() & !(M_BCAST | M_MCAST)); // just in case
                    } else {
                        // Multicast
                        mm.m_flags().set((mm.m_flags().get() & !M_BCAST) | M_MCAST);

                        // Confirm that the outgoing interface supports multicast.
                        if i.if_flags.get() & IFF_MULTICAST == 0 {
                            ip6stat_inc(Ip6statCounters::Ip6sNoroute);
                            break 'bad Err(Errno::ENETUNREACH);
                        }

                        if im6o.is_none_or(|im6o| im6o.im6o_loop != 0)
                            && in6_hasmulti(&ip6.ip6_dst, i)
                        {
                            // If we belong to the destination multicast group on the outgoing
                            // interface, and the caller did not forbid loopback, loop back a
                            // copy. Can't defer TCP/UDP checksumming, do the computation now.
                            in6_proto_cksum_out(mm, None);
                            // SAFETY: `dst` is the route's destination `sockaddr_in6`, read
                            // as a copy.
                            let d = unsafe { ptr::read_unaligned(dst) };
                            ip6_mloopback(i, mm, &d);
                        }
                        // MROUTING: else, when acting as a multicast router (ip6_mforwarding,
                        // ip6_mrouter_active, not IPV6_FORWARDING), ip6_mforward() under
                        // KERNEL_LOCK, `goto bad` if it fails; not configured.

                        // Multicasts with a hoplimit of zero may be looped back, above, but
                        // must not be transmitted on a network. Also, multicasts addressed to
                        // the loopback interface are not sent -- the above call to
                        // ip6_mloopback() will loop back a copy if this host actually belongs
                        // to the destination group on the loopback interface.
                        if ip6.ip6_hlim == 0
                            || i.if_flags.get() & IFF_LOOPBACK != 0
                            || in6_is_addr_mc_intfacelocal(&ip6.ip6_dst)
                        {
                            break 'bad Ok(());
                        }
                    }

                    // If this packet is going through a loopback interface we won't be able
                    // to restore its scope ID using the interface index.
                    if in6_is_scope_embed(&ip6.ip6_src) {
                        if i.if_flags.get() & IFF_LOOPBACK != 0 {
                            src_scope = ip6.ip6_src.s6_addr16(1);
                        }
                        ip6.ip6_src.set_s6_addr16(1, 0);
                    }
                    if in6_is_scope_embed(&ip6.ip6_dst) {
                        if i.if_flags.get() & IFF_LOOPBACK != 0 {
                            dst_scope = ip6.ip6_dst.s6_addr16(1);
                        }
                        ip6.ip6_dst.set_s6_addr16(1, 0);
                    }
                    mtod_ip6_store(mm, &ip6);

                    // Determine path MTU.
                    let mut mtu: u64 = 0;
                    let pmtu_rt = ro_pmtu.and_then(|r| r.ro_rt.get());
                    if let Err(e) = ip6_getpmtu(pmtu_rt, i, &mut mtu) {
                        break 'bad Err(e);
                    }

                    // The caller of this function may specify to use the minimum MTU in some
                    // cases. An advanced API option (IPV6_USE_MIN_MTU) can also override MTU
                    // setting. The logic is a bit complicated; by default, unicast packets
                    // will follow path MTU while multicast packets will be sent at the
                    // minimum MTU. If IP6PO_MINMTU_ALL is specified, all packets including
                    // unicast ones will be sent at the minimum MTU. Multicast packets will
                    // always be sent at the minimum MTU unless IP6PO_MINMTU_DISABLE is
                    // explicitly specified. See RFC 3542 for more details.
                    if mtu > u64::from(IPV6_MMTU)
                        && (flags & IPV6_MINMTU != 0
                            || opt.is_some_and(|o| o.ip6po_minmtu == IP6PO_MINMTU_ALL)
                            || (in6_is_addr_multicast(&ip6.ip6_dst)
                                && opt.is_none_or(|o| o.ip6po_minmtu != IP6PO_MINMTU_DISABLE)))
                    {
                        mtu = u64::from(IPV6_MMTU);
                    }

                    // If the outgoing packet contains a hop-by-hop options header, it must be
                    // examined and processed even by the source node. (RFC 2460, section 4.)
                    if let Some(hbhm) = exthdrs.ip6e_hbh {
                        // SAFETY: the hop-by-hop mbuf holds the whole header, at least 8
                        // bytes.
                        let hbh = unsafe { mtod::<Ip6Hbh>(hbhm).read_unaligned() };
                        let mut rtalert: u32 = 0; // returned value is ignored
                        let mut plen: u32 = 0; // no more than 1 jumbo payload option!

                        mm.m_pkthdr().ph_ifidx.set(i.if_index.get());
                        // SAFETY: the options follow the 2-byte `struct ip6_hbh` in the same
                        // mbuf, `((ip6h_len + 1) << 3) - 2` bytes of them.
                        let ok = unsafe {
                            ip6_process_hopopts(
                                &mut m,
                                mtod::<u8>(hbhm).add(size_of::<Ip6Hbh>()),
                                ((i32::from(hbh.ip6h_len) + 1) << 3) - size_of::<Ip6Hbh>() as i32,
                                &mut rtalert,
                                &mut plen,
                            )
                        };
                        let Some(m2) = m.filter(|_| ok) else {
                            // m was already freed at this point
                            break 'done Err(Errno::EINVAL); // better error?
                        };
                        mm = m2;
                        mm.m_pkthdr().ph_ifidx.set(0);
                    }

                    // Packet filter.
                    if pf_test(AF_INET6, PF_OUT, i, &mut m) != PF_PASS {
                        // `goto bad` frees `m` as pf_test left it.
                        break 'bad Err(Errno::EACCES);
                    }
                    let Some(m2) = m else {
                        break 'done Ok(());
                    };
                    mm = m2;
                    ip6 = mtod_ip6(mm);
                    let pf = &mm.m_pkthdr().pf;
                    if pf.flags.get() & (PF_TAG_REROUTE | PF_TAG_GENERATED)
                        == (PF_TAG_REROUTE | PF_TAG_GENERATED)
                    {
                        // already rerun the route lookup, go on
                        pf.flags
                            .set(pf.flags.get() & !(PF_TAG_GENERATED | PF_TAG_REROUTE));
                    } else if pf.flags.get() & PF_TAG_REROUTE != 0 {
                        // tag as generated to skip over pf_test on rerun
                        pf.flags.set(pf.flags.get() | PF_TAG_GENERATED);
                        finaldst = ip6.ip6_dst;
                        if ptr::eq(r, &iproute) {
                            rtfree(r.ro_rt.get());
                        }
                        ro = None;
                        if_put(ifp); // drop reference since destination changed
                        ifp = None;
                        continue 'reroute;
                    }

                    if flags & IPV6_FORWARDING != 0
                        && flags & IPV6_FORWARDING_IPSEC != 0
                        && mm.m_pkthdr().ph_tagsset.get() & PACKET_TAG_IPSEC_IN_DONE == 0
                    {
                        break 'bad Err(Errno::EHOSTUNREACH);
                    }

                    // If the packet is not going on the wire it can be destined to any local
                    // address. In this case do not clear its scopes to let ip6_input() find a
                    // matching local route.
                    if i.if_flags.get() & IFF_LOOPBACK != 0 {
                        if in6_is_scope_embed(&ip6.ip6_src) {
                            ip6.ip6_src.set_s6_addr16(1, src_scope);
                        }
                        if in6_is_scope_embed(&ip6.ip6_dst) {
                            ip6.ip6_dst.set_s6_addr16(1, dst_scope);
                        }
                        mtod_ip6_store(mm, &ip6);
                    }

                    // Send the packet to the outgoing interface. If necessary, do IPv6
                    // fragmentation before sending.
                    //
                    // the logic here is rather complex:
                    // 1: normal case (dontfrag == 0)
                    // 1-a: send as is if tlen <= path mtu
                    // 1-b: fragment if tlen > path mtu
                    //
                    // 2: if user asks us not to fragment (dontfrag == 1)
                    // 2-a: send as is if tlen <= interface mtu
                    // 2-b: error if tlen > interface mtu
                    let ph = mm.m_pkthdr();
                    let tlen: u32 = if ph.csum_flags.get() & M_TCP_TSO != 0 {
                        u32::from(ph.ph_mss.get())
                    } else {
                        ph.len.get() as u32
                    };

                    let dontfrag = if ph.csum_flags.get() & M_IPV6_DF_OUT != 0 {
                        ph.csum_flags.set(ph.csum_flags.get() & !M_IPV6_DF_OUT);
                        true
                    } else {
                        opt.is_some_and(|o| o.ip6po_flags & IP6PO_DONTFRAG != 0)
                    };

                    if dontfrag && tlen > i.if_mtu.get() {
                        // case 2-b
                        if ip_mtudisc.load(Ordering::Relaxed) != 0 {
                            ipsec_adjust_mtu(mm, mtu as u32);
                        }
                        break 'bad Err(Errno::EMSGSIZE);
                    }

                    // transmit packet without fragmentation
                    if dontfrag || u64::from(tlen) <= mtu {
                        // case 1-a and 2-a
                        // SAFETY: `dst` is the route's destination or gateway `sockaddr_in6`,
                        // readable for the call.
                        let error = unsafe {
                            if_output_tso(
                                i,
                                &mut m,
                                sin6tosa_const(dst),
                                r.ro_rt.get(),
                                i.if_mtu.get(),
                            )
                        };
                        if error.is_err() || m.is_none() {
                            break 'done error;
                        }
                        break 'bad Ok(()); // should not happen
                    }

                    // try to fragment the packet. case 1-b
                    if mtu < u64::from(IPV6_MMTU) {
                        // path MTU cannot be less than IPV6_MMTU
                        break 'bad Err(Errno::EMSGSIZE);
                    } else if ip6.ip6_plen == 0 {
                        // jumbo payload cannot be fragmented
                        break 'bad Err(Errno::EMSGSIZE);
                    }

                    // Too large for the destination or interface; fragment if possible. Must
                    // be able to put at least 8 bytes per fragment.
                    let hlen = unfragpartlen as i32;
                    if mtu > IPV6_MAXPACKET as u64 {
                        mtu = IPV6_MAXPACKET as u64;
                    }

                    // If we are doing fragmentation, we can't defer TCP/UDP checksumming;
                    // compute the checksum and clear the flag.
                    in6_proto_cksum_out(mm, None);

                    // Change the next header field of the last header in the unfragmentable
                    // part.
                    let nextproto = if let Some(d1) = exthdrs.ip6e_dest1 {
                        let n = mtod_byte(d1);
                        set_mtod_byte(d1, IPPROTO_FRAGMENT as u8);
                        n
                    } else if let Some(h) = exthdrs.ip6e_hbh {
                        let n = mtod_byte(h);
                        set_mtod_byte(h, IPPROTO_FRAGMENT as u8);
                        n
                    } else {
                        let n = ip6.ip6_nxt;
                        ip6.ip6_nxt = IPPROTO_FRAGMENT as u8;
                        mtod_ip6_store(mm, &ip6);
                        n
                    };

                    // ip6_fragment consumes the packet.
                    if let Err(e) = ip6_fragment(mm, &ml, hlen, nextproto, mtu) {
                        break 'done Err(e);
                    }
                    // SAFETY: as for `if_output_tso`.
                    let sent = unsafe { if_output_ml(i, &ml, sin6tosa_const(dst), r.ro_rt.get()) };
                    if let Err(e) = sent {
                        break 'done Err(e);
                    }
                    ip6stat_inc(Ip6statCounters::Ip6sFragmented);
                    break 'done Ok(());
                }
            };
            // freehdrs: m_freem will check if mbuf is 0
            m_freem(exthdrs.ip6e_hbh);
            m_freem(exthdrs.ip6e_dest1);
            m_freem(exthdrs.ip6e_dest2);
            freehdrs
        };
        // bad:
        m_freem(m);
        bad
    };
    // done:
    if ro.is_some_and(|r| ptr::eq(r, &iproute)) || ro_pmtu.is_some_and(|r| ptr::eq(r, &iproute)) {
        rtfree(iproute.ro_rt.get());
    }
    if_put(ifp);
    tdb_unref(tdb);
    let _ = finaldst; // the C keeps the final destination; nothing reads it
    error
}

/// `ip6_fragment`: splits `m0` (unfragmentable part `hlen` bytes, next header `nextproto`)
/// into fragments for `mtu` on `ml`. Consumes the packet; on error `ml` is purged.
pub fn ip6_fragment(
    m0: &'static Mbuf,
    ml: &MbufList,
    hlen: i32,
    nextproto: u8,
    mtu: u64,
) -> Result<(), Errno> {
    ml_init(ml);

    let ip6 = mtod_ip6(m0);
    let tlen = m0.m_pkthdr().len.get();
    let mut len = (mtu
        .wrapping_sub(hlen as u64)
        .wrapping_sub(size_of::<Ip6Frag>() as u64)
        & !7) as i32;

    let error = 'bad: {
        if len < 8 {
            break 'bad Errno::EMSGSIZE;
        }
        let id = htonl(ip6_randomid());

        // Loop through length of payload, make new header and copy data of each part and
        // link onto chain.
        let mut off = hlen;
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
            let mut mhip6 = ip6;
            m.m_len().set(size_of::<Ip6Hdr>() as u32);
            mtod_ip6_store(m, &mhip6);

            let ip6f = match ip6_insertfraghdr(m0, m, hlen) {
                Ok(p) => p,
                Err(e) => break 'bad e,
            };
            let mut offlg = htons(((off - hlen) & !7) as u16);
            if off + len >= tlen {
                len = tlen - off;
            } else {
                offlg |= IP6F_MORE_FRAG;
            }

            m.m_pkthdr()
                .len
                .set(hlen + size_of::<Ip6Frag>() as i32 + len);
            mhip6.ip6_plen = htons((m.m_pkthdr().len.get() - size_of::<Ip6Hdr>() as i32) as u16);
            mtod_ip6_store(m, &mhip6);
            let mut mlast = m;
            while let Some(n) = mlast.m_next().get() {
                mlast = n;
            }
            let Some(n) = m_copym(m0, off, len, M_DONTWAIT) else {
                break 'bad Errno::ENOBUFS;
            };
            mlast.m_next().set(Some(n));

            let frag = Ip6Frag {
                ip6f_nxt: nextproto,
                ip6f_reserved: 0,
                ip6f_offlg: offlg,
                ip6f_ident: id,
            };
            // SAFETY: `ip6_insertfraghdr` reserved `sizeof(struct ip6_frag)` bytes there.
            unsafe { ip6f.cast::<Ip6Frag>().write_unaligned(frag) };
            off += len;
        }

        ip6stat_add(Ip6statCounters::Ip6sOfragments, u64::from(ml_len(ml)));
        m_freem(m0);
        return Ok(());
    };
    // bad:
    ip6stat_inc(Ip6statCounters::Ip6sOdropped);
    let _ = ml_purge(ml);
    m_freem(m0);
    Err(error)
}

/// `ip6_copyexthdr`: a new mbuf holding the `hlen` bytes of extension header `hdr` (left
/// uninitialized without one), into `mp`.
fn ip6_copyexthdr(
    mp: &mut Option<&'static Mbuf>,
    hdr: Option<&[u8]>,
    hlen: i32,
) -> Result<(), Errno> {
    if hlen as usize > MCLBYTES {
        return Err(Errno::ENOBUFS); // XXX
    }

    let Some(m) = m_get(M_DONTWAIT, MT_DATA) else {
        return Err(Errno::ENOBUFS);
    };

    if hlen as usize > MLEN {
        mclget(m, M_DONTWAIT);
        if m.m_flags().get() & M_EXT == 0 {
            m_free(m);
            return Err(Errno::ENOBUFS);
        }
    }
    m.m_len().set(hlen as u32);
    if let Some(hdr) = hdr {
        // SAFETY: the mbuf (or its cluster) holds `hlen` bytes, `hdr` is `hlen` long.
        unsafe { ptr::copy_nonoverlapping(hdr.as_ptr(), mtod::<u8>(m), hlen as usize) };
    }

    *mp = Some(m);
    Ok(())
}

/// `ip6_insert_jumboopt`: inserts the jumbo payload option for payload length `plen` into
/// the hop-by-hop options header, making one if there is none.
fn ip6_insert_jumboopt(exthdrs: &mut Ip6Exthdrs, plen: u32) -> Result<(), Errno> {
    // If there is no hop-by-hop options header, allocate new one. If there is one but it
    // doesn't have enough space to store the jumbo payload option, allocate a cluster to
    // store the whole options. Otherwise, use it to store the options.
    let optbuf: *mut u8 = match exthdrs.ip6e_hbh {
        None => {
            let Some(mopt) = m_get(M_DONTWAIT, MT_DATA) else {
                return Err(Errno::ENOBUFS);
            };
            mopt.m_len().set(JUMBOOPTLEN as u32);
            let optbuf = mtod::<u8>(mopt);
            // SAFETY: the mbuf holds `JUMBOOPTLEN` bytes.
            unsafe { optbuf.add(1).write(0) }; // = ((JUMBOOPTLEN) >> 3) - 1
            exthdrs.ip6e_hbh = Some(mopt);
            optbuf
        }
        Some(mut mopt) => {
            let optbuf: *mut u8;
            if (m_trailingspace(mopt) as usize) < JUMBOOPTLEN {
                // XXX assumption:
                // - exthdrs->ip6e_hbh is not referenced from places other than exthdrs.
                // - exthdrs->ip6e_hbh is not an mbuf chain.
                let oldoptlen = mopt.m_len().get() as usize;

                // XXX: give up if the whole (new) hbh header does not fit even in an mbuf
                // cluster.
                if oldoptlen + JUMBOOPTLEN > MCLBYTES {
                    return Err(Errno::ENOBUFS);
                }

                // As a consequence, we must always prepare a cluster at this point.
                let mut n = m_get(M_DONTWAIT, MT_DATA);
                if let Some(nn) = n {
                    mclget(nn, M_DONTWAIT);
                    if nn.m_flags().get() & M_EXT == 0 {
                        m_freem(nn);
                        n = None;
                    }
                }
                let Some(n) = n else {
                    return Err(Errno::ENOBUFS);
                };
                n.m_len().set((oldoptlen + JUMBOOPTLEN) as u32);
                // SAFETY: the cluster holds `MCLBYTES` bytes, more than the old options and
                // the jumbo option; distinct mbufs.
                unsafe { ptr::copy_nonoverlapping(mtod::<u8>(mopt), mtod::<u8>(n), oldoptlen) };
                // SAFETY: within the cluster, as above.
                optbuf = unsafe { mtod::<u8>(n).add(oldoptlen) };
                m_freem(mopt);
                mopt = n;
                exthdrs.ip6e_hbh = Some(n);
            } else {
                // SAFETY: the trailing space holds `JUMBOOPTLEN` more bytes.
                optbuf = unsafe { mtod::<u8>(mopt).add(mopt.m_len().get() as usize) };
                mopt.m_len().set(mopt.m_len().get() + JUMBOOPTLEN as u32);
            }
            // SAFETY: `optbuf` has `JUMBOOPTLEN` bytes, as above.
            unsafe {
                optbuf.write(IP6OPT_PADN);
                optbuf.add(1).write(0);
            }

            // Adjust the header length according to the pad and the jumbo payload option.
            // SAFETY: the hop-by-hop header starts the mbuf; byte 1 is `ip6h_len`.
            unsafe {
                let lenp = mtod::<u8>(mopt).add(offset_of!(Ip6Hbh, ip6h_len));
                lenp.write(lenp.read().wrapping_add((JUMBOOPTLEN >> 3) as u8));
            }
            optbuf
        }
    };

    // fill in the option.
    let v = htonl(plen + JUMBOOPTLEN as u32);
    // SAFETY: `optbuf` has `JUMBOOPTLEN` bytes (see above).
    unsafe {
        optbuf.add(2).write(IP6OPT_JUMBO);
        optbuf.add(3).write(4);
        ptr::copy_nonoverlapping(v.to_ne_bytes().as_ptr(), optbuf.add(4), 4);
    }

    // finally, adjust the packet header length
    if let Some(ip6m) = exthdrs.ip6e_ip6 {
        let ph = ip6m.m_pkthdr();
        ph.len.set(ph.len.get() + JUMBOOPTLEN as i32);
    }

    Ok(())
}

/// `ip6_insertfraghdr`: inserts a fragment header into fragment `m` and copies the
/// unfragmentable header portions (`hlen` bytes) of `m0`; returns where the fragment header
/// goes.
fn ip6_insertfraghdr(m0: &Mbuf, m: &'static Mbuf, hlen: i32) -> Result<*mut u8, Errno> {
    let n: &'static Mbuf = if hlen as usize > size_of::<Ip6Hdr>() {
        let Some(n) = m_copym(
            m0,
            size_of::<Ip6Hdr>() as i32,
            hlen - size_of::<Ip6Hdr>() as i32,
            M_DONTWAIT,
        ) else {
            return Err(Errno::ENOBUFS);
        };
        m.m_next().set(Some(n));
        n
    } else {
        m
    };

    // Search for the last mbuf of unfragmentable part.
    let mut mlast = n;
    while let Some(nx) = mlast.m_next().get() {
        mlast = nx;
    }

    if mlast.m_flags().get() & M_EXT == 0 && m_trailingspace(mlast) as usize >= size_of::<Ip6Frag>()
    {
        // use the trailing space of the last mbuf for fragment hdr
        // SAFETY: the trailing space holds a fragment header.
        let p = unsafe { mtod::<u8>(mlast).add(mlast.m_len().get() as usize) };
        mlast
            .m_len()
            .set(mlast.m_len().get() + size_of::<Ip6Frag>() as u32);
        let ph = m.m_pkthdr();
        ph.len.set(ph.len.get() + size_of::<Ip6Frag>() as i32);
        Ok(p)
    } else {
        // allocate a new mbuf for the fragment header
        let Some(mfrg) = m_get(M_DONTWAIT, MT_DATA) else {
            return Err(Errno::ENOBUFS);
        };
        mfrg.m_len().set(size_of::<Ip6Frag>() as u32);
        mlast.m_next().set(Some(mfrg));
        Ok(mtod::<u8>(mfrg))
    }
}

/// `ip6_getpmtu`: the path MTU through route `rt` (if any) and `ifp`, into `mtup`.
fn ip6_getpmtu(rt: Option<&Rtentry>, ifp: &Ifnet, mtup: &mut u64) -> Result<(), Errno> {
    let mtu = match rt {
        Some(rt) => {
            let rtmtu = rt.rt_mtu().load(Ordering::Relaxed);
            let mut mtu = rtmtu;
            if mtu == 0 {
                mtu = ifp.if_mtu.get();
            } else if mtu < IPV6_MMTU {
                // RFC8021 IPv6 Atomic Fragments Considered Harmful
                mtu = IPV6_MMTU;
            } else if mtu > ifp.if_mtu.get() {
                // The MTU on the route is larger than the MTU on the interface! This
                // shouldn't happen, unless the MTU of the interface has been changed after
                // the interface was brought up. Change the MTU in the route to match the
                // interface MTU (as long as the field isn't locked).
                mtu = ifp.if_mtu.get();
                if rt.rt_locks().get() & RTV_MTU == 0 {
                    let _ = rt.rt_mtu().compare_exchange(
                        rtmtu,
                        mtu,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    );
                }
            }
            mtu
        }
        None => ifp.if_mtu.get(),
    };

    *mtup = u64::from(mtu);
    Ok(())
}

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
    // SAFETY: the first mbuf holds `m_len` bytes at `m_data`, read while the caller borrows
    // it.
    unsafe { core::slice::from_raw_parts(mtod::<u8>(m), m.m_len().get() as usize) }
}

/// Copies `data` into option mbuf `m` (a cluster when it does not fit `MLEN`), as the C's
/// `getsockopt` paths do.
fn mbuf_store_opt(m: &'static Mbuf, data: &[u8]) -> Result<(), Errno> {
    if data.len() > MCLBYTES {
        return Err(Errno::EMSGSIZE); // XXX
    }
    if data.len() > MLEN {
        mclget(m, M_WAIT);
    }
    m.m_len().set(data.len() as u32);
    if !data.is_empty() {
        // SAFETY: the mbuf (or the cluster just added) holds `data.len()` bytes.
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), mtod::<u8>(m), data.len()) };
    }
    Ok(())
}

/// The bytes of an extension header kept in packet options: `(ip6e_len + 1) << 3` of them.
///
/// # Safety
///
/// `p` points to a whole extension header (an `ip6_setpktopt` copy) that outlives `'a`.
unsafe fn exthdr_bytes<'a>(p: NonNull<u8>) -> &'a [u8] {
    // SAFETY: the caller's contract: the header starts with `struct ip6_ext`.
    let eh = unsafe { p.cast::<Ip6Ext>().as_ptr().read_unaligned() };
    let len = (usize::from(eh.ip6e_len) + 1) << 3;
    // SAFETY: the caller's contract covers the whole header.
    unsafe { core::slice::from_raw_parts(p.as_ptr(), len) }
}

/// `ip6_ctloutput`: IP6 socket option processing.
pub fn ip6_ctloutput(
    op: i32,
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let Some(inp) = sotoinpcb(so) else {
        panic(format_args!("ip6_ctloutput: socket {:p} without inpcb", so));
    };
    let Some(p) = curproc() else {
        panic(format_args!("ip6_ctloutput: no curproc")); // For IPsec and rdomain
    };
    let mut error = Ok(());

    let privileged = so.so_state.get() & SS_PRIV != 0;
    let uproto = i32::from(so.so_proto.pr_protocol);

    if level != IPPROTO_IPV6 {
        return Err(Errno::EINVAL);
    }

    let rtableid = p.process().ps_rtableid.load(Ordering::Relaxed);

    match op {
        PRCO_SETOPT => match optname {
            // Use of some Hop-by-Hop options or some Destination options, might require
            // special privilege. That is, normal applications (without special privilege)
            // might be forbidden from setting certain options in outgoing packets, and might
            // never see certain options in received packets. [RFC 2292 Section 6]
            // KAME specific note: KAME prevents non-privileged users from sending or
            // receiving ANY hbh/dst options in order to avoid overhead of parsing options in
            // the kernel.
            IPV6_RECVHOPOPTS | IPV6_RECVDSTOPTS | IPV6_UNICAST_HOPS | IPV6_MINHOPCOUNT
            | IPV6_HOPLIMIT | IPV6_RECVPKTINFO | IPV6_RECVHOPLIMIT | IPV6_RECVRTHDR
            | IPV6_RECVPATHMTU | IPV6_RECVTCLASS | IPV6_V6ONLY | IPV6_AUTOFLOWLABEL
            | IPV6_RECVDSTPORT => 'opt: {
                if (optname == IPV6_RECVHOPOPTS || optname == IPV6_RECVDSTOPTS) && !privileged {
                    error = Err(Errno::EPERM);
                    break 'opt;
                }
                let Some(m) = m.filter(|m| m.m_len().get() as usize == size_of::<i32>()) else {
                    error = Err(Errno::EINVAL);
                    break 'opt;
                };
                let optval = mtod_int(m);
                // OPTSET
                let optset = |bit: i32| {
                    if optval != 0 {
                        inp.set_flags(bit);
                    } else {
                        inp.clear_flags(bit);
                    }
                };
                match optname {
                    IPV6_UNICAST_HOPS => {
                        if !(-1..256).contains(&optval) {
                            error = Err(Errno::EINVAL);
                        } else {
                            // -1 = kernel default
                            inp.inp_hops.set(optval);
                        }
                    }

                    IPV6_MINHOPCOUNT => {
                        if !(0..=255).contains(&optval) {
                            error = Err(Errno::EINVAL);
                        } else {
                            inp.inp_ip6_minhlim().set(optval as u8);
                        }
                    }

                    IPV6_RECVPKTINFO => optset(IN6P_PKTINFO),

                    IPV6_HOPLIMIT => {
                        error = ip6_pcbopt(
                            IPV6_HOPLIMIT,
                            &optval.to_ne_bytes(),
                            &inp.inp_outputopts6,
                            privileged,
                            uproto,
                        );
                    }

                    IPV6_RECVHOPLIMIT => optset(IN6P_HOPLIMIT),
                    IPV6_RECVHOPOPTS => optset(IN6P_HOPOPTS),
                    IPV6_RECVDSTOPTS => optset(IN6P_DSTOPTS),
                    IPV6_RECVRTHDR => optset(IN6P_RTHDR),

                    IPV6_RECVPATHMTU => {
                        // We ignore this option for TCP sockets. (RFC3542 leaves this case
                        // unspecified.)
                        if uproto != IPPROTO_TCP {
                            optset(IN6P_MTU);
                        }
                    }

                    IPV6_V6ONLY => {
                        // make setsockopt(IPV6_V6ONLY) available only prior to bind(2). see
                        // ipng mailing list, Jun 22 2001.
                        if inp.inp_lport.get() != 0
                            || !in6_is_addr_unspecified(&inp.inp_laddr6.get())
                        {
                            error = Err(Errno::EINVAL);
                        } else if optval == 0 {
                            // No support for IPv4-mapped addresses.
                            error = Err(Errno::EINVAL);
                        }
                    }
                    IPV6_RECVTCLASS => optset(IN6P_TCLASS),
                    IPV6_AUTOFLOWLABEL => optset(IN6P_AUTOFLOWLABEL),
                    _ => optset(IN6P_RECVDSTPORT), // IPV6_RECVDSTPORT
                }
            }

            IPV6_TCLASS | IPV6_DONTFRAG | IPV6_USE_MIN_MTU => {
                let Some(m) = m.filter(|m| m.m_len().get() as usize == size_of::<i32>()) else {
                    return Err(Errno::EINVAL);
                };
                let optval = mtod_int(m);
                error = ip6_pcbopt(
                    optname,
                    &optval.to_ne_bytes(),
                    &inp.inp_outputopts6,
                    privileged,
                    uproto,
                );
            }

            IPV6_PKTINFO | IPV6_HOPOPTS | IPV6_RTHDR | IPV6_DSTOPTS | IPV6_RTHDRDSTOPTS => {
                // new advanced API (RFC3542)
                if m.is_some_and(|m| m.m_next().get().is_some()) {
                    return Err(Errno::EINVAL); // XXX
                }
                let optbuf: &[u8] = m.map_or(&[], mtod_bytes);
                error = ip6_pcbopt(optname, optbuf, &inp.inp_outputopts6, privileged, uproto);
            }

            IPV6_MULTICAST_IF | IPV6_MULTICAST_HOPS | IPV6_MULTICAST_LOOP | IPV6_JOIN_GROUP
            | IPV6_LEAVE_GROUP => {
                error = ip6_setmoptions(optname, &inp.inp_moptions6, m, inp.inp_rtableid.get());
            }

            IPV6_PORTRANGE => {
                let Some(m) = m.filter(|m| m.m_len().get() as usize == size_of::<i32>()) else {
                    return Err(Errno::EINVAL);
                };
                match mtod_int(m) {
                    IPV6_PORTRANGE_DEFAULT => {
                        inp.clear_flags(IN6P_LOWPORT);
                        inp.clear_flags(IN6P_HIGHPORT);
                    }

                    IPV6_PORTRANGE_HIGH => {
                        inp.clear_flags(IN6P_LOWPORT);
                        inp.set_flags(IN6P_HIGHPORT);
                    }

                    IPV6_PORTRANGE_LOW => {
                        inp.clear_flags(IN6P_HIGHPORT);
                        inp.set_flags(IN6P_LOWPORT);
                    }

                    _ => error = Err(Errno::EINVAL),
                }
            }

            IPSEC6_OUTSA => error = Err(Errno::EINVAL),

            IPV6_AUTH_LEVEL
            | IPV6_ESP_TRANS_LEVEL
            | IPV6_ESP_NETWORK_LEVEL
            | IPV6_IPCOMP_LEVEL => 'level: {
                let Some(m) = m.filter(|m| m.m_len().get() as usize == size_of::<i32>()) else {
                    error = Err(Errno::EINVAL);
                    break 'level;
                };
                let optval = mtod_int(m);

                if !(IPSEC_LEVEL_BYPASS..=IPSEC_LEVEL_UNIQUE).contains(&optval) {
                    error = Err(Errno::EINVAL);
                    break 'level;
                }

                let default = match optname {
                    IPV6_AUTH_LEVEL => IPSEC_AUTH_LEVEL_DEFAULT,
                    IPV6_ESP_TRANS_LEVEL => IPSEC_ESP_TRANS_LEVEL_DEFAULT,
                    IPV6_ESP_NETWORK_LEVEL => IPSEC_ESP_NETWORK_LEVEL_DEFAULT,
                    _ => IPSEC_IPCOMP_LEVEL_DEFAULT, // IPV6_IPCOMP_LEVEL
                };
                if optval < default && suser(p).is_err() {
                    error = Err(Errno::EACCES);
                    break 'level;
                }
                let mut sl = inp.inp_seclevel.get();
                let level = optval as u8;
                match optname {
                    IPV6_AUTH_LEVEL => sl.sl_auth = level,
                    IPV6_ESP_TRANS_LEVEL => sl.sl_esp_trans = level,
                    IPV6_ESP_NETWORK_LEVEL => sl.sl_esp_network = level,
                    _ => sl.sl_ipcomp = level, // IPV6_IPCOMP_LEVEL
                }
                inp.inp_seclevel.set(sl);
            }

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
            IPV6_PIPEX => match m.filter(|m| m.m_len().get() as usize == size_of::<i32>()) {
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
                IPV6_RECVHOPOPTS | IPV6_RECVDSTOPTS | IPV6_UNICAST_HOPS | IPV6_MINHOPCOUNT
                | IPV6_RECVPKTINFO | IPV6_RECVHOPLIMIT | IPV6_RECVRTHDR | IPV6_RECVPATHMTU
                | IPV6_V6ONLY | IPV6_PORTRANGE | IPV6_RECVTCLASS | IPV6_AUTOFLOWLABEL
                | IPV6_RECVDSTPORT => {
                    // OPTBIT
                    let optbit = |bit: i32| i32::from(inp.has_flags(bit));
                    let optval = match optname {
                        IPV6_RECVHOPOPTS => optbit(IN6P_HOPOPTS),
                        IPV6_RECVDSTOPTS => optbit(IN6P_DSTOPTS),
                        IPV6_UNICAST_HOPS => inp.inp_hops.get(),
                        IPV6_MINHOPCOUNT => i32::from(inp.inp_ip6_minhlim().get()),
                        IPV6_RECVPKTINFO => optbit(IN6P_PKTINFO),
                        IPV6_RECVHOPLIMIT => optbit(IN6P_HOPLIMIT),
                        IPV6_RECVRTHDR => optbit(IN6P_RTHDR),
                        IPV6_RECVPATHMTU => optbit(IN6P_MTU),
                        IPV6_V6ONLY => 1,
                        IPV6_PORTRANGE => {
                            if inp.has_flags(IN6P_HIGHPORT) {
                                IPV6_PORTRANGE_HIGH
                            } else if inp.has_flags(IN6P_LOWPORT) {
                                IPV6_PORTRANGE_LOW
                            } else {
                                0
                            }
                        }
                        IPV6_RECVTCLASS => optbit(IN6P_TCLASS),
                        IPV6_AUTOFLOWLABEL => optbit(IN6P_AUTOFLOWLABEL),
                        _ => optbit(IN6P_RECVDSTPORT), // IPV6_RECVDSTPORT
                    };
                    set_mtod_int(m, optval);
                }

                IPV6_PATHMTU => {
                    let mut pmtu: u64 = 0;

                    if so.so_state.get() & SS_ISCONNECTED == 0 {
                        return Err(Errno::ENOTCONN);
                    }

                    let rt = in6_pcbrtentry(inp);
                    let Some(rt) = rt.filter(|&rt| rtisvalid(Some(rt))) else {
                        return Err(Errno::EHOSTUNREACH);
                    };

                    let Some(ifp) = if_get(rt.rt_ifidx.get()) else {
                        return Err(Errno::EHOSTUNREACH);
                    };
                    // XXX: we dot not consider the case of source routing, or optional
                    // information to specify the outgoing interface.
                    error = ip6_getpmtu(Some(rt), ifp, &mut pmtu);
                    if_put(Some(ifp));
                    if error.is_ok() {
                        if pmtu > IPV6_MAXPACKET as u64 {
                            pmtu = IPV6_MAXPACKET as u64;
                        }

                        let mtuinfo = Ip6Mtuinfo {
                            ip6m_mtu: pmtu as u32,
                            ..Ip6Mtuinfo::default()
                        };
                        // SAFETY: `struct ip6_mtuinfo` is plain repr(C) data without padding
                        // (a 28-byte sockaddr_in6 and a u_int32_t), read as its bytes.
                        let optdata = unsafe {
                            core::slice::from_raw_parts(
                                ptr::from_ref(&mtuinfo).cast::<u8>(),
                                size_of::<Ip6Mtuinfo>(),
                            )
                        };
                        mbuf_store_opt(m, optdata)?;
                    }
                }

                IPV6_PKTINFO | IPV6_HOPOPTS | IPV6_RTHDR | IPV6_DSTOPTS | IPV6_RTHDRDSTOPTS
                | IPV6_TCLASS | IPV6_DONTFRAG | IPV6_USE_MIN_MTU => {
                    // SAFETY: the options are `ip6_pcbopt`'s allocation, owned by this
                    // control block and changed only under the socket lock the caller holds.
                    let opts = inp.inp_outputopts6.get().map(|o| unsafe { &*o.as_ptr() });
                    error = ip6_getpcbopt(opts, optname, m);
                }

                IPV6_MULTICAST_IF | IPV6_MULTICAST_HOPS | IPV6_MULTICAST_LOOP | IPV6_JOIN_GROUP
                | IPV6_LEAVE_GROUP => {
                    // SAFETY: the options are `ip6_setmoptions`' allocation, owned by this
                    // control block and changed only under the socket lock the caller holds.
                    let im6o = inp.inp_moptions6.get().map(|o| unsafe { &*o.as_ptr() });
                    error = ip6_getmoptions(optname, im6o, m);
                }

                IPSEC6_OUTSA => error = Err(Errno::EINVAL),

                IPV6_AUTH_LEVEL
                | IPV6_ESP_TRANS_LEVEL
                | IPV6_ESP_NETWORK_LEVEL
                | IPV6_IPCOMP_LEVEL => {
                    let sl = inp.inp_seclevel.get();
                    let optval = match optname {
                        IPV6_AUTH_LEVEL => sl.sl_auth,
                        IPV6_ESP_TRANS_LEVEL => sl.sl_esp_trans,
                        IPV6_ESP_NETWORK_LEVEL => sl.sl_esp_network,
                        _ => sl.sl_ipcomp, // IPV6_IPCOMP_LEVEL
                    };
                    set_mtod_int(m, i32::from(optval));
                }
                SO_RTABLE => set_mtod_int(m, inp.inp_rtableid.get() as i32),
                IPV6_PIPEX => set_mtod_int(m, inp.inp_pipex.get()),

                _ => error = Err(Errno::ENOPROTOOPT),
            }
        }

        _ => {}
    }
    error
}

/// `ip6_raw_ctloutput`: the raw-socket IPv6 options (`IPV6_CHECKSUM`).
pub fn ip6_raw_ctloutput(
    op: i32,
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let icmp6off = offset_of!(Icmp6Hdr, icmp6_cksum) as i32;
    let Some(inp) = sotoinpcb(so) else {
        panic(format_args!(
            "ip6_raw_ctloutput: socket {:p} without inpcb",
            so
        ));
    };

    if level != IPPROTO_IPV6 {
        return Err(Errno::EINVAL);
    }

    let mut error = Ok(());
    match optname {
        IPV6_CHECKSUM => {
            // For ICMPv6 sockets, no modification allowed for checksum offset, permit "no
            // change" values to help existing apps.
            //
            // RFC3542 says: "An attempt to set IPV6_CHECKSUM for an ICMPv6 socket will
            // fail." The current behavior does not meet RFC3542.
            match op {
                PRCO_SETOPT => {
                    let Some(m) = m.filter(|m| m.m_len().get() as usize == size_of::<i32>()) else {
                        return Err(Errno::EINVAL);
                    };
                    let optval = mtod_int(m);
                    if optval < -1 || (optval > 0 && optval % 2 != 0) {
                        // The API assumes non-negative even offset values or -1 as a special
                        // value.
                        error = Err(Errno::EINVAL);
                    } else if i32::from(so.so_proto.pr_protocol) == IPPROTO_ICMPV6 {
                        if optval != icmp6off {
                            error = Err(Errno::EINVAL);
                        }
                    } else {
                        inp.inp_cksum6.set(optval);
                    }
                }

                PRCO_GETOPT => {
                    let Some(m) = m else {
                        return Err(Errno::EINVAL);
                    };
                    let optval = if i32::from(so.so_proto.pr_protocol) == IPPROTO_ICMPV6 {
                        icmp6off
                    } else {
                        inp.inp_cksum6.get()
                    };
                    set_mtod_int(m, optval);
                }

                _ => error = Err(Errno::EINVAL),
            }
        }

        _ => error = Err(Errno::ENOPROTOOPT),
    }

    error
}

/// `ip6_initpktopts`: initializes packet options. Beware that there are non-zero default
/// values in the struct.
pub fn ip6_initpktopts(opt: &mut Ip6Pktopts) {
    *opt = Ip6Pktopts::default();
    opt.ip6po_hlim = -1; // -1 means default hop limit
    opt.ip6po_tclass = -1; // -1 means default traffic class
    opt.ip6po_minmtu = IP6PO_MINMTU_MCASTONLY;
}

/// `ip6_pcbopt`: sets sticky option `optname` from `buf` in the pcb's packet options,
/// allocating them on first use.
fn ip6_pcbopt(
    optname: i32,
    buf: &[u8],
    pktopt: &Cell<Option<NonNull<Ip6Pktopts>>>,
    priv_: bool,
    uproto: i32,
) -> Result<(), Errno> {
    let opt = match pktopt.get() {
        Some(o) => o,
        None => {
            let Some(o) = malloc(size_of::<Ip6Pktopts>(), M_IP6OPT, M_WAITOK) else {
                panic(format_args!("ip6_pcbopt: malloc(M_WAITOK)"));
            };
            let o = o.cast::<Ip6Pktopts>();
            // SAFETY: a fresh allocation of the structure's size, written once.
            unsafe { o.as_ptr().write(Ip6Pktopts::default()) };
            // SAFETY: as above; nothing else references it yet.
            ip6_initpktopts(unsafe { &mut *o.as_ptr() });
            pktopt.set(Some(o));
            o
        }
    };

    // SAFETY: the options are the control block's own allocation, changed here and in
    // `ip6_freepcbopts` only, under the socket lock the socket option calls hold.
    ip6_setpktopt(
        optname,
        buf,
        unsafe { &mut *opt.as_ptr() },
        priv_,
        true,
        uproto,
    )
}

/// `ip6_getpcbopt`: copies sticky option `optname` of `pktopt` (the defaults without
/// options) into option mbuf `m`.
fn ip6_getpcbopt(pktopt: Option<&Ip6Pktopts>, optname: i32, m: &'static Mbuf) -> Result<(), Errno> {
    let deftclass: i32 = 0;
    let defminmtu: i32 = IP6PO_MINMTU_MCASTONLY;
    let intbuf: [u8; size_of::<i32>()];
    let mut pktinfobuf = [0u8; size_of::<In6Pktinfo>()];

    let optdata: &[u8] = match optname {
        IPV6_PKTINFO => {
            // XXX: we don't have to do this every time...
            let pi = match pktopt.and_then(|o| o.ip6po_pktinfo) {
                // SAFETY: the packet info is the options' own allocation.
                Some(pi) => unsafe { pi.as_ptr().read() },
                None => In6Pktinfo::default(),
            };
            pktinfobuf[..16].copy_from_slice(&pi.ipi6_addr.s6_addr);
            pktinfobuf[16..].copy_from_slice(&pi.ipi6_ifindex.to_ne_bytes());
            &pktinfobuf
        }
        IPV6_TCLASS => {
            let v = match pktopt {
                Some(o) if o.ip6po_tclass >= 0 => o.ip6po_tclass,
                _ => deftclass,
            };
            intbuf = v.to_ne_bytes();
            &intbuf
        }
        IPV6_HOPOPTS => match pktopt.and_then(|o| o.ip6po_hbh) {
            // SAFETY: the header is the options' own whole copy.
            Some(h) => unsafe { exthdr_bytes(h.cast()) },
            None => &[],
        },
        IPV6_RTHDR => &[],
        IPV6_RTHDRDSTOPTS => match pktopt.and_then(|o| o.ip6po_dest1) {
            // SAFETY: as above.
            Some(h) => unsafe { exthdr_bytes(h.cast()) },
            None => &[],
        },
        IPV6_DSTOPTS => match pktopt.and_then(|o| o.ip6po_dest2) {
            // SAFETY: as above.
            Some(h) => unsafe { exthdr_bytes(h.cast()) },
            None => &[],
        },
        IPV6_USE_MIN_MTU => {
            intbuf = pktopt.map_or(defminmtu, |o| o.ip6po_minmtu).to_ne_bytes();
            &intbuf
        }
        IPV6_DONTFRAG => {
            let on = i32::from(pktopt.is_some_and(|o| o.ip6po_flags & IP6PO_DONTFRAG != 0));
            intbuf = on.to_ne_bytes();
            &intbuf
        }
        _ => {
            // should not happen
            #[cfg(feature = "diagnostic")]
            panic(format_args!("ip6_getpcbopt: unexpected option"));
            #[allow(unreachable_code)] // with `diagnostic`, the panic above
            return Err(Errno::ENOPROTOOPT);
        }
    };

    mbuf_store_opt(m, optdata)
}

/// `ip6_clearpktopts`: frees and clears option `optname` of `pktopt` (-1: all of them).
pub fn ip6_clearpktopts(pktopt: &mut Ip6Pktopts, optname: i32) {
    if optname == -1 || optname == IPV6_PKTINFO {
        if let Some(p) = pktopt.ip6po_pktinfo {
            free(p.cast(), M_IP6OPT, 0);
        }
        pktopt.ip6po_pktinfo = None;
    }
    if optname == -1 || optname == IPV6_HOPLIMIT {
        pktopt.ip6po_hlim = -1;
    }
    if optname == -1 || optname == IPV6_TCLASS {
        pktopt.ip6po_tclass = -1;
    }
    if optname == -1 || optname == IPV6_HOPOPTS {
        if let Some(p) = pktopt.ip6po_hbh {
            free(p.cast(), M_IP6OPT, 0);
        }
        pktopt.ip6po_hbh = None;
    }
    if optname == -1 || optname == IPV6_RTHDRDSTOPTS {
        if let Some(p) = pktopt.ip6po_dest1 {
            free(p.cast(), M_IP6OPT, 0);
        }
        pktopt.ip6po_dest1 = None;
    }
    if optname == -1 || optname == IPV6_DSTOPTS {
        if let Some(p) = pktopt.ip6po_dest2 {
            free(p.cast(), M_IP6OPT, 0);
        }
        pktopt.ip6po_dest2 = None;
    }
}

/// `PKTOPT_EXTHDRCPY`: a `malloc(M_IP6OPT)` copy of the extension header at `src`, if any;
/// `Err` when the allocation fails.
fn pktopt_exthdrcpy<T>(src: Option<NonNull<T>>) -> Result<Option<NonNull<T>>, ()> {
    let Some(src) = src else {
        return Ok(None);
    };
    // SAFETY: packet options hold whole extension header copies (`ip6_setpktopt`).
    let hdr = unsafe { exthdr_bytes(src.cast()) };
    let Some(dst) = malloc(hdr.len(), M_IP6OPT, M_NOWAIT) else {
        return Err(());
    };
    // SAFETY: `dst` is a fresh allocation of `hdr.len()` bytes.
    unsafe { ptr::copy_nonoverlapping(hdr.as_ptr(), dst.as_ptr(), hdr.len()) };
    Ok(Some(dst.cast()))
}

/// `copypktopts`: copies `src` into `dst` (initialized), duplicating its allocations.
fn copypktopts(dst: &mut Ip6Pktopts, src: &Ip6Pktopts) -> Result<(), Errno> {
    dst.ip6po_hlim = src.ip6po_hlim;
    dst.ip6po_tclass = src.ip6po_tclass;
    dst.ip6po_flags = src.ip6po_flags;
    let copied: Result<(), ()> = (|| {
        if let Some(pi) = src.ip6po_pktinfo {
            let p = malloc(size_of::<In6Pktinfo>(), M_IP6OPT, M_NOWAIT).ok_or(())?;
            let p = p.cast::<In6Pktinfo>();
            // SAFETY: a fresh allocation of the structure's size; `pi` is the source's own
            // packet info.
            unsafe { p.as_ptr().write(pi.as_ptr().read()) };
            dst.ip6po_pktinfo = Some(p);
        }
        dst.ip6po_hbh = pktopt_exthdrcpy(src.ip6po_hbh)?;
        dst.ip6po_dest1 = pktopt_exthdrcpy(src.ip6po_dest1)?;
        dst.ip6po_dest2 = pktopt_exthdrcpy(src.ip6po_dest2)?;
        Ok(())
    })();
    if copied.is_err() {
        // bad:
        ip6_clearpktopts(dst, -1);
        return Err(Errno::ENOBUFS);
    }
    Ok(())
}

/// `ip6_freepcbopts`: frees a pcb's packet options.
///
/// # Safety
///
/// `pktopt` is `None` or a `malloc(9)` allocation of the socket being torn down,
/// not used again.
pub unsafe fn ip6_freepcbopts(pktopt: Option<NonNull<Ip6Pktopts>>) {
    let Some(p) = pktopt else {
        return;
    };

    // SAFETY: the caller's contract: the options are ours alone now.
    ip6_clearpktopts(unsafe { &mut *p.as_ptr() }, -1);

    free(p.cast(), M_IP6OPT, 0);
}

/// `&'static` view of a membership on a multicast options list: memberships are
/// `in6_joingroup`'s allocations, alive until `in6_leavegroup` frees them.
fn imm_static(imm: &In6MultiMship) -> &'static In6MultiMship {
    // SAFETY: see above; the caller unlinks the membership before handing it to
    // `in6_leavegroup` and does not use it afterwards.
    unsafe { &*ptr::from_ref(imm) }
}

/// `ip6_setmoptions`: sets the IP6 multicast options in response to user setsockopt().
fn ip6_setmoptions(
    optname: i32,
    im6op: &Cell<Option<NonNull<Ip6Moptions>>>,
    m: Option<&Mbuf>,
    rtableid: u32,
) -> Result<(), Errno> {
    let mut error = Ok(());
    let ip6_defmcasthlim_local = IP6_DEFMCASTHLIM.load(Ordering::Relaxed);

    let im6o_ptr = match im6op.get() {
        Some(im6o) => im6o,
        None => {
            // No multicast option buffer attached to the pcb; allocate one and initialize to
            // default values.
            let Some(im6o) = malloc(size_of::<Ip6Moptions>(), M_IPMOPTS, M_WAITOK) else {
                return Err(Errno::ENOBUFS);
            };
            let im6o = im6o.cast::<Ip6Moptions>();
            // SAFETY: a fresh allocation of the structure's size, written once; the list head
            // stays in place there.
            unsafe {
                im6o.as_ptr().write(Ip6Moptions {
                    im6o_memberships: ListHead::new(),
                    im6o_ifidx: 0,
                    im6o_hlim: ip6_defmcasthlim_local as u8,
                    im6o_loop: IPV6_DEFAULT_MULTICAST_LOOP as u8,
                })
            };
            im6op.set(Some(im6o));
            im6o
        }
    };
    // SAFETY: the options are the control block's own allocation, changed here and in
    // ip6_freemoptions only, under the socket lock the socket option calls hold.
    let im6o = unsafe { &mut *im6o_ptr.as_ptr() };

    'out: {
        match optname {
            IPV6_MULTICAST_IF => {
                // Select the interface for outgoing multicast packets.
                let Some(m) = m.filter(|m| m.m_len().get() as usize == size_of::<u32>()) else {
                    error = Err(Errno::EINVAL);
                    break 'out;
                };
                let ifindex = mtod_int(m) as u32;
                if ifindex != 0 {
                    let Some(ifp) = if_get(ifindex) else {
                        error = Err(Errno::ENXIO); // XXX EINVAL?
                        break 'out;
                    };
                    if ifp.if_rdomain.get() != rtable_l2(rtableid)
                        || ifp.if_flags.get() & IFF_MULTICAST == 0
                    {
                        error = Err(Errno::EADDRNOTAVAIL);
                        if_put(Some(ifp));
                        break 'out;
                    }
                    if_put(Some(ifp));
                }
                im6o.im6o_ifidx = ifindex as u16;
            }

            IPV6_MULTICAST_HOPS => {
                // Set the IP6 hoplimit for outgoing multicast packets.
                let Some(m) = m.filter(|m| m.m_len().get() as usize == size_of::<i32>()) else {
                    error = Err(Errno::EINVAL);
                    break 'out;
                };
                let optval = mtod_int(m);
                if !(-1..256).contains(&optval) {
                    error = Err(Errno::EINVAL);
                } else if optval == -1 {
                    im6o.im6o_hlim = ip6_defmcasthlim_local as u8;
                } else {
                    im6o.im6o_hlim = optval as u8;
                }
            }

            IPV6_MULTICAST_LOOP => {
                // Set the loopback flag for outgoing multicast packets. Must be zero or one.
                let Some(m) = m.filter(|m| m.m_len().get() as usize == size_of::<u32>()) else {
                    error = Err(Errno::EINVAL);
                    break 'out;
                };
                let lp = mtod_int(m) as u32;
                if lp > 1 {
                    error = Err(Errno::EINVAL);
                    break 'out;
                }
                im6o.im6o_loop = lp as u8;
            }

            IPV6_JOIN_GROUP => {
                // Add a multicast group membership. Group must be a valid IP6 multicast
                // address.
                let Some(m) = m.filter(|m| m.m_len().get() as usize == size_of::<Ipv6Mreq>())
                else {
                    error = Err(Errno::EINVAL);
                    break 'out;
                };
                // SAFETY: the mbuf holds a `struct ipv6_mreq` (length checked above).
                let mut mreq = unsafe { mtod::<Ipv6Mreq>(m).read_unaligned() };
                if in6_is_addr_unspecified(&mreq.ipv6mr_multiaddr) {
                    // We use the unspecified address to specify to accept all multicast
                    // addresses. Only super user is allowed to do this.
                    if curproc().is_none_or(|p| suser(p).is_err()) {
                        error = Err(Errno::EACCES);
                        break 'out;
                    }
                } else if !in6_is_addr_multicast(&mreq.ipv6mr_multiaddr) {
                    error = Err(Errno::EINVAL);
                    break 'out;
                }

                // If no interface was explicitly specified, choose an appropriate one
                // according to the given multicast address.
                let ifp = if mreq.ipv6mr_interface == 0 {
                    let dst = SockaddrIn6::with_addr(mreq.ipv6mr_multiaddr);
                    // SAFETY: `dst` is a complete `sockaddr_in6` on the stack, readable for
                    // the call.
                    let rt = unsafe { rtalloc(sin6tosa_const(&dst), RT_RESOLVE, rtableid) };
                    let Some(rt) = rt else {
                        error = Err(Errno::EADDRNOTAVAIL);
                        break 'out;
                    };
                    let ifp = if_get(rt.rt_ifidx.get());
                    rtfree(Some(rt));
                    ifp
                } else {
                    // If the interface is specified, validate it.
                    let Some(ifp) = if_get(mreq.ipv6mr_interface) else {
                        error = Err(Errno::ENXIO); // XXX EINVAL?
                        break 'out;
                    };
                    Some(ifp)
                };

                // See if we found an interface, and confirm that it supports multicast
                let Some(ifp) = ifp.filter(|ifp| {
                    ifp.if_rdomain.get() == rtable_l2(rtableid)
                        && ifp.if_flags.get() & IFF_MULTICAST != 0
                }) else {
                    if_put(ifp);
                    error = Err(Errno::EADDRNOTAVAIL);
                    break 'out;
                };
                // Put interface index into the multicast address, if the address has
                // link/interface-local scope.
                if crate::netinet6::in6::in6_is_scope_embed(&mreq.ipv6mr_multiaddr) {
                    mreq.ipv6mr_multiaddr
                        .set_s6_addr16(1, htons(ifp.if_index.get() as u16));
                    // SAFETY: as above; the C changes the request in place.
                    unsafe { mtod::<Ipv6Mreq>(m).write_unaligned(mreq) };
                }
                // See if the membership already exists.
                let exists = im6o.im6o_memberships.iter().any(|imm| {
                    imm.i6mm_maddr.get().is_some_and(|in6m| {
                        in6m.in6m_ifidx().get() == ifp.if_index.get()
                            && in6_are_addr_equal(&in6m.in6m_addr(), &mreq.ipv6mr_multiaddr)
                    })
                });
                if exists {
                    if_put(Some(ifp));
                    error = Err(Errno::EADDRINUSE);
                    break 'out;
                }
                // Everything looks good; add a new record to the multicast address list for
                // the given interface.
                let imm = in6_joingroup(ifp, &mreq.ipv6mr_multiaddr);
                if_put(Some(ifp));
                match imm {
                    Ok(imm) => {
                        // SAFETY: a fresh membership, in no list; the head is in the malloc'd
                        // options, which stay in place while the list is not empty.
                        unsafe { im6o.im6o_memberships.insert_head(imm) };
                    }
                    Err(e) => error = Err(e),
                }
            }

            IPV6_LEAVE_GROUP => {
                // Drop a multicast group membership. Group must be a valid IP6 multicast
                // address.
                let Some(m) = m.filter(|m| m.m_len().get() as usize == size_of::<Ipv6Mreq>())
                else {
                    error = Err(Errno::EINVAL);
                    break 'out;
                };
                // SAFETY: the mbuf holds a `struct ipv6_mreq` (length checked above).
                let mut mreq = unsafe { mtod::<Ipv6Mreq>(m).read_unaligned() };
                if in6_is_addr_unspecified(&mreq.ipv6mr_multiaddr) {
                    if curproc().is_none_or(|p| suser(p).is_err()) {
                        error = Err(Errno::EACCES);
                        break 'out;
                    }
                } else if !in6_is_addr_multicast(&mreq.ipv6mr_multiaddr) {
                    error = Err(Errno::EINVAL);
                    break 'out;
                }

                // Put interface index into the multicast address, if the address has
                // link-local scope.
                if in6_is_addr_mc_linklocal(&mreq.ipv6mr_multiaddr) {
                    mreq.ipv6mr_multiaddr
                        .set_s6_addr16(1, htons(mreq.ipv6mr_interface as u16));
                    // SAFETY: as above; the C changes the request in place.
                    unsafe { mtod::<Ipv6Mreq>(m).write_unaligned(mreq) };
                }

                // If an interface address was specified, get a pointer to its ifnet
                // structure.
                let ifp = if mreq.ipv6mr_interface == 0 {
                    None
                } else {
                    let Some(ifp) = if_get(mreq.ipv6mr_interface) else {
                        error = Err(Errno::ENXIO); // XXX EINVAL?
                        break 'out;
                    };
                    Some(ifp)
                };

                // Find the membership in the membership list.
                let imm = im6o.im6o_memberships.iter().find(|imm| {
                    imm.i6mm_maddr.get().is_some_and(|in6m| {
                        ifp.is_none_or(|ifp| in6m.in6m_ifidx().get() == ifp.if_index.get())
                            && in6_are_addr_equal(&in6m.in6m_addr(), &mreq.ipv6mr_multiaddr)
                    })
                });

                if_put(ifp);

                let Some(imm) = imm.map(imm_static) else {
                    // Unable to resolve interface
                    error = Err(Errno::EADDRNOTAVAIL);
                    break 'out;
                };
                // Give up the multicast address record to which the membership points.
                // SAFETY: `imm` was found on this list.
                unsafe { ListHead::<In6MultiMshipList>::remove(imm) };
                in6_leavegroup(imm);
            }

            _ => error = Err(Errno::EOPNOTSUPP),
        }
    }

    // If all options have default values, no need to keep the option structure.
    if im6o.im6o_ifidx == 0
        && i32::from(im6o.im6o_hlim) == ip6_defmcasthlim_local
        && i32::from(im6o.im6o_loop) == IPV6_DEFAULT_MULTICAST_LOOP
        && im6o.im6o_memberships.is_empty()
    {
        free(im6o_ptr.cast(), M_IPMOPTS, size_of::<Ip6Moptions>());
        im6op.set(None);
    }

    error
}

/// `ip6_getmoptions`: returns the IP6 multicast options in response to user getsockopt().
fn ip6_getmoptions(optname: i32, im6o: Option<&Ip6Moptions>, m: &Mbuf) -> Result<(), Errno> {
    let v: u32 = match optname {
        IPV6_MULTICAST_IF => im6o.map_or(0, |im6o| u32::from(im6o.im6o_ifidx)),
        IPV6_MULTICAST_HOPS => im6o
            .map_or(IP6_DEFMCASTHLIM.load(Ordering::Relaxed) as u32, |im6o| {
                u32::from(im6o.im6o_hlim)
            }),
        // The C answers ip6_defmcasthlim without options (not IPV6_DEFAULT_MULTICAST_LOOP).
        IPV6_MULTICAST_LOOP => im6o
            .map_or(IP6_DEFMCASTHLIM.load(Ordering::Relaxed) as u32, |im6o| {
                u32::from(im6o.im6o_loop)
            }),
        _ => return Err(Errno::EOPNOTSUPP),
    };
    set_mtod_int(m, v as i32);
    Ok(())
}

/// `ip6_freemoptions`: frees a pcb's multicast options and leaves their groups.
///
/// # Safety
///
/// `im6o` is `None` or a `malloc(9)` allocation of the socket being torn down,
/// not used again.
pub unsafe fn ip6_freemoptions(im6o: Option<NonNull<Ip6Moptions>>) {
    let Some(p) = im6o else {
        return;
    };
    // SAFETY: the caller's contract.
    let o = unsafe { p.as_ref() };

    while let Some(imm) = o.im6o_memberships.first().map(imm_static) {
        // SAFETY: `imm` is the list's first element.
        unsafe { ListHead::<In6MultiMshipList>::remove(imm) };
        in6_leavegroup(imm);
    }
    free(p.cast(), M_IPMOPTS, size_of::<Ip6Moptions>());
}

/// `ip6_setpktopts`: sets the IPv6 outgoing packet options of the control messages in
/// `control` into `opt`, starting from the sticky options `stickyopt`; `priv_`: privileged
/// socket, `uproto`: the upper-layer protocol.
pub fn ip6_setpktopts(
    control: &Mbuf,
    opt: &mut Ip6Pktopts,
    stickyopt: Option<&Ip6Pktopts>,
    priv_: bool,
    uproto: i32,
) -> Result<(), Errno> {
    ip6_initpktopts(opt);
    if let Some(stickyopt) = stickyopt {
        // If stickyopt is provided, make a local copy of the options for this particular
        // packet, then override them by ancillary objects.
        // XXX: copypktopts() does not copy the cached route to a next hop (if any). This is
        // not very good in terms of efficiency, but we can allow this since this option
        // should be rarely used.
        copypktopts(opt, stickyopt)?;
    }

    // XXX: Currently, we assume all the optional information is stored in a single mbuf.
    if control.m_next().get().is_some() {
        return Err(Errno::EINVAL);
    }

    let cmsgs = mtod_bytes(control);
    let hdrlen = cmsg_len(0);
    let mut off = 0usize;
    let mut clen = cmsgs.len();
    loop {
        if clen < hdrlen {
            return Err(Errno::EINVAL);
        }
        // SAFETY: `clen >= CMSG_LEN(0)` bytes remain at `off`, more than a `struct cmsghdr`.
        let cm = unsafe { cmsgs.as_ptr().add(off).cast::<Cmsghdr>().read_unaligned() };
        let cmlen = cm.cmsg_len as usize;
        if cmlen < hdrlen || cmlen > clen || cmsg_align(cmlen) > clen {
            return Err(Errno::EINVAL);
        }
        if cm.cmsg_level == IPPROTO_IPV6 {
            // CMSG_DATA(cm), cm->cmsg_len - CMSG_LEN(0) bytes.
            let data = &cmsgs[off + hdrlen..off + cmlen];
            ip6_setpktopt(cm.cmsg_type, data, opt, priv_, false, uproto)?;
        }

        clen -= cmsg_align(cmlen);
        off += cmsg_align(cmlen);
        if clen == 0 {
            break;
        }
    }

    Ok(())
}

/// The `int` at the start of option value `buf` (its length was checked).
fn buf_int(buf: &[u8]) -> i32 {
    let mut b = [0u8; size_of::<i32>()];
    b.copy_from_slice(&buf[..size_of::<i32>()]);
    i32::from_ne_bytes(b)
}

/// `ip6_setpktopt`: sets a particular packet option from `buf`, as a sticky option or an
/// ancillary data item. `buf` can be empty only when it's a sticky option.
fn ip6_setpktopt(
    optname: i32,
    buf: &[u8],
    opt: &mut Ip6Pktopts,
    priv_: bool,
    sticky: bool,
    uproto: i32,
) -> Result<(), Errno> {
    let len = buf.len();
    match optname {
        IPV6_PKTINFO => {
            if len != size_of::<In6Pktinfo>() {
                return Err(Errno::EINVAL);
            }
            let mut addr = In6Addr::default();
            addr.s6_addr.copy_from_slice(&buf[..16]);
            let mut idx = [0u8; 4];
            idx.copy_from_slice(&buf[16..20]);
            let pktinfo = In6Pktinfo {
                ipi6_addr: addr,
                ipi6_ifindex: u32::from_ne_bytes(idx),
            };

            // An application can clear any sticky IPV6_PKTINFO option by doing a "regular"
            // setsockopt with ipi6_addr being in6addr_any and ipi6_ifindex being zero.
            // [RFC 3542, Section 6]
            if opt.ip6po_pktinfo.is_some()
                && pktinfo.ipi6_ifindex == 0
                && in6_is_addr_unspecified(&pktinfo.ipi6_addr)
            {
                ip6_clearpktopts(opt, optname);
                return Ok(());
            }

            if uproto == IPPROTO_TCP && sticky && !in6_is_addr_unspecified(&pktinfo.ipi6_addr) {
                return Err(Errno::EINVAL);
            }

            if pktinfo.ipi6_ifindex != 0 {
                let Some(ifp) = if_get(pktinfo.ipi6_ifindex) else {
                    return Err(Errno::ENXIO);
                };
                if_put(Some(ifp));
            }

            // We store the address anyway, and let in6_selectsrc() validate the specified
            // address. This is because ipi6_addr may not have enough information about its
            // scope zone, and we may need additional information (such as outgoing
            // interface or the scope zone of a destination address) to disambiguate the
            // scope.
            // XXX: the delay of the validation may confuse the application when it is used
            // as a sticky option.
            let p = match opt.ip6po_pktinfo {
                Some(p) => p,
                None => {
                    let Some(p) = malloc(size_of::<In6Pktinfo>(), M_IP6OPT, M_NOWAIT) else {
                        return Err(Errno::ENOBUFS);
                    };
                    let p = p.cast::<In6Pktinfo>();
                    opt.ip6po_pktinfo = Some(p);
                    p
                }
            };
            // SAFETY: `p` is the options' own allocation of a `struct in6_pktinfo`.
            unsafe { p.as_ptr().write(pktinfo) };
        }

        IPV6_HOPLIMIT => {
            // RFC 3542 deprecated the usage of sticky IPV6_HOPLIMIT to simplify the
            // ordering among hoplimit options.
            if sticky {
                return Err(Errno::ENOPROTOOPT);
            }

            if len != size_of::<i32>() {
                return Err(Errno::EINVAL);
            }
            let hlim = buf_int(buf);
            if !(-1..=255).contains(&hlim) {
                return Err(Errno::EINVAL);
            }

            opt.ip6po_hlim = hlim;
        }

        IPV6_TCLASS => {
            if len != size_of::<i32>() {
                return Err(Errno::EINVAL);
            }
            let tclass = buf_int(buf);
            if !(-1..=255).contains(&tclass) {
                return Err(Errno::EINVAL);
            }

            opt.ip6po_tclass = tclass;
        }

        IPV6_HOPOPTS => {
            // XXX: We don't allow a non-privileged user to set ANY HbH options, since
            // per-option restriction has too much overhead.
            if !priv_ {
                return Err(Errno::EPERM);
            }

            if len == 0 {
                ip6_clearpktopts(opt, IPV6_HOPOPTS);
                return Ok(()); // just remove the option
            }

            // message length validation
            if len < size_of::<Ip6Hbh>() {
                return Err(Errno::EINVAL);
            }
            let hbhlen = (usize::from(buf[offset_of!(Ip6Hbh, ip6h_len)]) + 1) << 3;
            if len != hbhlen {
                return Err(Errno::EINVAL);
            }

            // turn off the previous option, then set the new option.
            ip6_clearpktopts(opt, IPV6_HOPOPTS);
            let Some(p) = malloc(hbhlen, M_IP6OPT, M_NOWAIT) else {
                return Err(Errno::ENOBUFS);
            };
            // SAFETY: a fresh allocation of `hbhlen` bytes, `buf` is that long.
            unsafe { ptr::copy_nonoverlapping(buf.as_ptr(), p.as_ptr(), hbhlen) };
            opt.ip6po_hbh = Some(p.cast());
        }

        IPV6_DSTOPTS | IPV6_RTHDRDSTOPTS => {
            if !priv_ {
                // XXX: see the comment for IPV6_HOPOPTS
                return Err(Errno::EPERM);
            }

            if len == 0 {
                ip6_clearpktopts(opt, optname);
                return Ok(()); // just remove the option
            }

            // message length validation
            if len < size_of::<Ip6Dest>() {
                return Err(Errno::EINVAL);
            }
            let destlen = (usize::from(buf[offset_of!(Ip6Dest, ip6d_len)]) + 1) << 3;
            if len != destlen {
                return Err(Errno::EINVAL);
            }

            // turn off the previous option, then set the new option.
            ip6_clearpktopts(opt, optname);
            let Some(p) = malloc(destlen, M_IP6OPT, M_NOWAIT) else {
                return Err(Errno::ENOBUFS);
            };
            // SAFETY: a fresh allocation of `destlen` bytes, `buf` is that long.
            unsafe { ptr::copy_nonoverlapping(buf.as_ptr(), p.as_ptr(), destlen) };
            // Determine the position that the destination options header should be
            // inserted; before or after the routing header.
            if optname == IPV6_RTHDRDSTOPTS {
                opt.ip6po_dest1 = Some(p.cast());
            } else {
                opt.ip6po_dest2 = Some(p.cast());
            }
        }

        IPV6_USE_MIN_MTU => {
            if len != size_of::<i32>() {
                return Err(Errno::EINVAL);
            }
            let minmtupolicy = buf_int(buf);
            if minmtupolicy != IP6PO_MINMTU_MCASTONLY
                && minmtupolicy != IP6PO_MINMTU_DISABLE
                && minmtupolicy != IP6PO_MINMTU_ALL
            {
                return Err(Errno::EINVAL);
            }
            opt.ip6po_minmtu = minmtupolicy;
        }

        IPV6_DONTFRAG => {
            if len != size_of::<i32>() {
                return Err(Errno::EINVAL);
            }

            if uproto == IPPROTO_TCP || buf_int(buf) == 0 {
                // we ignore this option for TCP sockets. (RFC3542 leaves this case
                // unspecified.)
                opt.ip6po_flags &= !IP6PO_DONTFRAG;
            } else {
                opt.ip6po_flags |= IP6PO_DONTFRAG;
            }
        }

        _ => return Err(Errno::ENOPROTOOPT),
    }

    Ok(())
}

/// `ip6_mloopback`: loops a copy of IP6 multicast packet `m` back to the input queue of
/// `ifp` (called from `ip6_output`).
pub fn ip6_mloopback(ifp: &'static Ifnet, m: &Mbuf, dst: &SockaddrIn6) {
    // Duplicate the packet.
    let Some(mut copym) = m_copym(m, 0, M_COPYALL, M_NOWAIT) else {
        return;
    };

    // Make sure to deep-copy IPv6 header portion in case the data is in an mbuf cluster, so
    // that we can safely override the IPv6 header portion later.
    if copym.m_flags().get() & M_EXT != 0 || (copym.m_len().get() as usize) < size_of::<Ip6Hdr>() {
        let Some(c) = m_pullup(copym, size_of::<Ip6Hdr>() as i32) else {
            return;
        };
        copym = c;
    }

    #[cfg(feature = "diagnostic")]
    if (copym.m_len().get() as usize) < size_of::<Ip6Hdr>() {
        m_freem(copym);
        return;
    }

    let mut ip6 = mtod_ip6(copym);
    if in6_is_scope_embed(&ip6.ip6_src) {
        ip6.ip6_src.set_s6_addr16(1, 0);
    }
    if in6_is_scope_embed(&ip6.ip6_dst) {
        ip6.ip6_dst.set_s6_addr16(1, 0);
    }
    mtod_ip6_store(copym, &ip6);

    let _ = if_input_local(ifp, copym, dst.sin6_family, None);
}

/// `ip6_splithdr`: chops the IPv6 header off from the payload into an mbuf of its own,
/// `exthdrs.ip6e_ip6`. Frees `m` on failure.
fn ip6_splithdr(m: &'static Mbuf, exthdrs: &mut Ip6Exthdrs) -> Result<(), Errno> {
    let mut m = m;
    let ip6 = mtod_ip6(m);
    if m.m_len().get() as usize > size_of::<Ip6Hdr>() {
        let Some(mh) = m_get(M_DONTWAIT, MT_HEADER) else {
            m_freem(m);
            return Err(Errno::ENOBUFS);
        };
        m_move_pkthdr(mh, m);
        m_align(mh, size_of::<Ip6Hdr>() as i32);
        m.m_len().set(m.m_len().get() - size_of::<Ip6Hdr>() as u32);
        m.m_data()
            .set(m.m_data().get().wrapping_add(size_of::<Ip6Hdr>()));
        mh.m_next().set(Some(m));
        m = mh;
        m.m_len().set(size_of::<Ip6Hdr>() as u32);
        mtod_ip6_store(m, &ip6);
    }
    exthdrs.ip6e_ip6 = Some(m);
    Ok(())
}

/// `ip6_randomid`: a random, non-repeating fragment identification.
pub fn ip6_randomid() -> u32 {
    mtx_enter(&IP6_ID_MTX);
    // SAFETY: IP6_ID_CTX is touched only here and in `ip6_randomid_init`, with IP6_ID_MTX
    // held; no other reference to it exists while this one lives.
    let id = idgen32(unsafe { IP6_ID_CTX.get_mut() });
    mtx_leave(&IP6_ID_MTX);
    id
}

/// `ip6_randomid_init`: seeds the IPv6 fragment identification generator.
pub fn ip6_randomid_init() {
    mtx_enter(&IP6_ID_MTX);
    // SAFETY: as in `ip6_randomid`.
    idgen32_init(unsafe { IP6_ID_CTX.get_mut() });
    mtx_leave(&IP6_ID_MTX);
}

/// `in6_cksum_phdr`: computes the significant parts of the IPv6 checksum pseudo-header for
/// use in a delayed TCP/UDP checksum calculation; `len` and `nxt` in network order. The
/// embedded scope words of `src` and `dst` are left out.
fn in6_cksum_phdr(src: &In6Addr, dst: &In6Addr, len: u32, nxt: u32) -> u16 {
    let mut sum: u32 = 0;

    for a in [src, dst] {
        for i in 0..8 {
            if i == 1 && in6_is_scope_embed(a) {
                continue;
            }
            sum += u32::from(a.s6_addr16(i));
        }
    }

    sum += u32::from((len >> 16) as u16) + u32::from(len as u16);

    sum += u32::from((nxt >> 16) as u16) + u32::from(nxt as u16);

    sum = u32::from((sum >> 16) as u16) + u32::from(sum as u16);

    if sum > 0xffff {
        sum -= 0xffff;
    }

    sum as u16
}

/// Stores the 16-bit checksum `csum` (in memory order) at `offset` of packet `m`.
fn cksum_store(m: &Mbuf, offset: usize, csum: u16) {
    if offset + size_of::<u16>() > m.m_len().get() as usize {
        let _ = m_copyback(m, offset as i32, &csum.to_ne_bytes(), M_NOWAIT);
    } else {
        // SAFETY: the two bytes at `offset` lie in the first mbuf (checked above).
        unsafe {
            mtod::<u8>(m)
                .add(offset)
                .cast::<u16>()
                .write_unaligned(csum)
        };
    }
}

/// `in6_delayed_cksum`: processes a delayed payload checksum calculation for upper-layer
/// protocol `nxt`.
pub fn in6_delayed_cksum(m: &Mbuf, nxt: u8) {
    let mut nxtp: i32 = 0;
    let offset = ip6_lasthdr(m, 0, IPPROTO_IPV6, &mut nxtp);
    let Some(mut offset) = offset.filter(|&o| o > 0) else {
        // If the desired next protocol isn't found, punt.
        return;
    };
    if nxtp != i32::from(nxt) {
        return;
    }
    let mut csum = in6_cksum(
        m,
        0,
        offset as u32,
        (m.m_pkthdr().len.get() - offset) as u32,
    );

    match i32::from(nxt) {
        IPPROTO_TCP => offset += offset_of!(Tcphdr, th_sum) as i32,

        IPPROTO_UDP => {
            offset += offset_of!(Udphdr, uh_sum) as i32;
            if csum == 0 {
                csum = 0xffff;
            }
        }

        IPPROTO_ICMPV6 => offset += offset_of!(Icmp6Hdr, icmp6_cksum) as i32,

        _ => {}
    }

    cksum_store(m, offset as usize, csum);
}

/// `in6_proto_cksum_out`: computes the upper-layer checksum of `m` in software or leaves
/// it (with the pseudo-header sum filled in) to the hardware of `ifp`.
pub fn in6_proto_cksum_out(m: &Mbuf, ifp: Option<&Ifnet>) {
    let ip6 = mtod_ip6(m);
    let ph = m.m_pkthdr();

    // some hw and in6_delayed_cksum need the pseudo header cksum
    if ph.csum_flags.get() & (M_TCP_CSUM_OUT | M_UDP_CSUM_OUT | M_ICMP_CSUM_OUT) != 0 {
        let mut nxt: i32 = 0;
        // An invalid chain (-1 in C) gets no pseudo-header sum (see the deviations).
        if let Some(mut offset) = ip6_lasthdr(m, 0, IPPROTO_IPV6, &mut nxt) {
            let csum =
                if ph.csum_flags.get() & M_TCP_TSO != 0 && in_ifcap_cksum(m, ifp, IFCAP_TSOv6) {
                    in6_cksum_phdr(&ip6.ip6_src, &ip6.ip6_dst, htonl(0), htonl(nxt as u32))
                } else {
                    in6_cksum_phdr(
                        &ip6.ip6_src,
                        &ip6.ip6_dst,
                        htonl((ph.len.get() - offset) as u32),
                        htonl(nxt as u32),
                    )
                };
            if nxt == IPPROTO_TCP {
                offset += offset_of!(Tcphdr, th_sum) as i32;
            } else if nxt == IPPROTO_UDP {
                offset += offset_of!(Udphdr, uh_sum) as i32;
            } else if nxt == IPPROTO_ICMPV6 {
                offset += offset_of!(Icmp6Hdr, icmp6_cksum) as i32;
            }
            cksum_store(m, offset as usize, csum);
        }
    }

    let no_hw = |cap: u32, proto: i32| {
        ifp.is_none_or(|ifp| {
            ifp.if_capabilities.get() & cap == 0
                || i32::from(ip6.ip6_nxt) != proto
                || ifp.if_bridgeidx.get() != 0
        })
    };
    if ph.csum_flags.get() & M_TCP_CSUM_OUT != 0 {
        if no_hw(IFCAP_CSUM_TCPv6, IPPROTO_TCP) {
            tcpstat_inc(TcpstatCounters::TcpsOutswcsum);
            in6_delayed_cksum(m, IPPROTO_TCP as u8);
            ph.csum_flags.set(ph.csum_flags.get() & !M_TCP_CSUM_OUT); // Clear
        }
    } else if ph.csum_flags.get() & M_UDP_CSUM_OUT != 0 {
        if no_hw(IFCAP_CSUM_UDPv6, IPPROTO_UDP) {
            udpstat_inc(UdpstatCounters::UdpsOutswcsum);
            in6_delayed_cksum(m, IPPROTO_UDP as u8);
            ph.csum_flags.set(ph.csum_flags.get() & !M_UDP_CSUM_OUT); // Clear
        }
    } else if ph.csum_flags.get() & M_ICMP_CSUM_OUT != 0 {
        in6_delayed_cksum(m, IPPROTO_ICMPV6 as u8);
        ph.csum_flags.set(ph.csum_flags.get() & !M_ICMP_CSUM_OUT); // Clear
    }
}

/// `ip6_output_ipsec_lookup`: the SA the policy wants applied to `m`, with a reference;
/// `None` when no IPsec is needed (no SA, or the packet already went through this one).
/// Checks whether there was an outgoing SA bound to the flow from a transport protocol.
pub fn ip6_output_ipsec_lookup(
    m: &Mbuf,
    seclevel: Option<&IpsecLevel>,
) -> Result<Option<&'static Tdb>, SpdError> {
    let mut tdb = None;

    // Do we have any pending SAs to apply ?
    ipsp_spd_lookup(
        m,
        i32::from(AF_INET6),
        size_of::<Ip6Hdr>() as i32,
        IPSP_DIRECTION_OUT,
        None,
        seclevel,
        Some(&mut tdb),
        None,
    )?;
    let Some(t) = tdb else {
        return Ok(None);
    };
    // Loop detection
    let mut mtag = m_tag_first(m);
    while let Some(tag) = mtag {
        if tag.m_tag_id.get() == PACKET_TAG_IPSEC_OUT_DONE {
            // SAFETY: `IPSEC_OUT_DONE` tags carry a `struct tdb_ident`.
            let tdbi = unsafe { TdbIdent::read(tag.data()) };
            if tdbi.spi == t.tdb_spi.get()
                && tdbi.proto == t.tdb_sproto.get()
                && tdbi.rdomain == t.tdb_rdomain.get()
                && tdbi.dst == t.tdb_dst.get()
            {
                // no IPsec needed
                tdb_unref(Some(t));
                return Ok(None);
            }
        }
        mtag = m_tag_next(m, tag);
    }
    Ok(Some(t))
}

/// `ip6_output_ipsec_pmtu_update`: stores the SA's MTU in a host route to `dst` (cloned
/// when needed, the scope of `dst` taken from interface `ifidx`), but not for transport
/// mode SAs.
fn ip6_output_ipsec_pmtu_update(
    tdb: &Tdb,
    ro: Option<&Route>,
    dst: &mut In6Addr,
    ifidx: u32,
    rtableid: u32,
    transportmode: bool,
) -> Result<(), Errno> {
    let mut rt_mtucloned = false;

    // Find a host route to store the mtu in
    let mut rt = ro.and_then(|ro| ro.ro_rt.get());
    // but don't add a PMTU route for transport mode SAs
    if transportmode {
        rt = None;
    } else if rt.is_none_or(|r| r.rt_flags.get() & RTF_HOST == 0) {
        let mut sin6 = SockaddrIn6::with_addr(*dst);
        sin6.sin6_scope_id = in6_addr2scopeid(ifidx, dst) as u32;
        // should be impossible to fail
        in6_embedscope(dst, &sin6, None, None)?;
        rt = icmp6_mtudisc_clone(&sin6, rtableid, true);
        rt_mtucloned = true;
    }
    crate::ipsec_dprintf!(
        "ip6_output_ipsec_pmtu_update",
        "spi {:08x} mtu {} rt {:p} cloned {}",
        ntohl(tdb.tdb_spi.get()),
        tdb.tdb_mtu.get(),
        rt.map_or(ptr::null(), ptr::from_ref),
        rt_mtucloned
    );
    if let Some(r) = rt {
        r.rt_mtu().store(tdb.tdb_mtu.get(), Ordering::Relaxed);
        if let Some(ro) = ro
            && ro.ro_rt.get().is_some()
        {
            rtfree(ro.ro_rt.get());
            ro.ro_tableid.set(u64::from(rtableid));
            // SAFETY: `ro_dstsa` is the route's destination socket address.
            ro.ro_rt
                .set(unsafe { rtalloc(ro.ro_dstsa(), RT_RESOLVE, rtableid) });
        }
        if rt_mtucloned {
            rtfree(Some(r));
        }
    }
    Ok(())
}

/// `ip6_output_ipsec_send`: hands `m` to the SA `tdb` (after the path MTU check), as many
/// packets as TSO chopping makes; `rtableid`: the packet's original routing table, `fwd`:
/// forwarded. Consumes the packet.
pub fn ip6_output_ipsec_send(
    tdb: &'static Tdb,
    m: &'static Mbuf,
    ro: Option<&Route>,
    rtableid: u32,
    fwd: bool,
) -> Result<(), Errno> {
    let ml = MbufList::new();
    let ip_mtudisc_local = ip_mtudisc.load(Ordering::Relaxed);
    let mut tso = false;

    // Packet filter
    let encif = enc_getif(tdb.tdb_rdomain.get(), tdb.tdb_tap.get());
    let mut mp = Some(m);
    let Some(e) = encif else {
        m_freem(m);
        return Err(Errno::EACCES);
    };
    if pf_test(AF_INET6, if fwd { PF_FWD } else { PF_OUT }, e, &mut mp) != PF_PASS {
        m_freem(mp);
        return Err(Errno::EACCES);
    }
    let Some(m) = mp else {
        return Ok(());
    };
    // PF_TAG_REROUTE handling or not... Packet is entering IPsec so the routing is already
    // overruled by the IPsec policy. Until now the change was not reconsidered. What's the
    // behaviour?

    // Check if we can chop the TCP packet
    let ip6 = mtod_ip6(m);
    let len: u32 = if m.m_pkthdr().csum_flags.get() & M_TCP_TSO != 0
        && u32::from(m.m_pkthdr().ph_mss.get()) <= tdb.tdb_mtu.get()
    {
        tso = true;
        u32::from(m.m_pkthdr().ph_mss.get())
    } else {
        size_of::<Ip6Hdr>() as u32 + u32::from(ntohs(ip6.ip6_plen))
    };

    // Check if we are allowed to fragment
    let mut dst = ip6.ip6_dst;
    let ifidx = m.m_pkthdr().ph_ifidx.get();
    if ip_mtudisc_local != 0
        && tdb.tdb_mtu.get() != 0
        && len > tdb.tdb_mtu.get()
        && tdb.tdb_mtutimeout.get() > gettime() as u64
    {
        let tdst = tdb.tdb_dst.get();
        // SAFETY: the union is the 28 bytes of a `sockaddr_in6`, read as one.
        let tsin6 = unsafe { ptr::read_unaligned(tdst.as_sockaddr_ptr().cast::<SockaddrIn6>()) };
        let transportmode =
            tdst.sa().sa_family == AF_INET6 && in6_are_addr_equal(&tsin6.sin6_addr, &dst);
        if let Err(e) =
            ip6_output_ipsec_pmtu_update(tdb, ro, &mut dst, ifidx, rtableid, transportmode)
        {
            ipsecstat_inc(IpsecCounters::IpsecOdrops);
            tdbstat_inc(tdb, TdbCounters::TdbOdrops);
            m_freem(m);
            return Err(e);
        }
        ipsec_adjust_mtu(m, tdb.tdb_mtu.get());
        m_freem(m);
        return Err(Errno::EMSGSIZE);
    }
    // propagate don't fragment for v6-over-v6
    if ip_mtudisc_local != 0 {
        m.m_pkthdr()
            .csum_flags
            .set(m.m_pkthdr().csum_flags.get() | M_IPV6_DF_OUT);
    }

    // Clear these -- they'll be set in the recursive invocation as needed.
    m.m_flags().set(m.m_flags().get() & !(M_BCAST | M_MCAST));

    let mut error: Result<(), Errno> = Ok(());
    'done: {
        if tso {
            error = tcp_softtso_chop(&ml, m, e, len);
            if error.is_err() {
                break 'done;
            }
        } else {
            m.m_pkthdr()
                .csum_flags
                .set(m.m_pkthdr().csum_flags.get() & !M_TCP_TSO);
            in6_proto_cksum_out(m, encif);
            ml_init(&ml);
            ml_enqueue(&ml, m);
        }

        // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
        while let Some(m) = ml_dequeue(&ml) {
            // Callee frees mbuf
            error = ipsp_process_packet(m, tdb, i32::from(AF_INET6), false, IPSP_DF_INHERIT);
            if error.is_err() {
                break;
            }
        }
        // KERNEL_UNLOCK()
    }
    // done:
    if error.is_err() {
        ml_purge(&ml);
        ipsecstat_inc(IpsecCounters::IpsecOdrops);
        tdbstat_inc(tdb, TdbCounters::TdbOdrops);
    }
    if error.is_ok() && tso {
        tcpstat_inc(TcpstatCounters::TcpsOutswtso);
    }
    if ip_mtudisc_local != 0 && error == Err(Errno::EMSGSIZE) {
        let _ = ip6_output_ipsec_pmtu_update(tdb, ro, &mut dst, ifidx, rtableid, false);
    }
    error
}

#[cfg(test)]
mod tests;
