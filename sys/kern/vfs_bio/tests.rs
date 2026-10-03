//! Host tests for the buffer cache: `getblk`/`brelse`/`incore` bookkeeping, `bread` through a
//! fake strategy (hits, misses, errors), delayed writes and `vflushbuf`, the 2Q queues and
//! `vinvalbuf`, over `blkfs`, a vnode whose strategy reads and writes an in-memory disk.

use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::kern::subr_xxx::nullop;
use crate::kern::vfs_default::vop_generic_bwrite;
use crate::kern::vfs_subr::{getnewvnode, vflushbuf, vinvalbuf};
use crate::machine::cpu::Cpu;
use crate::sys::mount::MNT_WAIT;
use crate::sys::param::DEV_BSIZE;
use crate::sys::ucred::NOCRED;
use crate::sys::vnode::{
    VBIOONSYNCLIST, VT_NON, VopFsyncArgs, VopInactiveArgs, VopStrategyArgs, Vops,
};

/// The fake disk: `DISK_BLOCKS` sectors of `DEV_BSIZE` bytes.
const DISK_BLOCKS: usize = 256;

/// The disk's bytes, the strategy's read and write counts, and a block that fails.
struct Disk {
    data: Vec<u8>,
    reads: usize,
    writes: usize,
    bad: Daddr,
}

static DISK: std::sync::Mutex<Option<Disk>> = std::sync::Mutex::new(None);

fn disk<R>(f: impl FnOnce(&mut Disk) -> R) -> R {
    let mut d = DISK.lock().unwrap_or_else(|e| e.into_inner());
    f(d.as_mut().expect("the disk"))
}

/// `blkfs`'s strategy: a synchronous transfer between the buffer and the disk at
/// `b_blkno`, then `biodone`.
fn blkfs_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    let bp = ap.a_bp;
    let off = bp.b_blkno.get() as usize * DEV_BSIZE;
    let len = bp.b_bcount.get() as usize;
    disk(|d| {
        if bp.b_blkno.get() == d.bad || off + len > d.data.len() {
            bp.b_error.set(Some(Errno::EIO));
            bp.set(B_ERROR);
            return;
        }
        // SAFETY: the buffer is busy for this transfer and mapped.
        let data = unsafe { bp.data() };
        if bp.isset(B_READ) {
            data.copy_from_slice(&d.data[off..off + len]);
            d.reads += 1;
        } else {
            d.data[off..off + len].copy_from_slice(data);
            d.writes += 1;
        }
        bp.b_resid.set(0);
    });
    let s = splbio();
    biodone(bp);
    splx(s);
    Ok(())
}

/// `blkfs`'s fsync: `vflushbuf`, waiting for `MNT_WAIT`, as a disk file system does.
fn blkfs_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    vflushbuf(ap.a_vp, ap.a_waitfor == MNT_WAIT);
    Ok(())
}

fn blkfs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    crate::kern::vfs_vops::VOP_UNLOCK(ap.a_vp)
}

/// `vops` of `blkfs`: a strategy, the generic `bwrite`, no locking.
static BLKFS_VOPS: Vops = Vops {
    vop_lock: Some(|_| nullop()),
    vop_unlock: Some(|_| nullop()),
    vop_islocked: Some(|_| 0),
    vop_inactive: Some(blkfs_inactive),
    vop_reclaim: Some(|_| nullop()),
    vop_strategy: Some(blkfs_strategy),
    vop_bwrite: Some(vop_generic_bwrite),
    vop_fsync: Some(blkfs_fsync),
    ..Vops::EMPTY
};

