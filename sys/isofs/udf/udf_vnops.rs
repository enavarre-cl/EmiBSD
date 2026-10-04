/*	$OpenBSD: udf_vnops.c,v 1.76 2026/06/30 14:04:03 kirill Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2001, 2002 Scott Long <scottl@freebsd.org>
 * All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * $FreeBSD: src/sys/fs/udf/udf_vnops.c,v 1.50 2005/01/28 14:42:16 phk Exp $
 */
/* </LICENSES> */

//! UDF vnode operations: the unode hash, attributes and access, `read`, `readdir` and
//! `lookup` over the file identifier descriptors of a directory, the block map, and the
//! vnode lock (the unode's recursive rwlock, as `ufs_lock` does it).
//!
//! Upstream: sys/isofs/udf/udf_vnops.c @ 3ce1f3f79392
//!
//! Ported to OpenBSD by Pedro Martelletto in February 2005.
//!
//! ## Deviations
//! - `udf_readatoffset` returns where its data starts ([`UdfData`]) and fills the buffer slot
//!   `bp` (the C's `*bp`, which it may set on error too); [`udf_data`] turns the pair into the
//!   slice the C's `*data` points at. The size it returns is clamped to what the buffer (or
//!   the file entry) holds past the data's start: the C may hand out bytes past the buffer
//!   when the offset is not block aligned.
//! - For a file recorded in its file entry, `udf_readatoffset` returns the data from
//!   `offset` on; the C returns it from its start whatever the offset, so a `read` that does
//!   not start at 0, a directory read resumed past its start and a VAT lookup past the first
//!   entry got the wrong bytes.
//! - `udf_bmap_internal` returns [`UdfBmap`]: a block, or `UDF_INVALID_BMAP` (the file data
//!   is in the file entry) as its own variant. `udf_bmap` passes that case on as the C does:
//!   the C returns -1, `ERESTART`'s value ([`UDF_INVALID_BMAP`]). An allocation descriptor
//!   read past the end of the file entry's copy reads zeros (the C reads past an extended
//!   file entry's allocation).
//! - `udf_transname` returns 0 (failure) when `destname` cannot hold the name and its NUL;
//!   `udf_cmpname` returns a `bool` (`true` where the C returns nonzero: no match).
//! - `udf_uiodir` returns `Ok(false)` where the C returns -1 (no room for the entry); where
//!   the C's `readdir` breaks out with that -1 (the "." entry) and returns it, this returns
//!   `ERESTART`, its value. The bytes of a record past the name's NUL are zero (the C copies
//!   what an earlier, longer name left in `d_name`).
//! - The directory stream is a `udf_ds_pool` item handed around as `&'static mut`; its FID
//!   fragment buffer is freed by `udf_closedir` on every path (the C leaks it when reading
//!   the next extent fails or the FID is too long). A FID that would start past the end of
//!   its chunk is an invalid fragment (`EINVAL`), as one longer than a block is.
//! - `udf_read` stops on a zero-length chunk (an embedded file whose allocation
//!   descriptor area is empty), where the C would loop for ever; `uiomove` copies out of a
//!   file entry through a small bounce buffer (its source must be writable here).
//! - `udf_timetotimespec` ignores months past December (the C reads past `mon_lens`).
//! - `udf_strategy` gives the buffer the device's number (`u_devvp`'s `v_rdev`); the C reads
//!   `v_rdev` of the file's own vnode, which has no `specinfo`.
//! - `udf_getattr`'s `vp->v_type & VDIR` keeps the C's bitwise test of the enumeration
//!   (`VBLK`, `VSOCK` and `VFIFO` share `VDIR`'s bit).
//! - `udf_print` prints under feature `debug` or `diagnostic` (`VFSLCKDEBUG` has none); the
//!   `DIAGNOSTIC` messages of `udf_transname` under feature `diagnostic`.

use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::crypto::siphash::SipHash24;
use crate::isofs::udf::ecma167_udf::{
    ExtfileEntry, FileEntry, FileidDesc, LongAd, Packed, ShortAd, TAGID_EXTFENTRY, TAGID_FID,
    Timestamp, UDF_EXTFENTRY_SIZE, UDF_FENTRY_PERM_GRP_MASK, UDF_FENTRY_PERM_OWNER_MASK,
    UDF_FENTRY_PERM_USER_MASK, UDF_FENTRY_SIZE, UDF_FID_SIZE, UDF_FILE_CHAR_DEL, UDF_FILE_CHAR_DIR,
    UDF_FILE_CHAR_PAR, UDF_ICB_TAG_FLAGS_SETGID, UDF_ICB_TAG_FLAGS_SETUID,
    UDF_ICB_TAG_FLAGS_STICKY, geticblen,
};
use crate::isofs::udf::udf::{
    UDF_MNT_USES_META, UdfData, UdfDirstream, Udfino, Umount, Unicode, Unode, UnodeHash, VTOU,
    udf_getid, udf_readlblks,
};
use crate::isofs::udf::udf_subr::udf_rawnametounicode;
use crate::isofs::udf::udf_vfsops::{
    UDF_DS_POOL, UDF_TRANS_POOL, UNODE_POOL, udf_checktag, udf_vget,
};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{rrw_enter, rrw_exit, rrw_status};
use crate::kern::kern_subr::uiomove;
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::{panic, printf};
use crate::kern::subr_xxx::eopnotsupp;
use crate::kern::vfs_bio::{biodone, brelse};
use crate::kern::vfs_cache::{NCHSTATS, cache_enter, cache_lookup};
use crate::kern::vfs_subr::{getnewvnode, vaccess, vget, vput, vref, vrele};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_BMAP, VOP_STRATEGY, VOP_UNLOCK};
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::{B_ERROR, Buf, clrbuf};
use crate::sys::dirent::{DT_DIR, DT_UNKNOWN, Dirent, MAXNAMLEN, dirent_size};
use crate::sys::endian::{letoh16, letoh32, letoh64};
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY, LK_RWFLAGS};
use crate::sys::malloc::{M_UDFFENTRY, M_UDFFID, M_WAITOK, M_ZERO};
use crate::sys::mount::Mount;
use crate::sys::namei::{
    CREATE, ISDOTDOT, ISLASTCN, LOCKPARENT, LOOKUP, MAKEENTRY, PDIRUNLOCK, RENAME,
};
use crate::sys::param::{DEV_BSHIFT, MAXBSIZE};
use crate::sys::pool::{PR_WAITOK, PR_ZERO};
use crate::sys::proc::Proc;
use crate::sys::syslimits::NAME_MAX;
use crate::sys::time::Timespec;
use crate::sys::types::{Daddr, Gid, Ino, Mode, Nlink, Off, Register, Uid};
use crate::sys::ucred::Ucred;
use crate::sys::uio::Uio;
use crate::sys::unistd::{
    _PC_CHOWN_RESTRICTED, _PC_LINK_MAX, _PC_NAME_MAX, _PC_NO_TRUNC, _PC_TIMESTAMP_RESOLUTION,
};
use crate::sys::vnode::{
    VDIR, VEXEC, VLNK, VREG, VT_UDF, VWRITE, Vnode, VopAccessArgs, VopBmapArgs, VopCloseArgs,
    VopGetattrArgs, VopInactiveArgs, VopIoctlArgs, VopIslockedArgs, VopLockArgs, VopLookupArgs,
    VopOpenArgs, VopPathconfArgs, VopPrintArgs, VopReadArgs, VopReaddirArgs, VopReadlinkArgs,
    VopReclaimArgs, VopStrategyArgs, VopUnlockArgs, Vops, cred_ref,
};

