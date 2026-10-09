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

/*-
 * Copyright (c) 2013 Andrew Turner <andrew@freebsd.org>
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * $FreeBSD: head/sys/arm64/include/machdep.h 281494 2015-04-13 14:43:10Z andrew $
 */
/* </LICENSES> */

/* <CODE> */
//! arm64 `<machine/bootconfig.h>`: what `locore0.S` hands the kernel's first C.
//!
//! Upstream: sys/arch/arm64/include/bootconfig.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `initarm(struct arm64_bootparams *)` is not declared here: the C's `locore0.S` calls
//!   `initarm` with the parameters, here it calls the boot glue's `bootarg_main`
//!   (`sys/stand/bootarg.rs`), which hands them to `machdep.rs`'s `getbootinfo` and runs
//!   `initarm` (with a `BootInfo`) like the Limine entry does.

use crate::sys::types::Vaddr;

/// `struct arm64_bootparams`: `locore0.S` builds it on the boot stack.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Arm64Bootparams {
    /// `modulep`: FreeBSD's module pointer, unused (whatever `x0` held).
    pub modulep: Vaddr,
    /// `kern_l1pt`: the kernel's first page table (`locore0.S`'s level-0 `TTBR1_EL1` table
    /// here; see its deviations).
    pub kern_l1pt: Vaddr,
    /// `kern_delta`: physical minus virtual address of the kernel image.
    pub kern_delta: u64,
    /// `kern_stack`: the bottom of the boot stack (`initstack`).
    pub kern_stack: Vaddr,
    /// `arg0`: `x0` at entry: efiboot's end of the loaded symbols (`esym`), made virtual.
    pub arg0: usize,
    /// `arg1`: `x1` at entry (efiboot passes 0).
    pub arg1: usize,
    /// `arg2`: `x2` at entry: the physical address of the flattened device tree.
    pub arg2: usize,
}

const _: () = {
    assert!(core::mem::size_of::<Arm64Bootparams>() == 56);
};
/* </CODE> */
