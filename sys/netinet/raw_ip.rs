/*	$OpenBSD: raw_ip.c,v 1.168 2026/07/16 12:21:40 bluhm Exp $	*/
/*	$NetBSD: raw_ip.c,v 1.25 1996/02/18 18:58:33 christos Exp $	*/
/* <LICENSES> */
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
/* </LICENSES> */

//! Raw interface to IP protocol: `netinet/raw_ip.c`.
//!
//! Upstream: sys/netinet/raw_ip.c @ 3ce1f3f79392
//!
//! A `SOCK_RAW` socket of the inet domain (ping(8)'s `IPPROTO_ICMP` one, or any protocol
//! number) has a control block in `rawcbtable`. `rip_input`, the input of every protocol
//! without a handler of its own and the end of `icmp_input` and `igmp_input`, hands a copy of
//! each datagram, IP header included, to every raw socket whose protocol and addresses match.
//! `rip_output` prepends an IP header from the control block's prototype (or, with
//! `IP_HDRINCL`, checks the one the user wrote) and sends the datagram with `ip_output`.
//!
//! ## Deviations
//! - The `MRT_*` socket options (`<netinet/ip_mroute.h>`, not ported) are defined here;
//!   `MROUTING` is not configured, so they answer `EOPNOTSUPP` as the C's `#else` does, and
//!   `rip_detach` has no `ip_mrouter_done`.
//! - `rip_input`'s `ripsrc` and `rip_output`'s destination are local `sockaddr_in`s; the IP
//!   header is read and written as a copy (`ip_var.rs`'s `mtod_ip`), since mbuf data need
//!   not be aligned. `rip_output` takes the destination as a `sockaddr_in`; with
//!   `IP_HDRINCL` the header's destination is copied into it, as the C writes through its
//!   pointer.
//! - `ipcounters`' `counters_enter` pair (`ips_noproto` up, `ips_delivered` down) is two
//!   atomic updates.
//! - `rip_chkhdr` reads the options as a byte slice of the pulled-up header.
//! - `rip_sendspace`/`rip_recvspace` (`u_long`, no sysctl) are constants of the same names.
//! - `NPF` (pf(4)) is configured: the divert-to key and `pf_mbuf_link_inpcb`. So is `IPSEC`
//!   (M9c): `rip_output` passes the socket's `inp_seclevel` to `ip_output`.
//! - Not configured, a comment at its site: `INET6`.

use core::mem::size_of;
use core::slice;

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_copym, m_freem, m_prepend, m_pullup};
use crate::kern::uipc_socket::sorwakeup;
use crate::kern::uipc_socket2::{
    sbappendaddr, soassertlocked, socantsendmore, soisconnected, soisdisconnected, soreserve,
};
use crate::net::if_::ifa_ifwithaddr;
use crate::net::if_var::Netstack;
use crate::net::pf::{pf_find_divert, pf_mbuf_link_inpcb};
use crate::net::pfvar::{PF_DIVERT_REPLY, PF_DIVERT_TO};
use crate::net::rtable::rtable_l2;
use crate::netinet::in_::{
    INADDR_ANY, INADDR_BROADCAST, IP_HDRINCL, IPPROTO_DONE, IPPROTO_ICMP, IPPROTO_IP, IPPROTO_MAX,
    InAddr, SockaddrIn, in_broadcast, in_control, in_nam2sin, sintosa,
};
use crate::netinet::in_pcb::{
    INP_CONTROLOPTS, INP_HDRINCL, INP_IPV6, Inpcb, InpcbIterator, Inpcbtable, in_pcb_iterator,
    in_pcballoc, in_pcbdetach, in_pcbinit, in_pcbref, in_pcbselsrc, in_pcbunref, in_peeraddr,
    in_sockaddr, sotoinpcb,
};
use crate::netinet::ip::{
    IP_MAXPACKET, IPOPT_EOL, IPOPT_NOP, IPOPT_OLEN, IPOPT_OPTVAL, IPVERSION, Ip, MAXTTL,
};
use crate::netinet::ip_icmp::{ICMP_UNREACH, ICMP_UNREACH_PROTOCOL, icmp_error};
use crate::netinet::ip_id::ip_randomid;
use crate::netinet::ip_input::ip_savecontrol;
use crate::netinet::ip_output::{ip_ctloutput, ip_output};
use crate::netinet::ip_var::{
    IP_ALLOWBROADCAST, IP_RAWOUTPUT, IpstatCounters, ipstat_dec, ipstat_inc, mtod_ip, mtod_ip_store,
};
use crate::sys::endian::{htons, ntohs};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{M_COPYALL, M_DONTWAIT, Mbuf, PF_TAG_DIVERTED, mtod};
use crate::sys::proc::Proc;
use crate::sys::protosw::{PRCO_SETOPT, PrUsrreqs};
use crate::sys::socket::{AF_INET, SO_BINDANY, SO_TIMESTAMP};
use crate::sys::socketvar::{SS_CANTRCVMORE, SS_ISCONNECTED, SS_PRIV, Socket};

