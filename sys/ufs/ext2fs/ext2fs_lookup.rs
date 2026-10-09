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
 * Modified for NetBSD 1.2E
 * May 1997, Manuel Bouyer
 * Laboratoire d'informatique de Paris VI
 */
/*
 *  modified for Lites 1.1
 *
 *  Aug 1995, Godmar Back (gback@cs.utah.edu)
 *  University of Utah, Department of Computer Science
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
 *	@(#)ufs_lookup.c	8.6 (Berkeley) 4/1/94
 */
/* </LICENSES> */

/* <CODE> */
//! ext2fs directories: reading them as `struct dirent`s (`ext2fs_readdir`), looking a name up
//! and finding where a new entry would go (`ext2fs_lookup`, `ext2fs_search_dirblock`), and
//! the routines that check, add, remove and rewrite directory entries (`ext2fs_dirbadentry`,
//! `ext2fs_direnter`, `ext2fs_dirremove`, `ext2fs_dirrewrite`), test a directory for
//! emptiness and walk `..` to keep renames from making loops (`ext2fs_dirempty`,
//! `ext2fs_checkpath`).
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_lookup.c @ 3ce1f3f79392
//!
//! The file is `ufs_lookup.c` for the ext2 directory format: an entry does not store the
//! terminating NUL of its name, so the on-disk entries cannot be handed out as `struct dirent`s
//! and are converted one by one (`ext2fs_dirconv2ffs`); the directory block is the file
//! system block (`e2fs_bsize`), not `DIRBLKSIZ`. The host tests of these functions go through
//! the vnode operations, in `ext2fs_vnops.rs`.
//!
//! ## Deviations
//! - A `struct ext2fs_direct *` into a directory block is an offset into the block's bytes,
//!   read and written by `ext2fs_dir.rs`'s accessors (`e2d_ino`, `e2d_reclen`, ...), which
//!   convert from and to little-endian; `memcpy` of an entry that may overlap is
//!   `copy_within`.
//! - `ext2fs_dirconv2ffs` returns the `struct dirent` (`d_name` is the entry's name up to its
//!   first NUL, as `strncpy` copies it). `ext2fs_readdir`'s conversion stops with `EIO` at an
//!   entry whose header or name would run past the bytes read: the C reads on into its
//!   buffer.
//! - `ext2fs_search_dirblock` takes the next entry from `offset` at the top of its loop. The
//!   C's `continue` after a mangled entry leaves `ep` where it was, so on a read-only mount
//!   (where `ufs_dirbad` does not panic) it would report the same entry forever; here the
//!   skip to the end of the block ends the block's search, as in `ufs_lookup`. A name whose
//!   bytes would run past the block does not match.
//! - `ext2fs_dirbadentry` returns `bool` (`false` for the C's 0: it panics on a bad entry);
//!   `ext2fs_dirempty` returns `bool`; `*foundp` of `ext2fs_search_dirblock` is a `bool`.
//! - `ext2fs_direnter` zeroes the new entry's name padding: the C copies the byte after the
//!   name (`cn_namelen + 1`) and leaves the rest of its stack `struct ext2fs_direct` as it
//!   was.
//! - `dirchk` is `ufs_lookup.rs`'s [`DIRCHK`], the C's `extern int dirchk`.

