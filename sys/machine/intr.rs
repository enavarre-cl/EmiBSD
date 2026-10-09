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
/* </LICENSES> */

/* <CODE> */
//! `<machine/intr.h>` as a trait: the interrupt priority levels and `spl(9)`.
//!
//! Milestone M3 needs the `IPL_*` numbers that pools and mutexes carry around; M4 adds
//! `splraise`, `spllower`, `splx`, the `spl*()` helpers, `softintr` and `splassert`. The
//! handler registration stays per architecture (`intr_establish`, `arm_intr_establish_fdt`):
//! generic code reaches it through the drivers' bus attachments (`pci_intr_establish`,
//! `fdt_intr_establish`), whose handles are the `void *` cookies [`intr_barrier`] takes back.

use core::ffi::c_void;
use core::ptr::NonNull;

use crate::machine::Machine;

/// An interrupt handler, `int (*)(void *)`: nonzero when the interrupt was the device's.
pub type IntrFn = fn(*mut c_void) -> i32;

/// The interrupt priority levels of the selected architecture.
pub trait Intr {
    /// `IPL_NONE`: nothing.
    const IPL_NONE: i32;
    /// `IPL_SOFTCLOCK`: timeouts.
    const IPL_SOFTCLOCK: i32;
    /// `IPL_SOFTNET`: protocol stacks.
    const IPL_SOFTNET: i32;
    /// `IPL_SOFTTTY`: delayed terminal handling.
    const IPL_SOFTTTY: i32;
    /// `IPL_BIO`: block I/O.
    const IPL_BIO: i32;
    /// `IPL_NET`: network.
    const IPL_NET: i32;
    /// `IPL_TTY`: terminal.
    const IPL_TTY: i32;
    /// `IPL_VM`: memory allocation.
    const IPL_VM: i32;
    /// `IPL_AUDIO`: audio.
    const IPL_AUDIO: i32;
    /// `IPL_CLOCK`: clock.
    const IPL_CLOCK: i32;
    /// `IPL_SCHED`: the scheduler's level.
    const IPL_SCHED: i32;
    /// `IPL_STATCLOCK`: the statistics clock's level.
    const IPL_STATCLOCK: i32;
    /// `IPL_HIGH`: everything.
    const IPL_HIGH: i32;
    /// `IPL_IPI`: inter-processor interrupts.
    const IPL_IPI: i32;
    /// `IPL_MPFLOOR`: the lowest level that takes the kernel lock.
    const IPL_MPFLOOR: i32;
    /// `IPL_MPSAFE`: an 'mpsafe' interrupt, no kernel lock.
    const IPL_MPSAFE: i32;
    /// `IPL_WAKEUP`: a 'wakeup' interrupt.
    const IPL_WAKEUP: i32;

    /// `splraise(ipl)`: raises the current level to at least `ipl`; returns the old level.
    fn splraise(ipl: i32) -> i32;

    /// `spllower(ipl)`: lowers the level to `ipl`, running the interrupts that were held
    /// back; returns the old level.
    fn spllower(ipl: i32) -> i32;

    /// `splx(s)`: restores the level `splraise` returned.
    fn splx(s: i32);

    /// `softintr(si)`: marks soft interrupt level `si` (`SOFTINTR_*`) pending on this CPU.
    fn softintr(si: i32);

    /// `splassert_check(wantipl, func)` (`DIAGNOSTIC`): reports through `splassert_fail`
    /// when the current level is below `wantipl`.
    fn splassert_check(wantipl: i32, func: &str);

    /// `intr_barrier(cookie)`: returns once a handler running for the interrupt `cookie`
    /// (from a bus's `*_intr_establish`) on any CPU has finished.
    fn intr_barrier(cookie: NonNull<c_void>);
}

/// `splraise` on the selected machine.
pub fn splraise(ipl: i32) -> i32 {
    Machine::splraise(ipl)
}

/// `spllower` on the selected machine.
pub fn spllower(ipl: i32) -> i32 {
    Machine::spllower(ipl)
}

/// `splx` on the selected machine.
pub fn splx(s: i32) {
    Machine::splx(s)
}

/// `softintr` on the selected machine.
pub fn softintr(si: i32) {
    Machine::softintr(si)
}

