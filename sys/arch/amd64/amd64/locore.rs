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
//! The assembly glue of `arch/amd64/amd64/locore.S`, pulled in from the `.S` file next to
//! this module (the file keeps OpenBSD's licence blocks and layout; `{NAME}` placeholders are
//! what `assym.h` provides in C).
//!
//! Upstream: sys/arch/amd64/amd64/locore.S @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `lgdt` and `intr_fast_exit`; M5 adds `cpu_switchto` and
//! `proc_trampoline`; M6-a `Xsyscall` with the AST check and the `sysretq` return; M6-b
//! `intr_user_exit` (the `iretq` return) and the user-thread bits of `cpu_switchto`; with
//! `kern_prot.c` (the TCB) the user segment reset of `cpu_switchto` and the FS.base restore
//! of `intr_user_exit` and `Xsyscall` (`CPUPF_USERSEGS`); with `kern_sig.c` the signal
//! trampoline (`sigcode`, `sigcodecall`, `sigcoderet`, `esigcode`, `sigfill`,
//! `sigfillsiz`). The kernel entry (`start`, done by the boot protocol), the Meltdown
//! trampolines (`Xsyscall_meltdown`, the U-K text page), `retpoline_rax` and
//! `savectx`/`setjmp`/`longjmp` come later.
//!
//! ## Deviations
//! - AT&T syntax, as the C file, so the two can be diffed; the rest of the kernel's inline
//!   assembly is Intel syntax.
//! - `sigcode` keeps the C's retpoline (`JMP_RETPOLINE(rax)`, expanded in place) but not its
//!   `CODEPATCH_START`/`CODEPATCH_END` markers: there is no `codepatch`, so the retpoline is
//!   never replaced by a plain `jmp *%rax` on CPUs that do not need it.
//! - `cpu_switchto` saves and restores the stack pointers, resets the user segment registers
//!   when the CPU still holds a user thread's (`CPUPF_USERSEGS`), sets `curproc`/`curpcb`/
//!   `p_cpu`/`p_stat`, reloads `%cr3` when it changes, records `ci_proc_pmap` and, for a
//!   user thread, `ci_kern_rsp`. The FPU/"extended state" save and reset
//!   (`CPUPF_USERXSTATE`), the Meltdown CR3s, the RSB refill and retguard are not here.
//! - `intr_user_exit` checks for ASTs, restores FS.base from the pcb when the CPU does not
//!   have it (`CPUPF_USERSEGS`) and returns through `iretq` on the trampoline stack as the C
//!   does, without the xstate restore, `DIAGNOSTIC`'s SPL check, IBPB, `pku_xonly`, the MDS
//!   clear and the Meltdown page-table switch.
//! - `proc_trampoline` calls `proc_trampoline_run` (Rust) with the function and argument
//!   instead of calling the function itself: Rust `fn` pointers have no C calling
//!   convention; after it returns the thread takes the syscall exit path, as in C.
//! - `Xsyscall` is the kernel-thread-era subset plus the FS.base restore: no Meltdown
//!   page-table switch, no xstate restore (`CPUPF_USERXSTATE`), no IBPB/MDS code patches, no
//!   `pku_xonly`, no RSB refill, and `DIAGNOSTIC`'s "SPL NOT LOWERED" check (a `printf` from
//!   assembly) is not here.

use core::arch::global_asm;
use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr;

