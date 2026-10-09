/*	$KAME: icmp6.h,v 1.84 2003/04/23 10:26:51 itojun Exp $	*/
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
 *	@(#)ip_icmp.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! Internet Control Message Protocol for IPv6: the message formats of ICMPv6, Multicast
//! Listener Discovery, Neighbor Discovery, router renumbering and node information, the
//! `ICMP6_FILTER` socket option and the statistics: `<netinet/icmp6.h>`.
//!
//! Upstream: sys/netinet/icmp6.h @ 3ce1f3f79392
//!
//! The structures are the messages as they are on the wire, multi-byte fields in network
//! order; packet data has no alignment guarantee, so they are copied in and out of mbufs
//! (`ptr::read_unaligned`), never referenced in place.
//!
//! ## Deviations
//! - `struct icmp6_hdr`'s `icmp6_dataun` union is the `#[repr(C)]` union
//!   [`Icmp6Dataun`]; the shorthands (`icmp6_data32`, `icmp6_pptr`, `icmp6_mtu`, `icmp6_id`,
//!   `icmp6_seq`, `icmp6_maxdelay`, and the per-message `mld_*`, `nd_rs_*`, `nd_ra_*`,
//!   `nd_ns_*`, `nd_na_*`, `nd_rd_*`, `ni_*`, `rr_*` macros that reach into the ICMPv6
//!   header) are accessor methods. Every member is plain data, so they are safe.
//! - The C structures are `__packed`; here they are `#[repr(C)]` without padding (sizes
//!   asserted below).
//! - The `ICMP6_FILTER_*` macros are methods of [`Icmp6Filter`] and functions of the same
//!   name in lower case.
//! - `enum icmp6stat_counters` is [`Icmp6statCounters`] with the C's values; the C indexes
//!   the histograms with arithmetic (`icp6s_outhist + type`), which [`icmp6stat_inc_hist`]
//!   does. `icmp6counters` (`struct cpumem *`) is the static array of atomics
//!   `ICMP6COUNTERS` in `netinet6/icmp6.rs` (`icmp6.c` defines the C's pointer).
//! - `ICMPV6CTL_NAMES` is a `Ctlname` table. The prototypes and `icmp6_redirtimeout` are
//!   defined by `netinet6/icmp6.rs` and `netinet6/in6_proto.rs`.

use core::mem::size_of;
use core::sync::atomic::Ordering;

use crate::net::route::RTF_PROTO1;
use crate::netinet6::icmp6::ICMP6COUNTERS;
use crate::netinet6::in6::In6Addr;
use crate::sys::endian::{htonl, htons};
use crate::sys::sysctl::{CTLTYPE_INT, Ctlname};

/// `IPV6_MMTU - sizeof(struct ip6_hdr) - sizeof(struct icmp6_hdr)`.
pub const ICMPV6_PLD_MAXLEN: usize = 1232;

/// Dest unreachable, codes:
pub const ICMP6_DST_UNREACH: u8 = 1;
/// Packet too big.
pub const ICMP6_PACKET_TOO_BIG: u8 = 2;
/// Time exceeded, code:
pub const ICMP6_TIME_EXCEEDED: u8 = 3;
/// ip6 header bad.
pub const ICMP6_PARAM_PROB: u8 = 4;

/// Echo service.
pub const ICMP6_ECHO_REQUEST: u8 = 128;
/// Echo reply.
pub const ICMP6_ECHO_REPLY: u8 = 129;
/// Multicast listener query.
pub const MLD_LISTENER_QUERY: u8 = 130;
/// Multicast listener report.
pub const MLD_LISTENER_REPORT: u8 = 131;
/// Multicast listener done.
pub const MLD_LISTENER_DONE: u8 = 132;

// RFC2292 decls

/// Group membership query.
pub const ICMP6_MEMBERSHIP_QUERY: u8 = 130;
/// Group membership report.
pub const ICMP6_MEMBERSHIP_REPORT: u8 = 131;
/// Group membership termination.
pub const ICMP6_MEMBERSHIP_REDUCTION: u8 = 132;

/// Router solicitation.
pub const ND_ROUTER_SOLICIT: u8 = 133;
/// Router advertisement.
pub const ND_ROUTER_ADVERT: u8 = 134;
/// Neighbor solicitation.
pub const ND_NEIGHBOR_SOLICIT: u8 = 135;
/// Neighbor advertisement.
pub const ND_NEIGHBOR_ADVERT: u8 = 136;
/// Redirect.
pub const ND_REDIRECT: u8 = 137;

/// Router renumbering.
pub const ICMP6_ROUTER_RENUMBERING: u8 = 138;

/// Who are you request.
pub const ICMP6_WRUREQUEST: u8 = 139;
/// Who are you reply.
pub const ICMP6_WRUREPLY: u8 = 140;
/// FQDN query.
pub const ICMP6_FQDN_QUERY: u8 = 139;
/// FQDN reply.
pub const ICMP6_FQDN_REPLY: u8 = 140;
/// Node information request.
pub const ICMP6_NI_QUERY: u8 = 139;
/// Node information reply.
pub const ICMP6_NI_REPLY: u8 = 140;
/// RFC3810 listener report.
pub const MLDV2_LISTENER_REPORT: u8 = 143;

// The definitions below are experimental. TBA

/// mtrace response (to sender).
pub const MLD_MTRACE_RESP: u8 = 200;
/// mtrace messages.
pub const MLD_MTRACE: u8 = 201;

/// `ICMP6_MAXTYPE`.
pub const ICMP6_MAXTYPE: u8 = 201;

/// No route to destination.
pub const ICMP6_DST_UNREACH_NOROUTE: u8 = 0;
/// Administratively prohibited.
pub const ICMP6_DST_UNREACH_ADMIN: u8 = 1;
/// Beyond scope of source address.
pub const ICMP6_DST_UNREACH_BEYONDSCOPE: u8 = 2;
/// Address unreachable.
pub const ICMP6_DST_UNREACH_ADDR: u8 = 3;
/// Port unreachable.
pub const ICMP6_DST_UNREACH_NOPORT: u8 = 4;

/// ttl==0 in transit.
pub const ICMP6_TIME_EXCEED_TRANSIT: u8 = 0;
/// ttl==0 in reass.
pub const ICMP6_TIME_EXCEED_REASSEMBLY: u8 = 1;

/// Erroneous header field.
pub const ICMP6_PARAMPROB_HEADER: u8 = 0;
/// Unrecognized next header.
pub const ICMP6_PARAMPROB_NEXTHEADER: u8 = 1;
/// Unrecognized option.
pub const ICMP6_PARAMPROB_OPTION: u8 = 2;

/// All informational messages.
pub const ICMP6_INFOMSG_MASK: u8 = 0x80;

/// Query Subject is an IPv6 address.
pub const ICMP6_NI_SUBJ_IPV6: u8 = 0;
/// Query Subject is a Domain name.
pub const ICMP6_NI_SUBJ_FQDN: u8 = 1;
/// Query Subject is an IPv4 address.
pub const ICMP6_NI_SUBJ_IPV4: u8 = 2;

/// Node information successful reply.
pub const ICMP6_NI_SUCCESS: u8 = 0;
/// Node information request is refused.
pub const ICMP6_NI_REFUSED: u8 = 1;
/// Unknown Qtype.
pub const ICMP6_NI_UNKNOWN: u8 = 2;

/// rr command.
pub const ICMP6_ROUTER_RENUMBERING_COMMAND: u8 = 0;
/// rr result.
pub const ICMP6_ROUTER_RENUMBERING_RESULT: u8 = 1;
/// rr seq num reset.
pub const ICMP6_ROUTER_RENUMBERING_SEQNUM_RESET: u8 = 255;

// Used in kernel only

/// Redirect to an on-link node.
pub const ND_REDIRECT_ONLINK: u8 = 0;
/// Redirect to a better router.
pub const ND_REDIRECT_ROUTER: u8 = 1;

/// `ND_RA_FLAG_MANAGED`.
pub const ND_RA_FLAG_MANAGED: u8 = 0x80;
/// `ND_RA_FLAG_OTHER`.
pub const ND_RA_FLAG_OTHER: u8 = 0x40;

/// 00011000.
pub const ND_RA_FLAG_RTPREF_MASK: u8 = 0x18;

/// 00001000.
pub const ND_RA_FLAG_RTPREF_HIGH: u8 = 0x08;
/// 00000000.
pub const ND_RA_FLAG_RTPREF_MEDIUM: u8 = 0x00;
/// 00011000.
pub const ND_RA_FLAG_RTPREF_LOW: u8 = 0x18;
/// 00010000.
pub const ND_RA_FLAG_RTPREF_RSV: u8 = 0x10;

/// `ND_NA_FLAG_ROUTER` (network order).
pub const ND_NA_FLAG_ROUTER: u32 = htonl(0x8000_0000);
/// `ND_NA_FLAG_SOLICITED` (network order).
pub const ND_NA_FLAG_SOLICITED: u32 = htonl(0x4000_0000);
/// `ND_NA_FLAG_OVERRIDE` (network order).
pub const ND_NA_FLAG_OVERRIDE: u32 = htonl(0x2000_0000);

/// `ND_OPT_SOURCE_LINKADDR`.
pub const ND_OPT_SOURCE_LINKADDR: u8 = 1;
/// `ND_OPT_TARGET_LINKADDR`.
pub const ND_OPT_TARGET_LINKADDR: u8 = 2;
/// `ND_OPT_PREFIX_INFORMATION`.
pub const ND_OPT_PREFIX_INFORMATION: u8 = 3;
/// `ND_OPT_REDIRECTED_HEADER`.
pub const ND_OPT_REDIRECTED_HEADER: u8 = 4;
/// `ND_OPT_MTU`.
pub const ND_OPT_MTU: u8 = 5;
/// `ND_OPT_ROUTE_INFO`.
pub const ND_OPT_ROUTE_INFO: u8 = 24;
/// `ND_OPT_RDNSS`.
pub const ND_OPT_RDNSS: u8 = 25;
/// `ND_OPT_DNSSL`.
pub const ND_OPT_DNSSL: u8 = 31;

/// `ND_OPT_PI_FLAG_ONLINK`.
pub const ND_OPT_PI_FLAG_ONLINK: u8 = 0x80;
/// `ND_OPT_PI_FLAG_AUTO`.
pub const ND_OPT_PI_FLAG_AUTO: u8 = 0x40;

/// NOOP.
pub const NI_QTYPE_NOOP: u16 = 0;
/// Supported Qtypes.
pub const NI_QTYPE_SUPTYPES: u16 = 1;
/// FQDN (draft 04).
pub const NI_QTYPE_FQDN: u16 = 2;
/// DNS Name.
pub const NI_QTYPE_DNSNAME: u16 = 2;
/// Node Addresses.
pub const NI_QTYPE_NODEADDR: u16 = 3;
/// IPv4 Addresses.
pub const NI_QTYPE_IPV4ADDR: u16 = 4;

/// `NI_SUPTYPE_FLAG_COMPRESS` (network order).
pub const NI_SUPTYPE_FLAG_COMPRESS: u16 = htons(0x0001);
/// `NI_FQDN_FLAG_VALIDTTL` (network order).
pub const NI_FQDN_FLAG_VALIDTTL: u16 = htons(0x0001);

/// `NI_NODEADDR_FLAG_TRUNCATE` (network order).
pub const NI_NODEADDR_FLAG_TRUNCATE: u16 = htons(0x0001);
/// `NI_NODEADDR_FLAG_ALL` (network order).
pub const NI_NODEADDR_FLAG_ALL: u16 = htons(0x0002);
/// `NI_NODEADDR_FLAG_COMPAT` (network order).
pub const NI_NODEADDR_FLAG_COMPAT: u16 = htons(0x0004);
/// `NI_NODEADDR_FLAG_LINKLOCAL` (network order).
pub const NI_NODEADDR_FLAG_LINKLOCAL: u16 = htons(0x0008);
/// `NI_NODEADDR_FLAG_SITELOCAL` (network order).
pub const NI_NODEADDR_FLAG_SITELOCAL: u16 = htons(0x0010);
/// `NI_NODEADDR_FLAG_GLOBAL` (network order).
pub const NI_NODEADDR_FLAG_GLOBAL: u16 = htons(0x0020);
/// `NI_NODEADDR_FLAG_ANYCAST` (network order; not in spec).
pub const NI_NODEADDR_FLAG_ANYCAST: u16 = htons(0x0040);

/// `ICMP6_RR_FLAGS_TEST`.
pub const ICMP6_RR_FLAGS_TEST: u8 = 0x80;
/// `ICMP6_RR_FLAGS_REQRESULT`.
pub const ICMP6_RR_FLAGS_REQRESULT: u8 = 0x40;
/// `ICMP6_RR_FLAGS_FORCEAPPLY`.
pub const ICMP6_RR_FLAGS_FORCEAPPLY: u8 = 0x20;
/// `ICMP6_RR_FLAGS_SPECSITE`.
pub const ICMP6_RR_FLAGS_SPECSITE: u8 = 0x10;
/// `ICMP6_RR_FLAGS_PREVDONE`.
pub const ICMP6_RR_FLAGS_PREVDONE: u8 = 0x08;

/// `RPM_PCO_ADD`.
pub const RPM_PCO_ADD: u8 = 1;
/// `RPM_PCO_CHANGE`.
pub const RPM_PCO_CHANGE: u8 = 2;
/// `RPM_PCO_SETGLOBAL`.
pub const RPM_PCO_SETGLOBAL: u8 = 3;
/// `RPM_PCO_MAX`.
pub const RPM_PCO_MAX: u8 = 4;

/// `ICMP6_RR_PCOUSE_RAFLAGS_ONLINK`.
pub const ICMP6_RR_PCOUSE_RAFLAGS_ONLINK: u8 = 0x80;
/// `ICMP6_RR_PCOUSE_RAFLAGS_AUTO`.
pub const ICMP6_RR_PCOUSE_RAFLAGS_AUTO: u8 = 0x40;

/// `ICMP6_RR_PCOUSE_FLAGS_DECRVLTIME` (network order).
pub const ICMP6_RR_PCOUSE_FLAGS_DECRVLTIME: u32 = htonl(0x8000_0000);
/// `ICMP6_RR_PCOUSE_FLAGS_DECRPLTIME` (network order).
pub const ICMP6_RR_PCOUSE_FLAGS_DECRPLTIME: u32 = htonl(0x4000_0000);

/// `ICMP6_RR_RESULT_FLAGS_OOB` (network order).
pub const ICMP6_RR_RESULT_FLAGS_OOB: u16 = htons(0x0002);
/// `ICMP6_RR_RESULT_FLAGS_FORBIDDEN` (network order).
pub const ICMP6_RR_RESULT_FLAGS_FORBIDDEN: u16 = htons(0x0001);

// Names for ICMP sysctl objects

/// `ICMPV6CTL_STATS`.
pub const ICMPV6CTL_STATS: i32 = 1;
/// Accept/process redirects.
pub const ICMPV6CTL_REDIRACCEPT: i32 = 2;
/// Redirect cache time.
pub const ICMPV6CTL_REDIRTIMEOUT: i32 = 3;
/// `ICMPV6CTL_ND6_DELAY`.
pub const ICMPV6CTL_ND6_DELAY: i32 = 8;
/// `ICMPV6CTL_ND6_UMAXTRIES`.
pub const ICMPV6CTL_ND6_UMAXTRIES: i32 = 9;
/// `ICMPV6CTL_ND6_MMAXTRIES`.
pub const ICMPV6CTL_ND6_MMAXTRIES: i32 = 10;
/// `ICMPV6CTL_ND6_QUEUED`.
pub const ICMPV6CTL_ND6_QUEUED: i32 = 11;
/// `ICMPV6CTL_NODEINFO`.
pub const ICMPV6CTL_NODEINFO: i32 = 13;
/// ICMPv6 error pps limitation.
pub const ICMPV6CTL_ERRPPSLIMIT: i32 = 14;
/// `ICMPV6CTL_MTUDISC_HIWAT`.
pub const ICMPV6CTL_MTUDISC_HIWAT: i32 = 16;
/// `ICMPV6CTL_MTUDISC_LOWAT`.
pub const ICMPV6CTL_MTUDISC_LOWAT: i32 = 17;
/// `ICMPV6CTL_MAXID`.
pub const ICMPV6CTL_MAXID: i32 = 18;

/// `ICMPV6CTL_NAMES`.
pub const ICMPV6CTL_NAMES: [Ctlname; ICMPV6CTL_MAXID as usize] = {
    let mut n = [Ctlname::NONE; ICMPV6CTL_MAXID as usize];
    n[3] = Ctlname::new(b"redirtimeout", CTLTYPE_INT);
    n[8] = Ctlname::new(b"nd6_delay", CTLTYPE_INT);
    n[9] = Ctlname::new(b"nd6_umaxtries", CTLTYPE_INT);
    n[10] = Ctlname::new(b"nd6_mmaxtries", CTLTYPE_INT);
    n[11] = Ctlname::new(b"nd6_queued", CTLTYPE_INT);
    n[14] = Ctlname::new(b"errppslimit", CTLTYPE_INT);
    n[16] = Ctlname::new(b"mtudisc_hiwat", CTLTYPE_INT);
    n[17] = Ctlname::new(b"mtudisc_lowat", CTLTYPE_INT);
    n
};

/// `RTF_PROBEMTU`.
pub const RTF_PROBEMTU: u32 = RTF_PROTO1;

/// The number of entries of each histogram.
const NHIST: usize = 256;

/// The `icmp6_dataun` union of `struct icmp6_hdr`: the type-specific field.
#[repr(C)]
#[derive(Clone, Copy)]
pub union Icmp6Dataun {
    /// `icmp6_un_data32`.
    pub icmp6_un_data32: [u32; 1],
    /// `icmp6_un_data16`.
    pub icmp6_un_data16: [u16; 2],
    /// `icmp6_un_data8`.
    pub icmp6_un_data8: [u8; 4],
}

/// `struct icmp6_hdr`: the ICMPv6 header.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Icmp6Hdr {
    /// Type field.
    pub icmp6_type: u8,
    /// Code field.
    pub icmp6_code: u8,
    /// Checksum field.
    pub icmp6_cksum: u16,
    /// Type-specific field.
    pub icmp6_dataun: Icmp6Dataun,
}

