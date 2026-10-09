/* <LICENSES> */
/*
 * Copyright (c) 2015 Dale Rahn <drahn@dalerahn.com>
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
//! arm64 `syscall.c`: the `svc` system call entry and the child's first return.
//!
//! Upstream: sys/arch/arm64/arm64/syscall.c @ 3ce1f3f79392
//!
//! Status: `ported` (M6-a): `svc_handler` and `child_return`; M11a: `child_return` drops the
//! kernel lock (`KERNEL_UNLOCK`, `MULTIPROCESSOR`).

use core::ffi::c_void;
use core::sync::atomic::Ordering;

use crate::arch::arm64::include::armreg::{PSR_C, PSR_I};
use crate::arch::arm64::include::cpu::{curcpu, intr_enable};
use crate::arch::arm64::include::frame::Trapframe;
use crate::kern::init_sysent::SYSENT;
use crate::kern::subr_prf::panic;
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::syscall::SYS_MAXSYSCALL;
use crate::sys::syscall_mi::{mi_child_return, mi_syscall, mi_syscall_return};
use crate::sys::systm::{SysArgs, kernel_unlock};
use crate::sys::types::Register;
use crate::uvm::uvm_init::UVMEXP;

/// `svc_handler`: a system call from EL0 (`do_el0_sync`, `EXCP_SVC`).
pub fn svc_handler(frame: &mut Trapframe) {
    // SAFETY: `ci_curproc` names the thread that made the system call, hence alive.
    let Some(p) = (unsafe { curcpu().ci_curproc.get().as_ref() }) else {
        panic(format_args!("svc_handler: no curproc"));
    };

    UVMEXP.syscalls.fetch_add(1, Ordering::Relaxed);

    // Re-enable interrupts if they were enabled previously
    if frame.tf_spsr as u64 & PSR_I == 0 {
        // SAFETY: the user had them enabled; the kernel takes interrupts during a syscall.
        unsafe { intr_enable() };
    }

    // Skip over speculation-blocking barrier.
    frame.tf_elr += 8;

    let code: Register = frame.tf_x[8];
    let args: &SysArgs = frame.tf_x[..6].try_into().unwrap_or(&[0; 6]);

    let mut rval: [Register; 2] = [0, 0];

    let error = if code <= 0 || code as usize >= SYS_MAXSYSCALL {
        Err(Errno::ENOSYS)
    } else {
        let callp = &SYSENT[code as usize];
        mi_syscall(p, code, callp, args, &mut rval)
    };

    match error {
        Ok(()) => {
            frame.tf_x[0] = rval[0];
            frame.tf_spsr &= !(PSR_C as Register); // carry bit
        }
        Err(Errno::ERESTART) => {
            // Reconstruct the pc to point at the svc.
            frame.tf_elr -= 12;
        }
        Err(Errno::EJUSTRETURN) => {
            // nothing to do
        }
        Err(e) => {
            frame.tf_x[0] = e as i32 as Register;
            frame.tf_spsr |= PSR_C as Register; // carry bit
        }
    }

    mi_syscall_return(p, code, error, &rval);
}

/// `child_return`: the first thing a forked user thread runs: a `fork` return of 0 in the
/// child, then the user-mode return.
pub fn child_return(arg: *mut c_void) {
    // SAFETY: `fork1` passes the new thread itself as the argument.
    let p = unsafe { &*arg.cast::<Proc>() };
    let frame = p.pcb().pcb_tf.get();

    // SAFETY: `cpu_fork` set `pcb_tf` to the trap frame at the top of the thread's u-area.
    unsafe {
        (*frame).tf_x[0] = 0;
        (*frame).tf_spsr &= !(PSR_C as Register); // carry bit
    }

    // The kernel lock `proc_trampoline_mi` took for the new thread (nothing without
    // MULTIPROCESSOR).
    kernel_unlock();

    mi_child_return(p);
}
/* </CODE> */
