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
 * Copyright (c) 1989, 1993
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
 *	@(#)ufs_lookup.c	8.9 (Berkeley) 8/11/94
 */
/* </LICENSES> */

/* <CODE> */
//! Directory operations: `ufs_lookup` (convert a component of a pathname into a locked
//! vnode, and find where a new entry would go), and the routines that check, add, remove and
//! rewrite directory entries (`ufs_direnter`, `ufs_dirremove`, `ufs_dirrewrite`), test a
//! directory for emptiness and walk `..` to keep renames from making loops.
//!
//! Upstream: sys/ufs/ufs/ufs_lookup.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `#ifdef UFS_DIRHASH` is feature `ufs_dirhash` (default, as GENERIC has the option).
//!   The dirhash's `goto foundentry`/`goto notfound` into the linear search are a `hashed`
//!   flag that skips the search, with `found` set by the dirhash path; `ufsdirhash_build`'s
//!   0 is `true`, `ufsdirhash_findfree`'s and `ufsdirhash_enduseful`'s -1 is `None`.
//! - A `struct direct *` into a directory block is an offset into the buffer's bytes, its
//!   members read and written by `dir.rs`'s accessors; `memmove` of an entry is
//!   `copy_within`.
//! - `dirchk` is the atomic [`DIRCHK`].
//! - `ufs_dirbadentry` returns `bool` (`true` for the C's 1, a bad entry) and takes the block
//!   and the entry's offset; `ufs_dirempty` returns `bool`.
//! - The `newdirbp` argument of `ufs_direnter` (the soft dependency code's) is kept and
//!   unused, as in C.

use core::sync::atomic::{AtomicI32, Ordering};

use crate::kern::subr_prf::{Str, panic};
use crate::kern::vfs_bio::{bdwrite, brelse, bwrite};
use crate::kern::vfs_cache::{NCHSTATS, cache_enter, cache_lookup};
use crate::kern::vfs_subr::{vnoperm, vput, vref, vrele};
use crate::kern::vfs_vnops::{vn_lock, vn_rdwr};
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_BWRITE, VOP_UNLOCK};
use crate::kprintf;
use crate::machine::cpu::curproc;
use crate::sys::buf::{B_CLRBUF, B_SYNC, Buf};
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::mount::{MNT_RDONLY, VFS_VGET};
use crate::sys::namei::{
    CREATE, Componentname, DELETE, ISDOTDOT, ISLASTCN, LOCKPARENT, LOOKUP, MAKEENTRY, PDIRUNLOCK,
    RENAME, SAVENAME, WANTPARENT,
};
use crate::sys::param::roundup;
use crate::sys::ucred::Ucred;
use crate::sys::uio::{UioRw, UioSeg};
use crate::sys::vnode::{IO_NODELOCKED, IO_SYNC, VDIR, VEXEC, VWRITE, Vnode, VopLookupArgs};
use crate::ufs::ufs::dinode::{IFDIR, IFMT, ISVTX, ROOTINO, Ufsino};
use crate::ufs::ufs::dir::{
    DIR_ROUNDUP, DIRBLKSIZ, Direct, Dirtemplate, Doff, MAXNAMLEN, d_ino, d_name, d_namlen,
    d_reclen, dirsiz, iftodt, set_d_ino, set_d_reclen, set_d_type,
};
use crate::ufs::ufs::inode::{
    IN_CHANGE, IN_UPDATE, Inode, UFS_BUF_ALLOC, UFS_BUFATOFF, UFS_TRUNCATE, UFS_UPDATE, doingasync,
    vtoi,
};
#[cfg(feature = "ufs_dirhash")]
use crate::ufs::ufs::ufs_dirhash::{
    ufsdirhash_add, ufsdirhash_build, ufsdirhash_checkblock, ufsdirhash_dirtrunc,
    ufsdirhash_enduseful, ufsdirhash_findfree, ufsdirhash_lookup, ufsdirhash_move,
    ufsdirhash_newblk, ufsdirhash_remove,
};
use crate::ufs::ufs::ufsmount::vfstoufs;
use crate::uvm::uvm_vnode::uvm_vnp_setsize;

/// `dirchk`: full validation of directory entries during lookups (1 with `DIAGNOSTIC`).
pub static DIRCHK: AtomicI32 = AtomicI32::new(if cfg!(feature = "diagnostic") { 1 } else { 0 });

/// The slot search state of `ufs_lookup`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slotstatus {
    /// Still looking for a slot.
    None,
    /// Enough free space found that compaction would make room.
    Compact,
    /// A slot found (or none wanted).
    Found,
}

/// The vnode's mount, which a UFS vnode always has.
fn vmount(vp: &Vnode) -> &'static crate::sys::mount::Mount {
    match vp.v_mount.get() {
        Some(mp) => mp,
        None => panic(format_args!("ufs: vnode {:p} without a mount", vp)),
    }
}

/// `cnp->cn_cred`, which a lookup always has.
fn cn_cred(cnp: &Componentname) -> &Ucred {
    cnp.cred()
}

