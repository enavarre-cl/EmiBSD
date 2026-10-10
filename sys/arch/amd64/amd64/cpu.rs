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
 * Copyright (c) 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by RedBack Networks Inc.
 *
 * Author: Bill Sommerfeld
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * Copyright (c) 1999 Stefan Grefen
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by the NetBSD
 *      Foundation, Inc. and its contributors.
 * 4. Neither the name of The NetBSD Foundation nor the names of its
 *    contributors may be used to endorse or promote products derived
 *    from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 CPU attachment and per-CPU setup: `arch/amd64/amd64/cpu.c`.
//!
//! Upstream: sys/arch/amd64/amd64/cpu.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `cpu_info_full_primary`,
//! `cpu_init_msrs` and `cpu_enter_pages`'s TSS part; M7b the `cpu` device (`struct
//! cpu_softc`, `cpu_ca`, `cpu_cd`, `cpu_match`, `cpu_attach`) for autoconfiguration, with
//! `cpu_init`. M11a ports the `MULTIPROCESSOR` half: `cpu_info[]`, `mp_cpu_funcs`
//! (`mp_cpu_start`, `mp_cpu_start_cleanup`), the application processor's `cpu_attach`
//! (its `cpu_info_full`, idle pcb, `sched_init_cpu`, `ncpus`, `cpu_info_list`),
//! `cpu_boot_secondary_processors`, `cpu_start_secondary`, `cpu_boot_secondary`,
//! `cpu_hatch`, `cpu_init`'s `CPUF_RUNNING` and `wbinvd_on_all_cpus`. `patinit`, the
//! MDS/`cpu_fix_msrs` work and `cpu_debug_dump` (ddb, M11c) come later. M16e ports
//! `cpu_init_mwait` and `cpu_idle_mwait_cycle` (with `cpu_mwait_size`/`cpu_mwait_states`),
//! for acpicpu(4)'s idle states. M16d ports `rdrand` and `rdrand_tmo`, the entropy source
//! `cpu_configure` starts (the C's uninitialised sample is 0 where the CPU has neither
//! `RDSEED` nor `RDRAND`).
//!
//! ## Deviations
//! - `cpu_attach` reports what it cannot do yet: `cpu_fix_msrs`,
//!   `mem_range_attach` (`MTRR`) and `cpu_init_vmm`. Without
//!   `MULTIPROCESSOR` an application processor (never attached: mainbus attaches the boot
//!   CPU alone) is reported instead of getting a `km_alloc`ed `cpu_info_full`. `cpu_ca` has
//!   no `cpu_activate` (suspend/resume): `config_suspend` walks the CPU's children instead.
//! - `MULTIPROCESSOR` after a boot by boot(8) (M14): the processors are the MADT's enabled
//!   LAPIC/x2APIC entries (`mp_madt_cpus`, standing for acpimadt0 before autoconfiguration),
//!   and `BootMp::start` is the C's `mp_cpu_start` (`mp_cpu_start_tramp`: NVRAM reset code,
//!   the 40:67 vector written through the direct map, INIT, two STARTUP IPIs), the
//!   processor running `mptramp.S` into `cpu_hatch`, which loads the IDT after `CPUF_GO` as
//!   in C. `cpu_start_secondary` maps the trampoline pages and `mp_cpu_start_cleanup`
//!   restores the NVRAM byte, as in C.
//! - `MULTIPROCESSOR` under Limine: the bootloader starts the application processors
//!   (`stand`, Limine's MP request), so `mp_cpu_start` (`CPU_STARTUP`) is
//!   `BootMp::start` for the processor whose hardware ID is `ci_apicid`, with the
//!   `cpu_info` as its argument, in place of the warm-reset vector and the INIT/STARTUP
//!   IPIs; `mp_cpu_start_cleanup` has no NVRAM reset byte to restore. `init_x86_64` keeps the
//!   `BootMp` (`BOOT_MP`). The processor enters `cpu_hatch_entry` (`Cpu::cpu_hatch`) on the
//!   bootloader's 64 KiB stack, in long mode on the bootloader's GDT, with no IDT and
//!   interrupts masked; `cpu_hatch_entry` does what `mptramp.S`'s `cpu_spinup_finish` did
//!   (x2APIC mode when the boot CPU runs it, the CPU's own GDT, `CR3` = the kernel pmap,
//!   `CR0_DEFAULT`, `EFER.NXE` when `CPUID` has it, the idle pcb's stack) and loads the IDT
//!   first, before anything can fault: the C loads it in `cpu_hatch`, after `CPUF_GO`.
//! - The TSC synchronisation test (`tsc_test_sync_bp`/`tsc_test_sync_ap`) runs where the C
//!   runs it (M11b); under feature `qemu` `cpu_start_secondary` then prints a verdict line
//!   per application processor (`tsc.rs`'s deviations). `cpu_ucode_apply`, `cpu_tsx_disable`
//!   and the AP's `cpu_fix_msrs` are reported; `HIBERNATE`, `NPVBUS` and the memory range
//!   `initAP` are not configured. `mp_verbose` is off.
//! - `cpu_boot_secondary_processors` ends with `x86_ipi_selftest` under feature `qemu`
//!   (not in the C, `ipi.rs`).
//! - `cpu_init` sets `CR4_DEFAULT` (with `CR4_OSFXSR`: user SSE) and fills
//!   `fpu_cleandata`. Its CPUID-dependent bits wait for `identifycpu`: `CR4_SMEP`,
//!   `CR4_SMAP`, `CR4_UMIP`, `CR4_PKE` (`pg_xo`), `CR4_PCIDE` (`pmap_use_pcid` is 0) and the
//!   XSAVE setup (`CR4_OSXSAVE`, `xsave_mask`, `fpu_save_len`, XSAVES), reported; the FPU
//!   then uses `fxsave` (`amd64/fpu.rs`). `cpu_setup` (vendor quirks) is identcpu's.
//! - `cpu_match`'s SEV-ES check (`cpu_sev_guestmode`, not ported) only refuses units 1 and
//!   up, which `MAXCPUS` (1 without `MULTIPROCESSOR`) has already refused.
//! - `cpu_init_msrs` writes 0 to `MSR_LSTAR`: there is no `Xsyscall` until user mode (M6).
//!   `patinit` (the PAT MSR for write-combining) is reported as unported.
//! - `cpu_enter_pages` sets up the TSS stacks only: `pmap_enter_special` (the u-k mappings of
//!   the Meltdown mitigation) waits for M6.

use core::arch::asm;
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use libkern::StaticCell;

use crate::arch::amd64::amd64::fpu::XSAVE_MASK;
use crate::arch::amd64::amd64::identcpu::{HAS_RDRAND, HAS_RDSEED, identifycpu};
use crate::arch::amd64::amd64::intr::cpu_intr_init;
use crate::arch::amd64::amd64::lapic::{lapic_calibrate_timer, lapic_enable};
use crate::arch::amd64::amd64::locore::Xsyscall;
use crate::arch::amd64::include::cpu::{
    CPUF_BSP, CPUF_PRESENT, CPUF_PRIMARY, CPUF_SP, CpuInfo, CpuVendor, MAXCPUS, MWAIT_IDLING,
    MWAIT_ONLY, cpu_info_primary, curcpu,
};
use crate::arch::amd64::include::cpu_full::{
    CpuInfoFull, DBLFLT_STACK_WORDS, NMI_STACK_WORDS, TRAMP_STACK_WORDS,
};
use crate::arch::amd64::include::cpufunc::{
    lcr4, monitor, mwait, rcr4, rdmsr, rdtsc, read_rflags, wrmsr,
};
use crate::arch::amd64::include::cpuvar::{CPU_ROLE_AP, CPU_ROLE_BP, CPU_ROLE_SP, CpuAttachArgs};
use crate::arch::amd64::include::fpu::{
    INITIAL_MXCSR, INITIAL_NPXCW, Savefpu, fpu_cleandata, fpureset, fpusave, xrstor_user,
};
use crate::arch::amd64::include::frame::IretqFrame;
use crate::arch::amd64::include::intrdefs::IPL_NONE;
use crate::arch::amd64::include::psl::{PSL_AC, PSL_C, PSL_D, PSL_I, PSL_NT, PSL_T};
use crate::arch::amd64::include::segments::{GCODE_SEL, GUDATA_SEL, SEL_KPL, SEL_UPL, gsel};
use crate::arch::amd64::include::specialreg::{
    CPUIDECX_MWAIT, CR4_DEFAULT, EFER_SCE, MSR_CSTAR, MSR_EFER, MSR_FSBASE, MSR_GSBASE,
    MSR_KERNELGSBASE, MSR_LSTAR, MSR_SFMASK, MSR_STAR, cpuid,
};
use crate::arch::amd64::include::tss::X86_64Tss;
use crate::dev::rnd::enqueue_randomness;
use crate::kern::kern_timeout::timeout_add_msec;
use crate::kern::subr_prf::{Str, panic, printf};
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::sched::cpu_is_idle;
use crate::sys::timeout::Timeout;
use crate::unported;

