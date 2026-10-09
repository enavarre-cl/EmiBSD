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
 * Copyright (c) 1982, 1986, 1990, 1993, 1995
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
 *	@(#)ufs_quota.c	8.5 (Berkeley) 8/19/94
 */
/* </LICENSES> */

/* <CODE> */
//! Disk quotas (`option QUOTA`): the in-core quota of a user or group on a file system
//! (`struct dquot`), the charges every allocation and release of blocks and inodes makes
//! against it, the soft and hard limits with their grace times, and `quotactl(2)`'s
//! commands (`Q_QUOTAON`, `Q_QUOTAOFF`, `Q_GETQUOTA`, `Q_SETQUOTA`, `Q_SETUSE`, `Q_SYNC`).
//!
//! Upstream: sys/ufs/ufs/ufs_quota.c @ 3ce1f3f79392
//!
//! A quota file is an array of `struct dqblk` indexed by user or group id. `Q_QUOTAON` opens
//! it and keeps its vnode in `um_quotas[type]`; from then on every inode being written holds
//! a referenced dquot per quota type (`i_dquot[]`, set up by [`getinoquota`]), read from the
//! file by [`dqget`] and written back by [`dqsync`] through `VOP_READ`/`VOP_WRITE` on the
//! quota file's vnode. Unreferenced dquots stay cached in a SipHash-keyed hash and on a free
//! list from which [`dqget`] recycles them.
//!
//! The functions with the names `<ufs/ufs/quota.h>` declares are re-exported by `quota.rs`,
//! which keeps the no-quota answers of `ufs_quota_stub.c` for builds without the `quota`
//! feature.
//!
//! ## Deviations
//! - `struct dquot` is [`Dquot`], a `malloc(M_DQUOT)` item never freed (as in C, recycled
//!   through the free list) reached as `&'static Dquot`; its members are `Cell`s, changed
//!   under the kernel lock and the `DQ_LOCK` flag as in C. The `dq_bhardlimit`, ...
//!   shorthands are reads and updates of the `Cell<Dqblk>` `dq_dqb`. `NODQUOT` is `None`.
//! - `dq_cred` is an `Option<&'static Ucred>` (`None` for the C's `NOCRED`).
//! - [`Dquot`] has one member the C lacks, `dq_hashed`: [`dqget`]'s read-error path takes
//!   the dquot off its hash chain before releasing it to the free list, and the recycling
//!   path's `LIST_REMOVE` would then unlink it a second time through stale links. The flag
//!   makes the second removal a no-op.
//! - The soft/hard limit decision `chkdqchg` and `chkiqchg` share is [`chklimit`], and the
//!   new limits and usage of `setquota` and `setuse` are computed by [`setquota_dqblk`] and
//!   [`setuse_dqblk`], so the host tests check them without a file system; the messages and
//!   flags stay in `chkdqchg`/`chkiqchg`.
//! - `ufs_quota_alloc_blocks2` and `ufs_quota_alloc_inode2` treat `NOCRED` and `FSCRED`
//!   (and NULL) as the kernel's credential: no limit is checked. The C tests `NOCRED` only in
//!   the former and dereferences the pointer otherwise.
//! - [`dqget`] answers `ENOMEM` when `malloc(M_WAITOK)` cannot be satisfied (the C cannot
//!   fail there).
//! - `ufs_quota_init` also resets `numdquot` and `desireddquot`: the kernel calls it once, as
//!   the C does, but the host tests start every file system test afresh (`UFS_INIT_DONE`),
//!   and the cached dquots of the previous table are forgotten with it.
//! - `dqhashtbl`, `dqhash` and `dqhashkey` are `StaticCell`s written by `ufs_quota_init`
//!   only, before any UFS is mounted.
//! - `KTRACE` is not configured: `ktrquota` is not called.
//! - The `DIAGNOSTIC` checks (`chkdquot`, the `vfs_isbusy` and `VOP_ISLOCKED` panics) are
//!   under feature `diagnostic`.

use core::cell::Cell;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI64, Ordering};

use crate::crypto::siphash::{
    SipHash24_End, SipHash24_Init, SipHash24_Update, SiphashCtx, SiphashKey,
};
use crate::dev::rnd::arc4random_buf;
use crate::kern::kern_malloc::malloc;
use crate::kern::kern_prot::{crfree, crhold, suser};
use crate::kern::kern_subr::hashinit;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_tc::gettime;
use crate::kern::subr_prf::{Str, panic, tablefull, uprintf};
use crate::kern::vfs_lookup::ndinit;
use crate::kern::vfs_subr::{vfs_busy, vfs_mount_foreach_vnode, vfs_unbusy, vget, vput};
#[cfg(feature = "diagnostic")]
use crate::kern::vfs_subr::{vfs_isbusy, vprint};
use crate::kern::vfs_vnops::{vn_close, vn_lock, vn_open};
#[cfg(feature = "diagnostic")]
use crate::kern::vfs_vops::VOP_ISLOCKED;
use crate::kern::vfs_vops::{VOP_READ, VOP_UNLOCK, VOP_WRITE};
use crate::machine::copy::{copyin_obj, copyout_obj};
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::lock::{LK_EXCLUSIVE, LK_NOWAIT, LK_RETRY};
use crate::sys::malloc::{M_DQUOT, M_WAITOK, M_ZERO};
use crate::sys::mount::{MNT_QUOTA, Mount, VB_NOWAIT, VB_READ};
use crate::sys::namei::NiDirp;
use crate::sys::param::PINOD;
use crate::sys::proc::Proc;
use crate::sys::queue::{ListEntry, ListHead, TailqEntry, TailqHead};
use crate::sys::systm::INFSLP;
use crate::sys::types::{Daddr, Off, Time, Uid};
use crate::sys::ucred::{NOCRED, Ucred};
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::{VNON, VREG, VSYSTEM, Vnode, cred_ref};
use crate::ufs::ufs::inode::{Inode, vtoi};
use crate::ufs::ufs::quota::{
    Dqblk, GRPQUOTA, INITQFNAMES, MAX_DQ_TIME, MAX_IQ_TIME, MAXQUOTAS, Q_GETQUOTA, Q_QUOTAOFF,
    Q_QUOTAON, Q_SETQUOTA, Q_SETUSE, Q_SYNC, SUBCMDMASK, SUBCMDSHIFT, UFS_QUOTA_FORCE, USRQUOTA,
    UfsQuotaFlags,
};
use crate::ufs::ufs::ufsmount::{QTF_CLOSING, QTF_OPENING, Ufsmount, vfstoufs};
use libkern::StaticCell;

/// `DQ_LOCK`: this quota locked (no MODS).
pub const DQ_LOCK: u16 = 0x01;
/// `DQ_WANT`: wakeup on unlock.
pub const DQ_WANT: u16 = 0x02;
/// `DQ_MOD`: this quota modified since read.
pub const DQ_MOD: u16 = 0x04;
/// `DQ_FAKE`: no limits here, just usage.
pub const DQ_FAKE: u16 = 0x08;
/// `DQ_BLKS`: has been warned about blk limit.
pub const DQ_BLKS: u16 = 0x10;
/// `DQ_INODS`: has been warned about inode limit.
pub const DQ_INODS: u16 = 0x20;

/// `DQUOTINC`: minimum free dquots desired.
const DQUOTINC: i64 = 5;

/// `struct dquot`: the disk usage of a user or group on a filesystem. There is one
/// allocated for each quota that exists on any filesystem for the current user or group. A
/// cache is kept of recently used entries.
///
/// Protected by: the kernel lock; `dq_dqb` also by `DQ_LOCK` while it is read or written.
pub struct Dquot {
    /// `dq_hash`: hash list.
    pub dq_hash: ListEntry<Dquot>,
    /// `dq_freelist`: free list.
    pub dq_freelist: TailqEntry<Dquot>,
    /// `dq_flags`: flags, see `DQ_*`.
    pub dq_flags: Cell<u16>,
    /// `dq_type`: quota type of this dquot.
    pub dq_type: Cell<u16>,
    /// `dq_cnt`: count of active references.
    pub dq_cnt: Cell<u32>,
    /// `dq_id`: identifier this applies to.
    pub dq_id: Cell<u32>,
    /// `dq_vp`: file backing this quota.
    pub dq_vp: Cell<Option<&'static Vnode>>,
    /// `dq_cred`: credentials for writing file (`None` is `NOCRED`).
    pub dq_cred: Cell<Option<&'static Ucred>>,
    /// `dq_dqb`: actual usage & quotas.
    pub dq_dqb: Cell<Dqblk>,
    /// Whether `dq_hash` links the dquot into a hash chain (the module's deviations).
    dq_hashed: Cell<bool>,
}

