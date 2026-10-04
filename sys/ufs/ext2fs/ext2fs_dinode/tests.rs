use super::*;

fn fs(rev: u32, isize_: u16) -> MExt2fs {
    let fs = MExt2fs::new();
    fs.set_e2fs_rev(rev);
    fs.set_e2fs_inode_size(isize_);
    fs
}

#[test]
fn dinode_size_follows_the_revision() {
    assert_eq!(ext2_dinode_size(&fs(0, 256)), 128);
    assert_eq!(ext2_dinode_size(&fs(1, 256)), 256);
    assert_eq!(ext2_dinode_size(&fs(1, 128)), 128);
}

#[test]
fn load_and_save_copy_the_smaller_of_the_two_sizes() {
    let mut img = [0u8; 256];
    for (i, b) in img.iter_mut().enumerate() {
        *b = i as u8;
    }
    let mut d = Ext2fsDinode::default();
    e2fs_iload(&fs(1, 128), &img, &mut d);
    assert_eq!(d.e2di_mode, 0x0100);
    assert_eq!(d.e2di_blocks[0], 0x2b2a_2928);
    assert_eq!(d.e2di_isize, 0, "past 128 bytes: untouched");
    e2fs_iload(&fs(1, 256), &img, &mut d);
    assert_eq!(d.e2di_isize, 0x8180);
    assert_eq!(d.e2di_version_hi, 0x9b9a_9998);
    let mut out = [0xffu8; 256];
    e2fs_isave(&fs(1, 256), &d, &mut out);
    assert_eq!(out[..156], img[..156]);
    assert_eq!(out[156], 0xff);
    let mut out = [0xffu8; 256];
    e2fs_isave(&fs(0, 256), &d, &mut out);
    assert_eq!(out[..128], img[..128]);
    assert_eq!(out[128], 0xff);
}

#[test]
fn rdev_and_shortlink_overlay_the_block_pointers() {
    let mut d = Ext2fsDinode::default();
    d.e2di_shortlink_mut()[..5].copy_from_slice(b"/tmp\0");
    assert_eq!(&d.e2di_shortlink()[..5], b"/tmp\0");
    assert_eq!(d.e2di_shortlink().len(), 60);
    d.set_e2di_rdev(0x0102);
    assert_eq!(d.e2di_blocks[0], 0x0102);
    assert_eq!(d.e2di_rdev(), 0x0102);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn constants_match_the_c_header() {
    let defs = crate::reftest::defines("sys/ufs/ext2fs/ext2fs_dinode.h");
    for (name, value) in [
        ("NDADDR", NDADDR as i64),
        ("NIADDR", NIADDR as i64),
        ("EXT2_IEXEC", i64::from(EXT2_IEXEC)),
        ("EXT2_IWRITE", i64::from(EXT2_IWRITE)),
        ("EXT2_IREAD", i64::from(EXT2_IREAD)),
        ("EXT2_ISVTX", i64::from(EXT2_ISVTX)),
        ("EXT2_ISGID", i64::from(EXT2_ISGID)),
        ("EXT2_ISUID", i64::from(EXT2_ISUID)),
        ("EXT2_IFMT", i64::from(EXT2_IFMT)),
        ("EXT2_IFIFO", i64::from(EXT2_IFIFO)),
        ("EXT2_IFCHR", i64::from(EXT2_IFCHR)),
        ("EXT2_IFDIR", i64::from(EXT2_IFDIR)),
        ("EXT2_IFBLK", i64::from(EXT2_IFBLK)),
        ("EXT2_IFREG", i64::from(EXT2_IFREG)),
        ("EXT2_IFLNK", i64::from(EXT2_IFLNK)),
        ("EXT2_IFSOCK", i64::from(EXT2_IFSOCK)),
        ("EXT2_SECRM", i64::from(EXT2_SECRM)),
        ("EXT2_UNRM", i64::from(EXT2_UNRM)),
        ("EXT2_COMPR", i64::from(EXT2_COMPR)),
        ("EXT2_SYNC", i64::from(EXT2_SYNC)),
        ("EXT2_IMMUTABLE", i64::from(EXT2_IMMUTABLE)),
        ("EXT2_APPEND", i64::from(EXT2_APPEND)),
        ("EXT2_NODUMP", i64::from(EXT2_NODUMP)),
        ("EXT2_NOATIME", i64::from(EXT2_NOATIME)),
        ("EXT4_INDEX", i64::from(EXT4_INDEX)),
        ("EXT4_JOURNAL_DATA", i64::from(EXT4_JOURNAL_DATA)),
        ("EXT4_DIRSYNC", i64::from(EXT4_DIRSYNC)),
        ("EXT4_TOPDIR", i64::from(EXT4_TOPDIR)),
        ("EXT4_HUGE_FILE", i64::from(EXT4_HUGE_FILE)),
        ("EXT4_EXTENTS", i64::from(EXT4_EXTENTS)),
        ("EXT4_EOFBLOCKS", i64::from(EXT4_EOFBLOCKS)),
        ("EXT2_REV0_DINODE_SIZE", EXT2_REV0_DINODE_SIZE as i64),
    ] {
        assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
    }
}
