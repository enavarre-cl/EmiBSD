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
 * Copyright (c) 1997 Manuel Bouyer.
 * Copyright (c) 1993
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
 *	@(#)ufs_readwrite.c	8.8 (Berkeley) 8/4/94
 * Modified for ext2fs by Manuel Bouyer.
 */
/* </LICENSES> */

/* <CODE> */
//! ext2fs file reading and writing through the buffer cache: `ext2fs_read` (by the block
//! pointers, `ext2_ind_read`, or by the extent tree, `ext4_ext_read`) and `ext2fs_write`.
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_readwrite.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `ext4_ext_read` looks each block up with a fresh path, and releases the leaf buffer
//!   `ext4_ext_find_extent` may hold also when no extent covers the block (the C returns
//!   `EIO` with the buffer still busy). The C's path is reused across iterations, and only
//!   its `ep_bp`, released after each lookup, carries over.
//! - `ext2_ind_read`'s read-ahead is `breadn` with the next block, as in C.
//! - When `ext2fs_setsize` refuses the new size, `ext2fs_write` releases the block's buffer
//!   before it stops; the C leaves it busy.

use crate::kern::kern_subr::uiomove;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{bawrite, bdwrite, bread, breadn, brelse, bwrite};
use crate::kern::vfs_vnops::vn_fsizechk;
use crate::sys::buf::{B_CLRBUF, B_NOCACHE, B_SYNC};
use crate::sys::errno::Errno;
use crate::sys::event::{NOTE_EXTEND, NOTE_WRITE};
use crate::sys::mount::MNT_NOATIME;
use crate::sys::types::{Daddr, Off};
use crate::sys::uio::Uio;
#[cfg(feature = "diagnostic")]
use crate::sys::uio::UioRw;
use crate::sys::vnode::{
    IO_APPEND, IO_NOCACHE, IO_SYNC, IO_UNIT, VDIR, VLNK, VN_KNOTE, VREG, Vnode, VopReadArgs,
    VopWriteArgs, cred_ref,
};
use crate::ufs::ext2fs::ext2fs::{MExt2fs, blkoff, e2fs_overflow, fsbtodb, lblkno, lblktosize};
use crate::ufs::ext2fs::ext2fs_balloc::ext2fs_buf_alloc;
#[cfg(feature = "diagnostic")]
use crate::ufs::ext2fs::ext2fs_dinode::EXT2_MAXSYMLINKLEN;
use crate::ufs::ext2fs::ext2fs_dinode::{EXT2_APPEND, EXT4_EXTENTS};
use crate::ufs::ext2fs::ext2fs_extents::{
    EXT4_EXT_CACHE_GAP, EXT4_EXT_CACHE_IN, EXT4_EXT_CACHE_NO, Ext4Extent, Ext4ExtentPath,
    ext4_ext_find_extent, ext4_ext_in_cache, ext4_ext_put_cache,
};
use crate::ufs::ext2fs::ext2fs_inode::{
    ext2fs_setsize, ext2fs_size, ext2fs_truncate, ext2fs_update,
};
use crate::ufs::ufs::dinode::{ISGID, ISUID};
use crate::ufs::ufs::inode::{IN_ACCESS, IN_CHANGE, IN_UPDATE, Inode, vtoi};
use crate::uvm::uvm_vnode::{uvm_vnp_setsize, uvm_vnp_uncache};

/// `ext2fs_read` (`vop_read`): vnode op for reading.
pub fn ext2fs_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);
    let uio = &mut *ap.a_uio;
    let fs = ip.e2fs();

    if ip.i_e2fs_flags() & EXT4_EXTENTS != 0 {
        ext4_ext_read(vp, ip, fs, uio)
    } else {
        ext2_ind_read(vp, ip, fs, uio)
    }
}

