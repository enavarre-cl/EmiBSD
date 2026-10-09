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

/*-
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
 *	@(#)uipc_proto.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! Definitions of protocols supported in the UNIX domain: `kern/uipc_proto.c`.
//!
//! Upstream: sys/kern/uipc_proto.c @ 3ce1f3f79392
//!
//! `unixsw[]` holds the three local socket types (stream, sequenced packet and datagram),
//! all of them passing rights (`PR_RIGHTS`); `unixdomain` is `AF_UNIX`, initialised by
//! `unp_init` and externalizing and disposing of rights with `unp_externalize` and
//! `unp_dispose` (`uipc_usrreq.rs`).
//!
//! ## Deviations
//! - `dom_protoswNPROTOSW` is the end of the `dom_protosw` slice (`sys/domain.rs`).

use crate::kern::uipc_usrreq::{
    UIPC_DGRAM_USRREQS, UIPC_USRREQS, unp_dispose, unp_externalize, unp_init,
};
use crate::sys::domain::Domain;
use crate::sys::protosw::{PR_ADDR, PR_ATOMIC, PR_CONNREQUIRED, PR_RIGHTS, PR_WANTRCVD, Protosw};
use crate::sys::socket::{AF_UNIX, PF_UNIX, SOCK_DGRAM, SOCK_SEQPACKET, SOCK_STREAM};

/// `unixsw[]`: the protocols of the UNIX domain.
pub static UNIXSW: [Protosw; 3] = [
    Protosw {
        pr_type: SOCK_STREAM as i16,
        pr_protocol: PF_UNIX as i16,
        pr_flags: PR_CONNREQUIRED | PR_WANTRCVD | PR_RIGHTS,
        pr_usrreqs: Some(&UIPC_USRREQS),
        ..Protosw::new(&UNIXDOMAIN)
    },
    Protosw {
        pr_type: SOCK_SEQPACKET as i16,
        pr_protocol: PF_UNIX as i16,
        pr_flags: PR_ATOMIC | PR_CONNREQUIRED | PR_WANTRCVD | PR_RIGHTS,
        pr_usrreqs: Some(&UIPC_USRREQS),
        ..Protosw::new(&UNIXDOMAIN)
    },
    Protosw {
        pr_type: SOCK_DGRAM as i16,
        pr_protocol: PF_UNIX as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_RIGHTS,
        pr_usrreqs: Some(&UIPC_DGRAM_USRREQS),
        ..Protosw::new(&UNIXDOMAIN)
    },
];

/// `unixdomain`.
pub static UNIXDOMAIN: Domain = Domain {
    dom_family: AF_UNIX as i32,
    dom_name: b"unix",
    dom_init: Some(unp_init),
    dom_externalize: Some(unp_externalize),
    dom_dispose: Some(unp_dispose),
    dom_protosw: &UNIXSW,
    dom_sasize: 0,
    dom_rtoffset: 0,
    dom_maxplen: 0,
};
/* </CODE> */
