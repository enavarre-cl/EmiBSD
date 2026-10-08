/*	$OpenBSD: vfs_vops.c,v 1.39 2026/06/10 00:04:38 beck Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2010 Thordur I. Bjornsson <thib@openbsd.org>
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
 *
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! The `VOP_*` wrappers: each packs its arguments into the `struct vop_*_args` of
//! `<sys/vnode.h>` and calls the vnode's operation, or answers `EOPNOTSUPP` when the file
//! system left the slot NULL.
//!
//! Upstream: sys/kern/vfs_vops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The wrappers keep the C's uppercase names (`#[allow(non_snake_case)]`); out-parameters
//!   are `&mut Option<&'static Vnode>` as in the argument structures.
//! - `VOP_REMOVE` calls the slot without the NULL check, as in C; a NULL slot answers
//!   `EOPNOTSUPP` (and the vnodes are still released) where the C would call through NULL.
//! - `ASSERT_VP_ISLOCKED` is compiled only with `VFSLCKDEBUG`, which is not configured; the
//!   `KASSERT(p == curproc)`s are `kassert!`s. `VOP_PRINT` exists under feature
//!   `diagnostic` or `debug` (the C's `DEBUG || DIAGNOSTIC`).
//! - `VOP_BWRITE` panics for a buffer without a vnode, where the C would dereference NULL.

use core::ffi::c_void;
use core::ptr;

use crate::kassert;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_subr::{VNODE_MTX, vput, vrele};
use crate::machine::cpu::curproc;
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::Buf;
use crate::sys::errno::Errno;
use crate::sys::event::Knote;
use crate::sys::fcntl::Flock;
use crate::sys::mutex::mutex_assert_unlocked;
use crate::sys::namei::Componentname;
use crate::sys::proc::Proc;
use crate::sys::syslimits::{PATH_MAX, PIPE_BUF};
use crate::sys::types::{Daddr, Register};
use crate::sys::ucred::Ucred;
use crate::sys::uio::Uio;
use crate::sys::unistd::{_PC_ASYNC_IO, _PC_PATH_MAX, _PC_PIPE_BUF, _PC_PRIO_IO, _PC_SYNC_IO};
use crate::sys::vnode::{
    VBIOERROR, VDIR, Vattr, Vnode, VopAbortopArgs, VopAccessArgs, VopAdvlockArgs, VopBmapArgs,
    VopBwriteArgs, VopCloseArgs, VopCreateArgs, VopFsyncArgs, VopGetattrArgs, VopInactiveArgs,
    VopIoctlArgs, VopIslockedArgs, VopKqfilterArgs, VopLinkArgs, VopLockArgs, VopLookupArgs,
    VopMkdirArgs, VopMknodArgs, VopOpenArgs, VopPathconfArgs, VopReadArgs, VopReaddirArgs,
    VopReadlinkArgs, VopReclaimArgs, VopRemoveArgs, VopRenameArgs, VopRevokeArgs, VopRmdirArgs,
    VopSetattrArgs, VopStrategyArgs, VopSymlinkArgs, VopUnlockArgs, VopWriteArgs,
};

/// `KASSERT(p == curproc)`.
fn assert_curproc(p: &Proc) {
    kassert!(curproc().is_some_and(|cur| ptr::eq(cur, p)));
    let _ = p;
}

/// `VOP_ISLOCKED(vp)`: the lock status, or `EOPNOTSUPP` as an `int`, as in C.
#[allow(non_snake_case)] // the C name
pub fn VOP_ISLOCKED(vp: &'static Vnode) -> i32 {
    let mut a = VopIslockedArgs { a_vp: vp };

    match vp.op().vop_islocked {
        Some(f) => f(&mut a),
        None => Errno::EOPNOTSUPP as i32,
    }
}

/// `VOP_LOOKUP(dvp, vpp, cnp)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_LOOKUP(
    dvp: &'static Vnode,
    vpp: &mut Option<&'static Vnode>,
    cnp: &mut Componentname,
) -> Result<(), Errno> {
    let mut a = VopLookupArgs {
        a_dvp: dvp,
        a_vpp: vpp,
        a_cnp: cnp,
    };

    match dvp.op().vop_lookup {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_CREATE(dvp, vpp, cnp, vap)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_CREATE(
    dvp: &'static Vnode,
    vpp: &mut Option<&'static Vnode>,
    cnp: &mut Componentname,
    vap: &mut Vattr,
) -> Result<(), Errno> {
    let mut a = VopCreateArgs {
        a_dvp: dvp,
        a_vpp: vpp,
        a_cnp: cnp,
        a_vap: vap,
    };

    // ASSERT_VP_ISLOCKED(dvp): VFSLCKDEBUG.

    match dvp.op().vop_create {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_MKNOD(dvp, vpp, cnp, vap)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_MKNOD(
    dvp: &'static Vnode,
    vpp: &mut Option<&'static Vnode>,
    cnp: &mut Componentname,
    vap: &mut Vattr,
) -> Result<(), Errno> {
    let mut a = VopMknodArgs {
        a_dvp: dvp,
        a_vpp: vpp,
        a_cnp: cnp,
        a_vap: vap,
    };

    // ASSERT_VP_ISLOCKED(dvp): VFSLCKDEBUG.

    match dvp.op().vop_mknod {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_OPEN(vp, mode, cred, p)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_OPEN(vp: &'static Vnode, mode: i32, cred: *const Ucred, p: &Proc) -> Result<(), Errno> {
    let mut a = VopOpenArgs {
        a_vp: vp,
        a_mode: mode,
        a_cred: cred,
        a_p: p,
    };

    assert_curproc(p);

    match vp.op().vop_open {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_CLOSE(vp, fflag, cred, p)`: `p` is `None` when no thread closes.
#[allow(non_snake_case)] // the C name
pub fn VOP_CLOSE(
    vp: &'static Vnode,
    fflag: i32,
    cred: *const Ucred,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    let mut a = VopCloseArgs {
        a_vp: vp,
        a_fflag: fflag,
        a_cred: cred,
        a_p: p,
    };

    if let Some(p) = p {
        assert_curproc(p);
    }
    // ASSERT_VP_ISLOCKED(vp): VFSLCKDEBUG.

    match vp.op().vop_close {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_ACCESS(vp, mode, cred, p)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_ACCESS(
    vp: &'static Vnode,
    mode: i32,
    cred: *const Ucred,
    p: &Proc,
) -> Result<(), Errno> {
    let mut a = VopAccessArgs {
        a_vp: vp,
        a_mode: mode,
        a_cred: cred,
        a_p: p,
    };

    assert_curproc(p);
    // ASSERT_VP_ISLOCKED(vp): VFSLCKDEBUG.

    match vp.op().vop_access {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_GETATTR(vp, vap, cred, p)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_GETATTR(
    vp: &'static Vnode,
    vap: &mut Vattr,
    cred: *const Ucred,
    p: &Proc,
) -> Result<(), Errno> {
    let mut a = VopGetattrArgs {
        a_vp: vp,
        a_vap: vap,
        a_cred: cred,
        a_p: p,
    };

    assert_curproc(p);
    match vp.op().vop_getattr {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_SETATTR(vp, vap, cred, p)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_SETATTR(
    vp: &'static Vnode,
    vap: &mut Vattr,
    cred: *const Ucred,
    p: &Proc,
) -> Result<(), Errno> {
    let mut a = VopSetattrArgs {
        a_vp: vp,
        a_vap: vap,
        a_cred: cred,
        a_p: p,
    };

    assert_curproc(p);
    // ASSERT_VP_ISLOCKED(vp): VFSLCKDEBUG.

    match vp.op().vop_setattr {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_READ(vp, uio, ioflag, cred)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_READ(
    vp: &'static Vnode,
    uio: &mut Uio<'_>,
    ioflag: i32,
    cred: *const Ucred,
) -> Result<(), Errno> {
    let mut a = VopReadArgs {
        a_vp: vp,
        a_uio: uio,
        a_ioflag: ioflag,
        a_cred: cred,
    };

    // ASSERT_VP_ISLOCKED(vp): VFSLCKDEBUG.

    match vp.op().vop_read {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_WRITE(vp, uio, ioflag, cred)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_WRITE(
    vp: &'static Vnode,
    uio: &mut Uio<'_>,
    ioflag: i32,
    cred: *const Ucred,
) -> Result<(), Errno> {
    let mut a = VopWriteArgs {
        a_vp: vp,
        a_uio: uio,
        a_ioflag: ioflag,
        a_cred: cred,
    };

    // ASSERT_VP_ISLOCKED(vp): VFSLCKDEBUG.

    match vp.op().vop_write {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_IOCTL(vp, command, data, fflag, cred, p)`: `data` is the kernel copy of the
/// argument.
#[allow(non_snake_case)] // the C name
pub fn VOP_IOCTL(
    vp: &'static Vnode,
    command: u64,
    data: &mut [u8],
    fflag: i32,
    cred: *const Ucred,
    p: &Proc,
) -> Result<(), Errno> {
    let mut a = VopIoctlArgs {
        a_vp: vp,
        a_command: command,
        a_data: data,
        a_fflag: fflag,
        a_cred: cred,
        a_p: p,
    };

    assert_curproc(p);
    match vp.op().vop_ioctl {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_KQFILTER(vp, fflag, kn)`: attaches the knote to the vnode.
#[allow(non_snake_case)] // the C name
pub fn VOP_KQFILTER(vp: &'static Vnode, fflag: i32, kn: &Knote) -> Result<(), Errno> {
    let mut a = VopKqfilterArgs {
        a_vp: vp,
        a_fflag: fflag,
        a_kn: kn,
    };

    match vp.op().vop_kqfilter {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_REVOKE(vp, flags)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_REVOKE(vp: &'static Vnode, flags: i32) -> Result<(), Errno> {
    let mut a = VopRevokeArgs {
        a_vp: vp,
        a_flags: flags,
    };

    match vp.op().vop_revoke {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_FSYNC(vp, cred, waitfor, p)`: a write that failed earlier (`VBIOERROR`) turns a
/// successful sync into `EIO`.
#[allow(non_snake_case)] // the C name
pub fn VOP_FSYNC(
    vp: &'static Vnode,
    cred: *const Ucred,
    waitfor: i32,
    p: &Proc,
) -> Result<(), Errno> {
    let mut a = VopFsyncArgs {
        a_vp: vp,
        a_cred: cred,
        a_waitfor: waitfor,
        a_p: p,
    };

    assert_curproc(p);
    // ASSERT_VP_ISLOCKED(vp): VFSLCKDEBUG.

    let Some(f) = vp.op().vop_fsync else {
        return Err(Errno::EOPNOTSUPP);
    };

    let mut r = f(&mut a);
    let s = splbio();
    if r.is_ok() && vp.v_bioflag.get() & VBIOERROR != 0 {
        r = Err(Errno::EIO);
    }
    splx(s);
    r
}

/// `VOP_REMOVE(dvp, vp, cnp)`: removes the entry and releases both vnodes.
#[allow(non_snake_case)] // the C name
pub fn VOP_REMOVE(
    dvp: &'static Vnode,
    vp: &'static Vnode,
    cnp: &mut Componentname,
) -> Result<(), Errno> {
    let mut a = VopRemoveArgs {
        a_dvp: dvp,
        a_vp: vp,
        a_cnp: cnp,
    };

    // ASSERT_VP_ISLOCKED(dvp), ASSERT_VP_ISLOCKED(vp): VFSLCKDEBUG.

    let error = match dvp.op().vop_remove {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    };

    if ptr::eq(dvp, vp) {
        vrele(vp);
    } else {
        vput(vp);
    }
    vput(dvp);

    error
}

/// `VOP_LINK(dvp, vp, cnp)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_LINK(
    dvp: &'static Vnode,
    vp: &'static Vnode,
    cnp: &mut Componentname,
) -> Result<(), Errno> {
    let mut a = VopLinkArgs {
        a_dvp: dvp,
        a_vp: vp,
        a_cnp: cnp,
    };

    // ASSERT_VP_ISLOCKED(dvp): VFSLCKDEBUG.

    match dvp.op().vop_link {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_RENAME(fdvp, fvp, fcnp, tdvp, tvp, tcnp)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_RENAME(
    fdvp: &'static Vnode,
    fvp: &'static Vnode,
    fcnp: &mut Componentname,
    tdvp: &'static Vnode,
    tvp: Option<&'static Vnode>,
    tcnp: &mut Componentname,
) -> Result<(), Errno> {
    let mut a = VopRenameArgs {
        a_fdvp: fdvp,
        a_fvp: fvp,
        a_fcnp: fcnp,
        a_tdvp: tdvp,
        a_tvp: tvp,
        a_tcnp: tcnp,
    };

    // ASSERT_VP_ISLOCKED(tdvp): VFSLCKDEBUG.

    match fdvp.op().vop_rename {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_MKDIR(dvp, vpp, cnp, vap)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_MKDIR(
    dvp: &'static Vnode,
    vpp: &mut Option<&'static Vnode>,
    cnp: &mut Componentname,
    vap: &mut Vattr,
) -> Result<(), Errno> {
    let mut a = VopMkdirArgs {
        a_dvp: dvp,
        a_vpp: vpp,
        a_cnp: cnp,
        a_vap: vap,
    };

    // ASSERT_VP_ISLOCKED(dvp): VFSLCKDEBUG.

    match dvp.op().vop_mkdir {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_RMDIR(dvp, vp, cnp)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_RMDIR(
    dvp: &'static Vnode,
    vp: &'static Vnode,
    cnp: &mut Componentname,
) -> Result<(), Errno> {
    let mut a = VopRmdirArgs {
        a_dvp: dvp,
        a_vp: vp,
        a_cnp: cnp,
    };

    // ASSERT_VP_ISLOCKED(dvp), ASSERT_VP_ISLOCKED(vp): VFSLCKDEBUG.

    kassert!(!ptr::eq(dvp, vp));

    match dvp.op().vop_rmdir {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_SYMLINK(dvp, vpp, cnp, vap, target)`: `target` is the link's contents.
#[allow(non_snake_case)] // the C name
pub fn VOP_SYMLINK(
    dvp: &'static Vnode,
    vpp: &mut Option<&'static Vnode>,
    cnp: &mut Componentname,
    vap: &mut Vattr,
    target: &[u8],
) -> Result<(), Errno> {
    let mut a = VopSymlinkArgs {
        a_dvp: dvp,
        a_vpp: vpp,
        a_cnp: cnp,
        a_vap: vap,
        a_target: target,
    };

    // ASSERT_VP_ISLOCKED(dvp): VFSLCKDEBUG.

    match dvp.op().vop_symlink {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_READDIR(vp, uio, cred, eofflag)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_READDIR(
    vp: &'static Vnode,
    uio: &mut Uio<'_>,
    cred: *const Ucred,
    eofflag: &mut i32,
) -> Result<(), Errno> {
    let mut a = VopReaddirArgs {
        a_vp: vp,
        a_uio: uio,
        a_cred: cred,
        a_eofflag: eofflag,
    };

    // ASSERT_VP_ISLOCKED(vp): VFSLCKDEBUG.

    if vp.v_type.get() != VDIR {
        return Err(Errno::ENOTDIR);
    }

    match vp.op().vop_readdir {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_READLINK(vp, uio, cred)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_READLINK(
    vp: &'static Vnode,
    uio: &mut Uio<'_>,
    cred: *const Ucred,
) -> Result<(), Errno> {
    let mut a = VopReadlinkArgs {
        a_vp: vp,
        a_uio: uio,
        a_cred: cred,
    };

    // ASSERT_VP_ISLOCKED(vp): VFSLCKDEBUG.

    match vp.op().vop_readlink {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_ABORTOP(dvp, cnp)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_ABORTOP(dvp: &'static Vnode, cnp: &mut Componentname) -> Result<(), Errno> {
    let mut a = VopAbortopArgs {
        a_dvp: dvp,
        a_cnp: cnp,
    };

    match dvp.op().vop_abortop {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_INACTIVE(vp, p)`: `p` is `curproc`.
#[allow(non_snake_case)] // the C name
pub fn VOP_INACTIVE(vp: &'static Vnode, p: Option<&Proc>) -> Result<(), Errno> {
    let mut a = VopInactiveArgs { a_vp: vp, a_p: p };

    kassert!(match (p, curproc()) {
        (Some(p), Some(cur)) => ptr::eq(p, cur),
        (None, None) => true,
        _ => false,
    });
    // ASSERT_VP_ISLOCKED(vp): VFSLCKDEBUG.

    match vp.op().vop_inactive {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_RECLAIM(vp, p)`: `p` is `curproc`.
#[allow(non_snake_case)] // the C name
pub fn VOP_RECLAIM(vp: &'static Vnode, p: Option<&Proc>) -> Result<(), Errno> {
    let mut a = VopReclaimArgs { a_vp: vp, a_p: p };

    kassert!(match (p, curproc()) {
        (Some(p), Some(cur)) => ptr::eq(p, cur),
        (None, None) => true,
        _ => false,
    });
    match vp.op().vop_reclaim {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_LOCK(vp, flags)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_LOCK(vp: &'static Vnode, flags: i32) -> Result<(), Errno> {
    let mut a = VopLockArgs {
        a_vp: vp,
        a_flags: flags,
    };

    mutex_assert_unlocked(&VNODE_MTX, "VOP_LOCK");

    match vp.op().vop_lock {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_UNLOCK(vp)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_UNLOCK(vp: &'static Vnode) -> Result<(), Errno> {
    let mut a = VopUnlockArgs { a_vp: vp };

    match vp.op().vop_unlock {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_BMAP(vp, bn, vpp, bnp, runp)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_BMAP(
    vp: &'static Vnode,
    bn: Daddr,
    vpp: Option<&mut Option<&'static Vnode>>,
    bnp: Option<&mut Daddr>,
    runp: Option<&mut i32>,
) -> Result<(), Errno> {
    let mut a = VopBmapArgs {
        a_vp: vp,
        a_bn: bn,
        a_vpp: vpp,
        a_bnp: bnp,
        a_runp: runp,
    };

    // ASSERT_VP_ISLOCKED(vp): VFSLCKDEBUG.

    match vp.op().vop_bmap {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_PRINT(vp)`.
#[cfg(any(feature = "debug", feature = "diagnostic"))]
#[allow(non_snake_case)] // the C name
pub fn VOP_PRINT(vp: &'static Vnode) -> Result<(), Errno> {
    let mut a = crate::sys::vnode::VopPrintArgs { a_vp: vp };

    match vp.op().vop_print {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_PATHCONF(vp, name, retval)`: the names that are constant across filesystems are
/// answered here.
#[allow(non_snake_case)] // the C name
pub fn VOP_PATHCONF(vp: &'static Vnode, name: i32, retval: &mut Register) -> Result<(), Errno> {
    // Handle names that are constant across filesystem
    match name {
        _PC_PATH_MAX => {
            *retval = PATH_MAX as Register;
            return Ok(());
        }
        _PC_PIPE_BUF => {
            *retval = PIPE_BUF as Register;
            return Ok(());
        }
        _PC_ASYNC_IO | _PC_PRIO_IO | _PC_SYNC_IO => {
            *retval = 0;
            return Ok(());
        }
        _ => {}
    }

    let mut a = VopPathconfArgs {
        a_vp: vp,
        a_name: name,
        a_retval: retval,
    };

    // ASSERT_VP_ISLOCKED(vp): VFSLCKDEBUG.

    match vp.op().vop_pathconf {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_ADVLOCK(vp, id, op, fl, flags)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_ADVLOCK(
    vp: &'static Vnode,
    id: *const c_void,
    op: i32,
    fl: &mut Flock,
    flags: i32,
) -> Result<(), Errno> {
    let mut a = VopAdvlockArgs {
        a_vp: vp,
        a_id: id,
        a_op: op,
        a_fl: fl,
        a_flags: flags,
    };

    match vp.op().vop_advlock {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_STRATEGY(vp, bp)`.
#[allow(non_snake_case)] // the C name
pub fn VOP_STRATEGY(vp: &'static Vnode, bp: &'static Buf) -> Result<(), Errno> {
    let mut a = VopStrategyArgs { a_vp: vp, a_bp: bp };

    match vp.op().vop_strategy {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}

/// `VOP_BWRITE(bp)`: dispatches on the buffer's vnode.
#[allow(non_snake_case)] // the C name
pub fn VOP_BWRITE(bp: &'static Buf) -> Result<(), Errno> {
    let mut a = VopBwriteArgs { a_bp: bp };

    let Some(vp) = bp.b_vp.get() else {
        panic(format_args!("VOP_BWRITE: buffer {:p} without a vnode", bp));
    };
    match vp.op().vop_bwrite {
        Some(f) => f(&mut a),
        None => Err(Errno::EOPNOTSUPP),
    }
}
/* </CODE> */
