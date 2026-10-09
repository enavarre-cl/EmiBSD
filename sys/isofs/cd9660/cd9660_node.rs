/* <LICENSES> */
/*-
 * Copyright (c) 1994
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley
 * by Pace Willisson (pace@blitz.com).  The Rock Ridge Extension
 * Support code is derived from software contributed to Berkeley
 * by Atsushi Murai (amurai@spec.co.jp).
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
 *	@(#)cd9660_node.h	8.4 (Berkeley) 12/5/94
 */
/*-
 * Copyright (c) 1982, 1986, 1989, 1994
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley
 * by Pace Willisson (pace@blitz.com).  The Rock Ridge Extension
 * Support code is derived from software contributed to Berkeley
 * by Atsushi Murai (amurai@spec.co.jp).
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
 *	@(#)cd9660_node.c	8.5 (Berkeley) 12/5/94
 */
/* </LICENSES> */

/* <CODE> */
//! The in-core ISO 9660 node: `<isofs/cd9660/cd9660_node.h>` (`struct iso_node`, its
//! attributes `ISO_RRIP_INODE`, `VTOI`/`ITOV`) and `isofs/cd9660/cd9660_node.c` (the node
//! hash, `cd9660_inactive`/`cd9660_reclaim`, the attributes and time stamps of a plain
//! ISO 9660 directory record, and the conversion of its dates).
//!
//! Upstream: sys/isofs/cd9660/cd9660_node.h @ 3ce1f3f79392
//! Upstream: sys/isofs/cd9660/cd9660_node.c @ 3ce1f3f79392
//!
//! A node is `malloc(M_ISOFSNODE)`ed by `cd9660_vget_internal`, hung from its vnode's
//! `v_data`, and freed by `cd9660_reclaim`; it is handed around as `&'static IsoNode` while
//! the vnode is held. Its members are `Cell`s, changed under the node's lock (`i_lock`).
//!
//! ## Deviations
//! - The hash chain links `i_next`/`i_prev` are a `queue.h` list entry (`i_hash`, the
//!   [`IsoHash`] adapter); the C's `i_prev == NULL` test (a node that was never hashed) is
//!   the extra member `i_hashed`, since the list links are private to `sys/queue.rs`.
//! - `isohashtbl`, `isohash` and `isohashkey` are `StaticCell`s written by `cd9660_init`
//!   (from `vfsinit`, before any mount) and read afterwards, as in `ufs_ihash.rs`.
//! - `cd9660_defattr` and `cd9660_deftstamp` take the record as an [`IsoDirectoryRecord`]
//!   and the extended attribute buffer as an `Option`; `cd9660_tstamp_conv7`/`17` return
//!   `bool` (the C's 1 and 0) and read the date from a byte slice. The date arithmetic wraps
//!   as the C's `int` does in practice.
//! - `ISO_RRIP_INODE` is the `Copy` structure [`IsoRripInode`] in a `Cell`, changed with
//!   [`IsoNode::update_inode`].
//! - `prtactive`'s `vprint` under `DIAGNOSTIC` is behind feature `diagnostic`.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::crypto::siphash::{
    SipHash24_End, SipHash24_Init, SipHash24_Update, SiphashCtx, SiphashKey,
};
use crate::dev::rnd::arc4random_buf;
use crate::isofs::cd9660::cd9660_extern::IsoMnt;
use crate::isofs::cd9660::cd9660_lookup::cd9660_bufatoff;
use crate::isofs::cd9660::iso::{
    Cdino, IsoDirectoryRecord, IsoExtendedAttributes, isonum_711, isonum_723, isonum_733,
};
use crate::kern::kern_malloc::free;
use crate::kern::kern_subr::hashinit;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::brelse;
use crate::kern::vfs_cache::cache_purge;
use crate::kern::vfs_subr::{vget, vput, vrecycle, vrele};
use crate::kern::vfs_vops::{VOP_LOCK, VOP_UNLOCK};
use crate::queue_adapter;
use crate::sys::buf::{Buf, ClusterInfo};
use crate::sys::errno::Errno;
use crate::sys::lock::LK_EXCLUSIVE;
use crate::sys::malloc::{M_ISOFSMNT, M_ISOFSNODE, M_WAITOK};
use crate::sys::mount::{ISOFSMNT_EXTATT, Vfsconf};
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::rwlock::Rrwlock;
use crate::sys::stat::{S_IFDIR, S_IFREG, S_IRGRP, S_IROTH, S_IRUSR, S_IXGRP, S_IXOTH, S_IXUSR};
use crate::sys::time::Timespec;
use crate::sys::types::{Dev, Gid, Uid};
use crate::sys::vnode::{VT_ISOFS, Vnode, VopInactiveArgs, VopReclaimArgs};
use libkern::StaticCell;