use crate::arch::amd64::include::cpu::{CPUPF_USERSEGS, CPUPF_USERXSTATE, CpuInfo};
use crate::arch::amd64::include::frame::{IretqFrame, Trapframe};
use crate::arch::amd64::include::pcb::Pcb;
use crate::arch::amd64::include::proc::MDP_IRET;
use crate::arch::amd64::include::segments::{
    GCODE_SEL, GDATA_SEL, GUCODE_SEL, GUDATA_SEL, RegionDescriptor, SEL_KPL, SEL_UPL, gsel,
};
use crate::arch::amd64::include::specialreg::MSR_FSBASE;
use crate::kern::kern_fork::proc_trampoline_mi;
use crate::sys::proc::{P_SYSTEM, Proc, SONPROC};
use crate::sys::syscall::SYS_sigreturn;

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
    CI_KERN_RSP = const offset_of!(CpuInfo, ci_kern_rsp),
    CI_SCRATCH = const offset_of!(CpuInfo, ci_scratch),
    CI_INTR_RSP = const offset_of!(CpuInfo, ci_intr_rsp),
    CI_PROC_PMAP = const offset_of!(CpuInfo, ci_proc_pmap),
    IRETQ_RIP = const offset_of!(IretqFrame, iretq_rip),
    IRETQ_CS = const offset_of!(IretqFrame, iretq_cs),
    IRETQ_RFLAGS = const offset_of!(IretqFrame, iretq_rflags),
    IRETQ_RSP = const offset_of!(IretqFrame, iretq_rsp),
    IRETQ_SS = const offset_of!(IretqFrame, iretq_ss),
    P_FLAG = const offset_of!(Proc, p_flag),
    P_SYSTEM = const P_SYSTEM,
    PCB_PMAP = const offset_of!(Pcb, pcb_pmap),
    PCB_KSTACK = const offset_of!(Pcb, pcb_kstack),
    FRAMESIZE = const size_of::<Trapframe>(),
    TF_RSP = const offset_of!(Trapframe, tf_rsp),
    TF_SS = const offset_of!(Trapframe, tf_ss),
    TF_CS = const offset_of!(Trapframe, tf_cs),
    TF_RFLAGS = const offset_of!(Trapframe, tf_rflags),
    TF_ERR = const offset_of!(Trapframe, tf_err),
    GSEL_UDATA = const gsel(GUDATA_SEL, SEL_UPL),
    GSEL_UCODE = const gsel(GUCODE_SEL, SEL_UPL),
    P_MD_REGS = const offset_of!(Proc, p_md.md_regs),
    P_MD_FLAGS = const offset_of!(Proc, p_md.md_flags),
    P_MD_ASTPENDING = const offset_of!(Proc, p_md.md_astpending),
    MDP_IRET = const MDP_IRET,
    CI_PFLAGS = const offset_of!(CpuInfo, ci_pflags),
    CPUPF_USERSEGS = const CPUPF_USERSEGS,
    CPUPF_USERXSTATE = const CPUPF_USERXSTATE,
    PCB_SAVEFPU = const offset_of!(Pcb, pcb_savefpu),
    PROC0_UAREA = sym crate::arch::amd64::amd64::machdep::PROC0_UAREA,
    PROC0_SAVEFPU_OFF = const offset_of!(crate::sys::user::Uarea, u.u_pcb.pcb_savefpu),
    PCB_FSBASE = const offset_of!(Pcb, pcb_fsbase),
    MSR_FSBASE = const MSR_FSBASE,
    SYS_SIGRETURN = const SYS_sigreturn,
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
    /// `Xsyscall`: the `syscall` instruction's entry (`MSR_LSTAR`).
    pub fn Xsyscall();
    /// `sigcode[]`: the signal trampoline, copied into every process (`exec_sigcode_map`).
    static sigcode: [u8; 0];
    /// `sigcodecall[]`: the trampoline's `syscall` instruction.
    static sigcodecall: [u8; 0];
    /// `sigcoderet[]`: the instruction after the trampoline's `sigreturn` system call.
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
    // SAFETY: `sigcode` and `esigcode` bracket the trampoline in `.rodata` (`locore.S`),
    // read-only and alive for the kernel's lifetime.
    unsafe { core::slice::from_raw_parts(start, end as usize - start as usize) }
}

/// `sigcoderet - sigcode`.
pub fn sigcoderet_offset() -> usize {
    ptr::addr_of!(sigcoderet) as usize - ptr::addr_of!(sigcode) as usize
}

/// `sigcodecall - sigcode`: the `syscall` instruction of the trampoline.
pub fn sigcodecall_offset() -> usize {
    ptr::addr_of!(sigcodecall) as usize - ptr::addr_of!(sigcode) as usize
}

/// `sigfill` .. `sigfill + sigfillsiz`.
pub fn sigfill_bytes() -> &'static [u8] {
    // SAFETY: `sigfillsiz` is a constant word in `.rodata`, written by the assembler.
    let len = unsafe { ptr::addr_of!(sigfillsiz).read() } as usize;
    // SAFETY: `sigfill` is followed by `sigfillsiz` bytes of instructions in `.rodata`.
    unsafe { core::slice::from_raw_parts(ptr::addr_of!(sigfill).cast::<u8>(), len) }
}

/// What `proc_trampoline` calls with the switch frame's `sf_r12`/`sf_r13`: the
/// machine-independent start of a thread, then its function. A kernel thread's function
/// never returns; a user thread's (`child_return`) does, and the assembly then takes the
/// syscall exit path to user mode.
///
/// # Safety
///
/// Only `proc_trampoline` calls this, on a thread `cpu_fork` built: `func` is the
/// `fn(*mut c_void)` it stored in the switch frame, as a pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn proc_trampoline_run(func: *const (), arg: *mut c_void) {
    proc_trampoline_mi();
    // SAFETY: the caller's guarantee: `cpu_fork` stored a `fn(*mut c_void)` in `sf_r12` as
    // a pointer; this is the inverse cast.
    let func: fn(*mut c_void) = unsafe { core::mem::transmute::<*const (), fn(*mut c_void)>(func) };
    func(arg);
}
/* </CODE> */
