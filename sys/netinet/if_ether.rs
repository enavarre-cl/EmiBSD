/*	$OpenBSD: if_ether.h,v 1.99 2025/12/02 03:24:19 dlg Exp $	*/
/*	$NetBSD: if_ether.h,v 1.22 1996/05/11 13:00:00 mycroft Exp $	*/
/* <LICENSES> */
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
 *	@(#)if_ether.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! Ethernet: frame layout, constants and the Ethernet ARP packet: `<netinet/if_ether.h>`.
//!
//! Upstream: sys/netinet/if_ether.h @ 3ce1f3f79392
//!
//! The structures are the frames as they are on the wire: `ether_type`, `evl_tag` and the ARP
//! header fields hold network-order values (`ntohs(eh.ether_type) == ETHERTYPE_IP`), as in C.
//! `#include <net/ethertypes.h>` is `crate::net::ethertypes`.
//!
//! Status: `wip`.
//!
//! ## Deviations
//! - `struct arpcom`, `struct ether_port`, `struct ether_multi`, `struct ether_multistep`,
//!   `ETHER_LOOKUP_MULTI`, `ETHER_FIRST_MULTI`, `ETHER_NEXT_MULTI` and `struct
//!   ether_extracted` come with `net/if_ethersubr.c` and `netinet/if_ether.c`: `arpcom`
//!   embeds `struct ifnet` (`<net/if_var.h>`), the multicast list hangs off it, and
//!   `ether_extracted` points at `ip6_hdr`, `tcphdr` and `udphdr`, which are not ported.
//! - The globals (`arpt_keep`, `arpt_down`, `etherbroadcastaddr`, `etheranyaddr`,
//!   `ether_ipmulticast_min`/`_max`, `revarp_ifidx`) and the prototypes (`arpinput`,
//!   `ether_input`, `ether_output`, `ether_crc32_le`, ...) come with the `.c` files that define
//!   them; `ether_ntoa(3)` and friends are userland.
//! - The address predicates (`ETHER_IS_MULTICAST`, `ETHER_IS_BROADCAST`, `ETHER_IS_ANYADDR`,
//!   `ETHER_IS_EQ`) take `&[u8; ETHER_ADDR_LEN]`; the `ETH64_*` ones and `EVL_*OFTAG` are
//!   `const fn`s.
//! - `ETHER_MAP_IP_MULTICAST(ipaddr, enaddr)` and `ETHER_MAP_IPV6_MULTICAST` are functions
//!   that return the Ethernet address; the IPv6 one takes the sixteen bytes of the `in6_addr`
//!   until `<netinet6/in6.h>` is ported.

use core::mem::size_of;

use crate::net::if_arp::Arphdr;
use crate::net::route::{RTF_PROTO1, RTF_PROTO3};
use crate::netinet::in_::InAddr;

/// Ethernet address length.
pub const ETHER_ADDR_LEN: usize = 6;
/// Ethernet type field length.
pub const ETHER_TYPE_LEN: usize = 2;
/// Ethernet CRC length.
pub const ETHER_CRC_LEN: usize = 4;
/// Ethernet header length.
pub const ETHER_HDR_LEN: usize = (ETHER_ADDR_LEN * 2) + ETHER_TYPE_LEN;
/// Minimum frame length, CRC included.
pub const ETHER_MIN_LEN: usize = 64;
/// Maximum frame length, CRC included.
pub const ETHER_MAX_LEN: usize = 1518;
/// Maximum DIX frame length.
pub const ETHER_MAX_DIX_LEN: usize = 1536;

/// Len of 802.1Q VLAN encapsulation.
pub const ETHER_VLAN_ENCAP_LEN: usize = 4;

/// Mbuf adjust factor to force 32-bit alignment of IP header. Drivers should do
/// `m_adj(m, ETHER_ALIGN)` when setting up a receive so the upper layers get the IP header
/// properly aligned past the 14-byte Ethernet header.
pub const ETHER_ALIGN: usize = 2;

/// The maximum supported Ethernet length and some space for encapsulation.
pub const ETHER_MAX_HARDMTU_LEN: usize = 65435;

/// VLAN id mask of a tag.
pub const EVL_VLID_MASK: u16 = 0xFFF;
/// The null VLAN id (0x000 and 0xfff are reserved).
pub const EVL_VLID_NULL: u16 = 0x000;
/// Lowest VLAN id.
pub const EVL_VLID_MIN: u16 = 0x001;
/// Highest VLAN id.
pub const EVL_VLID_MAX: u16 = 0xFFE;

/// Highest priority of a tag.
pub const EVL_PRIO_MAX: u16 = 7;
/// Position of the priority in a tag.
pub const EVL_PRIO_BITS: u16 = 13;

/// Length in octets of encapsulation.
pub const EVL_ENCAPLEN: usize = 4;

/// `ETH64_8021_RSVD_PREFIX`: the IEEE 802.1 reserved multicast range, as a 48-bit integer.
pub const ETH64_8021_RSVD_PREFIX: u64 = 0x0180_c200_0000;
/// `ETH64_8021_RSVD_MASK`.
pub const ETH64_8021_RSVD_MASK: u64 = 0xffff_ffff_fff0;

/// Ethernet MTU.
pub const ETHERMTU: usize = ETHER_MAX_LEN - ETHER_HDR_LEN - ETHER_CRC_LEN;
/// Ethernet minimum payload.
pub const ETHERMIN: usize = ETHER_MIN_LEN - ETHER_HDR_LEN - ETHER_CRC_LEN;

/// Ethernet CRC32 polynomial, little-endian version.
pub const ETHER_CRC_POLY_LE: u32 = 0xedb8_8320;
/// Ethernet CRC32 polynomial, big-endian version.
pub const ETHER_CRC_POLY_BE: u32 = 0x04c1_1db6;

/// `sockaddr_inarp` flag: proxy entry.
pub const SIN_PROXY: u16 = 1;

/// Use trailers (IP and ethernet specific routing flag).
pub const RTF_USETRAILERS: u32 = RTF_PROTO1;
/// Only manual overwrite of entry.
pub const RTF_PERMANENT_ARP: u32 = RTF_PROTO3;

/// `struct ether_addr`: Ethernet address - 6 octets.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EtherAddr {
    /// The octets.
    pub ether_addr_octet: [u8; ETHER_ADDR_LEN],
}

