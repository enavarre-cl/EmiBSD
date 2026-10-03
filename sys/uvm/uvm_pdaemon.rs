/*	$OpenBSD: uvm_pdaemon.c,v 1.162 2026/08/31 17:06:54 kettenis Exp $	*/
/*	$NetBSD: uvm_pdaemon.c,v 1.23 2000/08/20 10:24:14 bjh21 Exp $	*/
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
 *	@(#)vm_pageout.c        8.5 (Berkeley) 2/14/94
 * from: Id: uvm_pdaemon.c,v 1.1.2.32 1998/02/06 05:26:30 chs Exp
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

//! The page daemon: `uvm/uvm_pdaemon.c`.
//!
//! Upstream: sys/uvm/uvm_pdaemon.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 only needs a `uvm_wait` the allocator can name; it reports the
//! gap, because there is no daemon to wait for before kernel threads exist (M5). The daemon
//! itself (`uvm_pageout`, `uvmpd_scan`, the aiodone daemon) arrives with swapping (M7).
//! `uvmpd_tune` is ported (M8): `uvm_pageout` calls it first thing, and since the daemon
//! thread is not created, `main` calls it where the C forks the daemon, so `wiredmax`,
//! `freemin` and `freetarg` have their values (`sysctl(2)` and `mlock(2)` compare against
//! `wiredmax`).
//!
//! ## Deviations
//! - `uvmpd_tune` runs from `main` instead of from the page daemon's first loop.

use core::sync::atomic::Ordering;

use crate::sys::param::PAGE_SHIFT;
use crate::unported;
use crate::uvm::uvm_init::UVMEXP;

/// `uvm_wait`: wait for the page daemon to free memory; nothing can be waited for yet.
pub fn uvm_wait(_wmsg: &str) {
    let _ = unported!("uvm_wait (uvm_pdaemon.c, M5)");
}

/// `uvmpd_tune`: tune paging parameters.
pub fn uvmpd_tune() {
    let npages = UVMEXP.npages.load(Ordering::Relaxed);
    let mut val = npages / 30;

    // XXX: what are these values good for?
    val = val.max((16 * 1024) >> PAGE_SHIFT);

    // Make sure there's always a user page free.
    let reserve_kernel = UVMEXP.reserve_kernel.load(Ordering::Relaxed);
    if val < reserve_kernel + 1 {
        val = reserve_kernel + 1;
    }
    UVMEXP.freemin.store(val, Ordering::Relaxed);

    // Calculate free target.
    let freemin = UVMEXP.freemin.load(Ordering::Relaxed);
    let mut val = (freemin * 4) / 3;
    if val <= freemin {
        val = freemin + 1;
    }
    UVMEXP.freetarg.store(val, Ordering::Relaxed);

    UVMEXP.wiredmax.store(npages / 3, Ordering::Relaxed);
}
