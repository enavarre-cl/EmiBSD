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
//! The trap and fault vector routines of `arch/amd64/amd64/vector.S`, pulled in from the
//! `.S` file next to this module (the file keeps OpenBSD's licence blocks and layout; `{NAME}`
//! placeholders are what `assym.h` and the headers provide in C).
//!
//! Upstream: sys/arch/amd64/amd64/vector.S @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports the exception stubs `Xtrap00` to `Xtrap1f`, the
//! `Xexceptions` table, `alltraps`/`alltraps_kern`, the `frameasm.h` entry macros, the
//! `INTRSTUB` generic stub with the sixteen legacy (i8259) instances, `i8259_stubs[]` and
//! the soft interrupt stubs `Xsoftclock`/`Xsoftnet`/`Xsofttty`; M5 adds the LAPIC timer stub
//! `Xintr_lapic_ltimer` with its recurse/resume entries; M11a the `MULTIPROCESSOR` IPI stubs
//! `Xintr_lapic_ipi`/`Xrecurse_lapic_ipi`/`Xresume_lapic_ipi` and the TLB shootdown IPIs
//! `Xipi_invltlb`, `Xipi_invlpg`, `Xipi_invlrange`; M13 the I/O APIC stubs (`ioapic_edge0`..63,
//! `ioapic_level0`..63, their tables and the `ioapic_*` macros). `x2apic_eoi`, the
//! PCID and `NVMM` shootdowns, `Xxcallintr` and the `#VC` (AMD SEV) and `DDBPROF` paths come
//! later.
//!
//! ## Deviations
//! - NMIs and double faults use `alltraps` on their IST stacks instead of the
//!   `calltrap_specstk` path, which exists to cope with user-mode GS.base and page tables
//!   (M6). The `#GP` resume stubs (`iretq`, `xrstor`, `xsetbv`, `rdmsr_safe`) and the
//!   Meltdown `Xalltraps` page are M6 too.
//! - `alltraps_kern` and the interrupt stubs skip `SMAP_CLAC` (no CPU identification yet).
//! - `INTRENTRY`'s path from user space (swapgs, the Meltdown CR3 switch and the kernel
//!   stack) is ported (M6-b), without the Meltdown CR3 switch; `intr_user_exit` is in
//!   `locore.S`.
//! - `retpoline_r13` (a `CODEPATCH`ed Spectre thunk) is a plain `jmp *%r13`: `codepatch.c`
//!   is not ported. `uvmexp` is reached by its C name (`export_name`) for `V_INTR`.
//! - AT&T syntax, as the C file, so the two can be diffed.
//! - The `MULTIPROCESSOR` stubs sit in an `.if {MULTIPROCESSOR}` block of the one `vector.S`
//!   (the C's `#ifdef`): the `vector_asm!` macro passes 1 and the shootdown globals of
//!   `pmap.rs` with the feature, 0 for all of them without. Only the no-PCID shootdown
//!   stubs exist, since `pmap_use_pcid` is never set.

use core::arch::global_asm;
use core::mem::offset_of;

use crate::arch::amd64::include::cpu::CpuInfo;
use crate::arch::amd64::include::frame::{Intrframe, IretqFrame, Trapframe};
use crate::arch::amd64::include::i8259::IRQ_SLAVE;
use crate::arch::amd64::include::i82489reg::LAPIC_EOI;
use crate::arch::amd64::include::intr::{Intrhand, Intrsource, Intrstub};
use crate::arch::amd64::include::intrdefs::{
    IPL_CLOCK, IPL_IPI, IPL_SOFTCLOCK, IPL_SOFTNET, IPL_SOFTTTY, IREENT_MAGIC, LIR_IPI, LIR_TIMER,
    MAX_INTR_SOURCES, NUM_LEGACY_IRQS,
};
use crate::arch::amd64::include::param::PAGE_SIZE;
use crate::arch::amd64::include::segments::SEL_RPL;
use crate::arch::amd64::include::trap::{
    T_ALIGNFLT, T_ARITHTRAP, T_BOUND, T_BPTFLT, T_CP, T_DIVIDE, T_DNA, T_DOUBLEFLT, T_FPOPFLT,
    T_MCA, T_NMI, T_OFLOW, T_PAGEFLT, T_PRIVINFLT, T_PROTFLT, T_RESERVED, T_SEGNPFLT, T_STKFLT,
    T_TRCTRAP, T_TSSFLT, T_VE, T_XMM,
};
use crate::dev::isa::isareg::{IO_ICU1, IO_ICU2};
use crate::sys::softintr::{SOFTINTR_CLOCK, SOFTINTR_NET, SOFTINTR_TTY};
use crate::uvm::uvmexp::Uvmexp;