/// `struct ether_header`: the Ethernet header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EtherHeader {
    /// Destination address.
    pub ether_dhost: [u8; ETHER_ADDR_LEN],
    /// Source address.
    pub ether_shost: [u8; ETHER_ADDR_LEN],
    /// Type, network order.
    pub ether_type: u16,
}

/// `struct ether_vlan_header`: an 802.1Q tagged Ethernet header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EtherVlanHeader {
    /// Destination address.
    pub evl_dhost: [u8; ETHER_ADDR_LEN],
    /// Source address.
    pub evl_shost: [u8; ETHER_ADDR_LEN],
    /// `ETHERTYPE_VLAN`, network order.
    pub evl_encap_proto: u16,
    /// Priority and VLAN id, network order.
    pub evl_tag: u16,
    /// Type of the payload, network order.
    pub evl_proto: u16,
}

/// `struct ether_arp`: Ethernet Address Resolution Protocol. See RFC 826 for protocol
/// description; this structure is adapted to resolving internet addresses. Field names used
/// correspond to RFC 826.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EtherArp {
    /// Fixed-size header.
    pub ea_hdr: Arphdr,
    /// Sender hardware address.
    pub arp_sha: [u8; ETHER_ADDR_LEN],
    /// Sender protocol address.
    pub arp_spa: [u8; 4],
    /// Target hardware address.
    pub arp_tha: [u8; ETHER_ADDR_LEN],
    /// Target protocol address.
    pub arp_tpa: [u8; 4],
}

impl EtherArp {
    /// `arp_hrd`: format of hardware address, network order.
    pub const fn arp_hrd(&self) -> u16 {
        self.ea_hdr.ar_hrd
    }

    /// `arp_pro`: format of protocol address, network order.
    pub const fn arp_pro(&self) -> u16 {
        self.ea_hdr.ar_pro
    }

    /// `arp_hln`: length of hardware address.
    pub const fn arp_hln(&self) -> u8 {
        self.ea_hdr.ar_hln
    }

    /// `arp_pln`: length of protocol address.
    pub const fn arp_pln(&self) -> u8 {
        self.ea_hdr.ar_pln
    }

    /// `arp_op`: operation, network order.
    pub const fn arp_op(&self) -> u16 {
        self.ea_hdr.ar_op
    }
}

