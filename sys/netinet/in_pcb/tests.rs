//! Host tests for the Internet control blocks: the port bitmaps, binding (explicit ports,
//! picked ports, `EADDRINUSE` and `SO_REUSEPORT`), the local port, connection and listen
//! lookups, a table that grows past its load factor, the iterator and detaching.
//!
//! [`setup`] is the network setup of `ip_input`'s tests plus a thread with credentials made
//! `curproc` (sockets read it), the socket pool and `in_init`; the UDP and raw IP tests use it
//! too. [`teardown`] clears `curproc`.

use std::boxed::Box;
use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq};

use super::*;
use crate::kern::kern_prot::{crget, crhold};
use crate::kern::uipc_mbuf::m_get;
use crate::kern::uipc_socket::{soalloc, soinit};
use crate::kern::uipc_socket2::{solock, sounlock};
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::netinet::in_proto::INETSW;
use crate::sys::mbuf::{M_DONTWAIT, MT_SONAME};
use crate::sys::proc::Process;

/// The network setup, a `curproc` with credentials, sockets and control blocks.
pub(crate) fn setup() -> (
    MutexGuard<'static, ()>,
    MutexGuard<'static, ()>,
    &'static Proc,
) {
    let (g, t) = crate::netinet::ip_input::tests::setup();
    crate::kern::kern_proc::procinit();
    let pr: &'static Process = Box::leak(Box::new(Process::new()));
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    p.p_p.set(pr);
    pr.ps_mainproc.set(p);
    let cr = crget();
    p.p_ucred.set(cr);
    pr.ps_ucred.set(crhold(cr));
    Machine::set_curproc(Machine::curcpu(), p);
    soinit();
    in_init();
    (g, t, p)
}

/// Undoes what outlives the reset memory: `curproc`.
pub(crate) fn teardown() {
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// An address mbuf holding `sin`.
pub(crate) fn nam(sin: SockaddrIn) -> &'static Mbuf {
    let m = m_get(M_DONTWAIT, MT_SONAME).expect("mbuf");
    m.m_len().set(size_of::<SockaddrIn>() as u32);
    // SAFETY: a fresh mbuf of `MLEN` bytes.
    unsafe { mtod::<SockaddrIn>(m).write_unaligned(sin) };
    m
}

/// `addr:port` as a `sockaddr_in`.
fn sin(addr: [u8; 4], port: u16) -> SockaddrIn {
    SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        sin_port: htons(port),
        sin_addr: InAddr {
            s_addr: u32::from_ne_bytes(addr),
        },
        ..SockaddrIn::default()
    }
}

/// A UDP socket with a control block in `table`.
fn pcb(table: &'static Inpcbtable) -> &'static Inpcb {
    let so = soalloc(&INETSW[1], M_WAIT).expect("socket");
    so.so_type.set(SOCK_DGRAM);
    in_pcballoc(so, table, M_WAIT).expect("in_pcballoc");
    sotoinpcb(so).expect("attached")
}

/// Detaches the control block and lets the socket go, as `udp_detach` and `soclose` do
/// (holding a reference of their own across the unlock).
fn release(inp: &'static Inpcb) {
    let so = inp.socket();
    let _ = soref(Some(so));
    solock(so);
    so.set_state(SS_NOFDREF);
    in_pcbdetach(inp);
    sounlock(so);
    sorele(so);
}

/// A table of `hashsize` buckets.
fn table(hashsize: i32) -> &'static Inpcbtable {
    let t: &'static Inpcbtable = Box::leak(Box::new(Inpcbtable::new()));
    in_pcbinit(t, hashsize);
    t
}

#[test]
fn the_port_bitmaps_and_their_defaults() {
    let (_g, _t, _p) = setup();
    let m: Box<[AtomicU32; DP_MAPSIZE]> = Box::new([const { AtomicU32::new(0) }; DP_MAPSIZE]);
    assert_eq!(DP_MAPSIZE, 2048);
    dp_set(&m, 65535);
    dp_set(&m, 33);
    assert!(dp_isset(&m, 65535) && dp_isset(&m, 33) && !dp_isset(&m, 32));
    dp_clr(&m, 33);
    assert!(!dp_isset(&m, 33));

    // ip_init filled the defaults.
    assert!(in_baddynamic(2049, IPPROTO_TCP as u16));
    assert!(in_baddynamic(7784, IPPROTO_UDP as u16));
    assert!(!in_baddynamic(7784, IPPROTO_TCP as u16));
    assert!(!in_baddynamic(2049, 1));
    assert!(in_rootonly(80, IPPROTO_UDP as u16));
    assert!(in_rootonly(2049, IPPROTO_UDP as u16));
    assert!(!in_rootonly(5353, IPPROTO_UDP as u16));
    teardown();
}

