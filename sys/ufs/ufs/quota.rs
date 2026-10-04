/*	$OpenBSD: quota.h,v 1.12 2013/07/03 04:58:40 guenther Exp $	*/
/*	$NetBSD: quota.h,v 1.6 1995/03/26 20:38:17 jtc Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Robert Elz at The University of Melbourne.
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
 *	@(#)quota.h	8.3 (Berkeley) 8/19/94
 */
/* </LICENSES> */

//! `<ufs/ufs/quota.h>`: definitions for disk quotas imposed on the average user (big brother
//! finally hits UNIX): the quota file format (`struct dqblk`), the `quotactl(2)` commands, and
//! the interface the UFS code calls on every allocation and ownership change.
//!
//! Upstream: sys/ufs/ufs/quota.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `option QUOTA` is the `quota` feature (default, as in GENERIC). With it, the functions
//!   this header declares are `ufs_quota.rs`'s (the port of `ufs_quota.c`), re-exported
//!   here. Without it, OpenBSD links `ufs_quota_stub.c`, whose functions answer as a kernel
//!   without quotas must: nothing to look up or charge (`0`), nothing to sync, and
//!   `EOPNOTSUPP` for `quotactl(2)`. That file carries no licence block and is recorded
//!   `skipped` in `ports.toml`; the `#[cfg(not(feature = "quota"))]` functions below are those
//!   answers, written from this header and `quotactl(2)`.
//! - `struct dquot` is defined by `ufs_quota.c` at this pin, so it is `ufs_quota.rs`'s
//!   `Dquot`; `i_dquot[]` exists only with the feature (`inode.rs`).
//! - The `ufs_quota_{alloc,free}_{blocks,inode}` macros are functions with those names.
//! - `Dqblk` is `AbiPod`: `quotactl(2)` copies it in and out.

use crate::machine::copy::AbiPod;
use crate::sys::errno::Errno;
#[cfg(not(feature = "quota"))]
use crate::sys::mount::Mount;
#[cfg(not(feature = "quota"))]
use crate::sys::proc::Proc;
use crate::sys::types::Daddr;
#[cfg(not(feature = "quota"))]
use crate::sys::types::Uid;
use crate::sys::ucred::Ucred;
use crate::ufs::ufs::inode::Inode;
#[cfg(feature = "quota")]
pub use crate::ufs::ufs::ufs_quota::{
    getinoquota, qsync, quotaoff, ufs_quota_alloc_blocks2, ufs_quota_alloc_inode2,
    ufs_quota_delete, ufs_quota_free_blocks2, ufs_quota_free_inode2, ufs_quota_init, ufs_quotactl,
};

/// `MAX_IQ_TIME`: seconds in 1 week (the grace before inode soft limits become hard).
pub const MAX_IQ_TIME: i64 = 7 * 24 * 60 * 60;
/// `MAX_DQ_TIME`: seconds in 1 week (the grace before block soft limits become hard).
pub const MAX_DQ_TIME: i64 = 7 * 24 * 60 * 60;

/// `MAXQUOTAS`: the number of quota types (the quota file array in `ufsmount` and the dquot
/// array in the inode).
pub const MAXQUOTAS: usize = 2;
/// `USRQUOTA`: element used for user quotas.
pub const USRQUOTA: usize = 0;
/// `GRPQUOTA`: element used for group quotas.
pub const GRPQUOTA: usize = 1;

/// `INITQFNAMES`: the default names of the quota files.
pub const INITQFNAMES: [&[u8]; 3] = [b"user", b"group", b"undefined"];
/// `QUOTAFILENAME`.
pub const QUOTAFILENAME: &[u8] = b"quota";
/// `QUOTAGROUP`.
pub const QUOTAGROUP: &[u8] = b"operator";

/// `SUBCMDMASK`: the quota type part of a `quotactl` command.
pub const SUBCMDMASK: i32 = 0x00ff;
/// `SUBCMDSHIFT`.
pub const SUBCMDSHIFT: i32 = 8;

/// `QCMD(cmd, type)`.
pub const fn qcmd(cmd: i32, type_: i32) -> i32 {
    (cmd << SUBCMDSHIFT) | (type_ & SUBCMDMASK)
}

/// `Q_QUOTAON`: enable quotas.
pub const Q_QUOTAON: i32 = 0x0100;
/// `Q_QUOTAOFF`: disable quotas.
pub const Q_QUOTAOFF: i32 = 0x0200;
/// `Q_GETQUOTA`: get limits and usage.
pub const Q_GETQUOTA: i32 = 0x0300;
/// `Q_SETQUOTA`: set limits and usage.
pub const Q_SETQUOTA: i32 = 0x0400;
/// `Q_SETUSE`: set usage.
pub const Q_SETUSE: i32 = 0x0500;
/// `Q_SYNC`: sync disk copy of a filesystems quotas.
pub const Q_SYNC: i32 = 0x0600;

/// `struct dqblk`: the format of the disk quota file, an array of these structures indexed
/// by user or group number.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dqblk {
    /// `dqb_bhardlimit`: absolute limit on disk blks alloc.
    pub dqb_bhardlimit: u32,
    /// `dqb_bsoftlimit`: preferred limit on disk blks.
    pub dqb_bsoftlimit: u32,
    /// `dqb_curblocks`: current block count.
    pub dqb_curblocks: u32,
    /// `dqb_ihardlimit`: maximum # allocated inodes + 1.
    pub dqb_ihardlimit: u32,
    /// `dqb_isoftlimit`: preferred inode limit.
    pub dqb_isoftlimit: u32,
    /// `dqb_curinodes`: current # allocated inodes.
    pub dqb_curinodes: u32,
    /// `dqb_btime`: time limit for excessive disk use.
    pub dqb_btime: u32,
    /// `dqb_itime`: time limit for excessive files.
    pub dqb_itime: u32,
}

