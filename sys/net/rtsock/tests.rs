//! Host tests for the routing socket: the message layout `rtm_msg2` builds, the address
//! parsing and checks of `rtm_xaddrs`, the `net.route` sysctl (route dumps, interface lists
//! and names, table information, statistics) over a test Ethernet interface with an
//! address, and a routing socket end to end (`socreate`, `sosend` of `RTM_GET`, `RTM_ADD`
//! and `RTM_DELETE`, the answers read back with `soreceive`, the socket options, `soclose`).
//!
//! The tests run as a thread with root credentials made `curproc` (the requests check
//! `suser` and stamp `rtm_pid`); they clear `curproc` before they return.

use std::boxed::Box;
use std::sync::MutexGuard;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::kern_proc::procinit;
use crate::kern::kern_prot::{crget, crhold};
use crate::kern::uipc_mbuf::m_get;
use crate::kern::uipc_socket::{soclose, socreate, soinit, soreceive, sosend};
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::net::route::{RTA_BRD, RTA_DST, RTA_GATEWAY, RTA_IFA, RTA_IFP, RTA_LABEL, RTA_NETMASK};
use crate::netinet::in_::sintosa;
use crate::netinet::ip_input::tests::{configure, sin, test_ether, unconfigure};
use crate::sys::mbuf::MT_SOOPTS;
use crate::sys::proc::Process;
use crate::sys::socket::MSG_DONTWAIT;
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};

type Guards = (MutexGuard<'static, ()>, MutexGuard<'static, ()>);

/// The pid the test thread's process has.
const PID: i32 = 42;

/// The network setup (memory, mbufs, a routing table, IPv4) with an empty interface list,
/// the socket and routing control block pools, and a root thread as `curproc`.
fn setup() -> Guards {
    let g = crate::netinet::ip_input::tests::setup();
    // Interfaces and interface groups of earlier tests live in the memory just reset.
    IFNETLIST.0.init();
    crate::net::if_::IFG_HEAD.0.init();
    procinit();
    soinit();
    route_prinit();
    RTPTABLE.rtp_count.store(0, Ordering::Relaxed);

    let pr: &'static Process = Box::leak(Box::new(Process::new()));
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    p.p_p.set(pr);
    pr.ps_mainproc.set(p);
    pr.ps_pid.set(PID);
    let cr = crget();
    p.p_ucred.set(cr);
    pr.ps_ucred.set(crhold(cr));
    Machine::set_curproc(Machine::curcpu(), p);
    g
}

