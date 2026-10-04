use core::mem::offset_of;

use super::*;
use crate::reftest::{assert_complete, assert_defines};
use crate::sys::endian::htonl;

/// `a:b:c:d:e:f:g:h` from its eight 16-bit groups.
fn addr(g: [u16; 8]) -> In6Addr {
    let mut a = In6Addr::default();
    for (i, w) in g.iter().enumerate() {
        a.s6_addr[2 * i..2 * i + 2].copy_from_slice(&w.to_be_bytes());
    }
    a
}

#[test]
fn views_are_network_order_words() {
    let lo = IN6ADDR_LOOPBACK;
    assert_eq!(lo.s6_addr32(3), __IPV6_ADDR_INT32_ONE);
    assert_eq!(lo.s6_addr32(3), htonl(1));
    let ll = addr([0xfe80, 0, 0, 0, 0, 0, 0, 1]);
    assert_eq!(ll.s6_addr16(0), __IPV6_ADDR_INT16_ULL);
    let mut a = In6Addr::default();
    a.set_s6_addr32(0, __IPV6_ADDR_INT32_MLL);
    a.set_s6_addr16(7, htons(2));
    assert_eq!(a, IN6ADDR_LINKLOCAL_ALLROUTERS);
    let w = [1, 2, 3, 4];
    let b = In6Addr::from_s6_addr32(w);
    assert_eq!(
        [
            b.s6_addr32(0),
            b.s6_addr32(1),
            b.s6_addr32(2),
            b.s6_addr32(3)
        ],
        w
    );
    assert_eq!(b.s6_addr8(0), b.s6_addr[0]);
}

#[test]
fn address_classes() {
    let any = IN6ADDR_ANY;
    let lo = IN6ADDR_LOOPBACK;
    let ll = addr([0xfe80, 0, 0, 0, 0x0200, 0x5eff, 0xfe00, 0x5301]);
    let ll_wide = addr([0xfebf, 0, 0, 0, 0, 0, 0, 1]);
    let sl = addr([0xfec0, 0, 0, 0, 0, 0, 0, 1]);
    let global = addr([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]);
    let mapped = addr([0, 0, 0, 0, 0, 0xffff, 0xc000, 0x0201]);
    let compat = addr([0, 0, 0, 0, 0, 0, 0xc000, 0x0201]);

    assert!(in6_is_addr_unspecified(&any) && !in6_is_addr_unspecified(&lo));
    assert!(in6_is_addr_loopback(&lo) && !in6_is_addr_loopback(&any));
    assert!(in6_is_addr_linklocal(&ll) && in6_is_addr_linklocal(&ll_wide));
    assert!(!in6_is_addr_linklocal(&sl) && !in6_is_addr_linklocal(&global));
    assert!(in6_is_addr_sitelocal(&sl) && !in6_is_addr_sitelocal(&ll));
    assert!(in6_is_addr_v4mapped(&mapped) && !in6_is_addr_v4mapped(&compat));
    assert!(in6_is_addr_v4compat(&compat) && !in6_is_addr_v4compat(&mapped));
    assert!(!in6_is_addr_v4compat(&any) && !in6_is_addr_v4compat(&lo));
    assert!(!in6_is_addr_multicast(&global));
    assert!(in6_are_addr_equal(&ll, &ll) && !in6_are_addr_equal(&ll, &ll_wide));
}

#[test]
fn multicast_scopes() {
    let nodelocal = IN6ADDR_NODELOCAL_ALLNODES_INIT;
    let linklocal = IN6ADDR_LINKLOCAL_ALLNODES;
    let site = addr([0xff05, 0, 0, 0, 0, 0, 0, 2]);
    let org = addr([0xff08, 0, 0, 0, 0, 0, 0, 2]);
    let global = addr([0xff0e, 0, 0, 0, 0, 0, 0, 0x101]);

    assert!(in6_is_addr_multicast(&nodelocal));
    assert!(in6_is_addr_mc_nodelocal(&nodelocal) && in6_is_addr_mc_intfacelocal(&nodelocal));
    assert!(in6_is_addr_mc_linklocal(&linklocal) && !in6_is_addr_mc_linklocal(&site));
    assert!(in6_is_addr_mc_sitelocal(&site));
    assert!(in6_is_addr_mc_orglocal(&org));
    assert!(in6_is_addr_mc_global(&global));
    assert_eq!(__ipv6_addr_mc_scope(&site), __IPV6_ADDR_SCOPE_SITELOCAL);

    let ll = addr([0xfe80, 0, 0, 0, 0, 0, 0, 1]);
    assert!(in6_is_scope_linklocal(&ll) && in6_is_scope_linklocal(&linklocal));
    assert!(!in6_is_scope_linklocal(&nodelocal));
    assert!(in6_is_scope_embed(&ll) && in6_is_scope_embed(&nodelocal));
    assert!(!in6_is_scope_embed(&site) && !in6_is_scope_embed(&IN6ADDR_LOOPBACK));
}

