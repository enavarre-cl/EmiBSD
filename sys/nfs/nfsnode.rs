/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Rick Macklem at The University of Guelph.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)nfsnode.h	8.9 (Berkeley) 5/14/95
 */
/* </LICENSES> */

/* <CODE> */
//! `<nfs/nfsnode.h>`: the NFS client's node (`struct nfsnode`, the NFS equivalent of ufs's
//! inode), the silly rename record, the conversions between nodes and vnodes, and the
//! nfsiod buffer queue.
//!
//! Upstream: sys/nfs/nfsnode.h @ 3ce1f3f79392
//!
//! There is a unique nfsnode allocated for each active file, each current directory, each
//! mounted-on file, text file, and the root. An nfsnode is "named" by its file handle
//! (`nfs_nget`, `nfs_node.rs`). Nodes are `nfs_node_pool` items handed around as
//! `&'static NfsNode` with `Cell` members: the C changes them through shared pointers under
//! the vnode lock (`n_lock`); the mount's node tree links them through `n_entry`
//! ([`NfsNodetree`]).
//!
//! ## Deviations
//! - `n_fhp` is gone: the C always points it at the node's own `n_fh` (`nfs_node.c`,
//!   `nfs_vnops.c`), which is large enough for any handle. [`NfsNode::n_fhp`] returns the
//!   `n_fhsize` bytes of `n_fh`, [`NfsNode::set_fh`] stores a handle; `nfs_nget`'s
//!   stack-allocated `find` node copies the handle into its own `n_fh`.
//! - The unions `n_un1`/`n_un2` are their members side by side (`n_atim` and
//!   `n_cookieverf`, `n_mtim` and `n_direofoffset`); a node uses the pair of its type, as in
//!   C.
//! - `n_sillyrename` is a `NonNull` to the `malloc(9)`ed record `nfs_inactive` frees; the
//!   record holds the credential and directory as `&'static` (held with `crdup`/`vref`).
//! - `n_rcred`/`n_wcred` are `Cell<*const Ucred>`, the long-lived credential idiom.
//! - `nfsnode_cmp`, the `RBT_GENERATE` comparator of `nfs_node.c`, is here because the tree
//!   adapter that `struct nfsmount`'s `nm_ntree` names needs it.
//! - `VTONFS` checks the vnode's tag and panics on a vnode that is not NFS's (the C casts).
//! - `nfs_bufq`, `nfs_bufqlen` and `nfs_bufqmax` are declared here and defined in C in
//!   `nfs_bio.c`.

use core::cell::Cell;
use core::cmp::Ordering;
use core::ops::Deref;
use core::ptr::NonNull;
use core::sync::atomic::AtomicU32;

use crate::kern::subr_prf::panic;
use crate::kern::vfs_lockf::LockfStateSlot;
use crate::nfs::nfsproto::{NFS_MAXFHSIZE, Nfsfh, Nfsuint64};
use crate::sys::buf::BFreelist;
use crate::sys::queue::TailqHead;
use crate::sys::rwlock::{Rrwlock, Rwlock};
use crate::sys::time::Timespec;
use crate::sys::tree::RbtEntry;
use crate::sys::types::{Off, Time, Uid};
use crate::sys::ucred::Ucred;
use crate::sys::vnode::{VT_NFS, Vattr, Vnode};
use crate::tree_adapter;

/// `NFS_COMMIT_PUSH_VALID`: push range valid; values for `n_commitflags`.
pub const NFS_COMMIT_PUSH_VALID: i32 = 0x0001;
/// `NFS_COMMIT_PUSHED_VALID`: pushed range valid.
pub const NFS_COMMIT_PUSHED_VALID: i32 = 0x0002;

