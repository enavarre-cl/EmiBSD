//! Host tests for tmpfs over a real mount: `tmpfs_mount`, the root vnode, files,
//! directories and symbolic links made with `tmpfs_alloc_file`, lookups, `getdents`,
//! resizing a file's object, attribute changes, file handles, `statfs` and `tmpfs_unmount`
//! (the vfs setup is `vfs_subr/tests.rs`'s). The data path (`tmpfs_uiomove`,
//! `tmpfs_zeropg`) maps the object into `kernel_map` and needs the kernel's faults, so it
//! runs in the kernel only.

use core::mem::offset_of;
use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq};

use super::*;
use crate::kern::vfs_lookup::ndinit;
use crate::kern::vfs_subr::{vfs_mount_alloc, vput, vrele};
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::sys::dirent::Dirent;
use crate::sys::mount::{MOUNT_TMPFS, TMPFS_ARGS_VERSION};
use crate::sys::namei::{Componentname, LOOKUP, NiDirp};
use crate::sys::types::Off;
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::{VLNK, VREG, VROOT, Vattr};
use crate::tmpfs::tmpfs::{TMPFS_DIRSEQ_EOF, TMPFS_DIRSEQ_START, VP_TO_TMPFS_DIR};
use crate::tmpfs::tmpfs_subr::{
    tmpfs_alloc_file, tmpfs_chmod, tmpfs_chown, tmpfs_dir_cached, tmpfs_dir_getdents,
    tmpfs_dir_lookup, tmpfs_reg_resize, tmpfs_truncate,
};
use crate::uvm::uvm_aobj::uao_init;

/// The configuration entry of tmpfs (`vfs_init.c`'s, not in `vfsconflist[]` yet).
static TMPFS_CONF: Vfsconf =
    Vfsconf::new(&TMPFS_VFSOPS, MOUNT_TMPFS, 19, MNT_LOCAL, TmpfsArgs::SIZE);

/// Memory, the vfs, the aobj and tmpfs pools, the thread as `curproc`.
fn setup() -> (MutexGuard<'static, ()>, &'static Proc) {
    let (g, p) = crate::kern::vfs_subr::tests::setup();
    Machine::set_curproc(Machine::curcpu(), p);
    crate::kern::kern_rwlock::rw_obj_init();
    uao_init();
    tmpfs_init(&TMPFS_CONF).expect("tmpfs_init");
    TMPFS_BYTES_USED.store(0, Ordering::Relaxed);
    (g, p)
}

/// The bytes of a `struct tmpfs_args` as `sys_mount` copies them in.
fn args(size_max: i64, nodes_max: u64, uid: Uid, mode: Mode) -> [u8; TmpfsArgs::SIZE] {
    let mut b = [0u8; TmpfsArgs::SIZE];
    let mut put = |off: usize, v: &[u8]| b[off..off + v.len()].copy_from_slice(v);
    put(
        offset_of!(TmpfsArgs, ta_version),
        &TMPFS_ARGS_VERSION.to_ne_bytes(),
    );
    put(
        offset_of!(TmpfsArgs, ta_nodes_max),
        &nodes_max.to_ne_bytes(),
    );
    put(offset_of!(TmpfsArgs, ta_size_max), &size_max.to_ne_bytes());
    put(offset_of!(TmpfsArgs, ta_root_uid), &uid.to_ne_bytes());
    put(offset_of!(TmpfsArgs, ta_root_gid), &0u32.to_ne_bytes());
    put(offset_of!(TmpfsArgs, ta_root_mode), &mode.to_ne_bytes());
    b
}

