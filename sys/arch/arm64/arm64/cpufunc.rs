/* $OpenBSD: cpufunc_asm.S,v 1.9 2026/06/23 11:45:54 kettenis Exp $ */
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

//! arm64 TLB and cache maintenance: `arch/arm64/arm64/cpufunc_asm.S`.
//!
//! Upstream: sys/arch/arm64/arm64/cpufunc_asm.S @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports the TLB invalidations (`cpu_tlb_flush`,
//! `cpu_tlb_flush_asid`, `cpu_tlb_flush_all_asid`, `cpu_tlb_flush_asid_all`) and `cpu_setttb`.
//! M6 adds `cpu_icache_sync_range`, which reads the line sizes from `CTR_EL0` itself (the
//! C takes them from `cpu.c`'s probe). `cpu_dcache_*_range` and `cpu_idcache_wbinv_range`
//! follow when a caller needs them.
//!
//! ## Deviations
//! - Each routine is an `asm!` block instead of a `.S` entry: they are a few instructions each
//!   and have no stack frame; the `RETGUARD` prologue has no meaning in Rust.
//! - The `CPTAG_REPEAT_TLBI` code patch (an erratum workaround that repeats the `tlbi` at
//!   run time on affected cores) waits for `codepatch` (M4).

use core::arch::asm;

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