/// `struct sockaddr_inarp`: the socket address of an ARP entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SockaddrInarp {
    /// Total length.
    pub sin_len: u8,
    /// `AF_INET`.
    pub sin_family: u8,
    /// Port.
    pub sin_port: u16,
    /// Address.
    pub sin_addr: InAddr,
    /// Source address.
    pub sin_srcaddr: InAddr,
    /// Type of service.
    pub sin_tos: u16,
    /// `SIN_PROXY`.
    pub sin_other: u16,
}

/// `EVL_VLANOFTAG(tag)`: the VLAN id of a (host-order) tag.
pub const fn evl_vlanoftag(tag: u16) -> u16 {
    tag & EVL_VLID_MASK
}

/// `EVL_PRIOFTAG(tag)`: the priority of a (host-order) tag.
pub const fn evl_prioftag(tag: u16) -> u16 {
    (tag >> EVL_PRIO_BITS) & 7
}

/// `ETHER_IS_MULTICAST(addr)`: is address mcast/bcast?
pub const fn ether_is_multicast(addr: &[u8; ETHER_ADDR_LEN]) -> bool {
    addr[0] & 0x01 != 0
}

/// `ETHER_IS_BROADCAST(addr)`: is address ff:ff:ff:ff:ff:ff?
pub const fn ether_is_broadcast(addr: &[u8; ETHER_ADDR_LEN]) -> bool {
    (addr[0] & addr[1] & addr[2] & addr[3] & addr[4] & addr[5]) == 0xff
}

/// `ETHER_IS_ANYADDR(addr)`: is address 00:00:00:00:00:00?
pub const fn ether_is_anyaddr(addr: &[u8; ETHER_ADDR_LEN]) -> bool {
    (addr[0] | addr[1] | addr[2] | addr[3] | addr[4] | addr[5]) == 0x00
}

/// `ETHER_IS_EQ(a1, a2)`: are the two addresses equal?
pub fn ether_is_eq(a1: &[u8; ETHER_ADDR_LEN], a2: &[u8; ETHER_ADDR_LEN]) -> bool {
    a1 == a2
}

/// `ETH64_IS_MULTICAST(e64)`: is the address (as a 48-bit integer) mcast/bcast?
pub const fn eth64_is_multicast(e64: u64) -> bool {
    e64 & 0x0100_0000_0000 != 0
}

/// `ETH64_IS_BROADCAST(e64)`.
pub const fn eth64_is_broadcast(e64: u64) -> bool {
    e64 == 0xffff_ffff_ffff
}

/// `ETH64_IS_ANYADDR(e64)`.
pub const fn eth64_is_anyaddr(e64: u64) -> bool {
    e64 == 0x0000_0000_0000
}

/// `ETH64_IS_8021_RSVD(e64)`: is the address in the IEEE 802.1 reserved range?
pub const fn eth64_is_8021_rsvd(e64: u64) -> bool {
    (e64 & ETH64_8021_RSVD_MASK) == ETH64_8021_RSVD_PREFIX
}

/// `ETHER_MAP_IP_MULTICAST(ipaddr, enaddr)`: maps an IP multicast address to an Ethernet
/// multicast address. The high-order 25 bits of the Ethernet address are statically assigned,
/// and the low-order 23 bits are taken from the low end of the IP address.
pub const fn ether_map_ip_multicast(ipaddr: &InAddr) -> [u8; ETHER_ADDR_LEN] {
    let ip = ipaddr.s_addr.to_ne_bytes();
    [0x01, 0x00, 0x5e, ip[1] & 0x7f, ip[2], ip[3]]
}

/// `ETHER_MAP_IPV6_MULTICAST(ip6addr, enaddr)`: maps an IPv6 multicast address (its sixteen
/// bytes) to an Ethernet multicast address. The high-order 16 bits of the Ethernet address
/// are statically assigned, and the low-order 32 bits are taken from the low end of the IPv6
/// address.
pub const fn ether_map_ipv6_multicast(ip6addr: &[u8; 16]) -> [u8; ETHER_ADDR_LEN] {
    [
        0x33,
        0x33,
        ip6addr[12],
        ip6addr[13],
        ip6addr[14],
        ip6addr[15],
    ]
}

// Sizes of the C structures.
const _: () = {
    assert!(size_of::<EtherAddr>() == 6);
    assert!(size_of::<EtherHeader>() == ETHER_HDR_LEN);
    assert!(size_of::<EtherVlanHeader>() == ETHER_HDR_LEN + EVL_ENCAPLEN);
    assert!(size_of::<EtherArp>() == 28);
    assert!(size_of::<SockaddrInarp>() == 16);
};

#[cfg(test)]
mod tests;
