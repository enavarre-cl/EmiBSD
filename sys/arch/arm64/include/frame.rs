/* $OpenBSD: frame.h,v 1.3 2018/06/30 15:23:37 deraadt Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2016 Dale Rahn <drahn@dalerahn.com>
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

//! arm64 `<machine/frame.h>`: the stack frames the kernel walks and builds.
//!
//! Upstream: sys/arch/arm64/include/frame.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `struct callframe`, M4 `struct trapframe` (`clockframe` is the
//! same struct); `struct sigframe` and `struct switchframe` come with M5 and M6. M2's note: `struct callframe`, what the frame-pointer chain is made
//! of (`stp x29, x30, [sp, #-16]!` leaves the caller's frame and the link register at `x29`).
//! `struct trapframe` and `struct switchframe` arrive with the trap and context-switch code
//! (M4, M5).

use crate::sys::types::Register;

/// `struct callframe`: one link of the frame-pointer chain.
#[repr(C)]
pub struct Callframe {
    /// The caller's frame (its saved `x29`).
    pub f_frame: *const Callframe,
    /// The saved link register: the return address into the caller.
    pub f_lr: Register,
}

const _: () = {
    assert!(core::mem::size_of::<Callframe>() == 16);
    assert!(core::mem::offset_of!(Callframe, f_lr) == 8);
};

/// `struct trapframe` (`trapframe_t`): what `exception.S` saves on entry.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Trapframe {
    /// `tf_sp`.
    pub tf_sp: Register,
    /// `tf_lr`.
    pub tf_lr: Register,
    /// `tf_elr`: the exception link register (the PC).
    pub tf_elr: Register,
    /// `tf_spsr`: the saved program status.
    pub tf_spsr: Register,
    /// `tf_x`: x0 to x29.
    pub tf_x: [Register; 30],
}

impl Trapframe {
    /// An empty frame.
    pub const fn new() -> Self {
        Self {
            tf_sp: 0,
            tf_lr: 0,
            tf_elr: 0,
            tf_spsr: 0,
            tf_x: [0; 30],
        }
    }
}

impl Default for Trapframe {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct switchframe`: stack frame inside `cpu_switch()`: the callee-saved registers
/// `cpu_switchto_asm` stores, `x19` to `x29` and `lr`. `cpu_fork` builds one so a new
/// thread's first switch returns into `proc_trampoline`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Switchframe {
    /// `sf_x19`.
    pub sf_x19: Register,
    /// `sf_x20`.
    pub sf_x20: Register,
    /// `sf_x21`.
    pub sf_x21: Register,
    /// `sf_x22`.
    pub sf_x22: Register,
    /// `sf_x23`.
    pub sf_x23: Register,
    /// `sf_x24`.
    pub sf_x24: Register,
    /// `sf_x25`.
    pub sf_x25: Register,
    /// `sf_x26`.
    pub sf_x26: Register,
    /// `sf_x27`.
    pub sf_x27: Register,
    /// `sf_x28`.
    pub sf_x28: Register,
    /// `sf_x29`.
    pub sf_x29: Register,
    /// `sf_lr`.
    pub sf_lr: Register,
}

/// `TF_SIZE`: the size of a trap frame.
pub const TF_SIZE: usize = size_of::<Trapframe>();

/// `SWITCHFRAME_SZ`: the size of a switch frame.
pub const SWITCHFRAME_SZ: usize = size_of::<Switchframe>();

const _: () = {
    assert!(TF_SIZE == 34 * 8);
    assert!(core::mem::offset_of!(Trapframe, tf_x) == 32);
    assert!(SWITCHFRAME_SZ == 12 * 8);
};
