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

/* <CODE> */
//! arm64 exception handling: `arch/arm64/arm64/trap.c`.
//!
//! Upstream: sys/arch/arm64/arm64/trap.c @ 3ce1f3f79392
//!
//! Status: `ported` (M11a). Milestone M4 ports the EL1 side: `is_unpriv_ldst`, `accesstype`, `fault`,
//! `kdata_abort`, `do_el1h_sync`, `serror`, `do_el1h_error` and `dumpregs`; M6-a adds
//! `do_el0_sync` (the `svc` path, `syscall.rs`) and `do_el0_error`, and `kdata_abort`'s
//! `pcb_onfault` recovery; M6-b/M7a `udata_abort`; `kern_sig.c` the `trapsignal`s of every
//! EL0 exception, `fpu_load` for the FP traps and `sve_load` for the SVE one. M11a:
//! `emulate_msr` (with `cpu.c`'s `cpu_id_aa64*`) and `do_el0_sync`'s `KERNEL_LOCK` around
//! `sigexit`. M11e: `kdata_abort` and `udata_abort` run `uvm_fault` and `uvm_grow` without the
//! kernel lock, as the C does (uvm takes the locks it needs, the kernel lock included around
//! the vnode pager's I/O and `pgo_fault`).
//!
//! ## Deviations
//! - `do_el0_sync`'s `KERNEL_UNLOCK` after `sigexit` is a comment: `sigexit` never returns.
//! - The `we_re_toast` path prints the syndrome and enters `db_ktrap` as the `DDB` build does,
//!   then panics with the same message as the non-`DDB` build: ddb-lite has no command loop
//!   to stay in, and returning would re-execute the faulting instruction.
//! - `fault` writes `curcpu()->ci_panicbuf` as the C does; `panic()` itself still uses
//!   `subr_prf`'s buffer (`kern/subr_prf.rs`, deviations).