#[cfg(all(feature = "multiprocessor", feature = "qemu"))]
use crate::arch::amd64::amd64::tsc::{tsc_report_verdict, tsc_sync_testable};
#[cfg(feature = "multiprocessor")]
use {
    crate::arch::amd64::amd64::autoconf::COLD,
    crate::arch::amd64::amd64::fpu::fpuinit,
    crate::arch::amd64::amd64::gdt::gdt_init_cpu,
    crate::arch::amd64::amd64::ipi::x86_broadcast_ipi,
    crate::arch::amd64::amd64::lapic::{
        X2APIC_ENABLED, lapic_cpu_number, lapic_set_lvt, lapic_startclock,
    },
    crate::arch::amd64::amd64::locore::lgdt,
    crate::arch::amd64::amd64::machdep::{cpu_init_idt, cpu_set_vendor, delay, setregion},
    crate::arch::amd64::amd64::pmap::{pmap_kenter_pa, pmap_kernel, pmap_kremove},
    crate::arch::amd64::amd64::tsc::{tsc_test_sync_ap, tsc_test_sync_bp},
    crate::arch::amd64::include::cpu::{
        CPUF_AP, CPUF_GO, CPUF_IDENTIFIED, CPUF_IDENTIFY, CPUF_RUNNING, cpu_is_primary,
        cpu_start_cleanup, cpu_startup_ci,
    },
    crate::arch::amd64::include::cpufunc::{
        intr_disable, intr_enable, intr_restore, lcr0, lcr3, lcr8, lldt, wbinvd,
    },
    crate::arch::amd64::include::cpuvar::CpuFunctions,
    crate::arch::amd64::include::intrdefs::X86_IPI_WBINVD,
    crate::arch::amd64::include::param::{PAGE_SIZE, USPACE},
    crate::arch::amd64::include::pcb::Pcb,
    crate::arch::amd64::include::segments::{GDT_SIZE, RegionDescriptor},
    crate::arch::amd64::include::specialreg::{
        APICBASE_ENABLE_X2APIC, CPUID_NXE, CR0_DEFAULT, CR4_PGE, EFER_NXE, MSR_APICBASE,
    },
    crate::dev::rnd::arc4random,
    crate::kern::init_main::NCPUS,
    crate::kern::kern_clockintr::clockqueue_init,
    crate::kern::kern_sched::{sched_init_cpu, sched_toidle},
    crate::machine::bootinfo::BootCpu,
    crate::machine::bootinfo::BootMp,
    crate::machine::intr::{splhigh, splx},
    crate::sys::errno::Errno,
    crate::uvm::uvm_km::{KD_NOWAIT, KD_WAITOK, KP_DIRTY, KP_ZERO, KV_ANY, km_alloc},
    core::sync::atomic::AtomicPtr,
};

/// `struct cpu_softc`.
#[repr(C)]
pub struct CpuSoftc {
    /// `sc_dev`: device tree glue.
    pub sc_dev: Device,
    /// `sc_info`: pointer to CPU info.
    pub sc_info: Cell<*const CpuInfo>,
}

// SAFETY: `#[repr(C)]`, the device first, and a null pointer is the all-zero `sc_info`.
unsafe impl Softc for CpuSoftc {}

/// `cpuid_level`: MIN cpuid(0).eax.
pub static CPUID_LEVEL: AtomicU32 = AtomicU32::new(0);
/// `cpu_sev_guestmode`: `MSR_SEV_STATUS`'s `SEV_STAT_*` bits when running as an SEV guest. The
/// SEV probe (`identcpu.c`) is not ported, so this stays 0.
pub static CPU_SEV_GUESTMODE: AtomicI32 = AtomicI32::new(0);
/// `need_retpoline`: most systems need retpoline. The C clears it when `identifycpu` patches
/// in the IBRS/IBT-safe jumps (`codepatch_replace`), which is not ported, so it stays 1.
pub static NEED_RETPOLINE: AtomicI32 = AtomicI32::new(1);
/// `cpu_vendor`: CPU0's cpuid(0).e\[bdc\]x, \0. Written once by `init_x86_64` (the C's
/// `locore0.S`) before anything reads it.
pub static CPU_VENDOR: StaticCell<[u8; 16]> = StaticCell::new([0; 16]);
/// `cpu_id`: cpuid(1).eax.
pub static CPU_ID: AtomicU32 = AtomicU32::new(0);
/// `cpu_ebxfeature`: cpuid(1).ebx.
pub static CPU_EBXFEATURE: AtomicU32 = AtomicU32::new(0);
/// `cpu_ecxfeature`: INTERSECTION(cpuid(1).ecx).
pub static CPU_ECXFEATURE: AtomicU32 = AtomicU32::new(0);
/// `cpu_feature`: cpuid(1).edx.
pub static CPU_FEATURE: AtomicU32 = AtomicU32::new(0);
/// `ecpu_ecxfeature`: cpuid(0x80000001).ecx.
pub static ECPU_ECXFEATURE: AtomicU32 = AtomicU32::new(0);
/// `cpu_mwait_size`: the largest monitor line size, 0 when `mwait` is not used.
pub static CPU_MWAIT_SIZE: AtomicU32 = AtomicU32::new(0);
/// `cpu_mwait_states`: cpuid(5).edx, the number of `mwait` sub-states of each C-state, four
/// bits each.
pub static CPU_MWAIT_STATES: AtomicU32 = AtomicU32::new(0);

/// `cpu_ca`.
pub static CPU_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<CpuSoftc>(),
    ca_match: Some(cpu_match),
    ca_attach: cpu_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `cpu_cd`.
pub static CPU_CD: Cfdriver = Cfdriver::new(b"cpu", DV_DULL, CD_COCOVM);

/// `cpu_info_full_primary`: the boot CPU's pages; `ci_self` and `ci_flags` are set by
/// `init_x86_64` (the C initialises them statically).
pub static CPU_INFO_FULL_PRIMARY: CpuInfoFull = CpuInfoFull::new();

/// `mp_cpu_funcs`: how `cpu_start_secondary` starts an application processor
/// (`MULTIPROCESSOR`; see the module's deviations).
#[cfg(feature = "multiprocessor")]
pub static MP_CPU_FUNCS: CpuFunctions = CpuFunctions {
    start: Some(mp_cpu_start),
    stop: None,
    cleanup: Some(mp_cpu_start_cleanup),
};

/// `cpu_info[MAXCPUS]`: the CPUs by `ci_cpuid` (`MULTIPROCESSOR`); `cpu_info_primary_init`
/// puts the primary at 0, `cpu_attach` the others. Must be statically-allocated because
/// curproc, etc. are used early.
#[cfg(feature = "multiprocessor")]
pub static CPU_INFO: [AtomicPtr<CpuInfo>; MAXCPUS as usize] =
    [const { AtomicPtr::new(ptr::null_mut()) }; MAXCPUS as usize];

/// The processors the bootloader found and how to start them (`MULTIPROCESSOR`): kept by
/// `init_x86_64` from `BootInfo::mp`, read by `mainbus_attach` and `mp_cpu_start`.
#[cfg(feature = "multiprocessor")]
pub static BOOT_MP: StaticCell<Option<BootMp>> = StaticCell::new(None);

