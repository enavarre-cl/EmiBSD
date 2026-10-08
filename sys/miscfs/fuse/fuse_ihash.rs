/*	$OpenBSD: fuse_ihash.c,v 1.4 2026/07/10 14:43:48 helg Exp $	*/
/*	$NetBSD: ufs_ihash.c,v 1.3 1996/02/09 22:36:04 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)ufs_ihash.c	8.4 (Berkeley) 12/30/93
 */
/* </LICENSES> */

/* <CODE> */
//! The FUSE inode hash: the in-core nodes of every FUSE mount, found by mount and inode
//! number (`fuse_ihashget`), so that a file has one vnode however it is reached.
//!
//! Upstream: sys/miscfs/fuse/fuse_ihash.c @ 3ce1f3f79392
//!
//! The chains are picked by a SipHash of the pair under a random key. The mount is hashed
//! by its address: mounts are unique for all mounted file systems.
//!
//! ## Deviations
//! - `fuse_ihashtbl`, `fuse_ihashsz` (the table's size - 1) and `fuse_ihashkey` are
//!   `StaticCell`s written by `fuse_ihashinit` (from `fusefs_init`, before any FUSE mount)
//!   and read afterwards, as `ufs_ihash.rs` does.
//! - `fuse_ihashrem`'s `i_hash.le_prev == NULL` test (a node that was never hashed) is the
//!   node's `i_hashed` flag (`fusefs_node.rs`), set exactly while the node is linked. The
//!   `DIAGNOSTIC` clearing of the links is `LIST_REMOVE`'s own (`_Q_INVALIDATE`).
//! - `fuse_ihashget` returns the vnode as an `Option` (the C's NULL is `None`).

use core::ptr;
use core::sync::atomic::Ordering;

use libkern::StaticCell;

use crate::conf::param::INITIALVNODES;
use crate::crypto::siphash::{
    SipHash24_End, SipHash24_Init, SipHash24_Update, SiphashCtx, SiphashKey,
};
use crate::dev::rnd::arc4random_buf;
use crate::kern::kern_subr::hashinit;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_subr::{vget, vput};
use crate::kern::vfs_vops::{VOP_LOCK, VOP_UNLOCK};
use crate::miscfs::fuse::fusefs::FusefsMnt;
use crate::miscfs::fuse::fusefs_node::{FusefsIHash, FusefsNode, ITOV};
use crate::sys::errno::Errno;
use crate::sys::lock::LK_EXCLUSIVE;
use crate::sys::malloc::{M_FUSEFS, M_WAITOK};
use crate::sys::queue::ListHead;
use crate::sys::types::Ino;
use crate::sys::vnode::Vnode;

/// The hash chains, with the claim that makes them shareable.
#[derive(Clone, Copy)]
struct FuseIhashtbl(&'static [ListHead<FusefsIHash>]);

// SAFETY: the chains are changed under the kernel lock (the C's "XXXLOCKING" comments), as
// in C.
unsafe impl Sync for FuseIhashtbl {}
// SAFETY: as above: the kernel lock.
unsafe impl Send for FuseIhashtbl {}

/// `LIST_HEAD(fuse_ihashhead, fusefs_node) *fuse_ihashtbl`: the hash chains.
static FUSE_IHASHTBL: StaticCell<FuseIhashtbl> = StaticCell::new(FuseIhashtbl(&[]));
/// `fuse_ihashsz`: size of hash table - 1.
static FUSE_IHASHSZ: StaticCell<u64> = StaticCell::new(0);
/// `fuse_ihashkey`.
static FUSE_IHASHKEY: StaticCell<SiphashKey> = StaticCell::new(SiphashKey { k0: 0, k1: 0 });

/// `fuse_ihash(fmp, inum)`: create a hash for the inode using the mounted file system id.
/// These are guaranteed to be unique for all mounted file systems by `vfs_getnewfsid`.
pub fn fuse_ihash(fmp: &FusefsMnt, inum: Ino) -> &'static ListHead<FusefsIHash> {
    // SAFETY: the three cells are written only by `fuse_ihashinit`, before any FUSE file
    // system is mounted, and read-only afterwards.
    let (tbl, mask, key) = unsafe {
        (
            FUSE_IHASHTBL.get().0,
            *FUSE_IHASHSZ.get(),
            FUSE_IHASHKEY.get(),
        )
    };
    if tbl.is_empty() {
        panic(format_args!("fuse_ihash: no table"));
    }

    let fmp_addr = ptr::from_ref(fmp) as usize;
    let mut ctx = SiphashCtx::default();
    SipHash24_Init(&mut ctx, key);
    SipHash24_Update(&mut ctx, &fmp_addr.to_ne_bytes());
    SipHash24_Update(&mut ctx, &inum.to_ne_bytes());

    &tbl[(SipHash24_End(&mut ctx) & mask) as usize]
}

/// `fuse_ihashinit`: initialize inode hash table.
pub fn fuse_ihashinit() {
    let elements = INITIALVNODES.load(Ordering::Relaxed);
    let Some(tbl) = hashinit::<FusefsIHash>(elements, M_FUSEFS, M_WAITOK) else {
        panic(format_args!("fuse_ihashinit: no memory"));
    };
    let mut key = [0u8; 16];
    arc4random_buf(&mut key);
    let mut k0 = [0u8; 8];
    let mut k1 = [0u8; 8];
    k0.copy_from_slice(&key[..8]);
    k1.copy_from_slice(&key[8..]);
    // SAFETY: called from `fusefs_init` (`vfsinit`), before any FUSE file system is
    // mounted, so nothing reads the cells meanwhile (the module's deviations).
    unsafe {
        FUSE_IHASHTBL.write(FuseIhashtbl(tbl));
        FUSE_IHASHSZ.write(tbl.len() as u64 - 1);
        FUSE_IHASHKEY.write(SiphashKey {
            k0: u64::from_ne_bytes(k0),
            k1: u64::from_ne_bytes(k1),
        });
    }
}