#[test]
fn masks_and_socket_addresses() {
    assert_eq!(IN6MASK64.s6_addr32(1), 0xffff_ffff);
    assert_eq!(IN6MASK64.s6_addr32(2), 0);
    assert_eq!(IN6MASK96.s6_addr32(2), 0xffff_ffff);
    assert_eq!(IN6MASK128, In6Addr::new([0xff; 16]));
    assert_eq!(SA6_ANY.sin6_len as usize, size_of::<SockaddrIn6>());
    assert_eq!(SA6_ANY.sin6_family, AF_INET6);
    assert!(in6_is_addr_unspecified(&SA6_ANY.sin6_addr));
    assert_eq!(offset_of!(SockaddrIn6, sin6_flowinfo), 4);
    assert_eq!(offset_of!(SockaddrIn6, sin6_addr), 8);
    assert_eq!(offset_of!(SockaddrIn6, sin6_scope_id), 24);
    let mut sin6 = SockaddrIn6::zeroed();
    assert_eq!(satosin6(sin6tosa(&mut sin6)), &mut sin6 as *mut SockaddrIn6);
    assert_eq!(CTL_IPV6PROTO_NAMES[41].ctl_name, Some(&b"ip6"[..]));
    assert_eq!(IPV6CTL_NAMES[3].ctl_name, Some(&b"hlim"[..]));
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/netinet6/in6.h");
    assert_defines!(defs; INET6_ADDRSTRLEN, ICMP6_FILTER, IPSEC6_OUTSA);
    let scope = assert_defines!(defs;
        __IPV6_ADDR_SCOPE_NODELOCAL, __IPV6_ADDR_SCOPE_INTFACELOCAL, __IPV6_ADDR_SCOPE_LINKLOCAL,
        __IPV6_ADDR_SCOPE_SITELOCAL, __IPV6_ADDR_SCOPE_ORGLOCAL, __IPV6_ADDR_SCOPE_GLOBAL);
    assert_complete(&defs, "__IPV6_ADDR_SCOPE_", &scope);
    let ipv6 = assert_defines!(defs;
        IPV6_UNICAST_HOPS, IPV6_MULTICAST_IF, IPV6_MULTICAST_HOPS, IPV6_MULTICAST_LOOP,
        IPV6_JOIN_GROUP, IPV6_LEAVE_GROUP, IPV6_PORTRANGE, IPV6_CHECKSUM, IPV6_V6ONLY,
        IPV6_RTHDRDSTOPTS, IPV6_RECVPKTINFO, IPV6_RECVHOPLIMIT, IPV6_RECVRTHDR,
        IPV6_RECVHOPOPTS, IPV6_RECVDSTOPTS, IPV6_USE_MIN_MTU, IPV6_RECVPATHMTU, IPV6_PATHMTU,
        IPV6_PKTINFO, IPV6_HOPLIMIT, IPV6_NEXTHOP, IPV6_HOPOPTS, IPV6_DSTOPTS, IPV6_RTHDR,
        IPV6_AUTH_LEVEL, IPV6_ESP_TRANS_LEVEL, IPV6_ESP_NETWORK_LEVEL, IPV6_RECVTCLASS,
        IPV6_AUTOFLOWLABEL, IPV6_IPCOMP_LEVEL, IPV6_TCLASS, IPV6_DONTFRAG, IPV6_PIPEX,
        IPV6_RECVDSTPORT, IPV6_MINHOPCOUNT, IPV6_RTABLE, IPV6_RTHDR_LOOSE, IPV6_RTHDR_TYPE_0,
        IPV6_DEFAULT_MULTICAST_HOPS, IPV6_DEFAULT_MULTICAST_LOOP, IPV6_PORTRANGE_DEFAULT,
        IPV6_PORTRANGE_HIGH, IPV6_PORTRANGE_LOW);
    assert_complete(&defs, "IPV6_", &ipv6);
    let ctl = assert_defines!(defs;
        IPV6CTL_FORWARDING, IPV6CTL_SENDREDIRECTS, IPV6CTL_DEFHLIM, IPV6CTL_FORWSRCRT,
        IPV6CTL_STATS, IPV6CTL_MRTSTATS, IPV6CTL_MRTPROTO, IPV6CTL_MAXFRAGPACKETS,
        IPV6CTL_SOURCECHECK, IPV6CTL_SOURCECHECK_LOGINT, IPV6CTL_ACCEPT_RTADV,
        IPV6CTL_LOG_INTERVAL, IPV6CTL_HDRNESTLIMIT, IPV6CTL_DAD_COUNT, IPV6CTL_AUTO_FLOWLABEL,
        IPV6CTL_DEFMCASTHLIM, IPV6CTL_MAXFRAGS, IPV6CTL_MFORWARDING, IPV6CTL_MULTIPATH,
        IPV6CTL_MCAST_PMTU, IPV6CTL_NEIGHBORGCTHRESH, IPV6CTL_MAXDYNROUTES,
        IPV6CTL_DAD_PENDING, IPV6CTL_MTUDISCTIMEOUT, IPV6CTL_IFQUEUE, IPV6CTL_MRTMIF,
        IPV6CTL_MRTMFC, IPV6CTL_MAXID);
    assert_complete(&defs, "IPV6CTL_", &[&ctl[..], &["IPV6CTL_NAMES"]].concat());
    assert_eq!(defs["IPV6PROTO_MAXID"], "(IPPROTO_DIVERT + 1)");
}

