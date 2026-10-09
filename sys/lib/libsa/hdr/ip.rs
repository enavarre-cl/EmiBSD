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
//! `<netinet/ip.h>` for libsa: the IP header, naked of options.

use super::in_::{InAddr, net_bytes};

/// `IPVERSION`: the IP version.
pub const IPVERSION: u8 = 4;

/// `struct ip`: structure of an internet header, naked of options. The C's `ip_hl:4` and
/// `ip_v:4` bit-fields share one byte (`ip_v` in the high nibble on both byte orders of the
/// C's `#if`), read and written through the accessors.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip {
    /// `ip_v:4` (the high nibble) and `ip_hl:4` (the low nibble).
    pub ip_vhl: u8,
    /// `ip_tos`: type of service.
    pub ip_tos: u8,
    /// `ip_len`: total length, network order.
    pub ip_len: u16,
    /// `ip_id`: identification, network order.
    pub ip_id: u16,
    /// `ip_off`: fragment offset field, network order.
    pub ip_off: u16,
    /// `ip_ttl`: time to live.
    pub ip_ttl: u8,
    /// `ip_p`: protocol.
    pub ip_p: u8,
    /// `ip_sum`: checksum.
    pub ip_sum: u16,
    /// `ip_src`: source address.
    pub ip_src: InAddr,
    /// `ip_dst`: destination address.
    pub ip_dst: InAddr,
}

impl Ip {
    /// `ip_hl`: header length, in 32-bit words.
    pub const fn ip_hl(&self) -> u8 {
        self.ip_vhl & 0x0f
    }

    /// Sets `ip_hl` (the low four bits of `hl`).
    pub fn set_ip_hl(&mut self, hl: u8) {
        self.ip_vhl = (self.ip_vhl & 0xf0) | (hl & 0x0f);
    }

    /// `ip_v`: version.
    pub const fn ip_v(&self) -> u8 {
        self.ip_vhl >> 4
    }

    /// Sets `ip_v` (the low four bits of `v`).
    pub fn set_ip_v(&mut self, v: u8) {
        self.ip_vhl = (self.ip_vhl & 0x0f) | (v << 4);
    }
}

net_bytes!(Ip);

const _: () = assert!(core::mem::size_of::<Ip>() == 20);
/* </CODE> */
