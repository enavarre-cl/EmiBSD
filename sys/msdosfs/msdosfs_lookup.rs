/*	$OpenBSD: msdosfs_lookup.c,v 1.35 2022/08/23 20:37:16 cheloha Exp $	*/
/*	$NetBSD: msdosfs_lookup.c,v 1.34 1997/10/18 22:12:27 ws Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (C) 1994, 1995, 1997 Wolfgang Solfrank.
 * Copyright (C) 1994, 1995, 1997 TooLs GmbH.
 * All rights reserved.
 * Original code by Paul Popelka (paulp@uts.amdahl.com) (see below).
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by TooLs GmbH.
 * 4. The name of TooLs GmbH may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY TOOLS GMBH ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL TOOLS GMBH BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/*
 * Written by Paul Popelka (paulp@uts.amdahl.com)
 *
 * You can do anything you want with this software, just don't say you wrote
 * it, and don't remove this notice.
 *
 * This software is provided "as is".
 *
 * The author supplies this software to be publicly redistributed on the
 * understanding that the author is not responsible for the correct
 * functioning of this software in any circumstances and is not liable for
 * any damages caused by this software.
 *
 * October 1992
 */
/* </LICENSES> */

/* <CODE> */
//! Directory operations of the msdos file system: `msdosfs_lookup` (find a name in a
//! directory, or the place a new entry for it would go), `createde` (write a directory entry,
//! with its Win95 long name entries), `dosdirempty`, `doscheckpath` (keep a rename from
//! moving a directory below itself), `readep`/`readde` (read the block of a directory entry),
//! `removede` (delete an entry and its long name entries) and `uniqdosname` (make a short
//! name no other entry has).
//!
//! Upstream: sys/msdosfs/msdosfs_lookup.c @ 3ce1f3f79392
//!
//! When a directory is searched, the blocks holding its entries are read and examined. The
//! entries hold what an inode holds in a unix file system, so part of a directory's contents
//! may also be in memory-resident denodes. A process that changes a directory must therefore
//! own the disk block of the entry first (`bread`/`brelse` give that exclusivity), then
//! update the block and the denode, then write the block, so that the blocks and the denodes
//! agree.
//!
//! ## Deviations
//! - A `struct direntry *` into a buffer is the entry's offset in the buffer's bytes, viewed
//!   with `Direntry::at`/`at_mut` (`Winentry` for a long name entry at the same offset);
//!   `ep--` subtracts the entry size. `removede` checks that it is not at the start of the
//!   block before it steps back, where the C steps back and then does not look at the entry.
//! - The directory search of `msdosfs_lookup` (up to its `found`/`foundroot`/`notfound`
//!   labels) is the private `msdosfs_lookup_search`, which returns where it ended; the
//!   function then does what the labels' code does. The host tests run the search alone,
//!   since getting a denode needs the vnode operations of `msdosfs_vnops.c`.
//! - `readep` and `readde` return the busy buffer and the offset of the entry in its data
//!   (the C's `bptoep`), where the C fills `*bpp` and `*epp`; on error the buffer is already
//!   released (the C sets `*bpp = NULL`).
//! - `createde` takes the caller's `depp` as `Option<&mut Option<&'static Denode>>` (the C's
//!   NULL is `None`) and sets it to `None` when `deget` fails, as `deget` NULLs it in the C.
//! - `dosdirempty` returns `bool` (the C's 1 and 0).
//! - `doscheckpath`'s `DIAGNOSTIC` check that both denodes are on one file system is behind
//!   feature `diagnostic`.
//! - The `MSDOSFS_DEBUG` `printf`s are left out: the option is not in GENERIC and has no
//!   feature here.

use crate::kern::subr_prf::printf;
use crate::kern::vfs_bio::{bread, brelse, bwrite};
use crate::kern::vfs_cache::{cache_enter, cache_lookup};
use crate::kern::vfs_subr::{vput, vref};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_UNLOCK};
use crate::msdosfs::bpb::getushort;
use crate::msdosfs::denode::{Denode, MSDOSFSROOT_OFS, de_externalize, vtode};
use crate::msdosfs::direntry::{
    ATTR_DIRECTORY, ATTR_VOLUME, ATTR_WIN95, Direntry, SLOT_DELETED, SLOT_EMPTY, Winentry,
};
use crate::msdosfs::fat::{DE_CLEAR, MSDOSFSROOT, fat32};
use crate::msdosfs::msdosfs_conv::{unix2dosfn, unix2winfn, winChkName, winChksum, winSlotCnt};
use crate::msdosfs::msdosfs_denode::{deget, detrunc};
use crate::msdosfs::msdosfs_fat::{extendfile, pcbmap};
use crate::msdosfs::msdosfsmount::{
    Msdosfsmount, bptoep, cntobn, de_blk, de_bn2off, de_clcount, de_cluster, de_cn2off, detobn,
};
use crate::sys::buf::Buf;
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::mount::{MSDOSFSMNT_NOWIN95, MSDOSFSMNT_SHORTNAME};
use crate::sys::namei::{
    CREATE, Componentname, DELETE, ISDOTDOT, ISLASTCN, LOCKPARENT, MAKEENTRY, PDIRUNLOCK, RENAME,
    SAVENAME, WANTPARENT,
};
use crate::sys::proc::Proc;
use crate::sys::types::Daddr;
use crate::sys::ucred::NOCRED;
use crate::sys::vnode::{VEXEC, VROOT, VWRITE, VopLookupArgs};

/// `sizeof(struct direntry)`.
const DIRENTRY_SIZE: u32 = Direntry::SIZE as u32;

/// The name of the `..` entry, padded as a DOS name.
const DOTDOT_NAME: [u8; 11] = *b"..         ";
/// The name of the `.` entry, padded as a DOS name.
const DOT_NAME: [u8; 11] = *b".          ";

/// Where the directory search of `msdosfs_lookup` ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Search {
    /// `found`/`foundroot`: the entry is a directory (`isadir`) starting at cluster `scn`;
    /// its denode is the one at (`cluster`, `blkoff`).
    Found {
        /// `isadir`: the entry is a directory.
        isadir: bool,
        /// `scn`: the entry's starting cluster.
        scn: u32,
        /// `cluster`: the directory cluster of the denode.
        cluster: u32,
        /// `blkoff`: the offset of the denode's entry.
        blkoff: u32,
    },
    /// `notfound`: the name is not in the directory; a new entry needing `wincnt` slots would
    /// start at `slotoffset` (the DOS entry is its last slot).
    NotFound {
        /// `slotoffset`: where the first of the new entry's slots would go.
        slotoffset: u32,
        /// `wincnt`: the slots the name needs, its long name entries and the DOS entry.
        wincnt: i32,
    },
}

/// The data of a buffer the caller owns.
///
/// # Safety
///
/// As `Buf::data`: the buffer is busy for the caller and mapped, and no other slice of it
/// is alive.
#[allow(clippy::mut_from_ref)] // the B_BUSY owner's view, as the C's b_data
unsafe fn bdata(bp: &Buf) -> &mut [u8] {
    // SAFETY: the caller's contract.
    unsafe { bp.data() }
}

/// `cnp->cn_proc`, `None` when the component name has no thread.
fn cn_proc(cnp: &Componentname) -> Option<&Proc> {
    (!cnp.cn_proc.is_null()).then(|| cnp.proc())
}

