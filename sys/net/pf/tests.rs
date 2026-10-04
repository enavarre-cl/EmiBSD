//! Host tests for pf's packet path: `pf_print_host` of IPv4 and IPv6 addresses, the 128-bit
//! address helpers, the ICMP/ICMPv6 type translation of `af-to`, and `pf_test(AF_INET6)` on
//! synthetic packets: pass and block by IPv6
//! source prefix, a state created by an outbound ICMPv6 echo that lets the reply in, and the
//! IPv6 fragment cache of `pf_normalize_ip6`.

use std::boxed::Box;
use std::format;
use std::sync::MutexGuard;
use std::vec::Vec;

use super::*;
use crate::kern::kern_proc::procinit;
use crate::kern::kern_prot::{crget, crhold};
use crate::kern::kern_timeout::timeout_del;
use crate::net::if_::if_attach;
use crate::net::if_::tests::{setup_net, test_ifnet, test_packet};
use crate::net::pf_ioctl::{pfattach, pfioctl};
use crate::net::pf_ruleset::pf_main_ruleset;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::ioctl::ioctl_ret;
use crate::sys::proc::{Proc, Process};
use crate::sys::ucred::Ucred;

/// An address from its eight 16-bit groups.
fn pf6(g: [u16; 8]) -> PfAddr {
    let mut a = PfAddr::zeroed();
    for (i, w) in g.iter().enumerate() {
        a.addr8[2 * i..2 * i + 2].copy_from_slice(&w.to_be_bytes());
    }
    a
}

#[test]
fn print_host_ipv4_and_ipv6() {
    let v4 = PfAddr::from_v4(crate::netinet::in_::InAddr {
        s_addr: u32::from_ne_bytes([192, 0, 2, 1]),
    });
    assert_eq!(
        format!("{}", PfHost(&v4, 80u16.to_be(), AF_INET)),
        "192.0.2.1:80"
    );
    assert_eq!(format!("{}", PfHost(&v4, 0, AF_INET)), "192.0.2.1");

    let a = pf6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]);
    assert_eq!(format!("{}", PfHost(&a, 0, AF_INET6)), "2001:db8::1");
    assert_eq!(
        format!("{}", PfHost(&a, 443u16.to_be(), AF_INET6)),
        "2001:db8::1[443]"
    );
    // The longest run of zero words is compressed, the first one on a tie.
    let b = pf6([0xfe80, 0, 0, 1, 0, 0, 0, 2]);
    assert_eq!(format!("{}", PfHost(&b, 0, AF_INET6)), "fe80:0:0:1::2");
    let c = pf6([0, 0, 0, 0, 0, 0, 0, 1]);
    assert_eq!(format!("{}", PfHost(&c, 0, AF_INET6)), "::1");
    let d = pf6([0xfd00, 1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(format!("{}", PfHost(&d, 0, AF_INET6)), "fd00:1:2:3:4:5:6:7");
}

#[test]
fn ipv6_address_helpers() {
    let lo = pf6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0xffff]);
    let hi = pf6([0x2001, 0xdb8, 0, 0, 0, 0, 1, 0]);
    let mut x = lo;
    pf_addr_inc(&mut x, AF_INET6);
    assert_eq!(x, pf6([0x2001, 0xdb8, 0, 0, 0, 0, 1, 0]));
    let mut all = pf6([0xffff; 8]);
    pf_addr_inc(&mut all, AF_INET6);
    assert_eq!(all, PfAddr::zeroed());

    assert!(pf_match_addr_range(&lo, &hi, &x, AF_INET6));
    assert!(!pf_match_addr_range(
        &lo,
        &hi,
        &pf6([0x2001, 0xdb8, 0, 0, 0, 0, 1, 1]),
        AF_INET6
    ));
    assert_eq!(
        pf_addr_compare(&lo, &hi, AF_INET6),
        -pf_addr_compare(&hi, &lo, AF_INET6)
    );

    let m48 = pf6([0xffff, 0xffff, 0xffff, 0, 0, 0, 0, 0]);
    let net = pf6([0x2001, 0xdb8, 1, 0, 0, 0, 0, 0]);
    assert!(pf_match_addr(
        0,
        &net,
        &m48,
        &pf6([0x2001, 0xdb8, 1, 9, 9, 9, 9, 9]),
        AF_INET6
    ));
    assert!(!pf_match_addr(
        0,
        &net,
        &m48,
        &pf6([0x2001, 0xdb8, 2, 0, 0, 0, 0, 0]),
        AF_INET6
    ));

    let mut n = PfAddr::zeroed();
    pf_poolmask(
        &mut n,
        &net,
        &m48,
        &pf6([0xaaaa, 0xbbbb, 0xcccc, 1, 2, 3, 4, 5]),
        AF_INET6,
    );
    assert_eq!(n, pf6([0x2001, 0xdb8, 1, 1, 2, 3, 4, 5]));

    // The checksum fixup of an address change equals a recomputation.
    let mut ck: u16 = 0x1234;
    pf_cksum_fixup_a(&mut ck, &lo, &hi, AF_INET6, IPPROTO_TCP as u8);
    let mut back = ck;
    pf_cksum_fixup_a(&mut back, &hi, &lo, AF_INET6, IPPROTO_TCP as u8);
    assert_eq!(back, 0x1234);
}

