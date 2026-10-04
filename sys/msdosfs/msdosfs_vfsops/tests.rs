//! Host tests for the msdos file system's mounting: `mkfat`, a small `newfs_msdos(8)`
//! equivalent that lays out a FAT12, FAT16 or FAT32 image in memory (boot sector, FSInfo,
//! FATs, root directory, files and subdirectories with short and Win95 long names), a block
//! device vnode whose strategy reads and writes that image, and tests that mount it with
//! `msdosfs_mountfs`, check the geometry it worked out and `statfs`, reject bad boot
//! sectors, and unmount. `msdosfs_lookup.rs`'s tests use the same image and disk.

use core::ptr;
use core::sync::atomic::Ordering;
use std::sync::MutexGuard;
use std::vec::Vec;

use super::*;
use crate::kern::subr_xxx::nullop;
use crate::kern::vfs_bio::{BCSTATS, BUFHEAD, BUFKVM, CLEANCACHE, biodone, bufinit};
use crate::kern::vfs_default::vop_generic_bwrite;
use crate::kern::vfs_init::vfs_byname;
use crate::kern::vfs_subr::{MOUNTLIST, bdevvp, vflushbuf, vfs_busy, vfs_mount_alloc, vfs_unbusy};
use crate::kern::vfs_syscalls::dounmount;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::msdosfs::fat::fat16;
use crate::sys::buf::{B_ERROR, B_READ};
use crate::sys::mount::{MOUNT_MSDOS, VB_WAIT, VB_WRITE};
use crate::sys::types::makedev;
use crate::sys::vnode::{VopFsyncArgs, VopInactiveArgs, VopStrategyArgs, Vops};

/// `mkfat`: a FAT image built in memory the way `newfs_msdos(8)` and `makefs -t msdos` build
/// one, for the tests and as a record of the on-disk layout.
pub(crate) mod mkfat {
    use std::vec;
    use std::vec::Vec;

    /// The geometry to build.
    #[derive(Clone, Copy)]
    pub(crate) struct Params {
        /// 12, 16 or 32: the FAT width (FAT32 means no fixed root directory).
        pub(crate) fat: u32,
        /// The image's size in bytes.
        pub(crate) size: usize,
        /// Bytes per sector.
        pub(crate) bps: u16,
        /// Sectors per cluster.
        pub(crate) spc: u8,
        /// Root directory entries (0 for FAT32).
        pub(crate) rde: u16,
    }

    /// FAT12, 1 MB, 512-byte clusters, 64 root directory entries.
    pub(crate) const FAT12_1M: Params = Params {
        fat: 12,
        size: 1 << 20,
        bps: 512,
        spc: 1,
        rde: 64,
    };

    /// FAT16, 4 MB, 512-byte clusters, 512 root directory entries.
    pub(crate) const FAT16_4M: Params = Params {
        fat: 16,
        size: 4 << 20,
        bps: 512,
        spc: 1,
        rde: 512,
    };

    /// FAT32, 1 MB, 512-byte clusters (a FAT32 the kernel accepts: the type comes from the
    /// missing root directory, not from the cluster count).
    pub(crate) const FAT32_1M: Params = Params {
        fat: 32,
        size: 1 << 20,
        bps: 512,
        spc: 1,
        rde: 0,
    };

