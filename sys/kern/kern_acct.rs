/*	$OpenBSD: kern_acct.c,v 1.51 2026/09/26 15:03:48 deraadt Exp $	*/
/*	$NetBSD: kern_acct.c,v 1.42 1996/02/04 02:15:12 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1994 Christopher G. Demetriou
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)kern_acct.c	8.1 (Berkeley) 6/14/93
 */
/* </LICENSES> */

//! Process accounting: `kern/kern_acct.c`.
//!
//! Upstream: sys/kern/kern_acct.c @ 3ce1f3f79392
//!
//! Status: `ported`. `option ACCOUNTING` is configured, as in GENERIC: `sys_acct`, the
//! record `exit1` writes (`acct_process`), `encode_comp_t`, the free-space watcher thread
//! (`acct_start`, `acct_thread`) and `acct_shutdown` (from `vfs_shutdown`).
//!
//! ## Deviations
//! - `ac_tty` is -1 for every process: the tty layer (`struct tty`, `t_dev`) is not here
//!   yet, so no session has a controlling terminal to name.
//! - `acct_thread` hands `VFS_STATFS` the watcher thread where the C passes NULL.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use libkern::StaticCell;

use crate::conf::param::{HZ, TICK};
use crate::kern::kern_kthread::{kthread_create, kthread_exit};
use crate::kern::kern_prot::suser;
use crate::kern::kern_resource::{calctsru, tuagg_get_process};
use crate::kern::kern_rwlock::{rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write};
use crate::kern::kern_synch::{rwsleep_nsec, wakeup};
use crate::kern::kern_tc::{nanoboottime, nanouptime};
use crate::kern::vfs_lookup::ndinit;
use crate::kern::vfs_vnops::{vn_close, vn_open, vn_rdwr};
use crate::kern::vfs_vops::VOP_UNLOCK;
use crate::log;
use crate::machine::cpu::curproc;
use crate::sys::acct::{AHZ, Acct, Comp};
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FWRITE, O_APPEND};
use crate::sys::mount::{Statfs, VFS_STATFS};
use crate::sys::namei::{NiDirp, UNVEIL_WRITE};
use crate::sys::param::PPAUSE;
use crate::sys::proc::{Proc, Tusage};
use crate::sys::rwlock::Rwlock;
use crate::sys::syscallargs::SysAcctArgs;
use crate::sys::syslog::LOG_NOTICE;
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::time::{sec_to_nsec, timespecadd, timespecsub};
use crate::sys::types::Register;
use crate::sys::ucred::NOCRED;
use crate::sys::uio::{UioRw, UioSeg};
use crate::sys::vnode::{IO_APPEND, IO_NOLIMIT, IO_UNIT, VBAD, VREG, Vnode};

/// `MANTSIZE`: 13 bit mantissa.
const MANTSIZE: u32 = 13;
/// `EXPSIZE`: base 8 (3 bit) exponent.
const EXPSIZE: u32 = 3;
/// `MAXFRACT`: maximum fractional value.
const MAXFRACT: u64 = (1 << MANTSIZE) - 1;

/// `acctp` and `savacctp`: the accounting vnode, and the one saved while the file system
/// is short of space. Protected by: `acct_lock`.
struct AcctVnodes {
    /// `acctp`.
    acctp: Option<&'static Vnode>,
    /// `savacctp`.
    savacctp: Option<&'static Vnode>,
}

/// `acctp`, `savacctp` (see [`AcctVnodes`]).
static ACCT: StaticCell<AcctVnodes> = StaticCell::new(AcctVnodes {
    acctp: None,
    savacctp: None,
});

/// `acct_lock`: protecting `acctp` and `savacctp`.
static ACCT_LOCK: Rwlock = Rwlock::new("acctlk");

