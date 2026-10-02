/*	$OpenBSD: cpu_full.h,v 1.5 2019/05/17 19:07:47 guenther Exp $	*/
/*
 * Copyright (c) 2018 Philip Guenther <guenther@openbsd.org>
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

//! amd64 `<machine/cpu_full.h>`: the layout of the full per-CPU information.
//!
//! Upstream: sys/arch/amd64/include/cpu_full.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `struct cpu_info_full` and its layout checks; the
//! `SETUP_IST_SPECIAL_STACK` macro is inlined in `cpu_enter_pages` (`amd64/cpu.rs`).
//!
//! ## Deviations
//! - The stacks are arrays of `u64` behind `UnsafeCell`, written by `cpu_enter_pages` and the
//!   hardware; `u_align` is the `align(4096)` of the struct plus an explicit pad.

use core::cell::UnsafeCell;

use crate::arch::amd64::include::cpu::{CI_PAGEALIGN, CpuInfo};
use crate::arch::amd64::include::param::PAGE_SIZE;
use crate::arch::amd64::include::segments::GDT_SIZE;
use crate::arch::amd64::include::tss::X86_64Tss;

/// Words in the trampoline stack: a quarter page less the visible head of `cpu_info`.
pub const TRAMP_STACK_WORDS: usize = (PAGE_SIZE / 4 - CI_PAGEALIGN) / size_of::<u64>();
/// Words in the double-fault stack: a quarter page.
pub const DBLFLT_STACK_WORDS: usize = (PAGE_SIZE / 4) / size_of::<u64>();
/// Words in the NMI stack: half a page.
pub const NMI_STACK_WORDS: usize = (2 * PAGE_SIZE / 4) / size_of::<u64>();
/// Padding after the TSS and the GDT up to the end of their page.
const RO_PAD: usize = PAGE_SIZE - size_of::<X86_64Tss>() - GDT_SIZE;

/// `struct cpu_info_full`: the layout of the full per-CPU information, including TSS, GDT,
/// trampoline stacks, and `cpu_info` described in `<machine/cpu.h>`.
#[repr(C, align(4096))]
pub struct CpuInfoFull {
    // page mapped kRO in u-k
    /// `cif_tss`.
    pub cif_tss: UnsafeCell<X86_64Tss>,
    /// `cif_gdt`.
    pub cif_gdt: UnsafeCell<[u64; GDT_SIZE / 8]>,
    /// The rest of the read-only page (`u_align`).
    _ro_pad: [u8; RO_PAD],
    // start of page mapped kRW in u-k
    /// `cif_tramp_stack`.
    pub cif_tramp_stack: UnsafeCell<[u64; TRAMP_STACK_WORDS]>,
    /// `cif_dblflt_stack`.
    pub cif_dblflt_stack: UnsafeCell<[u64; DBLFLT_STACK_WORDS]>,
    /// `cif_nmi_stack`.
    pub cif_nmi_stack: UnsafeCell<[u64; NMI_STACK_WORDS]>,
    // Beginning of this hangs over into the kRW page; rest is unmapped in u-k
    /// `cif_cpu`.
    pub cif_cpu: CpuInfo,
}

// SAFETY: one CPU's pages, written by that CPU at boot (`cpu_enter_pages`, `init_x86_64`) and
// by the hardware (the TSS); the boot CPU is alone.
unsafe impl Sync for CpuInfoFull {}

impl CpuInfoFull {
    /// Zeroed pages with a fresh `cpu_info`.
    pub const fn new() -> Self {
        Self {
            cif_tss: UnsafeCell::new(X86_64Tss::new()),
            cif_gdt: UnsafeCell::new([0; GDT_SIZE / 8]),
            _ro_pad: [0; RO_PAD],
            cif_tramp_stack: UnsafeCell::new([0; TRAMP_STACK_WORDS]),
            cif_dblflt_stack: UnsafeCell::new([0; DBLFLT_STACK_WORDS]),
            cif_nmi_stack: UnsafeCell::new([0; NMI_STACK_WORDS]),
            cif_cpu: CpuInfo::new(),
        }
    }
}

impl Default for CpuInfoFull {
    fn default() -> Self {
        Self::new()
    }
}

// tss, align shim, and gdt must fit in a page; the cpu_info's hidden part starts on a page;
// the whole thing is a multiple of pages.
const _: () = {
    assert!(size_of::<X86_64Tss>() + GDT_SIZE < PAGE_SIZE);
    assert!((core::mem::offset_of!(CpuInfoFull, cif_cpu) + CI_PAGEALIGN).is_multiple_of(PAGE_SIZE));
    assert!(size_of::<CpuInfoFull>().is_multiple_of(PAGE_SIZE));
};