impl Dquot {
    /// A zeroed dquot, as `malloc(M_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            dq_hash: ListEntry::new(),
            dq_freelist: TailqEntry::new(),
            dq_flags: Cell::new(0),
            dq_type: Cell::new(0),
            dq_cnt: Cell::new(0),
            dq_id: Cell::new(0),
            dq_vp: Cell::new(None),
            dq_cred: Cell::new(None),
            dq_dqb: Cell::new(Dqblk {
                dqb_bhardlimit: 0,
                dqb_bsoftlimit: 0,
                dqb_curblocks: 0,
                dqb_ihardlimit: 0,
                dqb_isoftlimit: 0,
                dqb_curinodes: 0,
                dqb_btime: 0,
                dqb_itime: 0,
            }),
            dq_hashed: Cell::new(false),
        }
    }

    /// `dq->dq_flags |= f`.
    fn setflag(&self, f: u16) {
        self.dq_flags.set(self.dq_flags.get() | f);
    }

    /// `dq->dq_flags &= ~f`.
    fn clrflag(&self, f: u16) {
        self.dq_flags.set(self.dq_flags.get() & !f);
    }

    /// `dq->dq_flags & f`.
    fn isset(&self, f: u16) -> bool {
        self.dq_flags.get() & f != 0
    }

    /// Changes `dq_dqb` (the `dq_curblocks`, `dq_btime`, ... shorthands on the left of an
    /// assignment).
    fn update(&self, f: impl FnOnce(&mut Dqblk)) {
        let mut b = self.dq_dqb.get();
        f(&mut b);
        self.dq_dqb.set(b);
    }
}

impl Default for Dquot {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: dquots are changed under the kernel lock and `DQ_LOCK`, as in C.
unsafe impl Sync for Dquot {}

queue_adapter!(
    /// `LIST_HEAD(dqhash, dquot)`: a dquot hash chain, through `dq_hash`.
    pub DqHash: Dquot, dq_hash => ListEntry<Dquot>
);

queue_adapter!(
    /// `TAILQ_HEAD(dqfreelist, dquot)`: the dquot free list, through `dq_freelist`.
    pub DqFreelist: Dquot, dq_freelist => TailqEntry<Dquot>
);

/// What [`chklimit`] found for a charge against one pair of limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    /// The charge stays within the limits.
    Under,
    /// The charge crosses the soft limit now: the grace time starts, a warning is printed.
    SoftCrossed,
    /// The charge would exceed the hard limit.
    HardReached,
    /// Over the soft limit for longer than the grace time.
    GraceExpired,
}