/// Whether `map_tramps` installed `mptramp.S`'s trampoline (a boot by boot(8)): then
/// `cpu_start_secondary` maps its pages around the start and `mp_cpu_start_cleanup` puts
/// the NVRAM reset byte back, as in C.
#[cfg(feature = "multiprocessor")]
pub static MP_TRAMP_INSTALLED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// The processors the firmware's MADT lists, after a boot by boot(8) (`mp_madt_cpus`):
/// `[0, MADT_NCPUS)` is filled once, before `BOOT_MP` is read.
#[cfg(feature = "multiprocessor")]
static MADT_CPUS: StaticCell<[BootCpu; MAXCPUS as usize]> = StaticCell::new(
    [BootCpu {
        processor_id: 0,
        hwid: 0,
    }; MAXCPUS as usize],
);
/// How many entries of `MADT_CPUS` are filled.
#[cfg(feature = "multiprocessor")]
static MADT_NCPUS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// `cpu_suspended`: set while the boot processor idles in the S0 suspend loop
/// (`cpu_suspend_primary`, `SUSPEND`, not ported: nothing sets it yet); an ACPI wake event
/// clears it (`acpi.c`, through `machine::cpu_suspended`).
pub static CPU_SUSPENDED: AtomicI32 = AtomicI32::new(0);
/// `rdrand_tmo`: runs [`rdrand`] every 10 ms (`cpu_configure` sets it with itself as the
/// argument).
pub static RDRAND_TMO: Timeout = Timeout::new(rdrand, ptr::null_mut());

/// `cpu_info_list`: the CPUs, the primary first.
pub fn cpu_info_list() -> &'static CpuInfo {
    &CPU_INFO_FULL_PRIMARY.cif_cpu
}

/// What the C's static initialiser of `cpu_info_full_primary` sets: `ci_self` and
/// `CPUF_PRIMARY`.
pub fn cpu_info_primary_init() {
    let ci = &CPU_INFO_FULL_PRIMARY.cif_cpu;
    ci.ci_self.set(ptr::from_ref(ci));
    ci.ci_flags
        .store(CPUF_PRIMARY, core::sync::atomic::Ordering::Relaxed);
    // cpu_info[MAXCPUS] = { &cpu_info_primary }
    #[cfg(feature = "multiprocessor")]
    CPU_INFO[0].store(ptr::from_ref(ci).cast_mut(), Ordering::Release);
}