/// Memory, the vnode table, a fresh buffer cache, a disk whose sector `i` holds the byte
/// `i`, and a `blkfs` vnode.
fn setup() -> (MutexGuard<'static, ()>, &'static Vnode) {
    let (g, p) = crate::kern::vfs_subr::tests::setup();
    Machine::set_curproc(Machine::curcpu(), p);

    BUFHEAD.0.init();
    for c in [
        &BCSTATS.numbufs,
        &BCSTATS.numbufpages,
        &BCSTATS.numdirtypages,
        &BCSTATS.numcleanpages,
        &BCSTATS.pendingwrites,
        &BCSTATS.pendingreads,
        &BCSTATS.numwrites,
        &BCSTATS.numreads,
        &BCSTATS.cachehits,
        &BCSTATS.busymapped,
        &BCSTATS.delwribufs,
    ] {
        c.store(0, Ordering::Relaxed);
    }
    CLEANCACHE.hotbufpages.set(0);
    CLEANCACHE.warmbufpages.set(0);
    CLEANCACHE.cachepages.set(0);
    BUFKVM.store(0, Ordering::Relaxed);
    crate::conf::param::bufpages.store(0, Ordering::Relaxed);
    bufinit();

    let mut data = vec![0u8; DISK_BLOCKS * DEV_BSIZE];
    for (i, sector) in data.chunks_mut(DEV_BSIZE).enumerate() {
        sector.fill(i as u8);
    }
    *DISK.lock().unwrap_or_else(|e| e.into_inner()) = Some(Disk {
        data,
        reads: 0,
        writes: 0,
        bad: -1,
    });

    let vp = getnewvnode(VT_NON, None, &BLKFS_VOPS).expect("a vnode");
    vp.v_type.set(VREG);
    (g, vp)
}

fn teardown() {
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// Whether `bp` is on `vp`'s list `which` (clean or dirty).
fn on_list(list: &crate::sys::vnode::Buflists, bp: &Buf) -> bool {
    list.iter().any(|b| ptr::eq(b, bp))
}

/// The number of buffers on a cache queue.
fn qlen(q: &Bufqueue) -> usize {
    q.iter().count()
}

#[test]
fn getblk_brelse_and_incore_keep_the_books() {
    let (_g, vp) = setup();
    let size = PAGE_SIZE as i32;

    assert!(incore(vp, 5).is_none());
    let bp = getblk(vp, 5, size, 0, INFSLP).expect("a buffer");
    assert!(bp.isset(B_BUSY) && bp.isset(B_BC) && !bp.isset(B_CACHE));
    assert_eq!((bp.b_lblkno.get(), bp.b_blkno.get()), (5, 5));
    assert!(bp.b_vp.get().is_some_and(|b| ptr::eq(b, vp)));
    assert!(on_list(&vp.v_cleanblkhd, bp));
    assert_eq!(vp.v_holdcnt.get(), 1);
    assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 1);
    assert_eq!(BCSTATS.numbufpages.load(Ordering::Relaxed), 1);
    assert_eq!(BCSTATS.busymapped.load(Ordering::Relaxed), 1);
    assert!(incore(vp, 5).is_some_and(|b| ptr::eq(b, bp)));
    // The mapping is usable.
    // SAFETY: the buffer is busy for this test and mapped.
    unsafe { bp.data() }.fill(0xa5);

    brelse(bp);
    assert!(!bp.isset(B_BUSY));
    assert_eq!(qlen(&CLEANCACHE.hotqueue), 1);
    assert_eq!(BCSTATS.numcleanpages.load(Ordering::Relaxed), 1);
    assert_eq!(BCSTATS.busymapped.load(Ordering::Relaxed), 0);

    // A second getblk finds it in the cache, with its data.
    let again = getblk(vp, 5, size, 0, INFSLP).expect("the buffer");
    assert!(ptr::eq(again, bp));
    assert!(again.isset(B_CACHE));
    assert_eq!(BCSTATS.cachehits.load(Ordering::Relaxed), 1);
    assert_eq!(qlen(&CLEANCACHE.hotqueue), 0);
    // SAFETY: as above.
    assert!(unsafe { again.data() }.iter().all(|&b| b == 0xa5));
    brelse(again);

    // An invalidated buffer is freed by brelse, and incore no longer finds the block.
    let bp = getblk(vp, 5, size, 0, INFSLP).expect("the buffer");
    bp.set(B_INVAL);
    brelse(bp);
    assert!(incore(vp, 5).is_none());
    assert!(vp.v_cleanblkhd.is_empty());
    assert_eq!(vp.v_holdcnt.get(), 0);
    assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 0);
    assert_eq!(BCSTATS.numbufpages.load(Ordering::Relaxed), 0);
    teardown();
}