/// `RIPSNDQ`: nominal space allocated to a raw ip socket.
const RIPSNDQ: u64 = 8192;
/// `RIPRCVQ`.
const RIPRCVQ: u64 = 8192;

// The multicast routing socket options (<netinet/ip_mroute.h>).

/// `MRT_INIT`: initialize forwarder.
pub const MRT_INIT: i32 = 100;
/// `MRT_DONE`: shut down forwarder.
pub const MRT_DONE: i32 = 101;
/// `MRT_ADD_VIF`: create virtual interface.
pub const MRT_ADD_VIF: i32 = 102;
/// `MRT_DEL_VIF`: delete virtual interface.
pub const MRT_DEL_VIF: i32 = 103;
/// `MRT_ADD_MFC`: insert forwarding cache entry.
pub const MRT_ADD_MFC: i32 = 104;
/// `MRT_DEL_MFC`: delete forwarding cache entry.
pub const MRT_DEL_MFC: i32 = 105;
/// `MRT_VERSION`: get kernel version number.
pub const MRT_VERSION: i32 = 106;
/// `MRT_ASSERT`: enable assert processing.
pub const MRT_ASSERT: i32 = 107;
/// `MRT_API_SUPPORT`: supported MRT API.
pub const MRT_API_SUPPORT: i32 = 109;
/// `MRT_API_CONFIG`: config MRT API.
pub const MRT_API_CONFIG: i32 = 110;

/// `rip_sendspace`.
pub const RIP_SENDSPACE: u64 = RIPSNDQ;
/// `rip_recvspace`.
pub const RIP_RECVSPACE: u64 = RIPRCVQ;

/// `rawcbtable`.
pub static RAWCBTABLE: Inpcbtable = Inpcbtable::new();

/// `rip_usrreqs`: raw interface to IP protocol.
pub static RIP_USRREQS: PrUsrreqs = PrUsrreqs {
    pru_attach: Some(rip_attach),
    pru_detach: Some(rip_detach),
    pru_bind: Some(rip_bind),
    pru_connect: Some(rip_connect),
    pru_disconnect: Some(rip_disconnect),
    pru_shutdown: Some(rip_shutdown),
    pru_send: Some(rip_send),
    pru_control: Some(in_control),
    pru_sockaddr: Some(in_sockaddr),
    pru_peeraddr: Some(in_peeraddr),
    ..PrUsrreqs::NONE
};

/// The bytes of a `sockaddr_in`.
fn sin_bytes(sin: &SockaddrIn) -> &[u8] {
    // SAFETY: `SockaddrIn` is `#[repr(C)]` without padding: its bytes are initialised.
    unsafe {
        slice::from_raw_parts(
            core::ptr::from_ref(sin).cast::<u8>(),
            size_of::<SockaddrIn>(),
        )
    }
}

/// `sotoinpcb(so)` of a raw socket the C knows to be attached.
fn inpcb_of(so: &Socket) -> &'static Inpcb {
    match sotoinpcb(so) {
        Some(inp) => inp,
        None => panic(format_args!("raw socket {:p}: no inpcb", so)),
    }
}

/// `rip_init`: initialize raw connection block q.
pub fn rip_init() {
    in_pcbinit(&RAWCBTABLE, 1);
}

