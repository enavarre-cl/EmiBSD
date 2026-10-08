/* $OpenBSD: exec.h,v 1.3 2025/07/31 16:09:59 kettenis Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2014 Patrick Wildt <patrick@blueri.se>
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
//! arm64 `<machine/exec.h>`: the ELF target parameters.
//!
//! Upstream: sys/arch/arm64/include/exec.h @ 3ce1f3f79392
//!
//! Status: `ported` (M6). The values are exposed to generic code through
//! `machine::exec::MachineExec`; `__HAVE_COREDUMP_NOTE_ELF_MD` waits for core dumps (M7).

use crate::sys::exec_elf::{ELFCLASS64, ELFDATA2LSB, EM_AARCH64};

/// `__LDPGSZ`.
pub const LDPGSZ: usize = 4096;
/// `ARCH_ELFSIZE`.
pub const ARCH_ELFSIZE: usize = 64;
/// `ELF_TARG_CLASS`.
pub const ELF_TARG_CLASS: u8 = ELFCLASS64;
/// `ELF_TARG_DATA`.
pub const ELF_TARG_DATA: u8 = ELFDATA2LSB;
/// `ELF_TARG_MACH`.
pub const ELF_TARG_MACH: u16 = EM_AARCH64;
/* </CODE> */