#[test]
fn icmp_types_translate_between_families() {
    use crate::netinet::icmp6::*;
    use crate::netinet::ip_icmp::*;

    let mut pd = PfPdesc::new();
    pd.proto = IPPROTO_ICMP as u8;
    pd.pcksum = PfLoc::Hdr(2);
    // Echo request towards IPv6.
    pd.hdr_bytes()[..8].copy_from_slice(&[ICMP_ECHO, 0, 0x12, 0x34, 0, 7, 0, 1]);
    assert_eq!(pf_translate_icmp_af(&mut pd, AF_INET6, PfLoc::Hdr(0)), 0);
    assert_eq!(pd.hdr_bytes()[..2], [ICMP6_ECHO_REQUEST, 0]);
    // Need-frag becomes packet-too-big with the IPv6 MTU (20 bytes more).
    pd.hdr_bytes()[..8].copy_from_slice(&[
        ICMP_UNREACH,
        ICMP_UNREACH_NEEDFRAG,
        0,
        0,
        0,
        0,
        5,
        0xc8,
    ]);
    assert_eq!(pf_translate_icmp_af(&mut pd, AF_INET6, PfLoc::Hdr(0)), 0);
    assert_eq!(pd.hdr_bytes()[..2], [ICMP6_PACKET_TOO_BIG, 0]);
    assert_eq!(pd.hdr_bytes()[6..8], 1500u16.to_be_bytes());
    // Parameter problem in the IPv6 next header field becomes protocol unreachable.
    pd.proto = crate::netinet::in_::IPPROTO_ICMPV6 as u8;
    pd.hdr_bytes()[..8].copy_from_slice(&[
        ICMP6_PARAM_PROB,
        ICMP6_PARAMPROB_NEXTHEADER,
        0,
        0,
        0,
        0,
        0,
        6,
    ]);
    assert_eq!(pf_translate_icmp_af(&mut pd, AF_INET, PfLoc::Hdr(0)), 0);
    assert_eq!(pd.hdr_bytes()[..2], [ICMP_UNREACH, ICMP_UNREACH_PROTOCOL]);
    // A router advertisement has no IPv4 counterpart.
    pd.hdr_bytes()[0] = ND_ROUTER_ADVERT;
    assert_eq!(pf_translate_icmp_af(&mut pd, AF_INET, PfLoc::Hdr(0)), -1);
}

