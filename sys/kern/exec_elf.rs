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
//! Status: `wip`. Everything but the core dump writers: `elf_check_header`,
//! `elf_load_psection`, `elf_read_from`, `elf_adjustpins`, `elf_read_pintable`,
//! `elf_load_file` (the interpreter, `ld.so`), `exec_elf_makecmds` (static and PIE
//! executables, `PT_INTERP`, `DT_TEXTREL`, `PT_OPENBSD_RANDOMIZE`, `PT_OPENBSD_MUTABLE`,
//! `PT_GNU_RELRO`, `PT_OPENBSD_SYSCALLS`), `exec_elf_fixup` (the auxiliary vector),
//! `elf_os_pt_note_name`, `elf_os_pt_note` and the `hwcap`/`hwcap2` globals.
//! `coredump_elf` and its helpers (`coredump_setup_elf`, `coredump_walk_elf`,
//! `coredump_notes_elf`, `coredump_note_elf`, `coredump_writenote_elf`) are not ported:
//! `kern_sig.c`'s `coredump` stops at `vn_open` (reported), so nothing would call them.
//!
//! ## Deviations
//! - The executable is a vnode, or the `init` boot module's memory image until a root file
//!   system exists (`ExecFile`, `sys/exec.rs`): `elf_read_from` reads the vnode with
//!   `vn_rdwr` as the C does, or slices the image; the `v_writecount`/`VTEXT` checks and
//!   `vn_marktext` are made for a vnode only.
//! - `elf_os_pt_note_name`'s two padding loops `continue` their own loop in C, so they check
//!   nothing; the port does not check either (only the name comparison decides).
//! - `hwcap`/`hwcap2` are the C's globals (atomics here); arm64's `cpu_identify` (`cpu.c`),
//!   which sets them, is not ported, so arm64 passes 0.
//! - The 4-clause licence of the Wasabi Systems block (advertising clause) was accepted by
//!   the user at M2 for this project.

use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicU64, Ordering};

use crate::kern::exec_subr::{exec_process_vmcmds, exec_setup_stack};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::vput;
use crate::kern::vfs_syscalls::NameiBuf;
use crate::kern::vfs_vnops::{vn_marktext, vn_rdwr};
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_CLOSE, VOP_GETATTR};
use crate::machine::copy::copyout;
use crate::machine::exec::MachineExec;
use crate::machine::{Machine, VmParam};
use crate::sys::errno::Errno;
use crate::sys::exec::{
    ELF_RANDOMIZE_LIMIT, EXEC_NOBTCFI, EXEC_PROFILE, EXEC_WXNEEDED, ExecFile, ExecPackage,
    ExecVmcmdSet, VMCMD_BASE, VMCMD_IMMUTABLE, VMCMD_RELATIVE, VMCMD_TEXTREL, VmcmdProc,
};
use crate::sys::exec_elf::aux_id::*;
use crate::sys::exec_elf::{
    AuxInfo, DT_TEXTREL, EI_CLASS, EI_DATA, EI_OSABI, EI_VERSION, ELF_AUX_ENTRIES,
    ELF_MAX_VALID_PHDR, ELF_NO_ADDR, ELFOSABI_OPENBSD, ET_DYN, ET_EXEC, EV_CURRENT, ElfDyn,
    ElfEhdr, ElfNote, ElfPhdr, NT_OPENBSD_PROF, PF_OPENBSD_MUTABLE, PF_R, PF_W, PF_X, PT_DYNAMIC,
    PT_GNU_RELRO, PT_INTERP, PT_LOAD, PT_NOTE, PT_OPENBSD_MUTABLE, PT_OPENBSD_NOBTCFI,
    PT_OPENBSD_RANDOMIZE, PT_OPENBSD_SYSCALLS, PT_OPENBSD_WXNEEDED, PT_PHDR, PT_SHLIB, elf_round,
    elf_trunc, elfround, is_elf,
};
use crate::sys::fcntl::FREAD;
use crate::sys::malloc::{M_PINSYSCALL, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::mman::{PROT_EXEC, PROT_READ, PROT_WRITE};
use crate::sys::mount::MNT_NOEXEC;
use crate::sys::namei::{FOLLOW, LOCKLEAF, LOOKUP, NiDirp, UNVEIL_READ};
use crate::sys::param::{MAXPATHLEN, PAGE_SIZE};
use crate::sys::pledge::PLEDGE_RPATH;
use crate::sys::proc::{BOGO_PC, Proc};
use crate::sys::syscall::{SYS_MAXSYSCALL, SYS_kbind};
use crate::sys::uio::{UioRw, UioSeg};
use crate::sys::vnode::{IO_UNIT, VREAD, VREG, Vnode};
use crate::uvm::uvm_extern::{UVM_UNKNOWN_OFFSET, VmProt};
use crate::uvm::uvm_map::{VM_MAP_PINSYSCALL_ONCE, uvm_map_hint, uvm_map_mquery, uvm_map_pie};
use crate::uvm::uvm_param::{round_page, trunc_page};

/// `ELF_TARG_VER`.
const ELF_TARG_VER: u32 = EV_CURRENT;

/// `ELF_NOTE_NAME_OPENBSD`: the note name id of "OpenBSD".
const ELF_NOTE_NAME_OPENBSD: i32 = 0x01;

/// `elf_note_names[]`: the note names the kernel knows, with their ids.
const ELF_NOTE_NAMES: &[(&[u8], i32)] = &[(b"OpenBSD", ELF_NOTE_NAME_OPENBSD)];

/// Reads a `T` out of `bytes` at `off`, unaligned. `None` when the slice is too short.
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

/// `hwcap`: the processor capabilities the machine reports as `AUX_hwcap`
/// (`__HAVE_CPU_HWCAP`).
pub static HWCAP: AtomicU64 = AtomicU64::new(0);
/// `hwcap2`: the continuation, `AUX_hwcap2` (`__HAVE_CPU_HWCAP2`).
pub static HWCAP2: AtomicU64 = AtomicU64::new(0);

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
pub fn elf_load_psection<'a>(
    vcset: &mut ExecVmcmdSet<'a>,
    vp: Option<ExecFile<'a>>,
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
                vp,
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
                vp,
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
            vp,
            offset as usize,
            *prot,
            flags,
        );
    }

    // Check if we need to extend the size of the segment
    let rm = round_page(addr.wrapping_add(ph.p_memsz).wrapping_add(diff) as usize);
    let rf = round_page(addr.wrapping_add(ph.p_filesz).wrapping_add(diff) as usize);

    if rm != rf {
        vcset.push(VmcmdProc::MapZero, rm - rf, rf, None, 0, *prot, flags);
    }
    *size = msize;
}

