/*	$OpenBSD: ip6.h,v 1.23 2025/05/27 07:52:49 bluhm Exp $	*/
/*	$KAME: ip6.h,v 1.45 2003/06/05 04:46:38 keiichi Exp $	*/
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
 * Copyright (c) 1982, 1986, 1993
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
 *	@(#)ip.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! Definition for internet protocol version 6 (RFC 2460): the header, the extension headers
//! and their options: `<netinet/ip6.h>`.
//!
//! Upstream: sys/netinet/ip6.h @ 3ce1f3f79392
//!
//! Every structure is the header as it is on the wire, multi-byte fields in network order.
//! Packet data has no alignment guarantee, so the headers are read and written as copies
//! (`ptr::read_unaligned`, `mtod_ip6` in `netinet6/ip6_var.rs`), never through a reference
//! into an mbuf, as `struct ip` is (`netinet/ip_var.rs`).
//!
//! ## Deviations
//! - `struct ip6_hdr`'s `ip6_ctlun` union is flattened into its `ip6_un1` members
//!   (`ip6_flow`, `ip6_plen`, `ip6_nxt`, `ip6_hlim`); `ip6_vfc`, the first byte of the
//!   header (`ip6_un2_vfc`), is the first byte of `ip6_flow` in memory, read and written by
//!   [`Ip6Hdr::ip6_vfc`] and [`Ip6Hdr::set_ip6_vfc`]; `ip6_hops` is [`Ip6Hdr::ip6_hops`].
//! - The C structures are `__packed`; here they are `#[repr(C)]` without padding (sizes
//!   asserted below), so their alignment may be 2 or 4, which is why they are only copied
//!   in and out of packets.
//! - The byte-order dependent masks (`IPV6_FLOWINFO_MASK`, `IPV6_FLOWLABEL_MASK`,
//!   `IP6F_*_MASK`, `IP6F_MORE_FRAG`, `IP6_ALERT_*`) are defined once with `htonl`/`htons`
//!   of the big-endian value, which is what both of the C's branches are.
//! - `IP6OPT_TYPE(o)` is a `const fn`; `ip6_exthdr_get` returns the address of the region,
//!   `None` when `m_pulldown` failed (and freed the chain, `*mp` cleared), as the C's `NULL`.

use core::mem::size_of;

use crate::kern::uipc_mbuf2::m_pulldown;
use crate::netinet6::in6::In6Addr;
use crate::sys::endian::{htonl, htons};
use crate::sys::mbuf::{Mbuf, mtod};

/// `IPV6_VERSION`: the version in `ip6_vfc`.
pub const IPV6_VERSION: u8 = 0x60;
/// `IPV6_VERSION_MASK`.
pub const IPV6_VERSION_MASK: u8 = 0xf0;

/// Flow info (28 bits), in network order.
pub const IPV6_FLOWINFO_MASK: u32 = htonl(0x0fff_ffff);
/// Flow label (20 bits), in network order.
pub const IPV6_FLOWLABEL_MASK: u32 = htonl(0x000f_ffff);

// ECN bits proposed by Sally Floyd

/// Congestion experienced.
pub const IP6TOS_CE: u8 = 0x01;
/// ECN-capable transport.
pub const IP6TOS_ECT: u8 = 0x02;

// Option types and related macros

/// 00 0 00000.
pub const IP6OPT_PAD1: u8 = 0x00;
/// 00 0 00001.
pub const IP6OPT_PADN: u8 = 0x01;
/// 11 0 00010 = 194.
pub const IP6OPT_JUMBO: u8 = 0xC2;
/// 11 0 00011.
pub const IP6OPT_NSAP_ADDR: u8 = 0xC3;
/// 00 0 00100.
pub const IP6OPT_TUNNEL_LIMIT: u8 = 0x04;
/// 00 0 00101 (RFC3542, recommended).
pub const IP6OPT_ROUTER_ALERT: u8 = 0x05;

/// `IP6OPT_RTALERT_LEN`.
pub const IP6OPT_RTALERT_LEN: usize = 4;
/// Datagram contains an MLD message.
pub const IP6OPT_RTALERT_MLD: u16 = 0;
/// Datagram contains an RSVP message.
pub const IP6OPT_RTALERT_RSVP: u16 = 1;
/// Contains an Active Networks msg.
pub const IP6OPT_RTALERT_ACTNET: u16 = 2;
/// `IP6OPT_MINLEN`.
pub const IP6OPT_MINLEN: usize = 2;

/// `IP6OPT_TYPE_SKIP`.
pub const IP6OPT_TYPE_SKIP: u8 = 0x00;
/// `IP6OPT_TYPE_DISCARD`.
pub const IP6OPT_TYPE_DISCARD: u8 = 0x40;
/// `IP6OPT_TYPE_FORCEICMP`.
pub const IP6OPT_TYPE_FORCEICMP: u8 = 0x80;
/// `IP6OPT_TYPE_ICMP`.
pub const IP6OPT_TYPE_ICMP: u8 = 0xC0;

/// `IP6OPT_MUTABLE`.
pub const IP6OPT_MUTABLE: u8 = 0x20;

/// `IP6OPT_JUMBO_LEN`.
pub const IP6OPT_JUMBO_LEN: usize = 6;

// Router alert values (in network byte order)

/// `IP6_ALERT_MLD`.
pub const IP6_ALERT_MLD: u16 = htons(0x0000);
/// `IP6_ALERT_RSVP`.
pub const IP6_ALERT_RSVP: u16 = htons(0x0001);
/// `IP6_ALERT_AN`.
pub const IP6_ALERT_AN: u16 = htons(0x0002);

/// Mask out offset from `ip6f_offlg` (network order).
pub const IP6F_OFF_MASK: u16 = htons(0xfff8);
/// Reserved bits in `ip6f_offlg` (network order).
pub const IP6F_RESERVED_MASK: u16 = htons(0x0006);
/// More-fragments flag (network order).
pub const IP6F_MORE_FRAG: u16 = htons(0x0001);

// Internet implementation parameters.

/// Maximum hoplimit.
pub const IPV6_MAXHLIM: u8 = 255;
/// Default hlim.
pub const IPV6_DEFHLIM: u8 = 64;
/// TTL for fragment packets, in slowtimo tick.
pub const IPV6_FRAGTTL: u8 = 120;
/// Subtracted when forwarding.
pub const IPV6_HLIMDEC: u8 = 1;

/// Minimal MTU and reassembly. 1024 + 256.
pub const IPV6_MMTU: u32 = 1280;
/// ip6 max packet size without Jumbo payload.
pub const IPV6_MAXPACKET: usize = 65535;

/// `struct ip6_hdr`: the IPv6 header (see the module's deviations for `ip6_ctlun`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6Hdr {
    /// `ip6_flow`: 4 bits version, 8 bits class, 20 bits flow-ID, network order.
    pub ip6_flow: u32,
    /// `ip6_plen`: payload length, network order.
    pub ip6_plen: u16,
    /// `ip6_nxt`: next header.
    pub ip6_nxt: u8,
    /// `ip6_hlim`: hop limit.
    pub ip6_hlim: u8,
    /// `ip6_src`: source address.
    pub ip6_src: In6Addr,
    /// `ip6_dst`: destination address.
    pub ip6_dst: In6Addr,
}

