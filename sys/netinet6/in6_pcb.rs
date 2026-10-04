/*	$OpenBSD: in6_pcb.c,v 1.152 2025/09/16 09:19:16 florian Exp $	*/
/* <LICENSES> */
/*
 * Copyright (C) 1995, 1996, 1997, and 1998 WIDE Project.
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
 * 3. Neither the name of the project nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE PROJECT AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE PROJECT OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */

/*
 *	@(#)COPYRIGHT	1.1 (NRL) 17 January 1995
 *
 * NRL grants permission for redistribution and use in source and binary
 * forms, with or without modification, of the software and documentation
 * created at NRL provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgements:
 *	This product includes software developed by the University of
 *	California, Berkeley and its contributors.
 *	This product includes software developed at the Information
 *	Technology Division, US Naval Research Laboratory.
 * 4. Neither the name of the NRL nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THE SOFTWARE PROVIDED BY NRL IS PROVIDED BY NRL AND CONTRIBUTORS ``AS
 * IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
 * PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL NRL OR
 * CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 * EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
 * PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
 * LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
 * NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 * The views and conclusions contained in the software and documentation
 * are those of the authors and should not be interpreted as representing
 * official policies, either expressed or implied, of the US Naval
 * Research Laboratory (NRL).
 */

/*
 * Copyright (c) 1982, 1986, 1990, 1993, 1995
 *	Regents of the University of California.  All rights reserved.
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
 */
/* </LICENSES> */

//! IPv6 protocol control blocks: binding, connecting, lookups and notifications of
//! `INP_IPV6` pcbs: `netinet6/in6_pcb.c` (prototypes in `<netinet/in_pcb.h>`).
//!
//! Upstream: sys/netinet6/in6_pcb.c @ 3ce1f3f79392
//!
//! The licence block carries the NRL notice with its advertising clause, accepted as
//! BSD-4 (`.claude/rules/scope-and-stubs.md`).
//!
//! Status: skeleton from the INET6 foundation step: the globals are defined, every
//! function has its final signature and a placeholder body that reports itself through
//! `unported!` until the file is ported.
//!
//! ## Deviations
//! - None yet: the file is a skeleton (see `Status`).

use crate::net::route::Rtentry;
use crate::netinet::in_pcb::{InpNotifyFn, Inpcb, Inpcbtable};
use crate::netinet6::in6::{IN6ADDR_ANY_INIT, In6Addr, SockaddrIn6};
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::proc::Proc;
use crate::sys::socketvar::Socket;
use core::ffi::c_void;

/// `zeroin6_addr`: the unspecified address.
pub static ZEROIN6_ADDR: In6Addr = IN6ADDR_ANY_INIT;

/// `in6_pcbhash`: the hash of an IPv6 address/port quadruple in `table`.
pub fn in6_pcbhash(
    table: &Inpcbtable,
    rdomain: u32,
    faddr: &In6Addr,
    fport: u16,
    laddr: &In6Addr,
    lport: u16,
) -> u64 {
    let _ = (table, rdomain, faddr, fport, laddr, lport);
    let _ = crate::unported!("in6_pcbhash: placeholder");
    0
}

/// `in6_pcbaddrisavail_lock`: whether `sin6` may be bound by `inp` (`lock`:
/// `IN_PCBLOCK_HOLD` or `IN_PCBLOCK_GRAB`); clears the port and scope of the copy it checks.
pub fn in6_pcbaddrisavail_lock(
    inp: &Inpcb,
    sin6: &mut SockaddrIn6,
    wild: i32,
    p: &Proc,
    lock: i32,
) -> Result<(), Errno> {
    let _ = (inp, sin6, wild, p, lock);
    Err(crate::unported!("in6_pcbaddrisavail_lock: placeholder"))
}

/// `in6_pcbaddrisavail`: `in6_pcbaddrisavail_lock` taking the table mutex.
pub fn in6_pcbaddrisavail(
    inp: &Inpcb,
    sin6: &mut SockaddrIn6,
    wild: i32,
    p: &Proc,
) -> Result<(), Errno> {
    let _ = (inp, sin6, wild, p);
    Err(crate::unported!("in6_pcbaddrisavail: placeholder"))
}

