//! Host tests for the ISO 9660 file system: a block device vnode whose strategy reads the
//! makefs test image (`iso/tests.rs`), mounted as root with `iso_mountfs` in its Rock
//! Ridge and plain forms, then looked up, read, listed, stat'ed and its symbolic link
//! followed through the system calls, and unmounted; the disk label a CD gets; file
//! handles.

use core::ptr;
use core::sync::atomic::Ordering;
use std::sync::{Mutex, MutexGuard};
use std::vec::Vec;
use std::{assert_eq, vec};

use super::*;
use crate::isofs::cd9660::iso::tests::image;
use crate::kern::kern_descrip::sys_close;
use crate::kern::subr_xxx::nullop;
use crate::kern::sys_generic::sys_read;
use crate::kern::vfs_bio::{BCSTATS, BUFHEAD, BUFKVM, CLEANCACHE, biodone, bufinit};
use crate::kern::vfs_default::vop_generic_bwrite;
use crate::kern::vfs_init::{set_rootvnode, vfs_byname};
use crate::kern::vfs_subr::{vflushbuf, vfs_busy, vfs_mount_alloc};
use crate::kern::vfs_syscalls::{dounmount, sys_getdents, sys_open, sys_readlink, sys_stat};
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::{B_ERROR, B_READ};
use crate::sys::fcntl::O_RDONLY;
use crate::sys::mount::{MNT_WAIT, VB_WAIT, VB_WRITE, VFS_ROOT};
use crate::sys::param::DEV_BSIZE;
use crate::sys::stat::{S_IFDIR, S_IFMT, S_IFREG, Stat};
use crate::sys::systm::{SyCall, SysArgs};
use crate::sys::types::{Register, makedev};
use crate::sys::vnode::{VopFsyncArgs, VopInactiveArgs, VopStrategyArgs, Vops};

/// The disk the strategy below reads.
static DISK: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// A synchronous transfer between the buffer and `DISK` at `b_blkno`, then `biodone`.
fn transfer(bp: &'static Buf) {
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
}

/// `diskvp`'s strategy.
fn disk_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    transfer(ap.a_bp);
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

/// The operations of the fake disk's block device vnode (no ioctl: `CDIOREADMSADDR` fails
/// and the session is 0).
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

/// The device number of the fake disk.
const DISKDEV: i32 = makedev(17, 0);

/// Memory, the vfs and a fresh buffer cache, `disk` as the disk, the thread as `curproc`.
fn setup(disk: Vec<u8>) -> (MutexGuard<'static, ()>, &'static Proc) {
    let (g, p) = crate::kern::vfs_subr::tests::setup();
    Machine::set_curproc(Machine::curcpu(), p);
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

    *DISK.lock().unwrap_or_else(|e| e.into_inner()) = disk;
    (g, p)
}

fn teardown() {
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// The fake disk's block device vnode, referenced.
fn diskvp() -> &'static Vnode {
    let vp = bdevvp(DISKDEV).unwrap().unwrap();
    vp.v_op.set(Some(&DISK_VOPS));
    vp
}

/// Mounts the disk read-only at `/` the way `cd9660_mountroot` and `main` do, with the
/// mount arguments' `flags`; returns the mount and the flags `iso_mountfs` left.
fn mount_root(p: &'static Proc, flags: i32) -> Result<(&'static Mount, i32), Errno> {
    let devvp = diskvp();
    let mp = vfs_mount_alloc(None, vfs_byname(b"cd9660").unwrap());
    mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
    mp.update_stat(|sp| sp.f_mntonname[0] = b'/');
    let mut args = IsoArgs::from_bytes(&[0u8; IsoArgs::SIZE]).unwrap();
    args.flags = flags;
    if let Err(e) = iso_mountfs(devvp, mp, p, &mut args) {
        vrele(devvp);
        vfs_unbusy(mp);
        vfs_mount_free(mp);
        return Err(e);
    }
    let mut st = mp.mnt_stat.get();
    cd9660_statfs(mp, &mut st, p).unwrap();
    mp.mnt_stat.set(st);
    vfs_unbusy(mp);
    // SAFETY: a new mount on no list.
    unsafe { MOUNTLIST.0.insert_tail(mp) };
    let root = VFS_ROOT(mp).unwrap();
    set_rootvnode(Some(root));
    p.fd().fd_cdir.set(Some(root));
    vref(root);
    let _ = VOP_UNLOCK(root);
    Ok((mp, args.flags))
}

