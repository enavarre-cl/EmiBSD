/*	$OpenBSD: kern_time.c,v 1.171 2026/05/05 12:28:59 kettenis Exp $	*/
/*	$NetBSD: kern_time.c,v 1.20 1996/02/18 11:57:06 fvdl Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)kern_time.c	8.4 (Berkeley) 5/26/95
 */
/* </LICENSES> */

//! Time-related system calls and the interval timers: `kern/kern_time.c`.
//!
//! Upstream: sys/kern/kern_time.c @ 3ce1f3f79392
//!
//! Status: `ported`. Time of day and interval timer support: `settime`, `clock_gettime`,
//! `sys_clock_gettime`, `sys_clock_settime`, `sys_clock_getres`, `sys_nanosleep`,
//! `sys_gettimeofday`, `sys_settimeofday`, `sys_adjfreq`, `sys_adjtime`, `setitimer`,
//! `cancel_all_itimers`, `sys_getitimer`, `sys_setitimer`, `realitexpire`, `itimerfix`,
//! `itimerdecr`, `itimer_update`, `process_reset_itimer_flag`, `ratecheck`, `ppsratecheck`,
//! `inittodr`, `resettodr`, `todr_attach` and the periodic `resettodr`.
//!
//! These routines provide the kernel entry points to get and set the time-of-day and
//! per-process interval timers. Subroutines here provide support for adding and
//! subtracting timeval structures and decrementing interval timers, optionally reloading
//! the interval timers when they expire.
//!
//! ## Deviations
//! - No time-of-day chip driver is ported (`todr_attach` has no caller): `inittodr` sets the
//!   clock from the file system's time and warns, `resettodr` has nothing to write.
//! - `itimerdecr` returns `bool` (`true` where the C returns 1: the timer is still
//!   running).

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::conf::param::TICK;
use crate::dev::clock_subr::{TodrChipHandle, todr_gettime, todr_settime};
use crate::kern::kern_clock::{STATHZ, hardclock_period};
use crate::kern::kern_clockintr::clockrequest_advance;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_pledge::pledge_adjtime;
use crate::kern::kern_proc::tfind_user;
use crate::kern::kern_prot::suser;
use crate::kern::kern_resource::{tuagg_get_proc, tuagg_get_process};
use crate::kern::kern_rwlock::{
    rw_enter, rw_enter_read, rw_enter_write, rw_exit, rw_exit_read, rw_exit_write,
};
use crate::kern::kern_sig::prsignal;
use crate::kern::kern_synch::{nowake, tsleep_nsec};
use crate::kern::kern_sysctl::SECURELEVEL;
use crate::kern::kern_task::{SYSTQ, task_add, task_del};
use crate::kern::kern_tc::{
    TC_LOCK, getmicrouptime, getnanouptime, microtime, nanoruntime, nanotime, nanouptime,
    tc_adjfreq, tc_adjtime, tc_getfrequency, tc_getprecision, tc_setclock, tc_setrealtimeclock,
};
use crate::kern::kern_timeout::{timeout_abs_ts, timeout_add_sec, timeout_del};
use crate::kprintf;
use crate::machine::Machine;
use crate::machine::copy::{copyin_obj, copyout, copyout_obj};
use crate::machine::cpu::{ClockFrame, Cpu, curcpu, curproc, need_resched};
use crate::machine::intr::{IPL_CLOCK, IPL_HIGH};
use crate::sys::_time::{
    CLOCK_BOOTTIME, CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME,
    CLOCK_THREAD_CPUTIME_ID, CLOCK_UPTIME, Itimerspec, clock_ptid, clock_type,
};
use crate::sys::clockintr::Clockrequest;
use crate::sys::errno::Errno;
use crate::sys::mutex::Mutex;
use crate::sys::param::{PCATCH, PWAIT};
use crate::sys::proc::{
    P_ALRMPEND, P_PROFPEND, P_SYSTEM, P_WEXIT, PS_EXITING, PS_ITIMER, Proc, Process, Tusage,
};
use crate::sys::rwlock::{RW_READ, RW_WRITE};
use crate::sys::signal::SIGALRM;
use crate::sys::syscallargs::{
    SysAdjfreqArgs, SysAdjtimeArgs, SysClockGetresArgs, SysClockGettimeArgs, SysClockSettimeArgs,
    SysGetitimerArgs, SysGettimeofdayArgs, SysNanosleepArgs, SysSetitimerArgs, SysSettimeofdayArgs,
};
use crate::sys::systm::{MAXTSLP, SysArgs, kernel_lock, kernel_unlock, sysargs};
use crate::sys::task::Task;
use crate::sys::time::{
    Bintime, ITIMER_PROF, ITIMER_REAL, ITIMER_VIRTUAL, Itimerval, SECDAY, SECYR, Timespec, Timeval,
    Timezone, bintime_to_timespec, nsec_to_timespec, timersub, timespec_to_nsec,
    timespec_to_timeval, timespecadd, timespecsub, timeval_to_timespec,
};
use crate::sys::timeout::Timeout;
use crate::sys::types::{Clockid, Register, Suseconds, Time};

