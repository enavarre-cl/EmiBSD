//! Host tests for the allocator's pure parts: the bitmap search (`ext2fs_mapsearch`), the
//! preferred block (`ext2fs_blkpref`), the directory group (`ext2fs_dirpref`) and the
//! order `ext2fs_hashalloc` tries the groups in. The bitmap allocations themselves
//! (`ext2fs_alloccg`, `ext2fs_nodealloccg`, the frees) run in the read-write mount test of
//! `ext2fs_vfsops`.

use std::boxed::Box;
use std::sync::Mutex;
use std::vec::Vec;
use std::{assert_eq, vec};

use super::*;
use crate::ufs::ext2fs::ext2fs::Ext2Gd;

/// A file system of `ncg` groups of 64 blocks (from block 1) and 32 inodes, with these
/// group descriptors.
fn fs(ncg: i32, gds: Vec<Ext2Gd>) -> &'static MExt2fs {
    let fs: &'static MExt2fs = Box::leak(Box::new(MExt2fs::new()));
    fs.set_e2fs_fpg(64);
    fs.set_e2fs_bpg(64);
    fs.set_e2fs_first_dblock(1);
    fs.set_e2fs_ipg(32);
    fs.e2fs_ncg.set(ncg);
    if !gds.is_empty() {
        fs.e2fs_gd.set(gds.leak().as_mut_ptr());
    }
    fs
}

/// An inode numbered `ino` on `fs`.
fn inode(fs: &'static MExt2fs, ino: Ufsino) -> &'static Inode {
    let ip: &'static Inode = Box::leak(Box::new(Inode::new()));
    ip.i_e2fs.set(Some(fs));
    ip.i_number.set(ino);
    ip
}

#[test]
fn mapsearch_finds_the_first_clear_bit_from_the_preference() {
    let fs = fs(1, vec![]);
    let mut map = [0xffu8, 0xff, 0x0f, 0, 0, 0, 0, 0];
    assert_eq!(ext2fs_mapsearch(fs, &map, 0), 20);
    // From byte 3 (block 1 + 25 is bit 25).
    assert_eq!(ext2fs_mapsearch(fs, &map, 26), 24);
    // Nothing from the preference to the end: search again from the start.
    map = [0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    assert_eq!(ext2fs_mapsearch(fs, &map, 34), 7);
}

#[test]
fn blkpref_follows_the_last_block_then_the_pointers_then_the_group() {
    let fs = fs(2, vec![]);
    let ip = inode(fs, 40); // group 1
    ip.i_e2fs_last_blk().set(100);
    ip.i_e2fs_last_lblk().set(4);
    assert_eq!(ext2fs_blkpref(ip, 5, 0, None), 101);

    let mut bap = [0u8; 12];
    bap[..4].copy_from_slice(&10u32.to_le_bytes());
    assert_eq!(ext2fs_blkpref(ip, 7, 2, Some(&bap)), 11);
    bap[8..].copy_from_slice(&30u32.to_le_bytes());
    assert_eq!(ext2fs_blkpref(ip, 7, 2, Some(&bap)), 31);
    assert_eq!(ext2fs_blkpref(ip, 7, 1, Some(&bap)), 11);
    assert_eq!(ext2fs_blkpref(ip, 7, 2, Some(&[0; 12])), 64 + 1 + 1);
    assert_eq!(ext2fs_blkpref(ip, 7, 0, None), 64 + 1 + 1);
}

#[test]
fn dirpref_picks_the_roomiest_group_with_enough_free_inodes() {
    let gd = |nifree, nbfree| Ext2Gd {
        ext2bgd_nifree: nifree,
        ext2bgd_nbfree: nbfree,
        ..Ext2Gd::default()
    };
    let fs = fs(3, vec![gd(5, 10), gd(20, 3), gd(20, 7)]);
    fs.set_e2fs_ficount(45);
    assert_eq!(ext2fs_dirpref(fs), 2);
    fs.set_e2fs_ficount(3);
    assert_eq!(ext2fs_dirpref(fs), 0);
}

/// The groups the recording allocators were asked for.
static VISITED: Mutex<Vec<i32>> = Mutex::new(Vec::new());

fn never(_ip: &Inode, cg: i32, _pref: u32, _size: i32) -> u32 {
    VISITED.lock().unwrap().push(cg);
    0
}

fn in_group_4(_ip: &Inode, cg: i32, _pref: u32, _size: i32) -> u32 {
    VISITED.lock().unwrap().push(cg);
    if cg == 4 { 77 } else { 0 }
}

#[test]
fn hashalloc_tries_the_group_then_rehashes_then_every_group() {
    let ip = inode(fs(5, vec![]), 1);
    VISITED.lock().unwrap().clear();
    assert_eq!(ext2fs_hashalloc(ip, 1, 9, 1024, never), 0);
    assert_eq!(*VISITED.lock().unwrap(), [1, 2, 4, 3, 3, 4, 0]);
    VISITED.lock().unwrap().clear();
    assert_eq!(ext2fs_hashalloc(ip, 1, 9, 1024, in_group_4), 77);
    assert_eq!(*VISITED.lock().unwrap(), [1, 2, 4]);
}
