use super::*;

fn fs() -> MExt2fs {
    let fs = MExt2fs::new();
    fs.with_e2fs_mut(|e| {
        e.e2fs_ipg = 8;
        e.e2fs_fpg = 100;
        e.e2fs_first_dblock = 1;
        e.e2fs_fbcount = 50;
        e.e2fs_rbcount = 5;
    });
    fs.e2fs_ipb.set(4);
    fs.e2fs_bsize.set(1024);
    fs.e2fs_bshift.set(10);
    fs.e2fs_bmask.set(!1023);
    fs.e2fs_qbmask.set(1023);
    fs.e2fs_fsbtodb.set(1);
    fs
}

#[test]
fn sector_and_block_arithmetic() {
    let fs = fs();
    assert_eq!(fsbtodb(&fs, 7), 14);
    assert_eq!(dbtofsb(&fs, 14), 7);
    assert_eq!(lblktosize(&fs, 3), 3072);
    assert_eq!(lblkno(&fs, 3073), 3);
    assert_eq!(blkoff(&fs, 3073), 1);
    assert_eq!(blkroundup(&fs, 1025), 2048);
    assert_eq!(blkroundup(&fs, 1024), 1024);
    assert_eq!(fragroundup(&fs, 1), 1024);
    assert_eq!(freespace(&fs), 45);
    assert_eq!(nindir(&fs), 256);
    assert_eq!(dtog(&fs, 101), 1);
    assert_eq!(dtogd(&fs, 101), 0);
    assert_eq!(dtog(&fs, 100), 0);
    fs.e2fs_maxfilesize.set(10);
    assert!(!e2fs_overflow(&fs, 0, 5));
    assert!(e2fs_overflow(&fs, 0, 11));
    assert!(e2fs_overflow(&fs, 0, -1));
}

#[test]
fn inode_numbers_map_to_groups_and_blocks() {
    let fs = fs();
    let mut gds = [Ext2Gd::default(); 3];
    gds[0].ext2bgd_i_tables = 10;
    gds[1].ext2bgd_i_tables = 20;
    gds[2].ext2bgd_i_tables = 30;
    fs.e2fs_ncg.set(3);
    fs.e2fs_gd.set(gds.as_mut_ptr());
    // Inode 1 is the first of group 0; inode 9 the first of group 1; inode 20 the 4th
    // of group 2 (index 3, in the second inode block: ipb is 4).
    assert_eq!(ino_to_cg(&fs, 1), 0);
    assert_eq!(ino_to_cg(&fs, 8), 0);
    assert_eq!(ino_to_cg(&fs, 9), 1);
    assert_eq!(ino_to_fsba(&fs, 9), 20);
    assert_eq!(ino_to_fsba(&fs, 13), 21);
    assert_eq!(ino_to_cg(&fs, 20), 2);
    assert_eq!(ino_to_fsba(&fs, 20), 30);
    assert_eq!(ino_to_fsbo(&fs, 20), 3);
    fs.with_gd_mut(1, |g| g.ext2bgd_nbfree = 9);
    assert_eq!(fs.gd(1).ext2bgd_nbfree, 9);
    fs.e2fs_gd.set(ptr::null_mut());
}

#[test]
fn sparse_super_groups() {
    let has: [i32; 12] = [0, 1, 3, 5, 7, 9, 25, 27, 49, 81, 125, 243];
    for i in 0..250 {
        assert_eq!(cg_has_sb(i), has.contains(&i) || i == 343, "{i}");
    }
    assert!(cg_has_sb(343));
    assert!(!cg_has_sb(2));
    assert!(!cg_has_sb(15));
}

#[test]
fn super_block_load_save_round_trip() {
    let mut img = [0u8; SBSIZE];
    img[0..4].copy_from_slice(&1234u32.to_le_bytes());
    img[56..58].copy_from_slice(&E2FS_MAGIC.to_le_bytes());
    img[76..80].copy_from_slice(&E2FS_REV1.to_le_bytes());
    img[1020..1024].copy_from_slice(&0xdead_beefu32.to_le_bytes());
    let sb = Ext2fs::from_disk(&img);
    assert_eq!(sb.e2fs_icount, 1234);
    assert_eq!(sb.e2fs_magic, E2FS_MAGIC);
    assert_eq!(sb.e2fs_rev, E2FS_REV1);
    assert_eq!(sb.e2fs_sbchksum, 0xdead_beef);
    let mut out = [0u8; SBSIZE];
    e2fs_sbsave(&sb, &mut out);
    assert_eq!(out, img);
}

