//! Host tests for the tmpfs memory accounting: the per-mount limit, the global limit of the
//! mounts without one, the name buffers' quantum and the node/entry pools.

use std::sync::MutexGuard;
use std::{assert, assert_eq};

use super::*;
use crate::kern::kern_rwlock::{rw_enter_read, rw_exit_read};
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::mount::{MNT_LOCAL, MOUNT_TMPFS, TmpfsArgs, Vfsconf};
use crate::sys::param::PAGE_SIZE;
use crate::tmpfs::tmpfs_vfsops::{TMPFS_VFSOPS, tmpfs_init};

/// The configuration entry `tmpfs_init` receives.
static TMPFS_CONF: Vfsconf =
    Vfsconf::new(&TMPFS_VFSOPS, MOUNT_TMPFS, 19, MNT_LOCAL, TmpfsArgs::SIZE);

/// Real memory, the tmpfs pools, and the global limit set to `limit` bytes with nothing used.
fn setup(limit: u64) -> MutexGuard<'static, ()> {
    let guard = setup_real_memory();
    let _ = tmpfs_init(&TMPFS_CONF);
    TMPFS_BYTES_LIMIT.store(limit, Ordering::Relaxed);
    TMPFS_BYTES_USED.store(0, Ordering::Relaxed);
    guard
}

/// A mount with a memory limit of `memlimit` bytes (0: the global limit) and room for
/// `nodes` nodes.
fn mount(memlimit: u64, nodes: u32) -> TmpfsMount {
    let tmp = TmpfsMount::new();
    tmp.tm_nodes_max.set(nodes);
    tmpfs_mntmem_init(&tmp, memlimit);
    tmp
}

#[test]
fn a_limited_mount_counts_against_its_own_limit() {
    let _g = setup(64 * PAGE_SIZE as u64);
    let tmp = mount(2 * PAGE_SIZE as u64, 10);

    assert!(tmpfs_mem_incr(&tmp, PAGE_SIZE));
    assert!(
        !tmpfs_mem_incr(&tmp, PAGE_SIZE + 1),
        "past the mount's limit"
    );
    assert_eq!(tmp.tm_bytes_used.get(), PAGE_SIZE as u64);
    assert_eq!(
        TMPFS_BYTES_USED.load(Ordering::Relaxed),
        0,
        "a limited mount reserved its limit at mount time"
    );
    assert_eq!(tmpfs_pages_total(&tmp), 2);
    rw_enter_read(&tmp.tm_acc_lock);
    assert_eq!(tmpfs_pages_avail(&tmp), 1);
    rw_exit_read(&tmp.tm_acc_lock);

    tmpfs_mem_decr(&tmp, PAGE_SIZE);
    assert_eq!(tmp.tm_bytes_used.get(), 0);
    tmpfs_mntmem_destroy(&tmp);
}

#[test]
fn unlimited_mounts_share_the_global_limit() {
    let _g = setup(4 * PAGE_SIZE as u64);
    let a = mount(0, 10);
    let b = mount(0, 10);

    assert!(tmpfs_mem_incr(&a, 3 * PAGE_SIZE));
    assert_eq!(
        TMPFS_BYTES_USED.load(Ordering::Relaxed),
        3 * PAGE_SIZE as u64
    );
    assert!(!tmpfs_mem_incr(&b, 2 * PAGE_SIZE), "a used most of it");
    assert!(tmpfs_mem_incr(&b, PAGE_SIZE));
    assert_eq!(tmpfs_pages_total(&a), 3, "the free memory plus a's own");
    rw_enter_read(&b.tm_acc_lock);
    assert_eq!(tmpfs_pages_avail(&b), 0);
    rw_exit_read(&b.tm_acc_lock);

    tmpfs_mem_decr(&a, 3 * PAGE_SIZE);
    tmpfs_mem_decr(&b, PAGE_SIZE);
    assert_eq!(TMPFS_BYTES_USED.load(Ordering::Relaxed), 0);
}

#[test]
fn names_are_accounted_in_quanta() {
    let _g = setup(64 * PAGE_SIZE as u64);
    let tmp = mount(PAGE_SIZE as u64, 10);

    let a = tmpfs_strname_alloc(&tmp, 1).expect("a name");
    assert_eq!(tmp.tm_bytes_used.get(), 32);
    let b = tmpfs_strname_alloc(&tmp, 33).expect("a name");
    assert_eq!(tmp.tm_bytes_used.get(), 32 + 64);
    tmpfs_strname_free(&tmp, a, 1);
    tmpfs_strname_free(&tmp, b, 33);
    assert_eq!(tmp.tm_bytes_used.get(), 0);
}

#[test]
fn nodes_are_limited_in_number_and_memory() {
    let _g = setup(64 * PAGE_SIZE as u64);
    let tmp = mount(PAGE_SIZE as u64, 2);

    let a = tmpfs_node_get(&tmp).expect("a node");
    let b = tmpfs_node_get(&tmp).expect("a node");
    assert!(tmpfs_node_get(&tmp).is_none(), "tm_nodes_max reached");
    assert_eq!(tmp.tm_nodes_cnt.get(), 2);
    assert_eq!(tmp.tm_bytes_used.get(), 2 * size_of::<TmpfsNode>() as u64);
    tmpfs_node_put(&tmp, a);
    tmpfs_node_put(&tmp, b);
    assert_eq!(tmp.tm_nodes_cnt.get(), 0);

    let de = tmpfs_dirent_get(&tmp).expect("an entry");
    assert!(de.td_node.get().is_none() && de.td_name.get().is_null());
    tmpfs_dirent_put(&tmp, de);
    assert_eq!(tmp.tm_bytes_used.get(), 0);

    // A mount whose memory cannot hold one more node.
    let small = mount(PAGE_SIZE as u64, 100);
    assert!(tmpfs_mem_incr(&small, PAGE_SIZE - 1));
    assert!(tmpfs_node_get(&small).is_none());
    assert_eq!(small.tm_nodes_cnt.get(), 0, "the count is given back");
    tmpfs_mem_decr(&small, PAGE_SIZE - 1);
}

#[test]
fn rename_needs_a_new_name_only_when_the_names_differ() {
    let mut f = Componentname::new();
    let mut t = Componentname::new();
    let (a, b, c) = (b"name", b"name", b"other");
    f.cn_nameptr = a.as_ptr();
    f.cn_namelen = 4;
    t.cn_nameptr = b.as_ptr();
    t.cn_namelen = 4;
    assert!(!tmpfs_strname_neqlen(&f, &t));
    t.cn_nameptr = c.as_ptr();
    t.cn_namelen = 5;
    assert!(tmpfs_strname_neqlen(&f, &t));
    assert_eq!(roundup2(33, TMPFS_NAME_QUANTUM), 64);
}
