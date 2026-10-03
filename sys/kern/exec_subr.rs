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
//! - No `uvm_map` until M7a: every vmcmd wires zeroed pages with `uvm_map_enter_wired` and
//!   the two file-backed ones copy the image bytes in through the direct map
//!   (`uvm_map_write_wired`) instead of mapping the vnode copy-on-write or `vn_rdwr`-ing into
//!   a writable mapping. `VMCMD_IMMUTABLE` (`uvm_map_immutable`) and `vmcmd_mutable` report
//!   themselves unported; a `PROT_NONE` command wires nothing. `vmcmd_randomize` writes
//!   through the direct map too, so the random bytes land whatever the segment's protection.
//! - `exec_setup_stack` takes the stack limit from `DFLSSIZ` (what `limit0` holds) until
//!   `lim_cur` exists (M6-c).
//! - The 4-clause licence (advertising clause) was accepted by the user at M2 for this
//!   project.

use core::ptr::NonNull;

use crate::dev::rnd::{arc4random, arc4random_buf, arc4random_uniform};
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc};
use crate::machine::pmap::pmap_proc_iflush;
use crate::machine::{Machine, VmParam};
use crate::sys::errno::Errno;
use crate::sys::exec::{
    ELF_RANDOMIZE_LIMIT, ExecPackage, ExecVmcmd, VMCMD_BASE, VMCMD_IMMUTABLE, VMCMD_RELATIVE,
    VMCMD_STACK, VmcmdProc,
};
use crate::sys::malloc::{M_TEMP, M_WAITOK};
use crate::sys::mman::{PROT_EXEC, PROT_NONE, PROT_READ, PROT_WRITE};
use crate::sys::param::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE};
use crate::sys::proc::Proc;
use crate::sys::types::{Vaddr, Vsize};
use crate::unported;
use crate::uvm::uvm_map::{uvm_map_enter_wired, uvm_map_write_wired};
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

    // first, attach to the object (uvn_attach), then do the map: here wired pages holding
    // the image bytes (see the module's deviations). A segment that extends past the image
    // reads as zeroes, as a short file would.
    let map = &p.vmspace().vm_map;
    uvm_map_enter_wired(map, cmd.ev_addr, cmd.ev_len, cmd.ev_prot)?;
    let end = cmd.ev_offset.saturating_add(cmd.ev_len).min(image.len());
    if cmd.ev_offset < end {
        uvm_map_write_wired(map, cmd.ev_addr, &image[cmd.ev_offset..end])?;
    }
    if cmd.ev_prot & PROT_EXEC != 0 {
        pmap_proc_iflush(p.process(), Vaddr::new(cmd.ev_addr), Vsize::new(cmd.ev_len));
    }

    if cmd.ev_flags & VMCMD_IMMUTABLE != 0 {
        let _ = unported!("vmcmd_map_pagedvn: uvm_map_immutable (M7a)");
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

    // The C maps the area PROT_WRITE so that vn_rdwr() could write to it and fixes the
    // protection up afterwards (uvm_map_protect); the direct-map copy needs no such thing.
    let map = &p.vmspace().vm_map;
    uvm_map_enter_wired(map, cmd.ev_addr, round_page(cmd.ev_len), prot)?;

    let end = cmd
        .ev_offset
        .checked_add(cmd.ev_len)
        .ok_or(Errno::ENOEXEC)?;
    let bytes = image.get(cmd.ev_offset..end).ok_or(Errno::ENOEXEC)?;
    uvm_map_write_wired(map, cmd.ev_addr, bytes)?;
    if prot & PROT_EXEC != 0 {
        pmap_proc_iflush(p.process(), Vaddr::new(cmd.ev_addr), Vsize::new(cmd.ev_len));
    }

    if cmd.ev_flags & VMCMD_IMMUTABLE != 0 {
        let _ = unported!("vmcmd_map_readvn: uvm_map_immutable (M7a)");
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
    // UVM_FLAG_STACK marks the entry (M7a); the wired pages are the same either way.
    let _ = VMCMD_STACK;
    uvm_map_enter_wired(map, cmd.ev_addr, round_page(cmd.ev_len), cmd.ev_prot)?;
    if cmd.ev_flags & VMCMD_IMMUTABLE != 0 && cmd.ev_prot != PROT_NONE {
        let _ = unported!("vmcmd_map_zero: uvm_map_immutable (M7a)");
    }
    Ok(())
}

/// `vmcmd_mutable`: handle vmcmd which changes an address space region back to mutable.
pub fn vmcmd_mutable(_p: &Proc, cmd: &ExecVmcmd) -> Result<(), Errno> {
    if cmd.ev_len == 0 {
        return Ok(());
    }

    // ev_addr, ev_len may be misaligned, so maximize the region: uvm_map_immutable(map,
    // trunc_page(ev_addr), round_page(ev_addr + ev_len), 0) (M7a).
    let _ = (
        trunc_page(cmd.ev_addr),
        round_page(cmd.ev_addr + cmd.ev_len),
    );
    let _ = unported!("vmcmd_mutable: uvm_map_immutable (M7a)");
    Ok(())
}

/// `vmcmd_randomize`: handle vmcmd which specifies a randomized address space region.
pub fn vmcmd_randomize(p: &Proc, cmd: &ExecVmcmd) -> Result<(), Errno> {
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
    let map = &p.vmspace().vm_map;
    let error = if len < RANDOMIZE_CTX_THRESHOLD {
        arc4random_buf(&mut buf[..len]);
        let e = uvm_map_write_wired(map, cmd.ev_addr, &buf[..len]);
        libkern::explicit_bzero(&mut buf[..len]);
        e
    } else {
        // arc4random_ctx_new(): a private generator context (M7); the global one serves.
        let mut e = Ok(());
        while len > 0 {
            let sublen = len.min(PAGE_SIZE);
            arc4random_buf(&mut buf[..sublen]);
            e = uvm_map_write_wired(map, cmd.ev_addr + off, &buf[..sublen]);
            if e.is_err() {
                break;
            }
            off += sublen;
            len -= sublen;
            // sched_pause(yield): one page at a time is short.
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
