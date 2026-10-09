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
//! Header ports: OpenBSD `sys/arch/amd64/include/*.h`.
//!
//! Constants, `#[repr(C)]` hardware structs and inline accessors only; never state.

pub mod _types;
pub mod apicvar;
pub mod biosvar;
pub mod bus;
pub mod cpu;
pub mod cpu_full;
pub mod cpufunc;
pub mod cpuvar;
pub mod db_machdep;
pub mod disklabel;
pub mod efifbvar;
pub mod exec;
pub mod fpu;
pub mod frame;
pub mod i82093reg;
pub mod i82093var;
pub mod i82489reg;
pub mod i82489var;
pub mod i8259;
pub mod intr;
pub mod intrdefs;
pub mod ioctl_fd;
pub mod isa_machdep;
pub mod mpbiosreg;
pub mod mpbiosvar;
pub mod mpconfig;
pub mod mplock;
pub mod mutex;
pub mod param;
pub mod pcb;
pub mod pci_machdep;
pub mod pic;
pub mod pio;
pub mod pmap;
pub mod proc;
pub mod psl;
pub mod pte;
pub mod segments;
pub mod signal;
pub mod smbiosvar;
pub mod specialreg;
pub mod tcb;
pub mod timetc;
pub mod trap;
pub mod tss;
pub mod vmparam;
/* </CODE> */
