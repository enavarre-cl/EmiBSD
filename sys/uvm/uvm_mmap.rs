/*	$OpenBSD: uvm_mmap.c,v 1.204 2026/02/11 22:34:41 deraadt Exp $	*/
/*	$NetBSD: uvm_mmap.c,v 1.49 2001/02/18 21:19:08 chs Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Charles D. Cranor and Washington University.
 * Copyright (c) 1991, 1993 The Regents of the University of California.
 * Copyright (c) 1988 University of Utah.
 *
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * the Systems Programming Group of the University of Utah Computer
 * Science Department.
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
 * from: Utah $Hdr: vm_mmap.c 1.6 91/10/21$
 *      @(#)vm_mmap.c   8.5 (Berkeley) 5/19/94
 * from: Id: uvm_mmap.c,v 1.1.2.14 1998/01/05 21:04:26 chuck Exp
 */
/* </LICENSES> */

//! `uvm_mmap.c`: system call interface into VM system, plus kernel vm_mmap function.
//!
//! Upstream: sys/uvm/uvm_mmap.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M7a ports the anonymous-memory half of the file: `sys_mquery`,
//! `uvm_wxcheck`, `sys_mmap`, `sys_msync`, `sys_munmap`, `sys_mprotect`, `sys_pinsyscalls`,
//! `sys_mimmutable`, `sys_minherit`, `sys_madvise`, `sys_mlock`, `sys_munlock`,
//! `sys_mlockall`, `sys_munlockall`, `uvm_mmaplock`, `uvm_mmapanon` and `sys_kbind`.
//! `uvm_mmapfile` (vnode and device mappings) comes with the vnode layer.
//!
//! ## Deviations
//! - No file table and no vnodes: `sys_mmap` of a descriptor and `sys_mquery` with `fd >= 0`
//!   report `fd_getfile`/`getvnode` (`kern_descrip.c`) and fail; `uvm_mmapfile` is not here.
//! - `uvm_wxcheck`: `ps_textvp` has no mount (no vnodes), so W^X is never allowed; the
//!   `uvm_wxabort` path reports `log` and then `sigexit`s.
//! - `pledge_protexec`: no process can be pledged before `kern_pledge.c`; a pledged one is
//!   reported and refused.
//! - `pmap_wired_count` exists on both machines, so the `suser` branches of `mlock(2)` and
//!   friends are not compiled, as in the C.

use core::sync::atomic::{AtomicI32, Ordering};

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::kern_sig::sigexit;
use crate::machine::copy::{copyin, kcopy};
use crate::machine::cpu::Cpu;
use crate::machine::exec::MachineExec;
use crate::machine::pmap::pmap_wired_count;
use crate::machine::{Machine, VmParam};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_PINSYSCALL, M_WAITOK, M_ZERO};
use crate::sys::mman::{
    __MAP_NOFAULT, __MAP_NOREPLACE, MADV_DONTNEED, MADV_FREE, MADV_NORMAL, MADV_RANDOM,
    MADV_SEQUENTIAL, MADV_SPACEAVAIL, MADV_WILLNEED, MAP_ANON, MAP_CONCEAL, MAP_FIXED,
    MAP_FLAGMASK, MAP_INHERIT_COPY, MAP_INHERIT_SHARE, MAP_PRIVATE, MAP_SHARED, MAP_STACK,
    MCL_CURRENT, MCL_FUTURE, MS_ASYNC, MS_INVALIDATE, MS_SYNC, PROT_EXEC, PROT_NONE, PROT_READ,
    PROT_WRITE,
};
use crate::sys::param::PAGE_MASK;
use crate::sys::proc::{PS_PLEDGE, Proc, p_hassibling};
use crate::sys::resource::{RLIMIT_DATA, RLIMIT_MEMLOCK};
use crate::sys::resourcevar::lim_cur;
use crate::sys::signal::{SIGABRT, SIGILL};
use crate::sys::syscall::SYS_MAXSYSCALL;
use crate::sys::syscallargs::{
    SysKbindArgs, SysMadviseArgs, SysMimmutableArgs, SysMinheritArgs, SysMlockArgs,
    SysMlockallArgs, SysMmapArgs, SysMprotectArgs, SysMqueryArgs, SysMsyncArgs, SysMunlockArgs,
    SysMunmapArgs, SysPinsyscallsArgs,
};
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::types::Register;
use crate::sys::unistd::{KBIND_BLOCK_MAX, KBIND_DATA_MAX, Kbind};
use crate::unported;
use crate::uvm::uvm_amap::AMAP_REFALL;
use crate::uvm::uvm_extern::{
    PROT_MASK, UVM_FLAG_CONCEAL, UVM_FLAG_COPYONW, UVM_FLAG_FIXED, UVM_FLAG_OVERLAY,
    UVM_FLAG_STACK, UVM_FLAG_UNMAP, UVM_LK_ENTER, UVM_UNKNOWN_OFFSET, VmProt, uvm_mapflag,
};
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_km::kernel_map;
use crate::uvm::uvm_map::{
    UVM_EXTRACT_FIXPROT, UvmMapDeadq, VmMap, uvm_map_advice, uvm_map_checkprot, uvm_map_clean,
    uvm_map_extract, uvm_map_hint, uvm_map_immutable, uvm_map_inherit, uvm_map_mquery,
    uvm_map_pageable, uvm_map_pageable_all, uvm_map_protect, uvm_mapanon, uvm_unmap,
    uvm_unmap_detach, uvm_unmap_remove, vm_map_lock, vm_map_unlock,
};
use crate::uvm::uvm_map::{VM_MAP_PINSYSCALL_ONCE, VM_MAP_WIREFUTURE};
use crate::uvm::uvm_pager::{PGO_CLEANIT, PGO_DEACTIVATE, PGO_FREE, PGO_SYNCIO};
use crate::uvm::uvm_param::{atop, ptoa, round_page, trunc_page};