// ---- in6.c ----

use std::sync::MutexGuard;
use std::vec::Vec;

use crate::net::if_::tests::{setup_net, test_ifnet, test_packet, zeroed_static};
use crate::net::if_::{IFF_MULTICAST, IFF_RUNNING, IFF_UP, if_attach, ifa_add};
use crate::netinet6::in6_var::ia6_in6;
use crate::netinet6::in6_var::{
    IN6_IFF_ANYCAST, IN6_IFF_DEPRECATED, IN6_IFF_DETACHED, IN6_IFF_DUPLICATED, IN6_IFF_TEMPORARY,
    IN6_IFF_TENTATIVE, In6Addrlifetime,
};
use crate::sys::systm::{net_lock, net_unlock};

/// The network test setup.
pub(crate) fn setup() -> MutexGuard<'static, ()> {
    setup_net()
}

/// An attached interface `name`, up and running, with multicast.
pub(crate) fn test_if(name: &[u8]) -> &'static Ifnet {
    let ifp = test_ifnet(name);
    ifp.if_flags.set(IFF_UP | IFF_RUNNING | IFF_MULTICAST);
    ifp.if_mtu.set(1500);
    if_attach(ifp);
    ifp
}

/// An IPv6 address `a`/64 with `flags` on `ifp`, linked into its address list as
/// `in6_update_ifa` does, with infinite lifetimes.
pub(crate) fn test_ia6(ifp: &'static Ifnet, a: In6Addr, flags: i32) -> &'static In6Ifaddr {
    // SAFETY: the all-zero `In6Ifaddr` is valid (`netinet6/in6_var.rs`).
    let ia: &'static In6Ifaddr = unsafe { zeroed_static() };
    ia.ia6_memberships.init();
    ia.ia_ifa.ifa_addr.set(ia.ia_addr.as_ptr().cast());
    ia.ia_ifa.ifa_netmask.set(ia.ia_prefixmask.as_ptr().cast());
    let mut sin6 = SockaddrIn6::with_addr(a);
    sin6.sin6_len = size_of::<SockaddrIn6>() as u8;
    ia.ia_addr.set(sin6);
    let mut mask = SockaddrIn6::with_addr(IN6MASK64);
    mask.sin6_len = size_of::<SockaddrIn6>() as u8;
    ia.ia_prefixmask.set(mask);
    ia.ia_ifp().set(Some(ifp));
    ia.ia6_flags.set(flags);
    ia.ia6_lifetime.set(In6Addrlifetime {
        ia6t_vltime: ND6_INFINITE_LIFETIME,
        ia6t_pltime: ND6_INFINITE_LIFETIME,
        ..In6Addrlifetime::default()
    });
    net_lock();
    // SAFETY: the address is leaked (lives for the test run), on no list yet.
    unsafe { ifa_add(ifp, &ia.ia_ifa) };
    net_unlock();
    ia
}

/// `a:b::c` text to an address (the forms the tests use).
pub(crate) fn a6(s: &str) -> In6Addr {
    let (head, tail) = s.split_once("::").unwrap_or((s, ""));
    let parse = |p: &str| -> Vec<u16> {
        p.split(':')
            .filter(|g| !g.is_empty())
            .map(|g| u16::from_str_radix(g, 16).expect("hex group"))
            .collect()
    };
    let (h, t) = (parse(head), parse(tail));
    let mut g = [0u16; 8];
    g[..h.len()].copy_from_slice(&h);
    g[8 - t.len()..].copy_from_slice(&t);
    addr(g)
}

#[test]
fn mask2len_and_prefixlen2mask_round_trip() {
    for len in 0..=128 {
        let mut m = In6Addr::default();
        in6_prefixlen2mask(&mut m, len);
        assert_eq!(in6_mask2len(&m, None), len, "len {len}");
        assert_eq!(in6_mask2len(&m, Some(16)), len);
        // Bit by bit: the first `len` bits are set.
        for bit in 0..128usize {
            let set = m.s6_addr[bit / 8] & (0x80 >> (bit % 8)) != 0;
            assert_eq!(set, (bit as i32) < len, "len {len} bit {bit}");
        }
    }
    assert_eq!(IN6MASK0, {
        let mut m = IN6MASK128;
        in6_prefixlen2mask(&mut m, 0);
        m
    });
    for (len, want) in [
        (32, IN6MASK32),
        (64, IN6MASK64),
        (96, IN6MASK96),
        (128, IN6MASK128),
    ] {
        let mut m = In6Addr::default();
        in6_prefixlen2mask(&mut m, len);
        assert_eq!(m, want);
    }
}

#[test]
fn prefixlen2mask_ignores_invalid_lengths() {
    let _g = setup();
    for bad in [-1, 129, i32::MAX, i32::MIN] {
        let mut m = IN6MASK64;
        in6_prefixlen2mask(&mut m, bad);
        assert_eq!(m, IN6MASK64, "an invalid length leaves the mask alone");
    }
}

#[test]
fn mask2len_rejects_non_contiguous_masks() {
    let mut m = IN6MASK64;
    m.s6_addr[15] = 1;
    assert_eq!(in6_mask2len(&m, None), -1);
    let mut m = In6Addr::default();
    m.s6_addr[0] = 0xff;
    m.s6_addr[1] = 0x80;
    m.s6_addr[3] = 0x01;
    assert_eq!(in6_mask2len(&m, None), -1);
    // A hole inside a byte.
    m = In6Addr::default();
    m.s6_addr[0] = 0xff;
    m.s6_addr[1] = 0xc0;
    m.s6_addr[2] = 0x40;
    assert_eq!(in6_mask2len(&m, None), -1);
    m.s6_addr[1] = 0xa0;
    m.s6_addr[2] = 0;
    assert_eq!(in6_mask2len(&m, None), -1);
}

#[test]
fn mask2len_limit_ignores_what_is_beyond_it() {
    // ff ff 80 00 | garbage: with a limit of 4 bytes only the first four count.
    let mut m = In6Addr::default();
    m.s6_addr[..4].copy_from_slice(&[0xff, 0xff, 0x80, 0x00]);
    m.s6_addr[8] = 0xff;
    assert_eq!(in6_mask2len(&m, Some(4)), 17);
    assert_eq!(in6_mask2len(&m, None), -1);
    // The stricter check inside the limit.
    m.s6_addr[3] = 0x01;
    assert_eq!(in6_mask2len(&m, Some(4)), -1);
    // A limit of zero bytes is an empty mask; more than 16 is the whole address.
    assert_eq!(in6_mask2len(&IN6MASK64, Some(0)), 0);
    assert_eq!(in6_mask2len(&IN6MASK64, Some(20)), 64);
    assert_eq!(in6_mask2len(&IN6MASK128, Some(17)), 128);
}

#[test]
fn matchlen_counts_the_common_prefix() {
    let x = a6("2001:db8::1");
    assert_eq!(in6_matchlen(&x, &x), 128);
    assert_eq!(in6_matchlen(&x, &a6("2001:db8::3")), 126);
    assert_eq!(in6_matchlen(&x, &a6("2001:db8::0")), 127);
    assert_eq!(in6_matchlen(&x, &a6("2001:db9::1")), 31);
    assert_eq!(in6_matchlen(&x, &a6("a001:db8::1")), 0);
    assert_eq!(in6_matchlen(&x, &a6("2001:db8:8000::1")), 32);
    assert_eq!(in6_matchlen(&a6("fe80::1"), &a6("fe80::2")), 126);
    assert_eq!(in6_matchlen(&IN6ADDR_ANY, &IN6ADDR_LOOPBACK), 127);
}

#[test]
fn address_scopes() {
    use crate::netinet6::in6::{
        __IPV6_ADDR_SCOPE_GLOBAL as G, __IPV6_ADDR_SCOPE_INTFACELOCAL as I,
        __IPV6_ADDR_SCOPE_LINKLOCAL as L, __IPV6_ADDR_SCOPE_SITELOCAL as S,
    };
    let scope = |s: &str| in6_addrscope(&a6(s));
    assert_eq!(scope("fe80::1"), i32::from(L));
    assert_eq!(scope("fec0::1"), i32::from(S));
    assert_eq!(scope("fe00::1"), i32::from(G)); // "just in case"
    assert_eq!(scope("ff01::1"), i32::from(I));
    assert_eq!(scope("ff02::1"), i32::from(L));
    assert_eq!(scope("ff05::1"), i32::from(S));
    assert_eq!(scope("ff0e::1"), i32::from(G));
    assert_eq!(scope("ff08::1"), i32::from(G)); // organization-local: global
    assert_eq!(scope("::1"), i32::from(I));
    assert_eq!(scope("::"), i32::from(L));
    assert_eq!(scope("::2"), i32::from(G));
    assert_eq!(scope("2001:db8::1"), i32::from(G));
    assert_eq!(scope("fd00:77::1"), i32::from(G));
}

#[test]
fn scope_zone_ids() {
    assert_eq!(in6_addr2scopeid(3, &a6("fe80::1")), 3);
    assert_eq!(in6_addr2scopeid(3, &a6("ff02::1")), 3);
    assert_eq!(in6_addr2scopeid(3, &a6("::1")), 3);
    assert_eq!(in6_addr2scopeid(3, &a6("ff01::2")), 3);
    assert_eq!(in6_addr2scopeid(3, &a6("fec0::1")), 0);
    assert_eq!(in6_addr2scopeid(3, &a6("2001:db8::1")), 0);
}

#[test]
fn embedded_scope_checks() {
    let mut sa = SockaddrIn6::with_addr(a6("fe80::1"));
    assert_eq!(in6_check_embed_scope(&mut sa, 7), Ok(()));
    assert_eq!(sa.sin6_addr.s6_addr16(1), htons(7));
    assert_eq!(in6_check_embed_scope(&mut sa, 7), Ok(()));
    assert_eq!(in6_check_embed_scope(&mut sa, 8), Err(Errno::EINVAL));
    // Not link-local: untouched.
    let mut g = SockaddrIn6::with_addr(a6("2001:db8::1"));
    assert_eq!(in6_check_embed_scope(&mut g, 7), Ok(()));
    assert_eq!(g.sin6_addr, a6("2001:db8::1"));

    let mut sa = SockaddrIn6::with_addr(a6("fe80::1"));
    sa.sin6_scope_id = 7;
    assert_eq!(in6_clear_scope_id(&mut sa, 8), Err(Errno::EINVAL));
    assert_eq!(in6_clear_scope_id(&mut sa, 7), Ok(()));
    assert_eq!(sa.sin6_scope_id, 0);
    let mut g = SockaddrIn6::with_addr(a6("2001:db8::1"));
    g.sin6_scope_id = 9;
    assert_eq!(in6_clear_scope_id(&mut g, 7), Ok(()));
    assert_eq!(g.sin6_scope_id, 9);
}

#[test]
fn sockaddr_checks() {
    let _g = setup();
    let mut sin6 = SockaddrIn6::with_addr(a6("fd00::1"));
    sin6.sin6_len = size_of::<SockaddrIn6>() as u8;
    // SAFETY: a local `sockaddr_in6`.
    let ok = unsafe { in6_sa2sin6(sin6tosa(&mut sin6)) };
    assert_eq!(ok, Ok(&mut sin6 as *mut SockaddrIn6));
    sin6.sin6_len = 16;
    // SAFETY: as above.
    let r = unsafe { in6_sa2sin6(sin6tosa(&mut sin6)) };
    assert_eq!(r, Err(Errno::EINVAL));
    sin6.sin6_len = size_of::<SockaddrIn6>() as u8;
    sin6.sin6_family = crate::sys::socket::AF_INET;
    // SAFETY: as above.
    let r = unsafe { in6_sa2sin6(sin6tosa(&mut sin6)) };
    assert_eq!(r, Err(Errno::EAFNOSUPPORT));

    // From an mbuf: the length must be the mbuf's and a sockaddr_in6's.
    sin6.sin6_family = AF_INET6;
    // SAFETY: `SockaddrIn6` is plain data.
    let bytes = unsafe {
        core::slice::from_raw_parts(ptr::from_ref(&sin6).cast::<u8>(), size_of::<SockaddrIn6>())
    };
    let m = test_packet(bytes);
    assert!(in6_nam2sin6(m).is_ok());
    m.m_len().set(size_of::<SockaddrIn6>() as u32 - 1);
    assert_eq!(in6_nam2sin6(m).err(), Some(Errno::EINVAL));
    m.m_len().set(1);
    assert_eq!(in6_nam2sin6(m).err(), Some(Errno::EINVAL));
    crate::kern::uipc_mbuf::m_freem(m);
}

#[test]
fn do_dad_needs_an_up_and_running_non_loopback_interface() {
    let ifp = test_ifnet(b"tdad0");
    assert!(!in6if_do_dad(ifp));
    ifp.if_flags.set(IFF_UP);
    assert!(!in6if_do_dad(ifp));
    ifp.if_flags.set(IFF_UP | IFF_RUNNING);
    assert!(in6if_do_dad(ifp));
    ifp.if_flags.set(IFF_UP | IFF_RUNNING | IFF_LOOPBACK);
    assert!(!in6if_do_dad(ifp));
}

#[test]
fn address_lookup_on_an_interface() {
    let _g = setup();
    let ifp = test_if(b"tlk0");
    net_lock();
    assert!(in6ifa_ifpforlinklocal(ifp, 0).is_none());
    net_unlock();
    let ll = test_ia6(ifp, a6("fe80:1::5054:ff:febb:2"), IN6_IFF_TENTATIVE);
    let g = test_ia6(ifp, a6("fd00:77::1"), 0);
    assert!(ptr::eq(
        in6ifa_ifpwithaddr(ifp, &a6("fd00:77::1")).expect("on list"),
        g
    ));
    assert!(in6ifa_ifpwithaddr(ifp, &a6("fd00:77::2")).is_none());
    // The link-local address is found unless its flags are ignored.
    assert!(ptr::eq(
        in6ifa_ifpforlinklocal(ifp, 0).expect("link-local"),
        ll
    ));
    assert!(ptr::eq(
        in6ifa_ifpforlinklocal(ifp, IN6_IFF_ANYCAST).expect("link-local"),
        ll
    ));
    assert!(in6ifa_ifpforlinklocal(ifp, IN6_IFF_TENTATIVE).is_none());
}

#[test]
fn source_address_follows_the_rfc_6724_rules() {
    let _g = setup();
    let if1 = test_if(b"tsa0");
    let if2 = test_if(b"tsa1");
    let ll1 = test_ia6(if1, a6("fe80:1::1"), 0);
    let ula1 = test_ia6(if1, a6("fd00::1"), 0);
    let doc2 = test_ia6(if2, a6("2001:db8::5"), 0);
    net_lock();
    let pick = |oifp: &Ifnet, dst: &str| in6_ifawithscope(oifp, &a6(dst), 0, None);

    // Global destination: the global address on the output interface (rule 5), not the
    // link-local one (rule 2) nor the other interface's.
    assert!(ptr::eq(pick(if1, "2001:db8::99").expect("src"), ula1));
    assert!(ptr::eq(pick(if2, "2001:db8::99").expect("src"), doc2));
    // Link-local destination: the zone is the output interface's, the smallest scope that is
    // big enough.
    assert!(ptr::eq(pick(if1, "fe80:1::2").expect("src"), ll1));
    // A routing domain without interfaces has no address.
    assert!(in6_ifawithscope(if1, &a6("2001:db8::99"), 5, None).is_none());
    net_unlock();
}

#[test]
fn source_selection_skips_unusable_addresses_and_avoids_deprecated_ones() {
    let _g = setup();
    let ifp = test_if(b"tsb0");
    // All unusable: tentative, duplicated, anycast, detached.
    test_ia6(ifp, a6("2001:db8::10"), IN6_IFF_TENTATIVE);
    test_ia6(ifp, a6("2001:db8::11"), IN6_IFF_DUPLICATED);
    test_ia6(ifp, a6("2001:db8::12"), IN6_IFF_ANYCAST);
    test_ia6(ifp, a6("2001:db8::13"), IN6_IFF_DETACHED);
    net_lock();
    assert!(in6_ifawithscope(ifp, &a6("2001:db8::99"), 0, None).is_none());
    net_unlock();

    // Rule 3: a deprecated address loses to a preferred one, whatever the order.
    let dep = test_ia6(ifp, a6("2001:db8::99"), IN6_IFF_DEPRECATED);
    net_lock();
    assert!(ptr::eq(
        in6_ifawithscope(ifp, &a6("2001:db8::99"), 0, None).expect("src"),
        dep
    ));
    net_unlock();
    let ok = test_ia6(ifp, a6("2001:db8:1::1"), 0);
    net_lock();
    assert!(ptr::eq(
        in6_ifawithscope(ifp, &a6("2001:db8::99"), 0, None).expect("src"),
        ok
    ));
    net_unlock();
    let ok2 = test_ia6(ifp, a6("2001:db8::7"), 0);
    // Rule 8: of two preferred addresses, the longest matching prefix wins.
    net_lock();
    assert!(ptr::eq(
        in6_ifawithscope(ifp, &a6("2001:db8::99"), 0, None).expect("src"),
        ok2
    ));
    net_unlock();
    // Rule 7: a temporary address is preferred to a public one, even with a shorter match.
    let tmp = test_ia6(ifp, a6("2001:db8::98"), IN6_IFF_TEMPORARY);
    net_lock();
    let best = in6_ifawithscope(ifp, &a6("2001:db8::99"), 0, None).expect("src");
    net_unlock();
    assert!(ptr::eq(best, tmp));
}

#[test]
fn selected_sources_are_counted() {
    use crate::netinet6::ip6_input::IP6COUNTERS;
    use crate::netinet6::ip6_var::Ip6statCounters;
    use core::sync::atomic::Ordering;

    let _g = setup();
    let ifp = test_if(b"tsc0");
    test_ia6(ifp, a6("2001:db8::1"), 0);
    let none = &IP6COUNTERS[Ip6statCounters::Ip6sSourcesNone as usize];
    let global =
        Ip6statCounters::Ip6sSourcesSameif as usize + usize::from(__IPV6_ADDR_SCOPE_GLOBAL);
    let before_none = none.load(Ordering::Relaxed);
    let before_same = IP6COUNTERS[global].load(Ordering::Relaxed);
    net_lock();
    assert!(in6_ifawithscope(ifp, &a6("2001:db8::2"), 0, None).is_some());
    assert!(in6_ifawithscope(ifp, &a6("2001:db8::2"), 9, None).is_none());
    net_unlock();
    assert_eq!(IP6COUNTERS[global].load(Ordering::Relaxed), before_same + 1);
    assert_eq!(none.load(Ordering::Relaxed), before_none + 1);
}

/// A driver `ioctl` that accepts the address and multicast requests (and brings the
/// interface up on the first address, as drivers do).
///
/// # Safety
///
/// `IfIoctlFn`'s contract.
unsafe fn accepting_ioctl(ifp: &'static Ifnet, cmd: u64, _data: *mut u8) -> Result<(), Errno> {
    use crate::sys::sockio::{SIOCADDMULTI, SIOCDELMULTI, SIOCSIFADDR};
    match cmd {
        SIOCSIFADDR => {
            ifp.if_flags.set(ifp.if_flags.get() | IFF_UP | IFF_RUNNING);
            Ok(())
        }
        SIOCADDMULTI | SIOCDELMULTI => Ok(()),
        _ => Err(Errno::ENOTTY),
    }
}

/// An attached Ethernet-like interface `name` with hardware address `mac` and a driver that
/// accepts what the IPv6 address code asks of it.
fn test_driver_if(name: &[u8], mac: [u8; 6]) -> &'static Ifnet {
    let ifp = test_ifnet(name);
    ifp.if_ioctl.set(Some(accepting_ioctl));
    ifp.if_type.set(crate::net::if_types::IFT_ETHER);
    ifp.if_flags.set(IFF_MULTICAST);
    ifp.if_mtu.set(1500);
    let mut sdl = crate::net::if_dl::SockaddrDl {
        sdl_alen: 6,
        ..Default::default()
    };
    sdl.sdl_data[..6].copy_from_slice(&mac);
    ifp.if_sadl
        .set(std::boxed::Box::leak(std::boxed::Box::new(sdl)));
    if_attach(ifp);
    ifp
}

