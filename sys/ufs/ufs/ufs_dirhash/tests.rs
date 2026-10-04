//! Host tests for `ufs_dirhash.rs`: the slot and free-space bookkeeping on a hash built by
//! hand, `ufsdirhash_getprev` on a block, and, on an FFS image mounted on the host (the
//! `newfs` builder of the ffs tests), a directory of thousands of entries that is hashed,
//! looked up, shrunk, renamed into and grown with `ufsdirhash_checkblock` on, then checked
//! against a linear walk; and recycling under a small `ufs_dirhashmaxmem`.

use core::ptr;
use core::sync::atomic::Ordering;
use std::boxed::Box;
use std::collections::BTreeSet;
use std::sync::MutexGuard;
use std::vec::Vec;
use std::{format, vec};

use super::*;
use crate::kern::kern_descrip::sys_close;
use crate::kern::subr_xxx::nullop;
use crate::kern::vfs_bio::{BCSTATS, BUFHEAD, BUFKVM, CLEANCACHE, biodone, bufinit};
use crate::kern::vfs_default::vop_generic_bwrite;
use crate::kern::vfs_init::{rootvnode, set_rootvnode, vfs_byname};
use crate::kern::vfs_subr::{
    MOUNTLIST, bdevvp, vflushbuf, vfs_busy, vfs_mount_alloc, vfs_unbusy, vput, vref, vrele,
};
use crate::kern::vfs_syscalls::{
    dounmount, sys_getdents, sys_link, sys_mkdir, sys_open, sys_rename, sys_stat, sys_sync,
    sys_unlink,
};
use crate::kern::vfs_vops::VOP_UNLOCK;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::{B_ERROR, B_READ};
use crate::sys::fcntl::{O_CREAT, O_RDONLY, O_RDWR};
use crate::sys::mount::{MNT_WAIT, Mount, VB_WAIT, VB_WRITE, VFS_ROOT, VFS_VGET};
use crate::sys::param::DEV_BSIZE;
use crate::sys::proc::Proc;
use crate::sys::stat::Stat;
use crate::sys::systm::{SyCall, SysArgs};
use crate::sys::types::{Register, makedev};
use crate::sys::vnode::{Vnode, VopFsyncArgs, VopInactiveArgs, VopStrategyArgs, Vops};
use crate::ufs::ffs::ffs_extern::FFS_DIRHASH_MEM;
use crate::ufs::ffs::ffs_vfsops::tests::newfs;
use crate::ufs::ffs::ffs_vfsops::{ffs_mountfs, ffs_statfs, ffs_sysctl};
use crate::ufs::ufs::inode::vtoi;

/// A dirhash with `hlen` slots and `dirblks` directory blocks, its arrays leaked test
/// memory, initialised as `ufsdirhash_build` initialises one (every block empty).
fn test_dh(hlen: i32, dirblks: i32) -> &'static Dirhash {
    let narrays = (hlen + DH_NBLKOFF - 1) / DH_NBLKOFF;
    let blocks: Vec<*mut Doff> = (0..narrays)
        .map(|_| Box::leak(Box::new([DIRHASH_EMPTY; DH_NBLKOFF as usize])).as_mut_ptr())
        .collect();
    let nblk = (dirblks * 3 + 1) / 2;
    let blkfree = vec![(DIRBLK / DIRALIGN) as u8; nblk as usize];
    let dh: &'static Dirhash = Box::leak(Box::new(Dirhash::new()));
    dh.dh_hash
        .set(Box::leak(blocks.into_boxed_slice()).as_mut_ptr());
    dh.dh_blkfree
        .set(Box::leak(blkfree.into_boxed_slice()).as_mut_ptr());
    dh.dh_narrays.set(narrays);
    dh.dh_hlen.set(hlen);
    dh.dh_nblk.set(nblk);
    dh.dh_dirblks.set(dirblks);
    for i in 0..DH_NFSTATS {
        dh.set_firstfree(i, -1);
    }
    dh.set_firstfree(DH_NFSTATS, 0);
    dh
}

