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
/* </LICENSES> */

/* <CODE> */
//! arm64 `<machine/cpu.h>`: per-CPU state and the interrupt-mask helpers.
//!
//! Upstream: sys/arch/arm64/include/cpu.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M0 ports the DAIF helpers (`restore_daif`, `enable_irq_daif`,
//! `disable_irq_daif`, `disable_irq_daif_ret`, `intr_enable`, `intr_disable`, `intr_restore`);
//! M4 adds `struct cpu_info` (the fields the exception and interrupt paths use), `curcpu()`,
//! `cpu_info_primary`, the `CPUF_*` flags and the `CI_DDB_*` states; M5 adds `ci_schedstate`,
//! `ci_queue`, `MAXCPUS`, `CPU_INFO_UNIT`, `struct clockframe` (the `trapframe`) and the
//! `CLKF_*` macros. M11a (`cpu.c`) completes `struct cpu_info` (the topology, the PSCI idle
//! state, the operating points, the capacity) and the `MULTIPROCESSOR` side:
//! `cpu_number()`, `CPU_IS_RUNNING`, `CPU_INFO_UNIT` from `ci_dev`, `MAXCPUS` 256 and
//! `CPU_BUSY_CYCLE`. The `CTL_MACHDEP` names and `CTL_MACHDEP_NAMES` (`cpu_sysctl`) come with
//! M13; the cache helpers arrive with their subsystems.
//!
//! ## Deviations
//! - DAIF values are `u64`, the width of the register (`mrs`/`msr` move a full X register);
//!   C narrows them to `uint32_t` on the way out and widens them back.
//! - The `cpu_info` fields kept follow the C's order; the ones left out are named in
//!   comments. Nothing reads the struct by a C offset but `ci_curproc`/`ci_curpcb`
//!   (`cpuswitch.S`, `exception.S`; `CI_TRAMPOLINE_VECTORS` waits for `trampoline.S`).
//! - The fields another CPU touches while the CPU runs are atomics (`ci_flags`,
//!   `ci_want_resched`, `ci_opp_idx`, `ci_opp_max`, `ci_ddb_paused`) or [`CiPtr`]
//!   (`ci_next`, `ci_curproc`, `ci_curpm`), where the C has plain or `volatile` fields; the
//!   struct's documentation says which protocol orders the rest.
//! - The `MULTIPROCESSOR`/uniprocessor pairs of `cpu_number`, `CPU_IS_PRIMARY` and
//!   `CPU_INFO_FOREACH` are one function each: without `MULTIPROCESSOR` `ci_cpuid` is 0,
//!   `curcpu()` is `cpu_info_primary` and `cpu_info_list` holds only it, so the answers are
//!   the C's constants.

use core::arch::asm;
use core::cell::{Cell, UnsafeCell};
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, Ordering};

use crate::arch::arm64::arm64::cpu::OppTable;
use crate::arch::arm64::include::frame::Trapframe;
use crate::arch::arm64::include::pcb::Pcb;
use crate::arch::arm64::include::pmap::Pmap;
use crate::sys::clockintr::Clockqueue;
use crate::sys::device::Device;
use crate::sys::proc::Proc;
use crate::sys::sched::SchedstatePercpu;
use crate::sys::sysctl::{CTLTYPE_INT, CTLTYPE_QUAD, CTLTYPE_STRING};

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

/// A pointer field of `struct cpu_info` that another CPU reads or writes (`ci_curproc`,
/// `ci_curpm`, `ci_next`): an `AtomicPtr` behind the `get`/`set` of a `Cell`, so the C's
/// plain loads and stores stay plain (relaxed) here. `#[repr(transparent)]`: the assembly
/// reaches `ci_curproc` by its offset (`cpuswitch.S`, `exception.S`).
#[repr(transparent)]
pub struct CiPtr<T>(AtomicPtr<T>);

impl<T> CiPtr<T> {
    /// A null pointer.
    pub const fn null() -> Self {
        Self(AtomicPtr::new(ptr::null_mut()))
    }

    /// The pointer `p`.
    pub const fn new(p: *const T) -> Self {
        Self(AtomicPtr::new(p.cast_mut()))
    }

