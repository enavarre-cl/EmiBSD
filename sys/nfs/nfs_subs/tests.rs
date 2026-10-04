//! Host tests for `nfs_subs.c`: dissecting and advancing across mbuf boundaries, building
//! and growing a request chain, the XDR strings and their padding, the uio copies both ways,
//! and the version 2 times.

use std::boxed::Box;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::tests::setup;
use crate::kern::uipc_mbuf::{m_copyback, m_copydata, m_freem, m_gethdr};
use crate::nfs::nfsmount::NfsMount;
use crate::nfs::nfsnode::NfsNode;
use crate::nfs::nfsproto::{NFSERR_NOENT, NFSERR_NOTDIR, NFSPROC_GETATTR, NFSPROC_LOOKUP};
use crate::sys::vnode::VT_NFS;

/// A chain of mbufs holding `parts`, one mbuf each.
pub(crate) fn chain(parts: &[&[u8]]) -> &'static Mbuf {
    let mut head: Option<&'static Mbuf> = None;
    let mut tail: Option<&'static Mbuf> = None;
    for p in parts {
        let m = m_get(M_WAIT, MT_DATA).expect("an mbuf");
        m.m_len().set(p.len() as u32);
        m_copyback(m, 0, p, M_WAIT).expect("copyback");
        match tail {
            Some(t) => t.m_next().set(Some(m)),
            None => head = Some(m),
        }
        tail = Some(m);
    }
    head.expect("at least one part")
}

/// The lengths of a chain's mbufs.
pub(crate) fn lens(m: &Mbuf) -> Vec<usize> {
    let mut v = vec![m.m_len().get() as usize];
    let mut n = m.m_next().get();
    while let Some(m) = n {
        v.push(m.m_len().get() as usize);
        n = m.m_next().get();
    }
    v
}

/// The data of a whole chain.
pub(crate) fn bytes(m: &Mbuf) -> Vec<u8> {
    let mut v = vec![0; lens(m).iter().sum()];
    m_copydata(m, 0, &mut v);
    v
}

