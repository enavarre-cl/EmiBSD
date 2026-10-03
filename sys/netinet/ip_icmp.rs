/*	$OpenBSD: ip_icmp.h,v 1.33 2025/03/02 21:28:32 bluhm Exp $	*/
/*	$NetBSD: ip_icmp.h,v 1.10 1996/02/13 23:42:28 christos Exp $	*/
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
/* </LICENSES> */

//! Interface Control Message Protocol definitions: `<netinet/ip_icmp.h>`.
//!
//! Upstream: sys/netinet/ip_icmp.h @ 3ce1f3f79392
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
//! Status: `ported`.
//!
//! ## Deviations
//! - `ICMP_V6ADVLEN(p)`, `ICMP_ADVLEN(p)` and `ICMP_INFOTYPE(type)` are `const fn`s.
//! - The prototypes (`icmp_error`, `icmp_input`, `icmp_reflect`, ...) come with
//!   `netinet/ip_icmp.c`.

use core::mem::size_of;

use crate::netinet::in_::InAddr;
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
