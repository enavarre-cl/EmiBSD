/*	$OpenBSD: mld6.h,v 1.2 2010/03/22 21:29:22 jsg Exp $	*/
/*	$FreeBSD: mld6.h,v 1.1 2009/04/29 11:31:23 bms Exp $	*/
/*	$OpenBSD: mld6.c,v 1.76 2026/09/17 15:56:59 bluhm Exp $	*/
/*	$KAME: mld6.c,v 1.26 2001/02/16 14:50:35 itojun Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 2009 Bruce Simpson.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. The name of the author may not be used to endorse or promote
 *    products derived from this software without specific prior written
 *    permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */

/*
 * Copyright (C) 1998 WIDE Project.
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
 * Copyright (c) 1988 Stephen Deering.
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Stephen Deering of Stanford University.
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
 *	@(#)igmp.c	8.1 (Berkeley) 7/19/93
 */
/* </LICENSES> */

//! Multicast Listener Discovery: the MLDv2 message formats of `<netinet6/mld6.h>`, and
//! `netinet6/mld6.c`, the MLDv1 host side (joining and leaving groups, answering queries).
//!
//! Upstream: sys/netinet6/mld6.h @ 3ce1f3f79392
//! Upstream: sys/netinet6/mld6.c @ 3ce1f3f79392
//!
//! `mld6.c` shares the module with the header. See `<netinet/icmp6.h>`
//! (`netinet/icmp6.rs`) for `struct mld_hdr` (MLDv1 query and host report format).
//!
//! ## Deviations
//! - The `MLD_MRC_*`, `MLD_QQIC_*`, `MLD_QRESV`, `MLD_SFLAG` and `MLD_QRV` macros are
//!   `const fn`s; `mld_numrecs` is [`Mldv2Report::mld_numrecs`]. The structures are
//!   `#[repr(C)]` without padding (the C's are `__packed`; sizes asserted below).

use crate::net::if_var::Ifnet;
use crate::netinet6::in6_var::In6Multi;
use crate::netinet6::mld6_var::Mld6Pktinfo;
use crate::sys::mbuf::Mbuf;
use core::mem::size_of;
use core::sync::atomic::AtomicI32;

use crate::netinet::icmp6::Icmp6Hdr;
use crate::netinet6::in6::In6Addr;
use crate::sys::endian::ntohs;

/// Minimum length of any MLD protocol message.
pub const MLD_MINLEN: usize = size_of::<Icmp6Hdr>();

/// `MLD_V2_REPORT_MAXRECS`.
pub const MLD_V2_REPORT_MAXRECS: u32 = 65535;

// MLDv2 report modes.

/// Don't send a record.
pub const MLD_DO_NOTHING: u8 = 0;
/// MODE_IN.
pub const MLD_MODE_IS_INCLUDE: u8 = 1;
/// MODE_EX.
pub const MLD_MODE_IS_EXCLUDE: u8 = 2;
/// TO_IN.
pub const MLD_CHANGE_TO_INCLUDE_MODE: u8 = 3;
/// TO_EX.
pub const MLD_CHANGE_TO_EXCLUDE_MODE: u8 = 4;
/// ALLOW_NEW.
pub const MLD_ALLOW_NEW_SOURCES: u8 = 5;
/// BLOCK_OLD.
pub const MLD_BLOCK_OLD_SOURCES: u8 = 6;

// MLDv2 query types.

/// `MLD_V2_GENERAL_QUERY`.
pub const MLD_V2_GENERAL_QUERY: i32 = 1;
/// `MLD_V2_GROUP_QUERY`.
pub const MLD_V2_GROUP_QUERY: i32 = 2;
/// `MLD_V2_GROUP_SOURCE_QUERY`.
pub const MLD_V2_GROUP_SOURCE_QUERY: i32 = 3;

/// Maximum report interval for MLDv1 host membership reports (in seconds).
pub const MLD_V1_MAX_RI: u32 = 10;

/// `MLD_TIMER_SCALE`: the MLD code field specifies time in milliseconds.
pub const MLD_TIMER_SCALE: u32 = 1000;

/// `struct mldv2_query`: MLD v2 query format (followed by 1..numsrc source addresses).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Mldv2Query {
    /// ICMPv6 header.
    pub mld_icmp6_hdr: Icmp6Hdr,
    /// Address being queried.
    pub mld_addr: In6Addr,
    /// Reserved/suppress/robustness.
    pub mld_misc: u8,
    /// Querier's query interval.
    pub mld_qqi: u8,
    /// Number of sources.
    pub mld_numsrc: u16,
}

/// `MLD_V2_QUERY_MINLEN`.
pub const MLD_V2_QUERY_MINLEN: usize = size_of::<Mldv2Query>();

/// `struct mldv2_report`: MLDv2 host membership report header (`mld_type`:
/// `MLDV2_LISTENER_REPORT`), followed by 1..numgrps records.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Mldv2Report {
    /// `mld_icmp6_hdr`.
    pub mld_icmp6_hdr: Icmp6Hdr,
}

impl Mldv2Report {
    /// `mld_numrecs` (`mld_icmp6_hdr.icmp6_data16[1]`), overlaid on the ICMPv6 header.
    pub fn mld_numrecs(&self) -> u16 {
        self.mld_icmp6_hdr.icmp6_data16(1)
    }

