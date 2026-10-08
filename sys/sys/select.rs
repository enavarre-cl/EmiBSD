/*	$OpenBSD: select.h,v 1.17 2016/09/12 19:41:20 guenther Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1992, 1993
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
 *	@(#)select.h	8.2 (Berkeley) 1/4/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/select.h>`: the `fd_set` bit masks of `select(2)`.
//!
//! Upstream: sys/sys/select.h @ 3ce1f3f79392
//!
//! `FD_SETSIZE`, `fd_mask`, `NFDBITS`, `NBBY`, `struct fd_set` and
//! `FD_SET`/`FD_CLR`/`FD_ISSET`/`FD_ZERO`/`FD_COPY`; the bit operations also work on a
//! slice of masks, the variable-size sets `dopselect` builds for `nd` descriptors. The
//! kernel's `struct selinfo` is `<sys/selinfo.h>`'s (`sys/selinfo.rs`); `struct timeval` and
//! `struct timespec` are `<sys/time.h>`'s; the `select`/`pselect` prototypes are user
//! space's.

/// `FD_SETSIZE`.
pub const FD_SETSIZE: usize = 1024;
/// `NBBY` (`__NBBY`): number of bits in a byte.
pub const NBBY: usize = 8;
/// `fd_mask` (`__fd_mask`).
pub type FdMask = u32;
/// `NFDBITS` (`__NFDBITS`): bits per mask.
pub const NFDBITS: usize = size_of::<FdMask>() * NBBY;

/// `howmany(x, y)` (`__howmany`).
pub const fn howmany(x: usize, y: usize) -> usize {
    x.div_ceil(y)
}

/// `struct fd_set`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdSet {
    /// `fds_bits`.
    pub fds_bits: [FdMask; howmany(FD_SETSIZE, NFDBITS)],
}

impl FdSet {
    /// `FD_ZERO`.
    pub const fn new() -> Self {
        Self {
            fds_bits: [0; howmany(FD_SETSIZE, NFDBITS)],
        }
    }
}

impl Default for FdSet {
    fn default() -> Self {
        Self::new()
    }
}

/// `FD_SET(fd, p)` (`__fd_set`) over a slice of masks.
pub fn fd_set(fd: usize, p: &mut [FdMask]) {
    p[fd / NFDBITS] |= 1 << (fd % NFDBITS);
}

/// `FD_CLR(fd, p)` (`__fd_clr`) over a slice of masks.
pub fn fd_clr(fd: usize, p: &mut [FdMask]) {
    p[fd / NFDBITS] &= !(1 << (fd % NFDBITS));
}

/// `FD_ISSET(fd, p)` (`__fd_isset`) over a slice of masks.
pub fn fd_isset(fd: usize, p: &[FdMask]) -> bool {
    p[fd / NFDBITS] & (1 << (fd % NFDBITS)) != 0
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits() {
        let mut s = FdSet::new();
        fd_set(33, &mut s.fds_bits);
        assert!(fd_isset(33, &s.fds_bits) && !fd_isset(32, &s.fds_bits));
        assert_eq!(s.fds_bits[1], 2);
        fd_clr(33, &mut s.fds_bits);
        assert_eq!(s, FdSet::default());
        assert_eq!(howmany(1, NFDBITS), 1);
        assert_eq!(howmany(33, NFDBITS), 2);
    }
}
/* </TESTS> */
