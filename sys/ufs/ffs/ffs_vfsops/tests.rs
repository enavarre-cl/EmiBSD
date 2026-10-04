//! Host tests for the fast file system: `newfs`, a small `newfs(8)`/`mkfs.c` equivalent that
//! lays out an FFS1 or FFS2 image in memory (super-block, cylinder groups, root directory,
//! and files in the root directory), a block device vnode whose strategy reads and writes
//! that image, and tests that mount it with `ffs_mountfs`, read, write, create and remove
//! files and directories through the system calls, sync, unmount, check the image's
//! counters and mount it again.

use core::ptr;
use core::sync::atomic::Ordering;
use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert_eq, vec};

use super::*;
use crate::kern::kern_descrip::sys_close;
use crate::kern::subr_xxx::nullop;
use crate::kern::sys_generic::{sys_read, sys_write};
use crate::kern::vfs_bio::{BCSTATS, BUFHEAD, BUFKVM, CLEANCACHE, biodone, bufinit};
use crate::kern::vfs_default::vop_generic_bwrite;
use crate::kern::vfs_init::{set_rootvnode, vfs_byname};
use crate::kern::vfs_subr::{vflushbuf, vfs_busy, vfs_mount_alloc};
use crate::kern::vfs_syscalls::{
    dounmount, sys_fsync, sys_getdents, sys_mkdir, sys_open, sys_readlink, sys_rename, sys_rmdir,
    sys_symlink, sys_sync, sys_unlink,
};
use crate::kern::vfs_vops::VOP_UNLOCK;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::sys::buf::{B_ERROR, B_READ};
use crate::sys::fcntl::{O_CREAT, O_RDONLY, O_RDWR};
use crate::sys::mount::{MNT_WAIT, VB_WAIT, VB_WRITE, VFS_ROOT};
use crate::sys::systm::{SyCall, SysArgs};
use crate::sys::types::{Register, makedev};
use crate::sys::vnode::{VopFsyncArgs, VopInactiveArgs, VopStrategyArgs, Vops};

/// `newfs`: an FFS image built in memory the way `newfs(8)` (`sbin/newfs/mkfs.c`) builds
/// one, for the tests and as a record of the on-disk layout.
pub(crate) mod newfs {
    use std::boxed::Box;
    use std::vec;
    use std::vec::Vec;

    use crate::sys::param::{DEV_BSIZE, MAXBSIZE, howmany};
    use crate::ufs::ffs::ffs_subr::{ffs_clrblock, ffs_isblock, ffs_setblock};
    use crate::ufs::ffs::fs::*;
    use crate::ufs::ufs::dinode::*;
    use crate::ufs::ufs::dir::{DIRBLKSIZ, DT_DIR, DT_REG, Direct, dirsiz};
    use crate::ufs::ufs::inode::{set_dinode1_at, set_dinode2_at};

    /// A block-aligned buffer as big as the largest block.
    #[repr(C, align(4096))]
    pub(crate) struct Blk(pub(crate) [u8; MAXBSIZE]);

    /// The geometry to build: the format, the image size, the block and fragment sizes, the
    /// fragments and inodes per cylinder group.
    #[derive(Clone, Copy)]
    pub(crate) struct Params {
        /// FFS2 (UFS2 dinodes, super-block at 64 KB) or FFS1 (super-block at 8 KB).
        pub(crate) ufs2: bool,
        /// The image's size in bytes (a multiple of `fpg * fsize` keeps every group full).
        pub(crate) size: usize,
        /// `fs_bsize`.
        pub(crate) bsize: i32,
        /// `fs_fsize`.
        pub(crate) fsize: i32,
        /// `fs_fpg`.
        pub(crate) fpg: i32,
        /// `fs_ipg` (a multiple of `INOPB`).
        pub(crate) ipg: u32,
    }

    /// FFS2, 4 MB: 8 KB blocks, 1 KB fragments, two cylinder groups of 512 inodes.
    pub(crate) const FFS2_4M: Params = Params {
        ufs2: true,
        size: 4 << 20,
        bsize: 8192,
        fsize: 1024,
        fpg: 2048,
        ipg: 512,
    };

    /// FFS1, 4 MB, as `makefs` builds the install ramdisks: 4 KB blocks, 512-byte fragments,
    /// two cylinder groups of 512 inodes.
    pub(crate) const FFS1_4M: Params = Params {
        ufs2: false,
        size: 4 << 20,
        bsize: 4096,
        fsize: 512,
        fpg: 4096,
        ipg: 512,
    };

    /// The fixed time stamp of everything `newfs` writes.
    pub(crate) const TIME: i64 = 1_700_000_000;

    /// An image under construction: the bytes, the super-block, each cylinder group block
    /// and the summaries, which `finish` writes into the bytes.
    pub(crate) struct Image {
        /// The disk.
        pub(crate) disk: Vec<u8>,
        /// The super-block.
        sb: Box<Blk>,
        /// Each cylinder group block.
        cgs: Vec<Box<Blk>>,
        /// The next generation number handed out.
        gen_seed: u32,
    }

    /// `ilog2`.
    fn ilog2(v: i32) -> i32 {
        31 - v.leading_zeros() as i32
    }

    impl Image {
        /// The super-block.
        pub(crate) fn fs(&self) -> &Fs {
            // SAFETY: `sb` is an aligned buffer larger than `Fs`, which is `Cell`s of
            // integers and raw pointers, valid for any bytes.
            unsafe { &*self.sb.0.as_ptr().cast::<Fs>() }
        }

        /// The header of cylinder group `cg`.
        fn cg(&self, cg: u32) -> &Cg {
            // SAFETY: as for `fs`.
            unsafe { &*self.cgs[cg as usize].0.as_ptr().cast::<Cg>() }
        }

        /// The bytes of cylinder group `cg` from `off` on (a map).
        fn cgmap(&mut self, cg: u32, off: u32) -> &mut [u8] {
            &mut self.cgs[cg as usize].0[off as usize..]
        }

        /// `fs_cs(fs, cg)`, kept in the image (the in-core array does not exist here).
        fn cs_add(&mut self, cg: u32, field: usize, d: i32) {
            let off = self.cs_off(cg) + field * 4;
            let v = i32::from_ne_bytes(self.disk[off..off + 4].try_into().unwrap()) + d;
            self.disk[off..off + 4].copy_from_slice(&v.to_ne_bytes());
        }

        /// The byte offset in the image of `fs_cs(fs, cg)`.
        fn cs_off(&self, cg: u32) -> usize {
            let fs = self.fs();
            fsbtodb(fs, fs.fs_csaddr.get()) as usize * DEV_BSIZE + cg as usize * 16
        }