/// `msdosfs_lookup` (`vop_lookup`): look the last component of `a_cnp` up in the directory
/// `a_dvp`, which is locked.
///
/// Found, the entry's vnode is returned locked in `a_vpp`. Not found, a `CREATE` or `RENAME`
/// of the last component answers `EJUSTRETURN` and leaves in the directory's denode where the
/// new entry should go (`de_fndoffset`, `de_fndcnt`); anything else answers `ENOENT`. The
/// directory stays locked unless `PDIRUNLOCK` is set on return.
pub fn msdosfs_lookup(ap: &mut VopLookupArgs<'_>) -> Result<(), Errno> {
    let vdp = ap.a_dvp;
    let cnp = &mut *ap.a_cnp;
    let nameiop = cnp.cn_nameiop;

    cnp.cn_flags &= !PDIRUNLOCK; // XXX why this ??
    let flags = cnp.cn_flags;

    let dp = vtode(vdp);
    let pmp = dp.pmp();
    *ap.a_vpp = None;
    let lockparent = flags & LOCKPARENT != 0;
    let wantparent = flags & (LOCKPARENT | WANTPARENT) != 0;

    // Check accessibility of directory.
    if dp.de_Attributes.get() & ATTR_DIRECTORY == 0 {
        return Err(Errno::ENOTDIR);
    }
    VOP_ACCESS(vdp, VEXEC, cnp.cn_cred, cnp.proc())?;

    // We now have a segment name to search for, and a directory to search.
    //
    // Before tediously performing a linear scan of the directory, check the name cache to see
    // if the directory/name pair we are looking for is known already.
    if let Some(vp) = cache_lookup(vdp, cnp)? {
        *ap.a_vpp = Some(vp);
        return Ok(());
    }

    let vroot = vdp.v_flag.get() & VROOT != 0;
    let (isadir, mut scn, cluster, blkoff) =
        match msdosfs_lookup_search(dp, vroot, cnp, nameiop, flags)? {
            Search::Found {
                isadir,
                scn,
                cluster,
                blkoff,
            } => (isadir, scn, cluster, blkoff),
            Search::NotFound { slotoffset, wincnt } => {
                // We hold no disk buffers at this point.
                //
                // If we get here we didn't find the entry we were looking for. But that's ok
                // if we are creating or renaming and are at the end of the pathname and the
                // directory hasn't been removed.
                if (nameiop == CREATE || nameiop == RENAME)
                    && flags & ISLASTCN != 0
                    && dp.de_refcnt.get() != 0
                {
                    // Access for write is interpreted as allowing creation of files in the
                    // directory.
                    VOP_ACCESS(vdp, VWRITE, cnp.cn_cred, cnp.proc())?;
                    // Return an indication of where the new directory entry should be put.
                    dp.de_fndoffset.set(slotoffset);
                    dp.de_fndcnt.set(wincnt - 1);

                    // We return with the directory locked, so that the parameters we set up
                    // above will still be valid if we actually decide to do a direnter().
                    // We return ni_vp == NULL to indicate that the entry does not currently
                    // exist; we leave a pointer to the (locked) directory inode in
                    // ndp->ni_dvp. The pathname buffer is saved so that the name can be
                    // obtained later.
                    //
                    // NB - if the directory is unlocked, then this information cannot be
                    // used.
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
            }
        };

    // foundroot: isadir, scn, cluster and blkoff describe the entry; no buffer is held.
    if fat32(pmp) && scn == MSDOSFSROOT {
        scn = pmp.pm_rootdirblk.get();
    }

    // If deleting, and at end of pathname, return parameters which can be used to remove
    // file. If the wantparent flag isn't set, we return only the directory (in
    // ndp->ni_dvp), otherwise we go on and lock the inode, being careful with ".".
    if nameiop == DELETE && flags & ISLASTCN != 0 {
        // Don't allow deleting the root.
        if blkoff == MSDOSFSROOT_OFS {
            return Err(Errno::EROFS); // really? XXX
        }

        // Write access to directory required to delete files.
        VOP_ACCESS(vdp, VWRITE, cnp.cn_cred, cnp.proc())?;

        // Return pointer to current entry in dp->i_offset. Save directory inode pointer in
        // ndp->ni_dvp for dirremove().
        if dp.de_StartCluster.get() == scn && isadir {
            // "."
            vref(vdp);
            *ap.a_vpp = Some(vdp);
            return Ok(());
        }
        let tdp = deget(pmp, cluster, blkoff)?;
        *ap.a_vpp = Some(tdp.detov());
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
        if blkoff == MSDOSFSROOT_OFS {
            return Err(Errno::EROFS); // really? XXX
        }

        VOP_ACCESS(vdp, VWRITE, cnp.cn_cred, cnp.proc())?;

        // Careful about locking second inode. This can only occur if the target is ".".
        if dp.de_StartCluster.get() == scn && isadir {
            return Err(Errno::EISDIR);
        }

        let tdp = deget(pmp, cluster, blkoff)?;
        *ap.a_vpp = Some(tdp.detov());
        cnp.cn_flags |= SAVENAME;
        if !lockparent {
            let _ = VOP_UNLOCK(vdp);
        }
        return Ok(());
    }

    // Step through the translation in the name. We do not `vput' the directory because we
    // may need it again if a symbolic link is relative to the current directory. Instead we
    // save it unlocked as "pdp". We must get the target inode before unlocking the
    // directory to insure that the inode will not be removed before we get it. We prevent
    // deadlock by always fetching inodes from the root, moving down the directory tree. Thus
    // when following backward pointers ".." we must unlock the parent directory before
    // getting the requested directory. There is a potential race condition here if both the
    // current and parent directories are removed before the VFS_VGET for the inode
    // associated with ".." returns. We hope that this occurs infrequently since we cannot
    // avoid this race condition without implementing a sophisticated deadlock detection
    // algorithm. Note also that this simple deadlock detection scheme will not work if the
    // file system has any hard links other than ".." that point backwards in the directory
    // structure.
    let pdp = vdp;
    if flags & ISDOTDOT != 0 {
        let _ = VOP_UNLOCK(pdp); // race to get the inode
        cnp.cn_flags |= PDIRUNLOCK;
        let tdp = match deget(pmp, cluster, blkoff) {
            Ok(tdp) => tdp,
            Err(error) => {
                if vn_lock(pdp, LK_EXCLUSIVE | LK_RETRY).is_ok() {
                    cnp.cn_flags &= !PDIRUNLOCK;
                }
                return Err(error);
            }
        };
        if lockparent && flags & ISLASTCN != 0 {
            if let Err(error) = vn_lock(pdp, LK_EXCLUSIVE | LK_RETRY) {
                vput(tdp.detov());
                return Err(error);
            }
            cnp.cn_flags &= !PDIRUNLOCK;
        }
        *ap.a_vpp = Some(tdp.detov());
    } else if dp.de_StartCluster.get() == scn && isadir {
        vref(vdp); // we want ourself, ie "."
        *ap.a_vpp = Some(vdp);
    } else {
        let tdp = deget(pmp, cluster, blkoff)?;
        if !lockparent || flags & ISLASTCN == 0 {
            let _ = VOP_UNLOCK(pdp);
            cnp.cn_flags |= PDIRUNLOCK;
        }
        *ap.a_vpp = Some(tdp.detov());
    }

    // Insert name into cache if appropriate.
    if cnp.cn_flags & MAKEENTRY != 0 {
        cache_enter(vdp, *ap.a_vpp, cnp);
    }
    Ok(())
}

/// The directory search of `msdosfs_lookup`: look the name of `cnp` up in the directory
/// `dp` (the root directory when `vroot`), up to the C's `found`, `foundroot` and `notfound`
/// labels. A match sets `dp.de_fndoffset` and `dp.de_fndcnt` to the entry's offset and the
/// long name entries a rename may reuse.
fn msdosfs_lookup_search(
    dp: &Denode,
    vroot: bool,
    cnp: &Componentname,
    nameiop: u64,
    flags: u64,
) -> Result<Search, Errno> {
    let pmp = dp.pmp();
    let name = cnp.name();

    // If they are going after the . or .. entry in the root directory, they won't find it.
    // DOS filesystems don't have them in the root directory. So, we fake it. deget() is in on
    // this scam too.
    if vroot && name.first() == Some(&b'.') && (name.len() == 1 || name == b"..") {
        return Ok(Search::Found {
            isadir: true,
            scn: MSDOSFSROOT,
            cluster: MSDOSFSROOT,
            blkoff: MSDOSFSROOT_OFS,
        });
    }

    let mut dosfilename = [0u8; 11];
    let mut wincnt = 1;
    let mut olddos = true;
    match unix2dosfn(name, &mut dosfilename, 0) {
        0 => return Err(Errno::EINVAL),
        2 => wincnt = winSlotCnt(name) + 1,
        3 => {
            olddos = false;
            wincnt = winSlotCnt(name) + 1;
        }
        _ => {}
    }
    let shortname = pmp.pm_flags.get() & MSDOSFSMNT_SHORTNAME as u32 != 0;
    if shortname {
        wincnt = 1;
    }

    // Suppress search for slots unless creating file and at end of pathname, in which case
    // we watch for a place to put the new file in case it doesn't already exist.
    let mut slotcount = wincnt;
    if (nameiop == CREATE || nameiop == RENAME) && flags & ISLASTCN != 0 {
        slotcount = 0;
    }
    let mut slotoffset: u32 = 0;

    // XXX UNIX allows filenames with trailing dots and blanks; we don't. Most of the
    // routines in msdosfs_conv.c adjust for this, but winChkName() does not, so we do it
    // here. Otherwise, a file such as ".foobar." cannot be retrieved properly.
    //
    // (Note that this is also faster: perform the adjustment once, rather than on each call
    // to winChkName. However, it is still a nasty hack.)
    let adjlen = name
        .iter()
        .rposition(|&c| c != b' ' && c != b'.')
        .map_or(0, |i| i + 1);
    let adj = &name[..adjlen];

    // The outer loop ranges over the clusters that make up the directory. Note that the root
    // directory is different from all other directories. It has a fixed number of blocks
    // that are not part of the pool of allocatable clusters. So, we treat it a little
    // differently. The root directory starts at "cluster" 0.
    let mut chksum: i32 = -1;
    let mut diroff: u32 = 0;
    let mut frcn: u32 = 0;
    'notfound: loop {
        let mut bn: Daddr = 0;
        let mut cluster: u32 = 0;
        let mut blsize: i32 = 0;
        match pcbmap(
            dp,
            frcn,
            Some(&mut bn),
            Some(&mut cluster),
            Some(&mut blsize),
        ) {
            Ok(()) => {}
            Err(Errno::E2BIG) => break 'notfound,
            Err(error) => return Err(error),
        }
        let (bp, error) = bread(pmp.devvp(), bn, blsize);
        if let Err(error) = error {
            brelse(bp);
            return Err(error);
        }
        // SAFETY: the buffer is busy for this function (from `bread`) and mapped; the slice
        // is not used after the buffer is released below.
        let data = unsafe { bdata(bp) };
        let mut blkoff: u32 = 0;
        while blkoff < blsize as u32 {
            let dep = Direntry::at(data, blkoff as usize);
            // If the slot is empty and we are still looking for an empty then remember this
            // one. If the slot is not empty then check to see if it matches what we are
            // looking for. If the slot has never been filled with anything, then the
            // remainder of the directory has never been used, so there is no point in
            // searching it.
            if dep.deName[0] == SLOT_EMPTY || dep.deName[0] == SLOT_DELETED {
                // Drop memory of previous long matches
                chksum = -1;

                if slotcount < wincnt {
                    slotcount += 1;
                    slotoffset = diroff;
                }
                if dep.deName[0] == SLOT_EMPTY {
                    brelse(bp);
                    break 'notfound;
                }
            } else {
                // If there wasn't enough space for our winentries, forget about the empty
                // space
                if slotcount < wincnt {
                    slotcount = 0;
                }

                if dep.deAttributes == ATTR_WIN95 {
                    // Check for Win95 long filename entry
                    if !shortname {
                        chksum = winChkName(adj, Winentry::at(data, blkoff as usize), chksum);
                    }
                } else if dep.deAttributes & ATTR_VOLUME != 0 {
                    // Ignore volume labels (anywhere, not just the root directory).
                    chksum = -1;
                } else {
                    // Check for a checksum or name match
                    let name11 = dep.name11();
                    let chksum_ok = chksum == i32::from(winChksum(&name11));
                    if !chksum_ok && (!olddos || dosfilename != name11) {
                        chksum = -1;
                    } else {
                        // Remember where this directory entry came from for whoever did this
                        // lookup.
                        dp.de_fndoffset.set(diroff);
                        if chksum_ok && nameiop == RENAME {
                            // Target had correct long name directory entries, reuse them as
                            // needed.
                            dp.de_fndcnt.set(wincnt - 1);
                        } else {
                            // Long name directory entries not present or corrupt, can only
                            // reuse dos directory entry.
                            dp.de_fndcnt.set(0);
                        }

                        // found: we still have the buffer with the matched directory entry.
                        let isadir = dep.deAttributes & ATTR_DIRECTORY != 0;
                        let mut scn = u32::from(getushort(&dep.deStartCluster));
                        if fat32(pmp) {
                            scn |= u32::from(getushort(&dep.deHighClust)) << 16;
                            if scn == pmp.pm_rootdirblk.get() {
                                // There should actually be 0 here. Just ignore the error.
                                scn = MSDOSFSROOT;
                            }
                        }

                        let mut cluster = cluster;
                        let mut blkoff = blkoff;
                        if cluster == MSDOSFSROOT {
                            blkoff = diroff;
                        }

                        if isadir {
                            cluster = scn;
                            blkoff = if cluster == MSDOSFSROOT {
                                MSDOSFSROOT_OFS
                            } else {
                                0
                            };
                        }

                        // Now release buf to allow deget to read the entry again. Reserving
                        // it here and giving it to deget could result in a deadlock.
                        brelse(bp);
                        return Ok(Search::Found {
                            isadir,
                            scn,
                            cluster,
                            blkoff,
                        });
                    }
                }
            }
            blkoff += DIRENTRY_SIZE;
            diroff += DIRENTRY_SIZE;
        }
        // Release the buffer holding the directory cluster just searched.
        brelse(bp);
        frcn += 1;
    }

    // notfound: fixup the slot description to point to the place where we might put the new
    // DOS direntry (putting the Win95 long name entries before that)
    if slotcount == 0 {
        slotcount = 1;
        slotoffset = diroff;
    }
    if wincnt > slotcount {
        slotoffset += DIRENTRY_SIZE * (wincnt - slotcount) as u32;
    }
    Ok(Search::NotFound { slotoffset, wincnt })
}

