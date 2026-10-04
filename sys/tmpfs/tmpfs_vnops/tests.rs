//! Host tests for the tmpfs vnode operations, through the system calls' own paths
//! (`namei`, `vn_open`, `domkdirat`, `dorenameat`, ...) over a tmpfs mounted as the root
//! (the vfs setup is `vfs_subr/tests.rs`'s, the mount `tmpfs_vfsops/tests.rs`'s). The data
//! path of `tmpfs_read`/`tmpfs_write` (`tmpfs_uiomove`) maps the file's object into
//! `kernel_map` and needs the kernel's faults, so reads and writes of bytes run in the
//! kernel only (the smoke); here a file is resized with `VOP_SETATTR` and read empty.

use std::boxed::Box;
use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq};

use super::*;
use crate::kern::vfs_init::set_rootvnode;
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::{vfs_mount_alloc, vfs_unbusy};
use crate::kern::vfs_syscalls::{
    dolinkat, domkdirat, doreadlinkat, dorenameat, dosymlinkat, dounlinkat,
};
use crate::kern::vfs_vnops::{vn_close, vn_open, vn_rdwr};
use crate::kern::vfs_vops::{
    VOP_GETATTR, VOP_KQFILTER, VOP_OPEN, VOP_PATHCONF, VOP_READDIR, VOP_SETATTR,
};
use crate::sys::dirent::Dirent;
use crate::sys::fcntl::{AT_FDCWD, AT_REMOVEDIR, FREAD, O_CREAT};
use crate::sys::lock::LK_RECURSEFAIL;
use crate::sys::namei::{FOLLOW, LOCKLEAF, LOOKUP, NiDirp};
use crate::sys::param::DEV_BSIZE;
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::Vattr;
use crate::tmpfs::tmpfs_vfsops::tests::{args, setup, tmpfs_conf};
use crate::tmpfs::tmpfs_vfsops::{tmpfs_mount, tmpfs_root};

/// The test setup plus a tmpfs of 1 MB mounted as "/" (mode 0755, owned by root) and made
/// the thread's current directory.
fn setup_root() -> (MutexGuard<'static, ()>, &'static Proc) {
    let (g, p) = setup();
    let mp = vfs_mount_alloc(None, tmpfs_conf());
    let mut data = args(1 << 20, 0, 0, 0o755);
    let mut nd = ndinit(LOOKUP, 0, NiDirp::Sys(b"/"), p);
    tmpfs_mount(mp, b"/", &mut data, &mut nd, p).expect("mount");
    vfs_unbusy(mp);
    let root = tmpfs_root(mp).expect("root");
    set_rootvnode(Some(root));
    p.fd().fd_cdir.set(Some(root));
    vref(root);
    let _ = VOP_UNLOCK(root);
    (g, p)
}

/// A user-space path for the `do*at` functions: the bytes and a NUL.
fn c(path: &str) -> Vec<u8> {
    let mut v = path.as_bytes().to_vec();
    v.push(0);
    v
}

