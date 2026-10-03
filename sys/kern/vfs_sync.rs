/*       $OpenBSD: vfs_sync.c,v 1.73 2024/10/18 05:52:32 miod Exp $  */
/* <LICENSES> */
/*
 *  Portions of this code are:
 *
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
 */
/* </LICENSES> */

//! The syncer daemon (`update`): a wheel of `SYNCER_MAXDELAY` worklists of vnodes with dirty
//! buffers, one turned per second, each vnode's `VOP_FSYNC(MNT_LAZY)` run when its slot comes
//! up; and the syncer vnode each writable mount gets, whose lazy `fsync` syncs the mount.
//!
//! Upstream: sys/kern/vfs_sync.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `syncer_maxdelay` keeps its lowercase name beside the constant `SYNCER_MAXDELAY`.
//! - `syncer_workitem_pending` (the `hashinit` table) is a pointer and a mask in atomics,
//!   reached through [`syncer_slot`]; the other globals are atomics, `syncerproc` an
//!   `AtomicPtr<Proc>`.
//! - `sync_vops` fills the slots the C sets to `nullop`/`eopnotsupp` with closures, as the
//!   other tables do; `vfs_allocate_syncvnode`'s `static long start, incr, next` are atomics.
//! - The C's `speedup_syncer` does not exist at this version (nothing to port).

use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicI64, AtomicPtr, AtomicUsize, Ordering};

use crate::kern::kern_subr::hashinit;
use crate::kern::kern_synch::tsleep_nsec;
use crate::kern::kern_tc::getnsecuptime;
use crate::kern::sched_bsd::r#yield;
use crate::kern::subr_prf::panic;
use crate::kern::subr_xxx::{eopnotsupp, nullop};
use crate::kern::vfs_subr::{getnewvnode, vfs_busy, vfs_unbusy, vget, vput};
use crate::kern::vfs_vops::{VOP_FSYNC, VOP_UNLOCK};
use crate::machine::cpu::curproc;
use crate::machine::intr::{splbio, splx};
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_NOWAIT};
use crate::sys::malloc::{M_VNODE, M_WAITOK};
use crate::sys::mount::{MNT_ASYNC, MNT_LAZY, Mount, VB_NOWAIT, VB_READ, VFS_SYNC};
use crate::sys::param::PPAUSE;
use crate::sys::proc::Proc;
use crate::sys::queue::ListHead;
use crate::sys::sched::sched_pause;
use crate::sys::time::sec_to_nsec;
use crate::sys::vnode::{
    VBIOONSYNCLIST, VNON, VSynclist, VT_VFS, Vnode, VopFsyncArgs, VopInactiveArgs, VopPrintArgs,
    Vops,
};

/// `SYNCER_MAXDELAY`: maximum sync delay time.
pub const SYNCER_MAXDELAY: i32 = 32;
/// `SYNCER_DEFAULT`: default sync delay time.
pub const SYNCER_DEFAULT: i32 = 30;

/// `struct synclist`: one slot of the wheel.
pub type Synclist = ListHead<VSynclist>;

/// `syncer_maxdelay`: maximum delay time.
#[allow(non_upper_case_globals)] // SYNCER_MAXDELAY is the constant of the same name
pub static syncer_maxdelay: AtomicI32 = AtomicI32::new(SYNCER_MAXDELAY);
/// `syncdelay`: time to delay syncing vnodes.
pub static SYNCDELAY: AtomicI32 = AtomicI32::new(SYNCER_DEFAULT);

/// `syncer_delayno`: the next slot to process.
pub static SYNCER_DELAYNO: AtomicI32 = AtomicI32::new(0);
/// `syncer_mask`: the wheel's size minus one.
pub static SYNCER_MASK: AtomicUsize = AtomicUsize::new(0);
/// `syncer_workitem_pending`: the wheel (`hashinit`'s table).
static SYNCER_WORKITEM_PENDING: AtomicPtr<Synclist> = AtomicPtr::new(ptr::null_mut());

/// `syncerproc`: the syncer thread.
pub static SYNCERPROC: AtomicPtr<Proc> = AtomicPtr::new(ptr::null_mut());
/// `syncer_chan`: what the syncer sleeps on between rounds.
pub static SYNCER_CHAN: AtomicI32 = AtomicI32::new(0);

