/*	$OpenBSD: ip_icmp.h,v 1.33 2025/03/02 21:28:32 bluhm Exp $	*/
/*	$NetBSD: ip_icmp.h,v 1.10 1996/02/13 23:42:28 christos Exp $	*/
/*	$OpenBSD: ip_icmp.c,v 1.203 2025/07/08 00:47:41 jsg Exp $	*/
/*	$NetBSD: ip_icmp.c,v 1.19 1996/02/13 23:42:22 christos Exp $	*/
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
 *	@(#)ip_icmp.h	8.1 (Berkeley) 6/10/93
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
 *	@(#)COPYRIGHT	1.1 (NRL) 17 January 1995
 *
 * NRL grants permission for redistribution and use in source and binary
 * forms, with or without modification, of the software and documentation
 * created at NRL provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgements:
 *	This product includes software developed by the University of
 *	California, Berkeley and its contributors.
 *	This product includes software developed at the Information
 *	Technology Division, US Naval Research Laboratory.
 * 4. Neither the name of the NRL nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THE SOFTWARE PROVIDED BY NRL IS PROVIDED BY NRL AND CONTRIBUTORS ``AS
 * IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
 * PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL NRL OR
 * CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 * EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
 * PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
 * LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
 * NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 * The views and conclusions contained in the software and documentation
 * are those of the authors and should not be interpreted as representing
 * official policies, either expressed or implied, of the US Naval
 * Research Laboratory (NRL).
 */
/* </LICENSES> */

//! Interface Control Message Protocol: the definitions of `<netinet/ip_icmp.h>` and
//! `netinet/ip_icmp.c`: error generation, receive packet processing, and routines to turn
//! packets around back to the originator, and host table maintenance routines.
//!
//! Upstream: sys/netinet/ip_icmp.h @ 3ce1f3f79392
//! Upstream: sys/netinet/ip_icmp.c @ 3ce1f3f79392
//!
//! Per RFC 792, September 1981; RFC 950, August 1985 (Address Mask Request / Reply); RFC 1256,
//! September 1991 (Router Advertisement and Solicitation); RFC 1108, November 1991 (Param
//! Problem, Missing Req. Option); RFC 1393, January 1993 (Traceroute); RFC 1475, June 1993
//! (Datagram Conversion Error); RFC 1812, June 1995 (adm prohib, host precedence, precedence
//! cutoff); RFC 2002, October 1996 (Mobility changes to Router Advertisement).
//!
//! [`Icmp`] is the message as it is on the wire, multi-byte fields in network order. The two
//! unions are `#[repr(C)]` unions ([`IcmpHun`], [`IcmpDun`]) and the C's `icmp_id`,
//! `icmp_seq`, `icmp_ip`, ... shorthands are accessor methods; every member is plain data,
//! valid for any bit pattern, so they are safe.
//!
//! `icmp_input` counts every message it accepts by type (`icps_inhist`), answers echo,
//! timestamp and mask requests (`icmp_reflect`, `icmp_send`), hands errors to the protocol of
//! the offending packet (`pr_ctlinput`) and redirects to the routing table, and passes every
//! message on to the raw sockets (`rip_input`).
//!
//! Status: `ip_icmp.h` `ported`, `ip_icmp.c` `ported` (M7b). The licence block of `ip_icmp.c`
//! carries the NRL notice with its advertising clause, accepted as BSD-4
//! (`.claude/rules/scope-and-stubs.md`).
//!
//! ## Deviations
//! - `ICMP_V6ADVLEN(p)`, `ICMP_ADVLEN(p)` and `ICMP_INFOTYPE(type)` are `const fn`s.
//! - The message inside a packet is reached through [`IcmpPkt`], bounds-checked reads and
//!   writes at the C's member offsets: the packet may be shorter than `struct icmp` (8 bytes
//!   for an echo) and its data has no alignment guarantee, so no `&Icmp` is made over mbuf
//!   data. The C's `ICMP_ADVLEN`-style length checks come first, as in C, and the accessor
//!   panics where the C would read past them.
//! - The sysctl variables are `AtomicI32` statics; `icmpcounters` (`struct cpumem *`) is the
//!   static array of atomics [`ICMPCOUNTERS`]. `icmperrppslim_last`/`icmperrpps_count` are a
//!   `StaticCell` touched only through `ppsratecheck`, under its mutex.
//! - `icmp_error` and `icmp_do_error` take the C's `int type, int code` as `u8`s (the values of
//!   `icmp_type`/`icmp_code`); `icmp_reflect` returns `Result` (its `ELOOP`/`EHOSTUNREACH`)
//!   with the options mbuf through an `Option<&mut ...>` out parameter.
//! - `ICMPPRINTFS` (debug printfs) is not configured; `NCARP` (`carp_lsdrop`) is not
//!   configured: each is a comment at its site. `INET6` is (feature `inet6`): the length
//!   check of a v6-in-v4 message.
//!   `NPF` (pf(4)) is configured: the `PF_TAG_DIVERTED` handling of diverted connections.

use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use libkern::StaticCell;

use crate::kassert;
use crate::kern::kern_sysctl::{
    sysctl_bounded_arr, sysctl_int_bounded, sysctl_rdstruct, sysctl_vslock, sysctl_vsunlock,
};
use crate::kern::kern_tc::microtime;
use crate::kern::kern_time::ppsratecheck_shared;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_domain::pfctlinput;
use crate::kern::uipc_mbuf::{
    m_align, m_copyback, m_copydata, m_free, m_freem, m_gethdr, m_getptr, m_prepend, m_pullup,
    m_resethdr,
};
use crate::net::if_::{
    IFF_BROADCAST, IFF_POINTOPOINT, IFF_UP, if_get, if_put, ifa_ifwithaddr, ifaof_ifpforaddr,
};
use crate::net::if_var::{Ifnet, Netstack};
use crate::net::route::{
    RT_RESOLVE, RTAX_DST, RTAX_GATEWAY, RTAX_LABEL, RTF_BLACKHOLE, RTF_BROADCAST, RTF_DYNAMIC,
    RTF_GATEWAY, RTF_HOST, RTF_LLINFO, RTF_LOCAL, RTF_REJECT, RTM_ADD, RTV_MTU, RtAddrinfo,
    Rtentry, RttimerQueue, SockaddrRtlabel, rt_timer_add, rt_timer_queue_change,
    rt_timer_queue_init, rtalloc, rtdeletemsg, rtfree, rtisvalid, rtlabel_id2sa, rtredirect,
    rtrequest,
};
use crate::net::rtable::{rt_key, rtable_getsource};
use crate::net::rtsock::rtm_send;
use crate::netinet::icmp_var::{
    ICMPCTL_BMCASTECHO, ICMPCTL_ERRPPSLIMIT, ICMPCTL_MASKREPL, ICMPCTL_REDIRACCEPT,
    ICMPCTL_REDIRTIMEOUT, ICMPCTL_STATS, ICMPCTL_TSTAMPREPL, ICPS_NCOUNTERS, Icmpstat,
    IcmpstatCounters, icmpstat_inc, icmpstat_inc_hist,
};
#[cfg(feature = "inet6")]
use crate::netinet::in_::IPPROTO_IPV6;
use crate::netinet::in_::{
    IN_CLASSA_NET, IN_CLASSA_NSHIFT, IN_LOOPBACKNET, INADDR_ANY, INADDR_BROADCAST, IPPROTO_DONE,
    IPPROTO_ICMP, IPPROTO_TCP, InAddr, SockaddrIn, in_canforward, in_multicast, satosin_const,
    sintosa,
};
use crate::netinet::in_proto::{INETSW, IP_PROTOX};
use crate::netinet::in_var::{InIfaddr, ifatoia};
use crate::netinet::in4_cksum::in4_cksum;
use crate::netinet::ip::{
    IP_OFFMASK, IPOPT_EOL, IPOPT_NOP, IPOPT_OLEN, IPOPT_OPTVAL, IPOPT_RR, IPOPT_SECURITY, IPOPT_TS,
    IPVERSION, MAXTTL,
};
use crate::netinet::ip_id::ip_randomid;
use crate::netinet::ip_input::{
    IP_MTUDISC_TIMEOUT, ip_forwarding, ip_send, ip_send_raw, ip_srcroute, ip_stripoptions,
};
use crate::netinet::ip_output::ip_insertoptions;
use crate::netinet::ip_var::{IpstatCounters, ipstat_inc, mtod_ip, mtod_ip_store};
use crate::netinet::raw_ip::rip_input;
use crate::sys::endian::{htonl, htons, ntohs};
use crate::sys::errno::Errno;
use crate::sys::limits::INT_MAX;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::mbuf::{
    M_BCAST, M_DONTWAIT, M_EXT, M_ICMP_CSUM_OUT, M_MCAST, MCLBYTES, MHLEN, MT_HEADER, Mbuf,
    PF_TAG_DIVERTED, PF_TAG_GENERATED, mclget, mtod,
};
use crate::sys::protosw::{
    PRC_MSGSIZE, PRC_MTUINC, PRC_PARAMPROB, PRC_QUENCH, PRC_REDIRECT_HOST, PRC_TIMXCEED_INTRANS,
    PRC_UNREACH_HOST, PRC_UNREACH_NET,
};
use crate::sys::socket::AF_INET;
use crate::sys::sysctl::SysctlBoundedArgs;
use crate::sys::systm::{net_assert_locked, net_lock, net_unlock};
use crate::sys::time::Timeval;

use crate::netinet::ip::Ip;

/// `ICMP_EXT_HDR_VERSION`.
pub const ICMP_EXT_HDR_VERSION: u8 = 0x20;
/// `ICMP_EXT_HDR_VMASK`.
pub const ICMP_EXT_HDR_VMASK: u8 = 0xf0;
/// `ICMP_EXT_OFFSET`.
pub const ICMP_EXT_OFFSET: usize = 128;

/// `ICMP_EXT_MPLS`.
pub const ICMP_EXT_MPLS: u8 = 1;
/// `ICMP_EXT_IFINFO`.
pub const ICMP_EXT_IFINFO: u8 = 2;

/// For IPv6 transition related ICMP errors: the shortest such message.
pub const ICMP_V6ADVLENMIN: usize = 8 + size_of::<Ip>() + 40;

// Lower bounds on packet lengths for various types. For the error advice packets must first
// insure that the packet is large enough to contain the returned ip header. Only then can we
// do the check to see if 64 bits of packet data have been returned, since we need to check
// the returned ip header length.

/// Abs minimum.
pub const ICMP_MINLEN: usize = 8;
/// Timestamp.
pub const ICMP_TSLEN: usize = 8 + 3 * size_of::<u32>();
/// Address mask.
pub const ICMP_MASKLEN: usize = 12;
/// Min.
pub const ICMP_ADVLENMIN: usize = 8 + size_of::<Ip>() + 8;
/// Maximum.
pub const ICMP_ADVLENMAX: usize = 8 + 60 + 40;

// Definition of type and code field values (https://www.iana.org/assignments/icmp-parameters).

/// Echo reply.
pub const ICMP_ECHOREPLY: u8 = 0;
/// Dest unreachable, codes:
pub const ICMP_UNREACH: u8 = 3;
/// Bad net.
pub const ICMP_UNREACH_NET: u8 = 0;
/// Bad host.
pub const ICMP_UNREACH_HOST: u8 = 1;
/// Bad protocol.
pub const ICMP_UNREACH_PROTOCOL: u8 = 2;
/// Bad port.
pub const ICMP_UNREACH_PORT: u8 = 3;
/// `IP_DF` caused drop.
pub const ICMP_UNREACH_NEEDFRAG: u8 = 4;
/// Src route failed.
pub const ICMP_UNREACH_SRCFAIL: u8 = 5;
/// Unknown net.
pub const ICMP_UNREACH_NET_UNKNOWN: u8 = 6;
/// Unknown host.
pub const ICMP_UNREACH_HOST_UNKNOWN: u8 = 7;
/// Src host isolated.
pub const ICMP_UNREACH_ISOLATED: u8 = 8;
/// For crypto devs.
pub const ICMP_UNREACH_NET_PROHIB: u8 = 9;
/// Ditto.
pub const ICMP_UNREACH_HOST_PROHIB: u8 = 10;
/// Bad tos for net.
pub const ICMP_UNREACH_TOSNET: u8 = 11;
/// Bad tos for host.
pub const ICMP_UNREACH_TOSHOST: u8 = 12;
/// Prohibited access.
pub const ICMP_UNREACH_FILTER_PROHIB: u8 = 13;
/// Precedence violation.
pub const ICMP_UNREACH_HOST_PRECEDENCE: u8 = 14;
/// Precedence cutoff.
pub const ICMP_UNREACH_PRECEDENCE_CUTOFF: u8 = 15;
/// Packet lost, slow down.
pub const ICMP_SOURCEQUENCH: u8 = 4;
/// Shorter route, codes:
pub const ICMP_REDIRECT: u8 = 5;
/// For network.
pub const ICMP_REDIRECT_NET: u8 = 0;
/// For host.
pub const ICMP_REDIRECT_HOST: u8 = 1;
/// For tos and net.
pub const ICMP_REDIRECT_TOSNET: u8 = 2;
/// For tos and host.
pub const ICMP_REDIRECT_TOSHOST: u8 = 3;
/// Alternate host address.
pub const ICMP_ALTHOSTADDR: u8 = 6;
/// Echo service.
pub const ICMP_ECHO: u8 = 8;
/// Router advertisement.
pub const ICMP_ROUTERADVERT: u8 = 9;
/// Normal advertisement.
pub const ICMP_ROUTERADVERT_NORMAL: u8 = 0;
/// Selective routing.
pub const ICMP_ROUTERADVERT_NOROUTE_COMMON: u8 = 16;
/// Router solicitation.
pub const ICMP_ROUTERSOLICIT: u8 = 10;
/// Time exceeded, code:
pub const ICMP_TIMXCEED: u8 = 11;
/// TTL==0 in transit.
pub const ICMP_TIMXCEED_INTRANS: u8 = 0;
/// TTL==0 in reass.
pub const ICMP_TIMXCEED_REASS: u8 = 1;
/// IP header bad.
pub const ICMP_PARAMPROB: u8 = 12;
/// Req. opt. absent.
pub const ICMP_PARAMPROB_ERRATPTR: u8 = 0;
/// Req. opt. absent.
pub const ICMP_PARAMPROB_OPTABSENT: u8 = 1;
/// Bad length.
pub const ICMP_PARAMPROB_LENGTH: u8 = 2;
/// Timestamp request.
pub const ICMP_TSTAMP: u8 = 13;
/// Timestamp reply.
pub const ICMP_TSTAMPREPLY: u8 = 14;
/// Information request.
pub const ICMP_IREQ: u8 = 15;
/// Information reply.
pub const ICMP_IREQREPLY: u8 = 16;
/// Address mask request.
pub const ICMP_MASKREQ: u8 = 17;
/// Address mask reply.
pub const ICMP_MASKREPLY: u8 = 18;
/// Traceroute.
pub const ICMP_TRACEROUTE: u8 = 30;
/// Data conversion error.
pub const ICMP_DATACONVERR: u8 = 31;
/// Mobile host redirect.
pub const ICMP_MOBILE_REDIRECT: u8 = 32;
/// IPv6 where-are-you.
pub const ICMP_IPV6_WHEREAREYOU: u8 = 33;
/// IPv6 i-am-here.
pub const ICMP_IPV6_IAMHERE: u8 = 34;
/// Mobile registration req.
pub const ICMP_MOBILE_REGREQUEST: u8 = 35;
/// Mobile registration reply.
pub const ICMP_MOBILE_REGREPLY: u8 = 36;
/// SKIP.
pub const ICMP_SKIP: u8 = 39;
/// Photuris.
pub const ICMP_PHOTURIS: u8 = 40;
/// Unknown sec index.
pub const ICMP_PHOTURIS_UNKNOWN_INDEX: u8 = 1;
/// Auth failed.
pub const ICMP_PHOTURIS_AUTH_FAILED: u8 = 2;
/// Decrypt failed.
pub const ICMP_PHOTURIS_DECRYPT_FAILED: u8 = 3;

/// The highest type.
pub const ICMP_MAXTYPE: u8 = 40;

/// `struct icmp_ra_addr`: ICMP Router Advertisement data.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IcmpRaAddr {
    /// Router address.
    pub ira_addr: u32,
    /// Preference.
    pub ira_preference: u32,
}

/// `struct ih_exthdr`: RFC 4884 extended header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IhExthdr {
    /// Padding.
    pub iex_pad: u8,
    /// Length of the original datagram, in 32-bit words.
    pub iex_length: u8,
}

