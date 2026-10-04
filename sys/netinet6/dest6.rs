/*	$OpenBSD: dest6.c,v 1.25 2026/05/26 20:27:27 bluhm Exp $	*/
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

#[cfg(test)]
mod tests;