        /// `mkfs` and `fsinit`: an empty file system with a root directory.
        pub(crate) fn new(p: Params) -> Self {
            let mut img = Image {
                disk: vec![0u8; p.size],
                sb: Box::new(Blk([0; MAXBSIZE])),
                cgs: Vec::new(),
                gen_seed: 0x1234_5678,
            };
            let fs = img.fs();
            let frag = p.bsize / p.fsize;
            fs.fs_postblformat.set(FS_DYNAMICPOSTBLFMT);
            fs.fs_avgfilesize.set(AVFILESIZ);
            fs.fs_avgfpdir.set(AFPDIR);
            fs.fs_bsize.set(p.bsize);
            fs.fs_fsize.set(p.fsize);
            fs.fs_bmask.set(!(p.bsize - 1));
            fs.fs_fmask.set(!(p.fsize - 1));
            fs.fs_qbmask.set(i64::from(!fs.fs_bmask.get()));
            fs.fs_qfmask.set(i64::from(!fs.fs_fmask.get()));
            fs.fs_bshift.set(ilog2(p.bsize));
            fs.fs_fshift.set(ilog2(p.fsize));
            fs.fs_frag.set(frag);
            fs.fs_fragshift.set(ilog2(frag));
            fs.fs_fsbtodb.set(ilog2(p.fsize / DEV_BSIZE as i32));
            fs.fs_size.set((p.size / p.fsize as usize) as i64);
            fs.fs_nspf.set(p.fsize / DEV_BSIZE as i32);
            fs.fs_maxcontig.set(1);
            fs.fs_nrpos.set(1);
            fs.fs_cpg.set(1);
            if p.ufs2 {
                fs.fs_inodefmt.set(FS_44INODEFMT);
                fs.fs_sblockloc.set(i64::from(SBLOCK_UFS2));
                fs.fs_nindir.set(p.bsize / 8);
                fs.fs_inopb.set((p.bsize / 256) as u32);
                fs.fs_maxsymlinklen.set(MAXSYMLINKLEN_UFS2 as i32);
            } else {
                fs.fs_sblockloc.set(i64::from(SBLOCK_UFS1));
                fs.fs_nindir.set(p.bsize / 4);
                fs.fs_inopb.set((p.bsize / 128) as u32);
                fs.fs_maxsymlinklen.set(MAXSYMLINKLEN_UFS1 as i32);
                fs.fs_inodefmt.set(FS_44INODEFMT);
                fs.fs_cgoffset.set(0);
                fs.fs_cgmask.set(-1);
                fs.fs_ffs1_size.set(fs.fs_size.get() as i32);
                fs.fs_rotdelay.set(0);
                fs.fs_rps.set(60);
                fs.fs_interleave.set(1);
                fs.fs_trackskew.set(0);
                fs.fs_cpc.set(0);
            }
            let div_ceil = |x: i64, y: i64| (x + y - 1) / y;
            let roundup = |x: i64, y: i64| div_ceil(x, y) * y;
            fs.fs_sblkno.set(roundup(
                div_ceil(
                    fs.fs_sblockloc.get() + SBLOCKSIZE as i64,
                    i64::from(p.fsize),
                ),
                i64::from(frag),
            ) as i32);
            fs.fs_cblkno.set(
                fs.fs_sblkno.get()
                    + roundup(div_ceil(SBSIZE as i64, i64::from(p.fsize)), i64::from(frag)) as i32,
            );
            fs.fs_iblkno.set(fs.fs_cblkno.get() + frag);
            let mut maxfilesize = p.bsize as u64 * NDADDR as u64 - 1;
            let mut sizepb = p.bsize as u64;
            for _ in 0..NIADDR {
                sizepb *= fs.fs_nindir.get() as u64;
                maxfilesize += sizepb;
            }
            fs.fs_maxfilesize.set(maxfilesize);
            fs.fs_fpg.set(p.fpg);
            fs.fs_ipg.set(p.ipg);
            let ncg = (fs.fs_size.get() as usize).div_ceil(p.fpg as usize) as u32;
            fs.fs_ncg.set(ncg);
            if !p.ufs2 {
                fs.fs_spc.set(p.fpg * fs.fs_nspf.get());
                fs.fs_nsect.set(fs.fs_spc.get());
                fs.fs_npsect.set(fs.fs_spc.get());
                fs.fs_ncyl.set(ncg as i32);
            }
            fs.fs_cgsize.set(fragroundup(fs, cgsize(fs) as i64) as i32);
            fs.fs_dblkno
                .set(fs.fs_iblkno.get() + (p.ipg / inopf(fs)) as i32);
            fs.fs_csaddr.set(cgdmin(fs, 0));
            fs.fs_cssize
                .set(fragroundup(fs, (ncg as usize * 16) as i64) as i32);
            fs.fs_sbsize
                .set(fragroundup(fs, size_of::<Fs>() as i64).min(SBLOCKSIZE as i64) as i32);
            fs.fs_minfree.set(MINFREE);
            fs.fs_maxbpg
                .set(if p.ufs2 { p.bsize / 8 } else { p.bsize / 4 });
            fs.fs_optim.set(FS_OPTTIME);
            fs.fs_clean.set(1);
            fs.fs_id[0].set(TIME as i32);
            fs.fs_id[1].set(0x0eb5_d00d);

            let csfrags = howmany(fs.fs_cssize.get() as usize, p.fsize as usize) as i64;
            let dsize = fs.fs_size.get()
                - i64::from(fs.fs_sblkno.get())
                - i64::from(ncg) * i64::from(fs.fs_dblkno.get() - fs.fs_sblkno.get());
            fs.fs_dsize.set(dsize);
            fs.fs_cstotal
                .cs_nbfree
                .set(fragstoblks(fs, dsize) - div_ceil(csfrags, i64::from(frag)));
            fs.fs_cstotal.cs_nffree.set(
                fragnum(fs, fs.fs_size.get())
                    + if fragnum(fs, csfrags) > 0 {
                        i64::from(frag) - fragnum(fs, csfrags)
                    } else {
                        0
                    },
            );
            fs.fs_cstotal
                .cs_nifree
                .set(i64::from(ncg * p.ipg - ROOTINO));
            fs.fs_cstotal.cs_ndir.set(0);
            fs.fs_dsize.set(dsize - csfrags);
            fs.fs_time.set(TIME);
            fs.fs_magic
                .set(if p.ufs2 { FS_UFS2_MAGIC } else { FS_UFS1_MAGIC });

            for cg in 0..ncg {
                img.initcg(cg, p);
            }
            img.fsinit();
            img
        }