/// Read from vnode into buffer at offset: `buf.len()` bytes from `off`, `ENOEXEC` when the
/// file is shorter. A memory image is sliced instead (see the module's deviations).
pub fn elf_read_from(p: &Proc, vp: ExecFile<'_>, off: u64, buf: &mut [u8]) -> Result<(), Errno> {
    match vp {
        ExecFile::Vnode(vp) => {
            let mut resid = 0usize;
            vn_rdwr(
                UioRw::UIO_READ,
                vp,
                buf.as_mut_ptr().cast::<c_void>(),
                buf.len(),
                off as i64,
                UioSeg::UIO_SYSSPACE,
                0,
                p.p_ucred.get(),
                Some(&mut resid),
                Some(p),
            )?;
            // See if we got all of it
            if resid != 0 {
                return Err(Errno::ENOEXEC);
            }
            Ok(())
        }
        ExecFile::Image(image) => {
            let off = usize::try_from(off).map_err(|_| Errno::ENOEXEC)?;
            let end = off.checked_add(buf.len()).ok_or(Errno::ENOEXEC)?;
            let src = image.get(off..end).ok_or(Errno::ENOEXEC)?;
            buf.copy_from_slice(src);
            Ok(())
        }
    }
}

/// Reads `n` program headers at `off` (`mallocarray` plus `elf_read_from` in C).
fn elf_read_phdrs(p: &Proc, vp: ExecFile<'_>, off: u64, n: usize) -> Result<Vec<ElfPhdr>, Errno> {
    let mut bytes = vec![0u8; n * size_of::<ElfPhdr>()];
    elf_read_from(p, vp, off, &mut bytes)?;
    let mut ph = Vec::with_capacity(n);
    for i in 0..n {
        let Some(pp) = read_image!(ElfPhdr, &bytes, i * size_of::<ElfPhdr>()) else {
            return Err(Errno::ENOEXEC);
        };
        ph.push(pp);
    }
    Ok(ph)
}

