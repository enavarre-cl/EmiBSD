//! Host tests for `wg(4)`: the ioctl numbers and layouts, the allowed-IPs table, `SIOCSWG`
//! and `SIOCGWG` through the host's `copyin`/`copyout`, the dispatch of `wg_ioctl`, and two
//! interfaces exchanging a handshake initiation (`wg_input`, the handshake worker, the MAC
//! check and the peer lookup) and data both ways (`wg_output`, `wg_qstart`, `wg_encap`,
//! `wg_input`, `wg_decap` with the allowed-IPs source check).
//!
//! The task queues have no threads on the host and `taskq_barrier` would wait for one
//! forever, so the tests run the workers by hand and never destroy an interface that has
//! peers (`wg_peer_destroy` barriers): the interfaces live in the memory each test resets.
//! They run as a root thread made `curproc`, with the uptime past the 20 ms in which Noise
//! refuses initiations.

use std::boxed::Box;
use std::sync::MutexGuard;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::kern_proc::procinit;
use crate::kern::kern_prot::{crget, crhold};
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::net::if_::{IFG_HEAD, IFNETLIST, if_unit};
use crate::net::wg_noise::tests::{advance_uptime, hex, uptime_past_reject_interval};
use crate::sys::mbuf::M_DONTWAIT;
use crate::sys::proc::{Proc, Process};

type Guards = (
    MutexGuard<'static, ()>,
    MutexGuard<'static, ()>,
    MutexGuard<'static, ()>,
);

/// RFC 7748, 6.1: Alice's private key and its public key.
const ALICE_PRIVATE: &str = "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a";
const ALICE_PUBLIC: &str = "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a";
/// RFC 7748, 6.1: Bob's.
const BOB_PRIVATE: &str = "5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb";
const BOB_PUBLIC: &str = "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f";

fn key(s: &str) -> [u8; WG_KEY_LEN] {
    hex(s).try_into().expect("32 bytes")
}

/// The network setup (memory, mbufs, a routing table, IPv4) with an empty interface list, the
/// pools `wgattach` makes, no `wg` interface yet, and a root thread as `curproc`.
fn setup() -> Guards {
    let tc = uptime_past_reject_interval();
    let (m, t) = crate::netinet::ip_input::tests::setup();
    IFNETLIST.0.init();
    IFG_HEAD.0.init();
    procinit();

    pool_init(
        &WG_AIP_POOL,
        size_of::<WgAip>(),
        0,
        IPL_NET,
        0,
        "wgaip",
        None,
    );
    pool_init(
        &WG_PEER_POOL,
        size_of::<WgPeer>(),
        0,
        IPL_NET,
        0,
        "wgpeer",
        None,
    );
    pool_init(
        &WG_RATELIMIT_POOL,
        size_of::<RatelimitEntry>(),
        0,
        IPL_NET,
        0,
        "wgratelimit",
        None,
    );
    WG_COUNTER.store(0, Ordering::Relaxed);
    WG_HANDSHAKE_TASKQ.store(ptr::null_mut(), Ordering::Relaxed);
    WG_CRYPT_TASKQ.store(ptr::null_mut(), Ordering::Relaxed);

    let pr: &'static Process = Box::leak(Box::new(Process::new()));
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    p.p_p.set(pr);
    pr.ps_mainproc.set(p);
    let cr = crget();
    p.p_ucred.set(cr);
    pr.ps_ucred.set(crhold(cr));
    Machine::set_curproc(Machine::curcpu(), p);
    (tc, m, t)
}

/// Undoes what outlives the reset memory: `curproc`.
fn teardown() {
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// `ifconfig wg<unit> create`: the new interface's softc.
fn create(unit: i32) -> &'static WgSoftc {
    wg_clone_create(&WG_CLONER, unit).expect("wg_clone_create");
    let mut name = [0u8; IFNAMSIZ];
    let _ = snprintf(&mut name, format_args!("wg{unit}"));
    let ifp = if_unit(cstr(&name)).expect("the interface is attached");
    WgSoftc::of_ifp(ifp)
}

