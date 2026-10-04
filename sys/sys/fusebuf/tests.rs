//! Host tests for `<sys/fusebuf.h>`: the protocol structures' layout (the bytes libfuse
//! reads and writes), the `op` union's overlapping members, the dirent helpers, and the
//! constants against the C header (`just test-ref`).

use std::{assert, assert_eq};

use super::*;

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/sys/fusebuf.h");
    for (name, value) in [
        ("FUSEBUFMAXSIZE", FUSEBUFMAXSIZE as i64),
        ("FUSE_KERNEL_VERSION", i64::from(FUSE_KERNEL_VERSION)),
        (
            "FUSE_KERNEL_MINOR_VERSION",
            i64::from(FUSE_KERNEL_MINOR_VERSION),
        ),
        ("FUSE_FATTR_MODE", i64::from(FUSE_FATTR_MODE)),
        ("FUSE_FATTR_UID", i64::from(FUSE_FATTR_UID)),
        ("FUSE_FATTR_GID", i64::from(FUSE_FATTR_GID)),
        ("FUSE_FATTR_SIZE", i64::from(FUSE_FATTR_SIZE)),
        ("FUSE_FATTR_ATIME", i64::from(FUSE_FATTR_ATIME)),
        ("FUSE_FATTR_MTIME", i64::from(FUSE_FATTR_MTIME)),
        ("FUSE_LOOKUP", i64::from(FUSE_LOOKUP)),
        ("FUSE_GETATTR", i64::from(FUSE_GETATTR)),
        ("FUSE_SETATTR", i64::from(FUSE_SETATTR)),
        ("FUSE_READLINK", i64::from(FUSE_READLINK)),
        ("FUSE_SYMLINK", i64::from(FUSE_SYMLINK)),
        ("FUSE_MKNOD", i64::from(FUSE_MKNOD)),
        ("FUSE_MKDIR", i64::from(FUSE_MKDIR)),
        ("FUSE_UNLINK", i64::from(FUSE_UNLINK)),
        ("FUSE_RMDIR", i64::from(FUSE_RMDIR)),
        ("FUSE_RENAME", i64::from(FUSE_RENAME)),
        ("FUSE_LINK", i64::from(FUSE_LINK)),
        ("FUSE_OPEN", i64::from(FUSE_OPEN)),
        ("FUSE_READ", i64::from(FUSE_READ)),
        ("FUSE_WRITE", i64::from(FUSE_WRITE)),
        ("FUSE_STATFS", i64::from(FUSE_STATFS)),
        ("FUSE_RELEASE", i64::from(FUSE_RELEASE)),
        ("FUSE_FSYNC", i64::from(FUSE_FSYNC)),
        ("FUSE_FLUSH", i64::from(FUSE_FLUSH)),
        ("FUSE_INIT", i64::from(FUSE_INIT)),
        ("FUSE_OPENDIR", i64::from(FUSE_OPENDIR)),
        ("FUSE_READDIR", i64::from(FUSE_READDIR)),
        ("FUSE_RELEASEDIR", i64::from(FUSE_RELEASEDIR)),
        ("FUSE_DESTROY", i64::from(FUSE_DESTROY)),
        ("FUSE_FORGET", i64::from(FUSE_FORGET)),
    ] {
        assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
    }
}

#[test]
fn header_bytes_are_the_c_layout() {
    let h = FuseInHeader {
        len: 0x0102_0304,
        opcode: FUSE_LOOKUP,
        unique: 0x1122_3344_5566_7788,
        nodeid: 9,
        uid: 1000,
        gid: 1001,
        pid: 100_005,
        padding: 0,
    };
    let b = abi_bytes(&h);
    assert_eq!(b.len(), 40);
    assert_eq!(&b[0..4], &0x0102_0304u32.to_ne_bytes());
    assert_eq!(&b[4..8], &FUSE_LOOKUP.to_ne_bytes());
    assert_eq!(&b[8..16], &0x1122_3344_5566_7788u64.to_ne_bytes());
    assert_eq!(&b[16..24], &9u64.to_ne_bytes());
    assert_eq!(&b[24..28], &1000u32.to_ne_bytes());
    assert_eq!(&b[28..32], &1001u32.to_ne_bytes());
    assert_eq!(&b[32..36], &100_005u32.to_ne_bytes());

    let mut o = FuseOutHeader::default();
    abi_bytes_mut(&mut o)[4..8].copy_from_slice(&(-2i32).to_ne_bytes());
    assert_eq!(o.error, -2);
}

