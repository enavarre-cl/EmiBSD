//! Host tests for `ip6_output.c`: header splitting, fragment header insertion and
//! fragmentation of synthetic mbuf chains, the jumbo payload option, the packet option
//! parser (`ip6_setpktopt`, `ip6_setpktopts`) and the pseudo-header sum.

use std::sync::MutexGuard;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::{m_cat, m_copydata};
use crate::netinet::ip6::IPV6_VERSION;

fn setup() -> MutexGuard<'static, ()> {
    crate::kern::uipc_mbuf::tests::setup()
}

/// An IPv6 header for `plen` payload bytes of protocol `nxt`.
fn hdr(plen: u16, nxt: u8) -> Ip6Hdr {
    let mut ip6 = Ip6Hdr::zeroed();
    ip6.set_ip6_vfc(IPV6_VERSION);
    ip6.ip6_plen = htons(plen);
    ip6.ip6_nxt = nxt;
    ip6.ip6_hlim = 64;
    ip6.ip6_src.s6_addr = [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    ip6.ip6_dst.s6_addr = [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];
    ip6
}

/// A packet: the header in a first mbuf of its own, then `payload` in mbufs of at most
/// `piece` bytes.
fn packet(ip6: &Ip6Hdr, payload: &[u8], piece: usize) -> &'static Mbuf {
    let head = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
    head.m_len().set(size_of::<Ip6Hdr>() as u32);
    mtod_ip6_store(head, ip6);
    for chunk in payload.chunks(piece) {
        let m = m_get(M_DONTWAIT, MT_DATA).expect("mbuf");
        // SAFETY: `piece` is at most `MLEN`, the room of a fresh mbuf.
        unsafe { ptr::copy_nonoverlapping(chunk.as_ptr(), mtod::<u8>(m), chunk.len()) };
        m.m_len().set(chunk.len() as u32);
        m_cat(head, Some(m));
    }
    head.m_pkthdr()
        .len
        .set((size_of::<Ip6Hdr>() + payload.len()) as i32);
    head
}

/// The bytes `off..off + len` of a chain.
fn bytes(m: &Mbuf, off: i32, len: i32) -> Vec<u8> {
    let mut v = vec![0u8; len as usize];
    m_copydata(m, off, &mut v);
    v
}

#[test]
fn ip6_splithdr_moves_header_to_own_mbuf() {
    let _g = setup();
    let ip6 = hdr(20, IPPROTO_UDP as u8);
    let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
    m.m_len().set(60);
    mtod_ip6_store(m, &ip6);
    // SAFETY: the mbuf holds 60 bytes; the payload follows the header.
    unsafe { ptr::write_bytes(mtod::<u8>(m).add(40), 0xab, 20) };
    m.m_pkthdr().len.set(60);

    let mut ex = Ip6Exthdrs::default();
    ip6_splithdr(m, &mut ex).expect("split");
    let h = ex.ip6e_ip6.expect("header mbuf");
    assert!(!ptr::eq(h, m));
    assert_eq!(h.m_len().get(), 40);
    assert_eq!(h.m_pkthdr().len.get(), 60);
    assert_eq!(mtod_ip6(h), ip6);
    let n = h.m_next().get().expect("payload");
    assert!(ptr::eq(n, m));
    assert_eq!(n.m_len().get(), 20);
    assert_eq!(bytes(h, 40, 20), vec![0xab; 20]);
    m_freem(h);

    // A header alone in its mbuf stays where it is.
    let m = packet(&ip6, &[], 1);
    let mut ex = Ip6Exthdrs::default();
    ip6_splithdr(m, &mut ex).expect("split");
    assert!(ex.ip6e_ip6.is_some_and(|h| ptr::eq(h, m)));
    m_freem(m);
}