/// `acctsuspend`: stop accounting when < 2% free space left.
pub static ACCTSUSPEND: AtomicI32 = AtomicI32::new(2);
/// `acctresume`: resume when free space risen to > 4%.
pub static ACCTRESUME: AtomicI32 = AtomicI32::new(4);
/// `acctrate`: delay (in seconds) between space checks.
pub static ACCTRATE: AtomicI32 = AtomicI32::new(15);

/// `acct_proc`: the free-space watcher, null when not running.
static ACCT_PROC: AtomicPtr<Proc> = AtomicPtr::new(ptr::null_mut());

/// The accounting vnodes, under `acct_lock`.
///
/// # Safety
///
/// The caller holds `acct_lock` (for writing if it changes them) and does not keep the
/// reference across a release of the lock.
unsafe fn acct_vnodes<'a>() -> &'a mut AcctVnodes {
    // SAFETY: the caller's contract.
    unsafe { ACCT.get_mut() }
}

/// Accounting system call. Written based on the specification and previous implementation
/// done by Mark Tinguely.
pub fn sys_acct(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysAcctArgs = sysargs(v);
    let path = uap.path.get() as usize;
    let mut newvp: Option<&'static Vnode> = None;

    // Make sure that the caller is root.
    suser(p)?;

    // If accounting is to be started to a file, open that file for writing and make sure
    // it's 'normal'.
    if path != 0 {
        let mut nd = ndinit(0, 0, NiDirp::User(path), p);
        nd.ni_unveil = UNVEIL_WRITE;
        vn_open(&mut nd, FWRITE | O_APPEND, 0)?;
        let Some(vp) = nd.ni_vp else {
            return Err(Errno::ENOENT);
        };
        let _ = VOP_UNLOCK(vp);
        if vp.v_type.get() != VREG {
            let _ = vn_close(vp, FWRITE, p.p_ucred.get(), Some(p));
            return Err(Errno::EACCES);
        }
        newvp = Some(vp);
    }

    rw_enter_write(&ACCT_LOCK);
    // SAFETY: `acct_lock` is held for writing until the end.
    let acct = unsafe { acct_vnodes() };

    // If accounting was previously enabled, kill the old space-watcher, close the file,
    // and (if no new file was specified, leave).
    if let Some(old) = acct.acctp.or(acct.savacctp) {
        wakeup(ptr::from_ref(&ACCT_PROC));
        let _ = vn_close(old, FWRITE, p.p_ucred.get(), Some(p));
        acct.acctp = None;
        acct.savacctp = None;
    }
    let mut error = Ok(());
    if let Some(vp) = newvp {
        // Save the new accounting file vnode, and schedule the new free space watcher.
        acct.acctp = Some(vp);
        error = acct_start();
        if error.is_err() {
            acct.acctp = None;
            let _ = vn_close(vp, FWRITE, p.p_ucred.get(), Some(p));
        }
    }

    // out:
    rw_exit_write(&ACCT_LOCK);
    error
}

