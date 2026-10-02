//! The exception entry code of `arch/arm64/arm64/exception.S`, pulled in from the `.S` file
//! next to this module (the file keeps OpenBSD's licence block and layout; `{NAME}`
//! placeholders are what `assym.h` provides in C).
//!
//! Upstream: sys/arch/arm64/arm64/exception.S @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `save_registers`/`restore_registers`, the four EL1h
//! handlers and `exception_vectors`. `do_ast`, `handle_el0_*` and the trampoline `return`
//! (`CI_TRAMPOLINE_VECTORS`, `tramp_return`) come with user mode (M6); their vector slots
//! are `vempty` until then.
//!
//! ## Deviations
//! - `x18` is a general register here (the C builds with `-ffixed-x18` and keeps `curcpu()`
//!   in it): the EL1 paths of the macros save the pre-exception `sp` with an `add` instead of
//!   parking it in `x18` first, restore `x18` from the frame, and recover `sp` from the
//!   frame's size instead of `mov sp, x18`. The EL0 paths are the C's.

use core::arch::global_asm;
use core::mem::offset_of;

use crate::arch::arm64::include::frame::{TF_SIZE, Trapframe};

global_asm!(
    include_str!("exception.S"),
    TF_SIZE = const TF_SIZE,
    TF_X = const offset_of!(Trapframe, tf_x),
    TF_ELR = const offset_of!(Trapframe, tf_elr),
    TF_SP = const offset_of!(Trapframe, tf_sp),
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
