//! Host tests for `namei`/`vfs_lookup` over `testfs` (`vfs_subr/tests.rs`): the splitting of
//! a path into components (leading, repeated and trailing slashes, `.` and `..`, the root
//! as a barrier), symbolic links (relative, absolute, loops), the locking and reference
//! protocol of `LOCKLEAF`/`LOCKPARENT`/`WANTPARENT`, `CREATE` of a missing last component,
//! `REALPATH`, the errors, and that every lookup leaves the use counts as it found them.

use std::{assert, assert_eq};

use super::*;
use crate::kern::vfs_subr::tests::setup;
use crate::kern::vfs_subr::tests::testfs::{
    self, A, ABS, B, C, L, LONG, LOOP, N, ROOT, any_locked, node_of, usecounts, vnode_of,
};
use crate::kern::vfs_vops::{VOP_ABORTOP, VOP_ISLOCKED};
use crate::sys::namei::{LOOKUP, NOFOLLOW, WANTPARENT};
use crate::sys::vnode::VREG;

/// `namei` of a kernel path, the result reduced to the node it found.
fn lookup(p: &Proc, path: &[u8], op: u64, flags: u64) -> Result<Nameidata<'static>, Errno> {
    let path: &'static [u8] = std::boxed::Box::leak(path.to_vec().into_boxed_slice());
    let mut nd = ndinit(op, flags, NiDirp::Sys(path), p);
    namei(&mut nd).map(|()| nd)
}

/// Looks `path` up with `LOOKUP` and `flags`, checks it found node `want`, releases it and
/// checks nothing leaked.
fn finds(p: &Proc, path: &[u8], flags: u64, want: usize) {
    let before = usecounts();
    let nd = lookup(p, path, LOOKUP, flags).unwrap_or_else(|e| {
        std::panic!("{:?}: error {e:?}", core::str::from_utf8(path));
    });
    let vp = nd.ni_vp.unwrap();
    assert_eq!(node_of(vp), want, "{:?}", core::str::from_utf8(path));
    if flags & LOCKLEAF != 0 {
        assert_eq!(VOP_ISLOCKED(vp), 1);
        vput(vp);
    } else {
        assert!(
            !any_locked(),
            "{:?} left a lock",
            core::str::from_utf8(path)
        );
        vrele(vp);
    }
    assert!(!any_locked());
    // Nodes looked up for the first time now have a vnode on the free list (count 0).
    let after = usecounts();
    for i in 0..N {
        assert_eq!(after[i], before[i], "use count of node {i} after {path:?}");
    }
}

#[test]
fn absolute_and_relative_paths() {
    let (_g, p, _mp) = testfs::setup_root();
    finds(p, b"/", FOLLOW, ROOT);
    finds(p, b"/a", FOLLOW, A);
    finds(p, b"/a/b", FOLLOW, B);
    finds(p, b"/a/b", FOLLOW | LOCKLEAF, B);
    finds(p, b"a/c", FOLLOW, C);
    finds(p, b"./a/./c/.", FOLLOW, C);
}

#[test]
fn slashes_split_components() {
    let (_g, p, _mp) = testfs::setup_root();
    finds(p, b"//a///c", FOLLOW, C);
    finds(p, b"/a/c/", FOLLOW, C);
    finds(p, b"/a/c//", FOLLOW | LOCKLEAF, C);
    // A trailing slash requires a directory.
    assert_eq!(
        lookup(p, b"/a/b/", LOOKUP, FOLLOW).err(),
        Some(Errno::ENOTDIR)
    );
    // A component under a file.
    assert_eq!(
        lookup(p, b"/a/b/x", LOOKUP, FOLLOW).err(),
        Some(Errno::ENOTDIR)
    );
    assert!(!any_locked());
}

