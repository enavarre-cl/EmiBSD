/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<net/if_arp.h>` for libsa (through `<netinet/if_ether.h>`): the fixed part of an ARP
//! packet and the values `arp.c` checks.

use super::in_::net_bytes;

/// `ARPHRD_ETHER`: ethernet hardware format.
pub const ARPHRD_ETHER: u16 = 1;

/// `ARPOP_REQUEST`: request to resolve address.
pub const ARPOP_REQUEST: u16 = 1;
/// `ARPOP_REPLY`: response to previous request.
pub const ARPOP_REPLY: u16 = 2;

/// `struct arphdr`: the fixed-length portion of an ARP packet.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Arphdr {
    /// `ar_hrd`: format of hardware address, network order.
    pub ar_hrd: u16,
    /// `ar_pro`: format of protocol address, network order.
    pub ar_pro: u16,
    /// `ar_hln`: length of hardware address.
    pub ar_hln: u8,
    /// `ar_pln`: length of protocol address.
    pub ar_pln: u8,
    /// `ar_op`: one of `ARPOP_*`, network order.
    pub ar_op: u16,
}

net_bytes!(Arphdr);

const _: () = assert!(core::mem::size_of::<Arphdr>() == 8);
/* </CODE> */