/// `open(path, O_RDWR | O_CREAT, mode)`: the vnode, unlocked and referenced.
fn create(p: &'static Proc, path: &[u8], mode: Mode) -> &'static Vnode {
    let mut nd = ndinit(0, 0, NiDirp::Sys(path), p);
    vn_open(&mut nd, FREAD | FWRITE | O_CREAT, mode).expect("create");
    let vp = nd.ni_vp.expect("a vnode");
    let _ = VOP_UNLOCK(vp);
    vp
}

/// `close` of a vnode `create` gave.
fn close(p: &'static Proc, vp: &'static Vnode) {
    vn_close(vp, FREAD | FWRITE, p.ucred(), Some(p)).expect("close");
}

/// `lstat(path)`: the attributes of the node `path` names.
fn stat(p: &'static Proc, path: &[u8]) -> Result<Vattr, Errno> {
    let mut nd = ndinit(LOOKUP, LOCKLEAF, NiDirp::Sys(path), p);
    namei(&mut nd)?;
    let vp = nd.ni_vp.expect("a vnode");
    let mut va = Vattr::new();
    let error = VOP_GETATTR(vp, &mut va, p.ucred(), p);
    vput(vp);
    error.map(|()| va)
}

/// The names `getdents` lists in the directory `path`, read in one call.
fn names(p: &'static Proc, path: &[u8]) -> Vec<Vec<u8>> {
    let mut nd = ndinit(LOOKUP, LOCKLEAF | FOLLOW, NiDirp::Sys(path), p);
    namei(&mut nd).expect("lookup");
    let vp = nd.ni_vp.expect("a vnode");
    let mut buf = std::vec![0u8; 4096];
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: 4096,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    let mut eof = 0;
    VOP_READDIR(vp, &mut uio, p.ucred(), &mut eof).expect("readdir");
    assert_eq!(eof, 1, "one call reads a small directory to its end");
    let used = 4096 - uio.uio_resid;
    vput(vp);

    let mut names = Vec::new();
    let mut off = 0;
    while off < used {
        let d = Dirent::from_bytes(&buf[off..]).expect("a dirent");
        let name = &buf[off + Dirent::NAME_OFFSET..][..usize::from(d.d_namlen)];
        names.push(name.to_vec());
        off += usize::from(d.d_reclen);
    }
    names
}

/// `truncate(vp, size)` through `VOP_SETATTR`.
fn truncate(p: &'static Proc, vp: &'static Vnode, size: u64) -> Result<(), Errno> {
    let mut va = Vattr::new();
    vattr_null(&mut va);
    va.va_size = size;
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_SETATTR(vp, &mut va, p.ucred(), p);
    let _ = VOP_UNLOCK(vp);
    error
}

#[test]
fn a_file_is_created_found_resized_and_removed() {
    let (_g, p) = setup_root();
    let vp = create(p, b"/f", 0o640);

    let va = stat(p, b"/f").expect("stat");
    assert_eq!(va.va_type, VREG);
    assert_eq!(va.va_mode, 0o640);
    assert_eq!((va.va_nlink, va.va_size, va.va_bytes), (1, 0, 0));
    assert_eq!(va.va_fileid, VP_TO_TMPFS_NODE(vp).tn_id.get());
    assert_eq!(va.va_blocksize, PAGE_SIZE as i64);
    assert_eq!(va.va_rdev, VNOVAL);
    assert_eq!(
        stat(p, b"/").expect("stat /").va_size,
        size_of::<TmpfsDirent>() as u64,
        "one entry"
    );

    // grow and shrink by whole pages (a partial page is zeroed through a mapping)
    truncate(p, vp, 3 * PAGE_SIZE as u64).expect("grow");
    let va = stat(p, b"/f").expect("stat");
    assert_eq!(va.va_size, 3 * PAGE_SIZE as u64);
    assert_eq!(va.va_bytes, 3 * PAGE_SIZE as u64);
    truncate(p, vp, PAGE_SIZE as u64).expect("shrink");
    assert_eq!(stat(p, b"/f").expect("stat").va_size, PAGE_SIZE as u64);

    // unsettable attributes
    let mut va = Vattr::new();
    vattr_null(&mut va);
    va.va_nlink = 3;
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    assert_eq!(VOP_SETATTR(vp, &mut va, p.ucred(), p), Err(Errno::EINVAL));
    let _ = VOP_UNLOCK(vp);

    // a second open finds the same vnode; O_CREAT on an existing file opens it
    let again = create(p, b"/f", 0o600);
    assert!(ptr::eq(again, vp));
    close(p, again);

    assert_eq!(names(p, b"/"), [&b"."[..], b"..", b"f"]);
    dounlinkat(p, AT_FDCWD, c("/f").as_ptr(), 0).expect("unlink");
    assert_eq!(stat(p, b"/f").err(), Some(Errno::ENOENT));
    assert_eq!(names(p, b"/"), [&b"."[..], b".."]);
    let node = VP_TO_TMPFS_NODE(vp);
    assert_eq!(node.tn_links.get(), 0, "open but nameless");
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    assert_eq!(
        VOP_OPEN(vp, FREAD, p.ucred(), p),
        Err(Errno::ENOENT),
        "a file without names cannot be opened again"
    );
    let _ = VOP_UNLOCK(vp);
    close(p, vp);
}

#[test]
fn reads_and_writes_check_their_arguments() {
    let (_g, p) = setup_root();
    let vp = create(p, b"/f", 0o644);
    let mut byte = [0u8; 1];
    let mut resid = 0;

    // an empty file reads nothing; a write of nothing writes nothing
    vn_rdwr(
        UioRw::UIO_READ,
        vp,
        byte.as_mut_ptr().cast(),
        1,
        0,
        UioSeg::UIO_SYSSPACE,
        0,
        p.ucred(),
        Some(&mut resid),
        Some(p),
    )
    .expect("read");
    assert_eq!(resid, 1);
    vn_rdwr(
        UioRw::UIO_WRITE,
        vp,
        byte.as_mut_ptr().cast(),
        0,
        0,
        UioSeg::UIO_SYSSPACE,
        0,
        p.ucred(),
        None,
        Some(p),
    )
    .expect("empty write");
    // past the largest offset
    assert_eq!(
        vn_rdwr(
            UioRw::UIO_WRITE,
            vp,
            byte.as_mut_ptr().cast(),
            1,
            i64::MAX,
            UioSeg::UIO_SYSSPACE,
            0,
            p.ucred(),
            None,
            Some(p),
        ),
        Err(Errno::EFBIG)
    );
    assert_eq!(VP_TO_TMPFS_NODE(vp).tn_size.get(), 0);
    close(p, vp);

    // a directory is neither read nor written as a file
    let root = tmpfs_root_vnode(p);
    for (rw, want) in [
        (UioRw::UIO_READ, Errno::EISDIR),
        (UioRw::UIO_WRITE, Errno::EINVAL),
    ] {
        assert_eq!(
            vn_rdwr(
                rw,
                root,
                byte.as_mut_ptr().cast(),
                1,
                0,
                UioSeg::UIO_SYSSPACE,
                0,
                p.ucred(),
                None,
                Some(p),
            ),
            Err(want)
        );
    }
    vrele(root);
}

/// The root vnode, referenced and unlocked.
fn tmpfs_root_vnode(p: &'static Proc) -> &'static Vnode {
    let mut nd = ndinit(LOOKUP, 0, NiDirp::Sys(b"/"), p);
    namei(&mut nd).expect("lookup /");
    nd.ni_vp.expect("a vnode")
}

#[test]
fn directories_are_made_listed_and_removed() {
    let (_g, p) = setup_root();
    assert_eq!(stat(p, b"/").expect("stat /").va_nlink, 2);

    domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755).expect("mkdir /d");
    domkdirat(p, AT_FDCWD, c("/d/e").as_ptr(), 0o700).expect("mkdir /d/e");
    assert_eq!(
        domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755),
        Err(Errno::EEXIST)
    );
    let d = stat(p, b"/d").expect("stat /d");
    assert_eq!((d.va_type, d.va_mode, d.va_nlink), (VDIR, 0o755, 3));
    assert_eq!(stat(p, b"/").expect("stat /").va_nlink, 3);
    assert_eq!(names(p, b"/d"), [&b"."[..], b"..", b"e"]);
    assert_eq!(
        stat(p, b"/d/e/..").expect("..").va_fileid,
        d.va_fileid,
        "`..` is the parent"
    );

    assert_eq!(
        dounlinkat(p, AT_FDCWD, c("/d").as_ptr(), AT_REMOVEDIR),
        Err(Errno::ENOTEMPTY)
    );
    assert_eq!(
        dounlinkat(p, AT_FDCWD, c("/d").as_ptr(), 0),
        Err(Errno::EPERM),
        "unlink(2) of a directory"
    );
    dounlinkat(p, AT_FDCWD, c("/d/e").as_ptr(), AT_REMOVEDIR).expect("rmdir /d/e");
    assert_eq!(stat(p, b"/d").expect("stat /d").va_nlink, 2);
    dounlinkat(p, AT_FDCWD, c("/d").as_ptr(), AT_REMOVEDIR).expect("rmdir /d");
    assert_eq!(stat(p, b"/d").err(), Some(Errno::ENOENT));
    assert_eq!(stat(p, b"/").expect("stat /").va_nlink, 2);
    assert_eq!(names(p, b"/"), [&b"."[..], b".."]);
}

