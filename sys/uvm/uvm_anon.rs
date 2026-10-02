/*	$OpenBSD: uvm_anon.h,v 1.24 2025/12/15 13:02:18 mpi Exp $	*/
/*	$NetBSD: uvm_anon.h,v 1.13 2000/12/27 09:17:04 chs Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Charles D. Cranor and Washington University.
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

//! Anonymous memory: `<uvm/uvm_anon.h>`.
//!
//! Upstream: sys/uvm/uvm_anon.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 needs `struct vm_anon` as the type a page may belong to;
//! `an_lock` (an `rwlock`, M5), `struct vm_aref`'s amap and the functions of `uvm_anon.c`
//! arrive with M6.

use core::cell::Cell;

use crate::uvm::uvm_page::VmPage;

/// `struct vm_anon`.
pub struct VmAnon {
    /// If in RAM.
    pub an_page: Cell<*const VmPage>,
    /// Reference count.
    pub an_ref: Cell<i32>,
    /// Drum swap slot # (if != 0) \[if we hold an_page, PG_BUSY\].
    pub an_swslot: Cell<i32>,
}

// SAFETY: `an_lock` (M5) guards every field; until then the single boot CPU is the lock.
unsafe impl Sync for VmAnon {}
