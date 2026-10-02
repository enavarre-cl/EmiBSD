/*	$OpenBSD: kern_xxx.c,v 1.42 2025/06/16 20:21:33 kettenis Exp $	*/
/*	$NetBSD: kern_xxx.c,v 1.32 1996/04/22 01:38:41 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)kern_xxx.c	8.2 (Berkeley) 11/14/93
 */
/* </LICENSES> */

//! Odds and ends: `kern/kern_xxx.c`.
//!
//! Upstream: sys/kern/kern_xxx.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `reboot()` and `rebooting`, the tail of `panic(9)`.
//! `sys_reboot` (the system call), `__stack_smash_handler`, `scdebug_call`/`scdebug_ret` and
//! `sys_sysctl` arrive with their subsystems.
//!
//! ## Deviations
//! - `KASSERT((howto & RB_NOSYNC) || curproc != NULL)`: `curproc` arrives with M5; the
//!   assertion returns with it.
//! - `stop_periodic_resettodr()` (`kern_time.c`) is reported as unported.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::machine::cpu::boot;
use crate::unported;

/// `rebooting`: set once the system started to go down, for the benefit of code that must not
/// sleep any more.
pub static REBOOTING: AtomicBool = AtomicBool::new(false);

/// `reboot`: stops the clock bookkeeping and hands over to the machine's `boot(9)`.
pub fn reboot(howto: i32) -> ! {
    let _ = unported!("stop_periodic_resettodr");

    REBOOTING.store(true, Ordering::Relaxed);

    boot(howto)
}
