/*	$OpenBSD: raw_ip6.h,v 1.4 2017/02/09 15:23:35 jca Exp $	*/
/*	$KAME: raw_ip6.h,v 1.2 2001/05/27 13:28:35 itojun Exp $	*/
/*	$OpenBSD: raw_ip6.c,v 1.195 2026/09/17 15:56:59 bluhm Exp $	*/
/*	$KAME: raw_ip6.c,v 1.69 2001/03/04 15:55:44 itojun Exp $	*/
/* <LICENSES> */
/*
 * Copyright (C) 2001 WIDE Project.
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
 * Copyright (c) 1982, 1986, 1988, 1993
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
 *	@(#)raw_ip.c	8.2 (Berkeley) 1/4/94
 */
/* </LICENSES> */

//! Raw IPv6 sockets: the statistics of `<netinet6/raw_ip6.h>`, and `netinet6/raw_ip6.c`,
//! the raw IPv6 protocol (input to every matching socket, output with a caller-built
//! payload, the `IPV6_CHECKSUM` and `ICMP6_FILTER` options).
//!
//! Upstream: sys/netinet6/raw_ip6.h @ 3ce1f3f79392
//! Upstream: sys/netinet6/raw_ip6.c @ 3ce1f3f79392
//!
//! `raw_ip6.c` shares the module with the header. ICMPv6 statistics are counted separately
//! (`netinet/icmp6.rs`).
//!
//! ## Deviations
//! - `enum rip6stat_counters` is [`Rip6statCounters`]; `rip6counters` (`struct cpumem *`) is
//!   the static array of atomics [`RIP6COUNTERS`].
//! - `RIPM6CTL_NAMES` is a `Ctlname` table.

use crate::net::if_var::Netstack;
use crate::netinet::in_pcb::Inpcbtable;
use crate::netinet6::in6::{SockaddrIn6, in6_control};
use crate::netinet6::in6_pcb::{in6_peeraddr, in6_sockaddr};
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::proc::Proc;
use crate::sys::protosw::PrUsrreqs;
use crate::sys::socket::Sockaddr;
use crate::sys::socketvar::Socket;
use core::ffi::c_void;
use core::mem::size_of;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::sys::sysctl::{CTLTYPE_NODE, Ctlname};

/// RIP6 stats.
pub const RIPV6CTL_STATS: i32 = 1;
/// `RIPV6CTL_MAXID`.
pub const RIPV6CTL_MAXID: i32 = 2;

/// `RIPM6CTL_NAMES`.
pub const RIPM6CTL_NAMES: [Ctlname; RIPV6CTL_MAXID as usize] =
    [Ctlname::NONE, Ctlname::new(b"stats", CTLTYPE_NODE)];

/// `struct rip6stat`: raw IPv6 statistics, as `sysctl(2)` returns them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rip6stat {
    /// Total input packets.
    pub rip6s_ipackets: u64,
    /// Input checksum computations.
    pub rip6s_isum: u64,
    /// Of above, checksum error.
    pub rip6s_badsum: u64,
    /// No matching socket.
    pub rip6s_nosock: u64,
    /// Of above, arrived as multicast.
    pub rip6s_nosockmcast: u64,
    /// Not delivered, input socket full.
    pub rip6s_fullsock: u64,
    /// Total output packets.
    pub rip6s_opackets: u64,
}

/// `enum rip6stat_counters`: the indices of [`RIP6COUNTERS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Rip6statCounters {
    /// `rip6s_ipackets`.
    Rip6sIpackets,
    /// `rip6s_isum`.
    Rip6sIsum,
    /// `rip6s_badsum`.
    Rip6sBadsum,
    /// `rip6s_nosock`.
    Rip6sNosock,
    /// `rip6s_nosockmcast`.
    Rip6sNosockmcast,
    /// `rip6s_fullsock`.
    Rip6sFullsock,
    /// `rip6s_opackets`.
    Rip6sOpackets,
    /// `rip6s_ncounters`.
    Rip6sNcounters,
}

/// The number of counters (`rip6s_ncounters`).
pub const RIP6S_NCOUNTERS: usize = Rip6statCounters::Rip6sNcounters as usize;

/// `rawin6pcbtable`.
pub static RAWIN6PCBTABLE: Inpcbtable = Inpcbtable::new();

/// `rip6counters`: the raw IPv6 statistics.
pub static RIP6COUNTERS: [AtomicU64; RIP6S_NCOUNTERS] =
    [const { AtomicU64::new(0) }; RIP6S_NCOUNTERS];

