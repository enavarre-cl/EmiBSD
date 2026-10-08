/*	$OpenBSD: poll.h,v 1.16 2024/08/04 22:28:08 guenther Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 1996 Theo de Raadt
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/poll.h>`: `struct pollfd` and the `POLL*` event bits of `poll(2)`.
//!
//! Upstream: sys/sys/poll.h @ 3ce1f3f79392
//!
//! Status: `ported`. The user-space prototypes and the `sigset_t`/`timespec` repeats are
//! libc's and `sys/signal.rs`/`sys/time.rs`'s.

/// `struct pollfd` (`pollfd_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pollfd {
    /// `fd`.
    pub fd: i32,
    /// `events`.
    pub events: i16,
    /// `revents`.
    pub revents: i16,
}

// SAFETY: `#[repr(C)]` of an `int` and two `short`s: 8 bytes, no padding, any bit pattern.
unsafe impl crate::machine::copy::AbiPod for Pollfd {}

/// `nfds_t`.
pub type Nfds = u32;

/// `POLLIN`.
pub const POLLIN: i16 = 0x0001;
/// `POLLPRI`.
pub const POLLPRI: i16 = 0x0002;
/// `POLLOUT`.
pub const POLLOUT: i16 = 0x0004;
/// `POLLERR`.
pub const POLLERR: i16 = 0x0008;
/// `POLLHUP`.
pub const POLLHUP: i16 = 0x0010;
/// `POLLNVAL`.
pub const POLLNVAL: i16 = 0x0020;
/// `POLLRDNORM`.
pub const POLLRDNORM: i16 = 0x0040;
/// `POLLNORM`.
pub const POLLNORM: i16 = POLLRDNORM;
/// `POLLWRNORM`.
pub const POLLWRNORM: i16 = POLLOUT;
/// `POLLRDBAND`.
pub const POLLRDBAND: i16 = 0x0080;
/// `POLLWRBAND`.
pub const POLLWRBAND: i16 = 0x0100;
/// `POLL_NOHUP`: internal use only.
pub const POLL_NOHUP: i16 = 0x1000;

/// `INFTIM`: an infinite `poll(2)` timeout.
pub const INFTIM: i32 = -1;

const _: () = {
    assert!(size_of::<Pollfd>() == 8);
};
/* </CODE> */
