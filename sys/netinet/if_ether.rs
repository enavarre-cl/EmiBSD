/*	$OpenBSD: if_ether.h,v 1.99 2025/12/02 03:24:19 dlg Exp $	*/
/*	$NetBSD: if_ether.h,v 1.22 1996/05/11 13:00:00 mycroft Exp $	*/
/*	$OpenBSD: if_ether.c,v 1.278 2026/03/23 13:12:39 jsg Exp $	*/
/*	$NetBSD: if_ether.c,v 1.31 1996/05/11 12:59:58 mycroft Exp $	*/
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

/*
 * Copyright (c) 1982, 1986, 1988, 1993
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
 *	@(#)if_ether.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! Ethernet: frame layout, constants and the Ethernet ARP packet (`<netinet/if_ether.h>`),
//! and the Ethernet address resolution protocol (`netinet/if_ether.c`).
//!
//! Upstream: sys/netinet/if_ether.h @ 3ce1f3f79392
//! Upstream: sys/netinet/if_ether.c @ 3ce1f3f79392
//!
//! The structures are the frames as they are on the wire: `ether_type`, `evl_tag` and the ARP
//! header fields hold network-order values (`ntohs(eh.ether_type) == ETHERTYPE_IP`), as in C.
//! `#include <net/ethertypes.h>` is `crate::net::ethertypes`.
//!
//! `struct arpcom` is what an Ethernet driver's softc embeds: `struct ifnet` first, then the
//! hardware address and the multicast list. `net/if_ethersubr.rs` has the functions over it.
//!
//! ARP keeps its cache in the routing table: a cloning route of a connected subnet makes a
//! host route per neighbour (`RTF_LLINFO`), whose gateway is a `sockaddr_dl` holding the
//! neighbour's hardware address and whose `rt_llinfo` is a `struct llinfo_arp` (the packets
//! held while the request is outstanding, the retry count). `arpresolve` answers from it or
//! sends a request, `in_arpinput` fills it from requests and replies (`arpcache`) and answers
//! requests for our addresses, and `arptimer` ages it. TODO in the C: add "inuse/lock" bit (or
//! ref. count) along with valid bit.
//!
//! Status: `if_ether.h` `ported`, `if_ether.c` `ported` (M7b).
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
//! - `struct ether_extracted` points into the mbuf with raw pointers, as the C does (the
//!   packet data has no alignment guarantee, so they are read unaligned); `tcphdr` and
//!   `udphdr` are not ported, so `tcp` and `udp` are byte pointers.
//! - `etherbroadcastaddr`, `etheranyaddr`, `ether_ipmulticast_min`/`_max` and the `ether_*`
//!   functions are in `net/if_ethersubr.rs`, which defines them; `ether_ntoa(3)` and friends
//!   are userland.
//! - The address predicates (`ETHER_IS_MULTICAST`, `ETHER_IS_BROADCAST`, `ETHER_IS_ANYADDR`,
//!   `ETHER_IS_EQ`) take `&[u8; ETHER_ADDR_LEN]`; the `ETH64_*` ones and `EVL_*OFTAG` are
//!   `const fn`s.
//! - `ETHER_MAP_IP_MULTICAST(ipaddr, enaddr)` and `ETHER_MAP_IPV6_MULTICAST(ip6addr, enaddr)`
//!   are functions that return the Ethernet address.
//! - `struct llinfo_arp_iterator` (the C's smaller marker with the same first two members) is
//!   a whole [`LlinfoArp`] with no route: a Rust list links one type. `arptimer`'s marker is a
//!   static instead of a stack variable (only that timeout walks with one).
//! - The ARP packet in an mbuf is read and written as a copy (`ea_get`, `ea_store`): mbuf data
//!   has no alignment guarantee. The Ethernet header `arprequest`/`arpreply` hand to
//!   `if_output` in a `pseudo_AF_HDRCMPLT` socket address is written into its `sa_data`, as in
//!   C.
//! - `inet_ntop` (`netinet/inet_ntop.c`) is not ported: the log lines print IPv4 addresses
//!   with a dotted-quad `Display` adaptor, which is what it writes for `AF_INET`.
//! - The `la_hold_total` counter keeps its C name beside the `LA_HOLD_TOTAL` limit
//!   (`docs/C_TO_RUST.md`); `arpcache` returns `bool` (the C's 0/-1); `arpresolve` is an
//!   `unsafe fn` over the raw destination address and answers `Result` (`EAGAIN`: the packet
//!   is held).
//! - `NFSCLIENT` (the `revarp*` state and functions behind it) is feature `nfsclient`;
//!   `NCARP` is not configured and is a comment at its site. `arpresolve` sets and clears
//!   `RTF_REJECT` under the kernel lock (`KERNEL_LOCK()`, nothing without `MULTIPROCESSOR`)
//!   and tests it without, as the C; `rt_flags` is a `Cell` of `net/route.rs`.
//! - The `revarp` state (`revarp_myip`, `revarp_srvip`, `revarp_finished`, `revarp_ifidx`) is
//!   atomics ([`REVARP_MYIP`], ...), one machine word each, as the C reads and writes them
//!   without a lock; `revarpwhoarewe` returns `Result<(InAddr, InAddr), Errno>` (the server's
//!   and the client's address, the C's two out parameters) and `revarpwhoami` the
//!   `Result<InAddr, Errno>` of the client's address.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;
#[cfg(feature = "nfsclient")]
use core::sync::atomic::AtomicBool;
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_synch::{refcnt_init, refcnt_rele, refcnt_take};
#[cfg(feature = "nfsclient")]
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_tc::getuptime;
use crate::kern::kern_timeout::{timeout_add_sec, timeout_set_flags};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{Str, panic};
use crate::kern::uipc_mbuf::{
    m_align, m_freem, m_gethdr, m_pullup, m_resethdr, ml_dequeue, mq_init, mq_purge, mq_push,
};
use crate::log;
use crate::machine::intr::IPL_SOFTNET;
#[cfg(feature = "nfsclient")]
use crate::net::ethertypes::ETHERTYPE_REVARP;
use crate::net::ethertypes::{ETHERTYPE_ARP, ETHERTYPE_IP};
use crate::net::if_::{
    IFF_NOARP, IFF_STATICARP, if_get, if_isconnected, if_output_mq, if_put, niq_enqueue,
};
#[cfg(feature = "nfsclient")]
use crate::net::if_arp::ARPOP_REVREQUEST;
use crate::net::if_arp::{ARPHRD_ETHER, ARPOP_REPLY, ARPOP_REQUEST, ARPOP_REVREPLY, Arphdr};
use crate::net::if_dl::{SockaddrDl, lladdr, satosdl};
use crate::net::if_ethersubr::{ETHERBROADCASTADDR, ether_sprintf};
use crate::net::if_var::{Ifnet, Netstack, Niqueue, niq_delist};
use crate::net::netisr::NETISR_ARP;
use crate::net::route::{
    RT_RESOLVE, RTF_ANNOUNCE, RTF_BROADCAST, RTF_CACHED, RTF_CLONING, RTF_GATEWAY, RTF_LLINFO,
    RTF_LOCAL, RTF_MPLS, RTF_MULTICAST, RTF_PROTO1, RTF_PROTO3, RTF_REJECT, RTF_STATIC, RTM_ADD,
    RTM_DELETE, RTM_INVALIDATE, RTM_RESOLVE, Rtentry, rt_getll, rtalloc, rtdeletemsg, rtfree,
    rtisvalid, rtref,
};
use crate::net::rtable::{rt_key, rtable_iterate, rtable_l2};
use crate::net::rtsock::rtm_send;
use crate::netinet::in_::{INADDR_ANY, InAddr, SockaddrIn, satosin_const, sintosa};
use crate::netinet::ip::Ip;
use crate::netinet::ip6::Ip6Hdr;
use crate::netinet6::in6::In6Addr;
use crate::queue_adapter;
use crate::sys::endian::{htons, ntohs};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{M_BCAST, M_DONTWAIT, M_MCAST, MT_DATA, Mbuf, MbufList, MbufQueue, mtod};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
#[cfg(feature = "nfsclient")]
use crate::sys::param::PSOCK;
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::socket::{AF_INET, AF_LINK, Sockaddr, pseudo_AF_HDRCMPLT};
use crate::sys::syslog::{LOG_DEBUG, LOG_ERR, LOG_INFO, LOG_WARNING};
use crate::sys::systm::{
    kernel_lock, kernel_unlock, net_assert_locked, net_assert_locked_exclusive, net_lock,
    net_unlock,
};
#[cfg(feature = "nfsclient")]
use crate::sys::time::msec_to_nsec;
use crate::sys::timeout::{KCLOCK_NONE, TIMEOUT_MPSAFE, TIMEOUT_PROC, Timeout};

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
    /// `ip6`.
    pub ip6: *mut Ip6Hdr,
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

/// `ETHER_MAP_IPV6_MULTICAST(ip6addr, enaddr)`: maps an IPv6 multicast address to an
/// Ethernet multicast address. The high-order 16 bits of the Ethernet address are statically
/// assigned, and the low-order 32 bits are taken from the low end of the IPv6 address.
pub const fn ether_map_ipv6_multicast(ip6addr: &In6Addr) -> [u8; ETHER_ADDR_LEN] {
    let a = &ip6addr.s6_addr;
    [0x33, 0x33, a[12], a[13], a[14], a[15]]
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

/// `LA_HOLD_QUEUE`: packets held per unresolved entry.
pub const LA_HOLD_QUEUE: u32 = 10;
/// `LA_HOLD_TOTAL`: packets held by all entries.
pub const LA_HOLD_TOTAL: u32 = 100;

/// `arp_maxtries`: arp requests before set to rejected.
const ARP_MAXTRIES: i32 = 5;

/// `struct llinfo_arp`: the ARP state of a route (`rt_llinfo`). Locks: \[m\] arp mutex,
/// \[I\] immutable after creation.
pub struct LlinfoArp {
    /// \[m\] `la_list`: global `arp_list`.
    pub la_list: ListEntry<LlinfoArp>,
    /// \[I\] `la_rt`: backpointer to rtentry (always NULL for an iterator marker).
    pub la_rt: Cell<Option<&'static Rtentry>>,
    /// `la_refcnt`: entry referenced by list.
    pub la_refcnt: Refcnt,
    /// `la_mq`: packet hold queue.
    pub la_mq: MbufQueue,
    /// `la_refreshed`: when was refresh sent.
    pub la_refreshed: Cell<i64>,
    /// `la_asked`: number of queries sent.
    pub la_asked: Cell<i32>,
}

impl LlinfoArp {
    /// An entry with no route: what `struct llinfo_arp_iterator` is in C (see the module's
    /// deviations).
    const fn iterator() -> Self {
        Self {
            la_list: ListEntry::new(),
            la_rt: Cell::new(None),
            la_refcnt: Refcnt::new(),
            la_mq: MbufQueue::new(0, IPL_SOFTNET),
            la_refreshed: Cell::new(0),
            la_asked: Cell::new(0),
        }
    }
}

queue_adapter!(
    /// `LIST_HEAD(, llinfo_arp)` through `la_list`: `arp_list`.
    pub LaList: LlinfoArp, la_list => ListEntry<LlinfoArp>
);

/// `arp_list`, made `Sync`: changed and walked under `arp_mtx`.
struct ArpListHead(ListHead<LaList>);

// SAFETY: see the type's doc.
unsafe impl Sync for ArpListHead {}

/// A network-order IPv4 address printed as a dotted quad (what `inet_ntop(AF_INET, ...)`
/// writes; see the module's deviations).
struct InAddrFmt(InAddr);

impl core::fmt::Display for InAddrFmt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let b = self.0.s_addr.to_ne_bytes();
        write!(f, "{}.{}.{}.{}", b[0], b[1], b[2], b[3])
    }
}

/// \[I\] `arpt_prune`: walk list every 5 minutes.
const ARPT_PRUNE: i32 = 5 * 60;
/// \[a\] `arpt_keep`: once resolved, cache for 20 minutes.
pub static ARPT_KEEP: AtomicI32 = AtomicI32::new(20 * 60);
/// \[a\] `arpt_down`: once declared down, don't send for 20 secs.
pub static ARPT_DOWN: AtomicI32 = AtomicI32::new(20);

/// `arpinq`.
pub static ARPINQ: Niqueue = Niqueue::new(50, NETISR_ARP);

/// `arp_mtx`: llinfo_arp live time, `rt_llinfo` and `RTF_LLINFO` are protected by it.
static ARP_MTX: Mutex = Mutex::new(IPL_SOFTNET);

/// \[m\] `arp_list`: list of `llinfo_arp` structures.
static ARP_LIST: ArpListHead = ArpListHead(ListHead::new());
/// \[I\] `arp_pool`: pool for `llinfo_arp` structures.
static ARP_POOL: Pool = Pool::new();
/// \[a\] `la_hold_total`: packets currently in the arp queue.
#[allow(non_upper_case_globals)] // the C name; `LA_HOLD_TOTAL` is the limit beside it
pub static la_hold_total: AtomicU32 = AtomicU32::new(0);

/// `arptimer_to`: `arpinit`'s timeout.
static ARPTIMER_TO: Timeout = Timeout::new(arptimer, ptr::null_mut());

/// `revarp_myip`: the client address a RARP reply gave (`in_addr`, network order). NFSCLIENT.
#[cfg(feature = "nfsclient")]
pub static REVARP_MYIP: AtomicU32 = AtomicU32::new(0);
/// `revarp_srvip`: the address of the server that answered. NFSCLIENT.
#[cfg(feature = "nfsclient")]
pub static REVARP_SRVIP: AtomicU32 = AtomicU32::new(0);
/// `revarp_finished`: a reply has been taken. NFSCLIENT.
#[cfg(feature = "nfsclient")]
pub static REVARP_FINISHED: AtomicBool = AtomicBool::new(false);
/// `revarp_ifidx`: the interface a RARP request is out on (0: none); `if_detach` clears it
/// for the interface that goes. NFSCLIENT.
#[cfg(feature = "nfsclient")]
pub static REVARP_IFIDX: AtomicU32 = AtomicU32::new(0);

/// The `struct llinfo_arp` of a route, NULL when none.
fn rt_la(rt: &Rtentry) -> Option<&'static LlinfoArp> {
    // SAFETY: an ARP route's `rt_llinfo` is NULL or the entry `arp_rtrequest` made, which
    // lives until its last `la_refcnt` reference (under `arp_mtx`, held by the callers).
    unsafe { (rt.rt_llinfo.get() as *const LlinfoArp).as_ref() }
}

/// Frees an ARP entry.
fn la_put(la: &LlinfoArp) {
    pool_put(&ARP_POOL, ptr::NonNull::from(la).cast());
}

/// `arpiterator`: the entry after `la` (or the first, without `la`) that has a route, with a
/// reference; `iter` marks the position in `arp_list` while `arp_mtx` is released.
fn arpiterator(
    la: Option<&'static LlinfoArp>,
    iter: &'static LlinfoArp,
) -> Option<&'static LlinfoArp> {
    mutex_assert_locked(&ARP_MTX, "arpiterator");

    let mut tmp = match la {
        Some(_) => ListHead::<LaList>::next(iter),
        None => ARP_LIST.0.first(),
    };

    while let Some(t) = tmp {
        if t.la_rt.get().is_some() {
            break;
        }
        tmp = ListHead::<LaList>::next(t);
    }
    // SAFETY: an entry on `arp_list` lives while referenced (taken below, under `arp_mtx`).
    let tmp: Option<&'static LlinfoArp> = tmp.map(|t| unsafe { &*ptr::from_ref(t) });

    if let Some(la) = la {
        // SAFETY: the iterator is on the list (inserted below by the previous call).
        unsafe { ListHead::<LaList>::remove(iter) };
        if refcnt_rele(&la.la_refcnt) {
            la_put(la);
        }
    }
    if let Some(t) = tmp {
        // SAFETY: the iterator is on no list; it lives until the caller's walk ends.
        unsafe { ListHead::<LaList>::insert_after(t, iter) };
        refcnt_take(&t.la_refcnt);
    }

    tmp
}

/// `arptimer`: timeout routine. Age arp table entries periodically.
pub fn arptimer(arg: *mut c_void) {
    // SAFETY: `arpinit` armed the timeout with its own address as the argument.
    let to = unsafe { &*arg.cast::<Timeout>() };
    // The iterator marker lives in a static: only this timeout walks the list with one.
    static ITER: ArpIter = ArpIter(LlinfoArp::iterator());
    let iter = &ITER.0;
    let mut la = None;

    let uptime = getuptime();
    let _ = timeout_add_sec(to, ARPT_PRUNE);

    mtx_enter(&ARP_MTX);
    loop {
        la = arpiterator(la, iter);
        let Some(l) = la else {
            break;
        };
        let Some(rt) = l.la_rt.get() else {
            continue;
        };

        if rt.rt_expire().get() != 0 && rt.rt_expire().get() < uptime {
            rtref(rt);
            mtx_leave(&ARP_MTX);
            net_lock();
            arptfree(rt); // timer has expired; clear
            net_unlock();
            rtfree(Some(rt));
            mtx_enter(&ARP_MTX);
        }
    }
    mtx_leave(&ARP_MTX);
}

/// `arptimer`'s iterator marker, made `Sync`: only `arptimer`, under `arp_mtx`, links it.
struct ArpIter(LlinfoArp);

// SAFETY: see the type's doc.
unsafe impl Sync for ArpIter {}

/// `arpinit`: the entry pool and the aging timeout.
pub fn arpinit() {
    pool_init(
        &ARP_POOL,
        size_of::<LlinfoArp>(),
        0,
        IPL_SOFTNET,
        0,
        "arp",
        None,
    );
    ARP_LIST.0.init();

    timeout_set_flags(
        &ARPTIMER_TO,
        arptimer,
        ptr::from_ref(&ARPTIMER_TO).cast_mut().cast(),
        KCLOCK_NONE,
        TIMEOUT_PROC | TIMEOUT_MPSAFE,
    );
    let _ = timeout_add_sec(&ARPTIMER_TO, ARPT_PRUNE);
}

/// `arp_rtrequest`: the ARP side of a route change on an Ethernet interface: an entry for a
/// new link-layer route, its removal, an invalidation.
pub fn arp_rtrequest(ifp: &'static Ifnet, req: i32, rt: &'static Rtentry) {
    let gate = rt.rt_gateway.get();

    net_assert_locked("arp_rtrequest");

    if rt.rt_flags.get() & (RTF_GATEWAY | RTF_BROADCAST | RTF_MULTICAST | RTF_MPLS) != 0 {
        return;
    }

    let uptime = getuptime();
    let req = req as u8;
    'sw: {
        if req == RTM_ADD || req == RTM_RESOLVE {
            if req == RTM_ADD {
                if rt.rt_flags.get() & RTF_CLONING != 0 {
                    rt.rt_expire().set(0);
                    break 'sw;
                }
                if rt.rt_flags.get() & RTF_LOCAL != 0 && rt.rt_llinfo.get().is_null() {
                    rt.rt_expire().set(0);
                }
                // Announce a new entry if requested or warn the user if another station has
                // this IP address.
                if rt.rt_flags.get() & (RTF_ANNOUNCE | RTF_LOCAL) != 0 {
                    // SAFETY: an ARP route's key is a `sockaddr_in` and its gateway a
                    // `sockaddr_dl` holding the interface's address.
                    unsafe {
                        let a = (*satosin_const(rt_key(rt))).sin_addr.s_addr;
                        let mut en = [0u8; ETHER_ADDR_LEN];
                        ptr::copy_nonoverlapping(
                            lladdr(satosdl(gate)),
                            en.as_mut_ptr(),
                            ETHER_ADDR_LEN,
                        );
                        arprequest(ifp, &a, &a, &en);
                    }
                }
            }
            // FALLTHROUGH (RTM_RESOLVE)
            // SAFETY: a route's gateway is a readable socket address.
            let (gfam, glen) = unsafe { ((*gate).sa_family, usize::from((*gate).sa_len)) };
            if gfam != AF_LINK || glen < size_of::<SockaddrDl>() {
                log!(
                    LOG_DEBUG,
                    "arp_rtrequest: bad gateway value: {}\n",
                    Str(&ifp.if_xname.get())
                );
                break 'sw;
            }
            // SAFETY: an `AF_LINK` gateway of at least a `sockaddr_dl`'s size.
            unsafe {
                (*satosdl(gate)).sdl_type = ifp.if_type.get();
                (*satosdl(gate)).sdl_index = ifp.if_index.get() as u16;
            }
            // Case 2: This route may come from cloning, or a manual route add with a LL
            // address.
            let Some(mem) = pool_get(&ARP_POOL, PR_NOWAIT | PR_ZERO) else {
                log!(LOG_DEBUG, "arp_rtrequest: pool get failed\n");
                break 'sw;
            };
            let lp = mem.as_ptr().cast::<LlinfoArp>();

            mtx_enter(&ARP_MTX);
            if !rt.rt_llinfo.get().is_null() {
                // we lost the race, another thread has entered it
                mtx_leave(&ARP_MTX);
                pool_put(&ARP_POOL, mem);
                break 'sw;
            }
            // SAFETY: a fresh pool item of `size_of::<LlinfoArp>()` bytes, written whole; it
            // lives until its last reference.
            let la: &'static LlinfoArp = unsafe {
                lp.write(LlinfoArp::iterator());
                &*lp
            };
            refcnt_init(&la.la_refcnt);
            mq_init(&la.la_mq, LA_HOLD_QUEUE, IPL_SOFTNET);
            rt.rt_llinfo.set(ptr::from_ref(la).cast_mut().cast());
            la.la_rt.set(Some(rt));
            rt.rt_flags.set(rt.rt_flags.get() | RTF_LLINFO);
            // SAFETY: `la` is on no list; `arp_list` is changed under `arp_mtx`.
            unsafe { ARP_LIST.0.insert_head(la) };
            if rt.rt_flags.get() & RTF_LOCAL == 0 {
                rt.rt_expire().set(uptime);
            }
            mtx_leave(&ARP_MTX);
        } else if req == RTM_DELETE {
            mtx_enter(&ARP_MTX);
            let Some(la) = rt_la(rt) else {
                // we lost the race, another thread has removed it
                mtx_leave(&ARP_MTX);
                break 'sw;
            };
            // SAFETY: the entry is on `arp_list`, under `arp_mtx`.
            unsafe { ListHead::<LaList>::remove(la) };
            rt.rt_llinfo.set(ptr::null_mut());
            rt.rt_flags.set(rt.rt_flags.get() & !RTF_LLINFO);
            la_hold_total.fetch_sub(mq_purge(&la.la_mq), Ordering::Relaxed);
            mtx_leave(&ARP_MTX);

            if refcnt_rele(&la.la_refcnt) {
                la_put(la);
            }
        } else if req == RTM_INVALIDATE && rt.rt_flags.get() & RTF_LOCAL == 0 {
            arpinvalidate(rt);
        }
    }
}

/// `arprequest`: broadcasts an ARP request for `tip` from `sip` and `enaddr`.
pub fn arprequest(ifp: &'static Ifnet, sip: &u32, tip: &u32, enaddr: &[u8; ETHER_ADDR_LEN]) {
    let Some(m) = m_gethdr(M_DONTWAIT, MT_DATA) else {
        return;
    };
    let len = size_of::<EtherArp>();
    m.m_len().set(len as u32);
    m.m_pkthdr().len.set(len as i32);
    m.m_pkthdr().ph_rtableid.set(ifp.if_rdomain.get());
    m.m_pkthdr().pf.prio.set(ifp.if_llprio.get());
    m_align(m, len as i32);
    let mut sa = Sockaddr::default();
    let eh = EtherHeader {
        ether_dhost: ETHERBROADCASTADDR,
        ether_shost: *enaddr,
        ether_type: htons(ETHERTYPE_ARP), // if_output will not swap
    };
    let ea = EtherArp {
        ea_hdr: Arphdr {
            ar_hrd: htons(ARPHRD_ETHER),
            ar_pro: htons(ETHERTYPE_IP),
            ar_hln: ETHER_ADDR_LEN as u8, // hardware address length
            ar_pln: 4,                    // protocol address length
            ar_op: htons(ARPOP_REQUEST),
        },
        arp_sha: *enaddr,
        arp_spa: sip.to_ne_bytes(),
        arp_tha: [0; ETHER_ADDR_LEN],
        arp_tpa: tip.to_ne_bytes(),
    };
    ea_store(m, &ea);
    eh_into_sa(&mut sa, &eh);
    sa.sa_family = pseudo_AF_HDRCMPLT;
    sa.sa_len = size_of::<Sockaddr>() as u8;
    m.m_flags().set(m.m_flags().get() | M_BCAST);
    // SAFETY: a local `sockaddr` carrying the complete Ethernet header.
    let _ = unsafe { ifp_output(ifp, m, &sa) };
}

/// The `struct ether_arp` at the start of `m`'s data, as a value (the data has no alignment
/// guarantee).
fn ea_get(m: &Mbuf) -> EtherArp {
    if (m.m_len().get() as usize) < size_of::<EtherArp>() {
        panic(format_args!("arp: mbuf shorter than an ether_arp"));
    }
    // SAFETY: the first mbuf holds an `ether_arp` (checked above); it is plain bytes.
    unsafe { ptr::read_unaligned(mtod::<EtherArp>(m)) }
}

/// Writes `ea` at the start of `m`'s data.
fn ea_store(m: &Mbuf, ea: &EtherArp) {
    if (m.m_len().get() as usize) < size_of::<EtherArp>() {
        panic(format_args!("arp: mbuf shorter than an ether_arp"));
    }
    // SAFETY: as in `ea_get`.
    unsafe { ptr::write_unaligned(mtod::<EtherArp>(m), *ea) };
}

/// `eh = (struct ether_header *)sa.sa_data`: the header in the address's 14 data bytes.
fn eh_into_sa(sa: &mut Sockaddr, eh: &EtherHeader) {
    const _: () = assert!(size_of::<EtherHeader>() == 14);
    // SAFETY: `sa_data` is 14 bytes, an `ether_header`'s size; both are plain bytes.
    unsafe { ptr::write_unaligned(sa.sa_data.as_mut_ptr().cast::<EtherHeader>(), *eh) };
}

/// `ifp->if_output(ifp, m, sa, NULL)`.
///
/// # Safety
///
/// `sa` is a readable socket address (`IfOutputFn`'s contract).
unsafe fn ifp_output(
    ifp: &'static Ifnet,
    m: &'static Mbuf,
    sa: *const Sockaddr,
) -> Result<(), Errno> {
    match ifp.if_output.get() {
        // SAFETY: the caller's contract.
        Some(output) => unsafe { output(ifp, m, sa, None) },
        None => panic(format_args!("{}: no if_output", Str(&ifp.if_xname.get()))),
    }
}

/// `arpreply`: turns request `m` into the reply that `sip` is at `eaddr` and sends it.
pub fn arpreply(
    ifp: &'static Ifnet,
    m: &'static Mbuf,
    sip: &InAddr,
    eaddr: &[u8; ETHER_ADDR_LEN],
    rdomain: u32,
) {
    let mut sa = Sockaddr::default();
    let mut eh = EtherHeader::default();

    m_resethdr(m);
    m.m_pkthdr().ph_rtableid.set(rdomain);

    let mut ea = ea_get(m);
    ea.ea_hdr.ar_op = htons(ARPOP_REPLY);
    ea.ea_hdr.ar_pro = htons(ETHERTYPE_IP); // let's be sure!

    // We're replying to a request.
    ea.arp_tha = ea.arp_sha;
    ea.arp_tpa = ea.arp_spa;

    ea.arp_sha = *eaddr;
    ea.arp_spa = sip.s_addr.to_ne_bytes();
    ea_store(m, &ea);

    eh.ether_dhost = ea.arp_tha;
    eh.ether_shost = *eaddr;
    eh.ether_type = htons(ETHERTYPE_ARP);
    eh_into_sa(&mut sa, &eh);
    sa.sa_family = pseudo_AF_HDRCMPLT;
    sa.sa_len = size_of::<Sockaddr>() as u8;
    // SAFETY: a local `sockaddr` carrying the complete Ethernet header.
    let _ = unsafe { ifp_output(ifp, m, &sa) };
}

/// `arpresolve`: resolves an IP address into an ethernet address. If success, `desten` is
/// filled in. If there is no entry in arptab, set one up and broadcast a request for the IP
/// address. Hold onto this mbuf and resend it once the address is finally resolved. `Ok`
/// indicates that `desten` has been filled in and the packet should be sent normally; `EAGAIN`
/// indicates that the packet has been taken over here, either now or for later transmission.
/// Any other error indicates an error (and the packet was freed).
///
/// # Safety
///
/// `dst` points at the readable `sockaddr_in` of the destination.
pub unsafe fn arpresolve(
    ifp: &'static Ifnet,
    rt0: Option<&'static Rtentry>,
    m: &'static Mbuf,
    dst: *const Sockaddr,
    desten: &mut [u8; ETHER_ADDR_LEN],
) -> Result<(), Errno> {
    let ac = arpcom_of(ifp);
    let mut refresh = false;
    // ~RTF_REJECT, RTF_REJECT or neither.
    let mut reject: Option<bool> = None;

    // SAFETY: the caller's contract.
    let dstin = unsafe { (*satosin_const(dst)).sin_addr };
    if m.m_flags().get() & M_BCAST != 0 {
        // broadcast
        *desten = ETHERBROADCASTADDR;
        return Ok(());
    }
    if m.m_flags().get() & M_MCAST != 0 {
        // multicast
        *desten = ether_map_ip_multicast(&dstin);
        return Ok(());
    }

    let uptime = getuptime();
    let rt = rt0.and_then(rt_getll);

    let Some(rt) = rt.filter(|r| {
        !(r.rt_flags.get() & RTF_REJECT != 0
            && (r.rt_expire().get() == 0 || r.rt_expire().get() > uptime))
    }) else {
        m_freem(m);
        let same = match (rt, rt0) {
            (Some(a), Some(b)) => ptr::eq(a, b),
            (None, None) => true,
            _ => false,
        };
        return Err(if same {
            Errno::EHOSTDOWN
        } else {
            Errno::EHOSTUNREACH
        });
    };

    'bad: {
        if rt.rt_flags.get() & RTF_LLINFO == 0 {
            // SAFETY: a route's key in the inet table is a `sockaddr_in`.
            let key = unsafe { (*satosin_const(rt_key(rt))).sin_addr };
            log!(
                LOG_DEBUG,
                "arpresolve: {}: route contains no arp information\n",
                InAddrFmt(key)
            );
            break 'bad;
        }

        let sdl = satosdl(rt.rt_gateway.get());
        // SAFETY: an `RTF_LLINFO` route's gateway is a `sockaddr_dl` (`arp_rtrequest`
        // checked it).
        let (alen, family) = unsafe { ((*sdl).sdl_alen, (*sdl).sdl_family) };
        if alen > 0 && usize::from(alen) != ETHER_ADDR_LEN {
            log!(
                LOG_DEBUG,
                "arpresolve: {}: incorrect arp information\n",
                InAddrFmt(dstin)
            );
            break 'bad;
        }

        // Check the address family and length is valid, the address is resolved; otherwise,
        // try to resolve.
        if (rt.rt_expire().get() == 0 || rt.rt_expire().get() > uptime)
            && family == AF_LINK
            && alen != 0
        {
            // SAFETY: the link address is `alen` (6) bytes after the name.
            unsafe {
                ptr::copy_nonoverlapping(lladdr(sdl), desten.as_mut_ptr(), usize::from(alen))
            };

            // refresh ARP entry when timeout gets close
            if rt.rt_expire().get() != 0
                && rt.rt_expire().get() - i64::from(ARPT_KEEP.load(Ordering::Relaxed) / 8) < uptime
            {
                mtx_enter(&ARP_MTX);
                if let Some(la) = rt_la(rt)
                    && la.la_refreshed.get() + 30 < uptime
                {
                    la.la_refreshed.set(uptime);
                    refresh = true;
                }
                mtx_leave(&ARP_MTX);
            }
            if refresh {
                // SAFETY: a route's address is an `AF_INET` interface address.
                let sip = unsafe { (*satosin_const(rt.ifa().ifa_addr.get())).sin_addr.s_addr };
                arprequest(ifp, &sip, &dstin.s_addr, &ac.ac_enaddr.get());
            }
            return Ok(());
        }

        if ifp.if_flags.get() & (IFF_NOARP | IFF_STATICARP) != 0 {
            break 'bad;
        }

        mtx_enter(&ARP_MTX);
        let Some(la) = rt_la(rt) else {
            mtx_leave(&ARP_MTX);
            break 'bad;
        };

        // There is an arptab entry, but no ethernet address response yet. Insert mbuf in
        // hold queue if below limit. If above the limit free the queue without queuing the
        // new packet.
        if la_hold_total.fetch_add(1, Ordering::Relaxed) < LA_HOLD_TOTAL {
            if mq_push(&la.la_mq, m) {
                la_hold_total.fetch_sub(1, Ordering::Relaxed);
            }
        } else {
            la_hold_total.fetch_sub(mq_purge(&la.la_mq) + 1, Ordering::Relaxed);
            m_freem(m);
        }

        // Re-send the ARP request when appropriate.
        #[cfg(feature = "diagnostic")]
        if rt.rt_expire().get() == 0 {
            // This should never happen. (Should it? -gwr)
            crate::kern::subr_prf::printf(format_args!(
                "arpresolve: unresolved and rt_expire == 0\n"
            ));
            // Set expiration time to now (expired).
            rt.rt_expire().set(uptime);
        }
        if rt.rt_expire().get() != 0 {
            reject = Some(false);
            if la.la_asked.get() == 0 || rt.rt_expire().get() != uptime {
                rt.rt_expire().set(uptime);
                let asked = la.la_asked.get();
                la.la_asked.set(asked + 1);
                if asked < ARP_MAXTRIES {
                    refresh = true;
                } else {
                    reject = Some(true);
                    rt.rt_expire()
                        .set(rt.rt_expire().get() + i64::from(ARPT_DOWN.load(Ordering::Relaxed)));
                    la.la_asked.set(0);
                    la.la_refreshed.set(0);
                    la_hold_total.fetch_sub(mq_purge(&la.la_mq), Ordering::Relaxed);
                }
            }
        }
        mtx_leave(&ARP_MTX);

        // The route flags are changed under the kernel lock.
        if reject == Some(true) && rt.rt_flags.get() & RTF_REJECT == 0 {
            kernel_lock();
            rt.rt_flags.set(rt.rt_flags.get() | RTF_REJECT);
            kernel_unlock();
        }
        if reject == Some(false) && rt.rt_flags.get() & RTF_REJECT != 0 {
            kernel_lock();
            rt.rt_flags.set(rt.rt_flags.get() & !RTF_REJECT);
            kernel_unlock();
        }
        if refresh {
            // SAFETY: as above.
            let sip = unsafe { (*satosin_const(rt.ifa().ifa_addr.get())).sin_addr.s_addr };
            arprequest(ifp, &sip, &dstin.s_addr, &ac.ac_enaddr.get());
        }
        return Err(Errno::EAGAIN);
    }
    // bad:
    m_freem(m);
    Err(Errno::EINVAL)
}

/// `arppullup`: the common length and type checks of an ARP packet; `None` (freed) if bad.
fn arppullup(m: &'static Mbuf) -> Option<&'static Mbuf> {
    #[cfg(feature = "diagnostic")]
    if m.m_flags().get() & crate::sys::mbuf::M_PKTHDR == 0 {
        panic(format_args!("arp without packet header"));
    }

    let mut m = m;
    let mut len = size_of::<Arphdr>();
    if (m.m_len().get() as usize) < len {
        m = m_pullup(m, len as i32)?;
    }

    // SAFETY: the first mbuf holds an `arphdr` (pulled up above); it is integers.
    let ar = unsafe { ptr::read_unaligned(mtod::<Arphdr>(m)) };
    if ntohs(ar.ar_hrd) != ARPHRD_ETHER
        || ntohs(ar.ar_pro) != ETHERTYPE_IP
        || usize::from(ar.ar_hln) != ETHER_ADDR_LEN
        || usize::from(ar.ar_pln) != size_of::<InAddr>()
    {
        m_freem(m);
        return None;
    }

    len += 2 * (usize::from(ar.ar_hln) + usize::from(ar.ar_pln));
    if (m.m_len().get() as usize) < len {
        m = m_pullup(m, len as i32)?;
    }

    Some(m)
}

/// `arpinput`: common length and type checks are done here, then the packet is queued for
/// the protocol-specific routine.
pub fn arpinput(_ifp: &'static Ifnet, m: &'static Mbuf, _ns: Option<&Netstack>) {
    let Some(m) = arppullup(m) else {
        return;
    };
    let _ = niq_enqueue(&ARPINQ, m);
}

/// `arpintr`: the ARP soft interrupt: processes the queued packets.
pub fn arpintr() {
    let ml = MbufList::new();

    niq_delist(&ARPINQ, &ml);

    while let Some(m) = ml_dequeue(&ml) {
        let ifp = if_get(m.m_pkthdr().ph_ifidx.get());

        match ifp {
            Some(ifp) => in_arpinput(ifp, m),
            None => {
                m_freem(m);
            }
        }

        if_put(ifp);
    }
}

/// `in_arpinput`: ARP for Internet protocols on Ethernet, RFC 826. In addition, a sanity check
/// is performed on the sender protocol address, to catch impersonators.
pub fn in_arpinput(ifp: &'static Ifnet, m: &'static Mbuf) {
    let mut rt: Option<&'static Rtentry> = None;
    let mut target = false;

    let rdomain = rtable_l2(m.m_pkthdr().ph_rtableid.get());

    let ea = ea_get(m);
    let op = ntohs(ea.arp_op());
    'out: {
        if op != ARPOP_REQUEST && op != ARPOP_REPLY {
            break 'out;
        }

        let mut itaddr = InAddr {
            s_addr: u32::from_ne_bytes(ea.arp_tpa),
        };
        let isaddr = InAddr {
            s_addr: u32::from_ne_bytes(ea.arp_spa),
        };
        let mut sin = SockaddrIn {
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_family: AF_INET,
            ..SockaddrIn::default()
        };

        if ether_is_multicast(&ea.arp_sha) && ether_is_broadcast(&ea.arp_sha) {
            log!(
                LOG_ERR,
                "arp: ether address is broadcast for IP address {}!\n",
                InAddrFmt(isaddr)
            );
            break 'out;
        }

        // SAFETY: an attached interface's link address has its hardware address after the
        // name.
        let ours =
            unsafe { core::slice::from_raw_parts(lladdr(ifp.if_sadl.get()), ETHER_ADDR_LEN) };
        if ea.arp_sha[..] == *ours {
            break 'out; // it's from me, ignore it.
        }

        // Check target against our interface addresses.
        sin.sin_addr = itaddr;
        // SAFETY: a local `sockaddr_in`.
        let r = unsafe { rtalloc(sintosa(&mut sin), 0, rdomain) };
        if let Some(x) = r
            && rtisvalid(Some(x))
            && x.rt_flags.get() & RTF_LOCAL != 0
            && x.rt_ifidx.get() == ifp.if_index.get()
        {
            target = true;
        }
        rtfree(r);

        // NCARP > 0: carp_iamatch for a request on a carp interface; not configured.

        // Do we have an ARP cache for the sender? Create if we are target.
        rt = arplookup(&isaddr, target, false, rdomain);

        // Check sender against our interface addresses.
        if let Some(r) = rt
            && rtisvalid(Some(r))
            && r.rt_flags.get() & RTF_LOCAL != 0
            && r.rt_ifidx.get() == ifp.if_index.get()
            && isaddr.s_addr != INADDR_ANY
        {
            let s = ether_sprintf(&ea.arp_sha);
            log!(
                LOG_ERR,
                "duplicate IP address {} sent from ethernet address {}\n",
                InAddrFmt(isaddr),
                Str(&s)
            );
            itaddr = isaddr;
        } else if let Some(r) = rt
            && !arpcache(ifp, &ea, r)
        {
            break 'out;
        }

        if op == ARPOP_REQUEST {
            let mut eaddr = [0u8; ETHER_ADDR_LEN];

            if target {
                // We already have all info for the reply
                eaddr.copy_from_slice(ours);
            } else {
                rtfree(rt);
                rt = arplookup(&itaddr, false, true, rdomain);
                // Protect from possible duplicates, only owner should respond
                let Some(r) = rt.filter(|r| r.rt_ifidx.get() == ifp.if_index.get()) else {
                    break 'out;
                };
                // SAFETY: a proxy ARP route's gateway is a `sockaddr_dl` with an address.
                unsafe {
                    ptr::copy_nonoverlapping(
                        lladdr(satosdl(r.rt_gateway.get())),
                        eaddr.as_mut_ptr(),
                        ETHER_ADDR_LEN,
                    )
                };
            }
            arpreply(ifp, m, &itaddr, &eaddr, rdomain);
            rtfree(rt);
            return;
        }
    }
    // out:
    rtfree(rt);
    m_freem(m);
}

/// `arpcache`: records the sender of `ea` in ARP route `rt` and sends the packets held for
/// it; `false` (the C's -1) when the entry may not be changed (the caller drops the packet).
pub fn arpcache(ifp: &'static Ifnet, ea: &EtherArp, rt: &'static Rtentry) -> bool {
    let sdl = satosdl(rt.rt_gateway.get());
    let spa = InAddr {
        s_addr: u32::from_ne_bytes(ea.arp_spa),
    };
    let mut changed = false;

    net_assert_locked_exclusive("arpcache");
    kassert!(!sdl.is_null());

    // This can happen if the entry has been deleted by another CPU after we found it.
    let Some(la) = rt_la(rt) else {
        return true;
    };

    let uptime = getuptime();
    // SAFETY: an ARP route's gateway is a `sockaddr_dl` with room for an Ethernet address.
    let alen = unsafe { (*sdl).sdl_alen };
    if alen > 0 {
        // SAFETY: as above.
        let cur = unsafe { core::slice::from_raw_parts(lladdr(sdl), usize::from(alen)) };
        if ea.arp_sha[..usize::from(alen)] != *cur {
            if rt.rt_flags.get() & (RTF_PERMANENT_ARP | RTF_LOCAL) != 0 {
                let s = ether_sprintf(&ea.arp_sha);
                log!(
                    LOG_WARNING,
                    "arp: attempt to overwrite permanent entry for {} by {} on {}\n",
                    InAddrFmt(spa),
                    Str(&s),
                    Str(&ifp.if_xname.get())
                );
                return false;
            } else if rt.rt_ifidx.get() != ifp.if_index.get() {
                // NCARP > 0: no warning for a carp interface; not configured.
                let Some(rifp) = if_get(rt.rt_ifidx.get()) else {
                    return false;
                };
                let s = ether_sprintf(&ea.arp_sha);
                log!(
                    LOG_WARNING,
                    "arp: attempt to overwrite entry for {} on {} by {} on {}\n",
                    InAddrFmt(spa),
                    Str(&rifp.if_xname.get()),
                    Str(&s),
                    Str(&ifp.if_xname.get())
                );
                if_put(Some(rifp));
                return false;
            } else {
                let s = ether_sprintf(&ea.arp_sha);
                log!(
                    LOG_INFO,
                    "arp info overwritten for {} by {} on {}\n",
                    InAddrFmt(spa),
                    Str(&s),
                    Str(&ifp.if_xname.get())
                );
                rt.rt_expire().set(1); // no longer static
            }
            changed = true;
        }
    } else if !if_isconnected(ifp, rt.rt_ifidx.get()) {
        let Some(rifp) = if_get(rt.rt_ifidx.get()) else {
            return false;
        };
        let s = ether_sprintf(&ea.arp_sha);
        log!(
            LOG_WARNING,
            "arp: attempt to add entry for {} on {} by {} on {}\n",
            InAddrFmt(spa),
            Str(&rifp.if_xname.get()),
            Str(&s),
            Str(&ifp.if_xname.get())
        );
        if_put(Some(rifp));
        return false;
    }
    // SAFETY: as above.
    unsafe {
        (*sdl).sdl_alen = ETHER_ADDR_LEN as u8;
        ptr::copy_nonoverlapping(ea.arp_sha.as_ptr(), lladdr(sdl), ETHER_ADDR_LEN);
    }
    if rt.rt_expire().get() != 0 {
        rt.rt_expire()
            .set(uptime + i64::from(ARPT_KEEP.load(Ordering::Relaxed)));
    }
    rt.rt_flags.set(rt.rt_flags.get() & !RTF_REJECT);

    // Notify userland that an ARP resolution has been done.
    if la.la_asked.get() != 0 || changed {
        rtm_send(rt, RTM_RESOLVE, 0, ifp.if_rdomain.get());
    }

    la.la_asked.set(0);
    la.la_refreshed.set(0);
    // SAFETY: the route's key is a readable `sockaddr_in`.
    let _ = unsafe { if_output_mq(ifp, &la.la_mq, &la_hold_total, rt_key(rt), Some(rt)) };

    true
}

/// `arpinvalidate`: forgets the hardware address of ARP route `rt` and its held packets.
pub fn arpinvalidate(rt: &Rtentry) {
    let sdl = satosdl(rt.rt_gateway.get());

    mtx_enter(&ARP_MTX);
    let Some(la) = rt_la(rt) else {
        mtx_leave(&ARP_MTX);
        return;
    };
    la_hold_total.fetch_sub(mq_purge(&la.la_mq), Ordering::Relaxed);
    // SAFETY: an ARP route's gateway is a `sockaddr_dl`.
    unsafe { (*sdl).sdl_alen = 0 };
    la.la_asked.set(0);
    mtx_leave(&ARP_MTX);
}

/// `arptfree`: frees an arp entry.
pub fn arptfree(rt: &'static Rtentry) {
    net_assert_locked_exclusive("arptfree");

    // might have been freed between leave arp_mtx and enter net lock
    if rt.rt_flags.get() & RTF_LLINFO == 0 {
        return;
    }

    kassert!(rt.rt_flags.get() & RTF_LOCAL == 0);
    arpinvalidate(rt);

    let Some(ifp) = if_get(rt.rt_ifidx.get()) else {
        return;
    };
    if rt.rt_flags.get() & (RTF_STATIC | RTF_CACHED) == 0 {
        let _ = rtdeletemsg(rt, ifp, ifp.if_rdomain.get());
    }
    if_put(Some(ifp));
}

/// `arplookup`: looks up (or, with `create`, enters) an address in arptab; with `proxy`, the
/// published (`RTF_ANNOUNCE`) entry.
pub fn arplookup(
    inp: &InAddr,
    create: bool,
    proxy: bool,
    tableid: u32,
) -> Option<&'static Rtentry> {
    let mut sin = SockaddrInarp {
        sin_len: size_of::<SockaddrInarp>() as u8,
        sin_family: AF_INET,
        sin_addr: *inp,
        sin_other: if proxy { SIN_PROXY } else { 0 },
        ..SockaddrInarp::default()
    };
    let flags = if create { RT_RESOLVE } else { 0 };

    // SAFETY: a local `sockaddr_inarp`, a `sockaddr_in` with more after the address.
    let rt = unsafe { rtalloc(ptr::from_mut(&mut sin).cast(), flags, tableid) };
    let Some(mut r) = rt.filter(|r| {
        rtisvalid(Some(r))
            && r.rt_flags.get() & RTF_GATEWAY == 0
            && r.rt_flags.get() & RTF_LLINFO != 0
            // SAFETY: a route's gateway is a readable socket address.
            && unsafe { (*r.rt_gateway.get()).sa_family } == AF_LINK
    }) else {
        rtfree(rt);
        return None;
    };

    if proxy && r.rt_flags.get() & RTF_ANNOUNCE == 0 {
        loop {
            r = rtable_iterate(r)?;
            if r.rt_flags.get() & RTF_ANNOUNCE != 0 {
                break;
            }
        }
    }

    Some(r)
}

/// `arpproxy`: whether we do proxy ARP for this address and we point to ourselves.
pub fn arpproxy(in_: InAddr, rtableid: u32) -> bool {
    let rt = arplookup(&in_, false, true, rtableid);
    let Some(r) = rt.filter(|r| rtisvalid(Some(r))) else {
        rtfree(rt);
        return false;
    };

    // Check that arp information are correct.
    let sdl = satosdl(r.rt_gateway.get());
    // SAFETY: an ARP route's gateway is a `sockaddr_dl`.
    if usize::from(unsafe { (*sdl).sdl_alen }) != ETHER_ADDR_LEN {
        rtfree(Some(r));
        return false;
    }

    let Some(ifp) = if_get(r.rt_ifidx.get()) else {
        rtfree(Some(r));
        return false;
    };

    // SAFETY: both link addresses hold `ETHER_ADDR_LEN` bytes after their names.
    let found = unsafe {
        core::slice::from_raw_parts(lladdr(sdl), ETHER_ADDR_LEN)
            == core::slice::from_raw_parts(lladdr(ifp.if_sadl.get()), ETHER_ADDR_LEN)
    };

    if_put(Some(ifp));
    rtfree(Some(r));
    found
}

/// `revarpinput`: called from Ethernet interrupt handlers when ether packet type
/// `ETHERTYPE_REVARP` is received. Common length and type checks are done here, then the
/// protocol-specific routine is called.
pub fn revarpinput(ifp: &'static Ifnet, m: &'static Mbuf, _ns: Option<&Netstack>) {
    let Some(m) = arppullup(m) else {
        return;
    };
    in_revarpinput(ifp, m);
}

/// `in_revarpinput`: RARP for Internet protocols on Ethernet. Algorithm is that given in RFC
/// 903. We are only using for bootstrap purposes to get an ip address for one of our
/// interfaces. Thus we support no user-interface. Since the contents of the RARP reply are
/// specific to the interface that sent the request, this code must ensure that they are
/// properly associated. Note: also supports ARP via RARP packets, per the RFC.
pub fn in_revarpinput(ifp: &'static Ifnet, m: &'static Mbuf) {
    let ar = ea_get(m);
    match ntohs(ar.arp_op()) {
        ARPOP_REQUEST | ARPOP_REPLY => {
            // per RFC
            let _ = niq_enqueue(&ARPINQ, m);
            return;
        }
        ARPOP_REVREPLY => {
            // NFSCLIENT: the reply to revarpwhoarewe; without it the reply is dropped.
            #[cfg(feature = "nfsclient")]
            revarp_reply(ifp, m, &ar);
            #[cfg(not(feature = "nfsclient"))]
            let _ = ifp;
        }
        // ARPOP_REVREQUEST: handled by rarpd(8); default:
        _ => {}
    }

    // out:
    m_freem(m);
}

/// The `ARPOP_REVREPLY` case of `in_revarpinput` (`NFSCLIENT`): a reply to our request, if
/// it is on the interface that asked and for our hardware address.
#[cfg(feature = "nfsclient")]
fn revarp_reply(ifp: &'static Ifnet, m: &'static Mbuf, ar: &EtherArp) {
    let ifidx = REVARP_IFIDX.load(Ordering::Relaxed);
    if ifidx == 0 {
        return;
    }
    if ifidx != m.m_pkthdr().ph_ifidx.get() {
        // !same interface
        return;
    }
    if !REVARP_FINISHED.load(Ordering::Relaxed) {
        // SAFETY: an attached interface's link address has its hardware address after the
        // name.
        let ours =
            unsafe { core::slice::from_raw_parts(lladdr(ifp.if_sadl.get()), ETHER_ADDR_LEN) };
        if ar.arp_tha[..] != *ours {
            return;
        }
        REVARP_SRVIP.store(u32::from_ne_bytes(ar.arp_spa), Ordering::Relaxed);
        REVARP_MYIP.store(u32::from_ne_bytes(ar.arp_tpa), Ordering::Relaxed);
        REVARP_FINISHED.store(true, Ordering::Relaxed);
    }
    // wake: Do wakeup every time in case it was missed.
    wakeup(&REVARP_MYIP);
}

/// `revarprequest`: send a RARP request for the ip address of the specified interface. The
/// request should be RFC 903-compliant.
#[cfg(feature = "nfsclient")]
pub fn revarprequest(ifp: &'static Ifnet) {
    let ac = arpcom_of(ifp);
    let enaddr = ac.ac_enaddr.get();

    let Some(m) = m_gethdr(M_DONTWAIT, MT_DATA) else {
        return;
    };
    let len = size_of::<EtherArp>();
    m.m_len().set(len as u32);
    m.m_pkthdr().len.set(len as i32);
    m.m_pkthdr().ph_rtableid.set(ifp.if_rdomain.get());
    m.m_pkthdr().pf.prio.set(ifp.if_llprio.get());
    m_align(m, len as i32);
    let mut sa = Sockaddr::default();
    let eh = EtherHeader {
        ether_dhost: ETHERBROADCASTADDR,
        ether_shost: enaddr,
        ether_type: htons(ETHERTYPE_REVARP), // if_output will not swap
    };
    let ea = EtherArp {
        ea_hdr: Arphdr {
            ar_hrd: htons(ARPHRD_ETHER),
            ar_pro: htons(ETHERTYPE_IP),
            ar_hln: ETHER_ADDR_LEN as u8, // hardware address length
            ar_pln: 4,                    // protocol address length
            ar_op: htons(ARPOP_REVREQUEST),
        },
        arp_sha: enaddr,
        arp_spa: [0; 4],
        arp_tha: enaddr,
        arp_tpa: [0; 4],
    };
    ea_store(m, &ea);
    eh_into_sa(&mut sa, &eh);
    sa.sa_family = pseudo_AF_HDRCMPLT;
    sa.sa_len = size_of::<Sockaddr>() as u8;
    m.m_flags().set(m.m_flags().get() | M_BCAST);
    // SAFETY: a local `sockaddr` carrying the complete Ethernet header.
    let _ = unsafe { ifp_output(ifp, m, &sa) };
}

/// `revarpwhoarewe`: RARP for the ip address of the specified interface, but also save the ip
/// address of the server that sent the answer. Timeout if no response is received. Returns
/// the server's and the client's addresses.
#[cfg(feature = "nfsclient")]
pub fn revarpwhoarewe(ifp: &'static Ifnet) -> Result<(InAddr, InAddr), Errno> {
    let mut count = 20;

    if REVARP_FINISHED.load(Ordering::Relaxed) {
        return Err(Errno::EIO);
    }

    REVARP_IFIDX.store(ifp.if_index.get(), Ordering::Relaxed);
    while count > 0 {
        count -= 1;
        revarprequest(ifp);
        let result = tsleep_nsec(&REVARP_MYIP, PSOCK, "revarp", msec_to_nsec(500));
        if result != Err(Errno::EWOULDBLOCK) {
            break;
        }
    }
    REVARP_IFIDX.store(0, Ordering::Relaxed);
    if !REVARP_FINISHED.load(Ordering::Relaxed) {
        return Err(Errno::ENETUNREACH);
    }

    let serv = InAddr {
        s_addr: REVARP_SRVIP.load(Ordering::Relaxed),
    };
    let clnt = InAddr {
        s_addr: REVARP_MYIP.load(Ordering::Relaxed),
    };
    Ok((serv, clnt))
}

/// `revarpwhoami`: for compatibility: only saves the interface address.
#[cfg(feature = "nfsclient")]
pub fn revarpwhoami(ifp: &'static Ifnet) -> Result<InAddr, Errno> {
    let (_server, clnt) = revarpwhoarewe(ifp)?;
    Ok(clnt)
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