/// `fuse_ihashget(fmp, inum)`: use the fsid/inum pair to find the incore inode, and return
/// its vnode, referenced and locked. If it is in core, but locked, wait for it.
pub fn fuse_ihashget(fmp: &FusefsMnt, inum: Ino) -> Option<&'static Vnode> {
    'loop_: loop {
        // XXXLOCKING lock hash list
        let ipp = fuse_ihash(fmp, inum);
        for ip in ipp.iter() {
            if inum == ip.i_number && ptr::eq(fmp, ip.i_fmp) {
                let vp = ITOV(ip);
                let vpid = vp.v_id.get();
                // XXXLOCKING unlock hash list?
                if vget(vp, LK_EXCLUSIVE).is_err() {
                    continue 'loop_;
                }
                if vpid != vp.v_id.get() {
                    vput(vp);
                    continue 'loop_;
                }

                return Some(vp);
            }
        }
        // XXXLOCKING unlock hash list?
        return None;
    }
}

/// `fuse_ihashins(ip)`: insert the inode into the hash table, and return it locked.
/// `EEXIST` (with the inode unlocked) when the pair is hashed already.
pub fn fuse_ihashins(ip: &'static FusefsNode) -> Result<(), Errno> {
    let fmp = ip.i_fmp;
    let inum = ip.i_number;

    // lock the inode, then put it on the appropriate hash list
    let _ = VOP_LOCK(ITOV(ip), LK_EXCLUSIVE);

    // XXXLOCKING lock hash list

    let ipp = fuse_ihash(fmp, inum);
    for curip in ipp.iter() {
        if inum == curip.i_number && ptr::eq(fmp, curip.i_fmp) {
            // XXXLOCKING unlock hash list?
            let _ = VOP_UNLOCK(ITOV(ip));
            return Err(Errno::EEXIST);
        }
    }

    ip.i_hashed.set(true);
    // SAFETY: the node is on no chain (it was not hashed, and the flag says so from now
    // on); it is `malloc`ed memory that stays in place until `fuse_ihashrem` unlinks it,
    // which `fusefs_reclaim` does before the node is freed.
    unsafe { ipp.insert_head(ip) };
    // XXXLOCKING unlock hash list?

    Ok(())
}

/// `fuse_ihashrem(ip)`: remove the inode from the hash table.
pub fn fuse_ihashrem(ip: &FusefsNode) {
    // XXXLOCKING lock hash list

    if !ip.i_hashed.get() {
        return;
    }
    // SAFETY: `i_hashed` says the node is linked on its chain (`fuse_ihashins`).
    unsafe { ListHead::<FusefsIHash>::remove(ip) };
    ip.i_hashed.set(false);
    // XXXLOCKING unlock hash list?
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::assert;
    use std::boxed::Box;

    use super::*;
    use crate::kern::vfs_subr::getnewvnode;
    use crate::miscfs::fuse::fuse_device::tests::{fake_fmp, setup};
    use crate::miscfs::fuse::fuse_vnops::FUSEFS_VOPS;
    use crate::sys::vnode::VT_FUSEFS;

    /// A vnode and its node for inode `ino` of `fmp`, unhashed.
    fn node(fmp: &'static FusefsMnt, ino: Ino) -> &'static FusefsNode {
        let vp = getnewvnode(VT_FUSEFS, None, &FUSEFS_VOPS).expect("vnode");
        let ip: &'static FusefsNode = Box::leak(Box::new(FusefsNode::new(vp, fmp, ino)));
        vp.v_data.set(ptr::from_ref(ip).cast_mut().cast());
        ip
    }

    #[test]
    fn nodes_are_found_by_mount_and_inode() {
        let (_g, _p) = setup();
        let (fmp_a, fmp_b) = (fake_fmp(1, 4096), fake_fmp(2, 4096));
        let a = node(fmp_a, 7);
        fuse_ihashins(a).expect("insert");
        let _ = VOP_UNLOCK(ITOV(a));
        assert!(fuse_ihashins(node(fmp_a, 7)) == Err(Errno::EEXIST));
        let b = node(fmp_b, 7);
        fuse_ihashins(b).expect("the same inode of another mount");
        let _ = VOP_UNLOCK(ITOV(b));

        let vp = fuse_ihashget(fmp_a, 7).expect("hashed");
        assert!(ptr::eq(vp, ITOV(a)));
        vput(vp);
        assert!(fuse_ihashget(fmp_a, 8).is_none());

        fuse_ihashrem(a);
        assert!(!a.i_hashed.get());
        assert!(fuse_ihashget(fmp_a, 7).is_none());
        fuse_ihashrem(a); // not hashed: nothing to do
        assert!(fuse_ihashget(fmp_b, 7).is_some_and(|vp| {
            vput(vp);
            ptr::eq(vp, ITOV(b))
        }));
    }
}
/* </TESTS> */
