//! Host tests for ext2fs: a small ext2 file system built in memory (1 KiB blocks, one block
//! group: super block, group descriptor, bitmaps, inode table, a root directory, a short
//! file, a file with an indirect block and, on the read-only image, a file mapped by a
//! two-level extent tree), a block device vnode whose strategy reads and writes it, and
//! tests that mount it with `ext2fs_mountfs`, get vnodes with `ext2fs_vget`, read through
//! `ext2fs_read`, map blocks, take `statfs`, write, allocate and truncate on a read-write
//! mount, and unmount it clean; plus the super block checks.

use core::mem::offset_of;
use core::sync::atomic::Ordering;
use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::kern::subr_xxx::nullop;
use crate::kern::vfs_bio::{BCSTATS, BUFHEAD, BUFKVM, CLEANCACHE, biodone, bufinit};
use crate::kern::vfs_default::vop_generic_bwrite;
use crate::kern::vfs_init::vfs_byname;
use crate::kern::vfs_subr::{vflushbuf, vfs_mount_alloc};
use crate::kern::vfs_vops::{VOP_BMAP, VOP_READ, VOP_WRITE};
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::sys::buf::{B_ERROR, B_READ};
use crate::sys::mount::VFS_ROOT;
use crate::sys::types::makedev;
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::{VDIR, VREG, VROOT, VopFsyncArgs, VopInactiveArgs, VopStrategyArgs, Vops};
use crate::ufs::ext2fs::ext2fs::{
    E2FS_REV1, EXT2F_INCOMPAT_FTYPE, EXT2F_INCOMPAT_RECOVER, EXT2F_ROCOMPAT_BTREE_DIR, Ext2fs,
};
use crate::ufs::ext2fs::ext2fs_alloc::ext2fs_inode_alloc;
use crate::ufs::ext2fs::ext2fs_dinode::EXT4_EXTENTS;
use crate::ufs::ext2fs::ext2fs_extents::EXT4_EXT_MAGIC;
use crate::ufs::ext2fs::ext2fs_inode::{ext2fs_size, ext2fs_truncate};
use crate::ufs::ext2fs::ext2fs_subr::ext2fs_bufatoff;
use crate::ufs::ufs::dinode::{IFREG, ROOTINO};

const B: usize = 1024;
/// Blocks in the file system.
const NBLK: u32 = 64;
/// Inodes per group (and in all).
const IPG: u32 = 32;
/// The inode size.
const ISZ: usize = 128;
/// Where things are (block numbers).
const GDT: usize = 2;
const BBITMAP: usize = 3;
const IBITMAP: usize = 4;
const ITABLE: usize = 5;
const ROOTDIR: usize = 9;
const HELLO: usize = 10;
/// `big`: 12 direct blocks at 11..=22, its indirect block at 23 pointing at 24 and 25.
const BIG0: usize = 11;
const BIGIND: usize = 23;
/// `ext`: its leaf at 26, extents {0, 2 blocks at 27} and {2, 1 block at 30}.
const EXTLEAF: usize = 26;
/// The free blocks: 29 and 31..=63.
const NFREE: u32 = 34;
/// The free inodes: 15..=32.
const NIFREE: u32 = 18;

const HELLO_DATA: &[u8] = b"hello, ext2\n";
const BIG_SIZE: usize = 14 * B - 100;
const EXT_SIZE: usize = 3 * B - 10;

/// `n` bytes of a pattern that differs per `seed`.
fn pattern(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| ((i * 7 + usize::from(seed)) % 251) as u8)
        .collect()
}

/// The image being built, 1 KiB blocks.
struct Img(Vec<u8>);

impl Img {
    fn put(&mut self, blk: usize, off: usize, b: &[u8]) {
        let o = blk * B + off;
        self.0[o..o + b.len()].copy_from_slice(b);
    }

    fn u16(&mut self, blk: usize, off: usize, v: u16) {
        self.put(blk, off, &v.to_le_bytes());
    }

    fn u32(&mut self, blk: usize, off: usize, v: u32) {
        self.put(blk, off, &v.to_le_bytes());
    }

    fn setbit(&mut self, blk: usize, bit: usize) {
        self.0[blk * B + bit / 8] |= 1 << (bit % 8);
    }

    /// The byte offset of inode `ino` (from 1) in the image.
    fn ioff(ino: u32) -> (usize, usize) {
        let i = (ino - 1) as usize;
        (ITABLE + i * ISZ / B, (i * ISZ) % B)
    }

