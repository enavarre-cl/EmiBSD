/*	$OpenBSD: vfs_lookup.c,v 1.95 2026/09/17 18:51:39 deraadt Exp $	*/
/*	$NetBSD: vfs_lookup.c,v 1.17 1996/02/09 19:00:59 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
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
 *
 *	@(#)vfs_lookup.c	8.6 (Berkeley) 11/21/94
 */
/* </LICENSES> */

//! Pathname translation: `namei` (copy the path in, pick the starting directory, call
//! `vfs_lookup`, expand symbolic links), `vfs_lookup` (walk the components, crossing mount
//! points and `..`), `vfs_relookup`, the realpath helpers `component_push`/`component_pop`
//! and `ndinitat`.
//!
//! Upstream: sys/kern/vfs_lookup.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `NDINITAT`/`NDINIT` are [`ndinitat`]/[`ndinit`], returning the `nameidata`; the path's
//!   segment is the [`NiDirp`] variant.
//! - `vfs_lookup` walks the pathname buffer by index (`cn_nameptr` and `ni_next` are still
//!   set as pointers into it for the file systems, as in C); its `goto bad`/`goto bad2`
//!   exits are the two error variants of an inner function.
//! - No root file system yet: when there is no `rootvnode` (or no current directory for a
//!   relative path) `namei` fails with `ENOENT`, where the C cannot get there (`main`
//!   panics first when it cannot mount root).
//! - `namei_pool` cannot sleep yet (`subr_pool.rs`): when it is empty `namei` fails with
//!   `ENOMEM` instead of waiting in `pool_get(PR_WAITOK)`.
//! - `pledge_namei` and `checkzoneinfopath` are `kern_pledge.rs`'s and take the pathname
//!   without its NUL; the `unveil_*` hooks are `kern_unveil.rs`'s. `KTRACE` is not
//!   configured; `NAMEI_DIAGNOSTIC` neither.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;

use crate::kern::kern_descrip::fd_getfile;
use crate::kern::kern_pledge::{checkzoneinfopath, pledge_namei};
use crate::kern::kern_unveil::{unveil_check_component, unveil_check_final, unveil_start_relative};
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::vfs_init::{NAMEI_POOL, rootvnode};
use crate::kern::vfs_subr::{vfs_busy, vfs_unbusy, vput, vref, vrele};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_LOOKUP, VOP_READLINK, VOP_UNLOCK};
use crate::machine::copy::copyinstr;
use crate::sys::errno::Errno;
use crate::sys::file::{DTYPE_VNODE, frele};
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::mount::{MNT_RDONLY, VB_READ, VB_WAIT, VFS_ROOT};
use crate::sys::namei::{
    AT_FDCWD, BPU_LOCALTIME, BPU_ZONEINFO, BYPASSUNVEIL, CREATE, Componentname, DELETE, EXECPATH,
    FOLLOW, HASBUF, ISDOTDOT, ISLASTCN, ISSYMLINK, KERNELPATH, LOCKLEAF, LOCKPARENT, MAKEENTRY,
    NOCACHE, NOCROSSMOUNT, Nameidata, NiDirp, PDIRUNLOCK, RDONLY, REALPATH, RENAME, REQUIREDIR,
    SAVENAME, SAVESTART, STRIPSLASHES, WANTPARENT,
};
use crate::sys::param::MAXPATHLEN;
use crate::sys::pledge::PLEDGE_UNVEIL;
use crate::sys::pool::PR_WAITOK;
use crate::sys::proc::Proc;
use crate::sys::syslimits::{NAME_MAX, SYMLOOP_MAX};
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::{VDIR, VLNK, VROOT, Vnode};

/// `component_push(cnp, component, len)`: appends `/component` to the realpath buffer; false
/// when it would not fit.
pub fn component_push(cnp: &mut Componentname, component: &[u8]) -> bool {
    let len = component.len();
    if cnp.cn_rpi + len + 1 >= MAXPATHLEN {
        return false;
    }
    let rpbuf = rpbuf(cnp);
    if cnp.cn_rpi > 1 {
        rpbuf[cnp.cn_rpi] = b'/';
        cnp.cn_rpi += 1;
    }
    rpbuf[cnp.cn_rpi..cnp.cn_rpi + len].copy_from_slice(component);
    cnp.cn_rpi += len;
    rpbuf[cnp.cn_rpi] = 0;
    true
}

/// `component_pop(cnp)`: drops the last component of the realpath buffer.
pub fn component_pop(cnp: &mut Componentname) {
    let rpbuf = rpbuf(cnp);
    while cnp.cn_rpi != 0 && rpbuf[cnp.cn_rpi] != b'/' {
        cnp.cn_rpi -= 1;
    }
    if cnp.cn_rpi == 0 && rpbuf[0] == b'/' {
        cnp.cn_rpi += 1;
    }
    rpbuf[cnp.cn_rpi] = 0;
}

