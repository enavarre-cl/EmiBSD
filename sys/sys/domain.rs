/*	$OpenBSD: domain.h,v 1.25 2024/10/26 05:39:03 jsg Exp $	*/
/*	$NetBSD: domain.h,v 1.10 1996/02/09 18:25:07 christos Exp $	*/
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
 *	@(#)domain.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

//! Structure per communications domain: `<sys/domain.h>`.
//!
//! Upstream: sys/sys/domain.h @ 3ce1f3f79392
//!
//! A domain is an address family (`AF_INET`, `AF_ROUTE`, ...) with its table of protocols and
//! the routing table's view of its addresses (`dom_rtoffset`, `dom_maxplen`).
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - `dom_protosw` and `dom_protoswNPROTOSW` (the table and one past its end) are one slice.
//! - `dom_name` is a byte string without the NUL.
//! - `domaininit` and `domains[]` are `kern/uipc_domain.rs`'s, which defines them; the domains
//!   themselves (`inetdomain`, `routedomain`, ...) are their protocol files' statics, as the
//!   C's `extern` declarations name them. `socklen_t` is `crate::sys::types::Socklen`.
//! - `dom_dispose` takes `Option<&'static Mbuf>`: `sorele` and `sorflush` hand it the
//!   receive buffer's chain, NULL when the buffer is empty.

use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::protosw::Protosw;
use crate::sys::types::Socklen;

/// `int (*dom_externalize)(struct mbuf *, socklen_t, int)`: externalize access rights.
pub type DomExternalizeFn = fn(&'static Mbuf, Socklen, i32) -> Result<(), Errno>;

/// `void (*dom_dispose)(struct mbuf *)`: dispose of internalized rights; the chain may be
/// empty (a socket buffer without records).
pub type DomDisposeFn = fn(Option<&'static Mbuf>);

/// `struct domain`.
pub struct Domain {
    /// `dom_family`: `AF_xxx`.
    pub dom_family: i32,
    /// `dom_name`.
    pub dom_name: &'static [u8],
    /// `dom_init`: initialize domain data structures.
    pub dom_init: Option<fn()>,
    /// `dom_externalize`: externalize access rights.
    pub dom_externalize: Option<DomExternalizeFn>,
    /// `dom_dispose`: dispose of internalized rights.
    pub dom_dispose: Option<DomDisposeFn>,
    /// `dom_protosw` .. `dom_protoswNPROTOSW`: the protocols.
    pub dom_protosw: &'static [Protosw],
    /// `dom_sasize`: size of sockaddr structure.
    pub dom_sasize: u32,
    /// `dom_rtoffset`: offset of the key, in bytes (0: no routing table).
    pub dom_rtoffset: u32,
    /// `dom_maxplen`: maximum prefix length, in bits.
    pub dom_maxplen: u32,
}
