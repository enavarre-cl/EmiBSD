//! Host tests for the msdosfs vnode operations, through the system calls' own paths
//! (`namei`, `vn_open`, `vn_rdwr`, `domkdirat`, `dorenameat`, `dounlinkat`, ...) over a FAT
//! image (`mkfat`) on the fake disk of `msdosfs_vfsops.rs`'s tests, mounted as the root; one
//! test mounts it on a directory of a tmpfs root and looks up across the mount point. What
//! the operations write is checked on the disk image after a `msdosfs_sync`.

use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq, assert_ne};

use super::*;
use crate::kern::kern_prot::crget;
use crate::kern::vfs_init::set_rootvnode;
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::{vattr_null, vref};
use crate::kern::vfs_syscalls::{domkdirat, dorenameat, dounlinkat};
use crate::kern::vfs_vnops::{vn_close, vn_open, vn_rdwr};
use crate::kern::vfs_vops::{VOP_GETATTR, VOP_KQFILTER, VOP_PATHCONF, VOP_READDIR, VOP_SETATTR};
use crate::msdosfs::msdosfs_vfsops::tests::mkfat::{self, Dir, Image};
use crate::msdosfs::msdosfs_vfsops::tests::{DISK, mount, setup};
use crate::msdosfs::msdosfs_vfsops::{msdosfs_root, msdosfs_statfs, msdosfs_sync};
use crate::sys::fcntl::{AT_FDCWD, AT_REMOVEDIR, FREAD, FWRITE, O_CREAT};
use crate::sys::lock::LK_RECURSEFAIL;
use crate::sys::namei::{FOLLOW, LOCKLEAF, LOOKUP, NiDirp};
use crate::sys::time::Timespec;
use crate::sys::uio::{Iovec, UioRw, UioSeg};
use crate::sys::vnode::Vattr;

/// The long-named file of the smoke image, and its DOS name.
const FATNAME: &[u8] = b"m10c-fat.txt";
const FATSHORT: &[u8; 11] = b"M10C-FATTXT";
const FATDATA: &[u8] = b"m10c-fat-42";

/// `n` bytes of a pattern that does not repeat within a cluster.
fn pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

/// A FAT12 image: the smoke's long-named file, a 3000-byte file (6 clusters) and a
/// subdirectory holding a long-named file.
fn sample() -> Vec<u8> {
    let mut img = Image::new(mkfat::FAT12_1M);
    let root = img.root();
    img.add_file(root, Some(FATNAME), FATSHORT, FATDATA);
    img.add_file(root, None, b"README  TXT", &pattern(3000));
    let sub = img.mkdir(root, b"SUBDIR     ", 1);
    img.add_file(
        Dir::Clust(sub),
        Some(b"inner file.dat"),
        b"INNERF~1DAT",
        b"inside",
    );
    img.finish()
}