/// `ifconfig <ifp> inet6 <addr> prefixlen <plen>`, from the kernel: `SIOCAIFADDR_IN6`.
fn configure6(ifp: &'static Ifnet, a: In6Addr, plen: i32) -> Result<(), Errno> {
    let mut ifra = In6Aliasreq::zeroed();
    ifra.ifra_name = ifp.if_xname.get();
    *ifra.ifra_addr_mut() = SockaddrIn6::with_addr(a);
    ifra.ifra_prefixmask = SockaddrIn6::with_addr(IN6ADDR_ANY);
    in6_prefixlen2mask(&mut ifra.ifra_prefixmask.sin6_addr, plen);
    ifra.ifra_lifetime.ia6t_vltime = ND6_INFINITE_LIFETIME;
    ifra.ifra_lifetime.ia6t_pltime = ND6_INFINITE_LIFETIME;
    // SAFETY: an `in6_aliasreq`, from the kernel itself, aligned as a local.
    unsafe {
        in6_ioctl(
            SIOCAIFADDR_IN6,
            ptr::from_mut(&mut ifra).cast(),
            Some(ifp),
            true,
        )
    }
}

#[test]
#[ignore = "needs if_output_tso's AF_INET6 case (if.c INET6) and an if_output for test_driver_if (the MLD reports go out)"]
fn siocaifaddr_in6_makes_the_address_the_link_local_one_and_the_memberships() {
    use crate::netinet::ip_input::tests::{OURS, setup as setup_ip};

    let (_g, _t) = setup_ip();
    let ifp = test_driver_if(b"tcf0", OURS);
    let idx = ifp.if_index.get();

    configure6(ifp, a6("fd00:77::1"), 64).expect("SIOCAIFADDR_IN6");

    let ia = in6ifa_ifpwithaddr(ifp, &a6("fd00:77::1")).expect("the address");
    assert_eq!(in6_mask2len(&ia6_maskin6(ia), None), 64);
    assert_eq!(ia.ia6_flags.get() & IN6_IFF_DUPLICATED, 0);

    // The link-local address of the MAC's EUI-64 was made first (fe80::5054:ff:fe12:3456,
    // the zone embedded as the interface index).
    let ll = in6ifa_ifpforlinklocal(ifp, 0).expect("link-local address");
    let mut want = a6("fe80::5054:ff:fe12:3456");
    want.set_s6_addr16(1, htons(idx as u16));
    assert_eq!(ia6_in6(ll), want);
    assert_eq!(OURS[..2], [0x52, 0x54]);

    // The multicast groups joined: solicited-node of both, all-nodes link- and
    // interface-local.
    net_lock();
    let joined = |g: In6Addr| {
        crate::kern::kern_rwlock::rw_enter_read(&ifp.if_maddrlock);
        let r = in6_lookupmulti(&g, ifp).is_some();
        crate::kern::kern_rwlock::rw_exit_read(&ifp.if_maddrlock);
        r
    };
    let mut snm = a6("ff02::1:ff00:1");
    snm.set_s6_addr16(1, htons(idx as u16));
    assert!(joined(snm), "solicited-node group of fd00:77::1");
    let mut allnodes = IN6ADDR_LINKLOCAL_ALLNODES;
    allnodes.set_s6_addr16(1, htons(idx as u16));
    assert!(joined(allnodes), "ff02::1");
    net_unlock();

    // The address is deleted again with SIOCDIFADDR_IN6 and takes its group with it.
    let mut ifr = In6Ifreq::zeroed();
    ifr.ifr_name = ifp.if_xname.get();
    ifr.set_ifr_addr(SockaddrIn6::with_addr(a6("fd00:77::1")));
    // SAFETY: an `in6_ifreq`, from the kernel itself.
    unsafe {
        in6_ioctl(
            SIOCDIFADDR_IN6,
            ptr::from_mut(&mut ifr).cast(),
            Some(ifp),
            true,
        )
    }
    .expect("SIOCDIFADDR_IN6");
    assert!(in6ifa_ifpwithaddr(ifp, &a6("fd00:77::1")).is_none());
    assert!(in6ifa_ifpforlinklocal(ifp, 0).is_some());
}

