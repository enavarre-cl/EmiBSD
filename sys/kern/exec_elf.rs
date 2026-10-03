/*	$OpenBSD: exec_elf.c,v 1.206 2026/09/16 03:22:37 deraadt Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 Per Fogelstrom
 * All rights reserved.
 *
 * Copyright (c) 1994 Christos Zoulas
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
 *
 */

/*
 * Copyright (c) 2001 Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Jason R. Thorpe for Wasabi Systems, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed for the NetBSD Project by
 *	Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! `exec_elf.c`: the ELF executable format.
//!
//! Upstream: sys/kern/exec_elf.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M6 (part b) ports `elf_check_header`, `elf_load_psection`,
//! `elf_read_from`, `elf_os_pt_note_name`, `elf_os_pt_note` and `exec_elf_makecmds` for a
//! static `ET_EXEC` executable. `ET_DYN` (PIE, `uvm_map_pie`), `PT_INTERP` (`elf_load_file`,
//! `exec_elf_fixup`, the auxiliary vector), the `PT_OPENBSD_SYSCALLS` pin tables
//! (`elf_read_pintable`, `elf_adjustpins`) and the core dump writers (`coredump_elf`) are
//! M7.
//!
//! ## Deviations
//! - The executable is a memory image (`ep_hdr` holds the whole file): `elf_read_from`
//!   slices it instead of `vn_rdwr`, the vmcmds carry file offsets into it, and the
//!   `v_writecount`/`VTEXT` checks and `vn_marktext` have no vnode to look at.
//! - The 4-clause licence of the Wasabi Systems block (advertising clause) was accepted by
//!   the user at M2 for this project.

use alloc::vec::Vec;
use core::ptr;

use crate::machine::exec::MachineExec;
use crate::machine::{Machine, VmParam};
use crate::sys::errno::Errno;
use crate::sys::exec::{
    ELF_RANDOMIZE_LIMIT, EXEC_NOBTCFI, EXEC_PROFILE, EXEC_WXNEEDED, ExecPackage, ExecVmcmdSet,
    VMCMD_BASE, VMCMD_IMMUTABLE, VMCMD_RELATIVE, VMCMD_TEXTREL, VmcmdProc,
};
use crate::sys::exec_elf::{
    EI_CLASS, EI_DATA, EI_OSABI, EI_VERSION, ELF_MAX_VALID_PHDR, ELF_NO_ADDR, ELFOSABI_OPENBSD,
    ET_DYN, ET_EXEC, EV_CURRENT, ElfEhdr, ElfNote, ElfPhdr, NT_OPENBSD_PROF, PF_OPENBSD_MUTABLE,
    PF_R, PF_W, PF_X, PT_DYNAMIC, PT_GNU_RELRO, PT_INTERP, PT_LOAD, PT_NOTE, PT_OPENBSD_MUTABLE,
    PT_OPENBSD_NOBTCFI, PT_OPENBSD_RANDOMIZE, PT_OPENBSD_SYSCALLS, PT_OPENBSD_WXNEEDED, PT_PHDR,
    PT_SHLIB, elf_trunc, elfround, is_elf,
};
use crate::sys::mman::{PROT_EXEC, PROT_READ, PROT_WRITE};
use crate::sys::proc::Proc;
use crate::sys::syslimits::PATH_MAX;
use crate::unported;
use crate::uvm::uvm_extern::VmProt;
use crate::uvm::uvm_param::{round_page, trunc_page};

/// `ELF_TARG_VER`.
const ELF_TARG_VER: u32 = EV_CURRENT;

/// `ELF_NOTE_NAME_OPENBSD`: the note name id of "OpenBSD".
const ELF_NOTE_NAME_OPENBSD: i32 = 0x01;

/// `elf_note_names[]`: the note names the kernel knows, with their ids.
const ELF_NOTE_NAMES: &[(&[u8], i32)] = &[(b"OpenBSD", ELF_NOTE_NAME_OPENBSD)];

/// Reads a `T` out of the image at `off`, unaligned. `None` when the image is too short.
///
/// Only for the plain-integer ELF structures of `exec_elf.rs`, for which every bit pattern
/// is a value.
macro_rules! read_image {
    ($ty:ty, $image:expr, $off:expr) => {{
        let image: &[u8] = $image;
        let off: usize = $off;
        image
            .get(off..off.saturating_add(size_of::<$ty>()))
            .filter(|bytes| bytes.len() == size_of::<$ty>())
            .map(|bytes| {
                // SAFETY: `bytes` holds `size_of::<$ty>()` readable bytes and the type is a
                // `#[repr(C)]` structure of integers, valid for any bit pattern.
                unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<$ty>()) }
            })
    }};
}

/// Check header for validity; `ENOEXEC` if error.
pub fn elf_check_header(ehdr: &ElfEhdr) -> Result<(), Errno> {
    // We need to check magic, class size, endianness, and version before we look at the
    // rest of the Elf_Ehdr structure. These few elements are represented in a machine
    // independent fashion.
    if !is_elf(ehdr)
        || ehdr.e_ident[EI_CLASS] != <Machine as MachineExec>::ELF_TARG_CLASS
        || ehdr.e_ident[EI_DATA] != <Machine as MachineExec>::ELF_TARG_DATA
        || u32::from(ehdr.e_ident[EI_VERSION]) != ELF_TARG_VER
    {
        return Err(Errno::ENOEXEC);
    }

    // Now check the machine dependent header
    if ehdr.e_machine != <Machine as MachineExec>::ELF_TARG_MACH || ehdr.e_version != ELF_TARG_VER {
        return Err(Errno::ENOEXEC);
    }

    // Don't allow an insane amount of sections.
    if ehdr.e_phnum > ELF_MAX_VALID_PHDR {
        return Err(Errno::ENOEXEC);
    }

    Ok(())
}

/// Load a psection at the appropriate address.
pub fn elf_load_psection(
    vcset: &mut ExecVmcmdSet,
    ph: &ElfPhdr,
    addr: &mut u64,
    size: &mut u64,
    prot: &mut VmProt,
    flags: u32,
) {
    let mut flags = flags;
    let diff: u64;
    let base: u64;

    // If the user specified an address, then we load there.
    if *addr != ELF_NO_ADDR {
        if ph.p_align > 1 {
            *addr = elf_trunc(*addr, ph.p_align);
            diff = ph.p_vaddr.wrapping_sub(elf_trunc(ph.p_vaddr, ph.p_align));
            // page align vaddr
            base = addr
                .wrapping_add(trunc_page(ph.p_vaddr as usize) as u64)
                .wrapping_sub(elf_trunc(ph.p_vaddr, ph.p_align));
        } else {
            diff = 0;
            base = addr
                .wrapping_add(trunc_page(ph.p_vaddr as usize) as u64)
                .wrapping_sub(ph.p_vaddr);
        }
    } else {
        *addr = ph.p_vaddr;
        if ph.p_align > 1 {
            *addr = elf_trunc(*addr, ph.p_align);
        }
        base = trunc_page(ph.p_vaddr as usize) as u64;
        diff = ph.p_vaddr.wrapping_sub(*addr);
    }
    let bdiff = ph
        .p_vaddr
        .wrapping_sub(trunc_page(ph.p_vaddr as usize) as u64);

    // Enforce W^X and map W|X segments without X permission initially. The dynamic linker
    // will make these read-only and add back X permission after relocation processing.
    // Static executables with W|X segments will probably crash.
    *prot |= if ph.p_flags & PF_R != 0 { PROT_READ } else { 0 };
    *prot |= if ph.p_flags & PF_W != 0 {
        PROT_WRITE
    } else {
        0
    };
    if ph.p_flags & PF_W == 0 {
        *prot |= if ph.p_flags & PF_X != 0 { PROT_EXEC } else { 0 };
    }

    // Apply immutability as much as possible, but not text/rodata segments of textrel
    // binaries, or RELRO or PT_OPENBSD_MUTABLE sections, or LOADS marked
    // PF_OPENBSD_MUTABLE, or LOADS which violate W^X. Userland (meaning crt0 or ld.so)
    // will repair those regions.
    if ph.p_flags & (PF_X | PF_W) != (PF_X | PF_W) && ph.p_flags & PF_OPENBSD_MUTABLE == 0 {
        flags |= VMCMD_IMMUTABLE;
    }
    if flags & VMCMD_TEXTREL != 0 && ph.p_flags & PF_W == 0 {
        flags &= !VMCMD_IMMUTABLE;
    }

    let msize = ph.p_memsz.wrapping_add(diff);
    let offset = ph.p_offset.wrapping_sub(bdiff);
    let lsize = ph.p_filesz.wrapping_add(bdiff);
    let mut psize = round_page(lsize as usize) as u64;

    // Because the pagedvn pager can't handle zero fill of the last data page if it's not
    // page aligned we map the last page readvn.
    if ph.p_flags & PF_W != 0 {
        psize = trunc_page(lsize as usize) as u64;
        if psize > 0 {
            vcset.push(
                VmcmdProc::MapPagedvn,
                psize as usize,
                base as usize,
                offset as usize,
                *prot,
                flags,
            );
        }
        if psize != lsize {
            vcset.push(
                VmcmdProc::MapReadvn,
                (lsize - psize) as usize,
                (base + psize) as usize,
                (offset + psize) as usize,
                *prot,
                flags,
            );
        }
    } else {
        vcset.push(
            VmcmdProc::MapPagedvn,
            psize as usize,
            base as usize,
            offset as usize,
            *prot,
            flags,
        );
    }

    // Check if we need to extend the size of the segment
    let rm = round_page(addr.wrapping_add(ph.p_memsz).wrapping_add(diff) as usize);
    let rf = round_page(addr.wrapping_add(ph.p_filesz).wrapping_add(diff) as usize);

    if rm != rf {
        vcset.push(VmcmdProc::MapZero, rm - rf, rf, 0, *prot, flags);
    }
    *size = msize;
}

/// Read from the image at offset: `size` bytes from `off`, or `ENOEXEC` when the image is
/// shorter (the C's short read).
pub fn elf_read_from(image: &[u8], off: u64, size: usize) -> Result<&[u8], Errno> {
    let off = usize::try_from(off).map_err(|_| Errno::ENOEXEC)?;
    let end = off.checked_add(size).ok_or(Errno::ENOEXEC)?;
    image.get(off..end).ok_or(Errno::ENOEXEC)
}

/// `exec_elf_makecmds`: the exec switch check for ELF: builds the vmcmds, the addresses and
/// the entry point of the package from the headers.
pub fn exec_elf_makecmds(p: &Proc, epp: &mut ExecPackage<'_>) -> Result<(), Errno> {
    let result = exec_elf_makecmds_inner(p, epp);
    if let Err(e) = result {
        // bad:
        epp.ep_vmcmds.kill();
        return Err(if e == Errno::ENOEXEC {
            Errno::ENOEXEC
        } else {
            e
        });
    }
    Ok(())
}

/// The body of `exec_elf_makecmds`; the wrapper does the `bad:` cleanup.
fn exec_elf_makecmds_inner(p: &Proc, epp: &mut ExecPackage<'_>) -> Result<(), Errno> {
    let image = epp.ep_hdr;
    let mut phdr: u64 = 0;
    let exe_base: u64 = 0;
    let mut has_phdr = false;
    let mut names = 0;
    let textrel = 0;
    let mut randomizequota = ELF_RANDOMIZE_LIMIT;

    if image.len() < size_of::<ElfEhdr>() {
        return Err(Errno::ENOEXEC);
    }
    let Some(eh) = read_image!(ElfEhdr, image, 0) else {
        return Err(Errno::ENOEXEC);
    };

    elf_check_header(&eh)?;
    if eh.e_type != ET_EXEC && eh.e_type != ET_DYN {
        return Err(Errno::ENOEXEC);
    }

    // check if vnode is in open for writing, because we want to demand-page out of it. if
    // it is, don't do it, for various reasons: a memory image is never open for writing.

    // Allocate space to hold all the program headers, and read them from the file
    let phnum = usize::from(eh.e_phnum);
    let phsize = phnum * size_of::<ElfPhdr>();
    let phbytes = elf_read_from(image, eh.e_phoff, phsize)?;
    let mut ph: Vec<ElfPhdr> = Vec::with_capacity(phnum);
    for i in 0..phnum {
        let Some(pp) = read_image!(ElfPhdr, phbytes, i * size_of::<ElfPhdr>()) else {
            return Err(Errno::ENOEXEC);
        };
        ph.push(pp);
    }

    epp.ep_tsize = ELF_NO_ADDR as usize;
    epp.ep_dsize = ELF_NO_ADDR as usize;

    let mut base_ph: Option<usize> = None;
    for (i, pp) in ph.iter().enumerate() {
        if pp.p_align > 1 && !pp.p_align.is_power_of_two() {
            return Err(Errno::EINVAL);
        }

        if pp.p_type == PT_INTERP {
            if pp.p_filesz < 2 || pp.p_filesz > PATH_MAX as u64 {
                return Err(Errno::ENOEXEC);
            }
            let interp = elf_read_from(image, pp.p_offset, pp.p_filesz as usize)?;
            if interp[interp.len() - 1] != 0 {
                return Err(Errno::ENOEXEC);
            }
            // The interpreter (ld.so): elf_load_file and exec_elf_fixup (M7).
            return Err(unported!("exec_elf_makecmds: PT_INTERP (ld.so, M7)"));
        } else if pp.p_type == PT_LOAD {
            if pp.p_filesz > pp.p_memsz || pp.p_memsz == 0 {
                return Err(Errno::EINVAL);
            }
            if base_ph.is_none() {
                base_ph = Some(i);
            }
        } else if pp.p_type == PT_PHDR {
            has_phdr = true;
        }
    }

    // Verify this is an OpenBSD executable. If it's marked that way via a PT_NOTE then also
    // check for a PT_OPENBSD_WXNEEDED segment.
    elf_os_pt_note(p, epp, &eh, &mut names)?;
    if eh.e_ident[EI_OSABI] == ELFOSABI_OPENBSD {
        names |= ELF_NOTE_NAME_OPENBSD;
    }
    let _ = names;

    if eh.e_type == ET_DYN {
        // need phdr and load sections for PIE
        match base_ph {
            Some(b) if has_phdr && ph[b].p_vaddr == 0 => {}
            _ => return Err(Errno::EINVAL),
        }
        // randomize exe_base for PIE: uvm_map_pie, and DT_TEXTREL in PT_DYNAMIC (M7a).
        return Err(unported!("exec_elf_makecmds: ET_DYN (uvm_map_pie, M7a)"));
    }

    // Load all the necessary sections
    let mut exe_end: u64 = 0;
    let mut syscall_ph = false;
    for (i, pp) in ph.iter().enumerate() {
        match pp.p_type {
            PT_LOAD => {
                let mut addr: u64;
                let mut size: u64 = 0;
                let mut prot: VmProt = 0;
                let mut flags: u32 = 0;

                if exe_base != 0 {
                    if Some(i) == base_ph {
                        flags = VMCMD_BASE;
                        addr = exe_base;
                    } else {
                        flags = VMCMD_RELATIVE;
                        addr = pp.p_vaddr.wrapping_sub(ph[base_ph.unwrap_or(i)].p_vaddr);
                    }
                } else {
                    addr = ELF_NO_ADDR;
                }

                // Calculates size of text and data segments by starting at first and going
                // to end of last. 'rwx' sections are treated as data. this is correct for
                // BSS_PLT, but may not be for DATA_PLT, is fine for TEXT_PLT.
                elf_load_psection(
                    &mut epp.ep_vmcmds,
                    pp,
                    &mut addr,
                    &mut size,
                    &mut prot,
                    flags | textrel,
                );

                // Update exe_base in case alignment was off. For PIE, addr is relative to
                // exe_base so adjust it (non PIE exe_base is 0 so no change).
                if flags != VMCMD_BASE {
                    addr = addr.wrapping_add(exe_base);
                }
                let addr = addr as usize;
                let size = size as usize;

                // Decide whether it's text or data by looking at the protection of the
                // section
                if prot & PROT_WRITE != 0 {
                    // data section
                    if epp.ep_dsize == ELF_NO_ADDR as usize {
                        epp.ep_daddr = addr;
                        epp.ep_dsize = size;
                    } else if addr < epp.ep_daddr {
                        epp.ep_dsize = epp.ep_dsize + epp.ep_daddr - addr;
                        epp.ep_daddr = addr;
                    } else {
                        epp.ep_dsize = addr + size - epp.ep_daddr;
                    }
                } else if prot & PROT_EXEC != 0 {
                    // text section
                    if epp.ep_tsize == ELF_NO_ADDR as usize {
                        epp.ep_taddr = addr;
                        epp.ep_tsize = size;
                    } else if addr < epp.ep_taddr {
                        epp.ep_tsize = epp.ep_tsize + epp.ep_taddr - addr;
                        epp.ep_taddr = addr;
                    } else {
                        epp.ep_tsize = addr + size - epp.ep_taddr;
                    }
                    // end of TEXT (no interpreter)
                    exe_end = (epp.ep_taddr + epp.ep_tsize) as u64;
                }
            }

            PT_SHLIB => return Err(Errno::ENOEXEC),

            // Already did this one
            PT_INTERP | PT_NOTE => {}

            // Note address of program headers (in text segment)
            PT_PHDR => phdr = pp.p_vaddr,

            PT_OPENBSD_RANDOMIZE => {
                if pp.p_memsz > randomizequota as u64 {
                    return Err(Errno::ENOMEM);
                }
                randomizequota -= pp.p_memsz as usize;
                epp.ep_vmcmds.push(
                    VmcmdProc::Randomize,
                    pp.p_memsz as usize,
                    pp.p_vaddr.wrapping_add(exe_base) as usize,
                    0,
                    0,
                    0,
                );
            }

            // DT_DEBUG is not ready on mips (only)
            PT_DYNAMIC => {}

            PT_GNU_RELRO | PT_OPENBSD_MUTABLE => epp.ep_vmcmds.push(
                VmcmdProc::Mutable,
                pp.p_memsz as usize,
                pp.p_vaddr.wrapping_add(exe_base) as usize,
                0,
                0,
                0,
            ),

            PT_OPENBSD_SYSCALLS => syscall_ph = true,

            // Not fatal, we don't need to understand everything :-)
            _ => {}
        }
    }

    if syscall_ph {
        // elf_read_pintable/elf_adjustpins: the pin tables (M7); pin_check accepts every
        // site until then.
        let _ = exe_end;
        let _ = unported!("exec_elf_makecmds: PT_OPENBSD_SYSCALLS pin table (M7)");
    }

    phdr = phdr.wrapping_add(exe_base);

    // Strangely some linux programs may have all load sections marked writeable, in this
    // case, textsize is not -1, but rather 0;
    if epp.ep_tsize == ELF_NO_ADDR as usize {
        epp.ep_tsize = 0;
    }
    // Another possibility is that it has all load sections marked read-only. Fake a
    // zero-sized data segment right after the text segment.
    if epp.ep_dsize == ELF_NO_ADDR as usize {
        epp.ep_daddr = round_page(epp.ep_taddr + epp.ep_tsize);
        epp.ep_dsize = 0;
    }

    // ep_interp: none (static).
    epp.ep_entry = eh.e_entry.wrapping_add(exe_base) as usize; // updated if ld.so loads
    epp.ep_entrymain = eh.e_entry.wrapping_add(exe_base) as usize;
    epp.ep_phdraddr = phdr as usize;
    epp.ep_interpaddr = exe_base as usize;

    // vn_marktext(epp->ep_vp): no vnode.
    crate::kern::exec_subr::exec_setup_stack(p, epp)
}

/// `elf_os_pt_note_name`: the id of the note's name when it is one the kernel knows, with
/// the note's type. `np` is the note header followed by its name and descriptor.
pub fn elf_os_pt_note_name(np: &ElfNote, body: &[u8]) -> Option<(i32, u32)> {
    let namesz = np.namesz as usize;
    let descsz = np.descsz as usize;
    'names: for (name, id) in ELF_NOTE_NAMES {
        let namlen = name.len();
        if namesz <= namlen {
            continue;
        }
        // verify name padding (after the NUL) is NUL
        for j in namlen + 1..elfround(namesz) {
            if body.get(j).copied().unwrap_or(0) != 0 {
                continue 'names;
            }
        }
        // verify desc padding is NUL
        for j in descsz..elfround(descsz) {
            if body.get(j).copied().unwrap_or(0) != 0 {
                continue 'names;
            }
        }
        if body.get(..namlen) == Some(name) && body.get(namlen) == Some(&0) {
            return Some((*id, np.r#type));
        }
    }
    None
}

/// `elf_os_pt_note`: reads the `PT_NOTE` segments and the OpenBSD-specific segment types:
/// sets `EXEC_WXNEEDED`, `EXEC_NOBTCFI` and `EXEC_PROFILE` on the package and returns the
/// note names seen; `ENOEXEC` unless the "OpenBSD" note is among them.
pub fn elf_os_pt_note(
    _p: &Proc,
    epp: &mut ExecPackage<'_>,
    eh: &ElfEhdr,
    namesp: &mut i32,
) -> Result<(), Errno> {
    let image = epp.ep_hdr;
    let mut names = 0;

    let phnum = usize::from(eh.e_phnum);
    let hph = elf_read_from(image, eh.e_phoff, phnum * size_of::<ElfPhdr>())?;

    for i in 0..phnum {
        let Some(ph) = read_image!(ElfPhdr, hph, i * size_of::<ElfPhdr>()) else {
            return Err(Errno::ENOEXEC);
        };
        if ph.p_type == PT_OPENBSD_WXNEEDED {
            epp.ep_flags |= EXEC_WXNEEDED;
            continue;
        }
        if ph.p_type == PT_OPENBSD_NOBTCFI {
            epp.ep_flags |= EXEC_NOBTCFI;
            continue;
        }

        if ph.p_type != PT_NOTE || ph.p_filesz > 1024 {
            continue;
        }

        let np = elf_read_from(image, ph.p_offset, ph.p_filesz as usize)?;

        let mut offset = 0;
        while offset < np.len() {
            let mut remaining = np.len() - offset;

            if size_of::<ElfNote>() > remaining {
                break;
            }
            let Some(np2) = read_image!(ElfNote, np, offset) else {
                break;
            };
            remaining -= size_of::<ElfNote>();

            let namesz = np2.namesz as usize;
            let descsz = np2.descsz as usize;
            if elfround(namesz) < namesz || elfround(descsz) < descsz {
                break;
            }

            if elfround(namesz) > remaining {
                break;
            }
            remaining -= elfround(namesz);
            if elfround(descsz) > remaining {
                break;
            }

            let total = size_of::<ElfNote>() + elfround(namesz) + elfround(descsz);
            let body = &np[offset + size_of::<ElfNote>()..offset + total];
            if let Some((name, r#type)) = elf_os_pt_note_name(&np2, body) {
                if name == ELF_NOTE_NAME_OPENBSD && r#type == NT_OPENBSD_PROF {
                    epp.ep_flags |= EXEC_PROFILE;
                }
                names |= name;
            }
            offset += total;
        }
    }

    *namesp = names;
    if names & ELF_NOTE_NAME_OPENBSD != 0 {
        Ok(())
    } else {
        Err(Errno::ENOEXEC)
    }
}

const _: () = {
    // The entry point sanity check in check_exec compares against this.
    assert!(<Machine as VmParam>::VM_MAXUSER_ADDRESS > 0);
};