/// The test setup plus `image` mounted read-write as "/" (long names, mask 0777, owned by
/// root) and made the thread's current directory. `mnt_stat` is filled as `sys_mount` does.
fn setup_root(image: Vec<u8>) -> (MutexGuard<'static, ()>, &'static Proc, &'static Mount) {
    let (g, p) = setup(image);
    let mp = mount(p, false);
    let pmp = vfstomsdosfs(mp);
    pmp.pm_flags
        .set(pmp.pm_flags.get() | MSDOSFSMNT_LONGNAME as u32);
    pmp.pm_mask.set(0o777);
    let mut sb = mp.mnt_stat.get();
    msdosfs_statfs(mp, &mut sb, p).expect("statfs");
    mp.mnt_stat.set(sb);
    let root = msdosfs_root(mp).expect("root");
    set_rootvnode(Some(root));
    p.fd().fd_cdir.set(Some(root));
    vref(root);
    let _ = VOP_UNLOCK(root);
    (g, p, mp)
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

/// The vnode `path` names, unlocked and referenced.
fn lookup(p: &'static Proc, path: &[u8]) -> Result<&'static Vnode, Errno> {
    let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(path), p);
    namei(&mut nd)?;
    Ok(nd.ni_vp.expect("a vnode"))
}

/// `stat(path)`.
fn stat(p: &'static Proc, path: &[u8]) -> Result<Vattr, Errno> {
    let mut nd = ndinit(LOOKUP, LOCKLEAF | FOLLOW, NiDirp::Sys(path), p);
    namei(&mut nd)?;
    let vp = nd.ni_vp.expect("a vnode");
    let mut va = Vattr::new();
    let error = VOP_GETATTR(vp, &mut va, p.ucred(), p);
    vput(vp);
    error.map(|()| va)
}

/// `vn_rdwr` of `len` bytes at `off` of `vp` (unlocked): the bytes moved. A write runs
/// without a thread (`uio_procp`), as the test process has no resource limits for
/// `vn_fsizechk` to look at.
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
fn read_file(p: &'static Proc, path: &[u8]) -> Vec<u8> {
    let size = stat(p, path).expect("stat").va_size as usize;
    let vp = lookup(p, path).expect("lookup");
    let mut buf = std::vec![0u8; size + 100];
    let n = rdwr(p, UioRw::UIO_READ, vp, &mut buf, 0);
    vrele(vp);
    buf.truncate(n);
    buf
}

/// `truncate(vp, size)` through `VOP_SETATTR`.
fn truncate(p: &'static Proc, vp: &'static Vnode, size: u64) -> Result<(), Errno> {
    let mut va = Vattr::new();
    vattr_null(&mut va);
    va.va_size = size;
    setattr(p, vp, &mut va)
}

/// `VOP_SETATTR` with the vnode locked.
fn setattr(p: &'static Proc, vp: &'static Vnode, va: &mut Vattr) -> Result<(), Errno> {
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_SETATTR(vp, va, p.ucred(), p);
    let _ = VOP_UNLOCK(vp);
    error
}

/// One `struct dirent` `getdents` returned.
#[derive(Debug, PartialEq, Eq)]
struct Ent {
    name: Vec<u8>,
    off: i64,
    fileno: u64,
    typ: u8,
}

/// One `VOP_READDIR` of the directory `path` at `offset` into a buffer of `size` bytes: the
/// entries, the new offset and the EOF flag.
fn readdir(p: &'static Proc, path: &[u8], offset: i64, size: usize) -> (Vec<Ent>, i64, i32) {
    let mut nd = ndinit(LOOKUP, LOCKLEAF | FOLLOW, NiDirp::Sys(path), p);
    namei(&mut nd).expect("lookup");
    let vp = nd.ni_vp.expect("a vnode");
    let mut buf = std::vec![0u8; size];
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
    VOP_READDIR(vp, &mut uio, p.ucred(), &mut eof).expect("readdir");
    let used = size - uio.uio_resid;
    let newoff = uio.uio_offset;
    vput(vp);

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
            typ: d.d_type,
        });
        off += usize::from(d.d_reclen);
    }
    (ents, newoff, eof)
}

/// The names of `readdir` entries.
fn names(ents: &[Ent]) -> Vec<&[u8]> {
    ents.iter().map(|e| &e.name[..]).collect()
}

/// Writes everything back (`msdosfs_sync` with `MNT_WAIT`) and returns the disk as an image.
fn synced(p: &'static Proc, mp: &'static Mount) -> Image {
    msdosfs_sync(mp, MNT_WAIT, 0, p.ucred(), p).expect("sync");
    let mut img = Image::new(mkfat::FAT12_1M);
    img.disk = DISK.lock().unwrap_or_else(|e| e.into_inner()).clone();
    img
}

/// The short entry named `short` in `dir` of `img`.
fn entry(img: &Image, dir: Dir, short: &[u8; 11]) -> Option<Direntry> {
    img.slots(dir)
        .into_iter()
        .map(|o| *Direntry::at(&img.disk, o))
        .find(|e| e.name11() == *short)
}

/// The data of the file whose entry is `e`, following its cluster chain on `img`.
fn contents(img: &Image, e: &Direntry) -> Vec<u8> {
    let size = getulong(&e.deFileSize) as usize;
    let mut cn = u32::from(getushort(&e.deStartCluster));
    let mut out = Vec::new();
    while out.len() < size {
        let o = img.clust_off(cn);
        out.extend_from_slice(&img.disk[o..o + img.bpc()]);
        cn = img.fat_get(cn);
    }
    out.truncate(size);
    out
}

#[test]
fn the_long_named_file_is_found_and_read() {
    let (_g, p, _mp) = setup_root(sample());

    // the smoke: cat /mnt/m10c-fat.txt
    assert_eq!(read_file(p, FATNAME), FATDATA);
    // a lookup by the DOS name finds the same file, case-insensitively
    assert_eq!(read_file(p, b"/m10c-fat.txt"), FATDATA);
    assert_eq!(read_file(p, b"/M10C-FAT.TXT"), FATDATA);
    // several clusters: read whole (bread_cluster), and from the middle of a cluster
    assert_eq!(read_file(p, b"/readme.txt"), pattern(3000));
    let vp = lookup(p, b"/README.TXT").expect("lookup");
    let mut buf = [0u8; 700];
    assert_eq!(rdwr(p, UioRw::UIO_READ, vp, &mut buf, 1000), 700);
    assert_eq!(&buf[..], &pattern(3000)[1000..1700]);
    // a read past the end moves nothing
    assert_eq!(rdwr(p, UioRw::UIO_READ, vp, &mut buf, 3000), 0);
    vrele(vp);
    // through a subdirectory
    assert_eq!(read_file(p, b"/subdir/inner file.dat"), b"inside");
    assert_eq!(lookup(p, b"/nonesuch").err(), Some(Errno::ENOENT));
}

