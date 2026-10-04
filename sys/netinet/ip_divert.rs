/*      $OpenBSD: ip_divert.h,v 1.30 2025/06/23 12:05:46 bluhm Exp $ */
/*      $OpenBSD: ip_divert.c,v 1.108 2026/06/24 15:56:17 claudio Exp $ */
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

//! Divert sockets: `<netinet/ip_divert.h>` and `netinet/ip_divert.c`. A pf rule with
//! `divert-packet port N` hands the packets it matches to the `IPPROTO_DIVERT` raw socket
//! bound to port N (`divert_packet`), with the address of the receiving interface (inbound)
//! or the wildcard (outbound) as the source; what the socket writes back is reinjected, into
//! `ipv4_input` for an address of ours, out through `ip_output` for the wildcard.
//!
//! Upstream: sys/netinet/ip_divert.h @ 3ce1f3f79392
//! Upstream: sys/netinet/ip_divert.c @ 3ce1f3f79392
//!
//! Locks used to protect data: \[a\] atomic.
//!
//! ## Deviations
//! - `divcounters` (`struct cpumem *`) is the static array of atomics [`DIVCOUNTERS`], as
//!   `udpcounters` is (`netinet/udp_usrreq.rs`); `counters_alloc` in `divert_init` has
//!   nothing left to do.
//! - `divert_sendspace` and `divert_recvspace` are `AtomicI32`s, the type
//!   `sysctl_bounded_arr` takes (`u_int` in C; the bounds keep them positive).
//! - `DIVERTCTL_NAMES` (the `struct ctlname` table for `sysctl(8)`) is userland's.
//! - `divert_packet` takes the packet by reference (it frees or queues it, as the C does)
//!   and skips the iterator markers `in_pcb_iterator` leaves in the table's queue, which the
//!   C's `TAILQ_FOREACH` would read as control blocks.
//! - `divert_output` with no address (which `sosend` never passes for an unconnected
//!   `PR_ADDR` socket) fails with `EINVAL` where the C would dereference NULL.
//! - The INET6 half (`divb6table`, `divert6_*`, `netinet6/ip6_divert.c`) is not configured.

use core::mem::size_of;
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_sysctl::{sysctl_bounded_arr, sysctl_rdstruct};
use crate::kern::uipc_mbuf::{m_freem, m_pullup};
use crate::kern::uipc_socket::sorwakeup;
use crate::kern::uipc_socket2::{sbappendaddr, soassertlocked, socantsendmore, soreserve};
use crate::net::if_::{if_get, if_put};
use crate::net::pfvar::{PF_IN, PF_OUT};
use crate::net::route::{RTF_LOCAL, rtalloc, rtfree, rtisvalid};
use crate::net::rtable::rtable_l2;
use crate::netinet::in_::{
    INADDR_ANY, IPPROTO_ICMP, IPPROTO_TCP, IPPROTO_UDP, SockaddrIn, in_control, in_nam2sin,
    satosin_const, sintosa,
};
use crate::netinet::in_pcb::{
    INP_HDRINCL, Inpcb, Inpcbtable, in_pcb_is_iterator, in_pcballoc, in_pcbbind, in_pcbdetach,
    in_pcbinit, in_pcbref, in_pcbunref, in_peeraddr, in_sockaddr, sotoinpcb,
};
use crate::netinet::ip::{IP_MAXPACKET, Ip};
use crate::netinet::ip_icmp::ICMP_MINLEN;
use crate::netinet::ip_input::ipv4_input;
use crate::netinet::ip_output::{in_hdr_cksum_out, in_proto_cksum_out, ip_output};
use crate::netinet::ip_var::{IP_ALLOWBROADCAST, IP_RAWOUTPUT, mtod_ip};
use crate::netinet::raw_ip::rip_chkhdr;
use crate::netinet::tcp::Tcphdr;
use crate::netinet::udp::Udphdr;
use crate::sys::errno::Errno;
use crate::sys::mbuf::{
    M_ICMP_CSUM_OUT, M_TCP_CSUM_OUT, M_UDP_CSUM_OUT, Mbuf, PF_TAG_DIVERTED_PACKET,
};
use crate::sys::proc::Proc;
use crate::sys::protosw::PrUsrreqs;
use crate::sys::socket::AF_INET;
use crate::sys::socketvar::{SB_MAX, SS_PRIV, Socket};
use crate::sys::sysctl::SysctlBoundedArgs;

