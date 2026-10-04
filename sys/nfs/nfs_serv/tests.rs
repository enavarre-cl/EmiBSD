//! Host tests for `nfs_serv.c`: the readdir packing and size rules, the `struct flrep`
//! layout, the request checks every procedure starts with, the null and no-op procedures,
//! and every procedure of the table against a real exported ffs mount (requests and replies
//! in XDR, version 2 and 3, reference counts checked by a clean unmount at the end).

use core::mem::offset_of;
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::tests::setup as mbuf_setup;
use crate::nfs::nfs_subs::tests::{bytes, chain};
use crate::nfs::nfs_syscalls::NFSRV3_PROCS;
use crate::nfs::nfsproto::NFS_NPROCS;
use crate::nfs::nfsproto::{
    NFSERR_EXIST, NFSERR_NOENT, NFSPROC_GETATTR, NFSV3SATTRTIME_DONTCHANGE,
};
use crate::nfs::rpcv2::{RPC_GARBAGE, RPC_PROCUNAVAIL};
use crate::nfs::xdr_subs::xdr_get;

/// The raw XDR words of `b` as host values.
fn words(b: &[u8]) -> Vec<u32> {
    b.chunks(4)
        .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// The words of a reply chain, freed.
fn reply_words(m: &'static Mbuf) -> Vec<u32> {
    let w = words(&bytes(m));
    m_freem(m);
    w
}

/// An XDR request body.
#[derive(Default)]
struct Req(Vec<u8>);

impl Req {
    fn u(mut self, v: u32) -> Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }

    fn hyper(self, v: u64) -> Self {
        self.u((v >> 32) as u32).u(v as u32)
    }

    fn raw(mut self, b: &[u8]) -> Self {
        self.0.extend_from_slice(b);
        self
    }

    /// A file handle: version 3 `nfs_fh3` (length and bytes), version 2 32 bytes.
    fn fh(self, fh: &Fhandle, v3: bool) -> Self {
        let b = xdr_bytes(fh).to_vec();
        if v3 {
            self.u(NFSX_V3FH as u32).raw(&b)
        } else {
            self.raw(&b).raw(&[0; NFSX_V2FH - NFSX_V3FH])
        }
    }

    /// A string or opaque: length, bytes and padding.
    fn name(self, s: &[u8]) -> Self {
        let pad = nfsm_padlen(s.len());
        self.u(s.len() as u32).raw(s).raw(&[0; 3][..pad])
    }

    /// A version 3 `sattr3` that sets only the mode (or nothing).
    fn sattr3(self, mode: Option<u32>) -> Self {
        let s = match mode {
            Some(m) => self.u(1).u(m),
            None => self.u(0),
        };
        s.u(0)
            .u(0)
            .u(0)
            .u(NFSV3SATTRTIME_DONTCHANGE)
            .u(NFSV3SATTRTIME_DONTCHANGE)
    }

    /// A version 2 `sattr` that sets only the mode.
    fn sattr2(self, mode: u32) -> Self {
        let mut s = self.u(mode);
        for _ in 0..7 {
            s = s.u(u32::MAX);
        }
        s
    }
}

/// A request descriptor for `body`, split over mbufs of 50 bytes (so that words straddle
/// them), from `client`.
fn descript(body: &[u8], v3: bool, client: Option<&'static Mbuf>) -> NfsrvDescript {
    let parts: Vec<&[u8]> = if body.is_empty() {
        std::vec![&[][..]]
    } else {
        body.chunks(50).collect()
    };
    let head = chain(&parts);
    let mut nd = NfsrvDescript::new();
    nd.nd_mrep = Some(head);
    nd.nd_md = Some(head);
    nd.nd_dpos = mtod::<u8>(head);
    if v3 {
        nd.nd_flag |= ND_NFSV3;
    }
    nd.nd_nam = client;
    nd.nd_retxid = 0x1234;
    nd
}

/// A reply being read back.
struct Rd {
    w: Vec<u32>,
    i: usize,
}

impl Rd {
    /// The reply `w`: an accepted RPC reply whose `accept_stat` is `stat`.
    fn new(w: Vec<u32>, stat: u32) -> Rd {
        assert!(w.len() >= 6, "short reply {w:x?}");
        assert_eq!(&w[..6], &[0x1234, 1, 0, 0, 0, stat], "RPC reply header");
        Rd { w, i: 6 }
    }

    fn u(&mut self) -> u32 {
        self.i += 1;
        self.w[self.i - 1]
    }

    fn hyper(&mut self) -> u64 {
        (u64::from(self.u()) << 32) | u64::from(self.u())
    }

