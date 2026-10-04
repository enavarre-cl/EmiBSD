/*	$OpenBSD: trap.c,v 1.119 2026/08/19 08:56:28 hshoexer Exp $	*/
/*	$NetBSD: trap.c,v 1.2 2003/05/04 23:51:56 fvdl Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1998, 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum.
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

/*-
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * the University of Utah, and William Jolitz.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)trap.c	7.4 (Berkeley) 5/13/91
 */
/* </LICENSES> */

//! amd64 trap handling: `arch/amd64/amd64/trap.c`.
//!
//! Upstream: sys/arch/amd64/amd64/trap.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `trap_type[]`, `fault`, `pgex2access`, the kernel page
//! fault entry (`kpageflttrap`), `kerntrap` and `trap_print`; M6-a `syscall`, `ast` and
//! `child_return`, and `kpageflttrap`'s `pcb_onfault` handling. `upageflttrap`, `usertrap`,
//! `frame_dump`, `verify_pkru` and the `#VC` handler come with user mode (M6-b);
//! `verify_smap` with CPU identification (M4-b); `debug_trap` is the `DEBUG` option's
//! `trapdebug` print. M11a: `child_return`'s `KERNEL_UNLOCK()` (the lock
//! `proc_trampoline_mi` takes for a new thread with `MULTIPROCESSOR`) and `ast`'s
//! `ci_want_resched`, an atomic other CPUs set; the system call's kernel lock is
//! `mi_syscall`'s.
//!
//! ## Deviations
//! - `kpageflttrap`: `p->p_vmspace` does not exist before M6-b, so a fault outside the
//!   kernel map with no `pcb_onfault` is fatal, and `uvm_fault` on the kernel map is
//!   reported: a kernel page fault is handled only when `pcb_onfault` catches it (the
//!   `copyin` family).
//! - `syscall` skips `verify_smap` (M4-b) and `verify_pkru` (PKU, M6-b).
//! - `usertrap` reports `fputrap` (the FPU, `fpu.c`) and posts `SIGFPE` with code 0 for the
//!   x87/SSE exceptions; the other user traps go to `kern_sig.c`'s `trapsignal` as in C.
//! - `MULTIPROCESSOR` (M11a): `kpageflttrap` and `upageflttrap` hold the kernel lock over
//!   `uvm_fault` and `uvm_grow`, which the C runs unlocked: uvm, the page allocator
//!   (`uvm_lock_fpageq` is still a no-op) and the pools are not audited for MP yet (M11a
//!   phase 2, M11e). Without `MULTIPROCESSOR` the lock is nothing.
//! - `fault` writes `curcpu()->ci_panicbuf` as the C does; `panic()` itself still uses
//!   `subr_prf`'s buffer (`kern/subr_prf.rs`, deviations).

use core::ffi::c_void;
use core::fmt;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::arch::amd64::amd64::db_interface::db_ktrap;
use crate::arch::amd64::amd64::intr::x86_nmi;
use crate::arch::amd64::include::cpu::curcpu;
use crate::arch::amd64::include::cpufunc::{rcr2, rdmsr, rdr6, rdr7};
use crate::arch::amd64::include::frame::Trapframe;
use crate::arch::amd64::include::psl::PSL_C;
use crate::arch::amd64::include::pte::{PGEX_I, PGEX_P, PGEX_W};
use crate::arch::amd64::include::segments::kernelmode;
use crate::arch::amd64::include::specialreg::{MSR_GSBASE, MSR_KERNELGSBASE};
use crate::arch::amd64::include::trap::{
    T_ALIGNFLT, T_ARITHTRAP, T_BPTFLT, T_CP, T_DIVIDE, T_NMI, T_PAGEFLT, T_PRIVINFLT, T_PROTFLT,
    T_SEGNPFLT, T_STKFLT, T_TRCTRAP, T_TSSFLT, T_XMM,
};
use crate::arch::amd64::include::vmparam::{VM_MAXUSER_ADDRESS, VM_MIN_KERNEL_ADDRESS};
use crate::kassert;
use crate::kern::init_sysent::SYSENT;
use crate::kern::kern_sig::{trapsignal, userret};
use crate::kern::subr_prf::{Str, db_printf, panic, panicstr_claim, printf, vsnprintf};
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::sys::errno::Errno;
use crate::sys::mman::{PROT_EXEC, PROT_READ, PROT_WRITE};
use crate::sys::proc::{Proc, refreshcreds};
use crate::sys::siginfo::{
    BUS_ADRALN, BUS_OBJERR, FPE_INTDIV, ILL_BADSTK, ILL_BTCFI, ILL_PRVOPC, SEGV_ACCERR,
    SEGV_MAPERR, Sigval, TRAP_BRKPT,
};
use crate::sys::signal::{SIGBUS, SIGFPE, SIGILL, SIGKILL, SIGSEGV, SIGTRAP};
use crate::sys::syscall::SYS_MAXSYSCALL;
use crate::sys::syscall_mi::{mi_ast, mi_child_return, mi_syscall, mi_syscall_return};
use crate::sys::systm::{SysArgs, kernel_lock, kernel_unlock};
use crate::sys::types::Register;
use crate::uvm::uvm_extern::VmProt;
use crate::uvm::uvm_fault::uvm_fault;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_map::{uvm_map_inentry, uvm_map_inentry_sp};
use crate::uvm::uvm_unix::uvm_grow;