/// The pin table `elf_read_pintable` allocated (`M_PINSYSCALL`), as a slice.
///
/// # Safety
///
/// `pins` came from `elf_read_pintable` (or `sys_pinsyscalls`) with `npins` entries and is
/// not freed while the slice lives; nobody else writes it meanwhile.
pub unsafe fn pins_slice<'b>(pins: NonNull<u32>, npins: i32) -> &'b mut [u32] {
    // SAFETY: the caller's contract.
    unsafe { core::slice::from_raw_parts_mut(pins.as_ptr(), npins.max(0) as usize) }
}

/// Rebase the pin offsets inside a base,len window for the text segment only.
pub fn elf_adjustpins(basep: &mut usize, lenp: &mut usize, pins: &mut [u32], offset: u32) {
    // Adjust offsets, base, len
    for pin in pins.iter_mut() {
        if *pin == u32::MAX || *pin == 0 {
            continue;
        }
        *pin = pin.wrapping_sub(offset);
    }
    *basep = basep.wrapping_add(offset as usize);
    *lenp = lenp.wrapping_sub(offset as usize);
}

/// `elf_read_pintable`: reads the `PT_OPENBSD_SYSCALLS` table (`struct pinsyscalls { u_int
/// offset; u_int sysno; }` pairs) and builds the pin table: indexed by system call number,
/// 0 = invalid, -1 = allowed from anywhere (several sites), else the offset from the text
/// base. Returns the number of entries and the table, `(0, None)` when the segment is
/// malformed or an entry is out of range.
pub fn elf_read_pintable(
    p: &Proc,
    vp: ExecFile<'_>,
    pp: &ElfPhdr,
    is_ldso: bool,
    len: usize,
) -> (i32, Option<NonNull<u32>>) {
    const PAIR: u64 = 2 * size_of::<u32>() as u64;
    let mut npins: usize = 0;

    if pp.p_filesz > SYS_MAXSYSCALL as u64 * 2 * PAIR || !pp.p_filesz.is_multiple_of(PAIR) {
        return (0, None);
    }
    let nsyscalls = (pp.p_filesz / PAIR) as usize;
    let mut syscalls = vec![0u8; pp.p_filesz as usize];
    if elf_read_from(p, vp, pp.p_offset, &mut syscalls).is_err() {
        return (0, None);
    }
    let pair = |i: usize| -> (u32, u32) {
        let at = i * PAIR as usize;
        let word = |o: usize| {
            let mut w = [0u8; 4];
            w.copy_from_slice(&syscalls[at + o..at + o + 4]);
            u32::from_ne_bytes(w)
        };
        (word(0), word(4))
    };

    // Validate, and calculate pintable size
    for i in 0..nsyscalls {
        let (offset, sysno) = pair(i);
        if sysno == 0 || sysno as usize >= SYS_MAXSYSCALL || offset as usize > len {
            return (0, None);
        }
        npins = npins.max(sysno as usize);
    }
    if is_ldso {
        npins = npins.max(SYS_kbind as usize); // XXX see ld.so/loader.c
    }
    npins += 1;

    // Fill pintable: 0 = invalid, -1 = allowed, else offset from base
    let Some(mem) = mallocarray(npins, size_of::<u32>(), M_PINSYSCALL, M_WAITOK | M_ZERO) else {
        return (0, None);
    };
    let pins_ptr = mem.cast::<u32>();
    // SAFETY: a fresh, zeroed allocation of `npins` u32s, ours until handed to the caller.
    let pins = unsafe { pins_slice(pins_ptr, npins as i32) };
    for i in 0..nsyscalls {
        let (offset, sysno) = pair(i);
        let slot = &mut pins[sysno as usize];
        if *slot != 0 {
            *slot = u32::MAX; // duplicated
        } else {
            *slot = offset;
        }
    }
    if is_ldso {
        pins[SYS_kbind as usize] = u32::MAX; // XXX see ld.so/loader.c
    }
    (npins as i32, Some(pins_ptr))
}

