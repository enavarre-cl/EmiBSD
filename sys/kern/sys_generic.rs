/*	$OpenBSD: sys_generic.c,v 1.161 2026/03/09 02:44:04 deraadt Exp $	*/
/*	$NetBSD: sys_generic.c,v 1.24 1996/03/29 00:25:32 cgd Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 Theo de Raadt
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)sys_generic.c	8.5 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! `sys_generic.c`: the generic file system calls: `read`, `write`, `ioctl`, `select`,
//! `poll`.
//!
//! Upstream: sys/kern/sys_generic.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M6 (part b) ports `sys_write` and the shape of `dofilewritev`
//! so the first user process can print. `sys_read`/`sys_readv`/`sys_pread`/`sys_preadv`,
//! `dofilereadv`, `sys_writev`/`sys_pwrite`/`sys_pwritev`, `sys_ioctl`, `select`, `poll`
//! and `kqueue`'s pollers come with the file descriptor table (M6-c) and the filesystems.
//!
//! ## Deviations
//! - Without `struct file` and `fd_getfile`, `dofilewritev` knows two descriptors: 1 and 2
//!   are the console (`cnputc`, in `PAGE_SIZE` chunks through `copyin`); any other
//!   descriptor is `EBADF`, and the gap is reported once.

use crate::dev::cons::cnputc;
use crate::machine::copy::copyin;
use crate::sys::errno::Errno;
use crate::sys::limits::SSIZE_MAX;
use crate::sys::proc::Proc;
use crate::sys::syscallargs::SysWriteArgs;
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::types::Register;
use crate::unported;

/// How much `dofilewritev` copies in at a time.
const WRITE_CHUNK: usize = 256;

/// Write system call.
pub fn sys_write(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysWriteArgs = sysargs(v);

    let iov_base = uap.buf.get() as usize;
    let iov_len = uap.nbyte.get();
    if iov_len > SSIZE_MAX as usize {
        return Err(Errno::EINVAL);
    }

    dofilewritev(p, uap.fd.get(), iov_base, iov_len, 0, retval)
}

/// `dofilewritev`: writes the user buffer to descriptor `fd` (see the module's deviations:
/// the console for 1 and 2).
pub fn dofilewritev(
    _p: &Proc,
    fd: i32,
    base: usize,
    len: usize,
    _flags: i32,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    if fd != 1 && fd != 2 {
        // fd_getfile(p->p_fd, fd), FILE_IS_USABLE, fo_write: the file table (M6-c).
        let _ = unported!("dofilewritev: fd_getfile (kern_descrip.c, M6-c)");
        return Err(Errno::EBADF);
    }

    let mut chunk = [0u8; WRITE_CHUNK];
    let mut done = 0;
    while done < len {
        let n = (len - done).min(WRITE_CHUNK);
        copyin(base + done, &mut chunk[..n])?;
        for &c in &chunk[..n] {
            cnputc(i32::from(c));
        }
        done += n;
    }

    retval[0] = len as Register;
    Ok(())
}
