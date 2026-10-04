//! Host tests for `krpc_subr.c`: the XDR string and address encoders and decoders (including
//! across mbuf boundaries), the portmapper shortcut, `krpc_get_xid` and the early failures of
//! `krpc_call`.

use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::m_copyback;
use crate::kern::uipc_mbuf::tests::setup;
use crate::nfs::nfs_subs::tests::{bytes, lens};
use crate::sys::mbuf::M_DONTWAIT;

/// A packet header chain holding `parts`, one mbuf each.
pub(crate) fn pkt(parts: &[&[u8]]) -> &'static Mbuf {
    let mut head: Option<&'static Mbuf> = None;
    let mut tail: Option<&'static Mbuf> = None;
    for p in parts {
        let m = if head.is_none() {
            m_gethdr(M_DONTWAIT, MT_DATA)
        } else {
            m_get(M_DONTWAIT, MT_DATA)
        }
        .expect("an mbuf");
        m.m_len().set(p.len() as u32);
        m_copyback(m, 0, p, M_DONTWAIT).expect("copyback");
        match tail {
            Some(t) => t.m_next().set(Some(m)),
            None => head = Some(m),
        }
        tail = Some(m);
    }
    let head = head.expect("at least one part");
    m_calchdrlen(head);
    head
}

#[test]
fn string_encode_pads_to_a_word() {
    let _g = setup();
    let m = xdr_string_encode(b"/export/root").expect("mbuf");
    assert_eq!(
        bytes(m),
        [
            0, 0, 0, 12, b'/', b'e', b'x', b'p', b'o', b'r', b't', b'/', b'r', b'o', b'o', b't'
        ]
    );
    m_freem(m);

    let m = xdr_string_encode(b"abcde").expect("mbuf");
    assert_eq!(
        bytes(m),
        [0, 0, 0, 5, b'a', b'b', b'c', b'd', b'e', 0, 0, 0]
    );
    m_freem(m);

    let m = xdr_string_encode(b"").expect("mbuf");
    assert_eq!(bytes(m), [0, 0, 0, 0]);
    m_freem(m);
}

#[test]
fn string_encode_uses_a_cluster_when_it_does_not_fit_and_refuses_when_too_big() {
    let _g = setup();
    let long = [b'x'; 300];
    let m = xdr_string_encode(&long).expect("mbuf");
    assert!(m.m_flags().get() & M_EXT != 0);
    assert_eq!(lens(m), [304]);
    assert_eq!(&bytes(m)[..4], [0, 0, 1, 44]);
    m_freem(m);

    // The largest that fits a cluster, and one more.
    let max = std::vec![b'y'; MCLBYTES - 4];
    let m = xdr_string_encode(&max).expect("mbuf");
    assert_eq!(lens(m), [MCLBYTES]);
    m_freem(m);
    let big = std::vec![b'y'; MCLBYTES - 3];
    assert!(xdr_string_encode(&big).is_none());
}

#[test]
fn string_decode_takes_the_string_and_trims_the_chain() {
    let _g = setup();
    // "abcde" padded, then "tail", split across mbufs in the middle of the string.
    let m = pkt(&[
        &[0, 0, 0, 5, b'a', b'b'],
        &[b'c', b'd', b'e', 0, 0, 0, b't', b'a', b'i', b'l'],
    ]);
    let mut buf = [0xffu8; 16];
    let mut len = 15;
    let m = xdr_string_decode(m, &mut buf, &mut len).expect("decoded");
    assert_eq!(len, 5);
    assert_eq!(&buf[..6], b"abcde\0");
    assert_eq!(bytes(m), b"tail");
    assert_eq!(m.m_pkthdr().len.get(), 4);
    m_freem(m);

    // A string longer than the room: truncated and terminated, but the whole string is
    // skipped.
    let m = pkt(&[&[0, 0, 0, 5, b'a', b'b', b'c', b'd', b'e', 0, 0, 0, 9]]);
    let mut buf = [0xffu8; 16];
    let mut len = 3;
    let m = xdr_string_decode(m, &mut buf, &mut len).expect("decoded");
    assert_eq!((len, &buf[..4]), (3, &b"abc\0"[..]));
    assert_eq!(bytes(m), [9]);
    m_freem(m);

    // The buffer's own size bounds it too.
    let m = pkt(&[&[0, 0, 0, 5, b'a', b'b', b'c', b'd', b'e', 0, 0, 0]]);
    let mut buf = [0xffu8; 3];
    let mut len = 100;
    let m = xdr_string_decode(m, &mut buf, &mut len).expect("decoded");
    assert_eq!((len, &buf[..]), (2, &b"ab\0"[..]));
    m_freem(m);
}

#[test]
fn string_decode_refuses_short_and_absurd_chains() {
    let _g = setup();
    let mut buf = [0u8; 8];
    let mut len = 7;
    // Not even a length word.
    assert!(xdr_string_decode(pkt(&[&[0, 0]]), &mut buf, &mut len).is_none());
    // A length that runs past the end of the chain.
    assert!(xdr_string_decode(pkt(&[&[0, 0, 0, 9, b'a', b'b']]), &mut buf, &mut len).is_none());
    // A length that overflows an int.
    assert!(
        xdr_string_decode(
            pkt(&[&[0xff, 0xff, 0xff, 0xfe, 1, 2, 3, 4]]),
            &mut buf,
            &mut len
        )
        .is_none()
    );
    assert!(
        xdr_string_decode(
            pkt(&[&[0x7f, 0xff, 0xff, 0xff, 1, 2, 3, 4]]),
            &mut buf,
            &mut len
        )
        .is_none()
    );
    // The length word split across two mbufs is pulled up.
    let m = pkt(&[&[0, 0], &[0, 2, b'h', b'i', 0, 0]]);
    let m = xdr_string_decode(m, &mut buf, &mut len).expect("decoded");
    assert_eq!((len, &buf[..3]), (2, &b"hi\0"[..]));
    assert_eq!(m.m_pkthdr().len.get(), 0);
    m_freem(m);
}

