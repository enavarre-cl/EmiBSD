/*	$OpenBSD: tsc.c,v 1.33 2026/09/18 19:24:50 jan Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2008 The NetBSD Foundation, Inc.
 * Copyright (c) 2016,2017 Reyk Floeter <reyk@openbsd.org>
 * Copyright (c) 2017 Adam Steen <adam@adamsteen.com.au>
 * Copyright (c) 2017 Mike Belopuhov <mike@openbsd.org>
 * Copyright (c) 2019 Paul Irofti <paul@irofti.net>
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

//! The TSC timecounter: `arch/amd64/amd64/tsc.c`. Finds the TSC's frequency (cpuid leaf 0x15
//! on Intel, the P0 state MSR on AMD family 17h/19h, or a measurement handed in by
//! `identifycpu` and recalibrated against a better timecounter), registers the `tsc`
//! timecounter and offers `tsc_delay` as `delay(9)`. The `MULTIPROCESSOR` part tests that
//! the CPUs' counters are synchronised.
//!
//! Upstream: sys/arch/amd64/amd64/tsc.c @ 3ce1f3f79392
//!
//! The timecounter reads the low 32 bits of the TSC (`tc_counter_mask` `~0u`), as in C: at
//! a few GHz that is a wrap every second or two, against the i8254's 27 ms.
//!
//! ## Deviations
//! - `NPVBUS` is not configured, so `tsc_freq_kvm` (the KVM/VMware timing leaf) is not
//!   compiled, as in a C kernel without `pvbus`.
//! - Under feature `qemu` only, `tsc_timecounter_init` gives the TSC quality 2000 when no
//!   reference timecounter has recalibrated it (`tc_priv` still null): the frequency is
//!   `identifycpu`'s `cpu_freq`, the TSC counted over a 100 ms `i8254_delay`. In C the TSC
//!   stays at -1000 until `acpitimer` or `acpihpet` call `cpu_recalibrate_tsc`, and neither is
//!   ported; QEMU's TCG also never reports an invariant TSC (`identcpu.rs`'s deviations). The
//!   i8254 itself cannot be the reference: `measure_tsc_freq` delays 100 ms with interrupts
//!   off, and its 15-bit counter wraps every 27 ms. `docs/ARCHITECTURE.md`.
//! - `tsc_delay` treats a negative `usecs` as 0 (the C converts it to a huge `uint64_t`).
//! - `tsc_rdtsc` is a `StaticCell<fn() -> u64>` written by `tsc_identify` on the boot CPU.
//! - The `MULTIPROCESSOR` synchronisation test is behind feature `multiprocessor`, unused
//!   until the APs start (M11b): `struct tsc_test_status` keeps its cache-line layout with
//!   atomics for the `volatile` and barrier-protected members. `TSC_DEBUG` is not configured.

use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use libkern::StaticCell;

use crate::arch::amd64::amd64::lapic::LAPIC_PER_SECOND;
use crate::arch::amd64::amd64::machdep::{delay, delay_init};
use crate::arch::amd64::include::cpu::{
    CPUF_CONST_TSC, CPUF_INVAR_TSC, CPUF_PRIMARY, CpuInfo, CpuVendor,
};
use crate::arch::amd64::include::cpufunc::{
    intr_disable, intr_restore, rdmsr, rdtsc, rdtsc_lfence, rdtscp,
};
use crate::arch::amd64::include::specialreg::{
    CPUID_RDTSCP, HWCR_TSCFREQSEL, MSR_HWCR, PSTATEDEF_EN, cpuid, msr_pstatedef,
};
use crate::arch::amd64::include::timetc::{TC_TSC_LFENCE, TC_TSC_RDTSCP};
use crate::kern::kern_tc::tc_init;
use crate::sys::timetc::Timecounter;

/// `RECALIBRATE_MAX_RETRIES`.
const RECALIBRATE_MAX_RETRIES: i32 = 5;
/// `RECALIBRATE_SMI_THRESHOLD`.
const RECALIBRATE_SMI_THRESHOLD: u64 = 50000;
/// `RECALIBRATE_DELAY_THRESHOLD`.
const RECALIBRATE_DELAY_THRESHOLD: i64 = 50;

/// `tsc_recalibrate`: the frequency was measured, so a better timecounter may refine it.
static TSC_RECALIBRATE: AtomicI32 = AtomicI32::new(0);

/// `tsc_frequency`: the TSC's frequency in Hz, 0 while unknown.
pub static TSC_FREQUENCY: AtomicU64 = AtomicU64::new(0);
/// `tsc_is_invariant`.
pub static TSC_IS_INVARIANT: AtomicI32 = AtomicI32::new(0);

/// `tsc_rdtsc`: how the TSC is read, `rdtsc_lfence` unless the CPU has `rdtscp`.
static TSC_RDTSC: StaticCell<fn() -> u64> = StaticCell::new(rdtsc_lfence);

/// `tsc_timecounter`.
pub static TSC_TIMECOUNTER: Timecounter = Timecounter::new(
    tsc_get_timecount_lfence,
    !0u32,
    0,
    "tsc",
    -1000,
    TC_TSC_LFENCE,
);

/// `tsc_rdtsc()`.
#[inline]
fn tsc_rdtsc() -> u64 {
    // SAFETY: written only by `tsc_identify` on the boot CPU during autoconfiguration, before
    // the timecounter is registered or `tsc_delay` installed.
    (unsafe { TSC_RDTSC.read() })()
}

/// `tsc_freq_cpuid`: the TSC frequency from Intel's crystal clock leaf 0x15, or 0.
pub fn tsc_freq_cpuid(ci: &CpuInfo) -> u64 {
    if ci.ci_vendor.get() == CpuVendor::CPUV_INTEL && ci.ci_cpuid_level.get() >= 0x15 {
        let (eax, ebx, khz, _) = cpuid(0x15);
        let mut khz = khz / 1000;
        if khz == 0 {
            khz = match ci.ci_model.get() {
                0x4e | // Skylake mobile
                0x5e | // Skylake desktop
                0x8e | // Kabylake mobile
                0x9e | // Kabylake desktop
                0xa5 | // CML-H CML-S62 CML-S102
                0xa6 => 24000, // CML-U62: 24.0 MHz
                0x5f => 25000, // Atom Denverton: 25.0 MHz
                0x5c => 19200, // Atom Goldmont: 19.2 MHz
                _ => 0,
            };
        }
        if ebx != 0 && eax != 0 {
            let count = u64::from(khz) * u64::from(ebx) / u64::from(eax);
            if count != 0 {
                // NLAPIC > 0
                LAPIC_PER_SECOND.store(khz.wrapping_mul(1000), Ordering::Relaxed);
                return count * 1000;
            }
        }
    }

    0
}

/// `tsc_freq_msr`: the TSC frequency from AMD's P0 state definition, or 0.
pub fn tsc_freq_msr(ci: &CpuInfo) -> u64 {
    if ci.ci_vendor.get() != CpuVendor::CPUV_AMD {
        return 0;
    }

    // All 10h+ CPUs have Core::X86::Msr:HWCR and the TscFreqSel bit. If TscFreqSel hasn't
    // been set, the TSC isn't advancing at the core P0 frequency and we need to calibrate by
    // hand.
    if ci.ci_family.get() < 0x10 {
        return 0;
    }
    // SAFETY: every AMD family 10h and later CPU has HWCR; reading it has no side effects.
    if unsafe { rdmsr(MSR_HWCR) } & HWCR_TSCFREQSEL == 0 {
        return 0;
    }

    // In 10h+ CPUs, Core::X86::Msr::PStateDef defines the voltage and frequency for each core
    // P-state. We want the P0 frequency. If the En bit isn't set, the register doesn't define
    // a valid P-state.
    // SAFETY: as for HWCR, the P-state definitions exist from family 10h on.
    let def = unsafe { rdmsr(msr_pstatedef(0)) };
    if def & PSTATEDEF_EN == 0 {
        return 0;
    }

    let (base, multiplier, divisor);
    match ci.ci_family.get() {
        0x17 | 0x19 => {
            // PPR for AMD Family 17h [...]:
            // Models 01h,08h B2, Rev 3.03, pp. 33, 139-140
            // Model 18h B1, Rev 3.16, pp. 36, 143-144
            // Model 60h A1, Rev 3.06, pp. 33, 155-157
            // Model 71h B0, Rev 3.06, pp. 28, 150-151
            //
            // PPR for AMD Family 19h [...]:
            // Model 21h B0, Rev 3.05, pp. 33, 166-167
            //
            // OSRR for AMD Family 17h processors,
            // Models 00h-2Fh, Rev 3.03, pp. 130-131
            base = 200_000_000u64; // 200.0 MHz
            divisor = (def >> 8) & 0x3f;
            if divisor <= 0x07 || divisor >= 0x2d {
                return 0; // reserved
            }
            if divisor >= 0x1b && divisor % 2 == 1 {
                return 0; // reserved
            }
            multiplier = def & 0xff;
            if multiplier <= 0x0f {
                return 0; // reserved
            }
        }
        _ => return 0,
    }

    base * multiplier / divisor
}

// NPVBUS > 0: tsc_freq_kvm, the KVM (or VMware) timing leaf at hv_base + 0x10 (eax = TSC kHz,
// ebx = LAPIC bus kHz, which also seeds lapic_per_second). pvbus is not configured.

/// `tsc_identify`: on the primary CPU with a constant, invariant TSC, picks the read
/// instruction, marks the TSC invariant, looks up its frequency and, if known, installs
/// `tsc_delay`.
pub fn tsc_identify(ci: &CpuInfo) {
    let flags = ci.ci_flags.load(Ordering::Relaxed);
    if flags & CPUF_PRIMARY == 0 || flags & CPUF_CONST_TSC == 0 || flags & CPUF_INVAR_TSC == 0 {
        return;
    }

    // Prefer RDTSCP where supported.
    if ci.ci_feature_eflags.get() & CPUID_RDTSCP != 0 {
        // SAFETY: the boot CPU during autoconfiguration, before anything reads it.
        unsafe { TSC_RDTSC.write(rdtscp) };
        TSC_TIMECOUNTER
            .tc_get_timecount
            .set(tsc_get_timecount_rdtscp);
        TSC_TIMECOUNTER.tc_user.set(TC_TSC_RDTSCP);
    }

    TSC_IS_INVARIANT.store(1, Ordering::Relaxed);

    let mut freq = tsc_freq_cpuid(ci);
    if freq == 0 {
        freq = tsc_freq_msr(ci);
    }
    // NPVBUS > 0: tsc_freq_kvm.
    TSC_FREQUENCY.store(freq, Ordering::Relaxed);
    if freq > 0 {
        delay_init(tsc_delay, 5000);
    }
}

/// `get_tsc_and_timecount`: reads `tc` between two TSC reads less than
/// `RECALIBRATE_SMI_THRESHOLD` cycles apart, retrying a few times. Returns `true` (the C's
/// nonzero) when every try was too slow, i.e. disturbed by an SMI.
#[inline]
fn get_tsc_and_timecount(tc: &Timecounter, tsc: &mut u64, count: &mut u64) -> bool {
    for _ in 0..RECALIBRATE_MAX_RETRIES {
        let tsc1 = tsc_rdtsc();
        let n = u64::from(tc.get_timecount() & tc.tc_counter_mask.get());
        let tsc2 = tsc_rdtsc();

        if tsc2.wrapping_sub(tsc1) < RECALIBRATE_SMI_THRESHOLD {
            *count = n;
            *tsc = tsc2;
            return false;
        }
    }
    true
}

/// `calculate_tsc_freq`: TSC cycles over `usec` microseconds, in Hz.
#[inline]
fn calculate_tsc_freq(tsc1: u64, tsc2: u64, usec: u64) -> u64 {
    let delta = tsc2.wrapping_sub(tsc1);
    delta * 1_000_000 / usec
}

/// `calculate_tc_delay`: microseconds between two readings of `tc`, allowing one wrap.
#[inline]
fn calculate_tc_delay(tc: &Timecounter, count1: u64, mut count2: u64) -> u64 {
    if count2 < count1 {
        count2 += u64::from(tc.tc_counter_mask.get());
    }

    let delta = count2 - count1;
    delta * 1_000_000 / tc.tc_frequency.get()
}

/// `measure_tsc_freq`: the TSC frequency over three 100 ms delays timed by `tc`, the
/// lowest of at least two good rounds, or 0.
pub fn measure_tsc_freq(tc: &Timecounter) -> u64 {
    // warmup the timers
    for _ in 0..3 {
        let _ = tc.get_timecount();
        let _ = rdtsc();
    }

    let mut min_freq = u64::MAX;
    let mut success = 0;

    let delay_usec: i64 = 100_000;
    for _ in 0..3 {
        let s = intr_disable();

        let (mut tsc1, mut count1, mut tsc2, mut count2) = (0, 0, 0, 0);
        let err1 = get_tsc_and_timecount(tc, &mut tsc1, &mut count1);
        delay(delay_usec as u32);
        let err2 = get_tsc_and_timecount(tc, &mut tsc2, &mut count2);

        // SAFETY: `s` is this CPU's flags saved just above.
        unsafe { intr_restore(s) };

        if err1 || err2 {
            continue;
        }

        let usec = calculate_tc_delay(tc, count1, count2) as i64;

        if usec < delay_usec - RECALIBRATE_DELAY_THRESHOLD
            || usec > delay_usec + RECALIBRATE_DELAY_THRESHOLD
        {
            continue;
        }

        let frequency = calculate_tsc_freq(tsc1, tsc2, usec as u64);

        min_freq = min_freq.min(frequency);
        success += 1;
    }

    if success > 1 { min_freq } else { 0 }
}

/// `calibrate_tsc_freq`: measures the TSC against the reference timecounter in `tc_priv`,
/// if there is one and the frequency is a measurement.
pub fn calibrate_tsc_freq() {
    let reference = TSC_TIMECOUNTER.tc_priv.get().cast::<Timecounter>();
    if reference.is_null() || TSC_RECALIBRATE.load(Ordering::Relaxed) == 0 {
        return;
    }

    // SAFETY: `tc_priv` only ever holds a `&'static Timecounter` (`cpu_recalibrate_tsc`).
    let freq = measure_tsc_freq(unsafe { &*reference });
    if freq == 0 {
        return;
    }
    TSC_FREQUENCY.store(freq, Ordering::Relaxed);
    TSC_TIMECOUNTER.tc_frequency.set(freq);
    if TSC_IS_INVARIANT.load(Ordering::Relaxed) != 0 {
        TSC_TIMECOUNTER.tc_quality.set(2000);
    }
}

/// `cpu_recalibrate_tsc`: a timecounter driver offers `tc` as the TSC's reference; it is
/// taken unless the current reference is better.
pub fn cpu_recalibrate_tsc(tc: &'static Timecounter) {
    let reference = TSC_TIMECOUNTER.tc_priv.get().cast::<Timecounter>();

    // Prevent recalibration with a worse timecounter source
    // SAFETY: as in `calibrate_tsc_freq`.
    if let Some(reference) = unsafe { reference.as_ref() }
        && reference.tc_quality.get() > tc.tc_quality.get()
    {
        return;
    }

    TSC_TIMECOUNTER.tc_priv.set(ptr::from_ref(tc).cast());
    calibrate_tsc_freq();
}

/// `tsc_get_timecount_lfence`.
pub fn tsc_get_timecount_lfence(_tc: &Timecounter) -> u32 {
    rdtsc_lfence() as u32
}

/// `tsc_get_timecount_rdtscp`.
pub fn tsc_get_timecount_rdtscp(_tc: &Timecounter) -> u32 {
    rdtscp() as u32
}

/// `tsc_timecounter_init`: registers the TSC timecounter on the primary CPU with a constant,
/// invariant TSC, at its known frequency, or at `cpufreq` to be recalibrated.
pub fn tsc_timecounter_init(ci: &CpuInfo, cpufreq: u64) {
    let flags = ci.ci_flags.load(Ordering::Relaxed);
    if flags & CPUF_PRIMARY == 0 || flags & CPUF_CONST_TSC == 0 || flags & CPUF_INVAR_TSC == 0 {
        return;
    }

    // Newer CPUs don't require recalibration
    let freq = TSC_FREQUENCY.load(Ordering::Relaxed);
    if freq > 0 {
        TSC_TIMECOUNTER.tc_frequency.set(freq);
        TSC_TIMECOUNTER.tc_quality.set(2000);
    } else {
        TSC_RECALIBRATE.store(1, Ordering::Relaxed);
        TSC_FREQUENCY.store(cpufreq, Ordering::Relaxed);
        TSC_TIMECOUNTER.tc_frequency.set(cpufreq);
        calibrate_tsc_freq();

        // No acpitimer/acpihpet to recalibrate against; see the module's deviations.
        #[cfg(feature = "qemu")]
        if TSC_TIMECOUNTER.tc_priv.get().is_null()
            && TSC_IS_INVARIANT.load(Ordering::Relaxed) != 0
            && cpufreq > 0
        {
            TSC_TIMECOUNTER.tc_quality.set(2000);
        }
    }

    tc_init(&TSC_TIMECOUNTER);
}

/// `tsc_delay`: busy-waits `usecs` microseconds on the TSC.
pub fn tsc_delay(usecs: i32) {
    let interval =
        u64::from(usecs.max(0) as u32) * TSC_FREQUENCY.load(Ordering::Relaxed) / 1_000_000;
    let start = tsc_rdtsc();
    while tsc_rdtsc().wrapping_sub(start) < interval {
        core::hint::spin_loop(); // CPU_BUSY_CYCLE()
    }
}

#[cfg(feature = "multiprocessor")]
pub use mp::*;

/// The `MULTIPROCESSOR` part: the TSC synchronisation test between the BP and each AP.
///
/// Protections for global variables in this code:
///
/// - a: modified atomically
/// - b: protected by a barrier
/// - p: only modified by the primary CPU
#[cfg(feature = "multiprocessor")]
mod mp {
    use core::sync::atomic::{AtomicI64, AtomicPtr, AtomicU32};

    use super::*;
    use crate::arch::amd64::include::cpufunc::wrmsr;
    use crate::arch::amd64::include::specialreg::{MSR_TSC_ADJUST, SEFF0EBX_TSC_ADJUST};
    use crate::kern::kern_tc::tc_reset_quality;
    use crate::kern::subr_prf::{Str, panic, printf};

    /// `TSC_TEST_MSECS`: test round duration.
    const TSC_TEST_MSECS: u64 = 1;
    /// `TSC_TEST_ROUNDS`: number of test rounds.
    const TSC_TEST_ROUNDS: u32 = 2;

    /// `struct tsc_test_status`. `val` is isolated to its own cache line to limit false
    /// sharing and reduce the test's margin of error.
    #[repr(C, align(64))]
    pub struct TscTestStatus {
        /// \[a\] latest RDTSC value.
        pub val: AtomicU64,
        pad1: [u64; 7],
        /// \[b\] number of lags seen by CPU.
        pub lag_count: AtomicU64,
        /// \[b\] biggest lag seen by CPU.
        pub lag_max: AtomicU64,
        /// \[b\] initial IA32_TSC_ADJUST value.
        pub adj: AtomicI64,
        pad2: [u64; 5],
    }

    impl TscTestStatus {
        const fn new() -> Self {
            Self {
                val: AtomicU64::new(0),
                pad1: [0; 7],
                lag_count: AtomicU64::new(0),
                lag_max: AtomicU64::new(0),
                adj: AtomicI64::new(0),
                pad2: [0; 5],
            }
        }

        /// `memset(tts, 0, sizeof *tts)`.
        fn clear(&self) {
            self.val.store(0, Ordering::Relaxed);
            self.lag_count.store(0, Ordering::Relaxed);
            self.lag_max.store(0, Ordering::Relaxed);
            self.adj.store(0, Ordering::Relaxed);
        }

        /// Records a lag of `lag` cycles.
        fn lag(&self, lag: u64) {
            self.lag_count.fetch_add(1, Ordering::Relaxed);
            self.lag_max.fetch_max(lag, Ordering::Relaxed);
        }
    }

    /// `tsc_ap_status`: test results from AP.
    pub static TSC_AP_STATUS: TscTestStatus = TscTestStatus::new();
    /// `tsc_bp_status`: test results from BP.
    pub static TSC_BP_STATUS: TscTestStatus = TscTestStatus::new();
    /// \[p\] `tsc_test_cycles`: TSC cycles per test round.
    static TSC_TEST_CYCLES: AtomicU64 = AtomicU64::new(0);
    /// \[b\] `tsc_ap_name`: name of AP running test.
    static TSC_AP_NAME: AtomicPtr<[u8; 16]> = AtomicPtr::new(ptr::null_mut());
    /// \[a\] `tsc_egress_barrier`: test end barrier.
    static TSC_EGRESS_BARRIER: AtomicU32 = AtomicU32::new(0);
    /// \[a\] `tsc_ingress_barrier`: test start barrier.
    static TSC_INGRESS_BARRIER: AtomicU32 = AtomicU32::new(0);
    /// \[p\] `tsc_test_rounds`: remaining test rounds.
    static TSC_TEST_ROUNDS_LEFT: AtomicU32 = AtomicU32::new(0);
    /// \[p\] `tsc_is_synchronized`: have we ever failed the test?
    pub static TSC_IS_SYNCHRONIZED: AtomicI32 = AtomicI32::new(1);

    /// The AP's name for the messages.
    fn ap_name() -> &'static [u8] {
        // SAFETY: `TSC_AP_NAME` holds null or an AP's `dv_xname`, which lives as long as the
        // device (forever for a CPU).
        unsafe { TSC_AP_NAME.load(Ordering::Relaxed).as_ref() }.map_or(&[], |n| &n[..])
    }

    /// `tsc_test_sync_bp`: the BP's side of the synchronisation test with one AP.
    pub fn tsc_test_sync_bp(ci: &CpuInfo) {
        if TSC_IS_INVARIANT.load(Ordering::Relaxed) == 0 {
            return;
        }
        // No point in testing again if we already failed (!TSC_DEBUG).
        if TSC_IS_SYNCHRONIZED.load(Ordering::Relaxed) == 0 {
            return;
        }
        // Reset IA32_TSC_ADJUST if it exists.
        tsc_adjust_reset(ci, &TSC_BP_STATUS);

        // Reset the test cycle limit and round count.
        TSC_TEST_CYCLES.store(
            TSC_TEST_MSECS * TSC_FREQUENCY.load(Ordering::Relaxed) / 1000,
            Ordering::Relaxed,
        );
        TSC_TEST_ROUNDS_LEFT.store(TSC_TEST_ROUNDS, Ordering::Relaxed);

        loop {
            // Pass through the ingress barrier, run the test, then wait for the AP to reach
            // the egress barrier.
            TSC_INGRESS_BARRIER.fetch_add(1, Ordering::SeqCst);
            while TSC_INGRESS_BARRIER.load(Ordering::SeqCst) != 2 {
                core::hint::spin_loop(); // CPU_BUSY_CYCLE()
            }
            tsc_test_bp();
            while TSC_EGRESS_BARRIER.load(Ordering::SeqCst) != 1 {
                core::hint::spin_loop(); // CPU_BUSY_CYCLE()
            }

            // Report what happened. Adjust the TSC's quality if this is the first time we've
            // failed the test.
            tsc_report_test_results();
            if TSC_AP_STATUS.lag_count.load(Ordering::Relaxed) != 0
                || TSC_BP_STATUS.lag_count.load(Ordering::Relaxed) != 0
            {
                if TSC_IS_SYNCHRONIZED.load(Ordering::Relaxed) != 0 {
                    TSC_IS_SYNCHRONIZED.store(0, Ordering::Relaxed);
                    tc_reset_quality(&TSC_TIMECOUNTER, -1000);
                }
                TSC_TEST_ROUNDS_LEFT.store(0, Ordering::Relaxed);
            } else {
                TSC_TEST_ROUNDS_LEFT.fetch_sub(1, Ordering::Relaxed);
            }

            // Clean up for the next round. It is safe to reset the ingress barrier because at
            // this point we know the AP has reached the egress barrier.
            TSC_AP_STATUS.clear();
            TSC_BP_STATUS.clear();
            TSC_INGRESS_BARRIER.store(0, Ordering::SeqCst);
            if TSC_TEST_ROUNDS_LEFT.load(Ordering::Relaxed) == 0 {
                TSC_AP_NAME.store(ptr::null_mut(), Ordering::SeqCst);
            }

            // Pass through the egress barrier and release the AP. The AP is responsible for
            // resetting the egress barrier.
            if TSC_EGRESS_BARRIER.fetch_add(1, Ordering::SeqCst) + 1 != 2 {
                panic(format_args!("tsc_test_sync_bp: unexpected egress count"));
            }
            if TSC_TEST_ROUNDS_LEFT.load(Ordering::Relaxed) == 0 {
                break;
            }
        }
    }

    /// `tsc_test_sync_ap`: the AP's side of the synchronisation test.
    pub fn tsc_test_sync_ap(ci: &CpuInfo) {
        if TSC_IS_INVARIANT.load(Ordering::Relaxed) == 0 {
            return;
        }
        // !TSC_DEBUG
        if TSC_IS_SYNCHRONIZED.load(Ordering::Relaxed) == 0 {
            return;
        }
        // SAFETY: an AP being tested has attached, so `ci_dev` is its device.
        let Some(dev) = (unsafe { ci.ci_dev.get().as_ref() }) else {
            return;
        };
        // The BP needs our name in order to report any problems.
        if TSC_AP_NAME
            .compare_exchange(
                ptr::null_mut(),
                dev.dv_xname.as_ptr(),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_err()
        {
            panic(format_args!(
                "tsc_test_sync_ap: {}: tsc_ap_name is not NULL: {}",
                dev.xname(),
                Str(ap_name())
            ));
        }

        tsc_adjust_reset(ci, &TSC_AP_STATUS);

        // The AP is only responsible for running the test and resetting the egress barrier.
        // The BP handles everything else.
        loop {
            TSC_INGRESS_BARRIER.fetch_add(1, Ordering::SeqCst);
            while TSC_INGRESS_BARRIER.load(Ordering::SeqCst) != 2 {
                core::hint::spin_loop(); // CPU_BUSY_CYCLE()
            }
            tsc_test_ap();
            TSC_EGRESS_BARRIER.fetch_add(1, Ordering::SeqCst);
            while TSC_EGRESS_BARRIER
                .compare_exchange(2, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                core::hint::spin_loop(); // CPU_BUSY_CYCLE()
            }
            if TSC_TEST_ROUNDS_LEFT.load(Ordering::Relaxed) == 0 {
                break;
            }
        }
    }

    /// `tsc_report_test_results` (without `TSC_DEBUG`).
    pub fn tsc_report_test_results() {
        if TSC_AP_STATUS.lag_count.load(Ordering::Relaxed) > 0
            || TSC_BP_STATUS.lag_count.load(Ordering::Relaxed) > 0
        {
            printf(format_args!(
                "tsc: cpu0/{}: sync test failed\n",
                Str(ap_name())
            ));
        }
    }

    /// `tsc_adjust_reset`: reset IA32_TSC_ADJUST if we have it.
    pub fn tsc_adjust_reset(ci: &CpuInfo, tts: &TscTestStatus) {
        if ci.ci_feature_sefflags_ebx.get() & SEFF0EBX_TSC_ADJUST != 0 {
            // SAFETY: cpuid(7).ebx says the MSR exists; reading it has no side effects.
            let adj = unsafe { rdmsr(MSR_TSC_ADJUST) } as i64;
            tts.adj.store(adj, Ordering::Relaxed);
            if adj != 0 {
                // SAFETY: zeroing the adjustment is what this test exists to do; the
                // timecounter is re-read against the result.
                unsafe { wrmsr(MSR_TSC_ADJUST, 0) };
            }
        }
    }

    /// `tsc_test_ap`: reads the BP's latest TSC value, then this AP's; LFENCE is
    /// serializing, so `bp_val` predates `ap_val`, and an `ap_val` below it means the AP's
    /// TSC trails the BP's.
    pub fn tsc_test_ap() {
        let mut ap_val = tsc_rdtsc();
        let end = ap_val.wrapping_add(TSC_TEST_CYCLES.load(Ordering::Relaxed));
        while ap_val < end {
            let bp_val = TSC_BP_STATUS.val.load(Ordering::SeqCst);
            ap_val = tsc_rdtsc();
            TSC_AP_STATUS.val.store(ap_val, Ordering::SeqCst);

            // Record the magnitude of the problem if the AP's TSC trails the BP's TSC.
            if ap_val < bp_val {
                TSC_AP_STATUS.lag(bp_val - ap_val);
            }
        }
    }

    /// `tsc_test_bp`: `tsc_test_ap` from the BP's perspective.
    pub fn tsc_test_bp() {
        let mut bp_val = tsc_rdtsc();
        let end = bp_val.wrapping_add(TSC_TEST_CYCLES.load(Ordering::Relaxed));
        while bp_val < end {
            let ap_val = TSC_AP_STATUS.val.load(Ordering::SeqCst);
            bp_val = tsc_rdtsc();
            TSC_BP_STATUS.val.store(bp_val, Ordering::SeqCst);

            if bp_val < ap_val {
                TSC_BP_STATUS.lag(ap_val - bp_val);
            }
        }
    }

    const _: () = assert!(size_of::<TscTestStatus>() == 128);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_delay_allows_one_wrap() {
        let tc = Timecounter::new(tsc_get_timecount_lfence, 0x7fff, 1_000_000, "t", 0, 0);
        assert_eq!(calculate_tc_delay(&tc, 100, 200), 100);
        assert_eq!(
            calculate_tc_delay(&tc, 0x7f00, 0x10),
            0x10 + 0x7fff - 0x7f00
        );
        assert_eq!(
            calculate_tsc_freq(1000, 1000 + 2_400_000, 1000),
            2_400_000_000
        );
    }
}
