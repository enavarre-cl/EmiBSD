//! Host tests for the Ethernet layer: address helpers, the CRCs, `ether_input`'s filtering
//! on crafted frames, the multicast list and header resolution, over a zero-filled `struct
//! arpcom` attached with `ether_ifattach`.

use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::MBPOOL;
use crate::net::ethertypes::ETHERTYPE_IPV6;
use crate::net::if_::tests::{setup_net, test_ioctl, test_packet, zeroed_static};
use crate::net::if_::{IFF_BROADCAST, IFNAMSIZ};
use crate::netinet::if_ether::EtherMultistep;
use crate::netinet::if_ether::ether_first_multi;
use crate::netinet::if_ether::ether_next_multi;

const OURS: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
const PEER: [u8; 6] = [0x52, 0x55, 0x0a, 0x00, 0x02, 0x02];

/// An Ethernet interface `teth<n>` with our address, attached to the Ethernet layer.
fn test_arpcom() -> &'static Arpcom {
    // SAFETY: the all-zero `Arpcom` is valid (`netinet/if_ether.rs`).
    let ac: &'static Arpcom = unsafe { zeroed_static() };
    let mut xname = [0u8; IFNAMSIZ];
    xname[..5].copy_from_slice(b"teth0");
    ac.ac_if.if_xname.set(xname);
    ac.ac_if.if_ioctl.set(Some(test_ioctl));
    ac.ac_if
        .if_flags
        .set(IFF_BROADCAST | IFF_SIMPLEX | IFF_MULTICAST | IFF_RUNNING);
    ac.ac_enaddr.set(OURS);
    ether_ifattach(ac);
    ac
}

/// A frame from `src` to `dst` of type `etype` with a few payload bytes.
fn frame(dst: [u8; 6], src: [u8; 6], etype: u16) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&dst);
    f.extend_from_slice(&src);
    f.extend_from_slice(&etype.to_be_bytes());
    f.extend_from_slice(&[
        0x45, 0, 0, 20, 0, 0, 0, 0, 64, 1, 0, 0, 10, 0, 2, 2, 10, 0, 2, 15,
    ]);
    f
}

#[test]
fn sprintf_and_e64_helpers() {
    let s = ether_sprintf(&[0x00, 0x1b, 0x21, 0xab, 0xcd, 0xef]);
    assert_eq!(&s[..17], b"00:1b:21:ab:cd:ef");
    assert_eq!(s[17], 0);

    let e64 = ether_addr_to_e64(&OURS);
    assert_eq!(e64, 0x5254_0012_3456);
    let mut back = [0u8; 6];
    ether_e64_to_addr(&mut back, e64);
    assert_eq!(back, OURS);
    assert!(eth64_is_broadcast(ether_addr_to_e64(&ETHERBROADCASTADDR)));
    assert_eq!(ether_addr_to_e64(&ETHERANYADDR), 0);
}

/// The C's `#if 0` reference versions, bit by bit.
fn crc32_le_bitwise(mut crc: u32, buf: &[u8]) -> u32 {
    for &b in buf {
        let mut c = u32::from(b);
        for _ in 0..8 {
            let carry = (crc & 1) ^ (c & 1);
            crc >>= 1;
            c >>= 1;
            if carry != 0 {
                crc ^= CRC_POLY_LE;
            }
        }
    }
    crc
}

fn crc32_be_bitwise(mut crc: u32, buf: &[u8]) -> u32 {
    for &b in buf {
        let mut c = u32::from(b);
        for _ in 0..8 {
            let carry = ((crc >> 31) & 1) ^ (c & 1);
            crc <<= 1;
            c >>= 1;
            if carry != 0 {
                crc = (crc ^ 0x04c1_1db6) | carry;
            }
        }
    }
    crc
}

