/* <LICENSES> */
/*
 * Copyright (c) 2012-2013 Sylvestre Gallon <ccna.syl@gmail.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! The FUSE file-system-type operations: mount (attach the daemon's fuse(4) device and send
//! `FUSE_INIT`), unmount (`FUSE_DESTROY`), root, statfs (`FUSE_STATFS`), vget (the inode
//! hash, a new node and `FUSE_GETATTR`), init (the fusebuf pool) and the `vfs.fuse` sysctls.
//!
//! Upstream: sys/miscfs/fuse/fuse_vfsops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `fusefs_mount` reads its `struct fusefs_args` out of the kernel copy of the mount
//!   arguments (`FusefsArgs::from_bytes`), `EINVAL` when they are short; the C dereferences
//!   the pointer it is given.
//! - `fusefs_root` and `fusefs_vget` return the vnode (`Vfsops`'s shape).
//! - The `struct fusefs_mnt` and the nodes are `malloc(M_FUSEFS)`ed and a fresh value
//!   ([`FusefsNode::new`]) written into them; the node's lock is then set up by
//!   `rrw_init_flags` as in C.
//! - `fusefs_vars[]`'s `&fusefs_fbuf_pool.pr_npages` is the `AtomicI32`
//!   `FUSEFS_POOL_NPAGES`, which `fusefs_sysctl` refreshes from the pool before each lookup:
//!   the pool keeps the count in a `Cell<u32>`, and `sysctl_bounded_arr` reads atomics.
//! - `fusefs_vget`'s `curproc->p_ucred, curproc` is `curp()`, which panics without a thread.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use libkern::strlcpy;

use crate::kern::kern_descrip::fd_getfile;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_prot::suser_ucred;
use crate::kern::kern_rwlock::rrw_init_flags;
use crate::kern::kern_sysctl::sysctl_bounded_arr;
use crate::kern::subr_pool::pool_init;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_subr::{copy_statfs_info, getnewvnode, vflush, vfs_getnewfsid, vrele};
use crate::kern::vfs_vops::VOP_GETATTR;
use crate::machine::intr::IPL_NONE;
use crate::miscfs::fuse::fuse_device::{
    STAT_FBUFS_IN, STAT_FBUFS_WAIT, STAT_OPENED_FUSEDEV, fuse_device_cleanup,
    fuse_device_queue_fbuf, fuse_device_set_fmp,
};
use crate::miscfs::fuse::fuse_ihash::{fuse_ihashget, fuse_ihashinit, fuse_ihashins};
use crate::miscfs::fuse::fuse_vnops::{FUSEFS_VOPS, curp};
use crate::miscfs::fuse::fusebuf::{fb_delete, fb_queue, fb_setup};
use crate::miscfs::fuse::fusefs::{
    FUSEFS_INFBUFS, FUSEFS_OPENDEVS, FUSEFS_POOL_NBPAGES, FUSEFS_WAITFBUFS, FusefsMnt, VFSTOFUSEFS,
};
use crate::miscfs::fuse::fusefs_node::{
    FUFH_INVALID, FUFH_MAXTYPE, FufhType, FusefsFilehandle, FusefsNode, VTOI,
};
use crate::sys::errno::Errno;
use crate::sys::file::{DTYPE_VNODE, frele};
use crate::sys::fusebuf::{
    FUSE_DESTROY, FUSE_INIT, FUSE_KERNEL_MINOR_VERSION, FUSE_KERNEL_VERSION, FUSE_ROOT_ID,
    FUSE_STATFS, FUSEBUFMAXSIZE, FuseInitIn, FuseStatfsOut, Fusebuf,
};
use crate::sys::malloc::{M_FUSEFS, M_WAITOK, M_ZERO};
use crate::sys::mount::{
    Fid, FusefsArgs, MNAMELEN, MNT_FORCE, MNT_UPDATE, Mount, Statfs, VFS_VGET, Vfsconf, Vfsops,
};
use crate::sys::namei::Nameidata;
use crate::sys::param::BLKDEV_IOSIZE;
use crate::sys::pool::{PR_WAITOK, Pool};
use crate::sys::proc::Proc;
use crate::sys::rwlock::{RWL_DUPOK, RWL_IS_VNODE};
use crate::sys::sysctl::SysctlBoundedArgs;
use crate::sys::types::Ino;
use crate::sys::ucred::Ucred;
use crate::sys::vnode::{FORCECLOSE, VCHR, VDIR, VROOT, VT_FUSEFS, Vattr, Vnode};

/// `PENDING`: `FUSE_INIT` reply not yet received.
pub const PENDING: i32 = 2;

/// `fusefs_fbuf_pool`: the fusebufs.
pub static FUSEFS_FBUF_POOL: Pool = Pool::new();

/// `fusefs_fbuf_pool.pr_npages` as `fusefs_vars[]` reads it (see the module's deviations).
pub static FUSEFS_POOL_NPAGES: AtomicI32 = AtomicI32::new(0);

/// `fusefs_vfsops`.
pub static FUSEFS_VFSOPS: Vfsops = Vfsops {
    vfs_mount: fusefs_mount,
    vfs_start: fusefs_start,
    vfs_unmount: fusefs_unmount,
    vfs_root: fusefs_root,
    vfs_quotactl: fusefs_quotactl,
    vfs_statfs: fusefs_statfs,
    vfs_sync: fusefs_sync,
    vfs_vget: fusefs_vget,
    vfs_fhtovp: fusefs_fhtovp,
    vfs_vptofh: fusefs_vptofh,
    vfs_init: Some(fusefs_init),
    vfs_sysctl: Some(fusefs_sysctl),
    vfs_checkexp: fusefs_checkexp,
};

/// `fusefs_vars[]`: the `vfs.fuse` sysctls, all read-only.
static FUSEFS_VARS: [SysctlBoundedArgs; 4] = [
    SysctlBoundedArgs::readonly(FUSEFS_OPENDEVS, &STAT_OPENED_FUSEDEV),
    SysctlBoundedArgs::readonly(FUSEFS_INFBUFS, &STAT_FBUFS_IN),
    SysctlBoundedArgs::readonly(FUSEFS_WAITFBUFS, &STAT_FBUFS_WAIT),
    SysctlBoundedArgs::readonly(FUSEFS_POOL_NBPAGES, &FUSEFS_POOL_NPAGES),
];

/// `fusefs_mount` (`vfs_mount`): mount system call, made by the daemon: `data` is the kernel
/// copy of its `struct fusefs_args`, whose `fd` is its open fuse(4) device.
pub fn fusefs_mount(
    mp: &'static Mount,
    path: &[u8],
    data: &mut [u8],
    _ndp: &mut Nameidata<'_>,
    p: &Proc,
) -> Result<(), Errno> {
    if mp.mnt_flag.get() & MNT_UPDATE != 0 {
        return Err(Errno::EOPNOTSUPP);
    }

    let Some(args) = FusefsArgs::from_bytes(data) else {
        return Err(Errno::EINVAL);
    };

    let Some(fp) = fd_getfile(p.fd(), args.fd) else {
        return Err(Errno::EBADF);
    };

    let error = 'bad: {
        if fp.f_type.get() != DTYPE_VNODE {
            break 'bad Err(Errno::EINVAL);
        }

        let vp = fp.vnode();
        if vp.v_type.get() != VCHR {
            break 'bad Err(Errno::EBADF);
        }

        // Only root may specify allow_other.
        if args.allow_other != 0
            && let Err(e) = suser_ucred(p.ucred())
        {
            break 'bad Err(e);
        }

        let Some(mem) = malloc(size_of::<FusefsMnt>(), M_FUSEFS, M_WAITOK | M_ZERO) else {
            panic(format_args!("fusefs_mount: malloc(M_WAITOK) failed"));
        };
        let fmp_ptr = mem.cast::<FusefsMnt>();
        let max_read = if args.max_read > 0 {
            args.max_read.min(FUSEBUFMAXSIZE as i32)
        } else {
            FUSEBUFMAXSIZE as i32
        };
        // SAFETY: a fresh block of `size_of::<FusefsMnt>()` bytes, aligned by `malloc`,
        // written once; it lives until `fusefs_unmount` frees it.
        let fmp: &'static FusefsMnt = unsafe {
            fmp_ptr.as_ptr().write(FusefsMnt {
                mp,
                undef_op: core::cell::Cell::new(0),
                max_read,
                // Initialise to a safe value just to be sure. This will be overwritten when
                // the file system responds to FUSE_INIT.
                max_write: core::cell::Cell::new(BLKDEV_IOSIZE as i32),
                sess_init: core::cell::Cell::new(PENDING),
                allow_other: args.allow_other,
                dev: vp.v_rdev(),
            });
            fmp_ptr.as_ref()
        };

        mp.mnt_data
            .set(ptr::from_ref(fmp).cast_mut().cast::<c_void>());
        vfs_getnewfsid(mp);

        mp.update_stat(|sp| {
            sp.f_mntonname = [0; MNAMELEN];
            strlcpy(&mut sp.f_mntonname[..MNAMELEN - 1], path);
            sp.f_mntfromname = [0; MNAMELEN];
            strlcpy(&mut sp.f_mntfromname[..MNAMELEN - 1], b"fusefs");
            sp.f_mntfromspec = [0; MNAMELEN];
            strlcpy(&mut sp.f_mntfromspec[..MNAMELEN - 1], b"fusefs");
        });

        fuse_device_set_fmp(fmp, true);
        let fbuf = fb_setup(0, 0, FUSE_INIT, p);
        fbuf.op_set(&FuseInitIn {
            major: FUSE_KERNEL_VERSION,
            minor: FUSE_KERNEL_MINOR_VERSION,
            max_readahead: 0,
            flags: 0, // OpenBSD supports nothing...
        });

        // cannot tsleep on mount
        fuse_device_queue_fbuf(fmp.dev, fbuf);

        Ok(())
    };

    let _ = frele(fp, p);
    error
}

/// `fusefs_start` (`vfs_start`).
pub fn fusefs_start(_mp: &'static Mount, _flags: i32, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

/// `fusefs_unmount` (`vfs_unmount`): flushes the vnodes, tells a live daemon
/// (`FUSE_DESTROY`), fails what is still queued and detaches the device.
pub fn fusefs_unmount(mp: &'static Mount, mntflags: i32, p: &Proc) -> Result<(), Errno> {
    let fmp = VFSTOFUSEFS(mp);
    let mut flags = 0;

    if mntflags & MNT_FORCE != 0 {
        flags |= FORCECLOSE;
    }

    vflush(mp, None, flags)?;

    if fmp.sess_init.get() != 0 && fmp.sess_init.get() != PENDING {
        let fbuf = fb_setup(0, 0, FUSE_DESTROY, p);

        let _error = fb_queue(fmp.dev, fbuf);
        // if (error) DPRINTF("error %d on destroy\n", error);

        fb_delete(fbuf);
    }
    fmp.sess_init.set(0);

    fuse_device_cleanup(fmp.dev);
    fuse_device_set_fmp(fmp, false);
    free(
        NonNull::from(fmp).cast::<u8>(),
        M_FUSEFS,
        size_of::<FusefsMnt>(),
    );
    mp.mnt_data.set(ptr::null_mut());

    Ok(())
}

/// `fusefs_root` (`vfs_root`): the root directory's vnode (`FUSE_ROOT_ID`), locked.
pub fn fusefs_root(mp: &'static Mount) -> Result<&'static Vnode, Errno> {
    let nvp = VFS_VGET(mp, FUSE_ROOT_ID)?;

    nvp.v_type.set(VDIR);

    Ok(nvp)
}

/// `fusefs_quotactl` (`vfs_quotactl`).
pub fn fusefs_quotactl(
    _mp: &'static Mount,
    _cmds: i32,
    _uid: crate::sys::types::Uid,
    _arg: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `fusefs_statfs` (`vfs_statfs`): the daemon's `FUSE_STATFS` answer once the session is
/// up, zeros before.
pub fn fusefs_statfs(mp: &'static Mount, sbp: &mut Statfs, p: &Proc) -> Result<(), Errno> {
    let fmp = VFSTOFUSEFS(mp);

    // Deny other users unless allow_other mount option was specified.
    if fmp.allow_other == 0 && p.ucred().cr_uid.get() != mp.mnt_stat.get().f_owner {
        return Err(Errno::EPERM);
    }

    copy_statfs_info(sbp, mp);

    // Both FUSE_INIT and FUSE_STATFS are sent to the FUSE file system daemon when it is
    // mounted. However, the daemon is the process that called mount(2) so to prevent a
    // deadlock return dummy values until the response to FUSE_INIT init is received. All
    // other VFS syscalls are queued.
    if fmp.sess_init.get() == 0 || fmp.sess_init.get() == PENDING {
        sbp.f_bavail = 0;
        sbp.f_bfree = 0;
        sbp.f_blocks = 0;
        sbp.f_ffree = 0;
        sbp.f_favail = 0;
        sbp.f_files = 0;
        sbp.f_bsize = 0;
        sbp.f_iosize = 0;
        sbp.f_namemax = 0;
    } else {
        let fbuf = fb_setup(0, FUSE_ROOT_ID, FUSE_STATFS, p);

        if let Err(error) = fb_queue(fmp.dev, fbuf) {
            fb_delete(fbuf);
            return Err(error);
        }

        let st = fbuf.op_get::<FuseStatfsOut>().st;
        sbp.f_blocks = st.blocks;
        sbp.f_bfree = st.bfree;
        sbp.f_bavail = st.bavail as i64;
        sbp.f_files = st.files;
        sbp.f_ffree = st.ffree;
        sbp.f_favail = st.ffree as i64;
        sbp.f_bsize = st.frsize;
        sbp.f_namemax = st.namelen;
        sbp.f_iosize = st.bsize;

        // Programs use this to allocate an i/o buffer so ensure it's sane.
        //
        // XXX Should this be larger?
        if sbp.f_iosize as usize > BLKDEV_IOSIZE {
            sbp.f_iosize = BLKDEV_IOSIZE as u32;
        }

        fb_delete(fbuf);
    }

    Ok(())
}

/// `fusefs_sync` (`vfs_sync`).
pub fn fusefs_sync(
    _mp: &'static Mount,
    _waitfor: i32,
    _stall: i32,
    _cred: *const Ucred,
    _p: &Proc,
) -> Result<(), Errno> {
    Ok(())
}

/// `fusefs_vget` (`vfs_vget`): the vnode of inode `ino`, referenced and locked: from the
/// inode hash (one more lookup), or a new node whose size `FUSE_GETATTR` gives.
pub fn fusefs_vget(mp: &'static Mount, ino: Ino) -> Result<&'static Vnode, Errno> {
    loop {
        // retry:
        let fmp = VFSTOFUSEFS(mp);

        // check if vnode is in hash.
        if let Some(vp) = fuse_ihashget(fmp, ino) {
            let ip = VTOI(vp);
            ip.nlookup.set(ip.nlookup.get() + 1);
            return Ok(vp);
        }

        // if not create it
        let nvp = getnewvnode(VT_FUSEFS, Some(mp), &FUSEFS_VOPS)?;
        // if error: DPRINTF("getnewvnode error: %d\n", error);

        let Some(mem) = malloc(size_of::<FusefsNode>(), M_FUSEFS, M_WAITOK | M_ZERO) else {
            panic(format_args!("fusefs_vget: malloc(M_WAITOK) failed"));
        };
        let ip_ptr = mem.cast::<FusefsNode>();
        // SAFETY: a fresh block of `size_of::<FusefsNode>()` bytes, aligned by `malloc`,
        // written once; it lives until `fusefs_reclaim` frees it.
        let ip: &'static FusefsNode = unsafe {
            ip_ptr.as_ptr().write(FusefsNode::new(nvp, fmp, ino));
            ip_ptr.as_ref()
        };
        rrw_init_flags(&ip.i_lock, "fuseinode", RWL_DUPOK | RWL_IS_VNODE);
        nvp.v_data
            .set(ptr::from_ref(ip).cast_mut().cast::<c_void>());
        ip.nlookup.set(1);

        for i in 0..FUFH_MAXTYPE.idx() {
            ip.set_fufh(
                FufhType::from_idx(i),
                FusefsFilehandle {
                    fh_id: ip.fufh(FufhType::from_idx(i)).fh_id,
                    fh_type: FUFH_INVALID,
                },
            );
        }

        if let Err(error) = fuse_ihashins(ip) {
            vrele(nvp);

            if error == Errno::EEXIST {
                continue;
            }

            return Err(error);
        }

        if ino == FUSE_ROOT_ID {
            nvp.v_flag.set(nvp.v_flag.get() | VROOT);
        } else {
            // Initialise the file size so that file size changes can be detected during
            // file operations.
            let p = curp();
            let mut vattr = Vattr::new();
            if let Err(error) = VOP_GETATTR(nvp, &mut vattr, ptr::from_ref(p.ucred()), p) {
                vrele(nvp);
                return Err(error);
            }
            ip.filesize.set(vattr.va_size as i64);
        }

        return Ok(nvp);
    }
}

/// `fusefs_fhtovp` (`vfs_fhtovp`): FUSE has no file handles.
pub fn fusefs_fhtovp(_mp: &'static Mount, _fhp: &Fid) -> Result<&'static Vnode, Errno> {
    Err(Errno::EINVAL)
}

/// `fusefs_vptofh` (`vfs_vptofh`).
pub fn fusefs_vptofh(_vp: &'static Vnode, _fhp: &mut Fid) -> Result<(), Errno> {
    Err(Errno::EINVAL)
}

/// `fusefs_init` (`vfs_init`): the fusebuf pool and the inode hash.
pub fn fusefs_init(_vfc: &'static Vfsconf) -> Result<(), Errno> {
    pool_init(
        &FUSEFS_FBUF_POOL,
        size_of::<Fusebuf>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "fmsg",
        None,
    );
    fuse_ihashinit();

    Ok(())
}

/// `fusefs_sysctl` (`vfs_sysctl`): `vfs.fuse.*`.
pub fn fusefs_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    FUSEFS_POOL_NPAGES.store(FUSEFS_FBUF_POOL.pr_npages.get() as i32, Ordering::Relaxed);
    sysctl_bounded_arr(&FUSEFS_VARS, name, oldp, oldlenp, newp, newlen)
}

/// `fusefs_checkexp` (`vfs_checkexp`): FUSE is not exported.
pub fn fusefs_checkexp(
    _mp: &'static Mount,
    _nam: &crate::sys::mbuf::Mbuf,
    _extflagsp: &mut i32,
    _credanonp: &mut *const Ucred,
) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for a FUSE mount driven end to end by a simulated daemon: the daemon side
    // reads every request from `/dev/fuse0` and writes its reply as libfuse does
    // (`fuse_device.rs`'s `read_req`/`reply`), from `fb_queue` (see `fusebuf.rs`'s
    // deviations: the host has no context switch). The file system it serves has a root
    // directory (inode 1) holding one file, `hello` (inode 2). Covered: `fusefs_mount` (the
    // `FUSE_INIT` it queues, answered on the first request), `statfs` before and after the
    // session starts, the root, lookups through the inode hash, `getattr`, `open`, `read`,
    // `write`, `setattr`, `readdir` in two calls, the vnode release (`FUSE_RELEASE*`,
    // `FUSE_FORGET` on reclaim) and `fusefs_unmount` (`FUSE_DESTROY`).

    use core::cell::RefCell;
    use core::mem::offset_of;
    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::kern_descrip::fnew;
    use crate::kern::kern_event::tests::install;
    use crate::kern::vfs_lookup::ndinit;
    use crate::kern::vfs_subr::{cdevvp, vattr_null, vfs_mount_alloc, vfs_unbusy, vput};
    use crate::kern::vfs_vops::{
        VOP_GETATTR, VOP_LOOKUP, VOP_OPEN, VOP_READ, VOP_READDIR, VOP_SETATTR, VOP_UNLOCK,
        VOP_WRITE,
    };
    use crate::miscfs::fuse::fuse_device::fuseclose;
    use crate::miscfs::fuse::fuse_device::tests::{Req, open_dev, read_req, reply, setup};
    use crate::miscfs::fuse::fusebuf::TEST_DAEMON;
    use crate::miscfs::fuse::fusefs_node::{FUFH_RDONLY, FUFH_WRONLY};
    use crate::sys::dirent::{DT_DIR, DT_REG, Dirent};
    use crate::sys::fcntl::{FREAD, FWRITE, O_RDONLY, O_WRONLY};
    use crate::sys::fusebuf::{
        FUSE_DESTROY, FUSE_FORGET, FUSE_GETATTR, FUSE_LOOKUP, FUSE_OPEN, FUSE_OPENDIR, FUSE_READ,
        FUSE_READDIR, FUSE_RELEASE, FUSE_RELEASEDIR, FUSE_SETATTR, FUSE_WRITE, FuseAttr,
        FuseAttrOut, FuseDirent, FuseEntryOut, FuseForgetIn, FuseInitOut, FuseKstatfs, FuseOpenIn,
        FuseOpenOut, FuseReadIn, FuseReleaseIn, FuseSetattrIn, FuseWriteIn, FuseWriteOut,
        abi_bytes, fuse_dirent_size,
    };
    use crate::sys::mount::VFS_ROOT;
    use crate::sys::namei::{Componentname, ISLASTCN, LOCKPARENT, LOOKUP, NiDirp};
    use crate::sys::types::{Dev, Off};
    use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
    use crate::sys::vnode::VREG;

    /// The file's initial contents.
    const HELLO: &[u8] = b"hello, world\n";

    std::thread_local! {
        /// The opcodes the daemon was sent, in order.
        static LOG: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
        /// The contents of `hello`.
        static CONTENT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        /// The `FUSE_FORGET` counts received, by inode.
        static FORGOTTEN: RefCell<Vec<(u64, u64)>> = const { RefCell::new(Vec::new()) };
    }

    /// The configuration entry of FUSE in `vfsconflist[]`.
    fn fuse_conf() -> &'static Vfsconf {
        crate::kern::vfs_init::vfs_byname(b"fuse").expect("fuse in vfsconflist[]")
    }

    /// The attributes of inode 1 (the root) or 2 (`hello`).
    fn attr(ino: u64) -> FuseAttr {
        let mut a = FuseAttr {
            ino,
            ..FuseAttr::default()
        };
        if ino == 1 {
            a.mode = 0o040755;
            a.nlink = 2;
            a.size = 512;
        } else {
            a.mode = 0o100644;
            a.nlink = 1;
            a.size = CONTENT.with(|c| c.borrow().len()) as u64;
            a.mtime = 1_700_000_000;
            a.mtimensec = 5;
        }
        a
    }

    /// The root directory's entries as `FUSE_READDIR` returns them from `offset` (each entry's
    /// offset is the next one's), at most `size` bytes.
    fn dirents(offset: u64, size: usize) -> Vec<u8> {
        let all: [(u64, &[u8], u32); 3] = [(1, b".", 4), (1, b"..", 4), (2, b"hello", 8)];
        let mut out = Vec::new();
        for (i, (ino, name, ty)) in all.iter().enumerate().skip(offset as usize) {
            let d = FuseDirent {
                ino: *ino,
                off: i as u64 + 1,
                namelen: name.len() as u32,
                r#type: *ty,
                name: [],
            };
            let len = fuse_dirent_size(&d);
            if out.len() + len > size {
                break;
            }
            let start = out.len();
            out.extend_from_slice(abi_bytes(&d));
            out.extend_from_slice(name);
            out.resize(start + len, 0);
        }
        out
    }

    /// One answer of the simulated daemon.
    fn answer(dev: Dev, req: &Req) {
        let unique = req.hdr.unique;
        let ok = |out: &[u8], data: &[u8]| reply(dev, unique, 0, out, data).expect("reply");
        match req.hdr.opcode {
            FUSE_INIT => {
                let init = FuseInitOut {
                    major: 7,
                    minor: 19,
                    max_write: 65536,
                    ..FuseInitOut::default()
                };
                ok(abi_bytes(&init), &[]);
            }
            FUSE_GETATTR => {
                let out = FuseAttrOut {
                    attr: attr(req.hdr.nodeid),
                    ..FuseAttrOut::default()
                };
                ok(abi_bytes(&out), &[]);
            }
            FUSE_LOOKUP => {
                if req.hdr.nodeid == 1 && req.data == b"hello\0" {
                    let out = FuseEntryOut {
                        nodeid: 2,
                        attr: attr(2),
                        ..FuseEntryOut::default()
                    };
                    ok(abi_bytes(&out), &[]);
                } else {
                    reply(dev, unique, -(Errno::ENOENT as i32), &[], &[]).expect("reply");
                }
            }
            FUSE_OPEN | FUSE_OPENDIR => {
                let flags = req.op::<FuseOpenIn>().flags as i32;
                let fh = if req.hdr.opcode == FUSE_OPEN {
                    10 + flags as u64
                } else {
                    20
                };
                let out = FuseOpenOut {
                    fh,
                    ..FuseOpenOut::default()
                };
                ok(abi_bytes(&out), &[]);
            }
            FUSE_READ => {
                let r = req.op::<FuseReadIn>();
                let data = CONTENT.with(|c| {
                    let c = c.borrow();
                    let start = (r.offset as usize).min(c.len());
                    let end = (start + r.size as usize).min(c.len());
                    c[start..end].to_vec()
                });
                ok(&[], &data);
            }
            FUSE_WRITE => {
                let w = req.op::<FuseWriteIn>();
                assert_eq!(w.size as usize, req.data.len());
                CONTENT.with(|c| {
                    let mut c = c.borrow_mut();
                    let end = w.offset as usize + req.data.len();
                    if c.len() < end {
                        c.resize(end, 0);
                    }
                    c[w.offset as usize..end].copy_from_slice(&req.data);
                });
                let out = FuseWriteOut {
                    size: w.size,
                    ..FuseWriteOut::default()
                };
                ok(abi_bytes(&out), &[]);
            }
            FUSE_SETATTR => {
                let s = req.op::<FuseSetattrIn>();
                assert_eq!(s.valid, crate::sys::fusebuf::FUSE_FATTR_SIZE);
                CONTENT.with(|c| c.borrow_mut().resize(s.size as usize, 0));
                let out = FuseAttrOut {
                    attr: attr(req.hdr.nodeid),
                    ..FuseAttrOut::default()
                };
                ok(abi_bytes(&out), &[]);
            }
            FUSE_READDIR => {
                let r = req.op::<FuseReadIn>();
                assert_eq!(r.fh, 20, "the handle FUSE_OPENDIR gave");
                ok(&[], &dirents(r.offset, r.size as usize));
            }
            FUSE_STATFS => {
                let st = FuseKstatfs {
                    blocks: 100,
                    bfree: 50,
                    bavail: 40,
                    files: 10,
                    ffree: 5,
                    bsize: 65536,
                    namelen: 255,
                    frsize: 512,
                    ..FuseKstatfs::default()
                };
                ok(abi_bytes(&crate::sys::fusebuf::FuseStatfsOut { st }), &[]);
            }
            FUSE_RELEASE | FUSE_RELEASEDIR => {
                let _ = req.op::<FuseReleaseIn>();
                ok(&[], &[]);
            }
            FUSE_FORGET => {
                let n = req.op::<FuseForgetIn>().nlookup;
                FORGOTTEN.with(|f| f.borrow_mut().push((req.hdr.nodeid, n)));
            }
            FUSE_DESTROY => ok(&[], &[]),
            op => std::panic!("the test daemon got opcode {op}"),
        }
    }

    /// The simulated daemon: answers every request waiting on the device.
    fn daemon(dev: Dev) {
        while let Some(req) = read_req(dev) {
            LOG.with(|l| l.borrow_mut().push(req.hdr.opcode));
            answer(dev, &req);
        }
    }

    /// The bytes of a `struct fusefs_args` as `sys_mount` copies them in.
    fn args(fd: i32, max_read: i32, allow_other: i32) -> [u8; FusefsArgs::SIZE] {
        let mut b = [0u8; FusefsArgs::SIZE];
        let mut put = |off: usize, v: &[u8]| b[off..off + v.len()].copy_from_slice(v);
        put(offset_of!(FusefsArgs, fd), &fd.to_ne_bytes());
        put(offset_of!(FusefsArgs, max_read), &max_read.to_ne_bytes());
        put(
            offset_of!(FusefsArgs, allow_other),
            &allow_other.to_ne_bytes(),
        );
        b
    }

    /// The daemon's descriptor `fd` for `/dev/fuse0`, a character device vnode, as `open(2)`
    /// leaves it in its table.
    fn install_dev_fd(p: &'static Proc, dev: Dev, fd: i32) {
        let vp = cdevvp(dev).expect("cdevvp").expect("a vnode");
        let fp = fnew(p).expect("a file");
        fp.f_type.set(DTYPE_VNODE);
        fp.f_data.set(ptr::from_ref(vp).cast_mut().cast());
        install(p, fd, fp);
    }

    /// A component name for `name`, with the thread's credentials and no pathname buffer.
    fn cn(p: &'static Proc, name: &'static [u8], flags: u64) -> Componentname {
        let mut cnp = Componentname::new();
        cnp.cn_nameiop = LOOKUP;
        cnp.cn_flags = flags;
        cnp.cn_proc = p;
        cnp.cn_cred = p.ucred();
        cnp.cn_nameptr = name.as_ptr();
        cnp.cn_namelen = name.len() as i64;
        cnp
    }

    /// `VOP_READ` or `VOP_WRITE` of `buf` at `offset`: the bytes moved.
    fn rdwr(
        vp: &'static Vnode,
        p: &'static Proc,
        buf: &mut [u8],
        offset: Off,
        write: bool,
    ) -> usize {
        let len = buf.len();
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: len,
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: offset,
            uio_resid: len,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: if write {
                UioRw::UIO_WRITE
            } else {
                UioRw::UIO_READ
            },
            uio_procp: Some(p),
        };
        let r = if write {
            VOP_WRITE(vp, &mut uio, 0, p.ucred())
        } else {
            VOP_READ(vp, &mut uio, 0, p.ucred())
        };
        r.expect("rdwr");
        len - uio.uio_resid
    }

    /// Directory entries as `readdir` returns them: name, `d_type`, `d_fileno`.
    type Entries = Vec<(Vec<u8>, u8, u64)>;

    /// `getdents` of `vp` from `offset` into 512 bytes: the entries and the end offset and flag.
    fn readdir(vp: &'static Vnode, p: &'static Proc, offset: Off) -> (Entries, Off, i32) {
        let mut buf = [0u8; 512];
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: offset,
            uio_resid: 512,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: Some(p),
        };
        let mut eof = 0;
        VOP_READDIR(vp, &mut uio, p.ucred(), &mut eof).expect("readdir");
        let used = 512 - uio.uio_resid;
        let mut out = Vec::new();
        let mut off = 0;
        while off < used {
            let d = Dirent::from_bytes(&buf[off..]).expect("a dirent");
            let name = buf[off + Dirent::NAME_OFFSET..][..usize::from(d.d_namlen)].to_vec();
            assert_eq!(buf[off + Dirent::NAME_OFFSET + usize::from(d.d_namlen)], 0);
            out.push((name, d.d_type, d.d_fileno));
            off += usize::from(d.d_reclen);
        }
        (out, uio.uio_offset, eof)
    }

    #[test]
    fn a_mount_served_by_a_simulated_daemon() {
        let (_g, p) = setup();
        LOG.with(|l| l.borrow_mut().clear());
        FORGOTTEN.with(|f| f.borrow_mut().clear());
        CONTENT.with(|c| *c.borrow_mut() = HELLO.to_vec());
        let dev = open_dev(p, 0);
        install_dev_fd(p, dev, 3);
        TEST_DAEMON.with(|d| d.set(Some(daemon)));

        // mount(MOUNT_FUSEFS, "/mnt", 0, &fargs), as libfuse's fuse_mount does.
        let mp = vfs_mount_alloc(None, fuse_conf());
        let mut nd = ndinit(LOOKUP, 0, NiDirp::Sys(b"/mnt"), p);
        let mut short = [0u8; 8];
        assert_eq!(
            fusefs_mount(mp, b"/mnt", &mut short, &mut nd, p),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            fusefs_mount(mp, b"/mnt", &mut args(9, 0, 0), &mut nd, p),
            Err(Errno::EBADF)
        );
        fusefs_mount(mp, b"/mnt", &mut args(3, 0, 0), &mut nd, p).expect("mount");
        vfs_unbusy(mp);
        let fmp = VFSTOFUSEFS(mp);
        assert_eq!(fmp.dev, dev);
        assert_eq!(fmp.max_read, FUSEBUFMAXSIZE as i32);
        assert_eq!(fmp.sess_init.get(), PENDING);
        assert_eq!(&mp.mnt_stat.get().f_mntfromname[..7], b"fusefs\0");
        assert_eq!(&mp.mnt_stat.get().f_mntonname[..5], b"/mnt\0");

        // Before FUSE_INIT is answered statfs returns zeros (the daemon is in mount(2)).
        let mut sb = Statfs::new();
        fusefs_statfs(mp, &mut sb, p).expect("statfs");
        assert_eq!((sb.f_blocks, sb.f_bsize), (0, 0));
        assert!(LOG.with(|l| l.borrow().is_empty()));

        // The root needs no request.
        let root = VFS_ROOT(mp).expect("root");
        assert_eq!(root.v_type.get(), VDIR);
        assert!(root.v_flag.get() & VROOT != 0);

        // lookup "hello": access (GETATTR of the root, after INIT), LOOKUP, then the new node's
        // GETATTR.
        let mut vpp = None;
        let mut cnp = cn(p, b"hello", LOCKPARENT | ISLASTCN);
        VOP_LOOKUP(root, &mut vpp, &mut cnp).expect("lookup");
        let vp = vpp.expect("a vnode");
        assert_eq!(fmp.sess_init.get(), 1, "INIT answered");
        assert_eq!(fmp.max_write.get(), 65536);
        assert_eq!(vp.v_type.get(), VREG);
        let ip = VTOI(vp);
        assert_eq!(ip.i_number, 2);
        assert_eq!(ip.filesize.get(), HELLO.len() as Off);
        assert_eq!(ip.nlookup.get(), 1);
        assert_eq!(
            LOG.with(|l| l.borrow().clone()),
            [FUSE_INIT, FUSE_GETATTR, FUSE_LOOKUP, FUSE_GETATTR]
        );
        let _ = VOP_UNLOCK(vp);

        // The second lookup finds the node in the inode hash.
        let mut vpp2 = None;
        let mut cnp = cn(p, b"hello", LOCKPARENT | ISLASTCN);
        VOP_LOOKUP(root, &mut vpp2, &mut cnp).expect("lookup");
        assert!(vpp2.is_some_and(|v| ptr::eq(v, vp)));
        assert_eq!(ip.nlookup.get(), 2);
        vput(vp);

        let mut vpp3 = None;
        let mut cnp = cn(p, b"nothere", LOCKPARENT | ISLASTCN);
        assert_eq!(VOP_LOOKUP(root, &mut vpp3, &mut cnp), Err(Errno::ENOENT));
        assert!(vpp3.is_none());

        // getattr
        let mut va = Vattr::new();
        VOP_GETATTR(vp, &mut va, p.ucred(), p).expect("getattr");
        assert_eq!(va.va_type, VREG);
        assert_eq!(va.va_mode, 0o644);
        assert_eq!(va.va_size, HELLO.len() as u64);
        assert_eq!(va.va_blocksize, 512, "a zero blksize becomes S_BLKSIZE");
        assert_eq!(va.va_bytes, 512, "blocks computed from the size");
        assert_eq!(
            (va.va_mtime.tv_sec, va.va_mtime.tv_nsec),
            (1_700_000_000, 5)
        );
        assert_eq!(va.va_fileid, 2);

        // open for reading, read
        VOP_OPEN(vp, FREAD, p.ucred(), p).expect("open");
        assert_eq!(ip.fufh(FUFH_RDONLY).fh_id, 10 + O_RDONLY as u64);
        let mut buf = [0u8; 64];
        let n = rdwr(vp, p, &mut buf, 0, false);
        assert_eq!(&buf[..n], HELLO);
        let n = rdwr(vp, p, &mut buf, 7, false);
        assert_eq!(&buf[..n], b"world\n");

        // open for writing, write
        VOP_OPEN(vp, FWRITE, p.ucred(), p).expect("open");
        assert_eq!(ip.fufh(FUFH_WRONLY).fh_id, 10 + O_WRONLY as u64);
        let mut hi = *b"HELLO";
        assert_eq!(rdwr(vp, p, &mut hi, 0, true), 5);
        let mut more = *b"!!";
        assert_eq!(rdwr(vp, p, &mut more, 13, true), 2);
        assert_eq!(ip.filesize.get(), 15, "a write past the end grows the file");
        CONTENT.with(|c| assert_eq!(&c.borrow()[..], b"HELLO, world\n!!"));

        // truncate
        let mut va = Vattr::new();
        vattr_null(&mut va);
        va.va_size = 5;
        VOP_SETATTR(vp, &mut va, p.ucred(), p).expect("setattr");
        assert_eq!(ip.filesize.get(), 5);
        vattr_null(&mut va);
        va.va_flags = 1;
        assert_eq!(
            VOP_SETATTR(vp, &mut va, p.ucred(), p),
            Err(Errno::EOPNOTSUPP)
        );
        vattr_null(&mut va);
        va.va_rdev = 0;
        assert_eq!(VOP_SETATTR(vp, &mut va, p.ucred(), p), Err(Errno::EINVAL));

        // readdir in two calls: the entries, then the end.
        LOG.with(|l| l.borrow_mut().clear());
        let (entries, end, eof) = readdir(root, p, 0);
        assert_eq!(
            entries,
            [
                (b".".to_vec(), DT_DIR, 1),
                (b"..".to_vec(), DT_DIR, 1),
                (b"hello".to_vec(), DT_REG, 2)
            ]
        );
        assert_eq!((end, eof), (3, 0));
        let (entries, end, eof) = readdir(root, p, end);
        assert!(entries.is_empty());
        assert_eq!((end, eof), (3, 1));
        assert_eq!(
            LOG.with(|l| l.borrow().clone()),
            [
                FUSE_OPENDIR,
                FUSE_READDIR,
                FUSE_RELEASEDIR,
                FUSE_OPENDIR,
                FUSE_READDIR,
                FUSE_RELEASEDIR
            ]
        );

        // statfs, now that the session is up
        let mut sb = Statfs::new();
        fusefs_statfs(mp, &mut sb, p).expect("statfs");
        assert_eq!((sb.f_blocks, sb.f_bfree, sb.f_bavail), (100, 50, 40));
        assert_eq!((sb.f_files, sb.f_ffree, sb.f_favail), (10, 5, 5));
        assert_eq!((sb.f_bsize, sb.f_namemax), (512, 255));
        assert_eq!(sb.f_iosize as usize, BLKDEV_IOSIZE, "capped");

        // The sysctls.
        let mut v = 0i32;
        let mut len = size_of::<i32>();
        fusefs_sysctl(
            &[FUSEFS_OPENDEVS],
            ptr::from_mut(&mut v) as usize,
            &mut len,
            0,
            0,
            p,
        )
        .expect("sysctl");
        assert_eq!(v, 1);

        // Last reference: VOP_INACTIVE releases both handles; unmount reclaims (FORGET with the
        // two lookups) and says FUSE_DESTROY.
        LOG.with(|l| l.borrow_mut().clear());
        vrele(vp);
        vput(root);
        fusefs_unmount(mp, 0, p).expect("unmount");
        assert_eq!(
            LOG.with(|l| l.borrow().clone()),
            [FUSE_RELEASE, FUSE_RELEASE, FUSE_FORGET, FUSE_DESTROY]
        );
        assert_eq!(FORGOTTEN.with(|f| f.borrow().clone()), [(2, 2)]);
        assert!(mp.mnt_data.get().is_null());

        TEST_DAEMON.with(|d| d.set(None));
        fuseclose(dev, 0, 0, Some(p)).expect("close");
    }

    #[test]
    fn a_dead_daemon_fails_the_vnode_operations() {
        let (_g, p) = setup();
        CONTENT.with(|c| *c.borrow_mut() = HELLO.to_vec());
        let dev = open_dev(p, 0);
        install_dev_fd(p, dev, 3);
        TEST_DAEMON.with(|d| d.set(Some(daemon)));

        let mp = vfs_mount_alloc(None, fuse_conf());
        let mut nd = ndinit(LOOKUP, 0, NiDirp::Sys(b"/mnt"), p);
        fusefs_mount(mp, b"/mnt", &mut args(3, 4096, 0), &mut nd, p).expect("mount");
        vfs_unbusy(mp);
        let fmp = VFSTOFUSEFS(mp);
        assert_eq!(fmp.max_read, 4096);
        let root = VFS_ROOT(mp).expect("root");
        let mut va = Vattr::new();
        VOP_GETATTR(root, &mut va, p.ucred(), p).expect("getattr");
        assert_eq!(va.va_type, VDIR);

        // The daemon exits: its descriptor's close fails what is queued and ends the session.
        TEST_DAEMON.with(|d| d.set(None));
        fuseclose(dev, 0, 0, Some(p)).expect("close");
        assert_eq!(fmp.sess_init.get(), 0);
        assert_eq!(VOP_GETATTR(root, &mut va, p.ucred(), p), Err(Errno::ENXIO));
        let mut vpp = None;
        let mut cnp = cn(p, b"hello", LOCKPARENT | ISLASTCN);
        assert_eq!(VOP_LOOKUP(root, &mut vpp, &mut cnp), Err(Errno::ENXIO));

        vput(root);
        fusefs_unmount(mp, 0, p).expect("unmount without a daemon");
    }
}
/* </TESTS> */
