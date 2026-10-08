/*	$OpenBSD: ipi.c,v 1.18 2022/11/10 08:26:54 jmatthew Exp $	*/
/*	$NetBSD: ipi.c,v 1.2 2003/03/01 13:05:37 fvdl Exp $	*/
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
/* </LICENSES> */

/* <CODE> */
//! Sending and taking inter-processor interrupts: `arch/amd64/amd64/ipi.c`
//! (`MULTIPROCESSOR`).
//!
//! Upstream: sys/arch/amd64/amd64/ipi.c @ 3ce1f3f79392
//!
//! `x86_send_ipi` posts IPI bits (`X86_IPI_*`) in the target's `ci_ipis` and sends
//! `LAPIC_IPI_VECTOR`; the target's `Xintr_lapic_ipi` (`vector.S`) calls `x86_ipi_handler`
//! at `IPL_IPI`, which runs `ipifunc[]` (`ipifuncs.rs`) for each bit. `x86_fast_ipi` sends
//! one of the vectors that have a stub of their own (the TLB shootdowns).
//!
//! ## Deviations
//! - `x86_fast_ipi` returns `Result<(), Errno>` (`ENOENT` for a CPU that is not running)
//!   instead of an `int`.
//! - `x86_ipi_selftest` is not in the C: the boot check (feature `qemu`) that every running
//!   application processor takes a `X86_IPI_NOP` and acknowledges the TLB shootdowns of a
//!   kernel page and of a range; `cpu_boot_secondary_processors` calls it.

use core::ptr;
use core::sync::atomic::Ordering;

use crate::arch::amd64::amd64::ipifuncs::IPIFUNC;
use crate::arch::amd64::amd64::lapic::{IPI_COUNT, x86_ipi};
use crate::arch::amd64::include::cpu::{CPUF_RUNNING, CpuInfo, cpu_info_primary, curcpu};
use crate::arch::amd64::include::i82489reg::{LAPIC_DEST_ALLEXCL, LAPIC_DLMODE_FIXED};
use crate::arch::amd64::include::i82489var::LAPIC_IPI_VECTOR;
use crate::arch::amd64::include::intrdefs::X86_NIPI;
use crate::kern::subr_evcount::evcount_inc;
use crate::sys::errno::Errno;

/// `x86_send_ipi`: posts `ipimask` to `ci` and interrupts it, if it runs.
pub fn x86_send_ipi(ci: &CpuInfo, ipimask: u32) {
    ci.ci_ipis.fetch_or(ipimask, Ordering::SeqCst);

    // Don't send IPI to cpu which isn't (yet) running.
    if ci.ci_flags.load(Ordering::Acquire) & CPUF_RUNNING == 0 {
        return;
    }

    x86_ipi(LAPIC_IPI_VECTOR, ci.ci_apicid.get(), LAPIC_DLMODE_FIXED);
}

/// `x86_fast_ipi`: sends the vector `ipi` (one with a stub of its own) to `ci`.
pub fn x86_fast_ipi(ci: &CpuInfo, ipi: i32) -> Result<(), Errno> {
    if ci.ci_flags.load(Ordering::Acquire) & CPUF_RUNNING == 0 {
        return Err(Errno::ENOENT);
    }

    x86_ipi(ipi, ci.ci_apicid.get(), LAPIC_DLMODE_FIXED);

    Ok(())
}

/// `x86_broadcast_ipi`: posts `ipimask` to every other running CPU and interrupts them all.
pub fn x86_broadcast_ipi(ipimask: u32) {
    let self_ = curcpu();
    let mut count = 0;

    let mut next: *const CpuInfo = cpu_info_primary();
    // SAFETY: CPU_INFO_FOREACH: the list links cpu_info structures that are never freed.
    while let Some(ci) = unsafe { next.as_ref() } {
        next = ci.ci_next.get();
        if ptr::eq(ci, self_) {
            continue;
        }
        if ci.ci_flags.load(Ordering::Acquire) & CPUF_RUNNING == 0 {
            continue;
        }
        ci.ci_ipis.fetch_or(ipimask, Ordering::SeqCst);
        count += 1;
    }
    if count == 0 {
        return;
    }

    x86_ipi(LAPIC_IPI_VECTOR, LAPIC_DEST_ALLEXCL, LAPIC_DLMODE_FIXED);
}