/// `cpu_match`: the attach arguments name a `cpu` and the unit fits `MAXCPUS`.
pub fn cpu_match(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();
    // SAFETY: mainbus's (and mpbios's, acpimadt's) attach arguments all start with the name
    // (`caa_name`, `mba_busname`), which is all this reads.
    let caa_name = unsafe { *aux.cast::<&'static [u8]>() };

    if caa_name != cf.cf_driver.cd_name {
        return 0;
    }

    if cf.cf_unit.get() as u32 >= MAXCPUS {
        return 0;
    }

    // XXX We don't support MP with SEV-ES, yet: see the module's deviations.

    1
}

/// `cpu_idle_mwait_cycle`: `cpu_idle_cycle_fcn` on a CPU with `mwait`: waits in `mwait`
/// on `ci_mwait`, which `cpu_kick` and `cpu_unidle` clear to wake it without an IPI.
pub fn cpu_idle_mwait_cycle() {
    let ci = curcpu();

    if read_rflags() & PSL_I == 0 {
        panic(format_args!("idle with interrupts blocked!"));
    }

    // something already queued?
    if !cpu_is_idle(ci) {
        return;
    }

    // About to idle; setting the MWAIT_IN_IDLE bit tells cpu_unidle() that it can't be a
    // no-op and tells cpu_kick() that it doesn't need to use an IPI. We also set the
    // MWAIT_KEEP_IDLING bit: those routines clear it to stop the mwait. Once they're set,
    // we do a final check of the queue, in case another cpu called setrunqueue() and added
    // something to the queue and called cpu_unidle() between the check in sched_idle() and
    // here.
    ci.ci_mwait
        .fetch_or(MWAIT_IDLING | MWAIT_ONLY, Ordering::SeqCst);
    if cpu_is_idle(ci) {
        monitor(ci.ci_mwait.as_ptr(), 0, 0);
        if ci.ci_mwait.load(Ordering::SeqCst) & MWAIT_IDLING == MWAIT_IDLING {
            mwait(0, 0);
        }
    }

    // done idling; let cpu_kick() know that an IPI is required
    ci.ci_mwait.fetch_and(!MWAIT_IDLING, Ordering::SeqCst);
}

/// What `cpu_init_mwait` makes of cpuid(5) (`smallest`, `largest`, `extensions`,
/// `states`): the line to print after the device name, `cpu_mwait_states`, and
/// `cpu_mwait_size` (0 when the sizes are bogus).
pub fn cpu_mwait_info(
    smallest: u32,
    largest: u32,
    extensions: u32,
    states: u32,
) -> (alloc::string::String, u32, u32) {
    use core::fmt::Write;
    let (smallest, largest) = (smallest & 0xffff, largest & 0xffff);
    let mut states = states;
    let mut line = alloc::string::String::new();
    let _ = write!(line, ": mwait min={smallest}, max={largest}");
    if extensions & 0x1 != 0 {
        if states > 0 {
            let mut c_substates = states;
            let _ = write!(line, ", C-substates={}", 0xf & c_substates);
            loop {
                c_substates >>= 4;
                if c_substates == 0 {
                    break;
                }
                let _ = write!(line, ".{}", 0xf & c_substates);
            }
        }
        if extensions & 0x2 != 0 {
            line.push_str(", IBE");
        }
    } else {
        // substates not supported, forge the default: just C1
        states = 1 << 4;
    }

    // paranoia: check the values
    let int = size_of::<i32>() as u32;
    let size = if smallest < int || largest < smallest || largest & (int - 1) != 0 {
        line.push_str(" (bogus)");
        0
    } else {
        largest
    };
    (line, states, size)
}

/// `cpu_init_mwait(sc, ci)`: when the CPU has `monitor`/`mwait`, reads its monitor line
/// sizes and C-sub-states (cpuid 5), prints them, and makes `cpu_idle_mwait_cycle` the idle
/// loop's wait (acpicpu may install its own later).
pub fn cpu_init_mwait(sc: &Device, ci: &CpuInfo) {
    if CPU_ECXFEATURE.load(Ordering::Relaxed) & CPUIDECX_MWAIT == 0 || ci.ci_cpuid_level.get() < 0x5
    {
        return;
    }

    // get the monitor granularity
    let (smallest, largest, extensions, mut states) = cpuid(0x5);

    // mask out states C6/C7 in 31:24 for CHT45 errata
    if ci.ci_vendor.get() == CpuVendor::CPUV_INTEL
        && ci.ci_family.get() == 0x06
        && ci.ci_model.get() == 0x4c
    {
        states &= 0x00ff_ffff;
    }

    let (line, states, size) = cpu_mwait_info(smallest, largest, extensions, states);
    CPU_MWAIT_STATES.store(states, Ordering::Relaxed);
    if size != 0 {
        CPU_MWAIT_SIZE.store(size, Ordering::Relaxed);
    }
    printf(format_args!("{}{}\n", sc.xname(), line));

    // enable use of mwait; may be overridden by acpicpu later
    if CPU_MWAIT_SIZE.load(Ordering::Relaxed) > 0 {
        // SAFETY: cpu_attach runs while cold on the boot CPU, before any idle loop
        // (machdep.rs, the module's deviations).
        unsafe {
            crate::arch::amd64::amd64::machdep::CPU_IDLE_CYCLE_FCN.write(cpu_idle_mwait_cycle)
        };
    }
}

/// `cpu_attach`: fills in the boot CPU's `cpu_info` and brings it up for its role.
pub fn cpu_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `cpu_ca`, whose softc is a `CpuSoftc`.
    let sc = unsafe { self_.softc::<CpuSoftc>() };
    // SAFETY: the `cpu` driver is only attached with a `cpu_attach_args` (`cpu_match`).
    let caa = unsafe { *aux.cast::<CpuAttachArgs>() };
    let xname = self_.dv_xname.get();

    #[cfg(feature = "multiprocessor")]
    let cpunum = self_.dv_unit.get();

    // If we're an Application Processor, allocate a cpu_info structure, otherwise use the
    // primary's.
    let ci: &'static CpuInfo = if caa.cpu_role == CPU_ROLE_AP {
        #[cfg(not(feature = "multiprocessor"))]
        {
            let _ = unported!("cpu_attach: an application processor's cpu_info_full (km_alloc)");
            printf(format_args!(
                ": apid {} (application processor)\n",
                caa.cpu_apicid
            ));
            printf(format_args!("{}: not started\n", Str(&xname)));
            return;
        }
        #[cfg(feature = "multiprocessor")]
        {
            let cif = cpu_info_full_alloc();
            let ci = &cif.cif_cpu;
            ci.ci_tss.set(cif.cif_tss.get());
            ci.ci_gdt.set(cif.cif_gdt.get().cast::<u8>());
            // SAFETY: the new CPU's GDT, loaded nowhere yet, gets a copy of the boot CPU's,
            // which nothing writes any more (`gdt_init_cpu` writes each CPU's own copy).
            unsafe { *cif.cif_gdt.get() = *CPU_INFO_FULL_PRIMARY.cif_gdt.get() };
            // SAFETY: once for this CPU, on the boot CPU, before its TSS is loaded.
            unsafe { cpu_enter_pages(cif) };
            if !CPU_INFO[cpunum as usize].load(Ordering::Acquire).is_null() {
                panic(format_args!("cpu at apic id {cpunum} already attached?"));
            }
            CPU_INFO[cpunum as usize].store(ptr::from_ref(ci).cast_mut(), Ordering::Release);
            // TRAPLOG: not configured.
            ci
        }
    } else {
        #[cfg(feature = "multiprocessor")]
        if caa.cpu_apicid as u32 != lapic_cpu_number() {
            panic(format_args!(
                "{}: running cpu is at apic {} instead of at expected {}",
                Str(&xname),
                lapic_cpu_number(),
                caa.cpu_apicid
            ));
        }
        cpu_info_primary()
    };

    ci.ci_self.set(ptr::from_ref(ci));
    sc.sc_info.set(ptr::from_ref(ci));

    ci.ci_dev.set(ptr::from_ref(self_));
    ci.ci_apicid.set(caa.cpu_apicid as u32);
    ci.ci_acpi_proc_id.set(caa.cpu_acpi_proc_id as u32);
    #[cfg(feature = "multiprocessor")]
    ci.ci_cpuid.set(cpunum as u32);
    #[cfg(not(feature = "multiprocessor"))]
    ci.ci_cpuid.set(0); // False for APs, but they're not used anyway
    ci.ci_func.set(caa.cpu_func);
    ci.ci_handled_intr_level.set(IPL_NONE);

    // !SMALL_KERNEL: ci_sensordev takes the device's name; there are no sensors yet.

    #[cfg(feature = "multiprocessor")]
    {
        // NXCALL > 0: cpu_xcall_establish(ci): kern_xcall.c is not ported; reported after
        // the attach line, which a report here would cut in two.

        // Allocate UPAGES contiguous pages for the idle PCB and stack.
        let Some(kstack) = km_alloc(USPACE, &KV_ANY, &KP_DIRTY, &KD_NOWAIT) else {
            if caa.cpu_role != CPU_ROLE_AP {
                panic(format_args!(
                    "cpu_attach: unable to allocate idle stack for primary"
                ));
            }
            printf(format_args!(
                "{}: unable to allocate idle stack\n",
                Str(&xname)
            ));
            return;
        };
        let kstack = kstack.as_ptr() as usize;
        // SAFETY: USPACE fresh bytes of kernel memory, page-aligned; an all-zero pcb is
        // valid (`Pcb::new` is all zeroes), and the pcb stays at the bottom of the stack for
        // the CPU's lifetime.
        let pcb: &'static Pcb = unsafe {
            ptr::write_bytes(kstack as *mut u8, 0, USPACE);
            &*(kstack as *const Pcb)
        };
        ci.ci_idle_pcb.set(pcb);

        pcb.pcb_kstack.set((kstack + USPACE - 16) as u64);
        pcb.pcb_rbp.set((kstack + USPACE - 16) as u64);
        pcb.pcb_rsp.set((kstack + USPACE - 16) as u64);
        pcb.pcb_pmap.set(pmap_kernel());
        pcb.pcb_cr3
            .set(pmap_kernel().pm_pdirpa.get().as_usize() as u64);
    }

    // further PCB init done later.

    printf(format_args!(": "));

    match caa.cpu_role {
        CPU_ROLE_SP => {
            printf(format_args!("(uniprocessor)\n"));
            ci.ci_flags
                .fetch_or(CPUF_PRESENT | CPUF_SP | CPUF_PRIMARY, Ordering::Relaxed);
            cpu_intr_init(ci);
            identifycpu(ci);
            let _ = unported!("cpu_fix_msrs, mem_range_attach (cpu.c, mtrr.c)");
            // XXX SP fpuinit(ci) is done earlier
            cpu_init(ci);
            cpu_init_mwait(self_, ci);
        }
        CPU_ROLE_BP => {
            printf(format_args!("apid {} (boot processor)\n", caa.cpu_apicid));
            ci.ci_flags
                .fetch_or(CPUF_PRESENT | CPUF_BSP | CPUF_PRIMARY, Ordering::Relaxed);
            cpu_intr_init(ci);
            identifycpu(ci);
            let _ = unported!("cpu_fix_msrs, mem_range_attach (cpu.c, mtrr.c)");
            // NLAPIC > 0: enable local apic
            lapic_enable();
            lapic_calibrate_timer(ci);
            // XXX BP fpuinit(ci) is done earlier
            cpu_init(ci);
            // NIOAPIC > 0
            crate::arch::amd64::amd64::ioapic::IOAPIC_BSP_ID
                .store(caa.cpu_apicid, Ordering::Relaxed);
            cpu_init_mwait(self_, ci);
        }
        CPU_ROLE_AP => {
            // report on an AP
            printf(format_args!(
                "apid {} (application processor)\n",
                caa.cpu_apicid
            ));

            #[cfg(feature = "multiprocessor")]
            {
                cpu_intr_init(ci);
                cpu_start_secondary(ci);
                clockqueue_init(&ci.ci_queue);
                sched_init_cpu(ci);
                NCPUS.fetch_add(1, Ordering::Relaxed);
                if ci.ci_flags.load(Ordering::Acquire) & CPUF_PRESENT != 0 {
                    let mut ci_last = cpu_info_list();
                    // SAFETY: the list links cpu_info structures that are never freed; only
                    // the boot CPU appends, during autoconfiguration.
                    while let Some(next) = unsafe { ci_last.ci_next.get().as_ref() } {
                        ci_last = next;
                    }
                    ci_last.ci_next.set(ptr::from_ref(ci));
                }
            }
        }
        _ => panic(format_args!("unknown processor type??")),
    }

    // MULTIPROCESSOR && mp_verbose: the kstack and idle pcb lines (mp_verbose is off).
    #[cfg(feature = "multiprocessor")]
    let _ = unported!("cpu_xcall_establish (kern_xcall.c)");

    // NVMM > 0
    let _ = unported!("cpu_init_vmm (vmm)");
    // !SMALL_KERNEL: sensordev_install when the CPU has sensors; none are attached yet.
}