/// Undoes what outlives the reset memory: `curproc`.
fn teardown() {
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// The `struct rt_msghdr` at `off` in `buf`; zeros past the end of a shorter message (the
/// interface messages share only the first fields).
fn hdr_at(buf: &[u8], off: usize) -> RtMsghdr {
    let mut h = [0u8; size_of::<RtMsghdr>()];
    let n = (buf.len() - off).min(h.len());
    h[..n].copy_from_slice(&buf[off..off + n]);
    // SAFETY: a local array of the header's size; integers only, read unaligned.
    unsafe { h.as_ptr().cast::<RtMsghdr>().read_unaligned() }
}

/// The bytes of a `T` (a `#[repr(C)]` value without padding).
fn bytes_of<T>(v: &T) -> &[u8] {
    // SAFETY: the callers pass socket addresses and message headers without padding.
    unsafe { slice::from_raw_parts(ptr::from_ref(v).cast::<u8>(), size_of::<T>()) }
}

/// The IPv4 address of the `sockaddr_in` at `off` in `buf`.
fn in_at(buf: &[u8], off: usize) -> [u8; 4] {
    let a = off + offset_of!(SockaddrIn, sin_addr);
    [buf[a], buf[a + 1], buf[a + 2], buf[a + 3]]
}

/// The messages of a sysctl answer: (offset, header) of each, by `rtm_msglen`.
fn messages(buf: &[u8]) -> Vec<(usize, RtMsghdr)> {
    let mut v = Vec::new();
    let mut off = 0;
    while off < buf.len() {
        let h = hdr_at(buf, off);
        assert_eq!(h.rtm_version, RTM_VERSION);
        assert!(h.rtm_msglen as usize >= usize::from(h.rtm_hdrlen));
        v.push((off, h));
        off += usize::from(h.rtm_msglen);
    }
    assert_eq!(off, buf.len(), "messages end at the answer's end");
    v
}

/// `sysctl(name)` the way libc does it: the size first, then the answer.
fn sysctl_answer(name: &[i32]) -> Vec<u8> {
    let mut size = 0;
    sysctl_rtable(name, 0, &mut size, 0, 0).expect("size estimate");
    let mut buf = vec![0u8; size];
    let mut len = size;
    sysctl_rtable(name, buf.as_mut_ptr() as usize, &mut len, 0, 0).expect("answer");
    buf.truncate(len);
    buf
}

#[test]
fn rtm_msg2_lays_out_the_header_and_the_rounded_addresses() {
    let mut dst = sin([10, 0, 2, 0]);
    let mut mask = sin([255, 255, 255, 0]);
    mask.sin_len = 7; // trimmed, as rt_plen2mask leaves it
    let mut info = RtAddrinfo::new();
    info.rti_info[RTAX_DST] = sintosa(&mut dst);
    info.rti_info[RTAX_NETMASK] = sintosa(&mut mask);

    // SAFETY: local addresses.
    let len = unsafe { rtm_msg2(RTM_GET, RTM_VERSION, &mut info, None, None) };
    assert_eq!(len, size_of::<RtMsghdr>() + 16 + 8);
    assert_eq!(info.rti_addrs, RTA_DST | RTA_NETMASK);

    let mut buf = vec![0xffu8; len];
    // SAFETY: local addresses; the buffer holds `len` bytes.
    let again = unsafe { rtm_msg2(RTM_GET, RTM_VERSION, &mut info, Some(&mut buf), None) };
    assert_eq!(again, len);
    let h = hdr_at(&buf, 0);
    assert_eq!(usize::from(h.rtm_msglen), len);
    assert_eq!(h.rtm_version, RTM_VERSION);
    assert_eq!(h.rtm_type, RTM_GET);
    assert_eq!(usize::from(h.rtm_hdrlen), size_of::<RtMsghdr>());
    // Without a walk the rest of the caller's header is left alone.
    assert_eq!(buf[offset_of!(RtMsghdr, rtm_index)], 0xff);
    let a = size_of::<RtMsghdr>();
    assert_eq!(&buf[a..a + 16], bytes_of(&dst));
    assert_eq!(&buf[a + 16..a + 23], &bytes_of(&mask)[..7]);
    assert_eq!(buf[a + 23], 0, "rounded up with zeros");

    // The interface messages have their own headers.
    let mut info = RtAddrinfo::new();
    // SAFETY: no addresses.
    assert_eq!(
        unsafe { rtm_msg2(RTM_IFINFO, RTM_VERSION, &mut info, None, None) },
        size_of::<IfMsghdr>()
    );
    // SAFETY: no addresses.
    assert_eq!(
        unsafe { rtm_msg2(RTM_NEWADDR, RTM_VERSION, &mut info, None, None) },
        size_of::<IfaMsghdr>()
    );
    assert_eq!(info.rti_addrs, 0);
}

#[test]
fn rtm_xaddrs_splits_and_checks_the_addresses() {
    let dst = sin([10, 0, 2, 0]);
    let gw = sin([10, 0, 2, 2]);
    let mut label = SockaddrRtlabel {
        sr_len: size_of::<SockaddrRtlabel>() as u8,
        sr_family: AF_UNSPEC,
        sr_label: [0; RTLABEL_LEN],
    };
    label.sr_label[..4].copy_from_slice(b"blue");
    let mut buf = [0u64; 16];
    // SAFETY: a local buffer of 128 bytes, viewed as bytes.
    let bytes = unsafe { slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), 128) };
    bytes[..16].copy_from_slice(bytes_of(&dst));
    bytes[16..32].copy_from_slice(bytes_of(&gw));
    bytes[32..32 + size_of::<SockaddrRtlabel>()].copy_from_slice(bytes_of(&label));
    let end = 32 + roundup_long(size_of::<SockaddrRtlabel>());
    let base = bytes.as_ptr();

    let parse = |addrs: i32, len: usize| {
        let mut info = RtAddrinfo::new();
        info.rti_addrs = addrs;
        // SAFETY: within the local buffer.
        let r = unsafe { rtm_xaddrs(base, base.add(len), &mut info) };
        (r, info)
    };

    let (r, info) = parse(RTA_DST | RTA_GATEWAY | RTA_LABEL, end);
    assert_eq!(r, Ok(()));
    assert_eq!(info.rti_info[RTAX_DST], base.cast());
    // SAFETY: within the local buffer.
    assert_eq!(info.rti_info[RTAX_GATEWAY], unsafe { base.add(16) }.cast());
    // SAFETY: within the local buffer.
    assert_eq!(info.rti_info[RTAX_LABEL], unsafe { base.add(32) }.cast());
    assert!(info.rti_info[RTAX_NETMASK].is_null());

    // An address past the end, a bit past RTAX_MAX.
    assert_eq!(parse(RTA_DST | RTA_GATEWAY, 24).0, Err(Errno::EINVAL));
    assert_eq!(parse(RTA_DST | (1 << 20), end).0, Err(Errno::EINVAL));
    // A sockaddr_in as the interface (which must be AF_LINK) or as a label.
    assert_eq!(parse(RTA_DST | RTA_IFP, end).0, Err(Errno::EAFNOSUPPORT));
    assert_eq!(parse(RTA_LABEL, end).0, Err(Errno::EAFNOSUPPORT));

    // A short sockaddr_in as the destination.
    bytes[0] = 8;
    assert_eq!(parse(RTA_DST, end).0, Err(Errno::EINVAL));
    bytes[0] = 16;

    // A label without its NUL.
    bytes[34..34 + RTLABEL_LEN].fill(b'x');
    assert_eq!(
        parse(RTA_DST | RTA_GATEWAY | RTA_LABEL, end).0,
        Err(Errno::EINVAL)
    );
}

