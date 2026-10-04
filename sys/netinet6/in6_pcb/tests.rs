//! Host tests for the IPv6 control blocks: the spread of `in6_pcbhash`, the exact and
//! listening lookups over a table of bound, connected and wildcard IPv6 control blocks,
//! `in6_pcbset_addr`'s conflict check, the addresses `getsockname(2)`/`getpeername(2)`
//! return (the scope zone recovered), `in6_pcbnotify`'s matching and the checks of
//! `in6_pcbaddrisavail`.
//!
//! The control blocks are placed in their hash chains by hand (`place6`), with the hash
//! `in_pcbrehash` computes for `INP_IPV6` ones (`in6_pcbhash`).

use std::boxed::Box;
use std::vec::Vec;
use std::{assert, assert_eq};

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use super::*;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::uipc_mbuf::m_get;
use crate::kern::uipc_socket::soalloc;
use crate::kern::uipc_socket::sorele;
use crate::kern::uipc_socket2::{solock, sounlock};
use crate::netinet::in_pcb::tests::{setup, teardown};
use crate::netinet::in_pcb::{in_pcballoc, in_pcbdetach, in_pcbinit};
use crate::netinet6::in6::tests::a6;
use crate::netinet6::in6_proto::INET6SW;
use crate::sys::endian::htons;
use crate::sys::mbuf::{M_DONTWAIT, M_WAIT, MT_SONAME};
use crate::sys::protosw::{PRC_MSGSIZE, PRC_UNREACH_PORT};
use crate::sys::socket::{AF_INET6, SOCK_RAW};
use crate::sys::socketvar::{SS_NOFDREF, soref};

/// A table of `hashsize` buckets.
fn table(hashsize: i32) -> &'static Inpcbtable {
    let t: &'static Inpcbtable = Box::leak(Box::new(Inpcbtable::new()));
    in_pcbinit(t, hashsize);
    t
}

/// An `INP_IPV6` control block of a raw IPv6 socket in `table`.
fn pcb6(table: &'static Inpcbtable) -> &'static Inpcb {
    let so = soalloc(&INET6SW[3], M_WAIT).expect("socket");
    so.so_type.set(SOCK_RAW);
    in_pcballoc(so, table, M_WAIT).expect("in_pcballoc");
    let inp = pcb_of(so);
    assert!(inp.has_flags(INP_IPV6), "in_pcballoc marks PF_INET6 pcbs");
    inp
}

/// The control block `in_pcballoc` gave `so` (`sotoinpcb(so)`, read without its family
/// check).
pub(crate) fn pcb_of(so: &Socket) -> &'static Inpcb {
    // SAFETY: `in_pcballoc` set `so_pcb` to its `inpcb_pool` item, which stays allocated
    // until the last `in_pcbunref` after `in_pcbdetach`.
    unsafe { &*so.so_pcb.get().cast::<Inpcb>().cast_const() }
}

/// Gives `inp` the quadruple `laddr.lport <-> faddr.fport` (host order ports) and moves it
/// to the hash chain `in6_pcbhash` picks.
fn place6(inp: &'static Inpcb, laddr: In6Addr, lport: u16, faddr: In6Addr, fport: u16) {
    let t = inp.table();
    mtx_enter(&t.inpt_mtx);
    inp.inp_laddr6.set(laddr);
    inp.inp_lport.set(htons(lport));
    inp.inp_faddr6.set(faddr);
    inp.inp_fport.set(htons(fport));
    let hash = in6_pcbhash(
        t,
        rtable_l2(inp.inp_rtableid.get()),
        &faddr,
        htons(fport),
        &laddr,
        htons(lport),
    );
    // SAFETY: the table mutex is held; `inp` is on a hash chain (in_pcballoc put it there)
    // and moves to another one of the same table.
    unsafe {
        ListHead::<InpHash>::remove(inp);
        t.inpt_hashtbl.get()[(hash & t.inpt_mask.get()) as usize].insert_head(inp);
    }
    mtx_leave(&t.inpt_mtx);
}

