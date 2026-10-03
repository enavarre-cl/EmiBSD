//! Host tests for the vnode table (`getnewvnode`, the use counts and free lists of
//! `vref`/`vrele`/`vput`/`vget`, `vgone`, `insmntque`, `vflush`), `vattr_null`, `vaccess` and
//! the mount helpers; and `testfs`, a small in-memory file system (a fixed tree of
//! directories, a file and symbolic links, with the lock discipline of a real one) that the
//! `namei`, name cache and `getcwd` tests share.

use std::sync::MutexGuard;
use std::{assert, assert_eq, boxed::Box};

use super::*;
use crate::kern::kern_descrip::{fdinit, filedesc_init};
use crate::kern::kern_proc::procinit;
use crate::kern::kern_prot::{crget, crhold};
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::kern::vfs_cache::{NUMCACHE, NUMNEG};
use crate::kern::vfs_init::{set_rootvnode, vfsinit};
use crate::sys::proc::Process;

/// `testfs`: the file system the vfs tests mount as root.
pub(crate) mod testfs {
    use core::ffi::c_void;
    use core::ptr;
    use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering};
    use std::boxed::Box;
    use std::sync::MutexGuard;

    use super::setup;
    use crate::kern::kern_subr::uiomove;
    use crate::kern::subr_xxx::eopnotsupp;
    use crate::kern::vfs_cache::{cache_enter, cache_lookup};
    use crate::kern::vfs_default::vop_generic_abortop;
    use crate::kern::vfs_init::set_rootvnode;
    use crate::kern::vfs_subr::{MOUNTLIST, getnewvnode, vfs_mount_alloc, vfs_unbusy, vget, vref};
    use crate::kern::vfs_vnops::vn_lock;
    use crate::kern::vfs_vops::VOP_UNLOCK;
    use crate::sys::dirent::{DT_DIR, DT_LNK, DT_REG, Dirent, dirent_recsize};
    use crate::sys::errno::Errno;
    use crate::sys::lock::{LK_EXCLUSIVE, LK_NOWAIT};
    use crate::sys::mount::{MNT_LOCAL, MNT_ROOTFS, Mount, VFS_ROOT, Vfsconf, Vfsops};
    use crate::sys::namei::{
        CREATE, ISDOTDOT, ISLASTCN, LOCKPARENT, MAKEENTRY, PDIRUNLOCK, RENAME,
    };
    use crate::sys::proc::Proc;
    use crate::sys::vnode::{
        VDIR, VLNK, VREG, VROOT, VT_TMPFS, Vnode, VopAccessArgs, VopGetattrArgs, VopInactiveArgs,
        VopIslockedArgs, VopLockArgs, VopLookupArgs, VopReaddirArgs, VopReadlinkArgs,
        VopReclaimArgs, VopUnlockArgs, Vops,
    };

    /// What a node is.
    pub(crate) enum Kind {
        /// A directory.
        Dir,
        /// A regular file.
        Reg,
        /// A symbolic link to the path.
        Lnk(&'static [u8]),
    }

    /// One node of the tree: its name, its parent's index and its kind.
    pub(crate) struct Node {
        pub(crate) name: &'static [u8],
        pub(crate) parent: usize,
        pub(crate) kind: Kind,
    }

    /// `/`.
    pub(crate) const ROOT: usize = 0;
    /// `/a`.
    pub(crate) const A: usize = 1;
    /// `/a/b`, a file.
    pub(crate) const B: usize = 2;
    /// `/a/c`.
    pub(crate) const C: usize = 3;
    /// `/l -> a/c`.
    pub(crate) const L: usize = 4;
    /// `/a/abs -> /a/b`.
    pub(crate) const ABS: usize = 5;
    /// `/loop -> loop`.
    pub(crate) const LOOP: usize = 6;
    /// `/a/xxx...x` (40 bytes, longer than `NAMECACHE_MAXLEN`).
    pub(crate) const LONG: usize = 7;
    /// The number of nodes.
    pub(crate) const N: usize = 8;

    /// The tree.
    pub(crate) static NODES: [Node; N] = [
        Node {
            name: b"/",
            parent: ROOT,
            kind: Kind::Dir,
        },
        Node {
            name: b"a",
            parent: ROOT,
            kind: Kind::Dir,
        },
        Node {
            name: b"b",
            parent: A,
            kind: Kind::Reg,
        },
        Node {
            name: b"c",
            parent: A,
            kind: Kind::Dir,
        },
        Node {
            name: b"l",
            parent: ROOT,
            kind: Kind::Lnk(b"a/c"),
        },
        Node {
            name: b"abs",
            parent: A,
            kind: Kind::Lnk(b"/a/b"),
        },
        Node {
            name: b"loop",
            parent: ROOT,
            kind: Kind::Lnk(b"loop"),
        },
        Node {
            name: b"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            parent: A,
            kind: Kind::Reg,
        },
    ];

    /// Each node's vnode, while it has one (the "inode hash").
    static VTAB: [AtomicPtr<Vnode>; N] = [const { AtomicPtr::new(ptr::null_mut()) }; N];
    /// Each node's lock: 1 while held.
    static LOCKED: [AtomicU32; N] = [const { AtomicU32::new(0) }; N];
    /// How many times `VOP_INACTIVE` ran.
    pub(crate) static INACTIVE: AtomicUsize = AtomicUsize::new(0);
    /// How many times `VOP_RECLAIM` ran.
    pub(crate) static RECLAIM: AtomicUsize = AtomicUsize::new(0);
    /// The mount.
    static MP: AtomicPtr<Mount> = AtomicPtr::new(ptr::null_mut());

    /// Forgets every vnode and lock (the pools behind them are new after `setup`).
    pub(crate) fn reset() {
        for i in 0..N {
            VTAB[i].store(ptr::null_mut(), Ordering::SeqCst);
            LOCKED[i].store(0, Ordering::SeqCst);
        }
        INACTIVE.store(0, Ordering::SeqCst);
        RECLAIM.store(0, Ordering::SeqCst);
        MP.store(ptr::null_mut(), Ordering::SeqCst);
    }

    /// The node a testfs vnode stands for.
    pub(crate) fn node_of(vp: &Vnode) -> usize {
        let idx = vp.v_data.get() as usize;
        assert!(idx > 0, "not a testfs vnode");
        idx - 1
    }

    /// The node's vnode, if it has one.
    pub(crate) fn vnode_of(idx: usize) -> Option<&'static Vnode> {
        // SAFETY: only `vget_node` stores here, and only vnodes, which are never freed.
        unsafe { VTAB[idx].load(Ordering::SeqCst).as_ref() }
    }

    /// The use count of every node's vnode (0 for none).
    pub(crate) fn usecounts() -> [u32; N] {
        core::array::from_fn(|i| vnode_of(i).map_or(0, |vp| vp.v_usecount.get()))
    }

    /// Whether any node's lock is held.
    pub(crate) fn any_locked() -> bool {
        LOCKED.iter().any(|l| l.load(Ordering::SeqCst) != 0)
    }

    /// The vnode of node `idx`, referenced and locked (`VFS_VGET`).
    pub(crate) fn vget_node(mp: &'static Mount, idx: usize) -> Result<&'static Vnode, Errno> {
        if let Some(vp) = vnode_of(idx) {
            vget(vp, LK_EXCLUSIVE)?;
            return Ok(vp);
        }
        let vp = getnewvnode(VT_TMPFS, Some(mp), &TESTFS_VOPS)?;
        vp.v_type.set(match NODES[idx].kind {
            Kind::Dir => VDIR,
            Kind::Reg => VREG,
            Kind::Lnk(_) => VLNK,
        });
        vp.v_data
            .set(ptr::without_provenance_mut::<c_void>(idx + 1));
        if idx == ROOT {
            vp.v_flag.set(VROOT);
        }
        vn_lock(vp, LK_EXCLUSIVE)?;
        VTAB[idx].store(ptr::from_ref(vp).cast_mut(), Ordering::SeqCst);
        Ok(vp)
    }

    fn the_mount() -> &'static Mount {
        // SAFETY: `mount_root` stores the mount, which stays allocated while mounted.
        unsafe { &*MP.load(Ordering::SeqCst) }
    }

    fn testfs_mount(
        _mp: &'static Mount,
        _path: &[u8],
        _data: &mut [u8],
        _ndp: &mut crate::sys::namei::Nameidata<'_>,
        _p: &Proc,
    ) -> Result<(), Errno> {
        Ok(())
    }

    fn testfs_root(mp: &'static Mount) -> Result<&'static Vnode, Errno> {
        vget_node(mp, ROOT)
    }

    fn testfs_vget(mp: &'static Mount, ino: u64) -> Result<&'static Vnode, Errno> {
        vget_node(mp, ino as usize)
    }

    /// `vfsops` of testfs: only the root and the inode lookup do anything.
    pub(crate) static TESTFS_VFSOPS: Vfsops = Vfsops {
        vfs_mount: testfs_mount,
        vfs_start: |_, _, _| Ok(()),
        vfs_unmount: |_, _, _| Ok(()),
        vfs_root: testfs_root,
        vfs_quotactl: |_, _, _, _, _| eopnotsupp(),
        vfs_statfs: |_, _, _| Ok(()),
        vfs_sync: |_, _, _, _, _| Ok(()),
        vfs_vget: testfs_vget,
        vfs_fhtovp: |_, _| Err(Errno::EOPNOTSUPP),
        vfs_vptofh: |_, _| eopnotsupp(),
        vfs_init: None,
        vfs_sysctl: None,
        vfs_checkexp: |_, _, _, _| eopnotsupp(),
    };

    /// The configuration entry of testfs.
    pub(crate) static TESTFS_CONF: Vfsconf =
        Vfsconf::new(&TESTFS_VFSOPS, b"testfs", 99, MNT_LOCAL, 0);

    fn testfs_lock(ap: &mut VopLockArgs) -> Result<(), Errno> {
        let l = &LOCKED[node_of(ap.a_vp)];
        if l.load(Ordering::SeqCst) != 0 {
            if ap.a_flags & LK_NOWAIT != 0 {
                return Err(Errno::EBUSY);
            }
            std::panic!("testfs: node {} locked twice", node_of(ap.a_vp));
        }
        l.store(1, Ordering::SeqCst);
        Ok(())
    }

    fn testfs_unlock(ap: &mut VopUnlockArgs) -> Result<(), Errno> {
        let l = &LOCKED[node_of(ap.a_vp)];
        assert_eq!(
            l.load(Ordering::SeqCst),
            1,
            "testfs: unlock of an unlocked node"
        );
        l.store(0, Ordering::SeqCst);
        Ok(())
    }

    fn testfs_islocked(ap: &mut VopIslockedArgs) -> i32 {
        LOCKED[node_of(ap.a_vp)].load(Ordering::SeqCst) as i32
    }

    fn testfs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
        INACTIVE.fetch_add(1, Ordering::SeqCst);
        VOP_UNLOCK(ap.a_vp)
    }

    fn testfs_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
        RECLAIM.fetch_add(1, Ordering::SeqCst);
        let idx = node_of(ap.a_vp);
        VTAB[idx].store(ptr::null_mut(), Ordering::SeqCst);
        LOCKED[idx].store(0, Ordering::SeqCst);
        ap.a_vp.v_data.set(ptr::null_mut());
        Ok(())
    }

    /// The lookup of `ufs_lookup`, over the fixed tree: the name cache first, then the
    /// directory; the parent stays locked only for `LOCKPARENT` on the last component.
    fn testfs_lookup(ap: &mut VopLookupArgs<'_>) -> Result<(), Errno> {
        let dvp = ap.a_dvp;
        let cnp = &mut *ap.a_cnp;
        *ap.a_vpp = None;
        if dvp.v_type.get() != VDIR {
            return Err(Errno::ENOTDIR);
        }
        let lockparent = cnp.cn_flags & LOCKPARENT != 0;
        let islast = cnp.cn_flags & ISLASTCN != 0;

        match cache_lookup(dvp, cnp) {
            Ok(Some(vp)) => {
                *ap.a_vpp = Some(vp);
                return Ok(());
            }
            Ok(None) => {}
            Err(e) => return Err(e),
        }

        let dir = node_of(dvp);
        let name = cnp.name();
        let found = if name == b"." {
            Some(dir)
        } else if cnp.cn_flags & ISDOTDOT != 0 {
            Some(NODES[dir].parent)
        } else {
            (1..N).find(|&i| NODES[i].parent == dir && NODES[i].name == name)
        };
        let Some(idx) = found else {
            if (cnp.cn_nameiop == CREATE || cnp.cn_nameiop == RENAME) && islast {
                if !lockparent {
                    let _ = VOP_UNLOCK(dvp);
                    cnp.cn_flags |= PDIRUNLOCK;
                }
                return Err(Errno::EJUSTRETURN);
            }
            if cnp.cn_flags & MAKEENTRY != 0 && cnp.cn_nameiop != CREATE {
                cache_enter(dvp, None, cnp);
            }
            return Err(Errno::ENOENT);
        };

        let mp = the_mount();
        let vp = if idx == dir {
            vref(dvp);
            dvp
        } else if cnp.cn_flags & ISDOTDOT != 0 {
            let _ = VOP_UNLOCK(dvp);
            cnp.cn_flags |= PDIRUNLOCK;
            let vp = vget_node(mp, idx)?;
            if lockparent && islast {
                vn_lock(dvp, LK_EXCLUSIVE)?;
                cnp.cn_flags &= !PDIRUNLOCK;
            }
            vp
        } else {
            let vp = vget_node(mp, idx)?;
            if !lockparent || !islast {
                let _ = VOP_UNLOCK(dvp);
                cnp.cn_flags |= PDIRUNLOCK;
            }
            vp
        };
        if cnp.cn_flags & MAKEENTRY != 0 {
            cache_enter(dvp, Some(vp), cnp);
        }
        *ap.a_vpp = Some(vp);
        Ok(())
    }

    fn testfs_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
        let idx = node_of(ap.a_vp);
        let va = &mut *ap.a_vap;
        *va = crate::sys::vnode::Vattr::new();
        va.va_type = ap.a_vp.v_type.get();
        va.va_mode = 0o755;
        va.va_nlink = 1;
        va.va_fsid = 99;
        va.va_fileid = 100 + idx as u64;
        va.va_size = match NODES[idx].kind {
            Kind::Lnk(target) => target.len() as u64,
            _ => 512,
        };
        va.va_blocksize = 512;
        va.va_bytes = 512;
        Ok(())
    }

    fn testfs_readlink(ap: &mut VopReadlinkArgs<'_, '_>) -> Result<(), Errno> {
        let Kind::Lnk(target) = NODES[node_of(ap.a_vp)].kind else {
            return Err(Errno::EINVAL);
        };
        let mut buf = target.to_vec();
        uiomove(&mut buf, ap.a_uio)
    }

    /// One `struct dirent` record for `name`.
    fn dirent(fileno: u64, type_: u8, name: &[u8]) -> std::vec::Vec<u8> {
        let reclen = dirent_recsize(name.len());
        let mut rec = std::vec![0u8; reclen];
        rec[..8].copy_from_slice(&fileno.to_ne_bytes());
        rec[16..18].copy_from_slice(&(reclen as u16).to_ne_bytes());
        rec[18] = type_;
        rec[19] = name.len() as u8;
        rec[Dirent::NAME_OFFSET..Dirent::NAME_OFFSET + name.len()].copy_from_slice(name);
        rec
    }

    /// The whole directory in one call: `.`, `..` and the children.
    fn testfs_readdir(ap: &mut VopReaddirArgs<'_, '_>) -> Result<(), Errno> {
        let dir = node_of(ap.a_vp);
        if ap.a_uio.uio_offset != 0 {
            *ap.a_eofflag = 1;
            return Ok(());
        }
        let mut buf = dirent(100 + dir as u64, DT_DIR, b".");
        buf.extend(dirent(100 + NODES[dir].parent as u64, DT_DIR, b".."));
        for i in (1..N).filter(|&i| NODES[i].parent == dir) {
            let type_ = match NODES[i].kind {
                Kind::Dir => DT_DIR,
                Kind::Reg => DT_REG,
                Kind::Lnk(_) => DT_LNK,
            };
            buf.extend(dirent(100 + i as u64, type_, NODES[i].name));
        }
        *ap.a_eofflag = 1;
        uiomove(&mut buf, ap.a_uio)
    }

    fn testfs_access(_ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
        Ok(())
    }

    /// `vops` of testfs.
    pub(crate) static TESTFS_VOPS: Vops = Vops {
        vop_lock: Some(testfs_lock),
        vop_unlock: Some(testfs_unlock),
        vop_islocked: Some(testfs_islocked),
        vop_abortop: Some(vop_generic_abortop),
        vop_access: Some(testfs_access),
        vop_close: Some(|_| Ok(())),
        vop_getattr: Some(testfs_getattr),
        vop_inactive: Some(testfs_inactive),
        vop_lookup: Some(testfs_lookup),
        vop_open: Some(|_| Ok(())),
        vop_readdir: Some(testfs_readdir),
        vop_readlink: Some(testfs_readlink),
        vop_reclaim: Some(testfs_reclaim),
        vop_print: Some(|_| Ok(())),
        ..Vops::EMPTY
    };

    /// Mounts testfs as the root file system the way `main` does after `mountroot`: on the
    /// mount list with `MNT_ROOTFS`, its root as `rootvnode` and `p`'s current directory.
    pub(crate) fn mount_root(p: &Proc) -> &'static Mount {
        let mp = vfs_mount_alloc(None, &TESTFS_CONF);
        vfs_unbusy(mp);
        MP.store(ptr::from_ref(mp).cast_mut(), Ordering::SeqCst);
        // SAFETY: a new mount on no list.
        unsafe { MOUNTLIST.0.insert_tail(mp) };
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_ROOTFS);
        let Ok(root) = VFS_ROOT(mp) else {
            std::panic!("testfs: no root");
        };
        set_rootvnode(Some(root));
        p.fd().fd_cdir.set(Some(root));
        vref(root);
        let _ = VOP_UNLOCK(root);
        mp
    }

    /// `setup` plus testfs mounted as root: the guard, the thread and the mount.
    pub(crate) fn setup_root() -> (MutexGuard<'static, ()>, &'static Proc, &'static Mount) {
        let (guard, p) = setup();
        let mp = mount_root(p);
        (guard, p, mp)
    }

    /// A leaked fresh `Box`, for tests that want a `'static` buffer.
    pub(crate) fn leak<T>(t: T) -> &'static mut T {
        Box::leak(Box::new(t))
    }
}