#[test]
fn renames_move_entries_within_and_across_directories() {
    let (_g, p) = setup_root();
    let vp = create(p, b"/a", 0o644);
    close(p, vp);
    let id = stat(p, b"/a").expect("stat /a").va_fileid;
    domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755).expect("mkdir /d");
    domkdirat(p, AT_FDCWD, c("/d2").as_ptr(), 0o755).expect("mkdir /d2");

    // within a directory: a new name, a longer one
    dorenameat(p, AT_FDCWD, c("/a").as_ptr(), AT_FDCWD, c("/bee").as_ptr()).expect("rename");
    assert_eq!(stat(p, b"/a").err(), Some(Errno::ENOENT));
    assert_eq!(stat(p, b"/bee").expect("stat /bee").va_fileid, id);

    // across directories
    dorenameat(
        p,
        AT_FDCWD,
        c("/bee").as_ptr(),
        AT_FDCWD,
        c("/d/c").as_ptr(),
    )
    .expect("rename");
    assert_eq!(stat(p, b"/d/c").expect("stat /d/c").va_fileid, id);
    assert_eq!(names(p, b"/"), [&b"."[..], b"..", b"d", b"d2"]);
    assert_eq!(names(p, b"/d"), [&b"."[..], b"..", b"c"]);

    // over an existing file, which goes
    let vp = create(p, b"/d/x", 0o644);
    close(p, vp);
    dorenameat(
        p,
        AT_FDCWD,
        c("/d/c").as_ptr(),
        AT_FDCWD,
        c("/d/x").as_ptr(),
    )
    .expect("rename");
    assert_eq!(stat(p, b"/d/x").expect("stat /d/x").va_fileid, id);
    assert_eq!(names(p, b"/d"), [&b"."[..], b"..", b"x"]);

    // a directory into another one: the link counts and `..` follow it
    dorenameat(p, AT_FDCWD, c("/d").as_ptr(), AT_FDCWD, c("/d2/d").as_ptr()).expect("rename");
    let d2 = stat(p, b"/d2").expect("stat /d2");
    assert_eq!(d2.va_nlink, 3);
    assert_eq!(stat(p, b"/").expect("stat /").va_nlink, 3);
    assert_eq!(stat(p, b"/d2/d/..").expect("..").va_fileid, d2.va_fileid);
    assert_eq!(stat(p, b"/d2/d/x").expect("stat").va_fileid, id);

    // a directory over a non-empty one, a file over a directory, a directory into itself
    domkdirat(p, AT_FDCWD, c("/e").as_ptr(), 0o755).expect("mkdir /e");
    assert_eq!(
        dorenameat(p, AT_FDCWD, c("/e").as_ptr(), AT_FDCWD, c("/d2").as_ptr()),
        Err(Errno::ENOTEMPTY)
    );
    assert_eq!(
        dorenameat(
            p,
            AT_FDCWD,
            c("/d2/d/x").as_ptr(),
            AT_FDCWD,
            c("/e").as_ptr()
        ),
        Err(Errno::EISDIR)
    );
    assert_eq!(
        dorenameat(
            p,
            AT_FDCWD,
            c("/d2").as_ptr(),
            AT_FDCWD,
            c("/d2/d/z").as_ptr()
        ),
        Err(Errno::EINVAL)
    );
    // an empty directory over an empty one
    domkdirat(p, AT_FDCWD, c("/f").as_ptr(), 0o755).expect("mkdir /f");
    dorenameat(p, AT_FDCWD, c("/e").as_ptr(), AT_FDCWD, c("/f").as_ptr()).expect("rename");
    assert_eq!(names(p, b"/"), [&b"."[..], b"..", b"d2", b"f"]);
    assert_eq!(stat(p, b"/").expect("stat /").va_nlink, 4);
}