    fn n(&mut self, n: usize) -> Vec<u32> {
        self.i += n;
        self.w[self.i - n..self.i].to_vec()
    }

    /// A string or opaque.
    fn opaque(&mut self) -> Vec<u8> {
        let len = self.u() as usize;
        let n = nfsm_rndup(len) / 4;
        let mut b: Vec<u8> = self.n(n).iter().flat_map(|w| w.to_be_bytes()).collect();
        b.truncate(len);
        b
    }

    /// A `post_op_attr`: the 21 words of the attributes, if there.
    fn postop(&mut self) -> Option<Vec<u32>> {
        (self.u() == 1).then(|| self.n(NFSX_V3FATTR / 4))
    }

    /// A `wcc_data`: whether the pre-op attributes are there, and the post-op ones.
    fn wcc(&mut self) -> (bool, Option<Vec<u32>>) {
        let pre = self.u() == 1;
        if pre {
            self.n(6);
        }
        (pre, self.postop())
    }

    /// A `post_op_fh3`.
    fn postop_fh(&mut self) -> Option<Vec<u8>> {
        (self.u() == 1).then(|| self.opaque())
    }

    fn done(&self) {
        assert_eq!(self.i, self.w.len(), "the whole reply was read");
    }
}

/// Runs `f` on the request `body` and returns its result and the reply's words.
fn run(
    f: NfsrvProc,
    body: &[u8],
    v3: bool,
    client: Option<&'static Mbuf>,
    p: &Proc,
) -> (Result<(), Errno>, Option<Vec<u32>>) {
    let mut nd = descript(body, v3, client);
    // nfsrv_errmap picks the version 3 status by procedure.
    nd.nd_procnum = NFSRV3_PROCS
        .iter()
        .position(|&g| core::ptr::fn_addr_eq(g, f))
        .expect("a procedure of the table");
    let slp = NfssvcSock::new();
    let mut mrq = None;
    let r = f(&mut nd, &slp, p, &mut mrq);
    assert!(nd.nd_mrep.is_none(), "the request was freed");
    (r, mrq.map(reply_words))
}

/// A directory entry as `VOP_READDIR` writes it.
fn dirent(fileno: u64, off: i64, name: &[u8]) -> Vec<u8> {
    let reclen = (Dirent::NAME_OFFSET + name.len() + 1 + 7) & !7;
    let mut b = std::vec![0u8; reclen];
    b[offset_of!(Dirent, d_fileno)..][..8].copy_from_slice(&fileno.to_ne_bytes());
    b[offset_of!(Dirent, d_off)..][..8].copy_from_slice(&off.to_ne_bytes());
    b[offset_of!(Dirent, d_reclen)..][..2].copy_from_slice(&(reclen as u16).to_ne_bytes());
    b[offset_of!(Dirent, d_namlen)] = name.len() as u8;
    b[Dirent::NAME_OFFSET..][..name.len()].copy_from_slice(name);
    b
}

#[test]
fn readdir_packs_entries_with_a_file_number_while_they_fit() {
    let _g = mbuf_setup();
    let mut buf = dirent(5, 100, b"a");
    buf.extend(dirent(0, 200, b"gone"));
    buf.extend(dirent(7, 300, b"bcdef"));

    // Version 3: true, fileid (hyper), name, cookie (hyper).
    let m = m_get(M_WAIT, MT_DATA).expect("an mbuf");
    let mut mb = m;
    assert!(nfsrv_readdir_pack(&mut mb, &buf, true, 1000, 12));
    let w = reply_words(m);
    assert_eq!(
        w,
        [
            1,
            0,
            5,
            1,
            u32::from_be_bytes(*b"a\0\0\0"),
            0,
            100, //
            1,
            0,
            7,
            5,
            u32::from_be_bytes(*b"bcde"),
            u32::from_be_bytes(*b"f\0\0\0"),
            0,
            300,
        ]
    );

    // Version 2: true, fileid, name, cookie; the second entry does not fit the count.
    let m = m_get(M_WAIT, MT_DATA).expect("an mbuf");
    let mut mb = m;
    // 12 + (16 + 4) = 32 for "a", then 32 + (16 + 8) = 56 > 40.
    assert!(!nfsrv_readdir_pack(&mut mb, &buf, false, 40, 12));
    let w = reply_words(m);
    assert_eq!(w, [1, 5, 1, u32::from_be_bytes(*b"a\0\0\0"), 100]);

    // A truncated entry or one with no length ends the walk.
    let m = m_get(M_WAIT, MT_DATA).expect("an mbuf");
    let mut mb = m;
    let mut bad = dirent(9, 1, b"x");
    bad[offset_of!(Dirent, d_reclen)..][..2].copy_from_slice(&0u16.to_ne_bytes());
    assert!(nfsrv_readdir_pack(&mut mb, &bad, true, 1000, 12));
    assert!(reply_words(m).is_empty());
    assert_eq!(nfsrv_dirent_skip(&bad), bad.len());

    // The degenerate-case walk stops at the first entry with a file number.
    let mut skip = dirent(0, 1, b"x");
    let first = skip.len();
    skip.extend(dirent(3, 2, b"y"));
    assert_eq!(nfsrv_dirent_skip(&skip), first);
    assert_eq!(
        nfsrv_dirent_skip(&dirent(0, 1, b"x")),
        dirent(0, 1, b"x").len()
    );
}

