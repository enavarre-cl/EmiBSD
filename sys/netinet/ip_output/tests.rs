//! Host tests for IPv4 output: fragmentation (offsets, flags, lengths and checksums of each
//! fragment), the option copy of the later fragments, and option insertion.

use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::{m_copydata, m_freem, ml_dequeue};
use crate::net::if_::tests::{setup_net, zeroed_static};
use crate::netinet::ip::{IP_OFFMASK, IPOPT_LSRR, IPOPT_RR};

/// A packet of `len` bytes: an IP header of `hlen` (options from `opts`, padded with NOPs)
/// and a patterned payload, in a chain of clusters.
fn packet(len: usize, opts: &[u8]) -> &'static Mbuf {
    let hlen = 20 + opts.len().div_ceil(4) * 4;
    let mut p = vec![0u8; len];
    p[0] = 0x40 | (hlen / 4) as u8;
    p[2..4].copy_from_slice(&(len as u16).to_be_bytes());
    p[8] = 64;
    p[9] = 17;
    p[12..16].copy_from_slice(&[10, 0, 2, 15]);
    p[16..20].copy_from_slice(&[10, 0, 2, 2]);
    p[20..20 + opts.len()].copy_from_slice(opts);
    for b in &mut p[20 + opts.len()..hlen] {
        *b = IPOPT_NOP;
    }
    for (i, b) in p[hlen..].iter_mut().enumerate() {
        *b = (i * 13 + 5) as u8;
    }
    let m = crate::kern::uipc_mbuf::m_devget(&p, 0).expect("packet");
    // The header must be in the first mbuf.
    m_pullup(m, hlen as i32).expect("pullup")
}

fn bytes(m: &Mbuf) -> Vec<u8> {
    let mut v = vec![0u8; m.m_pkthdr().len.get() as usize];
    m_copydata(m, 0, &mut v);
    v
}

use crate::kern::uipc_mbuf::m_pullup;

#[test]
fn fragments_cover_the_payload_with_valid_headers() {
    let _g = setup_net();
    // SAFETY: the all-zero `Ifnet` is valid.
    let ifp: &'static Ifnet = unsafe { zeroed_static() };
    let len = 3000;
    let m = packet(len, &[]);
    let whole = bytes(m);
    let ml = MbufList::new();

    ip_fragment(m, &ml, ifp, 1500).expect("fragment");
    assert_eq!(ml_len(&ml), 3);

    let mut payload = Vec::new();
    let mut expect_off = 0usize;
    let mut n = 0;
    while let Some(f) = ml_dequeue(&ml) {
        let b = bytes(f);
        let ip = mtod_ip(f);
        assert_eq!(ip.ip_hl(), 5);
        assert_eq!(usize::from(ntohs(ip.ip_len)), b.len());
        assert!(b.len() <= 1500);
        let off = ntohs(ip.ip_off);
        assert_eq!(usize::from(off & IP_OFFMASK) * 8, expect_off);
        let last = expect_off + b.len() - 20 >= len - 20;
        assert_eq!(off & IP_MF != 0, !last, "MF on every fragment but the last");
        assert_eq!(in_cksum(f, 20), 0, "fragment {n} header checksum");
        payload.extend_from_slice(&b[20..]);
        expect_off += b.len() - 20;
        n += 1;
        m_freem(f);
    }
    assert_eq!(
        payload,
        &whole[20..],
        "the fragments carry the payload in order"
    );
}

#[test]
fn too_small_an_mtu_is_emsgsize() {
    let _g = setup_net();
    // SAFETY: the all-zero `Ifnet` is valid.
    let ifp: &'static Ifnet = unsafe { zeroed_static() };
    let m = packet(100, &[]);
    let ml = MbufList::new();
    assert_eq!(ip_fragment(m, &ml, ifp, 27), Err(Errno::EMSGSIZE));
    assert_eq!(ml_len(&ml), 0, "the packet was freed");
}

#[test]
fn later_fragments_keep_only_the_copied_options() {
    // Record route (not copied) then loose source route (copied).
    let opts = [IPOPT_RR, 7, 4, 0, 0, 0, 0, IPOPT_LSRR, 7, 4, 10, 0, 2, 9];
    let mut hdr = vec![0u8; 20 + 16];
    hdr[0] = 0x49; // 36 bytes of header
    hdr[20..20 + opts.len()].copy_from_slice(&opts);
    hdr[34] = IPOPT_NOP;
    hdr[35] = IPOPT_EOL;
    let mut out = vec![0xeeu8; 20 + 16];
    // SAFETY: both buffers hold a 36-byte header.
    let n = unsafe { ip_optcopy(hdr.as_ptr(), 36, out.as_mut_ptr()) };
    assert_eq!(
        n, 8,
        "the source route and the NOP after it (kept for alignment)"
    );
    assert_eq!(&out[20..27], &opts[7..14]);
    assert_eq!(out[27], IPOPT_NOP);
}

