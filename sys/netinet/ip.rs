/*	$OpenBSD: ip.h,v 1.22 2025/12/19 13:58:53 tb Exp $	*/
/*	$NetBSD: ip.h,v 1.9 1995/05/15 01:22:44 cgd Exp $	*/
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
 *	@(#)ip.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! Definitions for internet protocol version 4, per RFC 791, September 1981:
//! `<netinet/ip.h>`.
//!
//! Upstream: sys/netinet/ip.h @ 3ce1f3f79392
//!
//! [`Ip`] is the header as it is on the wire: multi-byte fields hold network-order values
//! (`ntohs(ip.ip_len)`), as in C. The constants are host-order values (`IP_DF` is compared with
//! `ntohs(ip.ip_off) & IP_DF`).
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - The C bit-fields `ip_hl:4, ip_v:4` (and `ipt_flg:4, ipt_oflw:4` in `struct
//!   ip_timestamp`) are one byte, `ip_vhl` (`ipt_oflwflg`), with accessor methods
//!   (`ip_hl()`, `set_ip_v()`, ...). The C declares the fields in opposite orders for little-
//!   and big-endian machines so that the version is always the high nibble of the first byte
//!   on the wire; the byte with explicit shifts is that layout on every machine.
//! - The union `ipt_timestamp` keeps the C's one-element arrays (`ipt_time[1]`, `ipt_ta[1]`):
//!   the option is variable in size and the arrays only name its first entry.
//! - `IPOPT_COPIED(o)`, `IPOPT_CLASS(o)`, `IPOPT_NUMBER(o)` are `const fn`s.

use core::mem::size_of;

use crate::netinet::in_::InAddr;

/// `IPVERSION`.
pub const IPVERSION: u8 = 4;

/// Reserved fragment flag.
pub const IP_RF: u16 = 0x8000;
/// Dont fragment flag.
pub const IP_DF: u16 = 0x4000;
/// More fragments flag.
pub const IP_MF: u16 = 0x2000;
/// Mask for fragmenting bits.
pub const IP_OFFMASK: u16 = 0x1fff;

/// Maximum packet size.
pub const IP_MAXPACKET: usize = 65535;

// Definitions for IP type of service (ip_tos)

/// `IPTOS_LOWDELAY`.
pub const IPTOS_LOWDELAY: u8 = 0x10;
/// `IPTOS_THROUGHPUT`.
pub const IPTOS_THROUGHPUT: u8 = 0x08;
/// `IPTOS_RELIABILITY`.
pub const IPTOS_RELIABILITY: u8 = 0x04;
/// Congestion experienced (ECN RFC 3168 obsoletes RFC 2481, and this will be deprecated
/// soon).
pub const IPTOS_CE: u8 = 0x01;
/// ECN-capable transport (deprecated as `IPTOS_CE`).
pub const IPTOS_ECT: u8 = 0x02;

// Definitions for IP precedence (also in ip_tos) (hopefully unused)

/// `IPTOS_PREC_NETCONTROL`.
pub const IPTOS_PREC_NETCONTROL: u8 = 0xe0;
/// `IPTOS_PREC_INTERNETCONTROL`.
pub const IPTOS_PREC_INTERNETCONTROL: u8 = 0xc0;
/// `IPTOS_PREC_CRITIC_ECP`.
pub const IPTOS_PREC_CRITIC_ECP: u8 = 0xa0;
/// `IPTOS_PREC_FLASHOVERRIDE`.
pub const IPTOS_PREC_FLASHOVERRIDE: u8 = 0x80;
/// `IPTOS_PREC_FLASH`.
pub const IPTOS_PREC_FLASH: u8 = 0x60;
/// `IPTOS_PREC_IMMEDIATE`.
pub const IPTOS_PREC_IMMEDIATE: u8 = 0x40;
/// `IPTOS_PREC_PRIORITY`.
pub const IPTOS_PREC_PRIORITY: u8 = 0x20;
/// `IPTOS_PREC_ROUTINE`.
pub const IPTOS_PREC_ROUTINE: u8 = 0x00;

// Definitions for DiffServ Codepoints as per RFCs 2474, 3246, 4594, 5865, 8622. These are the
// 6 most significant bits as they appear on the wire, so the two least significant bits must
// be zero.

/// `IPTOS_DSCP_CS0`.
pub const IPTOS_DSCP_CS0: u8 = 0x00;
/// `IPTOS_DSCP_LE`.
pub const IPTOS_DSCP_LE: u8 = 0x04;
/// `IPTOS_DSCP_CS1`.
pub const IPTOS_DSCP_CS1: u8 = 0x20;
/// `IPTOS_DSCP_AF11`.
pub const IPTOS_DSCP_AF11: u8 = 0x28;
/// `IPTOS_DSCP_AF12`.
pub const IPTOS_DSCP_AF12: u8 = 0x30;
/// `IPTOS_DSCP_AF13`.
pub const IPTOS_DSCP_AF13: u8 = 0x38;
/// `IPTOS_DSCP_CS2`.
pub const IPTOS_DSCP_CS2: u8 = 0x40;
/// `IPTOS_DSCP_AF21`.
pub const IPTOS_DSCP_AF21: u8 = 0x48;
/// `IPTOS_DSCP_AF22`.
pub const IPTOS_DSCP_AF22: u8 = 0x50;
/// `IPTOS_DSCP_AF23`.
pub const IPTOS_DSCP_AF23: u8 = 0x58;
/// `IPTOS_DSCP_CS3`.
pub const IPTOS_DSCP_CS3: u8 = 0x60;
/// `IPTOS_DSCP_AF31`.
pub const IPTOS_DSCP_AF31: u8 = 0x68;
/// `IPTOS_DSCP_AF32`.
pub const IPTOS_DSCP_AF32: u8 = 0x70;
/// `IPTOS_DSCP_AF33`.
pub const IPTOS_DSCP_AF33: u8 = 0x78;
/// `IPTOS_DSCP_CS4`.
pub const IPTOS_DSCP_CS4: u8 = 0x80;
/// `IPTOS_DSCP_AF41`.
pub const IPTOS_DSCP_AF41: u8 = 0x88;
/// `IPTOS_DSCP_AF42`.
pub const IPTOS_DSCP_AF42: u8 = 0x90;
/// `IPTOS_DSCP_AF43`.
pub const IPTOS_DSCP_AF43: u8 = 0x98;
/// `IPTOS_DSCP_CS5`.
pub const IPTOS_DSCP_CS5: u8 = 0xa0;
/// `IPTOS_DSCP_VA`.
pub const IPTOS_DSCP_VA: u8 = 0xb0;
/// `IPTOS_DSCP_EF`.
pub const IPTOS_DSCP_EF: u8 = 0xb8;
/// `IPTOS_DSCP_CS6`.
pub const IPTOS_DSCP_CS6: u8 = 0xc0;
/// `IPTOS_DSCP_CS7`.
pub const IPTOS_DSCP_CS7: u8 = 0xe0;

// ECN (Explicit Congestion Notification) codepoints in RFC 3168 mapped to the lower 2 bits of
// the TOS field.

/// Not-ECT.
pub const IPTOS_ECN_NOTECT: u8 = 0x00;
/// ECN-capable transport (1).
pub const IPTOS_ECN_ECT1: u8 = 0x01;
/// ECN-capable transport (0).
pub const IPTOS_ECN_ECT0: u8 = 0x02;
/// Congestion experienced.
pub const IPTOS_ECN_CE: u8 = 0x03;
/// ECN field mask.
pub const IPTOS_ECN_MASK: u8 = 0x03;

// Definitions for options.

/// `IPOPT_CONTROL`.
pub const IPOPT_CONTROL: u8 = 0x00;
/// `IPOPT_RESERVED1`.
pub const IPOPT_RESERVED1: u8 = 0x20;
/// `IPOPT_DEBMEAS`.
pub const IPOPT_DEBMEAS: u8 = 0x40;
/// `IPOPT_RESERVED2`.
pub const IPOPT_RESERVED2: u8 = 0x60;

/// End of option list.
pub const IPOPT_EOL: u8 = 0;
/// No operation.
pub const IPOPT_NOP: u8 = 1;

/// Record packet route.
pub const IPOPT_RR: u8 = 7;
/// Timestamp.
pub const IPOPT_TS: u8 = 68;
/// Provide s,c,h,tcc.
pub const IPOPT_SECURITY: u8 = 130;
/// Loose source route.
pub const IPOPT_LSRR: u8 = 131;
/// Satnet id.
pub const IPOPT_SATID: u8 = 136;
/// Strict source route.
pub const IPOPT_SSRR: u8 = 137;
/// Router alert.
pub const IPOPT_RA: u8 = 148;

// Offsets to fields in options other than EOL and NOP.

/// Option ID.
pub const IPOPT_OPTVAL: usize = 0;
/// Option length.
pub const IPOPT_OLEN: usize = 1;
/// Offset within option.
pub const IPOPT_OFFSET: usize = 2;
/// Min value of above.
pub const IPOPT_MINOFF: u8 = 4;

// Flag bits for ipt_flg

/// Timestamps only.
pub const IPOPT_TS_TSONLY: u8 = 0;
/// Timestamps and addresses.
pub const IPOPT_TS_TSANDADDR: u8 = 1;
/// Specified modules only.
pub const IPOPT_TS_PRESPEC: u8 = 3;

// Bits for security (not byte swapped)

/// `IPOPT_SECUR_UNCLASS`.
pub const IPOPT_SECUR_UNCLASS: u16 = 0x0000;
/// `IPOPT_SECUR_CONFID`.
pub const IPOPT_SECUR_CONFID: u16 = 0xf135;
/// `IPOPT_SECUR_EFTO`.
pub const IPOPT_SECUR_EFTO: u16 = 0x789a;
/// `IPOPT_SECUR_MMMM`.
pub const IPOPT_SECUR_MMMM: u16 = 0xbc4d;
/// `IPOPT_SECUR_RESTR`.
pub const IPOPT_SECUR_RESTR: u16 = 0xaf13;
/// `IPOPT_SECUR_SECRET`.
pub const IPOPT_SECUR_SECRET: u16 = 0xd788;
/// `IPOPT_SECUR_TOPSECRET`.
pub const IPOPT_SECUR_TOPSECRET: u16 = 0x6bc5;

// Internet implementation parameters.

/// Maximum time to live (seconds).
pub const MAXTTL: u8 = 255;
/// Default ttl, from RFC 1340.
pub const IPDEFTTL: u8 = 64;
/// Time to live for frags, slowhz.
pub const IPFRAGTTL: u8 = 60;
/// Subtracted when forwarding.
pub const IPTTLDEC: u8 = 1;

/// Default maximum segment size.
pub const IP_MSS: usize = 576;

/// Maximum length for IP protocol queues.
pub const IPQ_MAXLEN: usize = 2048;

/// `struct ip`: structure of an internet header, naked of options.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip {
    /// `ip_v:4` (the high nibble) and `ip_hl:4` (the low nibble): version and header length
    /// in 32-bit words.
    pub ip_vhl: u8,
    /// Type of service.
    pub ip_tos: u8,
    /// Total length, network order.
    pub ip_len: u16,
    /// Identification, network order.
    pub ip_id: u16,
    /// Fragment offset field (with `IP_RF`, `IP_DF`, `IP_MF`), network order.
    pub ip_off: u16,
    /// Time to live.
    pub ip_ttl: u8,
    /// Protocol.
    pub ip_p: u8,
    /// Checksum.
    pub ip_sum: u16,
    /// Source address.
    pub ip_src: InAddr,
    /// Destination address.
    pub ip_dst: InAddr,
}

