/*	$OpenBSD: param.c,v 1.53 2025/08/06 14:00:33 mvs Exp $	*/
/*	$NetBSD: param.c,v 1.16 1996/03/12 03:08:40 mrg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1980, 1986, 1989 Regents of the University of California.
 * All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)param.c	7.20 (Berkeley) 6/27/91
 */
/* </LICENSES> */

//! Tunable kernel parameters that `config(8)` lets a kernel configuration override:
//! `sys/conf/param.c`.
//!
//! Upstream: sys/conf/param.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 needs the clock rate (`delay(9)` on amd64 divides by `hz`);
//! M5 adds `maxprocess`, `maxthread` and `maxfiles` from `MAXUSERS` (80, as `GENERIC`
//! configures); M7b adds `nmbclust` for the mbuf allocator and `kern_sysctl.c` `fscale`;
//! the buffer cache adds `bufcachepercent` and `bufpages`; the System V IPC limits and
//! `utsname` arrive with the subsystems that read them.
//!
//! ## Deviations
//! - The `int` and `long` globals are atomics: `sysctl(8)` writes them at runtime in OpenBSD
//!   too. `bufcachepercent` and `bufpages` keep their lowercase names beside the
//!   `BUFCACHEPERCENT`/`BUFPAGES` defaults.

use core::sync::atomic::{AtomicI32, AtomicI64};

use crate::sys::kernel::HZ as DEFAULT_HZ;
use crate::sys::param::NMBCLUSTERS;

/// `hz`: the system clock's frequency, in ticks per second.
pub static HZ: AtomicI32 = AtomicI32::new(DEFAULT_HZ);
/// `tick`: microseconds per tick.
pub static TICK: AtomicI32 = AtomicI32::new(1_000_000 / DEFAULT_HZ);
/// `tick_nsec`: nanoseconds per tick.
pub static TICK_NSEC: AtomicI32 = AtomicI32::new(1_000_000_000 / DEFAULT_HZ);
/// `utc_offset`: seconds east of UTC.
pub static UTC_OFFSET: AtomicI32 = AtomicI32::new(0);

/// `MAXUSERS`: what `GENERIC` configures (`maxusers 80`).
pub const MAXUSERS: i32 = 80;
/// `NPROCESS`.
pub const NPROCESS: i32 = 30 + 16 * MAXUSERS;
/// `NTEXT`: actually the object cache.
pub const NTEXT: i32 = 80 + NPROCESS / 8;
/// `NVNODE`.
pub const NVNODE: i32 = NPROCESS * 2 + NTEXT + 100;

/// `initialvnodes`: XXX number of vnodes to start (the vnode and name cache sizes).
pub static INITIALVNODES: AtomicI32 = AtomicI32::new(NVNODE);
/// \[a\] `maxprocess`.
pub static MAXPROCESS: AtomicI32 = AtomicI32::new(NPROCESS);
/// \[a\] `maxthread`.
pub static MAXTHREAD: AtomicI32 = AtomicI32::new(2 * NPROCESS);
/// \[a\] `maxfiles`.
pub static MAXFILES: AtomicI32 = AtomicI32::new(5 * (NPROCESS + MAXUSERS) + 80);
/// \[a\] `nmbclust`: the limit on the number of mbuf clusters.
pub static NMBCLUST: AtomicI64 = AtomicI64::new(NMBCLUSTERS as i64);

/// `fscale`: the kernel uses `FSCALE` (`sys/param.rs`), user programs read `fscale` through
/// `kern.fscale`.
pub static FSCALE: AtomicI32 = AtomicI32::new(crate::sys::param::FSCALE as i32);

/// `BUFCACHEPERCENT`: the default share of memory for the buffer cache.
pub const BUFCACHEPERCENT: i32 = 20;
/// `bufcachepercent`.
#[allow(non_upper_case_globals)] // BUFCACHEPERCENT is the default of the same name
pub static bufcachepercent: AtomicI32 = AtomicI32::new(BUFCACHEPERCENT);

/// `BUFPAGES`: the default buffer cache size in pages (0: `bufinit` decides).
pub const BUFPAGES: i64 = 0;
/// `bufpages`: max number of pages for buffers' data.
#[allow(non_upper_case_globals)] // BUFPAGES is the default of the same name
pub static bufpages: AtomicI64 = AtomicI64::new(BUFPAGES);
