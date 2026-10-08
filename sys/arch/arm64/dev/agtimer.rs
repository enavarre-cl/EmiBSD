/* $OpenBSD: agtimer.c,v 1.30 2026/05/04 20:43:41 kettenis Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2011 Dale Rahn <drahn@openbsd.org>
 * Copyright (c) 2013 Patrick Wildt <patrick@blueri.se>
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
/* </LICENSES> */

/* <CODE> */
//! The ARM generic timer: `arch/arm64/dev/agtimer.c`. The virtual counter (`CNTVCT_EL0`) is
//! the timecounter and the virtual timer (`CNTV_TVAL_EL0`) the interrupt clock, through the
//! PPI the device tree names.
//!
//! Upstream: sys/arch/arm64/dev/agtimer.c @ 3ce1f3f79392
//!
//! Status: `ported`. Milestone M5 ports the whole driver: `agtimer_readcnt64_{default,sun50i}`,
//! the register accessors, `agtimer_match`, `agtimer_attach`, `agtimer_get_timecount_*`,
//! `agtimer_rearm`, `agtimer_trigger`, `agtimer_intr`, `agtimer_set_clockrate`,
//! `agtimer_cpu_initclocks`, `agtimer_delay`, `agtimer_setstatclockrate`,
//! `agtimer_startclock` and `agtimer_init`. M11b checks it on every CPU: each application
//! processor routes the timer's PPI to itself in `agtimer_startclock` and runs its own
//! clock interrupt queue.
//!
//! ## Deviations
//! - One static softc (`AGTIMER`, the C's `agtimer_cd.cd_devs[0]`) holds what
//!   `struct agtimer_softc` adds to the device, so `agtimer_ca`'s `ca_devsize` is a bare
//!   `struct device`; mainbus attaches it from the device tree (`agtimer* at fdt?`).
//!   `agtimer_intrclock`'s cookie stays null and the callbacks read the softc directly.
//! - Not in the C: `agtimer_evtstrm_enable` turns on the timer's event stream
//!   (`CNTKCTL_EL1.EVNTEN`, counter bit 12 at QEMU's 62.5 MHz: a `wfe` wake-up every
//!   131 µs). Each application processor calls it first thing (`cpu_hatch_entry`), and
//!   `agtimer_startclock` calls it again after the C's `CNTKCTL_EL1` write, which clears it.
//!   QEMU's TCG can lose the `sev` that ends a `wfe` wait (its `sev` helper kicks the halted
//!   vCPU without the BQL, racing the vCPU thread's idle check); with no interrupt pending,
//!   the processor then sleeps forever. The event stream's wake-up comes from a QEMU timer
//!   and is reliable, so every `wfe` loop goes on checking its condition. On hardware it
//!   only costs a few spurious `wfe` returns; Linux keeps it on for the same reason.

use core::arch::asm;
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use libkern::StaticCell;

use crate::arch::arm64::arm64::intr::{
    arm_clock_register, arm_intr_establish_fdt_idx, arm_intr_route,
};
use crate::arch::arm64::include::armreg::{
    CNTKCTL_EL0VCTEN, CNTKCTL_EVNTDIR, CNTKCTL_EVNTEN, CNTKCTL_EVNTI_MASK, CNTKCTL_EVNTI_SHIFT,
    CNTV_CTL_ENABLE, CNTV_CTL_IMASK, CURRENTEL_EL_EL2, CURRENTEL_EL_MASK, read_specialreg,
    write_specialreg,
};
use crate::arch::arm64::include::cpu::{cpu_is_primary, curcpu};
use crate::arch::arm64::include::fdt::FdtAttachArgs;
use crate::arch::arm64::include::intr::{IPL_CLOCK, IPL_MPSAFE, MachineIntrHandle};
use crate::arch::arm64::include::timetc::{TC_AGTIMER, TC_AGTIMER_SUN50I};
use crate::dev::ofw::openfirm::{OF_getindex, OF_getpropbool, OF_getpropint, OF_is_compatible};
use crate::kassert;
use crate::kern::kern_clock::{PROFHZ, STATCLOCK_IS_RANDOMIZED, STATHZ};
use crate::kern::kern_clockintr::{clockintr_cpu_init, clockintr_dispatch, clockintr_trigger};
use crate::kern::kern_tc::tc_init;
use crate::kprintf;
use crate::sys::clockintr::Intrclock;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device};
use crate::sys::timetc::Timecounter;

