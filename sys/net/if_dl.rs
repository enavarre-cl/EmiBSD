/*	$OpenBSD: if_dl.h,v 1.13 2023/11/12 17:51:40 bluhm Exp $	*/
/*	$NetBSD: if_dl.h,v 1.8 1995/03/26 20:30:13 jtc Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1990, 1993
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
 *	@(#)if_dl.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! The link-level socket address: `<net/if_dl.h>`.
//!
//! Upstream: sys/net/if_dl.h @ 3ce1f3f79392
//!
//! A link-level sockaddr may specify the interface in one of two ways: by a system-provided
//! index number (computed anew and possibly differently on every reboot), or by a
//! human-readable string such as "il0" (for managerial convenience). Census taking actions,
//! such as something akin to `SIOCGCONF`, would return both the index and the human name.
//! High volume transactions (such as giving a link-level "from" address in a recvfrom or
//! recvmsg call) may be likely only to provide the indexed form, which requires fewer copy
//! operations and less space. The form and interpretation of the link-level address is purely
//! a matter of convention between the device driver and its consumers; however, all drivers
//! for an interface of a given `if_type` are expected to agree.
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - `LLADDR(s)` is the `unsafe fn` [`lladdr`] over a raw pointer: a `sockaddr_dl` may be
//!   longer than the structure (`if_alloc_sadl` allocates room for the name and the address),
//!   so the result can point past the 24 bytes a reference would cover.
//! - `satosdl`, `satosdl_const` and `sdltosa` are pointer casts (`docs/C_TO_RUST.md`).
//! - `link_ntoa(3)` is userland.

use core::mem::size_of;
use core::ptr::addr_of_mut;

use crate::sys::socket::Sockaddr;

/// `struct sockaddr_dl`: structure of a link-level sockaddr.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SockaddrDl {
    /// Total length of sockaddr.
    pub sdl_len: u8,
    /// `AF_LINK`.
    pub sdl_family: u8,
    /// If != 0, system given index for interface.
    pub sdl_index: u16,
    /// Interface type.
    pub sdl_type: u8,
    /// Interface name length, no trailing 0 reqd.
    pub sdl_nlen: u8,
    /// Link level address length.
    pub sdl_alen: u8,
    /// Link layer selector length, mostly 0.
    pub sdl_slen: u8,
    /// Minimum work area, can be larger; contains both if name and ll address; big enough for
    /// `IFNAMSIZ` plus 8byte ll addr.
    pub sdl_data: [u8; 24],
}

/// `LLADDR(s)`: the link-level address, which follows the interface name in `sdl_data`.
///
/// # Safety
///
/// `s` must point to a readable `struct sockaddr_dl`. The result is only an address; reading
/// `sdl_alen` bytes from it needs the allocation to extend that far.
pub unsafe fn lladdr(s: *mut SockaddrDl) -> *mut u8 {
    // SAFETY: the caller guarantees `s` points to a readable sockaddr_dl.
    let nlen = unsafe { (*s).sdl_nlen } as usize;
    // SAFETY: `s` is valid, so the address of its field is computed in bounds.
    let data = unsafe { addr_of_mut!((*s).sdl_data) };
    data.cast::<u8>().wrapping_add(nlen)
}

/// `satosdl(sa)`: a generic `sockaddr` seen as a `sockaddr_dl`.
pub const fn satosdl(sa: *mut Sockaddr) -> *mut SockaddrDl {
    sa.cast()
}

/// `satosdl_const(sa)`: a generic `sockaddr` seen as a `sockaddr_dl`, read-only.
pub const fn satosdl_const(sa: *const Sockaddr) -> *const SockaddrDl {
    sa.cast()
}

/// `sdltosa(sdl)`: a `sockaddr_dl` seen as a generic `sockaddr`.
pub const fn sdltosa(sdl: *mut SockaddrDl) -> *mut Sockaddr {
    sdl.cast()
}

// LP64 size of the C structure.
const _: () = assert!(size_of::<SockaddrDl>() == 32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lladdr_follows_the_name() {
        let mut sdl = SockaddrDl {
            sdl_nlen: 4,
            sdl_alen: 6,
            ..SockaddrDl::default()
        };
        sdl.sdl_data[..10].copy_from_slice(b"vio0\x52\x54\x00\x12\x34\x56");
        let p = &mut sdl as *mut SockaddrDl;
        // SAFETY: `p` points to `sdl`, and the six address bytes are inside `sdl_data`.
        let addr = unsafe { core::slice::from_raw_parts(lladdr(p), 6) };
        assert_eq!(addr, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        assert_eq!(sdltosa(p).cast::<SockaddrDl>(), p);
    }
}
