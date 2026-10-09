/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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
//! The inode hash: the in-core inodes of every UFS mount, found by device and inode number
//! (`ufs_ihashget`), so that a file has one vnode however it is reached.
//!
//! Upstream: sys/ufs/ufs/ufs_ihash.c @ 3ce1f3f79392
//!
//! The chains are picked by a SipHash of the pair under a random key, so that an attacker
//! cannot predict which inodes collide.
//!
//! ## Deviations
//! - `ihashtbl`, `ihash` (the table's size - 1) and `ihashkey` are `StaticCell`s written once
//!   by `ufs_ihashinit` (from `ffs_init`, before any mount) and read afterwards.
//! - `ufs_ihashrem`'s `i_hash.le_prev == NULL` test (an inode that was never hashed) is the
//!   `IN_HASHED` test that follows it: the list links are private to `sys/queue.rs`, and the
//!   flag is set exactly when the inode is linked. The `DIAGNOSTIC` clearing of the links is
//!   `LIST_REMOVE`'s own (`_Q_INVALIDATE`).
//! - The `EXT2FS` branch of `ufs_ihashget` (`IS_EXT2_VNODE`: an ext2fs inode's link count is
//!   `i_e2fs_nlink`, an unsigned 16 bits, so `<= 0` is `== 0`) is under feature `ext2fs`.

use crate::crypto::siphash::{
    SipHash24_End, SipHash24_Init, SipHash24_Update, SiphashCtx, SiphashKey,
};
use crate::dev::rnd::arc4random_buf;
use crate::kern::kern_subr::hashinit;
use crate::kern::sched_bsd::r#yield;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_subr::{vget, vput};
use crate::kern::vfs_vops::{VOP_LOCK, VOP_UNLOCK};
use crate::sys::errno::Errno;
use crate::sys::lock::LK_EXCLUSIVE;
use crate::sys::malloc::{M_UFSMNT, M_WAITOK};
use crate::sys::mount::MNT_RDONLY;
use crate::sys::queue::ListHead;
use crate::sys::types::Dev;
use crate::sys::vnode::Vnode;
use crate::ufs::ufs::dinode::Ufsino;
use crate::ufs::ufs::inode::{IHash, IN_HASHED, Inode};
use libkern::StaticCell;

/// The hash chains, with the claim that makes them shareable.
#[derive(Clone, Copy)]
struct Ihashtbl(&'static [ListHead<IHash>]);

// SAFETY: the chains are changed under the kernel lock (the C's "XXXLOCKING" comments), as
// in C.
unsafe impl Sync for Ihashtbl {}
// SAFETY: as above: the kernel lock.
unsafe impl Send for Ihashtbl {}

/// `LIST_HEAD(ihashhead, inode) *ihashtbl`: the hash chains.
static IHASHTBL: StaticCell<Ihashtbl> = StaticCell::new(Ihashtbl(&[]));
/// `ihash`: size of hash table - 1.
static IHASH: StaticCell<u64> = StaticCell::new(0);
/// `ihashkey`.
static IHASHKEY: StaticCell<SiphashKey> = StaticCell::new(SiphashKey { k0: 0, k1: 0 });

/// `ufs_ihash(dev, inum)` (`INOHASH`): the chain of the pair.
pub fn ufs_ihash(dev: Dev, inum: Ufsino) -> &'static ListHead<IHash> {
    // SAFETY: the three cells are written only by `ufs_ihashinit`, before any file system
    // is mounted, and read-only afterwards.
    let (tbl, mask, key) = unsafe { (IHASHTBL.get().0, *IHASH.get(), IHASHKEY.get()) };
    if tbl.is_empty() {
        panic(format_args!("ufs_ihash: no table"));
    }

    let mut ctx = SiphashCtx::default();
    SipHash24_Init(&mut ctx, key);
    SipHash24_Update(&mut ctx, &dev.to_ne_bytes());
    SipHash24_Update(&mut ctx, &inum.to_ne_bytes());

    &tbl[(SipHash24_End(&mut ctx) & mask) as usize]
}

