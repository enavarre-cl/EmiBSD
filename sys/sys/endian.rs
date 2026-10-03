/*	$OpenBSD: endian.h,v 1.25 2014/12/21 04:49:00 guenther Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1997 Niklas Hallqvist.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/*	$OpenBSD: _endian.h,v 1.8 2018/01/11 23:13:37 dlg Exp $	*/
/*-
 * Copyright (c) 1997 Niklas Hallqvist.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! Byte order: `<sys/endian.h>` together with `<sys/_endian.h>`.
//!
//! Upstream: sys/sys/endian.h @ 3ce1f3f79392
//! Upstream: sys/sys/_endian.h @ 3ce1f3f79392
//!
//! The network headers (`netinet/in.h`, `netinet/ip.h`, ...) keep wire fields in network
//! order, as the C does; these are the conversions between that order and the host's.
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - `<sys/_endian.h>` is folded in. Its double-underscore names (`__swap16`, `__htobe32`,
//!   `__bemtoh16`, ...) exist only so that headers can use them without exporting the public
//!   ones; Rust modules give that hygiene, so only the public names are defined.
//! - `_BYTE_ORDER` comes from the target (`cfg!(target_endian)`) instead of
//!   `<machine/endian.h>`: both archs are little-endian, and the compiler already knows.
//! - `__swapNNgen`/`__swapNNmd` (the constant-folding and the machine-specific swap) are both
//!   `swap_bytes`, which the compiler folds for constants and lowers to `bswap`/`rev`.
//! - The memory forms (`bemtoh16`, `htobem32`, ...) take a byte array of the right size
//!   instead of a pointer, so the read or the store cannot go past the field.
//! - `swap16_multi(v, n)` takes `&mut [u16]`; the length travels with the slice.
//! - The in-place `NTOHL(x)`, `NTOHS(x)`, `HTONL(x)`, `HTONS(x)` ("ancient stuff") are not
//!   defined: Rust writes `x = ntohl(x)`.

/// Byte order of little-endian machines.
pub const _LITTLE_ENDIAN: i32 = 1234;
/// Byte order of big-endian machines.
pub const _BIG_ENDIAN: i32 = 4321;
/// Byte order of PDP machines.
pub const _PDP_ENDIAN: i32 = 3412;
/// This machine's byte order.
pub const _BYTE_ORDER: i32 = if cfg!(target_endian = "little") {
    _LITTLE_ENDIAN
} else {
    _BIG_ENDIAN
};

/// Index of the high 32-bit word of a 64-bit quantity seen as two words.
pub const _QUAD_HIGHWORD: usize = if cfg!(target_endian = "little") { 1 } else { 0 };
/// Index of the low 32-bit word of a 64-bit quantity seen as two words.
pub const _QUAD_LOWWORD: usize = if cfg!(target_endian = "little") { 0 } else { 1 };

/// Public name of [`_LITTLE_ENDIAN`].
pub const LITTLE_ENDIAN: i32 = _LITTLE_ENDIAN;
/// Public name of [`_BIG_ENDIAN`].
pub const BIG_ENDIAN: i32 = _BIG_ENDIAN;
/// Public name of [`_PDP_ENDIAN`].
pub const PDP_ENDIAN: i32 = _PDP_ENDIAN;
/// Public name of [`_BYTE_ORDER`].
pub const BYTE_ORDER: i32 = _BYTE_ORDER;

/// `swap16(x)`: reverses the two bytes of `x`.
#[inline]
pub const fn swap16(x: u16) -> u16 {
    x.swap_bytes()
}

/// `swap32(x)`: reverses the four bytes of `x`.
#[inline]
pub const fn swap32(x: u32) -> u32 {
    x.swap_bytes()
}

/// `swap64(x)`: reverses the eight bytes of `x`.
#[inline]
pub const fn swap64(x: u64) -> u64 {
    x.swap_bytes()
}

/// `swap16_multi(v, n)`: swaps every element of `v` in place.
pub fn swap16_multi(v: &mut [u16]) {
    for x in v {
        *x = swap16(*x);
    }
}

/// `htobe16(x)`: host to big-endian.
#[inline]
pub const fn htobe16(x: u16) -> u16 {
    x.to_be()
}

/// `htobe32(x)`: host to big-endian.
#[inline]
pub const fn htobe32(x: u32) -> u32 {
    x.to_be()
}

/// `htobe64(x)`: host to big-endian.
#[inline]
pub const fn htobe64(x: u64) -> u64 {
    x.to_be()
}

/// `htole16(x)`: host to little-endian.
#[inline]
pub const fn htole16(x: u16) -> u16 {
    x.to_le()
}

/// `htole32(x)`: host to little-endian.
#[inline]
pub const fn htole32(x: u32) -> u32 {
    x.to_le()
}

/// `htole64(x)`: host to little-endian.
#[inline]
pub const fn htole64(x: u64) -> u64 {
    x.to_le()
}

/// `be16toh(x)`: big-endian to host (the POSIX name).
#[inline]
pub const fn be16toh(x: u16) -> u16 {
    u16::from_be(x)
}

/// `be32toh(x)`: big-endian to host (the POSIX name).
#[inline]
pub const fn be32toh(x: u32) -> u32 {
    u32::from_be(x)
}

/// `be64toh(x)`: big-endian to host (the POSIX name).
#[inline]
pub const fn be64toh(x: u64) -> u64 {
    u64::from_be(x)
}

/// `le16toh(x)`: little-endian to host (the POSIX name).
#[inline]
pub const fn le16toh(x: u16) -> u16 {
    u16::from_le(x)
}

/// `le32toh(x)`: little-endian to host (the POSIX name).
#[inline]
pub const fn le32toh(x: u32) -> u32 {
    u32::from_le(x)
}

/// `le64toh(x)`: little-endian to host (the POSIX name).
#[inline]
pub const fn le64toh(x: u64) -> u64 {
    u64::from_le(x)
}

/// `betoh16(x)`: big-endian to host (the original BSD name).
#[inline]
pub const fn betoh16(x: u16) -> u16 {
    be16toh(x)
}

/// `betoh32(x)`: big-endian to host (the original BSD name).
#[inline]
pub const fn betoh32(x: u32) -> u32 {
    be32toh(x)
}

/// `betoh64(x)`: big-endian to host (the original BSD name).
#[inline]
pub const fn betoh64(x: u64) -> u64 {
    be64toh(x)
}

/// `letoh16(x)`: little-endian to host (the original BSD name).
#[inline]
pub const fn letoh16(x: u16) -> u16 {
    le16toh(x)
}

/// `letoh32(x)`: little-endian to host (the original BSD name).
#[inline]
pub const fn letoh32(x: u32) -> u32 {
    le32toh(x)
}

/// `letoh64(x)`: little-endian to host (the original BSD name).
#[inline]
pub const fn letoh64(x: u64) -> u64 {
    le64toh(x)
}

/// `htons(x)`: host to network (big-endian) order, 16 bits.
#[inline]
pub const fn htons(x: u16) -> u16 {
    htobe16(x)
}

/// `htonl(x)`: host to network (big-endian) order, 32 bits.
#[inline]
pub const fn htonl(x: u32) -> u32 {
    htobe32(x)
}

/// `ntohs(x)`: network (big-endian) to host order, 16 bits.
#[inline]
pub const fn ntohs(x: u16) -> u16 {
    be16toh(x)
}

/// `ntohl(x)`: network (big-endian) to host order, 32 bits.
#[inline]
pub const fn ntohl(x: u32) -> u32 {
    be32toh(x)
}

/// `bemtoh16(p)`: reads a big-endian 16-bit value from memory.
#[inline]
pub const fn bemtoh16(p: &[u8; 2]) -> u16 {
    u16::from_be_bytes(*p)
}

/// `bemtoh32(p)`: reads a big-endian 32-bit value from memory.
#[inline]
pub const fn bemtoh32(p: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*p)
}

/// `bemtoh64(p)`: reads a big-endian 64-bit value from memory.
#[inline]
pub const fn bemtoh64(p: &[u8; 8]) -> u64 {
    u64::from_be_bytes(*p)
}

/// `htobem16(p, v)`: stores `v` in memory as big-endian.
#[inline]
pub fn htobem16(p: &mut [u8; 2], v: u16) {
    *p = v.to_be_bytes();
}

/// `htobem32(p, v)`: stores `v` in memory as big-endian.
#[inline]
pub fn htobem32(p: &mut [u8; 4], v: u32) {
    *p = v.to_be_bytes();
}

/// `htobem64(p, v)`: stores `v` in memory as big-endian.
#[inline]
pub fn htobem64(p: &mut [u8; 8], v: u64) {
    *p = v.to_be_bytes();
}

/// `lemtoh16(p)`: reads a little-endian 16-bit value from memory.
#[inline]
pub const fn lemtoh16(p: &[u8; 2]) -> u16 {
    u16::from_le_bytes(*p)
}

/// `lemtoh32(p)`: reads a little-endian 32-bit value from memory.
#[inline]
pub const fn lemtoh32(p: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*p)
}

/// `lemtoh64(p)`: reads a little-endian 64-bit value from memory.
#[inline]
pub const fn lemtoh64(p: &[u8; 8]) -> u64 {
    u64::from_le_bytes(*p)
}

/// `htolem16(p, v)`: stores `v` in memory as little-endian.
#[inline]
pub fn htolem16(p: &mut [u8; 2], v: u16) {
    *p = v.to_le_bytes();
}

/// `htolem32(p, v)`: stores `v` in memory as little-endian.
#[inline]
pub fn htolem32(p: &mut [u8; 4], v: u32) {
    *p = v.to_le_bytes();
}

/// `htolem64(p, v)`: stores `v` in memory as little-endian.
#[inline]
pub fn htolem64(p: &mut [u8; 8], v: u64) {
    *p = v.to_le_bytes();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_order_is_big_endian() {
        assert_eq!(htons(0x1234).to_ne_bytes(), [0x12, 0x34]);
        assert_eq!(htonl(0x0a00_0202).to_ne_bytes(), [10, 0, 2, 2]);
        assert_eq!(ntohs(htons(0xbeef)), 0xbeef);
        assert_eq!(ntohl(htonl(0xdead_beef)), 0xdead_beef);
        assert_eq!(swap64(0x0102_0304_0506_0708), 0x0807_0605_0403_0201);
        assert_eq!(htole32(0x1234_5678).to_ne_bytes(), [0x78, 0x56, 0x34, 0x12]);
    }

    #[test]
    fn memory_forms() {
        let mut b = [0u8; 4];
        htobem32(&mut b, 0x0a00_0202);
        assert_eq!(b, [10, 0, 2, 2]);
        assert_eq!(bemtoh32(&b), 0x0a00_0202);
        htolem32(&mut b, 0x0a00_0202);
        assert_eq!(lemtoh32(&b), 0x0a00_0202);
        let mut v = [0x0102u16, 0x0304];
        swap16_multi(&mut v);
        assert_eq!(v, [0x0201, 0x0403]);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/_endian.h");
        assert_eq!(crate::reftest::int(&defs, "_LITTLE_ENDIAN"), Some(1234));
        assert_eq!(crate::reftest::int(&defs, "_BIG_ENDIAN"), Some(4321));
        assert_eq!(crate::reftest::int(&defs, "_PDP_ENDIAN"), Some(3412));
        assert_eq!(_LITTLE_ENDIAN, 1234);
        assert_eq!(_BIG_ENDIAN, 4321);
        assert_eq!(_PDP_ENDIAN, 3412);
    }
}