/// The network test lock with fresh memory, the process tables and pf attached, and one
/// interface `pf6test0` attached to pf.
fn setup() -> (
    MutexGuard<'static, ()>,
    MutexGuard<'static, ()>,
    &'static Ifnet,
) {
    let guard = setup_net();
    let t = crate::kern::kern_timeout::tests::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    procinit();
    crate::kern::kern_timeout::timeout_startup();
    crate::net::if_::softnet_init();
    crate::net::rtable::rtable_init();
    crate::net::route::route_init();
    pfattach(1);
    let ifp = test_ifnet(b"pf6test0");
    if_attach(ifp);
    (guard, t, ifp)
}

/// A thread of a root process, for `/dev/pf`.
fn thread() -> &'static Proc {
    let cr: &'static Ucred = crget();
    let pr: &'static Process = Box::leak(Box::new(Process::new()));
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    p.p_p.set(pr);
    p.p_ucred.set(cr);
    pr.ps_ucred.set(crhold(cr));
    pr.ps_mainproc.set(p);
    p
}

fn ioctl(cmd: u64, data: &mut [u8], p: &Proc) -> Result<(), Errno> {
    pfioctl(0, cmd, data, FREAD | FWRITE, p)
}

/// A rule of `af` `AF_INET6`: `action` in `dir`, quick, for `proto` (0: any) from
/// `src/mask` to any, keeping state with `keep`.
fn rule6(action: u8, dir: u8, proto: u8, src: PfAddr, mask: PfAddr, keep: bool) -> Box<PfiocRule> {
    let mut pr = pf_abi_zeroed::<PfiocRule>();
    pr.rule.action = action;
    pr.rule.direction = dir;
    pr.rule.quick = 1;
    pr.rule.af = AF_INET6;
    pr.rule.proto = proto;
    pr.rule.keep_state = if keep { PF_STATE_NORMAL } else { 0 };
    pr.rule.rtableid = -1;
    pr.rule.onrdomain = -1;
    pr.rule.src.addr.type_.set(PF_ADDR_ADDRMASK);
    let mut v = pr.rule.src.addr.v.get();
    v.set_addr(&src);
    v.set_mask(&mask);
    pr.rule.src.addr.v.set(v);
    pr.rule.dst.addr.type_.set(PF_ADDR_ADDRMASK);
    // No nat-to, rdr-to or route-to pool, as pfctl leaves them.
    pr.rule.nat.addr.type_.set(PF_ADDR_NONE);
    pr.rule.rdr.addr.type_.set(PF_ADDR_NONE);
    pr.rule.route.addr.type_.set(PF_ADDR_NONE);
    pr
}

/// Installs `rules` as the main ruleset and starts pf.
fn load(rules: &mut [Box<PfiocRule>], p: &Proc) {
    let mut e = Box::new(PfiocTransE {
        type_: PF_TRANS_RULESET,
        anchor: [0; crate::sys::syslimits::PATH_MAX],
        ticket: 0,
    });
    let io = PfiocTrans {
        size: 1,
        esize: size_of::<PfiocTransE>() as i32,
        array: ptr::from_mut(&mut *e) as usize,
    };
    let mut data = std::vec![0u8; size_of::<PfiocTrans>()];
    ioctl_ret(&mut data, &io);
    assert_eq!(ioctl(DIOCXBEGIN, &mut data, p), Ok(()));
    for r in rules.iter_mut() {
        r.ticket = e.ticket;
        let mut d = pf_abi_bytes(&**r).to_vec();
        assert_eq!(ioctl(DIOCADDRULE, &mut d, p), Ok(()));
    }
    let mut data = std::vec![0u8; size_of::<PfiocTrans>()];
    ioctl_ret(&mut data, &io);
    assert_eq!(ioctl(DIOCXCOMMIT, &mut data, p), Ok(()));
    assert_eq!(ioctl(DIOCSTART, &mut [], p), Ok(()));
}

