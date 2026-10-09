/* <LICENSES> */
/*
 * Copyright (c) 2010, 2011 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Mindaugas Rasiukevicius.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! tmpfs memory allocation routines. Implements memory usage accounting and limiting: the
//! per-mount byte counter against its limit (or against the global `tmpfs_bytes_limit` when
//! the mount has none), the node and directory entry pools and the name buffers.
//!
//! Upstream: sys/tmpfs/tmpfs_mem.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `tmpfs_bytes_limit` and `tmpfs_bytes_used` are the atomics [`TMPFS_BYTES_LIMIT`] and
//!   [`TMPFS_BYTES_USED`]: the C updates them under whichever mount's `tm_acc_lock` is held.
//! - `tmpfs_mem_incr` returns `bool` (the C's 1 or 0).
//! - `tmpfs_dirent_get`/`tmpfs_node_get` write a fresh [`TmpfsDirent::new`]/
//!   [`TmpfsNode::new`] into the pool item (Rust needs every member initialised; the C's
//!   node is uninitialised until `tmpfs_alloc_node` fills it in).
//! - `tmpfs_strname_neqlen` compares the two names themselves after their rounded lengths;
//!   the C compares the rounded length's bytes of each path buffer, so it also calls equal
//!   names unequal when what follows them in the paths differs, and may read past the end
//!   of the buffer. Its one caller (`tmpfs_rename`) only uses the answer to decide whether
//!   to allocate a new name buffer.

use core::ptr::NonNull;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::kassert;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{rw_assert_anylock, rw_enter_write, rw_exit_write, rw_init};
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::panic;
use crate::sys::malloc::{M_TEMP, M_WAITOK};
use crate::sys::namei::Componentname;
use crate::sys::param::PAGE_SHIFT;
use crate::sys::pool::{PR_WAITOK, PR_ZERO};
use crate::tmpfs::tmpfs::{TmpfsDirent, TmpfsMount, TmpfsNode};
use crate::tmpfs::tmpfs_vfsops::{TMPFS_DIRENT_POOL, TMPFS_NODE_POOL};

/// `TMPFS_NAME_QUANTUM`: quantum size to round-up the tmpfs names in order to reduce
/// re-allocations.
const TMPFS_NAME_QUANTUM: usize = 32;

/// `tmpfs_bytes_limit`: the memory all tmpfs mounts without a limit of their own may use
/// (`tmpfs_init`: half of physical memory).
pub static TMPFS_BYTES_LIMIT: AtomicU64 = AtomicU64::new(0);
/// `tmpfs_bytes_used`: the memory reserved by the mounts with a limit plus the memory used
/// by those without one.
pub static TMPFS_BYTES_USED: AtomicU64 = AtomicU64::new(0);

/// `tmpfs_mntmem_init`: start the accounting of a new mount with limit `memlimit` (0: the
/// global limit).
pub fn tmpfs_mntmem_init(mp: &TmpfsMount, memlimit: u64) {
    rw_init(&mp.tm_acc_lock, "tacclk");
    mp.tm_mem_limit.set(memlimit);
    mp.tm_bytes_used.set(0);
}

/// `tmpfs_mntmem_destroy`: end the accounting of a mount; everything was given back.
pub fn tmpfs_mntmem_destroy(mp: &TmpfsMount) {
    kassert!(mp.tm_bytes_used.get() == 0);
    // mutex_destroy(&mp->tm_acc_lock): nothing to do for an rwlock.
}

/// `tmpfs_pages_total`: the size of the file system in pages.
pub fn tmpfs_pages_total(mp: &TmpfsMount) -> u64 {
    let total = if mp.tm_mem_limit.get() != 0 {
        mp.tm_mem_limit.get()
    } else {
        TMPFS_BYTES_LIMIT
            .load(Ordering::Relaxed)
            .wrapping_sub(TMPFS_BYTES_USED.load(Ordering::Relaxed))
            .wrapping_add(mp.tm_bytes_used.get())
    };

    total >> PAGE_SHIFT
}

/// `tmpfs_pages_avail`: the pages the file system may still use. The caller holds
/// `tm_acc_lock`.
pub fn tmpfs_pages_avail(mp: &TmpfsMount) -> u64 {
    rw_assert_anylock(&mp.tm_acc_lock);

    let free = if mp.tm_mem_limit.get() != 0 {
        mp.tm_mem_limit.get() - mp.tm_bytes_used.get()
    } else {
        TMPFS_BYTES_LIMIT
            .load(Ordering::Relaxed)
            .wrapping_sub(TMPFS_BYTES_USED.load(Ordering::Relaxed))
    };

    free >> PAGE_SHIFT
}

/// `tmpfs_mem_incr`: account `sz` more bytes to the mount; `false` when that would pass
/// its limit.
pub fn tmpfs_mem_incr(mp: &TmpfsMount, sz: usize) -> bool {
    let sz = sz as u64;
    rw_enter_write(&mp.tm_acc_lock);

    let ok = 'out: {
        if mp.tm_mem_limit.get() != 0 {
            if mp.tm_mem_limit.get() - mp.tm_bytes_used.get() < sz {
                break 'out false;
            }
        } else {
            let limit = TMPFS_BYTES_LIMIT.load(Ordering::Relaxed);
            if limit.wrapping_sub(TMPFS_BYTES_USED.load(Ordering::Relaxed)) < sz {
                break 'out false;
            }
            TMPFS_BYTES_USED.fetch_add(sz, Ordering::Relaxed);
        }

        mp.tm_bytes_used.set(mp.tm_bytes_used.get() + sz);
        true
    };

    rw_exit_write(&mp.tm_acc_lock);

    ok
}

/// `tmpfs_mem_decr`: give `sz` bytes back.
pub fn tmpfs_mem_decr(mp: &TmpfsMount, sz: usize) {
    let sz = sz as u64;
    rw_enter_write(&mp.tm_acc_lock);
    kassert!(mp.tm_bytes_used.get() >= sz);
    if mp.tm_mem_limit.get() == 0 {
        TMPFS_BYTES_USED.fetch_sub(sz, Ordering::Relaxed);
    }
    mp.tm_bytes_used.set(mp.tm_bytes_used.get() - sz);
    rw_exit_write(&mp.tm_acc_lock);
}

/// `tmpfs_dirent_get`: a new directory entry, accounted to the mount; `None` when the mount
/// is full.
pub fn tmpfs_dirent_get(mp: &TmpfsMount) -> Option<&'static TmpfsDirent> {
    if !tmpfs_mem_incr(mp, size_of::<TmpfsDirent>()) {
        return None;
    }
    let Some(mem) = pool_get(&TMPFS_DIRENT_POOL, PR_ZERO | PR_WAITOK) else {
        panic(format_args!("tmpfs_dirent_get: pool_get failed"));
    };
    let de = mem.cast::<TmpfsDirent>();
    // SAFETY: a fresh pool item of `size_of::<TmpfsDirent>()` bytes, written once; it lives
    // until `tmpfs_dirent_put` returns it.
    Some(unsafe {
        de.as_ptr().write(TmpfsDirent::new());
        de.as_ref()
    })
}

/// `tmpfs_dirent_put`: give a directory entry back.
pub fn tmpfs_dirent_put(mp: &TmpfsMount, de: &'static TmpfsDirent) {
    tmpfs_mem_decr(mp, size_of::<TmpfsDirent>());
    pool_put(&TMPFS_DIRENT_POOL, NonNull::from(de).cast::<u8>());
}

/// `tmpfs_node_get`: a new node, counted and accounted to the mount; `None` when the mount
/// has its maximum number of nodes or is full.
pub fn tmpfs_node_get(mp: &TmpfsMount) -> Option<&'static TmpfsNode> {
    mp.tm_nodes_cnt.set(mp.tm_nodes_cnt.get() + 1);
    if mp.tm_nodes_cnt.get() > mp.tm_nodes_max.get() {
        mp.tm_nodes_cnt.set(mp.tm_nodes_cnt.get() - 1);
        return None;
    }
    if !tmpfs_mem_incr(mp, size_of::<TmpfsNode>()) {
        mp.tm_nodes_cnt.set(mp.tm_nodes_cnt.get() - 1);
        return None;
    }
    let Some(mem) = pool_get(&TMPFS_NODE_POOL, PR_WAITOK) else {
        panic(format_args!("tmpfs_node_get: pool_get failed"));
    };
    let tn = mem.cast::<TmpfsNode>();
    // SAFETY: a fresh pool item of `size_of::<TmpfsNode>()` bytes, written once; it lives
    // until `tmpfs_node_put` returns it.
    Some(unsafe {
        tn.as_ptr().write(TmpfsNode::new());
        tn.as_ref()
    })
}

/// `tmpfs_node_put`: give a node back.
pub fn tmpfs_node_put(mp: &TmpfsMount, tn: &'static TmpfsNode) {
    mp.tm_nodes_cnt.set(mp.tm_nodes_cnt.get() - 1);
    tmpfs_mem_decr(mp, size_of::<TmpfsNode>());
    pool_put(&TMPFS_NODE_POOL, NonNull::from(tn).cast::<u8>());
}

/// `roundup2(x, y)`: if y is powers of two.
const fn roundup2(x: usize, y: usize) -> usize {
    (x + (y - 1)) & !(y - 1)
}

/// `tmpfs_strname_alloc`: a buffer for a name of `len` bytes, accounted to the mount in
/// `TMPFS_NAME_QUANTUM` steps; `None` when the mount is full.
pub fn tmpfs_strname_alloc(mp: &TmpfsMount, len: usize) -> Option<NonNull<u8>> {
    let sz = roundup2(len, TMPFS_NAME_QUANTUM);

    kassert!(sz > 0 && sz <= 1024);
    if !tmpfs_mem_incr(mp, sz) {
        return None;
    }
    malloc(sz, M_TEMP, M_WAITOK) // XXX
}

/// `tmpfs_strname_free`: give back the buffer of a name of `len` bytes.
pub fn tmpfs_strname_free(mp: &TmpfsMount, str: NonNull<u8>, len: usize) {
    let sz = roundup2(len, TMPFS_NAME_QUANTUM);

    kassert!(sz > 0 && sz <= 1024);
    tmpfs_mem_decr(mp, sz);
    free(str, M_TEMP, sz);
}

/// `tmpfs_strname_neqlen`: whether the two names differ (in their rounded lengths or in
/// their bytes; see the module's deviations), that is, whether a rename needs a new name
/// buffer.
pub fn tmpfs_strname_neqlen(fcnp: &Componentname, tcnp: &Componentname) -> bool {
    let fln = roundup2(fcnp.name().len(), TMPFS_NAME_QUANTUM);
    let tln = roundup2(tcnp.name().len(), TMPFS_NAME_QUANTUM);

    fln != tln || fcnp.name() != tcnp.name()
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the tmpfs memory accounting: the per-mount limit, the global limit of the
    // mounts without one, the name buffers' quantum and the node/entry pools.

    use std::sync::MutexGuard;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::kern_rwlock::{rw_enter_read, rw_exit_read};
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::sys::param::PAGE_SIZE;
    use crate::tmpfs::tmpfs_vfsops::tests::tmpfs_conf;
    use crate::tmpfs::tmpfs_vfsops::tmpfs_init;

    /// Real memory, the tmpfs pools, and the global limit set to `limit` bytes with nothing used.
    /// No `vfsinit` runs here, so `tmpfs_init` is called directly, with tmpfs's
    /// `vfsconflist[]` entry.
    fn setup(limit: u64) -> MutexGuard<'static, ()> {
        let guard = setup_real_memory();
        let _ = tmpfs_init(tmpfs_conf());
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
}
/* </TESTS> */