/// `createde(dep, ddep, depp, cnp)`: write the directory entry of `dep` (a template, often
/// the caller's stack denode) into the directory `ddep` at `ddep.de_fndoffset`, preceded by
/// `ddep.de_fndcnt` Win95 long name entries for `cnp`'s name, which `msdosfs_lookup` found
/// room for. When `depp` is given, the new entry's denode is returned there, locked.
pub fn createde(
    dep: &Denode,
    ddep: &'static Denode,
    depp: Option<&mut Option<&'static Denode>>,
    cnp: &Componentname,
) -> Result<(), Errno> {
    let pmp = ddep.pmp();

    // If no space left in the directory then allocate another cluster and chain it onto the
    // end of the file. There is one exception to this. That is, if the root directory has no
    // more space it can NOT be expanded. extendfile() checks for and fails attempts to extend
    // the root directory. We just return an error in that case.
    if ddep.de_fndoffset.get() >= ddep.de_FileSize.get() {
        let diroffset = ddep
            .de_fndoffset
            .get()
            .wrapping_add(DIRENTRY_SIZE)
            .wrapping_sub(ddep.de_FileSize.get());
        let dirclust = de_clcount(pmp, diroffset);
        if let Err(error) = extendfile(ddep, dirclust, None, None, DE_CLEAR) {
            let _ = detrunc(ddep, ddep.de_FileSize.get(), 0, NOCRED, cn_proc(cnp));
            return Err(error);
        }

        // Update the size of the directory
        ddep.de_FileSize.set(
            ddep.de_FileSize
                .get()
                .wrapping_add(de_cn2off(pmp, dirclust)),
        );
    }

    // We just read in the cluster with space. Copy the new directory entry in. Then write it
    // to disk. NOTE: DOS directories do not get smaller as clusters are emptied.
    let mut bn: Daddr = 0;
    let mut dirclust: u32 = 0;
    let mut blsize: i32 = 0;
    pcbmap(
        ddep,
        de_cluster(pmp, ddep.de_fndoffset.get()),
        Some(&mut bn),
        Some(&mut dirclust),
        Some(&mut blsize),
    )?;
    let mut diroffset = ddep.de_fndoffset.get();
    if dirclust != MSDOSFSROOT {
        diroffset &= pmp.pm_crbomask.get();
    }
    let (mut bp, error) = bread(pmp.devvp(), bn, blsize);
    if let Err(error) = error {
        brelse(bp);
        return Err(error);
    }
    let mut ndep = bptoep(pmp, ddep.de_fndoffset.get());

    // SAFETY: the buffer is busy for this function (from `bread`) and mapped; the slice ends
    // with the statement.
    de_externalize(Direntry::at_mut(unsafe { bdata(bp) }, ndep), dep);

    // Now write the Win95 long name
    if ddep.de_fndcnt.get() > 0 {
        // SAFETY: as above.
        let chksum = winChksum(&Direntry::at(unsafe { bdata(bp) }, ndep).name11());
        let un = cnp.name();
        let mut cnt = 1;

        loop {
            ddep.de_fndcnt.set(ddep.de_fndcnt.get() - 1);
            if ddep.de_fndcnt.get() < 0 {
                break;
            }
            if ddep.de_fndoffset.get() & pmp.pm_crbomask.get() == 0 {
                bwrite(bp)?;

                ddep.de_fndoffset
                    .set(ddep.de_fndoffset.get().wrapping_sub(DIRENTRY_SIZE));
                pcbmap(
                    ddep,
                    de_cluster(pmp, ddep.de_fndoffset.get()),
                    Some(&mut bn),
                    None,
                    Some(&mut blsize),
                )?;

                let (b, error) = bread(pmp.devvp(), bn, blsize);
                if let Err(error) = error {
                    brelse(b);
                    return Err(error);
                }
                bp = b;
                ndep = bptoep(pmp, ddep.de_fndoffset.get());
            } else {
                ndep -= Direntry::SIZE;
                ddep.de_fndoffset
                    .set(ddep.de_fndoffset.get().wrapping_sub(DIRENTRY_SIZE));
            }
            // SAFETY: as above.
            let wep = Winentry::at_mut(unsafe { bdata(bp) }, ndep);
            let more = unix2winfn(un, wep, cnt, chksum);
            cnt += 1;
            if more == 0 {
                break;
            }
        }
    }

    bwrite(bp)?;

    // If they want us to return with the denode gotten.
    if let Some(depp) = depp {
        if dep.de_Attributes.get() & ATTR_DIRECTORY != 0 {
            dirclust = dep.de_StartCluster.get();
            if fat32(pmp) && dirclust == pmp.pm_rootdirblk.get() {
                dirclust = MSDOSFSROOT;
            }
            diroffset = if dirclust == MSDOSFSROOT {
                MSDOSFSROOT_OFS
            } else {
                0
            };
        }
        return match deget(pmp, dirclust, diroffset) {
            Ok(ndep) => {
                *depp = Some(ndep);
                Ok(())
            }
            Err(error) => {
                *depp = None;
                Err(error)
            }
        };
    }

    Ok(())
}