/// `rip_input`: hands a copy of datagram `*mp` to every raw socket that matches it; without
/// one, an ICMP message is dropped and any other protocol is answered with a protocol
/// unreachable.
pub fn rip_input(
    mp: &mut Option<&'static Mbuf>,
    _offp: &mut i32,
    _proto: i32,
    af: i32,
    _ns: Option<&Netstack>,
) -> i32 {
    let Some(m) = mp.take() else {
        return IPPROTO_DONE;
    };
    let ip = mtod_ip(m);
    let iter = InpcbIterator::new();
    let mut inp: Option<&'static Inpcb> = None;
    let mut last: Option<&'static Inpcb> = None;

    kassert!(af == i32::from(AF_INET));

    let ripsrc = SockaddrIn {
        sin_family: AF_INET,
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_addr: ip.ip_src,
        ..SockaddrIn::default()
    };

    let mut key = ip.ip_dst;
    if m.m_pkthdr().pf.flags.get() & PF_TAG_DIVERTED != 0 {
        let divert = pf_find_divert(m);
        kassert!(divert.is_some());
        if let Some(divert) = divert {
            match divert.type_ {
                PF_DIVERT_TO => key = divert.addr.v4(),
                PF_DIVERT_REPLY => {}
                t => crate::kern::subr_prf::panic(format_args!(
                    "rip_input: unknown divert type {t}, mbuf {m:p}"
                )),
            }
        }
    }
    mtx_enter(&RAWCBTABLE.inpt_mtx);
    // SAFETY: the table mutex is held around every call; `iter` lives on this frame until the
    // walk ends with `None`.
    while let Some(i) = unsafe { in_pcb_iterator(&RAWCBTABLE, inp, &iter) } {
        inp = Some(i);
        kassert!(!i.has_flags(INP_IPV6));

        // Packet must not be inserted after disconnected wakeup call. To avoid race, check
        // again when holding receive buffer mutex.
        if i.socket().so_rcv.has_state(SS_CANTRCVMORE) {
            continue;
        }
        if rtable_l2(i.inp_rtableid.get()) != rtable_l2(m.m_pkthdr().ph_rtableid.get()) {
            continue;
        }

        let proto = i.inp_ip.get().ip_p;
        if proto != 0 && proto != ip.ip_p {
            continue;
        }
        let laddr = i.inp_laddr.get().s_addr;
        if laddr != 0 && laddr != key.s_addr {
            continue;
        }
        let faddr = i.inp_faddr.get().s_addr;
        if faddr != 0 && faddr != ip.ip_src.s_addr {
            continue;
        }

        if let Some(l) = last {
            mtx_leave(&RAWCBTABLE.inpt_mtx);

            if let Some(n) = m_copym(m, 0, M_COPYALL, M_DONTWAIT) {
                rip_sbappend(l, n, &ip, &ripsrc);
            }
            in_pcbunref(Some(l));

            mtx_enter(&RAWCBTABLE.inpt_mtx);
        }
        last = in_pcbref(Some(i));
    }
    mtx_leave(&RAWCBTABLE.inpt_mtx);

    let Some(last) = last else {
        if i32::from(ip.ip_p) == IPPROTO_ICMP {
            m_freem(m);
        } else {
            icmp_error(m, ICMP_UNREACH, ICMP_UNREACH_PROTOCOL, 0, 0);
        }
        ipstat_inc(IpstatCounters::IpsNoproto);
        ipstat_dec(IpstatCounters::IpsDelivered);

        return IPPROTO_DONE;
    };

    rip_sbappend(last, m, &ip, &ripsrc);
    in_pcbunref(Some(last));

    IPPROTO_DONE
}

/// `rip_sbappend`: queues datagram `m` from `ripsrc`, with the control messages its socket
/// asked for, on the receive buffer of `inp`'s socket.
pub fn rip_sbappend(inp: &Inpcb, m: &'static Mbuf, ip: &Ip, ripsrc: &SockaddrIn) {
    let so = inp.socket();
    let mut opts = None;
    let mut ret = false;

    if inp.has_flags(INP_CONTROLOPTS) || so.has_options(SO_TIMESTAMP) {
        ip_savecontrol(inp, &mut opts, ip, m);
    }

    mtx_enter(&so.so_rcv.sb_mtx);
    if !so.so_rcv.has_state(SS_CANTRCVMORE) {
        ret = sbappendaddr(&so.so_rcv, sin_bytes(ripsrc), Some(m), opts);
    }
    mtx_leave(&so.so_rcv.sb_mtx);

    if !ret {
        m_freem(m);
        m_freem(opts);
        ipstat_inc(IpstatCounters::IpsNoproto);
    } else {
        sorwakeup(so);
    }
}

