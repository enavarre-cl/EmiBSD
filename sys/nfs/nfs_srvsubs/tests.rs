//! Host tests for `nfs_srvsubs.c`: `nfsm_adj`, the reply builders (`nfsm_srvwcc`,
//! `nfsm_srvpostop_attr`, `nfsm_srvfattr`), `nfsm_srvsattr`, `netaddr_match`, and
//! `nfs_namei`/`nfsrv_fhtovp` against a real exported ffs mount.

use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::tests::setup as mbuf_setup;
use crate::kern::uipc_mbuf::{m_freem, m_get};
use crate::nfs::nfs_subs::nfsm_reqhead;
use crate::nfs::nfs_subs::tests::{bytes, chain, lens};
use crate::nfs::nfsm_subs::nfsm_avail;
use crate::nfs::nfsproto::NFSV3SATTRTIME_DONTCHANGE;
use crate::nfs::xdr_subs::{fxdr_hyper, xdr_bytes};
use crate::sys::mbuf::{M_DONTWAIT, MT_SONAME};
use crate::sys::types::makedev;
use crate::sys::vnode::{VCHR, VDIR, VLNK, VREG, Vtype};

/// The words of `b` (a reply), as raw XDR words in network order.
fn words(b: &[u8]) -> Vec<u32> {
    b.chunks(4)
        .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// The value of the raw word `w`.
fn v(w: u32) -> u32 {
    fxdr_unsigned(w)
}

/// A request descriptor positioned at the start of `m`.
fn request(m: &'static Mbuf, v3: bool) -> NfsrvDescript {
    let mut nd = NfsrvDescript::new();
    nd.nd_mrep = Some(m);
    nd.nd_md = Some(m);
    nd.nd_dpos = mtod::<u8>(m);
    if v3 {
        nd.nd_flag |= ND_NFSV3;
    }
    nd
}

/// An `MT_SONAME` mbuf holding the `AF_INET` address `a`, port `port` (host order).
fn nam(a: [u8; 4], port: u16) -> &'static Mbuf {
    let m = m_get(M_DONTWAIT, MT_SONAME).expect("an mbuf");
    let mut sin = [0u8; 16];
    sin[0] = 16;
    sin[1] = AF_INET;
    sin[2..4].copy_from_slice(&port.to_be_bytes());
    sin[4..8].copy_from_slice(&a);
    // SAFETY: an mbuf's data area holds 16 bytes; `mtod` points at it.
    unsafe { ptr::copy_nonoverlapping(sin.as_ptr(), mtod::<u8>(m), 16) };
    m.m_len().set(16);
    m
}

#[test]
fn adj_trims_inside_the_last_mbuf_and_zero_fills() {
    let _g = mbuf_setup();
    let head = chain(&[&[1, 2, 3, 4], &[5, 6, 7, 8, 9, 10]]);
    nfsm_adj(head, 2, 0);
    assert_eq!(lens(head), [4, 4]);
    assert_eq!(bytes(head), [1, 2, 3, 4, 5, 6, 7, 8]);
    // The last `nul` bytes that are left are zeroed.
    nfsm_adj(head, 1, 2);
    assert_eq!(bytes(head), [1, 2, 3, 4, 5, 0, 0]);
    // Nothing to fill.
    nfsm_adj(head, 1, -1);
    assert_eq!(bytes(head), [1, 2, 3, 4, 5, 0]);
    m_freem(head);
}

#[test]
fn adj_trims_across_mbufs() {
    let _g = mbuf_setup();
    let head = chain(&[&[1, 2, 3, 4], &[5, 6], &[7, 8, 9]]);
    // 9 bytes, trim 5: 4 are left, all in the first mbuf; the others are emptied.
    nfsm_adj(head, 5, 0);
    assert_eq!(lens(head), [4, 0, 0]);
    assert_eq!(bytes(head), [1, 2, 3, 4]);
    m_freem(head);

    let head = chain(&[&[1, 2, 3, 4], &[5, 6], &[7, 8, 9]]);
    // Trim 4: 5 are left, ending inside the second mbuf; the null fill lands in it.
    nfsm_adj(head, 4, 1);
    assert_eq!(lens(head), [4, 1, 0]);
    assert_eq!(bytes(head), [1, 2, 3, 4, 0]);
    m_freem(head);

    // Trimming everything or more leaves an empty chain.
    let head = chain(&[&[1, 2], &[3]]);
    nfsm_adj(head, 10, 2);
    assert_eq!(lens(head), [0, 0]);
    m_freem(head);

    // A fill larger than the data is clamped to it.
    let head = chain(&[&[1, 2, 3, 4]]);
    nfsm_adj(head, 2, 8);
    assert_eq!(bytes(head), [0, 0]);
    m_freem(head);
}