/// `NFLUSHWANT`: want wakeup from a flush in prog; flags for `n_flag`.
pub const NFLUSHWANT: i16 = 0x0001;
/// `NFLUSHINPROG`: avoid multiple calls to `vinvalbuf()`.
pub const NFLUSHINPROG: i16 = 0x0002;
/// `NMODIFIED`: might have a modified buffer in bio.
pub const NMODIFIED: i16 = 0x0004;
/// `NWRITEERR`: flag write errors so close will know.
pub const NWRITEERR: i16 = 0x0008;
/// `NACC`: special file accessed.
pub const NACC: i16 = 0x0100;
/// `NUPD`: special file updated.
pub const NUPD: i16 = 0x0200;
/// `NCHG`: special file times changed.
pub const NCHG: i16 = 0x0400;

/// `struct sillyrename`: silly rename structure that hangs off the nfsnode until the name
/// can be removed by `nfs_inactive()`.
pub struct Sillyrename {
    /// `s_cred`: the credential the rename was done with.
    pub s_cred: &'static Ucred,
    /// `s_dvp`: the directory.
    pub s_dvp: &'static Vnode,
    /// `s_namlen`: the length of `s_name`.
    pub s_namlen: i64,
    /// `s_name`: the silly name.
    pub s_name: [u8; 24],
}

/// The bytes of a node's file handle: `n_fhsize` bytes of `n_fh` (what the C reaches as
/// `n_fhp` with `n_fhsize`). A copy, so it outlives changes to the node.
#[derive(Clone, Copy)]
pub struct NfsFhBytes {
    fh: Nfsfh,
    len: usize,
}

impl NfsFhBytes {
    /// The whole 64-byte handle buffer (the C's `nfsfh_t` behind `n_fhp`).
    pub fn nfsfh(&self) -> &Nfsfh {
        &self.fh
    }
}

impl Deref for NfsFhBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.fh.fh_bytes[..self.len]
    }
}

/// `struct nfsnode`: the NFS equivalent of ufs's inode. Any similarity is purely
/// coincidental.
pub struct NfsNode {
    /// `n_entry`: filehandle/node tree.
    pub n_entry: RbtEntry,
    /// `n_size`: current size of file.
    pub n_size: Cell<u64>,
    /// `n_vattr`: vnode attribute cache.
    pub n_vattr: Cell<Vattr>,
    /// `n_attrstamp`: attr. cache timestamp.
    pub n_attrstamp: Cell<Time>,
    /// `n_mtime`: prev modify time.
    pub n_mtime: Cell<Timespec>,
    /// `n_ctime`: prev create time.
    pub n_ctime: Cell<Time>,
    /// `n_vnode`: associated vnode.
    pub n_vnode: Cell<Option<&'static Vnode>>,
    /// `n_lockf`: locking record of file.
    pub n_lockf: LockfStateSlot,
    /// `n_lock`: NFSnode lock.
    pub n_lock: Rrwlock,
    /// `n_error`: save write error value (an errno, 0 for none).
    pub n_error: Cell<i32>,
    /// `n_atim` (`n_un1.nf_atim`): special file times.
    pub n_atim: Cell<Timespec>,
    /// `n_cookieverf` (`n_un1.nd_cookieverf`): cookie verifier (dir only).
    pub n_cookieverf: Cell<Nfsuint64>,
    /// `n_mtim` (`n_un2.nf_mtim`).
    pub n_mtim: Cell<Timespec>,
    /// `n_direofoffset` (`n_un2.nd_direof`): dir. EOF offset cache.
    pub n_direofoffset: Cell<Off>,
    /// `n_sillyrename`: ptr to silly rename struct.
    pub n_sillyrename: Cell<Option<NonNull<Sillyrename>>>,
    /// `n_fhsize`: size in bytes, of fh.
    pub n_fhsize: Cell<i16>,
    /// `n_flag`: flag for locking.. (`NFLUSHWANT`, ...).
    pub n_flag: Cell<i16>,
    /// `n_fh`: small file handle (here: every file handle).
    pub n_fh: Cell<Nfsfh>,
    /// `n_accstamp`: access cache timestamp (`-1`: invalid).
    pub n_accstamp: Cell<Time>,
    /// `n_accuid`: last access requester.
    pub n_accuid: Cell<Uid>,
    /// `n_accmode`: last mode requested.
    pub n_accmode: Cell<i32>,
    /// `n_accerror`: last returned error.
    pub n_accerror: Cell<i32>,
    /// `n_rcred`: the credential of the last read (held with `crhold`).
    pub n_rcred: Cell<*const Ucred>,
    /// `n_wcred`: the credential of the last write (held with `crhold`).
    pub n_wcred: Cell<*const Ucred>,
    /// `n_pushedlo`: 1st blk in committed range.
    pub n_pushedlo: Cell<Off>,
    /// `n_pushedhi`: last block in range.
    pub n_pushedhi: Cell<Off>,
    /// `n_pushlo`: 1st block in commit range.
    pub n_pushlo: Cell<Off>,
    /// `n_pushhi`: last block in range.
    pub n_pushhi: Cell<Off>,
    /// `n_commitlock`: serialize commits.
    pub n_commitlock: Rwlock,
    /// `n_commitflags`: `NFS_COMMIT_*`.
    pub n_commitflags: Cell<i32>,
}