/// Real memory, the process, file and vfs pools, empty vnode lists; returns the guard and a
/// thread with credentials and a descriptor table (no current directory).
pub(crate) fn setup() -> (MutexGuard<'static, ()>, &'static Proc) {
    let guard = setup_real_memory();
    crate::machine::cons::consinit();
    procinit();
    filedesc_init();
    NUMVNODES.store(0, Ordering::SeqCst);
    NUMCACHE.store(0, Ordering::SeqCst);
    NUMNEG.store(0, Ordering::SeqCst);
    vfsinit();
    testfs::reset();
    set_rootvnode(None);

    let pr: &'static Process = Box::leak(Box::new(Process::new()));
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    p.p_p.set(pr);
    pr.ps_mainproc.set(p);
    let cr = crget();
    p.p_ucred.set(cr);
    pr.ps_ucred.set(crhold(cr));
    let fdp = fdinit();
    pr.ps_fd.set(fdp);
    p.p_fd.set(fdp);
    (guard, p)
}

#[test]
fn vattr_null_sets_every_field_to_vnoval() {
    let mut va = Vattr::new();
    va.va_type = VREG;
    va.va_vaflags = 7;
    vattr_null(&mut va);
    assert_eq!(va.va_type, VNON);
    assert_eq!(va.va_mode, Mode::MAX);
    assert_eq!(va.va_uid, Uid::MAX);
    assert_eq!(va.va_size, u64::MAX);
    assert_eq!(va.va_fsid, -1);
    assert_eq!(va.va_atime.tv_nsec, -1);
    assert_eq!(va.va_ctime.tv_sec, -1);
    assert_eq!(va.va_rdev, -1);
    assert_eq!(va.va_flags, u64::MAX);
    assert_eq!(va.va_vaflags, 0);
}

