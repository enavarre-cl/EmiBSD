//! Host tests for UDF: a small UDF image built in memory (anchor, volume descriptor
//! sequence, partition, file set descriptor, file entries and file identifier descriptors,
//! the layout `hdiutil makehybrid -udf` writes), a block device vnode whose strategy reads
//! it, and tests that mount it with `udf_mountfs`, look names up, read files (one through a
//! short allocation descriptor, one recorded in its extended file entry), read directories
//! (one whose descriptors straddle its extents), map blocks, take attributes, spoof a disk
//! label and unmount; plus the pure helpers (tags, times, modes, names).

use core::mem::offset_of;
use core::sync::atomic::Ordering;
use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::isofs::udf::ecma167_udf::{
    AnchorVdp, DescTag, FilesetDesc, LogvolDesc, PartDesc, PriVolDesc, TAGID_ANCHOR, TAGID_FENTRY,
    TAGID_FSD, TAGID_LOGVOL, TAGID_PARTITION, TAGID_PRI_VOL, TAGID_TERM, UDF_FILE_CHAR_DIR,
};
use crate::isofs::udf::udf::VFSTOUDFFS;
use crate::isofs::udf::udf_subr::udf_disklabelspoof;
use crate::isofs::udf::udf_vfsops::{udf_mountfs, udf_root, udf_unmount};
use crate::kern::subr_xxx::nullop;
use crate::kern::vfs_bio::{BCSTATS, BUFHEAD, BUFKVM, CLEANCACHE, bufinit};
use crate::kern::vfs_init::vfs_byname;
use crate::kern::vfs_subr::{bdevvp, vflushbuf, vfs_mount_alloc, vfs_mount_free};
use crate::kern::vfs_vops::{VOP_GETATTR, VOP_LOOKUP, VOP_READ, VOP_READDIR};
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::sys::buf::B_READ;
use crate::sys::disklabel::{DISKMAGIC, Disklabel, FS_UDF, RAW_PART, dl_getpsize, dl_setdsize};
use crate::sys::mount::{MNT_LOCAL, MNT_RDONLY, MNT_WAIT};
use crate::sys::namei::{Componentname, ISDOTDOT};
use crate::sys::param::DEV_BSIZE;
use crate::sys::types::makedev;
use crate::sys::uio::{Iovec, UioRw, UioSeg};
use crate::sys::vnode::{VROOT, Vattr, VopFsyncArgs, VopInactiveArgs};

const B: usize = 2048;
/// The partition's first sector and length.
const PSTART: usize = 260;
const PLEN: usize = 20;

/// The fragmented directory: 60 descriptors of 80 bytes after the parent's 40, over three
/// extents (2048, 2048 and 744 bytes).
const NSUB: usize = 60;
const SUB_SIZE: usize = 40 + NSUB * 80;

/// The image being built, 2048-byte sectors.
struct Img(Vec<u8>);

impl Img {
    fn new() -> Self {
        Img(vec![0; (PSTART + PLEN) * B])
    }

    fn put(&mut self, sector: usize, off: usize, b: &[u8]) {
        let o = sector * B + off;
        self.0[o..o + b.len()].copy_from_slice(b);
    }

    fn u16(&mut self, sector: usize, off: usize, v: u16) {
        self.put(sector, off, &v.to_le_bytes());
    }

    fn u32(&mut self, sector: usize, off: usize, v: u32) {
        self.put(sector, off, &v.to_le_bytes());
    }

    fn u64(&mut self, sector: usize, off: usize, v: u64) {
        self.put(sector, off, &v.to_le_bytes());
    }

    /// A descriptor tag at the start of `sector`.
    fn tag(&mut self, sector: usize, id: u16, loc: u32) {
        let t = tag_bytes(id, loc);
        self.put(sector, 0, &t);
    }
}

/// A descriptor tag: `id`, version 2, the checksum, `tag_loc`.
fn tag_bytes(id: u16, loc: u32) -> [u8; 16] {
    let mut t = [0u8; 16];
    t[0..2].copy_from_slice(&id.to_le_bytes());
    t[2..4].copy_from_slice(&2u16.to_le_bytes());
    t[12..16].copy_from_slice(&loc.to_le_bytes());
    let sum = t
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != 4)
        .fold(0u8, |s, (_, &b)| s.wrapping_add(b));
    t[4] = sum;
    t
}

