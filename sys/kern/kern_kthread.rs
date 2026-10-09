/* <LICENSES> */
/*-
 * Copyright (c) 1998, 1999 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center.
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
/* </LICENSES> */

/* <CODE> */
//! Kernel thread handling: `kern/kern_kthread.c`.
//!
//! Upstream: sys/kern/kern_kthread.c @ 3ce1f3f79392
//!
//! Status: `ported` (M5-b1: `kthread_create_deferred` and `kthread_run_deferred_queue`;
//! M5-b2: `kthread_create`; M6-b: `kthread_exit` over `exit1`; M11a: the kernel lock in
//! `kthread_create`).
//!
//! ## Deviations
//! - `kthread_create` returns the new thread (`Result<&Proc, Errno>`) instead of an `int`
//!   plus an out-pointer.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::kern::init_main::PROC0;
use crate::kern::kern_exit::exit1;
use crate::kern::kern_fork::fork1;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::machine::cpu::curproc;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_TEMP, M_ZERO};
use crate::sys::proc::{
    EXIT_NORMAL, FORK_NOZOMBIE, FORK_SHAREFILES, FORK_SHAREVM, FORK_SYSTEM, Proc,
};
use crate::sys::queue::{SimpleqEntry, SimpleqHead};
use crate::sys::systm::{kernel_lock, kernel_unlock};

/// `kthread_create_now`: set once the standard kernel threads exist.
pub static KTHREAD_CREATE_NOW: AtomicBool = AtomicBool::new(false);

/// `kthread_create`: fork a kernel thread. Any process can request this to be done. The VM
/// space and limits, etc. will be shared with proc0. Returns the new thread (the C's
/// `*newpp`).
pub fn kthread_create(
    func: fn(*mut c_void),
    arg: *mut c_void,
    name: &[u8],
) -> Result<&'static Proc, Errno> {
    kernel_lock(); // KERNEL_LOCK()

    // First, create the new process. Share the memory, file descriptors and don't leave the
    // exit status around for the parent to wait for.
    let p = match fork1(
        &PROC0,
        FORK_SHAREVM | FORK_SHAREFILES | FORK_NOZOMBIE | FORK_SYSTEM,
        func,
        arg,
    ) {
        Ok(p) => p,
        Err(error) => {
            kernel_unlock(); // KERNEL_UNLOCK()
            return Err(error);
        }
    };

    // Name it as specified.
    p.process().set_comm(name);

    kernel_unlock(); // KERNEL_UNLOCK()

    // All done!
    Ok(p)
}

/// `kthread_exit`: cause a kernel thread to exit. Assumes the exiting thread is the current
/// context.
pub fn kthread_exit(ecode: i32) -> ! {
    // XXX What do we do with the exit code? Should we even bother with it? The parent
    // (proc0) isn't going to do much with it.
    if ecode != 0
        && let Some(p) = curproc()
    {
        printf(format_args!(
            "WARNING: thread `{}' ({}) exits with status {}\n",
            Str(p.process().comm()),
            p.p_tid.get(),
            ecode
        ));
    }

    let Some(p) = curproc() else {
        panic(format_args!("kthread_exit: no curproc"));
    };
    exit1(p, ecode, 0, EXIT_NORMAL)
}

/// `struct kthread_q`: a deferred creation.
struct KthreadQ {
    /// `kq_q`.
    kq_q: SimpleqEntry<KthreadQ>,
    /// `kq_func`.
    kq_func: fn(*mut c_void),
    /// `kq_arg`.
    kq_arg: *mut c_void,
}

queue_adapter!(
    /// `SIMPLEQ_HEAD(, kthread_q) kthread_q`.
    KthreadQList: KthreadQ, kq_q => SimpleqEntry<KthreadQ>
);

/// `kthread_q`'s head, made `Sync`: filled and emptied during boot, by the boot CPU alone
/// (`cpu_attach` runs there; the queue is run before `cpu_boot_secondary_processors`).
struct KthreadQHead(SimpleqHead<KthreadQList>);
// SAFETY: see the type's doc.
unsafe impl Sync for KthreadQHead {}

/// `kthread_q`.
static KTHREAD_Q: KthreadQHead = KthreadQHead(SimpleqHead::new());

/// `kthread_create_deferred`: defer the creation of a kernel thread. Once the standard
/// kernel threads and processes have been created, this queue will be run to callback to the
/// caller to create threads for e.g. file systems and device drivers.
pub fn kthread_create_deferred(func: fn(*mut c_void), arg: *mut c_void) {
    if KTHREAD_CREATE_NOW.load(Ordering::Relaxed) {
        func(arg);
        return;
    }

    let Some(kq) = malloc(size_of::<KthreadQ>(), M_TEMP, M_NOWAIT | M_ZERO) else {
        panic(format_args!("unable to allocate kthread_q"));
    };
    let kq = kq.cast::<KthreadQ>();
    // SAFETY: a fresh allocation, written once before it is linked.
    unsafe {
        kq.as_ptr().write(KthreadQ {
            kq_q: SimpleqEntry::new(),
            kq_func: func,
            kq_arg: arg,
        })
    };
    // SAFETY: as above; the entry stays allocated until `kthread_run_deferred_queue` frees it.
    let kq: &'static KthreadQ = unsafe { kq.as_ref() };

    // SAFETY: `kq` is in no queue and lives until it is removed below.
    unsafe { KTHREAD_Q.0.insert_tail(kq) };
}

/// `kthread_run_deferred_queue`: runs the deferred creations, in order.
pub fn kthread_run_deferred_queue() {
    // No longer need to defer kthread creation.
    KTHREAD_CREATE_NOW.store(true, Ordering::Relaxed);

    while let Some(kq) = KTHREAD_Q.0.first() {
        // SAFETY: `kq` is the first element, under the boot CPU's exclusive use.
        unsafe { KTHREAD_Q.0.remove_head() };
        (kq.kq_func)(kq.kq_arg);
        // `kq` was allocated by `kthread_create_deferred` and is now unlinked.
        free(
            NonNull::from(kq).cast::<u8>(),
            M_TEMP,
            size_of::<KthreadQ>(),
        );
    }
    let _ = ptr::null::<KthreadQ>();
}
/* </CODE> */
