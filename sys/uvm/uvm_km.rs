/*	$OpenBSD: uvm_km.c,v 1.160 2026/06/23 14:40:40 bluhm Exp $	*/
/*	$NetBSD: uvm_km.c,v 1.42 2001/01/14 02:10:01 thorpej Exp $	*/

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
 *	@(#)vm_kern.c   8.3 (Berkeley) 1/12/94
 * from: Id: uvm_km.c,v 1.1.2.14 1998/02/06 05:19:27 chs Exp
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

//! Kernel memory allocation and management: `uvm/uvm_km.c`.
//!
//! Upstream: sys/uvm/uvm_km.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 starts with the unconstrained range `no_constraint` and the
//! `kv_*`/`kp_*`/`kd_*` allocation modes; `km_alloc`/`km_free` over the direct map follow once
//! the pmaps map pages, and `uvm_km_init`, `uvm_km_suballoc` and `uvm_km_pgremove` with
//! `uvm_map`.

use crate::sys::types::Paddr;
use crate::uvm::uvm_extern::{
    KmemDynMode, KmemPaMode, KmemVaMode, KvMap, UVM_UNKNOWN_OFFSET, UvmConstraintRange,
};

/// `no_constraint`: unconstrained range.
pub static NO_CONSTRAINT: UvmConstraintRange = UvmConstraintRange {
    ucr_low: Paddr::new(0),
    ucr_high: Paddr::new(usize::MAX),
};

/// `kv_any`: any kernel virtual address, from `kernel_map`.
pub static KV_ANY: KmemVaMode = KmemVaMode {
    kv_map: KvMap::Kernel,
    kv_align: 0,
    kv_wait: false,
    kv_singlepage: false,
};

/// `kv_intrsafe`: from `kmem_map`, usable from interrupt handlers.
pub static KV_INTRSAFE: KmemVaMode = KmemVaMode {
    kv_map: KvMap::Kmem,
    kv_align: 0,
    kv_wait: false,
    kv_singlepage: false,
};

/// `kv_page`: the single page allocator.
pub static KV_PAGE: KmemVaMode = KmemVaMode {
    kv_map: KvMap::None,
    kv_align: 0,
    kv_wait: false,
    kv_singlepage: true,
};

const fn pa_mode(constraint: &'static UvmConstraintRange) -> KmemPaMode {
    KmemPaMode {
        kp_constraint: constraint,
        kp_object: false,
        kp_align: Paddr::new(0),
        kp_boundary: Paddr::new(0),
        kp_maxseg: 0,
        kp_nomem: false,
        kp_zero: false,
        kp_pageable: false,
    }
}

/// `kp_dirty`: any physical pages, not zeroed.
pub static KP_DIRTY: KmemPaMode = pa_mode(&NO_CONSTRAINT);

/// `kp_zero`: any physical pages, zeroed.
pub static KP_ZERO: KmemPaMode = KmemPaMode {
    kp_zero: true,
    ..pa_mode(&NO_CONSTRAINT)
};

/// `kp_mbuf_contig`: one physically contiguous segment.
pub static KP_MBUF_CONTIG: KmemPaMode = KmemPaMode {
    kp_maxseg: 1,
    ..pa_mode(&NO_CONSTRAINT)
};

/// `kp_pageable`: pageable memory from the kernel object. XXX - kp_nomem, maybe, but we'll need
/// to fix km_free.
pub static KP_PAGEABLE: KmemPaMode = KmemPaMode {
    kp_object: true,
    kp_pageable: true,
    ..pa_mode(&NO_CONSTRAINT)
};

/// `kp_none`: virtual space only, no backing pages.
pub static KP_NONE: KmemPaMode = KmemPaMode {
    kp_nomem: true,
    ..pa_mode(&NO_CONSTRAINT)
};

/// `kd_waitok`: may sleep.
pub static KD_WAITOK: KmemDynMode = KmemDynMode {
    kd_prefer: UVM_UNKNOWN_OFFSET,
    kd_slowdown: false,
    kd_waitok: true,
    kd_trylock: false,
};

/// `kd_nowait`: must not sleep.
pub static KD_NOWAIT: KmemDynMode = KmemDynMode {
    kd_prefer: UVM_UNKNOWN_OFFSET,
    kd_slowdown: false,
    kd_waitok: false,
    kd_trylock: false,
};

/// `kd_trylock`: must not sleep on map locks.
pub static KD_TRYLOCK: KmemDynMode = KmemDynMode {
    kd_prefer: UVM_UNKNOWN_OFFSET,
    kd_slowdown: false,
    kd_waitok: false,
    kd_trylock: true,
};
