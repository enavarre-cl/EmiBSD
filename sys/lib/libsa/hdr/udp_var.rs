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
