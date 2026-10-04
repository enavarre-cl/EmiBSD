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
