/*	$OpenBSD: cpu.c,v 1.208 2026/10/01 23:51:29 jsg Exp $	*/
/* $NetBSD: cpu.c,v 1.1 2003/04/26 18:39:26 fvdl Exp $ */
/* <LICENSES> */
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

//! amd64 CPU attachment and per-CPU setup: `arch/amd64/amd64/cpu.c`.
//!
//! Upstream: sys/arch/amd64/amd64/cpu.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `cpu_info_full_primary`,
//! `cpu_init_msrs` and `cpu_enter_pages`'s TSS part; M7b the `cpu` device (`struct
//! cpu_softc`, `cpu_ca`, `cpu_cd`, `cpu_match`, `cpu_attach`) for autoconfiguration. CPU
//! identification (`identifycpu`), `cpu_init`, the AP boot (`cpu_boot_secondary`),
//! `cpu_hatch`, `patinit` and the MDS/`cpu_fix_msrs` work come later.
//!
//! ## Deviations
//! - `cpu_attach` reports what it cannot do yet: `cpu_fix_msrs`,
//!   `mem_range_attach` (`MTRR`), `cpu_init_mwait` and `cpu_init_vmm`; an
//!   application processor (never attached without `MULTIPROCESSOR` tables) is reported
//!   instead of getting a `km_alloc`ed `cpu_info_full`. `cpu_ca` has no `cpu_activate`
//!   (suspend/resume): `config_suspend` walks the CPU's children instead.
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

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

use libkern::StaticCell;

use crate::arch::amd64::amd64::fpu::XSAVE_MASK;
use crate::arch::amd64::amd64::identcpu::identifycpu;
use crate::arch::amd64::amd64::intr::cpu_intr_init;
use crate::arch::amd64::amd64::lapic::{lapic_calibrate_timer, lapic_enable};
use crate::arch::amd64::amd64::locore::Xsyscall;
use crate::arch::amd64::include::cpu::{
    CPUF_BSP, CPUF_PRESENT, CPUF_PRIMARY, CPUF_SP, CpuInfo, MAXCPUS, cpu_info_primary,
};
use crate::arch::amd64::include::cpu_full::{
    CpuInfoFull, DBLFLT_STACK_WORDS, NMI_STACK_WORDS, TRAMP_STACK_WORDS,
};
use crate::arch::amd64::include::cpufunc::{lcr4, rcr4, rdmsr, wrmsr};
use crate::arch::amd64::include::cpuvar::{CPU_ROLE_AP, CPU_ROLE_BP, CPU_ROLE_SP, CpuAttachArgs};
use crate::arch::amd64::include::fpu::{
    INITIAL_MXCSR, INITIAL_NPXCW, Savefpu, fpu_cleandata, fpureset, fpusave, xrstor_user,
};
use crate::arch::amd64::include::frame::IretqFrame;
use crate::arch::amd64::include::intrdefs::IPL_NONE;
use crate::arch::amd64::include::psl::{PSL_AC, PSL_C, PSL_D, PSL_I, PSL_NT, PSL_T};
use crate::arch::amd64::include::segments::{GCODE_SEL, GUDATA_SEL, SEL_KPL, SEL_UPL, gsel};
use crate::arch::amd64::include::specialreg::{
    CR4_DEFAULT, EFER_SCE, MSR_CSTAR, MSR_EFER, MSR_FSBASE, MSR_GSBASE, MSR_KERNELGSBASE,
    MSR_LSTAR, MSR_SFMASK, MSR_STAR,
};
use crate::arch::amd64::include::tss::X86_64Tss;
use crate::kern::subr_prf::{Str, panic, printf};
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::unported;

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

/// `cpu_attach`: fills in the boot CPU's `cpu_info` and brings it up for its role.
pub fn cpu_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `cpu_ca`, whose softc is a `CpuSoftc`.
    let sc = unsafe { self_.softc::<CpuSoftc>() };
    // SAFETY: the `cpu` driver is only attached with a `cpu_attach_args` (`cpu_match`).
    let caa = unsafe { *aux.cast::<CpuAttachArgs>() };
    let xname = self_.dv_xname.get();

    // If we're an Application Processor, allocate a cpu_info structure, otherwise use the
    // primary's.
    if caa.cpu_role == CPU_ROLE_AP {
        let _ = unported!("cpu_attach: an application processor's cpu_info_full (km_alloc)");
        printf(format_args!(
            ": apid {} (application processor)\n",
            caa.cpu_apicid
        ));
        printf(format_args!("{}: not started\n", Str(&xname)));
        return;
    }
    let ci = cpu_info_primary();
    // MULTIPROCESSOR: the running CPU's apic id is checked against caa.cpu_apicid.

    ci.ci_self.set(ptr::from_ref(ci));
    sc.sc_info.set(ptr::from_ref(ci));

    ci.ci_dev.set(ptr::from_ref(self_));
    ci.ci_apicid.set(caa.cpu_apicid as u32);
    ci.ci_acpi_proc_id.set(caa.cpu_acpi_proc_id as u32);
    ci.ci_cpuid.set(0); // False for APs, but they're not used anyway
    // ci_func = caa->cpu_func: the start/stop functions are MULTIPROCESSOR's.
    ci.ci_handled_intr_level.set(IPL_NONE);

    // !SMALL_KERNEL: ci_sensordev takes the device's name; there are no sensors yet.

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
            let _ = unported!("cpu_init_mwait (mwait)");
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
            let _ = unported!("cpu_init_mwait (mwait)");
            // NIOAPIC > 0: ioapic_bsp_id = caa->cpu_apicid (ioapic.c is not ported).
        }
        _ => panic(format_args!("unknown processor type??")),
    }

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

    // MULTIPROCESSOR: CPUF_RUNNING and the CR4_PGE TLB flush.
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