/// `ufs_lookup` (`vop_lookup`): convert a component of a pathname into a pointer to a
/// locked inode. This is a very central and rather complicated routine. If the file system
/// is not maintained in a strict tree hierarchy, this can result in a deadlock situation (see
/// comments in code below).
///
/// The `cnp->cn_nameiop` argument is `LOOKUP`, `CREATE`, `RENAME`, or `DELETE` depending on
/// whether the name is to be looked up, created, renamed, or deleted. When `CREATE`, `RENAME`,
/// or `DELETE` is specified, information usable in creating, renaming, or deleting a
/// directory entry may be calculated. If flag has `LOCKPARENT` or'ed into it and the target
/// of the pathname exists, lookup returns both the target and its parent directory locked.
/// When creating or renaming and `LOCKPARENT` is specified, the target may not be ".". When
/// deleting and `LOCKPARENT` is specified, the target may be ".", but the caller must check
/// to ensure it does an `vrele` and `vput` instead of two `vput`s.
///
/// Overall outline of `ufs_lookup`:
///
/// - check accessibility of directory
/// - look for name in cache, if found, then if at end of path and deleting or creating,
///   drop it, else return name
/// - search for name in directory, to found or notfound
/// - notfound: if creating, return locked directory, leaving info on available slots, else
///   return error
/// - found: if at end of path and deleting, return information to allow delete; if at end
///   of path and rewriting (`RENAME` and `LOCKPARENT`), lock target inode and return info to
///   allow rewrite; if not at end, add name to cache; if at end and neither creating nor
///   deleting, add name to cache
pub fn ufs_lookup(ap: &mut VopLookupArgs<'_>) -> Result<(), Errno> {
    let cnp = &mut *ap.a_cnp;
    let cred = cnp.cn_cred;
    let nameiop = cnp.cn_nameiop;

    cnp.cn_flags &= !PDIRUNLOCK;
    let flags = cnp.cn_flags;

    let mut bp: Option<&'static Buf> = None;
    let mut slotoffset: Doff = -1;
    *ap.a_vpp = None;
    let vdp = ap.a_dvp;
    let dp = vtoi(vdp);
    let lockparent = flags & LOCKPARENT != 0;
    let wantparent = flags & (LOCKPARENT | WANTPARENT) != 0;

    // Check accessibility of directory.
    if dp.dip_mode() & IFMT != IFDIR {
        return Err(Errno::ENOTDIR);
    }
    VOP_ACCESS(vdp, VEXEC, cred, cnp.proc())?;

    if flags & ISLASTCN != 0
        && vmount(vdp).mnt_flag.get() & MNT_RDONLY != 0
        && (nameiop == DELETE || nameiop == RENAME)
    {
        return Err(Errno::EROFS);
    }

    // We now have a segment name to search for, and a directory to search.
    //
    // Before tediously performing a linear scan of the directory, check the name cache to
    // see if the directory/name pair we are looking for is known already.
    if let Some(vp) = cache_lookup(vdp, cnp)? {
        *ap.a_vpp = Some(vp);
        return Ok(());
    }

    // Suppress search for slots unless creating file and at end of pathname, in which case
    // we watch for a place to put the new file in case it doesn't already exist.
    let mut slotstatus = Slotstatus::Found;
    let mut slotfreespace: i32 = 0;
    let mut slotsize: i32 = 0;
    let mut slotneeded: i32 = 0;
    if (nameiop == CREATE || nameiop == RENAME) && flags & ISLASTCN != 0 {
        slotstatus = Slotstatus::None;
        slotneeded = ((size_of::<Direct>() - MAXNAMLEN) as i32 + cnp.cn_namelen as i32 + 3) & !3;
    }

    // If there is cached information on a previous search of this directory, pick up where
    // we last left off. We cache only lookups as these are the most common and have the
    // greatest payoff. Caching CREATE has little benefit as it usually must search the entire
    // directory to determine that the entry does not exist. Caching the location of the last
    // DELETE or RENAME has not reduced profiling time and hence has been removed in the
    // interest of simplicity.
    let bmask = vfstoufs(vmount(vdp)).mountp().mnt_stat.get().f_iosize as i32 - 1;

    // The entry found: its DIRSIZ.
    let mut found: Option<usize> = None;
    let mut numdirpasses = 1;
    let mut prevoff: Doff = 0;
    let mut enduseful: Doff = 0;

    // Use dirhash for fast operations on large directories. The logic to determine whether
    // to hash the directory is contained within ufsdirhash_build(); a true return means that
    // it decided to hash this directory and it successfully built up the hash table.
    // `hashed`: the dirhash answered, found (`goto foundentry`) or not (`goto notfound`).
    #[cfg(not(feature = "ufs_dirhash"))]
    let hashed = false;
    #[cfg(feature = "ufs_dirhash")]
    let hashed = ufsdirhash_build(dp) && {
        // Look for a free slot if needed.
        enduseful = dp.dip_size() as Doff;
        if slotstatus != Slotstatus::Found {
            slotoffset = -1;
            if let Some((off, size)) = ufsdirhash_findfree(dp, slotneeded) {
                slotoffset = off;
                slotsize = size;
                slotstatus = Slotstatus::Compact;
                enduseful = ufsdirhash_enduseful(dp).unwrap_or(dp.dip_size() as Doff);
            }
        }
        // Look up the component.
        let prevoffp = if nameiop == DELETE {
            Some(&mut prevoff)
        } else {
            None
        };
        match ufsdirhash_lookup(dp, cnp.name(), prevoffp) {
            Ok((offset, b)) => {
                dp.i_offset.set(offset);
                bp = Some(b);
                // SAFETY: the buffer is ours (busy from ufsdirhash_lookup) and mapped; the
                // slice dies before the buffer is released.
                let data = unsafe { b.data() };
                let ep = (offset & bmask) as usize;
                // foundentry: save directory entry's inode number and reclen in ndp->ni_ufs
                // area, and release directory buffer.
                dp.i_ino.set(d_ino(data, ep));
                dp.i_reclen.set(u32::from(d_reclen(data, ep)));
                found = Some(dirsiz(d_namlen(data, ep)));
                true
            }
            Err(Errno::ENOENT) => {
                // notfound:
                dp.i_offset
                    .set(roundup(dp.dip_size() as usize, DIRBLKSIZ) as Doff);
                true
            }
            // Something failed; just do a linear search.
            Err(_) => false,
        }
    };

    let mut entryoffsetinblock: i32 = 0;
    let mut endsearch: Doff = 0;
    if !hashed {
        if nameiop != LOOKUP || dp.i_diroff.get() == 0 || dp.i_diroff.get() as u64 >= dp.dip_size()
        {
            entryoffsetinblock = 0;
            dp.i_offset.set(0);
            numdirpasses = 1;
        } else {
            dp.i_offset.set(dp.i_diroff.get());
            entryoffsetinblock = dp.i_offset.get() & bmask;
            if entryoffsetinblock != 0 {
                let (b, _) = UFS_BUFATOFF(dp, i64::from(dp.i_offset.get()))?;
                bp = Some(b);
            }
            numdirpasses = 2;
            NCHSTATS.ncs_2passes.fetch_add(1, Ordering::Relaxed);
        }
        prevoff = dp.i_offset.get();
        endsearch = roundup(dp.dip_size() as usize, DIRBLKSIZ) as Doff;
        enduseful = 0;
    }

    'searchloop: loop {
        while !hashed && dp.i_offset.get() < endsearch {
            // If necessary, get the next directory block.
            if dp.i_offset.get() & bmask == 0 {
                if let Some(b) = bp.take() {
                    brelse(b);
                }
                let (b, _) = UFS_BUFATOFF(dp, i64::from(dp.i_offset.get()))?;
                bp = Some(b);
                entryoffsetinblock = 0;
            }
            // If still looking for a slot, and at a DIRBLKSIZE boundary, have to start
            // looking for free space again.
            if slotstatus == Slotstatus::None && entryoffsetinblock & (DIRBLKSIZ as i32 - 1) == 0 {
                slotoffset = -1;
                slotfreespace = 0;
            }
            let Some(b) = bp else {
                panic(format_args!("ufs_lookup: no directory block"));
            };
            // SAFETY: the buffer is ours (busy from UFS_BUFATOFF) and mapped; the slice dies
            // before the buffer is released.
            let data = unsafe { b.data() };
            let ep = entryoffsetinblock as usize;

            // Get pointer to next entry. Full validation checks are slow, so we only check
            // enough to insure forward progress through the directory. Complete checks can
            // be run by patching "dirchk" to be true.
            let reclen = d_reclen(data, ep);
            if reclen == 0
                || (DIRCHK.load(Ordering::Relaxed) != 0 && ufs_dirbadentry(vdp, data, ep))
            {
                ufs_dirbad(dp, dp.i_offset.get(), "mangled entry");
                let i = DIRBLKSIZ as i32 - (entryoffsetinblock & (DIRBLKSIZ as i32 - 1));
                dp.i_offset.set(dp.i_offset.get() + i);
                entryoffsetinblock += i;
                continue;
            }

            // If an appropriate sized slot has not yet been found, check to see if one is
            // available. Also accumulate space in the current block so that we can determine
            // if compaction is viable.
            if slotstatus != Slotstatus::Found {
                let mut size = i32::from(reclen);

                if d_ino(data, ep) != 0 {
                    size -= dirsiz(d_namlen(data, ep)) as i32;
                }
                if size > 0 {
                    if size >= slotneeded {
                        slotstatus = Slotstatus::Found;
                        slotoffset = dp.i_offset.get();
                        slotsize = i32::from(reclen);
                    } else if slotstatus == Slotstatus::None {
                        slotfreespace += size;
                        if slotoffset == -1 {
                            slotoffset = dp.i_offset.get();
                        }
                        if slotfreespace >= slotneeded {
                            slotstatus = Slotstatus::Compact;
                            slotsize = dp.i_offset.get() + i32::from(reclen) - slotoffset;
                        }
                    }
                }
            }

            // Check for a name match.
            if d_ino(data, ep) != 0 {
                let namlen = usize::from(d_namlen(data, ep));
                if namlen as i64 == cnp.cn_namelen && d_name(data, ep, namlen) == cnp.name() {
                    // Save directory entry's inode number and reclen in ndp->ni_ufs area, and
                    // release directory buffer.
                    dp.i_ino.set(d_ino(data, ep));
                    dp.i_reclen.set(u32::from(reclen));
                    found = Some(dirsiz(d_namlen(data, ep)));
                    break 'searchloop;
                }
            }
            prevoff = dp.i_offset.get();
            dp.i_offset.set(dp.i_offset.get() + i32::from(reclen));
            entryoffsetinblock += i32::from(reclen);
            if d_ino(data, ep) != 0 {
                enduseful = dp.i_offset.get();
            }
        }
        // notfound:
        // If we started in the middle of the directory and failed to find our target, we
        // must check the beginning as well.
        if numdirpasses == 2 {
            numdirpasses -= 1;
            dp.i_offset.set(0);
            endsearch = dp.i_diroff.get();
            continue 'searchloop;
        }
        break;
    }

    let Some(ep_dirsiz) = found else {
        if let Some(b) = bp {
            brelse(b);
        }
        // If creating, and at end of pathname and current directory has not been removed,
        // then can consider allowing file to be created.
        if (nameiop == CREATE || nameiop == RENAME)
            && flags & ISLASTCN != 0
            && dp.i_effnlink.get() != 0
        {
            // Access for write is interpreted as allowing creation of files in the
            // directory.
            VOP_ACCESS(vdp, VWRITE, cred, cnp.proc())?;
            // Return an indication of where the new directory entry should be put. If we
            // didn't find a slot, then set dp->i_count to 0 indicating that the new slot
            // belongs at the end of the directory. If we found a slot, then the new entry can
            // be put in the range from dp->i_offset to dp->i_offset + dp->i_count.
            if slotstatus == Slotstatus::None {
                dp.i_offset
                    .set(roundup(dp.dip_size() as usize, DIRBLKSIZ) as Doff);
                dp.i_count.set(0);
                enduseful = dp.i_offset.get();
            } else if nameiop == DELETE {
                dp.i_offset.set(slotoffset);
                if dp.i_offset.get() & (DIRBLKSIZ as i32 - 1) == 0 {
                    dp.i_count.set(0);
                } else {
                    dp.i_count.set(dp.i_offset.get() - prevoff);
                }
            } else {
                dp.i_offset.set(slotoffset);
                dp.i_count.set(slotsize);
                if enduseful < slotoffset + slotsize {
                    enduseful = slotoffset + slotsize;
                }
            }
            dp.i_endoff
                .set(roundup(enduseful as usize, DIRBLKSIZ) as Doff);
            // We return with the directory locked, so that the parameters we set up above
            // will still be valid if we actually decide to do a direnter(). We return ni_vp
            // == NULL to indicate that the entry does not currently exist; we leave a
            // pointer to the (locked) directory inode in ndp->ni_dvp. The pathname buffer is
            // saved so that the name can be obtained later.
            //
            // NB - if the directory is unlocked, then this information cannot be used.
            cnp.cn_flags |= SAVENAME;
            if !lockparent {
                let _ = VOP_UNLOCK(vdp);
                cnp.cn_flags |= PDIRUNLOCK;
            }
            return Err(Errno::EJUSTRETURN);
        }
        // Insert name into cache (as non-existent) if appropriate.
        if cnp.cn_flags & MAKEENTRY != 0 && nameiop != CREATE {
            cache_enter(vdp, *ap.a_vpp, cnp);
        }
        return Err(Errno::ENOENT);
    };

    // found:
    if numdirpasses == 2 {
        NCHSTATS.ncs_pass2.fetch_add(1, Ordering::Relaxed);
    }
    // Check that directory length properly reflects presence of this entry.
    if dp.i_offset.get() as u64 + ep_dirsiz as u64 > dp.dip_size() {
        ufs_dirbad(dp, dp.i_offset.get(), "i_ffs_size too small");
        dp.dip_set_size(dp.i_offset.get() as u64 + ep_dirsiz as u64);
        dp.set_flag(IN_CHANGE | IN_UPDATE);
    }
    if let Some(b) = bp {
        brelse(b);
    }

    // Found component in pathname. If the final component of path name, save information in
    // the cache as to where the entry was found.
    if flags & ISLASTCN != 0 && nameiop == LOOKUP {
        dp.i_diroff.set(dp.i_offset.get() & !(DIRBLKSIZ as i32 - 1));
    }

    // If deleting, and at end of pathname, return parameters which can be used to remove
    // file. If the wantparent flag isn't set, we return only the directory (in
    // ndp->ni_dvp), otherwise we go on and lock the inode, being careful with ".".
    if nameiop == DELETE && flags & ISLASTCN != 0 {
        // Write access to directory required to delete files.
        VOP_ACCESS(vdp, VWRITE, cred, cnp.proc())?;
        // Return pointer to current entry in dp->i_offset, and distance past previous entry
        // (if there is a previous entry in this block) in dp->i_count. Save directory inode
        // pointer in ndp->ni_dvp for dirremove().
        if dp.i_offset.get() & (DIRBLKSIZ as i32 - 1) == 0 {
            dp.i_count.set(0);
        } else {
            dp.i_count.set(dp.i_offset.get() - prevoff);
        }
        if dp.i_number.get() == dp.i_ino.get() {
            vref(vdp);
            *ap.a_vpp = Some(vdp);
            return Ok(());
        }
        let tdp = VFS_VGET(vmount(vdp), u64::from(dp.i_ino.get()))?;
        // If directory is "sticky", then user must own the directory, or the file in it,
        // else she may not delete it (unless she's root). This implements append-only
        // directories.
        let uid = cn_cred(cnp).cr_uid.get();
        if dp.dip_mode() & ISVTX != 0
            && uid != 0
            && uid != dp.dip_uid()
            && !vnoperm(vdp)
            && vtoi(tdp).dip_uid() != uid
        {
            vput(tdp);
            return Err(Errno::EPERM);
        }
        *ap.a_vpp = Some(tdp);
        if !lockparent {
            let _ = VOP_UNLOCK(vdp);
            cnp.cn_flags |= PDIRUNLOCK;
        }
        return Ok(());
    }

    // If rewriting (RENAME), return the inode and the information required to rewrite the
    // present directory. Must get inode of directory entry to verify it's a regular file, or
    // empty directory.
    if nameiop == RENAME && wantparent && flags & ISLASTCN != 0 {
        VOP_ACCESS(vdp, VWRITE, cred, cnp.proc())?;
        // Careful about locking second inode. This can only occur if the target is ".".
        if dp.i_number.get() == dp.i_ino.get() {
            return Err(Errno::EISDIR);
        }
        let tdp = VFS_VGET(vmount(vdp), u64::from(dp.i_ino.get()))?;
        *ap.a_vpp = Some(tdp);
        cnp.cn_flags |= SAVENAME;
        if !lockparent {
            let _ = VOP_UNLOCK(vdp);
            cnp.cn_flags |= PDIRUNLOCK;
        }
        return Ok(());
    }

    // Step through the translation in the name. We do not `vput' the directory because we
    // may need it again if a symbolic link is relative to the current directory. Instead we
    // save it unlocked as "pdp". We must get the target inode before unlocking the directory
    // to insure that the inode will not be removed before we get it. We prevent deadlock by
    // always fetching inodes from the root, moving down the directory tree. Thus when
    // following backward pointers ".." we must unlock the parent directory before getting
    // the requested directory. There is a potential race condition here if both the current
    // and parent directories are removed before the VFS_VGET for the inode associated with
    // ".." returns. We hope that this occurs infrequently since we cannot avoid this race
    // condition without implementing a sophisticated deadlock detection algorithm. Note also
    // that this simple deadlock detection scheme will not work if the file system has any
    // hard links other than ".." that point backwards in the directory structure.
    let pdp = vdp;
    if flags & ISDOTDOT != 0 {
        let _ = VOP_UNLOCK(pdp); // race to get the inode
        cnp.cn_flags |= PDIRUNLOCK;
        let tdp = match VFS_VGET(vmount(vdp), u64::from(dp.i_ino.get())) {
            Ok(tdp) => tdp,
            Err(e) => {
                if vn_lock(pdp, LK_EXCLUSIVE | LK_RETRY).is_ok() {
                    cnp.cn_flags &= !PDIRUNLOCK;
                }
                return Err(e);
            }
        };
        if lockparent && flags & ISLASTCN != 0 {
            if let Err(e) = vn_lock(pdp, LK_EXCLUSIVE) {
                vput(tdp);
                return Err(e);
            }
            cnp.cn_flags &= !PDIRUNLOCK;
        }
        *ap.a_vpp = Some(tdp);
    } else if dp.i_number.get() == dp.i_ino.get() {
        vref(vdp); // we want ourself, ie "."
        *ap.a_vpp = Some(vdp);
    } else {
        let tdp = VFS_VGET(vmount(vdp), u64::from(dp.i_ino.get()))?;
        if !lockparent || flags & ISLASTCN == 0 {
            let _ = VOP_UNLOCK(pdp);
            cnp.cn_flags |= PDIRUNLOCK;
        }
        *ap.a_vpp = Some(tdp);
    }

    // Insert name into cache if appropriate.
    if cnp.cn_flags & MAKEENTRY != 0 {
        cache_enter(vdp, *ap.a_vpp, cnp);
    }
    Ok(())
}

