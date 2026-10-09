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
/* </LICENSES> */

/* <CODE> */
//! `arch/arm64/arm64/locore.S`, pulled in from the `.S` file next to this module (the file
//! keeps OpenBSD's licence block and layout; `{NAME}` placeholders are what `assym.h`,
//! `<machine/armreg.h>`, `<machine/hypervisor.h>` and `<sys/syscall.h>` provide in C).
//!
//! Upstream: sys/arch/arm64/arm64/locore.S @ 3ce1f3f79392
//!
//! Status: `wip`. `kern_sig.c` brings `sigcode` (with `sigcodecall`, `sigcoderet`,
//! `esigcode`, `sigfill` and `sigfillsiz`), which `exec_sigcode_map` copies into every
//! process. M14 (track A3, boot(8)'s entry, `locore0.rs`) brings the boot half:
//! `drop_to_el1` with `hyp_vectors`, `get_virt_delta`, `start_mmu`, `switch_mmu_kernel`,
//! the `mair`/`tcr`/`sctlr` words, `abort`, `esym`, the boot page tables (`pagetable`) and
//! `initstack`, and the `MULTIPROCESSOR` entry of the application processors,
//! `cpu_hatch_secondary`, which PSCI `CPU_ON` starts (`machdep.rs`'s boot(8) `BootMp`).
//! Under Limine the processors arrive in `cpu.rs`'s `cpu_hatch_entry` through the boot
//! protocol; `cpu_hatch_secondary` ends there too. `cpu_hatch_secondary_spin` and
//! `cpu_hatch_ci` (spin tables) are not ported: QEMU's `virt` uses PSCI, and
//! `cpu_start_secondary` reports a spin-table processor. `HIBERNATE`'s `cpu_park` and
//! `SUSPEND`'s `cpu_hatch_primary` are not configured.
//!
//! ## Deviations
//! - The trampoline is assembled inside the softfloat kernel: `.arch_extension fp` enables
//!   the `q` register moves for it alone, as the C file does, and `.arch_extension nofp`
//!   switches them off again.
//! - No `RETGUARD_SETUP`/`RETGUARD_CHECK` (no retguard) and no `CODEPATCH_START`/
//!   `CODEPATCH_END(CPTAG_REPEAT_TLBI)` around the repeated `tlbi` (no codepatch; the
//!   TLB invalidations never repeat, `cpu.rs`'s deviations).
//! - `mair` is this kernel's layout (`include/pte.rs`, deviations: 0 write-back, 1
//!   Device-nGnRE, 2 Device-nGnRnE, 3 non-cacheable, 4 write-through), and `tcr` has
//!   `T1SZ` 16 (a 48-bit, four-level kernel half, as `arm64/pmap.rs` walks it) where the C
//!   has `64 - VIRT_BITS` (39).
//! - `drop_to_el1` points `VBAR_EL2` at `hyp_vectors` with `adr` alone: with the MMU off it
//!   is the physical address; the C subtracts `x29`, which `get_virt_delta` has not set yet
//!   when `drop_to_el1` runs.
//! - The page tables: two more than the C's five, `pagetable_l0_ttbr1` (four levels) and
//!   `pagetable_l1_direct` (the direct map), `locore0.rs`. `esym` is aligned (`global_asm!`
//!   shares `.data` with the Rust objects).
//! - `cpu_hatch_secondary` gets the `cpu_info`'s virtual address as PSCI's context (the C
//!   passes its physical address and reads `ci_self` and `ci_ttbr1` there with the MMU off,
//!   which needs the structure in physically contiguous memory): `TPIDR_EL1` is the context
//!   itself and the kernel's `TTBR1_EL1` comes from `cpu.rs`'s `AP_TTBR1`, which the boot
//!   processor cleaned to the point of coherency before `CPU_ON`. It then calls
//!   `cpu_hatch_bootarg`, which enters `cpu_hatch_entry` (the C calls `cpu_init_secondary`):
//!   there the processor takes the boot processor's `MAIR_EL1`, `TCR_EL1` and `SCTLR_EL1`
//!   and the kernel's empty `TTBR0_EL1`, as under Limine.

use core::arch::global_asm;
use core::mem::offset_of;
use core::ptr;

