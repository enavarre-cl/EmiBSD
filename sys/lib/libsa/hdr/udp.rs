//! `<netinet/udp.h>` for libsa: the UDP header.

use super::in_::net_bytes;

/// `struct udphdr`: the UDP header; every member is in network order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Udphdr {
    /// `uh_sport`: source port.
    pub uh_sport: u16,
    /// `uh_dport`: destination port.
    pub uh_dport: u16,
    /// `uh_ulen`: udp length.
    pub uh_ulen: u16,
    /// `uh_sum`: udp checksum.
    pub uh_sum: u16,
}

net_bytes!(Udphdr);

const _: () = assert!(core::mem::size_of::<Udphdr>() == 8);
