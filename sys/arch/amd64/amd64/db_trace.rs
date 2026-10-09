/* <LICENSES> */
/*
 * Mach Operating System
 * Copyright (c) 1991,1990 Carnegie Mellon University
 * All Rights Reserved.
 *
 * Permission to use, copy, modify and distribute this software and its
 * documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND FOR
 * ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 stack traces for `ddb(4)`: `arch/amd64/amd64/db_trace.c`.
//!
//! Upstream: sys/arch/amd64/amd64/db_trace.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `db_stack_trace_print` for the "trace from this frame"
//! case `db_stack_dump` needs; M4 adds the trace from `ddb_regs` (a trap frame) with
//! `db_reg_args[]` and the breakpoint-before-the-frame case; M11c `db_regs[]` (the `$rdi`
//! variables of the command loop). The `/t` thread trace (`tfind`), `stacktrace_save_at` and
//! `stacktrace_save_utrace` arrive with the scheduler (M5).
//!
//! ## Deviations
//! - No symbol table in memory yet (`db_search_symbol`, `db_ctf_func_numargs`, `db_printsym`):
//!   every frame prints its return address as a number and its six "arguments" as the words
//!   below the frame pointer, as the C does for a function without CTF data. `cargo xtask
//!   symbolize` turns the addresses into names offline.
//! - `db_get_value` is a plain read: the faulting-read protection of `db_read_bytes` arrives
//!   with M4. The checks against the previous frame are the same as the C's.
//! - `CR4.SMAP` is not disabled around the walk: `rcr4`/`lcr4` come with M4, and SMAP is not
//!   enabled before then.

use core::mem::offset_of;
use core::ptr;

use crate::arch::amd64::amd64::db_interface::DDB_REGS;
use crate::arch::amd64::include::frame::{Callframe, Trapframe};
use crate::arch::amd64::include::vmparam::VM_MIN_KERNEL_ADDRESS;
use crate::db_reg_var;
use crate::ddb::db_variables::DbVariable;
use crate::machine::db_machdep::PrFn;
use crate::unported;

/// `db_regs[]`: the registers of `ddb_regs` as debugger variables. Each reads and writes its
/// field of the trap frame (`db_reg_var!`) where C points `valuep` at it.
pub static DB_REGS: [DbVariable; 20] = [
    db_reg_var!(DDB_REGS, "rdi", tf_rdi),
    db_reg_var!(DDB_REGS, "rsi", tf_rsi),
    db_reg_var!(DDB_REGS, "rbp", tf_rbp),
    db_reg_var!(DDB_REGS, "rbx", tf_rbx),
    db_reg_var!(DDB_REGS, "rdx", tf_rdx),
    db_reg_var!(DDB_REGS, "rcx", tf_rcx),
    db_reg_var!(DDB_REGS, "rax", tf_rax),
    db_reg_var!(DDB_REGS, "r8", tf_r8),
    db_reg_var!(DDB_REGS, "r9", tf_r9),
    db_reg_var!(DDB_REGS, "r10", tf_r10),
    db_reg_var!(DDB_REGS, "r11", tf_r11),
    db_reg_var!(DDB_REGS, "r12", tf_r12),
    db_reg_var!(DDB_REGS, "r13", tf_r13),
    db_reg_var!(DDB_REGS, "r14", tf_r14),
    db_reg_var!(DDB_REGS, "r15", tf_r15),
    db_reg_var!(DDB_REGS, "rip", tf_rip),
    db_reg_var!(DDB_REGS, "cs", tf_cs),
    db_reg_var!(DDB_REGS, "rflags", tf_rflags),
    db_reg_var!(DDB_REGS, "rsp", tf_rsp),
    db_reg_var!(DDB_REGS, "ss", tf_ss),
];

/// `INKERNEL(va)`.
fn inkernel(va: usize) -> bool {
    va >= VM_MIN_KERNEL_ADDRESS
}

/// `db_get_value(addr, 8, 0)`: one word of the stack, or of the code when the trace peeks
/// at the instruction at `callpc`, which is not aligned (see the module's deviations).
fn db_get_value(addr: usize) -> usize {
    // SAFETY: the callers walk the frame chain of the current stack, each frame checked to lie
    // in kernel space above the previous one, so the word is in mapped stack memory; the one
    // code read is at a return address in the mapped kernel text. The read is unaligned, as
    // the C's is on x86.
    unsafe { ptr::read_unaligned(addr as *const usize) }
}

/// `db_reg_args[]`: the registers that carry the first six arguments, from `ddb_regs`.
fn db_reg_args(regs: &Trapframe) -> [i64; 6] {
    [
        regs.tf_rdi,
        regs.tf_rsi,
        regs.tf_rdx,
        regs.tf_rcx,
        regs.tf_r8,
        regs.tf_r9,
    ]
}

/// `db_stack_trace_print`: prints the frames from `addr` (a `struct callframe`), or from the
/// trap frame in `ddb_regs` without an address, at most `count` of them, through `pr`.
pub fn db_stack_trace_print(addr: usize, have_addr: bool, count: usize, modif: &[u8], pr: PrFn) {
    let mut kernel_only = true;
    let mut trace_proc = false;
    for &c in modif {
        if c == b't' {
            trace_proc = true;
        }
        if c == b'u' {
            kernel_only = false;
        }
    }

    if trace_proc {
        let _ = unported!("tfind (trace /t)");
        pr(format_args!("not found\n"));
        return;
    }

    // cr4save = rcr4(); if (cr4save & CR4_SMAP) lcr4(cr4save & ~CR4_SMAP): SMAP is not
    // enabled before CPU identification (M4-b).

    let mut frame;
    let mut callpc;
    let mut tf_rsp = 0usize;
    if !have_addr {
        // SAFETY: ddb_regs is read while the debugger is active, after db_ktrap wrote it.
        let regs = unsafe { DDB_REGS.get() };
        frame = regs.tf_rbp as usize;
        callpc = regs.tf_rip as usize;
        tf_rsp = regs.tf_rsp as usize;
    } else {
        frame = addr;
        callpc = db_get_value(frame + offset_of!(Callframe, f_retaddr));
        frame = db_get_value(frame + offset_of!(Callframe, f_frame));
    }

    let mut lastframe = 0usize;
    let mut count = count;
    while count != 0 && frame != 0 {
        // No symbol table: sym == NULL and name == NULL for every frame.
        let mut offset = 1usize;
        if lastframe == 0 && callpc != 0 {
            // Symbol not found, peek at code
            let instr = db_get_value(callpc);

            if instr == 0xe589_4855
                /* enter: pushq %rbp, movq %rsp, %rbp */
                || (instr & 0x00ff_ffff) == 0x00e5_8948
            /* enter+1: movq %rsp, %rbp */
            {
                offset = 0;
            }
        }

        // db_ctf_func_numargs(NULL) < 0, so six arguments are shown.
        let mut narg = 6usize;

        pr(format_args!("{callpc:x}("));

        let arg0;
        if lastframe == 0 && offset == 0 && !have_addr {
            // We have a breakpoint before the frame is set up
            // SAFETY: as above.
            let args = db_reg_args(unsafe { DDB_REGS.get() });
            // The C counts `narg` down inside its `i < narg` loop, so four registers are
            // printed and the last two arguments come from the stack below.
            let mut i = 0;
            while i < narg {
                pr(format_args!("{:x}", args[i]));
                narg -= 1;
                if narg != 0 {
                    pr(format_args!(","));
                }
                i += 1;
            }

            // Use %rsp instead
            arg0 = tf_rsp - 8 + offset_of!(Callframe, f_arg0);
        } else {
            let mut argp = frame;
            for remaining in (1..=narg).rev() {
                argp -= core::mem::size_of::<usize>();
                pr(format_args!("{:x}", db_get_value(argp)));
                if remaining != 1 {
                    pr(format_args!(","));
                }
            }
            narg = 0;

            arg0 = frame + offset_of!(Callframe, f_arg0);
        }

        let mut argp = arg0;
        while narg > 0 {
            pr(format_args!("{:x}", db_get_value(argp)));
            argp += core::mem::size_of::<usize>();
            narg -= 1;
            if narg != 0 {
                pr(format_args!(","));
            }
        }
        pr(format_args!(") at "));
        // db_printsym(callpc, DB_STGY_PROC, pr) without symbols prints the address.
        pr(format_args!("{callpc:#x}"));
        pr(format_args!("\n"));

        if lastframe == 0 && offset == 0 && !have_addr {
            // Frame really belongs to next callpc
            lastframe = tf_rsp - 8;
            callpc = db_get_value(lastframe + offset_of!(Callframe, f_retaddr));
            continue;
        }

        lastframe = frame;
        callpc = db_get_value(frame + offset_of!(Callframe, f_retaddr));
        frame = db_get_value(frame + offset_of!(Callframe, f_frame));

        if frame == 0 {
            // end of chain
            break;
        }
        if inkernel(frame) {
            // staying in kernel
            if frame <= lastframe {
                pr(format_args!("Bad frame pointer: {frame:#x}\n"));
                break;
            }
        } else if inkernel(lastframe) {
            // switch from user to kernel
            if kernel_only {
                pr(format_args!("end of kernel\n"));
                break; // kernel stack only
            }
        } else {
            // in user
            if frame <= lastframe {
                pr(format_args!("Bad user frame pointer: {frame:#x}\n"));
                break;
            }
        }
        count -= 1;
    }
    let _ = lastframe;
    // `%d` of the C's signed counter: `trace` without a count starts it at -1.
    pr(format_args!(
        "end trace frame: {frame:#x}, count: {}\n",
        count as i32
    ));
}
/* </CODE> */