/// The hash chains, with the claim that makes them shareable.
#[derive(Clone, Copy)]
struct Dqhashtbl(&'static [ListHead<DqHash>]);

// SAFETY: the chains are changed under the kernel lock, as in C.
unsafe impl Sync for Dqhashtbl {}
// SAFETY: as above.
unsafe impl Send for Dqhashtbl {}

/// The free list, with the claim that makes it shareable.
struct Dqfreelst(TailqHead<DqFreelist>);

// SAFETY: the list is changed under the kernel lock, as in C.
unsafe impl Sync for Dqfreelst {}

/// `quotatypes[]`: quota name to error message mapping (`INITQFNAMES`).
static QUOTATYPES: [&[u8]; 3] = INITQFNAMES;

/// `LIST_HEAD(dqhash, dquot) *dqhashtbl`.
static DQHASHTBL: StaticCell<Dqhashtbl> = StaticCell::new(Dqhashtbl(&[]));
/// `dqhashkey`.
static DQHASHKEY: StaticCell<SiphashKey> = StaticCell::new(SiphashKey { k0: 0, k1: 0 });
/// `dqhash`: size of the hash table - 1.
static DQHASH: StaticCell<u64> = StaticCell::new(0);

/// `dqfreelist`: the dquot free list.
static DQFREELIST: Dqfreelst = Dqfreelst(TailqHead::new());
/// `numdquot`: the dquots allocated.
static NUMDQUOT: AtomicI64 = AtomicI64::new(0);
/// `desireddquot`: the dquots wanted before free ones are recycled.
static DESIREDDQUOT: AtomicI64 = AtomicI64::new(DQUOTINC);

/// `dqref`: obtain a reference to a dquot.
pub fn dqref(dq: &Dquot) {
    dq.dq_cnt.set(dq.dq_cnt.get() + 1);
}

/// `getinoquota`: set up the quotas for an inode.
///
/// This routine completely defines the semantics of quotas. If other criterion want to be
/// used to establish quotas, the `MAXQUOTAS` value in quota.h should be increased, and the
/// additional dquots set up here.
pub fn getinoquota(ip: &Inode) -> Result<(), Errno> {
    let ump = ip.ump();
    let vp = ip.itov();

    // Set up the user quota based on file uid, then the group quota based on file gid.
    // EINVAL means that quotas are not enabled.
    for (type_, id) in [(USRQUOTA, ip.dip_uid()), (GRPQUOTA, ip.dip_gid())] {
        if ip.i_dquot[type_].get().is_none() {
            match dqget(Some(vp), id, ump, type_) {
                Ok(dq) => ip.i_dquot[type_].set(Some(dq)),
                Err(Errno::EINVAL) => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

/// Sleeps until nobody holds `dq`'s `DQ_LOCK` (the C's `while (dq->dq_flags & DQ_LOCK)`
/// loops).
fn dqwait(dq: &Dquot, pri: i32, wmesg: &'static str) {
    while dq.isset(DQ_LOCK) {
        dq.setflag(DQ_WANT);
        let _ = tsleep_nsec(ptr::from_ref(dq), pri, wmesg, INFSLP);
    }
}

/// Whether `flags` excludes quota type `i` (`UFS_QUOTA_NOUID`, `UFS_QUOTA_NOGID`).
fn skiptype(flags: UfsQuotaFlags, i: usize) -> bool {
    flags & (1 << i) != 0
}

/// The credential a charge is checked against: `None` for the kernel's (`NOCRED`, `FSCRED`,
/// NULL; see the module's deviations).
fn chkcred<'a>(cred: *const Ucred) -> Option<&'a Ucred> {
    // SAFETY: the credential of a quota charge is the one of the vnode operation doing the
    // allocation, held by its caller for the operation's duration (`cred_ref`'s contract).
    unsafe { cred_ref(cred) }
}

/// `ufs_quota_alloc_blocks2`: update disk usage, and take corrective action.
pub fn ufs_quota_alloc_blocks2(
    ip: &Inode,
    change: Daddr,
    cred: *const Ucred,
    flags: UfsQuotaFlags,
) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    chkdquot(ip);

    if change == 0 {
        return Ok(());
    }

    if flags & UFS_QUOTA_FORCE == 0
        && let Some(c) = chkcred(cred).filter(|c| c.cr_uid.get() != 0)
    {
        for i in 0..MAXQUOTAS {
            if skiptype(flags, i) || ip.i_dquot[i].get().is_none() {
                continue;
            }
            chkdqchg(ip, change, c, i)?;
        }
    }
    for i in 0..MAXQUOTAS {
        if skiptype(flags, i) {
            continue;
        }
        let Some(dq) = ip.i_dquot[i].get() else {
            continue;
        };
        dqwait(dq, PINOD + 1, "chkdq");
        dq.update(|b| b.dqb_curblocks = (i64::from(b.dqb_curblocks) + change) as u32);
        dq.setflag(DQ_MOD);
    }
    Ok(())
}

/// `ufs_quota_free_blocks2`: give back `change` disk blocks.
pub fn ufs_quota_free_blocks2(
    ip: &Inode,
    change: Daddr,
    _cred: *const Ucred,
    flags: UfsQuotaFlags,
) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    if VOP_ISLOCKED(ip.itov()) == 0 {
        panic(format_args!("ufs_quota_free_blocks2: vnode is not locked"));
    }

    if change == 0 {
        return Ok(());
    }

    for i in 0..MAXQUOTAS {
        if skiptype(flags, i) {
            continue;
        }
        let Some(dq) = ip.i_dquot[i].get() else {
            continue;
        };
        dqwait(dq, PINOD + 1, "chkdq");
        dq.update(|b| {
            b.dqb_curblocks = if i64::from(b.dqb_curblocks) >= change {
                (i64::from(b.dqb_curblocks) - change) as u32
            } else {
                0
            };
        });
        dq.clrflag(DQ_BLKS);
        dq.setflag(DQ_MOD);
    }
    Ok(())
}

/// The limit test of `chkdqchg` and `chkiqchg`: charging `change` to a usage of `cur` with
/// a `hard` and a `soft` limit (0: none), whose grace time ends at `deadline`, at `now`.
pub fn chklimit(cur: u32, change: i64, hard: u32, soft: u32, deadline: u32, now: Time) -> Limit {
    let ncur = i64::from(cur) + change;

    // If user would exceed their hard limit, disallow the allocation.
    if ncur >= i64::from(hard) && hard != 0 {
        return Limit::HardReached;
    }
    // If user is over their soft limit for too long, disallow the allocation. Reset time
    // limit as they cross their soft limit.
    if ncur >= i64::from(soft) && soft != 0 {
        if cur < soft {
            return Limit::SoftCrossed;
        }
        if now > Time::from(deadline) {
            return Limit::GraceExpired;
        }
    }
    Limit::Under
}

/// `ITOV(ip)->v_mount->mnt_stat.f_mntonname`, for the messages.
fn mntonname(ip: &Inode) -> ([u8; crate::sys::mount::MNAMELEN], usize) {
    match ip.itov().v_mount.get() {
        Some(mp) => mp.mntonname(),
        None => ([0; crate::sys::mount::MNAMELEN], 0),
    }
}

/// `chkdqchg`: check for a valid change to a users allocation. Issue an error message if
/// appropriate.
fn chkdqchg(ip: &Inode, change: i64, cred: &Ucred, type_: usize) -> Result<(), Errno> {
    let Some(dq) = ip.i_dquot[type_].get() else {
        return Ok(());
    };
    let b = dq.dq_dqb.get();
    let owner = ip.dip_uid() == cred.cr_uid.get();
    let now = gettime();
    let (name, len) = mntonname(ip);
    let qtype = Str(QUOTATYPES[type_]);

    match chklimit(
        b.dqb_curblocks,
        change,
        b.dqb_bhardlimit,
        b.dqb_bsoftlimit,
        b.dqb_btime,
        now,
    ) {
        Limit::Under => Ok(()),
        Limit::HardReached => {
            if !dq.isset(DQ_BLKS) && owner {
                uprintf(format_args!(
                    "\n{}: write failed, {} disk limit reached\n",
                    Str(&name[..len]),
                    qtype
                ));
                dq.setflag(DQ_BLKS);
            }
            Err(Errno::EDQUOT)
        }
        Limit::SoftCrossed => {
            let grace = ip.ump().um_btime[type_].get();
            dq.update(|b| b.dqb_btime = (now + grace) as u32);
            if owner {
                uprintf(format_args!(
                    "\n{}: warning, {} {}\n",
                    Str(&name[..len]),
                    qtype,
                    "disk quota exceeded"
                ));
            }
            Ok(())
        }
        Limit::GraceExpired => {
            if !dq.isset(DQ_BLKS) && owner {
                uprintf(format_args!(
                    "\n{}: write failed, {} {}\n",
                    Str(&name[..len]),
                    qtype,
                    "disk quota exceeded for too long"
                ));
                dq.setflag(DQ_BLKS);
            }
            Err(Errno::EDQUOT)
        }
    }
}

/// `ufs_quota_alloc_inode2`: check the inode limit, applying corrective action.
pub fn ufs_quota_alloc_inode2(
    ip: &Inode,
    cred: *const Ucred,
    flags: UfsQuotaFlags,
) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    chkdquot(ip);

    if flags & UFS_QUOTA_FORCE == 0
        && let Some(c) = chkcred(cred).filter(|c| c.cr_uid.get() != 0)
    {
        for i in 0..MAXQUOTAS {
            if skiptype(flags, i) || ip.i_dquot[i].get().is_none() {
                continue;
            }
            chkiqchg(ip, 1, c, i)?;
        }
    }
    for i in 0..MAXQUOTAS {
        if skiptype(flags, i) {
            continue;
        }
        let Some(dq) = ip.i_dquot[i].get() else {
            continue;
        };
        dqwait(dq, PINOD + 1, "chkiq");
        dq.update(|b| b.dqb_curinodes = b.dqb_curinodes.wrapping_add(1));
        dq.setflag(DQ_MOD);
    }
    Ok(())
}

/// `ufs_quota_free_inode2`: give back an inode.
pub fn ufs_quota_free_inode2(
    ip: &Inode,
    _cred: *const Ucred,
    flags: UfsQuotaFlags,
) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    if VOP_ISLOCKED(ip.itov()) == 0 {
        panic(format_args!("ufs_quota_free_blocks2: vnode is not locked"));
    }

    for i in 0..MAXQUOTAS {
        if skiptype(flags, i) {
            continue;
        }
        let Some(dq) = ip.i_dquot[i].get() else {
            continue;
        };
        dqwait(dq, PINOD + 1, "chkiq");
        dq.update(|b| {
            if b.dqb_curinodes > 0 {
                b.dqb_curinodes -= 1;
            }
        });
        dq.clrflag(DQ_INODS);
        dq.setflag(DQ_MOD);
    }
    Ok(())
}

/// `chkiqchg`: check for a valid change to a users allocation. Issue an error message if
/// appropriate.
fn chkiqchg(ip: &Inode, change: i64, cred: &Ucred, type_: usize) -> Result<(), Errno> {
    let Some(dq) = ip.i_dquot[type_].get() else {
        return Ok(());
    };
    let b = dq.dq_dqb.get();
    let owner = ip.dip_uid() == cred.cr_uid.get();
    let now = gettime();
    let (name, len) = mntonname(ip);
    let qtype = Str(QUOTATYPES[type_]);

    match chklimit(
        b.dqb_curinodes,
        change,
        b.dqb_ihardlimit,
        b.dqb_isoftlimit,
        b.dqb_itime,
        now,
    ) {
        Limit::Under => Ok(()),
        Limit::HardReached => {
            if !dq.isset(DQ_INODS) && owner {
                uprintf(format_args!(
                    "\n{}: write failed, {} inode limit reached\n",
                    Str(&name[..len]),
                    qtype
                ));
                dq.setflag(DQ_INODS);
            }
            Err(Errno::EDQUOT)
        }
        Limit::SoftCrossed => {
            let grace = ip.ump().um_itime[type_].get();
            dq.update(|b| b.dqb_itime = (now + grace) as u32);
            if owner {
                uprintf(format_args!(
                    "\n{}: warning, {} {}\n",
                    Str(&name[..len]),
                    qtype,
                    "inode quota exceeded"
                ));
            }
            Ok(())
        }
        Limit::GraceExpired => {
            if !dq.isset(DQ_INODS) && owner {
                uprintf(format_args!(
                    "\n{}: write failed, {} {}\n",
                    Str(&name[..len]),
                    qtype,
                    "inode quota exceeded for too long"
                ));
                dq.setflag(DQ_INODS);
            }
            Err(Errno::EDQUOT)
        }
    }
}

/// `chkdquot`: on filesystems with quotas enabled, it is an error for a file to change size
/// and not to have a dquot structure associated with it.
#[cfg(feature = "diagnostic")]
fn chkdquot(ip: &Inode) {
    let ump = ip.ump();
    let vp = ip.itov();

    if VOP_ISLOCKED(vp) == 0 {
        panic(format_args!("chkdquot: vnode is not locked"));
    }

    for i in 0..MAXQUOTAS {
        if ump.um_quotas[i].get().is_none()
            || ump.um_qflags[i].get() & (QTF_OPENING | QTF_CLOSING) != 0
        {
            continue;
        }
        if ip.i_dquot[i].get().is_none() {
            vprint(Some("chkdquot: missing dquot"), vp);
            panic(format_args!("missing dquot"));
        }
    }
}

// Code to process quotactl commands.

/// `quotaon_vnode`: give a vnode open for writing its dquots.
fn quotaon_vnode(vp: &'static Vnode) -> Result<(), Errno> {
    if vp.v_type.get() == VNON || vp.v_writecount.get() == 0 {
        return Ok(());
    }

    if vget(vp, LK_EXCLUSIVE).is_err() {
        return Ok(());
    }

    let error = getinoquota(vtoi(vp));
    vput(vp);

    error
}

/// The credential `quotaon` keeps in `um_cred[type]`, if any.
fn um_cred(ump: &Ufsmount, type_: usize) -> Option<&'static Ucred> {
    // SAFETY: `um_cred[type]` is NULL, `NOCRED` or the credential `quotaon` took a reference
    // to with `crhold`, which `quotaoff` releases only after clearing the slot.
    unsafe { cred_ref(ump.um_cred[type_].get()) }
}

