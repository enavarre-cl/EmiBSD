/* $OpenBSD: fuse_lookup.c,v 1.27 2026/07/10 14:43:48 helg Exp $ */
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

//! `fusefs_lookup` (`vop_lookup`): looks a name up in a FUSE directory by asking the daemon
//! (`FUSE_LOOKUP`); `.` and `..` are answered in the kernel.
//!
//! Upstream: sys/miscfs/fuse/fuse_lookup.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The type the daemon reported (`nvtype`) is an `Option`: the C leaves it uninitialised
//!   for `.` and `..`, and for `..` under `RENAME` writes that garbage into the parent's
//!   `v_type`; the port leaves `v_type` as `VFS_VGET` found it there.
//! - As in C, a `..` whose node turns out not to be a directory fails with `EIO` without
//!   releasing the vnode `VFS_VGET` returned.
//! - `cnp->cn_proc` is read through `cn_proc` (`fuse_vnops.rs`), detached from `cnp` so that
//!   the component's flags can change meanwhile.

use crate::kern::vfs_subr::{vput, vref};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_UNLOCK};
use crate::miscfs::fuse::fuse_device::fuse_device_queue_fbuf;
use crate::miscfs::fuse::fuse_vnops::{cn_proc, vmount};
use crate::miscfs::fuse::fusebuf::{fb_delete, fb_queue, fb_setup};
use crate::miscfs::fuse::fusefs_node::VTOI;
use crate::sys::errno::Errno;
use crate::sys::fusebuf::{FUSE_FORGET, FUSE_LOOKUP, FUSE_ROOT_ID, FuseEntryOut, FuseForgetIn};
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::mount::{MNT_RDONLY, VFS_VGET};
use crate::sys::namei::{
    CREATE, DELETE, ISDOTDOT, ISLASTCN, LOCKPARENT, PDIRUNLOCK, RENAME, SAVENAME, WANTPARENT,
};
use crate::sys::types::Ino;
use crate::sys::vnode::{VDIR, VEXEC, VWRITE, VopLookupArgs, Vtype, iftovt};