/// A `Vattr` with distinct values everywhere.
fn attrs(t: Vtype) -> Vattr {
    let mut va = Vattr::new();
    va.va_type = t;
    va.va_mode = 0o644;
    va.va_nlink = 2;
    va.va_uid = 3;
    va.va_gid = 4;
    va.va_fsid = 7;
    va.va_fileid = 0x1_0000_0063;
    va.va_size = 0x1_0000_0002;
    va.va_blocksize = 16384;
    va.va_atime = Timespec::new(100, 1_000);
    va.va_mtime = Timespec::new(200, 2_000);
    va.va_ctime = Timespec::new(300, 3_000);
    va.va_rdev = makedev(5, 6);
    va.va_bytes = 8192;
    va
}

#[test]
fn srvfattr_version_3() {
    let nd = NfsrvDescript {
        nd_flag: ND_NFSV3,
        ..NfsrvDescript::new()
    };
    let fp = nfsm_srvfattr(&nd, &attrs(VCHR));
    assert_eq!(v(fp.fa_type), 4, "NFCHR");
    assert_eq!(v(fp.fa_mode), 0o644);
    assert_eq!((v(fp.fa_nlink), v(fp.fa_uid), v(fp.fa_gid)), (2, 3, 4));
    assert_eq!(fxdr_hyper(fp.fa3_size().words()), 0x1_0000_0002);
    assert_eq!(fxdr_hyper(fp.fa3_used().words()), 8192);
    assert_eq!(
        (v(fp.fa3_rdev().specdata1), v(fp.fa3_rdev().specdata2)),
        (5, 6)
    );
    assert_eq!(fp.fa3_fsid().words(), [0, txdr_unsigned(7)]);
    assert_eq!(fxdr_hyper(fp.fa3_fileid().words()), 0x1_0000_0063);
    let (a, m, c) = (fp.fa3_atime(), fp.fa3_mtime(), fp.fa3_ctime());
    assert_eq!((v(a.nfsv3_sec), v(a.nfsv3_nsec)), (100, 1_000));
    assert_eq!((v(m.nfsv3_sec), v(m.nfsv3_nsec)), (200, 2_000));
    assert_eq!((v(c.nfsv3_sec), v(c.nfsv3_nsec)), (300, 3_000));
}

#[test]
fn srvfattr_version_2() {
    let nd = NfsrvDescript::new();
    let fp = nfsm_srvfattr(&nd, &attrs(VREG));
    assert_eq!(v(fp.fa_type), 1, "NFREG");
    assert_eq!(v(fp.fa_mode), 0o100644, "a version 2 mode carries the type");
    assert_eq!(v(fp.fa2_size()), 2, "the size is truncated to 32 bits");
    assert_eq!(v(fp.fa2_blocksize()), 16384);
    assert_eq!(v(fp.fa2_rdev()), makedev(5, 6) as u32);
    assert_eq!(v(fp.fa2_blocks()), 8192 / 512);
    assert_eq!((v(fp.fa2_fsid()), v(fp.fa2_fileid())), (7, 0x63));
    assert_eq!(v(fp.fa2_mtime().nfsv2_sec), 200);
    assert_eq!(v(fp.fa2_mtime().nfsv2_usec), 2);
    // A fifo's rdev is all ones.
    let fp = nfsm_srvfattr(&nd, &attrs(VFIFO));
    assert_eq!(fp.fa2_rdev(), 0xffff_ffff);
    assert_eq!(v(fp.fa_type), 4, "a version 2 fifo is NFCHR");
}