impl Icmp6Hdr {
    /// An all-zero header, usable in `const` contexts.
    pub const fn zeroed() -> Self {
        Self {
            icmp6_type: 0,
            icmp6_code: 0,
            icmp6_cksum: 0,
            icmp6_dataun: Icmp6Dataun {
                icmp6_un_data32: [0],
            },
        }
    }

    /// `icmp6_data32[0]`.
    pub fn icmp6_data32(&self) -> u32 {
        // SAFETY: every member of the union is plain data, valid for any bit pattern.
        unsafe { self.icmp6_dataun.icmp6_un_data32[0] }
    }

    /// `icmp6_data32[0] = v`.
    pub fn set_icmp6_data32(&mut self, v: u32) {
        self.icmp6_dataun.icmp6_un_data32 = [v];
    }

    /// `icmp6_data16[i]`.
    pub fn icmp6_data16(&self, i: usize) -> u16 {
        // SAFETY: as above.
        unsafe { self.icmp6_dataun.icmp6_un_data16[i] }
    }

    /// `icmp6_data16[i] = v`.
    pub fn set_icmp6_data16(&mut self, i: usize, v: u16) {
        // SAFETY: as above; writing one half-word leaves every member valid.
        unsafe { self.icmp6_dataun.icmp6_un_data16[i] = v };
    }

