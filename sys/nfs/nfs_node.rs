/*	$OpenBSD: nfs_node.c,v 1.77 2026/06/30 14:04:04 kirill Exp $	*/
/*	$NetBSD: nfs_node.c,v 1.16 1996/02/18 11:53:42 fvdl Exp $	*/
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
 *	@(#)nfs_node.c	8.6 (Berkeley) 5/22/95
 */
/* </LICENSES> */

//! The NFS client's nodes: the per-mount tree of nfsnodes by file handle (`nfs_ninit`),
//! finding or making the node and vnode of a file handle (`nfs_nget`), and the vnode
//! operations `nfs_inactive` (remove a silly-renamed file on last close) and `nfs_reclaim`
//! (give the node back to `nfs_node_pool`).
//!
//! Upstream: sys/nfs/nfs_node.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `nfsnode_cmp` and the `RBT_GENERATE(nfs_nodetree, ...)` adapter (`NfsNodetree`) live in
//!   `nfsnode.rs`, beside `struct nfsnode`, because `struct nfsmount`'s tree head names the
//!   adapter; `nfs_node_pool` (`NFS_NODE_POOL`) is declared in `nfs.rs` and initialised by
//!   `nfs_vfs_init` (`nfs_subs.rs`), as in C.
//! - `nfs_nget(mnt, fh, fhsize, &np)` is `nfs_nget(mnt, fh) -> Result<&NfsNode, Errno>`:
//!   the handle is the slice of its `fhsize` bytes, and the node is the return value where
//!   the C fills `*npp` (NULL on error). The lookup key is a node on the stack holding a copy
//!   of the handle (the C points `find.n_fhp` at the caller's handle).
//! - `VFSLCKDEBUG` (`VLOCKSWORK`) is not configured.
//! - The `DIAGNOSTIC` checks (`prtactive` reports, the panic on a vnode without `v_data`)
//!   are behind feature `diagnostic`.
//! - `nfs_removeit` and the vops tables come from `nfs_vnops.rs`.

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::kassert;
use crate::kern::kern_malloc::free;
use crate::kern::kern_prot::{crfree, crhold};
use crate::kern::kern_rwlock::{rrw_init_flags, rw_init};
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::vfs_cache::cache_purge;
#[cfg(feature = "diagnostic")]
use crate::kern::vfs_subr::PRTACTIVE;
use crate::kern::vfs_subr::{getnewvnode, vget, vgone, vput};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_LOCK, VOP_UNLOCK};
use crate::machine::cpu::curproc;
use crate::nfs::nfs::NFS_NODE_POOL;
use crate::nfs::nfs_bio::nfs_vinvalbuf;
use crate::nfs::nfs_vnops::{NFS_VOPS, nfs_removeit};
use crate::nfs::nfsmount::{NfsMount, VFSTONFS};
use crate::nfs::nfsnode::{NFLUSHINPROG, NFLUSHWANT, NFSTOV, NMODIFIED, NfsNode, VTONFS};
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::M_NFSREQ;
use crate::sys::mount::Mount;
use crate::sys::pool::{PR_WAITOK, PR_ZERO};
use crate::sys::rwlock::{RWL_DUPOK, RWL_IS_VNODE};
use crate::sys::ucred::{FSCRED, NOCRED, Ucred};
use crate::sys::vnode::{VDIR, VLARVAL, VT_NFS, Vnode, VopInactiveArgs, VopReclaimArgs};

/// The nfsmount of an NFS vnode (`VFSTONFS(vp->v_mount)`).
pub(crate) fn vfstonfs_vp(vp: &Vnode) -> &'static NfsMount {
    match vp.v_mount.get() {
        Some(mp) => VFSTONFS(mp),
        None => panic(format_args!("nfs: vnode {:p} has no mount", vp)),
    }
}

/// The credential `cred` points at, `None` for NULL, `NOCRED` and `FSCRED`.
fn nfs_realcred(cred: *const Ucred) -> Option<&'static Ucred> {
    if cred.is_null() || cred == NOCRED || cred == FSCRED {
        return None;
    }
    // SAFETY: any other credential pointer the NFS code stores or is handed (a VOP's
    // `a_cred`, `n_rcred`, `n_wcred`) is a `crget`ed credential on which its holder keeps a
    // reference while it is used here.
    Some(unsafe { &*cred })
}

/// `crhold(cred)` of a credential pointer about to be kept in `n_rcred`/`n_wcred`.
pub(crate) fn nfs_crhold(cred: *const Ucred) {
    if let Some(cr) = nfs_realcred(cred) {
        crhold(cr);
    }
}

