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