/// `quotaon` (`Q_QUOTAON`): set up a quota file for a particular file system. `fname` is
/// the user address of the quota file's path.
pub fn quotaon(p: &Proc, mp: &'static Mount, type_: usize, fname: usize) -> Result<(), Errno> {
    let ump = vfstoufs(mp);

    #[cfg(feature = "diagnostic")]
    if !vfs_isbusy(mp) {
        panic(format_args!("quotaon: mount point not busy"));
    }

    let mut nd = ndinit(0, 0, NiDirp::User(fname), p);
    vn_open(&mut nd, FREAD | FWRITE, 0)?;
    let Some(vp) = nd.ni_vp else {
        panic(format_args!("quotaon: vn_open returned no vnode"));
    };
    let _ = VOP_UNLOCK(vp);
    if vp.v_type.get() != VREG {
        let _ = vn_close(vp, FREAD | FWRITE, p.p_ucred.get(), Some(p));
        return Err(Errno::EACCES);
    }

    // Update the vnode and ucred for quota file updates.
    if !ump.um_quotas[type_].get().is_some_and(|q| ptr::eq(q, vp)) {
        let _ = quotaoff(p, mp, type_);
        ump.um_quotas[type_].set(Some(vp));
        let cr = crhold(p.ucred());
        ump.um_cred[type_].set(ptr::from_ref(cr));
    } else {
        let ocred = ump.um_cred[type_].get();

        let _ = vn_close(vp, FREAD | FWRITE, ocred, Some(p));
        if !ptr::eq(ocred, p.p_ucred.get()) {
            let cr = crhold(p.ucred());
            ump.um_cred[type_].set(ptr::from_ref(cr));
            // SAFETY: `ocred` is the reference `quotaon` took before (see `um_cred`), given
            // up here after the slot was replaced.
            if let Some(ocred) = unsafe { cred_ref(ocred) } {
                crfree(ocred);
            }
        }
    }

    ump.um_qflags[type_].set(ump.um_qflags[type_].get() | QTF_OPENING);
    mp.mnt_flag.set(mp.mnt_flag.get() | MNT_QUOTA);
    vp.v_flag.set(vp.v_flag.get() | VSYSTEM);

    // Set up the time limits for this quota.
    ump.um_btime[type_].set(MAX_DQ_TIME);
    ump.um_itime[type_].set(MAX_IQ_TIME);
    if let Ok(dq) = dqget(None, 0, ump, type_) {
        let b = dq.dq_dqb.get();
        if b.dqb_btime > 0 {
            ump.um_btime[type_].set(Time::from(b.dqb_btime));
        }
        if b.dqb_itime > 0 {
            ump.um_itime[type_].set(Time::from(b.dqb_itime));
        }
        dqrele(None, Some(dq));
    }

    // Search vnodes associated with this mount point, adding references to quota file being
    // opened. NB: only need to add dquot's for inodes being modified.
    let error = vfs_mount_foreach_vnode(mp, &mut |vp| quotaon_vnode(vp));

    ump.um_qflags[type_].set(ump.um_qflags[type_].get() & !QTF_OPENING);
    if error.is_err() {
        let _ = quotaoff(p, mp, type_);
    }
    error
}

/// `quotaoff_vnode`: drop a vnode's dquot of the type being turned off.
fn quotaoff_vnode(vp: &'static Vnode, type_: usize) -> Result<(), Errno> {
    if vp.v_type.get() == VNON {
        return Ok(());
    }

    if vget(vp, LK_EXCLUSIVE).is_err() {
        return Ok(());
    }
    let ip = vtoi(vp);
    let dq = ip.i_dquot[type_].take();
    dqrele(Some(vp), dq);
    vput(vp);
    Ok(())
}

/// `quotaoff` (`Q_QUOTAOFF`): turn off disk quotas for a filesystem.
pub fn quotaoff(p: &Proc, mp: &'static Mount, type_: usize) -> Result<(), Errno> {
    let ump = vfstoufs(mp);

    #[cfg(feature = "diagnostic")]
    if !vfs_isbusy(mp) {
        panic(format_args!("quotaoff: mount point not busy"));
    }
    let Some(qvp) = ump.um_quotas[type_].get() else {
        return Ok(());
    };
    ump.um_qflags[type_].set(ump.um_qflags[type_].get() | QTF_CLOSING);
    // Search vnodes associated with this mount point, deleting any references to quota file
    // being closed.
    let _ = vfs_mount_foreach_vnode(mp, &mut |vp| quotaoff_vnode(vp, type_));

    let error = vn_close(qvp, FREAD | FWRITE, p.p_ucred.get(), Some(p));
    ump.um_quotas[type_].set(None);
    let cred = um_cred(ump, type_);
    ump.um_cred[type_].set(NOCRED);
    if let Some(cred) = cred {
        crfree(cred);
    }
    ump.um_qflags[type_].set(ump.um_qflags[type_].get() & !QTF_CLOSING);
    if ump.um_quotas.iter().all(|q| q.get().is_none()) {
        mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_QUOTA);
    }
    error
}

/// `getquota` (`Q_GETQUOTA`): return current values in a dqblk structure at the user
/// address `addr`.
fn getquota(mp: &'static Mount, id: u32, type_: usize, addr: usize) -> Result<(), Errno> {
    let dq = dqget(None, id, vfstoufs(mp), type_)?;
    let error = copyout_obj(&dq.dq_dqb.get(), addr);
    // KTRACE (KTR_STRUCT, ktrquota): not configured.

    dqrele(None, Some(dq));
    error
}

/// `setquota`'s new `dq_dqb`: all of `newlim` but the current values (and the grace times
/// of an id other than 0), with a grace time started if the user had no soft limit or was
/// under it, and is now over the new one; and the new `dq_flags`.
pub fn setquota_dqblk(
    old: Dqblk,
    flags: u16,
    id: u32,
    mut newlim: Dqblk,
    now: Time,
    bgrace: Time,
    igrace: Time,
) -> (Dqblk, u16) {
    // Copy all but the current values. Reset time limit if previously had no soft limit or
    // were under it, but now have a soft limit and are over it.
    newlim.dqb_curblocks = old.dqb_curblocks;
    newlim.dqb_curinodes = old.dqb_curinodes;
    if id != 0 {
        newlim.dqb_btime = old.dqb_btime;
        newlim.dqb_itime = old.dqb_itime;
    }
    if newlim.dqb_bsoftlimit != 0
        && old.dqb_curblocks >= newlim.dqb_bsoftlimit
        && (old.dqb_bsoftlimit == 0 || old.dqb_curblocks < old.dqb_bsoftlimit)
    {
        newlim.dqb_btime = (now + bgrace) as u32;
    }
    if newlim.dqb_isoftlimit != 0
        && old.dqb_curinodes >= newlim.dqb_isoftlimit
        && (old.dqb_isoftlimit == 0 || old.dqb_curinodes < old.dqb_isoftlimit)
    {
        newlim.dqb_itime = (now + igrace) as u32;
    }
    let b = newlim;
    let mut flags = flags;
    if b.dqb_curblocks < b.dqb_bsoftlimit {
        flags &= !DQ_BLKS;
    }
    if b.dqb_curinodes < b.dqb_isoftlimit {
        flags &= !DQ_INODS;
    }
    if b.dqb_isoftlimit == 0
        && b.dqb_bsoftlimit == 0
        && b.dqb_ihardlimit == 0
        && b.dqb_bhardlimit == 0
    {
        flags |= DQ_FAKE;
    } else {
        flags &= !DQ_FAKE;
    }
    (b, flags | DQ_MOD)
}

