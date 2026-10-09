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

/*-
 * Copyright (C) 1994, 1995, 1997 Wolfgang Solfrank.
 * Copyright (C) 1994, 1995, 1997 TooLs GmbH.
 * All rights reserved.
 * Original code by Paul Popelka (paulp@uts.amdahl.com) (see below).
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by TooLs GmbH.
 * 4. The name of TooLs GmbH may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY TOOLS GMBH ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL TOOLS GMBH BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/*
 * Written by Paul Popelka (paulp@uts.amdahl.com)
 *
 * You can do anything you want with this software, just don't say you wrote
 * it, and don't remove this notice.
 *
 * This software is provided "as is".
 *
 * The author supplies this software to be publicly redistributed on the
 * understanding that the author is not responsible for the correct
 * functioning of this software in any circumstances and is not liable for
 * any damages caused by this software.
 *
 * October 1992
 */
/* </LICENSES> */

/* <CODE> */
//! The denode cache and the life of a denode: `msdosfs_init` (the hash table), `deget` (find
//! or make the denode of a directory entry, with its vnode), `deupdat` (write the times and
//! size back to the entry), `detrunc` and `deextend` (change a file's length), `reinsert`
//! (rehash after a rename), and the vnode operations `msdosfs_reclaim` and
//! `msdosfs_inactive`.
//!
//! Upstream: sys/msdosfs/msdosfs_denode.c @ 3ce1f3f79392
//!
//! The chains are picked by a SipHash of the device and the entry's position under a random
//! key, so that an attacker cannot predict which denodes collide.
//!
//! ## Deviations
//! - `dehashtbl`, `dehash` (the table's size - 1) and `dehashkey` are `StaticCell`s written
//!   once by `msdosfs_init` (from `vfsinit`, before any mount) and read afterwards, as
//!   `ufs_ihash.rs` does.
//! - The chains are `queue.h` lists of [`DeHash`] (the C links `de_next`/`de_prev` by hand,
//!   the same doubly linked list); `de_prev == NULL` is `de_hashed`, which
//!   `msdosfs_hashrem` also clears, as the C does only under `DIAGNOSTIC`, so that a denode
//!   unhashed twice (a `reinsert` whose `msdosfs_hashins` failed, then `msdosfs_reclaim`) is
//!   not unlinked twice.
//! - `deget` returns the denode where the C fills `*depp` (and NULLs it on error). As in the
//!   C, a failing `readep` leaves the new vnode locked and hashed.
//! - The `MSDOSFS_DEBUG` `printf`s are left out: the option is not in GENERIC and has no
//!   feature here.
//! - `msdosfs_reclaim`'s and `msdosfs_inactive`'s `DIAGNOSTIC` `prtactive` reports are behind
//!   feature `diagnostic`.
//! - `MSDOSFS_VOPS` (`msdosfs_vops`) and `readep`/`readde` come from `msdosfs_vnops.rs` and
//!   `msdosfs_lookup.rs`.

use core::ptr::{self, NonNull};