/// `itimer_mtx`: the virtual and profiling interval timers.
pub static ITIMER_MTX: Mutex = Mutex::new(IPL_CLOCK);

/// This function is used by clock_settime and settimeofday.
pub fn settime(ts: &Timespec) -> Result<(), Errno> {
    // Don't allow the time to be set forward so far it will wrap and become negative, thus
    // allowing an attacker to bypass the next check below. The cutoff is 1 year before
    // rollover occurs, so even if the attacker uses adjtime(2) to move the time past the
    // cutoff, it will take a very long time to get to the wrap point.
    //
    // XXX: we check against UINT_MAX until we can figure out how to deal with the hardware
    // RTCs.
    if ts.tv_sec > i64::from(u32::MAX) - 365 * 24 * 60 * 60 {
        kprintf!("denied attempt to set clock forward to {}\n", ts.tv_sec);
        return Err(Errno::EPERM);
    }
    // If the system is secure, we do not allow the time to be set to an earlier value (it
    // may be slowed using adjtime, but not set back). This feature prevent interlopers from
    // setting arbitrary time stamps on files.
    let now = nanotime();
    if SECURELEVEL.load(Ordering::Relaxed) > 1 && *ts <= now {
        kprintf!(
            "denied attempt to set clock back {} seconds\n",
            now.tv_sec - ts.tv_sec
        );
        return Err(Errno::EPERM);
    }

    tc_setrealtimeclock(ts);
    kernel_lock(); // KERNEL_LOCK()
    resettodr();
    kernel_unlock(); // KERNEL_UNLOCK()

    Ok(())
}

/// `clock_gettime`: the time of `clock_id` as `p` sees it.
pub fn clock_gettime(p: &Proc, clock_id: Clockid) -> Result<Timespec, Errno> {
    let spc_runtime = || Machine::ci_schedstate(curcpu()).spc_runtime.get();
    match clock_id {
        CLOCK_REALTIME => Ok(nanotime()),
        CLOCK_UPTIME => Ok(nanoruntime()),
        CLOCK_MONOTONIC | CLOCK_BOOTTIME => Ok(nanouptime()),
        CLOCK_PROCESS_CPUTIME_ID => {
            let tu = Tusage::new();
            let tp = nanouptime();
            tuagg_get_process(&tu, p.process());
            let tp = timespecsub(&tp, &spc_runtime());
            Ok(timespecadd(&tp, &tu.tu_runtime.get()))
        }
        CLOCK_THREAD_CPUTIME_ID => {
            let tu = Tusage::new();
            let tp = nanouptime();
            tuagg_get_proc(&tu, p);
            let tp = timespecsub(&tp, &spc_runtime());
            Ok(timespecadd(&tp, &tu.tu_runtime.get()))
        }
        _ => {
            // check for clock from pthread_getcpuclockid()
            if clock_type(clock_id) == CLOCK_THREAD_CPUTIME_ID {
                kernel_lock(); // KERNEL_LOCK()
                let error = match tfind_user(clock_ptid(clock_id), p.process()) {
                    None => Err(Errno::ESRCH),
                    Some(q) => {
                        let tu = Tusage::new();
                        tuagg_get_proc(&tu, q);
                        Ok(tu.tu_runtime.get())
                    }
                };
                kernel_unlock(); // KERNEL_UNLOCK()
                error
            } else {
                Err(Errno::EINVAL)
            }
        }
    }
}

/// `clock_gettime(2)`.
pub fn sys_clock_gettime(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysClockGettimeArgs = sysargs(v);

    let ats = clock_gettime(p, uap.clock_id.get())?;

    copyout_obj(&ats, uap.tp.get() as usize)
    // KTRACE: not configured.
}

/// `clock_settime(2)`: root only; only `CLOCK_REALTIME` can be set.
pub fn sys_clock_settime(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysClockSettimeArgs = sysargs(v);

    suser(p)?;

    let ats: Timespec = copyin_obj(uap.tp.get() as usize)?;

    match uap.clock_id.get() {
        CLOCK_REALTIME => {
            if !ats.is_valid() {
                return Err(Errno::EINVAL);
            }
            settime(&ats)?;
        }
        // Other clocks are read-only
        _ => return Err(Errno::EINVAL),
    }

    Ok(())
}