#[test]
fn vaccess_checks_owner_then_group_then_others() {
    let _g = setup();
    let cr = crget();
    cr.cr_uid.set(1000);
    cr.cr_gid.set(10);
    cr.cr_ngroups.set(1);
    cr.cr_groups[0].set(10);
    // The owner's bits decide for the owner, even when others may.
    assert_eq!(
        vaccess(VREG, 0o077, 1000, 20, VREAD, cr),
        Err(Errno::EACCES)
    );
    assert_eq!(vaccess(VREG, 0o600, 1000, 20, VREAD | VWRITE, cr), Ok(()));
    // Then the group.
    assert_eq!(vaccess(VREG, 0o040, 0, 10, VREAD, cr), Ok(()));
    assert_eq!(vaccess(VREG, 0o040, 0, 10, VWRITE, cr), Err(Errno::EACCES));
    // Then everyone else.
    assert_eq!(vaccess(VDIR, 0o001, 0, 0, VEXEC, cr), Ok(()));
    // Root reads anything, but executes only something executable (directories search).
    cr.cr_uid.set(0);
    assert_eq!(vaccess(VREG, 0o000, 1, 1, VREAD | VWRITE, cr), Ok(()));
    assert_eq!(vaccess(VREG, 0o644, 1, 1, VEXEC, cr), Err(Errno::EACCES));
    assert_eq!(vaccess(VDIR, 0o000, 1, 1, VEXEC, cr), Ok(()));
}