    /// `mld_numrecs = v`.
    pub fn set_mld_numrecs(&mut self, v: u16) {
        self.mld_icmp6_hdr.set_icmp6_data16(1, v);
    }
}

/// `struct mldv2_record`: a report record (followed by 1..numsrc source addresses).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mldv2Record {
    /// Record type.
    pub mr_type: u8,
    /// Length of auxiliary data.
    pub mr_datalen: u8,
    /// Number of sources.
    pub mr_numsrc: u16,
    /// Address being reported.
    pub mr_addr: In6Addr,
}

/// \[a\] `mld6_timers_are_running`: shortcut for fast timer.
pub static MLD6_TIMERS_ARE_RUNNING: AtomicI32 = AtomicI32::new(0);

/// `MLD_MRC_EXP(x)`: the exponent of a network-order Maximum Response Code.
pub const fn mld_mrc_exp(x: u16) -> u16 {
    (ntohs(x) >> 12) & 0x0007
}

/// `MLD_MRC_MANT(x)`: the mantissa of a network-order Maximum Response Code.
pub const fn mld_mrc_mant(x: u16) -> u16 {
    ntohs(x) & 0x0fff
}

/// `MLD_QQIC_EXP(x)`.
pub const fn mld_qqic_exp(x: u8) -> u8 {
    (x >> 4) & 0x07
}

/// `MLD_QQIC_MANT(x)`.
pub const fn mld_qqic_mant(x: u8) -> u8 {
    x & 0x0f
}

/// `MLD_QRESV(x)`.
pub const fn mld_qresv(x: u8) -> u8 {
    (x >> 4) & 0x0f
}

/// `MLD_SFLAG(x)`.
pub const fn mld_sflag(x: u8) -> u8 {
    (x >> 3) & 0x01
}

/// `MLD_QRV(x)`.
pub const fn mld_qrv(x: u8) -> u8 {
    x & 0x07
}

/// `mld6_init`: initializes the MLD timers and the router alert option.
pub fn mld6_init() {
    let _ = crate::unported!("mld6_init: placeholder");
}

/// `mld6_start_listening`: starts listening to group `in6m` on `ifp`; the report to send
/// (if any) is stored in `pkt` for `mld6_sendpkt` after the locks are released.
pub fn mld6_start_listening(in6m: &In6Multi, ifp: &Ifnet, pkt: &mut Mld6Pktinfo) {
    let _ = (in6m, ifp, pkt);
    let _ = crate::unported!("mld6_start_listening: placeholder");
}

/// `mld6_stop_listening`: stops listening to group `in6m` on `ifp`; the done message to
/// send (if any) is stored in `pkt`.
pub fn mld6_stop_listening(in6m: &In6Multi, ifp: &Ifnet, pkt: &mut Mld6Pktinfo) {
    let _ = (in6m, ifp, pkt);
    let _ = crate::unported!("mld6_stop_listening: placeholder");
}

/// `mld6_input`: a received MLD message at offset `off` of `m`.
pub fn mld6_input(m: &'static Mbuf, off: i32) {
    let _ = (m, off);
    let _ = crate::unported!("mld6_input: placeholder");
}

/// `mld6_fasttimo`: the MLD fast timeout: sends the delayed reports.
pub fn mld6_fasttimo() {
    let _ = crate::unported!("mld6_fasttimo: placeholder");
}

/// `mld6_sendpkt`: sends the MLD message `pkt` describes.
pub fn mld6_sendpkt(pkt: &Mld6Pktinfo) {
    let _ = pkt;
    let _ = crate::unported!("mld6_sendpkt: placeholder");
}

// The sizes of the C's `__packed` structures.
const _: () = {
    assert!(size_of::<Mldv2Query>() == 28);
    assert!(size_of::<Mldv2Report>() == 8);
    assert!(size_of::<Mldv2Record>() == 20);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_fields() {
        use crate::sys::endian::htons;
        // Maximum Response Code 0x7123: exponent 7, mantissa 0x123.
        assert_eq!(mld_mrc_exp(htons(0x7123)), 7);
        assert_eq!(mld_mrc_mant(htons(0x7123)), 0x123);
        // misc: resv 0xa, S 1, QRV 2.
        let misc = 0xa0 | 0x08 | 0x02;
        assert_eq!(
            (mld_qresv(misc), mld_sflag(misc), mld_qrv(misc)),
            (0xa, 1, 2)
        );
        assert_eq!((mld_qqic_exp(0x9c), mld_qqic_mant(0x9c)), (1, 0xc));
        assert_eq!(MLD_MINLEN, 8);
        assert_eq!(MLD_V2_QUERY_MINLEN, 28);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet6/mld6.h");
        crate::reftest::assert_defines!(defs;
            MLD_V2_REPORT_MAXRECS, MLD_DO_NOTHING, MLD_MODE_IS_INCLUDE, MLD_MODE_IS_EXCLUDE,
            MLD_CHANGE_TO_INCLUDE_MODE, MLD_CHANGE_TO_EXCLUDE_MODE, MLD_ALLOW_NEW_SOURCES,
            MLD_BLOCK_OLD_SOURCES, MLD_V2_GENERAL_QUERY, MLD_V2_GROUP_QUERY,
            MLD_V2_GROUP_SOURCE_QUERY, MLD_V1_MAX_RI, MLD_TIMER_SCALE);
    }
}
