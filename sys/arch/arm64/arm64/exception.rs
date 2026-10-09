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
//! The exception entry code of `arch/arm64/arm64/exception.S`, pulled in from the `.S` file
//! next to this module (the file keeps OpenBSD's licence block and layout; `{NAME}`
//! placeholders are what `assym.h` provides in C).
//!
//! Upstream: sys/arch/arm64/arm64/exception.S @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `save_registers`/`restore_registers`, the four EL1h
//! handlers and `exception_vectors`; M6-a `do_ast`, `disable_ss`/`allow_ss`, the four EL0
//! handlers and `syscall_return`. The 32-bit EL0 slots stay `vempty` (no compat).
//!
//! ## Deviations
//! - `x18` is a general register here (the C builds with `-ffixed-x18` and keeps `curcpu()`
//!   in it): the EL1 paths of the macros save the pre-exception `sp` with an `add` instead of
//!   parking it in `x18` first, restore `x18` from the frame, and recover `sp` from the
//!   frame's size instead of `mov sp, x18`. The EL0 paths are the C's.
//! - `return` is an `eret`: the Spectre trampoline vectors (`trampoline.S`,
//!   `CI_TRAMPOLINE_VECTORS`, `tramp_return`) are not ported, so `VBAR_EL1` keeps the
//!   kernel vectors across user mode.

use core::arch::global_asm;
use core::mem::offset_of;

use crate::arch::arm64::include::armreg::DBG_MDSCR_SS;
use crate::arch::arm64::include::cpu::CpuInfo;
use crate::arch::arm64::include::frame::{TF_SIZE, Trapframe};
use crate::arch::arm64::include::pcb::Pcb;
use crate::sys::proc::Proc;

global_asm!(
    include_str!("exception.S"),
    TF_SIZE = const TF_SIZE,
    TF_X = const offset_of!(Trapframe, tf_x),
    TF_ELR = const offset_of!(Trapframe, tf_elr),
    TF_SP = const offset_of!(Trapframe, tf_sp),
    CI_CURPROC = const offset_of!(CpuInfo, ci_curproc),
    CI_CURPCB = const offset_of!(CpuInfo, ci_curpcb),
    P_ASTPENDING = const offset_of!(Proc, p_md.md_astpending),
    PCB_FLAGS = const offset_of!(Pcb, pcb_flags),
    DBG_MDSCR_SS = const DBG_MDSCR_SS,
);

unsafe extern "C" {
    /// `exception_vectors`: the 2 KiB vector table `VBAR_EL1` points at (16 entries of 128
    /// bytes).
    pub static exception_vectors: [u32; 512];
}

/// The address of the vector table, for `VBAR_EL1`.
pub fn exception_vectors_addr() -> u64 {
    core::ptr::addr_of!(exception_vectors) as u64
}
/* </CODE> */