/// `doff_t`.
pub type Doff = u64;

/// `ISO_RRIP_INODE`: the attributes of a node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IsoRripInode {
    /// `iso_atime`: time of last access.
    pub iso_atime: Timespec,
    /// `iso_mtime`: time of last modification.
    pub iso_mtime: Timespec,
    /// `iso_ctime`: time file changed.
    pub iso_ctime: Timespec,
    /// `iso_mode`: files access mode and type.
    pub iso_mode: u16,
    /// `iso_uid`: owner user id.
    pub iso_uid: Uid,
    /// `iso_gid`: owner group id.
    pub iso_gid: Gid,
    /// `iso_links`: links of file.
    pub iso_links: i16,
    /// `iso_rdev`: Major/Minor number for special.
    pub iso_rdev: Dev,
}

/// `struct iso_node`.
///
/// Protected by: the node's lock (`i_lock`), held through its vnode.
pub struct IsoNode {
    /// `i_next`, `i_prev`: hash chain.
    pub i_hash: ListEntry<IsoNode>,
    /// The node is on its hash chain (see the module's deviations).
    pub i_hashed: Cell<bool>,
    /// `i_vnode`: vnode associated with this inode.
    pub i_vnode: Cell<Option<&'static Vnode>>,
    /// `i_devvp`: vnode for block I/O.
    pub i_devvp: Cell<Option<&'static Vnode>>,
    /// `i_flag`: see below.
    pub i_flag: Cell<u32>,
    /// `i_dev`: device where inode resides.
    pub i_dev: Cell<Dev>,
    /// `i_number`: the identity of the inode; we use the actual starting block of the
    /// file.
    pub i_number: Cell<Cdino>,
    /// `i_mnt`: filesystem associated with this inode.
    pub i_mnt: Cell<Option<&'static IsoMnt>>,
    /// `i_endoff`: end of useful stuff in directory.
    pub i_endoff: Cell<Doff>,
    /// `i_diroff`: offset in dir, where we found last entry.
    pub i_diroff: Cell<Doff>,
    /// `i_offset`: offset of free space in directory.
    pub i_offset: Cell<Doff>,
    /// `i_ino`: inode number of found directory.
    pub i_ino: Cell<Cdino>,
    /// `i_lock`: node lock.
    pub i_lock: Rrwlock,
    /// `iso_extent`: extent of file.
    pub iso_extent: Cell<Doff>,
    /// `i_size`.
    pub i_size: Cell<Doff>,
    /// `iso_start`: actual start of data file (may be different from `iso_extent`, if the
    /// file has extended attributes).
    pub iso_start: Cell<Doff>,
    /// `inode`.
    pub inode: Cell<IsoRripInode>,
    /// `i_ci`.
    pub i_ci: Cell<ClusterInfo>,
}

// SAFETY: the members are changed under the node's lock or the kernel lock, as in C.
unsafe impl Sync for IsoNode {}