/// `ufs_dirbad`: report a bad directory entry; fatal on a read-write file system.
pub fn ufs_dirbad(ip: &Inode, offset: Doff, how: &str) {
    let mp = vmount(ip.itov());

    let (name, len) = mp.mntonname();
    kprintf!(
        "{}: bad dir ino {} at offset {}: {}\n",
        Str(&name[..len]),
        ip.i_number.get(),
        offset,
        how
    );
    if mp.mnt_stat.get().f_flags & MNT_RDONLY as u32 == 0 {
        panic(format_args!("bad dir"));
    }
}

/// `ufs_dirbadentry`: do consistency checking on the directory entry at `off` in `b`:
/// record length must be multiple of 4; entry must fit in rest of its `DIRBLKSIZ` block;
/// record must be large enough to contain entry; name is not longer than `MAXNAMLEN`; name
/// must be as long as advertised, and null terminated. `true` for a bad entry.
pub fn ufs_dirbadentry(_vdp: &Vnode, b: &[u8], off: usize) -> bool {
    let reclen = usize::from(d_reclen(b, off));
    let namlen = usize::from(d_namlen(b, off));
    if reclen & 0x3 != 0
        || reclen > DIRBLKSIZ - (off & (DIRBLKSIZ - 1))
        || reclen < dirsiz(d_namlen(b, off))
        || namlen > MAXNAMLEN
    {
        kprintf!("First bad\n");
        return true;
    }
    if d_ino(b, off) == 0 {
        return false;
    }
    let name = d_name(b, off, namlen + 1);
    if name[..namlen.min(name.len())].contains(&0) {
        kprintf!("Second bad\n");
        return true;
    }
    name.get(namlen).copied().unwrap_or(1) != 0
}