/// `clock_getres(2)`.
pub fn sys_clock_getres(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysClockGetresArgs = sysargs(v);
    let mut ts = Timespec::new(0, 0);
    let clock_id = uap.clock_id.get();
    let stathz = || i64::from(STATHZ.load(Ordering::Relaxed).max(1));

    let error = match clock_id {
        CLOCK_REALTIME | CLOCK_MONOTONIC | CLOCK_BOOTTIME | CLOCK_UPTIME => {
            let mut bt = Bintime::default();
            rw_enter_read(&TC_LOCK);
            let scale = ((1u64 << 63) / tc_getfrequency()) * 2;
            bt.frac = tc_getprecision().wrapping_mul(scale);
            rw_exit_read(&TC_LOCK);
            ts = bintime_to_timespec(&bt);
            Ok(())
        }
        CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID => {
            ts.tv_nsec = 1_000_000_000 / stathz();
            Ok(())
        }
        _ => {
            // check for clock from pthread_getcpuclockid()
            if clock_type(clock_id) == CLOCK_THREAD_CPUTIME_ID {
                kernel_lock(); // KERNEL_LOCK()
                let error = match tfind_user(clock_ptid(clock_id), p.process()) {
                    None => Err(Errno::ESRCH),
                    Some(_) => {
                        ts.tv_nsec = 1_000_000_000 / stathz();
                        Ok(())
                    }
                };
                kernel_unlock(); // KERNEL_UNLOCK()
                error
            } else {
                Err(Errno::EINVAL)
            }
        }
    };

    let tp = uap.tp.get() as usize;
    if error.is_ok() && tp != 0 {
        ts.tv_nsec = ts.tv_nsec.max(1);
        copyout_obj(&ts, tp)?;
        // KTRACE: not configured.
    }

    error
}

/// `nanosleep(2)`: sleeps for the requested time, or until a signal; the time left is
/// copied out to `rmtp`.
pub fn sys_nanosleep(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysNanosleepArgs = sysargs(v);
    let _ = p;

    let rmtp = uap.rmtp.get() as usize;
    let mut request: Timespec = copyin_obj(uap.rqtp.get() as usize)?;
    // KTRACE: not configured.

    if request.tv_sec < 0 || !request.is_valid() {
        return Err(Errno::EINVAL);
    }

    let mut error;
    loop {
        let start = getnanouptime();
        let nsecs = timespec_to_nsec(&request).clamp(1, MAXTSLP);
        error = tsleep_nsec(nowake(), PWAIT | PCATCH, "nanoslp", nsecs);
        let stop = getnanouptime();
        let elapsed = timespecsub(&stop, &start);
        request = timespecsub(&request, &elapsed);
        if request.tv_sec < 0 {
            request = Timespec::new(0, 0);
        }
        if error != Err(Errno::EWOULDBLOCK) {
            break;
        }
        if !request.is_set() {
            break;
        }
    }

    if error == Err(Errno::ERESTART) {
        error = Err(Errno::EINTR);
    }
    if error == Err(Errno::EWOULDBLOCK) {
        error = Ok(());
    }

    if rmtp != 0 {
        let remainder = request;
        if let Err(copyout_error) = copyout_obj(&remainder, rmtp) {
            error = Err(copyout_error);
        }
        // KTRACE: not configured.
    }

    error
}

/// `gettimeofday(2)`: the time of day; the time zone is always zero.
pub fn sys_gettimeofday(_p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGettimeofdayArgs = sysargs(v);
    let zerotz = Timezone::default();

    let tp = uap.tp.get() as usize;
    let tzp = uap.tzp.get() as usize;

    if tp != 0 {
        let atv = microtime();
        copyout_obj(&atv, tp)?;
        // KTRACE: not configured.
    }
    if tzp != 0 {
        copyout_obj(&zerotz, tzp)?;
    }
    Ok(())
}

/// `settimeofday(2)`: root only; the time zone is checked for readability and ignored.
pub fn sys_settimeofday(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSettimeofdayArgs = sysargs(v);

    let tv = uap.tv.get() as usize;
    let tzp = uap.tzp.get() as usize;

    suser(p)?;
    // Verify all parameters before changing time.
    let atv: Option<Timeval> = if tv != 0 { Some(copyin_obj(tv)?) } else { None };
    if tzp != 0 {
        let _atz: Timezone = copyin_obj(tzp)?;
    }
    if let Some(atv) = atv {
        // KTRACE: not configured.
        if !atv.is_valid() {
            return Err(Errno::EINVAL);
        }
        let ts = timeval_to_timespec(&atv);
        settime(&ts)?;
    }

    Ok(())
}

/// `ADJFREQ_MAX`.
const ADJFREQ_MAX: i64 = 500_000_000i64 << 32;
/// `ADJFREQ_MIN`.
const ADJFREQ_MIN: i64 = -ADJFREQ_MAX;