#[test]
fn bread_reads_once_and_reports_errors() {
    let (_g, vp) = setup();
    let size = PAGE_SIZE as i32;

    let (bp, error) = bread(vp, 8, size);
    assert_eq!(error, Ok(()));
    assert!(bp.isset(B_DONE) && !bp.isset(B_CACHE));
    // SAFETY: `bread` returned it busy and mapped.
    let data = unsafe { bp.data() };
    assert_eq!(data[0], 8);
    assert_eq!(data[DEV_BSIZE], 9);
    assert_eq!(disk(|d| d.reads), 1);
    assert_eq!(BCSTATS.numreads.load(Ordering::Relaxed), 1);
    assert_eq!(BCSTATS.pendingreads.load(Ordering::Relaxed), 0);
    brelse(bp);

    // The second read is a cache hit: no I/O.
    let (bp, error) = bread(vp, 8, size);
    assert_eq!(error, Ok(()));
    assert!(bp.isset(B_CACHE));
    assert_eq!(disk(|d| d.reads), 1);
    brelse(bp);

    // A failing read returns the buffer and the error; brelse throws it away.
    disk(|d| d.bad = 40);
    let (bp, error) = bread(vp, 40, size);
    assert_eq!(error, Err(Errno::EIO));
    brelse(bp);
    assert!(incore(vp, 40).is_none());

    // breadn starts the read-ahead too.
    let (bp, error) = breadn(vp, 16, size, &[24], &[size]);
    assert_eq!(error, Ok(()));
    assert!(incore(vp, 24).is_some());
    assert_eq!(disk(|d| d.reads), 3);
    brelse(bp);

    vinvalbuf(vp, 0, NOCRED, None, 0, INFSLP).expect("invalidated");
    assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 0);
    teardown();
}

#[test]
fn delayed_writes_go_dirty_and_vflushbuf_writes_them() {
    let (_g, vp) = setup();
    let size = PAGE_SIZE as i32;

    let bp = getblk(vp, 32, size, 0, INFSLP).expect("a buffer");
    // SAFETY: busy and mapped.
    unsafe { bp.data() }.fill(0x5a);
    bdwrite(bp);
    assert!(bp.isset(B_DELWRI) && !bp.isset(B_BUSY));
    assert!(on_list(&vp.v_dirtyblkhd, bp));
    assert!(vp.v_bioflag.get() & VBIOONSYNCLIST != 0);
    assert_eq!(BCSTATS.delwribufs.load(Ordering::Relaxed), 1);
    assert_eq!(BCSTATS.numdirtypages.load(Ordering::Relaxed), 1);
    assert!(bufcache_getdirtybuf().is_some_and(|b| ptr::eq(b, bp)));
    assert_eq!(disk(|d| d.writes), 0);

    vflushbuf(vp, true);
    assert_eq!(disk(|d| d.writes), 1);
    assert!(disk(|d| d.data[32 * DEV_BSIZE..32 * DEV_BSIZE + PAGE_SIZE]
        .iter()
        .all(|&b| b == 0x5a)));
    assert!(vp.v_dirtyblkhd.is_empty());
    assert!(on_list(&vp.v_cleanblkhd, bp));
    assert!(vp.v_bioflag.get() & VBIOONSYNCLIST == 0);
    assert_eq!(vp.v_numoutput.get(), 0);
    assert_eq!(BCSTATS.delwribufs.load(Ordering::Relaxed), 0);
    assert_eq!(BCSTATS.pendingwrites.load(Ordering::Relaxed), 0);

    // A synchronous bwrite writes and releases.
    let bp = getblk(vp, 48, size, 0, INFSLP).expect("a buffer");
    // SAFETY: busy and mapped.
    unsafe { bp.data() }.fill(0x77);
    assert_eq!(bwrite(bp), Ok(()));
    assert!(!bp.isset(B_BUSY));
    assert_eq!(disk(|d| d.data[48 * DEV_BSIZE]), 0x77);

    // vinvalbuf with V_SAVE writes a dirty buffer before throwing it away.
    let bp = getblk(vp, 64, size, 0, INFSLP).expect("a buffer");
    // SAFETY: busy and mapped.
    unsafe { bp.data() }.fill(0x33);
    bdwrite(bp);
    let p = crate::machine::cpu::curproc();
    // blkfs's fsync writes the dirty buffer, then the loop throws the clean one away.
    vinvalbuf(vp, crate::sys::vnode::V_SAVE, NOCRED, p, 0, INFSLP).expect("flushed");
    assert_eq!(disk(|d| d.data[64 * DEV_BSIZE]), 0x33);
    assert!(vp.v_dirtyblkhd.is_empty() && vp.v_cleanblkhd.is_empty());
    assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 0);
    teardown();
}

