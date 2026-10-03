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
//! `kv_uarea`, `uvm_uarea_alloc` and `uvm_uarea_free`. `uvm_kernacc`, `uvm_vslock`,
//! `uvm_vsunlock`, `uvm_vslock_device`, `uvm_vsunlock_device`, `uvm_exit`, `uvm_init_limits`,
//! `uvm_atopg` and the swapper come with user mode and the pager (M6, M7).
//!
//! ## Deviations
//! - `__HAVE_USPACE_GUARD`'s guard page is not carved out yet: `km_alloc` hands out
//!   direct-map addresses until `kernel_map` exists (`uvm_map.c`, M6), and punching a hole
//!   in the direct map would unmap the page from everyone. The carve-out is reported once
//!   and the u-area has no guard until then.

use core::ptr::{self, NonNull};

use crate::kern::subr_prf::panic;
use crate::machine::Machine;
use crate::machine::cpu::curproc;
use crate::machine::param::MachineParam;
use crate::sys::param::{USPACE, USPACE_ALIGN};
use crate::sys::proc::Proc;
use crate::sys::proc::Process;
use crate::unported;
use crate::uvm::uvm_extern::{KmemVaMode, KvMap};
use crate::uvm::uvm_km::{KD_WAITOK, KP_ZERO, km_alloc, km_free};
use crate::uvm::uvm_map::{uvmspace_free, uvmspace_purge};

/// `kv_uarea`: u-areas come from `kernel_map`, `USPACE_ALIGN`ed.
pub static KV_UAREA: KmemVaMode = KmemVaMode {
    kv_map: KvMap::Kernel,
    kv_align: USPACE_ALIGN,
    kv_wait: false,
    kv_singlepage: false,
};

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