/// `ufs_makedirentry`: construct a new directory entry after a call to namei, using the
/// parameters that it left in the componentname argument `cnp`. The argument `ip` is the
/// inode to which the new directory entry will refer.
pub fn ufs_makedirentry(ip: &Inode, cnp: &Componentname, newdirp: &mut Direct) {
    #[cfg(feature = "diagnostic")]
    if cnp.cn_flags & SAVENAME == 0 {
        panic(format_args!("ufs_makedirentry: missing name"));
    }
    let namelen = cnp.cn_namelen as usize;
    newdirp.d_ino = ip.i_number.get();
    newdirp.d_namlen = namelen as u8;
    let pad = namelen & !(DIR_ROUNDUP - 1);
    newdirp.d_name[pad..pad + DIR_ROUNDUP].fill(0);
    newdirp.d_name[..namelen].copy_from_slice(cnp.name());
    newdirp.d_type = iftodt(ip.dip_mode());
}

/// `ufs_direnter`: write a directory entry after a call to namei, using the parameters that
/// it left in the directory inode. `dirp` is the new directory entry contents. `dvp` is the
/// directory to be written, which was left locked by namei. Remaining parameters
/// (`dp->i_offset`, `dp->i_count`) indicate how the space for the new entry is to be
/// obtained. Non-null `newdirbp` indicates that a directory is being created (for the soft
/// dependency code).
pub fn ufs_direnter(
    dvp: &'static Vnode,
    tvp: Option<&'static Vnode>,
    dirp: &mut Direct,
    cnp: &Componentname,
    _newdirbp: Option<&'static Buf>,
) -> Result<(), Errno> {
    let cr = cnp.cn_cred;
    let dp = vtoi(dvp);
    let newentrysize = dirsiz(dirp.d_namlen);

    if dp.i_count.get() == 0 {
        // If dp->i_count is 0, then namei could find no space in the directory. Here,
        // dp->i_offset will be on a directory block boundary and we will write the new entry
        // into a fresh block.
        if dp.i_offset.get() & (DIRBLKSIZ as i32 - 1) != 0 {
            panic(format_args!("ufs_direnter: newblk"));
        }
        let flags = B_CLRBUF | B_SYNC;
        let bp = UFS_BUF_ALLOC(
            dp,
            i64::from(dp.i_offset.get()),
            DIRBLKSIZ as i32,
            cr,
            flags,
        )?;
        dp.dip_set_size(dp.i_offset.get() as u64 + DIRBLKSIZ as u64);
        dp.set_flag(IN_CHANGE | IN_UPDATE);
        uvm_vnp_setsize(dvp, dp.dip_size() as i64);
        dirp.d_reclen = DIRBLKSIZ as u16;
        let blkoff = dp.i_offset.get() as usize
            & (vfstoufs(vmount(dvp)).mountp().mnt_stat.get().f_iosize as usize - 1);
        // SAFETY: the buffer is ours (busy from UFS_BUF_ALLOC) and mapped; the slice dies
        // before it is written.
        let data = unsafe { bp.data() };
        dirp.write_to(data, blkoff);

        #[cfg(feature = "ufs_dirhash")]
        if dp.i_dirhash.get().is_some() {
            ufsdirhash_newblk(dp, dp.i_offset.get());
            ufsdirhash_add(
                dp,
                &dirp.d_name[..usize::from(dirp.d_namlen)],
                dp.i_offset.get(),
            );
            ufsdirhash_checkblock(dp, &data[blkoff..blkoff + DIRBLKSIZ], dp.i_offset.get());
        }

        let error = VOP_BWRITE(bp);
        let ret = UFS_UPDATE(dp, 1);
        error?;
        return ret;
    }

    // If dp->i_count is non-zero, then namei found space for the new entry in the range
    // dp->i_offset to dp->i_offset + dp->i_count in the directory. To use this space, we may
    // have to compact the entries located there, by copying them together towards the
    // beginning of the block, leaving the free space in one usable chunk at the end.

    // Increase size of directory if entry eats into new space. This should never push the
    // size past a new multiple of DIRBLKSIZE.
    //
    // N.B. - THIS IS AN ARTIFACT OF 4.2 AND SHOULD NEVER HAPPEN.
    if (dp.i_offset.get() + dp.i_count.get()) as u64 > dp.dip_size() {
        dp.dip_set_size((dp.i_offset.get() + dp.i_count.get()) as u64);
    }
    // Get the block containing the space for the new directory entry.
    let (bp, off) = UFS_BUFATOFF(dp, i64::from(dp.i_offset.get()))?;
    {
        // SAFETY: the buffer is ours (busy from UFS_BUFATOFF) and mapped; the slice dies
        // before it is written.
        let data = unsafe { bp.data() };
        let dirbuf = &mut data[off..];
        // Find space for the new entry. In the simple case, the entry at offset base will
        // have the space. If it does not, then namei arranged that compacting the region
        // dp->i_offset to dp->i_offset + dp->i_count would yield the space.
        let mut ep = 0usize;
        let mut dsize = if d_ino(dirbuf, ep) != 0 {
            dirsiz(d_namlen(dirbuf, ep))
        } else {
            0
        };
        let mut spacefree = usize::from(d_reclen(dirbuf, ep)) - dsize;
        let mut loc = usize::from(d_reclen(dirbuf, ep));
        while loc < dp.i_count.get() as usize {
            let nep = loc;

            // Trim the existing slot (NB: dsize may be zero).
            set_d_reclen(dirbuf, ep, dsize as u16);
            ep += dsize;

            // Read nep->d_reclen now as the memmove() may clobber it.
            let nep_reclen = usize::from(d_reclen(dirbuf, nep));
            loc += nep_reclen;
            if d_ino(dirbuf, nep) == 0 {
                // A mid-block unused entry. Such entries are never created by the kernel,
                // but fsck_ffs can create them (and it doesn't fix them).
                //
                // Add up the free space, and initialise the relocated entry since we don't
                // memmove it.
                spacefree += nep_reclen;
                set_d_ino(dirbuf, ep, 0);
                dsize = 0;
                continue;
            }
            dsize = dirsiz(d_namlen(dirbuf, nep));
            spacefree += nep_reclen - dsize;
            #[cfg(feature = "ufs_dirhash")]
            if dp.i_dirhash.get().is_some() {
                let namlen = usize::from(d_namlen(dirbuf, nep));
                ufsdirhash_move(
                    dp,
                    d_name(dirbuf, nep, namlen),
                    dp.i_offset.get() + nep as Doff,
                    dp.i_offset.get() + ep as Doff,
                );
            }
            dirbuf.copy_within(nep..nep + dsize, ep);
        }
        // Here, `ep' points to a directory entry containing `dsize' in-use bytes followed by
        // `spacefree' unused bytes. If ep->d_ino == 0, then the entry is completely unused
        // (dsize == 0). The value of ep->d_reclen is always indeterminate.
        //
        // Update the pointer fields in the previous entry (if any), copy in the new entry,
        // and write out the block.
        if d_ino(dirbuf, ep) == 0 {
            if spacefree + dsize < newentrysize {
                panic(format_args!("ufs_direnter: compact1"));
            }
            dirp.d_reclen = (spacefree + dsize) as u16;
        } else {
            if spacefree < newentrysize {
                panic(format_args!("ufs_direnter: compact2"));
            }
            dirp.d_reclen = spacefree as u16;
            set_d_reclen(dirbuf, ep, dsize as u16);
            ep += dsize;
        }

        #[cfg(feature = "ufs_dirhash")]
        if dp.i_dirhash.get().is_some()
            && (d_ino(dirbuf, ep) == 0 || usize::from(dirp.d_reclen) == spacefree)
        {
            ufsdirhash_add(
                dp,
                &dirp.d_name[..usize::from(dirp.d_namlen)],
                dp.i_offset.get() + ep as Doff,
            );
        }
        dirp.write_to(dirbuf, ep);
        #[cfg(feature = "ufs_dirhash")]
        if dp.i_dirhash.get().is_some() {
            let blk = off - (dp.i_offset.get() as usize & (DIRBLKSIZ - 1));
            ufsdirhash_checkblock(
                dp,
                &data[blk..blk + DIRBLKSIZ],
                dp.i_offset.get() & !(DIRBLKSIZ as i32 - 1),
            );
        }
    }

    let mut error = VOP_BWRITE(bp);
    dp.set_flag(IN_CHANGE | IN_UPDATE);

    // If all went well, and the directory can be shortened, proceed with the truncation.
    // Note that we have to unlock the inode for the entry that we just entered, as the
    // truncation may need to lock other inodes which can lead to deadlock if we also hold a
    // lock on the newly entered node.
    if error.is_ok() && dp.i_endoff.get() != 0 && (dp.i_endoff.get() as u64) < dp.dip_size() {
        if let Some(tvp) = tvp {
            let _ = VOP_UNLOCK(tvp);
        }
        error = UFS_TRUNCATE(dp, i64::from(dp.i_endoff.get()), IO_SYNC, cr);
        #[cfg(feature = "ufs_dirhash")]
        if error.is_ok() && dp.i_dirhash.get().is_some() {
            ufsdirhash_dirtrunc(dp, dp.i_endoff.get());
        }
        if let Some(tvp) = tvp {
            let _ = vn_lock(tvp, LK_EXCLUSIVE | LK_RETRY);
        }
    }
    error
}

