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
//! Interprocessor interrupt handlers: `arch/amd64/amd64/ipifuncs.c` (`MULTIPROCESSOR`).
//!
//! Upstream: sys/arch/amd64/amd64/ipifuncs.c @ 3ce1f3f79392
//!
//! `ipifunc[]` maps each `X86_IPI_*` bit to what the receiving CPU does
//! (`x86_ipi_handler`, `ipi.rs`): halt, nothing (the NOP that only wakes an idle CPU), write
//! back the caches, ...
//!
//! ## Deviations
//! - The entries for subsystems that are not ported are visible stubs:
//!   `x86_64_ipi_reload_mtrr` (option `MTRR`: `amd64_mem.c`'s `mem_range_softc` is not
//!   ported), `x86_setperf_ipi` (`mp_setperf.c`) and `x86_64_ipi_xcall` (`kern_xcall.c`:
//!   the C posts `SIR_XCALL`, whose `Xxcallintr` source does not exist here, so the stub
//!   posts nothing). `NVMM` and `NPCTR` are not configured: their entries are `None`, as the
//!   C's `NULL`. `x86_ipi_db` is `db_interface.rs`'s, as in the C (M11c).
//! - `x86_64_ipi_halt` waits in `cpu_suspend_cycle_fcn` when acpicpu(4) installed one
//!   (`acpicpu_suspend`, M16e), as the C does, and on `hlt` otherwise.

use core::arch::asm;
use core::sync::atomic::Ordering;

use crate::arch::amd64::amd64::db_interface::x86_ipi_db;
use crate::arch::amd64::amd64::lapic::lapic_disable;
use crate::arch::amd64::include::cpu::{CPUF_RUNNING, CpuInfo};
use crate::arch::amd64::include::cpufunc::{intr_disable, wbinvd};
use crate::arch::amd64::include::intrdefs::X86_NIPI;
use crate::kern::sched_bsd::sched_assert_unlocked;
use crate::sys::systm::kernel_assert_unlocked;
use crate::unported;

/// `ipifunc[X86_NIPI]`: the handler of each `X86_IPI_*` bit, by bit number.
pub static IPIFUNC: [Option<fn(&CpuInfo)>; X86_NIPI] = [
    Some(x86_64_ipi_halt),
    Some(x86_64_ipi_nop),
    None, // NVMM > 0: x86_64_ipi_vmclear_vmm
    None,
    None, // NPCTR > 0: x86_64_ipi_reload_pctr (pctr_reload)
    Some(x86_64_ipi_reload_mtrr),
    Some(x86_setperf_ipi),
    Some(x86_ipi_db), // DDB
    None,             // NVMM > 0: x86_64_ipi_start_vmm
    None,             // NVMM > 0: x86_64_ipi_stop_vmm
    Some(x86_64_ipi_wbinvd),
    Some(x86_64_ipi_xcall),
    None,
];

/// `x86_64_ipi_nop`: nothing; the interrupt itself woke the CPU.
pub fn x86_64_ipi_nop(_ci: &CpuInfo) {}

/// `x86_64_ipi_halt`: stops this CPU for good (`boot(9)`'s `X86_IPI_HALT`).
pub fn x86_64_ipi_halt(ci: &CpuInfo) {
    sched_assert_unlocked();
    kernel_assert_unlocked();

    let _ = intr_disable();
    lapic_disable();
    wbinvd();
    ci.ci_flags.fetch_and(!CPUF_RUNNING, Ordering::SeqCst);
    wbinvd();

    loop {
        // SAFETY: written only while cold (machdep.rs, the module's deviations).
        match unsafe { crate::arch::amd64::amd64::machdep::CPU_SUSPEND_CYCLE_FCN.read() } {
            Some(f) => f(),
            // SAFETY: with interrupts disabled `hlt` parks the CPU; nothing else is touched.
            None => unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) },
        }
    }
}

/// `x86_64_ipi_reload_mtrr` (option `MTRR`): see the module's deviations.
pub fn x86_64_ipi_reload_mtrr(_ci: &CpuInfo) {
    let _ = unported!("x86_64_ipi_reload_mtrr: mem_range_softc (amd64_mem.c)");
}

/// `x86_setperf_ipi` (`mp_setperf.c`): see the module's deviations.
pub fn x86_setperf_ipi(_ci: &CpuInfo) {
    let _ = unported!("x86_setperf_ipi (mp_setperf.c)");
}

/// `x86_64_ipi_wbinvd`: write back and invalidate this CPU's caches.
pub fn x86_64_ipi_wbinvd(_ci: &CpuInfo) {
    wbinvd();
}

/// `x86_64_ipi_xcall`: see the module's deviations.
pub fn x86_64_ipi_xcall(_ci: &CpuInfo) {
    // x86_atomic_setbits_u64(&ci->ci_ipending, 1UL << SIR_XCALL): no Xxcallintr source.
    let _ = unported!("x86_64_ipi_xcall (kern_xcall.c)");
}
/* </CODE> */