/// `dosdirempty(dep)`: be sure a directory is empty except for "." and "..". `true` if
/// empty, `false` if not empty or on error.
pub fn dosdirempty(dep: &Denode) -> bool {
    let pmp = dep.pmp();

    // Since the filesize field in directory entries for a directory is zero, we just have to
    // feel our way through the directory until we hit end of file.
    let mut cn: u32 = 0;
    loop {
        let mut bn: Daddr = 0;
        let mut blsize: i32 = 0;
        match pcbmap(dep, cn, Some(&mut bn), None, Some(&mut blsize)) {
            Ok(()) => {}
            Err(Errno::E2BIG) => return true, // it's empty
            Err(_) => return false,
        }
        let (bp, error) = bread(pmp.devvp(), bn, blsize);
        if error.is_err() {
            brelse(bp);
            return false;
        }
        // SAFETY: the buffer is busy for this function (from `bread`) and mapped; the slice
        // is not used after the buffer is released.
        let data = unsafe { bdata(bp) };
        for dentp in data[..blsize as usize].as_chunks::<{ Direntry::SIZE }>().0 {
            let dentp = Direntry::at(dentp, 0);
            if dentp.deName[0] != SLOT_DELETED && dentp.deAttributes & ATTR_VOLUME == 0 {
                // In dos directories an entry whose name starts with SLOT_EMPTY (0) starts the
                // beginning of the unused part of the directory, so we can just return that
                // it is empty.
                if dentp.deName[0] == SLOT_EMPTY {
                    brelse(bp);
                    return true;
                }
                // Any names other than "." and ".." in a directory mean it is not empty.
                let name11 = dentp.name11();
                if name11 != DOT_NAME && name11 != DOTDOT_NAME {
                    brelse(bp);
                    return false; // not empty
                }
            }
        }
        brelse(bp);
        cn += 1;
    }
}

/// `doscheckpath(source, target)`: check to see if the directory described by `target` is in
/// some subdirectory of `source`. This prevents something like the following from
/// succeeding and leaving a bunch or files and directories orphaned: `mv /a/b/c
/// /a/b/c/d/e/f`, where c and f are directories (`source` is the denode of /a/b/c, `target`
/// that of /a/b/c/d/e/f).
///
/// `Ok` if `target` is NOT a subdirectory of `source`, an error otherwise. The target's
/// vnode is always released (`vput`) on return.
pub fn doscheckpath(source: &Denode, target: &'static Denode) -> Result<(), Errno> {
    let mut dep: Option<&'static Denode> = Some(target);
    let mut bp: Option<&'static Buf> = None;

    let error: Result<(), Errno> = 'out: {
        if target.de_Attributes.get() & ATTR_DIRECTORY == 0
            || source.de_Attributes.get() & ATTR_DIRECTORY == 0
        {
            break 'out Err(Errno::ENOTDIR);
        }
        if target.de_StartCluster.get() == source.de_StartCluster.get() {
            break 'out Err(Errno::EEXIST);
        }
        if target.de_StartCluster.get() == MSDOSFSROOT {
            break 'out Ok(());
        }
        let pmp = target.pmp();
        #[cfg(feature = "diagnostic")]
        if !core::ptr::eq(pmp, source.pmp()) {
            crate::kern::subr_prf::panic(format_args!(
                "doscheckpath: source and target on different filesystems"
            ));
        }
        if fat32(pmp) && target.de_StartCluster.get() == pmp.pm_rootdirblk.get() {
            break 'out Ok(());
        }

        loop {
            let Some(d) = dep else {
                break 'out Ok(());
            };
            if d.de_Attributes.get() & ATTR_DIRECTORY == 0 {
                break 'out Err(Errno::ENOTDIR);
            }
            let scn = d.de_StartCluster.get();
            let (b, error) = bread(
                pmp.devvp(),
                Daddr::from(cntobn(pmp, scn)),
                pmp.pm_bpcluster.get() as i32,
            );
            bp = Some(b);
            if let Err(error) = error {
                break 'out Err(error);
            }

            // SAFETY: the buffer is busy for this function (from `bread`) and mapped; the
            // slice ends with the statement.
            let ep = *Direntry::at(unsafe { bdata(b) }, Direntry::SIZE);
            if ep.deAttributes & ATTR_DIRECTORY == 0 || ep.name11() != DOTDOT_NAME {
                break 'out Err(Errno::ENOTDIR);
            }
            let mut scn = u32::from(getushort(&ep.deStartCluster));
            if fat32(pmp) {
                scn |= u32::from(getushort(&ep.deHighClust)) << 16;
            }

            if scn == source.de_StartCluster.get() {
                break 'out Err(Errno::EINVAL);
            }
            if scn == MSDOSFSROOT {
                break 'out Ok(());
            }
            if fat32(pmp) && scn == pmp.pm_rootdirblk.get() {
                // scn should be 0 in this case, but we silently ignore the error.
                break 'out Ok(());
            }

            vput(d.detov());
            brelse(b);
            bp = None;
            // NOTE: deget() clears dep on error
            dep = None;
            match deget(pmp, scn, 0) {
                Ok(n) => dep = Some(n),
                Err(error) => break 'out Err(error),
            }
        }
    };

    // out:
    if let Some(b) = bp {
        brelse(b);
    }
    if error == Err(Errno::ENOTDIR) {
        printf(format_args!("doscheckpath(): .. not a directory?\n"));
    }
    if let Some(d) = dep {
        vput(d.detov());
    }
    error
}

/// `readep(pmp, dirclust, diroffset, bpp, epp)`: read in the disk block containing the
/// directory entry (`dirclust`, `diroffset`) and return the buf header, busy, and the offset
/// of the directory entry within the block.
pub fn readep(
    pmp: &Msdosfsmount,
    dirclust: u32,
    diroffset: u32,
) -> Result<(&'static Buf, usize), Errno> {
    let mut blsize = pmp.pm_bpcluster.get();
    if dirclust == MSDOSFSROOT
        && de_blk(pmp, diroffset.wrapping_add(blsize)) > pmp.pm_rootdirsize.get()
    {
        blsize = de_bn2off(pmp, pmp.pm_rootdirsize.get()) & pmp.pm_crbomask.get();
    }
    let bn = detobn(pmp, dirclust, diroffset);
    let (bp, error) = bread(pmp.devvp(), Daddr::from(bn), blsize as i32);
    if let Err(error) = error {
        brelse(bp);
        return Err(error);
    }
    Ok((bp, bptoep(pmp, diroffset)))
}

