//! Host tests for the ext2fs vnode and directory operations (`ext2fs_vnops.rs` and
//! `ext2fs_lookup.rs`), through the system calls' own paths (`namei`, `vn_open`, `vn_rdwr`,
//! `domkdirat`, `dorenameat`, `dounlinkat`, `dolinkat`, `dosymlinkat`, ...) over a fresh ext2
//! file system built in memory (`mkfs`) on the fake disk of `ext2fs_vfsops.rs`'s tests,
//! mounted read-write as the root. After `dounmount` every test runs `fsck` below on the
//! disk: a small `fsck_ext2fs` that checks what `e2fsck -fn` would (directory record chains,
//! link counts, block counts in 512-byte units, duplicate blocks, the bitmaps, and the free
//! and directory counts of the super block and the group descriptor).

use core::mem::offset_of;
use std::collections::BTreeMap;
use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq, assert_ne, vec};

use super::*;
use crate::kern::vfs_init::set_rootvnode;
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::{MOUNTLIST, vattr_null, vfs_busy, vfs_unbusy};
use crate::kern::vfs_syscalls::{
    dolinkat, domkdirat, domknodat, dorenameat, dosymlinkat, dounlinkat, dounmount,
};
use crate::kern::vfs_vnops::vn_close;
use crate::kern::vfs_vnops::vn_open;
use crate::kern::vfs_vops::{VOP_GETATTR, VOP_PATHCONF, VOP_READDIR, VOP_READLINK, VOP_SETATTR};
use crate::sys::dirent::Dirent;
use crate::sys::fcntl::{AT_FDCWD, AT_REMOVEDIR, FREAD, O_CREAT};
use crate::sys::lock::LK_RETRY;
use crate::sys::mount::VFS_VGET;
use crate::sys::mount::{VB_WAIT, VB_WRITE, VFS_ROOT};
use crate::sys::namei::{FOLLOW, LOCKLEAF, LOOKUP, NOFOLLOW, NiDirp};
use crate::sys::proc::Proc;
use crate::sys::stat::{S_IFCHR, S_IFIFO};
use crate::sys::syslimits::LINK_MAX;
use crate::sys::types::makedev;
use crate::sys::uio::{Iovec, Uio};
use crate::sys::unistd::_PC_LINK_MAX;
use crate::sys::vnode::{VCHR, Vattr};
use crate::ufs::ext2fs::ext2fs::{
    E2FS_ISCLEAN, E2FS_MAGIC, E2FS_REV1, EXT2F_INCOMPAT_FTYPE, EXT2F_ROCOMPAT_SPARSE_SUPER, Ext2Gd,
    Ext2fs, e2fs_sbsave,
};
use crate::ufs::ext2fs::ext2fs_dinode::{EXT2_FIRSTINO, EXT2_ROOTINO, Ext2fsDinode};
use crate::ufs::ext2fs::ext2fs_dir::{
    EXT2_FT_REG_FILE, EXT2_FT_SYMLINK, e2d_type, e2iftodt, ext2fs_dirsiz, inot2ext2dt,
};
use crate::ufs::ext2fs::ext2fs_vfsops::tests::{DISK, mount, setup, teardown};
use crate::ufs::ufs::ufs_ihash::ufs_ihashget;
use crate::ufs::ufs::ufsmount::vfstoufs;

/// The block size.
const B: usize = 1024;
/// Blocks in the file system (one group).
const NBLK: usize = 1024;
/// Inodes in the file system (one group).
const IPG: usize = 64;
/// Where things are (block numbers).
const GDT: usize = 2;
const BBITMAP: usize = 3;
const IBITMAP: usize = 4;
const ITABLE: usize = 5;

/// The inode table's size, in blocks, for inodes of `isz` bytes.
const fn itblocks(isz: usize) -> usize {
    IPG * isz / B
}

/// A fresh ext2 file system (revision 1, `FILETYPE`, sparse super blocks, 1 KiB blocks,
/// `isz`-byte inodes): the super block, one group descriptor, the bitmaps, the inode table and
/// an empty root directory owned by root, as `newfs_ext2fs` leaves it (without lost+found).
fn mkfs(isz: usize) -> Vec<u8> {
    let mut img = vec![0u8; NBLK * B];
    let rootblk = ITABLE + itblocks(isz);
    let used_blocks = rootblk; // blocks 1..=rootblk
    let nfree = (NBLK - 1 - used_blocks) as u32;
    let reserved = (EXT2_FIRSTINO - 1) as usize; // inodes 1..=10
    let nifree = (IPG - reserved) as u32;

    let mut sb = Ext2fs::new();
    sb.e2fs_icount = IPG as u32;
    sb.e2fs_bcount = NBLK as u32;
    sb.e2fs_rbcount = 50;
    sb.e2fs_fbcount = nfree;
    sb.e2fs_ficount = nifree;
    sb.e2fs_first_dblock = 1;
    sb.e2fs_bpg = 8192;
    sb.e2fs_fpg = 8192;
    sb.e2fs_ipg = IPG as u32;
    sb.e2fs_wtime = 1_700_000_000;
    sb.e2fs_max_mnt_count = 20;
    sb.e2fs_magic = E2FS_MAGIC;
    sb.e2fs_state = E2FS_ISCLEAN;
    sb.e2fs_beh = 1;
    sb.e2fs_rev = E2FS_REV1;
    sb.e2fs_first_ino = EXT2_FIRSTINO;
    sb.e2fs_inode_size = isz as u16;
    sb.e2fs_features_incompat = EXT2F_INCOMPAT_FTYPE;
    sb.e2fs_features_rocompat = EXT2F_ROCOMPAT_SPARSE_SUPER;
    e2fs_sbsave(&sb, &mut img[B..2 * B]);

    let gd = Ext2Gd {
        ext2bgd_b_bitmap: BBITMAP as u32,
        ext2bgd_i_bitmap: IBITMAP as u32,
        ext2bgd_i_tables: ITABLE as u32,
        ext2bgd_nbfree: nfree as u16,
        ext2bgd_nifree: nifree as u16,
        ext2bgd_ndirs: 1,
        ..Ext2Gd::default()
    };
    img[GDT * B..GDT * B + gd.to_le_bytes().len()].copy_from_slice(&gd.to_le_bytes());

    let setbit =
        |img: &mut Vec<u8>, blk: usize, bit: usize| img[blk * B + bit / 8] |= 1 << (bit % 8);
    // Block bitmap: bit b is block b + 1.
    for b in 1..=used_blocks {
        setbit(&mut img, BBITMAP, b - 1);
    }
    for bit in (NBLK - 1)..8 * B {
        setbit(&mut img, BBITMAP, bit);
    }
    for i in 0..reserved {
        setbit(&mut img, IBITMAP, i);
    }
    for bit in IPG..8 * B {
        setbit(&mut img, IBITMAP, bit);
    }

    // The root directory.
    let at = ITABLE * B + (EXT2_ROOTINO as usize - 1) * isz;
    let put16 =
        |img: &mut Vec<u8>, o: usize, v: u16| img[o..o + 2].copy_from_slice(&v.to_le_bytes());
    let put32 =
        |img: &mut Vec<u8>, o: usize, v: u32| img[o..o + 4].copy_from_slice(&v.to_le_bytes());
    put16(&mut img, at + offset_of!(Ext2fsDinode, e2di_mode), 0o40755);
    put16(&mut img, at + offset_of!(Ext2fsDinode, e2di_nlink), 2);
    put32(&mut img, at + offset_of!(Ext2fsDinode, e2di_size), B as u32);
    put32(&mut img, at + offset_of!(Ext2fsDinode, e2di_nblock), 2);
    put32(
        &mut img,
        at + offset_of!(Ext2fsDinode, e2di_blocks),
        rootblk as u32,
    );
    if isz > 128 {
        put16(&mut img, at + offset_of!(Ext2fsDinode, e2di_isize), 32);
    }
    let d = rootblk * B;
    put32(&mut img, d, EXT2_ROOTINO);
    put16(&mut img, d + 4, 12);
    img[d + 6] = 1;
    img[d + 7] = 2;
    img[d + 8] = b'.';
    put32(&mut img, d + 12, EXT2_ROOTINO);
    put16(&mut img, d + 16, (B - 12) as u16);
    img[d + 18] = 2;
    img[d + 19] = 2;
    img[d + 20..d + 22].copy_from_slice(b"..");
    img
}