/// A timestamp: 2026-10-04 12:34:56, time zone `tz` minutes (type 1 when given).
fn timestamp(tz: Option<i16>) -> [u8; 12] {
    let mut t = [0u8; 12];
    let type_tz: u16 = match tz {
        Some(m) => 0x1000 | (m as u16 & 0x0fff),
        None => 0,
    };
    t[0..2].copy_from_slice(&type_tz.to_le_bytes());
    t[2..4].copy_from_slice(&2026u16.to_le_bytes());
    t[4..12].copy_from_slice(&[10, 4, 12, 34, 56, 0, 0, 0]);
    t
}

/// A file identifier descriptor: `name` (8-bit CS0, empty for the parent), its file entry at
/// `lb`, padded to 4 bytes.
fn fid(file_char: u8, name: &[u8], lb: u32) -> Vec<u8> {
    let l_fi = if name.is_empty() { 0 } else { name.len() + 1 };
    let mut f = vec![0u8; (38 + l_fi + 3) & !3];
    f[16..18].copy_from_slice(&1u16.to_le_bytes());
    f[18] = file_char;
    f[19] = l_fi as u8;
    f[20..24].copy_from_slice(&(B as u32).to_le_bytes());
    f[24..28].copy_from_slice(&lb.to_le_bytes());
    if l_fi != 0 {
        f[38] = 8;
        f[39..39 + name.len()].copy_from_slice(name);
    }
    let t = tag_bytes(TAGID_FID, 0);
    f[..16].copy_from_slice(&t);
    f
}

/// A (extended, `efe`) file entry at partition block `lb`: type, allocation kind (the low
/// bits of the ICB flags), permissions, owner, size and allocation descriptors.
#[allow(clippy::too_many_arguments)]
fn file_entry(
    img: &mut Img,
    lb: usize,
    efe: bool,
    file_type: u8,
    alloc: u16,
    perm: u32,
    uid: u32,
    inf_len: u64,
    ads: &[u8],
) {
    let s = PSTART + lb;
    img.tag(
        s,
        if efe { TAGID_EXTFENTRY } else { TAGID_FENTRY },
        lb as u32,
    );
    let icb = offset_of!(FileEntry, icbtag);
    img.u16(s, icb + 4, 4); // strat_type
    img.u16(s, icb + 8, 1); // max_num_entries
    img.put(s, icb + 11, &[file_type]);
    img.u16(s, icb + 18, alloc);
    img.u32(s, offset_of!(FileEntry, uid), uid);
    img.u32(s, offset_of!(FileEntry, gid), 20);
    img.u32(s, offset_of!(FileEntry, perm), perm);
    img.u16(s, offset_of!(FileEntry, link_cnt), 1);
    img.u64(s, offset_of!(FileEntry, inf_len), inf_len);
    if efe {
        img.put(s, offset_of!(ExtfileEntry, atime), &timestamp(None));
        img.put(s, offset_of!(ExtfileEntry, mtime), &timestamp(Some(-60)));
        img.u32(s, offset_of!(ExtfileEntry, l_ad), ads.len() as u32);
        img.put(s, UDF_EXTFENTRY_SIZE, ads);
    } else {
        img.put(s, offset_of!(FileEntry, atime), &timestamp(None));
        img.put(s, offset_of!(FileEntry, mtime), &timestamp(None));
        img.u32(s, offset_of!(FileEntry, l_ad), ads.len() as u32);
        img.put(s, UDF_FENTRY_SIZE, ads);
    }
}

/// A short allocation descriptor.
fn short_ad(len: u32, lb: u32) -> [u8; 8] {
    let mut a = [0u8; 8];
    a[0..4].copy_from_slice(&len.to_le_bytes());
    a[4..8].copy_from_slice(&lb.to_le_bytes());
    a
}

/// The name of the `i`th entry of the fragmented directory: 41 characters, so that each
/// descriptor is 80 bytes.
fn sub_name(i: usize) -> Vec<u8> {
    let mut n = std::format!("file-{i:02}-").into_bytes();
    n.resize(41, b'x');
    n
}