#[test]
fn getnewvnode_puts_the_vnode_on_its_mount() {
    let (_g, _p, mp) = testfs::setup_root();
    let before = NUMVNODES.load(Ordering::SeqCst);
    let vp = getnewvnode(VT_NON, Some(mp), &testfs::TESTFS_VOPS).unwrap();
    assert_eq!(vp.v_usecount.get(), 1);
    assert_eq!(vp.v_type.get(), VNON);
    assert!(vp.v_mount.get().is_some_and(|m| ptr::eq(m, mp)));
    assert!(mp.mnt_vnodelist.iter().any(|v| ptr::eq(v, vp)));
    assert_eq!(NUMVNODES.load(Ordering::SeqCst), before + 1);

    insmntque(vp, None);
    assert!(vp.v_mount.get().is_none());
    assert!(!mp.mnt_vnodelist.iter().any(|v| ptr::eq(v, vp)));
}

#[test]
fn use_counts_and_the_free_list() {
    let (_g, _p, mp) = testfs::setup_root();
    let vp = testfs::vget_node(mp, testfs::B).unwrap();
    assert_eq!(vp.v_usecount.get(), 1);
    assert_eq!(VOP_ISLOCKED(vp), 1);

    vref(vp);
    assert_eq!(vp.v_usecount.get(), 2);
    // vput of a vnode still referenced only unlocks it.
    vput(vp);
    assert_eq!(vp.v_usecount.get(), 1);
    assert_eq!(VOP_ISLOCKED(vp), 0);
    assert_eq!(testfs::INACTIVE.load(Ordering::SeqCst), 0);

    // The last vrele locks, deactivates (which unlocks) and frees the vnode, which keeps its
    // identity on the free list.
    assert!(vrele(vp));
    assert_eq!(vp.v_usecount.get(), 0);
    assert_eq!(testfs::INACTIVE.load(Ordering::SeqCst), 1);
    assert!(!testfs::any_locked());
    assert!(vp.v_bioflag.get() & VBIOONFREELIST != 0);
    assert!(VNODE_FREE_LIST.0.iter().any(|v| ptr::eq(v, vp)));
    assert_eq!(testfs::node_of(vp), testfs::B);

    // vget takes it back off the free list, referenced and locked.
    let again = testfs::vget_node(mp, testfs::B).unwrap();
    assert!(ptr::eq(again, vp));
    assert_eq!(vp.v_usecount.get(), 1);
    assert!(vp.v_bioflag.get() & VBIOONFREELIST == 0);
    assert!(!VNODE_FREE_LIST.0.iter().any(|v| ptr::eq(v, vp)));
    vput(vp);
    assert_eq!(vp.v_usecount.get(), 0);
    assert_eq!(testfs::INACTIVE.load(Ordering::SeqCst), 2);
}