/// What `fsck` found: every allocated inode with its mode, link count, size and data, and
/// every directory's entries.
struct Fs {
    inodes: BTreeMap<u32, Ino>,
    dirs: BTreeMap<u32, Vec<(Vec<u8>, u32, u8)>>,
}

/// An allocated inode, as `fsck` read it.
struct Ino {
    mode: u16,
    nlink: u16,
    uid: u32,
    gid: u32,
    size: u64,
    data: Vec<u8>,
}

/// The disk, as bytes.
fn disk() -> Vec<u8> {
    DISK.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn le16(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}

fn le32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

fn bit(d: &[u8], blk: usize, bit: usize) -> bool {
    d[blk * B + bit / 8] & (1 << (bit % 8)) != 0
}

/// A small `fsck_ext2fs -n` of the unmounted disk: panics on any inconsistency `e2fsck -fn`
/// would report, and returns what it read.
fn fsck() -> Fs {
    let d = disk();
    let sb = B;
    let isz = usize::from(le16(&d, sb + offset_of!(Ext2fs, e2fs_inode_size)));
    assert_eq!(
        le16(&d, sb + offset_of!(Ext2fs, e2fs_state)),
        E2FS_ISCLEAN,
        "clean"
    );
    let first_ino = le32(&d, sb + offset_of!(Ext2fs, e2fs_first_ino));
    let itend = ITABLE + itblocks(isz);

    let mut claimed = vec![false; NBLK];
    for c in claimed.iter_mut().take(itend).skip(1) {
        *c = true;
    }
    let claim = |claimed: &mut Vec<bool>, b: u32, who: u32| {
        let b = b as usize;
        assert!(
            b >= itend && b < NBLK,
            "inode {who}: block {b} out of range"
        );
        assert!(!claimed[b], "inode {who}: block {b} claimed twice");
        claimed[b] = true;
    };

    let mut inodes = BTreeMap::new();
    for ino in 1..=IPG as u32 {
        let at = ITABLE * B + (ino as usize - 1) * isz;
        let f = |o: usize| le32(&d, at + o);
        let mode = le16(&d, at + offset_of!(Ext2fsDinode, e2di_mode));
        let nlink = le16(&d, at + offset_of!(Ext2fsDinode, e2di_nlink));
        let dtime = f(offset_of!(Ext2fsDinode, e2di_dtime));
        if !bit(&d, IBITMAP, ino as usize - 1) {
            // e2fsck takes an inode with links for one in use, and wants a deletion time on
            // a freed one.
            assert_eq!(nlink, 0, "free inode {ino} with links");
            assert!(
                mode == 0 || dtime != 0,
                "deleted inode {ino} has zero dtime"
            );
            continue;
        }
        if ino < first_ino && ino != EXT2_ROOTINO {
            continue;
        }
        assert_ne!(mode, 0, "inode {ino} allocated with mode 0");
        assert_ne!(nlink, 0, "inode {ino} allocated with no links");
        assert_eq!(dtime, 0, "inode {ino} in use, but has dtime set");
        let size = u64::from(f(offset_of!(Ext2fsDinode, e2di_size)));
        let nblock = f(offset_of!(Ext2fsDinode, e2di_nblock));
        let blocks: Vec<u32> = (0..15)
            .map(|i| f(offset_of!(Ext2fsDinode, e2di_blocks) + 4 * i))
            .collect();
        let uid = u32::from(le16(&d, at + offset_of!(Ext2fsDinode, e2di_uid_low)))
            | u32::from(le16(&d, at + offset_of!(Ext2fsDinode, e2di_uid_high))) << 16;
        let gid = u32::from(le16(&d, at + offset_of!(Ext2fsDinode, e2di_gid_low)))
            | u32::from(le16(&d, at + offset_of!(Ext2fsDinode, e2di_gid_high))) << 16;

        let fast = u32::from(mode) & IFMT == IFLNK && size < EXT2_MAXSYMLINKLEN as u64;
        let special = !matches!(u32::from(mode) & IFMT, IFREG | IFDIR | IFLNK);
        let mut data = Vec::new();
        let mut nblk = 0u32;
        if special {
            // A device keeps its number in the first block pointer.
            assert_eq!((size, nblock), (0, 0), "special file {ino} with data");
            assert!(
                blocks[1..].iter().all(|&b| b == 0),
                "special file {ino}: pointers"
            );
            data.extend_from_slice(&blocks[0].to_le_bytes());
        } else if fast {
            assert_eq!(nblock, 0, "fast symlink {ino} has blocks");
            let s = at + offset_of!(Ext2fsDinode, e2di_blocks);
            data.extend_from_slice(&d[s..s + size as usize]);
        } else {
            // The logical blocks in order (0 for a hole), claiming each block.
            let mut lblocks: Vec<u32> = blocks[..12].to_vec();
            let ptrs = |b: u32| -> Vec<u32> {
                (0..B / 4)
                    .map(|i| le32(&d, b as usize * B + 4 * i))
                    .collect()
            };
            for &b in blocks[..12].iter().filter(|&&b| b != 0) {
                claim(&mut claimed, b, ino);
                nblk += 1;
            }
            if blocks[12] != 0 {
                claim(&mut claimed, blocks[12], ino);
                nblk += 1;
                lblocks.extend(ptrs(blocks[12]));
            }
            if blocks[13] != 0 {
                claim(&mut claimed, blocks[13], ino);
                nblk += 1;
                for b in ptrs(blocks[13]).into_iter().filter(|&b| b != 0) {
                    claim(&mut claimed, b, ino);
                    nblk += 1;
                    lblocks.extend(ptrs(b));
                }
            }
            assert_eq!(blocks[14], 0, "inode {ino}: triple indirect block");
            for &b in lblocks[12..].iter().filter(|&&b| b != 0) {
                claim(&mut claimed, b, ino);
                nblk += 1;
            }
            assert_eq!(nblock, 2 * nblk, "inode {ino}: i_blocks in 512-byte units");
            let nlb = (size as usize).div_ceil(B);
            assert!(
                lblocks.iter().skip(nlb).all(|&b| b == 0),
                "inode {ino}: blocks past its size"
            );
            for i in 0..nlb {
                match lblocks.get(i).copied().unwrap_or(0) {
                    0 => data.extend_from_slice(&[0; B]),
                    b => data.extend_from_slice(&d[b as usize * B..(b as usize + 1) * B]),
                }
            }
            data.truncate(size as usize);
        }
        inodes.insert(
            ino,
            Ino {
                mode,
                nlink,
                uid,
                gid,
                size,
                data,
            },
        );
    }

    // Directories: record chains, entries, and the references they make.
    let mut refs: BTreeMap<u32, u16> = BTreeMap::new();
    let mut dirs = BTreeMap::new();
    for (&ino, ip) in inodes
        .iter()
        .filter(|(_, ip)| u32::from(ip.mode) & IFMT == IFDIR)
    {
        assert!(
            ip.size > 0 && ip.size % B as u64 == 0,
            "dir {ino}: size {}",
            ip.size
        );
        let mut ents = Vec::new();
        for blk in ip.data.chunks(B) {
            let mut off = 0;
            while off < B {
                let e = le32(blk, off);
                let reclen = usize::from(le16(blk, off + 4));
                let namlen = usize::from(blk[off + 6]);
                assert!(
                    reclen >= 8 && reclen % 4 == 0,
                    "dir {ino}: rec_len {reclen}"
                );
                assert!(off + reclen <= B, "dir {ino}: entry across blocks");
                if e != 0 {
                    assert!(
                        reclen >= ext2fs_dirsiz(namlen),
                        "dir {ino}: rec_len too small"
                    );
                    let target = inodes
                        .get(&e)
                        .unwrap_or_else(|| std::panic!("dir {ino}: entry to free inode {e}"));
                    let want = inot2ext2dt(e2iftodt(target.mode));
                    assert_eq!(e2d_type(blk, off), want, "dir {ino}: file type of {e}");
                    ents.push((blk[off + 8..off + 8 + namlen].to_vec(), e, blk[off + 7]));
                    *refs.entry(e).or_default() += 1;
                }
                off += reclen;
            }
            assert_eq!(
                off, B,
                "dir {ino}: the record chain ends at the block's end"
            );
        }
        assert_eq!(ents[0].0, b".", "dir {ino}: first entry");
        assert_eq!(ents[0].1, ino, "dir {ino}: `.`");
        assert_eq!(ents[1].0, b"..", "dir {ino}: second entry");
        dirs.insert(ino, ents);
    }
    for (&ino, ip) in &inodes {
        assert_eq!(
            ip.nlink,
            refs.get(&ino).copied().unwrap_or(0),
            "inode {ino}: links"
        );
    }
    // Every directory's ".." is the directory that names it (the root's is itself).
    for (&ino, ents) in &dirs {
        let parents: Vec<u32> = dirs
            .iter()
            .filter(|(_, es)| es[2..].iter().any(|e| e.1 == ino))
            .map(|(&p, _)| p)
            .collect();
        let want = if ino == EXT2_ROOTINO {
            vec![]
        } else {
            vec![ents[1].1]
        };
        assert_eq!(parents, want, "dir {ino}: `..` and the entries naming it");
    }

    // Bitmaps and counters.
    let mut freeb = 0;
    for (b, &c) in claimed.iter().enumerate().skip(1) {
        assert_eq!(bit(&d, BBITMAP, b - 1), c, "block {b} in the bitmap");
        freeb += u32::from(!c);
    }
    let mut freei = 0;
    for i in 1..=IPG as u32 {
        let used = bit(&d, IBITMAP, i as usize - 1);
        if i >= first_ino {
            assert_eq!(used, inodes.contains_key(&i), "inode {i} in the bitmap");
        }
        freei += u32::from(!used);
    }
    let ndirs = dirs.len() as u16;
    assert_eq!(
        le32(&d, sb + offset_of!(Ext2fs, e2fs_fbcount)),
        freeb,
        "sb free blocks"
    );
    assert_eq!(
        le32(&d, sb + offset_of!(Ext2fs, e2fs_ficount)),
        freei,
        "sb free inodes"
    );
    assert_eq!(le16(&d, GDT * B + 12), freeb as u16, "gd free blocks");
    assert_eq!(le16(&d, GDT * B + 14), freei as u16, "gd free inodes");
    assert_eq!(le16(&d, GDT * B + 16), ndirs, "gd directories");
    Fs { inodes, dirs }
}

impl Fs {
    /// The inode `path` names (absolute, no symbolic links).
    fn lookup(&self, path: &str) -> Option<u32> {
        let mut ino = EXT2_ROOTINO;
        for c in path.split('/').filter(|c| !c.is_empty()) {
            ino = self.dirs.get(&ino)?.iter().find(|e| e.0 == c.as_bytes())?.1;
        }
        Some(ino)
    }

    fn ino(&self, path: &str) -> &Ino {
        let i = self
            .lookup(path)
            .unwrap_or_else(|| std::panic!("{path} on disk"));
        &self.inodes[&i]
    }

    /// The names in directory `path`, without `.` and `..`.
    fn names(&self, path: &str) -> Vec<Vec<u8>> {
        let i = self.lookup(path).unwrap();
        self.dirs[&i][2..].iter().map(|e| e.0.clone()).collect()
    }
}

/// The locks a test holds: the timecounter tests' and the memory's.
type Guards = (MutexGuard<'static, ()>, MutexGuard<'static, ()>);

/// The test setup plus a fresh file system of `isz`-byte inodes mounted read-write as "/"
/// and made the thread's current directory, on the mount list as `sys_mount` leaves it.
///
/// The clock is set to 2023 first (under the timecounter tests' lock, taken before the
/// memory's as `wg_noise`'s tests do): `ext2fs_inactive` marks a freed inode by its deletion
/// time, and a time of 0 would leave it looking alive, to be freed again by the next
/// `ext2fs_inactive`, which a booted kernel never sees.
fn setup_root(isz: usize) -> (Guards, &'static Proc, &'static Mount) {
    let t = crate::kern::kern_tc::tests::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (g, p) = setup(mkfs(isz));
    crate::kern::kern_tc::tc_setrealtimeclock(&Timespec::new(1_700_000_000, 0));
    assert!(crate::kern::kern_tc::getnanotime().tv_sec >= 1_700_000_000);
    let mp = mount(p, false).unwrap();
    vfs_unbusy(mp);
    // SAFETY: a new mount on no list.
    unsafe { MOUNTLIST.0.insert_tail(mp) };
    let root = VFS_ROOT(mp).unwrap();
    set_rootvnode(Some(root));
    p.fd().fd_cdir.set(Some(root));
    vref(root);
    let _ = VOP_UNLOCK(root);
    ((t, g), p, mp)
}

/// Lets go of the root and unmounts (`dounmount`: `ext2fs_sync`, `ext2fs_unmount`).
fn unmount(p: &'static Proc, mp: &'static Mount) {
    let root = p.fd().fd_cdir.get().unwrap();
    p.fd().fd_cdir.set(None);
    set_rootvnode(None);
    vrele(root);
    vrele(root);
    vfs_busy(mp, VB_WRITE | VB_WAIT).unwrap();
    dounmount(mp, 0, p).unwrap();
    teardown();
}

/// A user-space path for the `do*at` functions: the bytes and a NUL.
fn c(path: &str) -> Vec<u8> {
    let mut v = path.as_bytes().to_vec();
    v.push(0);
    v
}

/// `open(path, O_RDWR | O_CREAT, mode)`: the vnode, unlocked and referenced.
fn create(p: &'static Proc, path: &str, mode: Mode) -> &'static Vnode {
    let path = c(path);
    let mut nd = ndinit(0, 0, NiDirp::Sys(&path), p);
    vn_open(&mut nd, FREAD | FWRITE | O_CREAT, mode).expect("create");
    let vp = nd.ni_vp.expect("a vnode");
    let _ = VOP_UNLOCK(vp);
    vp
}

/// `close` of a vnode `create` gave.
fn close(p: &'static Proc, vp: &'static Vnode) {
    vn_close(vp, FREAD | FWRITE, p.ucred(), Some(p)).expect("close");
}

/// The vnode `path` names, unlocked and referenced.
fn lookup(p: &'static Proc, path: &str) -> Result<&'static Vnode, Errno> {
    let path = c(path);
    let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(&path), p);
    namei(&mut nd)?;
    Ok(nd.ni_vp.expect("a vnode"))
}

/// `stat(path)` (`lstat` with `nofollow`).
fn stat_flags(p: &'static Proc, path: &str, follow: u64) -> Result<Vattr, Errno> {
    let path = c(path);
    let mut nd = ndinit(LOOKUP, LOCKLEAF | follow, NiDirp::Sys(&path), p);
    namei(&mut nd)?;
    let vp = nd.ni_vp.expect("a vnode");
    let mut va = Vattr::new();
    let error = VOP_GETATTR(vp, &mut va, p.ucred(), p);
    vput(vp);
    error.map(|()| va)
}

fn stat(p: &'static Proc, path: &str) -> Result<Vattr, Errno> {
    stat_flags(p, path, FOLLOW)
}

/// `vn_rdwr` of `buf` at `off` of `vp` (unlocked): the bytes moved.
fn rdwr(p: &'static Proc, rw: UioRw, vp: &'static Vnode, buf: &mut [u8], off: i64) -> usize {
    let mut resid = 0;
    let procp = if rw == UioRw::UIO_WRITE {
        None
    } else {
        Some(p)
    };
    vn_rdwr(
        rw,
        vp,
        buf.as_mut_ptr().cast(),
        buf.len(),
        off,
        UioSeg::UIO_SYSSPACE,
        0,
        p.ucred(),
        Some(&mut resid),
        procp,
    )
    .expect("vn_rdwr");
    buf.len() - resid
}

/// The whole contents of the file `path`.
fn read_file(p: &'static Proc, path: &str) -> Vec<u8> {
    let size = stat(p, path).expect("stat").va_size as usize;
    let vp = lookup(p, path).expect("lookup");
    let mut buf = vec![0u8; size + 100];
    let n = rdwr(p, UioRw::UIO_READ, vp, &mut buf, 0);
    vrele(vp);
    buf.truncate(n);
    buf
}

/// A file `path` holding `data`.
fn write_file(p: &'static Proc, path: &str, data: &[u8]) {
    let vp = create(p, path, 0o644);
    let mut w = data.to_vec();
    assert_eq!(rdwr(p, UioRw::UIO_WRITE, vp, &mut w, 0), data.len());
    close(p, vp);
}

/// `VOP_SETATTR` with the vnode locked.
fn setattr(p: &'static Proc, path: &str, f: impl FnOnce(&mut Vattr)) -> Result<(), Errno> {
    let vp = lookup(p, path)?;
    let mut va = Vattr::new();
    vattr_null(&mut va);
    f(&mut va);
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_SETATTR(vp, &mut va, p.ucred(), p);
    vput(vp);
    error
}

/// One `struct dirent` `getdents` returned.
#[derive(Debug)]
struct Ent {
    name: Vec<u8>,
    off: i64,
    fileno: u64,
}

/// One `VOP_READDIR` of the directory `path` at `offset` into a buffer of `size` bytes: the
/// entries, the new offset and the EOF flag.
fn readdir(
    p: &'static Proc,
    path: &str,
    offset: i64,
    size: usize,
) -> Result<(Vec<Ent>, i64, i32), Errno> {
    let cpath = c(path);
    let mut nd = ndinit(LOOKUP, LOCKLEAF | FOLLOW, NiDirp::Sys(&cpath), p);
    namei(&mut nd)?;
    let vp = nd.ni_vp.expect("a vnode");
    let mut buf = vec![0u8; size];
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: size,
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: offset,
        uio_resid: size,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    let mut eof = 0;
    let error = VOP_READDIR(vp, &mut uio, p.ucred(), &mut eof);
    let used = size - uio.uio_resid;
    let newoff = uio.uio_offset;
    vput(vp);
    error?;

    let mut ents = Vec::new();
    let mut off = 0;
    while off < used {
        let d = Dirent::from_bytes(&buf[off..]).expect("a dirent");
        let name = &buf[off + Dirent::NAME_OFFSET..][..usize::from(d.d_namlen)];
        assert_eq!(
            buf[off + Dirent::NAME_OFFSET + name.len()],
            0,
            "NUL-terminated"
        );
        ents.push(Ent {
            name: name.to_vec(),
            off: d.d_off,
            fileno: d.d_fileno,
        });
        off += usize::from(d.d_reclen);
    }
    Ok((ents, newoff, eof))
}

/// Every name `readdir` gives for `path`, `size` bytes at a time from offset 0, without the
/// free entries (`d_fileno` 0, which `readdir(3)` skips): the names and how many free entries
/// there were.
fn readdir_all(p: &'static Proc, path: &str, size: usize) -> (Vec<Vec<u8>>, usize) {
    let mut names = Vec::new();
    let mut free = 0;
    let mut off = 0;
    loop {
        let (ents, noff, eof) = readdir(p, path, off, size).expect("readdir");
        for e in &ents {
            if e.fileno == 0 {
                free += 1;
            } else {
                names.push(e.name.clone());
            }
        }
        if let Some(last) = ents.last() {
            assert_eq!(last.off, noff, "the last d_off is the new offset");
        }
        off = noff;
        if eof != 0 {
            return (names, free);
        }
        assert!(!ents.is_empty(), "progress");
    }
}

/// `n` bytes of a pattern that differs per `seed`.
fn pattern(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| ((i * 7 + usize::from(seed)) % 251) as u8)
        .collect()
}

#[test]
fn lookups_creates_writes_links_and_removes() {
    let (_g, p, mp) = setup_root(128);

    // Lookups of what is there and what is not.
    assert_eq!(stat(p, "/").unwrap().va_fileid, u64::from(EXT2_ROOTINO));
    assert_eq!(stat(p, "/.").unwrap().va_fileid, u64::from(EXT2_ROOTINO));
    assert_eq!(stat(p, "/..").unwrap().va_fileid, u64::from(EXT2_ROOTINO));
    assert_eq!(stat(p, "/missing").err(), Some(Errno::ENOENT));
    assert_eq!(stat(p, "/missing/x").err(), Some(Errno::ENOENT));

    // A file, written across a few blocks and read back.
    let data = pattern(3000, 1);
    write_file(p, "/hello", &data);
    let va = stat(p, "/hello").unwrap();
    assert_eq!(va.va_size, 3000);
    assert_eq!(va.va_type, VREG);
    assert_eq!(va.va_mode, 0o644);
    assert_eq!(va.va_nlink, 1);
    assert_eq!(va.va_bytes, 3 * B as u64);
    assert_eq!(read_file(p, "/hello"), data);
    assert_eq!(stat(p, "/hello/x").err(), Some(Errno::ENOTDIR));
    // A second, big enough for an indirect block.
    let big = pattern(14 * B + 7, 2);
    write_file(p, "/big", &big);
    assert_eq!(read_file(p, "/big"), big);
    assert_eq!(stat(p, "/big").unwrap().va_bytes, 16 * B as u64);

    // Hard links.
    dolinkat(
        p,
        AT_FDCWD,
        c("/hello").as_ptr(),
        AT_FDCWD,
        c("/hl").as_ptr(),
        0,
    )
    .unwrap();
    assert_eq!(stat(p, "/hl").unwrap().va_nlink, 2);
    assert_eq!(stat(p, "/hl").unwrap().va_fileid, va.va_fileid);
    assert_eq!(
        dolinkat(
            p,
            AT_FDCWD,
            c("/hello").as_ptr(),
            AT_FDCWD,
            c("/big").as_ptr(),
            0
        ),
        Err(Errno::EEXIST)
    );
    dounlinkat(p, AT_FDCWD, c("/hello").as_ptr(), 0).unwrap();
    assert_eq!(stat(p, "/hello").err(), Some(Errno::ENOENT));
    assert_eq!(stat(p, "/hl").unwrap().va_nlink, 1);
    assert_eq!(read_file(p, "/hl"), data);

    // A removed file's blocks and inode go back.
    let fs = vfstoufs(mp).e2fs();
    let (fb, fi) = (fs.e2fs_fbcount(), fs.e2fs_ficount());
    dounlinkat(p, AT_FDCWD, c("/big").as_ptr(), 0).unwrap();
    assert_eq!(fs.e2fs_fbcount(), fb + 16);
    assert_eq!(fs.e2fs_ficount(), fi + 1);

    // pathconf
    let vp = lookup(p, "/hl").unwrap();
    let mut r = 0;
    VOP_PATHCONF(vp, _PC_TIMESTAMP_RESOLUTION, &mut r).unwrap();
    assert_eq!(r, 1_000_000_000);
    VOP_PATHCONF(vp, _PC_LINK_MAX, &mut r).unwrap();
    assert_eq!(r, LINK_MAX as Register);
    vrele(vp);

    unmount(p, mp);
    let fs = fsck();
    assert_eq!(fs.names("/"), [b"hl".to_vec()]);
    assert_eq!(fs.ino("/hl").data, data);
    assert_eq!(fs.ino("/hl").nlink, 1);
}

#[test]
fn directories_are_made_renamed_and_removed() {
    let (_g, p, mp) = setup_root(256);

    domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755).unwrap();
    domkdirat(p, AT_FDCWD, c("/d/e").as_ptr(), 0o700).unwrap();
    domkdirat(p, AT_FDCWD, c("/f").as_ptr(), 0o755).unwrap();
    assert_eq!(
        domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755),
        Err(Errno::EEXIST)
    );
    let d = stat(p, "/d").unwrap();
    assert_eq!(
        (d.va_type, d.va_mode, d.va_nlink, d.va_size),
        (VDIR, 0o755, 3, B as u64)
    );
    assert_eq!(stat(p, "/").unwrap().va_nlink, 4);
    assert_eq!(stat(p, "/d/e/..").unwrap().va_fileid, d.va_fileid);
    write_file(p, "/d/x", b"x data");
    write_file(p, "/d/e/inner", b"inner");

    // A file, within a directory, then across directories.
    dorenameat(
        p,
        AT_FDCWD,
        c("/d/x").as_ptr(),
        AT_FDCWD,
        c("/d/x2").as_ptr(),
    )
    .unwrap();
    dorenameat(
        p,
        AT_FDCWD,
        c("/d/x2").as_ptr(),
        AT_FDCWD,
        c("/f/y").as_ptr(),
    )
    .unwrap();
    assert_eq!(stat(p, "/d/x2").err(), Some(Errno::ENOENT));
    assert_eq!(read_file(p, "/f/y"), b"x data");
    // Over an existing file, whose inode goes.
    write_file(p, "/f/z", b"zzz");
    let fi = vfstoufs(mp).e2fs().e2fs_ficount();
    dorenameat(
        p,
        AT_FDCWD,
        c("/f/y").as_ptr(),
        AT_FDCWD,
        c("/f/z").as_ptr(),
    )
    .unwrap();
    assert_eq!(read_file(p, "/f/z"), b"x data");
    assert_eq!(vfstoufs(mp).e2fs().e2fs_ficount(), fi + 1);
    // Two names of one file: rename(2) does nothing.
    dolinkat(
        p,
        AT_FDCWD,
        c("/f/z").as_ptr(),
        AT_FDCWD,
        c("/f/w").as_ptr(),
        0,
    )
    .unwrap();
    dorenameat(
        p,
        AT_FDCWD,
        c("/f/w").as_ptr(),
        AT_FDCWD,
        c("/f/z").as_ptr(),
    )
    .unwrap();
    assert_eq!(stat(p, "/f/z").unwrap().va_nlink, 2);
    dounlinkat(p, AT_FDCWD, c("/f/w").as_ptr(), 0).unwrap();
    assert_eq!(stat(p, "/f/z").unwrap().va_nlink, 1);

    // A directory across directories: ".." follows, the parents' link counts too.
    dorenameat(
        p,
        AT_FDCWD,
        c("/d/e").as_ptr(),
        AT_FDCWD,
        c("/f/e").as_ptr(),
    )
    .unwrap();
    assert_eq!(stat(p, "/d").unwrap().va_nlink, 2);
    assert_eq!(stat(p, "/f").unwrap().va_nlink, 3);
    assert_eq!(
        stat(p, "/f/e/..").unwrap().va_fileid,
        stat(p, "/f").unwrap().va_fileid
    );
    assert_eq!(read_file(p, "/f/e/inner"), b"inner");
    // Into its own subtree, onto a file, onto a non-empty directory: refused.
    assert_eq!(
        dorenameat(
            p,
            AT_FDCWD,
            c("/f").as_ptr(),
            AT_FDCWD,
            c("/f/e/g").as_ptr()
        ),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        dorenameat(
            p,
            AT_FDCWD,
            c("/f/e").as_ptr(),
            AT_FDCWD,
            c("/f/z").as_ptr()
        ),
        Err(Errno::ENOTDIR)
    );
    domkdirat(p, AT_FDCWD, c("/d/full").as_ptr(), 0o755).unwrap();
    write_file(p, "/d/full/f", b"");
    assert_eq!(
        dorenameat(
            p,
            AT_FDCWD,
            c("/f/e").as_ptr(),
            AT_FDCWD,
            c("/d/full").as_ptr()
        ),
        Err(Errno::ENOTEMPTY)
    );
    // A directory within its parent, then over an empty directory elsewhere.
    dorenameat(
        p,
        AT_FDCWD,
        c("/f/e").as_ptr(),
        AT_FDCWD,
        c("/f/e2").as_ptr(),
    )
    .unwrap();
    assert_eq!(stat(p, "/f").unwrap().va_nlink, 3);
    domkdirat(p, AT_FDCWD, c("/d/empty").as_ptr(), 0o755).unwrap();
    assert_eq!(stat(p, "/d").unwrap().va_nlink, 4);
    dorenameat(
        p,
        AT_FDCWD,
        c("/f/e2").as_ptr(),
        AT_FDCWD,
        c("/d/empty").as_ptr(),
    )
    .unwrap();
    assert_eq!(stat(p, "/d").unwrap().va_nlink, 4);
    assert_eq!(stat(p, "/f").unwrap().va_nlink, 2);
    assert_eq!(read_file(p, "/d/empty/inner"), b"inner");
    assert_eq!(stat(p, "/d/empty/..").unwrap().va_fileid, d.va_fileid);

    // rmdir: not a non-empty one; unlink: not a directory.
    assert_eq!(
        dounlinkat(p, AT_FDCWD, c("/d/full").as_ptr(), AT_REMOVEDIR),
        Err(Errno::ENOTEMPTY)
    );
    assert_eq!(
        dounlinkat(p, AT_FDCWD, c("/d/full").as_ptr(), 0),
        Err(Errno::EPERM)
    );
    dounlinkat(p, AT_FDCWD, c("/d/full/f").as_ptr(), 0).unwrap();
    dounlinkat(p, AT_FDCWD, c("/d/full").as_ptr(), AT_REMOVEDIR).unwrap();
    assert_eq!(stat(p, "/d/full").err(), Some(Errno::ENOENT));
    assert_eq!(stat(p, "/d").unwrap().va_nlink, 3);

    unmount(p, mp);
    let fs = fsck();
    assert_eq!(fs.names("/"), [b"d".to_vec(), b"f".to_vec()]);
    assert_eq!(fs.names("/d"), [b"empty".to_vec()]);
    assert_eq!(fs.names("/f"), [b"z".to_vec()]);
    assert_eq!(fs.names("/d/empty"), [b"inner".to_vec()]);
    assert_eq!(fs.ino("/d/empty/inner").data, b"inner");
    assert_eq!(fs.ino("/d").nlink, 3);
    assert_eq!(fs.ino("/").nlink, 4);
    assert_eq!(fs.ino("/d/empty").mode, 0o40700);
}