#[test]
fn sysctl_dumps_the_routes_of_an_interface() {
    let _g = setup();
    let ifp = test_ether();
    configure(ifp, [10, 0, 2, 15], [255, 255, 255, 0]);

    // The size estimate: what is needed plus a tenth (at least 1024), in pages.
    let mut size = 0;
    sysctl_rtable(&[0, NET_RT_DUMP, 0, 0], 0, &mut size, 0, 0).expect("estimate");
    assert!(size > 0 && size % PAGE_SIZE == 0);

    let buf = sysctl_answer(&[0, NET_RT_DUMP, 0, 0]);
    let msgs = messages(&buf);
    let mut seen = Vec::new();
    for &(off, h) in &msgs {
        assert_eq!(h.rtm_type, RTM_GET);
        assert_eq!(usize::from(h.rtm_hdrlen), size_of::<RtMsghdr>());
        assert_ne!(h.rtm_flags as u32 & RTF_DONE, 0);
        assert_eq!(h.rtm_pid, PID);
        assert_eq!(u32::from(h.rtm_index), ifp.if_index.get());
        assert_eq!(h.rtm_tableid, 0);
        let want = RTA_DST | RTA_GATEWAY | RTA_NETMASK | RTA_IFP | RTA_IFA;
        assert_eq!(h.rtm_addrs & want, want, "addresses of {h:?}");
        seen.push((
            in_at(&buf, off + usize::from(h.rtm_hdrlen)),
            h.rtm_flags as u32,
        ));
    }
    let has = |a: [u8; 4], flag: u32| seen.iter().any(|&(d, f)| d == a && f & flag != 0);
    assert!(has([10, 0, 2, 15], RTF_LOCAL), "local route: {seen:?}");
    assert!(has([10, 0, 2, 0], RTF_CLONING), "prefix route: {seen:?}");
    assert!(
        has([10, 0, 2, 255], RTF_BROADCAST),
        "broadcast route: {seen:?}"
    );

    // NET_RT_FLAGS keeps the routes with one of the flags.
    let local = sysctl_answer(&[0, NET_RT_FLAGS, RTF_LOCAL as i32, 0]);
    let local = messages(&local);
    assert_eq!(local.len(), 1);
    // A priority filter: none of these is static.
    let stat = sysctl_answer(&[0, NET_RT_DUMP, i32::from(crate::net::route::RTP_STATIC), 0]);
    assert!(stat.is_empty());

    // A buffer too small for the first message: nothing copied, ENOMEM.
    let mut small = [0u8; 50];
    let mut len = small.len();
    assert_eq!(
        sysctl_rtable(
            &[0, NET_RT_DUMP, 0, 0],
            small.as_mut_ptr() as usize,
            &mut len,
            0,
            0
        ),
        Err(Errno::ENOMEM)
    );
    assert_eq!(len, 0);

    // The argument checks.
    let mut len = 0;
    assert_eq!(
        sysctl_rtable(&[0, NET_RT_DUMP, 0, 0], 0, &mut len, 8, 4),
        Err(Errno::EPERM)
    );
    assert_eq!(
        sysctl_rtable(&[0, NET_RT_DUMP], 0, &mut len, 0, 0),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        sysctl_rtable(&[0, NET_RT_DUMP, 0, 9], 0, &mut len, 0, 0),
        Err(Errno::ENOENT)
    );
    assert_eq!(
        sysctl_rtable(&[0, 99, 0, 0], 0, &mut len, 0, 0),
        Err(Errno::EINVAL)
    );

    unconfigure(ifp, [10, 0, 2, 15]);
    teardown();
}