    /// The pointer.
    #[inline]
    pub fn get(&self) -> *const T {
        self.0.load(Ordering::Relaxed)
    }

    /// Stores `p`.
    #[inline]
    pub fn set(&self, p: *const T) {
        self.0.store(p.cast_mut(), Ordering::Relaxed);
    }
}

/// `struct cpu_info`: the per-CPU state.
///
/// Who touches what (`MULTIPROCESSOR`): the identity fields (`ci_dev`, `ci_cpuid`,
/// `ci_mpidr`, `ci_node`, `ci_self`, `ci_el1_stkend`, `ci_ttbr1`, the topology) are written by
/// the boot CPU in `cpu_attach`/`cpu_start_secondary` before the CPU is started, which
/// publishes them (`BootMp::start`'s release store); they are read-only afterwards.
/// `ci_randseed` is written by `cpu_boot_secondary_processors` before `CPUF_GO`. The fields
/// other CPUs read or write while the CPU runs are atomics: `ci_flags` (the `CPUF_*`
/// handshake), `ci_want_resched` (`need_resched` from anywhere), `ci_curproc` and `ci_curpm`
/// (`need_resched`/`cpu_unidle` and `pmap_rollover_asid`, under `pmap_asid_mtx`),
/// `ci_next`, `ci_opp_idx`/`ci_opp_max` (`cpu_opp_setperf` and the cooling device) and
/// `ci_ddb_paused`. Everything else (`ci_cpl`, `ci_ipending`, `ci_idepth`, `ci_curpcb`,
/// the PSCI idle statistics, the panic buffer) belongs to the CPU itself.
#[repr(C)]
pub struct CpuInfo {
    /// Device corresponding to this CPU.
    pub ci_dev: Cell<*const Device>,
    /// The next CPU (`cpu_info_list`).
    pub ci_next: CiPtr<CpuInfo>,
    /// Scheduler state.
    pub ci_schedstate: SchedstatePercpu,
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
    /// `ci_cputype` (`__HAVE_CPU_TOPOLOGY`): `CPUTYP_*`, set by `cpu_classify`.
    pub ci_cputype: Cell<u32>,
    /// `ci_smt_id`.
    pub ci_smt_id: Cell<u32>,
    /// `ci_core_id`.
    pub ci_core_id: Cell<u32>,
    /// `ci_pkg_id`.
    pub ci_pkg_id: Cell<u32>,
    /// The thread on this CPU.
    pub ci_curproc: CiPtr<Proc>,
    /// The thread's pcb.
    pub ci_curpcb: Cell<*const Pcb>,
    /// The active pmap.
    pub ci_curpm: CiPtr<Pmap>,
    /// `ci_randseed`.
    pub ci_randseed: Cell<u32>,
    /// `ci_ctrl`: The CPU control register.
    pub ci_ctrl: Cell<u32>,
    /// `ci_trampoline_vectors`: the EL0 vector table (`cpu.rs`, `TrampolineVectors`).
    pub ci_trampoline_vectors: Cell<u64>,
    /// `ci_cpl`: the current interrupt priority level.
    pub ci_cpl: Cell<u32>,
    /// `ci_ipending`.
    pub ci_ipending: Cell<u32>,
    /// `ci_idepth`: the interrupt nesting depth.
    pub ci_idepth: Cell<u32>,
    /// `DIAGNOSTIC`: the mutex nesting level.
    pub ci_mutex_level: Cell<i32>,
    /// The scheduler asks for a reschedule (`need_resched`, from any CPU).
    pub ci_want_resched: AtomicI32,
    /// `ci_flush_bp`: the branch predictor flush, if the CPU needs one.
    pub ci_flush_bp: Cell<Option<fn()>>,
    /// `ci_serror`: the system error handler, if the CPU has one.
    pub ci_serror: Cell<Option<fn()>>,
    /// `ci_ttbr1`.
    pub ci_ttbr1: Cell<u64>,
    /// `ci_el1_stkend`.
    pub ci_el1_stkend: Cell<usize>,
    /// `ci_psci_idle_latency`.
    pub ci_psci_idle_latency: Cell<u32>,
    /// `ci_psci_idle_param`.
    pub ci_psci_idle_param: Cell<u32>,
    /// `ci_psci_suspend_param`.
    pub ci_psci_suspend_param: Cell<u32>,
    /// `ci_opp_table`: the operating points (`cpu.rs`, `cpu_opp_init`).
    pub ci_opp_table: Cell<*const OppTable>,
    /// `ci_opp_idx` (volatile).
    pub ci_opp_idx: AtomicI32,
    /// `ci_opp_max` (volatile).
    pub ci_opp_max: AtomicI32,
    /// `ci_cpu_supply`: the regulator's phandle.
    pub ci_cpu_supply: Cell<u32>,
    /// `ci_capacity`.
    pub ci_capacity: Cell<u64>,
    /// `ci_prev_sleep`.
    pub ci_prev_sleep: Cell<u64>,
    /// `ci_last_itime`.
    pub ci_last_itime: Cell<u64>,
    // MULTIPROCESSOR: ci_srp_hazards (srp(9), not ported), ci_xcall (NXCALL is 0 on
    // arm64: only psp(4) needs xcall), ci_uvm (__HAVE_UVM_PERCPU, uvm_pmemrange's per-CPU
    // cache, M11a phase 2).
    /// \[a\] `CPUF_*`.
    pub ci_flags: AtomicU32,
    /// `CI_DDB_*` (volatile).
    pub ci_ddb_paused: AtomicI32,
    // ci_gmon, ci_gmonclock: GPROF.
    /// The clock interrupt queue.
    pub ci_queue: Clockqueue,
    /// The first panic message of this CPU.
    pub ci_panicbuf: UnsafeCell<[u8; 512]>,
}