/// Stops pf and the purge timeouts `DIOCSTART` armed.
fn unload(p: &Proc) {
    assert_eq!(ioctl(DIOCSTOP, &mut [], p), Ok(()));
    timeout_del(&PF_PURGE_STATES_TO);
    timeout_del(&PF_PURGE_TO);
}

/// An IPv6 packet `src -> dst` with next header `nxt` and `payload`, hop limit 64.
fn ip6_packet(src: PfAddr, dst: PfAddr, nxt: u8, payload: &[u8]) -> Vec<u8> {
    let mut b = std::vec![0u8; 40];
    b[0] = 0x60;
    b[4..6].copy_from_slice(&(payload.len() as u16).to_be_bytes());
    b[6] = nxt;
    b[7] = 64;
    b[8..24].copy_from_slice(&src.addr8);
    b[24..40].copy_from_slice(&dst.addr8);
    b.extend_from_slice(payload);
    b
}

/// An ICMPv6 echo request (128) or reply (129) with `id` and sequence 1.
fn echo6(type_: u8, id: u16) -> [u8; 8] {
    let mut e = [0u8; 8];
    e[0] = type_;
    e[4..6].copy_from_slice(&id.to_be_bytes());
    e[6..8].copy_from_slice(&1u16.to_be_bytes());
    e
}

/// `pf_test(AF_INET6, dir, ifp)` on `bytes`; the action, the packet freed whatever pf did.
fn test6(dir: u8, ifp: &'static Ifnet, bytes: &[u8]) -> u8 {
    let mut m = Some(test_packet(bytes));
    let action = pf_test(AF_INET6, dir, ifp, &mut m);
    crate::kern::uipc_mbuf::m_freem(m);
    action
}

#[test]
fn ipv6_rules_and_icmp6_echo_state() {
    let (_g, _t, ifp) = setup();
    let p = thread();
    let any = PfAddr::zeroed();
    let icmp6 = crate::netinet::in_::IPPROTO_ICMPV6 as u8;
    load(
        &mut [
            // pass in quick inet6 from 2001:db8:1::/48 no state
            rule6(
                PF_PASS,
                PF_IN,
                0,
                pf6([0x2001, 0xdb8, 1, 0, 0, 0, 0, 0]),
                pf6([0xffff, 0xffff, 0xffff, 0, 0, 0, 0, 0]),
                false,
            ),
            // pass out quick inet6 proto ipv6-icmp keep state
            rule6(PF_PASS, PF_OUT, icmp6, any, any, true),
            // block in quick inet6
            rule6(PF_DROP, PF_IN, 0, any, any, false),
        ],
        p,
    );
    assert_eq!(pf_main_ruleset().active.rcount.get(), 3);

    let local = pf6([0xfd00, 0, 0, 0, 0, 0, 0, 1]);
    let peer = pf6([0xfd00, 0, 0, 0, 0, 0, 0, 2]);

    // By source prefix.
    let inside = pf6([0x2001, 0xdb8, 1, 0, 0, 0, 0, 5]);
    let outside = pf6([0x2001, 0xdb8, 2, 0, 0, 0, 0, 5]);
    assert_eq!(
        test6(
            PF_IN,
            ifp,
            &ip6_packet(inside, local, icmp6, &echo6(128, 7))
        ),
        PF_PASS
    );
    assert_eq!(
        test6(
            PF_IN,
            ifp,
            &ip6_packet(outside, local, icmp6, &echo6(128, 7))
        ),
        PF_DROP
    );

    // A reply before any request is blocked.
    let reply = ip6_packet(peer, local, icmp6, &echo6(129, 0x1234));
    assert_eq!(test6(PF_IN, ifp, &reply), PF_DROP);
    // The outbound echo request creates a state; its reply then passes, another id does not.
    let before = PF_STATUS.states.get();
    assert_eq!(
        test6(
            PF_OUT,
            ifp,
            &ip6_packet(local, peer, icmp6, &echo6(128, 0x1234))
        ),
        PF_PASS
    );
    assert_eq!(PF_STATUS.states.get(), before + 1);
    assert_eq!(test6(PF_IN, ifp, &reply), PF_PASS);
    let other = ip6_packet(peer, local, icmp6, &echo6(129, 0x9999));
    assert_eq!(test6(PF_IN, ifp, &other), PF_DROP);

    unload(p);
}

