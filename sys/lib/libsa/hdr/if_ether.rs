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
//! `<netinet/if_ether.h>` for libsa: the Ethernet header, the Ethernet ARP packet and the
//! lengths `efipxe.c` sizes its buffers with.

use super::if_arp::Arphdr;
use super::in_::net_bytes;

/// `ETHER_ADDR_LEN`: Ethernet address length.
pub const ETHER_ADDR_LEN: usize = 6;
/// `ETHER_TYPE_LEN`: Ethernet type field length.
pub const ETHER_TYPE_LEN: usize = 2;
/// `ETHER_CRC_LEN`: Ethernet CRC length.
pub const ETHER_CRC_LEN: usize = 4;
/// `ETHER_HDR_LEN`: Ethernet header length.
pub const ETHER_HDR_LEN: usize = (ETHER_ADDR_LEN * 2) + ETHER_TYPE_LEN;
/// `ETHER_ALIGN`: the bytes before a received frame that align its payload.
pub const ETHER_ALIGN: usize = 2;

/// `struct ether_header`: the Ethernet header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EtherHeader {
    /// `ether_dhost`: destination address.
    pub ether_dhost: [u8; ETHER_ADDR_LEN],
    /// `ether_shost`: source address.
    pub ether_shost: [u8; ETHER_ADDR_LEN],
    /// `ether_type`: type, network order.
    pub ether_type: u16,
}

/// `struct ether_arp`: an Ethernet ARP packet (RFC 826) for internet addresses. The C's
/// `arp_hrd`... macros for `ea_hdr`'s members are its accessors.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EtherArp {
    /// `ea_hdr`: fixed-size header.
    pub ea_hdr: Arphdr,
    /// `arp_sha`: sender hardware address.
    pub arp_sha: [u8; ETHER_ADDR_LEN],
    /// `arp_spa`: sender protocol address.
    pub arp_spa: [u8; 4],
    /// `arp_tha`: target hardware address.
    pub arp_tha: [u8; ETHER_ADDR_LEN],
    /// `arp_tpa`: target protocol address.
    pub arp_tpa: [u8; 4],
}

impl EtherArp {
    /// `arp_hrd`: format of hardware address, network order.
    pub const fn arp_hrd(&self) -> u16 {
        self.ea_hdr.ar_hrd
    }

    /// `arp_pro`: format of protocol address, network order.
    pub const fn arp_pro(&self) -> u16 {
        self.ea_hdr.ar_pro
    }

    /// `arp_hln`: length of hardware address.
    pub const fn arp_hln(&self) -> u8 {
        self.ea_hdr.ar_hln
    }

    /// `arp_pln`: length of protocol address.
    pub const fn arp_pln(&self) -> u8 {
        self.ea_hdr.ar_pln
    }

    /// `arp_op`: operation, network order.
    pub const fn arp_op(&self) -> u16 {
        self.ea_hdr.ar_op
    }
}

net_bytes!(EtherHeader, EtherArp);

const _: () = assert!(core::mem::size_of::<EtherHeader>() == ETHER_HDR_LEN);
const _: () = assert!(core::mem::size_of::<EtherArp>() == 28);
/* </CODE> */