/// `setquota` (`Q_SETQUOTA`): assign an entire dqblk structure, read from the user address
/// `addr`.
fn setquota(mp: &'static Mount, id: u32, type_: usize, addr: usize) -> Result<(), Errno> {
    let ump = vfstoufs(mp);

    let newlim: Dqblk = copyin_obj(addr)?;
    // KTRACE (KTR_STRUCT, ktrquota): not configured.

    let dq = dqget(None, id, ump, type_)?;
    dqwait(dq, PINOD + 1, "setquota");
    let (b, flags) = setquota_dqblk(
        dq.dq_dqb.get(),
        dq.dq_flags.get(),
        dq.dq_id.get(),
        newlim,
        gettime(),
        ump.um_btime[type_].get(),
        ump.um_itime[type_].get(),
    );
    dq.dq_dqb.set(b);
    dq.dq_flags.set(flags);
    dqrele(None, Some(dq));
    Ok(())
}

/// `setuse`'s new `dq_dqb` and `dq_flags`: the current values of `usage`, with a grace time
/// started if the user has a soft limit, was under it, and is now over it.
pub fn setuse_dqblk(
    old: Dqblk,
    flags: u16,
    usage: Dqblk,
    now: Time,
    bgrace: Time,
    igrace: Time,
) -> (Dqblk, u16) {
    let mut b = old;
    // Reset time limit if have a soft limit and were previously under it, but are now over
    // it.
    if b.dqb_bsoftlimit != 0
        && b.dqb_curblocks < b.dqb_bsoftlimit
        && usage.dqb_curblocks >= b.dqb_bsoftlimit
    {
        b.dqb_btime = (now + bgrace) as u32;
    }
    if b.dqb_isoftlimit != 0
        && b.dqb_curinodes < b.dqb_isoftlimit
        && usage.dqb_curinodes >= b.dqb_isoftlimit
    {
        b.dqb_itime = (now + igrace) as u32;
    }
    b.dqb_curblocks = usage.dqb_curblocks;
    b.dqb_curinodes = usage.dqb_curinodes;
    let mut flags = flags;
    if b.dqb_curblocks < b.dqb_bsoftlimit {
        flags &= !DQ_BLKS;
    }
    if b.dqb_curinodes < b.dqb_isoftlimit {
        flags &= !DQ_INODS;
    }
    (b, flags | DQ_MOD)
}

/// `setuse` (`Q_SETUSE`): set current inode and block usage, read from the user address
/// `addr`.
fn setuse(mp: &'static Mount, id: u32, type_: usize, addr: usize) -> Result<(), Errno> {
    let ump = vfstoufs(mp);

    let usage: Dqblk = copyin_obj(addr)?;
    // KTRACE (KTR_STRUCT, ktrquota): not configured.

    let dq = dqget(None, id, ump, type_)?;
    dqwait(dq, PINOD + 1, "setuse");
    let (b, flags) = setuse_dqblk(
        dq.dq_dqb.get(),
        dq.dq_flags.get(),
        usage,
        gettime(),
        ump.um_btime[type_].get(),
        ump.um_itime[type_].get(),
    );
    dq.dq_dqb.set(b);
    dq.dq_flags.set(flags);
    dqrele(None, Some(dq));
    Ok(())
}

/// `qsync_vnode`: write a vnode's modified dquots.
fn qsync_vnode(vp: &'static Vnode) -> Result<(), Errno> {
    if vp.v_type.get() == VNON {
        return Ok(());
    }

    if vget(vp, LK_EXCLUSIVE | LK_NOWAIT).is_err() {
        return Ok(());
    }

    for i in 0..MAXQUOTAS {
        if let Some(dq) = vtoi(vp).i_dquot[i].get()
            && dq.isset(DQ_MOD)
        {
            let _ = dqsync(Some(vp), dq);
        }
    }
    vput(vp);
    Ok(())
}

/// `qsync` (`Q_SYNC`): sync quota files to disk.
pub fn qsync(mp: &'static Mount) -> Result<(), Errno> {
    let ump = vfstoufs(mp);

    // Check if the mount point has any quotas. If not, simply return.
    if ump.um_quotas.iter().all(|q| q.get().is_none()) {
        return Ok(());
    }
    // Search vnodes associated with this mount point, synchronizing any modified dquot
    // structures.
    let _ = vfs_mount_foreach_vnode(mp, &mut |vp| qsync_vnode(vp));
    Ok(())
}

// Code pertaining to management of the in-core dquot data structures.

/// `ufs_quota_init`: initialize the quota system.
pub fn ufs_quota_init() {
    let elements = crate::conf::param::INITIALVNODES.load(Ordering::Relaxed);
    let Some(tbl) = hashinit::<DqHash>(elements, M_DQUOT, M_WAITOK) else {
        panic(format_args!("ufs_quota_init: no memory"));
    };
    let mut key = [0u8; 16];
    arc4random_buf(&mut key);
    let mut k0 = [0u8; 8];
    let mut k1 = [0u8; 8];
    k0.copy_from_slice(&key[..8]);
    k1.copy_from_slice(&key[8..]);
    // SAFETY: called from `ufs_init` before any UFS is mounted, so no dquot exists and
    // nothing reads the cells meanwhile (the module's deviations).
    unsafe {
        DQHASHTBL.write(Dqhashtbl(tbl));
        DQHASH.write(tbl.len() as u64 - 1);
        DQHASHKEY.write(SiphashKey {
            k0: u64::from_ne_bytes(k0),
            k1: u64::from_ne_bytes(k1),
        });
    }
    DQFREELIST.0.init();
    NUMDQUOT.store(0, Ordering::Relaxed);
    DESIREDDQUOT.store(DQUOTINC, Ordering::Relaxed);
}

/// The hash chain of the quota file `dqvp` and the identifier `id`.
fn dqhash(dqvp: &Vnode, id: u32) -> &'static ListHead<DqHash> {
    // SAFETY: the cells are written only by `ufs_quota_init`, before any file system is
    // mounted, and read-only afterwards.
    let (tbl, mask, key) = unsafe { (DQHASHTBL.get().0, *DQHASH.get(), DQHASHKEY.get()) };
    if tbl.is_empty() {
        panic(format_args!("dqget: no table"));
    }

    let mut ctx = SiphashCtx::default();
    SipHash24_Init(&mut ctx, key);
    SipHash24_Update(&mut ctx, &(ptr::from_ref(dqvp) as usize).to_ne_bytes());
    SipHash24_Update(&mut ctx, &u64::from(id).to_ne_bytes());
    &tbl[(SipHash24_End(&mut ctx) & mask) as usize]
}

/// Whether `vp` is `dqvp` (the caller already holds the quota file's lock).
fn same(vp: Option<&Vnode>, dqvp: &Vnode) -> bool {
    vp.is_some_and(|vp| ptr::eq(vp, dqvp))
}

/// A transfer of `dq_dqb` to or from the quota file at `dq_id`'s slot.
fn dqio(dqvp: &'static Vnode, dq: &Dquot, rw: UioRw) -> (Result<(), Errno>, usize) {
    let len = size_of::<Dqblk>();
    let mut aiov = [Iovec {
        iov_base: dq.dq_dqb.as_ptr().cast(),
        iov_len: len,
    }];
    let mut auio = Uio {
        uio_iov: &mut aiov,
        uio_offset: Off::from(dq.dq_id.get()) * len as Off,
        uio_resid: len,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: rw,
        uio_procp: None,
    };
    let cred = dq.dq_cred.get().map_or(NOCRED, ptr::from_ref);
    let error = if rw == UioRw::UIO_READ {
        VOP_READ(dqvp, &mut auio, 0, cred)
    } else {
        VOP_WRITE(dqvp, &mut auio, 0, cred)
    };
    (error, auio.uio_resid)
}

