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
//! `struct arpcom` is what an Ethernet driver's softc embeds: `struct ifnet` first, then the
//! hardware address and the multicast list. `net/if_ethersubr.rs` has the functions over it.
//!
//! Status: `ported` (M7b); the ARP functions and globals are `netinet/if_ether.c`'s.
//!
//! ## Deviations
//! - `struct arpcom` is `#[repr(C)]` with `ac_if` first, as the C's `(struct arpcom *)ifp`
//!   cast needs; [`arpcom_of`] is that cast, checked: `ether_ifattach`, which receives the
//!   `Arpcom`, marks its `ifnet` (`Ifnet::is_arpcom`), and the cast panics on an interface it
//!   did not mark. The all-zero `Arpcom` is valid (a softc is `M_ZERO`). `ac__pad` is
//!   `ac_pad` (a double underscore is not snake case). `ac_trport`/`ac_brport` (SMR pointers
//!   in C) are atomics.
//! - `struct ether_port`'s `ep_input` returns the packet when the port does not take it, as
//!   the C's; the `void *` port is an opaque pointer.
//! - `ETHER_LOOKUP_MULTI`, `ETHER_FIRST_MULTI` and `ETHER_NEXT_MULTI` are functions that
//!   return the record instead of assigning their `enm` argument.
//! - `struct ether_extracted` points into the mbuf with raw pointers, as the C does;
//!   `ip6_hdr`, `tcphdr` and `udphdr` are not ported, so `ip6`, `tcp` and `udp` are byte
//!   pointers.
//! - The globals (`arpt_keep`, `arpt_down`, `revarp_ifidx`) and the ARP prototypes
//!   (`arpinput`, `arpresolve`, ...) come with `netinet/if_ether.c`; `etherbroadcastaddr`,
//!   `etheranyaddr`, `ether_ipmulticast_min`/`_max` and the `ether_*` functions are in
//!   `net/if_ethersubr.rs`, which defines them; `ether_ntoa(3)` and friends are userland.
//! - The address predicates (`ETHER_IS_MULTICAST`, `ETHER_IS_BROADCAST`, `ETHER_IS_ANYADDR`,
//!   `ETHER_IS_EQ`) take `&[u8; ETHER_ADDR_LEN]`; the `ETH64_*` ones and `EVL_*OFTAG` are
//!   `const fn`s.
//! - `ETHER_MAP_IP_MULTICAST(ipaddr, enaddr)` and `ETHER_MAP_IPV6_MULTICAST` are functions
//!   that return the Ethernet address; the IPv6 one takes the sixteen bytes of the `in6_addr`
//!   until `<netinet6/in6.h>` is ported.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::size_of;
use core::sync::atomic::AtomicPtr;

use crate::kern::subr_prf::{Str, panic};
use crate::net::if_arp::Arphdr;
use crate::net::if_var::{Ifnet, Netstack};
use crate::net::route::{RTF_PROTO1, RTF_PROTO3};
use crate::netinet::in_::InAddr;
use crate::netinet::ip::Ip;
use crate::queue_adapter;
use crate::sys::mbuf::Mbuf;
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::refcnt::Refcnt;

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

/// `struct mbuf *(*ep_input)(struct ifnet *, struct mbuf *, uint64_t, void *, struct netstack
/// *)`: a port's input; returns the packet when the port does not take it.
pub type EpInputFn =
    fn(&'static Ifnet, &'static Mbuf, u64, *mut c_void, Option<&Netstack>) -> Option<&'static Mbuf>;

/// `struct ether_port`: an aggregation (`ac_trport`) or bridge (`ac_brport`) port on an
/// Ethernet interface.
pub struct EtherPort {
    /// `ep_input`.
    pub ep_input: EpInputFn,
    /// `ep_port_take`: takes a reference to the port.
    pub ep_port_take: fn(*mut c_void) -> *mut c_void,
    /// `ep_port_rele`: releases the reference `ep_port_take` returned.
    pub ep_port_rele: fn(*mut c_void, *mut c_void),
    /// `ep_port`: the port.
    pub ep_port: *mut c_void,
}

// SAFETY: the port is immutable while installed; the functions synchronise themselves.
unsafe impl Sync for EtherPort {}

/// `struct arpcom`: structure shared between the ethernet driver modules and the address
/// resolution code. For example, each `ec_softc` or `il_softc` begins with this structure.
#[repr(C)]
pub struct Arpcom {
    /// `ac_if`: network-visible interface.
    pub ac_if: Ifnet,
    /// `ac_enaddr`: ethernet hardware address.
    pub ac_enaddr: Cell<[u8; ETHER_ADDR_LEN]>,
    /// `ac__pad`: pad for some machines.
    pub ac_pad: [u8; 2],
    /// `ac_multiaddrs`: list of multicast addrs.
    pub ac_multiaddrs: ListHead<EtherMultiList>,
    /// `ac_multicnt`: length of `ac_multiaddrs`.
    pub ac_multicnt: Cell<i32>,
    /// `ac_multirangecnt`: number of mcast ranges.
    pub ac_multirangecnt: Cell<i32>,

    /// `ac_trport`: the aggregation port (`aggr(4)`, `trunk(4)`), NULL when none.
    pub ac_trport: AtomicPtr<EtherPort>,
    /// `ac_brport`: the bridge port (`bridge(4)`, `veb(4)`, `tpmr(4)`), NULL when none.
    pub ac_brport: AtomicPtr<EtherPort>,
}