use core::sync::atomic::Ordering;

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_subr::uiomove;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::brelse;
use crate::kern::vfs_cache::{cache_enter, cache_lookup};
use crate::kern::vfs_subr::{vput, vref};
use crate::kern::vfs_vnops::{vn_lock, vn_rdwr};
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_BWRITE, VOP_READ, VOP_UNLOCK, VOP_WRITE};
use crate::kprintf;
use crate::machine::cpu::curproc;
use crate::sys::buf::Buf;
use crate::sys::dirent::{DT_UNKNOWN, Dirent, MAXNAMLEN, dirent_recsize};
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::{M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::mount::{MNT_RDONLY, Mount, VFS_VGET};
use crate::sys::namei::{
    CREATE, Componentname, DELETE, ISDOTDOT, ISLASTCN, LOCKPARENT, LOOKUP, MAKEENTRY, PDIRUNLOCK,
    RENAME, SAVENAME, WANTPARENT,
};
use crate::sys::param::roundup;
use crate::sys::types::Off;
use crate::sys::ucred::Ucred;
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::{
    IO_NODELOCKED, IO_SYNC, VDIR, VEXEC, VWRITE, Vnode, VopLookupArgs, VopReaddirArgs,
};
use crate::ufs::ext2fs::ext2fs::{E2FS_REV0, EXT2F_INCOMPAT_FTYPE};
use crate::ufs::ext2fs::ext2fs_dir::{
    Doff, Ext2fsDirect, Ext2fsDirtemplate, Ext2fsSearchslot, Slotstatus, e2d_ino, e2d_name,
    e2d_namlen, e2d_reclen, e2iftodt, ext2fs_dirsiz, inot2ext2dt, set_e2d_ino, set_e2d_reclen,
    set_e2d_type,
};
use crate::ufs::ext2fs::ext2fs_inode::{ext2fs_setsize, ext2fs_size, ext2fs_truncate};
use crate::ufs::ext2fs::ext2fs_subr::ext2fs_bufatoff;
use crate::ufs::ufs::dinode::{ISVTX, ROOTINO, Ufsino};
use crate::ufs::ufs::inode::{IN_CHANGE, IN_UPDATE, Inode, vtoi};
use crate::ufs::ufs::ufs_lookup::{DIRCHK, ufs_dirbad};
use crate::ufs::ufs::ufsmount::vfstoufs;

/// `MINDIRSIZ`: the first half of a `struct ext2fs_dirtemplate`, an entry whose name is at
/// most 4 bytes.
const MINDIRSIZ: usize = Ext2fsDirtemplate::SIZE / 2;

/// The vnode's mount, which an ext2fs vnode always has.
fn vmount(vp: &Vnode) -> &'static Mount {
    match vp.v_mount.get() {
        Some(mp) => mp,
        None => panic(format_args!("ext2fs: vnode {:p} without a mount", vp)),
    }
}

/// Whether directory entries keep the file type: a revision 1 file system with the
/// `FILETYPE` incompatible feature.
pub(crate) fn has_ftype(ip: &Inode) -> bool {
    let fs = ip.e2fs();
    fs.e2fs_rev() > E2FS_REV0 && fs.e2fs_features_incompat() & EXT2F_INCOMPAT_FTYPE != 0
}

/// `ext2fs_dirconv2ffs`: the `struct dirent` of the on-disk entry at `off` in `b`.
///
/// The problem that is tackled here is the fact that FFS includes the terminating zero on
/// disk while EXT2FS doesn't; this implies that we need to introduce some padding. For
/// instance, a filename "sbin" has normally a reclen 12 in EXT2, but 16 in FFS. This reminds
/// me of that Pepsi commercial: 'Kid saved a lousy nine cents...' If it wasn't for that, the
/// complete ufs code for directories would have worked w/o changes (except for the
/// difference in DIRBLKSIZ).
fn ext2fs_dirconv2ffs(b: &[u8], off: usize) -> Dirent {
    let namlen = e2d_namlen(b, off);
    let mut d = Dirent {
        d_fileno: u64::from(e2d_ino(b, off)),
        d_off: 0,
        d_reclen: 0,
        d_type: DT_UNKNOWN, // don't know more here
        d_namlen: namlen,
        __d_padding: [0; 4],
        d_name: [0; MAXNAMLEN + 1],
    };
    // XXX Right now this can't happen, but if one day MAXNAMLEN != E2FS_MAXNAMLEN we should
    // handle this more gracefully! (e2d_namlen is too small for such a comparison.)
    let name = e2d_name(b, off, usize::from(namlen));
    let n = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    d.d_name[..n].copy_from_slice(&name[..n]);

    // Godmar thinks: since e2dir->e2d_reclen can be big and means nothing anyway, we compute
    // our own reclen according to what we think is right.
    d.d_reclen = dirent_recsize(usize::from(namlen)) as u16;
    d
}

/// `uiomove(&dstd, dstd.d_reclen, uio)`: copy out the first `d_reclen` bytes of a
/// `struct dirent`.
fn dirent_uiomove(dp: &Dirent, uio: &mut Uio<'_>) -> Result<(), Errno> {
    let mut b = [0u8; size_of::<Dirent>()];
    b[0..8].copy_from_slice(&dp.d_fileno.to_ne_bytes());
    b[8..16].copy_from_slice(&dp.d_off.to_ne_bytes());
    b[16..18].copy_from_slice(&dp.d_reclen.to_ne_bytes());
    b[18] = dp.d_type;
    b[19] = dp.d_namlen;
    b[20..24].copy_from_slice(&dp.__d_padding);
    b[Dirent::NAME_OFFSET..].copy_from_slice(&dp.d_name);
    let reclen = usize::from(dp.d_reclen).min(b.len());
    uiomove(&mut b[..reclen], uio)
}