    /// Inode `ino`: mode, links, size, block pointers (`blocks`), `DEV_BSIZE` blocks, flags.
    fn inode(&mut self, ino: u32, mode: u16, nlink: u16, size: u32, blocks: &[u32], flags: u32) {
        let (blk, off) = Self::ioff(ino);
        self.u16(blk, off + offset_of!(Ext2fsDinode, e2di_mode), mode);
        self.u16(blk, off + offset_of!(Ext2fsDinode, e2di_uid_low), 1000);
        self.u16(blk, off + offset_of!(Ext2fsDinode, e2di_uid_high), 1);
        self.u16(blk, off + offset_of!(Ext2fsDinode, e2di_gid_low), 20);
        self.u32(blk, off + offset_of!(Ext2fsDinode, e2di_size), size);
        self.u16(blk, off + offset_of!(Ext2fsDinode, e2di_nlink), nlink);
        self.u32(blk, off + offset_of!(Ext2fsDinode, e2di_flags), flags);
        self.u32(blk, off + offset_of!(Ext2fsDinode, e2di_gen), 7 + ino);
        let mut n = 0;
        for (i, &b) in blocks.iter().enumerate() {
            self.u32(blk, off + offset_of!(Ext2fsDinode, e2di_blocks) + 4 * i, b);
            if b != 0 && flags & EXT4_EXTENTS == 0 {
                n += 2;
            }
        }
        self.u32(blk, off + offset_of!(Ext2fsDinode, e2di_nblock), n);
    }

    /// A directory entry at `off` of block `blk`.
    fn dirent(&mut self, blk: usize, off: usize, ino: u32, reclen: u16, name: &[u8], t: u8) {
        self.u32(blk, off, ino);
        self.u16(blk, off + 4, reclen);
        self.put(blk, off + 6, &[name.len() as u8, t]);
        self.put(blk, off + 8, name);
    }

    /// An extent tree node header.
    fn eh(&mut self, blk: usize, off: usize, ecount: u16, max: u16, depth: u16) {
        self.u16(blk, off, EXT4_EXT_MAGIC);
        self.u16(blk, off + 2, ecount);
        self.u16(blk, off + 4, max);
        self.u16(blk, off + 6, depth);
    }

    /// An extent: `len` blocks from logical `lblk` at disk block `start`.
    fn extent(&mut self, blk: usize, off: usize, lblk: u32, len: u16, start: u32) {
        self.u32(blk, off, lblk);
        self.u16(blk, off + 4, len);
        self.u16(blk, off + 6, 0);
        self.u32(blk, off + 8, start);
    }
}