/// `ufs_ihashinit`: initialize inode hash table.
pub fn ufs_ihashinit() {
    let elements = crate::conf::param::INITIALVNODES.load(core::sync::atomic::Ordering::Relaxed);
    let Some(tbl) = hashinit::<IHash>(elements, M_UFSMNT, M_WAITOK) else {
        panic(format_args!("ufs_ihashinit: no memory"));
    };
    let mut key = [0u8; 16];
    arc4random_buf(&mut key);
    let mut k0 = [0u8; 8];
    let mut k1 = [0u8; 8];
    k0.copy_from_slice(&key[..8]);
    k1.copy_from_slice(&key[8..]);
    // SAFETY: called once, from `ufs_init` before any UFS is mounted, so nothing reads the
    // cells meanwhile (the module's deviations).
    unsafe {
        IHASHTBL.write(Ihashtbl(tbl));
        IHASH.write(tbl.len() as u64 - 1);
        IHASHKEY.write(SiphashKey {
            k0: u64::from_ne_bytes(k0),
            k1: u64::from_ne_bytes(k1),
        });
    }
}

/// `ufs_ihashget(dev, inum)`: use the device/inum pair to find the incore inode, and return
/// its vnode, referenced and locked. If it is in core, but locked, wait for it.
pub fn ufs_ihashget(dev: Dev, inum: Ufsino) -> Option<&'static Vnode> {
    'retry: loop {
        // XXXLOCKING lock hash list
        let ipp = ufs_ihash(dev, inum);
        for ip in ipp.iter() {
            if inum == ip.i_number.get() && dev == ip.i_dev.get() {
                let vp = ip.itov();
                // XXXLOCKING unlock hash list?
                if vget(vp, LK_EXCLUSIVE).is_err() {
                    continue 'retry;
                }
                // Check if the inode is valid. The condition has been adapted from
                // ufs_inactive().
                let rdonly = vp
                    .v_mount
                    .get()
                    .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0);
                // XXX DIP does not cover ext2fs so hack around this for now since this is
                // using ufs_ihashget as well.
                #[cfg(feature = "ext2fs")]
                let unlinked = if crate::ufs::ext2fs::ext2fs_extern::is_ext2_vnode(vp) {
                    ip.i_e2fs_nlink() == 0
                } else {
                    ip.dip_nlink() <= 0
                };
                #[cfg(not(feature = "ext2fs"))]
                let unlinked = ip.dip_nlink() <= 0;
                if unlinked && !rdonly {
                    // This should recycle the inode immediately, unless there are other
                    // threads that try to access it. Pause to give the threads a chance to
                    // finish with the inode.
                    vput(vp);
                    r#yield();
                    continue 'retry;
                }

                return Some(vp);
            }
        }
        // XXXLOCKING unlock hash list?
        return None;
    }
}

/// `ufs_ihashins(ip)`: insert the inode into the hash table, and return it locked.
/// `EEXIST` (with the inode unlocked) when the pair is hashed already.
pub fn ufs_ihashins(ip: &'static Inode) -> Result<(), Errno> {
    let dev = ip.i_dev.get();
    let inum = ip.i_number.get();

    // lock the inode, then put it on the appropriate hash list
    let _ = VOP_LOCK(ip.itov(), LK_EXCLUSIVE);

    // XXXLOCKING lock hash list

    let ipp = ufs_ihash(dev, inum);
    for curip in ipp.iter() {
        if inum == curip.i_number.get() && dev == curip.i_dev.get() {
            // XXXLOCKING unlock hash list?
            let _ = VOP_UNLOCK(ip.itov());
            return Err(Errno::EEXIST);
        }
    }

    ip.set_flag(IN_HASHED);
    // SAFETY: the inode is on no chain (it was not hashed, and the flag says so from now
    // on); it is a pool item that stays in place until `ufs_ihashrem` unlinks it, which
    // `ufs_reclaim` does before the inode is freed.
    unsafe { ipp.insert_head(ip) };
    // XXXLOCKING unlock hash list?

    Ok(())
}

/// `ufs_ihashrem(ip)`: remove the inode from the hash table.
pub fn ufs_ihashrem(ip: &Inode) {
    // XXXLOCKING lock hash list

    if ip.i_flag.get() & IN_HASHED != 0 {
        // SAFETY: `IN_HASHED` says the inode is linked on its chain (`ufs_ihashins`).
        unsafe { ListHead::<IHash>::remove(ip) };
        ip.clr_flag(IN_HASHED);
    }
    // XXXLOCKING unlock hash list?
}
/* </CODE> */