use crate::uvm::uvm_param::trunc_page;

/// `trap_type[]`: the name of each `T_*` trap.
pub static TRAP_TYPE: [&str; 23] = [
    "privileged instruction fault", /*  0 T_PRIVINFLT */
    "breakpoint trap",              /*  1 T_BPTFLT */
    "arithmetic trap",              /*  2 T_ARITHTRAP */
    "reserved trap",                /*  3 T_RESERVED */
    "protection fault",             /*  4 T_PROTFLT */
    "trace trap",                   /*  5 T_TRCTRAP */
    "page fault",                   /*  6 T_PAGEFLT */
    "alignment fault",              /*  7 T_ALIGNFLT */
    "integer divide fault",         /*  8 T_DIVIDE */
    "non-maskable interrupt",       /*  9 T_NMI */
    "overflow trap",                /* 10 T_OFLOW */
    "bounds check fault",           /* 11 T_BOUND */
    "FPU not available fault",      /* 12 T_DNA */
    "double fault",                 /* 13 T_DOUBLEFLT */
    "FPU operand fetch fault",      /* 14 T_FPOPFLT */
    "invalid TSS fault",            /* 15 T_TSSFLT */
    "segment not present fault",    /* 16 T_SEGNPFLT */
    "stack fault",                  /* 17 T_STKFLT */
    "machine check",                /* 18 T_MCA */
    "SSE FP exception",             /* 19 T_XMM */
    "virtualization exception",     /* 20 T_VE */
    "control protection exception", /* 21 T_CP */
    "VMM communication exception",  /* 29 T_VC */
];
/// `trap_types`: how many names `trap_type[]` has.
pub const TRAP_TYPES: i32 = TRAP_TYPE.len() as i32;

/// `fault`: claims `panicstr` for this CPU's `ci_panicbuf`, formats the fatal fault's message
/// into it and prints it; the `panic()` that follows is then the second one on this CPU.
fn fault(args: fmt::Arguments<'_>) {
    let ci = curcpu();
    panicstr_claim(ci.ci_panicbuf.get().cast::<u8>());

    // SAFETY: this CPU's buffer, written only by this CPU, with no other reference alive.
    let buf = unsafe { &mut *ci.ci_panicbuf.get() };
    vsnprintf(buf, args);
    db_printf(format_args!("{}\n", Str(buf)));
}

/// `pgex2access`: the access a page fault's error code describes.
pub fn pgex2access(pgex: u64) -> VmProt {
    if pgex & PGEX_W != 0 {
        PROT_WRITE
    } else if pgex & PGEX_I != 0 {
        PROT_EXEC
    } else {
        PROT_READ
    }
}