/// `readde(dep, bpp, epp)`: read in the disk block containing the directory entry `dep` came
/// from and return the buf header, busy, and the offset of the directory entry within the
/// block.
pub fn readde(dep: &Denode) -> Result<(&'static Buf, usize), Errno> {
    readep(dep.pmp(), dep.de_dirclust.get(), dep.de_diroffset.get())
}

/// `removede(pdep, dep)`: remove a directory entry, the one at `pdep.de_fndoffset` in the
/// directory `pdep` (with the Win95 long name entries before it), which describes the file
/// `dep`.
///
/// At this point the file represented by the directory entry to be removed is still full
/// length until noone has it open. When the file no longer being used `msdosfs_inactive()`
/// is called and will truncate the file to 0 length. When the vnode containing the denode is
/// needed for some other purpose by VFS it will call `msdosfs_reclaim()` which will remove
/// the denode from the denode cache.
pub fn removede(pdep: &Denode, dep: &Denode) -> Result<(), Errno> {
    let pmp = pdep.pmp();
    let nowin95 = pmp.pm_flags.get() & MSDOSFSMNT_NOWIN95 as u32 != 0;
    let crbomask = pmp.pm_crbomask.get();
    let mut offset = pdep.de_fndoffset.get();

    dep.de_refcnt.set(dep.de_refcnt.get() - 1);
    offset = offset.wrapping_add(DIRENTRY_SIZE);
    loop {
        offset = offset.wrapping_sub(DIRENTRY_SIZE);
        let mut bn: Daddr = 0;
        let mut blsize: i32 = 0;
        pcbmap(
            pdep,
            de_cluster(pmp, offset),
            Some(&mut bn),
            None,
            Some(&mut blsize),
        )?;
        let (bp, error) = bread(pmp.devvp(), bn, blsize);
        if let Err(error) = error {
            brelse(bp);
            return Err(error);
        }
        // SAFETY: the buffer is busy for this function (from `bread`) and mapped; the slice
        // is not used after the buffer is written or released.
        let data = unsafe { bdata(bp) };
        let mut ep = bptoep(pmp, offset);
        // Check whether, if we came here the second time, i.e. when underflowing into the
        // previous block, the last entry in this block is a longfilename entry, too.
        if Direntry::at(data, ep).deAttributes != ATTR_WIN95 && offset != pdep.de_fndoffset.get() {
            brelse(bp);
            break;
        }
        offset = offset.wrapping_add(DIRENTRY_SIZE);
        loop {
            // We are a bit aggressive here in that we delete any Win95 entries preceding
            // this entry, not just the ones we "own". Since these presumably aren't valid
            // anyway, there should be no harm.
            offset = offset.wrapping_sub(DIRENTRY_SIZE);
            Direntry::at_mut(data, ep).deName[0] = SLOT_DELETED;
            if nowin95 || offset & crbomask == 0 {
                break;
            }
            ep -= Direntry::SIZE;
            if Direntry::at(data, ep).deAttributes != ATTR_WIN95 {
                break;
            }
        }
        bwrite(bp)?;
        if nowin95 || offset & crbomask != 0 || offset == 0 {
            break;
        }
    }
    Ok(())
}