/// Write out process accounting information, on process exit. Data to be written out is
/// specified in Leffler, et al. and are enumerated below. (They're also noted in the
/// system "acct.h" header file.)
pub fn acct_process(p: &Proc) -> Result<(), Errno> {
    let pr = p.process();

    // If accounting isn't enabled, don't bother
    // SAFETY: an unlocked peek, as the C's; checked again under the lock.
    if unsafe { acct_vnodes() }.acctp.is_none() {
        return Ok(());
    }

    rw_enter_read(&ACCT_LOCK);

    // Check the vnode again in case accounting got disabled while waiting for the lock.
    // SAFETY: `acct_lock` is held for reading; nothing is changed.
    let Some(vp) = unsafe { acct_vnodes() }.acctp else {
        rw_exit_read(&ACCT_LOCK);
        return Ok(());
    };

    // Get process accounting information.

    // (1) The name of the command that ran
    let mut ac_comm = [0u8; 24];
    let comm = pr.comm();
    let n = comm.len().min(ac_comm.len());
    ac_comm[..n].copy_from_slice(&comm[..n]);

    // (2) The amount of user and system time that was used
    let tu = Tusage::new();
    tuagg_get_process(&tu, pr);
    let (ut, st, _) = calctsru(&tu);

    // (3) The elapsed time the command ran (and its starting time)
    let uptime = nanouptime();
    let booted = nanoboottime();
    let realstart = timespecadd(&booted, &pr.ps_start.get());
    let elapsed = timespecsub(&uptime, &pr.ps_start.get());

    // (4) The average amount of memory used
    let r = &p.p_ru;
    let tmp = timespecadd(&ut, &st);
    let t = tmp.tv_sec * i64::from(HZ.load(Ordering::Relaxed))
        + tmp.tv_nsec / (1000 * i64::from(TICK.load(Ordering::Relaxed)));
    let ac_mem = if t != 0 {
        ((r.ru_ixrss.get() + r.ru_idrss.get() + r.ru_isrss.get()) / t) as u32
    } else {
        0
    };

    let cred = pr.ucred();
    let acct = Acct {
        ac_comm,
        ac_utime: encode_comp_t(ut.tv_sec as u64, ut.tv_nsec as u64),
        ac_stime: encode_comp_t(st.tv_sec as u64, st.tv_nsec as u64),
        ac_etime: encode_comp_t(elapsed.tv_sec as u64, elapsed.tv_nsec as u64),
        // (5) The number of disk I/O operations done
        ac_io: encode_comp_t((r.ru_inblock.get() + r.ru_oublock.get()) as u64, 0),
        ac_btime: realstart.tv_sec,
        // (6) The UID and GID of the process
        ac_uid: cred.cr_ruid.get(),
        ac_gid: cred.cr_rgid.get(),
        ac_mem,
        // (7) The terminal from which the process was started: PS_CONTROLT and the
        // session's s_ttyp->t_dev (see the module's deviations).
        ac_tty: -1,
        // Extensions
        ac_pid: pr.ps_pid.get(),
        // (8) The flags that tell how process terminated or misbehaved.
        ac_flag: pr.ps_acflag.get(),
    };

    // Now, just write the accounting information to the file.
    let error = vn_rdwr(
        UioRw::UIO_WRITE,
        vp,
        ptr::from_ref(&acct).cast_mut().cast::<c_void>(),
        size_of::<Acct>(),
        0,
        UioSeg::UIO_SYSSPACE,
        IO_APPEND | IO_UNIT | IO_NOLIMIT,
        p.p_ucred.get(),
        None,
        Some(p),
    );

    // out:
    rw_exit_read(&ACCT_LOCK);
    error
}

/// `encode_comp_t` converts from ticks in seconds and nanoseconds to ticks in 1/AHZ
/// seconds. The encoding is described in Leffler, et al., on page 63.
pub fn encode_comp_t(s: u64, ns: u64) -> Comp {
    let mut exp: u64 = 0;
    let mut rnd: u64 = 0;
    let mut s = s.wrapping_mul(u64::from(AHZ));
    s = s.wrapping_add(ns / (1_000_000_000 / u64::from(AHZ))); // Maximize precision.

    while s > MAXFRACT {
        rnd = s & (1 << (EXPSIZE - 1)); // Round up?
        s >>= EXPSIZE; // Base 8 exponent == 3 bit shift.
        exp += 1;
    }

    // If we need to round up, do it (and handle overflow correctly).
    if rnd != 0 {
        s += 1;
        if s > MAXFRACT {
            s >>= EXPSIZE;
            exp += 1;
        }
    }

    // Clean it up and polish it off.
    exp <<= MANTSIZE; // Shift the exponent into place
    exp += s; // and add on the mantissa.
    exp as Comp
}