impl Ip6Hdr {
    /// An all-zero header, usable in `const` contexts.
    pub const fn zeroed() -> Self {
        Self {
            ip6_flow: 0,
            ip6_plen: 0,
            ip6_nxt: 0,
            ip6_hlim: 0,
            ip6_src: In6Addr { s6_addr: [0; 16] },
            ip6_dst: In6Addr { s6_addr: [0; 16] },
        }
    }

    /// `ip6_vfc`: 4 bits version, top 4 bits class (the header's first byte).
    pub const fn ip6_vfc(&self) -> u8 {
        self.ip6_flow.to_ne_bytes()[0]
    }

    /// `ip6_vfc = v`.
    pub const fn set_ip6_vfc(&mut self, v: u8) {
        let mut b = self.ip6_flow.to_ne_bytes();
        b[0] = v;
        self.ip6_flow = u32::from_ne_bytes(b);
    }

    /// `ip6_hops` (`ip6_hlim`).
    pub const fn ip6_hops(&self) -> u8 {
        self.ip6_hlim
    }
}

/// `struct ip6_hdr_pseudo`: for IPv6 pseudo header checksum (nonstandard).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6HdrPseudo {
    /// `ip6ph_src`.
    pub ip6ph_src: In6Addr,
    /// `ip6ph_dst`.
    pub ip6ph_dst: In6Addr,
    /// `ip6ph_len`: network order.
    pub ip6ph_len: u32,
    /// `ip6ph_zero`.
    pub ip6ph_zero: [u8; 3],
    /// `ip6ph_nxt`.
    pub ip6ph_nxt: u8,
}