/// The test volume "M10C":
/// - `/m10c-udf.txt` ("m10c-udf-42\n", a file entry at lb 4, data at lb 5), mode 0644;
/// - `/emb.txt` ("hello", recorded in its extended file entry at lb 6);
/// - `/sub/` (lb 7): 60 files over three extents at lb 8, 10 and 12;
/// - `/edir/` (lb 13): three files, recorded in its extended file entry;
/// - a deleted entry `gone`.
fn image() -> Vec<u8> {
    let mut img = Img::new();

    // Anchor Volume Descriptor Pointer at sector 256: the main sequence at 32, 16 sectors.
    img.tag(256, TAGID_ANCHOR, 256);
    img.u32(256, offset_of!(AnchorVdp, main_vds_ex), 16 * B as u32);
    img.u32(256, offset_of!(AnchorVdp, main_vds_ex) + 4, 32);

    // Primary volume descriptor, its identifier a dstring.
    img.tag(32, TAGID_PRI_VOL, 32);
    let mut vol_id = [0u8; 32];
    vol_id[0] = 8;
    vol_id[1..5].copy_from_slice(b"M10C");
    vol_id[31] = 5;
    img.put(32, offset_of!(PriVolDesc, vol_id), &vol_id);

    // Partition descriptor: partition 0 at PSTART.
    img.tag(33, TAGID_PARTITION, 33);
    img.u16(33, offset_of!(PartDesc, part_num), 0);
    img.u32(33, offset_of!(PartDesc, start_loc), PSTART as u32);
    img.u32(33, offset_of!(PartDesc, part_len), PLEN as u32);

    // Logical volume descriptor: 2048-byte blocks, the FSD at lb 0, one type 1 map.
    img.tag(34, TAGID_LOGVOL, 34);
    img.u32(34, offset_of!(LogvolDesc, lb_size), B as u32);
    img.u32(34, offset_of!(LogvolDesc, _lvd_use), B as u32);
    img.u32(34, offset_of!(LogvolDesc, mt_l), 6);
    img.u32(34, offset_of!(LogvolDesc, n_pm), 1);
    img.put(34, LogvolDesc::SIZE, &[1, 6, 1, 0, 0, 0]);

    img.tag(35, TAGID_TERM, 35);

    // File set descriptor: the root's file entry at lb 2.
    img.tag(PSTART, TAGID_FSD, 0);
    img.u32(PSTART, offset_of!(FilesetDesc, rootdir_icb), B as u32);
    img.u32(PSTART, offset_of!(FilesetDesc, rootdir_icb) + 4, 2);

    // The root directory, its descriptors at lb 3.
    let mut root = Vec::new();
    root.extend(fid(UDF_FILE_CHAR_DIR | UDF_FILE_CHAR_PAR, b"", 2));
    root.extend(fid(0, b"m10c-udf.txt", 4));
    root.extend(fid(UDF_FILE_CHAR_DEL, b"gone", 4));
    root.extend(fid(UDF_FILE_CHAR_DIR, b"sub", 7));
    root.extend(fid(0, b"emb.txt", 6));
    root.extend(fid(UDF_FILE_CHAR_DIR, b"edir", 13));
    file_entry(
        &mut img,
        2,
        false,
        4,
        0,
        0x1ca5,
        0,
        root.len() as u64,
        &short_ad(root.len() as u32, 3),
    );
    img.put(PSTART + 3, 0, &root);

    // The file and its data.
    file_entry(&mut img, 4, false, 5, 0, 0x1884, 501, 12, &short_ad(12, 5));
    img.put(PSTART + 5, 0, b"m10c-udf-42\n");

    // A file recorded in its extended file entry.
    file_entry(&mut img, 6, true, 5, 3, 0x1884, u32::MAX, 5, b"hello");

    // The fragmented directory.
    let mut sub = Vec::new();
    sub.extend(fid(UDF_FILE_CHAR_DIR | UDF_FILE_CHAR_PAR, b"", 2));
    for i in 0..NSUB {
        let f = fid(0, &sub_name(i), 4);
        assert_eq!(f.len(), 80);
        sub.extend(f);
    }
    assert_eq!(sub.len(), SUB_SIZE);
    let mut ads = Vec::new();
    ads.extend(short_ad(B as u32, 8));
    ads.extend(short_ad(B as u32, 10));
    ads.extend(short_ad((SUB_SIZE - 2 * B) as u32, 12));
    file_entry(&mut img, 7, false, 4, 0, 0x1ca5, 0, SUB_SIZE as u64, &ads);
    img.put(PSTART + 8, 0, &sub[..B]);
    img.put(PSTART + 10, 0, &sub[B..2 * B]);
    img.put(PSTART + 12, 0, &sub[2 * B..]);

    // A directory recorded in its extended file entry.
    let mut edir = Vec::new();
    edir.extend(fid(UDF_FILE_CHAR_DIR | UDF_FILE_CHAR_PAR, b"", 2));
    for name in [b"a1", b"b2", b"c3"] {
        edir.extend(fid(0, name, 4));
    }
    file_entry(
        &mut img,
        13,
        true,
        4,
        3,
        0x1ca5,
        0,
        edir.len() as u64,
        &edir,
    );

    img.0
}