/// `cpu_init`: configure the CPU: `CR4` and, on the primary CPU, the clean FPU state.
pub fn cpu_init(ci: &CpuInfo) {
    // configure the CPU if needed: ci->cpu_setup comes from identifycpu (identcpu.c).

    let cr4 = rcr4() | CR4_DEFAULT;
    let _ = unported!("cpu_init: SMEP/SMAP/UMIP/OSXSAVE/PKE/PCIDE (CPUID features, identcpu.c)");
    // SAFETY: CR4_DEFAULT adds the paging bits the boot loader already set (PAE, PGE, PSE)
    // and FXSR/XMM exception support, which every amd64 CPU has.
    unsafe { lcr4(cr4) };

    // (cpu_ecxfeature & CPUIDECX_XSAVE) && ci->ci_cpuid_level >= 0xd: xsave_mask,
    // fpu_save_len and XSAVES need identifycpu and the XSAVE codepatches (amd64/fpu.rs).

    if ci.ci_flags.load(Ordering::Relaxed) & CPUF_PRIMARY != 0 {
        // Clean our FPU save area
        let sfp = fpu_cleandata();
        let mask = XSAVE_MASK.load(Ordering::Relaxed);
        // SAFETY: proc0's save area, which nothing else touches while the boot CPU
        // configures itself; zeroed and then given the initial control words.
        unsafe {
            *sfp = Savefpu::zeroed();
            (*sfp).fp_fxsave.fx_fcw = INITIAL_NPXCW;
            (*sfp).fp_fxsave.fx_mxcsr = INITIAL_MXCSR;
            let _ = xrstor_user(sfp, mask);
            // cpu_use_xsaves || !xsave_mask: always, without XSAVE
            fpusave(sfp);
        }
    } else {
        fpureset();
    }

    #[cfg(feature = "multiprocessor")]
    {
        ci.ci_flags.fetch_or(CPUF_RUNNING, Ordering::SeqCst);
        // Big hammer: flush all TLB entries, including ones from PTEs with the G bit set.
        // This should only be necessary if TLB shootdown falls far behind.
        let cr4 = rcr4();
        // SAFETY: clearing and setting CR4_PGE again only flushes the TLB (global entries
        // included); every other bit is put back as it was.
        unsafe {
            lcr4(cr4 & !CR4_PGE);
            lcr4(cr4);
        }

        // Check if TSC is synchronized.
        if COLD.load(Ordering::Relaxed) && !cpu_is_primary(ci) {
            tsc_test_sync_ap(ci);
        }
    }
}

/// `cpu_boot_secondary_processors`: lets every attached application processor run
/// (`CPUF_GO`) and waits until each reports `CPUF_RUNNING`. Without `MULTIPROCESSOR` there
/// are no application processors to start.
pub fn cpu_boot_secondary_processors() {
    #[cfg(feature = "multiprocessor")]
    {
        for slot in CPU_INFO.iter() {
            // SAFETY: `cpu_info[]` holds null or cpu_info structures that are never freed.
            let Some(ci) = (unsafe { slot.load(Ordering::Acquire).as_ref() }) else {
                continue;
            };
            if ci.ci_idle_pcb.get().is_null() {
                continue;
            }
            let flags = ci.ci_flags.load(Ordering::Acquire);
            if flags & CPUF_PRESENT == 0 {
                continue;
            }
            if flags & (CPUF_BSP | CPUF_SP | CPUF_PRIMARY) != 0 {
                continue;
            }
            ci.ci_randseed.set((arc4random() & 0x7fff_ffff) + 1);
            cpu_boot_secondary(ci);
        }

        #[cfg(feature = "qemu")]
        crate::arch::amd64::amd64::ipi::x86_ipi_selftest();
    }
}

/// `cpu_start_secondary`: starts `ci` (`CPU_STARTUP`), waits until it is present, lets it
/// identify itself and waits for that (see the module's deviations).
#[cfg(feature = "multiprocessor")]
pub fn cpu_start_secondary(ci: &'static CpuInfo) {
    ci.ci_flags.fetch_or(CPUF_AP, Ordering::SeqCst);

    // After a boot by boot(8) the processor starts in mptramp.S's pages; under Limine the
    // bootloader parked it and nothing is mapped.
    let tramp = MP_TRAMP_INSTALLED.load(Ordering::Acquire);
    if tramp {
        use crate::arch::amd64::include::mpbiosvar::{MP_TRAMP_DATA, MP_TRAMPOLINE};
        use crate::sys::mman::{PROT_EXEC, PROT_READ, PROT_WRITE};
        use crate::sys::types::{Paddr, Vaddr};
        // SAFETY: the two low pages hold the trampoline map_tramps copied; their page
        // tables exist (pmap_prealloc_lowmem_ptps).
        unsafe {
            pmap_kenter_pa(
                Vaddr::new(MP_TRAMPOLINE),
                Paddr::new(MP_TRAMPOLINE),
                PROT_READ | PROT_EXEC,
            );
            pmap_kenter_pa(
                Vaddr::new(MP_TRAMP_DATA),
                Paddr::new(MP_TRAMP_DATA),
                PROT_READ | PROT_WRITE,
            );
        }
    }

    let _ = cpu_startup_ci(ci);

    // wait for it to become ready
    let mut i = 100_000;
    while ci.ci_flags.load(Ordering::Acquire) & CPUF_PRESENT == 0 && i > 0 {
        delay(10);
        i -= 1;
    }
    // SAFETY: `cpu_attach` set `ci_dev` to the CPU's device before starting it.
    let xname = unsafe { ci.ci_dev.get().as_ref() }.map_or([0; 16], |d| d.dv_xname.get());
    if ci.ci_flags.load(Ordering::Acquire) & CPUF_PRESENT == 0 {
        printf(format_args!("{}: failed to become ready\n", Str(&xname)));
        // MPDEBUG && DDB: not configured.
    }

    if ci.ci_flags.load(Ordering::Acquire) & CPUF_IDENTIFIED == 0 {
        ci.ci_flags.fetch_or(CPUF_IDENTIFY, Ordering::SeqCst);

        // wait for it to identify
        let mut i = 2_000_000;
        while ci.ci_flags.load(Ordering::Acquire) & CPUF_IDENTIFY != 0 && i > 0 {
            delay(10);
            i -= 1;
        }

        if ci.ci_flags.load(Ordering::Acquire) & CPUF_IDENTIFY != 0 {
            printf(format_args!("{}: failed to identify\n", Str(&xname)));
        }
    }

    if ci.ci_flags.load(Ordering::Acquire) & CPUF_IDENTIFIED != 0 {
        // Test if TSCs are synchronized. Invalidate cache to minimize possible cache
        // effects. Disable interrupts to try to rule out external interference.
        let s = intr_disable();
        wbinvd();
        #[cfg(feature = "qemu")]
        let tested = tsc_sync_testable();
        tsc_test_sync_bp(curcpu());
        // SAFETY: `s` is this CPU's saved flags.
        unsafe { intr_restore(s) };
        #[cfg(feature = "qemu")]
        tsc_report_verdict(&xname, tested);
    }

    cpu_start_cleanup(ci);

    if tramp {
        use crate::arch::amd64::include::mpbiosvar::{MP_TRAMP_DATA, MP_TRAMPOLINE};
        use crate::sys::types::Vaddr;
        // SAFETY: the mappings entered above; the processor runs on its own stack and page
        // tables by now (CPUF_PRESENT came from cpu_hatch).
        unsafe {
            pmap_kremove(
                Vaddr::new(MP_TRAMPOLINE),
                crate::sys::types::Vsize::new(PAGE_SIZE),
            );
            pmap_kremove(
                Vaddr::new(MP_TRAMP_DATA),
                crate::sys::types::Vsize::new(PAGE_SIZE),
            );
        }
    }
}

/// `cpu_boot_secondary`: lets `ci` leave `cpu_hatch`'s wait (`CPUF_GO`) and waits until it
/// runs.
#[cfg(feature = "multiprocessor")]
pub fn cpu_boot_secondary(ci: &CpuInfo) {
    ci.ci_flags.fetch_or(CPUF_GO, Ordering::SeqCst);

    let mut i = 100_000;
    while ci.ci_flags.load(Ordering::Acquire) & CPUF_RUNNING == 0 && i > 0 {
        delay(10);
        i -= 1;
    }
    if ci.ci_flags.load(Ordering::Acquire) & CPUF_RUNNING == 0 {
        printf(format_args!("cpu failed to start\n"));
        // MPDEBUG && DDB: not configured.
    } else if COLD.load(Ordering::Relaxed) {
        // Test if TSCs are synchronized again.
        let s = intr_disable();
        wbinvd();
        tsc_test_sync_bp(curcpu());
        // SAFETY: `s` is this CPU's saved flags.
        unsafe { intr_restore(s) };
    }
}

