/*	$OpenBSD: uipc_domain.c,v 1.70 2025/06/12 20:37:58 deraadt Exp $	*/
/*	$NetBSD: uipc_domain.c,v 1.14 1996/02/09 19:00:44 christos Exp $	*/
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
 *	@(#)uipc_domain.c	8.2 (Berkeley) 10/18/93
 */
/* </LICENSES> */

//! The communication domains: `domains[]`, `domaininit`, the protocol lookups and the
//! `net.*` sysctl tree.
//!
//! Upstream: sys/kern/uipc_domain.c @ 3ce1f3f79392
//!
//! `domaininit` runs every domain's `dom_init` and every protocol's `pr_init`, sizes the link
//! and protocol headers the mbuf code reserves (`max_linkhdr`, `max_hdr`) and starts the
//! protocols' fast (200 ms) and slow (500 ms) timeouts.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - `domains[]` is a slice without the C's NULL terminator. It holds `inetdomain`
//!   (`netinet/in_proto.rs`) and `routedomain` (`net/rtsock.rs`). `unixdomain`
//!   (`kern/uipc_proto.c`, local sockets) is not ported: `domaininit` reports it with
//!   `unported!` where it would initialise it, and `net_sysctl`'s `PF_UNIX` branch
//!   (`uipc_sysctl`) is reported too. `MPLS`, `IPSEC`/`TCP_SIGNATURE` (`pfkeydomain`),
//!   `INET6` and `NAF_FRAME` are not configured; their entries are comments, and so are the
//!   `NBPFILTER`, `NPFLOW` and `PIPEX` branches of `net_sysctl`.
//! - The two timeouts are statics initialised in `domaininit` with their own address as the
//!   argument, as the C's function-local statics are.
//! - `pffinddomain`, `pffindtype` and `pffindproto` return `Option`s for the C's NULL;
//!   `pfctlinput` is an `unsafe fn` over a raw socket address, as `pr_ctlinput` is.
//! - `net_sysctl` follows `kern_sysctl.rs`'s calling convention (`docs/C_TO_RUST.md`);
//!   `sysctl_vslock` wraps the protocol handlers that are not `PR_MPSYSCTL`, as in C.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::kern::kern_sysctl::{sysctl_vslock, sysctl_vsunlock};
use crate::kern::kern_timeout::{timeout_add, timeout_add_msec, timeout_set_flags};
use crate::kern::uipc_mbuf::{MAX_HDR, MAX_LINKHDR, MAX_PROTOHDR};
use crate::net::ifq::net_ifiq_sysctl;
use crate::net::rtsock::ROUTEDOMAIN;
use crate::netinet::in_proto::INETDOMAIN;
use crate::sys::domain::Domain;
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::protosw::{PR_MPSYSCTL, Protosw};
use crate::sys::socket::{NET_LINK_IFRXQ, PF_LINK, PF_UNIX, PF_UNSPEC, SOCK_RAW, Sockaddr};
use crate::sys::systm::net_assert_locked;
use crate::sys::timeout::{KCLOCK_NONE, TIMEOUT_MPSAFE, TIMEOUT_PROC, Timeout};
use crate::unported;

/// `domains[]`: the configured communication domains.
pub static DOMAINS: [&Domain; 2] = [
    // MPLS: &mplsdomain, not configured.
    // IPSEC, TCP_SIGNATURE: &pfkeydomain, not configured.
    // INET6: &inet6domain, not configured.
    &INETDOMAIN,
    // &unixdomain: kern/uipc_proto.c is not ported (see the module's deviations).
    &ROUTEDOMAIN,
    // NAF_FRAME: &framedomain, not configured.
];

/// `pffast_timeout`: the 200 ms protocol timeout.
static PFFAST_TIMEOUT: Timeout = Timeout::new(pffasttimo, ptr::null_mut());
/// `pfslow_timeout`: the 500 ms protocol timeout.
static PFSLOW_TIMEOUT: Timeout = Timeout::new(pfslowtimo, ptr::null_mut());

/// `domaininit`: initialises the domains and their protocols.
pub fn domaininit() {
    // &unixdomain, between inetdomain and routedomain in the C's table, would run unp_init.
    let _ = unported!("unixdomain (kern/uipc_proto.c)");
    for dp in DOMAINS {
        if let Some(init) = dp.dom_init {
            init();
        }
        for pr in dp.dom_protosw {
            if let Some(init) = pr.pr_init {
                init();
            }
        }
    }

    // max_linkhdr of 64 was chosen to encompass tunnelling traffic in IP payloads, eg, by
    // etherip(4) or gif(4), without needing to prepend an mbuf to fit those headers.
    if MAX_LINKHDR.load(Ordering::Relaxed) < 64 {
        MAX_LINKHDR.store(64, Ordering::Relaxed);
    }

    MAX_HDR.store(
        MAX_LINKHDR.load(Ordering::Relaxed) + MAX_PROTOHDR.load(Ordering::Relaxed),
        Ordering::Relaxed,
    );
    timeout_set_flags(
        &PFFAST_TIMEOUT,
        pffasttimo,
        ptr::from_ref(&PFFAST_TIMEOUT).cast_mut().cast(),
        KCLOCK_NONE,
        TIMEOUT_PROC | TIMEOUT_MPSAFE,
    );
    timeout_set_flags(
        &PFSLOW_TIMEOUT,
        pfslowtimo,
        ptr::from_ref(&PFSLOW_TIMEOUT).cast_mut().cast(),
        KCLOCK_NONE,
        TIMEOUT_PROC | TIMEOUT_MPSAFE,
    );
    timeout_add(&PFFAST_TIMEOUT, 1);
    timeout_add(&PFSLOW_TIMEOUT, 1);
}

/// `pffinddomain`: the domain of address family `family`.
pub fn pffinddomain(family: i32) -> Option<&'static Domain> {
    DOMAINS.iter().copied().find(|dp| dp.dom_family == family)
}

