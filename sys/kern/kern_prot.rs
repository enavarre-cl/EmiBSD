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
 * Copyright (c) 1982, 1986, 1989, 1990, 1991, 1993
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
 *	@(#)kern_prot.c	8.6 (Berkeley) 1/21/94
 */
/* </LICENSES> */

/* <CODE> */
//! System calls related to processes and protection: the process, group and session ids,
//! the user and group ids, the credentials (`ucred_pool`, `crget`/`crhold`/`crfree`),
//! `suser`, the login name, the thread control block and the thread names.
//!
//! Upstream: sys/kern/kern_prot.c @ 3ce1f3f79392
//!
//! Status: `ported`. Every function of the C file is here.
//!
//! ## Deviations
//! - Credentials are pool items handed around as `&'static Ucred`, as `uvmspace_alloc`/
//!   `uvmspace_free` hand out `&'static Vmspace`: `crget` and `crhold` return the reference
//!   the caller now owns, `crfree` gives one back (and the memory, with the last one). The
//!   process and thread fields are `Cell<*const Ucred>` with `ucred()` accessors.
//! - `crset` copies the members after `cr_refcnt` by name instead of `memcpy` from
//!   `offsetof(cr_startcopy)`; `crcopy`/`crdup` do `*newcr = *cr` the same way (then
//!   `refcnt_init`, as the C).
//! - `suser`, `suser_ucred` and `crfromxucred` return `Result<(), Errno>`; `groupmember` and
//!   `proc_cansugid` return `bool`. `(uid_t)-1`/`(gid_t)-1` ("leave unchanged") are
//!   `Uid::MAX`/`Gid::MAX`.
//! - `sys___set_tcb`/`sys___get_tcb` go through `machine::tcb` (`TCB_SET`, `TCB_GET`,
//!   `TCB_INVALID`); the TCB is a `usize`.

use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use libkern::strlcpy;

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_proc::{
    PGRP_POOL, SESSION_POOL, UCRED_POOL, chgproccnt, enternewpgrp, enterthispgrp, inferior, pgfind,
    prfind, tfind_user,
};
use crate::kern::kern_synch::{refcnt_init, refcnt_rele, refcnt_shared, refcnt_take};
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::panic;
use crate::machine::copy::{copyin, copyinstr, copyout, copyoutstr};
use crate::machine::tcb::{tcb_get, tcb_invalid, tcb_set};
use crate::sys::errno::Errno;
use crate::sys::pool::{PR_WAITOK, PR_ZERO};
use crate::sys::proc::{
    _MAXCOMLEN, PS_EXEC, PS_SUGID, PS_SUGIDEXEC, PS_TRACED, Proc, Process, Session,
    THREAD_PID_OFFSET, sess_leader,
};
use crate::sys::syscallargs::{
    SysGetgroupsArgs, SysGetloginRArgs, SysGetpgidArgs, SysGetresgidArgs, SysGetresuidArgs,
    SysGetsidArgs, SysGetthrnameArgs, SysSetTcbArgs, SysSetegidArgs, SysSeteuidArgs, SysSetgidArgs,
    SysSetgroupsArgs, SysSetloginArgs, SysSetpgidArgs, SysSetregidArgs, SysSetresgidArgs,
    SysSetresuidArgs, SysSetreuidArgs, SysSetthrnameArgs, SysSetuidArgs,
};
use crate::sys::syslimits::{LOGIN_NAME_MAX, NGROUPS_MAX};
use crate::sys::systm::{SysArgs, kernel_lock, kernel_unlock, sysargs};
use crate::sys::types::{Gid, Register, Uid};
use crate::sys::ucred::{Ucred, Xucred};

/// `crset`: copies everything after the reference count of `cr` into `newcr`.
#[inline]
pub fn crset(newcr: &Ucred, cr: &Ucred) {
    kassert!(cr.cr_refcnt.r_refs.load(Ordering::Relaxed) > 0);
    newcr.cr_uid.set(cr.cr_uid.get());
    newcr.cr_ruid.set(cr.cr_ruid.get());
    newcr.cr_svuid.set(cr.cr_svuid.get());
    newcr.cr_gid.set(cr.cr_gid.get());
    newcr.cr_rgid.set(cr.cr_rgid.get());
    newcr.cr_svgid.set(cr.cr_svgid.get());
    newcr.cr_ngroups.set(cr.cr_ngroups.get());
    for (to, from) in newcr.cr_groups.iter().zip(&cr.cr_groups) {
        to.set(from.get());
    }
}

/// `getpid(2)`.
pub fn sys_getpid(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    retval[0] = p.process().ps_pid.get() as Register;
    Ok(())
}

/// `getthrid(2)`.
pub fn sys_getthrid(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    retval[0] = (p.p_tid.get() + THREAD_PID_OFFSET) as Register;
    Ok(())
}

/// `getppid(2)`.
pub fn sys_getppid(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let pr = p.process();
    mtx_enter(&pr.ps_mtx);
    retval[0] = pr.ps_ppid.get() as Register;
    mtx_leave(&pr.ps_mtx);
    Ok(())
}

/// `getpgrp(2)`: get process group ID; note that POSIX getpgrp takes no parameter.
pub fn sys_getpgrp(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    retval[0] = p.process().pgid() as Register;
    Ok(())
}

/// `ps_session`: the session of `pr`, which every process has once it is in a process group.
fn session_of(pr: &Process) -> &Session {
    let s = pr.session();
    kassert!(!s.is_null());
    // SAFETY: a process's session outlives its membership in it (`Process::session`).
    unsafe { &*s }
}

/// The process `pid` names for `getpgid`/`getsid`: the caller's own for 0 or its pid,
/// otherwise one in the caller's session.
fn sameses_target(curp: &Proc, pid: i32) -> Result<&Process, Errno> {
    let curpr = curp.process();
    if pid == 0 || pid == curpr.ps_pid.get() {
        return Ok(curpr);
    }
    let targpr = prfind(pid).ok_or(Errno::ESRCH)?;
    if !ptr::eq(targpr.session(), curpr.session()) {
        return Err(Errno::EPERM);
    }
    Ok(targpr)
}

/// `getpgid(2)`: SysVR.4 compatible getpgid().
pub fn sys_getpgid(curp: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetpgidArgs = sysargs(v);

    let targpr = sameses_target(curp, uap.pid.get())?;
    retval[0] = targpr.pgid() as Register;
    Ok(())
}

/// `getsid(2)`.
pub fn sys_getsid(curp: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetsidArgs = sysargs(v);

    let targpr = sameses_target(curp, uap.pid.get())?;
    // Skip exiting processes
    // SAFETY: a session leader stays allocated while it leads (exit clears `s_leader`).
    let Some(leader) = (unsafe { session_of(targpr).s_leader.get().as_ref() }) else {
        return Err(Errno::ESRCH);
    };
    retval[0] = leader.ps_pid.get() as Register;
    Ok(())
}

/// `getuid(2)`.
pub fn sys_getuid(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    retval[0] = p.ucred().cr_ruid.get() as Register;
    Ok(())
}

/// `geteuid(2)`.
pub fn sys_geteuid(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    retval[0] = p.ucred().cr_uid.get() as Register;
    Ok(())
}

/// `issetugid(2)`.
pub fn sys_issetugid(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    retval[0] = Register::from(p.process().ps_flags.load(Ordering::Relaxed) & PS_SUGIDEXEC != 0);
    Ok(())
}

/// `getgid(2)`.
pub fn sys_getgid(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    retval[0] = p.ucred().cr_rgid.get() as Register;
    Ok(())
}

/// `getegid(2)`: get effective group ID. The "egid" is groups\[0\], and could be obtained
/// via getgroups. This syscall exists because it is somewhat painful to do correctly in a
/// library function.
pub fn sys_getegid(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    retval[0] = p.ucred().cr_gid.get() as Register;
    Ok(())
}

/// `getgroups(2)`.
pub fn sys_getgroups(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetgroupsArgs = sysargs(v);
    let uc = p.ucred();

    let ngrp = uap.gidsetsize.get();
    if ngrp == 0 {
        retval[0] = Register::from(uc.cr_ngroups.get());
        return Ok(());
    }
    if ngrp < i32::from(uc.cr_ngroups.get()) {
        return Err(Errno::EINVAL);
    }
    let ngrp = usize::try_from(uc.cr_ngroups.get()).unwrap_or(0);
    let mut groups = [[0u8; size_of::<Gid>()]; NGROUPS_MAX];
    for (bytes, gid) in groups.iter_mut().zip(&uc.cr_groups) {
        *bytes = gid.get().to_ne_bytes();
    }
    copyout(groups[..ngrp].as_flattened(), uap.gidset.get() as usize)?;
    retval[0] = ngrp as Register;
    Ok(())
}

/// `setsid(2)`.
pub fn sys_setsid(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let pr = p.process();
    let pid = pr.ps_pid.get();

    let (Some(newsess), Some(newpgrp)) = (
        pool_get(&SESSION_POOL, PR_WAITOK),
        pool_get(&PGRP_POOL, PR_WAITOK),
    ) else {
        panic(format_args!("sys_setsid: pool_get"));
    };

    if pr.pgid() == pid || pgfind(pid).is_some() {
        pool_put(&PGRP_POOL, newpgrp);
        pool_put(&SESSION_POOL, newsess);
        Err(Errno::EPERM)
    } else {
        enternewpgrp(pr, newpgrp, Some(newsess));
        retval[0] = pid as Register;
        Ok(())
    }
}

/// `setpgid(2)`: set process group (setpgid/old setpgrp).
///
/// The caller does `setpgid(targpid, targpgid)`:
/// - `pid` must be the caller or a child of the caller (`ESRCH`); if a child, it must be in
///   the same session (`EPERM`) and must not have done an exec (`EACCES`);
/// - if `pgid != pid`, there must exist some process in the same session having `pgid`
///   (`EPERM`);
/// - `pid` must not be a session leader (`EPERM`).
pub fn sys_setpgid(curp: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetpgidArgs = sysargs(v);
    let curpr = curp.process();

    let pid = uap.pid.get();
    let mut pgid = uap.pgid.get();

    if pgid < 0 {
        return Err(Errno::EINVAL);
    }

    let Some(newpgrp) = pool_get(&PGRP_POOL, PR_WAITOK) else {
        panic(format_args!("sys_setpgid: pool_get"));
    };
    let mut newpgrp = Some(newpgrp);

    let result = 'out: {
        let targpr: &Process = if pid != 0 && pid != curpr.ps_pid.get() {
            let Some(targpr) = prfind(pid).filter(|t| inferior(t, curpr)) else {
                break 'out Err(Errno::ESRCH);
            };
            if !ptr::eq(targpr.session(), curpr.session()) {
                break 'out Err(Errno::EPERM);
            }
            if targpr.ps_flags.load(Ordering::Relaxed) & PS_EXEC != 0 {
                break 'out Err(Errno::EACCES);
            }
            targpr
        } else {
            curpr
        };
        if sess_leader(targpr) {
            break 'out Err(Errno::EPERM);
        }
        if pgid == 0 {
            pgid = targpr.ps_pid.get();
        }

        match pgfind(pgid) {
            None => {
                // can only create a new process group with pgid == pid
                if pgid != targpr.ps_pid.get() {
                    Err(Errno::EPERM)
                } else if let Some(pg) = newpgrp.take() {
                    enternewpgrp(targpr, pg, None);
                    Ok(())
                } else {
                    Ok(())
                }
            }
            // anything to do?
            Some(pgrp) if !ptr::eq(pgrp, targpr.ps_pgrp.get()) => {
                if pgid != targpr.ps_pid.get() && !ptr::eq(pgrp.pg_session.get(), curpr.session()) {
                    Err(Errno::EPERM)
                } else {
                    enterthispgrp(targpr, pgrp);
                    Ok(())
                }
            }
            Some(_) => Ok(()),
        }
    };

    // out:
    if let Some(newpgrp) = newpgrp {
        pool_put(&PGRP_POOL, newpgrp);
    }
    result
}