/// `in6_pcbconnect`: connects `inp` to the `sockaddr_in6` in `nam`, choosing the
/// local address and port if unset.
pub fn in6_pcbconnect(inp: &'static Inpcb, nam: &Mbuf) -> Result<(), Errno> {
    let _ = (inp, nam);
    Err(crate::unported!("in6_pcbconnect: placeholder"))
}

/// `in6_setsockaddr`: writes the local address of `inp` into `nam`.
pub fn in6_setsockaddr(inp: &Inpcb, nam: &Mbuf) {
    let _ = (inp, nam);
    let _ = crate::unported!("in6_setsockaddr: placeholder");
}

/// `in6_setpeeraddr`: writes the foreign address of `inp` into `nam`.
pub fn in6_setpeeraddr(inp: &Inpcb, nam: &Mbuf) {
    let _ = (inp, nam);
    let _ = crate::unported!("in6_setpeeraddr: placeholder");
}

/// `in6_sockaddr`: the `pru_sockaddr` of the IPv6 protocols.
pub fn in6_sockaddr(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    let _ = (so, nam);
    Err(crate::unported!("in6_sockaddr: placeholder"))
}

/// `in6_peeraddr`: the `pru_peeraddr` of the IPv6 protocols.
pub fn in6_peeraddr(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    let _ = (so, nam);
    Err(crate::unported!("in6_peeraddr: placeholder"))
}

/// `in6_pcbnotify`: passes control command `cmd` to every pcb of `table` connected to
/// `dst` (and the source `src`, `None` for a notification of local fragmentation),
/// calling `notify` with the errno of `inet6ctlerrmap`; `cmdarg` is the command's
/// argument (the `ip6ctlparam`'s `ip6c_cmdarg`).
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn in6_pcbnotify(
    table: &Inpcbtable,
    dst: &SockaddrIn6,
    fport_arg: u32,
    src: Option<&SockaddrIn6>,
    lport_arg: u32,
    rtable: u32,
    cmd: i32,
    cmdarg: *mut c_void,
    notify: Option<InpNotifyFn>,
) {
    let _ = (
        table, dst, fport_arg, src, lport_arg, rtable, cmd, cmdarg, notify,
    );
    let _ = crate::unported!("in6_pcbnotify: placeholder");
}

/// `in6_pcbrtentry`: the cached route of `inp` to its foreign address, refreshed.
pub fn in6_pcbrtentry(inp: &Inpcb) -> Option<&'static Rtentry> {
    let _ = inp;
    let _ = crate::unported!("in6_pcbrtentry: placeholder");
    None
}

/// `in6_pcblookup`: the pcb of `table` connected with exactly this quadruple.
pub fn in6_pcblookup(
    table: &Inpcbtable,
    faddr: &In6Addr,
    fport: u16,
    laddr: &In6Addr,
    lport: u16,
    rtable: u32,
) -> Option<&'static Inpcb> {
    let _ = (table, faddr, fport, laddr, lport, rtable);
    let _ = crate::unported!("in6_pcblookup: placeholder");
    None
}

/// `in6_pcblookup_listen`: the pcb of `table` listening on `laddr`/`lport_arg` or on
/// the wildcard address (pf's divert and redirect keys from `m`).
pub fn in6_pcblookup_listen(
    table: &Inpcbtable,
    laddr: &In6Addr,
    lport_arg: u16,
    m: Option<&Mbuf>,
    rtable: u32,
) -> Option<&'static Inpcb> {
    let _ = (table, laddr, lport_arg, m, rtable);
    let _ = crate::unported!("in6_pcblookup_listen: placeholder");
    None
}

/// `in6_pcbset_addr`: sets both addresses and ports of `inp` and rehashes it (the
/// IPv6 half of `in_pcbset_addr`).
pub fn in6_pcbset_addr(
    inp: &'static Inpcb,
    fsin6: &SockaddrIn6,
    lsin6: &SockaddrIn6,
    rtableid: u32,
) -> Result<(), Errno> {
    let _ = (inp, fsin6, lsin6, rtableid);
    Err(crate::unported!("in6_pcbset_addr: placeholder"))
}
