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
//! `<machine/proc.h>`, `<machine/pcb.h>` and `proc0paddr` as a trait: the machine-dependent
//! parts of a thread.
//!
//! Milestone M5 (part b) needs `struct mdproc` (the per-thread machine state `struct proc`
//! embeds) and `struct pcb` (the process control block `struct user` holds at the top of the
//! kernel stack). Both are plain data per architecture; generic code only embeds them and
//! hands references to the machine.

use crate::machine::Machine;

/// The machine-dependent thread and process control block types.
pub trait MachineProc {
    /// `struct mdproc`: machine-dependent part of the proc structure.
    type Mdproc: 'static;

    /// The `struct mdproc` of a thread before `cpu_fork`: all zero (what a `static struct
    /// proc` holds). An associated constant, so `proc0` can be a `static`.
    const MDPROC_INIT: Self::Mdproc;

    /// `struct pcb`: the process control block.
    type Pcb: 'static;

    /// The `struct pcb` of a thread before `cpu_fork`: all zero.
    const PCB_INIT: Self::Pcb;
}

/// `struct mdproc` on the selected machine.
pub type Mdproc = <Machine as MachineProc>::Mdproc;

/// `struct pcb` on the selected machine.
pub type Pcb = <Machine as MachineProc>::Pcb;

// The initialisers are used as `<Machine as MachineProc>::MDPROC_INIT`/`PCB_INIT`: a free
// `const` of a type with interior mutability would trip `declare_interior_mutable_const`.
/* </CODE> */
