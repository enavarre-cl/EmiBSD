//! Host tests for `<nfs/nfsm_subs.h>`: the reply and request dissectors, their error paths
//! (the chain freed and cleared, the error returned) and the views.

use std::vec;

use super::*;
use crate::kern::uipc_mbuf::tests::setup;
use crate::nfs::nfs_subs::nfsm_reqhead;
use crate::nfs::nfs_subs::tests::{bytes, chain};
use crate::nfs::nfsproto::{NFSX_V3FATTR, NfsFattr, Nfsv3Time};
use crate::nfs::xdr_subs::txdr_unsigned;
use crate::sys::uio::{Iovec, UioRw, UioSeg};

/// The raw XDR words of `w`, as bytes.
fn words(w: &[u32]) -> std::vec::Vec<u8> {
    w.iter()
        .flat_map(|x| txdr_unsigned(*x).to_ne_bytes())
        .collect()
}

#[test]
fn dissect_reads_words_and_structures() {
    let _g = setup();
    let w = words(&[1, 2, 0x10, 0x20]);
    let head = chain(&[&w[..6], &w[6..]]);
    let mut info = NfsmInfo::new();
    info.nmi_mrep = Some(head);
    info.dissect_from(Some(head));
    let tl = nfsm_dissect(&mut info, 8).expect("two words");
    assert_eq!(fxdr_unsigned(tl.get(0)), 1);
    assert_eq!(fxdr_unsigned(tl.get(1)), 2);
    let t: Nfsv3Time = nfsm_dissect(&mut info, 8).expect("a time").read(0);
    assert_eq!(
        (fxdr_unsigned(t.nfsv3_sec), fxdr_unsigned(t.nfsv3_nsec)),
        (0x10, 0x20)
    );
    // A short view reads the rest of a structure as zero.
    let tl = nfsm_dissect(&mut info, 0).expect("nothing");
    let fa: NfsFattr = tl.read(0);
    assert_eq!(fa, NfsFattr::default());
    assert!(size_of::<NfsFattr>() == NFSX_V3FATTR);

    // Past the end: the reply is freed and cleared, the error returned.
    assert_eq!(nfsm_dissect(&mut info, 4).err(), Some(Errno::EBADRPC));
    assert!(info.nmi_mrep.is_none());
}

#[test]
fn strsiz_and_mtouio() {
    let _g = setup();
    let mut w = words(&[5]);
    w.extend_from_slice(b"abcde\0\0\0");
    w.extend(words(&[300]));
    let head = chain(&[&w]);
    let mut info = NfsmInfo::new();
    info.nmi_mrep = Some(head);
    info.dissect_from(Some(head));
    let len = nfsm_strsiz(&mut info, 255).expect("a length");
    assert_eq!(len, 5);
    let mut buf = vec![0u8; 5];
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: 5,
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: 5,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    nfsm_mtouio(&mut info, &mut uio, len as i32).expect("the string");
    nfsm_mtouio(&mut info, &mut uio, -1).expect("a no-op");
    assert_eq!(&buf, b"abcde");
    assert_eq!(nfsm_strsiz(&mut info, 255).err(), Some(Errno::EBADRPC));
    assert!(info.nmi_mrep.is_none(), "too long: the reply is freed");
}

#[test]
fn server_side_dissect_and_adv() {
    let _g = setup();
    let w = words(&[7, 8, 9]);
    let head = chain(&[&w[..2], &w[2..]]);
    let mut nd = NfsrvDescript::new();
    nd.nd_mrep = Some(head);
    nd.nd_md = Some(head);
    nd.nd_dpos = mtod::<u8>(head);
    nfsd_adv(&mut nd, 4).expect("skip a word");
    assert_eq!(
        fxdr_unsigned(nfsd_dissect(&mut nd, 4).expect("a word").get(0)),
        8
    );
    let tl = nfsd_dissect(&mut nd, 4).expect("a word");
    assert_eq!(fxdr_unsigned(tl.get(0)), 9);
    assert_eq!(nfsd_adv(&mut nd, 4).err(), Some(Errno::EBADRPC));
    assert!(nd.nd_mrep.is_none());
}

#[test]
fn strtom_refuses_long_names() {
    let _g = setup();
    let mut info = NfsmInfo::new();
    let req = nfsm_reqhead(0);
    info.nmi_mreq = Some(req);
    let mut mb = req;
    nfsm_strtom(&mut info, &mut mb, b"ok", 2).expect("fits");
    assert_eq!(bytes(req), [0, 0, 0, 2, b'o', b'k', 0, 0]);
    assert_eq!(
        nfsm_strtom(&mut info, &mut mb, b"long", 3).err(),
        Some(Errno::ENAMETOOLONG)
    );
    assert!(info.nmi_mreq.is_none());
}

#[test]
fn build_views_write_words_and_structures() {
    let _g = setup();
    let req = nfsm_reqhead(0);
    let mut mb = req;
    let mut tl = crate::nfs::nfs_subs::nfsm_build(&mut mb, 12);
    tl.put(txdr_unsigned(1));
    tl.write(
        4,
        &Nfsv3Time::from_words([txdr_unsigned(2), txdr_unsigned(3)]),
    );
    assert_eq!(bytes(req), [0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3]);
    crate::kern::uipc_mbuf::m_freem(req);
}
