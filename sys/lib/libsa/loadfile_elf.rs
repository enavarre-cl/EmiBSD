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

/*
 * Copyright (c) 2007 Tom Cosgrove <tom@openbsd.org>
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

/*
 * Copyright (c) 2007 Tom Cosgrove <tom@openbsd.org>
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
//! The ELF loader `loadfile()` calls: load the `PT_LOAD` segments, zero their bss, fill the
//! `PT_OPENBSD_RANDOMIZE` segment, and copy the ELF header, the section headers and the
//! symbol and string tables (and `.debug_line`, `.SUNW_ctf`) after the image for ddb.
//!
//! Upstream: sys/lib/libsa/loadfile_elf.c @ 3ce1f3f79392; its instances
//! sys/arch/amd64/stand/libsa/elf32.c @ 3ce1f3f79392 and
//! sys/arch/amd64/stand/libsa/elf64.c @ 3ce1f3f79392
//!
//! The C is compiled twice, as `elf32_exec` (`ELFSIZE` 32, `elf32.c`) and `elf64_exec`
//! (`ELFSIZE` 64, `elf64.c`), each including this file with `ELFNAME`, `Elf_Ehdr` and friends
//! defined for its class. Here it is one generic function, [`elf_exec`], over the
//! [`ElfClass`] trait, and the two instances are [`elf32_exec`] and [`elf64_exec`].
//!
//! ## Deviations
//! - `marks` is a `[u64; MARK_MAX]`; failure is an `Err` (the C's 1).
//! - `READ`, `BCOPY` and `BZERO` go through [`load_mem`], a slice over the memory
//!   `LOADADDR` names; `ALLOC`/`FREE` are `Vec`s.
//! - `CHECK_PHDR` is not defined on amd64 and has no counterpart.

use alloc::vec;
use alloc::vec::Vec;

use crate::arc4::{RANDOMCTX, rc4_getbytes};
use crate::cread::{lseek, read};
use crate::hdr::exec_elf::{
    ELF_CTF, Elf32Ehdr, Elf32Phdr, Elf32Shdr, Elf64Ehdr, Elf64Phdr, Elf64Shdr, PF_R, PF_W, PF_X,
    PT_LOAD, PT_OPENBSD_RANDOMIZE, SHF_ALLOC, SHT_STRTAB, SHT_SYMTAB,
};
use crate::hdr::param::roundup;
use crate::loadfile::{
    COUNT_BSS, COUNT_HDR, COUNT_RANDOM, COUNT_SYM, COUNT_TEXT, LOAD_BSS, LOAD_DATA, LOAD_HDR,
    LOAD_RANDOM, LOAD_SYM, LOAD_TEXT, MARK_END, MARK_ENTRY, MARK_ERANDOM, MARK_MAX, MARK_NSYM,
    MARK_RANDOM, MARK_START, MARK_SYM, MARK_VENTRY, warn,
};
use crate::printf;
use crate::saerrno::Errno;
use crate::stand::{SEEK_SET, sa_conf};

/// What `ELFSIZE` selects: the class's header types, read as 64-bit values.
pub trait ElfClass {
    /// `Elf_Ehdr`.
    type Ehdr;
    /// `Elf_Phdr`.
    type Phdr: Copy;
    /// `Elf_Shdr`.
    type Shdr: Copy;
    /// `sizeof(Elf_Addr)`.
    const ADDR_SIZE: u64;
    /// `sizeof(Elf_Ehdr)`, `sizeof(Elf_Phdr)`, `sizeof(Elf_Shdr)`.
    const SIZES: (usize, usize, usize);
    /// `e_phoff`, `e_phnum`, `e_shoff`, `e_shnum`, `e_shstrndx`, `e_entry`.
    fn ehdr(e: &Self::Ehdr) -> (u64, usize, u64, usize, usize, u64);
    /// The header with `e_phoff = 0`, `e_shoff = sizeof(Elf_Ehdr)`, `e_phentsize = 0`,
    /// `e_phnum = 0` (the copy the kernel gets), as bytes.
    fn frob_ehdr(e: &mut Self::Ehdr) -> Vec<u8>;
    /// A program header from its bytes.
    fn phdr(b: &[u8]) -> Self::Phdr;
    /// `p_type`, `p_flags`, `p_offset`, `p_paddr`, `p_filesz`, `p_memsz`.
    fn phdr_fields(p: &Self::Phdr) -> (u32, u32, u64, u64, u64, u64);
    /// A section header from its bytes.
    fn shdr(b: &[u8]) -> Self::Shdr;
    /// `sh_name`, `sh_type`, `sh_offset`, `sh_size`.
    fn shdr_fields(s: &Self::Shdr) -> (u32, u32, u64, u64);
    /// Sets `sh_offset` and adds `SHF_ALLOC` to `sh_flags`.
    fn shdr_relocate(s: &mut Self::Shdr, off: u64);
    /// The section header's bytes.
    fn shdr_bytes(s: &Self::Shdr) -> Vec<u8>;
}

/// `ELFSIZE` 32.
pub struct Elf32;

/// `ELFSIZE` 64.
pub struct Elf64;

impl ElfClass for Elf32 {
    type Ehdr = Elf32Ehdr;
    type Phdr = Elf32Phdr;
    type Shdr = Elf32Shdr;
    const ADDR_SIZE: u64 = 4;
    const SIZES: (usize, usize, usize) = (52, 32, 40);

    fn ehdr(e: &Elf32Ehdr) -> (u64, usize, u64, usize, usize, u64) {
        (
            u64::from(e.e_phoff),
            usize::from(e.e_phnum),
            u64::from(e.e_shoff),
            usize::from(e.e_shnum),
            usize::from(e.e_shstrndx),
            u64::from(e.e_entry),
        )
    }

    fn frob_ehdr(e: &mut Elf32Ehdr) -> Vec<u8> {
        e.e_phoff = 0;
        e.e_shoff = Self::SIZES.0 as u32;
        e.e_phentsize = 0;
        e.e_phnum = 0;
        e.as_bytes().to_vec()
    }

    fn phdr(b: &[u8]) -> Elf32Phdr {
        Elf32Phdr::from_bytes(b).unwrap_or_default()
    }

    fn phdr_fields(p: &Elf32Phdr) -> (u32, u32, u64, u64, u64, u64) {
        (
            p.p_type,
            p.p_flags,
            u64::from(p.p_offset),
            u64::from(p.p_paddr),
            u64::from(p.p_filesz),
            u64::from(p.p_memsz),
        )
    }

    fn shdr(b: &[u8]) -> Elf32Shdr {
        Elf32Shdr::from_bytes(b).unwrap_or_default()
    }

    fn shdr_fields(s: &Elf32Shdr) -> (u32, u32, u64, u64) {
        (
            s.sh_name,
            s.sh_type,
            u64::from(s.sh_offset),
            u64::from(s.sh_size),
        )
    }

    fn shdr_relocate(s: &mut Elf32Shdr, off: u64) {
        s.sh_offset = off as u32;
        s.sh_flags |= SHF_ALLOC as u32;
    }

    fn shdr_bytes(s: &Elf32Shdr) -> Vec<u8> {
        s.as_bytes().to_vec()
    }
}

impl ElfClass for Elf64 {
    type Ehdr = Elf64Ehdr;
    type Phdr = Elf64Phdr;
    type Shdr = Elf64Shdr;
    const ADDR_SIZE: u64 = 8;
    const SIZES: (usize, usize, usize) = (64, 56, 64);

    fn ehdr(e: &Elf64Ehdr) -> (u64, usize, u64, usize, usize, u64) {
        (
            e.e_phoff,
            usize::from(e.e_phnum),
            e.e_shoff,
            usize::from(e.e_shnum),
            usize::from(e.e_shstrndx),
            e.e_entry,
        )
    }

    fn frob_ehdr(e: &mut Elf64Ehdr) -> Vec<u8> {
        e.e_phoff = 0;
        e.e_shoff = Self::SIZES.0 as u64;
        e.e_phentsize = 0;
        e.e_phnum = 0;
        e.as_bytes().to_vec()
    }

    fn phdr(b: &[u8]) -> Elf64Phdr {
        Elf64Phdr::from_bytes(b).unwrap_or_default()
    }

    fn phdr_fields(p: &Elf64Phdr) -> (u32, u32, u64, u64, u64, u64) {
        (
            p.p_type, p.p_flags, p.p_offset, p.p_paddr, p.p_filesz, p.p_memsz,
        )
    }

    fn shdr(b: &[u8]) -> Elf64Shdr {
        Elf64Shdr::from_bytes(b).unwrap_or_default()
    }

    fn shdr_fields(s: &Elf64Shdr) -> (u32, u32, u64, u64) {
        (s.sh_name, s.sh_type, s.sh_offset, s.sh_size)
    }

    fn shdr_relocate(s: &mut Elf64Shdr, off: u64) {
        s.sh_offset = off;
        s.sh_flags |= SHF_ALLOC;
    }

    fn shdr_bytes(s: &Elf64Shdr) -> Vec<u8> {
        s.as_bytes().to_vec()
    }
}

/// The `len` bytes of memory `LOADADDR(a)` names, for `offset` (`READ`, `BCOPY`, `BZERO`).
///
/// # Safety
///
/// As for [`loadfile`](crate::loadfile::loadfile): the program reserved that memory.
unsafe fn load_mem(a: u64, offset: u64, len: usize) -> &'static mut [u8] {
    let addr = (sa_conf().loadaddr)(a, offset);
    // SAFETY: the caller's contract: `LOADADDR` maps the kernel's addresses to memory the
    // program reserved for the kernel; nothing else refers to it while loadfile runs.
    unsafe { core::slice::from_raw_parts_mut(addr as usize as *mut u8, len) }
}

/// `ELFNAME(exec)(fd, elf, marks, flags)`: load the ELF file open on `fd`, whose header is
/// `elf`.
///
/// # Safety
///
/// As for [`loadfile`](crate::loadfile::loadfile).
pub unsafe fn elf_exec<E: ElfClass>(
    fd: usize,
    elf: &mut E::Ehdr,
    marks: &mut [u64; MARK_MAX],
    flags: i32,
) -> Result<(), Errno> {
    let (e_phoff, e_phnum, e_shoff, e_shnum, e_shstrndx, e_entry) = E::ehdr(elf);
    let (ehdr_size, phdr_size, shdr_size) = E::SIZES;
    let mut minp: u64 = !0;
    let mut maxp: u64 = 0;
    let mut pos: u64 = 0;
    let offset = marks[MARK_START];

    let sz = e_phnum * phdr_size;
    let mut phbuf = vec![0u8; sz];

    if lseek(fd, e_phoff as i64, SEEK_SET).is_err() {
        warn(format_args!("lseek phdr"));
        return Err(Errno::EIO);
    }
    if read(fd, &mut phbuf) != Ok(sz) {
        warn(format_args!("read program headers"));
        return Err(Errno::EIO);
    }

    let mut first = true;
    for i in 0..e_phnum {
        let ph = E::phdr(&phbuf[i * phdr_size..]);
        let (p_type, p_flags, p_offset, p_paddr, p_filesz, p_memsz) = E::phdr_fields(&ph);

        if p_type == PT_OPENBSD_RANDOMIZE {
            // Fill segment if asked for.
            if (flags & LOAD_RANDOM) != 0 {
                // SAFETY: the caller's contract on LOADADDR; RANDOMCTX is only used here
                // and by boot(8) before it calls loadfile (single-threaded).
                unsafe {
                    rc4_getbytes(
                        RANDOMCTX.get_mut(),
                        load_mem(p_paddr, offset, p_filesz as usize),
                    );
                }
            }
            if (flags & (LOAD_RANDOM | COUNT_RANDOM)) != 0 {
                marks[MARK_RANDOM] = (sa_conf().loadaddr)(p_paddr, offset);
                marks[MARK_ERANDOM] = marks[MARK_RANDOM] + p_filesz;
            }
            continue;
        }

        if p_type != PT_LOAD || (p_flags & (PF_W | PF_R | PF_X)) == 0 {
            continue;
        }

        let is_text = (p_flags & PF_X) != 0;
        let is_data = !is_text;
        let is_bss = p_filesz < p_memsz;

        // XXX: Assume first address is lowest
        if (is_text && (flags & LOAD_TEXT) != 0) || (is_data && (flags & LOAD_DATA) != 0) {
            // Read in segment.
            printf!("{}{}", if first { "" } else { "+" }, p_filesz);

            if lseek(fd, p_offset as i64, SEEK_SET).is_err() {
                warn(format_args!("lseek text"));
                return Err(Errno::EIO);
            }
            // SAFETY: the caller's contract on LOADADDR.
            let dst = unsafe { load_mem(p_paddr, offset, p_filesz as usize) };
            if read(fd, dst) != Ok(p_filesz as usize) {
                warn(format_args!("read text"));
                return Err(Errno::EIO);
            }

            first = false;
        }

        if (is_text && (flags & (LOAD_TEXT | COUNT_TEXT)) != 0)
            || (is_data && (flags & (LOAD_DATA | COUNT_TEXT)) != 0)
        {
            pos = p_paddr;
            if minp > pos {
                minp = pos;
            }
            pos += p_filesz;
            if maxp < pos {
                maxp = pos;
            }
        }

        // Zero out BSS.
        if is_bss && (flags & LOAD_BSS) != 0 {
            printf!("+{}", p_memsz - p_filesz);
            // SAFETY: the caller's contract on LOADADDR.
            unsafe { load_mem(p_paddr + p_filesz, offset, (p_memsz - p_filesz) as usize) }.fill(0);
        }
        if is_bss && (flags & (LOAD_BSS | COUNT_BSS)) != 0 {
            pos += p_memsz - p_filesz;
            if maxp < pos {
                maxp = pos;
            }
        }
    }
    drop(phbuf);

    // Copy the ELF and section headers.
    maxp = roundup(maxp, E::ADDR_SIZE);
    let elfp = maxp;
    if (flags & (LOAD_HDR | COUNT_HDR)) != 0 {
        maxp += ehdr_size as u64;
    }

    if (flags & (LOAD_SYM | COUNT_SYM)) != 0 {
        if lseek(fd, e_shoff as i64, SEEK_SET).is_err() {
            warn(format_args!("lseek section headers"));
            return Err(Errno::EIO);
        }
        let sz = e_shnum * shdr_size;
        let mut shbuf = vec![0u8; sz];
        if read(fd, &mut shbuf) != Ok(sz) {
            warn(format_args!("read section headers"));
            return Err(Errno::EIO);
        }
        let mut shp: Vec<E::Shdr> = (0..e_shnum)
            .map(|i| E::shdr(&shbuf[i * shdr_size..]))
            .collect();

        let shpp = maxp;
        maxp += roundup(sz as u64, E::ADDR_SIZE);

        let (_, _, stroff, strsz) = shp.get(e_shstrndx).map(E::shdr_fields).unwrap_or_default();
        let mut shstr = vec![0u8; strsz as usize];
        if lseek(fd, stroff as i64, SEEK_SET).is_err() {
            warn(format_args!("lseek section header string table"));
            return Err(Errno::EIO);
        }
        if read(fd, &mut shstr) != Ok(strsz as usize) {
            warn(format_args!("read section header string table"));
            return Err(Errno::EIO);
        }
        let name_is = |n: u32, want: &[u8]| {
            let s = shstr.get(n as usize..).unwrap_or(&[]);
            let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
            &s[..end] == want
        };

        // Now load the symbol sections themselves. Make sure the sections are aligned. Don't
        // bother with string tables if there are no symbol sections.
        let mut off = roundup((ehdr_size + sz) as u64, E::ADDR_SIZE);

        let havesyms = shp.iter().any(|s| E::shdr_fields(s).1 == SHT_SYMTAB);

        let mut first = true;
        for s in &mut shp {
            let (sh_name, sh_type, sh_offset, sh_size) = E::shdr_fields(s);
            if sh_type == SHT_SYMTAB
                || sh_type == SHT_STRTAB
                || name_is(sh_name, b".debug_line")
                || name_is(sh_name, ELF_CTF)
            {
                if havesyms && (flags & LOAD_SYM) != 0 {
                    printf!("{}{}", if first { " [" } else { "+" }, sh_size);
                    if lseek(fd, sh_offset as i64, SEEK_SET).is_err() {
                        warn(format_args!("lseek symbols"));
                        return Err(Errno::EIO);
                    }
                    // SAFETY: the caller's contract on LOADADDR.
                    let dst = unsafe { load_mem(maxp, offset, sh_size as usize) };
                    if read(fd, dst) != Ok(sh_size as usize) {
                        warn(format_args!("read symbols"));
                        return Err(Errno::EIO);
                    }
                }
                maxp += roundup(sh_size, E::ADDR_SIZE);
                E::shdr_relocate(s, off);
                off += roundup(sh_size, E::ADDR_SIZE);
                first = false;
            }
        }
        if (flags & LOAD_SYM) != 0 {
            let bytes: Vec<u8> = shp.iter().flat_map(E::shdr_bytes).collect();
            // SAFETY: the caller's contract on LOADADDR.
            unsafe { load_mem(shpp, offset, bytes.len()) }.copy_from_slice(&bytes);

            if havesyms && !first {
                printf!("]");
            }
        }
    }

    // Frob the copied ELF header to give information relative to elfp.
    if (flags & LOAD_HDR) != 0 {
        let bytes = E::frob_ehdr(elf);
        // SAFETY: the caller's contract on LOADADDR.
        unsafe { load_mem(elfp, offset, bytes.len()) }.copy_from_slice(&bytes);
    }

    let loadaddr = sa_conf().loadaddr;
    marks[MARK_START] = loadaddr(minp, offset);
    marks[MARK_ENTRY] = loadaddr(e_entry, offset);
    marks[MARK_VENTRY] = e_entry;
    marks[MARK_NSYM] = 1; // XXX: Kernel needs >= 0
    marks[MARK_SYM] = loadaddr(elfp, offset);
    marks[MARK_END] = loadaddr(maxp, offset);

    Ok(())
}

/// `elf32_exec` (`elf32.c`).
///
/// # Safety
///
/// As for [`loadfile`](crate::loadfile::loadfile).
pub unsafe fn elf32_exec(
    fd: usize,
    elf: &mut Elf32Ehdr,
    marks: &mut [u64; MARK_MAX],
    flags: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { elf_exec::<Elf32>(fd, elf, marks, flags) }
}

/// `elf64_exec` (`elf64.c`).
///
/// # Safety
///
/// As for [`loadfile`](crate::loadfile::loadfile).
pub unsafe fn elf64_exec(
    fd: usize,
    elf: &mut Elf64Ehdr,
    marks: &mut [u64; MARK_MAX],
    flags: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { elf_exec::<Elf64>(fd, elf, marks, flags) }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // `loadfile` on a small hand-made ELF64 "kernel": text, data with bss, a random segment
    // and a symbol table, loaded through `LOADADDR` into a buffer.

    use alloc::boxed::Box;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::sync::atomic::Ordering;

    use super::*;
    use crate::arc4::{Rc4Ctx, rc4_keysetup};
    use crate::cread::close;
    use crate::hdr::exec_elf::{ELFCLASS64, ELFMAG, Elf64Ehdr, Elf64Phdr, Elf64Shdr};
    use crate::loadfile::{LOAD_ALL, loadfile};
    use crate::testutil::{LOADBASE, add_file, output, setup};

    const PADDR: u64 = 0x100_0000;
    const ENTRY: u64 = 0xffff_ffff_8100_0000;

    /// The ELF file and the bytes of its symbol, string and section-name tables.
    fn kernel() -> (Vec<u8>, [Vec<u8>; 3]) {
        let text: Vec<u8> = (0..16).collect();
        let data = vec![0xdd; 8];
        let symtab = vec![0x5a; 24];
        let strtab = b"\0_start\0".to_vec();
        let shstrtab = b"\0.symtab\0.strtab\0.shstrtab\0.text\0".to_vec();

        let mut f = vec![0u8; 0x100];
        f.extend_from_slice(&text); // 0x100
        f.extend_from_slice(&data); // 0x110
        f.extend_from_slice(&symtab); // 0x118
        f.extend_from_slice(&strtab); // 0x130
        f.extend_from_slice(&shstrtab); // 0x138
        let shoff = f.len().next_multiple_of(8);
        f.resize(shoff, 0);

        let sh = |name, ty, off, size| Elf64Shdr {
            sh_name: name,
            sh_type: ty,
            sh_offset: off,
            sh_size: size,
            ..Default::default()
        };
        let shdrs = [
            Elf64Shdr::default(),
            sh(27, 1, 0x100, 16), // .text, PROGBITS: not copied
            sh(1, SHT_SYMTAB, 0x118, 24),
            sh(9, SHT_STRTAB, 0x130, 8),
            sh(17, SHT_STRTAB, 0x138, shstrtab.len() as u64),
        ];
        for s in &shdrs {
            f.extend_from_slice(s.as_bytes());
        }

        let mut ident = [0u8; 16];
        ident[..4].copy_from_slice(ELFMAG);
        ident[4] = ELFCLASS64;
        let ehdr = Elf64Ehdr {
            e_ident: ident,
            e_entry: ENTRY + 0x10,
            e_phoff: 64,
            e_shoff: shoff as u64,
            e_phentsize: 56,
            e_phnum: 3,
            e_shentsize: 64,
            e_shnum: shdrs.len() as u16,
            e_shstrndx: 4,
            ..Default::default()
        };
        f[..64].copy_from_slice(ehdr.as_bytes());
        let ph = |ty, flags, off, paddr, filesz, memsz| Elf64Phdr {
            p_type: ty,
            p_flags: flags,
            p_offset: off,
            p_vaddr: ENTRY + (paddr - PADDR),
            p_paddr: paddr,
            p_filesz: filesz,
            p_memsz: memsz,
            p_align: 0x1000,
        };
        let phdrs = [
            ph(PT_LOAD, PF_R | PF_X, 0x100, PADDR, 16, 16),
            ph(PT_LOAD, PF_R | PF_W, 0x110, PADDR + 0x1000, 8, 32),
            ph(PT_OPENBSD_RANDOMIZE, PF_R, 0, PADDR + 0x1010, 8, 8),
        ];
        for (i, p) in phdrs.iter().enumerate() {
            f[64 + 56 * i..64 + 56 * (i + 1)].copy_from_slice(&{
                let mut b = [0u8; 56];
                b.copy_from_slice(
                    // SAFETY: `Elf64Phdr` is 56 bytes of `#[repr(C)]` integers.
                    unsafe {
                        core::slice::from_raw_parts((p as *const Elf64Phdr).cast::<u8>(), 56)
                    },
                );
                b
            });
        }
        (f, [symtab, strtab, shstrtab])
    }

    #[test]
    fn loads_segments_symbols_and_marks() {
        let _g = setup();
        let (file, [symtab, strtab, shstrtab]) = kernel();
        add_file("elf", Box::leak(file.into_boxed_slice()));

        let mut mem = vec![0xeeu8; 0x2000];
        let base = mem.as_mut_ptr() as u64;
        LOADBASE.store(base.wrapping_sub(PADDR), Ordering::Relaxed);
        // SAFETY: the test is single-threaded under `setup`'s lock.
        unsafe { rc4_keysetup(RANDOMCTX.get_mut(), b"seed") };

        let mut marks = [0u64; MARK_MAX];
        // SAFETY: LOADADDR maps the kernel's 0x1000000.. into `mem`, which is big enough.
        let fd = unsafe { loadfile(b"elf:/bsd", &mut marks, LOAD_ALL) }.unwrap();
        close(fd).unwrap();

        let size = 0x1060 + 320 + 24 + 8 + shstrtab.len().next_multiple_of(8) as u64;
        assert_eq!(
            output(),
            alloc::format!("16+8+24 [24+8+{}]=0x{:x}\n\r", shstrtab.len(), size)
        );
        assert_eq!(marks[MARK_START], base);
        assert_eq!(marks[MARK_ENTRY], base + 0x10);
        assert_eq!(marks[MARK_VENTRY], ENTRY + 0x10);
        assert_eq!(marks[MARK_SYM], base + 0x1020);
        assert_eq!(marks[MARK_RANDOM], base + 0x1010);
        assert_eq!(marks[MARK_ERANDOM], base + 0x1018);
        assert_eq!(marks[MARK_END], base + size);

        // text, data, the zeroed bss with the random bytes in it
        assert_eq!(mem[..16], (0..16).collect::<Vec<u8>>()[..]);
        assert_eq!(mem[0x1000..0x1008], [0xdd; 8]);
        let mut ctx = Rc4Ctx::new();
        rc4_keysetup(&mut ctx, b"seed");
        let mut rnd = [0u8; 8];
        rc4_getbytes(&mut ctx, &mut rnd);
        assert_eq!(mem[0x1010..0x1018], rnd);
        assert_eq!(mem[0x1018..0x1020], [0; 8]);
        // the ELF header the kernel gets: no program headers, sections right after it
        let ehdr = Elf64Ehdr::from_bytes(&mem[0x1020..]).unwrap();
        assert_eq!((ehdr.e_phoff, ehdr.e_phnum, ehdr.e_shoff), (0, 0, 64));
        // the section headers, the tables offset after them, and the tables
        let sh = |i: usize| Elf64Shdr::from_bytes(&mem[0x1060 + 64 * i..]).unwrap();
        assert_eq!(sh(2).sh_offset, 64 + 320);
        assert_eq!(sh(2).sh_flags & SHF_ALLOC, SHF_ALLOC);
        assert_eq!(sh(3).sh_offset, 64 + 320 + 24);
        assert_eq!(sh(1).sh_offset, 0x100);
        let tables = 0x1060 + 320;
        assert_eq!(mem[tables..tables + 24], symtab[..]);
        assert_eq!(mem[tables + 24..tables + 32], strtab[..]);
        assert_eq!(mem[tables + 32..tables + 32 + shstrtab.len()], shstrtab[..]);
    }
}
/* </TESTS> */