/// `uniqdosname(dep, cnp, cp)`: create in `cp` a DOS name for `cnp`'s name that no entry of
/// the directory `dep` has, trying the generation numbers 1, 2, ... in turn.
pub fn uniqdosname(dep: &Denode, cnp: &Componentname, cp: &mut [u8; 11]) -> Result<(), Errno> {
    let pmp = dep.pmp();

    let mut r#gen: u32 = 1;
    loop {
        // Generate DOS name with generation number
        if unix2dosfn(cnp.name(), cp, r#gen) == 0 {
            return Err(if r#gen == 1 {
                Errno::EINVAL
            } else {
                Errno::EEXIST
            });
        }

        // Now look for a dir entry with this exact name
        let mut cn: u32 = 0;
        loop {
            let mut bn: Daddr = 0;
            let mut blsize: i32 = 0;
            match pcbmap(dep, cn, Some(&mut bn), None, Some(&mut blsize)) {
                Ok(()) => {}
                Err(Errno::E2BIG) => return Ok(()), // EOF reached and not found
                Err(error) => return Err(error),
            }
            let (bp, error) = bread(pmp.devvp(), bn, blsize);
            if let Err(error) = error {
                brelse(bp);
                return Err(error);
            }
            // SAFETY: the buffer is busy for this function (from `bread`) and mapped; the
            // slice is not used after the buffer is released.
            let data = unsafe { bdata(bp) };
            let mut exists = false;
            for dentp in data[..blsize as usize].as_chunks::<{ Direntry::SIZE }>().0 {
                let dentp = Direntry::at(dentp, 0);
                if dentp.deName[0] == SLOT_EMPTY {
                    // Last used entry and not found
                    brelse(bp);
                    return Ok(());
                }
                // Ignore volume labels and Win95 entries
                if dentp.deAttributes & ATTR_VOLUME != 0 {
                    continue;
                }
                if dentp.name11() == *cp {
                    exists = true;
                    break;
                }
            }
            brelse(bp);
            if exists {
                // EEXIST: try the next generation number.
                break;
            }
            cn += 1;
        }
        r#gen += 1;
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of `msdosfs_lookup.rs` over a FAT image mounted on the fake disk of
    // `msdosfs_vfsops.rs`'s tests: the directory search (short names, Win95 long names, volume
    // labels, the room found for new entries), `readep`/`readde`, `uniqdosname`, `createde`
    // and `removede` (within a block, across a block boundary, and growing a full directory),
    // `dosdirempty`, and the paths of `msdosfs_lookup` that need no new denode.
    //
    // Getting a denode (`deget`) locks its new vnode, which needs the vnode operations of
    // `msdosfs_vnops.c`; the directories here are denodes made by hand, as `deget` makes them.

    use std::boxed::Box;
    use std::vec::Vec;

    use super::*;
    use crate::kern::subr_xxx::nullop;
    use crate::kern::vfs_subr::{getnewvnode, vrele};
    use crate::msdosfs::direntry::ATTR_ARCHIVE;
    use crate::msdosfs::fat::CLUST_END;
    use crate::msdosfs::msdosfs_fat::fc_purge;
    use crate::msdosfs::msdosfs_vfsops::msdosfs_sync;
    use crate::msdosfs::msdosfs_vfsops::tests::mkfat::{self, Dir, Image};
    use crate::msdosfs::msdosfs_vfsops::tests::{DISK, mount, setup, teardown, unmount};
    use crate::msdosfs::msdosfsmount::vfstomsdosfs;
    use crate::sys::mount::{MNT_WAIT, Mount};
    use crate::sys::namei::LOOKUP;
    use crate::sys::vnode::{VDIR, VT_MSDOSFS, Vnode, VopInactiveArgs, VopReclaimArgs, Vops};

    /// The name of a 2-part long name entry file in the sample image.
    const LONG2: &[u8] = b"a rather long name.txt";

    /// The DOS name `unix2dosfn` makes of `name` with generation number `gen`.
    fn dosname(name: &[u8], r#gen: u32) -> [u8; 11] {
        let mut dn = [0u8; 11];
        assert_ne!(unix2dosfn(name, &mut dn, r#gen), 0);
        dn
    }

    /// A short entry of a file with no data.
    fn empty_file(short: &[u8; 11]) -> [u8; 32] {
        mkfat::short_entry(short, ATTR_ARCHIVE, 0, 0)
    }

    /// The sample root directory, FAT12, 512-byte blocks (16 entries each):
    ///
    /// | offset | entry |
    /// |---|---|
    /// | 0 | volume label `EMIBSD` |
    /// | 32 | `README.TXT` (short name only) |
    /// | 64, 96 | `m10c-fat.txt`: one long name entry, short `M10C-FATTXT` |
    /// | 128 | directory `SUBDIR` |
    /// | 160, 192, 224 | `LONG2`: two long name entries, short `ARATHE~1TXT` |
    /// | 256, 288, 320 | deleted |
    /// | 352 | `AFTER.TXT` |
    /// | 384... | never used |
    fn sample() -> (Vec<u8>, u32) {
        let mut img = Image::new(mkfat::FAT12_1M);
        let mut label = mkfat::short_entry(b"EMIBSD     ", ATTR_VOLUME, 0, 0);
        label[24..26].fill(0);
        img.put(Dir::Root, &[label]);
        assert_eq!(
            img.add_file(Dir::Root, None, b"README  TXT", b"read me\n"),
            32
        );
        assert_eq!(
            img.add_file(Dir::Root, Some(b"m10c-fat.txt"), b"M10C-FATTXT", b"fat\n"),
            96
        );
        let sub = img.mkdir(Dir::Root, b"SUBDIR     ", 1);
        assert_eq!(
            img.add_file(Dir::Root, Some(LONG2), &dosname(LONG2, 1), b"long\n"),
            224
        );
        let mut deleted = empty_file(b"GONE    TXT");
        deleted[0] = SLOT_DELETED;
        img.put(Dir::Root, &[deleted, deleted, deleted]);
        img.put(Dir::Root, &[empty_file(b"AFTER   TXT")]);
        (img.finish(), sub)
    }

    /// The denode of the directory starting at cluster `start` (the root directory for
    /// `MSDOSFSROOT`), filled in as `deget` fills it, without a vnode.
    fn dirnode(pmp: &'static Msdosfsmount, start: u32) -> &'static Denode {
        let dep: &'static Denode = Box::leak(Box::new(Denode::new()));
        dep.de_pmp.set(Some(pmp));
        dep.de_Attributes.set(ATTR_DIRECTORY);
        dep.de_StartCluster.set(start);
        dep.de_refcnt.set(1);
        fc_purge(dep, 0);
        if start == MSDOSFSROOT {
            dep.de_FileSize
                .set(pmp.pm_rootdirsize.get() * u32::from(pmp.pm_BytesPerSec()));
        } else {
            let mut size = 0;
            assert_eq!(
                pcbmap(dep, CLUST_END, None, Some(&mut size), None),
                Err(Errno::E2BIG)
            );
            dep.de_FileSize.set(de_cn2off(pmp, size));
        }
        dep
    }

    /// A component name for `name`.
    fn cn(p: &Proc, name: &'static [u8], nameiop: u64, flags: u64) -> Componentname {
        let mut cnp = Componentname::new();
        cnp.cn_nameiop = nameiop;
        cnp.cn_flags = flags;
        cnp.cn_proc = p;
        cnp.cn_cred = p.p_ucred.get();
        cnp.cn_nameptr = name.as_ptr();
        cnp.cn_namelen = name.len() as i64;
        cnp
    }

    /// `msdosfs_lookup_search` for `name` in `dp`.
    fn search(
        p: &Proc,
        dp: &Denode,
        name: &'static [u8],
        nameiop: u64,
        flags: u64,
    ) -> Result<Search, Errno> {
        let cnp = cn(p, name, nameiop, flags);
        msdosfs_lookup_search(
            dp,
            dp.de_StartCluster.get() == MSDOSFSROOT,
            &cnp,
            nameiop,
            flags,
        )
    }

    /// The `n`th 32-byte entry of the root directory on the disk.
    fn root_entry(off: u32) -> [u8; 32] {
        let img = Image::new(mkfat::FAT12_1M);
        let o = img.root_off() + off as usize;
        DISK.lock().unwrap()[o..o + 32].try_into().unwrap()
    }

    /// The mount and its root directory's denode.
    fn mounted(p: &'static Proc) -> (&'static Mount, &'static Msdosfsmount, &'static Denode) {
        let mp = mount(p, false);
        let pmp = vfstomsdosfs(mp);
        (mp, pmp, dirnode(pmp, MSDOSFSROOT))
    }

    #[test]
    fn search_finds_short_and_long_names() {
        let (disk, sub) = sample();
        let readme_clust = {
            let e = &disk[Image::new(mkfat::FAT12_1M).root_off() + 32..][..32];
            u32::from(u16::from_le_bytes([e[26], e[27]]))
        };
        let (_g, p) = setup(disk);
        let (mp, _pmp, root) = mounted(p);

        let file = |blkoff, scn| Search::Found {
            isadir: false,
            scn,
            cluster: MSDOSFSROOT,
            blkoff,
        };
        // A short name, in any case.
        assert_eq!(
            search(p, root, b"README.TXT", LOOKUP, ISLASTCN),
            Ok(file(32, readme_clust))
        );
        assert_eq!(root.de_fndoffset.get(), 32);
        assert_eq!(root.de_fndcnt.get(), 0);
        assert_eq!(
            search(p, root, b"readme.txt", LOOKUP, ISLASTCN),
            Ok(file(32, readme_clust))
        );
        // A long name, by its long name entry or its short name; a rename may reuse the long
        // name entries found with a good checksum.
        let Ok(Search::Found { blkoff, .. }) = search(p, root, b"m10c-fat.txt", RENAME, ISLASTCN)
        else {
            panic!("m10c-fat.txt not found");
        };
        assert_eq!(blkoff, 96);
        assert_eq!(root.de_fndcnt.get(), 1);
        let Ok(Search::Found { blkoff, .. }) = search(p, root, b"M10C-FAT.TXT", LOOKUP, 0) else {
            panic!("M10C-FAT.TXT not found");
        };
        assert_eq!(blkoff, 96);
        // Trailing dots and blanks do not count.
        let Ok(Search::Found { blkoff, .. }) = search(p, root, b"m10c-fat.txt. ", LOOKUP, 0) else {
            panic!("m10c-fat.txt. not found");
        };
        assert_eq!(blkoff, 96);
        // A long name whose short name is a generated one is found only by its long name.
        let Ok(Search::Found { blkoff, .. }) = search(p, root, LONG2, LOOKUP, 0) else {
            panic!("{LONG2:?} not found");
        };
        assert_eq!(blkoff, 224);
        assert_eq!(root.de_fndoffset.get(), 224);
        assert!(matches!(
            search(p, root, b"a rather long name.tx", LOOKUP, 0),
            Ok(Search::NotFound { .. })
        ));
        // A directory: its own "." entry is its denode.
        assert_eq!(
            search(p, root, b"subdir", LOOKUP, 0),
            Ok(Search::Found {
                isadir: true,
                scn: sub,
                cluster: sub,
                blkoff: 0
            })
        );
        // The volume label is not a file.
        assert!(matches!(
            search(p, root, b"EMIBSD", LOOKUP, 0),
            Ok(Search::NotFound { .. })
        ));
        assert!(matches!(
            search(p, root, b"nothere", LOOKUP, 0),
            Ok(Search::NotFound { .. })
        ));
        // "." and ".." of the root directory are faked.
        let root_found = Search::Found {
            isadir: true,
            scn: MSDOSFSROOT,
            cluster: MSDOSFSROOT,
            blkoff: MSDOSFSROOT_OFS,
        };
        assert_eq!(search(p, root, b".", LOOKUP, 0), Ok(root_found));
        assert_eq!(search(p, root, b"..", LOOKUP, ISDOTDOT), Ok(root_found));
        // In a subdirectory they are real entries; ".." of a child of the root is the root.
        let subdp = dirnode(_pmp, sub);
        assert_eq!(
            search(p, subdp, b"..", LOOKUP, ISDOTDOT),
            Ok(Search::Found {
                isadir: true,
                scn: MSDOSFSROOT,
                cluster: MSDOSFSROOT,
                blkoff: MSDOSFSROOT_OFS
            })
        );
        // A name made of dots and blanks has no DOS name.
        assert_eq!(search(p, root, b"...", LOOKUP, 0), Err(Errno::EINVAL));

        unmount(p, mp);
        teardown();
    }

    #[test]
    fn search_finds_room_for_new_entries() {
        let (disk, _) = sample();
        let (_g, p) = setup(disk);
        let (mp, pmp, root) = mounted(p);
        let create = |name: &'static [u8]| search(p, root, name, CREATE, ISLASTCN);

        // One long name entry and the DOS entry: the last two deleted slots.
        assert_eq!(
            create(b"x.txt"),
            Ok(Search::NotFound {
                slotoffset: 288,
                wincnt: 2
            })
        );
        // Three slots: the whole run of deleted ones.
        assert_eq!(
            create(b"a much longer name.txt"),
            Ok(Search::NotFound {
                slotoffset: 320,
                wincnt: 3
            })
        );
        // Four slots do not fit there: the first never used slot and the three after it, the
        // DOS entry last.
        assert_eq!(
            create(b"a name that needs four slots.txt"),
            Ok(Search::NotFound {
                slotoffset: 384 + 3 * 32,
                wincnt: 4
            })
        );
        // A DOS name needs only one.
        assert_eq!(
            create(b"NEW.TXT"),
            Ok(Search::NotFound {
                slotoffset: 256,
                wincnt: 1
            })
        );
        // Not a creation: no slot is looked for.
        assert_eq!(
            search(p, root, b"x.txt", LOOKUP, ISLASTCN),
            Ok(Search::NotFound {
                slotoffset: 0,
                wincnt: 2
            })
        );
        // Mounted with short names only, long names take one slot.
        pmp.pm_flags
            .set(pmp.pm_flags.get() | MSDOSFSMNT_SHORTNAME as u32);
        assert_eq!(
            create(b"a much longer name.txt"),
            Ok(Search::NotFound {
                slotoffset: 256,
                wincnt: 1
            })
        );
        // ... and the long name entries are not read: a long name is not found.
        assert!(matches!(
            search(p, root, LONG2, LOOKUP, 0),
            Ok(Search::NotFound { .. })
        ));

        unmount(p, mp);
        teardown();
    }

    #[test]
    fn readep_and_readde() {
        let (disk, sub) = sample();
        let (_g, p) = setup(disk);
        let (mp, pmp, _root) = mounted(p);

        let (bp, off) = readep(pmp, MSDOSFSROOT, 96).unwrap();
        // SAFETY: the buffer is busy for the test (from `readep`) and mapped.
        let e = *Direntry::at(unsafe { bp.data() }, off);
        brelse(bp);
        assert_eq!(&e.name11(), b"M10C-FATTXT");
        assert_eq!(bp.b_bcount.get(), 512);

        let dep: &'static Denode = Box::leak(Box::new(Denode::new()));
        dep.de_pmp.set(Some(pmp));
        dep.de_dirclust.set(sub);
        dep.de_diroffset.set(32);
        let (bp, off) = readde(dep).unwrap();
        assert_eq!(off, 32);
        // SAFETY: as above.
        let e = *Direntry::at(unsafe { bp.data() }, off);
        brelse(bp);
        assert_eq!(&e.name11(), b"..         ");
        assert_eq!(e.deAttributes, ATTR_DIRECTORY);

        unmount(p, mp);
        teardown();
    }

    #[test]
    fn readep_shortens_the_last_root_directory_block() {
        // 2 KB clusters and a root directory of one 512-byte block.
        let params = mkfat::Params {
            fat: 12,
            size: 1 << 20,
            bps: 512,
            spc: 4,
            rde: 16,
        };
        let mut img = Image::new(params);
        img.add_file(Dir::Root, None, b"ONLY    TXT", b"x");
        let (_g, p) = setup(img.finish());
        let mp = mount(p, true);
        let pmp = vfstomsdosfs(mp);
        assert_eq!(pmp.pm_rootdirsize.get(), 1);
        assert_eq!(pmp.pm_bpcluster.get(), 2048);
        let (bp, off) = readep(pmp, MSDOSFSROOT, 0).unwrap();
        assert_eq!(bp.b_bcount.get(), 512);
        // SAFETY: the buffer is busy for the test (from `readep`) and mapped.
        let e = *Direntry::at(unsafe { bp.data() }, off);
        brelse(bp);
        assert_eq!(&e.name11(), b"ONLY    TXT");
        unmount(p, mp);
        teardown();
    }

    #[test]
    fn uniqdosname_skips_taken_names() {
        let (disk, _) = sample();
        let (_g, p) = setup(disk);
        let (mp, _pmp, root) = mounted(p);

        // LONG2's first generated name is taken.
        let mut short = [0u8; 11];
        uniqdosname(root, &cn(p, LONG2, CREATE, ISLASTCN), &mut short).unwrap();
        assert_eq!(short, dosname(LONG2, 2));
        // A new name keeps its first one.
        uniqdosname(
            root,
            &cn(p, b"another long name.txt", CREATE, ISLASTCN),
            &mut short,
        )
        .unwrap();
        assert_eq!(short, dosname(b"another long name.txt", 1));
        // Dots and blanks alone make no name.
        assert_eq!(
            uniqdosname(root, &cn(p, b". .", CREATE, ISLASTCN), &mut short),
            Err(Errno::EINVAL)
        );

        unmount(p, mp);
        teardown();
    }

    /// Creates the entry for `name` in `dp` where a `CREATE` lookup puts it (as
    /// `msdosfs_create` does with a stack denode): the DOS entry's directory offset.
    fn create_entry(p: &Proc, dp: &'static Denode, name: &'static [u8]) -> u32 {
        let cnp = cn(p, name, CREATE, ISLASTCN);
        let Ok(Search::NotFound { slotoffset, wincnt }) = msdosfs_lookup_search(
            dp,
            dp.de_StartCluster.get() == MSDOSFSROOT,
            &cnp,
            CREATE,
            ISLASTCN,
        ) else {
            panic!("{name:?} exists");
        };
        dp.de_fndoffset.set(slotoffset);
        dp.de_fndcnt.set(wincnt - 1);

        let tmpl = Denode::new();
        tmpl.de_pmp.set(dp.de_pmp.get());
        let mut short = [0u8; 11];
        uniqdosname(dp, &cnp, &mut short).unwrap();
        tmpl.de_Name.set(short);
        tmpl.de_Attributes.set(ATTR_ARCHIVE);
        tmpl.de_MDate.set(0x21);
        createde(&tmpl, dp, None, &cnp).unwrap();
        slotoffset
    }

    /// Removes the entry of `name` from `dp`, as `msdosfs_remove` does after a `DELETE` lookup.
    fn remove_entry(p: &Proc, dp: &Denode, name: &'static [u8]) {
        assert!(matches!(
            search(p, dp, name, DELETE, ISLASTCN),
            Ok(Search::Found { .. })
        ));
        let victim = Denode::new();
        victim.de_refcnt.set(1);
        removede(dp, &victim).unwrap();
        assert_eq!(victim.de_refcnt.get(), 0);
    }

    #[test]
    fn createde_and_removede_in_one_block() {
        let (disk, _) = sample();
        let (_g, p) = setup(disk);
        let (mp, _pmp, root) = mounted(p);
        let name: &'static [u8] = b"a much longer name.txt";

        assert_eq!(create_entry(p, root, name), 320);
        // The DOS entry where the lookup said, its two long name entries before it.
        let short = dosname(name, 1);
        let e = root_entry(320);
        assert_eq!(&e[..11], &short);
        assert_eq!(e[11], ATTR_ARCHIVE);
        let lfn = mkfat::lfn_entries(name, &short);
        assert_eq!(root_entry(256), lfn[0]);
        assert_eq!(root_entry(288), lfn[1]);
        assert_eq!(root.de_fndoffset.get(), 256);
        // It is found by its long name.
        let Ok(Search::Found { blkoff, .. }) = search(p, root, name, LOOKUP, 0) else {
            panic!("not created");
        };
        assert_eq!(blkoff, 320);

        remove_entry(p, root, name);
        for off in [256, 288, 320] {
            assert_eq!(root_entry(off)[0], SLOT_DELETED, "offset {off}");
        }
        // The entry before the long name entries is another file's, and stays.
        assert_eq!(&root_entry(224)[..11], &dosname(LONG2, 1));
        assert!(matches!(
            search(p, root, name, LOOKUP, 0),
            Ok(Search::NotFound { .. })
        ));
        assert!(matches!(
            search(p, root, LONG2, LOOKUP, 0),
            Ok(Search::Found { .. })
        ));

        unmount(p, mp);
        teardown();
    }

    #[test]
    fn createde_and_removede_across_blocks() {
        // 14 used entries: the next three slots are 448, 480 (block 0) and 512 (block 1).
        let mut img = Image::new(mkfat::FAT12_1M);
        for i in 0..14u8 {
            let mut n = *b"F00     TXT";
            n[1] = b'0' + i / 10;
            n[2] = b'0' + i % 10;
            img.put(Dir::Root, &[empty_file(&n)]);
        }
        let (_g, p) = setup(img.finish());
        let (mp, _pmp, root) = mounted(p);
        let name: &'static [u8] = b"a much longer name.txt";

        assert_eq!(create_entry(p, root, name), 512);
        let short = dosname(name, 1);
        let lfn = mkfat::lfn_entries(name, &short);
        assert_eq!(&root_entry(512)[..11], &short);
        assert_eq!(root_entry(448), lfn[0]);
        assert_eq!(root_entry(480), lfn[1]);
        let Ok(Search::Found { blkoff, .. }) = search(p, root, name, LOOKUP, 0) else {
            panic!("not created");
        };
        assert_eq!(blkoff, 512);

        remove_entry(p, root, name);
        for off in [448, 480, 512] {
            assert_eq!(root_entry(off)[0], SLOT_DELETED, "offset {off}");
        }
        assert_eq!(&root_entry(416)[..3], b"F13");

        unmount(p, mp);
        teardown();
    }

    #[test]
    fn createde_grows_a_full_directory_and_dosdirempty() {
        let mut img = Image::new(mkfat::FAT12_1M);
        let sub = img.mkdir(Dir::Root, b"SUBDIR     ", 1);
        let full = img.mkdir(Dir::Root, b"FULL       ", 1);
        // "." and "..", then 14 files: the 512-byte cluster is full.
        for i in 0..14u8 {
            let mut n = *b"F00     TXT";
            n[1] = b'0' + i / 10;
            n[2] = b'0' + i % 10;
            img.put(Dir::Clust(full), &[empty_file(&n)]);
        }
        let disk = img.finish();
        let (_g, p) = setup(disk);
        let (mp, pmp, _root) = mounted(p);

        let subdp = dirnode(pmp, sub);
        assert!(dosdirempty(subdp));
        let fulldp = dirnode(pmp, full);
        assert!(!dosdirempty(fulldp));
        assert_eq!(fulldp.de_FileSize.get(), 512);

        // A deleted entry does not make a directory non-empty.
        create_entry(p, subdp, b"x.txt");
        assert!(!dosdirempty(subdp));
        remove_entry(p, subdp, b"x.txt");
        assert!(dosdirempty(subdp));

        // No room: the new entry and its long name entry go to a new cluster.
        let free = pmp.pm_freeclustercount.get();
        assert_eq!(create_entry(p, fulldp, b"x.txt"), 544);
        assert_eq!(fulldp.de_FileSize.get(), 1024);
        assert_eq!(pmp.pm_freeclustercount.get(), free - 1);
        let Ok(Search::Found {
            cluster, blkoff, ..
        }) = search(p, fulldp, b"x.txt", LOOKUP, 0)
        else {
            panic!("not created");
        };
        assert_eq!(blkoff, 32);
        assert_ne!(cluster, full);
        msdosfs_sync(mp, MNT_WAIT, 0, p.p_ucred.get(), p).unwrap();
        {
            // The chain on the disk: the directory's cluster, then the new one.
            let d = DISK.lock().unwrap();
            let mut img = Image::new(mkfat::FAT12_1M);
            img.disk.copy_from_slice(&d);
            assert_eq!(img.fat_get(full), cluster);
            assert_eq!(img.fat_get(cluster), 0xfff);
            let o = img.clust_off(cluster);
            assert_eq!(&img.disk[o + 32..o + 43], b"X       TXT");
            assert_eq!(img.disk[o + 11], 0x0f);
        }

        unmount(p, mp);
        teardown();
    }

    fn test_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
        VOP_UNLOCK(ap.a_vp)
    }

    fn test_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
        ap.a_vp.v_data.set(core::ptr::null_mut());
        Ok(())
    }

    /// The operations `msdosfs_lookup` uses on the directory it searches, until the port of
    /// `msdosfs_vnops.c`: every access allowed, locks that always succeed.
    static TEST_VOPS: Vops = Vops {
        vop_access: Some(|_| nullop()),
        vop_lock: Some(|_| nullop()),
        vop_unlock: Some(|_| nullop()),
        vop_islocked: Some(|_| 0),
        vop_inactive: Some(test_inactive),
        vop_reclaim: Some(test_reclaim),
        ..Vops::EMPTY
    };

    /// A vnode of `mp` for the directory denode `dp`.
    fn dirvnode(mp: &'static Mount, dp: &'static Denode) -> &'static Vnode {
        let vp = getnewvnode(VT_MSDOSFS, Some(mp), &TEST_VOPS).unwrap();
        vp.v_type.set(VDIR);
        vp.v_data.set(core::ptr::from_ref(dp).cast_mut().cast());
        dp.de_vnode.set(Some(vp));
        vp
    }

    #[test]
    fn msdosfs_lookup_paths_without_new_denodes() {
        let (disk, sub) = sample();
        let (_g, p) = setup(disk);
        let (mp, pmp, root) = mounted(p);
        let rootvp = dirvnode(mp, root);
        rootvp.v_flag.set(rootvp.v_flag.get() | VROOT);
        let subdp = dirnode(pmp, sub);
        let subvp = dirvnode(mp, subdp);

        let lookup = |dvp: &'static Vnode, name: &'static [u8], op: u64, flags: u64| {
            let mut cnp = cn(p, name, op, flags);
            let mut vpp = None;
            let r = msdosfs_lookup(&mut VopLookupArgs {
                a_dvp: dvp,
                a_vpp: &mut vpp,
                a_cnp: &mut cnp,
            });
            (r, vpp, cnp.cn_flags)
        };

        // Not there.
        let (r, vpp, _) = lookup(rootvp, b"nothere", LOOKUP, ISLASTCN);
        assert_eq!(r, Err(Errno::ENOENT));
        assert!(vpp.is_none());
        // Not there, to be created: where it goes, the directory left locked.
        let (r, vpp, flags) = lookup(rootvp, b"x.txt", CREATE, ISLASTCN | LOCKPARENT);
        assert_eq!(r, Err(Errno::EJUSTRETURN));
        assert!(vpp.is_none());
        assert_eq!(root.de_fndoffset.get(), 288);
        assert_eq!(root.de_fndcnt.get(), 1);
        assert_ne!(flags & SAVENAME, 0);
        assert_eq!(flags & PDIRUNLOCK, 0);
        // Without LOCKPARENT the directory is unlocked.
        let (_, _, flags) = lookup(rootvp, b"x.txt", CREATE, ISLASTCN);
        assert_ne!(flags & PDIRUNLOCK, 0);
        // The root directory cannot be deleted or renamed over.
        let (r, _, _) = lookup(rootvp, b".", DELETE, ISLASTCN);
        assert_eq!(r, Err(Errno::EROFS));
        let (r, _, _) = lookup(rootvp, b"..", RENAME, ISLASTCN | WANTPARENT);
        assert_eq!(r, Err(Errno::EROFS));
        // "." of a subdirectory is the directory itself.
        let (r, vpp, _) = lookup(subvp, b".", LOOKUP, ISLASTCN);
        assert_eq!(r, Ok(()));
        assert!(vpp.is_some_and(|vp| core::ptr::eq(vp, subvp)));
        vrele(subvp);
        let (r, vpp, _) = lookup(subvp, b".", DELETE, ISLASTCN);
        assert_eq!(r, Ok(()));
        assert!(vpp.is_some_and(|vp| core::ptr::eq(vp, subvp)));
        vrele(subvp);
        let (r, _, _) = lookup(subvp, b".", RENAME, ISLASTCN | WANTPARENT);
        assert_eq!(r, Err(Errno::EISDIR));
        // A file is not a directory to search.
        let file = dirnode(pmp, sub);
        file.de_Attributes.set(ATTR_ARCHIVE);
        let filevp = dirvnode(mp, file);
        let (r, _, _) = lookup(filevp, b"x", LOOKUP, ISLASTCN);
        assert_eq!(r, Err(Errno::ENOTDIR));

        for vp in [rootvp, subvp, filevp] {
            vrele(vp);
        }
        unmount(p, mp);
        teardown();
    }
}
/* </TESTS> */