#[test]
fn srvpostop_attr_and_wcc_layouts() {
    let _g = mbuf_setup();
    let nd = NfsrvDescript {
        nd_flag: ND_NFSV3,
        ..NfsrvDescript::new()
    };

    // No attributes: a false word.
    let head = nfsm_reqhead(0);
    let mut mb = head;
    nfsm_srvpostop_attr(&nd, None, &mut mb);
    assert_eq!(words(&bytes(head)), [nfs_false]);
    m_freem(head);

    // Attributes: a true word and the 84-byte fattr.
    let head = nfsm_reqhead(0);
    let mut mb = head;
    let va = attrs(VREG);
    nfsm_srvpostop_attr(&nd, Some(&va), &mut mb);
    let b = bytes(head);
    assert_eq!(b.len(), NFSX_UNSIGNED + NFSX_V3FATTR);
    assert_eq!(words(&b)[0], nfs_true);
    assert_eq!(&b[4..], xdr_bytes(&nfsm_srvfattr(&nd, &va)));
    m_freem(head);

    // wcc_data with both halves: 7 words of pre-op attributes, then the post-op attributes.
    let head = nfsm_reqhead(0);
    let mut mb = head;
    let mut before = Vattr::new();
    before.va_size = 0x5_0000_0006;
    before.va_mtime = Timespec::new(11, 12);
    before.va_ctime = Timespec::new(13, 14);
    nfsm_srvwcc(&nd, Some(&before), Some(&va), &mut mb);
    let b = bytes(head);
    assert_eq!(b.len(), 7 * 4 + NFSX_UNSIGNED + NFSX_V3FATTR);
    let w = words(&b);
    assert_eq!(w[0], nfs_true);
    assert_eq!(fxdr_hyper([w[1], w[2]]), 0x5_0000_0006);
    assert_eq!([v(w[3]), v(w[4]), v(w[5]), v(w[6])], [11, 12, 13, 14]);
    assert_eq!(w[7], nfs_true);
    m_freem(head);

    // Neither: two false words.
    let head = nfsm_reqhead(0);
    let mut mb = head;
    nfsm_srvwcc(&nd, None, None, &mut mb);
    assert_eq!(words(&bytes(head)), [nfs_false, nfs_false]);
    m_freem(head);
}

/// An `sattr3` request: every combination of the optional members.
#[test]
fn srvsattr_decodes_every_member() {
    let _g = mbuf_setup();
    let t = txdr_unsigned;
    let mut b: Vec<u8> = Vec::new();
    for w in [
        nfs_true,
        t(0o10640), // mode (the type bits are masked off)
        nfs_false,  // uid not set
        nfs_true,
        t(5), // gid
        nfs_true,
        t(1),
        t(0x2_0000), // size 0x1_0002_0000: high word 1, low word 0x20000
        t(NFSV3SATTRTIME_TOCLIENT),
        t(100),
        t(5), // atime to the client's value
        t(NFSV3SATTRTIME_TOSERVER),
    ] {
        b.extend_from_slice(&w.to_ne_bytes());
    }
    // Split in the middle of a word and of a time to exercise the straddling dissects.
    let (a, c) = b.split_at(30);
    let head = chain(&[a, c]);
    let mut nd = request(head, true);
    let mut va = Vattr::new();
    va.va_vaflags = VA_UTIMES_NULL;
    va.va_uid = 77;
    nfsm_srvsattr(&mut nd, &mut va).expect("decoded");
    assert_eq!(va.va_mode, 0o640);
    assert_eq!(va.va_uid, 77, "left alone");
    assert_eq!(va.va_gid, 5);
    assert_eq!(va.va_size, 0x1_0002_0000);
    assert_eq!(va.va_atime, Timespec::new(100, 5));
    assert_eq!(va.va_vaflags, VA_UTIMES_CHANGE, "NULL cleared, CHANGE set");
    assert!(nd.nd_mrep.is_some());
    m_freem(head);

    // DONTCHANGE for both times: nothing is touched.
    let mut b: Vec<u8> = Vec::new();
    for w in [nfs_false, nfs_false, nfs_false, nfs_false] {
        b.extend_from_slice(&w.to_ne_bytes());
    }
    for _ in 0..2 {
        b.extend_from_slice(&txdr_unsigned(NFSV3SATTRTIME_DONTCHANGE).to_ne_bytes());
    }
    let head = chain(&[&b]);
    let mut nd = request(head, true);
    let mut va = Vattr::new();
    va.va_vaflags = VA_UTIMES_NULL;
    nfsm_srvsattr(&mut nd, &mut va).expect("decoded");
    assert_eq!(va.va_vaflags, VA_UTIMES_NULL);
    assert_eq!(va.va_atime, Timespec::new(0, 0));
    m_freem(head);
}

#[test]
fn srvsattr_frees_a_short_request() {
    let _g = mbuf_setup();
    let t = txdr_unsigned;
    let mut b: Vec<u8> = Vec::new();
    for w in [nfs_true, t(0o644), nfs_false] {
        b.extend_from_slice(&w.to_ne_bytes());
    }
    let head = chain(&[&b]);
    let mut nd = request(head, true);
    let mut va = Vattr::new();
    assert_eq!(nfsm_srvsattr(&mut nd, &mut va), Err(Errno::EBADRPC));
    assert!(nd.nd_mrep.is_none(), "the request is freed");
    assert_eq!(
        va.va_mode, 0o644,
        "what was decoded before the failure stays"
    );
}