use core::fmt;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::arch::arm64::arm64::cpu;
use crate::arch::arm64::arm64::db_interface::db_ktrap;
use crate::arch::arm64::arm64::fpu::{fpu_load, sve_load};
use crate::arch::arm64::arm64::pmap::pmap_fault_fixup;
use crate::arch::arm64::arm64::syscall::svc_handler;
use crate::arch::arm64::include::armreg::{
    EXCP_BRANCH_TGT, EXCP_BRK, EXCP_DATA_ABORT, EXCP_DATA_ABORT_L, EXCP_FP_SIMD, EXCP_FPAC,
    EXCP_INSN_ABORT, EXCP_INSN_ABORT_L, EXCP_MSR, EXCP_PC_ALIGN, EXCP_SOFTSTP_EL0,
    EXCP_SOFTSTP_EL1, EXCP_SP_ALIGN, EXCP_SVC, EXCP_SVE, EXCP_TRAP_FP, EXCP_UNKNOWN,
    EXCP_WATCHPT_EL1, INSN_SIZE, ISS_BRK_COMMENT_MASK, ISS_DATA_CM, ISS_DATA_DFSC_ALIGN,
    ISS_DATA_DFSC_MASK, ISS_DATA_WNR, ISS_MSR_CRM_MASK, ISS_MSR_CRM_SHIFT, ISS_MSR_CRN_MASK,
    ISS_MSR_CRN_SHIFT, ISS_MSR_DIR, ISS_MSR_OP0_MASK, ISS_MSR_OP0_SHIFT, ISS_MSR_OP1_MASK,
    ISS_MSR_OP1_SHIFT, ISS_MSR_OP2_MASK, ISS_MSR_OP2_SHIFT, ISS_MSR_RT_MASK, ISS_MSR_RT_SHIFT,
    esr_elx_exception, iss_msr_field, read_specialreg,
};
use crate::arch::arm64::include::cpu::{curcpu, intr_enable};
use crate::arch::arm64::include::frame::Trapframe;
use crate::arch::arm64::include::vmparam::VM_MAXUSER_ADDRESS;
use crate::kern::kern_sig::{sigexit, trapsignal, userret};
use crate::kern::subr_prf::{Str, db_printf, panic, panicstr_claim, printf, vsnprintf};
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::sys::errno::Errno;
use crate::sys::mman::{PROT_EXEC, PROT_READ, PROT_WRITE};
use crate::sys::proc::refreshcreds;
use crate::sys::siginfo::{
    BUS_ADRALN, BUS_OBJERR, ILL_BTCFI, ILL_ILLOPC, SEGV_ACCERR, SEGV_MAPERR, Sigval, TRAP_BRKPT,
    TRAP_TRACE,
};
use crate::sys::signal::{SIGBUS, SIGILL, SIGKILL, SIGSEGV, SIGTRAP};
use crate::sys::systm::kernel_lock;
use crate::sys::types::Register;
use crate::sys::types::Vaddr;
use crate::uvm::uvm_extern::VmProt;
use crate::uvm::uvm_fault::uvm_fault;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_map::{uvm_map_inentry, uvm_map_inentry_sp};
use crate::uvm::uvm_param::trunc_page;
use crate::uvm::uvm_unix::uvm_grow;

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
        if !kernel_map && let Some(p) = p {
            uvm_grow(p, va);
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

/// `udata_abort`: a data or instruction abort from EL0: `pmap_fault_fixup`, then
/// `uvm_fault` on the process's map; a fault that cannot be served is a signal.
fn udata_abort(_frame: &mut Trapframe, esr: u64, far: u64, exe: bool) {
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
        trapsignal(p, SIGBUS, esr, BUS_ADRALN, Sigval::from_ptr(far as usize));
        return;
    }

    let map = &p.vmspace().vm_map;

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
    if !ok {
        return;
    }

    // Handle referenced/modified emulation
    if pmap_fault_fixup(map.pmap(), Vaddr::new(va), access_type) {
        return;
    }
    let result = uvm_fault(map, va, 0, access_type);
    if result.is_ok() {
        uvm_grow(p, va);
    }
    let error = match result {
        Ok(()) => return,
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
    trapsignal(p, sig, esr, code, Sigval::from_ptr(far as usize));
}

/// `emulate_msr`: emulate a read of an ID register from EL0 with the values userland may see
/// (`cpu_identify_cleanup`): `true` when the access was emulated and the instruction
/// skipped.
fn emulate_msr(frame: &mut Trapframe, esr: u64) -> bool {
    let field = |mask, shift| iss_msr_field(esr, mask, shift);
    let rt = field(ISS_MSR_RT_MASK, ISS_MSR_RT_SHIFT) as usize;
    let id = |a: &core::sync::atomic::AtomicU64| a.load(Ordering::Relaxed);

    // Only emulate reads.
    if esr & ISS_MSR_DIR == 0 {
        return false;
    }

    // Only emulate non-debug System register access.
    if field(ISS_MSR_OP0_MASK, ISS_MSR_OP0_SHIFT) != 3
        || field(ISS_MSR_OP1_MASK, ISS_MSR_OP1_SHIFT) != 0
        || field(ISS_MSR_CRN_MASK, ISS_MSR_CRN_SHIFT) != 0
    {
        return false;
    }

    let op2 = field(ISS_MSR_OP2_MASK, ISS_MSR_OP2_SHIFT);
    let val = match (field(ISS_MSR_CRM_MASK, ISS_MSR_CRM_SHIFT), op2) {
        // MIDR_EL1
        (0, 0) => read_specialreg!("midr_el1"),
        // MPIDR_EL1: don't reveal the topology to userland. But return a valid value; Bit
        // 31 is RES1.
        (0, 5) => 0x8000_0000,
        // REVIDR_EL1
        (0, 6) => 0,
        // ID_AA64PFR0_EL1
        (4, 0) => id(&cpu::CPU_ID_AA64PFR0),
        // ID_AA64PFR1_EL1
        (4, 1) => id(&cpu::CPU_ID_AA64PFR1),
        // ID_AA64PFR2_EL1, ID_AA64ZFR0_EL1, ID_AA64SMFR0_EL1
        (4, 2 | 4 | 5) => 0,
        // ID_AA64ISAR0_EL1
        (6, 0) => id(&cpu::CPU_ID_AA64ISAR0),
        // ID_AA64ISAR1_EL1
        (6, 1) => id(&cpu::CPU_ID_AA64ISAR1),
        // ID_AA64ISAR2_EL2
        (6, 2) => id(&cpu::CPU_ID_AA64ISAR2),
        // ID_AA64MMFR0_EL1 .. ID_AA64MMFR4_EL1
        (7, 0..=4) => 0,
        _ => return false,
    };

    if rt < 30 {
        frame.tf_x[rt] = val as Register;
    } else if rt == 30 {
        frame.tf_lr = val as Register;
    }
    frame.tf_elr += 4;

    true
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

    let flush_bp = || {
        if let Some(flush_bp) = ci.ci_flush_bp.get() {
            flush_bp();
        }
    };
    let elr = Sigval::from_ptr(frame.tf_elr as usize);

    match exception {
        EXCP_UNKNOWN => {
            flush_bp();
            trapsignal(p, SIGILL, esr, ILL_ILLOPC, elr);
        }
        EXCP_SVE => sve_load(p),
        EXCP_FP_SIMD | EXCP_TRAP_FP => fpu_load(p),
        EXCP_BRANCH_TGT => {
            flush_bp();
            trapsignal(p, SIGILL, esr, ILL_BTCFI, elr);
        }
        EXCP_MSR if emulate_msr(frame, esr) => {}
        EXCP_MSR | EXCP_FPAC => {
            flush_bp();
            trapsignal(p, SIGILL, esr, ILL_ILLOPC, elr);
        }
        EXCP_SVC => svc_handler(frame),
        EXCP_INSN_ABORT_L => udata_abort(frame, esr, far, true),
        EXCP_PC_ALIGN => {
            flush_bp();
            trapsignal(p, SIGBUS, esr, BUS_ADRALN, elr);
        }
        EXCP_SP_ALIGN => {
            flush_bp();
            let sv = Sigval::from_ptr(frame.tf_sp as usize);
            trapsignal(p, SIGBUS, esr, BUS_ADRALN, sv);
        }
        EXCP_DATA_ABORT_L => udata_abort(frame, esr, far, false),
        EXCP_BRK => trapsignal(p, SIGTRAP, esr, TRAP_BRKPT, elr),
        EXCP_SOFTSTP_EL0 => trapsignal(p, SIGTRAP, esr, TRAP_TRACE, elr),
        _ => {
            // panic("Unknown userland exception %x esr_el1 %lx", exception, esr);
            // USERLAND MUST NOT PANIC MACHINE
            // only here to debug !?!?
            printf(format_args!(
                "exception {:x} esr_el1 {:x}\n",
                exception, esr
            ));
            dumpregs(frame);
            flush_bp();
            kernel_lock();
            sigexit(p, SIGILL);
            // KERNEL_UNLOCK(): sigexit does not return.
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
/* </CODE> */

/* <TESTS> */
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
/* </TESTS> */