    /// Where a directory is: the fixed root directory of FAT12/16, or a cluster chain.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Dir {
        /// The FAT12/16 root directory.
        Root,
        /// The directory whose first cluster is this one.
        Clust(u32),
    }

    /// An image under construction.
    pub(crate) struct Image {
        /// The disk.
        pub(crate) disk: Vec<u8>,
        /// The geometry.
        pub(crate) p: Params,
        /// Reserved sectors.
        pub(crate) res: u32,
        /// Sectors per FAT.
        pub(crate) spf: u32,
        /// Sectors of the root directory (FAT12/16).
        pub(crate) rootsecs: u32,
        /// Data clusters.
        pub(crate) nclusters: u32,
        /// The next cluster handed out.
        next: u32,
    }

    /// The Win95 checksum of a DOS name.
    pub(crate) fn chksum(name: &[u8; 11]) -> u8 {
        name.iter().fold(0u8, |s, &c| {
            ((s & 1) << 7).wrapping_add(s >> 1).wrapping_add(c)
        })
    }

    /// The Win95 long name entries of `name` for the DOS name `short`, in on-disk order (the
    /// last part first).
    pub(crate) fn lfn_entries(name: &[u8], short: &[u8; 11]) -> Vec<[u8; 32]> {
        let ck = chksum(short);
        let n = name.len().div_ceil(13);
        let mut out = Vec::new();
        for i in (1..=n).rev() {
            let mut e = [0u8; 32];
            e[0] = i as u8 | if i == n { 0x40 } else { 0 };
            e[11] = 0x0f;
            e[13] = ck;
            let offs = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
            for (k, &o) in offs.iter().enumerate() {
                let idx = (i - 1) * 13 + k;
                let c: u16 = match idx.cmp(&name.len()) {
                    core::cmp::Ordering::Less => u16::from(name[idx]),
                    core::cmp::Ordering::Equal => 0,
                    core::cmp::Ordering::Greater => 0xffff,
                };
                e[o..o + 2].copy_from_slice(&c.to_le_bytes());
            }
            out.push(e);
        }
        out
    }

    /// A short directory entry.
    pub(crate) fn short_entry(short: &[u8; 11], attr: u8, cluster: u32, size: u32) -> [u8; 32] {
        let mut e = [0u8; 32];
        e[..11].copy_from_slice(short);
        e[11] = attr;
        e[20..22].copy_from_slice(&((cluster >> 16) as u16).to_le_bytes());
        e[24..26].copy_from_slice(&0x21u16.to_le_bytes()); // 1980-01-01
        e[26..28].copy_from_slice(&(cluster as u16).to_le_bytes());
        e[28..32].copy_from_slice(&size.to_le_bytes());
        e
    }

    impl Image {
        /// `newfs_msdos`: an empty file system.
        pub(crate) fn new(p: Params) -> Self {
            let bps = u32::from(p.bps);
            let sectors = (p.size / p.bps as usize) as u32;
            let res = if p.fat == 32 { 32 } else { 1 };
            let rootsecs = (u32::from(p.rde) * 32).div_ceil(bps);
            let mut spf = 1;
            let nclusters = loop {
                let data = sectors - res - 2 * spf - rootsecs;
                let ncl = data / u32::from(p.spc);
                let need = ((ncl + 2) * p.fat).div_ceil(8).div_ceil(bps);
                if need <= spf {
                    break ncl;
                }
                spf = need;
            };
            let mut img = Image {
                disk: vec![0u8; p.size],
                p,
                res,
                spf,
                rootsecs,
                nclusters,
                next: 2,
            };

            let d = &mut img.disk;
            d[0..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
            d[3..11].copy_from_slice(b"EMIBSD  ");
            d[11..13].copy_from_slice(&p.bps.to_le_bytes());
            d[13] = p.spc;
            d[14..16].copy_from_slice(&(res as u16).to_le_bytes());
            d[16] = 2;
            d[17..19].copy_from_slice(&p.rde.to_le_bytes());
            if sectors < 65536 && p.fat != 32 {
                d[19..21].copy_from_slice(&(sectors as u16).to_le_bytes());
            } else {
                d[32..36].copy_from_slice(&sectors.to_le_bytes());
            }
            d[21] = 0xf8;
            if p.fat != 32 {
                d[22..24].copy_from_slice(&(spf as u16).to_le_bytes());
            }
            d[24..26].copy_from_slice(&63u16.to_le_bytes());
            d[26..28].copy_from_slice(&255u16.to_le_bytes());
            if p.fat == 32 {
                d[36..40].copy_from_slice(&spf.to_le_bytes());
                d[44..48].copy_from_slice(&2u32.to_le_bytes()); // root cluster
                d[48..50].copy_from_slice(&1u16.to_le_bytes()); // FSInfo sector
                d[50..52].copy_from_slice(&6u16.to_le_bytes()); // backup boot sector
                d[66] = 0x29;
                d[82..90].copy_from_slice(b"FAT32   ");
                // FSInfo, 1024 bytes from sector 1.
                let fsi = bps as usize;
                d[fsi..fsi + 4].copy_from_slice(b"RRaA");
                d[fsi + 484..fsi + 488].copy_from_slice(b"rrAa");
                d[fsi + 488..fsi + 492].copy_from_slice(&u32::MAX.to_le_bytes());
                d[fsi + 492..fsi + 496].copy_from_slice(&u32::MAX.to_le_bytes());
                d[fsi + 508..fsi + 512].copy_from_slice(&[0, 0, 0x55, 0xaa]);
                d[fsi + 1020..fsi + 1024].copy_from_slice(&[0, 0, 0x55, 0xaa]);
            } else {
                d[38] = 0x29;
                d[54..62].copy_from_slice(if p.fat == 12 {
                    b"FAT12   "
                } else {
                    b"FAT16   "
                });
            }
            d[510] = 0x55;
            d[511] = 0xaa;

            let mask = img.mask();
            img.fat_set(0, 0xffff_fff8 & mask);
            img.fat_set(1, mask);
            if p.fat == 32 {
                let root = img.alloc(1);
                assert_eq!(root, 2);
            }
            img
        }

        /// The FAT entry mask.
        pub(crate) fn mask(&self) -> u32 {
            match self.p.fat {
                12 => 0xfff,
                16 => 0xffff,
                _ => 0x0fff_ffff,
            }
        }

        /// Bytes per cluster.
        pub(crate) fn bpc(&self) -> usize {
            usize::from(self.p.bps) * usize::from(self.p.spc)
        }

        /// The byte offset of the first data cluster.
        fn data_off(&self) -> usize {
            (self.res + 2 * self.spf + self.rootsecs) as usize * usize::from(self.p.bps)
        }

        /// The byte offset of cluster `cn`.
        pub(crate) fn clust_off(&self, cn: u32) -> usize {
            self.data_off() + (cn as usize - 2) * self.bpc()
        }

        /// The byte offset of the FAT12/16 root directory.
        pub(crate) fn root_off(&self) -> usize {
            (self.res + 2 * self.spf) as usize * usize::from(self.p.bps)
        }

        /// The entry of cluster `cn` in the first FAT.
        pub(crate) fn fat_get(&self, cn: u32) -> u32 {
            let base = self.res as usize * usize::from(self.p.bps);
            match self.p.fat {
                12 => {
                    let o = base + cn as usize * 3 / 2;
                    let v = u16::from_le_bytes([self.disk[o], self.disk[o + 1]]);
                    u32::from(if cn & 1 != 0 { v >> 4 } else { v & 0xfff })
                }
                16 => {
                    let o = base + cn as usize * 2;
                    u32::from(u16::from_le_bytes([self.disk[o], self.disk[o + 1]]))
                }
                _ => {
                    let o = base + cn as usize * 4;
                    u32::from_le_bytes(self.disk[o..o + 4].try_into().unwrap()) & 0x0fff_ffff
                }
            }
        }

        /// Sets the entry of cluster `cn` in both FATs.
        pub(crate) fn fat_set(&mut self, cn: u32, v: u32) {
            for f in 0..2 {
                let base = (self.res + f * self.spf) as usize * usize::from(self.p.bps);
                match self.p.fat {
                    12 => {
                        let o = base + cn as usize * 3 / 2;
                        let old = u16::from_le_bytes([self.disk[o], self.disk[o + 1]]);
                        let v = v as u16 & 0xfff;
                        let new = if cn & 1 != 0 {
                            (old & 0x000f) | (v << 4)
                        } else {
                            (old & 0xf000) | v
                        };
                        self.disk[o..o + 2].copy_from_slice(&new.to_le_bytes());
                    }
                    16 => {
                        let o = base + cn as usize * 2;
                        self.disk[o..o + 2].copy_from_slice(&(v as u16).to_le_bytes());
                    }
                    _ => {
                        let o = base + cn as usize * 4;
                        self.disk[o..o + 4].copy_from_slice(&v.to_le_bytes());
                    }
                }
            }
        }

        /// Allocates a chain of `n` clusters, zeroed; its first cluster.
        pub(crate) fn alloc(&mut self, n: u32) -> u32 {
            let first = self.next;
            for i in 0..n {
                let cn = first + i;
                let v = if i + 1 == n { self.mask() } else { cn + 1 };
                self.fat_set(cn, v);
                let o = self.clust_off(cn);
                let bpc = self.bpc();
                self.disk[o..o + bpc].fill(0);
            }
            self.next += n;
            first
        }

        /// The byte offsets of the slots of `dir`, in order.
        pub(crate) fn slots(&self, dir: Dir) -> Vec<usize> {
            match dir {
                Dir::Root => {
                    let o = self.root_off();
                    (0..usize::from(self.p.rde)).map(|i| o + i * 32).collect()
                }
                Dir::Clust(mut cn) => {
                    let mut v = Vec::new();
                    loop {
                        let o = self.clust_off(cn);
                        v.extend((0..self.bpc() / 32).map(|i| o + i * 32));
                        let next = self.fat_get(cn);
                        if next >= (0x0fff_fff8 & self.mask()) {
                            break v;
                        }
                        cn = next;
                    }
                }
            }
        }

        /// The FAT32 root directory, or the fixed one.
        pub(crate) fn root(&self) -> Dir {
            if self.p.fat == 32 {
                Dir::Clust(2)
            } else {
                Dir::Root
            }
        }

        /// Appends raw entries to `dir`, after its last used slot; the byte offset of the
        /// first one in the directory.
        pub(crate) fn put(&mut self, dir: Dir, entries: &[[u8; 32]]) -> u32 {
            let slots = self.slots(dir);
            let first = slots
                .iter()
                .position(|&o| self.disk[o] == 0)
                .expect("directory full");
            assert!(first + entries.len() <= slots.len(), "directory full");
            for (k, e) in entries.iter().enumerate() {
                let o = slots[first + k];
                self.disk[o..o + 32].copy_from_slice(e);
            }
            (first * 32) as u32
        }

        /// Adds a file: its data in new clusters and its entry (with long name entries when
        /// `long` is given) in `dir`; the directory offset of the short entry.
        pub(crate) fn add_file(
            &mut self,
            dir: Dir,
            long: Option<&[u8]>,
            short: &[u8; 11],
            data: &[u8],
        ) -> u32 {
            let n = data.len().div_ceil(self.bpc()) as u32;
            let cn = if n == 0 { 0 } else { self.alloc(n) };
            for (i, chunk) in data.chunks(self.bpc()).enumerate() {
                let o = self.clust_off(cn + i as u32);
                self.disk[o..o + chunk.len()].copy_from_slice(chunk);
            }
            let mut ents = long.map_or(Vec::new(), |l| lfn_entries(l, short));
            ents.push(short_entry(short, 0x20, cn, data.len() as u32));
            self.put(dir, &ents) + 32 * (ents.len() as u32 - 1)
        }

        /// Adds a directory of `nclust` clusters with its "." and ".." entries; its first
        /// cluster.
        pub(crate) fn mkdir(&mut self, parent: Dir, short: &[u8; 11], nclust: u32) -> u32 {
            let cn = self.alloc(nclust);
            let pcn = match parent {
                Dir::Root => 0,
                Dir::Clust(c) if self.p.fat == 32 && c == 2 => 0,
                Dir::Clust(c) => c,
            };
            let o = self.clust_off(cn);
            self.disk[o..o + 32].copy_from_slice(&short_entry(b".          ", 0x10, cn, 0));
            self.disk[o + 32..o + 64].copy_from_slice(&short_entry(b"..         ", 0x10, pcn, 0));
            self.put(parent, &[short_entry(short, 0x10, cn, 0)]);
            cn
        }

        /// The finished image.
        pub(crate) fn finish(self) -> Vec<u8> {
            self.disk
        }
    }
}