        /// `initcg`: initialize a cylinder group.
        fn initcg(&mut self, cg: u32, p: Params) {
            let fs = self.fs();
            let ufs2 = p.ufs2;
            let frag = fs.fs_frag.get();
            let ipg = fs.fs_ipg.get();
            let cbase = cgbase(fs, cg);
            let dmax = (cbase + i64::from(fs.fs_fpg.get())).min(fs.fs_size.get());
            let dlower = cgsblock(fs, cg) - cbase;
            let mut dupper = cgdmin(fs, cg) - cbase;
            if cg == 0 {
                dupper += howmany(fs.fs_cssize.get() as usize, fs.fs_fsize.get() as usize) as i64;
            }
            let inopb = inopb(fs);
            let cpg = fs.fs_cpg.get();
            let ncg = fs.fs_ncg.get();
            let fpg = fs.fs_fpg.get();
            self.cgs.push(Box::new(Blk([0; MAXBSIZE])));
            let c = self.cg(cg);
            c.cg_ffs2_time.set(TIME);
            c.cg_magic.set(CG_MAGIC);
            c.cg_cgx.set(cg);
            c.cg_ffs2_niblk.set(ipg);
            c.cg_initediblk.set(ipg.min(2 * inopb));
            c.cg_ndblk.set((dmax - cbase) as u32);
            let start = size_of::<Cg>() as u32;
            if !ufs2 {
                // Hack to maintain compatibility with old fsck.
                c.cg_ncyl.set(if cg == ncg - 1 { 0 } else { cpg as i16 });
                c.cg_time.set(TIME as i32);
                c.cg_ffs2_time.set(0);
                c.cg_niblk.set(c.cg_ffs2_niblk.get() as i16);
                c.cg_ffs2_niblk.set(0);
                c.cg_initediblk.set(0);
                c.cg_btotoff.set(start as i32);
                c.cg_boff.set(c.cg_btotoff.get() + cpg * 4);
                c.cg_iusedoff.set((c.cg_boff.get() + cpg * 2) as u32);
            } else {
                c.cg_iusedoff.set(start);
            }
            c.cg_freeoff
                .set(c.cg_iusedoff.get() + howmany(ipg as usize, 8) as u32);
            c.cg_nextfreeoff
                .set(c.cg_freeoff.get() + howmany(fpg as usize, 8) as u32);
            c.cg_cs.cs_nifree.set(c.cg_cs.cs_nifree.get() + ipg as i32);
            let iusedoff = c.cg_iusedoff.get();
            let freeoff = c.cg_freeoff.get();
            if cg == 0 {
                for i in 0..ROOTINO as usize {
                    self.cgmap(cg, iusedoff)[i / 8] |= 1 << (i % 8);
                    let c = self.cg(cg);
                    c.cg_cs.cs_nifree.set(c.cg_cs.cs_nifree.get() - 1);
                }
            }
            let blocks_free = |img: &mut Self, d: i64| {
                let blkno = d / i64::from(frag);
                // SAFETY-free: the map is the image's own bytes.
                let fsp: *const Fs = img.fs();
                // SAFETY: `fsp` points into `img.sb`, which `cgmap` does not borrow.
                ffs_setblock(unsafe { &*fsp }, img.cgmap(cg, freeoff), blkno);
                let c = img.cg(cg);
                c.cg_cs.cs_nbfree.set(c.cg_cs.cs_nbfree.get() + 1);
                if !ufs2 {
                    img.cg_blktot_add(cg, 0, 1);
                }
            };
            if cg > 0 {
                // In cg 0, space is reserved for boot and super blocks.
                let mut d = 0;
                while d < dlower {
                    blocks_free(self, d);
                    d += i64::from(frag);
                }
            }
            let r = dupper % i64::from(frag);
            if r != 0 {
                let c = self.cg(cg);
                let k = (i64::from(frag) - r) as usize;
                c.cg_frsum[k].set(c.cg_frsum[k].get() + 1);
                let end = dupper + i64::from(frag) - r;
                while dupper < end {
                    self.cgmap(cg, freeoff)[(dupper / 8) as usize] |= 1 << (dupper % 8);
                    let c = self.cg(cg);
                    c.cg_cs.cs_nffree.set(c.cg_cs.cs_nffree.get() + 1);
                    dupper += 1;
                }
            }
            let ndblk = i64::from(self.cg(cg).cg_ndblk.get());
            let mut d = dupper;
            while d + i64::from(frag) <= ndblk {
                blocks_free(self, d);
                d += i64::from(frag);
            }
            if d < ndblk {
                let c = self.cg(cg);
                let k = (ndblk - d) as usize;
                c.cg_frsum[k].set(c.cg_frsum[k].get() + 1);
                while d < ndblk {
                    self.cgmap(cg, freeoff)[(d / 8) as usize] |= 1 << (d % 8);
                    let c = self.cg(cg);
                    c.cg_cs.cs_nffree.set(c.cg_cs.cs_nffree.get() + 1);
                    d += 1;
                }
            }
            // *cs = acg.cg_cs
            let c = self.cg(cg);
            let cs = [
                c.cg_cs.cs_ndir.get(),
                c.cg_cs.cs_nbfree.get(),
                c.cg_cs.cs_nifree.get(),
                c.cg_cs.cs_nffree.get(),
            ];
            for (i, v) in cs.iter().enumerate() {
                self.cs_add(cg, i, *v);
            }

            // Generation numbers for the inodes newfs initialises: the first two blocks of
            // each group for FFS2 (the rest lazily, ffs_nodealloccg), all of them for FFS1.
            let n = if ufs2 { ipg.min(2 * inopb) } else { ipg };
            for i in 0..n {
                let ino = cg * ipg + i;
                let g = self.next_gen();
                self.with_dinode(ino, |d| match d {
                    Din::Ufs1(d) => d.di_gen = g,
                    Din::Ufs2(d) => d.di_gen = g as i32,
                });
            }
        }

        /// `cg_blktot(cgp)[cylno] += d` (FFS1).
        fn cg_blktot_add(&mut self, cg: u32, cylno: usize, d: i32) {
            let off = self.cg(cg).cg_btotoff.get() as u32 + cylno as u32 * 4;
            let b = self.cgmap(cg, off);
            let v = i32::from_ne_bytes(b[..4].try_into().unwrap()) + d;
            b[..4].copy_from_slice(&v.to_ne_bytes());
            let boff = self.cg(cg).cg_boff.get() as u32;
            let b = self.cgmap(cg, boff);
            let v = i16::from_ne_bytes(b[..2].try_into().unwrap()) + d as i16;
            b[..2].copy_from_slice(&v.to_ne_bytes());
        }

