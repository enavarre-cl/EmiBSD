//! Host tests for ESP: the anti-replay window, and packets that go out through
//! `ipsp_process_packet` (the transform, `ipsp_process_done`, `ip_output` onto a test
//! Ethernet interface) and come back in through `esp46_input`, in transport and in tunnel
//! mode, with AES-CBC and HMAC-SHA2-256 and with AES-GCM, through the crypto framework's
//! software driver.

use std::sync::MutexGuard;
use std::vec::Vec;

use super::*;
use crate::crypto::crypto::crypto_reset;
use crate::crypto::cryptosoft::swcr_init;
use crate::net::ethertypes::{ETHERTYPE_ARP, ETHERTYPE_IP};
use crate::net::if_::tests::test_packet;
use crate::net::if_ethersubr::ether_input;
use crate::net::ifq::ifq_dequeue;
use crate::netinet::if_ether::{EtherArp, EtherHeader, arpintr};
use crate::netinet::in_::{IPPROTO_ICMP, IPPROTO_IPV4};
use crate::netinet::in_cksum::in_cksum;
use crate::netinet::ip_input::tests::{
    ADDR, GATEWAY, OURS, PEER, bytes, configure, frame, sin, test_ether,
};
use crate::netinet::ip_ipsp::{
    IPSP_DF_INHERIT, IpsecInit, SockaddrUnion, TDBF_TUNNELING, XF_ESP, ipsp_reset, puttdb,
    tdb_alloc, tdb_init,
};
use crate::netinet::ip_var::mtod_ip;
use crate::netinet::ipsec_input::{ESP_ENABLE, esp46_input};
use crate::netinet::ipsec_output::ipsp_process_packet;
use crate::sys::endian::htons;
use crate::sys::socket::AF_INET;

/// A TDB with replay window `wnd` and `rpl` as its counter, outside the tables.
fn window(esn: bool, rpl: u64) -> Tdb {
    let t = Tdb::new();
    t.tdb_wnd.set(32);
    t.tdb_rpl.set(rpl);
    if esn {
        t.set_flags(TDBF_ESN);
    }
    t
}

/// `checkreplaywindow` under the TDB's mutex; the high half of the sequence number too.
fn check(t: &Tdb, seq: u32, commit: bool) -> (i32, u32) {
    let mut seqh = 0;
    mtx_enter(&t.tdb_mtx);
    let r = checkreplaywindow(t, t.tdb_rpl.get(), seq, &mut seqh, commit);
    mtx_leave(&t.tdb_mtx);
    (r, seqh)
}

#[test]
fn the_replay_window_accepts_new_numbers_once() {
    let t = window(false, AH_HMAC_INITIAL_RPL);

    assert_eq!(check(&t, 0, true).0, 1, "sequence number 0 is never valid");
    assert_eq!(check(&t, 1, false).0, 0, "first packet: checked");
    assert_eq!(check(&t, 1, true).0, 0, "then committed");
    assert_eq!(check(&t, 1, true).0, 3, "a duplicate");
    assert_eq!(check(&t, 5, true).0, 0, "ahead: the window moves");
    assert_eq!(t.tdb_rpl.get(), 5);
    assert_eq!(
        check(&t, 3, true).0,
        0,
        "behind, inside the window, not seen"
    );
    assert_eq!(check(&t, 3, false).0, 3, "seen now");
    assert_eq!(check(&t, 4, false).0, 0, "not seen, not committed");
    assert_eq!(check(&t, 4, false).0, 0, "still not seen");

    // Far ahead: the window starts over; old numbers are rejected.
    let far = 5 + TDB_REPLAYMAX + 10;
    assert_eq!(check(&t, far, true).0, 0);
    assert_eq!(t.tdb_rpl.get(), u64::from(far));
    assert_eq!(check(&t, 5, true).0, 2, "too old");
    let edge = far - (TDB_REPLAYMAX - TDB_REPLAYWASTE) + 1;
    assert_eq!(
        check(&t, edge, true).0,
        0,
        "the oldest number the window still covers"
    );
    assert_eq!(check(&t, edge - 1, true).0, 2, "one older");

    // Without ESN the counter cannot wrap.
    let t = window(false, 0xffff_fff0);
    assert_eq!(check(&t, 0xffff_fff8, true).0, 0);
    assert_eq!(check(&t, 3, true).0, 2);
}