    /// `icmp6_data8[i]`.
    pub fn icmp6_data8(&self, i: usize) -> u8 {
        // SAFETY: as above.
        unsafe { self.icmp6_dataun.icmp6_un_data8[i] }
    }

    /// `icmp6_data8[i] = v`.
    pub fn set_icmp6_data8(&mut self, i: usize, v: u8) {
        // SAFETY: as above; writing one byte leaves every member valid.
        unsafe { self.icmp6_dataun.icmp6_un_data8[i] = v };
    }

    /// `icmp6_pptr`: parameter prob (`icmp6_data32[0]`).
    pub fn icmp6_pptr(&self) -> u32 {
        self.icmp6_data32()
    }

    /// `icmp6_pptr = v`.
    pub fn set_icmp6_pptr(&mut self, v: u32) {
        self.set_icmp6_data32(v);
    }

    /// `icmp6_mtu`: packet too big (`icmp6_data32[0]`).
    pub fn icmp6_mtu(&self) -> u32 {
        self.icmp6_data32()
    }

    /// `icmp6_mtu = v`.
    pub fn set_icmp6_mtu(&mut self, v: u32) {
        self.set_icmp6_data32(v);
    }

    /// `icmp6_id`: echo request/reply (`icmp6_data16[0]`).
    pub fn icmp6_id(&self) -> u16 {
        self.icmp6_data16(0)
    }

    /// `icmp6_id = v`.
    pub fn set_icmp6_id(&mut self, v: u16) {
        self.set_icmp6_data16(0, v);
    }

    /// `icmp6_seq`: echo request/reply (`icmp6_data16[1]`).
    pub fn icmp6_seq(&self) -> u16 {
        self.icmp6_data16(1)
    }

    /// `icmp6_seq = v`.
    pub fn set_icmp6_seq(&mut self, v: u16) {
        self.set_icmp6_data16(1, v);
    }

    /// `icmp6_maxdelay`: mcast group membership (`icmp6_data16[0]`).
    pub fn icmp6_maxdelay(&self) -> u16 {
        self.icmp6_data16(0)
    }

