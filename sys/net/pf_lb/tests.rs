//! Host tests for the address pools (`none`, `bitmask`, `round-robin` over an address and
//! mask) and the source port search of `nat-to`.

use std::boxed::Box;
use std::{assert, assert_eq};

use super::*;
use crate::net::pfvar::{PF_ADDR_ADDRMASK, PfAddrWrapV};
use crate::netinet::in_::InAddr;

/// An IPv4 `PfAddr` from its dotted bytes.
fn a4(b: [u8; 4]) -> PfAddr {
    PfAddr::from_v4(InAddr {
        s_addr: u32::from_ne_bytes(b),
    })
}

/// A kernel rule whose `nat` pool is `addr`/`mask` with the pool type `opts`.
fn rule_with_nat(addr: [u8; 4], mask: [u8; 4], opts: u8) -> &'static PfRule {
    let r: &'static PfRule = Box::leak(Box::new(PfRule::zeroed()));
    let mut v = PfAddrWrapV::default();
    v.set_addr(&a4(addr));
    v.set_mask(&a4(mask));
    r.nat.addr.v.set(v);
    r.nat.addr.type_.set(PF_ADDR_ADDRMASK);
    r.nat.opts.set(opts);
    r
}

/// `pf_map_addr` of `saddr` through the rule's `nat` pool.
fn map(r: &'static PfRule, saddr: [u8; 4]) -> Option<PfAddr> {
    let mut sns: [Option<&'static PfSrcNode>; PF_SN_MAX] = [None; PF_SN_MAX];
    let mut naddr = PfAddr::zeroed();
    pf_map_addr(
        AF_INET,
        r,
        &a4(saddr),
        &mut naddr,
        None,
        &mut sns,
        &r.nat,
        PF_SN_NAT,
    )
    .then_some(naddr)
}

#[test]
fn pool_none_maps_to_the_pool_address() {
    let r = rule_with_nat([10, 0, 0, 1], [255, 255, 255, 255], PF_POOL_NONE);
    assert_eq!(map(r, [192, 168, 1, 5]), Some(a4([10, 0, 0, 1])));
    assert_eq!(map(r, [172, 16, 0, 9]), Some(a4([10, 0, 0, 1])));
}

#[test]
fn pool_bitmask_keeps_the_host_part() {
    let r = rule_with_nat([10, 0, 0, 0], [255, 255, 255, 0], PF_POOL_BITMASK);
    assert_eq!(map(r, [192, 168, 1, 5]), Some(a4([10, 0, 0, 5])));
    assert_eq!(map(r, [172, 16, 3, 200]), Some(a4([10, 0, 0, 200])));
}

#[test]
fn pool_roundrobin_walks_the_network() {
    let r = rule_with_nat([10, 0, 0, 0], [255, 255, 255, 252], PF_POOL_ROUNDROBIN);
    let s = [192, 168, 1, 5];
    assert_eq!(map(r, s), Some(a4([10, 0, 0, 0])));
    assert_eq!(map(r, s), Some(a4([10, 0, 0, 1])));
    assert_eq!(map(r, s), Some(a4([10, 0, 0, 2])));
    assert_eq!(map(r, s), Some(a4([10, 0, 0, 3])));
    // The counter's host part wraps within the mask.
    assert_eq!(map(r, s), Some(a4([10, 0, 0, 0])));
    // A counter that matches the pool's network makes the C's pf_match_addr check fail.
    r.nat.counter.set(a4([10, 0, 0, 1]));
    assert_eq!(map(r, s), None);
}

#[test]
fn pool_roundrobin_single_host_falls_back_to_none() {
    let r = rule_with_nat([10, 0, 0, 7], [255, 255, 255, 255], PF_POOL_ROUNDROBIN);
    assert_eq!(map(r, [1, 2, 3, 4]), Some(a4([10, 0, 0, 7])));
    assert_eq!(map(r, [1, 2, 3, 4]), Some(a4([10, 0, 0, 7])));
}

#[test]
fn noroute_pool_fails() {
    let r = rule_with_nat([10, 0, 0, 7], [255, 255, 255, 255], PF_POOL_NONE);
    r.nat.addr.type_.set(PF_ADDR_NOROUTE);
    assert_eq!(map(r, [1, 2, 3, 4]), None);
}

/// An outbound UDP descriptor from 192.168.1.5:1234 to 8.8.8.8:53.
fn udp_pd() -> PfPdesc {
    let mut pd = PfPdesc::new();
    pd.af = AF_INET;
    pd.naf = AF_INET;
    pd.proto = IPPROTO_UDP as u8;
    pd.dir = PF_OUT;
    pd.sidx = 1;
    pd.didx = 0;
    pd.nsaddr = a4([192, 168, 1, 5]);
    pd.ndaddr = a4([8, 8, 8, 8]);
    pd.nsport = 1234u16.to_be();
    pd.ndport = 53u16.to_be();
    pd
}

#[test]
fn get_sport_picks_a_port_in_the_range() {
    let r = rule_with_nat([10, 0, 0, 1], [255, 255, 255, 255], PF_POOL_NONE);
    let mut pd = udp_pd();
    let mut sns: [Option<&'static PfSrcNode>; PF_SN_MAX] = [None; PF_SN_MAX];
    for _ in 0..16 {
        let mut naddr = PfAddr::zeroed();
        let mut nport = 0u16;
        assert!(pf_get_sport(
            &mut pd, r, &mut naddr, &mut nport, 50000, 50010, &mut sns
        ));
        assert_eq!(naddr, a4([10, 0, 0, 1]));
        let p = u16::from_be(nport);
        assert!((50000..=50010).contains(&p), "port {p}");
    }

    // A one-port range gives that port; reversed bounds are swapped.
    let mut naddr = PfAddr::zeroed();
    let mut nport = 0u16;
    assert!(pf_get_sport(
        &mut pd, r, &mut naddr, &mut nport, 40000, 40000, &mut sns
    ));
    assert_eq!(u16::from_be(nport), 40000);
    assert!(pf_get_sport(
        &mut pd, r, &mut naddr, &mut nport, 60010, 60000, &mut sns
    ));
    assert!((60000..=60010).contains(&u16::from_be(nport)));

    // No range: the packet's own port.
    assert!(pf_get_sport(
        &mut pd, r, &mut naddr, &mut nport, 0, 0, &mut sns
    ));
    assert_eq!(u16::from_be(nport), 1234);
}

#[test]
fn get_sport_leaves_non_echo_icmp_alone() {
    let r = rule_with_nat([10, 0, 0, 1], [255, 255, 255, 255], PF_POOL_NONE);
    let mut pd = udp_pd();
    pd.proto = IPPROTO_ICMP as u8;
    pd.ndport = 3u16.to_be(); // unreachable, not an echo
    let mut sns: [Option<&'static PfSrcNode>; PF_SN_MAX] = [None; PF_SN_MAX];
    let mut naddr = PfAddr::zeroed();
    let mut nport = 0xbeefu16;
    assert!(pf_get_sport(
        &mut pd, r, &mut naddr, &mut nport, 50000, 50010, &mut sns
    ));
    assert_eq!(nport, 0xbeef);
    assert_eq!(naddr, a4([10, 0, 0, 1]));
}

#[test]
fn rand_addr_stays_in_the_host_part() {
    let mask = u32::from_ne_bytes([255, 255, 255, 0]);
    for _ in 0..32 {
        let a = pf_rand_addr(mask);
        assert_eq!(a & mask, 0);
    }
}
