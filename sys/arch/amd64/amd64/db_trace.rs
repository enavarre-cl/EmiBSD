/*	$OpenBSD: db_trace.c,v 1.60 2025/08/03 11:17:08 sashan Exp $	*/
/*	$NetBSD: db_trace.c,v 1.1 2003/04/26 18:39:27 fvdl Exp $	*/

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

//! amd64 stack traces for `ddb(4)`: `arch/amd64/amd64/db_trace.c`.
//!
//! Upstream: sys/arch/amd64/amd64/db_trace.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `db_stack_trace_print` for the "trace from this frame"
//! case `db_stack_dump` needs. `db_regs[]`, `db_reg_args[]`, the trace from `ddb_regs` (a trap
//! frame), the `/t` thread trace (`tfind`), `stacktrace_save_at` and `stacktrace_save_utrace`
//! arrive with the trap handlers and the scheduler (M4, M5).
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

use crate::arch::amd64::include::frame::Callframe;
use crate::arch::amd64::include::vmparam::VM_MIN_KERNEL_ADDRESS;
use crate::machine::db_machdep::PrFn;
use crate::unported;

/// `INKERNEL(va)`.
fn inkernel(va: usize) -> bool {
    va >= VM_MIN_KERNEL_ADDRESS
}

/// `db_get_value(addr, 8, 0)`: one word of the stack (see the module's deviations).
fn db_get_value(addr: usize) -> usize {
    // SAFETY: the callers walk the frame chain of the current stack, each frame checked to lie
    // in kernel space above the previous one, so the word is in mapped stack memory.
    unsafe { ptr::read_volatile(addr as *const usize) }
}

/// `db_stack_trace_print`: prints the frames from `addr` (a `struct callframe`), at most
/// `count` of them, through `pr`.
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

    if !have_addr {
        // frame = ddb_regs.tf_rbp; callpc = ddb_regs.tf_rip: the trap frame arrives with M4.
        let _ = unported!("ddb_regs (trace without an address)");
        return;
    }
    let mut frame = addr;
    let mut callpc = db_get_value(frame + offset_of!(Callframe, f_retaddr));
    frame = db_get_value(frame + offset_of!(Callframe, f_frame));

    let mut lastframe = 0usize;
    let mut count = count;
    while count != 0 && frame != 0 {
        // No symbol: db_ctf_func_numargs(NULL) < 0, so six arguments are shown.
        let narg = 6;

        pr(format_args!("{callpc:x}("));

        // The breakpoint-before-the-frame case needs ddb_regs (M4); the frame is set up.
        let mut argp = frame;
        for remaining in (1..=narg).rev() {
            argp -= core::mem::size_of::<usize>();
            pr(format_args!("{:x}", db_get_value(argp)));
            if remaining != 1 {
                pr(format_args!(","));
            }
        }
        // arg0 = &frame->f_arg0; narg is 0 here, so nothing more is printed.

        pr(format_args!(") at "));
        // db_printsym(callpc, DB_STGY_PROC, pr) without symbols prints the address.
        pr(format_args!("{callpc:#x}"));
        pr(format_args!("\n"));

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
    pr(format_args!(
        "end trace frame: {frame:#x}, count: {count}\n"
    ));
}
