/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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

/*
 * Copyright (c) 1999 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Bill Sommerfeld.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `getcwd(3)` in the kernel: the path from a directory up to the root, found through the
//! name cache's reverse map (`vfs_getcwd_getcache`) or, failing that, by looking `..` up and
//! scanning it for the child's file number (`vfs_getcwd_scandir`). Shared by
//! `sys___getcwd`, `vn_isunder` and `sys___realpath`.
//!
//! Upstream: sys/kern/vfs_getcwd.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The output buffer and the position in it are `Option<(&mut [u8], &mut usize)>`: the
//!   path is built backwards from the index, where the C moves a `char *` (`*bpp`) inside
//!   `bufp`. `vfs_getcwd_getcache` returns `Ok(false)` for the C's `-1` ("try the hard
//!   way").
//! - `DIRBLKSIZ` is `ufs/ufs/dir.h`'s (`DEV_BSIZE`); that header waits for ufs.
//! - With no root file system (no `rootvnode`) or no current directory, the walk fails with
//!   `ENOENT`, as `namei` does (`vfs_lookup.rs`).
//! - `KTRACE` is not configured.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::vfs_cache::cache_revlookup;
use crate::kern::vfs_init::rootvnode;
use crate::kern::vfs_subr::{vget, vput, vref, vrele};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_GETATTR, VOP_LOOKUP, VOP_READDIR, VOP_UNLOCK};
use crate::machine::copy::copyoutstr;
use crate::sys::dirent::{Dirent, dirent_recsize};
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::{M_TEMP, M_WAITOK};
use crate::sys::namei::{Componentname, ISDOTDOT, ISLASTCN, LOOKUP, RDONLY};
use crate::sys::param::{DEV_BSIZE, MAXPATHLEN};
use crate::sys::proc::Proc;
use crate::sys::syscallargs::SysGetcwdArgs;
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::types::Register;
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::{GETCWD_CHECK_ACCESS, VDIR, VEXEC, VREAD, VROOT, Vattr, Vnode};

/// `DIRBLKSIZ` (`ufs/ufs/dir.h`): only for the smallest directory buffer.
const DIRBLKSIZ: usize = DEV_BSIZE;

/// The `".."` that `vfs_getcwd_scandir` looks up.
static DOTDOT: [u8; 3] = *b"..\0";

/// Find parent vnode of `*lvpp`, return in `*uvpp`.
///
/// On return `*lvpp` is NULL (its reference is gone); `*uvpp` holds the parent, if found. With
/// `buf`, the child's name is copied just before the index, which moves back to its start.
pub fn vfs_getcwd_scandir(
    lvpp: &mut Option<&'static Vnode>,
    uvpp: &mut Option<&'static Vnode>,
    buf: Option<(&mut [u8], &mut usize)>,
    p: &Proc,
) -> Result<(), Errno> {
    let Some(lvp) = *lvpp else {
        crate::kern::subr_prf::panic(format_args!("vfs_getcwd_scandir: no vnode"));
    };
    let mut va = Vattr::new();

    // If we want the filename, get some info we need while the current directory is still
    // locked.
    if buf.is_some()
        && let Err(error) = VOP_GETATTR(lvp, &mut va, p.p_ucred.get(), p)
    {
        vput(lvp);
        *lvpp = None;
        *uvpp = None;
        return Err(error);
    }

    let mut cn = Componentname::new();
    cn.cn_nameiop = LOOKUP;
    cn.cn_flags = ISLASTCN | ISDOTDOT | RDONLY;
    cn.cn_proc = p;
    cn.cn_cred = p.p_ucred.get();
    cn.cn_pnbuf = ptr::null_mut();
    cn.cn_nameptr = DOTDOT.as_ptr();
    cn.cn_namelen = 2;
    cn.cn_consume = 0;

    // Get parent vnode using lookup of '..'
    if let Err(error) = VOP_LOOKUP(lvp, uvpp, &mut cn) {
        vput(lvp);
        *lvpp = None;
        *uvpp = None;
        return Err(error);
    }

    let Some(uvp) = *uvpp else {
        crate::kern::subr_prf::panic(format_args!("vfs_getcwd_scandir: no parent"));
    };

    let mut dirbuf: Option<(NonNull<u8>, usize)> = None;
    let error = 'out: {
        // If we don't care about the pathname, we're done
        let Some((bufp, bpp)) = buf else {
            break 'out Ok(());
        };

        let fileno = va.va_fileid;

        let mut dirbuflen = DIRBLKSIZ;
        if (dirbuflen as i64) < va.va_blocksize {
            dirbuflen = va.va_blocksize as usize;
        }
        // XXX we need some limit for fuse, 1 MB should be enough
        if dirbuflen > 0xfffff {
            break 'out Err(Errno::EINVAL);
        }
        let Some(mem) = malloc(dirbuflen, M_TEMP, M_WAITOK) else {
            break 'out Err(Errno::ENOMEM);
        };
        dirbuf = Some((mem, dirbuflen));

        let mut off = 0;
        let mut tries = 0;

        loop {
            let mut iov = [Iovec {
                iov_base: mem.as_ptr().cast::<c_void>(),
                iov_len: dirbuflen,
            }];
            let mut uio = Uio {
                uio_iov: &mut iov,
                uio_offset: off,
                uio_resid: dirbuflen,
                uio_segflg: UioSeg::UIO_SYSSPACE,
                uio_rw: UioRw::UIO_READ,
                uio_procp: Some(p),
            };

            let mut eofflag = 0;

            // Call VOP_READDIR of parent
            let error = VOP_READDIR(uvp, &mut uio, p.p_ucred.get(), &mut eofflag);

            off = uio.uio_offset;

            // Try again if NFS tosses its cookies
            if error == Err(Errno::EINVAL) && tries < 3 {
                tries += 1;
                off = 0;
                continue;
            } else if let Err(e) = error {
                break 'out Err(e); // Old userland getcwd() behaviour
            }

            // SAFETY: `mem` is the `dirbuflen`-byte buffer `VOP_READDIR` just filled, owned
            // here until the `free` below.
            let dirbytes = unsafe { slice::from_raw_parts(mem.as_ptr(), dirbuflen) };
            let mut cpos = 0;
            tries = 0;

            // Scan directory page looking for matching vnode
            let mut len = (dirbuflen - uio.uio_resid) as isize;
            while len > 0 {
                let Some(dp) = Dirent::from_bytes(&dirbytes[cpos..]) else {
                    break 'out Err(Errno::EINVAL);
                };
                let reclen = dp.d_reclen as usize;

                // Check for malformed directory
                if reclen < dirent_recsize(1) || reclen as isize > len {
                    break 'out Err(Errno::EINVAL);
                }

                if dp.d_fileno == fileno {
                    let namlen = dp.d_namlen as usize;
                    if Dirent::NAME_OFFSET + namlen > reclen {
                        break 'out Err(Errno::EINVAL);
                    }
                    if *bpp <= namlen {
                        break 'out Err(Errno::ERANGE);
                    }
                    let bp = *bpp - namlen;

                    let name = &dirbytes[cpos + Dirent::NAME_OFFSET..][..namlen];
                    bufp[bp..bp + namlen].copy_from_slice(name);
                    *bpp = bp;

                    break 'out Ok(());
                }

                cpos += reclen;
                len -= reclen as isize;
            }

            if eofflag != 0 {
                break;
            }
        }

        Err(Errno::ENOENT)
    };

    // out:
    vrele(lvp);
    *lvpp = None;

    if let Some((mem, len)) = dirbuf {
        free(mem, M_TEMP, len);
    }

    error
}

/// Do a lookup in the vnode-to-name reverse map. `Ok(false)` is the C's `-1`: not found
/// there, the caller tries the hard way (with `*lvpp` still held and locked).
pub fn vfs_getcwd_getcache(
    lvpp: &mut Option<&'static Vnode>,
    uvpp: &mut Option<&'static Vnode>,
    buf: Option<(&mut [u8], &mut usize)>,
) -> Result<bool, Errno> {
    let Some(lvp) = *lvpp else {
        crate::kern::subr_prf::panic(format_args!("vfs_getcwd_getcache: no vnode"));
    };
    // Save original position to restore to on error
    let (bufp, bpp) = match buf {
        Some((bufp, bpp)) => (Some(bufp), Some(bpp)),
        None => (None, None),
    };
    let obp = bpp.as_deref().copied();

    let mut bp = obp.unwrap_or(0);
    let found = cache_revlookup(lvp, &mut bp, bufp);
    let uvp = match found {
        Err(error) => {
            vput(lvp);
            *lvpp = None;
            *uvpp = None;
            return Err(error);
        }
        Ok(None) => return Ok(false),
        Ok(Some(uvp)) => uvp,
    };
    if let Some(bpp) = bpp {
        *bpp = bp;
        *uvpp = Some(uvp);
        return getcache_finish(lvpp, uvpp, lvp, uvp, Some((bpp, obp.unwrap_or(0))));
    }
    *uvpp = Some(uvp);
    getcache_finish(lvpp, uvpp, lvp, uvp, None)
}

/// The rest of `vfs_getcwd_getcache` once the cache named the parent: trade the child's lock
/// for the parent's.
fn getcache_finish(
    lvpp: &mut Option<&'static Vnode>,
    uvpp: &mut Option<&'static Vnode>,
    lvp: &'static Vnode,
    uvp: &'static Vnode,
    restore: Option<(&mut usize, usize)>,
) -> Result<bool, Errno> {
    let vpid = uvp.v_id.get();

    // Release current lock before acquiring the parent lock
    let _ = VOP_UNLOCK(lvp);

    let mut error = vget(uvp, LK_EXCLUSIVE | LK_RETRY);
    if error.is_err() {
        *uvpp = None;
    }

    // Verify that vget() succeeded, and check that vnode capability didn't change while we
    // were waiting for the lock.
    if error.is_err() || vpid != uvp.v_id.get() {
        // Try to get our lock back. If that works, tell the caller to try things the hard
        // way, otherwise give up.
        if error.is_ok() {
            vput(uvp);
        }

        *uvpp = None;

        error = vn_lock(lvp, LK_EXCLUSIVE | LK_RETRY);
        if error.is_ok() {
            if let Some((bpp, obp)) = restore {
                *bpp = obp; // restore the buffer
            }
            return Ok(false);
        }
    }

    vrele(lvp);
    *lvpp = None;

    error.map(|()| true)
}

/// Common routine shared by `sys___getcwd()` and `vn_isunder()` and `sys___realpath()`.
pub fn vfs_getcwd_common(
    lvp: &'static Vnode,
    rvp: Option<&'static Vnode>,
    mut buf: Option<(&mut [u8], &mut usize)>,
    mut limit: i32,
    flags: i32,
    p: &Proc,
) -> Result<(), Errno> {
    let fdp = p.fd();
    let mut perms = VEXEC;

    let Some(rvp) = rvp.or(fdp.fd_rdir.get()).or_else(rootvnode) else {
        // No root file system (see the module's deviations).
        return Err(Errno::ENOENT);
    };

    let first = lvp;
    vref(rvp);
    vref(first);

    let mut lvp: Option<&'static Vnode> = Some(first);
    let mut uvp: Option<&'static Vnode> = None;

    let error = 'out: {
        if let Err(error) = vn_lock(first, LK_EXCLUSIVE | LK_RETRY) {
            vrele(first);
            lvp = None;
            break 'out Err(error);
        }

        if ptr::eq(first, rvp) {
            if let Some((bufp, bpp)) = buf.as_mut() {
                **bpp -= 1;
                bufp[**bpp] = b'/';
            }
            break 'out Ok(());
        }

        // This loop will terminate when we hit the root, VOP_READDIR() or VOP_LOOKUP() fails,
        // or we run out of space in the user buffer.
        loop {
            let Some(mut cur) = lvp else {
                break 'out Err(Errno::ENOENT);
            };
            if cur.v_type.get() != VDIR {
                break 'out Err(Errno::ENOTDIR);
            }

            // Check for access if caller cares
            if flags & GETCWD_CHECK_ACCESS != 0 {
                if let Err(error) = VOP_ACCESS(cur, perms, p.p_ucred.get(), p) {
                    break 'out Err(error);
                }
                perms = VEXEC | VREAD;
            }

            // Step up if we're a covered vnode
            while cur.v_flag.get() & VROOT != 0 {
                if ptr::eq(cur, rvp) {
                    break 'out Ok(());
                }

                let tvp = cur;
                let covered = cur.v_mount.get().and_then(|mp| mp.mnt_vnodecovered.get());

                vput(tvp);

                let Some(covered) = covered else {
                    lvp = None;
                    break 'out Err(Errno::ENOENT);
                };
                cur = covered;
                lvp = Some(cur);

                vref(cur);

                if let Err(error) = vn_lock(cur, LK_EXCLUSIVE | LK_RETRY) {
                    vrele(cur);
                    lvp = None;
                    break 'out Err(error);
                }
            }

            // Look in the name cache
            let reborrow = buf.as_mut().map(|(b, i)| (&mut **b, &mut **i));
            let error = match vfs_getcwd_getcache(&mut lvp, &mut uvp, reborrow) {
                // If that fails, look in the directory
                Ok(false) => {
                    let reborrow = buf.as_mut().map(|(b, i)| (&mut **b, &mut **i));
                    vfs_getcwd_scandir(&mut lvp, &mut uvp, reborrow, p)
                }
                Ok(true) => Ok(()),
                Err(e) => Err(e),
            };

            if let Err(e) = error {
                break 'out Err(e);
            }

            #[cfg(feature = "diagnostic")]
            {
                if lvp.is_some() {
                    crate::kern::subr_prf::panic(format_args!("getcwd: oops, forgot to null lvp"));
                }
                if let Some((_, bpp)) = buf.as_ref()
                    && **bpp == 0
                {
                    crate::kern::subr_prf::panic(format_args!("getcwd: oops, went back too far"));
                }
            }

            if let Some((bufp, bpp)) = buf.as_mut() {
                **bpp -= 1;
                bufp[**bpp] = b'/';
            }

            lvp = uvp.take();
            limit -= 1;

            if lvp.is_some_and(|l| ptr::eq(l, rvp)) || limit <= 0 {
                break;
            }
        }
        Ok(())
    };

    // out:
    if let Some(uvp) = uvp {
        vput(uvp);
    }

    if let Some(lvp) = lvp {
        vput(lvp);
    }

    vrele(rvp);

    error
}

/// Find pathname of a process's current directory.
#[allow(non_snake_case)] // the C name: sys___getcwd
pub fn sys___getcwd(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetcwdArgs = sysargs(v);
    let mut len = uap.len.get();

    if len > MAXPATHLEN * 4 {
        len = MAXPATHLEN * 4;
    } else if len < 2 {
        return Err(Errno::ERANGE);
    }

    let Some(mem) = malloc(len, M_TEMP, M_WAITOK) else {
        return Err(Errno::ENOMEM);
    };
    // SAFETY: a fresh `len`-byte allocation, owned here until the `free` below; every byte
    // is written before it is read (the path is built backwards from the NUL).
    let path = unsafe { slice::from_raw_parts_mut(mem.as_ptr(), len) };

    let mut bp = len - 1;
    path[bp] = 0;

    // 5th argument here is "max number of vnodes to traverse". Since each entry takes up at
    // least 2 bytes in the output buffer, limit it to N/2 vnodes for an N byte buffer.
    let error = match p.fd().fd_cdir.get() {
        None => Err(Errno::ENOENT), // no root file system yet
        Some(cdir) => vfs_getcwd_common(
            cdir,
            None,
            Some((&mut *path, &mut bp)),
            (len / 2) as i32,
            GETCWD_CHECK_ACCESS,
            p,
        ),
    }
    // Put the result into user buffer
    .and_then(|()| copyoutstr(&path[bp..], uap.buf.get() as usize).map(|_| ()));

    // KTRACE: not configured.

    free(mem, M_TEMP, len);

    error
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use core::sync::atomic::Ordering;

    use super::*;
    use crate::kern::vfs_cache::NCHSTATS;
    use crate::kern::vfs_lookup::{namei, ndinit};
    use crate::kern::vfs_subr::tests::testfs::{self, A, C, any_locked, vget_node};
    use crate::kern::vfs_vnops::vn_isunder;
    use crate::sys::namei::{FOLLOW, NiDirp};

    fn cwd(p: &Proc, vp: &'static Vnode) -> std::vec::Vec<u8> {
        let mut buf = [0u8; 64];
        let mut bp = buf.len() - 1;
        let lim = 32;
        vfs_getcwd_common(
            vp,
            None,
            Some((&mut buf, &mut bp)),
            lim,
            GETCWD_CHECK_ACCESS,
            p,
        )
        .unwrap();
        buf[bp..buf.len() - 1].to_vec()
    }

    #[test]
    fn getcwd_by_directory_scan_then_by_cache() {
        let (_g, p, mp) = testfs::setup_root();
        let c = vget_node(mp, C).unwrap();
        let _ = VOP_UNLOCK(c);
        // Nothing cached: each step looks ".." up and scans it.
        assert_eq!(cwd(p, c), b"/a/c");
        assert!(!any_locked());

        // After a lookup the reverse map answers.
        let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(b"/a/c"), p);
        namei(&mut nd).unwrap();
        vrele(nd.ni_vp.unwrap());
        let revhits = NCHSTATS.ncs_revhits.load(Ordering::SeqCst);
        assert_eq!(cwd(p, c), b"/a/c");
        assert_eq!(NCHSTATS.ncs_revhits.load(Ordering::SeqCst), revhits + 2);

        let root = rootvnode().unwrap();
        assert_eq!(cwd(p, root), b"/");
        assert!(vn_isunder(c, root, p));
        let a = vget_node(mp, A).unwrap();
        let _ = VOP_UNLOCK(a);
        assert!(vn_isunder(c, a, p));
        assert!(!vn_isunder(a, c, p));
        vrele(a);
        vrele(c);
        assert!(!any_locked());
    }
}
/* </TESTS> */
