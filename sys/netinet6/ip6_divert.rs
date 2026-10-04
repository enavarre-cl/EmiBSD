/*      $OpenBSD: ip6_divert.c,v 1.109 2026/06/24 15:56:17 claudio Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2009 Michele Marchetto <michele@openbsd.org>
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
/* </LICENSES> */

//! Divert sockets for IPv6: `netinet6/ip6_divert.c` (prototypes in
//! `<netinet/ip_divert.h>`, `netinet/ip_divert.rs`).
//!
//! Upstream: sys/netinet6/ip6_divert.c @ 3ce1f3f79392
//!
//! `div6counters` is the static array of atomics [`DIV6COUNTERS`], indexed by
//! `netinet/ip_divert.rs`'s `DivstatCounters`.
//!
//! Status: skeleton from the INET6 foundation step: the globals are defined, every
//! function has its final signature and a placeholder body that reports itself through
//! `unported!` until the file is ported.
//!
//! ## Deviations
//! - None yet: the file is a skeleton (see `Status`).

use crate::netinet::in_pcb::Inpcbtable;
use crate::netinet::ip_divert::{DIVS_NCOUNTERS, divert_bind, divert_detach, divert_shutdown};
use crate::netinet6::in6::in6_control;
use crate::netinet6::in6_pcb::{in6_peeraddr, in6_sockaddr};
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::protosw::PrUsrreqs;
use crate::sys::socketvar::Socket;
use core::sync::atomic::AtomicU64;

/// `divb6table`.
pub static DIVB6TABLE: Inpcbtable = Inpcbtable::new();

/// `div6counters`: the IPv6 divert statistics.
pub static DIV6COUNTERS: [AtomicU64; DIVS_NCOUNTERS] =
    [const { AtomicU64::new(0) }; DIVS_NCOUNTERS];

/// `divert6_usrreqs`.
pub static DIVERT6_USRREQS: PrUsrreqs = PrUsrreqs {
    pru_attach: Some(divert6_attach),
    pru_detach: Some(divert_detach),
    pru_bind: Some(divert_bind),
    pru_shutdown: Some(divert_shutdown),
    pru_send: Some(divert6_send),
    pru_control: Some(in6_control),
    pru_sockaddr: Some(in6_sockaddr),
    pru_peeraddr: Some(in6_peeraddr),
    ..PrUsrreqs::NONE
};

/// `divert6_init`: initializes the divert pcb table and counters.
pub fn divert6_init() {
    let _ = crate::unported!("divert6_init: placeholder");
}

/// `divert6_packet`: hands `m` (direction `dir`, `PF_IN`/`PF_OUT`) to the divert socket
/// bound to `divert_port`; consumes it.
pub fn divert6_packet(m: &'static Mbuf, dir: u8, divert_port: u16) {
    let _ = (m, dir, divert_port);
    let _ = crate::unported!("divert6_packet: placeholder");
}

/// `divert6_attach`: `pru_attach`.
pub fn divert6_attach(so: &'static Socket, proto: i32, wait: i32) -> Result<(), Errno> {
    let _ = (so, proto, wait);
    Err(crate::unported!("divert6_attach: placeholder"))
}

/// `divert6_send`: `pru_send`: reinjects a packet written to the socket.
pub fn divert6_send(
    so: &'static Socket,
    m: Option<&'static Mbuf>,
    nam: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let _ = (so, m, nam, control);
    Err(crate::unported!("divert6_send: placeholder"))
}
