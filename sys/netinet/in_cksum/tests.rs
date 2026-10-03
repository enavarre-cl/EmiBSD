//! Host tests for the Internet checksum: the RFC 1071 example, an IPv4 header, chains split at
//! odd and even offsets, and the pseudo header of `in4_cksum`.

use std::sync::MutexGuard;
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::{m_cat, m_freem, m_get, m_gethdr};
use crate::netinet::in4_cksum::in4_cksum;
use crate::sys::mbuf::{M_DONTWAIT, MLEN, MT_DATA};

fn setup() -> MutexGuard<'static, ()> {
    crate::kern::uipc_mbuf::tests::setup()
}

/// A chain holding `bytes`, cut into mbufs of the given lengths (the rest in the last one).
fn chain(bytes: &[u8], cuts: &[usize]) -> &'static Mbuf {
    let mut pieces = Vec::new();
    let mut rest = bytes;
    for &c in cuts {
        let (a, b) = rest.split_at(c);
        pieces.push(a);
        rest = b;
    }
    pieces.push(rest);

    let head = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
    head.m_len().set(0);
    let mut first = true;
    for p in pieces {
        assert!(p.len() <= MLEN - 8);
        let m = if first {
            head
        } else {
            m_get(M_DONTWAIT, MT_DATA).expect("mbuf")
        };
        if !first {
            // Start the data at an odd address to exercise the C's "force to even boundary".
            m.m_data().set(m.m_data().get().wrapping_add(1));
        }
        // SAFETY: the mbuf has room for `p` (checked above) at its data pointer.
        unsafe { core::ptr::copy_nonoverlapping(p.as_ptr(), m.m_data().get(), p.len()) };
        m.m_len().set(p.len() as u32);
        if !first {
            m_cat(head, Some(m));
        }
        first = false;
    }
    head.m_pkthdr().len.set(bytes.len() as i32);
    head
}

#[test]
fn rfc1071_example() {
    let _g = setup();
    // RFC 1071 section 3: 0001 f203 f4f5 f6f7 sums to ddf2, whose complement is 220d.
    let data = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
    let m = chain(&data, &[]);
    assert_eq!(in_cksum(m, data.len() as i32).to_ne_bytes(), [0x22, 0x0d]);
    m_freem(m);
}

#[test]
fn ipv4_header_checksum() {
    let _g = setup();
    // A textbook header (192.168.0.1 -> 192.168.0.199, TTL 64, UDP): checksum b861.
    let mut hdr = [
        0x45, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8, 0x00,
        0x01, 0xc0, 0xa8, 0x00, 0xc7,
    ];
    let m = chain(&hdr, &[]);
    assert_eq!(in_cksum(m, 20).to_ne_bytes(), [0xb8, 0x61]);
    m_freem(m);

    // A header with its checksum verifies to 0.
    hdr[10] = 0xb8;
    hdr[11] = 0x61;
    let m = chain(&hdr, &[]);
    assert_eq!(in_cksum(m, 20), 0);
    m_freem(m);
}

#[test]
fn split_chains_sum_like_one_buffer() {
    let _g = setup();
    let data: Vec<u8> = (0..101u32).map(|i| (i * 37 + 11) as u8).collect();
    let whole = chain(&data, &[]);
    let want = in_cksum(whole, data.len() as i32);
    m_freem(whole);

    for cuts in [&[1usize][..], &[2], &[3, 5], &[7, 7, 7], &[50, 1, 1]] {
        let m = chain(&data, cuts);
        assert_eq!(in_cksum(m, data.len() as i32), want, "cuts {cuts:?}");
        // A prefix of odd length pads its last byte with zero.
        let m2 = chain(&data[..33], &[]);
        assert_eq!(in_cksum(m, 33), in_cksum(m2, 33), "prefix, cuts {cuts:?}");
        m_freem(m2);
        m_freem(m);
    }
}

#[test]
fn in4_cksum_covers_the_pseudo_header() {
    let _g = setup();
    // An IPv4 header (no options) followed by an 8-byte UDP header and 4 bytes of payload.
    let mut pkt = [
        0x45, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x40, 0x11, 0x00, 0x00, 10, 0, 2, 15, 10,
        0, 2, 2, // ip
        0x30, 0x39, 0x00, 0x35, 0x00, 0x0c, 0x00, 0x00, // udp, sum 0
        0xde, 0xad, 0xbe, 0xef,
    ];
    let m = chain(&pkt, &[]);
    let sum = in4_cksum(m, 17, 20, 12);
    m_freem(m);

    // The pseudo header plus the UDP datagram, summed by hand as one buffer.
    let mut manual = Vec::new();
    manual.extend_from_slice(&pkt[12..20]); // src, dst
    manual.extend_from_slice(&[0, 17, 0, 12]); // zero, proto, udp length
    manual.extend_from_slice(&pkt[20..]);
    let m = chain(&manual, &[]);
    assert_eq!(sum, in_cksum(m, manual.len() as i32));
    m_freem(m);

    // With the checksum in place the datagram verifies.
    pkt[26..28].copy_from_slice(&sum.to_ne_bytes());
    let m = chain(&pkt, &[23]);
    assert_eq!(in4_cksum(m, 17, 20, 12), 0);
    m_freem(m);
}