#[test]
fn vhold_moves_a_free_vnode_to_the_hold_list() {
    let (_g, _p, mp) = testfs::setup_root();
    let vp = testfs::vget_node(mp, testfs::B).unwrap();
    vput(vp);
    assert!(VNODE_FREE_LIST.0.iter().any(|v| ptr::eq(v, vp)));
    vhold(vp);
    assert!(VNODE_HOLD_LIST.0.iter().any(|v| ptr::eq(v, vp)));
    assert!(!VNODE_FREE_LIST.0.iter().any(|v| ptr::eq(v, vp)));
    vdrop(vp);
    assert!(VNODE_FREE_LIST.0.iter().any(|v| ptr::eq(v, vp)));
    assert!(!VNODE_HOLD_LIST.0.iter().any(|v| ptr::eq(v, vp)));
}

#[test]
fn vgone_reclaims_and_leaves_a_dead_vnode() {
    let (_g, _p, mp) = testfs::setup_root();
    let vp = testfs::vget_node(mp, testfs::C).unwrap();
    let _ = VOP_UNLOCK(vp);
    let id = vp.v_id.get();

    // An active vnode: closed, deactivated, reclaimed; the reference survives.
    vgone(vp);
    assert_eq!(testfs::RECLAIM.load(Ordering::SeqCst), 1);
    assert_eq!(testfs::INACTIVE.load(Ordering::SeqCst), 1);
    assert_eq!(vp.v_type.get(), VBAD);
    assert!(ptr::eq(vp.op(), &DEAD_VOPS));
    assert!(vp.v_mount.get().is_none());
    assert!(vp.v_data.get().is_null());
    assert_ne!(vp.v_id.get(), id, "cache_purge gives a new capability");
    assert!(testfs::vnode_of(testfs::C).is_none());
    assert_eq!(vp.v_usecount.get(), 1);

    // The dead vnode's last release puts it at the head of the free list, to be recycled
    // first.
    assert!(vrele(vp));
    assert!(VNODE_FREE_LIST.0.first().is_some_and(|v| ptr::eq(v, vp)));
}

