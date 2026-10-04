//! Host tests for raw IPv6 sockets: what ping6 opens (a `SOCK_RAW`, `IPPROTO_ICMPV6` socket
//! with an `ICMP6_FILTER`) seeing or not seeing ICMPv6 messages through `rip6_input`, the
//! `IPV6_CHECKSUM` offset of another protocol's raw socket checked on input, the
//! statistics sysctl and the privilege `rip6_attach` requires.

use std::{assert, assert_eq, assert_ne, vec};

use super::*;
use crate::kern::kern_prot::crget;
use crate::kern::uipc_mbuf::{m_copydata, m_get};
use crate::kern::uipc_socket::{soclose, socreate};
use crate::net::if_::tests::test_packet;
use crate::netinet::icmp6::{ICMP6_ECHO_REPLY, ICMP6_ECHO_REQUEST};
use crate::netinet::in_pcb::tests::{setup, teardown};
use crate::netinet6::in6::tests::a6;
use crate::sys::mbuf::{MT_SONAME, MT_SOOPTS};
use crate::sys::protosw::{PRCO_GETOPT, PRCO_SETOPT};
use crate::sys::socket::SOCK_RAW;

/// A raw IPv6 socket of protocol `proto`, as ping6 opens one for `IPPROTO_ICMPV6`.
fn raw6_socket(proto: i32) -> &'static Socket {
    let so = socreate(i32::from(AF_INET6), SOCK_RAW, proto).expect("socket");
    // in_pcballoc sets it for PF_INET6 sockets once the INET6 part of in_pcb.c is ported.
    inpcb_of(so).set_flags(INP_IPV6);
    so
}

/// Replaces the ICMPv6 filter of `so`'s control block, as `setsockopt(ICMP6_FILTER)` does.
fn set_filter(so: &Socket, f: Icmp6Filter) {
    let filt = inpcb_of(so)
        .inp_icmp6filt
        .get()
        .expect("rip6_attach made one");
    // SAFETY: the filter is the control block's allocation, alive until `rip6_detach`;
    // nothing else uses it during the test.
    unsafe { filt.as_ptr().write(f) };
}

/// An option mbuf holding the `int` `v`.
fn intopt(v: i32) -> &'static Mbuf {
    let m = m_get(M_DONTWAIT, MT_SOOPTS).expect("mbuf");
    m.m_len().set(4);
    // SAFETY: a fresh mbuf of `MLEN` bytes.
    unsafe { mtod::<i32>(m).write_unaligned(v) };
    m
}

/// A packet from `fd00:77::2` to `fd00:77::1`: the IPv6 header (next header `nxt`) and
/// `payload`.
fn packet(nxt: u8, payload: &[u8]) -> &'static Mbuf {
    let mut v = vec![0x60, 0, 0, 0];
    v.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    v.extend_from_slice(&[nxt, 64]);
    v.extend_from_slice(&a6("fd00:77::2").s6_addr);
    v.extend_from_slice(&a6("fd00:77::1").s6_addr);
    v.extend_from_slice(payload);
    test_packet(&v)
}

/// `rip6_input` of `m` for protocol `proto`, the payload after the IPv6 header.
fn deliver(m: &'static Mbuf, proto: i32) {
    let mut mp = Some(m);
    let mut off = size_of::<Ip6Hdr>() as i32;
    assert_eq!(
        rip6_input(&mut mp, &mut off, proto, i32::from(AF_INET6), None),
        IPPROTO_DONE
    );
}

/// The datagram of the first record of `so`'s receive buffer, and the source address.
fn received(so: &Socket) -> (SockaddrIn6, std::vec::Vec<u8>) {
    let rec = so.so_rcv.sb_mb.get().expect("a record");
    assert_eq!(i32::from(rec.m_type().get()), MT_SONAME);
    // SAFETY: an `MT_SONAME` mbuf of a raw inet6 socket holds a `sockaddr_in6`.
    let from = unsafe { mtod::<SockaddrIn6>(rec).read_unaligned() };
    let data = rec.m_next().get().expect("the datagram");
    let mut got = vec![0u8; data.m_pkthdr().len.get() as usize];
    m_copydata(data, 0, &mut got);
    (from, got)
}

fn rip6stat(c: Rip6statCounters) -> u64 {
    RIP6COUNTERS[c as usize].load(Ordering::Relaxed)
}

