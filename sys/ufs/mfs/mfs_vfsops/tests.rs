//! Host tests for the memory file system: an FFS image built in memory by the FFS tests'
//! `newfs` is the "memory of the mounting process" (the host has one address space, so
//! `copyin`/`copyout` are plain copies), `mfs_mount` mounts it through the vfs, files are
//! read and written through the system calls, and the unmount leaves the image clean.
//! `mfs_start` sleeps until the unmount, which the host cannot do: it is tested only for the
//! shutdown it answers at once and for the I/O it serves, which only the smoke runs end to
//! end (`mount_mfs -s 8m swap /mfs`).

use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::kern_descrip::sys_close;
use crate::kern::sys_generic::sys_write;
use crate::kern::vfs_init::{set_rootvnode, vfs_byname};
use crate::kern::vfs_lookup::ndinit;
use crate::kern::vfs_subr::{MOUNTLIST, vfs_mount_alloc, vfs_unbusy, vref};
use crate::kern::vfs_syscalls::{sys_open, sys_sync};
use crate::kern::vfs_vops::VOP_UNLOCK;
use crate::sys::fcntl::{O_CREAT, O_RDWR};
use crate::sys::mount::{MNT_LOCAL, VFS_ROOT, VFS_STATFS};
use crate::sys::namei::{FOLLOW, LOOKUP, NiDirp};
use crate::sys::types::major;
use crate::ufs::ffs::ffs_vfsops::tests::{newfs, path, read_file, setup, sys, teardown};
use crate::ufs::ffs::fs::FS_UFS2_MAGIC;
use crate::ufs::mfs::mfs_vnops::MFS_VOPS;

/// The name `newfs -DMFS` hands the kernel as `fspec`: `mfs:<pid>`.
static FSPEC: &[u8] = b"mfs:1234\0";

/// `struct mfs_args` as `sys_mount` copies it in: `fspec`, the (zeroed) export arguments,
/// `base` and `size`.
fn mfs_args(fspec: usize, base: usize, size: usize) -> [u8; MfsArgs::SIZE] {
    let mut b = [0u8; MfsArgs::SIZE];
    b[..8].copy_from_slice(&fspec.to_ne_bytes());
    b[MfsArgs::SIZE - 16..MfsArgs::SIZE - 8].copy_from_slice(&base.to_ne_bytes());
    b[MfsArgs::SIZE - 8..].copy_from_slice(&(size as u64).to_ne_bytes());
    b
}

/// A new `mfs` mount (as `sys_mount` makes it, before the call), flagged read-only or not.
fn new_mount(ronly: bool) -> &'static Mount {
    let mp = vfs_mount_alloc(None, vfs_byname(b"mfs").unwrap());
    if ronly {
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
    }
    mp
}

/// Mounts `image` at `/` the way `sys_mount` does: `mfs_mount`, `statfs`, the mount list, the
/// root vnode and the thread's current directory.
fn mount_mfs(p: &'static Proc, image: &mut [u8], ronly: bool) -> &'static Mount {
    let mp = new_mount(ronly);
    let mut args = mfs_args(
        FSPEC.as_ptr() as usize,
        image.as_mut_ptr() as usize,
        image.len(),
    );
    let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(b"/"), p);
    mfs_mount(mp, b"/\0", &mut args, &mut nd, p).unwrap();
    let mut st = mp.mnt_stat.get();
    VFS_STATFS(mp, &mut st, p).unwrap();
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

/// Creates `name` with `data`.
fn write_file(p: &Proc, name: &'static [u8], data: &[u8]) {
    let fd = sys(
        sys_open,
        p,
        &[path(name), (O_RDWR | O_CREAT) as usize, 0o644],
    )
    .unwrap();
    for chunk in data.chunks(5000) {
        let n = sys(
            sys_write,
            p,
            &[fd as usize, chunk.as_ptr() as usize, chunk.len()],
        )
        .unwrap();
        assert_eq!(n as usize, chunk.len());
    }
    sys(sys_close, p, &[fd as usize]).unwrap();
}

fn pattern(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(29).wrapping_add(seed) ^ (i >> 9) as u8)
        .collect()
}