#[test]
fn dot_dot_climbs_and_stops_at_the_root() {
    let (_g, p, _mp) = testfs::setup_root();
    finds(p, b"/a/c/..", FOLLOW, A);
    finds(p, b"/a/../a/b", FOLLOW, B);
    finds(p, b"/..", FOLLOW, ROOT);
    finds(p, b"../../a", FOLLOW, A);
    finds(p, b"/a/c/../../a/c", FOLLOW | LOCKLEAF, C);
}

#[test]
fn errors() {
    let (_g, p, _mp) = testfs::setup_root();
    let before = usecounts();
    assert_eq!(lookup(p, b"", LOOKUP, FOLLOW).err(), Some(Errno::ENOENT));
    assert_eq!(
        lookup(p, b"/nope", LOOKUP, FOLLOW).err(),
        Some(Errno::ENOENT)
    );
    assert_eq!(
        lookup(p, b"/a/nope/b", LOOKUP, FOLLOW).err(),
        Some(Errno::ENOENT)
    );
    let long = [b'y'; 300];
    assert_eq!(
        lookup(p, &long, LOOKUP, FOLLOW).err(),
        Some(Errno::ENAMETOOLONG)
    );
    let toolong = [b'z'; MAXPATHLEN + 10];
    assert_eq!(
        lookup(p, &toolong, LOOKUP, FOLLOW).err(),
        Some(Errno::ENAMETOOLONG)
    );
    assert!(!any_locked());
    assert_eq!(usecounts()[ROOT], before[ROOT]);
}

#[test]
fn symbolic_links() {
    let (_g, p, _mp) = testfs::setup_root();
    // Relative to the link's directory, absolute from the root.
    finds(p, b"/l", FOLLOW, C);
    finds(p, b"/l/..", FOLLOW, A);
    finds(p, b"/a/abs", FOLLOW | LOCKLEAF, B);
    // The last component is not followed without FOLLOW; the others always are.
    finds(p, b"/l", NOFOLLOW, L);
    finds(p, b"/a/abs", NOFOLLOW, ABS);
    finds(p, b"/l/", NOFOLLOW, C);
    finds(p, b"l/..", NOFOLLOW, A);
    assert_eq!(
        lookup(p, b"/loop", LOOKUP, FOLLOW).err(),
        Some(Errno::ELOOP)
    );
    finds(p, b"/loop", NOFOLLOW, LOOP);
    assert!(!any_locked());
}

#[test]
fn lockparent_returns_both_vnodes_locked() {
    let (_g, p, _mp) = testfs::setup_root();
    let before = usecounts();
    let nd = lookup(p, b"/a/b", LOOKUP, LOCKPARENT | LOCKLEAF).unwrap();
    let (vp, dvp) = (nd.ni_vp.unwrap(), nd.ni_dvp.unwrap());
    assert_eq!((node_of(dvp), node_of(vp)), (A, B));
    assert_eq!((VOP_ISLOCKED(dvp), VOP_ISLOCKED(vp)), (1, 1));
    vput(vp);
    vput(dvp);
    assert!(!any_locked());
    assert_eq!(usecounts()[ROOT], before[ROOT]);

    // WANTPARENT: the parent comes back referenced but unlocked.
    let nd = lookup(p, b"/a/c", LOOKUP, WANTPARENT).unwrap();
    let (vp, dvp) = (nd.ni_vp.unwrap(), nd.ni_dvp.unwrap());
    assert_eq!((node_of(dvp), node_of(vp)), (A, C));
    assert!(!any_locked());
    vrele(vp);
    vrele(dvp);
}