/// `dqget`: obtain a dquot structure for the specified identifier and quota file reading
/// the information from the file if necessary. `vp` is the vnode the caller holds locked,
/// if any. `EINVAL` when the quota type is not enabled.
pub fn dqget(
    vp: Option<&'static Vnode>,
    id: u32,
    ump: &'static Ufsmount,
    type_: usize,
) -> Result<&'static Dquot, Errno> {
    let Some(dqvp) = ump.um_quotas[type_].get() else {
        return Err(Errno::EINVAL);
    };
    if ump.um_qflags[type_].get() & QTF_CLOSING != 0 {
        return Err(Errno::EINVAL);
    }
    // Check the cache first.
    let dqh = dqhash(dqvp, id);
    for dq in dqh.iter() {
        if dq.dq_id.get() != id || !dq.dq_vp.get().is_some_and(|v| ptr::eq(v, dqvp)) {
            continue;
        }
        // Cache hit with no references. Take the structure off the free list.
        if dq.dq_cnt.get() == 0 {
            // SAFETY: an unreferenced cached dquot is on the free list.
            unsafe { DQFREELIST.0.remove(dq) };
        }
        dqref(dq);
        return Ok(dq);
    }
    // Not in cache, allocate a new one.
    let maxdquot =
        MAXQUOTAS as i64 * i64::from(crate::conf::param::INITIALVNODES.load(Ordering::Relaxed));
    if DQFREELIST.0.is_empty() && NUMDQUOT.load(Ordering::Relaxed) < maxdquot {
        DESIREDDQUOT.fetch_add(DQUOTINC, Ordering::Relaxed);
    }
    let dq: &'static Dquot =
        if NUMDQUOT.load(Ordering::Relaxed) < DESIREDDQUOT.load(Ordering::Relaxed) {
            let dq = dqalloc()?;
            NUMDQUOT.fetch_add(1, Ordering::Relaxed);
            dq
        } else {
            let Some(dq) = DQFREELIST.0.first() else {
                tablefull("dquot");
                return Err(Errno::EUSERS);
            };
            if dq.dq_cnt.get() != 0 || dq.isset(DQ_MOD) {
                panic(format_args!("free dquot isn't"));
            }
            // SAFETY: `dq` is the first element of the free list, and hashed if
            // `dq_hashed` says so.
            unsafe {
                DQFREELIST.0.remove(dq);
                if dq.dq_hashed.replace(false) {
                    ListHead::<DqHash>::remove(dq);
                }
            }
            if let Some(cr) = dq.dq_cred.take() {
                crfree(cr);
            }
            dq
        };
    // Initialize the contents of the dquot structure.
    if !same(vp, dqvp) {
        let _ = vn_lock(dqvp, LK_EXCLUSIVE | LK_RETRY);
    }
    // SAFETY: `dq` is on no chain (new, or taken off above); it is never freed.
    unsafe { dqh.insert_head(dq) };
    dq.dq_hashed.set(true);
    dqref(dq);
    dq.dq_flags.set(DQ_LOCK);
    dq.dq_id.set(id);
    dq.dq_vp.set(Some(dqvp));
    dq.dq_type.set(type_ as u16);
    if let Some(cr) = um_cred(ump, type_) {
        dq.dq_cred.set(Some(crhold(cr)));
    }
    let (error, resid) = dqio(dqvp, dq, UioRw::UIO_READ);
    if resid == size_of::<Dqblk>() && error.is_ok() {
        dq.dq_dqb.set(Dqblk::default());
    }
    if !same(vp, dqvp) {
        let _ = VOP_UNLOCK(dqvp);
    }
    if dq.isset(DQ_WANT) {
        wakeup(ptr::from_ref(dq));
    }
    dq.dq_flags.set(0);
    // I/O error in reading quota file, release quota structure and reflect problem to
    // caller.
    if let Err(e) = error {
        // SAFETY: `dq` was hashed above.
        unsafe { ListHead::<DqHash>::remove(dq) };
        dq.dq_hashed.set(false);
        dqrele(vp, Some(dq));
        return Err(e);
    }
    // Check for no limit to enforce. Initialize time values if necessary.
    let b = dq.dq_dqb.get();
    if b.dqb_isoftlimit == 0
        && b.dqb_bsoftlimit == 0
        && b.dqb_ihardlimit == 0
        && b.dqb_bhardlimit == 0
    {
        dq.setflag(DQ_FAKE);
    }
    if dq.dq_id.get() != 0 {
        let now = gettime();
        dq.update(|b| {
            if b.dqb_btime == 0 {
                b.dqb_btime = (now + ump.um_btime[type_].get()) as u32;
            }
            if b.dqb_itime == 0 {
                b.dqb_itime = (now + ump.um_itime[type_].get()) as u32;
            }
        });
    }
    Ok(dq)
}

/// `malloc(sizeof *dq, M_DQUOT, M_WAITOK | M_ZERO)`: a new dquot, never freed.
fn dqalloc() -> Result<&'static Dquot, Errno> {
    let Some(p) = malloc(size_of::<Dquot>(), M_DQUOT, M_WAITOK | M_ZERO) else {
        return Err(Errno::ENOMEM);
    };
    let p: NonNull<Dquot> = p.cast();
    // SAFETY: `p` is a fresh allocation of a `Dquot`'s size, aligned by malloc for any
    // kernel structure; dquots are never freed, so the reference lives forever.
    unsafe {
        p.as_ptr().write(Dquot::new());
        Ok(&*p.as_ptr())
    }
}

/// `dqrele`: release a reference to a dquot. `vp` is the vnode the caller holds locked, if
/// any.
pub fn dqrele(vp: Option<&'static Vnode>, dq: Option<&'static Dquot>) {
    let Some(dq) = dq else {
        return;
    };
    if dq.dq_cnt.get() > 1 {
        dq.dq_cnt.set(dq.dq_cnt.get() - 1);
        return;
    }
    if dq.isset(DQ_MOD) {
        let _ = dqsync(vp, dq);
    }
    dq.dq_cnt.set(dq.dq_cnt.get() - 1);
    if dq.dq_cnt.get() > 0 {
        return;
    }
    // SAFETY: an unreferenced dquot is on no free list (`dqget` took it off when it gained
    // its first reference); dquots are never freed.
    unsafe { DQFREELIST.0.insert_tail(dq) };
}

/// `dqsync`: update the disk quota in the quota file. `vp` is the vnode the caller holds
/// locked, if any.
pub fn dqsync(vp: Option<&'static Vnode>, dq: &'static Dquot) -> Result<(), Errno> {
    if !dq.isset(DQ_MOD) {
        return Ok(());
    }
    let Some(dqvp) = dq.dq_vp.get() else {
        panic(format_args!("dqsync: file"));
    };

    if !same(vp, dqvp) {
        let _ = vn_lock(dqvp, LK_EXCLUSIVE | LK_RETRY);
    }
    while dq.isset(DQ_LOCK) {
        dq.setflag(DQ_WANT);
        let _ = tsleep_nsec(ptr::from_ref(dq), PINOD + 2, "dqsync", INFSLP);
        if !dq.isset(DQ_MOD) {
            if !same(vp, dqvp) {
                let _ = VOP_UNLOCK(dqvp);
            }
            return Ok(());
        }
    }
    dq.setflag(DQ_LOCK);
    let (mut error, resid) = dqio(dqvp, dq, UioRw::UIO_WRITE);
    if resid != 0 && error.is_ok() {
        error = Err(Errno::EIO);
    }
    if dq.isset(DQ_WANT) {
        wakeup(ptr::from_ref(dq));
    }
    dq.clrflag(DQ_MOD | DQ_LOCK | DQ_WANT);
    if !same(vp, dqvp) {
        let _ = VOP_UNLOCK(dqvp);
    }
    error
}

/// `ufs_quota_delete`: release the inode's dquots.
pub fn ufs_quota_delete(ip: &Inode) -> Result<(), Errno> {
    let vp = ip.itov();
    for i in 0..MAXQUOTAS {
        if let Some(dq) = ip.i_dquot[i].take() {
            dqrele(Some(vp), Some(dq));
        }
    }

    Ok(())
}