/// `BOGO_PC`: the `ps_kbind_addr` that disables `kbind(2)`.
const BOGO_PC: usize = usize::MAX;

/// `ALIGN_ADDR(addr, size, pageoff)`: page align `addr` and `size`, returning `EINVAL` on
/// wraparound. Yields the aligned address and size and the page offset taken off.
fn align_addr(addr: usize, size: usize) -> Result<(usize, usize, usize), Errno> {
    let (mut addr, mut size) = (addr, size);
    let pageoff = addr & PAGE_MASK;
    if pageoff != 0 {
        if size > usize::MAX - pageoff {
            return Err(Errno::EINVAL); // wraparound
        }
        addr -= pageoff;
        size += pageoff;
    }
    if size != 0 {
        // (vsize_t)round_page(size), which wraps to 0 near the top of the address space.
        size = size.wrapping_add(PAGE_MASK) & !PAGE_MASK;
        if size == 0 {
            return Err(Errno::EINVAL); // wraparound
        }
    }
    Ok((addr, size, pageoff))
}

/// `pledge_protexec(p, prot)` (`kern_pledge.c`): see the module's deviations.
fn pledge_protexec(p: &Proc, _prot: VmProt) -> Result<(), Errno> {
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE == 0 {
        return Ok(());
    }
    Err(unported!("pledge_protexec (kern_pledge.c)"))
}

/// `sys_mquery`: provide mapping hints to applications that do fixed mappings.
///
/// - flags: 0 or `MAP_FIXED` (`MAP_FIXED` means that we insist on this addr and don't care
///   about `PMAP_PREFER` or such)
/// - addr: hint where we'd like to place the mapping.
/// - size: size of the mapping
/// - fd: fd of the file we want to map
/// - off: offset within the file
pub fn sys_mquery(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMqueryArgs = sysargs(v);
    let mut vaddr = uap.addr.get() as usize;
    let prot = uap.prot.get();
    let size = uap.len.get();
    let fd = uap.fd.get();
    let mut flags = 0;

    if prot & PROT_MASK != prot {
        return Err(Errno::EINVAL);
    }

    if uap.flags.get() & MAP_FIXED != 0 {
        flags |= UVM_FLAG_FIXED;
    }

    let uoff = if fd >= 0 {
        // getvnode(p, fd, &fp); uoff = SCARG(uap, pos): see the module's deviations.
        return Err(unported!("sys_mquery: getvnode (kern_descrip.c)"));
    } else {
        UVM_UNKNOWN_OFFSET
    };

    if vaddr == 0 {
        vaddr = uvm_map_hint(
            p.vmspace(),
            prot,
            <Machine as VmParam>::VM_MIN_ADDRESS,
            <Machine as VmParam>::VM_MAXUSER_ADDRESS,
        );
    }

    uvm_map_mquery(&p.vmspace().vm_map, &mut vaddr, size, uoff, flags)?;
    retval[0] = vaddr as Register;
    Ok(())
}

/// `uvm_wxabort`: kill a process that attempts a W^X violation (`kern.wxabort`).
pub static UVM_WXABORT: AtomicI32 = AtomicI32::new(0);

