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
