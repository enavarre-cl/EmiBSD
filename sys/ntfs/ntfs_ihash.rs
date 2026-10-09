/* <LICENSES> */
/*-
 * Copyright (c) 1998, 1999 Semen Ustimenko
 * All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	Id: ntfs_ihash.h,v 1.3 1999/05/12 09:42:59 semenu Exp
 */
/*
 * Copyright (c) 1982, 1986, 1989, 1991, 1993, 1995
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
 *	@(#)ufs_ihash.c	8.7 (Berkeley) 5/17/95
 * Id: ntfs_ihash.c,v 1.5 1999/05/12 09:42:58 semenu Exp
 */
/* </LICENSES> */

/* <CODE> */
//! The ntnode hash: the in-core MFT records of every NTFS mount, found by device and record
//! number (`ntfs_nthashlookup`), so that a record has one ntnode however it is reached.
//!
//! Upstream: sys/ntfs/ntfs_ihash.h @ 3ce1f3f79392, sys/ntfs/ntfs_ihash.c @ 3ce1f3f79392
//!
//! The chains are picked by a SipHash of the pair under a random key, so that an attacker
//! cannot predict which records collide.
//!
//! ## Deviations
//! - The header and the file share this module, as `.h`/`.c` pairs do (`siphash.rs`).
//! - `ntfs_nthashtbl`, `ntfs_nthash` (the table's size - 1) and `ntfs_nthashkey` are atomics:
//!   `ntfs_nthashinit` runs at every `ntfs_mount` and sets them on the first one (under the
//!   kernel lock, as the C); the table is published last, so a reader that sees it sees the
//!   mask and the key. `hashinit` cannot sleep between the C's two tests of the table here
//!   (the kernel lock is held throughout), so the C's `hashfree` of a table that lost the race never
//!   runs; it is kept.
//! - `ntfs_nthashlookup` returns `Option`; `ntfs_nthashins` returns `Err(EEXIST)`.

use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use crate::crypto::siphash::{
    SipHash24_End, SipHash24_Init, SipHash24_Update, SiphashCtx, SiphashKey,
};
use crate::dev::rnd::arc4random_buf;
use crate::kern::kern_subr::{hashfree, hashinit};
use crate::kern::subr_prf::panic;
use crate::ntfs::ntfs::Ntfsino;
use crate::ntfs::ntfs_inode::{IN_HASHED, Ntnode, NtnodeHash};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NTFSNTHASH, M_WAITOK};
use crate::sys::queue::ListHead;
use crate::sys::types::Dev;

/// `LIST_HEAD(nthashhead, ntnode)`.
pub type Nthashhead = ListHead<NtnodeHash>;

/// `ntfs_nthashtbl`: the hash chains (null before `ntfs_nthashinit`).
static NTFS_NTHASHTBL: AtomicPtr<Nthashhead> = AtomicPtr::new(core::ptr::null_mut());
/// `ntfs_nthash`: size of hash table - 1.
static NTFS_NTHASH: AtomicU64 = AtomicU64::new(0);
/// `ntfs_nthashkey`, its two halves.
static NTFS_NTHASHKEY: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// The table, `None` before `ntfs_nthashinit`.
fn nthashtbl() -> Option<&'static [Nthashhead]> {
    let p = NTFS_NTHASHTBL.load(Ordering::Acquire);
    if p.is_null() {
        return None;
    }
    let n = NTFS_NTHASH.load(Ordering::Relaxed) as usize + 1;
    // SAFETY: a non-null table is the `hashinit` allocation of `ntfs_nthash + 1` chains
    // (stored before the pointer was published), never freed afterwards.
    Some(unsafe { core::slice::from_raw_parts(p, n) })
}

