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
//! `<netinet/in.h>` for libsa: `struct in_addr`, the addresses and protocol numbers the
//! network code uses. The module is `in_` as in the kernel (`in` is a Rust keyword).
//!
//! The network headers here are `#[repr(C)]` integers and byte arrays without padding, read
//! from and written to the unaligned bytes of a packet with `net_bytes!`. Their members
//! hold what the C's members hold: values in network order, as they are in memory.

use super::endian::htonl;

/// Implements `from_bytes`/`as_bytes` for the network headers, which are `#[repr(C)]`
/// integers and byte arrays without padding, read from and written to packet buffers.
macro_rules! net_bytes {
    ($($t:ty),*) => {$(
        impl $t {
            /// The header in the first bytes of `b`, or `None` if `b` is too short.
            pub fn from_bytes(b: &[u8]) -> Option<Self> {
                if b.len() < core::mem::size_of::<Self>() {
                    return None;
                }
                // SAFETY: `b` holds a whole header (checked above), the read is unaligned,
                // and the header is `#[repr(C)]` integers and byte arrays, valid for any
                // byte pattern.
                Some(unsafe { b.as_ptr().cast::<Self>().read_unaligned() })
            }

            /// The header's bytes, as they are on the wire.
            pub fn as_bytes(&self) -> &[u8] {
                // SAFETY: the header is `#[repr(C)]` integers and byte arrays laid out with
                // no padding (each file's compile-time checks pin the sizes), so all its
                // bytes are initialised; the slice borrows `self`.
                unsafe {
                    core::slice::from_raw_parts(
                        (self as *const Self).cast::<u8>(),
                        core::mem::size_of::<Self>(),
                    )
                }
            }

            /// Writes the header to the first bytes of `b`, which must hold it.
            pub fn write_to(&self, b: &mut [u8]) {
                let n = core::mem::size_of::<Self>();
                b[..n].copy_from_slice(self.as_bytes());
            }
        }
    )*};
}
pub(crate) use net_bytes;

/// `IPPROTO_UDP`: user datagram protocol.
pub const IPPROTO_UDP: u8 = 17;

/// `INADDR_ANY`: 0.0.0.0, network order.
pub const INADDR_ANY: u32 = htonl(0x0000_0000);
/// `INADDR_BROADCAST`: 255.255.255.255, network order.
pub const INADDR_BROADCAST: u32 = htonl(0xffff_ffff);
/// `INADDR_NONE`: -1 return (outside the kernel, where `net.h` makes `__IPADDR` `htonl`).
pub const INADDR_NONE: u32 = htonl(0xffff_ffff);

/// `IP_TTL`: the socket option number, which libsa's `sendudp` puts in `ip_ttl`.
pub const IP_TTL: u8 = 4;

/// `struct in_addr`: an internet address.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InAddr {
    /// `s_addr`: the address, network order.
    pub s_addr: u32,
}

net_bytes!(InAddr);

const _: () = assert!(core::mem::size_of::<InAddr>() == 4);
/* </CODE> */
