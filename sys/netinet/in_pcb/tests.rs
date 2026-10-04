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
    let found = in_pcblookup_local_lock(t, &ZEROIN46_ADDR, htons(5000), 0, 0, IN_PCBLOCK_GRAB);
    assert!(found.is_some_and(|f| ptr::eq(f, a)));
    in_pcbunref(found);
    let any = Inpaddru::from_addr(sin([10, 0, 2, 15], 0).sin_addr);
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
    let (fsu, lsu) = (
        SockaddrUnion::from_sin(&fsin),
        SockaddrUnion::from_sin(&lsin),
    );
    in_pcbset_addr(d, &fsu, &lsu, 0).expect("set_addr");
    assert_eq!(
        in_pcbset_addr(pcb(t), &fsu, &lsu, 0),
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

/// IPv6 helpers shared by the UDP and TCP tests (a test interface with `fd00:77::1/64` whose
/// output the tests read, address mbufs, packets from `fd00:77::2`), and the `INP_IPV6`
/// branches of binding, port picking, connecting and the lookups.
#[cfg(feature = "inet6")]
pub(crate) mod inet6 {
    use std::sync::Mutex as StdMutex;
    use std::{assert, assert_eq, assert_ne, vec, vec::Vec};

    use core::mem::size_of;
    use core::ptr;
    use core::sync::atomic::Ordering;

    use super::{setup, teardown};
    use crate::kern::uipc_mbuf::{m_freem, m_get};
    use crate::kern::uipc_socket::{soclose, socreate};
    use crate::kern::uipc_socket2::{solock, sounlock};
    use crate::net::if_::tests::{test_ifnet, test_packet};
    use crate::net::if_var::Ifnet;
    use crate::net::route::Rtentry;
    use crate::netinet::in_pcb::*;
    use crate::netinet6::in6::tests::a6;
    use crate::netinet6::in6::{
        IN6ADDR_ANY, In6Addr, SockaddrIn6, in6_ioctl, in6_prefixlen2mask, in6ifa_ifpwithaddr,
    };
    use crate::netinet6::in6_cksum::in6_cksum;
    use crate::netinet6::in6_pcb::in6_pcblookup;
    use crate::netinet6::in6_var::{IN6_IFF_TENTATIVE, In6Aliasreq, SIOCAIFADDR_IN6};
    use crate::netinet6::nd6::ND6_INFINITE_LIFETIME;
    use crate::sys::endian::htons;
    use crate::sys::errno::Errno;
    use crate::sys::mbuf::{M_DONTWAIT, MT_SONAME, Mbuf, mtod};
    use crate::sys::socket::{AF_INET6, SOCK_DGRAM, Sockaddr};

    /// What the IPv6 test interface was asked to send, as bytes from the IPv6 header on.
    pub(crate) static SENT6: StdMutex<Vec<Vec<u8>>> = StdMutex::new(Vec::new());

    /// Takes what the test interface sent so far.
    pub(crate) fn take_sent6() -> Vec<Vec<u8>> {
        core::mem::take(&mut *SENT6.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// A driver `ioctl` that accepts the address and multicast requests (and brings the
    /// interface up on the first address).
    ///
    /// # Safety
    ///
    /// `IfIoctlFn`'s contract.
    unsafe fn accepting_ioctl(ifp: &'static Ifnet, cmd: u64, _data: *mut u8) -> Result<(), Errno> {
        use crate::net::if_::{IFF_RUNNING, IFF_UP};
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

    /// A driver output that keeps the bytes of what it is handed.
    ///
    /// # Safety
    ///
    /// `IfOutputFn`'s contract.
    unsafe fn capturing_output(
        _ifp: &'static Ifnet,
        m: &'static Mbuf,
        _dst: *const Sockaddr,
        _rt: Option<&'static Rtentry>,
    ) -> Result<(), Errno> {
        let b = crate::netinet::ip_input::tests::bytes(m);
        SENT6.lock().unwrap_or_else(|e| e.into_inner()).push(b);
        m_freem(m);
        Ok(())
    }

    /// An attached Ethernet-like interface `name` with `fd00:77::1/64`, usable at once (its
    /// duplicate address detection skipped), whose output lands in [`SENT6`].
    pub(crate) fn test_if6(name: &[u8]) -> &'static Ifnet {
        let ifp = test_ifnet(name);
        ifp.if_ioctl.set(Some(accepting_ioctl));
        ifp.if_output.set(Some(capturing_output));
        ifp.if_type.set(crate::net::if_types::IFT_ETHER);
        ifp.if_flags.set(crate::net::if_::IFF_MULTICAST);
        ifp.if_mtu.set(1500);
        let mut sdl = crate::net::if_dl::SockaddrDl {
            sdl_alen: 6,
            ..Default::default()
        };
        sdl.sdl_data[..6].copy_from_slice(&crate::netinet::ip_input::tests::OURS);
        ifp.if_sadl
            .set(std::boxed::Box::leak(std::boxed::Box::new(sdl)));
        crate::net::if_::if_attach(ifp);

        let mut ifra = In6Aliasreq::zeroed();
        ifra.ifra_name = ifp.if_xname.get();
        *ifra.ifra_addr_mut() = SockaddrIn6::with_addr(a6("fd00:77::1"));
        ifra.ifra_prefixmask = SockaddrIn6::with_addr(IN6ADDR_ANY);
        in6_prefixlen2mask(&mut ifra.ifra_prefixmask.sin6_addr, 64);
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
        .expect("SIOCAIFADDR_IN6");
        let ia = in6ifa_ifpwithaddr(ifp, &a6("fd00:77::1")).expect("the address");
        ia.ia6_flags.set(ia.ia6_flags.get() & !IN6_IFF_TENTATIVE);
        let _ = take_sent6();
        ifp
    }

    /// `[addr]:port` (host order port).
    pub(crate) fn sin6(addr: In6Addr, port: u16) -> SockaddrIn6 {
        SockaddrIn6 {
            sin6_port: htons(port),
            ..SockaddrIn6::with_addr(addr)
        }
    }

    /// An address mbuf holding `sin6`.
    pub(crate) fn nam6(sin6: SockaddrIn6) -> &'static Mbuf {
        let m = m_get(M_DONTWAIT, MT_SONAME).expect("mbuf");
        m.m_len().set(size_of::<SockaddrIn6>() as u32);
        // SAFETY: a fresh mbuf of `MLEN` bytes.
        unsafe { mtod::<SockaddrIn6>(m).write_unaligned(sin6) };
        m
    }

    /// The packet `fd00:77::2 -> fd00:77::1` of next header `nxt` carrying `payload` (a
    /// transport header and its data), received on `ifp`; the transport checksum at
    /// `cksum_off` in `payload` is filled in unless `cksum_off` is `None`.
    pub(crate) fn packet6(
        ifp: &Ifnet,
        nxt: u8,
        payload: &[u8],
        cksum_off: Option<usize>,
    ) -> &'static Mbuf {
        let mut v = vec![0x60, 0, 0, 0];
        v.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        v.extend_from_slice(&[nxt, 64]);
        v.extend_from_slice(&a6("fd00:77::2").s6_addr);
        v.extend_from_slice(&a6("fd00:77::1").s6_addr);
        v.extend_from_slice(payload);
        if let Some(off) = cksum_off {
            let m = test_packet(&v);
            let sum = in6_cksum(m, nxt, 40, payload.len() as u32);
            m_freem(m);
            v[40 + off..42 + off].copy_from_slice(&sum.to_ne_bytes());
        }
        let m = test_packet(&v);
        m.m_pkthdr().ph_ifidx.set(u32::from(ifp.if_index.get()));
        m
    }

    #[test]
    fn inet6_bind_pick_connect_and_lookups() {
        let (_g, _t, p) = setup();
        crate::netinet::udp_usrreq::udp_init();
        let _ifp = test_if6(b"tpcb6");
        let ours = a6("fd00:77::1");

        let so = socreate(i32::from(AF_INET6), SOCK_DGRAM, 0).expect("socket");
        let inp = sotoinpcb(so).expect("attached");
        assert!(inp.has_flags(INP_IPV6), "in_pcballoc marks a PF_INET6 pcb");
        assert_eq!(inp.inp_cksum6.get(), -1);
        let t = inp.table();

        // An explicit port on our address, then the local port lookups.
        in_pcbbind(inp, Some(nam6(sin6(ours, 5000))), p).expect("bind");
        assert_eq!(inp.inp_laddr6.get(), ours);
        assert_eq!(inp.inp_lport.get(), htons(5000));
        let found = in_pcblookup_local_lock(
            t,
            &Inpaddru::from_addr6(ours),
            htons(5000),
            INPLOOKUP_IPV6,
            0,
            IN_PCBLOCK_GRAB,
        );
        assert!(found.is_some_and(|f| ptr::eq(f, inp)));
        in_pcbunref(found);
        let any = Inpaddru::from_addr6(IN6ADDR_ANY);
        let lookup = |flags| {
            let f = in_pcblookup_local_lock(t, &any, htons(5000), flags, 0, IN_PCBLOCK_GRAB);
            let hit = f.is_some_and(|f| ptr::eq(f, inp));
            in_pcbunref(f);
            hit
        };
        assert!(!lookup(INPLOOKUP_IPV6), "an exact match only");
        assert!(lookup(INPLOOKUP_IPV6 | INPLOOKUP_WILDCARD));
        assert_eq!(
            in_pcbbind(inp, Some(nam6(sin6(ours, 5001))), p),
            Err(Errno::EINVAL),
            "bound already"
        );

        // The same address and port again; an address that is not ours.
        let so2 = socreate(i32::from(AF_INET6), SOCK_DGRAM, 0).expect("socket");
        let inp2 = sotoinpcb(so2).expect("attached");
        assert_eq!(
            in_pcbbind(inp2, Some(nam6(sin6(ours, 5000))), p),
            Err(Errno::EADDRINUSE)
        );
        assert_eq!(
            in_pcbbind(inp2, Some(nam6(sin6(a6("fd00:77::9"), 5000))), p),
            Err(Errno::EADDRNOTAVAIL)
        );
        // A picked port from the default range, on the unspecified address.
        in_pcbbind(inp2, None, p).expect("anonymous bind");
        let port = i32::from(u16::from_be(inp2.inp_lport.get()));
        assert!(
            (IPPORT_FIRSTAUTO.load(Ordering::Relaxed)..=IPPORT_LASTAUTO.load(Ordering::Relaxed))
                .contains(&port),
            "port {port}"
        );
        assert_eq!(inp2.inp_laddr6.get(), IN6ADDR_ANY);

        // connect(2) of an unbound socket: in6_pcbconnect binds it to the selected source
        // and a picked port (in_pcbbind_locked with an IPv6 local address).
        let so3 = socreate(i32::from(AF_INET6), SOCK_DGRAM, 0).expect("socket");
        let inp3 = sotoinpcb(so3).expect("attached");
        let peer = a6("fd00:77::2");
        solock(so3);
        in_pcbconnect(inp3, nam6(sin6(peer, 53))).expect("connect");
        assert!(
            in_pcbrtentry(inp3).is_some(),
            "in6_pcbrtentry: the route to the peer"
        );
        sounlock(so3);
        assert_eq!(
            inp3.inp_laddr6.get(),
            ours,
            "the source in6_pcbselsrc picked"
        );
        assert_ne!(inp3.inp_lport.get(), 0);
        assert_eq!(inp3.inp_faddr6.get(), peer);
        let hit = in6_pcblookup(t, &peer, htons(53), &ours, inp3.inp_lport.get(), 0);
        assert!(hit.is_some_and(|h| ptr::eq(h, inp3)));
        in_pcbunref(hit);

        // getpeername(2) and getsockname(2) of an INP_IPV6 pcb.
        let m = m_get(M_DONTWAIT, MT_SONAME).expect("mbuf");
        in_setpeeraddr(inp3, m);
        // SAFETY: `in6_setpeeraddr` wrote a `sockaddr_in6`.
        let got = unsafe { mtod::<SockaddrIn6>(m).read_unaligned() };
        assert_eq!((got.sin6_family, got.sin6_port), (AF_INET6, htons(53)));
        assert_eq!(got.sin6_addr, peer);
        in_setsockaddr(inp3, m);
        // SAFETY: as above, `in6_setsockaddr`'s.
        let got = unsafe { mtod::<SockaddrIn6>(m).read_unaligned() };
        assert_eq!(got.sin6_addr, ours);
        m_freem(Some(m));

        // in_pcbunset_laddr forgets both IPv6 addresses.
        in_pcbunset_laddr(inp3);
        assert_eq!(
            (inp3.inp_laddr6.get(), inp3.inp_faddr6.get()),
            (IN6ADDR_ANY, IN6ADDR_ANY)
        );

        for so in [so, so2, so3] {
            soclose(so, 0).expect("close");
        }
        let _ = take_sent6();
        teardown();
    }

    #[test]
    fn kern_file_reports_an_inet6_socket() {
        use crate::kern::kern_sysctl::fill_file;
        use crate::netinet::udp_usrreq::{udp_bind, udp_init};
        use crate::sys::sysctl::KinfoFile;

        let (_g, _t, p) = setup();
        udp_init();
        let _ifp = test_if6(b"tkf6");
        let so = socreate(i32::from(AF_INET6), SOCK_DGRAM, 0).expect("socket");
        solock(so);
        udp_bind(so, nam6(sin6(a6("fd00:77::1"), 5353)), p).expect("bind");
        let mut kf = KinfoFile::zeroed();
        fill_file(&mut kf, None, None, 0, None, None, p, Some(so), false);
        sounlock(so);

        assert_eq!(kf.so_family, u32::from(AF_INET6));
        assert_eq!(kf.inp_lport, u32::from(htons(5353)));
        let ours = a6("fd00:77::1");
        for (i, w) in kf.inp_laddru.iter().enumerate() {
            assert_eq!(*w, ours.s6_addr32(i), "inp_laddru[{i}]");
        }
        assert_eq!(kf.inp_faddru, [0; 4]);
        assert_eq!(kf.inp_ppcb, 0, "no pointers without show_pointers");

        soclose(so, 0).expect("close");
        teardown();
    }
}