/// `struct ip6_ext`: the start of every extension header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6Ext {
    /// `ip6e_nxt`.
    pub ip6e_nxt: u8,
    /// `ip6e_len`.
    pub ip6e_len: u8,
}

/// `struct ip6_hbh`: Hop-by-Hop options header (followed by options).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6Hbh {
    /// Next header.
    pub ip6h_nxt: u8,
    /// Length in units of 8 octets.
    pub ip6h_len: u8,
}

/// `struct ip6_dest`: Destination options header (followed by options).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6Dest {
    /// Next header.
    pub ip6d_nxt: u8,
    /// Length in units of 8 octets.
    pub ip6d_len: u8,
}

/// `struct ip6_opt`: IPv6 options, common part.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6Opt {
    /// `ip6o_type`.
    pub ip6o_type: u8,
    /// `ip6o_len`.
    pub ip6o_len: u8,
}

/// `struct ip6_opt_jumbo`: Jumbo Payload Option.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6OptJumbo {
    /// `ip6oj_type`.
    pub ip6oj_type: u8,
    /// `ip6oj_len`.
    pub ip6oj_len: u8,
    /// `ip6oj_jumbo_len`.
    pub ip6oj_jumbo_len: [u8; 4],
}

/// `struct ip6_opt_nsap`: NSAP Address Option (followed by source and destination NSAP).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6OptNsap {
    /// `ip6on_type`.
    pub ip6on_type: u8,
    /// `ip6on_len`.
    pub ip6on_len: u8,
    /// `ip6on_src_nsap_len`.
    pub ip6on_src_nsap_len: u8,
    /// `ip6on_dst_nsap_len`.
    pub ip6on_dst_nsap_len: u8,
}

/// `struct ip6_opt_tunnel`: Tunnel Limit Option.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6OptTunnel {
    /// `ip6ot_type`.
    pub ip6ot_type: u8,
    /// `ip6ot_len`.
    pub ip6ot_len: u8,
    /// `ip6ot_encap_limit`.
    pub ip6ot_encap_limit: u8,
}

/// `struct ip6_opt_router`: Router Alert Option.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6OptRouter {
    /// `ip6or_type`.
    pub ip6or_type: u8,
    /// `ip6or_len`.
    pub ip6or_len: u8,
    /// `ip6or_value`.
    pub ip6or_value: [u8; 2],
}

/// `struct ip6_rthdr`: Routing header (followed by routing type specific data).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6Rthdr {
    /// Next header.
    pub ip6r_nxt: u8,
    /// Length in units of 8 octets.
    pub ip6r_len: u8,
    /// Routing type.
    pub ip6r_type: u8,
    /// Segments left.
    pub ip6r_segleft: u8,
}

