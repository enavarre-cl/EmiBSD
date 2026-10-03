/* $OpenBSD: trap.c,v 1.55 2026/03/08 17:07:31 deraadt Exp $ */
/* <LICENSES> */
/*-
 * Copyright (c) 2014 Andrew Turner
 * All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

//! arm64 exception handling: `arch/arm64/arm64/trap.c`.
//!
//! Upstream: sys/arch/arm64/arm64/trap.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports the EL1 side: `is_unpriv_ldst`, `accesstype`, `fault`,
//! `kdata_abort`, `do_el1h_sync`, `serror`, `do_el1h_error` and `dumpregs`; M6-a adds
//! `do_el0_sync` (the `svc` path, `syscall.rs`) and `do_el0_error`, and `kdata_abort`'s
//! `pcb_onfault` recovery. `udata_abort`, `emulate_msr` and the `trapsignal`s of the other
//! EL0 exceptions come with user address spaces and signals (M6-b).
//!
//! ## Deviations
//! - `do_el0_sync`: every exception but `svc` reports its `trapsignal`/`udata_abort` and
//!   panics (no signals yet, M6-b); the C never panics for user mode.
//! - The `we_re_toast` path prints the syndrome and enters `db_ktrap` as the `DDB` build does,
//!   then panics with the same message as the non-`DDB` build: ddb-lite has no command loop
//!   to stay in, and returning would re-execute the faulting instruction.
//! - `fault` writes `curcpu()->ci_panicbuf` as the C does; `panic()` itself still uses
//!   `subr_prf`'s buffer (`kern/subr_prf.rs`, deviations).

use core::fmt;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::arch::arm64::arm64::db_interface::db_ktrap;
use crate::arch::arm64::arm64::pmap::pmap_fault_fixup;
use crate::arch::arm64::arm64::syscall::svc_handler;
use crate::arch::arm64::include::armreg::{
    EXCP_BRANCH_TGT, EXCP_BRK, EXCP_DATA_ABORT, EXCP_DATA_ABORT_L, EXCP_FP_SIMD, EXCP_FPAC,
    EXCP_INSN_ABORT, EXCP_INSN_ABORT_L, EXCP_SOFTSTP_EL1, EXCP_SVC, EXCP_TRAP_FP, EXCP_WATCHPT_EL1,
    INSN_SIZE, ISS_BRK_COMMENT_MASK, ISS_DATA_CM, ISS_DATA_DFSC_ALIGN, ISS_DATA_DFSC_MASK,
    ISS_DATA_WNR, esr_elx_exception, read_specialreg,
};
use crate::arch::arm64::include::cpu::{curcpu, intr_enable};
use crate::arch::arm64::include::frame::Trapframe;
use crate::arch::arm64::include::vmparam::VM_MAXUSER_ADDRESS;
use crate::kern::kern_exit::exit1;
use crate::kern::kern_sig::userret;
use crate::kern::subr_prf::{Str, db_printf, panic, panicstr_claim, printf, vsnprintf};
use crate::sys::errno::Errno;
use crate::sys::mman::{PROT_EXEC, PROT_READ, PROT_WRITE};
use crate::sys::proc::{EXIT_NORMAL, Proc, refreshcreds};
use crate::sys::signal::{SIGBUS, SIGILL, SIGKILL, SIGSEGV};
use crate::sys::types::Vaddr;
use crate::unported;
use crate::uvm::uvm_extern::VmProt;
use crate::uvm::uvm_fault::uvm_fault;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_param::trunc_page;

/// `is_unpriv_ldst`: whether the instruction at `elr` (a kernel address) is an unprivileged
/// load or store (`ldtr`/`sttr` family), the only way the kernel may touch user addresses.
fn is_unpriv_ldst(elr: u64) -> bool {
    if (elr >> 63) == 1 {
        // SAFETY: `elr` is the kernel address of the instruction that just faulted, so it is
        // mapped and readable.
        let insn = unsafe { ptr::read_volatile(elr as usize as *const u32) };
        return (insn & 0x3f20_0c00) == 0x3800_0800;
    }

    false
}

/// `accesstype`: the access an abort's syndrome describes.
pub fn accesstype(esr: u64, exe: bool) -> VmProt {
    if exe {
        return PROT_EXEC;
    }
    if (esr & ISS_DATA_CM) == 0 && (esr & ISS_DATA_WNR) != 0 {
        PROT_WRITE
    } else {
        PROT_READ
    }
}

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

/// `kdata_abort`: a data or instruction abort taken at EL1.
fn kdata_abort(frame: &mut Trapframe, esr: u64, far: u64, exe: bool) {
    let ci = curcpu();
    let access_type = accesstype(esr, exe);

    // SAFETY: `ci_curpcb` is the running thread's pcb, alive while it runs.
    let pcb_onfault: usize =
        unsafe { ci.ci_curpcb.get().as_ref() }.map_or(0, |pcb| pcb.pcb_onfault.get());

    let va = trunc_page(far as usize);

    // The top bit tells us which range to use
    let kernel_map = if (far >> 63) == 1 {
        true
    } else if is_unpriv_ldst(frame.tf_elr as u64) {
        // Only allow user-space access using unprivileged load/store instructions.
        // map = &p->p_vmspace->vm_map
        false
    } else if pcb_onfault != 0 {
        true
    } else {
        fault(format_args!(
            "attempt to {} user address 0x{:x} from EL1",
            if exe { "execute" } else { "access" },
            far
        ));
        db_ktrap(esr_elx_exception(esr) as i32, frame);
        true
    };

    // SAFETY: `ci_curproc` names the thread on this CPU, hence alive.
    let p = unsafe { ci.ci_curproc.get().as_ref() };
    let map = match p {
        Some(p) if !kernel_map => &p.vmspace().vm_map,
        _ => crate::uvm::uvm_km::kernel_map(),
    };

    // Handle referenced/modified emulation
    if pmap_fault_fixup(map.pmap(), Vaddr::new(va), access_type) {
        return;
    }

    let error = uvm_fault(map, va, 0, access_type);
    if error.is_ok() {
        if !kernel_map {
            // uvm_grow(p, va): uvm_unix.c, with the stack accounting of M7+.
            let _ = unported!("uvm_grow (uvm_unix.c)");
        }
        return;
    }

    // error != 0:
    if ci.ci_idepth.get() == 0 && pcb_onfault != 0 {
        frame.tf_elr = pcb_onfault as isize;
        return;
    }
    panic(format_args!(
        "uvm_fault failed: {:x} esr {:x} far {:x}",
        frame.tf_elr, esr, far
    ));
}

/// `do_el1h_sync`: the synchronous exception handler for EL1, called from
/// `handle_el1h_sync` (`exception.S`) with the saved registers.
#[unsafe(no_mangle)]
pub extern "C" fn do_el1h_sync(frame: &mut Trapframe) {
    // Read the ESR and FAR registers to get the exception details
    let esr = read_specialreg!("esr_el1");
    let far = read_specialreg!("far_el1");

    // SAFETY: the exception entry masked interrupts; the kernel takes them during a trap.
    unsafe { intr_enable() };
    UVMEXP.traps.fetch_add(1, Ordering::Relaxed);

    let exception = esr_elx_exception(esr);
    let toast = match exception {
        EXCP_FP_SIMD | EXCP_TRAP_FP => {
            fault(format_args!("FP exception in kernel"));
            true
        }
        EXCP_BRANCH_TGT => {
            fault(format_args!("Branch target exception in kernel"));
            true
        }
        EXCP_FPAC => {
            fault(format_args!("Pointher authentication failure in kernel"));
            true
        }
        EXCP_INSN_ABORT => {
            kdata_abort(frame, esr, far, true);
            false
        }
        EXCP_DATA_ABORT => {
            kdata_abort(frame, esr, far, false);
            false
        }
        EXCP_BRK | EXCP_WATCHPT_EL1 | EXCP_SOFTSTP_EL1 => {
            db_ktrap(exception as i32, frame);
            // Step over permanent breakpoints.
            if exception == EXCP_BRK && (esr & ISS_BRK_COMMENT_MASK) == 0xf000 {
                frame.tf_elr += INSN_SIZE as isize;
            }
            false
        }
        _ => {
            fault(format_args!("Unknown kernel exception 0x{exception:02x}"));
            true
        }
    };
    if toast {
        // we_re_toast:
        db_printf(format_args!(
            "esr 0x{:08x} far 0x{:016x} elr 0x{:016x}",
            esr, far, frame.tf_elr
        ));
        db_ktrap(exception as i32, frame);
        panic(format_args!(
            "esr 0x{:08x} far 0x{:016x} elr 0x{:016x}",
            esr, far, frame.tf_elr
        ));
    }
}

/// `serror`: reports a system error interrupt and calls the CPU's handler, if any.
fn serror(frame: &Trapframe) {
    let ci = curcpu();

    let esr = read_specialreg!("esr_el1");
    let far = read_specialreg!("far_el1");

    printf(format_args!(
        "SError: {:x} esr {:x} far {:0x}\n",
        frame.tf_elr, esr, far
    ));

    if let Some(handler) = ci.ci_serror.get() {
        handler();
    }
}

/// `do_el1h_error`: an SError taken at EL1, called from `handle_el1h_error` (`exception.S`).
#[unsafe(no_mangle)]
pub extern "C" fn do_el1h_error(frame: &mut Trapframe) {
    serror(frame);
    panic(format_args!("do_el1h_error"));
}

/// `udata_abort`: a data or instruction abort from EL0: `uvm_fault` on the process's map
/// (M7a); until then every user page is wired by exec, so the fault is a bad address and
/// the process dies of the signal (see `trapsignal`).
fn udata_abort(frame: &mut Trapframe, esr: u64, far: u64, exe: bool) {
    let ci = curcpu();
    // SAFETY: `ci_curproc` names the thread that trapped from user mode, hence alive.
    let Some(p) = (unsafe { ci.ci_curproc.get().as_ref() }) else {
        panic(format_args!("udata_abort: no curproc"));
    };
    let access_type = accesstype(esr, exe);

    let va = trunc_page(far as usize);
    if va >= VM_MAXUSER_ADDRESS
        && let Some(flush_bp) = ci.ci_flush_bp.get()
    {
        flush_bp();
    }

    if esr & ISS_DATA_DFSC_MASK == ISS_DATA_DFSC_ALIGN {
        trapsignal(p, frame, SIGBUS, esr, BUS_ADRALN, far);
    }

    let map = &p.vmspace().vm_map;
    // uvm_map_inentry (the MAP_STACK check): with the stack of M7a-3b.

    // Handle referenced/modified emulation
    if pmap_fault_fixup(map.pmap(), Vaddr::new(va), access_type) {
        return;
    }
    let error = match uvm_fault(map, va, 0, access_type) {
        Ok(()) => {
            // uvm_grow(p, va): uvm_unix.c, with the stack accounting of M7+.
            let _ = unported!("uvm_grow (uvm_unix.c)");
            return;
        }
        Err(e) => e,
    };

    let (sig, code) = if error == Errno::ENOMEM {
        (SIGKILL, 0)
    } else if error == Errno::EIO {
        (SIGBUS, BUS_OBJERR)
    } else if error == Errno::EACCES {
        (SIGSEGV, SEGV_ACCERR)
    } else {
        (SIGSEGV, SEGV_MAPERR)
    };
    trapsignal(p, frame, sig, esr, code, far);
}

/// `SEGV_MAPERR`: address not mapped to object (`<sys/siginfo.h>`, M6-c).
const SEGV_MAPERR: i32 = 1;
/// `SEGV_ACCERR`: invalid permissions.
const SEGV_ACCERR: i32 = 2;
/// `BUS_ADRALN`: invalid address alignment.
const BUS_ADRALN: i32 = 1;
/// `BUS_OBJERR`: object specific hardware error.
const BUS_OBJERR: i32 = 3;

/// `trapsignal(p, sig, esr, code, sv)` until `kern_sig.c` lands (M6-c): there is no handler
/// to run, so the process dies of the signal (what `sigexit` does), after a dump of the
/// frame so the fault is visible on the console.
fn trapsignal(p: &Proc, frame: &Trapframe, sig: i32, esr: u64, code: i32, addr: u64) -> ! {
    let _ = unported!("trapsignal (kern_sig.c, M6-c): the process dies of the signal");
    printf(format_args!(
        "pid {} ({}): signal {} (esr {:#x} code {}) at elr {:#x} addr {:#x}\n",
        p.process().ps_pid.get(),
        Str(p.process().comm()),
        sig,
        esr,
        code,
        frame.tf_elr,
        addr
    ));
    dumpregs(frame);
    sigexit(p, sig)
}

/// `sigexit(p, signum)`: the process dies of `signum` (the core dump is M7).
fn sigexit(p: &Proc, signum: i32) -> ! {
    exit1(p, 0, signum, EXIT_NORMAL)
}

/// `do_el0_sync`: the synchronous exception handler for EL0, called from `handle_el0_sync`
/// (`exception.S`) with the saved registers.
#[unsafe(no_mangle)]
pub extern "C" fn do_el0_sync(frame: &mut Trapframe) {
    let ci = curcpu();
    // SAFETY: `ci_curproc` names the thread that trapped from user mode, hence alive.
    let Some(p) = (unsafe { ci.ci_curproc.get().as_ref() }) else {
        panic(format_args!("do_el0_sync: no curproc"));
    };

    let esr = read_specialreg!("esr_el1");
    let exception = esr_elx_exception(esr);
    let far = read_specialreg!("far_el1");

    // SAFETY: the exception entry masked interrupts; the kernel takes them during a trap.
    unsafe { intr_enable() };
    UVMEXP.traps.fetch_add(1, Ordering::Relaxed);

    p.pcb().pcb_tf.set(frame);
    refreshcreds(p);

    match exception {
        EXCP_SVC => svc_handler(frame),
        EXCP_INSN_ABORT_L => udata_abort(frame, esr, far, true),
        EXCP_DATA_ABORT_L => udata_abort(frame, esr, far, false),
        _ => {
            // EXCP_UNKNOWN/BRANCH_TGT/MSR/FPAC/PC_ALIGN/SP_ALIGN/BRK/SOFTSTP_EL0: trapsignal;
            // EXCP_SVE/FP_SIMD/TRAP_FP: sve_load/fpu_load (M6-c); the default: USERLAND
            // MUST NOT PANIC MACHINE, so sigexit(SIGILL) after the debug print.
            let _ = unported!("do_el0_sync: trapsignal/fpu_load (M6-c): the process dies");
            printf(format_args!(
                "exception {:x} esr_el1 {:x}\n",
                exception, esr
            ));
            dumpregs(frame);
            if let Some(flush_bp) = ci.ci_flush_bp.get() {
                flush_bp();
            }
            sigexit(p, SIGILL);
        }
    }

    userret(p);
}

/// `do_el0_error`: an SError taken at EL0, called from `handle_el0_error` (`exception.S`).
#[unsafe(no_mangle)]
pub extern "C" fn do_el0_error(frame: &mut Trapframe) {
    serror(frame);
    panic(format_args!("do_el0_error"));
}

/// `dumpregs`: prints a trap frame.
pub fn dumpregs(frame: &Trapframe) {
    for i in (0..30).step_by(2) {
        printf(format_args!(
            "x{:02}: 0x{:016x} 0x{:016x}\n",
            i,
            frame.tf_x[i],
            frame.tf_x[i + 1]
        ));
    }
    printf(format_args!("sp: 0x{:016x}\n", frame.tf_sp));
    printf(format_args!("lr: 0x{:016x}\n", frame.tf_lr));
    printf(format_args!("pc: 0x{:016x}\n", frame.tf_elr));
    printf(format_args!("spsr: 0x{:016x}\n", frame.tf_spsr));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syndrome_to_access() {
        assert_eq!(accesstype(0, true), PROT_EXEC);
        assert_eq!(accesstype(0, false), PROT_READ);
        assert_eq!(accesstype(ISS_DATA_WNR, false), PROT_WRITE);
        // a cache maintenance instruction reports WnR but is a read for the fault's purposes
        assert_eq!(accesstype(ISS_DATA_WNR | ISS_DATA_CM, false), PROT_READ);
    }
}