/// `cpu_hatch`: the CPU ends up here when it's ready to run. This is called from
/// `cpu_hatch_entry` (`mptramp.S` in C); at this point, we are running in the idle pcb/idle
/// stack of the new cpu. When this function returns, this processor will enter the idle
/// loop and start looking for work: here it goes there itself, through `sched_toidle`.
///
/// XXX should share some of this with init386 in machdep.c
#[cfg(feature = "multiprocessor")]
pub(crate) extern "C" fn cpu_hatch(v: *const CpuInfo) -> ! {
    // SAFETY: `cpu_hatch_entry` passes the `cpu_info` `mp_cpu_start` handed the bootloader,
    // which is never freed.
    let ci: &'static CpuInfo = unsafe { &*v };

    {
        let (level, vb, vc, vd) = cpuid(0);
        let mut vendor = [0u8; 16];
        vendor[0..4].copy_from_slice(&vb.to_le_bytes());
        vendor[4..8].copy_from_slice(&vd.to_le_bytes());
        vendor[8..12].copy_from_slice(&vc.to_le_bytes());
        cpu_set_vendor(ci, level, &vendor);
    }

    // SAFETY: this CPU's own cpu_info, once, before anything reads curcpu().
    unsafe { cpu_init_msrs(ci) };

    #[cfg(feature = "debug")]
    if ci.ci_flags.load(Ordering::Acquire) & CPUF_PRESENT != 0 {
        panic(format_args!("cpu_hatch: already running!?"));
    }
    ci.ci_flags.fetch_or(CPUF_PRESENT, Ordering::SeqCst);

    lapic_enable();
    let _ = unported!("cpu_ucode_apply, cpu_tsx_disable (cpu_hatch)");

    if ci.ci_flags.load(Ordering::Acquire) & CPUF_IDENTIFIED == 0 {
        // We need to wait until we can identify, otherwise dmesg output will be messy.
        while ci.ci_flags.load(Ordering::Acquire) & CPUF_IDENTIFY == 0 {
            delay(10);
        }

        identifycpu(ci);

        // Prevent identifycpu() from running again
        ci.ci_flags.fetch_or(CPUF_IDENTIFIED, Ordering::SeqCst);

        // Signal we're done
        ci.ci_flags.fetch_and(!CPUF_IDENTIFY, Ordering::SeqCst);
    }

    // These have to run after identifycpu()
    let _ = unported!("cpu_fix_msrs (cpu_hatch)");

    // Test if our TSC is synchronized for the first time. Note that interrupts are off at
    // this point.
    wbinvd();
    tsc_test_sync_ap(ci);

    while ci.ci_flags.load(Ordering::Acquire) & CPUF_GO == 0 {
        delay(10);
    }
    // HIBERNATE (CPUF_PARK): not configured.

    #[cfg(feature = "debug")]
    if ci.ci_flags.load(Ordering::Acquire) & CPUF_RUNNING != 0 {
        panic(format_args!("cpu_hatch: already running!?"));
    }

    cpu_init_idt();
    lapic_set_lvt();
    // SAFETY: this CPU's own GDT (the copy `cpu_attach` made) and TSS, loaded nowhere else.
    unsafe { gdt_init_cpu(ci) };
    fpuinit();

    // SAFETY: selector 0: no LDT.
    unsafe { lldt(0) };

    cpu_init(ci);
    // NPVBUS > 0: pvbus_init_cpu(): not configured.

    // Re-initialise memory range handling on AP: mem_range_softc is not ported (MTRR).

    let s = splhigh();
    // SAFETY: 0 lets every interrupt through, as on the boot CPU; the IDT, the LAPIC and the
    // masks are set up, and the level is IPL_HIGH until splx.
    unsafe {
        lcr8(0);
        intr_enable();
    }
    splx(s);

    lapic_startclock();

    sched_toidle()
}

/// `mp_cpu_start` (`CPU_STARTUP`): releases the processor whose hardware ID is `ci_apicid`
/// from the bootloader's wait into `cpu_hatch_entry` (see the module's deviations); 0 when
/// it was released.
#[cfg(feature = "multiprocessor")]
pub fn mp_cpu_start(ci: &CpuInfo) -> i32 {
    // The warm reset vector (CMOS shutdown code, 40:67) and the INIT/STARTUP IPIs: the
    // bootloader parked the processor instead.
    // SAFETY: written once by `init_x86_64`, before autoconfiguration reads it.
    let Some(mp) = (unsafe { BOOT_MP.read() }) else {
        return Errno::ENXIO as i32;
    };
    let Some(index) = mp.index_of(u64::from(ci.ci_apicid.get())) else {
        return Errno::ENXIO as i32;
    };
    if ci.ci_flags.load(Ordering::Acquire) & CPUF_AP != 0
        && mp.bsp_hwid != u64::from(ci.ci_apicid.get())
    {
        // SAFETY: `index` is this application processor's, started once (`cpu_attach`
        // attaches each processor once), and the argument is its `cpu_info`, which is what
        // `cpu_hatch_entry` expects; everything it reads was written before (the boot
        // glue's release store orders it).
        unsafe { (mp.start)(index, ptr::from_ref(ci) as usize) };
    }
    0
}

/// `mp_cpu_start_cleanup` (`CPU_START_CLEANUP`): after a start through `mptramp.S`,
/// ensure the NVRAM reset byte contains something vaguely sane; Limine's start left nothing
/// to clean.
#[cfg(feature = "multiprocessor")]
pub fn mp_cpu_start_cleanup(_ci: &CpuInfo) {
    use crate::arch::amd64::include::pio::outb;
    use crate::arch::amd64::isa::nvram::{NVRAM_RESET, NVRAM_RESET_RST};
    use crate::dev::isa::isareg::IO_RTC;
    if MP_TRAMP_INSTALLED.load(Ordering::Acquire) {
        // SAFETY: the RTC's index and data ports; the boot CPU starts processors one at a
        // time, so nothing else talks to the RTC's NVRAM here.
        unsafe {
            outb(IO_RTC, NVRAM_RESET as u8);
            outb(IO_RTC + 1, NVRAM_RESET_RST as u8);
        }
    }
}

/// `mp_cpu_start` as the C does it, for a processor `mptramp.S` starts (the [`BootMp::start`]
/// `mp_madt_cpus` hands out): the warm reset code and vector point at `MP_TRAMPOLINE`, then
/// INIT and two STARTUP IPIs. `arg` is the processor's `cpu_info`.
///
/// # Safety
///
/// As [`BootMp::start`] states; `map_tramps` installed the trampoline and
/// `cpu_start_secondary` mapped its pages.
#[cfg(feature = "multiprocessor")]
unsafe fn mp_cpu_start_tramp(_i: usize, arg: usize) {
    use crate::arch::amd64::amd64::lapic::{x86_ipi, x86_ipi_init};
    use crate::arch::amd64::amd64::pmap::pmap_direct_map;
    use crate::arch::amd64::include::i82489reg::LAPIC_DLMODE_STARTUP;
    use crate::arch::amd64::include::mpbiosvar::MP_TRAMPOLINE;
    use crate::arch::amd64::include::pio::outb;
    use crate::arch::amd64::include::specialreg::CPUID_APIC;
    use crate::arch::amd64::isa::nvram::{NVRAM_RESET, NVRAM_RESET_JUMP};
    use crate::dev::isa::isareg::IO_RTC;
    use crate::sys::types::Paddr;

    // SAFETY: the caller's guarantee: `arg` is the processor's cpu_info, never freed.
    let ci: &CpuInfo = unsafe { &*(arg as *const CpuInfo) };

    // "The BSP must initialize CMOS shutdown code to 0Ah ..."
    // SAFETY: the RTC's index and data ports, used by this CPU alone here.
    unsafe {
        outb(IO_RTC, NVRAM_RESET as u8);
        outb(IO_RTC + 1, NVRAM_RESET_JUMP as u8);
    }

    // "and the warm reset vector (DWORD based at 40:67) to point to the AP startup code ..."
    let dwordptr: [u16; 2] = [0, (MP_TRAMPOLINE >> 4) as u16];
    let warm = pmap_direct_map(Paddr::new(0x467)).as_usize() as *mut [u16; 2];
    // SAFETY: the BIOS data area's warm reset vector, low memory outside uvm, reached through
    // the direct map (the C maps page 0 at VA 0 for the write).
    unsafe { ptr::write_unaligned(warm, dwordptr) };

    // "... prior to executing the following sequence:"
    if ci.ci_flags.load(Ordering::Acquire) & CPUF_AP != 0 {
        x86_ipi_init(ci.ci_apicid.get());

        delay(10000);

        if CPU_FEATURE.load(Ordering::Relaxed) & CPUID_APIC != 0 {
            x86_ipi(
                (MP_TRAMPOLINE / PAGE_SIZE) as i32,
                ci.ci_apicid.get(),
                LAPIC_DLMODE_STARTUP,
            );
            delay(200);

            x86_ipi(
                (MP_TRAMPOLINE / PAGE_SIZE) as i32,
                ci.ci_apicid.get(),
                LAPIC_DLMODE_STARTUP,
            );
            delay(200);
        }
    }
}

