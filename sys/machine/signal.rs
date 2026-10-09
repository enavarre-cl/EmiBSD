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
//! `<machine/signal.h>` and the machine-dependent half of signal delivery as a trait: the
//! `struct sigcontext` a handler sees, `sendsig` (`<sys/signalvar.h>`'s machine-dependent
//! function), `sys_sigreturn` and the signal trampoline (`sigcode`) that `exec` maps into
//! every process.
//!
//! `kern_sig.c` calls `sendsig` to build the handler's frame on the user stack; the handler
//! returns into the trampoline, which calls `sigreturn(2)`, whose machine-dependent body
//! restores the registers saved in the `struct sigcontext`. `kern_exec.c`'s
//! `exec_sigcode_map` copies the trampoline (`sigcode` to `esigcode`, `sigfill` after it)
//! into a shared object and records where `sigcoderet` is, the only place `sigreturn(2)`
//! may be called from.

use crate::machine::Machine;
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::siginfo::Siginfo;
use crate::sys::signal::{Sig, Sigset};
use crate::sys::systm::SysArgs;
use crate::sys::types::Register;

/// The machine-dependent signal delivery of the selected architecture.
pub trait MachineSignal {
    /// `struct sigcontext`: information pushed on stack when a signal is delivered (ABI).
    type Sigcontext: 'static;

    /// `sendsig(catcher, sig, mask, ksip, info, onstack)`: send an interrupt to the current
    /// process: the stack is set up to allow `sigcode` to call `catcher`, followed by a
    /// system call to `sigreturn`. `mask` is the signal mask to restore afterwards; `ksip`
    /// is copied out when `info` is set; `onstack` asks for the alternate signal stack.
    /// `Err` (the C's nonzero return) when the frame cannot be written to user space.
    fn sendsig(
        catcher: Sig,
        sig: i32,
        mask: Sigset,
        ksip: &Siginfo,
        info: bool,
        onstack: bool,
    ) -> Result<(), Errno>;

    /// `sys_sigreturn(p, v, retval)`: system call to clean up state after a signal has been
    /// taken: reset signal mask and stack state from the context left by `sendsig`.
    /// `Err(EJUSTRETURN)` is success.
    fn sys_sigreturn(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno>;

    /// `sigcode` .. `esigcode`: the signal trampoline's instructions.
    fn sigcode() -> &'static [u8];

    /// `sigcoderet - sigcode`: the offset of the instruction after the `sigreturn` system
    /// call in the trampoline (`PROC_PC(p)` during that call).
    fn sigcoderet() -> usize;

    /// `sigcodecall - sigcode`: the offset of the system call instruction itself;
    /// `sigcoderet - sigcodecall` is how far `PROC_PC(p)` is past any system call
    /// instruction during the call (`pin_check`).
    fn sigcodecall() -> usize;

    /// `sigfill` .. `esigfill`: the trap instruction(s) the rest of the trampoline's page is
    /// filled with (`sigfillsiz` is the slice's length).
    fn sigfill() -> &'static [u8];
}

/// `struct sigcontext` on the selected machine.
pub type Sigcontext = <Machine as MachineSignal>::Sigcontext;

/// `sendsig` on the selected machine.
pub fn sendsig(
    catcher: Sig,
    sig: i32,
    mask: Sigset,
    ksip: &Siginfo,
    info: bool,
    onstack: bool,
) -> Result<(), Errno> {
    Machine::sendsig(catcher, sig, mask, ksip, info, onstack)
}
/* </CODE> */
