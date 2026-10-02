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
//! `arm_cpu_fiq`), the `spl` machinery (`arm_smask`, `arm_intr_func` with the `arm_dflt_*`
//! functions, `arm_do_pending_intr`, `arm_set_intr_handler`, `arm_init_smask`, `splraise`,
//! `spllower`, `splx`, `softintr`, `arm_splassert_check`) and the wakeup hooks. The device
//! tree registration (`arm_intr_register_fdt`, `arm_intr_establish_fdt*`, the
//! pre-registration, `arm_intr_map_msi`) and `intr_barrier` come with the interrupt
//! controller (M4-b, part 2); the generic timer (`agtimer.c`) that replaces `arm_dflt_delay`
//! attaches from `mainbus` (M5).
//!
//! ## Deviations
//! - `arm_clock_func` is a [`StaticCell`], written by `arm_clock_register` during
//!   autoconfiguration on the boot CPU.
//! - `arm_dflt_delay`'s inner loop spins on `yield` so the compiler keeps it; the C's empty
//!   loop body relies on the compiler not optimising it away.
//! - `arm_intr_func` and `arm_smask` are `StaticCell`s written by the controller's attach
//!   (`arm_set_intr_handler`, `arm_init_smask`) on the boot CPU before interrupts are enabled.

use core::sync::atomic::Ordering;

use libkern::StaticCell;

use crate::arch::arm64::include::cpu::{curcpu, disable_irq_daif_ret, restore_daif};
use crate::arch::arm64::include::frame::Trapframe;
use crate::arch::arm64::include::intr::{
    ArmIntrFunc, IPL_HIGH, IPL_NONE, IPL_SOFTCLOCK, IPL_SOFTNET, IPL_SOFTTTY, NIPL,
};
use crate::kern::kern_softintr::softintr_dispatch;
use crate::kern::subr_prf::{panic, splassert_fail};
use crate::sys::softintr::{SOFTINTR_CLOCK, SOFTINTR_NET, SOFTINTR_TTY};
use crate::unported;
use crate::uvm::uvm_init::UVMEXP;

/// `SI_TO_IRQBIT(x)`: the `ci_ipending` bit of soft interrupt `x`.
const fn si_to_irqbit(x: i32) -> u32 {
    1 << x
}

/// `arm_smask[NIPL]`: the soft interrupts each level leaves unmasked.
static ARM_SMASK: StaticCell<[u32; NIPL]> = StaticCell::new([0; NIPL]);

/// `arm_intr_func`: the controller's `spl` functions, the defaults until one attaches.
static ARM_INTR_FUNC: StaticCell<ArmIntrFunc> = StaticCell::new(ArmIntrFunc {
    raise: arm_dflt_splraise,
    lower: arm_dflt_spllower,
    x: arm_dflt_splx,
    setipl: arm_dflt_setipl,
    enable_wakeup: None,
    disable_wakeup: None,
});

/// The registered `spl` functions.
fn arm_intr_func() -> &'static ArmIntrFunc {
    // SAFETY: written only by `arm_set_intr_handler` during autoconfiguration on the boot
    // CPU, before interrupts are enabled; read afterwards.
    unsafe { ARM_INTR_FUNC.get() }
}

/// `arm_smask[level]`.
fn arm_smask(level: i32) -> u32 {
    // SAFETY: written only by `arm_init_smask` on the boot CPU before interrupts are
    // enabled; read afterwards.
    unsafe { ARM_SMASK.get()[level as usize] }
}

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

/// `arm_dflt_splraise`: raise the level in `ci_cpl`.
pub fn arm_dflt_splraise(newcpl: i32) -> i32 {
    let ci = curcpu();
    let oldcpl = ci.ci_cpl.get() as i32;
    ci.ci_cpl.set(newcpl.max(oldcpl) as u32);
    oldcpl
}

/// `arm_dflt_spllower`: lower the level, running what becomes unmasked.
pub fn arm_dflt_spllower(newcpl: i32) -> i32 {
    let ci = curcpu();
    let oldcpl = ci.ci_cpl.get() as i32;
    splx(newcpl);
    oldcpl
}

/// `arm_dflt_splx`: restore the level, running the pending soft interrupts it unmasks.
pub fn arm_dflt_splx(newcpl: i32) {
    let ci = curcpu();
    if ci.ci_ipending.get() & arm_smask(newcpl) != 0 {
        arm_do_pending_intr(newcpl);
    }
    ci.ci_cpl.set(newcpl as u32);
}

/// `arm_dflt_setipl`: set the level.
pub fn arm_dflt_setipl(newcpl: i32) {
    curcpu().ci_cpl.set(newcpl as u32);
}

