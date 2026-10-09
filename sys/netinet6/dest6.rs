/*	$KAME: dest6.c,v 1.25 2001/02/22 01:39:16 itojun Exp $	*/
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
//! The IPv6 destination options header: `netinet6/dest6.c`.
//!
//! Upstream: sys/netinet6/dest6.c @ 3ce1f3f79392
//!
//! `dest6_input` validates the length of the header (made contiguous with
//! `ip6_exthdr_get`), walks the options and hands every option other than Pad1 and PadN
//! to `ip6_unknown_opt`, which acts on the option's action bits.
//!
//! ## Deviations
//! - `ip6_unknown_opt` returns a `bool` here (`true`: skip the option), not the C's option
//!   length or -1; for a skipped option the length is read from the option itself
//!   (`*(opt + 1)`), which is the value the C function returns.

use crate::net::if_var::Netstack;
use crate::netinet::in_::IPPROTO_DONE;
use crate::netinet::ip6::{IP6OPT_MINLEN, IP6OPT_PAD1, IP6OPT_PADN, Ip6Dest, ip6_exthdr_get};
use crate::netinet6::ip6_input::ip6_unknown_opt;
use crate::netinet6::ip6_var::{Ip6statCounters, ip6stat_inc};
use crate::sys::mbuf::{Mbuf, m_freemp};
use core::mem::size_of;