/// [`BootMp::cpu`] for the MADT's processors.
#[cfg(feature = "multiprocessor")]
fn madt_cpu(i: usize) -> BootCpu {
    // SAFETY: filled once by mp_madt_cpus before BOOT_MP was written; read-only since.
    let cpus = unsafe { MADT_CPUS.get() };
    cpus.get(i).copied().unwrap_or(BootCpu {
        processor_id: u32::MAX,
        hwid: u64::MAX,
    })
}

/// The processors of the firmware's MADT (the enabled local APIC and x2APIC entries, in
/// table order), for a boot by boot(8): what acpimadt0 enumerates in C, here before
/// autoconfiguration, because `mainbus_attach` attaches the CPUs from `BOOT_MP` (acpimadt.c
/// is not ported; under Limine its MP response stands for it). `rsdp` and `direct_base` are
/// virtual: the RSDP in the direct map, and the direct map's base. `None` without an MADT.
///
/// # Safety
///
/// Once, on the boot CPU, before `BOOT_MP` is read; the ACPI tables are in the direct map.
#[cfg(feature = "multiprocessor")]
pub unsafe fn mp_madt_cpus(rsdp: usize, direct_base: usize) -> Option<BootMp> {
    use crate::dev::acpi::acpireg::{
        ACPI_MADT_LAPIC, ACPI_MADT_X2APIC, ACPI_PROC_ENABLE, AcpiMadt, AcpiMadtLapic,
        AcpiMadtX2apic, AcpiRsdp, AcpiTableHeader, MADT_SIG, RSDP_SIG,
    };
    use crate::machine::bootinfo::BootMp;
    use core::mem::size_of;

    // The reads below: firmware tables in the direct map, read unaligned as the packed
    // structures they are; lengths come from the tables themselves.
    let read = |va: usize| -> AcpiTableHeader {
        // SAFETY: a table header the RSDP or the XSDT/RSDT points to, in the direct map.
        unsafe { ptr::read_unaligned(va as *const AcpiTableHeader) }
    };
    // SAFETY: the caller's guarantee: the RSDP the firmware gave, in the direct map.
    let r: AcpiRsdp = unsafe { ptr::read_unaligned(rsdp as *const AcpiRsdp) };
    if r.rsdp1.signature != *RSDP_SIG {
        return None;
    }
    // The XSDT's 64-bit entries when there is one (ACPI 2.0 and later), else the RSDT's.
    let (sdt, entsize) = if r.rsdp1.revision >= 2 && r.rsdp_xsdt != 0 {
        (direct_base + r.rsdp_xsdt as usize, 8)
    } else {
        (direct_base + r.rsdp1.rsdt as usize, 4)
    };
    let hdr = read(sdt);
    let nent = (hdr.length as usize).saturating_sub(size_of::<AcpiTableHeader>()) / entsize;
    let mut madt = None;
    for i in 0..nent {
        let ent = sdt + size_of::<AcpiTableHeader>() + i * entsize;
        // SAFETY: as above.
        let pa = unsafe {
            if entsize == 8 {
                ptr::read_unaligned(ent as *const u64) as usize
            } else {
                ptr::read_unaligned(ent as *const u32) as usize
            }
        };
        if pa != 0 && read(direct_base + pa).signature == *MADT_SIG {
            madt = Some(direct_base + pa);
            break;
        }
    }
    let madt = madt?;
    let len = read(madt).length as usize;

    // SAFETY: once, before anything reads it.
    let cpus = unsafe { MADT_CPUS.get_mut() };
    let mut n = 0;
    let mut off = size_of::<AcpiMadt>();
    while off + 2 <= len && n < cpus.len() {
        let entry = madt + off;
        // SAFETY: as above.
        let (ty, elen) = unsafe {
            (
                ptr::read(entry as *const u8),
                ptr::read((entry + 1) as *const u8),
            )
        };
        if elen < 2 {
            break;
        }
        match ty {
            ACPI_MADT_LAPIC if elen as usize >= size_of::<AcpiMadtLapic>() => {
                // SAFETY: as above.
                let e: AcpiMadtLapic =
                    unsafe { ptr::read_unaligned(entry as *const AcpiMadtLapic) };
                if e.flags & ACPI_PROC_ENABLE != 0 {
                    cpus[n] = BootCpu {
                        processor_id: u32::from(e.acpi_proc_id),
                        hwid: u64::from(e.apic_id),
                    };
                    n += 1;
                }
            }
            ACPI_MADT_X2APIC if elen as usize >= size_of::<AcpiMadtX2apic>() => {
                // SAFETY: as above.
                let e: AcpiMadtX2apic =
                    unsafe { ptr::read_unaligned(entry as *const AcpiMadtX2apic) };
                if e.flags & ACPI_PROC_ENABLE != 0 {
                    cpus[n] = BootCpu {
                        processor_id: e.acpi_proc_uid,
                        hwid: u64::from(e.apic_id),
                    };
                    n += 1;
                }
            }
            _ => {}
        }
        off += elen as usize;
    }
    if n == 0 {
        return None;
    }
    MADT_NCPUS.store(n, Ordering::Relaxed);
    // The boot processor's local APIC ID: CPUID leaf 1's initial APIC ID (xAPIC IDs; the
    // firmware runs in xAPIC mode under QEMU).
    let bsp_hwid = u64::from(crate::arch::amd64::include::specialreg::cpuid(1).1 >> 24);
    Some(BootMp {
        bsp_hwid,
        ncpus: n,
        cpu: madt_cpu,
        start: mp_cpu_start_tramp,
    })
}

/// `cpu_init_msrs`: the `syscall` MSRs and the segment bases of `ci`.
///
/// # Safety
///
/// Call on `ci`'s own CPU, once, before anything reads `curcpu()`.
pub unsafe fn cpu_init_msrs(ci: &CpuInfo) {
    // SAFETY: the caller's guarantee; these MSRs exist on every x86-64 CPU.
    unsafe {
        wrmsr(
            MSR_STAR,
            (u64::from(gsel(GCODE_SEL, SEL_KPL)) << 32)
                | (u64::from(gsel(GUDATA_SEL - 1, SEL_UPL)) << 48),
        );
        // cpu_meltdown ? Xsyscall_meltdown : Xsyscall: the U-K trampoline page is M6-b.
        wrmsr(MSR_LSTAR, Xsyscall as *const () as usize as u64);
        wrmsr(MSR_CSTAR, 0);
        wrmsr(MSR_SFMASK, PSL_NT | PSL_T | PSL_I | PSL_C | PSL_D | PSL_AC);
        // EFER.SCE enables the syscall/sysret pair: the C's locore0 sets it with LME and NXE
        // at boot, before paging; the boot protocol leaves it clear.
        wrmsr(MSR_EFER, rdmsr(MSR_EFER) | EFER_SCE);

        wrmsr(MSR_FSBASE, 0);
        wrmsr(MSR_GSBASE, ptr::from_ref(ci) as u64);
        wrmsr(MSR_KERNELGSBASE, 0);
    }
    let _ = unported!("patinit (the PAT MSR, M4-b)");
}

