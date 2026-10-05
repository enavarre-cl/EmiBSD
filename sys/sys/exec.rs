/*	$OpenBSD: exec.h,v 1.61 2026/09/17 19:45:07 dgl Exp $	*/
/*	$NetBSD: exec.h,v 1.59 1996/02/09 18:25:09 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1994 Christopher G. Demetriou
 * Copyright (c) 1993 Theo de Raadt
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)exec.h	8.3 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! `<sys/exec.h>`: the exec package, the vmcmds that build an address space and
//! `ps_strings`.
//!
//! Upstream: sys/sys/exec.h @ 3ce1f3f79392
//!
//! Status: `wip`. `struct ps_strings`, `struct exec_vmcmd` with `exec_vmcmd_set`, the
//! `VMCMD_*` flags, `struct exec_package` (every member), `struct execsw`, the `EXEC_*`
//! flags, `ELF_RANDOMIZE_LIMIT` and `exec_maxhdrsz`. The a.out `MID_*` machine ids and the
//! a.out `struct exec` are not used by any configured format.
//!
//! ## Deviations
//! - The executable is a vnode (`ep_vp`, M8) or, for the `init` boot module until a root
//!   file system exists, a memory image (`ep_image`). A vmcmd that reads the file names it
//!   with an [`ExecFile`]: the vnode (referenced as `new_vmcmd` does) or the image, read at
//!   `ev_offset`.
//! - `exec_vmcmd_set` is a `Vec` instead of the growable array with
//!   `EXEC_DEFAULT_VMCMD_SETSIZE`; `new_vmcmd`/`vmcmdset_extend`/`kill_vmcmds` are
//!   `ExecVmcmdSet::{push, kill}`.
//! - `ep_hdr` is an owned buffer of `exec_maxhdrsz` bytes (the C's `malloc(M_EXEC)`);
//!   `ep_interp` is a `namei_pool` buffer that gives itself back when dropped; `ep_ndp` is
//!   passed to `check_exec` and to the exec switch's functions as an argument instead of
//!   being stored (an image has none).
//! - `ep_fa`, the fake argument vector of `exec_script.c`, is a `Vec` of byte strings
//!   without their NULs.
//! - `exec_maxhdrsz` is a `const fn` over the constant exec switch instead of the global
//!   `init_exec` (`exec_conf.c`) computes at boot.

use alloc::vec;
use alloc::vec::Vec;
use core::ptr::NonNull;

use crate::kern::vfs_subr::{vref, vrele};
use crate::kern::vfs_syscalls::NameiBuf;
use crate::sys::errno::Errno;
use crate::sys::exec_elf::ElfEhdr;
use crate::sys::exec_script::EXEC_SCRIPT_HDRSZ;
use crate::sys::namei::Nameidata;
use crate::sys::proc::Proc;
use crate::sys::vnode::{Vattr, Vnode};
use crate::uvm::uvm_extern::VmProt;

/// `struct ps_strings`: the array of argument and environment strings a process starts with,
/// at `ps_strings`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PsStrings {
    /// `ps_argvstr`: first of 0 or more argument strings.
    pub ps_argvstr: usize,
    /// `ps_nargvstr`: the number of argument strings.
    pub ps_nargvstr: i32,
    /// `ps_envstr`: first of 0 or more environment strings.
    pub ps_envstr: usize,
    /// `ps_nenvstr`: the number of environment strings.
    pub ps_nenvstr: i32,
}

impl PsStrings {
    /// The structure's bytes as `copyout` writes them: the C layout, with the padding after
    /// each `int` zeroed.
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        out[0..8].copy_from_slice(&self.ps_argvstr.to_ne_bytes());
        out[8..12].copy_from_slice(&self.ps_nargvstr.to_ne_bytes());
        out[16..24].copy_from_slice(&self.ps_envstr.to_ne_bytes());
        out[24..28].copy_from_slice(&self.ps_nenvstr.to_ne_bytes());
        out
    }
}

/// What an `exec_vmcmd` does (`ev_proc`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmcmdProc {
    /// `vmcmd_map_pagedvn`: map a range of the file, demand paged.
    MapPagedvn,
    /// `vmcmd_map_readvn`: read a range of the file into fresh pages.
    MapReadvn,
    /// `vmcmd_map_zero`: zero-filled pages.
    MapZero,
    /// `vmcmd_randomize`: fill with random data (`PT_OPENBSD_RANDOMIZE`).
    Randomize,
    /// `vmcmd_mutable`: let the range be changed (`PT_OPENBSD_MUTABLE`).
    Mutable,
}

/// `VMCMD_RELATIVE`: `ev_addr` is relative to the base entry.
pub const VMCMD_RELATIVE: u32 = 0x0001;
/// `VMCMD_BASE`: marks the base entry.
pub const VMCMD_BASE: u32 = 0x0002;
/// `VMCMD_STACK`: create with `UVM_FLAG_STACK`.
pub const VMCMD_STACK: u32 = 0x0004;
/// `VMCMD_WANTPROT`: `ev_prot` is the mapping's wanted protection, not `PROT_MASK`.
pub const VMCMD_WANTPROT: u32 = 0x0008;
/// `VMCMD_IMMUTABLE`: immutable mapping.
pub const VMCMD_IMMUTABLE: u32 = 0x0010;
/// `VMCMD_TEXTREL`: text relocations.
pub const VMCMD_TEXTREL: u32 = 0x0020;

/// The file an exec reads: the C's `struct vnode *` (`ep_vp`, `ev_vp`), or the boot
/// module's memory image (see the module's deviations).
#[derive(Clone, Copy)]
pub enum ExecFile<'a> {
    /// A vnode: a regular file found by `namei`.
    Vnode(&'static Vnode),
    /// The whole executable, in kernel memory.
    Image(&'a [u8]),
}

/// `struct exec_vmcmd`: one step of building the address space.
#[derive(Clone, Copy)]
pub struct ExecVmcmd<'a> {
    /// `ev_proc`.
    pub ev_proc: VmcmdProc,
    /// `ev_len`: bytes of memory to be mapped.
    pub ev_len: usize,
    /// `ev_addr`: user virtual address.
    pub ev_addr: usize,
    /// `ev_vp`: the file to read or map (`None` for NULL), referenced while the command
    /// holds it.
    pub ev_vp: Option<ExecFile<'a>>,
    /// `ev_offset`: offset in the file.
    pub ev_offset: usize,
    /// `ev_prot`: protections for the segment.
    pub ev_prot: VmProt,
    /// `ev_flags`: `VMCMD_*`.
    pub ev_flags: u32,
}

/// `struct exec_vmcmd_set`: the vmcmds, in order.
#[derive(Default)]
pub struct ExecVmcmdSet<'a> {
    /// `evs_cmds` (`evs_used` is its length).
    pub evs_cmds: Vec<ExecVmcmd<'a>>,
}

impl<'a> ExecVmcmdSet<'a> {
    /// `VMCMDSET_INIT`.
    pub const fn new() -> Self {
        Self {
            evs_cmds: Vec::new(),
        }
    }

    /// `new_vmcmd(evsp, proc, len, addr, vp, offset, prot, flags)` (`NEW_VMCMD2`): create a
    /// new vmcmd structure and fill in its fields based on function call arguments; make
    /// sure objects ref'd by the vmcmd are 'held'.
    #[allow(clippy::too_many_arguments)] // the C's signature
    pub fn push(
        &mut self,
        proc: VmcmdProc,
        len: usize,
        addr: usize,
        vp: Option<ExecFile<'a>>,
        offset: usize,
        prot: VmProt,
        flags: u32,
    ) {
        if let Some(ExecFile::Vnode(vp)) = vp {
            vref(vp);
        }
        self.evs_cmds.push(ExecVmcmd {
            ev_proc: proc,
            ev_len: len,
            ev_addr: addr,
            ev_vp: vp,
            ev_offset: offset,
            ev_prot: prot,
            ev_flags: flags,
        });
    }

    /// `kill_vmcmds`: release the commands' vnodes and reset the set.
    pub fn kill(&mut self) {
        for vcp in &self.evs_cmds {
            if let Some(ExecFile::Vnode(vp)) = vcp.ev_vp {
                vrele(vp);
            }
        }
        // Free old vmcmds and reset the array.
        self.evs_cmds.clear();
    }
}

/// `struct exec_package`: what the exec switch fills in and `exec` acts on.
pub struct ExecPackage<'a> {
    /// `ep_name`: file's name.
    pub ep_name: &'a [u8],
    /// `ep_hdr`: file's exec header (`ep_hdrlen` is its length).
    pub ep_hdr: Vec<u8>,
    /// `ep_hdrvalid`: bytes of `ep_hdr` that are valid.
    pub ep_hdrvalid: usize,
    /// `ep_vmcmds`: vmcmds used to build vmspace.
    pub ep_vmcmds: ExecVmcmdSet<'a>,
    /// `ep_vp`: executable's vnode (`None` for a memory image).
    pub ep_vp: Option<&'static Vnode>,
    /// The memory image standing in for `ep_vp` (see the module's deviations).
    pub ep_image: Option<&'a [u8]>,
    /// `ep_vap`: executable's attributes.
    pub ep_vap: Vattr,
    /// `ep_taddr`: process's text address.
    pub ep_taddr: usize,
    /// `ep_tsize`: size of process's text.
    pub ep_tsize: usize,
    /// `ep_daddr`: process's data(+bss) address.
    pub ep_daddr: usize,
    /// `ep_dsize`: size of process's data(+bss).
    pub ep_dsize: usize,
    /// `ep_maxsaddr`: proc's max stack addr ("top").
    pub ep_maxsaddr: usize,
    /// `ep_minsaddr`: proc's min stack addr ("bottom").
    pub ep_minsaddr: usize,
    /// `ep_ssize`: size of process's stack.
    pub ep_ssize: usize,
    /// `ep_entry`: process's (maybe ld.so) point.
    pub ep_entry: usize,
    /// `ep_entrymain`: process's (main) entry point.
    pub ep_entrymain: usize,
    /// `ep_phdraddr`: process's elf phdr location.
    pub ep_phdraddr: usize,
    /// `ep_interpaddr`: process's ld.so location.
    pub ep_interpaddr: usize,
    /// `ep_flags`: `EXEC_*`.
    pub ep_flags: u32,
    /// `ep_fa`: a fake args vector for scripts.
    pub ep_fa: Vec<Vec<u8>>,
    /// `ep_fd`: a file descriptor we're holding.
    pub ep_fd: i32,
    /// `ep_auxinfo`: userspace auxinfo address.
    pub ep_auxinfo: usize,
    /// `ep_interp`: name of interpreter if any (a NUL-terminated `namei_pool` buffer).
    pub ep_interp: Option<NameiBuf>,
    /// `ep_pinstart`: executable region start.
    pub ep_pinstart: usize,
    /// `ep_pinend`: executable region end.
    pub ep_pinend: usize,
    /// `ep_pins`: array of system call offsets (`M_PINSYSCALL`, `ep_npins` entries).
    pub ep_pins: Option<NonNull<u32>>,
    /// `ep_npins`: entries in array.
    pub ep_npins: i32,
    /// `ep_execpath`: execve path on userland stack (0 for NULL).
    pub ep_execpath: usize,
}

impl<'a> ExecPackage<'a> {
    /// A package for the file named `name`, before `check_exec` found it: no vnode, no
    /// image, `ep_hdr` `exec_maxhdrsz` zero bytes.
    pub fn new(name: &'a [u8]) -> Self {
        Self {
            ep_name: name,
            ep_hdr: vec![0; exec_maxhdrsz()],
            ep_hdrvalid: 0,
            ep_vmcmds: ExecVmcmdSet::new(),
            ep_vp: None,
            ep_image: None,
            ep_vap: Vattr::default(),
            ep_taddr: 0,
            ep_tsize: 0,
            ep_daddr: 0,
            ep_dsize: 0,
            ep_maxsaddr: 0,
            ep_minsaddr: 0,
            ep_ssize: 0,
            ep_entry: 0,
            ep_entrymain: 0,
            ep_phdraddr: 0,
            ep_interpaddr: 0,
            ep_flags: 0,
            ep_fa: Vec::new(),
            ep_fd: -1,
            ep_auxinfo: 0,
            ep_interp: None,
            ep_pinstart: 0,
            ep_pinend: 0,
            ep_pins: None,
            ep_npins: 0,
            ep_execpath: 0,
        }
    }

    /// The executable as the loaders read it: `ep_vp`, or the image standing in for it.
    pub fn file(&self) -> Option<ExecFile<'a>> {
        match (self.ep_vp, self.ep_image) {
            (Some(vp), _) => Some(ExecFile::Vnode(vp)),
            (None, Some(image)) => Some(ExecFile::Image(image)),
            (None, None) => None,
        }
    }

    /// The valid part of the header, `ep_hdr[..ep_hdrvalid]`.
    pub fn hdr(&self) -> &[u8] {
        &self.ep_hdr[..self.ep_hdrvalid.min(self.ep_hdr.len())]
    }
}

/// `exec_makecmds_fcn`: an exec switch entry's check function: fills the package's vmcmds
/// and addresses from the header, or says why not. The nameidata is `epp->ep_ndp` (`None`
/// for a memory image), with which the script handler looks its interpreter up.
pub type ExecMakecmdsFcn =
    fn(&Proc, &mut ExecPackage<'_>, Option<&mut Nameidata<'_>>) -> Result<(), Errno>;

/// `struct execsw`: one executable format.
pub struct Execsw {
    /// `es_hdrsz`: size of header for this format.
    pub es_hdrsz: usize,
    /// `es_check`: check function.
    pub es_check: ExecMakecmdsFcn,
}

/// `exec_maxhdrsz`: the largest `es_hdrsz` of the exec switch (see the module's
/// deviations): shell scripts' `EXEC_SCRIPT_HDRSZ` or ELF's `Elf_Ehdr` (`kern_exec.rs`).
pub const fn exec_maxhdrsz() -> usize {
    let elf = size_of::<ElfEhdr>();
    if EXEC_SCRIPT_HDRSZ > elf {
        EXEC_SCRIPT_HDRSZ
    } else {
        elf
    }
}

/// `ELF_RANDOMIZE_LIMIT`: limit on total `PT_OPENBSD_RANDOMIZE` bytes.
pub const ELF_RANDOMIZE_LIMIT: usize = 1024 * 1024;

/// `EXEC_INDIR`: script handling already done.
pub const EXEC_INDIR: u32 = 0x0001;
/// `EXEC_HASFD`: holding a shell script.
pub const EXEC_HASFD: u32 = 0x0002;
/// `EXEC_HASARGL`: has fake args vector.
pub const EXEC_HASARGL: u32 = 0x0004;
/// `EXEC_SKIPARG`: don't copy user-supplied argv\[0\].
pub const EXEC_SKIPARG: u32 = 0x0008;
/// `EXEC_DESTR`: destructive ops performed.
pub const EXEC_DESTR: u32 = 0x0010;
/// `EXEC_WXNEEDED`: executable will violate W^X.
pub const EXEC_WXNEEDED: u32 = 0x0020;
/// `EXEC_NOBTCFI`: no branch target CFI.
pub const EXEC_NOBTCFI: u32 = 0x0040;
/// `EXEC_PROFILE`: profiled binary.
pub const EXEC_PROFILE: u32 = 0x0080;

const _: () = {
    assert!(size_of::<PsStrings>() == 32);
};
