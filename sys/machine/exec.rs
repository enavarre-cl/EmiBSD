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
//! `<machine/exec.h>` as a trait: what the ELF loader needs to know about the architecture's
//! executables.
//!
//! Milestone M6 (part b) needs the ELF target class, data encoding and machine type
//! `elf_check_header` compares against, the page size the linker assumes (`__LDPGSZ`) and the
//! ELF size (`ARCH_ELFSIZE`, 64 on both targets, so `Elf_Ehdr` is `Elf64_Ehdr`).

/// The executable format parameters of the selected architecture.
pub trait MachineExec {
    /// `__LDPGSZ`: the page size `ld(1)` lays segments out with.
    const LDPGSZ: usize;
    /// `ARCH_ELFSIZE`: 32 or 64.
    const ARCH_ELFSIZE: usize;
    /// `ELF_TARG_CLASS`: `ELFCLASS32` or `ELFCLASS64`.
    const ELF_TARG_CLASS: u8;
    /// `ELF_TARG_DATA`: `ELFDATA2LSB` or `ELFDATA2MSB`.
    const ELF_TARG_DATA: u8;
    /// `ELF_TARG_MACH`: the `EM_*` value of this architecture.
    const ELF_TARG_MACH: u16;
    /// `__HAVE_CPU_HWCAP`: the machine sets `hwcap` (`exec_elf.c`'s global) and
    /// `exec_elf_fixup` passes it as `AUX_hwcap`.
    const HAVE_CPU_HWCAP: bool;
    /// `__HAVE_CPU_HWCAP2`: the same for `hwcap2` and `AUX_hwcap2`.
    const HAVE_CPU_HWCAP2: bool;
}
/* </CODE> */
