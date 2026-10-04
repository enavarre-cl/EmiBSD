/*	$OpenBSD: kern_clock.c,v 1.127 2025/06/01 03:43:48 dlg Exp $	*/
/*	$NetBSD: kern_clock.c,v 1.34 1996/06/09 04:51:03 briggs Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1991, 1993
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
 *	@(#)kern_clock.c	8.5 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! Clock handling routines: `kern/kern_clock.c`.
//!
//! This code is written to operate with two timers that run independently of each other.
//! The main clock, running hz times per second, is used to keep track of real time. The
//! second timer handles kernel and user profiling, and does resource use estimation. If the
//! second timer is programmable, it is randomized to avoid aliasing between the two clocks.
//! For example, the randomization prevents an adversary from always giving up the cpu just
//! before its quantum expires. Otherwise, it would never accumulate cpu ticks. The mean
//! frequency of the second timer is stathz.
//!
//! If no second timer exists, stathz will be zero; in this case we drive profiling and
//! statistics off the main clock. This WILL NOT be accurate; do not do it unless absolutely
//! necessary.
//!
//! The statistics clock may (or may not) be run at a higher rate while profiling. This
//! profile clock runs at profhz. We require that profhz be an integral multiple of stathz.
//!
//! If the statistics clock is running fast, it must be divided by the ratio profhz/stathz
//! for statistics. (For profiling, every tick counts.)
//!
//! Upstream: sys/kern/kern_clock.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports the globals, `initclocks`, `hardclock`, `tvtohz`,
//! `tstohz` and `statclock`; M7b completes `statclock`'s per-thread accounting
//! (`p_cpticks`, the `tusage` ticks and sizes, `schedclock`). `startprofclock` and
//! `stopprofclock` (profiling) are not ported; `sysctl_clockrate` came with `kern_sysctl.c`.
//!
//! ## Deviations
//! - None beyond the module's status: the profiling half of the file is missing.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

use crate::conf::param::{HZ, TICK};
use crate::kassert;
use crate::kern::kern_clockintr::{clockrequest_advance, clockrequest_advance_random};
use crate::kern::kern_lock::{pc_sprod_enter, pc_sprod_leave};
use crate::kern::kern_sysctl::sysctl_rdstruct;
use crate::kern::kern_tc::{inittimecounter, tc_ticktock};
use crate::kern::kern_timeout::timeout_hardclock_update;
use crate::kern::sched_bsd::ROUNDROBIN_PERIOD;
use crate::kern::sched_bsd::schedclock;
use crate::kern::subr_prof::PROFCLOCK_PERIOD;
use crate::machine::Machine;
use crate::machine::cpu::{ClockFrame, Cpu, cpu_initclocks, cpu_startclock, curcpu};
use crate::sys::clockintr::Clockrequest;
use crate::sys::errno::Errno;
use crate::sys::param::{NZERO, PAGE_SHIFT};
use crate::sys::proc::{P_SYSTEM, TU_ITICKS, TU_STICKS, TU_UTICKS, tu_enter, tu_leave};
use crate::sys::sched::{CP_IDLE, CP_INTR, CP_NICE, CP_SPIN, CP_SYS, CP_USER};
use crate::sys::sysctl::SysctlPlain;
use crate::sys::time::{Clockinfo, Timespec, Timeval, timespec_to_timeval};

/// `stathz`: the statistics clock's frequency.
pub static STATHZ: AtomicI32 = AtomicI32::new(0);
/// `profhz`: the profiling clock's frequency.
pub static PROFHZ: AtomicI32 = AtomicI32::new(0);
/// `profprocs`: the number of processes being profiled.
pub static PROFPROCS: AtomicI32 = AtomicI32::new(0);
/// `ticks`: the number of hardclock ticks, started close to wrapping.
pub static TICKS: AtomicI32 = AtomicI32::new(i32::MAX - (15 * 60 * crate::sys::kernel::HZ));

/// `jiffies` (volatile): don't force early wrap around, triggers bug in inteldrm.
pub static JIFFIES: AtomicU64 = AtomicU64::new(0);

/// \[I\] `hardclock_period`: hardclock period (ns).
pub static HARDCLOCK_PERIOD: AtomicU64 = AtomicU64::new(0);
/// \[I\] `statclock_avg`: average statclock period (ns).
pub static STATCLOCK_AVG: AtomicU64 = AtomicU64::new(0);
/// \[I\] `statclock_min`: minimum statclock period (ns).
pub static STATCLOCK_MIN: AtomicU64 = AtomicU64::new(0);
/// \[I\] `statclock_mask`: set of allowed offsets.
pub static STATCLOCK_MASK: AtomicU32 = AtomicU32::new(0);
/// \[I\] `statclock_is_randomized`: fixed or pseudorandom period?
pub static STATCLOCK_IS_RANDOMIZED: AtomicBool = AtomicBool::new(false);

/// `ticks`.
pub fn ticks() -> i32 {
    TICKS.load(Ordering::Relaxed)
}

/// `hardclock_period`.
pub fn hardclock_period() -> u64 {
    HARDCLOCK_PERIOD.load(Ordering::Relaxed)
}

/// `initclocks`: initialize clock frequencies and start both clocks running.
pub fn initclocks() {
    // Let the machine-specific code do its bit.
    cpu_initclocks();

    let hz = HZ.load(Ordering::Relaxed);
    kassert!((1..=1_000_000_000).contains(&hz));
    HARDCLOCK_PERIOD.store(1_000_000_000 / hz as u64, Ordering::Relaxed);
    ROUNDROBIN_PERIOD.store(hardclock_period() * 10, Ordering::Relaxed);

    let stathz = STATHZ.load(Ordering::Relaxed);
    kassert!((1..=1_000_000_000).contains(&stathz));

    // Compute the average statclock() period. Then find var, the largest 32-bit power of
    // two such that var <= statclock_avg / 2.
    let statclock_avg = 1_000_000_000 / stathz as u64;
    STATCLOCK_AVG.store(statclock_avg, Ordering::Relaxed);
    let half_avg = statclock_avg / 2;
    let mut var: u32 = 1 << 31;
    while u64::from(var) > half_avg {
        var /= 2;
    }

    // Set a lower bound for the range using statclock_avg and var. The mask for that range
    // is just (var - 1).
    STATCLOCK_MIN.store(statclock_avg - u64::from(var / 2), Ordering::Relaxed);
    STATCLOCK_MASK.store(var - 1, Ordering::Relaxed);

    let profhz = PROFHZ.load(Ordering::Relaxed);
    kassert!((stathz..=1_000_000_000).contains(&profhz));
    kassert!(profhz % stathz == 0);
    PROFCLOCK_PERIOD.store(1_000_000_000 / profhz as u64, Ordering::Relaxed);

    inittimecounter();

    // Start dispatching clock interrupts on the primary CPU.
    cpu_startclock();
}

/// `hardclock`: the real-time timer, interrupting hz times per second.
pub fn hardclock(_frame: *mut ClockFrame) {
    tc_ticktock();
    TICKS.fetch_add(1, Ordering::Relaxed);
    JIFFIES.fetch_add(1, Ordering::Relaxed);

    // Update the timeout wheel.
    timeout_hardclock_update();
}

/// `tvtohz`: compute number of hz in the specified amount of time.
pub fn tvtohz(tv: &Timeval) -> i32 {
    // If the number of usecs in the whole seconds part of the time fits in a long, then the
    // total number of usecs will fit in an unsigned long. Compute the total and convert it to
    // ticks, rounding up and adding 1 to allow for the current tick to expire. Rounding also
    // depends on unsigned long arithmetic to avoid overflow.
    //
    // Otherwise, if the number of ticks in the whole seconds part of the time fits in a
    // long, then convert the parts to ticks separately and add, using similar rounding
    // methods and overflow avoidance. This method would work in the previous case but it is
    // slightly slower and assumes that hz is integral.
    //
    // Otherwise, round the time down to the maximum representable value.
    //
    // If ints have 32 bits, then the maximum value for any timeout in 10ms ticks is 248 days.
    let sec = tv.tv_sec;
    let usec = tv.tv_usec as i64;
    let tick = TICK.load(Ordering::Relaxed) as u64;
    let hz = HZ.load(Ordering::Relaxed) as i64;
    let nticks: u64 = if sec < 0 || (sec == 0 && usec <= 0) {
        0
    } else if sec <= i64::MAX / 1_000_000 {
        (sec as u64)
            .wrapping_mul(1_000_000)
            .wrapping_add(usec as u64)
            .div_ceil(tick)
            + 1
    } else if sec <= i64::MAX / hz {
        (sec as u64).wrapping_mul(hz as u64) + (usec as u64).div_ceil(tick) + 1
    } else {
        i64::MAX as u64
    };
    nticks.min(i32::MAX as u64) as i32
}

/// `tstohz`.
pub fn tstohz(ts: &Timespec) -> i32 {
    let mut tv = timespec_to_timeval(ts);

    // Round up.
    if ts.tv_nsec % 1000 != 0 {
        tv.tv_usec += 1;
        if tv.tv_usec >= 1_000_000 {
            tv.tv_usec -= 1_000_000;
            tv.tv_sec += 1;
        }
    }

    tvtohz(&tv)
}

// startprofclock, stopprofclock: struct process (M5-b).

/// `statclock`: statistics clock. Grab profile sample, and if divider reaches 0, do process
/// and kernel statistics.
pub fn statclock(cr: &Clockrequest, cf: *mut c_void, _arg: *mut c_void) {
    let ci = curcpu();
    let spc = Machine::ci_schedstate(ci);
    let p = Machine::ci_curproc(ci);

    let count = if STATCLOCK_IS_RANDOMIZED.load(Ordering::Relaxed) {
        clockrequest_advance_random(
            cr,
            STATCLOCK_MIN.load(Ordering::Relaxed),
            STATCLOCK_MASK.load(Ordering::Relaxed),
        )
    } else {
        clockrequest_advance(cr, STATCLOCK_AVG.load(Ordering::Relaxed))
    };

    // SAFETY: the dispatcher passes the clock frame the interrupt entry built.
    let frame = unsafe { cf.cast::<ClockFrame>().as_ref() };
    // SAFETY: `ci_curproc` names the thread on this CPU, hence alive.
    let p = unsafe { p.as_ref() };
    let usermode = frame.is_some_and(Machine::clkf_usermode);
    let mut tu_tick: Option<usize> = None;
    let cp_time = if let (true, Some(p)) = (usermode, p) {
        let pr = p.process();
        // Came from user mode; CPU was in user state. If this process is being profiled
        // record the tick.
        tu_tick = Some(TU_UTICKS);
        if i32::from(pr.ps_nice.get()) > NZERO {
            CP_NICE
        } else {
            CP_USER
        }
    } else {
        // Came from kernel mode, so we were:
        // - spinning on a lock
        // - handling an interrupt,
        // - doing syscall or trap work on behalf of the current user process, or
        // - spinning in the idle loop.
        // Whichever it is, charge the time as appropriate. Note that we charge interrupts to
        // the current process, regardless of whether they are ``for'' that process, so that we
        // know how much of its real time was spent in ``non-process'' (i.e., interrupt) work.
        let mut cp_time = if frame.is_some_and(Machine::clkf_intr) {
            tu_tick = Some(TU_ITICKS);
            CP_INTR
        } else if p.is_some_and(|p| !ptr::eq(p, spc.spc_idleproc.get())) {
            tu_tick = Some(TU_STICKS);
            CP_SYS
        } else {
            CP_IDLE
        };

        if spc.spc_spinning.load(Ordering::Relaxed) != 0 {
            cp_time = CP_SPIN;
        }
        cp_time
    };

    let generation = pc_sprod_enter(&spc.spc_cp_time_lock);
    let t = &spc.spc_cp_time[cp_time]; // this CPU is the only writer
    t.store(t.load(Ordering::Relaxed) + count, Ordering::Relaxed);
    pc_sprod_leave(&spc.spc_cp_time_lock, generation);

    if let Some(p) = p {
        p.p_cpticks
            .set(p.p_cpticks.get().wrapping_add(count as u32));

        if p.p_flag.load(Ordering::Relaxed) & P_SYSTEM == 0
            && let Some(tu_tick) = tu_tick
        {
            let vm = p.vmspace();
            let tu = &p.p_tu;

            let generation = tu_enter(tu);
            tu.tu_ticks[tu_tick].set(tu.tu_ticks[tu_tick].get().wrapping_add(count));

            // maxrss is handled by uvm
            if tu_tick != TU_ITICKS {
                let kb = |pages: i32| ((pages as u64) << (PAGE_SHIFT - 10)).wrapping_mul(count);
                tu.tu_ixrss
                    .set(tu.tu_ixrss.get().wrapping_add(kb(vm.vm_tsize.get())));
                tu.tu_idrss
                    .set(tu.tu_idrss.get().wrapping_add(kb(vm.vm_dused.get())));
                tu.tu_isrss
                    .set(tu.tu_isrss.get().wrapping_add(kb(vm.vm_ssize.get())));
            }
            tu_leave(tu, generation);
        }

        // schedclock() runs every fourth statclock().
        for _ in 0..count {
            let ticks = spc.spc_schedticks.get().wrapping_add(1);
            spc.spc_schedticks.set(ticks);
            if ticks & 3 == 0 {
                schedclock(p);
            }
        }
    }
}

/// `sysctl_clockrate`: return information about system clocks (`kern.clockrate`).
pub fn sysctl_clockrate(where_: usize, sizep: &mut usize, newp: usize) -> Result<(), Errno> {
    // Construct clockinfo structure.
    let clkinfo = Clockinfo {
        tick: TICK.load(Ordering::Relaxed),
        hz: HZ.load(Ordering::Relaxed),
        profhz: PROFHZ.load(Ordering::Relaxed),
        stathz: STATHZ.load(Ordering::Relaxed),
    };
    sysctl_rdstruct(where_, sizep, newp, clkinfo.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tvtohz_rounds_up_and_adds_one() {
        // hz = 100, tick = 10000 us.
        assert_eq!(tvtohz(&Timeval::new(0, 0)), 0);
        assert_eq!(tvtohz(&Timeval::new(-1, 0)), 0);
        assert_eq!(tvtohz(&Timeval::new(0, 1)), 2);
        assert_eq!(tvtohz(&Timeval::new(0, 10_000)), 2);
        assert_eq!(tvtohz(&Timeval::new(0, 10_001)), 3);
        assert_eq!(tvtohz(&Timeval::new(1, 0)), 101);
        assert_eq!(tvtohz(&Timeval::new(i64::MAX, 0)), i32::MAX);
        assert_eq!(tstohz(&Timespec::new(0, 1)), 2);
        assert_eq!(tstohz(&Timespec::new(0, 10_000_001)), 3);
        // Rounds up to a whole second: 100 ticks plus the one for the current tick.
        assert_eq!(tstohz(&Timespec::new(0, 999_999_999)), 101);
    }
}