/// `struct ip6_rthdr0`: Type 0 Routing header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6Rthdr0 {
    /// Next header.
    pub ip6r0_nxt: u8,
    /// Length in units of 8 octets.
    pub ip6r0_len: u8,
    /// Always zero.
    pub ip6r0_type: u8,
    /// Segments left.
    pub ip6r0_segleft: u8,
    /// Reserved field.
    pub ip6r0_reserved: u32,
}

/// `struct ip6_frag`: Fragment header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6Frag {
    /// Next header.
    pub ip6f_nxt: u8,
    /// Reserved field.
    pub ip6f_reserved: u8,
    /// Offset, reserved, and flag (network order).
    pub ip6f_offlg: u16,
    /// Identification.
    pub ip6f_ident: u32,
}

/// `IP6OPT_TYPE(o)`: the action bits of option type `o`.
pub const fn ip6opt_type(o: u8) -> u8 {
    o & 0xC0
}

/// `ip6_exthdr_get(mp, off, len)`: ensures that the intermediate protocol header from `off`
/// to `off + len` is located in a single mbuf, on contiguous memory, and returns its
/// address. `None` when the packet is too short: the chain was freed and `*mp` cleared.
pub fn ip6_exthdr_get(mp: &mut Option<&'static Mbuf>, off: i32, len: i32) -> Option<*mut u8> {
    let m = (*mp)?;
    if i64::from(m.m_len().get()) >= i64::from(off) + i64::from(len) {
        return Some(mtod::<u8>(m).wrapping_add(off as usize));
    }

    let mut toff = 0;
    let Some(t) = m_pulldown(m, off, len, Some(&mut toff)) else {
        *mp = None;
        return None;
    };
    Some(mtod::<u8>(t).wrapping_add(toff as usize))
}