        /// A fresh nonzero generation number.
        fn next_gen(&mut self) -> u32 {
            self.gen_seed = self
                .gen_seed
                .wrapping_mul(1_103_515_245)
                .wrapping_add(12345)
                | 1;
            self.gen_seed
        }

        /// `f` on the dinode of `ino` in the image.
        fn with_dinode(&mut self, ino: Ufsino, f: impl FnOnce(&mut Din)) {
            let fs = self.fs();
            let blk = fsbtodb(fs, ino_to_fsba(fs, ino)) as usize * DEV_BSIZE;
            let idx = ino_to_fsbo(fs, ino);
            let ufs2 = fs.fs_magic.get() == FS_UFS2_MAGIC;
            let bsize = fs.fs_bsize.get() as usize;
            let b = &mut self.disk[blk..blk + bsize];
            if ufs2 {
                let mut d = Din::Ufs2(crate::ufs::ufs::inode::dinode2_at(b, idx));
                f(&mut d);
                if let Din::Ufs2(d) = d {
                    set_dinode2_at(b, idx, &d);
                }
            } else {
                let mut d = Din::Ufs1(crate::ufs::ufs::inode::dinode1_at(b, idx));
                f(&mut d);
                if let Din::Ufs1(d) = d {
                    set_dinode1_at(b, idx, &d);
                }
            }
        }

        /// `alloc(size, mode)`: a block (or its first fragments) in cylinder group 0.
        fn alloc(&mut self, size: i32, mode: u32) -> i64 {
            let fs = self.fs();
            let frag = fs.fs_frag.get();
            let fsize = fs.fs_fsize.get();
            let bsize = fs.fs_bsize.get();
            let ufs2 = fs.fs_magic.get() == FS_UFS2_MAGIC;
            let freeoff = self.cg(0).cg_freeoff.get();
            let ndblk = i64::from(self.cg(0).cg_ndblk.get());
            let fsp: *const Fs = self.fs();
            // SAFETY: `fsp` points into `self.sb`, which `cgmap` does not borrow.
            let fs = unsafe { &*fsp };
            let mut d = 0;
            while d < ndblk {
                if ffs_isblock(fs, self.cgmap(0, freeoff), d / i64::from(frag)) {
                    break;
                }
                d += i64::from(frag);
            }
            assert!(d < ndblk, "newfs: cg 0 is full");
            ffs_clrblock(fs, self.cgmap(0, freeoff), d / i64::from(frag));
            let c = self.cg(0);
            c.cg_cs.cs_nbfree.set(c.cg_cs.cs_nbfree.get() - 1);
            fs.fs_cstotal
                .cs_nbfree
                .set(fs.fs_cstotal.cs_nbfree.get() - 1);
            self.cs_add(0, 1, -1);
            if mode & IFMT == IFDIR {
                let c = self.cg(0);
                c.cg_cs.cs_ndir.set(c.cg_cs.cs_ndir.get() + 1);
                fs.fs_cstotal.cs_ndir.set(fs.fs_cstotal.cs_ndir.get() + 1);
                self.cs_add(0, 0, 1);
            }
            if !ufs2 {
                self.cg_blktot_add(0, 0, -1);
            }
            if size != bsize {
                let used = howmany(size as usize, fsize as usize) as i32;
                let c = self.cg(0);
                c.cg_cs.cs_nffree.set(c.cg_cs.cs_nffree.get() + frag - used);
                fs.fs_cstotal
                    .cs_nffree
                    .set(fs.fs_cstotal.cs_nffree.get() + i64::from(frag - used));
                self.cs_add(0, 3, frag - used);
                let k = (frag - used) as usize;
                let c = self.cg(0);
                c.cg_frsum[k].set(c.cg_frsum[k].get() + 1);
                for i in used..frag {
                    let bit = (d + i64::from(i)) as usize;
                    self.cgmap(0, freeoff)[bit / 8] |= 1 << (bit % 8);
                }
            }
            d
        }

        /// `iput`: allocate inode `ino` in the map and write its dinode.
        fn iput(&mut self, ino: Ufsino, f: impl FnOnce(&mut Din)) {
            let iusedoff = self.cg(0).cg_iusedoff.get();
            let c = self.cg(0);
            c.cg_cs.cs_nifree.set(c.cg_cs.cs_nifree.get() - 1);
            self.cgmap(0, iusedoff)[ino as usize / 8] |= 1 << (ino % 8);
            let fs = self.fs();
            fs.fs_cstotal
                .cs_nifree
                .set(fs.fs_cstotal.cs_nifree.get() - 1);
            self.cs_add(0, 2, -1);
            let g = self.next_gen();
            self.with_dinode(ino, |d| {
                match d {
                    Din::Ufs1(d) => d.di_gen = g,
                    Din::Ufs2(d) => d.di_gen = g as i32,
                }
                f(d);
            });
        }

        /// The byte offset in the image of fragment `frag`.
        fn frag_off(&self, frag: i64) -> usize {
            fsbtodb(self.fs(), frag) as usize * DEV_BSIZE
        }

        /// `fsinit`: the root directory (`.` and `..` in one `DIRBLKSIZ` block).
        fn fsinit(&mut self) {
            let fsize = self.fs().fs_fsize.get();
            let mode = IFDIR | 0o755;
            let db0 = self.alloc(fsize, mode);
            let off = self.frag_off(db0);
            let mut dot = Direct::new();
            dot.d_ino = ROOTINO;
            dot.d_type = DT_DIR;
            dot.d_namlen = 1;
            dot.d_name[0] = b'.';
            dot.d_reclen = dirsiz(1) as u16;
            dot.write_to(&mut self.disk, off);
            let mut dotdot = dot;
            dotdot.d_namlen = 2;
            dotdot.d_name[1] = b'.';
            dotdot.d_reclen = (DIRBLKSIZ - dirsiz(1)) as u16;
            dotdot.write_to(&mut self.disk, off + dirsiz(1));
            let blocks = (fragroundup(self.fs(), DIRBLKSIZ as i64) / DEV_BSIZE as i64) as i64;
            self.iput(ROOTINO, |d| match d {
                Din::Ufs1(d) => {
                    d.di_mode = mode as u16;
                    d.di_nlink = 2;
                    d.di_size = DIRBLKSIZ as u64;
                    d.di_db[0] = db0 as i32;
                    d.di_blocks = blocks as i32;
                    (d.di_atime, d.di_mtime, d.di_ctime) = (TIME as i32, TIME as i32, TIME as i32);
                }
                Din::Ufs2(d) => {
                    d.di_mode = mode as u16;
                    d.di_nlink = 2;
                    d.di_size = DIRBLKSIZ as u64;
                    d.di_db[0] = db0;
                    d.di_blocks = blocks as u64;
                    (d.di_atime, d.di_mtime, d.di_ctime) = (TIME, TIME, TIME);
                }
            });
        }