/// `DIVERTCTL_RECVSPACE`: receive buffer space.
pub const DIVERTCTL_RECVSPACE: i32 = 1;
/// `DIVERTCTL_SENDSPACE`: send buffer space.
pub const DIVERTCTL_SENDSPACE: i32 = 2;
/// `DIVERTCTL_STATS`: divert statistics.
pub const DIVERTCTL_STATS: i32 = 3;
/// `DIVERTCTL_MAXID`.
pub const DIVERTCTL_MAXID: i32 = 4;

/// `DIVERT_SENDSPACE`.
pub const DIVERT_SENDSPACE: i32 = 65536 + 100;
/// `DIVERT_RECVSPACE`.
pub const DIVERT_RECVSPACE: i32 = 65536 + 100;
/// `DIVERT_HASHSIZE`.
pub const DIVERT_HASHSIZE: i32 = 128;

/// `struct divstat`: `net.inet.divert.stats`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Divstat {
    /// `divs_ipackets`: total input packets.
    pub divs_ipackets: u64,
    /// `divs_noport`: no socket on port.
    pub divs_noport: u64,
    /// `divs_fullsock`: not delivered, input socket full.
    pub divs_fullsock: u64,
    /// `divs_opackets`: total output packets.
    pub divs_opackets: u64,
    /// `divs_errors`: generic errors.
    pub divs_errors: u64,
}

/// `enum divstat_counters`: the indices of [`DIVCOUNTERS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum DivstatCounters {
    /// `divs_ipackets`.
    DivsIpackets,
    /// `divs_noport`.
    DivsNoport,
    /// `divs_fullsock`.
    DivsFullsock,
    /// `divs_opackets`.
    DivsOpackets,
    /// `divs_errors`.
    DivsErrors,
    /// `divs_ncounters`.
    DivsNcounters,
}

/// The number of counters (`divs_ncounters`).
pub const DIVS_NCOUNTERS: usize = DivstatCounters::DivsNcounters as usize;

/// `divbtable`.
pub static DIVBTABLE: Inpcbtable = Inpcbtable::new();

/// `divcounters`.
pub static DIVCOUNTERS: [AtomicU64; DIVS_NCOUNTERS] = [const { AtomicU64::new(0) }; DIVS_NCOUNTERS];

/// \[a\] `divert_sendspace`.
#[allow(non_upper_case_globals)] // `DIVERT_SENDSPACE` is the default, a constant of the header
pub static divert_sendspace: AtomicI32 = AtomicI32::new(DIVERT_SENDSPACE);
/// \[a\] `divert_recvspace`.
#[allow(non_upper_case_globals)] // `DIVERT_RECVSPACE` is the default, a constant of the header
pub static divert_recvspace: AtomicI32 = AtomicI32::new(DIVERT_RECVSPACE);

/// `divertctl_vars[]`.
static DIVERTCTL_VARS: [SysctlBoundedArgs; 2] = [
    SysctlBoundedArgs::new(DIVERTCTL_RECVSPACE, &divert_recvspace, 0, SB_MAX as i32),
    SysctlBoundedArgs::new(DIVERTCTL_SENDSPACE, &divert_sendspace, 0, SB_MAX as i32),
];

/// `divert_usrreqs`.
pub static DIVERT_USRREQS: PrUsrreqs = PrUsrreqs {
    pru_attach: Some(divert_attach),
    pru_detach: Some(divert_detach),
    pru_bind: Some(divert_bind),
    pru_shutdown: Some(divert_shutdown),
    pru_send: Some(divert_send),
    pru_control: Some(in_control),
    pru_sockaddr: Some(in_sockaddr),
    pru_peeraddr: Some(in_peeraddr),
    ..PrUsrreqs::NONE
};

