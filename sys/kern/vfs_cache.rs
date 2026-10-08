/*	$OpenBSD: vfs_cache.c,v 1.58 2022/08/14 01:58:28 jsg Exp $	*/
/*	$NetBSD: vfs_cache.c,v 1.13 1996/02/04 02:18:09 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
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
 *	@(#)vfs_cache.c	8.3 (Berkeley) 8/22/94
 */
/* </LICENSES> */

/* <CODE> */
//! The name cache: recent `namei` lookups, kept per directory in a red-black tree of the
//! names it contains (`v_nc_tree`), with positive entries on one LRU list and negative ones
//! ("this name does not exist") on another, and a reverse map from a directory vnode to the
//! entries naming it (`v_cache_dst`) for `getcwd`.
//!
//! For simplicity (and economy of storage), names longer than a maximum length of
//! `NAMECACHE_MAXLEN` are not cached; they occur infrequently in any case, and are almost
//! never of interest.
//!
//! Upon reaching the last segment of a path, if the reference is for `DELETE`, or `NOCACHE`
//! is set (rewrite), and the name is located in the cache, it will be dropped.
//!
//! Upstream: sys/kern/vfs_cache.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `cache_lookup` returns `Ok(Some(vp))` for a hit, `Err(ENOENT)` for a negative hit,
//!   `Ok(None)` for the C's `-1` (a miss: ask the file system) and `Err(e)` for a failure to
//!   relock; `cache_revlookup` returns `Ok(Some(dvp))`, `Ok(None)` for `-1` and `Err(ERANGE)`.
//! - `cache_revlookup` copies the name into `buf` just before the index `*bpp`, which it moves
//!   backwards, where the C moves a `char *` inside the buffer.
//! - The counters are `AtomicI64`s (`numcache`, `numneg`), `nextvnodeid` an `AtomicU32` and
//!   `doingcache` an `AtomicI32`; `nchstats` is `sys/namei.rs`'s atomic structure.
//! - `nch_pool` cannot sleep yet (`subr_pool.rs`): when it is empty, `cache_enter` skips the
//!   entry (a cache may always forget), where the C would sleep in `pool_get(PR_WAITOK)`.

use core::cmp::Ordering as CmpOrdering;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, Ordering};

use crate::kassert;
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::vfs_subr::{vdrop, vget, vhold, vput, vref};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::VOP_UNLOCK;
use crate::machine::intr::IPL_NONE;
use crate::sys::errno::Errno;
use crate::sys::lock::LK_EXCLUSIVE;
use crate::sys::mount::Mount;
use crate::sys::namei::{
    CREATE, Componentname, ISDOTDOT, ISLASTCN, LOCKPARENT, MAKEENTRY, NAMECACHE_MAXLEN, Namecache,
    NamecacheRbCache, NcLru, NcNeg, Nchstats, PDIRUNLOCK,
};
use crate::sys::pool::{PR_WAITOK, PR_ZERO, Pool};
use crate::sys::queue::TailqHead;
use crate::sys::tree::RbtHead;
use crate::sys::vnode::{VDIR, Vnode};

// TODO: namecache access should really be locked.

/// `numcache`: total number of cache entries allocated.
pub static NUMCACHE: AtomicI64 = AtomicI64::new(0);
/// `numneg`: number of negative cache entries.
pub static NUMNEG: AtomicI64 = AtomicI64::new(0);

/// A global LRU chain of name cache entries.
pub struct NcList<A: crate::sys::queue::TailqAdapter>(pub TailqHead<A>);

// SAFETY: the chains are changed under the kernel lock, as in C.
unsafe impl<A: crate::sys::queue::TailqAdapter> Sync for NcList<A> {}

/// `nclruhead`: Regular Entry LRU chain.
pub static NCLRUHEAD: NcList<NcLru> = NcList(TailqHead::new());
/// `nclruneghead`: Negative Entry LRU chain.
pub static NCLRUNEGHEAD: NcList<NcNeg> = NcList(TailqHead::new());
/// `nchstats`: cache effectiveness statistics.
pub static NCHSTATS: Nchstats = Nchstats::new();

/// `doingcache`: 1 => enable the cache.
pub static DOINGCACHE: AtomicI32 = AtomicI32::new(1);