/// Load a file (interpreter/library) pointed to by path [stolen from coff_load_shlib()].
/// Made slightly generic so it might be used externally.
pub fn elf_load_file(p: &Proc, path: &[u8], epp: &mut ExecPackage<'_>) -> Result<(), Errno> {
    let mut nd = ndinit(LOOKUP, FOLLOW | LOCKLEAF, NiDirp::Sys(path), p);
    nd.ni_pledge = PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_READ;
    namei(&mut nd)?;
    let Some(vp) = nd.ni_vp else {
        return Err(Errno::ENOENT);
    };

    let result = elf_load_file_vnode(p, vp, epp);
    if let Err((error, opened)) = result {
        // bad1: the C closes the vnode whether or not anything opened it.
        if opened {
            let _ = VOP_CLOSE(vp, FREAD, p.p_ucred.get(), Some(p));
        }
        // bad:
        vput(vp);
        return Err(error);
    }
    // bad1: VOP_CLOSE on the success path too, as the C falls through.
    let _ = VOP_CLOSE(vp, FREAD, p.p_ucred.get(), Some(p));
    vput(vp);
    Ok(())
}

/// The body of `elf_load_file` once `namei` found `vp` (locked): the error carries whether
/// the C would `goto bad1` (close the vnode) rather than `goto bad`.
fn elf_load_file_vnode(
    p: &Proc,
    vp: &'static Vnode,
    epp: &mut ExecPackage<'_>,
) -> Result<(), (Errno, bool)> {
    let file = ExecFile::Vnode(vp);
    let mut file_align: u64 = PAGE_SIZE as u64;
    let mut randomizequota = ELF_RANDOMIZE_LIMIT as u64;
    let mut text_start: usize = usize::MAX;
    let mut text_end: usize = 0;
    let mut base_ph: Option<usize> = None;
    let mut syscall_ph: Option<usize> = None;

    if vp.v_type.get() != VREG {
        return Err((Errno::EACCES, false));
    }
    VOP_GETATTR(vp, &mut epp.ep_vap, p.p_ucred.get(), p).map_err(|e| (e, false))?;
    if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_NOEXEC != 0)
    {
        return Err((Errno::EACCES, false));
    }
    VOP_ACCESS(vp, VREAD, p.p_ucred.get(), p).map_err(|e| (e, true))?;
    let mut ehbytes = [0u8; size_of::<ElfEhdr>()];
    elf_read_from(p, file, 0, &mut ehbytes).map_err(|e| (e, true))?;
    let Some(eh) = read_image!(ElfEhdr, &ehbytes, 0) else {
        return Err((Errno::ENOEXEC, true));
    };

    if elf_check_header(&eh).is_err() || eh.e_type != ET_DYN {
        return Err((Errno::ENOEXEC, true));
    }

    let phnum = usize::from(eh.e_phnum);
    let ph = elf_read_phdrs(p, file, eh.e_phoff, phnum).map_err(|e| (e, true))?;

    // loadmap[]: the page-rounded address and size of each PT_LOAD.
    let mut loadmap: Vec<(u64, u64)> = Vec::with_capacity(phnum);
    for pp in &ph {
        if pp.p_align > 1 && !pp.p_align.is_power_of_two() {
            return Err((Errno::EINVAL, true));
        }

        if pp.p_type == PT_LOAD {
            if pp.p_filesz > pp.p_memsz || pp.p_memsz == 0 {
                return Err((Errno::EINVAL, true));
            }
            let vaddr = trunc_page(pp.p_vaddr as usize) as u64;
            let memsz = round_page((pp.p_vaddr + pp.p_memsz - vaddr) as usize) as u64;
            loadmap.push((vaddr, memsz));
            if pp.p_align > file_align {
                file_align = pp.p_align;
            }
        }
    }
    let nload = loadmap.len();
    if nload == 0 {
        return Err((Errno::EINVAL, true));
    }

    // Load the interpreter where a non-fixed mmap(NULL, ...) would (i.e. something safely
    // out of the way).
    let vm = p.vmspace();
    let mut pos = uvm_map_hint(
        vm,
        PROT_EXEC,
        <Machine as VmParam>::VM_MIN_ADDRESS,
        <Machine as VmParam>::VM_MAXUSER_ADDRESS,
    ) as u64;
    pos = elf_round(pos, file_align);

    let mut looped = false;
    let mut i = 0;
    while i < nload {
        // #ifdef this_needs_fixing: the first segment would map the vnode's object.
        let uoff = 0;

        let mut addr = trunc_page((pos + loadmap[i].0) as usize);
        let size = round_page(addr + loadmap[i].1 as usize) - addr;

        // CRAP - map_findspace does not avoid daddr+BRKSIZ
        let daddr = vm.vm_daddr.get();
        if addr + size > daddr && addr < daddr + <Machine as VmParam>::BRKSIZ {
            addr = round_page(daddr + <Machine as VmParam>::BRKSIZ);
        }

        if uvm_map_mquery(
            &vm.vm_map,
            &mut addr,
            size,
            if i == 0 { uoff } else { UVM_UNKNOWN_OFFSET },
            0,
        )
        .is_err()
        {
            if !looped {
                looped = true;
                i = 0;
                pos = 0;
                continue;
            }
            return Err((Errno::ENOMEM, true));
        }
        if addr as u64 != pos + loadmap[i].0 {
            // base changed.
            pos = addr as u64 - trunc_page(loadmap[i].0 as usize) as u64;
            pos = elf_round(pos, file_align);
            i = 0;
            continue;
        }

        i += 1;
    }

    // Load all the necessary sections
    for (i, pp) in ph.iter().enumerate() {
        let mut size: u64 = 0;
        let mut prot: VmProt = 0;

        match pp.p_type {
            PT_LOAD => {
                let flags;
                let mut addr;
                match base_ph {
                    None => {
                        flags = VMCMD_BASE;
                        addr = pos;
                        base_ph = Some(i);
                    }
                    Some(b) => {
                        flags = VMCMD_RELATIVE;
                        addr = pp.p_vaddr.wrapping_sub(ph[b].p_vaddr);
                    }
                }
                elf_load_psection(
                    &mut epp.ep_vmcmds,
                    Some(file),
                    pp,
                    &mut addr,
                    &mut size,
                    &mut prot,
                    flags,
                );
                // If entry is within this section it must be text
                if eh.e_entry >= pp.p_vaddr && eh.e_entry < pp.p_vaddr.wrapping_add(size) {
                    // LOAD containing e_entry may not be writable
                    if prot & PROT_WRITE != 0 {
                        return Err((Errno::ENOEXEC, true));
                    }
                    epp.ep_entry = addr
                        .wrapping_add(eh.e_entry)
                        .wrapping_sub(elf_trunc(pp.p_vaddr, pp.p_align))
                        as usize;
                    if flags == VMCMD_RELATIVE {
                        epp.ep_entry = epp.ep_entry.wrapping_add(pos as usize);
                    }
                    epp.ep_interpaddr = pos as usize;
                }
                if prot & PROT_EXEC != 0 {
                    if (addr as usize) < text_start {
                        text_start = addr as usize;
                    }
                    if (addr + size) as usize >= text_end {
                        text_end = (addr + size) as usize;
                    }
                }
            }

            PT_PHDR | PT_NOTE => {}

            PT_OPENBSD_RANDOMIZE => {
                if pp.p_memsz > randomizequota {
                    return Err((Errno::ENOMEM, true));
                }
                randomizequota -= pp.p_memsz;
                epp.ep_vmcmds.push(
                    VmcmdProc::Randomize,
                    pp.p_memsz as usize,
                    pp.p_vaddr.wrapping_add(pos) as usize,
                    None,
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
                pp.p_vaddr.wrapping_add(pos) as usize,
                None,
                0,
                0,
                0,
            ),

            PT_OPENBSD_SYSCALLS => syscall_ph = Some(i),

            _ => {}
        }
    }

    match syscall_ph {
        Some(s) if text_start != usize::MAX => {
            let pr = p.process();
            let mut base = pos as usize;
            let mut len = text_end;

            let (npins, pins) = elf_read_pintable(p, file, &ph[s], true, len);
            if let Some(pins) = pins
                && npins != 0
            {
                // SAFETY: the fresh table `elf_read_pintable` returned, `npins` entries.
                let table = unsafe { pins_slice(pins, npins) };
                elf_adjustpins(&mut base, &mut len, table, text_start as u32);
                pr.ps_pin.pn_start.set(base);
                pr.ps_pin.pn_end.set(base + len);
                pr.ps_pin.pn_pins.set(pins.as_ptr());
                pr.ps_pin.pn_npins.set(npins);
            }
        }
        // nothing executable or no pin table
        _ => return Err((Errno::EINVAL, true)),
    }

    vn_marktext(vp);
    Ok(())
}

