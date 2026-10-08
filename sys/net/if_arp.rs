/*	$OpenBSD: if_arp.h,v 1.8 2024/10/15 00:41:40 jsg Exp $	*/
/*	$NetBSD: if_arp.h,v 1.8 1995/03/08 02:56:52 cgd Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1986, 1993
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
 *	@(#)if_arp.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! Address Resolution Protocol: `<net/if_arp.h>`.
//!
//! Upstream: sys/net/if_arp.h @ 3ce1f3f79392
//!
//! See RFC 826 for protocol description. ARP packets are variable in size; the `arphdr`
//! structure defines the fixed-length portion. Protocol type values are the same as those for
//! 10 Mb/s Ethernet. It is followed by the variable-sized fields `ar_sha`, `ar_spa`, `ar_tha`
//! and `ar_tpa` in that order, according to the lengths specified. Field names used
//! correspond to RFC 826. The fields are in network order on the wire; the constants are
//! host-order values to compare after `ntohs`.
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - The variable-sized fields exist only in a `COMMENT_ONLY` block in the C; there is nothing
//!   to port. `struct ether_arp` (`netinet/if_ether.rs`) is the Ethernet/IPv4 layout.

use core::mem::size_of;

/// Ethernet hardware format.
pub const ARPHRD_ETHER: u16 = 1;
/// IEEE 802 hardware format.
pub const ARPHRD_IEEE802: u16 = 6;
/// Frame relay hardware format.
pub const ARPHRD_FRELAY: u16 = 15;
/// IEEE 1394 (FireWire) hardware format.
pub const ARPHRD_IEEE1394: u16 = 24;

/// Request to resolve address.
pub const ARPOP_REQUEST: u16 = 1;
/// Response to previous request.
pub const ARPOP_REPLY: u16 = 2;
/// Request protocol address given hardware.
pub const ARPOP_REVREQUEST: u16 = 3;
/// Response giving protocol address.
pub const ARPOP_REVREPLY: u16 = 4;
/// Request to identify peer.
pub const ARPOP_INVREQUEST: u16 = 8;
/// Response identifying peer.
pub const ARPOP_INVREPLY: u16 = 9;

/// `struct arphdr`: the fixed-length portion of an ARP packet.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Arphdr {
    /// Format of hardware address (`ARPHRD_*`).
    pub ar_hrd: u16,
    /// Format of protocol address.
    pub ar_pro: u16,
    /// Length of hardware address.
    pub ar_hln: u8,
    /// Length of protocol address.
    pub ar_pln: u8,
    /// One of `ARPOP_*`.
    pub ar_op: u16,
}

// Size of the C structure.
const _: () = assert!(size_of::<Arphdr>() == 8);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/net/if_arp.h");
        let ours = crate::reftest::assert_defines!(defs;
            ARPHRD_ETHER, ARPHRD_IEEE802, ARPHRD_FRELAY, ARPHRD_IEEE1394, ARPOP_REQUEST,
            ARPOP_REPLY, ARPOP_REVREQUEST, ARPOP_REVREPLY, ARPOP_INVREQUEST, ARPOP_INVREPLY);
        crate::reftest::assert_complete(&defs, "ARP", &ours);
    }
}
/* </TESTS> */