#[test]
fn getnewvnode_recycles_from_the_free_list_past_maxvnodes() {
    let (_g, _p, mp) = testfs::setup_root();
    let vp = testfs::vget_node(mp, testfs::B).unwrap();
    vput(vp);
    MAXVNODES.store(1, Ordering::SeqCst);
    let before = NUMVNODES.load(Ordering::SeqCst);
    let nvp = getnewvnode(VT_NON, None, &testfs::TESTFS_VOPS).unwrap();
    assert!(ptr::eq(nvp, vp), "the free vnode is recycled");
    assert_eq!(NUMVNODES.load(Ordering::SeqCst), before);
    assert_eq!(testfs::RECLAIM.load(Ordering::SeqCst), 1);
    assert!(testfs::vnode_of(testfs::B).is_none());
    assert_eq!(nvp.v_usecount.get(), 1);
    assert!(nvp.v_mount.get().is_none());
}

#[test]
fn vflush_counts_busy_vnodes_unless_forced() {
    let (_g, _p, mp) = testfs::setup_root();
    let busy = testfs::vget_node(mp, testfs::B).unwrap();
    let _ = VOP_UNLOCK(busy);
    let idle = testfs::vget_node(mp, testfs::C).unwrap();
    vput(idle);

    // The root (two references) and B are busy; C is idle and goes.
    let root = crate::kern::vfs_init::rootvnode();
    assert_eq!(vflush(mp, root, 0), Err(Errno::EBUSY));
    assert!(testfs::vnode_of(testfs::C).is_none());
    assert!(testfs::vnode_of(testfs::B).is_some());

    assert_eq!(vflush(mp, root, FORCECLOSE), Ok(()));
    assert!(testfs::vnode_of(testfs::B).is_none());
    assert_eq!(busy.v_type.get(), VBAD);
    // Only the skipped root is left on the mount.
    assert_eq!(mp.mnt_vnodelist.iter().count(), 1);
}