/// `splassert(wantipl)`: the `DIAGNOSTIC` check, when `splassert_ctl` is on.
pub fn splassert(wantipl: i32, func: &str) {
    #[cfg(feature = "diagnostic")]
    if crate::kern::subr_prf::SPLASSERT_CTL.load(core::sync::atomic::Ordering::Relaxed) > 0 {
        Machine::splassert_check(wantipl, func);
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = (wantipl, func);
}

/// `intr_barrier(9)` on the selected machine.
pub fn intr_barrier(cookie: NonNull<c_void>) {
    Machine::intr_barrier(cookie)
}

/// `splsoftassert(wantipl)`.
pub fn splsoftassert(wantipl: i32, func: &str) {
    splassert(wantipl, func)
}

/// `splbio()`.
pub fn splbio() -> i32 {
    splraise(IPL_BIO)
}
/// `splnet()`.
pub fn splnet() -> i32 {
    splraise(IPL_NET)
}
/// `spltty()`.
pub fn spltty() -> i32 {
    splraise(IPL_TTY)
}
/// `splaudio()`.
pub fn splaudio() -> i32 {
    splraise(IPL_AUDIO)
}
/// `splclock()`.
pub fn splclock() -> i32 {
    splraise(IPL_CLOCK)
}
/// `splstatclock()`.
pub fn splstatclock() -> i32 {
    splraise(IPL_STATCLOCK)
}
/// `splipi()`.
pub fn splipi() -> i32 {
    splraise(IPL_IPI)
}
/// `splsoftclock()`.
pub fn splsoftclock() -> i32 {
    splraise(IPL_SOFTCLOCK)
}
/// `splsoftnet()`.
pub fn splsoftnet() -> i32 {
    splraise(IPL_SOFTNET)
}
/// `splsofttty()`.
pub fn splsofttty() -> i32 {
    splraise(IPL_SOFTTTY)
}
/// `splvm()`.
pub fn splvm() -> i32 {
    splraise(IPL_VM)
}
/// `splhigh()`.
pub fn splhigh() -> i32 {
    splraise(IPL_HIGH)
}
/// `splsched()`.
pub fn splsched() -> i32 {
    splraise(IPL_SCHED)
}
/// `spl0()`.
pub fn spl0() -> i32 {
    spllower(IPL_NONE)
}

/// `IPL_NONE` on the selected machine.
pub const IPL_NONE: i32 = <Machine as Intr>::IPL_NONE;
/// `IPL_SOFTCLOCK` on the selected machine.
pub const IPL_SOFTCLOCK: i32 = <Machine as Intr>::IPL_SOFTCLOCK;
/// `IPL_SOFTNET` on the selected machine.
pub const IPL_SOFTNET: i32 = <Machine as Intr>::IPL_SOFTNET;
/// `IPL_SOFTTTY` on the selected machine.
pub const IPL_SOFTTTY: i32 = <Machine as Intr>::IPL_SOFTTTY;
/// `IPL_BIO` on the selected machine.
pub const IPL_BIO: i32 = <Machine as Intr>::IPL_BIO;
/// `IPL_NET` on the selected machine.
pub const IPL_NET: i32 = <Machine as Intr>::IPL_NET;
/// `IPL_TTY` on the selected machine.
pub const IPL_TTY: i32 = <Machine as Intr>::IPL_TTY;
/// `IPL_VM` on the selected machine.
pub const IPL_VM: i32 = <Machine as Intr>::IPL_VM;
/// `IPL_AUDIO` on the selected machine.
pub const IPL_AUDIO: i32 = <Machine as Intr>::IPL_AUDIO;
/// `IPL_CLOCK` on the selected machine.
pub const IPL_CLOCK: i32 = <Machine as Intr>::IPL_CLOCK;
/// `IPL_SCHED` on the selected machine.
pub const IPL_SCHED: i32 = <Machine as Intr>::IPL_SCHED;
/// `IPL_STATCLOCK` on the selected machine.
pub const IPL_STATCLOCK: i32 = <Machine as Intr>::IPL_STATCLOCK;
/// `IPL_HIGH` on the selected machine.
pub const IPL_HIGH: i32 = <Machine as Intr>::IPL_HIGH;
/// `IPL_IPI` on the selected machine.
pub const IPL_IPI: i32 = <Machine as Intr>::IPL_IPI;
/// `IPL_MPFLOOR` on the selected machine.
pub const IPL_MPFLOOR: i32 = <Machine as Intr>::IPL_MPFLOOR;
/// `IPL_MPSAFE` on the selected machine.
pub const IPL_MPSAFE: i32 = <Machine as Intr>::IPL_MPSAFE;
/// `IPL_WAKEUP` on the selected machine.
pub const IPL_WAKEUP: i32 = <Machine as Intr>::IPL_WAKEUP;
/* </CODE> */