#[test]
fn create_of_a_missing_name_returns_the_locked_parent() {
    let (_g, p, _mp) = testfs::setup_root();
    let before = usecounts();
    let mut nd = lookup(p, b"/a/new", CREATE, LOCKPARENT | SAVENAME).unwrap();
    assert!(nd.ni_vp.is_none());
    let dvp = nd.ni_dvp.unwrap();
    assert_eq!(node_of(dvp), A);
    assert_eq!(VOP_ISLOCKED(dvp), 1);
    // SAVENAME keeps the pathname buffer and the last component for the caller.
    assert!(nd.ni_cnd.cn_flags & HASBUF != 0);
    assert_eq!(nd.ni_cnd.name(), b"new");
    let _ = VOP_ABORTOP(dvp, &mut nd.ni_cnd);
    vput(dvp);
    assert!(!any_locked());
    assert_eq!(usecounts()[A], before[A]);

    // A missing intermediate directory is an error, not a creation.
    assert_eq!(
        lookup(p, b"/nope/new", CREATE, LOCKPARENT).err(),
        Some(Errno::ENOENT)
    );
    // Neither is a trailing slash on a missing name.
    assert_eq!(
        lookup(p, b"/a/new/", CREATE, LOCKPARENT).err(),
        Some(Errno::ENOENT)
    );
    // The root has no parent to create in.
    assert_eq!(
        lookup(p, b"/", CREATE, LOCKPARENT).err(),
        Some(Errno::EISDIR)
    );
    assert!(!any_locked());
}

#[test]
fn realpath_builds_the_canonical_name() {
    let (_g, p, _mp) = testfs::setup_root();
    let rpbuf: &'static mut [u8; MAXPATHLEN] = testfs::leak([0u8; MAXPATHLEN]);
    let path: &'static [u8] = b"/a/./c/../abs";
    let mut nd = ndinit(LOOKUP, FOLLOW | SAVENAME | REALPATH, NiDirp::Sys(path), p);
    nd.ni_cnd.cn_rpbuf = rpbuf.as_mut_ptr();
    nd.ni_cnd.cn_rpi = 0;
    namei(&mut nd).unwrap();
    let vp = nd.ni_vp.unwrap();
    assert_eq!(node_of(vp), B);
    let len = rpbuf.iter().position(|&c| c == 0).unwrap();
    assert_eq!(&rpbuf[..len], b"/a/b");
    vrele(vp);
    pool_put(&NAMEI_POOL, NonNull::new(nd.ni_cnd.cn_pnbuf).unwrap());
}

#[test]
fn stripslashes_drops_the_trailing_slashes_first() {
    let (_g, p, _mp) = testfs::setup_root();
    let nd = lookup(p, b"/a/b///", LOOKUP, FOLLOW | STRIPSLASHES).unwrap();
    let vp = nd.ni_vp.unwrap();
    assert_eq!(node_of(vp), B);
    assert_eq!(vp.v_type.get(), VREG);
    vrele(vp);
}

#[test]
fn no_root_file_system_is_enoent() {
    let (_g, p) = setup();
    assert_eq!(lookup(p, b"/a", LOOKUP, FOLLOW).err(), Some(Errno::ENOENT));
    assert_eq!(lookup(p, b"a", LOOKUP, FOLLOW).err(), Some(Errno::ENOENT));
    assert!(vnode_of(ROOT).is_none());
}

#[test]
fn component_push_and_pop() {
    let rpbuf: &'static mut [u8; MAXPATHLEN] = testfs::leak([0u8; MAXPATHLEN]);
    let mut cn = Componentname::new();
    cn.cn_rpbuf = rpbuf.as_mut_ptr();
    rpbuf[0] = b'/';
    cn.cn_rpi = 1;
    assert!(component_push(&mut cn, b"usr"));
    assert!(component_push(&mut cn, b"share"));
    assert_eq!(&rpbuf[..cn.cn_rpi + 1], b"/usr/share\0");
    component_pop(&mut cn);
    assert_eq!(&rpbuf[..cn.cn_rpi + 1], b"/usr\0");
    component_pop(&mut cn);
    assert_eq!(&rpbuf[..cn.cn_rpi + 1], b"/\0");
    // A component that would not fit is refused.
    cn.cn_rpi = MAXPATHLEN - 3;
    assert!(!component_push(&mut cn, b"xy"));
    let _ = LONG;
}
