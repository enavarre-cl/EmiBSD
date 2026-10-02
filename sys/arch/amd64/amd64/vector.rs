//! The trap and fault vector routines of `arch/amd64/amd64/vector.S`, pulled in from the
//! `.S` file next to this module (the file keeps OpenBSD's licence blocks and layout; `{NAME}`
//! placeholders are what `assym.h` and the headers provide in C).
//!
//! Upstream: sys/arch/amd64/amd64/vector.S @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports the exception stubs `Xtrap00` to `Xtrap1f`, the
//! `Xexceptions` table, `alltraps`/`alltraps_kern`, the `frameasm.h` entry macros, the
//! `INTRSTUB` generic stub with the sixteen legacy (i8259) instances, `i8259_stubs[]` and
//! the soft interrupt stubs `Xsoftclock`/`Xsoftnet`/`Xsofttty`. The IOAPIC and LAPIC stubs,
//! the IPIs, `x2apic_eoi` and the `#VC` (AMD SEV) and `DDBPROF` paths come with M5 and later.
//!
//! ## Deviations
//! - NMIs and double faults use `alltraps` on their IST stacks instead of the
//!   `calltrap_specstk` path, which exists to cope with user-mode GS.base and page tables
//!   (M6). The `#GP` resume stubs (`iretq`, `xrstor`, `xsetbv`, `rdmsr_safe`) and the
//!   Meltdown `Xalltraps` page are M6 too.
//! - `alltraps_kern` and the interrupt stubs skip `SMAP_CLAC` (no CPU identification yet).
//! - `INTRENTRY`'s path from user space (swapgs, the Meltdown CR3 switch and the kernel
//!   stack) is `ud2` until user mode exists (M6); `intr_user_exit` is `locore.S`'s stub.
//! - `retpoline_r13` (a `CODEPATCH`ed Spectre thunk) is a plain `jmp *%r13`: `codepatch.c`
//!   is not ported. `uvmexp` is reached by its C name (`export_name`) for `V_INTR`.
//! - AT&T syntax, as the C file, so the two can be diffed.

use core::arch::global_asm;
use core::mem::offset_of;

use crate::arch::amd64::include::cpu::CpuInfo;
use crate::arch::amd64::include::frame::{Intrframe, IretqFrame, Trapframe};
use crate::arch::amd64::include::i8259::IRQ_SLAVE;
use crate::arch::amd64::include::intr::{Intrhand, Intrsource, Intrstub};
use crate::arch::amd64::include::intrdefs::{
    IPL_SOFTCLOCK, IPL_SOFTNET, IPL_SOFTTTY, IREENT_MAGIC, NUM_LEGACY_IRQS,
};
use crate::arch::amd64::include::segments::SEL_RPL;
use crate::arch::amd64::include::trap::{
    T_ALIGNFLT, T_ARITHTRAP, T_BOUND, T_BPTFLT, T_CP, T_DIVIDE, T_DNA, T_DOUBLEFLT, T_FPOPFLT,
    T_MCA, T_NMI, T_OFLOW, T_PAGEFLT, T_PRIVINFLT, T_PROTFLT, T_RESERVED, T_SEGNPFLT, T_STKFLT,
    T_TRCTRAP, T_TSSFLT, T_VE, T_XMM,
};
use crate::dev::isa::isareg::{IO_ICU1, IO_ICU2};
use crate::sys::softintr::{SOFTINTR_CLOCK, SOFTINTR_NET, SOFTINTR_TTY};
use crate::uvm::uvmexp::Uvmexp;

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
    IF_PPL = const offset_of!(Intrframe, if_ppl),
    IS_MAXLEVEL = const offset_of!(Intrsource, is_maxlevel),
    IS_HANDLERS = const offset_of!(Intrsource, is_handlers),
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
    options(att_syntax)
);

unsafe extern "C" {
    /// `Xexceptions[]`: the entry points of the 32 CPU exceptions, for the IDT.
    pub static Xexceptions: [unsafe extern "C" fn(); 32];
    /// `i8259_stubs[]`: the entry, recurse and resume points of the sixteen legacy IRQs.
    pub static i8259_stubs: [Intrstub; NUM_LEGACY_IRQS];
    /// `Xintrspurious`: the spurious interrupt stub (an `iretq`), the LAPIC's spurious vector.
    pub fn Xintrspurious();
    /// `Xsoftclock`: the soft clock interrupt stub (an `is_recurse`/`is_resume` entry).
    pub fn Xsoftclock();
    /// `Xsoftnet`: the soft network interrupt stub.
    pub fn Xsoftnet();
    /// `Xsofttty`: the soft tty interrupt stub.
    pub fn Xsofttty();
}
