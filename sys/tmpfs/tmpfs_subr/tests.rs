//! Host tests for the tmpfs directory sequence numbers on a bare directory node (no vnode,
//! no pools: the entries are leaked boxes linked by hand, as `tmpfs_dir_attach` links them).
//! The node, entry and vnode paths run through a real mount in `tmpfs_vfsops/tests.rs`.

use std::boxed::Box;
use std::{assert, assert_eq, assert_ne};

use super::*;

/// A fresh directory node, as `tmpfs_alloc_node` leaves one.
fn dir() -> &'static TmpfsNode {
    let node: &'static TmpfsNode = Box::leak(Box::new(TmpfsNode::new()));
    node.tn_type.set(VDIR);
    node.tn_spec.tn_dir.tn_next_seq.set(TMPFS_DIRSEQ_START);
    node
}

/// Links a new entry at the end of `dnode`, numbered as `tmpfs_dir_attach` numbers it.
fn link(dnode: &'static TmpfsNode) -> &'static TmpfsDirent {
    let de: &'static TmpfsDirent = Box::leak(Box::new(TmpfsDirent::new()));
    de.td_seq.set(TMPFS_DIRSEQ_NONE);
    de.td_seq.set(tmpfs_dir_getseq(dnode, de));
    // SAFETY: a fresh, leaked entry on no list.
    unsafe { dnode.tn_spec.tn_dir.tn_dir.insert_tail(de) };
    dnode
        .tn_size
        .set(dnode.tn_size.get() + size_of::<TmpfsDirent>() as Off);
    de
}

/// Unlinks an entry as `tmpfs_dir_detach` does.
fn unlink(dnode: &'static TmpfsNode, de: &'static TmpfsDirent) {
    // SAFETY: the entry is on the directory's list.
    unsafe { dnode.tn_spec.tn_dir.tn_dir.remove(de) };
    dnode
        .tn_size
        .set(dnode.tn_size.get() - size_of::<TmpfsDirent>() as Off);
    tmpfs_dir_putseq(dnode, de);
}

#[test]
fn sequence_numbers_start_after_the_reserved_ones_and_step_back() {
    let d = dir();
    let a = link(d);
    let b = link(d);
    let c = link(d);
    assert_eq!([a.td_seq.get(), b.td_seq.get(), c.td_seq.get()], [3, 4, 5]);
    assert_eq!(tmpfs_dir_getseq(d, b), 4, "a number once given stays");

    // removing the last one given steps the counter back; a hole in the middle does not
    unlink(d, c);
    assert_eq!(c.td_seq.get(), TMPFS_DIRSEQ_NONE);
    assert_eq!(d.tn_spec.tn_dir.tn_next_seq.get(), 5);
    unlink(d, a);
    assert_eq!(d.tn_spec.tn_dir.tn_next_seq.get(), 5);
    assert_eq!(link(d).td_seq.get(), 5);

    // lookups by number: the readdir cache first, then the list
    assert!(tmpfs_dir_lookupbyseq(d, 4).is_some_and(|de| ptr::eq(de, b)));
    d.tn_spec.tn_dir.tn_readdir_lastp.set(Some(b));
    assert!(tmpfs_dir_lookupbyseq(d, 4).is_some_and(|de| ptr::eq(de, b)));
    assert!(tmpfs_dir_lookupbyseq(d, 3).is_none());
    assert!(!tmpfs_dirseq_full(d));
}

#[test]
fn an_emptied_directory_restarts_its_numbers() {
    let d = dir();
    let a = link(d);
    let b = link(d);
    unlink(d, a);
    unlink(d, b);
    assert_eq!(d.tn_size.get(), 0);
    assert_eq!(d.tn_spec.tn_dir.tn_next_seq.get(), TMPFS_DIRSEQ_START);
}

#[test]
fn a_full_directory_takes_no_more_entries() {
    let d = dir();
    d.tn_spec.tn_dir.tn_next_seq.set(TMPFS_DIRSEQ_END);
    assert!(tmpfs_dirseq_full(d));
}

#[test]
fn update_sets_the_requested_times() {
    let node = TmpfsNode::new();
    let never = Timespec::new(-1, 0);
    node.tn_atime.set(never);
    node.tn_mtime.set(never);
    node.tn_ctime.set(never);
    tmpfs_update(&node, TMPFS_NODE_MODIFIED);
    assert_eq!(node.tn_atime.get(), never);
    assert_eq!(node.tn_ctime.get(), never);
    assert_ne!(node.tn_mtime.get(), never);
    tmpfs_update(&node, TMPFS_NODE_STATUSALL);
    assert_eq!(node.tn_atime.get(), node.tn_ctime.get());
    assert_eq!(node.tn_atime.get(), node.tn_mtime.get());
}

#[test]
fn the_page_cache_bookkeeping_pairs_number_and_address() {
    let node = TmpfsNode::new();
    assert!(!tmpfs_uio_cached(&node));
    assert_eq!(tmpfs_uio_lookup(&node, 0), None);
    tmpfs_uio_cache(&node, PAGE_SIZE as Voff, 0x1000_0000);
    assert!(tmpfs_uio_cached(&node));
    assert_eq!(
        tmpfs_uio_lookup(&node, PAGE_SIZE as Voff),
        Some(0x1000_0000)
    );
    assert_eq!(tmpfs_uio_lookup(&node, 0), None);
}