/// `rip_output`: generates an IP header and passes the packet to `ip_output`; tacks on the
/// options the user may have set up with control calls.
pub fn rip_output(
    m: &'static Mbuf,
    so: &'static Socket,
    dst: &mut SockaddrIn,
    _control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let inp = inpcb_of(so);
    let mut flags = IP_ALLOWBROADCAST;
    let mut m = m;

    // If the user handed us a complete IP packet, use it. Otherwise, allocate an mbuf for a
    // header and fill it in.
    let mut ip = if !inp.has_flags(INP_HDRINCL) {
        if m.m_pkthdr().len.get() as usize + size_of::<Ip>() > IP_MAXPACKET {
            m_freem(m);
            return Err(Errno::EMSGSIZE);
        }
        let Some(n) = m_prepend(m, size_of::<Ip>() as i32, M_DONTWAIT) else {
            return Err(Errno::ENOBUFS);
        };
        m = n;
        let proto = inp.inp_ip.get();
        let ip = Ip {
            ip_tos: proto.ip_tos,
            ip_off: htons(0),
            ip_p: proto.ip_p,
            ip_len: htons(m.m_pkthdr().len.get() as u16),
            ip_src: InAddr { s_addr: INADDR_ANY },
            ip_dst: dst.sin_addr,
            ip_ttl: if proto.ip_ttl != 0 {
                proto.ip_ttl
            } else {
                MAXTTL
            },
            ..mtod_ip(m)
        };
        mtod_ip_store(m, &ip);
        ip
    } else {
        if m.m_pkthdr().len.get() as usize > IP_MAXPACKET {
            m_freem(m);
            return Err(Errno::EMSGSIZE);
        }

        let Some(n) = rip_chkhdr(m, inp.inp_options.get()) else {
            return Err(Errno::EINVAL);
        };
        m = n;

        let mut ip = mtod_ip(m);
        if ip.ip_id == 0 {
            ip.ip_id = htons(ip_randomid());
            mtod_ip_store(m, &ip);
        }
        dst.sin_addr = ip.ip_dst;

        // XXX prevent ip_output from overwriting header fields
        flags |= IP_RAWOUTPUT;
        ipstat_inc(IpstatCounters::IpsRawout);
        ip
    };

    if ip.ip_src.s_addr == INADDR_ANY {
        in_pcbselsrc(&mut ip.ip_src, dst, inp)?;
        mtod_ip_store(m, &ip);
    }

    // INET6: a thought: even though raw IP shouldn't be able to set IPv6 multicast options,
    // if it does, the last parameter to ip_output should be guarded against v6/v4 problems.

    // force routing table
    m.m_pkthdr().ph_rtableid.set(inp.inp_rtableid.get());

    if inp.socket().has_state(SS_ISCONNECTED) && i32::from(mtod_ip(m).ip_p) != IPPROTO_ICMP {
        pf_mbuf_link_inpcb(m, Some(inp));
    }

    ip_output(
        m,
        inp.inp_options.get(),
        Some(&inp.inp_route),
        flags,
        inp.moptions(),
        Some(&inp.inp_seclevel.get()),
        0,
    )
}