// SAFETY: eight `u_int32_t`s, `#[repr(C)]`: no padding, every bit pattern valid.
unsafe impl AbiPod for Dqblk {}

/// `enum ufs_quota_flags`: flags to `ufs_quota_{alloc,free}_{blocks,inode}2`.
pub type UfsQuotaFlags = i32;
/// `UFS_QUOTA_NOUID`: don't change UID quota.
pub const UFS_QUOTA_NOUID: UfsQuotaFlags = 0x1;
/// `UFS_QUOTA_NOGID`: don't change GID quota.
pub const UFS_QUOTA_NOGID: UfsQuotaFlags = 0x2;
/// `UFS_QUOTA_FORCE`: don't check limits - just change it.
pub const UFS_QUOTA_FORCE: UfsQuotaFlags = 0x1000;

/// `ufs_quota_alloc_blocks(i, c, cr)`.
pub fn ufs_quota_alloc_blocks(ip: &Inode, change: Daddr, cred: *const Ucred) -> Result<(), Errno> {
    ufs_quota_alloc_blocks2(ip, change, cred, 0)
}

/// `ufs_quota_free_blocks(i, c, cr)`.
pub fn ufs_quota_free_blocks(ip: &Inode, change: Daddr, cred: *const Ucred) -> Result<(), Errno> {
    ufs_quota_free_blocks2(ip, change, cred, 0)
}

/// `ufs_quota_alloc_inode(i, cr)`.
pub fn ufs_quota_alloc_inode(ip: &Inode, cred: *const Ucred) -> Result<(), Errno> {
    ufs_quota_alloc_inode2(ip, cred, 0)
}

/// `ufs_quota_free_inode(i, cr)`.
pub fn ufs_quota_free_inode(ip: &Inode, cred: *const Ucred) -> Result<(), Errno> {
    ufs_quota_free_inode2(ip, cred, 0)
}

/// `ufs_quota_alloc_blocks2`: charge `change` disk blocks; without `QUOTA` there is no limit.
#[cfg(not(feature = "quota"))]
pub fn ufs_quota_alloc_blocks2(
    _ip: &Inode,
    _change: Daddr,
    _cred: *const Ucred,
    _flags: UfsQuotaFlags,
) -> Result<(), Errno> {
    Ok(())
}

/// `ufs_quota_free_blocks2`: give back `change` disk blocks; nothing is charged without
/// `QUOTA`.
#[cfg(not(feature = "quota"))]
pub fn ufs_quota_free_blocks2(
    _ip: &Inode,
    _change: Daddr,
    _cred: *const Ucred,
    _flags: UfsQuotaFlags,
) -> Result<(), Errno> {
    Ok(())
}

/// `ufs_quota_alloc_inode2`: charge an inode; without `QUOTA` there is no limit.
#[cfg(not(feature = "quota"))]
pub fn ufs_quota_alloc_inode2(
    _ip: &Inode,
    _cred: *const Ucred,
    _flags: UfsQuotaFlags,
) -> Result<(), Errno> {
    Ok(())
}

/// `ufs_quota_free_inode2`: give back an inode; nothing is charged without `QUOTA`.
#[cfg(not(feature = "quota"))]
pub fn ufs_quota_free_inode2(
    _ip: &Inode,
    _cred: *const Ucred,
    _flags: UfsQuotaFlags,
) -> Result<(), Errno> {
    Ok(())
}

/// `ufs_quota_delete`: drop the inode's dquot references; there are none without `QUOTA`.
#[cfg(not(feature = "quota"))]
pub fn ufs_quota_delete(_ip: &Inode) -> Result<(), Errno> {
    Ok(())
}

/// `getinoquota`: set up the quotas for an inode; there are none without `QUOTA`.
#[cfg(not(feature = "quota"))]
pub fn getinoquota(_ip: &Inode) -> Result<(), Errno> {
    Ok(())
}

/// `quotaoff`: turn off a quota type on a file system; nothing is on without `QUOTA`.
#[cfg(not(feature = "quota"))]
pub fn quotaoff(_p: &Proc, _mp: &'static Mount, _type: usize) -> Result<(), Errno> {
    Ok(())
}

/// `qsync`: write the file system's quota changes to disk; nothing to write without
/// `QUOTA`.
#[cfg(not(feature = "quota"))]
pub fn qsync(_mp: &'static Mount) -> Result<(), Errno> {
    Ok(())
}

/// `ufs_quotactl` (`vfs_quotactl`): `quotactl(2)` is not supported without `QUOTA`.
#[cfg(not(feature = "quota"))]
pub fn ufs_quotactl(
    _mp: &'static Mount,
    _cmds: i32,
    _uid: Uid,
    _arg: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `ufs_quota_init`: nothing to initialise without `QUOTA`.
#[cfg(not(feature = "quota"))]
pub fn ufs_quota_init() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/ufs/ufs/quota.h");
        for (name, value) in [
            ("MAXQUOTAS", MAXQUOTAS as i64),
            ("USRQUOTA", USRQUOTA as i64),
            ("GRPQUOTA", GRPQUOTA as i64),
            ("SUBCMDMASK", i64::from(SUBCMDMASK)),
            ("SUBCMDSHIFT", i64::from(SUBCMDSHIFT)),
            ("Q_QUOTAON", i64::from(Q_QUOTAON)),
            ("Q_SYNC", i64::from(Q_SYNC)),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
        assert_eq!(qcmd(Q_GETQUOTA, GRPQUOTA as i32), 0x30001);
        assert_eq!(size_of::<Dqblk>(), 32);
    }
}
