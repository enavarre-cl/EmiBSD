/*	$OpenBSD: db_trace.c,v 1.20 2025/07/22 09:11:13 kettenis Exp $	*/
/*	$NetBSD: db_trace.c,v 1.8 2003/01/17 22:28:48 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2000, 2001 Ben Harris
 * Copyright (c) 1996 Scott K. Stevens
 *
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
//! arm64 stack traces for `ddb(4)`: `arch/arm64/arm64/db_trace.c`.
//!
//! Upstream: sys/arch/arm64/arm64/db_trace.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `db_stack_trace_print` for the "trace from this frame"
//! case `db_stack_dump` needs; M4 adds the trace from `ddb_regs` (a trap frame). The `/t`
//! thread trace (`tfind`, `switchframe`), `stacktrace_save_at` and `stacktrace_save_utrace`
//! arrive with the scheduler (M5).
//!
//! ## Deviations
//! - No symbol table in memory yet (`db_search_symbol`, `db_printsym`): every frame prints
//!   its return addresses as numbers, as the C does for an address without a symbol, and the
//!   "--- interrupt ---" / "--- trap ---" markers, which hinge on symbol names, cannot appear.
//!   `cargo xtask symbolize` turns the addresses into names offline.
//! - `db_get_value` is a plain read: the faulting-read protection of `db_read_bytes` arrives
//!   with M4. The checks against the previous frame are the same as the C's.
//! - The C decrements `count` twice per frame (`while (count--)` and `--count`); so does
//!   this, on a signed counter, so the two behave alike at the limit.

use core::ptr;

use crate::arch::arm64::arm64::db_interface::DDB_REGS;
use crate::machine::db_machdep::PrFn;
use crate::unported;

/// `INKERNEL(va)`.
fn inkernel(va: usize) -> bool {
    va & (1 << 63) != 0
}

/// `db_get_value(addr, 8, 0)`: one word of the stack (see the module's deviations).
fn db_get_value(addr: usize) -> usize {
    // SAFETY: the callers walk the frame chain of the current stack, each frame checked to lie
    // in kernel space above the previous one, so the word is in mapped stack memory.
    unsafe { ptr::read_volatile(addr as *const usize) }
}

/// `db_stack_trace_print`: prints the frames from `addr` (a `struct callframe`), or from the
/// trap frame in `ddb_regs` without an address, at most `count` of them, through `pr`.
pub fn db_stack_trace_print(addr: usize, have_addr: bool, count: usize, modif: &[u8], pr: PrFn) {
    let mut kernel_only = true;
    let mut trace_thread = false;
    for &c in modif {
        if c == b'u' {
            kernel_only = false;
        }
        if c == b't' {
            trace_thread = true;
        }
    }

    if trace_thread {
        let _ = unported!("tfind (trace /t)");
        pr(format_args!("not found\n"));
        return;
    }

    let mut frame;
    let mut lr;
    if !have_addr {
        // SAFETY: ddb_regs is read while the debugger is active, after db_ktrap wrote it.
        let regs = unsafe { DDB_REGS.get() };
        frame = regs.tf_x[29] as usize;
        lr = regs.tf_elr as usize;
    } else {
        frame = db_get_value(addr);
        lr = db_get_value(addr + 8);
    }

    let mut count = count as i64;
    loop {
        let go_on = count != 0;
        count -= 1;
        if !go_on || frame == 0 {
            break;
        }
        let lastlr = lr;
        lr = db_get_value(frame + 8);

        // No symbol: name == NULL.
        pr(format_args!("{lastlr:x} at {:#x}", lr.wrapping_sub(4)));
        pr(format_args!("\n"));

        let lastframe = frame;
        frame = db_get_value(frame);

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
}
/* </CODE> */