/// `pffindtype`: the first protocol of `family` with socket type `type_`.
pub fn pffindtype(family: i32, type_: i32) -> Option<&'static Protosw> {
    let dp = pffinddomain(family)?;

    dp.dom_protosw
        .iter()
        .find(|pr| pr.pr_type != 0 && i32::from(pr.pr_type) == type_)
}

/// `pffindproto`: the protocol `protocol` of `family` with socket type `type_`; for raw
/// sockets, the domain's raw wildcard when no protocol matches.
pub fn pffindproto(family: i32, protocol: i32, type_: i32) -> Option<&'static Protosw> {
    let mut maybe = None;

    if family == i32::from(PF_UNSPEC) {
        return None;
    }

    let dp = pffinddomain(family)?;

    for pr in dp.dom_protosw {
        if i32::from(pr.pr_protocol) == protocol && i32::from(pr.pr_type) == type_ {
            return Some(pr);
        }

        if type_ == SOCK_RAW
            && i32::from(pr.pr_type) == SOCK_RAW
            && pr.pr_protocol == 0
            && maybe.is_none()
        {
            maybe = Some(pr);
        }
    }
    maybe
}

/// `net_link_sysctl`: `net.link`.
fn net_link_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    // All sysctl names at this level are nonterminal.
    if name.len() < 2 {
        return Err(Errno::EISDIR); // overloaded
    }
    let node = name[0];

    match node {
        NET_LINK_IFRXQ => net_ifiq_sysctl(&name[1..], oldp, Some(oldlenp), newp, newlen),
        _ => Err(Errno::ENOPROTOOPT),
    }
}