impl Ip {
    /// `ip_hl`: header length, in 32-bit words.
    pub const fn ip_hl(&self) -> u8 {
        self.ip_vhl & 0x0f
    }

    /// Sets `ip_hl` (the low four bits of `hl`).
    pub fn set_ip_hl(&mut self, hl: u8) {
        self.ip_vhl = (self.ip_vhl & 0xf0) | (hl & 0x0f);
    }

    /// `ip_v`: version.
    pub const fn ip_v(&self) -> u8 {
        self.ip_vhl >> 4
    }

    /// Sets `ip_v` (the low four bits of `v`).
    pub fn set_ip_v(&mut self, v: u8) {
        self.ip_vhl = (self.ip_vhl & 0x0f) | (v << 4);
    }
}

/// `struct ipt_ta`: one address and timestamp of a timestamp option.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IptTa {
    /// The address.
    pub ipt_addr: InAddr,
    /// The timestamp.
    pub ipt_time: u32,
}

/// `union ipt_timestamp`: the entries of a timestamp option.
#[repr(C)]
#[derive(Clone, Copy)]
pub union IptTimestamp {
    /// Timestamps only.
    pub ipt_time: [u32; 1],
    /// Addresses and timestamps.
    pub ipt_ta: [IptTa; 1],
}

/// `struct ip_timestamp`: time stamp option structure.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IpTimestamp {
    /// `IPOPT_TS`.
    pub ipt_code: u8,
    /// Size of structure (variable).
    pub ipt_len: u8,
    /// Index of current entry.
    pub ipt_ptr: u8,
    /// `ipt_oflw:4` (the high nibble, overflow counter) and `ipt_flg:4` (the low nibble,
    /// flags, see `IPOPT_TS_*`).
    pub ipt_oflwflg: u8,
    /// The entries.
    pub ipt_timestamp: IptTimestamp,
}

