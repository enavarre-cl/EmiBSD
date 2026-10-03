/*	$OpenBSD: user.h,v 1.12 2025/11/10 12:34:52 dlg Exp $	*/
/*	$NetBSD: user.h,v 1.11 1996/04/22 01:23:44 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1991, 1993
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
 *	@(#)user.h	8.2 (Berkeley) 9/23/93
 */
/* </LICENSES> */

//! `<sys/user.h>`: per process structure containing data that isn't needed in core when the
//! process isn't running (esp. when swapped out). This structure may or may not be at the
//! same kernel address in all processes.
//!
//! Upstream: sys/sys/user.h @ 3ce1f3f79392
//!
//! Status: `ported` (M5). The u-area is the first part of a thread's kernel stack
//! (`USPACE` bytes from `p_addr`).

use crate::machine::Machine;
use crate::machine::proc::{MachineProc, Pcb};

/// `struct user`.
#[repr(C)]
pub struct User {
    /// `u_pcb`.
    pub u_pcb: Pcb,
}

impl User {
    /// A zero u-area.
    pub const fn new() -> Self {
        Self {
            u_pcb: <Machine as MachineProc>::PCB_INIT,
        }
    }
}

impl Default for User {
    fn default() -> Self {
        Self::new()
    }
}