/// `copyout` of one id to a user pointer, skipped when the pointer is NULL.
fn copyout_id(id: u32, uaddr: *mut u32) -> Result<(), Errno> {
    if uaddr.is_null() {
        return Ok(());
    }
    copyout(&id.to_ne_bytes(), uaddr as usize)
}

/// `getresuid(2)`.
pub fn sys_getresuid(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetresuidArgs = sysargs(v);
    let uc = p.ucred();

    let error1 = copyout_id(uc.cr_ruid.get(), uap.ruid.get());
    let error2 = copyout_id(uc.cr_uid.get(), uap.euid.get());
    let error3 = copyout_id(uc.cr_svuid.get(), uap.suid.get());

    error1.and(error2).and(error3)
}

/// `setresuid(2)`.
pub fn sys_setresuid(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetresuidArgs = sysargs(v);
    let pr = p.process();
    let uc = p.ucred();

    let ruid = uap.ruid.get();
    let euid = uap.euid.get();
    let suid = uap.suid.get();

    // make permission checks against the thread's ucred, but the actual changes will be to
    // the process's ucred
    let pruc = pr.ucred();
    if (ruid == Uid::MAX || ruid == pruc.cr_ruid.get())
        && (euid == Uid::MAX || euid == pruc.cr_uid.get())
        && (suid == Uid::MAX || suid == pruc.cr_svuid.get())
    {
        return Ok(()); // no change
    }

    // Any of the real, effective, and saved uids may be changed to the current value of one
    // of the three (root is not limited).
    let held = |id: Uid| id == uc.cr_ruid.get() || id == uc.cr_uid.get() || id == uc.cr_svuid.get();
    if ruid != Uid::MAX && !held(ruid) {
        suser(p)?;
    }
    if euid != Uid::MAX && !held(euid) {
        suser(p)?;
    }
    if suid != Uid::MAX && !held(suid) {
        suser(p)?;
    }

    // Copy credentials so other references do not see our changes. ps_ucred may change
    // during the crget().
    let newcred = crget();
    let pruc = pr.ucred();
    crset(newcred, pruc);

    // Note that unlike the other set*uid() calls, each uid type is set independently of the
    // others.
    if ruid != Uid::MAX {
        newcred.cr_ruid.set(ruid);
    }
    if euid != Uid::MAX {
        newcred.cr_uid.set(euid);
    }
    if suid != Uid::MAX {
        newcred.cr_svuid.set(suid);
    }
    pr.ps_ucred.set(newcred);
    pr.ps_flags.fetch_or(PS_SUGID, Ordering::Relaxed);

    // now that we can sleep, transfer proc count to new user
    if ruid != Uid::MAX && ruid != pruc.cr_ruid.get() {
        chgproccnt(pruc.cr_ruid.get(), -1);
        chgproccnt(ruid, 1);
    }
    crfree(pruc);

    Ok(())
}