#[test]
fn inserted_options_grow_the_header() {
    let _g = setup_net();
    let m = packet(60, &[]);
    let opt = crate::kern::uipc_mbuf::m_get(M_DONTWAIT, crate::sys::mbuf::MT_SOOPTS).expect("mbuf");
    // struct ipoption: no first hop, then a 4-byte record route.
    let o = [0u8, 0, 0, 0, IPOPT_RR, 3, 4, IPOPT_EOL];
    // SAFETY: a fresh mbuf has room for the eight bytes.
    unsafe { ptr::copy_nonoverlapping(o.as_ptr(), mtod::<u8>(opt), o.len()) };
    opt.m_len().set(o.len() as u32);
    let mut hlen = 0;
    let m = ip_insertoptions(m, opt, &mut hlen);
    assert_eq!(hlen, 24);
    assert_eq!(m.m_pkthdr().len.get(), 64);
    let b = bytes(m);
    assert_eq!(&b[2..4], &64u16.to_be_bytes(), "ip_len grew");
    assert_eq!(&b[20..24], &o[4..8]);
    assert_eq!(
        &b[16..20],
        &[10, 0, 2, 2],
        "the destination stays without a first hop"
    );
    m_freem(m);
    m_freem(opt);
}

/// An option mbuf holding `opts`.
fn optm(opts: &[u8]) -> &'static Mbuf {
    let m = m_get(M_DONTWAIT, MT_SOOPTS).expect("mbuf");
    m.m_len().set(opts.len() as u32);
    // SAFETY: a fresh mbuf of `MLEN` bytes, more than any option list here.
    unsafe { ptr::copy_nonoverlapping(opts.as_ptr(), mtod::<u8>(m), opts.len()) };
    m
}

#[test]
fn ip_pcbopts_pops_the_first_hop_of_a_source_route() {
    let _g = setup_net();
    let pcbopt = Cell::new(None);

    // NOP, then LSRR through 10.0.0.1 (first hop) and 10.0.0.2 as the user writes it (the
    // addresses where the pointer byte goes), then EOL: the first hop moves before the
    // options and the route shrinks by one address.
    let m = optm(&[
        IPOPT_NOP, IPOPT_LSRR, 10, 10, 0, 0, 1, 10, 0, 0, 2, IPOPT_EOL,
    ]);
    ip_pcbopts(&pcbopt, Some(m)).expect("options");
    let n = pcbopt.get().expect("stored");
    assert_eq!(
        mtod_bytes(n),
        &[
            10, 0, 0, 1, IPOPT_NOP, IPOPT_LSRR, 6, 10, 0, 0, 2, IPOPT_EOL
        ]
    );

    // A length that is not a multiple of 4, or an option longer than the list, is refused
    // (and the old options are gone either way).
    assert_eq!(
        ip_pcbopts(&pcbopt, Some(optm(&[IPOPT_NOP; 3]))),
        Err(Errno::EINVAL)
    );
    assert!(pcbopt.get().is_none());
    assert_eq!(
        ip_pcbopts(&pcbopt, Some(optm(&[IPOPT_RR, 9, 4, 0]))),
        Err(Errno::EINVAL)
    );
    // No mbuf only turns the options off.
    ip_pcbopts(&pcbopt, Some(m)).expect("options");
    ip_pcbopts(&pcbopt, None).expect("off");
    assert!(pcbopt.get().is_none());
    m_freem(m);
}

#[test]
fn ip_getmoptions_answers_the_defaults_without_options() {
    let _g = setup_net();
    let m = optm(&[0; 4]);
    ip_getmoptions(IP_MULTICAST_TTL, None, m).expect("ttl");
    assert_eq!(mtod_bytes(m), &[IP_DEFAULT_MULTICAST_TTL]);
    ip_getmoptions(IP_MULTICAST_LOOP, None, m).expect("loop");
    assert_eq!(mtod_bytes(m), &[IP_DEFAULT_MULTICAST_LOOP]);
    ip_getmoptions(IP_MULTICAST_IF, None, m).expect("if");
    assert_eq!(mtod_bytes(m), &[0, 0, 0, 0]);
    assert_eq!(ip_getmoptions(IP_TTL, None, m), Err(Errno::EOPNOTSUPP));
    m_freem(m);
}