/// A fragment of the datagram `ident`: offset `off` (bytes), more fragments `mf`, carrying
/// `data` of a UDP datagram.
fn frag6(src: PfAddr, dst: PfAddr, off: u16, mf: bool, data: &[u8]) -> Vec<u8> {
    let mut f = std::vec![0u8; 8];
    f[0] = IPPROTO_UDP as u8;
    f[2..4].copy_from_slice(&(off | u16::from(mf)).to_be_bytes());
    f[4..8].copy_from_slice(&0x0abc_def0u32.to_be_bytes());
    f.extend_from_slice(data);
    ip6_packet(src, dst, crate::netinet::in_::IPPROTO_FRAGMENT as u8, &f)
}

#[test]
fn ipv6_fragments_are_reassembled() {
    let (_g, _t, _ifp) = setup();
    let src = pf6([0xfd00, 0, 0, 0, 0, 0, 0, 1]);
    let dst = pf6([0xfd00, 0, 0, 0, 0, 0, 0, 2]);
    // A UDP datagram of 24 bytes: the header and 16 bytes of data, in two fragments.
    let mut udp = std::vec![0u8; 24];
    udp[0..2].copy_from_slice(&5353u16.to_be_bytes());
    udp[2..4].copy_from_slice(&53u16.to_be_bytes());
    udp[4..6].copy_from_slice(&24u16.to_be_bytes());
    for (i, b) in udp[8..].iter_mut().enumerate() {
        *b = i as u8;
    }
    PF_STATUS.reass.set(PF_REASS_ENABLED);

    let mut reason = 0u16;
    let mut pd = PfPdesc::new();
    let m1 = test_packet(&frag6(src, dst, 0, true, &udp[..16]));
    assert_eq!(
        pf_setup_pdesc(&mut pd, AF_INET6, PF_IN, None, m1, &mut reason),
        PF_PASS
    );
    assert_eq!((pd.fragoff, pd.virtual_proto), (40, PF_VPROTO_FRAGMENT));
    assert_eq!(
        crate::net::pf_norm::pf_normalize_ip6(&mut pd, &mut reason),
        PF_PASS
    );
    assert!(pd.m.is_none(), "the first fragment waits in the cache");

    let m2 = test_packet(&frag6(src, dst, 16, false, &udp[16..]));
    assert_eq!(
        pf_setup_pdesc(&mut pd, AF_INET6, PF_IN, None, m2, &mut reason),
        PF_PASS
    );
    assert_eq!(
        crate::net::pf_norm::pf_normalize_ip6(&mut pd, &mut reason),
        PF_PASS
    );
    let m = pd.m.expect("reassembled");
    assert_eq!(m.m_pkthdr().len.get(), 40 + 24);
    let h = crate::netinet6::ip6_var::mtod_ip6(m);
    assert_eq!(
        (u16::from_be(h.ip6_plen), i32::from(h.ip6_nxt)),
        (24, IPPROTO_UDP)
    );
    let mut got = [0u8; 24];
    crate::kern::uipc_mbuf::m_copydata(m, 40, &mut got);
    assert_eq!(&got[..], &udp[..]);
    let tag =
        crate::kern::uipc_mbuf2::m_tag_find(m, crate::sys::mbuf::PACKET_TAG_PF_REASSEMBLED, None);
    assert!(tag.is_some(), "pf_refragment6's tag");
    crate::kern::uipc_mbuf::m_freem(m);
    PF_STATUS.reass.set(0);
}