#[test]
fn symbolic_links_short_and_long() {
    let (_g, p, mp) = setup_root(128);
    write_file(p, "/target", b"through the link");
    dosymlinkat(p, c("target").as_ptr(), AT_FDCWD, c("/s").as_ptr()).unwrap();
    let long: std::string::String = (0..100).map(|i| (b'a' + (i % 26) as u8) as char).collect();
    dosymlinkat(p, c(&long).as_ptr(), AT_FDCWD, c("/l").as_ptr()).unwrap();
    assert_eq!(
        dosymlinkat(p, c("x").as_ptr(), AT_FDCWD, c("/s").as_ptr()),
        Err(Errno::EEXIST)
    );

    // readlink of both, lstat, and a lookup through the short one.
    let readlink = |path: &str| {
        let cpath = c(path);
        let mut nd = ndinit(LOOKUP, LOCKLEAF | NOFOLLOW, NiDirp::Sys(&cpath), p);
        namei(&mut nd).unwrap();
        let vp = nd.ni_vp.unwrap();
        let mut buf = vec![0u8; 300];
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: 300,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        VOP_READLINK(vp, &mut uio, p.ucred()).unwrap();
        let n = 300 - uio.uio_resid;
        vput(vp);
        buf.truncate(n);
        buf
    };
    assert_eq!(readlink("/s"), b"target");
    assert_eq!(readlink("/l"), long.as_bytes());
    let s = stat_flags(p, "/s", NOFOLLOW).unwrap();
    assert_eq!((s.va_type, s.va_size, s.va_bytes), (VLNK, 6, 0));
    let l = stat_flags(p, "/l", NOFOLLOW).unwrap();
    assert_eq!((l.va_type, l.va_size, l.va_bytes), (VLNK, 100, B as u64));
    assert_eq!(read_file(p, "/s"), b"through the link");

    unmount(p, mp);
    let fs = fsck();
    assert_eq!(fs.ino("/s").data, b"target");
    assert_eq!(fs.ino("/l").data, long.as_bytes());
    assert_eq!(u32::from(fs.ino("/s").mode) & IFMT, IFLNK);
    let root = &fs.dirs[&EXT2_ROOTINO];
    assert!(root.iter().any(|e| e.0 == b"s" && e.2 == EXT2_FT_SYMLINK));
    assert!(
        root.iter()
            .any(|e| e.0 == b"target" && e.2 == EXT2_FT_REG_FILE)
    );
}

