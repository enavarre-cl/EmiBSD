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

/*-
 * Copyright (c) 1989, 1993, 1994
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley
 * by Pace Willisson (pace@blitz.com).  The Rock Ridge Extension
 * Support code is derived from software contributed to Berkeley
 * by Atsushi Murai (amurai@spec.co.jp).
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
 *	from: @(#)ufs_lookup.c	7.33 (Berkeley) 5/19/91
 *
 *	@(#)cd9660_lookup.c	8.5 (Berkeley) 12/5/94
 */
/* </LICENSES> */

/* <CODE> */
//! Directory lookup on ISO 9660: `cd9660_lookup` (convert a component of a pathname into a
//! locked vnode, by a linear scan of the directory's records) and `cd9660_bufatoff` (the
//! buffer of a directory block).
//!
//! Upstream: sys/isofs/cd9660/cd9660_lookup.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The C's `goto`s between `searchloop`, `foundino`, `notfound` and `found` are a loop
//!   over the passes and a labelled block whose value says where the scan ended.
//! - A `struct iso_directory_record *` into the directory buffer is its offset in the
//!   buffer, viewed as an [`IsoDirectoryRecord`].
//! - `cd9660_bufatoff` returns the buffer and the offset of `offset` in it (the C's `res`)
//!   instead of filling `*bpp` and `*res`.
//! - The Rock Ridge name buffer is a `NAME_MAX + 1` byte array on the stack instead of a
//!   `NAME_MAX` byte `malloc(M_TEMP)`: the name translation of a record without an `NM`
//!   entry can be one byte longer than `NAME_MAX` (the associated-file prefix), which the C
//!   would write past its buffer.
//! - `NOSORTBUG` is not defined, as in GENERIC.

use core::sync::atomic::Ordering;

use crate::isofs::cd9660::cd9660_extern::{ISO_FTYPE_RRIP, blkoff, blksize, lblkno};
use crate::isofs::cd9660::cd9660_node::{Doff, IsoNode, isodirino, vtoi};
use crate::isofs::cd9660::cd9660_rrip::cd9660_rrip_getname;
use crate::isofs::cd9660::cd9660_util::isofncmp;
use crate::isofs::cd9660::cd9660_vfsops::cd9660_vget_internal;
use crate::isofs::cd9660::iso::{
    ASSOCCHAR, Cdino, ISO_DIRECTORY_RECORD_SIZE, IsoDirectoryRecord, isonum_711,
};
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{bread, brelse};
use crate::kern::vfs_cache::{cache_enter, cache_lookup};
use crate::kern::vfs_subr::{vput, vref};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_UNLOCK};
use crate::sys::buf::Buf;
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::mount::{MNT_RDONLY, Mount};
use crate::sys::namei::{
    CREATE, DELETE, ISDOTDOT, ISLASTCN, LOCKPARENT, LOOKUP, MAKEENTRY, Nchstats, PDIRUNLOCK, RENAME,
};
use crate::sys::param::DEV_BSHIFT;
use crate::sys::syslimits::NAME_MAX;
use crate::sys::types::Off;
use crate::sys::vnode::{VEXEC, Vnode, VopLookupArgs};

/// Where the scan of a directory ended.
enum Scan {
    /// `found`: the entry at the given offset in the buffer is the one.
    Found(usize),
    /// `foundino`: a match was seen (`ino`, at `saveoffset`), the scan has gone past it.
    FoundIno(usize),
    /// `notfound`.
    NotFound,
}

/// `iso_nchstats`.
pub static ISO_NCHSTATS: Nchstats = Nchstats::new();

/// The vnode's mount, which a cd9660 vnode always has.
fn vmount(vp: &Vnode) -> &'static Mount {
    match vp.v_mount.get() {
        Some(mp) => mp,
        None => panic(format_args!("cd9660: vnode {:p} without a mount", vp)),
    }
}

