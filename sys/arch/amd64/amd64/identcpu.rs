/*	$OpenBSD: identcpu.c,v 1.158 2026/09/21 21:02:15 deraadt Exp $	*/
/*	$NetBSD: identcpu.c,v 1.1 2003/04/26 18:39:28 fvdl Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2003 Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Frank van der Linden for Wasabi Systems, Inc.
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
 *      This product includes software developed for the NetBSD Project by
 *      Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! CPU identification: `arch/amd64/amd64/identcpu.c`, the part the TSC timecounter needs.
//!
//! Upstream: sys/arch/amd64/amd64/identcpu.c @ 3ce1f3f79392
//!
//! Status: `wip`. `identifycpu` reads the extended function range, the signature and feature
//! words, the brand string, family and model, works out `CPUF_CONST_TSC`/`CPUF_INVAR_TSC`
//! and calls `tsc_identify`, measures the CPU frequency (`cpu_freq_ctr`, `cpu_freq`), prints
//! the `cpu0: <model>, <MHz>, <fam-model-step>` line, sets `cpuspeed`/`cpu_cpuspeed`
//! (`cpu_amd64speed`) and hands the frequency to `tsc_timecounter_init`. `cpu_model` and
//! `cpuspeed` are kept for `hw.model`/`hw.cpuspeed`.
//!
//! ## Deviations
//! - Under feature `qemu` only, `CPUF_CONST_TSC` and `CPUF_INVAR_TSC` are set whenever
//!   cpuid(1) reports `CPUID_TSC`. QEMU's TCG never exposes the invariant-TSC bit
//!   (cpuid 0x80000007 `%edx` bit 8): `-cpu qemu64`, `qemu64,+invtsc` and `max,+invtsc` all
//!   read 0 under QEMU 11.1.2, which warns "TCG doesn't support requested feature:
//!   CPUID\[eax=80000007h\].EDX.invtsc \[bit 8\]". Its TSC is monotonic all the same (derived
//!   from the host clock). The user's decision of 2026-10-04; `docs/ARCHITECTURE.md`.
//!   Without the feature (real hardware) the C's rules apply unchanged.
//! - With `MULTIPROCESSOR` (M11a) each application processor identifies itself from
//!   `cpu_hatch` (the AP branch re-reads cpuid(1) and intersects `cpu_feature` and
//!   `cpu_ecxfeature`); `print_perf_cpuid`'s and `pcpuid`'s `MULTIPROCESSOR` "say only what
//!   differs from the previous CPU" state goes with the feature printing, reported.
//! - Reported as unported when `identifycpu` reaches them: the feature-flag printing
//!   (`pcpuid*`, `print_perf_cpuid`, `pbitdiff`), leaf 7's `%ecx`/`%edx` and sub-leaf 2,
//!   leaf 6's `%ecx` (leaf 6's `%eax` is `ci_feature_tpmflags`, M16e), leaf 0xd sub-leaf 1, the speculation-control and SEV
//!   leaves, `replacemeltdown`, `x86_print_cacheinfo` (`cacheinfo.c`), `setperf_setup`
//!   (`k8_powernow_init`, `k1x_init`, `est_init`), `has_rdrand`/`has_rdseed`,
//!   `replacesmap`, the sensors (`intelcore_update_sensor`, `via_update_sensor`,
//!   `cpu_hz_update_sensor`), `via_nano_setup`, `cpu_topology`, `mask_width` and
//!   `cpu_check_vmm_cap` (`NVMM`).
//! - `NPVBUS` is not configured: no `pvbus_identify`.
//! - `cpu_model` and the brand are byte arrays; `hw.model` still reports itself unported in
//!   `kern_sysctl.rs`.

use core::sync::atomic::{AtomicI32, Ordering};

use libkern::StaticCell;

use crate::arch::amd64::amd64::cpu::{
    CPU_EBXFEATURE, CPU_ECXFEATURE, CPU_FEATURE, CPU_ID, ECPU_ECXFEATURE,
};
use crate::arch::amd64::amd64::machdep::delay;
use crate::arch::amd64::amd64::tsc::{tsc_identify, tsc_timecounter_init};
use crate::arch::amd64::include::cpu::{
    CPUF_CONST_TSC, CPUF_INVAR_TSC, CpuInfo, CpuVendor, cpu_is_primary,
};
use crate::arch::amd64::include::cpufunc::{rdmsr, rdtsc, wrmsr};
use crate::arch::amd64::include::specialreg::{
    CPUID_NXE, CPUID_TSC, CPUIDEAX_VERID, CPUIDECX_HV, CPUIDEDX_ITSC, MSR_BIOS_SIGN,
    MSR_PATCH_LEVEL, MSR_PERF_FIXED_CTR_CTRL, MSR_PERF_FIXED_CTR_FC_1, MSR_PERF_FIXED_CTR_FC_MASK,
    MSR_PERF_FIXED_CTR1, MSR_PERF_GLOBAL_CTR1_EN, MSR_PERF_GLOBAL_CTRL, TPM_ARAT, cpuid,
    cpuid_leaf, cpuidedx_num_fc, msr_perf_fixed_ctr_fc,
};
use crate::kern::kern_sysctl::CPU_CPUSPEED;
use crate::kern::subr_prf::{Str, printf};
use crate::sys::errno::Errno;
use crate::unported;

/// `cpu_model`: sysctl wants this. Written by `identifycpu` on the primary CPU during
/// autoconfiguration.
pub static CPU_MODEL: StaticCell<[u8; 48]> = StaticCell::new([0; 48]);
/// `amd64_has_xcrypt`: the VIA PadLock xcrypt feature bits. `via_nano_setup` (which sets them)
/// is not ported, so this stays 0.
pub static AMD64_HAS_XCRYPT: AtomicI32 = AtomicI32::new(0);
/// `cpuspeed`: the primary CPU's frequency in MHz.
pub static CPUSPEED: AtomicI32 = AtomicI32::new(0);

/// `cpu_amd64speed`: `hw.cpuspeed`'s reader.
pub fn cpu_amd64speed(freq: &mut i32) -> Result<(), Errno> {
    *freq = CPUSPEED.load(Ordering::Relaxed);
    Ok(())
}

/// `cpu_freq_ctr`: the CPU frequency from the fixed-function counter 1
/// (CPU_CLK_Unhalted.Core) over 100 ms; 0 when the TSC is not constant or the counter is
/// missing or in use.
pub fn cpu_freq_ctr(ci: &CpuInfo, cpu_perf_eax: u32, cpu_perf_edx: u32) -> u64 {
    if ci.ci_flags.load(Ordering::Relaxed) & CPUF_CONST_TSC == 0
        || (cpu_perf_eax & CPUIDEAX_VERID) <= 1
        || cpuidedx_num_fc(cpu_perf_edx) <= 1
    {
        return 0;
    }

    let fc1_mask = msr_perf_fixed_ctr_fc(1, MSR_PERF_FIXED_CTR_FC_MASK);
    // SAFETY: architectural performance monitoring version 2 and up has these MSRs (checked
    // above through cpuid(0xa)); reading them has no side effects.
    let mut msr = unsafe { rdmsr(MSR_PERF_FIXED_CTR_CTRL) };
    if msr & fc1_mask != 0 {
        // some hypervisor is dicking us around
        return 0;
    }

    msr |= msr_perf_fixed_ctr_fc(1, MSR_PERF_FIXED_CTR_FC_1);
    // SAFETY: enables fixed counter 1 for ring 1 counting, which nothing else uses (its field
    // was clear above).
    unsafe { wrmsr(MSR_PERF_FIXED_CTR_CTRL, msr) };

    // SAFETY: as above; the enable bit belongs to the counter just configured.
    unsafe {
        wrmsr(
            MSR_PERF_GLOBAL_CTRL,
            rdmsr(MSR_PERF_GLOBAL_CTRL) | MSR_PERF_GLOBAL_CTR1_EN,
        )
    };

    // SAFETY: reading a counter has no side effects.
    let last_count = unsafe { rdmsr(MSR_PERF_FIXED_CTR1) };
    delay(100_000);
    // SAFETY: as above.
    let count = unsafe { rdmsr(MSR_PERF_FIXED_CTR1) };

    // SAFETY: puts the control MSR back as the C does: it keeps only counter 1's field (the
    // C's `&=` without `~`).
    unsafe {
        wrmsr(
            MSR_PERF_FIXED_CTR_CTRL,
            rdmsr(MSR_PERF_FIXED_CTR_CTRL) & fc1_mask,
        )
    };

    // SAFETY: clears the enable bit set above.
    unsafe {
        wrmsr(
            MSR_PERF_GLOBAL_CTRL,
            rdmsr(MSR_PERF_GLOBAL_CTRL) & !MSR_PERF_GLOBAL_CTR1_EN,
        )
    };

    count.wrapping_sub(last_count) * 10
}

/// `cpu_freq`: the CPU frequency from the TSC over a 100 ms `delay`.
pub fn cpu_freq(_ci: &CpuInfo) -> u64 {
    let last_count = rdtsc();
    delay(100_000);
    let count = rdtsc();

    count.wrapping_sub(last_count) * 10
}

/// Removes leading, trailing and duplicated spaces from the NUL-terminated `model` in place.
fn compact_spaces(model: &mut [u8; 48]) {
    let mut to = 0;
    let mut skipspace = true;
    let mut from = 0;
    while from < model.len() && model[from] != 0 {
        let c = model[from];
        if !skipspace || c != b' ' {
            skipspace = false;
            model[to] = c;
            to += 1;
        }
        if c == b' ' {
            skipspace = true;
        }
        from += 1;
    }
    if skipspace && to > 0 {
        to -= 1;
    }
    model[to] = 0;
}

/// `identifycpu`: identifies `ci` (see the module's deviations for what is left out).
pub fn identifycpu(ci: &CpuInfo) {
    let (pnfeatset, _, _, _) = cpuid(0x8000_0000);
    ci.ci_pnfeatset.set(pnfeatset);
    let (efeature_eax, _, efeature_ecx, feature_eflags) = cpuid(0x8000_0001);
    ci.ci_efeature_eax.set(efeature_eax);
    ci.ci_efeature_ecx.set(efeature_ecx);
    ci.ci_feature_eflags.set(feature_eflags);

    let cflushsz;
    if cpu_is_primary(ci) {
        ci.ci_signature.set(CPU_ID.load(Ordering::Relaxed));
        ci.ci_feature_flags
            .set(CPU_FEATURE.load(Ordering::Relaxed) & !CPUID_NXE);
        cflushsz = CPU_EBXFEATURE.load(Ordering::Relaxed);
        ECPU_ECXFEATURE.store(efeature_ecx, Ordering::Relaxed);
    } else {
        let (signature, ebx, curcpu_1_ecx, feature_flags) = cpuid(1);
        ci.ci_signature.set(signature);
        ci.ci_feature_flags.set(feature_flags);
        cflushsz = ebx;
        // Let cpu_feature be the common bits
        CPU_FEATURE.fetch_and(
            feature_flags | (feature_eflags & CPUID_NXE),
            Ordering::Relaxed,
        );
        CPU_ECXFEATURE.fetch_and(curcpu_1_ecx, Ordering::Relaxed);
    }
    // cflush cacheline size is equal to bits 15-8 of ebx * 8
    ci.ci_cflushsz.set(((cflushsz >> 8) & 0xff) * 8);

    let mut brand = [0u32; 12];
    for (i, code) in (0x8000_0002u32..=0x8000_0004).enumerate() {
        let (a, b, c, d) = cpuid(code);
        brand[i * 4..i * 4 + 4].copy_from_slice(&[a, b, c, d]);
    }
    ci.ci_brand.set(brand);
    // strlcpy(mycpu_model, ci_brand, sizeof(mycpu_model)): at most 47 bytes and a NUL.
    let mut mycpu_model = [0u8; 48];
    for (i, w) in brand.iter().enumerate() {
        mycpu_model[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    mycpu_model[47] = 0;

    compact_spaces(&mut mycpu_model);

    if mycpu_model[0] == 0 {
        let s = b"Opteron or Athlon 64";
        mycpu_model[..s.len()].copy_from_slice(s);
        mycpu_model[s.len()] = 0;
    }

    // If primary cpu, fill in the global cpu_model used by sysctl
    if cpu_is_primary(ci) {
        // SAFETY: the primary CPU during autoconfiguration, before any sysctl can read it.
        unsafe { CPU_MODEL.write(mycpu_model) };
    }

    let signature = ci.ci_signature.get();
    let mut family = (signature >> 8) & 0x0f;
    let mut model = (signature >> 4) & 0x0f;
    if family == 0x6 || family == 0xf {
        family += (signature >> 20) & 0xff;
        model += ((signature >> 16) & 0x0f) << 4;
    }
    ci.ci_family.set(family);
    ci.ci_model.set(model);

    // NPVBUS > 0: hypervisors are detected early here (pvbus_identify); not configured.

    let curcpu_apmi_edx = if ci.ci_pnfeatset.get() >= 0x8000_0007 {
        cpuid(0x8000_0007).3
    } else {
        0
    };

    let feature_flags = ci.ci_feature_flags.get();
    if feature_flags != 0 && feature_flags & CPUID_TSC != 0 {
        // Has TSC, check if it's constant
        match ci.ci_vendor.get() {
            CpuVendor::CPUV_INTEL => {
                if (family == 0x0f && model >= 0x03) || (family == 0x06 && model >= 0x0e) {
                    ci.ci_flags.fetch_or(CPUF_CONST_TSC, Ordering::Relaxed);
                }
            }
            CpuVendor::CPUV_VIA => {
                if model >= 0x0f {
                    ci.ci_flags.fetch_or(CPUF_CONST_TSC, Ordering::Relaxed);
                }
            }
            CpuVendor::CPUV_AMD => {
                if curcpu_apmi_edx & CPUIDEDX_ITSC != 0 {
                    // Invariant TSC indicates constant TSC on AMD
                    ci.ci_flags.fetch_or(CPUF_CONST_TSC, Ordering::Relaxed);
                }
            }
            CpuVendor::CPUV_UNKNOWN => {}
        }

        // Check if it's an invariant TSC
        if curcpu_apmi_edx & CPUIDEDX_ITSC != 0 {
            ci.ci_flags.fetch_or(CPUF_INVAR_TSC, Ordering::Relaxed);
        }

        // QEMU's TCG hides the invariant-TSC bit; see the module's deviations.
        #[cfg(feature = "qemu")]
        ci.ci_flags
            .fetch_or(CPUF_CONST_TSC | CPUF_INVAR_TSC, Ordering::Relaxed);

        tsc_identify(ci);
    }

    let mut freq = 0;
    if ci.ci_cpuid_level.get() >= 0xa {
        let (curcpu_perf_eax, _, _, curcpu_perf_edx) = cpuid(0xa);
        freq = cpu_freq_ctr(ci, curcpu_perf_eax, curcpu_perf_edx);
    }
    if freq == 0 {
        freq = cpu_freq(ci);
    }

    if ci.ci_cpuid_level.get() >= 0x07 {
        // "Structured Extended Feature Flags"
        let (_, ebx, _, _) = cpuid_leaf(0x7, 0);
        ci.ci_feature_sefflags_ebx.set(ebx);
        let _ = unported!("identifycpu: cpuid 7 %ecx/%edx and sub-leaf 2");
    }

    // SAFETY: `cpu_attach` set `ci_dev` to the attaching device before calling us.
    let xname = unsafe { ci.ci_dev.get().as_ref() }.map_or([0; 16], |d| d.dv_xname.get());
    printf(format_args!("{}: {}", Str(&xname), Str(&mycpu_model)));

    if freq != 0 {
        printf(format_args!(
            ", {}.{:02} MHz",
            (freq + 4999) / 1_000_000,
            ((freq + 4999) / 10_000) % 100
        ));
    }

    if cpu_is_primary(ci) {
        CPUSPEED.store(((freq + 4999) / 1_000_000) as i32, Ordering::Relaxed);
        // SAFETY: the primary CPU during autoconfiguration, before any sysctl can read it.
        unsafe { CPU_CPUSPEED.write(Some(cpu_amd64speed)) };
    }

    printf(format_args!(
        ", {:02x}-{:02x}-{:02x}",
        family,
        model,
        signature & 0x0f
    ));

    if CPU_ECXFEATURE.load(Ordering::Relaxed) & CPUIDECX_HV == 0 {
        let mut level = 0;
        match ci.ci_vendor.get() {
            // SAFETY: AMD CPUs have the patch-level MSR; reading it has no side effects.
            CpuVendor::CPUV_AMD => level = unsafe { rdmsr(MSR_PATCH_LEVEL) },
            CpuVendor::CPUV_INTEL => {
                // SAFETY: Intel's documented sequence: clear IA32_BIOS_SIGN_ID, execute
                // cpuid(1), read the microcode revision from the high half.
                unsafe { wrmsr(MSR_BIOS_SIGN, 0) };
                let _ = cpuid(1);
                // SAFETY: as above.
                level = unsafe { rdmsr(MSR_BIOS_SIGN) } >> 32;
            }
            _ => {}
        }
        if level != 0 {
            printf(format_args!(", patch {level:08x}"));
        }
    }

    if ci.ci_cpuid_level.get() >= 0x06 {
        // %ecx (curcpu_tpm_ecxflags) only feeds the feature printing (pcpuid2), reported.
        let (tpmflags, _, _, _) = cpuid(0x06);
        ci.ci_feature_tpmflags.set(tpmflags);
    }
    if ci.ci_vendor.get() == CpuVendor::CPUV_AMD && ci.ci_family.get() >= 0x12 {
        ci.ci_feature_tpmflags
            .set(ci.ci_feature_tpmflags.get() | TPM_ARAT);
    }

    // cpuid 0xd, 0x80000008 and 0x8000001f and the feature flag lines (pcpuid) would print
    // here, before the newline; reported after it.
    printf(format_args!("\n"));

    let _ = unported!(
        "identifycpu: cpuid 0xd/0x80000008/0x8000001f, pcpuid, replacemeltdown, \
         x86_print_cacheinfo, setperf_setup"
    );

    tsc_timecounter_init(ci, freq);

    let _ = unported!("identifycpu: cpu_topology, cpu_check_vmm_cap, the cpu sensors");
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    fn model(s: &[u8]) -> [u8; 48] {
        let mut m = [0u8; 48];
        m[..s.len()].copy_from_slice(s);
        compact_spaces(&mut m);
        m
    }

    #[test]
    fn brand_spaces_are_compacted() {
        assert_eq!(
            &model(b"  Intel(R)  Xeon   CPU  ")[..20],
            b"Intel(R) Xeon CPU\0\0\0"
        );
        assert_eq!(
            &model(b"QEMU Virtual CPU version 2.5+")[..30],
            b"QEMU Virtual CPU version 2.5+\0"
        );
        assert_eq!(model(b"    ")[0], 0);
    }
}
/* </TESTS> */