/// The `global_asm!` of `vector.S` with its placeholders; `$extra` are the
/// `MULTIPROCESSOR`-only ones (the IPI stubs), constant zeroes without the feature.
macro_rules! vector_asm {
    ($($extra:tt)*) => {
        global_asm!(
            include_str!("vector.S"),
            T_DIVIDE = const T_DIVIDE,
            T_TRCTRAP = const T_TRCTRAP,
            T_NMI = const T_NMI,
            T_BPTFLT = const T_BPTFLT,
            T_OFLOW = const T_OFLOW,
            T_BOUND = const T_BOUND,
            T_PRIVINFLT = const T_PRIVINFLT,
            T_DNA = const T_DNA,
            T_DOUBLEFLT = const T_DOUBLEFLT,
            T_FPOPFLT = const T_FPOPFLT,
            T_TSSFLT = const T_TSSFLT,
            T_SEGNPFLT = const T_SEGNPFLT,
            T_STKFLT = const T_STKFLT,
            T_PROTFLT = const T_PROTFLT,
            T_PAGEFLT = const T_PAGEFLT,
            T_ARITHTRAP = const T_ARITHTRAP,
            T_ALIGNFLT = const T_ALIGNFLT,
            T_MCA = const T_MCA,
            T_XMM = const T_XMM,
            T_VE = const T_VE,
            T_CP = const T_CP,
            T_RESERVED = const T_RESERVED,
            SEL_RPL = const SEL_RPL,
            TF_TRAPNO = const offset_of!(Trapframe, tf_trapno),
            TF_RCX = const offset_of!(Trapframe, tf_rcx),
            TF_RIP = const offset_of!(Trapframe, tf_rip),
            TF_ERR = const offset_of!(Trapframe, tf_err),
            TF_R15 = const offset_of!(Trapframe, tf_r15),
            TF_R14 = const offset_of!(Trapframe, tf_r14),
            TF_R13 = const offset_of!(Trapframe, tf_r13),
            TF_R12 = const offset_of!(Trapframe, tf_r12),
            TF_R11 = const offset_of!(Trapframe, tf_r11),
            TF_R10 = const offset_of!(Trapframe, tf_r10),
            TF_R9 = const offset_of!(Trapframe, tf_r9),
            TF_R8 = const offset_of!(Trapframe, tf_r8),
            TF_RDI = const offset_of!(Trapframe, tf_rdi),
            TF_RSI = const offset_of!(Trapframe, tf_rsi),
            TF_RBP = const offset_of!(Trapframe, tf_rbp),
            TF_RBX = const offset_of!(Trapframe, tf_rbx),
            TF_RDX = const offset_of!(Trapframe, tf_rdx),
            TF_RAX = const offset_of!(Trapframe, tf_rax),
            TF_CS = const offset_of!(Trapframe, tf_cs),
            IRETQ_CS = const offset_of!(IretqFrame, iretq_cs),
            IRETQ_RIP = const offset_of!(IretqFrame, iretq_rip),
            IRETQ_RFLAGS = const offset_of!(IretqFrame, iretq_rflags),
            IRETQ_RSP = const offset_of!(IretqFrame, iretq_rsp),
            IRETQ_SS = const offset_of!(IretqFrame, iretq_ss),
            TF_RSP = const offset_of!(Trapframe, tf_rsp),
            TF_SS = const offset_of!(Trapframe, tf_ss),
            TF_RFLAGS = const offset_of!(Trapframe, tf_rflags),
            CI_SCRATCH = const offset_of!(CpuInfo, ci_scratch),
            CI_KERN_RSP = const offset_of!(CpuInfo, ci_kern_rsp),
            IF_PPL = const offset_of!(Intrframe, if_ppl),
            IS_MAXLEVEL = const offset_of!(Intrsource, is_maxlevel),
            IS_HANDLERS = const offset_of!(Intrsource, is_handlers),
            IS_PIC = const offset_of!(Intrsource, is_pic),
            IS_PIN = const offset_of!(Intrsource, is_pin),
            IF_ERR = const offset_of!(Intrframe, if_err),
            IH_LEVEL = const offset_of!(Intrhand, ih_level),
            IH_NEXT = const offset_of!(Intrhand, ih_next),
            IH_COUNT = const offset_of!(Intrhand, ih_count),
            CI_ISOURCES = const offset_of!(CpuInfo, ci_isources),
            CI_ILEVEL = const offset_of!(CpuInfo, ci_ilevel),
            CI_IDEPTH = const offset_of!(CpuInfo, ci_idepth),
            CI_IPENDING = const offset_of!(CpuInfo, ci_ipending),
            V_INTR = const offset_of!(Uvmexp, intrs),
            IREENT_MAGIC = const IREENT_MAGIC,
            IO_ICU1 = const IO_ICU1,
            IO_ICU2 = const IO_ICU2,
            IRQ_SLAVE = const IRQ_SLAVE,
            IPL_SOFTTTY = const IPL_SOFTTTY,
            IPL_SOFTNET = const IPL_SOFTNET,
            IPL_SOFTCLOCK = const IPL_SOFTCLOCK,
            SOFTINTR_TTY = const SOFTINTR_TTY,
            SOFTINTR_NET = const SOFTINTR_NET,
            SOFTINTR_CLOCK = const SOFTINTR_CLOCK,
            LAPIC_EOI = const LAPIC_EOI,
            IPL_CLOCK = const IPL_CLOCK,
            LIR_TIMER = const LIR_TIMER,
            IPL_IPI = const IPL_IPI,
            LIR_IPI = const LIR_IPI,
            PAGE_SIZE = const PAGE_SIZE,
            $($extra)*
            options(att_syntax)
        );
    };
}

