//! Host tests for the IPv6 half of `ah_massage_headers`: the IPv6 header is cooked (flow
//! label, hop limit and the scope of link-local addresses zeroed), the mutable options of
//! hop-by-hop and destination option headers are zeroed, a type 0 routing header is put in
//! its final form on output, and a jumbogram or an unknown header is refused.

use std::sync::MutexGuard;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::{m_cat, m_copydata, m_get, m_gethdr};
use crate::netinet::in_::{IPPROTO_DSTOPTS, IPPROTO_HOPOPTS, IPPROTO_ROUTING, IPPROTO_UDP};
use crate::netinet::ipsec_input::AHCOUNTERS;
use crate::sys::mbuf::{MHLEN, MT_DATA};

fn setup() -> MutexGuard<'static, ()> {
    crate::kern::uipc_mbuf::tests::setup()
}

/// An IPv6 header with a flow label, a hop limit and a link-local source (scope 1 embedded).
fn header(plen: u16, nxt: u8, dst: [u8; 16]) -> Vec<u8> {
    let mut h = vec![0x6a, 0xbc, 0xde, 0xf0];
    h.extend_from_slice(&plen.to_be_bytes());
    h.extend_from_slice(&[nxt, 64]);
    let mut src = [0u8; 16];
    src[0] = 0xfe;
    src[1] = 0x80;
    src[2] = 0x00;
    src[3] = 0x01; // the embedded scope (s6_addr16[1])
    src[15] = 1;
    h.extend_from_slice(&src);
    h.extend_from_slice(&dst);
    h
}

const DST: [u8; 16] = [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];

/// A packet of `bytes` in mbufs of at most `piece` bytes.
fn packet(bytes: &[u8], piece: usize) -> &'static Mbuf {
    let head = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
    for (i, chunk) in bytes.chunks(piece).enumerate() {
        let m = if i == 0 {
            head
        } else {
            m_get(M_DONTWAIT, MT_DATA).expect("mbuf")
        };
        // SAFETY: `piece` is at most `MHLEN`, the room of a fresh mbuf.
        unsafe { ptr::copy_nonoverlapping(chunk.as_ptr(), mtod::<u8>(m), chunk.len()) };
        m.m_len().set(chunk.len() as u32);
        if i != 0 {
            m_cat(head, Some(m));
        }
    }
    head.m_pkthdr().len.set(bytes.len() as i32);
    head
}

fn bytes(m: &Mbuf) -> Vec<u8> {
    let mut v = vec![0u8; m.m_pkthdr().len.get() as usize];
    m_copydata(m, 0, &mut v);
    v
}

fn hdrops() -> u64 {
    AHCOUNTERS[AhstatCounters::AhsHdrops as usize].load(Ordering::Relaxed)
}

/// A hop-by-hop header: a mutable option (type 0x20 with two data bytes), then a Router Alert
/// shaped option that is not mutable and a Pad1.
fn hbh(nxt: u8) -> Vec<u8> {
    vec![nxt, 0, 0x20, 2, 0xaa, 0xbb, 0x05, 0x00]
}

#[test]
fn the_ipv6_header_is_cooked_and_mutable_options_zeroed() {
    let _g = setup();
    for piece in [MHLEN, 40] {
        // One mbuf holds the whole packet (the headers are changed in place), or the first
        // mbuf has the IPv6 header only (they are copied out and back).
        let mut p = header(8 + 8, IPPROTO_HOPOPTS as u8, DST);
        p.extend_from_slice(&hbh(IPPROTO_UDP as u8));
        p.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let m = packet(&p, piece);

        let mut mp = Some(m);
        ah_massage_headers(&mut mp, AF_INET6, 48, 0, false).expect("massaged");
        let out = bytes(mp.expect("the packet is kept"));
        assert_eq!(out.len(), p.len());

        // ip6_flow: version only; ip6_hlim: zero; the source's scope is zeroed.
        assert_eq!(&out[..4], &[0x60, 0, 0, 0]);
        assert_eq!(&out[4..7], &p[4..7], "plen and next header stay");
        assert_eq!(out[7], 0, "hop limit");
        assert_eq!(&out[8..10], &[0xfe, 0x80]);
        assert_eq!(&out[10..12], &[0, 0], "scope zeroed");
        assert_eq!(&out[12..24], &p[12..24]);
        assert_eq!(&out[24..40], &DST, "a global destination is untouched");

        // The mutable option is gone, the other one and the payload stay.
        assert_eq!(
            &out[40..48],
            &[IPPROTO_UDP as u8, 0, 0, 0, 0, 0, 0x05, 0x00]
        );
        assert_eq!(&out[48..], &p[48..]);
        m_freem(mp);
    }
}