/// Undoes `mount_root` and unmounts.
fn unmount_root(p: &'static Proc, mp: &'static Mount) {
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
fn sys(f: SyCall, p: &Proc, args: &[usize]) -> Result<isize, Errno> {
    let mut v: SysArgs = [0; 6];
    for (slot, a) in v.iter_mut().zip(args) {
        *slot = *a as Register;
    }
    let mut rv = [0; 2];
    f(p, &v, &mut rv)?;
    Ok(rv[0])
}

/// A NUL-terminated path as a "user" address (the host's copyin reads it directly).
fn path(s: &'static [u8]) -> usize {
    assert_eq!(s.last(), Some(&0));
    s.as_ptr() as usize
}

/// The whole contents of the file at `name`, read in small pieces.
fn read_file(p: &Proc, name: &'static [u8]) -> Result<Vec<u8>, Errno> {
    let fd = sys(sys_open, p, &[path(name), O_RDONLY as usize, 0])?;
    let mut out = Vec::new();
    let mut buf = vec![0u8; 5];
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

/// The names in the directory `name` in the order `getdents` returns them, read with a
/// buffer of `bufsize` bytes.
fn list_dir(p: &Proc, name: &'static [u8], bufsize: usize) -> Vec<Vec<u8>> {
    let fd = sys(sys_open, p, &[path(name), O_RDONLY as usize, 0]).unwrap();
    let mut buf = vec![0u8; bufsize];
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
            names.push(buf[off + 24..off + 24 + namlen].to_vec());
            off += reclen;
        }
    }
    sys(sys_close, p, &[fd as usize]).unwrap();
    names
}

/// `stat(2)` of `name`.
fn stat(p: &Proc, name: &'static [u8]) -> Result<Stat, Errno> {
    let mut st = Stat::default();
    sys(sys_stat, p, &[path(name), ptr::from_mut(&mut st) as usize])?;
    Ok(st)
}

/// `mount -u` of a disc with an export list (`im_export`): `cd9660_check_export` answers the
/// listed client and refuses the others.
#[cfg(feature = "nfsserver")]
#[test]
fn an_exported_disc_answers_check_export() {
    use crate::kern::uipc_mbuf::tests::mbinit_again;
    use crate::kern::vfs_subr::tests::exports::{args, check_export, sin};
    use crate::sys::mount::{MNT_EXPORTED, MNT_EXRDONLY};

    let (_g, p) = setup(image::image_bytes());
    mbinit_again();
    let (mp, _flags) = mount_root(p, 0).unwrap();
    let imp = vfstoisofs(mp);
    let ro = MNT_EXPORTED | MNT_EXRDONLY;
    assert_eq!(check_export(mp, [10, 0, 0, 5]), Err(Errno::EACCES));

    let net = sin(2, [10, 0, 0, 0]);
    let mask = sin(2, [255, 255, 255, 0]);
    vfs_export(mp, &imp.im_export, &args(ro, 32767, Some(net), Some(mask))).unwrap();
    assert_eq!(check_export(mp, [10, 0, 0, 5]), Ok((ro, 32767)));
    assert_eq!(check_export(mp, [10, 0, 1, 5]), Err(Errno::EACCES));

    unmount_root(p, mp);
    teardown();
}