/// `uvm_wxcheck`: W^X violations are only allowed on permitted filesystems.
fn uvm_wxcheck(p: &Proc, call: &str) -> Result<(), Errno> {
    let pr = p.process();
    // pr->ps_textvp->v_mount->mnt_flag & MNT_WXALLOWED: see the module's deviations.
    let wxallowed = false;

    if wxallowed {
        return Ok(());
    }

    if UVM_WXABORT.load(Ordering::Relaxed) != 0 {
        // KERNEL_LOCK(): one CPU.
        // Report W^X failures
        let n = pr.ps_wxcounter.get();
        pr.ps_wxcounter.set(n + 1);
        if n == 0 {
            // log(LOG_NOTICE, ...): no syslog yet; see the module's deviations.
            let _ = unported!("uvm_wxcheck: log (subr_log.c)");
            crate::kprintf!(
                "{}({}): {} W^X violation\n",
                core::str::from_utf8(pr.comm()).unwrap_or("?"),
                pr.ps_pid.get(),
                call
            );
        }
        // Send uncatchable SIGABRT for coredump
        sigexit(p, SIGABRT);
    }

    Err(Errno::ENOTSUP)
}

/// `sys_mmap`: mmap system call.
///
/// File offset and address may not be page aligned:
/// - if `MAP_FIXED`, offset and address must have remainder mod `PAGE_SIZE`
/// - if address isn't page aligned the mapping starts at `trunc_page(addr)` and the return
///   value is adjusted up by the page offset.
pub fn sys_mmap(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMmapArgs = sysargs(v);

    // first, extract syscall args from the uap.
    let mut addr = uap.addr.get() as usize;
    let size = uap.len.get();
    let prot = uap.prot.get();
    let mut flags = uap.flags.get();
    let fd = uap.fd.get();
    let pos = uap.pos.get();
    let vm_min_address = <Machine as VmParam>::VM_MIN_ADDRESS;
    let vm_maxuser_address = <Machine as VmParam>::VM_MAXUSER_ADDRESS;

    // Validate the flags.
    if prot & PROT_MASK != prot {
        return Err(Errno::EINVAL);
    }
    if prot & (PROT_WRITE | PROT_EXEC) == (PROT_WRITE | PROT_EXEC) {
        uvm_wxcheck(p, "mmap")?;
    }

    if flags & MAP_FLAGMASK != flags {
        return Err(Errno::EINVAL);
    }
    if flags & (MAP_SHARED | MAP_PRIVATE) == (MAP_SHARED | MAP_PRIVATE) {
        return Err(Errno::EINVAL);
    }
    if flags & (MAP_FIXED | __MAP_NOREPLACE) == __MAP_NOREPLACE {
        return Err(Errno::EINVAL);
    }
    if flags & MAP_STACK != 0 {
        if flags & (MAP_ANON | MAP_PRIVATE) != (MAP_ANON | MAP_PRIVATE) {
            return Err(Errno::EINVAL);
        }
        if flags & !(MAP_STACK | MAP_FIXED | MAP_ANON | MAP_PRIVATE) != 0 {
            return Err(Errno::EINVAL);
        }
        if pos != 0 {
            return Err(Errno::EINVAL);
        }
        if prot & (PROT_READ | PROT_WRITE) != (PROT_READ | PROT_WRITE) {
            return Err(Errno::EINVAL);
        }
    }
    if size == 0 {
        return Err(Errno::EINVAL);
    }

    pledge_protexec(p, prot)?;

    // align file position and save offset. adjust size.
    let (_pos, size, pageoff) = align_addr(pos as usize, size)?;

    // now check (MAP_FIXED) or get (!MAP_FIXED) the "addr"
    if flags & MAP_FIXED != 0 {
        // adjust address by the same amount as we did the offset
        addr = addr.wrapping_sub(pageoff);
        if addr & PAGE_MASK != 0 {
            return Err(Errno::EINVAL); // not page aligned
        }

        if addr > usize::MAX - size {
            return Err(Errno::EINVAL); // no wrapping!
        }
        if vm_maxuser_address > 0 && addr + size > vm_maxuser_address {
            return Err(Errno::EINVAL);
        }
        if vm_min_address > 0 && addr < vm_min_address {
            return Err(Errno::EINVAL);
        }
    }

    // check for file mappings (i.e. not anonymous) and verify file.
    if flags & MAP_ANON == 0 {
        // fd_getfile(fdp, fd), the vnode checks, uvm_mmapfile: see the module's deviations.
        return Err(unported!(
            "sys_mmap: file mappings (fd_getfile, kern_descrip.c)"
        ));
    }

    // MAP_ANON case
    if fd != -1 {
        return Err(Errno::EINVAL);
    }

    // is_anon: label for SunOS style /dev/zero

    // __MAP_NOFAULT only makes sense with a backing object
    if flags & __MAP_NOFAULT != 0 {
        return Err(Errno::EINVAL);
    }

    if prot != PROT_NONE || flags & MAP_SHARED != 0 {
        let limit = lim_cur(RLIMIT_DATA) as usize;
        if limit < size || limit - size < ptoa(p.vmspace().vm_dused.get() as usize) {
            return Err(Errno::ENOMEM);
        }
    }

    // We've been treating (MAP_SHARED|MAP_PRIVATE) == 0 as MAP_PRIVATE, so make that clear.
    if flags & MAP_SHARED == 0 {
        flags |= MAP_PRIVATE;
    }

    let maxprot = PROT_MASK;
    uvm_mmapanon(
        &p.vmspace().vm_map,
        &mut addr,
        size,
        prot,
        maxprot,
        flags,
        lim_cur(RLIMIT_MEMLOCK) as usize,
        p,
    )?;

    // remember to add offset
    retval[0] = (addr + pageoff) as Register;
    Ok(())
}