/// The realpath buffer of a `REALPATH`/`EXECPATH` lookup.
fn rpbuf<'b>(cnp: &Componentname) -> &'b mut [u8] {
    if cnp.cn_rpbuf.is_null() {
        crate::kern::subr_prf::panic(format_args!("namei: REALPATH without cn_rpbuf"));
    }
    // SAFETY: a `REALPATH`/`EXECPATH` caller points `cn_rpbuf` at `MAXPATHLEN` bytes it owns
    // for the lookup's duration (`sys___realpath`, exec), and nothing else uses them meanwhile.
    unsafe { slice::from_raw_parts_mut(cnp.cn_rpbuf, MAXPATHLEN) }
}

/// The pathname buffer `namei` filled, read-only (what `vfs_lookup` walks while the file
/// systems read the component through `cn_nameptr`).
fn pnbuf_ref<'b>(cnp: &Componentname) -> &'b [u8] {
    if cnp.cn_pnbuf.is_null() {
        crate::kern::subr_prf::panic(format_args!("namei: no cn_pnbuf"));
    }
    // SAFETY: as for `pnbuf`; nobody writes the buffer while the lookup walks it.
    unsafe { slice::from_raw_parts(cnp.cn_pnbuf, MAXPATHLEN) }
}

/// The pathname buffer `namei` filled.
fn pnbuf<'b>(cnp: &Componentname) -> &'b mut [u8] {
    if cnp.cn_pnbuf.is_null() {
        crate::kern::subr_prf::panic(format_args!("namei: no cn_pnbuf"));
    }
    // SAFETY: `cn_pnbuf` is a `namei_pool` item (`MAXPATHLEN` bytes) the lookup owns until
    // it gives it back; the file systems only read it.
    unsafe { slice::from_raw_parts_mut(cnp.cn_pnbuf, MAXPATHLEN) }
}

/// `ndinitat(ndp, op, flags, segflg, dirfd, namep, p)`: a `nameidata` for a lookup relative
/// to `dirfd`.
pub fn ndinitat<'a>(op: u64, flags: u64, dirfd: i32, namep: NiDirp<'a>, p: &Proc) -> Nameidata<'a> {
    let mut cnd = Componentname::new();
    cnd.cn_nameiop = op;
    cnd.cn_flags = flags;
    cnd.cn_proc = p;
    Nameidata {
        ni_dirp: namep,
        ni_dirfd: dirfd,
        ni_segflg: match namep {
            NiDirp::User(_) => UioSeg::UIO_USERSPACE,
            NiDirp::Sys(_) => UioSeg::UIO_SYSSPACE,
        },
        ni_startdir: None,
        ni_rootdir: None,
        ni_pledge: 0,
        ni_unveil: 0,
        ni_vp: None,
        ni_dvp: None,
        ni_pathlen: 0,
        ni_next: ptr::null(),
        ni_loopcnt: 0,
        ni_unveil_match: ptr::null(),
        ni_cnd: cnd,
    }
}

/// `NDINIT(ndp, op, flags, segflg, namep, p)`: `ndinitat` relative to the current directory.
pub fn ndinit<'a>(op: u64, flags: u64, namep: NiDirp<'a>, p: &Proc) -> Nameidata<'a> {
    ndinitat(op, flags, AT_FDCWD, namep, p)
}

/// The pathname in `buf` up to its NUL.
fn pn_str(buf: &[u8]) -> &[u8] {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    &buf[..len]
}

/// Gives the pathname buffer back and fails the lookup (`fail:` in C).
fn namei_fail(ndp: &mut Nameidata<'_>, error: Errno) -> Result<(), Errno> {
    if let Some(buf) = NonNull::new(ndp.ni_cnd.cn_pnbuf) {
        pool_put(&NAMEI_POOL, buf);
    }
    ndp.ni_cnd.cn_pnbuf = ptr::null_mut();
    ndp.ni_vp = None;
    Err(error)
}