/// `net_sysctl`: the `net.*` tree, dispatched to the protocol family and the protocol.
pub fn net_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    // All sysctl names at this level are nonterminal. Usually: next two components are
    // protocol family and protocol number, then at least one addition component.
    if name.len() < 2 {
        return Err(Errno::EISDIR); // overloaded
    }
    let family = name[0];

    if family == i32::from(PF_UNSPEC) {
        return Ok(());
    }
    if family == i32::from(PF_LINK) {
        return net_link_sysctl(&name[1..], oldp, oldlenp, newp, newlen);
    }
    if family == i32::from(PF_UNIX) {
        // uipc_sysctl: kern/uipc_usrreq.c is not ported.
        return Err(unported!("uipc_sysctl (kern/uipc_usrreq.c)"));
    }
    // NBPFILTER (PF_BPF), NPFLOW (PF_PFLOW), PIPEX (PF_PIPEX), MPLS (PF_MPLS): not configured.
    let Some(dp) = pffinddomain(family) else {
        return Err(Errno::ENOPROTOOPT);
    };

    if name.len() < 3 {
        return Err(Errno::EISDIR); // overloaded
    }
    let protocol = name[1];
    for pr in dp.dom_protosw {
        if i32::from(pr.pr_protocol) == protocol
            && let Some(sysctl) = pr.pr_sysctl
        {
            let savelen = *oldlenp;
            if pr.pr_flags & PR_MPSYSCTL == 0 {
                sysctl_vslock(oldp, savelen)?;
            }
            let error = sysctl(&name[2..], oldp, oldlenp, newp, newlen);
            if pr.pr_flags & PR_MPSYSCTL == 0 {
                sysctl_vsunlock(oldp, savelen);
            }

            return error;
        }
    }
    Err(Errno::ENOPROTOOPT)
}

/// `pfctlinput`: hands control input `cmd` about `sa` to every protocol that takes it.
///
/// # Safety
///
/// `sa` points at a readable socket address of its `sa_len` bytes (`PrCtlinputFn`'s
/// contract).
pub unsafe fn pfctlinput(cmd: i32, sa: *const Sockaddr) {
    net_assert_locked("pfctlinput");

    for dp in DOMAINS {
        for pr in dp.dom_protosw {
            if let Some(ctlinput) = pr.pr_ctlinput {
                // SAFETY: the caller's contract; no argument, as the C passes NULL.
                unsafe { ctlinput(cmd, sa, 0, ptr::null_mut()) };
            }
        }
    }
}

/// `pfslowtimo`: the 500 ms protocol timeout; `arg` is its own timeout.
pub fn pfslowtimo(arg: *mut c_void) {
    // SAFETY: `domaininit` armed the timeout with its own address as the argument.
    let to = unsafe { &*arg.cast::<Timeout>() };

    for dp in DOMAINS {
        for pr in dp.dom_protosw {
            if let Some(slowtimo) = pr.pr_slowtimo {
                slowtimo();
            }
        }
    }
    timeout_add_msec(to, 500);
}

/// `pffasttimo`: the 200 ms protocol timeout; `arg` is its own timeout.
pub fn pffasttimo(arg: *mut c_void) {
    // SAFETY: as in `pfslowtimo`.
    let to = unsafe { &*arg.cast::<Timeout>() };

    for dp in DOMAINS {
        for pr in dp.dom_protosw {
            if let Some(fasttimo) = pr.pr_fasttimo {
                fasttimo();
            }
        }
    }
    timeout_add_msec(to, 200);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netinet::in_::{IPPROTO_ICMP, IPPROTO_RAW, IPPROTO_UDP};
    use crate::sys::socket::{AF_INET, AF_ROUTE, SOCK_DGRAM};

    #[test]
    fn the_inet_protocols_are_found_by_number_and_type() {
        assert!(pffinddomain(i32::from(AF_INET)).is_some());
        assert!(pffinddomain(i32::from(AF_ROUTE)).is_some());
        assert!(pffinddomain(i32::from(PF_UNSPEC)).is_none());

        let icmp = pffindproto(i32::from(AF_INET), IPPROTO_ICMP, SOCK_RAW).expect("icmp");
        assert_eq!(i32::from(icmp.pr_protocol), IPPROTO_ICMP);
        let udp = pffindproto(i32::from(AF_INET), IPPROTO_UDP, SOCK_DGRAM).expect("udp");
        assert_eq!(i32::from(udp.pr_protocol), IPPROTO_UDP);
        // An unknown raw protocol gets the raw wildcard; IPPROTO_RAW has its own entry.
        let wild = pffindproto(i32::from(AF_INET), 200, SOCK_RAW).expect("wildcard");
        assert_eq!(wild.pr_protocol, 0);
        let raw = pffindproto(i32::from(AF_INET), IPPROTO_RAW, SOCK_RAW).expect("raw");
        assert_eq!(i32::from(raw.pr_protocol), IPPROTO_RAW);
        assert!(pffindtype(i32::from(AF_INET), SOCK_DGRAM).is_some());
    }
}