#[test]
fn mfs_is_in_vfsconflist() {
    let vfsp = vfs_byname(b"mfs").unwrap();
    assert_eq!(vfsp.vfc_typenum, 3);
    assert_eq!(vfsp.vfc_datasize, MfsArgs::SIZE);
    assert_eq!(vfsp.vfc_flags, MNT_LOCAL);
    // ffs comes first, as in vfsconflist[].
    assert_eq!(vfs_byname(b"ffs").unwrap().vfc_typenum, 1);
}

#[test]
fn mfs_args_are_read_from_the_kernel_copy() {
    let a = mfs_args(0x1000, 0x7000_0000, 8 << 20);
    let args = MfsArgs::from_bytes(&a).unwrap();
    assert_eq!(args.fspec, 0x1000);
    assert_eq!(args.base, 0x7000_0000);
    assert_eq!(args.size, 8 << 20);
    assert_eq!(args.export_info.ex_flags, 0);
    assert!(MfsArgs::from_bytes(&a[..MfsArgs::SIZE - 1]).is_none());
    assert!(MfsArgs::from_bytes(&[]).is_none());
}

#[test]
fn mfs_mount_read_write_and_unmount() {
    let mut img = newfs::Image::new(newfs::FFS2_4M);
    img.add_file(b"motd", b"hello, memory\n");
    let big = pattern(30000, 3);
    img.add_file(b"big", &big);
    let mut image = img.finish();
    let (_g, p) = setup(Vec::new());

    let mp = mount_mfs(p, &mut image, false);
    let ump = vfstoufs(mp);
    let fs = ump.fs();
    assert_eq!(fs.fs_magic.get(), FS_UFS2_MAGIC);
    assert_eq!(&fs.fs_fsmnt.get()[..2], b"/\0");
    assert_eq!(fs.fs_ronly.get(), 0);

    // The "device" is a made-up block device vnode of the mfs type, and the mount knows
    // what it was mounted from, on, and with.
    let devvp = ump.devvp();
    assert_eq!(devvp.v_tag.get(), VT_MFS);
    assert_eq!(devvp.v_type.get(), VBLK);
    assert_eq!(major(devvp.v_rdev()), 255);
    let mfsp = vtomfs(devvp);
    assert_eq!(mfsp.mfs_size.get(), image.len() as i64);
    assert_eq!(mfsp.mfs_baseoff.get(), image.as_ptr() as usize);
    assert_eq!(mfsp.mfs_tid.get(), p.p_tid.get());
    assert!(core::ptr::eq(crate::ufs::mfs::mfsnode::mfstov(mfsp), devvp));
    let st = mp.mnt_stat.get();
    assert_eq!(&st.f_mntonname[..2], b"/\0");
    assert_eq!(&st.f_mntfromname[..9], b"mfs:1234\0");
    assert_eq!(&st.f_mntfromspec[..9], b"mfs:1234\0");
    assert_eq!(&st.f_fstypename[..4], b"mfs\0");
    assert_eq!(st.f_fsid.val[0], devvp.v_rdev());
    assert_eq!(
        &st.mount_info.__align[..8],
        &(FSPEC.as_ptr() as usize).to_ne_bytes()
    );
    assert_eq!(
        &st.mount_info.__align[MfsArgs::SIZE - 16..MfsArgs::SIZE - 8],
        &(image.as_ptr() as usize).to_ne_bytes()
    );

    // What newfs left in the memory reads back; what is written lands in it.
    assert_eq!(read_file(p, b"/motd\0").unwrap(), b"hello, memory\n");
    assert_eq!(read_file(p, b"/big\0").unwrap(), big);
    let note = pattern(20000, 9);
    write_file(p, b"/note\0", &note);
    assert_eq!(read_file(p, b"/note\0").unwrap(), note);
    sys(sys_sync, p, &[]).unwrap();

    // mfs_start answers a shutdown at once, with the queue untouched.
    mfsp.mfs_shutdown.store(1, Ordering::Relaxed);
    mfs_start(mp, 0, p).unwrap();
    mfsp.mfs_shutdown.store(0, Ordering::Relaxed);

    // Unmount: the close of the device tells the process to exit, the image is clean.
    if let Some(cdir) = p.fd().fd_cdir.take() {
        crate::kern::vfs_subr::vrele(cdir);
    }
    if let Some(root) = crate::kern::vfs_init::rootvnode() {
        set_rootvnode(None);
        crate::kern::vfs_subr::vrele(root);
    }
    crate::kern::vfs_subr::vfs_busy(mp, crate::sys::mount::VB_WRITE | crate::sys::mount::VB_WAIT)
        .unwrap();
    crate::kern::vfs_syscalls::dounmount(mp, 0, p).unwrap();
    assert_eq!(mfsp.mfs_shutdown.load(Ordering::Relaxed), 1);
    newfs::check(&image, true);
    assert_eq!(newfs::clean(&image, true), 1);

    // Recycling the device vnode frees the mfsnode (`mfs_reclaim`).
    crate::kern::vfs_subr::vgone(devvp);
    assert!(devvp.v_data.get().is_null());

    // The same memory mounts again (a new "process"): the file written is still there.
    let mp = mount_mfs(p, &mut image, true);
    assert_eq!(read_file(p, b"/note\0").unwrap(), note);
    assert_eq!(read_file(p, b"/big\0").unwrap(), big);
    assert_eq!(vfstoufs(mp).fs().fs_ronly.get(), 1);
    teardown();
}

