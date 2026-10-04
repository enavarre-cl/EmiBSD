/* $OpenBSD: ast.c,v 1.9 2026/03/08 17:07:31 deraadt Exp $ */
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

//! arm64 `ast.c`: transform cpu ast to `mi_ast`.
//!
//! Upstream: sys/arch/arm64/arm64/ast.c @ 3ce1f3f79392
//!
//! Status: `ported` (M6-a).

use core::sync::atomic::Ordering;

use crate::arch::arm64::include::cpu::curcpu;
use crate::arch::arm64::include::frame::Trapframe;
use crate::kern::kern_sig::userret;
use crate::kern::subr_prf::panic;
use crate::sys::proc::refreshcreds;
use crate::sys::syscall_mi::mi_ast;
use crate::uvm::uvm_init::UVMEXP;

/// `ast(tf)`: the asynchronous system trap on the way back to user mode (`do_ast` in
/// `exception.S`).
#[unsafe(no_mangle)]
pub extern "C" fn ast(tf: &mut Trapframe) {
    let ci = curcpu();
    // SAFETY: `ci_curproc` names the thread returning to user mode, hence alive.
    let Some(p) = (unsafe { ci.ci_curproc.get().as_ref() }) else {
        panic(format_args!("ast: no curproc"));
    };

    p.pcb().pcb_tf.set(tf);

    refreshcreds(p);
    UVMEXP.softs.fetch_add(1, Ordering::Relaxed);
    mi_ast(p, ci.ci_want_resched.load(Ordering::Relaxed) != 0);
    userret(p);
}
