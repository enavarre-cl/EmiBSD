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
//! `kdata_abort`, `do_el1h_sync`, `serror`, `do_el1h_error` and `dumpregs`. `udata_abort`,
//! `emulate_msr`, `do_el0_sync`, `do_el0_error` and the `svc` system call path come with user
//! mode (M6).
//!
//! ## Deviations
//! - `kdata_abort`: `curcpu()->ci_curpcb` is null before M5, read as "no `pcb_onfault`", and
//!   the process's `vm_map` does not exist; `pmap_fault_fixup` (M6) and `uvm_fault` (M6) are
//!   reported, so every kernel data abort ends in the C's `panic("uvm_fault failed: ...")`.
//! - The `we_re_toast` path prints the syndrome and enters `db_ktrap` as the `DDB` build does,
//!   then panics with the same message as the non-`DDB` build: ddb-lite has no command loop
//!   to stay in, and returning would re-execute the faulting instruction.
//! - `fault` writes `curcpu()->ci_panicbuf` as the C does; `panic()` itself still uses
//!   `subr_prf`'s buffer (`kern/subr_prf.rs`, deviations).

use core::fmt;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::arch::arm64::arm64::db_interface::db_ktrap;
use crate::arch::arm64::include::armreg::{
    EXCP_BRANCH_TGT, EXCP_BRK, EXCP_DATA_ABORT, EXCP_FP_SIMD, EXCP_FPAC, EXCP_INSN_ABORT,
    EXCP_SOFTSTP_EL1, EXCP_TRAP_FP, EXCP_WATCHPT_EL1, INSN_SIZE, ISS_BRK_COMMENT_MASK, ISS_DATA_CM,
    ISS_DATA_WNR, esr_elx_exception, read_specialreg,
};
use crate::arch::arm64::include::cpu::{curcpu, intr_enable};
use crate::arch::arm64::include::frame::Trapframe;
use crate::kern::subr_prf::{Str, db_printf, panic, panicstr_claim, printf, vsnprintf};
use crate::sys::mman::{PROT_EXEC, PROT_READ, PROT_WRITE};
use crate::unported;
use crate::uvm::uvm_extern::VmProt;
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

    // pcb = curcpu()->ci_curpcb; p = curcpu()->ci_curproc: none before M5 (see the module's
    // deviations), so pcb_onfault is never set.
    let pcb_onfault: usize = 0;

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

    // Handle referenced/modified emulation: pmap_fault_fixup(map->pmap, va, access_type),
    // then uvm_fault(map, va, 0, access_type) and uvm_grow for a user map (M6). Both are
    // reported; the result is the C's error path.
    let _ = unported!("pmap_fault_fixup (M6)");
    let _ = unported!(if kernel_map {
        "uvm_fault(kernel_map) (M6)"
    } else {
        "uvm_fault(vm_map) (M6)"
    });
    let _ = (va, access_type);

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
