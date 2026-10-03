//! Host tests for `kern_unveil.c`: the permission strings, the flag matching, the name tree,
//! and `unveil(2)` end to end over `testfs` (`vfs_subr/tests.rs`): `sys_unveil` adds
//! directories and names, and `namei` then allows, refuses (`EACCES`) or hides (`ENOENT`)
//! what lies under them, through covers, `..`, symbolic links and relative lookups; fork's
//! copy, exec's and exit's destroy and unmount's `unveil_removevnode` give back every vnode
//! reference they took.

use std::{assert, assert_eq, boxed::Box, vec::Vec};

use super::*;
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::tests::testfs::{self, A, C, LONG, N, ROOT, any_locked, usecounts};
use crate::kern::vfs_syscalls::sys_unveil;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::sys::namei::{FOLLOW, NiDirp};
use crate::sys::proc::ProcessList;
use crate::sys::queue::ListHead;
use crate::sys::systm::SysArgs;
use crate::sys::types::Register;

#[test]
fn parsepermissions_takes_rwxc_and_nothing_else() {
    assert_eq!(unveil_parsepermissions(b""), Ok(UNVEIL_USERSET));
    assert_eq!(
        unveil_parsepermissions(b"r"),
        Ok(UNVEIL_USERSET | UNVEIL_READ)
    );
    assert_eq!(
        unveil_parsepermissions(b"cxwr"),
        Ok(UNVEIL_USERSET | UNVEIL_READ | UNVEIL_WRITE | UNVEIL_EXEC | UNVEIL_CREATE)
    );
    assert_eq!(
        unveil_parsepermissions(b"rr"),
        Ok(UNVEIL_USERSET | UNVEIL_READ)
    );
    // The string ends at its NUL, as copyinstr leaves it.
    assert_eq!(
        unveil_parsepermissions(b"w\0x"),
        Ok(UNVEIL_USERSET | UNVEIL_WRITE)
    );
    for bad in [&b"rz"[..], b"R", b" ", b"rwxc-"] {
        assert_eq!(unveil_parsepermissions(bad), Err(Errno::EINVAL));
    }
}

#[test]
fn flagmatch_requires_every_requested_bit() {
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    let mut ni = ndinit(LOOKUP, 0, NiDirp::Sys(b"x"), p);
    let all = UNVEIL_READ | UNVEIL_WRITE | UNVEIL_EXEC | UNVEIL_CREATE;
    for want in 0..=all {
        ni.ni_unveil = want;
        // Flags 0 forbid everything, even a lookup that asks for nothing.
        assert!(!unveil_flagmatch(&ni, 0));
        for have in 0..=all {
            let flags = have | UNVEIL_USERSET;
            assert_eq!(
                unveil_flagmatch(&ni, flags),
                want & !have == 0,
                "want {want:#x} have {have:#x}"
            );
        }
    }
}

#[test]
fn the_name_tree_orders_by_size_then_bytes() {
    let (_g, _p, _mp) = testfs::setup_root();
    let uv: &'static Unveil = Box::leak(Box::new(Unveil::new()));
    // unveil_namelookup asserts a vnode under DIAGNOSTIC; any directory will do here.
    uv.uv_vp.set(rootvnode());
    for name in [&b"zz"[..], b"b", b"a", b"abc", b"ab"] {
        assert!(unveil_add_name(uv, name, UNVEIL_READ));
    }
    // Already there: not added again, the flags untouched.
    assert!(!unveil_add_name(uv, b"ab", UNVEIL_WRITE));
    let names: Vec<&[u8]> = uv.uv_names.iter().map(Unvname::name).collect();
    assert_eq!(names, [&b"a"[..], b"b", b"ab", b"zz", b"abc"]);

    let n1 = unvname_new(b"ab", 0);
    let n2 = unvname_new(b"b", 0);
    assert_eq!(unvname_compare(n1, n2), CmpOrdering::Greater);
    assert_eq!(unvname_compare(n2, n1), CmpOrdering::Less);
    unvname_delete(n1);
    unvname_delete(n2);

    assert_eq!(
        unveil_namelookup(uv, b"ab").map(|n| n.un_flags.get()),
        Some(UNVEIL_READ)
    );
    assert!(unveil_namelookup(uv, b"abd").is_none());
    assert!(unveil_namelookup(uv, b"").is_none());
    assert_eq!(unveil_delete_names(uv), 5);
    assert!(uv.uv_names.is_empty());
}