/// The test file system; with `extents`, it has the `EXTENTS` incompatible feature (and
/// mounts only read-only).
fn image(extents: bool) -> Vec<u8> {
    let mut img = Img(vec![0; NBLK as usize * B]);

    // Super block.
    let mut sb = Ext2fs::new();
    sb.e2fs_icount = IPG;
    sb.e2fs_bcount = NBLK;
    sb.e2fs_rbcount = 3;
    sb.e2fs_fbcount = NFREE;
    sb.e2fs_ficount = NIFREE;
    sb.e2fs_first_dblock = 1;
    sb.e2fs_bpg = 8192;
    sb.e2fs_fpg = 8192;
    sb.e2fs_ipg = IPG;
    sb.e2fs_wtime = 1_700_000_000;
    sb.e2fs_max_mnt_count = 20;
    sb.e2fs_magic = E2FS_MAGIC;
    sb.e2fs_state = E2FS_ISCLEAN;
    sb.e2fs_beh = 1;
    sb.e2fs_rev = E2FS_REV1;
    sb.e2fs_first_ino = EXT2_FIRSTINO;
    sb.e2fs_inode_size = ISZ as u16;
    sb.e2fs_features_incompat =
        EXT2F_INCOMPAT_FTYPE | if extents { EXT2F_INCOMPAT_EXTENTS } else { 0 };
    sb.e2fs_features_rocompat = EXT2F_ROCOMPAT_SPARSE_SUPER;
    e2fs_sbsave(&sb, &mut img.0[1024..2048]);

    // Group descriptor.
    let gd = Ext2Gd {
        ext2bgd_b_bitmap: BBITMAP as u32,
        ext2bgd_i_bitmap: IBITMAP as u32,
        ext2bgd_i_tables: ITABLE as u32,
        ext2bgd_nbfree: NFREE as u16,
        ext2bgd_nifree: NIFREE as u16,
        ext2bgd_ndirs: 1,
        ..Ext2Gd::default()
    };
    img.put(GDT, 0, &gd.to_le_bytes());

    // Block bitmap: bit b is block b + 1; blocks 1..=28 and 30 are used, and the bits past
    // the last block.
    for b in 1..=28 {
        img.setbit(BBITMAP, b - 1);
    }
    img.setbit(BBITMAP, 30 - 1);
    for bit in (NBLK as usize - 1)..8 * B {
        img.setbit(BBITMAP, bit);
    }
    // Inode bitmap: inodes 1..=14, and the bits past the last inode.
    for i in 1..=14 {
        img.setbit(IBITMAP, i - 1);
    }
    for bit in IPG as usize..8 * B {
        img.setbit(IBITMAP, bit);
    }

    // The root directory.
    let mut root = [0u32; 15];
    root[0] = ROOTDIR as u32;
    img.inode(EXT2_ROOTINO, 0o40755, 2, B as u32, &root, 0);
    img.dirent(ROOTDIR, 0, 2, 12, b".", 2);
    img.dirent(ROOTDIR, 12, 2, 12, b"..", 2);
    img.dirent(ROOTDIR, 24, 12, 20, b"hello.txt", 1);
    img.dirent(ROOTDIR, 44, 13, 12, b"big", 1);
    img.dirent(ROOTDIR, 56, 14, (B - 56) as u16, b"ext", 1);

    // hello.txt (inode 12).
    let mut hello = [0u32; 15];
    hello[0] = HELLO as u32;
    img.inode(12, 0o100644, 1, HELLO_DATA.len() as u32, &hello, 0);
    img.put(HELLO, 0, HELLO_DATA);

    // big (inode 13): 14 blocks, two of them behind the indirect block.
    let mut big = [0u32; 15];
    for (i, b) in big.iter_mut().take(12).enumerate() {
        *b = (BIG0 + i) as u32;
    }
    big[12] = BIGIND as u32;
    img.inode(13, 0o100644, 1, BIG_SIZE as u32, &big, 0);
    img.u32(BIGIND, 0, 24);
    img.u32(BIGIND, 4, 25);
    let data = pattern(BIG_SIZE, 13);
    let blocks: Vec<usize> = (BIG0..BIG0 + 12).chain([24, 25]).collect();
    for (i, chunk) in data.chunks(B).enumerate() {
        img.put(blocks[i], 0, chunk);
    }

    // ext (inode 14): an index root in the inode, one leaf with two extents.
    img.inode(14, 0o100644, 1, EXT_SIZE as u32, &[], EXT4_EXTENTS);
    let (iblk, ioff) = Img::ioff(14);
    let broot = ioff + offset_of!(Ext2fsDinode, e2di_blocks);
    img.eh(iblk, broot, 1, 4, 1);
    img.u32(iblk, broot + 12, 0); // ei_blk
    img.u32(iblk, broot + 16, EXTLEAF as u32); // ei_leaf_lo
    img.eh(EXTLEAF, 0, 2, 84, 0);
    img.extent(EXTLEAF, 12, 0, 2, 27);
    img.extent(EXTLEAF, 24, 2, 1, 30);
    let data = pattern(EXT_SIZE, 14);
    for (chunk, blk) in data.chunks(B).zip([27, 28, 30]) {
        img.put(blk, 0, chunk);
    }

    img.0
}

/// The disk the strategy below reads and writes.
pub(crate) static DISK: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

/// A synchronous transfer between the buffer and the image at `b_blkno`, then `biodone`.
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

/// The fake disk's block device vnode operations.
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

/// The fake disk's device number.
const DISKDEV: i32 = makedev(17, 5);

/// Memory, the vfs (whose `vfsinit` runs `ext2fs_init`), a fresh buffer cache, the image as
/// the disk, the thread as `curproc`.
pub(crate) fn setup(image: Vec<u8>) -> (MutexGuard<'static, ()>, &'static Proc) {
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
    *DISK.lock().unwrap_or_else(|e| e.into_inner()) = image;
    (g, p)
}