/// Prepare an Elf binary's exec package.
///
/// First, set of the various offsets/lengths in the exec package.
///
/// Then, mark the text image busy (so it can be demand paged) or error out if this is not
/// possible. Finally, set up vmcmds for the text, data, bss, and stack segments.
pub fn exec_elf_makecmds(p: &Proc, epp: &mut ExecPackage<'_>) -> Result<(), Errno> {
    match exec_elf_makecmds_inner(p, epp) {
        Ok(()) => Ok(()),
        Err(e) => {
            // bad:
            epp.ep_interp = None;
            epp.ep_vmcmds.kill();
            Err(e)
        }
    }
}

/// The body of `exec_elf_makecmds`; the wrapper does the `bad:` cleanup. A `goto bad` with
/// `error` still 0 is `ENOEXEC`, as the C's tail says.
fn exec_elf_makecmds_inner(p: &Proc, epp: &mut ExecPackage<'_>) -> Result<(), Errno> {
    let mut phdr: u64 = 0;
    let mut exe_base: u64 = 0;
    let mut exe_end: u64 = 0;
    let mut has_phdr = false;
    let mut names = 0;
    let mut textrel = 0;
    let mut randomizequota = ELF_RANDOMIZE_LIMIT as u64;
    let mut base_ph: Option<usize> = None;
    let mut syscall_ph: Option<usize> = None;

    if epp.ep_hdrvalid < size_of::<ElfEhdr>() {
        return Err(Errno::ENOEXEC);
    }
    let Some(eh) = read_image!(ElfEhdr, epp.hdr(), 0) else {
        return Err(Errno::ENOEXEC);
    };

    if elf_check_header(&eh).is_err() || (eh.e_type != ET_EXEC && eh.e_type != ET_DYN) {
        return Err(Errno::ENOEXEC);
    }
    let Some(file) = epp.file() else {
        return Err(Errno::ENOEXEC);
    };

    // check if vnode is in open for writing, because we want to demand-page out of it. if
    // it is, don't do it, for various reasons.
    if let ExecFile::Vnode(vp) = file
        && vp.v_writecount.get() != 0
    {
        #[cfg(feature = "diagnostic")]
        if vp.v_flag.get() & crate::sys::vnode::VTEXT != 0 {
            crate::kern::subr_prf::panic(format_args!("exec: a VTEXT vnode has writecount != 0"));
        }
        return Err(Errno::ETXTBSY);
    }

    // Allocate space to hold all the program headers, and read them from the file
    let phnum = usize::from(eh.e_phnum);
    let ph = elf_read_phdrs(p, file, eh.e_phoff, phnum)?;

    epp.ep_tsize = ELF_NO_ADDR as usize;
    epp.ep_dsize = ELF_NO_ADDR as usize;

    for (i, pp) in ph.iter().enumerate() {
        if pp.p_align > 1 && !pp.p_align.is_power_of_two() {
            return Err(Errno::EINVAL);
        }

        if pp.p_type == PT_INTERP && epp.ep_interp.is_none() {
            if pp.p_filesz < 2 || pp.p_filesz > MAXPATHLEN as u64 {
                return Err(Errno::ENOEXEC);
            }
            let interp = NameiBuf::get()?;
            let buf = &mut interp.as_mut()[..pp.p_filesz as usize];
            let read = elf_read_from(p, file, pp.p_offset, buf);
            let terminated = buf[buf.len() - 1] == 0;
            epp.ep_interp = Some(interp);
            read?;
            if !terminated {
                return Err(Errno::ENOEXEC);
            }
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
        let Some(b) = base_ph.filter(|&b| has_phdr && ph[b].p_vaddr == 0) else {
            return Err(Errno::EINVAL);
        };
        // randomize exe_base for PIE
        exe_base = uvm_map_pie(ph[b].p_align as usize) as u64;

        // Check if DYNAMIC contains DT_TEXTREL
        for pp in &ph {
            if pp.p_type != PT_DYNAMIC || pp.p_filesz > 64 * 1024 {
                continue;
            }
            let Some(dt) = malloc(pp.p_filesz as usize, M_TEMP, M_WAITOK) else {
                continue;
            };
            // SAFETY: a fresh `p_filesz`-byte allocation, ours until `free` below.
            let bytes =
                unsafe { core::slice::from_raw_parts_mut(dt.as_ptr(), pp.p_filesz as usize) };
            let error = match file {
                ExecFile::Vnode(vp) => vn_rdwr(
                    UioRw::UIO_READ,
                    vp,
                    bytes.as_mut_ptr().cast::<c_void>(),
                    bytes.len(),
                    pp.p_offset as i64,
                    UioSeg::UIO_SYSSPACE,
                    IO_UNIT,
                    p.p_ucred.get(),
                    None,
                    Some(p),
                ),
                ExecFile::Image(_) => {
                    elf_read_from(p, file, pp.p_offset, bytes).map_err(|_| Errno::EIO)
                }
            };
            if error.is_ok() {
                for j in 0..bytes.len() / size_of::<ElfDyn>() {
                    if let Some(d) = read_image!(ElfDyn, bytes, j * size_of::<ElfDyn>())
                        && d.d_tag == DT_TEXTREL
                    {
                        textrel = VMCMD_TEXTREL;
                        break;
                    }
                }
            }
            free(dt, M_TEMP, pp.p_filesz as usize);
        }
    }

    // Load all the necessary sections
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
                    Some(file),
                    pp,
                    &mut addr,
                    &mut size,
                    &mut prot,
                    flags | textrel,
                );

                // Update exe_base in case alignment was off. For PIE, addr is relative to
                // exe_base so adjust it (non PIE exe_base is 0 so no change).
                if flags == VMCMD_BASE {
                    exe_base = addr;
                } else {
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
                    if epp.ep_interp.is_none() {
                        exe_end = (epp.ep_taddr + epp.ep_tsize) as u64; // end of TEXT
                    }
                }
            }

            PT_SHLIB => return Err(Errno::ENOEXEC),

            // Already did this one
            PT_INTERP | PT_NOTE => {}

            // Note address of program headers (in text segment)
            PT_PHDR => phdr = pp.p_vaddr,

            PT_OPENBSD_RANDOMIZE => {
                if pp.p_memsz > randomizequota {
                    return Err(Errno::ENOMEM);
                }
                randomizequota -= pp.p_memsz;
                epp.ep_vmcmds.push(
                    VmcmdProc::Randomize,
                    pp.p_memsz as usize,
                    pp.p_vaddr.wrapping_add(exe_base) as usize,
                    None,
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
                None,
                0,
                0,
                0,
            ),

            PT_OPENBSD_SYSCALLS if epp.ep_interp.is_none() => syscall_ph = Some(i),

            // Not fatal, we don't need to understand everything :-)
            _ => {}
        }
    }

    if let Some(s) = syscall_ph {
        let mut base = exe_base as usize;
        let mut len = exe_end.wrapping_sub(exe_base) as usize;

        let (npins, pins) = elf_read_pintable(p, file, &ph[s], false, len);
        if let Some(pins) = pins
            && npins != 0
        {
            // SAFETY: the fresh table `elf_read_pintable` returned, `npins` entries.
            let table = unsafe { pins_slice(pins, npins) };
            elf_adjustpins(
                &mut base,
                &mut len,
                table,
                (epp.ep_taddr as u64).wrapping_sub(exe_base) as u32,
            );
            epp.ep_pinstart = base;
            epp.ep_pinend = base + len;
            epp.ep_pins = Some(pins);
            epp.ep_npins = npins;
        }
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

    // ep_interp: already in the package.
    epp.ep_entry = eh.e_entry.wrapping_add(exe_base) as usize; // updated if ld.so loads
    epp.ep_entrymain = eh.e_entry.wrapping_add(exe_base) as usize;
    epp.ep_phdraddr = phdr as usize;
    epp.ep_interpaddr = exe_base as usize;

    if let ExecFile::Vnode(vp) = file {
        vn_marktext(vp);
    }
    exec_setup_stack(p, epp)
}