#[test]
fn released_buffers_move_through_the_2q_queues() {
    let (_g, vp) = setup();
    let size = PAGE_SIZE as i32;

    // 100 one-page buffers released in order: the hot queue holds at most 96 pages (the
    // minimum before chilling), the oldest four go cold.
    for blk in 0..100 {
        let bp = getblk(vp, blk, size, 0, INFSLP).expect("a buffer");
        brelse(bp);
    }
    assert_eq!(qlen(&CLEANCACHE.hotqueue), 96);
    assert_eq!(qlen(&CLEANCACHE.coldqueue), 4);
    assert_eq!(CLEANCACHE.hotbufpages.get(), 96);
    assert_eq!(CLEANCACHE.cachepages.get(), 100);
    let cold: Vec<Daddr> = CLEANCACHE
        .coldqueue
        .iter()
        .map(|b| b.b_lblkno.get())
        .collect();
    assert_eq!(cold, [0, 1, 2, 3]);
    assert!(CLEANCACHE.coldqueue.iter().all(|b| b.isset(B_COLD)));

    // A cold buffer used again becomes warm.
    let bp = getblk(vp, 2, size, 0, INFSLP).expect("the buffer");
    assert!(bp.isset(B_CACHE));
    brelse(bp);
    assert!(bp.isset(B_WARM) && !bp.isset(B_COLD));
    assert_eq!(qlen(&CLEANCACHE.warmqueue), 1);
    assert_eq!(CLEANCACHE.warmbufpages.get(), 1);
    assert_eq!(qlen(&CLEANCACHE.coldqueue), 3);

    // Recovering pages takes the cold ones first, then warm, then hot.
    let s = splbio();
    assert_eq!(bufcache_recover_pages(false, 4), 4);
    splx(s);
    assert!(CLEANCACHE.coldqueue.is_empty());
    assert!(CLEANCACHE.warmqueue.is_empty());
    assert!(incore(vp, 0).is_none() && incore(vp, 2).is_none());
    assert!(incore(vp, 4).is_some());
    assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 96);

    // Every buffer is on the free lists exactly once.
    let mut n = 0;
    for b in CLEANCACHE.hotqueue.iter() {
        assert!(b.b_onfreelist.get());
        n += 1;
    }
    assert_eq!(n, 96);

    vinvalbuf(vp, 0, NOCRED, None, 0, INFSLP).expect("invalidated");
    assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 0);
    assert_eq!(CLEANCACHE.cachepages.get(), 0);
    teardown();
}

#[test]
fn geteblk_gives_an_anonymous_invalid_buffer() {
    let (_g, _vp) = setup();
    let bp = geteblk(2 * PAGE_SIZE);
    assert!(bp.b_vp.get().is_none());
    assert!(bp.isset(B_INVAL) && bp.isset(B_BUSY));
    assert_eq!(bp.b_bufsize.get(), 2 * PAGE_SIZE as i64);
    assert_eq!(bp.b_dev.get(), NODEV);
    brelse(bp);
    assert_eq!(BCSTATS.numbufs.load(Ordering::Relaxed), 0);
    teardown();
}