/// `ufs_dirremove`: remove a directory entry after a call to namei, using the parameters
/// which it left in the directory inode. The entry `dp->i_offset` contains the offset into
/// the directory of the entry to be eliminated. The `dp->i_count` field contains the size
/// of the previous record in the directory. If this is 0, the first entry is being deleted,
/// so we need only zero the inode number to mark the entry as free. If the entry is not the
/// first in the directory, we must reclaim the space of the now empty record by adding the
/// record size to the size of the previous entry.
pub fn ufs_dirremove(
    dvp: &'static Vnode,
    ip: Option<&Inode>,
    _flags: u64,
    _isrmdir: i32,
) -> Result<(), Errno> {
    let dp = vtoi(dvp);

    let (bp, off) = UFS_BUFATOFF(dp, i64::from(dp.i_offset.get() - dp.i_count.get()))?;
    {
        // SAFETY: the buffer is ours (busy from UFS_BUFATOFF) and mapped; the slice dies
        // before it is written.
        let data = unsafe { bp.data() };
        // Remove the dirhash entry. This is complicated by the fact that `ep' is the previous
        // entry when dp->i_count != 0.
        #[cfg(feature = "ufs_dirhash")]
        if dp.i_dirhash.get().is_some() {
            let rp = if dp.i_count.get() == 0 {
                off
            } else {
                off + usize::from(d_reclen(data, off))
            };
            let namlen = usize::from(d_namlen(data, rp));
            ufsdirhash_remove(dp, d_name(data, rp, namlen), dp.i_offset.get());
        }

        if dp.i_count.get() == 0 {
            // First entry in block: set d_ino to zero.
            set_d_ino(data, off, 0);
        } else {
            // Collapse new free space into previous entry.
            let r = d_reclen(data, off) as u32 + dp.i_reclen.get();
            set_d_reclen(data, off, r as u16);
        }
        #[cfg(feature = "ufs_dirhash")]
        if dp.i_dirhash.get().is_some() {
            let blk = off - ((dp.i_offset.get() - dp.i_count.get()) as usize & (DIRBLKSIZ - 1));
            ufsdirhash_checkblock(
                dp,
                &data[blk..blk + DIRBLKSIZ],
                dp.i_offset.get() & !(DIRBLKSIZ as i32 - 1),
            );
        }
    }
    if let Some(ip) = ip {
        ip.i_effnlink.set(ip.i_effnlink.get() - 1);
        ip.dip_set_nlink(ip.dip_nlink() - 1);
        ip.set_flag(IN_CHANGE);
    }
    let error = if doingasync(dvp) && dp.i_count.get() != 0 {
        bdwrite(bp);
        Ok(())
    } else {
        bwrite(bp)
    };

    dp.set_flag(IN_CHANGE | IN_UPDATE);
    error
}