#[test]
fn hard_links_count_names() {
    let (_g, p) = setup_root();
    let vp = create(p, b"/f", 0o644);
    close(p, vp);
    domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755).expect("mkdir /d");

    dolinkat(
        p,
        AT_FDCWD,
        c("/f").as_ptr(),
        AT_FDCWD,
        c("/d/g").as_ptr(),
        0,
    )
    .expect("link");
    let f = stat(p, b"/f").expect("stat /f");
    assert_eq!(f.va_nlink, 2);
    assert_eq!(stat(p, b"/d/g").expect("stat /d/g").va_fileid, f.va_fileid);
    assert_eq!(
        dolinkat(
            p,
            AT_FDCWD,
            c("/f").as_ptr(),
            AT_FDCWD,
            c("/d/g").as_ptr(),
            0
        ),
        Err(Errno::EEXIST)
    );
    assert_eq!(
        dolinkat(p, AT_FDCWD, c("/d").as_ptr(), AT_FDCWD, c("/h").as_ptr(), 0),
        Err(Errno::EPERM),
        "no links to directories"
    );

    dounlinkat(p, AT_FDCWD, c("/f").as_ptr(), 0).expect("unlink /f");
    assert_eq!(stat(p, b"/d/g").expect("stat /d/g").va_nlink, 1);
    dounlinkat(p, AT_FDCWD, c("/d/g").as_ptr(), 0).expect("unlink /d/g");
    assert_eq!(names(p, b"/d"), [&b"."[..], b".."]);
}