// SAFETY: see the struct's documentation: the fields other CPUs touch are atomics (or own
// their lock, `ci_schedstate`/`ci_queue`); the plain cells are written before the CPU is
// published or only by the CPU itself.
unsafe impl Sync for CpuInfo {}

impl CpuInfo {
    /// A CPU before `cpu_attach`: everything zero, `ci_self` unset.
    pub const fn new() -> Self {
        Self {
            ci_dev: Cell::new(ptr::null()),
            ci_next: CiPtr::null(),
            ci_schedstate: SchedstatePercpu::new(),
            ci_cpuid: Cell::new(0),
            ci_mpidr: Cell::new(0),
            ci_midr: Cell::new(0),
            ci_acpi_proc_id: Cell::new(0),
            ci_node: Cell::new(0),
            ci_self: Cell::new(ptr::null()),
            ci_cputype: Cell::new(0),
            ci_smt_id: Cell::new(0),
            ci_core_id: Cell::new(0),
            ci_pkg_id: Cell::new(0),
            ci_curproc: CiPtr::null(),
            ci_curpcb: Cell::new(ptr::null()),
            ci_curpm: CiPtr::null(),
            ci_randseed: Cell::new(0),
            ci_ctrl: Cell::new(0),
            ci_trampoline_vectors: Cell::new(0),
            ci_cpl: Cell::new(0),
            ci_ipending: Cell::new(0),
            ci_idepth: Cell::new(0),
            ci_mutex_level: Cell::new(0),
            ci_want_resched: AtomicI32::new(0),
            ci_flush_bp: Cell::new(None),
            ci_serror: Cell::new(None),
            ci_ttbr1: Cell::new(0),
            ci_el1_stkend: Cell::new(0),
            ci_psci_idle_latency: Cell::new(0),
            ci_psci_idle_param: Cell::new(0),
            ci_psci_suspend_param: Cell::new(0),
            ci_opp_table: Cell::new(ptr::null()),
            ci_opp_idx: AtomicI32::new(0),
            ci_opp_max: AtomicI32::new(0),
            ci_cpu_supply: Cell::new(0),
            ci_capacity: Cell::new(0),
            ci_prev_sleep: Cell::new(0),
            ci_last_itime: Cell::new(0),
            ci_flags: AtomicU32::new(0),
            ci_ddb_paused: AtomicI32::new(0),
            ci_queue: Clockqueue::new(),
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

/// `MAXCPUS`: 1 without `MULTIPROCESSOR`.
#[cfg(not(feature = "multiprocessor"))]
pub const MAXCPUS: u32 = 1;
/// `MAXCPUS`: 256 with `MULTIPROCESSOR`.
#[cfg(feature = "multiprocessor")]
pub const MAXCPUS: u32 = 256;

/// `CTL_MACHDEP` names (`<machine/cpu.h>`), the ids `machdep.*` sysctls use.
/// `CPU_COMPATIBLE`: compatible property.
pub const CPU_COMPATIBLE: i32 = 1;
/// `CPU_ID_AA64ISAR0`: `ID_AA64ISAR0_EL1`.
pub const CPU_ID_AA64ISAR0: i32 = 2;
/// `CPU_ID_AA64ISAR1`: `ID_AA64ISAR1_EL1`.
pub const CPU_ID_AA64ISAR1: i32 = 3;
/// `CPU_ID_AA64ISAR2`: `ID_AA64ISAR2_EL1`.
pub const CPU_ID_AA64ISAR2: i32 = 4;
/// `CPU_ID_AA64MMFR0`: `ID_AA64MMFR0_EL1`.
pub const CPU_ID_AA64MMFR0: i32 = 5;
/// `CPU_ID_AA64MMFR1`: `ID_AA64MMFR1_EL1`.
pub const CPU_ID_AA64MMFR1: i32 = 6;
/// `CPU_ID_AA64MMFR2`: `ID_AA64MMFR2_EL1`.
pub const CPU_ID_AA64MMFR2: i32 = 7;
/// `CPU_ID_AA64PFR0`: `ID_AA64PFR0_EL1`.
pub const CPU_ID_AA64PFR0: i32 = 8;
/// `CPU_ID_AA64PFR1`: `ID_AA64PFR1_EL1`.
pub const CPU_ID_AA64PFR1: i32 = 9;
/// `CPU_ID_AA64SMFR0`: `ID_AA64SMFR0_EL1`.
pub const CPU_ID_AA64SMFR0: i32 = 10;
/// `CPU_ID_AA64ZFR0`: `ID_AA64ZFR0_EL1`.
pub const CPU_ID_AA64ZFR0: i32 = 11;
/// `CPU_LIDACTION`: action caused by lid close.
pub const CPU_LIDACTION: i32 = 12;
/// `CPU_LED_BLINK`: int: blink leds?.
pub const CPU_LED_BLINK: i32 = 13;
/// `CPU_MAXID`: number of valid machdep ids.
pub const CPU_MAXID: i32 = 14;

/// `CTL_MACHDEP_NAMES`: the `machdep` names `sysctl(8)` knows, indexed by id (`None` for the
/// unused 0). Each is `(name, type)`, the C's `{ "name", CTLTYPE_x }`.
pub const CTL_MACHDEP_NAMES: [Option<(&str, i32)>; CPU_MAXID as usize] = [
    None,
    Some(("compatible", CTLTYPE_STRING)),
    Some(("id_aa64isar0", CTLTYPE_QUAD)),
    Some(("id_aa64isar1", CTLTYPE_QUAD)),
    Some(("id_aa64isar2", CTLTYPE_QUAD)),
    Some(("id_aa64mmfr0", CTLTYPE_QUAD)),
    Some(("id_aa64mmfr1", CTLTYPE_QUAD)),
    Some(("id_aa64mmfr2", CTLTYPE_QUAD)),
    Some(("id_aa64pfr0", CTLTYPE_QUAD)),
    Some(("id_aa64pfr1", CTLTYPE_QUAD)),
    Some(("id_aa64smfr0", CTLTYPE_QUAD)),
    Some(("id_aa64zfr0", CTLTYPE_QUAD)),
    Some(("lidaction", CTLTYPE_INT)),
    Some(("led_blink", CTLTYPE_INT)),
];

/// `struct clockframe`: all the `CLKF_*` macros take a struct clockframe * as an argument.
pub type Clockframe = Trapframe;

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

/// `cpu_number()`: `curcpu()->ci_cpuid` (0 without `MULTIPROCESSOR`, where `ci_cpuid` is 0).
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

/// `CPU_IS_RUNNING(ci)`: `ci_flags & CPUF_RUNNING` with `MULTIPROCESSOR`, always true
/// without.
pub fn cpu_is_running(ci: &CpuInfo) -> bool {
    if cfg!(feature = "multiprocessor") {
        ci.ci_flags.load(Ordering::Relaxed) & CPUF_RUNNING != 0
    } else {
        true
    }
}

/// `CPU_INFO_UNIT(ci)`: `ci_dev ? ci_dev->dv_unit : 0` with `MULTIPROCESSOR`, 0 without.
pub fn cpu_info_unit(ci: &CpuInfo) -> u32 {
    if !cfg!(feature = "multiprocessor") {
        return 0;
    }
    // SAFETY: `cpu_attach` points `ci_dev` at the CPU's device, which is never detached.
    match unsafe { ci.ci_dev.get().as_ref() } {
        Some(dev) => dev.dv_unit.get() as u32,
        None => 0,
    }
}

/// `CPU_BUSY_CYCLE()`: `yield`.
#[inline]
pub fn cpu_busy_cycle() {
    // SAFETY: a hint instruction with no architectural effect.
    unsafe { asm!("yield", options(nomem, nostack, preserves_flags)) };
}

/// `CLKF_USERMODE(frame)`: return TRUE/FALSE (1/0) depending on whether the frame came from
/// USR mode or not.
#[inline]
pub fn clkf_usermode(frame: &Clockframe) -> bool {
    (frame.tf_elr as u64) & (1u64 << 63) == 0
}

/// `CLKF_INTR(frame)`: true if we took the interrupt from inside another interrupt handler.
#[inline]
pub fn clkf_intr(_frame: &Clockframe) -> bool {
    curcpu().ci_idepth.get() > 1
}

/// `CLKF_PC(frame)`: extract the program counter from a clockframe.
#[inline]
pub fn clkf_pc(frame: &Clockframe) -> usize {
    frame.tf_elr as usize
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn ctl_machdep_names_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/arm64/include/cpu.h");
        let ours: &[(&str, i64)] = &[
            ("CPU_COMPATIBLE", i64::from(CPU_COMPATIBLE)),
            ("CPU_ID_AA64ISAR0", i64::from(CPU_ID_AA64ISAR0)),
            ("CPU_ID_AA64ISAR1", i64::from(CPU_ID_AA64ISAR1)),
            ("CPU_ID_AA64ISAR2", i64::from(CPU_ID_AA64ISAR2)),
            ("CPU_ID_AA64MMFR0", i64::from(CPU_ID_AA64MMFR0)),
            ("CPU_ID_AA64MMFR1", i64::from(CPU_ID_AA64MMFR1)),
            ("CPU_ID_AA64MMFR2", i64::from(CPU_ID_AA64MMFR2)),
            ("CPU_ID_AA64PFR0", i64::from(CPU_ID_AA64PFR0)),
            ("CPU_ID_AA64PFR1", i64::from(CPU_ID_AA64PFR1)),
            ("CPU_ID_AA64SMFR0", i64::from(CPU_ID_AA64SMFR0)),
            ("CPU_ID_AA64ZFR0", i64::from(CPU_ID_AA64ZFR0)),
            ("CPU_LIDACTION", i64::from(CPU_LIDACTION)),
            ("CPU_LED_BLINK", i64::from(CPU_LED_BLINK)),
            ("CPU_MAXID", i64::from(CPU_MAXID)),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }

    #[test]
    fn ctl_machdep_names_are_indexed_by_id() {
        let names = &CTL_MACHDEP_NAMES;
        assert_eq!(names.len(), CPU_MAXID as usize);
        assert_eq!(
            names[CPU_COMPATIBLE as usize],
            Some(("compatible", CTLTYPE_STRING))
        );
        assert_eq!(
            names[CPU_LED_BLINK as usize],
            Some(("led_blink", CTLTYPE_INT))
        );
        assert_eq!(names[0], None);
    }
}
/* </TESTS> */