/// An IPv4 allowed IP.
fn aip(a: [u8; 4], cidr: i32) -> WgAipIo {
    let mut d = WgAipIo {
        a_af: AF_INET,
        a_cidr: cidr,
        ..WgAipIo::default()
    };
    d.a_addr.addr_bytes[..4].copy_from_slice(&a);
    d
}

/// A `sockaddr_in` for `a`:`port`.
fn sin(a: [u8; 4], port: u16) -> SockaddrIn {
    SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        sin_port: htons(port),
        sin_addr: InAddr {
            s_addr: u32::from_ne_bytes(a),
        },
        ..SockaddrIn::default()
    }
}

fn lookup(sc: &WgSoftc, a: [u8; 4]) -> Option<&'static WgPeer> {
    wg_aip_lookup(sc.aip4(), &a)
}

fn same(a: Option<&WgPeer>, b: &WgPeer) -> bool {
    a.is_some_and(|a| ptr::eq(a, b))
}

#[test]
fn ioctl_numbers_and_layouts() {
    // _IOWR('i', 210, struct wg_data_io): IOC_INOUT, 32 bytes.
    assert_eq!(SIOCSWG, 0xc020_69d2);
    assert_eq!(SIOCGWG, 0xc020_69d3);
    assert_eq!(WG_PKT_INITIATION.to_le_bytes(), [1, 0, 0, 0]);
    assert_eq!(WG_PKT_DATA.to_le_bytes(), [4, 0, 0, 0]);
    assert_eq!(wg_pkt_with_padding(0), 0);
    assert_eq!(wg_pkt_with_padding(1), 16);
    assert_eq!(wg_pkt_with_padding(1420), 1424);
    assert_eq!(MAX_TIMER_HANDSHAKES, 18);
    let mut s = std::string::String::new();
    use core::fmt::Write;
    let _ = write!(s, "{}", SaNtop::of(&sin([192, 168, 77, 2], 51820)));
    assert_eq!(s, "192.168.77.2");
}

#[test]
fn allowed_ips_route_to_their_peer() {
    let _g = setup();
    let sc = create(0);

    rw_enter_write(&sc.sc_lock);
    let pa = wg_peer_create(sc, &[1; WG_KEY_SIZE]).expect("peer a");
    let pb = wg_peer_create(sc, &[2; WG_KEY_SIZE]).expect("peer b");
    rw_exit_write(&sc.sc_lock);
    assert_eq!(sc.sc_peer_num.get(), 2);
    assert!(same(wg_peer_lookup(sc, &[1; WG_KEY_SIZE]), pa));
    assert!(same(wg_peer_lookup(sc, &[2; WG_KEY_SIZE]), pb));
    assert!(wg_peer_lookup(sc, &[3; WG_KEY_SIZE]).is_none());

    wg_aip_add(sc, pa, &aip([10, 77, 0, 2], 32)).expect("add");
    wg_aip_add(sc, pa, &aip([10, 0, 0, 0], 8)).expect("add");
    wg_aip_add(sc, pb, &aip([10, 77, 0, 0], 24)).expect("add");
    wg_aip_add(sc, pb, &aip([192, 168, 0, 0], 16)).expect("add");
    assert_eq!(sc.sc_aip_num.get(), 4);

    // The longest prefix wins.
    assert!(same(lookup(sc, [10, 77, 0, 2]), pa));
    assert!(same(lookup(sc, [10, 77, 0, 3]), pb));
    assert!(same(lookup(sc, [10, 1, 2, 3]), pa));
    assert!(same(lookup(sc, [192, 168, 5, 5]), pb));
    assert!(lookup(sc, [172, 16, 0, 1]).is_none());

    // Adding a prefix another peer has moves it.
    wg_aip_add(sc, pb, &aip([10, 0, 0, 0], 8)).expect("move");
    assert_eq!(sc.sc_aip_num.get(), 4);
    assert!(same(lookup(sc, [10, 1, 2, 3]), pb));
    assert_eq!(pa.p_aip.iter().count(), 1);
    assert_eq!(pb.p_aip.iter().count(), 3);

    // Removal: only the owner, only what exists.
    assert_eq!(
        wg_aip_remove(sc, pa, &aip([10, 0, 0, 0], 8)),
        Err(Errno::EXDEV)
    );
    assert_eq!(
        wg_aip_remove(sc, pb, &aip([172, 16, 0, 0], 12)),
        Err(Errno::ENOENT)
    );
    assert_eq!(wg_aip_remove(sc, pb, &aip([10, 0, 0, 0], 8)), Ok(()));
    assert!(lookup(sc, [10, 1, 2, 3]).is_none());
    assert_eq!(sc.sc_aip_num.get(), 3);

    // No IPv6 without INET6; prefix lengths beyond the address are refused.
    let mut v6 = aip([0x20, 0x01, 0x0d, 0xb8], 32);
    v6.a_af = AF_INET6;
    assert_eq!(wg_aip_add(sc, pa, &v6), Err(Errno::EAFNOSUPPORT));
    assert_eq!(
        wg_aip_add(sc, pa, &aip([10, 0, 0, 0], 33)),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        wg_aip_add(sc, pa, &aip([10, 0, 0, 0], -1)),
        Err(Errno::EINVAL)
    );
    teardown();
}

