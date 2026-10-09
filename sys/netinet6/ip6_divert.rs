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

/* <CODE> */
//! Divert sockets for IPv6: `netinet6/ip6_divert.c` (prototypes in
//! `<netinet/ip_divert.h>`, `netinet/ip_divert.rs`). A pf rule with `divert-packet port N`
//! hands the IPv6 packets it matches to the `IPPROTO_DIVERT` raw socket of the inet6 domain
//! bound to port N (`divert6_packet`), with an IPv6 address of the receiving interface
//! (inbound) or the wildcard (outbound) as the source; what the socket writes back is
//! reinjected, into `ipv6_input` for an address of ours, out through `ip6_output` for the
//! wildcard.
//!
//! Upstream: sys/netinet6/ip6_divert.c @ 3ce1f3f79392
//!
//! Locks used to protect data: \[a\] atomic.
//!
//! ## Deviations
//! - `div6counters` is the static array of atomics [`DIV6COUNTERS`], indexed by
//!   `netinet/ip_divert.rs`'s `DivstatCounters`; `counters_alloc` in `divert6_init` has
//!   nothing left to do. As in C, the IPv6 paths count in `divcounters` (`divstat_inc`),
//!   so `div6counters` stays at zero.
//! - `divert6_packet` takes the packet by reference (it frees or queues it, as the C does)
//!   and skips the iterator markers `in_pcb_iterator` leaves in the table's queue, which
//!   the C's `TAILQ_FOREACH` would read as control blocks (as `divert_packet` does).
//! - `divert6_output` with no address (which `sosend` never passes for an unconnected
//!   `PR_ADDR` socket) fails with `EINVAL` where the C would dereference NULL, and
//!   `divert6_send` without a packet fails with `EINVAL`, as `divert_send` does.

use core::mem::size_of;
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::uipc_mbuf::{m_freem, m_pullup};
use crate::kern::uipc_socket::sorwakeup;
use crate::kern::uipc_socket2::{sbappendaddr, soassertlocked, soreserve};
use crate::net::if_::{if_get, if_put};
use crate::net::pfvar::{PF_IN, PF_OUT};
use crate::net::route::{RTF_LOCAL, rtalloc, rtfree, rtisvalid};
use crate::net::rtable::rtable_l2;
use crate::netinet::icmp6::Icmp6Hdr;
use crate::netinet::in_::{IPPROTO_ICMPV6, IPPROTO_IPV6, IPPROTO_TCP, IPPROTO_UDP};
use crate::netinet::in_pcb::{
    Inpcb, Inpcbtable, in_pcb_is_iterator, in_pcballoc, in_pcbinit, in_pcbref, in_pcbunref,
    sotoinpcb,
};
use crate::netinet::ip_divert::{
    DIVERT_HASHSIZE, DIVS_NCOUNTERS, DivstatCounters, divert_bind, divert_detach, divert_recvspace,
    divert_sendspace, divert_shutdown, divstat_inc,
};
use crate::netinet::ip_var::{IP_ALLOWBROADCAST, IP_RAWOUTPUT};
use crate::netinet::ip6::{IPV6_VERSION, IPV6_VERSION_MASK, Ip6Hdr};
use crate::netinet::tcp::Tcphdr;
use crate::netinet::udp::Udphdr;
use crate::netinet6::in6::{
    IN6ADDR_ANY, SockaddrIn6, in6_control, in6_is_addr_unspecified, in6_nam2sin6, satosin6_const,
    sin6tosa_const,
};
use crate::netinet6::in6_pcb::{in6_peeraddr, in6_sockaddr};
use crate::netinet6::ip6_input::{ip6_lasthdr, ipv6_input};
use crate::netinet6::ip6_output::{in6_proto_cksum_out, ip6_output};
use crate::netinet6::ip6_var::mtod_ip6;
use crate::sys::endian::ntohs;
use crate::sys::errno::Errno;
use crate::sys::mbuf::{
    M_ICMP_CSUM_OUT, M_TCP_CSUM_OUT, M_UDP_CSUM_OUT, Mbuf, PF_TAG_DIVERTED_PACKET,
};
use crate::sys::protosw::PrUsrreqs;
use crate::sys::socket::AF_INET6;
use crate::sys::socketvar::{SS_PRIV, Socket};

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

/// `divert6_init`: initializes the divert pcb table.
pub fn divert6_init() {
    in_pcbinit(&DIVB6TABLE, DIVERT_HASHSIZE);
    // div6counters = counters_alloc(divs_ncounters): a static array here.
}