/// `fusefs_lookup` (`vop_lookup`).
pub fn fusefs_lookup(ap: &mut VopLookupArgs<'_>) -> Result<(), Errno> {
    let vdp = ap.a_dvp; // vnode for directory being searched
    let dp = VTOI(vdp); // inode for directory being searched
    let fmp = dp.i_fmp; // file system that directory is in
    let p = cn_proc(ap.a_cnp);
    let cred = ap.a_cnp.cn_cred;
    let nameiop = ap.a_cnp.cn_nameiop;

    let flags = ap.a_cnp.cn_flags;
    *ap.a_vpp = None;
    let lockparent = flags & LOCKPARENT != 0;
    let wantparent = flags & (LOCKPARENT | WANTPARENT) != 0;

    VOP_ACCESS(vdp, VEXEC, cred, p)?;

    if flags & ISLASTCN != 0
        && vmount(vdp).mnt_flag.get() & MNT_RDONLY != 0
        && (nameiop == DELETE || nameiop == RENAME)
    {
        return Err(Errno::EROFS);
    }

    // FUSE doesn't send . or .. lookups to userland so they must be handled here. The
    // parent node id is only cached for directories and will be refreshed below the next
    // time the directory is looked up by name.
    let nid: Ino;
    let mut nvtype: Option<Vtype> = None;
    if ap.a_cnp.name() == b"." {
        nid = dp.i_number;
    } else if flags & ISDOTDOT != 0 {
        nid = dp.i_parent_cache.get();
    } else {
        if fmp.sess_init.get() == 0 {
            return Err(Errno::ENOENT);
        }

        // got a real entry
        let name = ap.a_cnp.name();
        let fbuf = fb_setup(name.len() + 1, dp.i_number, FUSE_LOOKUP, p);

        // SAFETY: a fresh fusebuf, not queued yet: its data is ours.
        let dat = unsafe { fbuf.fb_dat_slice() };
        dat[..name.len()].copy_from_slice(name);
        dat[name.len()] = 0;

        if let Err(error) = fb_queue(fmp.dev, fbuf) {
            fb_delete(fbuf);

            // file system is dead
            if error == Errno::ENXIO {
                return Err(error);
            }

            if (nameiop == CREATE || nameiop == RENAME) && flags & ISLASTCN != 0 {
                // Access for write is interpreted as allowing creation of files in the
                // directory.
                VOP_ACCESS(vdp, VWRITE, cred, p)?;

                ap.a_cnp.cn_flags |= SAVENAME;

                if !lockparent {
                    let _ = VOP_UNLOCK(vdp);
                    ap.a_cnp.cn_flags |= PDIRUNLOCK;
                }

                return Err(Errno::EJUSTRETURN);
            }

            return Err(Errno::ENOENT);
        }

        let entry: FuseEntryOut = fbuf.op_get();
        nid = entry.nodeid;
        nvtype = Some(iftovt(entry.attr.mode));
        fb_delete(fbuf);

        // An error of ENOENT or an inode value of 0 mean the entry was not found. The
        // difference is that 0 indicates that the result may be cached. We don't support
        // caching yet so just return.
        if nid == 0 {
            return Err(Errno::ENOENT);
        }
    }

    let error = 'reclaim: {
        if nameiop == DELETE && flags & ISLASTCN != 0 {
            // Write access to directory required to delete files.
            if let Err(e) = VOP_ACCESS(vdp, VWRITE, cred, p) {
                break 'reclaim e;
            }

            ap.a_cnp.cn_flags |= SAVENAME;
        }

        if nameiop == RENAME && wantparent && flags & ISLASTCN != 0 {
            // Write access to directory required to delete files.
            if let Err(e) = VOP_ACCESS(vdp, VWRITE, cred, p) {
                break 'reclaim e;
            }

            if nid == dp.i_number {
                return Err(Errno::EISDIR);
            }

            let tdp = match VFS_VGET(fmp.mp, nid) {
                Ok(tdp) => tdp,
                Err(e) => break 'reclaim e,
            };

            if let Some(t) = nvtype {
                tdp.v_type.set(t);
            }
            VTOI(tdp).i_parent_cache.set(dp.i_number);
            *ap.a_vpp = Some(tdp);
            ap.a_cnp.cn_flags |= SAVENAME;

            return Ok(());
        }

        if flags & ISDOTDOT != 0 {
            let _ = VOP_UNLOCK(vdp); // race to get the inode
            ap.a_cnp.cn_flags |= PDIRUNLOCK;

            let tdp = match VFS_VGET(fmp.mp, nid) {
                Ok(tdp) if tdp.v_type.get() != VDIR => {
                    // DPRINTF("%s: parent not dir: %s\n", __func__, cnp->cn_nameptr);
                    Err(Errno::EIO)
                }
                r => r,
            };
            let tdp = match tdp {
                Ok(tdp) => tdp,
                Err(e) => {
                    if vn_lock(vdp, LK_EXCLUSIVE | LK_RETRY).is_ok() {
                        ap.a_cnp.cn_flags &= !PDIRUNLOCK;
                    }

                    break 'reclaim e;
                }
            };

            if lockparent && flags & ISLASTCN != 0 {
                if let Err(e) = vn_lock(vdp, LK_EXCLUSIVE) {
                    vput(tdp);
                    return Err(e);
                }
                ap.a_cnp.cn_flags &= !PDIRUNLOCK;
            }
            *ap.a_vpp = Some(tdp);

            // Didn't actually make a call but vget increments lookup
            let tip = VTOI(tdp);
            tip.nlookup.set(tip.nlookup.get().wrapping_sub(1));
        } else if nid == dp.i_number {
            vref(vdp);
            *ap.a_vpp = Some(vdp);
        } else {
            let tdp = match VFS_VGET(fmp.mp, nid) {
                Ok(tdp) => tdp,
                Err(e) => break 'reclaim e,
            };

            if let Some(t) = nvtype {
                tdp.v_type.set(t);
            }
            VTOI(tdp).i_parent_cache.set(dp.i_number);

            // Cache the parent if it's a directory so that we can resolve any .. lookups
            // later.
            if tdp.v_type.get() == VDIR {
                VTOI(tdp).i_parent_cache.set(dp.i_number);
            }

            if !lockparent || flags & ISLASTCN == 0 {
                let _ = VOP_UNLOCK(vdp);
                ap.a_cnp.cn_flags |= PDIRUNLOCK;
            }

            *ap.a_vpp = Some(tdp);
        }

        return Ok(());
    };

    // reclaim:
    if nid != dp.i_number && nid != FUSE_ROOT_ID {
        let fbuf = fb_setup(0, nid, FUSE_FORGET, p);
        fbuf.op_set(&FuseForgetIn { nlookup: 1 });
        fuse_device_queue_fbuf(fmp.dev, fbuf); // no response
    }
    Err(error)
}