/// A user buffer holding `T`s and bytes: 8-aligned, as `malloc(3)` gives.
struct UserBuf(Vec<u64>);

impl UserBuf {
    fn new(len: usize) -> Self {
        Self(vec![0u64; len.div_ceil(8)])
    }

    fn addr(&self) -> usize {
        self.0.as_ptr() as usize
    }

    fn put<T: AbiPod>(&mut self, off: usize, v: &T) {
        assert!(off + size_of::<T>() <= self.0.len() * 8);
        // SAFETY: in bounds (checked); `T` is plain data, written unaligned.
        unsafe {
            self.0
                .as_mut_ptr()
                .cast::<u8>()
                .add(off)
                .cast::<T>()
                .write_unaligned(*v)
        };
    }

    fn get<T: AbiPod>(&self, off: usize) -> T {
        assert!(off + size_of::<T>() <= self.0.len() * 8);
        // SAFETY: in bounds (checked); any bytes are a `T` (`AbiPod`).
        unsafe {
            self.0
                .as_ptr()
                .cast::<u8>()
                .add(off)
                .cast::<T>()
                .read_unaligned()
        }
    }
}

/// `ifconfig wg<n> wgkey <private> wgport <port> wgpeer <peer> wgendpoint ... wgaip ...`.
fn configure(
    sc: &'static WgSoftc,
    private: &[u8; WG_KEY_LEN],
    port: u16,
    peer: &[u8; WG_KEY_LEN],
    endpoint: SockaddrIn,
    aips: &[WgAipIo],
) {
    let len =
        size_of::<WgInterfaceIo>() + size_of::<WgPeerIo>() + aips.len() * size_of::<WgAipIo>();
    let mut buf = UserBuf::new(len);
    let iface = WgInterfaceIo {
        i_flags: WG_INTERFACE_HAS_PRIVATE | WG_INTERFACE_HAS_PORT,
        i_port: port,
        i_private: *private,
        i_peers_count: 1,
        ..WgInterfaceIo::default()
    };
    buf.put(0, &iface);
    let mut p = WgPeerIo {
        p_flags: WG_PEER_HAS_PUBLIC
            | WG_PEER_HAS_PSK
            | WG_PEER_HAS_PKA
            | WG_PEER_HAS_ENDPOINT
            | WG_PEER_REPLACE_AIPS
            | WG_PEER_SET_DESCRIPTION,
        p_public: *peer,
        p_psk: [7; WG_KEY_LEN],
        p_pka: 25,
        p_aips_count: aips.len(),
        ..WgPeerIo::default()
    };
    p.p_endpoint.set_sa_sin(&endpoint);
    p.p_description[..4].copy_from_slice(b"peer");
    buf.put(size_of::<WgInterfaceIo>(), &p);
    for (i, a) in aips.iter().enumerate() {
        buf.put(
            size_of::<WgInterfaceIo>() + size_of::<WgPeerIo>() + i * size_of::<WgAipIo>(),
            a,
        );
    }
    let mut data = WgDataIo {
        wgd_size: len,
        wgd_interface: buf.addr(),
        ..WgDataIo::default()
    };
    wg_ioctl_set(sc, &mut data).expect("SIOCSWG");
}

