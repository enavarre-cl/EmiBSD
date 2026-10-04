//! Host tests for `<nfs/nfsproto.h>`: the union accessors, the type conversions and, against
//! the C header, the constants.

use std::string::String;

use super::*;
use crate::sys::vnode::{VBLK, VDIR, VLNK, VNON, VREG, VSOCK};

#[test]
fn union_accessors_name_the_c_words() {
    let mut fa = NfsFattr::default();
    fa.set_fa2_size(1);
    fa.set_fa2_ctime(Nfsv2Time::from_words([2, 3]));
    assert_eq!(fa.fa_un[0], 1);
    assert_eq!(fa.fa_un[10..12], [2, 3]);
    assert_eq!(fa.fa3_size(), Nfsuint64::from_words([1, 0]));
    fa.set_fa3_ctime(Nfsv3Time::from_words([7, 8]));
    assert_eq!(fa.fa_un[14..16], [7, 8]);
    assert_eq!(fa.fa3_ctime().nfsv3_nsec, 8);

    let mut sf = NfsStatfs::default();
    sf.set_sf_invarsec(9);
    sf.set_sf_afiles(Nfsuint64::from_words([4, 5]));
    assert_eq!(sf.sf_un[10..13], [4, 5, 9]);
}

#[test]
fn file_handle_generic_member_is_the_first_bytes() {
    let mut fh = Nfsfh::new();
    let mut g = Fhandle::default();
    g.fh_fsid.val = [1, 2];
    g.fh_fid.fid_len = 12;
    g.fh_fid.fid_data[0] = 0xaa;
    fh.set_fh_generic(&g);
    assert_eq!(fh.fh_generic(), g);
    assert_eq!(fh.fh_bytes[0..4], 1i32.to_ne_bytes());
    assert!(fh.fh_bytes[NFSX_V3FH..].iter().all(|&b| b == 0));
}

#[test]
fn type_and_mode_conversions() {
    assert_eq!(nfsv3tov_type(txdr_unsigned(NFFIFO as u32)), VFIFO);
    assert_eq!(nfsv2tov_type(txdr_unsigned(NFSOCK as u32)), VNON);
    assert_eq!(nfsv3tov_type(txdr_unsigned(NFSOCK as u32)), VSOCK);
    assert_eq!(nfsv3tov_type(txdr_unsigned(8 | NFDIR as u32)), VDIR);
    assert_eq!(fxdr_unsigned(vtonfsv3_type(VLNK)), NFLNK as u32);
    assert_eq!(fxdr_unsigned(vtonfsv2_type(VFIFO)), NFCHR as u32);
    assert_eq!(fxdr_unsigned(vtonfsv2_type(VREG)), NFREG as u32);
    assert_eq!(fxdr_unsigned(vtonfsv3_type(VBLK)), NFBLK as u32);
    assert_eq!(fxdr_unsigned(vtonfsv2_mode(VFIFO, 0o644)), 0o020644);
    assert_eq!(fxdr_unsigned(vtonfsv2_mode(VDIR, 0o755)), 0o040755);
    assert_eq!(fxdr_unsigned(vtonfsv3_mode(0o104755)), 0o4755);
    assert_eq!(nfstov_mode(txdr_unsigned(0o100644)), 0o644);
}