#[test]
fn icmpv6_reaches_the_sockets_whose_filter_passes_it() {
    let (_g, _t, _p) = setup();
    rip6_init();

    // ping6's socket passes only echo replies; another one blocks just them.
    let ping = raw6_socket(IPPROTO_ICMPV6);
    let other = raw6_socket(IPPROTO_ICMPV6);
    let inp = inpcb_of(ping);
    assert_eq!(i32::from(inp.inp_ipv6.get().ip6_nxt), IPPROTO_ICMPV6);
    assert_eq!(inp.inp_cksum6.get(), -1);
    assert_eq!(ping.so_rcv.sb_hiwat.get(), RIP6_RECVSPACE);
    let mut f = Icmp6Filter::default();
    f.setblockall();
    f.setpass(ICMP6_ECHO_REPLY);
    set_filter(ping, f);
    let mut f = Icmp6Filter::default();
    f.setpassall();
    f.setblock(ICMP6_ECHO_REPLY);
    set_filter(other, f);

    // An echo reply, id 7 seq 1: to ping6 only, the IPv6 header stripped, from fd00:77::2.
    let reply = [
        ICMP6_ECHO_REPLY,
        0,
        0x12,
        0x34,
        0,
        7,
        0,
        1,
        1,
        2,
        3,
        4,
        5,
        6,
        7,
        8,
    ];
    deliver(packet(IPPROTO_ICMPV6 as u8, &reply), IPPROTO_ICMPV6);
    assert_eq!(other.so_rcv.sb_cc.get(), 0);
    let (from, got) = received(ping);
    assert_eq!(got, reply);
    assert_eq!(from.sin6_family, AF_INET6);
    assert_eq!(from.sin6_addr, a6("fd00:77::2"));
    assert_eq!(from.sin6_port, 0);

    // An echo request: to the other socket only.
    let request = [ICMP6_ECHO_REQUEST, 0, 0, 0, 0, 9, 0, 1];
    let cc = ping.so_rcv.sb_cc.get();
    deliver(packet(IPPROTO_ICMPV6 as u8, &request), IPPROTO_ICMPV6);
    assert_eq!(ping.so_rcv.sb_cc.get(), cc);
    assert_eq!(received(other).1, request);

    // A socket bound to another address does not see it; ICMPv6 is not counted as raw
    // input.
    inpcb_of(other).inp_laddr6.set(a6("fd00:77::9"));
    let before = rip6stat(Rip6statCounters::Rip6sIpackets);
    let cc = other.so_rcv.sb_cc.get();
    deliver(packet(IPPROTO_ICMPV6 as u8, &request), IPPROTO_ICMPV6);
    assert_eq!(other.so_rcv.sb_cc.get(), cc);
    assert_eq!(rip6stat(Rip6statCounters::Rip6sIpackets), before);

    soclose(ping, 0).expect("close");
    soclose(other, 0).expect("close");

    // Without a socket, ICMPv6 is dropped and not counted as delivered.
    let delivered = || IP6COUNTERS[Ip6statCounters::Ip6sDelivered as usize].load(Ordering::Relaxed);
    let d = delivered();
    deliver(packet(IPPROTO_ICMPV6 as u8, &request), IPPROTO_ICMPV6);
    assert_eq!(delivered(), d.wrapping_sub(1));
    teardown();
}

#[test]
fn ipv6_checksum_offsets_are_verified_on_input() {
    let (_g, _t, _p) = setup();
    rip6_init();

    // OSPFv3-like: protocol 89, the checksum at offset 2 of the payload.
    let so = raw6_socket(89);
    let m = intopt(2);
    rip6_ctloutput(PRCO_SETOPT, so, IPPROTO_IPV6, IPV6_CHECKSUM, Some(m)).expect("setsockopt");
    rip6_ctloutput(PRCO_GETOPT, so, IPPROTO_IPV6, IPV6_CHECKSUM, Some(m)).expect("getsockopt");
    // SAFETY: the option was written as an `int`.
    assert_eq!(unsafe { mtod::<i32>(m).read_unaligned() }, 2);
    m_freem(m);
    assert_eq!(inpcb_of(so).inp_cksum6.get(), 2);
    // Odd offsets are refused.
    let m = intopt(3);
    assert_eq!(
        rip6_ctloutput(PRCO_SETOPT, so, IPPROTO_IPV6, IPV6_CHECKSUM, Some(m)),
        Err(Errno::EINVAL)
    );
    m_freem(m);

    // A payload with a correct checksum at offset 2 is delivered.
    let payload = [3, 1, 0, 0, 0, 0, 0, 40, 1, 1, 1, 1];
    let good = packet(89, &payload);
    let sum = in6_cksum(good, 89, 40, payload.len() as u32);
    // SAFETY: the test packet is one mbuf holding the header and the payload.
    unsafe {
        mtod::<u8>(good)
            .add(40 + 2)
            .cast::<u16>()
            .write_unaligned(sum)
    };
    assert_eq!(in6_cksum(good, 89, 40, payload.len() as u32), 0);
    let mut sent = vec![0u8; payload.len()];
    m_copydata(good, 40, &mut sent);
    let (isum, bad) = (
        rip6stat(Rip6statCounters::Rip6sIsum),
        rip6stat(Rip6statCounters::Rip6sBadsum),
    );
    deliver(good, 89);
    assert_eq!(received(so).1, sent);
    assert_eq!(rip6stat(Rip6statCounters::Rip6sIsum), isum + 1);
    assert_eq!(rip6stat(Rip6statCounters::Rip6sBadsum), bad);

    // A wrong checksum, and a payload too short for the offset, are not.
    let cc = so.so_rcv.sb_cc.get();
    let mut wrong = sent.clone();
    wrong[8] ^= 0xff;
    deliver(packet(89, &wrong), 89);
    deliver(packet(89, &[3, 1, 0]), 89);
    assert_eq!(so.so_rcv.sb_cc.get(), cc);
    assert_eq!(rip6stat(Rip6statCounters::Rip6sBadsum), bad + 2);
    assert_eq!(rip6stat(Rip6statCounters::Rip6sIsum), isum + 3);

    // Without IPV6_CHECKSUM nothing is verified.
    let m = intopt(-1);
    rip6_ctloutput(PRCO_SETOPT, so, IPPROTO_IPV6, IPV6_CHECKSUM, Some(m)).expect("setsockopt");
    m_freem(m);
    deliver(packet(89, &wrong), 89);
    assert!(so.so_rcv.sb_cc.get() > cc);
    assert_eq!(rip6stat(Rip6statCounters::Rip6sIsum), isum + 3);

    soclose(so, 0).expect("close");
    teardown();
}

