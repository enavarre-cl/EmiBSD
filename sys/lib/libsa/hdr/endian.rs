//! `<sys/endian.h>` for libsa: the network byte order conversions the network code uses.

/// `htons(x)`: a 16-bit value in network (big-endian) order.
pub const fn htons(x: u16) -> u16 {
    x.to_be()
}

/// `ntohs(x)`: a 16-bit value from network order.
pub const fn ntohs(x: u16) -> u16 {
    u16::from_be(x)
}

/// `htonl(x)`: a 32-bit value in network order.
pub const fn htonl(x: u32) -> u32 {
    x.to_be()
}

/// `ntohl(x)`: a 32-bit value from network order.
pub const fn ntohl(x: u32) -> u32 {
    u32::from_be(x)
}
