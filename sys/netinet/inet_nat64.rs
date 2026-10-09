/*	$vantronix: inet_nat64.c,v 1.2 2011/02/28 14:57:58 mike Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2011 Reyk Floeter <reyk@vantronix.net>
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
//! The NAT64 address mappings of RFC 6052 (`inet_nat64`, IPv6 to IPv4 and an IPv4 address
//! into an IPv6 prefix) and their NAT46 counterparts (`inet_nat46`), which pf's `af-to`
//! uses.
//!
//! Upstream: sys/netinet/inet_nat64.c @ 3ce1f3f79392
//!
//! An address is the 16 bytes of `union inet_nat64_addr` (a `struct pf_addr` in pf), an
//! IPv4 address in the first four. The `u32` words are native-endian readings of the
//! network-order bytes, as the union's `u32` member.
//!
//! ## Deviations
//! - The `void *` arguments are `[u8; 16]` addresses; the C's -1 is `Err(EINVAL)` or
//!   `Err(EAFNOSUPPORT)`, the `errno` the userland build sets (the kernel's only -1).

use crate::sys::errno::Errno;
use crate::sys::socket::{AF_INET, AF_INET6};

/// `union inet_nat64_addr`'s `u32[i]`.
fn u32_at(a: &[u8; 16], i: usize) -> u32 {
    u32::from_ne_bytes([a[4 * i], a[4 * i + 1], a[4 * i + 2], a[4 * i + 3]])
}

/// `u32[i] = v`.
fn set_u32(a: &mut [u8; 16], i: usize, v: u32) {
    a[4 * i..4 * i + 4].copy_from_slice(&v.to_ne_bytes());
}

/// `inet_nat64_mask`: the first `pfxlen` bits (at most 32) of the word `pfx`, the rest of
/// `src` (network order).
pub fn inet_nat64_mask(src: u32, pfx: u32, pfxlen: u8) -> u32 {
    if pfxlen == 0 {
        return src;
    }
    let pfxlen = pfxlen.min(32);
    let mask = (0xffff_ffffu32 << (32 - u32::from(pfxlen))).to_be();
    (src & !mask) | (pfx & mask)
}

/// `inet_nat64`: the address of family `af` that the IPv6 or IPv4 address `src` maps to
/// under the NAT64 prefix `pfx` of `pfxlen` bits.
pub fn inet_nat64(
    af: i32,
    src: &[u8; 16],
    dst: &mut [u8; 16],
    pfx: &[u8; 16],
    pfxlen: u8,
) -> Result<(), Errno> {
    match af {
        a if a == i32::from(AF_INET) => inet_nat64_inet(src, dst, pfx, pfxlen),
        a if a == i32::from(AF_INET6) => inet_nat64_inet6(src, dst, pfx, pfxlen),
        _ => Err(Errno::EAFNOSUPPORT),
    }
}

/// `inet_nat64_inet`: the IPv4 address embedded in the IPv6 address `src` at the position
/// RFC 6052 gives the prefix length (bits 64-71 are reserved and skipped).
pub fn inet_nat64_inet(
    src: &[u8; 16],
    dst: &mut [u8; 16],
    pfx: &[u8; 16],
    pfxlen: u8,
) -> Result<(), Errno> {
    let mut i = match pfxlen {
        32 | 40 | 48 | 56 | 64 | 96 => usize::from(pfxlen / 8),
        _ => {
            if !(96..=128).contains(&pfxlen) {
                return Err(Errno::EINVAL);
            }

            // As an extension, mask out any other bits.
            set_u32(
                dst,
                0,
                inet_nat64_mask(u32_at(src, 3), u32_at(pfx, 3), 32 - (128 - pfxlen)),
            );
            return Ok(());
        }
    };

    // Fill the octets with the source and skip reserved octet 8.
    for d in dst.iter_mut().take(4) {
        if i == 8 {
            i += 1;
        }
        *d = src[i];
        i += 1;
    }

    Ok(())
}

/// `inet_nat64_inet6`: the IPv6 address of the prefix `pfx` with the IPv4 address `src`
/// embedded as RFC 6052 places it for the prefix length.
pub fn inet_nat64_inet6(
    src: &[u8; 16],
    dst: &mut [u8; 16],
    pfx: &[u8; 16],
    pfxlen: u8,
) -> Result<(), Errno> {
    // First copy the prefix octets to the destination.
    *dst = *pfx;

    let mut i = match pfxlen {
        32 | 40 | 48 | 56 | 64 | 96 => usize::from(pfxlen / 8),
        _ => {
            if !(96..=128).contains(&pfxlen) {
                return Err(Errno::EINVAL);
            }

            // As an extension, mask out any other bits.
            set_u32(
                dst,
                3,
                inet_nat64_mask(u32_at(src, 0), u32_at(pfx, 3), 32 - (128 - pfxlen)),
            );
            return Ok(());
        }
    };

    // Octet 8 is reserved and must be set to zero.
    dst[8] = 0;

    // Fill the other octets with the source and skip octet 8.
    for &s in src.iter().take(4) {
        if i == 8 {
            i += 1;
        }
        dst[i] = s;
        i += 1;
    }

    Ok(())
}

/// `inet_nat46`: the address of family `af` that `src` maps to under the NAT46 prefix `pfx`
/// of `pfxlen` bits (at most 32).
pub fn inet_nat46(
    af: i32,
    src: &[u8; 16],
    dst: &mut [u8; 16],
    pfx: &[u8; 16],
    pfxlen: u8,
) -> Result<(), Errno> {
    if pfxlen > 32 {
        return Err(Errno::EINVAL);
    }

    match af {
        a if a == i32::from(AF_INET) => inet_nat46_inet(src, dst, pfx, pfxlen),
        a if a == i32::from(AF_INET6) => inet_nat46_inet6(src, dst, pfx, pfxlen),
        _ => Err(Errno::EAFNOSUPPORT),
    }
}

/// `inet_nat46_inet`: the IPv4 address of the prefix `pfx` with the remaining bits from the
/// last word of the IPv6 address `src`.
pub fn inet_nat46_inet(
    src: &[u8; 16],
    dst: &mut [u8; 16],
    pfx: &[u8; 16],
    pfxlen: u8,
) -> Result<(), Errno> {
    // Set the remaining bits to the source.
    set_u32(
        dst,
        0,
        inet_nat64_mask(u32_at(src, 3), u32_at(pfx, 0), pfxlen),
    );

    Ok(())
}

/// `inet_nat46_inet6`: the IPv6 address `::a.b.c.d` with the IPv4 prefix `pfx` over the
/// IPv4 address `src`.
pub fn inet_nat46_inet6(
    src: &[u8; 16],
    dst: &mut [u8; 16],
    pfx: &[u8; 16],
    pfxlen: u8,
) -> Result<(), Errno> {
    // Set the initial octets to zero.
    set_u32(dst, 0, 0);
    set_u32(dst, 1, 0);
    set_u32(dst, 2, 0);

    // Now set the remaining bits to the source.
    set_u32(
        dst,
        3,
        inet_nat64_mask(u32_at(src, 0), u32_at(pfx, 0), pfxlen),
    );

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    fn v6(s: [u16; 8]) -> [u8; 16] {
        let mut a = [0u8; 16];
        for (i, w) in s.iter().enumerate() {
            a[2 * i..2 * i + 2].copy_from_slice(&w.to_be_bytes());
        }
        a
    }

    #[test]
    fn nat64_well_known_prefix() {
        // 64:ff9b::/96 with 192.0.2.33 (RFC 6052 2.4).
        let pfx = v6([0x64, 0xff9b, 0, 0, 0, 0, 0, 0]);
        let mut v4 = [0u8; 16];
        v4[..4].copy_from_slice(&[192, 0, 2, 33]);
        let mut d6 = [0u8; 16];
        inet_nat64(i32::from(AF_INET6), &v4, &mut d6, &pfx, 96).unwrap();
        assert_eq!(d6, v6([0x64, 0xff9b, 0, 0, 0, 0, 0xc000, 0x0221]));

        let mut back = [0u8; 16];
        inet_nat64(i32::from(AF_INET), &d6, &mut back, &pfx, 96).unwrap();
        assert_eq!(&back[..4], &[192, 0, 2, 33]);
    }

    #[test]
    fn nat64_prefix_64_skips_octet_8() {
        // 2001:db8:122:344::/64 with 192.0.2.33 is 2001:db8:122:344:c0:2:2100:: (RFC 6052).
        let pfx = v6([0x2001, 0xdb8, 0x122, 0x344, 0, 0, 0, 0]);
        let mut v4 = [0u8; 16];
        v4[..4].copy_from_slice(&[192, 0, 2, 33]);
        let mut d6 = [0u8; 16];
        inet_nat64_inet6(&v4, &mut d6, &pfx, 64).unwrap();
        assert_eq!(d6, v6([0x2001, 0xdb8, 0x122, 0x344, 0xc0, 0x2, 0x2100, 0]));
        assert!(inet_nat64(i32::from(AF_INET), &d6, &mut [0; 16], &pfx, 33).is_err());
    }

    #[test]
    fn nat46_mask() {
        let mut d = [0u8; 16];
        let src = v6([0, 0, 0, 0, 0, 0, 0x0a0b, 0x0c0d]);
        let mut pfx = [0u8; 16];
        pfx[..4].copy_from_slice(&[192, 168, 0, 0]);
        inet_nat46(i32::from(AF_INET), &src, &mut d, &pfx, 16).unwrap();
        assert_eq!(&d[..4], &[192, 168, 12, 13]);
        assert!(inet_nat46(i32::from(AF_INET), &src, &mut d, &pfx, 33).is_err());
    }
}
/* </TESTS> */
