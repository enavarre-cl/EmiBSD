/*	$OpenBSD: mman.h,v 1.36 2026/03/26 21:46:24 daniel Exp $	*/
/*	$NetBSD: mman.h,v 1.11 1995/03/26 20:24:23 jtc Exp $	*/
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
 *	@(#)mman.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

//! Memory mapping flags: `<sys/mman.h>`.
//!
//! Upstream: sys/sys/mman.h @ 3ce1f3f79392
//!
//! Status: `ported` for the kernel. Milestone M3 needs the protections and the inheritance and
//! advice codes that `uvm` encodes into its map flags; M7a adds the `MAP_*` flags,
//! `MAP_FLAGMASK`, `MS_*` and `MCL_*` for `uvm_mmap.c`. The userland prototypes and
//! `MAP_FAILED` are not the kernel's.

/// No permissions.
pub const PROT_NONE: i32 = 0x00;
/// Pages can be read.
pub const PROT_READ: i32 = 0x01;
/// Pages can be written.
pub const PROT_WRITE: i32 = 0x02;
/// Pages can be executed.
pub const PROT_EXEC: i32 = 0x04;

/// Share changes.
pub const MAP_SHARED: i32 = 0x0001;
/// Changes are private.
pub const MAP_PRIVATE: i32 = 0x0002;

// Other flags

/// Map addr must be exactly as requested.
pub const MAP_FIXED: i32 = 0x0010;
/// `__MAP_NOREPLACE`: fail if address not available.
pub const __MAP_NOREPLACE: i32 = 0x0800;
/// Allocated from memory, swap space.
pub const MAP_ANON: i32 = 0x1000;
/// Alternate POSIX spelling.
pub const MAP_ANONYMOUS: i32 = MAP_ANON;
/// `__MAP_NOFAULT`.
pub const __MAP_NOFAULT: i32 = 0x2000;
/// Mapping is used for a stack.
pub const MAP_STACK: i32 = 0x4000;
/// Omit from dumps.
pub const MAP_CONCEAL: i32 = 0x8000;

/// The flags `mmap(2)` accepts.
pub const MAP_FLAGMASK: i32 = 0xfff7;

// Deprecated flags: the C defines them as aliases or 0.

/// "copy" region at mmap time.
pub const MAP_COPY: i32 = MAP_PRIVATE;
/// Map from file (default).
pub const MAP_FILE: i32 = 0;
/// Region may contain semaphores.
pub const MAP_HASSEMAPHORE: i32 = 0;
/// Region is retained after exec.
pub const MAP_INHERIT: i32 = 0;
/// For `MAP_FILE`, don't change file size.
pub const MAP_NOEXTEND: i32 = 0;
/// Sun: don't reserve needed swap area.
pub const MAP_NORESERVE: i32 = 0;
/// Sun: rename private pages to file.
pub const MAP_RENAME: i32 = 0;
/// Attempt hint address, even within heap.
pub const MAP_TRYFIXED: i32 = 0;

// Flags to msync

/// Perform asynchronous writes.
pub const MS_ASYNC: i32 = 0x01;
/// Perform synchronous writes.
pub const MS_SYNC: i32 = 0x02;
/// Invalidate cached data.
pub const MS_INVALIDATE: i32 = 0x04;

// Advice to madvise

/// No further special treatment.
pub const MADV_NORMAL: i32 = 0;
/// Expect random page references.
pub const MADV_RANDOM: i32 = 1;
/// Expect sequential page references.
pub const MADV_SEQUENTIAL: i32 = 2;
/// Will need these pages.
pub const MADV_WILLNEED: i32 = 3;
/// Don't need these pages.
pub const MADV_DONTNEED: i32 = 4;
/// Insure that resources are reserved.
pub const MADV_SPACEAVAIL: i32 = 5;
/// Pages are empty, free them.
pub const MADV_FREE: i32 = 6;

// Flags to mlockall

/// `MCL_CURRENT`: lock all pages currently mapped.
pub const MCL_CURRENT: i32 = 0x01;
/// `MCL_FUTURE`: lock all pages mapped in the future.
pub const MCL_FUTURE: i32 = 0x02;

// Flags to minherit

/// Share with child.
pub const MAP_INHERIT_SHARE: i32 = 0;
/// Copy into child.
pub const MAP_INHERIT_COPY: i32 = 1;
/// Absent from child.
pub const MAP_INHERIT_NONE: i32 = 2;
/// Zero in child.
pub const MAP_INHERIT_ZERO: i32 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/mman.h");
        let ours: &[(&str, i64)] = &[
            ("PROT_NONE", PROT_NONE as i64),
            ("PROT_READ", PROT_READ as i64),
            ("PROT_WRITE", PROT_WRITE as i64),
            ("PROT_EXEC", PROT_EXEC as i64),
            ("MAP_SHARED", MAP_SHARED as i64),
            ("MAP_PRIVATE", MAP_PRIVATE as i64),
            ("MAP_FIXED", MAP_FIXED as i64),
            ("__MAP_NOREPLACE", __MAP_NOREPLACE as i64),
            ("MAP_ANON", MAP_ANON as i64),
            ("__MAP_NOFAULT", __MAP_NOFAULT as i64),
            ("MAP_STACK", MAP_STACK as i64),
            ("MAP_CONCEAL", MAP_CONCEAL as i64),
            ("MAP_FLAGMASK", MAP_FLAGMASK as i64),
            ("MS_ASYNC", MS_ASYNC as i64),
            ("MS_SYNC", MS_SYNC as i64),
            ("MS_INVALIDATE", MS_INVALIDATE as i64),
            ("MCL_CURRENT", MCL_CURRENT as i64),
            ("MCL_FUTURE", MCL_FUTURE as i64),
            ("MADV_SPACEAVAIL", MADV_SPACEAVAIL as i64),
            ("MADV_FREE", MADV_FREE as i64),
            ("MAP_INHERIT_SHARE", MAP_INHERIT_SHARE as i64),
            ("MAP_INHERIT_COPY", MAP_INHERIT_COPY as i64),
            ("MAP_INHERIT_NONE", MAP_INHERIT_NONE as i64),
            ("MAP_INHERIT_ZERO", MAP_INHERIT_ZERO as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