/// `rip6_usrreqs`.
pub static RIP6_USRREQS: PrUsrreqs = PrUsrreqs {
    pru_attach: Some(rip6_attach),
    pru_detach: Some(rip6_detach),
    pru_bind: Some(rip6_bind),
    pru_connect: Some(rip6_connect),
    pru_disconnect: Some(rip6_disconnect),
    pru_shutdown: Some(rip6_shutdown),
    pru_send: Some(rip6_send),
    pru_control: Some(in6_control),
    pru_sockaddr: Some(in6_sockaddr),
    pru_peeraddr: Some(in6_peeraddr),
    ..PrUsrreqs::NONE
};

/// `rip6stat_inc(c)`.
pub fn rip6stat_inc(c: Rip6statCounters) {
    RIP6COUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `rip6_init`: initializes the raw connection block table and counters.
pub fn rip6_init() {
    let _ = crate::unported!("rip6_init: placeholder");
}

/// `rip6_input`: raw IPv6's `pr_input`: delivers to every matching socket.
pub fn rip6_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let _ = (mp, offp, proto, af, ns);
    let _ = crate::unported!("rip6_input: placeholder");
    crate::netinet::in_::IPPROTO_DONE
}

/// `rip6_ctlinput`: raw IPv6's `pr_ctlinput`.
///
/// # Safety
///
/// `sa` points at a readable socket address of its `sa_len` bytes; `d` is NULL or
/// the `Ip6ctlparam` of the ICMPv6 error, valid for the call.
pub unsafe fn rip6_ctlinput(cmd: i32, sa: *const Sockaddr, rdomain: u32, d: *mut c_void) {
    let _ = (cmd, sa, rdomain, d);
    let _ = crate::unported!("rip6_ctlinput: placeholder");
}

/// `rip6_output`: sends `m` built by the user to `dstaddr`, with the control messages in
/// `control`. Consumes both.
pub fn rip6_output(
    m: &'static Mbuf,
    so: &'static Socket,
    dstaddr: &SockaddrIn6,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let _ = (m, so, dstaddr, control);
    Err(crate::unported!("rip6_output: placeholder"))
}

/// `rip6_ctloutput`: raw IPv6's `pr_ctloutput`.
pub fn rip6_ctloutput(
    op: i32,
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let _ = (op, so, level, optname, m);
    Err(crate::unported!("rip6_ctloutput: placeholder"))
}

/// `rip6_attach`: `pru_attach`.
pub fn rip6_attach(so: &'static Socket, proto: i32, wait: i32) -> Result<(), Errno> {
    let _ = (so, proto, wait);
    Err(crate::unported!("rip6_attach: placeholder"))
}

/// `rip6_detach`: `pru_detach`.
pub fn rip6_detach(so: &'static Socket) -> Result<(), Errno> {
    let _ = so;
    Err(crate::unported!("rip6_detach: placeholder"))
}

/// `rip6_bind`: `pru_bind`.
pub fn rip6_bind(so: &'static Socket, nam: &'static Mbuf, p: &Proc) -> Result<(), Errno> {
    let _ = (so, nam, p);
    Err(crate::unported!("rip6_bind: placeholder"))
}

/// `rip6_connect`: `pru_connect`.
pub fn rip6_connect(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    let _ = (so, nam);
    Err(crate::unported!("rip6_connect: placeholder"))
}

/// `rip6_disconnect`: `pru_disconnect`.
pub fn rip6_disconnect(so: &'static Socket) -> Result<(), Errno> {
    let _ = so;
    Err(crate::unported!("rip6_disconnect: placeholder"))
}

/// `rip6_shutdown`: `pru_shutdown`.
pub fn rip6_shutdown(so: &'static Socket) -> Result<(), Errno> {
    let _ = so;
    Err(crate::unported!("rip6_shutdown: placeholder"))
}

/// `rip6_send`: `pru_send`.
pub fn rip6_send(
    so: &'static Socket,
    m: Option<&'static Mbuf>,
    nam: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let _ = (so, m, nam, control);
    Err(crate::unported!("rip6_send: placeholder"))
}

/// `rip6_sysctl`: the `net.inet6.ip6.rip6` sysctls (`RIPV6CTL_STATS`).
pub fn rip6_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let _ = (name, oldp, oldlenp, newp, newlen);
    Err(crate::unported!("rip6_sysctl: placeholder"))
}

// The counters are the statistics' words.
const _: () = assert!(size_of::<Rip6stat>() == RIP6S_NCOUNTERS * size_of::<u64>());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet6/raw_ip6.h");
        let ctl = crate::reftest::assert_defines!(defs; RIPV6CTL_STATS, RIPV6CTL_MAXID);
        crate::reftest::assert_complete(&defs, "RIPV6CTL_", &ctl);
    }
}