#[test]
fn hash_stays_in_range_and_depends_on_the_name() {
    let dh = test_dh(3 * DH_NBLKOFF, 4);
    let mut seen = BTreeSet::new();
    for i in 0..200 {
        let name = format!("name{i}");
        let slot = ufsdirhash_hash(dh, name.as_bytes());
        assert!((0..dh.dh_hlen.get()).contains(&slot));
        assert_eq!(slot, ufsdirhash_hash(dh, name.as_bytes()));
        seen.insert(slot);
    }
    // 200 names over 768 slots: a hash worth the name spreads them.
    assert!(seen.len() > 120, "only {} distinct slots", seen.len());
}

#[test]
fn slots_chain_on_collisions_and_deleted_chains_collapse() {
    // Four slots, so that names collide; the chain wraps around the end.
    let dh = test_dh(4, 4);
    let add = |name: &[u8], off: Doff| {
        let mut slot = ufsdirhash_hash(dh, name);
        while dh.dh_entry(slot) >= 0 {
            slot = wrapincr(slot, dh.dh_hlen.get());
        }
        if dh.dh_entry(slot) == DIRHASH_EMPTY {
            dh.dh_hused.set(dh.dh_hused.get() + 1);
        }
        dh.set_dh_entry(slot, off);
        slot
    };
    let a = add(b"a", 0);
    let b = add(b"b", 12);
    let c = add(b"c", 24);
    assert_eq!(dh.dh_hused.get(), 3);
    for (n, off, slot) in [(&b"a"[..], 0, a), (b"b", 12, b), (b"c", 24, c)] {
        assert_eq!(ufsdirhash_findslot(dh, n, off), slot);
    }

    // Deleting an entry inside a chain leaves a DIRHASH_DEL marker that keeps the chain
    // walkable; deleting the chain's last live entries empties the markers too.
    let empty = (0..4).find(|&s| dh.dh_entry(s) == DIRHASH_EMPTY).unwrap();
    let chain: Vec<i32> = (1..4).map(|k| (empty + k) % 4).collect();
    let first = chain[0];
    ufsdirhash_delslot(dh, first);
    assert_eq!(dh.dh_entry(first), DIRHASH_DEL);
    assert_eq!(dh.dh_hused.get(), 3);
    for &s in &chain[1..] {
        let off = dh.dh_entry(s);
        let name: &[u8] = match off {
            0 => b"a",
            12 => b"b",
            _ => b"c",
        };
        assert_eq!(ufsdirhash_findslot(dh, name, off), s);
    }
    ufsdirhash_delslot(dh, chain[2]);
    assert_eq!(dh.dh_entry(chain[2]), DIRHASH_EMPTY);
    assert_eq!(dh.dh_hused.get(), 2);
    ufsdirhash_delslot(dh, chain[1]);
    // The whole chain collapses: chain[1], then the marker left at chain[0].
    assert!((0..4).all(|s| dh.dh_entry(s) == DIRHASH_EMPTY));
    assert_eq!(dh.dh_hused.get(), 0);
}

#[test]
fn adjfree_keeps_the_first_free_lists() {
    let dh = test_dh(DH_NBLKOFF, 4);
    let full = DH_NFSTATS;
    assert_eq!(dh.firstfree(full), 0);

    // Block 0 loses 400 bytes: 28 words free, so it is the first block with 28 free, and
    // block 1 becomes the first entirely free one.
    ufsdirhash_adjfree(dh, 0, -400);
    assert_eq!(dh.blkfree(0), 28);
    assert_eq!(dh.firstfree(28), 0);
    assert_eq!(dh.firstfree(full), 1);
    // Block 2 likewise; block 0 stays the first with 28 free.
    ufsdirhash_adjfree(dh, 2 * DIRBLK + 100, -400);
    assert_eq!(dh.firstfree(28), 0);
    // Block 0 empties again: block 2 is now the first with 28 free, block 0 the first free.
    ufsdirhash_adjfree(dh, 12, 400);
    assert_eq!(dh.blkfree(0), (DIRBLK / DIRALIGN) as u8);
    assert_eq!(dh.firstfree(28), 2);
    assert_eq!(dh.firstfree(full), 0);
    // Small changes within the top bucket (more than DH_NFSTATS words free) move nothing.
    ufsdirhash_adjfree(dh, 3 * DIRBLK, -16);
    assert_eq!(dh.blkfree(3), 124);
    assert_eq!(dh.firstfree(full), 0);
    assert_eq!(blkfree2idx(124), full);
}

