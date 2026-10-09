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

/*
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz and Don Ahn.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by the University of
 *      California, Berkeley and its contributors.
 * 4. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */
/*
 * Copyright (c) 2001 Opsycon AB  (www.opsycon.se)
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS
 * OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! arm64 signal delivery: `arch/arm64/arm64/sig_machdep.c`.
//!
//! Upstream: sys/arch/arm64/arm64/sig_machdep.c @ 3ce1f3f79392
//!
//! Status: `ported` (with `kern_sig.c`): `process_frame`, `sendsig` and `sys_sigreturn`.
//! `sys_sigreturn` is reached through `machine::MachineSignal` from the system call table's
//! entry in `kern/kern_sig.rs`.
//!
//! ## Deviations
//! - The floating-point context is not saved here, as in C ("XXX"): the trampoline
//!   (`sigcode`, `locore.S`) saves and restores the `q` registers on the user stack.
//! - The 4-clause licence (advertising clause) of the first block was accepted by the user
//!   at M2 for this project.

use core::mem::offset_of;

use crate::arch::arm64::include::armreg::{PSR_BTYPE, PSR_F, PSR_I, PSR_M_EL0t, PSR_M_MASK};
use crate::arch::arm64::include::frame::{Sigframe, Trapframe};
use crate::arch::arm64::include::param::stackalign;
use crate::arch::arm64::include::signal::Sigcontext;
use crate::kern::kern_sig::{sigexit, sigonstack};
use crate::machine::Machine;
use crate::machine::copy::{copyin_obj, copyout, copyout_obj};
use crate::machine::cpu::{Cpu, curproc};
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::siginfo::Siginfo;
use crate::sys::signal::{SIGILL, SS_DISABLE, Sig, Sigset};
use crate::sys::signalvar::sigcantmask;
use crate::sys::syscallargs::SysSigreturnArgs;
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::types::Register;
use crate::uvm::uvm_param::trunc_page;

/// `process_frame(p)`: the thread's user trap frame.
fn process_frame(p: &Proc) -> *mut Trapframe {
    p.pcb().pcb_tf.get()
}

/// `sendsig`: send an interrupt to process.
///
/// Stack is set up to allow sigcode to call routine, followed by syscall to sigreturn
/// routine below. After sigreturn resets the signal mask, the stack, and the frame pointer,
/// it returns to the user specified pc.
pub fn sendsig(
    catcher: Sig,
    sig: i32,
    mask: Sigset,
    ksip: &Siginfo,
    info: bool,
    onstack: bool,
) -> Result<(), Errno> {
    let Some(p) = curproc() else {
        return Err(Errno::EFAULT);
    };
    // SAFETY: `pcb_tf` is the current thread's trap frame on its kernel stack (the
    // exception entry recorded it); only this thread touches it, and no other reference to
    // it is alive while we run.
    let tf = unsafe { &mut *process_frame(p) };

    // Allocate space for the signal handler context.
    let ss = p.p_sigstk.get();
    let mut fp = if ss.ss_flags & SS_DISABLE == 0 && !sigonstack(tf.tf_sp as usize) && onstack {
        trunc_page(ss.ss_sp + ss.ss_size)
    } else {
        tf.tf_sp as usize
    };

    // make room on the stack
    fp = fp.wrapping_sub(size_of::<Sigframe>());

    // make the stack aligned
    fp = stackalign(fp);

    // Build stack frame for signal trampoline.
    let mut frame = Sigframe {
        sf_signum: sig,
        ..Sigframe::default()
    };

    // Save register context.
    for (sc, &x) in frame.sf_sc.sc_x.iter_mut().zip(tf.tf_x.iter()) {
        *sc = x as u64;
    }
    frame.sf_sc.sc_sp = tf.tf_sp as u64;
    frame.sf_sc.sc_lr = tf.tf_lr as u64;
    frame.sf_sc.sc_elr = tf.tf_elr as u64;
    frame.sf_sc.sc_spsr = tf.tf_spsr as u64;

    // Save signal mask.
    frame.sf_sc.sc_mask = mask as i32;

    // XXX Save floating point context

    let mut sip = 0usize;
    if info {
        sip = fp + offset_of!(Sigframe, sf_si);
        frame.sf_si = *ksip;
    }

    let scp = fp + offset_of!(Sigframe, sf_sc);
    frame.sf_sc.sc_cookie = (scp as i64) ^ (p.process().ps_sigcookie.get() as i64);
    copyout_obj(&frame, fp)?;

    // Build context to run handler in. We invoke the handler directly, only returning via
    // the trampoline.
    tf.tf_x[0] = sig as Register;
    tf.tf_x[1] = sip as Register;
    tf.tf_x[2] = scp as Register;
    tf.tf_lr = catcher as Register;
    tf.tf_sp = fp as Register;

    tf.tf_elr = p.process().ps_sigcode.get() as Register;
    tf.tf_spsr &= !(PSR_BTYPE as Register);

    Ok(())
}

/// `sys_sigreturn`: system call to cleanup state after a signal has been taken. Reset
/// signal mask and stack state from context left by sendsig (above). Return to previous pc
/// and psl as specified by context left by sendsig. Check carefully to make sure that the
/// user has not modified the psr to gain improper privileges or to cause a machine fault.
pub fn sys_sigreturn(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSigreturnArgs = sysargs(v);
    let scp = uap.sigcntxp.get() as usize;
    let pr = p.process();

    if <Machine as Cpu>::proc_pc(p) != pr.ps_sigcoderet.get() {
        sigexit(p, SIGILL);
        // return (EPERM): sigexit does not return.
    }

    let Ok(mut ksc) = copyin_obj::<Sigcontext>(scp) else {
        return Err(Errno::EFAULT);
    };

    if ksc.sc_cookie != ((scp as i64) ^ (pr.ps_sigcookie.get() as i64)) {
        sigexit(p, SIGILL);
        // return (EFAULT): sigexit does not return.
    }

    // Prevent reuse of the sigcontext cookie
    ksc.sc_cookie = 0;
    let _ = copyout(
        &ksc.sc_cookie.to_ne_bytes(),
        scp + offset_of!(Sigcontext, sc_cookie),
    );

    // Make sure the processor mode has not been tampered with and interrupts have not been
    // disabled.
    if ksc.sc_spsr & PSR_M_MASK != PSR_M_EL0t || ksc.sc_spsr & (PSR_I | PSR_F) != 0 {
        return Err(Errno::EINVAL);
    }

    // XXX Restore floating point context

    // Restore register context.
    // SAFETY: as in `sendsig`: the current thread's trap frame, which the `svc` entry
    // recorded.
    let tf = unsafe { &mut *process_frame(p) };
    for (x, &sc) in tf.tf_x.iter_mut().zip(ksc.sc_x.iter()) {
        *x = sc as Register;
    }
    tf.tf_sp = ksc.sc_sp as Register;
    tf.tf_lr = ksc.sc_lr as Register;
    tf.tf_elr = ksc.sc_elr as Register;
    tf.tf_spsr = ksc.sc_spsr as Register;

    // Restore signal mask.
    p.p_sigmask.set(ksc.sc_mask as Sigset & !sigcantmask());

    Err(Errno::EJUSTRETURN)
}
/* </CODE> */