#[test]
fn symbolic_links_read_back_and_are_followed() {
    let (_g, p) = setup_root();
    domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755).expect("mkdir /d");
    dosymlinkat(p, c("d").as_ptr(), AT_FDCWD, c("/l").as_ptr()).expect("symlink");

    let l = stat(p, b"/l").expect("lstat /l");
    assert_eq!((l.va_type, l.va_size), (VLNK, 1));
    let mut buf = [0u8; 16];
    let mut retval = [0; 2];
    doreadlinkat(
        p,
        AT_FDCWD,
        c("/l").as_ptr(),
        buf.as_mut_ptr(),
        buf.len(),
        &mut retval,
    )
    .expect("readlink");
    assert_eq!(&buf[..retval[0] as usize], b"d");

    // followed in the middle of a path
    let d = stat(p, b"/d").expect("stat /d").va_fileid;
    assert_eq!(stat(p, b"/l/.").expect("stat /l/.").va_fileid, d);

    // a long target
    let target = [b'x'; 200];
    let mut t = target.to_vec();
    t.push(0);
    dosymlinkat(p, t.as_ptr(), AT_FDCWD, c("/long").as_ptr()).expect("symlink");
    let mut buf = [0u8; 300];
    doreadlinkat(
        p,
        AT_FDCWD,
        c("/long").as_ptr(),
        buf.as_mut_ptr(),
        buf.len(),
        &mut retval,
    )
    .expect("readlink");
    assert_eq!(&buf[..retval[0] as usize], &target[..]);
    dounlinkat(p, AT_FDCWD, c("/long").as_ptr(), 0).expect("unlink");
}