#[test]
fn getprev_finds_the_previous_entry_in_the_block() {
    // A block at directory offset 512 with entries at 0 (12 bytes), 12 (16) and 28 (rest).
    let mut blk = vec![0u8; DIRBLKSIZ];
    let mut put = |off: usize, reclen: u16| {
        blk[off..off + 4].copy_from_slice(&5u32.to_ne_bytes());
        blk[off + 4..off + 6].copy_from_slice(&reclen.to_ne_bytes());
        blk[off + 7] = 1;
    };
    put(0, 12);
    put(12, 16);
    put(28, (DIRBLKSIZ - 28) as u16);
    assert_eq!(ufsdirhash_getprev(&blk, 28, DIRBLK + 28), Some(DIRBLK + 12));
    assert_eq!(ufsdirhash_getprev(&blk, 12, DIRBLK + 12), Some(DIRBLK));
    assert_eq!(ufsdirhash_getprev(&blk, 0, DIRBLK), None);
    // An entry offset that does not fall on an entry boundary is corruption.
    assert_eq!(ufsdirhash_getprev(&blk, 20, DIRBLK + 20), None);
}

/// The disk the strategy below reads and writes.
static DISK: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

/// The fake disk's strategy: a synchronous transfer between the buffer and the image.
fn disk_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    let bp = ap.a_bp;
    let off = bp.b_blkno.get() as usize * DEV_BSIZE;
    let len = bp.b_bcount.get() as usize;
    {
        let mut d = DISK.lock().unwrap_or_else(|e| e.into_inner());
        if off + len > d.len() {
            bp.b_error.set(Some(Errno::EIO));
            bp.set(B_ERROR);
        } else {
            // SAFETY: the buffer is busy for this transfer and mapped.
            let data = unsafe { bp.data() };
            if bp.isset(B_READ) {
                data.copy_from_slice(&d[off..off + len]);
            } else {
                d[off..off + len].copy_from_slice(data);
            }
            bp.b_resid.set(0);
        }
    }
    let s = splbio();
    biodone(bp);
    splx(s);
    Ok(())
}

fn disk_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    vflushbuf(ap.a_vp, ap.a_waitfor == MNT_WAIT);
    Ok(())
}

fn disk_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    VOP_UNLOCK(ap.a_vp)
}

/// The fake disk's block device operations.
static DISK_VOPS: Vops = Vops {
    vop_open: Some(|_| nullop()),
    vop_close: Some(|_| nullop()),
    vop_lock: Some(|_| nullop()),
    vop_unlock: Some(|_| nullop()),
    vop_islocked: Some(|_| 0),
    vop_inactive: Some(disk_inactive),
    vop_reclaim: Some(|_| nullop()),
    vop_strategy: Some(disk_strategy),
    vop_bwrite: Some(vop_generic_bwrite),
    vop_fsync: Some(disk_fsync),
    ..Vops::EMPTY
};