/// `arm_do_pending_intr`: runs the soft interrupts pending above `pcpl`, highest first.
pub fn arm_do_pending_intr(pcpl: i32) {
    let ci = curcpu();

    let mut oldirqstate = disable_irq_daif_ret();

    let mut do_softint = |si: i32, ipl: i32, ipending: u32| {
        if ipending & si_to_irqbit(si) != 0 {
            ci.ci_ipending.set(ci.ci_ipending.get() & !si_to_irqbit(si));
            (arm_intr_func().setipl)(ipl);
            // SAFETY: `oldirqstate` is this CPU's DAIF from `disable_irq_daif_ret`.
            unsafe { restore_daif(oldirqstate) };
            softintr_dispatch(si);
            oldirqstate = disable_irq_daif_ret();
        }
    };

    loop {
        let ipending = ci.ci_ipending.get() & arm_smask(pcpl);
        do_softint(SOFTINTR_TTY, IPL_SOFTTTY, ipending);
        do_softint(SOFTINTR_NET, IPL_SOFTNET, ipending);
        do_softint(SOFTINTR_CLOCK, IPL_SOFTCLOCK, ipending);
        // MULTIPROCESSOR + NXCALL: SOFTINTR_XCALL (M5).
        if ci.ci_ipending.get() & arm_smask(pcpl) == 0 {
            break;
        }
    }

    // Don't use splx... we are here already!
    (arm_intr_func().setipl)(pcpl);
    // SAFETY: as above.
    unsafe { restore_daif(oldirqstate) };
}

/// `arm_set_intr_handler`: the interrupt controller registers its `spl` functions and
/// dispatchers.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn arm_set_intr_handler(
    raise: fn(i32) -> i32,
    lower: fn(i32) -> i32,
    x: fn(i32),
    setipl: fn(i32),
    irq_dispatch: Option<fn(&mut Trapframe)>,
    fiq_dispatch: Option<fn(&mut Trapframe)>,
    enable_wakeup: Option<fn()>,
    disable_wakeup: Option<fn()>,
) {
    // SAFETY: the controller attaches once, on the boot CPU, before interrupts are enabled.
    unsafe {
        ARM_INTR_FUNC.write(ArmIntrFunc {
            raise,
            lower,
            x,
            setipl,
            enable_wakeup,
            disable_wakeup,
        });
        if let Some(irq) = irq_dispatch {
            ARM_IRQ_DISPATCH.write(irq);
        }
        if let Some(fiq) = fiq_dispatch {
            ARM_FIQ_DISPATCH.write(fiq);
        }
    }
}

/// `arm_init_smask`: which soft interrupts each level leaves unmasked, once.
pub fn arm_init_smask() {
    static INITED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

    if INITED.swap(true, Ordering::Relaxed) {
        return;
    }
    // SAFETY: once, on the boot CPU, before interrupts are enabled.
    let smask = unsafe { ARM_SMASK.get_mut() };
    for (i, mask) in smask.iter_mut().enumerate().take(IPL_HIGH as usize + 1) {
        let i = i as i32;
        *mask = 0;
        if i < IPL_SOFTCLOCK {
            *mask |= si_to_irqbit(SOFTINTR_CLOCK);
            // MULTIPROCESSOR + NXCALL: SOFTINTR_XCALL (M5).
        }
        if i < IPL_SOFTNET {
            *mask |= si_to_irqbit(SOFTINTR_NET);
        }
        if i < IPL_SOFTTTY {
            *mask |= si_to_irqbit(SOFTINTR_TTY);
        }
    }
    let _ = IPL_NONE;
}

/// `splraise`: through `arm_intr_func`.
pub fn splraise(ipl: i32) -> i32 {
    (arm_intr_func().raise)(ipl)
}

/// `spllower`: through `arm_intr_func`.
pub fn spllower(ipl: i32) -> i32 {
    (arm_intr_func().lower)(ipl)
}

/// `splx`: through `arm_intr_func`.
pub fn splx(ipl: i32) {
    (arm_intr_func().x)(ipl)
}

/// `softintr`: marks a soft interrupt pending on this CPU.
pub fn softintr(si: i32) {
    let ci = curcpu();
    ci.ci_ipending.set(ci.ci_ipending.get() | si_to_irqbit(si));
}

/// `arm_splassert_check`: the `DIAGNOSTIC` level check behind `splassert`.
pub fn arm_splassert_check(wantipl: i32, func: &str) {
    let oldipl = curcpu().ci_cpl.get() as i32;

    if oldipl < wantipl {
        splassert_fail(wantipl, oldipl, func);
        // If the splassert_ctl is set to not panic, raise the ipl in a feeble attempt to
        // reduce damage.
        (arm_intr_func().setipl)(wantipl);
    }

    if wantipl == IPL_NONE && curcpu().ci_idepth.get() != 0 {
        splassert_fail(-1, curcpu().ci_idepth.get() as i32, func);
    }
}

/// `intr_enable_wakeup`.
pub fn intr_enable_wakeup() {
    if let Some(f) = arm_intr_func().enable_wakeup {
        f();
    }
}

/// `intr_disable_wakeup`.
pub fn intr_disable_wakeup() {
    if let Some(f) = arm_intr_func().disable_wakeup {
        f();
    }
}
