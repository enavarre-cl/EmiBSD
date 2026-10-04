use super::*;

/// The layout of `struct nfs_dirent` as the C compiler lays it out on LP64.
#[test]
fn nfs_dirent_layout() {
    assert_eq!(NFS_DIRENT_OVERHEAD, 8);
    assert_eq!(NFS_DIRHDSIZ, 32);
    assert_eq!(ND_NAME, NFS_DIRHDSIZ);
    // The biggest record still fits in a readdir block.
    assert!(dirent_recsize(NFS_MAXNAMLEN) + NFS_DIRENT_OVERHEAD <= NFS_READDIRBLKSIZ as usize);
}

/// The access cache answers a request it covers, a success for the modes granted and a
/// failure for the modes refused, and only for its uid while it is young.
#[test]
fn access_cache_decisions() {
    let np = NfsNode::new();
    np.n_accstamp.set(-1);
    assert!(!nfs_access_cachevalid(&np, 100, 1000, 5));

    // A success for VREAD|VEXEC by uid 100 at t=1000.
    nfs_access_update(&np, false, VREAD | VEXEC, 100, 0, 1000);
    assert!(nfs_access_cachevalid(&np, 100, 1004, 5));
    assert!(!nfs_access_cachevalid(&np, 100, 1005, 5));
    assert!(!nfs_access_cachevalid(&np, 101, 1001, 5));
    assert_eq!(nfs_access_cached(&np, VREAD), Some(0));
    assert_eq!(nfs_access_cached(&np, VREAD | VEXEC), Some(0));
    assert_eq!(nfs_access_cached(&np, VWRITE), None);

    // Another success, for VWRITE: ORed in, the stamp kept.
    nfs_access_update(&np, true, VWRITE, 100, 0, 1003);
    assert_eq!(np.n_accstamp.get(), 1000);
    assert_eq!(nfs_access_cached(&np, VREAD | VWRITE | VEXEC), Some(0));

    // A refusal replaces the entry.
    let eacces = Errno::EACCES.as_i32();
    nfs_access_update(&np, true, VWRITE, 100, eacces, 1004);
    assert_eq!(np.n_accstamp.get(), 1004);
    assert_eq!(nfs_access_cached(&np, VWRITE), Some(eacces));
    assert_eq!(nfs_access_cached(&np, VWRITE | VREAD), Some(eacces));
    assert_eq!(nfs_access_cached(&np, VREAD), None);

    // Other errors are not cached.
    nfs_access_update(&np, true, VREAD, 100, Errno::EIO.as_i32(), 1004);
    assert_eq!(np.n_accerror.get(), eacces);
    assert_eq!(np.n_accmode.get(), VWRITE);
}

/// The NFSv3 ACCESS bits of a VOP_ACCESS mode.
#[test]
fn access_mode_bits() {
    assert_eq!(nfs_access_mode(VREG, VREAD), NFSV3ACCESS_READ);
    assert_eq!(
        nfs_access_mode(VREG, VWRITE | VEXEC),
        NFSV3ACCESS_MODIFY | NFSV3ACCESS_EXTEND | NFSV3ACCESS_EXECUTE
    );
    assert_eq!(
        nfs_access_mode(VDIR, VREAD | VWRITE | VEXEC),
        NFSV3ACCESS_READ
            | NFSV3ACCESS_MODIFY
            | NFSV3ACCESS_EXTEND
            | NFSV3ACCESS_DELETE
            | NFSV3ACCESS_LOOKUP
    );
}

/// The lowest commitment level of a series of writes.
#[test]
fn lowest_commit_level() {
    let (f, d, u) = (
        NFSV3WRITE_FILESYNC,
        NFSV3WRITE_DATASYNC,
        NFSV3WRITE_UNSTABLE,
    );
    assert_eq!(nfs_lowest_commit(f, d), d);
    assert_eq!(nfs_lowest_commit(f, u), u);
    assert_eq!(nfs_lowest_commit(d, u), u);
    assert_eq!(nfs_lowest_commit(d, f), d);
    assert_eq!(nfs_lowest_commit(u, f), u);
}