/// `divert6_output`: reinjects a packet a divert socket wrote: inbound (into `ipv6_input`)
/// when its address `nam` is one of ours, outbound (through `ip6_output`) for the wildcard.
/// The protocol checksum is recalculated, since the userspace application may have
/// modified the packet prior to reinjection.
fn divert6_output(
    inp: &'static Inpcb,
    m: &'static Mbuf,
    nam: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    m_freem(control);

    let mut m = m;
    let error: Option<Errno> = 'fail: {
        let Some(nam) = nam else {
            break 'fail Some(Errno::EINVAL);
        };
        let sin6 = match in6_nam2sin6(nam) {
            // SAFETY: `in6_nam2sin6` checked the mbuf holds a whole `sockaddr_in6`; read it
            // unaligned into a local.
            Ok(sin6) => unsafe { sin6.read_unaligned() },
            Err(e) => break 'fail Some(e),
        };

        // Do basic sanity checks.
        if (m.m_pkthdr().len.get() as usize) < size_of::<Ip6Hdr>() {
            break 'fail None;
        }
        let Some(n) = m_pullup(m, size_of::<Ip6Hdr>() as i32) else {
            // m_pullup() has freed the mbuf, so just return.
            divstat_inc(DivstatCounters::DivsErrors);
            return Err(Errno::ENOBUFS);
        };
        m = n;
        let ip6 = mtod_ip6(m);
        if ip6.ip6_vfc() & IPV6_VERSION_MASK != IPV6_VERSION {
            break 'fail None;
        }
        if (m.m_pkthdr().len.get() as usize)
            < size_of::<Ip6Hdr>() + usize::from(ntohs(ip6.ip6_plen))
        {
            break 'fail None;
        }

        // Recalculate the protocol checksum since the userspace application may have
        // modified the packet prior to reinjection.
        let mut nxt = 0;
        let off = match ip6_lasthdr(m, 0, IPPROTO_IPV6, &mut nxt) {
            Some(off) if off as usize >= size_of::<Ip6Hdr>() => off,
            _ => break 'fail None,
        };

        let dir = if in6_is_addr_unspecified(&sin6.sin6_addr) {
            PF_OUT
        } else {
            PF_IN
        };

        let ph = m.m_pkthdr();
        let min_hdrlen = match nxt {
            IPPROTO_TCP => {
                ph.csum_flags.set(ph.csum_flags.get() | M_TCP_CSUM_OUT);
                size_of::<Tcphdr>() as i32
            }
            IPPROTO_UDP => {
                ph.csum_flags.set(ph.csum_flags.get() | M_UDP_CSUM_OUT);
                size_of::<Udphdr>() as i32
            }
            IPPROTO_ICMPV6 => {
                ph.csum_flags.set(ph.csum_flags.get() | M_ICMP_CSUM_OUT);
                size_of::<Icmp6Hdr>() as i32
            }
            _ => 0,
        };
        if min_hdrlen != 0 && ph.len.get() < off + min_hdrlen {
            break 'fail None;
        }

        ph.pf.flags.set(ph.pf.flags.get() | PF_TAG_DIVERTED_PACKET);

        let result = if dir == PF_IN {
            // SAFETY: a local `sockaddr_in6`, read for the call.
            let rt = unsafe { rtalloc(sin6tosa_const(&sin6), 0, inp.inp_rtableid.get()) };
            if !rtisvalid(rt) || rt.is_none_or(|rt| rt.rt_flags.get() & RTF_LOCAL == 0) {
                rtfree(rt);
                break 'fail Some(Errno::EADDRNOTAVAIL);
            }
            if let Some(rt) = rt {
                ph.ph_ifidx.set(rt.rt_ifidx.get());
            }
            rtfree(rt);

            // Recalculate the protocol checksum for the inbound packet since the userspace
            // application may have modified the packet prior to reinjection.
            in6_proto_cksum_out(m, None);

            let Some(ifp) = if_get(ph.ph_ifidx.get()) else {
                break 'fail Some(Errno::ENETDOWN);
            };
            ipv6_input(ifp, m, None);
            if_put(ifp);
            Ok(())
        } else {
            ph.ph_rtableid.set(inp.inp_rtableid.get());

            ip6_output(
                m,
                None,
                Some(&inp.inp_route),
                IP_ALLOWBROADCAST | IP_RAWOUTPUT,
                None,
                None,
            )
        };

        divstat_inc(DivstatCounters::DivsOpackets);
        return result;
    };

    divstat_inc(DivstatCounters::DivsErrors);
    m_freem(m);
    Err(error.unwrap_or(Errno::EINVAL))
}