/// `rip_chkhdr`: checks the IP header (and its options) a user handed over with
/// `IP_HDRINCL`; the packet, pulled up, or `None` when it was freed.
pub fn rip_chkhdr(m: &'static Mbuf, options: Option<&'static Mbuf>) -> Option<&'static Mbuf> {
    if (m.m_pkthdr().len.get() as usize) < size_of::<Ip>() {
        m_freem(m);
        return None;
    }

    let m = m_pullup(m, size_of::<Ip>() as i32)?;

    let ip = mtod_ip(m);
    let hlen = usize::from(ip.ip_hl()) << 2;

    // Don't allow packet length sizes that will crash.
    if hlen < size_of::<Ip>()
        || usize::from(ntohs(ip.ip_len)) < hlen
        || i32::from(ntohs(ip.ip_len)) != m.m_pkthdr().len.get()
    {
        m_freem(m);
        return None;
    }
    let m = m_pullup(m, hlen as i32)?;

    let ip = mtod_ip(m);

    if ip.ip_v() != IPVERSION {
        m_freem(m);
        return None;
    }

    // Don't allow both user specified and setsockopt options. If options are present verify
    // them.
    if hlen != size_of::<Ip>() {
        if options.is_some() {
            m_freem(m);
            return None;
        }
        // SAFETY: `m_pullup` made the whole header (`hlen` bytes) contiguous in `m`.
        let opts = unsafe {
            slice::from_raw_parts(mtod::<u8>(m).add(size_of::<Ip>()), hlen - size_of::<Ip>())
        };
        let mut cp = 0usize;
        let mut cnt = opts.len() as i32;
        while cnt > 0 {
            let opt = opts[cp + IPOPT_OPTVAL];
            if opt == IPOPT_EOL {
                break;
            }
            let optlen = if opt == IPOPT_NOP {
                1
            } else {
                if cnt < (IPOPT_OLEN + 1) as i32 {
                    m_freem(m);
                    return None;
                }
                let optlen = i32::from(opts[cp + IPOPT_OLEN]);
                if optlen < (IPOPT_OLEN + 1) as i32 || optlen > cnt {
                    m_freem(m);
                    return None;
                }
                optlen
            };
            cnt -= optlen;
            cp += optlen as usize;
        }
    }

    Some(m)
}

/// `rip_ctloutput`: raw IP socket option processing: `IP_HDRINCL`, the multicast routing
/// options, then `ip_ctloutput`.
pub fn rip_ctloutput(
    op: i32,
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let inp = inpcb_of(so);

    if level != IPPROTO_IP {
        return Err(Errno::EINVAL);
    }

    match optname {
        IP_HDRINCL => {
            if op == PRCO_SETOPT {
                match m.filter(|m| m.m_len().get() as usize >= size_of::<i32>()) {
                    None => return Err(Errno::EINVAL),
                    // SAFETY: the mbuf holds at least an `int` (checked).
                    Some(m) if unsafe { mtod::<i32>(m).read_unaligned() } != 0 => {
                        inp.set_flags(INP_HDRINCL);
                    }
                    Some(_) => inp.clear_flags(INP_HDRINCL),
                }
            } else if let Some(m) = m {
                m.m_len().set(size_of::<i32>() as u32);
                // SAFETY: an option mbuf holds `MLEN` bytes, more than an `int`.
                unsafe { mtod::<i32>(m).write_unaligned(inp.inp_flags.get() & INP_HDRINCL) };
            }
            Ok(())
        }

        MRT_INIT | MRT_DONE | MRT_ADD_VIF | MRT_DEL_VIF | MRT_ADD_MFC | MRT_DEL_MFC
        | MRT_VERSION | MRT_ASSERT | MRT_API_SUPPORT | MRT_API_CONFIG => {
            // MROUTING: ip_mrouter_set / ip_mrouter_get; not configured.
            Err(Errno::EOPNOTSUPP)
        }

        _ => ip_ctloutput(op, so, level, optname, m),
    }
}

/// `rip_attach`: a control block for a privileged raw socket of protocol `proto`.
pub fn rip_attach(so: &'static Socket, proto: i32, wait: i32) -> Result<(), Errno> {
    if !so.so_pcb.get().is_null() {
        panic(format_args!("rip_attach"));
    }
    if !so.has_state(SS_PRIV) {
        return Err(Errno::EACCES);
    }
    if !(0..IPPROTO_MAX).contains(&proto) {
        return Err(Errno::EPROTONOSUPPORT);
    }

    soreserve(so, RIP_SENDSPACE, RIP_RECVSPACE)?;
    in_pcballoc(so, &RAWCBTABLE, wait)?;
    let inp = inpcb_of(so);
    let mut ip = inp.inp_ip.get();
    ip.ip_p = proto as u8;
    inp.inp_ip.set(ip);
    Ok(())
}