#[test]
fn siocswg_and_siocgwg_round_trip() {
    let _g = setup();
    let sc = create(0);
    let alice = key(ALICE_PRIVATE);
    let bob = key(BOB_PUBLIC);
    let endpoint = sin([192, 168, 77, 2], 51820);

    configure(
        sc,
        &alice,
        51820,
        &bob,
        endpoint,
        &[aip([10, 77, 0, 2], 32), aip([10, 88, 0, 0], 16)],
    );
    assert_eq!(sc.sc_udp_port.get(), htons(51820));
    assert_eq!(sc.sc_peer_num.get(), 1);
    assert_eq!(sc.sc_aip_num.get(), 2);

    // The first SIOCGWG asks for the size (ifconfig's wg_status passes 0 and NULL).
    let mut data = WgDataIo::default();
    wg_ioctl_get(sc, &mut data).expect("SIOCGWG size");
    let want = size_of::<WgInterfaceIo>() + size_of::<WgPeerIo>() + 2 * size_of::<WgAipIo>();
    assert_eq!(data.wgd_size, want);

    let buf = UserBuf::new(want);
    data.wgd_interface = buf.addr();
    wg_ioctl_get(sc, &mut data).expect("SIOCGWG");
    let iface: WgInterfaceIo = buf.get(0);
    assert_eq!(
        iface.i_flags,
        WG_INTERFACE_HAS_PORT | WG_INTERFACE_HAS_PUBLIC | WG_INTERFACE_HAS_PRIVATE
    );
    assert_eq!(iface.i_port, 51820);
    assert_eq!(
        iface.i_public,
        key(ALICE_PUBLIC),
        "curve25519 of the private key"
    );
    let mut clamped = alice;
    crate::crypto::curve25519::curve25519_clamp_secret(&mut clamped);
    assert_eq!(iface.i_private, clamped);
    assert_eq!(iface.i_peers_count, 1);

    let p: WgPeerIo = buf.get(size_of::<WgInterfaceIo>());
    assert_eq!(
        p.p_flags,
        WG_PEER_HAS_PUBLIC | WG_PEER_HAS_PSK | WG_PEER_HAS_PKA | WG_PEER_HAS_ENDPOINT
    );
    assert_eq!(p.p_protocol_version, 1);
    assert_eq!(p.p_public, bob);
    assert_eq!(p.p_psk, [7; WG_KEY_LEN]);
    assert_eq!(p.p_pka, 25);
    assert_eq!(p.p_endpoint.sa_sin(), endpoint);
    assert_eq!(&p.p_description[..5], b"peer\0");
    assert_eq!(p.p_aips_count, 2);
    let aips: Vec<WgAipIo> = (0..2)
        .map(|i| buf.get(size_of::<WgInterfaceIo>() + size_of::<WgPeerIo>() + i * 24))
        .collect();
    assert!(aips.contains(&aip([10, 77, 0, 2], 32)));
    assert!(aips.contains(&aip([10, 88, 0, 0], 16)));

    // A buffer too small gets the size and nothing else.
    data.wgd_size = size_of::<WgInterfaceIo>();
    assert_eq!(wg_ioctl_get(sc, &mut data), Ok(()));
    assert_eq!(data.wgd_size, want);

    // Our own public key is never a peer; a peer with a version from the future is refused.
    let mut buf2 = UserBuf::new(size_of::<WgInterfaceIo>() + size_of::<WgPeerIo>());
    buf2.put(
        0,
        &WgInterfaceIo {
            i_peers_count: 1,
            ..WgInterfaceIo::default()
        },
    );
    buf2.put(
        size_of::<WgInterfaceIo>(),
        &WgPeerIo {
            p_flags: WG_PEER_HAS_PUBLIC,
            p_public: key(ALICE_PUBLIC),
            ..WgPeerIo::default()
        },
    );
    let mut data2 = WgDataIo {
        wgd_interface: buf2.addr(),
        ..WgDataIo::default()
    };
    assert_eq!(wg_ioctl_set(sc, &mut data2), Ok(()));
    assert_eq!(sc.sc_peer_num.get(), 1);
    buf2.put(
        size_of::<WgInterfaceIo>(),
        &WgPeerIo {
            p_flags: WG_PEER_HAS_PUBLIC,
            p_protocol_version: 2,
            p_public: [9; WG_KEY_LEN],
            ..WgPeerIo::default()
        },
    );
    assert_eq!(wg_ioctl_set(sc, &mut data2), Err(Errno::EPFNOSUPPORT));
    teardown();
}