/// The packets the test interface was handed, as bytes.
static SENT: std::sync::Mutex<std::vec::Vec<std::vec::Vec<u8>>> =
    std::sync::Mutex::new(std::vec::Vec::new());

/// A driver `ioctl` that accepts the address and multicast requests (and brings the
/// interface up on the first address).
///
/// # Safety
///
/// `IfIoctlFn`'s contract.
unsafe fn accepting_ioctl(
    ifp: &'static crate::net::if_var::Ifnet,
    cmd: u64,
    _data: *mut u8,
) -> Result<(), Errno> {
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
    _ifp: &'static crate::net::if_var::Ifnet,
    m: &'static Mbuf,
    _dst: *const Sockaddr,
    _rt: Option<&'static crate::net::route::Rtentry>,
) -> Result<(), Errno> {
    let b = crate::netinet::ip_input::tests::bytes(m);
    SENT.lock().unwrap_or_else(|e| e.into_inner()).push(b);
    m_freem(m);
    Ok(())
}

/// An attached Ethernet-like interface with `fd00:77::1/64`, usable at once (its duplicate
/// address detection skipped), whose output the test reads.
fn test_if6() -> &'static crate::net::if_var::Ifnet {
    use crate::netinet6::in6::{in6_ioctl, in6_prefixlen2mask, in6ifa_ifpwithaddr};
    use crate::netinet6::in6_var::{IN6_IFF_TENTATIVE, In6Aliasreq, SIOCAIFADDR_IN6};
    use crate::netinet6::nd6::ND6_INFINITE_LIFETIME;

    let ifp = crate::net::if_::tests::test_ifnet(b"tp6v0");
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
    SENT.lock().unwrap_or_else(|e| e.into_inner()).clear();
    ifp
}

/// An address mbuf holding `sin6`.
fn nam6(sin6: SockaddrIn6) -> &'static Mbuf {
    let m = m_get(M_DONTWAIT, MT_SONAME).expect("mbuf");
    m.m_len().set(size_of::<SockaddrIn6>() as u32);
    // SAFETY: a fresh mbuf of `MLEN` bytes.
    unsafe { mtod::<SockaddrIn6>(m).write_unaligned(sin6) };
    m
}

/// `sendto(so, payload, dst)`, from the kernel.
fn send6(so: &'static Socket, payload: &[u8], dst: In6Addr) -> Result<(), Errno> {
    let nam = nam6(SockaddrIn6::with_addr(dst));
    crate::kern::uipc_socket2::solock(so);
    let r = rip6_send(so, Some(test_packet(payload)), Some(nam), None);
    crate::kern::uipc_socket2::sounlock(so);
    m_freem(nam);
    r
}