/// Memory, the vfs, a fresh buffer cache, the image as the disk and the thread as
/// `curproc` (as the ffs tests set up), with the dirhash checks on and no hash memory in use.
fn setup(image: Vec<u8>) -> (MutexGuard<'static, ()>, &'static Proc) {
    let (g, p) = crate::kern::vfs_subr::tests::setup();
    Machine::set_curproc(Machine::curcpu(), p);
    let limit: &'static crate::sys::resourcevar::Plimit =
        Box::leak(Box::new(crate::sys::resourcevar::Plimit::new()));
    for l in &limit.pl_rlimit {
        l.set(crate::sys::resource::Rlimit {
            rlim_cur: crate::sys::resource::RLIM_INFINITY,
            rlim_max: crate::sys::resource::RLIM_INFINITY,
        });
    }
    limit.pl_rlimit[crate::sys::resource::RLIMIT_NOFILE].set(crate::sys::resource::Rlimit {
        rlim_cur: 128,
        rlim_max: 128,
    });
    p.process().ps_limit.set(limit);
    p.p_limit.set(limit);

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

    *DISK.lock().unwrap_or_else(|e| e.into_inner()) = image;
    // Hashes of earlier tests lived in the memory just reset (vfsinit ran ufsdirhash_init).
    UFS_DIRHASHMEM.store(0, Ordering::Relaxed);
    UFS_DIRHASHCHECK.store(1, Ordering::Relaxed);
    (g, p)
}