/// `struct ih_idseq`: identifier and sequence number of echo and timestamp messages.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IhIdseq {
    /// Identifier.
    pub icd_id: u16,
    /// Sequence number.
    pub icd_seq: u16,
}

/// `struct ih_pmtu`: `ICMP_UNREACH_NEEDFRAG` -- Path MTU Discovery (RFC 1191).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IhPmtu {
    /// Unused.
    pub ipm_void: u16,
    /// MTU of the next hop.
    pub ipm_nextmtu: u16,
}

/// `struct ih_rtradv`: router advertisement header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IhRtradv {
    /// Number of addresses.
    pub irt_num_addrs: u8,
    /// Words per address.
    pub irt_wpa: u8,
    /// Lifetime.
    pub irt_lifetime: u16,
}

/// The `icmp_hun` union of `struct icmp`: the second word of the header.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IcmpHun {
    /// `ICMP_PARAMPROB`.
    pub ih_pptr: u8,
    /// RFC 4884 extended header.
    pub ih_exthdr: IhExthdr,
    /// `ICMP_REDIRECT`.
    pub ih_gwaddr: InAddr,
    /// Echo and timestamp identifier and sequence.
    pub ih_idseq: IhIdseq,
    /// Unused.
    pub ih_void: i32,
    /// Path MTU discovery.
    pub ih_pmtu: IhPmtu,
    /// Router advertisement.
    pub ih_rtradv: IhRtradv,
}

/// `struct id_ts`: timestamps of the timestamp messages.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IdTs {
    /// Originate timestamp.
    pub its_otime: u32,
    /// Receive timestamp.
    pub its_rtime: u32,
    /// Transmit timestamp.
    pub its_ttime: u32,
}

/// `struct id_ip`: the offending header an error message carries (options and then 64 bits of
/// data follow it).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IdIp {
    /// The returned IP header.
    pub idi_ip: Ip,
}

/// The `icmp_dun` union of `struct icmp`: the message's data.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IcmpDun {
    /// Timestamps.
    pub id_ts: IdTs,
    /// Returned IP header.
    pub id_ip: IdIp,
    /// Address mask.
    pub id_mask: u32,
    /// Start of the data.
    pub id_data: [i8; 1],
}

/// `struct icmp`: structure of an icmp header.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Icmp {
    /// Type of message, see below.
    pub icmp_type: u8,
    /// Type sub code.
    pub icmp_code: u8,
    /// Ones complement cksum of struct.
    pub icmp_cksum: u16,
    /// The header's second word.
    pub icmp_hun: IcmpHun,
    /// The data.
    pub icmp_dun: IcmpDun,
}

