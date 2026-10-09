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
//! The machine-dependent half of `<dev/acpi/acpivar.h>` as a trait: what each architecture's
//! `acpi_machdep.c` defines for `acpi(4)` (`dev/acpi/acpi.c`, `dsdt.c` and the ACPI
//! drivers) to call.
//!
//! amd64 implements it (`arch/amd64/amd64/acpi_machdep.c`, acpi0 at bios0), and arm64
//! since M14 (`arch/arm64/arm64/acpi_machdep.c`, acpi0 at the device tree node efiboot
//! makes from the UEFI ACPI tables; no global lock). The host double answers as a machine
//! without ACPI, with the global lock's compare and swap kept so the interpreter's tests see
//! a working lock.
//!
//! Two members are not functions in C: `pwr_action` (the global the power button
//! consults, defined by amd64's `machdep.c` and arm64's `acpi_machdep.c`) and
//! `ci->ci_acpi_proc_id` (a `struct cpu_info` member on both, read by `acpi_add_device`);
//! nor is `cpu_suspended` (each `cpu.c`'s, cleared by `acpi.c`'s wake events).
//!
//! The timer drivers (`acpitimer.c`, `acpihpet.c`, configured on amd64 and i386 only) also
//! call `delay_init`/`delay_fini` (amd64's `<machine/cpu.h>`) and, under
//! `#if defined(__amd64__)`, `tsc.c`'s `cpu_recalibrate_tsc`; they are here because those
//! drivers are their only generic callers.

use core::ffi::c_void;
use core::ptr::NonNull;
use core::sync::atomic::AtomicI32;

use crate::dev::acpi::acpivar::{AcpiMemMap, AcpiSoftc};
use crate::dev::acpi::amltypes::AmlNodeRef;
use crate::machine::Machine;
use crate::machine::bus::{BusAddr, BusDmaTag, BusSize, BusSpaceHandle, BusSpaceTag};
use crate::machine::cpu::CpuInfo;
use crate::sys::errno::Errno;
use crate::sys::timetc::Timecounter;
use crate::sys::types::Paddr;

/// What `acpi(4)` needs from the machine.
pub trait AcpiMachdep {
    /// `defined(__amd64__) || defined(__i386__)`: the PCI interrupt routing tables
    /// (`_PRT`) attach `acpiprt`.
    const ACPI_PRT: bool;
    /// `defined(__arm64__)`: an `ECTC` node attaches `acpisectwo` (the Surface Pro X's
    /// embedded controller).
    const ACPI_SECTWO: bool;

    /// `acpi_map(pa, len, handle)`: maps `len` bytes of physical memory at `pa` (a firmware
    /// table) read-write into kernel virtual space.
    fn acpi_map(pa: Paddr, len: usize) -> Result<AcpiMemMap, Errno>;

    /// `acpi_unmap(handle)`: undoes [`acpi_map`](Self::acpi_map).
    fn acpi_unmap(handle: &AcpiMemMap);

    /// `acpi_bus_space_map(t, addr, size, flags, &bsh)`: maps a register range an AML
    /// operation region or a fixed register names.
    ///
    /// # Safety
    ///
    /// As for `bus_space_map`: the range is device or firmware space the firmware tables
    /// describe.
    unsafe fn acpi_bus_space_map(
        t: BusSpaceTag,
        addr: BusAddr,
        size: BusSize,
        flags: i32,
    ) -> Result<BusSpaceHandle, Errno>;

    /// `acpi_bus_space_unmap(t, bsh, size)`.
    fn acpi_bus_space_unmap(t: BusSpaceTag, bsh: BusSpaceHandle, size: BusSize);

    /// `acpi_intr_establish(irq, flags, level, handler, arg, what)`: establishes a handler
    /// for global system interrupt `irq` with the `LR_EXTIRQ_*` trigger `flags`; NULL when it
    /// cannot.
    fn acpi_intr_establish(
        irq: i32,
        flags: i32,
        level: i32,
        handler: fn(*mut c_void) -> i32,
        arg: *mut c_void,
        what: &'static str,
    ) -> Option<NonNull<c_void>>;

    /// `acpi_intr_disestablish(cookie)`.
    ///
    /// # Safety
    ///
    /// `cookie` came from [`acpi_intr_establish`](Self::acpi_intr_establish) and is not used
    /// afterwards.
    unsafe fn acpi_intr_disestablish(cookie: NonNull<c_void>);

