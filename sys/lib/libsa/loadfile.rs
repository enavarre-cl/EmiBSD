/* <LICENSES> */
/*-
 * Copyright (c) 1997 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center and by Christos Zoulas.
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Ralph Campbell.
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
 *
 *	@(#)boot.c	8.1 (Berkeley) 6/10/93
 */

/*-
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Christos Zoulas.
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * Array indices in the u_long position array
 */
/* </LICENSES> */

/* <CODE> */
//! `loadfile()`: open a kernel, recognise its ELF class and load it (see `loadfile_elf.rs`),
//! leaving the positions of what it loaded in `marks`.
//!
//! Upstream: sys/lib/libsa/loadfile.c @ 3ce1f3f79392, sys/lib/libsa/loadfile.h @ 3ce1f3f79392
//!
//! Compiled as on amd64 (`<machine/loadfile_machdep.h>`): `BOOT_ELF`, `BOOT_ELF32` and
//! `BOOT_ELF64`, so a 32-bit and a 64-bit ELF loader are both built and the header's class
//! picks one; `LOADADDR` is the program's ([`SaConf::loadaddr`](crate::stand::SaConf)),
//! `WARN` prints the message and `strerror(errno)`, `PROGRESS` prints.
//!
//! ## Deviations
//! - The descriptor is an `Ok`, failure an `Err` with the error number (the C's -1, `errno`
//!   also set). `marks` is a `[u64; MARK_MAX]`.
//! - `loadfile` is an `unsafe fn`: it writes the kernel wherever the program's `LOADADDR`
//!   says, which only the program can vouch for (the C has the same contract, unwritten).

use crate::cread::{close, open, read};
use crate::dev::{errno, set_errno};
use crate::hdr::exec_elf::{
    EI_CLASS, ELFCLASS32, ELFCLASS64, ELFMAG, Elf32Ehdr, Elf64Ehdr, SELFMAG,
};
use crate::loadfile_elf::{elf32_exec, elf64_exec};
use crate::printf;
use crate::printf::Str;
use crate::saerrno::Errno;
use crate::stand::O_RDONLY;
use crate::strerror::strerror;

/// `MARK_START`: where the image starts.
pub const MARK_START: usize = 0;
/// `MARK_ENTRY`: the entry point.
pub const MARK_ENTRY: usize = 1;
/// `MARK_NSYM`.
pub const MARK_NSYM: usize = 2;
/// `MARK_SYM`: where the symbols are.
pub const MARK_SYM: usize = 3;
/// `MARK_END`: where the image ends.
pub const MARK_END: usize = 4;
/// `MARK_RANDOM`: the random data segment.
pub const MARK_RANDOM: usize = 5;
/// `MARK_ERANDOM`: its end.
pub const MARK_ERANDOM: usize = 6;
/// `MARK_VENTRY`: the virtual entry point.
pub const MARK_VENTRY: usize = 7;
/// `MARK_MAX`.
pub const MARK_MAX: usize = 8;

/// `LOAD_TEXT`.
pub const LOAD_TEXT: i32 = 0x0001;
/// `LOAD_DATA`.
pub const LOAD_DATA: i32 = 0x0004;
/// `LOAD_BSS`.
pub const LOAD_BSS: i32 = 0x0008;
/// `LOAD_SYM`.
pub const LOAD_SYM: i32 = 0x0010;
/// `LOAD_HDR`.
pub const LOAD_HDR: i32 = 0x0020;
/// `LOAD_RANDOM`.
pub const LOAD_RANDOM: i32 = 0x0040;
/// `LOAD_ALL`.
pub const LOAD_ALL: i32 = 0x007d;
/// `COUNT_TEXT`.
pub const COUNT_TEXT: i32 = 0x0100;
/// `COUNT_DATA`.
pub const COUNT_DATA: i32 = 0x0400;
/// `COUNT_BSS`.
pub const COUNT_BSS: i32 = 0x0800;
/// `COUNT_SYM`.
pub const COUNT_SYM: i32 = 0x1000;
/// `COUNT_HDR`.
pub const COUNT_HDR: i32 = 0x2000;
/// `COUNT_RANDOM`.
pub const COUNT_RANDOM: i32 = 0x4000;
/// `COUNT_ALL`.
pub const COUNT_ALL: i32 = 0x7d00;

/// `WARN((fmt, ...))` of `<machine/loadfile_machdep.h>`: the message, then `: strerror(errno)`
/// if `errno` is set.
pub fn warn(args: core::fmt::Arguments<'_>) {
    printf::vprintf(args);
    let e = errno();
    if e != Errno(0) {
        printf!(": {}\n", strerror(e));
    } else {
        printf!("\n");
    }
}

/// `loadfile(fname, marks, flags)`: open `fname`, read in the program and leave it open; its
/// descriptor. `marks[MARK_START]` is the offset `LOADADDR` adds.
///
/// # Safety
///
/// The program's `LOADADDR` must map every address of the kernel (and of its symbols, after
/// it) to memory the program reserved for it and nothing else uses.
pub unsafe fn loadfile(
    fname: &[u8],
    marks: &mut [u64; MARK_MAX],
    flags: i32,
) -> Result<usize, Errno> {
    // Open the file.
    let fd = match open(fname, O_RDONLY) {
        Ok(fd) => fd,
        Err(e) => {
            warn(format_args!("open {}", Str(fname)));
            return Err(e);
        }
    };

    // Read the exec header.
    let mut hdr = [0u8; core::mem::size_of::<Elf64Ehdr>()];
    match read(fd, &mut hdr) {
        Ok(n) if n == hdr.len() => {}
        _ => {
            warn(format_args!("read header"));
            let _ = close(fd);
            return Err(errno());
        }
    }

    let rval = if hdr[..SELFMAG] == ELFMAG[..] && hdr[EI_CLASS] == ELFCLASS32 {
        let mut elf = Elf32Ehdr::from_bytes(&hdr).unwrap_or_default();
        // SAFETY: the caller's contract on LOADADDR.
        unsafe { elf32_exec(fd, &mut elf, marks, flags) }
    } else if hdr[..SELFMAG] == ELFMAG[..] && hdr[EI_CLASS] == ELFCLASS64 {
        let mut elf = Elf64Ehdr::from_bytes(&hdr).unwrap_or_default();
        // SAFETY: the caller's contract on LOADADDR.
        unsafe { elf64_exec(fd, &mut elf, marks, flags) }
    } else {
        set_errno(Errno::EFTYPE);
        warn(format_args!("{}", Str(fname)));
        Err(Errno::EFTYPE)
    };

    match rval {
        Ok(()) => {
            printf!("={:#x}\n", marks[MARK_END].wrapping_sub(marks[MARK_START]));
            Ok(fd)
        }
        Err(e) => {
            let _ = close(fd);
            Err(e)
        }
    }
}
/* </CODE> */