/// Defines `struct icmp` accessor pairs for `Copy` members of its unions.
macro_rules! icmp_accessor {
    ($(#[$doc:meta] $get:ident, $set:ident => $($path:ident).+ : $ty:ty;)*) => {
        $(
            #[$doc]
            pub fn $get(&self) -> $ty {
                // SAFETY: every member of the unions is plain data, valid for any bit pattern.
                unsafe { self.$($path).+ }
            }

            #[$doc]
            pub fn $set(&mut self, v: $ty) {
                self.$($path).+ = v;
            }
        )*
    };
}

impl Icmp {
    icmp_accessor! {
        /// `icmp_pptr`: parameter problem pointer.
        icmp_pptr, set_icmp_pptr => icmp_hun.ih_pptr: u8;
        /// `icmp_length`: RFC 4884 length.
        icmp_length, set_icmp_length => icmp_hun.ih_exthdr.iex_length: u8;
        /// `icmp_gwaddr`: redirect gateway.
        icmp_gwaddr, set_icmp_gwaddr => icmp_hun.ih_gwaddr: InAddr;
        /// `icmp_id`: echo identifier, network order.
        icmp_id, set_icmp_id => icmp_hun.ih_idseq.icd_id: u16;
        /// `icmp_seq`: echo sequence number, network order.
        icmp_seq, set_icmp_seq => icmp_hun.ih_idseq.icd_seq: u16;
        /// `icmp_void`.
        icmp_void, set_icmp_void => icmp_hun.ih_void: i32;
        /// `icmp_pmvoid`.
        icmp_pmvoid, set_icmp_pmvoid => icmp_hun.ih_pmtu.ipm_void: u16;
        /// `icmp_nextmtu`: next-hop MTU, network order.
        icmp_nextmtu, set_icmp_nextmtu => icmp_hun.ih_pmtu.ipm_nextmtu: u16;
        /// `icmp_num_addrs`.
        icmp_num_addrs, set_icmp_num_addrs => icmp_hun.ih_rtradv.irt_num_addrs: u8;
        /// `icmp_wpa`.
        icmp_wpa, set_icmp_wpa => icmp_hun.ih_rtradv.irt_wpa: u8;
        /// `icmp_lifetime`.
        icmp_lifetime, set_icmp_lifetime => icmp_hun.ih_rtradv.irt_lifetime: u16;
        /// `icmp_otime`: originate timestamp.
        icmp_otime, set_icmp_otime => icmp_dun.id_ts.its_otime: u32;
        /// `icmp_rtime`: receive timestamp.
        icmp_rtime, set_icmp_rtime => icmp_dun.id_ts.its_rtime: u32;
        /// `icmp_ttime`: transmit timestamp.
        icmp_ttime, set_icmp_ttime => icmp_dun.id_ts.its_ttime: u32;
        /// `icmp_mask`: address mask.
        icmp_mask, set_icmp_mask => icmp_dun.id_mask: u32;
    }

    /// `icmp_ip`: the returned IP header of an error message.
    pub fn icmp_ip(&self) -> &Ip {
        // SAFETY: every member of the unions is plain data, valid for any bit pattern.
        unsafe { &self.icmp_dun.id_ip.idi_ip }
    }

    /// `icmp_ip`, writable.
    pub fn icmp_ip_mut(&mut self) -> &mut Ip {
        // SAFETY: as above; an `Ip` is integers, so writing it leaves every member valid.
        unsafe { &mut self.icmp_dun.id_ip.idi_ip }
    }

    /// `icmp_data`: the address of the message's data (which runs past the structure).
    pub fn icmp_data(&mut self) -> *mut i8 {
        core::ptr::addr_of_mut!(self.icmp_dun.id_data).cast()
    }
}

/// `struct icmp_ext_hdr`: the RFC 4884 extension header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IcmpExtHdr {
    /// Only high nibble used.
    pub ieh_version: u8,
    /// Reserved, must be zero.
    pub ieh_res: u8,
    /// Ones complement cksum of ext hdr.
    pub ieh_cksum: u16,
}

/// `struct icmp_ext_obj_hdr`: an RFC 4884 extension object header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IcmpExtObjHdr {
    /// Length of obj incl this header.
    pub ieo_length: u16,
    /// Class number.
    pub ieo_cnum: u8,
    /// Sub class type.
    pub ieo_ctype: u8,
}

/// `offsetof(struct icmp, icmp_cksum)`: where the checksum of an ICMP message is.
pub const ICMP_CKSUM_OFFSET: usize = offset_of!(Icmp, icmp_cksum);

/// The offset of `icmp_hun` (the second word).
const ICMP_HUN: usize = offset_of!(Icmp, icmp_hun);
/// The offset of `icmp_dun` (the data).
const ICMP_DUN: usize = offset_of!(Icmp, icmp_dun);

/// An ICMP message inside a packet: `len` contiguous bytes from its type byte on, read and
/// written at the members' offsets (see the module's deviations).
#[derive(Clone, Copy)]
pub struct IcmpPkt {
    p: *mut u8,
    len: usize,
}

impl IcmpPkt {
    /// The message at `p`, of which `len` bytes are contiguous.
    ///
    /// # Safety
    ///
    /// `p` points at `len` readable and writable bytes (an mbuf's data), valid as long as the
    /// view is used.
    pub unsafe fn new(p: *mut u8, len: usize) -> Self {
        Self { p, len }
    }

    /// The message at offset `off` of `m`'s first mbuf, up to that mbuf's end.
    pub fn of(m: &Mbuf, off: usize) -> Self {
        let mlen = m.m_len().get() as usize;
        if off > mlen {
            panic(format_args!("icmp: header at {off} of {mlen}"));
        }
        // SAFETY: the first mbuf holds `m_len` bytes at its data pointer.
        unsafe { Self::new(mtod::<u8>(m).add(off), mlen - off) }
    }

    /// The address of member bytes `off .. off + size_of::<T>()`.
    fn at<T>(&self, off: usize) -> *mut T {
        if off + size_of::<T>() > self.len {
            panic(format_args!("icmp: access at {off} of {}", self.len));
        }
        self.p.wrapping_add(off).cast()
    }

    fn get<T: Copy>(&self, off: usize) -> T {
        // SAFETY: `at` checked the bounds; the members are integers, valid for any bytes.
        unsafe { ptr::read_unaligned(self.at::<T>(off)) }
    }

    fn set<T>(&self, off: usize, v: T) {
        // SAFETY: `at` checked the bounds of the view's writable bytes.
        unsafe { ptr::write_unaligned(self.at::<T>(off), v) };
    }

    /// The message's first byte.
    pub fn as_ptr(&self) -> *mut u8 {
        self.p
    }

    /// `icmp_type`.
    pub fn icmp_type(&self) -> u8 {
        self.get(offset_of!(Icmp, icmp_type))
    }

    /// Sets `icmp_type`.
    pub fn set_icmp_type(&self, v: u8) {
        self.set(offset_of!(Icmp, icmp_type), v);
    }

    /// `icmp_code`.
    pub fn icmp_code(&self) -> u8 {
        self.get(offset_of!(Icmp, icmp_code))
    }

    /// Sets `icmp_code`.
    pub fn set_icmp_code(&self, v: u8) {
        self.set(offset_of!(Icmp, icmp_code), v);
    }

    /// Sets `icmp_cksum`.
    pub fn set_icmp_cksum(&self, v: u16) {
        self.set(ICMP_CKSUM_OFFSET, v);
    }

    /// Sets `icmp_pptr`.
    pub fn set_icmp_pptr(&self, v: u8) {
        self.set(ICMP_HUN, v);
    }

    /// `icmp_length` (RFC 4884).
    pub fn icmp_length(&self) -> u8 {
        self.get(ICMP_HUN + offset_of!(IhExthdr, iex_length))
    }

    /// Sets `icmp_length`.
    pub fn set_icmp_length(&self, v: u8) {
        self.set(ICMP_HUN + offset_of!(IhExthdr, iex_length), v);
    }

    /// `icmp_gwaddr`.
    pub fn icmp_gwaddr(&self) -> InAddr {
        self.get(ICMP_HUN)
    }

    /// Sets `icmp_gwaddr`.
    pub fn set_icmp_gwaddr(&self, v: InAddr) {
        self.set(ICMP_HUN, v);
    }

    /// `icmp_id`, network order.
    pub fn icmp_id(&self) -> u16 {
        self.get(ICMP_HUN + offset_of!(IhIdseq, icd_id))
    }

    /// Sets `icmp_id`.
    pub fn set_icmp_id(&self, v: u16) {
        self.set(ICMP_HUN + offset_of!(IhIdseq, icd_id), v);
    }

    /// `icmp_seq`, network order.
    pub fn icmp_seq(&self) -> u16 {
        self.get(ICMP_HUN + offset_of!(IhIdseq, icd_seq))
    }

    /// Sets `icmp_seq`.
    pub fn set_icmp_seq(&self, v: u16) {
        self.set(ICMP_HUN + offset_of!(IhIdseq, icd_seq), v);
    }

    /// Sets `icmp_void`.
    pub fn set_icmp_void(&self, v: i32) {
        self.set(ICMP_HUN, v);
    }

    /// `icmp_nextmtu`.
    pub fn icmp_nextmtu(&self) -> u16 {
        self.get(ICMP_HUN + offset_of!(IhPmtu, ipm_nextmtu))
    }

    /// Sets `icmp_nextmtu`.
    pub fn set_icmp_nextmtu(&self, v: u16) {
        self.set(ICMP_HUN + offset_of!(IhPmtu, ipm_nextmtu), v);
    }

