//! Host tests for PF_KEY: `pfkeyv2_parsemessage` and the conversions of `SADB_ADD` and
//! `SADB_X_ADDFLOW` messages as `ipsecctl(8)` writes them, and a PF_KEY socket end to end
//! (`SADB_REGISTER`, `SADB_ADD`, `SADB_X_ADDFLOW` and the SPD lookup it enables,
//! `SADB_GET`, the `net.key` dumps, `SADB_DELETE`, `SADB_FLUSH`).
//!
//! The tests run as a thread with root credentials made `curproc` (`socket(PF_KEY)` needs
//! `SS_PRIV`, and the messages carry the process's pid); they clear `curproc` before they
//! return.

use std::boxed::Box;
use std::sync::MutexGuard;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::crypto::crypto::crypto_reset;
use crate::crypto::cryptosoft::swcr_init;
use crate::kern::kern_proc::procinit;
use crate::kern::kern_prot::{crget, crhold};
use crate::kern::uipc_socket::{soclose, socreate, soinit, soreceive, sosend};
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::net::pfkeyv2_convert::import_lifetime;
use crate::net::radix::rn_test_reset;
use crate::netinet::in_::IPPROTO_ESP;
use crate::netinet::ip_input::tests::sin;
use crate::netinet::ip_ipsp::{
    IPSP_DIRECTION_OUT, TDBF_ALLOCATIONS, TDBF_BYTES, TDBF_TIMER, TDBF_TUNNELING, XF_ESP, gettdb,
    ipsp_reset, tdb_unref,
};
use crate::netinet::ip_spd::{ipsp_spd_lookup, spd_reset};
use crate::sys::endian::htonl;
use crate::sys::proc::Process;
use crate::sys::socket::{MSG_DONTWAIT, PF_KEY as PFK};
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};

type Guards = (
    (MutexGuard<'static, ()>, MutexGuard<'static, ()>),
    MutexGuard<'static, ()>,
);

/// The pid the test thread's process has.
const PID: i32 = 42;

/// The network setup, the socket layer, empty IPsec tables, the software crypto driver,
/// `pfkey_init`, and a root thread as `curproc`.
fn setup() -> Guards {
    let g = crate::netinet::ip_input::tests::setup();
    let s = crate::crypto::testutil::serial();
    crypto_reset();
    swcr_init();
    crate::net::if_::IFNETLIST.0.init();
    crate::net::if_::IFG_HEAD.0.init();
    procinit();
    soinit();
    rn_test_reset();
    spd_reset();
    ipsp_reset();
    pfkey_reset();
    pfkey_init();

    let pr: &'static Process = Box::leak(Box::new(Process::new()));
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    p.p_p.set(pr);
    pr.ps_mainproc.set(p);
    pr.ps_pid.set(PID);
    let cr = crget();
    p.p_ucred.set(cr);
    pr.ps_ucred.set(crhold(cr));
    Machine::set_curproc(Machine::curcpu(), p);
    (g, s)
}

