/*	$OpenBSD: exec.c,v 1.8 2020/05/10 11:55:42 kettenis Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2006, 2016 Mark Kettenis
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
//! `run_loadfile()`: hand the loaded kernel its arguments in the device tree's `/chosen`,
//! leave the boot services, clean the caches over the kernel and the tree, and enter the
//! kernel at its entry point with `x0` = the end of the loaded image (`marks[MARK_END]`),
//! `x1` = 0, `x2` = the device tree.
//!
//! Upstream: sys/arch/arm64/stand/efiboot/exec.c @ 3ce1f3f79392
//!
//! The kernel was loaded by libsa's `loadfile` at `LOADADDR(a)` =
//! `((a + offset) & 0x7fffffffff) + efi_loadaddr` (arm64's `<machine/loadfile_machdep.h>`,
//! `conf.rs`): its image in the 64 MB block `machdep()` allocated at a 2 MB boundary,
//! entered where it lies, with the MMU and caches as the firmware left them.
//!
//! ## Deviations
//! - `run_loadfile` takes boot(8)'s command state (`cmd.path`, the C's global `cmd`).
//! - `CTR_DLINE_SIZE` (arm64's `<machine/armreg.h>`) is declared here: the kernel's header
//!   is not a dependency of the boot loader.
//! - The host build (`cargo test`) has no cache instructions: the `asm!` statements are
//!   compiled for the target only (nothing on the host calls `run_loadfile`).

use boot::cmd::{CmdState, cstr};
use libsa::hdr::reboot::{RB_ASKNAME, RB_CONFIG, RB_KDB, RB_SINGLE};
use libsa::loadfile::{MARK_END, MARK_ENTRY, MARK_MAX};

use crate::efiboot::{efi_cleanup, efi_makebootargs};
use crate::fdt::fdt_get_size;

/// `CTR_DLINE_SHIFT`, `CTR_DLINE_MASK` (`<machine/armreg.h>`).
const CTR_DLINE_SHIFT: u64 = 16;
const CTR_DLINE_MASK: u64 = 0xf << CTR_DLINE_SHIFT;

/// `startfuncp`: the kernel's entry, which does not return.
type Startfuncp = unsafe extern "C" fn(*mut u8, *mut u8, *mut u8) -> !;

/// `cpu_get_dcache_line_size()`: the smallest data cache line, in bytes.
fn cpu_get_dcache_line_size() -> u64 {
    let ctr: u64;

    // Accessible from all security levels
    #[cfg(not(test))]
    // SAFETY: CTR_EL0 is readable at EL1 and EL2, where UEFI runs us; no side effects.
    unsafe {
        core::arch::asm!("mrs {}, ctr_el0", out(reg) ctr, options(nomem, nostack, preserves_flags));
    }
    #[cfg(test)]
    {
        ctr = 4 << CTR_DLINE_SHIFT;
    }

    // Relevant field [19:16] is LOG2 of the number of words in DCache line
    let dcl_size = (ctr & CTR_DLINE_MASK) >> CTR_DLINE_SHIFT;

    // Size of word shifted by cache line size
    (core::mem::size_of::<i32>() as u64) << dcl_size
}

/// `cpu_flush_dcache(addr, len)`: clean and invalidate the data cache over `addr..+len`.
fn cpu_flush_dcache(addr: u64, len: u64) {
    let cl_size = cpu_get_dcache_line_size();

    // Calculate end address to clean
    let end = addr + len;
    // Align start address to cache line
    let mut addr = addr & !(cl_size - 1);

    while addr < end {
        #[cfg(not(test))]
        // SAFETY: a cache maintenance operation by address; it changes no memory contents.
        unsafe {
            core::arch::asm!("dc civac, {}", in(reg) addr, options(nostack, preserves_flags));
        }
        addr += cl_size;
    }

    // Full system DSB
    #[cfg(not(test))]
    // SAFETY: a barrier.
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
}

/// `cpu_inval_icache()`: invalidate the instruction caches.
fn cpu_inval_icache() {
    #[cfg(not(test))]
    // SAFETY: cache maintenance and a barrier; no memory contents change.
    unsafe {
        core::arch::asm!("ic ialluis", "dsb ish", options(nostack, preserves_flags));
    }
}

/// `run_loadfile(marks, howto)`.
pub fn run_loadfile(cmd: &mut CmdState, marks: &mut [u64; MARK_MAX], howto: i32) {
    let mut args = [0u8; 256];

    let path = cstr(&cmd.path);
    let mut cp = path.len().min(args.len() - 8);
    args[..cp].copy_from_slice(&path[..cp]);

    args[cp] = b' ';
    cp += 1;
    args[cp] = b'-';
    for (flag, c) in [
        (RB_ASKNAME, b'a'),
        (RB_CONFIG, b'c'),
        (RB_SINGLE, b's'),
        (RB_KDB, b'd'),
    ] {
        if (howto & flag) != 0 {
            cp += 1;
            args[cp] = c;
        }
    }
    if args[cp] == b'-' {
        cp -= 1;
        args[cp] = 0;
    } else {
        cp += 1;
        args[cp] = 0;
    }

    let fdt = efi_makebootargs(cmd, &args, howto);

    efi_cleanup();

    cpu_flush_dcache(marks[MARK_ENTRY], marks[MARK_END] - marks[MARK_ENTRY]);
    cpu_inval_icache();

    // SAFETY: the tree efi_makebootargs made (or null: size 0).
    cpu_flush_dcache(fdt as u64, unsafe { fdt_get_size(fdt) } as u64);

    // SAFETY: loadfile put the kernel's image where `marks` say and its entry point is
    // `marks[MARK_ENTRY]`; the boot services are gone and the memory is the kernel's. The
    // kernel takes the end of its image, 0 and the device tree, and does not return.
    unsafe {
        let start = core::mem::transmute::<usize, Startfuncp>(marks[MARK_ENTRY] as usize);
        start(
            marks[MARK_END] as usize as *mut u8,
            core::ptr::null_mut(),
            fdt,
        );
    }
}
/* </CODE> */
