/*	$OpenBSD: uvm_glue.c,v 1.95 2026/02/11 22:34:40 deraadt Exp $	*/
/*	$NetBSD: uvm_glue.c,v 1.44 2001/02/06 19:54:44 eeh Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Charles D. Cranor and Washington University.
 * Copyright (c) 1991, 1993, The Regents of the University of California.
 *
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * The Mach Operating System project at Carnegie-Mellon University.
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
 *	@(#)vm_glue.c	8.6 (Berkeley) 1/5/94
 * from: Id: uvm_glue.c,v 1.1.2.8 1998/02/07 01:16:54 chs Exp
 *
 *
 * Copyright (c) 1987, 1990 Carnegie-Mellon University.
 * All rights reserved.
 *
 * Permission to use, copy, modify and distribute this software and
 * its documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND
 * FOR ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */
/* </LICENSES> */

//! `uvm_glue.c`: glue functions between UVM and the rest of the kernel.
//!
//! Upstream: sys/uvm/uvm_glue.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part b2) ports the u-area allocator the fork path needs:
//! `kv_uarea`, `uvm_uarea_alloc` and `uvm_uarea_free`. M7a adds `uvm_init_limits`.
//! `kern_sysctl.c` brings `uvm_vslock` and `uvm_vsunlock`; M10a (physio) brings
//! `uvm_vslock_device` and `uvm_vsunlock_device`. `uvm_kernacc`, `uvm_atopg` and the swapper
//! come with the pager.
//!
//! ## Deviations
//! - `__HAVE_USPACE_GUARD`'s guard page is not carved out yet: `km_alloc` hands out
//!   direct-map addresses until `kernel_map` exists (`uvm_map.c`, M6), and punching a hole
//!   in the direct map would unmap the page from everyone. The carve-out is reported once
//!   and the u-area has no guard until then.

use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::Ordering;

use crate::kern::subr_prf::panic;
use crate::machine::Machine;
use crate::machine::Pmap;
use crate::machine::VmParam;
use crate::machine::copy::{copyin, copyout};
use crate::machine::cpu::curproc;
use crate::machine::param::MachineParam;
use crate::machine::pmap::{pmap_extract, pmap_kenter_pa, pmap_kernel, pmap_kremove, pmap_update};
use crate::sys::errno::Errno;
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::param::PAGE_SIZE;
use crate::sys::param::{USPACE, USPACE_ALIGN};
use crate::sys::proc::Proc;
use crate::sys::proc::Process;
use crate::sys::resource::{RLIMIT_DATA, RLIMIT_RSS, RLIMIT_STACK};
use crate::sys::resourcevar::Plimit;
use crate::sys::types::{Paddr, Rlim, Vaddr, Vsize};
use crate::uvm::uvm_extern::{KmemVaMode, KvMap, UVM_PLA_WAITOK, VmProt};
use crate::uvm::uvm_fault::{uvm_fault_unwire_locked, uvm_fault_wire};
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_km::{
    KD_NOWAIT, KD_WAITOK, KP_NONE, KP_ZERO, KV_ANY, km_alloc, km_free, uvm_km_pgremove_intrsafe,
};
use crate::uvm::uvm_map::{
    uvm_map_pageable, uvmspace_free, uvmspace_purge, vm_map_lock_read, vm_map_unlock_read,
};
use crate::uvm::uvm_page::{Pglist, paddr_is_dma_reachable, uvm_pglistalloc, vm_page_to_phys};
use crate::uvm::uvm_param::{atop, ptoa, round_page, trunc_page};
use crate::{kassert, unported};

/// `kv_uarea`: u-areas come from `kernel_map`, `USPACE_ALIGN`ed.
pub static KV_UAREA: KmemVaMode = KmemVaMode {
    kv_map: KvMap::Kernel,
    kv_align: USPACE_ALIGN,
    kv_wait: false,
    kv_singlepage: false,
};

/// `uvm_vslock`: wires the user memory `[addr, addr + len)` of `p` for I/O, so that it
/// cannot fault while a lock is held (`sys_sysctl`). `access_type` is the C's argument,
/// which the wiring does not use either.
pub fn uvm_vslock(p: &Proc, addr: usize, len: usize, access_type: VmProt) -> Result<(), Errno> {
    let _ = access_type;
    let map = &p.vmspace().vm_map;

    let start = trunc_page(addr);
    let end = round_page(addr.wrapping_add(len));
    if end <= start {
        return Err(Errno::EINVAL);
    }

    uvm_map_pageable(map, start, end, false, 0)
}