#[test]
fn the_vnode_lock_recurses_as_ufs_does_and_vnd_reads_under_it() {
    let (_g, p) = setup_root();
    let mut nd = ndinit(0, 0, NiDirp::Sys(b"/new.img"), p);
    vn_open(&mut nd, FREAD | FWRITE | O_CREAT, 0o600).expect("open");
    let vp = nd.ni_vp.expect("a vnode");
    let lock = &VP_TO_TMPFS_NODE(vp).tn_vlock;

    // vn_open returns the vnode locked by this thread
    assert_eq!(VOP_ISLOCKED(vp), LK_EXCLUSIVE);
    assert_eq!(lock.rrwl_wcnt.get(), 1);

    // the same thread takes it again: rrw_enter counts, as ufs_lock does
    vn_lock(vp, LK_EXCLUSIVE | LK_RETRY).expect("recursive lock");
    assert_eq!(lock.rrwl_wcnt.get(), 2);
    assert_eq!(
        vn_lock(vp, LK_EXCLUSIVE | LK_RECURSEFAIL),
        Err(Errno::EDEADLK)
    );
    let _ = VOP_UNLOCK(vp);
    assert_eq!(lock.rrwl_wcnt.get(), 1);
    assert_eq!(VOP_ISLOCKED(vp), LK_EXCLUSIVE, "still held once");

    // vnd(4)'s VNDIOCSET: vndsetcred reads the file through vn_rdwr (which locks it
    // again) while vn_open's lock is held, and the lock is back to one level afterwards
    let mut buf = [0u8; DEV_BSIZE];
    let mut resid = 0;
    vn_rdwr(
        UioRw::UIO_READ,
        vp,
        buf.as_mut_ptr().cast(),
        DEV_BSIZE,
        0,
        UioSeg::UIO_SYSSPACE,
        0,
        p.ucred(),
        Some(&mut resid),
        Some(p),
    )
    .expect("read under the open lock");
    assert_eq!(resid, DEV_BSIZE, "an empty file reads nothing");
    assert_eq!(lock.rrwl_wcnt.get(), 1);
    assert_eq!(VOP_ISLOCKED(vp), LK_EXCLUSIVE);

    let _ = VOP_UNLOCK(vp);
    assert_eq!(VOP_ISLOCKED(vp), 0);
    close(p, vp);
}

#[test]
fn pathconf_and_kqueue_filters() {
    let (_g, p) = setup_root();
    let vp = create(p, b"/f", 0o644);
    let mut v: Register = 0;
    VOP_PATHCONF(vp, _PC_NAME_MAX, &mut v).expect("pathconf");
    assert_eq!(v, TMPFS_MAXNAMLEN as Register);
    VOP_PATHCONF(vp, _PC_LINK_MAX, &mut v).expect("pathconf");
    assert_eq!(v, LINK_MAX as Register);
    VOP_PATHCONF(vp, _PC_FILESIZEBITS, &mut v).expect("pathconf");
    assert_eq!(v, 64);
    assert_eq!(VOP_PATHCONF(vp, 9999, &mut v), Err(Errno::EINVAL));

    // a vnode filter hooks onto the vnode's list and comes off it
    let kn: &'static Knote = Box::leak(Box::new(Knote::new()));
    kn.kn_filter().set(EVFILT_VNODE);
    VOP_KQFILTER(vp, 0, kn).expect("kqfilter");
    assert!(
        kn.kn_fop
            .get()
            .is_some_and(|f| ptr::eq(f, &TMPFSVNODE_FILTOPS))
    );
    assert!(ptr::eq(kn_vnode(kn), vp));
    assert!(vp.v_klist.kl_list.first().is_some());
    filt_tmpfsdetach(kn);
    assert!(vp.v_klist.kl_list.first().is_none());
    let bad: &'static Knote = Box::leak(Box::new(Knote::new()));
    bad.kn_filter().set(-100);
    assert_eq!(VOP_KQFILTER(vp, 0, bad), Err(Errno::EINVAL));

    // the events: only the subscribed notes are recorded; revocation ends it
    let kn = Knote::new();
    kn.kn_sfflags.set(NOTE_WRITE | NOTE_RENAME);
    assert!(!filt_tmpfsvnode(&kn, i64::from(NOTE_LINK)));
    assert!(filt_tmpfsvnode(&kn, i64::from(NOTE_RENAME)));
    assert_eq!(kn.kn_fflags().get(), NOTE_RENAME);
    assert!(filt_tmpfsvnode(&kn, i64::from(NOTE_REVOKE)) && kn.has_flags(EV_EOF));
    let kn = Knote::new();
    assert!(filt_tmpfswrite(&kn, 0) && kn.kn_data().get() == 0);
    assert!(filt_tmpfswrite(&kn, i64::from(NOTE_REVOKE)));
    assert!(kn.has_flags(EV_EOF) && kn.has_flags(EV_ONESHOT));
    close(p, vp);
}