// SAFETY: the members are changed under the node's vnode lock (`n_lock`) or at `splbio`, the
// tree link under the kernel lock, as in C.
unsafe impl Sync for NfsNode {}

impl NfsNode {
    /// A node with every member cleared (`pool_get(&nfs_node_pool, PR_ZERO)`); `nfs_nget`
    /// then initialises its locks by name.
    pub const fn new() -> Self {
        Self {
            n_entry: RbtEntry::new(),
            n_size: Cell::new(0),
            n_vattr: Cell::new(Vattr::new()),
            n_attrstamp: Cell::new(0),
            n_mtime: Cell::new(Timespec::new(0, 0)),
            n_ctime: Cell::new(0),
            n_vnode: Cell::new(None),
            n_lockf: Cell::new(None),
            n_lock: Rrwlock::new("nfsnode"),
            n_error: Cell::new(0),
            n_atim: Cell::new(Timespec::new(0, 0)),
            n_cookieverf: Cell::new(Nfsuint64 { nfsuquad: [0; 2] }),
            n_mtim: Cell::new(Timespec::new(0, 0)),
            n_direofoffset: Cell::new(0),
            n_sillyrename: Cell::new(None),
            n_fhsize: Cell::new(0),
            n_flag: Cell::new(0),
            n_fh: Cell::new(Nfsfh::new()),
            n_accstamp: Cell::new(0),
            n_accuid: Cell::new(0),
            n_accmode: Cell::new(0),
            n_accerror: Cell::new(0),
            n_rcred: Cell::new(core::ptr::null()),
            n_wcred: Cell::new(core::ptr::null()),
            n_pushedlo: Cell::new(0),
            n_pushedhi: Cell::new(0),
            n_pushlo: Cell::new(0),
            n_pushhi: Cell::new(0),
            n_commitlock: Rwlock::new("nfs_commitlk"),
            n_commitflags: Cell::new(0),
        }
    }

    /// `n_fhp` with `n_fhsize`: the node's file handle bytes.
    pub fn n_fhp(&self) -> NfsFhBytes {
        NfsFhBytes {
            fh: self.n_fh.get(),
            len: (self.n_fhsize.get().max(0) as usize).min(NFS_MAXFHSIZE),
        }
    }

    /// `np->n_fhp = &np->n_fh; bcopy(fh, np->n_fhp, fhsize); np->n_fhsize = fhsize`: stores
    /// the file handle `fh` (at most `NFS_MAXFHSIZE` bytes, panics otherwise).
    pub fn set_fh(&self, fh: &[u8]) {
        if fh.len() > NFS_MAXFHSIZE {
            panic(format_args!("nfsnode: file handle of {} bytes", fh.len()));
        }
        let mut n = self.n_fh.get();
        n.fh_bytes[..fh.len()].copy_from_slice(fh);
        self.n_fh.set(n);
        self.n_fhsize.set(fh.len() as i16);
    }

    /// `n_flag & f`.
    pub fn isset(&self, f: i16) -> bool {
        self.n_flag.get() & f != 0
    }

    /// `n_flag |= f`.
    pub fn set(&self, f: i16) {
        self.n_flag.set(self.n_flag.get() | f);
    }

