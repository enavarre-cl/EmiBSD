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
//! BOOTX64.EFI: amd64's UEFI boot loader, OpenBSD's efiboot (`sys/arch/amd64/stand/efiboot`).
//!
//! The firmware enters `_start` (`start_amd64.S`), which relocates the image
//! (`self_reloc.rs`) and calls `efi_main` (`efiboot.rs`); that registers efiboot's tables
//! with libsa and boot(8) (`conf.rs`), sets up the console and the heap, and runs boot(8)
//! (`boot::boot::boot`): the probes (`machdep.rs`), `/etc/boot.conf`, the `boot>` prompt,
//! the kernel's load (libsa's `loadfile`) and its start (`exec_i386.rs`, `run_i386.S`).
//! The files of `sys/arch/amd64/stand/libsa` efiboot compiles (`.PATH: ${SADIR}/libsa`) and
//! `<machine/biosvar.h>` are modules here too, by path.
//!
//! The ELF is linked position-independent at 0 (`ldscript.amd64`) and made a PE32+ image by
//! `llvm-objcopy -O binary` (`just efiboot-amd64`, docs/ARCHITECTURE.md, "Boot loaders").

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../../include/biosvar.rs"]
mod biosvar;
mod cmd_i386;
mod conf;
mod dev_i386;
#[path = "../libsa/disk.rs"]
mod disk;
mod diskprobe;
mod efiboot;
mod efidev;
mod efipxe;
mod efirng;
mod exec_i386;
mod heap;
#[path = "../libsa/libsa.rs"]
mod libsa_md;
mod machdep;
#[path = "../libsa/mdrandom.rs"]
mod mdrandom;
mod memprobe;
mod run_i386;
mod self_reloc;
mod start_amd64;

use core::panic::PanicInfo;

/// libsa's `alloc()`, which `Box` and `Vec` go through too.
#[global_allocator]
static ALLOCATOR: libsa::sa_alloc::SaAlloc = libsa::sa_alloc::SaAlloc;

/// A Rust panic is libsa's `panic()`: close the files, print, reset.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    libsa::exit::panic(format_args!("{}", info.message()))
}
/* </CODE> */
