//! Host tests for the extent tree: the constants against the C header, `ext4_ext_find_extent`
//! over a root node held in an in-memory inode (the deeper levels, read through the buffer
//! cache, are exercised by the mount tests of `ext2fs_vfsops`), and the extent cache.

use std::boxed::Box;
use std::{assert, assert_eq};

use super::*;

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn constants_match_the_c_header() {
    let defs = crate::reftest::defines("sys/ufs/ext2fs/ext2fs_extents.h");
    for (name, value) in [
        ("EXT4_EXT_MAGIC", i64::from(EXT4_EXT_MAGIC)),
        ("EXT4_EXT_CACHE_NO", i64::from(EXT4_EXT_CACHE_NO)),
        ("EXT4_EXT_CACHE_GAP", i64::from(EXT4_EXT_CACHE_GAP)),
        ("EXT4_EXT_CACHE_IN", i64::from(EXT4_EXT_CACHE_IN)),
    ] {
        assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
    }
}

/// An inode whose block array is an extent tree root: `ecount` entries of `extents`
/// (`(first logical block, length, first disk block)`), with `magic`.
fn inode(magic: u16, ecount: u16, extents: &[(u32, u16, u32)]) -> &'static Inode {
    let mut blocks = [0u32; NDADDR + NIADDR];
    blocks[0] = u32::from(magic) | u32::from(ecount) << 16;
    blocks[1] = 4; // eh_max, eh_depth 0
    for (i, &(blk, len, start)) in extents.iter().enumerate() {
        blocks[3 + 3 * i] = blk;
        blocks[4 + 3 * i] = u32::from(len);
        blocks[5 + 3 * i] = start;
    }
    let din: &'static mut Ext2fsDinode = Box::leak(Box::new(Ext2fsDinode {
        e2di_blocks: blocks,
        ..Ext2fsDinode::default()
    }));
    let ip: &'static Inode = Box::leak(Box::new(Inode::new()));
    ip.dinode_u.set(ptr::from_mut(din).cast());
    ip
}

/// `ext4_ext_find_extent` at depth 0: the extent found, if any.
fn find(ip: &Inode, lbn: Daddr) -> Option<Option<Ext4Extent>> {
    let fs = MExt2fs::new();
    let mut path = Ext4ExtentPath::new();
    ext4_ext_find_extent(&fs, ip, lbn, &mut path).map(|p| {
        assert!(p.ep_bp.is_none());
        assert_eq!(p.ep_depth, 0);
        p.ext()
    })
}

#[test]
fn the_root_leaf_is_searched_for_the_extent_at_or_before_a_block() {
    let e = |blk, len, start| Ext4Extent {
        e_blk: blk,
        e_len: len,
        e_start_hi: 0,
        e_start_lo: start,
    };
    let ip = inode(
        EXT4_EXT_MAGIC,
        3,
        &[(5, 4, 100), (10, 2, 200), (20, 1, 300)],
    );
    assert_eq!(find(ip, 5), Some(Some(e(5, 4, 100))));
    assert_eq!(find(ip, 8), Some(Some(e(5, 4, 100))));
    assert_eq!(find(ip, 10), Some(Some(e(10, 2, 200))));
    // Past an extent's end the C still answers with it; the callers do the arithmetic.
    assert_eq!(find(ip, 15), Some(Some(e(10, 2, 200))));
    assert_eq!(find(ip, 1000), Some(Some(e(20, 1, 300))));

    // Before the first extent: the entry before it, the header read as an extent.
    let fs = MExt2fs::new();
    let mut path = Ext4ExtentPath::new();
    ext4_ext_find_extent(&fs, ip, 2, &mut path);
    assert_eq!(path.ep_ext.cast::<Ext4ExtentHeader>(), path.ep_header);

    // An empty leaf finds nothing; no tree at all is no path.
    assert_eq!(find(inode(EXT4_EXT_MAGIC, 0, &[]), 3), Some(None));
    assert_eq!(find(inode(0xef53, 1, &[(0, 1, 9)]), 0), None);
    // A header claiming more entries than the root holds (4) is a corrupt tree.
    assert_eq!(find(inode(EXT4_EXT_MAGIC, 5, &[(0, 1, 9)]), 0), Some(None));
    assert_eq!(
        find(inode(EXT4_EXT_MAGIC, 4, &[(0, 1, 9)]), 0).map(|e| e.is_some()),
        Some(true)
    );
}

#[test]
fn the_extent_cache_answers_inside_its_range_only() {
    let ip = inode(EXT4_EXT_MAGIC, 0, &[]);
    let mut ep = Ext4Extent::default();
    assert_eq!(ext4_ext_in_cache(ip, 0, &mut ep), EXT4_EXT_CACHE_NO);

    let cached = Ext4Extent {
        e_blk: 10,
        e_len: 2,
        e_start_hi: 1,
        e_start_lo: 5,
    };
    ext4_ext_put_cache(ip, &cached, EXT4_EXT_CACHE_IN);
    assert_eq!(ip.i_e2fs_ext_cache().get().ec_start, (1 << 32) | 5);
    assert_eq!(ext4_ext_in_cache(ip, 11, &mut ep), EXT4_EXT_CACHE_IN);
    assert_eq!(ep, cached);
    let mut other = Ext4Extent::default();
    assert_eq!(ext4_ext_in_cache(ip, 12, &mut other), EXT4_EXT_CACHE_NO);
    assert_eq!(ext4_ext_in_cache(ip, 9, &mut other), EXT4_EXT_CACHE_NO);
    assert_eq!(other, Ext4Extent::default());

    ext4_ext_put_cache(ip, &cached, EXT4_EXT_CACHE_GAP);
    assert_eq!(ext4_ext_in_cache(ip, 10, &mut ep), EXT4_EXT_CACHE_GAP);
}
