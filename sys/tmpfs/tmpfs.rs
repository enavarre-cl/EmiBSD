/* <LICENSES> */
/*
 * Copyright (c) 2005, 2006, 2007 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Julio M. Merino Vidal, developed as part of Google's Summer of Code
 * 2005 program.
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
//! `<tmpfs/tmpfs.h>`: the in-memory representation of tmpfs: the inode (`struct tmpfs_node`),
//! the directory entry (`struct tmpfs_dirent`), the mount (`struct tmpfs_mount`), the NFS file
//! handle (`struct tmpfs_fid`), their constants and the conversions from the VFS structures.
//!
//! Upstream: sys/tmpfs/tmpfs.h @ 3ce1f3f79392
//!
//! Nodes and directory entries are pool items (`tmpfs_node_pool`, `tmpfs_dirent_pool`,
//! `tmpfs_vfsops.rs`) that live until `tmpfs_node_put`/`tmpfs_dirent_put` return them. They
//! are handed around as `&'static TmpfsNode`/`&'static TmpfsDirent`, the `crget`/`crfree`
//! idiom of `docs/C_TO_RUST.md`: the C's reference rules (the link count, the vnode
//! association, the directory lists) say who may still use one. The members are `Cell`s: the
//! C mutates them through shared pointers under the vnode lock (`tn_vlock`), and the vnode
//! association under `tn_nlock`.
//!
//! The prototypes of `tmpfs_subr.c` and `tmpfs_mem.c` are their functions in
//! `tmpfs_subr.rs` and `tmpfs_mem.rs`.
//!
//! ## Deviations
//! - `union tn_spec` is the structure [`TnSpec`] holding the four alternatives side by side
//!   (`tn_spec.tn_dir.tn_parent` reads as in C); only the one of the node's type is used,
//!   as in C. The `tn_uobj`, `tn_pgptr`, `tn_pgnum` shorthands are methods.
//! - `tn_flags` is a `u32`, the type of the `UF_*`/`SF_*` flags it holds (an `int` in C).
//! - `td_name` and `tn_link` stay pointers to their `tmpfs_strname_alloc` buffers with the
//!   length beside them; [`TmpfsDirent::name`] and [`TmpfsNode::link`] are the slices.
//! - `vaddr_t tn_aobj_pgptr` is a `usize` (0 for NULL), as `uvm_map` returns addresses.
//! - `TMPFS_DIRSEQ_FULL`, `TMPFS_NODE_RECLAIMING`, `TMPFS_NODE_GEN` and
//!   `TMPFS_VALIDATE_DIR` are `const fn`s/functions with lowercase names; `VFS_TO_TMPFS`,
//!   `VP_TO_TMPFS_DIR` and `VP_TO_TMPFS_NODE` keep their names.
//! - `struct tmpfs_fid` is read from and written into a `struct fid` member by member
//!   ([`TmpfsFid::from_fid`], [`TmpfsFid::to_fid`]), where the C `memcpy`s it over the
//!   `struct fid`.

use core::cell::Cell;
use core::ptr;

use crate::kassert;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_lockf::LockfStateSlot;
use crate::kern::vfs_vops::VOP_ISLOCKED;
use crate::queue_adapter;
use crate::sys::mount::{Fid, Mount};
use crate::sys::queue::{ListEntry, ListHead, TailqEntry, TailqHead};
use crate::sys::rwlock::{Rrwlock, Rwlock};
use crate::sys::time::Timespec;
use crate::sys::types::{Dev, Gid, Ino, Mode, Nlink, Off, Uid};
use crate::sys::vnode::{VDIR, VNON, VT_TMPFS, Vnode, Vtype};
use crate::uvm::uvm_extern::Voff;
use crate::uvm::uvm_object::UvmObject;

/// `struct tmpfs_dirent` (`tmpfs_dirent_t`): internal representation of a tmpfs directory
/// entry.
///
/// All fields are protected by vnode lock.
pub struct TmpfsDirent {
    /// `td_entries`: the link in the directory's `tn_dir` list.
    pub td_entries: TailqEntry<TmpfsDirent>,
    /// `td_node`: pointer to the inode this entry refers to.
    pub td_node: Cell<Option<&'static TmpfsNode>>,
    /// `td_seq`: sequence number, see `tmpfs_dir_getseq()`.
    pub td_seq: Cell<u64>,
    /// `td_name`: the name, a `tmpfs_strname_alloc` buffer of `td_namelen` bytes (no NUL).
    pub td_name: Cell<*mut u8>,
    /// `td_namelen`: the name's length.
    pub td_namelen: Cell<u16>,
}

// SAFETY: the members are changed under the directory's vnode lock, as in C.
unsafe impl Sync for TmpfsDirent {}

impl TmpfsDirent {
    /// An entry with no node, no name and no sequence number (what `pool_get` with
    /// `PR_ZERO` hands out).
    pub const fn new() -> Self {
        Self {
            td_entries: TailqEntry::new(),
            td_node: Cell::new(None),
            td_seq: Cell::new(0),
            td_name: Cell::new(ptr::null_mut()),
            td_namelen: Cell::new(0),
        }
    }

    /// The name: the `td_namelen` bytes at `td_name`.
    pub fn name(&self) -> &[u8] {
        let p = self.td_name.get();
        if p.is_null() {
            return &[];
        }
        // SAFETY: `tmpfs_alloc_dirent` (or a rename) points `td_name` at a
        // `tmpfs_strname_alloc` buffer of at least `td_namelen` bytes, freed only together
        // with the entry or when the name is replaced, under the vnode lock the caller holds.
        unsafe { core::slice::from_raw_parts(p, usize::from(self.td_namelen.get())) }
    }
}

impl Default for TmpfsDirent {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(tmpfs_dir, tmpfs_dirent)`: the entries of a directory (`td_entries`).
    pub TmpfsDir: TmpfsDirent, td_entries => TailqEntry<TmpfsDirent>
);

/// `tn_spec.tn_dev`: type case VBLK or VCHR.
pub struct TnDev {
    /// `tn_rdev`.
    pub tn_rdev: Cell<Dev>,
}

/// `tn_spec.tn_dir`: type case VDIR.
pub struct TnDir {
    /// `tn_parent`: parent directory (root inode points to itself).
    pub tn_parent: Cell<Option<&'static TmpfsNode>>,
    /// `tn_dir`: list of directory entries.
    pub tn_dir: TailqHead<TmpfsDir>,
    /// `tn_next_seq`: last given sequence number.
    pub tn_next_seq: Cell<u64>,
    /// `tn_readdir_lastp`: pointer of the last directory entry returned by the readdir(3)
    /// operation.
    pub tn_readdir_lastp: Cell<Option<&'static TmpfsDirent>>,
}

/// `struct tn_lnk`: type case VLNK.
pub struct TnLnk {
    /// `tn_link`: the link's target, a `tmpfs_strname_alloc` buffer of `tn_size` bytes, or
    /// null when the target is empty.
    pub tn_link: Cell<*mut u8>,
}

/// `struct tn_reg`: type case VREG.
pub struct TnReg {
    /// `tn_aobj`: underlying UVM object to store contents.
    pub tn_aobj: Cell<Option<&'static UvmObject>>,
    /// `tn_aobj_pages`: the pages accounted to the object.
    pub tn_aobj_pages: Cell<usize>,
    /// `tn_aobj_pgptr`: the kernel address of the cached mapping of one page, or 0.
    pub tn_aobj_pgptr: Cell<usize>,
    /// `tn_aobj_pgnum`: the object offset of that page, or -1.
    pub tn_aobj_pgnum: Cell<Voff>,
}

/// `union tn_spec`: the data that is only applicable to a particular type (see the module's
/// deviations).
pub struct TnSpec {
    /// `tn_dev`.
    pub tn_dev: TnDev,
    /// `tn_dir`.
    pub tn_dir: TnDir,
    /// `tn_lnk`.
    pub tn_lnk: TnLnk,
    /// `tn_reg`.
    pub tn_reg: TnReg,
}

/// `struct tmpfs_node` (`tmpfs_node_t`): internal representation of a tmpfs file system
/// node -- inode.
///
/// This structure is split in two parts: one holds attributes common to all file types and
/// the other holds data that is only applicable to a particular type.
///
/// All fields are protected by vnode lock. The vnode association itself is protected by
/// `tn_nlock`.
pub struct TmpfsNode {
    /// `tn_entries`: the link in the mount's `tm_nodes`.
    pub tn_entries: ListEntry<TmpfsNode>,
    /// `tn_nlock`: node lock.
    pub tn_nlock: Rwlock,
    /// `tn_vlock`: vnode lock.
    pub tn_vlock: Rrwlock,
    /// `tn_vnode`: each inode has a corresponding vnode. It is a bi-directional association.
    /// Whenever vnode is allocated, its `v_data` field is set to the inode it reference, and
    /// `tn_vnode` is set to point to the said vnode.
    ///
    /// Further attempts to allocate a vnode for this same node will result in returning a
    /// new reference to the value stored in `tn_vnode`. It may be `None` when the node is
    /// unused (that is, no vnode has been allocated or it has been reclaimed).
    pub tn_vnode: Cell<Option<&'static Vnode>>,
    /// `tn_dirent_hint`: directory entry. Only a hint, since hard link can have multiple.
    pub tn_dirent_hint: Cell<Option<&'static TmpfsDirent>>,
    /// `tn_type`: the inode type: VBLK, VCHR, VDIR, VFIFO, VLNK, VREG or VSOCK.
    pub tn_type: Cell<Vtype>,
    /// `tn_id`: inode identifier.
    pub tn_id: Cell<Ino>,
    /// `tn_gen`: generation number (and [`TMPFS_RECLAIMING_BIT`]).
    pub tn_gen: Cell<u64>,
    /// `tn_size`: the inode size.
    pub tn_size: Cell<Off>,
    /// `tn_uid`.
    pub tn_uid: Cell<Uid>,
    /// `tn_gid`.
    pub tn_gid: Cell<Gid>,
    /// `tn_mode`.
    pub tn_mode: Cell<Mode>,
    /// `tn_flags`.
    pub tn_flags: Cell<u32>,
    /// `tn_links`.
    pub tn_links: Cell<Nlink>,
    /// `tn_atime`.
    pub tn_atime: Cell<Timespec>,
    /// `tn_mtime`.
    pub tn_mtime: Cell<Timespec>,
    /// `tn_ctime`.
    pub tn_ctime: Cell<Timespec>,
    /// `tn_birthtime`.
    pub tn_birthtime: Cell<Timespec>,
    /// `tn_lockf`: head of byte-level lock list (used by `tmpfs_advlock`).
    pub tn_lockf: LockfStateSlot,
    /// `tn_spec`.
    pub tn_spec: TnSpec,
}

// SAFETY: the members are changed under the node's vnode lock or `tn_nlock`, and the list
// link under the mount's `tm_lock`, as in C.
unsafe impl Sync for TmpfsNode {}

impl TmpfsNode {
    /// A node with every member cleared, before `tmpfs_alloc_node` fills it in.
    pub const fn new() -> Self {
        Self {
            tn_entries: ListEntry::new(),
            tn_nlock: Rwlock::new("tvlk"),
            tn_vlock: Rrwlock::new("tnode"),
            tn_vnode: Cell::new(None),
            tn_dirent_hint: Cell::new(None),
            tn_type: Cell::new(VNON),
            tn_id: Cell::new(0),
            tn_gen: Cell::new(0),
            tn_size: Cell::new(0),
            tn_uid: Cell::new(0),
            tn_gid: Cell::new(0),
            tn_mode: Cell::new(0),
            tn_flags: Cell::new(0),
            tn_links: Cell::new(0),
            tn_atime: Cell::new(Timespec::new(0, 0)),
            tn_mtime: Cell::new(Timespec::new(0, 0)),
            tn_ctime: Cell::new(Timespec::new(0, 0)),
            tn_birthtime: Cell::new(Timespec::new(0, 0)),
            tn_lockf: Cell::new(None),
            tn_spec: TnSpec {
                tn_dev: TnDev {
                    tn_rdev: Cell::new(0),
                },
                tn_dir: TnDir {
                    tn_parent: Cell::new(None),
                    tn_dir: TailqHead::new(),
                    tn_next_seq: Cell::new(0),
                    tn_readdir_lastp: Cell::new(None),
                },
                tn_lnk: TnLnk {
                    tn_link: Cell::new(ptr::null_mut()),
                },
                tn_reg: TnReg {
                    tn_aobj: Cell::new(None),
                    tn_aobj_pages: Cell::new(0),
                    tn_aobj_pgptr: Cell::new(0),
                    tn_aobj_pgnum: Cell::new(-1),
                },
            },
        }
    }

    /// `tn_uobj` (`tn_spec.tn_reg.tn_aobj`): the regular file's object.
    pub fn tn_uobj(&self) -> &'static UvmObject {
        match self.tn_spec.tn_reg.tn_aobj.get() {
            Some(uobj) => uobj,
            None => panic(format_args!("tmpfs node {:p}: no aobj", self)),
        }
    }

    /// `tn_pgptr` (`tn_spec.tn_reg.tn_aobj_pgptr`).
    pub fn tn_pgptr(&self) -> &Cell<usize> {
        &self.tn_spec.tn_reg.tn_aobj_pgptr
    }

    /// `tn_pgnum` (`tn_spec.tn_reg.tn_aobj_pgnum`).
    pub fn tn_pgnum(&self) -> &Cell<Voff> {
        &self.tn_spec.tn_reg.tn_aobj_pgnum
    }

    /// The symbolic link's target: the `tn_size` bytes at `tn_spec.tn_lnk.tn_link`.
    pub fn link(&self) -> &[u8] {
        let p = self.tn_spec.tn_lnk.tn_link.get();
        if p.is_null() || self.tn_size.get() <= 0 {
            return &[];
        }
        // SAFETY: `tmpfs_alloc_node` points `tn_link` at a `tmpfs_strname_alloc` buffer of
        // at least `tn_size` bytes, which lives until `tmpfs_free_node`; a symbolic link's
        // size never changes.
        unsafe { core::slice::from_raw_parts(p, self.tn_size.get() as usize) }
    }
}

impl Default for TmpfsNode {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `LIST_HEAD(tmpfs_node_list, tmpfs_node)`: the inodes of a mount (`tn_entries`).
    pub TmpfsNodeList: TmpfsNode, tn_entries => ListEntry<TmpfsNode>
);

/// `TMPFS_MAXNAMLEN`. Validate maximum `td_namelen` length: `TMPFS_MAXNAMLEN < UINT16_MAX`
/// (checked at the end of the file).
pub const TMPFS_MAXNAMLEN: usize = 255;

// Reserved values for the virtual entries (the first must be 0) and EOF. The start/end of
// the incremental range, see tmpfs_dir_getseq().

/// `TMPFS_DIRSEQ_DOT`.
pub const TMPFS_DIRSEQ_DOT: u64 = 0;
/// `TMPFS_DIRSEQ_DOTDOT`.
pub const TMPFS_DIRSEQ_DOTDOT: u64 = 1;
/// `TMPFS_DIRSEQ_EOF`.
pub const TMPFS_DIRSEQ_EOF: u64 = 2;

/// `TMPFS_DIRSEQ_START`: inclusive.
pub const TMPFS_DIRSEQ_START: u64 = 3;
/// `TMPFS_DIRSEQ_END`: exclusive.
pub const TMPFS_DIRSEQ_END: u64 = u64::MAX;

/// `TMPFS_DIRSEQ_NONE`: mark to indicate that the number is not set.
pub const TMPFS_DIRSEQ_NONE: u64 = u64::MAX;

/// `TMPFS_DIRSEQ_FULL(dnode)`: can we still append entries to a directory?
pub fn tmpfs_dirseq_full(dnode: &TmpfsNode) -> bool {
    dnode.tn_spec.tn_dir.tn_next_seq.get() == TMPFS_DIRSEQ_END
}

// Status flags.

/// `TMPFS_NODE_ACCESSED`.
pub const TMPFS_NODE_ACCESSED: i32 = 0x01;
/// `TMPFS_NODE_MODIFIED`.
pub const TMPFS_NODE_MODIFIED: i32 = 0x02;
/// `TMPFS_NODE_CHANGED`.
pub const TMPFS_NODE_CHANGED: i32 = 0x04;

/// `TMPFS_NODE_STATUSALL`.
pub const TMPFS_NODE_STATUSALL: i32 =
    TMPFS_NODE_ACCESSED | TMPFS_NODE_MODIFIED | TMPFS_NODE_CHANGED;

/// `TMPFS_NODE_GEN_MASK`: bit indicating vnode reclamation. We abuse `tn_gen` for that.
pub const TMPFS_NODE_GEN_MASK: u64 = !0u64 >> 1;
/// `TMPFS_RECLAIMING_BIT`.
pub const TMPFS_RECLAIMING_BIT: u64 = !TMPFS_NODE_GEN_MASK;

/// `TMPFS_NODE_RECLAIMING(node)`.
pub fn tmpfs_node_reclaiming(node: &TmpfsNode) -> bool {
    node.tn_gen.get() & TMPFS_RECLAIMING_BIT != 0
}

/// `TMPFS_NODE_GEN(node)`.
pub fn tmpfs_node_gen(node: &TmpfsNode) -> u64 {
    node.tn_gen.get() & TMPFS_NODE_GEN_MASK
}

/// `struct tmpfs_mount` (`tmpfs_mount_t`): internal representation of a tmpfs mount point.
pub struct TmpfsMount {
    /// `tm_mem_limit`: limit of bytes in use by the file system (0: the global limit).
    pub tm_mem_limit: Cell<u64>,
    /// `tm_bytes_used`: number of bytes in use by the file system.
    pub tm_bytes_used: Cell<u64>,
    /// `tm_highest_inode`: highest allocated inode number.
    pub tm_highest_inode: Cell<u64>,
    /// `tm_acc_lock`: protects the accounting and `tm_highest_inode`.
    pub tm_acc_lock: Rwlock,
    /// `tm_root`: pointer to the root inode.
    pub tm_root: Cell<Option<&'static TmpfsNode>>,
    /// `tm_nodes_max`: maximum number of possible nodes for this file system.
    pub tm_nodes_max: Cell<u32>,
    /// `tm_nodes_cnt`: number of nodes currently allocated.
    pub tm_nodes_cnt: Cell<u32>,
    /// `tm_lock`: the lock protecting the list of inodes.
    pub tm_lock: Rwlock,
    /// `tm_nodes`: list of inodes.
    pub tm_nodes: ListHead<TmpfsNodeList>,
}

// SAFETY: the counters are changed under `tm_acc_lock`, the node list under `tm_lock`, the
// rest under the vnode locks, as in C.
unsafe impl Sync for TmpfsMount {}

impl TmpfsMount {
    /// An empty mount, before `tmpfs_mount` fills it in.
    pub const fn new() -> Self {
        Self {
            tm_mem_limit: Cell::new(0),
            tm_bytes_used: Cell::new(0),
            tm_highest_inode: Cell::new(0),
            tm_acc_lock: Rwlock::new("tacclk"),
            tm_root: Cell::new(None),
            tm_nodes_max: Cell::new(0),
            tm_nodes_cnt: Cell::new(0),
            tm_lock: Rwlock::new("tmplk"),
            tm_nodes: ListHead::new(),
        }
    }

    /// `tm_root`, which `tmpfs_mount` sets before the mount is used.
    pub fn root(&self) -> &'static TmpfsNode {
        match self.tm_root.get() {
            Some(root) => root,
            None => panic(format_args!("tmpfs mount {:p}: no root", self)),
        }
    }
}

impl Default for TmpfsMount {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct tmpfs_fid` (`tmpfs_fid_t`): maps a file identifier to a tmpfs node. Used by the
/// NFS code.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TmpfsFid {
    /// `tf_len`: `sizeof(tmpfs_fid_t)`, over `fid_len`.
    pub tf_len: u16,
    /// `tf_pad`.
    pub tf_pad: u16,
    /// `tf_gen`: the node's `TMPFS_NODE_GEN`.
    pub tf_gen: u32,
    /// `tf_id`: the node's `tn_id`.
    pub tf_id: Ino,
}

impl TmpfsFid {
    /// `sizeof(tmpfs_fid_t)`.
    pub const SIZE: usize = size_of::<TmpfsFid>();

    /// `memcpy(&tfh, fhp, sizeof(tmpfs_fid_t))`: the handle stored in a `struct fid`.
    pub fn from_fid(fid: &Fid) -> Self {
        let d = &fid.fid_data;
        Self {
            tf_len: fid.fid_len,
            tf_pad: fid.fid_reserved,
            tf_gen: u32::from_ne_bytes([d[0], d[1], d[2], d[3]]),
            tf_id: u64::from_ne_bytes([d[4], d[5], d[6], d[7], d[8], d[9], d[10], d[11]]),
        }
    }

    /// `memcpy(fhp, &tfh, sizeof(tfh))`: stores the handle over a `struct fid` (the bytes
    /// past the handle are left as they are, as in C).
    pub fn to_fid(&self, fid: &mut Fid) {
        fid.fid_len = self.tf_len;
        fid.fid_reserved = self.tf_pad;
        fid.fid_data[0..4].copy_from_slice(&self.tf_gen.to_ne_bytes());
        fid.fid_data[4..12].copy_from_slice(&self.tf_id.to_ne_bytes());
    }
}

/// `TMPFS_VALIDATE_DIR(node)`: ensures that the node pointed by `node` is a directory and
/// that its contents are consistent with respect to directories.
pub fn tmpfs_validate_dir(node: &TmpfsNode) {
    kassert!(node.tn_vnode.get().is_none_or(|vp| VOP_ISLOCKED(vp) != 0));
    kassert!(node.tn_type.get() == VDIR);
    kassert!(node.tn_size.get() % size_of::<TmpfsDirent>() as Off == 0);
}

/// `VFS_TO_TMPFS(mp)`: the tmpfs mount of a mount.
#[allow(non_snake_case)] // the C name
pub fn VFS_TO_TMPFS(mp: &Mount) -> &'static TmpfsMount {
    let tmp = mp.mnt_data.get();
    if tmp.is_null() {
        panic(format_args!(
            "VFS_TO_TMPFS: mount {:p} has no tmpfs_mount",
            mp
        ));
    }
    // SAFETY: a tmpfs mount's `mnt_data` is the `TmpfsMount` `tmpfs_mount` allocated, which
    // lives until `tmpfs_unmount` frees it and clears `mnt_data`; the caller holds the mount
    // busy or a vnode of it.
    unsafe { &*tmp.cast::<TmpfsMount>() }
}

/// `VP_TO_TMPFS_DIR(vp)`: the directory node of a tmpfs vnode.
#[allow(non_snake_case)] // the C name
pub fn VP_TO_TMPFS_DIR(vp: &Vnode) -> &'static TmpfsNode {
    let node = VP_TO_TMPFS_NODE(vp);
    tmpfs_validate_dir(node);
    node
}

/// `VP_TO_TMPFS_NODE(vp)`: the node of a tmpfs vnode.
#[allow(non_snake_case)] // the C name
pub fn VP_TO_TMPFS_NODE(vp: &Vnode) -> &'static TmpfsNode {
    let node = vp.v_data.get();
    if node.is_null() || vp.v_tag.get() != VT_TMPFS {
        panic(format_args!(
            "VP_TO_TMPFS_NODE: vnode {:p} has no tmpfs node",
            vp
        ));
    }
    // SAFETY: a tmpfs vnode's `v_data` (checked above) is the node `tmpfs_vnode_get` hung
    // there, which lives at least until `tmpfs_reclaim` clears `v_data`; the caller holds
    // the vnode (a reference or its lock), so it is not reclaimed meanwhile.
    unsafe { &*node.cast::<TmpfsNode>() }
}

const _: () = {
    assert!(TMPFS_MAXNAMLEN < u16::MAX as usize);
    assert!(TmpfsFid::SIZE == 16);
    assert!(TmpfsFid::SIZE <= size_of::<Fid>());
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reclaiming_bit_is_outside_the_generation() {
        let node = TmpfsNode::new();
        node.tn_gen.set(TMPFS_NODE_GEN_MASK & 0x1234_5678);
        assert!(!tmpfs_node_reclaiming(&node));
        node.tn_gen.set(node.tn_gen.get() | TMPFS_RECLAIMING_BIT);
        assert!(tmpfs_node_reclaiming(&node));
        assert_eq!(tmpfs_node_gen(&node), 0x1234_5678);
    }

    #[test]
    fn fid_round_trips_through_a_struct_fid() {
        let tfh = TmpfsFid {
            tf_len: TmpfsFid::SIZE as u16,
            tf_pad: 0,
            tf_gen: 0xdead_beef,
            tf_id: 42,
        };
        let mut fid = Fid::default();
        tfh.to_fid(&mut fid);
        assert_eq!(fid.fid_len, 16);
        assert_eq!(TmpfsFid::from_fid(&fid), tfh);
    }
}
/* </TESTS> */