/// `TIMER_FREQUENCY`: ARM core clock.
const TIMER_FREQUENCY: i32 = 24 * 1000 * 1000;

/// `agtimer_frequency`.
pub static AGTIMER_FREQUENCY: AtomicI32 = AtomicI32::new(TIMER_FREQUENCY);

/// `agtimer_timecounter`.
static AGTIMER_TIMECOUNTER: Timecounter = Timecounter::new(
    agtimer_get_timecount_default,
    0xffff_ffff,
    0,
    "agtimer",
    0,
    TC_AGTIMER,
);

/// `struct agtimer_softc`.
pub struct AgtimerSoftc {
    /// `sc_node`.
    pub sc_node: Cell<i32>,
    /// `sc_ticks_per_second`.
    pub sc_ticks_per_second: Cell<u32>,
    /// `sc_nsec_cycle_ratio`.
    pub sc_nsec_cycle_ratio: Cell<u64>,
    /// `sc_nsec_max`.
    pub sc_nsec_max: Cell<u64>,
    /// `sc_ih`.
    pub sc_ih: Cell<Option<NonNull<MachineIntrHandle>>>,
    /// Whether `agtimer_attach` ran (`agtimer_cd.cd_devs[0] != NULL`).
    pub sc_attached: AtomicBool,
}

// SAFETY: written at attach on the boot CPU; read by the clock paths afterwards.
unsafe impl Sync for AgtimerSoftc {}

/// `agtimer_ca`.
pub static AGTIMER_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<Device>(),
    ca_match: Some(agtimer_match),
    ca_attach: agtimer_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `agtimer_cd`.
pub static AGTIMER_CD: Cfdriver = Cfdriver::new(b"agtimer", DV_DULL, 0);

/// `agtimer_cd.cd_devs[0]`: the one generic timer.
static AGTIMER: AgtimerSoftc = AgtimerSoftc {
    sc_node: Cell::new(0),
    sc_ticks_per_second: Cell::new(0),
    sc_nsec_cycle_ratio: Cell::new(0),
    sc_nsec_max: Cell::new(0),
    sc_ih: Cell::new(None),
    sc_attached: AtomicBool::new(false),
};

/// `agtimer_intrclock`.
pub const AGTIMER_INTRCLOCK: Intrclock = Intrclock {
    ic_cookie: ptr::null_mut(),
    ic_rearm: agtimer_rearm,
    ic_trigger: agtimer_trigger,
};

/// `agtimer_readcnt64_default`: reads `CNTVCT_EL0`.
pub fn agtimer_readcnt64_default() -> u64 {
    let (val0, val1): (u64, u64);
    // Work around Cortex-A73 errata 858921, where there is a one-cycle window where the read
    // might return the old value for the low 32 bits and the new value for the high 32 bits
    // upon roll-over of the low 32 bits.
    // SAFETY: an `isb` and two counter reads; no memory is touched (the `memory` clobber of
    // the C keeps the compiler from moving loads across, so no `nomem` here).
    unsafe {
        asm!(
            "isb",
            "mrs {0}, cntvct_el0",
            "mrs {1}, cntvct_el0",
            out(reg) val0,
            out(reg) val1,
            options(nostack, preserves_flags)
        )
    };
    if (val0 ^ val1) & 0x1_0000_0000 != 0 {
        val0
    } else {
        val1
    }
}

/// `agtimer_readcnt64_sun50i`: reads the counter around the Allwinner A64 erratum.
pub fn agtimer_readcnt64_sun50i() -> u64 {
    let mut val: u64 = 0;
    let mut retry = 0;
    // SAFETY: as for `agtimer_readcnt64_default`.
    unsafe { asm!("isb", options(nostack, preserves_flags)) };
    while retry < 150 {
        // SAFETY: a counter read with no side effects.
        unsafe {
            asm!("mrs {}, cntvct_el0", out(reg) val, options(nomem, nostack, preserves_flags))
        };
        if ((val + 1) & 0x1ff) > 1 {
            break;
        }
        retry += 1;
    }
    kassert!(retry < 150);

    val
}

