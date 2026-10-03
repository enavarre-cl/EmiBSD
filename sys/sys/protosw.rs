/*	$OpenBSD: protosw.h,v 1.73 2025/10/24 15:09:56 bluhm Exp $	*/
/*	$NetBSD: protosw.h,v 1.10 1996/04/09 20:55:32 cgd Exp $	*/
/* <LICENSES> */
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
 *	@(#)protosw.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

//! Protocol switch table: `<sys/protosw.h>`.
//!
//! Upstream: sys/sys/protosw.h @ 3ce1f3f79392
//!
//! Each protocol has a handle initializing one of these structures, which is used for
//! protocol-protocol and system-protocol communication. A protocol is called through the
//! `pr_init` entry before any other. Thereafter it is called every 200ms through the
//! `pr_fasttimo` entry and every 500ms through the `pr_slowtimo` for timer based actions.
//! Protocols pass data between themselves as chains of mbufs using the `pr_input` hook, which
//! passes data up (towards UNIX); control information passes up on `pr_ctlinput`. The protocol
//! is responsible for the space occupied by any of the arguments to these entries and must
//! dispose of it.
//!
//! Status: `wip` (M7b): what the protocols below the socket layer use.
//!
//! ## Deviations
//! - `struct pr_usrreqs`, the `pr_usrreqs` and `pr_ctloutput` members and the `pru_*` inline
//!   wrappers need `struct socket` (`<sys/socketvar.h>`, not ported): they come with the socket
//!   layer. No protocol here can be reached from a socket yet.
//! - The hooks are `Option<fn>` with Rust signatures: `pr_input`'s `struct mbuf **` is
//!   `&mut Option<&'static Mbuf>` (the protocol may consume the packet and leave `None`), its
//!   netstack an `Option`; `pr_ctlinput` is an `unsafe fn` over a raw socket address (it may be
//!   any flavour of sockaddr) and an opaque argument; `pr_sysctl` is the sysctl calling
//!   convention of `kern_sysctl.rs` without the process.
//! - `PRC_IS_REDIRECT(cmd)` is a `const fn`; the `prurequests`/`prcrequests`/`prcorequests`
//!   name tables (behind `PRUREQUESTS`, ...) are not compiled, as in GENERIC.
//! - `pffindproto`, `pffindtype`, `pffinddomain` and `pfctlinput` are defined in
//!   `kern/uipc_domain.rs`; `ip_protox[]` and `inetsw[]` in `netinet/in_proto.rs`.

use core::ffi::c_void;

use crate::net::if_var::Netstack;
use crate::sys::domain::Domain;
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::socket::Sockaddr;

/// 2 slow timeouts per second.
pub const PR_SLOWHZ: i32 = 2;
/// 5 fast timeouts per second.
pub const PR_FASTHZ: i32 = 5;

// Values for pr_flags. PR_ADDR requires PR_ATOMIC; PR_ADDR and PR_CONNREQUIRED are mutually
// exclusive.

/// Exchange atomic messages only.
pub const PR_ATOMIC: i16 = 0x0001;
/// Addresses given with messages.
pub const PR_ADDR: i16 = 0x0002;
/// Connection required by protocol.
pub const PR_CONNREQUIRED: i16 = 0x0004;
/// Want `PRU_RCVD` calls.
pub const PR_WANTRCVD: i16 = 0x0008;
/// Passes capabilities.
pub const PR_RIGHTS: i16 = 0x0010;
/// Abort on accept(2) to disconnected socket.
pub const PR_ABRTACPTDIS: i16 = 0x0020;
/// Socket splicing is possible.
pub const PR_SPLICE: i16 = 0x0040;
/// Input runs with shared netlock.
pub const PR_MPINPUT: i16 = 0x0080;
/// mp-safe sysctl(2) handler.
pub const PR_MPSYSCTL: i16 = 0x0200;

// The user requests (pr_usrreq's req argument).

/// Attach protocol to up.
pub const PRU_ATTACH: i32 = 0;
/// Detach protocol from up.
pub const PRU_DETACH: i32 = 1;
/// Bind socket to address.
pub const PRU_BIND: i32 = 2;
/// Listen for connection.
pub const PRU_LISTEN: i32 = 3;
/// Establish connection to peer.
pub const PRU_CONNECT: i32 = 4;
/// Accept connection from peer.
pub const PRU_ACCEPT: i32 = 5;
/// Disconnect from peer.
pub const PRU_DISCONNECT: i32 = 6;
/// Won't send any more data.
pub const PRU_SHUTDOWN: i32 = 7;
/// Have taken data; more room now.
pub const PRU_RCVD: i32 = 8;
/// Send this data.
pub const PRU_SEND: i32 = 9;
/// Abort (fast DISCONNECT, DETACH).
pub const PRU_ABORT: i32 = 10;
/// Control operations on protocol.
pub const PRU_CONTROL: i32 = 11;
/// Return status into m.
pub const PRU_SENSE: i32 = 12;
/// Retrieve out of band data.
pub const PRU_RCVOOB: i32 = 13;
/// Send out of band data.
pub const PRU_SENDOOB: i32 = 14;
/// Fetch socket's address.
pub const PRU_SOCKADDR: i32 = 15;
/// Fetch peer's address.
pub const PRU_PEERADDR: i32 = 16;
/// Connect two sockets.
pub const PRU_CONNECT2: i32 = 17;
/// 200ms timeout (for protocols internal use).
pub const PRU_FASTTIMO: i32 = 18;
/// 500ms timeout.
pub const PRU_SLOWTIMO: i32 = 19;
/// Receive from below.
pub const PRU_PROTORCV: i32 = 20;
/// Send to below.
pub const PRU_PROTOSEND: i32 = 21;
/// The number of user requests.
pub const PRU_NREQ: i32 = 22;

// The commands of pr_ctlinput: (*protosw[].pr_ctlinput)(cmd, sa, arg), where sa is a pointer
// to a sockaddr and arg an optional argument used within a protocol family.

/// Interface transition.
pub const PRC_IFDOWN: i32 = 0;
/// Select new route if possible ???
pub const PRC_ROUTEDEAD: i32 = 1;
/// Increase in mtu to host.
pub const PRC_MTUINC: i32 = 2;
/// DEC congestion bit says slow down.
pub const PRC_QUENCH2: i32 = 3;
/// Some one said to slow down.
pub const PRC_QUENCH: i32 = 4;
/// Message size forced drop.
pub const PRC_MSGSIZE: i32 = 5;
/// Host appears to be down.
pub const PRC_HOSTDEAD: i32 = 6;
/// Deprecated (use `PRC_UNREACH_HOST`).
pub const PRC_HOSTUNREACH: i32 = 7;
/// No route to network.
pub const PRC_UNREACH_NET: i32 = 8;
/// No route to host.
pub const PRC_UNREACH_HOST: i32 = 9;
/// Dst says bad protocol.
pub const PRC_UNREACH_PROTOCOL: i32 = 10;
/// Bad port #.
pub const PRC_UNREACH_PORT: i32 = 11;
// was PRC_UNREACH_NEEDFRAG 12 (use PRC_MSGSIZE)
/// Source route failed.
pub const PRC_UNREACH_SRCFAIL: i32 = 13;
/// Net routing redirect.
pub const PRC_REDIRECT_NET: i32 = 14;
/// Host routing redirect.
pub const PRC_REDIRECT_HOST: i32 = 15;
/// Redirect for type of service & net.
pub const PRC_REDIRECT_TOSNET: i32 = 16;
/// Redirect for tos & host.
pub const PRC_REDIRECT_TOSHOST: i32 = 17;
/// Packet lifetime expired in transit.
pub const PRC_TIMXCEED_INTRANS: i32 = 18;
/// Lifetime expired on reass q.
pub const PRC_TIMXCEED_REASS: i32 = 19;
/// Header incorrect.
pub const PRC_PARAMPROB: i32 = 20;
/// The number of control commands.
pub const PRC_NCMDS: usize = 21;

// The requests of pr_ctloutput.

/// `PRCO_GETOPT`.
pub const PRCO_GETOPT: i32 = 0;
/// `PRCO_SETOPT`.
pub const PRCO_SETOPT: i32 = 1;
/// `PRCO_NCMDS`.
pub const PRCO_NCMDS: i32 = 2;

/// `int (*pr_input)(struct mbuf **, int *, int, int, struct netstack *)`: input to protocol
/// (from below). Takes the packet, the offset of the protocol's header, the protocol number
/// and the address family; returns the next protocol (`IPPROTO_DONE` when the packet is
/// consumed, and then `*mp` is `None`).
pub type PrInputFn = fn(&mut Option<&'static Mbuf>, &mut i32, i32, i32, Option<&Netstack>) -> i32;

/// `void (*pr_ctlinput)(int, struct sockaddr *, u_int, void *)`: control input (from below).
///
/// # Safety
///
/// `sa` points at a readable socket address of its `sa_len` bytes; `arg` is NULL or what the
/// family passes (`struct ip *` of the offending packet for `AF_INET`), valid for the call.
pub type PrCtlinputFn = unsafe fn(i32, *const Sockaddr, u32, *mut c_void);

/// `int (*pr_sysctl)(int *, u_int, void *, size_t *, void *, size_t)`: sysctl for protocol.
pub type PrSysctlFn = fn(&[i32], usize, &mut usize, usize, usize) -> Result<(), Errno>;

/// `struct protosw`.
pub struct Protosw {
    /// `pr_type`: socket type used for.
    pub pr_type: i16,
    /// `pr_domain`: domain protocol a member of.
    pub pr_domain: &'static Domain,
    /// `pr_protocol`: protocol number.
    pub pr_protocol: i16,
    /// `pr_flags`: see `PR_*`.
    pub pr_flags: i16,
    /// `pr_input`: input to protocol (from below).
    pub pr_input: Option<PrInputFn>,
    /// `pr_ctlinput`: control input (from below).
    pub pr_ctlinput: Option<PrCtlinputFn>,
    // pr_ctloutput, pr_usrreqs: need struct socket (see the module's deviations).
    /// `pr_init`: initialization hook.
    pub pr_init: Option<fn()>,
    /// `pr_fasttimo`: fast timeout (200ms).
    pub pr_fasttimo: Option<fn()>,
    /// `pr_slowtimo`: slow timeout (500ms).
    pub pr_slowtimo: Option<fn()>,
    /// `pr_sysctl`: sysctl for protocol.
    pub pr_sysctl: Option<PrSysctlFn>,
}

impl Protosw {
    /// A protocol of `domain` with every member zero or NULL, as the C's designated
    /// initialisers leave the members they do not name.
    pub const fn new(domain: &'static Domain) -> Self {
        Self {
            pr_type: 0,
            pr_domain: domain,
            pr_protocol: 0,
            pr_flags: 0,
            pr_input: None,
            pr_ctlinput: None,
            pr_init: None,
            pr_fasttimo: None,
            pr_slowtimo: None,
            pr_sysctl: None,
        }
    }
}

/// `PRC_IS_REDIRECT(cmd)`.
pub const fn prc_is_redirect(cmd: i32) -> bool {
    cmd >= PRC_REDIRECT_NET && cmd <= PRC_REDIRECT_TOSHOST
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest::{assert_complete, assert_defines};

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/protosw.h");
        let pr = assert_defines!(defs;
            PR_SLOWHZ, PR_FASTHZ, PR_ATOMIC, PR_ADDR, PR_CONNREQUIRED, PR_WANTRCVD, PR_RIGHTS,
            PR_ABRTACPTDIS, PR_SPLICE, PR_MPINPUT, PR_MPSYSCTL);
        assert_complete(&defs, "PR_", &pr);
        let pru = assert_defines!(defs;
            PRU_ATTACH, PRU_DETACH, PRU_BIND, PRU_LISTEN, PRU_CONNECT, PRU_ACCEPT,
            PRU_DISCONNECT, PRU_SHUTDOWN, PRU_RCVD, PRU_SEND, PRU_ABORT, PRU_CONTROL,
            PRU_SENSE, PRU_RCVOOB, PRU_SENDOOB, PRU_SOCKADDR, PRU_PEERADDR, PRU_CONNECT2,
            PRU_FASTTIMO, PRU_SLOWTIMO, PRU_PROTORCV, PRU_PROTOSEND, PRU_NREQ);
        assert_complete(&defs, "PRU_", &pru);
        let prc = assert_defines!(defs;
            PRC_IFDOWN, PRC_ROUTEDEAD, PRC_MTUINC, PRC_QUENCH2, PRC_QUENCH, PRC_MSGSIZE,
            PRC_HOSTDEAD, PRC_HOSTUNREACH, PRC_UNREACH_NET, PRC_UNREACH_HOST,
            PRC_UNREACH_PROTOCOL, PRC_UNREACH_PORT, PRC_UNREACH_SRCFAIL, PRC_REDIRECT_NET,
            PRC_REDIRECT_HOST, PRC_REDIRECT_TOSNET, PRC_REDIRECT_TOSHOST,
            PRC_TIMXCEED_INTRANS, PRC_TIMXCEED_REASS, PRC_PARAMPROB, PRC_NCMDS);
        assert_complete(&defs, "PRC_", &[&prc[..], &["PRC_IS_REDIRECT"]].concat());
        let prco = assert_defines!(defs; PRCO_GETOPT, PRCO_SETOPT, PRCO_NCMDS);
        assert_complete(&defs, "PRCO_", &prco);
    }
}