/// `getresgid(2)`.
pub fn sys_getresgid(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetresgidArgs = sysargs(v);
    let uc = p.ucred();

    let error1 = copyout_id(uc.cr_rgid.get(), uap.rgid.get());
    let error2 = copyout_id(uc.cr_gid.get(), uap.egid.get());
    let error3 = copyout_id(uc.cr_svgid.get(), uap.sgid.get());

    error1.and(error2).and(error3)
}

/// `setresgid(2)`.
pub fn sys_setresgid(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetresgidArgs = sysargs(v);
    let pr = p.process();
    let uc = p.ucred();

    let rgid = uap.rgid.get();
    let egid = uap.egid.get();
    let sgid = uap.sgid.get();

    // make permission checks against the thread's ucred, but the actual changes will be to
    // the process's ucred
    let pruc = pr.ucred();
    if (rgid == Gid::MAX || rgid == pruc.cr_rgid.get())
        && (egid == Gid::MAX || egid == pruc.cr_gid.get())
        && (sgid == Gid::MAX || sgid == pruc.cr_svgid.get())
    {
        return Ok(()); // no change
    }

    // Any of the real, effective, and saved gids may be changed to the current value of one
    // of the three (root is not limited).
    let held = |id: Gid| id == uc.cr_rgid.get() || id == uc.cr_gid.get() || id == uc.cr_svgid.get();
    if rgid != Gid::MAX && !held(rgid) {
        suser(p)?;
    }
    if egid != Gid::MAX && !held(egid) {
        suser(p)?;
    }
    if sgid != Gid::MAX && !held(sgid) {
        suser(p)?;
    }

    // Copy credentials so other references do not see our changes. ps_ucred may change
    // during the crget().
    let newcred = crget();
    let pruc = pr.ucred();
    crset(newcred, pruc);

    // Note that unlike the other set*gid() calls, each gid type is set independently of the
    // others.
    if rgid != Gid::MAX {
        newcred.cr_rgid.set(rgid);
    }
    if egid != Gid::MAX {
        newcred.cr_gid.set(egid);
    }
    if sgid != Gid::MAX {
        newcred.cr_svgid.set(sgid);
    }
    pr.ps_ucred.set(newcred);
    pr.ps_flags.fetch_or(PS_SUGID, Ordering::Relaxed);
    crfree(pruc);
    Ok(())
}