pub(crate) fn teardown() {
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// `ext2fs_mountfs` of the disk on a fresh mount, and the `statfs` `sys_mount` follows it
/// with.
pub(crate) fn mount(p: &'static Proc, ronly: bool) -> Result<&'static Mount, Errno> {
    let devvp = bdevvp(DISKDEV).unwrap().unwrap();
    devvp.v_op.set(Some(&DISK_VOPS));
    let mp = vfs_mount_alloc(None, vfs_byname(b"ext2fs").unwrap());
    if ronly {
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
    }
    match ext2fs_mountfs(devvp, mp, p) {
        Ok(()) => {
            let mut st = mp.mnt_stat.get();
            ext2fs_statfs(mp, &mut st, p).unwrap();
            mp.mnt_stat.set(st);
            Ok(mp)
        }
        Err(e) => {
            vrele(devvp);
            vfs_mount_free(mp);
            Err(e)
        }
    }
}

/// A `Uio` over `buf` at `offset`.
fn uio<'a>(iov: &'a mut [Iovec; 1], offset: Off, rw: UioRw) -> Uio<'a> {
    let len = iov[0].iov_len;
    Uio {
        uio_iov: iov,
        uio_offset: offset,
        uio_resid: len,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: rw,
        uio_procp: None,
    }
}

/// The whole file through `VOP_READ`, `chunk` bytes at a time.
fn read_all(vp: &'static Vnode, chunk: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut buf = vec![0u8; chunk];
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: chunk,
        }];
        let mut u = uio(&mut iov, out.len() as Off, UioRw::UIO_READ);
        VOP_READ(vp, &mut u, 0, ptr::null()).unwrap();
        let n = chunk - u.uio_resid;
        if n == 0 {
            return out;
        }
        out.extend_from_slice(&buf[..n]);
    }
}

/// `VOP_WRITE` of `data` at `offset`.
fn write_at(p: &Proc, vp: &'static Vnode, offset: Off, data: &[u8]) {
    let mut buf = data.to_vec();
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    }];
    let mut u = uio(&mut iov, offset, UioRw::UIO_WRITE);
    VOP_WRITE(vp, &mut u, 0, p.p_ucred.get()).unwrap();
    assert_eq!(u.uio_resid, 0);
}

/// `VOP_BMAP`: the disk block and run of logical block `bn`.
fn bmap(vp: &'static Vnode, bn: Daddr) -> (Daddr, i32) {
    let mut bnp = 0;
    let mut run = 0;
    VOP_BMAP(vp, bn, None, Some(&mut bnp), Some(&mut run)).unwrap();
    (bnp, run)
}

/// A little-endian `u32` of the disk image.
fn disk_u32(off: usize) -> u32 {
    let d = DISK.lock().unwrap();
    u32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]])
}

/// A little-endian `u16` of the disk image.
fn disk_u16(off: usize) -> u16 {
    let d = DISK.lock().unwrap();
    u16::from_le_bytes([d[off], d[off + 1]])
}

/// Whether bit `bit` of the bitmap in block `blk` is set on the disk.
fn disk_bit(blk: usize, bit: usize) -> bool {
    DISK.lock().unwrap()[blk * B + bit / 8] & (1 << (bit % 8)) != 0
}