/// `ufs_dirrewrite`: rewrite an existing directory entry to point at the inode supplied.
/// The parameters describing the directory entry are set up by a call to namei.
pub fn ufs_dirrewrite(
    dp: &Inode,
    oip: &Inode,
    newinum: Ufsino,
    newtype: u8,
    _isrmdir: i32,
) -> Result<(), Errno> {
    let vdp = dp.itov();

    let (bp, off) = UFS_BUFATOFF(dp, i64::from(dp.i_offset.get()))?;
    {
        // SAFETY: the buffer is ours (busy from UFS_BUFATOFF) and mapped; the slice dies
        // before it is written.
        let data = unsafe { bp.data() };
        set_d_ino(data, off, newinum);
        set_d_type(data, off, newtype);
    }
    oip.i_effnlink.set(oip.i_effnlink.get() - 1);
    oip.dip_set_nlink(oip.dip_nlink() - 1);
    oip.set_flag(IN_CHANGE);
    let error = if doingasync(vdp) {
        bdwrite(bp);
        Ok(())
    } else {
        VOP_BWRITE(bp)
    };
    dp.set_flag(IN_CHANGE | IN_UPDATE);
    error
}

/// `MINDIRSIZ`: the first half of a `struct dirtemplate`, enough for a `struct direct`
/// whose name is at most 3 bytes.
const MINDIRSIZ: usize = Dirtemplate::SIZE / 2;

