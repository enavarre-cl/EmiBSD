//! Host tests for the IPv6 checksum: a hand-computed ICMPv6 vector, an independent
//! flat-buffer oracle over chains cut at odd and even offsets, odd payload lengths, an
//! embedded scope zone, and the no-pseudo-header form.

use std::sync::MutexGuard;
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::{m_cat, m_freem, m_get, m_gethdr};
use crate::sys::mbuf::{M_DONTWAIT, MLEN, MT_DATA};

fn setup() -> MutexGuard<'static, ()> {
    crate::kern::uipc_mbuf::tests::setup()
}

const IPPROTO_ICMPV6: u8 = 58;

/// An IPv6 header (version 6, `plen`, next header `nxt`, hop limit 255).
fn ip6_header(src: &[u8; 16], dst: &[u8; 16], plen: u16, nxt: u8) -> [u8; 40] {
    let mut h = [0u8; 40];
    h[0] = 0x60;
    h[4..6].copy_from_slice(&plen.to_be_bytes());
    h[6] = nxt;
    h[7] = 255;
    h[8..24].copy_from_slice(src);
    h[24..40].copy_from_slice(dst);
    h
}

/// A packet holding `bytes` (the IPv6 header first), cut into mbufs of the given lengths
/// (the rest in the last one); every mbuf after the first starts at an odd address.
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
    for (i, p) in pieces.iter().enumerate() {
        assert!(p.len() <= MLEN - 8);
        let m = if i == 0 {
            head
        } else {
            m_get(M_DONTWAIT, MT_DATA).expect("mbuf")
        };
        if i != 0 {
            m.m_data().set(m.m_data().get().wrapping_add(1));
        }
        // SAFETY: the mbuf has room for `p` (checked above) at its data pointer.
        unsafe { core::ptr::copy_nonoverlapping(p.as_ptr(), m.m_data().get(), p.len()) };
        m.m_len().set(p.len() as u32);
        if i != 0 {
            m_cat(head, Some(m));
        }
    }
    head.m_pkthdr().len.set(bytes.len() as i32);
    head
}