/// `adjfreq(2)`: reads and (root only) sets the timecounter's frequency adjustment.
pub fn sys_adjfreq(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysAdjfreqArgs = sysargs(v);
    let freq = uap.freq.get() as usize;
    let oldfreq = uap.oldfreq.get() as usize;
    let mut f = 0i64;

    if freq != 0 {
        suser(p)?;
        let mut b = [0u8; 8];
        crate::machine::copy::copyin(freq, &mut b)?;
        f = i64::from_ne_bytes(b);
        if !(ADJFREQ_MIN..=ADJFREQ_MAX).contains(&f) {
            return Err(Errno::EINVAL);
        }
    }

    let _ = rw_enter(&TC_LOCK, if freq == 0 { RW_READ } else { RW_WRITE });
    let error = 'out: {
        if oldfreq != 0 {
            let mut oldf = 0i64;
            tc_adjfreq(Some(&mut oldf), None);
            if let Err(e) = copyout(&oldf.to_ne_bytes(), oldfreq) {
                break 'out Err(e);
            }
        }
        if freq != 0 {
            tc_adjfreq(None, Some(f));
        }
        Ok(())
    };
    rw_exit(&TC_LOCK);
    error
}

/// `adjtime(2)`: reads and (root only) sets the remaining adjustment of the clock.
pub fn sys_adjtime(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysAdjtimeArgs = sysargs(v);
    let delta = uap.delta.get() as usize;
    let olddelta = uap.olddelta.get() as usize;
    let mut adjustment = 0i64;

    pledge_adjtime(p, delta)?;

    if delta != 0 {
        suser(p)?;
        let atv: Timeval = copyin_obj(delta)?;
        // KTRACE: not configured.
        if !atv.is_valid() {
            return Err(Errno::EINVAL);
        }

        if atv.tv_sec > i64::MAX / 1_000_000 {
            return Err(Errno::EINVAL);
        }
        if atv.tv_sec < i64::MIN / 1_000_000 {
            return Err(Errno::EINVAL);
        }
        adjustment = atv.tv_sec * 1_000_000;
        if adjustment > i64::MAX - atv.tv_usec as i64 {
            return Err(Errno::EINVAL);
        }
        adjustment += atv.tv_usec as i64;

        rw_enter_write(&TC_LOCK);
    }

    let error = 'out: {
        if olddelta != 0 {
            let mut remaining = 0i64;
            tc_adjtime(Some(&mut remaining), None);
            let mut atv = Timeval::new(remaining / 1_000_000, (remaining % 1_000_000) as Suseconds);
            if atv.tv_usec < 0 {
                atv.tv_usec += 1_000_000;
                atv.tv_sec -= 1;
            }

            if let Err(e) = copyout_obj(&atv, olddelta) {
                break 'out Err(e);
            }
        }

        if delta != 0 {
            tc_adjtime(None, Some(adjustment));
        }
        Ok(())
    };
    if delta != 0 {
        rw_exit_write(&TC_LOCK);
    }
    error
}

/// Get or set value of an interval timer. The process virtual and profiling virtual time
/// timers are kept internally in the way they are specified externally: in time until they
/// expire.
///
/// The real time interval timer's `it_value`, in contrast, is kept as an absolute time
/// rather than as a delta, so that it is easy to keep periodic real-time signals from
/// drifting.
///
/// Virtual time timers are processed in the hardclock() routine of kern_clock.c. The real
/// time timer is processed by a timeout routine, called from the softclock() routine.
/// Since a callout may be delayed in real time due to interrupt processing in the system,
/// it is possible for the real time timeout routine (realitexpire, given below), to be
/// delayed in real time past when it is supposed to occur. It does not suffice, therefore,
/// to reload the real timer .it_value from the real time timers .it_interval. Rather, we
/// compute the next time in absolute time the timer should go off.
pub fn setitimer(which: i32, itv: Option<&Itimerval>, olditv: Option<&mut Itimerval>) {
    crate::kassert!((ITIMER_REAL..=ITIMER_PROF).contains(&which));

    let Some(p) = curproc() else {
        return;
    };
    let pr = p.process();
    let itimer = &pr.ps_timer[which as usize];
    let mut now = Timespec::new(0, 0);

    let its = itv.map(|itv| Itimerspec {
        it_value: timeval_to_timespec(&itv.it_value),
        it_interval: timeval_to_timespec(&itv.it_interval),
    });

    if which == ITIMER_REAL {
        mtx_enter(&pr.ps_mtx);
        now = nanouptime();
    } else {
        mtx_enter(&ITIMER_MTX);
    }

    let mut oldits = Itimerspec::new();
    if olditv.is_some() {
        oldits = itimer.get();
    }
    if let Some(mut its) = its {
        if which == ITIMER_REAL {
            if its.it_value.is_set() {
                its.it_value = timespecadd(&its.it_value, &now);
                timeout_abs_ts(&pr.ps_realit_to, &its.it_value);
            } else {
                timeout_del(&pr.ps_realit_to);
            }
        }
        itimer.set(its);
        if which == ITIMER_VIRTUAL || which == ITIMER_PROF {
            process_reset_itimer_flag(pr);
            need_resched(curcpu());
        }
    }

    if which == ITIMER_REAL {
        mtx_leave(&pr.ps_mtx);
    } else {
        mtx_leave(&ITIMER_MTX);
    }

    if let Some(olditv) = olditv {
        if which == ITIMER_REAL && oldits.it_value.is_set() {
            if oldits.it_value < now {
                oldits.it_value = Timespec::new(0, 0);
            } else {
                oldits.it_value = timespecsub(&oldits.it_value, &now);
            }
        }
        olditv.it_value = timespec_to_timeval(&oldits.it_value);
        olditv.it_interval = timespec_to_timeval(&oldits.it_interval);
    }
}