impl IpTimestamp {
    /// `ipt_flg`: flags, see `IPOPT_TS_*`.
    pub const fn ipt_flg(&self) -> u8 {
        self.ipt_oflwflg & 0x0f
    }

    /// Sets `ipt_flg`.
    pub fn set_ipt_flg(&mut self, flg: u8) {
        self.ipt_oflwflg = (self.ipt_oflwflg & 0xf0) | (flg & 0x0f);
    }

    /// `ipt_oflw`: overflow counter.
    pub const fn ipt_oflw(&self) -> u8 {
        self.ipt_oflwflg >> 4
    }

    /// Sets `ipt_oflw`.
    pub fn set_ipt_oflw(&mut self, oflw: u8) {
        self.ipt_oflwflg = (self.ipt_oflwflg & 0x0f) | (oflw << 4);
    }
}

/// `struct ippseudo`: the real IPv4 pseudo header, used for computing the TCP and UDP
/// checksums. For the Internet checksum, `struct ipovly` can be used instead; for stronger
/// checksums, the real thing must be used.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ippseudo {
    /// Source internet address.
    pub ippseudo_src: InAddr,
    /// Destination internet address.
    pub ippseudo_dst: InAddr,
    /// Pad, must be zero.
    pub ippseudo_pad: u8,
    /// Protocol.
    pub ippseudo_p: u8,
    /// Protocol length.
    pub ippseudo_len: u16,
}

/// `IPOPT_COPIED(o)`: the option's copied flag.
pub const fn ipopt_copied(o: u8) -> u8 {
    o & 0x80
}

/// `IPOPT_CLASS(o)`: the option's class.
pub const fn ipopt_class(o: u8) -> u8 {
    o & 0x60
}

/// `IPOPT_NUMBER(o)`: the option's number.
pub const fn ipopt_number(o: u8) -> u8 {
    o & 0x1f
}

// Sizes of the C structures.
const _: () = {
    assert!(size_of::<Ip>() == 20);
    assert!(size_of::<IptTa>() == 8);
    assert!(size_of::<IpTimestamp>() == 12);
    assert!(size_of::<Ippseudo>() == 12);
};

#[cfg(test)]
mod tests;