/// `x86_ipi_handler`: runs the IPI functions posted to this CPU, from `Xintr_lapic_ipi`
/// (or `Xresume_lapic_ipi`/`Xrecurse_lapic_ipi`) at `IPL_IPI`.
#[unsafe(no_mangle)]
pub extern "C" fn x86_ipi_handler() {
    let ci = curcpu();

    let floor = ci.ci_handled_intr_level.get();
    ci.ci_handled_intr_level.set(ci.ci_ilevel.get());

    let mut pending = ci.ci_ipis.swap(0, Ordering::SeqCst);
    let mut bit = 0;
    while bit < X86_NIPI && pending != 0 {
        if pending & (1 << bit) != 0 {
            pending &= !(1 << bit);
            if let Some(func) = IPIFUNC[bit] {
                func(ci);
            }
            evcount_inc(&IPI_COUNT);
        }
        bit += 1;
    }

    ci.ci_handled_intr_level.set(floor);
}

/// The boot check of the IPIs (see the module's deviations): a `X86_IPI_NOP` to every other
/// running CPU, each waited for until its handler has taken it, then the shootdown of one
/// kernel page and of a range, which every running CPU must acknowledge. Panics when a CPU
/// does not answer within a second; prints what it saw.
#[cfg(feature = "qemu")]
pub fn x86_ipi_selftest() {
    use crate::arch::amd64::amd64::machdep::delay;
    use crate::arch::amd64::amd64::pmap::{
        TLB_SHOOT_COUNTS, pmap_kernel, pmap_tlb_shootpage, pmap_tlb_shootrange, pmap_tlb_shootwait,
    };
    use crate::arch::amd64::include::intrdefs::X86_IPI_NOP;
    use crate::arch::amd64::include::param::PAGE_SIZE;
    use crate::kern::subr_prf::{panic, printf};

    let self_ = curcpu();
    let mut answered = 0;
    let mut next: *const CpuInfo = cpu_info_primary();
    // SAFETY: as in `x86_broadcast_ipi`.
    while let Some(ci) = unsafe { next.as_ref() } {
        next = ci.ci_next.get();
        if ptr::eq(ci, self_) || ci.ci_flags.load(Ordering::Acquire) & CPUF_RUNNING == 0 {
            continue;
        }
        let before = IPI_COUNT.ec_count.load(Ordering::Relaxed);
        x86_send_ipi(ci, X86_IPI_NOP);
        let mut i = 100_000;
        while (ci.ci_ipis.load(Ordering::Acquire) != 0
            || IPI_COUNT.ec_count.load(Ordering::Relaxed) == before)
            && i > 0
        {
            delay(10);
            i -= 1;
        }
        if ci.ci_ipis.load(Ordering::Acquire) != 0 {
            panic(format_args!(
                "x86_ipi_selftest: cpu{} did not take X86_IPI_NOP",
                ci.ci_cpuid.get()
            ));
        }
        answered += 1;
    }

    // The shootdowns: the page of this function's text and the pages after it; every running
    // CPU may cache a kernel address, so every one is a target.
    let va = (x86_ipi_selftest as *const () as usize) & !(PAGE_SIZE - 1);
    let me = self_.ci_cpuid.get() as usize;
    let wait = |what: &str| {
        let mut i = 100_000;
        while TLB_SHOOT_COUNTS[me].load(Ordering::Acquire) != 0 && i > 0 {
            delay(10);
            i -= 1;
        }
        let left = TLB_SHOOT_COUNTS[me].load(Ordering::Acquire);
        if left != 0 {
            panic(format_args!(
                "x86_ipi_selftest: {what}: {left} cpus did not acknowledge"
            ));
        }
        pmap_tlb_shootwait();
    };
    pmap_tlb_shootpage(pmap_kernel(), va, true);
    wait("pmap_tlb_shootpage");
    pmap_tlb_shootrange(pmap_kernel(), va, va + 4 * PAGE_SIZE, true);
    wait("pmap_tlb_shootrange");

    printf(format_args!(
        "x86_ipi_selftest: X86_IPI_NOP taken by {answered} cpus, tlb shootdowns acknowledged\n"
    ));
}
/* </CODE> */