#[test]
fn bind_lookups_and_the_iterator() {
    let (_g, _t, p) = setup();
    let t = table(1);

    // An explicit port on the wildcard address.
    let a = pcb(t);
    let n = nam(sin([0; 4], 5000));
    in_pcbbind(a, Some(n), p).expect("bind");
    assert_eq!(a.inp_lport.get(), htons(5000));
    let found = in_pcblookup_local_lock(t, &ZEROIN_ADDR, htons(5000), 0, 0, IN_PCBLOCK_GRAB);
    assert!(found.is_some_and(|f| ptr::eq(f, a)));
    in_pcbunref(found);
    let any = sin([10, 0, 2, 15], 0).sin_addr;
    assert!(in_pcblookup_local_lock(t, &any, htons(5000), 0, 0, IN_PCBLOCK_GRAB).is_none());
    let wild =
        in_pcblookup_local_lock(t, &any, htons(5000), INPLOOKUP_WILDCARD, 0, IN_PCBLOCK_GRAB);
    assert!(wild.is_some_and(|f| ptr::eq(f, a)));
    in_pcbunref(wild);
    // Bound already.
    assert_eq!(in_pcbbind(a, Some(n), p), Err(Errno::EINVAL));

    // The same port again (a second socket grows the table past its load factor).
    let b = pcb(t);
    assert!(t.inpt_size.get() > 1, "resized");
    assert_eq!(in_pcbbind(b, Some(n), p), Err(Errno::EADDRINUSE));
    // ... unless both sockets allow it.
    a.socket().so_options.set(SO_REUSEPORT);
    b.socket().so_options.set(SO_REUSEPORT);
    in_pcbbind(b, Some(n), p).expect("SO_REUSEPORT bind");
    // An address that is not ours.
    let c = pcb(t);
    let other = nam(sin([192, 0, 2, 1], 5001));
    assert_eq!(in_pcbbind(c, Some(other), p), Err(Errno::EADDRNOTAVAIL));
    // A picked port, from the default range.
    in_pcbbind(c, None, p).expect("anonymous bind");
    let port = i32::from(u16::from_be(c.inp_lport.get()));
    assert!(
        (IPPORT_RESERVED..=IPPORT_USERRESERVED).contains(&port),
        "port {port}"
    );

    // A connection's quadruple, then the exact and the listen lookups.
    let fsin = sin([10, 0, 2, 2], 53);
    let lsin = sin([10, 0, 2, 15], 6000);
    let d = pcb(t);
    in_pcbset_addr(d, &fsin, &lsin, 0).expect("set_addr");
    assert_eq!(
        in_pcbset_addr(pcb(t), &fsin, &lsin, 0),
        Err(Errno::EADDRINUSE)
    );
    let hit = in_pcblookup(
        t,
        fsin.sin_addr,
        fsin.sin_port,
        lsin.sin_addr,
        lsin.sin_port,
        0,
    );
    assert!(hit.is_some_and(|h| ptr::eq(h, d)));
    in_pcbunref(hit);
    let listen = in_pcblookup_listen(t, lsin.sin_addr, htons(5000), None, 0);
    assert!(listen.is_some_and(|l| ptr::eq(l, a) || ptr::eq(l, b)));
    in_pcbunref(listen);
    assert!(in_pcblookup_listen(t, lsin.sin_addr, htons(5002), None, 0).is_none());

    // in_pcbunset_laddr forgets the quadruple; the exact lookup misses then.
    in_pcbunset_laddr(d);
    assert!(
        in_pcblookup(
            t,
            fsin.sin_addr,
            fsin.sin_port,
            lsin.sin_addr,
            lsin.sin_port,
            0
        )
        .is_none()
    );

    // The iterator sees every control block once, with an aborted walk in between.
    let iter = InpcbIterator::new();
    let mut seen = Vec::new();
    let mut inp = None;
    mtx_enter(&t.inpt_mtx);
    // SAFETY: the mutex is held; `iter` lives until the walk ends.
    while let Some(i) = unsafe { in_pcb_iterator(t, inp, &iter) } {
        inp = Some(i);
        seen.push(ptr::from_ref(i));
    }
    let abort = InpcbIterator::new();
    // SAFETY: as above, ended by the abort.
    let first = unsafe { in_pcb_iterator(t, None, &abort) };
    // SAFETY: the same walk.
    unsafe { in_pcb_iterator_abort(t, first, &abort) };
    mtx_leave(&t.inpt_mtx);
    assert_eq!(seen.len() as i32, t.inpt_count.get());

    for i in [a, b, c, d] {
        release(i);
    }
    assert_eq!(t.inpt_count.get(), 1, "the refused one is left");
    teardown();
}
