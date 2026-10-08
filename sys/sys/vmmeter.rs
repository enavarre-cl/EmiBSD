/*	$OpenBSD: vmmeter.h,v 1.15 2016/07/27 14:44:59 tedu Exp $	*/
/*	$NetBSD: vmmeter.h,v 1.9 1995/03/26 20:25:04 jtc Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)vmmeter.h	8.2 (Berkeley) 7/10/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/vmmeter.h>`: system wide statistics counters. Look in `uvm/uvm_extern.rs` for the
//! UVM equivalent.
//!
//! Upstream: sys/sys/vmmeter.h @ 3ce1f3f79392
//!
//! Status: `ported` (M5). `forkstat` itself lives in `kern/kern_fork.rs`, `vmtotal` in
//! `uvm/uvm_meter.c` (M7).
//!
//! ## Deviations
//! - The fork counters are atomics: `fork1` bumps them under the kernel lock in C, which
//!   this kernel does not have yet.

use core::sync::atomic::{AtomicU32, AtomicU64};

/// `struct vmtotal`: systemwide totals computed every five seconds.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Vmtotal {
    /// `t_rq`: length of the run queue.
    pub t_rq: u16,
    /// `t_dw`: jobs in ``disk wait'' (neg priority).
    pub t_dw: u16,
    /// `t_pw`: jobs in page wait.
    pub t_pw: u16,
    /// `t_sl`: jobs sleeping in core.
    pub t_sl: u16,
    /// `t_sw`: swapped out runnable/short block jobs.
    pub t_sw: u16,
    /// `t_vm`: total virtual memory.
    pub t_vm: u32,
    /// `t_avm`: active virtual memory.
    pub t_avm: u32,
    /// `t_rm`: total real memory in use.
    pub t_rm: u32,
    /// `t_arm`: active real memory.
    pub t_arm: u32,
    /// `t_vmshr`: shared virtual memory.
    pub t_vmshr: u32,
    /// `t_avmshr`: active shared virtual memory.
    pub t_avmshr: u32,
    /// `t_rmshr`: shared real memory.
    pub t_rmshr: u32,
    /// `t_armshr`: active shared real memory.
    pub t_armshr: u32,
    /// `t_free`: free memory pages.
    pub t_free: u32,
}

/// `struct forkstat`: fork/vfork/__tfork/kthread statistics.
pub struct Forkstat {
    /// `cntfork`: number of fork() calls.
    pub cntfork: AtomicU32,
    /// `cntvfork`: number of vfork() calls.
    pub cntvfork: AtomicU32,
    /// `cnttfork`: number of __tfork() calls.
    pub cnttfork: AtomicU32,
    /// `cntkthread`: number of kernel threads created.
    pub cntkthread: AtomicU32,
    /// `sizfork`: VM pages affected by fork().
    pub sizfork: AtomicU64,
    /// `sizvfork`: VM pages affected by vfork().
    pub sizvfork: AtomicU64,
    /// `siztfork`: VM pages affected by __tfork().
    pub siztfork: AtomicU64,
    /// `sizkthread`: VM pages affected by kernel thread creation.
    pub sizkthread: AtomicU64,
}

impl Forkstat {
    /// All counters at zero.
    pub const fn new() -> Self {
        Self {
            cntfork: AtomicU32::new(0),
            cntvfork: AtomicU32::new(0),
            cnttfork: AtomicU32::new(0),
            cntkthread: AtomicU32::new(0),
            sizfork: AtomicU64::new(0),
            sizvfork: AtomicU64::new(0),
            siztfork: AtomicU64::new(0),
            sizkthread: AtomicU64::new(0),
        }
    }
}

impl Default for Forkstat {
    fn default() -> Self {
        Self::new()
    }
}
/* </CODE> */