#[test]
fn crc_tables_and_values_match_the_c() {
    assert_eq!(
        CRCTAB_LE,
        [
            0x00000000, 0x1db71064, 0x3b6e20c8, 0x26d930ac, 0x76dc4190, 0x6b6b51f4, 0x4db26158,
            0x5005713c, 0xedb88320, 0xf00f9344, 0xd6d6a3e8, 0xcb61b38c, 0x9b64c2b0, 0x86d3d2d4,
            0xa00ae278, 0xbdbdf21c,
        ]
    );
    assert_eq!(
        CRCTAB_BE,
        [
            0x00000000, 0x04c11db7, 0x09823b6e, 0x0d4326d9, 0x130476dc, 0x17c56b6b, 0x1a864db2,
            0x1e475005, 0x2608edb8, 0x22c9f00f, 0x2f8ad6d6, 0x2b4bcb61, 0x350c9b64, 0x31cd86d3,
            0x3c8ea00a, 0x384fbdbd,
        ]
    );
    assert_eq!(
        REV,
        [
            0x0, 0x8, 0x4, 0xc, 0x2, 0xa, 0x6, 0xe, 0x1, 0x9, 0x5, 0xd, 0x3, 0xb, 0x7, 0xf
        ]
    );

    // The standard check value: CRC-32 of "123456789" is 0xcbf43926 after the final
    // inversion, which ether_crc32_le leaves to its callers.
    assert_eq!(ether_crc32_le(b"123456789"), !0xcbf4_3926);
    for buf in [&b""[..], b"a", b"123456789", &OURS, &[0xffu8; 64]] {
        assert_eq!(ether_crc32_le(buf), crc32_le_bitwise(0xffff_ffff, buf));
        assert_eq!(ether_crc32_be(buf), crc32_be_bitwise(0xffff_ffff, buf));
    }
}

#[test]
fn ether_ifattach_sets_up_the_interface() {
    let _g = setup_net();
    let ac = test_arpcom();
    let ifp = &ac.ac_if;
    assert!(ifp.is_arpcom());
    assert_eq!(ifp.if_type.get(), IFT_ETHER);
    assert_eq!(usize::from(ifp.if_addrlen.get()), ETHER_ADDR_LEN);
    assert_eq!(usize::from(ifp.if_hdrlen.get()), ETHER_HDR_LEN);
    assert_eq!(ifp.if_mtu.get() as usize, ETHERMTU);
    assert_eq!(ifp.if_hardmtu.get() as usize, ETHERMTU);
    assert!(ifp.if_input.get().is_some() && ifp.if_output.get().is_some());
    let sdl = ifp.if_sadl.get();
    // SAFETY: if_alloc_sadl's allocation, with the name and then the address.
    unsafe {
        assert_eq!((*sdl).sdl_family, crate::sys::socket::AF_LINK);
        assert_eq!((*sdl).sdl_nlen, 5);
        let data = (*sdl).sdl_data;
        assert_eq!(&data[..5], b"teth0");
        assert_eq!(core::slice::from_raw_parts(lladdr(sdl), 6), &OURS);
    }

    // A multicast hardware address is replaced by a locally administered one.
    // SAFETY: the all-zero `Arpcom` is valid.
    let bad: &'static Arpcom = unsafe { zeroed_static() };
    bad.ac_enaddr.set([0x01, 0, 0, 0, 0, 1]);
    ether_ifattach(bad);
    let fake = bad.ac_enaddr.get();
    assert_eq!(&fake[..3], &[0xfe, 0xe1, 0xba]);
    assert_eq!(fake[3] & 0xf0, 0xd0);
}