/// `ext2fs_readdir` (`vop_readdir`): vnode op for reading directories.
///
/// Convert the on-disk entries to `<sys/dirent.h>` entries. The problem is that the
/// conversion will blow up some entries by four bytes, so it can't be done in place. This is
/// too bad. Right now the conversion is done entry by entry, the converted entry is sent via
/// uiomove.
///
/// XXX allocate a buffer, convert as many entries as possible, then send the whole buffer to
/// uiomove
pub fn ext2fs_readdir(ap: &mut VopReaddirArgs<'_, '_>) -> Result<(), Errno> {
    let uio = &mut *ap.a_uio;
    let vp = ap.a_vp;
    let fs = vtoi(vp).e2fs();
    let mut off = uio.uio_offset;

    if vp.v_type.get() != VDIR {
        return Err(Errno::ENOTDIR);
    }

    let mut e2fs_count = uio.uio_resid;
    let entries =
        (uio.uio_offset as usize).wrapping_add(e2fs_count) & (fs.e2fs_bsize.get() as usize - 1);

    // Make sure we don't return partial entries.
    if e2fs_count <= entries {
        return Err(Errno::EINVAL);
    }

    e2fs_count -= entries;
    let Some(dirbuf) = malloc(e2fs_count, M_TEMP, M_WAITOK | M_ZERO) else {
        panic(format_args!("ext2fs_readdir: no memory"));
    };
    let mut aiov = [Iovec {
        iov_base: dirbuf.as_ptr().cast(),
        iov_len: e2fs_count,
    }];
    let mut auio = Uio {
        uio_iov: &mut aiov,
        uio_offset: uio.uio_offset,
        uio_resid: e2fs_count,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: uio.uio_rw,
        uio_procp: uio.uio_procp,
    };

    let mut error = VOP_READ(vp, &mut auio, 0, ap.a_cred);
    if error.is_ok() {
        let readcnt = e2fs_count - auio.uio_resid;
        // SAFETY: `dirbuf` is a fresh allocation of `e2fs_count` bytes, zeroed and then
        // filled by the read; it is freed below, after the last use of the slice.
        let buf = unsafe { core::slice::from_raw_parts(dirbuf.as_ptr(), readcnt) };
        let mut dp = 0usize;
        while dp < readcnt {
            let e2d_reclen = usize::from(e2d_reclen(buf, dp));
            if e2d_reclen == 0
                || dp + Ext2fsDirect::NAME_OFFSET + usize::from(e2d_namlen(buf, dp)) > readcnt
            {
                error = Err(Errno::EIO);
                break;
            }
            let mut dstd = ext2fs_dirconv2ffs(buf, dp);
            if dstd.d_name[..usize::from(dstd.d_namlen)].contains(&b'/') {
                error = Err(Errno::EINVAL);
                break;
            }
            if usize::from(dstd.d_reclen) > uio.uio_resid {
                break;
            }
            dstd.d_off = off + e2d_reclen as Off;
            error = dirent_uiomove(&dstd, uio);
            if error.is_err() {
                break;
            }
            off += e2d_reclen as Off;
            // advance dp
            dp += e2d_reclen;
        }
        // we need to correct uio_offset
        uio.uio_offset = off;
    }
    free(dirbuf, M_TEMP, e2fs_count);
    *ap.a_eofflag = i32::from(ext2fs_size(vtoi(vp)) as Off <= uio.uio_offset);
    error
}