impl IsoNode {
    /// A zeroed node, as `malloc(M_ISOFSNODE, M_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            i_hash: ListEntry::new(),
            i_hashed: Cell::new(false),
            i_vnode: Cell::new(None),
            i_devvp: Cell::new(None),
            i_flag: Cell::new(0),
            i_dev: Cell::new(0),
            i_number: Cell::new(0),
            i_mnt: Cell::new(None),
            i_endoff: Cell::new(0),
            i_diroff: Cell::new(0),
            i_offset: Cell::new(0),
            i_ino: Cell::new(0),
            i_lock: Rrwlock::new("isoinode"),
            iso_extent: Cell::new(0),
            i_size: Cell::new(0),
            iso_start: Cell::new(0),
            inode: Cell::new(IsoRripInode {
                iso_atime: Timespec::new(0, 0),
                iso_mtime: Timespec::new(0, 0),
                iso_ctime: Timespec::new(0, 0),
                iso_mode: 0,
                iso_uid: 0,
                iso_gid: 0,
                iso_links: 0,
                iso_rdev: 0,
            }),
            i_ci: Cell::new(ClusterInfo {
                ci_lastr: 0,
                ci_lastw: 0,
                ci_cstart: 0,
                ci_lasta: 0,
                ci_clen: 0,
                ci_ralen: 0,
                ci_maxra: 0,
            }),
        }
    }

    /// `ITOV(ip)`: the node's vnode.
    pub fn itov(&self) -> &'static Vnode {
        match self.i_vnode.get() {
            Some(vp) => vp,
            None => panic(format_args!("iso_node {:p}: no vnode", self)),
        }
    }

    /// `ip->i_mnt`, which `cd9660_vget_internal` sets.
    pub fn mnt(&self) -> &'static IsoMnt {
        match self.i_mnt.get() {
            Some(imp) => imp,
            None => panic(format_args!("iso_node {:p}: no mount", self)),
        }
    }

    /// Changes `ip->inode` in place.
    pub fn update_inode(&self, f: impl FnOnce(&mut IsoRripInode)) {
        let mut ino = self.inode.get();
        f(&mut ino);
        self.inode.set(ino);
    }
}

impl Default for IsoNode {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// The node hash chains, through `i_hash` (the C's `i_next`/`i_prev`).
    pub IsoHash: IsoNode, i_hash => ListEntry<IsoNode>
);

/// `IN_ACCESS`: inode access time to be updated.
pub const IN_ACCESS: u32 = 0x0020;

/// The hash chains, with the claim that makes them shareable.
#[derive(Clone, Copy)]
struct Isohashtbl(&'static [ListHead<IsoHash>]);

// SAFETY: the chains are changed under the kernel lock (the C's "XXX locking" comments), as
// in C.
unsafe impl Sync for Isohashtbl {}
// SAFETY: as above: the kernel lock.
unsafe impl Send for Isohashtbl {}

/// `isohashtbl`: structures associated with `iso_node` caching.
static ISOHASHTBL: StaticCell<Isohashtbl> = StaticCell::new(Isohashtbl(&[]));
/// `isohash`: size of hash table - 1.
static ISOHASH: StaticCell<u64> = StaticCell::new(0);
/// `isohashkey`.
static ISOHASHKEY: StaticCell<SiphashKey> = StaticCell::new(SiphashKey { k0: 0, k1: 0 });

/// `VTOI(vp)`: the node of a cd9660 vnode.
pub fn vtoi(vp: &Vnode) -> &'static IsoNode {
    let data = vp.v_data.get();
    if data.is_null() || vp.v_tag.get() != VT_ISOFS {
        panic(format_args!("VTOI: vnode {:p} has no iso_node", vp));
    }
    // SAFETY: a cd9660 vnode's `v_data` (checked above) is the node `cd9660_vget_internal`
    // hung there, which lives until `cd9660_reclaim` frees it and clears `v_data`; the
    // caller holds the vnode (a reference or its lock), so it is not reclaimed meanwhile.
    unsafe { &*data.cast::<IsoNode>() }
}

