/*	$KAME: route6.c,v 1.22 2000/12/03 00:54:00 itojun Exp $	*/
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
/* </LICENSES> */

/* <CODE> */
//! The IPv6 routing header: `netinet6/route6.c`.
//!
//! Upstream: sys/netinet6/route6.c @ 3ce1f3f79392
//!
//! Routing headers of any type (type 0 is treated as unknown, RFC 5095) are skipped when no
//! segments are left, and answered with an ICMPv6 parameter problem otherwise.
//!
//! ## Deviations
//! - The ICMPv6 pointer of the parameter problem is `off + 2` (the offset of `ip6r_type`
//!   from the start of the packet); the C computes it as the difference of two addresses
//!   (`&rh->ip6r_type - ip6`), which is the same value when the header is in the first
//!   mbuf and not meaningful when `m_pulldown` moved it.

use crate::net::if_var::Netstack;
use crate::netinet::icmp6::{ICMP6_PARAM_PROB, ICMP6_PARAMPROB_HEADER};
use crate::netinet::in_::IPPROTO_DONE;
use crate::netinet::ip6::{Ip6Rthdr, ip6_exthdr_get};
use crate::netinet6::icmp6::icmp6_error;
use crate::netinet6::ip6_var::{Ip6statCounters, ip6stat_inc};
use crate::sys::mbuf::Mbuf;
use core::mem::{offset_of, size_of};

/// `route6_input`: the routing header's `pr_input`: skips a header with no segments
/// left, refuses the others; returns the next header. `proto` is unused.
pub fn route6_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    _proto: i32,
    _af: i32,
    _ns: Option<&Netstack>,
) -> i32 {
    let off = *offp;

    let Some(rh) = ip6_exthdr_get(mp, off, size_of::<Ip6Rthdr>() as i32) else {
        ip6stat_inc(Ip6statCounters::Ip6sTooshort);
        return IPPROTO_DONE;
    };
    // SAFETY: ip6_exthdr_get made the `Ip6Rthdr` bytes readable at `rh`; the packet data has
    // no alignment guarantee, so it is read unaligned.
    let rh = unsafe { rh.cast::<Ip6Rthdr>().read_unaligned() };

    // Routing header type 0 is handled like an unrecognised routing type (RFC 5095).
    if rh.ip6r_segleft != 0 {
        ip6stat_inc(Ip6statCounters::Ip6sBadoptions);
        if let Some(m) = mp.take() {
            icmp6_error(
                m,
                ICMP6_PARAM_PROB,
                ICMP6_PARAMPROB_HEADER,
                off + offset_of!(Ip6Rthdr, ip6r_type) as i32,
            );
        }
        return IPPROTO_DONE;
    }

    // Final dst. Just ignore the header.
    *offp += (i32::from(rh.ip6r_len) + 1) << 3;
    i32::from(rh.ip6r_nxt)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `route6_input`: a routing header with no segments left is skipped, one
    // with segments left (any type, type 0 included) gets an ICMPv6 parameter problem.

    use super::*;
    use crate::kern::uipc_mbuf::m_freem;
    use crate::net::if_::tests::test_packet;
    use crate::net::if_var::Ifnet;
    use crate::netinet::icmp6::Icmp6statCounters;
    use crate::netinet::ip6::{IPV6_VERSION, Ip6Hdr};
    use crate::netinet6::ip6_input::IP6COUNTERS;
    use crate::netinet6::nd6::tests::{OURS6, PEER6, icmp6stat, nd6_setup, take_sent};
    use core::sync::atomic::Ordering;
    use std::vec::Vec;

    const IPPROTO_UDP: u8 = 17;
    const IPPROTO_ROUTING: u8 = 43;

    /// A packet from `PEER6` to `OURS6`, a routing header (`nxt`, `len`, `type`, `segleft`)
    /// padded to its length, and 8 bytes of payload.
    fn packet(ifp: &Ifnet, rh: [u8; 4]) -> &'static Mbuf {
        let mut ip6 = Ip6Hdr::zeroed();
        ip6.set_ip6_vfc(IPV6_VERSION);
        ip6.ip6_nxt = IPPROTO_ROUTING;
        ip6.ip6_hlim = 64;
        ip6.ip6_src = PEER6;
        ip6.ip6_dst = OURS6;
        // SAFETY: `Ip6Hdr` is 40 bytes of integers without padding.
        let hdr: [u8; 40] = unsafe { core::mem::transmute(ip6) };
        let mut b: Vec<u8> = hdr.to_vec();
        b.extend_from_slice(&rh);
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&[0xee; 8]);
        let m = test_packet(&b);
        m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
        m
    }

    fn ip6stat(c: Ip6statCounters) -> u64 {
        IP6COUNTERS[c as usize].load(Ordering::Relaxed)
    }

    #[test]
    fn a_header_without_segments_left_is_skipped() {
        let (_g, ifp) = nd6_setup();
        // type 0 and an unknown type alike: nothing left to route, ignore the header.
        for ty in [0, 2, 77] {
            let mut mp = Some(packet(ifp, [IPPROTO_UDP, 0, ty, 0]));
            let mut off = 40;
            assert_eq!(
                route6_input(&mut mp, &mut off, 43, 10, None),
                i32::from(IPPROTO_UDP)
            );
            assert_eq!(off, 48);
            m_freem(mp.take().expect("packet kept"));
        }
    }

    #[test]
    fn a_header_with_segments_left_is_refused() {
        let (_g, ifp) = nd6_setup();
        let _ = take_sent();
        let bad = ip6stat(Ip6statCounters::Ip6sBadoptions);
        let errors = icmp6stat(Icmp6statCounters::Icp6sError);
        let header = icmp6stat(Icmp6statCounters::Icp6sOparamprobHeader);
        let mut mp = Some(packet(ifp, [IPPROTO_UDP, 0, 0, 2]));
        let mut off = 40;
        assert_eq!(route6_input(&mut mp, &mut off, 43, 10, None), IPPROTO_DONE);
        assert!(mp.is_none(), "icmp6_error consumed the packet");
        assert_eq!(off, 40);
        assert_eq!(ip6stat(Ip6statCounters::Ip6sBadoptions), bad + 1);
        // A parameter problem about the routing header was made.
        assert_eq!(icmp6stat(Icmp6statCounters::Icp6sError), errors + 1);
        assert_eq!(
            icmp6stat(Icmp6statCounters::Icp6sOparamprobHeader),
            header + 1
        );
    }
}
/* </TESTS> */
