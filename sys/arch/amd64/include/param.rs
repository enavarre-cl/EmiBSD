/*	$OpenBSD: param.h,v 1.30 2025/07/07 18:33:36 kettenis Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/param.h>`: machine type and page geometry.
//!
//! Upstream: sys/arch/amd64/include/param.h @ 3ce1f3f79392
//!
//! The constants are exported to generic code through the [`MachineParam`] implementation at the
//! bottom, re-exported by `sys::param`.
//!
//! ## Deviations
//! - `_MACHINE` and `_MACHINE_ARCH` (unquoted tokens for the preprocessor) have no counterpart.
//! - `MID_MACHINE` (`MID_AMD64`) arrives with `sys/sys/exec.h`.
//! - The `<machine/cpu.h>` / `<machine/psl.h>` includes are not reproduced; those headers are
//!   modules of their own when ported.

use super::_types::{_ALIGNBYTES, _MAX_PAGE_SHIFT, _STACKALIGNBYTES, _aligned_pointer};
use crate::machine::MachineParam;

/// `MACHINE`: the OpenBSD machine name.
pub const MACHINE: &str = "amd64";
/// `MACHINE_ARCH`: the CPU architecture name.
pub const MACHINE_ARCH: &str = "amd64";

/// log2 of the page size.
pub const PAGE_SHIFT: usize = 12;
/// Bytes per page.
pub const PAGE_SIZE: usize = 1 << PAGE_SHIFT;
/// Byte offset mask within a page.
pub const PAGE_MASK: usize = PAGE_SIZE - 1;

/// Start of kernel virtual space.
pub const KERNBASE: usize = 0xffff_ffff_8000_0000;
/// Start of kernel text.
pub const KERNTEXTOFF: usize = KERNBASE + 0x0100_0000;

/// Bytes per page.
pub const NBPG: usize = PAGE_SIZE;
/// LOG2(PAGE_SIZE).
pub const PGSHIFT: usize = PAGE_SHIFT;
/// Byte offset into page.
pub const PGOFSET: usize = PAGE_MASK;

/// Pages of u-area.
pub const UPAGES: usize = 6;
/// Total size of the u-area.
pub const USPACE: usize = UPAGES * PAGE_SIZE;
/// u-area alignment, 0 for none.
pub const USPACE_ALIGN: usize = 0;

/// Max cluster allocation.
pub const NMBCLUSTERS: usize = 256 * 1024;

/// Default message buffer size.
pub const MSGBUFSIZE: usize = 32 * PAGE_SIZE;

impl MachineParam for super::super::Machine {
    const PAGE_SHIFT: usize = PAGE_SHIFT;
    const PAGE_SIZE: usize = PAGE_SIZE;
    const PAGE_MASK: usize = PAGE_MASK;
    const KERNBASE: usize = KERNBASE;
    const UPAGES: usize = UPAGES;
    const USPACE: usize = USPACE;
    const USPACE_ALIGN: usize = USPACE_ALIGN;
    const HAVE_USPACE_GUARD: bool = true;
    const NMBCLUSTERS: usize = NMBCLUSTERS;
    const MSGBUFSIZE: usize = MSGBUFSIZE;
    const HAVE_ACPI: bool = true;
    const HAVE_FDT: bool = false;
    const ALIGNBYTES: usize = _ALIGNBYTES;
    const STACKALIGNBYTES: usize = _STACKALIGNBYTES;
    const MAX_PAGE_SHIFT: usize = _MAX_PAGE_SHIFT;
    const STRICT_ALIGNMENT: bool = false;

    fn aligned_pointer<T>(p: usize) -> bool {
        _aligned_pointer::<T>(p)
    }
}
/* </CODE> */
