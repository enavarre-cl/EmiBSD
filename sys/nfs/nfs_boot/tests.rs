//! Host tests for `nfs_boot.c`: the parsers of the bootparam and mountd replies
//! (`bp_whoami_reply`, `bp_getfile_reply`, `md_mount_reply`) and the structures of
//! `nfsdiskless.h`. The RPCs themselves (`krpc_call`) need a network and are the smoke
//! test's.

use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::tests::setup;
use crate::nfs::krpc_subr::tests::pkt;
use crate::sys::mbuf::{M_DONTWAIT, MT_SONAME};

/// The XDR encoding of a string, as `xdr_string_encode` makes it.
fn xstr(s: &[u8]) -> Vec<u8> {
    let mut v = (s.len() as u32).to_be_bytes().to_vec();
    v.extend_from_slice(s);
    v.resize(4 + s.len().div_ceil(4) * 4, 0);
    v
}

/// The XDR encoding of an Internet address (type 1 and a word per byte).
fn xaddr(a: [u8; 4]) -> Vec<u8> {
    let mut v = 1u32.to_be_bytes().to_vec();
    for b in a {
        v.extend_from_slice(&u32::from(b).to_be_bytes());
    }
    v
}

#[test]
fn getfile_reply_gives_the_server_its_address_and_the_path() {
    let _g = setup();
    let mut b = xstr(b"fileserver");
    b.extend(xaddr([10, 0, 2, 2]));
    b.extend(xstr(b"/export/root"));
    b.extend([9, 9]); // trailing garbage is ignored
    let m = pkt(&[&b[..7], &b[7..30], &b[30..]]);

    let mut sin = SockaddrIn::default();
    let mut name = [0xffu8; MNAMELEN];
    let mut path = [0xffu8; MAXPATHLEN];
    bp_getfile_reply(Some(m), &mut sin, &mut name, &mut path).expect("parsed");
    assert_eq!(cstr(&name), b"fileserver");
    assert_eq!(cstr(&path), b"/export/root");
    assert_eq!(sin.sin_len, 16);
    assert_eq!(sin.sin_family, AF_INET);
    assert_eq!(sin.sin_addr.s_addr, u32::from_ne_bytes([10, 0, 2, 2]));
    assert_eq!(sin.sin_port, 0);
}

#[test]
fn getfile_reply_refuses_a_short_reply() {
    let _g = setup();
    let mut b = xstr(b"fileserver");
    b.extend(xaddr([10, 0, 2, 2]));
    // The path is missing.
    let m = pkt(&[&b]);
    let mut sin = SockaddrIn::default();
    let mut name = [0u8; MNAMELEN];
    let mut path = [0u8; MAXPATHLEN];
    assert!(bp_getfile_reply(Some(m), &mut sin, &mut name, &mut path).is_err());
    assert_eq!(
        sin.sin_family, 0,
        "the address is only set for a good reply"
    );
    assert!(bp_getfile_reply(None, &mut sin, &mut name, &mut path).is_err());
}

#[test]
fn getfile_reply_truncates_a_long_server_name() {
    let _g = setup();
    let long = [b'h'; 120];
    let mut b = xstr(&long);
    b.extend(xaddr([1, 2, 3, 4]));
    b.extend(xstr(b"/p"));
    let m = pkt(&[&b]);
    let mut sin = SockaddrIn::default();
    let mut name = [0u8; MNAMELEN];
    let mut path = [0u8; MAXPATHLEN];
    bp_getfile_reply(Some(m), &mut sin, &mut name, &mut path).expect("parsed");
    assert_eq!(cstr(&name).len(), MNAMELEN - 1);
    assert_eq!(cstr(&path), b"/p");
}

#[test]
fn whoami_reply_sets_the_server_the_names_and_the_gateway() {
    let _g = setup();
    let mut b: Vec<u8> = Vec::new();
    b.extend(0x0000_0801u32.to_be_bytes()); // the port of the bootparam server
    b.extend(0x0000_0040u32.to_be_bytes()); // the length of what follows
    b.extend(xstr(b"client"));
    b.extend(xstr(b"example.org"));
    b.extend(xaddr([10, 0, 2, 1]));
    let m = pkt(&[&b[..5], &b[5..]]);

    // The address the reply came from.
    let from = m_get(M_DONTWAIT, MT_SONAME).expect("an mbuf");
    let mut sa = [0u8; 16];
    sa[0] = 16;
    sa[1] = AF_INET;
    sa[4..8].copy_from_slice(&[10, 0, 2, 2]);
    // SAFETY: an mbuf's data area holds 16 bytes; `mtod` points at it.
    unsafe { ptr::copy_nonoverlapping(sa.as_ptr(), mtod::<u8>(from), 16) };
    from.m_len().set(16);

    let mut bpsin = SockaddrIn {
        sin_len: 16,
        sin_family: AF_INET,
        sin_addr: InAddr {
            s_addr: u32::from_ne_bytes([10, 0, 2, 255]),
        },
        sin_port: 111u16.to_be(),
        ..SockaddrIn::default()
    };
    let gw = bp_whoami_reply(Some(m), Some(from), &mut bpsin).expect("parsed");
    assert_eq!(gw.s_addr, u32::from_ne_bytes([10, 0, 2, 1]));
    assert_eq!(bpsin.sin_port, 0x0801u16.to_be());
    assert_eq!(bpsin.sin_addr.s_addr, u32::from_ne_bytes([10, 0, 2, 2]));
    // SAFETY: nothing else touches the hostname statics in this test.
    let (host, dom) = unsafe { (HOSTNAME.get(), DOMAINNAME.get()) };
    assert_eq!(cstr(host), b"client");
    assert_eq!(HOSTNAMELEN.load(Ordering::Relaxed), 6);
    assert_eq!(cstr(dom), b"example.org");
    assert_eq!(DOMAINNAMELEN.load(Ordering::Relaxed), 11);
}