/// The checksum of RFC 8200 section 8.1, over a flat buffer, as the bytes to put on the
/// wire.
fn oracle(src: &[u8; 16], dst: &[u8; 16], nxt: u8, payload: &[u8]) -> [u8; 2] {
    let mut buf = Vec::new();
    buf.extend_from_slice(src);
    buf.extend_from_slice(dst);
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(&[0, 0, 0, nxt]);
    buf.extend_from_slice(payload);
    if buf.len() % 2 != 0 {
        buf.push(0);
    }
    let mut sum = 0u32;
    for w in buf.chunks(2) {
        sum += u32::from(u16::from_be_bytes([w[0], w[1]]));
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    (!(sum as u16)).to_be_bytes()
}

const LOOPBACK: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
const A: [u8; 16] = [0xfd, 0x00, 0x00, 0x77, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
const B: [u8; 16] = [0xfd, 0x00, 0x00, 0x77, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];

#[test]
fn icmp6_echo_hand_computed_vector() {
    let _g = setup();
    // ::1 -> ::1, echo request (type 128, code 0), id 0, seq 0, no payload:
    // 0x0001 + 0x0001 (addresses) + 0x0008 (length) + 0x003a (next header) + 0x8000 =
    // 0x8044, whose complement is 0x7fbb.
    let mut pkt = ip6_header(&LOOPBACK, &LOOPBACK, 8, IPPROTO_ICMPV6).to_vec();
    pkt.extend_from_slice(&[128, 0, 0, 0, 0, 0, 0, 0]);
    let m = chain(&pkt, &[]);
    let c = in6_cksum(m, IPPROTO_ICMPV6, 40, 8);
    assert_eq!(c.to_ne_bytes(), [0x7f, 0xbb]);
    assert_eq!(
        oracle(&LOOPBACK, &LOOPBACK, IPPROTO_ICMPV6, &pkt[40..]),
        [0x7f, 0xbb]
    );
    m_freem(m);
}

#[test]
fn chains_cut_anywhere_match_the_oracle() {
    let _g = setup();
    for plen in [8usize, 9, 17, 40, 63, 64, 101] {
        let mut payload: Vec<u8> = (0..plen).map(|i| (i * 37 + 11) as u8).collect();
        payload[0] = 128;
        payload[1] = 0;
        let mut pkt = ip6_header(&A, &B, plen as u16, IPPROTO_ICMPV6).to_vec();
        pkt.extend_from_slice(&payload);
        let want = oracle(&A, &B, IPPROTO_ICMPV6, &payload);

        let cut_sets: [&[usize]; 7] = [
            &[],
            &[40],
            &[41],
            &[40, 1],
            &[43, 3, 5],
            &[40, 7, 1, 2],
            &[60.min(pkt.len() - 1)],
        ];
        for cuts in cut_sets {
            if cuts.iter().sum::<usize>() >= pkt.len() {
                continue;
            }
            let m = chain(&pkt, cuts);
            assert_eq!(
                in6_cksum(m, IPPROTO_ICMPV6, 40, plen as u32).to_ne_bytes(),
                want,
                "plen {plen}, cuts {cuts:?}"
            );
            m_freem(m);
        }
    }
}

#[test]
fn scope_zone_embedded_in_the_address_is_not_summed() {
    let _g = setup();
    let ll_src = [
        0xfe, 0x80, 0x00, 0x05, 0, 0, 0, 0, 0x50, 0x54, 0x00, 0xff, 0xfe, 0xbb, 0, 2,
    ];
    let ll_dst = [0xff, 0x02, 0x00, 0x05, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    let payload = [135u8, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4];
    let mut pkt = ip6_header(&ll_src, &ll_dst, payload.len() as u16, IPPROTO_ICMPV6).to_vec();
    pkt.extend_from_slice(&payload);

    let mut src0 = ll_src;
    src0[2] = 0;
    src0[3] = 0;
    let mut dst0 = ll_dst;
    dst0[2] = 0;
    dst0[3] = 0;
    let want = oracle(&src0, &dst0, IPPROTO_ICMPV6, &payload);

    let m = chain(&pkt, &[]);
    assert_eq!(
        in6_cksum(m, IPPROTO_ICMPV6, 40, payload.len() as u32).to_ne_bytes(),
        want
    );
    m_freem(m);
}

#[test]
fn nxt_zero_skips_the_pseudo_header() {
    let _g = setup();
    // RFC 1071's example, behind an IPv6 header: 0001 f203 f4f5 f6f7 -> 220d.
    let payload = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
    let mut pkt = ip6_header(&A, &B, 8, 59).to_vec();
    pkt.extend_from_slice(&payload);
    for cuts in [&[][..], &[41], &[40, 3]] {
        let m = chain(&pkt, cuts);
        assert_eq!(in6_cksum(m, 0, 40, 8).to_ne_bytes(), [0x22, 0x0d]);
        m_freem(m);
    }
    // From an odd offset into the first mbuf.
    let m = chain(&pkt, &[]);
    assert_eq!(in6_cksum(m, 0, 41, 7), {
        let flat = &payload[1..];
        let mut sum = 0u32;
        let mut it = flat.chunks(2);
        for w in &mut it {
            sum += u32::from(u16::from_be_bytes([w[0], *w.get(1).unwrap_or(&0)]));
        }
        while sum >> 16 != 0 {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        u16::from_ne_bytes((!(sum as u16)).to_be_bytes())
    });
    m_freem(m);
}

#[test]
fn zero_length_payload_is_the_pseudo_header_alone() {
    let _g = setup();
    let pkt = ip6_header(&A, &B, 0, IPPROTO_ICMPV6).to_vec();
    let m = chain(&pkt, &[]);
    assert_eq!(
        in6_cksum(m, IPPROTO_ICMPV6, 40, 0).to_ne_bytes(),
        oracle(&A, &B, IPPROTO_ICMPV6, &[])
    );
    m_freem(m);
}