        /// A regular file `name` holding `data` in the root directory (direct blocks only;
        /// the last block is fragments when it is the file's only partial one): its inode.
        pub(crate) fn add_file(&mut self, name: &[u8], data: &[u8]) -> Ufsino {
            let fs = self.fs();
            let bsize = fs.fs_bsize.get() as usize;
            let nblk = data.len().div_ceil(bsize);
            assert!(nblk <= NDADDR, "newfs: add_file is direct blocks only");
            // The next free inode of cylinder group 0.
            let iusedoff = self.cg(0).cg_iusedoff.get();
            let map = self.cgmap(0, iusedoff);
            let ino = (0..)
                .find(|&i: &u32| map[i as usize / 8] & (1 << (i % 8)) == 0)
                .unwrap();
            let mut db = [0i64; NDADDR];
            let mut used = 0i64;
            for (b, chunk) in data.chunks(bsize).enumerate() {
                let size = if chunk.len() == bsize {
                    bsize as i32
                } else {
                    fragroundup(self.fs(), chunk.len() as i64) as i32
                };
                db[b] = self.alloc(size, IFREG);
                used += i64::from(size);
                let off = self.frag_off(db[b]);
                self.disk[off..off + chunk.len()].copy_from_slice(chunk);
            }
            let mode = IFREG | 0o644;
            let blocks = used / DEV_BSIZE as i64;
            self.iput(ino, |d| match d {
                Din::Ufs1(d) => {
                    d.di_mode = mode as u16;
                    d.di_nlink = 1;
                    d.di_size = data.len() as u64;
                    for (i, b) in db.iter().enumerate() {
                        d.di_db[i] = *b as i32;
                    }
                    d.di_blocks = blocks as i32;
                    (d.di_atime, d.di_mtime, d.di_ctime) = (TIME as i32, TIME as i32, TIME as i32);
                }
                Din::Ufs2(d) => {
                    d.di_mode = mode as u16;
                    d.di_nlink = 1;
                    d.di_size = data.len() as u64;
                    d.di_db = db;
                    d.di_blocks = blocks as u64;
                    (d.di_atime, d.di_mtime, d.di_ctime) = (TIME, TIME, TIME);
                }
            });
            self.dir_add(name, ino, DT_REG);
            ino
        }

        /// Adds an entry to the root directory's block, splitting the last entry's space.
        fn dir_add(&mut self, name: &[u8], ino: Ufsino, dtype: u8) {
            let mut root_db0 = 0;
            self.with_dinode(ROOTINO, |d| {
                root_db0 = match d {
                    Din::Ufs1(d) => i64::from(d.di_db[0]),
                    Din::Ufs2(d) => d.di_db[0],
                }
            });
            let base = self.frag_off(root_db0);
            let blk = &mut self.disk[base..base + DIRBLKSIZ];
            let mut off = 0;
            loop {
                let reclen = usize::from(crate::ufs::ufs::dir::d_reclen(blk, off));
                if off + reclen >= DIRBLKSIZ {
                    break;
                }
                off += reclen;
            }
            let used = dirsiz(crate::ufs::ufs::dir::d_namlen(blk, off));
            let reclen = usize::from(crate::ufs::ufs::dir::d_reclen(blk, off));
            assert!(
                reclen - used >= dirsiz(name.len() as u8),
                "newfs: root directory full"
            );
            crate::ufs::ufs::dir::set_d_reclen(blk, off, used as u16);
            let mut e = Direct::new();
            e.d_ino = ino;
            e.d_type = dtype;
            e.d_namlen = name.len() as u8;
            e.d_name[..name.len()].copy_from_slice(name);
            e.d_reclen = (reclen - used) as u16;
            e.write_to(blk, off + used);
        }

        /// Writes the super-block (and its copy in each group), the cylinder groups and the
        /// summaries into the image, and returns it.
        pub(crate) fn finish(mut self) -> Vec<u8> {
            let fs = self.fs();
            if fs.fs_magic.get() == FS_UFS1_MAGIC {
                fs.fs_ffs1_time.set(fs.fs_time.get() as i32);
                fs.fs_ffs1_dsize.set(fs.fs_dsize.get() as i32);
                fs.fs_ffs1_csaddr.set(fs.fs_csaddr.get() as i32);
                fs.fs_ffs1_cstotal
                    .cs_ndir
                    .set(fs.fs_cstotal.cs_ndir.get() as i32);
                fs.fs_ffs1_cstotal
                    .cs_nbfree
                    .set(fs.fs_cstotal.cs_nbfree.get() as i32);
                fs.fs_ffs1_cstotal
                    .cs_nifree
                    .set(fs.fs_cstotal.cs_nifree.get() as i32);
                fs.fs_ffs1_cstotal
                    .cs_nffree
                    .set(fs.fs_cstotal.cs_nffree.get() as i32);
            }
            let loc = fs.fs_sblockloc.get() as usize;
            let cgsz = fs.fs_cgsize.get() as usize;
            let ncg = fs.fs_ncg.get();
            let mut offs = Vec::new();
            for cg in 0..ncg {
                offs.push((
                    self.frag_off(cgsblock(self.fs(), cg)),
                    self.frag_off(cgtod(self.fs(), cg)),
                ));
            }
            let sb = self.sb.0[..SBLOCKSIZE].to_vec();
            self.disk[loc..loc + SBLOCKSIZE].copy_from_slice(&sb);
            for (cg, (sboff, cgoff)) in offs.into_iter().enumerate() {
                self.disk[sboff..sboff + SBLOCKSIZE].copy_from_slice(&sb);
                let blk = self.cgs[cg].0[..cgsz].to_vec();
                self.disk[cgoff..cgoff + cgsz].copy_from_slice(&blk);
            }
            self.disk
        }
    }

    /// A dinode of either format, as `with_dinode` hands it.
    pub(crate) enum Din {
        /// FFS1.
        Ufs1(Ufs1Dinode),
        /// FFS2.
        Ufs2(Ufs2Dinode),
    }