/// `syncerproc`, NULL before `main` creates it.
pub fn syncerproc() -> *const Proc {
    SYNCERPROC.load(Ordering::Relaxed)
}

/// `&syncer_workitem_pending[slot]`.
pub fn syncer_slot(slot: usize) -> &'static Synclist {
    let base = SYNCER_WORKITEM_PENDING.load(Ordering::Relaxed);
    if base.is_null() || slot > SYNCER_MASK.load(Ordering::Relaxed) {
        panic(format_args!("syncer_slot: slot {slot} of no wheel"));
    }
    // SAFETY: `vn_initialize_syncerd` stored a `hashinit` table of `syncer_mask + 1` heads
    // that is never freed, and `slot` is within it.
    unsafe { &*base.add(slot) }
}

// The workitem queue.
//
// It is useful to delay writes of file data and filesystem metadata for tens of seconds so
// that quickly created and deleted files need not waste disk bandwidth being created and
// removed. To realize this, we append vnodes to a "workitem" queue. When running with a soft
// updates implementation, most pending metadata dependencies should not wait for more than a
// few seconds. Thus, mounted block devices are delayed only about half the time that file
// data is delayed. Similarly, directory updates are more critical, so are only delayed about
// a third the time that file data is delayed. Thus, there are SYNCER_MAXDELAY queues that are
// processed round-robin at a rate of one each second (driven off the filesystem syncer
// process). The syncer_delayno variable indicates the next queue that is to be processed.
// Items that need to be processed soon are placed in this queue:
//
//	syncer_workitem_pending[syncer_delayno]
//
// A delay of fifteen seconds is done by placing the request fifteen entries later in the
// queue:
//
//	syncer_workitem_pending[(syncer_delayno + 15) & syncer_mask]

/// `vn_initialize_syncerd()`: allocates the wheel.
pub fn vn_initialize_syncerd() {
    let Some(table) =
        hashinit::<VSynclist>(syncer_maxdelay.load(Ordering::Relaxed), M_VNODE, M_WAITOK)
    else {
        panic(format_args!("vn_initialize_syncerd: out of memory"));
    };
    SYNCER_MASK.store(table.len() - 1, Ordering::Relaxed);
    SYNCER_WORKITEM_PENDING.store(table.as_ptr().cast_mut(), Ordering::Relaxed);
    syncer_maxdelay.store(table.len() as i32, Ordering::Relaxed);
}

/// Add an item to the syncer work queue.
pub fn vn_syncer_add_to_worklist(vp: &'static Vnode, delay: i32) {
    let maxdelay = syncer_maxdelay.load(Ordering::Relaxed);
    let delay = delay.min(maxdelay - 2);
    let slot = (SYNCER_DELAYNO.load(Ordering::Relaxed) + delay) as usize
        & SYNCER_MASK.load(Ordering::Relaxed);

    let s = splbio();
    if vp.v_bioflag.get() & VBIOONSYNCLIST != 0 {
        // SAFETY: a vnode marked `VBIOONSYNCLIST` is on one slot of the wheel.
        unsafe { ListHead::<VSynclist>::remove(vp) };
    }

    vp.v_bioflag.set(vp.v_bioflag.get() | VBIOONSYNCLIST);
    // SAFETY: the vnode was just taken off the wheel (or was on none); vnodes never move.
    unsafe { syncer_slot(slot).insert_head(vp) };
    splx(s);
}