use crate::crypto::siphash::{SipHash24Ctx, SiphashKey};
use crate::dev::rnd::arc4random_buf;
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::rrw_init_flags;
use crate::kern::kern_subr::hashinit;
use crate::kern::kern_tc::getnanotime;
use crate::kern::subr_prf::{panic, printf};
use crate::kern::vfs_bio::{bdwrite, bread, brelse, bwrite};
use crate::kern::vfs_cache::cache_purge;
#[cfg(feature = "diagnostic")]
use crate::kern::vfs_subr::PRTACTIVE;
use crate::kern::vfs_subr::{getnewvnode, vget, vinvalbuf, vput, vrecycle, vref, vrele};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::VOP_UNLOCK;
use crate::machine::cpu::curproc;
use crate::msdosfs::denode::{
    DE_MODIFIED, DE_UPDATE, DeHash, Denode, FC_LASTFC, MSDOSFSROOT_OFS, de_externalize,
    de_internalize, detimes, fc_setcache, vtode,
};
use crate::msdosfs::direntry::{
    ATTR_DIRECTORY, DD_DAY_SHIFT, DD_MONTH_SHIFT, Direntry, SLOT_DELETED,
};
use crate::msdosfs::fat::{
    CLUST_END, CLUST_EOFE, DE_CLEAR, FAT_GET_AND_SET, MSDOSFSROOT, fat32, msdosfseof,
};
use crate::msdosfs::msdosfs_fat::{extendfile, fatentry, fc_purge, freeclusterchain, pcbmap};
use crate::msdosfs::msdosfs_lookup::{readde, readep};
use crate::msdosfs::msdosfs_vnops::MSDOSFS_VOPS;
use crate::msdosfs::msdosfsmount::{
    Msdosfsmount, cntobn, de_blk, de_clcount, de_cluster, de_cn2off,
};
use crate::sys::buf::Buf;
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::{M_MSDOSFSMNT, M_MSDOSFSNODE, M_WAITOK, M_ZERO};
use crate::sys::mount::{MNT_RDONLY, Vfsconf};
use crate::sys::proc::Proc;
use crate::sys::queue::ListHead;
use crate::sys::rwlock::{RWL_DUPOK, RWL_IS_VNODE};
use crate::sys::systm::INFSLP;
use crate::sys::types::{Daddr, Dev};
use crate::sys::ucred::{NOCRED, Ucred};
use crate::sys::vnode::{
    IO_SYNC, V_SAVE, V_SAVEMETA, VDIR, VREG, VROOT, VT_MSDOSFS, VopInactiveArgs, VopReclaimArgs,
};
use crate::uvm::uvm_vnode::{uvm_vnp_setsize, uvm_vnp_uncache};
use libkern::StaticCell;

/// The hash chains, with the claim that makes them shareable.
#[derive(Clone, Copy)]
struct Dehashtbl(&'static [ListHead<DeHash>]);

// SAFETY: the chains are changed under the kernel lock, as in C.
unsafe impl Sync for Dehashtbl {}
// SAFETY: as above: the kernel lock.
unsafe impl Send for Dehashtbl {}

/// `dehashtbl`: the denode hash chains.
static DEHASHTBL: StaticCell<Dehashtbl> = StaticCell::new(Dehashtbl(&[]));
/// `dehash`: size of hash table - 1.
static DEHASH: StaticCell<u64> = StaticCell::new(0);
/// `dehashkey`.
static DEHASHKEY: StaticCell<SiphashKey> = StaticCell::new(SiphashKey { k0: 0, k1: 0 });

/// The data of a buffer the caller owns.
///
/// # Safety
///
/// As `Buf::data`: the buffer is busy for the caller and mapped, and no other slice of it
/// is alive.
#[allow(clippy::mut_from_ref)] // the B_BUSY owner's view, as the C's b_data
unsafe fn bdata(bp: &Buf) -> &mut [u8] {
    // SAFETY: the caller's contract.
    unsafe { bp.data() }
}

/// `msdosfs_init(vfsp)` (`vfs_init`): set up the denode hash table.
pub fn msdosfs_init(_vfsp: &'static Vfsconf) -> Result<(), Errno> {
    let elements =
        crate::conf::param::INITIALVNODES.load(core::sync::atomic::Ordering::Relaxed) / 2;
    let Some(tbl) = hashinit::<DeHash>(elements, M_MSDOSFSMNT, M_WAITOK) else {
        panic(format_args!("msdosfs_init: no memory"));
    };
    let mut key = [0u8; 16];
    arc4random_buf(&mut key);
    let mut k0 = [0u8; 8];
    let mut k1 = [0u8; 8];
    k0.copy_from_slice(&key[..8]);
    k1.copy_from_slice(&key[8..]);
    // SAFETY: called once, from `vfsinit` before any msdos file system is mounted, so nothing
    // reads the cells meanwhile (the module's deviations).
    unsafe {
        DEHASHTBL.write(Dehashtbl(tbl));
        DEHASH.write(tbl.len() as u64 - 1);
        DEHASHKEY.write(SiphashKey {
            k0: u64::from_ne_bytes(k0),
            k1: u64::from_ne_bytes(k1),
        });
    }
    Ok(())
}

/// `msdosfs_dehash(dev, dirclust, diroff)` (`DEHASH`): the index of the chain of a directory
/// entry.
pub fn msdosfs_dehash(dev: Dev, dirclust: u32, diroff: u32) -> u32 {
    // SAFETY: the cells are written only by `msdosfs_init`, before any msdos file system is
    // mounted, and read-only afterwards.
    let (mask, key) = unsafe { (*DEHASH.get(), DEHASHKEY.get()) };

    let mut ctx = SipHash24Ctx::new(key);
    ctx.update(&dev.to_ne_bytes());
    ctx.update(&dirclust.to_ne_bytes());
    ctx.update(&diroff.to_ne_bytes());

    (ctx.end() & mask) as u32
}

/// `&dehashtbl[DEHASH(dev, dcl, doff)]`.
fn dehash_chain(dev: Dev, dirclust: u32, diroff: u32) -> &'static ListHead<DeHash> {
    // SAFETY: as in `msdosfs_dehash`.
    let tbl = unsafe { DEHASHTBL.get().0 };
    if tbl.is_empty() {
        panic(format_args!("msdosfs_dehash: no table"));
    }
    &tbl[msdosfs_dehash(dev, dirclust, diroff) as usize]
}

