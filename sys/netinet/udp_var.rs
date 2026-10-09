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
 *	@(#)udp_var.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! UDP kernel structures and variables: `<netinet/udp_var.h>`.
//!
//! Upstream: sys/netinet/udp_var.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The `ui_*` shorthands of `struct udpiphdr` are its members' members (`ui_i.ih_pr`,
//!   `ui_u.uh_sport`, ...).
//! - `enum udpstat_counters` is [`UdpstatCounters`] with the C's values; `udpcounters`
//!   (`struct cpumem *`, `<sys/percpu.h>` not ported) is the static array of atomics
//!   `UDPCOUNTERS` in `netinet/udp_usrreq.rs`, which defines the C's pointer.
//! - `UDPCTL_NAMES` (a `struct ctlname` table) comes with `<sys/sysctl.h>`.
//! - `udbtable`, `udp_usrreqs` and the prototypes are `netinet/udp_usrreq.rs`'s, and so are
//!   the `INET6` `udb6table`, `udp6_usrreqs` and `udp6_ctlinput` (compiled always, as
//!   `netinet6` is); `udp6_output` is `netinet6/udp6_output.rs`'s.

use core::mem::size_of;
use core::sync::atomic::Ordering;

use crate::netinet::ip_var::Ipovly;
use crate::netinet::udp::Udphdr;
use crate::netinet::udp_usrreq::UDPCOUNTERS;

// Names for UDP sysctl objects

/// `UDPCTL_CHECKSUM`: checksum UDP packets.
pub const UDPCTL_CHECKSUM: i32 = 1;
/// `UDPCTL_BADDYNAMIC`: return bad dynamic port bitmap.
pub const UDPCTL_BADDYNAMIC: i32 = 2;
/// `UDPCTL_RECVSPACE`: receive buffer space.
pub const UDPCTL_RECVSPACE: i32 = 3;
/// `UDPCTL_SENDSPACE`: send buffer space.
pub const UDPCTL_SENDSPACE: i32 = 4;
/// `UDPCTL_STATS`: UDP statistics.
pub const UDPCTL_STATS: i32 = 5;
/// `UDPCTL_ROOTONLY`: root only port bitmap.
pub const UDPCTL_ROOTONLY: i32 = 6;
/// `UDPCTL_MAXID`.
pub const UDPCTL_MAXID: i32 = 7;

/// `struct udpiphdr`: UDP kernel structures and variables.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Udpiphdr {
    /// `ui_i`: overlaid ip structure.
    pub ui_i: Ipovly,
    /// `ui_u`: udp header.
    pub ui_u: Udphdr,
}

/// `struct udpstat`: UDP statistics, as `sysctl(2)` returns them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Udpstat {
    // input statistics:
    /// Total input packets.
    pub udps_ipackets: u64,
    /// Packet shorter than header.
    pub udps_hdrops: u64,
    /// Checksum error.
    pub udps_badsum: u64,
    /// No checksum.
    pub udps_nosum: u64,
    /// Data length larger than packet.
    pub udps_badlen: u64,
    /// No socket on port.
    pub udps_noport: u64,
    /// Of above, arrived as broadcast.
    pub udps_noportbcast: u64,
    /// Dropped for lack of ipsec.
    pub udps_nosec: u64,
    /// Not delivered, input socket full.
    pub udps_fullsock: u64,
    /// Input packets missing pcb hash.
    pub udps_pcbhashmiss: u64,
    /// Input software-csummed packets.
    pub udps_inswcsum: u64,
    // output statistics:
    /// Total output packets.
    pub udps_opackets: u64,
    /// Output software-csummed packets.
    pub udps_outswcsum: u64,
}

/// `enum udpstat_counters`.
#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UdpstatCounters {
    // input statistics:
    /// Total input packets.
    UdpsIpackets,
    /// Packet shorter than header.
    UdpsHdrops,
    /// Checksum error.
    UdpsBadsum,
    /// No checksum.
    UdpsNosum,
    /// Data length larger than packet.
    UdpsBadlen,
    /// No socket on port.
    UdpsNoport,
    /// Of above, arrived as broadcast.
    UdpsNoportbcast,
    /// Dropped for lack of ipsec.
    UdpsNosec,
    /// Not delivered, input socket full.
    UdpsFullsock,
    /// Input packets missing pcb hash.
    UdpsPcbhashmiss,
    /// Input software-csummed packets.
    UdpsInswcsum,
    // output statistics:
    /// Total output packets.
    UdpsOpackets,
    /// Output software-csummed packets.
    UdpsOutswcsum,

    /// `udps_ncounters`.
    UdpsNcounters,
}

/// `UDPS_NCOUNTERS`: the number of counters.
pub const UDPS_NCOUNTERS: usize = UdpstatCounters::UdpsNcounters as usize;

/// `udpstat_inc(c)`.
pub fn udpstat_inc(c: UdpstatCounters) {
    UDPCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

// The wire layout, and the counters are the structure's words.
const _: () = assert!(size_of::<Udpiphdr>() == 28);
const _: () = assert!(size_of::<Udpstat>() == UDPS_NCOUNTERS * size_of::<u64>());
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest::{assert_complete, assert_defines};

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet/udp_var.h");
        let ctl = assert_defines!(defs;
            UDPCTL_CHECKSUM, UDPCTL_BADDYNAMIC, UDPCTL_RECVSPACE, UDPCTL_SENDSPACE,
            UDPCTL_STATS, UDPCTL_ROOTONLY, UDPCTL_MAXID);
        assert_complete(&defs, "UDPCTL_", &[&ctl[..], &["UDPCTL_NAMES"]].concat());
    }
}
/* </TESTS> */