#[test]
fn siocaifaddr_in6_validates_the_request() {
    use crate::netinet::ip_input::tests::{setup as setup_ip, test_ether};

    let (_g, _t) = setup_ip();
    let ifp = test_ether();
    // Not a /0 and a non-contiguous mask.
    assert_eq!(configure6(ifp, a6("fd00:77::1"), 0), Err(Errno::EINVAL));
    // The read-only flags are refused.
    let mut ifra = In6Aliasreq::zeroed();
    *ifra.ifra_addr_mut() = SockaddrIn6::with_addr(a6("fd00:77::2"));
    ifra.ifra_prefixmask = SockaddrIn6::with_addr(IN6MASK64);
    ifra.ifra_flags = IN6_IFF_DUPLICATED;
    // SAFETY: as above.
    let r = unsafe {
        in6_ioctl(
            SIOCAIFADDR_IN6,
            ptr::from_mut(&mut ifra).cast(),
            Some(ifp),
            true,
        )
    };
    assert_eq!(r, Err(Errno::EINVAL));
    // Unprivileged, no interface, a request not for IPv6.
    // SAFETY: as above.
    let r = unsafe {
        in6_ioctl(
            SIOCAIFADDR_IN6,
            ptr::from_mut(&mut ifra).cast(),
            Some(ifp),
            false,
        )
    };
    assert_eq!(r, Err(Errno::EPERM));
    // SAFETY: as above.
    let r = unsafe { in6_ioctl(SIOCAIFADDR_IN6, ptr::from_mut(&mut ifra).cast(), None, true) };
    assert_eq!(r, Err(Errno::ENXIO));
    // SAFETY: as above.
    let r = unsafe {
        in6_ioctl(
            SIOCSIFADDR,
            ptr::from_mut(&mut ifra).cast(),
            Some(ifp),
            true,
        )
    };
    assert_eq!(r, Err(Errno::EINVAL));
}