#[cfg(feature = "multiprocessor")]
vector_asm!(
    MULTIPROCESSOR = const 1,
    TLB_SHOOT_LOCK = sym crate::arch::amd64::amd64::pmap::TLB_SHOOT_LOCK,
    TLB_SHOOT_CPU = sym crate::arch::amd64::amd64::pmap::TLB_SHOOT_CPU,
    TLB_SHOOT_COUNTS = sym crate::arch::amd64::amd64::pmap::TLB_SHOOT_COUNTS,
    TLB_SHOOT_ADDR1 = sym crate::arch::amd64::amd64::pmap::TLB_SHOOT_ADDR1,
    TLB_SHOOT_ADDR2 = sym crate::arch::amd64::amd64::pmap::TLB_SHOOT_ADDR2,
);
#[cfg(not(feature = "multiprocessor"))]
vector_asm!(
    MULTIPROCESSOR = const 0,
    TLB_SHOOT_LOCK = const 0,
    TLB_SHOOT_CPU = const 0,
    TLB_SHOOT_COUNTS = const 0,
    TLB_SHOOT_ADDR1 = const 0,
    TLB_SHOOT_ADDR2 = const 0,
);

unsafe extern "C" {
    /// `Xexceptions[]`: the entry points of the 32 CPU exceptions, for the IDT.
    pub static Xexceptions: [unsafe extern "C" fn(); 32];
    /// `i8259_stubs[]`: the entry, recurse and resume points of the sixteen legacy IRQs.
    pub static i8259_stubs: [Intrstub; NUM_LEGACY_IRQS];
    /// `ioapic_edge_stubs[]`: the entry, recurse and resume points of the 64 edge-triggered
    /// I/O APIC sources (`NIOAPIC > 0`).
    pub static ioapic_edge_stubs: [Intrstub; MAX_INTR_SOURCES];
    /// `ioapic_level_stubs[]`: the same for the level-triggered sources.
    pub static ioapic_level_stubs: [Intrstub; MAX_INTR_SOURCES];
    /// `Xintrspurious`: the spurious interrupt stub (an `iretq`), the LAPIC's spurious vector.
    pub fn Xintrspurious();
    /// `Xsoftclock`: the soft clock interrupt stub (an `is_recurse`/`is_resume` entry).
    pub fn Xsoftclock();
    /// `Xsoftnet`: the soft network interrupt stub.
    pub fn Xsoftnet();
    /// `Xsofttty`: the soft tty interrupt stub.
    pub fn Xsofttty();
    /// `Xintr_lapic_ltimer`: the local APIC timer's interrupt entry (its IDT gate).
    pub fn Xintr_lapic_ltimer();
    /// `Xrecurse_lapic_ltimer`: the timer's `is_recurse` entry (from `spllower`).
    pub fn Xrecurse_lapic_ltimer();
    /// `Xresume_lapic_ltimer`: the timer's `is_resume` entry (from `Xdoreti`).
    pub fn Xresume_lapic_ltimer();
}

#[cfg(feature = "multiprocessor")]
unsafe extern "C" {
    /// `Xintr_lapic_ipi`: the IPI vector's entry (`LAPIC_IPI_VECTOR`'s IDT gate).
    pub fn Xintr_lapic_ipi();
    /// `Xrecurse_lapic_ipi`: the IPI source's `is_recurse` entry (from `spllower`).
    pub fn Xrecurse_lapic_ipi();
    /// `Xresume_lapic_ipi`: the IPI source's `is_resume` entry (from `Xdoreti`).
    pub fn Xresume_lapic_ipi();
    /// `Xipi_invltlb`: the "fast" whole-TLB shootdown IPI (`LAPIC_IPI_INVLTLB`).
    pub fn Xipi_invltlb();
    /// `Xipi_invlpg`: the "fast" one-page shootdown IPI (`LAPIC_IPI_INVLPG`).
    pub fn Xipi_invlpg();
    /// `Xipi_invlrange`: the "fast" range shootdown IPI (`LAPIC_IPI_INVLRANGE`).
    pub fn Xipi_invlrange();
}
/* </CODE> */