/// Detaches the control block and lets the socket go, as `rip6_detach` and `soclose` do.
fn release(inp: &'static Inpcb) {
    let so = inp.socket();
    let _ = soref(Some(so));
    solock(so);
    so.set_state(SS_NOFDREF);
    in_pcbdetach(inp);
    sounlock(so);
    sorele(so);
}

/// `addr.port` (host order) as a `sockaddr_in6`.
fn sin6(addr: In6Addr, port: u16) -> SockaddrIn6 {
    SockaddrIn6 {
        sin6_port: htons(port),
        ..SockaddrIn6::with_addr(addr)
    }
}

/// Whether the lookup found `want`; drops the reference it took.
fn found(hit: Option<&'static Inpcb>, want: &Inpcb) -> bool {
    let r = hit.is_some_and(|h| ptr::eq(h, want));
    in_pcbunref(hit);
    r
}

#[test]
fn in6_pcbhash_spreads_quadruples_over_the_buckets() {
    let (_g, _t, _p) = setup();
    let t = table(64);
    let buckets = (t.inpt_mask.get() + 1) as usize;
    let mut count = std::vec![0u32; buckets];
    let l = a6("fd00:77::1");
    for i in 0..4096u32 {
        let mut f = a6("fd00:77::2");
        f.set_s6_addr32(3, i / 64);
        let h = in6_pcbhash(t, 0, &f, htons(443), &l, htons(1024 + (i % 64) as u16));
        count[(h & t.inpt_mask.get()) as usize] += 1;
    }
    let mean = 4096 / buckets as u32;
    let (min, max) = (count.iter().min(), count.iter().max());
    assert!(min.is_some_and(|&m| m >= mean / 3), "{count:?}");
    assert!(max.is_some_and(|&m| m <= mean * 3), "{count:?}");

    // The hash is a function of the whole quadruple and the routing domain.
    let f = a6("fd00:77::2");
    let h = in6_pcbhash(t, 0, &f, htons(1), &l, htons(2));
    assert_eq!(h, in6_pcbhash(t, 0, &f, htons(1), &l, htons(2)));
    assert_ne!(h, in6_pcbhash(t, 1, &f, htons(1), &l, htons(2)));
    assert_ne!(h, in6_pcbhash(t, 0, &l, htons(1), &f, htons(2)));
    assert_ne!(h, in6_pcbhash(t, 0, &f, htons(2), &l, htons(1)));
    teardown();
}

#[test]
fn exact_and_listen_lookups_over_bound_connected_and_wildcard_pcbs() {
    let (_g, _t, _p) = setup();
    let t = table(8);
    let (l, f, g) = (a6("fd00:77::1"), a6("fd00:77::2"), a6("fd00:77::3"));

    let conn = pcb6(t);
    place6(conn, l, 1000, f, 2000);
    let bound = pcb6(t);
    place6(bound, l, 80, IN6ADDR_ANY, 0);
    let wild80 = pcb6(t);
    place6(wild80, IN6ADDR_ANY, 80, IN6ADDR_ANY, 0);
    let wild22 = pcb6(t);
    place6(wild22, IN6ADDR_ANY, 22, IN6ADDR_ANY, 0);

    // The exact lookup: the whole quadruple, nothing else.
    let hit = in6_pcblookup(t, &f, htons(2000), &l, htons(1000), 0);
    assert!(found(hit, conn));
    assert!(in6_pcblookup(t, &f, htons(2001), &l, htons(1000), 0).is_none());
    assert!(in6_pcblookup(t, &g, htons(2000), &l, htons(1000), 0).is_none());
    assert!(in6_pcblookup(t, &f, htons(2000), &IN6ADDR_ANY, htons(1000), 0).is_none());
    // Not the listeners either.
    assert!(in6_pcblookup(t, &f, htons(2000), &l, htons(80), 0).is_none());

    // The listen lookup: the bound address first, then the wildcard.
    assert!(found(
        in6_pcblookup_listen(t, &l, htons(80), None, 0),
        bound
    ));
    assert!(found(
        in6_pcblookup_listen(t, &g, htons(80), None, 0),
        wild80
    ));
    assert!(found(
        in6_pcblookup_listen(t, &l, htons(22), None, 0),
        wild22
    ));
    assert!(in6_pcblookup_listen(t, &l, htons(23), None, 0).is_none());
    // A connected control block is no listener.
    assert!(in6_pcblookup_listen(t, &l, htons(1000), None, 0).is_none());

    // in6_pcbset_addr refuses a quadruple that is taken.
    let other = pcb6(t);
    assert_eq!(
        in6_pcbset_addr(other, &sin6(f, 2000), &sin6(l, 1000), 0),
        Err(Errno::EADDRINUSE)
    );

    for i in [conn, bound, wild80, wild22, other] {
        release(i);
    }
    assert_eq!(t.inpt_count.get(), 0);
    teardown();
}