/// `msdosfs_hashget(dev, dirclust, diroff)`: the denode of the entry, its vnode referenced and
/// locked, when it is in the cache.
fn msdosfs_hashget(dev: Dev, dirclust: u32, diroff: u32) -> Option<&'static Denode> {
    'retry: loop {
        for dep in dehash_chain(dev, dirclust, diroff).iter() {
            if dirclust == dep.de_dirclust.get()
                && diroff == dep.de_diroffset.get()
                && dev == dep.de_dev.get()
                && dep.de_refcnt.get() != 0
            {
                let vp = dep.detov();
                let vpid = vp.v_id.get();

                if vget(vp, LK_EXCLUSIVE).is_ok() {
                    if vpid != vp.v_id.get() {
                        vput(vp);
                        continue 'retry;
                    }
                    return Some(dep);
                }
                continue 'retry;
            }
        }
        return None;
    }
}

/// `msdosfs_hashins(dep)`: put the denode on its chain; `EEXIST` when a live denode of the
/// same entry is there already.
fn msdosfs_hashins(dep: &'static Denode) -> Result<(), Errno> {
    let depp = dehash_chain(
        dep.de_dev.get(),
        dep.de_dirclust.get(),
        dep.de_diroffset.get(),
    );

    for deq in depp.iter() {
        if dep.de_dirclust.get() == deq.de_dirclust.get()
            && dep.de_diroffset.get() == deq.de_diroffset.get()
            && dep.de_dev.get() == deq.de_dev.get()
            && deq.de_refcnt.get() != 0
        {
            return Err(Errno::EEXIST);
        }
    }

    kassert!(!dep.de_hashed.get());
    // SAFETY: the denode is on no chain (`de_hashed` is clear, and is set from now on until
    // `msdosfs_hashrem` unlinks it); it is a `malloc`ed denode that `msdosfs_reclaim` unhashes
    // before freeing it.
    unsafe { depp.insert_head(dep) };
    dep.de_hashed.set(true);
    Ok(())
}

/// `msdosfs_hashrem(dep)`: take the denode off its chain, if it is on one.
fn msdosfs_hashrem(dep: &Denode) {
    if !dep.de_hashed.get() {
        return;
    }

    // SAFETY: `de_hashed` says the denode is linked on its chain (`msdosfs_hashins`).
    unsafe { ListHead::<DeHash>::remove(dep) };
    dep.de_hashed.set(false);
}