/// Undoes what outlives the reset memory: `curproc`.
fn teardown() {
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// The bytes of a `T` (an ABI structure without padding).
fn bytes_of<T>(v: &T) -> &[u8] {
    // SAFETY: the callers pass `#[repr(C)]` PF_KEY structures and socket addresses, which
    // have no padding.
    unsafe { core::slice::from_raw_parts(ptr::from_ref(v).cast::<u8>(), size_of::<T>()) }
}

/// A PF_KEY message under construction: the header, then extensions.
struct Msg(Vec<u8>);

impl Msg {
    fn new(type_: u8, satype: u8, seq: u32) -> Self {
        let h = SadbMsg {
            sadb_msg_version: PF_KEY_V2,
            sadb_msg_type: type_,
            sadb_msg_satype: satype,
            sadb_msg_seq: seq,
            sadb_msg_pid: PID as u32,
            ..SadbMsg::default()
        };
        Msg(bytes_of(&h).to_vec())
    }

    /// Appends an extension of `type_`: `hdr` (its first four bytes are the length and type,
    /// set here) and `data`, padded to 8 bytes.
    fn ext<T>(mut self, type_: u16, hdr: T, data: &[u8]) -> Self {
        let start = self.0.len();
        self.0.extend_from_slice(bytes_of(&hdr));
        self.0.extend_from_slice(data);
        self.0.resize(start + padup(self.0.len() - start), 0);
        let words = ((self.0.len() - start) / 8) as u16;
        self.0[start..start + 2].copy_from_slice(&words.to_ne_bytes());
        self.0[start + 2..start + 4].copy_from_slice(&type_.to_ne_bytes());
        self
    }

    fn address(self, type_: u16, a: [u8; 4], port: u16) -> Self {
        let mut s = sin(a);
        s.sin_port = port.to_be();
        self.ext(type_, SadbAddress::default(), bytes_of(&s))
    }

    fn done(mut self) -> Vec<u8> {
        let words = (self.0.len() / 8) as u16;
        self.0[4..6].copy_from_slice(&words.to_ne_bytes());
        self.0
    }
}

const SPI: u32 = 0x4242;
const ENCKEY: [u8; 16] = [7; 16];
const AUTHKEY: [u8; 32] = [9; 32];
const LOCAL: [u8; 4] = [192, 168, 77, 1];
const PEER: [u8; 4] = [192, 168, 77, 2];

/// `ipsecctl -f` of `esp tunnel from 10.77.1.0/24 to 10.77.2.0/24 peer 192.168.77.2 spi
/// 0x4242 auth hmac-sha2-256 enc aes`: the `SADB_ADD` of the SA.
fn sadb_add() -> Vec<u8> {
    let sa = SadbSa {
        sadb_sa_spi: htonl(SPI),
        sadb_sa_replay: 64,
        sadb_sa_state: SADB_SASTATE_MATURE,
        sadb_sa_auth: SADB_X_AALG_SHA2_256,
        sadb_sa_encrypt: SADB_X_EALG_AES,
        sadb_sa_flags: SADB_X_SAFLAGS_TUNNEL,
        ..SadbSa::default()
    };
    Msg::new(SADB_ADD, SADB_SATYPE_ESP, 1)
        .ext(SADB_EXT_SA, sa, &[])
        .address(SADB_EXT_ADDRESS_SRC, LOCAL, 0)
        .address(SADB_EXT_ADDRESS_DST, PEER, 0)
        .ext(
            SADB_EXT_KEY_AUTH,
            SadbKey {
                sadb_key_bits: 256,
                ..SadbKey::default()
            },
            &AUTHKEY,
        )
        .ext(
            SADB_EXT_KEY_ENCRYPT,
            SadbKey {
                sadb_key_bits: 128,
                ..SadbKey::default()
            },
            &ENCKEY,
        )
        .done()
}

/// The `SADB_X_ADDFLOW` of the same rule: 10.77.1.0/24 to 10.77.2.0/24 out, through the
/// peer.
fn sadb_x_addflow() -> Vec<u8> {
    Msg::new(SADB_X_ADDFLOW, SADB_SATYPE_ESP, 2)
        .address(SADB_EXT_ADDRESS_DST, PEER, 0)
        .address(SADB_X_EXT_SRC_FLOW, [10, 77, 1, 0], 0)
        .address(SADB_X_EXT_SRC_MASK, [255, 255, 255, 0], 0)
        .address(SADB_X_EXT_DST_FLOW, [10, 77, 2, 0], 0)
        .address(SADB_X_EXT_DST_MASK, [255, 255, 255, 0], 0)
        .ext(SADB_X_EXT_PROTOCOL, SadbProtocol::default(), &[])
        .ext(
            SADB_X_EXT_FLOW_TYPE,
            SadbProtocol {
                sadb_protocol_proto: SADB_X_FLOW_TYPE_REQUIRE,
                sadb_protocol_direction: IPSP_DIRECTION_OUT,
                sadb_protocol_flags: SADB_X_POLICYFLAGS_POLICY,
                ..SadbProtocol::default()
            },
            &[],
        )
        .done()
}

#[test]
fn sadb_add_parses_and_imports_into_a_tdb() {
    let _g = setup();
    let mut msg = sadb_add();
    let mut headers = sadb_headers_new();
    pfkeyv2_parsemessage(&mut msg, &mut headers).expect("a valid SADB_ADD");

    let base = msg.as_ptr() as usize;
    assert_eq!(headers[0] as usize, base);
    assert_eq!(headers[usize::from(SADB_EXT_SA)] as usize, base + 16);
    assert_eq!(
        headers[usize::from(SADB_EXT_ADDRESS_SRC)] as usize,
        base + 32
    );
    assert_eq!(
        headers[usize::from(SADB_EXT_ADDRESS_DST)] as usize,
        base + 56
    );
    assert_eq!(headers[usize::from(SADB_EXT_KEY_AUTH)] as usize, base + 80);
    assert_eq!(
        headers[usize::from(SADB_EXT_KEY_ENCRYPT)] as usize,
        base + 120
    );
    assert!(headers[usize::from(SADB_EXT_LIFETIME_HARD)].is_null());

    // import_sa, import_address, import_key, as SADB_ADD does.
    let t = Tdb::new();
    let mut ii = IpsecInit::default();
    // SAFETY: the headers are the parsed message's.
    unsafe {
        import_sa(&t, sadb_ext::<SadbSa>(&headers, SADB_EXT_SA), Some(&mut ii));
        let mut su = SockaddrUnion::new();
        import_address(&mut su, headers[usize::from(SADB_EXT_ADDRESS_DST)]);
        t.tdb_dst.set(su);
        import_key(
            &mut ii,
            headers[usize::from(SADB_EXT_KEY_AUTH)],
            PFKEYV2_AUTHENTICATION_KEY,
        );
        import_key(
            &mut ii,
            headers[usize::from(SADB_EXT_KEY_ENCRYPT)],
            PFKEYV2_ENCRYPTION_KEY,
        );
    }
    assert_eq!(t.tdb_spi.get(), htonl(SPI));
    assert_eq!(t.tdb_wnd.get(), 64);
    assert!(t.has_flags(TDBF_TUNNELING));
    assert!(!t.has_flags(TDBF_INVALID), "a mature SA");
    assert_eq!(t.tdb_dst.get().sa_family(), AF_INET);
    assert_eq!(t.tdb_dst.get().sin_addr(), sin(PEER).sin_addr);
    assert_eq!(ii.ii_encalg, SADB_X_EALG_AES);
    assert_eq!(ii.ii_authalg, SADB_X_AALG_SHA2_256);
    assert_eq!((ii.ii_enckeylen, ii.ii_enckey), (16, &ENCKEY[..]));
    assert_eq!((ii.ii_authkeylen, ii.ii_authkey), (32, &AUTHKEY[..]));

    // Lifetimes set their flags; a hard byte limit is a TDBF_BYTES.
    import_lifetime(
        &t,
        Some(SadbLifetime {
            sadb_lifetime_bytes: 1 << 20,
            sadb_lifetime_addtime: 3600,
            ..SadbLifetime::default()
        }),
        PFKEYV2_LIFETIME_HARD,
    );
    assert!(t.has_flags(TDBF_BYTES) && t.has_flags(TDBF_TIMER));
    assert!(!t.has_flags(TDBF_ALLOCATIONS));
    assert_eq!(t.tdb_exp_timeout.get(), 3600);

    // export_sa writes back what import_sa read (and the transform's algorithms once set).
    let mut out = [0u8; 16];
    let mut p = out.as_mut_ptr();
    // SAFETY: room for a `struct sadb_sa`.
    unsafe { export_sa(&mut p, &t) };
    // SAFETY: as above.
    let back: SadbSa = unsafe { sadb_get(out.as_ptr()) };
    assert_eq!(back.sadb_sa_len, 2);
    assert_eq!(back.sadb_sa_spi, htonl(SPI));
    assert_eq!(back.sadb_sa_state, SADB_SASTATE_MATURE);
    assert_eq!(back.sadb_sa_flags, SADB_X_SAFLAGS_TUNNEL);
    teardown();
}

#[test]
fn malformed_messages_are_rejected() {
    let _g = setup();
    let mut headers = sadb_headers_new();
    let reject = |mut m: Vec<u8>, headers: &mut SadbHeaders| {
        assert_eq!(pfkeyv2_parsemessage(&mut m, headers), Err(Errno::EINVAL));
    };

    reject(vec![0; 8], &mut headers);
    let mut m = sadb_add();
    m[12] = 7; // another pid
    reject(m, &mut headers);
    let mut m = sadb_add();
    m[4] += 1; // the length
    reject(m, &mut headers);
    // SADB_ADD without its destination.
    let sa = SadbSa {
        sadb_sa_state: SADB_SASTATE_MATURE,
        ..SadbSa::default()
    };
    reject(
        Msg::new(SADB_ADD, SADB_SATYPE_ESP, 1)
            .ext(SADB_EXT_SA, sa, &[])
            .done(),
        &mut headers,
    );
    // A larval SA cannot be added.
    let larval = SadbSa {
        sadb_sa_state: SADB_SASTATE_LARVAL,
        ..SadbSa::default()
    };
    reject(
        Msg::new(SADB_ADD, SADB_SATYPE_ESP, 1)
            .ext(SADB_EXT_SA, larval, &[])
            .address(SADB_EXT_ADDRESS_DST, PEER, 0)
            .done(),
        &mut headers,
    );
    // A port on an SA address.
    reject(
        Msg::new(SADB_ADD, SADB_SATYPE_ESP, 1)
            .ext(SADB_EXT_SA, sa, &[])
            .address(SADB_EXT_ADDRESS_DST, PEER, 500)
            .done(),
        &mut headers,
    );
    // An extension the message type does not take.
    reject(
        Msg::new(SADB_DELETE, SADB_SATYPE_ESP, 1)
            .ext(SADB_EXT_SA, sa, &[])
            .address(SADB_EXT_ADDRESS_DST, PEER, 0)
            .ext(
                SADB_EXT_KEY_AUTH,
                SadbKey {
                    sadb_key_bits: 64,
                    ..SadbKey::default()
                },
                &[0; 8],
            )
            .done(),
        &mut headers,
    );
    teardown();
}

#[test]
fn sadb_x_addflow_imports_the_flow_and_its_mask() {
    let _g = setup();
    let mut msg = sadb_x_addflow();
    let mut headers = sadb_headers_new();
    pfkeyv2_parsemessage(&mut msg, &mut headers).expect("a valid SADB_X_ADDFLOW");

    let mut flow = SockaddrEncap::new();
    let mut mask = SockaddrEncap::new();
    // SAFETY: the headers are the parsed message's.
    unsafe {
        import_flow(
            &mut flow,
            &mut mask,
            headers[usize::from(SADB_X_EXT_SRC_FLOW)],
            headers[usize::from(SADB_X_EXT_SRC_MASK)],
            headers[usize::from(SADB_X_EXT_DST_FLOW)],
            headers[usize::from(SADB_X_EXT_DST_MASK)],
            sadb_ext::<SadbProtocol>(&headers, SADB_X_EXT_PROTOCOL),
            sadb_ext::<SadbProtocol>(&headers, SADB_X_EXT_FLOW_TYPE),
        )
    }
    .expect("import_flow");

    assert_eq!(usize::from(flow.sen_len()), size_of::<SockaddrEncap>());
    assert_eq!(flow.sen_family(), PFK);
    assert_eq!(flow.sen_type(), SENT_IP4);
    assert_eq!(flow.sen_direction(), IPSP_DIRECTION_OUT);
    assert_eq!(flow.sen_ip_src(), sin([10, 77, 1, 0]).sin_addr);
    assert_eq!(flow.sen_ip_dst(), sin([10, 77, 2, 0]).sin_addr);
    assert_eq!(flow.sen_proto(), 0);
    assert_eq!(mask.sen_direction(), 0xff);
    assert_eq!(mask.sen_ip_src(), sin([255, 255, 255, 0]).sin_addr);
    assert_eq!(mask.sen_ip_dst(), sin([255, 255, 255, 0]).sin_addr);
    assert_eq!(mask.sen_proto(), 0, "any protocol");

    // export_flow gives the extensions back.
    let mut buf = vec![0u8; 2 * 8 + 4 * 24];
    let mut out = sadb_headers_new();
    let mut p = buf.as_mut_ptr();
    // SAFETY: room for two protocol and four address extensions.
    unsafe { export_flow(&mut p, IPSP_IPSEC_REQUIRE, &flow, &mask, &mut out) };
    assert_eq!(p as usize - buf.as_ptr() as usize, buf.len());
    // SAFETY: the headers point into `buf`.
    let ft: SadbProtocol = unsafe { sadb_get(out[usize::from(SADB_X_EXT_FLOW_TYPE)]) };
    assert_eq!(ft.sadb_protocol_proto, SADB_X_FLOW_TYPE_REQUIRE);
    assert_eq!(ft.sadb_protocol_direction, IPSP_DIRECTION_OUT);
    // SAFETY: as above.
    let dst = unsafe { sadb_address_sunion(out[usize::from(SADB_X_EXT_DST_FLOW)]) };
    assert_eq!(dst.sin_addr(), sin([10, 77, 2, 0]).sin_addr);
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
    let mut buf = vec![0u8; 4096];
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

/// The header of a message.
fn header(m: &[u8]) -> SadbMsg {
    assert!(m.len() >= size_of::<SadbMsg>());
    // SAFETY: the message holds a header.
    unsafe { sadb_get(m.as_ptr()) }
}

/// The extension types of a reply, in order.
fn ext_types(m: &[u8]) -> Vec<u16> {
    let mut v = Vec::new();
    let mut off = size_of::<SadbMsg>();
    while off < m.len() {
        // SAFETY: inside the message.
        let e: SadbExt = unsafe { sadb_get(m.as_ptr().add(off)) };
        v.push(e.sadb_ext_type);
        off += usize::from(e.sadb_ext_len) * 8;
    }
    assert_eq!(off, m.len());
    v
}

/// An IPv4 UDP packet from `src` to `dst`, as `ip_output` sees it.
fn udp_packet(src: [u8; 4], dst: [u8; 4]) -> &'static Mbuf {
    let mut p = vec![0u8; 28];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&28u16.to_be_bytes());
    p[9] = 17;
    p[12..16].copy_from_slice(&src);
    p[16..20].copy_from_slice(&dst);
    p[20..22].copy_from_slice(&1234u16.to_be_bytes());
    p[22..24].copy_from_slice(&53u16.to_be_bytes());
    crate::net::if_::tests::test_packet(&p)
}