#[test]
fn readdir_lists_long_names_with_cookies_to_resume_from() {
    let (_g, p, _mp) = setup_root(sample());

    let (ents, off, eof) = readdir(p, b"/", 0, 4096);
    assert_eq!(
        names(&ents),
        [&b"."[..], b"..", FATNAME, b"README.TXT", b"SUBDIR"]
    );
    // "." and ".." are simulated in the root (fileno 1); the cookies then count 32-byte
    // slots past them, the long name entry included
    assert_eq!((ents[0].fileno, ents[0].typ, ents[0].off), (1, DT_DIR, 32));
    assert_eq!((ents[1].fileno, ents[1].off), (1, 64));
    assert_eq!((ents[2].off, ents[2].typ), (64 + 2 * 32, DT_REG));
    assert_eq!(ents[3].off, 64 + 3 * 32);
    assert_eq!((ents[4].off, ents[4].typ), (64 + 4 * 32, DT_DIR));
    assert_eq!(off, 64 + 4 * 32, "stopped at the first never-used slot");
    assert_eq!(eof, 0, "the fixed root directory goes on");
    // d_fileno is what getattr calls va_fileid
    for (i, path) in [
        (2, &b"/m10c-fat.txt"[..]),
        (3, b"/README.TXT"),
        (4, b"/SUBDIR"),
    ] {
        assert_eq!(stat(p, path).expect("stat").va_fileid, ents[i].fileno);
    }

    // resume from each cookie
    for i in 0..4 {
        let (rest, _, _) = readdir(p, b"/", ents[i].off, 4096);
        assert_eq!(names(&rest), names(&ents[i + 1..]), "from cookie {i}");
    }

    // a buffer too small for the long-named entry ends before its long name entry, so the
    // next call reads the long name again
    // (96 bytes: "." and ".." take 64, the 32 left are short of the long name's 40)
    let small = 96;
    assert!(small - 2 * dirent_size_of(2) < dirent_size_of(FATNAME.len()));
    let (first, off, _) = readdir(p, b"/", 0, small);
    assert_eq!(names(&first), [&b"."[..], b".."]);
    assert_eq!(off, 64, "the long name's first slot");
    let (next, _, _) = readdir(p, b"/", off, 4096);
    assert_eq!(names(&next)[0], FATNAME);

    // a subdirectory has real "." and ".." entries
    let (sub, _, _) = readdir(p, b"/subdir", 0, 4096);
    assert_eq!(names(&sub), [&b"."[..], b"..", b"inner file.dat"]);
    assert_eq!(sub[1].fileno, 1, "`..` of a directory in the root");

    // bad offsets and buffers, and a regular file
    let vp = lookup(p, b"/").expect("root");
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let mut buf = [0u8; 64];
    for (offset, len, want) in [(3, 64, Errno::EINVAL), (0, 16, Errno::EINVAL)] {
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
        assert_eq!(VOP_READDIR(vp, &mut uio, p.ucred(), &mut eof), Err(want));
    }
    vput(vp);
}

/// `DIRENT_SIZE` of an entry with a name of `namlen` bytes.
fn dirent_size_of(namlen: usize) -> usize {
    crate::sys::dirent::dirent_recsize(namlen)
}