/// `agtimer_readcnt64`: the counter reader in use.
static AGTIMER_READCNT64: StaticCell<fn() -> u64> = StaticCell::new(agtimer_readcnt64_default);

/// `agtimer_readcnt64()`.
pub fn agtimer_readcnt64() -> u64 {
    // SAFETY: written only by `agtimer_attach` on the boot CPU, before the clocks start.
    (unsafe { AGTIMER_READCNT64.read() })()
}

/// `agtimer_get_freq`: `CNTFRQ_EL0`.
#[inline]
fn agtimer_get_freq() -> u64 {
    read_specialreg!("cntfrq_el0")
}

/// `agtimer_get_ctrl`: `CNTV_CTL_EL0`.
#[inline]
fn agtimer_get_ctrl() -> u32 {
    read_specialreg!("cntv_ctl_el0") as u32
}

/// `agtimer_set_ctrl`: writes `CNTV_CTL_EL0`.
#[inline]
fn agtimer_set_ctrl(val: u32) {
    // SAFETY: the virtual timer's control register is the kernel's to program; `isb` makes
    // the write take effect before the next instruction.
    unsafe {
        write_specialreg!("cntv_ctl_el0", u64::from(val));
        asm!("isb", options(nostack, preserves_flags));
    }
}

/// `agtimer_set_tval`: writes `CNTV_TVAL_EL0`.
#[inline]
fn agtimer_set_tval(val: u32) {
    // SAFETY: as for `agtimer_set_ctrl`.
    unsafe {
        write_specialreg!("cntv_tval_el0", u64::from(val));
        asm!("isb", options(nostack, preserves_flags));
    }
}

/// `agtimer_match`.
pub fn agtimer_match(_parent: Option<&Device>, _cfdata: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `agtimer` attaches at `fdt`, whose buses hand over a `FdtAttachArgs`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };
    i32::from(
        OF_is_compatible(faa.fa_node, b"arm,armv7-timer")
            || OF_is_compatible(faa.fa_node, b"arm,armv8-timer"),
    )
}

/// `agtimer_attach`.
pub fn agtimer_attach(_parent: Option<&Device>, _self: &Device, aux: *mut c_void) {
    let sc = &AGTIMER;
    // SAFETY: as in `agtimer_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    sc.sc_node.set(faa.fa_node);

    if agtimer_get_freq() != 0 {
        AGTIMER_FREQUENCY.store(agtimer_get_freq() as i32, Ordering::Relaxed);
    }
    AGTIMER_FREQUENCY.store(
        OF_getpropint(
            sc.sc_node.get(),
            b"clock-frequency",
            AGTIMER_FREQUENCY.load(Ordering::Relaxed) as u32,
        ) as i32,
        Ordering::Relaxed,
    );
    sc.sc_ticks_per_second
        .set(AGTIMER_FREQUENCY.load(Ordering::Relaxed) as u32);
    sc.sc_nsec_cycle_ratio
        .set(u64::from(sc.sc_ticks_per_second.get()) * (1u64 << 32) / 1_000_000_000);
    sc.sc_nsec_max.set(u64::MAX / sc.sc_nsec_cycle_ratio.get());

    kprintf!(": {} kHz\n", sc.sc_ticks_per_second.get() / 1000);

    // The Allwinner A64 has an erratum where the bottom 9 bits of the counter register can't
    // be trusted if any of the higher bits are rolling over.
    if OF_getpropbool(sc.sc_node.get(), b"allwinner,erratum-unknown1") {
        // SAFETY: once, at attach on the boot CPU, before the clocks start.
        unsafe { AGTIMER_READCNT64.write(agtimer_readcnt64_sun50i) };
        AGTIMER_TIMECOUNTER
            .tc_get_timecount
            .set(agtimer_get_timecount_sun50i);
        AGTIMER_TIMECOUNTER.tc_user.set(TC_AGTIMER_SUN50I);
    }

    // private timer and interrupts not enabled until timer configures

    arm_clock_register(
        Some(agtimer_cpu_initclocks),
        agtimer_delay,
        Some(agtimer_setstatclockrate),
        Some(agtimer_startclock),
    );

    AGTIMER_TIMECOUNTER
        .tc_frequency
        .set(u64::from(sc.sc_ticks_per_second.get()));
    AGTIMER_TIMECOUNTER
        .tc_priv
        .set(ptr::from_ref(sc).cast::<()>());
    tc_init(&AGTIMER_TIMECOUNTER);

    // agtimer_intrclock.ic_cookie = sc: the callbacks read AGTIMER (see the deviations).
    sc.sc_attached.store(true, Ordering::Relaxed);
}