/// `deget(pmp, dirclust, diroffset, depp)`: the denode of the directory entry at
/// (`dirclust`, `diroffset`), returned with its vnode locked.
///
/// - `pmp`: the msdosfsmount structure of the filesystem containing the denode of interest.
///   The `pm_dev` field and the address of the msdosfsmount structure are used.
/// - `dirclust`: which cluster bp contains; if `dirclust` is 0 (root directory) `diroffset` is
///   relative to the beginning of the root directory, otherwise it is cluster relative.
/// - `diroffset`: offset past begin of cluster of denode we want
pub fn deget(
    pmp: &'static Msdosfsmount,
    mut dirclust: u32,
    diroffset: u32,
) -> Result<&'static Denode, Errno> {
    // On FAT32 filesystems, root is a (more or less) normal directory.
    if fat32(pmp) && dirclust == MSDOSFSROOT {
        dirclust = pmp.pm_rootdirblk.get();
    }

    // See if the denode is in the denode cache. Use the location of the directory entry to
    // compute the hash value. For subdir use address of "." entry. For root dir (if not FAT32)
    // use cluster MSDOSFSROOT, offset MSDOSFSROOT_OFS.
    //
    // NOTE: The check for de_refcnt > 0 below insures the denode being examined does not
    // represent an unlinked but still open file. These files are not to be accessible even
    // when the directory entry that represented the file happens to be reused while the
    // deleted file is still open.
    let (ldep, nvp) = loop {
        // retry:
        if let Some(ldep) = msdosfs_hashget(pmp.pm_dev.get(), dirclust, diroffset) {
            return Ok(ldep);
        }

        // Directory entry was not in cache, have to create a vnode and copy it from the passed
        // disk buffer.
        // getnewvnode() does a vref() on the vnode
        let nvp = getnewvnode(VT_MSDOSFS, Some(pmp.mountp()), &MSDOSFS_VOPS)?;
        let Some(mem) = malloc(size_of::<Denode>(), M_MSDOSFSNODE, M_WAITOK | M_ZERO) else {
            panic(format_args!("deget: malloc"));
        };
        let p = mem.cast::<Denode>();
        // SAFETY: a fresh allocation of `size_of::<Denode>()` bytes, aligned by malloc(9),
        // written once before anything else sees it.
        unsafe { p.as_ptr().write(Denode::new()) };
        // SAFETY: as above; it lives until `msdosfs_reclaim` frees it.
        let ldep: &'static Denode = unsafe { p.as_ref() };
        rrw_init_flags(&ldep.de_lock, "denode", RWL_DUPOK | RWL_IS_VNODE);
        nvp.v_data.set(p.as_ptr().cast());
        ldep.de_vnode.set(Some(nvp));
        ldep.de_flag.set(0);
        ldep.de_devvp.set(None);
        ldep.de_lockf.set(None);
        ldep.de_dev.set(pmp.pm_dev.get());
        ldep.de_dirclust.set(dirclust);
        ldep.de_diroffset.set(diroffset);
        fc_purge(ldep, 0); // init the fat cache for this denode

        // Insert the denode into the hash queue and lock the denode so it can't be accessed
        // until we've read it in and have done what we need to it.
        let _ = vn_lock(nvp, LK_EXCLUSIVE | LK_RETRY);
        match msdosfs_hashins(ldep) {
            Ok(()) => break (ldep, nvp),
            Err(error) => {
                vput(nvp);

                if error == Errno::EEXIST {
                    continue;
                }

                return Err(error);
            }
        }
    };

    ldep.de_pmp.set(Some(pmp));
    ldep.de_devvp.set(Some(pmp.devvp()));
    ldep.de_refcnt.set(1);
    // Copy the directory entry into the denode area of the vnode.
    if (dirclust == MSDOSFSROOT || (fat32(pmp) && dirclust == pmp.pm_rootdirblk.get()))
        && diroffset == MSDOSFSROOT_OFS
    {
        // Directory entry for the root directory. There isn't one, so we manufacture one. We
        // should probably rummage through the root directory and find a label entry (if it
        // exists), and then use the time and date from that entry as the time and date for
        // the root denode.
        nvp.v_flag.set(nvp.v_flag.get() | VROOT); // should be further down XXX

        ldep.de_Attributes.set(ATTR_DIRECTORY);
        if fat32(pmp) {
            ldep.de_StartCluster.set(pmp.pm_rootdirblk.get());
            // de_FileSize will be filled in further down
        } else {
            ldep.de_StartCluster.set(MSDOSFSROOT);
            ldep.de_FileSize.set(
                pmp.pm_rootdirsize
                    .get()
                    .wrapping_mul(u32::from(pmp.pm_BytesPerSec())),
            );
        }
        // fill in time and date so that dos2unixtime() doesn't spit up when called from
        // msdosfs_getattr() with root denode
        ldep.de_CTime.set(0x0000); // 00:00:00
        ldep.de_CTimeHundredth.set(0);
        // Year 0 (`0 << DD_YEAR_SHIFT`), month 1, day 1: Jan 1, 1980.
        ldep.de_CDate
            .set((1 << DD_MONTH_SHIFT | 1 << DD_DAY_SHIFT) as u16);
        ldep.de_ADate.set(ldep.de_CDate.get());
        ldep.de_MTime.set(ldep.de_CTime.get());
        ldep.de_MDate.set(ldep.de_CDate.get());
        // leave the other fields as garbage
    } else {
        let (bp, off) = readep(pmp, dirclust, diroffset)?;
        // SAFETY: the buffer is busy for this function (from `readep`) and mapped; the slice
        // ends with the statement.
        de_internalize(ldep, Direntry::at(unsafe { bdata(bp) }, off));
        brelse(bp);
    }

    // Fill in a few fields of the vnode and finish filling in the denode. Then return the
    // address of the found denode.
    if ldep.de_Attributes.get() & ATTR_DIRECTORY != 0 {
        // Since DOS directory entries that describe directories have 0 in the filesize field,
        // we take this opportunity to find out the length of the directory and plug it into
        // the denode structure.
        nvp.v_type.set(VDIR);
        if ldep.de_StartCluster.get() != MSDOSFSROOT {
            let mut size = 0;
            match pcbmap(ldep, CLUST_END, None, Some(&mut size), None) {
                Err(Errno::E2BIG) => ldep.de_FileSize.set(de_cn2off(pmp, size)),
                Err(error) => {
                    printf(format_args!("deget(): pcbmap returned {}\n", error as i32));
                    return Err(error);
                }
                Ok(()) => {}
            }
        }
    } else {
        nvp.v_type.set(VREG);
    }
    vref(pmp.devvp());
    Ok(ldep)
}