/// `cd9660_init`: initialize hash links for inodes and dnodes.
pub fn cd9660_init(_vfsp: &'static Vfsconf) -> Result<(), Errno> {
    let elements = crate::conf::param::INITIALVNODES.load(Ordering::Relaxed);
    let Some(tbl) = hashinit::<IsoHash>(elements, M_ISOFSMNT, M_WAITOK) else {
        panic(format_args!("cd9660_init: no memory"));
    };
    let mut key = [0u8; 16];
    arc4random_buf(&mut key);
    let mut k0 = [0u8; 8];
    let mut k1 = [0u8; 8];
    k0.copy_from_slice(&key[..8]);
    k1.copy_from_slice(&key[8..]);
    // SAFETY: called from `vfsinit`, before any cd9660 file system is mounted, so nothing
    // reads the cells meanwhile (the module's deviations).
    unsafe {
        ISOHASHTBL.write(Isohashtbl(tbl));
        ISOHASH.write(tbl.len() as u64 - 1);
        ISOHASHKEY.write(SiphashKey {
            k0: u64::from_ne_bytes(k0),
            k1: u64::from_ne_bytes(k1),
        });
    }
    Ok(())
}

/// `cd9660_isohash(device, inum)` (`INOHASH`): the chain of the pair.
fn cd9660_isohash(device: Dev, inum: Cdino) -> &'static ListHead<IsoHash> {
    // SAFETY: the three cells are written only by `cd9660_init`, before any file system is
    // mounted, and read-only afterwards.
    let (tbl, mask, key) = unsafe { (ISOHASHTBL.get().0, *ISOHASH.get(), ISOHASHKEY.get()) };
    if tbl.is_empty() {
        panic(format_args!("cd9660_isohash: no table"));
    }

    let mut ctx = SiphashCtx::default();
    SipHash24_Init(&mut ctx, key);
    SipHash24_Update(&mut ctx, &device.to_ne_bytes());
    SipHash24_Update(&mut ctx, &inum.to_ne_bytes());
    &tbl[(SipHash24_End(&mut ctx) & mask) as usize]
}

/// `cd9660_ihashget(dev, inum)`: use the device/inum pair to find the incore inode, and
/// return its vnode, referenced and locked. If it is in core, but locked, wait for it.
pub fn cd9660_ihashget(dev: Dev, inum: Cdino) -> Option<&'static Vnode> {
    'loop_: loop {
        // XXX locking lock hash list?
        for ip in cd9660_isohash(dev, inum).iter() {
            if inum == ip.i_number.get() && dev == ip.i_dev.get() {
                let vp = ip.itov();
                let vpid = vp.v_id.get();
                // XXX locking unlock hash list?
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
        // XXX locking unlock hash list?
        return None;
    }
}

/// `cd9660_ihashins(ip)`: insert the inode into the hash table, and return it locked;
/// `EEXIST` (unlocked) when the pair is hashed already.
pub fn cd9660_ihashins(ip: &'static IsoNode) -> Result<(), Errno> {
    // XXX locking lock hash list?
    let ipp = cd9660_isohash(ip.i_dev.get(), ip.i_number.get());

    for iq in ipp.iter() {
        if iq.i_dev.get() == ip.i_dev.get() && iq.i_number.get() == ip.i_number.get() {
            return Err(Errno::EEXIST);
        }
    }

    // SAFETY: the node is on no chain (it was just allocated); it stays in place until
    // `cd9660_ihashrem` unlinks it, which `cd9660_reclaim` does before freeing it.
    unsafe { ipp.insert_head(ip) };
    ip.i_hashed.set(true);
    // XXX locking unlock hash list?

    let _ = VOP_LOCK(ip.itov(), LK_EXCLUSIVE);

    Ok(())
}