/// `agtimer_get_timecount_default`: the low 32 bits of the counter.
pub fn agtimer_get_timecount_default(_tc: &Timecounter) -> u32 {
    let val: u64;
    // No need to work around Cortex-A73 errata 858921 since we only look at the low 32 bits
    // here.
    // SAFETY: as for `agtimer_readcnt64_default`.
    unsafe {
        asm!(
            "isb",
            "mrs {}, cntvct_el0",
            out(reg) val,
            options(nostack, preserves_flags)
        )
    };
    val as u32
}

/// `agtimer_get_timecount_sun50i`.
pub fn agtimer_get_timecount_sun50i(_tc: &Timecounter) -> u32 {
    agtimer_readcnt64_sun50i() as u32
}

/// `agtimer_rearm`: fires the timer in `nsecs`.
pub fn agtimer_rearm(_cookie: *mut c_void, nsecs: u64) {
    let sc = &AGTIMER;

    let nsecs = nsecs.min(sc.sc_nsec_max.get());
    let cycles = (nsecs.wrapping_mul(sc.sc_nsec_cycle_ratio.get()) >> 32).min(i32::MAX as u64);
    agtimer_set_tval(cycles as u32);
}

/// `agtimer_trigger`: fires the timer now.
pub fn agtimer_trigger(_unused: *mut c_void) {
    agtimer_set_tval(0);
}

/// `agtimer_intr`: the timer's interrupt handler.
pub fn agtimer_intr(frame: *mut c_void) -> i32 {
    clockintr_dispatch(frame)
}

/// `agtimer_set_clockrate`: a new counter frequency.
pub fn agtimer_set_clockrate(new_frequency: i32) {
    let sc = &AGTIMER;

    AGTIMER_FREQUENCY.store(new_frequency, Ordering::Relaxed);

    if !sc.sc_attached.load(Ordering::Relaxed) {
        return;
    }

    sc.sc_ticks_per_second
        .set(AGTIMER_FREQUENCY.load(Ordering::Relaxed) as u32);
    sc.sc_nsec_cycle_ratio
        .set(u64::from(sc.sc_ticks_per_second.get()) * (1u64 << 32) / 1_000_000_000);
    sc.sc_nsec_max.set(u64::MAX / sc.sc_nsec_cycle_ratio.get());

    AGTIMER_TIMECOUNTER
        .tc_frequency
        .set(u64::from(sc.sc_ticks_per_second.get()));

    kprintf!(
        "agtimer0: adjusting clock: new tick rate {} kHz\n",
        sc.sc_ticks_per_second.get() / 1000
    );
}

/// `agtimer_cpu_initclocks`: `cpu_initclocks` on the generic timer.
pub fn agtimer_cpu_initclocks() {
    let sc = &AGTIMER;

    let hz = crate::conf::param::HZ.load(Ordering::Relaxed);
    STATHZ.store(hz, Ordering::Relaxed);
    PROFHZ.store(hz * 10, Ordering::Relaxed);
    STATCLOCK_IS_RANDOMIZED.store(true, Ordering::Relaxed);

    if sc.sc_ticks_per_second.get() != AGTIMER_FREQUENCY.load(Ordering::Relaxed) as u32 {
        agtimer_set_clockrate(AGTIMER_FREQUENCY.load(Ordering::Relaxed));
    }

    // Pick the correct PPI depending on the running EL.
    let el = read_specialreg!("CurrentEL") & CURRENTEL_EL_MASK;
    let mut idx = if el == CURRENTEL_EL_EL2 {
        OF_getindex(sc.sc_node.get(), Some(b"hyp-virt"), b"interrupt-names")
    } else {
        OF_getindex(sc.sc_node.get(), Some(b"virt"), b"interrupt-names")
    };
    if idx == -1 {
        idx = if el == CURRENTEL_EL_EL2 { 4 } else { 2 };
    }

    // configure virtual timer interrupt
    sc.sc_ih.set(arm_intr_establish_fdt_idx(
        sc.sc_node.get(),
        idx as usize,
        IPL_CLOCK | IPL_MPSAFE,
        agtimer_intr,
        ptr::null_mut(),
        "tick",
    ));
}