/// `mount -t tmpfs` on "/tmp" with these arguments.
fn mount(p: &'static Proc, data: &mut [u8]) -> Result<&'static Mount, Errno> {
    let mp = vfs_mount_alloc(None, &TMPFS_CONF);
    let mut nd = ndinit(LOOKUP, 0, NiDirp::Sys(b"/tmp"), p);
    tmpfs_mount(mp, b"/tmp", data, &mut nd, p).map(|()| mp)
}

/// A component name for `name`, with the thread's credentials and no pathname buffer.
fn cn(p: &'static Proc, name: &'static [u8]) -> Componentname {
    let mut cnp = Componentname::new();
    cnp.cn_proc = p;
    cnp.cn_cred = p.ucred();
    cnp.cn_nameptr = name.as_ptr();
    cnp.cn_namelen = name.len() as i64;
    cnp
}

/// The attributes `VOP_CREATE`/`VOP_MKDIR`/`VOP_SYMLINK` pass for a new node.
fn vattr(type_: crate::sys::vnode::Vtype, mode: Mode) -> Vattr {
    let mut va = Vattr::new();
    va.va_type = type_;
    va.va_mode = mode;
    va.va_rdev = VNOVAL;
    va
}

/// `getdents` of a directory from `offset` into a buffer of `len` bytes: the names read
/// and the offset the read ended at.
fn getdents(node: &'static TmpfsNode, offset: Off, len: usize) -> (Vec<Vec<u8>>, Off) {
    let mut buf = std::vec![0u8; len];
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
    tmpfs_dir_getdents(node, &mut uio).expect("getdents");
    let used = len - uio.uio_resid;
    let end = uio.uio_offset;

    let mut names = Vec::new();
    let mut off = 0;
    while off < used {
        let d = Dirent::from_bytes(&buf[off..]).expect("a dirent");
        let name = &buf[off + Dirent::NAME_OFFSET..][..usize::from(d.d_namlen)];
        names.push(name.to_vec());
        off += usize::from(d.d_reclen);
    }
    (names, end)
}

#[test]
fn mount_rejects_unset_owners() {
    let (_g, p) = setup();
    let mut data = args(0, 0, VNOVAL as Uid, 0o755);
    assert_eq!(mount(p, &mut data).err(), Some(Errno::EINVAL));
    let mut short = [0u8; 8];
    assert_eq!(mount(p, &mut short).err(), Some(Errno::EINVAL));
}

#[test]
fn files_directories_and_links_live_and_die_with_the_mount() {
    let (_g, p) = setup();
    let mut data = args(64 * 1024, 0, 0, 0o1777);
    let mp = mount(p, &mut data).expect("mount");
    let tmp = VFS_TO_TMPFS(mp);
    assert_eq!(TMPFS_BYTES_USED.load(Ordering::Relaxed), 64 * 1024);
    assert_eq!(tmp.tm_nodes_max.get(), 3 + 64);
    assert_eq!(&mp.mnt_stat.get().f_mntonname[..5], b"/tmp\0");
    assert_eq!(&mp.mnt_stat.get().f_mntfromname[..6], b"tmpfs\0");
    assert!(mp.mnt_flag.get() & MNT_LOCAL != 0);

    // the root: its own parent, mode as given, a vnode flagged VROOT
    let root = tmp.root();
    assert!(
        root.tn_spec
            .tn_dir
            .tn_parent
            .get()
            .is_some_and(|r| ptr::eq(r, root))
    );
    assert_eq!(root.tn_links.get(), 2);
    assert_eq!(root.tn_mode.get(), 0o1777);
    let dvp = tmpfs_root(mp).expect("root vnode");
    assert!(dvp.v_flag.get() & VROOT != 0);
    assert!(ptr::eq(VP_TO_TMPFS_DIR(dvp), root));

    // a file, a directory and a symbolic link
    let fvp =
        tmpfs_alloc_file(dvp, &vattr(VREG, 0o644), &mut cn(p, b"file"), None).expect("create");
    let ddvp = tmpfs_alloc_file(dvp, &vattr(VDIR, 0o755), &mut cn(p, b"dir"), None).expect("mkdir");
    let lvp = tmpfs_alloc_file(
        dvp,
        &vattr(VLNK, 0o777),
        &mut cn(p, b"link"),
        Some(b"file\0"),
    )
    .expect("symlink");
    let (file, sub, link) = (
        VP_TO_TMPFS_NODE(fvp),
        VP_TO_TMPFS_NODE(ddvp),
        VP_TO_TMPFS_NODE(lvp),
    );
    assert_eq!(root.tn_size.get(), 3 * size_of::<TmpfsDirent>() as Off);
    assert_eq!(root.tn_links.get(), 3, "'.', the root's own and dir's '..'");
    assert_eq!(file.tn_links.get(), 1);
    assert_eq!(sub.tn_links.get(), 2);
    assert!(
        sub.tn_spec
            .tn_dir
            .tn_parent
            .get()
            .is_some_and(|r| ptr::eq(r, root))
    );
    assert_eq!(link.link(), b"file");
    assert_eq!(link.tn_size.get(), 4);
    assert_eq!(tmp.tm_nodes_cnt.get(), 4);

    // lookups
    let de = tmpfs_dir_lookup(root, &cn(p, b"dir")).expect("dir's entry");
    assert!(de.td_node.get().is_some_and(|n| ptr::eq(n, sub)));
    assert_eq!(de.td_seq.get(), TMPFS_DIRSEQ_START + 1);
    assert!(tmpfs_dir_lookup(root, &cn(p, b"none")).is_none());
    assert!(tmpfs_dir_cached(file).is_some_and(|de| de.name() == b"file"));

    // getdents: everything at once, then two entries at a time
    let (names, end) = getdents(root, 0, 1024);
    assert_eq!(names, [&b"."[..], b"..", b"file", b"dir", b"link"]);
    assert_eq!(end as u64, TMPFS_DIRSEQ_EOF);
    let (names, end) = getdents(root, 0, 64);
    assert_eq!(names, [&b"."[..], b".."]);
    assert_eq!(end as u64, TMPFS_DIRSEQ_START);
    let (names, end) = getdents(root, end, 64);
    assert_eq!(names, [&b"file"[..], b"dir"]);
    let (names, end) = getdents(root, end, 64);
    assert_eq!(names, [&b"link"[..]]);
    assert_eq!(end as u64, TMPFS_DIRSEQ_EOF);
    let (names, _) = getdents(sub, 0, 1024);
    assert_eq!(names, [&b"."[..], b".."]);

    // the file's object grows and shrinks a page at a time, accounted to the mount
    let used = tmp.tm_bytes_used.get();
    tmpfs_reg_resize(fvp, 3 * PAGE_SIZE as Off).expect("grow");
    assert_eq!(file.tn_spec.tn_reg.tn_aobj_pages.get(), 3);
    assert_eq!(tmp.tm_bytes_used.get(), used + 3 * PAGE_SIZE as u64);
    tmpfs_truncate(fvp, PAGE_SIZE as Off).expect("shrink");
    assert_eq!(file.tn_spec.tn_reg.tn_aobj_pages.get(), 1);
    assert_eq!(file.tn_size.get(), PAGE_SIZE as Off);
    assert_eq!(tmpfs_truncate(fvp, -1), Err(Errno::EINVAL));
    assert_eq!(
        tmpfs_reg_resize(fvp, 1 << 20).err(),
        Some(Errno::ENOSPC),
        "past the 64 KB limit"
    );
    assert_eq!(file.tn_size.get(), PAGE_SIZE as Off);

    // attributes, as root
    let cred = p.ucred();
    tmpfs_chmod(fvp, 0o4600, cred, p).expect("chmod");
    assert_eq!(file.tn_mode.get(), 0o4600);
    tmpfs_chown(fvp, 7, VNOVAL as Gid, cred, p).expect("chown");
    assert_eq!((file.tn_uid.get(), file.tn_gid.get()), (7, 0));

    // file handles
    let mut fid = Fid::default();
    tmpfs_vptofh(fvp, &mut fid).expect("vptofh");
    assert_eq!(usize::from(fid.fid_len), TmpfsFid::SIZE);
    let _ = crate::kern::vfs_vops::VOP_UNLOCK(fvp);
    let again = tmpfs_fhtovp(mp, &fid).expect("fhtovp");
    assert!(ptr::eq(again, fvp));
    vput(again);
    let mut stale = fid;
    stale.fid_data[4] ^= 0xff;
    assert_eq!(tmpfs_fhtovp(mp, &stale).err(), Some(Errno::ESTALE));

    // statfs
    let mut sb = Statfs::new();
    tmpfs_statfs(mp, &mut sb, p).expect("statfs");
    assert_eq!(sb.f_blocks, 16);
    assert_eq!(sb.f_files, 4 + sb.f_ffree);

    // drop the vnodes (fvp is unlocked already), then unmount
    vrele(fvp);
    vput(ddvp);
    vput(lvp);
    vput(dvp);
    tmpfs_unmount(mp, 0, p).expect("unmount");
    assert!(mp.mnt_data.get().is_null());
    assert_eq!(TMPFS_BYTES_USED.load(Ordering::Relaxed), 0);
}

#[test]
fn the_node_limit_is_enospc() {
    let (_g, p) = setup();
    let mut data = args(0, 4, 0, 0o755);
    let mp = mount(p, &mut data).expect("mount");
    let tmp = VFS_TO_TMPFS(mp);
    assert_eq!(tmp.tm_nodes_max.get(), 4);
    let dvp = tmpfs_root(mp).expect("root vnode");

    let mut vps = Vec::new();
    for name in [&b"a"[..], b"b", b"c"] {
        let name: &'static [u8] = std::boxed::Box::leak(name.to_vec().into_boxed_slice());
        vps.push(
            tmpfs_alloc_file(dvp, &vattr(VREG, 0o644), &mut cn(p, name), None).expect("create"),
        );
    }
    assert_eq!(
        tmpfs_alloc_file(dvp, &vattr(VREG, 0o644), &mut cn(p, b"d"), None).err(),
        Some(Errno::ENOSPC)
    );
    assert_eq!(tmp.tm_nodes_cnt.get(), 4);

    for vp in vps {
        vput(vp);
    }
    vput(dvp);
    tmpfs_unmount(mp, 0, p).expect("unmount");
}
