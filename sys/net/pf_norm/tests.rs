//! Host tests for the normalizer: IPv4 reassembly in real mbufs (in order, with overlaps,
//! the `IP_DF` rule) and `pf_scrub`'s header rewrites.

use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::kern::uipc_mbuf::{m_copydata, m_gethdr};
use crate::net::pfvar::{PF_IN, PF_REASS_ENABLED, PFTM_FRAG};
use crate::netinet::in_::IPPROTO_UDP;
use crate::sys::mbuf::{M_DONTWAIT, MT_DATA};

/// Real memory and mbufs, pf's fragment pools, the default `frag` timeout and reassembly
/// on.
fn setup() -> MutexGuard<'static, ()> {
    let guard = crate::kern::uipc_mbuf::tests::setup();
    pf_normalize_init();
    PF_FRNODE_TREE.init();
    PF_DEFAULT_RULE.timeout[PFTM_FRAG].set(60);
    PF_STATUS.fragments.set(0);
    PF_STATUS.reass.set(PF_REASS_ENABLED);
    guard
}

/// The bytes of an IPv4 header (no options, checksum left zero).
fn ip_header(id: u16, off_bytes: u16, mf: bool, df: bool, payload_len: usize) -> [u8; 20] {
    let len = (20 + payload_len) as u16;
    let mut off = off_bytes >> 3;
    if mf {
        off |= IP_MF;
    }
    if df {
        off |= IP_DF;
    }
    let mut h = [0u8; 20];
    h[0] = 0x45;
    h[2..4].copy_from_slice(&len.to_be_bytes());
    h[4..6].copy_from_slice(&id.to_be_bytes());
    h[6..8].copy_from_slice(&off.to_be_bytes());
    h[8] = 64;
    h[9] = IPPROTO_UDP as u8;
    h[12..16].copy_from_slice(&[192, 168, 1, 5]);
    h[16..20].copy_from_slice(&[10, 0, 0, 1]);
    h
}

/// A one-mbuf IPv4 fragment carrying `payload` at byte offset `off_bytes`.
fn fragment(id: u16, off_bytes: u16, mf: bool, df: bool, payload: &[u8]) -> &'static Mbuf {
    let mut bytes = Vec::from(ip_header(id, off_bytes, mf, df, payload.len()));
    bytes.extend_from_slice(payload);
    let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("a header mbuf");
    m.m_len().set(bytes.len() as u32);
    m.m_pkthdr().len.set(bytes.len() as i32);
    m_copyback(m, 0, &bytes, M_NOWAIT).expect("copyback");
    m
}

/// `pf_normalize_ip` of `m` arriving inbound.
fn normalize(m: &'static Mbuf, reason: &mut u16) -> (u8, Option<&'static Mbuf>) {
    let mut pd = PfPdesc::new();
    pd.m = Some(m);
    pd.af = AF_INET;
    pd.dir = PF_IN;
    let action = pf_normalize_ip(&mut pd, reason);
    (action, pd.m)
}

/// The packet's bytes.
fn contents(m: &Mbuf) -> Vec<u8> {
    let mut v = vec![0u8; m.m_pkthdr().len.get() as usize];
    m_copydata(m, 0, &mut v);
    v
}