#[test]
fn files_are_created_written_read_back_and_truncated() {
    let (_g, p, mp) = setup_root(sample());
    let pmp = vfstomsdosfs(mp);
    let free0 = pmp.pm_freeclustercount.get();

    let vp = create(p, b"/A Longer Name.bin", 0o644);
    let data = pattern(5000);
    let mut w = data.clone();
    assert_eq!(rdwr(p, UioRw::UIO_WRITE, vp, &mut w, 0), 5000);
    assert_eq!(
        free0 - pmp.pm_freeclustercount.get(),
        10,
        "5000 bytes in 512-byte clusters"
    );
    assert_eq!(read_file(p, b"/a longer name.bin"), data);

    // overwrite inside, then append past the end
    let mut w = std::vec![0xaa; 100];
    rdwr(p, UioRw::UIO_WRITE, vp, &mut w, 500);
    let mut want = data.clone();
    want[500..600].fill(0xaa);
    assert_eq!(read_file(p, b"/A Longer Name.bin"), want);
    // a write beyond EOF fills the hole with zeroes (DOS has no holes)
    let mut w = std::vec![0x55; 10];
    rdwr(p, UioRw::UIO_WRITE, vp, &mut w, 6000);
    want.resize(6000, 0);
    want.extend_from_slice(&[0x55; 10]);
    assert_eq!(read_file(p, b"/A Longer Name.bin"), want);

    truncate(p, vp, 100).expect("truncate");
    assert_eq!(stat(p, b"/A Longer Name.bin").expect("stat").va_size, 100);
    assert_eq!(read_file(p, b"/A Longer Name.bin"), &want[..100]);
    assert_eq!(free0 - pmp.pm_freeclustercount.get(), 1);
    close(p, vp);

    // on the disk: a long name entry, the short entry and the data
    let img = synced(p, mp);
    let mut short = [0u8; 11];
    assert_ne!(
        crate::msdosfs::msdosfs_conv::unix2dosfn(b"A Longer Name.bin", &mut short, 1),
        0
    );
    let e = entry(&img, Dir::Root, &short).expect("the entry on disk");
    assert_eq!(getulong(&e.deFileSize), 100);
    assert_eq!(contents(&img, &e), &want[..100]);
    assert_eq!(e.deAttributes & ATTR_ARCHIVE, ATTR_ARCHIVE);

    // a zero-length file: a hashed file id with the top bit set, the same in readdir
    close(p, create(p, b"/empty", 0o644));
    let va = stat(p, b"/empty").expect("stat");
    assert_eq!(va.va_size, 0);
    assert_ne!(va.va_fileid & 0x8000_0000, 0);
    let (ents, _, _) = readdir(p, b"/", 0, 4096);
    let empty = ents.iter().find(|e| e.name == b"empty").expect("listed");
    assert_eq!(empty.fileno, va.va_fileid);

    // directories cannot be written
    let root = lookup(p, b"/").expect("root");
    let mut b = [0u8; 4];
    assert_eq!(
        vn_rdwr(
            UioRw::UIO_WRITE,
            root,
            b.as_mut_ptr().cast(),
            4,
            0,
            UioSeg::UIO_SYSSPACE,
            0,
            p.ucred(),
            None,
            Some(p),
        ),
        Err(Errno::EISDIR)
    );
    vrele(root);
}

#[test]
fn directories_are_made_listed_and_removed() {
    let (_g, p, mp) = setup_root(sample());

    domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755).expect("mkdir /d");
    domkdirat(p, AT_FDCWD, c("/d/a deeper one").as_ptr(), 0o755).expect("mkdir deeper");
    assert_eq!(
        domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755),
        Err(Errno::EEXIST)
    );
    let d = stat(p, b"/d").expect("stat /d");
    assert_eq!(d.va_type, VDIR);
    let (ents, _, _) = readdir(p, b"/d", 0, 4096);
    assert_eq!(names(&ents), [&b"."[..], b"..", b"a deeper one"]);
    assert_eq!(ents[0].fileno, d.va_fileid, "`.` is the directory itself");
    assert_eq!(ents[1].fileno, 1, "`..` is the root");
    assert_eq!(
        stat(p, b"/d/a deeper one/..").expect("..").va_fileid,
        d.va_fileid
    );

    // on the disk: "." and ".." of the new directory point at it and at the root
    let img = synced(p, mp);
    let e = entry(&img, Dir::Root, b"D          ").expect("/d on disk");
    assert_eq!(e.deAttributes, ATTR_DIRECTORY);
    let cn = u32::from(getushort(&e.deStartCluster));
    let o = img.clust_off(cn);
    let dot = Direntry::at(&img.disk, o);
    let dotdot = Direntry::at(&img.disk, o + 32);
    assert_eq!(dot.name11(), *b".          ");
    assert_eq!(u32::from(getushort(&dot.deStartCluster)), cn);
    assert_eq!(dotdot.name11(), *b"..         ");
    assert_eq!(getushort(&dotdot.deStartCluster), 0);

    // a file in it; rmdir refuses a non-empty directory, unlink refuses a directory
    close(p, create(p, b"/d/f", 0o644));
    assert_eq!(
        dounlinkat(p, AT_FDCWD, c("/d").as_ptr(), AT_REMOVEDIR),
        Err(Errno::ENOTEMPTY)
    );
    assert_eq!(
        dounlinkat(p, AT_FDCWD, c("/d/a deeper one").as_ptr(), 0),
        Err(Errno::EPERM)
    );
    dounlinkat(p, AT_FDCWD, c("/d/a deeper one").as_ptr(), AT_REMOVEDIR).expect("rmdir");
    dounlinkat(p, AT_FDCWD, c("/d/f").as_ptr(), 0).expect("unlink /d/f");
    assert_eq!(stat(p, b"/d/f").err(), Some(Errno::ENOENT));
    dounlinkat(p, AT_FDCWD, c("/d").as_ptr(), AT_REMOVEDIR).expect("rmdir /d");
    assert_eq!(stat(p, b"/d").err(), Some(Errno::ENOENT));
    let (ents, _, _) = readdir(p, b"/", 0, 4096);
    assert_eq!(
        names(&ents),
        [&b"."[..], b"..", FATNAME, b"README.TXT", b"SUBDIR"]
    );
}

