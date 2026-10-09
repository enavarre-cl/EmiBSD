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
//! `<sys/exec_elf.h>` (and amd64's `<machine/reloc.h>`) for libsa: the ELF headers
//! `loadfile` reads and the dynamic section `self_reloc` walks, in both classes.

/// `EI_CLASS`: the file class's index in `e_ident`.
pub const EI_CLASS: usize = 4;
/// `ELFMAG`: the magic.
pub const ELFMAG: &[u8; 4] = b"\x7fELF";
/// `SELFMAG`: the size of the magic.
pub const SELFMAG: usize = 4;
/// `ELFCLASS32`: 32-bit objects.
pub const ELFCLASS32: u8 = 1;
/// `ELFCLASS64`: 64-bit objects.
pub const ELFCLASS64: u8 = 2;
/// `EI_NIDENT`: the size of `e_ident`.
pub const EI_NIDENT: usize = 16;

/// `SHT_SYMTAB`: symbol table section.
pub const SHT_SYMTAB: u32 = 2;
/// `SHT_STRTAB`: string table section.
pub const SHT_STRTAB: u32 = 3;
/// `ELF_CTF`: the CTF data section's name.
pub const ELF_CTF: &[u8] = b".SUNW_ctf";
/// `SHF_ALLOC`: occupies memory.
pub const SHF_ALLOC: u64 = 0x2;

/// `PT_LOAD`: loadable segment.
pub const PT_LOAD: u32 = 1;
/// `PT_OPENBSD_RANDOMIZE`: fill with random data.
pub const PT_OPENBSD_RANDOMIZE: u32 = 0x65a3_dbe6;
/// `PF_X`: executable.
pub const PF_X: u32 = 0x1;
/// `PF_W`: writable.
pub const PF_W: u32 = 0x2;
/// `PF_R`: readable.
pub const PF_R: u32 = 0x4;

/// `DT_NULL`: marks the end of `_DYNAMIC`.
pub const DT_NULL: i64 = 0;
/// `DT_RELA`: address of the relocation table with addends.
pub const DT_RELA: i64 = 7;
/// `DT_RELASZ`: size of the `DT_RELA` table.
pub const DT_RELASZ: i64 = 8;
/// `DT_RELAENT`: size of a `DT_RELA` entry.
pub const DT_RELAENT: i64 = 9;
/// `DT_REL`: address of the relocation table.
pub const DT_REL: i64 = 17;
/// `DT_RELSZ`: size of the `DT_REL` table.
pub const DT_RELSZ: i64 = 18;
/// `DT_RELENT`: size of a `DT_REL` entry.
pub const DT_RELENT: i64 = 19;

/// `R_X86_64_NONE` (`<machine/reloc.h>`).
pub const R_X86_64_NONE: u32 = 0;
/// `R_X86_64_RELATIVE` (`<machine/reloc.h>`).
pub const R_X86_64_RELATIVE: u32 = 8;
/// `R_AARCH64_NONE` (arm64's `<machine/reloc.h>`).
pub const R_AARCH64_NONE: u32 = 0;
/// `R_AARCH64_RELATIVE` (arm64's `<machine/reloc.h>`).
pub const R_AARCH64_RELATIVE: u32 = 1027;