/// `nch_pool`.
pub static NCH_POOL: Pool = Pool::new();

/// `nextvnodeid`: the capability number `cache_purge` hands out.
pub static NEXTVNODEID: AtomicU32 = AtomicU32::new(0);

/// `namecache_compare(n1, n2)`: by length, then by the bytes of the name.
pub fn namecache_compare(n1: &Namecache, n2: &Namecache) -> CmpOrdering {
    let (name1, len1) = n1.name();
    let (name2, len2) = n2.name();
    if len1 == len2 {
        name1[..len1].cmp(&name2[..len2])
    } else {
        len1.cmp(&len2)
    }
}

/// `cache_tree_init(tree)`: an empty per-directory tree.
pub fn cache_tree_init(tree: &RbtHead<NamecacheRbCache>) {
    tree.init();
}

/// Whether the entry is one `cache_enter` put in the reverse map: a directory entry other
/// than `.` and `..` (the C's length-and-dot test).
fn in_reverse_map(ncp: &Namecache, vp: &Vnode) -> bool {
    let (name, nlen) = ncp.name();
    !ncp.nc_dvp.get().is_some_and(|dvp| ptr::eq(dvp, vp))
        && vp.v_type.get() == VDIR
        && (nlen > 2 || (nlen > 1 && name[1] != b'.') || (nlen > 0 && name[0] != b'.'))
}

/// Blow away a namecache entry.
pub fn cache_zap(ncp: &'static Namecache) {
    let mut dvp = None;

    // SAFETY: an entry with a vnode is on the regular chain, one without on the negative
    // chain (`cache_enter`); entries never move.
    unsafe {
        if ncp.nc_vp.get().is_some() {
            NCLRUHEAD.0.remove(ncp);
            NUMCACHE.fetch_sub(1, Ordering::Relaxed);
        } else {
            NCLRUNEGHEAD.0.remove(ncp);
            NUMNEG.fetch_sub(1, Ordering::Relaxed);
        }
    }
    if let Some(ndvp) = ncp.nc_dvp.get() {
        // SAFETY: an entry with a parent is in the parent's tree (`cache_enter`).
        unsafe { ndvp.v_nc_tree.remove(ncp) };
        if ndvp.v_nc_tree.is_empty() {
            dvp = Some(ndvp);
        }
    }
    if let Some(vp) = ncp.nc_vp.get()
        && ncp.nc_vpid.get() == u64::from(vp.v_id.get())
        && in_reverse_map(ncp, vp)
    {
        // SAFETY: such an entry is on its vnode's reverse map while the capability number
        // has not changed (`cache_purge` empties the map before changing it).
        unsafe { vp.v_cache_dst.remove(ncp) };
    }
    pool_put(&NCH_POOL, NonNull::from(ncp).cast());
    if let Some(dvp) = dvp {
        vdrop(dvp);
    }
}

