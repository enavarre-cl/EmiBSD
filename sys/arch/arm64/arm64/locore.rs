//! The signal trampoline of `arch/arm64/arm64/locore.S`, pulled in from the `.S` file next to
//! this module (the file keeps OpenBSD's licence block and layout; `{NAME}` placeholders are
//! what `assym.h` and `<sys/syscall.h>` provide in C).
//!
//! Upstream: sys/arch/arm64/arm64/locore.S @ 3ce1f3f79392
//!
//! Status: `wip`. `kern_sig.c` brings `sigcode` (with `sigcodecall`, `sigcoderet`,
//! `esigcode`, `sigfill` and `sigfillsiz`), which `exec_sigcode_map` copies into every
//! process. The rest of the file is the kernel entry, which the boot protocol replaces
//! (`docs/ARCHITECTURE.md`, "Boot flow"): `drop_to_el1`, the boot page tables and
//! `initstack` have no counterpart here.
//!
//! ## Deviations
//! - The trampoline is assembled inside the softfloat kernel: `.arch_extension fp` enables
//!   the `q` register moves for it alone, as the C file does, and `.arch_extension nofp`
//!   switches them off again.

use core::arch::global_asm;
use core::mem::offset_of;
use core::ptr;

use crate::arch::arm64::include::frame::Sigframe;
use crate::sys::syscall::SYS_sigreturn;

global_asm!(
    include_str!("locore.S"),
    SF_SC = const offset_of!(Sigframe, sf_sc),
    SYS_SIGRETURN = const SYS_sigreturn,
);

unsafe extern "C" {
    /// `sigcode[]`: the signal trampoline, copied into every process (`exec_sigcode_map`).
    static sigcode: [u8; 0];
    /// `sigcoderet[]`: the instruction after the trampoline's `sigreturn` system call (and
    /// the speculation barrier `svc_handler` skips).
    static sigcoderet: [u8; 0];
    /// `esigcode[]`: the end of the trampoline.
    static esigcode: [u8; 0];
    /// `sigfill[]`: the trap instruction the rest of the trampoline's page is filled with.
    static sigfill: [u8; 0];
    /// `sigfillsiz`: the size of `sigfill`.
    static sigfillsiz: i32;
}

/// `sigcode` .. `esigcode`: the signal trampoline's bytes.
pub fn sigcode_bytes() -> &'static [u8] {
    let start = ptr::addr_of!(sigcode).cast::<u8>();
    let end = ptr::addr_of!(esigcode).cast::<u8>();
    // SAFETY: `sigcode` and `esigcode` bracket the trampoline in `.text` (`locore.S`),
    // never written and alive for the kernel's lifetime.
    unsafe { core::slice::from_raw_parts(start, end as usize - start as usize) }
}

/// `sigcoderet - sigcode`.
pub fn sigcoderet_offset() -> usize {
    ptr::addr_of!(sigcoderet) as usize - ptr::addr_of!(sigcode) as usize
}

/// `sigfill` .. `sigfill + sigfillsiz`.
pub fn sigfill_bytes() -> &'static [u8] {
    // SAFETY: `sigfillsiz` is a word in `.data` that the assembler initialised and nothing
    // writes.
    let len = unsafe { ptr::addr_of!(sigfillsiz).read() } as usize;
    // SAFETY: `sigfill` is followed by `sigfillsiz` bytes of instructions in `.text`.
    unsafe { core::slice::from_raw_parts(ptr::addr_of!(sigfill).cast::<u8>(), len) }
}