/// `unveil(path, permissions)` through the system call, `None` for a NULL argument.
fn unveil(p: &Proc, path: Option<&[u8]>, perms: Option<&[u8]>) -> Result<(), Errno> {
    let cstr = |s: Option<&[u8]>| -> Register {
        s.map_or(0, |s| {
            let mut v = s.to_vec();
            v.push(0);
            Box::leak(v.into_boxed_slice()).as_ptr() as Register
        })
    };
    let mut v: SysArgs = [0; 6];
    v[0] = cstr(path);
    v[1] = cstr(perms);
    let mut retval = [0; 2];
    sys_unveil(p, &v, &mut retval)
}

/// `namei` of `path` asking for `want` (`ni_unveil`): `Ok` with the vnode released, or the
/// error.
fn access(p: &Proc, path: &[u8], want: u8) -> Result<(), Errno> {
    let path: &'static [u8] = Box::leak(path.to_vec().into_boxed_slice());
    let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(path), p);
    nd.ni_unveil = want;
    namei(&mut nd)?;
    if let Some(vp) = nd.ni_vp {
        vrele(vp);
    }
    Ok(())
}

/// The test's lock, and `curproc` cleared again when the test ends.
struct Guard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        Machine::set_curproc(Machine::curcpu(), ptr::null());
    }
}

/// testfs as root, with the thread as `curproc` and counted as its process's one thread
/// (what `single_thread_set` expects).
fn setup_unveil() -> (Guard, &'static Proc, &'static crate::sys::mount::Mount) {
    let (g, p, mp) = testfs::setup_root();
    p.process().ps_threadcnt.set(1);
    Machine::set_curproc(Machine::curcpu(), p);
    (Guard { _lock: g }, p, mp)
}

/// Every node's `v_uvcount`.
fn uvcounts() -> [u32; N] {
    core::array::from_fn(|i| testfs::vnode_of(i).map_or(0, |vp| vp.v_uvcount.get()))
}

#[test]
fn an_unveiled_directory_hides_the_rest() {
    let (_g, p, _mp) = setup_unveil();
    let pr = p.process();
    let before = usecounts();

    // Nothing unveiled: everything is visible.
    assert_eq!(access(p, b"/a/b", UNVEIL_READ | UNVEIL_WRITE), Ok(()));

    assert_eq!(unveil(p, Some(b"/a/c"), Some(b"r")), Ok(()));
    assert!(!pr.ps_uvpaths.get().is_null());
    assert_eq!(pr.ps_uvvcount.get(), 1);
    assert_eq!(uvcounts()[C], 1);

    assert_eq!(access(p, b"/a/c", UNVEIL_READ), Ok(()));
    assert_eq!(access(p, b"/a/c/.", UNVEIL_READ), Ok(()));
    // Through a symbolic link, which is looked up component by component.
    assert_eq!(access(p, b"/l", UNVEIL_READ), Ok(()));
    // Unveiled read-only: EACCES for more.
    assert_eq!(access(p, b"/a/c", UNVEIL_WRITE), Err(Errno::EACCES));
    assert_eq!(pr.ps_acflag.get() & AUNVEIL, AUNVEIL);
    // Everything else is gone.
    assert_eq!(access(p, b"/a/b", UNVEIL_READ), Err(Errno::ENOENT));
    assert_eq!(access(p, b"/", UNVEIL_READ), Err(Errno::ENOENT));
    assert_eq!(access(p, b"/a", UNVEIL_READ), Err(Errno::ENOENT));
    assert_eq!(access(p, b"/a/c/..", UNVEIL_READ), Err(Errno::ENOENT));
    assert_eq!(access(p, b"/a/c/../b", UNVEIL_READ), Err(Errno::ENOENT));
    // A lookup that asks for nothing (stat-like) is still refused outside.
    assert_eq!(access(p, b"/a/b", 0), Err(Errno::ENOENT));
    assert!(!any_locked());

    // Exec (no execpromises) and exit drop the table and every reference.
    unveil_destroy(pr);
    assert!(pr.ps_uvpaths.get().is_null());
    assert_eq!(pr.ps_uvvcount.get(), 0);
    assert_eq!(uvcounts(), [0; N]);
    assert_eq!(usecounts(), before);
    assert_eq!(access(p, b"/a/b", UNVEIL_READ), Ok(()));
}