/// `ntfs_nthashinit`: initialize inode hash table.
pub fn ntfs_nthashinit() {
    if nthashtbl().is_some() {
        return;
    }

    let initialvnodes =
        crate::conf::param::INITIALVNODES.load(core::sync::atomic::Ordering::Relaxed);
    let Some(tbl) = hashinit::<NtnodeHash>(initialvnodes, M_NTFSNTHASH, M_WAITOK) else {
        panic(format_args!("ntfs_nthashinit: no memory"));
    };
    if nthashtbl().is_some() {
        // SAFETY: the table just came from `hashinit` with these arguments and nothing
        // links into it.
        unsafe { hashfree(tbl, initialvnodes, M_NTFSNTHASH) };
        return;
    }

    let mut key = [0u8; 16];
    arc4random_buf(&mut key);
    let mut k0 = [0u8; 8];
    let mut k1 = [0u8; 8];
    k0.copy_from_slice(&key[..8]);
    k1.copy_from_slice(&key[8..]);
    NTFS_NTHASHKEY[0].store(u64::from_ne_bytes(k0), Ordering::Relaxed);
    NTFS_NTHASHKEY[1].store(u64::from_ne_bytes(k1), Ordering::Relaxed);
    NTFS_NTHASH.store(tbl.len() as u64 - 1, Ordering::Relaxed);
    NTFS_NTHASHTBL.store(tbl.as_ptr().cast_mut(), Ordering::Release);
}

/// Forget the table: the host tests reset the memory it lives in between tests, so the next
/// `ntfs_nthashinit` must make a new one.
#[cfg(test)]
pub(crate) fn ntfs_nthash_reset() {
    NTFS_NTHASHTBL.store(core::ptr::null_mut(), Ordering::Release);
}

/// `ntfs_hash(dev, inum)`: the chain index of the pair.
pub fn ntfs_hash(dev: Dev, inum: Ntfsino) -> u32 {
    let key = SiphashKey {
        k0: NTFS_NTHASHKEY[0].load(Ordering::Relaxed),
        k1: NTFS_NTHASHKEY[1].load(Ordering::Relaxed),
    };
    let mut ctx = SiphashCtx::default();
    SipHash24_Init(&mut ctx, &key);
    SipHash24_Update(&mut ctx, &dev.to_ne_bytes());
    SipHash24_Update(&mut ctx, &inum.to_ne_bytes());

    (SipHash24_End(&mut ctx) & NTFS_NTHASH.load(Ordering::Relaxed)) as u32
}

/// `&ntfs_nthashtbl[NTNOHASH(device, inum)]`.
fn nthashchain(dev: Dev, inum: Ntfsino) -> &'static Nthashhead {
    let Some(tbl) = nthashtbl() else {
        panic(format_args!("ntfs_nthash: no table"));
    };
    &tbl[ntfs_hash(dev, inum) as usize]
}

/// `ntfs_nthashlookup(dev, inum)`: use the device/inum pair to find the incore inode, and
/// return a pointer to it. If it is in core, return it, even if it is locked.
pub fn ntfs_nthashlookup(dev: Dev, inum: Ntfsino) -> Option<&'static Ntnode> {
    // XXXLOCKING lock hash list?
    let ipp = nthashchain(dev, inum);
    // XXXLOCKING unlock hash list?
    ipp.iter()
        .find(|ip| inum == ip.i_number.get() && dev == ip.i_dev.get())
}

/// `ntfs_nthashins(ip)`: insert the ntnode into the hash table; `EEXIST` when the pair is
/// hashed already.
pub fn ntfs_nthashins(ip: &'static Ntnode) -> Result<(), Errno> {
    // XXXLOCKING lock hash list?
    let ipp = nthashchain(ip.i_dev.get(), ip.i_number.get());
    for curip in ipp.iter() {
        if ip.i_number.get() == curip.i_number.get() && ip.i_dev.get() == curip.i_dev.get() {
            return Err(Errno::EEXIST);
        }
    }

    ip.set_flag(IN_HASHED);
    // SAFETY: the ntnode is on no chain (a fresh allocation, and `IN_HASHED` says so from
    // now on); it stays in place until `ntfs_nthashrem` unlinks it, which `ntfs_ntput` does
    // before it frees the ntnode.
    unsafe { ipp.insert_head(ip) };
    // XXXLOCKING unlock hash list?

    Ok(())
}

/// `ntfs_nthashrem(ip)`: remove the inode from the hash table.
pub fn ntfs_nthashrem(ip: &Ntnode) {
    // XXXLOCKING lock hash list?
    if ip.i_flag.get() & IN_HASHED != 0 {
        ip.clr_flag(IN_HASHED);
        // SAFETY: `IN_HASHED` said the ntnode is linked on its chain (`ntfs_nthashins`).
        unsafe { ListHead::<NtnodeHash>::remove(ip) };
    }
    // XXXLOCKING unlock hash list?
}
/* </CODE> */