/// `acct_start`: start the free-space watcher, unless it is already running.
pub fn acct_start() -> Result<(), Errno> {
    // Already running.
    if !ACCT_PROC.load(Ordering::Relaxed).is_null() {
        return Ok(());
    }

    let p = kthread_create(acct_thread, ptr::null_mut(), b"acct")?;
    ACCT_PROC.store(ptr::from_ref(p).cast_mut(), Ordering::Relaxed);
    Ok(())
}

/// Periodically check the file system to see if accounting should be turned on or off.
/// Beware the case where the vnode has been vgone()'d out from underneath us, e.g. when the
/// file system containing the accounting file has been forcibly unmounted.
pub fn acct_thread(_arg: *mut c_void) {
    let p = curproc();
    let mut sb = Statfs::default();

    rw_enter_write(&ACCT_LOCK);
    loop {
        // SAFETY: `acct_lock` is held for writing here (`rwsleep_nsec` gives it back
        // before returning).
        let acct = unsafe { acct_vnodes() };
        if let Some(sav) = acct.savacctp {
            if sav.v_type.get() == VBAD {
                let _ = vn_close(sav, FWRITE, NOCRED, p);
                acct.savacctp = None;
                break;
            }
            if let (Some(mp), Some(p)) = (sav.v_mount.get(), p) {
                let _ = VFS_STATFS(mp, &mut sb, p);
            }
            if sb.f_bavail
                > i64::from(ACCTRESUME.load(Ordering::Relaxed)) * sb.f_blocks as i64 / 100
            {
                acct.acctp = Some(sav);
                acct.savacctp = None;
                log!(LOG_NOTICE, "Accounting resumed\n");
            }
        } else if let Some(cur) = acct.acctp {
            if cur.v_type.get() == VBAD {
                let _ = vn_close(cur, FWRITE, NOCRED, p);
                acct.acctp = None;
                break;
            }
            if let (Some(mp), Some(p)) = (cur.v_mount.get(), p) {
                let _ = VFS_STATFS(mp, &mut sb, p);
            }
            if sb.f_bavail
                <= i64::from(ACCTSUSPEND.load(Ordering::Relaxed)) * sb.f_blocks as i64 / 100
            {
                acct.savacctp = Some(cur);
                acct.acctp = None;
                log!(LOG_NOTICE, "Accounting suspended\n");
            }
        } else {
            break;
        }
        let _ = rwsleep_nsec(
            ptr::from_ref(&ACCT_PROC),
            &ACCT_LOCK,
            PPAUSE,
            "acct",
            sec_to_nsec(ACCTRATE.load(Ordering::Relaxed) as u64),
        );
    }
    ACCT_PROC.store(ptr::null_mut(), Ordering::Relaxed);
    rw_exit_write(&ACCT_LOCK);
    kthread_exit(0);
}

/// `acct_shutdown`: close the accounting file (`vfs_shutdown`).
pub fn acct_shutdown() {
    let p = curproc();

    rw_enter_write(&ACCT_LOCK);
    // SAFETY: `acct_lock` is held for writing.
    let acct = unsafe { acct_vnodes() };
    if let Some(vp) = acct.acctp.or(acct.savacctp) {
        let _ = vn_close(vp, FWRITE, NOCRED, p);
        acct.acctp = None;
        acct.savacctp = None;
    }
    rw_exit_write(&ACCT_LOCK);
}

const _: () = {
    // The record is written as its bytes: no padding.
    assert!(size_of::<Acct>() == 64);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comp_t_encoding() {
        // below the mantissa limit: plain 1/64 s ticks
        assert_eq!(encode_comp_t(1, 0), 64);
        assert_eq!(encode_comp_t(0, 500_000_000), 32);
        // 200 s = 12800 ticks > 8191: one base-8 exponent step, rounded
        let c = encode_comp_t(200, 0);
        assert_eq!(c >> MANTSIZE, 1);
        assert_eq!(u64::from(c & MAXFRACT as u16), 12800 >> 3);
    }
}