/// `uvm_vsunlock`: unwires the user memory wired by [`uvm_vslock`] (`sys_sysctl`).
pub fn uvm_vsunlock(p: &Proc, addr: usize, len: usize) {
    let map = &p.vmspace().vm_map;

    let start = trunc_page(addr);
    let end = round_page(addr.wrapping_add(len));
    kassert!(end > start);

    let _ = uvm_map_pageable(map, start, end, true, 0);
}

/// `uvm_vslock_device`: wire user memory, make sure it's device reachable and bounce if
/// necessary (physio).
///
/// On success the map of `p` stays read-locked until [`uvm_vsunlock_device`] and the
/// result is the C's `*retp`: `None` when the device reaches the user pages themselves,
/// otherwise the kernel bounce buffer (DMA-reachable pages holding a copy of the `len` user
/// bytes) that stands for `addr` during the transfer.
pub fn uvm_vslock_device(
    p: &Proc,
    addr: usize,
    len: usize,
    access_type: VmProt,
) -> Result<Option<NonNull<u8>>, Errno> {
    let map = &p.vmspace().vm_map;

    let start = trunc_page(addr);
    let end = round_page(addr.wrapping_add(len));
    let sz = end.wrapping_sub(start);
    let off = addr - start;
    if end <= start {
        return Err(Errno::EINVAL);
    }

    vm_map_lock_read(map);
    loop {
        // retry:
        let mapv = map.timestamp.get();
        vm_map_unlock_read(map);

        uvm_fault_wire(map, start, end, access_type)?;

        vm_map_lock_read(map);
        if mapv == map.timestamp.get() {
            break;
        }
    }

    let npages = atop(sz);
    let error = 'out_unwire: {
        let mut reachable = true;
        for i in 0..npages {
            let Some(pa) = pmap_extract(map.pmap(), Vaddr::new(start + ptoa(i))) else {
                break 'out_unwire Errno::EFAULT;
            };
            if !paddr_is_dma_reachable(pa) {
                reachable = false;
                break;
            }
        }
        if reachable {
            return Ok(None);
        }

        let Some(sva) = km_alloc(sz, &KV_ANY, &KP_NONE, &KD_NOWAIT) else {
            break 'out_unwire Errno::ENOMEM;
        };
        let sva = sva.as_ptr() as usize;

        let error = 'out_unmap: {
            let pgl = Pglist::new();
            pgl.init();
            let dma = <Machine as Pmap>::DMA_CONSTRAINT;
            if let Err(e) = uvm_pglistalloc(
                npages * PAGE_SIZE,
                dma.ucr_low,
                dma.ucr_high,
                Paddr::new(0),
                Paddr::new(0),
                &pgl,
                npages as i32,
                UVM_PLA_WAITOK,
            ) {
                break 'out_unmap e;
            }

            let mut va = sva;
            while let Some(pg) = pgl.first() {
                // SAFETY: `pg` is the head of `pgl`.
                unsafe { pgl.remove(pg) };
                // SAFETY: `va` is a page of the virtual-only range km_alloc just reserved for
                // us; `pg` a page uvm_pglistalloc just gave us.
                unsafe {
                    pmap_kenter_pa(Vaddr::new(va), vm_page_to_phys(pg), PROT_READ | PROT_WRITE)
                };
                va += PAGE_SIZE;
            }
            pmap_update(pmap_kernel());
            kassert!(va == sva + sz);
            let ret = (sva + off) as *mut u8;

            // SAFETY: `[ret, ret + len)` lies inside the `sz` bytes just mapped, which only
            // this request uses.
            let bounce = unsafe { slice::from_raw_parts_mut(ret, len) };
            match copyin(addr, bounce) {
                Ok(()) => return Ok(NonNull::new(ret)),
                Err(e) => {
                    uvm_km_pgremove_intrsafe(Vaddr::new(sva), Vaddr::new(sva + sz));
                    // SAFETY: the bounce range entered above; nothing uses it any more.
                    unsafe { pmap_kremove(Vaddr::new(sva), Vsize::new(sz)) };
                    pmap_update(pmap_kernel());
                    e
                }
            }
        };
        // out_unmap:
        if let Some(v) = NonNull::new(sva as *mut u8) {
            km_free(v, sz, &KV_ANY, &KP_NONE);
        }
        error
    };
    // out_unwire:
    uvm_fault_unwire_locked(map, start, end);
    vm_map_unlock_read(map);
    Err(error)
}

