/*	$OpenBSD: syslimits.h,v 1.16 2024/08/02 01:53:21 guenther Exp $	*/
/*	$NetBSD: syslimits.h,v 1.12 1995/10/05 05:26:19 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1988, 1993
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
 *	@(#)syslimits.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

//! POSIX system limits: `<sys/syslimits.h>`.
//!
//! Upstream: sys/sys/syslimits.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Every value is visible: the kernel compiles with `__BSD_VISIBLE`, which enables them all.
//! - Byte counts and object counts are `usize`. `LINK_MAX` is a [`Nlink`]; `NZERO` is an `i32`
//!   because nice values are signed; `BC_*_MAX` and `SEM_VALUE_MAX` keep the width of the
//!   `INT_MAX`/`UINT_MAX` they are defined from.

use crate::sys::types::Nlink;

/// Max bytes for an exec function.
pub const ARG_MAX: usize = 512 * 1024;
/// Max simultaneous processes.
pub const CHILD_MAX: usize = 80;
/// Max file link count.
pub const LINK_MAX: Nlink = 32767;
/// Max bytes in a terminal canonical input line.
pub const MAX_CANON: usize = 255;
/// Max bytes in terminal input.
pub const MAX_INPUT: usize = 255;
/// Max bytes in a file name.
pub const NAME_MAX: usize = 255;
/// Max supplemental group ids.
pub const NGROUPS_MAX: usize = 16;
/// Max open files per process.
pub const OPEN_MAX: usize = 64;
/// Max bytes in a pathname.
pub const PATH_MAX: usize = 1024;
/// Max bytes for atomic pipe writes.
pub const PIPE_BUF: usize = 512;
/// Max bytes in a symbolic link.
pub const SYMLINK_MAX: usize = PATH_MAX;
/// Max symlinks per path (for loops).
pub const SYMLOOP_MAX: usize = 32;

/// Max ibase/obase values in bc(1).
pub const BC_BASE_MAX: i32 = i32::MAX;
/// Max array elements in bc(1).
pub const BC_DIM_MAX: i32 = 65535;
/// Max scale value in bc(1).
pub const BC_SCALE_MAX: i32 = i32::MAX;
/// Max constant string length in bc(1).
pub const BC_STRING_MAX: i32 = i32::MAX;
/// Max weights for the order keyword.
pub const COLL_WEIGHTS_MAX: usize = 2;
/// Max expressions nested in expr(1).
pub const EXPR_NEST_MAX: usize = 32;
/// Max bytes in an input line.
pub const LINE_MAX: usize = 2048;
/// Max REs in interval notation.
pub const RE_DUP_MAX: usize = 255;
/// Max value of a `sem_*` semaphore.
pub const SEM_VALUE_MAX: u32 = u32::MAX;

/// Max number of iovs (readv, sendmsg, etc).
pub const IOV_MAX: usize = 1024;
/// Default "nice".
pub const NZERO: i32 = 20;

/// Max tty device name length with NUL.
pub const TTY_NAME_MAX: usize = 260;
/// Max login name length with NUL.
pub const LOGIN_NAME_MAX: usize = 32;

/// Max hostname length without NUL.
pub const HOST_NAME_MAX: usize = 255;

/// Max bytes from getentropy(2).
pub const GETENTROPY_MAX: usize = 256;

/// Max command name length, including the NUL.
pub const _MAXCOMLEN: usize = 24;

#[cfg(test)]
mod tests;