#[test]
fn with_esn_the_window_crosses_into_the_next_subspace() {
    let t = window(true, 0xffff_fff0);
    assert_eq!(check(&t, 0xffff_fffe, true), (0, 0));
    // Wrapped: the high half goes up.
    assert_eq!(check(&t, 2, true), (0, 1));
    assert_eq!(t.tdb_rpl.get(), (1 << 32) | 2);
    // A late packet of the previous subspace, inside the window.
    assert_eq!(check(&t, 0xffff_fff8, true), (0, 0));
    assert_eq!(
        check(&t, 0xffff_fff8, true).0,
        3,
        "a duplicate across the wrap"
    );
    assert_eq!(check(&t, 1, true), (0, 1));
}

/// Memory, the network with `tvio0` at 10.0.2.15/24, an empty SA database and the software
/// crypto driver (under the crypto tests' lock).
fn setup() -> (
    (MutexGuard<'static, ()>, MutexGuard<'static, ()>),
    MutexGuard<'static, ()>,
    &'static crate::net::if_var::Ifnet,
) {
    let g = crate::netinet::ip_input::tests::setup();
    let s = crate::crypto::testutil::serial();
    crypto_reset();
    swcr_init();
    ipsp_reset();
    ESP_ENABLE.store(1, core::sync::atomic::Ordering::Relaxed);
    let ifp = test_ether();
    configure(ifp, ADDR, [255, 255, 255, 0]);
    while let Some(m) = ifq_dequeue(&ifp.if_snd) {
        m_freem(m);
    }
    (g, s, ifp)
}

/// The cipher and authenticator of an SA.
#[derive(Clone, Copy)]
enum Suite {
    /// AES-128-CBC with HMAC-SHA2-256.
    CbcSha256,
    /// AES-128-GCM.
    Gcm,
}

const ENCKEY: [u8; 20] = [
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
    0xca, 0xfe, 0xba, 0xbe,
];
const AUTHKEY: [u8; 32] = [0x5a; 32];

/// An ESP SA from us (10.0.2.15) to the gateway, set up by `tdb_init` (as `SADB_ADD` does)
/// and put in the tables.
fn esp_sa(suite: Suite, tunnel: bool) -> &'static Tdb {
    let t = tdb_alloc(0);
    t.tdb_spi.set(htonl(0x1234));
    t.tdb_sproto.set(IPPROTO_ESP as u8);
    t.tdb_src.set(SockaddrUnion::from_sin(&sin(ADDR)));
    t.tdb_dst.set(SockaddrUnion::from_sin(&sin(GATEWAY)));
    t.tdb_wnd.set(16);
    if tunnel {
        t.set_flags(TDBF_TUNNELING);
    }
    let mut ii = match suite {
        Suite::CbcSha256 => IpsecInit {
            ii_encalg: SADB_X_EALG_AES,
            ii_enckey: &ENCKEY[..16],
            ii_enckeylen: 16,
            ii_authalg: SADB_X_AALG_SHA2_256,
            ii_authkey: &AUTHKEY,
            ii_authkeylen: 32,
            ..IpsecInit::default()
        },
        Suite::Gcm => IpsecInit {
            ii_encalg: SADB_X_EALG_AESGCM16,
            ii_enckey: &ENCKEY,
            ii_enckeylen: 20,
            ..IpsecInit::default()
        },
    };
    tdb_init(t, XF_ESP, &mut ii).expect("esp_init");
    puttdb(t);
    t
}