/// `kpageflttrap(frame, cr2)`: page fault handler. Returns `true` if the fault was handled
/// (possibly by generating a signal). Returns `false` if something was so broken that we
/// should panic.
pub fn kpageflttrap(frame: &mut Trapframe, cr2: u64) -> bool {
    let va = trunc_page(cr2 as usize);
    let access_type = pgex2access(frame.tf_err as u64);

    // SAFETY: `ci_curproc` names a thread on the CPU, hence alive.
    let Some(p) = (unsafe { curcpu().ci_curproc.get().as_ref() }) else {
        return false;
    };
    if p.p_addr.get().is_null() {
        return false;
    }
    // p->p_vmspace == NULL: no user address spaces before M6-b; see below.

    let pcb = p.pcb();
    let pcb_onfault = pcb.pcb_onfault.get();
    if pcb_onfault != 0 && !nofault_table_has(pcb_onfault) {
        fault(format_args!("invalid pcb_nofault={pcb_onfault:#x}"));
        return false;
    }

    // This will only trigger if SMEP is enabled
    if pcb_onfault == 0 && cr2 <= VM_MAXUSER_ADDRESS as u64 && frame.tf_err as u64 & PGEX_I != 0 {
        fault(format_args!(
            "attempt to execute user address {cr2:#x} in supervisor mode"
        ));
        return false;
    }
    // This will only trigger if SMAP is enabled
    if pcb_onfault == 0 && cr2 <= VM_MAXUSER_ADDRESS as u64 && frame.tf_err as u64 & PGEX_P != 0 {
        fault(format_args!(
            "attempt to access user address {cr2:#x} in supervisor mode"
        ));
        return false;
    }

    // It is only a kernel address space fault iff:
    //	1. when running in ring 0 and
    //	2. pcb_onfault not set or
    //	3. pcb_onfault set but supervisor space fault
    // The last can occur during an exec() copyin where the argument space is lazy-allocated.
    // map = &p->p_vmspace->vm_map, or kernel_map:
    let kernel_map = va >= VM_MIN_KERNEL_ADDRESS;

    let error = if curcpu().ci_inatomic.get() == 0 || kernel_map {
        let map = if kernel_map {
            crate::uvm::uvm_km::kernel_map()
        } else {
            &p.vmspace().vm_map
        };
        let onfault = pcb.pcb_onfault.get();
        pcb.pcb_onfault.set(0);
        // M11a: the kernel lock around the fault (see the module's deviations).
        kernel_lock();
        let error = uvm_fault(map, va, 0, access_type);
        pcb.pcb_onfault.set(onfault);
        if error.is_ok() && !kernel_map {
            uvm_grow(p, va);
        }
        kernel_unlock();
        error.err()
    } else {
        Some(Errno::EFAULT)
    };

    match error {
        None => true,
        Some(error) if pcb_onfault == 0 => {
            // bad memory access in the kernel
            fault(format_args!(
                "uvm_fault({}, {:#x}, 0, {}) -> {:x}",
                if kernel_map { "kernel_map" } else { "vm_map" },
                cr2,
                access_type,
                error as i32
            ));
            false
        }
        Some(_) => {
            frame.tf_rip = pcb_onfault as i64;
            true
        }
    }
}

unsafe extern "C" {
    /// `__nofault_start[]`: the `.nofault` table of fault handlers (`DECLARE_ONFAULT` in
    /// `copy.S`), from the linker script.
    static __nofault_start: [usize; 0];
    /// `__nofault_end[]`.
    static __nofault_end: [usize; 0];
}

/// Whether `onfault` is one of the handlers `copy.S` declared in `.nofault`.
fn nofault_table_has(onfault: usize) -> bool {
    let mut nf = ptr::addr_of!(__nofault_start).cast::<usize>();
    let end = ptr::addr_of!(__nofault_end).cast::<usize>();
    while nf < end {
        // SAFETY: the linker script lays the `.nofault` words out between the two symbols.
        if unsafe { nf.read() } == onfault {
            return true;
        }
        nf = nf.wrapping_add(1);
    }
    false
}

/// `ast(frame)`: AST handler. This is called from assembly language stubs when returning to
/// userspace after a syscall or interrupt.
#[unsafe(no_mangle)]
pub extern "C" fn ast(frame: &mut Trapframe) {
    let Some(p) = current() else {
        panic(format_args!("ast: no curproc"));
    };

    UVMEXP.traps.fetch_add(1, Ordering::Relaxed);
    kassert!(!kernelmode(frame.tf_cs as u64));
    p.p_md.md_regs.set(frame);
    refreshcreds(p);
    UVMEXP.softs.fetch_add(1, Ordering::Relaxed);
    mi_ast(p, curcpu().ci_want_resched.load(Ordering::Relaxed) != 0);
    userret(p);
}