    /// `icmp6_maxdelay = v`.
    pub fn set_icmp6_maxdelay(&mut self, v: u16) {
        self.set_icmp6_data16(0, v);
    }
}

/// Defines accessor pairs for the C's shorthand macros that reach into an embedded
/// `struct icmp6_hdr` (`$hdr`): the type, code and checksum members, and the views of the
/// type-specific field.
macro_rules! icmp6_hdr_shorthands {
    ($hdr:ident; type $ty:ident, $sty:ident; code $co:ident, $sco:ident;
     cksum $ck:ident, $sck:ident; $($rest:tt)*) => {
        /// The `icmp6_type` of the embedded header.
        pub fn $ty(&self) -> u8 {
            self.$hdr.icmp6_type
        }

        /// Sets the `icmp6_type` of the embedded header.
        pub fn $sty(&mut self, v: u8) {
            self.$hdr.icmp6_type = v;
        }

        /// The `icmp6_code` of the embedded header.
        pub fn $co(&self) -> u8 {
            self.$hdr.icmp6_code
        }

        /// Sets the `icmp6_code` of the embedded header.
        pub fn $sco(&mut self, v: u8) {
            self.$hdr.icmp6_code = v;
        }

        /// The `icmp6_cksum` of the embedded header.
        pub fn $ck(&self) -> u16 {
            self.$hdr.icmp6_cksum
        }

        /// Sets the `icmp6_cksum` of the embedded header.
        pub fn $sck(&mut self, v: u16) {
            self.$hdr.icmp6_cksum = v;
        }

        icmp6_hdr_shorthands!(@views $hdr; $($rest)*);
    };
    (@views $hdr:ident;) => {};
    (@views $hdr:ident; data32 $get:ident, $set:ident; $($rest:tt)*) => {
        /// `icmp6_data32[0]` of the embedded header.
        pub fn $get(&self) -> u32 {
            self.$hdr.icmp6_data32()
        }

        /// Sets `icmp6_data32[0]` of the embedded header.
        pub fn $set(&mut self, v: u32) {
            self.$hdr.set_icmp6_data32(v);
        }

        icmp6_hdr_shorthands!(@views $hdr; $($rest)*);
    };
    (@views $hdr:ident; data16 [$i:expr] $get:ident, $set:ident; $($rest:tt)*) => {
        /// An `icmp6_data16` half-word of the embedded header.
        pub fn $get(&self) -> u16 {
            self.$hdr.icmp6_data16($i)
        }

        /// Sets an `icmp6_data16` half-word of the embedded header.
        pub fn $set(&mut self, v: u16) {
            self.$hdr.set_icmp6_data16($i, v);
        }

        icmp6_hdr_shorthands!(@views $hdr; $($rest)*);
    };
    (@views $hdr:ident; data8 [$i:expr] $get:ident, $set:ident; $($rest:tt)*) => {
        /// An `icmp6_data8` byte of the embedded header.
        pub fn $get(&self) -> u8 {
            self.$hdr.icmp6_data8($i)
        }

        /// Sets an `icmp6_data8` byte of the embedded header.
        pub fn $set(&mut self, v: u8) {
            self.$hdr.set_icmp6_data8($i, v);
        }

        icmp6_hdr_shorthands!(@views $hdr; $($rest)*);
    };
}

/// `struct mld_hdr`: Multicast Listener Discovery.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MldHdr {
    /// `mld_icmp6_hdr`.
    pub mld_icmp6_hdr: Icmp6Hdr,
    /// Multicast address.
    pub mld_addr: In6Addr,
}

impl MldHdr {
    icmp6_hdr_shorthands! {
        mld_icmp6_hdr;
        type mld_type, set_mld_type;
        code mld_code, set_mld_code;
        cksum mld_cksum, set_mld_cksum;
        data16 [0] mld_maxdelay, set_mld_maxdelay;
        data16 [1] mld_reserved, set_mld_reserved;
    }
}

/// `struct nd_router_solicit`: router solicitation (could be followed by options).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NdRouterSolicit {
    /// `nd_rs_hdr`.
    pub nd_rs_hdr: Icmp6Hdr,
}

impl NdRouterSolicit {
    icmp6_hdr_shorthands! {
        nd_rs_hdr;
        type nd_rs_type, set_nd_rs_type;
        code nd_rs_code, set_nd_rs_code;
        cksum nd_rs_cksum, set_nd_rs_cksum;
        data32 nd_rs_reserved, set_nd_rs_reserved;
    }
}

/// `struct nd_router_advert`: router advertisement (could be followed by options).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NdRouterAdvert {
    /// `nd_ra_hdr`.
    pub nd_ra_hdr: Icmp6Hdr,
    /// Reachable time.
    pub nd_ra_reachable: u32,
    /// Retransmit timer.
    pub nd_ra_retransmit: u32,
}

impl NdRouterAdvert {
    icmp6_hdr_shorthands! {
        nd_ra_hdr;
        type nd_ra_type, set_nd_ra_type;
        code nd_ra_code, set_nd_ra_code;
        cksum nd_ra_cksum, set_nd_ra_cksum;
        data8 [0] nd_ra_curhoplimit, set_nd_ra_curhoplimit;
        data8 [1] nd_ra_flags_reserved, set_nd_ra_flags_reserved;
        data16 [1] nd_ra_router_lifetime, set_nd_ra_router_lifetime;
    }
}

/// `struct nd_neighbor_solicit`: neighbor solicitation (could be followed by options).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NdNeighborSolicit {
    /// `nd_ns_hdr`.
    pub nd_ns_hdr: Icmp6Hdr,
    /// Target address.
    pub nd_ns_target: In6Addr,
}

impl NdNeighborSolicit {
    icmp6_hdr_shorthands! {
        nd_ns_hdr;
        type nd_ns_type, set_nd_ns_type;
        code nd_ns_code, set_nd_ns_code;
        cksum nd_ns_cksum, set_nd_ns_cksum;
        data32 nd_ns_reserved, set_nd_ns_reserved;
    }
}

/// `struct nd_neighbor_advert`: neighbor advertisement (could be followed by options).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NdNeighborAdvert {
    /// `nd_na_hdr`.
    pub nd_na_hdr: Icmp6Hdr,
    /// Target address.
    pub nd_na_target: In6Addr,
}

impl NdNeighborAdvert {
    icmp6_hdr_shorthands! {
        nd_na_hdr;
        type nd_na_type, set_nd_na_type;
        code nd_na_code, set_nd_na_code;
        cksum nd_na_cksum, set_nd_na_cksum;
        data32 nd_na_flags_reserved, set_nd_na_flags_reserved;
    }
}

/// `struct nd_redirect`: redirect (could be followed by options).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NdRedirect {
    /// `nd_rd_hdr`.
    pub nd_rd_hdr: Icmp6Hdr,
    /// Target address.
    pub nd_rd_target: In6Addr,
    /// Destination address.
    pub nd_rd_dst: In6Addr,
}

impl NdRedirect {
    icmp6_hdr_shorthands! {
        nd_rd_hdr;
        type nd_rd_type, set_nd_rd_type;
        code nd_rd_code, set_nd_rd_code;
        cksum nd_rd_cksum, set_nd_rd_cksum;
        data32 nd_rd_reserved, set_nd_rd_reserved;
    }
}