#[test]
fn mount_read_map_statfs_and_unmount() {
    let (_g, p) = setup(image(true));
    let mp = mount(p, true).unwrap();
    let ump = vfstoufs(mp);
    let fs = ump.e2fs();
    assert_eq!(ump.um_fstype.get(), UM_EXT2FS);
    assert_eq!(ump.um_nindir.get(), 256);
    assert_eq!(ump.um_bptrtodb.get(), 1);
    assert_eq!(ump.um_seqinc.get(), 1);
    assert_eq!(ump.um_maxsymlinklen.get(), 60);
    assert_eq!(fs.e2fs_ncg.get(), 1);
    assert_eq!(fs.e2fs_ngdb.get(), 1);
    assert_eq!(fs.e2fs_bsize.get(), 1024);
    assert_eq!(fs.e2fs_ipb.get(), 8);
    assert_eq!(fs.e2fs_itpg.get(), 4);
    assert_eq!(fs.gd(0).ext2bgd_i_tables, ITABLE as u32);
    assert_eq!(fs.e2fs_ronly.get(), 1);
    assert_eq!(fs.e2fs_maxfilesize.get(), Off::from(i32::MAX) * 4);
    assert_eq!(mp.mnt_flag.get() & MNT_LOCAL, MNT_LOCAL);

    // statfs: the overhead is the boot block, two bitmaps and four inode table blocks, and
    // one super block and one descriptor block.
    let st = mp.mnt_stat.get();
    assert_eq!(st.f_bsize, 1024);
    assert_eq!(st.f_iosize, 1024);
    assert_eq!(st.f_blocks, u64::from(NBLK) - 9);
    assert_eq!(st.f_bfree, u64::from(NFREE));
    assert_eq!(st.f_bavail, i64::from(NFREE) - 3);
    assert_eq!(st.f_files, u64::from(IPG));
    assert_eq!(st.f_ffree, u64::from(NIFREE));
    assert_eq!(&st.f_fstypename[..7], b"ext2fs\0");
    assert_eq!(st.f_fsid.val[1], 17);

    // The root directory.
    let root = VFS_ROOT(mp).unwrap();
    assert_eq!(root.v_type.get(), VDIR);
    assert_eq!(root.v_flag.get() & VROOT, VROOT);
    let rip = vtoi(root);
    assert_eq!(rip.i_number.get(), ROOTINO);
    assert_eq!(rip.i_e2fs_uid().get(), 1000 | 1 << 16);
    assert_eq!(rip.i_e2fs_gid().get(), 20);
    assert_eq!(rip.i_effnlink.get(), 2);
    let dir = read_all(root, 300);
    assert_eq!(dir.len(), B);
    assert_eq!(&dir[24..28], &12u32.to_le_bytes());
    assert_eq!(&dir[32..41], b"hello.txt");

    // hello.txt
    let hello = VFS_VGET(mp, 12).unwrap();
    assert_eq!(hello.v_type.get(), VREG);
    assert_eq!(read_all(hello, 5), HELLO_DATA);
    assert_eq!(bmap(hello, 0), (2 * HELLO as Daddr, 0));
    assert_eq!(bmap(hello, 1).0, -1);

    // big: direct blocks in one run, then the two behind the indirect block.
    let big = VFS_VGET(mp, 13).unwrap();
    assert_eq!(read_all(big, 1000), pattern(BIG_SIZE, 13));
    assert_eq!(bmap(big, 0), (2 * BIG0 as Daddr, 11));
    assert_eq!(bmap(big, 12), (48, 1));
    assert_eq!(bmap(big, 13), (50, 0));
    assert_eq!(bmap(big, 14).0, -1);

    // ext: through the extent tree (and its cache).
    let ext = VFS_VGET(mp, 14).unwrap();
    assert_eq!(read_all(ext, 700), pattern(EXT_SIZE, 14));
    assert_eq!(bmap(ext, 1).0, 2 * 28);
    assert_eq!(bmap(ext, 2).0, 2 * 30);
    assert_eq!(
        ext2fs_bufatoff(vtoi(ext), 2 * B as Off + 5).map(|(bp, off)| {
            // SAFETY: the buffer is ours (busy) and mapped.
            let b = unsafe { bp.data() }[off];
            brelse(bp);
            b
        }),
        Ok(pattern(EXT_SIZE, 14)[2 * B + 5])
    );

    // File handles.
    let mut fid = Fid::default();
    ext2fs_vptofh(hello, &mut fid).unwrap();
    let u = Ufid::from_fid(&fid);
    assert_eq!((u.ufid_len, u.ufid_ino, u.ufid_gen), (12, 12, 19));
    for ino in [5, IPG + 1] {
        let bad = Ufid { ufid_ino: ino, ..u };
        bad.to_fid(&mut fid);
        assert_eq!(ext2fs_fhtovp(mp, &fid).err(), Some(Errno::ESTALE));
    }

    vput(ext);
    vput(big);
    vput(hello);
    vput(root);
    ext2fs_unmount(mp, 0, p).unwrap();
    vfs_mount_free(mp);
    // A read-only unmount writes nothing.
    assert_eq!(*DISK.lock().unwrap(), image(true));
    teardown();
}