fn teardown() {
    UFS_DIRHASHCHECK.store(0, Ordering::Relaxed);
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// Mounts the disk at `/`, as the ffs tests do.
fn mount_root(p: &'static Proc) -> &'static Mount {
    let devvp = bdevvp(makedev(17, 1)).unwrap().unwrap();
    devvp.v_op.set(Some(&DISK_VOPS));
    let mp = vfs_mount_alloc(None, vfs_byname(b"ffs").unwrap());
    mp.update_stat(|sp| sp.f_mntonname[0] = b'/');
    ffs_mountfs(devvp, mp, p).unwrap();
    let mut st = mp.mnt_stat.get();
    ffs_statfs(mp, &mut st, p).unwrap();
    mp.mnt_stat.set(st);
    vfs_unbusy(mp);
    // SAFETY: a new mount on no list.
    unsafe { MOUNTLIST.0.insert_tail(mp) };
    let root = VFS_ROOT(mp).unwrap();
    set_rootvnode(Some(root));
    p.fd().fd_cdir.set(Some(root));
    vref(root);
    let _ = VOP_UNLOCK(root);
    mp
}

/// Undoes `mount_root` and unmounts.
fn unmount_root(p: &'static Proc, mp: &'static Mount) {
    if let Some(cdir) = p.fd().fd_cdir.take() {
        vrele(cdir);
    }
    if let Some(root) = rootvnode() {
        set_rootvnode(None);
        vrele(root);
    }
    vfs_busy(mp, VB_WRITE | VB_WAIT).unwrap();
    dounmount(mp, 0, p).unwrap();
}

/// A system call with up to six arguments; `retval[0]`.
fn sys(f: SyCall, p: &Proc, args: &[usize]) -> Result<isize, Errno> {
    let mut v: SysArgs = [0; 6];
    for (slot, a) in v.iter_mut().zip(args) {
        *slot = *a as Register;
    }
    let mut rv = [0; 2];
    f(p, &v, &mut rv)?;
    Ok(rv[0])
}

/// `s` as a NUL-terminated path.
fn cpath(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

/// `stat(2)`: the inode number of `path`.
fn ino_of(p: &Proc, path: &str) -> Result<u64, Errno> {
    let c = cpath(path);
    let mut sb = [0u8; Stat::SIZE];
    sys(
        sys_stat,
        p,
        &[c.as_ptr() as usize, sb.as_mut_ptr() as usize],
    )?;
    let o = core::mem::offset_of!(Stat, st_ino);
    Ok(u64::from_ne_bytes(sb[o..o + 8].try_into().unwrap()))
}

fn link(p: &Proc, from: &str, to: &str) -> Result<isize, Errno> {
    let (a, b) = (cpath(from), cpath(to));
    sys(sys_link, p, &[a.as_ptr() as usize, b.as_ptr() as usize])
}

fn rename(p: &Proc, from: &str, to: &str) -> Result<isize, Errno> {
    let (a, b) = (cpath(from), cpath(to));
    sys(sys_rename, p, &[a.as_ptr() as usize, b.as_ptr() as usize])
}

fn unlink(p: &Proc, path: &str) -> Result<isize, Errno> {
    let c = cpath(path);
    sys(sys_unlink, p, &[c.as_ptr() as usize])
}

fn mkdir(p: &Proc, path: &str) {
    let c = cpath(path);
    sys(sys_mkdir, p, &[c.as_ptr() as usize, 0o755]).unwrap();
}

fn create(p: &Proc, path: &str) {
    let c = cpath(path);
    let fd = sys(
        sys_open,
        p,
        &[c.as_ptr() as usize, (O_RDWR | O_CREAT) as usize, 0o644],
    )
    .unwrap();
    sys(sys_close, p, &[fd as usize]).unwrap();
}

/// The names in directory `path` (without `.` and `..`) read with getdents(2), which walks
/// the blocks linearly and never consults the hash.
fn list_dir(p: &Proc, path: &str) -> BTreeSet<Vec<u8>> {
    let c = cpath(path);
    let fd = sys(sys_open, p, &[c.as_ptr() as usize, O_RDONLY as usize, 0]).unwrap();
    let mut buf = vec![0u8; 8192];
    let mut names = BTreeSet::new();
    loop {
        let n = sys(
            sys_getdents,
            p,
            &[fd as usize, buf.as_mut_ptr() as usize, buf.len()],
        )
        .unwrap();
        if n == 0 {
            break;
        }
        let mut off = 0;
        while off < n as usize {
            let reclen = u16::from_ne_bytes([buf[off + 16], buf[off + 17]]) as usize;
            let namlen = buf[off + 19] as usize;
            let nm = &buf[off + 24..off + 24 + namlen];
            if nm != b"." && nm != b".." {
                assert!(names.insert(nm.to_vec()), "duplicate entry");
            }
            off += reclen;
        }
    }
    sys(sys_close, p, &[fd as usize]).unwrap();
    names
}

/// Runs `f` on the in-core inode of directory `path`.
fn with_dir_inode<R>(p: &Proc, mp: &'static Mount, path: &str, f: impl FnOnce(&Inode) -> R) -> R {
    let ino = ino_of(p, path).unwrap();
    let vp: &'static Vnode = VFS_VGET(mp, ino).unwrap();
    let r = f(vtoi(vp));
    vput(vp);
    r
}

/// Whether directory `path` has an intact hash.
fn hashed(p: &Proc, mp: &'static Mount, path: &str) -> bool {
    with_dir_inode(p, mp, path, |ip| {
        // SAFETY: the vnode is locked by VFS_VGET for the duration of the closure.
        unsafe { i_dirhash(ip) }.is_some_and(|dh| !dh.dh_hash.get().is_null())
    })
}

/// `sysctl vfs.ffs.dirhash_mem`.
fn sysctl_dirhash_mem(p: &Proc) -> i32 {
    let mut v = 0i32;
    let mut len = size_of::<i32>();
    ffs_sysctl(
        &[FFS_DIRHASH_MEM],
        ptr::from_mut(&mut v) as usize,
        &mut len,
        0,
        0,
        p,
    )
    .unwrap();
    assert_eq!(len, size_of::<i32>());
    v
}

/// Every name in `names` resolves to `ino` in `dir`, and a few names not there do not.
fn check_lookups(p: &Proc, dir: &str, names: &BTreeSet<Vec<u8>>, ino: u64, tag: &str) {
    for n in names {
        let path = format!("{dir}/{}", core::str::from_utf8(n).unwrap());
        assert_eq!(ino_of(p, &path), Ok(ino), "{path}");
    }
    for i in 0..20 {
        let path = format!("{dir}/absent-{tag}-{i}");
        assert_eq!(ino_of(p, &path), Err(Errno::ENOENT), "{path}");
    }
}

/// The name of entry `i` of the big directory: lengths vary from 1 to 30 bytes, so that the
/// entries have different record sizes.
fn big_name(i: usize) -> std::string::String {
    let pad = "x".repeat(i % 23);
    format!("n{i}{pad}")
}

#[test]
fn a_large_directory_is_hashed_and_kept_in_step() {
    const N: usize = 3000;
    let (_g, p) = setup(newfs::Image::new(newfs::FFS2_4M).finish());
    let mp = mount_root(p);
    assert_eq!(UFS_MINDIRHASHSIZE.load(Ordering::Relaxed), 5 * DIRBLK);
    assert_eq!(sysctl_dirhash_mem(p), 0);

    mkdir(p, "/d");
    create(p, "/d/target");
    let target = ino_of(p, "/d/target").unwrap();
    let mut names: BTreeSet<Vec<u8>> = BTreeSet::new();
    names.insert(b"target".to_vec());
    for i in 0..N {
        let n = big_name(i);
        link(p, "/d/target", &format!("/d/{n}")).unwrap();
        names.insert(n.into_bytes());
    }
    // The creations past ufs_mindirhashsize went through the hash.
    assert!(hashed(p, mp, "/d"));
    let mem = sysctl_dirhash_mem(p);
    assert!(mem > 0);
    assert_eq!(mem, UFS_DIRHASHMEM.load(Ordering::Relaxed));
    check_lookups(p, "/d", &names, target, "a");
    // A name that exists cannot be created again.
    assert_eq!(
        link(p, "/d/target", &format!("/d/{}", big_name(7))),
        Err(Errno::EEXIST)
    );

    // Remove every third entry (DELETE lookups through the hash, with the previous
    // entry's offset), so that blocks get holes.
    for i in (0..N).step_by(3) {
        let n = big_name(i);
        unlink(p, &format!("/d/{n}")).unwrap();
        names.remove(n.as_bytes());
    }
    assert!(hashed(p, mp, "/d"));
    check_lookups(p, "/d", &names, target, "b");

    // Rename some entries to longer names inside the directory: removals, and creations
    // that reuse the holes (ufsdirhash_findfree, compaction with ufsdirhash_move).
    for i in (1..N).step_by(7) {
        if i % 3 == 0 {
            continue;
        }
        let (from, to) = (big_name(i), format!("renamed-{i}-{}", "y".repeat(i % 17)));
        rename(p, &format!("/d/{from}"), &format!("/d/{to}")).unwrap();
        names.remove(from.as_bytes());
        names.insert(to.into_bytes());
    }
    // And add more, past the end of the directory (ufsdirhash_newblk).
    for i in N..N + 1000 {
        let n = big_name(i);
        link(p, "/d/target", &format!("/d/{n}")).unwrap();
        names.insert(n.into_bytes());
    }
    assert!(hashed(p, mp, "/d"));
    check_lookups(p, "/d", &names, target, "c");

    // The directory as a linear walk sees it agrees with the names.
    assert_eq!(list_dir(p, "/d"), names);

    // Without the hash (the size threshold raised: ufsdirhash_build frees it), every name
    // still resolves by the linear search, and the memory goes back.
    UFS_MINDIRHASHSIZE.store(i32::MAX, Ordering::Relaxed);
    check_lookups(p, "/d", &names, target, "d");
    assert!(!hashed(p, mp, "/d"));
    assert_eq!(sysctl_dirhash_mem(p), 0);
    UFS_MINDIRHASHSIZE.store(5 * DIRBLK, Ordering::Relaxed);

    // Rebuilt from the blocks on the next lookup: a hash of what is on disk.
    check_lookups(p, "/d", &names, target, "e");
    assert!(hashed(p, mp, "/d"));

    // Empty the directory but for `target`: the truncations (ufsdirhash_dirtrunc) follow.
    let all: Vec<Vec<u8>> = names.iter().filter(|n| *n != b"target").cloned().collect();
    for n in &all {
        unlink(p, &format!("/d/{}", core::str::from_utf8(n).unwrap())).unwrap();
    }
    create(p, "/d/last");
    let mut left = BTreeSet::new();
    left.insert(b"target".to_vec());
    left.insert(b"last".to_vec());
    assert_eq!(list_dir(p, "/d"), left);

    sys(sys_sync, p, &[]).unwrap();
    unmount_root(p, mp);
    assert_eq!(
        UFS_DIRHASHMEM.load(Ordering::Relaxed),
        0,
        "reclaim freed the hash"
    );
    newfs::check(&DISK.lock().unwrap(), true);

    // Mounted again, the directory reads back.
    let mp = mount_root(p);
    assert_eq!(ino_of(p, "/d/last").map(|i| i != target), Ok(true));
    assert_eq!(ino_of(p, "/d/target"), Ok(target));
    unmount_root(p, mp);
    teardown();
}

#[test]
fn recycling_steals_the_least_used_hash() {
    const N: usize = 400;
    let (_g, p) = setup(newfs::Image::new(newfs::FFS2_4M).finish());
    let mp = mount_root(p);

    create(p, "/target");
    let target = ino_of(p, "/target").unwrap();
    let mut names = BTreeSet::new();
    for i in 0..N {
        names.insert(format!("e{i:03}").into_bytes());
    }
    for d in ["/a", "/b", "/c"] {
        mkdir(p, d);
        for n in &names {
            link(
                p,
                "/target",
                &format!("{d}/{}", core::str::from_utf8(n).unwrap()),
            )
            .unwrap();
        }
    }
    assert!(hashed(p, mp, "/a") && hashed(p, mp, "/b") && hashed(p, mp, "/c"));

    // Drop the hashes (they were built while the directories grew), then measure one built
    // at the final size: the three directories are the same size, so each costs that.
    UFS_MINDIRHASHSIZE.store(i32::MAX, Ordering::Relaxed);
    for d in ["/a", "/b", "/c"] {
        assert_eq!(ino_of(p, &format!("{d}/nothere")), Err(Errno::ENOENT));
    }
    assert_eq!(UFS_DIRHASHMEM.load(Ordering::Relaxed), 0);
    UFS_MINDIRHASHSIZE.store(5 * DIRBLK, Ordering::Relaxed);
    assert_eq!(ino_of(p, "/a/nothere2"), Err(Errno::ENOENT));
    let one = UFS_DIRHASHMEM.load(Ordering::Relaxed);
    assert!(one > 0);
    // Allow two and a half hashes.
    let maxmem = 2 * one + one / 2;
    UFS_DIRHASHMAXMEM.store(maxmem, Ordering::Relaxed);

    // Look-ups in /a and /b hash them; /c does not fit.
    check_lookups(p, "/a", &names, target, "a");
    check_lookups(p, "/b", &names, target, "b");
    assert!(hashed(p, mp, "/a") && hashed(p, mp, "/b"));
    assert_eq!(UFS_DIRHASHMEM.load(Ordering::Relaxed), 2 * one);

    // Every look-up in /c that tries to build its hash takes a point off the score of the
    // least recently used hash (/a, at the head of the list); at zero, /a's memory goes to
    // /c. Each look-up is of a new name, so that the name cache does not answer it.
    let mut tries = 0;
    while !hashed(p, mp, "/c") {
        assert_eq!(ino_of(p, &format!("/c/miss{tries}")), Err(Errno::ENOENT));
        assert!(UFS_DIRHASHMEM.load(Ordering::Relaxed) <= maxmem);
        tries += 1;
        assert!(tries < 200, "/c never got a hash");
    }
    assert!(tries > 1, "recycling waits for the score to run out");
    assert!(!hashed(p, mp, "/a"), "/a's hash was recycled");
    assert!(hashed(p, mp, "/b"));
    assert!(UFS_DIRHASHMEM.load(Ordering::Relaxed) <= maxmem);

    // Every directory still answers correctly, hashed or not.
    check_lookups(p, "/c", &names, target, "c");
    check_lookups(p, "/a", &names, target, "a2");
    check_lookups(p, "/b", &names, target, "b2");
    assert!(UFS_DIRHASHMEM.load(Ordering::Relaxed) <= maxmem);

    unmount_root(p, mp);
    assert_eq!(UFS_DIRHASHMEM.load(Ordering::Relaxed), 0);
    UFS_DIRHASHMAXMEM.store(5 * 1024 * 1024, Ordering::Relaxed);
    teardown();
}