/// `struct nd_opt_hdr`: Neighbor discovery option header (followed by option specific
/// data).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NdOptHdr {
    /// `nd_opt_type`.
    pub nd_opt_type: u8,
    /// `nd_opt_len`: in units of 8 octets.
    pub nd_opt_len: u8,
}

/// `struct nd_opt_prefix_info`: prefix information.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NdOptPrefixInfo {
    /// `nd_opt_pi_type`.
    pub nd_opt_pi_type: u8,
    /// `nd_opt_pi_len`.
    pub nd_opt_pi_len: u8,
    /// `nd_opt_pi_prefix_len`.
    pub nd_opt_pi_prefix_len: u8,
    /// `nd_opt_pi_flags_reserved`.
    pub nd_opt_pi_flags_reserved: u8,
    /// `nd_opt_pi_valid_time`.
    pub nd_opt_pi_valid_time: u32,
    /// `nd_opt_pi_preferred_time`.
    pub nd_opt_pi_preferred_time: u32,
    /// `nd_opt_pi_reserved2`.
    pub nd_opt_pi_reserved2: u32,
    /// `nd_opt_pi_prefix`.
    pub nd_opt_pi_prefix: In6Addr,
}

/// `struct nd_opt_rd_hdr`: redirected header (followed by IP header and data).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NdOptRdHdr {
    /// `nd_opt_rh_type`.
    pub nd_opt_rh_type: u8,
    /// `nd_opt_rh_len`.
    pub nd_opt_rh_len: u8,
    /// `nd_opt_rh_reserved1`.
    pub nd_opt_rh_reserved1: u16,
    /// `nd_opt_rh_reserved2`.
    pub nd_opt_rh_reserved2: u32,
}

/// `struct nd_opt_mtu`: MTU option.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NdOptMtu {
    /// `nd_opt_mtu_type`.
    pub nd_opt_mtu_type: u8,
    /// `nd_opt_mtu_len`.
    pub nd_opt_mtu_len: u8,
    /// `nd_opt_mtu_reserved`.
    pub nd_opt_mtu_reserved: u16,
    /// `nd_opt_mtu_mtu`.
    pub nd_opt_mtu_mtu: u32,
}

/// `struct nd_opt_route_info`: route info.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NdOptRouteInfo {
    /// `nd_opt_rti_type`.
    pub nd_opt_rti_type: u8,
    /// `nd_opt_rti_len`.
    pub nd_opt_rti_len: u8,
    /// `nd_opt_rti_prefixlen`.
    pub nd_opt_rti_prefixlen: u8,
    /// `nd_opt_rti_flags`.
    pub nd_opt_rti_flags: u8,
    /// `nd_opt_rti_lifetime`.
    pub nd_opt_rti_lifetime: u32,
}

/// `struct nd_opt_rdnss`: RDNSS option (followed by list of recursive DNS servers).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NdOptRdnss {
    /// `nd_opt_rdnss_type`.
    pub nd_opt_rdnss_type: u8,
    /// `nd_opt_rdnss_len`.
    pub nd_opt_rdnss_len: u8,
    /// `nd_opt_rdnss_reserved`.
    pub nd_opt_rdnss_reserved: u16,
    /// `nd_opt_rdnss_lifetime`.
    pub nd_opt_rdnss_lifetime: u32,
}

/// `struct nd_opt_dnssl`: DNSSL option (followed by list of DNS search domains).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NdOptDnssl {
    /// `nd_opt_dnssl_type`.
    pub nd_opt_dnssl_type: u8,
    /// `nd_opt_dnssl_len`.
    pub nd_opt_dnssl_len: u8,
    /// `nd_opt_dnssl_reserved`.
    pub nd_opt_dnssl_reserved: u16,
    /// `nd_opt_dnssl_lifetime`.
    pub nd_opt_dnssl_lifetime: u32,
}

/// `struct icmp6_namelookup`: icmp6 namelookup (could be followed by options).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Icmp6Namelookup {
    /// `icmp6_nl_hdr`.
    pub icmp6_nl_hdr: Icmp6Hdr,
    /// `icmp6_nl_nonce`.
    pub icmp6_nl_nonce: [u8; 8],
    /// `icmp6_nl_ttl`.
    pub icmp6_nl_ttl: i32,
}

/// `struct icmp6_nodeinfo`: icmp6 node information (could be followed by reply data).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Icmp6Nodeinfo {
    /// `icmp6_ni_hdr`.
    pub icmp6_ni_hdr: Icmp6Hdr,
    /// `icmp6_ni_nonce`.
    pub icmp6_ni_nonce: [u8; 8],
}

impl Icmp6Nodeinfo {
    icmp6_hdr_shorthands! {
        icmp6_ni_hdr;
        type ni_type, set_ni_type;
        code ni_code, set_ni_code;
        cksum ni_cksum, set_ni_cksum;
        data16 [0] ni_qtype, set_ni_qtype;
        data16 [1] ni_flags, set_ni_flags;
    }
}

/// `struct ni_reply_fqdn`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NiReplyFqdn {
    /// TTL.
    pub ni_fqdn_ttl: u32,
    /// Length in octets of the FQDN.
    pub ni_fqdn_namelen: u8,
    /// XXX: alignment.
    pub ni_fqdn_name: [u8; 3],
}

/// `struct icmp6_router_renum`: router renumbering header (router-renum-08.txt).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Icmp6RouterRenum {
    /// `rr_hdr`.
    pub rr_hdr: Icmp6Hdr,
    /// `rr_segnum`.
    pub rr_segnum: u8,
    /// `rr_flags`.
    pub rr_flags: u8,
    /// `rr_maxdelay`.
    pub rr_maxdelay: u16,
    /// `rr_reserved`.
    pub rr_reserved: u32,
}

impl Icmp6RouterRenum {
    icmp6_hdr_shorthands! {
        rr_hdr;
        type rr_type, set_rr_type;
        code rr_code, set_rr_code;
        cksum rr_cksum, set_rr_cksum;
        data32 rr_seqnum, set_rr_seqnum;
    }
}

/// `struct rr_pco_match`: match prefix part.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RrPcoMatch {
    /// `rpm_code`.
    pub rpm_code: u8,
    /// `rpm_len`.
    pub rpm_len: u8,
    /// `rpm_ordinal`.
    pub rpm_ordinal: u8,
    /// `rpm_matchlen`.
    pub rpm_matchlen: u8,
    /// `rpm_minlen`.
    pub rpm_minlen: u8,
    /// `rpm_maxlen`.
    pub rpm_maxlen: u8,
    /// `rpm_reserved`.
    pub rpm_reserved: u16,
    /// `rpm_prefix`.
    pub rpm_prefix: In6Addr,
}

/// `struct rr_pco_use`: use prefix part.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RrPcoUse {
    /// `rpu_uselen`.
    pub rpu_uselen: u8,
    /// `rpu_keeplen`.
    pub rpu_keeplen: u8,
    /// `rpu_ramask`.
    pub rpu_ramask: u8,
    /// `rpu_raflags`.
    pub rpu_raflags: u8,
    /// `rpu_vltime`.
    pub rpu_vltime: u32,
    /// `rpu_pltime`.
    pub rpu_pltime: u32,
    /// `rpu_flags`.
    pub rpu_flags: u32,
    /// `rpu_prefix`.
    pub rpu_prefix: In6Addr,
}