#[test]
fn disct_makes_straddling_bytes_contiguous() {
    let _g = setup();
    let head = chain(&[&[1, 2, 3], &[4, 5, 6, 7, 8], &[9, 10]]);
    let mut md = Some(head);
    let mut dpos = mtod::<u8>(head).wrapping_add(1);

    let v = nfsm_disct(&mut md, &mut dpos, 6).expect("six bytes");
    assert_eq!(v.bytes(), &[2, 3, 4, 5, 6, 7]);
    // The first mbuf lost its two bytes to a new one, the second its first four.
    assert_eq!(lens(head), [1, 6, 1, 2]);
    assert_eq!(bytes(head), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    assert_eq!(nfsm_avail(md.expect("cursor"), dpos), 1);

    let v = nfsm_disct(&mut md, &mut dpos, 3).expect("three bytes");
    assert_eq!(v.bytes(), &[8, 9, 10]);
    assert_eq!(
        nfsm_disct(&mut md, &mut dpos, 1).err(),
        Some(Errno::EBADRPC)
    );
    m_freem(head);
}

#[test]
fn disct_in_one_mbuf_and_past_empty_ones() {
    let _g = setup();
    let head = chain(&[&[1, 2], &[], &[3, 4, 5, 6]]);
    let mut md = Some(head);
    let mut dpos = mtod::<u8>(head).wrapping_add(2);
    let v = nfsm_disct(&mut md, &mut dpos, 4).expect("four bytes");
    assert_eq!(v.get(0), u32::from_ne_bytes([3, 4, 5, 6]));
    assert_eq!(
        lens(head),
        [2, 0, 4],
        "no copy when the bytes are contiguous"
    );
    // A short chain is EBADRPC, not a read past it.
    let mut md = Some(head);
    let mut dpos = mtod::<u8>(head);
    assert_eq!(
        nfsm_disct(&mut md, &mut dpos, 7).err(),
        Some(Errno::EBADRPC)
    );
    m_freem(head);
}

#[test]
fn adv_crosses_mbufs() {
    let _g = setup();
    let head = chain(&[&[1, 2], &[3, 4, 5], &[6]]);
    let mut md = Some(head);
    let mut dpos = mtod::<u8>(head);
    nfs_adv(&mut md, &mut dpos, 4).expect("advance");
    let second = head.m_next().get().expect("second mbuf");
    assert!(core::ptr::eq(md.expect("cursor"), second));
    assert_eq!(nfsm_avail(second, dpos), 1);
    let v = nfsm_disct(&mut md, &mut dpos, 2).expect("two bytes");
    assert_eq!(v.bytes(), &[5, 6]);
    assert_eq!(nfs_adv(&mut md, &mut dpos, 1).err(), Some(Errno::EBADRPC));
    m_freem(head);
}

#[test]
fn nextbytes_walks_mbuf_by_mbuf() {
    let _g = setup();
    let head = chain(&[&[1, 2], &[], &[3, 4, 5]]);
    let mut md = Some(head);
    let mut dpos = mtod::<u8>(head).wrapping_add(1);
    let mut name = Vec::new();
    while name.len() < 4 {
        let v = nfsm_nextbytes(&mut md, &mut dpos, 4 - name.len()).expect("more bytes");
        name.extend_from_slice(v.bytes());
    }
    assert_eq!(name, [2, 3, 4, 5]);
    assert_eq!(
        nfsm_nextbytes(&mut md, &mut dpos, 1).err(),
        Some(Errno::EBADRPC)
    );
    m_freem(head);
}

#[test]
fn build_grows_the_chain() {
    let _g = setup();
    let head = nfsm_reqhead(0);
    let mut mb = head;
    let mut tl = nfsm_build(&mut mb, 200);
    tl.put(txdr_unsigned(7));
    tl.put_hyper(0x0102_0304_0506_0708);
    assert!(core::ptr::eq(mb, head));
    let mut tl = nfsm_build(&mut mb, 40);
    tl.set(9, nfs_true);
    assert!(!core::ptr::eq(mb, head), "a new mbuf for what does not fit");
    assert_eq!(lens(head), [200, 40]);
    let b = bytes(head);
    assert_eq!(b[..12], [0, 0, 0, 7, 1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(b[236..240], [0, 0, 0, 1]);
    m_freem(head);
}

#[test]
fn strings_are_counted_and_padded() {
    let _g = setup();
    let head = nfsm_reqhead(0);
    let mut mb = head;
    nfsm_strtombuf(&mut mb, b"hello");
    assert_eq!(
        bytes(head),
        [0, 0, 0, 5, b'h', b'e', b'l', b'l', b'o', 0, 0, 0]
    );
    nfsm_buftombuf(&mut mb, &[9; 4]);
    assert_eq!(lens(head), [16]);
    m_freem(head);
}

#[test]
fn uio_round_trip_across_mbufs() {
    let _g = setup();
    let data: Vec<u8> = (0..601).map(|i| (i * 7 + 3) as u8).collect();
    let head = nfsm_reqhead(0);
    let mut mb = head;
    nfsm_buftombuf(&mut mb, &data);
    let mut tl = nfsm_build(&mut mb, 4);
    tl.put(nfs_false);
    // 601 bytes and 3 of padding, then the word.
    assert_eq!(lens(head).iter().sum::<usize>(), 608);
    assert!(lens(head).len() > 1);

    // Read it back in two iovecs.
    let mut a = vec![0u8; 100];
    let mut b = vec![0u8; 600];
    let mut iov = [
        Iovec {
            iov_base: a.as_mut_ptr().cast(),
            iov_len: a.len(),
        },
        Iovec {
            iov_base: b.as_mut_ptr().cast(),
            iov_len: b.len(),
        },
    ];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: 601,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    let mut md = Some(head);
    let mut dpos = mtod::<u8>(head);
    nfsm_mbuftouio(&mut md, &mut uio, 601, &mut dpos).expect("copy out");
    assert_eq!(uio.uio_resid, 0);
    assert_eq!(uio.uio_offset, 601);
    assert_eq!(
        uio.uio_iovcnt(),
        1,
        "the first iovec consumed, the second partly"
    );
    assert_eq!(uio.uio_iov[0].iov_len, 99);
    assert_eq!(a[..], data[..100]);
    assert_eq!(b[..501], data[100..]);
    // The padding was skipped: the cursor is at the last word.
    let v = nfsm_disct(&mut md, &mut dpos, 4).expect("the last word");
    assert_eq!(v.get(0), nfs_false);
    m_freem(head);
}

#[test]
fn mbuftouio_needs_iovecs() {
    let _g = setup();
    let head = chain(&[&[1, 2, 3, 4]]);
    let mut iov: [Iovec; 0] = [];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: 4,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    let mut md = Some(head);
    let mut dpos = mtod::<u8>(head);
    assert_eq!(
        nfsm_mbuftouio(&mut md, &mut uio, 4, &mut dpos).err(),
        Some(Errno::EFBIG)
    );
    m_freem(head);
}

#[test]
fn file_handles() {
    let _g = setup();
    let mut fh = Nfsfh::new();
    fh.fh_bytes[0] = 0xab;
    fh.fh_bytes[NFSX_V2FH - 1] = 0xcd;
    let head = nfsm_reqhead(0);
    let mut mb = head;
    nfsm_srvfhtom(&mut mb, &fh, false);
    nfsm_srvfhtom(&mut mb, &fh, true);
    let b = bytes(head);
    assert_eq!(b.len(), NFSX_V2FH + 4 + NFSX_V3FH);
    assert_eq!(b[0], 0xab);
    assert_eq!(b[NFSX_V2FH - 1], 0xcd);
    assert_eq!(
        b[NFSX_V2FH..NFSX_V2FH + 4],
        (NFSX_V3FH as u32).to_be_bytes()
    );
    m_freem(head);
}

#[test]
fn version2_times() {
    let none = txdr_nfsv2time(&Timespec::new(5, i64::from(VNOVAL)));
    assert_eq!((none.nfsv2_sec, none.nfsv2_usec), (u32::MAX, u32::MAX));
    let minus = txdr_nfsv2time(&Timespec::new(-1, 0));
    assert_eq!(fxdr_unsigned(minus.nfsv2_sec) as i32, -2);
    assert_eq!(fxdr_unsigned(minus.nfsv2_usec), 999_999);
    let t = txdr_nfsv2time(&Timespec::new(100, 5_000_999));
    assert_eq!(
        crate::nfs::xdr_subs::fxdr_nfsv2time(&t),
        Timespec::new(100, 5_000_000)
    );
}

#[test]
fn tables() {
    assert_eq!(NFSV3_PROCID[NFSV2PROC_STATFS], NFSPROC_FSSTAT);
    assert_eq!(NFSV2_PROCID[NFSPROC_MKNOD_FOR_TEST], NFSV2PROC_CREATE);
    for (v2, &generic) in NFSV3_PROCID.iter().enumerate() {
        if generic != NFSPROC_NOOP {
            assert_eq!(NFSV2_PROCID[generic], v2, "procedure {generic}");
        }
    }
    assert_eq!(
        NFSRV_V2ERRMAP[Errno::ESTALE as usize - 1],
        NFSERR_STALE as u8
    );
    assert_eq!(
        NFSRV_V2ERRMAP[Errno::EACCES as usize - 1],
        NFSERR_ACCES as u8
    );
    for (p, l) in NFSRV_V3ERRMAP.iter().enumerate() {
        assert_eq!(l.last(), Some(&0), "procedure {p} ends with 0");
    }
}

/// `NFSPROC_MKNOD`, which version 2 maps to `NFSV2PROC_CREATE`.
const NFSPROC_MKNOD_FOR_TEST: usize = crate::nfs::nfsproto::NFSPROC_MKNOD;

/// A leaked NFS mount (named "nfs", over the test file system's operations), and a vnode of
/// it with its node.
fn nfs_vnode(vtype: Vtype) -> (&'static Vnode, &'static NfsNode, &'static NfsMount) {
    static CONF: Vfsconf = Vfsconf::new(
        &crate::kern::vfs_subr::tests::testfs::TESTFS_VFSOPS,
        b"nfs",
        2,
        0,
        0,
    );
    let nmp: &'static NfsMount = Box::leak(Box::new(NfsMount::new()));
    let mp: &'static Mount = Box::leak(Box::new(Mount::new()));
    mp.mnt_vfc.set(Some(&CONF));
    mp.mnt_data.set(ptr::from_ref(nmp).cast_mut().cast());
    let np: &'static NfsNode = Box::leak(Box::new(NfsNode::new()));
    let vp: &'static Vnode = Box::leak(Box::new(Vnode::new()));
    vp.v_tag.set(VT_NFS);
    vp.v_type.set(vtype);
    vp.v_mount.set(Some(mp));
    vp.v_data.set(ptr::from_ref(np).cast_mut().cast());
    np.n_vnode.set(Some(vp));
    (vp, np, nmp)
}

#[test]
fn errors_are_mapped_per_version_and_procedure() {
    let mut nd = NfsrvDescript::new();
    assert_eq!(nfsrv_errmap(&nd, Errno::ESTALE as i32), NFSERR_STALE);
    assert_eq!(nfsrv_errmap(&nd, Errno::ENOTSUP as i32), NFSERR_IO);
    assert_eq!(nfsrv_errmap(&nd, 0), NFSERR_IO);
    nd.nd_flag = ND_NFSV3;
    nd.nd_procnum = NFSPROC_LOOKUP;
    assert_eq!(nfsrv_errmap(&nd, NFSERR_NOENT), NFSERR_NOENT);
    assert_eq!(nfsrv_errmap(&nd, NFSERR_NOTDIR), NFSERR_NOTDIR);
    assert_eq!(
        nfsrv_errmap(&nd, NFSERR_PERM),
        NFSERR_IO,
        "not a LOOKUP error"
    );
    nd.nd_procnum = NFSPROC_GETATTR;
    assert_eq!(nfsrv_errmap(&nd, NFSERR_NOENT), NFSERR_IO);
    nd.nd_procnum = NFSPROC_NOOP;
    assert_eq!(
        nfsrv_errmap(&nd, 0x1_0000 | NFSERR_BADHANDLE),
        NFSERR_BADHANDLE
    );
}

#[test]
fn commit_ranges() {
    let (vp, np, _) = nfs_vnode(VREG);
    let bp = Buf::new();
    bp.b_blkno.set(2);
    bp.b_dirtyend.set(100);
    let lo = 2 * DEV_BSIZE as Off;
    assert!(!nfs_in_tobecommitted_range(vp, &bp));
    nfs_add_tobecommitted_range(vp, &bp);
    assert!(nfs_in_tobecommitted_range(vp, &bp));
    assert_eq!((np.n_pushlo.get(), np.n_pushhi.get()), (lo, lo + 100));
    nfs_merge_commit_ranges(vp);
    assert!(nfs_in_committed_range(vp, &bp));
    assert!(!nfs_in_tobecommitted_range(vp, &bp));

    // A second buffer further on widens the committed range; deleting the first leaves the
    // second's part.
    let bp2 = Buf::new();
    bp2.b_blkno.set(10);
    bp2.b_dirtyend.set(512);
    nfs_add_committed_range(vp, &bp2);
    assert_eq!(np.n_pushedhi.get(), 10 * DEV_BSIZE as Off + 512);
    nfs_del_committed_range(vp, &bp);
    assert_eq!(np.n_pushedlo.get(), lo + 100);
    assert!(nfs_in_committed_range(vp, &bp2));
    assert!(!nfs_in_committed_range(vp, &bp));

    nfs_add_tobecommitted_range(vp, &bp2);
    nfs_del_tobecommitted_range(vp, &bp2);
    assert_eq!(np.n_pushlo.get(), 10 * DEV_BSIZE as Off + 512);
}

#[test]
fn attribute_cache_timeouts_and_hits() {
    let (vp, np, nmp) = nfs_vnode(VREG);
    nmp.nm_acregmin.set(3);
    nmp.nm_acregmax.set(60);
    np.n_mtime.set(Timespec::new(gettime(), 0));
    assert_eq!(nfs_attrtimeo(np), 3);
    np.n_mtime.set(Timespec::new(gettime() - 200, 0));
    assert_eq!(nfs_attrtimeo(np), 20);
    np.n_mtime.set(Timespec::new(gettime() - 10_000, 0));
    assert_eq!(nfs_attrtimeo(np), 60);

    let mut va = Vattr::new();
    assert_eq!(nfs_getattrcache(vp, &mut va).err(), Some(Errno::ENOENT));
    let mut cached = Vattr::new();
    cached.va_type = VREG;
    cached.va_mode = 0o644;
    np.n_vattr.set(cached);
    np.n_attrstamp.set(gettime() - 1);
    np.n_atim.set(Timespec::new(7, 8));
    np.n_flag
        .set(crate::nfs::nfsnode::NCHG | crate::nfs::nfsnode::NACC);
    nfs_getattrcache(vp, &mut va).expect("a hit");
    assert_eq!(va.va_mode, 0o644);
    assert_eq!(va.va_atime, Timespec::new(7, 8));
}

#[test]
fn rpc_header_with_unix_credentials() {
    let _g = setup();
    let nmp: &'static NfsMount = Box::leak(Box::new(NfsMount::new()));
    nmp.nm_flag.set(NFSMNT_NFSV3);
    nmp.nm_numgrps.set(16);
    let req = NfsReq::new();
    let m = m_gethdr(M_WAIT, MT_DATA).expect("a header mbuf");
    req.r_mreq.set(Some(m));
    req.r_nmp.set(Some(nmp));
    req.r_procnum.set(NFSPROC_GETATTR);
    let cr = Ucred::new();
    cr.cr_uid.set(1000);
    cr.cr_gid.set(10);
    cr.cr_ngroups.set(2);
    cr.cr_groups[0].set(10);
    cr.cr_groups[1].set(20);

    nfsm_rpchead(&req, &cr, RPCAUTH_UNIX);

    let b = bytes(m);
    assert_eq!(b.len(), 68, "10 words and a 28-byte credential");
    assert_eq!(m.m_pkthdr().len.get(), 68);
    let w: Vec<u32> = b
        .chunks(4)
        .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    assert_ne!(w[0], 0);
    assert_eq!(req.r_xid.get(), txdr_unsigned(w[0]));
    assert_eq!(
        w[1..],
        [0, 2, 100003, 3, 1, 1, 28, 0, 0, 1000, 10, 2, 10, 20, 0, 0]
    );
    // Version 2 maps the procedure number.
    nmp.nm_flag.set(0);
    let m2 = m_gethdr(M_WAIT, MT_DATA).expect("a header mbuf");
    req.r_mreq.set(Some(m2));
    req.r_procnum.set(NFSPROC_LOOKUP);
    nfsm_rpchead(&req, &cr, RPCAUTH_UNIX);
    let b = bytes(m2);
    assert_eq!(b[16..24], [0, 0, 0, 2, 0, 0, 0, 4]);
    m_freem(m);
    m_freem(m2);
}

#[test]
fn v3_sattr() {
    let _g = setup();
    let mut a = Vattr::new();
    crate::kern::vfs_subr::vattr_null(&mut a);
    a.va_mode = 0o600;
    a.va_size = 5;
    a.va_mtime = Timespec::new(gettime() + 100, 9);
    let head = nfsm_reqhead(0);
    let mut mb = head;
    nfsm_v3attrbuild(&mut mb, &a, true);
    let w: Vec<u32> = bytes(head)
        .chunks(4)
        .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let t = (gettime() + 100) as u32;
    assert_eq!(w, [1, 0o600, 0, 0, 1, 0, 5, 0, 2, t, 9]);
    // Not full: only the mode and the times.
    let head2 = nfsm_reqhead(0);
    let mut mb = head2;
    a.va_mtime = Timespec::new(gettime(), 0);
    nfsm_v3attrbuild(&mut mb, &a, false);
    assert_eq!(
        bytes(head2)[8..],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]
    );
    m_freem(head);
    m_freem(head2);
}

#[test]
fn xids_are_nonzero_and_vary() {
    let a = nfs_get_xid();
    let b = nfs_get_xid();
    assert_ne!(a, 0);
    assert_ne!(a, b);
}