/// `ufs_dirempty`: check if a directory is empty or not (only `.` and `..`, the latter
/// naming `parentino`). Inode supplied must be locked.
///
/// Using a struct dirtemplate here is not precisely what we want, but better than using a
/// struct direct.
///
/// NB: does not handle corrupted directories.
pub fn ufs_dirempty(ip: &Inode, parentino: Ufsino, cred: *const Ucred) -> bool {
    let m = ip.dip_size() as i64;
    let mut dbuf = [0u8; Dirtemplate::SIZE];
    let mut off: i64 = 0;
    while off < m {
        let mut count = 0usize;
        let error = vn_rdwr(
            UioRw::UIO_READ,
            ip.itov(),
            dbuf.as_mut_ptr().cast(),
            MINDIRSIZ,
            off,
            UioSeg::UIO_SYSSPACE,
            IO_NODELOCKED,
            cred,
            Some(&mut count),
            curproc(),
        );
        // Since we read MINDIRSIZ, residual must be 0 unless we're at end of file.
        if error.is_err() || count != 0 {
            return false;
        }
        let reclen = d_reclen(&dbuf, 0);
        // avoid infinite loops
        if reclen == 0 {
            return false;
        }
        off += i64::from(reclen);
        let ino = d_ino(&dbuf, 0);
        // skip empty entries
        if ino == 0 {
            continue;
        }
        // accept only "." and ".."
        let namlen = d_namlen(&dbuf, 0);
        if namlen > 2 {
            return false;
        }
        let name = d_name(&dbuf, 0, 2);
        if name[0] != b'.' {
            return false;
        }
        // At this point namlen must be 1 or 2. 1 implies ".", 2 implies ".." if second char
        // is also "."
        if namlen == 1 && ino == ip.i_number.get() {
            continue;
        }
        if name[1] == b'.' && ino == parentino {
            continue;
        }
        return false;
    }
    true
}