    /// `icmp_ip`: the returned IP header of an error message.
    pub fn icmp_ip(&self) -> Ip {
        self.get(ICMP_DUN)
    }

    /// `&icmp_ip`.
    pub fn icmp_ip_ptr(&self) -> *mut Ip {
        self.at::<Ip>(ICMP_DUN)
    }

    /// `icmp_rtime`.
    pub fn icmp_rtime(&self) -> u32 {
        self.get(ICMP_DUN + offset_of!(IdTs, its_rtime))
    }

    /// Sets `icmp_rtime`.
    pub fn set_icmp_rtime(&self, v: u32) {
        self.set(ICMP_DUN + offset_of!(IdTs, its_rtime), v);
    }

    /// Sets `icmp_ttime`.
    pub fn set_icmp_ttime(&self, v: u32) {
        self.set(ICMP_DUN + offset_of!(IdTs, its_ttime), v);
    }

    /// Sets `icmp_mask`.
    pub fn set_icmp_mask(&self, v: u32) {
        self.set(ICMP_DUN, v);
    }
}

/// `icmperrppslim_last` and `icmperrpps_count`.
struct IcmpErrPps {
    last: Timeval,
    count: i32,
}

/// `ICMP_V6ADVLEN(p)`: the length of an IPv6 transition error message for `p`.
pub const fn icmp_v6advlen(p: &Icmp) -> usize {
    // SAFETY: every member of the unions is plain data, valid for any bit pattern.
    let hl = unsafe { p.icmp_dun.id_ip.idi_ip.ip_hl() };
    8 + ((hl as usize) << 2) + 40
}

/// `ICMP_ADVLEN(p)`: the length of an error message for `p`. N.B.: must separately check
/// that `ip_hl >= 5`.
pub const fn icmp_advlen(p: &Icmp) -> usize {
    // SAFETY: every member of the unions is plain data, valid for any bit pattern.
    let hl = unsafe { p.icmp_dun.id_ip.idi_ip.ip_hl() };
    8 + ((hl as usize) << 2) + 8
}

/// `ICMP_INFOTYPE(type)`: whether `t` is an informational (not an error) message type.
pub const fn icmp_infotype(t: u8) -> bool {
    matches!(
        t,
        ICMP_ECHOREPLY
            | ICMP_ECHO
            | ICMP_ROUTERADVERT
            | ICMP_ROUTERSOLICIT
            | ICMP_TSTAMP
            | ICMP_TSTAMPREPLY
            | ICMP_IREQ
            | ICMP_IREQREPLY
            | ICMP_MASKREQ
            | ICMP_MASKREPLY
    )
}

/// \[a\] `icmpmaskrepl`.
pub static ICMPMASKREPL: AtomicI32 = AtomicI32::new(0);
/// \[a\] `icmpbmcastecho`.
pub static ICMPBMCASTECHO: AtomicI32 = AtomicI32::new(0);
/// \[a\] `icmptstamprepl`.
pub static ICMPTSTAMPREPL: AtomicI32 = AtomicI32::new(1);
/// \[a\] `icmperrppslim`.
pub static ICMPERRPPSLIM: AtomicI32 = AtomicI32::new(100);
/// \[a\] `icmp_rediraccept`.
pub static ICMP_REDIRACCEPT: AtomicI32 = AtomicI32::new(0);
/// `icmp_redirtimeout`: changed under the net lock.
pub static ICMP_REDIRTIMEOUT: AtomicI32 = AtomicI32::new(10 * 60);

/// `icmperrpps_count`, `icmperrppslim_last`: only `ppsratecheck` touches them, under its
/// mutex.
static ICMPERRPPS: StaticCell<IcmpErrPps> = StaticCell::new(IcmpErrPps {
    last: Timeval {
        tv_sec: 0,
        tv_usec: 0,
    },
    count: 0,
});

/// `ip_mtudisc_timeout_q`.
pub static IP_MTUDISC_TIMEOUT_Q: RttimerQueue = RttimerQueue::new();
/// `icmp_redirect_timeout_q`.
pub static ICMP_REDIRECT_TIMEOUT_Q: RttimerQueue = RttimerQueue::new();

/// `icmpcounters`: the ICMP statistics (see the module's deviations).
pub static ICMPCOUNTERS: [AtomicU64; ICPS_NCOUNTERS] =
    [const { AtomicU64::new(0) }; ICPS_NCOUNTERS];

/// `icmpctl_vars[]`.
static ICMPCTL_VARS: [SysctlBoundedArgs; 5] = [
    SysctlBoundedArgs::new(ICMPCTL_MASKREPL, &ICMPMASKREPL, 0, 1),
    SysctlBoundedArgs::new(ICMPCTL_BMCASTECHO, &ICMPBMCASTECHO, 0, 1),
    SysctlBoundedArgs::new(ICMPCTL_ERRPPSLIMIT, &ICMPERRPPSLIM, -1, INT_MAX),
    SysctlBoundedArgs::new(ICMPCTL_REDIRACCEPT, &ICMP_REDIRACCEPT, 0, 1),
    SysctlBoundedArgs::new(ICMPCTL_TSTAMPREPL, &ICMPTSTAMPREPL, 0, 1),
];

/// `mtu_table[]`: table of common MTUs.
const MTU_TABLE: [u16; 14] = [
    65535, 65280, 32000, 17914, 9180, 8166, 4352, 2002, 1492, 1006, 508, 296, 68, 0,
];

/// `icmp_init`.
pub fn icmp_init() {
    rt_timer_queue_init(
        &IP_MTUDISC_TIMEOUT_Q,
        IP_MTUDISC_TIMEOUT.load(Ordering::Relaxed),
        Some(icmp_mtudisc_timeout),
    );
    rt_timer_queue_init(
        &ICMP_REDIRECT_TIMEOUT_Q,
        ICMP_REDIRTIMEOUT.load(Ordering::Relaxed),
        None,
    );
    // icmpcounters = counters_alloc(icps_ncounters): a static (the module's deviations).
}

/// `icmp_do_error`: the ICMP error message of `type_` and `code` about packet `n` (freed),
/// with its IP header in front; `None` when no error may or can be sent.
pub fn icmp_do_error(
    n: &'static Mbuf,
    type_: u8,
    code: u8,
    dest: u32,
    destmtu: i32,
) -> Option<&'static Mbuf> {
    let oip = mtod_ip(n);
    let oiplen = usize::from(oip.ip_hl()) << 2;
    let mut code = code;

    // ICMPPRINTFS: not configured.
    if type_ != ICMP_REDIRECT {
        icmpstat_inc(IcmpstatCounters::IcpsError);
    }
    'freeit: {
        // Don't send error if not the first fragment of message. Don't error if the old packet
        // protocol was ICMP error message, only known informational types.
        if oip.ip_off & htons(IP_OFFMASK) != 0 {
            break 'freeit;
        }
        if i32::from(oip.ip_p) == IPPROTO_ICMP
            && type_ != ICMP_REDIRECT
            && n.m_len().get() as usize >= oiplen + ICMP_MINLEN
            && !icmp_infotype(IcmpPkt::of(n, oiplen).icmp_type())
        {
            icmpstat_inc(IcmpstatCounters::IcpsOldicmp);
            break 'freeit;
        }
        // Don't send error in response to a multicast or broadcast packet
        if n.m_flags().get() & (M_BCAST | M_MCAST) != 0 {
            break 'freeit;
        }

        // First, do a rate limitation check.
        if icmp_ratelimit(&oip.ip_src, type_, code) {
            icmpstat_inc(IcmpstatCounters::IcpsToofreq);
            break 'freeit;
        }

        // Now, formulate icmp message
        let mut icmplen = oiplen + 8.min(usize::from(ntohs(oip.ip_len)));
        // Defend against mbuf chains shorter than oip->ip_len:
        let mut mblen = 0;
        let mut mm = Some(n);
        while let Some(x) = mm {
            if mblen >= icmplen {
                break;
            }
            mblen += x.m_len().get() as usize;
            mm = x.m_next().get();
        }
        icmplen = mblen.min(icmplen);

        // As we are not required to return everything we have, we return whatever we can
        // return at ease. Note that ICMP datagrams longer than 576 octets are out of spec
        // according to RFC1812;
        kassert!(ICMP_MINLEN + size_of::<Ip>() <= MCLBYTES);

        if size_of::<Ip>() + icmplen + ICMP_MINLEN > MCLBYTES {
            icmplen = MCLBYTES - ICMP_MINLEN - size_of::<Ip>();
        }

        let mut m = m_gethdr(M_DONTWAIT, MT_HEADER);
        if let Some(mm) = m
            && ((size_of::<Ip>() + icmplen + ICMP_MINLEN + size_of::<u64>() - 1)
                & !(size_of::<u64>() - 1))
                > MHLEN
        {
            mclget(mm, M_DONTWAIT);
            if mm.m_flags().get() & M_EXT == 0 {
                m_freem(mm);
                m = None;
            }
        }
        let Some(m) = m else {
            break 'freeit;
        };
        // keep in same rtable and preserve other pkthdr bits
        m.m_pkthdr().ph_rtableid.set(n.m_pkthdr().ph_rtableid.get());
        m.m_pkthdr().ph_ifidx.set(n.m_pkthdr().ph_ifidx.get());
        // move PF_GENERATED to new packet, if existent XXX preserve more?
        if n.m_pkthdr().pf.flags.get() & PF_TAG_GENERATED != 0 {
            let f = &m.m_pkthdr().pf.flags;
            f.set(f.get() | PF_TAG_GENERATED);
        }
        let len = icmplen + ICMP_MINLEN;
        m.m_pkthdr().len.set(len as i32);
        m.m_len().set(len as u32);
        m_align(m, len as i32);
        let icp = IcmpPkt::of(m, 0);
        if type_ > ICMP_MAXTYPE {
            panic(format_args!("icmp_error"));
        }
        icmpstat_inc_hist(IcmpstatCounters::IcpsOuthist, type_);
        icp.set_icmp_type(type_);
        if type_ == ICMP_REDIRECT {
            icp.set_icmp_gwaddr(InAddr { s_addr: dest });
        } else {
            icp.set_icmp_void(0);
            // The following assignments assume an overlay with the zeroed icmp_void field.
            if type_ == ICMP_PARAMPROB {
                icp.set_icmp_pptr(code);
                code = 0;
            } else if type_ == ICMP_UNREACH && code == ICMP_UNREACH_NEEDFRAG && destmtu != 0 {
                icp.set_icmp_nextmtu(htons(destmtu as u16));
            }
        }

        icp.set_icmp_code(code);
        // SAFETY: the new mbuf holds `ICMP_MINLEN + icmplen` bytes at its data.
        let dst = unsafe { core::slice::from_raw_parts_mut(icp.as_ptr().add(ICMP_DUN), icmplen) };
        m_copydata(n, 0, dst);

        // Now, copy old ip header (without options) in front of icmp message.
        let Some(m) = m_prepend(m, size_of::<Ip>() as i32, M_DONTWAIT) else {
            break 'freeit;
        };
        let mut nip = Ip::default();
        // ip_v set in ip_output
        nip.set_ip_hl((size_of::<Ip>() >> 2) as u8);
        nip.ip_tos = 0;
        nip.ip_len = htons(m.m_len().get() as u16);
        // ip_id set in ip_output
        nip.ip_off = 0;
        // ip_ttl set in icmp_reflect
        nip.ip_p = IPPROTO_ICMP as u8;
        nip.ip_src = oip.ip_src;
        nip.ip_dst = oip.ip_dst;
        mtod_ip_store(m, &nip);

        m_freem(n);
        return Some(m);
    }
    // freeit:
    m_freem(n);
    None
}