#[test]
fn mfs_mount_failures_release_the_device() {
    let (_g, p) = setup(Vec::new());
    let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(b"/"), p);

    // No arguments.
    let mp = new_mount(false);
    assert_eq!(
        mfs_mount(mp, b"/\0", &mut [], &mut nd, p),
        Err(Errno::EINVAL)
    );

    // A name that cannot be copied in.
    let mut image = vec![0u8; 1 << 20];
    let mut args = mfs_args(0, image.as_mut_ptr() as usize, image.len());
    assert_eq!(
        mfs_mount(mp, b"/\0", &mut args, &mut nd, p),
        Err(Errno::EFAULT)
    );

    // Memory with no file system in it: ffs_mountfs fails, the mfsnode is told to shut down.
    let minor = MFS_MINOR.load(Ordering::Relaxed);
    let mut args = mfs_args(
        FSPEC.as_ptr() as usize,
        image.as_mut_ptr() as usize,
        image.len(),
    );
    assert_eq!(
        mfs_mount(mp, b"/\0", &mut args, &mut nd, p),
        Err(Errno::EINVAL)
    );
    assert_eq!(MFS_MINOR.load(Ordering::Relaxed), minor + 1);
    assert!(mp.mnt_data.get().is_null());
    teardown();
}

#[test]
fn mfs_update_flushes_and_reenables_writes() {
    let mut image = newfs::Image::new(newfs::FFS1_4M).finish();
    let (_g, p) = setup(Vec::new());
    let mp = mount_mfs(p, &mut image, false);
    let fs = vfstoufs(mp).fs();
    let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(b"/"), p);

    // An update to read-only flushes the files open for writing; the C leaves `fs_ronly` to
    // the generic code, and so does this.
    mp.mnt_flag.set(mp.mnt_flag.get() | MNT_UPDATE | MNT_RDONLY);
    mfs_mount(mp, b"/\0", &mut [], &mut nd, p).unwrap();
    assert_eq!(fs.fs_ronly.get(), 0);

    // A read-only file system asked to be read-write.
    fs.fs_ronly.set(1);
    mp.mnt_flag
        .set((mp.mnt_flag.get() & !MNT_RDONLY) | MNT_UPDATE | MNT_WANTRDWR);
    mfs_mount(mp, b"/\0", &mut [], &mut nd, p).unwrap();
    assert_eq!(fs.fs_ronly.get(), 0);

    mp.mnt_flag
        .set(mp.mnt_flag.get() & !(MNT_UPDATE | MNT_WANTRDWR));
    teardown();
}

#[test]
fn mfs_vfsops_reuse_ffs() {
    assert!(MFS_VFSOPS.vfs_init.is_some());
    assert!(MFS_VFSOPS.vfs_sysctl.is_some());
    assert!(MFS_VOPS.vop_strategy.is_some());
    let _ = mfs_checkexp;
}