/// `sys_msync`: the msync system call (a front-end for flush).
pub fn sys_msync(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMsyncArgs = sysargs(v);

    // extract syscall args from the uap
    let addr = uap.addr.get() as usize;
    let size = uap.len.get();
    let mut flags = uap.flags.get();

    // sanity check flags
    if flags & !(MS_ASYNC | MS_SYNC | MS_INVALIDATE) != 0
        || flags & (MS_ASYNC | MS_SYNC | MS_INVALIDATE) == 0
        || flags & (MS_ASYNC | MS_SYNC) == (MS_ASYNC | MS_SYNC)
    {
        return Err(Errno::EINVAL);
    }
    if flags & (MS_ASYNC | MS_SYNC) == 0 {
        flags |= MS_SYNC;
    }

    // align the address to a page boundary, and adjust the size accordingly
    let (addr, size, _) = align_addr(addr, size)?;
    if addr > usize::MAX - size {
        return Err(Errno::EINVAL); // disallow wrap-around.
    }

    // translate MS_ flags into PGO_ flags
    let mut uvmflags = PGO_CLEANIT;
    if flags & MS_INVALIDATE != 0 {
        uvmflags |= PGO_FREE;
    }
    // MS_SYNC, and MS_ASYNC too: XXXCDC: force sync for now!
    uvmflags |= PGO_SYNCIO;

    uvm_map_clean(&p.vmspace().vm_map, addr, addr + size, uvmflags)
}

/// `sys_munmap`: unmap a users memory.
pub fn sys_munmap(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMunmapArgs = sysargs(v);
    let vm_min_address = <Machine as VmParam>::VM_MIN_ADDRESS;
    let vm_maxuser_address = <Machine as VmParam>::VM_MAXUSER_ADDRESS;

    // get syscall args...
    let addr = uap.addr.get() as usize;
    let size = uap.len.get();

    // align address to a page boundary, and adjust size accordingly
    let (addr, size, _) = align_addr(addr, size)?;

    // Check for illegal addresses. Watch out for address wrap...
    if addr > usize::MAX - size {
        return Err(Errno::EINVAL);
    }
    if vm_maxuser_address > 0 && addr + size > vm_maxuser_address {
        return Err(Errno::EINVAL);
    }
    if vm_min_address > 0 && addr < vm_min_address {
        return Err(Errno::EINVAL);
    }
    let map = &p.vmspace().vm_map;

    vm_map_lock(map); // lock map so we can checkprot

    // interesting system call semantic: make sure entire range is allocated before allowing
    // an unmap.
    if !uvm_map_checkprot(map, addr, addr + size, PROT_NONE) {
        vm_map_unlock(map);
        return Err(Errno::EINVAL);
    }

    let dead_entries = UvmMapDeadq::new();
    if uvm_unmap_remove(map, addr, addr + size, &dead_entries, false, true, true).is_err() {
        vm_map_unlock(map);
        return Err(Errno::EPERM); // immutable entries found
    }
    vm_map_unlock(map); // and unlock

    uvm_unmap_detach(&dead_entries, 0);

    Ok(())
}

/// `sys_mprotect`: the mprotect system call.
pub fn sys_mprotect(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMprotectArgs = sysargs(v);

    // extract syscall args from uap
    let addr = uap.addr.get() as usize;
    let size = uap.len.get();
    let prot = uap.prot.get();

    if prot & PROT_MASK != prot {
        return Err(Errno::EINVAL);
    }
    if prot & (PROT_WRITE | PROT_EXEC) == (PROT_WRITE | PROT_EXEC) {
        uvm_wxcheck(p, "mprotect")?;
    }

    pledge_protexec(p, prot)?;

    // align the address to a page boundary, and adjust the size accordingly
    let (addr, size, _) = align_addr(addr, size)?;
    if addr > usize::MAX - size {
        return Err(Errno::EINVAL); // disallow wrap-around.
    }

    uvm_map_protect(&p.vmspace().vm_map, addr, addr + size, prot, 0, false, true)
}