/// `syscall(frame)`: system call request from POSIX system call gate interface to kernel.
#[unsafe(no_mangle)]
pub extern "C" fn syscall(frame: &mut Trapframe) {
    // verify_smap(__func__): CPU identification (M4-b).
    UVMEXP.syscalls.fetch_add(1, Ordering::Relaxed);
    let Some(p) = current() else {
        panic(format_args!("syscall: no curproc"));
    };

    // verify_pkru(p): PKU (M6-b).

    let code = frame.tf_rax as Register;
    // The arguments are the first six registers of the frame, in the C ABI's order:
    // tf_rdi, tf_rsi, tf_rdx, tf_r10, tf_r8, tf_r9 (contiguous, checked below).
    // SAFETY: the six fields are consecutive `i64`s at the start of the frame.
    let args: &SysArgs = unsafe { &*ptr::addr_of!(frame.tf_rdi).cast::<SysArgs>() };

    let mut rval: [Register; 2] = [0, 0];

    let error = if code <= 0 || code as usize >= SYS_MAXSYSCALL {
        Err(Errno::ENOSYS)
    } else {
        let callp = &SYSENT[code as usize];
        mi_syscall(p, code, callp, args, &mut rval)
    };

    match error {
        Ok(()) => {
            frame.tf_rax = rval[0] as i64;
            frame.tf_rflags &= !(PSL_C as i64); // carry bit
        }
        Err(Errno::ERESTART) => {
            // Back up over the syscall instruction (2 bytes)
            frame.tf_rip -= 2;
        }
        Err(Errno::EJUSTRETURN) => {
            // nothing to do
        }
        Err(e) => {
            frame.tf_rax = i64::from(e as i32);
            frame.tf_rflags |= PSL_C as i64; // carry bit
        }
    }

    mi_syscall_return(p, code, error, &rval);
}

/// `child_return`: the first thing a forked user thread runs: a `fork` return of 0 in the
/// child, then the user-mode return.
pub fn child_return(arg: *mut c_void) {
    // SAFETY: `fork1` passes the new thread itself as the argument.
    let p = unsafe { &*arg.cast::<Proc>() };
    let tf = p.p_md.md_regs.get();

    // SAFETY: `cpu_fork` set `md_regs` to the trap frame at the top of the thread's u-area.
    unsafe {
        (*tf).tf_rax = 0;
        (*tf).tf_rflags &= !(PSL_C as i64);
    }

    // The kernel lock proc_trampoline_mi took for the new thread (MULTIPROCESSOR).
    kernel_unlock();

    mi_child_return(p);
}

/// `uvm_map_inentry(p, &p->p_spinentry, PROC_STACK(p), ..., uvm_map_inentry_sp, sserial)`:
/// the MAP_STACK check `usertrap` makes before a user page fault.
fn user_stack_ok(p: &Proc) -> bool {
    let mut ie = p.p_spinentry.get();
    let ok = uvm_map_inentry(
        p,
        &mut ie,
        <Machine as Cpu>::proc_stack(p),
        "sp",
        "not MAP_STACK",
        uvm_map_inentry_sp,
        p.vmspace().vm_map.sserial.get(),
    );
    p.p_spinentry.set(ie);
    ok
}

/// `upageflttrap(frame, cr2)`: page fault handler. Returns `true` if the fault was handled
/// (possibly by generating a signal). Returns `false`, possibly still holding the kernel
/// lock, if something was so broken that we should panic.
pub fn upageflttrap(frame: &mut Trapframe, cr2: u64) -> bool {
    let Some(p) = current() else {
        return false;
    };
    let va = trunc_page(cr2 as usize);
    let access_type = pgex2access(frame.tf_err as u64);

    // We used to set PROT_EXEC when CPU NX bit was not set, but the pmap now treats them
    // as read-only and PROT_EXEC access, so try both.
    let map = &p.vmspace().vm_map;
    // M11a: the kernel lock around the fault (see the module's deviations).
    kernel_lock();
    let mut result = uvm_fault(map, va, 0, access_type);
    if crate::arch::amd64::amd64::pmap::PG_NX_BIT.load(core::sync::atomic::Ordering::Relaxed) == 0
        && result == Err(Errno::EACCES)
        && access_type == PROT_READ
    {
        result = uvm_fault(map, va, 0, PROT_EXEC);
    }
    if result.is_ok() {
        uvm_grow(p, va);
    }
    kernel_unlock();
    let error = match result {
        Ok(()) => return true,
        Err(e) => e,
    };

    let (signal, sicode) = if error == Errno::ENOMEM {
        printf(format_args!(
            "UVM: pid {} ({}), uid {} killed: out of swap\n",
            p.process().ps_pid.get(),
            Str(p.process().comm()),
            -1
        ));
        (SIGKILL, 0)
    } else if error == Errno::EACCES {
        (SIGSEGV, SEGV_ACCERR)
    } else if error == Errno::EIO {
        (SIGBUS, BUS_OBJERR)
    } else {
        (SIGSEGV, SEGV_MAPERR)
    };
    let sv = Sigval::from_ptr(cr2 as usize);
    trapsignal(p, signal, T_PAGEFLT as u64, sicode, sv);
    true
}