#[test]
fn group_descriptors_load_save_round_trip() {
    let mut img = [0u8; 80];
    for (i, b) in img.iter_mut().enumerate() {
        *b = i as u8;
    }
    let mut gds = [Ext2Gd::default(); 3];
    e2fs_cgload(&img, &mut gds, 80);
    assert_eq!(gds[0].ext2bgd_b_bitmap, 0x0302_0100);
    assert_eq!(gds[0].ext2bgd_nbfree, 0x0d0c);
    assert_eq!(gds[1].ext2bgd_i_tables, 0x2b2a_2928);
    let mut out = [0u8; 80];
    e2fs_cgsave(&gds, &mut out, 80);
    assert_eq!(out, img);
}

#[test]
fn feature_tables_name_every_flag() {
    assert_eq!(RO_COMPAT.iter().fold(0, |m, f| m | f.mask), 0x377f);
    assert_eq!(INCOMPAT.iter().fold(0, |m, f| m | f.mask), 0x1f7df);
    assert_eq!(EXT2F_ROCOMPAT_SUPP, 3);
    assert_eq!(EXT2F_INCOMPAT_SUPP, 2);
    assert_eq!(EXT4F_RO_INCOMPAT_SUPP, 0x254);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn constants_match_the_c_header() {
    let defs = crate::reftest::defines("sys/ufs/ext2fs/ext2fs.h");
    for (name, value) in [
        ("BBSIZE", BBSIZE as i64),
        ("SBSIZE", SBSIZE as i64),
        ("LOG_MINBSIZE", i64::from(LOG_MINBSIZE)),
        ("MINBSIZE", MINBSIZE as i64),
        ("LOG_MINFSIZE", i64::from(LOG_MINFSIZE)),
        ("MINFSIZE", MINFSIZE as i64),
        ("MAXMNTLEN", MAXMNTLEN as i64),
        ("MINFREE", i64::from(MINFREE)),
        ("E2FS_MAGIC", i64::from(E2FS_MAGIC)),
        ("E2FS_REV0", i64::from(E2FS_REV0)),
        ("E2FS_REV1", i64::from(E2FS_REV1)),
        ("EXT2F_COMPAT_PREALLOC", i64::from(EXT2F_COMPAT_PREALLOC)),
        (
            "EXT2F_COMPAT_IMAGIC_INODES",
            i64::from(EXT2F_COMPAT_IMAGIC_INODES),
        ),
        (
            "EXT2F_COMPAT_HAS_JOURNAL",
            i64::from(EXT2F_COMPAT_HAS_JOURNAL),
        ),
        ("EXT2F_COMPAT_EXT_ATTR", i64::from(EXT2F_COMPAT_EXT_ATTR)),
        ("EXT2F_COMPAT_RESIZE", i64::from(EXT2F_COMPAT_RESIZE)),
        ("EXT2F_COMPAT_DIR_INDEX", i64::from(EXT2F_COMPAT_DIR_INDEX)),
        (
            "EXT2F_COMPAT_SPARSE_SUPER2",
            i64::from(EXT2F_COMPAT_SPARSE_SUPER2),
        ),
        (
            "EXT2F_ROCOMPAT_SPARSE_SUPER",
            i64::from(EXT2F_ROCOMPAT_SPARSE_SUPER),
        ),
        (
            "EXT2F_ROCOMPAT_LARGE_FILE",
            i64::from(EXT2F_ROCOMPAT_LARGE_FILE),
        ),
        (
            "EXT2F_ROCOMPAT_BTREE_DIR",
            i64::from(EXT2F_ROCOMPAT_BTREE_DIR),
        ),
        (
            "EXT2F_ROCOMPAT_HUGE_FILE",
            i64::from(EXT2F_ROCOMPAT_HUGE_FILE),
        ),
        (
            "EXT2F_ROCOMPAT_GDT_CSUM",
            i64::from(EXT2F_ROCOMPAT_GDT_CSUM),
        ),
        (
            "EXT2F_ROCOMPAT_DIR_NLINK",
            i64::from(EXT2F_ROCOMPAT_DIR_NLINK),
        ),
        (
            "EXT2F_ROCOMPAT_EXTRA_ISIZE",
            i64::from(EXT2F_ROCOMPAT_EXTRA_ISIZE),
        ),
        ("EXT2F_ROCOMPAT_QUOTA", i64::from(EXT2F_ROCOMPAT_QUOTA)),
        (
            "EXT2F_ROCOMPAT_BIGALLOC",
            i64::from(EXT2F_ROCOMPAT_BIGALLOC),
        ),
        (
            "EXT2F_ROCOMPAT_METADATA_CKSUM",
            i64::from(EXT2F_ROCOMPAT_METADATA_CKSUM),
        ),
        (
            "EXT2F_ROCOMPAT_READONLY",
            i64::from(EXT2F_ROCOMPAT_READONLY),
        ),
        ("EXT2F_ROCOMPAT_PROJECT", i64::from(EXT2F_ROCOMPAT_PROJECT)),
        ("EXT2F_INCOMPAT_COMP", i64::from(EXT2F_INCOMPAT_COMP)),
        ("EXT2F_INCOMPAT_FTYPE", i64::from(EXT2F_INCOMPAT_FTYPE)),
        ("EXT2F_INCOMPAT_RECOVER", i64::from(EXT2F_INCOMPAT_RECOVER)),
        (
            "EXT2F_INCOMPAT_JOURNAL_DEV",
            i64::from(EXT2F_INCOMPAT_JOURNAL_DEV),
        ),
        ("EXT2F_INCOMPAT_META_BG", i64::from(EXT2F_INCOMPAT_META_BG)),
        ("EXT2F_INCOMPAT_EXTENTS", i64::from(EXT2F_INCOMPAT_EXTENTS)),
        ("EXT2F_INCOMPAT_64BIT", i64::from(EXT2F_INCOMPAT_64BIT)),
        ("EXT2F_INCOMPAT_MMP", i64::from(EXT2F_INCOMPAT_MMP)),
        ("EXT2F_INCOMPAT_FLEX_BG", i64::from(EXT2F_INCOMPAT_FLEX_BG)),
        (
            "EXT2F_INCOMPAT_EA_INODE",
            i64::from(EXT2F_INCOMPAT_EA_INODE),
        ),
        ("EXT2F_INCOMPAT_DIRDATA", i64::from(EXT2F_INCOMPAT_DIRDATA)),
        (
            "EXT2F_INCOMPAT_CSUM_SEED",
            i64::from(EXT2F_INCOMPAT_CSUM_SEED),
        ),
        (
            "EXT2F_INCOMPAT_LARGEDIR",
            i64::from(EXT2F_INCOMPAT_LARGEDIR),
        ),
        (
            "EXT2F_INCOMPAT_INLINE_DATA",
            i64::from(EXT2F_INCOMPAT_INLINE_DATA),
        ),
        ("EXT2F_INCOMPAT_ENCRYPT", i64::from(EXT2F_INCOMPAT_ENCRYPT)),
        ("EXT2F_COMPAT_SUPP", i64::from(EXT2F_COMPAT_SUPP)),
        ("EXT2F_INCOMPAT_SUPP", i64::from(EXT2F_INCOMPAT_SUPP)),
        ("E2FS_BEH_CONTINUE", i64::from(E2FS_BEH_CONTINUE)),
        ("E2FS_BEH_READONLY", i64::from(E2FS_BEH_READONLY)),
        ("E2FS_BEH_PANIC", i64::from(E2FS_BEH_PANIC)),
        ("E2FS_BEH_DEFAULT", i64::from(E2FS_BEH_DEFAULT)),
        ("E2FS_OS_LINUX", i64::from(E2FS_OS_LINUX)),
        ("E2FS_OS_HURD", i64::from(E2FS_OS_HURD)),
        ("E2FS_OS_MASIX", i64::from(E2FS_OS_MASIX)),
        ("E2FS_ISCLEAN", i64::from(E2FS_ISCLEAN)),
        ("E2FS_ERRORS", i64::from(E2FS_ERRORS)),
    ] {
        assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
    }
}