/// Look for a name in the cache. `dvp` points to the directory to search. The componentname
/// `cnp` holds the information on the entry being sought, such as its length and its name.
/// A hit returns the vnode, locked; a negative hit (the name does not exist) `ENOENT`; a miss
/// `Ok(None)` (the C's `-1`).
pub fn cache_lookup(
    dvp: &'static Vnode,
    cnp: &mut Componentname,
) -> Result<Option<&'static Vnode>, Errno> {
    if DOINGCACHE.load(Ordering::Relaxed) == 0 {
        cnp.cn_flags &= !MAKEENTRY;
        return Ok(None);
    }
    if cnp.cn_namelen > NAMECACHE_MAXLEN as i64 {
        NCHSTATS.ncs_long.fetch_add(1, Ordering::Relaxed);
        cnp.cn_flags &= !MAKEENTRY;
        return Ok(None);
    }

    // lookup in directory vnode's redblack tree
    let n = Namecache::new();
    let name = cnp.name();
    n.nc_nlen.set(name.len() as u8);
    let mut key = [0u8; NAMECACHE_MAXLEN];
    key[..name.len()].copy_from_slice(name);
    n.nc_name.set(key);
    let Some(ncp) = dvp.v_nc_tree.find(&n) else {
        NCHSTATS.ncs_miss.fetch_add(1, Ordering::Relaxed);
        return Ok(None);
    };

    if cnp.cn_flags & MAKEENTRY == 0 {
        NCHSTATS.ncs_badhits.fetch_add(1, Ordering::Relaxed);
        // Last component and we are renaming or deleting, the cache entry is invalid, or
        // otherwise don't want cache entry to exist.
        cache_zap(ncp);
        return Ok(None);
    }
    let Some(vp) = ncp.nc_vp.get() else {
        if cnp.cn_nameiop != CREATE || cnp.cn_flags & ISLASTCN == 0 {
            NCHSTATS.ncs_neghits.fetch_add(1, Ordering::Relaxed);
            // Move this slot to end of the negative LRU chain,
            if TailqHead::<NcNeg>::next(ncp).is_some() {
                // SAFETY: a negative entry is on the negative chain.
                unsafe {
                    NCLRUNEGHEAD.0.remove(ncp);
                    NCLRUNEGHEAD.0.insert_tail(ncp);
                }
            }
            return Err(Errno::ENOENT);
        }
        NCHSTATS.ncs_badhits.fetch_add(1, Ordering::Relaxed);
        cache_zap(ncp);
        return Ok(None);
    };
    if ncp.nc_vpid.get() != u64::from(vp.v_id.get()) {
        NCHSTATS.ncs_falsehits.fetch_add(1, Ordering::Relaxed);
        cache_zap(ncp);
        return Ok(None);
    }

    // Move this slot to end of the regular LRU chain.
    if TailqHead::<NcLru>::next(ncp).is_some() {
        // SAFETY: a positive entry is on the regular chain.
        unsafe {
            NCLRUHEAD.0.remove(ncp);
            NCLRUHEAD.0.insert_tail(ncp);
        }
    }

    let vpid = vp.v_id.get();
    let mut error = if ptr::eq(vp, dvp) {
        // lookup on "."
        vref(dvp);
        Ok(())
    } else if cnp.cn_flags & ISDOTDOT != 0 {
        let _ = VOP_UNLOCK(dvp);
        cnp.cn_flags |= PDIRUNLOCK;
        let error = vget(vp, LK_EXCLUSIVE);
        // If the above vget() succeeded and both LOCKPARENT and ISLASTCN is set, lock the
        // directory vnode as well.
        if error.is_ok() && (!cnp.cn_flags & (LOCKPARENT | ISLASTCN)) == 0 {
            if let Err(e) = vn_lock(dvp, LK_EXCLUSIVE) {
                vput(vp);
                return Err(e);
            }
            cnp.cn_flags &= !PDIRUNLOCK;
        }
        error
    } else {
        let error = vget(vp, LK_EXCLUSIVE);
        // If the above vget() failed or either of LOCKPARENT or ISLASTCN is set, unlock the
        // directory vnode.
        if error.is_err() || (!cnp.cn_flags & (LOCKPARENT | ISLASTCN)) != 0 {
            let _ = VOP_UNLOCK(dvp);
            cnp.cn_flags |= PDIRUNLOCK;
        }
        error
    };

    // Check that the lock succeeded, and that the capability number did not change while we
    // were waiting for the lock.
    if error.is_err() || vpid != vp.v_id.get() {
        if error.is_ok() {
            vput(vp);
            NCHSTATS.ncs_falsehits.fetch_add(1, Ordering::Relaxed);
        } else {
            NCHSTATS.ncs_badhits.fetch_add(1, Ordering::Relaxed);
        }
        // The parent needs to be locked when we return to VOP_LOOKUP(). The `.' case here
        // should be extremely rare (if it can happen at all), so we don't bother optimizing
        // out the unlock/relock.
        if ptr::eq(vp, dvp) || error.is_err() || (!cnp.cn_flags & (LOCKPARENT | ISLASTCN)) != 0 {
            error = vn_lock(dvp, LK_EXCLUSIVE);
            error?;
            cnp.cn_flags &= !PDIRUNLOCK;
        }
        return Ok(None);
    }

    NCHSTATS.ncs_goodhits.fetch_add(1, Ordering::Relaxed);
    Ok(Some(vp))
}