/// `uvm_vsunlock_device`: unwire user memory wired by [`uvm_vslock_device`], copying the
/// bounce buffer `map` (its result) back out to the user first, and drop the map lock it
/// kept.
pub fn uvm_vsunlock_device(p: &Proc, addr: usize, len: usize, map: Option<NonNull<u8>>) {
    let start = trunc_page(addr);
    let end = round_page(addr.wrapping_add(len));
    kassert!(end > start);
    let sz = end - start;

    if let Some(bounce) = map {
        // SAFETY: the bounce buffer uvm_vslock_device made holds `len` bytes at `bounce`,
        // ours until the km_free below.
        let bounce = unsafe { slice::from_raw_parts(bounce.as_ptr(), len) };
        // The C ignores the copyout's result too: the transfer's error is already known.
        let _ = copyout(bounce, addr);
    }

    let vm_map = &p.vmspace().vm_map;
    uvm_fault_unwire_locked(vm_map, start, end);
    vm_map_unlock_read(vm_map);

    let Some(bounce) = map else {
        return;
    };

    let kva = trunc_page(bounce.as_ptr() as usize);
    uvm_km_pgremove_intrsafe(Vaddr::new(kva), Vaddr::new(kva + sz));
    // SAFETY: the bounce range uvm_vslock_device entered; the transfer is over.
    unsafe { pmap_kremove(Vaddr::new(kva), Vsize::new(sz)) };
    pmap_update(pmap_kernel());
    if let Some(v) = NonNull::new(kva as *mut u8) {
        km_free(v, sz, &KV_ANY, &KP_NONE);
    }
}

/// `uvm_uarea_alloc`: allocates a u-area (`USPACE` bytes of zeroed, wired kernel memory for
/// a thread's `struct user` and kernel stack). `None` (the C's `0`) when memory is short.
pub fn uvm_uarea_alloc() -> Option<NonNull<u8>> {
    let va = km_alloc(USPACE, &KV_UAREA, &KP_ZERO, &KD_WAITOK)?;

    if <Machine as MachineParam>::HAVE_USPACE_GUARD {
        // Carve out a guard page between the PCB and the stack: pmap_extract(pmap_kernel(),
        // va + PAGE_SIZE), pmap_kremove(va + PAGE_SIZE, PAGE_SIZE), pmap_update,
        // uvm_pagefree(pg): see the module's deviations.
        let _ = unported!("uvm_uarea_alloc: the guard page (km_alloc from kernel_map, M6)");
    }

    Some(va)
}

/// `uvm_uarea_free`: frees `p`'s u-area and clears `p_addr`.
pub fn uvm_uarea_free(p: &Proc) {
    if let Some(va) = NonNull::new(p.p_addr.get().cast_mut().cast::<u8>()) {
        km_free(va, USPACE, &KV_UAREA, &KP_ZERO);
    }
    p.p_addr.set(ptr::null());
}

/// `uvm_purge`: teardown a virtual address space. If multi-threaded, must be called by the
/// last thread of a process.
pub fn uvm_purge() {
    let Some(p) = curproc() else {
        panic(format_args!("uvm_purge: no curproc"));
    };
    let vm = p.vmspace();

    // KERNEL_ASSERT_UNLOCKED(); __HAVE_PMAP_PURGE: neither amd64 nor arm64.
    uvmspace_purge(vm);
}

/// `uvm_exit`: exit a virtual address space.
pub fn uvm_exit(pr: &Process) {
    let vm = pr.ps_vmspace.get();

    pr.ps_vmspace.set(ptr::null());
    if !vm.is_null() {
        // SAFETY: the process's reference, which this drop releases; nothing else reads the
        // pointer after it was cleared above.
        uvmspace_free(unsafe { &*vm });
    }
}

/// `uvm_init_limits`: init per-process VM limits.
///
/// Set up the initial limits on process VM. Set the maximum resident set size to be all of
/// (reasonably) available memory. This causes any single, large process to start random page
/// replacement once it fills memory.
pub fn uvm_init_limits(limit0: &Plimit) {
    let set = |which: usize, cur: Option<Rlim>, max: Option<Rlim>| {
        let mut r = limit0.pl_rlimit[which].get();
        if let Some(c) = cur {
            r.rlim_cur = c;
        }
        if let Some(m) = max {
            r.rlim_max = m;
        }
        limit0.pl_rlimit[which].set(r);
    };
    set(
        RLIMIT_STACK,
        Some(<Machine as VmParam>::DFLSSIZ as Rlim),
        Some(<Machine as VmParam>::MAXSSIZ as Rlim),
    );
    set(
        RLIMIT_DATA,
        Some(<Machine as VmParam>::DFLDSIZ as Rlim),
        Some(<Machine as VmParam>::MAXDSIZ as Rlim),
    );
    set(
        RLIMIT_RSS,
        Some(ptoa(UVMEXP.free.load(Ordering::Relaxed) as usize) as Rlim),
        None,
    );
}
