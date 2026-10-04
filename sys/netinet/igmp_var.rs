/*	$OpenBSD: igmp_var.h,v 1.17 2026/02/26 00:53:18 bluhm Exp $	*/
/*	$NetBSD: igmp_var.h,v 1.9 1996/02/13 23:41:31 christos Exp $	*/
/* <LICENSES> */
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
 *	@(#)igmp_var.h	8.1 (Berkeley) 7/19/93
 */
/* </LICENSES> */

//! `<netinet/igmp_var.h>`: Internet Group Management Protocol (IGMP), implementation-specific
//! definitions: the statistics, the sysctl names and the report a membership change asks
//! for. Written by Steve Deering, Stanford, May 1988. Modified by Rosen Sharma, Stanford,
//! Aug 1994. Modified by Bill Fenner, Xerox PARC, Feb 1995. MULTICAST 1.3.
//!
//! Upstream: sys/netinet/igmp_var.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct igmpstat`'s `u_long`s are `u64` (LP64), the words `net.inet.igmp.stats` copies.
//! - `igmpcounters` (`struct cpumem *`) is the static array of atomics `IGMPCOUNTERS` in
//!   `netinet/igmp.rs`; `igmpstat_inc` bumps it.
//! - `struct igmp_pktinfo` has no `ipi_list`: the list of reports `igmp_fasttimo` collects
//!   (`struct igmp_pktlist`) is a `Vec` of them (`netinet/igmp.rs`).
//! - `IGMP_RANDOM_DELAY(X)` is [`igmp_random_delay`]; `IGMPCTL_NAMES` is userland's.

use core::sync::atomic::Ordering;

use crate::dev::rnd::arc4random_uniform;
use crate::netinet::igmp::IGMPCOUNTERS;
use crate::netinet::in_::InAddr;

/// `IGMPCTL_STATS`: IGMP statistics.
pub const IGMPCTL_STATS: i32 = 1;
/// `IGMPCTL_MAXID`.
pub const IGMPCTL_MAXID: i32 = 2;

/// `struct igmpstat`: `net.inet.igmp.stats`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Igmpstat {
    /// `igps_rcv_total`: total IGMP messages received.
    pub igps_rcv_total: u64,
    /// `igps_rcv_tooshort`: received with too few bytes.
    pub igps_rcv_tooshort: u64,
    /// `igps_rcv_badsum`: received with bad checksum.
    pub igps_rcv_badsum: u64,
    /// `igps_rcv_queries`: received membership queries.
    pub igps_rcv_queries: u64,
    /// `igps_rcv_badqueries`: received invalid queries.
    pub igps_rcv_badqueries: u64,
    /// `igps_rcv_reports`: received membership reports.
    pub igps_rcv_reports: u64,
    /// `igps_rcv_badreports`: received invalid reports.
    pub igps_rcv_badreports: u64,
    /// `igps_rcv_ourreports`: received reports for our groups.
    pub igps_rcv_ourreports: u64,
    /// `igps_snd_reports`: sent membership reports.
    pub igps_snd_reports: u64,
}

/// `enum igmpstat_counters`: the indices of `IGMPCOUNTERS`.
#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IgmpstatCounters {
    /// `igps_rcv_total`: total IGMP messages received.
    IgpsRcvTotal,
    /// `igps_rcv_tooshort`: received with too few bytes.
    IgpsRcvTooshort,
    /// `igps_rcv_badsum`: received with bad checksum.
    IgpsRcvBadsum,
    /// `igps_rcv_queries`: received membership queries.
    IgpsRcvQueries,
    /// `igps_rcv_badqueries`: received invalid queries.
    IgpsRcvBadqueries,
    /// `igps_rcv_reports`: received membership reports.
    IgpsRcvReports,
    /// `igps_rcv_badreports`: received invalid reports.
    IgpsRcvBadreports,
    /// `igps_rcv_ourreports`: received reports for our groups.
    IgpsRcvOurreports,
    /// `igps_snd_reports`: sent membership reports.
    IgpsSndReports,
    /// `igps_ncounters`.
    IgpsNcounters,
}

/// The number of counters (`igps_ncounters`).
pub const IGPS_NCOUNTERS: usize = IgmpstatCounters::IgpsNcounters as usize;

/// `struct igmp_pktinfo`: the report or leave message a membership change asks
/// `igmp_sendpkt` to send (`ipi_ifidx` 0: none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IgmpPktinfo {
    /// `ipi_addr`: the destination and the group.
    pub ipi_addr: InAddr,
    /// `ipi_rdomain`.
    pub ipi_rdomain: u32,
    /// `ipi_ifidx`: the interface the message leaves on.
    pub ipi_ifidx: u32,
    /// `ipi_type`: the IGMP message type.
    pub ipi_type: i32,
}

/// `igmpstat_inc(c)`.
pub fn igmpstat_inc(c: IgmpstatCounters) {
    IGMPCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `IGMP_RANDOM_DELAY(X)`: a random timer value between 1 and `x` (`IGMP_MAX_REPORTING_DELAY`
/// times the countdown frequency).
pub fn igmp_random_delay(x: u32) -> u32 {
    arc4random_uniform(x) + 1
}

// The counters are the structure's words (`CTASSERT` in `igmp_sysctl_igmpstat`).
const _: () = assert!(size_of::<Igmpstat>() == IGPS_NCOUNTERS * size_of::<u64>());