/// Scan cache looking for name of directory entry pointing at vp. Returns the directory
/// (`dvpp`), or `Ok(None)` on a cache miss (the C's `-1`).
///
/// If `buf` is given, also place the name in the buffer immediately before index `*bpp`, and
/// move `*bpp` backwards to point at the start of it. (Yes, this is a little baroque, but it's
/// done this way to cater to the whims of getcwd).
///
/// TODO: should we return *dvpp locked?
pub fn cache_revlookup(
    vp: &'static Vnode,
    bpp: &mut usize,
    buf: Option<&mut [u8]>,
) -> Result<Option<&'static Vnode>, Errno> {
    if DOINGCACHE.load(Ordering::Relaxed) == 0 {
        return Ok(None);
    }
    let found = vp.v_cache_dst.iter().find_map(|ncp| {
        let dvp = ncp.nc_dvp.get()?;
        (!ptr::eq(dvp, vp) && ncp.nc_dvpid.get() == u64::from(dvp.v_id.get())).then_some((ncp, dvp))
    });
    let Some((ncp, dvp)) = found else {
        NCHSTATS.ncs_revmiss.fetch_add(1, Ordering::Relaxed);
        return Ok(None);
    };

    let (name, nlen) = ncp.name();
    #[cfg(feature = "diagnostic")]
    {
        if nlen == 1 && name[0] == b'.' {
            panic(format_args!("cache_revlookup: found entry for ."));
        }
        if nlen == 2 && name[0] == b'.' && name[1] == b'.' {
            panic(format_args!("cache_revlookup: found entry for .."));
        }
    }
    NCHSTATS.ncs_revhits.fetch_add(1, Ordering::Relaxed);

    if let Some(buf) = buf {
        // bp <= bufp: no room for the name and the slash before it.
        if *bpp <= nlen {
            return Err(Errno::ERANGE);
        }
        let bp = *bpp - nlen;
        buf[bp..bp + nlen].copy_from_slice(&name[..nlen]);
        *bpp = bp;
    }

    // XXX: Should we vget() here to have more consistent semantics with cache_lookup()?
    Ok(Some(dvp))
}