/// `deupdat(dep, waitfor)`: write the denode's times, attributes and size back to its
/// directory entry, synchronously when `waitfor` is nonzero.
pub fn deupdat(dep: &Denode, waitfor: i32) -> Result<(), Errno> {
    if dep
        .detov()
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
    {
        return Ok(());
    }
    let ts = getnanotime();
    detimes(dep, &ts, &ts, &ts);
    if dep.de_flag.get() & DE_MODIFIED == 0 {
        return Ok(());
    }
    dep.clr_flag(DE_MODIFIED);
    if dep.de_Attributes.get() & ATTR_DIRECTORY != 0 {
        return Ok(());
    }
    if dep.de_refcnt.get() <= 0 {
        return Ok(());
    }
    let (bp, off) = readde(dep)?;
    // SAFETY: the buffer is busy for this function (from `readde`) and mapped; the slice
    // ends with the statement.
    de_externalize(Direntry::at_mut(unsafe { bdata(bp) }, off), dep);
    if waitfor != 0 {
        bwrite(bp)
    } else {
        bdwrite(bp);
        Ok(())
    }
}

/// `detrunc(dep, length, flags, cred, p)`: truncate the file described by `dep` to the length
/// specified by `length`.
pub fn detrunc(
    dep: &Denode,
    length: u32,
    flags: i32,
    cred: *const Ucred,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    let isadir = dep.de_Attributes.get() & ATTR_DIRECTORY != 0;
    let pmp = dep.pmp();
    let vp = dep.detov();
    let mut chaintofree = 0;

    // Disallow attempts to truncate the root directory since it is of fixed size. That's
    // just the way dos filesystems are. We use the VROOT bit in the vnode because checking for
    // the directory bit and a startcluster of 0 in the denode is not adequate to recognize the
    // root directory at this point in a file or directory's life.
    if vp.v_flag.get() & VROOT != 0 && !fat32(pmp) {
        printf(format_args!(
            "detrunc(): can't truncate root directory, clust {}, offset {}\n",
            dep.de_dirclust.get(),
            dep.de_diroffset.get()
        ));
        return Err(Errno::EINVAL);
    }

    uvm_vnp_setsize(vp, i64::from(length));

    if dep.de_FileSize.get() < length {
        return deextend(dep, length, cred);
    }

    // If the desired length is 0 then remember the starting cluster of the file and set the
    // StartCluster field in the directory entry to 0. If the desired length is not zero, then
    // get the number of the last cluster in the shortened file. Then get the number of the
    // first cluster in the part of the file that is to be freed. Then set the next cluster
    // pointer in the last cluster of the file to CLUST_EOFE.
    let eofentry = if length == 0 {
        chaintofree = dep.de_StartCluster.get();
        dep.de_StartCluster.set(0);
        !0
    } else {
        let mut eofentry = 0;
        pcbmap(
            dep,
            de_clcount(pmp, length) - 1,
            None,
            Some(&mut eofentry),
            None,
        )?;
        eofentry
    };

    fc_purge(dep, de_clcount(pmp, length));

    // If the new length is not a multiple of the cluster size then we must zero the tail end
    // of the new last cluster in case it becomes part of the file again because of a seek.
    let boff = length & pmp.pm_crbomask.get();
    if boff != 0 {
        let (bp, error) = if isadir {
            let bn = cntobn(pmp, eofentry);
            bread(pmp.devvp(), Daddr::from(bn), pmp.pm_bpcluster.get() as i32)
        } else {
            let bn = de_blk(pmp, length);
            bread(vp, Daddr::from(bn), pmp.pm_bpcluster.get() as i32)
        };
        if let Err(error) = error {
            brelse(bp);
            return Err(error);
        }
        uvm_vnp_uncache(vp);
        // is this the right place for it?
        // SAFETY: the buffer is busy for this function (from `bread`) and mapped; the slice
        // ends before the buffer is written.
        let data = unsafe { bdata(bp) };
        data[boff as usize..pmp.pm_bpcluster.get() as usize].fill(0);
        if flags & IO_SYNC != 0 {
            let _ = bwrite(bp);
        } else {
            bdwrite(bp);
        }
    }

    // Write out the updated directory entry. Even if the update fails we free the trailing
    // clusters.
    dep.de_FileSize.set(length);
    if !isadir {
        dep.set_flag(DE_UPDATE | DE_MODIFIED);
    }
    let vflags = (if length > 0 { V_SAVE } else { 0 }) | V_SAVEMETA;
    let _ = vinvalbuf(vp, vflags, cred, p, 0, INFSLP);
    let allerror = deupdat(dep, 1);

    // If we need to break the cluster chain for the file then do it now.
    if eofentry != !0 {
        fatentry(
            FAT_GET_AND_SET,
            pmp,
            eofentry,
            Some(&mut chaintofree),
            CLUST_EOFE,
        )?;
        fc_setcache(dep, FC_LASTFC, de_cluster(pmp, length - 1), eofentry);
    }

    // Now free the clusters removed from the file because of the truncation.
    if chaintofree != 0 && !msdosfseof(pmp, chaintofree) {
        let _ = freeclusterchain(pmp, chaintofree);
    }

    allerror
}

