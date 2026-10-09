/* <LICENSES> */
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
//! UDP over IP over Ethernet for the standalone network code: send a datagram (the IP and
//! UDP headers and their checksums in front of the data) and receive one for our socket.
//!
//! Upstream: sys/lib/libsa/netudp.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - A packet is a buffer and the offset of its data (`net.rs`); the 8 + 20 + 14 header
//!   bytes go in front of the data, and a buffer without them is `EINVAL` where the C
//!   writes before its pointer.
//! - The headers are read from and written to the buffer as copies (`from_bytes`,
//!   `write_to`); the C's `struct udpiphdr` overlay of the IP header, saved and restored
//!   around the checksum, is the same bytes rewritten in place.
//! - `readudp` drops a datagram whose UDP length reaches past the buffer (the C sums the
//!   bytes beyond it), one whose IP options are longer than the buffer's data (the C's
//!   `len - hlen` wraps) and one left shorter than its headers once its options are gone
//!   (the C returns a negative length other than -1).
//! - `ip_ttl` is `IP_TTL` (4), the socket option's number, as in the C.
//! - The `NET_DEBUG` messages are not ported: no efiboot Makefile defines it.

use core::mem::size_of;
use core::sync::atomic::Ordering;

use crate::arp::{arp_reply, arpwhohas};
use crate::ether::{readether, sendether};
use crate::globals::{GATEIP, NETMASK};
use crate::hdr::endian::{htons, ntohs};
use crate::hdr::ethertypes::{ETHERTYPE_ARP, ETHERTYPE_IP};
use crate::hdr::if_arp::{ARPOP_REQUEST, Arphdr};
use crate::hdr::in_::{INADDR_BROADCAST, IP_TTL, IPPROTO_UDP, InAddr};
use crate::hdr::ip::{IPVERSION, Ip};
use crate::hdr::types::Time;
use crate::hdr::udp::Udphdr;
use crate::in_cksum::in_cksum;
use crate::iodesc::IoDesc;
use crate::net::{ETHER_SIZE, RECV_SIZE, fail, samenet};
use crate::saerrno::Errno;

/// `sizeof(struct ip)`.
const IP_SIZE: usize = size_of::<Ip>();
/// `sizeof(struct udphdr)`.
const UH_SIZE: usize = size_of::<Udphdr>();

/// The bytes of `struct udpiphdr`'s overlay that precede `ui_len`: `ui_x1` (9) and `ui_pr`.
const UI_X1: usize = 9;
/// The offset of `ui_len` in the overlay.
const UI_LEN: usize = 10;
/// The offset of `uh_sum` in the UDP header.
const UH_SUM: usize = 6;

/// The UDP checksum of the datagram `pkt[ipoff..ipoff + len]`, its IP header seen as
/// `struct udpiphdr`: `ui_x1` zeroed and `ui_len` the UDP length (must save and restore ip
/// header).
fn udp_cksum(pkt: &mut [u8], ipoff: usize, len: usize, ulen: u16) -> u16 {
    let tip: [u8; IP_SIZE] = pkt[ipoff..ipoff + IP_SIZE]
        .try_into()
        .unwrap_or([0; IP_SIZE]);
    pkt[ipoff..ipoff + UI_X1].fill(0);
    pkt[ipoff + UI_LEN..ipoff + UI_LEN + 2].copy_from_slice(&ulen.to_ne_bytes());
    let sum = in_cksum(&pkt[ipoff..ipoff + len]);
    pkt[ipoff..ipoff + IP_SIZE].copy_from_slice(&tip);
    sum
}