/// `cancel_all_itimers`: stop the calling process's three interval timers.
pub fn cancel_all_itimers() {
    let itv = Itimerval::default();

    for i in ITIMER_REAL..=ITIMER_PROF {
        setitimer(i, Some(&itv), None);
    }
}

/// `getitimer(2)`.
pub fn sys_getitimer(_p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetitimerArgs = sysargs(v);

    let which = uap.which.get();
    if !(ITIMER_REAL..=ITIMER_PROF).contains(&which) {
        return Err(Errno::EINVAL);
    }

    let mut aitv = Itimerval::default();

    setitimer(which, None, Some(&mut aitv));

    copyout_obj(&aitv, uap.itv.get() as usize)
    // KTRACE: not configured.
}

/// `setitimer(2)`.
pub fn sys_setitimer(_p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetitimerArgs = sysargs(v);

    let which = uap.which.get();
    if !(ITIMER_REAL..=ITIMER_PROF).contains(&which) {
        return Err(Errno::EINVAL);
    }

    let uitv = uap.itv.get() as usize;
    let uoitv = uap.oitv.get() as usize;
    let mut aitv = Itimerval::default();
    let mut olditv = Itimerval::default();
    let mut newitv = false;
    if uitv != 0 {
        aitv = copyin_obj(uitv)?;
        // KTRACE: not configured.
        itimerfix(&mut aitv)?;
        newitv = true;
    }
    if !newitv && uoitv == 0 {
        return Ok(());
    }

    setitimer(
        which,
        if newitv { Some(&aitv) } else { None },
        if uoitv != 0 { Some(&mut olditv) } else { None },
    );

    if uoitv != 0 {
        return copyout_obj(&olditv, uoitv);
        // KTRACE: not configured.
    }

    Ok(())
}

/// Real interval timer expired: send process whose timer expired an alarm signal. If time
/// is not set up to reload, then just return. Else compute next time timer should go off
/// which is > current time. This is where delay in processing this timeout causes multiple
/// SIGALRM calls to be compressed into one.
pub fn realitexpire(arg: *mut c_void) {
    // SAFETY: `process_initialize` hands the process itself, alive while its timeout can
    // fire (`exit1` cancels it).
    let pr: &Process = unsafe { &*arg.cast::<Process>() };
    let tp = &pr.ps_timer[ITIMER_REAL as usize];
    let mut need_signal = false;

    mtx_enter(&pr.ps_mtx);

    'out: {
        let mut t = tp.get();
        // Do nothing if the timer was cancelled or rescheduled while we were entering the
        // mutex.
        if !t.it_value.is_set() || crate::sys::timeout::timeout_pending(&pr.ps_realit_to) {
            break 'out;
        }

        // The timer expired. We need to send the signal.
        need_signal = true;

        // One-shot timers are not reloaded.
        if !t.it_interval.is_set() {
            t.it_value = Timespec::new(0, 0);
            tp.set(t);
            break 'out;
        }

        // Find the nearest future expiration point and restart the timeout.
        let cts = nanouptime();
        while t.it_value <= cts {
            t.it_value = timespecadd(&t.it_value, &t.it_interval);
        }
        tp.set(t);
        if pr.ps_flags.load(Ordering::Relaxed) & PS_EXITING == 0 {
            timeout_abs_ts(&pr.ps_realit_to, &t.it_value);
        }
    }

    mtx_leave(&pr.ps_mtx);

    if need_signal {
        prsignal(pr, SIGALRM);
    }
}

