/* <LICENSES> */
/*-
 * Copyright (c) 2014 Robin Randhawa
 * Copyright (c) 2015 The FreeBSD Foundation
 * All rights reserved.
 *
 * Portions of this software were developed by Andrew Turner
 * under sponsorship from the FreeBSD Foundation
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
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
//! arm64 TLB and cache maintenance: `arch/arm64/arm64/cpufunc_asm.S`.
//!
//! Upstream: sys/arch/arm64/arm64/cpufunc_asm.S @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports the TLB invalidations (`cpu_tlb_flush`,
//! `cpu_tlb_flush_asid`, `cpu_tlb_flush_all_asid`, `cpu_tlb_flush_asid_all`) and `cpu_setttb`.
//! M6 adds `cpu_icache_sync_range`, which reads the line sizes from `CTR_EL0` itself (the
//! C takes them from `cpu.c`'s probe). M7b adds `cpu_dcache_wb_range`,
//! `cpu_dcache_wbinv_range` and `cpu_dcache_inv_range` for `bus_dma`, which read
//! `dcache_line_size` from `CTR_EL0` the same way. M16f adds `cpu_idcache_wbinv_range`, for
//! `pmap_kenter_cache`'s non-cacheable mappings of managed pages (agintc's LPI and ITS
//! tables), with `idcache_line_size` from `CTR_EL0` too.
//!
//! ## Deviations
//! - Each routine is an `asm!` block instead of a `.S` entry: they are a few instructions each
//!   and have no stack frame; the `RETGUARD` prologue has no meaning in Rust.
//! - The `CPTAG_REPEAT_TLBI` code patch (an erratum workaround that repeats the `tlbi` at
//!   run time on affected cores) waits for `codepatch` (M4).

use core::arch::asm;

/// `smc_call(a0, a1, a2, a3)`: `smc #0` with the SMCCC registers `x0`..`x3`; returns `x0`.
/// The call may clobber `x0`..`x3` only (SMCCC), which is all the asm declares.
pub fn smc_call(a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    // SAFETY: `smc` traps to the secure monitor at EL3; PSCI/SMCCC calls preserve every
    // register but the argument/result ones (`x0`..`x3` are in/out here) and touch no kernel
    // memory.
    unsafe {
        asm!(
            "smc #0",
            inout("x0") a0 => ret,
            inout("x1") a1 => _,
            inout("x2") a2 => _,
            inout("x3") a3 => _,
            options(nostack),
        )
    };
    ret
}

/// `hvc_call(a0, a1, a2, a3)`: as [`smc_call`] with `hvc #0` (to EL2).
pub fn hvc_call(a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    // SAFETY: as for `smc_call`; `hvc` enters the hypervisor (or the firmware at EL2).
    unsafe {
        asm!(
            "hvc #0",
            inout("x0") a0 => ret,
            inout("x1") a1 => _,
            inout("x2") a2 => _,
            inout("x3") a3 => _,
            options(nostack),
        )
    };
    ret
}

/// `cpu_setttb(asid, pt0pa)`: switches `TTBR1_EL1`'s ASID and `TTBR0_EL1`.
///
/// # Safety
///
/// `pt0pa` must be the physical address of a level-0 table valid for the lower half, and
/// `asid` an address space id the caller owns.
pub unsafe fn cpu_setttb(asid: u64, pt0pa: u64) {
    // SAFETY: the caller's guarantee; the `isb`s order the register writes before later
    // translations.
    unsafe {
        asm!(
            "mrs {tmp}, ttbr1_el1",
            "bfi {tmp}, {asid}, #48, #16",
            "msr ttbr1_el1, {tmp}",
            "isb",
            "msr ttbr0_el1, {pt0pa}",
            "isb",
            tmp = out(reg) _,
            asid = in(reg) asid,
            pt0pa = in(reg) pt0pa,
            options(nostack, preserves_flags)
        )
    };
}

/// `cpu_wfi`: `dsb sy; wfi`: waits for an interrupt, the default `cpu_idle_cycle_fcn`.
pub fn cpu_wfi() {
    // SAFETY: waiting for an interrupt has no effect on memory or registers; the barrier
    // only completes pending stores first.
    unsafe { asm!("dsb sy", "wfi", options(nomem, nostack, preserves_flags)) };
}

/// `cpu_tlb_flush`: invalidates every TLB entry of this inner-shareable domain.
pub fn cpu_tlb_flush() {
    // SAFETY: TLB invalidation only forces refetches; the barriers complete pending table
    // writes first (`dsb ishst`) and the invalidation after (`dsb ish; isb`).
    unsafe {
        asm!(
            "dsb ishst",
            "tlbi vmalle1is",
            "dsb ish",
            "isb",
            options(nostack, preserves_flags)
        )
    };
}

/// `cpu_tlb_flush_asid(va)`: invalidates one page of one ASID (`va` carries the ASID in bits
/// 63:48 and the page number in 43:0).
pub fn cpu_tlb_flush_asid(va: u64) {
    // SAFETY: as for `cpu_tlb_flush`.
    unsafe {
        asm!(
            "dsb ishst",
            "tlbi vae1is, {va}",
            "dsb ish",
            "isb",
            va = in(reg) va,
            options(nostack, preserves_flags)
        )
    };
}

/// `cpu_tlb_flush_all_asid(va)`: invalidates one page for every ASID (kernel mappings).
pub fn cpu_tlb_flush_all_asid(va: u64) {
    // SAFETY: as for `cpu_tlb_flush`.
    unsafe {
        asm!(
            "dsb ishst",
            "tlbi vaale1is, {va}",
            "dsb ish",
            "isb",
            va = in(reg) va,
            options(nostack, preserves_flags)
        )
    };
}

/// `cpu_tlb_flush_asid_all(asid)`: invalidates every entry of one ASID (bits 63:48).
pub fn cpu_tlb_flush_asid_all(asid: u64) {
    // SAFETY: as for `cpu_tlb_flush`.
    unsafe {
        asm!(
            "dsb ishst",
            "tlbi aside1is, {asid}",
            "dsb ish",
            "isb",
            asid = in(reg) asid,
            options(nostack, preserves_flags)
        )
    };
}

/// The smallest data cache line, `dcache_line_size`, from `CTR_EL0.DminLine` (log2 of words).
fn dcache_line_size() -> usize {
    let ctr: u64;
    // SAFETY: `ctr_el0` is readable at EL1 and the read has no side effect.
    unsafe { asm!("mrs {}, ctr_el0", out(reg) ctr, options(nomem, nostack, preserves_flags)) };
    4usize << ((ctr >> 16) & 0xf)
}

/// `cache_handle_range dcop`: applies one `dc` operation to every line of `[va, va + len)`,
/// then `dsb ish`.
macro_rules! dcache_range {
    ($op:literal, $va:expr, $len:expr) => {{
        let line = dcache_line_size();
        let end = $va + $len;
        let mut addr = $va & !(line - 1);
        while addr < end {
            // SAFETY: cache maintenance by address on a range the caller has mapped; the
            // caller's contract says whether discarding dirty lines is acceptable.
            unsafe { asm!(concat!("dc ", $op, ", {}"), in(reg) addr, options(nostack, preserves_flags)) };
            addr += line;
        }
        // SAFETY: a barrier.
        unsafe { asm!("dsb ish", options(nostack, preserves_flags)) };
    }};
}

/// `cpu_dcache_wb_range(va, len)`: writes the dirty data cache lines of `[va, va+len)` back
/// to memory (`dc cvac`), for a device about to read it.
pub fn cpu_dcache_wb_range(va: usize, len: usize) {
    dcache_range!("cvac", va, len);
}

/// `cpu_dcache_wbinv_range(va, len)`: writes back and invalidates the data cache lines of
/// `[va, va+len)` (`dc civac`).
pub fn cpu_dcache_wbinv_range(va: usize, len: usize) {
    dcache_range!("civac", va, len);
}

/// `cpu_dcache_inv_range(va, len)`: invalidates the data cache lines of `[va, va+len)`
/// without writing them back (`dc ivac`), so a device's writes to memory become visible.
///
/// # Safety
///
/// `[va, va+len)` is mapped and nothing the CPU wrote to it (or to the rest of its first and
/// last cache lines) is still needed: dirty lines are discarded.
pub unsafe fn cpu_dcache_inv_range(va: usize, len: usize) {
    dcache_range!("ivac", va, len);
}

/// `cpu_icache_sync_range(va, len)`: makes instructions written to `[va, va+len)` visible to
/// instruction fetch: cleans the data cache to the point of unification and invalidates the
/// instruction cache, line by line, then `dsb ish; isb`.
pub fn cpu_icache_sync_range(va: usize, len: usize) {
    let ctr: u64;
    // SAFETY: `ctr_el0` is readable at EL1 and the read has no side effect.
    unsafe { asm!("mrs {}, ctr_el0", out(reg) ctr, options(nomem, nostack, preserves_flags)) };
    // CTR_EL0.DminLine and IminLine: log2 of the line size in words.
    let dline = 4usize << ((ctr >> 16) & 0xf);
    let iline = 4usize << (ctr & 0xf);
    let end = va + len;

    let mut addr = va & !(dline - 1);
    while addr < end {
        // SAFETY: cache maintenance by address on a mapped range the caller owns.
        unsafe { asm!("dc cvau, {}", in(reg) addr, options(nostack, preserves_flags)) };
        addr += dline;
    }
    // SAFETY: a barrier.
    unsafe { asm!("dsb ish", options(nostack, preserves_flags)) };
    let mut addr = va & !(iline - 1);
    while addr < end {
        // SAFETY: as above.
        unsafe { asm!("ic ivau, {}", in(reg) addr, options(nostack, preserves_flags)) };
        addr += iline;
    }
    // SAFETY: barriers.
    unsafe { asm!("dsb ish", "isb", options(nostack, preserves_flags)) };
}

/// `cpu_idcache_wbinv_range(va, len)`: writes back and invalidates the data cache lines of
/// `[va, va+len)` (`dc civac`), then invalidates its instruction cache lines (`ic ivau`),
/// stepping by `idcache_line_size`, the smaller of the two line sizes (`CTR_EL0`); for a
/// managed page about to be mapped non-cacheable.
pub fn cpu_idcache_wbinv_range(va: usize, len: usize) {
    let ctr: u64;
    // SAFETY: `ctr_el0` is readable at EL1 and the read has no side effect.
    unsafe { asm!("mrs {}, ctr_el0", out(reg) ctr, options(nomem, nostack, preserves_flags)) };
    let line = (4usize << ((ctr >> 16) & 0xf)).min(4usize << (ctr & 0xf));
    let end = va + len;

    let mut addr = va & !(line - 1);
    while addr < end {
        // SAFETY: cache maintenance by address on a mapped range the caller owns; dirty lines
        // are written back, not discarded.
        unsafe { asm!("dc civac, {}", in(reg) addr, options(nostack, preserves_flags)) };
        addr += line;
    }
    // SAFETY: a barrier.
    unsafe { asm!("dsb ish", options(nostack, preserves_flags)) };
    let mut addr = va & !(line - 1);
    while addr < end {
        // SAFETY: as above.
        unsafe { asm!("ic ivau, {}", in(reg) addr, options(nostack, preserves_flags)) };
        addr += line;
    }
    // SAFETY: barriers.
    unsafe { asm!("dsb ish", "isb", options(nostack, preserves_flags)) };
}
/* </CODE> */