/// `icmp_error`: generates an error packet of `type_` in response to bad packet `n`.
pub fn icmp_error(n: &'static Mbuf, type_: u8, code: u8, dest: u32, destmtu: i32) {
    if let Some(m) = icmp_do_error(n, type_, code, dest, destmtu)
        && icmp_reflect(m, None, None).is_ok()
    {
        icmp_send(m, None);
    }
}

/// `icmp_input`: processes a received ICMP message.
pub fn icmp_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };
    let Some(ifp) = if_get(m.m_pkthdr().ph_ifidx.get()) else {
        crate::sys::mbuf::m_freemp(mp);
        return IPPROTO_DONE;
    };

    let proto = icmp_input_if(ifp, mp, offp, proto, af, ns);
    if_put(Some(ifp));
    proto
}

/// `icmp_input_if`: `icmp_input` with the receiving interface.
pub fn icmp_input_if(
    ifp: &'static Ifnet,
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let Some(mut m) = *mp else {
        return IPPROTO_DONE;
    };
    let hlen = *offp as usize;
    let mut ip = mtod_ip(m);

    // Locate icmp structure in mbuf, and check that not corrupted and of at least minimum
    // length.
    let icmplen = i32::from(ntohs(ip.ip_len)) - hlen as i32;
    // ICMPPRINTFS: not configured.
    'freeit: {
        if icmplen < ICMP_MINLEN as i32 {
            icmpstat_inc(IcmpstatCounters::IcpsTooshort);
            break 'freeit;
        }
        let icmplen = icmplen as usize;
        let i = hlen + icmplen.min(ICMP_ADVLENMAX);
        let Some(mm) = m_pullup(m, i as i32) else {
            *mp = None;
            icmpstat_inc(IcmpstatCounters::IcpsTooshort);
            return IPPROTO_DONE;
        };
        m = mm;
        *mp = Some(m);
        ip = mtod_ip(m);
        if in4_cksum(m, 0, hlen as i32, icmplen as i32) != 0 {
            icmpstat_inc(IcmpstatCounters::IcpsChecksum);
            break 'freeit;
        }

        let icp = IcmpPkt::of(m, hlen);
        // ICMPPRINTFS: not configured.
        'raw: {
            if icp.icmp_type() > ICMP_MAXTYPE {
                break 'raw;
            }
            let pf = &m.m_pkthdr().pf;
            if pf.flags.get() & PF_TAG_DIVERTED != 0 {
                match icp.icmp_type() {
                    // As pf_icmp_mapping() considers redirects belonging to a diverted
                    // connection, we must include it here. The other types map to other
                    // connections: they must be delivered to pr_ctlinput() also for diverted
                    // connections.
                    ICMP_REDIRECT | ICMP_UNREACH | ICMP_TIMXCEED | ICMP_PARAMPROB
                    | ICMP_SOURCEQUENCH => {
                        // Do not use the divert-to property of the TCP or UDP rule when doing
                        // the PCB lookup for the raw socket.
                        pf.flags.set(pf.flags.get() & !PF_TAG_DIVERTED);
                    }
                    _ => break 'raw,
                }
            }
            icmpstat_inc_hist(IcmpstatCounters::IcpsInhist, icp.icmp_type());
            let code = icp.icmp_code();
            // The C's deliver/badcode/reflect labels: what the message turns into.
            enum Next {
                Deliver(i32),
                Badcode,
                Reflect,
                Done,
            }
            let next = match icp.icmp_type() {
                ICMP_UNREACH => match code {
                    ICMP_UNREACH_NET
                    | ICMP_UNREACH_HOST
                    | ICMP_UNREACH_PROTOCOL
                    | ICMP_UNREACH_PORT
                    | ICMP_UNREACH_SRCFAIL => Next::Deliver(i32::from(code) + PRC_UNREACH_NET),

                    ICMP_UNREACH_NEEDFRAG => Next::Deliver(PRC_MSGSIZE),

                    ICMP_UNREACH_NET_UNKNOWN | ICMP_UNREACH_NET_PROHIB | ICMP_UNREACH_TOSNET => {
                        Next::Deliver(PRC_UNREACH_NET)
                    }

                    ICMP_UNREACH_HOST_UNKNOWN
                    | ICMP_UNREACH_ISOLATED
                    | ICMP_UNREACH_HOST_PROHIB
                    | ICMP_UNREACH_TOSHOST
                    | ICMP_UNREACH_FILTER_PROHIB
                    | ICMP_UNREACH_HOST_PRECEDENCE
                    | ICMP_UNREACH_PRECEDENCE_CUTOFF => Next::Deliver(PRC_UNREACH_HOST),

                    _ => Next::Badcode,
                },

                ICMP_TIMXCEED => {
                    if code > 1 {
                        Next::Badcode
                    } else {
                        Next::Deliver(i32::from(code) + PRC_TIMXCEED_INTRANS)
                    }
                }

                ICMP_PARAMPROB => {
                    if code > 1 {
                        Next::Badcode
                    } else {
                        Next::Deliver(PRC_PARAMPROB)
                    }
                }

                ICMP_SOURCEQUENCH => {
                    if code != 0 {
                        Next::Badcode
                    } else {
                        Next::Deliver(PRC_QUENCH)
                    }
                }

                ICMP_ECHO => {
                    if ICMPBMCASTECHO.load(Ordering::Relaxed) == 0
                        && m.m_flags().get() & (M_MCAST | M_BCAST) != 0
                    {
                        icmpstat_inc(IcmpstatCounters::IcpsBmcastecho);
                        Next::Done
                    } else {
                        icp.set_icmp_type(ICMP_ECHOREPLY);
                        Next::Reflect
                    }
                }

                ICMP_TSTAMP => 'ts: {
                    if ICMPTSTAMPREPL.load(Ordering::Relaxed) == 0 {
                        break 'ts Next::Done;
                    }

                    if ICMPBMCASTECHO.load(Ordering::Relaxed) == 0
                        && m.m_flags().get() & (M_MCAST | M_BCAST) != 0
                    {
                        icmpstat_inc(IcmpstatCounters::IcpsBmcastecho);
                        break 'ts Next::Done;
                    }
                    if icmplen < ICMP_TSLEN {
                        icmpstat_inc(IcmpstatCounters::IcpsBadlen);
                        break 'ts Next::Done;
                    }
                    icp.set_icmp_type(ICMP_TSTAMPREPLY);
                    icp.set_icmp_rtime(iptime());
                    icp.set_icmp_ttime(icp.icmp_rtime()); // bogus, do later!
                    Next::Reflect
                }

                ICMP_MASKREQ => 'mask: {
                    if ICMPMASKREPL.load(Ordering::Relaxed) == 0 {
                        break 'mask Next::Done;
                    }
                    if icmplen < ICMP_MASKLEN {
                        icmpstat_inc(IcmpstatCounters::IcpsBadlen);
                        break 'mask Next::Done;
                    }
                    // We are not able to respond with all ones broadcast unless we receive it
                    // over a point-to-point interface.
                    let mut sin = SockaddrIn {
                        sin_family: AF_INET,
                        sin_len: size_of::<SockaddrIn>() as u8,
                        ..SockaddrIn::default()
                    };
                    if ip.ip_dst.s_addr == INADDR_BROADCAST || ip.ip_dst.s_addr == INADDR_ANY {
                        sin.sin_addr = ip.ip_src;
                    } else {
                        sin.sin_addr = ip.ip_dst;
                    }
                    // SAFETY: a local `sockaddr_in`.
                    let Some(ifa) = (unsafe { ifaof_ifpforaddr(sintosa(&mut sin), ifp) }) else {
                        break 'mask Next::Done;
                    };
                    let ia = ifatoia(ifa);
                    icp.set_icmp_type(ICMP_MASKREPLY);
                    icp.set_icmp_mask(ia.ia_sockmask.get().sin_addr.s_addr);
                    if ip.ip_src.s_addr == 0 {
                        if ifp.if_flags.get() & IFF_BROADCAST != 0 {
                            if ia.ia_broadaddr().get().sin_addr.s_addr != 0 {
                                ip.ip_src = ia.ia_broadaddr().get().sin_addr;
                            } else {
                                ip.ip_src.s_addr = INADDR_BROADCAST;
                            }
                        } else if ifp.if_flags.get() & IFF_POINTOPOINT != 0 {
                            ip.ip_src = ia.ia_dstaddr.get().sin_addr;
                        }
                        mtod_ip_store(m, &ip);
                    }
                    Next::Reflect
                }

                ICMP_REDIRECT => 'redirect: {
                    let i_am_router = ip_forwarding.load(Ordering::Relaxed) != 0;

                    if ICMP_REDIRACCEPT.load(Ordering::Relaxed) == 0 || i_am_router {
                        break 'freeit;
                    }
                    if code > 3 {
                        break 'redirect Next::Badcode;
                    }
                    if icmplen < ICMP_ADVLENMIN
                        || icmplen < icmp_advlen_of(&icp)
                        || usize::from(icp.icmp_ip().ip_hl()) < (size_of::<Ip>() >> 2)
                    {
                        icmpstat_inc(IcmpstatCounters::IcpsBadlen);
                        break 'redirect Next::Done;
                    }
                    // Short circuit routing redirects to force immediate change in the
                    // kernel's routing tables. The message is also handed to anyone listening
                    // on a raw socket (e.g. the routing daemon for use in updating its tables).
                    let blank = SockaddrIn {
                        sin_family: AF_INET,
                        sin_len: size_of::<SockaddrIn>() as u8,
                        ..SockaddrIn::default()
                    };
                    let mut sdst = SockaddrIn {
                        sin_addr: icp.icmp_ip().ip_dst,
                        ..blank
                    };
                    let mut sgw = SockaddrIn {
                        sin_addr: icp.icmp_gwaddr(),
                        ..blank
                    };
                    let mut ssrc = SockaddrIn {
                        sin_addr: ip.ip_src,
                        ..blank
                    };

                    // ICMPPRINTFS: not configured. NCARP > 0: carp_lsdrop; not configured.
                    let mut newrt = None;
                    // SAFETY: local `sockaddr_in`s.
                    unsafe {
                        rtredirect(
                            sintosa(&mut sdst),
                            sintosa(&mut sgw),
                            sintosa(&mut ssrc),
                            Some(&mut newrt),
                            m.m_pkthdr().ph_rtableid.get(),
                        )
                    };
                    if let Some(nrt) = newrt
                        && ICMP_REDIRTIMEOUT.load(Ordering::Relaxed) > 0
                    {
                        let _ = rt_timer_add(
                            nrt,
                            &ICMP_REDIRECT_TIMEOUT_Q,
                            m.m_pkthdr().ph_rtableid.get(),
                        );
                    }
                    rtfree(newrt);
                    // SAFETY: a local `sockaddr_in`.
                    unsafe { pfctlinput(PRC_REDIRECT_HOST, sintosa(&mut sdst)) };
                    Next::Done
                }

                // No kernel processing for the following; just fall through to send to raw
                // listener: ICMP_ECHOREPLY, ICMP_ROUTERADVERT, ICMP_ROUTERSOLICIT,
                // ICMP_TSTAMPREPLY, ICMP_IREQREPLY, ICMP_MASKREPLY, ICMP_TRACEROUTE,
                // ICMP_DATACONVERR, ICMP_MOBILE_REDIRECT, ICMP_IPV6_WHEREAREYOU,
                // ICMP_IPV6_IAMHERE, ICMP_MOBILE_REGREQUEST, ICMP_MOBILE_REGREPLY,
                // ICMP_PHOTURIS, default:
                _ => Next::Done,
            };

            match next {
                Next::Deliver(code) => {
                    // Problem with datagram; advise higher level routines.
                    if icmplen < ICMP_ADVLENMIN
                        || icmplen < icmp_advlen_of(&icp)
                        || usize::from(icp.icmp_ip().ip_hl()) < (size_of::<Ip>() >> 2)
                    {
                        icmpstat_inc(IcmpstatCounters::IcpsBadlen);
                        break 'freeit;
                    }
                    let oip = icp.icmp_ip();
                    if in_multicast(oip.ip_dst.s_addr) {
                        icmpstat_inc(IcmpstatCounters::IcpsBadcode);
                    } else {
                        // Get more contiguous data for a v6 in v4 ICMP message.
                        #[cfg(feature = "inet6")]
                        if i32::from(oip.ip_p) == IPPROTO_IPV6
                            && (icmplen < ICMP_V6ADVLENMIN || icmplen < icmp_v6advlen_of(&icp))
                        {
                            icmpstat_inc(IcmpstatCounters::IcpsBadlen);
                            break 'freeit;
                        }
                        // ICMPPRINTFS: not configured.
                        let mut sin = SockaddrIn {
                            sin_family: AF_INET,
                            sin_len: size_of::<SockaddrIn>() as u8,
                            sin_addr: oip.ip_dst,
                            ..SockaddrIn::default()
                        };
                        // NCARP > 0: carp_lsdrop; not configured.
                        // XXX if the packet contains [IPv4 AH TCP], we can't make a
                        // notification to TCP layer.
                        let pr = &INETSW
                            [usize::from(IP_PROTOX[usize::from(oip.ip_p)].load(Ordering::Relaxed))];
                        if let Some(ctlfunc) = pr.pr_ctlinput {
                            // SAFETY: a local `sockaddr_in`; the argument is the returned IP
                            // header, inside the pulled-up message.
                            unsafe {
                                ctlfunc(
                                    code,
                                    sintosa(&mut sin),
                                    m.m_pkthdr().ph_rtableid.get(),
                                    icp.icmp_ip_ptr().cast::<c_void>(),
                                )
                            };
                        }
                    }
                }
                Next::Badcode => icmpstat_inc(IcmpstatCounters::IcpsBadcode),
                Next::Reflect => {
                    // NCARP > 0: carp_lsdrop; not configured.
                    icmpstat_inc(IcmpstatCounters::IcpsReflect);
                    icmpstat_inc_hist(IcmpstatCounters::IcpsOuthist, icp.icmp_type());
                    let mut opts = None;
                    if icmp_reflect(m, Some(&mut opts), None).is_ok() {
                        icmp_send(m, opts);
                        m_free(opts);
                    }
                    *mp = None;
                    return IPPROTO_DONE;
                }
                Next::Done => {}
            }
        }
        // raw:
        return rip_input(mp, offp, proto, af, ns);
    }
    // freeit:
    m_freem(m);
    *mp = None;
    IPPROTO_DONE
}