    /// `fsck`'s counter check: the summaries of each cylinder group against its maps, and
    /// their sum against the super-block's totals. Panics on a mismatch.
    pub(crate) fn check(disk: &[u8], ufs2: bool) {
        let loc = if ufs2 { SBLOCK_UFS2 } else { SBLOCK_UFS1 } as usize;
        let mut sb = Box::new(Blk([0; MAXBSIZE]));
        sb.0[..SBLOCKSIZE].copy_from_slice(&disk[loc..loc + SBLOCKSIZE]);
        // SAFETY: an aligned buffer larger than `Fs`, valid for any bytes.
        let fs = unsafe { &*sb.0.as_ptr().cast::<Fs>() };
        let frag = fs.fs_frag.get();
        let cs_off = fsbtodb(fs, fs.fs_csaddr.get()) as usize * DEV_BSIZE;
        let (mut tb, mut tf, mut ti, mut td) = (0i64, 0i64, 0i64, 0i64);
        for cg in 0..fs.fs_ncg.get() {
            let off = fsbtodb(fs, cgtod(fs, cg)) as usize * DEV_BSIZE;
            let mut blk = Box::new(Blk([0; MAXBSIZE]));
            let sz = fs.fs_cgsize.get() as usize;
            blk.0[..sz].copy_from_slice(&disk[off..off + sz]);
            // SAFETY: as above.
            let c = unsafe { &*blk.0.as_ptr().cast::<Cg>() };
            assert_eq!(c.cg_magic.get(), CG_MAGIC, "cg {cg} magic");
            let freemap = &blk.0[c.cg_freeoff.get() as usize..];
            let inomap = &blk.0[c.cg_iusedoff.get() as usize..];
            let (mut nb, mut nf) = (0, 0);
            let ndblk = c.cg_ndblk.get() as i64;
            let mut d = 0;
            while d < ndblk {
                if ffs_isblock(fs, freemap, d / i64::from(frag)) {
                    nb += 1;
                } else {
                    for i in 0..i64::from(frag) {
                        if d + i < ndblk
                            && freemap[((d + i) / 8) as usize] & (1 << ((d + i) % 8)) != 0
                        {
                            nf += 1;
                        }
                    }
                }
                d += i64::from(frag);
            }
            let ni = (0..fs.fs_ipg.get())
                .filter(|&i| inomap[i as usize / 8] & (1 << (i % 8)) == 0)
                .count() as i32;
            assert_eq!(c.cg_cs.cs_nbfree.get(), nb, "cg {cg} nbfree");
            assert_eq!(c.cg_cs.cs_nffree.get(), nf, "cg {cg} nffree");
            assert_eq!(c.cg_cs.cs_nifree.get(), ni, "cg {cg} nifree");
            let s = |k: usize| {
                let o = cs_off + cg as usize * 16 + k * 4;
                i32::from_ne_bytes(disk[o..o + 4].try_into().unwrap())
            };
            assert_eq!(
                [s(0), s(1), s(2), s(3)],
                [c.cg_cs.cs_ndir.get(), nb, ni, nf],
                "cg {cg} summary"
            );
            tb += i64::from(nb);
            tf += i64::from(nf);
            ti += i64::from(ni);
            td += i64::from(c.cg_cs.cs_ndir.get());
        }
        assert_eq!(fs.fs_cstotal.cs_nbfree.get(), tb, "total nbfree");
        assert_eq!(fs.fs_cstotal.cs_nffree.get(), tf, "total nffree");
        assert_eq!(fs.fs_cstotal.cs_nifree.get(), ti, "total nifree");
        assert_eq!(fs.fs_cstotal.cs_ndir.get(), td, "total ndir");
    }

    /// The super-block's `fs_clean` in an image.
    pub(crate) fn clean(disk: &[u8], ufs2: bool) -> i8 {
        let loc = if ufs2 { SBLOCK_UFS2 } else { SBLOCK_UFS1 } as usize;
        disk[loc + core::mem::offset_of!(Fs, fs_clean)] as i8
    }
}

/// The disk the strategy below reads and writes.
pub(crate) static DISK: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

/// `diskvn`'s strategy: a synchronous transfer between the buffer and the image at
/// `b_blkno`, then `biodone`.
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

/// The fake disk's fsync: `vflushbuf`, as `spec_fsync` does.
fn disk_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    vflushbuf(ap.a_vp, ap.a_waitfor == MNT_WAIT);
    Ok(())
}

fn disk_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    VOP_UNLOCK(ap.a_vp)
}

/// The operations of the fake disk's block device vnode: the device switch's open, close
/// and strategy over `DISK`.
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

/// The device number of the fake disk (`rd0a`-like).
const DISKDEV: i32 = makedev(17, 0);

/// Memory, the vfs and a fresh buffer cache, the image as the disk, and the thread as
/// `curproc`.
pub(crate) fn setup(image: Vec<u8>) -> (MutexGuard<'static, ()>, &'static Proc) {
    let (g, p) = crate::kern::vfs_subr::tests::setup();
    Machine::set_curproc(Machine::curcpu(), p);
    // No resource limits (write(2) checks RLIMIT_FSIZE).
    let limit: &'static crate::sys::resourcevar::Plimit =
        std::boxed::Box::leak(std::boxed::Box::new(crate::sys::resourcevar::Plimit::new()));
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
    (g, p)
}

pub(crate) fn teardown() {
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// The fake disk's block device vnode, referenced.
fn diskvp() -> &'static Vnode {
    let vp = bdevvp(DISKDEV).unwrap().unwrap();
    vp.v_op.set(Some(&DISK_VOPS));
    vp
}