#[test]
fn readdir_sizes_follow_the_c() {
    assert_eq!(nfsrv_readdir_cnt(100, 8192), 100);
    assert_eq!(nfsrv_readdir_cnt(-1, 8192), 8192);
    assert_eq!(nfsrv_readdir_cnt(9000, 8192), 8192);
    assert_eq!(nfsrv_readdir_siz(100, 8192), 512);
    assert_eq!(nfsrv_readdir_siz(512, 8192), 512);
    assert_eq!(nfsrv_readdir_siz(513, 8192), 1024);
    assert_eq!(nfsrv_readdir_siz(9000, 8192), 8192);
    assert_eq!(
        nfsrv_readdir_siz(i32::MAX, 8192),
        8192,
        "wraps negative: xfer"
    );
}

#[test]
fn flrep_is_cookie_attributes_and_handle() {
    let fattr = NfsFattr {
        fa_type: txdr_unsigned(1),
        ..NfsFattr::default()
    };
    let mut nfh = Nfsfh::new();
    nfh.fh_bytes[0] = 0xaa;
    nfh.fh_bytes[NFSX_V3FH - 1] = 0xbb;
    nfh.fh_bytes[NFSX_V3FH] = 0xcc; // past the handle: not copied
    let fl = nfsrv_flrep(0x1_0000_0002, &fattr, &nfh);
    assert_eq!(FLREP_SIZE, 33 * 4);
    let w = words(&fl);
    assert_eq!(&w[..4], &[1, 2, 1, 1], "cookie, post_op_attr true, fa_type");
    assert_eq!(
        &w[24..26],
        &[1, NFSX_V3FH as u32],
        "post_op_fh3 true, length"
    );
    assert_eq!(fl[26 * 4], 0xaa);
    assert_eq!(fl[FLREP_SIZE - 1], 0xbb);
}

#[test]
fn null_noop_and_bad_requests() {
    let (_g, p) = crate::kern::vfs_subr::tests::setup();
    crate::kern::uipc_mbuf::tests::mbinit_again();

    // NULL: an accepted reply with no status.
    let (r, w) = run(nfsrv_null, &[], true, None, p);
    assert_eq!(r, Ok(()));
    Rd::new(w.expect("a reply"), 0).done();

    // An obsolete procedure: PROC_UNAVAIL ...
    let (r, w) = run(nfsrv_noop, &[], false, None, p);
    assert_eq!(r, Ok(()));
    Rd::new(w.expect("a reply"), RPC_PROCUNAVAIL).done();
    // ... or the status nfs_getreq left.
    let mut nd = descript(&[], true, None);
    nd.nd_repstat = Errno::EBADRPC.as_i32();
    let slp = NfssvcSock::new();
    let mut mrq = None;
    assert_eq!(nfsrv_noop(&mut nd, &slp, p, &mut mrq), Ok(()));
    Rd::new(reply_words(mrq.expect("a reply")), RPC_GARBAGE).done();

    // A version 3 file handle of the wrong length: GARBAGE_ARGS, no NFS status.
    let body = Req::default().u(12).raw(&[0; 12]).0;
    let (r, w) = run(nfsrv_getattr, &body, true, None, p);
    assert_eq!(r, Ok(()));
    Rd::new(w.expect("a reply"), RPC_GARBAGE).done();

    // A request that ends inside the file handle is dropped without a reply.
    let body = Req::default().u(NFSX_V3FH as u32).raw(&[0; 8]).0;
    let (r, w) = run(nfsrv_getattr, &body, true, None, p);
    assert_eq!(r, Err(Errno::EBADRPC));
    assert!(w.is_none());

    // A request without a client address does not pass the port check.
    let body = Req::default().fh(&Fhandle::default(), true).0;
    let (r, w) = run(nfsrv_getattr, &body, true, None, p);
    assert_eq!(r, Ok(()));
    // MSG_DENIED, AUTH_ERROR, AUTH_TOOWEAK.
    assert_eq!(w.expect("a reply"), [0x1234, 1, 1, 1, AUTH_TOOWEAK]);

    // The table is in procedure number order.
    assert!(core::ptr::fn_addr_eq(
        NFSRV3_PROCS[NFSPROC_GETATTR],
        nfsrv_getattr as NfsrvProc
    ));
    assert!(core::ptr::fn_addr_eq(
        NFSRV3_PROCS[NFS_NPROCS - 1],
        nfsrv_noop as NfsrvProc
    ));
}

