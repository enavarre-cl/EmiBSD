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
 * Copyright (c) 1985, 1986, 1993
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
 *	@(#)in_var.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! Internet interface addresses and multicast records: `<netinet/in_var.h>`.
//!
//! Upstream: sys/netinet/in_var.h @ 3ce1f3f79392
//!
//! One `struct in_ifaddr` is allocated for each interface with an Internet address. The
//! `struct ifaddr` it embeds first holds the protocol-independent part; its `ifa_addr`,
//! `ifa_dstaddr` and `ifa_netmask` point at the `sockaddr_in`s the `in_ifaddr` reserves.
//! Locks: \[I\] immutable after creation, \[m\] the parent interface's `if_maddrlock`.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - `struct in_ifaddr` and `struct in_multi` are `#[repr(C)]` with the generic structure
//!   first and `Cell` members (the C changes them through shared pointers under the net
//!   lock); the all-zero value is valid, as `malloc(M_ZERO)` needs. `ifatoia` and
//!   `ifmatoinm` are the C's casts, checked: they take the address family as the type tag
//!   (only `netinet/in.c` makes `AF_INET` addresses and multicast records) and panic on
//!   another (`docs/C_TO_RUST.md`).
//! - The `ia_ifp`, `ia_flags`, `ia_broadaddr`, `inm_refcnt`, `inm_ifidx` and `inm_addr`
//!   shorthands are methods; `in_aliasreq`'s `ifra_ifrau` union is a `#[repr(C)]` union with
//!   the `ifra_addr` accessors, and `ifra_broadaddr` is the `ifra_dstaddr` member.
//! - The prototypes are `netinet/in_.rs`'s functions (`in.c` shares the module with `in.h`).

use core::cell::Cell;
use core::mem::size_of;

use crate::kern::subr_prf::panic;
use crate::net::if_::IFNAMSIZ;
use crate::net::if_var::{Ifaddr, Ifmaddr, Ifnet};
use crate::netinet::in_::{InAddr, SockaddrIn};
use crate::queue_adapter;
use crate::sys::queue::TailqEntry;
use crate::sys::refcnt::Refcnt;
use crate::sys::socket::AF_INET;

/// `struct in_ifaddr`: interface address, Internet version.
#[repr(C)]
pub struct InIfaddr {
    /// `ia_ifa`: protocol-independent info.
    pub ia_ifa: Ifaddr,
    /// `ia_net`: network number of interface.
    pub ia_net: Cell<u32>,
    /// `ia_netmask`: mask of net part.
    pub ia_netmask: Cell<u32>,
    /// `ia_list`: list of internet addresses.
    pub ia_list: TailqEntry<InIfaddr>,
    /// `ia_addr`: reserve space for interface name.
    pub ia_addr: Cell<SockaddrIn>,
    /// `ia_dstaddr`: reserve space for broadcast addr (`ia_broadaddr`).
    pub ia_dstaddr: Cell<SockaddrIn>,
    /// `ia_sockmask`: reserve space for general netmask.
    pub ia_sockmask: Cell<SockaddrIn>,
    /// `ia_allhosts`: multicast address record for the allhosts multicast group.
    pub ia_allhosts: Cell<Option<&'static InMulti>>,
}

// SAFETY: the members change under the net lock, as in C.
unsafe impl Sync for InIfaddr {}

impl InIfaddr {
    /// `ia_ifp` (`ia_ifa.ifa_ifp`).
    pub fn ia_ifp(&self) -> &Cell<Option<&'static Ifnet>> {
        &self.ia_ifa.ifa_ifp
    }

    /// `ia_flags` (`ia_ifa.ifa_flags`).
    pub fn ia_flags(&self) -> &Cell<u32> {
        &self.ia_ifa.ifa_flags
    }

    /// `ia_broadaddr` (`ia_dstaddr`).
    pub fn ia_broadaddr(&self) -> &Cell<SockaddrIn> {
        &self.ia_dstaddr
    }
}

queue_adapter!(
    /// `TAILQ_ENTRY(in_ifaddr) ia_list`.
    pub InIfaddrList: InIfaddr, ia_list => TailqEntry<InIfaddr>
);

/// `in_aliasreq`'s `ifra_ifrau` union.
#[repr(C)]
#[derive(Clone, Copy)]
pub union InAliasreqIfrau {
    /// `ifrau_addr`.
    pub ifrau_addr: SockaddrIn,
    /// `ifrau_align`.
    pub ifrau_align: i32,
}

