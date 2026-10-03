/*	$OpenBSD: uvm_swap.c,v 1.183 2026/07/11 13:13:16 kettenis Exp $	*/
/*	$NetBSD: uvm_swap.c,v 1.40 2000/11/17 11:39:39 mrg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1995, 1996, 1997 Matthew R. Green
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
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
 * BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED
 * AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * from: NetBSD: vm_swap.c,v 1.52 1997/12/02 13:47:37 pk Exp
 * from: Id: uvm_swap.c,v 1.1.2.42 1998/02/02 20:38:06 chuck Exp
 */
/* </LICENSES> */

//! `uvm_swap.c`: the swap system.
//!
//! Upstream: sys/uvm/uvm_swap.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M7a (part 3) has only what the fault handler asks:
//! [`uvm_swapisfull`] and `SWSLOT_BAD`. The swap devices, `uvm_swap_get`/`put`, the
//! slot allocator and `uvm_swap_markbad` are M7 (the pagedaemon); until then the counters
//! they would move stay at zero, so "swap is full" is what every out-of-memory path sees.
//!
//! ## Deviations
//! - `uvm_swap_data_lock` is not taken: nothing else writes the two counters yet.

use core::sync::atomic::Ordering;

use crate::kassert;
use crate::uvm::uvm_init::UVMEXP;

/// `SWSLOT_BAD`: a swap slot the pager could not read; never freed, never used again.
pub const SWSLOT_BAD: i32 = -1;

/// `uvm_swapisfull`: return true if the amount of pages only in swap accounts for more than
/// 99% of the total swap space.
pub fn uvm_swapisfull() -> bool {
    // mtx_enter(&uvm_swap_data_lock): see the module's deviations.
    let swpgonly = UVMEXP.swpgonly.load(Ordering::Relaxed);
    let swpages = UVMEXP.swpages.load(Ordering::Relaxed);
    kassert!(swpgonly <= swpages);
    swpgonly >= swpages * 99 / 100
}