#[test]
fn wg_ioctl_dispatch() {
    let _g = setup();
    let sc = create(0);
    let ifp = &sc.sc_if;
    assert_eq!(ifp.if_type.get(), IFT_WIREGUARD);
    assert_eq!(ifp.if_mtu.get(), DEFAULT_MTU);
    assert_eq!(
        ifp.if_flags.get(),
        IFF_BROADCAST | IFF_MULTICAST | IFF_NOARP
    );
    assert_eq!(cstr(&ifp.if_xname.get()), b"wg0");

    let mut ifr = Ifreq::zeroed();
    net_lock();
    ifr.set_ifr_mtu(0);
    // SAFETY: an aligned `ifreq`, what SIOCSIFMTU takes.
    let r = unsafe { wg_ioctl(ifp, SIOCSIFMTU, ptr::from_mut(&mut ifr).cast()) };
    assert_eq!(r, Err(Errno::EINVAL));
    ifr.set_ifr_mtu(1400);
    // SAFETY: as above.
    let r = unsafe { wg_ioctl(ifp, SIOCSIFMTU, ptr::from_mut(&mut ifr).cast()) };
    assert_eq!(r, Ok(()));
    assert_eq!(ifp.if_mtu.get(), 1400);
    // SAFETY: as above; the command is not wg's.
    let r = unsafe {
        wg_ioctl(
            ifp,
            crate::sys::sockio::SIOCGIFMTU,
            ptr::from_mut(&mut ifr).cast(),
        )
    };
    assert_eq!(r, Err(Errno::ENOTTY));

    // SIOCGWG through the dispatch (it drops and retakes the net lock).
    let mut data = WgDataIo::default();
    // SAFETY: an aligned `wg_data_io`, what SIOCGWG takes.
    let r = unsafe { wg_ioctl(ifp, SIOCGWG, ptr::from_mut(&mut data).cast()) };
    assert_eq!(r, Ok(()));
    assert_eq!(data.wgd_size, size_of::<WgInterfaceIo>());

    // Up needs the UDP socket, which this tree cannot make yet: the interface stays down.
    ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
    // SAFETY: as above, for SIOCSIFFLAGS.
    let r = unsafe { wg_ioctl(ifp, SIOCSIFFLAGS, ptr::from_mut(&mut ifr).cast()) };
    assert!(r.is_err());
    assert_eq!(ifp.if_flags.get() & IFF_RUNNING, 0);
    net_unlock();
    teardown();
}

/// A packet header mbuf holding `bytes`.
fn packet(bytes: &[u8]) -> &'static Mbuf {
    let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
    if bytes.len() > MHLEN {
        let _ = m_clget(Some(m), M_DONTWAIT, bytes.len() as u32);
        assert!(m.m_flags().get() & M_EXT != 0, "cluster");
    }
    // SAFETY: the mbuf's buffer holds `bytes.len()` bytes.
    unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), mtod::<u8>(m), bytes.len()) };
    m.m_len().set(bytes.len() as u32);
    m.m_pkthdr().len.set(bytes.len() as i32);
    m
}

/// The bytes of a contiguous packet.
fn bytes(m: &Mbuf) -> Vec<u8> {
    let mut v = vec![0u8; m.m_pkthdr().len.get() as usize];
    m_copydata(m, 0, &mut v);
    v
}

/// An IPv4 header of a `len`-byte UDP packet from `src` to `dst`.
fn ip_header(src: [u8; 4], dst: [u8; 4], len: usize) -> Vec<u8> {
    let mut h = vec![0x45, 0];
    h.extend_from_slice(&(len as u16).to_be_bytes());
    h.extend_from_slice(&[0, 0, 0, 0, 64, 17, 0, 0]);
    h.extend_from_slice(&src);
    h.extend_from_slice(&dst);
    h
}