/// System filesystem synchronizer daemon.
pub fn syncer_thread(_arg: *mut core::ffi::c_void) {
    let Some(p) = curproc() else {
        panic(format_args!("syncer_thread: no curproc"));
    };

    loop {
        let start = getnsecuptime();

        // Push files whose dirty time has expired.
        let mut s = splbio();
        let slp = syncer_slot(SYNCER_DELAYNO.load(Ordering::Relaxed) as usize);

        let mut delayno = SYNCER_DELAYNO.load(Ordering::Relaxed) + 1;
        if delayno == syncer_maxdelay.load(Ordering::Relaxed) {
            delayno = 0;
        }
        SYNCER_DELAYNO.store(delayno, Ordering::Relaxed);

        while let Some(vp) = slp.first() {
            if vget(vp, LK_EXCLUSIVE | LK_NOWAIT).is_err() {
                // If we fail to get the lock, we move this vnode one second ahead in time.
                // XXX - no good, but the best we can do.
                vn_syncer_add_to_worklist(vp, 1);
                continue;
            }
            splx(s);
            let _ = VOP_FSYNC(vp, p.p_ucred.get(), MNT_LAZY, p);
            vput(vp);
            s = splbio();
            if slp.first().is_some_and(|f| ptr::eq(f, vp)) {
                // Note: disk vps can remain on the worklist too with no dirty blocks, but
                // since sync_fsync() moves it to a different slot we are safe.
                #[cfg(feature = "diagnostic")]
                if vp.v_dirtyblkhd.is_empty() && vp.v_type.get() != crate::sys::vnode::VBLK {
                    crate::kern::vfs_subr::vprint(Some("fsync failed"), vp);
                    if let Some(mp) = vp.v_mount.get() {
                        let (name, len) = mp.mntonname();
                        crate::kprintf!(
                            "mounted on: {}\n",
                            crate::kern::subr_prf::Str(&name[..len])
                        );
                    }
                    panic(format_args!("syncer_thread: fsync failed"));
                }
                // Put us back on the worklist. The worklist routine will remove us from our
                // current position and then add us back in at a later position.
                vn_syncer_add_to_worklist(vp, SYNCDELAY.load(Ordering::Relaxed));
            }

            sched_pause(r#yield);
        }

        splx(s);

        // If it has taken us less than a second to process the current work, then wait.
        // Otherwise start right over again. We can still lose time if any single round takes
        // more than two seconds, but it does not really matter as we are just trying to
        // generally pace the filesystem activity.
        let elapsed = getnsecuptime() - start;
        if elapsed < sec_to_nsec(1) {
            let _ = tsleep_nsec(
                ptr::from_ref(&SYNCER_CHAN),
                PPAUSE,
                "syncer",
                sec_to_nsec(1) - elapsed,
            );
        }
    }
}

/// `sync_vops`: routines to create and manage a filesystem syncer vnode.
pub static SYNC_VOPS: Vops = Vops {
    vop_close: Some(|_| nullop()),
    vop_fsync: Some(sync_fsync),
    vop_inactive: Some(sync_inactive),
    vop_reclaim: Some(|_| nullop()),
    vop_lock: Some(|_| nullop()),
    vop_unlock: Some(|_| nullop()),
    vop_islocked: Some(|_| 0), // nullop
    vop_print: Some(sync_print),

    vop_abortop: None,
    vop_access: None,
    vop_advlock: None,
    vop_bmap: None,
    vop_bwrite: None,
    vop_create: None,
    vop_getattr: None,
    vop_ioctl: None,
    vop_link: None,
    vop_lookup: None,
    vop_mknod: None,
    vop_open: None,
    vop_pathconf: None,
    vop_read: None,
    vop_readdir: None,
    vop_readlink: None,
    vop_remove: Some(|_| eopnotsupp()),
    vop_rename: None,
    vop_revoke: None,
    vop_mkdir: None,
    vop_rmdir: None,
    vop_setattr: None,
    vop_strategy: None,
    vop_symlink: None,
    vop_write: None,
    vop_kqfilter: None,
};

/// Create a new filesystem syncer vnode for the specified mount point.
pub fn vfs_allocate_syncvnode(mp: &'static Mount) -> Result<(), Errno> {
    static START: AtomicI64 = AtomicI64::new(0);
    static INCR: AtomicI64 = AtomicI64::new(0);
    static NEXT: AtomicI64 = AtomicI64::new(0);

    // Allocate a new vnode
    let vp = match getnewvnode(VT_VFS, Some(mp), &SYNC_VOPS) {
        Ok(vp) => vp,
        Err(error) => {
            mp.mnt_syncer.set(None);
            return Err(error);
        }
    };
    vp.v_writecount.set(1);
    vp.v_type.set(VNON);
    // Place the vnode onto the syncer worklist. We attempt to scatter them about on the list
    // so that they will go off at evenly distributed times even if all the filesystems are
    // mounted at once.
    let maxdelay = i64::from(syncer_maxdelay.load(Ordering::Relaxed));
    let mut next = NEXT.load(Ordering::Relaxed) + INCR.load(Ordering::Relaxed);
    if next == 0 || next > maxdelay {
        let mut start = START.load(Ordering::Relaxed) / 2;
        let mut incr = INCR.load(Ordering::Relaxed) / 2;
        if start == 0 {
            start = maxdelay / 2;
            incr = maxdelay;
        }
        START.store(start, Ordering::Relaxed);
        INCR.store(incr, Ordering::Relaxed);
        next = start;
    }
    NEXT.store(next, Ordering::Relaxed);
    vn_syncer_add_to_worklist(vp, next as i32);
    mp.mnt_syncer.set(Some(vp));
    Ok(())
}

/// Do a lazy sync of the filesystem.
pub fn sync_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    let syncvp = ap.a_vp;
    let Some(mp) = syncvp.v_mount.get() else {
        panic(format_args!("sync_fsync: syncer vnode without a mount"));
    };

    // We only need to do something if this is a lazy evaluation.
    if ap.a_waitfor != MNT_LAZY {
        return Ok(());
    }

    // Move ourselves to the back of the sync list.
    vn_syncer_add_to_worklist(syncvp, SYNCDELAY.load(Ordering::Relaxed));

    // Walk the list of vnodes pushing all that are dirty and not already on the sync list.
    if vfs_busy(mp, VB_READ | VB_NOWAIT).is_ok() {
        let asyncflag = mp.mnt_flag.get() & MNT_ASYNC;
        mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_ASYNC);
        let _ = VFS_SYNC(mp, MNT_LAZY, 0, ap.a_cred, ap.a_p);
        if asyncflag != 0 {
            mp.mnt_flag.set(mp.mnt_flag.get() | MNT_ASYNC);
        }
        vfs_unbusy(mp);
    }

    Ok(())
}