/// `ICMP_ADVLEN(icp)` for a message in a packet.
fn icmp_advlen_of(icp: &IcmpPkt) -> usize {
    8 + (usize::from(icp.icmp_ip().ip_hl()) << 2) + 8
}

/// `ICMP_V6ADVLEN(icp)` for a message in a packet.
#[cfg(feature = "inet6")]
fn icmp_v6advlen_of(icp: &IcmpPkt) -> usize {
    8 + (usize::from(icp.icmp_ip().ip_hl()) << 2) + 40
}

/// `icmp_reflect`: reflects the IP packet back to the source. With `op`, the options to send
/// it with (`ip_srcroute`'s, plus the record-route, timestamp and security options of the
/// packet) go there. On error the packet is freed.
pub fn icmp_reflect(
    m: &'static Mbuf,
    op: Option<&mut Option<&'static Mbuf>>,
    ia: Option<&InIfaddr>,
) -> Result<(), Errno> {
    let mut ip = mtod_ip(m);
    let mut opts: Option<&'static Mbuf> = None;
    let mut ip_src = InAddr { s_addr: INADDR_ANY };
    let optlen = (usize::from(ip.ip_hl()) << 2) as i32 - size_of::<Ip>() as i32;

    if !in_canforward(ip.ip_src)
        && (ip.ip_src.s_addr & IN_CLASSA_NET) != htonl(IN_LOOPBACKNET << IN_CLASSA_NSHIFT)
    {
        m_freem(m); // Bad return address
        return Err(Errno::EHOSTUNREACH);
    }

    let loopcnt = m.m_pkthdr().ph_loopcnt.get();
    m.m_pkthdr().ph_loopcnt.set(loopcnt.wrapping_add(1));
    if loopcnt >= crate::sys::mbuf::M_MAXLOOP {
        m_freem(m);
        return Err(Errno::ELOOP);
    }
    let rtableid = m.m_pkthdr().ph_rtableid.get();

    // If the incoming packet was addressed directly to us, use dst as the src for the reply.
    // For broadcast, use the address which corresponds to the incoming interface.
    match ia {
        None => {
            let mut sin = SockaddrIn {
                sin_len: size_of::<SockaddrIn>() as u8,
                sin_family: AF_INET,
                sin_addr: ip.ip_dst,
                ..SockaddrIn::default()
            };

            // SAFETY: a local `sockaddr_in`.
            let rt = unsafe { rtalloc(sintosa(&mut sin), 0, rtableid) };
            if let Some(r) = rt
                && rtisvalid(Some(r))
            {
                if r.rt_flags.get() & RTF_LOCAL != 0 {
                    ip_src = ip.ip_dst;
                } else if r.rt_flags.get() & RTF_BROADCAST != 0 {
                    ip_src = ifatoia(r.ifa()).ia_addr.get().sin_addr;
                }
            }
            rtfree(rt);
        }
        Some(ia) => ip_src = ia.ia_addr.get().sin_addr,
    }

    // The following happens if the packet was not addressed to us. If we're directly
    // connected use the closest address, otherwise try to use the sourceaddr from the routing
    // table.
    if ip_src.s_addr == INADDR_ANY {
        let mut sin = SockaddrIn {
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_family: AF_INET,
            sin_addr: ip.ip_src,
            ..SockaddrIn::default()
        };

        // keep packet in the original virtual instance
        // SAFETY: a local `sockaddr_in`.
        let rt = unsafe { rtalloc(sintosa(&mut sin), RT_RESOLVE, rtableid) };
        if let Some(r) = rt
            && rtisvalid(Some(r))
            && r.rt_flags.get() & RTF_GATEWAY == 0
        {
            ip_src = ifatoia(r.ifa()).ia_addr.get().sin_addr;
        } else {
            let sourceaddr = rtable_getsource(rtableid, AF_INET);
            if !sourceaddr.is_null() {
                // SAFETY: a preferred source is an interface address, readable.
                let ifa = unsafe { ifa_ifwithaddr(sourceaddr, rtableid) };
                if let Some(ifa) = ifa
                    && ifa
                        .ifa_ifp
                        .get()
                        .is_some_and(|ifp| ifp.if_flags.get() & IFF_UP != 0)
                {
                    // SAFETY: an `AF_INET` source is a `sockaddr_in`.
                    ip_src = unsafe { (*satosin_const(sourceaddr)).sin_addr };
                }
            }
        }
        rtfree(rt);
    }

    // If the above didn't find an ip_src, ip_output() will try and fill it in for us.

    let pfflags = m.m_pkthdr().pf.flags.get();

    m_resethdr(m);
    m.m_pkthdr().ph_rtableid.set(rtableid);
    m.m_pkthdr().pf.flags.set(pfflags & PF_TAG_GENERATED);
    ip.ip_dst = ip.ip_src;
    ip.ip_src = ip_src;
    ip.ip_ttl = MAXTTL;
    mtod_ip_store(m, &ip);

    let want_opts = op.is_some();
    if optlen > 0 {
        // Retrieve any source routing from the incoming packet; add on any record-route or
        // timestamp options.
        if want_opts {
            opts = ip_srcroute(m);
            if opts.is_none() {
                opts = m_gethdr(M_DONTWAIT, MT_HEADER);
                if let Some(o) = opts {
                    o.m_len().set(size_of::<InAddr>() as u32);
                    // SAFETY: a fresh mbuf has room for an address at its data.
                    unsafe { ptr::write_unaligned(mtod::<u32>(o), 0) };
                }
            }
        }
        if let Some(o) = opts {
            // ICMPPRINTFS: not configured.
            // SAFETY: the options follow the fixed header in the first mbuf.
            let cp = unsafe {
                core::slice::from_raw_parts(mtod::<u8>(m).add(size_of::<Ip>()), optlen as usize)
            };
            let mut c = 0usize;
            let mut cnt = optlen;
            while cnt > 0 {
                let opt = cp[c + IPOPT_OPTVAL];
                if opt == IPOPT_EOL {
                    break;
                }
                let len: i32;
                if opt == IPOPT_NOP {
                    len = 1;
                } else {
                    if cnt < (IPOPT_OLEN + 1) as i32 {
                        break;
                    }
                    len = i32::from(cp[c + IPOPT_OLEN]);
                    if len < (IPOPT_OLEN + 1) as i32 || len > cnt {
                        break;
                    }
                }
                // Should check for overflow, but it "can't happen"
                if opt == IPOPT_RR || opt == IPOPT_TS || opt == IPOPT_SECURITY {
                    let at = o.m_len().get() as usize;
                    // SAFETY: an options mbuf has MLEN bytes, more than the 40 bytes of
                    // options a header can carry plus the first hop.
                    unsafe {
                        ptr::copy_nonoverlapping(
                            cp[c..].as_ptr(),
                            mtod::<u8>(o).add(at),
                            len as usize,
                        )
                    };
                    o.m_len().set(o.m_len().get() + len as u32);
                }
                cnt -= len;
                c += len as usize;
            }
            // Terminate & pad, if necessary
            let mut cnt = o.m_len().get() % 4;
            if cnt != 0 {
                while cnt < 4 {
                    let at = o.m_len().get() as usize;
                    // SAFETY: as above.
                    unsafe { *mtod::<u8>(o).add(at) = IPOPT_EOL };
                    o.m_len().set(o.m_len().get() + 1);
                    cnt += 1;
                }
            }
            // ICMPPRINTFS: not configured.
        }
        ip_stripoptions(m);
    }
    m.m_flags().set(m.m_flags().get() & !(M_BCAST | M_MCAST));
    if let Some(op) = op {
        *op = opts;
    }

    Ok(())
}

