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

/*-
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz.
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
 *	@(#)cpu.h	5.4 (Berkeley) 5/9/91
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/cpu.h>`: definitions unique to x86-64 cpu support.
//!
//! Upstream: sys/arch/amd64/include/cpu.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `struct cpu_info` (the fields the trap and interrupt
//! paths use), `curcpu()`, `cpu_info_primary` and the `CPUF_*` flags; M5 adds
//! `ci_schedstate`, `ci_queue`, `MAXCPUS`, `CPU_INFO_UNIT`, `struct clockframe` (the
//! `intrframe`) and the `CLKF_*` macros; the TSC timecounter adds `enum cpu_vendor` and the
//! identification fields `identifycpu` fills for it (`ci_vendor` .. `ci_model`). The other
//! identification fields, the sensors and the vmm fields arrive with their subsystems; M13
//! adds the `CTL_MACHDEP` names and `CTL_MACHDEP_NAMES` (`cpu_sysctl`). M11a adds the `MULTIPROCESSOR` part: `ci_func`, `CPU_STARTUP`,
//! `CPU_START_CLEANUP`, `CPU_BUSY_CYCLE` and `CPU_INFO_UNIT` over `ci_dev`.
//!
//! ## Deviations
//! - The fields kept follow the C's order; the ones left out are named in comments. Nothing
//!   reads the struct by a C offset: the entry stubs get their offsets from `offset_of!`.
//! - `curcpu()` reads `%gs:ci_self`, so it is valid only once `cpu_init_msrs` has set
//!   `GS.base` (the C's `locore0.S` does that before `init_x86_64`; here `init_x86_64` does it
//!   first thing; an application processor's `cpu_hatch` does it first thing too).
//! - The fields other CPUs write are atomics (`[a]`): `ci_flags`, `ci_ipis`,
//!   `ci_want_resched` (`need_resched` from another CPU) and `ci_proc_pmap` (read by the TLB
//!   shootdown of another CPU, `pmap_is_active`), and `ci_mwait` (`[a]`, M16e). `ci_acpicpudev`
//!   (`[I]`, set by acpicpu(4) while cold) is an `AtomicPtr` too: every CPU's idle loop reads
//!   it. The `[o]` fields stay `Cell`s: only their
//!   own CPU touches them. The `[I]` fields (`ci_next`, `ci_cpuid`, `ci_apicid`, `ci_func`,
//!   ...) are written by the boot CPU in `cpu_attach` before the CPU is started; the release
//!   of `CPUF_GO` (`cpu_boot_secondary`) and the acquire on `ci_flags` order them.

use core::arch::asm;
use core::cell::{Cell, UnsafeCell};
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, AtomicU64};

use crate::arch::amd64::include::cpuvar::CpuFunctions;
use crate::arch::amd64::include::frame::Intrframe;
use crate::arch::amd64::include::intr::Intrsource;
use crate::arch::amd64::include::intrdefs::{MAX_INTR_SOURCES, NIPL};
use crate::arch::amd64::include::pcb::Pcb;
use crate::arch::amd64::include::pmap::Pmap;
use crate::arch::amd64::include::segments::usermode;
use crate::arch::amd64::include::tss::X86_64Tss;
use crate::sys::clockintr::Clockqueue;
use crate::sys::device::Device;
use crate::sys::proc::Proc;
use crate::sys::sched::SchedstatePercpu;
use crate::sys::sysctl::{CTLTYPE_INT, CTLTYPE_QUAD, CTLTYPE_STRING, CTLTYPE_STRUCT};

/// `enum cpu_vendor`: the vendor `cpu_set_vendor` maps cpuid(0)'s string to.
#[allow(non_camel_case_types, clippy::upper_case_acronyms)] // OpenBSD names, verbatim
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CpuVendor {
    /// `CPUV_UNKNOWN`.
    CPUV_UNKNOWN,
    /// `CPUV_AMD`.
    CPUV_AMD,
    /// `CPUV_INTEL`.
    CPUV_INTEL,
    /// `CPUV_VIA`.
    CPUV_VIA,
}

/// `struct cpu_info`: the per-CPU state (the M4/M5 subset, see the module doc).
#[repr(C)]
pub struct CpuInfo {
    // The beginning of this structure in mapped in the userspace "u-k" page tables, so that
    // these first couple members can be accessed from the trampoline code. The ci_PAGEALIGN
    // member defines where the part that is *not* visible begins, so don't put anything above
    // it that must be kept hidden from userspace!
    /// \[o\] U+K page table.
    pub ci_kern_cr3: Cell<u64>,
    /// \[o\] for U<-->K transition.
    pub ci_scratch: Cell<u64>,
    // ci_PAGEALIGN = ci_dev
    /// \[I\] `struct device`: the `cpu` device, set by `cpu_attach`.
    pub ci_dev: Cell<*const Device>,
    /// \[I\] this structure's own address, what `curcpu()` reads.
    pub ci_self: Cell<*const CpuInfo>,
    /// \[I\] the next CPU.
    pub ci_next: Cell<*const CpuInfo>,
    /// \[I\].
    pub ci_cpuid: Cell<u32>,
    /// \[I\].
    pub ci_apicid: Cell<u32>,
    /// \[I\].
    pub ci_acpi_proc_id: Cell<u32>,
    /// \[o\].
    pub ci_randseed: Cell<u32>,
    /// \[o\] kernel-only stack.
    pub ci_kern_rsp: Cell<u64>,
    /// \[o\] U<-->K trampoline stack.
    pub ci_intr_rsp: Cell<u64>,
    /// \[o\] U-K page table.
    pub ci_user_cr3: Cell<u64>,
    // ci_mds_tmp, ci_mds_buf (Micro-architectural Data Sampling): M6.
    /// \[o\] the thread on this CPU.
    pub ci_curproc: Cell<*const Proc>,
    /// Scheduler state.
    pub ci_schedstate: SchedstatePercpu,
    /// \[a\] active, non-kernel pmap: written by its CPU (`cpu_switchto`, `pmap_activate`),
    /// read by the others' TLB shootdowns (`pmap_is_active`).
    pub ci_proc_pmap: AtomicPtr<Pmap>,
    /// \[o\] last pmap used in userspace.
    pub ci_user_pmap: Cell<*const Pmap>,
    /// \[o\] the thread's pcb.
    pub ci_curpcb: Cell<*const Pcb>,
    /// \[o\] the idle thread's pcb.
    pub ci_idle_pcb: Cell<*const Pcb>,
    /// \[o\] `CPUPF_*`.
    pub ci_pflags: Cell<u32>,
    /// `ci_isources[MAX_INTR_SOURCES]`: the interrupt sources, by stub number.
    pub ci_isources: [Cell<*const Intrsource>; MAX_INTR_SOURCES],
    /// Pending interrupts, by source; the stubs set bits under `cli`, `softintr` atomically.
    pub ci_ipending: AtomicU64,
    /// The current interrupt priority level.
    pub ci_ilevel: Cell<i32>,
    /// The interrupt nesting depth.
    pub ci_idepth: Cell<i32>,
    /// The level of the interrupt being handled.
    pub ci_handled_intr_level: Cell<i32>,
    /// The sources masked at each level.
    pub ci_imask: [Cell<u64>; NIPL],
    /// The sources unmasked at each level.
    pub ci_iunmask: [Cell<u64>; NIPL],
    /// `DIAGNOSTIC`: the mutex nesting level.
    pub ci_mutex_level: Cell<i32>,
    /// \[a\] `CPUF_*`.
    pub ci_flags: AtomicU32,
    /// \[a\] pending IPIs.
    pub ci_ipis: AtomicU32,
    /// \[I\] mapped from cpuid(0).
    pub ci_vendor: Cell<CpuVendor>,
    /// \[I\] cpuid(0).eax.
    pub ci_cpuid_level: Cell<u32>,
    /// \[I\] cpuid(1).edx.
    pub ci_feature_flags: Cell<u32>,
    /// \[I\] cpuid(0x80000001).edx.
    pub ci_feature_eflags: Cell<u32>,
    /// \[I\] `CPUID(7).ebx` (for the SMAP check in the trap handler).
    pub ci_feature_sefflags_ebx: Cell<u32>,
    // ci_feature_sefflags_ecx .. ci_feature_amdsev_edx: the rest of identifycpu.
    /// \[I\] cpuid(6).eax (`TPM_*`).
    pub ci_feature_tpmflags: Cell<u32>,
    /// \[I\] cpuid(0x80000000).eax, the highest extended function.
    pub ci_pnfeatset: Cell<u32>,
    /// \[I\] cpuid(0x80000001).eax.
    pub ci_efeature_eax: Cell<u32>,
    /// \[I\] cpuid(0x80000001).ecx.
    pub ci_efeature_ecx: Cell<u32>,
    /// \[I\] the brand string, cpuid(0x80000002..0x80000004).
    pub ci_brand: Cell<[u32; 12]>,
    /// \[I\] cpuid(1).eax.
    pub ci_signature: Cell<u32>,
    /// \[I\] the family, extended family included.
    pub ci_family: Cell<u32>,
    /// \[I\] the model, extended model included.
    pub ci_model: Cell<u32>,
    /// \[I\] the `clflush` line size.
    pub ci_cflushsz: Cell<u32>,
    /// \[o\] inside an atomic section (copyin/copyout).
    pub ci_inatomic: Cell<i32>,
    // ci_cputype .. ci_pkg_id (topology): M4-b/M5.
    /// \[I\] `ci_func`: how `cpu_start_secondary` starts this CPU (`MULTIPROCESSOR`).
    pub ci_func: Cell<Option<&'static CpuFunctions>>,
    // cpu_setup: M4-b/M5.
    /// \[I\] the acpicpu(4) device of this CPU, set by `acpicpu_attach` while cold.
    pub ci_acpicpudev: AtomicPtr<Device>,
    /// \[a\] `MWAIT_*`: written by this CPU's idle loop and by the CPUs waking it.
    pub ci_mwait: AtomicU32,
    /// \[a\] the scheduler asks for a reschedule; another CPU's `need_resched` sets it.
    pub ci_want_resched: AtomicI32,
    /// \[o\] the TSS.
    pub ci_tss: Cell<*const X86_64Tss>,
    /// \[o\] the GDT.
    pub ci_gdt: Cell<*const u8>,
    /// `CI_DDB_*` (volatile): written by the CPUs entering and leaving ddb.
    pub ci_ddb_paused: AtomicI32,
    // ci_srp_hazards, ci_xcall, ci_uvm (MULTIPROCESSOR), the sensors, gmon, vmm: later.
    /// The clock interrupt queue.
    pub ci_queue: Clockqueue,
    /// The first panic message of this CPU.
    pub ci_panicbuf: UnsafeCell<[u8; 512]>,
}

// SAFETY: the `[o]` fields are touched by their own CPU only (and read by ddb); the fields
// other CPUs write are atomics; the `[I]` fields are written by the boot CPU before the CPU
// they describe runs (see the module's deviations).
unsafe impl Sync for CpuInfo {}

impl CpuInfo {
    /// A CPU before `cpu_attach`: everything zero, `ci_self` unset.
    pub const fn new() -> Self {
        Self {
            ci_kern_cr3: Cell::new(0),
            ci_scratch: Cell::new(0),
            ci_dev: Cell::new(ptr::null()),
            ci_self: Cell::new(ptr::null()),
            ci_next: Cell::new(ptr::null()),
            ci_cpuid: Cell::new(0),
            ci_apicid: Cell::new(0),
            ci_acpi_proc_id: Cell::new(0),
            ci_randseed: Cell::new(0),
            ci_kern_rsp: Cell::new(0),
            ci_intr_rsp: Cell::new(0),
            ci_user_cr3: Cell::new(0),
            ci_curproc: Cell::new(ptr::null()),
            ci_schedstate: SchedstatePercpu::new(),
            ci_proc_pmap: AtomicPtr::new(ptr::null_mut()),
            ci_user_pmap: Cell::new(ptr::null()),
            ci_curpcb: Cell::new(ptr::null()),
            ci_idle_pcb: Cell::new(ptr::null()),
            ci_pflags: Cell::new(0),
            ci_isources: [const { Cell::new(ptr::null()) }; MAX_INTR_SOURCES],
            ci_ipending: AtomicU64::new(0),
            ci_ilevel: Cell::new(0),
            ci_idepth: Cell::new(0),
            ci_handled_intr_level: Cell::new(0),
            ci_imask: [const { Cell::new(0) }; NIPL],
            ci_iunmask: [const { Cell::new(0) }; NIPL],
            ci_mutex_level: Cell::new(0),
            ci_flags: AtomicU32::new(0),
            ci_ipis: AtomicU32::new(0),
            ci_vendor: Cell::new(CpuVendor::CPUV_UNKNOWN),
            ci_cpuid_level: Cell::new(0),
            ci_feature_flags: Cell::new(0),
            ci_feature_eflags: Cell::new(0),
            ci_feature_sefflags_ebx: Cell::new(0),
            ci_feature_tpmflags: Cell::new(0),
            ci_pnfeatset: Cell::new(0),
            ci_efeature_eax: Cell::new(0),
            ci_efeature_ecx: Cell::new(0),
            ci_brand: Cell::new([0; 12]),
            ci_signature: Cell::new(0),
            ci_family: Cell::new(0),
            ci_model: Cell::new(0),
            ci_cflushsz: Cell::new(0),
            ci_inatomic: Cell::new(0),
            ci_func: Cell::new(None),
            ci_acpicpudev: AtomicPtr::new(ptr::null_mut()),
            ci_mwait: AtomicU32::new(0),
            ci_want_resched: AtomicI32::new(0),
            ci_tss: Cell::new(ptr::null()),
            ci_gdt: Cell::new(ptr::null()),
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

/// `MWAIT_IN_IDLE`: don't need IPI to wake.
pub const MWAIT_IN_IDLE: u32 = 0x1;
/// `MWAIT_KEEP_IDLING`: cleared by other cpus to wake me.
pub const MWAIT_KEEP_IDLING: u32 = 0x2;
/// `MWAIT_ONLY`: set if all idle states use mwait.
pub const MWAIT_ONLY: u32 = 0x4;
/// `MWAIT_IDLING`.
pub const MWAIT_IDLING: u32 = MWAIT_IN_IDLE | MWAIT_KEEP_IDLING;

/// `ci_PAGEALIGN`: the offset of the first field hidden from user space.
pub const CI_PAGEALIGN: usize = core::mem::offset_of!(CpuInfo, ci_dev);

/// `CPUPF_USERSEGS`: CPU has curproc's segs and FS.base.
pub const CPUPF_USERSEGS: u32 = 0x01;
/// `CPUPF_USERXSTATE`: CPU has curproc's xsave state.
pub const CPUPF_USERXSTATE: u32 = 0x02;

/// `CPUF_BSP`: CPU is the original BSP.
pub const CPUF_BSP: u32 = 0x0001;
/// `CPUF_AP`: CPU is an AP.
pub const CPUF_AP: u32 = 0x0002;
/// `CPUF_SP`: CPU is only processor.
pub const CPUF_SP: u32 = 0x0004;
/// `CPUF_PRIMARY`: CPU is active primary processor.
pub const CPUF_PRIMARY: u32 = 0x0008;
/// `CPUF_IDENTIFY`: CPU may now identify.
pub const CPUF_IDENTIFY: u32 = 0x0010;
/// `CPUF_IDENTIFIED`: CPU has been identified.
pub const CPUF_IDENTIFIED: u32 = 0x0020;
/// `CPUF_CONST_TSC`: CPU has constant TSC.
pub const CPUF_CONST_TSC: u32 = 0x0040;
/// `CPUF_INVAR_TSC`: CPU has invariant TSC.
pub const CPUF_INVAR_TSC: u32 = 0x0100;
/// `CPUF_PRESENT`: CPU is present.
pub const CPUF_PRESENT: u32 = 0x1000;
/// `CPUF_RUNNING`: CPU is running.
pub const CPUF_RUNNING: u32 = 0x2000;
/// `CPUF_PAUSE`: CPU is paused in DDB.
pub const CPUF_PAUSE: u32 = 0x4000;
/// `CPUF_GO`: CPU should start running.
pub const CPUF_GO: u32 = 0x8000;
/// `CPUF_PARK`: CPU should self-park in real mode.
pub const CPUF_PARK: u32 = 0x10000;
/// `CPUF_VMM`: CPU is executing in VMM mode.
pub const CPUF_VMM: u32 = 0x20000;

/// `MAXCPUS`: 1 without `MULTIPROCESSOR`.
#[cfg(not(feature = "multiprocessor"))]
pub const MAXCPUS: u32 = 1;
/// `MAXCPUS`: 255 with `MULTIPROCESSOR` (the xAPIC broadcast ID is the 256th).
#[cfg(feature = "multiprocessor")]
pub const MAXCPUS: u32 = 255;

/// `CTL_MACHDEP` names (`<machine/cpu.h>`), the ids `machdep.*` sysctls use.
/// `CPU_CONSDEV`: dev_t: console terminal device.
pub const CPU_CONSDEV: i32 = 1;
/// `CPU_BIOS`: BIOS variables.
pub const CPU_BIOS: i32 = 2;
/// `CPU_BLK2CHR`: convert blk maj into chr one.
pub const CPU_BLK2CHR: i32 = 3;
/// `CPU_CHR2BLK`: convert chr maj into blk one.
pub const CPU_CHR2BLK: i32 = 4;
/// `CPU_ALLOWAPERTURE`: allow mmap of /dev/xf86.
pub const CPU_ALLOWAPERTURE: i32 = 5;
/// `CPU_CPUVENDOR`: cpuid vendor string.
pub const CPU_CPUVENDOR: i32 = 6;
/// `CPU_CPUID`: cpuid.
pub const CPU_CPUID: i32 = 7;
/// `CPU_CPUFEATURE`: cpuid features.
pub const CPU_CPUFEATURE: i32 = 8;
/// `CPU_KBDRESET`: keyboard reset under pcvt.
pub const CPU_KBDRESET: i32 = 10;
/// `CPU_XCRYPT`: supports VIA xcrypt in userland.
pub const CPU_XCRYPT: i32 = 12;
/// `CPU_HIBERNATEDELAY`: hibernate delay after suspend.
pub const CPU_HIBERNATEDELAY: i32 = 13;
/// `CPU_LIDACTION`: action caused by lid close.
pub const CPU_LIDACTION: i32 = 14;
/// `CPU_FORCEUKBD`: Force ukbd(4) as console keyboard.
pub const CPU_FORCEUKBD: i32 = 15;
/// `CPU_TSCFREQ`: TSC frequency.
pub const CPU_TSCFREQ: i32 = 16;
/// `CPU_INVARIANTTSC`: has invariant TSC.
pub const CPU_INVARIANTTSC: i32 = 17;
/// `CPU_PWRACTION`: action caused by power button.
pub const CPU_PWRACTION: i32 = 18;
/// `CPU_RETPOLINE`: cpu requires retpoline pattern.
pub const CPU_RETPOLINE: i32 = 19;
/// `CPU_VMMODE`: virtualization mode.
pub const CPU_VMMODE: i32 = 20;
/// `CPU_MAXID`: number of valid machdep ids.
pub const CPU_MAXID: i32 = 21;

/// `CTL_MACHDEP_NAMES`: the `machdep` names `sysctl(8)` knows, indexed by id (`None` for the
/// unused 0, 9 and 11). Each is `(name, type)`, the C's `{ "name", CTLTYPE_x }`.
pub const CTL_MACHDEP_NAMES: [Option<(&str, i32)>; CPU_MAXID as usize] = [
    None,
    Some(("console_device", CTLTYPE_STRUCT)),
    Some(("bios", CTLTYPE_INT)),
    Some(("blk2chr", CTLTYPE_STRUCT)),
    Some(("chr2blk", CTLTYPE_STRUCT)),
    Some(("allowaperture", CTLTYPE_INT)),
    Some(("cpuvendor", CTLTYPE_STRING)),
    Some(("cpuid", CTLTYPE_INT)),
    Some(("cpufeature", CTLTYPE_INT)),
    None,
    Some(("kbdreset", CTLTYPE_INT)),
    None,
    Some(("xcrypt", CTLTYPE_INT)),
    Some(("hibernatedelay", CTLTYPE_INT)),
    Some(("lidaction", CTLTYPE_INT)),
    Some(("forceukbd", CTLTYPE_INT)),
    Some(("tscfreq", CTLTYPE_QUAD)),
    Some(("invarianttsc", CTLTYPE_INT)),
    Some(("pwraction", CTLTYPE_INT)),
    Some(("retpoline", CTLTYPE_INT)),
    Some(("vmmode", CTLTYPE_STRING)),
];

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

/// `struct clockframe`: arguments to hardclock, softclock and statclock encapsulate the
/// previous machine state in an opaque clockframe; for now, use generic intrframe.
pub type Clockframe = Intrframe;

/// `curcpu()`: this CPU's `cpu_info`, through `%gs:ci_self` (see the module's deviations).
#[inline]
pub fn curcpu() -> &'static CpuInfo {
    let ci: *const CpuInfo;
    // SAFETY: a read through GS, which `cpu_init_msrs` pointed at this CPU's cpu_info.
    unsafe {
        asm!(
            "mov {}, gs:[{off}]",
            out(reg) ci,
            off = const core::mem::offset_of!(CpuInfo, ci_self),
            options(nostack, preserves_flags, readonly)
        )
    };
    // SAFETY: `ci_self` names a static cpu_info, which lives forever.
    unsafe { &*ci }
}

/// `cpu_number()`.
#[inline]
pub fn cpu_number() -> u32 {
    curcpu().ci_cpuid.get()
}

/// `cpu_info_primary`: the boot CPU's `cpu_info`.
pub fn cpu_info_primary() -> &'static CpuInfo {
    &crate::arch::amd64::amd64::cpu::CPU_INFO_FULL_PRIMARY.cif_cpu
}

/// `CPU_IS_PRIMARY(ci)`.
pub fn cpu_is_primary(ci: &CpuInfo) -> bool {
    ci.ci_flags.load(core::sync::atomic::Ordering::Relaxed) & CPUF_PRIMARY != 0
}

/// `CPU_IS_RUNNING(ci)`.
pub fn cpu_is_running(ci: &CpuInfo) -> bool {
    ci.ci_flags.load(core::sync::atomic::Ordering::Relaxed) & CPUF_RUNNING != 0
}

/// `CPU_INFO_UNIT(ci)`: `ci_dev->dv_unit`, 0 without a device.
pub fn cpu_info_unit(ci: &CpuInfo) -> u32 {
    // SAFETY: `ci_dev` is null or the CPU's attached device, which is never freed.
    unsafe { ci.ci_dev.get().as_ref() }.map_or(0, |d| d.dv_unit.get().max(0) as u32)
}

/// `CPU_STARTUP(ci)`: `ci->ci_func->start(ci)` (`MULTIPROCESSOR`); 0 when started.
#[cfg(feature = "multiprocessor")]
pub fn cpu_startup_ci(ci: &CpuInfo) -> i32 {
    match ci.ci_func.get().and_then(|f| f.start) {
        Some(start) => start(ci),
        None => crate::sys::errno::Errno::ENXIO as i32,
    }
}

/// `CPU_START_CLEANUP(ci)`: `ci->ci_func->cleanup(ci)` (`MULTIPROCESSOR`).
#[cfg(feature = "multiprocessor")]
pub fn cpu_start_cleanup(ci: &CpuInfo) {
    if let Some(cleanup) = ci.ci_func.get().and_then(|f| f.cleanup) {
        cleanup(ci);
    }
}

/// `CPU_BUSY_CYCLE()`: `pause` with `MULTIPROCESSOR`, a compiler barrier without.
#[inline]
pub fn cpu_busy_cycle() {
    #[cfg(feature = "multiprocessor")]
    core::hint::spin_loop();
    #[cfg(not(feature = "multiprocessor"))]
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// `CLKF_USERMODE(frame)`.
#[inline]
pub fn clkf_usermode(frame: &Clockframe) -> bool {
    usermode(frame.if_cs as u64)
}

/// `CLKF_PC(frame)`.
#[inline]
pub fn clkf_pc(frame: &Clockframe) -> usize {
    frame.if_rip as usize
}

/// `CLKF_INTR(frame)`: the clock interrupted another interrupt handler.
#[inline]
pub fn clkf_intr(_frame: &Clockframe) -> bool {
    curcpu().ci_idepth.get() > 1
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn ctl_machdep_names_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/cpu.h");
        let ours: &[(&str, i64)] = &[
            ("CPU_CONSDEV", i64::from(CPU_CONSDEV)),
            ("CPU_BIOS", i64::from(CPU_BIOS)),
            ("CPU_BLK2CHR", i64::from(CPU_BLK2CHR)),
            ("CPU_CHR2BLK", i64::from(CPU_CHR2BLK)),
            ("CPU_ALLOWAPERTURE", i64::from(CPU_ALLOWAPERTURE)),
            ("CPU_CPUVENDOR", i64::from(CPU_CPUVENDOR)),
            ("CPU_CPUID", i64::from(CPU_CPUID)),
            ("CPU_CPUFEATURE", i64::from(CPU_CPUFEATURE)),
            ("CPU_KBDRESET", i64::from(CPU_KBDRESET)),
            ("CPU_XCRYPT", i64::from(CPU_XCRYPT)),
            ("CPU_HIBERNATEDELAY", i64::from(CPU_HIBERNATEDELAY)),
            ("CPU_LIDACTION", i64::from(CPU_LIDACTION)),
            ("CPU_FORCEUKBD", i64::from(CPU_FORCEUKBD)),
            ("CPU_TSCFREQ", i64::from(CPU_TSCFREQ)),
            ("CPU_INVARIANTTSC", i64::from(CPU_INVARIANTTSC)),
            ("CPU_PWRACTION", i64::from(CPU_PWRACTION)),
            ("CPU_RETPOLINE", i64::from(CPU_RETPOLINE)),
            ("CPU_VMMODE", i64::from(CPU_VMMODE)),
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
            names[CPU_LIDACTION as usize],
            Some(("lidaction", CTLTYPE_INT))
        );
        assert_eq!(names[CPU_TSCFREQ as usize], Some(("tscfreq", CTLTYPE_QUAD)));
        assert_eq!(names[CPU_VMMODE as usize], Some(("vmmode", CTLTYPE_STRING)));
        assert_eq!(names[9], None);
        assert_eq!(names[11], None);
    }
}
/* </TESTS> */