#[test]
fn ether_input_filters_and_frees_every_frame() {
    let _g = setup_net();
    let ac = test_arpcom();
    let ifp = &ac.ac_if;
    let before = MBPOOL.pr_nout.get();
    let input = |bytes: &[u8]| ether_input(ifp, test_packet(bytes), None);

    // Too short for an Ethernet header.
    input(&[0u8; 10]);
    // To us: the IPv4 input (not ported yet) is where it would go.
    input(&frame(OURS, PEER, ETHERTYPE_IP));
    assert_eq!(ifp.if_imcasts().get(), 0);
    // Unicast for someone else.
    input(&frame([0x52, 0x54, 0, 0, 0, 1], PEER, ETHERTYPE_IP));
    assert_eq!(ifp.if_imcasts().get(), 0);
    // Broadcast ARP from a peer counts as a multicast reception.
    input(&frame(ETHERBROADCASTADDR, PEER, ETHERTYPE_ARP));
    assert_eq!(ifp.if_imcasts().get(), 1);
    // A VLAN-tagged frame without vlan(4) is service delimited: dropped.
    input(&frame(OURS, PEER, ETHERTYPE_VLAN));
    // An IPv6 multicast (not configured) is counted, then dropped by the demux.
    input(&frame([0x33, 0x33, 0, 0, 0, 1], PEER, ETHERTYPE_IPV6));
    assert_eq!(ifp.if_imcasts().get(), 2);
    // Our own multicast on a non-simplex interface comes back from the wire: dropped
    // before it is counted.
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_SIMPLEX);
    input(&frame(ETHERBROADCASTADDR, OURS, ETHERTYPE_ARP));
    assert_eq!(ifp.if_imcasts().get(), 2);
    // ARP on an IFF_NOARP interface.
    ifp.if_flags.set(ifp.if_flags.get() | IFF_NOARP);
    input(&frame(ETHERBROADCASTADDR, PEER, ETHERTYPE_ARP));
    assert_eq!(ifp.if_imcasts().get(), 3);

    assert_eq!(MBPOOL.pr_nout.get(), before, "every frame was freed");
}

/// A request whose address is `sa` (16 bytes).
fn ifreq_with(family: u8, data: &[u8]) -> Ifreq {
    let mut ifr = Ifreq::zeroed();
    let sa = ifr.ifr_addr_mut();
    sa.sa_len = 16;
    sa.sa_family = family;
    sa.sa_data[..data.len()].copy_from_slice(data);
    ifr
}

#[test]
fn multicast_list_counts_claims() {
    let _g = setup_net();
    let ac = test_arpcom();
    let group = [0x01, 0x00, 0x5e, 0x00, 0x00, 0x01];
    let ifr = ifreq_with(AF_UNSPEC, &group);

    assert_eq!(
        ether_addmulti(&ifr, ac),
        Err(Errno::ENETRESET),
        "list changed"
    );
    assert_eq!(ether_addmulti(&ifr, ac), Ok(()), "a second claim");
    assert_eq!(ac.ac_multicnt.get(), 1);
    assert_eq!(ac.ac_multirangecnt.get(), 0);

    // INADDR_ANY means the whole IPv4 multicast range.
    let any = ifreq_with(AF_INET, &[0, 0, 0, 0, 0, 0]);
    assert_eq!(ether_addmulti(&any, ac), Err(Errno::ENETRESET));
    assert_eq!(ac.ac_multirangecnt.get(), 1);
    let mut step = EtherMultistep { e_enm: None };
    let mut n = 0;
    let mut enm = ether_first_multi(&mut step, ac);
    while let Some(e) = enm {
        n += 1;
        enm = ether_next_multi(&mut step);
        let _ = e;
    }
    assert_eq!(n, 2);

    // Unicast is not a multicast address.
    assert_eq!(
        ether_addmulti(&ifreq_with(AF_UNSPEC, &OURS), ac),
        Err(Errno::EINVAL)
    );

    assert_eq!(ether_delmulti(&ifr, ac), Ok(()), "a claim remains");
    assert_eq!(ether_delmulti(&ifr, ac), Err(Errno::ENETRESET), "gone");
    assert_eq!(ether_delmulti(&ifr, ac), Err(Errno::ENXIO));
    assert_eq!(ether_delmulti(&any, ac), Err(Errno::ENETRESET));
    assert_eq!(ac.ac_multicnt.get(), 0);
    assert_eq!(ac.ac_multirangecnt.get(), 0);
}