#[test]
fn a_pfkey_socket_registers_adds_an_sa_and_a_flow() {
    let _g = setup();

    let so = socreate(i32::from(PF_KEY), SOCK_RAW, i32::from(PF_KEY_V2)).expect("socket(PF_KEY)");

    // SADB_REGISTER: the reply lists the supported algorithms.
    send(so, &Msg::new(SADB_REGISTER, SADB_SATYPE_ESP, 7).done()).expect("register");
    let r = recv(so).expect("the reply");
    let h = header(&r);
    assert_eq!(h.sadb_msg_type, SADB_REGISTER);
    assert_eq!(h.sadb_msg_errno, 0);
    assert_eq!(h.sadb_msg_seq, 7);
    assert_eq!(h.sadb_msg_pid, PID as u32);
    assert_eq!(usize::from(h.sadb_msg_len) * 8, r.len());
    assert_eq!(
        ext_types(&r),
        [
            SADB_EXT_SUPPORTED_AUTH,
            SADB_EXT_SUPPORTED_ENCRYPT,
            SADB_X_EXT_SUPPORTED_COMP
        ]
    );
    assert_eq!(NREGISTERED.load(Ordering::Relaxed), 1);

    // SADB_ADD: the SA is in the database, set up by the ESP transform.
    send(so, &sadb_add()).expect("add");
    let r = recv(so).expect("the reply");
    assert_eq!(header(&r).sadb_msg_errno, 0);
    let types = ext_types(&r);
    assert!(types.contains(&SADB_EXT_SA) && types.contains(&SADB_EXT_ADDRESS_DST));
    assert!(
        !types.contains(&SADB_EXT_KEY_ENCRYPT),
        "keys are not echoed"
    );
    let peer = SockaddrUnion::from_sin(&sin(PEER));
    let t = gettdb(0, htonl(SPI), &peer, IPPROTO_ESP as u8).expect("the SA");
    assert_eq!(t.tdb_xform.get().map(|x| x.xf_type), Some(XF_ESP));
    assert_eq!(t.tdb_satype.get(), SADB_SATYPE_ESP);
    assert!(t.tdb_encalgxform.get().is_some() && t.tdb_authalgxform.get().is_some());
    assert!(t.has_flags(TDBF_TUNNELING));
    tdb_unref(Some(t));

    // The same SA again: EEXIST in the reply.
    send(so, &sadb_add()).expect("add again");
    let r = recv(so).expect("the reply");
    assert_eq!(header(&r).sadb_msg_errno, Errno::EEXIST as u8);

    // SADB_X_ADDFLOW: the SPD sends 10.77.1.0/24 -> 10.77.2.0/24 through the SA.
    send(so, &sadb_x_addflow()).expect("addflow");
    let r = recv(so).expect("the reply");
    assert_eq!(header(&r).sadb_msg_errno, 0);
    assert_eq!(IPSEC_IN_USE.load(Ordering::Relaxed), 1);

    let m = udp_packet([10, 77, 1, 5], [10, 77, 2, 9]);
    let mut tdb = None;
    ipsp_spd_lookup(
        m,
        i32::from(AF_INET),
        20,
        IPSP_DIRECTION_OUT,
        None,
        None,
        Some(&mut tdb),
        None,
    )
    .expect("IPsec required");
    let t = tdb.expect("the flow's SA");
    assert_eq!(t.tdb_spi.get(), htonl(SPI));
    tdb_unref(Some(t));
    crate::kern::uipc_mbuf::m_freem(m);
    // Other traffic does not match.
    let m = udp_packet([10, 77, 3, 5], [10, 77, 2, 9]);
    let mut tdb = None;
    ipsp_spd_lookup(
        m,
        i32::from(AF_INET),
        20,
        IPSP_DIRECTION_OUT,
        None,
        None,
        Some(&mut tdb),
        None,
    )
    .expect("no policy");
    assert!(tdb.is_none());
    crate::kern::uipc_mbuf::m_freem(m);
    // SADB_GET gives the SA back with its keys.
    let sa = SadbSa {
        sadb_sa_spi: htonl(SPI),
        ..SadbSa::default()
    };
    let get = Msg::new(SADB_GET, SADB_SATYPE_ESP, 3)
        .ext(SADB_EXT_SA, sa, &[])
        .address(SADB_EXT_ADDRESS_DST, PEER, 0)
        .done();
    send(so, &get).expect("get");
    let r = recv(so).expect("the reply");
    assert_eq!(header(&r).sadb_msg_errno, 0);
    let types = ext_types(&r);
    assert!(types.contains(&SADB_EXT_KEY_ENCRYPT) && types.contains(&SADB_X_EXT_COUNTER));

    // The sysctls ipsecctl -sa reads: the SA dump and the flow dump.
    for (op, type_) in [
        (NET_KEY_SADB_DUMP, SADB_DUMP),
        (NET_KEY_SPD_DUMP, SADB_X_SPDDUMP),
    ] {
        let mut size = 0;
        pfkeyv2_sysctl(&[op], 0, &mut size, 0, 0).expect("size");
        assert!(size > size_of::<SadbMsg>());
        let mut buf = vec![0u8; size];
        let mut len = size;
        pfkeyv2_sysctl(&[op], buf.as_mut_ptr() as usize, &mut len, 0, 0).expect("dump");
        assert_eq!(len, size);
        let h = header(&buf);
        assert_eq!(h.sadb_msg_type, type_);
        assert_eq!(usize::from(h.sadb_msg_len) * 8, len, "one message");
    }

    // SADB_DELETE, then SADB_FLUSH takes the flow.
    let del = Msg::new(SADB_DELETE, SADB_SATYPE_ESP, 4)
        .ext(SADB_EXT_SA, sa, &[])
        .address(SADB_EXT_ADDRESS_DST, PEER, 0)
        .done();
    send(so, &del).expect("delete");
    assert_eq!(header(&recv(so).expect("the reply")).sadb_msg_errno, 0);
    assert!(gettdb(0, htonl(SPI), &peer, IPPROTO_ESP as u8).is_none());

    send(so, &Msg::new(SADB_FLUSH, SADB_SATYPE_UNSPEC, 5).done()).expect("flush");
    assert_eq!(header(&recv(so).expect("the reply")).sadb_msg_errno, 0);
    assert_eq!(IPSEC_IN_USE.load(Ordering::Relaxed), 0, "the flow went too");

    soclose(so, 0).expect("close");
    assert_eq!(NREGISTERED.load(Ordering::Relaxed), 0);
    teardown();
}