/// `struct rr_result`: router renumbering result message.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RrResult {
    /// `rrr_flags`.
    pub rrr_flags: u16,
    /// `rrr_ordinal`.
    pub rrr_ordinal: u8,
    /// `rrr_matchedlen`.
    pub rrr_matchedlen: u8,
    /// `rrr_ifid`.
    pub rrr_ifid: u32,
    /// `rrr_prefix`.
    pub rrr_prefix: In6Addr,
}

/// `struct icmp6_filter`: which ICMPv6 types a raw socket receives (a bit set by type).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Icmp6Filter {
    /// `icmp6_filt`.
    pub icmp6_filt: [u32; 8],
}

impl Icmp6Filter {
    /// `ICMP6_FILTER_SETPASSALL(filterp)`.
    pub fn setpassall(&mut self) {
        self.icmp6_filt = [0xffff_ffff; 8];
    }

    /// `ICMP6_FILTER_SETBLOCKALL(filterp)`.
    pub fn setblockall(&mut self) {
        self.icmp6_filt = [0; 8];
    }

    /// `ICMP6_FILTER_SETPASS(type, filterp)`.
    pub fn setpass(&mut self, type_: u8) {
        self.icmp6_filt[usize::from(type_ >> 5)] |= 1 << (type_ & 31);
    }

    /// `ICMP6_FILTER_SETBLOCK(type, filterp)`.
    pub fn setblock(&mut self, type_: u8) {
        self.icmp6_filt[usize::from(type_ >> 5)] &= !(1 << (type_ & 31));
    }

    /// `ICMP6_FILTER_WILLPASS(type, filterp)`.
    pub fn willpass(&self, type_: u8) -> bool {
        self.icmp6_filt[usize::from(type_ >> 5)] & (1 << (type_ & 31)) != 0
    }

    /// `ICMP6_FILTER_WILLBLOCK(type, filterp)`.
    pub fn willblock(&self, type_: u8) -> bool {
        self.icmp6_filt[usize::from(type_ >> 5)] & (1 << (type_ & 31)) == 0
    }
}

/// `struct icmp6stat`: variables related to this implementation of the internet control
/// message protocol version 6, as `sysctl(2)` returns them.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Icmp6stat {
    // statistics related to icmp6 packets generated
    /// # of calls to `icmp6_error`.
    pub icp6s_error: u64,
    /// No error because old was icmp.
    pub icp6s_canterror: u64,
    /// No error because rate limitation.
    pub icp6s_toofreq: u64,
    /// Messages sent, by type.
    pub icp6s_outhist: [u64; NHIST],
    // statistics related to input message processed
    /// `icmp6_code` out of range.
    pub icp6s_badcode: u64,
    /// Packet < sizeof(struct icmp6_hdr).
    pub icp6s_tooshort: u64,
    /// Bad checksum.
    pub icp6s_checksum: u64,
    /// Calculated bound mismatch.
    pub icp6s_badlen: u64,
    /// Number of responses: inherited from netinet code, but for netinet6 code it is
    /// already available in `icp6s_outhist[]`.
    pub icp6s_reflect: u64,
    /// Messages received, by type.
    pub icp6s_inhist: [u64; NHIST],
    /// Too many ND options.
    pub icp6s_nd_toomanyopt: u64,
    /// `icp6s_odst_unreach_noroute`.
    pub icp6s_odst_unreach_noroute: u64,
    /// `icp6s_odst_unreach_admin`.
    pub icp6s_odst_unreach_admin: u64,
    /// `icp6s_odst_unreach_beyondscope`.
    pub icp6s_odst_unreach_beyondscope: u64,
    /// `icp6s_odst_unreach_addr`.
    pub icp6s_odst_unreach_addr: u64,
    /// `icp6s_odst_unreach_noport`.
    pub icp6s_odst_unreach_noport: u64,
    /// `icp6s_opacket_too_big`.
    pub icp6s_opacket_too_big: u64,
    /// `icp6s_otime_exceed_transit`.
    pub icp6s_otime_exceed_transit: u64,
    /// `icp6s_otime_exceed_reassembly`.
    pub icp6s_otime_exceed_reassembly: u64,
    /// `icp6s_oparamprob_header`.
    pub icp6s_oparamprob_header: u64,
    /// `icp6s_oparamprob_nextheader`.
    pub icp6s_oparamprob_nextheader: u64,
    /// `icp6s_oparamprob_option`.
    pub icp6s_oparamprob_option: u64,
    /// We regard redirect as an error here.
    pub icp6s_oredirect: u64,
    /// `icp6s_ounknown`.
    pub icp6s_ounknown: u64,
    /// Path MTU changes.
    pub icp6s_pmtuchg: u64,
    /// Bad ND options.
    pub icp6s_nd_badopt: u64,
    /// Bad neighbor solicitation.
    pub icp6s_badns: u64,
    /// Bad neighbor advertisement.
    pub icp6s_badna: u64,
    /// Bad router advertisement.
    pub icp6s_badrs: u64,
    /// Bad router advertisement.
    pub icp6s_badra: u64,
    /// Bad redirect message.
    pub icp6s_badredirect: u64,
}

/// `enum icmp6stat_counters`: the per-CPU ICMPv6 counters, one per word of [`Icmp6stat`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Icmp6statCounters {
    /// `icp6s_error`.
    Icp6sError,
    /// `icp6s_canterror`.
    Icp6sCanterror,
    /// `icp6s_toofreq`.
    Icp6sToofreq,
    /// `icp6s_outhist`: the first of 256.
    Icp6sOuthist,
    /// `icp6s_badcode`.
    Icp6sBadcode = 3 + NHIST,
    /// `icp6s_tooshort`.
    Icp6sTooshort,
    /// `icp6s_checksum`.
    Icp6sChecksum,
    /// `icp6s_badlen`.
    Icp6sBadlen,
    /// `icp6s_reflect`.
    Icp6sReflect,
    /// `icp6s_inhist`: the first of 256.
    Icp6sInhist,
    /// `icp6s_nd_toomanyopt`.
    Icp6sNdToomanyopt = 3 + NHIST + 5 + NHIST,
    /// `icp6s_odst_unreach_noroute`.
    Icp6sOdstUnreachNoroute,
    /// `icp6s_odst_unreach_admin`.
    Icp6sOdstUnreachAdmin,
    /// `icp6s_odst_unreach_beyondscope`.
    Icp6sOdstUnreachBeyondscope,
    /// `icp6s_odst_unreach_addr`.
    Icp6sOdstUnreachAddr,
    /// `icp6s_odst_unreach_noport`.
    Icp6sOdstUnreachNoport,
    /// `icp6s_opacket_too_big`.
    Icp6sOpacketTooBig,
    /// `icp6s_otime_exceed_transit`.
    Icp6sOtimeExceedTransit,
    /// `icp6s_otime_exceed_reassembly`.
    Icp6sOtimeExceedReassembly,
    /// `icp6s_oparamprob_header`.
    Icp6sOparamprobHeader,
    /// `icp6s_oparamprob_nextheader`.
    Icp6sOparamprobNextheader,
    /// `icp6s_oparamprob_option`.
    Icp6sOparamprobOption,
    /// `icp6s_oredirect`.
    Icp6sOredirect,
    /// `icp6s_ounknown`.
    Icp6sOunknown,
    /// `icp6s_pmtuchg`.
    Icp6sPmtuchg,
    /// `icp6s_nd_badopt`.
    Icp6sNdBadopt,
    /// `icp6s_badns`.
    Icp6sBadns,
    /// `icp6s_badna`.
    Icp6sBadna,
    /// `icp6s_badrs`.
    Icp6sBadrs,
    /// `icp6s_badra`.
    Icp6sBadra,
    /// `icp6s_badredirect`.
    Icp6sBadredirect,
    /// `icp6s_ncounters`.
    Icp6sNcounters,
}