#[test]
fn sysctl_lists_interfaces_their_addresses_and_names() {
    let _g = setup();
    let ifp = test_ether();
    configure(ifp, [10, 0, 2, 15], [255, 255, 255, 0]);
    let index = ifp.if_index.get() as i32;

    // NET_RT_IFLIST: the interface, then its address.
    let buf = sysctl_answer(&[0, NET_RT_IFLIST, index]);
    let msgs = messages(&buf);
    assert_eq!(msgs.len(), 2, "{msgs:?}");
    let (off, h) = msgs[0];
    assert_eq!(h.rtm_type, RTM_IFINFO);
    assert_eq!(usize::from(h.rtm_hdrlen), size_of::<IfMsghdr>());
    // SAFETY: an if_msghdr at the start of the answer; integers only.
    let ifm = unsafe { buf.as_ptr().add(off).cast::<IfMsghdr>().read_unaligned() };
    assert_eq!(i32::from(ifm.ifm_index), index);
    assert_eq!(ifm.ifm_addrs, RTA_IFP);
    assert_eq!(ifm.ifm_flags, ifp.if_flags.get());
    assert_eq!(ifm.ifm_data.ifi_mtu, ifp.if_mtu.get());
    let sdl = off + size_of::<IfMsghdr>();
    assert_eq!(buf[sdl + 1], AF_LINK);
    let nlen = usize::from(buf[sdl + offset_of!(crate::net::if_dl::SockaddrDl, sdl_nlen)]);
    let name = sdl + offset_of!(crate::net::if_dl::SockaddrDl, sdl_data);
    assert_eq!(&buf[name..name + nlen], b"tvio0");

    let (off, h) = msgs[1];
    assert_eq!(h.rtm_type, RTM_NEWADDR);
    assert_eq!(usize::from(h.rtm_hdrlen), size_of::<IfaMsghdr>());
    // SAFETY: an ifa_msghdr; integers only.
    let ifam = unsafe { buf.as_ptr().add(off).cast::<IfaMsghdr>().read_unaligned() };
    assert_eq!(i32::from(ifam.ifam_index), index);
    assert_eq!(ifam.ifam_addrs, RTA_IFA | RTA_NETMASK | RTA_BRD);
    // RTAX_NETMASK comes first, then RTAX_IFA.
    let mask_len = usize::from(buf[off + size_of::<IfaMsghdr>()]);
    let ifa = off + size_of::<IfaMsghdr>() + roundup_long(mask_len);
    assert_eq!(buf[ifa + 1], AF_INET);
    assert_eq!(in_at(&buf, ifa), [10, 0, 2, 15]);

    // Only the link message for a family without addresses.
    let link = sysctl_answer(&[
        i32::from(crate::sys::socket::AF_INET6),
        NET_RT_IFLIST,
        index,
    ]);
    assert_eq!(messages(&link).len(), 1);

    // NET_RT_IFNAMES: the index and the name.
    let names = sysctl_answer(&[0, NET_RT_IFNAMES, index]);
    assert_eq!(names.len(), size_of::<IfNameindexMsg>());
    assert_eq!(names[..4], (index as u32).to_ne_bytes());
    assert_eq!(&names[4..10], b"tvio0\0");

    // NET_RT_TABLE: table 0 in routing domain 0; table 9 does not exist.
    let mut info = [0xffu8; 4];
    let mut len = info.len();
    sysctl_rtable(
        &[0, NET_RT_TABLE, 0],
        info.as_mut_ptr() as usize,
        &mut len,
        0,
        0,
    )
    .expect("table 0");
    assert_eq!((len, info), (4, [0; 4]));
    assert_eq!(
        sysctl_rtable(&[0, NET_RT_TABLE, 9], 0, &mut len, 0, 0),
        Err(Errno::ENOENT)
    );

    // NET_RT_STATS: struct rtstat, one word per counter.
    RTCOUNTERS[RtstatCounters::RtsUnreach as usize].store(3, Ordering::Relaxed);
    let mut stats = [0u32; RtstatCounters::RtsNcounters as usize];
    let mut len = size_of::<crate::net::route::Rtstat>();
    sysctl_rtable(
        &[0, NET_RT_STATS, 0],
        stats.as_mut_ptr() as usize,
        &mut len,
        0,
        0,
    )
    .expect("stats");
    assert_eq!(stats[RtstatCounters::RtsUnreach as usize], 3);
    RTCOUNTERS[RtstatCounters::RtsUnreach as usize].store(0, Ordering::Relaxed);

    // NET_RT_SOURCE: no preferred source is set.
    let mut len = 0;
    sysctl_rtable(&[0, NET_RT_SOURCE, 0], 0, &mut len, 0, 0).expect("source");
    assert_eq!(len, 0);

    unconfigure(ifp, [10, 0, 2, 15]);
    teardown();
}