    /// `acpi_attach_machdep(sc)`: the machine's part of acpi0's attachment (its interrupt,
    /// the reset hook, the wakeup trampoline).
    fn acpi_attach_machdep(sc: &'static AcpiSoftc);

    /// `acpi_acquire_glk(lock)`: takes the firmware's global lock (the FACS's `global_lock`
    /// word, section 5.2.10.1), or marks it pending; nonzero when it is ours.
    ///
    /// # Safety
    ///
    /// `lock` points at the mapped FACS's `global_lock`, valid for atomic access.
    unsafe fn acpi_acquire_glk(lock: *mut u32) -> i32;

    /// `acpi_release_glk(lock)`: releases it; nonzero when the firmware waits for it.
    ///
    /// # Safety
    ///
    /// As for [`acpi_acquire_glk`](Self::acpi_acquire_glk).
    unsafe fn acpi_release_glk(lock: *mut u32) -> i32;

    /// `acpi_iommu_device_map(node, dmat)`: the DMA tag a device described by `node` uses.
    fn acpi_iommu_device_map(node: &AmlNodeRef, dmat: Option<BusDmaTag>) -> Option<BusDmaTag>;

    /// `pwr_action`: what the power button does: 0 nothing, 1 power down, 2 suspend
    /// (`machdep.pwraction`).
    fn pwr_action() -> i32;

    /// `ci->ci_acpi_proc_id`: the processor's ACPI id.
    fn ci_acpi_proc_id(ci: &CpuInfo) -> u32;

    /// `cpu_suspended` (`cpu.c`): set while the boot processor idles in the S0 suspend loop
    /// (`cpu_suspend_primary`); a wake event clears it.
    fn cpu_suspended() -> &'static AtomicI32;

    /// `delay_init(fn, fn_quality)` (amd64's `<machine/cpu.h>`): makes `f` the `delay(9)`
    /// implementation if `fn_quality` beats the current one's.
    fn delay_init(f: fn(i32), fn_quality: i32);

    /// `delay_fini(fn)`: if `f` is the `delay(9)` implementation, goes back to the default.
    fn delay_fini(f: fn(i32));

    /// `cpu_recalibrate_tsc(tc)` (amd64's `tsc.c`): offers `tc` as the TSC's reference
    /// timecounter. The drivers call it only `#if defined(__amd64__)`; elsewhere it does
    /// nothing, as that code is not compiled.
    fn cpu_recalibrate_tsc(tc: &'static Timecounter);
}

/// `ACPI_PRT` of the selected machine.
pub const ACPI_PRT: bool = <Machine as AcpiMachdep>::ACPI_PRT;
/// `ACPI_SECTWO` of the selected machine.
pub const ACPI_SECTWO: bool = <Machine as AcpiMachdep>::ACPI_SECTWO;

/// `acpi_map(pa, len, handle)` on the selected machine.
pub fn acpi_map(pa: Paddr, len: usize) -> Result<AcpiMemMap, Errno> {
    Machine::acpi_map(pa, len)
}

/// `acpi_unmap(handle)` on the selected machine.
pub fn acpi_unmap(handle: &AcpiMemMap) {
    Machine::acpi_unmap(handle)
}

/// `acpi_bus_space_map(t, addr, size, flags, &bsh)` on the selected machine.
///
/// # Safety
///
/// As for [`AcpiMachdep::acpi_bus_space_map`].
pub unsafe fn acpi_bus_space_map(
    t: BusSpaceTag,
    addr: BusAddr,
    size: BusSize,
    flags: i32,
) -> Result<BusSpaceHandle, Errno> {
    // SAFETY: forwarded.
    unsafe { Machine::acpi_bus_space_map(t, addr, size, flags) }
}

/// `acpi_bus_space_unmap(t, bsh, size)` on the selected machine.
pub fn acpi_bus_space_unmap(t: BusSpaceTag, bsh: BusSpaceHandle, size: BusSize) {
    Machine::acpi_bus_space_unmap(t, bsh, size)
}

/// `acpi_intr_establish(irq, flags, level, handler, arg, what)` on the selected machine.
pub fn acpi_intr_establish(
    irq: i32,
    flags: i32,
    level: i32,
    handler: fn(*mut c_void) -> i32,
    arg: *mut c_void,
    what: &'static str,
) -> Option<NonNull<c_void>> {
    Machine::acpi_intr_establish(irq, flags, level, handler, arg, what)
}

/// `acpi_intr_disestablish(cookie)` on the selected machine.
///
/// # Safety
///
/// As for [`AcpiMachdep::acpi_intr_disestablish`].
pub unsafe fn acpi_intr_disestablish(cookie: NonNull<c_void>) {
    // SAFETY: forwarded.
    unsafe { Machine::acpi_intr_disestablish(cookie) }
}

/// `acpi_attach_machdep(sc)` on the selected machine.
pub fn acpi_attach_machdep(sc: &'static AcpiSoftc) {
    Machine::acpi_attach_machdep(sc)
}

/// `acpi_acquire_glk(lock)` on the selected machine.
///
/// # Safety
///
/// As for [`AcpiMachdep::acpi_acquire_glk`].
pub unsafe fn acpi_acquire_glk(lock: *mut u32) -> i32 {
    // SAFETY: forwarded.
    unsafe { Machine::acpi_acquire_glk(lock) }
}

/// `acpi_release_glk(lock)` on the selected machine.
///
/// # Safety
///
/// As for [`AcpiMachdep::acpi_release_glk`].
pub unsafe fn acpi_release_glk(lock: *mut u32) -> i32 {
    // SAFETY: forwarded.
    unsafe { Machine::acpi_release_glk(lock) }
}

/// `acpi_iommu_device_map(node, dmat)` on the selected machine.
pub fn acpi_iommu_device_map(node: &AmlNodeRef, dmat: Option<BusDmaTag>) -> Option<BusDmaTag> {
    Machine::acpi_iommu_device_map(node, dmat)
}

/// `pwr_action` on the selected machine.
pub fn pwr_action() -> i32 {
    Machine::pwr_action()
}

/// `ci->ci_acpi_proc_id` on the selected machine.
pub fn ci_acpi_proc_id(ci: &CpuInfo) -> u32 {
    Machine::ci_acpi_proc_id(ci)
}

/// `cpu_suspended` on the selected machine.
pub fn cpu_suspended() -> &'static AtomicI32 {
    Machine::cpu_suspended()
}

/// `delay_init(fn, fn_quality)` on the selected machine.
pub fn delay_init(f: fn(i32), fn_quality: i32) {
    Machine::delay_init(f, fn_quality)
}

/// `delay_fini(fn)` on the selected machine.
pub fn delay_fini(f: fn(i32)) {
    Machine::delay_fini(f)
}

/// `cpu_recalibrate_tsc(tc)` on the selected machine (amd64 only; nothing elsewhere).
pub fn cpu_recalibrate_tsc(tc: &'static Timecounter) {
    Machine::cpu_recalibrate_tsc(tc)
}

/// The global lock's compare and swap, the same on every machine (`acpi_machdep.c` of amd64,
/// i386 and arm64): `acpi_acquire_glk` when `acquire`, else `acpi_release_glk`.
///
/// # Safety
///
/// `lock` is valid for atomic access to a `u32` (the FACS's `global_lock`, which the
/// firmware updates with locked instructions too).
pub unsafe fn acpi_glk_cas(lock: *mut u32, acquire: bool) -> i32 {
    use crate::dev::acpi::acpivar::{GL_BIT_OWNED, GL_BIT_PENDING};
    use core::sync::atomic::{AtomicU32, Ordering};

    // SAFETY: the caller's guarantee; `AtomicU32` has the layout of `u32`.
    let lock = unsafe { AtomicU32::from_ptr(lock) };
    loop {
        let old = lock.load(Ordering::Relaxed);
        let new = if acquire {
            let mut new = (old & !GL_BIT_PENDING) | GL_BIT_OWNED;
            if old & GL_BIT_OWNED != 0 {
                new |= GL_BIT_PENDING;
            }
            new
        } else {
            old & !(GL_BIT_PENDING | GL_BIT_OWNED)
        };
        // atomic_cas_uint(lock, old, new) != old: try again.
        if lock
            .compare_exchange(old, new, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            return if acquire {
                i32::from(new & GL_BIT_PENDING == 0)
            } else {
                i32::from(old & GL_BIT_PENDING != 0)
            };
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::acpi::acpivar::{GL_BIT_OWNED, GL_BIT_PENDING};

    #[test]
    fn global_lock_protocol() {
        let mut lock = 0u32;
        // SAFETY: a local word.
        unsafe {
            assert_eq!(acpi_glk_cas(&mut lock, true), 1);
            assert_eq!(lock, GL_BIT_OWNED);
            // Owned by someone: pending, not ours.
            assert_eq!(acpi_glk_cas(&mut lock, true), 0);
            assert_eq!(lock, GL_BIT_OWNED | GL_BIT_PENDING);
            // Releasing reports the waiter and clears both bits.
            assert_eq!(acpi_glk_cas(&mut lock, false), 1);
            assert_eq!(lock, 0);
            assert_eq!(acpi_glk_cas(&mut lock, false), 0);
        }
    }
}
/* </TESTS> */