/// `cd9660_ihashrem(ip)`: remove the inode from the hash table.
pub fn cd9660_ihashrem(ip: &IsoNode) {
    if !ip.i_hashed.get() {
        return;
    }

    // XXX locking lock hash list?
    // SAFETY: `i_hashed` says the node is linked on its chain (`cd9660_ihashins`).
    unsafe { ListHead::<IsoHash>::remove(ip) };
    ip.i_hashed.set(false);
    // XXX locking unlock hash list?
}

/// `cd9660_inactive` (`vop_inactive`): last reference to an inode; if it is not valid,
/// reclaim it so that it can be reused immediately.
pub fn cd9660_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);

    #[cfg(feature = "diagnostic")]
    if crate::kern::vfs_subr::PRTACTIVE.load(Ordering::Relaxed) != 0 && vp.v_usecount.get() != 0 {
        crate::kern::vfs_subr::vprint(Some("cd9660_inactive: pushing active"), vp);
    }

    ip.i_flag.set(0);
    let _ = VOP_UNLOCK(vp);
    // If we are done with the inode, reclaim it so that it can be reused immediately.
    if ip.inode.get().iso_mode == 0 {
        vrecycle(vp, ap.a_p);
    }

    Ok(())
}

/// `cd9660_reclaim` (`vop_reclaim`): reclaim an inode so that it can be used for other
/// purposes.
pub fn cd9660_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);

    #[cfg(feature = "diagnostic")]
    if crate::kern::vfs_subr::PRTACTIVE.load(Ordering::Relaxed) != 0 && vp.v_usecount.get() != 0 {
        crate::kern::vfs_subr::vprint(Some("cd9660_reclaim: pushing active"), vp);
    }

    // Remove the inode from its hash chain.
    cd9660_ihashrem(ip);
    // Purge old data structures associated with the inode.
    cache_purge(vp);
    if let Some(devvp) = ip.i_devvp.take() {
        vrele(devvp);
    }
    if let Some(data) = NonNull::new(vp.v_data.get()) {
        vp.v_data.set(ptr::null_mut());
        free(data.cast(), M_ISOFSNODE, size_of::<IsoNode>());
    }
    Ok(())
}

/// The extended attribute record at the start of a buffer, when it is version 1 (the only
/// one the C reads).
fn extattr_v1(bp: &Buf) -> Option<&IsoExtendedAttributes> {
    // SAFETY: the caller owns the buffer (busy, from `bread`) and holds no other view of it;
    // the reference does not outlive the caller's use of the buffer.
    let data: &[u8] = unsafe { bp.data() };
    IsoExtendedAttributes::from_bytes(data).filter(|ap| isonum_711(&ap.version) == 1)
}