#[test]
fn a_read_write_mount_allocates_truncates_and_unmounts_clean() {
    let (_g, p) = setup(image(false));
    let mp = mount(p, false).unwrap();
    let fs = vfstoufs(mp).e2fs();
    assert_eq!(fs.e2fs_state(), 0);
    assert_eq!(fs.e2fs_fmod.get(), 1);
    assert_eq!(fs.e2fs_maxfilesize.get(), Off::from(i32::MAX));

    let root = VFS_ROOT(mp).unwrap();
    let hello = VFS_VGET(mp, 12).unwrap();
    let ip = vtoi(hello);
    let cred = p.p_ucred.get();

    // Grow hello.txt to three blocks: two new ones, the first 8 free ones of the group.
    let data = pattern(3000, 1);
    write_at(p, hello, 0, &data);
    assert_eq!(ext2fs_size(ip), 3000);
    assert_eq!(fs.e2fs_fbcount(), NFREE - 2);
    assert_eq!(fs.gd(0).ext2bgd_nbfree as u32, NFREE - 2);
    assert_eq!(ip.i_e2fs_nblock(), 6);
    assert_eq!(ip.i_e2fs_block(1), 33);
    assert_eq!(ip.i_e2fs_block(2), 34);
    assert_eq!(read_all(hello, 512), data);

    // Truncate it back into its first block.
    ext2fs_truncate(ip, 12, 0, cred).unwrap();
    assert_eq!(ext2fs_size(ip), 12);
    assert_eq!(fs.e2fs_fbcount(), NFREE);
    assert_eq!(ip.i_e2fs_nblock(), 2);
    assert_eq!(ip.i_e2fs_blocks()[1..], [0; 14]);
    assert_eq!(read_all(hello, 512), data[..12]);

    // A byte at logical block 13: an indirect block and a data block, holes before. With no
    // block to follow, both go to the first wholly free byte of the bitmap: blocks 33..=40,
    // then 41..=48.
    write_at(p, hello, 13 * B as Off, b"Z");
    assert_eq!(fs.e2fs_fbcount(), NFREE - 2);
    assert_eq!(ip.i_e2fs_block(12), 33);
    assert_eq!(bmap(hello, 13).0, 2 * 41);
    let all = read_all(hello, 4096);
    assert_eq!(all.len(), 13 * B + 1);
    assert_eq!(all[..12], data[..12]);
    assert!(all[12..13 * B].iter().all(|&b| b == 0));
    assert_eq!(all[13 * B], b'Z');

    // Truncate to nothing: the data block under the indirect block, the indirect block and
    // the first block go back.
    ext2fs_truncate(ip, 0, 0, cred).unwrap();
    assert_eq!(ext2fs_size(ip), 0);
    assert_eq!(fs.e2fs_fbcount(), NFREE + 1);
    assert_eq!(ip.i_e2fs_nblock(), 0);
    assert_eq!(ip.i_e2fs_blocks(), [0; 15]);

    // A new inode: the first free one, 15; let go with no links, it is freed again.
    let nvp = ext2fs_inode_alloc(vtoi(root), IFREG | 0o644, cred).unwrap();
    let nip = vtoi(nvp);
    assert_eq!(nip.i_number.get(), 15);
    assert_eq!(fs.e2fs_ficount(), NIFREE - 1);
    assert_ne!(nip.i_e2fs_gen(), 0);
    nip.set_i_e2fs_mode((IFREG | 0o644) as u16);
    vput(nvp);
    assert_eq!(fs.e2fs_ficount(), NIFREE);

    vput(hello);
    vput(root);
    ext2fs_unmount(mp, 0, p).unwrap();
    vfs_mount_free(mp);

    // On the disk: clean, the counts and bitmaps back, hello.txt empty, inode 15 free.
    let sb = 1024;
    assert_eq!(disk_u16(sb + offset_of!(Ext2fs, e2fs_state)), E2FS_ISCLEAN);
    assert_eq!(disk_u32(sb + offset_of!(Ext2fs, e2fs_fbcount)), NFREE + 1);
    assert_eq!(disk_u32(sb + offset_of!(Ext2fs, e2fs_ficount)), NIFREE);
    assert_eq!(disk_u16(GDT * B + 12), (NFREE + 1) as u16);
    assert_eq!(disk_u16(GDT * B + 14), NIFREE as u16);
    assert!(!disk_bit(BBITMAP, HELLO - 1));
    assert!(!disk_bit(BBITMAP, 32) && !disk_bit(BBITMAP, 33) && !disk_bit(BBITMAP, 40));
    assert!(!disk_bit(IBITMAP, 14));
    let (blk, off) = Img::ioff(12);
    let at = blk * B + off;
    assert_eq!(disk_u32(at + offset_of!(Ext2fsDinode, e2di_size)), 0);
    assert_eq!(disk_u32(at + offset_of!(Ext2fsDinode, e2di_nblock)), 0);
    assert_eq!(disk_u32(at + offset_of!(Ext2fsDinode, e2di_blocks)), 0);
    assert_eq!(disk_u16(at + offset_of!(Ext2fsDinode, e2di_uid_high)), 1);
    // Inode 15 went to its block as it was let go (its `dtime` is the host clock's, 0 here).
    let (blk, off) = Img::ioff(15);
    let at = blk * B + off;
    assert_eq!(disk_u16(at + offset_of!(Ext2fsDinode, e2di_mode)), 0o100644);
    assert_eq!(disk_u16(at + offset_of!(Ext2fsDinode, e2di_nlink)), 0);
    assert_ne!(disk_u32(at + offset_of!(Ext2fsDinode, e2di_gen)), 0);
    teardown();
}