use crate::arch::arm64::arm64::cpu::{AP_TTBR1, cpu_hatch_entry};
use crate::arch::arm64::include::armreg::{
    CNTHCTL_EL1PCEN, CNTHCTL_EL1PCTEN, ICC_SRE_EL2_EN, ICC_SRE_EL2_SRE, ID_AA64PFR0_GIC_BITS,
    ID_AA64PFR0_GIC_CPUIF_EN, ID_AA64PFR0_GIC_SHIFT, PSR_A, PSR_D, PSR_F, PSR_I, PSR_M_EL1H,
    SCTLR_A, SCTLR_C, SCTLR_CP15BEN, SCTLR_DZE, SCTLR_EE, SCTLR_EOE, SCTLR_I, SCTLR_ITD, SCTLR_M,
    SCTLR_RES0, SCTLR_RES1, SCTLR_SA, SCTLR_SA0, SCTLR_SED, SCTLR_THEE, SCTLR_UCI, SCTLR_UCT,
    SCTLR_UMA, SCTLR_WXN, SCTLR_nTWE, SCTLR_nTWI, TCR_AS, TCR_CACHE_ATTRS, TCR_SMP_ATTRS,
    TCR_TG0_4K, TCR_TG1_4K, tcr_t0sz, tcr_t1sz,
};
use crate::arch::arm64::include::cpu::CpuInfo;
use crate::arch::arm64::include::frame::Sigframe;
use crate::arch::arm64::include::hypervisor::{
    CPTR_RES1, HCR_API, HCR_APK, HCR_E2H, HCR_RW, HCR_TGE,
};
use crate::arch::arm64::include::param::PAGE_SIZE;
use crate::arch::arm64::include::pte::{
    MAIR_CI, MAIR_DEV_NGNRE, MAIR_DEV_NGNRNE, MAIR_WB, MAIR_WT, mair_attr,
};
use crate::sys::syscall::SYS_sigreturn;

/// `locore.S`'s `mair`: this kernel's attribute layout (see the module's deviations).
const MAIR: u64 = mair_attr(MAIR_WB, 0)
    | mair_attr(MAIR_DEV_NGNRE, 1)
    | mair_attr(MAIR_DEV_NGNRNE, 2)
    | mair_attr(MAIR_CI, 3)
    | mair_attr(MAIR_WT, 4);
/// `locore.S`'s `tcr`: 48-bit halves with 4 KiB granules (see the module's deviations).
const TCR: u64 = tcr_t1sz(64 - 48)
    | tcr_t0sz(64 - 48)
    | TCR_AS
    | TCR_TG1_4K
    | TCR_TG0_4K
    | TCR_CACHE_ATTRS
    | TCR_SMP_ATTRS;
/// `locore.S`'s `sctlr_set`.
const SCTLR_SET: u64 = SCTLR_UCI
    | SCTLR_nTWE
    | SCTLR_nTWI
    | SCTLR_UCT
    | SCTLR_DZE
    | SCTLR_I
    | SCTLR_SED
    | SCTLR_SA0
    | SCTLR_SA
    | SCTLR_C
    | SCTLR_M
    | SCTLR_RES1;
/// `locore.S`'s `sctlr_clear`.
const SCTLR_CLEAR: u64 = SCTLR_EE
    | SCTLR_EOE
    | SCTLR_WXN
    | SCTLR_UMA
    | SCTLR_ITD
    | SCTLR_THEE
    | SCTLR_CP15BEN
    | SCTLR_A
    | SCTLR_RES0;
/// `initstack`'s size: 64 KiB, as Limine's boot stack (`USPACE` in C; `locore0.rs`).
const INITSTACK_SIZE: usize = 64 * 1024;