/// The disk the strategy below reads.
static DISK: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

/// A synchronous read of the image at `b_blkno`, then `biodone`.
fn disk_io(bp: &'static Buf) {
    let off = bp.b_blkno.get() as usize * DEV_BSIZE;
    let len = bp.b_bcount.get() as usize;
    {
        let d = DISK.lock().unwrap_or_else(|e| e.into_inner());
        if !bp.isset(B_READ) || off + len > d.len() {
            bp.b_error.set(Some(Errno::EIO));
            bp.set(B_ERROR);
        } else {
            // SAFETY: the buffer is busy for this transfer and mapped.
            let data = unsafe { bp.data() };
            data.copy_from_slice(&d[off..off + len]);
            bp.b_resid.set(0);
        }
    }
    let s = splbio();
    biodone(bp);
    splx(s);
}

fn disk_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    disk_io(ap.a_bp);
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
    vop_fsync: Some(disk_fsync),
    ..Vops::EMPTY
};

/// The fake disk's device number.
const DISKDEV: i32 = makedev(17, 2);

/// Memory, the vfs (whose `vfsinit` runs `udf_init`), a fresh buffer cache, the image as
/// the disk, the thread as `curproc`.
fn setup(image: Vec<u8>) -> (MutexGuard<'static, ()>, &'static Proc) {
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

fn teardown() {
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// `udf_mountfs` of the disk on a fresh read-only mount.
fn mount(p: &'static Proc) -> Result<&'static Mount, Errno> {
    let devvp = bdevvp(DISKDEV).unwrap().unwrap();
    devvp.v_op.set(Some(&DISK_VOPS));
    let mp = vfs_mount_alloc(None, vfs_byname(b"udf").unwrap());
    mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
    match udf_mountfs(devvp, mp, 0, p) {
        Ok(()) => Ok(mp),
        Err(e) => {
            vrele(devvp);
            vfs_mount_free(mp);
            Err(e)
        }
    }
}

/// A component name for `name`, the last one, the parent kept locked.
fn cn(p: &'static Proc, name: &[u8], flags: u64) -> Componentname {
    let mut cnp = Componentname::new();
    cnp.cn_nameiop = LOOKUP;
    cnp.cn_flags = ISLASTCN | LOCKPARENT | flags;
    cnp.cn_proc = p;
    cnp.cn_cred = p.ucred();
    cnp.cn_nameptr = name.as_ptr();
    cnp.cn_namelen = name.len() as i64;
    cnp
}

/// `VOP_LOOKUP` of `name` in `dvp`: the vnode, referenced and locked.
fn lookup(p: &'static Proc, dvp: &'static Vnode, name: &[u8]) -> Result<&'static Vnode, Errno> {
    let flags = if name == b".." { ISDOTDOT } else { 0 };
    let mut cnp = cn(p, name, flags);
    let mut vpp = None;
    VOP_LOOKUP(dvp, &mut vpp, &mut cnp)?;
    Ok(vpp.unwrap())
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
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: out.len() as Off,
            uio_resid: chunk,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        VOP_READ(vp, &mut uio, 0, ptr::null()).unwrap();
        let n = chunk - uio.uio_resid;
        if n == 0 {
            return out;
        }
        out.extend_from_slice(&buf[..n]);
    }
}

/// `getdents` of a directory, `len` bytes at a time from offset 0: (name, fileno, type).
fn read_dir(vp: &'static Vnode, len: usize) -> Vec<(Vec<u8>, u64, u8)> {
    let mut out = Vec::new();
    let mut offset: Off = 0;
    loop {
        let mut buf = vec![0u8; len];
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: len,
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: offset,
            uio_resid: len,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let mut eof = 0;
        VOP_READDIR(vp, &mut uio, ptr::null(), &mut eof).unwrap();
        let used = len - uio.uio_resid;
        let mut pos = 0;
        while pos < used {
            let d = Dirent::from_bytes(&buf[pos..]).unwrap();
            let name = buf[pos + Dirent::NAME_OFFSET..][..usize::from(d.d_namlen)].to_vec();
            assert_eq!(buf[pos + Dirent::NAME_OFFSET + usize::from(d.d_namlen)], 0);
            out.push((name, d.d_fileno, d.d_type));
            pos += usize::from(d.d_reclen);
        }
        offset = uio.uio_offset;
        if eof != 0 || used == 0 {
            return out;
        }
    }
}

fn getattr(p: &'static Proc, vp: &'static Vnode) -> Vattr {
    let mut va = Vattr::new();
    VOP_GETATTR(vp, &mut va, p.ucred(), p).unwrap();
    va
}

#[test]
fn mount_lookup_read_readdir_and_unmount() {
    let (_g, p) = setup(image());
    let mp = mount(p).unwrap();
    let ump = VFSTOUDFFS(mp);
    assert_eq!(ump.um_bsize.get(), 2048);
    assert_eq!(ump.um_bshift.get(), 11);
    assert_eq!(ump.um_start.get(), PSTART as u32);
    assert_eq!(mp.mnt_flag.get() & MNT_LOCAL, MNT_LOCAL);

    let root = udf_root(mp).unwrap();
    assert_eq!(root.v_type.get(), VDIR);
    assert_ne!(root.v_flag.get() & VROOT, 0);
    assert_eq!(VTOU(root).u_ino.get(), 2);
    let va = getattr(p, root);
    assert_eq!(va.va_mode, 0o755);
    assert_eq!(va.va_nlink, 2);
    assert_eq!(va.va_size, 2048);

    // The file: lookup, attributes, data in pieces of every size, the block map.
    let vp = lookup(p, root, b"m10c-udf.txt").unwrap();
    assert_eq!(vp.v_type.get(), VREG);
    let va = getattr(p, vp);
    assert_eq!((va.va_mode, va.va_uid, va.va_gid), (0o644, 501, 20));
    assert_eq!((va.va_size, va.va_bytes, va.va_fileid), (12, 12, 4));
    // The C's arithmetic (one day more than the date: it counts the day of the month whole).
    assert_eq!(va.va_mtime.tv_sec, 1_791_203_696);
    for chunk in [1, 5, 12, 4096] {
        assert_eq!(read_all(vp, chunk), b"m10c-udf-42\n");
    }
    let mut bn: Daddr = 0;
    let mut devvp = None;
    VOP_BMAP(vp, 0, Some(&mut devvp), Some(&mut bn), None).unwrap();
    assert_eq!(bn, ((PSTART + 5) * (B / DEV_BSIZE)) as Daddr);
    assert!(ptr::eq(devvp.unwrap(), ump.devvp()));
    // Looking it up again finds the cached vnode.
    let vp2 = lookup(p, root, b"m10c-udf.txt").unwrap();
    assert!(ptr::eq(vp, vp2));
    vput(vp2);
    vput(vp);

    // A file recorded in its extended file entry, owner -1 (shown as 0), time zone -60.
    let vp = lookup(p, root, b"emb.txt").unwrap();
    assert_eq!(read_all(vp, 3), b"hello");
    let va = getattr(p, vp);
    assert_eq!((va.va_size, va.va_uid), (5, 0));
    assert_eq!(va.va_mtime.tv_sec, 1_791_203_696 + 3600);
    let mut bn: Daddr = 0;
    assert_eq!(
        VOP_BMAP(vp, 0, None, Some(&mut bn), None),
        Err(UDF_INVALID_BMAP)
    );
    vput(vp);

    // Names that are not there, the deleted one and ".".
    assert_eq!(lookup(p, root, b"gone").err(), Some(Errno::ENOENT));
    assert_eq!(lookup(p, root, b"m10c-udf.tx").err(), Some(Errno::ENOENT));
    let dot = lookup(p, root, b".").unwrap();
    assert!(ptr::eq(dot, root));
    vrele(dot);

    // The root directory.
    let names: Vec<_> = read_dir(root, 4096)
        .into_iter()
        .map(|(n, ino, t)| (std::string::String::from_utf8(n).unwrap(), ino, t))
        .collect();
    assert_eq!(
        names,
        vec![
            (".".into(), 2, DT_DIR),
            ("..".into(), 2, DT_DIR),
            ("m10c-udf.txt".into(), 4, DT_UNKNOWN),
            ("sub".into(), 7, DT_DIR),
            ("emb.txt".into(), 6, DT_UNKNOWN),
            ("edir".into(), 13, DT_DIR),
        ]
    );

    // The directory recorded in its file entry, whole and resumed. (A buffer too small for
    // both "." and ".." would get "." again and again: the C does not move past it.)
    let edir = lookup(p, root, b"edir").unwrap();
    for len in [4096, 72] {
        let names: Vec<_> = read_dir(edir, len).into_iter().map(|(n, _, _)| n).collect();
        assert_eq!(
            names,
            vec![&b"."[..], b"..", b"a1", b"b2", b"c3"],
            "buffer {len}"
        );
    }
    let vp = lookup(p, edir, b"c3").unwrap();
    assert_eq!(read_all(vp, 4096), b"m10c-udf-42\n");
    vput(vp);
    vput(edir);

    // The directory whose descriptors straddle its extents (at 2040 and 4040), read whole and
    // in small pieces; lookups of the straddling names; ".." back to the root.
    let sub = lookup(p, root, b"sub").unwrap();
    assert_eq!(sub.v_type.get(), VDIR);
    for len in [8192, 100] {
        let ents = read_dir(sub, len);
        assert_eq!(ents.len(), 2 + NSUB, "buffer {len}");
        for (i, (name, ino, _)) in ents[2..].iter().enumerate() {
            assert_eq!(name, &sub_name(i));
            assert_eq!(*ino, 4);
        }
    }
    for i in [0, 25, 26, 50, 51, NSUB - 1] {
        let vp = lookup(p, sub, &sub_name(i)).unwrap();
        assert_eq!(read_all(vp, 64), b"m10c-udf-42\n");
        vput(vp);
    }
    let up = lookup(p, sub, b"..").unwrap();
    assert!(ptr::eq(up, root));
    vput(up);
    vput(sub);

    let _ = VOP_UNLOCK(root);
    vrele(root);
    udf_unmount(mp, 0, p).unwrap();
    vfs_mount_free(mp);
    teardown();
}

#[test]
fn a_damaged_volume_is_not_mounted() {
    let mut img = image();
    // Break the anchor's checksum.
    img[256 * B + 4] ^= 1;
    let (_g, p) = setup(img);
    assert_eq!(mount(p).err(), Some(Errno::EINVAL));

    // No partition descriptor.
    let mut img = image();
    img[33 * B..34 * B].fill(0);
    *DISK.lock().unwrap() = img;
    assert_eq!(mount(p).err(), Some(Errno::EINVAL));
    teardown();
}

/// The disk label strategy: the image read through `disk_io`.
fn spoof_strategy(bp: &'static Buf) {
    disk_io(bp);
}

#[test]
fn a_udf_volume_gets_a_spoofed_label() {
    let (_g, _p) = setup(image());
    let mut lp = Disklabel::zeroed();
    lp.d_secsize = DEV_BSIZE as u32;
    lp.d_secpercyl = 64;
    let size = ((PSTART + PLEN) * B / DEV_BSIZE) as u64;
    dl_setdsize(&mut lp, size);
    udf_disklabelspoof(DISKDEV, spoof_strategy, &mut lp).unwrap();
    assert_eq!(&lp.d_typename[..5], b"M10C\0");
    assert_eq!(lp.d_partitions[0].p_fstype, FS_UDF);
    assert_eq!(lp.d_partitions[RAW_PART as usize].p_fstype, FS_UDF);
    assert_eq!(dl_getpsize(&lp.d_partitions[0]), size);
    assert_eq!(lp.d_magic, DISKMAGIC);

    // Not UDF: no anchor.
    DISK.lock().unwrap()[256 * B..257 * B].fill(0);
    let mut lp = Disklabel::zeroed();
    lp.d_secsize = DEV_BSIZE as u32;
    lp.d_secpercyl = 64;
    assert_eq!(
        udf_disklabelspoof(DISKDEV, spoof_strategy, &mut lp),
        Err(Errno::EINVAL)
    );
    teardown();
}

#[test]
fn names_translate_and_compare() {
    let (_g, _p) = setup(image());
    let mut d = [0xffu8; 16];
    assert_eq!(udf_transname(b"\x08abc", &mut d, 4, None), 3);
    assert_eq!(&d[..4], b"abc\0");
    // 16-bit names: a character past 8 bits becomes '?'.
    assert_eq!(udf_transname(b"\x10\x00a\x26\x3a\x00b", &mut d, 7, None), 3);
    assert_eq!(&d[..4], b"a?b\0");
    // Too long, bad compression id, no room for the NUL.
    assert_eq!(udf_transname(b"\x08abc", &mut d, 300, None), 0);
    assert_eq!(udf_transname(b"\x09abc", &mut d, 4, None), 0);
    assert_eq!(udf_transname(b"\x08abc", &mut d[..3], 4, None), 0);

    let ump = Umount::new();
    assert!(!udf_cmpname(b"\x08abc", b"abc", 4, 3, &ump));
    assert!(udf_cmpname(b"\x08abc", b"abd", 4, 3, &ump));
    assert!(udf_cmpname(b"\x08abc", b"ab", 4, 2, &ump));
    assert!(udf_cmpname(b"\x08", b"", 1, 0, &ump));
    teardown();
}

#[test]
fn checktag_wants_the_id_and_the_sum() {
    let t = tag_bytes(TAGID_FID, 1234);
    let tag = DescTag::at(&t, 0).unwrap();
    assert_eq!(udf_checktag(tag, TAGID_FID), Ok(()));
    assert_eq!(udf_checktag(tag, TAGID_FENTRY), Err(Errno::EINVAL));
    let mut bad = t;
    bad[13] ^= 0x40;
    assert_eq!(
        udf_checktag(DescTag::at(&bad, 0).unwrap(), TAGID_FID),
        Err(Errno::EINVAL)
    );
}

#[test]
fn timestamps_follow_the_c_arithmetic() {
    let ts = |tz| *Timestamp::at(&timestamp(tz), 0).unwrap();
    let mut t = Timespec::new(7, 7);
    udf_timetotimespec(&ts(None), &mut t);
    assert_eq!((t.tv_sec, t.tv_nsec), (1_791_203_696, 0));
    udf_timetotimespec(&ts(Some(120)), &mut t);
    assert_eq!(t.tv_sec, 1_791_203_696 - 7200);
    // -2047 means "no time zone".
    udf_timetotimespec(&ts(Some(-2047)), &mut t);
    assert_eq!(t.tv_sec, 1_791_203_696);
    // Bogus years are the epoch; a month past December adds nothing more.
    let mut raw = timestamp(None);
    raw[2..4].copy_from_slice(&1969u16.to_le_bytes());
    udf_timetotimespec(Timestamp::at(&raw, 0).unwrap(), &mut t);
    assert_eq!((t.tv_sec, t.tv_nsec), (0, 0));
    let mut raw = timestamp(None);
    raw[4] = 13;
    raw[9] = 1; // centisec
    let mut u = Timespec::new(0, 0);
    udf_timetotimespec(Timestamp::at(&raw, 0).unwrap(), &mut u);
    raw[4] = 200;
    udf_timetotimespec(Timestamp::at(&raw, 0).unwrap(), &mut t);
    assert_eq!(t, u);
    assert_eq!(t.tv_nsec, 10_000_000);
}

#[test]
fn leap_years() {
    assert_eq!(udf_isaleapyear(2000), 1);
    assert_eq!(udf_isaleapyear(1900), 0);
    assert_eq!(udf_isaleapyear(2024), 1);
    assert_eq!(udf_isaleapyear(2023), 0);
}