/// `agtimer_delay`: busy-waits `usecs` on the counter.
pub fn agtimer_delay(usecs: u32) {
    let start = agtimer_readcnt64();
    let cycles = u64::from(usecs) * AGTIMER_FREQUENCY.load(Ordering::Relaxed) as u64 / 1_000_000;
    while agtimer_readcnt64().wrapping_sub(start) < cycles {
        core::hint::spin_loop(); // CPU_BUSY_CYCLE()
    }
}

/// `agtimer_setstatclockrate`: the statclock shares the timer; nothing to change.
pub fn agtimer_setstatclockrate(_newhz: i32) {}

/// `agtimer_startclock`: starts the clock interrupts on this CPU.
pub fn agtimer_startclock() {
    let sc = &AGTIMER;

    if !cpu_is_primary(curcpu())
        && let Some(ih) = sc.sc_ih.get()
    {
        arm_intr_route(ih, true, curcpu());
    }

    clockintr_cpu_init(Some(&AGTIMER_INTRCLOCK));

    let mut reg = agtimer_get_ctrl();
    reg &= !CNTV_CTL_IMASK;
    reg |= CNTV_CTL_ENABLE;
    agtimer_set_tval(i32::MAX as u32);
    agtimer_set_ctrl(reg);

    clockintr_trigger();

    // enable userland access to virtual counter
    // SAFETY: CNTKCTL_EL1 is the kernel's; granting EL0 the virtual counter is the C's intent.
    unsafe { write_specialreg!("cntkctl_el1", CNTKCTL_EL0VCTEN) };
    // Not in the C: the write above clears the event stream; keep it on (see the deviations).
    agtimer_evtstrm_enable();
}

/// `agtimer_init`: what `mainbus` does before the children attach: the counter's frequency
/// and `agtimer_delay` as `delay(9)`.
pub fn agtimer_init() {
    // XXX: Check for Generic Timer support.
    let cntfrq = agtimer_get_freq();

    if cntfrq != 0 {
        AGTIMER_FREQUENCY.store(cntfrq as i32, Ordering::Relaxed);
        arm_clock_register(None, agtimer_delay, None, None);
    }
}

/// Not in the C: turns on this CPU's event stream, a `wfe` wake-up about every 100 µs, so
/// that a `wfe` loop never depends on one `sev` alone (see the module's deviations). Reads
/// the counter frequency itself: an application processor calls it from its first kernel
/// instructions, before `agtimer_attach` may have run.
pub fn agtimer_evtstrm_enable() {
    let reg = read_specialreg!("cntkctl_el1") & !(CNTKCTL_EVNTI_MASK | CNTKCTL_EVNTDIR);
    let reg = reg | (agtimer_evtstrm_evnti(agtimer_get_freq()) << CNTKCTL_EVNTI_SHIFT);
    // SAFETY: CNTKCTL_EL1 is the kernel's; the event stream only ends `wfe` waits early,
    // and every `wfe` in the kernel sits in a loop that checks its condition again.
    unsafe { write_specialreg!("cntkctl_el1", reg | CNTKCTL_EVNTEN) };
}

/// The event stream's trigger bit for a counter of `freq` Hz: the lowest counter bit whose
/// 0-to-1 transitions (every `2 << n` ticks) are at least 100 µs apart.
const fn agtimer_evtstrm_evnti(freq: u64) -> u64 {
    let ticks = freq / 10_000;
    let mut n = 0;
    while n < 15 && (2u64 << n) < ticks {
        n += 1;
    }
    n
}

// QEMU's 62.5 MHz counter: bit 12, an event every 131 µs; 24 MHz: bit 11, every 171 µs.
const _: () = {
    assert!(agtimer_evtstrm_evnti(62_500_000) == 12);
    assert!(agtimer_evtstrm_evnti(24_000_000) == 11);
    assert!(agtimer_evtstrm_evnti(0) == 0);
    assert!(agtimer_evtstrm_evnti(u64::MAX) == 15);
};
/* </CODE> */