/// Convert a pathname into a pointer to a vnode.
///
/// The `FOLLOW` flag is set when symbolic links are to be followed when they occur at the end
/// of the name translation process. Symbolic links are always followed for all other
/// pathname components other than the last.
///
/// If the `LOCKLEAF` flag is set, a locked vnode is returned.
///
/// The segflg defines whether the name is to be copied from user space or kernel space.
///
/// Overall outline of namei:
///
/// ```text
///     copy in name
///     get starting directory
///     while (!done && !error) {
///             call lookup to search path.
///             if symbolic link, massage name in buffer and continue
///     }
/// ```
pub fn namei(ndp: &mut Nameidata<'_>) -> Result<(), Errno> {
    let p = ndp.ni_cnd.proc();
    // SAFETY: the thread outlives its own lookup; detached from `ndp` so `ndp` can be
    // borrowed mutably below.
    let p: &Proc = unsafe { &*ptr::from_ref(p) };

    ndp.ni_cnd.cn_cred = p.p_ucred.get();
    #[cfg(feature = "diagnostic")]
    {
        if ndp.ni_cnd.cn_cred.is_null() {
            crate::kern::subr_prf::panic(format_args!("namei: bad cred/proc"));
        }
        if ndp.ni_cnd.cn_nameiop & !crate::sys::namei::OPMASK != 0 {
            crate::kern::subr_prf::panic(format_args!("namei: nameiop contaminated with flags"));
        }
        if ndp.ni_cnd.cn_flags & crate::sys::namei::OPMASK != 0 {
            crate::kern::subr_prf::panic(format_args!("namei: flags contaminated with nameiops"));
        }
    }
    let fdp = p.fd();

    // Get a buffer for the name to be translated, and copy the name into the buffer.
    if ndp.ni_cnd.cn_flags & HASBUF == 0 {
        let Some(buf) = pool_get(&NAMEI_POOL, PR_WAITOK) else {
            // PR_WAITOK cannot sleep yet (see the module's deviations).
            ndp.ni_vp = None;
            return Err(Errno::ENOMEM);
        };
        ndp.ni_cnd.cn_pnbuf = buf.as_ptr();
    }
    let pn = pnbuf(&ndp.ni_cnd);
    let copied = match ndp.ni_dirp {
        NiDirp::Sys(path) => {
            let path = path.split(|&c| c == 0).next().unwrap_or(&[]);
            if path.len() >= MAXPATHLEN {
                Err(Errno::ENAMETOOLONG)
            } else {
                pn[..path.len()].copy_from_slice(path);
                pn[path.len()] = 0;
                Ok(path.len() + 1) // ni_pathlen includes NUL
            }
        }
        NiDirp::User(uaddr) => copyinstr(uaddr, pn),
    };
    ndp.ni_pathlen = match copied {
        // Fail on null pathnames
        Ok(1) => return namei_fail(ndp, Errno::ENOENT),
        Ok(len) => len,
        Err(e) => return namei_fail(ndp, e),
    };

    // KTRACE: not configured.

    // Strip trailing slashes, as requested
    if ndp.ni_cnd.cn_flags & STRIPSLASHES != 0 {
        let end = ndp.ni_pathlen as isize - 2;
        let mut cp = end;
        while cp >= 0 && pn[cp as usize] == b'/' {
            cp -= 1;
        }

        // Still some remaining characters in the buffer
        if cp >= 0 {
            ndp.ni_pathlen -= (end - cp) as usize;
            pn[(cp + 1) as usize] = 0;
        }
    }

    ndp.ni_loopcnt = 0;

    // Get starting point for the translation.
    ndp.ni_rootdir = fdp.fd_rdir.get();
    if ndp.ni_rootdir.is_none() || ndp.ni_cnd.cn_flags & KERNELPATH != 0 {
        ndp.ni_rootdir = rootvnode();
    }
    let Some(rootdir) = ndp.ni_rootdir else {
        // No root file system (see the module's deviations).
        return namei_fail(ndp, Errno::ENOENT);
    };

    if ndp.ni_cnd.cn_flags & KERNELPATH != 0 {
        ndp.ni_cnd.cn_flags |= BYPASSUNVEIL;
    } else if let Err(e) = pledge_namei(p, ndp, pn_str(pnbuf_ref(&ndp.ni_cnd))) {
        return namei_fail(ndp, e);
    }

    // Check if starting from root directory or current directory.
    let mut dp: &'static Vnode;
    if pn[0] == b'/' {
        dp = rootdir;
        vref(dp);
        if ndp.ni_cnd.cn_flags & (REALPATH | EXECPATH) != 0 && ndp.ni_cnd.cn_rpi == 0 {
            let rp = rpbuf(&ndp.ni_cnd);
            rp[0] = b'/';
            rp[1] = 0;
            ndp.ni_cnd.cn_rpi = 1;
        }
    } else if ndp.ni_dirfd == AT_FDCWD {
        let Some(cdir) = fdp.fd_cdir.get() else {
            // No current directory before a root is mounted (see the module's deviations).
            return namei_fail(ndp, Errno::ENOENT);
        };
        dp = cdir;
        vref(dp);
        unveil_start_relative(p, ndp, dp);
        unveil_check_component(p, ndp, dp);
    } else {
        let Some(fp) = fd_getfile(fdp, ndp.ni_dirfd) else {
            return namei_fail(ndp, Errno::EBADF);
        };
        if fp.f_type.get() != DTYPE_VNODE || fp.vnode().v_type.get() != VDIR {
            let _ = frele(fp, p);
            return namei_fail(ndp, Errno::ENOTDIR);
        }
        dp = fp.vnode();
        vref(dp);
        unveil_start_relative(p, ndp, dp);
        unveil_check_component(p, ndp, dp);
        let _ = frele(fp, p);
    }

    let error = loop {
        if dp.v_mount.get().is_none() {
            // Give up if the directory is no longer mounted
            vrele(dp);
            return namei_fail(ndp, Errno::ENOENT);
        }

        ndp.ni_cnd.cn_nameptr = ndp.ni_cnd.cn_pnbuf;
        ndp.ni_startdir = Some(dp);
        if let Err(e) = vfs_lookup(ndp) {
            return namei_fail(ndp, e);
        }

        // If not a symbolic link, return search result.
        if ndp.ni_cnd.cn_flags & ISSYMLINK == 0 {
            if let Err(e) = unveil_check_final(p, ndp) {
                let cn_flags = ndp.ni_cnd.cn_flags;
                if cn_flags & LOCKPARENT != 0
                    && cn_flags & ISLASTCN != 0
                    && !opt_eq(ndp.ni_vp, ndp.ni_dvp)
                    && let Some(dvp) = ndp.ni_dvp
                {
                    vput(dvp);
                }
                if let Some(vp) = ndp.ni_vp {
                    if cn_flags & LOCKLEAF != 0 {
                        vput(vp);
                    } else {
                        vrele(vp);
                    }
                }
                return namei_fail(ndp, e);
            }
            if ndp.ni_cnd.cn_flags & (SAVENAME | SAVESTART) == 0 {
                if let Some(buf) = NonNull::new(ndp.ni_cnd.cn_pnbuf) {
                    pool_put(&NAMEI_POOL, buf);
                }
                ndp.ni_cnd.cn_pnbuf = ptr::null_mut();
            } else {
                ndp.ni_cnd.cn_flags |= HASBUF;
            }
            return Ok(());
        }
        let (Some(vp), Some(dvp)) = (ndp.ni_vp, ndp.ni_dvp) else {
            crate::kern::subr_prf::panic(format_args!("namei: symlink without vnodes"));
        };
        if ndp.ni_cnd.cn_flags & LOCKPARENT != 0 && ndp.ni_cnd.cn_flags & ISLASTCN != 0 {
            let _ = VOP_UNLOCK(dvp);
        }
        let loopcnt = ndp.ni_loopcnt;
        ndp.ni_loopcnt += 1;
        if loopcnt >= SYMLOOP_MAX as u64 {
            break Errno::ELOOP;
        }
        let cp: *mut u8 = if ndp.ni_pathlen > 1 {
            match pool_get(&NAMEI_POOL, PR_WAITOK) {
                Some(buf) => buf.as_ptr(),
                None => break Errno::ENOMEM,
            }
        } else {
            ndp.ni_cnd.cn_pnbuf
        };
        let badlink = |ndp: &Nameidata<'_>, cp: *mut u8| {
            if ndp.ni_pathlen > 1
                && let Some(buf) = NonNull::new(cp)
            {
                pool_put(&NAMEI_POOL, buf);
            }
        };
        let mut aiov = [Iovec {
            iov_base: cp.cast::<c_void>(),
            iov_len: MAXPATHLEN,
        }];
        let mut auio = Uio {
            uio_iov: &mut aiov,
            uio_offset: 0,
            uio_resid: MAXPATHLEN,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: Some(p),
        };
        if let Err(e) = VOP_READLINK(vp, &mut auio, ndp.ni_cnd.cn_cred) {
            badlink(ndp, cp);
            break e;
        }
        let linklen = MAXPATHLEN - auio.uio_resid;
        if linklen == 0 {
            badlink(ndp, cp);
            break Errno::ENOENT;
        }
        if linklen + ndp.ni_pathlen >= MAXPATHLEN {
            badlink(ndp, cp);
            break Errno::ENAMETOOLONG;
        }
        if ndp.ni_pathlen > 1 {
            // SAFETY: `cp` is a fresh `MAXPATHLEN`-byte pool item and `ni_next` points at the
            // `ni_pathlen` remaining bytes (NUL included) of the old buffer; `linklen +
            // ni_pathlen < MAXPATHLEN`, and the two buffers are distinct.
            unsafe { ptr::copy_nonoverlapping(ndp.ni_next, cp.add(linklen), ndp.ni_pathlen) };
            if let Some(buf) = NonNull::new(ndp.ni_cnd.cn_pnbuf) {
                pool_put(&NAMEI_POOL, buf);
            }
            ndp.ni_cnd.cn_pnbuf = cp;
        } else {
            pnbuf(&ndp.ni_cnd)[linklen] = 0;
        }
        ndp.ni_pathlen += linklen;
        let pn = pnbuf(&ndp.ni_cnd);
        if ndp.ni_cnd.cn_flags & BPU_LOCALTIME != 0 {
            // /etc/localtime can be a symbolic link but must point into /usr/share/zoneinfo/
            // without ..
            if !checkzoneinfopath(pn_str(pn)) {
                break Errno::EACCES;
            }
            ndp.ni_cnd.cn_flags &= !BPU_LOCALTIME;
            ndp.ni_cnd.cn_flags |= BPU_ZONEINFO;
        }
        vput(vp);
        dp = dvp;
        // Check if root directory should replace current directory.
        if pn[0] == b'/' {
            vrele(dp);
            dp = rootdir;
            vref(dp);
            ndp.ni_unveil_match = ptr::null();
            unveil_check_component(p, ndp, dp);
            if ndp.ni_cnd.cn_flags & (REALPATH | EXECPATH) != 0 {
                let rp = rpbuf(&ndp.ni_cnd);
                rp[0] = b'/';
                rp[1] = 0;
                ndp.ni_cnd.cn_rpi = 1;
            }
        } else if ndp.ni_cnd.cn_flags & (REALPATH | EXECPATH) != 0 {
            component_pop(&mut ndp.ni_cnd);
        }
    };
    if let Some(dvp) = ndp.ni_dvp {
        vrele(dvp);
    }
    if let Some(vp) = ndp.ni_vp {
        vput(vp);
    }
    namei_fail(ndp, error)
}

