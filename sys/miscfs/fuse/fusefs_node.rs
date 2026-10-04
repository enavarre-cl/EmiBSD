/* $OpenBSD: fusefs_node.h,v 1.9 2026/07/10 14:43:48 helg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2012-2013 Sylvestre Gallon <ccna.syl@gmail.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

//! `fusefs_node.h`: the in-core node of a FUSE file (`struct fusefs_node`), its file handles
//! and the `ITOV`/`VTOI` conversions.
//!
//! Upstream: sys/miscfs/fuse/fusefs_node.h @ 3ce1f3f79392
//!
//! The prototypes of the header are functions of the files that define them:
//! `fuse_ihashinit`, `fuse_ihashget`, `fuse_ihashins`, `fuse_ihashrem` (`fuse_ihash.rs`) and
//! `fusefs_fd_get` (`fuse_file.rs`).
//!
//! ## Deviations
//! - The node is `malloc(M_FUSEFS)`ed by `fusefs_vget` and freed by `fusefs_reclaim`; the
//!   members the C changes through shared pointers are `Cell`s.
//! - `i_hashed` is new: it says whether the node is on its hash chain, which the C reads
//!   from `i_hash.le_prev != NULL` (the list links are private to `sys/queue.rs`); it is set
//!   and cleared exactly where the node is linked and unlinked.
//! - `ITOV(ip)` is the member `i_vnode` (a node always has its vnode: `fusefs_vget` sets
//!   it at creation); `VTOI(vp)` checks the vnode's tag and panics on a vnode that is not a
//!   FUSE one or has no node, where the C would follow the pointer.

use core::cell::Cell;

use crate::kern::subr_prf::panic;
use crate::kern::vfs_lockf::LockfStateSlot;
use crate::miscfs::fuse::fusefs::FusefsMnt;
use crate::queue_adapter;
use crate::sys::queue::ListEntry;
use crate::sys::rwlock::Rrwlock;
use crate::sys::types::{Ino, Off};
use crate::sys::vnode::{VT_FUSEFS, Vnode};

/// `enum fufh_type`: the kind of a FUSE file handle, which is also its index in
/// `fusefs_node.fufh[]`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types, clippy::upper_case_acronyms)] // OpenBSD names, verbatim
pub enum FufhType {
    /// No handle.
    FUFH_INVALID = -1,
    /// Opened for reading.
    FUFH_RDONLY = 0,
    /// Opened for writing.
    FUFH_WRONLY = 1,
    /// Opened for reading and writing.
    FUFH_RDWR = 2,
    /// The number of handle kinds.
    FUFH_MAXTYPE = 3,
}

pub use FufhType::{FUFH_INVALID, FUFH_MAXTYPE, FUFH_RDONLY, FUFH_RDWR, FUFH_WRONLY};

impl FufhType {
    /// The kind's index in `fufh[]` (the C uses the enum as the index).
    pub const fn idx(self) -> usize {
        self as i32 as usize
    }

    /// The kind of index `i` (`0..FUFH_MAXTYPE`), as the C's `for (type = 0; ...)` loops
    /// read it.
    pub const fn from_idx(i: usize) -> FufhType {
        match i {
            0 => FUFH_RDONLY,
            1 => FUFH_WRONLY,
            2 => FUFH_RDWR,
            _ => FUFH_INVALID,
        }
    }
}

/// `struct fusefs_filehandle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FusefsFilehandle {
    /// `fh_id`: the daemon's handle.
    pub fh_id: u64,
    /// `fh_type`: the kind, `FUFH_INVALID` when the slot is unused.
    pub fh_type: FufhType,
}

impl FusefsFilehandle {
    /// A zeroed handle, as `malloc(M_ZERO)` leaves it (`fusefs_vget` then marks it invalid).
    pub const fn new() -> Self {
        Self {
            fh_id: 0,
            fh_type: FUFH_RDONLY,
        }
    }
}

impl Default for FusefsFilehandle {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct fusefs_node`.
pub struct FusefsNode {
    /// `i_hash`: hash chain.
    pub i_hash: ListEntry<FusefsNode>,
    /// `i_vnode`: vnode associated with this inode.
    pub i_vnode: &'static Vnode,
    /// `i_fmp`: the mount.
    pub i_fmp: &'static FusefsMnt,
    /// `i_number`: the identity of the inode.
    pub i_number: Ino,
    /// `i_parent_cache`: parent inode (only dirs).
    pub i_parent_cache: Cell<Ino>,
    /// `i_lockf`: byte-level lock state.
    pub i_lockf: LockfStateSlot,
    /// `i_lock`: inode lock.
    pub i_lock: Rrwlock,
    /// `fufh`: I/O: one handle per kind.
    pub fufh: [Cell<FusefsFilehandle>; FUFH_MAXTYPE.idx()],
    /// `filesize`: meta.
    pub filesize: Cell<Off>,
    /// `nlookup`: lookup count needed by `FUSE_FORGET`.
    pub nlookup: Cell<u64>,
    /// Whether the node is on its hash chain (see the module's deviations).
    pub i_hashed: Cell<bool>,
}

// SAFETY: changed under the vnode lock or the kernel lock, as in C; the hash link
// under the kernel lock (the C's "XXXLOCKING").
unsafe impl Sync for FusefsNode {}

queue_adapter!(
    /// `i_hash`: the link of a node on its hash chain (`struct fuse_ihashhead`).
    pub FusefsIHash: FusefsNode, i_hash => ListEntry<FusefsNode>
);

impl FusefsNode {
    /// A node of `vp` on `fmp` for inode `ino`, every other member as `malloc(M_ZERO)`
    /// leaves it.
    pub const fn new(vp: &'static Vnode, fmp: &'static FusefsMnt, ino: Ino) -> Self {
        Self {
            i_hash: ListEntry::new(),
            i_vnode: vp,
            i_fmp: fmp,
            i_number: ino,
            i_parent_cache: Cell::new(0),
            i_lockf: Cell::new(None),
            i_lock: Rrwlock::new("fuseinode"),
            fufh: [
                Cell::new(FusefsFilehandle::new()),
                Cell::new(FusefsFilehandle::new()),
                Cell::new(FusefsFilehandle::new()),
            ],
            filesize: Cell::new(0),
            nlookup: Cell::new(0),
            i_hashed: Cell::new(false),
        }
    }

    /// `ip->fufh[type]`.
    pub fn fufh(&self, t: FufhType) -> FusefsFilehandle {
        self.fufh[t.idx()].get()
    }

    /// `ip->fufh[type] = fh`.
    pub fn set_fufh(&self, t: FufhType, fh: FusefsFilehandle) {
        self.fufh[t.idx()].set(fh);
    }
}

/// `ITOV(ip)`: the vnode of a node.
#[allow(non_snake_case)] // the C name
pub fn ITOV(ip: &FusefsNode) -> &'static Vnode {
    ip.i_vnode
}

/// `VTOI(vp)`: the node of a FUSE vnode.
#[allow(non_snake_case)] // the C name
pub fn VTOI(vp: &Vnode) -> &'static FusefsNode {
    let ip = vp.v_data.get();
    if ip.is_null() || vp.v_tag.get() != VT_FUSEFS {
        panic(format_args!("VTOI: vnode {:p} has no fusefs node", vp));
    }
    // SAFETY: a FUSE vnode's `v_data` (checked above) is the node `fusefs_vget` hung there,
    // which lives until `fusefs_reclaim` frees it and clears `v_data`; the caller holds the
    // vnode (a reference or its lock), so it is not reclaimed meanwhile.
    unsafe { &*ip.cast::<FusefsNode>() }
}
