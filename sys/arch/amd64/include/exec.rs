/* <LICENSES> */
/*
 * Written by Artur Grabowski <art@openbsd.org> Public Domain
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/exec.h>`: the ELF target parameters.
//!
//! Upstream: sys/arch/amd64/include/exec.h @ 3ce1f3f79392
//!
//! Status: `ported` (M6). The values are exposed to generic code through
//! `machine::exec::MachineExec`.

use crate::sys::exec_elf::{ELFCLASS64, ELFDATA2LSB, EM_AMD64};

/// `__LDPGSZ`.
pub const LDPGSZ: usize = 4096;
/// `ARCH_ELFSIZE`.
pub const ARCH_ELFSIZE: usize = 64;
/// `ELF_TARG_CLASS`.
pub const ELF_TARG_CLASS: u8 = ELFCLASS64;
/// `ELF_TARG_DATA`.
pub const ELF_TARG_DATA: u8 = ELFDATA2LSB;
/// `ELF_TARG_MACH`.
pub const ELF_TARG_MACH: u16 = EM_AMD64;
/* </CODE> */
