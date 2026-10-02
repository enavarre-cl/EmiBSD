/*	$OpenBSD: systm.h,v 1.179 2026/04/22 01:51:37 jsg Exp $	*/
/*	$NetBSD: systm.h,v 1.50 1996/06/09 04:55:09 briggs Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1988, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)systm.h	8.4 (Berkeley) 2/23/94
 */
/* </LICENSES> */

//! `<sys/systm.h>`: the kernel's global declarations.
//!
//! Upstream: sys/sys/systm.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports `physmem`; the hostname and boot-time globals, the
//! `copyin`/`copyout` family, `tsleep`/`wakeup`, the `panic`/`printf` prototypes (already in
//! `kern/subr_prf.rs`) and the rest arrive with their files.
//!
//! ## Deviations
//! - `physmem` is defined here (the C defines it in every `machdep.c` and declares it here),
//!   so generic code names it without an architecture path; the `machdep`s fill it.

use core::sync::atomic::AtomicUsize;

/// `physmem`: physical memory, in pages (an `int` in C).
pub static PHYSMEM: AtomicUsize = AtomicUsize::new(0);