/// `ext2_ind_read`: read a file mapped by its block pointers.
fn ext2_ind_read(
    vp: &'static Vnode,
    ip: &Inode,
    fs: &MExt2fs,
    uio: &mut Uio<'_>,
) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    {
        if uio.uio_rw != UioRw::UIO_READ {
            panic(format_args!("ext2fs_read: mode"));
        }

        if vp.v_type.get() == VLNK {
            if ext2fs_size(ip) < EXT2_MAXSYMLINKLEN as u64 {
                panic(format_args!("ext2fs_read: short symlink"));
            }
        } else if vp.v_type.get() != VREG && vp.v_type.get() != VDIR {
            panic(format_args!("ext2fs_read: type {}", vp.v_type.get() as i32));
        }
    }
    if uio.uio_offset < 0 {
        return Err(Errno::EINVAL);
    }
    if uio.uio_resid == 0 {
        return Ok(());
    }

    let bsize = i64::from(fs.e2fs_bsize.get());
    let mut error = Ok(());
    while uio.uio_resid > 0 {
        let bytesinfile = ext2fs_size(ip) as Off - uio.uio_offset;
        if bytesinfile <= 0 {
            break;
        }
        let lbn = lblkno(fs, uio.uio_offset);
        let nextlbn = lbn + 1;
        let mut size = bsize;
        let blkoffset = blkoff(fs, uio.uio_offset);
        let mut xfersize = bsize - blkoffset;
        if (uio.uio_resid as i64) < xfersize {
            xfersize = uio.uio_resid as i64;
        }
        if bytesinfile < xfersize {
            xfersize = bytesinfile;
        }

        let (bp, r) = if lblktosize(fs, nextlbn) >= ext2fs_size(ip) as Off {
            bread(vp, lbn, size as i32)
        } else if lbn - 1 == ip.i_ci.get().ci_lastr {
            let nextsize = fs.e2fs_bsize.get();
            breadn(vp, lbn, size as i32, &[nextlbn], &[nextsize])
        } else {
            bread(vp, lbn, size as i32)
        };
        if let Err(e) = r {
            // The C breaks with bp set, and the brelse after the loop releases it.
            brelse(bp);
            error = Err(e);
            break;
        }
        let mut ci = ip.i_ci.get();
        ci.ci_lastr = lbn;
        ip.i_ci.set(ci);

        // We should only get non-zero b_resid when an I/O error has occurred, which should
        // cause us to break above. However, if the short read did not cause an error, then
        // we want to ensure that we do not uiomove bad or uninitialized data.
        size -= bp.b_resid.get() as i64;
        if size < xfersize {
            if size == 0 {
                brelse(bp);
                break;
            }
            xfersize = size;
        }
        // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies before it
        // is released.
        let data = unsafe { bp.data() };
        let r = uiomove(
            &mut data[blkoffset as usize..(blkoffset + xfersize) as usize],
            uio,
        );
        brelse(bp);
        if let Err(e) = r {
            error = Err(e);
            break;
        }
    }

    if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_NOATIME == 0)
    {
        ip.set_flag(IN_ACCESS);
    }
    error
}

/// `ext4_ext_read`: read a file mapped by its extent tree.
pub fn ext4_ext_read(
    _vp: &'static Vnode,
    ip: &Inode,
    fs: &MExt2fs,
    uio: &mut Uio<'_>,
) -> Result<(), Errno> {
    if uio.uio_offset < 0 {
        return Err(Errno::EINVAL);
    }
    if uio.uio_resid == 0 {
        return Ok(());
    }

    while uio.uio_resid > 0 {
        let bytesinfile = ext2fs_size(ip) as Off - uio.uio_offset;
        if bytesinfile <= 0 {
            break;
        }
        let lbn = lblkno(fs, uio.uio_offset);
        let mut size = i64::from(fs.e2fs_bsize.get());
        let blkoffset = blkoff(fs, uio.uio_offset);

        let mut xfersize = i64::from(fs.e2fs_fsize.get()) - blkoffset;
        xfersize = xfersize.min(uio.uio_resid as i64);
        xfersize = xfersize.min(bytesinfile);

        let mut nex = Ext4Extent::default();
        let cache_type = ext4_ext_in_cache(ip, lbn, &mut nex);
        let pos: Daddr = match cache_type {
            EXT4_EXT_CACHE_NO => {
                let mut path = Ext4ExtentPath::new();
                let ep = ext4_ext_find_extent(fs, ip, lbn, &mut path).and_then(|p| p.ext());
                if let Some(bp) = path.ep_bp.take() {
                    brelse(bp);
                }
                let Some(ep) = ep else {
                    return Err(Errno::EIO);
                };
                ext4_ext_put_cache(ip, &ep, EXT4_EXT_CACHE_IN);

                lbn - Daddr::from(ep.e_blk)
                    + ((Daddr::from(ep.e_start_hi) << 32) | Daddr::from(ep.e_start_lo))
            }
            // block has not been allocated yet
            EXT4_EXT_CACHE_GAP => return Ok(()),
            EXT4_EXT_CACHE_IN => {
                lbn - Daddr::from(nex.e_blk)
                    + ((Daddr::from(nex.e_start_hi) << 32) | Daddr::from(nex.e_start_lo))
            }
            t => panic(format_args!("ext4_ext_read: cache type {}", t)),
        };
        let (bp, error) = bread(ip.i_devvp(), fsbtodb(fs, pos), size as i32);
        if let Err(e) = error {
            brelse(bp);
            return Err(e);
        }
        size -= bp.b_resid.get() as i64;
        if size < xfersize {
            if size == 0 {
                brelse(bp);
                break;
            }
            xfersize = size;
        }
        let r = {
            // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies
            // before it is released.
            let data = unsafe { bp.data() };
            uiomove(
                &mut data[blkoffset as usize..(blkoffset + xfersize) as usize],
                uio,
            )
        };
        brelse(bp);
        r?;
    }
    Ok(())
}