#[test]
fn readdir_spans_blocks_and_skips_deleted_entries() {
    let (_g, p, mp) = setup_root(128);
    domkdirat(p, AT_FDCWD, c("/t").as_ptr(), 0o755).unwrap();
    write_file(p, "/t/base", b"b");
    // 40 links with 60-byte names: 68-byte entries, 15 to a block, three blocks.
    let name = |i: usize| std::format!("{i:02}{}", "n".repeat(58));
    for i in 0..40 {
        let to = std::format!("/t/{}", name(i));
        dolinkat(
            p,
            AT_FDCWD,
            c("/t/base").as_ptr(),
            AT_FDCWD,
            c(&to).as_ptr(),
            0,
        )
        .unwrap();
    }
    assert_eq!(stat(p, "/t").unwrap().va_size, 3 * B as u64);
    assert_eq!(stat(p, "/t/base").unwrap().va_nlink, 41);
    let mut want: Vec<Vec<u8>> = vec![b".".to_vec(), b"..".to_vec(), b"base".to_vec()];
    want.extend((0..40).map(|i| name(i).into_bytes()));
    // A buffer must reach the end of a directory block; 1024 bytes of 68-byte ext2 entries
    // are more than 1024 bytes of dirents, so the smaller reads stop and resume mid-block.
    for size in [1024, 1500, 4096] {
        let (names, free) = readdir_all(p, "/t", size);
        assert_eq!(names, want, "{size}-byte reads");
        assert_eq!(free, 0);
    }

    // Remove the first entry of the second block (its inode is zeroed, the entry stays) and
    // one in the middle of the third (its space goes to the entry before it).
    let first2 = 14; // ".", "..", "base" (36 bytes) and 14 names fill the first block
    for i in [first2, 33] {
        let path = std::format!("/t/{}", name(i));
        dounlinkat(p, AT_FDCWD, c(&path).as_ptr(), 0).unwrap();
        assert_eq!(stat(p, &path).err(), Some(Errno::ENOENT));
    }
    want.retain(|n| *n != name(first2).into_bytes() && *n != name(33).into_bytes());
    let (names, free) = readdir_all(p, "/t", 1024);
    assert_eq!(names, want);
    assert_eq!(
        free, 1,
        "the free first entry of a block is listed with d_fileno 0"
    );
    // A new name fits in the freed space: the directory does not grow.
    dolinkat(
        p,
        AT_FDCWD,
        c("/t/base").as_ptr(),
        AT_FDCWD,
        c("/t/new").as_ptr(),
        0,
    )
    .unwrap();
    assert_eq!(stat(p, "/t").unwrap().va_size, 3 * B as u64);
    // A partial entry is refused, a read past the end is empty.
    assert_eq!(readdir(p, "/t", 0, 10).err(), Some(Errno::EINVAL));
    let (ents, _, eof) = readdir(p, "/t", 3 * B as i64, 1024).unwrap();
    assert!(ents.is_empty() && eof != 0);
    assert_eq!(readdir(p, "/t/base", 0, 1024).err(), Some(Errno::ENOTDIR));

    unmount(p, mp);
    let fs = fsck();
    assert_eq!(fs.ino("/t/base").nlink, 40);
    assert_eq!(fs.names("/t").len(), 40);
}

