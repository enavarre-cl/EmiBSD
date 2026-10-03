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
//! `cpu_init_msrs` and `cpu_enter_pages`'s TSS part. CPU identification (`identifycpu`), the
//! `cpu` device (`cpu_match`/`cpu_attach`), the AP boot (`cpu_boot_secondary`), `cpu_hatch`,
//! `patinit` and the MDS/`cpu_fix_msrs` work come with M4-b to M6.
//!
//! ## Deviations
//! - `cpu_init_msrs` writes 0 to `MSR_LSTAR`: there is no `Xsyscall` until user mode (M6).
//!   `patinit` (the PAT MSR for write-combining) is reported as unported.
//! - `cpu_enter_pages` sets up the TSS stacks only: `pmap_enter_special` (the u-k mappings of
//!   the Meltdown mitigation) waits for M6.

use core::ptr;

use crate::arch::amd64::amd64::locore::Xsyscall;
use crate::arch::amd64::include::cpu::{CPUF_PRIMARY, CpuInfo};
use crate::arch::amd64::include::cpu_full::{
    CpuInfoFull, DBLFLT_STACK_WORDS, NMI_STACK_WORDS, TRAMP_STACK_WORDS,
};
use crate::arch::amd64::include::cpufunc::{rdmsr, wrmsr};
use crate::arch::amd64::include::frame::IretqFrame;
use crate::arch::amd64::include::psl::{PSL_AC, PSL_C, PSL_D, PSL_I, PSL_NT, PSL_T};
use crate::arch::amd64::include::segments::{GCODE_SEL, GUDATA_SEL, SEL_KPL, SEL_UPL, gsel};
use crate::arch::amd64::include::specialreg::{
    EFER_SCE, MSR_CSTAR, MSR_EFER, MSR_FSBASE, MSR_GSBASE, MSR_KERNELGSBASE, MSR_LSTAR, MSR_SFMASK,
    MSR_STAR,
};
use crate::arch::amd64::include::tss::X86_64Tss;
use crate::unported;

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