#[test]
fn ext4_needs_a_read_only_mount_and_bad_super_blocks_are_refused() {
    let (_g, p) = setup(image(true));
    assert_eq!(mount(p, false).err(), Some(Errno::EROFS));
    teardown();

    /// A change to the super block.
    type Edit = dyn Fn(&mut Ext2fs);
    let sb = |f: &Edit| {
        let mut img = image(false);
        let mut s = Ext2fs::new();
        e2fs_sbload(&img[1024..2048], &mut s);
        f(&mut s);
        e2fs_sbsave(&s, &mut img[1024..2048]);
        img
    };
    assert_eq!(e2fs_sbcheck(&sb(&|_| {})[1024..2048], false), Ok(()));
    let cases: [(&Edit, bool, Errno); 7] = [
        (&|s| s.e2fs_magic = 0x1234, true, Errno::EIO),
        (&|s| s.e2fs_log_bsize = 3, true, Errno::EIO),
        (&|s| s.e2fs_bpg = 0, true, Errno::EIO),
        (&|s| s.e2fs_rev = 2, true, Errno::EIO),
        (&|s| s.e2fs_first_ino = 12, true, Errno::EINVAL),
        (&|s| s.e2fs_features_incompat |= 0x8000, true, Errno::EINVAL),
        (
            &|s| s.e2fs_features_incompat |= EXT2F_INCOMPAT_RECOVER,
            false,
            Errno::EROFS,
        ),
    ];
    for (f, ronly, e) in cases {
        assert_eq!(e2fs_sbcheck(&sb(f)[1024..2048], ronly), Err(e));
    }
    // A file system to recover mounts read-only; an unknown read-only feature too.
    let recover = sb(&|s| s.e2fs_features_incompat |= EXT2F_INCOMPAT_RECOVER);
    assert_eq!(e2fs_sbcheck(&recover[1024..2048], true), Ok(()));
    let btree = sb(&|s| s.e2fs_features_rocompat |= EXT2F_ROCOMPAT_BTREE_DIR);
    assert_eq!(e2fs_sbcheck(&btree[1024..2048], true), Ok(()));
    assert_eq!(e2fs_sbcheck(&btree[1024..2048], false), Err(Errno::EROFS));
    // Revision 0 skips the feature checks.
    let rev0 = sb(&|s| {
        s.e2fs_rev = 0;
        s.e2fs_first_ino = 0;
    });
    assert_eq!(e2fs_sbcheck(&rev0[1024..2048], false), Ok(()));
}

#[test]
fn the_largest_file_follows_the_block_size_and_huge_files() {
    let fs: &'static MExt2fs = std::boxed::Box::leak(std::boxed::Box::new(MExt2fs::new()));
    fs.e2fs_bsize.set(1024);
    assert_eq!(
        ext2fs_maxfilesize(fs),
        (12 + 256 + 256 * 256 + 256 * 256 * 256) * 1024
    );
    fs.e2fs_bsize.set(4096);
    // Logically 4 TiB and more; the 32-bit sector count of a disk address stops at 2 TiB.
    assert_eq!(ext2fs_maxfilesize(fs), Off::from(u32::MAX) * 512);
    fs.set_e2fs_features_rocompat(EXT2F_ROCOMPAT_HUGE_FILE);
    assert_eq!(
        ext2fs_maxfilesize(fs),
        (12 + 1024 + 1024 * 1024 + 1024 * 1024 * 1024) * 4096
    );
}
