/*	$OpenBSD: exec_elf.h,v 1.112 2026/09/16 03:22:35 deraadt Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1995, 1996 Erik Theisen.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! `<sys/exec_elf.h>`: the ELF ABI header file, formerly known as "elf_abi.h".
//!
//! Upstream: sys/sys/exec_elf.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M6 (part b) ports the 64-bit ELF header and program header,
//! the identification, type, machine, version, segment type and segment flag constants
//! `exec_elf.c` reads: both targets are 64-bit (`ELFSIZE 64`, so `Elf_Ehdr` is
//! `Elf64_Ehdr`). Sections, symbols, relocations, notes, dynamic entries and the auxiliary
//! vector (`AuxInfo`) come with `ld.so` support and core dumps (M7).

/// `Elf64_Addr`.
pub type Elf64Addr = u64;
/// `Elf64_Off`.
pub type Elf64Off = u64;
/// `Elf64_Word`.
pub type Elf64Word = u32;
/// `Elf64_Xword`.
pub type Elf64Xword = u64;
/// `Elf64_Half`.
pub type Elf64Half = u16;

/// `EI_NIDENT`: size of `e_ident[]`.
pub const EI_NIDENT: usize = 16;

/// `EI_MAG0`: file ID.
pub const EI_MAG0: usize = 0;
/// `EI_MAG1`.
pub const EI_MAG1: usize = 1;
/// `EI_MAG2`.
pub const EI_MAG2: usize = 2;
/// `EI_MAG3`.
pub const EI_MAG3: usize = 3;
/// `EI_CLASS`: file class.
pub const EI_CLASS: usize = 4;
/// `EI_DATA`: data encoding.
pub const EI_DATA: usize = 5;
/// `EI_VERSION`: ELF header version.
pub const EI_VERSION: usize = 6;
/// `EI_OSABI`: OS/ABI ID.
pub const EI_OSABI: usize = 7;
/// `EI_ABIVERSION`: ABI version.
pub const EI_ABIVERSION: usize = 8;

/// `ELFMAG`: the magic, `e_ident[EI_MAG0..=EI_MAG3]`.
pub const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// `ELFCLASS64`: 64-bit objs.
pub const ELFCLASS64: u8 = 2;
/// `ELFDATA2LSB`: little-endian.
pub const ELFDATA2LSB: u8 = 1;
/// `EV_CURRENT`: the current version.
pub const EV_CURRENT: u32 = 1;
/// `ELFOSABI_OPENBSD`.
pub const ELFOSABI_OPENBSD: u8 = 12;

/// `ET_EXEC`: executable file.
pub const ET_EXEC: u16 = 2;
/// `ET_DYN`: shared object file.
pub const ET_DYN: u16 = 3;

/// `EM_AMD64` (`EM_X86_64`).
pub const EM_AMD64: u16 = 62;
/// `EM_AARCH64`: ARM 64-bit architecture (AArch64).
pub const EM_AARCH64: u16 = 183;

/// `Elf64_Ehdr`: the ELF header.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Elf64Ehdr {
    /// `e_ident`: Id bytes.
    pub e_ident: [u8; EI_NIDENT],
    /// `e_type`: file type.
    pub e_type: Elf64Half,
    /// `e_machine`: machine type.
    pub e_machine: Elf64Half,
    /// `e_version`: version number.
    pub e_version: Elf64Word,
    /// `e_entry`: entry point.
    pub e_entry: Elf64Addr,
    /// `e_phoff`: program hdr offset.
    pub e_phoff: Elf64Off,
    /// `e_shoff`: section hdr offset.
    pub e_shoff: Elf64Off,
    /// `e_flags`: processor flags.
    pub e_flags: Elf64Word,
    /// `e_ehsize`: sizeof ehdr.
    pub e_ehsize: Elf64Half,
    /// `e_phentsize`: program header entry size.
    pub e_phentsize: Elf64Half,
    /// `e_phnum`: number of program headers.
    pub e_phnum: Elf64Half,
    /// `e_shentsize`: section header entry size.
    pub e_shentsize: Elf64Half,
    /// `e_shnum`: number of section headers.
    pub e_shnum: Elf64Half,
    /// `e_shstrndx`: string table index.
    pub e_shstrndx: Elf64Half,
}

/// `Elf64_Phdr`: a program header.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Elf64Phdr {
    /// `p_type`: entry type.
    pub p_type: Elf64Word,
    /// `p_flags`: flags.
    pub p_flags: Elf64Word,
    /// `p_offset`: offset.
    pub p_offset: Elf64Off,
    /// `p_vaddr`: virtual address.
    pub p_vaddr: Elf64Addr,
    /// `p_paddr`: physical address.
    pub p_paddr: Elf64Addr,
    /// `p_filesz`: file size.
    pub p_filesz: Elf64Xword,
    /// `p_memsz`: memory size.
    pub p_memsz: Elf64Xword,
    /// `p_align`: memory & file alignment.
    pub p_align: Elf64Xword,
}

/// `Elf_Ehdr` (`ELFSIZE 64`).
pub type ElfEhdr = Elf64Ehdr;
/// `Elf_Phdr`.
pub type ElfPhdr = Elf64Phdr;

/// `PT_NULL`: unused.
pub const PT_NULL: u32 = 0;
/// `PT_LOAD`: loadable segment.
pub const PT_LOAD: u32 = 1;
/// `PT_DYNAMIC`: dynamic linking section.
pub const PT_DYNAMIC: u32 = 2;
/// `PT_INTERP`: the RTLD.
pub const PT_INTERP: u32 = 3;
/// `PT_NOTE`: auxiliary information.
pub const PT_NOTE: u32 = 4;
/// `PT_SHLIB`: reserved - purpose undefined.
pub const PT_SHLIB: u32 = 5;
/// `PT_PHDR`: program header.
pub const PT_PHDR: u32 = 6;
/// `PT_TLS`: thread local storage.
pub const PT_TLS: u32 = 7;
/// `PT_GNU_EH_FRAME`.
pub const PT_GNU_EH_FRAME: u32 = 0x6474_e550;
/// `PT_GNU_RELRO`.
pub const PT_GNU_RELRO: u32 = 0x6474_e552;
/// `PT_OPENBSD_MUTABLE`: like bss, but not immutable.
pub const PT_OPENBSD_MUTABLE: u32 = 0x65a3_dbe5;
/// `PT_OPENBSD_RANDOMIZE`: fill with random data.
pub const PT_OPENBSD_RANDOMIZE: u32 = 0x65a3_dbe6;
/// `PT_OPENBSD_WXNEEDED`: program performs W^X violations.
pub const PT_OPENBSD_WXNEEDED: u32 = 0x65a3_dbe7;
/// `PT_OPENBSD_NOBTCFI`: no branch target CFI.
pub const PT_OPENBSD_NOBTCFI: u32 = 0x65a3_dbe8;
/// `PT_OPENBSD_SYSCALLS`: syscall locations.
pub const PT_OPENBSD_SYSCALLS: u32 = 0x65a3_dbe9;
/// `PT_OPENBSD_BOOTDATA`: section for boot arguments.
pub const PT_OPENBSD_BOOTDATA: u32 = 0x65a4_1be6;

/// `PF_X`: executable.
pub const PF_X: u32 = 0x1;
/// `PF_W`: writable.
pub const PF_W: u32 = 0x2;
/// `PF_R`: readable.
pub const PF_R: u32 = 0x4;
/// `PF_OPENBSD_MUTABLE`.
pub const PF_OPENBSD_MUTABLE: u32 = 0x0800_0000;

/// `Elf64_Note`: a note header; the name and the descriptor follow, each padded to
/// `ELFROUNDSIZE`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Elf64Note {
    /// `namesz`.
    pub namesz: Elf64Word,
    /// `descsz`.
    pub descsz: Elf64Word,
    /// `type`.
    pub r#type: Elf64Word,
}

/// `Elf_Note`.
pub type ElfNote = Elf64Note;

/// `NT_OPENBSD_PROF`: the binary is profiled.
pub const NT_OPENBSD_PROF: u32 = 2;
/// `NT_OPENBSD_PROCINFO`: note is a "elfcore_procinfo" structure.
pub const NT_OPENBSD_PROCINFO: u32 = 10;
/// `NT_OPENBSD_AUXV`: note is a bunch of Auxiliary Vectors, terminated by an `AT_NULL` entry.
pub const NT_OPENBSD_AUXV: u32 = 11;
/// `NT_OPENBSD_REGS`: note is a "reg" structure.
pub const NT_OPENBSD_REGS: u32 = 20;
/// `NT_OPENBSD_FPREGS`: note is a "fpreg" structure.
pub const NT_OPENBSD_FPREGS: u32 = 21;

/// `ELFROUNDSIZE`: `sizeof(Elf_Word)`.
pub const ELFROUNDSIZE: usize = size_of::<Elf64Word>();

/// `elfround(x)`: `roundup(x, ELFROUNDSIZE)`.
pub const fn elfround(x: usize) -> usize {
    x.div_ceil(ELFROUNDSIZE) * ELFROUNDSIZE
}

/// `ELF_MAX_VALID_PHDR`: don't allow an insane amount of sections.
pub const ELF_MAX_VALID_PHDR: u16 = 32;

/// `IS_ELF(ehdr)`.
pub fn is_elf(ehdr: &Elf64Ehdr) -> bool {
    ehdr.e_ident[EI_MAG0..=EI_MAG3] == ELFMAG
}

/// `ELF_TRUNC(addr, align)`.
pub const fn elf_trunc(addr: u64, align: u64) -> u64 {
    addr & !(align - 1)
}

/// `ELF_ROUND(addr, align)`.
pub const fn elf_round(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}

/// `ELF_NO_ADDR`: "no address" marker in `exec_elf.c`.
pub const ELF_NO_ADDR: u64 = u64::MAX;

const _: () = {
    assert!(size_of::<Elf64Ehdr>() == 64);
    assert!(size_of::<Elf64Phdr>() == 56);
    assert!(size_of::<Elf64Note>() == 12);
};
