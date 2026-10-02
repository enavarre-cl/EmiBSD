/* $OpenBSD: intr.c,v 1.39 2026/03/09 06:38:02 tb Exp $ */
/* <LICENSES> */
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
/* </LICENSES> */

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
//! `spllower`, `splx`, `softintr`, `arm_splassert_check`), the wakeup hooks and the device
//! tree registration (`arm_intr_get_parent`, the pre-registration `arm_intr_prereg_*` and
//! `arm_intr_init_fdt`, `arm_intr_register_fdt`, `arm_intr_establish_fdt`/`_cpu`/`_idx`/
//! `_idx_cpu`, `arm_intr_disestablish_fdt`, `arm_intr_enable`/`disable`,
//! `arm_intr_parent_establish_fdt`/`_disestablish_fdt`, `arm_intr_route`,
//! `arm_intr_cpu_enable`, `intr_barrier`, `intr_set_wakeup`). The `imap` and `msi` variants
//! and `arm_intr_map_msi` come with PCI (M5), the IPIs with `MULTIPROCESSOR`; the generic
//! timer (`agtimer.c`) that replaces `arm_dflt_delay` attaches from `mainbus` (M5).
//!
//! ## Deviations
//! - `arm_clock_func` is a [`StaticCell`], written by `arm_clock_register` during
//!   autoconfiguration on the boot CPU.
//! - `arm_dflt_delay`'s inner loop spins on `yield` so the compiler keeps it; the C's empty
//!   loop body relies on the compiler not optimising it away.
//! - `arm_intr_func` and `arm_smask` are `StaticCell`s written by the controller's attach
//!   (`arm_set_intr_handler`, `arm_init_smask`) on the boot CPU before interrupts are enabled.
//! - The controller and pre-registration lists are `LIST`s behind a `Sync` wrapper, touched
//!   at attach time on the boot CPU; the handles are `NonNull<MachineIntrHandle>`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use libkern::StaticCell;

