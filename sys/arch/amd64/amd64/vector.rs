//! The trap and fault vector routines of `arch/amd64/amd64/vector.S`, pulled in from the
//! `.S` file next to this module (the file keeps OpenBSD's licence blocks and layout; `{NAME}`
//! placeholders are what `assym.h` and the headers provide in C).
//!
//! Upstream: sys/arch/amd64/amd64/vector.S @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports the exception stubs `Xtrap00` to `Xtrap1f`, the
//! `Xexceptions` table and `alltraps`/`alltraps_kern`. The hardware interrupt stubs
//! (`Xintr_*`, `Xrecurse_*`, `Xresume_*`), the IPI and software interrupt stubs and the
//! `#VC` (AMD SEV) and `DDBPROF` paths come with M4-b and later.
//!
//! ## Deviations
//! - NMIs and double faults use `alltraps` on their IST stacks instead of the
//!   `calltrap_specstk` path, which exists to cope with user-mode GS.base and page tables
//!   (M6). The `#GP` resume stubs (`iretq`, `xrstor`, `xsetbv`, `rdmsr_safe`) and the
//!   Meltdown `Xalltraps` page are M6 too.
//! - `alltraps_kern` does not `sti` (no interrupt stubs yet) and skips `SMAP_CLAC` (no CPU
//!   identification yet).
//! - AT&T syntax, as the C file, so the two can be diffed.

use core::arch::global_asm;
use core::mem::offset_of;

use crate::arch::amd64::include::frame::Trapframe;
use crate::arch::amd64::include::segments::SEL_RPL;
use crate::arch::amd64::include::trap::{
    T_ALIGNFLT, T_ARITHTRAP, T_BOUND, T_BPTFLT, T_CP, T_DIVIDE, T_DNA, T_DOUBLEFLT, T_FPOPFLT,
    T_MCA, T_NMI, T_OFLOW, T_PAGEFLT, T_PRIVINFLT, T_PROTFLT, T_RESERVED, T_SEGNPFLT, T_STKFLT,
    T_TRCTRAP, T_TSSFLT, T_VE, T_XMM,
};

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
    options(att_syntax)
);

unsafe extern "C" {
    /// `Xexceptions[]`: the entry points of the 32 CPU exceptions, for the IDT.
    pub static Xexceptions: [unsafe extern "C" fn(); 32];
}
