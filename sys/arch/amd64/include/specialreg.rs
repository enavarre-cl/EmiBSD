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
 * Copyright (c) 1991 The Regents of the University of California.
 * All rights reserved.
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
 *	@(#)specialreg.h	7.1 (Berkeley) 5/9/91
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/specialreg.h>`: control registers, MSRs and CPUID bits.
//!
//! Upstream: sys/arch/amd64/include/specialreg.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestones M3 and M4 port the `CR0`/`CR3` bits, `MSR_EFER` and the
//! `syscall`/segment-base MSRs; the TSC timecounter (`tsc.c`, `identcpu.c`'s TSC part) adds the
//! CPUID bits and MSRs it reads and the `CPUID`/`CPUID_LEAF` macros. The other CPUID feature
//! words, MSRs and the MTRR/PAT definitions arrive with the rest of CPU identification.
//!
//! ## Deviations
//! - `CPUID(code, eax, ebx, ecx, edx)` and `CPUID_LEAF(code, leaf, ...)` are the functions
//!   [`cpuid`] and [`cpuid_leaf`] returning `(eax, ebx, ecx, edx)`: Rust has no output macro
//!   arguments, and LLVM reserves `rbx`, which `core::arch::x86_64::__cpuid_count` handles.

use core::arch::x86_64::{__cpuid_count, CpuidResult};

// Bits in 386 special registers:

/// `CR0_PE`: Protected mode Enable.
pub const CR0_PE: u64 = 0x0000_0001;
/// `CR0_MP`: "Math" Present (NPX or NPX emulator).
pub const CR0_MP: u64 = 0x0000_0002;
/// `CR0_EM`: EMulate non-NPX coproc. (trap ESC only).
pub const CR0_EM: u64 = 0x0000_0004;
/// `CR0_TS`: Task Switched (if MP, trap ESC and WAIT).
pub const CR0_TS: u64 = 0x0000_0008;
/// `CR0_ET`: Extension Type (387 (if set) vs 287).
pub const CR0_ET: u64 = 0x0000_0010;
/// `CR0_NE`: Numeric Error enable (EX16 vs IRQ13).
pub const CR0_NE: u64 = 0x0000_0020;
/// `CR0_WP`: Write Protect (honor PG_RW in all modes).
pub const CR0_WP: u64 = 0x0001_0000;
/// `CR0_PG`: PaGing enable.
pub const CR0_PG: u64 = 0x8000_0000;
/// `CR0_DEFAULT`: what `mptramp.S` loads into an application processor's `CR0`.
pub const CR0_DEFAULT: u64 = CR0_PE | CR0_PG | CR0_NE | CR0_WP;

/// `CR4_PSE`: large (4MB) page size enable.
pub const CR4_PSE: u64 = 0x0000_0010;
/// `CR4_PAE`: physical address extension enable.
pub const CR4_PAE: u64 = 0x0000_0020;
/// `CR4_PGE`: page global enable.
pub const CR4_PGE: u64 = 0x0000_0080;
/// `CR4_OSFXSR`: enable fxsave/fxrestor and SSE.
pub const CR4_OSFXSR: u64 = 0x0000_0200;
/// `CR4_OSXMMEXCPT`: enable unmasked SSE exceptions.
pub const CR4_OSXMMEXCPT: u64 = 0x0000_0400;
/// `CR4_UMIP`: user mode instruction prevention.
pub const CR4_UMIP: u64 = 0x0000_0800;
/// `CR4_PCIDE`: enable process-context IDs.
pub const CR4_PCIDE: u64 = 0x0002_0000;
/// `CR4_OSXSAVE`: enable XSAVE and extended states.
pub const CR4_OSXSAVE: u64 = 0x0004_0000;
/// `CR4_SMEP`: supervisor mode exec protection.
pub const CR4_SMEP: u64 = 0x0010_0000;
/// `CR4_SMAP`: supervisor mode access prevention.
pub const CR4_SMAP: u64 = 0x0020_0000;
/// `CR4_PKE`: user-mode protection keys.
pub const CR4_PKE: u64 = 0x0040_0000;

/// `CR3_REUSE_PCID`: do not flush the PCID's TLB entries on load.
pub const CR3_REUSE_PCID: u64 = 1 << 63;
/// `CR3_PADDR`: the page-table address bits of `CR3`.
pub const CR3_PADDR: u64 = 0x7fff_ffff_ffff_f000;

// CPUID "features" bits (CPUID function 0x1):

/// `CPUID_TSC`: has time stamp counter.
pub const CPUID_TSC: u32 = 0x0000_0010;
/// `CPUID_APIC`: has enabled APIC.
pub const CPUID_APIC: u32 = 0x0000_0200;

/// `CPUIDECX_MWAIT`: Monitor/Mwait.
pub const CPUIDECX_MWAIT: u32 = 0x0000_0008;
/// `CPUIDECX_RDRAND`: RDRAND instruction.
pub const CPUIDECX_RDRAND: u32 = 0x4000_0000;
/// `CPUIDECX_HV`: running on hypervisor.
pub const CPUIDECX_HV: u32 = 0x8000_0000;

// `MSR_SEV_STATUS` bits (`cpu_sev_guestmode`):

/// `SEV_STAT_ENABLED`: SEV is active.
pub const SEV_STAT_ENABLED: i32 = 0x0000_0001;
/// `SEV_STAT_ES_ENABLED`: SEV-ES is active.
pub const SEV_STAT_ES_ENABLED: i32 = 0x0000_0002;
/// `SEV_STAT_SNP_ACTIVE`: SEV-SNP is active.
pub const SEV_STAT_SNP_ACTIVE: i32 = 0x0000_0004;

// "Structured Extended Feature Flags Parameters" (CPUID function 0x7, leaf 0)

/// `SEFF0EBX_TSC_ADJUST`: has IA32_TSC_ADJUST MSR.
pub const SEFF0EBX_TSC_ADJUST: u32 = 0x0000_0002;
/// `SEFF0EBX_RDSEED`: RDSEED instruction.
pub const SEFF0EBX_RDSEED: u32 = 0x0004_0000;

// Thermal and Power Management (CPUID function 0x6) EAX bits:

/// `TPM_SENSOR`: digital temp sensor.
pub const TPM_SENSOR: u32 = 0x0000_0001;
/// `TPM_ARAT`: APIC Timer Always Running.
pub const TPM_ARAT: u32 = 0x0000_0004;
/// `TPM_PTS`: Intel Package Thermal Status.
pub const TPM_PTS: u32 = 0x0000_0040;

// "Architectural Performance Monitoring" bits (CPUID function 0x0a):

/// `CPUIDEAX_VERID`: version ID.
pub const CPUIDEAX_VERID: u32 = 0x0000_00ff;
/// `CPUIDEDX_NUM_FC(cpuid)`: the number of fixed-function counters.
pub const fn cpuidedx_num_fc(cpuid: u32) -> u32 {
    cpuid & 0x0000_001f
}

// CPUID "extended features" bits (CPUID function 0x80000001):

/// `CPUID_NXE`: No-Execute Extension.
pub const CPUID_NXE: u32 = 0x0010_0000;
/// `CPUID_RDTSCP`: RDTSCP / IA32_TSC_AUX available.
pub const CPUID_RDTSCP: u32 = 0x0800_0000;

// "Advanced Power Management Information" bits (CPUID function 0x80000007):

/// `CPUIDEDX_ITSC`: Invariant TSC.
pub const CPUIDEDX_ITSC: u32 = 1 << 8;

/// `CPUID(code, eax, ebx, ecx, edx)`: executes `cpuid` for function `code`.
#[inline]
pub fn cpuid(code: u32) -> (u32, u32, u32, u32) {
    cpuid_leaf(code, 0)
}

/// `CPUID_LEAF(code, leaf, eax, ebx, ecx, edx)`: executes `cpuid` for function `code`, sub-leaf
/// `leaf`.
#[inline]
pub fn cpuid_leaf(code: u32, leaf: u32) -> (u32, u32, u32, u32) {
    // `__cpuid_count` is safe to call on every x86-64 CPU (the instruction always exists in
    // long mode); it saves and restores `rbx` around `cpuid` for LLVM.
    #[allow(unused_unsafe)]
    // SAFETY: `cpuid` only reads identification registers; it has no side effects.
    let CpuidResult { eax, ebx, ecx, edx } = unsafe { __cpuid_count(code, leaf) };
    (eax, ebx, ecx, edx)
}

/// `MSR_TSC_ADJUST`.
pub const MSR_TSC_ADJUST: u32 = 0x03b;
/// `MSR_BIOS_SIGN`.
pub const MSR_BIOS_SIGN: u32 = 0x08b;
/// `MSR_PERF_FIXED_CTR1`: CPU_CLK_Unhalted.Core.
pub const MSR_PERF_FIXED_CTR1: u32 = 0x30a;
/// `MSR_PERF_FIXED_CTR_CTRL`.
pub const MSR_PERF_FIXED_CTR_CTRL: u32 = 0x38d;
/// `MSR_PERF_FIXED_CTR_FC_1`: count ring 1.
pub const MSR_PERF_FIXED_CTR_FC_1: u64 = 0x1;
/// `MSR_PERF_FIXED_CTR_FC_MASK`.
pub const MSR_PERF_FIXED_CTR_FC_MASK: u64 = 0x3;
/// `MSR_PERF_FIXED_CTR_FC(_i, _v)`.
pub const fn msr_perf_fixed_ctr_fc(i: u64, v: u64) -> u64 {
    v << (4 * i)
}
/// `MSR_PERF_GLOBAL_CTRL`.
pub const MSR_PERF_GLOBAL_CTRL: u32 = 0x38f;
/// `MSR_PERF_GLOBAL_CTR1_EN`.
pub const MSR_PERF_GLOBAL_CTR1_EN: u64 = 1 << 33;
/// `MSR_PATCH_LEVEL`.
pub const MSR_PATCH_LEVEL: u32 = 0x0000_008b;
/// `MSR_APICBASE`: the local APIC's base address and mode.
pub const MSR_APICBASE: u32 = 0x01b;
/// `APICBASE_BSP`.
pub const APICBASE_BSP: u64 = 0x100;
/// `APICBASE_ENABLE_X2APIC`.
pub const APICBASE_ENABLE_X2APIC: u64 = 0x400;
/// `APICBASE_GLOBAL_ENABLE`.
pub const APICBASE_GLOBAL_ENABLE: u64 = 0x800;
/// `APICBASE_ADDRESS_MASK`.
pub const APICBASE_ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;
/// `MSR_EFER`: Extended feature enable.
pub const MSR_EFER: u32 = 0xc000_0080;
/// `MSR_STAR`: the `syscall`/`sysret` segment selectors.
pub const MSR_STAR: u32 = 0xc000_0081;
/// `MSR_LSTAR`: the 64-bit `syscall` entry point.
pub const MSR_LSTAR: u32 = 0xc000_0082;
/// `MSR_CSTAR`: the compatibility-mode `syscall` entry point.
pub const MSR_CSTAR: u32 = 0xc000_0083;
/// `MSR_SFMASK`: the `RFLAGS` bits `syscall` clears.
pub const MSR_SFMASK: u32 = 0xc000_0084;
/// `MSR_FSBASE`: the `FS` segment base.
pub const MSR_FSBASE: u32 = 0xc000_0100;
/// `MSR_GSBASE`: the `GS` segment base.
pub const MSR_GSBASE: u32 = 0xc000_0101;
/// `MSR_KERNELGSBASE`: the `GS` base `swapgs` swaps in.
pub const MSR_KERNELGSBASE: u32 = 0xc000_0102;
/// `MSR_HWCR`.
pub const MSR_HWCR: u32 = 0xc001_0015;
/// `HWCR_TSCFREQSEL`.
pub const HWCR_TSCFREQSEL: u64 = 0x0100_0000;
/// `MSR_SEV_GHCB`: the guest-hypervisor communication block of an SEV-ES guest
/// (`locore0.S`'s `#VC` termination request).
pub const MSR_SEV_GHCB: u32 = 0xc001_0130;
/// `MSR_PSTATEDEF(_n)`.
pub const fn msr_pstatedef(n: u32) -> u32 {
    0xc001_0064 + n
}
/// `PSTATEDEF_EN`.
pub const PSTATEDEF_EN: u64 = 0x8000_0000_0000_0000;
/// `EFER_SCE`: SYSCALL extension.
pub const EFER_SCE: u64 = 0x0000_0001;
/// `EFER_LME`: Long Mode Enabled.
pub const EFER_LME: u64 = 0x0000_0100;
/// `EFER_LMA`: Long Mode Active.
pub const EFER_LMA: u64 = 0x0000_0400;
/// `EFER_NXE`: No-Execute Enabled.
pub const EFER_NXE: u64 = 0x0000_0800;

/// `CR4_DEFAULT`: the `CR4` bits every CPU runs with.
pub const CR4_DEFAULT: u64 = CR4_PAE | CR4_PGE | CR4_PSE | CR4_OSFXSR | CR4_OSXMMEXCPT;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/specialreg.h");
        let ours: &[(&str, i64)] = &[
            ("CR0_PE", CR0_PE as i64),
            ("CR0_PG", CR0_PG as i64),
            ("CR0_NE", CR0_NE as i64),
            ("CR0_WP", CR0_WP as i64),
            ("CR4_PSE", CR4_PSE as i64),
            ("CR4_PAE", CR4_PAE as i64),
            ("CR4_PGE", CR4_PGE as i64),
            ("CR4_OSFXSR", CR4_OSFXSR as i64),
            ("CR4_OSXMMEXCPT", CR4_OSXMMEXCPT as i64),
            ("CR4_UMIP", CR4_UMIP as i64),
            ("CR4_PCIDE", CR4_PCIDE as i64),
            ("CR4_OSXSAVE", CR4_OSXSAVE as i64),
            ("CR4_SMEP", CR4_SMEP as i64),
            ("CR4_SMAP", CR4_SMAP as i64),
            ("CR4_PKE", CR4_PKE as i64),
            ("CR3_PADDR", CR3_PADDR as i64),
            ("MSR_EFER", i64::from(MSR_EFER)),
            ("EFER_SCE", EFER_SCE as i64),
            ("EFER_LME", EFER_LME as i64),
            ("EFER_LMA", EFER_LMA as i64),
            ("EFER_NXE", EFER_NXE as i64),
            ("MSR_APICBASE", i64::from(MSR_APICBASE)),
            ("APICBASE_ENABLE_X2APIC", APICBASE_ENABLE_X2APIC as i64),
            ("MSR_STAR", i64::from(MSR_STAR)),
            ("MSR_LSTAR", i64::from(MSR_LSTAR)),
            ("MSR_FSBASE", i64::from(MSR_FSBASE)),
            ("MSR_GSBASE", i64::from(MSR_GSBASE)),
            ("MSR_KERNELGSBASE", i64::from(MSR_KERNELGSBASE)),
            ("CPUID_TSC", i64::from(CPUID_TSC)),
            ("CPUIDECX_HV", i64::from(CPUIDECX_HV)),
            ("CPUIDECX_RDRAND", i64::from(CPUIDECX_RDRAND)),
            ("CPUIDECX_MWAIT", i64::from(CPUIDECX_MWAIT)),
            ("TPM_SENSOR", i64::from(TPM_SENSOR)),
            ("TPM_ARAT", i64::from(TPM_ARAT)),
            ("TPM_PTS", i64::from(TPM_PTS)),
            ("SEFF0EBX_TSC_ADJUST", i64::from(SEFF0EBX_TSC_ADJUST)),
            ("SEFF0EBX_RDSEED", i64::from(SEFF0EBX_RDSEED)),
            ("CPUID_NXE", i64::from(CPUID_NXE)),
            ("CPUID_RDTSCP", i64::from(CPUID_RDTSCP)),
            ("MSR_TSC_ADJUST", i64::from(MSR_TSC_ADJUST)),
            ("MSR_HWCR", i64::from(MSR_HWCR)),
            ("CPUIDEAX_VERID", i64::from(CPUIDEAX_VERID)),
            ("MSR_BIOS_SIGN", i64::from(MSR_BIOS_SIGN)),
            ("MSR_PERF_FIXED_CTR1", i64::from(MSR_PERF_FIXED_CTR1)),
            (
                "MSR_PERF_FIXED_CTR_CTRL",
                i64::from(MSR_PERF_FIXED_CTR_CTRL),
            ),
            ("MSR_PERF_GLOBAL_CTRL", i64::from(MSR_PERF_GLOBAL_CTRL)),
            ("MSR_PATCH_LEVEL", i64::from(MSR_PATCH_LEVEL)),
            ("HWCR_TSCFREQSEL", HWCR_TSCFREQSEL as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