/// Add an entry to the cache.
pub fn cache_enter(dvp: &'static Vnode, vp: Option<&'static Vnode>, cnp: &Componentname) {
    let initialvnodes = i64::from(crate::conf::param::INITIALVNODES.load(Ordering::Relaxed));

    if DOINGCACHE.load(Ordering::Relaxed) == 0 || cnp.cn_namelen > NAMECACHE_MAXLEN as i64 {
        return;
    }

    // allocate, or recycle (free and allocate) an ncp.
    if NUMCACHE.load(Ordering::Relaxed) >= initialvnodes {
        if let Some(ncp) = NCLRUHEAD.0.first() {
            cache_zap(ncp);
        } else if let Some(ncp) = NCLRUNEGHEAD.0.first() {
            cache_zap(ncp);
        } else {
            panic(format_args!("wtf? leak?"));
        }
    }
    let Some(mem) = pool_get(&NCH_POOL, PR_WAITOK | PR_ZERO) else {
        // PR_WAITOK cannot sleep yet (see the module's deviations).
        return;
    };
    let ncp = mem.cast::<Namecache>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Namecache>()` bytes, written
    // once before anything else sees it.
    unsafe { ncp.as_ptr().write(Namecache::new()) };
    // SAFETY: as above; the item stays allocated until `cache_zap` (or the race below).
    let ncp: &'static Namecache = unsafe { ncp.as_ref() };

    // grab the vnode we just found
    ncp.nc_vp.set(vp);
    if let Some(vp) = vp {
        ncp.nc_vpid.set(u64::from(vp.v_id.get()));
    }

    // fill in cache info
    ncp.nc_dvp.set(Some(dvp));
    ncp.nc_dvpid.set(u64::from(dvp.v_id.get()));
    let name = cnp.name();
    ncp.nc_nlen.set(name.len() as u8);
    let mut nc_name = [0u8; NAMECACHE_MAXLEN];
    nc_name[..name.len()].copy_from_slice(name);
    ncp.nc_name.set(nc_name);
    if dvp.v_nc_tree.is_empty() {
        vhold(dvp);
    }
    // SAFETY: a fresh entry in no tree; it never moves.
    if unsafe { dvp.v_nc_tree.insert(ncp) }.is_some() {
        // someone has raced us and added a different entry for the same vnode (different
        // ncp) - we don't need this entry, so free it and we are done. We know now
        // dvp->v_nc_tree is not empty, no need to vdrop here.
        pool_put(&NCH_POOL, NonNull::from(ncp).cast());
        return;
    }
    if let Some(vp) = vp {
        // SAFETY: a fresh entry on no chain and in no reverse map; it never moves.
        unsafe {
            NCLRUHEAD.0.insert_tail(ncp);
            NUMCACHE.fetch_add(1, Ordering::Relaxed);
            // don't put . or .. in the reverse map
            if in_reverse_map(ncp, vp) {
                vp.v_cache_dst.insert_tail(ncp);
            }
        }
    } else {
        // SAFETY: as above.
        unsafe { NCLRUNEGHEAD.0.insert_tail(ncp) };
        NUMNEG.fetch_add(1, Ordering::Relaxed);
    }
    if NUMNEG.load(Ordering::Relaxed) > initialvnodes
        && let Some(ncp) = NCLRUNEGHEAD.0.first()
    {
        cache_zap(ncp);
    }
}

/// Name cache initialization, from `vfsinit()` when we are booting.
pub fn nchinit() {
    NCLRUHEAD.0.init();
    NCLRUNEGHEAD.0.init();
    pool_init(
        &NCH_POOL,
        size_of::<Namecache>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "nchpl",
        None,
    );
}

/// Cache flush, a particular vnode; called when a vnode is renamed to hide entries that would
/// now be invalid.
pub fn cache_purge(vp: &'static Vnode) {
    // We should never have destinations cached for a non-VDIR vnode.
    kassert!(vp.v_type.get() == VDIR || vp.v_cache_dst.is_empty());

    while let Some(ncp) = vp.v_cache_dst.first() {
        cache_zap(ncp);
    }
    while let Some(ncp) = vp.v_nc_tree.root() {
        cache_zap(ncp);
    }

    // XXX this blows goats
    let mut id = NEXTVNODEID.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    if id == 0 {
        id = NEXTVNODEID.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    }
    vp.v_id.set(id);
}

/// Cache flush, a whole filesystem; called when filesys is umounted to remove entries that
/// would now be invalid.
pub fn cache_purgevfs(mp: &'static Mount) {
    let ours = |ncp: &Namecache| {
        ncp.nc_dvp
            .get()
            .is_some_and(|dvp| dvp.v_mount.get().is_some_and(|m| ptr::eq(m, mp)))
    };

    // whack the regular entries
    for ncp in NCLRUHEAD.0.iter() {
        if !ours(ncp) {
            continue;
        }
        // free the resources we had (the iterator has already read the next one)
        cache_zap(ncp);
    }
    // whack the negative entries
    for ncp in NCLRUNEGHEAD.0.iter() {
        if !ours(ncp) {
            continue;
        }
        // free the resources we had
        cache_zap(ncp);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the name cache over `testfs` (`vfs_subr.rs`): `cache_enter` and
    // `cache_lookup` hits (the vnode comes back referenced and locked, the parent as the flags
    // say), negative entries, the names it ignores (too long, `MAKEENTRY` clear), the
    // capability check after `cache_purge`, the reverse map of `cache_revlookup` and
    // `cache_purgevfs`; and that `namei` fills and uses it.

    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::vfs_lookup::{namei, ndinit};
    use crate::kern::vfs_subr::tests::testfs::{
        self, A, B, C, LONG, any_locked, node_of, vget_node,
    };
    use crate::kern::vfs_subr::vrele;
    use crate::kern::vfs_vops::VOP_ISLOCKED;
    use crate::sys::namei::{FOLLOW, LOOKUP, NiDirp};

    /// A componentname for `name` in a lookup with `flags`.
    fn cn(name: &'static [u8], op: u64, flags: u64) -> Componentname {
        let mut cnp = Componentname::new();
        cnp.cn_nameiop = op;
        cnp.cn_flags = flags;
        cnp.cn_nameptr = name.as_ptr();
        cnp.cn_namelen = name.len() as i64;
        cnp
    }

    #[test]
    fn compare_orders_by_length_then_bytes() {
        let n = |name: &[u8]| {
            let nc = Namecache::new();
            let mut buf = [0u8; NAMECACHE_MAXLEN];
            buf[..name.len()].copy_from_slice(name);
            nc.nc_name.set(buf);
            nc.nc_nlen.set(name.len() as u8);
            nc
        };
        assert_eq!(namecache_compare(&n(b"b"), &n(b"aa")), CmpOrdering::Less);
        assert_eq!(
            namecache_compare(&n(b"ab"), &n(b"aa")),
            CmpOrdering::Greater
        );
        assert_eq!(namecache_compare(&n(b"ab"), &n(b"ab")), CmpOrdering::Equal);
    }

    #[test]
    fn enter_then_hit() {
        let (_g, _p, mp) = testfs::setup_root();
        let dvp = vget_node(mp, A).unwrap();
        let vp = vget_node(mp, C).unwrap();
        let _ = VOP_UNLOCK(vp);
        let uses = vp.v_usecount.get();

        cache_enter(dvp, Some(vp), &cn(b"c", LOOKUP, MAKEENTRY));
        assert_eq!(NUMCACHE.load(Ordering::SeqCst), 1);
        // A directory entry other than . and .. goes into the reverse map.
        assert!(vp.v_cache_dst.first().is_some());

        // A hit on the last component without LOCKPARENT unlocks the parent.
        let mut c = cn(b"c", LOOKUP, MAKEENTRY | ISLASTCN);
        let hits = NCHSTATS.ncs_goodhits.load(Ordering::SeqCst);
        let found = cache_lookup(dvp, &mut c).unwrap().unwrap();
        assert!(ptr::eq(found, vp));
        assert_eq!(vp.v_usecount.get(), uses + 1);
        assert_eq!(VOP_ISLOCKED(vp), 1);
        assert_eq!(VOP_ISLOCKED(dvp), 0);
        assert!(c.cn_flags & PDIRUNLOCK != 0);
        assert_eq!(NCHSTATS.ncs_goodhits.load(Ordering::SeqCst), hits + 1);
        crate::kern::vfs_subr::vput(vp);

        // With LOCKPARENT on the last component the parent stays locked.
        crate::kern::vfs_vnops::vn_lock(dvp, crate::sys::lock::LK_EXCLUSIVE).unwrap();
        let mut c = cn(b"c", LOOKUP, MAKEENTRY | ISLASTCN | LOCKPARENT);
        let found = cache_lookup(dvp, &mut c).unwrap().unwrap();
        assert_eq!(VOP_ISLOCKED(dvp), 1);
        assert!(c.cn_flags & PDIRUNLOCK == 0);
        crate::kern::vfs_subr::vput(found);
        crate::kern::vfs_subr::vput(dvp);
        assert!(!any_locked());
    }

    #[test]
    fn negative_entries_and_misses() {
        let (_g, _p, mp) = testfs::setup_root();
        let dvp = vget_node(mp, A).unwrap();

        let mut c = cn(b"nope", LOOKUP, MAKEENTRY);
        assert!(matches!(cache_lookup(dvp, &mut c), Ok(None))); // a miss
        cache_enter(dvp, None, &c);
        assert_eq!(NUMNEG.load(Ordering::SeqCst), 1);
        let neg = NCHSTATS.ncs_neghits.load(Ordering::SeqCst);
        assert!(matches!(cache_lookup(dvp, &mut c), Err(Errno::ENOENT)));
        assert_eq!(NCHSTATS.ncs_neghits.load(Ordering::SeqCst), neg + 1);

        // CREATE of the last component drops the negative entry instead.
        let mut c = cn(b"nope", CREATE, MAKEENTRY | ISLASTCN);
        assert!(matches!(cache_lookup(dvp, &mut c), Ok(None)));
        assert_eq!(NUMNEG.load(Ordering::SeqCst), 0);

        // Names longer than NAMECACHE_MAXLEN are never cached.
        let long = cn(testfs::NODES[LONG].name, LOOKUP, MAKEENTRY);
        cache_enter(dvp, None, &long);
        assert_eq!(NUMNEG.load(Ordering::SeqCst), 0);
        let mut long = long;
        let n = NCHSTATS.ncs_long.load(Ordering::SeqCst);
        assert!(matches!(cache_lookup(dvp, &mut long), Ok(None)));
        assert_eq!(NCHSTATS.ncs_long.load(Ordering::SeqCst), n + 1);
        assert!(long.cn_flags & MAKEENTRY == 0);
        crate::kern::vfs_subr::vput(dvp);
    }

    #[test]
    fn purge_invalidates_entries_naming_the_vnode() {
        let (_g, _p, mp) = testfs::setup_root();
        let dvp = vget_node(mp, A).unwrap();
        let vp = vget_node(mp, B).unwrap();
        let _ = VOP_UNLOCK(vp);
        cache_enter(dvp, Some(vp), &cn(b"b", LOOKUP, MAKEENTRY));
        assert!(!dvp.v_nc_tree.is_empty());
        assert_eq!(dvp.v_holdcnt.get(), 1, "a directory with entries is held");

        let id = vp.v_id.get();
        cache_purge(vp);
        assert_ne!(vp.v_id.get(), id);
        // The entry is still in the parent's tree but its capability is stale: a false hit.
        let mut c = cn(b"b", LOOKUP, MAKEENTRY | ISLASTCN);
        let falsehits = NCHSTATS.ncs_falsehits.load(Ordering::SeqCst);
        assert!(matches!(cache_lookup(dvp, &mut c), Ok(None)));
        assert_eq!(NCHSTATS.ncs_falsehits.load(Ordering::SeqCst), falsehits + 1);
        assert!(dvp.v_nc_tree.is_empty());
        assert_eq!(
            dvp.v_holdcnt.get(),
            0,
            "the last entry gone, the hold is dropped"
        );

        // Purging the directory empties its own tree.
        cache_enter(dvp, Some(vp), &cn(b"b", LOOKUP, MAKEENTRY));
        cache_purge(dvp);
        assert!(dvp.v_nc_tree.is_empty());
        assert_eq!(NUMCACHE.load(Ordering::SeqCst), 0);
        vrele(vp);
        crate::kern::vfs_subr::vput(dvp);
    }

    #[test]
    fn revlookup_finds_the_parent_and_the_name() {
        let (_g, _p, mp) = testfs::setup_root();
        let dvp = vget_node(mp, A).unwrap();
        let vp = vget_node(mp, C).unwrap();
        let mut buf = [0u8; 8];
        let mut bp = buf.len();
        assert!(matches!(
            cache_revlookup(vp, &mut bp, Some(&mut buf)),
            Ok(None)
        ));
        cache_enter(dvp, Some(vp), &cn(b"c", LOOKUP, MAKEENTRY));
        let found = cache_revlookup(vp, &mut bp, Some(&mut buf)).unwrap();
        assert!(found.is_some_and(|d| ptr::eq(d, dvp)));
        assert_eq!((bp, &buf[bp..]), (7, &b"c"[..]));
        // No room for the name: ERANGE.
        let mut small = [0u8; 1];
        let mut bp = 1;
        assert!(matches!(
            cache_revlookup(vp, &mut bp, Some(&mut small)),
            Err(Errno::ERANGE)
        ));
        crate::kern::vfs_subr::vput(vp);
        crate::kern::vfs_subr::vput(dvp);
    }

    #[test]
    fn namei_fills_the_cache_and_hits_it() {
        let (_g, p, mp) = testfs::setup_root();
        let lookup = || {
            let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(b"/a/c"), p);
            namei(&mut nd).unwrap();
            let vp = nd.ni_vp.unwrap();
            assert_eq!(node_of(vp), C);
            vrele(vp);
        };
        lookup();
        let hits = NCHSTATS.ncs_goodhits.load(Ordering::SeqCst);
        lookup();
        // "a" and "c" both come from the cache the second time.
        assert_eq!(NCHSTATS.ncs_goodhits.load(Ordering::SeqCst), hits + 2);
        assert!(!any_locked());

        // Unmounting purges the file system's entries.
        assert!(NUMCACHE.load(Ordering::SeqCst) >= 2);
        cache_purgevfs(mp);
        assert_eq!(NUMCACHE.load(Ordering::SeqCst), 0);
        assert_eq!(NUMNEG.load(Ordering::SeqCst), 0);
    }
}
/* </TESTS> */
