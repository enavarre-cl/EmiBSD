/*	$OpenBSD: if.h,v 1.224 2026/06/23 14:40:40 bluhm Exp $	*/
/*	$NetBSD: if.h,v 1.23 1996/05/07 02:40:27 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)if.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! Network interfaces as user space and the routing socket see them: `<net/if.h>`.
//!
//! Upstream: sys/net/if.h @ 3ce1f3f79392
//!
//! `if` is a Rust keyword, so the module is `if_` (`docs/C_TO_RUST.md`). The kernel's own
//! `struct ifnet` is in `<net/if_var.h>`, not here; this header has the interface flags, the
//! statistics block, the routing-socket messages and the `ioctl(2)` request structures. The
//! structures cross the user/kernel boundary, so they are `#[repr(C)]` with their LP64 sizes
//! checked at compile time. The C's unnamed unions are `#[repr(C)]` unions named after their
//! member (`ifr_ifru` → [`IfrIfru`]), and the C's `#define ifr_flags ifr_ifru.ifru_flags`
//! shorthands are accessor methods (`ifr.ifr_flags()`, `ifr.set_ifr_flags(f)`).
//!
//! Status: `wip`.
//!
//! ## Deviations
//! - `CTL_IFQ_NAMES` (a `struct ctlname` table) comes with `<sys/sysctl.h>`.
//! - `LINK_STATE_DESCRIPTIONS` is a slice of [`IfStatusDescription`] whose strings are byte
//!   slices; the C's terminating `{ 0, 0, NULL }` entry is the slice's end.
//! - `IFG_ALL` and `IFG_EGRESS` are byte strings without the NUL.
//! - The prototypes (`if_attach`, `ifioctl`, `if_get`, ...) come with `net/if.c`.
//! - `#include <net/if_arp.h>` is `crate::net::if_arp`, a module of its own.

use core::mem::size_of;

use crate::net::if_types::{IFT_CARP, IFT_ETHER, IFT_IEEE80211, IFT_PPP};
use crate::sys::socket::{Sockaddr, SockaddrStorage};
use crate::sys::time::Timeval;
use crate::sys::types::SaFamily;

/// Length of interface external name, including terminating '\0'. Note: this is the same size
/// as a generic device's external name.
pub const IF_NAMESIZE: usize = 16;

/// Number of cluster pools.
pub const MCLPOOLS: usize = 8;

/// `IFQ_NQUEUES`: priority queues per interface queue.
pub const IFQ_NQUEUES: u32 = 8;
/// `IFQ_MINPRIO`.
pub const IFQ_MINPRIO: u32 = 0;
/// `IFQ_MAXPRIO`.
pub const IFQ_MAXPRIO: u32 = IFQ_NQUEUES - 1;
/// `IFQ_DEFPRIO`.
pub const IFQ_DEFPRIO: u32 = 3;

// Values for if_link_state.

/// Link unknown.
pub const LINK_STATE_UNKNOWN: u8 = 0;
/// Link invalid.
pub const LINK_STATE_INVALID: u8 = 1;
/// Link is down.
pub const LINK_STATE_DOWN: u8 = 2;
/// Keepalive reports down.
pub const LINK_STATE_KALIVE_DOWN: u8 = 3;
/// Link is up.
pub const LINK_STATE_UP: u8 = 4;
/// Link is up and half duplex.
pub const LINK_STATE_HALF_DUPLEX: u8 = 5;
/// Link is up and full duplex.
pub const LINK_STATE_FULL_DUPLEX: u8 = 6;

/// Traditional BSD name for length of interface external name.
pub const IFNAMSIZ: usize = IF_NAMESIZE;

/// Length of interface description, including terminating '\0'.
pub const IFDESCRSIZE: usize = 64;

// Interface flags can be either owned by the stack or the driver. The comments say who toggles
// which flag: [I] immutable after creation, [N] written by the stack (upon user request), [d]
// written by the driver, [c] for userland compatibility only.

/// [N] interface is up.
pub const IFF_UP: i32 = 0x1;
/// [I] broadcast address valid.
pub const IFF_BROADCAST: i32 = 0x2;
/// [N] turn on debugging.
pub const IFF_DEBUG: i32 = 0x4;
/// [I] is a loopback net.
pub const IFF_LOOPBACK: i32 = 0x8;
/// [I] is point-to-point link.
pub const IFF_POINTOPOINT: i32 = 0x10;
/// [N] only static ARP.
pub const IFF_STATICARP: i32 = 0x20;
/// [d] resources allocated.
pub const IFF_RUNNING: i32 = 0x40;
/// [N] no address resolution protocol.
pub const IFF_NOARP: i32 = 0x80;
/// [N] receive all packets.
pub const IFF_PROMISC: i32 = 0x100;
/// [d] receive all multicast packets.
pub const IFF_ALLMULTI: i32 = 0x200;
/// [c] transmission in progress.
pub const IFF_OACTIVE: i32 = 0x400;
/// [I] can't hear own transmissions.
pub const IFF_SIMPLEX: i32 = 0x800;
/// [N] per link layer defined bit.
pub const IFF_LINK0: i32 = 0x1000;
/// [N] per link layer defined bit.
pub const IFF_LINK1: i32 = 0x2000;
/// [N] per link layer defined bit.
pub const IFF_LINK2: i32 = 0x4000;
/// [I] supports multicast.
pub const IFF_MULTICAST: i32 = 0x8000;

/// Flags set internally only.
pub const IFF_CANTCHANGE: i32 = IFF_BROADCAST
    | IFF_LOOPBACK
    | IFF_POINTOPOINT
    | IFF_RUNNING
    | IFF_OACTIVE
    | IFF_SIMPLEX
    | IFF_MULTICAST
    | IFF_ALLMULTI;

/// [I] `if_start` is mpsafe.
pub const IFXF_MPSAFE: i32 = 0x1;
/// [I] pseudo interface.
pub const IFXF_CLONED: i32 = 0x2;
/// [N] v6 temporary addrs enabled.
pub const IFXF_AUTOCONF6TEMP: i32 = 0x4;
/// [N] supports MPLS.
pub const IFXF_MPLS: i32 = 0x8;
/// [N] wake on lan enabled.
pub const IFXF_WOL: i32 = 0x10;
/// [N] v6 autoconf enabled.
pub const IFXF_AUTOCONF6: i32 = 0x20;
/// [N] don't do RFC 7217.
pub const IFXF_INET6_NOSOII: i32 = 0x40;
/// [N] v4 autoconf (aka dhcp) enabled.
pub const IFXF_AUTOCONF4: i32 = 0x80;
/// [N] only used for bpf.
pub const IFXF_MONITOR: i32 = 0x100;
/// [N] TCP large recv offload.
pub const IFXF_LRO: i32 = 0x200;
/// [I] mbuf with 64 bit DMA supported.
pub const IFXF_MBUF_64BIT: i32 = 0x400;

/// Extended flags the user cannot change.
pub const IFXF_CANTCHANGE: i32 = IFXF_MPSAFE | IFXF_CLONED;

// Capabilities that interfaces can advertise.

/// Can do IPv4 header csum.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_CSUM_IPv4: u32 = 0x0000_0001;
/// Can do IPv4/TCP csum.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_CSUM_TCPv4: u32 = 0x0000_0002;
/// Can do IPv4/UDP csum.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_CSUM_UDPv4: u32 = 0x0000_0004;
/// VLAN-compatible MTU.
pub const IFCAP_VLAN_MTU: u32 = 0x0000_0010;
/// Hardware VLAN tag support.
pub const IFCAP_VLAN_HWTAGGING: u32 = 0x0000_0020;
/// HW offload w/ inline tag.
pub const IFCAP_VLAN_HWOFFLOAD: u32 = 0x0000_0040;
/// Can do IPv6/TCP checksums.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_CSUM_TCPv6: u32 = 0x0000_0080;
/// Can do IPv6/UDP checksums.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_CSUM_UDPv6: u32 = 0x0000_0100;
/// IPv4/TCP segment offload.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_TSOv4: u32 = 0x0000_1000;
/// IPv6/TCP segment offload.
#[allow(non_upper_case_globals)] // the C name
pub const IFCAP_TSOv6: u32 = 0x0000_2000;
/// TCP large recv offload.
pub const IFCAP_LRO: u32 = 0x0000_4000;
/// Can do wake on lan.
pub const IFCAP_WOL: u32 = 0x0000_8000;

/// The checksum offload capabilities.
pub const IFCAP_CSUM_MASK: u32 =
    IFCAP_CSUM_IPv4 | IFCAP_CSUM_TCPv4 | IFCAP_CSUM_UDPv4 | IFCAP_CSUM_TCPv6 | IFCAP_CSUM_UDPv6;

// Symbolic names for terminal (per-protocol) CTL_IFQ_ nodes.

/// `IFQCTL_LEN`.
pub const IFQCTL_LEN: i32 = 1;
/// `IFQCTL_MAXLEN`.
pub const IFQCTL_MAXLEN: i32 = 2;
/// `IFQCTL_DROPS`.
pub const IFQCTL_DROPS: i32 = 3;
/// `IFQCTL_CONGESTION`.
pub const IFQCTL_CONGESTION: i32 = 4;
/// `IFQCTL_MAXID`.
pub const IFQCTL_MAXID: i32 = 5;

/// Interface arrival.
pub const IFAN_ARRIVAL: u16 = 0;
/// Interface departure.
pub const IFAN_DEPARTURE: u16 = 1;

/// Group contains all interfaces.
pub const IFG_ALL: &[u8] = b"all";
/// If(s) default route(s) point to.
pub const IFG_EGRESS: &[u8] = b"egress";

/// `IF_HDRPRIO_MIN`.
pub const IF_HDRPRIO_MIN: i32 = IFQ_MINPRIO as i32;
/// `IF_HDRPRIO_MAX`.
pub const IF_HDRPRIO_MAX: i32 = IFQ_MAXPRIO as i32;
/// Use mbuf prio.
pub const IF_HDRPRIO_PACKET: i32 = -1;
/// Copy payload prio.
pub const IF_HDRPRIO_PAYLOAD: i32 = -2;
/// Use outer prio.
pub const IF_HDRPRIO_OUTER: i32 = -3;

/// Ethernet or ethernet tagged.
pub const IF_PWE3_ETHERNET: i32 = 1;
/// IP layer 2.
pub const IF_PWE3_IP: i32 = 2;

/// In: prefix given; out: kernel fills id.
pub const IFLR_PREFIX: u32 = 0x8000;

/// `IFSFF_ADDR_EEPROM`.
pub const IFSFF_ADDR_EEPROM: u8 = 0xa0;
/// `IFSFF_ADDR_DDM`.
pub const IFSFF_ADDR_DDM: u8 = 0xa2;

/// `IFSFF_DATA_LEN`.
pub const IFSFF_DATA_LEN: usize = 256;

/// `IF_MAX_VECTORS`: most interrupt vectors (and queues) one interface uses.
pub const IF_MAX_VECTORS: usize = 8;

/// `struct if_nameindex`: one entry of `if_nameindex(3)`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IfNameindex {
    /// Interface index.
    pub if_index: u32,
    /// Interface name.
    pub if_name: *mut u8,
}

/// `struct if_clonereq`: structure used to query names of interface cloners.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IfClonereq {
    /// Total cloners (out).
    pub ifcr_total: i32,
    /// Room for this many in user buffer.
    pub ifcr_count: i32,
    /// Buffer for cloner names.
    pub ifcr_buffer: *mut u8,
}

/// `struct if_rxring`: a receive ring's watermarks.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfRxring {
    /// `rxr_adjusted`.
    pub rxr_adjusted: i32,
    /// `rxr_alive`.
    pub rxr_alive: u32,
    /// Current watermark.
    pub rxr_cwm: u32,
    /// Low watermark.
    pub rxr_lwm: u32,
    /// High watermark.
    pub rxr_hwm: u32,
}

/// `struct if_rxring_info`: one ring of a `SIOCGIFRXR` answer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfRxringInfo {
    /// Name of the ring.
    pub ifr_name: [u8; 16],
    /// Size of the packets on the ring.
    pub ifr_size: u32,
    /// The ring's watermarks.
    pub ifr_info: IfRxring,
}

/// `struct if_rxrinfo`: structure used in `SIOCGIFRXR` request.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IfRxrinfo {
    /// `ifri_total`.
    pub ifri_total: u32,
    /// `ifri_entries`.
    pub ifri_entries: *mut IfRxringInfo,
}

/// `struct if_data`: structure defining statistics and other data kept regarding a network
/// interface.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfData {
    // generic interface information
    /// Ethernet, tokenring, etc.
    pub ifi_type: u8,
    /// Media address length.
    pub ifi_addrlen: u8,
    /// Media header length.
    pub ifi_hdrlen: u8,
    /// Current link state.
    pub ifi_link_state: u8,
    /// Maximum transmission unit.
    pub ifi_mtu: u32,
    /// Routing metric (external only).
    pub ifi_metric: u32,
    /// Routing instance.
    pub ifi_rdomain: u32,
    /// Linespeed.
    pub ifi_baudrate: u64,
    // volatile statistics
    /// Packets received on interface.
    pub ifi_ipackets: u64,
    /// Input errors on interface.
    pub ifi_ierrors: u64,
    /// Packets sent on interface.
    pub ifi_opackets: u64,
    /// Output errors on interface.
    pub ifi_oerrors: u64,
    /// Collisions on csma interfaces.
    pub ifi_collisions: u64,
    /// Total number of octets received.
    pub ifi_ibytes: u64,
    /// Total number of octets sent.
    pub ifi_obytes: u64,
    /// Packets received via multicast.
    pub ifi_imcasts: u64,
    /// Packets sent via multicast.
    pub ifi_omcasts: u64,
    /// Dropped on input, this interface.
    pub ifi_iqdrops: u64,
    /// Dropped on output, this interface.
    pub ifi_oqdrops: u64,
    /// Destined for unsupported protocol.
    pub ifi_noproto: u64,
    /// Interface capabilities.
    pub ifi_capabilities: u32,
    /// Last operational state change.
    pub ifi_lastchange: Timeval,
}

/// `struct if_status_description`: status bit descriptions for the various interface types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IfStatusDescription {
    /// Interface type the description applies to, 0 for any.
    pub ifs_type: u8,
    /// Link state.
    pub ifs_state: u8,
    /// The description.
    pub ifs_string: &'static [u8],
}

/// `struct if_msghdr`: message format for use in obtaining information about interfaces from
/// sysctl and the routing socket.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfMsghdr {
    /// To skip over non-understood messages.
    pub ifm_msglen: u16,
    /// Future binary compatibility.
    pub ifm_version: u8,
    /// Message type.
    pub ifm_type: u8,
    /// sizeof(if_msghdr) to skip over the header.
    pub ifm_hdrlen: u16,
    /// Index for associated ifp.
    pub ifm_index: u16,
    /// Routing table id.
    pub ifm_tableid: u16,
    /// Padding.
    pub ifm_pad1: u8,
    /// Padding.
    pub ifm_pad2: u8,
    /// Like `rtm_addrs`.
    pub ifm_addrs: i32,
    /// Value of `if_flags`.
    pub ifm_flags: i32,
    /// Value of `if_xflags`.
    pub ifm_xflags: i32,
    /// Statistics and other data about if.
    pub ifm_data: IfData,
}

/// `struct ifa_msghdr`: message format for use in obtaining information about interface
/// addresses from sysctl and the routing socket.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfaMsghdr {
    /// To skip over non-understood messages.
    pub ifam_msglen: u16,
    /// Future binary compatibility.
    pub ifam_version: u8,
    /// Message type.
    pub ifam_type: u8,
    /// sizeof(ifa_msghdr) to skip over the header.
    pub ifam_hdrlen: u16,
    /// Index for associated ifp.
    pub ifam_index: u16,
    /// Routing table id.
    pub ifam_tableid: u16,
    /// Padding.
    pub ifam_pad1: u8,
    /// Padding.
    pub ifam_pad2: u8,
    /// Like `rtm_addrs`.
    pub ifam_addrs: i32,
    /// Value of `ifa_flags`.
    pub ifam_flags: i32,
    /// Value of `ifa_metric`.
    pub ifam_metric: i32,
}

/// `struct if_announcemsghdr`: message format announcing the arrival or departure of a network
/// interface.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfAnnouncemsghdr {
    /// To skip over non-understood messages.
    pub ifan_msglen: u16,
    /// Future binary compatibility.
    pub ifan_version: u8,
    /// Message type.
    pub ifan_type: u8,
    /// sizeof(if_announcemsghdr) to skip header.
    pub ifan_hdrlen: u16,
    /// Index for associated ifp.
    pub ifan_index: u16,
    /// What type of announcement.
    pub ifan_what: u16,
    /// If name, e.g. "en0".
    pub ifan_name: [u8; IFNAMSIZ],
}

/// `struct if_ieee80211_data`: message format used to pass 80211 interface info.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfIeee80211Data {
    /// `IEEE80211_CHAN_MAX` == 255.
    pub ifie_channel: u8,
    /// Length of `ifie_nwid`.
    pub ifie_nwid_len: u8,
    /// `ieee80211com.ic_flags`.
    pub ifie_flags: u32,
    /// `ieee80211com.ic_xflags`.
    pub ifie_xflags: u32,
    /// `IEEE80211_NWID_LEN`.
    pub ifie_nwid: [u8; 32],
    /// `IEEE80211_ADDR_LEN`.
    pub ifie_addr: [u8; 6],
}

/// `struct if_ieee80211_msghdr`: the routing message carrying [`IfIeee80211Data`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfIeee80211Msghdr {
    /// To skip over non-understood messages.
    pub ifim_msglen: u16,
    /// Future binary compatibility.
    pub ifim_version: u8,
    /// Message type.
    pub ifim_type: u8,
    /// sizeof(if_ieee80211_msghdr) to skip over the header.
    pub ifim_hdrlen: u16,
    /// Index for associated ifp.
    pub ifim_index: u16,
    /// Routing table id.
    pub ifim_tableid: u16,
    /// The interface data.
    pub ifim_ifie: IfIeee80211Data,
}

/// `struct if_nameindex_msg`: message format used to pass interface name to index mappings.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfNameindexMsg {
    /// Interface index.
    pub if_index: u32,
    /// Interface name.
    pub if_name: [u8; IFNAMSIZ],
}

/// The unnamed union of `struct ifg_req`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IfgrqIfgrqu {
    /// A group name.
    pub ifgrqu_group: [u8; IFNAMSIZ],
    /// A member name.
    pub ifgrqu_member: [u8; IFNAMSIZ],
}

/// `struct ifg_req`: one group (or member) name of a `SIOCGIFGROUP`/`SIOCGIFGMEMB` answer.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IfgReq {
    /// The name.
    pub ifgrq_ifgrqu: IfgrqIfgrqu,
}

impl IfgReq {
    /// `ifgrq_group`.
    pub fn ifgrq_group(&self) -> &[u8; IFNAMSIZ] {
        // SAFETY: both members are the same byte array; every bit pattern is valid.
        unsafe { &self.ifgrq_ifgrqu.ifgrqu_group }
    }

    /// `ifgrq_member`.
    pub fn ifgrq_member(&self) -> &[u8; IFNAMSIZ] {
        // SAFETY: both members are the same byte array; every bit pattern is valid.
        unsafe { &self.ifgrq_ifgrqu.ifgrqu_member }
    }
}

/// `struct ifg_attrib`: attributes of an interface group.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfgAttrib {
    /// CARP demotion counter.
    pub ifg_carp_demoted: i32,
}

/// The unnamed union of `struct ifgroupreq`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IfgrIfgru {
    /// A group name.
    pub ifgru_group: [u8; IFNAMSIZ],
    /// The user's array of group requests.
    pub ifgru_groups: *mut IfgReq,
    /// The group's attributes.
    pub ifgru_attrib: IfgAttrib,
}

/// `struct ifgroupreq`: used to lookup groups for an interface.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ifgroupreq {
    /// Interface name.
    pub ifgr_name: [u8; IFNAMSIZ],
    /// Length of the `ifgr_groups` buffer.
    pub ifgr_len: u32,
    /// The request's argument.
    pub ifgr_ifgru: IfgrIfgru,
}

impl Ifgroupreq {
    /// `ifgr_group`.
    pub fn ifgr_group(&self) -> &[u8; IFNAMSIZ] {
        // SAFETY: the union's members are plain data; every bit pattern is a valid byte array.
        unsafe { &self.ifgr_ifgru.ifgru_group }
    }

    /// `ifgr_group`, writable.
    pub fn ifgr_group_mut(&mut self) -> &mut [u8; IFNAMSIZ] {
        // SAFETY: as above; writing bytes cannot make another member invalid.
        unsafe { &mut self.ifgr_ifgru.ifgru_group }
    }

    /// `ifgr_groups`: the user address of the group array.
    pub fn ifgr_groups(&self) -> *mut IfgReq {
        // SAFETY: every bit pattern is a valid raw pointer; it is not dereferenced here.
        unsafe { self.ifgr_ifgru.ifgru_groups }
    }

    /// `ifgr_attrib`.
    pub fn ifgr_attrib(&self) -> IfgAttrib {
        // SAFETY: every bit pattern is a valid `int`.
        unsafe { self.ifgr_ifgru.ifgru_attrib }
    }
}

/// The unnamed union of `struct ifreq`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IfrIfru {
    /// Address.
    pub ifru_addr: Sockaddr,
    /// Other end of p-to-p link.
    pub ifru_dstaddr: Sockaddr,
    /// Broadcast address.
    pub ifru_broadaddr: Sockaddr,
    /// Flags.
    pub ifru_flags: i16,
    /// Metric, and the other `int` overloads.
    pub ifru_metric: i32,
    /// Virtual Net Id.
    pub ifru_vnetid: i64,
    /// Media options.
    pub ifru_media: u64,
    /// For use by interface (a user address).
    pub ifru_data: *mut u8,
    /// Interface index.
    pub ifru_index: u32,
}

/// `struct ifreq`: interface request structure used for socket ioctl's. All interface ioctl's
/// must have parameter definitions which begin with `ifr_name`. The remainder may be interface
/// specific.
///
/// Every member of the union is plain data, valid for any bit pattern, so the accessors are
/// safe; they are the C's `ifr_*` shorthands.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ifreq {
    /// If name, e.g. "en0".
    pub ifr_name: [u8; IFNAMSIZ],
    /// The request's argument.
    pub ifr_ifru: IfrIfru,
}

/// Defines an `ifreq` accessor pair for a `Copy` member of the union.
macro_rules! ifr_accessor {
    ($(#[$doc:meta] $get:ident, $set:ident => $field:ident: $ty:ty;)*) => {
        $(
            #[$doc]
            pub fn $get(&self) -> $ty {
                // SAFETY: every member of `ifr_ifru` is plain data, valid for any bit pattern.
                unsafe { self.ifr_ifru.$field }
            }

            #[$doc]
            pub fn $set(&mut self, v: $ty) {
                self.ifr_ifru.$field = v;
            }
        )*
    };
}

impl Ifreq {
    /// An all-zero request.
    pub const fn zeroed() -> Self {
        Self {
            ifr_name: [0; IFNAMSIZ],
            ifr_ifru: IfrIfru {
                ifru_addr: Sockaddr {
                    sa_len: 0,
                    sa_family: 0,
                    sa_data: [0; 14],
                },
            },
        }
    }

    /// `ifr_addr`: address.
    pub fn ifr_addr(&self) -> &Sockaddr {
        // SAFETY: every member of `ifr_ifru` is plain data, valid for any bit pattern.
        unsafe { &self.ifr_ifru.ifru_addr }
    }

    /// `ifr_addr`, writable.
    pub fn ifr_addr_mut(&mut self) -> &mut Sockaddr {
        // SAFETY: as above; a `Sockaddr` is bytes, so writing it leaves every member valid.
        unsafe { &mut self.ifr_ifru.ifru_addr }
    }

    /// `ifr_dstaddr`: other end of p-to-p link.
    pub fn ifr_dstaddr(&self) -> &Sockaddr {
        // SAFETY: every member of `ifr_ifru` is plain data, valid for any bit pattern.
        unsafe { &self.ifr_ifru.ifru_dstaddr }
    }

    /// `ifr_dstaddr`, writable.
    pub fn ifr_dstaddr_mut(&mut self) -> &mut Sockaddr {
        // SAFETY: as for `ifr_addr_mut`.
        unsafe { &mut self.ifr_ifru.ifru_dstaddr }
    }

    /// `ifr_broadaddr`: broadcast address.
    pub fn ifr_broadaddr(&self) -> &Sockaddr {
        // SAFETY: every member of `ifr_ifru` is plain data, valid for any bit pattern.
        unsafe { &self.ifr_ifru.ifru_broadaddr }
    }

    /// `ifr_broadaddr`, writable.
    pub fn ifr_broadaddr_mut(&mut self) -> &mut Sockaddr {
        // SAFETY: as for `ifr_addr_mut`.
        unsafe { &mut self.ifr_ifru.ifru_broadaddr }
    }

    ifr_accessor! {
        /// `ifr_flags`: flags.
        ifr_flags, set_ifr_flags => ifru_flags: i16;
        /// `ifr_metric`: metric.
        ifr_metric, set_ifr_metric => ifru_metric: i32;
        /// `ifr_mtu`: mtu (overload).
        ifr_mtu, set_ifr_mtu => ifru_metric: i32;
        /// `ifr_hardmtu`: hardmtu (overload).
        ifr_hardmtu, set_ifr_hardmtu => ifru_metric: i32;
        /// `ifr_media`: media options.
        ifr_media, set_ifr_media => ifru_media: u64;
        /// `ifr_rdomainid`: VRF instance (overload).
        ifr_rdomainid, set_ifr_rdomainid => ifru_metric: i32;
        /// `ifr_vnetid`: Virtual Net Id.
        ifr_vnetid, set_ifr_vnetid => ifru_vnetid: i64;
        /// `ifr_ttl`: tunnel TTL (overload).
        ifr_ttl, set_ifr_ttl => ifru_metric: i32;
        /// `ifr_df`: tunnel DF (overload).
        ifr_df, set_ifr_df => ifru_metric: i32;
        /// `ifr_data`: for use by interface.
        ifr_data, set_ifr_data => ifru_data: *mut u8;
        /// `ifr_index`: interface index.
        ifr_index, set_ifr_index => ifru_index: u32;
        /// `ifr_llprio`: link layer priority.
        ifr_llprio, set_ifr_llprio => ifru_metric: i32;
        /// `ifr_hdrprio`: header prio field config.
        ifr_hdrprio, set_ifr_hdrprio => ifru_metric: i32;
        /// `ifr_pwe3`: PWE3 type.
        ifr_pwe3, set_ifr_pwe3 => ifru_metric: i32;
    }
}

/// The unnamed union of `struct ifaliasreq`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IfraIfrau {
    /// The address.
    pub ifrau_addr: Sockaddr,
    /// Forces `int` alignment.
    pub ifrau_align: i32,
}

/// `struct ifaliasreq`: the argument of `SIOCAIFADDR`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ifaliasreq {
    /// If name, e.g. "en0".
    pub ifra_name: [u8; IFNAMSIZ],
    /// The address (`ifra_addr`).
    pub ifra_ifrau: IfraIfrau,
    /// Destination (or broadcast, `ifra_broadaddr`) address.
    pub ifra_dstaddr: Sockaddr,
    /// Netmask.
    pub ifra_mask: Sockaddr,
}

impl Ifaliasreq {
    /// `ifra_addr`.
    pub fn ifra_addr(&self) -> &Sockaddr {
        // SAFETY: both members are plain data, valid for any bit pattern.
        unsafe { &self.ifra_ifrau.ifrau_addr }
    }

    /// `ifra_addr`, writable.
    pub fn ifra_addr_mut(&mut self) -> &mut Sockaddr {
        // SAFETY: as above.
        unsafe { &mut self.ifra_ifrau.ifrau_addr }
    }

    /// `ifra_broadaddr`: the same field as `ifra_dstaddr`.
    pub fn ifra_broadaddr(&self) -> &Sockaddr {
        &self.ifra_dstaddr
    }
}

/// `struct ifmediareq`: the argument of `SIOCGIFMEDIA`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Ifmediareq {
    /// If name, e.g. "en0".
    pub ifm_name: [u8; IFNAMSIZ],
    /// Get/set current media options.
    pub ifm_current: u64,
    /// Don't care mask.
    pub ifm_mask: u64,
    /// Media status.
    pub ifm_status: u64,
    /// Active options.
    pub ifm_active: u64,
    /// # entries in `ifm_ulist` array.
    pub ifm_count: i32,
    /// Media words.
    pub ifm_ulist: *mut u64,
}

/// `struct ifkalivereq`: the argument of `SIOCSETKALIVE`/`SIOCGETKALIVE`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ifkalivereq {
    /// If name, e.g. "en0".
    pub ikar_name: [u8; IFNAMSIZ],
    /// Keepalive timeout.
    pub ikar_timeo: i32,
    /// Keepalive count.
    pub ikar_cnt: i32,
}

/// The unnamed union of `struct ifconf`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IfcIfcu {
    /// Buffer address.
    pub ifcu_buf: *mut u8,
    /// Array of structures returned.
    pub ifcu_req: *mut Ifreq,
}

/// `struct ifconf`: structure used in `SIOCGIFCONF` request. Used to retrieve interface
/// configuration for machine (useful for programs which must know all networks accessible).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ifconf {
    /// Size of associated buffer.
    pub ifc_len: i32,
    /// The buffer.
    pub ifc_ifcu: IfcIfcu,
}

impl Ifconf {
    /// `ifc_buf`: buffer address.
    pub fn ifc_buf(&self) -> *mut u8 {
        // SAFETY: both members are raw pointers, valid for any bit pattern.
        unsafe { self.ifc_ifcu.ifcu_buf }
    }

    /// `ifc_req`: array of structures returned.
    pub fn ifc_req(&self) -> *mut Ifreq {
        // SAFETY: both members are raw pointers, valid for any bit pattern.
        unsafe { self.ifc_ifcu.ifcu_req }
    }
}

/// `struct if_laddrreq`: structure for `SIOC[AGD]LIFADDR`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IfLaddrreq {
    /// Interface name.
    pub iflr_name: [u8; IFNAMSIZ],
    /// `IFLR_PREFIX`.
    pub flags: u32,
    /// In/out.
    pub prefixlen: u32,
    /// In/out.
    pub addr: SockaddrStorage,
    /// Out.
    pub dstaddr: SockaddrStorage,
}

/// `struct if_afreq`: `SIOCIFAFDETACH`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfAfreq {
    /// Interface name.
    pub ifar_name: [u8; IFNAMSIZ],
    /// Address family.
    pub ifar_af: SaFamily,
}

/// `struct if_parent`: `SIOC[SG]IFPARENT`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IfParent {
    /// Interface name.
    pub ifp_name: [u8; IFNAMSIZ],
    /// Parent interface name.
    pub ifp_parent: [u8; IFNAMSIZ],
}

/// `struct if_sffpage`: `SIOCGIFSFFPAGE`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IfSffpage {
    /// u -> k.
    pub sff_ifname: [u8; IFNAMSIZ],
    /// u -> k.
    pub sff_addr: u8,
    /// u -> k.
    pub sff_page: u8,
    /// k -> u.
    pub sff_data: [u8; IFSFF_DATA_LEN],
}

/// `LINK_STATE_DESCRIPTIONS`: how `ifconfig(8)` names a link state, per interface type (0
/// matches any type).
pub const LINK_STATE_DESCRIPTIONS: &[IfStatusDescription] = &[
    lsd(IFT_ETHER, LINK_STATE_DOWN, b"no carrier"),
    lsd(IFT_IEEE80211, LINK_STATE_DOWN, b"no network"),
    lsd(IFT_PPP, LINK_STATE_DOWN, b"no carrier"),
    lsd(IFT_CARP, LINK_STATE_DOWN, b"backup"),
    lsd(IFT_CARP, LINK_STATE_UP, b"master"),
    lsd(IFT_CARP, LINK_STATE_HALF_DUPLEX, b"master"),
    lsd(IFT_CARP, LINK_STATE_FULL_DUPLEX, b"master"),
    lsd(0, LINK_STATE_UP, b"active"),
    lsd(0, LINK_STATE_HALF_DUPLEX, b"active"),
    lsd(0, LINK_STATE_FULL_DUPLEX, b"active"),
    lsd(0, LINK_STATE_UNKNOWN, b"unknown"),
    lsd(0, LINK_STATE_INVALID, b"invalid"),
    lsd(0, LINK_STATE_DOWN, b"down"),
    lsd(0, LINK_STATE_KALIVE_DOWN, b"keepalive down"),
];

const fn lsd(ifs_type: u8, ifs_state: u8, ifs_string: &'static [u8]) -> IfStatusDescription {
    IfStatusDescription {
        ifs_type,
        ifs_state,
        ifs_string,
    }
}

/// `IFQ_PRIO2TOS(p)`: a queue priority as the precedence bits of an IP type of service.
pub const fn ifq_prio2tos(p: u32) -> u32 {
    p << 5
}

/// `IFQ_TOS2PRIO(t)`: the queue priority of an IP type of service.
pub const fn ifq_tos2prio(t: u32) -> u32 {
    t >> 5
}

/// `LINK_STATE_IS_UP(s)`: whether link state `s` counts as up (unknown does).
pub const fn link_state_is_up(s: u8) -> bool {
    s >= LINK_STATE_UP || s == LINK_STATE_UNKNOWN
}

/// `LINK_STATE_DESC_MATCH(ifs, t, s)`: whether description `ifs` applies to type `t` in state
/// `s`.
pub const fn link_state_desc_match(ifs: &IfStatusDescription, t: u8, s: u8) -> bool {
    (ifs.ifs_type == t || ifs.ifs_type == 0) && ifs.ifs_state == s
}

/// `IF_Kbps(x)`: kilobits/sec.
pub const fn if_kbps(x: u64) -> u64 {
    x * 1000
}

/// `IF_Mbps(x)`: megabits/sec.
pub const fn if_mbps(x: u64) -> u64 {
    if_kbps(x * 1000)
}

/// `IF_Gbps(x)`: gigabits/sec.
pub const fn if_gbps(x: u64) -> u64 {
    if_mbps(x * 1000)
}

// LP64 sizes of the C structures.
const _: () = {
    assert!(size_of::<IfNameindex>() == 16);
    assert!(size_of::<IfClonereq>() == 16);
    assert!(size_of::<IfRxring>() == 20);
    assert!(size_of::<IfRxringInfo>() == 40);
    assert!(size_of::<IfRxrinfo>() == 16);
    assert!(size_of::<IfData>() == 144);
    assert!(size_of::<IfMsghdr>() == 168);
    assert!(size_of::<IfaMsghdr>() == 24);
    assert!(size_of::<IfAnnouncemsghdr>() == 26);
    assert!(size_of::<IfIeee80211Data>() == 52);
    assert!(size_of::<IfIeee80211Msghdr>() == 64);
    assert!(size_of::<IfNameindexMsg>() == 20);
    assert!(size_of::<IfgReq>() == 16);
    assert!(size_of::<Ifgroupreq>() == 40);
    assert!(size_of::<Ifreq>() == 32);
    assert!(size_of::<Ifaliasreq>() == 64);
    assert!(size_of::<Ifmediareq>() == 64);
    assert!(size_of::<Ifkalivereq>() == 24);
    assert!(size_of::<Ifconf>() == 16);
    assert!(size_of::<IfLaddrreq>() == 536);
    assert!(size_of::<IfAfreq>() == 17);
    assert!(size_of::<IfParent>() == 32);
    assert!(size_of::<IfSffpage>() == 274);
};

#[cfg(test)]
mod tests;