/// `sys_pinsyscalls`. The caller is required to normalize base,len to the minimum .text
/// region, and adjust pintable offsets relative to that base.
pub fn sys_pinsyscalls(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPinsyscallsArgs = sysargs(v);
    let pr = p.process();
    let map = &p.vmspace().vm_map;

    // Must be called before any threads are created
    if p_hassibling(p) {
        return Err(Errno::EPERM);
    }

    // Only allow libc syscall pinning once per process
    let pmap_ = &pr.vmspace().vm_map;
    mtx_enter(&pmap_.flags_lock);
    let map_flags = pmap_.flags.get();
    pmap_.flags.set(map_flags | VM_MAP_PINSYSCALL_ONCE);
    mtx_leave(&pmap_.flags_lock);
    if map_flags & VM_MAP_PINSYSCALL_ONCE != 0 {
        return Err(Errno::EPERM);
    }

    let base = uap.base.get() as usize;
    let len = uap.len.get();
    if base > usize::MAX - len {
        return Err(Errno::EINVAL); // disallow wrap-around.
    }
    if base < map.min_offset.get() || base + len > map.max_offset.get() {
        return Err(Errno::EINVAL);
    }

    let npins = uap.npins.get();
    if npins < 1 || npins as usize > SYS_MAXSYSCALL {
        return Err(Errno::E2BIG);
    }
    let npins_u = npins as usize;
    let Some(mem) = mallocarray(npins_u, size_of::<u32>(), M_PINSYSCALL, M_WAITOK | M_ZERO) else {
        return Err(Errno::ENOMEM);
    };
    // SAFETY: a fresh allocation of `npins` u32s, ours until freed or handed to ps_libcpin.
    let pins: &mut [u32] =
        unsafe { core::slice::from_raw_parts_mut(mem.cast::<u32>().as_ptr(), npins_u) };
    let mut bytes = [0u8; 4];
    let mut error = Ok(());
    for (i, pin) in pins.iter_mut().enumerate() {
        if let Err(e) = copyin(uap.pins.get() as usize + i * 4, &mut bytes) {
            error = Err(e);
            break;
        }
        *pin = u32::from_ne_bytes(bytes);
    }

    // Range-check pintable offsets
    if error.is_ok() {
        for &pin in pins.iter() {
            if pin == u32::MAX || pin == 0 {
                continue;
            }
            if pin as usize > len {
                error = Err(Errno::ERANGE);
                break;
            }
        }
    }
    if let Err(e) = error {
        // err:
        free(mem, M_PINSYSCALL, npins_u * size_of::<u32>());
        return Err(e);
    }
    pr.ps_libcpin.pn_start.set(base);
    pr.ps_libcpin.pn_end.set(base + len);
    pr.ps_libcpin.pn_pins.set(mem.cast::<u32>().as_ptr());
    pr.ps_libcpin.pn_npins.set(npins);

    // PMAP_CHECK_COPYIN: not configured.
    Ok(())
}

/// `sys_mimmutable`: the mimmutable system call.
pub fn sys_mimmutable(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMimmutableArgs = sysargs(v);
    let addr = uap.addr.get() as usize;
    let size = uap.len.get();

    // align the address to a page boundary, and adjust the size accordingly
    let (addr, size, _) = align_addr(addr, size)?;
    if addr > usize::MAX - size {
        return Err(Errno::EINVAL); // disallow wrap-around.
    }

    uvm_map_immutable(&p.vmspace().vm_map, addr, addr + size, true)
}

/// `sys_minherit`: the minherit system call.
pub fn sys_minherit(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMinheritArgs = sysargs(v);
    let addr = uap.addr.get() as usize;
    let size = uap.len.get();
    let inherit = uap.inherit.get();

    // align the address to a page boundary, and adjust the size accordingly
    let (addr, size, _) = align_addr(addr, size)?;
    if addr > usize::MAX - size {
        return Err(Errno::EINVAL); // disallow wrap-around.
    }

    uvm_map_inherit(&p.vmspace().vm_map, addr, addr + size, inherit)
}