/// An `SADB_ADD` of an ESP tunnel SA from `src` to `dst`, as ipsecctl(8) sends a static one
/// (no replay window).
fn sa_msg(spi: u32, src: [u8; 4], dst: [u8; 4], seq: u32) -> Vec<u8> {
    let sa = SadbSa {
        sadb_sa_spi: htonl(spi),
        sadb_sa_state: SADB_SASTATE_MATURE,
        sadb_sa_auth: SADB_X_AALG_SHA2_256,
        sadb_sa_encrypt: SADB_X_EALG_AES,
        sadb_sa_flags: SADB_X_SAFLAGS_TUNNEL,
        ..SadbSa::default()
    };
    Msg::new(SADB_ADD, SADB_SATYPE_ESP, seq)
        .ext(SADB_EXT_SA, sa, &[])
        .address(SADB_EXT_ADDRESS_SRC, src, 0)
        .address(SADB_EXT_ADDRESS_DST, dst, 0)
        .ext(
            SADB_EXT_KEY_AUTH,
            SadbKey {
                sadb_key_bits: 256,
                ..SadbKey::default()
            },
            &AUTHKEY,
        )
        .ext(
            SADB_EXT_KEY_ENCRYPT,
            SadbKey {
                sadb_key_bits: 128,
                ..SadbKey::default()
            },
            &ENCKEY,
        )
        .done()
}

