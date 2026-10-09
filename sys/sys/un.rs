/* <LICENSES> */
/*
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
 *	@(#)un.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

/* <CODE> */
//! Definitions for UNIX IPC domain: `<sys/un.h>`.
//!
//! Upstream: sys/sys/un.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `SUN_LEN(su)` is userland only (`#ifndef _KERNEL`) and not ported.
//! - `sa_family_t` is `crate::sys::types::SaFamily`.

use crate::sys::types::SaFamily;

/// The size of `sun_path`.
pub const SUN_PATH_LEN: usize = 104;

/// `struct sockaddr_un`: definitions for UNIX IPC domain.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SockaddrUn {
    /// `sun_len`: sockaddr len excluding NUL.
    pub sun_len: u8,
    /// `sun_family`: `AF_UNIX`.
    pub sun_family: SaFamily,
    /// `sun_path`: path name (gag).
    pub sun_path: [u8; SUN_PATH_LEN],
}

impl SockaddrUn {
    /// `offsetof(struct sockaddr_un, sun_path)`.
    pub const PATH_OFFSET: usize = core::mem::offset_of!(SockaddrUn, sun_path);
}

const _: () = {
    assert!(size_of::<SockaddrUn>() == 106);
    assert!(SockaddrUn::PATH_OFFSET == 2);
};
/* </CODE> */