/// `sys_madvise`: give advice about memory usage.
pub fn sys_madvise(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMadviseArgs = sysargs(v);
    let addr = uap.addr.get() as usize;
    let size = uap.len.get();
    let advice = uap.behav.get();

    // align the address to a page boundary, and adjust the size accordingly
    let (addr, size, _) = align_addr(addr, size)?;
    if addr > usize::MAX - size {
        return Err(Errno::EINVAL); // disallow wrap-around.
    }

    let map = &p.vmspace().vm_map;
    match advice {
        MADV_NORMAL | MADV_RANDOM | MADV_SEQUENTIAL => {
            uvm_map_advice(map, addr, addr + size, advice)
        }

        // Activate all these pages, pre-faulting them in if necessary.
        //
        // XXX IMPLEMENT ME. Should invent a "weak" mode for uvm_fault() which would only do
        // the PGO_LOCKED pgo_get().
        MADV_WILLNEED => Ok(()),

        // Deactivate all these pages. We don't need them any more. We don't, however, toss
        // the data in the pages.
        MADV_DONTNEED => uvm_map_clean(map, addr, addr + size, PGO_DEACTIVATE),

        // These pages contain no valid data, and may be garbage-collected. Toss all
        // resources, including any swap space in use.
        MADV_FREE => uvm_map_clean(map, addr, addr + size, PGO_FREE),

        // XXXMRG What is this? I think it's: Ensure that we have allocated backing-store for
        // these pages. This is going to require changes to the page daemon, as it will free
        // swap space allocated to pages in core. There's also what to do for
        // device/file/anonymous memory.
        MADV_SPACEAVAIL => Err(Errno::EINVAL),

        _ => Err(Errno::EINVAL),
    }
}

/// `sys_mlock`: memory lock.
pub fn sys_mlock(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMlockArgs = sysargs(v);

    // extract syscall args from uap
    let addr = uap.addr.get() as usize;
    let size = uap.len.get();

    // align address to a page boundary and adjust size accordingly
    let (addr, size, _) = align_addr(addr, size)?;
    if addr > usize::MAX - size {
        return Err(Errno::EINVAL); // disallow wrap-around.
    }

    if atop(size) as i64 + UVMEXP.wired.load(Ordering::Relaxed) as i64
        > UVMEXP.wiredmax.load(Ordering::Relaxed) as i64
    {
        return Err(Errno::EAGAIN);
    }

    // pmap_wired_count exists on both machines.
    let map = &p.vmspace().vm_map;
    if (size + ptoa(pmap_wired_count(map.pmap()) as usize)) as u64 > lim_cur(RLIMIT_MEMLOCK) {
        return Err(Errno::EAGAIN);
    }

    uvm_map_pageable(map, addr, addr + size, false, 0).map_err(|_| Errno::ENOMEM)
}

/// `sys_munlock`: unlock wired pages.
pub fn sys_munlock(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMunlockArgs = sysargs(v);

    // extract syscall args from uap
    let addr = uap.addr.get() as usize;
    let size = uap.len.get();

    // align address to a page boundary, and adjust size accordingly
    let (addr, size, _) = align_addr(addr, size)?;
    if addr > usize::MAX - size {
        return Err(Errno::EINVAL); // disallow wrap-around.
    }

    // pmap_wired_count exists: no suser check.
    uvm_map_pageable(&p.vmspace().vm_map, addr, addr + size, true, 0).map_err(|_| Errno::ENOMEM)
}

/// `sys_mlockall`: lock all pages mapped into an address space.
pub fn sys_mlockall(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMlockallArgs = sysargs(v);
    let flags = uap.flags.get();

    if flags == 0 || flags & !(MCL_CURRENT | MCL_FUTURE) != 0 {
        return Err(Errno::EINVAL);
    }

    // pmap_wired_count exists: no suser check.
    match uvm_map_pageable_all(&p.vmspace().vm_map, flags, lim_cur(RLIMIT_MEMLOCK) as usize) {
        Err(e) if e != Errno::ENOMEM => Err(Errno::EAGAIN),
        r => r,
    }
}

/// `sys_munlockall`: unlock all pages mapped into an address space.
pub fn sys_munlockall(p: &Proc, _v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let _ = uvm_map_pageable_all(&p.vmspace().vm_map, 0, 0);
    Ok(())
}

/// `uvm_mmaplock`: common code for mmapanon and mmapfile to lock a mmaping.
pub fn uvm_mmaplock(
    map: &VmMap,
    addr: &mut usize,
    size: usize,
    prot: VmProt,
    locklimit: usize,
) -> Result<(), Errno> {
    // POSIX 1003.1b -- if our address space was configured to lock all future mappings,
    // wire the one we just made.
    if prot == PROT_NONE {
        // No more work to do in this case.
        return Ok(());
    }

    vm_map_lock(map);
    if map.flags.get() & VM_MAP_WIREFUTURE != 0 {
        // KERNEL_LOCK(): one CPU.
        if atop(size) as i64 + UVMEXP.wired.load(Ordering::Relaxed) as i64
            > UVMEXP.wiredmax.load(Ordering::Relaxed) as i64
            || (locklimit != 0 && size + ptoa(pmap_wired_count(map.pmap()) as usize) > locklimit)
        {
            vm_map_unlock(map);
            // unmap the region!
            uvm_unmap(map, *addr, *addr + size);
            return Err(Errno::ENOMEM);
        }
        // uvm_map_pageable() always returns the map unlocked.
        if let Err(e) = uvm_map_pageable(map, *addr, *addr + size, false, UVM_LK_ENTER) {
            // unmap the region!
            uvm_unmap(map, *addr, *addr + size);
            return Err(e);
        }
        return Ok(());
    }
    vm_map_unlock(map);
    Ok(())
}