/// `cd9660_lookup` (`vop_lookup`): convert a component of a pathname into a pointer to a
/// locked inode. This is a very central and rather complicated routine. If the file system
/// is not maintained in a strict tree hierarchy, this can result in a deadlock situation
/// (see comments in code below).
///
/// The flag argument is `LOOKUP`, `CREATE`, `RENAME`, or `DELETE` depending on whether the
/// name is to be looked up, created, renamed, or deleted. When `CREATE`, `RENAME`, or
/// `DELETE` is specified, information usable in creating, renaming, or deleting a directory
/// entry may be calculated. If flag has `LOCKPARENT` or'ed into it and the target of the
/// pathname exists, lookup returns both the target and its parent directory locked. When
/// creating or renaming and `LOCKPARENT` is specified, the target may not be ".". When
/// deleting and `LOCKPARENT` is specified, the target may be ".", but the caller must check
/// to ensure it does an `vrele` and `iput` instead of two `iput`s.
///
/// Overall outline of `cd9660_lookup`:
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
///
/// NOTE: (`LOOKUP | LOCKPARENT`) currently returns the parent inode unlocked.
pub fn cd9660_lookup(ap: &mut VopLookupArgs<'_>) -> Result<(), Errno> {
    let cnp = &mut *ap.a_cnp;
    let cred = cnp.cn_cred;
    let nameiop = cnp.cn_nameiop;

    cnp.cn_flags &= !PDIRUNLOCK;
    let flags = cnp.cn_flags;

    let mut bp: Option<&'static Buf> = None;
    *ap.a_vpp = None;
    let vdp = ap.a_dvp;
    let dp = vtoi(vdp);
    let imp = dp.mnt();
    let lockparent = flags & LOCKPARENT != 0;

    // Check accessibility of directory.
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

    let mut name = cnp.name();
    // A leading `=' means, we are looking for an associated file
    let assoc = imp.iso_ftype != ISO_FTYPE_RRIP && name.first() == Some(&ASSOCCHAR);
    if assoc {
        name = &name[1..];
    }

    // If there is cached information on a previous search of this directory, pick up where
    // we last left off. We cache only lookups as these are the most common and have the
    // greatest payoff. Caching CREATE has little benefit as it usually must search the entire
    // directory to determine that the entry does not exist. Caching the location of the last
    // DELETE or RENAME has not reduced profiling time and hence has been removed in the
    // interest of simplicity.
    let bmask = imp.im_bmask as Doff;
    let lbs = imp.logical_block_size as usize;
    let mut entryoffsetinblock: usize;
    let mut numdirpasses;
    if nameiop != LOOKUP || dp.i_diroff.get() == 0 || dp.i_diroff.get() > dp.i_size.get() {
        entryoffsetinblock = 0;
        dp.i_offset.set(0);
        numdirpasses = 1;
    } else {
        dp.i_offset.set(dp.i_diroff.get());
        entryoffsetinblock = (dp.i_offset.get() & bmask) as usize;
        if entryoffsetinblock != 0 {
            let (b, _) = cd9660_bufatoff(dp, dp.i_offset.get() as Off)?;
            bp = Some(b);
        }
        numdirpasses = 2;
        ISO_NCHSTATS.ncs_2passes.fetch_add(1, Ordering::Relaxed);
    }
    let mut endsearch = dp.i_size.get();
    let mut saveoffset: Doff = Doff::MAX; // the C's int -1
    let mut ino: Cdino = 0;

    // The record that was found: its offset in `bp`.
    let ep: usize = 'searchloop: loop {
        let scan = 'scan: {
            while dp.i_offset.get() < endsearch {
                // If offset is on a block boundary, read the next directory block. Release
                // previous if it exists.
                if dp.i_offset.get() & bmask == 0 {
                    if let Some(b) = bp.take() {
                        brelse(b);
                    }
                    let (b, _) = cd9660_bufatoff(dp, dp.i_offset.get() as Off)?;
                    bp = Some(b);
                    entryoffsetinblock = 0;
                }
                let Some(b) = bp else {
                    panic(format_args!("cd9660_lookup: no directory buffer"));
                };
                // SAFETY: the buffer is ours (busy from `bread`) and mapped; no other view of
                // it is alive.
                let data: &[u8] = unsafe { b.data() };

                // Get pointer to next entry.
                let reclen = usize::from(data.get(entryoffsetinblock).copied().unwrap_or(0));
                if reclen == 0 {
                    // skip to next block, if any
                    dp.i_offset
                        .set((dp.i_offset.get() & !bmask) + imp.logical_block_size as Doff);
                    continue;
                }

                if reclen < ISO_DIRECTORY_RECORD_SIZE {
                    // illegal entry, stop
                    break;
                }

                if entryoffsetinblock + reclen > lbs {
                    // entries are not allowed to cross boundaries
                    break;
                }

                let Some(rec) = data
                    .get(entryoffsetinblock..)
                    .and_then(IsoDirectoryRecord::new)
                else {
                    break;
                };
                let namelen = usize::from(isonum_711(rec.name_len()));

                if reclen < ISO_DIRECTORY_RECORD_SIZE + namelen {
                    // illegal entry, stop
                    break;
                }

                // Check for a name match.
                if imp.iso_ftype == ISO_FTYPE_RRIP {
                    ino = if isonum_711(rec.flags()) & 2 != 0 {
                        isodirino(&rec, imp)
                    } else {
                        ((b.b_blkno.get() << DEV_BSHIFT) + entryoffsetinblock as i64) as Cdino
                    };
                    let mut i_ino = ino;
                    let mut altname = [0u8; NAME_MAX + 1];
                    let mut altlen = 0u16;
                    cd9660_rrip_getname(&rec, &mut altname, &mut altlen, &mut i_ino, imp);
                    dp.i_ino.set(i_ino);
                    let altlen = usize::from(altlen).min(altname.len());
                    if altlen as i64 == cnp.cn_namelen && name == &altname[..altlen] {
                        break 'scan Scan::Found(entryoffsetinblock);
                    }
                    ino = 0;
                } else if (isonum_711(rec.flags()) & 4 != 0) == assoc {
                    if (name.len() == 1 && name[0] == b'.') || flags & ISDOTDOT != 0 {
                        let want = if flags & ISDOTDOT != 0 { 1 } else { 0 };
                        if namelen == 1 && rec.name0() == want {
                            // Save directory entry's inode number and release directory
                            // buffer.
                            dp.i_ino.set(isodirino(&rec, imp));
                            break 'scan Scan::Found(entryoffsetinblock);
                        }
                        if namelen != 1 || rec.name0() != 0 {
                            break 'scan Scan::NotFound;
                        }
                    } else if isofncmp(name, &rec.name()[..namelen], imp.joliet_level) == 0 {
                        ino = if isonum_711(rec.flags()) & 2 != 0 {
                            isodirino(&rec, imp)
                        } else {
                            ((b.b_blkno.get() << DEV_BSHIFT) + entryoffsetinblock as i64) as Cdino
                        };
                        saveoffset = dp.i_offset.get();
                    } else if ino != 0 {
                        break 'scan Scan::FoundIno(entryoffsetinblock);
                    }
                    // NOSORTBUG (not defined): on some CDs directory entries are not sorted
                    // correctly, a res < 0 would go to notfound and a res > 0 on the second
                    // pass would add a third.
                }
                dp.i_offset.set(dp.i_offset.get() + reclen as Doff);
                entryoffsetinblock += reclen;
            }
            if ino != 0 {
                Scan::FoundIno(entryoffsetinblock)
            } else {
                Scan::NotFound
            }
        };

        match scan {
            Scan::Found(ep) => break 'searchloop ep,
            Scan::FoundIno(mut ep) => {
                dp.i_ino.set(ino);
                if saveoffset != dp.i_offset.get() {
                    if lblkno(imp, dp.i_offset.get() as Off) != lblkno(imp, saveoffset as Off) {
                        if let Some(b) = bp.take() {
                            brelse(b);
                        }
                        let (b, _) = cd9660_bufatoff(dp, saveoffset as Off)?;
                        bp = Some(b);
                    }
                    entryoffsetinblock = (saveoffset & bmask) as usize;
                    ep = entryoffsetinblock;
                    dp.i_offset.set(saveoffset);
                }
                break 'searchloop ep;
            }
            Scan::NotFound => {
                // If we started in the middle of the directory and failed to find our
                // target, we must check the beginning as well.
                if numdirpasses == 2 {
                    numdirpasses -= 1;
                    dp.i_offset.set(0);
                    endsearch = dp.i_diroff.get();
                    continue 'searchloop;
                }
                if let Some(b) = bp {
                    brelse(b);
                }

                // Insert name into cache (as non-existent) if appropriate.
                if cnp.cn_flags & MAKEENTRY != 0 {
                    cache_enter(vdp, *ap.a_vpp, cnp);
                }
                if nameiop == CREATE || nameiop == RENAME {
                    return Err(Errno::EJUSTRETURN);
                }
                return Err(Errno::ENOENT);
            }
        }
    };

    // found:
    if numdirpasses == 2 {
        ISO_NCHSTATS.ncs_pass2.fetch_add(1, Ordering::Relaxed);
    }

    // Found component in pathname. If the final component of path name, save information
    // in the cache as to where the entry was found.
    if flags & ISLASTCN != 0 && nameiop == LOOKUP {
        dp.i_diroff.set(dp.i_offset.get());
    }

    // Step through the translation in the name. We do not `iput' the directory because we
    // may need it again if a symbolic link is relative to the current directory. Instead we
    // save it unlocked as "pdp". We must get the target inode before unlocking the directory
    // to insure that the inode will not be removed before we get it. We prevent deadlock by
    // always fetching inodes from the root, moving down the directory tree. Thus when
    // following backward pointers ".." we must unlock the parent directory before getting
    // the requested directory. There is a potential race condition here if both the current
    // and parent directories are removed before the `iget' for the inode associated with
    // ".." returns. We hope that this occurs infrequently since we cannot avoid this race
    // condition without implementing a sophisticated deadlock detection algorithm. Note also
    // that this simple deadlock detection scheme will not work if the file system has any
    // hard links other than ".." that point backwards in the directory structure.
    let pdp = vdp;
    // If ino is different from dp->i_ino, it's a relocated directory.
    let relocated = dp.i_ino.get() != ino;
    if flags & ISDOTDOT != 0 {
        if let Some(b) = bp.take() {
            brelse(b);
        }
        let _ = VOP_UNLOCK(pdp); // race to get the inode
        cnp.cn_flags |= PDIRUNLOCK;
        let tdp = match cd9660_vget_internal(vmount(vdp), dp.i_ino.get(), relocated, None) {
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
        if let Some(b) = bp.take() {
            brelse(b);
        }
        vref(vdp); // we want ourself, ie "."
        *ap.a_vpp = Some(vdp);
    } else {
        let Some(b) = bp.take() else {
            panic(format_args!("cd9660_lookup: found without a buffer"));
        };
        let error = {
            // SAFETY: the buffer is ours (busy) and mapped; the view dies before it is
            // released below.
            let data: &[u8] = unsafe { b.data() };
            let rec = data.get(ep..).and_then(IsoDirectoryRecord::new);
            cd9660_vget_internal(vmount(vdp), dp.i_ino.get(), relocated, rec)
        };
        brelse(b);
        let tdp = error?;
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

/// `cd9660_bufatoff`: return buffer with the contents of block `offset` from the beginning
/// of directory `ip`, and the offset of `offset` in it (the C's `res`, a pointer to the
/// remaining space in the directory).
pub fn cd9660_bufatoff(ip: &IsoNode, offset: Off) -> Result<(&'static Buf, usize), Errno> {
    let vp = ip.itov();
    let imp = ip.mnt();
    let lbn = lblkno(imp, offset);
    let bsize = blksize(imp);

    let (bp, error) = bread(vp, lbn, bsize);
    if let Err(e) = error {
        brelse(bp);
        return Err(e);
    }
    Ok((bp, blkoff(imp, offset) as usize))
}
/* </CODE> */