/// The bytes of a `sockaddr_in6`.
fn sin6_bytes(sin6: &SockaddrIn6) -> &[u8] {
    // SAFETY: `SockaddrIn6` is `#[repr(C)]` without padding: its bytes are initialised.
    unsafe { slice::from_raw_parts(ptr::from_ref(sin6).cast::<u8>(), size_of::<SockaddrIn6>()) }
}

/// `divert6_packet`: hands a packet pf diverted (`dir` as pf saw it) to the divert socket
/// bound to `divert_port` in the packet's routing domain, or frees it.
pub fn divert6_packet(m: &'static Mbuf, dir: u8, divert_port: u16) {
    let mut inp: Option<&'static Inpcb> = None;
    let mut m = Some(m);

    divstat_inc(DivstatCounters::DivsIpackets);

    'bad: {
        let Some(m0) = m else {
            break 'bad;
        };
        if (m0.m_len().get() as usize) < size_of::<Ip6Hdr>() {
            m = m_pullup(m0, size_of::<Ip6Hdr>() as i32);
            if m.is_none() {
                divstat_inc(DivstatCounters::DivsErrors);
                break 'bad;
            }
        }
        let Some(mm) = m else {
            break 'bad;
        };

        let rdomain = rtable_l2(mm.m_pkthdr().ph_rtableid.get());
        mtx_enter(&DIVB6TABLE.inpt_mtx);
        for i in DIVB6TABLE.inpt_queue.iter() {
            if in_pcb_is_iterator(i) {
                continue;
            }
            if i.inp_lport.get() != divert_port || rtable_l2(i.inp_rtableid.get()) != rdomain {
                continue;
            }
            inp = in_pcbref(Some(i));
            break;
        }
        mtx_leave(&DIVB6TABLE.inpt_mtx);
        let Some(inp) = inp else {
            divstat_inc(DivstatCounters::DivsNoport);
            break 'bad;
        };

        let mut sin6 = SockaddrIn6::with_addr(IN6ADDR_ANY);

        if dir == PF_IN {
            let Some(ifp) = if_get(mm.m_pkthdr().ph_ifidx.get()) else {
                divstat_inc(DivstatCounters::DivsErrors);
                break 'bad;
            };
            for ifa in ifp.if_addrlist.iter() {
                let sa = ifa.ifa_addr.get();
                // SAFETY: an interface address's `ifa_addr` is a readable socket address of
                // its family (`ifa_add`'s contract); an `AF_INET6` one is a `sockaddr_in6`.
                if unsafe { (*sa).sa_family } != AF_INET6 {
                    continue;
                }
                // SAFETY: as above.
                sin6.sin6_addr = unsafe { satosin6_const(sa).read_unaligned() }.sin6_addr;
                break;
            }
            if_put(ifp);
        } else {
            // Calculate protocol checksum for outbound packet diverted to userland. pf out
            // rule diverts before cksum offload.
            in6_proto_cksum_out(mm, None);
        }

        let so = inp.socket();
        mtx_enter(&so.so_rcv.sb_mtx);
        if !sbappendaddr(&so.so_rcv, sin6_bytes(&sin6), Some(mm), None) {
            mtx_leave(&so.so_rcv.sb_mtx);
            divstat_inc(DivstatCounters::DivsFullsock);
            break 'bad;
        }
        mtx_leave(&so.so_rcv.sb_mtx);
        sorwakeup(so);

        in_pcbunref(Some(inp));
        return;
    }

    in_pcbunref(inp);
    m_freem(m);
}

/// `divert6_attach`: a control block for a privileged IPv6 divert socket.
pub fn divert6_attach(so: &'static Socket, proto: i32, wait: i32) -> Result<(), Errno> {
    let _ = proto;
    if !so.so_pcb.get().is_null() {
        return Err(Errno::EINVAL);
    }
    if !so.has_state(SS_PRIV) {
        return Err(Errno::EACCES);
    }

    soreserve(
        so,
        divert_sendspace.load(Ordering::Relaxed) as u64,
        divert_recvspace.load(Ordering::Relaxed) as u64,
    )?;
    in_pcballoc(so, &DIVB6TABLE, wait)?;

    Ok(())
}

/// `divert6_send`: reinjects what the socket writes (`divert6_output`).
pub fn divert6_send(
    so: &'static Socket,
    m: Option<&'static Mbuf>,
    nam: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    soassertlocked(so);
    let (Some(inp), Some(m)) = (sotoinpcb(so), m) else {
        m_freem(m);
        m_freem(control);
        return Err(Errno::EINVAL);
    };
    divert6_output(inp, m, nam, control)
}
/* </CODE> */