/// `uvm_mmapanon`: internal version of mmap for anons, used by `sys_mmap`.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uvm_mmapanon(
    map: &VmMap,
    addr: &mut usize,
    size: usize,
    prot: VmProt,
    maxprot: VmProt,
    flags: i32,
    locklimit: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    let advice = MADV_NORMAL;
    let mut uvmflag = 0;
    let mut align = 0; // userland page size

    // for non-fixed mappings, round off the suggested address. for fixed mappings, check
    // alignment and zap old mappings.
    if flags & MAP_FIXED == 0 {
        *addr = round_page(*addr); // round
    } else {
        if *addr & PAGE_MASK != 0 {
            return Err(Errno::EINVAL);
        }

        uvmflag |= UVM_FLAG_FIXED;
        if flags & __MAP_NOREPLACE == 0 {
            uvmflag |= UVM_FLAG_UNMAP;
        }
    }

    if flags & MAP_FIXED == 0 && size >= <Machine as MachineExec>::LDPGSZ {
        align = <Machine as MachineExec>::LDPGSZ;
    }
    if flags & MAP_SHARED == 0 {
        uvmflag |= UVM_FLAG_COPYONW;
    } else {
        uvmflag |= UVM_FLAG_OVERLAY;
    }
    if flags & MAP_STACK != 0 {
        uvmflag |= UVM_FLAG_STACK;
    }
    if flags & MAP_CONCEAL != 0 {
        uvmflag |= UVM_FLAG_CONCEAL;
    }

    // set up mapping flags
    let uvmflag = uvm_mapflag(
        prot,
        maxprot,
        if flags & MAP_SHARED != 0 {
            MAP_INHERIT_SHARE
        } else {
            MAP_INHERIT_COPY
        },
        advice,
        uvmflag,
    );

    uvm_mapanon(map, addr, size, align, uvmflag)?;
    uvm_mmaplock(map, addr, size, prot, locklimit)
}