#[test]
fn files_are_removed_and_their_clusters_freed() {
    let (_g, p, mp) = setup_root(sample());
    let pmp = vfstomsdosfs(mp);
    let free0 = pmp.pm_freeclustercount.get();

    dounlinkat(p, AT_FDCWD, c("/README.TXT").as_ptr(), 0).expect("unlink");
    assert_eq!(stat(p, b"/readme.txt").err(), Some(Errno::ENOENT));
    assert_eq!(pmp.pm_freeclustercount.get() - free0, 6, "its 6 clusters");
    dounlinkat(p, AT_FDCWD, c("/m10c-fat.txt").as_ptr(), 0).expect("unlink long");
    let img = synced(p, mp);
    assert!(entry(&img, Dir::Root, b"README  TXT").is_none());
    assert!(entry(&img, Dir::Root, FATSHORT).is_none());
    // the long name entry went with it
    assert_eq!(img.disk[img.slots(Dir::Root)[0]], SLOT_DELETED);
    let (ents, _, _) = readdir(p, b"/", 0, 4096);
    assert_eq!(names(&ents), [&b"."[..], b"..", b"SUBDIR"]);
}

#[test]
fn renames_move_entries_within_and_across_directories() {
    let (_g, p, mp) = setup_root(sample());

    // within a directory, to a long name
    dorenameat(
        p,
        AT_FDCWD,
        c("/README.TXT").as_ptr(),
        AT_FDCWD,
        c("/now a long name.txt").as_ptr(),
    )
    .expect("rename");
    assert_eq!(stat(p, b"/README.TXT").err(), Some(Errno::ENOENT));
    assert_eq!(read_file(p, b"/now a long name.txt"), pattern(3000));

    // into a subdirectory
    dorenameat(
        p,
        AT_FDCWD,
        c("/now a long name.txt").as_ptr(),
        AT_FDCWD,
        c("/subdir/moved").as_ptr(),
    )
    .expect("rename across");
    assert_eq!(read_file(p, b"/subdir/moved"), pattern(3000));
    let (ents, _, _) = readdir(p, b"/subdir", 0, 4096);
    assert_eq!(
        names(&ents),
        [&b"."[..], b"..", b"inner file.dat", b"moved"]
    );

    // over an existing file, which goes away
    dorenameat(
        p,
        AT_FDCWD,
        c("/m10c-fat.txt").as_ptr(),
        AT_FDCWD,
        c("/subdir/moved").as_ptr(),
    )
    .expect("rename over");
    assert_eq!(read_file(p, b"/subdir/moved"), FATDATA);
    assert_eq!(stat(p, b"/m10c-fat.txt").err(), Some(Errno::ENOENT));

    // a directory to a new parent: its ".." follows
    domkdirat(p, AT_FDCWD, c("/newparent").as_ptr(), 0o755).expect("mkdir");
    dorenameat(
        p,
        AT_FDCWD,
        c("/subdir").as_ptr(),
        AT_FDCWD,
        c("/newparent/sub").as_ptr(),
    )
    .expect("rename dir");
    let np = stat(p, b"/newparent").expect("stat").va_fileid;
    assert_eq!(stat(p, b"/newparent/sub/..").expect("..").va_fileid, np);
    assert_eq!(read_file(p, b"/newparent/sub/moved"), FATDATA);
    let img = synced(p, mp);
    let sub = entry(&img, Dir::Clust(np as u32), b"SUB        ").expect("sub on disk");
    let o = img.clust_off(u32::from(getushort(&sub.deStartCluster)));
    assert_eq!(
        u64::from(getushort(&Direntry::at(&img.disk, o + 32).deStartCluster)),
        np,
        "`..` on the disk"
    );

    // not into itself, and not "."
    assert_eq!(
        dorenameat(
            p,
            AT_FDCWD,
            c("/newparent").as_ptr(),
            AT_FDCWD,
            c("/newparent/sub/x").as_ptr(),
        ),
        Err(Errno::EINVAL)
    );
    // a file over a directory
    close(p, create(p, b"/plain", 0o644));
    assert_eq!(
        dorenameat(
            p,
            AT_FDCWD,
            c("/plain").as_ptr(),
            AT_FDCWD,
            c("/newparent").as_ptr(),
        ),
        Err(Errno::EISDIR)
    );
}

