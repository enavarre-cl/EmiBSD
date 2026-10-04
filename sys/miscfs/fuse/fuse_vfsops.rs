/* $OpenBSD: fuse_vfsops.c,v 1.53 2026/07/10 14:43:48 helg Exp $ */
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

#[cfg(test)]
mod tests;
