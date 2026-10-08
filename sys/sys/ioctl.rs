/*	$OpenBSD: ioctl.h,v 1.17 2016/02/28 15:46:19 naddy Exp $	*/
/*	$NetBSD: ioctl.h,v 1.20 1996/01/30 18:21:47 thorpej Exp $	*/
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
 *	@(#)ioctl.h	8.6 (Berkeley) 3/28/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/ioctl.h>`: the `ioctl(2)` commands, which the header gathers from `<sys/ttycom.h>`,
//! `<sys/filio.h>` and `<sys/sockio.h>`.
//!
//! Upstream: sys/sys/ioctl.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The three `#include`s are `pub use`s, so `crate::sys::ioctl::TIOCGETA` names what
//!   `<sys/ioctl.h>` makes visible. The `ioctl()` prototype is userland.
//! - [`ioctl_arg`] and [`ioctl_ret`] are not in the C: they are the casts every handler does
//!   on its `caddr_t data` (`*(int *)data`, `*(struct termios *)data = ...`), over the byte
//!   slice `sys_ioctl` hands down (`docs/C_TO_RUST.md`).

pub use crate::sys::filio::*;
pub use crate::sys::sockio::*;
pub use crate::sys::ttycom::*;

use core::mem::{MaybeUninit, size_of};
use core::ptr;

use crate::machine::copy::AbiPod;

/// `*(T *)data`: the command's argument read out of the kernel copy `data`, all zeros if the
/// copy is shorter than a `T` (`sys_ioctl` sizes it from the command, so it never is).
pub fn ioctl_arg<T: AbiPod>(data: &[u8]) -> T {
    let mut obj = MaybeUninit::<T>::zeroed();
    if let Some(src) = data.get(..size_of::<T>()) {
        // SAFETY: `src` has `size_of::<T>()` bytes and `obj` is a distinct local of that size;
        // `T: AbiPod` makes every byte pattern a valid `T`.
        unsafe { ptr::copy_nonoverlapping(src.as_ptr(), obj.as_mut_ptr().cast::<u8>(), src.len()) };
    }
    // SAFETY: zeroed, then possibly overwritten with arbitrary bytes: valid for an `AbiPod`.
    unsafe { obj.assume_init() }
}

/// `*(T *)data = *v`: stores a result in the kernel copy `data` for `sys_ioctl` to copy out;
/// nothing if `data` is too short (see [`ioctl_arg`]).
pub fn ioctl_ret<T: AbiPod>(data: &mut [u8], v: &T) {
    if let Some(dst) = data.get_mut(..size_of::<T>()) {
        // SAFETY: `T: AbiPod` has no padding, so all `size_of::<T>()` bytes of `*v` are
        // initialised; `dst` has that many bytes and does not overlap `v`.
        unsafe {
            ptr::copy_nonoverlapping(ptr::from_ref(v).cast::<u8>(), dst.as_mut_ptr(), dst.len())
        };
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::termios::{ECHO, Termios};

    #[test]
    fn arguments_round_trip() {
        let mut data = [0u8; 8];
        ioctl_ret(&mut data, &-5i32);
        assert_eq!(ioctl_arg::<i32>(&data), -5);
        let mut t = Termios::zeroed();
        t.c_lflag = ECHO;
        t.c_cc[3] = 0x7f;
        let mut buf = [0u8; size_of::<Termios>()];
        ioctl_ret(&mut buf, &t);
        assert_eq!(ioctl_arg::<Termios>(&buf), t);
        // Too short: zeros out, nothing in.
        assert_eq!(ioctl_arg::<Termios>(&data), Termios::zeroed());
        let mut short = [1u8; 2];
        ioctl_ret(&mut short, &7i32);
        assert_eq!(short, [1, 1]);
    }
}
/* </TESTS> */