/// Mounts the disk at `/` the way `ffs_mount` and `main` do: `ffs_mountfs`, the mount list,
/// the root vnode and the thread's current directory.
pub(crate) fn mount_root(p: &'static Proc, ronly: bool) -> &'static Mount {
    let devvp = diskvp();
    let mp = vfs_mount_alloc(None, vfs_byname(b"ffs").unwrap());
    if ronly {
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
    }
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
pub(crate) fn unmount_root(p: &'static Proc, mp: &'static Mount) {
    if let Some(cdir) = p.fd().fd_cdir.take() {
        vrele(cdir);
    }
    if let Some(root) = crate::kern::vfs_init::rootvnode() {
        set_rootvnode(None);
        vrele(root);
    }
    vfs_busy(mp, VB_WRITE | VB_WAIT).unwrap();
    dounmount(mp, 0, p).unwrap();
}

/// A system call with up to six arguments; `retval[0]`.
pub(crate) fn sys(f: SyCall, p: &Proc, args: &[usize]) -> Result<isize, Errno> {
    let mut v: SysArgs = [0; 6];
    for (slot, a) in v.iter_mut().zip(args) {
        *slot = *a as Register;
    }
    let mut rv = [0; 2];
    f(p, &v, &mut rv)?;
    Ok(rv[0])
}

/// A NUL-terminated path as a "user" address (the host's copyin reads it directly).
pub(crate) fn path(s: &'static [u8]) -> usize {
    assert_eq!(s.last(), Some(&0));
    s.as_ptr() as usize
}

/// The whole contents of the file at `name`.
pub(crate) fn read_file(p: &Proc, name: &'static [u8]) -> Result<Vec<u8>, Errno> {
    let fd = sys(sys_open, p, &[path(name), O_RDONLY as usize, 0])?;
    let mut out = Vec::new();
    let mut buf = vec![0u8; 3000];
    loop {
        let n = sys(
            sys_read,
            p,
            &[fd as usize, buf.as_mut_ptr() as usize, buf.len()],
        )?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    sys(sys_close, p, &[fd as usize])?;
    Ok(out)
}

/// Creates (or truncates nothing: the file must be new) `name` with `data`.
fn write_file(p: &Proc, name: &'static [u8], data: &[u8]) -> Result<(), Errno> {
    let fd = sys(
        sys_open,
        p,
        &[path(name), (O_RDWR | O_CREAT) as usize, 0o644],
    )?;
    // Write in uneven pieces, to cross block and fragment boundaries.
    let mut off = 0;
    for chunk in data.chunks(7001) {
        let n = sys(
            sys_write,
            p,
            &[fd as usize, chunk.as_ptr() as usize, chunk.len()],
        )?;
        assert_eq!(n as usize, chunk.len());
        off += chunk.len();
    }
    assert_eq!(off, data.len());
    sys(sys_fsync, p, &[fd as usize])?;
    sys(sys_close, p, &[fd as usize])?;
    Ok(())
}

/// The names in the directory `name` (without `.` and `..`), sorted.
fn list_dir(p: &Proc, name: &'static [u8]) -> Vec<Vec<u8>> {
    let fd = sys(sys_open, p, &[path(name), O_RDONLY as usize, 0]).unwrap();
    let mut buf = vec![0u8; 4096];
    let mut names = Vec::new();
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
                names.push(nm.to_vec());
            }
            off += reclen;
        }
    }
    sys(sys_close, p, &[fd as usize]).unwrap();
    names.sort();
    names
}

/// A pattern of `n` bytes that differs block to block.
fn pattern(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed) ^ (i >> 9) as u8)
        .collect()
}

#[test]
fn newfs_images_are_consistent() {
    for params in [newfs::FFS2_4M, newfs::FFS1_4M] {
        let mut img = newfs::Image::new(params);
        img.add_file(b"motd", b"hello, world\n");
        img.add_file(b"big", &pattern(30000, 1));
        let disk = img.finish();
        newfs::check(&disk, params.ufs2);
        assert_eq!(newfs::clean(&disk, params.ufs2), 1);
    }
}

#[test]
fn ffs2_mount_read_write_and_remount() {
    let mut img = newfs::Image::new(newfs::FFS2_4M);
    img.add_file(b"motd", b"hello, world\n");
    let big = pattern(30000, 1);
    img.add_file(b"big", &big);
    let (_g, p) = setup(img.finish());

    let mp = mount_root(p, false);
    let fs = vfstoufs(mp).fs();
    assert_eq!(fs.fs_magic.get(), FS_UFS2_MAGIC);
    assert_eq!(
        fs.fs_clean.get(),
        0,
        "a read-write mount marks the file system dirty"
    );
    assert_eq!(newfs::clean(&DISK.lock().unwrap(), true), 0);

    // Reading what newfs wrote.
    assert_eq!(read_file(p, b"/motd\0").unwrap(), b"hello, world\n");
    assert_eq!(read_file(p, b"/big\0").unwrap(), big);
    assert_eq!(read_file(p, b"/nothere\0"), Err(Errno::ENOENT));

    // A file past the direct blocks (an indirect block), a directory with a file in it,
    // renames, removals and a symbolic link.
    let large = pattern(12 * 8192 + 20000, 7);
    write_file(p, b"/large\0", &large).unwrap();
    sys(sys_mkdir, p, &[path(b"/dir\0"), 0o755]).unwrap();
    write_file(p, b"/dir/f\0", b"in a directory\n").unwrap();
    sys(sys_rename, p, &[path(b"/dir/f\0"), path(b"/dir/g\0")]).unwrap();
    sys(sys_mkdir, p, &[path(b"/dir/sub\0"), 0o755]).unwrap();
    assert_eq!(sys(sys_rmdir, p, &[path(b"/dir\0")]), Err(Errno::ENOTEMPTY));
    sys(sys_rmdir, p, &[path(b"/dir/sub\0")]).unwrap();
    sys(sys_unlink, p, &[path(b"/motd\0")]).unwrap();
    sys(sys_symlink, p, &[path(b"/dir/g\0"), path(b"/lnk\0")]).unwrap();
    let mut lbuf = [0u8; 64];
    let n = sys(
        sys_readlink,
        p,
        &[path(b"/lnk\0"), lbuf.as_mut_ptr() as usize, lbuf.len()],
    )
    .unwrap();
    assert_eq!(&lbuf[..n as usize], b"/dir/g");
    assert_eq!(read_file(p, b"/lnk\0").unwrap(), b"in a directory\n");
    assert_eq!(read_file(p, b"/large\0").unwrap(), large);
    assert_eq!(
        list_dir(p, b"/\0"),
        vec![
            b"big".to_vec(),
            b"dir".to_vec(),
            b"large".to_vec(),
            b"lnk".to_vec()
        ]
    );
    assert_eq!(list_dir(p, b"/dir\0"), vec![b"g".to_vec()]);

    // sync(2), unmount: the image is clean and its counters agree with its maps.
    sys(sys_sync, p, &[]).unwrap();
    unmount_root(p, mp);
    {
        let disk = DISK.lock().unwrap();
        assert_eq!(newfs::clean(&disk, true), 1);
        newfs::check(&disk, true);
    }

    // Mount it again: everything is where it was left.
    let mp = mount_root(p, true);
    assert_eq!(read_file(p, b"/large\0").unwrap(), large);
    assert_eq!(read_file(p, b"/big\0").unwrap(), big);
    assert_eq!(read_file(p, b"/dir/g\0").unwrap(), b"in a directory\n");
    assert_eq!(read_file(p, b"/motd\0"), Err(Errno::ENOENT));
    assert_eq!(read_file(p, b"/lnk\0").unwrap(), b"in a directory\n");
    // A read-only mount refuses to create.
    assert_eq!(write_file(p, b"/new\0", b"x"), Err(Errno::EROFS));

    // Remove everything on a read-write mount: the space comes back.
    unmount_root(p, mp);
    let mp = mount_root(p, false);
    let fs = vfstoufs(mp).fs();
    sys(sys_unlink, p, &[path(b"/large\0")]).unwrap();
    sys(sys_unlink, p, &[path(b"/lnk\0")]).unwrap();
    sys(sys_unlink, p, &[path(b"/dir/g\0")]).unwrap();
    sys(sys_rmdir, p, &[path(b"/dir\0")]).unwrap();
    sys(sys_unlink, p, &[path(b"/big\0")]).unwrap();
    assert_eq!(list_dir(p, b"/\0"), Vec::<Vec<u8>>::new());
    sys(sys_sync, p, &[]).unwrap();
    let ndir = fs.fs_cstotal.cs_ndir.get();
    unmount_root(p, mp);
    let disk = DISK.lock().unwrap();
    newfs::check(&disk, true);
    assert_eq!(ndir, 1, "only the root directory is left");
    drop(disk);
    teardown();
}

