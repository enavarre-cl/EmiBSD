/*	$OpenBSD: unistd.h,v 1.31 2015/07/20 00:56:10 guenther Exp $	*/
/*	$NetBSD: unistd.h,v 1.10 1994/06/29 06:46:06 cgd Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
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
 *	@(#)unistd.h	8.2 (Berkeley) 1/7/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/unistd.h>`: the POSIX constants the kernel shares with userland, and the parameter
//! blocks of `__tfork(2)` and `kbind(2)`.
//!
//! Upstream: sys/sys/unistd.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct __tfork` and `struct __kbind` are `#[repr(C)]` with their pointers as `usize`
//!   (user addresses the kernel never dereferences directly).

/// `_POSIX_VDISABLE`.
pub const _POSIX_VDISABLE: u8 = 0o377;
/// `_POSIX_ASYNC_IO`.
pub const _POSIX_ASYNC_IO: i32 = -1;
/// `_POSIX_PRIO_IO`.
pub const _POSIX_PRIO_IO: i32 = -1;
/// `_POSIX_SYNC_IO`.
pub const _POSIX_SYNC_IO: i32 = -1;

/// `_POSIX_VERSION`: the POSIX.1 version we target for compliance.
pub const _POSIX_VERSION: i64 = 200809;

// access function

/// Test for existence of file.
pub const F_OK: i32 = 0;
/// Test for execute or search permission.
pub const X_OK: i32 = 0x01;
/// Test for write permission.
pub const W_OK: i32 = 0x02;
/// Test for read permission.
pub const R_OK: i32 = 0x04;

// whence values for lseek(2)

/// Set file offset to offset.
pub const SEEK_SET: i32 = 0;
/// Set file offset to current plus offset.
pub const SEEK_CUR: i32 = 1;
/// Set file offset to EOF plus offset.
pub const SEEK_END: i32 = 2;

// old BSD whence values for lseek(2); renamed by POSIX 1003.1

/// `L_SET`.
pub const L_SET: i32 = SEEK_SET;
/// `L_INCR`.
pub const L_INCR: i32 = SEEK_CUR;
/// `L_XTND`.
pub const L_XTND: i32 = SEEK_END;

/// `struct __tfork`: the parameters argument passed to the `__tfork()` syscall.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Tfork {
    /// `tf_tcb`.
    pub tf_tcb: usize,
    /// `tf_tid`: a user `pid_t *`.
    pub tf_tid: usize,
    /// `tf_stack`.
    pub tf_stack: usize,
}

/// `struct __kbind`: the parameters argument for the `kbind()` syscall.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Kbind {
    /// `kb_addr`.
    pub kb_addr: usize,
    /// `kb_size`.
    pub kb_size: usize,
}

/// `KBIND_BLOCK_MAX`: powerpc, sparc, and sparc64 need 2 blocks.
pub const KBIND_BLOCK_MAX: usize = 2;
/// `KBIND_DATA_MAX`: sparc64 needs 6, four-byte words.
pub const KBIND_DATA_MAX: usize = 24;

// the pathconf(2) variable values are part of the ABI

// configurable pathname variables

/// `_PC_LINK_MAX`.
pub const _PC_LINK_MAX: i32 = 1;
/// `_PC_MAX_CANON`.
pub const _PC_MAX_CANON: i32 = 2;
/// `_PC_MAX_INPUT`.
pub const _PC_MAX_INPUT: i32 = 3;
/// `_PC_NAME_MAX`.
pub const _PC_NAME_MAX: i32 = 4;
/// `_PC_PATH_MAX`.
pub const _PC_PATH_MAX: i32 = 5;
/// `_PC_PIPE_BUF`.
pub const _PC_PIPE_BUF: i32 = 6;
/// `_PC_CHOWN_RESTRICTED`.
pub const _PC_CHOWN_RESTRICTED: i32 = 7;
/// `_PC_NO_TRUNC`.
pub const _PC_NO_TRUNC: i32 = 8;
/// `_PC_VDISABLE`.
pub const _PC_VDISABLE: i32 = 9;
/// `_PC_2_SYMLINKS`.
pub const _PC_2_SYMLINKS: i32 = 10;
/// `_PC_ALLOC_SIZE_MIN`.
pub const _PC_ALLOC_SIZE_MIN: i32 = 11;
/// `_PC_ASYNC_IO`.
pub const _PC_ASYNC_IO: i32 = 12;
/// `_PC_FILESIZEBITS`.
pub const _PC_FILESIZEBITS: i32 = 13;
/// `_PC_PRIO_IO`.
pub const _PC_PRIO_IO: i32 = 14;
/// `_PC_REC_INCR_XFER_SIZE`.
pub const _PC_REC_INCR_XFER_SIZE: i32 = 15;
/// `_PC_REC_MAX_XFER_SIZE`.
pub const _PC_REC_MAX_XFER_SIZE: i32 = 16;
/// `_PC_REC_MIN_XFER_SIZE`.
pub const _PC_REC_MIN_XFER_SIZE: i32 = 17;
/// `_PC_REC_XFER_ALIGN`.
pub const _PC_REC_XFER_ALIGN: i32 = 18;
/// `_PC_SYMLINK_MAX`.
pub const _PC_SYMLINK_MAX: i32 = 19;
/// `_PC_SYNC_IO`.
pub const _PC_SYNC_IO: i32 = 20;
/// `_PC_TIMESTAMP_RESOLUTION`.
pub const _PC_TIMESTAMP_RESOLUTION: i32 = 21;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kbind_layout() {
        assert_eq!(size_of::<Kbind>(), 16);
        assert_eq!(size_of::<Tfork>(), 24);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/unistd.h");
        let ours: &[(&str, i64)] = &[
            ("F_OK", F_OK as i64),
            ("X_OK", X_OK as i64),
            ("W_OK", W_OK as i64),
            ("R_OK", R_OK as i64),
            ("SEEK_SET", SEEK_SET as i64),
            ("SEEK_CUR", SEEK_CUR as i64),
            ("SEEK_END", SEEK_END as i64),
            ("KBIND_BLOCK_MAX", KBIND_BLOCK_MAX as i64),
            ("KBIND_DATA_MAX", KBIND_DATA_MAX as i64),
            ("_PC_LINK_MAX", _PC_LINK_MAX as i64),
            ("_PC_NAME_MAX", _PC_NAME_MAX as i64),
            ("_PC_PATH_MAX", _PC_PATH_MAX as i64),
            ("_PC_TIMESTAMP_RESOLUTION", _PC_TIMESTAMP_RESOLUTION as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