#[test]
fn names_beneath_a_directory_and_covers() {
    let (_g, p, _mp) = setup_unveil();
    let pr = p.process();
    let before = usecounts();

    // A file: its directory gets a slot with flags 0, the name its own flags.
    assert_eq!(unveil(p, Some(b"/a/b"), Some(b"rw")), Ok(()));
    assert_eq!(pr.ps_uvvcount.get(), 1);
    assert_eq!(pr.ps_uvncount.get(), 1);
    assert_eq!(uvcounts()[A], 1);
    assert_eq!(access(p, b"/a/b", UNVEIL_READ | UNVEIL_WRITE), Ok(()));
    assert_eq!(access(p, b"/a/b", UNVEIL_EXEC), Err(Errno::EACCES));
    // A sibling name was not unveiled, and nothing covers /a.
    let long: Vec<u8> = [&b"/a/"[..], testfs::NODES[LONG].name].concat();
    assert_eq!(access(p, &long, UNVEIL_READ), Err(Errno::ENOENT));
    assert_eq!(access(p, b"/a", UNVEIL_READ), Err(Errno::ENOENT));

    // Re-unveiling the name replaces its flags; no new name.
    assert_eq!(unveil(p, Some(b"/a/b"), Some(b"x")), Ok(()));
    assert_eq!(pr.ps_uvncount.get(), 1);
    assert_eq!(access(p, b"/a/b", UNVEIL_EXEC), Ok(()));
    assert_eq!(access(p, b"/a/b", UNVEIL_READ), Err(Errno::EACCES));

    // A name that does not exist yet can be unveiled for creation.
    assert_eq!(unveil(p, Some(b"/a/new"), Some(b"c")), Ok(()));
    assert_eq!(pr.ps_uvncount.get(), 2);
    assert_eq!(uvcounts()[A], 1);

    // Unveiling the root read-only covers /a: its other names become readable, not writable.
    assert_eq!(unveil(p, Some(b"/"), Some(b"r")), Ok(()));
    assert_eq!(pr.ps_uvvcount.get(), 2);
    assert_eq!(uvcounts()[ROOT], 1);
    assert_eq!(access(p, &long, UNVEIL_READ), Ok(()));
    assert_eq!(access(p, &long, UNVEIL_WRITE), Err(Errno::EACCES));
    assert_eq!(access(p, b"/a/c", UNVEIL_READ), Ok(()));
    assert_eq!(access(p, b"/a/c/..", UNVEIL_READ), Ok(()));
    // The name keeps its own flags.
    assert_eq!(access(p, b"/a/b", UNVEIL_READ), Err(Errno::EACCES));

    // Unveiling /a itself (a directory already in the table) sets its flags in place.
    assert_eq!(unveil(p, Some(b"/a"), Some(b"rwc")), Ok(()));
    assert_eq!(pr.ps_uvvcount.get(), 2);
    assert_eq!(uvcounts()[A], 1);
    assert_eq!(access(p, &long, UNVEIL_WRITE), Ok(()));
    // ... and "" hides it: ENOENT, not EACCES.
    assert_eq!(unveil(p, Some(b"/a"), Some(b"")), Ok(()));
    assert_eq!(access(p, b"/a", UNVEIL_READ), Err(Errno::ENOENT));
    assert!(!any_locked());

    unveil_destroy(pr);
    assert_eq!(pr.ps_uvncount.get(), 0);
    assert_eq!(uvcounts(), [0; N]);
    assert_eq!(usecounts(), before);
}

#[test]
fn relative_lookups_start_from_the_cover_of_the_directory() {
    let (_g, p, mp) = setup_unveil();
    let pr = p.process();

    assert_eq!(unveil(p, Some(b"/a"), Some(b"r")), Ok(()));
    // chdir /a/c, which is not itself unveiled: covered by /a.
    let cdir = testfs::vget_node(mp, C).unwrap();
    let _ = crate::kern::vfs_vops::VOP_UNLOCK(cdir);
    let old = p.fd().fd_cdir.replace(Some(cdir));
    assert_eq!(access(p, b".", UNVEIL_READ), Ok(()));
    assert_eq!(access(p, b"../b", UNVEIL_READ), Ok(()));
    assert_eq!(access(p, b"../b", UNVEIL_WRITE), Err(Errno::EACCES));
    // Above the cover nothing is visible.
    assert_eq!(access(p, b"../..", UNVEIL_READ), Err(Errno::ENOENT));
    assert_eq!(access(p, b"../../l", UNVEIL_READ), Ok(())); // l -> a/c
    p.fd().fd_cdir.set(old);
    vrele(cdir);
    assert!(!any_locked());
    unveil_destroy(pr);
    assert_eq!(uvcounts(), [0; N]);
}