#[test]
fn whoami_reply_refuses_a_bad_reply() {
    let _g = setup();
    let from = m_get(M_DONTWAIT, MT_SONAME).expect("an mbuf");
    from.m_len().set(16);
    let mut bpsin = SockaddrIn::default();
    // Too short for the callit header.
    assert_eq!(
        bp_whoami_reply(Some(pkt(&[&[0, 0, 0]])), Some(from), &mut bpsin).err(),
        Some(Errno::EBADRPC)
    );
    // The names are there, the gateway is not.
    let mut b: Vec<u8> = vec_of(&[0, 0, 0, 1, 0, 0, 0, 8]);
    b.extend(xstr(b"client"));
    b.extend(xstr(b"dom"));
    assert_eq!(
        bp_whoami_reply(Some(pkt(&[&b])), Some(from), &mut bpsin).err(),
        Some(Errno::EBADRPC)
    );
    assert_eq!(
        bp_whoami_reply(None, Some(from), &mut bpsin).err(),
        Some(Errno::EBADRPC)
    );
}

fn vec_of(b: &[u8]) -> Vec<u8> {
    b.to_vec()
}

/// The `nfs_args` `md_mount` fills.
fn args(v3: bool) -> NfsArgs {
    let mut a = NfsDlmount::new().ndm_args;
    if v3 {
        a.flags = NFSMNT_NFSV3;
    }
    a
}

#[test]
fn mount_reply_version_3_gives_a_variable_length_handle() {
    let _g = setup();
    let fh: Vec<u8> = (1..=28).collect();
    let mut b = vec_of(&[0, 0, 0, 0, 0, 0, 0, 28]);
    b.extend(&fh);
    // Split inside the handle's length word: it is pulled up.
    let m = pkt(&[&b[..6], &b[6..]]);
    let mut a = args(true);
    let mut out = [0xffu8; NFSX_V3FHMAX];
    md_mount_reply(m, 3, &mut a, &mut out).expect("a handle");
    assert_eq!(a.fhsize, 28);
    assert_eq!(&out[..28], &fh[..]);
    assert_eq!(out[28], 0xff, "nothing is written past the handle");
}

#[test]
fn mount_reply_version_2_gives_a_32_byte_handle() {
    let _g = setup();
    let fh: Vec<u8> = (100..132).collect();
    let mut b = vec_of(&[0, 0, 0, 0]);
    b.extend(&fh);
    let m = pkt(&[&b[..10], &b[10..]]);
    let mut a = args(false);
    let mut out = [0u8; NFSX_V3FHMAX];
    md_mount_reply(m, 2, &mut a, &mut out).expect("a handle");
    assert_eq!(a.fhsize, NFSX_V2FH as i32);
    assert_eq!(&out[..32], &fh[..]);
}

#[test]
fn mount_reply_errors() {
    let _g = setup();
    let mut a = args(true);
    let mut out = [0u8; NFSX_V3FHMAX];
    // Not even an errno.
    assert_eq!(
        md_mount_reply(pkt(&[&[0, 0]]), 3, &mut a, &mut out),
        Err(Errno::EBADRPC)
    );
    // The daemon refuses: its errno (EACCES), and nothing else is read.
    assert_eq!(
        md_mount_reply(pkt(&[&[0, 0, 0, 13]]), 3, &mut a, &mut out),
        Err(Errno::EACCES)
    );
    // An errno that is not one of ours.
    assert_eq!(
        md_mount_reply(pkt(&[&[0, 0, 1, 0]]), 3, &mut a, &mut out),
        Err(Errno::EIO)
    );
    // A version 3 handle that is too long.
    let mut b = vec_of(&[0, 0, 0, 0, 0, 0, 0, 65]);
    b.extend([0u8; 65]);
    assert_eq!(
        md_mount_reply(pkt(&[&b]), 3, &mut a, &mut out),
        Err(Errno::EBADRPC)
    );
    // A handle that the reply does not hold.
    let mut b = vec_of(&[0, 0, 0, 0, 0, 0, 0, 20]);
    b.extend([0u8; 5]);
    assert_eq!(
        md_mount_reply(pkt(&[&b]), 3, &mut a, &mut out),
        Err(Errno::EBADRPC)
    );
}

#[test]
fn the_diskless_structures_start_out_empty() {
    let nd = NfsDiskless::new();
    assert_eq!(nd.nd_boot.sin_family, 0);
    assert_eq!(nd.nd_root.ndm_args.flags, 0);
    assert_eq!(nd.nd_swap.ndm_host[0], 0);
    assert_eq!(nd.nd_swap.ndm_fh, [0; NFSX_V3FHMAX]);
    assert!(nd.sw_vp.is_none());
}

#[test]
fn the_remote_path_is_host_colon_path_cut_to_mnamelen() {
    let mut host = [0u8; MNAMELEN];
    host[..3].copy_from_slice(b"srv");
    remote_path(&mut host, b"/export/root");
    assert_eq!(cstr(&host), b"srv:/export/root");

    let mut host = [0u8; MNAMELEN];
    host[..3].copy_from_slice(b"srv");
    remote_path(&mut host, &[b'p'; 200]);
    assert_eq!(cstr(&host).len(), MNAMELEN - 1);
    assert_eq!(&host[..5], b"srv:p");
}