/// `setregid(2)`.
pub fn sys_setregid(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetregidArgs = sysargs(v);
    let pr = p.process();
    let uc = p.ucred();

    let rgid = uap.rgid.get();
    let egid = uap.egid.get();

    // make permission checks against the thread's ucred, but the actual changes will be to
    // the process's ucred
    //
    // The saved gid check here is complicated: we reset the saved gid to the real gid if the
    // real gid is specified *and* either it's changing _or_ the saved gid won't equal the
    // effective gid. So, the svgid *won't* change when the rgid isn't specified or when the
    // rgid isn't changing and the svgid equals the requested egid.
    let pruc = pr.ucred();
    let new_egid = |cr: &Ucred| {
        if egid != Gid::MAX {
            egid
        } else {
            cr.cr_gid.get()
        }
    };
    if (rgid == Gid::MAX || rgid == pruc.cr_rgid.get())
        && (egid == Gid::MAX || egid == pruc.cr_gid.get())
        && (rgid == Gid::MAX
            || (rgid == pruc.cr_rgid.get() && pruc.cr_svgid.get() == new_egid(pruc)))
    {
        return Ok(()); // no change
    }

    // Any of the real, effective, and saved gids may be changed to the current value of one
    // of the three (root is not limited).
    let held = |id: Gid| id == uc.cr_rgid.get() || id == uc.cr_gid.get() || id == uc.cr_svgid.get();
    if rgid != Gid::MAX && !held(rgid) {
        suser(p)?;
    }
    if egid != Gid::MAX && !held(egid) {
        suser(p)?;
    }

    // Copy credentials so other references do not see our changes. ps_ucred may change
    // during the crget().
    let newcred = crget();
    let pruc = pr.ucred();
    crset(newcred, pruc);

    if rgid != Gid::MAX {
        newcred.cr_rgid.set(rgid);
    }
    if egid != Gid::MAX {
        newcred.cr_gid.set(egid);
    }

    // The saved gid presents a bit of a dilemma, as it did not exist when setregid(2) was
    // conceived. We only set the saved gid when the real gid is specified and either its
    // value would change, or where the saved and effective gids are different.
    if rgid != Gid::MAX && (rgid != pruc.cr_rgid.get() || pruc.cr_svgid.get() != new_egid(pruc)) {
        newcred.cr_svgid.set(rgid);
    }
    pr.ps_ucred.set(newcred);
    pr.ps_flags.fetch_or(PS_SUGID, Ordering::Relaxed);
    crfree(pruc);
    Ok(())
}

/// `setreuid(2)`.
pub fn sys_setreuid(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetreuidArgs = sysargs(v);
    let pr = p.process();
    let uc = p.ucred();

    let ruid = uap.ruid.get();
    let euid = uap.euid.get();

    // make permission checks against the thread's ucred, but the actual changes will be to
    // the process's ucred
    //
    // The saved uid check here is complicated: we reset the saved uid to the real uid if the
    // real uid is specified *and* either it's changing _or_ the saved uid won't equal the
    // effective uid. So, the svuid *won't* change when the ruid isn't specified or when the
    // ruid isn't changing and the svuid equals the requested euid.
    let pruc = pr.ucred();
    let new_euid = |cr: &Ucred| {
        if euid != Uid::MAX {
            euid
        } else {
            cr.cr_uid.get()
        }
    };
    if (ruid == Uid::MAX || ruid == pruc.cr_ruid.get())
        && (euid == Uid::MAX || euid == pruc.cr_uid.get())
        && (ruid == Uid::MAX
            || (ruid == pruc.cr_ruid.get() && pruc.cr_svuid.get() == new_euid(pruc)))
    {
        return Ok(()); // no change
    }

    // Any of the real, effective, and saved uids may be changed to the current value of one
    // of the three (root is not limited).
    let held = |id: Uid| id == uc.cr_ruid.get() || id == uc.cr_uid.get() || id == uc.cr_svuid.get();
    if ruid != Uid::MAX && !held(ruid) {
        suser(p)?;
    }
    if euid != Uid::MAX && !held(euid) {
        suser(p)?;
    }

    // Copy credentials so other references do not see our changes. ps_ucred may change
    // during the crget().
    let newcred = crget();
    let pruc = pr.ucred();
    crset(newcred, pruc);

    if ruid != Uid::MAX {
        newcred.cr_ruid.set(ruid);
    }
    if euid != Uid::MAX {
        newcred.cr_uid.set(euid);
    }

    // The saved uid presents a bit of a dilemma, as it did not exist when setreuid(2) was
    // conceived. We only set the saved uid when the real uid is specified and either its
    // value would change, or where the saved and effective uids are different.
    if ruid != Uid::MAX && (ruid != pruc.cr_ruid.get() || pruc.cr_svuid.get() != new_euid(pruc)) {
        newcred.cr_svuid.set(ruid);
    }
    pr.ps_ucred.set(newcred);
    pr.ps_flags.fetch_or(PS_SUGID, Ordering::Relaxed);

    // now that we can sleep, transfer proc count to new user
    if ruid != Uid::MAX && ruid != pruc.cr_ruid.get() {
        chgproccnt(pruc.cr_ruid.get(), -1);
        chgproccnt(ruid, 1);
    }
    crfree(pruc);

    Ok(())
}

/// `setuid(2)`.
pub fn sys_setuid(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetuidArgs = sysargs(v);
    let pr = p.process();
    let uc = p.ucred();

    let uid = uap.uid.get();

    let pruc = pr.ucred();
    if pruc.cr_uid.get() == uid && pruc.cr_ruid.get() == uid && pruc.cr_svuid.get() == uid {
        return Ok(());
    }

    if uid != uc.cr_ruid.get() && uid != uc.cr_svuid.get() && uid != uc.cr_uid.get() {
        suser(p)?;
    }

    // Copy credentials so other references do not see our changes. ps_ucred may change
    // during the crget().
    let newcred = crget();
    let pruc = pr.ucred();
    crset(newcred, pruc);

    // Everything's okay, do it.
    let did_real = uid == pruc.cr_uid.get() || suser(p).is_ok();
    if did_real {
        newcred.cr_ruid.set(uid);
        newcred.cr_svuid.set(uid);
    }
    newcred.cr_uid.set(uid);
    pr.ps_ucred.set(newcred);
    pr.ps_flags.fetch_or(PS_SUGID, Ordering::Relaxed);

    // Transfer proc count to new user.
    if did_real && uid != pruc.cr_ruid.get() {
        chgproccnt(pruc.cr_ruid.get(), -1);
        chgproccnt(uid, 1);
    }
    crfree(pruc);

    Ok(())
}

/// `seteuid(2)`.
pub fn sys_seteuid(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSeteuidArgs = sysargs(v);
    let pr = p.process();
    let uc = p.ucred();

    let euid = uap.euid.get();

    if pr.ucred().cr_uid.get() == euid {
        return Ok(());
    }

    if euid != uc.cr_ruid.get() && euid != uc.cr_svuid.get() {
        suser(p)?;
    }

    // Copy credentials so other references do not see our changes. ps_ucred may change
    // during the crget().
    let newcred = crget();
    let pruc = pr.ucred();
    crset(newcred, pruc);
    newcred.cr_uid.set(euid);
    pr.ps_ucred.set(newcred);
    pr.ps_flags.fetch_or(PS_SUGID, Ordering::Relaxed);
    crfree(pruc);
    Ok(())
}