#[test]
fn attributes_follow_the_mount_and_the_dos_entry() {
    let (_g, p, mp) = setup_root(sample());
    let pmp = vfstomsdosfs(mp);
    pmp.pm_mask.set(0o755);
    pmp.pm_uid.set(0);
    pmp.pm_gid.set(0);

    let f = stat(p, b"/README.TXT").expect("stat");
    assert_eq!((f.va_type, f.va_mode, f.va_nlink), (VREG, 0o644, 1));
    assert_eq!((f.va_size, f.va_bytes, f.va_blocksize), (3000, 3072, 512));
    assert_eq!(f.va_flags, 0, "the archive bit is set: not SF_ARCHIVED");
    let d = stat(p, b"/SUBDIR").expect("stat");
    assert_eq!(
        (d.va_type, d.va_mode),
        (VDIR, 0o755),
        "S_IFDIR is masked away"
    );
    let r = stat(p, b"/").expect("stat /");
    assert_eq!(r.va_fileid, 1, "the FAT12 root");

    let vp = lookup(p, b"/README.TXT").expect("lookup");
    // the owner write bit is the read-only attribute
    let mut va = Vattr::new();
    vattr_null(&mut va);
    va.va_mode = 0o444;
    setattr(p, vp, &mut va).expect("chmod");
    assert_eq!(stat(p, b"/README.TXT").expect("stat").va_mode, 0o444);
    assert_eq!(vtode(vp).de_Attributes.get() & ATTR_READONLY, ATTR_READONLY);
    vattr_null(&mut va);
    va.va_mode = 0o600;
    setattr(p, vp, &mut va).expect("chmod");
    assert_eq!(stat(p, b"/README.TXT").expect("stat").va_mode, 0o644);

    // times: DOS keeps two-second modification times and access dates
    let t = Timespec::new(1_577_836_800 + 3 * 3600 + 61, 0); // 2020-01-01 03:01:01
    vattr_null(&mut va);
    va.va_mtime = t;
    va.va_atime = t;
    setattr(p, vp, &mut va).expect("utimes");
    let f = stat(p, b"/README.TXT").expect("stat");
    assert_eq!(f.va_mtime, Timespec::new(t.tv_sec - 1, 0));
    assert_eq!(f.va_atime, Timespec::new(1_577_836_800, 0));

    // SF_ARCHIVED clears the archive attribute; other flags are not supported
    vattr_null(&mut va);
    va.va_flags = u64::from(SF_ARCHIVED);
    setattr(p, vp, &mut va).expect("chflags");
    assert_eq!(
        stat(p, b"/README.TXT").expect("stat").va_flags,
        u64::from(SF_ARCHIVED)
    );
    vattr_null(&mut va);
    va.va_flags = 1; // UF_NODUMP
    assert_eq!(setattr(p, vp, &mut va), Err(Errno::EOPNOTSUPP));

    // the owner can only be the mount's; unsettable attributes
    vattr_null(&mut va);
    va.va_uid = 1000;
    assert_eq!(setattr(p, vp, &mut va), Err(Errno::EINVAL));
    vattr_null(&mut va);
    va.va_nlink = 2;
    assert_eq!(setattr(p, vp, &mut va), Err(Errno::EINVAL));
    // a directory cannot be truncated
    let dvp = lookup(p, b"/SUBDIR").expect("lookup");
    assert_eq!(truncate(p, dvp, 0), Err(Errno::EISDIR));
    vrele(dvp);

    // access: the mask and the read-only attribute, for another user
    let other = crget();
    other.cr_uid.set(1000);
    other.cr_gid.set(1000);
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    assert_eq!(VOP_ACCESS(vp, VWRITE, other, p), Err(Errno::EACCES));
    VOP_ACCESS(vp, crate::sys::vnode::VREAD, other, p).expect("readable");
    VOP_ACCESS(vp, VWRITE, p.ucred(), p).expect("root writes");
    let _ = VOP_UNLOCK(vp);

    // the change reached the entry
    let img = synced(p, mp);
    let e = entry(&img, Dir::Root, b"README  TXT").expect("on disk");
    assert_eq!(e.deAttributes & ATTR_ARCHIVE, 0);
    assert_eq!(getushort(&e.deMDate), (40 << 9) | (1 << 5) | 1);
    vrele(vp);
}