/// `rip_detach`.
pub fn rip_detach(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);

    let Some(inp) = sotoinpcb(so) else {
        return Err(Errno::EINVAL);
    };

    // MROUTING: ip_mrouter_done(so); not configured.
    in_pcbdetach(inp);

    Ok(())
}

/// The `sockaddr_in` of an address mbuf, read out of it.
fn nam_sin(nam: &Mbuf) -> Result<SockaddrIn, Errno> {
    let sin = in_nam2sin(nam)?;
    // SAFETY: `in_nam2sin` checked the mbuf holds a whole `sockaddr_in`; read unaligned.
    Ok(unsafe { sin.read_unaligned() })
}

/// `rip_bind`: a local address of ours (or a broadcast or wildcard one) for `so`.
pub fn rip_bind(so: &'static Socket, nam: &'static Mbuf, _p: &Proc) -> Result<(), Errno> {
    let inp = inpcb_of(so);

    soassertlocked(so);

    let mut addr = nam_sin(nam)?;

    if !(so.has_options(SO_BINDANY)
        || addr.sin_addr.s_addr == INADDR_ANY
        || addr.sin_addr.s_addr == INADDR_BROADCAST
        || in_broadcast(addr.sin_addr, inp.inp_rtableid.get())
        // SAFETY: a local `sockaddr_in`, read for the call.
        || unsafe { ifa_ifwithaddr(sintosa(&mut addr), inp.inp_rtableid.get()) }.is_some())
    {
        return Err(Errno::EADDRNOTAVAIL);
    }

    mtx_enter(&RAWCBTABLE.inpt_mtx);
    inp.inp_laddr.set(addr.sin_addr);
    mtx_leave(&RAWCBTABLE.inpt_mtx);

    Ok(())
}

/// `rip_connect`: the foreign address of `so`.
pub fn rip_connect(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    let inp = inpcb_of(so);

    soassertlocked(so);

    let addr = nam_sin(nam)?;

    mtx_enter(&RAWCBTABLE.inpt_mtx);
    inp.inp_faddr.set(addr.sin_addr);
    mtx_leave(&RAWCBTABLE.inpt_mtx);
    soisconnected(so);

    Ok(())
}

/// `rip_disconnect`.
pub fn rip_disconnect(so: &'static Socket) -> Result<(), Errno> {
    let inp = inpcb_of(so);

    soassertlocked(so);

    if !so.has_state(SS_ISCONNECTED) {
        return Err(Errno::ENOTCONN);
    }

    soisdisconnected(so);
    mtx_enter(&RAWCBTABLE.inpt_mtx);
    inp.inp_faddr.set(InAddr { s_addr: INADDR_ANY });
    mtx_leave(&RAWCBTABLE.inpt_mtx);

    Ok(())
}

/// `rip_shutdown`: marks the connection as being incapable of further input.
pub fn rip_shutdown(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);
    socantsendmore(so);

    Ok(())
}

/// `rip_send`: ships a packet out to the connected address or `nam`; `rip_output` handles
/// any massaging necessary.
pub fn rip_send(
    so: &'static Socket,
    m: Option<&'static Mbuf>,
    nam: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let inp = inpcb_of(so);

    soassertlocked(so);

    let mut dst = SockaddrIn {
        sin_family: AF_INET,
        sin_len: size_of::<SockaddrIn>() as u8,
        ..SockaddrIn::default()
    };
    let error = 'out: {
        if so.has_state(SS_ISCONNECTED) {
            if nam.is_some() {
                break 'out Err(Errno::EISCONN);
            }
            dst.sin_addr = inp.inp_faddr.get();
        } else {
            let Some(nam) = nam else {
                break 'out Err(Errno::ENOTCONN);
            };
            match nam_sin(nam) {
                Ok(addr) => dst.sin_addr = addr.sin_addr,
                Err(e) => break 'out Err(e),
            }
        }
        // XXX Find an IPsec TDB
        // sosend always hands over a packet (a pkthdr mbuf, empty or not).
        let Some(m) = m else {
            break 'out Err(Errno::EINVAL);
        };
        let error = rip_output(m, so, &mut dst, None);
        m_freem(control);
        return error;
    };

    m_freem(control);
    m_freem(m);

    error
}

#[cfg(test)]
mod tests;