#[test]
fn getsockname_and_getpeername_recover_the_scope_zone() {
    let (_g, _t, _p) = setup();
    let t = table(1);
    let inp = pcb6(t);
    // fe80::1 on the interface of index 3, as the kernel keeps it (the zone embedded).
    let mut ll = a6("fe80::1");
    ll.set_s6_addr16(1, htons(3));
    inp.inp_laddr6.set(ll);
    inp.inp_lport.set(htons(546));
    inp.inp_faddr6.set(a6("fd00:77::2"));
    inp.inp_fport.set(htons(547));

    let nam = m_get(M_DONTWAIT, MT_SONAME).expect("mbuf");
    // in6_sockaddr(so, nam), without its sotoinpcb.
    in6_setsockaddr(inp, nam);
    assert_eq!(nam.m_len().get() as usize, size_of::<SockaddrIn6>());
    // SAFETY: in6_setsockaddr wrote a `sockaddr_in6`.
    let s = unsafe { mtod::<SockaddrIn6>(nam).read_unaligned() };
    assert_eq!((s.sin6_len, s.sin6_family), (28, AF_INET6));
    assert_eq!(s.sin6_port, htons(546));
    assert_eq!(s.sin6_addr, a6("fe80::1"));
    assert_eq!(s.sin6_scope_id, 3);

    in6_setpeeraddr(inp, nam);
    // SAFETY: in6_setpeeraddr wrote a `sockaddr_in6`.
    let s = unsafe { mtod::<SockaddrIn6>(nam).read_unaligned() };
    assert_eq!(s.sin6_port, htons(547));
    assert_eq!(s.sin6_addr, a6("fd00:77::2"));
    assert_eq!(s.sin6_scope_id, 0);
    crate::kern::uipc_mbuf::m_freem(nam);
    release(inp);
    teardown();
}

/// What `record` saw: the number of calls, the local port and errno of the last one.
static NOTIFIED: AtomicUsize = AtomicUsize::new(0);
static LAST_LPORT: AtomicU32 = AtomicU32::new(0);
static LAST_ERRNO: AtomicU32 = AtomicU32::new(0);

/// An `InpNotifyFn` that records its calls.
fn record(inp: &'static Inpcb, errno: Option<Errno>) {
    NOTIFIED.fetch_add(1, Ordering::Relaxed);
    LAST_LPORT.store(
        u32::from(u16::from_be(inp.inp_lport.get())),
        Ordering::Relaxed,
    );
    LAST_ERRNO.store(errno.map_or(0, |e| e as u32), Ordering::Relaxed);
}

