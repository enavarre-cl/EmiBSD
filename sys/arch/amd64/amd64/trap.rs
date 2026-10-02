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
//! fault entry (`kpageflttrap`), `kerntrap` and `trap_print`. `upageflttrap`, `usertrap`,
//! `ast`, `syscall`, `frame_dump`, `verify_pkru` and the `#VC` handler come with user mode
//! (M6); `verify_smap` with CPU identification (M4-b); `debug_trap` is the `DEBUG` option's
//! `trapdebug` print.
//!
//! ## Deviations
//! - `kpageflttrap` returns at the C's first check: there is no `curproc` before M5, so every
//!   kernel page fault is fatal, exactly as it is in C when `p == NULL`. The code behind that
//!   check is ported with `pcb_onfault` always unset and `uvm_fault` reported (M6), for when
//!   a process exists.
//! - `fault` writes `curcpu()->ci_panicbuf` as the C does; `panic()` itself still uses
//!   `subr_prf`'s buffer (`kern/subr_prf.rs`, deviations).

use core::fmt;
use core::sync::atomic::Ordering;

use crate::arch::amd64::amd64::db_interface::db_ktrap;
use crate::arch::amd64::amd64::intr::x86_nmi;
use crate::arch::amd64::include::cpu::curcpu;
use crate::arch::amd64::include::cpufunc::{rcr2, rdmsr, rdr6, rdr7};
use crate::arch::amd64::include::frame::Trapframe;
use crate::arch::amd64::include::pte::{PGEX_I, PGEX_P, PGEX_W};
use crate::arch::amd64::include::segments::kernelmode;
use crate::arch::amd64::include::specialreg::{MSR_GSBASE, MSR_KERNELGSBASE};
use crate::arch::amd64::include::trap::{T_NMI, T_PAGEFLT, T_TRCTRAP};
use crate::arch::amd64::include::vmparam::{VM_MAXUSER_ADDRESS, VM_MIN_KERNEL_ADDRESS};
use crate::kern::subr_prf::{Str, db_printf, panic, panicstr_claim, printf, vsnprintf};
use crate::sys::errno::Errno;
use crate::sys::mman::{PROT_EXEC, PROT_READ, PROT_WRITE};
use crate::unported;
use crate::uvm::uvm_extern::VmProt;
use crate::uvm::uvm_init::UVMEXP;
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

    // struct proc *p = curproc; if (p == NULL || p->p_addr == NULL || p->p_vmspace == NULL)
    // return 0: there is no proc before M5 (see the module's deviations), so nothing below
    // runs yet.
    if curcpu().ci_curproc.get().is_null() {
        return false;
    }

    // pcb = &p->p_addr->u_pcb; the __nofault_start/__nofault_end check of pcb_onfault: M5.
    let pcb_onfault: Option<usize> = None;
    let _ = unported!("pcb_onfault (kpageflttrap, M5)");

    // This will only trigger if SMEP is enabled
    if pcb_onfault.is_none()
        && cr2 <= VM_MAXUSER_ADDRESS as u64
        && frame.tf_err as u64 & PGEX_I != 0
    {
        fault(format_args!(
            "attempt to execute user address {cr2:#x} in supervisor mode"
        ));
        return false;
    }
    // This will only trigger if SMAP is enabled
    if pcb_onfault.is_none()
        && cr2 <= VM_MAXUSER_ADDRESS as u64
        && frame.tf_err as u64 & PGEX_P != 0
    {
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
        // onfault = pcb->pcb_onfault; pcb->pcb_onfault = NULL;
        // error = uvm_fault(map, va, 0, access_type); pcb->pcb_onfault = onfault;
        // if (error == 0 && map != kernel_map) uvm_grow(p, va): M6.
        Some(unported!("uvm_fault (M6)"))
    } else {
        Some(Errno::EFAULT)
    };

    match (error, pcb_onfault) {
        (None, _) => true,
        (Some(error), None) => {
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
        (Some(_), Some(onfault)) => {
            frame.tf_rip = onfault as i64;
            true
        }
    }
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