/// `deextend(dep, length, cred)`: extend the file described by `dep` to length specified by
/// `length`.
pub fn deextend(dep: &Denode, length: u32, cred: *const Ucred) -> Result<(), Errno> {
    let pmp = dep.pmp();

    // The root of a DOS filesystem cannot be extended.
    if dep.detov().v_flag.get() & VROOT != 0 && !fat32(pmp) {
        return Err(Errno::EINVAL);
    }

    // Directories cannot be extended.
    if dep.de_Attributes.get() & ATTR_DIRECTORY != 0 {
        return Err(Errno::EISDIR);
    }

    if length <= dep.de_FileSize.get() {
        panic(format_args!("deextend: file too large"));
    }

    // Compute the number of clusters to allocate.
    let count = de_clcount(pmp, length) - de_clcount(pmp, dep.de_FileSize.get());
    if count > 0 {
        if count > pmp.pm_freeclustercount.get() {
            return Err(Errno::ENOSPC);
        }
        if let Err(error) = extendfile(dep, count, None, None, DE_CLEAR) {
            // truncate the added clusters away again
            let _ = detrunc(dep, dep.de_FileSize.get(), 0, cred, curproc());
            return Err(error);
        }
    }

    dep.de_FileSize.set(length);
    dep.set_flag(DE_UPDATE | DE_MODIFIED);
    deupdat(dep, 1)
}