/// `udf_vops`.
pub static UDF_VOPS: Vops = Vops {
    vop_access: Some(udf_access),
    vop_bmap: Some(udf_bmap),
    vop_lookup: Some(udf_lookup),
    vop_getattr: Some(udf_getattr),
    vop_open: Some(udf_open),
    vop_close: Some(udf_close),
    vop_ioctl: Some(udf_ioctl),
    vop_read: Some(udf_read),
    vop_readdir: Some(udf_readdir),
    vop_readlink: Some(udf_readlink),
    vop_inactive: Some(udf_inactive),
    vop_reclaim: Some(udf_reclaim),
    vop_strategy: Some(udf_strategy),
    vop_lock: Some(udf_lock),
    vop_unlock: Some(udf_unlock),
    vop_pathconf: Some(udf_pathconf),
    vop_islocked: Some(udf_islocked),
    vop_print: Some(udf_print),

    vop_abortop: None,
    vop_advlock: None,
    vop_bwrite: None,
    vop_create: None,
    vop_fsync: None,
    vop_link: None,
    vop_mknod: None,
    vop_remove: Some(|_| eopnotsupp()),
    vop_rename: None,
    vop_revoke: None,
    vop_mkdir: None,
    vop_rmdir: None,
    vop_setattr: None,
    vop_symlink: None,
    vop_write: None,
    vop_kqfilter: None,
};

/// `UDF_INVALID_BMAP`: the C's -1, which `udf_bmap` returns as an error (`ERESTART` has its
/// value).
pub const UDF_INVALID_BMAP: Errno = Errno::ERESTART;

/// What `udf_bmap_internal` found for an offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UdfBmap {
    /// The physical sector (`*sector`) and the length of its extent (`*max_size`).
    Block {
        /// `*sector`.
        sector: Daddr,
        /// `*max_size`.
        max_size: u32,
    },
    /// `UDF_INVALID_BMAP`: the file data is stored in the allocation descriptor field of the
    /// file entry, recorded at `*sector` (`*max_size` is 0).
    Invalid {
        /// `*sector`: the file entry's.
        sector: Daddr,
    },
}

/// `&ump->um_hashtbl[SipHash24(&ump->um_hashkey, &id, sizeof(id)) & ump->um_hashsz]`, `None`
/// when the mount has no table.
fn udf_hashchain(ump: &Umount, id: Udfino) -> Option<&'static crate::isofs::udf::udf::UdfHashLh> {
    let tbl = ump.um_hashtbl.get()?;
    let key = ump.um_hashkey.get();
    let h = SipHash24(&key, &id.to_ne_bytes()) & ump.um_hashsz.get();
    tbl.get(h as usize)
}

/// Look up a unode based on the `udfino_t` passed in and return its vnode, referenced and
/// locked with `flags`; `None` when it is not in the cache.
pub fn udf_hashlookup(
    ump: &Umount,
    id: Udfino,
    flags: i32,
) -> Result<Option<&'static Vnode>, Errno> {
    'loop_: loop {
        mtx_enter(&ump.um_hashmtx);
        let Some(lh) = udf_hashchain(ump, id) else {
            mtx_leave(&ump.um_hashmtx);
            return Err(Errno::ENOENT);
        };

        for up in lh.iter() {
            if up.u_ino.get() == id {
                let Some(vp) = up.u_vnode.get() else {
                    panic(format_args!("udf_hashlookup: unode without a vnode"));
                };
                let vpid = vp.v_id.get();
                mtx_leave(&ump.um_hashmtx);
                match vget(vp, flags) {
                    Err(Errno::ENOENT) => continue 'loop_,
                    Err(e) => return Err(e),
                    Ok(()) => {}
                }
                if vpid != vp.v_id.get() {
                    vput(vp);
                    continue 'loop_;
                }
                return Ok(Some(vp));
            }
        }

        mtx_leave(&ump.um_hashmtx);

        return Ok(None);
    }
}

/// `udf_hashins(up)`: lock the unode's vnode and put the unode on its hash chain.
pub fn udf_hashins(up: &'static Unode) -> Result<(), Errno> {
    let ump = up.ump();

    let Some(vp) = up.u_vnode.get() else {
        panic(format_args!("udf_hashins: unode without a vnode"));
    };
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    mtx_enter(&ump.um_hashmtx);
    let Some(lh) = udf_hashchain(ump, up.u_ino.get()) else {
        panic(format_args!(
            "hash entry is NULL, up->u_ino = {}",
            up.u_ino.get()
        ));
    };
    // SAFETY: a new unode (from `udf_vget`) is on no chain; it is a pool item that
    // `udf_reclaim` takes off the chain (`udf_hashrem`) before giving it back.
    unsafe { lh.insert_head(up) };
    mtx_leave(&ump.um_hashmtx);

    Ok(())
}

/// `udf_hashrem(up)`: take the unode off its hash chain.
pub fn udf_hashrem(up: &Unode) -> Result<(), Errno> {
    let ump = up.ump();

    mtx_enter(&ump.um_hashmtx);
    if udf_hashchain(ump, up.u_ino.get()).is_none() {
        panic(format_args!(
            "hash entry is NULL, up->u_ino = {}",
            up.u_ino.get()
        ));
    }
    // SAFETY: every unode with a vnode was put on its chain by `udf_hashins` in `udf_vget`,
    // and only `udf_reclaim` takes it off, once.
    unsafe { crate::sys::queue::ListHead::<UnodeHash>::remove(up) };
    mtx_leave(&ump.um_hashmtx);

    Ok(())
}