#[test]
fn resolve_and_encap_from_a_link_sockaddr() {
    let _g = setup_net();
    let ac = test_arpcom();
    let ifp = &ac.ac_if;
    let before = MBPOOL.pr_nout.get();

    // AF_UNSPEC: destination and type from the sockaddr, source from the interface.
    let mut header = Vec::new();
    header.extend_from_slice(&PEER);
    header.extend_from_slice(&[0; 6]);
    header.extend_from_slice(&ETHERTYPE_ARP.to_be_bytes());
    let ifr = ifreq_with(AF_UNSPEC, &header);
    let dst: *const Sockaddr = ifr.ifr_addr();

    let m = test_packet(&[0xaa; 28]);
    // SAFETY: `dst` is a 16-byte sockaddr.
    let m = unsafe { ether_encap(ifp, m, dst, None) }
        .expect("resolved")
        .expect("not deferred");
    assert_eq!(m.m_pkthdr().len.get(), 28 + 14);
    let eh = ether_header(m);
    assert_eq!(eh.ether_dhost, PEER);
    assert_eq!(eh.ether_shost, OURS);
    assert_eq!(ntohs(eh.ether_type), ETHERTYPE_ARP);
    m_freem(m);

    // A complete header is taken as it is.
    let mut eh = EtherHeader::default();
    let hdrcmplt = {
        let mut ifr = ifreq_with(pseudo_AF_HDRCMPLT, &header);
        ifr.ifr_addr_mut().sa_data[6..12].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        ifr
    };
    let m = test_packet(&[0; 4]);
    // SAFETY: as above.
    assert_eq!(
        unsafe { ether_resolve(ifp, m, hdrcmplt.ifr_addr(), None, &mut eh) },
        Ok(())
    );
    assert_eq!(eh.ether_shost, [1, 2, 3, 4, 5, 6]);
    m_freem(m);

    // Not running: ENETDOWN, and the packet is freed.
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);
    let m = test_packet(&[0; 4]);
    // SAFETY: as above.
    assert_eq!(
        unsafe { ether_resolve(ifp, m, dst, None, &mut eh) },
        Err(Errno::ENETDOWN)
    );
    assert_eq!(MBPOOL.pr_nout.get(), before);
}

#[test]
fn extract_headers_of_an_ipv4_udp_frame() {
    let _g = setup_net();
    let mut f = Vec::new();
    f.extend_from_slice(&OURS);
    f.extend_from_slice(&PEER);
    f.extend_from_slice(&ETHERTYPE_IP.to_be_bytes());
    // IPv4, 20-byte header, total length 28, UDP.
    f.extend_from_slice(&[
        0x45, 0, 0, 28, 0, 0, 0, 0, 64, 17, 0, 0, 10, 0, 2, 2, 10, 0, 2, 15,
    ]);
    f.extend_from_slice(&[0x12, 0x34, 0x00, 0x35, 0x00, 0x08, 0x00, 0x00]);
    let m = test_packet(&f);

    let mut ext = EtherExtracted::new();
    ether_extract_headers(m, &mut ext);
    assert!(!ext.eh.is_null() && !ext.ip4.is_null() && !ext.udp.is_null());
    assert!(ext.tcp.is_null() && ext.ip6.is_null());
    assert_eq!(ext.iplen, 28);
    assert_eq!(ext.iphlen, 20);
    assert_eq!(ext.paylen, 8);

    // A fragment: the transport header is not looked at.
    // SAFETY: the byte at ip_off in the mbuf's data.
    unsafe { mtod::<u8>(m).add(14 + 6).write(0x20) };
    ether_extract_headers(m, &mut ext);
    assert!(!ext.ip4.is_null() && ext.udp.is_null());
    m_freem(m);
}