/// `icp6s_ncounters`.
pub const ICP6S_NCOUNTERS: usize = Icmp6statCounters::Icp6sNcounters as usize;

/// `ICMP6_FILTER_SETPASSALL(filterp)`.
pub fn icmp6_filter_setpassall(filterp: &mut Icmp6Filter) {
    filterp.setpassall();
}

/// `ICMP6_FILTER_SETBLOCKALL(filterp)`.
pub fn icmp6_filter_setblockall(filterp: &mut Icmp6Filter) {
    filterp.setblockall();
}

/// `ICMP6_FILTER_SETPASS(type, filterp)`.
pub fn icmp6_filter_setpass(type_: u8, filterp: &mut Icmp6Filter) {
    filterp.setpass(type_);
}

/// `ICMP6_FILTER_SETBLOCK(type, filterp)`.
pub fn icmp6_filter_setblock(type_: u8, filterp: &mut Icmp6Filter) {
    filterp.setblock(type_);
}

/// `ICMP6_FILTER_WILLPASS(type, filterp)`.
pub fn icmp6_filter_willpass(type_: u8, filterp: &Icmp6Filter) -> bool {
    filterp.willpass(type_)
}

/// `ICMP6_FILTER_WILLBLOCK(type, filterp)`.
pub fn icmp6_filter_willblock(type_: u8, filterp: &Icmp6Filter) -> bool {
    filterp.willblock(type_)
}

