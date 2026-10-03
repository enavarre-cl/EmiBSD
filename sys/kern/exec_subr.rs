/*	$OpenBSD: exec_subr.c,v 1.72 2026/08/15 18:52:28 kettenis Exp $	*/
/*	$NetBSD: exec_subr.c,v 1.9 1994/12/04 03:10:42 mycroft Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1993, 1994 Christopher G. Demetriou
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
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Christopher G. Demetriou.
 * 4. The name of the author may not be used to endorse or promote products
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

//! `exec_subr.c`: the vmcmds that build an address space, and the stack setup.
//!
//! Upstream: sys/kern/exec_subr.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M6 (part b) ports `exec_process_vmcmds`, the five vmcmds and
//! `exec_setup_stack`. `new_vmcmd`/`vmcmdset_extend`/`kill_vmcmds` are
//! `ExecVmcmdSet::{push, kill}` in `sys/exec.rs` (the set is a `Vec`).
//!
//! ## Deviations
//! - There is no vnode: the image is a byte slice in kernel memory (`ExecPackage::ep_hdr`).
//!   `vmcmd_map_pagedvn` therefore cannot `uvn_attach` and map the file copy-on-write; it
//!   maps anonymous zero-fill memory the way `vmcmd_map_readvn` does and both copy the
//!   bytes in with `copyout` (the C's `vn_rdwr(UIO_USERSPACE)`), so the pages are faulted
//!   in by `uvm_fault` from the kernel's copy. A segment that extends past the image reads
//!   as zeroes. `pagedvn` then lowers the protection with `uvm_map_protect` as `readvn`
//!   does.
//! - `arc4random_ctx_new` is not ported: `vmcmd_randomize` uses the global generator for
//!   large regions too.
//! - `exec_setup_stack` takes the stack limit from `DFLSSIZ` (what `limit0` holds) until
//!   `lim_cur` exists (M6-c).
//! - The 4-clause licence (advertising clause) was accepted by the user at M2 for this
//!   project.

use core::ptr::NonNull;

use crate::dev::rnd::{arc4random, arc4random_buf, arc4random_uniform};
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::sched_bsd::r#yield;
use crate::machine::copy::copyout;
use crate::machine::{Machine, VmParam};
use crate::sys::errno::Errno;
use crate::sys::exec::{
    ELF_RANDOMIZE_LIMIT, ExecPackage, ExecVmcmd, VMCMD_BASE, VMCMD_IMMUTABLE, VMCMD_RELATIVE,
    VMCMD_STACK, VmcmdProc,
};
use crate::sys::malloc::{M_TEMP, M_WAITOK};
use crate::sys::mman::{MADV_NORMAL, MAP_INHERIT_COPY, PROT_NONE, PROT_READ, PROT_WRITE};
use crate::sys::param::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE};
use crate::sys::proc::Proc;
use crate::sys::sched::sched_pause;
use crate::uvm::uvm_extern::{
    PROT_MASK, UVM_FLAG_COPYONW, UVM_FLAG_FIXED, UVM_FLAG_OVERLAY, UVM_FLAG_STACK,
    UVM_UNKNOWN_OFFSET, uvm_mapflag,
};
use crate::uvm::uvm_map::{uvm_map, uvm_map_immutable, uvm_map_protect};
use crate::uvm::uvm_param::{round_page, trunc_page};

/// `RANDOMIZE_CTX_THRESHOLD`: below this many bytes `vmcmd_randomize` uses the global
/// generator directly.
const RANDOMIZE_CTX_THRESHOLD: usize = 512;

/// `MAXSSIZ_GUARD`: the guard below the stack's maximum extent.
const MAXSSIZ_GUARD: usize = 1024 * 1024;

/// `exec_process_vmcmds`: runs the package's vmcmds in order, resolving `VMCMD_RELATIVE`
/// addresses against the last `VMCMD_BASE` command, then kills the set. The first error
/// stops the run.
pub fn exec_process_vmcmds(p: &Proc, epp: &mut ExecPackage<'_>) -> Result<(), Errno> {
    let mut base_addr: Option<usize> = None;
    let mut error = Ok(());
    let image = epp.ep_hdr;
    let cmds = core::mem::take(&mut epp.ep_vmcmds.evs_cmds);

    for vcp in cmds {
        let mut vcp = vcp;
        if vcp.ev_flags & VMCMD_RELATIVE != 0 {
            let Some(base) = base_addr else {
                #[cfg(feature = "diagnostic")]
                crate::kern::subr_prf::panic(format_args!("exec_process_vmcmds: RELATIVE no base"));
                #[cfg(not(feature = "diagnostic"))]
                return Err(Errno::EINVAL);
            };
            vcp.ev_addr = vcp.ev_addr.wrapping_add(base);
        }
        error = match vcp.ev_proc {
            VmcmdProc::MapPagedvn => vmcmd_map_pagedvn(p, &vcp, image),
            VmcmdProc::MapReadvn => vmcmd_map_readvn(p, &vcp, image),
            VmcmdProc::MapZero => vmcmd_map_zero(p, &vcp),
            VmcmdProc::Mutable => vmcmd_mutable(p, &vcp),
            VmcmdProc::Randomize => vmcmd_randomize(p, &vcp),
        };
        if vcp.ev_flags & VMCMD_BASE != 0 {
            base_addr = Some(vcp.ev_addr);
        }
        if error.is_err() {
            break;
        }
    }

    epp.ep_vmcmds.kill();

    error
}

/// `vmcmd_map_pagedvn`: handle vmcmd which specifies that a vnode should be mmap'd.
/// appropriate for handling demand-paged text and data segments.
pub fn vmcmd_map_pagedvn(p: &Proc, cmd: &ExecVmcmd, image: &[u8]) -> Result<(), Errno> {
    // note that if you're going to map part of a process as being paged from a vnode, that
    // vnode had damn well better be marked as VTEXT. that's handled in the routine which
    // sets up the vmcmd to call this routine.

    // map the vnode in using uvm_map.
    if cmd.ev_len == 0 {
        return Ok(());
    }
    if cmd.ev_offset & PAGE_MASK != 0 {
        return Err(Errno::EINVAL);
    }
    if cmd.ev_addr & PAGE_MASK != 0 {
        return Err(Errno::EINVAL);
    }
    if cmd.ev_len & PAGE_MASK != 0 {
        return Err(Errno::EINVAL);
    }

    // first, attach to the object: no vnode (see the module's deviations); the bytes the
    // file would page in are copied into anonymous memory.
    let end = cmd.ev_offset.saturating_add(cmd.ev_len).min(image.len());
    let bytes = image.get(cmd.ev_offset..end).unwrap_or(&[]);
    let map = &p.vmspace().vm_map;
    let mut addr = cmd.ev_addr;

    // do the map
    uvm_map(
        map,
        &mut addr,
        cmd.ev_len,
        None,
        UVM_UNKNOWN_OFFSET,
        0,
        uvm_mapflag(
            cmd.ev_prot | PROT_WRITE,
            PROT_MASK,
            MAP_INHERIT_COPY,
            MADV_NORMAL,
            UVM_FLAG_COPYONW | UVM_FLAG_FIXED,
        ),
    )?;
    copyout(bytes, cmd.ev_addr)?;
    if cmd.ev_prot & PROT_WRITE == 0 {
        uvm_map_protect(
            map,
            cmd.ev_addr,
            cmd.ev_addr + cmd.ev_len,
            cmd.ev_prot,
            0,
            false,
            true,
        )?;
    }

    if cmd.ev_flags & VMCMD_IMMUTABLE != 0 {
        let _ = uvm_map_immutable(map, cmd.ev_addr, round_page(cmd.ev_addr + cmd.ev_len), true);
    }
    // PMAP_CHECK_COPYIN: not configured.

    Ok(())
}

/// `vmcmd_map_readvn`: handle vmcmd which specifies that a vnode should be read from.
/// appropriate for non-demand-paged text/data segments, i.e. impure objects (a la OMAGIC and
/// NMAGIC).
pub fn vmcmd_map_readvn(p: &Proc, cmd: &ExecVmcmd, image: &[u8]) -> Result<(), Errno> {
    if cmd.ev_len == 0 {
        return Ok(());
    }
    if cmd.ev_addr & PAGE_MASK != 0 {
        return Err(Errno::EINVAL);
    }

    let prot = cmd.ev_prot;

    let map = &p.vmspace().vm_map;
    let mut addr = cmd.ev_addr;
    uvm_map(
        map,
        &mut addr,
        round_page(cmd.ev_len),
        None,
        UVM_UNKNOWN_OFFSET,
        0,
        uvm_mapflag(
            prot | PROT_WRITE,
            PROT_MASK,
            MAP_INHERIT_COPY,
            MADV_NORMAL,
            UVM_FLAG_FIXED | UVM_FLAG_OVERLAY | UVM_FLAG_COPYONW,
        ),
    )?;

    // vn_rdwr(UIO_READ, ..., UIO_USERSPACE): the image's bytes, copied out (a short image
    // is an I/O error, as a short read).
    let end = cmd.ev_offset.checked_add(cmd.ev_len).ok_or(Errno::EIO)?;
    let bytes = image.get(cmd.ev_offset..end).ok_or(Errno::EIO)?;
    copyout(bytes, cmd.ev_addr)?;

    if prot & PROT_WRITE == 0 {
        // we had to map in the area at PROT_WRITE so that vn_rdwr() could write to it.
        // however, the caller seems to want it mapped read-only, so now we are going to
        // have to call uvm_map_protect() to fix up the protection. ICK.
        uvm_map_protect(
            map,
            cmd.ev_addr,
            round_page(cmd.ev_addr + cmd.ev_len),
            prot,
            0,
            false,
            true,
        )?;
    }
    if cmd.ev_flags & VMCMD_IMMUTABLE != 0 {
        let _ = uvm_map_immutable(map, cmd.ev_addr, round_page(cmd.ev_addr + cmd.ev_len), true);
    }
    Ok(())
}

/// `vmcmd_map_zero`: handle vmcmd which specifies a zero-filled address space region.
pub fn vmcmd_map_zero(p: &Proc, cmd: &ExecVmcmd) -> Result<(), Errno> {
    if cmd.ev_len == 0 {
        return Ok(());
    }

    kassert!(cmd.ev_addr & PAGE_MASK == 0);
    let map = &p.vmspace().vm_map;
    let mut addr = cmd.ev_addr;
    let error = uvm_map(
        map,
        &mut addr,
        round_page(cmd.ev_len),
        None,
        UVM_UNKNOWN_OFFSET,
        0,
        uvm_mapflag(
            cmd.ev_prot,
            PROT_MASK,
            MAP_INHERIT_COPY,
            MADV_NORMAL,
            UVM_FLAG_FIXED
                | UVM_FLAG_COPYONW
                | if cmd.ev_flags & VMCMD_STACK != 0 {
                    UVM_FLAG_STACK
                } else {
                    0
                },
        ),
    );
    if cmd.ev_flags & VMCMD_IMMUTABLE != 0 {
        let _ = uvm_map_immutable(map, cmd.ev_addr, round_page(cmd.ev_addr + cmd.ev_len), true);
    }
    error
}

/// `vmcmd_mutable`: handle vmcmd which changes an address space region back to mutable.
pub fn vmcmd_mutable(p: &Proc, cmd: &ExecVmcmd) -> Result<(), Errno> {
    if cmd.ev_len == 0 {
        return Ok(());
    }

    // ev_addr, ev_len may be misaligned, so maximize the region
    let _ = uvm_map_immutable(
        &p.vmspace().vm_map,
        trunc_page(cmd.ev_addr),
        round_page(cmd.ev_addr + cmd.ev_len),
        false,
    );
    Ok(())
}

/// `vmcmd_randomize`: handle vmcmd which specifies a randomized address space region.
pub fn vmcmd_randomize(_p: &Proc, cmd: &ExecVmcmd) -> Result<(), Errno> {
    let mut len = cmd.ev_len;
    let mut off = 0;

    if len == 0 {
        return Ok(());
    }
    if len > ELF_RANDOMIZE_LIMIT {
        return Err(Errno::EINVAL);
    }

    let Some(mem) = malloc(PAGE_SIZE, M_TEMP, M_WAITOK) else {
        return Err(Errno::ENOMEM);
    };
    // SAFETY: a fresh `PAGE_SIZE`-byte allocation, ours until `free` below.
    let buf: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(mem.as_ptr(), PAGE_SIZE) };
    let error = if len < RANDOMIZE_CTX_THRESHOLD {
        arc4random_buf(&mut buf[..len]);
        let e = copyout(&buf[..len], cmd.ev_addr);
        libkern::explicit_bzero(&mut buf[..len]);
        e
    } else {
        // arc4random_ctx_new(): see the module's deviations.
        let mut e = Ok(());
        while len > 0 {
            let sublen = len.min(PAGE_SIZE);
            arc4random_buf(&mut buf[..sublen]);
            e = copyout(&buf[..sublen], cmd.ev_addr + off);
            if e.is_err() {
                break;
            }
            off += sublen;
            len -= sublen;
            sched_pause(r#yield);
        }
        libkern::explicit_bzero(buf);
        e
    };
    free(NonNull::from(&mut *buf).cast::<u8>(), M_TEMP, PAGE_SIZE);
    error
}

/// `exec_setup_stack`: Set up the stack segment for an executable.
///
/// Note that the ep_ssize parameter must be set to be the current stack limit; this is
/// adjusted in the body of execve() to yield the appropriate stack segment usage once the
/// argument length is calculated.
///
/// This function returns an int for uniformity with other (future) formats' stack setup
/// functions. They might have errors to return.
pub fn exec_setup_stack(_p: &Proc, epp: &mut ExecPackage<'_>) -> Result<(), Errno> {
    const USRSTACK: usize = <Machine as VmParam>::USRSTACK;
    const MAXSSIZ: usize = <Machine as VmParam>::MAXSSIZ;
    const VM_MIN_STACK_ADDRESS: usize = <Machine as VmParam>::VM_MIN_STACK_ADDRESS;

    // MACHINE_STACK_GROWS_UP: neither amd64 nor arm64.
    epp.ep_maxsaddr = USRSTACK - MAXSSIZ - MAXSSIZ_GUARD;
    epp.ep_minsaddr = USRSTACK;
    // round_page(lim_cur(RLIMIT_STACK)): see the module's deviations.
    epp.ep_ssize = round_page(<Machine as VmParam>::DFLSSIZ);

    // VM_MIN_STACK_ADDRESS is defined on both.
    let mut dist: usize = USRSTACK - MAXSSIZ - MAXSSIZ_GUARD - VM_MIN_STACK_ADDRESS;
    if dist >> PAGE_SHIFT > 0xffff_ffff {
        dist = (arc4random() as usize) << PAGE_SHIFT;
    } else {
        dist = (arc4random_uniform((dist >> PAGE_SHIFT) as u32) as usize) << PAGE_SHIFT;
    }

    epp.ep_maxsaddr -= dist;
    epp.ep_minsaddr -= dist;

    // set up commands for stack. note that this takes *two*, one to map the part of the
    // stack which we can access, and one to map the part which we can't.
    //
    // arguably, it could be made into one, but that would require the addition of another
    // mapping proc, which is unnecessary
    //
    // note that in memory, things assumed to be: 0 ....... ep_maxsaddr <stack> ep_minsaddr
    epp.ep_vmcmds.push(
        VmcmdProc::MapZero,
        (epp.ep_minsaddr - epp.ep_ssize) - epp.ep_maxsaddr,
        epp.ep_maxsaddr,
        0,
        PROT_NONE,
        VMCMD_IMMUTABLE,
    );
    epp.ep_vmcmds.push(
        VmcmdProc::MapZero,
        epp.ep_ssize,
        epp.ep_minsaddr - epp.ep_ssize,
        0,
        PROT_READ | PROT_WRITE,
        VMCMD_STACK | VMCMD_IMMUTABLE,
    );

    Ok(())
}