/// `if (cred) crfree(cred)` of a long-lived credential pointer (`n_rcred`, `n_wcred`).
pub(crate) fn nfs_crfree(cred: *const Ucred) {
    if let Some(cr) = nfs_realcred(cred) {
        crfree(cr);
    }
}

/// `nfs_ninit(nmp)`: empty the mount's node tree.
pub fn nfs_ninit(nmp: &NfsMount) {
    nmp.nm_ntree.init();
}

/// `nfs_nget(mnt, fh, fhsize, npp)`: look up the vnode/nfsnode of the file handle `fh`,
/// making one when there is none; the node comes back with its vnode referenced and locked.
/// Callers must check for mount points!!
pub fn nfs_nget(mnt: &'static Mount, fh: &[u8]) -> Result<&'static NfsNode, Errno> {
    let nmp = VFSTONFS(mnt);

    let find = NfsNode::new();
    find.set_fh(fh);

    loop {
        // loop:
        if let Some(np) = nmp.nm_ntree.find(&find) {
            let vp = NFSTOV(np);
            let vpid = vp.v_id.get();
            if vget(vp, LK_EXCLUSIVE).is_err() {
                continue;
            }
            if vpid != vp.v_id.get() {
                vput(vp);
                continue;
            }
            return Ok(np);
        }

        // getnewvnode() could recycle a vnode, potentially formerly owned by NFS. This will
        // cause a VOP_RECLAIM() to happen, which will cause recursive locking, so we unlock
        // before calling getnewvnode() lock again afterwards, but must check to see if this
        // nfsnode has been added while we did not hold the lock.
        let nvp = getnewvnode(VT_NFS, Some(mnt), &NFS_VOPS)?;
        // note that we don't have this vnode set up completely yet
        nvp.v_flag.set(nvp.v_flag.get() | VLARVAL);
        let Some(mem) = pool_get(&NFS_NODE_POOL, PR_WAITOK | PR_ZERO) else {
            panic(format_args!("nfs_nget: pool_get"));
        };
        let p = mem.cast::<NfsNode>();
        // SAFETY: a fresh `nfs_node_pool` item (sized for `struct nfsnode` by
        // `nfs_vfs_init`), written once before anything else sees it.
        unsafe { p.as_ptr().write(NfsNode::new()) };

        // getnewvnode() and pool_get() can sleep, check for race.
        if nmp.nm_ntree.find(&find).is_some() {
            pool_put(&NFS_NODE_POOL, mem);
            vgone(nvp);
            continue;
        }

        // SAFETY: as above; the node lives until `nfs_reclaim` returns it to the pool.
        let np: &'static NfsNode = unsafe { p.as_ref() };
        let vp = nvp;
        rrw_init_flags(&np.n_lock, "nfsnode", RWL_DUPOK | RWL_IS_VNODE);
        vp.v_data.set(p.as_ptr().cast::<c_void>());
        // we now have an nfsnode on this vnode
        vp.v_flag.set(vp.v_flag.get() & !VLARVAL);
        np.n_vnode.set(Some(vp));
        rw_init(&np.n_commitlock, "nfs_commitlk");
        np.set_fh(fh);
        // lock the nfsnode, then put it on the rbtree
        let _ = VOP_LOCK(vp, LK_EXCLUSIVE);
        // SAFETY: the node is fresh, so in no tree, and it stays in place (a pool item) until
        // `nfs_reclaim` removes it from this tree.
        let np2 = unsafe { nmp.nm_ntree.insert(np) };
        kassert!(np2.is_none());
        np.n_accstamp.set(-1);
        return Ok(np);
    }
}

/// `nfs_inactive` (`vop_inactive`): last reference to the vnode. Flush and remove the file a
/// silly rename left behind, and unlock the vnode.
pub fn nfs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    #[cfg(feature = "diagnostic")]
    if PRTACTIVE.load(core::sync::atomic::Ordering::Relaxed) != 0 && vp.v_usecount.get() != 0 {
        crate::kern::vfs_subr::vprint(Some("nfs_inactive: pushing active"), vp);
    }
    if vp.v_flag.get() & VLARVAL != 0 {
        // vnode was incompletely set up, just return as we are throwing it away.
        return Ok(());
    }
    #[cfg(feature = "diagnostic")]
    if vp.v_data.get().is_null() {
        panic(format_args!(
            "NULL v_data (no nfsnode set up?) in vnode {:p}",
            vp
        ));
    }
    let np = VTONFS(vp);
    let sp = if vp.v_type.get() != VDIR {
        np.n_sillyrename.take()
    } else {
        None
    };
    if let Some(sp) = sp {
        // SAFETY: `n_sillyrename` points at the record `nfs_sillyrename` `malloc`ed and
        // filled; it was just taken off the node, so this function owns it until the `free`
        // below.
        let s = unsafe { sp.as_ref() };
        let _ = nfs_vinvalbuf(vp, 0, ptr::from_ref(s.s_cred), curproc());
    }
    np.n_flag
        .set(np.n_flag.get() & (NMODIFIED | NFLUSHINPROG | NFLUSHWANT));

    let _ = VOP_UNLOCK(vp);

    if let Some(sp) = sp {
        // SAFETY: as above.
        let s = unsafe { sp.as_ref() };
        // Remove the silly file that was rename'd earlier
        let _ = vn_lock(s.s_dvp, LK_EXCLUSIVE | LK_RETRY);
        let _ = nfs_removeit(s);
        crfree(s.s_cred);
        vput(s.s_dvp);
        free(
            sp.cast::<u8>(),
            M_NFSREQ,
            size_of::<crate::nfs::nfsnode::Sillyrename>(),
        );
    }

    Ok(())
}