#[test]
fn ip6_insertfraghdr_uses_trailing_space() {
    let _g = setup();
    let ip6 = hdr(8, IPPROTO_DSTOPTS as u8);
    let m0 = packet(&ip6, &[17, 0, 1, 4, 0, 0, 0, 0], 8);
    let m = m_gethdr(M_DONTWAIT, MT_HEADER).expect("mbuf");
    m.m_len().set(40);
    m.m_pkthdr().len.set(40);

    // Unfragmentable part is the header alone: the fragment header follows it in `m`.
    let p = ip6_insertfraghdr(m0, m, 40).expect("fraghdr");
    // SAFETY: pointer arithmetic within the same mbuf.
    assert_eq!(p, unsafe { mtod::<u8>(m).add(40) });
    assert_eq!(m.m_len().get(), 48);
    assert_eq!(m.m_pkthdr().len.get(), 48);
    assert!(m.m_next().get().is_none());
    m_freem(m);

    // With a destination options header the copy of it carries the fragment header.
    let m = m_gethdr(M_DONTWAIT, MT_HEADER).expect("mbuf");
    m.m_len().set(40);
    m.m_pkthdr().len.set(40);
    let p = ip6_insertfraghdr(m0, m, 48).expect("fraghdr");
    let n = m.m_next().get().expect("copied exthdr");
    assert_eq!(n.m_len().get(), 16);
    // SAFETY: as above.
    assert_eq!(p, unsafe { mtod::<u8>(n).add(8) });
    assert_eq!(bytes(n, 0, 8), vec![17, 0, 1, 4, 0, 0, 0, 0]);
    m_freem(m);
    m_freem(m0);
}

#[test]
fn ip6_fragment_sizes_offsets_and_more_flag() {
    let _g = setup();
    ip6_randomid_init();
    let payload: Vec<u8> = (0..3000u32).map(|i| (i * 7) as u8).collect();
    let ip6 = hdr(3000, IPPROTO_UDP as u8);
    let mut ip6f = ip6;
    ip6f.ip6_nxt = IPPROTO_FRAGMENT as u8;
    let m0 = packet(&ip6f, &payload, 200);
    let ml = MbufList::new();

    ip6_fragment(m0, &ml, 40, IPPROTO_UDP as u8, 1280).expect("fragment");
    assert_eq!(ml_len(&ml), 3);

    // (1280 - 40 - 8) & ~7 = 1232 bytes per fragment.
    let expect = [(0, 1232, true), (1232, 1232, true), (2464, 536, false)];
    let mut ident = None;
    for &(off, len, more) in &expect {
        let f = ml_dequeue(&ml).expect("fragment");
        assert_eq!(f.m_pkthdr().len.get(), 48 + len);
        let h = mtod_ip6(f);
        assert_eq!(ntohs(h.ip6_plen) as i32, 8 + len);
        assert_eq!(h.ip6_nxt, IPPROTO_FRAGMENT as u8);
        assert_eq!(h.ip6_src, ip6.ip6_src);
        let fb = bytes(f, 40, 8);
        let frag = Ip6Frag {
            ip6f_nxt: fb[0],
            ip6f_reserved: fb[1],
            ip6f_offlg: u16::from_ne_bytes([fb[2], fb[3]]),
            ip6f_ident: u32::from_ne_bytes([fb[4], fb[5], fb[6], fb[7]]),
        };
        assert_eq!(frag.ip6f_nxt, IPPROTO_UDP as u8);
        assert_eq!(frag.ip6f_reserved, 0);
        assert_eq!(ntohs(frag.ip6f_offlg & !IP6F_MORE_FRAG) as i32, off);
        assert_eq!(frag.ip6f_offlg & IP6F_MORE_FRAG != 0, more);
        assert_eq!(*ident.get_or_insert(frag.ip6f_ident), frag.ip6f_ident);
        assert_eq!(
            bytes(f, 48, len),
            payload[off as usize..(off + len) as usize].to_vec()
        );
        m_freem(f);
    }
}

#[test]
fn ip6_fragment_mtu_too_small() {
    let _g = setup();
    let m0 = packet(&hdr(100, IPPROTO_UDP as u8), &[0u8; 100], 100);
    let ml = MbufList::new();
    // (55 - 40 - 8) & ~7 = 0 < 8.
    assert_eq!(
        ip6_fragment(m0, &ml, 40, IPPROTO_UDP as u8, 55),
        Err(Errno::EMSGSIZE)
    );
    assert_eq!(ml_len(&ml), 0);
}

#[test]
fn ip6_insert_jumboopt_builds_hbh() {
    let _g = setup();
    let m = packet(&hdr(0, IPPROTO_UDP as u8), &[], 1);
    let mut ex = Ip6Exthdrs {
        ip6e_ip6: Some(m),
        ..Ip6Exthdrs::default()
    };
    ip6_insert_jumboopt(&mut ex, 70000).expect("jumbo");
    let h = ex.ip6e_hbh.expect("hbh");
    assert_eq!(h.m_len().get(), 8);
    let b = bytes(h, 0, 8);
    assert_eq!(b[1], 0);
    assert_eq!(&b[2..4], &[IP6OPT_JUMBO, 4]);
    assert_eq!(u32::from_be_bytes([b[4], b[5], b[6], b[7]]), 70008);
    assert_eq!(m.m_pkthdr().len.get(), 48);

    // A second insertion appends PadN and the option to the existing header.
    ip6_insert_jumboopt(&mut ex, 70000).expect("jumbo");
    let h = ex.ip6e_hbh.expect("hbh");
    assert_eq!(h.m_len().get(), 16);
    let b = bytes(h, 0, 16);
    assert_eq!(b[1], 1); // ip6h_len
    assert_eq!(&b[8..12], &[IP6OPT_PADN, 0, IP6OPT_JUMBO, 4]);
    m_freem(h);
    m_freem(m);
}