#[test]
fn a_rock_ridge_disc_mounts_and_reads() {
    let (_g, p) = setup(image::image_bytes());
    let (mp, flags) = mount_root(p, 0).unwrap();
    // Rock Ridge found: generation numbers are off, the format is RRIP
    assert_eq!(flags & (ISOFSMNT_NORRIP | ISOFSMNT_GENS), 0);
    let imp = vfstoisofs(mp);
    assert_eq!(imp.iso_ftype, ISO_FTYPE_RRIP);
    assert_eq!((imp.logical_block_size, imp.im_bshift), (2048, 11));
    assert_eq!((imp.root_extent, imp.rr_skip, imp.rr_skip0), (20, 0, 0));
    assert_eq!(mp.mnt_stat.get().f_blocks, 25);
    assert_eq!(mp.mnt_stat.get().f_namemax, 255);
    assert_ne!(mp.mnt_flag.get() & MNT_LOCAL, 0);

    assert_eq!(read_file(p, b"/m10c-iso.txt\0").unwrap(), b"m10c-iso-42\n");
    assert_eq!(
        read_file(p, b"sub/Mixed_Case-name.long.txt\0").unwrap(),
        b"hello\n"
    );
    // the symbolic link, followed by namei through "..", and read by readlink(2)
    assert_eq!(read_file(p, b"/sub/link\0").unwrap(), b"m10c-iso-42\n");
    let mut buf = [0u8; 64];
    let n = sys(
        sys_readlink,
        p,
        &[path(b"/sub/link\0"), buf.as_mut_ptr() as usize, buf.len()],
    )
    .unwrap();
    assert_eq!(&buf[..n as usize], b"../m10c-iso.txt");
    assert_eq!(read_file(p, b"/M10C_ISO.TXT\0"), Err(Errno::ENOENT));

    assert_eq!(
        list_dir(p, b"/\0", 4096),
        [
            b".".to_vec(),
            b"..".to_vec(),
            b"m10c-iso.txt".to_vec(),
            b"sub".to_vec()
        ]
    );
    // a buffer that holds two short entries, or the long one, at a time
    assert_eq!(list_dir(p, b"/sub\0", 64).len(), 4);

    let st = stat(p, b"/m10c-iso.txt\0").unwrap();
    assert_eq!(
        (st.st_mode, st.st_size, st.st_nlink),
        (S_IFREG | 0o644, 12, 1)
    );
    assert_eq!(st.st_mtim.tv_sec, 1_791_110_208);
    let st = stat(p, b"/sub\0").unwrap();
    assert_eq!(st.st_mode & S_IFMT, S_IFDIR);
    assert_eq!(st.st_ino, 21 << 11);
    let st = stat(p, b"/\0").unwrap();
    assert_eq!((st.st_mode, st.st_ino), (S_IFDIR | 0o755, 20 << 11));

    unmount_root(p, mp);
    teardown();
}

#[test]
fn a_disc_mounted_without_rock_ridge_shows_iso_names() {
    let (_g, p) = setup(image::image_bytes());
    let (mp, flags) = mount_root(p, ISOFSMNT_NORRIP).unwrap();
    assert_eq!(flags & ISOFSMNT_NORRIP, ISOFSMNT_NORRIP);
    assert_eq!(vfstoisofs(mp).iso_ftype, ISO_FTYPE_DEFAULT);

    // ISO names, case-insensitively and without the version
    assert_eq!(read_file(p, b"/m10c_iso.txt\0").unwrap(), b"m10c-iso-42\n");
    assert_eq!(
        read_file(p, b"/M10C_ISO.TXT;1\0").unwrap(),
        b"m10c-iso-42\n"
    );
    assert_eq!(read_file(p, b"/m10c-iso.txt\0"), Err(Errno::ENOENT));
    assert_eq!(
        list_dir(p, b"/SUB\0", 4096),
        [
            b".".to_vec(),
            b"..".to_vec(),
            b"LINK".to_vec(),
            b"MIXED_CASE_NAME.LONG_TXT".to_vec()
        ]
    );
    // plain ISO 9660 has no owners or modes: everything is readable and executable
    let st = stat(p, b"/m10c_iso.txt\0").unwrap();
    assert_eq!(st.st_mode, S_IFREG | 0o555);
    assert_eq!(st.st_mtim.tv_sec, 1_791_110_208);

    unmount_root(p, mp);
    teardown();
}