/// An `SADB_X_ADDFLOW` requiring ESP through `peer` for `src`/`smask` to `dst`/`dmask`.
fn flow_msg(dir: u8, src: [[u8; 4]; 2], dst: [[u8; 4]; 2], peer: [u8; 4], seq: u32) -> Vec<u8> {
    Msg::new(SADB_X_ADDFLOW, SADB_SATYPE_ESP, seq)
        .address(SADB_EXT_ADDRESS_DST, peer, 0)
        .address(SADB_X_EXT_SRC_FLOW, src[0], 0)
        .address(SADB_X_EXT_SRC_MASK, src[1], 0)
        .address(SADB_X_EXT_DST_FLOW, dst[0], 0)
        .address(SADB_X_EXT_DST_MASK, dst[1], 0)
        .ext(SADB_X_EXT_PROTOCOL, SadbProtocol::default(), &[])
        .ext(
            SADB_X_EXT_FLOW_TYPE,
            SadbProtocol {
                sadb_protocol_proto: SADB_X_FLOW_TYPE_REQUIRE,
                sadb_protocol_direction: dir,
                ..SadbProtocol::default()
            },
            &[],
        )
        .done()
}

/// RFC 1071 over `b`.
fn cksum(b: &[u8]) -> u16 {
    let mut sum: u32 = b
        .chunks(2)
        .map(|w| u32::from(u16::from_be_bytes([w[0], *w.get(1).unwrap_or(&0)])))
        .sum();
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// The far end of a tunnel (`smoke-esp`'s B, here the host under test at 10.0.2.15 with
/// 10.77.2.1 on lo0 and a default route through 10.0.2.2): an echo request from 10.77.1.1
/// comes in through ESP from the peer 10.0.2.2 (SPI 0x1001), is decapsulated, passes the
/// inbound policy and, on a forwarding gateway, is answered through the reverse SA (SPI
/// 0x1002); a plain host drops it as received on the wrong interface (`NBPFILTER` is 0).
#[test]
fn an_esp_tunnel_echo_request_is_answered_through_the_reverse_sa() {
    use crate::net::route::{
        RTAX_DST, RTAX_GATEWAY, RTAX_NETMASK, RTF_GATEWAY, RTF_STATIC, RTM_ADD, RtAddrinfo, rtfree,
        rtrequest,
    };
    use crate::netinet::in_::sintosa;
    use crate::netinet::ip_input::tests::{ADDR, GATEWAY, configure, test_ether};
    use crate::netinet::ip_input::{IPCOUNTERS, ip_forwarding};
    use crate::netinet::ip_ipsp::{IPSP_DF_INHERIT, IPSP_DIRECTION_IN, TdbCounters};
    use crate::netinet::ip_var::IpstatCounters;
    use crate::netinet::ipsec_output::ipsp_process_packet;

    let _g = setup();
    let ifp = test_ether();
    configure(ifp, ADDR, [255, 255, 255, 0]);
    crate::net::if_loop::loop_clone_create(&crate::net::if_loop::LOOP_CLONER, 0).expect("lo0");
    // enc0: ip_output_ipsec_send runs pf_test on it, and drops the packet without it.
    crate::net::if_enc::enc_reset();
    crate::net::if_enc::enc_clone_create(&crate::net::if_enc::ENC_CLONER, 0).expect("enc0");
    let lo = crate::net::if_::if_get(crate::net::rtable::rtable_loindex(0)).expect("lo0");
    configure(lo, [10, 77, 2, 1], [255, 255, 255, 255]);
    // route add default 10.0.2.2
    let mut dst = sin([0; 4]);
    let mut mask = sin([0; 4]);
    let mut gw = sin(GATEWAY);
    let mut info = RtAddrinfo::new();
    info.rti_info[RTAX_DST] = sintosa(&mut dst);
    info.rti_info[RTAX_NETMASK] = sintosa(&mut mask);
    info.rti_info[RTAX_GATEWAY] = sintosa(&mut gw);
    info.rti_flags = RTF_GATEWAY | RTF_STATIC;
    // SAFETY: a local `sockaddr_in`.
    info.rti_ifa = unsafe { crate::net::if_::ifaof_ifpforaddr(sintosa(&mut gw), ifp) };
    let mut rt = None;
    // SAFETY: the addresses are locals.
    unsafe { rtrequest(RTM_ADD, &mut info, 0, Some(&mut rt), 0) }.expect("default route");
    rtfree(rt);
    let so = socreate(i32::from(PF_KEY), SOCK_RAW, i32::from(PF_KEY_V2)).expect("socket(PF_KEY)");

    let net = [[10, 77, 1, 0], [255, 255, 255, 0]];
    let us = [[10, 77, 2, 0], [255, 255, 255, 0]];
    for msg in [
        sa_msg(0x1001, GATEWAY, ADDR, 1),
        sa_msg(0x1002, ADDR, GATEWAY, 2),
        flow_msg(IPSP_DIRECTION_OUT, us, net, GATEWAY, 3),
        flow_msg(IPSP_DIRECTION_IN, net, us, GATEWAY, 4),
    ] {
        send(so, &msg).expect("send");
        assert_eq!(header(&recv(so).expect("the reply")).sadb_msg_errno, 0);
    }

    // The peer's echo request, encrypted with SPI 0x1001: ip_output loops it to lo0, and it
    // is handed in from the peer on the Ethernet instead.
    let peer = SockaddrUnion::from_sin(&sin(ADDR));
    let ta = gettdb(0, htonl(0x1001), &peer, IPPROTO_ESP as u8).expect("SPI 0x1001");
    let esp_in = |seq: u8| {
        let mut p = vec![0u8; 20 + 8 + 16];
        p[0] = 0x45;
        p[2..4].copy_from_slice(&44u16.to_be_bytes());
        p[8] = 64;
        p[9] = 1;
        p[12..16].copy_from_slice(&[10, 77, 1, 1]);
        p[16..20].copy_from_slice(&[10, 77, 2, 1]);
        let s = cksum(&p[..20]);
        p[10..12].copy_from_slice(&s.to_be_bytes());
        p[20] = 8;
        p[24..28].copy_from_slice(&[0x12, 0x34, 0, seq]);
        p[28..].copy_from_slice(b"through the tun!");
        let s = cksum(&p[20..]);
        p[22..24].copy_from_slice(&s.to_be_bytes());
        ipsp_process_packet(
            crate::net::if_::tests::test_packet(&p),
            ta,
            i32::from(AF_INET),
            false,
            IPSP_DF_INHERIT,
        )
        .expect("encrypted");
        let ml = crate::sys::mbuf::MbufList::new();
        crate::kern::uipc_mbuf::ml_enlist(&ml, &lo.ifiq(0).ifiq_ml);
        let m = crate::kern::uipc_mbuf::ml_dequeue(&ml).expect("the ESP packet");
        let mut esp = crate::netinet::ip_input::tests::bytes(m);
        m_freem(m);
        assert_eq!(esp[9], IPPROTO_ESP as u8);
        // The loopback left the header checksum to its (offloaded) output.
        esp[10..12].fill(0);
        let s = cksum(&esp[..20]);
        esp[10..12].copy_from_slice(&s.to_be_bytes());
        let f = crate::netinet::ip_input::tests::frame(
            ifp,
            crate::netinet::ip_input::tests::OURS,
            crate::net::ethertypes::ETHERTYPE_IP,
            &esp,
        );
        crate::net::if_ethersubr::ether_input(ifp, f, None);
        crate::netinet::ip_input::ipintr();
    };
    let c = |t: &Tdb, k: TdbCounters| t.tdb_counters[k as usize].load(Ordering::Relaxed);
    let wrongif = || IPCOUNTERS[IpstatCounters::IpsWrongif as usize].load(Ordering::Relaxed);

    // A host: NBPFILTER is 0, so the decapsulated packet keeps vio's ph_ifidx (the C moves
    // it to enc0 only under NBPFILTER > 0), and 10.77.2.1 lives on lo0: ips_wrongif.
    let w = wrongif();
    esp_in(1);
    assert_eq!(c(ta, TdbCounters::TdbIpackets), 1, "decrypted");
    assert_eq!(wrongif(), w + 1, "received on the wrong interface");
    assert_eq!(crate::netinet::ip_input::tests::sent(|_, _| {}), 0);

    // A gateway (net.inet.ip.forwarding=1) takes it, answers, and the reply leaves through
    // the reverse SA.
    ip_forwarding.store(1, Ordering::Relaxed);
    esp_in(2);
    ip_forwarding.store(0, Ordering::Relaxed);
    assert_eq!(c(ta, TdbCounters::TdbIpackets), 2, "decrypted");
    crate::netinet::ip_input::tests::run_ip_send();
    let gw = SockaddrUnion::from_sin(&sin(GATEWAY));
    let tb = gettdb(0, htonl(0x1002), &gw, IPPROTO_ESP as u8).expect("SPI 0x1002");
    assert_eq!(
        c(tb, TdbCounters::TdbOpackets),
        1,
        "the reply went through ESP"
    );
    tdb_unref(Some(ta));
    tdb_unref(Some(tb));

    soclose(so, 0).expect("close");
    teardown();
}