use crate::arch::arm64::include::cpu::{CpuInfo, curcpu, disable_irq_daif_ret, restore_daif};
use crate::arch::arm64::include::frame::Trapframe;
use crate::arch::arm64::include::intr::{
    ArmIntrFunc, IPL_HIGH, IPL_NONE, IPL_SOFTCLOCK, IPL_SOFTNET, IPL_SOFTTTY, IPL_WAKEUP,
    InterruptController, IntrFn, MachineIntrHandle, NIPL,
};
use crate::dev::ofw::openfirm::{
    OF_child, OF_getnodebyphandle, OF_getpropbool, OF_getpropint, OF_getpropintarray,
    OF_getproplen, OF_parent, OF_peer,
};
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_softintr::softintr_dispatch;
use crate::kern::subr_prf::printf;
use crate::kern::subr_prf::{panic, splassert_fail};
use crate::queue_adapter;
use crate::sys::malloc::{M_DEVBUF, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::queue::{ListEntry, ListHead};
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
pub fn arm_smask(level: i32) -> u32 {
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

/// `arm_intr_get_parent`: find the interrupt parent by walking up the tree.
pub fn arm_intr_get_parent(node: i32) -> i32 {
    let mut node = node;
    while node != 0 {
        let phandle = OF_getpropint(node, b"interrupt-parent", 0);
        if phandle != 0 {
            return OF_getnodebyphandle(phandle);
        }
        node = OF_parent(node);
        if OF_getpropbool(node, b"interrupt-controller") {
            return node;
        }
    }
    0
}

// arm_intr_map_msi: with PCI (M5).

/// `MAX_INTERRUPT_CELLS`.
pub const MAX_INTERRUPT_CELLS: usize = 4;

/// `struct intr_prereg`: interrupt pre-registration.
///
/// To allow device drivers to establish interrupt handlers before all relevant interrupt
/// controllers have been attached, we support pre-registration of interrupt handlers. For
/// each node in the device tree that has an "interrupt-controller" property, we register a
/// dummy interrupt controller that simply stashes away all relevant details of the
/// interrupt handler being established. Later, when the real interrupt controller registers
/// itself, we establish those interrupt handlers based on that information.
pub struct IntrPrereg {
    /// `ip_list`.
    pub ip_list: ListEntry<IntrPrereg>,
    /// `ip_phandle`.
    pub ip_phandle: u32,
    /// `ip_cell[MAX_INTERRUPT_CELLS]`.
    pub ip_cell: [u32; MAX_INTERRUPT_CELLS],
    /// `ip_level`.
    pub ip_level: Cell<i32>,
    /// `ip_ci`.
    pub ip_ci: Option<&'static CpuInfo>,
    /// `ip_func`.
    pub ip_func: IntrFn,
    /// `ip_arg`.
    pub ip_arg: *mut c_void,
    /// `ip_name`.
    pub ip_name: &'static str,
    /// `ip_ic`: the controller that took it, once one did.
    pub ip_ic: Cell<*const InterruptController>,
    /// `ip_ih`: the controller's handle.
    pub ip_ih: Cell<*mut c_void>,
}

queue_adapter!(
    /// `LIST_HEAD(, intr_prereg) prereg_interrupts`.
    pub PreregList: IntrPrereg, ip_list => ListEntry<IntrPrereg>
);

queue_adapter!(
    /// `LIST_HEAD(, interrupt_controller) interrupt_controllers`.
    pub IcList: InterruptController, ic_list => ListEntry<InterruptController>
);

/// A list head that can be a static: every access happens at attach time on the boot CPU.
struct PreregHead(ListHead<PreregList>);
// SAFETY: see the type's doc.
unsafe impl Sync for PreregHead {}
/// As `PreregHead`.
struct IcHead(ListHead<IcList>);
// SAFETY: see `PreregHead`.
unsafe impl Sync for IcHead {}

/// `prereg_interrupts`.
static PREREG_INTERRUPTS: PreregHead = PreregHead(ListHead::new());
/// `interrupt_controllers`.
static INTERRUPT_CONTROLLERS: IcHead = IcHead(ListHead::new());

/// `arm_intr_prereg_establish_fdt`: the dummy controller's `ic_establish`.
pub fn arm_intr_prereg_establish_fdt(
    cookie: *const (),
    cell: &[u32],
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> *mut c_void {
    // SAFETY: the cookie is the controller itself (`arm_intr_init_fdt_recurse`).
    let ic = unsafe { &*cookie.cast::<InterruptController>() };

    let Some(ip) = malloc(size_of::<IntrPrereg>(), M_DEVBUF, M_ZERO | M_WAITOK) else {
        return ptr::null_mut();
    };
    let ip = ip.cast::<IntrPrereg>();
    let mut cells = [0u32; MAX_INTERRUPT_CELLS];
    for (i, c) in cells
        .iter_mut()
        .enumerate()
        .take(ic.ic_cells.get() as usize)
    {
        *c = cell[i];
    }
    // SAFETY: a fresh allocation of the right size and alignment, written once before use.
    unsafe {
        ip.write(IntrPrereg {
            ip_list: ListEntry::new(),
            ip_phandle: ic.ic_phandle.get(),
            ip_cell: cells,
            ip_level: Cell::new(level),
            ip_ci: ci,
            ip_func: func,
            ip_arg: arg,
            ip_name: name,
            ip_ic: Cell::new(ptr::null()),
            ip_ih: Cell::new(ptr::null_mut()),
        });
    }
    // SAFETY: as above; the entry lives until disestablished.
    let entry: &'static IntrPrereg = unsafe { ip.as_ref() };
    // SAFETY: a new entry, on no list; attach time, boot CPU.
    unsafe { PREREG_INTERRUPTS.0.insert_head(entry) };
    ip.as_ptr().cast::<c_void>()
}

/// `arm_intr_prereg_disestablish_fdt`.
pub fn arm_intr_prereg_disestablish_fdt(cookie: *mut c_void) {
    let Some(ip) = NonNull::new(cookie.cast::<IntrPrereg>()) else {
        return;
    };
    // SAFETY: a pre-registration from `arm_intr_prereg_establish_fdt`, alive until freed here.
    let entry = unsafe { ip.as_ref() };
    // SAFETY: the controller that took the entry is a registered one.
    let ic = unsafe { entry.ip_ic.get().as_ref() };

    if let Some(ic) = ic
        && !entry.ip_ih.get().is_null()
        && let Some(dis) = ic.ic_disestablish
    {
        dis(entry.ip_ih.get());
    }

    if ic.is_none() {
        // SAFETY: an entry no controller took is still on the list.
        unsafe { ListHead::<PreregList>::remove(entry) };
    }
    free(ip.cast::<u8>(), M_DEVBUF, size_of::<IntrPrereg>());
}

/// `arm_intr_prereg_barrier_fdt`.
pub fn arm_intr_prereg_barrier_fdt(cookie: *mut c_void) {
    // SAFETY: as for `arm_intr_prereg_disestablish_fdt`.
    let entry = unsafe { &*cookie.cast::<IntrPrereg>() };
    // SAFETY: as above.
    if let Some(ic) = unsafe { entry.ip_ic.get().as_ref() }
        && !entry.ip_ih.get().is_null()
        && let Some(barrier) = ic.ic_barrier
    {
        barrier(entry.ip_ih.get());
    }
}

/// `arm_intr_prereg_set_wakeup_fdt`.
pub fn arm_intr_prereg_set_wakeup_fdt(cookie: *mut c_void) {
    // SAFETY: as for `arm_intr_prereg_disestablish_fdt`.
    let entry = unsafe { &*cookie.cast::<IntrPrereg>() };
    // SAFETY: as above.
    if let Some(ic) = unsafe { entry.ip_ic.get().as_ref() }
        && !entry.ip_ih.get().is_null()
        && let Some(set_wakeup) = ic.ic_set_wakeup
    {
        set_wakeup(entry.ip_ih.get());
    }
    entry.ip_level.set(entry.ip_level.get() | IPL_WAKEUP);
}

/// `arm_intr_init_fdt_recurse`: a dummy controller for every "interrupt-controller" node.
fn arm_intr_init_fdt_recurse(node: i32) {
    if OF_getproplen(node, b"interrupt-controller") >= 0 {
        let Some(ic) = malloc(
            size_of::<InterruptController>(),
            M_DEVBUF,
            M_ZERO | M_WAITOK,
        ) else {
            return;
        };
        let ic = ic.cast::<InterruptController>();
        // SAFETY: a fresh allocation, written once before use; it lives forever.
        unsafe {
            ic.write(InterruptController {
                ic_node: Cell::new(node),
                ic_cookie: Cell::new(ic.as_ptr().cast::<()>()),
                ic_establish: Some(arm_intr_prereg_establish_fdt),
                ic_disestablish: Some(arm_intr_prereg_disestablish_fdt),
                ic_enable: None,
                ic_disable: None,
                ic_route: None,
                ic_cpu_enable: None,
                ic_barrier: Some(arm_intr_prereg_barrier_fdt),
                ic_set_wakeup: Some(arm_intr_prereg_set_wakeup_fdt),
                ic_list: ListEntry::new(),
                ic_phandle: Cell::new(0),
                ic_cells: Cell::new(0),
                ic_gic_its_id: Cell::new(0),
            });
        }
        // SAFETY: as above.
        arm_intr_register_fdt(unsafe { ic.as_ref() });
    }

    let mut child = OF_child(node);
    while child != 0 {
        arm_intr_init_fdt_recurse(child);
        child = OF_peer(child);
    }
}

/// `arm_intr_init_fdt`: pre-registers every interrupt controller of the tree.
pub fn arm_intr_init_fdt() {
    let node = OF_peer(0);
    if node != 0 {
        arm_intr_init_fdt_recurse(node);
    }
}

/// `arm_intr_register_fdt`: a real controller registers; the handlers pre-registered for
/// its node are established now.
pub fn arm_intr_register_fdt(ic: &'static InterruptController) {
    ic.ic_cells
        .set(OF_getpropint(ic.ic_node.get(), b"#interrupt-cells", 0));
    ic.ic_phandle
        .set(OF_getpropint(ic.ic_node.get(), b"phandle", 0));
    kassert!(ic.ic_cells.get() as usize <= MAX_INTERRUPT_CELLS);

    // SAFETY: a controller registers once; attach time, boot CPU.
    unsafe { INTERRUPT_CONTROLLERS.0.insert_head(ic) };

    // Establish pre-registered interrupt handlers.
    let mut next = PREREG_INTERRUPTS.0.first();
    while let Some(ip) = next {
        next = ListHead::<PreregList>::next(ip);
        if ip.ip_phandle != ic.ic_phandle.get() {
            continue;
        }
        ip.ip_ic.set(ptr::from_ref(ic));
        if let Some(establish) = ic.ic_establish {
            ip.ip_ih.set(establish(
                ic.ic_cookie.get(),
                &ip.ip_cell,
                ip.ip_level.get(),
                ip.ip_ci,
                ip.ip_func,
                ip.ip_arg,
                ip.ip_name,
            ));
        }
        if ip.ip_ih.get().is_null() {
            printf(format_args!("can't establish interrupt {}\n", ip.ip_name));
        }
        // SAFETY: `ip` is on the list; attach time, boot CPU.
        unsafe { ListHead::<PreregList>::remove(ip) };
    }
}

/// `arm_intr_establish_fdt`: the first interrupt of `node`.
pub fn arm_intr_establish_fdt(
    node: i32,
    level: i32,
    func: IntrFn,
    cookie: *mut c_void,
    name: &'static str,
) -> Option<NonNull<MachineIntrHandle>> {
    arm_intr_establish_fdt_idx(node, 0, level, func, cookie, name)
}

/// `arm_intr_establish_fdt_cpu`.
pub fn arm_intr_establish_fdt_cpu(
    node: i32,
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    cookie: *mut c_void,
    name: &'static str,
) -> Option<NonNull<MachineIntrHandle>> {
    arm_intr_establish_fdt_idx_cpu(node, 0, level, ci, func, cookie, name)
}

/// `arm_intr_establish_fdt_idx`: interrupt `idx` of `node`.
pub fn arm_intr_establish_fdt_idx(
    node: i32,
    idx: usize,
    level: i32,
    func: IntrFn,
    cookie: *mut c_void,
    name: &'static str,
) -> Option<NonNull<MachineIntrHandle>> {
    arm_intr_establish_fdt_idx_cpu(node, idx, level, None, func, cookie, name)
}

/// `arm_intr_establish_fdt_idx_cpu`: interrupt `idx` of `node` on `ci`, through the
/// controller its `interrupts-extended` or `interrupts` property names.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn arm_intr_establish_fdt_idx_cpu(
    node: i32,
    idx: usize,
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    cookie: *mut c_void,
    name: &'static str,
) -> Option<NonNull<MachineIntrHandle>> {
    let mut extended = true;
    let mut len = OF_getproplen(node, b"interrupts-extended");
    if len <= 0 {
        len = OF_getproplen(node, b"interrupts");
        extended = false;
    }
    if len <= 0 || len % 4 != 0 {
        return None;
    }
    let len = len as usize;

    // Old style.
    let mut ic: Option<&'static InterruptController> = None;
    if !extended {
        let parent = arm_intr_get_parent(node);
        ic = INTERRUPT_CONTROLLERS
            .0
            .iter()
            .find(|c| c.ic_node.get() == parent);
        ic?;
    }

    let cells = malloc(len, M_TEMP, M_WAITOK)?.cast::<u32>();
    // SAFETY: a fresh allocation of `len` bytes, a whole number of cells.
    let cells_slice = unsafe { core::slice::from_raw_parts_mut(cells.as_ptr(), len / 4) };
    if extended {
        OF_getpropintarray(node, b"interrupts-extended", cells_slice);
    } else {
        OF_getpropintarray(node, b"interrupts", cells_slice);
    }
    let mut cell: &[u32] = cells_slice;
    let mut val: *mut c_void = ptr::null_mut();
    let mut i = 0;
    while i <= idx && !cell.is_empty() {
        if extended {
            let phandle = cell[0];
            // Handle "empty" phandle reference.
            if phandle == 0 {
                cell = &cell[1..];
                continue;
            }
            ic = INTERRUPT_CONTROLLERS
                .0
                .iter()
                .find(|c| c.ic_phandle.get() == phandle);
            if ic.is_none() {
                break;
            }
            cell = &cell[1..];
        }
        let Some(c) = ic else {
            break;
        };
        let ncells = c.ic_cells.get() as usize;
        if i == idx
            && cell.len() >= ncells
            && let Some(establish) = c.ic_establish
        {
            val = establish(c.ic_cookie.get(), cell, level, ci, func, cookie, name);
            break;
        }
        if cell.len() < ncells {
            break;
        }
        cell = &cell[ncells..];
        i += 1;
    }
    free(cells.cast::<u8>(), M_TEMP, len);

    if val.is_null() {
        return None;
    }
    let ih =
        malloc(size_of::<MachineIntrHandle>(), M_DEVBUF, M_WAITOK)?.cast::<MachineIntrHandle>();
    // SAFETY: a fresh allocation, written once before use.
    unsafe {
        ih.write(MachineIntrHandle {
            ih_ic: ic.map_or(ptr::null(), ptr::from_ref),
            ih_ih: val,
        });
    }
    Some(ih)
}

// arm_intr_establish_fdt_imap, _imap_cpu, _msi, _msi_cpu: with PCI (M5).

/// `arm_intr_disestablish_fdt`.
///
/// # Safety
///
/// `cookie` must come from `arm_intr_establish_fdt*` and not be used afterwards.
pub unsafe fn arm_intr_disestablish_fdt(cookie: NonNull<MachineIntrHandle>) {
    // SAFETY: the caller's guarantee.
    let ih = unsafe { cookie.as_ref() };
    // SAFETY: an established handle names a registered controller.
    let ic = unsafe { &*ih.ih_ic };
    if let Some(dis) = ic.ic_disestablish {
        dis(ih.ih_ih);
    }
    free(
        cookie.cast::<u8>(),
        M_DEVBUF,
        size_of::<MachineIntrHandle>(),
    );
}

/// `arm_intr_enable`.
pub fn arm_intr_enable(cookie: NonNull<MachineIntrHandle>) {
    // SAFETY: an established handle names a registered controller.
    let ih = unsafe { cookie.as_ref() };
    // SAFETY: as above.
    let ic = unsafe { &*ih.ih_ic };
    kassert!(ic.ic_enable.is_some());
    if let Some(enable) = ic.ic_enable {
        enable(ih.ih_ih);
    }
}

/// `arm_intr_disable`.
pub fn arm_intr_disable(cookie: NonNull<MachineIntrHandle>) {
    // SAFETY: as for `arm_intr_enable`.
    let ih = unsafe { cookie.as_ref() };
    // SAFETY: as above.
    let ic = unsafe { &*ih.ih_ic };
    kassert!(ic.ic_disable.is_some());
    if let Some(disable) = ic.ic_disable {
        disable(ih.ih_ih);
    }
}

/// `arm_intr_parent_establish_fdt`: some interrupt controllers transparently forward
/// interrupts to their parent. Such interrupt controllers can use this function to delegate
/// the interrupt handler to their parent.
pub fn arm_intr_parent_establish_fdt(
    cookie: *const (),
    cell: &[u32],
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> *mut c_void {
    // SAFETY: the cookie is the forwarding controller itself.
    let ic = unsafe { &*cookie.cast::<InterruptController>() };

    let parent = arm_intr_get_parent(ic.ic_node.get());
    let Some(pic) = INTERRUPT_CONTROLLERS
        .0
        .iter()
        .find(|c| c.ic_node.get() == parent)
    else {
        return ptr::null_mut();
    };

    let Some(establish) = pic.ic_establish else {
        return ptr::null_mut();
    };
    let val = establish(pic.ic_cookie.get(), cell, level, ci, func, arg, name);
    if val.is_null() {
        return ptr::null_mut();
    }

    let Some(ih) = malloc(size_of::<MachineIntrHandle>(), M_DEVBUF, M_WAITOK) else {
        return ptr::null_mut();
    };
    let ih = ih.cast::<MachineIntrHandle>();
    // SAFETY: a fresh allocation, written once before use.
    unsafe {
        ih.write(MachineIntrHandle {
            ih_ic: ptr::from_ref(pic),
            ih_ih: val,
        });
    }
    ih.as_ptr().cast::<c_void>()
}

/// `arm_intr_parent_disestablish_fdt`.
pub fn arm_intr_parent_disestablish_fdt(cookie: *mut c_void) {
    let Some(ih) = NonNull::new(cookie.cast::<MachineIntrHandle>()) else {
        return;
    };
    // SAFETY: a handle from `arm_intr_parent_establish_fdt`.
    unsafe { arm_intr_disestablish_fdt(ih) };
}

/// `arm_intr_route`.
pub fn arm_intr_route(cookie: NonNull<MachineIntrHandle>, enable: bool, ci: &CpuInfo) {
    // SAFETY: as for `arm_intr_enable`.
    let ih = unsafe { cookie.as_ref() };
    // SAFETY: as above.
    let ic = unsafe { &*ih.ih_ic };
    if let Some(route) = ic.ic_route {
        route(ih.ih_ih, enable, ci);
    }
}

/// `arm_intr_cpu_enable`: every controller's per-CPU setup, on a CPU that comes up.
pub fn arm_intr_cpu_enable() {
    for ic in INTERRUPT_CONTROLLERS.0.iter() {
        if let Some(cpu_enable) = ic.ic_cpu_enable {
            cpu_enable();
        }
    }
}

/// `intr_barrier`: waits until no CPU runs the handler.
pub fn intr_barrier(cookie: NonNull<MachineIntrHandle>) {
    // SAFETY: as for `arm_intr_enable`.
    let ih = unsafe { cookie.as_ref() };
    // SAFETY: as above.
    let ic = unsafe { &*ih.ih_ic };
    if let Some(barrier) = ic.ic_barrier {
        barrier(ih.ih_ih);
    }
}

/// `intr_set_wakeup`.
pub fn intr_set_wakeup(cookie: NonNull<MachineIntrHandle>) {
    // SAFETY: as for `arm_intr_enable`.
    let ih = unsafe { cookie.as_ref() };
    // SAFETY: as above.
    let ic = unsafe { &*ih.ih_ic };
    if let Some(set_wakeup) = ic.ic_set_wakeup {
        set_wakeup(ih.ih_ih);
    }
}