/// `divstat_inc(c)`.
pub fn divstat_inc(c: DivstatCounters) {
    DIVCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `divert_init`.
pub fn divert_init() {
    in_pcbinit(&DIVBTABLE, DIVERT_HASHSIZE);
    // divcounters = counters_alloc(divs_ncounters): a static array here.
}

/// `divert_output`: reinjects a packet a divert socket wrote: inbound (into `ipv4_input`)
/// when its address `nam` is one of ours, outbound (through `ip_output`) for the wildcard.
fn divert_output(
    inp: &'static Inpcb,
    m: &'static Mbuf,
    nam: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    m_freem(control);

    let mut m = m;
    let error = 'fail: {
        let Some(nam) = nam else {
            break 'fail Errno::EINVAL;
        };
        let sin = match in_nam2sin(nam) {
            // SAFETY: `in_nam2sin` checked the mbuf holds a whole `sockaddr_in`; read it
            // unaligned into a local.
            Ok(sin) => unsafe { sin.read_unaligned() },
            Err(e) => break 'fail e,
        };

        if m.m_pkthdr().len.get() as usize > IP_MAXPACKET {
            break 'fail Errno::EMSGSIZE;
        }

        let Some(n) = rip_chkhdr(m, None) else {
            divstat_inc(DivstatCounters::DivsErrors);
            return Err(Errno::EINVAL);
        };
        m = n;

        let ip: Ip = mtod_ip(m);
        let off = i32::from(ip.ip_hl()) << 2;

        let dir = if sin.sin_addr.s_addr == INADDR_ANY {
            PF_OUT
        } else {
            PF_IN
        };

        let ph = m.m_pkthdr();
        let min_hdrlen = match i32::from(ip.ip_p) {
            IPPROTO_TCP => {
                ph.csum_flags.set(ph.csum_flags.get() | M_TCP_CSUM_OUT);
                size_of::<Tcphdr>() as i32
            }
            IPPROTO_UDP => {
                ph.csum_flags.set(ph.csum_flags.get() | M_UDP_CSUM_OUT);
                size_of::<Udphdr>() as i32
            }
            IPPROTO_ICMP => {
                ph.csum_flags.set(ph.csum_flags.get() | M_ICMP_CSUM_OUT);
                ICMP_MINLEN as i32
            }
            _ => 0,
        };
        if min_hdrlen != 0 && ph.len.get() < off + min_hdrlen {
            break 'fail Errno::EINVAL;
        }

        ph.pf.flags.set(ph.pf.flags.get() | PF_TAG_DIVERTED_PACKET);

        let result = if dir == PF_IN {
            let mut dst = sin;
            // SAFETY: a local `sockaddr_in`, read for the call.
            let rt = unsafe { rtalloc(sintosa(&mut dst), 0, inp.inp_rtableid.get()) };
            if !rtisvalid(rt) || rt.is_none_or(|rt| rt.rt_flags.get() & RTF_LOCAL == 0) {
                rtfree(rt);
                break 'fail Errno::EADDRNOTAVAIL;
            }
            if let Some(rt) = rt {
                ph.ph_ifidx.set(rt.rt_ifidx.get());
            }
            rtfree(rt);

            // Recalculate IP and protocol checksums for the inbound packet since the
            // userspace application may have modified the packet prior to reinjection.
            in_hdr_cksum_out(m, None);
            in_proto_cksum_out(m, None);

            let Some(ifp) = if_get(ph.ph_ifidx.get()) else {
                break 'fail Errno::ENETDOWN;
            };
            ipv4_input(ifp, m, None);
            if_put(ifp);
            Ok(())
        } else {
            ph.ph_rtableid.set(inp.inp_rtableid.get());

            ip_output(
                m,
                None,
                Some(&inp.inp_route),
                IP_ALLOWBROADCAST | IP_RAWOUTPUT,
                None,
                None,
                0,
            )
        };

        divstat_inc(DivstatCounters::DivsOpackets);
        return result;
    };

    m_freem(m);
    divstat_inc(DivstatCounters::DivsErrors);
    Err(error)
}

/// The bytes of a `sockaddr_in`.
fn sin_bytes(sin: &SockaddrIn) -> &[u8] {
    // SAFETY: `SockaddrIn` is `#[repr(C)]` without padding: its bytes are initialised.
    unsafe { slice::from_raw_parts(ptr::from_ref(sin).cast::<u8>(), size_of::<SockaddrIn>()) }
}