/// An `MT_SONAME` mbuf holding the `AF_INET` address `a`, port `port`.
fn nam(a: [u8; 4], port: u16) -> &'static Mbuf {
    use crate::sys::mbuf::{M_DONTWAIT, MT_SONAME};
    let m = m_get(M_DONTWAIT, MT_SONAME).expect("an mbuf");
    let mut sin = [0u8; 16];
    sin[0] = 16;
    sin[1] = crate::sys::socket::AF_INET;
    sin[2..4].copy_from_slice(&port.to_be_bytes());
    sin[4..8].copy_from_slice(&a);
    // SAFETY: an mbuf's data area holds 16 bytes; `mtod` points at it.
    unsafe { ptr::copy_nonoverlapping(sin.as_ptr(), mtod::<u8>(m), 16) };
    m.m_len().set(16);
    m
}

#[test]
fn procedures_over_an_exported_ffs() {
    use crate::kern::kern_descrip::sys_close;
    use crate::kern::sys_generic::sys_write;
    use crate::kern::uipc_mbuf::tests::mbinit_again;
    use crate::kern::vfs_lookup::namei;
    use crate::kern::vfs_subr::tests::exports::{args, sin};
    use crate::kern::vfs_syscalls::{sys_mkdir, sys_mount, sys_open};
    use crate::nfs::nfsproto::NFSV3CREATE_EXCLUSIVE;
    use crate::sys::fcntl::{O_CREAT, O_RDWR};
    use crate::sys::mount::{MNT_EXPORTED, MNT_UPDATE, UfsArgs};
    use crate::ufs::ffs::ffs_vfsops::tests::{
        mount_root, newfs, path, read_file, setup, sys, teardown, unmount_root,
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
    let hello = b"hello";
    sys(
        sys_write,
        p,
        &[fd as usize, hello.as_ptr() as usize, hello.len()],
    )
    .unwrap();
    sys(sys_close, p, &[fd as usize]).unwrap();

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
    let filefh = fh(b"/dir/f");

    // mountd(8): export it read-write to 10.0.2.0/24, root mapped to root.
    let net = sin(2, [10, 0, 2, 0]);
    let mask = sin(2, [255, 255, 255, 0]);
    let mut ua = UfsArgs {
        fspec: 0,
        export_info: args(MNT_EXPORTED, 0, Some(net), Some(mask)),
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
    let client = Some(nam([10, 0, 2, 9], 700));
    let call = |f: NfsrvProc, body: Req, v3: bool| -> Rd {
        let (r, w) = run(f, &body.0, v3, client, p);
        assert_eq!(r, Ok(()));
        Rd::new(w.expect("a reply"), 0)
    };

    // GETATTR, both versions.
    let mut rd = call(nfsrv_getattr, Req::default().fh(&dirfh, true), true);
    assert_eq!(rd.u(), 0);
    let fa = rd.n(21);
    assert_eq!((fa[0], fa[1] & 0o7777), (2, 0o755), "NF3DIR, mode");
    rd.done();
    let mut rd = call(nfsrv_getattr, Req::default().fh(&filefh, false), false);
    assert_eq!(rd.u(), 0);
    let fa = rd.n(17);
    assert_eq!((fa[0], fa[5]), (1, 5), "NFREG, size");
    rd.done();

    // LOOKUP: the handle and attributes of the file, the directory's attributes.
    let mut rd = call(
        nfsrv_lookup,
        Req::default().fh(&dirfh, true).name(b"f"),
        true,
    );
    assert_eq!(rd.u(), 0);
    assert_eq!(rd.opaque(), xdr_bytes(&filefh));
    assert!(rd.postop().is_some());
    assert!(rd.postop().is_some());
    rd.done();
    let mut rd = call(
        nfsrv_lookup,
        Req::default().fh(&dirfh, true).name(b"nope"),
        true,
    );
    assert_eq!(rd.u() as i32, NFSERR_NOENT);
    assert!(rd.postop().is_some(), "the directory's attributes");
    rd.done();
    // A name too long for NFS: NAMETOOLONG, nothing more.
    let long = [b'x'; NFS_MAXNAMLEN + 1];
    let mut rd = call(
        nfsrv_lookup,
        Req::default().fh(&dirfh, true).name(&long),
        true,
    );
    assert_eq!(rd.u() as i32, NFSERR_NAMETOL);
    rd.done();

    // ACCESS: root may read and modify a 0644 file, not execute it.
    let mut rd = call(
        nfsrv3_access,
        Req::default()
            .fh(&filefh, true)
            .u(NFSV3ACCESS_READ | NFSV3ACCESS_MODIFY | NFSV3ACCESS_EXECUTE),
        true,
    );
    assert_eq!(rd.u(), 0);
    assert!(rd.postop().is_some());
    assert_eq!(rd.u(), NFSV3ACCESS_READ | NFSV3ACCESS_MODIFY);
    rd.done();

    // READ: all of it, eof.
    let mut rd = call(
        nfsrv_read,
        Req::default().fh(&filefh, true).hyper(0).u(100),
        true,
    );
    assert_eq!(rd.u(), 0);
    assert!(rd.postop().is_some());
    assert_eq!((rd.u(), rd.u()), (5, 1), "count, eof");
    assert_eq!(rd.opaque(), hello);
    rd.done();

    // WRITE (the data spread over several mbufs), then a version 2 READ of the result.
    let data = b" world, from far away";
    let mut rd = call(
        nfsrv_write,
        Req::default()
            .fh(&filefh, true)
            .hyper(5)
            .u(data.len() as u32)
            .u(NFSV3WRITE_FILESYNC)
            .name(data),
        true,
    );
    assert_eq!(rd.u(), 0);
    assert_eq!(rd.wcc().0, true);
    assert_eq!((rd.u(), rd.u()), (data.len() as u32, NFSV3WRITE_FILESYNC));
    rd.n(2); // the write verifier
    rd.done();
    let mut rd = call(
        nfsrv_read,
        Req::default().fh(&filefh, false).u(0).u(100).u(0),
        false,
    );
    assert_eq!(rd.u(), 0);
    let fa = rd.n(17);
    assert_eq!(fa[5] as usize, 5 + data.len());
    assert_eq!(rd.opaque(), b"hello world, from far away");
    rd.done();
    assert_eq!(
        read_file(p, b"/dir/f\0").unwrap(),
        b"hello world, from far away"
    );
    // A read of 3 bytes: the data padded to a word (nfsm_adj), not eof.
    let mut rd = call(
        nfsrv_read,
        Req::default().fh(&filefh, true).hyper(1).u(3),
        true,
    );
    assert_eq!(rd.u(), 0);
    assert!(rd.postop().is_some());
    assert_eq!((rd.u(), rd.u()), (3, 0), "count, eof");
    let n = rd.u();
    assert_eq!(n, 3);
    assert_eq!(rd.u().to_be_bytes(), *b"ell\0");
    rd.done();
    // A read past the end: nothing, eof.
    let mut rd = call(
        nfsrv_read,
        Req::default().fh(&filefh, true).hyper(1000).u(10),
        true,
    );
    assert_eq!(rd.u(), 0);
    assert!(rd.postop().is_some());
    assert_eq!((rd.u(), rd.u()), (0, 1));
    assert_eq!(rd.opaque(), b"");
    rd.done();

    // CREATE: unchecked, then guarded over it (EEXIST, the file not left locked).
    let mut rd = call(
        nfsrv_create,
        Req::default()
            .fh(&dirfh, true)
            .name(b"new")
            .u(NFSV3CREATE_UNCHECKED)
            .sattr3(Some(0o600)),
        true,
    );
    assert_eq!(rd.u(), 0);
    let newfh = rd.postop_fh().expect("the handle");
    let fa = rd.postop().expect("the attributes");
    assert_eq!((fa[0], fa[1] & 0o7777), (1, 0o600));
    let (pre, post) = rd.wcc();
    assert!(pre && post.is_some());
    rd.done();
    let mut rd = call(
        nfsrv_create,
        Req::default()
            .fh(&dirfh, true)
            .name(b"new")
            .u(NFSV3CREATE_GUARDED)
            .sattr3(None),
        true,
    );
    assert_eq!(rd.u() as i32, NFSERR_EXIST);
    let (pre, post) = rd.wcc();
    assert!(pre && post.is_some());
    rd.done();
    // Exclusive: the verifier goes into va_atime.tv_sec, but with tv_nsec left VNOVAL
    // ufs_setattr does not set the times, so the file's atime never matches it and even a
    // retransmission with the same verifier is EEXIST: the C's behaviour with a 64-bit
    // time_t, kept.
    let excl = |verf: &[u8; 8]| {
        call(
            nfsrv_create,
            Req::default()
                .fh(&dirfh, true)
                .name(b"excl")
                .u(NFSV3CREATE_EXCLUSIVE)
                .raw(verf),
            true,
        )
        .u()
    };
    assert_eq!(excl(b"verifier"), 0);
    assert_eq!(excl(b"verifier") as i32, NFSERR_EXIST);
    assert_eq!(excl(b"other!!!") as i32, NFSERR_EXIST);
    // Version 2: a file with the sattr's mode, its handle and attributes.
    let mut rd = call(
        nfsrv_create,
        Req::default()
            .fh(&dirfh, false)
            .name(b"v2file")
            .sattr2(0o100640),
        false,
    );
    assert_eq!(rd.u(), 0);
    rd.n(8); // the handle
    let fa = rd.n(17);
    assert_eq!((fa[0], fa[1] & 0o7777), (1, 0o640));
    rd.done();

    // MKDIR, MKNOD (a fifo), SYMLINK and READLINK.
    let mut rd = call(
        nfsrv_mkdir,
        Req::default()
            .fh(&dirfh, true)
            .name(b"sub")
            .sattr3(Some(0o700)),
        true,
    );
    assert_eq!(rd.u(), 0);
    assert!(rd.postop_fh().is_some());
    assert_eq!(rd.postop().expect("attributes")[0], 2);
    rd.wcc();
    rd.done();
    // A character device (VOP_MKNOD, then the lookup again), a socket (VOP_CREATE), and a
    // fifo, which this ffs cannot load (no fifofs): VOP_MKNOD makes the entry, the lookup
    // after it fails with EOPNOTSUPP, mapped to NFSERR_IO.
    let mut rd = call(
        nfsrv_mknod,
        Req::default()
            .fh(&dirfh, true)
            .name(b"chr")
            .u(4)
            .sattr3(Some(0o600))
            .u(2)
            .u(3),
        true,
    );
    assert_eq!(rd.u(), 0);
    assert!(rd.postop_fh().is_some());
    let fa = rd.postop().expect("attributes");
    assert_eq!((fa[0], fa[9], fa[10]), (4, 2, 3), "NF3CHR, major, minor");
    rd.wcc();
    rd.done();
    let mut rd = call(
        nfsrv_mknod,
        Req::default()
            .fh(&dirfh, true)
            .name(b"sock")
            .u(6)
            .sattr3(Some(0o644)),
        true,
    );
    assert_eq!(rd.u(), 0);
    assert!(rd.postop_fh().is_some());
    assert_eq!(rd.postop().expect("attributes")[0], 6, "NF3SOCK");
    rd.wcc();
    rd.done();
    let mut rd = call(
        nfsrv_mknod,
        Req::default()
            .fh(&dirfh, true)
            .name(b"fifo")
            .u(7)
            .sattr3(Some(0o644)),
        true,
    );
    assert_eq!(rd.u(), 5, "NFSERR_IO");
    let (pre, post) = rd.wcc();
    assert!(pre && post.is_some());
    rd.done();
    // A regular file is not a type for MKNOD.
    let mut rd = call(
        nfsrv_mknod,
        Req::default().fh(&dirfh, true).name(b"reg").u(1),
        true,
    );
    assert_eq!(rd.u() as i32, NFSERR_BADTYPE);
    rd.wcc();
    rd.done();
    let mut rd = call(
        nfsrv_symlink,
        Req::default()
            .fh(&dirfh, true)
            .name(b"ln")
            .sattr3(Some(0o755)) // the mode clients send (VNOVAL would make a bad inode, as in C)
            .name(b"f"),
        true,
    );
    assert_eq!(rd.u(), 0);
    let lnfh = rd.postop_fh().expect("the link's handle");
    assert_eq!(rd.postop().expect("attributes")[0], 5, "NF3LNK");
    rd.wcc();
    rd.done();
    let lnfh: Fhandle = xdr_get(&lnfh);
    let mut rd = call(nfsrv_readlink, Req::default().fh(&lnfh, true), true);
    assert_eq!(rd.u(), 0);
    assert!(rd.postop().is_some());
    assert_eq!(rd.opaque(), b"f");
    rd.done();
    // READLINK of a file that is not a link: version 2 ENXIO.
    let mut rd = call(nfsrv_readlink, Req::default().fh(&filefh, false), false);
    assert_eq!(rd.u() as i32, Errno::ENXIO.as_i32());
    rd.done();

    // LINK, RENAME, REMOVE.
    let mut rd = call(
        nfsrv_link,
        Req::default()
            .fh(&filefh, true)
            .fh(&dirfh, true)
            .name(b"hard"),
        true,
    );
    assert_eq!(rd.u(), 0);
    assert_eq!(rd.postop().expect("attributes")[2], 2, "two links");
    rd.wcc();
    rd.done();
    let mut rd = call(
        nfsrv_rename,
        Req::default()
            .fh(&dirfh, true)
            .name(b"hard")
            .fh(&dirfh, true)
            .name(b"hard2"),
        true,
    );
    assert_eq!(rd.u(), 0);
    rd.wcc();
    rd.wcc();
    rd.done();
    // A second file handle of the wrong length: GARBAGE_ARGS after the source was looked
    // up (nfsmout releases it), and a request that ends after the source name is dropped.
    let (r, w) = run(
        nfsrv_rename,
        &Req::default()
            .fh(&dirfh, true)
            .name(b"f")
            .u(12)
            .raw(&[0; 12])
            .0,
        true,
        client,
        p,
    );
    assert_eq!(r, Ok(()));
    Rd::new(w.expect("a reply"), RPC_GARBAGE).done();
    let (r, w) = run(
        nfsrv_rename,
        &Req::default().fh(&dirfh, true).name(b"f").0,
        true,
        client,
        p,
    );
    assert_eq!(r, Err(Errno::EBADRPC));
    assert!(w.is_none());
    // Renaming a name to itself does nothing.
    let mut rd = call(
        nfsrv_rename,
        Req::default()
            .fh(&dirfh, false)
            .name(b"hard2")
            .fh(&dirfh, false)
            .name(b"hard2"),
        false,
    );
    assert_eq!(rd.u(), 0);
    rd.done();
    let mut rd = call(
        nfsrv_remove,
        Req::default().fh(&dirfh, true).name(b"hard2"),
        true,
    );
    assert_eq!(rd.u(), 0);
    rd.wcc();
    rd.done();
    let mut rd = call(
        nfsrv_remove,
        Req::default().fh(&dirfh, true).name(b"hard2"),
        true,
    );
    assert_eq!(rd.u() as i32, NFSERR_NOENT);
    rd.wcc();
    rd.done();
    // RMDIR of a file: ENOTDIR; of the directory: done.
    let mut rd = call(
        nfsrv_rmdir,
        Req::default().fh(&dirfh, true).name(b"f"),
        true,
    );
    assert_eq!(rd.u() as i32, Errno::ENOTDIR.as_i32());
    rd.wcc();
    rd.done();
    let mut rd = call(
        nfsrv_rmdir,
        Req::default().fh(&dirfh, true).name(b"sub"),
        true,
    );
    assert_eq!(rd.u(), 0);
    rd.wcc();
    rd.done();

    // READDIR and READDIRPLUS: every name, eof.
    let expect: Vec<&[u8]> = std::vec![
        b".", b"..", b"f", b"new", b"excl", b"v2file", b"chr", b"sock", b"ln", b"fifo"
    ];
    let mut rd = call(
        nfsrv_readdir,
        Req::default().fh(&dirfh, true).hyper(0).hyper(0).u(4096),
        true,
    );
    assert_eq!(rd.u(), 0);
    assert!(rd.postop().is_some());
    rd.n(2); // the cookie verifier
    let mut names = Vec::new();
    while rd.u() == 1 {
        rd.hyper();
        names.push(rd.opaque());
        rd.hyper();
    }
    assert_eq!(rd.u(), 1, "eof");
    rd.done();
    let mut sorted = names.clone();
    sorted.sort();
    let mut want: Vec<Vec<u8>> = expect.iter().map(|n| n.to_vec()).collect();
    want.sort();
    assert_eq!(sorted, want);
    let mut rd = call(
        nfsrv_readdirplus,
        Req::default()
            .fh(&dirfh, true)
            .hyper(0)
            .hyper(0)
            .u(4096)
            .u(8192),
        true,
    );
    assert_eq!(rd.u(), 0);
    assert!(rd.postop().is_some());
    rd.n(2);
    let mut plus = Vec::new();
    while rd.u() == 1 {
        rd.hyper();
        let name = rd.opaque();
        rd.hyper();
        assert!(rd.postop().is_some());
        let h = rd.postop_fh().expect("a handle");
        if name == b"f" {
            assert_eq!(h, xdr_bytes(&filefh));
        }
        plus.push(name);
    }
    assert_eq!(rd.u(), 1, "eof");
    rd.done();
    assert_eq!(plus, names);
    // A count too small for all of them: the first few, not eof; the C's paranoia words
    // (12) and one version 3 entry of a short name (32) fit 64 bytes, two do not.
    let mut rd = call(
        nfsrv_readdir,
        Req::default().fh(&dirfh, true).hyper(0).hyper(0).u(64),
        true,
    );
    assert_eq!(rd.u(), 0);
    assert!(rd.postop().is_some());
    rd.n(2);
    let mut few = 0;
    while rd.u() == 1 {
        rd.hyper();
        rd.opaque();
        rd.hyper();
        few += 1;
    }
    assert_eq!((few, rd.u()), (1, 0), "one entry, not eof");
    rd.done();
    // A count of 0: TOOSMALL.
    let mut rd = call(
        nfsrv_readdir,
        Req::default().fh(&dirfh, true).hyper(0).hyper(0).u(0),
        true,
    );
    assert_eq!(rd.u() as i32, NFSERR_TOOSMALL);
    assert!(rd.postop().is_none());
    rd.done();

    // SETATTR: truncate the new file (guard off), then GETATTR sees it.
    let newfh: Fhandle = xdr_get(&newfh);
    let mut rd = call(
        nfsrv_setattr,
        Req::default()
            .fh(&newfh, true)
            .u(0)
            .u(0)
            .u(0)
            .u(1)
            .hyper(1234)
            .u(0)
            .u(0)
            .u(0),
        true,
    );
    assert_eq!(rd.u(), 0);
    let (pre, post) = rd.wcc();
    assert!(pre);
    let post = post.expect("post-op attributes");
    assert_eq!(((u64::from(post[5]) << 32) | u64::from(post[6])), 1234);
    rd.done();

    // STATFS, FSINFO, PATHCONF, COMMIT.
    let mut rd = call(nfsrv_statfs, Req::default().fh(&dirfh, true), true);
    assert_eq!(rd.u(), 0);
    assert!(rd.postop().is_some());
    let sf = rd.n(13);
    assert!(sf[0] > 0 || sf[1] > 0, "total bytes");
    rd.done();
    let mut rd = call(nfsrv_statfs, Req::default().fh(&dirfh, false), false);
    assert_eq!(rd.u(), 0);
    let sf = rd.n(5);
    assert_eq!(sf[0], NFS_MAXDGRAMDATA as u32);
    rd.done();
    let mut rd = call(nfsrv_fsinfo, Req::default().fh(&dirfh, true), true);
    assert_eq!(rd.u(), 0);
    assert!(rd.postop().is_some());
    let fs = rd.n(12);
    assert_eq!((fs[0], fs[1]), (NFS_MAXDATA as u32, NFS_MAXDATA as u32));
    assert_eq!(fs[11], 0x1b, "LINK | SYMLINK | HOMOGENEOUS | CANSETTIME");
    rd.done();
    let mut rd = call(nfsrv_pathconf, Req::default().fh(&dirfh, true), true);
    assert_eq!(rd.u(), 0);
    assert!(rd.postop().is_some());
    let pc = rd.n(6);
    assert_eq!(pc[1], 255, "name max");
    assert_eq!((pc[4], pc[5]), (0, 1), "case sensitive, preserving");
    rd.done();
    let mut rd = call(
        nfsrv_commit,
        Req::default().fh(&filefh, true).hyper(0).u(0),
        true,
    );
    assert_eq!(rd.u(), 0);
    let (pre, post) = rd.wcc();
    assert!(pre && post.is_some());
    rd.n(2);
    rd.done();

    // A client outside the export list: EACCES (version 2; version 3 GETATTR maps it to
    // NFSERR_IO).
    let (r, w) = run(
        nfsrv_getattr,
        &Req::default().fh(&dirfh, false).0,
        false,
        Some(nam([10, 0, 3, 9], 700)),
        p,
    );
    assert_eq!(r, Ok(()));
    let mut rd = Rd::new(w.expect("a reply"), 0);
    assert_eq!(rd.u() as i32, Errno::EACCES.as_i32());
    rd.done();

    // Nothing is left referenced or locked: the file system unmounts.
    mbinit_again();
    unmount_root(p, mp);
    teardown();
}
