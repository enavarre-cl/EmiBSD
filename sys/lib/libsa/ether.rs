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

/*
 * Copyright (c) 1992 Regents of the University of California.
 * All rights reserved.
 *
 * This software was developed by the Computer Systems Engineering group
 * at Lawrence Berkeley Laboratory under DARPA contract BG 91-66 and
 * contributed to Berkeley.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by the University of
 *	California, Lawrence Berkeley Laboratory and its contributors.
 * 4. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * @(#) Header: net.c,v 1.9 93/08/06 19:32:15 leres Exp  (LBL)
 */
/* </LICENSES> */

/* <CODE> */
//! The Ethernet layer of the standalone network code: send a packet with an Ethernet
//! header in front, receive one addressed to us or to the broadcast address.
//!
//! Upstream: sys/lib/libsa/ether.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - A packet is a buffer and the offset of its data (`net.rs`); the header goes in the 14
//!   bytes in front of the data, and a buffer without them is `EINVAL` where the C writes
//!   before its pointer.
//! - `readether` returns the Ethernet type with the length, where the C stores it through
//!   its fifth argument. A driver that claims more bytes than the buffer holds is taken at
//!   the buffer's length.
//! - `ether_sprintf()` returns the text by value ([`NetStr`]) where the C returns its
//!   `static char etherbuf[]`.
//! - The `ETHER_DEBUG` messages are not ported: no efiboot Makefile defines it.

use crate::globals::BCEA;
use crate::hdr::endian::{htons, ntohs};
use crate::hdr::if_ether::{ETHER_HDR_LEN, EtherHeader};
use crate::hdr::types::Time;
use crate::iodesc::IoDesc;
use crate::net::{NetStr, fail};
use crate::netif::{netif_get, netif_put};
use crate::saerrno::Errno;

/// `sendether(d, pkt, len, dea, etype)`: send `pkt[off..]` to the Ethernet address `dea`
/// as type `etype`, the header in front of it; the number of data bytes sent.
///
/// Caller must leave room for ethernet header in front!!
pub fn sendether(
    d: &mut IoDesc,
    pkt: &mut [u8],
    off: usize,
    dea: &[u8; 6],
    etype: u16,
) -> Result<usize, Errno> {
    let Some(eoff) = off.checked_sub(ETHER_HDR_LEN) else {
        return Err(Errno::EINVAL);
    };
    if off > pkt.len() {
        return Err(Errno::EINVAL);
    }

    let eh = EtherHeader {
        ether_dhost: *dea,
        ether_shost: d.myea,
        ether_type: htons(etype),
    };
    eh.write_to(&mut pkt[eoff..]);

    match netif_put(d, &pkt[eoff..]) {
        Ok(n) if n >= ETHER_HDR_LEN => Ok(n - ETHER_HDR_LEN),
        _ => fail(),
    }
}

/// `readether(d, pkt, len, tleft, &etype)`: get a packet of any Ethernet type, with our
/// address or the broadcast address, its data into `pkt[off..]`; the data's length and the
/// Ethernet type (host order).
///
/// NOTE: Caller must leave room for the Ether header.
pub fn readether(
    d: &mut IoDesc,
    pkt: &mut [u8],
    off: usize,
    tleft: Time,
) -> Result<(usize, u16), Errno> {
    let Some(eoff) = off.checked_sub(ETHER_HDR_LEN) else {
        return Err(Errno::EINVAL);
    };
    let Some(frame) = pkt.get_mut(eoff..) else {
        return Err(Errno::EINVAL);
    };

    let n = match netif_get(d, frame, tleft) {
        Ok(n) if n >= ETHER_HDR_LEN => n.min(frame.len()),
        _ => return fail(),
    };
    let Some(eh) = EtherHeader::from_bytes(frame) else {
        return fail();
    };

    // Validate Ethernet address.
    if d.myea != eh.ether_dhost && BCEA != eh.ether_dhost {
        return fail();
    }

    Ok((n - ETHER_HDR_LEN, ntohs(eh.ether_type)))
}

/// `digits[]`.
const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// `ether_sprintf(ap)`: convert Ethernet address to printable (loggable) representation.
pub fn ether_sprintf(ap: &[u8; 6]) -> NetStr<17> {
    let mut etherbuf = [0u8; 17];
    let mut cp = 0;
    for (i, &b) in ap.iter().enumerate() {
        etherbuf[cp] = DIGITS[usize::from(b >> 4)];
        etherbuf[cp + 1] = DIGITS[usize::from(b & 0xf)];
        if i < 5 {
            etherbuf[cp + 2] = b':';
        }
        cp += 3;
    }
    NetStr::new(etherbuf, 0)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ether_sprintf_formats_lowercase_hex() {
        let s = ether_sprintf(&[0x52, 0x54, 0x00, 0x12, 0x34, 0xaf]);
        assert_eq!(s.as_bytes(), b"52:54:00:12:34:af");
    }
}
/* </TESTS> */
