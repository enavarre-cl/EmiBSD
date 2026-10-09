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
//! `<netinet/ip_var.h>` for libsa: the overlay of the IP header the UDP checksum covers.

use super::in_::{InAddr, net_bytes};

/// `struct ipovly`: overlay for ip header used by other protocols (tcp, udp).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ipovly {
    /// `ih_x1`: (unused).
    pub ih_x1: [u8; 9],
    /// `ih_pr`: protocol.
    pub ih_pr: u8,
    /// `ih_len`: protocol length, network order.
    pub ih_len: u16,
    /// `ih_src`: source internet address.
    pub ih_src: InAddr,
    /// `ih_dst`: destination internet address.
    pub ih_dst: InAddr,
}

net_bytes!(Ipovly);

const _: () = assert!(core::mem::size_of::<Ipovly>() == 20);
/* </CODE> */