#[test]
fn inaddr_encode_is_the_type_and_one_word_per_byte() {
    let _g = setup();
    let m = xdr_inaddr_encode(&InAddr {
        s_addr: u32::from_ne_bytes([10, 0, 2, 15]),
    })
    .expect("mbuf");
    assert_eq!(
        bytes(m),
        [0, 0, 0, 1, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 15]
    );
    m_freem(m);
}

#[test]
fn inaddr_decode_round_trips_and_trims() {
    let _g = setup();
    let ia = InAddr {
        s_addr: u32::from_ne_bytes([192, 168, 1, 77]),
    };
    let enc = xdr_inaddr_encode(&ia).expect("mbuf");
    // More data behind it, in another mbuf, and the address itself split in two.
    let mut v = bytes(enc);
    m_freem(enc);
    let rest = v.split_off(9);
    let m = pkt(&[&v, &rest, &[7, 7]]);
    let mut out = InAddr::default();
    let m = xdr_inaddr_decode(m, &mut out).expect("decoded");
    assert_eq!(out, ia);
    assert_eq!(bytes(m), [7, 7]);
    m_freem(m);

    // Another address type is INADDR_ANY.
    let mut w: Vec<u8> = Vec::new();
    for x in [2u32, 1, 2, 3, 4] {
        w.extend_from_slice(&txdr_unsigned(x).to_ne_bytes());
    }
    let m = pkt(&[&w]);
    let mut out = InAddr { s_addr: 5 };
    let m = xdr_inaddr_decode(m, &mut out).expect("decoded");
    assert_eq!(out.s_addr, INADDR_ANY);
    assert_eq!(lens(m), [0]);
    m_freem(m);

    // Too short: refused (the chain is freed).
    let mut out = InAddr::default();
    assert!(xdr_inaddr_decode(pkt(&[&w[..12]]), &mut out).is_none());
}

#[test]
fn xids_are_not_repeated_and_never_zero() {
    let _g = setup();
    let ids: Vec<u32> = (0..64).map(|_| krpc_get_xid()).collect();
    assert!(ids.iter().all(|&x| x != 0));
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len());
}

#[test]
fn portmap_of_the_portmapper_needs_no_call_and_krpc_call_checks_the_family() {
    let _g = setup();
    let mut sin = SockaddrIn {
        sin_len: 16,
        sin_family: AF_INET,
        ..SockaddrIn::default()
    };
    assert_eq!(
        krpc_portmap(&mut sin, PMAPPROG, PMAPVERS),
        Ok(111u16.to_be())
    );
    assert_eq!(sin.sin_port, 0, "untouched: no call was made");

    // A non-INET address is refused and the body freed.
    let body = pkt(&[&[1, 2, 3, 4]]);
    let mut data = Some(body);
    sin.sin_family = 24;
    assert_eq!(
        krpc_call(&sin, 100003, 2, 0, &mut data, None, 1),
        Err(Errno::EAFNOSUPPORT)
    );
    assert!(data.is_none());
}

#[test]
fn wire_structures_have_the_c_sizes() {
    // struct rpc_call: 6 words + 2 (auth) + 5 (auth_unix) + 2 (verf).
    assert_eq!(size_of::<RpcCall>(), 60);
    let call = RpcCall {
        rp_xid: txdr_unsigned(0x0102_0304),
        rp_rpcvers: txdr_unsigned(2),
        auth_authtype: txdr_unsigned(RPCAUTH_UNIX),
        auth_authlen: txdr_unsigned(20),
        ..RpcCall::default()
    };
    let b = xdr_bytes(&call);
    assert_eq!(&b[..12], [1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 2]);
    assert_eq!(&b[24..32], [0, 0, 0, 1, 0, 0, 0, 20]);
    assert!(b[32..].iter().all(|&x| x == 0));
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn constants_match_the_c_header() {
    let defs = crate::reftest::defines("sys/nfs/krpc.h");
    for (name, value) in [
        ("PMAPPORT", i64::from(PMAPPORT)),
        ("PMAPPROG", i64::from(PMAPPROG)),
        ("PMAPVERS", i64::from(PMAPVERS)),
        ("PMAPPROC_NULL", i64::from(PMAPPROC_NULL)),
        ("PMAPPROC_SET", i64::from(PMAPPROC_SET)),
        ("PMAPPROC_UNSET", i64::from(PMAPPROC_UNSET)),
        ("PMAPPROC_GETPORT", i64::from(PMAPPROC_GETPORT)),
        ("PMAPPROC_DUMP", i64::from(PMAPPROC_DUMP)),
        ("PMAPPROC_CALLIT", i64::from(PMAPPROC_CALLIT)),
        ("BOOTPARAM_PROG", i64::from(BOOTPARAM_PROG)),
        ("BOOTPARAM_VERS", i64::from(BOOTPARAM_VERS)),
        ("BOOTPARAM_WHOAMI", i64::from(BOOTPARAM_WHOAMI)),
        ("BOOTPARAM_GETFILE", i64::from(BOOTPARAM_GETFILE)),
    ] {
        assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
    }
}