/// `a == b` for two optional vnodes.
fn opt_eq(a: Option<&'static Vnode>, b: Option<&'static Vnode>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// The directory `vfs_lookup` is searching, and whether it is already unlocked.
struct Walk {
    dp: &'static Vnode,
    dpunlocked: bool,
}

/// The two error exits of `vfs_lookup`.
enum LookupError {
    /// `goto bad`: release the directory being searched.
    Bad(Errno),
    /// `goto bad2`: release the parent first.
    Bad2(Errno),
}

/// Search a pathname. This is a very central and rather complicated routine.
///
/// The pathname is pointed to by `ni_cnd.cn_nameptr` and is of length `ni_pathlen`. The
/// starting directory is taken from `ni_startdir`. The pathname is descended until done, or a
/// symbolic link is encountered. If the path is completed the flag `ISLASTCN` is set in
/// `ni_cnd.cn_flags`. If a symbolic link need interpretation is encountered, the flag
/// `ISSYMLINK` is set in `ni_cnd.cn_flags`.
///
/// The flag argument is `LOOKUP`, `CREATE`, `RENAME`, or `DELETE` depending on whether the
/// name is to be looked up, created, renamed, or deleted. When `CREATE`, `RENAME`, or
/// `DELETE` is specified, information usable in creating, renaming, or deleting a directory
/// entry may be calculated. If flag has `LOCKPARENT` or'ed into it, the parent directory is
/// returned locked. If flag has `WANTPARENT` or'ed into it, the parent directory is returned
/// unlocked. Otherwise the parent directory is not returned. If the target of the pathname
/// exists and `LOCKLEAF` is or'ed into the flag the target is returned locked, otherwise it
/// is returned unlocked. When creating or renaming and `LOCKPARENT` is specified, the target
/// may not be ".". When deleting and `LOCKPARENT` is specified, the target may be ".".
///
/// Overall outline of lookup:
///
/// ```text
/// dirloop:
///     identify next component of name at ndp->ni_ptr
///     handle degenerate case where name is null string
///     if .. and crossing mount points and on mounted filesys, find parent
///     call VOP_LOOKUP routine for next component name
///         directory vnode returned in ni_dvp, unlocked unless LOCKPARENT set
///         component vnode returned in ni_vp (if it exists), locked.
///     if result vnode is mounted on and crossing mount points,
///         find mounted on vnode
///     if more components of name, do next level at dirloop
///     return the answer in ni_vp, locked if LOCKLEAF set
///         if LOCKPARENT set, return locked parent in ni_dvp
///         if WANTPARENT set, return unlocked parent in ni_dvp
/// ```
pub fn vfs_lookup(ndp: &mut Nameidata<'_>) -> Result<(), Errno> {
    // Setup: break out flag bits into variables.
    let wantparent = ndp.ni_cnd.cn_flags & (LOCKPARENT | WANTPARENT) != 0;
    let mut docache = (ndp.ni_cnd.cn_flags & NOCACHE) ^ NOCACHE != 0;
    if ndp.ni_cnd.cn_nameiop == DELETE || (wantparent && ndp.ni_cnd.cn_nameiop != CREATE) {
        docache = false;
    }
    let rdonly = ndp.ni_cnd.cn_flags & RDONLY != 0;
    ndp.ni_dvp = None;
    ndp.ni_cnd.cn_flags &= !ISSYMLINK;
    let Some(dp) = ndp.ni_startdir.take() else {
        crate::kern::subr_prf::panic(format_args!("vfs_lookup: no ni_startdir"));
    };
    let _ = vn_lock(dp, LK_EXCLUSIVE | LK_RETRY);

    let mut st = Walk {
        dp,
        dpunlocked: false,
    };
    match lookup_walk(ndp, &mut st, wantparent, docache, rdonly) {
        Ok(()) => Ok(()),
        Err(err) => {
            let error = match err {
                LookupError::Bad2(error) => {
                    let flags = ndp.ni_cnd.cn_flags;
                    if let Some(dvp) = ndp.ni_dvp {
                        if flags & LOCKPARENT != 0
                            && flags & ISLASTCN != 0
                            && flags & PDIRUNLOCK == 0
                        {
                            let _ = VOP_UNLOCK(dvp);
                        }
                        vrele(dvp);
                    }
                    error
                }
                LookupError::Bad(error) => error,
            };
            if st.dpunlocked {
                vrele(st.dp);
            } else {
                vput(st.dp);
            }
            ndp.ni_vp = None;
            Err(error)
        }
    }
}

/// The body of `vfs_lookup`, from the leading slashes to `terminal:`.
fn lookup_walk(
    ndp: &mut Nameidata<'_>,
    st: &mut Walk,
    wantparent: bool,
    docache: bool,
    rdonly: bool,
) -> Result<(), LookupError> {
    use LookupError::{Bad, Bad2};

    let p = ndp.ni_cnd.proc();
    // SAFETY: the thread outlives its own lookup; detached from `ndp` so `ndp` can be
    // borrowed mutably below.
    let p: &Proc = unsafe { &*ptr::from_ref(p) };
    let pn = pnbuf_ref(&ndp.ni_cnd);
    let base = ndp.ni_cnd.cn_pnbuf.cast_const();
    let at = |i: usize| base.wrapping_add(i);
    // SAFETY: `cn_nameptr` points into `cn_pnbuf` (`namei` set it there).
    let mut name = unsafe { ndp.ni_cnd.cn_nameptr.offset_from(base) } as usize;

    // If we have a leading string of slashes, remove them, and just make sure the current
    // node is a directory.
    let mut cp = name;
    let mut terminal = false;
    if pn[cp] == b'/' {
        while pn[cp] == b'/' {
            cp += 1;
        }
        ndp.ni_pathlen -= cp - name;
        name = cp;
        ndp.ni_cnd.cn_nameptr = at(name);

        if st.dp.v_type.get() != VDIR {
            return Err(Bad(Errno::ENOTDIR));
        }

        // If we've exhausted the path name, then just return the current node. If the caller
        // requested the parent node (i.e. it's a CREATE, DELETE, or RENAME), and we don't have
        // one (because this is the root directory), then we must fail.
        if pn[name] == 0 {
            if ndp.ni_dvp.is_none() && wantparent {
                return Err(Bad(Errno::EISDIR));
            }
            ndp.ni_vp = Some(st.dp);
            ndp.ni_cnd.cn_flags |= ISLASTCN;
            terminal = true;
        }
    }

    if !terminal {
        // dirloop: Search a new directory.
        //
        // The last component of the filename is left accessible via cnp->cn_nameptr for
        // callers that need the name. Callers needing the name set the SAVENAME flag. When
        // done, they assume responsibility for freeing the pathname buffer.
        loop {
            ndp.ni_cnd.cn_consume = 0;

            // XXX: Figure out the length of the last component.
            cp = name;
            while pn[cp] != 0 && pn[cp] != b'/' {
                cp += 1;
            }
            ndp.ni_cnd.cn_namelen = (cp - name) as i64;
            if ndp.ni_cnd.cn_namelen > NAME_MAX as i64 {
                return Err(Bad(Errno::ENAMETOOLONG));
            }

            if ndp.ni_cnd.cn_flags & (REALPATH | EXECPATH) != 0 {
                let len = cp - name;
                if len == 2 && pn[name] == b'.' && pn[name + 1] == b'.' {
                    component_pop(&mut ndp.ni_cnd);
                } else if !(len == 1 && pn[name] == b'.')
                    && !component_push(&mut ndp.ni_cnd, &pn[name..cp])
                {
                    if ndp.ni_cnd.cn_flags & REALPATH != 0 {
                        return Err(Bad(Errno::ENAMETOOLONG));
                    }
                    // EXECPATH: give up building path
                    ndp.ni_cnd.cn_rpi = 0;
                    ndp.ni_cnd.cn_flags &= !EXECPATH;
                }
            }

            ndp.ni_pathlen -= ndp.ni_cnd.cn_namelen as usize;
            let mut next = cp;
            // If this component is followed by a slash, then move the pointer to the next
            // component forward, and remember that this component must be a directory.
            let slashes;
            if pn[cp] == b'/' {
                while pn[cp] == b'/' {
                    cp += 1;
                }
                slashes = cp - next;
                ndp.ni_pathlen -= slashes;
                next = cp;
                ndp.ni_cnd.cn_flags |= REQUIREDIR;
            } else {
                slashes = 0;
                ndp.ni_cnd.cn_flags &= !REQUIREDIR;
            }
            ndp.ni_next = at(next);
            // We do special processing on the last component, whether or not it's a
            // directory. Cache all intervening lookups, but not the final one.
            if pn[cp] == 0 {
                if docache {
                    ndp.ni_cnd.cn_flags |= MAKEENTRY;
                } else {
                    ndp.ni_cnd.cn_flags &= !MAKEENTRY;
                }
                ndp.ni_cnd.cn_flags |= ISLASTCN;
            } else {
                ndp.ni_cnd.cn_flags |= MAKEENTRY;
                ndp.ni_cnd.cn_flags &= !ISLASTCN;
            }
            if ndp.ni_cnd.cn_namelen == 2 && pn[name + 1] == b'.' && pn[name] == b'.' {
                ndp.ni_cnd.cn_flags |= ISDOTDOT;
            } else {
                ndp.ni_cnd.cn_flags &= !ISDOTDOT;
            }

            'nextname: {
                // Handle "..": two special cases.
                // 1. If at root directory (e.g. after chroot) or at absolute root directory
                //    then ignore it so can't get out.
                // 2. If this vnode is the root of a mounted filesystem, then replace it with
                //    the vnode which was mounted on so we take the .. in the other file
                //    system.
                if ndp.ni_cnd.cn_flags & ISDOTDOT != 0 {
                    loop {
                        unveil_check_component(p, ndp, st.dp);
                        if opt_eq(Some(st.dp), ndp.ni_rootdir) || opt_eq(Some(st.dp), rootvnode()) {
                            ndp.ni_dvp = Some(st.dp);
                            ndp.ni_vp = Some(st.dp);
                            vref(st.dp);
                            ndp.ni_unveil_match = ptr::null();
                            break 'nextname;
                        }
                        if st.dp.v_flag.get() & VROOT == 0
                            || ndp.ni_cnd.cn_flags & NOCROSSMOUNT != 0
                        {
                            break;
                        }
                        let tdp = st.dp;
                        let Some(covered) =
                            st.dp.v_mount.get().and_then(|mp| mp.mnt_vnodecovered.get())
                        else {
                            break;
                        };
                        st.dp = covered;
                        vput(tdp);
                        vref(st.dp);
                        unveil_check_component(p, ndp, st.dp);
                        let _ = vn_lock(st.dp, LK_EXCLUSIVE | LK_RETRY);
                    }
                }

                // We now have a segment name to search for, and a directory to search.
                ndp.ni_dvp = Some(st.dp);
                ndp.ni_vp = None;
                ndp.ni_cnd.cn_flags &= !PDIRUNLOCK;
                unveil_check_component(p, ndp, st.dp);

                if let Err(lookup_error) = VOP_LOOKUP(st.dp, &mut ndp.ni_vp, &mut ndp.ni_cnd) {
                    #[cfg(feature = "diagnostic")]
                    if ndp.ni_vp.is_some() {
                        crate::kern::subr_prf::panic(format_args!("leaf should be empty"));
                    }
                    let mut error = lookup_error;
                    // Allow for unveiling a file in a directory which we cannot create
                    // ourselves.
                    if ndp.ni_pledge == PLEDGE_UNVEIL
                        && matches!(error, Errno::EPERM | Errno::EACCES | Errno::EROFS)
                    {
                        error = Errno::EJUSTRETURN;
                    }

                    if error != Errno::EJUSTRETURN {
                        return Err(Bad(error));
                    }
                    // If this was not the last component, or there were trailing slashes,
                    // then the name must exist.
                    if ndp.ni_cnd.cn_flags & REQUIREDIR != 0 {
                        return Err(Bad(Errno::ENOENT));
                    }
                    // If creating and at end of pathname, then can consider allowing file to
                    // be created. Check for a read only filesystem and disallow this unless
                    // we are unveil'ing
                    if ndp.ni_pledge != PLEDGE_UNVEIL && (rdonly || mnt_rdonly(ndp.ni_dvp)) {
                        return Err(Bad(Errno::EROFS));
                    }
                    // We return with ni_vp NULL to indicate that the entry doesn't currently
                    // exist, leaving a pointer to the (possibly locked) directory vnode in
                    // ndp->ni_dvp.
                    if ndp.ni_cnd.cn_flags & SAVESTART != 0 {
                        ndp.ni_startdir = ndp.ni_dvp;
                        if let Some(sd) = ndp.ni_startdir {
                            vref(sd);
                        }
                    }
                    return Ok(());
                }

                // Take into account any additional components consumed by the underlying
                // filesystem. This will include any trailing slashes after the last
                // component consumed.
                if ndp.ni_cnd.cn_consume > 0 {
                    let consume = ndp.ni_cnd.cn_consume as usize;
                    if consume >= slashes {
                        ndp.ni_cnd.cn_flags &= !REQUIREDIR;
                    }

                    ndp.ni_pathlen = ndp.ni_pathlen + slashes - consume;
                    next = next + consume - slashes;
                    ndp.ni_next = at(next);
                    ndp.ni_cnd.cn_consume = 0;
                    if pn[next] == 0 {
                        ndp.ni_cnd.cn_flags |= ISLASTCN;
                    }
                }

                let Some(found) = ndp.ni_vp else {
                    crate::kern::subr_prf::panic(format_args!("vfs_lookup: no vnode found"));
                };
                st.dp = found;
                // Check to see if the vnode has been mounted on; if so find the root of the
                // mounted file system.
                while st.dp.v_type.get() == VDIR
                    && let Some(mp) = st.dp.v_mountedhere()
                    && ndp.ni_cnd.cn_flags & NOCROSSMOUNT == 0
                {
                    if vfs_busy(mp, VB_READ | VB_WAIT).is_err() {
                        continue;
                    }
                    let _ = VOP_UNLOCK(st.dp);
                    let root = VFS_ROOT(mp);
                    vfs_unbusy(mp);
                    match root {
                        Err(error) => {
                            st.dpunlocked = true;
                            return Err(Bad2(error));
                        }
                        Ok(tdp) => {
                            vrele(st.dp);
                            st.dp = tdp;
                            ndp.ni_vp = Some(tdp);
                        }
                    }
                }

                // Check for symbolic link. Back up over any slashes that we skipped, as we
                // will need them again.
                if st.dp.v_type.get() == VLNK && ndp.ni_cnd.cn_flags & (FOLLOW | REQUIREDIR) != 0 {
                    ndp.ni_pathlen += slashes;
                    ndp.ni_next = at(next - slashes);
                    ndp.ni_cnd.cn_flags |= ISSYMLINK;
                    if ndp.ni_cnd.cn_flags & BPU_ZONEINFO != 0 {
                        // /usr/share/zoneinfo prohibits symbolic links
                        return Err(Bad2(Errno::ELOOP));
                    }
                    return Ok(());
                }

                // Check for directory, if the component was followed by a series of slashes.
                if st.dp.v_type.get() != VDIR && ndp.ni_cnd.cn_flags & REQUIREDIR != 0 {
                    return Err(Bad2(Errno::ENOTDIR));
                }
            }

            // nextname: Not a symbolic link. If this was not the last component, then
            // continue at the next component, else return.
            if ndp.ni_cnd.cn_flags & ISLASTCN == 0 {
                // SAFETY: `ni_next` points into `cn_pnbuf`, set just above.
                name = unsafe { ndp.ni_next.offset_from(base) } as usize;
                ndp.ni_cnd.cn_nameptr = at(name);
                if let Some(dvp) = ndp.ni_dvp {
                    vrele(dvp);
                }
                continue;
            }
            break;
        }
    }

    // terminal:
    // __pledge_open() only opens regular files in /usr/share/zoneinfo
    if ndp.ni_cnd.cn_flags & BPU_ZONEINFO != 0 && st.dp.v_type.get() != crate::sys::vnode::VREG {
        return Err(Bad2(Errno::EACCES));
    }
    // Check for read-only file systems.
    if ndp.ni_cnd.cn_nameiop == DELETE || ndp.ni_cnd.cn_nameiop == RENAME {
        // Disallow directory write attempts on read-only file systems.
        if rdonly || mnt_rdonly(Some(st.dp)) || (wantparent && mnt_rdonly(ndp.ni_dvp)) {
            return Err(Bad2(Errno::EROFS));
        }
    }
    if let Some(dvp) = ndp.ni_dvp {
        if ndp.ni_cnd.cn_flags & SAVESTART != 0 {
            ndp.ni_startdir = Some(dvp);
            vref(dvp);
        }
        if !wantparent {
            vrele(dvp);
        }
    }
    if ndp.ni_cnd.cn_flags & LOCKLEAF == 0 {
        let _ = VOP_UNLOCK(st.dp);
    }
    Ok(())
}