/// The ones-complement sum of a header (0xffff when its checksum is right).
fn ip_sum(h: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    for w in h.chunks(2) {
        sum += u32::from(u16::from_be_bytes([w[0], w[1]]));
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    sum as u16
}

/// Fills in a header's checksum.
fn set_ip_sum(h: &mut [u8; 20]) {
    h[10] = 0;
    h[11] = 0;
    let s = !ip_sum(h);
    h[10..12].copy_from_slice(&s.to_be_bytes());
}

#[test]
fn two_fragments_reassemble() {
    let _g = setup();
    let a: Vec<u8> = (0..16).collect();
    let b: Vec<u8> = (100..108).collect();
    let mut reason = 0;

    let (action, m) = normalize(fragment(7, 0, true, false, &a), &mut reason);
    assert_eq!(action, PF_PASS);
    assert!(m.is_none(), "the first fragment is queued");
    assert_eq!(PF_STATUS.fragments.get(), 1);

    let (action, m) = normalize(fragment(7, 16, false, false, &b), &mut reason);
    assert_eq!(action, PF_PASS);
    let m = m.expect("the reassembled packet");
    assert_eq!(m.m_pkthdr().len.get(), 20 + 24);
    let bytes = contents(m);
    assert_eq!(u16::from_be_bytes([bytes[2], bytes[3]]), 44, "ip_len");
    assert_eq!(
        u16::from_be_bytes([bytes[6], bytes[7]]),
        0,
        "no fragment bits left"
    );
    assert_eq!(&bytes[20..36], &a[..]);
    assert_eq!(&bytes[36..44], &b[..]);
    assert_eq!(PF_STATUS.fragments.get(), 0);
    assert!(PF_FRAGQUEUE.is_empty());
    assert!(PF_FRNODE_TREE.is_empty());
    m_freem(m);
}

#[test]
fn overlapping_fragments_are_trimmed() {
    let _g = setup();
    let a: Vec<u8> = (0..16).collect();
    let b: Vec<u8> = (200..216).collect();
    let c: Vec<u8> = (50..58).collect();
    let mut reason = 0;

    let (action, m) = normalize(fragment(9, 0, true, false, &a), &mut reason);
    assert_eq!((action, m.is_none()), (PF_PASS, true));

    // Completely inside the first fragment: dropped, the packet stays the caller's.
    let cm = fragment(9, 0, true, false, &c);
    let (action, m) = normalize(cm, &mut reason);
    assert_eq!(action, PF_DROP);
    assert!(m.is_some_and(|m| core::ptr::eq(m, cm)));
    m_freem(cm);
    assert_eq!(PF_STATUS.fragments.get(), 1);

    // Its head overlaps the first fragment by 8 bytes, which the first one keeps.
    let (action, m) = normalize(fragment(9, 8, false, false, &b), &mut reason);
    assert_eq!(action, PF_PASS);
    let m = m.expect("the reassembled packet");
    let bytes = contents(m);
    assert_eq!(bytes.len(), 20 + 24);
    assert_eq!(&bytes[20..36], &a[..]);
    assert_eq!(&bytes[36..44], &b[8..]);
    assert_eq!(PF_STATUS.fragments.get(), 0);
    m_freem(m);
}

#[test]
fn tail_overlap_trims_the_queued_fragment() {
    let _g = setup();
    let first: Vec<u8> = (0..8).collect();
    let last: Vec<u8> = (100..116).collect();
    let mid: Vec<u8> = (50..66).collect();
    let mut reason = 0;

    // The last fragment first, then the first one.
    assert_eq!(
        normalize(fragment(3, 16, false, false, &last), &mut reason).0,
        PF_PASS
    );
    assert_eq!(
        normalize(fragment(3, 0, true, false, &first), &mut reason).0,
        PF_PASS
    );
    // 8..24 overlaps the head of the queued 16..32, which loses its first 8 bytes.
    let (action, m) = normalize(fragment(3, 8, true, false, &mid), &mut reason);
    assert_eq!(action, PF_PASS);
    let m = m.expect("the reassembled packet");
    let bytes = contents(m);
    assert_eq!(bytes.len(), 20 + 32);
    assert_eq!(&bytes[20..28], &first[..]);
    assert_eq!(&bytes[28..44], &mid[..]);
    assert_eq!(&bytes[44..52], &last[8..]);
    m_freem(m);
}

#[test]
fn fragment_with_df_is_dropped() {
    let _g = setup();
    let mut reason = 0;
    let m = fragment(5, 0, true, true, &[0u8; 8]);
    let (action, back) = normalize(m, &mut reason);
    assert_eq!(action, PF_DROP);
    assert_eq!(reason, PFRES_FRAG);
    assert!(back.is_some());
    m_freem(m);
    assert_eq!(PF_STATUS.fragments.get(), 0);
}

#[test]
fn bad_fragment_length_is_dropped() {
    let _g = setup();
    let mut reason = 0;
    // More fragments, but a length that is not a multiple of 8.
    let m = fragment(6, 0, true, false, &[0u8; 5]);
    let (action, _) = normalize(m, &mut reason);
    assert_eq!(action, PF_DROP);
    assert_eq!(reason, PFRES_FRAG);
    m_freem(m);
}

#[test]
fn scrub_no_df_min_ttl_set_tos() {
    let _g = setup();
    let mut h = ip_header(1, 0, false, true, 0);
    h[1] = 0x03; // ECN bits
    h[8] = 2; // ttl
    set_ip_sum(&mut h);
    let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("a header mbuf");
    m.m_len().set(20);
    m.m_pkthdr().len.set(20);
    m_copyback(m, 0, &h, M_NOWAIT).expect("copyback");

    pf_scrub(m, PFSTATE_NODF | PFSTATE_SETTOS, AF_INET, 32, 0x10);

    let b = contents(m);
    assert_eq!(u16::from_be_bytes([b[6], b[7]]) & IP_DF, 0, "no-df");
    assert_eq!(b[8], 32, "min-ttl");
    assert_eq!(b[1], 0x10 | 0x03, "set-tos keeps the ECN bits");
    assert_eq!(ip_sum(&b), 0xffff, "the header checksum still adds up");

    // A TTL above the minimum stays.
    pf_scrub(m, 0, AF_INET, 16, 0);
    assert_eq!(contents(m)[8], 32);
    m_freem(m);
}

#[test]
fn frent_index_and_entry_points() {
    let frent = PfFrent {
        fr_next: TailqEntry::new(),
        fe_m: Cell::new(None),
        fe_hdrlen: Cell::new(20),
        fe_extoff: Cell::new(0),
        fe_len: Cell::new(8),
        fe_off: Cell::new(0),
        fe_mff: Cell::new(1),
    };
    assert_eq!(pf_frent_index(&frent), 0);
    frent.fe_off.set(4096);
    assert_eq!(pf_frent_index(&frent), 1);
    frent.fe_off.set(0xfff8);
    assert_eq!(pf_frent_index(&frent), PF_FRAG_ENTRY_POINTS - 1);
}