/// `icmp_send`: sends an icmp packet back to the ip level.
pub fn icmp_send(m: &'static Mbuf, opts: Option<&'static Mbuf>) {
    let ip = mtod_ip(m);
    let mut hlen = i32::from(ip.ip_hl()) << 2;

    IcmpPkt::of(m, hlen as usize).set_icmp_cksum(0);
    m.m_pkthdr().csum_flags.set(M_ICMP_CSUM_OUT);
    // ICMPPRINTFS: not configured.
    // ip_send() cannot handle IP options properly. So in case we have options fill out the IP
    // header here and use ip_send_raw() instead.
    match opts {
        Some(opts) => {
            let m = ip_insertoptions(m, opts, &mut hlen);
            let mut ip = mtod_ip(m);
            ip.set_ip_hl((hlen >> 2) as u8);
            ip.set_ip_v(IPVERSION);
            ip.ip_off &= htons(crate::netinet::ip::IP_DF);
            ip.ip_id = htons(ip_randomid());
            mtod_ip_store(m, &ip);
            ipstat_inc(IpstatCounters::IpsLocalout);
            ip_send_raw(m);
        }
        None => ip_send(m),
    }
}

/// `iptime`: milliseconds since 00:00 GMT, in network order.
pub fn iptime() -> u32 {
    let atv = microtime();
    let t = (atv.tv_sec % (24 * 60 * 60)) as u64 * 1000 + atv.tv_usec as u64 / 1000;
    htonl(t as u32)
}

/// `icmp_sysctl`: the `net.inet.icmp` sysctls.
pub fn icmp_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    // All sysctl names at this level are terminal.
    let [n] = name else {
        return Err(Errno::ENOTDIR);
    };

    match *n {
        ICMPCTL_REDIRTIMEOUT => {
            let savelen = *oldlenp;

            sysctl_vslock(oldp, savelen)?;
            net_lock();
            let error =
                sysctl_int_bounded(oldp, oldlenp, newp, newlen, &ICMP_REDIRTIMEOUT, 0, INT_MAX);
            rt_timer_queue_change(
                &ICMP_REDIRECT_TIMEOUT_Q,
                ICMP_REDIRTIMEOUT.load(Ordering::Relaxed),
            );
            net_unlock();
            sysctl_vsunlock(oldp, savelen);
            error
        }
        ICMPCTL_STATS => icmp_sysctl_icmpstat(oldp, oldlenp, newp),

        _ => sysctl_bounded_arr(&ICMPCTL_VARS, name, oldp, oldlenp, newp, newlen),
    }
}

/// `icmp_sysctl_icmpstat`: `net.inet.icmp.stats`, the counters as a `struct icmpstat`.
fn icmp_sysctl_icmpstat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    const _: () = assert!(size_of::<Icmpstat>() == ICPS_NCOUNTERS * size_of::<u64>());
    let mut bytes = [0u8; ICPS_NCOUNTERS * size_of::<u64>()];
    for (i, c) in ICMPCOUNTERS.iter().enumerate() {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&c.load(Ordering::Relaxed).to_ne_bytes());
    }

    sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
}

/// `icmp_mtudisc_clone`: a host route to `dst` for path MTU discovery (cloned from the
/// network route if needed) with a timer on `ip_mtudisc_timeout_q`; referenced.
pub fn icmp_mtudisc_clone(dst: InAddr, rtableid: u32, ipsec: bool) -> Option<&'static Rtentry> {
    let mut sin = SockaddrIn {
        sin_family: AF_INET,
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_addr: dst,
        ..SockaddrIn::default()
    };

    // SAFETY: a local `sockaddr_in`.
    let mut rt = unsafe { rtalloc(sintosa(&mut sin), RT_RESOLVE, rtableid) };

    let ok = 'bad: {
        let Some(r) = rt else {
            break 'bad false;
        };
        // Check if the route is actually usable
        if !rtisvalid(Some(r)) {
            break 'bad false;
        }
        // IPsec needs the route only for PMTU, it can use reject for that
        if !ipsec && r.rt_flags.get() & (RTF_REJECT | RTF_BLACKHOLE) != 0 {
            break 'bad false;
        }

        // No PMTU for local routes and permanent neighbors, ARP and NDP use the same expire
        // timer as the route.
        if r.rt_flags.get() & RTF_LOCAL != 0
            || (r.rt_flags.get() & RTF_LLINFO != 0 && r.rt_expire().get() == 0)
        {
            break 'bad false;
        }

        // If we didn't get a host route, allocate one
        let mut r = r;
        if r.rt_flags.get() & RTF_HOST == 0 {
            let mut info = RtAddrinfo::new();
            let mut sa_rl = SockaddrRtlabel::default();

            info.rti_ifa = r.rt_ifa.get();
            info.rti_flags = RTF_GATEWAY | RTF_HOST | RTF_DYNAMIC;
            info.rti_info[RTAX_DST] = sintosa(&mut sin);
            info.rti_info[RTAX_GATEWAY] = r.rt_gateway.get();
            info.rti_info[RTAX_LABEL] = rtlabel_id2sa(r.rt_labelid.get(), &mut sa_rl);

            let mut nrt = None;
            // SAFETY: a local `sockaddr_in`, the route's gateway and a local label.
            if unsafe {
                rtrequest(
                    RTM_ADD,
                    &mut info,
                    r.rt_priority.get(),
                    Some(&mut nrt),
                    rtableid,
                )
            }
            .is_err()
            {
                break 'bad false;
            }
            let Some(n) = nrt else {
                break 'bad false;
            };
            n.rt_rmx.assign(&r.rt_rmx);
            rtfree(Some(r));
            r = n;
            rt = Some(n);
            rtm_send(r, RTM_ADD, 0, rtableid);
        }
        if rt_timer_add(r, &IP_MTUDISC_TIMEOUT_Q, rtableid).is_err() {
            break 'bad false;
        }

        true
    };
    if ok {
        return rt;
    }
    // bad:
    rtfree(rt);
    None
}