#[test]
fn the_denode_lock_recurses_as_ufs_does_and_vnd_reads_under_it() {
    let (_g, p, _mp) = setup_root(sample());
    let mut nd = ndinit(0, 0, NiDirp::Sys(b"/README.TXT"), p);
    vn_open(&mut nd, FREAD | FWRITE, 0).expect("open");
    let vp = nd.ni_vp.expect("a vnode");
    let lock = &vtode(vp).de_lock;

    // vn_open returns the vnode locked by this thread
    assert_eq!(VOP_ISLOCKED(vp), LK_EXCLUSIVE);
    assert_eq!(lock.rrwl_wcnt.get(), 1);

    // the same thread takes it again: rrw_enter counts
    vn_lock(vp, LK_EXCLUSIVE | LK_RETRY).expect("recursive lock");
    assert_eq!(lock.rrwl_wcnt.get(), 2);
    assert_eq!(
        vn_lock(vp, LK_EXCLUSIVE | LK_RECURSEFAIL),
        Err(Errno::EDEADLK)
    );
    let _ = VOP_UNLOCK(vp);
    assert_eq!(VOP_ISLOCKED(vp), LK_EXCLUSIVE, "still held once");

    // vnd(4)'s VNDIOCSET reads the file through vn_rdwr (which locks it again) while
    // vn_open's lock is held
    let mut buf = [0u8; 512];
    assert_eq!(rdwr(p, UioRw::UIO_READ, vp, &mut buf, 512), 512);
    assert_eq!(&buf[..], &pattern(3000)[512..1024]);
    assert_eq!(lock.rrwl_wcnt.get(), 1);

    let _ = VOP_UNLOCK(vp);
    assert_eq!(VOP_ISLOCKED(vp), 0);
    close(p, vp);
}

#[test]
fn bmap_maps_clusters_and_counts_runs() {
    let (_g, p, mp) = setup_root(sample());
    let pmp = vfstomsdosfs(mp);
    let vp = lookup(p, b"/README.TXT").expect("lookup");
    let dep = vtode(vp);

    let mut bn: Daddr = 0;
    let mut run = 0;
    msdosfs_bmaparray(vp, 0, &mut bn, Some(&mut run)).expect("bmap");
    assert_eq!(bn, cntobn(pmp, i64::from(dep.de_StartCluster.get())));
    assert_eq!(run, 5, "six contiguous clusters");
    msdosfs_bmaparray(vp, 5, &mut bn, Some(&mut run)).expect("bmap last");
    assert_eq!(run, 0);
    assert_eq!(
        msdosfs_bmaparray(vp, 6, &mut bn, None),
        Err(Errno::E2BIG),
        "past the end of the chain"
    );

    let mut devvp = None;
    let mut a = VopBmapArgs {
        a_vp: vp,
        a_bn: 1 << 40,
        a_vpp: Some(&mut devvp),
        a_bnp: Some(&mut bn),
        a_runp: None,
    };
    assert_eq!(msdosfs_bmap(&mut a), Err(Errno::EFBIG));
    assert!(devvp.is_some_and(|d| ptr::eq(d, pmp.devvp())));
    vrele(vp);
}

#[cfg(feature = "tmpfs")]
#[test]
fn lookups_cross_the_mount_point_both_ways() {
    use crate::kern::kern_rwlock::rw_obj_init;
    use crate::tmpfs::tmpfs_mem::TMPFS_BYTES_USED;
    use crate::tmpfs::tmpfs_vfsops::tests::{args, tmpfs_conf};
    use crate::tmpfs::tmpfs_vfsops::{tmpfs_mount, tmpfs_root};
    use crate::uvm::uvm_aobj::uao_init;
    use core::sync::atomic::Ordering;

    let (_g, p) = setup(sample());
    rw_obj_init();
    uao_init();
    TMPFS_BYTES_USED.store(0, Ordering::Relaxed);

    // a tmpfs root with /mnt
    let tmp = crate::kern::vfs_subr::vfs_mount_alloc(None, tmpfs_conf());
    let mut data = args(1 << 20, 0, 0, 0o755);
    let mut nd = ndinit(LOOKUP, 0, NiDirp::Sys(b"/"), p);
    tmpfs_mount(tmp, b"/", &mut data, &mut nd, p).expect("mount tmpfs");
    crate::kern::vfs_subr::vfs_unbusy(tmp);
    let root = tmpfs_root(tmp).expect("root");
    set_rootvnode(Some(root));
    p.fd().fd_cdir.set(Some(root));
    vref(root);
    let _ = VOP_UNLOCK(root);
    domkdirat(p, AT_FDCWD, c("/mnt").as_ptr(), 0o755).expect("mkdir /mnt");

    // mount_msdos /dev/vnd0c /mnt, as sys_mount ends
    let mp = mount(p, true);
    let mut sb = mp.mnt_stat.get();
    msdosfs_statfs(mp, &mut sb, p).expect("statfs");
    mp.mnt_stat.set(sb);
    let covered = lookup(p, b"/mnt").expect("/mnt");
    mp.mnt_vnodecovered.set(Some(covered));
    covered.set_v_mountedhere(Some(mp));

    // cat /mnt/m10c-fat.txt
    assert_eq!(read_file(p, b"/mnt/m10c-fat.txt"), FATDATA);
    let (ents, _, _) = readdir(p, b"/mnt", 0, 4096);
    assert_eq!(
        names(&ents),
        [&b"."[..], b"..", FATNAME, b"README.TXT", b"SUBDIR"]
    );
    // the root of the msdosfs, and back up through ".."
    let mroot = lookup(p, b"/mnt").expect("/mnt");
    assert_eq!(mroot.v_tag.get(), crate::sys::vnode::VT_MSDOSFS);
    vrele(mroot);
    assert_eq!(
        stat(p, b"/mnt/..").expect("..").va_fileid,
        stat(p, b"/").expect("/").va_fileid
    );
    assert_eq!(
        read_file(p, b"/mnt/subdir/../SUBDIR/inner file.dat"),
        b"inside"
    );
    // read-only: no creation
    assert_eq!(
        domkdirat(p, AT_FDCWD, c("/mnt/x").as_ptr(), 0o755),
        Err(Errno::EROFS)
    );
}