#[test]
fn a_type_0_routing_header_is_put_in_its_final_form_on_output() {
    let _g = setup();
    let first = DST;
    let mut last = [0u8; 16];
    last[0] = 0x20;
    last[1] = 0x01;
    last[2] = 0x0d;
    last[3] = 0xb9;
    last[15] = 9;

    // The packet is addressed to the first hop (DST) with one segment left: the final
    // destination is `last`.
    let mut p = header(24 + 4, IPPROTO_ROUTING as u8, first);
    p.extend_from_slice(&[IPPROTO_UDP as u8, 2, 0, 1, 0, 0, 0, 0]);
    p.extend_from_slice(&last);
    p.extend_from_slice(&[1, 2, 3, 4]);

    // On input nothing is done to the routing header.
    let m = packet(&p, MHLEN);
    let mut mp = Some(m);
    ah_massage_headers(&mut mp, AF_INET6, 64, 0, false).expect("massaged");
    let out = bytes(mp.expect("kept"));
    assert_eq!(&out[24..40], &first);
    assert_eq!(&out[40..], &p[40..]);
    m_freem(mp);

    // On output the header looks as it will at the final destination: the destination is the
    // last address, the first hop took the place of the address in the header, no segments
    // left.
    let m = packet(&p, MHLEN);
    let mut mp = Some(m);
    ah_massage_headers(&mut mp, AF_INET6, 64, 0, true).expect("massaged");
    let out = bytes(mp.expect("kept"));
    assert_eq!(&out[24..40], &last, "ip6_dst is the final destination");
    assert_eq!(
        &out[40..44],
        &[IPPROTO_UDP as u8, 2, 0, 0],
        "no segments left"
    );
    assert_eq!(&out[48..64], &first, "the first hop is in the address list");
    assert_eq!(&out[64..], &[1, 2, 3, 4]);
    m_freem(mp);

    // A routing header that claims more segments than the skipped headers hold is refused.
    let mut bad = p.clone();
    bad[43] = 5;
    let m = packet(&bad, MHLEN);
    let before = hdrops();
    let mut mp = Some(m);
    assert_eq!(
        ah_massage_headers(&mut mp, AF_INET6, 64, 0, true),
        Err(Errno::EINVAL)
    );
    assert!(mp.is_none(), "the packet is freed");
    assert_eq!(hdrops(), before + 1);
}

#[test]
fn jumbograms_and_unexpected_headers_are_refused() {
    let _g = setup();

    // ip6_plen 0 is a jumbogram.
    let mut p = header(0, IPPROTO_UDP as u8, DST);
    p.extend_from_slice(&[0; 8]);
    let before = hdrops();
    let mut mp = Some(packet(&p, MHLEN));
    assert_eq!(
        ah_massage_headers(&mut mp, AF_INET6, 40, 0, false),
        Err(Errno::EMSGSIZE)
    );
    assert!(mp.is_none());
    assert_eq!(hdrops(), before + 1);

    // A fragment header between the IPv6 header and AH cannot be authenticated.
    let mut p = header(16, 44, DST);
    p.extend_from_slice(&[IPPROTO_UDP as u8, 0, 0, 0, 0, 0, 0, 1]);
    p.extend_from_slice(&[0; 8]);
    let before = hdrops();
    let mut mp = Some(packet(&p, MHLEN));
    assert_eq!(
        ah_massage_headers(&mut mp, AF_INET6, 48, 0, false),
        Err(Errno::EINVAL)
    );
    assert!(mp.is_none());
    assert_eq!(hdrops(), before + 1);

    // An options header whose options run past its length.
    let mut p = header(16, IPPROTO_DSTOPTS as u8, DST);
    p.extend_from_slice(&[IPPROTO_UDP as u8, 0, 0x20, 9, 0, 0, 0, 0]);
    p.extend_from_slice(&[0; 8]);
    let mut mp = Some(packet(&p, MHLEN));
    assert_eq!(
        ah_massage_headers(&mut mp, AF_INET6, 48, 0, false),
        Err(Errno::EINVAL)
    );
    assert!(mp.is_none());
}