#[test]
fn ip6_setpktopt_validation() {
    let _g = setup();
    let mut opt = Ip6Pktopts::default();
    ip6_initpktopts(&mut opt);
    assert_eq!((opt.ip6po_hlim, opt.ip6po_tclass), (-1, -1));
    assert_eq!(opt.ip6po_minmtu, IP6PO_MINMTU_MCASTONLY);
    let int = |v: i32| v.to_ne_bytes();
    let udp = IPPROTO_UDP;

    // Hop limit: not sticky, an int in -1..=255.
    let r = ip6_setpktopt(IPV6_HOPLIMIT, &int(64), &mut opt, false, true, udp);
    assert_eq!(r, Err(Errno::ENOPROTOOPT));
    assert_eq!(
        ip6_setpktopt(IPV6_HOPLIMIT, &int(64), &mut opt, false, false, udp),
        Ok(())
    );
    assert_eq!(opt.ip6po_hlim, 64);
    let r = ip6_setpktopt(IPV6_HOPLIMIT, &int(256), &mut opt, false, false, udp);
    assert_eq!(r, Err(Errno::EINVAL));
    let r = ip6_setpktopt(IPV6_HOPLIMIT, &[1, 2], &mut opt, false, false, udp);
    assert_eq!(r, Err(Errno::EINVAL));

    // Traffic class.
    assert_eq!(
        ip6_setpktopt(IPV6_TCLASS, &int(0x2e), &mut opt, false, true, udp),
        Ok(())
    );
    assert_eq!(opt.ip6po_tclass, 0x2e);
    let r = ip6_setpktopt(IPV6_TCLASS, &int(-2), &mut opt, false, true, udp);
    assert_eq!(r, Err(Errno::EINVAL));

    // Minimum MTU policy and don't-fragment.
    let r = ip6_setpktopt(IPV6_USE_MIN_MTU, &int(5), &mut opt, false, true, udp);
    assert_eq!(r, Err(Errno::EINVAL));
    assert_eq!(
        ip6_setpktopt(IPV6_USE_MIN_MTU, &int(1), &mut opt, false, true, udp),
        Ok(())
    );
    assert_eq!(opt.ip6po_minmtu, IP6PO_MINMTU_ALL);
    assert_eq!(
        ip6_setpktopt(IPV6_DONTFRAG, &int(1), &mut opt, false, true, udp),
        Ok(())
    );
    assert_ne!(opt.ip6po_flags & IP6PO_DONTFRAG, 0);
    let r = ip6_setpktopt(IPV6_DONTFRAG, &int(1), &mut opt, false, true, IPPROTO_TCP);
    assert_eq!(r, Ok(()));
    assert_eq!(opt.ip6po_flags & IP6PO_DONTFRAG, 0);

    // Packet info: length, TCP sticky address.
    let r = ip6_setpktopt(IPV6_PKTINFO, &[0; 16], &mut opt, false, true, udp);
    assert_eq!(r, Err(Errno::EINVAL));
    let mut pi = [0u8; 20];
    pi[0] = 0x20;
    let r = ip6_setpktopt(IPV6_PKTINFO, &pi, &mut opt, false, true, IPPROTO_TCP);
    assert_eq!(r, Err(Errno::EINVAL));
    assert_eq!(
        ip6_setpktopt(IPV6_PKTINFO, &pi, &mut opt, false, true, udp),
        Ok(())
    );
    let stored = opt.ip6po_pktinfo.expect("pktinfo");
    // SAFETY: the option's own allocation.
    assert_eq!(unsafe { stored.as_ptr().read() }.ipi6_addr.s6_addr[0], 0x20);
    // in6addr_any and index zero clear it.
    assert_eq!(
        ip6_setpktopt(IPV6_PKTINFO, &[0; 20], &mut opt, false, true, udp),
        Ok(())
    );
    assert!(opt.ip6po_pktinfo.is_none());

    // Hop-by-hop options: privileged, length matching ip6h_len.
    let hbh = [0u8, 0, IP6OPT_PADN, 4, 0, 0, 0, 0];
    let r = ip6_setpktopt(IPV6_HOPOPTS, &hbh, &mut opt, false, true, udp);
    assert_eq!(r, Err(Errno::EPERM));
    let r = ip6_setpktopt(IPV6_HOPOPTS, &hbh[..6], &mut opt, true, true, udp);
    assert_eq!(r, Err(Errno::EINVAL));
    assert_eq!(
        ip6_setpktopt(IPV6_HOPOPTS, &hbh, &mut opt, true, true, udp),
        Ok(())
    );
    assert!(opt.ip6po_hbh.is_some());
    let r = ip6_setpktopt(IPV6_RTHDRDSTOPTS, &hbh, &mut opt, true, true, udp);
    assert_eq!(r, Ok(()));
    assert!(opt.ip6po_dest1.is_some() && opt.ip6po_dest2.is_none());

    // A copy duplicates the allocations; clearing frees them.
    let mut copy = Ip6Pktopts::default();
    ip6_initpktopts(&mut copy);
    copypktopts(&mut copy, &opt).expect("copy");
    assert!(copy.ip6po_hbh.is_some() && copy.ip6po_hbh != opt.ip6po_hbh);
    ip6_clearpktopts(&mut copy, -1);
    ip6_clearpktopts(&mut opt, -1);
    assert!(opt.ip6po_hbh.is_none() && opt.ip6po_dest1.is_none());
    assert_eq!((opt.ip6po_hlim, opt.ip6po_tclass), (-1, -1));
}

