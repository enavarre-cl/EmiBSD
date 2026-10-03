//! The assembly glue of `arch/amd64/amd64/locore.S`, pulled in from the `.S` file next to
//! this module (the file keeps OpenBSD's licence blocks and layout; `{NAME}` placeholders are
//! what `assym.h` provides in C).
//!
//! Upstream: sys/arch/amd64/amd64/locore.S @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `lgdt` and `intr_fast_exit`; `intr_user_exit` is a
//! stub that reports itself. M5 adds `cpu_switchto` and `proc_trampoline`. The kernel entry
//! (`start`, done by the boot protocol), `sigcode`, the `syscall` entry and exit, the real
//! `intr_user_exit`, the Meltdown trampolines and the copy routines come with M6.
//!
//! ## Deviations
//! - AT&T syntax, as the C file, so the two can be diffed; the rest of the kernel's inline
//!   assembly is Intel syntax.
//! - `cpu_switchto` is the kernel-thread subset: it saves and restores the stack pointers,
//!   sets `curproc`/`curpcb`/`p_cpu`/`p_stat` and reloads `%cr3` when it changes. The
//!   FPU/"extended state" save and reset (`CPUPF_USERXSTATE`), the user segment reset
//!   (`CPUPF_USERSEGS`), `ci_proc_pmap`/`ci_kern_rsp`/the Meltdown CR3s, the RSB refill and
//!   retguard come with user mode (M6).
//! - `proc_trampoline` calls `proc_trampoline_run` (Rust) with the function and argument
//!   instead of calling the function itself: Rust `fn` pointers have no C calling
//!   convention. After the function returns the C goes to the syscall return path; here it
//!   is a panic until M6.

use core::arch::global_asm;
use core::ffi::c_void;
use core::mem::offset_of;

use crate::arch::amd64::include::cpu::CpuInfo;
use crate::arch::amd64::include::frame::Trapframe;
use crate::arch::amd64::include::pcb::Pcb;
use crate::arch::amd64::include::segments::{
    GCODE_SEL, GDATA_SEL, RegionDescriptor, SEL_KPL, gsel,
};
use crate::kern::kern_fork::proc_trampoline_mi;
use crate::kern::subr_prf::panic;
use crate::sys::proc::{Proc, SONPROC};

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
    SONPROC = const SONPROC,
    P_STAT = const offset_of!(Proc, p_stat),
    P_CPU = const offset_of!(Proc, p_cpu),
    P_ADDR = const offset_of!(Proc, p_addr),
    PCB_RSP = const offset_of!(Pcb, pcb_rsp),
    PCB_RBP = const offset_of!(Pcb, pcb_rbp),
    PCB_CR3 = const offset_of!(Pcb, pcb_cr3),
    CI_SELF = const offset_of!(CpuInfo, ci_self),
    CI_CURPROC = const offset_of!(CpuInfo, ci_curproc),
    CI_CURPCB = const offset_of!(CpuInfo, ci_curpcb),
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
    /// `cpu_switchto(old, new)`: the context switch (`machine::cpu::Cpu::cpu_switchto`
    /// states the contract). Both are `struct proc *` (opaque to the ABI: `Proc` has Rust
    /// layout, the assembly reads it by `offset_of!`).
    pub fn cpu_switchto(old: *const c_void, new: *const c_void);
    /// `proc_trampoline`: the first instructions of a thread built by `cpu_fork`.
    pub fn proc_trampoline();
}

/// What `proc_trampoline` calls with the switch frame's `sf_r12`/`sf_r13`: the
/// machine-independent start of a thread, then its function. The function never returns for
/// a kernel thread; a user thread's return to user mode (`syscall_return`) is M6.
///
/// # Safety
///
/// Only `proc_trampoline` calls this, on a thread `cpu_fork` built: `func` is the
/// `fn(*mut c_void)` it stored in the switch frame, as a pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn proc_trampoline_run(func: *const (), arg: *mut c_void) -> ! {
    proc_trampoline_mi();
    // SAFETY: the caller's guarantee: `cpu_fork` stored a `fn(*mut c_void)` in `sf_r12` as a pointer; this is the
    // inverse cast.
    let func: fn(*mut c_void) = unsafe { core::mem::transmute::<*const (), fn(*mut c_void)>(func) };
    func(arg);
    panic(format_args!(
        "proc_trampoline: the thread function returned (the user-mode return is M6)"
    ))
}

/// What `intr_user_exit` does until user mode exists: a return to user mode is a bug.
#[unsafe(no_mangle)]
pub extern "C" fn intr_user_exit_unported(frame: &Trapframe) -> ! {
    crate::kern::subr_prf::panic(format_args!(
        "intr_user_exit: return to user mode without user mode (M6): cs {:x} rip {:x}",
        frame.tf_cs, frame.tf_rip
    ))
}
