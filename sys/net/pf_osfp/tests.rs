//! Host tests for OS fingerprinting: adding fingerprints, matching a crafted SYN (from its
//! headers and through a packet descriptor), reading them back by number, and flushing.

use std::sync::{MutexGuard, Once};
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::m_freem;
use crate::net::if_::tests::test_packet;
use crate::net::pfvar::{PF_OSFP_WSIZE_MSS, pf_osfp_pack};
use crate::netinet::tcp::TCPOPT_SACK_PERMITTED as SACKOK;

/// The mbuf test lock (the fingerprint list and `pf_lock` are global), pools initialised
/// once, the list empty.
fn setup() -> MutexGuard<'static, ()> {
    static ONCE: Once = Once::new();
    let g = crate::kern::uipc_mbuf::tests::setup();
    ONCE.call_once(pf_osfp_initialize);
    pf_osfp_flush();
    g
}

/// The options of the test SYN: MSS 1460, SACK permitted, a timestamp, NOP, window scale 7.
const OPTS: [u8; 20] = [
    TCPOPT_MAXSEG,
    4,
    0x05,
    0xb4,
    SACKOK,
    2,
    TCPOPT_TIMESTAMP,
    10,
    0,
    0,
    0,
    1,
    0,
    0,
    0,
    0,
    TCPOPT_NOP,
    TCPOPT_WINDOW,
    3,
    7,
];

/// `fp_tcpopts` of `OPTS`.
const TCPOPTS: u64 = ((((PF_OSFP_TCPOPT_MSS << 3 | PF_OSFP_TCPOPT_SACK) << 3 | PF_OSFP_TCPOPT_TS)
    << 3
    | PF_OSFP_TCPOPT_NOP)
    << 3)
    | PF_OSFP_TCPOPT_WSCALE;

/// An IPv4 header of the test SYN with `ttl`.
fn ip(ttl: u8) -> Ip {
    let mut ip = Ip {
        ip_len: 60u16.to_be(),
        ip_off: IP_DF.to_be(),
        ip_ttl: ttl,
        ip_p: 6,
        ip_src: InAddr {
            s_addr: u32::from_ne_bytes([192, 0, 2, 7]),
        },
        ..Ip::default()
    };
    ip.set_ip_v(4);
    ip.set_ip_hl(5);
    ip
}

/// The TCP header and options of a SYN with window `win` and flags `flags`.
fn tcp(win: u16, flags: u8) -> Vec<u8> {
    let mut th = Tcphdr {
        th_sport: 40000u16.to_be(),
        th_dport: 22u16.to_be(),
        th_flags: flags,
        th_win: win.to_be(),
        ..Tcphdr::default()
    };
    th.set_th_off(10);
    // SAFETY: `Tcphdr` is plain integers.
    let bytes = unsafe {
        core::slice::from_raw_parts(core::ptr::from_ref(&th).cast::<u8>(), size_of::<Tcphdr>())
    };
    let mut v = bytes.to_vec();
    v.extend_from_slice(&OPTS);
    v
}

fn name(s: &[u8]) -> [u8; 32] {
    let mut n = [0u8; 32];
    n[..s.len()].copy_from_slice(s);
    n
}

/// A fingerprint ioctl for the test SYN with OS `os` named `class version subtype`.
fn fpioc(os: PfOsfp, class: &[u8], version: &[u8], flags: u16, wsize: u16) -> PfOsfpIoctl {
    let mut io = PfOsfpIoctl::default();
    io.fp_os = PfOsfpIoctlEntry {
        fp_os: os,
        fp_class_nm: name(class),
        fp_version_nm: name(version),
        fp_subtype_nm: name(b""),
        ..PfOsfpIoctlEntry::default()
    };
    io.fp_tcpopts = TCPOPTS;
    io.fp_wsize = wsize;
    io.fp_psize = 60;
    io.fp_mss = 1460;
    io.fp_flags = flags;
    io.fp_optcnt = 5;
    io.fp_wscale = 7;
    io.fp_ttl = 64;
    io
}

fn fingerprint(ip: &Ip, tcp: &[u8]) -> Option<&'static SlistHead<PfOsfpEnlist>> {
    pf_lock();
    let r = pf_osfp_fingerprint_hdr(Some(ip), None, tcp);
    pf_unlock();
    r
}