/// `sys_kbind`: the lazy-binding update of `ld.so`: writes the new PLT/GOT data into the
/// caller's (possibly read-only) text through a temporary kernel mapping.
pub fn sys_kbind(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysKbindArgs = sysargs(v);
    let pr = p.process();
    const KBSZ: usize = size_of::<Kbind>();
    let vm_maxuser_address = <Machine as VmParam>::VM_MAXUSER_ADDRESS;

    // extract syscall args from uap
    let paramp = uap.param.get() as usize;
    let psize = uap.psize.get();

    // If paramp is NULL and we're uninitialized, disable the syscall for the process. Raise
    // SIGILL if paramp is NULL and we're already initialized.
    //
    // If paramp is non-NULL and we're uninitialized, do initialization. Otherwise, do
    // security checks and raise SIGILL on failure.
    let pc = <Machine as Cpu>::proc_pc(p);
    let mut sigill = false;
    mtx_enter(&pr.ps_mtx);
    if paramp == 0 {
        // ld.so disables kbind() when lazy binding is disabled
        if pr.ps_kbind_addr.get() == 0 {
            pr.ps_kbind_addr.set(BOGO_PC);
        }
        // pre-7.3 static binaries disable kbind
        // XXX delete check in 2026
        else if pr.ps_kbind_addr.get() != BOGO_PC {
            sigill = true;
        }
    } else if pr.ps_kbind_addr.get() == 0 {
        pr.ps_kbind_addr.set(pc);
        pr.ps_kbind_cookie.set(uap.proc_cookie.get());
    } else if pc != pr.ps_kbind_addr.get()
        || pc == BOGO_PC
        || pr.ps_kbind_cookie.get() != uap.proc_cookie.get()
    {
        sigill = true;
    }
    mtx_leave(&pr.ps_mtx);

    // Raise SIGILL if something is off.
    if sigill {
        // KERNEL_LOCK(): one CPU.
        sigexit(p, SIGILL);
    }

    // We're done if we were disabling the syscall.
    if paramp == 0 {
        return Ok(());
    }

    // union { struct __kbind uk[KBIND_BLOCK_MAX]; char upad[...]; } param
    const PARAM_SIZE: usize = KBIND_BLOCK_MAX * KBSZ + KBIND_DATA_MAX;
    if !(KBSZ..=PARAM_SIZE).contains(&psize) {
        return Err(Errno::EINVAL);
    }
    let mut param = [0u8; PARAM_SIZE];
    copyin(paramp, &mut param[..psize])?;

    let kbind_at = |i: usize| -> Kbind {
        let mut a = [0u8; 8];
        let mut s = [0u8; 8];
        a.copy_from_slice(&param[i * KBSZ..i * KBSZ + 8]);
        s.copy_from_slice(&param[i * KBSZ + 8..i * KBSZ + 16]);
        Kbind {
            kb_addr: usize::from_ne_bytes(a),
            kb_size: usize::from_ne_bytes(s),
        }
    };

    // The param argument points to an array of __kbind structures followed by the
    // corresponding new data areas for them. Verify that the sizes in the __kbind structures
    // add up to the total size and find the start of the new area.
    let mut s = psize;
    let mut count = 0;
    while s > 0 && count < KBIND_BLOCK_MAX {
        if s < KBSZ {
            return Err(Errno::EINVAL);
        }
        s -= KBSZ;

        let kb = kbind_at(count);
        let baseva = kb.kb_addr;
        let endva = baseva.wrapping_add(kb.kb_size).wrapping_sub(1);
        if kb.kb_addr == 0
            || kb.kb_size == 0
            || kb.kb_size > KBIND_DATA_MAX
            || baseva >= vm_maxuser_address
            || endva >= vm_maxuser_address
            || s < kb.kb_size
        {
            return Err(Errno::EINVAL);
        }

        s -= kb.kb_size;
        count += 1;
    }
    if s > 0 {
        return Err(Errno::EINVAL);
    }
    let mut data = count * KBSZ;

    // all looks good, so do the bindings
    let mut last_baseva = vm_maxuser_address;
    let mut kva = 0usize;
    let dead_entries = UvmMapDeadq::new();
    let mut error = Ok(());
    let kmap = kernel_map();
    'binds: for i in 0..count {
        let kb = kbind_at(i);
        let mut baseva = kb.kb_addr;
        let mut s = kb.kb_size;
        let mut pageoffset = baseva & PAGE_MASK;
        baseva = trunc_page(baseva);

        // hppa at least runs PLT entries over page edge
        let mut extra = (pageoffset + s) & PAGE_MASK;
        if extra > pageoffset {
            extra = 0;
        } else {
            s -= extra;
        }
        loop {
            // redo:
            // make sure the desired page is mapped into kernel_map
            if baseva != last_baseva {
                if kva != 0 {
                    vm_map_lock(kmap);
                    let _ = uvm_unmap_remove(
                        kmap,
                        kva,
                        kva + crate::sys::param::PAGE_SIZE,
                        &dead_entries,
                        false,
                        true,
                        false,
                    ); // XXX
                    vm_map_unlock(kmap);
                    kva = 0;
                }
                match uvm_map_extract(
                    &p.vmspace().vm_map,
                    baseva,
                    crate::sys::param::PAGE_SIZE,
                    UVM_EXTRACT_FIXPROT,
                ) {
                    Ok(k) => kva = k,
                    Err(e) => {
                        error = Err(e);
                        break 'binds;
                    }
                }
                last_baseva = baseva;
            }

            // do the update
            // SAFETY: `data .. data + s` lies within `param` (the sizes were checked above)
            // and `kva + pageoffset .. + s` within the page just extracted into kernel_map;
            // kcopy recovers from a fault.
            let copied = unsafe { kcopy(param[data..].as_ptr(), (kva + pageoffset) as *mut u8, s) };
            if let Err(e) = copied {
                error = Err(e);
                break 'binds;
            }
            data += s;

            if extra > 0 {
                baseva += crate::sys::param::PAGE_SIZE;
                s = extra;
                pageoffset = 0;
                extra = 0;
                continue;
            }
            break;
        }
    }

    if kva != 0 {
        vm_map_lock(kmap);
        let _ = uvm_unmap_remove(
            kmap,
            kva,
            kva + crate::sys::param::PAGE_SIZE,
            &dead_entries,
            false,
            true,
            false,
        ); // XXX
        vm_map_unlock(kmap);
    }
    uvm_unmap_detach(&dead_entries, AMAP_REFALL);

    error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn align_addr_rounds_and_reports_the_offset() {
        assert_eq!(align_addr(0x1000, 0x10), Ok((0x1000, 0x1000, 0)));
        assert_eq!(align_addr(0x1234, 0x10), Ok((0x1000, 0x1000, 0x234)));
        assert_eq!(align_addr(0x1ff0, 0x20), Ok((0x1000, 0x2000, 0xff0)));
        assert_eq!(align_addr(0x1000, 0), Ok((0x1000, 0, 0)));
        assert_eq!(align_addr(0x1001, usize::MAX), Err(Errno::EINVAL));
        assert_eq!(align_addr(0, usize::MAX - 10), Err(Errno::EINVAL));
    }
}