#[test]
fn a_disc_mounted_with_gens_keeps_versions() {
    let (_g, p) = setup(image::image_bytes());
    let (mp, _) = mount_root(p, ISOFSMNT_NORRIP | ISOFSMNT_GENS).unwrap();
    assert_eq!(vfstoisofs(mp).iso_ftype, ISO_FTYPE_9660);
    assert_eq!(
        list_dir(p, b"/\0", 4096),
        [
            b".".to_vec(),
            b"..".to_vec(),
            b"M10C_ISO.TXT;1".to_vec(),
            b"SUB".to_vec()
        ]
    );
    unmount_root(p, mp);
    teardown();
}

#[test]
fn a_disc_without_volume_descriptors_does_not_mount() {
    let (_g, p) = setup(vec![0u8; image::IMAGE_SIZE]);
    assert_eq!(mount_root(p, 0).err(), Some(Errno::EINVAL));
    teardown();
}

#[test]
fn a_bad_logical_block_size_does_not_mount() {
    let mut img = image::image_bytes();
    img[16 * 2048 + 128] = 0x03; // 2051, not a power of two
    let (_g, p) = setup(img);
    assert_eq!(mount_root(p, 0).err(), Some(Errno::EINVAL));
    teardown();
}

/// `iso_disklabelspoof`'s strategy over `DISK`.
fn spoof_strategy(bp: &'static Buf) {
    transfer(bp);
}

#[test]
fn a_cd_gets_a_disk_label() {
    let (_g, _p) = setup(image::image_bytes());
    let mut lp = Disklabel::zeroed();
    lp.d_secsize = 512;
    lp.d_secperunit = 100;
    iso_disklabelspoof(DISKDEV, spoof_strategy, &mut lp).unwrap();
    assert_eq!(&lp.d_typename, b"M10C            ");
    assert_eq!(&lp.d_packname, b"                ");
    for i in [0, RAW_PART as usize] {
        assert_eq!(lp.d_partitions[i].p_fstype, FS_ISO9660);
        assert_eq!(crate::sys::disklabel::dl_getpsize(&lp.d_partitions[i]), 100);
        assert_eq!(crate::sys::disklabel::dl_getpoffset(&lp.d_partitions[i]), 0);
    }
    assert_eq!(usize::from(lp.d_npartitions), MAXPARTITIONS);
    assert_eq!(
        (lp.d_magic, lp.d_magic2, lp.d_version),
        (DISKMAGIC, DISKMAGIC, 1)
    );
    assert_eq!(dkcksum(&lp), 0);

    // not a CD
    *DISK.lock().unwrap_or_else(|e| e.into_inner()) = vec![0u8; image::IMAGE_SIZE];
    let mut lp = Disklabel::zeroed();
    assert_eq!(
        iso_disklabelspoof(DISKDEV, spoof_strategy, &mut lp),
        Err(Errno::EINVAL)
    );
    teardown();
}

#[test]
fn file_handles_overlay_struct_fid() {
    let ifid = Ifid {
        ifid_len: IFID_SIZE,
        ifid_pad: 0,
        ifid_ino: 41284,
        ifid_start: 23,
    };
    let mut fid = Fid::default();
    ifid.to_fid(&mut fid);
    assert_eq!(fid.fid_len, 16);
    assert_eq!(&fid.fid_data[..4], &41284i32.to_ne_bytes());
    assert_eq!(&fid.fid_data[4..12], &23i64.to_ne_bytes());
    assert_eq!(Ifid::from_fid(&fid), ifid);
}

#[test]
fn names_copy_like_strlcpy_and_strncpy() {
    let mut m = [b'x'; MNAMELEN];
    mname_copy(&mut m, b"/dev/vnd1c\0junk");
    assert_eq!(&m[..11], b"/dev/vnd1c\0");
    assert!(m[11..].iter().all(|&c| c == 0));
    let mut d = [b'x'; 8];
    strncpy(&mut d, b"ab\0cd");
    assert_eq!(&d, b"ab\0\0\0\0\0\0");
    strncpy(&mut d, b"0123456789");
    assert_eq!(&d, b"01234567");
}