#[test]
fn ip6_setpktopts_walks_cmsgs() {
    let _g = setup();
    let hdrlen = cmsg_len(0);
    let mut buf = Vec::new();
    for (level, type_, v) in [
        (IPPROTO_IPV6, IPV6_HOPLIMIT, 7i32),
        (IPPROTO_TCP, 1, 0),
        (IPPROTO_IPV6, IPV6_TCLASS, 3),
    ] {
        let len = (hdrlen + 4) as u32;
        buf.extend_from_slice(&len.to_ne_bytes());
        buf.extend_from_slice(&level.to_ne_bytes());
        buf.extend_from_slice(&type_.to_ne_bytes());
        buf.resize(buf.len() + hdrlen - 12, 0);
        buf.extend_from_slice(&v.to_ne_bytes());
        buf.resize(cmsg_align(buf.len()), 0);
    }
    let control = m_get(M_DONTWAIT, MT_DATA).expect("mbuf");
    // SAFETY: the control messages fit a fresh mbuf.
    unsafe { ptr::copy_nonoverlapping(buf.as_ptr(), mtod::<u8>(control), buf.len()) };
    control.m_len().set(buf.len() as u32);

    let mut opt = Ip6Pktopts::default();
    assert_eq!(
        ip6_setpktopts(control, &mut opt, None, false, IPPROTO_UDP),
        Ok(())
    );
    assert_eq!((opt.ip6po_hlim, opt.ip6po_tclass), (7, 3));

    // A truncated message is rejected.
    control.m_len().set(buf.len() as u32 - 2);
    let r = ip6_setpktopts(control, &mut opt, None, false, IPPROTO_UDP);
    assert_eq!(r, Err(Errno::EINVAL));
    m_freem(control);
}

#[test]
fn in6_cksum_phdr_skips_embedded_scope() {
    let ip6 = hdr(8, IPPROTO_UDP as u8);
    let sum = in6_cksum_phdr(&ip6.ip6_src, &ip6.ip6_dst, htonl(8), htonl(17));
    // 2001+0db8 twice, 1 + 2, length 8 and protocol 17, folded (network-order words).
    let expect: u32 = 2 * (0x2001 + 0x0db8) + 1 + 2 + 8 + 17;
    assert_eq!(ntohs(sum), expect as u16);

    // fe80::1%5 with the scope in word 1: the word is left out.
    let mut a = In6Addr::default();
    a.s6_addr[0] = 0xfe;
    a.s6_addr[1] = 0x80;
    a.s6_addr[3] = 5;
    a.s6_addr[15] = 1;
    let mut b = a;
    b.s6_addr[3] = 0;
    assert_eq!(
        in6_cksum_phdr(&a, &a, htonl(8), htonl(17)),
        in6_cksum_phdr(&b, &b, htonl(8), htonl(17))
    );
}