#[test]
fn ffs1_mount_read_write_and_remount() {
    let mut img = newfs::Image::new(newfs::FFS1_4M);
    img.add_file(b"motd", b"FFS1, as makefs builds the ramdisks\n");
    let (_g, p) = setup(img.finish());

    let mp = mount_root(p, false);
    let ump = vfstoufs(mp);
    assert_eq!(ump.um_fstype.get(), crate::ufs::ufs::ufsmount::UM_UFS1);
    assert_eq!(
        ump.fs().fs_sblockloc.get(),
        i64::from(crate::ufs::ffs::fs::SBLOCK_UFS1)
    );
    assert_eq!(
        read_file(p, b"/motd\0").unwrap(),
        b"FFS1, as makefs builds the ramdisks\n"
    );
    // Small file (fragments), then one past the direct blocks (12 * 4 KB).
    write_file(p, b"/small\0", b"tiny").unwrap();
    let large = pattern(12 * 4096 + 5000, 3);
    write_file(p, b"/large\0", &large).unwrap();
    sys(sys_mkdir, p, &[path(b"/etc\0"), 0o755]).unwrap();
    write_file(p, b"/etc/rc\0", b"#!/bin/sh\n").unwrap();
    sys(sys_sync, p, &[]).unwrap();
    unmount_root(p, mp);
    {
        let disk = DISK.lock().unwrap();
        newfs::check(&disk, false);
        assert_eq!(newfs::clean(&disk, false), 1);
    }

    let mp = mount_root(p, true);
    assert_eq!(read_file(p, b"/small\0").unwrap(), b"tiny");
    assert_eq!(read_file(p, b"/large\0").unwrap(), large);
    assert_eq!(read_file(p, b"/etc/rc\0").unwrap(), b"#!/bin/sh\n");
    unmount_root(p, mp);
    teardown();
}

#[test]
fn ufs_getlbns_finds_the_indirect_path() {
    let (_g, p) = setup(newfs::Image::new(newfs::FFS2_4M).finish());
    let mp = mount_root(p, true);
    let root = crate::kern::vfs_init::rootvnode().unwrap();
    let nindir = vfstoufs(mp).um_nindir.get() as i64; // 1024
    let mut a = [crate::ufs::ufs::inode::Indir::default(); NIADDR + 2];
    let mut num = -1;
    crate::ufs::ufs::ufs_bmap::ufs_getlbns(root, 5, &mut a, Some(&mut num)).unwrap();
    assert_eq!(num, 0, "a direct block");
    crate::ufs::ufs::ufs_bmap::ufs_getlbns(root, NDADDR as i64, &mut a, Some(&mut num)).unwrap();
    assert_eq!(num, 2);
    assert_eq!((a[0].in_lbn, a[0].in_off), (-(NDADDR as i64), 0));
    assert_eq!((a[1].in_lbn, a[1].in_off), (-(NDADDR as i64), 0));
    let lbn = NDADDR as i64 + nindir + 3;
    crate::ufs::ufs::ufs_bmap::ufs_getlbns(root, lbn, &mut a, Some(&mut num)).unwrap();
    assert_eq!(num, 3, "a double indirect path");
    assert_eq!(a[0].in_off, 1);
    assert_eq!(a[2].in_off, 3);
    unmount_root(p, mp);
    teardown();
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn fs_h_constants_match_the_c_header() {
    use crate::ufs::ffs::fs::*;
    let defs = crate::reftest::defines("sys/ufs/ffs/fs.h");
    for (name, value) in [
        ("BBSIZE", BBSIZE as i64),
        ("SBSIZE", SBSIZE as i64),
        ("SBLOCK_UFS1", i64::from(SBLOCK_UFS1)),
        ("SBLOCK_UFS2", i64::from(SBLOCK_UFS2)),
        ("SBLOCK_PIGGY", i64::from(SBLOCK_PIGGY)),
        ("SBLOCKSIZE", SBLOCKSIZE as i64),
        ("MAXFRAG", MAXFRAG as i64),
        ("MINBSIZE", MINBSIZE as i64),
        ("MAXMNTLEN", MAXMNTLEN as i64),
        ("MAXVOLLEN", MAXVOLLEN as i64),
        ("FS_MAXCONTIG", FS_MAXCONTIG as i64),
        ("MINFREE", i64::from(MINFREE)),
        ("AVFILESIZ", i64::from(AVFILESIZ)),
        ("AFPDIR", i64::from(AFPDIR)),
        ("FSMAXSNAP", FSMAXSNAP as i64),
        ("FS_MAGIC", i64::from(FS_MAGIC)),
        ("FS_UFS1_MAGIC", i64::from(FS_UFS1_MAGIC)),
        ("FS_UFS2_MAGIC", i64::from(FS_UFS2_MAGIC)),
        ("FS_OKAY", i64::from(FS_OKAY)),
        ("FS_44INODEFMT", i64::from(FS_44INODEFMT)),
        ("FS_ISCLEAN", i64::from(FS_ISCLEAN)),
        ("FS_WASCLEAN", i64::from(FS_WASCLEAN)),
        ("FS_OPTTIME", i64::from(FS_OPTTIME)),
        ("FS_OPTSPACE", i64::from(FS_OPTSPACE)),
        ("FS_UNCLEAN", i64::from(FS_UNCLEAN)),
        ("FS_FLAGS_UPDATED", i64::from(FS_FLAGS_UPDATED)),
        ("FS_DYNAMICPOSTBLFMT", i64::from(FS_DYNAMICPOSTBLFMT)),
        ("CG_MAGIC", i64::from(CG_MAGIC)),
    ] {
        assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
    }
    // The negative ones are written `-1`.
    assert_eq!(defs.get("FS_42INODEFMT").map(|s| s.as_str()), Some("-1"));
    assert_eq!(defs.get("FS_42POSTBLFMT").map(|s| s.as_str()), Some("-1"));
    let ufsmount = crate::reftest::defines("sys/ufs/ufs/ufsmount.h");
    assert_eq!(crate::reftest::int(&ufsmount, "UM_UFS1"), Some(1));
    assert_eq!(crate::reftest::int(&ufsmount, "UM_UFS2"), Some(2));
}