/// Check if the given setitimer(2) input is valid. Clear it_interval if it_value is unset.
/// Round it_interval up to the minimum interval if necessary.
pub fn itimerfix(itv: &mut Itimerval) -> Result<(), Errno> {
    let max = Timeval::new(i64::from(u32::MAX), 0);
    let min_interval = Timeval::new(0, TICK.load(Ordering::Relaxed) as Suseconds);

    if itv.it_value.tv_sec < 0 || !itv.it_value.is_valid() {
        return Err(Errno::EINVAL);
    }
    if itv.it_value > max {
        return Err(Errno::EINVAL);
    }
    if itv.it_interval.tv_sec < 0 || !itv.it_interval.is_valid() {
        return Err(Errno::EINVAL);
    }
    if itv.it_interval > max {
        return Err(Errno::EINVAL);
    }

    if !itv.it_value.is_set() {
        itv.it_interval = Timeval::new(0, 0);
    }
    if itv.it_interval.is_set() && itv.it_interval < min_interval {
        itv.it_interval = min_interval;
    }

    Ok(())
}

/// Decrement an interval timer by the given duration. If the timer expires and it is
/// periodic then reload it. When reloading the timer we subtract any overrun from the next
/// period so that the timer does not drift. `true` while the timer has not expired.
pub fn itimerdecr(itp: &mut Itimerspec, decrement: &Timespec) -> bool {
    itp.it_value = timespecsub(&itp.it_value, decrement);
    if itp.it_value.tv_sec >= 0 && itp.it_value.is_set() {
        return true;
    }
    if !itp.it_interval.is_set() {
        itp.it_value = Timespec::new(0, 0);
        return false;
    }
    while itp.it_value.tv_sec < 0 || !itp.it_value.is_set() {
        itp.it_value = timespecadd(&itp.it_value, &itp.it_interval);
    }
    false
}

/// `itimer_update`: the per-CPU clock interrupt that decrements the running process's
/// virtual and profiling interval timers.
pub fn itimer_update(cr: &Clockrequest, cf: *mut c_void, _arg: *mut c_void) {
    let Some(p) = curproc() else {
        return;
    };
    if p.p_flag.load(Ordering::Relaxed) & (P_SYSTEM | P_WEXIT) != 0 {
        return;
    }

    let pr = p.process();
    if pr.ps_flags.load(Ordering::Relaxed) & PS_ITIMER == 0 {
        return;
    }

    let nsecs = clockrequest_advance(cr, hardclock_period()) * hardclock_period();
    let elapsed = nsec_to_timespec(nsecs);

    // SAFETY: the dispatcher passes the clock frame the interrupt entry built.
    let usermode = unsafe { cf.cast::<ClockFrame>().as_ref() }.is_some_and(Machine::clkf_usermode);

    mtx_enter(&ITIMER_MTX);
    let mut virt = pr.ps_timer[ITIMER_VIRTUAL as usize].get();
    if usermode && virt.it_value.is_set() && !itimerdecr(&mut virt, &elapsed) {
        pr.ps_timer[ITIMER_VIRTUAL as usize].set(virt);
        process_reset_itimer_flag(pr);
        p.p_flag.fetch_or(P_ALRMPEND, Ordering::Relaxed);
        Machine::need_proftick(p);
    } else {
        pr.ps_timer[ITIMER_VIRTUAL as usize].set(virt);
    }
    let mut prof = pr.ps_timer[ITIMER_PROF as usize].get();
    if prof.it_value.is_set() && !itimerdecr(&mut prof, &elapsed) {
        pr.ps_timer[ITIMER_PROF as usize].set(prof);
        process_reset_itimer_flag(pr);
        p.p_flag.fetch_or(P_PROFPEND, Ordering::Relaxed);
        Machine::need_proftick(p);
    } else {
        pr.ps_timer[ITIMER_PROF as usize].set(prof);
    }
    mtx_leave(&ITIMER_MTX);
}

/// `process_reset_itimer_flag`: `PS_ITIMER` says whether a virtual or profiling timer runs.
pub fn process_reset_itimer_flag(ps: &Process) {
    if ps.ps_timer[ITIMER_VIRTUAL as usize].get().it_value.is_set()
        || ps.ps_timer[ITIMER_PROF as usize].get().it_value.is_set()
    {
        ps.ps_flags.fetch_or(PS_ITIMER, Ordering::Relaxed);
    } else {
        ps.ps_flags.fetch_and(!PS_ITIMER, Ordering::Relaxed);
    }
}

/// `ratecheck_mtx`.
static RATECHECK_MTX: Mutex = Mutex::new(IPL_HIGH);