/// Phase II of load. It is now safe to load the interpreter. Info collected when loading
/// the program is available for setup of the interpreter.
pub fn exec_elf_fixup(p: &Proc, epp: &mut ExecPackage<'_>) -> Result<(), Errno> {
    let interp = epp.ep_interp.take();

    // disable kbind() and pinsyscalls() in programs that don't use ld.so
    if interp.is_none() {
        p.process().ps_kbind_addr.set(BOGO_PC);
        let map = &p.vmspace().vm_map;
        map.flags.set(map.flags.get() | VM_MAP_PINSYSCALL_ONCE);
    }

    if let Some(interp) = &interp
        && let Err(error) = elf_load_file(p, interp.as_str(), epp)
    {
        epp.ep_vmcmds.kill();
        return Err(error);
    }
    // We have to do this ourselves...
    let mut error = exec_process_vmcmds(p, epp);

    // Push extra arguments on the stack needed by dynamically linked binaries
    if error.is_ok() {
        let eh = read_image!(ElfEhdr, epp.hdr(), 0);
        let (phentsize, phnum) = eh.map_or((0, 0), |eh| (eh.e_phentsize, eh.e_phnum));
        let mut ai = [AuxInfo::default(); ELF_AUX_ENTRIES];
        let mut a = 0;
        let mut push = |id: i32, v: u64| {
            ai[a] = AuxInfo {
                au_id: id,
                _pad: 0,
                au_v: v,
            };
            a += 1;
        };

        push(AUX_phdr, epp.ep_phdraddr as u64);
        push(AUX_phent, u64::from(phentsize));
        push(AUX_phnum, u64::from(phnum));
        push(AUX_pagesz, PAGE_SIZE as u64);
        push(AUX_base, epp.ep_interpaddr as u64);
        push(AUX_flags, 0);
        push(AUX_entry, epp.ep_entrymain as u64);
        if <Machine as MachineExec>::HAVE_CPU_HWCAP {
            push(AUX_hwcap, HWCAP.load(Ordering::Relaxed));
        }
        if <Machine as MachineExec>::HAVE_CPU_HWCAP2 {
            push(AUX_hwcap2, HWCAP2.load(Ordering::Relaxed));
        }
        push(AUX_openbsd_timekeep, p.process().ps_timekeep.get() as u64);
        push(AUX_openbsd_execpath, epp.ep_execpath as u64);
        push(AUX_null, 0);

        let mut bytes = [0u8; ELF_AUX_ENTRIES * size_of::<AuxInfo>()];
        let (chunks, _) = bytes.as_chunks_mut::<{ size_of::<AuxInfo>() }>();
        for (chunk, entry) in chunks.iter_mut().zip(ai.iter()) {
            chunk.copy_from_slice(&entry.to_bytes());
        }
        error = copyout(&bytes, epp.ep_auxinfo);
    }
    // pool_put(&namei_pool, interp): `interp` drops here.
    drop(interp);
    error
}