/// Whether the vnode's file system is mounted read-only.
fn mnt_rdonly(vp: Option<&'static Vnode>) -> bool {
    vp.and_then(|vp| vp.v_mount.get())
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
}

/// Reacquire a path name component.
pub fn vfs_relookup(
    dvp: &'static Vnode,
    vpp: &mut Option<&'static Vnode>,
    cnp: &mut Componentname,
) -> Result<(), Errno> {
    // Setup: break out flag bits into variables.
    let wantparent = cnp.cn_flags & (LOCKPARENT | WANTPARENT) != 0;
    let rdonly = cnp.cn_flags & RDONLY != 0;
    cnp.cn_flags &= !ISSYMLINK;
    let dp = dvp;
    let _ = vn_lock(dp, LK_EXCLUSIVE | LK_RETRY);

    // dirloop: Search a new directory.
    //
    // The last component of the filename is left accessible via cnp->cn_nameptr for callers
    // that need the name. Callers needing the name set the SAVENAME flag. When done, they
    // assume responsibility for freeing the pathname buffer.

    // NAMEI_DIAGNOSTIC: not configured.

    // Check for degenerate name (e.g. / or "") which is a way of talking about a directory,
    // e.g. like "/." or ".".
    if cnp.name().first().is_none_or(|&c| c == 0) {
        crate::kern::subr_prf::panic(format_args!("relookup: null name"));
    }

    if cnp.cn_flags & ISDOTDOT != 0 {
        crate::kern::subr_prf::panic(format_args!("relookup: lookup on dot-dot"));
    }

    // We now have a segment name to search for, and a directory to search.
    if let Err(error) = VOP_LOOKUP(dp, vpp, cnp) {
        #[cfg(feature = "diagnostic")]
        if vpp.is_some() {
            crate::kern::subr_prf::panic(format_args!("leaf should be empty"));
        }
        if error != Errno::EJUSTRETURN {
            vput(dp);
            *vpp = None;
            return Err(error);
        }
        // If creating and at end of pathname, then can consider allowing file to be created.
        if rdonly || mnt_rdonly(Some(dvp)) {
            vput(dp);
            *vpp = None;
            return Err(Errno::EROFS);
        }
        // ASSERT(dvp == ndp->ni_startdir)
        if cnp.cn_flags & SAVESTART != 0 {
            vref(dvp);
        }
        // We return with ni_vp NULL to indicate that the entry doesn't currently exist,
        // leaving a pointer to the (possibly locked) directory vnode in ndp->ni_dvp.
        return Ok(());
    }
    let Some(dp) = *vpp else {
        crate::kern::subr_prf::panic(format_args!("relookup: no vnode found"));
    };

    #[cfg(feature = "diagnostic")]
    if dp.v_type.get() == VLNK && cnp.cn_flags & FOLLOW != 0 {
        crate::kern::subr_prf::panic(format_args!("relookup: symlink found."));
    }

    // Check for read-only file systems.
    if (cnp.cn_nameiop == DELETE || cnp.cn_nameiop == RENAME)
        && (rdonly || mnt_rdonly(Some(dp)) || (wantparent && mnt_rdonly(Some(dvp))))
    {
        // Disallow directory write attempts on read-only file systems.
        if cnp.cn_flags & LOCKPARENT != 0 && cnp.cn_flags & ISLASTCN != 0 {
            let _ = VOP_UNLOCK(dvp);
        }
        vrele(dvp);
        vput(dp);
        *vpp = None;
        return Err(Errno::EROFS);
    }
    // ASSERT(dvp == ndp->ni_startdir)
    if cnp.cn_flags & SAVESTART != 0 {
        vref(dvp);
    }
    if !wantparent {
        vrele(dvp);
    }
    if cnp.cn_flags & LOCKLEAF == 0 {
        let _ = VOP_UNLOCK(dp);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