/// `setgid(2)`.
pub fn sys_setgid(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetgidArgs = sysargs(v);
    let pr = p.process();
    let uc = p.ucred();

    let gid = uap.gid.get();

    let pruc = pr.ucred();
    if pruc.cr_gid.get() == gid && pruc.cr_rgid.get() == gid && pruc.cr_svgid.get() == gid {
        return Ok(());
    }

    if gid != uc.cr_rgid.get() && gid != uc.cr_svgid.get() && gid != uc.cr_gid.get() {
        suser(p)?;
    }

    // Copy credentials so other references do not see our changes. ps_ucred may change
    // during the crget().
    let newcred = crget();
    let pruc = pr.ucred();
    crset(newcred, pruc);

    if gid == pruc.cr_gid.get() || suser(p).is_ok() {
        newcred.cr_rgid.set(gid);
        newcred.cr_svgid.set(gid);
    }
    newcred.cr_gid.set(gid);
    pr.ps_ucred.set(newcred);
    pr.ps_flags.fetch_or(PS_SUGID, Ordering::Relaxed);
    crfree(pruc);
    Ok(())
}

/// `setegid(2)`.
pub fn sys_setegid(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetegidArgs = sysargs(v);
    let pr = p.process();
    let uc = p.ucred();

    let egid = uap.egid.get();

    if pr.ucred().cr_gid.get() == egid {
        return Ok(());
    }

    if egid != uc.cr_rgid.get() && egid != uc.cr_svgid.get() {
        suser(p)?;
    }

    // Copy credentials so other references do not see our changes. ps_ucred may change
    // during the crget().
    let newcred = crget();
    let pruc = pr.ucred();
    crset(newcred, pruc);
    newcred.cr_gid.set(egid);
    pr.ps_ucred.set(newcred);
    pr.ps_flags.fetch_or(PS_SUGID, Ordering::Relaxed);
    crfree(pruc);
    Ok(())
}

/// `setgroups(2)`.
pub fn sys_setgroups(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetgroupsArgs = sysargs(v);
    let pr = p.process();

    suser(p)?;
    let ngrp = uap.gidsetsize.get();
    let Ok(ngrp) = usize::try_from(ngrp) else {
        return Err(Errno::EINVAL);
    };
    if ngrp > NGROUPS_MAX {
        return Err(Errno::EINVAL);
    }
    let mut groups = [[0u8; size_of::<Gid>()]; NGROUPS_MAX];
    copyin(uap.gidset.get() as usize, groups[..ngrp].as_flattened_mut())?;

    let newcred = crget();
    let pruc = pr.ucred();
    crset(newcred, pruc);
    for (gid, &bytes) in newcred.cr_groups.iter().zip(&groups[..ngrp]) {
        gid.set(Gid::from_ne_bytes(bytes));
    }
    newcred.cr_ngroups.set(ngrp as i16);
    pr.ps_ucred.set(newcred);
    pr.ps_flags.fetch_or(PS_SUGID, Ordering::Relaxed);
    crfree(pruc);
    Ok(())
}

/// `groupmember`: check if `gid` is a member of the group set.
pub fn groupmember(gid: Gid, cred: &Ucred) -> bool {
    if cred.cr_gid.get() == gid {
        return true;
    }
    let ngroups = usize::try_from(cred.cr_ngroups.get()).unwrap_or(0);
    cred.cr_groups[..ngroups].iter().any(|g| g.get() == gid)
}

/// `suser`: test whether this process has special user powers.
pub fn suser(p: &Proc) -> Result<(), Errno> {
    let cred = p.ucred();

    if cred.cr_uid.get() == 0 {
        return Ok(());
    }
    Err(Errno::EPERM)
}

/// `suser_ucred`: replacement for old suser, for callers who don't have a process.
pub fn suser_ucred(cred: &Ucred) -> Result<(), Errno> {
    if cred.cr_uid.get() == 0 {
        return Ok(());
    }
    Err(Errno::EPERM)
}

