/* $OpenBSD: cpu.h,v 1.57 2026/09/06 20:02:12 kettenis Exp $ */
/*
 * Copyright (c) 2016 Dale Rahn <drahn@dalerahn.com>
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

//! arm64 `<machine/cpu.h>`: per-CPU state and the interrupt-mask helpers.
//!
//! Upstream: sys/arch/arm64/include/cpu.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M0 ports the DAIF helpers (`restore_daif`, `enable_irq_daif`,
//! `disable_irq_daif`, `disable_irq_daif_ret`, `intr_enable`, `intr_disable`, `intr_restore`);
//! M4 adds `struct cpu_info` (the fields the exception and interrupt paths use), `curcpu()`,
//! `cpu_info_primary`, the `CPUF_*` flags and the `CI_DDB_*` states. The CPU topology, the
//! scheduler state (M5), the `CTL_MACHDEP` names and the cache helpers arrive with their
//! subsystems.
//!
//! ## Deviations
//! - DAIF values are `u64`, the width of the register (`mrs`/`msr` move a full X register);
//!   C narrows them to `uint32_t` on the way out and widens them back.
//! - The `cpu_info` fields kept follow the C's order; the ones left out are named in
//!   comments. Nothing reads the struct by a C offset yet (`CI_TRAMPOLINE_VECTORS` is M6).

use core::arch::asm;
use core::cell::{Cell, UnsafeCell};
use core::ptr;
use core::sync::atomic::AtomicU32;

use crate::arch::arm64::include::pmap::Pmap;

/// `restore_daif`: writes `daif` back into `DAIF`.
///
/// # Safety
///
/// Changing the interrupt mask bits must be done by code that owns the current interrupt
/// level; use through `intr_restore` with a value from `intr_disable`.
#[inline]
pub unsafe fn restore_daif(daif: u64) {
    // SAFETY: a system register write with no memory effect; the caller owns the mask.
    unsafe { asm!("msr daif, {}", in(reg) daif, options(nomem, nostack, preserves_flags)) };
}

/// `enable_irq_daif`: unmasks IRQ and FIQ.
///
/// # Safety
///
/// As for [`restore_daif`].
#[inline]
pub unsafe fn enable_irq_daif() {
    // SAFETY: as for `restore_daif`.
    unsafe { asm!("msr daifclr, #3", options(nomem, nostack, preserves_flags)) };
}

/// `disable_irq_daif`: masks IRQ and FIQ.
#[inline]
pub fn disable_irq_daif() {
    // SAFETY: masking interrupts is always sound; it only delays their delivery.
    unsafe { asm!("msr daifset, #3", options(nomem, nostack, preserves_flags)) };
}

/// `disable_irq_daif_ret`: masks IRQ and FIQ and returns the previous `DAIF`.
#[inline]
pub fn disable_irq_daif_ret() -> u64 {
    let daif: u64;
    // SAFETY: as for `disable_irq_daif`; the read has no side effects.
    unsafe {
        asm!("mrs {}, daif", out(reg) daif, options(nomem, nostack, preserves_flags));
        asm!("msr daifset, #3", options(nomem, nostack, preserves_flags));
    }
    daif
}

/// `intr_enable`: enables interrupts.
///
/// # Safety
///
/// As for [`restore_daif`].
#[inline]
pub unsafe fn intr_enable() {
    // SAFETY: forwarded.
    unsafe { enable_irq_daif() };
}

/// `intr_disable`: disables interrupts and returns the state for [`intr_restore`].
#[inline]
pub fn intr_disable() -> u64 {
    disable_irq_daif_ret()
}

/// `intr_restore`: restores the interrupt state saved by [`intr_disable`].
///
/// # Safety
///
/// `daif` must come from [`intr_disable`] on this CPU.
#[inline]
pub unsafe fn intr_restore(daif: u64) {
    // SAFETY: forwarded.
    unsafe { restore_daif(daif) };
}

/// `struct cpu_info`: the per-CPU state (the M4 subset, see the module doc).
#[repr(C)]
pub struct CpuInfo {
    /// `struct device`: Device corresponding to this CPU (M4-b).
    pub ci_dev: Cell<*const ()>,
    /// The next CPU.
    pub ci_next: Cell<*const CpuInfo>,
    // ci_schedstate: scheduler state (M5).
    /// `ci_cpuid`.
    pub ci_cpuid: Cell<u32>,
    /// `ci_mpidr`.
    pub ci_mpidr: Cell<u64>,
    /// `ci_midr`.
    pub ci_midr: Cell<u64>,
    /// `ci_acpi_proc_id`.
    pub ci_acpi_proc_id: Cell<u32>,
    /// `ci_node`: the device tree node.
    pub ci_node: Cell<i32>,
    /// This structure's own address.
    pub ci_self: Cell<*const CpuInfo>,
    // __HAVE_CPU_TOPOLOGY: ci_cputype, ci_smt_id, ci_core_id, ci_pkg_id (M4-b).
    /// `struct proc` (M5).
    pub ci_curproc: Cell<*const ()>,
    /// `struct pcb` (M5).
    pub ci_curpcb: Cell<*const ()>,
    /// The active pmap.
    pub ci_curpm: Cell<*const Pmap>,
    /// `ci_randseed`.
    pub ci_randseed: Cell<u32>,
    /// `ci_ctrl`: The CPU control register.
    pub ci_ctrl: Cell<u32>,
    /// `ci_trampoline_vectors`: the EL0 vector table (M6).
    pub ci_trampoline_vectors: Cell<u64>,
    /// `ci_cpl`: the current interrupt priority level.
    pub ci_cpl: Cell<u32>,
    /// `ci_ipending`.
    pub ci_ipending: Cell<u32>,
    /// `ci_idepth`: the interrupt nesting depth.
    pub ci_idepth: Cell<u32>,
    /// `DIAGNOSTIC`: the mutex nesting level.
    pub ci_mutex_level: Cell<i32>,
    /// The scheduler asks for a reschedule.
    pub ci_want_resched: Cell<i32>,
    /// `ci_flush_bp`: the branch predictor flush, if the CPU needs one.
    pub ci_flush_bp: Cell<Option<fn()>>,
    /// `ci_serror`: the system error handler, if the CPU has one.
    pub ci_serror: Cell<Option<fn()>>,
    /// `ci_ttbr1`.
    pub ci_ttbr1: Cell<u64>,
    /// `ci_el1_stkend`.
    pub ci_el1_stkend: Cell<usize>,
    // ci_psci_*, ci_opp_*, ci_cpu_supply, ci_capacity, ci_prev_sleep, ci_last_itime: M5.
    // MULTIPROCESSOR: ci_srp_hazards, ci_xcall, ci_uvm.
    /// \[a\] `CPUF_*`.
    pub ci_flags: AtomicU32,
    /// `CI_DDB_*`.
    pub ci_ddb_paused: Cell<i32>,
    // ci_gmon (GPROF), ci_queue (clockintr): M5.
    /// The first panic message of this CPU.
    pub ci_panicbuf: UnsafeCell<[u8; 512]>,
}

// SAFETY: one CPU's state, touched by that CPU (and read by ddb); the boot CPU is alone.
unsafe impl Sync for CpuInfo {}

impl CpuInfo {
    /// A CPU before `cpu_attach`: everything zero, `ci_self` unset.
    pub const fn new() -> Self {
        Self {
            ci_dev: Cell::new(ptr::null()),
            ci_next: Cell::new(ptr::null()),
            ci_cpuid: Cell::new(0),
            ci_mpidr: Cell::new(0),
            ci_midr: Cell::new(0),
            ci_acpi_proc_id: Cell::new(0),
            ci_node: Cell::new(0),
            ci_self: Cell::new(ptr::null()),
            ci_curproc: Cell::new(ptr::null()),
            ci_curpcb: Cell::new(ptr::null()),
            ci_curpm: Cell::new(ptr::null()),
            ci_randseed: Cell::new(0),
            ci_ctrl: Cell::new(0),
            ci_trampoline_vectors: Cell::new(0),
            ci_cpl: Cell::new(0),
            ci_ipending: Cell::new(0),
            ci_idepth: Cell::new(0),
            ci_mutex_level: Cell::new(0),
            ci_want_resched: Cell::new(0),
            ci_flush_bp: Cell::new(None),
            ci_serror: Cell::new(None),
            ci_ttbr1: Cell::new(0),
            ci_el1_stkend: Cell::new(0),
            ci_flags: AtomicU32::new(0),
            ci_ddb_paused: Cell::new(0),
            ci_panicbuf: UnsafeCell::new([0; 512]),
        }
    }
}

impl Default for CpuInfo {
    fn default() -> Self {
        Self::new()
    }
}

/// `CI_DDB_RUNNING`.
pub const CI_DDB_RUNNING: i32 = 0;
/// `CI_DDB_SHOULDSTOP`.
pub const CI_DDB_SHOULDSTOP: i32 = 1;
/// `CI_DDB_STOPPED`.
pub const CI_DDB_STOPPED: i32 = 2;
/// `CI_DDB_ENTERDDB`.
pub const CI_DDB_ENTERDDB: i32 = 3;
/// `CI_DDB_INDDB`.
pub const CI_DDB_INDDB: i32 = 4;

/// `CPUF_PRIMARY`.
pub const CPUF_PRIMARY: u32 = 1 << 0;
/// `CPUF_AP`.
pub const CPUF_AP: u32 = 1 << 1;
/// `CPUF_IDENTIFY`.
pub const CPUF_IDENTIFY: u32 = 1 << 2;
/// `CPUF_IDENTIFIED`.
pub const CPUF_IDENTIFIED: u32 = 1 << 3;
/// `CPUF_PRESENT`.
pub const CPUF_PRESENT: u32 = 1 << 4;
/// `CPUF_GO`.
pub const CPUF_GO: u32 = 1 << 5;
/// `CPUF_RUNNING`.
pub const CPUF_RUNNING: u32 = 1 << 6;
/// `CPUF_PARK`.
pub const CPUF_PARK: u32 = 1 << 7;
/// `CPUF_PARKED`.
pub const CPUF_PARKED: u32 = 1 << 8;

/// `curcpu()`: this CPU's `cpu_info`, from `TPIDR_EL1` (set by `initarm` for the boot CPU
/// and by `cpu_start_secondary` for the others).
#[inline]
pub fn curcpu() -> &'static CpuInfo {
    let ci: *const CpuInfo;
    // SAFETY: a system register read with no side effects.
    unsafe { asm!("mrs {}, tpidr_el1", out(reg) ci, options(nomem, nostack, preserves_flags)) };
    // SAFETY: TPIDR_EL1 names a static cpu_info, which lives forever.
    unsafe { &*ci }
}

/// `cpu_number()`.
#[inline]
pub fn cpu_number() -> u32 {
    curcpu().ci_cpuid.get()
}

/// `cpu_info_primary`: the boot CPU's `cpu_info` (`arm64/machdep.rs`).
pub fn cpu_info_primary() -> &'static CpuInfo {
    &crate::arch::arm64::arm64::machdep::CPU_INFO_PRIMARY
}

/// `CPU_IS_PRIMARY(ci)`.
pub fn cpu_is_primary(ci: &CpuInfo) -> bool {
    ptr::eq(ci, cpu_info_primary())
}

/// `CPU_IS_RUNNING(ci)`.
pub fn cpu_is_running(ci: &CpuInfo) -> bool {
    ci.ci_flags.load(core::sync::atomic::Ordering::Relaxed) & CPUF_RUNNING != 0
}
