//! Host tests for the size of an inode (the high 32 bits of a regular file's size, the
//! file system's limit) and the block arithmetic of `ext2fs_truncate`. Truncating through
//! the buffer cache runs in the read-write mount test of `ext2fs_vfsops`.

use std::assert_eq;
use std::boxed::Box;

use super::*;
use crate::ufs::ext2fs::ext2fs::{E2FS_REV1, MExt2fs};
use crate::ufs::ext2fs::ext2fs_dinode::Ext2fsDinode;
use crate::ufs::ufs::dinode::IFDIR;

/// An inode of `mode` with its own dinode, on a file system of revision `rev` whose files
/// may grow to `maxfilesize`.
fn inode(mode: u32, rev: u32, maxfilesize: Off) -> &'static Inode {
    let fs: &'static MExt2fs = Box::leak(Box::new(MExt2fs::new()));
    fs.set_e2fs_rev(rev);
    fs.e2fs_maxfilesize.set(maxfilesize);
    let din: &'static mut Ext2fsDinode = Box::leak(Box::new(Ext2fsDinode::default()));
    din.e2di_mode = mode as u16;
    let ip: &'static Inode = Box::leak(Box::new(Inode::new()));
    ip.i_e2fs.set(Some(fs));
    ip.dinode_u.set(core::ptr::from_mut(din).cast());
    ip
}

#[test]
fn the_high_size_word_counts_for_regular_files_only() {
    let ip = inode(IFREG | 0o644, E2FS_REV1, 1 << 40);
    ext2fs_setsize(ip, (5 << 32) | 7).unwrap();
    assert_eq!((ip.i_e2fs_size(), ip.i_e2fs_size_hi()), (7, 5));
    assert_eq!(ext2fs_size(ip), (5 << 32) | 7);

    let dir = inode(IFDIR | 0o755, E2FS_REV1, 1 << 40);
    dir.set_i_e2fs_size_hi(3);
    ext2fs_setsize(dir, 1024).unwrap();
    assert_eq!((ext2fs_size(dir), dir.i_e2fs_size_hi()), (1024, 3));
}

#[test]
fn growing_past_the_limit_is_efbig_and_asks_for_large_files() {
    let ip = inode(IFREG, E2FS_REV0, 1000);
    assert_eq!(ext2fs_setsize(ip, 1001), Err(Errno::EFBIG));
    assert_eq!(ip.e2fs().e2fs_features_rocompat(), 0);

    let ip = inode(IFREG, E2FS_REV1, 1000);
    assert_eq!(ext2fs_setsize(ip, 1001), Err(Errno::EFBIG));
    assert_eq!(
        ip.e2fs().e2fs_features_rocompat(),
        EXT2F_ROCOMPAT_LARGE_FILE
    );
    assert_eq!(ip.e2fs().e2fs_fmod.get(), 1);
    assert_eq!(ext2fs_size(ip), 0);
}

#[test]
fn truncate_keeps_the_blocks_below_the_new_end() {
    // To nothing: no direct block, no block under any indirect one.
    assert_eq!(
        ext2fs_truncate_lastblocks(-1, 256),
        (-1, [-13, -13 - 256, -13 - 256 - 65536])
    );
    // One byte into logical block 13: blocks 0..=13, so two under the single indirect.
    assert_eq!(
        ext2fs_truncate_lastblocks(13, 256),
        (13, [1, 1 - 256, 1 - 256 - 65536])
    );
    // Into the double indirect range.
    let (_, l) = ext2fs_truncate_lastblocks(12 + 256 + 300, 256);
    assert_eq!((l[SINGLE], l[DOUBLE]), (556, 300));

    assert_eq!(ext2fs_truncate_nblock(6, 4), 2);
    assert_eq!(ext2fs_truncate_nblock(6, 6), 0);
    assert_eq!(ext2fs_truncate_nblock(6, 10), 0);
}