/// `icmp6stat_inc(c)`.
pub fn icmp6stat_inc(c: Icmp6statCounters) {
    ICMP6COUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `icmp6stat_inc(icp6s_outhist + type)` / `icmp6stat_inc(icp6s_inhist + type)`: bumps entry
/// `type_` of histogram `hist`.
pub fn icmp6stat_inc_hist(hist: Icmp6statCounters, type_: u8) {
    ICMP6COUNTERS[hist as usize + usize::from(type_)].fetch_add(1, Ordering::Relaxed);
}

// The sizes of the C's `__packed` structures, and the counters are the statistics' words.
const _: () = {
    assert!(size_of::<Icmp6Hdr>() == 8);
    assert!(size_of::<MldHdr>() == 24);
    assert!(size_of::<NdRouterSolicit>() == 8);
    assert!(size_of::<NdRouterAdvert>() == 16);
    assert!(size_of::<NdNeighborSolicit>() == 24);
    assert!(size_of::<NdNeighborAdvert>() == 24);
    assert!(size_of::<NdRedirect>() == 40);
    assert!(size_of::<NdOptHdr>() == 2);
    assert!(size_of::<NdOptPrefixInfo>() == 32);
    assert!(size_of::<NdOptRdHdr>() == 8);
    assert!(size_of::<NdOptMtu>() == 8);
    assert!(size_of::<NdOptRouteInfo>() == 8);
    assert!(size_of::<NdOptRdnss>() == 8);
    assert!(size_of::<NdOptDnssl>() == 8);
    assert!(size_of::<Icmp6Namelookup>() == 20);
    assert!(size_of::<Icmp6Nodeinfo>() == 16);
    assert!(size_of::<NiReplyFqdn>() == 8);
    assert!(size_of::<Icmp6RouterRenum>() == 16);
    assert!(size_of::<RrPcoMatch>() == 24);
    assert!(size_of::<RrPcoUse>() == 32);
    assert!(size_of::<RrResult>() == 24);
    assert!(size_of::<Icmp6Filter>() == 32);
    assert!(size_of::<Icmp6stat>() == ICP6S_NCOUNTERS * size_of::<u64>());
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use core::mem::offset_of;

    use super::*;
    use crate::reftest::{assert_complete, assert_defines};

    #[test]
    fn filter_macros() {
        let mut f = Icmp6Filter::default();
        icmp6_filter_setblockall(&mut f);
        assert!(icmp6_filter_willblock(ICMP6_ECHO_REQUEST, &f));
        icmp6_filter_setpass(ICMP6_ECHO_REQUEST, &mut f);
        assert!(icmp6_filter_willpass(ICMP6_ECHO_REQUEST, &f));
        assert!(icmp6_filter_willblock(ICMP6_ECHO_REPLY, &f));
        // 128 is bit 0 of word 4.
        assert_eq!(f.icmp6_filt[4], 1);
        icmp6_filter_setpassall(&mut f);
        assert!(icmp6_filter_willpass(ICMP6_MAXTYPE, &f) && icmp6_filter_willpass(255, &f));
        icmp6_filter_setblock(255, &mut f);
        assert!(icmp6_filter_willblock(255, &f) && icmp6_filter_willpass(254, &f));
        assert_eq!(f.icmp6_filt[7], 0x7fff_ffff);
    }

    #[test]
    fn header_views() {
        let mut h = Icmp6Hdr::zeroed();
        h.set_icmp6_id(htons(0x1234));
        h.set_icmp6_seq(htons(7));
        assert_eq!(h.icmp6_data8(0), 0x12);
        assert_eq!(h.icmp6_data8(3), 7);
        assert_eq!(h.icmp6_maxdelay(), h.icmp6_id());
        h.set_icmp6_mtu(htonl(1280));
        assert_eq!(h.icmp6_pptr(), htonl(1280));
        let mut ra = NdRouterAdvert {
            nd_ra_hdr: Icmp6Hdr::zeroed(),
            nd_ra_reachable: 0,
            nd_ra_retransmit: 0,
        };
        ra.set_nd_ra_curhoplimit(64);
        ra.set_nd_ra_flags_reserved(ND_RA_FLAG_MANAGED);
        ra.set_nd_ra_router_lifetime(htons(1800));
        assert_eq!(ra.nd_ra_hdr.icmp6_data8(0), 64);
        assert_eq!(ra.nd_ra_hdr.icmp6_data16(1), htons(1800));
        assert_eq!(offset_of!(NdNeighborSolicit, nd_ns_target), 8);
        assert_eq!(offset_of!(NdRedirect, nd_rd_dst), 24);
        assert_eq!(offset_of!(NdOptPrefixInfo, nd_opt_pi_prefix), 16);
    }

    #[test]
    fn counters_match_the_structure() {
        let w = |off: usize| off / size_of::<u64>();
        assert_eq!(
            w(offset_of!(Icmp6stat, icp6s_outhist)),
            Icmp6statCounters::Icp6sOuthist as usize
        );
        assert_eq!(
            w(offset_of!(Icmp6stat, icp6s_badcode)),
            Icmp6statCounters::Icp6sBadcode as usize
        );
        assert_eq!(
            w(offset_of!(Icmp6stat, icp6s_inhist)),
            Icmp6statCounters::Icp6sInhist as usize
        );
        assert_eq!(
            w(offset_of!(Icmp6stat, icp6s_nd_toomanyopt)),
            Icmp6statCounters::Icp6sNdToomanyopt as usize
        );
        assert_eq!(
            w(offset_of!(Icmp6stat, icp6s_badredirect)),
            Icmp6statCounters::Icp6sBadredirect as usize
        );
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet/icmp6.h");
        assert_defines!(defs; ICMPV6_PLD_MAXLEN);
        let icmp6 = assert_defines!(defs;
        ICMP6_DST_UNREACH, ICMP6_PACKET_TOO_BIG, ICMP6_TIME_EXCEEDED, ICMP6_PARAM_PROB,
        ICMP6_ECHO_REQUEST, ICMP6_ECHO_REPLY, ICMP6_MEMBERSHIP_QUERY, ICMP6_MEMBERSHIP_REPORT,
        ICMP6_MEMBERSHIP_REDUCTION, ICMP6_ROUTER_RENUMBERING, ICMP6_WRUREQUEST, ICMP6_WRUREPLY,
        ICMP6_FQDN_QUERY, ICMP6_FQDN_REPLY, ICMP6_NI_QUERY, ICMP6_NI_REPLY, ICMP6_MAXTYPE,
        ICMP6_DST_UNREACH_NOROUTE, ICMP6_DST_UNREACH_ADMIN, ICMP6_DST_UNREACH_BEYONDSCOPE,
        ICMP6_DST_UNREACH_ADDR, ICMP6_DST_UNREACH_NOPORT, ICMP6_TIME_EXCEED_TRANSIT,
        ICMP6_TIME_EXCEED_REASSEMBLY, ICMP6_PARAMPROB_HEADER, ICMP6_PARAMPROB_NEXTHEADER,
        ICMP6_PARAMPROB_OPTION, ICMP6_INFOMSG_MASK, ICMP6_NI_SUBJ_IPV6, ICMP6_NI_SUBJ_FQDN,
        ICMP6_NI_SUBJ_IPV4, ICMP6_NI_SUCCESS, ICMP6_NI_REFUSED, ICMP6_NI_UNKNOWN,
        ICMP6_ROUTER_RENUMBERING_COMMAND, ICMP6_ROUTER_RENUMBERING_RESULT,
        ICMP6_ROUTER_RENUMBERING_SEQNUM_RESET, ICMP6_RR_FLAGS_TEST, ICMP6_RR_FLAGS_REQRESULT,
        ICMP6_RR_FLAGS_FORCEAPPLY, ICMP6_RR_FLAGS_SPECSITE, ICMP6_RR_FLAGS_PREVDONE,
        ICMP6_RR_PCOUSE_RAFLAGS_ONLINK, ICMP6_RR_PCOUSE_RAFLAGS_AUTO);
        // The htonl()/htons() values are checked by value below.
        let swapped = [
            "ICMP6_RR_PCOUSE_FLAGS_DECRVLTIME",
            "ICMP6_RR_PCOUSE_FLAGS_DECRPLTIME",
            "ICMP6_RR_RESULT_FLAGS_OOB",
            "ICMP6_RR_RESULT_FLAGS_FORBIDDEN",
        ];
        assert_complete(&defs, "ICMP6_", &[&icmp6[..], &swapped[..]].concat());
        let mld = assert_defines!(defs;
        MLD_LISTENER_QUERY, MLD_LISTENER_REPORT, MLD_LISTENER_DONE, MLD_MTRACE_RESP, MLD_MTRACE,
        MLDV2_LISTENER_REPORT);
        assert_complete(&defs, "MLD", &mld);
        let nd = assert_defines!(defs;
        ND_ROUTER_SOLICIT, ND_ROUTER_ADVERT, ND_NEIGHBOR_SOLICIT, ND_NEIGHBOR_ADVERT, ND_REDIRECT,
        ND_REDIRECT_ONLINK, ND_REDIRECT_ROUTER, ND_RA_FLAG_MANAGED, ND_RA_FLAG_OTHER,
        ND_RA_FLAG_RTPREF_MASK, ND_RA_FLAG_RTPREF_HIGH, ND_RA_FLAG_RTPREF_MEDIUM,
        ND_RA_FLAG_RTPREF_LOW, ND_RA_FLAG_RTPREF_RSV, ND_OPT_SOURCE_LINKADDR,
        ND_OPT_TARGET_LINKADDR, ND_OPT_PREFIX_INFORMATION, ND_OPT_REDIRECTED_HEADER, ND_OPT_MTU,
        ND_OPT_ROUTE_INFO, ND_OPT_RDNSS, ND_OPT_DNSSL, ND_OPT_PI_FLAG_ONLINK,
        ND_OPT_PI_FLAG_AUTO);
        let na = [
            "ND_NA_FLAG_ROUTER",
            "ND_NA_FLAG_SOLICITED",
            "ND_NA_FLAG_OVERRIDE",
        ];
        assert_complete(&defs, "ND_", &[&nd[..], &na[..]].concat());
        let ni = assert_defines!(defs;
        NI_QTYPE_NOOP, NI_QTYPE_SUPTYPES, NI_QTYPE_FQDN, NI_QTYPE_DNSNAME, NI_QTYPE_NODEADDR,
        NI_QTYPE_IPV4ADDR);
        assert_complete(&defs, "NI_QTYPE_", &ni);
        let rpm =
            assert_defines!(defs; RPM_PCO_ADD, RPM_PCO_CHANGE, RPM_PCO_SETGLOBAL, RPM_PCO_MAX);
        assert_complete(&defs, "RPM_", &rpm);
        let ctl = assert_defines!(defs;
        ICMPV6CTL_STATS, ICMPV6CTL_REDIRACCEPT, ICMPV6CTL_REDIRTIMEOUT, ICMPV6CTL_ND6_DELAY,
        ICMPV6CTL_ND6_UMAXTRIES, ICMPV6CTL_ND6_MMAXTRIES, ICMPV6CTL_ND6_QUEUED,
        ICMPV6CTL_NODEINFO, ICMPV6CTL_ERRPPSLIMIT, ICMPV6CTL_MTUDISC_HIWAT,
        ICMPV6CTL_MTUDISC_LOWAT, ICMPV6CTL_MAXID);
        assert_complete(
            &defs,
            "ICMPV6CTL_",
            &[&ctl[..], &["ICMPV6CTL_NAMES"]].concat(),
        );

        // The byte-swapped flags, by their host-order values.
        let swapped32: &[(&str, u32, u32)] = &[
            ("ND_NA_FLAG_ROUTER", ND_NA_FLAG_ROUTER, 0x8000_0000),
            ("ND_NA_FLAG_SOLICITED", ND_NA_FLAG_SOLICITED, 0x4000_0000),
            ("ND_NA_FLAG_OVERRIDE", ND_NA_FLAG_OVERRIDE, 0x2000_0000),
            (
                "ICMP6_RR_PCOUSE_FLAGS_DECRVLTIME",
                ICMP6_RR_PCOUSE_FLAGS_DECRVLTIME,
                0x8000_0000,
            ),
            (
                "ICMP6_RR_PCOUSE_FLAGS_DECRPLTIME",
                ICMP6_RR_PCOUSE_FLAGS_DECRPLTIME,
                0x4000_0000,
            ),
        ];
        for (name, ours, host) in swapped32 {
            assert_eq!(defs[*name], std::format!("htonl(0x{host:08x})"), "{name}");
            assert_eq!(*ours, htonl(*host), "{name}");
        }
    }
}
/* </TESTS> */
