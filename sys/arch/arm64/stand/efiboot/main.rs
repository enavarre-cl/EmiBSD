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
//! BOOTAA64.EFI: arm64's UEFI boot loader, OpenBSD's efiboot (`sys/arch/arm64/stand/efiboot`).
//!
//! The firmware enters `_start` (`start.S`), which clears the bss, relocates the image
//! (`self_reloc.rs`) and calls `efi_main` (`efiboot.rs`); that registers efiboot's tables
//! with libsa and boot(8) (`conf.rs`), finds the firmware's device tree and SMBIOS tables,
//! and runs boot(8) (`boot::boot::boot`): `machdep()` (the console, the heap, the kernel's
//! 64 MB, the disks, the network), `/etc/boot.conf`, the `boot>` prompt, the kernel's load
//! (libsa's `loadfile`) and its start (`exec.rs`) with the device tree `efi_makebootargs`
//! filled in (`fdt.rs`; made from the ACPI tables by `efiacpi.rs` when there is none).
//!
//! The ELF is linked position-independent at 0 (`ldscript.arm64`) and made a PE32+ image by
//! `llvm-objcopy -O binary` (`just efiboot-arm64`, docs/ARCHITECTURE.md, "Boot loaders"),
//! as OpenBSD makes it with its own `.peheader`. On the host (`cargo test`) the crate
//! builds without its entry code, for the unit tests.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

mod conf;
mod disk;
mod dt_blob;
mod efiacpi;
mod efiboot;
mod efidev;
mod efipxe;
mod efirng;
mod exec;
mod fdt;
mod heap;
mod self_reloc;
mod smbios;
#[cfg(not(test))]
mod start;

/// libsa's `alloc()`, which `Box` and `Vec` go through too.
#[cfg(not(test))]
#[global_allocator]
static ALLOCATOR: libsa::sa_alloc::SaAlloc = libsa::sa_alloc::SaAlloc;

/// A Rust panic is libsa's `panic()`: close the files, print, reset.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    libsa::exit::panic(format_args!("{}", info.message()))
}
/* </CODE> */
