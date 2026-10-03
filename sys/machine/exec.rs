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
}
