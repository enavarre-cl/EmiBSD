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
//! arm64 machine-dependent sources: OpenBSD `sys/arch/arm64/arm64/*.c` and `*.S`.
//!
//! `machdep` (boot, the early init, `consinit`), `exception` (the vector table) and `trap`,
//! `intr` (`delay`, the IRQ/FIQ entry), `bus_space`, `db_trace` and `db_interface`
//! (ddb-lite) are partial ports; `qemu` is the emulator exit under feature `qemu`, a project
//! helper (`ports.toml`, `[[extra]]`). `bus_dma` (M7b) is a whole port.

pub mod acpi_machdep;
pub mod ast;
pub mod autoconf;
pub mod bus_dma;
pub mod bus_space;
pub mod conf;
pub mod copy;
pub mod copystr;
pub mod cpu;
pub mod cpufunc;
pub mod cpuswitch;
pub mod db_interface;
pub mod db_trace;
pub mod disksubr;
pub mod exception;
pub mod fpu;
pub mod intr;
pub mod locore;
pub mod locore0;
pub mod machdep;
pub mod mem;
pub mod pmap;
#[cfg(feature = "qemu")]
pub mod qemu;
pub mod sig_machdep;
pub mod syscall;
pub mod trap;
pub mod vm_machdep;
/* </CODE> */