/// `usertrap(frame)`: handler for exceptions, faults, and traps from user mode. This is
/// called from the assembly language IDT gate entries which prepare a suitable stack frame
/// and restores the CPU state after the fault has been processed.
#[unsafe(no_mangle)]
pub extern "C" fn usertrap(frame: &mut Trapframe) {
    let Some(p) = current() else {
        panic(format_args!("usertrap: no curproc"));
    };
    let type_ = frame.tf_trapno as i32;
    let cr2 = rcr2();

    // verify_smap(__func__): SMAP arrives with CPU identification (M4-b).
    UVMEXP.traps.fetch_add(1, Ordering::Relaxed);
    // debug_trap(frame, p, type): the DEBUG option's trapdebug print.

    p.p_md.md_regs.set(frame);
    refreshcreds(p);

    // verify_pkru(p): PKU (M7).

    let (sig, code) = match type_ {
        T_TSSFLT => (SIGBUS, BUS_OBJERR),
        // protection fault
        T_PROTFLT | T_SEGNPFLT | T_STKFLT => {
            frame_dump(frame, p, "SEGV", 0);
            (SIGSEGV, SEGV_MAPERR)
        }
        T_ALIGNFLT => (SIGBUS, BUS_ADRALN),
        // privileged instruction fault
        T_PRIVINFLT => (SIGILL, ILL_PRVOPC),
        T_DIVIDE => (SIGFPE, FPE_INTDIV),
        // real arithmetic exceptions
        T_ARITHTRAP | T_XMM => (SIGFPE, crate::arch::amd64::amd64::fpu::fputrap(type_)),
        // bpt instruction fault, trace trap
        T_BPTFLT | T_TRCTRAP => (SIGTRAP, TRAP_BRKPT),
        T_CP => (
            SIGILL,
            if frame.tf_err & 0x7fff < 4 {
                ILL_BTCFI
            } else {
                ILL_BADSTK
            },
        ),
        // AMDSEV's T_VC: not configured.
        T_PAGEFLT if !user_stack_ok(p) => {
            userret(p);
            return;
        }
        T_PAGEFLT if upageflttrap(frame, cr2) => {
            userret(p);
            return;
        }
        _ => {
            trap_print(frame, type_);
            panic(format_args!("impossible trap"));
        }
    };

    let sv = Sigval::from_ptr(frame.tf_rip as usize);
    trapsignal(p, sig, type_ as u64, code, sv);

    // out:
    userret(p);
}

/// `frame_dump`: the `TRAP_SIGDEBUG` dump of a faulting user frame (always on here).
fn frame_dump(tf: &Trapframe, p: &Proc, sig: &str, cr2: u64) {
    printf(format_args!(
        "pid {} ({}): {} at rip {:x} addr {:x}\n",
        p.process().ps_pid.get(),
        Str(p.process().comm()),
        sig,
        tf.tf_rip,
        cr2
    ));
    printf(format_args!(
        "rip {:#x}  cs {:#x}  rfl {:#x}  rsp {:#x}  ss {:#x}\n",
        tf.tf_rip,
        tf.tf_cs & 0xffff,
        tf.tf_rflags,
        tf.tf_rsp,
        tf.tf_ss & 0xffff
    ));
    printf(format_args!(
        "err {:#x}  trapno {:#x}\n",
        tf.tf_err, tf.tf_trapno
    ));
    printf(format_args!(
        "rdi {:#x}  rsi {:#x}  rdx {:#x}\n",
        tf.tf_rdi, tf.tf_rsi, tf.tf_rdx
    ));
    printf(format_args!(
        "rcx {:#x}  r8  {:#x}  r9  {:#x}\n",
        tf.tf_rcx, tf.tf_r8, tf.tf_r9
    ));
    printf(format_args!(
        "r10 {:#x}  r11 {:#x}  r12 {:#x}\n",
        tf.tf_r10, tf.tf_r11, tf.tf_r12
    ));
    printf(format_args!(
        "r13 {:#x}  r14 {:#x}  r15 {:#x}\n",
        tf.tf_r13, tf.tf_r14, tf.tf_r15
    ));
    printf(format_args!(
        "rbp {:#x}  rbx {:#x}  rax {:#x}\n",
        tf.tf_rbp, tf.tf_rbx, tf.tf_rax
    ));
}

/// `curproc`, as a reference.
fn current() -> Option<&'static Proc> {
    // SAFETY: `ci_curproc` names a thread on the CPU, hence alive.
    unsafe { curcpu().ci_curproc.get().as_ref() }
}