/// `nfs_reclaim` (`vop_reclaim`): reclaim an nfsnode so that it can be used for other
/// purposes.
pub fn nfs_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    #[cfg(feature = "diagnostic")]
    if PRTACTIVE.load(core::sync::atomic::Ordering::Relaxed) != 0 && vp.v_usecount.get() != 0 {
        crate::kern::vfs_subr::vprint(Some("nfs_reclaim: pushing active"), vp);
    }
    if vp.v_flag.get() & VLARVAL != 0 {
        // vnode was incompletely set up, just return as we are throwing it away.
        return Ok(());
    }
    #[cfg(feature = "diagnostic")]
    if vp.v_data.get().is_null() {
        panic(format_args!(
            "NULL v_data (no nfsnode set up?) in vnode {:p}",
            vp
        ));
    }
    let np = VTONFS(vp);
    let nmp = vfstonfs_vp(vp);
    // SAFETY: a node whose vnode is set up (not `VLARVAL`) was put in its mount's tree by
    // `nfs_nget`, and only this function takes it out.
    unsafe { nmp.nm_ntree.remove(np) };

    nfs_crfree(np.n_rcred.get());
    nfs_crfree(np.n_wcred.get());

    cache_purge(vp);
    match NonNull::new(vp.v_data.get()) {
        Some(data) => pool_put(&NFS_NODE_POOL, data.cast::<u8>()),
        None => panic(format_args!("nfs_reclaim: vnode {:p} without nfsnode", vp)),
    }
    vp.v_data.set(ptr::null_mut());

    Ok(())
}

const _: () = assert!(!core::mem::needs_drop::<NfsNode>());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nfs::nfsnode::NfsNodetree;
    use crate::sys::tree::RbtHead;

    /// The tree `nfs_nget` searches finds a node by a key holding a copy of its handle,
    /// and keeps handles of different sizes apart.
    #[test]
    fn nodetree_finds_by_handle() {
        let tree: RbtHead<NfsNodetree> = RbtHead::new();
        let nodes: std::vec::Vec<&'static NfsNode> = (0..8u8)
            .map(|i| {
                let np: &'static NfsNode = std::boxed::Box::leak(std::boxed::Box::default());
                let len = if i % 2 == 0 { 32 } else { 28 };
                let mut fh = [0u8; 32];
                fh[0] = i;
                np.set_fh(&fh[..len]);
                np
            })
            .collect();
        for np in &nodes {
            // SAFETY: leaked nodes, each inserted once and never moved.
            assert!(unsafe { tree.insert(np) }.is_none());
        }
        let find = NfsNode::new();
        let mut fh = [0u8; 32];
        fh[0] = 3;
        find.set_fh(&fh[..28]);
        assert!(tree.find(&find).is_some_and(|np| ptr::eq(np, nodes[3])));
        find.set_fh(&fh[..32]);
        assert!(tree.find(&find).is_none());
        // SAFETY: nodes[3] is in the tree.
        unsafe { tree.remove(nodes[3]) };
        find.set_fh(&fh[..28]);
        assert!(tree.find(&find).is_none());
        // A duplicate handle is refused, as `nfs_nget`'s KASSERT expects never to see.
        // SAFETY: nodes[3] is in no tree now.
        assert!(unsafe { tree.insert(nodes[3]) }.is_none());
        let dup: &'static NfsNode = std::boxed::Box::leak(std::boxed::Box::default());
        dup.set_fh(&fh[..28]);
        // SAFETY: a leaked node in no tree.
        assert!(unsafe { tree.insert(dup) }.is_some());
    }
}