/// The silly name is ".nfs" and 16 upper-case hex digits, NUL-terminated.
#[test]
fn silly_name() {
    let (name, len) = nfs_sillyname([0x0123_abcd, 0xdead_beef]);
    assert_eq!(len, 20);
    assert_eq!(&name[..len], b".nfs0123ABCDDEADBEEF");
    assert!(name[len..].iter().all(|&b| b == 0));
}

/// Pack entries as nfs_readdirrpc does, then fix them up as nfs_readdir does: the records
/// tile whole NFS_READDIRBLKSIZ blocks, carry their names and cookies, and come out as
/// `struct dirent`s.
#[test]
fn readdir_pack_and_fixup() {
    let mut buf = std::vec![0u8; NFS_DIRBLKSIZ as usize];
    let names: std::vec::Vec<std::vec::Vec<u8>> = (0..20)
        .map(|i| {
            let len = 1 + (i * 37) % 200;
            (0..len).map(|j| b'a' + ((i + j) % 26) as u8).collect()
        })
        .collect();
    let mut packed = 0;
    let (pos, eof) = {
        let mut pack = NfsDirPack::new(&mut buf);
        let mut fit = true;
        for (i, name) in names.iter().enumerate() {
            fit = pack.begin(1000 + i as u64, name.len());
            if !fit {
                break;
            }
            pack.name_slot(name.len()).copy_from_slice(name);
            pack.end_name(name.len());
            let cookie = txdr_hyper(i as u64 + 1);
            pack.set_cookie(cookie[0], cookie[1]);
            packed += 1;
        }
        pack.finish();
        (pack.pos, fit)
    };
    assert!(!eof, "20 long names do not fit in one 1024-byte buffer");
    assert!(packed > 0);
    assert_eq!(pos % NFS_READDIRBLKSIZ as usize, 0);

    let mut off = 0;
    let mut n = 0;
    while off < pos {
        let namlen = usize::from(buf[off + ND_NAMLEN]);
        let fileno = u64::from_ne_bytes(
            buf[off + ND_FILENO..off + ND_FILENO + 8]
                .try_into()
                .unwrap(),
        );
        let (cookie, reclen, d_reclen) = nfs_dirent_fixup(&mut buf[off..]).unwrap();
        assert_eq!(fileno, 1000 + n as u64);
        assert_eq!(cookie, n as u64 + 1);
        assert_eq!(d_reclen, reclen - NFS_DIRENT_OVERHEAD);
        assert_eq!(&buf[off + ND_NAME..off + ND_NAME + namlen], &names[n][..]);
        assert_eq!(buf[off + ND_NAME + namlen], 0);
        assert_eq!(buf[off + ND_TYPE], DT_UNKNOWN);
        let d = Dirent::from_bytes(&buf[off + NFS_DIRENT_OVERHEAD..]).unwrap();
        assert_eq!(d.d_off, cookie as Off);
        assert_eq!(usize::from(d.d_reclen), d_reclen);
        assert!(reclen >= dirent_recsize(namlen) + NFS_DIRENT_OVERHEAD);
        // No record crosses a block boundary.
        let blk = NFS_READDIRBLKSIZ as usize;
        assert_eq!(off / blk, (off + reclen - 1) / blk);
        off += reclen;
        n += 1;
    }
    assert_eq!(off, pos);
    assert_eq!(n, packed);
}

/// A name with a '/' in it is refused.
#[test]
fn readdir_fixup_refuses_slash() {
    let mut buf = std::vec![0u8; NFS_DIRBLKSIZ as usize];
    let mut pack = NfsDirPack::new(&mut buf);
    assert!(pack.begin(7, 3));
    pack.name_slot(3).copy_from_slice(b"a/b");
    pack.end_name(3);
    pack.finish();
    assert_eq!(nfs_dirent_fixup(&mut buf), Err(Errno::EBADRPC));
}