#[test]
fn op_members_overlap_like_the_c_union() {
    let fb = Fusebuf::new();
    fb.op_set(&FuseOpenIn {
        flags: 0x0202,
        unused: 0,
    });
    // The input occupies the first bytes of the union; the reply overwrites the same bytes.
    assert_eq!(fb.op_get::<FuseOpenIn>().flags, 0x0202);
    assert_eq!(&fb.op.get().bytes[0..4], &0x0202u32.to_ne_bytes());
    fb.op_set(&FuseOpenOut {
        fh: 77,
        open_flags: 1,
        padding: 0,
    });
    assert_eq!(fb.op_get::<FuseOpenOut>().fh, 77);
    assert_eq!(fb.op_get::<FuseOpenIn>().flags, 77);

    // Writing a small member leaves the bytes past it alone.
    let mut op = FusebufOp::new();
    op.bytes[8] = 0xaa;
    op.set(&FuseMkdirIn { mode: 1, umask: 2 });
    assert_eq!(op.bytes[8], 0xaa);
    assert_eq!(op.get::<FuseMkdirIn>(), FuseMkdirIn { mode: 1, umask: 2 });
}

#[test]
fn fusebuf_aliases_read_the_header() {
    let fb = Fusebuf::new();
    fb.update_hdr(|h| {
        h.opcode = FUSE_READ;
        h.unique = 42;
        h.nodeid = FUSE_ROOT_ID;
        h.uid = 3;
        h.gid = 4;
        h.pid = 5;
    });
    fb.set_fb_err(2);
    fb.set_fb_len(16);
    assert_eq!(fb.fb_type(), FUSE_READ);
    assert_eq!(fb.fb_uuid(), 42);
    assert_eq!(fb.fb_ino(), 1);
    assert_eq!((fb.fb_uid(), fb.fb_gid(), fb.fb_tid()), (3, 4, 5));
    assert_eq!(fb.fb_err(), 2);
    assert_eq!(fb.fb_len(), 16);
    assert!(fb.fb_dat().is_null());
    // SAFETY: no buffer: the slice is empty.
    assert!(unsafe { fb.fb_dat_slice() }.is_empty());
}

#[test]
fn dirent_size_rounds_to_eight() {
    assert_eq!(fuse_dirent_align(0), 0);
    assert_eq!(fuse_dirent_align(1), 8);
    assert_eq!(fuse_dirent_align(8), 8);
    assert_eq!(fuse_dirent_align(25), 32);
    let mut d = FuseDirent {
        namelen: 1,
        ..FuseDirent::default()
    };
    assert_eq!(fuse_dirent_size(&d), 32);
    d.namelen = 8;
    assert_eq!(fuse_dirent_size(&d), 32);
    d.namelen = 9;
    assert_eq!(fuse_dirent_size(&d), 40);
}

#[test]
fn dirent_from_bytes_reads_the_fixed_part() {
    let mut b = [0u8; 32];
    b[0..8].copy_from_slice(&7u64.to_ne_bytes());
    b[8..16].copy_from_slice(&3u64.to_ne_bytes());
    b[16..20].copy_from_slice(&5u32.to_ne_bytes());
    b[20..24].copy_from_slice(&4u32.to_ne_bytes());
    b[24..29].copy_from_slice(b"hello");
    let d = FuseDirent::from_bytes(&b).unwrap();
    assert_eq!((d.ino, d.off, d.namelen, d.r#type), (7, 3, 5, 4));
    assert!(FuseDirent::from_bytes(&b[..23]).is_none());
}