/// `ratecheck()`: simple time-based rate-limit checking. see ratecheck(9) for usage and
/// rationale.
pub fn ratecheck(lasttime: &mut Timeval, mininterval: &Timeval) -> bool {
    let mut rv = false;
    let tv = getmicrouptime();

    mtx_enter(&RATECHECK_MTX);
    let delta = timersub(&tv, lasttime);

    // check for 0,0 is so that the message will be seen at least once, even if interval is
    // huge.
    if delta >= *mininterval || (lasttime.tv_sec == 0 && lasttime.tv_usec == 0) {
        *lasttime = tv;
        rv = true;
    }
    mtx_leave(&RATECHECK_MTX);

    rv
}

/// `ppsratecheck_mtx`.
static PPSRATECHECK_MTX: Mutex = Mutex::new(IPL_HIGH);

/// `ppsratecheck()`: packets (or events) per second limitation.
pub fn ppsratecheck(lasttime: &mut Timeval, curpps: &mut i32, maxpps: i32) -> bool {
    // SAFETY: two exclusive references are valid and touched by no one else.
    unsafe { ppsratecheck_shared(lasttime, curpps, maxpps) }
}

/// `ppsratecheck()` on counters shared between CPUs (the network's rate limiters, which
/// softnet threads reach at once): the pointers are dereferenced only inside
/// `ppsratecheck_mtx`, which is what orders every CPU's accesses in C.
///
/// # Safety
/// `lasttime` and `curpps` are valid for reads and writes for the whole call, and every
/// other access to them, from any CPU, also goes through `ppsratecheck` (inside the mutex).
pub unsafe fn ppsratecheck_shared(lasttime: *mut Timeval, curpps: *mut i32, maxpps: i32) -> bool {
    let tv = getmicrouptime();

    mtx_enter(&PPSRATECHECK_MTX);
    // SAFETY: the caller's contract; the mutex is held until the references' last use.
    let (lasttime, curpps) = unsafe { (&mut *lasttime, &mut *curpps) };
    let delta = timersub(&tv, lasttime);

    // check for 0,0 is so that the message will be seen at least once. if more than one
    // second have passed since the last update of lasttime, reset the counter.
    //
    // we do increment *curpps even in *curpps < maxpps case, as some may try to use *curpps
    // for stat purposes as well.
    let rv = if (lasttime.tv_sec == 0 && lasttime.tv_usec == 0) || delta.tv_sec >= 1 {
        *lasttime = tv;
        *curpps = 0;
        true
    } else if maxpps < 0 {
        true
    } else {
        *curpps < maxpps
    };

    // the following should be done on 32-bit arithmetic, since the C code did it so
    // (rate-limit "maxpps" is an int).
    *curpps = curpps.saturating_add(1);
    mtx_leave(&PPSRATECHECK_MTX);

    rv
}

/// `todr_handle`: the time-of-day clock chip, null until a driver attaches one.
static TODR_HANDLE: AtomicPtr<TodrChipHandle> = AtomicPtr::new(ptr::null_mut());
/// `inittodr_done`.
static INITTODR_DONE: AtomicBool = AtomicBool::new(false);

/// `MINYEAR`: minimum plausible year (`OpenBSD / 100 - 1`, from `<sys/param.h>`'s
/// `OpenBSD` date).
const MINYEAR: i64 = crate::sys::param::OpenBSD as i64 / 100 - 1;

/// The attached chip, if any.
fn todr_handle() -> Option<&'static TodrChipHandle> {
    // SAFETY: a non-null `todr_handle` is a `&'static` chip handle `todr_attach` stored.
    unsafe { TODR_HANDLE.load(Ordering::Acquire).as_ref() }
}

/// inittodr: initialize time from the time-of-day register.
pub fn inittodr(base: Time) {
    let mut base = base;
    let badbase;

    INITTODR_DONE.store(true, Ordering::Relaxed);

    if base < (MINYEAR - 1970) * SECYR {
        kprintf!("WARNING: preposterous time in file system\n");
        // read the system clock anyway
        base = (MINYEAR - 1970) * SECYR;
        badbase = true;
    } else {
        badbase = false;
    }

    let mut rtctime = Timeval::new(base, 0);

    'bad: {
        let read_ok = match todr_handle() {
            Some(todr) => todr_gettime(todr, &mut rtctime).is_ok(),
            None => false,
        };
        if !read_ok || rtctime.tv_sec < (MINYEAR - 1970) * SECYR {
            // Believe the time in the file system for lack of anything better, resetting
            // the TODR.
            rtctime = Timeval::new(base, 0);
            if todr_handle().is_some() && !badbase {
                kprintf!("WARNING: bad clock chip time\n");
            }
            let ts = Timespec::new(rtctime.tv_sec, rtctime.tv_usec as i64 * 1000);
            tc_setclock(&ts);
            break 'bad;
        } else {
            let ts = Timespec::new(rtctime.tv_sec, rtctime.tv_usec as i64 * 1000);
            tc_setclock(&ts);
        }

        if !badbase {
            // See if we gained/lost two or more days; if so, assume something is amiss.
            let deltat = (rtctime.tv_sec - base).abs();
            if deltat < 2 * SECDAY {
                return; // all is well
            }
            kprintf!(
                "WARNING: clock {} {} days\n",
                if rtctime.tv_sec < base {
                    "lost"
                } else {
                    "gained"
                },
                deltat / SECDAY
            );
        }
    }
    // bad:
    kprintf!("WARNING: CHECK AND RESET THE DATE!\n");
}