/// What the UDP socket's upcall gets: `payload` from `src`:`sport` to `dst`:51820.
fn deliver(sc: &'static WgSoftc, src: [u8; 4], sport: u16, dst: [u8; 4], payload: &[u8]) {
    let mut d = ip_header(src, dst, 28 + payload.len());
    d.extend_from_slice(&sport.to_be_bytes());
    d.extend_from_slice(&51820u16.to_be_bytes());
    d.extend_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
    d.extend_from_slice(&[0, 0]);
    d.extend_from_slice(payload);
    let m = packet(&d);
    let ip = mtod::<Ip>(m).cast_const();
    let uh = ip.cast::<u8>().wrapping_add(20).cast::<c_void>();
    // SAFETY: `ip` and `uh` point at the headers just written into the packet.
    let r = unsafe {
        wg_input(
            ptr::from_ref(sc).cast_mut().cast(),
            m,
            ip,
            ptr::null(),
            uh,
            28,
            None,
        )
    };
    assert!(r.is_none(), "wg_input consumes the packet");
}

#[test]
fn two_interfaces_handshake_and_carry_data() {
    let _g = setup();
    let sc0 = create(0);
    let sc1 = create(1);
    let (a, b) = ([192, 168, 77, 1], [192, 168, 77, 2]);
    let (ta, tb) = ([10, 77, 0, 1], [10, 77, 0, 2]);

    configure(
        sc0,
        &key(ALICE_PRIVATE),
        51820,
        &key(BOB_PUBLIC),
        sin(b, 51820),
        &[aip(tb, 32)],
    );
    configure(
        sc1,
        &key(BOB_PRIVATE),
        51820,
        &key(ALICE_PUBLIC),
        sin(a, 51820),
        &[aip(ta, 32)],
    );
    let bob0 = wg_peer_lookup(sc0, &key(BOB_PUBLIC)).expect("bob on wg0");
    let alice1 = wg_peer_lookup(sc1, &key(ALICE_PUBLIC)).expect("alice on wg1");

    // An initiation as wg_send_initiation builds it, through wg1's upcall and handshake
    // worker: the MACs check, the remote is found by its key, a response is made (its send
    // fails: no socket) and Bob's session waits for confirmation.
    let mut init = WgPktInitiation::zeroed();
    noise_create_initiation(
        &bob0.p_remote,
        &mut init.s_idx,
        &mut init.ue,
        &mut init.es,
        &mut init.ets,
    )
    .expect("initiation");
    init.t = WG_PKT_INITIATION;
    let mut macs = CookieMacs::default();
    cookie_maker_mac(&bob0.p_cookie, &mut macs, &pkt_bytes(&init)[..116]);
    init.m = macs;
    assert!(same(
        wg_index_get(sc0, init.s_idx).map(WgPeer::of_remote),
        bob0
    ));

    // A bad mac1 is dropped before any Diffie-Hellman.
    let mut bad = init;
    bad.m.mac1[0] ^= 1;
    deliver(sc1, a, 51820, b, pkt_bytes(&bad));
    wg_handshake_worker(ptr::from_ref(sc1).cast_mut().cast());
    assert!(alice1.p_remote.r_next.get().is_none());

    deliver(sc1, a, 51820, b, pkt_bytes(&init));
    assert_eq!(mq_len(&sc1.sc_handshake_queue), 1);
    wg_handshake_worker(ptr::from_ref(sc1).cast_mut().cast());
    assert_eq!(mq_len(&sc1.sc_handshake_queue), 0);
    assert!(alice1.p_remote.r_next.get().is_some(), "responder session");
    assert_eq!(alice1.p_endpoint.get().e_remote, sin(a, 51820));
    assert_eq!(alice1.p_counters_rx.get(), 148);
    noise_remote_clear(&alice1.p_remote);

    // A whole handshake through the interfaces' index tables, past the replay (TAI64N) and
    // flood (REJECT_INTERVAL) checks of the first initiation.
    advance_uptime(2 * crate::net::wg_noise::REJECT_INTERVAL);
    let mut s_idx = 0;
    let (mut ue, mut es, mut ets) = ([0; 32], [0; 48], [0; 28]);
    noise_create_initiation(&bob0.p_remote, &mut s_idx, &mut ue, &mut es, &mut ets)
        .expect("initiation");
    let remote = noise_consume_initiation(&sc1.sc_local, s_idx, &ue, &es, &ets).expect("consume");
    assert!(ptr::eq(remote, &alice1.p_remote));
    let (mut rs, mut rr, mut rue, mut en) = (0, 0, [0; 32], [0; 16]);
    noise_create_response(&alice1.p_remote, &mut rs, &mut rr, &mut rue, &mut en).expect("response");
    noise_remote_begin_session(&alice1.p_remote).expect("responder keys");
    noise_consume_response(&bob0.p_remote, rs, rr, &rue, &en).expect("consume response");
    noise_remote_begin_session(&bob0.p_remote).expect("initiator keys");

    // Alice sends 10.77.0.1 -> 10.77.0.2: wg_output finds Bob by the destination,
    // wg_qstart stages and queues it, the encap worker encrypts it.
    let mut inner = ip_header(ta, tb, 20 + 13);
    inner.extend_from_slice(b"hello, tunnel");
    let m = packet(&inner);
    let dst = sin(tb, 0);
    net_lock();
    // SAFETY: `dst` is a `sockaddr_in`, readable as the `sockaddr` it starts with.
    let r = unsafe { wg_output(&sc0.sc_if, m, ptr::from_ref(&dst).cast(), None) };
    net_unlock();
    assert_eq!(r, Ok(()));
    wg_qstart(&sc0.sc_if.if_snd);
    wg_encap_worker(ptr::from_ref(sc0).cast_mut().cast());
    let (m, t) = wg_queue_dequeue(&bob0.p_encap_queue).expect("encrypted");
    let wire = bytes(t.t_mbuf.get().expect("data message"));
    assert_eq!(wire.len(), 16 + wg_pkt_with_padding(inner.len()) + 16);
    assert_eq!(&wire[..4], &[4, 0, 0, 0]);
    assert_eq!(&wire[8..16], &0u64.to_le_bytes(), "the first nonce");
    m_freem(t.t_mbuf.get());
    m_freem(m);

    // Bob receives it: the index finds Alice, the decap worker decrypts it, confirms the
    // session and checks the inner source against Alice's allowed IPs.
    deliver(sc1, a, 51820, b, &wire);
    wg_decap_worker(ptr::from_ref(sc1).cast_mut().cast());
    let (m, t) = wg_queue_dequeue(&alice1.p_decap_queue).expect("decrypted");
    assert!(ptr::eq(t.t_mbuf.get().expect("inner packet"), m));
    assert_eq!(bytes(m), inner, "the padding is trimmed to ip_len");
    assert_eq!(m.m_pkthdr().ph_family.get(), AF_INET);
    assert!(alice1.p_remote.r_current.get().is_some(), "confirmed");
    m_freem(m);

    // Bob answers; a forged inner source from Bob's tunnel is refused on the way in.
    for (src, ok) in [(tb, true), ([10, 99, 0, 1], false)] {
        let mut reply = ip_header(src, ta, 20 + 4);
        reply.extend_from_slice(b"pong");
        let m = packet(&reply);
        let t = wg_tag_get(m).expect("tag");
        t.t_peer.set(Some(alice1));
        let _ = mq_push(&alice1.p_stage_queue, m);
        wg_queue_out(sc1, alice1);
        wg_encap_worker(ptr::from_ref(sc1).cast_mut().cast());
        let (m, t) = wg_queue_dequeue(&alice1.p_encap_queue).expect("encrypted");
        let wire = bytes(t.t_mbuf.get().expect("data message"));
        m_freem(t.t_mbuf.get());
        m_freem(m);

        deliver(sc0, b, 51820, a, &wire);
        wg_decap_worker(ptr::from_ref(sc0).cast_mut().cast());
        let (m, t) = wg_queue_dequeue(&bob0.p_decap_queue).expect("decrypted");
        // Only a packet from Bob's allowed IPs comes out.
        assert_eq!(t.t_mbuf.get().is_some(), ok, "src {src:?}");
        if ok {
            assert_eq!(bytes(m), reply);
        }
        m_freem(m);
    }
    teardown();
}