#[test]
fn pathconf_and_kqueue_filters() {
    let (_g, p, mp) = setup_root(sample());
    let vp = lookup(p, b"/README.TXT").expect("lookup");
    let mut v: Register = 0;
    VOP_PATHCONF(vp, _PC_NAME_MAX, &mut v).expect("pathconf");
    assert_eq!(v, WIN_MAXLEN as Register);
    let pmp = vfstomsdosfs(mp);
    pmp.pm_flags
        .set(pmp.pm_flags.get() & !(MSDOSFSMNT_LONGNAME as u32));
    VOP_PATHCONF(vp, _PC_NAME_MAX, &mut v).expect("pathconf");
    assert_eq!(v, 12, "8.3");
    VOP_PATHCONF(vp, _PC_LINK_MAX, &mut v).expect("pathconf");
    assert_eq!(v, 1);
    VOP_PATHCONF(vp, _PC_TIMESTAMP_RESOLUTION, &mut v).expect("pathconf");
    assert_eq!(v, 2_000_000_000);
    assert_eq!(VOP_PATHCONF(vp, 9999, &mut v), Err(Errno::EINVAL));

    // EVFILT_VNODE: only the subscribed notes are recorded; revocation ends it
    let kn = Knote::new();
    kn.kn_filter().set(EVFILT_VNODE);
    VOP_KQFILTER(vp, 0, &kn).expect("kqfilter");
    assert!(
        kn.kn_fop
            .get()
            .is_some_and(|f| ptr::eq(f, &MSDOSFSVNODE_FILTOPS))
    );
    kn.kn_sfflags.set(NOTE_WRITE | NOTE_DELETE);
    assert!(!filt_msdosfsvnode(&kn, i64::from(NOTE_ATTRIB)));
    assert!(filt_msdosfsvnode(&kn, i64::from(NOTE_WRITE)));
    assert_eq!(kn.kn_fflags().get(), NOTE_WRITE);
    assert!(filt_msdosfsvnode(&kn, i64::from(NOTE_REVOKE)) && kn.has_flags(EV_EOF));
    filt_msdosfsdetach(&kn);

    // EVFILT_WRITE: always writable; an unknown filter is refused
    let kn = Knote::new();
    assert!(filt_msdosfswrite(&kn, 0) && kn.kn_data().get() == 0);
    assert!(filt_msdosfswrite(&kn, i64::from(NOTE_REVOKE)));
    assert!(kn.has_flags(EV_EOF) && kn.has_flags(EV_ONESHOT));
    kn.kn_filter().set(-100);
    assert_eq!(VOP_KQFILTER(vp, 0, &kn), Err(Errno::EINVAL));

    // no links, symlinks or device nodes on DOS
    assert_eq!(
        crate::kern::vfs_syscalls::dolinkat(
            p,
            AT_FDCWD,
            c("/README.TXT").as_ptr(),
            AT_FDCWD,
            c("/link").as_ptr(),
            0
        ),
        Err(Errno::EOPNOTSUPP)
    );
    vrele(vp);
}

#[test]
fn fileidhash_sets_the_top_bit_and_spreads() {
    assert_ne!(fileidhash(0) & 0x8000_0000, 0);
    assert_ne!(fileidhash(1), fileidhash(2));
    assert_eq!(fileidhash(12345), fileidhash(12345));
}