/// resettodr: reset the time-of-day register with the current time.
pub fn resettodr() {
    // Skip writing the RTC if inittodr(9) never ran. We don't want to overwrite a
    // reasonable value with a nonsense value.
    if !INITTODR_DONE.load(Ordering::Relaxed) {
        return;
    }

    let mut rtctime = microtime();

    if let Some(todr) = todr_handle()
        && todr_settime(todr, &mut rtctime).is_err()
    {
        kprintf!("WARNING: can't update clock chip time\n");
    }
}

/// `todr_attach`: the best chip a driver offers becomes the time-of-day clock.
pub fn todr_attach(todr: &'static TodrChipHandle) {
    let better = match todr_handle() {
        None => true,
        Some(cur) => todr.todr_quality > cur.todr_quality,
    };
    if better {
        TODR_HANDLE.store(ptr::from_ref(todr).cast_mut(), Ordering::Release);
    }
}

/// `RESETTODR_PERIOD`.
const RESETTODR_PERIOD: i32 = 1800;

/// `resettodr_to`.
static RESETTODR_TO: Timeout = Timeout::new(periodic_resettodr, ptr::null_mut());
/// `resettodr_task`.
static RESETTODR_TASK: Task = Task::new(perform_resettodr, ptr::null_mut());

/// `periodic_resettodr`: the timeout hands the write to `systq`.
pub fn periodic_resettodr(_arg: *mut c_void) {
    task_add(SYSTQ, &RESETTODR_TASK);
}

/// `perform_resettodr`.
pub fn perform_resettodr(_arg: *mut c_void) {
    resettodr();
    timeout_add_sec(&RESETTODR_TO, RESETTODR_PERIOD);
}

/// `start_periodic_resettodr`.
pub fn start_periodic_resettodr() {
    timeout_add_sec(&RESETTODR_TO, RESETTODR_PERIOD);
}

/// `stop_periodic_resettodr`.
pub fn stop_periodic_resettodr() {
    timeout_del(&RESETTODR_TO);
    task_del(SYSTQ, &RESETTODR_TASK);
}

const _: () = {
    // The interval timers are indexed by ITIMER_REAL..=ITIMER_PROF.
    assert!(ITIMER_REAL == 0 && ITIMER_PROF == 2);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn itimerdecr_reloads_without_drift() {
        let mut it = Itimerspec {
            it_interval: Timespec::new(1, 0),
            it_value: Timespec::new(0, 500_000_000),
        };
        assert!(itimerdecr(&mut it, &Timespec::new(0, 200_000_000)));
        assert_eq!(it.it_value, Timespec::new(0, 300_000_000));
        // expires, overruns by 0.2 s: the next period is shortened by the overrun
        assert!(!itimerdecr(&mut it, &Timespec::new(0, 500_000_000)));
        assert_eq!(it.it_value, Timespec::new(0, 800_000_000));
        // a one-shot timer clears
        let mut one = Itimerspec {
            it_interval: Timespec::new(0, 0),
            it_value: Timespec::new(0, 1),
        };
        assert!(!itimerdecr(&mut one, &Timespec::new(1, 0)));
        assert!(!one.it_value.is_set());
    }

    #[test]
    fn itimerfix_validates_and_rounds() {
        let tick = TICK.load(Ordering::Relaxed) as Suseconds;
        let mut itv = Itimerval {
            it_interval: Timeval::new(0, 1),
            it_value: Timeval::new(1, 0),
        };
        assert_eq!(itimerfix(&mut itv), Ok(()));
        assert_eq!(itv.it_interval, Timeval::new(0, tick));
        let mut zero = Itimerval {
            it_interval: Timeval::new(5, 0),
            it_value: Timeval::new(0, 0),
        };
        assert_eq!(itimerfix(&mut zero), Ok(()));
        assert!(!zero.it_interval.is_set());
        let mut bad = Itimerval {
            it_interval: Timeval::new(0, 0),
            it_value: Timeval::new(-1, 0),
        };
        assert_eq!(itimerfix(&mut bad), Err(Errno::EINVAL));
        let mut big = Itimerval {
            it_interval: Timeval::new(0, 0),
            it_value: Timeval::new(i64::from(u32::MAX) + 1, 0),
        };
        assert_eq!(itimerfix(&mut big), Err(Errno::EINVAL));
    }
}
