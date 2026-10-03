//! Host tests for the name cache over `testfs` (`vfs_subr/tests.rs`): `cache_enter` and
//! `cache_lookup` hits (the vnode comes back referenced and locked, the parent as the flags
//! say), negative entries, the names it ignores (too long, `MAKEENTRY` clear), the
//! capability check after `cache_purge`, the reverse map of `cache_revlookup` and
//! `cache_purgevfs`; and that `namei` fills and uses it.

use std::{assert, assert_eq};

use super::*;
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::tests::testfs::{self, A, B, C, LONG, any_locked, node_of, vget_node};
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