#[test]
fn in6_pcbnotify_reaches_the_matching_connections() {
    let (_g, _t, _p) = setup();
    let t = table(4);
    let (l, f, g) = (a6("fd00:77::1"), a6("fd00:77::2"), a6("fd00:77::3"));
    let a = pcb6(t);
    place6(a, l, 1000, f, 2000);
    let b = pcb6(t);
    place6(b, l, 1001, f, 2000);
    let c = pcb6(t);
    place6(c, l, 1000, g, 2000);
    let pcbs: Vec<_> = std::vec![a, b, c];

    // A message about one connection: its ports and addresses select it.
    NOTIFIED.store(0, Ordering::Relaxed);
    in6_pcbnotify(
        t,
        &sin6(f, 0),
        u32::from(htons(2000)),
        Some(&sin6(l, 0)),
        u32::from(htons(1000)),
        0,
        PRC_UNREACH_PORT,
        ptr::null_mut(),
        Some(record),
    );
    assert_eq!(NOTIFIED.load(Ordering::Relaxed), 1);
    assert_eq!(LAST_LPORT.load(Ordering::Relaxed), 1000);
    assert_eq!(
        LAST_ERRNO.load(Ordering::Relaxed),
        Errno::ECONNREFUSED as u32
    );

    // A dead host: every connection to it, whatever the ports.
    NOTIFIED.store(0, Ordering::Relaxed);
    in6_pcbnotify(
        t,
        &sin6(f, 0),
        0,
        None,
        0,
        0,
        PRC_HOSTDEAD,
        ptr::null_mut(),
        Some(record),
    );
    assert_eq!(NOTIFIED.load(Ordering::Relaxed), 2);
    assert_eq!(LAST_ERRNO.load(Ordering::Relaxed), Errno::EHOSTDOWN as u32);

    // Nothing for the unspecified or a v4-mapped destination, nor without a notifier.
    NOTIFIED.store(0, Ordering::Relaxed);
    for dst in [IN6ADDR_ANY, a6("::ffff:a00:202")] {
        in6_pcbnotify(
            t,
            &sin6(dst, 0),
            0,
            None,
            0,
            0,
            PRC_MSGSIZE,
            ptr::null_mut(),
            Some(record),
        );
    }
    in6_pcbnotify(
        t,
        &sin6(f, 0),
        0,
        None,
        0,
        0,
        PRC_MSGSIZE,
        ptr::null_mut(),
        None,
    );
    assert_eq!(NOTIFIED.load(Ordering::Relaxed), 0);

    for i in pcbs {
        release(i);
    }
    teardown();
}

#[test]
fn in6_pcbaddrisavail_checks_the_address() {
    let (_g, _t, p) = setup();
    let t = table(1);
    let inp = pcb6(t);

    // The wildcard without a port is always available.
    let mut s = sin6(IN6ADDR_ANY, 0);
    assert_eq!(in6_pcbaddrisavail(inp, &mut s, 0, p), Ok(()));
    // No IPv4-mapped addresses.
    let mut s = sin6(a6("::ffff:a00:20f"), 0);
    assert_eq!(
        in6_pcbaddrisavail(inp, &mut s, 0, p),
        Err(Errno::EADDRNOTAVAIL)
    );
    // Not an address of ours, unless SO_BINDANY.
    let mut s = sin6(a6("2001:db8::1"), 0);
    assert_eq!(
        in6_pcbaddrisavail(inp, &mut s, 0, p),
        Err(Errno::EADDRNOTAVAIL)
    );
    inp.socket().so_options.set(SO_BINDANY);
    let mut s = sin6(a6("2001:db8::1"), 0);
    assert_eq!(in6_pcbaddrisavail(inp, &mut s, 0, p), Ok(()));
    // A multicast group is available too, the scope id cleared.
    let mut s = sin6(a6("ff02::1"), 0);
    s.sin6_scope_id = 0;
    assert_eq!(in6_pcbaddrisavail(inp, &mut s, 0, p), Ok(()));
    assert_eq!(s.sin6_scope_id, 0);
    release(inp);
    teardown();
}