#[test]
fn netaddr_match_compares_inet_addresses() {
    let _g = mbuf_setup();
    let m = nam([10, 0, 2, 9], 700);
    let host = |a: [u8; 4]| Nethostaddr {
        had_inetaddr: u32::from_ne_bytes(a),
        had_nam: None,
    };
    assert!(netaddr_match(AF_INET, &host([10, 0, 2, 9]), m));
    assert!(!netaddr_match(AF_INET, &host([10, 0, 2, 8]), m));
    // Any other family is "if there is any doubt, 0".
    assert!(!netaddr_match(0, &host([10, 0, 2, 9]), m));
    assert!(!netaddr_match(24, &host([10, 0, 2, 9]), m));
    // An address mbuf of another family never matches.
    // SAFETY: byte 1 of the address mbuf is `sin_family`.
    unsafe { mtod::<u8>(m).add(1).write(24) };
    assert!(!netaddr_match(AF_INET, &host([10, 0, 2, 9]), m));
    m_freem(m);
}

/// `nfs_namei` and `nfsrv_fhtovp` over an exported ffs: the name is copied out of a request
/// chain (across mbufs), the directory comes from the file handle, the lookup stays inside
/// it, the export maps root to the anonymous user, and every refusal has the C's error.
#[test]
fn namei_and_fhtovp_over_an_exported_ffs() {
    use crate::kern::kern_descrip::sys_close;
    use crate::kern::uipc_mbuf::tests::mbinit_again;
    use crate::kern::vfs_lookup::{namei, ndinit};
    use crate::kern::vfs_subr::tests::exports::{args, sin};
    use crate::kern::vfs_syscalls::{sys_mkdir, sys_mount, sys_open, sys_symlink};
    use crate::sys::fcntl::{O_CREAT, O_RDWR};
    use crate::sys::mount::{MNT_EXPORTED, MNT_EXRDONLY, MNT_UPDATE, UfsArgs, VFS_VPTOFH};
    use crate::sys::namei::{FOLLOW, LOCKLEAF, LOOKUP, NiDirp};
    use crate::ufs::ffs::ffs_vfsops::tests::{
        mount_root, newfs, path, setup, sys, teardown, unmount_root,
    };

    let img = newfs::Image::new(newfs::FFS2_4M);
    let (_g, p) = setup(img.finish());
    mbinit_again();
    let mp = mount_root(p, false);

    sys(sys_mkdir, p, &[path(b"/dir\0"), 0o755]).unwrap();
    let fd = sys(
        sys_open,
        p,
        &[path(b"/dir/f\0"), (O_CREAT | O_RDWR) as usize, 0o644],
    )
    .unwrap();
    sys(sys_close, p, &[fd as usize]).unwrap();
    let fd = sys(
        sys_open,
        p,
        &[path(b"/file\0"), (O_CREAT | O_RDWR) as usize, 0o644],
    )
    .unwrap();
    sys(sys_close, p, &[fd as usize]).unwrap();
    sys(sys_symlink, p, &[path(b"f\0"), path(b"/dir/ln\0")]).unwrap();

    // The file handle of `name`.
    let fh = |name: &'static [u8]| -> Fhandle {
        let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(name), p);
        namei(&mut nd).unwrap();
        let vp = nd.ni_vp.unwrap();
        let mut fid = Default::default();
        VFS_VPTOFH(vp, &mut fid).unwrap();
        vrele(vp);
        Fhandle {
            fh_fsid: mp.mnt_stat.get().f_fsid,
            fh_fid: fid,
        }
    };
    let dirfh = fh(b"/dir");
    let filefh = fh(b"/file");

    // mountd(8): export it read-only to 10.0.2.0/24, everyone else as user 32767.
    let net = sin(2, [10, 0, 2, 0]);
    let mask = sin(2, [255, 255, 255, 0]);
    let mut ua = UfsArgs {
        fspec: 0,
        export_info: args(MNT_EXPORTED | MNT_EXRDONLY, 32767, Some(net), Some(mask)),
    };
    sys(
        sys_mount,
        p,
        &[
            b"ffs\0".as_ptr() as usize,
            path(b"/\0"),
            MNT_UPDATE as usize,
            ptr::from_mut(&mut ua) as usize,
        ],
    )
    .unwrap();

    let slp = NfssvcSock::new();
    let client = nam([10, 0, 2, 9], 700);
    let cred = nd_cred_root();
    let weak = AUTH_TOOWEAK as i32;

    // Looks `name` up in `fh` as the client at `from`, the name in `parts` (a request chain).
    let run = |fhp: &Fhandle,
               name_len: usize,
               parts: &[&[u8]],
               from: &Mbuf,
               flags: u64|
     -> (Result<(), i32>, Nameidata<'static>, Option<&'static Vnode>) {
        let head = chain(parts);
        let mut md = Some(head);
        let mut dpos = mtod::<u8>(head);
        let mut nd = ndinit(LOOKUP, flags, NiDirp::Sys(b""), p);
        nd.ni_cnd.cn_cred = cred;
        let mut retdir = None;
        let r = nfs_namei(
            &mut nd,
            fhp,
            name_len,
            &slp,
            from,
            &mut md,
            &mut dpos,
            &mut retdir,
            p,
        );
        if r.is_ok() {
            // The cursor is past the name and its padding.
            let m = md.expect("cursor");
            assert_eq!(nfsm_avail(m, dpos), parts_after(name_len, m, head));
        }
        m_freem(head);
        (r, nd, retdir)
    };

    // The name starts in the second mbuf (the first is empty), its padding runs into the
    // third: the lookup finds /dir/f, does not cross mounts, is read-only, and root became
    // user 32767.
    assert_eq!(cred.cr_uid.get(), 0);
    let (r, nd, retdir) = run(&dirfh, 1, &[b"", b"f", b"\0\0\0z"], client, LOCKLEAF);
    assert_eq!(r, Ok(()));
    assert_eq!(cred.cr_uid.get(), 32767);
    assert_eq!(cred.cr_gid.get(), 32767);
    assert_eq!(cred.cr_ngroups.get(), 0);
    let vp = nd.ni_vp.expect("the file");
    assert_eq!(vp.v_type.get(), VREG);
    assert!(nd.ni_cnd.cn_flags & (NOCROSSMOUNT | RDONLY) == (NOCROSSMOUNT | RDONLY));
    assert!(
        nd.ni_cnd.cn_pnbuf.is_null(),
        "the buffer went back to the pool"
    );
    let dir = retdir.expect("the directory");
    assert_eq!(dir.v_type.get(), VDIR);
    vput(vp);
    vrele(dir);

    // A name copied in two chunks, across mbufs: "ln" is a symbolic link. Without FOLLOW
    // (what the server's lookups ask for) the lookup returns the link itself ...
    cred.cr_uid.set(0);
    let (r, nd, retdir) = run(&dirfh, 2, &[b"l", b"n\0\0"], client, LOCKLEAF);
    assert_eq!(r, Ok(()));
    let vp = nd.ni_vp.expect("the link");
    assert_eq!(vp.v_type.get(), VLNK);
    vput(vp);
    vrele(retdir.expect("the directory"));
    // ... and a request that would follow it is EINVAL, nothing left locked or referenced.
    let (r, nd, retdir) = run(&dirfh, 2, &[b"l", b"n\0\0"], client, LOCKLEAF | FOLLOW);
    assert_eq!(r, Err(Errno::EINVAL.as_i32()));
    assert!(nd.ni_vp.is_none());
    vrele(retdir.expect("the directory is still the caller's"));

    // A NUL or a slash in the name is EACCES; the directory has not been looked up yet, so
    // retdir stays None.
    for bad in [&b"a/b\0"[..], &b"a\0b\0"[..]] {
        let (r, nd, retdir) = run(&dirfh, 3, &[bad], client, LOCKLEAF);
        assert_eq!(r, Err(Errno::EACCES.as_i32()));
        assert!(retdir.is_none());
        assert!(nd.ni_cnd.cn_pnbuf.is_null());
    }

    // A chain that ends inside the name: EBADRPC.
    let (r, _, retdir) = run(&dirfh, 8, &[b"ab", b"cd"], client, LOCKLEAF);
    assert_eq!(r, Err(Errno::EBADRPC.as_i32()));
    assert!(retdir.is_none());

    // A name that does not exist: ENOENT, the directory returned for the caller to release.
    let (r, _, retdir) = run(&dirfh, 3, &[b"abc\0"], client, LOCKLEAF);
    assert_eq!(r, Err(Errno::ENOENT.as_i32()));
    vrele(retdir.expect("the directory"));

    // The handle of a regular file is not a directory.
    let (r, _, retdir) = run(&filefh, 1, &[b"x\0\0\0"], client, LOCKLEAF);
    assert_eq!(r, Err(Errno::ENOTDIR.as_i32()));
    assert!(retdir.is_none());

    // A file system that is not mounted: ESTALE.
    let mut stale = dirfh;
    stale.fh_fsid.val[0] ^= 0x5a5a;
    let (r, _, _) = run(&stale, 1, &[b"f\0\0\0"], client, LOCKLEAF);
    assert_eq!(r, Err(Errno::ESTALE.as_i32()));

    // A client outside the export list: EACCES from VFS_CHECKEXP.
    let outsider = nam([10, 0, 3, 9], 700);
    let (r, _, _) = run(&dirfh, 1, &[b"f\0\0\0"], outsider, LOCKLEAF);
    assert_eq!(r, Err(Errno::EACCES.as_i32()));

    // A client on an unprivileged port, and on a stream socket from port 20.
    let high = nam([10, 0, 2, 9], 1024);
    let (r, _, _) = run(&dirfh, 1, &[b"f\0\0\0"], high, LOCKLEAF);
    assert_eq!(r, Err(NFSERR_AUTHERR | weak));
    // (A privileged port is the 700 of every success above.)
    let ftp = nam([10, 0, 2, 9], 20);
    let (r, nd, retdir) = run(&dirfh, 1, &[b"f\0\0\0"], ftp, LOCKLEAF);
    assert_eq!(r, Ok(()), "datagram sockets may use port 20");
    vput(nd.ni_vp.expect("the file"));
    vrele(retdir.expect("the directory"));

    // SAVENAME keeps the pathname buffer for the caller.
    let (r, nd, retdir) = run(&dirfh, 1, &[b"f\0\0\0"], client, LOCKLEAF | SAVENAME);
    assert_eq!(r, Ok(()));
    assert!(nd.ni_cnd.cn_flags & HASBUF != 0);
    assert!(!nd.ni_cnd.cn_pnbuf.is_null());
    // The name's length (no NUL, unlike namei's), less what the lookup consumed.
    assert_eq!(nd.ni_pathlen, 0);
    pool_put(
        &NAMEI_POOL,
        NonNull::new(nd.ni_cnd.cn_pnbuf).expect("a buffer"),
    );
    vput(nd.ni_vp.expect("the file"));
    vrele(retdir.expect("the directory"));

    // nfsrv_fhtovp alone: a non-root caller keeps its credentials; lockflag keeps the lock.
    let user = Ucred::new();
    user.cr_uid.set(1000);
    user.cr_gid.set(1000);
    let (vp, rdonly) = nfsrv_fhtovp(&filefh, true, &user, &slp, client).expect("a vnode");
    assert!(rdonly, "exported read-only");
    assert_eq!((user.cr_uid.get(), user.cr_gid.get()), (1000, 1000));
    vput(vp);
    let (vp, _) = nfsrv_fhtovp(&filefh, false, &user, &slp, client).expect("a vnode");
    vrele(vp);

    // A sockaddr shorter than a sockaddr_in is not vouched for.
    let short = nam([10, 0, 2, 9], 700);
    short.m_len().set(8);
    assert_eq!(
        nfsrv_fhtovp(&filefh, false, &user, &slp, short).err(),
        Some(NFSERR_AUTHERR | weak),
        "the export lookup reads the key from the mbuf's data area, the port check refuses"
    );

    mbinit_again();
    unmount_root(p, mp);
    teardown();
}

/// What is left in the mbuf `m` of the request `head` after the name and its padding: the
/// cursor is `name_len` rounded up to a word bytes from the start of the chain.
fn parts_after(name_len: usize, m: &Mbuf, head: &Mbuf) -> usize {
    let consumed = name_len + (4 - name_len % 4) % 4;
    // Offset of `m` from the start of the chain.
    let mut off = 0;
    let mut cur = Some(head);
    while let Some(c) = cur {
        if ptr::eq(c, m) {
            break;
        }
        off += c.m_len().get() as usize;
        cur = c.m_next().get();
    }
    off + m.m_len().get() as usize - consumed
}

/// Root's credentials for a request: uid 0, gid 0, no groups.
fn nd_cred_root() -> &'static Ucred {
    std::boxed::Box::leak(std::boxed::Box::new(Ucred::new()))
}