/// `udf_allocv(mp, vpp, p)`: a new vnode for a unode of `mp`.
pub fn udf_allocv(mp: &'static Mount, _p: Option<&Proc>) -> Result<&'static Vnode, Errno> {
    match getnewvnode(VT_UDF, Some(mp), &UDF_VOPS) {
        Ok(vp) => Ok(vp),
        Err(e) => {
            printf(format_args!("udf_allocv: failed to allocate new vnode\n"));
            Err(e)
        }
    }
}

/// Convert file entry permission (5 bits per owner/group/user) to a `mode_t`.
fn udf_permtomode(up: &Unode) -> Mode {
    let fe = up.fentry();
    let perm = letoh32(fe.perm);
    let flags = u32::from(letoh16(fe.icbtag.flags));

    let mut mode = perm & UDF_FENTRY_PERM_USER_MASK;
    mode |= (perm & UDF_FENTRY_PERM_GRP_MASK) >> 2;
    mode |= (perm & UDF_FENTRY_PERM_OWNER_MASK) >> 4;
    mode |= (flags & u32::from(UDF_ICB_TAG_FLAGS_STICKY)) << 4;
    mode |= (flags & u32::from(UDF_ICB_TAG_FLAGS_SETGID)) << 6;
    mode |= (flags & u32::from(UDF_ICB_TAG_FLAGS_SETUID)) << 8;

    mode
}

/// `*cred` of a credential the C dereferences: a real one (`NOCRED`/`FSCRED` panic).
fn ucred<'a>(cred: *const Ucred) -> &'a Ucred {
    // SAFETY: the credentials a vnode operation receives are held by its caller for the
    // operation's duration (`cred_ref`'s contract).
    match unsafe { cred_ref(cred) } {
        Some(c) => c,
        None => panic(format_args!("udf: credential {:p} is not a real one", cred)),
    }
}

/// `udf_access` (`vop_access`).
pub fn udf_access(ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let up = VTOU(vp);
    let a_mode = ap.a_mode;

    if a_mode & VWRITE != 0 {
        match vp.v_type.get() {
            VDIR | VLNK | VREG => return Err(Errno::EROFS),
            _ => {}
        }
    }

    let mode = udf_permtomode(up);

    let fe = up.fentry();
    vaccess(
        vp.v_type.get(),
        mode,
        { fe.uid } as Uid,
        { fe.gid } as Gid,
        a_mode,
        ucred(ap.a_cred),
    )
}

/// The month lengths of a common and of a leap year.
static MON_LENS: [[i32; 12]; 2] = [
    [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
    [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
];

/// 1 for a leap year, 0 otherwise.
fn udf_isaleapyear(year: i32) -> i32 {
    let mut i = if year % 4 != 0 { 0 } else { 1 };
    i &= if year % 100 != 0 { 1 } else { 0 };
    i |= if year % 400 != 0 { 0 } else { 1 };

    i
}

/// This is just a rough hack. Daylight savings isn't calculated and `tv_nsec` is ignored.
/// Timezone calculation compliments of Julian Elischer <julian@elischer.org>.
fn udf_timetotimespec(time: &Timestamp, t: &mut Timespec) {
    // DirectCD seems to like using bogus year values
    let year = i32::from(letoh16(time.year));
    if year < 1970 {
        t.tv_sec = 0;
        t.tv_nsec = 0;
        return;
    }

    // Calculate the time and day
    t.tv_nsec = 1000 * i64::from(time.usec)
        + 100_000 * i64::from(time.hund_usec)
        + 10_000_000 * i64::from(time.centisec);
    t.tv_sec = i64::from(time.second);
    t.tv_sec += i64::from(time.minute) * 60;
    t.tv_sec += i64::from(time.hour) * 3600;
    t.tv_sec += i64::from(time.day) * 3600 * 24;

    // Calculate the month
    let lpyear = udf_isaleapyear(year) as usize;
    for i in 1..usize::from(time.month) {
        let days = MON_LENS[lpyear].get(i).copied().unwrap_or(0);
        t.tv_sec += i64::from(days) * 3600 * 24;
    }

    // Speed up the calculation
    if year > 1979 {
        t.tv_sec += 315_532_800;
    }
    if year > 1989 {
        t.tv_sec += 315_619_200;
    }
    if year > 1999 {
        t.tv_sec += 315_532_800;
    }
    for i in 2000..year {
        let daysinyear = udf_isaleapyear(i) + 365;
        t.tv_sec += i64::from(daysinyear) * 3600 * 24;
    }

    // Calculate the time zone. The timezone is 12 bit signed 2's compliment, so we gotta do
    // some extra magic to handle it right.
    let mut tz = letoh16(time.type_tz) & 0x0fff;
    if tz & 0x0800 != 0 {
        tz |= 0xf000; // extend the sign to 16 bits
    }
    let s_tz_offset = tz as i16;
    if { time.type_tz } & 0x1000 != 0 && s_tz_offset != -2047 {
        t.tv_sec -= i64::from(s_tz_offset) * 60;
    }
}

/// `udf_getattr` (`vop_getattr`).
pub fn udf_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let vap = &mut *ap.a_vap;
    let up = VTOU(vp);
    let ump = up.ump();
    let bsize = ump.um_bsize.get() as u64;

    let xfentry = up.fentry();
    let fentry = up.fentry_fe();

    // The C tests `vp->v_type & VDIR`, a bitwise and of the enumeration (the module's
    // deviations).
    let dirlike = vp.v_type.get() as i32 & VDIR as i32 != 0;

    vap.va_fsid = i64::from(up.u_dev.get());
    vap.va_fileid = u64::from(up.u_ino.get());
    vap.va_mode = udf_permtomode(up);
    vap.va_nlink = Nlink::from(letoh16(fentry.link_cnt));
    // The spec says that -1 is valid for uid/gid and indicates an invalid uid/gid. How should
    // this be represented?
    let uid = letoh32(fentry.uid);
    vap.va_uid = if uid == u32::MAX { 0 } else { uid };
    let gid = letoh32(fentry.gid);
    vap.va_gid = if gid == u32::MAX { 0 } else { gid };
    vap.va_rdev = 0;
    if dirlike {
        vap.va_nlink += 1; // Count a reference to ourselves
        // Directories that are recorded within their ICB will show as having 0 blocks
        // recorded. Since tradition dictates that directories consume at least one logical
        // block, make it appear so.
        vap.va_size = bsize;
    } else {
        vap.va_size = letoh64(fentry.inf_len);
    }
    if udf_checktag(&xfentry.tag, TAGID_EXTFENTRY).is_ok() {
        udf_timetotimespec(&{ xfentry.atime }, &mut vap.va_atime);
        udf_timetotimespec(&{ xfentry.mtime }, &mut vap.va_mtime);
        if dirlike && { xfentry.logblks_rec } != 0 {
            vap.va_size = letoh64(xfentry.logblks_rec).wrapping_mul(bsize);
        }
    } else {
        udf_timetotimespec(&{ fentry.atime }, &mut vap.va_atime);
        udf_timetotimespec(&{ fentry.mtime }, &mut vap.va_mtime);
        if dirlike && { fentry.logblks_rec } != 0 {
            vap.va_size = letoh64(fentry.logblks_rec).wrapping_mul(bsize);
        }
    }
    vap.va_ctime = vap.va_mtime; // Stored as an Extended Attribute
    vap.va_flags = 0;
    vap.va_gen = 1;
    vap.va_blocksize = ump.um_bsize.get() as i64;
    vap.va_bytes = letoh64(fentry.inf_len);
    vap.va_type = vp.v_type.get();
    vap.va_filerev = 0;

    Ok(())
}

/// `udf_open` (`vop_open`): nothing to be done at this point.
pub fn udf_open(_ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    Ok(())
}

/// `udf_close` (`vop_close`): nothing to be done at this point.
pub fn udf_close(_ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    Ok(())
}

/// `udf_ioctl` (`vop_ioctl`): file specific ioctls.
pub fn udf_ioctl(_ap: &mut VopIoctlArgs<'_>) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

/// `udf_pathconf` (`vop_pathconf`). I'm not sure that this has much value in a read-only
/// filesystem, but cd9660 has it too.
pub fn udf_pathconf(ap: &mut VopPathconfArgs<'_>) -> Result<(), Errno> {
    *ap.a_retval = match ap.a_name {
        _PC_LINK_MAX => 65535,
        _PC_NAME_MAX => NAME_MAX as Register,
        _PC_CHOWN_RESTRICTED => 1,
        _PC_NO_TRUNC => 1,
        _PC_TIMESTAMP_RESOLUTION => 1000, // 1 microsecond
        _ => return Err(Errno::EINVAL),
    };

    Ok(())
}

/// `uiomove(data, size, uio)` out of read-only bytes (a file entry), through a bounce
/// buffer: `uiomove` wants a writable source.
fn uiomove_from(src: &[u8], uio: &mut Uio<'_>) -> Result<(), Errno> {
    let mut bounce = [0u8; 256];
    for chunk in src.chunks(bounce.len()) {
        if uio.uio_resid == 0 {
            break;
        }
        let b = &mut bounce[..chunk.len()];
        b.copy_from_slice(chunk);
        uiomove(b, uio)?;
    }
    Ok(())
}

/// `udf_read` (`vop_read`).
pub fn udf_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let uio = &mut *ap.a_uio;
    let up = VTOU(vp);

    if uio.uio_offset < 0 {
        return Err(Errno::EINVAL);
    }

    let fsize = letoh64(up.fentry().inf_len) as Off;

    while uio.uio_offset < fsize && uio.uio_resid > 0 {
        let offset = uio.uio_offset;
        let mut size = uio.uio_resid.min(MAXBSIZE) as i32;
        if Off::from(size) > fsize - offset {
            size = (fsize - offset) as i32;
        }
        let mut bp = None;
        let error = udf_readatoffset(up, &mut size, offset, &mut bp).and_then(|data| {
            if size <= 0 {
                return Ok(false);
            }
            // SAFETY: `bp` is the buffer `udf_readatoffset` returned, busy for us until the
            // release below; no other slice of it is alive.
            let d = unsafe { udf_data(up, bp, data, size) };
            uiomove_from(d, uio).map(|()| true)
        });
        if let Some(b) = bp.take() {
            brelse(b);
        }
        match error {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

/// Translate the name from a CS0 dstring to a 16-bit Unicode String. Hooks need to be placed
/// in here to translate from Unicode to the encoding that the kernel/user expects. Return the
/// length of the translated string (0 on failure).
pub fn udf_transname(
    cs0string: &[u8],
    destname: &mut [u8],
    len: i32,
    _ump: Option<&Umount>,
) -> usize {
    if len > MAXNAMLEN as i32 {
        #[cfg(feature = "diagnostic")]
        printf(format_args!("udf_transname(): name too long\n"));
        return 0;
    }

    // allocate a buffer big enough to hold an 8->16 bit expansion
    let Some(mem) = pool_get(&UDF_TRANS_POOL, PR_WAITOK) else {
        panic(format_args!("udf_transname: no buffer"));
    };
    // SAFETY: a `udf_trans_pool` item is `MAXNAMLEN` unicode characters, aligned to at least
    // `ALIGNBYTES + 1` (`pool_init`), ours until the `pool_put` below.
    let transname: &mut [Unicode] =
        unsafe { core::slice::from_raw_parts_mut(mem.as_ptr().cast::<Unicode>(), MAXNAMLEN) };

    let unilen = match udf_rawnametounicode(len as u32, cs0string, transname) {
        Ok(n) if n < destname.len() => n,
        _ => {
            #[cfg(feature = "diagnostic")]
            printf(format_args!(
                "udf_transname(): Unicode translation failed\n"
            ));
            pool_put(&UDF_TRANS_POOL, mem);
            return 0;
        }
    };

    // Pack it back to 8-bit Unicode.
    for (d, &t) in destname.iter_mut().zip(transname[..unilen].iter()) {
        *d = if t & 0xff00 != 0 {
            b'?' // Fudge the 16bit chars
        } else {
            (t & 0xff) as u8
        };
    }

    pool_put(&UDF_TRANS_POOL, mem);

    // Don't forget to terminate the string.
    destname[unilen] = 0;

    unilen
}

/// Compare a CS0 dstring with a name passed in from the VFS layer. Return `false` on a
/// successful match, `true` otherwise (the C's 0 and nonzero). Unicode work may need to be
/// done here also.
fn udf_cmpname(cs0string: &[u8], cmpname: &[u8], cs0len: i32, cmplen: usize, ump: &Umount) -> bool {
    // This is overkill, but not worth creating a new pool
    let Some(mem) = pool_get(&UDF_TRANS_POOL, PR_WAITOK) else {
        panic(format_args!("udf_cmpname: no buffer"));
    };
    // SAFETY: a `udf_trans_pool` item is `MAXNAMLEN * sizeof(unicode_t)` bytes, ours until
    // the `pool_put` below.
    let transname: &mut [u8] =
        unsafe { core::slice::from_raw_parts_mut(mem.as_ptr(), MAXNAMLEN * size_of::<Unicode>()) };

    let cs0len = udf_transname(cs0string, transname, cs0len, Some(ump));

    // Easy check. If they aren't the same length, they aren't equal
    let error = if cs0len == 0 || cs0len != cmplen {
        true
    } else {
        transname.get(..cmplen) != cmpname.get(..cmplen)
    };

    pool_put(&UDF_TRANS_POOL, mem);

    error
}

/// `struct udf_uiodir`.
struct UdfUiodir {
    /// `dirent`: the entry being built.
    dirent: Dirent,
    /// `eofflag`.
    eofflag: i32,
}

/// The first `d_reclen` bytes of a `struct dirent`, as `uiomove(dirent, de_size, uio)` copies
/// them out.
fn dirent_bytes(dp: &Dirent) -> ([u8; size_of::<Dirent>()], usize) {
    let mut b = [0u8; size_of::<Dirent>()];
    let reclen = usize::from(dp.d_reclen).min(b.len());
    b[0..8].copy_from_slice(&dp.d_fileno.to_ne_bytes());
    b[8..16].copy_from_slice(&dp.d_off.to_ne_bytes());
    b[16..18].copy_from_slice(&dp.d_reclen.to_ne_bytes());
    b[18] = dp.d_type;
    b[19] = dp.d_namlen;
    let namlen = usize::from(dp.d_namlen);
    b[Dirent::NAME_OFFSET..Dirent::NAME_OFFSET + namlen].copy_from_slice(&dp.d_name[..namlen]);
    (b, reclen)
}

/// Copy the entry out with `d_off = off`; `Ok(false)` (the C's -1) when it does not fit.
fn udf_uiodir(uiodir: &mut UdfUiodir, uio: &mut Uio<'_>, off: i64) -> Result<bool, Errno> {
    let de_size = dirent_size(&uiodir.dirent);

    if uio.uio_resid < de_size {
        uiodir.eofflag = 0;
        return Ok(false);
    }
    uiodir.dirent.d_off = off;
    uiodir.dirent.d_reclen = de_size as u16;

    let namlen = usize::from(uiodir.dirent.d_namlen);
    if uiodir.dirent.d_name[..namlen].contains(&b'/') {
        // illegal file name
        return Err(Errno::EINVAL);
    }

    let (mut b, n) = dirent_bytes(&uiodir.dirent);
    uiomove(&mut b[..n], uio).map(|()| true)
}

/// `udf_opendir(up, offset, fsize, ump)`: a directory stream over `up` from `offset`.
#[allow(clippy::mut_from_ref)] // a fresh pool item, not a borrow of the arguments
fn udf_opendir(
    up: &'static Unode,
    offset: i32,
    fsize: i32,
    ump: &'static Umount,
) -> &'static mut UdfDirstream {
    let Some(mem) = pool_get(&UDF_DS_POOL, PR_WAITOK | PR_ZERO) else {
        panic(format_args!("udf_opendir: no stream"));
    };
    let ds = mem.cast::<UdfDirstream>();
    // SAFETY: a fresh `udf_ds_pool` item, sized and aligned for a stream, written once and
    // ours alone until `udf_closedir` gives it back.
    unsafe {
        ds.as_ptr().write(UdfDirstream {
            node: up,
            ump,
            bp: None,
            data: UdfData::Buf(0),
            buf: None,
            fsize,
            off: 0,
            this_off: 0,
            offset,
            size: 0,
            error: Ok(()),
            fid_fragment: false,
        });
        &mut *ds.as_ptr()
    }
}

/// The current chunk of a stream: `ds->size` bytes at `ds->data`.
fn ds_chunk(ds: &UdfDirstream) -> &[u8] {
    // SAFETY: `ds->bp` is the buffer `udf_readatoffset` returned for this chunk, busy for the
    // stream until the stream releases it, which needs the `&mut` this slice borrows; only
    // shared slices of it are made.
    unsafe { udf_data(ds.node, ds.bp, ds.data, ds.size) }
}

/// The fragment buffer of a stream, `um_bsize` bytes.
///
/// # Safety
///
/// No other slice of the fragment buffer is alive while the returned one is.
#[allow(clippy::mut_from_ref)] // the stream's own allocation, as the C's ds->buf
unsafe fn ds_buf(ds: &UdfDirstream) -> &mut [u8] {
    match ds.buf {
        // SAFETY: `buf` is the stream's own zeroed `malloc` of `um_bsize` bytes, freed only
        // by `udf_getfid` or `udf_closedir`; the caller makes one slice of it at a time.
        Some(p) => unsafe {
            core::slice::from_raw_parts_mut(p.as_ptr(), ds.ump.um_bsize.get() as usize)
        },
        None => &mut [],
    }
}

/// The next file identifier descriptor of the stream, with its implementation use area and
/// its name (`UDF_FID_SIZE + l_iu + l_fi` bytes); `None` at the end or on an error
/// (`ds->error`).
fn udf_getfid(ds: &mut UdfDirstream) -> Option<&[u8]> {
    let bsize = ds.ump.um_bsize.get();

    // End of directory?
    if ds.offset + ds.off >= ds.fsize {
        ds.error = Ok(());
        return None;
    }

    // Grab the first extent of the directory
    if ds.off == 0 {
        ds.size = 0;
        let mut bp = None;
        match udf_readatoffset(ds.node, &mut ds.size, Off::from(ds.offset), &mut bp) {
            Ok(data) => {
                ds.bp = bp;
                ds.data = data;
            }
            Err(e) => {
                ds.error = Err(e);
                if let Some(b) = bp {
                    brelse(b);
                }
                ds.bp = None;
                return None;
            }
        }
    }

    // Clean up from a previous fragmented FID. Is this the right place for this?
    if ds.fid_fragment
        && let Some(buf) = ds.buf.take()
    {
        ds.fid_fragment = false;
        free(buf, M_UDFFID, bsize as usize);
    }

    let off = ds.off as usize;
    let size = ds.size as usize;

    // Check to see if the fid is fragmented. The first test ensures that we don't wander off
    // the end of the buffer looking for the l_iu and l_fi fields.
    let fragmented = off + UDF_FID_SIZE > size || {
        let chunk = ds_chunk(ds);
        FileidDesc::at(chunk, off).is_none_or(|fid| {
            off + usize::from(letoh16(fid.l_iu)) + usize::from(fid.l_fi) + UDF_FID_SIZE > size
        })
    };

    let (total_fid_size, frag_size) = if fragmented {
        // Copy what we have of the fid into a buffer
        let frag_size = ds.size - ds.off;
        if frag_size >= bsize || frag_size < 0 {
            printf(format_args!("udf: invalid FID fragment\n"));
            ds.error = Err(Errno::EINVAL);
            return None;
        }
        let frag = frag_size as usize;

        // File ID descriptors can only be at most one logical sector in size.
        let Some(buf) = malloc(bsize as usize, M_UDFFID, M_WAITOK | M_ZERO) else {
            panic(format_args!("udf_getfid: no memory"));
        };
        ds.buf = Some(buf);
        // The stream owns the buffer from here on: `udf_closedir` frees it on every path.
        ds.fid_fragment = true;
        {
            let chunk = ds_chunk(ds);
            let have = chunk.get(off..).unwrap_or(&[]);
            let n = frag.min(have.len());
            // SAFETY: the only slice of the fragment buffer; `have` is in the chunk.
            let buf = unsafe { ds_buf(ds) };
            buf[..n].copy_from_slice(&have[..n]);
        }

        if let Some(b) = ds.bp.take() {
            brelse(b);
        }

        // Fetch the next allocation
        ds.offset += ds.size;
        ds.size = 0;
        let mut bp = None;
        match udf_readatoffset(ds.node, &mut ds.size, Off::from(ds.offset), &mut bp) {
            Ok(data) => {
                ds.bp = bp;
                ds.data = data;
            }
            Err(e) => {
                ds.error = Err(e);
                if let Some(b) = bp {
                    brelse(b);
                }
                ds.bp = None;
                return None;
            }
        }

        // If the fragment was so small that we didn't get the l_iu and l_fi fields, copy
        // those in.
        if frag < UDF_FID_SIZE {
            let chunk = ds_chunk(ds);
            let n = (UDF_FID_SIZE - frag).min(chunk.len());
            // SAFETY: as above.
            let buf = unsafe { ds_buf(ds) };
            buf[frag..frag + n].copy_from_slice(&chunk[..n]);
        }

        // Now that we have enough of the fid to work with, copy in the rest of the fid from
        // the new allocation.
        let total = {
            // SAFETY: as above.
            let buf = unsafe { ds_buf(ds) };
            FileidDesc::at(buf, 0).map_or(usize::MAX, |fid| {
                UDF_FID_SIZE + usize::from(letoh16(fid.l_iu)) + usize::from(fid.l_fi)
            })
        };
        if total > bsize as usize {
            printf(format_args!("udf: invalid FID\n"));
            ds.error = Err(Errno::EIO);
            return None;
        }
        {
            let chunk = ds_chunk(ds);
            let n = (total - frag).min(chunk.len());
            // SAFETY: as above.
            let buf = unsafe { ds_buf(ds) };
            buf[frag..frag + n].copy_from_slice(&chunk[..n]);
        }
        (total, frag_size)
    } else {
        let chunk = ds_chunk(ds);
        let total = FileidDesc::at(chunk, off).map_or(UDF_FID_SIZE, |fid| {
            usize::from(letoh16(fid.l_iu)) + usize::from(fid.l_fi) + UDF_FID_SIZE
        });
        (total, 0)
    };

    // Update the offset. Align on a 4 byte boundary because the UDF spec says so.
    if !ds.fid_fragment {
        ds.off += ((total_fid_size + 3) & !0x03) as i32;
    } else {
        ds.off = ((total_fid_size - frag_size as usize + 3) & !0x03) as i32;
    }
    ds.this_off = ds.offset + ds.off;

    if ds.fid_fragment {
        // SAFETY: the one slice of the fragment buffer until the stream is used again (which
        // needs the `&mut` this slice borrows).
        Some(&unsafe { ds_buf(ds) }[..total_fid_size])
    } else {
        ds_chunk(ds).get(off..off + total_fid_size)
    }
}

/// `udf_closedir(ds)`: release the stream's buffers and give it back.
fn udf_closedir(ds: &'static mut UdfDirstream) {
    if let Some(b) = ds.bp.take() {
        brelse(b);
    }

    if ds.fid_fragment
        && let Some(buf) = ds.buf.take()
    {
        free(buf, M_UDFFID, ds.ump.um_bsize.get() as usize);
    }

    pool_put(&UDF_DS_POOL, NonNull::from(ds).cast::<u8>());
}

/// The `d_off` of the "." entry.
const SELF_OFFSET: i64 = 1;
/// The `d_off` of the ".." entry.
const PARENT_OFFSET: i64 = 2;

/// Where `udf_readdir` starts: from the beginning, or at "." or "..".
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReaddirMode {
    /// `MODE_NORMAL`.
    Normal,
    /// `MODE_SELF`.
    SelfEntry,
    /// `MODE_PARENT`.
    Parent,
}

/// `um_start += um_meta_start; um_len = um_meta_len` while a directory of a metadata
/// partition is read.
fn meta_enter(ump: &Umount) {
    if ump.um_flags.get() & UDF_MNT_USES_META != 0 {
        ump.um_start
            .set(ump.um_start.get().wrapping_add(ump.um_meta_start.get()));
        ump.um_len.set(ump.um_meta_len.get());
    }
}

/// Undoes [`meta_enter`].
fn meta_leave(ump: &Umount) {
    if ump.um_flags.get() & UDF_MNT_USES_META != 0 {
        ump.um_start.set(ump.um_realstart.get());
        ump.um_len.set(ump.um_reallen.get());
    }
}

/// A zeroed `struct dirent`.
const fn dirent_zero() -> Dirent {
    Dirent {
        d_fileno: 0,
        d_off: 0,
        d_reclen: 0,
        d_type: 0,
        d_namlen: 0,
        __d_padding: [0; 4],
        d_name: [0; MAXNAMLEN + 1],
    }
}

/// `udf_readdir` (`vop_readdir`).
pub fn udf_readdir(ap: &mut VopReaddirArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let uio = &mut *ap.a_uio;
    let up = VTOU(vp);
    let ump = up.ump();
    let mut uiodir = UdfUiodir {
        dirent: dirent_zero(),
        eofflag: 1,
    };
    let mut error: Result<(), Errno> = Ok(());

    // if asked to start at SELF_OFFSET or PARENT_OFFSET, search for the parent ref
    let mut mode = if uio.uio_offset == SELF_OFFSET {
        uio.uio_offset = 0;
        ReaddirMode::SelfEntry
    } else if uio.uio_offset == PARENT_OFFSET {
        uio.uio_offset = 0;
        ReaddirMode::Parent
    } else {
        ReaddirMode::Normal
    };

    // Iterate through the file id descriptors. Give the parent dir entry special attention.
    meta_enter(ump);
    let ds = udf_opendir(
        up,
        uio.uio_offset as i32,
        letoh64(up.fentry().inf_len) as i32,
        ump,
    );

    let mut last_off = ds.offset + ds.off;
    while let Some(fid) = udf_getfid(ds) {
        let Some(hdr) = FileidDesc::at(fid, 0).copied() else {
            break;
        };

        // Should we return an error on a bad fid?
        if udf_checktag(&hdr.tag, TAGID_FID).is_err() {
            printf(format_args!("Invalid FID tag ({})\n", { hdr.tag.id }));
            error = Err(Errno::EIO);
            break;
        }

        // Is this a deleted file?
        if hdr.file_char & UDF_FILE_CHAR_DEL != 0 {
            continue;
        }

        let mut r: Result<bool, Errno> = Ok(true);
        if hdr.l_fi == 0 && hdr.file_char & UDF_FILE_CHAR_PAR != 0 {
            // Do up the '.' and '..' entries. Dummy values are used for the offset since the
            // offset here is usually zero, and NFS doesn't like that value
            if mode == ReaddirMode::Normal {
                let d = &mut uiodir.dirent;
                d.d_fileno = Ino::from(up.u_ino.get());
                d.d_type = DT_DIR;
                d.d_name[0] = b'.';
                d.d_name[1] = 0;
                d.d_namlen = 1;
                match udf_uiodir(&mut uiodir, uio, SELF_OFFSET) {
                    Ok(true) => {}
                    // The C breaks out with udf_uiodir's -1 and returns it (the module's
                    // deviations).
                    Ok(false) => {
                        error = Err(Errno::ERESTART);
                        break;
                    }
                    Err(e) => {
                        error = Err(e);
                        break;
                    }
                }
            }
            if mode != ReaddirMode::Parent {
                let d = &mut uiodir.dirent;
                d.d_fileno = Ino::from(udf_getid(&{ hdr.icb }));
                d.d_type = DT_DIR;
                d.d_name[0] = b'.';
                d.d_name[1] = b'.';
                d.d_name[2] = 0;
                d.d_namlen = 2;
                r = udf_uiodir(&mut uiodir, uio, PARENT_OFFSET);
            }
            mode = ReaddirMode::Normal;
        } else if mode != ReaddirMode::Normal {
            continue;
        } else {
            let name = fid
                .get(UDF_FID_SIZE + usize::from(letoh16(hdr.l_iu))..)
                .unwrap_or(&[]);
            let d = &mut uiodir.dirent;
            d.d_namlen = udf_transname(name, &mut d.d_name, i32::from(hdr.l_fi), Some(ump)) as u8;
            d.d_fileno = Ino::from(udf_getid(&{ hdr.icb }));
            d.d_type = if hdr.file_char & UDF_FILE_CHAR_DIR != 0 {
                DT_DIR
            } else {
                DT_UNKNOWN
            };
            r = udf_uiodir(&mut uiodir, uio, i64::from(ds.this_off));
        }
        match r {
            Ok(true) => {}
            // udf_uiodir() indicates there isn't space for another entry by returning -1
            Ok(false) => break,
            Err(e) => {
                error = Err(e);
                break;
            }
        }
        last_off = ds.this_off;
    }

    // tell the calling layer whether we need to be called again
    *ap.a_eofflag = uiodir.eofflag;
    uio.uio_offset = Off::from(last_off);

    if error.is_ok() {
        error = ds.error;
    }

    udf_closedir(ds);
    meta_leave(ump);

    error
}

/// `udf_readlink` (`vop_readlink`). Are there any implementations out there that do
/// soft-links?
pub fn udf_readlink(_ap: &mut VopReadlinkArgs<'_, '_>) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `udf_strategy` (`vop_strategy`).
pub fn udf_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    let bp = ap.a_bp;
    let Some(vp) = bp.b_vp.get() else {
        panic(format_args!("udf_strategy: buffer without a vnode"));
    };
    let up = VTOU(vp);

    // cd9660 has this test reversed, but it seems more logical this way
    if bp.b_blkno.get() != bp.b_lblkno.get() {
        // Files that are embedded in the fentry don't translate well to a block number.
        // Reject.
        let off = bp
            .b_lblkno
            .get()
            .wrapping_mul(Daddr::from(up.ump().um_bsize.get()));
        let invalid = match udf_bmap_internal(up, off) {
            Ok(UdfBmap::Block { sector, .. }) => {
                bp.b_lblkno.set(sector);
                false
            }
            Ok(UdfBmap::Invalid { sector }) => {
                bp.b_lblkno.set(sector);
                true
            }
            Err(_) => true,
        };
        if invalid {
            // SAFETY: a buffer handed to the strategy routine is busy for this I/O and
            // mapped; the strategy owns it until biodone.
            unsafe { clrbuf(bp) };
            bp.b_blkno.set(-1);
        }
    } else {
        let mut blkno = bp.b_blkno.get();
        let error = VOP_BMAP(vp, bp.b_lblkno.get(), None, Some(&mut blkno), None);
        bp.b_blkno.set(blkno);
        if let Err(e) = error {
            bp.b_error.set(Some(e));
            bp.set(B_ERROR);
            let s = splbio();
            biodone(bp);
            splx(s);
            return Err(e);
        }

        if bp.b_blkno.get() == -1 {
            // SAFETY: as above.
            unsafe { clrbuf(bp) };
        }
    }

    if bp.b_blkno.get() == -1 {
        let s = splbio();
        biodone(bp);
        splx(s);
    } else {
        let devvp = up.devvp();
        bp.b_dev.set(devvp.v_rdev());
        let _ = VOP_STRATEGY(devvp, bp);
    }

    Ok(())
}

/// `udf_lock` (`vop_lock`).
pub fn udf_lock(ap: &mut VopLockArgs) -> Result<(), Errno> {
    rrw_enter(&VTOU(ap.a_vp).u_lock, ap.a_flags & LK_RWFLAGS)
}

/// `udf_unlock` (`vop_unlock`).
pub fn udf_unlock(ap: &mut VopUnlockArgs) -> Result<(), Errno> {
    rrw_exit(&VTOU(ap.a_vp).u_lock);
    Ok(())
}

/// `udf_islocked` (`vop_islocked`).
pub fn udf_islocked(ap: &mut VopIslockedArgs) -> i32 {
    rrw_status(&VTOU(ap.a_vp).u_lock)
}

/// `udf_print` (`vop_print`): complete the information given by `vprint()`.
pub fn udf_print(ap: &mut VopPrintArgs) -> Result<(), Errno> {
    #[cfg(any(feature = "debug", feature = "diagnostic"))]
    {
        let vp = ap.a_vp;
        let up = VTOU(vp);

        printf(format_args!("tag VT_UDF, hash id {}\n", up.u_ino.get()));
        #[cfg(feature = "diagnostic")]
        printf(format_args!("\n"));
    }
    #[cfg(not(any(feature = "debug", feature = "diagnostic")))]
    let _ = ap;

    Ok(())
}

/// `udf_bmap` (`vop_bmap`).
pub fn udf_bmap(ap: &mut VopBmapArgs<'_>) -> Result<(), Errno> {
    let up = VTOU(ap.a_vp);
    let ump = up.ump();

    if let Some(vpp) = ap.a_vpp.as_deref_mut() {
        *vpp = up.u_devvp.get();
    }
    let Some(bnp) = ap.a_bnp.as_deref_mut() else {
        return Ok(());
    };

    let lsector =
        match udf_bmap_internal(up, ap.a_bn.wrapping_mul(Daddr::from(ump.um_bsize.get())))? {
            UdfBmap::Block { sector, .. } => sector,
            UdfBmap::Invalid { .. } => return Err(UDF_INVALID_BMAP),
        };

    // Translate logical to physical sector number
    *bnp = lsector.wrapping_shl((ump.um_bshift.get() - DEV_BSHIFT as i32) as u32);

    // Punt on read-ahead for now
    if let Some(runp) = ap.a_runp.as_deref_mut() {
        *runp = 0;
    }

    Ok(())
}

/// `udf_lookup` (`vop_lookup`): the all powerful VOP_LOOKUP().
pub fn udf_lookup(ap: &mut VopLookupArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let up = VTOU(dvp);
    let ump = up.ump();
    let cnp = &mut *ap.a_cnp;
    let nameiop = cnp.cn_nameiop;
    let flags = cnp.cn_flags;
    let namelen = usize::try_from(cnp.cn_namelen).unwrap_or(0);
    let mut namebuf = [0u8; NAME_MAX + 1];
    {
        let name = cnp.name();
        let n = name.len().min(NAME_MAX);
        namebuf[..n].copy_from_slice(&name[..n]);
    }
    let nameptr = &namebuf[..namelen.min(NAME_MAX)];
    let fsize = letoh64(up.fentry().inf_len) as i32;
    *ap.a_vpp = None;

    // Make sure the process can scan the requested directory.
    VOP_ACCESS(dvp, VEXEC, cnp.cn_cred, cnp.proc())?;

    // Check if the (directory, name) tuple has been already cached.
    if let Some(vp) = cache_lookup(dvp, cnp)? {
        *ap.a_vpp = Some(vp);
        return Ok(());
    }

    // If dvp is what's being looked up, then return it.
    if namelen == 1 && nameptr[0] == b'.' {
        vref(dvp);
        *ap.a_vpp = Some(dvp);
        return Ok(());
    }

    // If this is a LOOKUP and we've already partially searched through the directory, pick
    // up where we left off and flag that the directory may need to be searched twice. For a
    // full description, see /sys/isofs/cd9660/cd9660_lookup.c:cd9660_lookup()
    let diroff = up.u_diroff();
    let (mut offset, mut numdirpasses) =
        if nameiop != LOOKUP || diroff == 0 || diroff > i64::from(fsize) {
            (0, 1)
        } else {
            NCHSTATS.ncs_2passes.fetch_add(1, Ordering::Relaxed);
            (diroff as i32, 2)
        };

    meta_enter(ump);
    let mut id: Udfino = 0;
    let ds = loop {
        // lookloop:
        let ds = udf_opendir(up, offset, fsize, ump);
        let mut error: Result<(), Errno> = Ok(());

        while let Some(fid) = udf_getfid(ds) {
            let Some(hdr) = FileidDesc::at(fid, 0).copied() else {
                break;
            };

            // Check for a valid FID tag.
            if udf_checktag(&hdr.tag, TAGID_FID).is_err() {
                printf(format_args!("udf_lookup: Invalid tag\n"));
                error = Err(Errno::EIO);
                break;
            }

            // Is this a deleted file?
            if hdr.file_char & UDF_FILE_CHAR_DEL != 0 {
                continue;
            }

            if hdr.l_fi == 0 && hdr.file_char & UDF_FILE_CHAR_PAR != 0 {
                if flags & ISDOTDOT != 0 {
                    id = udf_getid(&{ hdr.icb });
                    break;
                }
            } else {
                let name = fid
                    .get(UDF_FID_SIZE + usize::from(letoh16(hdr.l_iu))..)
                    .unwrap_or(&[]);
                if !udf_cmpname(name, nameptr, i32::from(hdr.l_fi), namelen, ump) {
                    id = udf_getid(&{ hdr.icb });
                    break;
                }
            }
        }

        if error.is_ok() {
            error = ds.error;
        }

        if let Err(e) = error {
            udf_closedir(ds);
            meta_leave(ump);
            return Err(e);
        }

        // Did we have a match? If not, do another pass?
        if id == 0 && numdirpasses == 2 {
            numdirpasses -= 1;
            offset = 0;
            udf_closedir(ds);
            continue;
        }
        break ds;
    };

    let mut error: Result<(), Errno> = Ok(());
    if id != 0 {
        match udf_vget(ump.mountp(), Ino::from(id)) {
            Ok(tdp) => {
                // Remember where this entry was if it's the final component.
                if flags & ISLASTCN != 0 && nameiop == LOOKUP {
                    up.set_u_diroff(i64::from(ds.offset + ds.off));
                }
                if numdirpasses == 2 {
                    NCHSTATS.ncs_pass2.fetch_add(1, Ordering::Relaxed);
                }
                if flags & LOCKPARENT == 0 || flags & ISLASTCN == 0 {
                    cnp.cn_flags |= PDIRUNLOCK;
                    let _ = VOP_UNLOCK(dvp);
                }

                *ap.a_vpp = Some(tdp);
            }
            Err(e) => error = Err(e),
        }
    } else if flags & ISLASTCN != 0 && (nameiop == CREATE || nameiop == RENAME) {
        error = Err(Errno::EROFS);
    } else {
        error = Err(Errno::ENOENT);
    }

    // Cache the result of this lookup.
    if flags & MAKEENTRY != 0 {
        cache_enter(dvp, *ap.a_vpp, cnp);
    }

    udf_closedir(ds);
    meta_leave(ump);

    error
}

/// `udf_inactive` (`vop_inactive`): no need to sync anything, so just unlock the vnode and
/// return.
pub fn udf_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    let _ = VOP_UNLOCK(ap.a_vp);

    Ok(())
}

/// `udf_reclaim` (`vop_reclaim`).
pub fn udf_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let data = vp.v_data.get();

    if !data.is_null() {
        let up = VTOU(vp);
        let _ = udf_hashrem(up);
        if let Some(devvp) = up.u_devvp.take() {
            vrele(devvp);
        }

        if let Some(fe) = up.u_fentry.take() {
            free(fe, M_UDFFENTRY, up.u_fentry_len.get());
        }

        // SAFETY: `v_data` is the `unode_pool` item `udf_vget` hung there (VTOU checked it);
        // nothing refers to it once it is off its chain and `v_data` is cleared below.
        pool_put(&UNODE_POOL, unsafe {
            NonNull::new_unchecked(data.cast::<u8>())
        });
        vp.v_data.set(ptr::null_mut());
    }

    Ok(())
}

/// The `size` bytes `udf_readatoffset` pointed its `*data` at.
///
/// # Safety
///
/// For `UdfData::Buf`, `bp` is the buffer the same `udf_readatoffset` call returned, still
/// busy for the caller and not released while the returned slice lives, and no mutable
/// slice of its data is alive meanwhile.
pub unsafe fn udf_data<'a>(
    up: &'a Unode,
    bp: Option<&'a Buf>,
    data: UdfData,
    size: i32,
) -> &'a [u8] {
    let size = usize::try_from(size).unwrap_or(0);
    let (bytes, off): (&[u8], usize) = match data {
        UdfData::Fentry(off) => (up.fentry_bytes(), off),
        UdfData::Buf(off) => match bp {
            Some(bp) => {
                let len = usize::try_from(bp.b_bcount.get()).unwrap_or(0);
                // SAFETY: the caller's contract: the buffer is busy for the caller and mapped
                // (`b_data` holds `b_bcount` bytes, `buf_map`), and nothing writes it while
                // the shared slice lives.
                (
                    unsafe { core::slice::from_raw_parts(bp.b_data.get(), len) },
                    off,
                )
            }
            None => (&[], 0),
        },
    };
    let end = off.saturating_add(size).min(bytes.len());
    bytes.get(off..end).unwrap_or(&[])
}