/// `kerntrap(frame)`: handler for exceptions, faults, and traps from supervisor mode. This is
/// called from the assembly language IDT gate entries (`vector.S`), which prepare a suitable
/// stack frame and restore the CPU state after the fault has been processed.
#[unsafe(no_mangle)]
pub extern "C" fn kerntrap(frame: &mut Trapframe) {
    let type_ = frame.tf_trapno as i32;
    let cr2 = rcr2();

    // verify_smap(__func__): SMAP arrives with CPU identification (M4-b).
    UVMEXP.traps.fetch_add(1, Ordering::Relaxed);
    // debug_trap(frame, curproc, type): the DEBUG option's trapdebug print.

    let handled = match type_ {
        // allow page faults in kernel mode
        T_PAGEFLT => kpageflttrap(frame, cr2),
        // NISA > 0: NMI can be hooked up to a pushbutton for debugging
        T_NMI => {
            printf(format_args!("NMI ... going to debugger\n"));
            if db_ktrap(type_, 0, frame) {
                return;
            }
            // machine/parity/power fail/"kitchen sink" faults
            !x86_nmi()
        }
        // T_VC (AMDSEV): not configured.
        _ => false,
    };
    if handled {
        return;
    }

    // we_re_toast:
    if db_ktrap(type_, frame.tf_err as i32, frame) {
        return;
    }
    trap_print(frame, type_);
    panic(format_args!(
        "trap type {}, code={:x}, pc={:x}",
        type_, frame.tf_err, frame.tf_rip
    ));
}

/// `trap_print`: the fatal trap's description, before the panic.
fn trap_print(frame: &Trapframe, type_: i32) {
    match usize::try_from(type_).ok().and_then(|t| TRAP_TYPE.get(t)) {
        Some(name) => printf(format_args!("fatal {name}")),
        None => printf(format_args!("unknown trap {type_}")),
    };
    printf(format_args!(
        " in {} mode\n",
        if kernelmode(frame.tf_cs as u64) {
            "supervisor"
        } else {
            "user"
        }
    ));
    printf(format_args!(
        "trap type {} code {:x} rip {:x} cs {:x} rflags {:x} cr2 {:x} cpl {:x} rsp {:x}\n",
        type_,
        frame.tf_err,
        frame.tf_rip,
        frame.tf_cs,
        frame.tf_rflags,
        rcr2(),
        curcpu().ci_ilevel.get(),
        frame.tf_rsp
    ));
    // SAFETY: both MSRs exist on every x86-64 CPU and reading them has no side effect.
    let (gsbase, kgsbase) = unsafe { (rdmsr(MSR_GSBASE), rdmsr(MSR_KERNELGSBASE)) };
    printf(format_args!("gsbase {gsbase:#x}  kgsbase {kgsbase:#x}\n"));
    if type_ == T_TRCTRAP {
        printf(format_args!("dr6 {:x} dr7 {:x}\n", rdr6(), rdr7()));
    }
}

const _: () = {
    assert!(
        core::mem::offset_of!(Trapframe, tf_rsi) == core::mem::offset_of!(Trapframe, tf_rdi) + 8
    );
    assert!(
        core::mem::offset_of!(Trapframe, tf_rdx) == core::mem::offset_of!(Trapframe, tf_rdi) + 16
    );
    assert!(
        core::mem::offset_of!(Trapframe, tf_r10) == core::mem::offset_of!(Trapframe, tf_rdi) + 24
    );
    assert!(
        core::mem::offset_of!(Trapframe, tf_r8) == core::mem::offset_of!(Trapframe, tf_rdi) + 32
    );
    assert!(
        core::mem::offset_of!(Trapframe, tf_r9) == core::mem::offset_of!(Trapframe, tf_rdi) + 40
    );
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trap_names_follow_the_numbers() {
        assert_eq!(TRAP_TYPE[T_PAGEFLT as usize], "page fault");
        assert_eq!(TRAP_TYPE[T_NMI as usize], "non-maskable interrupt");
        assert_eq!(TRAP_TYPES, 23);
    }

    #[test]
    fn error_code_to_access() {
        assert_eq!(pgex2access(0), PROT_READ);
        assert_eq!(pgex2access(PGEX_W), PROT_WRITE);
        assert_eq!(pgex2access(PGEX_I), PROT_EXEC);
        assert_eq!(pgex2access(PGEX_W | PGEX_I), PROT_WRITE);
    }
}
