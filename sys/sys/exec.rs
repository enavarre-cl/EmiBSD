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
//! Status: `wip`. Milestone M6 (part b) ports `struct ps_strings`, `struct exec_vmcmd` with
//! `exec_vmcmd_set`, the `VMCMD_*` flags, `struct exec_package` (the members the ELF loader
//! and `exec` from a memory image use) and the `EXEC_*` flags. `struct execsw`, the
//! `exec_maxhdrsz` machinery, `exec_script` and the `NCARGS`/`ARG_MAX` argument copying
//! come with `sys_execve` (M6-c). `struct execsw` is here.
//!
//! ## Deviations
//! - The executable is a memory image (`ep_hdr` is the whole file, a Limine module), not a
//!   vnode: `ep_vp`, `ep_vap` and `ep_ndp` do not exist yet, and a vmcmd that reads the file
//!   (`vmcmd_map_readvn`, `vmcmd_map_pagedvn`) copies from `ep_hdr` at its offset.
//! - `exec_vmcmd_set` is a `Vec` instead of the growable array with `EXEC_DEFAULT_VMCMD_SETSIZE`.

use alloc::vec::Vec;

use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
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
    /// `vmcmd_map_pagedvn`: map a range of the file, demand paged (copied, see the module's
    /// deviations).
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

/// `struct exec_vmcmd`: one step of building the address space.
#[derive(Clone, Copy, Debug)]
pub struct ExecVmcmd {
    /// `ev_proc`.
    pub ev_proc: VmcmdProc,
    /// `ev_len`: bytes of memory to be mapped.
    pub ev_len: usize,
    /// `ev_addr`: user virtual address.
    pub ev_addr: usize,
    /// `ev_offset`: offset in the image (`ev_vp` is the image itself).
    pub ev_offset: usize,
    /// `ev_prot`: protections for the segment.
    pub ev_prot: VmProt,
    /// `ev_flags`: `VMCMD_*`.
    pub ev_flags: u32,
}

/// `struct exec_vmcmd_set`: the vmcmds, in order.
#[derive(Debug, Default)]
pub struct ExecVmcmdSet {
    /// `evs_cmds` (`evs_used` is its length).
    pub evs_cmds: Vec<ExecVmcmd>,
}

impl ExecVmcmdSet {
    /// `VMCMDSET_INIT`.
    pub const fn new() -> Self {
        Self {
            evs_cmds: Vec::new(),
        }
    }

    /// `NEW_VMCMD2(evsp, proc, len, addr, vp, offset, prot, flags)`.
    pub fn push(
        &mut self,
        proc: VmcmdProc,
        len: usize,
        addr: usize,
        offset: usize,
        prot: VmProt,
        flags: u32,
    ) {
        self.evs_cmds.push(ExecVmcmd {
            ev_proc: proc,
            ev_len: len,
            ev_addr: addr,
            ev_offset: offset,
            ev_prot: prot,
            ev_flags: flags,
        });
    }

    /// `kill_vmcmds`: drops the commands.
    pub fn kill(&mut self) {
        self.evs_cmds.clear();
    }
}

/// `struct exec_package`: what the exec switch fills in and `exec` acts on.
pub struct ExecPackage<'a> {
    /// `ep_name`: file's name.
    pub ep_name: &'a [u8],
    /// `ep_hdr`: file's exec header; here the whole image (`ep_hdrvalid` is its length).
    pub ep_hdr: &'a [u8],
    /// `ep_vmcmds`: vmcmds used to build vmspace.
    pub ep_vmcmds: ExecVmcmdSet,
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
    /// `ep_auxinfo`: userspace auxinfo address.
    pub ep_auxinfo: usize,
}

impl<'a> ExecPackage<'a> {
    /// A package for the image `hdr` named `name`, before the exec switch looked at it.
    pub const fn new(name: &'a [u8], hdr: &'a [u8]) -> Self {
        Self {
            ep_name: name,
            ep_hdr: hdr,
            ep_vmcmds: ExecVmcmdSet::new(),
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
            ep_auxinfo: 0,
        }
    }
}

/// `exec_makecmds_fcn`: an exec switch entry's check function: fills the package's vmcmds
/// and addresses from the header, or says why not.
pub type ExecMakecmdsFcn = fn(&Proc, &mut ExecPackage<'_>) -> Result<(), Errno>;

/// `struct execsw`: one executable format.
pub struct Execsw {
    /// `es_hdrsz`: size of header for this format.
    pub es_hdrsz: usize,
    /// `es_check`: check function.
    pub es_check: ExecMakecmdsFcn,
}

/// `ELF_RANDOMIZE_LIMIT`: how much `PT_OPENBSD_RANDOMIZE` data one executable may ask for.
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