/// An ICMP echo request from `src` to `dst` with a recognisable payload.
fn icmp_packet(src: [u8; 4], dst: [u8; 4]) -> Vec<u8> {
    let mut p = std::vec![0u8; 20 + 8 + 37];
    p[0] = 0x45;
    let len = p.len() as u16;
    p[2..4].copy_from_slice(&len.to_be_bytes());
    p[6] = 0x40; // DF
    p[8] = 64;
    p[9] = IPPROTO_ICMP as u8;
    p[12..16].copy_from_slice(&src);
    p[16..20].copy_from_slice(&dst);
    p[20] = 8;
    for (i, b) in p[28..].iter_mut().enumerate() {
        *b = b"the quick brown fox jumps over IPsec"[i % 36];
    }
    // The header checksum (RFC 1071), computed here: no mbufs exist before the setup.
    let mut sum: u32 = p[..20]
        .chunks(2)
        .map(|w| u32::from(u16::from_be_bytes([w[0], w[1]])))
        .sum();
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    p[10..12].copy_from_slice(&(!(sum as u16)).to_be_bytes());
    p
}

/// What `ip_output` sent on `ifp`: the IP packet of the frame, answering the gateway's ARP
/// request first if the packet waits for it.
fn sent(ifp: &'static crate::net::if_var::Ifnet) -> Vec<u8> {
    let m = ifq_dequeue(&ifp.if_snd).expect("a frame");
    let b = bytes(m);
    m_freem(m);
    if b[12..14] == ETHERTYPE_ARP.to_be_bytes() {
        let reply = EtherArp {
            ea_hdr: crate::net::if_arp::Arphdr {
                ar_hrd: htons(crate::net::if_arp::ARPHRD_ETHER),
                ar_pro: htons(ETHERTYPE_IP),
                ar_hln: 6,
                ar_pln: 4,
                ar_op: htons(crate::net::if_arp::ARPOP_REPLY),
            },
            arp_sha: PEER,
            arp_spa: GATEWAY,
            arp_tha: OURS,
            arp_tpa: ADDR,
        };
        // SAFETY: an `ether_arp` is plain bytes.
        let arp = unsafe {
            core::slice::from_raw_parts(ptr::from_ref(&reply).cast::<u8>(), size_of::<EtherArp>())
        };
        ether_input(ifp, frame(ifp, OURS, ETHERTYPE_ARP, arp), None);
        arpintr();
        return sent(ifp);
    }
    assert_eq!(&b[12..14], &ETHERTYPE_IP.to_be_bytes());
    b[size_of::<EtherHeader>()..].to_vec()
}

