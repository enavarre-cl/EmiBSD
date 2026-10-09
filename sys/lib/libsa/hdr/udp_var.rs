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
//! `<netinet/udp_var.h>` for libsa: the UDP header with the IP overlay in front, which the
//! UDP checksum covers.

use super::in_::net_bytes;
use super::ip_var::Ipovly;
use super::udp::Udphdr;

/// `struct udpiphdr`: the overlaid IP structure and the UDP header. The C's `ui_x1`,
/// `ui_len`... macros are the members of `ui_i` and `ui_u`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Udpiphdr {
    /// `ui_i`: overlaid ip structure.
    pub ui_i: Ipovly,
    /// `ui_u`: udp header.
    pub ui_u: Udphdr,
}

net_bytes!(Udpiphdr);

const _: () = assert!(core::mem::size_of::<Udpiphdr>() == 28);
/* </CODE> */