/// The disk the strategy below reads and writes.
pub(crate) static DISK: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

/// The fake disk's strategy: a synchronous transfer between the buffer and the image at
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

/// The operations of the fake disk's block device vnode.
static DISK_VOPS: Vops = Vops {
    vop_open: Some(|_| nullop()),
    vop_close: Some(|_| nullop()),
    vop_ioctl: Some(|_| nullop()),
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

/// The device number of the fake disk (`vnd0c`-like).
const DISKDEV: i32 = makedev(41, 2);

/// Memory, the vfs and a fresh buffer cache, the image as the disk, and the thread as
/// `curproc`.
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

/// Clears `curproc`.
pub(crate) fn teardown() {
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// The fake disk's block device vnode, referenced.
pub(crate) fn diskvp() -> &'static Vnode {
    let vp = bdevvp(DISKDEV).unwrap().unwrap();
    vp.v_op.set(Some(&DISK_VOPS));
    vp
}

/// Zeroed mount arguments.
fn noargs() -> MsdosfsArgs {
    MsdosfsArgs::from_bytes(&[0u8; MsdosfsArgs::SIZE]).unwrap()
}

/// A fresh msdos mount structure, `ronly` or read-write.
fn newmount(ronly: bool) -> &'static Mount {
    let mp = vfs_mount_alloc(None, vfs_byname(MOUNT_MSDOS).unwrap());
    if ronly {
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
    }
    mp
}