/// `crget`: allocate a zeroed cred structure, with one reference (the caller's).
pub fn crget() -> &'static Ucred {
    let Some(mem) = pool_get(&UCRED_POOL, PR_WAITOK | PR_ZERO) else {
        panic(format_args!("crget: ucred_pool is empty"));
    };
    let cr = mem.cast::<Ucred>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Ucred>()` bytes, written
    // once before anything else sees it.
    unsafe { cr.as_ptr().write(Ucred::new()) };
    // SAFETY: as above; the item stays allocated until the last `crfree`.
    let cr: &'static Ucred = unsafe { cr.as_ref() };
    refcnt_init(&cr.cr_refcnt);
    cr
}

/// `crhold`: increment the reference count of a cred structure. Returns the passed
/// structure.
pub fn crhold(cr: &'static Ucred) -> &'static Ucred {
    refcnt_take(&cr.cr_refcnt);
    cr
}

/// `crfree`: free a cred structure. Throws away space when ref count gets to 0. The caller
/// gives up its reference and must not use `cr` afterwards.
pub fn crfree(cr: &'static Ucred) {
    if refcnt_rele(&cr.cr_refcnt) {
        pool_put(&UCRED_POOL, NonNull::from(cr).cast::<u8>());
    }
}

/// `crcopy`: copy cred structure to a new one and free the old one (unless the caller holds
/// the only reference, in which case `cr` itself is returned).
pub fn crcopy(cr: &'static Ucred) -> &'static Ucred {
    if !refcnt_shared(&cr.cr_refcnt) {
        return cr;
    }
    let newcr = crget();
    crset(newcr, cr);
    crfree(cr);
    refcnt_init(&newcr.cr_refcnt);
    newcr
}

/// `crdup`: dup cred struct to a new held one.
pub fn crdup(cr: &Ucred) -> &'static Ucred {
    let newcr = crget();
    crset(newcr, cr);
    refcnt_init(&newcr.cr_refcnt);
    newcr
}

/// `crfromxucred`: convert the userspace xucred to a kernel ucred.
pub fn crfromxucred(cr: &Ucred, xcr: &Xucred) -> Result<(), Errno> {
    let Ok(ngroups) = usize::try_from(xcr.cr_ngroups) else {
        return Err(Errno::EINVAL);
    };
    if ngroups > NGROUPS_MAX {
        return Err(Errno::EINVAL);
    }
    refcnt_init(&cr.cr_refcnt);
    cr.cr_uid.set(xcr.cr_uid);
    cr.cr_gid.set(xcr.cr_gid);
    cr.cr_ngroups.set(xcr.cr_ngroups);
    for (to, &from) in cr.cr_groups.iter().zip(&xcr.cr_groups[..ngroups]) {
        to.set(from);
    }
    Ok(())
}

/// `getlogin_r(2)`: get login name, if available. The error is the return value, as in C.
pub fn sys_getlogin_r(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetloginRArgs = sysargs(v);
    let s = session_of(p.process());
    let mut buf = [0u8; LOGIN_NAME_MAX];

    let namelen = uap.namelen.get().min(LOGIN_NAME_MAX);
    // SAFETY: `s_login` is written only by `setlogin` under the kernel lock; this is a read.
    strlcpy(&mut buf, unsafe { &*s.s_login.get() });
    let error = match copyoutstr(&buf[..namelen], uap.namebuf.get() as usize) {
        Ok(_) => 0,
        Err(Errno::ENAMETOOLONG) => Errno::ERANGE as i32,
        Err(e) => e as i32,
    };
    retval[0] = error as Register;
    Ok(())
}

/// `setlogin(2)`: set login name.
pub fn sys_setlogin(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetloginArgs = sysargs(v);
    let mut buf = [0u8; LOGIN_NAME_MAX];

    suser(p)?;
    match copyinstr(uap.namebuf.get() as usize, &mut buf) {
        Ok(_) => {
            let s = session_of(p.process());
            // SAFETY: as in `sys_getlogin_r`: the one writer, under the kernel lock.
            strlcpy(unsafe { &mut *s.s_login.get() }, &buf);
            Ok(())
        }
        Err(Errno::ENAMETOOLONG) => Err(Errno::EINVAL),
        Err(e) => Err(e),
    }
}

/// `proc_cansugid`: check if a process is allowed to raise its privileges.
pub fn proc_cansugid(p: &Proc) -> bool {
    // ptrace(2)d processes shouldn't.
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_TRACED != 0 {
        return false;
    }

    // processes with shared filedescriptors shouldn't.
    if p.fd().fd_refcnt.get() > 1 {
        return false;
    }

    // Allow.
    true
}

/// `__set_tcb(2)`: set address of the proc's thread-control-block.
#[allow(non_snake_case)] // the C name, verbatim: the syscall table finds it by name
pub fn sys___set_tcb(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetTcbArgs = sysargs(v);
    let tcb = uap.tcb.get() as usize;

    if tcb_invalid(tcb) {
        return Err(Errno::EINVAL);
    }
    tcb_set(p, tcb);
    Ok(())
}

/// `__get_tcb(2)`: get address of the proc's thread-control-block.
#[allow(non_snake_case)] // the C name, verbatim: the syscall table finds it by name
pub fn sys___get_tcb(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    retval[0] = tcb_get(p) as Register;
    Ok(())
}

/// `getthrname(2)`. The error is the return value, as in C.
pub fn sys_getthrname(curp: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetthrnameArgs = sysargs(v);
    let mut buf = [0u8; _MAXCOMLEN];
    let tid = uap.tid.get();

    let p = if tid != 0 {
        tfind_user(tid, curp.process())
    } else {
        Some(curp)
    };
    let Some(p) = p else {
        return Err(Errno::ESRCH);
    };
    strlcpy(&mut buf, p.name());

    let len = uap.len.get().min(buf.len());
    let error = match copyoutstr(&buf[..len], uap.name.get() as usize) {
        Ok(_) => 0,
        Err(Errno::ENAMETOOLONG) => Errno::ERANGE as i32,
        Err(e) => e as i32,
    };
    retval[0] = error as Register;
    Ok(())
}

/// `setthrname(2)`. The error is the return value, as in C; a name too long is truncated.
pub fn sys_setthrname(curp: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetthrnameArgs = sysargs(v);
    let mut buf = [0u8; _MAXCOMLEN];
    let tid = uap.tid.get();

    let error = match copyinstr(uap.name.get() as usize, &mut buf) {
        Ok(_) => 0,
        Err(Errno::ENAMETOOLONG) => {
            buf[_MAXCOMLEN - 1] = 0;
            0
        }
        Err(e) => e as i32,
    };
    if error == 0 {
        let p = if tid != 0 {
            tfind_user(tid, curp.process())
        } else {
            Some(curp)
        };
        let Some(p) = p else {
            return Err(Errno::ESRCH);
        };
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        p.set_name(&buf[..n]);
    }
    retval[0] = error as Register;
    Ok(())
}

/// `dorefreshcreds`: refresh the thread's reference to the process's credentials.
pub fn dorefreshcreds(pr: &Process, p: &Proc) {
    let uc = p.ucred();

    kernel_lock(); // KERNEL_LOCK() (XXX should be PROCESS_RLOCK(pr))
    if !ptr::eq(uc, pr.ps_ucred.get()) {
        let cr = crhold(pr.ucred());
        p.p_ucred.set(cr);
        crfree(uc);
    }
    kernel_unlock(); // KERNEL_UNLOCK()
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the credentials and the id system calls: the reference counting of
    // `crget`/`crhold`/`crfree`/`crcopy`/`crdup`, `groupmember`, `suser`, and the permission
    // rules of the set*id calls against threads built by hand.

    use std::boxed::Box;
    use std::sync::MutexGuard;
    use std::{assert, assert_eq, assert_ne};

    use super::*;
    use crate::kern::kern_proc::procinit;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::sys::proc::refreshcreds;
    use crate::sys::user::User;

    /// Real memory and the process pools (`ucred_pool` among them) and hashes.
    fn setup() -> MutexGuard<'static, ()> {
        let guard = setup_real_memory();
        crate::machine::cons::consinit();
        procinit();
        guard
    }

    /// Credentials with one reference whose real, effective and saved ids are `uid`/`gid`.
    fn cred(uid: Uid, gid: Gid) -> &'static Ucred {
        let cr = crget();
        cr.cr_uid.set(uid);
        cr.cr_ruid.set(uid);
        cr.cr_svuid.set(uid);
        cr.cr_gid.set(gid);
        cr.cr_rgid.set(gid);
        cr.cr_svgid.set(gid);
        cr
    }

    /// A thread of a fresh process, both holding `cr` (the thread `cr`'s reference, the process
    /// one more), with a u-area for the TCB, charged to its real uid as `fork1` does.
    fn thread(cr: &'static Ucred) -> &'static Proc {
        let pr: &'static Process = Box::leak(Box::new(Process::new()));
        let p: &'static Proc = Box::leak(Box::new(Proc::new()));
        p.p_p.set(pr);
        p.p_addr.set(Box::leak(Box::new(User::new())));
        pr.ps_mainproc.set(p);
        p.p_ucred.set(cr);
        pr.ps_ucred.set(crhold(cr));
        chgproccnt(cr.cr_ruid.get(), 1);
        p
    }

    /// The argument registers of a system call.
    fn args(a: &[Register]) -> SysArgs {
        let mut v: SysArgs = [0; 6];
        v[..a.len()].copy_from_slice(a);
        v
    }

    /// `(uid_t)-1` in an argument register.
    const NONE: Register = Uid::MAX as Register;

    /// The real, effective and saved uids of `cr`.
    fn uids(cr: &Ucred) -> (Uid, Uid, Uid) {
        (cr.cr_ruid.get(), cr.cr_uid.get(), cr.cr_svuid.get())
    }

    #[test]
    fn crget_crhold_crfree_count_references() {
        let _g = setup();
        let nout = UCRED_POOL.pr_nout.get();

        let cr = crget();
        assert_eq!(cr.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
        assert_eq!(uids(cr), (0, 0, 0));
        assert_eq!(cr.cr_ngroups.get(), 0);
        assert_eq!(UCRED_POOL.pr_nout.get(), nout + 1);

        assert!(ptr::eq(crhold(cr), cr));
        assert_eq!(cr.cr_refcnt.r_refs.load(Ordering::Relaxed), 2);
        crfree(cr);
        assert_eq!(cr.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
        assert_eq!(UCRED_POOL.pr_nout.get(), nout + 1);
        crfree(cr);
        assert_eq!(UCRED_POOL.pr_nout.get(), nout);
    }

    #[test]
    fn crcopy_copies_only_shared_credentials_and_crdup_always() {
        let _g = setup();

        let cr = cred(1000, 100);
        cr.cr_ngroups.set(2);
        cr.cr_groups[0].set(100);
        cr.cr_groups[1].set(20);
        assert!(ptr::eq(crcopy(cr), cr)); // the only reference: nothing to copy

        crhold(cr);
        let copy = crcopy(cr);
        assert!(!ptr::eq(copy, cr));
        assert_eq!(cr.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
        assert_eq!(copy.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
        assert_eq!(uids(copy), (1000, 1000, 1000));
        assert_eq!(copy.cr_svgid.get(), 100);
        assert_eq!(copy.cr_ngroups.get(), 2);
        assert_eq!(copy.cr_groups[1].get(), 20);

        let dup = crdup(cr);
        assert!(!ptr::eq(dup, cr));
        assert_eq!(cr.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
        assert_eq!(dup.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
        assert_eq!(uids(dup), (1000, 1000, 1000));
        for c in [cr, copy, dup] {
            crfree(c);
        }
    }

    #[test]
    fn groupmember_suser_and_crfromxucred() {
        let _g = setup();

        let cr = cred(1000, 100);
        cr.cr_ngroups.set(2);
        cr.cr_groups[0].set(100);
        cr.cr_groups[1].set(20);
        cr.cr_groups[2].set(30); // beyond cr_ngroups
        assert!(groupmember(100, cr));
        assert!(groupmember(20, cr));
        assert!(!groupmember(30, cr));
        assert_eq!(suser_ucred(cr), Err(Errno::EPERM));
        cr.cr_uid.set(0);
        assert_eq!(suser_ucred(cr), Ok(()));

        let mut x = Xucred {
            cr_uid: 5,
            cr_gid: 6,
            cr_ngroups: 1,
            cr_groups: [7; NGROUPS_MAX],
        };
        let k = Ucred::new();
        assert_eq!(crfromxucred(&k, &x), Ok(()));
        assert_eq!(
            (k.cr_uid.get(), k.cr_gid.get(), k.cr_ngroups.get()),
            (5, 6, 1)
        );
        assert_eq!((k.cr_groups[0].get(), k.cr_groups[1].get()), (7, 0));
        x.cr_ngroups = NGROUPS_MAX as i16 + 1;
        assert_eq!(crfromxucred(&k, &x), Err(Errno::EINVAL));
        x.cr_ngroups = -1;
        assert_eq!(crfromxucred(&k, &x), Err(Errno::EINVAL));
        crfree(cr);
    }

    #[test]
    fn setresuid_allows_the_current_ids_and_root_anything() {
        let _g = setup();
        let mut rv: [Register; 2] = [0; 2];

        // A user whose saved uid is still root may become root again, nothing else.
        let cr = cred(1000, 100);
        cr.cr_svuid.set(0);
        let p = thread(cr);
        let pr = p.process();
        assert_eq!(
            sys_setresuid(p, &args(&[NONE, NONE, NONE]), &mut rv),
            Ok(())
        );
        assert_eq!(pr.ps_flags.load(Ordering::Relaxed) & PS_SUGID, 0);
        assert_eq!(
            sys_setresuid(p, &args(&[NONE, 2000, NONE]), &mut rv),
            Err(Errno::EPERM)
        );
        assert!(ptr::eq(pr.ucred(), cr));
        assert_eq!(sys_setresuid(p, &args(&[NONE, 0, NONE]), &mut rv), Ok(()));

        // The process has new credentials; the thread keeps its own until it refreshes them.
        assert!(!ptr::eq(pr.ucred(), cr));
        assert_eq!(uids(pr.ucred()), (1000, 0, 0));
        assert_ne!(pr.ps_flags.load(Ordering::Relaxed) & PS_SUGID, 0);
        assert_eq!(uids(p.ucred()), (1000, 1000, 0));
        assert_eq!(cr.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
        refreshcreds(p);
        assert!(ptr::eq(p.ucred(), pr.ucred()));
        assert_eq!(p.ucred().cr_refcnt.r_refs.load(Ordering::Relaxed), 2);

        // Now root: any uid at all, and the process count moves to the new real uid.
        assert_eq!(
            sys_setresuid(p, &args(&[3000, 3000, 3000]), &mut rv),
            Ok(())
        );
        refreshcreds(p);
        assert_eq!(uids(p.ucred()), (3000, 3000, 3000));
        assert_eq!(
            sys_setresuid(p, &args(&[0, NONE, NONE]), &mut rv),
            Err(Errno::EPERM)
        );
    }

    #[test]
    fn setreuid_resets_the_saved_uid_with_the_real_one() {
        let _g = setup();
        let mut rv: [Register; 2] = [0; 2];

        let p = thread(cred(0, 0));
        let pr = p.process();
        // Only the effective uid: the saved uid stays.
        assert_eq!(sys_setreuid(p, &args(&[NONE, 1000]), &mut rv), Ok(()));
        assert_eq!(uids(pr.ucred()), (0, 1000, 0));
        refreshcreds(p);
        // The real uid changes: the saved one follows it.
        assert_eq!(sys_setreuid(p, &args(&[1000, NONE]), &mut rv), Ok(()));
        assert_eq!(uids(pr.ucred()), (1000, 1000, 1000));
        refreshcreds(p);
        assert_eq!(
            sys_setreuid(p, &args(&[0, NONE]), &mut rv),
            Err(Errno::EPERM)
        );
    }

    #[test]
    fn setuid_and_seteuid_of_a_user_with_a_root_saved_uid() {
        let _g = setup();
        let mut rv: [Register; 2] = [0; 2];

        let cr = cred(1000, 100);
        cr.cr_svuid.set(0);
        let p = thread(cr);
        let pr = p.process();
        // setuid(0) is allowed (0 is the saved uid) but, not being root, only the effective
        // uid changes.
        assert_eq!(sys_setuid(p, &args(&[0]), &mut rv), Ok(()));
        assert_eq!(uids(pr.ucred()), (1000, 0, 0));
        assert_eq!(sys_seteuid(p, &args(&[2000]), &mut rv), Err(Errno::EPERM));
        assert_eq!(sys_seteuid(p, &args(&[1000]), &mut rv), Ok(()));
        assert_eq!(uids(pr.ucred()), (1000, 1000, 0));
        refreshcreds(p);
        // setgid to a group the thread does not have needs root.
        assert_eq!(sys_setgid(p, &args(&[200]), &mut rv), Err(Errno::EPERM));
        assert_eq!(sys_setegid(p, &args(&[100]), &mut rv), Ok(()));
    }

    #[test]
    fn the_get_calls_copy_out_and_the_groups_round_trip() {
        let _g = setup();
        let mut rv: [Register; 2] = [0; 2];

        let cr = cred(1000, 100);
        cr.cr_svuid.set(7);
        let p = thread(cred(0, 0));
        let (mut r, e, mut s): (Uid, Uid, Uid) = (9, 9, 9);
        let ptrs = |a: &mut Uid| a as *mut Uid as Register;
        // A root thread installs cr's groups, then reads them back.
        let groups: [Gid; 3] = [100, 20, 30];
        assert_eq!(
            sys_setgroups(p, &args(&[3, groups.as_ptr() as Register]), &mut rv),
            Ok(())
        );
        refreshcreds(p);
        assert_eq!(sys_getgroups(p, &args(&[0, 0]), &mut rv), Ok(()));
        assert_eq!(rv[0], 3);
        let mut back: [Gid; 4] = [0; 4];
        assert_eq!(
            sys_getgroups(p, &args(&[2, back.as_mut_ptr() as Register]), &mut rv),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            sys_getgroups(p, &args(&[4, back.as_mut_ptr() as Register]), &mut rv),
            Ok(())
        );
        assert_eq!((rv[0], back), (3, [100, 20, 30, 0]));

        let q = thread(cr);
        assert_eq!(
            sys_getresuid(q, &args(&[ptrs(&mut r), 0, ptrs(&mut s)]), &mut rv),
            Ok(())
        );
        assert_eq!((r, e, s), (1000, 9, 7));
        assert_eq!(
            sys_setgroups(q, &args(&[1, groups.as_ptr() as Register]), &mut rv),
            Err(Errno::EPERM)
        );
    }

    #[test]
    fn the_tcb_and_the_thread_name_round_trip() {
        let _g = setup();
        let mut rv: [Register; 2] = [0; 2];

        let p = thread(cred(0, 0));
        assert_eq!(sys___set_tcb(p, &args(&[0x1234_5000]), &mut rv), Ok(()));
        assert_eq!(sys___get_tcb(p, &args(&[]), &mut rv), Ok(()));
        assert_eq!(rv[0], 0x1234_5000);

        let name = b"worker\0";
        assert_eq!(
            sys_setthrname(p, &args(&[0, name.as_ptr() as Register]), &mut rv),
            Ok(())
        );
        assert_eq!((rv[0], p.name()), (0, &b"worker"[..]));
        let mut out = [0xffu8; 4];
        assert_eq!(
            sys_getthrname(p, &args(&[0, out.as_mut_ptr() as Register, 4]), &mut rv),
            Ok(())
        );
        assert_eq!(rv[0], Errno::ERANGE as i32 as Register);
        let mut out = [0xffu8; _MAXCOMLEN];
        assert_eq!(
            sys_getthrname(
                p,
                &args(&[0, out.as_mut_ptr() as Register, _MAXCOMLEN as Register]),
                &mut rv
            ),
            Ok(())
        );
        assert_eq!((rv[0], &out[..7]), (0, &name[..]));
        assert_eq!(
            sys_getthrname(p, &args(&[12345, out.as_mut_ptr() as Register, 4]), &mut rv),
            Err(Errno::ESRCH)
        );
    }
}
/* </TESTS> */