// SAFETY: the members change under the net lock or with splnet, as in C; the port pointers are
// atomics.
unsafe impl Sync for Arpcom {}

/// `struct ether_multi`: Ethernet multicast address structure. There is one of these for each
/// multicast address or range of multicast addresses that we are supposed to listen to on a
/// particular interface. They are kept in a linked list, rooted in the interface's arpcom
/// structure. (This really has nothing to do with ARP, or with the Internet address family,
/// but this appears to be the minimally-disrupting place to put it.)
pub struct EtherMulti {
    /// `enm_addrlo`: low or only address of range.
    pub enm_addrlo: [u8; ETHER_ADDR_LEN],
    /// `enm_addrhi`: high or only address of range.
    pub enm_addrhi: [u8; ETHER_ADDR_LEN],
    /// `enm_refcnt`: no. claims to this addr/range.
    pub enm_refcnt: Refcnt,
    /// `enm_list`.
    pub enm_list: ListEntry<EtherMulti>,
}

queue_adapter!(
    /// `LIST_HEAD(, ether_multi) ac_multiaddrs`.
    pub EtherMultiList: EtherMulti, enm_list => ListEntry<EtherMulti>
);

/// `struct ether_multistep`: used by the macros below to remember position when stepping
/// through all of the `ether_multi` records.
pub struct EtherMultistep<'a> {
    /// `e_enm`: the next record.
    pub e_enm: Option<&'a EtherMulti>,
}

/// `struct ether_extracted`: a quick view of the TCP/IP headers inside an Ethernet frame
/// (`ether_extract_headers`); NULL members were not found.
pub struct EtherExtracted {
    /// `eh`.
    pub eh: *mut EtherHeader,
    /// `evh`.
    pub evh: *mut EtherVlanHeader,
    /// `ip4`.
    pub ip4: *mut Ip,
    /// `ip6` (`struct ip6_hdr *`, not ported).
    pub ip6: *mut u8,
    /// `tcp` (`struct tcphdr *`, not ported).
    pub tcp: *mut u8,
    /// `udp` (`struct udphdr *`, not ported).
    pub udp: *mut u8,
    /// `iplen`.
    pub iplen: u32,
    /// `iphlen`.
    pub iphlen: u32,
    /// `tcphlen`.
    pub tcphlen: u32,
    /// `paylen`.
    pub paylen: u32,
}

impl EtherExtracted {
    /// `memset(ext, 0, sizeof(*ext))`.
    pub const fn new() -> Self {
        Self {
            eh: core::ptr::null_mut(),
            evh: core::ptr::null_mut(),
            ip4: core::ptr::null_mut(),
            ip6: core::ptr::null_mut(),
            tcp: core::ptr::null_mut(),
            udp: core::ptr::null_mut(),
            iplen: 0,
            iphlen: 0,
            tcphlen: 0,
            paylen: 0,
        }
    }
}

impl Default for EtherExtracted {
    fn default() -> Self {
        Self::new()
    }
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

/// `(struct arpcom *)ifp`: the `struct arpcom` an Ethernet interface is the first member of.
/// Panics on an interface that `ether_ifattach` did not attach.
pub fn arpcom_of(ifp: &Ifnet) -> &Arpcom {
    if !ifp.is_arpcom() {
        panic(format_args!("{}: not an arpcom", Str(&ifp.if_xname.get())));
    }
    // SAFETY: `ether_ifattach`, given the `Arpcom`, marked the interface: it is the `ac_if`
    // member, at offset 0 of the `#[repr(C)]` structure, which lives as long as it.
    unsafe { &*core::ptr::from_ref(ifp).cast::<Arpcom>() }
}

/// `ETHER_LOOKUP_MULTI(addrlo, addrhi, ac, enm)`: the `ether_multi` record for a given range
/// of Ethernet multicast addresses connected to a given arpcom structure; `None` if no
/// matching record is found.
pub fn ether_lookup_multi<'a>(
    addrlo: &[u8; ETHER_ADDR_LEN],
    addrhi: &[u8; ETHER_ADDR_LEN],
    ac: &'a Arpcom,
) -> Option<&'a EtherMulti> {
    ac.ac_multiaddrs
        .iter()
        .find(|enm| enm.enm_addrlo == *addrlo && enm.enm_addrhi == *addrhi)
}

/// `ETHER_NEXT_MULTI(step, enm)`: step through all of the `ether_multi` records, one at a
/// time; the current position is remembered in `step`. `None` when there are no remaining
/// records.
pub fn ether_next_multi<'a>(step: &mut EtherMultistep<'a>) -> Option<&'a EtherMulti> {
    let enm = step.e_enm?;
    step.e_enm = ListHead::<EtherMultiList>::next(enm);
    Some(enm)
}

/// `ETHER_FIRST_MULTI(step, ac, enm)`: initialises `step` and returns the first record.
pub fn ether_first_multi<'a>(
    step: &mut EtherMultistep<'a>,
    ac: &'a Arpcom,
) -> Option<&'a EtherMulti> {
    step.e_enm = ac.ac_multiaddrs.first();
    ether_next_multi(step)
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