#[test]
fn vfs_busy_fails_on_an_unmounting_file_system() {
    let (_g, _p, mp) = testfs::setup_root();
    assert_eq!(vfs_busy(mp, VB_READ | VB_NOWAIT), Ok(()));
    assert!(vfs_isbusy(mp));
    vfs_unbusy(mp);
    assert!(!vfs_isbusy(mp));
    mp.mnt_flag.set(mp.mnt_flag.get() | MNT_UNMOUNT);
    assert_eq!(vfs_busy(mp, VB_READ | VB_NOWAIT), Err(Errno::EBUSY));
    assert!(!vfs_isbusy(mp));
    mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_UNMOUNT);
}

#[test]
fn vfs_getnewfsid_is_unique_among_mounts() {
    let (_g, _p, mp) = testfs::setup_root();
    vfs_getnewfsid(mp);
    let fsid = mp.mnt_stat.get().f_fsid;
    assert!(vfs_getvfs(&fsid).is_some_and(|m| ptr::eq(m, mp)));
    let mp2 = vfs_mount_alloc(None, &testfs::TESTFS_CONF);
    vfs_unbusy(mp2);
    // SAFETY: a new mount on no list.
    unsafe { MOUNTLIST.0.insert_tail(mp2) };
    vfs_getnewfsid(mp2);
    assert_ne!(mp2.mnt_stat.get().f_fsid, fsid);
    assert_eq!(&mp2.mnt_stat.get().f_fstypename[..7], b"testfs\0");
}

#[test]
fn checkalias_is_not_reached_for_regular_vnodes() {
    let (_g, _p, mp) = testfs::setup_root();
    let vp = testfs::vget_node(mp, testfs::B).unwrap();
    assert!(checkalias(vp, 0, Some(mp)).is_none());
    assert!(vp.v_specinfo().is_none());
    vput(vp);
}
