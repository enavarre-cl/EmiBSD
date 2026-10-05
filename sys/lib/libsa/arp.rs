/*	$OpenBSD: arp.c,v 1.13 2021/03/12 10:22:46 jsg Exp $	*/
/*	$NetBSD: arp.c,v 1.15 1996/10/13 02:28:58 christos Exp $	*/
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
 * @(#) Header: arp.c,v 1.5 93/07/15 05:52:26 leres Exp  (LBL)
 */
/* </LICENSES> */

//! ARP for the standalone network code: ask who has an address (with a small cache of the
//! answers) and answer the requests for our own.
//!
//! Upstream: sys/lib/libsa/arp.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `arpwhohas()` returns the Ethernet address by value where the C returns a pointer into
//!   its cache.
//! - When the cache overflows, the C recycles it (`arp_num = 1`) but goes on writing through
//!   the pointer its search left one past the end of `arp_list[]`; here the new entry is
//!   `arp_list[arp_num]`, the slot the recycling frees (and the one `arprecv` compares
//!   the reply with, as in the C).
//! - `arp_list[]` is the [`StaticCell`] [`ARP_LIST`] (statics are upper case in Rust) and
//!   `arp_num` an atomic that keeps its lower-case name: in upper case it would be the
//!   constant `ARP_NUM`.
//! - `arp_reply()` takes the packet buffer and the offset of the ARP packet in it (the
//!   Ethernet header goes in front, as `net.rs` says), and does nothing when the buffer
//!   cannot hold the reply's 46 bytes.
//! - The `ARP_DEBUG` messages are not ported: no efiboot Makefile defines it.

use core::sync::atomic::{AtomicUsize, Ordering};

use libkern::staticcell::StaticCell;

use crate::dev::set_errno;
use crate::ether::{readether, sendether};
use crate::globals::BCEA;
use crate::hdr::endian::htons;
use crate::hdr::ethertypes::{ETHERTYPE_ARP, ETHERTYPE_IP};
use crate::hdr::if_arp::{ARPHRD_ETHER, ARPOP_REPLY, ARPOP_REQUEST, Arphdr};
use crate::hdr::if_ether::{ETHER_HDR_LEN, EtherArp};
use crate::hdr::in_::InAddr;
use crate::hdr::types::Time;
use crate::iodesc::IoDesc;
use crate::net::{BA, inet_ntoa, sendrecv};
use crate::saerrno::Errno;

/// `ARP_NUM`: the cache's size (need at most 3 arp entries).
pub const ARP_NUM: usize = 8;

/// `sizeof(struct ether_arp)`.
const ARP_SIZE: usize = core::mem::size_of::<EtherArp>();

/// The request's padding to the 60-byte minimum frame (`60 - sizeof(...)`).
const WPAD: usize = 18;
/// The reply buffer's extra space.
const RPAD: usize = 24;

/// `struct arp_list`: a cache entry.
#[derive(Clone, Copy, Debug)]
pub struct ArpList {
    /// `addr`: the internet address.
    pub addr: InAddr,
    /// `ea`: its Ethernet address.
    pub ea: [u8; 6],
}

/// `arp_list[ARP_NUM]`: the cache; entry 0 is the broadcast address (XXX - net order
/// `INADDR_BROADCAST` must be a constant).
pub static ARP_LIST: StaticCell<[ArpList; ARP_NUM]> = StaticCell::new(arp_list_init());

/// `arp_num`: the entries in use.
#[allow(non_upper_case_globals)] // `ARP_NUM` is the cache's size
pub static arp_num: AtomicUsize = AtomicUsize::new(1);

/// `arp_list[]`'s initial value.
const fn arp_list_init() -> [ArpList; ARP_NUM] {
    let mut l = [ArpList {
        addr: InAddr { s_addr: 0 },
        ea: [0; 6],
    }; ARP_NUM];
    l[0] = ArpList {
        addr: InAddr {
            s_addr: 0xffff_ffff,
        },
        ea: BA,
    };
    l
}

/// Entry `i` of the cache.
fn arp_entry(i: usize) -> ArpList {
    // SAFETY: a copy out of the cache; no reference into it outlives the copy, and the
    // standalone program is single-threaded.
    unsafe { ARP_LIST.get()[i % ARP_NUM] }
}

/// Sets entry `i` of the cache.
fn set_arp_entry(i: usize, e: ArpList) {
    // SAFETY: as for `arp_entry`; the store holds no reference past the statement.
    unsafe { ARP_LIST.get_mut()[i % ARP_NUM] = e };
}

/// `arpwhohas(d, addr)`: broadcast an ARP packet, asking who has `addr` on interface `d`;
/// its Ethernet address. No answer is a `panic()`.
pub fn arpwhohas(d: &mut IoDesc, addr: InAddr) -> [u8; 6] {
    // Try for cached answer first
    let mut num = arp_num.load(Ordering::Relaxed);
    for i in 0..num {
        let al = arp_entry(i);
        if addr.s_addr == al.addr.s_addr {
            return al.ea;
        }
    }

    // Don't overflow cache
    if num > ARP_NUM - 1 {
        num = 1; // recycle
        arp_num.store(num, Ordering::Relaxed);
        crate::printf!("arpwhohas: overflowed arp_list!\n");
    }

    let mut wbuf = [0u8; ETHER_HDR_LEN + ARP_SIZE + WPAD];
    let mut rbuf = [0u8; ETHER_HDR_LEN + ARP_SIZE + RPAD];
    let ah = EtherArp {
        ea_hdr: Arphdr {
            ar_hrd: htons(ARPHRD_ETHER),
            ar_pro: htons(ETHERTYPE_IP),
            ar_hln: 6, // hardware address length
            ar_pln: 4, // protocol address length
            ar_op: htons(ARPOP_REQUEST),
        },
        arp_sha: d.myea,
        arp_spa: d.myip.s_addr.to_ne_bytes(),
        // Leave zeros in arp_tha
        arp_tha: [0; 6],
        arp_tpa: addr.s_addr.to_ne_bytes(),
    };
    ah.write_to(&mut wbuf[ETHER_HDR_LEN..]);

    // Store ip address in cache (incomplete entry).
    let mut al = arp_entry(num);
    al.addr = addr;
    set_arp_entry(num, al);

    if sendrecv(
        d,
        arpsend,
        &mut wbuf,
        ETHER_HDR_LEN,
        arprecv,
        &mut rbuf,
        ETHER_HDR_LEN,
    )
    .is_err()
    {
        crate::exit::panic(format_args!("arp: no response for {}", inet_ntoa(addr)));
    }

    // Store ethernet address in cache
    let ah = EtherArp::from_bytes(&rbuf[ETHER_HDR_LEN..]).unwrap_or_default();
    al.ea = ah.arp_sha;
    set_arp_entry(num, al);
    arp_num.store(num + 1, Ordering::Relaxed);

    al.ea
}

/// `arpsend(d, pkt, len)`: broadcast the request.
fn arpsend(d: &mut IoDesc, pkt: &mut [u8], off: usize) -> Result<usize, Errno> {
    sendether(d, pkt, off, &BCEA, ETHERTYPE_ARP)
}

/// `arprecv(d, pkt, len, tleft)`: the length if this is the reply we're waiting for, else
/// an error with `errno` 0 (answering the requests for our address on the way).
fn arprecv(d: &mut IoDesc, pkt: &mut [u8], off: usize, tleft: Time) -> Result<usize, Errno> {
    let r = readether(d, pkt, off, tleft);
    set_errno(Errno(0)); // XXX
    let not_yet = Err(Errno(0));
    let (n, etype) = match r {
        Ok((n, etype)) if n >= ARP_SIZE => (n, etype),
        _ => return not_yet,
    };

    if etype != ETHERTYPE_ARP {
        return not_yet;
    }

    // Ethernet address now checked in readether()

    let Some(ah) = EtherArp::from_bytes(&pkt[off..]) else {
        return not_yet;
    };
    if ah.arp_hrd() != htons(ARPHRD_ETHER)
        || ah.arp_pro() != htons(ETHERTYPE_IP)
        || ah.arp_hln() != 6
        || ah.arp_pln() != 4
    {
        return not_yet;
    }

    if ah.arp_op() == htons(ARPOP_REQUEST) {
        arp_reply(d, pkt, off);
        return not_yet;
    }

    if ah.arp_op() != htons(ARPOP_REPLY) {
        return not_yet;
    }

    // Is the reply from the source we want?
    let want = arp_entry(arp_num.load(Ordering::Relaxed));
    if want.addr.s_addr.to_ne_bytes() != ah.arp_spa {
        return not_yet;
    }
    // We don't care who the reply was sent to.

    // We have our answer.
    Ok(n)
}

/// `arp_reply(d, pkt)`: convert the ARP request at `pkt[off..]` into a reply and send it.
/// Re-uses buffer. Pad to length = 46.
pub fn arp_reply(d: &mut IoDesc, pkt: &mut [u8], off: usize) {
    let Some(mut arp) = pkt.get(off..).and_then(EtherArp::from_bytes) else {
        return;
    };

    if arp.arp_hrd() != htons(ARPHRD_ETHER)
        || arp.arp_pro() != htons(ETHERTYPE_IP)
        || arp.arp_hln() != 6
        || arp.arp_pln() != 4
    {
        return;
    }

    if arp.arp_op() != htons(ARPOP_REQUEST) {
        return;
    }

    // If we are not the target, ignore the request.
    if arp.arp_tpa != d.myip.s_addr.to_ne_bytes() {
        return;
    }

    arp.ea_hdr.ar_op = htons(ARPOP_REPLY);
    // source becomes target
    arp.arp_tha = arp.arp_sha;
    arp.arp_tpa = arp.arp_spa;
    // here becomes source
    arp.arp_sha = d.myea;
    arp.arp_spa = d.myip.s_addr.to_ne_bytes();

    let Some(frame) = pkt.get_mut(..off + ARP_SIZE + WPAD) else {
        return;
    };
    arp.write_to(&mut frame[off..]);

    // No need to get fancy here. If the send fails, the requestor will just ask again.
    let _ = sendether(d, frame, off, &arp.arp_tha, ETHERTYPE_ARP);
}
