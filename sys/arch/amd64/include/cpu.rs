/*	$OpenBSD: cpu.h,v 1.186 2026/09/08 21:01:59 daniel Exp $	*/
/*	$NetBSD: cpu.h,v 1.1 2003/04/26 18:39:39 fvdl Exp $	*/

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

//! amd64 `<machine/cpu.h>`: definitions unique to x86-64 cpu support.
//!
//! Upstream: sys/arch/amd64/include/cpu.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `struct cpu_info` (the fields the trap and interrupt
//! paths use), `curcpu()`, `cpu_info_primary` and the `CPUF_*` flags. The CPU identification
//! fields, the scheduler state (M5), the sensors, the vmm fields and the `CTL_MACHDEP` names
//! arrive with their subsystems.
//!
//! ## Deviations
//! - The fields kept follow the C's order; the ones left out are named in comments. Nothing
//!   reads the struct by a C offset: the entry stubs get their offsets from `offset_of!`.
//! - `curcpu()` reads `%gs:ci_self`, so it is valid only once `cpu_init_msrs` has set
//!   `GS.base` (the C's `locore0.S` does that before `init_x86_64`; here `init_x86_64` does it
//!   first thing).

use core::arch::asm;
use core::cell::{Cell, UnsafeCell};
use core::ptr;
use core::sync::atomic::AtomicU32;

use crate::arch::amd64::include::intrdefs::NIPL;
use crate::arch::amd64::include::pmap::Pmap;
use crate::arch::amd64::include::tss::X86_64Tss;

/// `struct cpu_info`: the per-CPU state (the M4 subset, see the module doc).
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
    /// \[I\] `struct device` (M6).
    pub ci_dev: Cell<*const ()>,
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
    /// \[o\] `struct proc` (M5).
    pub ci_curproc: Cell<*const ()>,
    // ci_schedstate: M5.
    /// Active, non-kernel pmap.
    pub ci_proc_pmap: Cell<*const Pmap>,
    /// \[o\] last pmap used in userspace.
    pub ci_user_pmap: Cell<*const Pmap>,
    /// \[o\] `struct pcb` (M5).
    pub ci_curpcb: Cell<*const ()>,
    /// \[o\] `struct pcb` (M5).
    pub ci_idle_pcb: Cell<*const ()>,
    /// \[o\] `CPUPF_*`.
    pub ci_pflags: Cell<u32>,
    // ci_isources[MAX_INTR_SOURCES]: with intr.c.
    /// Pending interrupts, by source.
    pub ci_ipending: Cell<u64>,
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
    // ci_vendor .. ci_model (CPU identification, identifycpu): M4-b.
    /// \[I\] `CPUID(7).ebx` (for the SMAP check in the trap handler).
    pub ci_feature_sefflags_ebx: Cell<u32>,
    /// \[I\] the `clflush` line size.
    pub ci_cflushsz: Cell<u32>,
    /// \[o\] inside an atomic section (copyin/copyout).
    pub ci_inatomic: Cell<i32>,
    // ci_cputype .. ci_mwait (topology, cpu_functions, acpi, mwait): M4-b/M5.
    /// The scheduler asks for a reschedule.
    pub ci_want_resched: Cell<i32>,
    /// \[o\] the TSS.
    pub ci_tss: Cell<*const X86_64Tss>,
    /// \[o\] the GDT.
    pub ci_gdt: Cell<*const u8>,
    /// `CI_DDB_*`.
    pub ci_ddb_paused: Cell<i32>,
    // ci_srp_hazards, ci_xcall, ci_uvm (MULTIPROCESSOR), the sensors, gmon, vmm: later.
    /// The first panic message of this CPU.
    pub ci_panicbuf: UnsafeCell<[u8; 512]>,
}

// SAFETY: one CPU's state, touched by that CPU (and read by ddb); the boot CPU is alone.
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
            ci_proc_pmap: Cell::new(ptr::null()),
            ci_user_pmap: Cell::new(ptr::null()),
            ci_curpcb: Cell::new(ptr::null()),
            ci_idle_pcb: Cell::new(ptr::null()),
            ci_pflags: Cell::new(0),
            ci_ipending: Cell::new(0),
            ci_ilevel: Cell::new(0),
            ci_idepth: Cell::new(0),
            ci_handled_intr_level: Cell::new(0),
            ci_imask: [const { Cell::new(0) }; NIPL],
            ci_iunmask: [const { Cell::new(0) }; NIPL],
            ci_mutex_level: Cell::new(0),
            ci_flags: AtomicU32::new(0),
            ci_ipis: AtomicU32::new(0),
            ci_feature_sefflags_ebx: Cell::new(0),
            ci_cflushsz: Cell::new(0),
            ci_inatomic: Cell::new(0),
            ci_want_resched: Cell::new(0),
            ci_tss: Cell::new(ptr::null()),
            ci_gdt: Cell::new(ptr::null()),
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