/// `ufs_quotactl` (`vfs_quotactl`): do operations associated with quotas. `arg` is a user
/// address.
pub fn ufs_quotactl(
    mp: &'static Mount,
    cmds: i32,
    uid: Uid,
    arg: usize,
    p: &Proc,
) -> Result<(), Errno> {
    let cred = p.ucred();
    let uid = if uid == Uid::MAX {
        cred.cr_ruid.get()
    } else {
        uid
    };
    let cmd = cmds >> SUBCMDSHIFT;

    match cmd {
        Q_SYNC => {}
        Q_GETQUOTA if uid == cred.cr_ruid.get() => {}
        _ => suser(p)?,
    }

    let type_ = (cmds & SUBCMDMASK) as u32 as usize;
    if type_ >= MAXQUOTAS {
        return Err(Errno::EINVAL);
    }

    if vfs_busy(mp, VB_READ | VB_NOWAIT).is_err() {
        return Ok(());
    }

    let error = match cmd {
        Q_QUOTAON => quotaon(p, mp, type_, arg),
        Q_QUOTAOFF => quotaoff(p, mp, type_),
        Q_SETQUOTA => setquota(mp, uid, type_, arg),
        Q_SETUSE => setuse(mp, uid, type_, arg),
        Q_GETQUOTA => getquota(mp, uid, type_, arg),
        Q_SYNC => qsync(mp),
        _ => Err(Errno::EINVAL),
    };

    vfs_unbusy(mp);
    error
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for disk quotas: the limit and grace logic of `chkdqchg`/`chkiqchg`,
    // `setquota` and `setuse`, and `quotactl(2)` on an FFS image mounted on the host, with a
    // quota file laid out as `quotacheck(8)` leaves it: a user writes until `EDQUOT`, the
    // usage is read back with `Q_GETQUOTA`, and it survives `Q_SYNC`, an unmount with quotas
    // on, `Q_QUOTAOFF` and a remount.

    use std::vec;
    use std::vec::Vec;

    use super::*;
    use crate::kern::kern_descrip::sys_close;
    use crate::kern::kern_prot::crget;
    use crate::kern::sys_generic::sys_write;
    use crate::kern::vfs_syscalls::{sys_chown, sys_open, sys_quotactl, sys_unlink};
    use crate::sys::fcntl::{O_CREAT, O_RDWR};
    use crate::ufs::ffs::ffs_vfsops::tests::{
        DISK as DISKIMG, mount_root, newfs, path, read_file, setup, sys, teardown, unmount_root,
    };
    use crate::ufs::ufs::quota::qcmd;

    const NOW: Time = 1_000_000;
    const WEEK: Time = 7 * 24 * 60 * 60;

    fn dqblk(bhard: u32, bsoft: u32, curb: u32, ihard: u32, isoft: u32, curi: u32) -> Dqblk {
        Dqblk {
            dqb_bhardlimit: bhard,
            dqb_bsoftlimit: bsoft,
            dqb_curblocks: curb,
            dqb_ihardlimit: ihard,
            dqb_isoftlimit: isoft,
            dqb_curinodes: curi,
            dqb_btime: 0,
            dqb_itime: 0,
        }
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn dquot_flags_match_the_c_file() {
        let defs = crate::reftest::defines("sys/ufs/ufs/ufs_quota.c");
        for (name, value) in [
            ("DQ_LOCK", DQ_LOCK),
            ("DQ_WANT", DQ_WANT),
            ("DQ_MOD", DQ_MOD),
            ("DQ_FAKE", DQ_FAKE),
            ("DQ_BLKS", DQ_BLKS),
            ("DQ_INODS", DQ_INODS),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
        assert_eq!(crate::reftest::int(&defs, "DQUOTINC"), Some(DQUOTINC));
        let defs = crate::reftest::defines("sys/ufs/ufs/quota.h");
        assert_eq!(crate::reftest::int(&defs, "MAX_DQ_TIME"), Some(MAX_DQ_TIME));
        assert_eq!(crate::reftest::int(&defs, "MAX_IQ_TIME"), Some(MAX_IQ_TIME));
    }

    #[test]
    fn chklimit_hard_soft_and_grace() {
        // No limits: anything goes.
        assert_eq!(chklimit(1000, 1000, 0, 0, 0, NOW), Limit::Under);
        // The hard limit is reached when the new usage gets to it (">=").
        assert_eq!(chklimit(90, 9, 100, 0, 0, NOW), Limit::Under);
        assert_eq!(chklimit(90, 10, 100, 0, 0, NOW), Limit::HardReached);
        assert_eq!(chklimit(0, 200, 100, 0, 0, NOW), Limit::HardReached);
        // Crossing the soft limit starts the grace time (allowed, with a warning).
        assert_eq!(chklimit(40, 10, 100, 50, 0, NOW), Limit::SoftCrossed);
        assert_eq!(chklimit(40, 9, 100, 50, 0, NOW), Limit::Under);
        // Over it already: allowed until the deadline, refused after it.
        let deadline = (NOW + 10) as u32;
        assert_eq!(chklimit(60, 1, 100, 50, deadline, NOW), Limit::Under);
        assert_eq!(chklimit(60, 1, 100, 50, deadline, NOW + 10), Limit::Under);
        assert_eq!(
            chklimit(60, 1, 100, 50, deadline, NOW + 11),
            Limit::GraceExpired
        );
        // The hard limit wins over the soft one.
        assert_eq!(chklimit(60, 50, 100, 50, deadline, NOW), Limit::HardReached);
        // Giving back is never refused by the soft limit's grace when under it.
        assert_eq!(
            chklimit(60, -20, 100, 50, deadline, NOW + 100),
            Limit::Under
        );
    }

    #[test]
    fn setquota_keeps_usage_and_starts_grace() {
        // The current values come from the old dquot, not from the caller.
        let old = dqblk(0, 0, 70, 0, 0, 5);
        let (b, f) = setquota_dqblk(old, 0, 1000, dqblk(100, 50, 0, 10, 4, 0), NOW, WEEK, WEEK);
        assert_eq!((b.dqb_curblocks, b.dqb_curinodes), (70, 5));
        assert_eq!((b.dqb_bhardlimit, b.dqb_bsoftlimit), (100, 50));
        // Over both new soft limits with none before: both grace times start now.
        assert_eq!(b.dqb_btime, (NOW + WEEK) as u32);
        assert_eq!(b.dqb_itime, (NOW + WEEK) as u32);
        assert_eq!(f, DQ_MOD);

        // Already over the old soft limit: the old grace time is kept for an id but 0.
        let mut old = dqblk(100, 50, 70, 0, 0, 0);
        old.dqb_btime = 1234;
        let (b, _) = setquota_dqblk(old, 0, 1000, dqblk(100, 60, 0, 0, 0, 0), NOW, WEEK, WEEK);
        assert_eq!(b.dqb_btime, 1234);
        // Id 0 carries the file system's grace times: they are taken from the caller.
        let mut newlim = dqblk(0, 0, 0, 0, 0, 0);
        newlim.dqb_btime = 3600;
        newlim.dqb_itime = 7200;
        let (b, f) = setquota_dqblk(old, DQ_BLKS, 0, newlim, NOW, WEEK, WEEK);
        assert_eq!((b.dqb_btime, b.dqb_itime), (3600, 7200));
        // No limits left: just usage; the warning flags clear only under a soft limit.
        assert_eq!(f, DQ_BLKS | DQ_FAKE | DQ_MOD);
        let (_, f) = setquota_dqblk(
            old,
            DQ_BLKS | DQ_FAKE,
            1000,
            dqblk(0, 80, 0, 0, 0, 0),
            NOW,
            1,
            1,
        );
        assert_eq!(f, DQ_MOD);
    }

    #[test]
    fn setuse_sets_usage_and_starts_grace() {
        let old = dqblk(100, 50, 10, 10, 4, 1);
        let (b, f) = setuse_dqblk(
            old,
            DQ_BLKS | DQ_INODS,
            dqblk(9, 9, 60, 9, 9, 5),
            NOW,
            30,
            40,
        );
        assert_eq!((b.dqb_curblocks, b.dqb_curinodes), (60, 5));
        // The limits are not the caller's.
        assert_eq!(
            (b.dqb_bhardlimit, b.dqb_bsoftlimit, b.dqb_isoftlimit),
            (100, 50, 4)
        );
        assert_eq!(b.dqb_btime, (NOW + 30) as u32);
        assert_eq!(b.dqb_itime, (NOW + 40) as u32);
        assert_eq!(f, DQ_BLKS | DQ_INODS | DQ_MOD);
        // Back under the soft limits: the warnings are forgotten, the times stay.
        let (b2, f) = setuse_dqblk(b, f, dqblk(0, 0, 3, 0, 0, 1), NOW + 5, 30, 40);
        assert_eq!(b2.dqb_btime, b.dqb_btime);
        assert_eq!(f, DQ_MOD);
    }

    /// The user whose quota the file system test sets.
    const USER: Uid = 1000;
    /// Their block hard limit, in `DEV_BSIZE` units (50 KB).
    const BHARD: u32 = 100;

    /// A user quota file as `quotacheck(8)` leaves it for a file system where nothing belongs
    /// to `USER` yet, with `edquota(8)`'s hard limit for `USER`: one `dqblk` per id up to
    /// `USER`.
    fn quota_file() -> Vec<u8> {
        let mut f = Vec::new();
        for id in 0..=USER {
            let b = if id == USER {
                dqblk(BHARD, 0, 0, 0, 0, 0)
            } else {
                Dqblk::default()
            };
            for w in [
                b.dqb_bhardlimit,
                b.dqb_bsoftlimit,
                b.dqb_curblocks,
                b.dqb_ihardlimit,
                b.dqb_isoftlimit,
                b.dqb_curinodes,
                b.dqb_btime,
                b.dqb_itime,
            ] {
                f.extend_from_slice(&w.to_ne_bytes());
            }
        }
        f
    }

    /// The `dqblk` of `id` in a quota file's bytes.
    fn entry(file: &[u8], id: Uid) -> Dqblk {
        let off = id as usize * size_of::<Dqblk>();
        let w = |i: usize| {
            let o = off + 4 * i;
            u32::from_ne_bytes([file[o], file[o + 1], file[o + 2], file[o + 3]])
        };
        Dqblk {
            dqb_bhardlimit: w(0),
            dqb_bsoftlimit: w(1),
            dqb_curblocks: w(2),
            dqb_ihardlimit: w(3),
            dqb_isoftlimit: w(4),
            dqb_curinodes: w(5),
            dqb_btime: w(6),
            dqb_itime: w(7),
        }
    }

    /// `quotactl("/", QCMD(cmd, USRQUOTA), uid, addr)`.
    fn quotactl(p: &Proc, cmd: i32, uid: Uid, addr: usize) -> Result<isize, Errno> {
        sys(
            sys_quotactl,
            p,
            &[
                path(b"/\0"),
                qcmd(cmd, USRQUOTA as i32) as usize,
                uid as usize,
                addr,
            ],
        )
    }

    /// `Q_GETQUOTA` of `uid`'s user quota.
    fn getq(p: &Proc, uid: Uid) -> Result<Dqblk, Errno> {
        let mut b = Dqblk::default();
        quotactl(p, Q_GETQUOTA, uid, ptr::from_mut(&mut b) as usize)?;
        Ok(b)
    }

    /// Runs the thread as `uid` (a new credential) until the returned one is put back.
    fn become_user(p: &Proc, uid: Uid) -> *const Ucred {
        let cr = crget();
        for c in [&cr.cr_uid, &cr.cr_ruid, &cr.cr_svuid] {
            c.set(uid);
        }
        for c in [&cr.cr_gid, &cr.cr_rgid, &cr.cr_svgid] {
            c.set(uid);
        }
        p.p_ucred.replace(ptr::from_ref(cr))
    }

    #[test]
    fn quotas_on_an_ffs_image() {
        let mut img = newfs::Image::new(newfs::FFS2_4M);
        img.add_file(b"quota.user", &quota_file());
        let (_g, p) = setup(img.finish());

        let mp = mount_root(p, false);
        // Not on yet: no quota to get.
        assert_eq!(getq(p, USER), Err(Errno::EINVAL));
        quotactl(p, Q_QUOTAON, 0, path(b"/quota.user\0")).unwrap();
        assert_ne!(mp.mnt_flag.get() & MNT_QUOTA, 0);
        let b = getq(p, USER).unwrap();
        assert_eq!(
            (b.dqb_bhardlimit, b.dqb_curblocks, b.dqb_curinodes),
            (BHARD, 0, 0)
        );

        // A file for the user: chown moves the inode's charge to them.
        let fd = sys(
            sys_open,
            p,
            &[path(b"/f\0"), (O_RDWR | O_CREAT) as usize, 0o644],
        )
        .unwrap();
        sys(sys_close, p, &[fd as usize]).unwrap();
        sys(sys_chown, p, &[path(b"/f\0"), USER as usize, 0]).unwrap();
        assert_eq!(getq(p, USER).unwrap().dqb_curinodes, 1);

        // The user writes 8 KB blocks until the hard limit refuses one: six blocks are 96
        // sectors, a seventh would make 112 >= 100.
        let root = become_user(p, USER);
        let fd = sys(sys_open, p, &[path(b"/f\0"), O_RDWR as usize, 0]).unwrap();
        let chunk = vec![0x5au8; 8192];
        let mut written = 0;
        let error = loop {
            match sys(
                sys_write,
                p,
                &[fd as usize, chunk.as_ptr() as usize, chunk.len()],
            ) {
                Ok(n) => written += n as usize,
                Err(e) => break e,
            }
            assert!(
                written <= 1 << 20,
                "the hard limit never stopped the writes"
            );
        };
        assert_eq!(error, Errno::EDQUOT);
        assert_eq!(written, 6 * 8192);
        sys(sys_close, p, &[fd as usize]).unwrap();
        // A user may read their own quota, not somebody else's.
        let b = getq(p, USER).unwrap();
        assert_eq!((b.dqb_curblocks, b.dqb_curinodes), (96, 1));
        assert_eq!(getq(p, 0), Err(Errno::EPERM));
        assert_eq!(quotactl(p, Q_QUOTAOFF, 0, 0), Err(Errno::EPERM));
        let user = p.p_ucred.replace(root);
        // SAFETY: `become_user`'s credential, whose reference the thread gives up.
        crfree(unsafe { &*user });

        // Q_SYNC writes the usage into the quota file.
        quotactl(p, Q_SYNC, 0, 0).unwrap();
        let e = entry(&read_file(p, b"/quota.user\0").unwrap(), USER);
        assert_eq!(
            (e.dqb_bhardlimit, e.dqb_curblocks, e.dqb_curinodes),
            (BHARD, 96, 1)
        );

        // Unmounting with quotas on turns them off (ffs_flushfiles); a remount reads the usage
        // back from the file.
        unmount_root(p, mp);
        newfs::check(&DISKIMG.lock().unwrap(), true);
        let mp = mount_root(p, false);
        assert_eq!(mp.mnt_flag.get() & MNT_QUOTA, 0);
        quotactl(p, Q_QUOTAON, 0, path(b"/quota.user\0")).unwrap();
        let b = getq(p, USER).unwrap();
        assert_eq!(
            (b.dqb_bhardlimit, b.dqb_curblocks, b.dqb_curinodes),
            (BHARD, 96, 1)
        );

        // Q_SETQUOTA raises the limit (the usage stays the kernel's), Q_SETUSE sets the usage.
        let mut lim = dqblk(BHARD * 2, 0, 12345, 0, 0, 999);
        quotactl(p, Q_SETQUOTA, USER, ptr::from_mut(&mut lim) as usize).unwrap();
        let b = getq(p, USER).unwrap();
        assert_eq!(
            (b.dqb_bhardlimit, b.dqb_curblocks, b.dqb_curinodes),
            (BHARD * 2, 96, 1)
        );
        let mut usage = dqblk(0, 0, 90, 0, 0, 1);
        quotactl(p, Q_SETUSE, USER, ptr::from_mut(&mut usage) as usize).unwrap();
        assert_eq!(getq(p, USER).unwrap().dqb_curblocks, 90);

        // Removing the file gives the blocks and the inode back.
        sys(sys_unlink, p, &[path(b"/f\0")]).unwrap();
        let b = getq(p, USER).unwrap();
        assert_eq!((b.dqb_curblocks, b.dqb_curinodes), (0, 0));

        // Q_QUOTAOFF writes the dquots back and closes the file; a remount and Q_QUOTAON find
        // the new limit on disk.
        quotactl(p, Q_QUOTAOFF, 0, 0).unwrap();
        assert_eq!(mp.mnt_flag.get() & MNT_QUOTA, 0);
        assert_eq!(getq(p, USER), Err(Errno::EINVAL));
        unmount_root(p, mp);
        let mp = mount_root(p, false);
        quotactl(p, Q_QUOTAON, 0, path(b"/quota.user\0")).unwrap();
        let b = getq(p, USER).unwrap();
        assert_eq!(
            (b.dqb_bhardlimit, b.dqb_curblocks, b.dqb_curinodes),
            (BHARD * 2, 0, 0)
        );
        unmount_root(p, mp);
        newfs::check(&DISKIMG.lock().unwrap(), true);
        teardown();
    }
}
/* </TESTS> */