/// `divert_packet`: hands a packet pf diverted (`dir` as pf saw it) to the divert socket
/// bound to `divert_port` in the packet's routing domain, or frees it.
pub fn divert_packet(m: &'static Mbuf, dir: u8, divert_port: u16) {
    let mut inp: Option<&'static Inpcb> = None;
    let mut m = Some(m);

    divstat_inc(DivstatCounters::DivsIpackets);

    'bad: {
        let Some(m0) = m else {
            break 'bad;
        };
        if (m0.m_len().get() as usize) < size_of::<Ip>() {
            m = m_pullup(m0, size_of::<Ip>() as i32);
            if m.is_none() {
                divstat_inc(DivstatCounters::DivsErrors);
                break 'bad;
            }
        }
        let Some(mm) = m else {
            break 'bad;
        };

        let rdomain = rtable_l2(mm.m_pkthdr().ph_rtableid.get());
        mtx_enter(&DIVBTABLE.inpt_mtx);
        for i in DIVBTABLE.inpt_queue.iter() {
            if in_pcb_is_iterator(i) {
                continue;
            }
            if i.inp_lport.get() != divert_port || rtable_l2(i.inp_rtableid.get()) != rdomain {
                continue;
            }
            inp = in_pcbref(Some(i));
            break;
        }
        mtx_leave(&DIVBTABLE.inpt_mtx);
        let Some(inp) = inp else {
            divstat_inc(DivstatCounters::DivsNoport);
            break 'bad;
        };

        let mut sin = SockaddrIn {
            sin_family: AF_INET,
            sin_len: size_of::<SockaddrIn>() as u8,
            ..SockaddrIn::default()
        };

        if dir == PF_IN {
            let Some(ifp) = if_get(mm.m_pkthdr().ph_ifidx.get()) else {
                divstat_inc(DivstatCounters::DivsErrors);
                break 'bad;
            };
            for ifa in ifp.if_addrlist.iter() {
                let sa = ifa.ifa_addr.get();
                // SAFETY: an interface address's `ifa_addr` is a readable socket address of
                // its family (`ifa_add`'s contract); an `AF_INET` one is a `sockaddr_in`.
                if unsafe { (*sa).sa_family } != AF_INET {
                    continue;
                }
                // SAFETY: as above.
                sin.sin_addr = unsafe { satosin_const(sa).read_unaligned() }.sin_addr;
                break;
            }
            if_put(ifp);
        } else {
            // Calculate IP and protocol checksums for outbound packet diverted to userland.
            // pf rule diverts before cksum offload.
            in_hdr_cksum_out(mm, None);
            in_proto_cksum_out(mm, None);
        }

        let so = inp.socket();
        mtx_enter(&so.so_rcv.sb_mtx);
        if !sbappendaddr(&so.so_rcv, sin_bytes(&sin), Some(mm), None) {
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

/// `divert_attach`: a control block for a privileged divert socket.
pub fn divert_attach(so: &'static Socket, _proto: i32, wait: i32) -> Result<(), Errno> {
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
    in_pcballoc(so, &DIVBTABLE, wait)?;

    if let Some(inp) = sotoinpcb(so) {
        inp.inp_flags.set(inp.inp_flags.get() | INP_HDRINCL);
    }
    Ok(())
}

/// `divert_detach`.
pub fn divert_detach(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);

    let Some(inp) = sotoinpcb(so) else {
        return Err(Errno::EINVAL);
    };

    in_pcbdetach(inp);
    Ok(())
}

/// `divert_bind`: the divert port of the socket (`in_pcbbind`).
pub fn divert_bind(so: &'static Socket, addr: &'static Mbuf, p: &Proc) -> Result<(), Errno> {
    soassertlocked(so);
    let Some(inp) = sotoinpcb(so) else {
        return Err(Errno::EINVAL);
    };
    in_pcbbind(inp, Some(addr), p)
}

/// `divert_shutdown`.
pub fn divert_shutdown(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);
    socantsendmore(so);
    Ok(())
}

/// `divert_send`: reinjects what the socket writes (`divert_output`).
pub fn divert_send(
    so: &'static Socket,
    m: Option<&'static Mbuf>,
    addr: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    soassertlocked(so);
    let (Some(inp), Some(m)) = (sotoinpcb(so), m) else {
        m_freem(m);
        m_freem(control);
        return Err(Errno::EINVAL);
    };
    divert_output(inp, m, addr, control)
}

/// `divert_sysctl_divstat`: `net.inet.divert.stats`, the counters as a `struct divstat`.
fn divert_sysctl_divstat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    let mut bytes = [0u8; DIVS_NCOUNTERS * size_of::<u64>()];
    for (i, c) in DIVCOUNTERS.iter().enumerate() {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&c.load(Ordering::Relaxed).to_ne_bytes());
    }

    sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
}

/// `divert_sysctl`: sysctl for divert variables (`net.inet.divert.*`).
pub fn divert_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    // All sysctl names at this level are terminal.
    let [n] = name else {
        return Err(Errno::ENOTDIR);
    };

    match *n {
        DIVERTCTL_STATS => divert_sysctl_divstat(oldp, oldlenp, newp),
        _ => sysctl_bounded_arr(&DIVERTCTL_VARS, name, oldp, oldlenp, newp, newlen),
    }
}

// The counters are the structure's words (`CTASSERT` in `divert_sysctl_divstat`).
const _: () = assert!(size_of::<Divstat>() == DIVS_NCOUNTERS * size_of::<u64>());

#[cfg(test)]
mod tests;