/// Read the block and then set the data pointer to correspond with the offset passed in.
/// Only read in at most `size` bytes, and then set `size` to the number of bytes pointed to.
/// If `size` is zero, try to read in a whole extent.
///
/// Note that `*bp` may be assigned error or not.
pub fn udf_readatoffset(
    up: &Unode,
    size: &mut i32,
    offset: Off,
    bp: &mut Option<&'static Buf>,
) -> Result<UdfData, Errno> {
    let ump = up.ump();

    *bp = None;
    let (sector, max_size) = match udf_bmap_internal(up, offset)? {
        UdfBmap::Invalid { .. } => {
            // This error means that the file *data* is stored in the allocation descriptor
            // field of the file entry.
            let (off, l_ad) = if udf_checktag(&up.fentry().tag, TAGID_EXTFENTRY).is_ok() {
                let xfentry = up.fentry();
                (
                    UDF_EXTFENTRY_SIZE as u64 + u64::from(letoh32(xfentry.l_ea)),
                    letoh32(xfentry.l_ad),
                )
            } else {
                let fentry = up.fentry_fe();
                (
                    UDF_FENTRY_SIZE as u64 + u64::from(letoh32(fentry.l_ea)),
                    letoh32(fentry.l_ad),
                )
            };
            // The data from `offset` on (the module's deviations).
            let skip = (offset.max(0) as u64).min(u64::from(l_ad));
            let len = up.fentry_bytes().len() as u64;
            let off = (off + skip).min(len);
            *size = ((u64::from(l_ad) - skip).min(len - off)) as i32;
            return Ok(UdfData::Fentry(off as usize));
        }
        UdfBmap::Block { sector, max_size } => (sector, max_size),
    };

    // Adjust the size so that it is within range
    if *size == 0 || *size as u32 > max_size {
        *size = max_size as i32;
    }
    *size = (*size as u32).min(MAXBSIZE as u32) as i32;

    let (b, error) = udf_readlblks(ump, sector as i32, *size);
    *bp = Some(b);
    if let Err(e) = error {
        printf(format_args!(
            "warning: udf_readlblks returned error {}\n",
            e as i32
        ));
        // note: *bp may be non-NULL
        return Err(e);
    }

    let off = offset
        .checked_rem(Off::from(ump.um_bsize.get()))
        .unwrap_or(0) as usize;
    let avail = usize::try_from(b.b_bcount.get())
        .unwrap_or(0)
        .saturating_sub(off);
    *size = (*size as u32).min(avail as u32) as i32;
    Ok(UdfData::Buf(off))
}

