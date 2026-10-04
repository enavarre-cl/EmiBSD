use super::*;

#[test]
fn super_block_swap_keeps_the_unused_fields_and_round_trips() {
    let mut sb = Ext2fs::new();
    sb.e2fs_icount = 0x0102_0304;
    sb.e2fs_magic = 0xef53;
    sb.e2fs_rev = 1;
    sb.e2fs_features_incompat = 0x0002;
    sb.e2fs_mkfs_time = 0xaabb_ccdd;
    sb.e2fs_uuid = [7; 16];
    sb.e2fs_sbchksum = 0x1122_3344;
    let mut sw = Ext2fs::new();
    e2fs_sb_bswap(&sb, &mut sw);
    assert_eq!(sw.e2fs_icount, 0x0403_0201);
    assert_eq!(sw.e2fs_magic, 0x53ef);
    assert_eq!(sw.e2fs_rev, 0x0100_0000);
    assert_eq!(sw.e2fs_features_incompat, 0x0200_0000);
    assert_eq!(sw.e2fs_mkfs_time, 0xddcc_bbaa);
    assert_eq!(sw.e2fs_uuid, [7; 16]);
    assert_eq!(sw.e2fs_sbchksum, 0x1122_3344, "the checksum is not swapped");
    let mut back = Ext2fs::new();
    e2fs_sb_bswap(&sw, &mut back);
    assert_eq!(back.e2fs_icount, sb.e2fs_icount);
    assert_eq!(back.e2fs_magic, sb.e2fs_magic);
    assert_eq!(back.e2fs_mkfs_time, sb.e2fs_mkfs_time);
    assert_eq!(back.e2fs_rev, sb.e2fs_rev);
}

#[test]
fn group_descriptors_swap_by_whole_descriptors_only() {
    let mut old = [Ext2Gd::default(); 3];
    for (i, g) in old.iter_mut().enumerate() {
        g.ext2bgd_b_bitmap = 0x0102_0300 + i as u32;
        g.ext2bgd_i_tables = 0x1000_0000;
        g.ext2bgd_nbfree = 0x0102;
        g.ext2bgd_ndirs = 3;
        g.reserved = 9;
    }
    let mut new = [Ext2Gd::default(); 3];
    e2fs_cg_bswap(&old, &mut new, 2 * 32 + 31);
    assert_eq!(new[0].ext2bgd_b_bitmap, 0x0003_0201);
    assert_eq!(new[1].ext2bgd_b_bitmap, 0x0103_0201);
    assert_eq!(new[1].ext2bgd_i_tables, 0x10);
    assert_eq!(new[1].ext2bgd_nbfree, 0x0201);
    assert_eq!(new[1].ext2bgd_ndirs, 0x0300);
    assert_eq!(new[0].reserved, 0, "reserved is not copied");
    assert_eq!(
        new[2],
        Ext2Gd::default(),
        "a partial descriptor is not swapped"
    );
}

#[test]
fn inode_swap_follows_the_inode_size() {
    let mut old = Ext2fsDinode {
        e2di_mode: 0x81a4,
        e2di_size: 0x0000_1000,
        e2di_isize: 0x0020,
        e2di_x_ctime: 5,
        ..Ext2fsDinode::default()
    };
    old.e2di_blocks[0] = 0x0102_0304;
    let fs = MExt2fs::new();
    fs.set_e2fs_rev(1);
    fs.set_e2fs_inode_size(256);
    let mut new = Ext2fsDinode::default();
    e2fs_i_bswap(&fs, &old, &mut new);
    assert_eq!(new.e2di_mode, 0xa481);
    assert_eq!(new.e2di_size, 0x0010_0000);
    assert_eq!(new.e2di_isize, 0x2000);
    assert_eq!(
        new.e2di_blocks[0], 0x0102_0304,
        "block pointers are copied raw"
    );
    assert_eq!(new.e2di_x_ctime, 0, "the extra fields are not touched");
    fs.set_e2fs_inode_size(128);
    let mut small = Ext2fsDinode::default();
    e2fs_i_bswap(&fs, &old, &mut small);
    assert_eq!(small.e2di_isize, 0);
    assert_eq!(small.e2di_mode, 0xa481);
}