/// `sosend` of `bytes` from kernel space.
fn send(so: &'static Socket, bytes: &[u8]) -> Result<(), Errno> {
    let mut iov = [Iovec {
        iov_base: bytes.as_ptr().cast_mut().cast(),
        iov_len: bytes.len(),
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: bytes.len(),
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: None,
    };
    sosend(so, None, Some(&mut uio), None, None, 0)
}

/// One message read without waiting, `EWOULDBLOCK` when there is none.
fn recv(so: &'static Socket) -> Result<Vec<u8>, Errno> {
    let mut buf = vec![0u8; 1024];
    let len = buf.len();
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: len,
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: len,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    let mut flags = MSG_DONTWAIT;
    soreceive(so, None, &mut uio, None, None, Some(&mut flags), 0)?;
    let n = len - uio.uio_resid;
    buf.truncate(n);
    Ok(buf)
}

/// A request: a `struct rt_msghdr` of `type_` and `seq` with `addrs` after it.
fn request(type_: u8, seq: i32, flags: u32, addrs: &[(i32, SockaddrIn)]) -> Vec<u8> {
    let mut rtm = RtMsghdr {
        rtm_version: RTM_VERSION,
        rtm_type: type_,
        rtm_hdrlen: size_of::<RtMsghdr>() as u16,
        rtm_seq: seq,
        rtm_flags: flags as i32,
        ..RtMsghdr::default()
    };
    let mut v = bytes_of(&rtm).to_vec();
    for (bit, sa) in addrs {
        rtm.rtm_addrs |= bit;
        v.extend_from_slice(bytes_of(sa));
    }
    rtm.rtm_msglen = v.len() as u16;
    v[..size_of::<RtMsghdr>()].copy_from_slice(bytes_of(&rtm));
    v
}

/// The answer to request `seq`, skipping the kernel's own announcements.
fn answer(so: &'static Socket, seq: i32) -> (Vec<u8>, RtMsghdr) {
    loop {
        let m = recv(so).expect("an answer");
        let h = hdr_at(&m, 0);
        if h.rtm_seq == seq && h.rtm_pid == PID {
            return (m, h);
        }
    }
}

/// An option mbuf holding `v`.
fn opt(v: u32) -> &'static Mbuf {
    let m = m_get(M_DONTWAIT, MT_SOOPTS).expect("mbuf");
    m.m_len().set(4);
    // SAFETY: a fresh mbuf holds MLEN bytes.
    unsafe { mtod::<u32>(m).write_unaligned(v) };
    m
}