/// `reinsert(dep)`: move a denode to its correct hash queue after the file it represents has
/// been moved to a new directory.
pub fn reinsert(dep: &'static Denode) {
    // Fix up the denode cache. If the denode is for a directory, there is nothing to do since
    // the hash is based on the starting cluster of the directory file and that hasn't
    // changed. If for a file the hash is based on the location of the directory entry, so we
    // must remove it from the cache and re-enter it with the hash based on the new location
    // of the directory entry.
    if dep.de_Attributes.get() & ATTR_DIRECTORY != 0 {
        return;
    }
    msdosfs_hashrem(dep);
    let _ = msdosfs_hashins(dep);
}

/// `msdosfs_reclaim` (`vop_reclaim`): unhash the denode and free it, so that the vnode can
/// be used for other purposes.
pub fn msdosfs_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let dep = vtode(vp);

    #[cfg(feature = "diagnostic")]
    if PRTACTIVE.load(core::sync::atomic::Ordering::Relaxed) != 0 && vp.v_usecount.get() != 0 {
        crate::kern::vfs_subr::vprint(Some("msdosfs_reclaim(): pushing active"), vp);
    }

    // Remove the denode from its hash chain.
    msdosfs_hashrem(dep);
    // Purge old data structures associated with the denode.
    cache_purge(vp);
    if let Some(devvp) = dep.de_devvp.take() {
        vrele(devvp);
    }
    // `#if 0 /* XXX */ dep->de_flag = 0;` in the C.
    vp.v_data.set(ptr::null_mut());
    free(NonNull::from(dep).cast(), M_MSDOSFSNODE, 0);
    Ok(())
}

/// `msdosfs_inactive` (`vop_inactive`): last reference to a denode. Truncate a removed file
/// and write the times back; recycle the vnode of a deleted entry at once.
pub fn msdosfs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let dep = vtode(vp);
    let mut error = Ok(());

    #[cfg(feature = "diagnostic")]
    if PRTACTIVE.load(core::sync::atomic::Ordering::Relaxed) != 0 && vp.v_usecount.get() != 0 {
        crate::kern::vfs_subr::vprint(Some("msdosfs_inactive(): pushing active"), vp);
    }

    // Get rid of denodes related to stale file handles.
    if dep.de_Name.get()[0] != SLOT_DELETED {
        // If the file has been deleted and it is on a read/write filesystem, then truncate the
        // file, and mark the directory slot as empty. (This may not be necessary for the dos
        // filesystem.)
        let rdonly = vp
            .v_mount
            .get()
            .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0);
        if dep.de_refcnt.get() <= 0 && !rdonly {
            error = detrunc(dep, 0, 0, NOCRED, ap.a_p);
            let mut name = dep.de_Name.get();
            name[0] = SLOT_DELETED;
            dep.de_Name.set(name);
        }
        let _ = deupdat(dep, 0);
    }

    // out:
    let _ = VOP_UNLOCK(vp);
    // If we are done with the denode, reclaim it so that it can be reused immediately.
    if dep.de_Name.get()[0] == SLOT_DELETED {
        vrecycle(vp, ap.a_p);
    }
    error
}

const _: () = assert!(!core::mem::needs_drop::<Denode>());
/* </CODE> */
