/*	$OpenBSD: ttycom.h,v 1.17 2018/06/16 13:55:03 deraadt Exp $	*/
/*	$NetBSD: ttycom.h,v 1.4 1996/05/19 17:17:53 jonathan Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1990, 1993, 1994
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
 *	@(#)ttycom.h	8.1 (Berkeley) 3/28/94
 */
/* </LICENSES> */

//! `<sys/ttycom.h>`: the tty `ioctl(2)` commands.
//!
//! Upstream: sys/sys/ttycom.h @ 3ce1f3f79392
//!
//! Status: `wip`. `kern_sig.c` needs `TIOCGPGRP` and `TIOCSPGRP` (the process-group
//! variants of `sigio_getown`/`sigio_setown`); `struct winsize`, `struct tstamps` and the
//! other commands come with the tty layer (M7).

use crate::sys::ioccom::{_ior, _iow};

/// `TIOCGPGRP`: get pgrp of tty (`_IOR('t', 119, int)`).
pub const TIOCGPGRP: u64 = _ior::<i32>(b't', 119);
/// `TIOCSPGRP`: set pgrp of tty (`_IOW('t', 118, int)`).
pub const TIOCSPGRP: u64 = _iow::<i32>(b't', 118);
