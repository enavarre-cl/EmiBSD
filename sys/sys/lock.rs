/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*
 * Copyright (c) 1995
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code contains ideas from software contributed to Berkeley by
 * Avadis Tevanian, Jr., Michael Wayne Young, and the Mach Operating
 * System project at Carnegie-Mellon University.
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
 *	@(#)lock.h	8.12 (Berkeley) 5/19/95
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/lock.h>`: the `LK_*` flags of `vn_lock(9)` and `VOP_LOCK(9)`, which are the
//! `rwlock(9)` operation flags plus `LK_DRAIN` and `LK_RETRY`.
//!
//! Upstream: sys/sys/lock.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The flags are `i32`, the type of the `int flags` argument they travel in.

use crate::sys::rwlock::{RW_NOSLEEP, RW_READ, RW_RECURSEFAIL, RW_WRITE, RW_WRITE_OTHER};

/// `LK_EXCLUSIVE`: exclusive lock.
pub const LK_EXCLUSIVE: i32 = RW_WRITE;
/// `LK_SHARED`: shared lock.
pub const LK_SHARED: i32 = RW_READ;
/// `LK_TYPE_MASK`: type of lock sought.
pub const LK_TYPE_MASK: i32 = RW_WRITE | RW_READ;
/// `LK_NOWAIT`: do not sleep to await lock.
pub const LK_NOWAIT: i32 = RW_NOSLEEP;
/// `LK_RECURSEFAIL`: fail if recursive exclusive lock.
pub const LK_RECURSEFAIL: i32 = RW_RECURSEFAIL;
/// `LK_EXCLOTHER`: exclusive lock held by some other thread.
pub const LK_EXCLOTHER: i32 = RW_WRITE_OTHER;
/// `LK_RWFLAGS`.
pub const LK_RWFLAGS: i32 = RW_WRITE | RW_READ | RW_NOSLEEP | RW_RECURSEFAIL | RW_WRITE_OTHER;

/// `LK_DRAIN`: wait for all lock activity to end.
pub const LK_DRAIN: i32 = 0x1000;
/// `LK_RETRY`: `vn_lock`: retry until locked.
pub const LK_RETRY: i32 = 0x2000;
/* </CODE> */
