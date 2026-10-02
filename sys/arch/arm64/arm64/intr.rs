/* $OpenBSD: intr.c,v 1.39 2026/03/09 06:38:02 tb Exp $ */
/*
 * Copyright (c) 2011 Dale Rahn <drahn@openbsd.org>
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

//! arm64 interrupt dispatch and the clock hooks: `arch/arm64/arm64/intr.c`.
//!
//! Upstream: sys/arch/arm64/arm64/intr.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports the clock function table and `delay(9)`
//! (`arm_clock_func`, `arm_clock_register`, `arm_dflt_delay`, `delay`, `cpu_initclocks`,
//! `cpu_startclock`, `setstatclockrate`); M4 adds the IRQ/FIQ entry from `exception.S`
//! (`arm_dflt_irq`, `arm_dflt_fiq`, `arm_irq_dispatch`, `arm_fiq_dispatch`, `arm_cpu_irq`,
//! `arm_cpu_fiq`). The interrupt controller hooks (`arm_intr_func`, `arm_intr_register_fdt`),
//! `splraise`/`spllower`, `arm_intr_establish` and friends arrive with M4-b; the generic timer
//! (`agtimer.c`) that replaces `arm_dflt_delay` attaches from `mainbus` (M4-b).
//!
//! ## Deviations
//! - `arm_clock_func` is a [`StaticCell`], written by `arm_clock_register` during
//!   autoconfiguration on the boot CPU.
//! - `arm_dflt_delay`'s inner loop spins on `yield` so the compiler keeps it; the C's empty
//!   loop body relies on the compiler not optimising it away.

use core::sync::atomic::Ordering;

use libkern::StaticCell;

use crate::arch::arm64::include::cpu::curcpu;
use crate::arch::arm64::include::frame::Trapframe;
use crate::kern::subr_prf::panic;
use crate::unported;
use crate::uvm::uvm_init::UVMEXP;

/// `arm_clock_func`: the clock driver's entry points, registered by the driver that attaches.
pub struct ArmClockFunc {
    /// `delay`: busy-wait for a number of microseconds.
    pub delay: fn(u32),
    /// `initclocks`: start the clock interrupts.
    pub initclocks: Option<fn()>,
    /// `setstatclockrate`: change the statistics clock rate.
    pub setstatclockrate: Option<fn(i32)>,
    /// `mpstartclock`: start the clock on a secondary CPU.
    pub mpstartclock: Option<fn()>,
}

/// `arm_clock_func`: starts with the calibration-free delay and no clock.
static ARM_CLOCK_FUNC: StaticCell<ArmClockFunc> = StaticCell::new(ArmClockFunc {
    delay: arm_dflt_delay,
    initclocks: None,
    setstatclockrate: None,
    mpstartclock: None,
});

/// The registered clock functions.
fn arm_clock_func() -> &'static ArmClockFunc {
    // SAFETY: written only by `arm_clock_register` during autoconfiguration on the boot CPU,
    // before interrupts exist; read afterwards.
    unsafe { ARM_CLOCK_FUNC.get() }
}

/// `arm_clock_register`: installs the clock driver's functions; the first registration wins.
pub fn arm_clock_register(
    initclock: Option<fn()>,
    delay: fn(u32),
    statclock: Option<fn(i32)>,
    mpstartclock: Option<fn()>,
) {
    if arm_clock_func().initclocks.is_some() {
        return;
    }
    // SAFETY: the single writer, on the boot CPU during autoconfiguration; no reference from
    // `arm_clock_func` is held across this call.
    unsafe {
        ARM_CLOCK_FUNC.write(ArmClockFunc {
            delay,
            initclocks: initclock,
            setstatclockrate: statclock,
            mpstartclock,
        });
    }
}

/// `delay(9)`: busy-waits `usec` microseconds through the registered clock.
pub fn delay(usec: u32) {
    (arm_clock_func().delay)(usec)
}

/// `cpu_initclocks`: starts the clock interrupts.
pub fn cpu_initclocks() {
    match arm_clock_func().initclocks {
        Some(initclocks) => initclocks(),
        None => {
            let _ = unported!("initclocks function not initialized yet");
        }
    }
}

/// `cpu_startclock`: starts the clock on this (secondary) CPU.
pub fn cpu_startclock() {
    match arm_clock_func().mpstartclock {
        Some(mpstartclock) => mpstartclock(),
        None => {
            let _ = unported!("startclock function not initialized yet");
        }
    }
}

/// `setstatclockrate`: changes the statistics clock rate.
pub fn setstatclockrate(new: i32) {
    match arm_clock_func().setstatclockrate {
        Some(setstatclockrate) => setstatclockrate(new),
        None => {
            let _ = unported!("arm_clock_func.setstatclockrate not initialized");
        }
    }
}

/// `arm_dflt_delay`: BAH - there is no good way to make this close, but this isn't supposed to
/// be used after the real clock attaches.
pub fn arm_dflt_delay(usecs: u32) {
    for _ in 0..usecs {
        for _ in 0..100 {
            core::hint::spin_loop();
        }
    }
}

/// `arm_dflt_irq`: the IRQ dispatcher before an interrupt controller registers one.
pub fn arm_dflt_irq(_frame: &mut Trapframe) {
    panic(format_args!("arm_dflt_irq"));
}

/// `arm_dflt_fiq`: the FIQ dispatcher before an interrupt controller registers one.
pub fn arm_dflt_fiq(_frame: &mut Trapframe) {
    panic(format_args!("arm_dflt_fiq"));
}

/// `arm_irq_dispatch`: where `arm_cpu_irq` sends an IRQ; set by `arm_intr_register_fdt`
/// (M4-b) during autoconfiguration on the boot CPU.
pub static ARM_IRQ_DISPATCH: StaticCell<fn(&mut Trapframe)> = StaticCell::new(arm_dflt_irq);

/// `arm_fiq_dispatch`: as `arm_irq_dispatch`, for FIQs.
pub static ARM_FIQ_DISPATCH: StaticCell<fn(&mut Trapframe)> = StaticCell::new(arm_dflt_fiq);

/// `arm_cpu_irq`: the IRQ entry, called from `handle_el1h_irq` (`exception.S`).
#[unsafe(no_mangle)]
pub extern "C" fn arm_cpu_irq(frame: &mut Trapframe) {
    let ci = curcpu();

    UVMEXP.intrs.fetch_add(1, Ordering::Relaxed);
    ci.ci_idepth.set(ci.ci_idepth.get() + 1);
    // SAFETY: written once during autoconfiguration, read on every interrupt afterwards.
    let dispatch = unsafe { ARM_IRQ_DISPATCH.read() };
    dispatch(frame);
    ci.ci_idepth.set(ci.ci_idepth.get() - 1);
}

/// `arm_cpu_fiq`: the FIQ entry, called from `handle_el1h_fiq` (`exception.S`).
#[unsafe(no_mangle)]
pub extern "C" fn arm_cpu_fiq(frame: &mut Trapframe) {
    let ci = curcpu();

    UVMEXP.intrs.fetch_add(1, Ordering::Relaxed);
    ci.ci_idepth.set(ci.ci_idepth.get() + 1);
    // SAFETY: as for `arm_cpu_irq`.
    let dispatch = unsafe { ARM_FIQ_DISPATCH.read() };
    dispatch(frame);
    ci.ci_idepth.set(ci.ci_idepth.get() - 1);
}