/// `dest6_input`: the destination options header's `pr_input`: processes the options
/// and returns the next header.
pub fn dest6_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    _proto: i32,
    _af: i32,
    _ns: Option<&Netstack>,
) -> i32 {
    const HDRLEN: i32 = size_of::<Ip6Dest>() as i32;
    let mut off = *offp;

    // validation of the length of the header
    let Some(dstopts) = ip6_exthdr_get(mp, off, HDRLEN) else {
        return IPPROTO_DONE;
    };
    // SAFETY: ip6_exthdr_get made the two header bytes readable at `dstopts`.
    let hdr_len = unsafe { *dstopts.add(1) };
    let mut dstoptlen = (i32::from(hdr_len) + 1) << 3;

    let Some(dstopts) = ip6_exthdr_get(mp, off, dstoptlen) else {
        return IPPROTO_DONE;
    };
    off += dstoptlen;
    dstoptlen -= HDRLEN;
    // The options start right after the 2-byte header; `pos` is the offset of the current
    // one from `dstopts`. All of the header's bytes are contiguous and readable.
    let mut pos = HDRLEN;

    // search header for all options.
    while dstoptlen > 0 {
        // SAFETY: `pos` is below the header's length (`dstoptlen > 0` bytes remain from it).
        let opt = unsafe { dstopts.add(pos as usize) };
        // SAFETY: as above.
        let ty = unsafe { *opt };
        let mut len = 0;
        if ty != IP6OPT_PAD1 {
            if dstoptlen < IP6OPT_MINLEN as i32 {
                ip6stat_inc(Ip6statCounters::Ip6sToosmall);
                m_freemp(mp);
                return IPPROTO_DONE;
            }
            // SAFETY: at least IP6OPT_MINLEN (2) bytes remain, so the length byte is
            // inside the header.
            len = i32::from(unsafe { *opt.add(1) });
            if len + 2 > dstoptlen {
                ip6stat_inc(Ip6statCounters::Ip6sToosmall);
                m_freemp(mp);
                return IPPROTO_DONE;
            }
        }

        let optlen = match ty {
            IP6OPT_PAD1 => 1,
            IP6OPT_PADN => len + 2,
            // unknown option
            _ => {
                // SAFETY: `opt` points at the option (type and length bytes validated above)
                // inside the contiguous header in `*mp`'s mbuf chain.
                if !unsafe { ip6_unknown_opt(mp, opt, *offp + pos) } {
                    return IPPROTO_DONE;
                }
                len + 2
            }
        };
        dstoptlen -= optlen;
        pos += optlen;
    }

    *offp = off;
    // SAFETY: the first byte of the header (`ip6d_nxt`) is readable, and `*mp` is intact
    // (a skipped option leaves the chain alone).
    i32::from(unsafe { *dstopts })
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `dest6_input` on synthetic packets: the length validation and the
    // Pad1/PadN walk (options that need `ip6_unknown_opt` are that function's tests).

    use super::*;
    use crate::kern::uipc_mbuf::m_freem;
    use crate::net::if_::tests::test_packet;
    use crate::netinet::icmp6::Icmp6statCounters;
    use crate::netinet::ip_input::tests::setup;
    use crate::netinet::ip6::{IPV6_VERSION, Ip6Hdr};
    use crate::netinet6::icmp6::ICMP6COUNTERS;
    use crate::netinet6::in6::In6Addr;
    use crate::netinet6::ip6_input::IP6COUNTERS;
    use crate::netinet6::nd6::tests::{OURS6, PEER6};
    use core::sync::atomic::Ordering;
    use std::vec::Vec;

    const IPPROTO_UDP: u8 = 17;

    /// An IPv6 header followed by `ext`.
    fn packet(ext: &[u8]) -> &'static Mbuf {
        packet_to(OURS6, ext)
    }

    /// An IPv6 header from `PEER6` to `dst` followed by `ext`.
    fn packet_to(dst: In6Addr, ext: &[u8]) -> &'static Mbuf {
        let mut ip6 = Ip6Hdr::zeroed();
        ip6.set_ip6_vfc(IPV6_VERSION);
        ip6.ip6_nxt = 60;
        ip6.ip6_hlim = 64;
        ip6.ip6_src = PEER6;
        ip6.ip6_dst = dst;
        // SAFETY: `Ip6Hdr` is 40 bytes of integers without padding.
        let hdr: [u8; 40] = unsafe { core::mem::transmute(ip6) };
        let mut b: Vec<u8> = hdr.to_vec();
        b.extend_from_slice(ext);
        test_packet(&b)
    }

    fn toosmall() -> u64 {
        IP6COUNTERS[Ip6statCounters::Ip6sToosmall as usize].load(Ordering::Relaxed)
    }

    #[test]
    fn pad_options_are_skipped_and_the_next_header_returned() {
        let _g = setup();
        // 16 bytes: the header, two Pad1, a PadN with 4 bytes of data, a PadN with 2.
        #[rustfmt::skip]
    let ext = [
        IPPROTO_UDP, 1,
        0, 0,
        1, 4, 0, 0, 0, 0,
        1, 2, 0, 0,
        0, 0,
    ];
        let mut mp = Some(packet(&ext));
        let mut off = 40;
        assert_eq!(
            dest6_input(&mut mp, &mut off, 60, 10, None),
            i32::from(IPPROTO_UDP)
        );
        assert_eq!(off, 40 + 16);
        m_freem(mp.take().expect("packet kept"));
    }

    #[test]
    fn an_option_running_past_the_header_is_dropped() {
        let _g = setup();
        let before = toosmall();
        // The PadN claims 6 bytes of data, but only 4 are left in the 8-byte header.
        let ext = [IPPROTO_UDP, 0, 1, 6, 0, 0, 0, 0];
        let mut mp = Some(packet(&ext));
        let mut off = 40;
        assert_eq!(dest6_input(&mut mp, &mut off, 60, 10, None), IPPROTO_DONE);
        assert!(mp.is_none());
        assert_eq!(off, 40, "the offset is left alone");
        assert_eq!(toosmall(), before + 1);

        // A lone option type byte: no room for its length.
        let before = toosmall();
        let ext = [IPPROTO_UDP, 0, 0, 0, 0, 0, 0, 1];
        let mut mp = Some(packet(&ext));
        assert_eq!(dest6_input(&mut mp, &mut off, 60, 10, None), IPPROTO_DONE);
        assert!(mp.is_none());
        assert_eq!(toosmall(), before + 1);
    }

    #[test]
    fn a_header_longer_than_the_packet_is_dropped() {
        let _g = setup();
        // The length field claims 24 bytes, the packet has 8.
        let ext = [IPPROTO_UDP, 2, 0, 0, 0, 0, 0, 0];
        let mut mp = Some(packet(&ext));
        let mut off = 40;
        assert_eq!(dest6_input(&mut mp, &mut off, 60, 10, None), IPPROTO_DONE);
        assert!(mp.is_none(), "ip6_exthdr_get freed the chain");
    }

    fn counter(c: Icmp6statCounters) -> u64 {
        ICMP6COUNTERS[c as usize].load(Ordering::Relaxed)
    }

    fn badoptions() -> u64 {
        IP6COUNTERS[Ip6statCounters::Ip6sBadoptions as usize].load(Ordering::Relaxed)
    }

    /// Runs `dest6_input` on a header with the one option `opt` (type, length 4, 4 bytes of data)
    /// in a packet to `dst`: what it returns, and whether the packet is still there.
    fn with_option(dst: In6Addr, opt: u8) -> (i32, bool) {
        let ext = [IPPROTO_UDP, 0, opt, 4, 0, 0, 0, 0];
        let mut mp = Some(packet_to(dst, &ext));
        let mut off = 40;
        let r = dest6_input(&mut mp, &mut off, 60, 10, None);
        let kept = mp.is_some();
        if let Some(m) = mp {
            m_freem(m);
        }
        (r, kept)
    }

    #[test]
    fn unknown_options_follow_their_action_bits() {
        let _g = setup();
        let mcast = crate::netinet6::in6::IN6ADDR_LINKLOCAL_ALLNODES;
        let errors = || counter(Icmp6statCounters::Icp6sError);
        let option_errors = || counter(Icmp6statCounters::Icp6sOparamprobOption);

        // 00: skip over the option.
        assert_eq!(with_option(OURS6, 0x1e), (i32::from(IPPROTO_UDP), true));

        // 01: discard the packet, quietly.
        let (bad, err) = (badoptions(), errors());
        assert_eq!(with_option(OURS6, 0x5e), (IPPROTO_DONE, false));
        assert_eq!((badoptions(), errors()), (bad, err));

        // 10: discard and send a parameter problem, even for a multicast destination.
        for dst in [OURS6, mcast] {
            let (bad, err, opt) = (badoptions(), errors(), option_errors());
            assert_eq!(with_option(dst, 0x9e), (IPPROTO_DONE, false));
            assert_eq!(badoptions(), bad + 1);
            assert_eq!((errors(), option_errors()), (err + 1, opt + 1));
        }

        // 11: the parameter problem only for a unicast destination.
        let (bad, err) = (badoptions(), errors());
        assert_eq!(with_option(OURS6, 0xde), (IPPROTO_DONE, false));
        assert_eq!((badoptions(), errors()), (bad + 1, err + 1));
        let (bad, err) = (badoptions(), errors());
        assert_eq!(with_option(mcast, 0xde), (IPPROTO_DONE, false));
        assert_eq!((badoptions(), errors()), (bad + 1, err));
    }
}
/* </TESTS> */