#[test]
fn ping6_sends_an_echo_request_with_its_checksum() {
    let (_g, _t, _p) = setup();
    rip6_init();
    let _ifp = test_if6();

    // ping6 -c 1 fd00:77::2: the payload is the ICMPv6 message, its checksum left zero.
    let so = raw6_socket(IPPROTO_ICMPV6);
    let echo = [
        ICMP6_ECHO_REQUEST,
        0,
        0,
        0,
        0x12,
        0x34,
        0,
        1,
        1,
        2,
        3,
        4,
        5,
        6,
        7,
        8,
    ];
    let hist = || {
        crate::netinet6::icmp6::ICMP6COUNTERS
            [Icmp6statCounters::Icp6sOuthist as usize + usize::from(ICMP6_ECHO_REQUEST)]
        .load(Ordering::Relaxed)
    };
    let before = hist();
    send6(so, &echo, a6("fd00:77::2")).expect("sent");
    let sent = core::mem::take(&mut *SENT.lock().unwrap_or_else(|e| e.into_inner()));
    assert_eq!(sent.len(), 1, "the echo request");
    let p = &sent[0];
    assert_eq!(p.len(), 40 + echo.len());
    assert_eq!(p[0] >> 4, 6);
    assert_eq!(u16::from_be_bytes([p[4], p[5]]) as usize, echo.len());
    assert_eq!(p[6], IPPROTO_ICMPV6 as u8);
    assert_eq!(p[7], 64, "the default hop limit");
    assert_eq!(
        &p[8..24],
        &a6("fd00:77::1").s6_addr,
        "in6_pcbselsrc chose our address"
    );
    assert_eq!(&p[24..40], &a6("fd00:77::2").s6_addr);
    assert_eq!(p[40], ICMP6_ECHO_REQUEST);
    assert_eq!(&p[44..], &echo[4..]);
    let m = test_packet(p);
    assert_ne!(&p[42..44], &[0, 0]);
    assert_eq!(in6_cksum(m, IPPROTO_ICMPV6 as u8, 40, echo.len() as u32), 0);
    m_freem(m);
    assert_eq!(hist(), before + 1);
    soclose(so, 0).expect("close");

    // Another protocol with IPV6_CHECKSUM 2: the checksum at that offset; a payload too short
    // for it is refused.
    let so = raw6_socket(89);
    let opt = intopt(2);
    rip6_ctloutput(PRCO_SETOPT, so, IPPROTO_IPV6, IPV6_CHECKSUM, Some(opt)).expect("setsockopt");
    m_freem(opt);
    let out = rip6stat(Rip6statCounters::Rip6sOpackets);
    let payload = [3, 1, 0, 0, 0, 0, 0, 40, 1, 1, 1, 1];
    send6(so, &payload, a6("fd00:77::2")).expect("sent");
    let sent = core::mem::take(&mut *SENT.lock().unwrap_or_else(|e| e.into_inner()));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0][6], 89);
    let m = test_packet(&sent[0]);
    assert_eq!(in6_cksum(m, 89, 40, payload.len() as u32), 0);
    m_freem(m);
    assert_eq!(rip6stat(Rip6statCounters::Rip6sOpackets), out + 1);
    assert_eq!(send6(so, &[3, 1, 0], a6("fd00:77::2")), Err(Errno::EINVAL));
    // IPv4-mapped destinations are refused.
    assert_eq!(
        send6(so, &payload, a6("::ffff:a00:202")),
        Err(Errno::EADDRNOTAVAIL)
    );
    assert!(SENT.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
    soclose(so, 0).expect("close");
    teardown();
}

#[test]
fn the_statistics_sysctl_and_the_privilege_to_attach() {
    let (_g, _t, p) = setup();
    rip6_init();

    let mut st = Rip6stat::default();
    let mut len = size_of::<Rip6stat>();
    rip6_sysctl(
        &[RIPV6CTL_STATS],
        ptr::from_mut(&mut st) as usize,
        &mut len,
        0,
        0,
    )
    .expect("rip6stat");
    assert_eq!(len, size_of::<Rip6stat>());
    assert_eq!(st.rip6s_opackets, rip6stat(Rip6statCounters::Rip6sOpackets));
    assert_eq!(
        rip6_sysctl(&[RIPV6CTL_STATS, 0], 0, &mut len, 0, 0),
        Err(Errno::ENOTDIR)
    );
    assert_eq!(
        rip6_sysctl(&[99], 0, &mut len, 0, 0),
        Err(Errno::EOPNOTSUPP)
    );

    // An unprivileged process may not open a raw socket.
    let cr = crget();
    cr.cr_uid.set(1000);
    p.p_ucred.set(cr);
    assert_eq!(
        socreate(i32::from(AF_INET6), SOCK_RAW, IPPROTO_ICMPV6).err(),
        Some(Errno::EACCES)
    );
    teardown();
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/netinet6/raw_ip6.h");
    let ctl = crate::reftest::assert_defines!(defs; RIPV6CTL_STATS, RIPV6CTL_MAXID);
    crate::reftest::assert_complete(&defs, "RIPV6CTL_", &ctl);
}