#[test]
fn version_sizes() {
    assert_eq!(nfsx_fh(true), 68);
    assert_eq!(nfsx_fh(false), 32);
    assert_eq!(nfsx_srvfh(true), 28);
    assert_eq!(nfsx_postoporfattr(true), 88);
    assert_eq!(nfsx_wccdata(true), 120);
    assert_eq!(nfsx_readdir(false), 8);
    assert_eq!(NFSX_V3SRVSATTR, 44);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn constants_match_the_c_header() {
    let defs = crate::reftest::defines("sys/nfs/nfsproto.h");
    let ours: &[(&str, i64)] = &[
        ("NFS_PORT", NFS_PORT.into()),
        ("NFS_PROG", NFS_PROG.into()),
        ("NFS_VER2", NFS_VER2.into()),
        ("NFS_VER3", NFS_VER3.into()),
        ("NFS_VER4", NFS_VER4.into()),
        ("NFS_V2MAXDATA", NFS_V2MAXDATA as i64),
        ("NFS_MAXDGRAMDATA", NFS_MAXDGRAMDATA as i64),
        ("NFS_MAXPATHLEN", NFS_MAXPATHLEN as i64),
        ("NFS_MAXNAMLEN", NFS_MAXNAMLEN as i64),
        ("NFS_MAXPKTHDR", NFS_MAXPKTHDR as i64),
        ("NFS_MINPACKET", NFS_MINPACKET as i64),
        ("NFS_FABLKSIZE", NFS_FABLKSIZE as i64),
        ("NFS_OK", NFS_OK.into()),
        ("NFSERR_PERM", NFSERR_PERM.into()),
        ("NFSERR_NOENT", NFSERR_NOENT.into()),
        ("NFSERR_IO", NFSERR_IO.into()),
        ("NFSERR_NXIO", NFSERR_NXIO.into()),
        ("NFSERR_ACCES", NFSERR_ACCES.into()),
        ("NFSERR_EXIST", NFSERR_EXIST.into()),
        ("NFSERR_XDEV", NFSERR_XDEV.into()),
        ("NFSERR_NODEV", NFSERR_NODEV.into()),
        ("NFSERR_NOTDIR", NFSERR_NOTDIR.into()),
        ("NFSERR_ISDIR", NFSERR_ISDIR.into()),
        ("NFSERR_INVAL", NFSERR_INVAL.into()),
        ("NFSERR_FBIG", NFSERR_FBIG.into()),
        ("NFSERR_NOSPC", NFSERR_NOSPC.into()),
        ("NFSERR_ROFS", NFSERR_ROFS.into()),
        ("NFSERR_MLINK", NFSERR_MLINK.into()),
        ("NFSERR_NAMETOL", NFSERR_NAMETOL.into()),
        ("NFSERR_NOTEMPTY", NFSERR_NOTEMPTY.into()),
        ("NFSERR_DQUOT", NFSERR_DQUOT.into()),
        ("NFSERR_STALE", NFSERR_STALE.into()),
        ("NFSERR_REMOTE", NFSERR_REMOTE.into()),
        ("NFSERR_WFLUSH", NFSERR_WFLUSH.into()),
        ("NFSERR_BADHANDLE", NFSERR_BADHANDLE.into()),
        ("NFSERR_NOT_SYNC", NFSERR_NOT_SYNC.into()),
        ("NFSERR_BAD_COOKIE", NFSERR_BAD_COOKIE.into()),
        ("NFSERR_NOTSUPP", NFSERR_NOTSUPP.into()),
        ("NFSERR_TOOSMALL", NFSERR_TOOSMALL.into()),
        ("NFSERR_SERVERFAULT", NFSERR_SERVERFAULT.into()),
        ("NFSERR_BADTYPE", NFSERR_BADTYPE.into()),
        ("NFSERR_JUKEBOX", NFSERR_JUKEBOX.into()),
        ("NFSERR_TRYLATER", NFSERR_TRYLATER.into()),
        ("NFSERR_STALEWRITEVERF", NFSERR_STALEWRITEVERF.into()),
        ("NFSERR_RETVOID", NFSERR_RETVOID.into()),
        ("NFSERR_AUTHERR", NFSERR_AUTHERR.into()),
        ("NFSERR_RETERR", i64::from(NFSERR_RETERR as u32)),
        ("NFSX_UNSIGNED", NFSX_UNSIGNED as i64),
        ("NFSX_V2FH", NFSX_V2FH as i64),
        ("NFSX_V2FATTR", NFSX_V2FATTR as i64),
        ("NFSX_V2SATTR", NFSX_V2SATTR as i64),
        ("NFSX_V2COOKIE", NFSX_V2COOKIE as i64),
        ("NFSX_V2STATFS", NFSX_V2STATFS as i64),
        ("NFSX_V3FHMAX", NFSX_V3FHMAX as i64),
        ("NFSX_V3FATTR", NFSX_V3FATTR as i64),
        ("NFSX_V3SATTR", NFSX_V3SATTR as i64),
        ("NFSX_V3POSTOPATTR", NFSX_V3POSTOPATTR as i64),
        ("NFSX_V3WCCDATA", NFSX_V3WCCDATA as i64),
        ("NFSX_V3COOKIEVERF", NFSX_V3COOKIEVERF as i64),
        ("NFSX_V3WRITEVERF", NFSX_V3WRITEVERF as i64),
        ("NFSX_V3CREATEVERF", NFSX_V3CREATEVERF as i64),
        ("NFSX_V3STATFS", NFSX_V3STATFS as i64),
        ("NFSX_V3FSINFO", NFSX_V3FSINFO as i64),
        ("NFSX_V3PATHCONF", NFSX_V3PATHCONF as i64),
        ("NFSPROC_NULL", NFSPROC_NULL as i64),
        ("NFSPROC_GETATTR", NFSPROC_GETATTR as i64),
        ("NFSPROC_SETATTR", NFSPROC_SETATTR as i64),
        ("NFSPROC_LOOKUP", NFSPROC_LOOKUP as i64),
        ("NFSPROC_ACCESS", NFSPROC_ACCESS as i64),
        ("NFSPROC_READLINK", NFSPROC_READLINK as i64),
        ("NFSPROC_READ", NFSPROC_READ as i64),
        ("NFSPROC_WRITE", NFSPROC_WRITE as i64),
        ("NFSPROC_CREATE", NFSPROC_CREATE as i64),
        ("NFSPROC_MKDIR", NFSPROC_MKDIR as i64),
        ("NFSPROC_SYMLINK", NFSPROC_SYMLINK as i64),
        ("NFSPROC_MKNOD", NFSPROC_MKNOD as i64),
        ("NFSPROC_REMOVE", NFSPROC_REMOVE as i64),
        ("NFSPROC_RMDIR", NFSPROC_RMDIR as i64),
        ("NFSPROC_RENAME", NFSPROC_RENAME as i64),
        ("NFSPROC_LINK", NFSPROC_LINK as i64),
        ("NFSPROC_READDIR", NFSPROC_READDIR as i64),
        ("NFSPROC_READDIRPLUS", NFSPROC_READDIRPLUS as i64),
        ("NFSPROC_FSSTAT", NFSPROC_FSSTAT as i64),
        ("NFSPROC_FSINFO", NFSPROC_FSINFO as i64),
        ("NFSPROC_PATHCONF", NFSPROC_PATHCONF as i64),
        ("NFSPROC_COMMIT", NFSPROC_COMMIT as i64),
        ("NFSPROC_NOOP", NFSPROC_NOOP as i64),
        ("NFS_NPROCS", NFS_NPROCS as i64),
        ("NFSV2PROC_NULL", NFSV2PROC_NULL as i64),
        ("NFSV2PROC_GETATTR", NFSV2PROC_GETATTR as i64),
        ("NFSV2PROC_SETATTR", NFSV2PROC_SETATTR as i64),
        ("NFSV2PROC_NOOP", NFSV2PROC_NOOP as i64),
        ("NFSV2PROC_ROOT", NFSV2PROC_ROOT as i64),
        ("NFSV2PROC_LOOKUP", NFSV2PROC_LOOKUP as i64),
        ("NFSV2PROC_READLINK", NFSV2PROC_READLINK as i64),
        ("NFSV2PROC_READ", NFSV2PROC_READ as i64),
        ("NFSV2PROC_WRITECACHE", NFSV2PROC_WRITECACHE as i64),
        ("NFSV2PROC_WRITE", NFSV2PROC_WRITE as i64),
        ("NFSV2PROC_CREATE", NFSV2PROC_CREATE as i64),
        ("NFSV2PROC_REMOVE", NFSV2PROC_REMOVE as i64),
        ("NFSV2PROC_RENAME", NFSV2PROC_RENAME as i64),
        ("NFSV2PROC_LINK", NFSV2PROC_LINK as i64),
        ("NFSV2PROC_SYMLINK", NFSV2PROC_SYMLINK as i64),
        ("NFSV2PROC_MKDIR", NFSV2PROC_MKDIR as i64),
        ("NFSV2PROC_RMDIR", NFSV2PROC_RMDIR as i64),
        ("NFSV2PROC_READDIR", NFSV2PROC_READDIR as i64),
        ("NFSV2PROC_STATFS", NFSV2PROC_STATFS as i64),
        (
            "NFSV3SATTRTIME_DONTCHANGE",
            NFSV3SATTRTIME_DONTCHANGE.into(),
        ),
        ("NFSV3SATTRTIME_TOSERVER", NFSV3SATTRTIME_TOSERVER.into()),
        ("NFSV3SATTRTIME_TOCLIENT", NFSV3SATTRTIME_TOCLIENT.into()),
        ("NFSV3ACCESS_READ", NFSV3ACCESS_READ.into()),
        ("NFSV3ACCESS_LOOKUP", NFSV3ACCESS_LOOKUP.into()),
        ("NFSV3ACCESS_MODIFY", NFSV3ACCESS_MODIFY.into()),
        ("NFSV3ACCESS_EXTEND", NFSV3ACCESS_EXTEND.into()),
        ("NFSV3ACCESS_DELETE", NFSV3ACCESS_DELETE.into()),
        ("NFSV3ACCESS_EXECUTE", NFSV3ACCESS_EXECUTE.into()),
        ("NFSV3WRITE_UNSTABLE", NFSV3WRITE_UNSTABLE.into()),
        ("NFSV3WRITE_DATASYNC", NFSV3WRITE_DATASYNC.into()),
        ("NFSV3WRITE_FILESYNC", NFSV3WRITE_FILESYNC.into()),
        ("NFSV3CREATE_UNCHECKED", NFSV3CREATE_UNCHECKED.into()),
        ("NFSV3CREATE_GUARDED", NFSV3CREATE_GUARDED.into()),
        ("NFSV3CREATE_EXCLUSIVE", NFSV3CREATE_EXCLUSIVE.into()),
        ("NFSV3FSINFO_LINK", NFSV3FSINFO_LINK.into()),
        ("NFSV3FSINFO_SYMLINK", NFSV3FSINFO_SYMLINK.into()),
        ("NFSV3FSINFO_HOMOGENEOUS", NFSV3FSINFO_HOMOGENEOUS.into()),
        ("NFSV3FSINFO_CANSETTIME", NFSV3FSINFO_CANSETTIME.into()),
        ("NFS_MAXFHSIZE", NFS_MAXFHSIZE as i64),
    ];
    for (name, value) in ours {
        assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
    }
    // NFS_MAXDATA is MAXBSIZE, NFSX_V3FH and NFSX_V3SRVSATTR are sizeof expressions.
    assert_eq!(
        defs.get("NFS_MAXDATA").map(String::as_str),
        Some("MAXBSIZE")
    );
    assert_eq!(
        defs.get("NFSX_V3FH").map(String::as_str),
        Some("(sizeof (fhandle_t))")
    );
}
