/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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
//! `<machine/tcb.h>` as a trait: the kernel side of the thread control block, the per-thread
//! pointer through which user space reaches its thread-local storage (`FS.base` on amd64,
//! `TPIDR_EL0` on arm64).
//!
//! `kern_prot.c` needs `TCB_GET` and `TCB_SET` for `__get_tcb(2)`/`__set_tcb(2)`, and
//! `TCB_INVALID` where the architecture defines it; exec resets the pointer with `TCB_SET`.
//! The userland half of the header (`TLS_VARIANT`, `TCB_GET()` reading the register) is not
//! kernel code. The TCB is a user address the kernel never dereferences, so it travels as a
//! `usize`.

use crate::machine::Machine;
use crate::sys::proc::Proc;

/// The thread control block pointer of the selected architecture.
pub trait Tcb {
    /// `TCB_GET(p)`: the TCB address saved for thread `p`.
    fn tcb_get(p: &Proc) -> usize;

    /// `TCB_SET(p, addr)`: records `addr` as the TCB of `p`, which must be `curproc`, so
    /// that `p` finds it in its TLS register when it next runs in user mode.
    fn tcb_set(p: &Proc, addr: usize);

    /// `TCB_INVALID(addr)`: `true` when `addr` can never be a TCB. Architectures whose
    /// header does not define the macro (arm64) accept every address.
    fn tcb_invalid(addr: usize) -> bool;
}

/// `TCB_GET(p)` on the selected machine.
pub fn tcb_get(p: &Proc) -> usize {
    Machine::tcb_get(p)
}

/// `TCB_SET(p, addr)` on the selected machine.
pub fn tcb_set(p: &Proc, addr: usize) {
    Machine::tcb_set(p, addr)
}

/// `TCB_INVALID(addr)` on the selected machine (`false` where the C has no such macro).
pub fn tcb_invalid(addr: usize) -> bool {
    Machine::tcb_invalid(addr)
}
/* </CODE> */