/// `N` bytes of the node's file entry copy from `off` on, zeros past its end.
fn fentry_read<const N: usize>(up: &Unode, off: i64) -> [u8; N] {
    let mut out = [0u8; N];
    let bytes = up.fentry_bytes();
    if let Ok(off) = usize::try_from(off)
        && let Some(src) = bytes.get(off..)
    {
        let n = src.len().min(N);
        out[..n].copy_from_slice(&src[..n]);
    }
    out
}

/// Translate a file offset into a logical block and then into a physical block.
pub fn udf_bmap_internal(up: &Unode, offset: Off) -> Result<UdfBmap, Errno> {
    let ump = up.ump();
    let xfentry: &ExtfileEntry = up.fentry();
    let fentry: &FileEntry = up.fentry_fe();
    let tag = fentry.icbtag;
    let (l_ea, l_ad, hdr) = if udf_checktag(&xfentry.tag, TAGID_EXTFENTRY).is_ok() {
        (
            letoh32(xfentry.l_ea) as i32,
            letoh32(xfentry.l_ad) as i32,
            UDF_EXTFENTRY_SIZE as i64,
        )
    } else {
        (
            letoh32(fentry.l_ea) as i32,
            letoh32(fentry.l_ad) as i32,
            UDF_FENTRY_SIZE as i64,
        )
    };

    match letoh16(tag.strat_type) {
        4 => {}
        4096 => {
            printf(format_args!("Cannot deal with strategy4096 yet!\n"));
            return Err(Errno::ENODEV);
        }
        _ => {
            printf(format_args!("Unknown strategy type {}\n", {
                tag.strat_type
            }));
            return Err(Errno::ENODEV);
        }
    }

    let bshift = ump.um_bshift.get() as u32;
    let mut offset = offset;
    let mut icblen: u32 = 0;
    let mut ad_num: i32 = 0;
    let lsector: Daddr;
    let max_size: u32;

    match letoh16(tag.flags) & 0x7 {
        0 => {
            // The allocation descriptor field is filled with short_ad's. If the offset is
            // beyond the current extent, look for the next extent.
            let mut icb;
            loop {
                offset -= Off::from(icblen);
                let ad_offset = size_of::<ShortAd>() as i32 * ad_num;
                if ad_offset > l_ad {
                    printf(format_args!(
                        "SFile offset out of bounds ({} > {})\n",
                        ad_offset, l_ad
                    ));
                    return Err(Errno::EINVAL);
                }

                let raw: [u8; 8] = fentry_read(up, hdr + i64::from(l_ea) + i64::from(ad_offset));
                icb = ShortAd::at(&raw, 0).copied();
                icblen = icb.as_ref().map_or(0, geticblen);
                ad_num += 1;
                if offset < Off::from(icblen) {
                    break;
                }
            }

            let lb_num = icb.map_or(0, |icb| letoh32(icb.lb_num));
            lsector = offset.wrapping_shr(bshift) + Daddr::from(lb_num);

            max_size = icblen;
        }
        1 => {
            // The allocation descriptor field is filled with long_ad's If the offset is
            // beyond the current extent, look for the next extent.
            let mut icb;
            loop {
                offset -= Off::from(icblen);
                let ad_offset = size_of::<LongAd>() as i32 * ad_num;
                if ad_offset > l_ad {
                    printf(format_args!(
                        "LFile offset out of bounds ({} > {})\n",
                        ad_offset, l_ad
                    ));
                    return Err(Errno::EINVAL);
                }
                let raw: [u8; 16] = fentry_read(up, hdr + i64::from(l_ea) + i64::from(ad_offset));
                icb = LongAd::at(&raw, 0).copied();
                icblen = icb.as_ref().map_or(0, geticblen);
                ad_num += 1;
                if offset < Off::from(icblen) {
                    break;
                }
            }

            let lb_num = icb.map_or(0, |icb| letoh32(icb.loc.lb_num));
            lsector = offset.wrapping_shr(bshift) + Daddr::from(lb_num);

            max_size = icblen;
        }
        3 => {
            // This type means that the file *data* is stored in the allocation descriptor
            // field of the file entry.
            return Ok(UdfBmap::Invalid {
                sector: Daddr::from(up.u_ino.get().wrapping_add(ump.um_start.get())),
            });
        }
        // 2: DirectCD does not use extended_ad's
        _ => {
            printf(format_args!(
                "Unsupported allocation descriptor {}\n",
                { tag.flags } & 0x7
            ));
            return Err(Errno::ENODEV);
        }
    }

    let mut sector = lsector + Daddr::from(ump.um_start.get());

    // Check the sparing table. Each entry represents the beginning of a packet.
    let stbl = ump.stbl_bytes();
    if !stbl.is_empty() {
        let entries =
            &stbl[crate::isofs::udf::ecma167_udf::UdfSparingTable::SIZE.min(stbl.len())..];
        for i in 0..usize::try_from(ump.um_stbl_len.get()).unwrap_or(0) {
            let Some(e) = crate::isofs::udf::ecma167_udf::SpareMapEntry::at(entries, i * 8) else {
                break;
            };
            let p_offset = lsector - Daddr::from(letoh32(e.org));
            if p_offset < Daddr::from(ump.um_psecs.get()) && p_offset >= 0 {
                sector = Daddr::from(letoh32(e.map)) + p_offset;
                break;
            }
        }
    }

    Ok(UdfBmap::Block { sector, max_size })
}

#[cfg(test)]
mod tests;