/// `icmp_mtudisc`: path MTU discovery from a "fragmentation needed" message `icp`.
pub fn icmp_mtudisc(icp: &IcmpPkt, rtableid: u32) {
    let mut mtu = u64::from(ntohs(icp.icmp_nextmtu())); // Why a long? IPv6
    let oip = icp.icmp_ip();

    let Some(rt) = icmp_mtudisc_clone(oip.ip_dst, rtableid, false) else {
        return;
    };

    let Some(ifp) = if_get(rt.rt_ifidx.get()) else {
        rtfree(Some(rt));
        return;
    };

    let rtmtu = rt.rt_mtu().load(Ordering::Relaxed);
    if mtu == 0 {
        mtu = u64::from(ntohs(oip.ip_len));
        // Some 4.2BSD-based routers incorrectly adjust the ip_len
        if mtu > u64::from(rtmtu) && rtmtu != 0 {
            mtu -= u64::from(oip.ip_hl()) << 2;
        }

        // If we still can't guess a value, try the route
        if mtu == 0 {
            mtu = u64::from(rtmtu);

            // If no route mtu, default to the interface mtu
            if mtu == 0 {
                mtu = u64::from(ifp.if_mtu.get());
            }
        }

        for &t in &MTU_TABLE {
            if mtu > u64::from(t) {
                mtu = u64::from(t);
                break;
            }
        }
    }

    // XXX: RTV_MTU is overloaded, since the admin can set it to turn off PMTU for a route,
    // and the kernel can set it to indicate a serious problem with PMTU on a route. We should
    // be using a separate flag for the kernel to indicate this.
    if rt.rt_locks().get() & RTV_MTU == 0 {
        if mtu < 296 || mtu > u64::from(ifp.if_mtu.get()) {
            rt.rt_locks().set(rt.rt_locks().get() | RTV_MTU);
        } else if u64::from(rtmtu) > mtu || rtmtu == 0 {
            let _ = rt.rt_mtu().compare_exchange(
                rtmtu,
                mtu as u32,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }

    if_put(Some(ifp));
    rtfree(Some(rt));
}

/// `icmp_mtudisc_timeout`: a path MTU timer fired: delete the dynamic host route (telling TCP
/// the MTU may be larger again) or forget the learned MTU.
pub fn icmp_mtudisc_timeout(rt: &'static Rtentry, rtableid: u32) {
    net_assert_locked("icmp_mtudisc_timeout");

    let Some(ifp) = if_get(rt.rt_ifidx.get()) else {
        return;
    };

    if rt.rt_flags.get() & (RTF_DYNAMIC | RTF_HOST) == (RTF_DYNAMIC | RTF_HOST) {
        // SAFETY: a route's key in the inet table is a `sockaddr_in`.
        let mut sin = unsafe { ptr::read_unaligned(satosin_const(rt_key(rt))) };

        let _ = rtdeletemsg(rt, ifp, rtableid);

        // Notify TCP layer of increased Path MTU estimate
        let pr = &INETSW[usize::from(IP_PROTOX[IPPROTO_TCP as usize].load(Ordering::Relaxed))];
        if let Some(ctlfunc) = pr.pr_ctlinput {
            // SAFETY: a local `sockaddr_in`; no argument.
            unsafe { ctlfunc(PRC_MTUINC, sintosa(&mut sin), rtableid, ptr::null_mut()) };
        }
    } else if rt.rt_locks().get() & RTV_MTU == 0 {
        rt.rt_mtu().store(0, Ordering::Relaxed);
    }

    if_put(Some(ifp));
}

/// `icmp_ratelimit`: perform rate limit check. `false` if it is okay to send the icmp packet,
/// `true` if the router SHOULD NOT send this icmp packet due to rate limitation. XXX
/// per-destination/type check necessary?
pub fn icmp_ratelimit(_dst: &InAddr, _type: u8, _code: u8) -> bool {
    let icmperrppslim_local = ICMPERRPPSLIM.load(Ordering::Relaxed);
    let pps = ICMPERRPPS.as_ptr();
    // PPS limit
    // SAFETY: the counters live forever and are touched only here, through
    // `ppsratecheck_shared`, which dereferences them only inside `ppsratecheck_mtx`.
    let ok = unsafe {
        ppsratecheck_shared(
            &raw mut (*pps).last,
            &raw mut (*pps).count,
            icmperrppslim_local,
        )
    };
    if !ok {
        return true; // The packet is subject to rate limit
    }
    false // okay to send
}

/// `icmp_do_exthdr`: appends an RFC 4884 extension object (`class`, `ctype`, `buf`) to the
/// ICMP error in `m`; on failure `m` is freed (`ENOBUFS`).
pub fn icmp_do_exthdr(m: &'static Mbuf, class: u16, ctype: u8, buf: &[u8]) -> Result<(), Errno> {
    let mut ip = mtod_ip(m);
    let mut hlen = usize::from(ip.ip_hl()) << 2;
    let icp = IcmpPkt::of(m, hlen);
    let t = icp.icmp_type();
    if t != ICMP_TIMXCEED && t != ICMP_UNREACH && t != ICMP_PARAMPROB {
        // exthdr not supported
        return Ok(());
    }

    if icp.icmp_length() != 0 {
        // exthdr already present, giving up
        return Ok(());
    }

    // the actual offset starts after the common ICMP header
    hlen += ICMP_MINLEN;
    // exthdr must start on a word boundary
    let off = (usize::from(ntohs(ip.ip_len)) - hlen).next_multiple_of(size_of::<u32>());
    // ... and at an offset of ICMP_EXT_OFFSET or bigger
    let off = off.max(ICMP_EXT_OFFSET);
    icp.set_icmp_length((off / size_of::<u32>()) as u8);

    let ieh = IcmpExtHdr {
        ieh_version: ICMP_EXT_HDR_VERSION,
        ..IcmpExtHdr::default()
    };
    let ieo = IcmpExtObjHdr {
        ieo_length: htons((size_of::<IcmpExtObjHdr>() + buf.len()) as u16),
        ieo_cnum: class as u8,
        ieo_ctype: ctype,
    };
    let mut hdr = [0u8; size_of::<IcmpExtHdr>() + size_of::<IcmpExtObjHdr>()];
    hdr[0] = ieh.ieh_version;
    hdr[1] = ieh.ieh_res;
    hdr[2..4].copy_from_slice(&ieh.ieh_cksum.to_ne_bytes());
    hdr[4..6].copy_from_slice(&ieo.ieo_length.to_ne_bytes());
    hdr[6] = ieo.ieo_cnum;
    hdr[7] = ieo.ieo_ctype;

    if m_copyback(m, (hlen + off) as i32, &hdr, M_NOWAIT).is_err()
        || m_copyback(m, (hlen + off + hdr.len()) as i32, buf, M_NOWAIT).is_err()
    {
        m_freem(m);
        return Err(Errno::ENOBUFS);
    }

    // calculate checksum
    let Some((n, noff)) = m_getptr(m, (hlen + off) as i32) else {
        panic(format_args!("icmp_do_exthdr: m_getptr failure"));
    };
    let sum = in4_cksum(n, 0, noff, (hdr.len() + buf.len()) as i32);
    // ieh_cksum, the third and fourth bytes of the extension header.
    let at = noff as usize + offset_of!(IcmpExtHdr, ieh_cksum);
    if at + 2 <= n.m_len().get() as usize {
        // SAFETY: the two bytes are inside `n`'s data.
        unsafe { ptr::write_unaligned(mtod::<u8>(n).add(at).cast::<u16>(), sum) };
    } else {
        let _ = m_copyback(
            m,
            (hlen + off + offset_of!(IcmpExtHdr, ieh_cksum)) as i32,
            &sum.to_ne_bytes(),
            M_NOWAIT,
        );
    }

    ip.ip_len = htons(m.m_pkthdr().len.get() as u16);
    mtod_ip_store(m, &ip);

    Ok(())
}

// Sizes of the C structures.
const _: () = {
    assert!(size_of::<IcmpRaAddr>() == 8);
    assert!(size_of::<IcmpHun>() == 4);
    assert!(size_of::<IcmpDun>() == 20);
    assert!(size_of::<Icmp>() == 28);
    assert!(core::mem::offset_of!(Icmp, icmp_dun) == ICMP_MINLEN);
    assert!(size_of::<IcmpExtHdr>() == 4);
    assert!(size_of::<IcmpExtObjHdr>() == 4);
};

#[cfg(test)]
mod tests;