/// `struct in_aliasreq`: the argument of `SIOCAIFADDR` and `SIOCDIFADDR`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InAliasreq {
    /// `ifra_name`: if name, e.g. "en0".
    pub ifra_name: [u8; IFNAMSIZ],
    /// `ifra_ifrau`.
    pub ifra_ifrau: InAliasreqIfrau,
    /// `ifra_dstaddr` (`ifra_broadaddr`).
    pub ifra_dstaddr: SockaddrIn,
    /// `ifra_mask`.
    pub ifra_mask: SockaddrIn,
}

impl InAliasreq {
    /// An all-zero request.
    pub const fn zeroed() -> Self {
        Self {
            ifra_name: [0; IFNAMSIZ],
            ifra_ifrau: InAliasreqIfrau { ifrau_align: 0 },
            ifra_dstaddr: SockaddrIn {
                sin_len: 0,
                sin_family: 0,
                sin_port: 0,
                sin_addr: InAddr { s_addr: 0 },
                sin_zero: [0; 8],
            },
            ifra_mask: SockaddrIn {
                sin_len: 0,
                sin_family: 0,
                sin_port: 0,
                sin_addr: InAddr { s_addr: 0 },
                sin_zero: [0; 8],
            },
        }
    }

    /// `ifra_addr` (`ifra_ifrau.ifrau_addr`).
    pub fn ifra_addr(&self) -> &SockaddrIn {
        // SAFETY: both members are plain data, valid for any bit pattern; the union is the
        // size of the address.
        unsafe { &self.ifra_ifrau.ifrau_addr }
    }

    /// `ifra_addr`, writable.
    pub fn ifra_addr_mut(&mut self) -> &mut SockaddrIn {
        // SAFETY: as above.
        unsafe { &mut self.ifra_ifrau.ifrau_addr }
    }

    /// `ifra_broadaddr` (`ifra_dstaddr`).
    pub fn ifra_broadaddr(&self) -> &SockaddrIn {
        &self.ifra_dstaddr
    }
}

/// `struct in_multi`: Internet multicast address structure. There is one of these for each
/// IP multicast group to which this host belongs on a given network interface.
#[repr(C)]
pub struct InMulti {
    /// `inm_ifma`: protocol-independent info.
    pub inm_ifma: Ifmaddr,
    /// \[I\] `inm_sin`: IPv4 multicast address.
    pub inm_sin: Cell<SockaddrIn>,
    /// \[m\] `inm_state`: state of membership.
    pub inm_state: Cell<u32>,
    /// \[m\] `inm_timer`: IGMP membership report.
    pub inm_timer: Cell<u32>,
}

// SAFETY: the members change under the interface's `if_maddrlock`, as in C.
unsafe impl Sync for InMulti {}

impl InMulti {
    /// `inm_refcnt` (`inm_ifma.ifma_refcnt`).
    pub fn inm_refcnt(&self) -> &Refcnt {
        &self.inm_ifma.ifma_refcnt
    }

    /// `inm_ifidx` (`inm_ifma.ifma_ifidx`).
    pub fn inm_ifidx(&self) -> &Cell<u32> {
        &self.inm_ifma.ifma_ifidx
    }

    /// `inm_addr` (`inm_sin.sin_addr`).
    pub fn inm_addr(&self) -> InAddr {
        self.inm_sin.get().sin_addr
    }
}

/// `ifatoia(ifa)`: the Internet address an `AF_INET` interface address is the first member
/// of.
pub fn ifatoia(ifa: &Ifaddr) -> &InIfaddr {
    let addr = ifa.ifa_addr.get();
    // SAFETY: an interface address's `ifa_addr` is a readable socket address (`ifa_add`'s
    // contract).
    if addr.is_null() || unsafe { (*addr).sa_family } != AF_INET {
        panic(format_args!("ifatoia: not an inet address"));
    }
    // SAFETY: every `AF_INET` interface address is the `ia_ifa` member, at offset 0 of the
    // `#[repr(C)]` `InIfaddr` that `in_ioctl_*` allocated.
    unsafe { &*core::ptr::from_ref(ifa).cast::<InIfaddr>() }
}

/// `ifmatoinm(ifma)`: the Internet multicast record an `AF_INET` multicast address is the
/// first member of.
pub fn ifmatoinm(ifma: &Ifmaddr) -> &InMulti {
    let addr = ifma.ifma_addr.get();
    // SAFETY: a multicast record's address is readable (its protocol set it).
    if addr.is_null() || unsafe { (*addr).sa_family } != AF_INET {
        panic(format_args!("ifmatoinm: not an inet record"));
    }
    // SAFETY: every `AF_INET` multicast record is the `inm_ifma` member, at offset 0 of the
    // `#[repr(C)]` `InMulti` that `in_addmulti` allocated.
    unsafe { &*core::ptr::from_ref(ifma).cast::<InMulti>() }
}

// LP64 size of the user-visible structure.
const _: () = assert!(size_of::<InAliasreq>() == 64);
/* </CODE> */