/// `ext2fs_write` (`vop_write`): vnode op for writing.
pub fn ext2fs_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    let mut extended = false;
    let ioflag = ap.a_ioflag;
    let uio = &mut *ap.a_uio;
    let vp = ap.a_vp;
    let ip = vtoi(vp);

    #[cfg(feature = "diagnostic")]
    if uio.uio_rw != UioRw::UIO_WRITE {
        panic(format_args!("ext2fs_write: mode"));
    }

    // If writing 0 bytes, succeed and do not change update time or file offset (standards
    // compliance)
    if uio.uio_resid == 0 {
        return Ok(());
    }

    match vp.v_type.get() {
        VREG => {
            if ioflag & IO_APPEND != 0 {
                uio.uio_offset = ext2fs_size(ip) as Off;
            }
            if ip.i_e2fs_flags() & EXT2_APPEND != 0 && uio.uio_offset != ext2fs_size(ip) as Off {
                return Err(Errno::EPERM);
            }
        }
        VLNK => {}
        VDIR => {
            if ioflag & IO_SYNC == 0 {
                panic(format_args!("ext2fs_write: nonsync dir write"));
            }
        }
        _ => panic(format_args!("ext2fs_write: type")),
    }

    let fs = ip.e2fs();
    if e2fs_overflow(
        fs,
        uio.uio_resid as Off,
        uio.uio_offset + uio.uio_resid as Off,
    ) {
        return Err(Errno::EFBIG);
    }

    // do the filesize rlimit check
    let overrun = vn_fsizechk(vp, uio, ioflag)?;

    let resid = uio.uio_resid;
    let osize = ext2fs_size(ip) as Off;
    let mut flags = if ioflag & IO_SYNC != 0 { B_SYNC } else { 0 };
    let bsize = i64::from(fs.e2fs_bsize.get());

    let mut error = Ok(());
    while uio.uio_resid > 0 {
        let lbn = lblkno(fs, uio.uio_offset) as i32;
        let blkoffset = blkoff(fs, uio.uio_offset);
        let mut xfersize = bsize - blkoffset;
        if (uio.uio_resid as i64) < xfersize {
            xfersize = uio.uio_resid as i64;
        }
        if bsize > xfersize {
            flags |= B_CLRBUF;
        } else {
            flags &= !B_CLRBUF;
        }

        let bp = match ext2fs_buf_alloc(
            ip,
            lbn as u32,
            (blkoffset + xfersize) as i32,
            ap.a_cred,
            flags,
        ) {
            Ok(bp) => bp,
            Err(e) => {
                error = Err(e);
                break;
            }
        };
        if uio.uio_offset + xfersize > ext2fs_size(ip) as Off {
            if let Err(e) = ext2fs_setsize(ip, (uio.uio_offset + xfersize) as u64) {
                // The C breaks with the buffer still busy (see the module's deviations).
                brelse(bp);
                error = Err(e);
                break;
            }
            uvm_vnp_setsize(vp, ext2fs_size(ip) as Off);
            extended = true;
        }
        let _ = uvm_vnp_uncache(vp);

        let size = bsize - bp.b_resid.get() as i64;
        if size < xfersize {
            xfersize = size;
        }

        {
            // SAFETY: the buffer is ours (busy from ext2fs_buf_alloc) and mapped; the slice
            // dies before it is written.
            let data = unsafe { bp.data() };
            let range = blkoffset as usize..(blkoffset + xfersize) as usize;
            error = uiomove(&mut data[range.clone()], uio);
            // If the buffer is not already filled and we encounter an error while trying to
            // fill it, we have to clear out any garbage data from the pages instantiated for
            // the buffer. If we do not, a failed uiomove() during a write can leave the
            // prior contents of the pages exposed to a userland mmap.
            //
            // Note that we don't need to clear buffers that were allocated with the
            // B_CLRBUF flag set.
            if error.is_err() && flags & B_CLRBUF == 0 {
                data[range].fill(0);
            }
        }

        if ioflag & IO_NOCACHE != 0 {
            bp.set(B_NOCACHE);
        }

        if ioflag & IO_SYNC != 0 {
            let _ = bwrite(bp);
        } else if xfersize + blkoffset == bsize {
            bawrite(bp);
        } else {
            bdwrite(bp);
        }
        if error.is_err() || xfersize == 0 {
            break;
        }
        ip.set_flag(IN_CHANGE | IN_UPDATE);
    }
    // If we successfully wrote any data, and we are not the superuser we clear the setuid
    // and setgid bits as a precaution against tampering.
    // SAFETY: the write's credentials are held by its caller (`cred_ref`'s contract).
    let cred = unsafe { cred_ref(ap.a_cred) };
    if resid > uio.uio_resid && cred.is_some_and(|c| c.cr_uid.get() != 0) {
        ip.set_i_e2fs_mode(ip.i_e2fs_mode() & !((ISUID | ISGID) as u16));
    }
    if resid > uio.uio_resid {
        VN_KNOTE(vp, NOTE_WRITE | if extended { NOTE_EXTEND } else { 0 });
    }
    if error.is_err() {
        if ioflag & IO_UNIT != 0 {
            let _ = ext2fs_truncate(ip, osize, ioflag & IO_SYNC, ap.a_cred);
            uio.uio_offset -= (resid - uio.uio_resid) as i64;
            uio.uio_resid = resid;
        }
    } else if resid > uio.uio_resid && ioflag & IO_SYNC != 0 {
        error = ext2fs_update(ip, 1);
    }
    // correct the result for writes clamped by vn_fsizechk()
    uio.uio_resid = (uio.uio_resid as isize + overrun) as usize;
    error
}
/* </CODE> */