/// `elf_os_pt_note_name`: the id of the note's name when it is one the kernel knows, with
/// the note's type. `body` is what follows the note header: the name, then the descriptor.
pub fn elf_os_pt_note_name(np: &ElfNote, body: &[u8]) -> Option<(i32, u32)> {
    for (name, id) in ELF_NOTE_NAMES {
        let namlen = name.len();
        if np.namesz as usize <= namlen {
            continue;
        }
        // verify name padding (after the NUL) is NUL; verify desc padding is NUL: the C's
        // loops `continue` themselves and check nothing (see the module's deviations).
        let nul = body.get(namlen) == Some(&0);
        if body.get(..namlen) == Some(name) && nul {
            return Some((*id, np.r#type));
        }
    }
    None
}

/// `elf_os_pt_note`: reads the `PT_NOTE` segments and the OpenBSD-specific segment types:
/// sets `EXEC_WXNEEDED`, `EXEC_NOBTCFI` and `EXEC_PROFILE` on the package and returns the
/// note names seen; `ENOEXEC` unless the "OpenBSD" note is among them.
pub fn elf_os_pt_note(
    p: &Proc,
    epp: &mut ExecPackage<'_>,
    eh: &ElfEhdr,
    namesp: &mut i32,
) -> Result<(), Errno> {
    let mut names = 0;
    let Some(file) = epp.file() else {
        return Err(Errno::ENOEXEC);
    };

    let result = (|| -> Result<(), Errno> {
        let hph = elf_read_phdrs(p, file, eh.e_phoff, usize::from(eh.e_phnum))?;

        for ph in &hph {
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

            let mut np = vec![0u8; ph.p_filesz as usize];
            elf_read_from(p, file, ph.p_offset, &mut np)?;

            let mut offset = 0;
            while offset < np.len() {
                let mut remaining = np.len() - offset;

                if size_of::<ElfNote>() > remaining {
                    break;
                }
                let Some(np2) = read_image!(ElfNote, &np, offset) else {
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
        Ok(())
    })();

    *namesp = names;
    // out1: the C returns the note verdict whatever the reads said.
    let _ = result;
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

#[cfg(test)]
mod tests;