/// Sends `inner` through the SA of `suite`/`tunnel`, checks the ESP packet on the wire, then
/// feeds it back through `esp46_input` with an inbound SA of the same keys and returns the
/// protocol it answers and the packet after it.
fn round_trip(suite: Suite, tunnel: bool, inner: &[u8]) -> (i32, Vec<u8>) {
    let (_g, _s, ifp) = setup();

    let out = esp_sa(suite, tunnel);
    let m = test_packet(inner);
    ipsp_process_packet(m, out, i32::from(AF_INET), false, IPSP_DF_INHERIT).expect("sent");
    assert_eq!(
        out.tdb_rpl.get(),
        AH_HMAC_INITIAL_RPL + 1,
        "one sequence number used"
    );
    let wire = sent(ifp);

    // On the wire: an IPv4 packet from us to the gateway carrying ESP, the SPI and sequence
    // number 1 after the header, and none of the plaintext.
    assert_eq!(wire[0], 0x45);
    assert_eq!(wire[9], IPPROTO_ESP as u8);
    assert_eq!(&wire[12..16], &ADDR);
    assert_eq!(&wire[16..20], &GATEWAY);
    assert_eq!(&wire[20..24], &0x1234u32.to_be_bytes());
    assert_eq!(&wire[24..28], &1u32.to_be_bytes());
    let fox = b"quick brown fox";
    assert!(!wire.windows(fox.len()).any(|w| w == fox), "encrypted");
    let m = test_packet(&wire);
    assert_eq!(in_cksum(m, 20), 0, "the outer header checksum");
    m_freem(m);

    // The way back: the same keys on an inbound SA (the outbound one gone).
    tdb_delete(out);
    let inb = esp_sa(suite, tunnel);
    let m = test_packet(&wire);
    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    let mut mp = Some(m);
    let mut off = 20;
    let prot = esp46_input(&mut mp, &mut off, IPPROTO_ESP, i32::from(AF_INET), None);
    let m = mp.expect("decrypted");
    assert_ne!(m.m_flags().get() & crate::sys::mbuf::M_CONF, 0);
    assert_ne!(m.m_flags().get() & crate::sys::mbuf::M_AUTH, 0);
    let back = bytes(m);
    m_freem(m);
    assert_eq!(
        inb.tdb_rpl.get(),
        1,
        "the window moved to the packet's number"
    );

    // A replay of the same packet is dropped.
    let m = test_packet(&wire);
    let mut mp = Some(m);
    let mut off = 20;
    let again = esp46_input(&mut mp, &mut off, IPPROTO_ESP, i32::from(AF_INET), None);
    assert_eq!(again, crate::netinet::in_::IPPROTO_DONE);
    assert!(mp.is_none());

    // A damaged packet fails authentication.
    let mut bad = wire.clone();
    let n = bad.len();
    bad[n - 20] ^= 1;
    bad[24..28].copy_from_slice(&2u32.to_be_bytes());
    let m = test_packet(&bad);
    let mut mp = Some(m);
    let mut off = 20;
    let r = esp46_input(&mut mp, &mut off, IPPROTO_ESP, i32::from(AF_INET), None);
    assert_eq!(r, crate::netinet::in_::IPPROTO_DONE);

    tdb_delete(inb);
    (prot, back)
}

#[test]
fn esp_cbc_sha256_transport_mode_round_trip() {
    let inner = icmp_packet(ADDR, GATEWAY);
    let (prot, back) = round_trip(Suite::CbcSha256, false, &inner);
    assert_eq!(prot, IPPROTO_ICMP, "the next protocol is restored");
    assert_eq!(back.len(), inner.len());
    assert_eq!(&back[20..], &inner[20..], "the payload");
    let ip = mtod_ip(test_packet(&back));
    assert_eq!(ip.ip_p, IPPROTO_ICMP as u8);
    assert_eq!(usize::from(u16::from_be(ip.ip_len)), inner.len());
}

#[test]
fn esp_gcm_transport_mode_round_trip() {
    let inner = icmp_packet(ADDR, GATEWAY);
    let (prot, back) = round_trip(Suite::Gcm, false, &inner);
    assert_eq!(prot, IPPROTO_ICMP);
    assert_eq!(&back[20..], &inner[20..]);
}

#[test]
fn esp_cbc_sha256_tunnel_mode_round_trip() {
    let inner = icmp_packet([10, 77, 1, 1], [10, 77, 2, 1]);
    let (prot, back) = round_trip(Suite::CbcSha256, true, &inner);
    assert_eq!(prot, IPPROTO_IPV4, "an IP packet inside");
    assert_eq!(back[9], IPPROTO_IPV4 as u8, "the outer header says so");
    assert_eq!(&back[20..], &inner[..], "the inner packet, untouched");
}

#[test]
fn esp_gcm_tunnel_mode_round_trip() {
    let inner = icmp_packet([10, 77, 1, 1], [10, 77, 2, 1]);
    let (prot, back) = round_trip(Suite::Gcm, true, &inner);
    assert_eq!(prot, IPPROTO_IPV4);
    assert_eq!(&back[20..], &inner[..]);
}

#[test]
fn ipsec_hdrsz_counts_the_overhead() {
    let (_g, _s, _ifp) = setup();
    let t = esp_sa(Suite::CbcSha256, true);
    // SPI + sequence (8) + IV (16) + authenticator (16) + padding (16) + outer header (20).
    assert_eq!(
        crate::netinet::ipsec_output::ipsec_hdrsz(t),
        8 + 16 + 16 + 16 + 20
    );
    tdb_delete(t);
}