/// `cpu_enter_pages`: the TSS stacks of `cif` (see the module's deviations).
///
/// # Safety
///
/// Call once per CPU, on the boot CPU, before the TSS is loaded.
pub unsafe fn cpu_enter_pages(cif: &CpuInfoFull) {
    // The TSS+GDT page and the trampoline stack page in the u-k tables (pmap_enter_special):
    // M6.

    let tramp = cif.cif_tramp_stack.get();
    let tramp_end = tramp as usize + size_of::<[u64; TRAMP_STACK_WORDS]>();
    // SAFETY: the caller's guarantee; the TSS is this CPU's and not loaded yet.
    let tss = unsafe { &mut *cif.cif_tss.get() };
    tss.tss_rsp0 = (tramp_end - 16) as u64;
    cif.cif_cpu
        .ci_intr_rsp
        .set(tss.tss_rsp0 - size_of::<IretqFrame>() as u64);

    // SETUP_IST_SPECIAL_STACK(0, cif, cif_dblflt_stack); (1, cif, cif_nmi_stack): the top of
    // each stack, with the cpu_info pointer in its second-to-last word for the NMI entry.
    // (the array is copied out and back: a packed field cannot be indexed in place)
    let mut ist = tss.tss_ist;
    // SAFETY: as above; the stacks are this CPU's.
    unsafe {
        let dbl = cif.cif_dblflt_stack.get();
        ist[0] = (dbl as usize + size_of::<[u64; DBLFLT_STACK_WORDS]>() - 16) as u64;
        (*dbl)[DBLFLT_STACK_WORDS - 2] = ptr::from_ref(&cif.cif_cpu) as u64;
        let nmi = cif.cif_nmi_stack.get();
        ist[1] = (nmi as usize + size_of::<[u64; NMI_STACK_WORDS]>() - 16) as u64;
        (*nmi)[NMI_STACK_WORDS - 2] = ptr::from_ref(&cif.cif_cpu) as u64;
    }
    tss.tss_ist = ist;

    // an empty iomap, by setting its offset to the TSS limit
    tss.tss_iobase = size_of::<X86_64Tss>() as u16;
}

/// `rdrand`: feeds the time stamp counter, mixed with an `RDSEED` (or, failing that, an
/// `RDRAND`) sample where the CPU has them, into the entropy pool; again in 10 ms when `v`
/// is the timeout ([`RDRAND_TMO`]), as `cpu_configure` starts it.
pub fn rdrand(v: *mut c_void) {
    let has_rdrand = HAS_RDRAND.load(Ordering::Relaxed);
    let has_rdseed = HAS_RDSEED.load(Ordering::Relaxed);
    let mut r: u64 = 0;
    let mut valid: u8 = 0;

    let tsc = rdtsc();
    if has_rdseed {
        // SAFETY: the CPU has RDSEED (identifycpu); it writes the register and CF only.
        unsafe {
            asm!("rdseed {r}", "setc {v}", r = out(reg) r, v = out(reg_byte) valid,
                options(nomem, nostack));
        }
    }
    if has_rdrand && (!has_rdseed || valid == 0) {
        // SAFETY: the CPU has RDRAND (identifycpu); it writes the register and CF only.
        unsafe {
            asm!("rdrand {r}", "setc {v}", r = out(reg) r, v = out(reg_byte) valid,
                options(nomem, nostack));
        }
    }

    let mut t = tsc ^ r;
    t ^= u64::from(valid); // potential rdrand empty
    if has_rdrand {
        t = t.wrapping_add(rdtsc()); // potential vmexit latency
    }

    enqueue_randomness(t as u32);
    enqueue_randomness((t >> 32) as u32);

    // SAFETY: cpu_configure passes RDRAND_TMO itself (a static) or nothing.
    if let Some(tmo) = unsafe { v.cast::<Timeout>().as_ref() } {
        timeout_add_msec(tmo, 10);
    }
}

/// `wbinvd_on_all_cpus` (`MULTIPROCESSOR`): every other running CPU writes back and
/// invalidates its caches (`X86_IPI_WBINVD`), then this one.
#[cfg(feature = "multiprocessor")]
pub fn wbinvd_on_all_cpus() -> i32 {
    x86_broadcast_ipi(X86_IPI_WBINVD);
    wbinvd();
    0
}

/// `km_alloc(sizeof *cif, &kv_any, &kp_zero, &kd_waitok)` for an application processor's
/// `cpu_info_full`, with its `cpu_info` built in place: the rest of the structure (the TSS,
/// the GDT, the stacks) is plain data, valid all zero.
#[cfg(feature = "multiprocessor")]
fn cpu_info_full_alloc() -> &'static CpuInfoFull {
    let Some(va) = km_alloc(size_of::<CpuInfoFull>(), &KV_ANY, &KP_ZERO, &KD_WAITOK) else {
        panic(format_args!("cpu_attach: cannot allocate cpu_info_full"));
    };
    let cif = va.cast::<CpuInfoFull>();
    // SAFETY: fresh, zeroed, page-aligned kernel memory of the structure's size (a multiple
    // of pages, `cpu_full.rs`), never freed: the CPU lives as long as the kernel. Only the
    // `cpu_info` needs its constructor; it is written in place.
    unsafe {
        ptr::addr_of_mut!((*cif.as_ptr()).cif_cpu).write(CpuInfo::new());
        cif.as_ref()
    }
}

/// The application processor's entry from the boot glue (`Cpu::cpu_hatch`): what
/// `mptramp.S`'s `cpu_spinup_finish` does once the processor is in long mode, then
/// `cpu_hatch` on the idle pcb's stack (see the module's deviations). `arg` is the
/// processor's `struct cpu_info`, as `mp_cpu_start` passed it.
///
/// # Safety
///
/// Called once per application processor by the boot glue, with the `arg` the boot processor
/// passed to `BootMp::start`, on the bootloader's stack with interrupts masked.
#[cfg(feature = "multiprocessor")]
pub unsafe fn cpu_hatch_entry(arg: usize) -> ! {
    // SAFETY: the caller's guarantee: `arg` is the cpu_info `cpu_attach` built, never freed.
    let ci: &'static CpuInfo = unsafe { &*(arg as *const CpuInfo) };

    // Before anything can fault: the kernel's IDT (the bootloader's is not defined here).
    cpu_init_idt();

    // SAFETY: MSR_APICBASE and MSR_EFER exist on every amd64 CPU. x2APIC mode is turned on
    // only when the boot CPU runs in it (the same LAPIC accessors serve every CPU); NXE is
    // what the kernel page tables' PG_NX bits need, as `mptramp.S` sets it.
    unsafe {
        if X2APIC_ENABLED.load(Ordering::Relaxed) {
            wrmsr(MSR_APICBASE, rdmsr(MSR_APICBASE) | APICBASE_ENABLE_X2APIC);
        }
        if cpuid(0x8000_0001).3 & CPUID_NXE != 0 {
            wrmsr(MSR_EFER, rdmsr(MSR_EFER) | EFER_NXE);
        }
    }

    // SAFETY: `cpu_attach` allocated the idle pcb before starting the processor.
    let pcb: &Pcb = unsafe { &*ci.ci_idle_pcb.get() };

    let mut region = RegionDescriptor {
        rd_limit: 0,
        rd_base: 0,
    };
    setregion(&mut region, ci.ci_gdt.get() as usize, (GDT_SIZE - 1) as u16);
    // SAFETY: the CPU's GDT is the copy of the boot CPU's `cpu_attach` made, with valid
    // 64-bit kernel code and data segments; it lives as long as the CPU. CR3 is the kernel
    // pmap's PML4, which maps this code, the stacks and the cpu_info; CR0_DEFAULT keeps
    // protected mode and paging on.
    unsafe {
        lgdt(ptr::from_ref(&region));
        lcr3(pcb.pcb_cr3.get());
        lcr0(CR0_DEFAULT);
    }

    // SAFETY: the idle pcb's stack is USPACE bytes `cpu_attach` allocated for this CPU,
    // unused until now; `pcb_rsp`/`pcb_rbp` point 16 bytes below its top, 16-byte aligned as
    // the call needs. `cpu_hatch` never returns, so leaving the bootloader's stack behind is
    // fine; `ud2` traps if it ever did.
    unsafe {
        core::arch::asm!(
            "mov rsp, {sp}",
            "mov rbp, {bp}",
            "call {hatch}",
            "ud2",
            sp = in(reg) pcb.pcb_rsp.get(),
            bp = in(reg) pcb.pcb_rbp.get(),
            hatch = sym cpu_hatch,
            in("rdi") ptr::from_ref(ci),
            options(noreturn)
        )
    }
}

/// The application processor's entry from the boot glue, without `MULTIPROCESSOR`: nothing
/// starts one (the boot glue makes no MP request), so this is never reached; it parks.
///
/// # Safety
///
/// As with `MULTIPROCESSOR`: called once per started processor by the boot glue.
#[cfg(not(feature = "multiprocessor"))]
pub unsafe fn cpu_hatch_entry(_arg: usize) -> ! {
    <crate::machine::Machine as crate::machine::cpu::Cpu>::halt()
}
/* </CODE> */