/// The buffer of a node's extended attribute record, when the mount reads them and the
/// record has one (`cd9660_bufatoff` at the negative offset of the record).
fn extattr_buf(isodir: &IsoDirectoryRecord<'_>, inop: &IsoNode) -> Option<&'static Buf> {
    let imp = inop.mnt();
    if imp.im_flags & ISOFSMNT_EXTATT == 0 {
        return None;
    }
    let off = isonum_711(isodir.ext_attr_length());
    if off == 0 {
        return None;
    }
    cd9660_bufatoff(inop, -(i64::from(off) << imp.im_bshift))
        .ok()
        .map(|(bp, _)| bp)
}

/// `cd9660_defattr`: file attributes of a plain ISO 9660 record (and of its extended
/// attribute record, in `bp` or read here).
pub fn cd9660_defattr(isodir: &IsoDirectoryRecord<'_>, inop: &IsoNode, bp: Option<&'static Buf>) {
    let mut ino = inop.inode.get();
    if isonum_711(isodir.flags()) & 2 != 0 {
        ino.iso_mode = S_IFDIR as u16;
        // If we return 2, fts() will assume there are no subdirectories (just links for
        // the path and .), so instead we return 1.
        ino.iso_links = 1;
    } else {
        ino.iso_mode = S_IFREG as u16;
        ino.iso_links = 1;
    }
    let bp2 = if bp.is_none() {
        extattr_buf(isodir, inop)
    } else {
        None
    };
    let ap = bp.or(bp2).and_then(extattr_v1);
    match ap {
        Some(ap) => {
            let mut mode = ino.iso_mode as u32;
            if ap.perm[1] & 0x10 == 0 {
                mode |= S_IRUSR;
            }
            if ap.perm[1] & 0x40 == 0 {
                mode |= S_IXUSR;
            }
            if ap.perm[0] & 0x01 == 0 {
                mode |= S_IRGRP;
            }
            if ap.perm[0] & 0x04 == 0 {
                mode |= S_IXGRP;
            }
            if ap.perm[0] & 0x10 == 0 {
                mode |= S_IROTH;
            }
            if ap.perm[0] & 0x40 == 0 {
                mode |= S_IXOTH;
            }
            ino.iso_mode = mode as u16;
            ino.iso_uid = Uid::from(isonum_723(&ap.owner)); // what about 0?
            ino.iso_gid = Gid::from(isonum_723(&ap.group)); // what about 0?
        }
        None => {
            ino.iso_mode |= (S_IRUSR | S_IXUSR | S_IRGRP | S_IXGRP | S_IROTH | S_IXOTH) as u16;
            ino.iso_uid = 0;
            ino.iso_gid = 0;
        }
    }
    inop.inode.set(ino);
    if let Some(bp2) = bp2 {
        brelse(bp2);
    }
}

/// `cd9660_deftstamp`: time stamps of a plain ISO 9660 record (and of its extended
/// attribute record, in `bp` or read here).
pub fn cd9660_deftstamp(isodir: &IsoDirectoryRecord<'_>, inop: &IsoNode, bp: Option<&'static Buf>) {
    let mut ino = inop.inode.get();
    let bp2 = if bp.is_none() {
        extattr_buf(isodir, inop)
    } else {
        None
    };
    match bp.or(bp2).and_then(extattr_v1) {
        Some(ap) => {
            if !cd9660_tstamp_conv17(&ap.ftime, &mut ino.iso_atime) {
                cd9660_tstamp_conv17(&ap.ctime, &mut ino.iso_atime);
            }
            if !cd9660_tstamp_conv17(&ap.ctime, &mut ino.iso_ctime) {
                ino.iso_ctime = ino.iso_atime;
            }
            if !cd9660_tstamp_conv17(&ap.mtime, &mut ino.iso_mtime) {
                ino.iso_mtime = ino.iso_ctime;
            }
        }
        None => {
            cd9660_tstamp_conv7(isodir.date(), &mut ino.iso_ctime);
            ino.iso_atime = ino.iso_ctime;
            ino.iso_mtime = ino.iso_ctime;
        }
    }
    inop.inode.set(ino);
    if let Some(bp2) = bp2 {
        brelse(bp2);
    }
}

/// `cd9660_tstamp_conv7`: a 7-byte date (years since 1900, month, day, hour, minute,
/// second, offset from GMT in 15-minute units) to a time; `false` and the epoch before
/// 1970.
pub fn cd9660_tstamp_conv7(pi: &[u8], pu: &mut Timespec) -> bool {
    let y = i32::from(pi[0]) + 1900;
    let m = i32::from(pi[1]);
    let d = i32::from(pi[2]);
    let hour = i32::from(pi[3]);
    let minute = i32::from(pi[4]);
    let second = i32::from(pi[5]);
    let tz = pi[6] as i8;

    if y < 1970 {
        pu.tv_sec = 0;
        pu.tv_nsec = 0;
        return false;
    }

    // ORIGINAL: computes day number relative to Sept. 19th,1989; don't even *THINK* about
    // changing formula. It works!
    //   days = 367*(y-1980)-7*(y+(m+9)/12)/4-3*((y+(m-9)/7)/100+1)/4+275*m/9+d-100;
    // Changed :-) to make it relative to Jan. 1st, 1970 and to disambiguate negative
    // division.
    let days =
        367 * (y - 1960) - 7 * (y + (m + 9) / 12) / 4 - 3 * ((y + (m + 9) / 12 - 1) / 100 + 1) / 4
            + 275 * m / 9
            + d
            - 239;
    let mut crtime = days
        .wrapping_mul(24)
        .wrapping_add(hour)
        .wrapping_mul(60)
        .wrapping_add(minute)
        .wrapping_mul(60)
        .wrapping_add(second);

    // timezone offset is unreliable on some disks
    if (-48..=52).contains(&tz) {
        crtime = crtime.wrapping_sub(i32::from(tz) * 15 * 60);
    }
    pu.tv_sec = i64::from(crtime);
    pu.tv_nsec = 0;
    true
}

/// `cd9660_chars2ui`: the decimal number in the `len` digits at `begin`.
fn cd9660_chars2ui(begin: &[u8], len: usize) -> u32 {
    begin[..len].iter().fold(0u32, |rc, &c| {
        rc.wrapping_mul(10)
            .wrapping_add(u32::from(c).wrapping_sub(u32::from(b'0')))
    })
}

/// `cd9660_tstamp_conv17`: a 17-byte date ("YYYYMMDDHHMMSScc" digits and the offset from
/// GMT) to a time, through the 7-byte form.
pub fn cd9660_tstamp_conv17(pi: &[u8], pu: &mut Timespec) -> bool {
    let buf = [
        // year:"0001"-"9999" -> -1900
        cd9660_chars2ui(pi, 4).wrapping_sub(1900) as u8,
        // month: " 1"-"12" -> 1 - 12
        cd9660_chars2ui(&pi[4..], 2) as u8,
        // day: " 1"-"31" -> 1 - 31
        cd9660_chars2ui(&pi[6..], 2) as u8,
        // hour: " 0"-"23" -> 0 - 23
        cd9660_chars2ui(&pi[8..], 2) as u8,
        // minute:" 0"-"59" -> 0 - 59
        cd9660_chars2ui(&pi[10..], 2) as u8,
        // second:" 0"-"59" -> 0 - 59
        cd9660_chars2ui(&pi[12..], 2) as u8,
        // difference of GMT
        pi[16],
    ];

    cd9660_tstamp_conv7(&buf, pu)
}

/// `isodirino`: the inode number of a record, the byte offset of the data it describes.
pub fn isodirino(isodir: &IsoDirectoryRecord<'_>, imp: &IsoMnt) -> Cdino {
    isonum_733(isodir.extent()).wrapping_add(u32::from(isonum_711(isodir.ext_attr_length())))
        << imp.im_bshift
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the date conversions, the inode numbers of records and the attributes of
    // plain ISO 9660 records.

    use super::*;
    use crate::isofs::cd9660::cd9660_extern::ISO_FTYPE_DEFAULT;
    use crate::isofs::cd9660::iso::tests::{image, rec, test_mnt, test_node};
    use crate::sys::stat::S_IFMT;

    /// `cd9660_tstamp_conv7` of a 7-byte date.
    fn conv7(d: [u8; 7]) -> (bool, Timespec) {
        let mut t = Timespec::new(-1, -1);
        let ok = cd9660_tstamp_conv7(&d, &mut t);
        (ok, t)
    }

    /// `cd9660_tstamp_conv17` of a 17-byte date.
    fn conv17(d: &[u8; 17]) -> (bool, Timespec) {
        let mut t = Timespec::new(-1, -1);
        let ok = cd9660_tstamp_conv17(d, &mut t);
        (ok, t)
    }

    #[test]
    fn conv7_counts_seconds_since_the_epoch() {
        assert_eq!(conv7([70, 1, 1, 0, 0, 0, 0]), (true, Timespec::new(0, 0)));
        assert_eq!(
            conv7([99, 12, 31, 23, 59, 59, 0]),
            (true, Timespec::new(946_684_799, 0))
        );
        assert_eq!(
            conv7([100, 2, 29, 12, 0, 0, 0]),
            (true, Timespec::new(951_825_600, 0))
        );
        // the offset from GMT is in 15-minute units: local 07:36:48 at -3h is 10:36:48 GMT
        assert_eq!(
            conv7([126, 10, 4, 7, 36, 48, (-12i8) as u8]),
            (true, Timespec::new(1_791_110_208, 0))
        );
        // an offset outside -48..=52 is unreliable and ignored
        assert_eq!(
            conv7([126, 10, 4, 7, 36, 48, 100]),
            (true, Timespec::new(1_791_099_408, 0))
        );
        // before 1970: the epoch, and false
        assert_eq!(
            conv7([69, 12, 31, 0, 0, 0, 0]),
            (false, Timespec::new(0, 0))
        );
    }

    #[test]
    fn conv17_reads_the_digits() {
        assert_eq!(
            conv17(b"2026100407364800\xf4"),
            (true, Timespec::new(1_791_110_208, 0))
        );
        assert_eq!(conv17(b"1970010100000000\x00"), (true, Timespec::new(0, 0)));
        assert_eq!(
            conv17(b"1969123123595900\x00"),
            (false, Timespec::new(0, 0))
        );
    }

    #[test]
    fn isodirino_is_the_byte_offset_of_the_data() {
        let imp = test_mnt(ISO_FTYPE_DEFAULT);
        assert_eq!(isodirino(&rec(&image::SUB_REC), imp), 21 << 11);
        assert_eq!(isodirino(&rec(&image::ROOT_DOT), imp), 20 << 11);
        // the extended attribute record comes first
        let mut r = image::FILE_REC;
        r[1] = 2;
        assert_eq!(isodirino(&rec(&r), imp), (23 + 2) << 11);
    }

    #[test]
    fn plain_records_get_default_attributes_and_times() {
        let imp = test_mnt(ISO_FTYPE_DEFAULT);
        let (ip, _vp) = test_node(imp);

        cd9660_defattr(&rec(&image::SUB_REC), ip, None);
        let ino = ip.inode.get();
        assert_eq!(u32::from(ino.iso_mode) & S_IFMT, S_IFDIR);
        assert_eq!(u32::from(ino.iso_mode) & 0o7777, 0o555);
        assert_eq!((ino.iso_links, ino.iso_uid, ino.iso_gid), (1, 0, 0));

        cd9660_defattr(&rec(&image::FILE_REC), ip, None);
        cd9660_deftstamp(&rec(&image::FILE_REC), ip, None);
        let ino = ip.inode.get();
        assert_eq!(u32::from(ino.iso_mode), S_IFREG | 0o555);
        assert_eq!(ino.iso_ctime, Timespec::new(1_791_110_208, 0));
        assert_eq!(ino.iso_atime, ino.iso_ctime);
        assert_eq!(ino.iso_mtime, ino.iso_ctime);
    }

    #[test]
    fn a_new_node_is_unhashed_and_zeroed() {
        let ip = IsoNode::new();
        assert!(!ip.i_hashed.get());
        assert_eq!(ip.inode.get(), IsoRripInode::default());
        ip.update_inode(|i| i.iso_mode = 0o40555);
        assert_eq!(ip.inode.get().iso_mode, 0o40555);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/isofs/cd9660/cd9660_node.h");
        assert_eq!(
            crate::reftest::int(&defs, "IN_ACCESS"),
            Some(i64::from(IN_ACCESS))
        );
    }
}
/* </TESTS> */