#[test]
fn a_routing_socket_gets_adds_and_deletes_routes() {
    let _g = setup();
    let ifp = test_ether();
    configure(ifp, [10, 0, 2, 15], [255, 255, 255, 0]);

    let so = socreate(i32::from(PF_ROUTE), SOCK_RAW, 0).expect("socket(AF_ROUTE)");
    assert_eq!(RTPTABLE.rtp_count.load(Ordering::Relaxed), 1);
    assert!(so.has_state(SS_ISCONNECTED) && so.has_options(SO_USELOOPBACK));

    // RTM_GET of an address of the subnet: the /24, with its netmask.
    send(
        so,
        &request(RTM_GET, 1, 0, &[(RTA_DST, sin([10, 0, 2, 77]))]),
    )
    .expect("RTM_GET");
    let (m, h) = answer(so, 1);
    assert_eq!(h.rtm_type, RTM_GET);
    assert_eq!(h.rtm_errno, 0);
    assert_ne!(h.rtm_flags as u32 & RTF_DONE, 0);
    assert_eq!(u32::from(h.rtm_index), ifp.if_index.get());
    assert_ne!(h.rtm_addrs & RTA_NETMASK, 0);
    assert_eq!(h.rtm_addrs & RTA_LABEL, 0);
    assert_eq!(in_at(&m, size_of::<RtMsghdr>()), [10, 0, 2, 0]);

    // RTM_ADD of a default route through 10.0.2.2.
    let default = [
        (RTA_DST, sin([0, 0, 0, 0])),
        (RTA_GATEWAY, sin([10, 0, 2, 2])),
        (RTA_NETMASK, sin([0, 0, 0, 0])),
    ];
    let flags = crate::net::route::RTF_UP | RTF_GATEWAY | RTF_STATIC;
    send(so, &request(RTM_ADD, 2, flags, &default)).expect("RTM_ADD");
    let (_, h) = answer(so, 2);
    assert_eq!((h.rtm_type, h.rtm_errno), (RTM_ADD, 0));
    assert_ne!(h.rtm_flags as u32 & RTF_GATEWAY, 0);

    // An address off the subnet now goes through it.
    send(so, &request(RTM_GET, 3, 0, &[(RTA_DST, sin([8, 8, 8, 8]))])).expect("RTM_GET");
    let (m, h) = answer(so, 3);
    assert_eq!(in_at(&m, size_of::<RtMsghdr>()), [0, 0, 0, 0]);
    assert_eq!(
        in_at(&m, size_of::<RtMsghdr>() + 16),
        [10, 0, 2, 2],
        "gateway"
    );
    assert_eq!(h.rtm_addrs & RTA_GATEWAY, RTA_GATEWAY);
    // The sysctl dump shows it.
    let dump = sysctl_answer(&[0, NET_RT_DUMP, 0, 0]);
    assert!(
        messages(&dump)
            .iter()
            .any(
                |&(off, h)| in_at(&dump, off + usize::from(h.rtm_hdrlen)) == [0; 4]
                    && h.rtm_flags as u32 & RTF_STATIC != 0
            )
    );

    // RTM_DELETE, then RTM_GET fails with ESRCH, which the answer carries too.
    send(so, &request(RTM_DELETE, 4, 0, &default)).expect("RTM_DELETE");
    let (_, h) = answer(so, 4);
    assert_eq!((h.rtm_type, h.rtm_errno), (RTM_DELETE, 0));
    // As in C, deleting the default route leaves the egress group until the next rebuild;
    // rebuild it now (empty) so no group of this test outlives its memory.
    net_lock();
    crate::net::if_::if_group_egress_build().expect("egress");
    net_unlock();
    assert!(
        crate::net::if_::IFG_HEAD
            .0
            .iter()
            .all(|g| !g.ifg_group.starts_with(b"egress"))
    );
    let get = request(RTM_GET, 5, 0, &[(RTA_DST, sin([8, 8, 8, 8]))]);
    assert_eq!(send(so, &get), Err(Errno::ESRCH));
    let (_, h) = answer(so, 5);
    assert_eq!(h.rtm_errno, Errno::ESRCH as i32);
    assert_eq!(h.rtm_flags as u32 & RTF_DONE, 0);

    // Malformed requests.
    let mut bad = get.clone();
    bad[offset_of!(RtMsghdr, rtm_version)] = 4;
    assert_eq!(send(so, &bad), Err(Errno::EPROTONOSUPPORT));
    let mut bad = get.clone();
    bad[offset_of!(RtMsghdr, rtm_type)] = RTM_IFINFO;
    assert_eq!(send(so, &bad), Err(Errno::EOPNOTSUPP));
    let mut bad = get.clone();
    bad[0] += 1;
    assert_eq!(
        send(so, &bad),
        Err(Errno::EINVAL),
        "rtm_msglen is not the length"
    );
    let local = request(RTM_ADD, 6, RTF_LOCAL, &default);
    assert_eq!(send(so, &local), Err(Errno::EINVAL), "a kernel-only flag");
    assert_eq!(
        recv(so),
        Err(Errno::EWOULDBLOCK),
        "no answers to malformed requests"
    );

    // The options: a message filter that only lets RTM_ADD through.
    let level = i32::from(AF_ROUTE);
    route_ctloutput(
        PRCO_SETOPT,
        so,
        level,
        ROUTE_MSGFILTER,
        Some(opt(1 << RTM_ADD)),
    )
    .expect("ROUTE_MSGFILTER");
    let m = opt(0);
    route_ctloutput(PRCO_GETOPT, so, level, ROUTE_MSGFILTER, Some(m)).expect("get");
    // SAFETY: the option mbuf holds four bytes.
    assert_eq!(unsafe { mtod::<u32>(m).read_unaligned() }, 1 << RTM_ADD);
    m_freem(m);
    send(
        so,
        &request(RTM_GET, 7, 0, &[(RTA_DST, sin([10, 0, 2, 77]))]),
    )
    .expect("RTM_GET");
    assert_eq!(recv(so), Err(Errno::EWOULDBLOCK), "filtered out");
    assert_eq!(
        route_ctloutput(PRCO_SETOPT, so, level, ROUTE_TABLEFILTER, Some(opt(9))),
        Err(Errno::ENOENT)
    );
    assert_eq!(
        route_ctloutput(PRCO_SETOPT, so, level, ROUTE_PRIOFILTER, Some(opt(64))),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        route_ctloutput(PRCO_SETOPT, so, level, ROUTE_FLAGFILTER, None),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        route_ctloutput(PRCO_SETOPT, so, level, 99, Some(opt(0))),
        Err(Errno::ENOPROTOOPT)
    );
    assert_eq!(
        route_ctloutput(PRCO_SETOPT, so, 0, ROUTE_MSGFILTER, None),
        Err(Errno::EINVAL)
    );

    soclose(so, 0).expect("close");
    assert_eq!(RTPTABLE.rtp_count.load(Ordering::Relaxed), 0);

    unconfigure(ifp, [10, 0, 2, 15]);
    teardown();
}