#[test]
fn entries_are_compacted_to_make_room() {
    let (_g, p, mp) = setup_root(128);
    domkdirat(p, AT_FDCWD, c("/c").as_ptr(), 0o755).unwrap();
    // 168 names of 12-byte entries: 83 after "." and ".." fill the first block but 4 bytes,
    // 85 the second.
    write_file(p, "/c/x0", b"x");
    for i in 1..168 {
        let to = std::format!("/c/x{i}");
        dolinkat(
            p,
            AT_FDCWD,
            c("/c/x0").as_ptr(),
            AT_FDCWD,
            c(&to).as_ptr(),
            0,
        )
        .unwrap();
    }
    assert_eq!(stat(p, "/c").unwrap().va_size, 2 * B as u64);
    let unlink = |name: &str| {
        let path = std::format!("/c/{name}");
        dounlinkat(p, AT_FDCWD, c(&path).as_ptr(), 0).unwrap();
    };
    let link = |name: &str| {
        let path = std::format!("/c/{name}");
        dolinkat(
            p,
            AT_FDCWD,
            c("/c/x0").as_ptr(),
            AT_FDCWD,
            c(&path).as_ptr(),
            0,
        )
        .unwrap();
    };

    // Two 12-byte holes in the first block, none big enough for a 16-byte entry alone: the
    // entries between them move up to make one.
    unlink("x10");
    unlink("x20");
    link("yyyyy");
    // In the second block the hole starts with a free first entry (inode 0).
    unlink("x83");
    unlink("x85");
    link("zzzzz");
    assert_eq!(stat(p, "/c").unwrap().va_size, 2 * B as u64, "no new block");
    assert_eq!(read_file(p, "/c/yyyyy"), b"x");
    assert_eq!(read_file(p, "/c/zzzzz"), b"x");
    let (names, free) = readdir_all(p, "/c", 4096);
    assert_eq!(names.len(), 2 + 168 - 4 + 2);
    assert_eq!(free, 0);

    unmount(p, mp);
    let fs = fsck();
    let names = fs.names("/c");
    let at = |n: &[u8]| names.iter().position(|x| x == n).unwrap();
    // yyyyy took the place of x11..x20 moved up, before x21; zzzzz followed x84.
    assert!(at(b"x9") < at(b"yyyyy") && at(b"yyyyy") < at(b"x21"));
    assert!(at(b"x84") < at(b"zzzzz") && at(b"zzzzz") < at(b"x86"));
    assert_eq!(fs.ino("/c/x0").nlink, 166);
}

