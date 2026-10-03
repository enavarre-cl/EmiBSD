/*	$OpenBSD: uio.h,v 1.20 2024/10/26 05:39:03 jsg Exp $	*/
/*	$NetBSD: uio.h,v 1.12 1996/02/09 18:25:45 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1993, 1994
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
 *	@(#)uio.h	8.5 (Berkeley) 2/22/94
 */
/* </LICENSES> */

//! `<sys/uio.h>`: scatter/gather I/O descriptions: `struct iovec`, the `uio_rw` and
//! `uio_seg` enums and the kernel's `struct uio`.
//!
//! Upstream: sys/sys/uio.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `uio_iov`/`uio_iovcnt` are one mutable slice of iovecs: advancing to the next iovec
//!   (`uio->uio_iov++; uio->uio_iovcnt--`) shortens the slice, and `uio_iovcnt()` is its
//!   length.
//! - `uio_procp` is `Option<&Proc>` (the C's NULL is `None`).
//! - [`Iovec::from_bytes`] is the `copyin` of one user `struct iovec`; the structure is two
//!   words without padding.
//! - The prototypes of `ureadc`, `iovec_copyin`, `iovec_free`, `dofilereadv` and
//!   `dofilewritev` are their functions in `kern/kern_subr.rs` and `kern/sys_generic.rs`;
//!   the userland ones (`readv`, `preadv`, ...) are not kernel material.

use core::ffi::c_void;
use core::ptr;

use crate::sys::proc::Proc;
use crate::sys::types::Off;

/// `UIO_SMALLIOV`: 8 on stack, else malloc.
pub const UIO_SMALLIOV: usize = 8;

/// `UIO_MAXIOV`: deprecated, use `IOV_MAX` instead.
pub const UIO_MAXIOV: usize = 1024;

/// `struct iovec`: one segment of a scatter/gather list.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Iovec {
    /// `iov_base`: base address (a user address under `UIO_USERSPACE`).
    pub iov_base: *mut c_void,
    /// `iov_len`: length.
    pub iov_len: usize,
}

impl Iovec {
    /// The structure's size in user space.
    pub const SIZE: usize = size_of::<Iovec>();

    /// An empty iovec.
    pub const fn new() -> Self {
        Self {
            iov_base: ptr::null_mut(),
            iov_len: 0,
        }
    }

    /// The iovec read from its user-space bytes (`copyin`).
    pub fn from_bytes(b: &[u8; Self::SIZE]) -> Self {
        let mut base = [0u8; size_of::<usize>()];
        let mut len = [0u8; size_of::<usize>()];
        base.copy_from_slice(&b[..size_of::<usize>()]);
        len.copy_from_slice(&b[size_of::<usize>()..]);
        Self {
            iov_base: ptr::without_provenance_mut(usize::from_ne_bytes(base)),
            iov_len: usize::from_ne_bytes(len),
        }
    }
}

impl Default for Iovec {
    fn default() -> Self {
        Self::new()
    }
}

/// `enum uio_rw`: the direction of a transfer.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types, clippy::upper_case_acronyms)] // OpenBSD names, verbatim
pub enum UioRw {
    /// `UIO_READ`: from the kernel buffer to the iovecs.
    UIO_READ,
    /// `UIO_WRITE`: from the iovecs to the kernel buffer.
    UIO_WRITE,
}

/// `enum uio_seg`: segment flag values.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types, clippy::upper_case_acronyms)] // OpenBSD names, verbatim
pub enum UioSeg {
    /// `UIO_USERSPACE`: from user data space.
    UIO_USERSPACE,
    /// `UIO_SYSSPACE`: from system space.
    UIO_SYSSPACE,
}

/// `struct uio`: a scatter/gather transfer in progress.
///
/// A `UIO_SYSSPACE` uio's iovecs are kernel buffers: whoever builds one vouches for them, as
/// in C (`uiomove` copies with `kcopy`, which catches a fault but not a wrong address).
pub struct Uio<'a> {
    /// `uio_iov`/`uio_iovcnt`: the iovecs not yet consumed.
    pub uio_iov: &'a mut [Iovec],
    /// `uio_offset`: offset into file this uio corresponds to.
    pub uio_offset: Off,
    /// `uio_resid`: residual i/o count.
    pub uio_resid: usize,
    /// `uio_segflg`: see [`UioSeg`].
    pub uio_segflg: UioSeg,
    /// `uio_rw`: see [`UioRw`].
    pub uio_rw: UioRw,
    /// `uio_procp`: associated thread or NULL.
    pub uio_procp: Option<&'a Proc>,
}

impl Uio<'_> {
    /// `uio_iovcnt`: number of iovecs left in the array.
    pub fn uio_iovcnt(&self) -> usize {
        self.uio_iov.len()
    }
}

const _: () = assert!(size_of::<Iovec>() == 2 * size_of::<usize>());
