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
/* </CODE> */