global_asm!(
    include_str!("locore.S"),
    CPTR_RES1 = const CPTR_RES1,
    CNTHCTL_EL1 = const CNTHCTL_EL1PCTEN | CNTHCTL_EL1PCEN,
    PSR_EL1H_MASKED = const PSR_F | PSR_I | PSR_A | PSR_D | PSR_M_EL1H,
    ID_AA64PFR0_GIC_SHIFT = const ID_AA64PFR0_GIC_SHIFT,
    ID_AA64PFR0_GIC_BITS = const ID_AA64PFR0_GIC_BITS,
    ID_AA64PFR0_GIC_CPUIF_EN = const ID_AA64PFR0_GIC_CPUIF_EN >> ID_AA64PFR0_GIC_SHIFT,
    ICC_SRE_EL2_EN = const ICC_SRE_EL2_EN,
    ICC_SRE_EL2_SRE = const ICC_SRE_EL2_SRE,
    SCTLR_RES1 = const SCTLR_RES1,
    HCR_HOST = const HCR_E2H | HCR_TGE,
    HCR_GUEST = const HCR_RW | HCR_API | HCR_APK,
    MAIR = const MAIR,
    TCR = const TCR,
    SCTLR_SET = const SCTLR_SET,
    SCTLR_CLEAR = const SCTLR_CLEAR,
    PAGE_SIZE = const PAGE_SIZE,
    INITSTACK_SIZE = const INITSTACK_SIZE,
    SF_SC = const offset_of!(Sigframe, sf_sc),
    SYS_SIGRETURN = const SYS_sigreturn,
    MULTIPROCESSOR = const cfg!(feature = "multiprocessor") as u32,
    AP_TTBR1 = sym AP_TTBR1,
    CI_EL1_STKEND = const offset_of!(CpuInfo, ci_el1_stkend),
    CPU_HATCH_BOOTARG = sym cpu_hatch_bootarg,
);

unsafe extern "C" {
    /// `sigcode[]`: the signal trampoline, copied into every process (`exec_sigcode_map`).
    static sigcode: [u8; 0];
    /// `sigcodecall[]`: the trampoline's `svc` instruction.
    static sigcodecall: [u8; 0];
    /// `sigcoderet[]`: the instruction after the trampoline's `sigreturn` system call (and
    /// the speculation barrier `svc_handler` skips).
    static sigcoderet: [u8; 0];
    /// `esigcode[]`: the end of the trampoline.
    static esigcode: [u8; 0];
    /// `sigfill[]`: the trap instruction the rest of the trampoline's page is filled with.
    static sigfill: [u8; 0];
    /// `sigfillsiz`: the size of `sigfill`.
    static sigfillsiz: i32;
}

/// `sigcode` .. `esigcode`: the signal trampoline's bytes.
pub fn sigcode_bytes() -> &'static [u8] {
    let start = ptr::addr_of!(sigcode).cast::<u8>();
    let end = ptr::addr_of!(esigcode).cast::<u8>();
    // SAFETY: `sigcode` and `esigcode` bracket the trampoline in `.text` (`locore.S`),
    // never written and alive for the kernel's lifetime.
    unsafe { core::slice::from_raw_parts(start, end as usize - start as usize) }
}

/// `sigcoderet - sigcode`.
pub fn sigcoderet_offset() -> usize {
    ptr::addr_of!(sigcoderet) as usize - ptr::addr_of!(sigcode) as usize
}

/// `sigcodecall - sigcode`: the `svc` instruction of the trampoline.
pub fn sigcodecall_offset() -> usize {
    ptr::addr_of!(sigcodecall) as usize - ptr::addr_of!(sigcode) as usize
}

/// `sigfill` .. `sigfill + sigfillsiz`.
pub fn sigfill_bytes() -> &'static [u8] {
    // SAFETY: `sigfillsiz` is a word in `.data` that the assembler initialised and nothing
    // writes.
    let len = unsafe { ptr::addr_of!(sigfillsiz).read() } as usize;
    // SAFETY: `sigfill` is followed by `sigfillsiz` bytes of instructions in `.text`.
    unsafe { core::slice::from_raw_parts(ptr::addr_of!(sigfill).cast::<u8>(), len) }
}

/// Where `cpu_hatch_secondary` goes once the MMU is on and the processor runs on its
/// `ci_el1_stkend` stack: the processor's entry under Limine, `cpu_hatch_entry`, with `ci`
/// its `cpu_info` (see the module's deviations).
///
/// # Safety
///
/// Only `cpu_hatch_secondary` calls it, once per application processor that
/// `machdep.rs`'s boot(8) `BootMp` started with `ci` as its context.
unsafe extern "C" fn cpu_hatch_bootarg(ci: *const CpuInfo) -> ! {
    // SAFETY: `ci` is what the boot processor passed to `BootMp::start` for this
    // processor (`cpu_start_secondary`), as `cpu_hatch_entry` requires.
    unsafe { cpu_hatch_entry(ci as usize) }
}
/* </CODE> */
