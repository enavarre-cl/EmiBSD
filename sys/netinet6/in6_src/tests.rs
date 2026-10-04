//! Host tests for `in6_src.c`: scope embedding and recovery, the source address of
//! link-local and multicast destinations, route selection with a packet info.

use super::*;
use crate::netinet6::in6::IN6ADDR_ANY;
use crate::netinet6::in6::tests::{a6, setup, test_ia6, test_if};
use crate::netinet6::in6_var::IN6_IFF_TENTATIVE;
use crate::sys::systm::{net_lock, net_unlock};

fn sin6(a: &str, scope_id: u32) -> SockaddrIn6 {
    let mut s = SockaddrIn6::with_addr(a6(a));
    s.sin6_scope_id = scope_id;
    s
}

fn pktopts(ifindex: u32, addr: In6Addr) -> (Ip6Pktopts, &'static mut In6Pktinfo) {
    let pi: &'static mut In6Pktinfo = std::boxed::Box::leak(std::boxed::Box::new(In6Pktinfo {
        ipi6_addr: addr,
        ipi6_ifindex: ifindex,
    }));
    let opts = Ip6Pktopts {
        ip6po_pktinfo: ptr::NonNull::new(ptr::from_mut(pi)),
        ..Ip6Pktopts::default()
    };
    (opts, pi)
}

#[test]
fn scope_is_embedded_recovered_and_cleared() {
    let _g = setup();
    let ifp = test_if(b"tsc0");
    let idx = ifp.if_index.get();

    // sin6_scope_id of a link-local address goes into s6_addr16[1].
    let mut out = In6Addr::default();
    in6_embedscope(&mut out, &sin6("fe80::1", idx), None, None).expect("embed");
    assert_eq!(out.s6_addr16(1), htons(idx as u16));
    assert_eq!(&out.s6_addr[..2], [0xfe, 0x80]);
    // A global address is copied as it is, whatever the scope id.
    in6_embedscope(&mut out, &sin6("2001:db8::1", idx), None, None).expect("embed");
    assert_eq!(out, a6("2001:db8::1"));
    // An unknown interface.
    assert_eq!(
        in6_embedscope(&mut out, &sin6("fe80::1", 4000), None, None),
        Err(Errno::ENXIO)
    );
    // No scope id: the zone is left alone.
    in6_embedscope(&mut out, &sin6("fe80::1", 0), None, None).expect("embed");
    assert_eq!(out, a6("fe80::1"));

    // The packet info's interface wins over sin6_scope_id; for a multicast address, the
    // multicast options' wins when there is no packet info.
    let other = test_if(b"tsc1");
    let (opts, _pi) = pktopts(other.if_index.get(), IN6ADDR_ANY);
    in6_embedscope(&mut out, &sin6("fe80::1", idx), Some(&opts), None).expect("embed");
    assert_eq!(out.s6_addr16(1), htons(other.if_index.get() as u16));
    let mopts = Ip6Moptions {
        im6o_memberships: crate::sys::queue::ListHead::new(),
        im6o_ifidx: other.if_index.get() as u16,
        im6o_hlim: 1,
        im6o_loop: 1,
    };
    in6_embedscope(&mut out, &sin6("ff02::1", idx), None, Some(&mopts)).expect("embed");
    assert_eq!(out.s6_addr16(1), htons(other.if_index.get() as u16));
    // ... but not for a unicast address.
    in6_embedscope(&mut out, &sin6("fe80::1", idx), None, Some(&mopts)).expect("embed");
    assert_eq!(out.s6_addr16(1), htons(idx as u16));

    // And back.
    let mut sa = SockaddrIn6::zeroed();
    in6_recoverscope(&mut sa, &out);
    assert_eq!(sa.sin6_addr, a6("fe80::1"));
    assert_eq!(sa.sin6_scope_id, idx);
    in6_recoverscope(&mut sa, &a6("2001:db8::1"));
    assert_eq!(sa.sin6_addr, a6("2001:db8::1"));
    assert_eq!(sa.sin6_scope_id, 0);
    // Not embedded (zero zone): nothing to recover.
    in6_recoverscope(&mut sa, &a6("fe80::9"));
    assert_eq!(sa.sin6_scope_id, 0);

    let mut ll = a6("fe80:5::1");
    in6_clearscope(&mut ll);
    assert_eq!(ll, a6("fe80::1"));
    let mut g = a6("2001:5::1");
    in6_clearscope(&mut g);
    assert_eq!(g, a6("2001:5::1"));
    let mut m = a6("ff02:5::1");
    in6_clearscope(&mut m);
    assert_eq!(m, a6("ff02::1"));
    let mut m = a6("ff01:5::1");
    in6_clearscope(&mut m);
    assert_eq!(m, a6("ff01::1"));
}

#[test]
fn source_for_a_scoped_or_multicast_destination() {
    let _g = setup();
    let if1 = test_if(b"tss0");
    let if2 = test_if(b"tss1");
    let ll1 = test_ia6(if1, a6("fe80:1::1"), 0);
    let bare = test_if(b"tss2");
    let ll2 = test_ia6(if2, a6("fe80:2::1"), 0);
    test_ia6(if2, a6("2001:db8::5"), 0);
    net_lock();
    let mut src = In6Addr::default();

    // A link-local destination with a scope id: the address of that interface.
    in6_selectsrc(&mut src, &sin6("fe80::99", if2.if_index.get()), None, 0).expect("src");
    assert_eq!(src, ia6_in6(ll2));
    in6_selectsrc(&mut src, &sin6("ff02::1", if1.if_index.get()), None, 0).expect("src");
    assert_eq!(src, ia6_in6(ll1));
    assert_eq!(
        in6_selectsrc(&mut src, &sin6("fe80::99", 4000), None, 0),
        Err(Errno::ENXIO)
    );
    assert_eq!(
        in6_selectsrc(&mut src, &sin6("fe80::99", bare.if_index.get()), None, 0),
        Err(Errno::EADDRNOTAVAIL)
    );

    // A multicast destination: the interface of the multicast options.
    let mopts = Ip6Moptions {
        im6o_memberships: crate::sys::queue::ListHead::new(),
        im6o_ifidx: if2.if_index.get() as u16,
        im6o_hlim: 1,
        im6o_loop: 1,
    };
    in6_selectsrc(&mut src, &sin6("ff05::1", 0), Some(&mopts), 0).expect("src");
    assert_eq!(src, a6("2001:db8::5"));
    // None given: nothing to choose from.
    assert_eq!(
        in6_selectsrc(&mut src, &sin6("ff05::1", 0), None, 0),
        Err(Errno::EADDRNOTAVAIL)
    );
    // A unicast global destination is the route's business.
    assert_eq!(
        in6_selectsrc(&mut src, &sin6("2001:db8::99", 0), Some(&mopts), 0),
        Err(Errno::EADDRNOTAVAIL)
    );
    net_unlock();
}

#[test]
fn a_tentative_address_is_not_a_source() {
    let _g = setup();
    let ifp = test_if(b"tst0");
    test_ia6(ifp, a6("fe80:1::1"), IN6_IFF_TENTATIVE);
    net_lock();
    let mut src = In6Addr::default();
    assert_eq!(
        in6_selectsrc(&mut src, &sin6("fe80::99", ifp.if_index.get()), None, 0),
        Err(Errno::EADDRNOTAVAIL)
    );
    net_unlock();
}