#[test]
fn syns_match_the_fingerprints_added() {
    let _g = setup();

    let linux = pf_osfp_pack(1, 2, 3);
    assert_eq!(
        pf_osfp_add(&fpioc(linux, b"Linux", b"2.6", PF_OSFP_DF, 65535)),
        Ok(())
    );
    assert_eq!(
        pf_osfp_add(&fpioc(linux, b"Linux", b"2.6", PF_OSFP_DF, 65535)),
        Err(Errno::EEXIST)
    );
    // A second OS on the same fingerprint.
    let other = pf_osfp_pack(4, 5, 6);
    assert_eq!(
        pf_osfp_add(&fpioc(other, b"Other", b"1", PF_OSFP_DF, 65535)),
        Ok(())
    );
    // A window that is a multiple of the MSS.
    let bsd = pf_osfp_pack(7, 1, 0);
    assert_eq!(
        pf_osfp_add(&fpioc(
            bsd,
            b"BSD",
            b"9",
            PF_OSFP_DF | PF_OSFP_WSIZE_MSS,
            44
        )),
        Ok(())
    );
    let mut long = fpioc(bsd, b"BSD", b"9", 0, 1);
    long.fp_os.fp_class_nm = [b'x'; 32];
    assert_eq!(pf_osfp_add(&long), Err(Errno::ENAMETOOLONG));

    // A SYN from four hops away matches the first fingerprint, both its OSes.
    let list = fingerprint(&ip(60), &tcp(65535, TH_SYN));
    assert!(list.is_some());
    assert_eq!(list.expect("list").iter().count(), 2);
    assert!(pf_osfp_match(list, linux));
    assert!(pf_osfp_match(list, other));
    assert!(pf_osfp_match(list, pf_osfp_pack(1, 0, 0)), "any version");
    assert!(!pf_osfp_match(list, pf_osfp_pack(2, 0, 0)));
    assert!(!pf_osfp_match(list, bsd));
    assert!(pf_osfp_match(list, PF_OSFP_ANY));

    // The MSS-multiple window.
    let list = fingerprint(&ip(64), &tcp(44 * 1460, TH_SYN));
    assert!(pf_osfp_match(list, bsd));
    assert!(!pf_osfp_match(list, linux));

    // Not a fingerprint: too many hops, a SYN-ACK, a TTL above the list's, an unknown window.
    assert!(fingerprint(&ip(20), &tcp(65535, TH_SYN)).is_none());
    assert!(fingerprint(&ip(60), &tcp(65535, TH_SYN | TH_ACK)).is_none());
    assert!(fingerprint(&ip(65), &tcp(65535, TH_SYN)).is_none());
    assert!(fingerprint(&ip(60), &tcp(1234, TH_SYN)).is_none());
    assert!(pf_osfp_match(None, PF_OSFP_UNKNOWN));
    assert!(!pf_osfp_match(None, linux));

    // Bad options: a length past the header.
    let mut bad = tcp(65535, TH_SYN);
    bad[size_of::<Tcphdr>() + 1] = 40;
    assert!(fingerprint(&ip(60), &bad).is_none());

    pf_lock();
    assert!(
        pf_osfp_validate().is_none(),
        "every fingerprint is reachable"
    );
    pf_unlock();
}

#[test]
fn a_packet_descriptor_is_fingerprinted_from_its_mbuf() {
    let _g = setup();
    let linux = pf_osfp_pack(1, 2, 3);
    assert_eq!(
        pf_osfp_add(&fpioc(linux, b"Linux", b"2.6", PF_OSFP_DF, 65535)),
        Ok(())
    );

    let iph = ip(64);
    let th = tcp(65535, TH_SYN);
    let mut pkt = Vec::new();
    // SAFETY: `Ip` is plain integers.
    pkt.extend_from_slice(unsafe {
        core::slice::from_raw_parts(core::ptr::from_ref(&iph).cast::<u8>(), size_of::<Ip>())
    });
    pkt.extend_from_slice(&th);
    let m = test_packet(&pkt);

    let mut pd = PfPdesc::new();
    pd.m = Some(m);
    pd.af = AF_INET;
    pd.proto = IPPROTO_TCP as u8;
    pd.off = size_of::<Ip>() as u32;
    pd.hdr_bytes()[..size_of::<Tcphdr>()].copy_from_slice(&th[..size_of::<Tcphdr>()]);

    pf_lock();
    let list = pf_osfp_fingerprint(&mut pd);
    pd.proto = 17;
    let udp = pf_osfp_fingerprint(&mut pd);
    pf_unlock();
    assert!(pf_osfp_match(list, linux));
    assert!(udp.is_none());
    m_freem(m);
}

#[test]
fn fingerprints_are_read_back_by_number_and_flushed() {
    let _g = setup();
    let a = pf_osfp_pack(1, 1, 1);
    let b = pf_osfp_pack(2, 2, 2);
    assert_eq!(pf_osfp_add(&fpioc(a, b"A", b"1", PF_OSFP_DF, 100)), Ok(()));
    assert_eq!(pf_osfp_add(&fpioc(b, b"B", b"2", PF_OSFP_DF, 200)), Ok(()));

    let mut io = PfOsfpIoctl::default();
    io.fp_getnum = 1;
    assert_eq!(pf_osfp_get(&mut io), Ok(()));
    assert_eq!(io.fp_getnum, 1);
    assert_eq!(io.fp_os.fp_os, b);
    assert_eq!(pf_cstr(&io.fp_os.fp_class_nm), b"B");
    assert_eq!(io.fp_wsize, 200);
    assert_eq!(io.fp_mss, 1460);
    assert_eq!(io.fp_tcpopts, 0, "DIOCOSFPGET does not return the options");

    io.fp_getnum = 0;
    assert_eq!(pf_osfp_get(&mut io), Ok(()));
    assert_eq!(io.fp_os.fp_os, a);
    io.fp_getnum = 2;
    assert_eq!(pf_osfp_get(&mut io), Err(Errno::EBUSY));

    pf_osfp_flush();
    io.fp_getnum = 0;
    assert_eq!(pf_osfp_get(&mut io), Err(Errno::EBUSY));
    assert!(PF_OSFP_LIST.is_empty());
}