/// `Elf32_Ehdr`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
#[allow(missing_docs)] // the C member names, documented by exec_elf.h
pub struct Elf32Ehdr {
    pub e_ident: [u8; EI_NIDENT],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u32,
    pub e_phoff: u32,
    pub e_shoff: u32,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

/// `Elf64_Ehdr`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
#[allow(missing_docs)] // the C member names, documented by exec_elf.h
pub struct Elf64Ehdr {
    pub e_ident: [u8; EI_NIDENT],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

/// `Elf32_Shdr`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
#[allow(missing_docs)] // the C member names, documented by exec_elf.h
pub struct Elf32Shdr {
    pub sh_name: u32,
    pub sh_type: u32,
    pub sh_flags: u32,
    pub sh_addr: u32,
    pub sh_offset: u32,
    pub sh_size: u32,
    pub sh_link: u32,
    pub sh_info: u32,
    pub sh_addralign: u32,
    pub sh_entsize: u32,
}

/// `Elf64_Shdr`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
#[allow(missing_docs)] // the C member names, documented by exec_elf.h
pub struct Elf64Shdr {
    pub sh_name: u32,
    pub sh_type: u32,
    pub sh_flags: u64,
    pub sh_addr: u64,
    pub sh_offset: u64,
    pub sh_size: u64,
    pub sh_link: u32,
    pub sh_info: u32,
    pub sh_addralign: u64,
    pub sh_entsize: u64,
}

/// `Elf32_Phdr`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
#[allow(missing_docs)] // the C member names, documented by exec_elf.h
pub struct Elf32Phdr {
    pub p_type: u32,
    pub p_offset: u32,
    pub p_vaddr: u32,
    pub p_paddr: u32,
    pub p_filesz: u32,
    pub p_memsz: u32,
    pub p_flags: u32,
    pub p_align: u32,
}

/// `Elf64_Phdr`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
#[allow(missing_docs)] // the C member names, documented by exec_elf.h
pub struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

/// `Elf64_Dyn`: `d_un` is the `d_val`/`d_ptr` union, one 64-bit word.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Elf64Dyn {
    /// `d_tag`: what the entry is.
    pub d_tag: i64,
    /// `d_un`: `d_val` or `d_ptr`.
    pub d_un: u64,
}

/// `Elf64_Rela`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Elf64Rela {
    /// `r_offset`: where to apply it.
    pub r_offset: u64,
    /// `r_info`: symbol and type.
    pub r_info: u64,
    /// `r_addend`.
    pub r_addend: i64,
}

/// `ELF64_R_TYPE(info)`.
pub const fn elf64_r_type(info: u64) -> u32 {
    (info & 0xffff_ffff) as u32
}

/// Implements `from_bytes`/`as_bytes` for the ELF structures, which are `#[repr(C)]` integers
/// read from and written to unaligned buffers.
macro_rules! elf_bytes {
    ($($t:ty),*) => {$(
        impl $t {
            /// The structure in the first bytes of `b`, or `None` if `b` is too short.
            pub fn from_bytes(b: &[u8]) -> Option<Self> {
                if b.len() < core::mem::size_of::<Self>() {
                    return None;
                }
                // SAFETY: `b` holds a whole structure (checked above), the read is unaligned,
                // and the structure is `#[repr(C)]` integers, valid for any byte pattern.
                Some(unsafe { b.as_ptr().cast::<Self>().read_unaligned() })
            }

            /// The structure's bytes.
            pub fn as_bytes(&self) -> &[u8] {
                // SAFETY: the structure is `#[repr(C)]` integers laid out with no padding
                // (the compile-time checks below pin the sizes), so all its bytes are
                // initialised; the slice borrows `self`.
                unsafe {
                    core::slice::from_raw_parts(
                        (self as *const Self).cast::<u8>(),
                        core::mem::size_of::<Self>(),
                    )
                }
            }
        }
    )*};
}

elf_bytes!(
    Elf32Ehdr, Elf64Ehdr, Elf32Shdr, Elf64Shdr, Elf32Phdr, Elf64Phdr
);

const _: () = assert!(core::mem::size_of::<Elf32Ehdr>() == 52);
const _: () = assert!(core::mem::size_of::<Elf64Ehdr>() == 64);
const _: () = assert!(core::mem::size_of::<Elf32Shdr>() == 40);
const _: () = assert!(core::mem::size_of::<Elf64Shdr>() == 64);
const _: () = assert!(core::mem::size_of::<Elf32Phdr>() == 32);
const _: () = assert!(core::mem::size_of::<Elf64Phdr>() == 56);
const _: () = assert!(core::mem::size_of::<Elf64Rela>() == 24);
/* </CODE> */
