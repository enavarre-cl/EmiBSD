//! Host tests for `<ufs/ext2fs/ext2fs_dir.h>`.

use super::*;

#[test]
fn dirsiz_rounds_up_to_four_bytes() {
    assert_eq!(ext2fs_dirsiz(0), 8);
    assert_eq!(ext2fs_dirsiz(1), 12);
    assert_eq!(ext2fs_dirsiz(4), 12);
    assert_eq!(ext2fs_dirsiz(5), 16);
    assert_eq!(ext2fs_dirsiz(255), 264);
}

#[test]
fn modes_map_to_directory_types() {
    for (mode, ft) in [
        (EXT2_IFIFO, EXT2_FT_FIFO),
        (EXT2_IFCHR, EXT2_FT_CHRDEV),
        (EXT2_IFDIR, EXT2_FT_DIR),
        (EXT2_IFBLK, EXT2_FT_BLKDEV),
        (EXT2_IFREG, EXT2_FT_REG_FILE),
        (EXT2_IFLNK, EXT2_FT_SYMLINK),
        (EXT2_IFSOCK, EXT2_FT_SOCK),
        (0o030000, EXT2_FT_UNKNOWN),
        (0, EXT2_FT_UNKNOWN),
    ] {
        assert_eq!(inot2ext2dt(e2iftodt(mode | 0o644)), ft, "{mode:o}");
    }
    assert_eq!(e2iftodt(EXT2_IFDIR), 4);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn constants_match_the_c_header() {
    let defs = crate::reftest::defines("sys/ufs/ext2fs/ext2fs_dir.h");
    for (name, value) in [
        ("EXT2FS_MAXDIRSIZE", i64::from(EXT2FS_MAXDIRSIZE)),
        ("EXT2FS_MAXNAMLEN", EXT2FS_MAXNAMLEN as i64),
        ("EXT2_FT_UNKNOWN", i64::from(EXT2_FT_UNKNOWN)),
        ("EXT2_FT_REG_FILE", i64::from(EXT2_FT_REG_FILE)),
        ("EXT2_FT_DIR", i64::from(EXT2_FT_DIR)),
        ("EXT2_FT_CHRDEV", i64::from(EXT2_FT_CHRDEV)),
        ("EXT2_FT_BLKDEV", i64::from(EXT2_FT_BLKDEV)),
        ("EXT2_FT_FIFO", i64::from(EXT2_FT_FIFO)),
        ("EXT2_FT_SOCK", i64::from(EXT2_FT_SOCK)),
        ("EXT2_FT_SYMLINK", i64::from(EXT2_FT_SYMLINK)),
        ("EXT2_FT_MAX", i64::from(EXT2_FT_MAX)),
    ] {
        assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
    }
}

#[test]
fn entries_are_read_and_written_little_endian_in_place() {
    let mut b = [0xffu8; 64];
    let mut e = Ext2fsDirect::new();
    e.e2d_ino = 0x0102_0304;
    e.e2d_reclen = 0x0506;
    e.e2d_namlen = 5;
    e.e2d_type = EXT2_FT_DIR;
    e.e2d_name[..5].copy_from_slice(b"hello");
    e.write_to(&mut b, 4, ext2fs_dirsiz(5));
    assert_eq!(&b[4..12], &[4, 3, 2, 1, 6, 5, 5, EXT2_FT_DIR]);
    assert_eq!(&b[12..20], b"hello\0\0\0");
    assert_eq!(b[20], 0xff, "nothing past the entry");
    assert_eq!(e2d_ino(&b, 4), 0x0102_0304);
    assert_eq!(e2d_reclen(&b, 4), 0x0506);
    assert_eq!((e2d_namlen(&b, 4), e2d_type(&b, 4)), (5, EXT2_FT_DIR));
    assert_eq!(e2d_name(&b, 4, 5), b"hello");
    set_e2d_ino(&mut b, 4, 7);
    set_e2d_reclen(&mut b, 4, 12);
    set_e2d_type(&mut b, 4, EXT2_FT_REG_FILE);
    assert_eq!(&b[4..12], &[7, 0, 0, 0, 12, 0, 5, EXT2_FT_REG_FILE]);
    // Past the end: zeroes, and a name cut at the end.
    assert_eq!(
        (e2d_ino(&b, 62), e2d_reclen(&b, 60), e2d_namlen(&b, 64)),
        (0, 0, 0)
    );
    assert_eq!(e2d_name(&b, 50, 10).len(), 6);
}

#[test]
fn the_template_round_trips_through_its_disk_bytes() {
    let t = Ext2fsDirtemplate {
        dot_ino: 12,
        dot_reclen: 12,
        dot_namlen: 1,
        dot_type: EXT2_FT_DIR,
        dot_name: *b".\0\0\0",
        dotdot_ino: 2,
        dotdot_reclen: 1012,
        dotdot_namlen: 2,
        dotdot_type: EXT2_FT_DIR,
        dotdot_name: *b"..\0\0",
    };
    let b = t.to_le_bytes();
    assert_eq!(e2d_ino(&b, 0), 12);
    assert_eq!(e2d_reclen(&b, 12), 1012);
    assert_eq!(e2d_name(&b, 12, 2), b"..");
    let u = Ext2fsDirtemplate::from_le_bytes(&b);
    assert_eq!(u.to_le_bytes(), b);
    assert_eq!((u.dotdot_ino, u.dotdot_reclen), (2, 1012));
}