/// `ext2fs_lookup` (`vop_lookup`): convert a component of a pathname into a pointer to a
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
/// Overall outline of `ext2fs_lookup`:
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
pub fn ext2fs_lookup(ap: &mut VopLookupArgs<'_>) -> Result<(), Errno> {
    let cnp = &mut *ap.a_cnp;
    let cred = cnp.cn_cred;
    let flags = cnp.cn_flags;
    let nameiop = cnp.cn_nameiop;

    let mut ss = Ext2fsSearchslot {
        slotstatus: Slotstatus::Found,
        slotoffset: -1,
        slotfreespace: 0,
        slotsize: 0,
        slotneeded: 0,
    };

    let mut bp: Option<&'static Buf> = None;
    *ap.a_vpp = None;
    let vdp = ap.a_dvp;
    let dp = vtoi(vdp);
    let dirblksize = dp.e2fs().e2fs_bsize.get();
    let lockparent = flags & LOCKPARENT != 0;
    let wantparent = flags & (LOCKPARENT | WANTPARENT) != 0;

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

    // Suppress search for slots unless creating file and at end of pathname, in which case
    // we watch for a place to put the new file in case it doesn't already exist.
    if (nameiop == CREATE || nameiop == RENAME) && flags & ISLASTCN != 0 {
        ss.slotstatus = Slotstatus::None;
        ss.slotneeded = ext2fs_dirsiz(cnp.cn_namelen as usize) as i32;
    }

    // If there is cached information on a previous search of this directory, pick up where
    // we last left off. We cache only lookups as these are the most common and have the
    // greatest payoff. Caching CREATE has little benefit as it usually must search the entire
    // directory to determine that the entry does not exist. Caching the location of the last
    // DELETE or RENAME has not reduced profiling time and hence has been removed in the
    // interest of simplicity.
    let bmask = vfstoufs(vmount(vdp)).mountp().mnt_stat.get().f_iosize as i32 - 1;
    let mut entryoffsetinblock: i32;
    let mut numdirpasses;
    if nameiop != LOOKUP || dp.i_diroff.get() == 0 || dp.i_diroff.get() as u64 > ext2fs_size(dp) {
        dp.i_offset.set(0);
        numdirpasses = 1;
    } else {
        dp.i_offset.set(dp.i_diroff.get());
        entryoffsetinblock = dp.i_offset.get() & bmask;
        if entryoffsetinblock != 0 {
            let (b, _) = ext2fs_bufatoff(dp, Off::from(dp.i_offset.get()))?;
            bp = Some(b);
        }
        numdirpasses = 2;
    }
    let mut prevoff: Doff = dp.i_offset.get();
    let mut endsearch = roundup(ext2fs_size(dp) as usize, dirblksize as usize) as Doff;
    let mut enduseful: Doff = 0;

    // The entry found: its offset in its block and its name length.
    let mut found: Option<(i32, u8)> = None;
    'searchloop: loop {
        while dp.i_offset.get() < endsearch {
            // If necessary, get the next directory block.
            if let Some(b) = bp.take() {
                brelse(b);
            }

            let (b, _) = ext2fs_bufatoff(dp, Off::from(dp.i_offset.get()))?;
            bp = Some(b);
            entryoffsetinblock = 0;

            // If still looking for a slot, and at a dirblksize boundary, have to start looking
            // for free space again.
            if ss.slotstatus == Slotstatus::None {
                ss.slotoffset = -1;
                ss.slotfreespace = 0;
            }

            // SAFETY: the buffer is ours (busy from ext2fs_bufatoff) and mapped; the slice
            // dies before the buffer is released.
            let data: &[u8] = unsafe { b.data() };
            let mut entry_found = false;
            if let Err(e) = ext2fs_search_dirblock(
                dp,
                data,
                &mut entry_found,
                cnp,
                &mut entryoffsetinblock,
                &mut prevoff,
                &mut enduseful,
                &mut ss,
            ) {
                brelse(b);
                return Err(e);
            }
            if entry_found {
                let ep = (entryoffsetinblock & bmask) as usize;
                // foundentry:
                dp.i_ino.set(e2d_ino(data, ep));
                dp.i_reclen.set(u32::from(e2d_reclen(data, ep)));
                found = Some((entryoffsetinblock, e2d_namlen(data, ep)));
                break 'searchloop;
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

    let Some((entryoffsetinblock, namlen)) = found else {
        if let Some(b) = bp {
            brelse(b);
        }
        // If creating, and at end of pathname and current directory has not been removed,
        // then can consider allowing file to be created.
        if (nameiop == CREATE || nameiop == RENAME)
            && flags & ISLASTCN != 0
            && dp.i_e2fs_nlink() != 0
        {
            // Creation of files on a read-only mounted file system is pointless, so don't
            // proceed any further.
            if vmount(vdp).mnt_flag.get() & MNT_RDONLY != 0 {
                return Err(Errno::EROFS);
            }
            // Access for write is interpreted as allowing creation of files in the
            // directory.
            VOP_ACCESS(vdp, VWRITE, cred, cnp.proc())?;
            // Return an indication of where the new directory entry should be put. If we
            // didn't find a slot, then set dp->i_count to 0 indicating that the new slot
            // belongs at the end of the directory. If we found a slot, then the new entry can
            // be put in the range from dp->i_offset to dp->i_offset + dp->i_count.
            if ss.slotstatus == Slotstatus::None {
                dp.i_offset
                    .set(roundup(ext2fs_size(dp) as usize, dirblksize as usize) as Doff);
                dp.i_count.set(0);
                enduseful = dp.i_offset.get();
            } else {
                dp.i_offset.set(ss.slotoffset);
                dp.i_count.set(ss.slotsize);
                if enduseful < ss.slotoffset + ss.slotsize {
                    enduseful = ss.slotoffset + ss.slotsize;
                }
            }
            dp.i_endoff
                .set(roundup(enduseful as usize, dirblksize as usize) as Doff);
            dp.set_flag(IN_CHANGE | IN_UPDATE);
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
    let Some(b) = bp else {
        panic(format_args!("ext2fs_lookup: no directory block"));
    };
    // Check that directory length properly reflects presence of this entry.
    let reach = entryoffsetinblock as u64 + ext2fs_dirsiz(usize::from(namlen)) as u64;
    if reach > ext2fs_size(dp) {
        ufs_dirbad(dp, dp.i_offset.get(), "i_size too small");
        if let Err(e) = ext2fs_setsize(dp, reach) {
            brelse(b);
            return Err(e);
        }
        dp.set_flag(IN_CHANGE | IN_UPDATE);
    }
    brelse(b);

    // Found component in pathname. If the final component of path name, save information in
    // the cache as to where the entry was found.
    if flags & ISLASTCN != 0 && nameiop == LOOKUP {
        dp.i_diroff.set(dp.i_offset.get() & !(dirblksize - 1));
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
        if dp.i_offset.get() & (dirblksize - 1) == 0 {
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
        let uid = cnp.cred().cr_uid.get();
        if u32::from(dp.i_e2fs_mode()) & ISVTX != 0
            && uid != 0
            && uid != dp.i_e2fs_uid().get()
            && vtoi(tdp).i_e2fs_uid().get() != uid
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

/// `ext2fs_search_dirblock`: look for the name of `cnp` in the directory block `data` of
/// `ip`, from `*entryoffsetinblockp` (and `ip->i_offset`, the same place in the file) on,
/// noting free space for a new entry in `*ssp` on the way. On a match `*foundp` is set and
/// `*entryoffsetinblockp` is the entry's offset in the block.
#[allow(clippy::too_many_arguments)] // the C's prototype
pub fn ext2fs_search_dirblock(
    ip: &Inode,
    data: &[u8],
    foundp: &mut bool,
    cnp: &Componentname,
    entryoffsetinblockp: &mut i32,
    prevoffp: &mut Doff,
    endusefulp: &mut Doff,
    ssp: &mut Ext2fsSearchslot,
) -> Result<(), Errno> {
    let mut offset = *entryoffsetinblockp;
    let dirblksize = ip.e2fs().e2fs_bsize.get();

    let vdp = ip.itov();

    let lim = dirblksize - ext2fs_dirsiz(0) as i32;

    while offset < lim {
        let ep = offset as usize;
        let reclen = i32::from(e2d_reclen(data, ep));
        // Full validation checks are slow, so we only check enough to insure forward progress
        // through the directory. Complete checks can be run by patching "dirchk" to be true.
        if reclen == 0
            || (DIRCHK.load(Ordering::Relaxed) != 0 && ext2fs_dirbadentry(vdp, data, offset))
        {
            ufs_dirbad(ip, ip.i_offset.get(), "mangled entry");
            let i = dirblksize - (offset & (dirblksize - 1));
            ip.i_offset.set(ip.i_offset.get() + i);
            offset += i;
            continue;
        }

        // If an appropriate sized slot has not yet been found, check to see if one is
        // available. Also accumulate space in the current block so that we can determine if
        // compaction is viable.
        if ssp.slotstatus != Slotstatus::Found {
            let mut size = reclen;

            if e2d_ino(data, ep) != 0 {
                size -= ext2fs_dirsiz(usize::from(e2d_namlen(data, ep))) as i32;
            }
            if size > 0 {
                if size >= ssp.slotneeded {
                    ssp.slotstatus = Slotstatus::Found;
                    ssp.slotoffset = ip.i_offset.get();
                    ssp.slotsize = reclen;
                } else if ssp.slotstatus == Slotstatus::None {
                    ssp.slotfreespace += size;
                    if ssp.slotoffset == -1 {
                        ssp.slotoffset = ip.i_offset.get();
                    }
                    if ssp.slotfreespace >= ssp.slotneeded {
                        ssp.slotstatus = Slotstatus::Compact;
                        ssp.slotsize = ip.i_offset.get() + reclen - ssp.slotoffset;
                    }
                }
            }
        }

        // Check for a name match.
        if e2d_ino(data, ep) != 0 {
            let namlen = usize::from(e2d_namlen(data, ep));
            let name = e2d_name(data, ep, namlen);
            if namlen as i64 == cnp.cn_namelen && name.len() == namlen && name == cnp.name() {
                // Save directory entry's inode number and reclen in ndp->ni_ufs area, and
                // release directory buffer.
                *foundp = true;
                return Ok(());
            }
        }
        *prevoffp = ip.i_offset.get();
        ip.i_offset.set(ip.i_offset.get() + reclen);
        offset += reclen;
        *entryoffsetinblockp = offset;
        if e2d_ino(data, ep) != 0 {
            *endusefulp = ip.i_offset.get();
        }
    }

    Ok(())
}

/// `ext2fs_dirbadentry`: do consistency checking on the directory entry at
/// `entryoffsetinblock` in `b` (changed so that it conforms to Linux's
/// `ext2fs_check_dir_entry`): record length must be a multiple of 4 and at least the size of
/// an entry with a one-byte name; record must be large enough to contain the entry; entry
/// must fit in the rest of its block; inode number must be in the file system. A bad entry
/// is reported and the system panics; `false` otherwise.
pub fn ext2fs_dirbadentry(dp: &Vnode, b: &[u8], entryoffsetinblock: i32) -> bool {
    let fs = vtoi(dp).e2fs();
    let dirblksize = fs.e2fs_bsize.get();
    let off = entryoffsetinblock as usize;
    let reclen = i32::from(e2d_reclen(b, off));
    let namlen = e2d_namlen(b, off);

    let error_msg = if reclen < ext2fs_dirsiz(1) as i32 {
        // e2d_namlen = 1
        Some("rec_len is smaller than minimal")
    } else if reclen % 4 != 0 {
        Some("rec_len % 4 != 0")
    } else if reclen < ext2fs_dirsiz(usize::from(namlen)) as i32 {
        Some("reclen is too small for name_len")
    } else if entryoffsetinblock + reclen > dirblksize {
        Some("directory entry across blocks")
    } else if e2d_ino(b, off) > fs.e2fs_icount() {
        Some("inode out of bounds")
    } else {
        None
    };

    if let Some(msg) = error_msg {
        kprintf!(
            "bad directory entry: {}\noffset={}, inode={}, rec_len={}, name_len={} \n",
            msg,
            entryoffsetinblock,
            e2d_ino(b, off),
            reclen,
            namlen
        );
        panic(format_args!("ext2fs_dirbadentry"));
    }
    false
}

/// `ext2fs_direnter`: write a directory entry after a call to namei, using the parameters
/// that it left in nameidata. The argument `ip` is the inode which the new directory entry
/// will refer to. `dvp` is a pointer to the directory to be written, which was left locked
/// by namei. Remaining parameters (`dp->i_offset`, `dp->i_count`) indicate how the space for
/// the new entry is to be obtained.
pub fn ext2fs_direnter(ip: &Inode, dvp: &'static Vnode, cnp: &Componentname) -> Result<(), Errno> {
    let dirblksize = ip.e2fs().e2fs_bsize.get();

    #[cfg(feature = "diagnostic")]
    if cnp.cn_flags & SAVENAME == 0 {
        panic(format_args!("direnter: missing name"));
    }
    let dp = vtoi(dvp);
    let mut newdir = Ext2fsDirect::new();
    newdir.e2d_ino = ip.i_number.get();
    newdir.e2d_namlen = cnp.cn_namelen as u8;
    newdir.e2d_type = if has_ftype(ip) {
        inot2ext2dt(e2iftodt(ip.i_e2fs_mode()))
    } else {
        0
    };
    let name = cnp.name();
    newdir.e2d_name[..name.len()].copy_from_slice(name);
    let newentrysize = ext2fs_dirsiz(cnp.cn_namelen as usize);
    if dp.i_count.get() == 0 {
        // If dp->i_count is 0, then namei could find no space in the directory. Here,
        // dp->i_offset will be on a directory block boundary and we will write the new entry
        // into a fresh block.
        if dp.i_offset.get() & (dirblksize - 1) != 0 {
            panic(format_args!("ext2fs_direnter: newblk"));
        }
        newdir.e2d_reclen = dirblksize as u16;
        let mut entry = [0u8; size_of::<Ext2fsDirect>()];
        newdir.write_to(&mut entry, 0, newentrysize);
        let mut aiov = [Iovec {
            iov_base: entry.as_mut_ptr().cast(),
            iov_len: newentrysize,
        }];
        let mut auio = Uio {
            uio_iov: &mut aiov,
            uio_offset: Off::from(dp.i_offset.get()),
            uio_resid: newentrysize,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_WRITE,
            uio_procp: None,
        };
        let error = VOP_WRITE(dvp, &mut auio, IO_SYNC, cnp.cn_cred);
        if i64::from(dirblksize) > vfstoufs(vmount(dvp)).mountp().mnt_stat.get().f_bsize as i64 {
            // XXX should grow with balloc()
            panic(format_args!("ext2fs_direnter: frag size"));
        } else if error.is_ok() {
            ext2fs_setsize(
                dp,
                roundup(ext2fs_size(dp) as usize, dirblksize as usize) as u64,
            )?;
            dp.set_flag(IN_CHANGE);
        }
        return error;
    }

    // If dp->i_count is non-zero, then namei found space for the new entry in the range
    // dp->i_offset to dp->i_offset + dp->i_count in the directory. To use this space, we may
    // have to compact the entries located there, by copying them together towards the
    // beginning of the block, leaving the free space in one usable chunk at the end.

    // Get the block containing the space for the new directory entry.
    let (bp, boff) = ext2fs_bufatoff(dp, Off::from(dp.i_offset.get()))?;
    {
        // SAFETY: the buffer is ours (busy from ext2fs_bufatoff) and mapped; the slice dies
        // before it is written.
        let data = unsafe { bp.data() };
        let dirbuf = &mut data[boff..];
        // Find space for the new entry. In the simple case, the entry at offset base will
        // have the space. If it does not, then namei arranged that compacting the region
        // dp->i_offset to dp->i_offset + dp->i_count would yield the space.
        let mut ep = 0usize;
        let mut dsize = ext2fs_dirsiz(usize::from(e2d_namlen(dirbuf, ep)));
        let mut spacefree = i32::from(e2d_reclen(dirbuf, ep)) - dsize as i32;
        let mut loc = i32::from(e2d_reclen(dirbuf, ep));
        while loc < dp.i_count.get() {
            let nep = loc as usize;
            if e2d_ino(dirbuf, ep) != 0 {
                // trim the existing slot
                set_e2d_reclen(dirbuf, ep, dsize as u16);
                ep += dsize;
            } else {
                // overwrite; nothing there; header is ours
                spacefree += dsize as i32;
            }
            dsize = ext2fs_dirsiz(usize::from(e2d_namlen(dirbuf, nep)));
            spacefree += i32::from(e2d_reclen(dirbuf, nep)) - dsize as i32;
            loc += i32::from(e2d_reclen(dirbuf, nep));
            dirbuf.copy_within(nep..nep + dsize, ep);
        }
        // Update the pointer fields in the previous entry (if any), copy in the new entry,
        // and write out the block.
        if e2d_ino(dirbuf, ep) == 0 {
            #[cfg(feature = "diagnostic")]
            if spacefree + (dsize as i32) < newentrysize as i32 {
                panic(format_args!("ext2fs_direnter: compact1"));
            }
            newdir.e2d_reclen = (spacefree + dsize as i32) as u16;
        } else {
            #[cfg(feature = "diagnostic")]
            if spacefree < newentrysize as i32 {
                kprintf!(
                    "ext2fs_direnter: compact2 {} {}",
                    spacefree as u32,
                    newentrysize
                );
                panic(format_args!("ext2fs_direnter: compact2"));
            }
            newdir.e2d_reclen = spacefree as u16;
            set_e2d_reclen(dirbuf, ep, dsize as u16);
            ep += dsize;
        }
        newdir.write_to(dirbuf, ep, newentrysize);
    }
    let mut error = VOP_BWRITE(bp);
    dp.set_flag(IN_CHANGE | IN_UPDATE);
    if error.is_ok() && dp.i_endoff.get() != 0 && (dp.i_endoff.get() as u64) < ext2fs_size(dp) {
        error = ext2fs_truncate(dp, Off::from(dp.i_endoff.get()), IO_SYNC, cnp.cn_cred);
    }
    error
}

/// `ext2fs_dirremove`: remove a directory entry after a call to namei, using the parameters
/// which it left in nameidata. The entry `dp->i_offset` contains the offset into the
/// directory of the entry to be eliminated. The `dp->i_count` field contains the size of the
/// previous record in the directory. If this is 0, the first entry is being deleted, so we
/// need only zero the inode number to mark the entry as free. If the entry is not the first
/// in the directory, we must reclaim the space of the now empty record by adding the record
/// size to the size of the previous entry.
pub fn ext2fs_dirremove(dvp: &'static Vnode, _cnp: &Componentname) -> Result<(), Errno> {
    let dp = vtoi(dvp);
    if dp.i_count.get() == 0 {
        // First entry in block: set d_ino to zero.
        let (bp, off) = ext2fs_bufatoff(dp, Off::from(dp.i_offset.get()))?;
        // SAFETY: the buffer is ours (busy from ext2fs_bufatoff) and mapped; the slice dies
        // before it is written.
        set_e2d_ino(unsafe { bp.data() }, off, 0);
        let error = VOP_BWRITE(bp);
        dp.set_flag(IN_CHANGE | IN_UPDATE);
        return error;
    }
    // Collapse new free space into previous entry.
    let (bp, off) = ext2fs_bufatoff(dp, Off::from(dp.i_offset.get() - dp.i_count.get()))?;
    {
        // SAFETY: as above.
        let data = unsafe { bp.data() };
        let r = u32::from(e2d_reclen(data, off)) + dp.i_reclen.get();
        set_e2d_reclen(data, off, r as u16);
    }
    let error = VOP_BWRITE(bp);
    dp.set_flag(IN_CHANGE | IN_UPDATE);
    error
}

/// `ext2fs_dirrewrite`: rewrite an existing directory entry to point at the inode supplied.
/// The parameters describing the directory entry are set up by a call to namei.
pub fn ext2fs_dirrewrite(dp: &Inode, ip: &Inode, _cnp: &Componentname) -> Result<(), Errno> {
    let (bp, off) = ext2fs_bufatoff(dp, Off::from(dp.i_offset.get()))?;
    {
        // SAFETY: the buffer is ours (busy from ext2fs_bufatoff) and mapped; the slice dies
        // before it is written.
        let data = unsafe { bp.data() };
        set_e2d_ino(data, off, ip.i_number.get());
        let t = if has_ftype(ip) {
            inot2ext2dt(e2iftodt(ip.i_e2fs_mode()))
        } else {
            0
        };
        set_e2d_type(data, off, t);
    }
    let error = VOP_BWRITE(bp);
    dp.set_flag(IN_CHANGE | IN_UPDATE);
    error
}

/// `ext2fs_dirempty`: check if a directory is empty or not (only `.` and `..`, the latter
/// naming `parentino`). Inode supplied must be locked.
///
/// Using a struct dirtemplate here is not precisely what we want, but better than using a
/// struct ext2fs_direct.
///
/// NB: does not handle corrupted directories.
pub fn ext2fs_dirempty(ip: &Inode, parentino: Ufsino, cred: *const Ucred) -> bool {
    let mut dbuf = [0u8; Ext2fsDirtemplate::SIZE];
    let mut off: Off = 0;
    while (off as u64) < ext2fs_size(ip) {
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
        let reclen = e2d_reclen(&dbuf, 0);
        // avoid infinite loops
        if reclen == 0 {
            return false;
        }
        off += Off::from(reclen);
        // skip empty entries
        let ino = e2d_ino(&dbuf, 0);
        if ino == 0 {
            continue;
        }
        // accept only "." and ".."
        let namlen = e2d_namlen(&dbuf, 0);
        if namlen > 2 {
            return false;
        }
        let name = e2d_name(&dbuf, 0, 2);
        if name[0] != b'.' {
            return false;
        }
        // At this point namlen must be 1 or 2. 1 implies ".", 2 implies ".." if second char
        // is also "."
        if namlen == 1 {
            continue;
        }
        if name[1] == b'.' && ino == parentino {
            continue;
        }
        return false;
    }
    true
}

/// `ext2fs_checkpath`: check if source directory is in the path of the target directory.
/// Target is supplied locked, source is unlocked. The target is always vput before
/// returning.
pub fn ext2fs_checkpath(source: &Inode, target: &Inode, cred: *const Ucred) -> Result<(), Errno> {
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
            let mut dirbuf = [0u8; Ext2fsDirtemplate::SIZE];
            if let Err(e) = vn_rdwr(
                UioRw::UIO_READ,
                cur,
                dirbuf.as_mut_ptr().cast(),
                Ext2fsDirtemplate::SIZE,
                0,
                UioSeg::UIO_SYSSPACE,
                IO_NODELOCKED,
                cred,
                None,
                curproc(),
            ) {
                break 'out Err(e);
            }
            let t = Ext2fsDirtemplate::from_le_bytes(&dirbuf);
            if t.dotdot_namlen != 2 || t.dotdot_name[0] != b'.' || t.dotdot_name[1] != b'.' {
                break 'out Err(Errno::ENOTDIR);
            }
            let ino = t.dotdot_ino;
            if ino == source.i_number.get() {
                break 'out Err(Errno::EINVAL);
            }
            if ino == rootino {
                break 'out Ok(());
            }
            let mp = vmount(cur);
            vput(cur);
            match VFS_VGET(mp, u64::from(ino)) {
                Ok(next) => vp = Some(next),
                Err(e) => {
                    vp = None;
                    break 'out Err(e);
                }
            }
        }
    };

    // out:
    if error == Err(Errno::ENOTDIR) {
        kprintf!("checkpath: .. not a directory\n");
        panic(format_args!("checkpath"));
    }
    if let Some(vp) = vp {
        vput(vp);
    }
    error
}

const _: () = {
    // The entry `ext2fs_readdir` converts fits the `struct dirent` it copies out.
    assert!(dirent_recsize(255) <= size_of::<Dirent>());
};
/* </CODE> */