/// `ufs_checkpath`: check if source directory is in the path of the target directory.
/// Target is supplied locked, source is unlocked. The target is always vput before
/// returning.
pub fn ufs_checkpath(source: &Inode, target: &Inode, cred: *const Ucred) -> Result<(), Errno> {
    let mut vp = Some(target.itov());
    let rootino = ROOTINO;
    let error: Result<(), Errno> = 'out: {
        if target.i_number.get() == source.i_number.get() {
            break 'out Err(Errno::EEXIST);
        }
        if target.i_number.get() == rootino {
            break 'out Ok(());
        }

        loop {
            let Some(cur) = vp else {
                break 'out Ok(());
            };
            if cur.v_type.get() != VDIR {
                break 'out Err(Errno::ENOTDIR);
            }
            let mut dirbuf = [0u8; Dirtemplate::SIZE];
            if let Err(e) = vn_rdwr(
                UioRw::UIO_READ,
                cur,
                dirbuf.as_mut_ptr().cast(),
                Dirtemplate::SIZE,
                0,
                UioSeg::UIO_SYSSPACE,
                IO_NODELOCKED,
                cred,
                None,
                curproc(),
            ) {
                break 'out Err(e);
            }
            let t = Dirtemplate::from_bytes(&dirbuf);
            if t.dotdot_namlen != 2 || t.dotdot_name[0] != b'.' || t.dotdot_name[1] != b'.' {
                break 'out Err(Errno::ENOTDIR);
            }
            if t.dotdot_ino == source.i_number.get() {
                break 'out Err(Errno::EINVAL);
            }
            if t.dotdot_ino == rootino {
                break 'out Ok(());
            }
            let _ = VOP_UNLOCK(cur);
            let next = VFS_VGET(vmount(cur), u64::from(t.dotdot_ino));
            vrele(cur);
            match next {
                Ok(nextvp) => vp = Some(nextvp),
                Err(e) => {
                    vp = None;
                    break 'out Err(e);
                }
            }
        }
    };

    if error == Err(Errno::ENOTDIR) {
        kprintf!("checkpath: .. not a directory\n");
    }
    if let Some(vp) = vp {
        vput(vp);
    }
    error
}
/* </CODE> */