#[test]
fn attributes_are_set_and_read() {
    let (_g, p, mp) = setup_root(128);
    write_file(p, "/f", &pattern(5000, 3));

    // chmod and chown (as root).
    setattr(p, "/f", |va| va.va_mode = 0o4751).unwrap();
    assert_eq!(stat(p, "/f").unwrap().va_mode, 0o4751);
    setattr(p, "/f", |va| {
        va.va_uid = 70_000;
        va.va_gid = 20;
    })
    .unwrap();
    let va = stat(p, "/f").unwrap();
    assert_eq!((va.va_uid, va.va_gid), (70_000, 20));
    // A directory cannot be truncated.
    assert_eq!(setattr(p, "/", |va| va.va_size = 0), Err(Errno::EISDIR));
    // Unsettable attributes.
    assert_eq!(setattr(p, "/f", |va| va.va_nlink = 3), Err(Errno::EINVAL));

    // Truncate: shorter, then empty; the blocks go back.
    let fs = vfstoufs(mp).e2fs();
    let fb = fs.e2fs_fbcount();
    setattr(p, "/f", |va| va.va_size = 100).unwrap();
    assert_eq!(read_file(p, "/f"), pattern(100, 3));
    assert_eq!(fs.e2fs_fbcount(), fb + 4);
    assert_eq!(stat(p, "/f").unwrap().va_bytes, B as u64);

    // Times.
    setattr(p, "/f", |va| {
        va.va_mtime = Timespec::new(1_234_567_890, 0);
        va.va_atime = Timespec::new(1_000_000_000, 0);
    })
    .unwrap();
    let va = stat(p, "/f").unwrap();
    assert_eq!(va.va_mtime.tv_sec, 1_234_567_890);
    assert_eq!(va.va_atime.tv_sec, 1_000_000_000);

    // Immutable files cannot be written or removed.
    setattr(p, "/f", |va| va.va_flags = u64::from(SF_IMMUTABLE)).unwrap();
    assert_eq!(stat(p, "/f").unwrap().va_flags, u64::from(SF_IMMUTABLE));
    assert_eq!(setattr(p, "/f", |va| va.va_mode = 0o600), Err(Errno::EPERM));
    assert_eq!(
        dounlinkat(p, AT_FDCWD, c("/f").as_ptr(), 0),
        Err(Errno::EPERM)
    );
    setattr(p, "/f", |va| va.va_flags = 0).unwrap();
    setattr(p, "/f", |va| va.va_size = 0).unwrap();
    assert_eq!(stat(p, "/f").unwrap().va_bytes, 0);

    // A device node: mknod reloads it as a special file. Fifos need option FIFO.
    let dev = makedev(2, 7);
    domknodat(p, AT_FDCWD, c("/tty").as_ptr(), S_IFCHR | 0o600, dev).unwrap();
    let va = stat(p, "/tty").unwrap();
    assert_eq!((va.va_type, va.va_rdev), (VCHR, dev));
    assert_eq!(
        domknodat(p, AT_FDCWD, c("/fifo").as_ptr(), S_IFIFO | 0o600, 0),
        Err(Errno::EOPNOTSUPP)
    );

    unmount(p, mp);
    let fs = fsck();
    let f = fs.ino("/f");
    assert_eq!((f.uid, f.gid, f.size, f.mode), (70_000, 20, 0, 0o104751));
    assert_eq!(fs.ino("/tty").mode, 0o20600);
    assert_eq!(fs.names("/"), [b"f".to_vec(), b"tty".to_vec()]);
}

#[test]
fn a_second_vget_of_a_cached_root_owned_inode_returns() {
    // ufs_ihashget's link count check reads i_e2fs_nlink for an ext2fs inode; read as a UFS1
    // dinode, the nlink of a file owned by uid 0 is its uid_low, 0, and the lookup would wait
    // for the inode to go away forever.
    let (_g, p, mp) = setup_root(128);
    write_file(p, "/root-owned", b"r");
    let ino = stat(p, "/root-owned").unwrap().va_fileid;
    let vp = lookup(p, "/root-owned").unwrap();
    assert_eq!(vtoi(vp).i_e2fs_uid().get(), 0);
    let dev = vtoi(vp).i_dev.get();
    vrele(vp);
    for _ in 0..2 {
        let vp = VFS_VGET(mp, ino).unwrap();
        assert_eq!(vtoi(vp).i_number.get() as u64, ino);
        vput(vp);
    }
    let vp = ufs_ihashget(dev, ino as Ufsino).expect("in the hash");
    vput(vp);
    unmount(p, mp);
    fsck();
}