/// `sendudp(d, pkt, len)`: send `pkt[off..]` from `d`'s address and port to its
/// destination's; the number of data bytes sent.
///
/// Caller must leave room for ethernet, ip and udp headers in front!!
pub fn sendudp(d: &mut IoDesc, pkt: &mut [u8], off: usize) -> Result<usize, Errno> {
    let Some(ipoff) = off.checked_sub(UH_SIZE + IP_SIZE) else {
        return Err(Errno::EINVAL);
    };
    if off > pkt.len() {
        return Err(Errno::EINVAL);
    }
    let uoff = ipoff + IP_SIZE;
    let len = pkt.len() - off + IP_SIZE + UH_SIZE;

    pkt[ipoff..off].fill(0);

    let mut ip = Ip::default();
    ip.set_ip_v(IPVERSION); // half-char
    ip.set_ip_hl((IP_SIZE >> 2) as u8); // half-char
    ip.ip_len = htons(len as u16);
    ip.ip_p = IPPROTO_UDP; // char
    ip.ip_ttl = IP_TTL; // char
    ip.ip_src = d.myip;
    ip.ip_dst = d.destip;
    ip.write_to(&mut pkt[ipoff..]);
    ip.ip_sum = in_cksum(&pkt[ipoff..ipoff + IP_SIZE]); // short, but special
    ip.write_to(&mut pkt[ipoff..]);

    let uh = Udphdr {
        uh_sport: d.myport,
        uh_dport: d.destport,
        uh_ulen: htons((len - IP_SIZE) as u16),
        uh_sum: 0,
    };
    uh.write_to(&mut pkt[uoff..]);

    // Calculate checksum (must save and restore ip header)
    let sum = udp_cksum(pkt, ipoff, len, uh.uh_ulen);
    pkt[uoff + UH_SUM..uoff + UH_SUM + 2].copy_from_slice(&sum.to_ne_bytes());

    let netmask = NETMASK.load(Ordering::Relaxed);
    let ea = if ip.ip_dst.s_addr == INADDR_BROADCAST
        || ip.ip_src.s_addr == 0
        || netmask == 0
        || samenet(ip.ip_src, ip.ip_dst, netmask)
    {
        arpwhohas(d, ip.ip_dst)
    } else {
        arpwhohas(
            d,
            InAddr {
                s_addr: GATEIP.load(Ordering::Relaxed),
            },
        )
    };

    let cc = sendether(d, pkt, ipoff, &ea, ETHERTYPE_IP)?;
    if cc != len {
        crate::exit::panic(format_args!("sendudp: bad write ({} != {})", cc, len));
    }
    Ok(cc - (IP_SIZE + UH_SIZE))
}

/// `readudp(d, pkt, len, tleft)`: receive a UDP packet for `d` and validate it is for us,
/// its data into `pkt[off..]`; the data's length. Answers the ARP requests for our address
/// it meets.
///
/// Caller leaves room for the headers (Ether, IP, UDP)
pub fn readudp(d: &mut IoDesc, pkt: &mut [u8], off: usize, tleft: Time) -> Result<usize, Errno> {
    let Some(ipoff) = off.checked_sub(UH_SIZE + IP_SIZE) else {
        return Err(Errno::EINVAL);
    };
    let Some(len) = pkt.len().checked_sub(off) else {
        return Err(Errno::EINVAL);
    };
    let uoff = ipoff + IP_SIZE;

    let (mut n, etype) = match readether(d, pkt, ipoff, tleft) {
        Ok((n, etype)) if n >= IP_SIZE + UH_SIZE => (n, etype),
        _ => return fail(),
    };

    // Ethernet address checks now in readether()

    // Need to respond to ARP requests.
    if etype == ETHERTYPE_ARP {
        let ah = Arphdr::from_bytes(&pkt[ipoff..]).unwrap_or_default();
        if ah.ar_op == htons(ARPOP_REQUEST) {
            // Send ARP reply
            arp_reply(d, pkt, ipoff);
        }
        return fail();
    }

    if etype != ETHERTYPE_IP {
        return fail();
    }

    // Check ip header
    let Some(mut ip) = Ip::from_bytes(&pkt[ipoff..]) else {
        return fail();
    };
    if ip.ip_v() != IPVERSION || ip.ip_p != IPPROTO_UDP {
        // half char
        return fail();
    }

    let hlen = usize::from(ip.ip_hl()) << 2;
    if hlen < IP_SIZE || ipoff + hlen > pkt.len() || in_cksum(&pkt[ipoff..ipoff + hlen]) != 0 {
        return fail();
    }
    if n < usize::from(ntohs(ip.ip_len)) {
        return fail();
    }
    if d.myip.s_addr != 0 && ip.ip_dst.s_addr != d.myip.s_addr {
        return fail();
    }

    // If there were ip options, make them go away
    if hlen != IP_SIZE {
        let Some(count) = len.checked_sub(hlen) else {
            return fail();
        };
        pkt.copy_within(ipoff + hlen..ipoff + hlen + count, uoff);
        ip.ip_len = htons(IP_SIZE as u16);
        ip.write_to(&mut pkt[ipoff..]);
        n -= hlen - IP_SIZE;
    }
    let uh = Udphdr::from_bytes(&pkt[uoff..]).unwrap_or_default();
    if uh.uh_dport != d.myport {
        return fail();
    }

    if uh.uh_sum != 0 {
        n = usize::from(ntohs(uh.uh_ulen)) + IP_SIZE;
        if n > RECV_SIZE - ETHER_SIZE {
            crate::printf!("readudp: huge packet, udp len {}\n", n);
            return fail();
        }
        if ipoff + n > pkt.len() {
            return fail();
        }

        // Check checksum (must save and restore ip header)
        if udp_cksum(pkt, ipoff, n, uh.uh_ulen) != 0 {
            return fail();
        }
    }
    if usize::from(ntohs(uh.uh_ulen)) < UH_SIZE {
        return fail();
    }

    // `n` may be the UDP length plus the IP header, shorter than both headers
    n.checked_sub(IP_SIZE + UH_SIZE).map_or_else(fail, Ok)
}
/* </CODE> */