/// Mounts the disk the way `msdosfs_mount` does after its argument checks
/// (`msdosfs_mountfs`), and puts it on the mount list.
pub(crate) fn mount(p: &'static Proc, ronly: bool) -> &'static Mount {
    let mp = newmount(ronly);
    msdosfs_mountfs(diskvp(), mp, p, &noargs()).unwrap();
    vfs_unbusy(mp);
    // SAFETY: a new mount on no list.
    unsafe { MOUNTLIST.0.insert_tail(mp) };
    mp
}

/// Unmounts what `mount` mounted (`dounmount`: `msdosfs_sync`, `msdosfs_unmount`).
pub(crate) fn unmount(p: &'static Proc, mp: &'static Mount) {
    vfs_busy(mp, VB_WRITE | VB_WAIT).unwrap();
    dounmount(mp, 0, p).unwrap();
}

/// A FAT image with a file in the root directory and an empty subdirectory.
fn sample(params: mkfat::Params) -> (Vec<u8>, u32) {
    let mut img = mkfat::Image::new(params);
    let root = img.root();
    img.add_file(root, None, b"README  TXT", &[b'r'; 3000]);
    img.mkdir(root, b"SUBDIR     ", 1);
    let used = img.nclusters;
    (img.finish(), used)
}

/// An export list on a mounted FAT file system (`pm_export`): `msdosfs_check_export` answers
/// the listed client and refuses the others.
#[cfg(feature = "nfsserver")]
#[test]
fn an_exported_fat_file_system_answers_check_export() {
    use crate::kern::uipc_mbuf::tests::mbinit_again;
    use crate::kern::vfs_subr::tests::exports::{args, check_export, sin};
    use crate::sys::mount::{MNT_EXPORTED, MNT_EXRDONLY};

    let (disk, _nclusters) = sample(mkfat::FAT12_1M);
    let (_g, p) = setup(disk);
    mbinit_again();
    let mp = mount(p, false);
    let pmp = vfstomsdosfs(mp);
    let ro = MNT_EXPORTED | MNT_EXRDONLY;
    assert_eq!(check_export(mp, [10, 0, 0, 5]), Err(Errno::EACCES));

    let net = sin(2, [10, 0, 0, 0]);
    let mask = sin(2, [255, 255, 255, 0]);
    vfs_export(mp, &pmp.pm_export, &args(ro, 32767, Some(net), Some(mask))).unwrap();
    assert_eq!(check_export(mp, [10, 0, 0, 5]), Ok((ro, 32767)));
    assert_eq!(check_export(mp, [10, 0, 1, 5]), Err(Errno::EACCES));

    unmount(p, mp);
}

#[test]
fn fat12_mountfs_works_out_the_geometry() {
    let (disk, nclusters) = sample(mkfat::FAT12_1M);
    let (_g, p) = setup(disk);

    let mp = mount(p, false);
    let pmp = vfstomsdosfs(mp);
    assert!(fat12(pmp));
    assert_eq!(pmp.pm_fatmask.get(), FAT12_MASK);
    assert_eq!((pmp.pm_fatmult.get(), pmp.pm_fatdiv.get()), (3, 2));
    assert_eq!(pmp.pm_BytesPerSec(), 512);
    assert_eq!(pmp.pm_BlkPerSec.get(), 1);
    assert_eq!(pmp.pm_bpcluster.get(), 512);
    assert_eq!(pmp.pm_crbomask.get(), 511);
    assert_eq!(pmp.pm_cnshift.get(), 9);
    assert_eq!(pmp.pm_bnshift.get(), 9);
    assert_eq!(pmp.pm_fatblk.get(), 1);
    // 1 reserved sector and two FATs of 6 sectors; 64 entries are 4 sectors.
    assert_eq!(pmp.pm_FATsecs.get(), 6);
    assert_eq!(pmp.pm_rootdirblk.get(), 13);
    assert_eq!(pmp.pm_rootdirsize.get(), 4);
    assert_eq!(pmp.pm_firstcluster.get(), 17);
    assert_eq!(pmp.pm_nmbrofclusters.get(), nclusters);
    assert_eq!(pmp.pm_nmbrofclusters.get(), 2031);
    assert_eq!(pmp.pm_maxcluster.get(), 2032);
    assert_eq!(pmp.pm_fatblocksize.get(), 3 * 512);
    assert_eq!(pmp.pm_fatsize.get(), 6 * 512);
    assert_eq!(pmp.pm_fsinfo.get(), 0);
    assert_ne!(pmp.pm_flags.get() & MSDOSFS_FATMIRROR, 0);
    assert_eq!(pmp.pm_flags.get() & MSDOSFSMNT_RONLY, 0);
    assert_eq!(pmp.pm_fmod.get(), 1);
    // README.TXT has 6 clusters, SUBDIR 1.
    assert_eq!(pmp.pm_freeclustercount.get(), 2031 - 7);
    assert_eq!(pmp.inusemap().len(), 2033usize.div_ceil(32));
    assert!(ptr::eq(pmp.mountp(), mp));
    assert_eq!(pmp.pm_dev.get(), DISKDEV);
    let st = mp.mnt_stat.get();
    assert_eq!(st.f_fsid.val, [DISKDEV, 4]);
    let devvp = pmp.devvp();
    assert!(
        devvp
            .v_specinfo()
            .and_then(|si| si.si_mountpoint.get())
            .is_some_and(|m| ptr::eq(m, mp))
    );

    let mut sb = mp.mnt_stat.get();
    msdosfs_statfs(mp, &mut sb, p).unwrap();
    assert_eq!(sb.f_bsize, 512);
    assert_eq!(sb.f_iosize, 512);
    assert_eq!(sb.f_blocks, 2031);
    assert_eq!(sb.f_bfree, 2024);
    assert_eq!(sb.f_bavail, 2024);
    assert_eq!(sb.f_files, 64);
    assert_eq!(sb.f_ffree, 0);

    // The root vnode needs the vnode operations of msdosfs_vnops.c (deget locks it), which
    // are not here yet; sync and unmount do not.
    msdosfs_sync(mp, MNT_WAIT, 0, p.p_ucred.get(), p).unwrap();
    unmount(p, mp);
    assert!(devvp.v_specinfo().unwrap().si_mountpoint.get().is_none());
    teardown();
}

#[test]
fn fat16_mount_read_only() {
    let (disk, nclusters) = sample(mkfat::FAT16_4M);
    let (_g, p) = setup(disk);
    let mp = mount(p, true);
    let pmp = vfstomsdosfs(mp);
    assert!(fat16(pmp));
    assert_eq!((pmp.pm_fatmult.get(), pmp.pm_fatdiv.get()), (2, 1));
    assert_eq!(pmp.pm_fatblocksize.get(), MAXBSIZE as u32);
    assert_eq!(pmp.pm_rootdirsize.get(), 32);
    assert_eq!(pmp.pm_nmbrofclusters.get(), nclusters);
    assert_ne!(pmp.pm_flags.get() & MSDOSFSMNT_RONLY, 0);
    assert_eq!(pmp.pm_fmod.get(), 0);
    unmount(p, mp);
    teardown();
}

#[test]
fn fat32_mount() {
    let (disk, nclusters) = sample(mkfat::FAT32_1M);
    let (_g, p) = setup(disk);
    let mp = mount(p, false);
    let pmp = vfstomsdosfs(mp);
    assert!(fat32(pmp));
    assert_eq!(pmp.pm_fatmask.get(), FAT32_MASK);
    assert_eq!(pmp.pm_rootdirblk.get(), 2);
    assert_eq!(pmp.pm_rootdirsize.get(), 0);
    assert_eq!(
        pmp.pm_firstcluster.get(),
        pmp.pm_fatblk.get() + 2 * pmp.pm_FATsecs.get()
    );
    assert_eq!(pmp.pm_fatblk.get(), 32);
    assert_eq!(pmp.pm_fsinfo.get(), 1, "a valid FSInfo block is kept");
    assert_eq!(pmp.pm_curfat.get(), 0);
    assert_ne!(pmp.pm_flags.get() & MSDOSFS_FATMIRROR, 0);
    assert_eq!(pmp.pm_nmbrofclusters.get(), nclusters);
    // The root directory, README.TXT and SUBDIR.
    assert_eq!(pmp.pm_freeclustercount.get(), nclusters - 8);
    unmount(p, mp);
    teardown();
}

#[test]
fn fat32_fsinfo_and_active_fat() {
    let (mut disk, _) = sample(mkfat::FAT32_1M);
    // A bad FSInfo signature is ignored, and FAT 1 is the only active one.
    disk[512 + 484] = b'X';
    disk[40..42].copy_from_slice(&0x0081u16.to_le_bytes());
    let (_g, p) = setup(disk);
    let mp = mount(p, false);
    let pmp = vfstomsdosfs(mp);
    assert_eq!(pmp.pm_fsinfo.get(), 0);
    assert_eq!(pmp.pm_curfat.get(), 1);
    assert_eq!(pmp.pm_flags.get() & MSDOSFS_FATMIRROR, 0);
    unmount(p, mp);
    teardown();
}

#[test]
fn mountfs_rejects_bad_boot_sectors() {
    let (good, _) = sample(mkfat::FAT12_1M);
    let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
    let mut d = good.clone();
    d[11..13].copy_from_slice(&0u16.to_le_bytes());
    cases.push(("no bytes per sector", d));
    let mut d = good.clone();
    d[13] = 0;
    cases.push(("no sectors per cluster", d));
    let mut d = good.clone();
    d[13] = 3;
    cases.push(("sectors per cluster not a power of 2", d));
    let mut d = good.clone();
    d[11..13].copy_from_slice(&768u16.to_le_bytes());
    cases.push(("sector size not a power of 2", d));
    let mut d = good.clone();
    d[11..13].copy_from_slice(&256u16.to_le_bytes());
    cases.push(("sector smaller than DEV_BSIZE", d));
    let mut d = good.clone();
    d[22..24].copy_from_slice(&0u16.to_le_bytes());
    cases.push(("no FAT sectors", d));
    let mut d = good.clone();
    d[13] = 0x80;
    d[11..13].copy_from_slice(&1024u16.to_le_bytes());
    cases.push(("clusters larger than MAXBSIZE", d));
    let mut d = good.clone();
    d[17..19].copy_from_slice(&0u16.to_le_bytes());
    cases.push(("FAT32 with 16-bit sector counts", d));

    let (g, p) = setup(good.clone());
    let devvp = diskvp();
    for (what, disk) in cases {
        *DISK.lock().unwrap() = disk;
        let mp = newmount(false);
        assert_eq!(
            msdosfs_mountfs(devvp, mp, p, &noargs()),
            Err(Errno::EINVAL),
            "{what}"
        );
        assert!(mp.mnt_data.get().is_null(), "{what}");
        assert!(devvp.v_specinfo().unwrap().si_mountpoint.get().is_none());
        vfs_unbusy(mp);
    }

    // The device is still usable.
    *DISK.lock().unwrap() = good;
    let mp = newmount(false);
    msdosfs_mountfs(devvp, mp, p, &noargs()).unwrap();
    assert_eq!(vfstomsdosfs(mp).pm_nmbrofclusters.get(), 2031);
    vfs_unbusy(mp);
    // SAFETY: a new mount on no list.
    unsafe { MOUNTLIST.0.insert_tail(mp) };
    unmount(p, mp);
    teardown();
    drop(g);
}

#[test]
fn mkfat_long_names_match_the_kernel() {
    // The builder's long name entries are what unix2winfn writes.
    let name = b"m10c-fat.txt";
    let short = *b"M10C-FATTXT";
    let ents = mkfat::lfn_entries(name, &short);
    assert_eq!(ents.len(), 1);
    assert_eq!(
        mkfat::chksum(&short),
        crate::msdosfs::msdosfs_conv::winChksum(&short)
    );
    let mut we = [0u8; 32];
    let wep = crate::msdosfs::direntry::Winentry::at_mut(&mut we, 0);
    let more = crate::msdosfs::msdosfs_conv::unix2winfn(name, wep, 1, mkfat::chksum(&short));
    assert_eq!(more, 0);
    assert_eq!(we, ents[0]);
}
