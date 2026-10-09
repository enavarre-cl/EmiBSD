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
 *	@(#)icmp_var.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! Variables related to this implementation of the internet control message protocol:
//! `<netinet/icmp_var.h>`.
//!
//! Upstream: sys/netinet/icmp_var.h @ 3ce1f3f79392
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - `enum icmpstat_counters` is [`IcmpstatCounters`] with the C's values; the C indexes the
//!   two histograms with arithmetic (`icps_outhist + type`), which [`icmpstat_inc_hist`] does.
//! - `icmpcounters` (`struct cpumem *`, `<sys/percpu.h>` not ported) is the static array of
//!   atomics `ICMPCOUNTERS` in `netinet/ip_icmp.rs`, which defines the C's pointer.
//! - `ICMPCTL_NAMES` (a `struct ctlname` table) comes with `<sys/sysctl.h>`.

use core::mem::size_of;
use core::sync::atomic::Ordering;

use crate::netinet::ip_icmp::{ICMP_MAXTYPE, ICMPCOUNTERS};

/// The number of entries of each histogram.
const NHIST: usize = ICMP_MAXTYPE as usize + 1;

// Names for ICMP sysctl objects

/// Allow replies to netmask requests.
pub const ICMPCTL_MASKREPL: i32 = 1;
/// Reply to icmps to broadcast/mcast.
pub const ICMPCTL_BMCASTECHO: i32 = 2;
/// ICMP error pps limitation.
pub const ICMPCTL_ERRPPSLIMIT: i32 = 3;
/// Accept redirects from routers.
pub const ICMPCTL_REDIRACCEPT: i32 = 4;
/// Remove routes added via redirects.
pub const ICMPCTL_REDIRTIMEOUT: i32 = 5;
/// Allow replies to timestamp requests.
pub const ICMPCTL_TSTAMPREPL: i32 = 6;
/// ICMP statistics.
pub const ICMPCTL_STATS: i32 = 7;
/// `ICMPCTL_MAXID`.
pub const ICMPCTL_MAXID: i32 = 8;

/// `struct icmpstat`: ICMP statistics, as `sysctl(2)` returns them.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Icmpstat {
    // statistics related to icmp packets generated
    /// # of calls to `icmp_error`.
    pub icps_error: u64,
    /// No error because rate limiter.
    pub icps_toofreq: u64,
    /// No error because old ip too short.
    pub icps_oldshort: u64,
    /// No error because old was icmp.
    pub icps_oldicmp: u64,
    /// Messages sent, by type.
    pub icps_outhist: [u64; NHIST],
    // statistics related to input messages processed
    /// `icmp_code` out of range.
    pub icps_badcode: u64,
    /// Packet < `ICMP_MINLEN`.
    pub icps_tooshort: u64,
    /// Bad checksum.
    pub icps_checksum: u64,
    /// Calculated bound mismatch.
    pub icps_badlen: u64,
    /// Number of responses.
    pub icps_reflect: u64,
    /// Rejected broadcast icmps.
    pub icps_bmcastecho: u64,
    /// Messages received, by type.
    pub icps_inhist: [u64; NHIST],
}

impl Icmpstat {
    /// An all-zero block.
    pub const fn zeroed() -> Self {
        Self {
            icps_error: 0,
            icps_toofreq: 0,
            icps_oldshort: 0,
            icps_oldicmp: 0,
            icps_outhist: [0; NHIST],
            icps_badcode: 0,
            icps_tooshort: 0,
            icps_checksum: 0,
            icps_badlen: 0,
            icps_reflect: 0,
            icps_bmcastecho: 0,
            icps_inhist: [0; NHIST],
        }
    }
}

/// `enum icmpstat_counters`: the per-CPU ICMP counters, the fields of [`Icmpstat`] in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum IcmpstatCounters {
    /// `icps_error`.
    IcpsError,
    /// `icps_toofreq`.
    IcpsToofreq,
    /// `icps_oldshort`.
    IcpsOldshort,
    /// `icps_oldicmp`.
    IcpsOldicmp,
    /// `icps_outhist`: the first of `ICMP_MAXTYPE + 1`.
    IcpsOuthist,
    /// `icps_badcode`.
    IcpsBadcode = 4 + NHIST,
    /// `icps_tooshort`.
    IcpsTooshort,
    /// `icps_checksum`.
    IcpsChecksum,
    /// `icps_badlen`.
    IcpsBadlen,
    /// `icps_reflect`.
    IcpsReflect,
    /// `icps_bmcastecho`.
    IcpsBmcastecho,
    /// `icps_inhist`: the first of `ICMP_MAXTYPE + 1`.
    IcpsInhist,
    /// `icps_ncounters`.
    IcpsNcounters = 4 + NHIST + 6 + NHIST,
}

/// `icps_ncounters`.
pub const ICPS_NCOUNTERS: usize = IcmpstatCounters::IcpsNcounters as usize;

/// `icmpstat_inc(c)`.
pub fn icmpstat_inc(c: IcmpstatCounters) {
    ICMPCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `icmpstat_inc(icps_outhist + type)` / `icmpstat_inc(icps_inhist + type)`: bumps entry
/// `type_` of histogram `hist`.
pub fn icmpstat_inc_hist(hist: IcmpstatCounters, type_: u8) {
    ICMPCOUNTERS[hist as usize + usize::from(type_)].fetch_add(1, Ordering::Relaxed);
}

// The counters are the structure's words.
const _: () = assert!(size_of::<Icmpstat>() == ICPS_NCOUNTERS * size_of::<u64>());
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_match_the_structure() {
        use core::mem::offset_of;
        let w = |off: usize| off / size_of::<u64>();
        assert_eq!(
            w(offset_of!(Icmpstat, icps_outhist)),
            IcmpstatCounters::IcpsOuthist as usize
        );
        assert_eq!(
            w(offset_of!(Icmpstat, icps_badcode)),
            IcmpstatCounters::IcpsBadcode as usize
        );
        assert_eq!(
            w(offset_of!(Icmpstat, icps_inhist)),
            IcmpstatCounters::IcpsInhist as usize
        );
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet/icmp_var.h");
        let ctl = crate::reftest::assert_defines!(defs;
            ICMPCTL_MASKREPL, ICMPCTL_BMCASTECHO, ICMPCTL_ERRPPSLIMIT, ICMPCTL_REDIRACCEPT,
            ICMPCTL_REDIRTIMEOUT, ICMPCTL_TSTAMPREPL, ICMPCTL_STATS, ICMPCTL_MAXID);
        crate::reftest::assert_complete(
            &defs,
            "ICMPCTL_",
            &[&ctl[..], &["ICMPCTL_NAMES"]].concat(),
        );
    }
}
/* </TESTS> */