#[test]
fn the_system_call_checks_its_arguments_and_locks() {
    let (_g, p, _mp) = setup_unveil();
    let pr = p.process();

    assert_eq!(unveil(p, Some(b"/a"), Some(b"rq")), Err(Errno::EINVAL));
    assert_eq!(unveil(p, Some(b""), Some(b"r")), Err(Errno::EINVAL));
    assert_eq!(
        unveil(p, Some(b"/"), Some(b"rwxcr")),
        Err(Errno::ENAMETOOLONG)
    );
    assert_eq!(unveil(p, Some(b"/nope/x"), Some(b"r")), Err(Errno::ENOENT));
    assert_eq!(unveil(p, None, Some(b"r")), Err(Errno::EFAULT));
    assert_eq!(pr.ps_single.get(), ptr::null());

    // E2BIG once the names are used up.
    pr.ps_uvncount.set(UNVEIL_MAX_NAMES);
    assert_eq!(unveil(p, Some(b"/a/b"), Some(b"r")), Err(Errno::E2BIG));
    pr.ps_uvncount.set(0);

    // unveil(NULL, NULL) locks the table.
    assert_eq!(unveil(p, None, None), Ok(()));
    assert_eq!(pr.ps_uvdone.get(), 1);
    assert_eq!(unveil(p, Some(b"/"), Some(b"r")), Err(Errno::EPERM));
    unveil_destroy(pr);
    assert_eq!(uvcounts(), [0; N]);
}

#[test]
fn fork_copies_and_unmount_zaps() {
    let (_g, p, _mp) = setup_unveil();
    let pr = p.process();
    let before = usecounts();

    assert_eq!(unveil(p, Some(b"/a/c"), Some(b"r")), Ok(()));
    assert_eq!(unveil(p, Some(b"/a/b"), Some(b"w")), Ok(()));
    pr.ps_uvdone.set(1);

    let child: &'static Process = Box::leak(Box::new(Process::new()));
    unveil_copy(pr, child);
    assert_eq!(child.ps_uvdone.get(), 1);
    assert_eq!(child.ps_uvvcount.get(), 2);
    assert_eq!(child.ps_uvncount.get(), 1);
    assert_eq!(uvcounts()[C], 2);
    assert_eq!(uvcounts()[A], 2);
    let cuv = unveil_lookup(testfs::vnode_of(A).unwrap(), child, None).unwrap();
    assert_eq!(
        unveil_namelookup(cuv, b"b").map(|n| n.un_flags.get()),
        Some(UNVEIL_USERSET | UNVEIL_WRITE)
    );

    // A process with nothing unveiled copies nothing.
    let empty: &'static Process = Box::leak(Box::new(Process::new()));
    let grandchild: &'static Process = Box::leak(Box::new(Process::new()));
    unveil_copy(empty, grandchild);
    assert!(grandchild.ps_uvpaths.get().is_null());

    // Both processes on allprocess: unmount drops their unveils of /a/c.
    // SAFETY: two leaked processes in no list, taken off again below.
    unsafe {
        ALLPROCESS.0.insert_head(pr);
        ALLPROCESS.0.insert_head(child);
    }
    let cvp = testfs::vnode_of(C).unwrap();
    unveil_removevnode(cvp);
    assert_eq!(cvp.v_uvcount.get(), 0);
    let mut pos = 0;
    assert!(unveil_lookup(cvp, pr, Some(&mut pos)).is_none());
    assert_eq!(pos, -1);
    assert_eq!(pr.ps_uvvcount.get(), 2); // the slot stays, zapped
    // SAFETY: both are on the list (inserted above).
    unsafe {
        ListHead::<ProcessList>::remove(child);
        ListHead::<ProcessList>::remove(pr);
    }

    unveil_destroy(child);
    unveil_destroy(pr);
    assert_eq!(uvcounts(), [0; N]);
    assert_eq!(usecounts(), before);
}

#[test]
fn find_cover_walks_up_to_the_nearest_unveil() {
    let (_g, p, _mp) = setup_unveil();
    let pr = p.process();
    let root = rootvnode().unwrap();
    assert_eq!(unveil(p, Some(b"/"), Some(b"r")), Ok(()));
    assert_eq!(unveil(p, Some(b"/a/c"), Some(b"rw")), Ok(()));
    let cvp = testfs::vnode_of(C).unwrap();
    // The root has no cover; /a/c is covered by the root's slot (0).
    assert_eq!(unveil_find_cover(root, p), -1);
    assert_eq!(unveil_find_cover(cvp, p), 0);
    assert_eq!(unveil_lookup(cvp, pr, None).unwrap().uv_cover.get(), 0);
    // Interposing /a re-checks /a/c's cover: slot 2.
    assert_eq!(unveil(p, Some(b"/a"), Some(b"r")), Ok(()));
    assert_eq!(unveil_lookup(cvp, pr, None).unwrap().uv_cover.get(), 2);
    assert_eq!(unveil_find_cover(testfs::vnode_of(A).unwrap(), p), 0);
    assert!(!any_locked());
    unveil_destroy(pr);
    assert_eq!(uvcounts(), [0; N]);
}
