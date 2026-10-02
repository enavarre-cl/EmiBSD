//! The assembly glue of `arch/amd64/amd64/locore.S`, pulled in from the `.S` file next to
//! this module (the file keeps OpenBSD's licence blocks and layout; `{NAME}` placeholders are
//! what `assym.h` provides in C).
//!
//! Upstream: sys/arch/amd64/amd64/locore.S @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `lgdt` and `intr_fast_exit`; the kernel entry
//! (`start`, done by the boot protocol), `cpu_switchto`, `sigcode`, the `syscall` entry and
//! exit, `intr_user_exit`, the Meltdown trampolines and the copy routines come with M5 and M6.
//!
//! ## Deviations
//! - AT&T syntax, as the C file, so the two can be diffed; the rest of the kernel's inline
//!   assembly is Intel syntax.

use core::arch::global_asm;
use core::mem::offset_of;

use crate::arch::amd64::include::frame::Trapframe;
use crate::arch::amd64::include::segments::{
    GCODE_SEL, GDATA_SEL, RegionDescriptor, SEL_KPL, gsel,
};

global_asm!(
    include_str!("locore.S"),
    GSEL_KDATA = const gsel(GDATA_SEL, SEL_KPL),
    GSEL_KCODE = const gsel(GCODE_SEL, SEL_KPL),
    TF_RDI = const offset_of!(Trapframe, tf_rdi),
    TF_RSI = const offset_of!(Trapframe, tf_rsi),
    TF_R8 = const offset_of!(Trapframe, tf_r8),
    TF_R9 = const offset_of!(Trapframe, tf_r9),
    TF_R10 = const offset_of!(Trapframe, tf_r10),
    TF_R12 = const offset_of!(Trapframe, tf_r12),
    TF_R13 = const offset_of!(Trapframe, tf_r13),
    TF_R14 = const offset_of!(Trapframe, tf_r14),
    TF_R15 = const offset_of!(Trapframe, tf_r15),
    TF_RBP = const offset_of!(Trapframe, tf_rbp),
    TF_RBX = const offset_of!(Trapframe, tf_rbx),
    TF_RDX = const offset_of!(Trapframe, tf_rdx),
    TF_RCX = const offset_of!(Trapframe, tf_rcx),
    TF_R11 = const offset_of!(Trapframe, tf_r11),
    TF_RAX = const offset_of!(Trapframe, tf_rax),
    TF_RIP = const offset_of!(Trapframe, tf_rip),
    options(att_syntax)
);

unsafe extern "C" {
    /// `lgdt`: loads the GDT and reloads every segment register from it.
    ///
    /// # Safety
    ///
    /// `rdp` must describe a GDT whose `GCODE_SEL`/`GDATA_SEL` entries are valid 64-bit
    /// kernel segments, and the table must outlive its use.
    pub fn lgdt(rdp: *const RegionDescriptor);
}