    /// `n_flag &= ~f`.
    pub fn clr(&self, f: i16) {
        self.n_flag.set(self.n_flag.get() & !f);
    }
}

impl Default for NfsNode {
    fn default() -> Self {
        Self::new()
    }
}

/// `nfsnode_cmp`: filehandle to node lookup order: the shorter handle first, then the bytes
/// (`memcmp`).
pub fn nfsnode_cmp(a: &NfsNode, b: &NfsNode) -> Ordering {
    a.n_fhsize
        .get()
        .cmp(&b.n_fhsize.get())
        .then_with(|| (*a.n_fhp()).cmp(&*b.n_fhp()))
}

tree_adapter!(
    /// `RBT_HEAD(nfs_nodetree, nfsnode)`: a mount's nodes by file handle, through `n_entry`.
    pub NfsNodetree: NfsNode, n_entry => RbtEntry, nfsnode_cmp
);

/// `struct nfs_bufqhead`: the queue head for nfsiod's.
pub struct NfsBufqhead(pub TailqHead<BFreelist>);

// SAFETY: the queue is changed at `splbio` under the kernel lock, as in C.
unsafe impl Sync for NfsBufqhead {}

/// `nfs_bufq`: buffers waiting for an nfsiod (defined in `nfs_bio.c`).
pub static NFS_BUFQ: NfsBufqhead = NfsBufqhead(TailqHead::new());
/// `nfs_bufqlen`: the length of `nfs_bufq` (defined in `nfs_bio.c`).
pub static NFS_BUFQLEN: AtomicU32 = AtomicU32::new(0);
/// `nfs_bufqmax`: the most buffers `nfs_bufq` may hold (defined in `nfs_bio.c`).
pub static NFS_BUFQMAX: AtomicU32 = AtomicU32::new(0);

/// `NFS_INVALIDATE_ATTRCACHE(np)`.
pub fn nfs_invalidate_attrcache(np: &NfsNode) {
    np.n_attrstamp.set(0);
}

/// `VTONFS(vp)`: the nfsnode of an NFS vnode.
#[allow(non_snake_case)] // the C name
pub fn VTONFS(vp: &Vnode) -> &'static NfsNode {
    let data = vp.v_data.get();
    if data.is_null() || vp.v_tag.get() != VT_NFS {
        panic(format_args!("VTONFS: vnode {:p} has no nfsnode", vp));
    }
    // SAFETY: an NFS vnode's `v_data` (checked above) is the node `nfs_nget` hung there,
    // which lives until `nfs_reclaim` returns it to `nfs_node_pool` and clears `v_data`; the
    // caller holds the vnode (a reference or its lock), so it is not reclaimed meanwhile.
    unsafe { &*data.cast::<NfsNode>() }
}

/// `NFSTOV(np)`: the vnode of an nfsnode.
#[allow(non_snake_case)] // the C name
pub fn NFSTOV(np: &NfsNode) -> &'static Vnode {
    match np.n_vnode.get() {
        Some(vp) => vp,
        None => panic(format_args!("NFSTOV: nfsnode {:p} has no vnode", np)),
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_order_by_size_then_bytes() {
        let (a, b) = (NfsNode::new(), NfsNode::new());
        a.set_fh(&[1, 2, 3, 4]);
        b.set_fh(&[0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(nfsnode_cmp(&a, &b), Ordering::Less);
        b.set_fh(&[1, 2, 3, 5]);
        assert_eq!(nfsnode_cmp(&a, &b), Ordering::Less);
        b.set_fh(&[1, 2, 3, 4]);
        assert_eq!(nfsnode_cmp(&a, &b), Ordering::Equal);
        assert_eq!(&*a.n_fhp(), &[1, 2, 3, 4]);
        assert!(crate::nfs::nfs::nfs_cmpfh(&a, &[1, 2, 3, 4]));
        assert!(!crate::nfs::nfs::nfs_cmpfh(&a, &[1, 2, 3]));
    }
}
/* </TESTS> */