// The sizes of the C's `__packed` structures.
const _: () = {
    assert!(size_of::<Ip6Hdr>() == 40);
    assert!(size_of::<Ip6HdrPseudo>() == 40);
    assert!(size_of::<Ip6Ext>() == 2);
    assert!(size_of::<Ip6Hbh>() == 2);
    assert!(size_of::<Ip6Dest>() == 2);
    assert!(size_of::<Ip6Opt>() == 2);
    assert!(size_of::<Ip6OptJumbo>() == IP6OPT_JUMBO_LEN);
    assert!(size_of::<Ip6OptNsap>() == 4);
    assert!(size_of::<Ip6OptTunnel>() == 3);
    assert!(size_of::<Ip6OptRouter>() == IP6OPT_RTALERT_LEN);
    assert!(size_of::<Ip6Rthdr>() == 4);
    assert!(size_of::<Ip6Rthdr0>() == 8);
    assert!(size_of::<Ip6Frag>() == 8);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use core::mem::offset_of;

    use super::*;
    use crate::reftest::{assert_complete, assert_defines};

    #[test]
    fn vfc_is_the_first_byte_of_the_header() {
        let mut ip6 = Ip6Hdr::zeroed();
        ip6.set_ip6_vfc(IPV6_VERSION);
        assert_eq!(ip6.ip6_vfc() & IPV6_VERSION_MASK, IPV6_VERSION);
        assert_eq!(ip6.ip6_flow.to_ne_bytes(), [0x60, 0, 0, 0]);
        // The flow label survives a version update, as with the C's union.
        ip6.ip6_flow = htonl(0x6123_4567);
        ip6.set_ip6_vfc(0x6f);
        assert_eq!(ip6.ip6_flow & IPV6_FLOWLABEL_MASK, htonl(0x0003_4567));
        assert_eq!(ip6.ip6_flow & IPV6_FLOWINFO_MASK, htonl(0x0f23_4567));
        ip6.ip6_hlim = 64;
        assert_eq!(ip6.ip6_hops(), 64);
    }

    #[test]
    fn layout_matches_the_wire() {
        assert_eq!(offset_of!(Ip6Hdr, ip6_plen), 4);
        assert_eq!(offset_of!(Ip6Hdr, ip6_nxt), 6);
        assert_eq!(offset_of!(Ip6Hdr, ip6_hlim), 7);
        assert_eq!(offset_of!(Ip6Hdr, ip6_src), 8);
        assert_eq!(offset_of!(Ip6Hdr, ip6_dst), 24);
        assert_eq!(offset_of!(Ip6Frag, ip6f_offlg), 2);
        assert_eq!(offset_of!(Ip6Frag, ip6f_ident), 4);
        assert_eq!(offset_of!(Ip6HdrPseudo, ip6ph_len), 32);
        assert_eq!(offset_of!(Ip6HdrPseudo, ip6ph_nxt), 39);
    }

    #[test]
    fn fragment_and_option_bits() {
        // offset 0x1238 bytes (in units of 8), more fragments
        let offlg = htons(0x1238 | 0x0001);
        assert_eq!(offlg & IP6F_OFF_MASK, htons(0x1238));
        assert_eq!(offlg & IP6F_MORE_FRAG, htons(1));
        assert_eq!(offlg & IP6F_RESERVED_MASK, 0);
        assert_eq!(ip6opt_type(IP6OPT_JUMBO), IP6OPT_TYPE_ICMP);
        assert_eq!(ip6opt_type(IP6OPT_ROUTER_ALERT), IP6OPT_TYPE_SKIP);
        assert_eq!(IP6_ALERT_MLD, 0);
        assert_eq!(IP6_ALERT_RSVP.to_ne_bytes(), [0, 1]);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet/ip6.h");
        let opt = assert_defines!(defs;
        IP6OPT_PAD1, IP6OPT_PADN, IP6OPT_JUMBO, IP6OPT_NSAP_ADDR, IP6OPT_TUNNEL_LIMIT,
        IP6OPT_ROUTER_ALERT, IP6OPT_RTALERT_LEN, IP6OPT_RTALERT_MLD, IP6OPT_RTALERT_RSVP,
        IP6OPT_RTALERT_ACTNET, IP6OPT_MINLEN, IP6OPT_TYPE_SKIP, IP6OPT_TYPE_DISCARD,
        IP6OPT_TYPE_FORCEICMP, IP6OPT_TYPE_ICMP, IP6OPT_MUTABLE, IP6OPT_JUMBO_LEN);
        assert_complete(&defs, "IP6OPT_", &opt);
        let ipv6 = assert_defines!(defs;
        IPV6_VERSION, IPV6_VERSION_MASK, IPV6_MAXHLIM, IPV6_DEFHLIM, IPV6_FRAGTTL, IPV6_HLIMDEC,
        IPV6_MMTU, IPV6_MAXPACKET);
        // The byte-order dependent masks: the last (little-endian) branch is what `defines`
        // keeps, the order of the host these tests run on.
        #[cfg(target_endian = "little")]
        let ipv6 = [
            &ipv6[..],
            &assert_defines!(defs; IPV6_FLOWINFO_MASK, IPV6_FLOWLABEL_MASK)[..],
        ]
        .concat();
        #[cfg(target_endian = "little")]
        {
            let frag = assert_defines!(defs; IP6F_OFF_MASK, IP6F_RESERVED_MASK, IP6F_MORE_FRAG);
            assert_complete(&defs, "IP6F_", &frag);
            let alert = assert_defines!(defs; IP6_ALERT_MLD, IP6_ALERT_RSVP, IP6_ALERT_AN);
            assert_complete(&defs, "IP6_ALERT_", &alert);
        }
        assert_complete(&defs, "IPV6_", &ipv6);
        assert_defines!(defs; IP6TOS_CE, IP6TOS_ECT);
    }
}
/* </TESTS> */