/// The syncer vnode is no longer needed and is being decommissioned.
pub fn sync_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    if vp.v_usecount.get() == 0 {
        let _ = VOP_UNLOCK(vp);
        return Ok(());
    }

    if let Some(mp) = vp.v_mount.get() {
        mp.mnt_syncer.set(None);
    }

    let s = splbio();

    // SAFETY: the syncer vnode is on the wheel from `vfs_allocate_syncvnode` until here.
    unsafe { ListHead::<VSynclist>::remove(vp) };
    vp.v_bioflag.set(vp.v_bioflag.get() & !VBIOONSYNCLIST);

    splx(s);

    vp.v_writecount.set(0);
    vput(vp);

    Ok(())
}

/// Print out a syncer vnode.
pub fn sync_print(_ap: &mut VopPrintArgs) -> Result<(), Errno> {
    #[cfg(any(feature = "debug", feature = "diagnostic"))]
    crate::kprintf!("syncer vnode\n");

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::assert;

    use super::*;
    use crate::kern::vfs_subr::tests::{setup, testfs::TESTFS_CONF};
    use crate::kern::vfs_subr::vfs_mount_alloc;

    /// Whether `vp` is on slot `slot` (modulo the wheel) of the wheel.
    fn on_slot(slot: usize, vp: &Vnode) -> bool {
        syncer_slot(slot & SYNCER_MASK.load(Ordering::Relaxed))
            .iter()
            .any(|v| ptr::eq(v, vp))
    }

    #[test]
    fn syncer_vnodes_go_on_the_wheel_and_move_by_their_delay() {
        let _g = setup();
        assert!(SYNCER_MASK.load(Ordering::Relaxed) + 1 >= SYNCER_MAXDELAY as usize);
        let base = SYNCER_DELAYNO.load(Ordering::Relaxed) as usize;
        let max = syncer_maxdelay.load(Ordering::Relaxed) as usize;

        let mp = vfs_mount_alloc(None, &TESTFS_CONF);
        vfs_allocate_syncvnode(mp).expect("a syncer vnode");
        let vp = mp.mnt_syncer.get().expect("mnt_syncer");
        assert!(vp.v_bioflag.get() & VBIOONSYNCLIST != 0);
        assert!(core::ptr::eq(vp.op(), &SYNC_VOPS));
        assert!((0..max).filter(|&d| on_slot(base + d, vp)).count() == 1);

        // A later add moves it; the delay is clamped to the wheel.
        vn_syncer_add_to_worklist(vp, 1000);
        assert!(on_slot(base + max - 2, vp));
        vn_syncer_add_to_worklist(vp, 3);
        assert!(on_slot(base + 3, vp));
        assert!(!on_slot(base + max - 2, vp));
    }
}
