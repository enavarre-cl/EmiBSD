/*	$OpenBSD: udp_usrreq.c,v 1.351 2026/07/17 18:51:29 bluhm Exp $	*/
/*	$NetBSD: udp_usrreq.c,v 1.28 1996/03/16 23:54:03 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1988, 1990, 1993
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

//! The User Datagram Protocol, per RFC 768, August, 1980: `netinet/udp_usrreq.c`.
//!
//! Upstream: sys/netinet/udp_usrreq.c @ 3ce1f3f79392
//!
//! `udp_input` checks a datagram's length and checksum and appends it, with the sender's
//! address, to the socket bound to its destination: the connected one
//! (`in_pcblookup`), else the listening one (`in_pcblookup_listen`); a broadcast or multicast
//! datagram goes to every matching socket. Without a socket the sender gets a port
//! unreachable. `udp_output` prepends the UDP and IP headers (choosing the source address
//! and, for an unbound socket, a port) and sends with `ip_output`; the checksum is left to
//! the interface or `in_proto_cksum_out`. `udp_ctlinput` passes ICMP errors on to the
//! sockets they concern.
//!
//! Locks used to protect data: \[a\] atomic.
//!
//! ## Deviations
//! - `udpcksum`, `udp_sendspace` and `udp_recvspace` are `AtomicI32`s, the type
//!   `sysctl_bounded_arr` takes (`u_int` for the spaces in C; the bounds keep them positive).
//!   `udpcounters` (`struct cpumem *`) is the static array of atomics [`UDPCOUNTERS`].
//! - The UDP and IP headers in the packet are read and written as copies, unaligned (mbuf
//!   data need not be aligned): `udp_input` keeps the address of the UDP header in the mbuf
//!   (it writes the checksum it computes there, and puts the original back before an ICMP
//!   error, as the C does through its pointer); `udp_output` writes the `struct udpiphdr`
//!   over the prepended space, keeping its `uh_sum` bytes, which the C does not set.
//! - `udp_sbappend` takes the UDP header and the IP or IPv6 header as copies, and the
//!   sender's address (`struct sockaddr *`) as its bytes; an `inp_upcall` gets the addresses
//!   of the copies. `udp_input`'s `srcsa` union is a [`SockaddrUnion`].
//! - `udp_ctlinput` is an `unsafe fn` (`PrCtlinputFn`): it reads the returned IP and UDP
//!   headers through the raw argument; its `notify` takes `Option<Errno>` (`in_pcb.rs`).
//! - `udp_sysctl`'s port bitmaps are copied through a byte buffer on the stack (the C's
//!   `malloc(M_SYSCTL)`), as `sysctl_struct` takes bytes.
//! - `INET6` is configured (feature `inet6`): the IPv6 paths of `udp_input`,
//!   `udp_sbappend` (`ip6_savecontrol`, `IPV6_RECVDSTPORT`), `udp_output` (`udp6_output`)
//!   and the user requests. `udb6table`, `udp6_usrreqs` and `udp6_ctlinput` compile
//!   always, as `netinet6` does (its `inet6sw` names them), like `route6_mpath`.
//!   `udp6_ctlinput` writes the scope-embedded final destination back through
//!   `ip6c_finaldst`, as the C does.
//! - Not configured, a comment at its site: `PIPEX`. `NPF` (`pf_inp_lookup`,
//!   `pf_inp_link`, `pf_mbuf_link_inpcb`), `NSTOEPLITZ` (the flow id) and `IPSEC` (M9c: UDP
//!   encapsulation of ESP, the SPD lookup, `IP_IPSECFLOWINFO` control messages) are
//!   configured.
//! - `SMALL_KERNEL` is not set: the sysctl handlers are compiled.

use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_sysctl::{SECURELEVEL, sysctl_bounded_arr, sysctl_rdstruct, sysctl_struct};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_adj, m_copydata, m_copym, m_freem, m_prepend, m_pullup};
use crate::kern::uipc_mbuf2::m_tag_find;
use crate::kern::uipc_socket::{sorwakeup, sowwakeup};
use crate::kern::uipc_socket2::{
    sbappendaddr, sbcreatecontrol, soassertlocked, soassertlocked_readonly, socantsendmore,
    soisconnected, soreserve,
};
use crate::machine::cpu::curproc;
use crate::net::if_var::Netstack;
use crate::net::pf::{pf_inp_link, pf_inp_lookup, pf_mbuf_link_inpcb};
use crate::net::rtable::rtable_l2;
#[cfg(feature = "inet6")]
use crate::netinet::icmp6::{ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_NOPORT};
#[cfg(feature = "inet6")]
use crate::netinet::in_::IPPROTO_IPV6;
use crate::netinet::in_::{
    INADDR_ANY, IP_IPSECFLOWINFO, IP_RECVDSTPORT, IP_SENDSRCADDR, IPPROTO_DONE, IPPROTO_ESP,
    IPPROTO_IP, IPPROTO_UDP, InAddr, SockaddrIn, in_control, in_nam2sin,
};
use crate::netinet::in_pcb::{
    BADDYNAMICPORTS, DP_MAPSIZE, INP_CONTROLOPTS, INP_IPSECFLOWINFO, INP_IPV6, INP_RECVDSTPORT,
    InpNotifyFn, Inpcb, InpcbIterator, Inpcbtable, ROOTONLYPORTS, in_flowid, in_pcb_iterator,
    in_pcb_iterator_abort, in_pcbaddrisavail, in_pcballoc, in_pcbbind, in_pcbconnect, in_pcbdetach,
    in_pcbdisconnect, in_pcbinit, in_pcblookup, in_pcblookup_listen, in_pcbnotifyall, in_pcbref,
    in_pcbrtchange, in_pcbselsrc, in_pcbsolock, in_pcbsounlock, in_pcbunref, in_pcbunset_laddr,
    in_peeraddr, in_sockaddr, sotoinpcb,
};
#[cfg(feature = "inet6")]
use crate::netinet::in_pcb::{IN6P_CONTROLOPTS, IN6P_RECVDSTPORT};
use crate::netinet::in4_cksum::in4_cksum;
use crate::netinet::ip::{IP_MAXPACKET, Ip};
use crate::netinet::ip_esp::{EspstatCounters, espstat_inc};
use crate::netinet::ip_icmp::{ICMP_UNREACH, ICMP_UNREACH_PORT, icmp_error};
use crate::netinet::ip_input::{INETCTLERRMAP, IP_DEFTTL, ip_mtudisc, ip_savecontrol};
use crate::netinet::ip_ipsp::{
    IPSEC_IN_USE, IPSP_DIRECTION_IN, SockaddrUnion, TdbIdent, gettdb, tdb_unref,
};
use crate::netinet::ip_output::ip_output;
use crate::netinet::ip_spd::ipsp_spd_lookup;
use crate::netinet::ip_var::{mtod_ip, mtod_ip_store};
use crate::netinet::ip6::{Ip6Hdr, ip6_exthdr_get};
use crate::netinet::ipsec_input::{ESP_ENABLE, ipsec_common_input, udpencap_ctlinput};
use crate::netinet::ipsec_output::{UDPENCAP_ENABLE, UDPENCAP_PORT};
use crate::netinet::udp::Udphdr;
use crate::netinet::udp_var::{
    UDPCTL_BADDYNAMIC, UDPCTL_CHECKSUM, UDPCTL_RECVSPACE, UDPCTL_ROOTONLY, UDPCTL_SENDSPACE,
    UDPCTL_STATS, UDPS_NCOUNTERS, Udpiphdr, Udpstat, UdpstatCounters, udpstat_inc,
};
use crate::netinet6::icmp6::icmp6_mtudisc_update;
use crate::netinet6::in6::{SA6_ANY, SockaddrIn6, in6_addr2scopeid, in6_control, satosin6_const};
use crate::netinet6::in6_pcb::{in6_pcblookup, in6_pcbnotify, in6_peeraddr, in6_sockaddr};
use crate::netinet6::in6_src::in6_embedscope;
use crate::netinet6::ip6_input::INET6CTLERRMAP;
use crate::netinet6::ip6protosw::Ip6ctlparam;
#[cfg(feature = "inet6")]
use crate::netinet6::{
    icmp6::icmp6_error,
    in6::{IN6ADDR_ANY, IPV6_RECVDSTPORT, in6_are_addr_equal, in6_is_addr_unspecified},
    in6_cksum::in6_cksum,
    in6_pcb::in6_pcblookup_listen,
    in6_proto::IP6_DEFHLIM,
    in6_src::in6_recoverscope,
    ip6_input::ip6_savecontrol,
    ip6_var::mtod_ip6,
    udp6_output::udp6_output,
};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{
    M_BCAST, M_COPYALL, M_DONTWAIT, M_MCAST, M_UDP_CSUM_IN_BAD, M_UDP_CSUM_IN_OK, M_UDP_CSUM_OUT,
    Mbuf, PACKET_TAG_IPSEC_IN_DONE, PF_TAG_DIVERTED, m_freemp, mtod,
};
use crate::sys::proc::Proc;
use crate::sys::protosw::{PRC_HOSTDEAD, PRC_MSGSIZE, PRC_NCMDS, PrUsrreqs, prc_is_redirect};
#[cfg(feature = "inet6")]
use crate::sys::socket::PF_INET6;
use crate::sys::socket::{
    AF_INET, AF_INET6, Cmsghdr, SO_BROADCAST, SO_REUSEADDR, SO_REUSEPORT, SO_TIMESTAMP, Sockaddr,
    cmsg_align, cmsg_data, cmsg_len,
};
use crate::sys::socketvar::{SB_MAX, SS_CANTRCVMORE, SS_ISCONNECTED, Socket};
use crate::sys::sysctl::SysctlBoundedArgs;
use crate::sys::systm::{net_lock, net_lock_shared, net_unlock, net_unlock_shared};

/// `UDB_INITIAL_HASH_SIZE`.
const UDB_INITIAL_HASH_SIZE: i32 = 128;

/// `udp_usrreqs`.
pub static UDP_USRREQS: PrUsrreqs = PrUsrreqs {
    pru_attach: Some(udp_attach),
    pru_detach: Some(udp_detach),
    pru_bind: Some(udp_bind),
    pru_connect: Some(udp_connect),
    pru_disconnect: Some(udp_disconnect),
    pru_shutdown: Some(udp_shutdown),
    pru_send: Some(udp_send),
    pru_control: Some(in_control),
    pru_sockaddr: Some(in_sockaddr),
    pru_peeraddr: Some(in_peeraddr),
    pru_flowid: Some(in_flowid),
    ..PrUsrreqs::NONE
};

/// `udp6_usrreqs` (`INET6`; compiled always, as `netinet6` is, for `inet6sw`).
pub static UDP6_USRREQS: PrUsrreqs = PrUsrreqs {
    pru_attach: Some(udp_attach),
    pru_detach: Some(udp_detach),
    pru_bind: Some(udp_bind),
    pru_connect: Some(udp_connect),
    pru_disconnect: Some(udp_disconnect),
    pru_shutdown: Some(udp_shutdown),
    pru_send: Some(udp_send),
    pru_control: Some(in6_control),
    pru_sockaddr: Some(in6_sockaddr),
    pru_peeraddr: Some(in6_peeraddr),
    pru_flowid: Some(in_flowid),
    ..PrUsrreqs::NONE
};

/// \[a\] `udpcksum`.
pub static UDPCKSUM: AtomicI32 = AtomicI32::new(1);
/// \[a\] `udp_sendspace`: really max datagram size.
pub static UDP_SENDSPACE: AtomicI32 = AtomicI32::new(9216);
/// \[a\] `udp_recvspace`: 40 1K datagrams.
pub static UDP_RECVSPACE: AtomicI32 = AtomicI32::new(40 * (1024 + size_of::<SockaddrIn>() as i32));

/// `udpctl_vars[]`.
static UDPCTL_VARS: [SysctlBoundedArgs; 3] = [
    SysctlBoundedArgs::new(UDPCTL_CHECKSUM, &UDPCKSUM, 0, 1),
    SysctlBoundedArgs::new(UDPCTL_RECVSPACE, &UDP_RECVSPACE, 0, SB_MAX as i32),
    SysctlBoundedArgs::new(UDPCTL_SENDSPACE, &UDP_SENDSPACE, 0, SB_MAX as i32),
];

/// `udbtable`.
pub static UDBTABLE: Inpcbtable = Inpcbtable::new();
/// `udb6table` (`INET6`; compiled always, for `udp6_ctlinput`).
pub static UDB6TABLE: Inpcbtable = Inpcbtable::new();

/// `udpcounters`.
pub static UDPCOUNTERS: [AtomicU64; UDPS_NCOUNTERS] = [const { AtomicU64::new(0) }; UDPS_NCOUNTERS];

/// The bytes of a `sockaddr_in6`.
#[cfg(feature = "inet6")]
fn sin6_bytes(sin6: &SockaddrIn6) -> &[u8] {
    // SAFETY: `SockaddrIn6` is `#[repr(C)]` without padding: its bytes are initialised.
    unsafe {
        core::slice::from_raw_parts(ptr::from_ref(sin6).cast::<u8>(), size_of::<SockaddrIn6>())
    }
}

/// `curproc`, which the socket requests run as.
fn curproc_or_panic(func: &str) -> &'static Proc {
    match curproc() {
        Some(p) => p,
        None => panic(format_args!("{}: no curproc", func)),
    }
}

/// `udp_init`.
pub fn udp_init() {
    // udpcounters = counters_alloc(udps_ncounters): a static array of atomics.
    in_pcbinit(&UDBTABLE, UDB_INITIAL_HASH_SIZE);
    #[cfg(feature = "inet6")]
    in_pcbinit(&UDB6TABLE, UDB_INITIAL_HASH_SIZE);
}

/// The UDP header at `uh`, read unaligned.
///
/// # Safety
///
/// `uh` points at the eight bytes of a UDP header inside a live mbuf.
unsafe fn uh_read(uh: *const u8) -> Udphdr {
    // SAFETY: the caller's contract.
    unsafe { uh.cast::<Udphdr>().read_unaligned() }
}

/// Writes `uh_sum` of the UDP header at `uh`.
///
/// # Safety
///
/// As for [`uh_read`].
unsafe fn uh_set_sum(uh: *mut u8, sum: u16) {
    // SAFETY: the caller's contract.
    unsafe {
        uh.add(offset_of!(Udphdr, uh_sum))
            .cast::<u16>()
            .write_unaligned(sum)
    };
}

/// `udp_input`: a datagram from IP: to the socket(s) bound to its destination, or a port
/// unreachable.
pub fn udp_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    _proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let iphlen = *offp;
    let mut inp: Option<&'static Inpcb> = None;
    let mut ipsecflowinfo: u32 = 0;
    let udpencap_port_local = UDPENCAP_PORT.load(Ordering::Relaxed);

    udpstat_inc(UdpstatCounters::UdpsIpackets);

    let Some(uh) = ip6_exthdr_get(mp, iphlen, size_of::<Udphdr>() as i32) else {
        udpstat_inc(UdpstatCounters::UdpsHdrops);
        return IPPROTO_DONE;
    };
    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };
    // SAFETY: `ip6_exthdr_get` made the header contiguous at `uh`, inside `m`; the reads and
    // writes through it below happen while `m` is held.
    let hdr = unsafe { uh_read(uh) };

    'bad: {
        // Check for illegal destination port 0
        if hdr.uh_dport == 0 {
            udpstat_inc(UdpstatCounters::UdpsNoport);
            break 'bad;
        }

        // Make mbuf data length reflect UDP length. If not enough data to reflect UDP length,
        // drop. `ip` (a copy of the IPv4 header, also the C's `save_ip`, kept in case we want
        // to restore it for sending an ICMP error message in response) or `ip6` is set.
        let ulen = i32::from(u16::from_be(hdr.uh_ulen));
        let (ip, ip6, len): (Option<Ip>, Option<Ip6Hdr>, i32) = match af {
            x if x == i32::from(AF_INET) => {
                let plen = m.m_pkthdr().len.get() - iphlen;
                if plen != ulen {
                    if ulen > plen || ulen < size_of::<Udphdr>() as i32 {
                        udpstat_inc(UdpstatCounters::UdpsBadlen);
                        break 'bad;
                    }
                    m_adj(m, ulen - plen);
                }
                (Some(mtod_ip(m)), None, ulen)
            }
            #[cfg(feature = "inet6")]
            x if x == i32::from(AF_INET6) => {
                let plen = m.m_pkthdr().len.get() - iphlen;
                // jumbograms
                let len = if ulen == 0 && plen > 0xffff {
                    plen
                } else {
                    ulen
                };
                if len != plen {
                    udpstat_inc(UdpstatCounters::UdpsBadlen);
                    break 'bad;
                }
                (None, Some(mtod_ip6(m)), len)
            }
            _ => crate::net::if_::unhandled_af(af),
        };

        // Checksum extended UDP header and data. from W.R.Stevens: check incoming udp cksums
        // even if udpcksum is not set.
        let savesum = hdr.uh_sum;
        if hdr.uh_sum == 0 {
            udpstat_inc(UdpstatCounters::UdpsNosum);
            // In IPv6, the UDP checksum is ALWAYS used.
            #[cfg(feature = "inet6")]
            if ip6.is_some() {
                break 'bad;
            }
        } else if m.m_pkthdr().csum_flags.get() & M_UDP_CSUM_IN_OK == 0 {
            if m.m_pkthdr().csum_flags.get() & M_UDP_CSUM_IN_BAD != 0 {
                udpstat_inc(UdpstatCounters::UdpsBadsum);
                break 'bad;
            }
            udpstat_inc(UdpstatCounters::UdpsInswcsum);

            let sum = match (ip, ip6) {
                (Some(_), _) => in4_cksum(m, IPPROTO_UDP as u8, iphlen, len),
                #[cfg(feature = "inet6")]
                (None, Some(_)) => in6_cksum(m, IPPROTO_UDP as u8, iphlen as u32, len as u32),
                _ => hdr.uh_sum,
            };
            // SAFETY: as for `hdr`.
            unsafe { uh_set_sum(uh, sum) };
            if sum != 0 {
                udpstat_inc(UdpstatCounters::UdpsBadsum);
                break 'bad;
            }
        }
        m.m_pkthdr()
            .csum_flags
            .set(m.m_pkthdr().csum_flags.get() & !M_UDP_CSUM_OUT);

        if UDPENCAP_ENABLE.load(Ordering::Relaxed) != 0
            && udpencap_port_local != 0
            && ESP_ENABLE.load(Ordering::Relaxed) != 0
            && m.m_pkthdr().pf.flags.get() & PF_TAG_DIVERTED == 0
            && hdr.uh_dport == (udpencap_port_local as u16).to_be()
        {
            let mut skip = iphlen + size_of::<Udphdr>() as i32;

            if m.m_pkthdr().len.get() - skip < size_of::<u32>() as i32 {
                // packet too short
                m_freemp(mp);
                return IPPROTO_DONE;
            }
            let mut spi = [0u8; 4];
            m_copydata(m, skip, &mut spi);
            // decapsulate if the SPI is not zero, otherwise pass to userland
            if spi != [0; 4] {
                let Some(m) = m_pullup(m, skip) else {
                    *mp = None;
                    udpstat_inc(UdpstatCounters::UdpsHdrops);
                    return IPPROTO_DONE;
                };
                *mp = Some(m);

                // remove the UDP header
                // SAFETY: `m_pullup` made the first `skip` bytes contiguous; the IP header
                // moves up over the UDP header (an overlapping copy).
                unsafe {
                    let p = mtod::<u8>(m);
                    ptr::copy(p, p.add(size_of::<Udphdr>()), iphlen as usize);
                }
                m_adj(m, size_of::<Udphdr>() as i32);
                skip -= size_of::<Udphdr>() as i32;

                espstat_inc(EspstatCounters::EspsUdpencin);
                let protoff = if af == i32::from(AF_INET) {
                    offset_of!(Ip, ip_p)
                } else {
                    offset_of!(Ip6Hdr, ip6_nxt)
                } as i32;
                return ipsec_common_input(mp, skip, protoff, af, IPPROTO_ESP, true, ns);
            }
        }

        // srcsa: the sender's address. dstsa: the C fills it and does not read it further.
        let mut srcsa = SockaddrUnion::new();
        if let Some(ip) = ip {
            srcsa.set_sin(&SockaddrIn {
                sin_len: size_of::<SockaddrIn>() as u8,
                sin_family: AF_INET,
                sin_port: hdr.uh_sport,
                sin_addr: ip.ip_src,
                ..SockaddrIn::default()
            });
        }
        #[cfg(feature = "inet6")]
        if let Some(ip6) = ip6 {
            let mut sin6 = SockaddrIn6 {
                sin6_port: hdr.uh_sport,
                ..SockaddrIn6::with_addr(IN6ADDR_ANY)
            };
            // XXX inbound flowinfo (#if 0 in the C)
            // KAME hack: recover scopeid
            in6_recoverscope(&mut sin6, &ip6.ip6_src);
            srcsa.as_bytes_mut().copy_from_slice(sin6_bytes(&sin6));
        }

        if m.m_flags().get() & (M_BCAST | M_MCAST) != 0 {
            let iter = InpcbIterator::new();
            let mut last: Option<&'static Inpcb> = None;

            // Deliver a multicast or broadcast datagram to *all* sockets for which the local
            // and remote addresses and ports match those of the incoming datagram. This
            // allows more than one process to receive multi/broadcasts on the same port.
            // (This really ought to be done for unicast datagrams as well, but that would
            // cause problems with existing applications that open both address-specific
            // sockets and a wildcard socket listening to the same port -- they would end up
            // receiving duplicates of every unicast datagram. Those applications open the
            // multiple sockets to overcome an inadequacy of the UDP socket interface, but for
            // backwards compatibility we avoid the problem here rather than fixing the
            // interface. Maybe 4.5BSD will remedy this?)

            #[cfg(feature = "inet6")]
            let table = if ip6.is_some() { &UDB6TABLE } else { &UDBTABLE };
            #[cfg(not(feature = "inet6"))]
            let table = &UDBTABLE;

            mtx_enter(&table.inpt_mtx);
            // SAFETY: the table mutex is held around every call; `iter` lives on this frame
            // until the walk ends with `None` or is aborted.
            while let Some(i) = unsafe { in_pcb_iterator(table, inp, &iter) } {
                inp = Some(i);
                if ip6.is_some() {
                    kassert!(i.has_flags(INP_IPV6));
                } else {
                    kassert!(!i.has_flags(INP_IPV6));
                }

                let so = i.socket();
                if so.so_rcv.has_state(SS_CANTRCVMORE) {
                    continue;
                }
                if rtable_l2(i.inp_rtableid.get()) != rtable_l2(m.m_pkthdr().ph_rtableid.get()) {
                    continue;
                }
                if i.inp_lport.get() != hdr.uh_dport {
                    continue;
                }
                #[cfg(feature = "inet6")]
                if let Some(ip6) = ip6 {
                    let minhlim = i.inp_ip6_minhlim().get();
                    if minhlim != 0 && minhlim > ip6.ip6_hlim {
                        continue;
                    }
                    let laddr6 = i.inp_laddr6.get();
                    if !in6_is_addr_unspecified(&laddr6)
                        && !in6_are_addr_equal(&laddr6, &ip6.ip6_dst)
                    {
                        continue;
                    }
                }
                if let Some(ip) = ip {
                    let minttl = i.inp_ip_minttl.get();
                    if minttl != 0 && minttl > ip.ip_ttl {
                        continue;
                    }

                    let laddr = i.inp_laddr.get().s_addr;
                    if laddr != INADDR_ANY && laddr != ip.ip_dst.s_addr {
                        continue;
                    }
                }
                #[cfg(feature = "inet6")]
                if let Some(ip6) = ip6 {
                    let faddr6 = i.inp_faddr6.get();
                    if !in6_is_addr_unspecified(&faddr6)
                        && (!in6_are_addr_equal(&faddr6, &ip6.ip6_src)
                            || i.inp_fport.get() != hdr.uh_sport)
                    {
                        continue;
                    }
                }
                if let Some(ip) = ip {
                    let faddr = i.inp_faddr.get().s_addr;
                    if faddr != INADDR_ANY
                        && (faddr != ip.ip_src.s_addr || i.inp_fport.get() != hdr.uh_sport)
                    {
                        continue;
                    }
                }

                if let Some(l) = last {
                    mtx_leave(&table.inpt_mtx);

                    if let Some(n) = m_copym(m, 0, M_COPYALL, M_DONTWAIT) {
                        udp_sbappend(
                            l,
                            n,
                            ip.as_ref(),
                            ip6.as_ref(),
                            iphlen,
                            &hdr,
                            srcsa.sa_bytes(),
                            0,
                            ns,
                        );
                    }
                    in_pcbunref(Some(l));

                    mtx_enter(&table.inpt_mtx);
                }
                last = in_pcbref(Some(i));

                // Don't look for additional matches if this one does not have either the
                // SO_REUSEPORT or SO_REUSEADDR socket options set. This heuristic avoids
                // searching through all pcbs in the common case of a non-shared port. It
                // assumes that an application will never clear these options after setting
                // them.
                if !so.has_options(SO_REUSEPORT | SO_REUSEADDR) {
                    // SAFETY: the mutex is held and `iter` belongs to this walk.
                    unsafe { in_pcb_iterator_abort(table, inp, &iter) };
                    break;
                }
            }
            mtx_leave(&table.inpt_mtx);

            let Some(last) = last else {
                // No matching pcb found; discard datagram. (No need to send an ICMP Port
                // Unreachable for a broadcast or multicast datagram.)
                udpstat_inc(UdpstatCounters::UdpsNoportbcast);
                m_freem(m);
                *mp = None;
                return IPPROTO_DONE;
            };

            udp_sbappend(
                last,
                m,
                ip.as_ref(),
                ip6.as_ref(),
                iphlen,
                &hdr,
                srcsa.sa_bytes(),
                0,
                ns,
            );
            in_pcbunref(Some(last));

            *mp = None;
            return IPPROTO_DONE;
        }
        // Locate pcb for datagram.
        inp = pf_inp_lookup(m);
        if inp.is_none() {
            #[cfg(feature = "inet6")]
            if let Some(ip6) = ip6 {
                inp = in6_pcblookup(
                    &UDB6TABLE,
                    &ip6.ip6_src,
                    hdr.uh_sport,
                    &ip6.ip6_dst,
                    hdr.uh_dport,
                    m.m_pkthdr().ph_rtableid.get(),
                );
            }
            if let Some(ip) = ip {
                inp = in_pcblookup(
                    &UDBTABLE,
                    ip.ip_src,
                    hdr.uh_sport,
                    ip.ip_dst,
                    hdr.uh_dport,
                    m.m_pkthdr().ph_rtableid.get(),
                );
            }
        }
        if inp.is_none() {
            udpstat_inc(UdpstatCounters::UdpsPcbhashmiss);
            #[cfg(feature = "inet6")]
            if let Some(ip6) = ip6 {
                inp = in6_pcblookup_listen(
                    &UDB6TABLE,
                    &ip6.ip6_dst,
                    hdr.uh_dport,
                    Some(m),
                    m.m_pkthdr().ph_rtableid.get(),
                );
            }
            if let Some(ip) = ip {
                inp = in_pcblookup_listen(
                    &UDBTABLE,
                    ip.ip_dst,
                    hdr.uh_dport,
                    Some(m),
                    m.m_pkthdr().ph_rtableid.get(),
                );
            }
        }

        if IPSEC_IN_USE.load(Ordering::Relaxed) != 0 {
            let tdb = match m_tag_find(m, PACKET_TAG_IPSEC_IN_DONE, None) {
                Some(mtag) => {
                    // SAFETY: `IPSEC_IN_DONE` tags carry a `struct tdb_ident`.
                    let tdbi = unsafe { TdbIdent::read(mtag.data()) };
                    gettdb(tdbi.rdomain, tdbi.spi, &tdbi.dst, tdbi.proto)
                }
                None => None,
            };
            let seclevel = inp.map(|i| i.inp_seclevel.get());
            let error = ipsp_spd_lookup(
                m,
                af,
                iphlen,
                IPSP_DIRECTION_IN,
                tdb,
                seclevel.as_ref(),
                None,
                None,
            );
            if error.is_err() {
                udpstat_inc(UdpstatCounters::UdpsNosec);
                tdb_unref(tdb);
                break 'bad;
            }
            // create ipsec options, id is not modified after creation
            if let Some(ids) = tdb.and_then(|t| t.tdb_ids.get()) {
                ipsecflowinfo = ids.id_flow.get();
            }
            tdb_unref(tdb);
        }

        let Some(i) = inp else {
            udpstat_inc(UdpstatCounters::UdpsNoport);
            if m.m_flags().get() & (M_BCAST | M_MCAST) != 0 {
                udpstat_inc(UdpstatCounters::UdpsNoportbcast);
                break 'bad;
            }
            #[cfg(feature = "inet6")]
            if ip6.is_some() {
                // SAFETY: as for `hdr`.
                unsafe { uh_set_sum(uh, savesum) };
                icmp6_error(m, ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_NOPORT, 0);
                *mp = None;
                return IPPROTO_DONE;
            }
            if let Some(save_ip) = ip {
                mtod_ip_store(m, &save_ip);
            }
            // SAFETY: as for `hdr`; `mtod_ip_store` rewrote only the IP header.
            unsafe { uh_set_sum(uh, savesum) };
            icmp_error(m, ICMP_UNREACH, ICMP_UNREACH_PORT, 0, 0);
            *mp = None;
            return IPPROTO_DONE;
        };

        soassertlocked_readonly(i.socket());

        #[cfg(feature = "inet6")]
        if let Some(ip6) = ip6 {
            let minhlim = i.inp_ip6_minhlim().get();
            if minhlim != 0 && minhlim > ip6.ip6_hlim {
                break 'bad;
            }
        }
        if let Some(ip) = ip {
            let minttl = i.inp_ip_minttl.get();
            if minttl != 0 && minttl > ip.ip_ttl {
                break 'bad;
            }
        }

        if i.socket().has_state(SS_ISCONNECTED) {
            pf_inp_link(m, Some(i));
        }
        // PIPEX: pipex_l2tp_lookup_session and pipex_l2tp_input; not configured.

        udp_sbappend(
            i,
            m,
            ip.as_ref(),
            ip6.as_ref(),
            iphlen,
            &hdr,
            srcsa.sa_bytes(),
            ipsecflowinfo,
            ns,
        );
        in_pcbunref(inp);
        *mp = None;
        return IPPROTO_DONE;
    }
    // bad:
    m_freem(m);
    *mp = None;
    in_pcbunref(inp);
    IPPROTO_DONE
}

/// Appends control message `n` at the end of the chain `opts` (the C's walk to the last
/// `m_next`).
fn opts_append(opts: &mut Option<&'static Mbuf>, n: Option<&'static Mbuf>) {
    match *opts {
        None => *opts = n,
        Some(mut t) => {
            while let Some(next) = t.m_next().get() {
                t = next;
            }
            t.m_next().set(n);
        }
    }
}

/// `udp_sbappend`: appends datagram `m` from `srcaddr` (the bytes of a socket address; its
/// IP header `ip` or IPv6 header `ip6`, `hlen` bytes, and UDP header `uh` still in front) to
/// the receive buffer of `inp`'s socket, with the control messages the socket asked for.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn udp_sbappend(
    inp: &Inpcb,
    m: &'static Mbuf,
    ip: Option<&Ip>,
    ip6: Option<&Ip6Hdr>,
    hlen: i32,
    uh: &Udphdr,
    srcaddr: &[u8],
    ipsecflowinfo: u32,
    ns: Option<&Netstack>,
) {
    let so = inp.socket();
    let mut opts: Option<&'static Mbuf> = None;
    let mut m = m;

    let hlen = hlen + size_of::<Udphdr>() as i32;

    if let Some(upcall) = inp.inp_upcall.get() {
        let ipp = ip.map_or(ptr::null(), ptr::from_ref);
        let ip6p = ip6.map_or(ptr::null(), |h| ptr::from_ref(h).cast::<c_void>());
        let uhc = *uh;
        // SAFETY: the argument is the one installed with the upcall, `ipp`/`ip6p` are null or
        // copies of the packet's IP header, and `uhc` is a copy of the UDP header; all
        // outlive the call.
        let Some(n) = (unsafe {
            upcall(
                inp.inp_upcall_arg.get(),
                m,
                ipp,
                ip6p,
                ptr::from_ref(&uhc).cast::<c_void>(),
                hlen,
                ns,
            )
        }) else {
            return;
        };
        m = n;
    }

    #[cfg(feature = "inet6")]
    if ip6.is_some() && (inp.has_flags(IN6P_CONTROLOPTS) || so.has_options(SO_TIMESTAMP)) {
        ip6_savecontrol(inp, m, &mut opts);
    }
    if let Some(ip) = ip
        && (inp.has_flags(INP_CONTROLOPTS) || so.has_options(SO_TIMESTAMP))
    {
        ip_savecontrol(inp, &mut opts, ip, m);
    }
    #[cfg(feature = "inet6")]
    if ip6.is_some() && inp.has_flags(IN6P_RECVDSTPORT) {
        let n = sbcreatecontrol(&uh.uh_dport.to_ne_bytes(), IPV6_RECVDSTPORT, IPPROTO_IPV6);
        opts_append(&mut opts, n);
    }
    if ip.is_some() && inp.has_flags(INP_RECVDSTPORT) {
        let n = sbcreatecontrol(&uh.uh_dport.to_ne_bytes(), IP_RECVDSTPORT, IPPROTO_IP);
        opts_append(&mut opts, n);
    }
    if ipsecflowinfo != 0 && inp.has_flags(INP_IPSECFLOWINFO) {
        let n = sbcreatecontrol(&ipsecflowinfo.to_ne_bytes(), IP_IPSECFLOWINFO, IPPROTO_IP);
        opts_append(&mut opts, n);
    }
    m_adj(m, hlen);

    mtx_enter(&so.so_rcv.sb_mtx);
    if !sbappendaddr(&so.so_rcv, srcaddr, Some(m), opts) {
        mtx_leave(&so.so_rcv.sb_mtx);
        udpstat_inc(UdpstatCounters::UdpsFullsock);
        m_freem(m);
        m_freem(opts);
        return;
    }
    mtx_leave(&so.so_rcv.sb_mtx);

    sorwakeup(so);
}

/// `udp_notify`: notifies a udp user of an asynchronous error; just wakes up so that he can
/// collect error status.
pub fn udp_notify(inp: &'static Inpcb, errno: Option<Errno>) {
    let so = inp.socket();
    so.set_error(errno);
    sorwakeup(so);
    sowwakeup(so);
}

/// `udp6_ctlinput`: an ICMPv6 error (`cmd`) about a datagram to `sa`; `d` is the
/// `Ip6ctlparam` of `icmp6_notify_error` (or NULL). A path MTU change (`PRC_MSGSIZE`) updates
/// the route if a connected socket matches; then every matching socket is notified
/// (`in6_pcbnotify`). `INET6`; compiled always, as `netinet6` is, for `inet6sw`.
///
/// # Safety
///
/// `PrCtlinputFn`'s contract: `sa` is NULL or a readable socket address of its `sa_len`
/// bytes; `d` is NULL or the `Ip6ctlparam` of the ICMPv6 error, valid for the call.
pub unsafe fn udp6_ctlinput(cmd: i32, sa: *const Sockaddr, rdomain: u32, d: *mut c_void) {
    let mut d = d;
    let mut notify: InpNotifyFn = udp_notify;

    if sa.is_null() {
        return;
    }
    // SAFETY: the caller's contract: a readable socket address.
    let (family, len) = unsafe { ((*sa).sa_family, (*sa).sa_len) };
    if family != AF_INET6 || usize::from(len) != size_of::<SockaddrIn6>() {
        return;
    }
    // SAFETY: a whole `sockaddr_in6` (checked), read unaligned.
    let sa_sin6 = unsafe { satosin6_const(sa).read_unaligned() };

    if cmd as u32 as usize >= PRC_NCMDS {
        return;
    }
    if prc_is_redirect(cmd) {
        notify = in_pcbrtchange;
        d = ptr::null_mut();
    } else if cmd == PRC_HOSTDEAD {
        d = ptr::null_mut();
    } else if cmd == PRC_MSGSIZE {
        // special code is present, see below
    } else if INET6CTLERRMAP[cmd as usize].is_none() {
        return;
    }

    // if the parameter is from icmp6, decode it.
    let ip6cp: Option<&Ip6ctlparam> = if d.is_null() {
        None
    } else {
        // SAFETY: the caller's contract: a non-NULL `d` is the ICMPv6 error's parameter.
        Some(unsafe { &*d.cast::<Ip6ctlparam>() })
    };
    let (m, ip6, off, cmdarg) = match ip6cp {
        Some(p) => (p.ip6c_m, p.ip6c_ip6, p.ip6c_off, p.ip6c_cmdarg),
        None => {
            // XXX: translate addresses into internal form
            let mut sa6 = sa_sin6;
            if in6_embedscope(&mut sa6.sin6_addr, &sa_sin6, None, None).is_err() {
                // should be impossible
                return;
            }
            (None, ptr::null_mut(), 0, ptr::null_mut())
        }
    };
    let ifidx = m.map_or(0, |m| m.m_pkthdr().ph_ifidx.get());

    let mut sa6;
    if let Some(p) = ip6cp
        && !p.ip6c_finaldst.is_null()
    {
        // SAFETY: `icmp6_notify_error` points `ip6c_finaldst` at an address valid for the
        // call; read unaligned.
        let finaldst = unsafe { p.ip6c_finaldst.read_unaligned() };
        sa6 = SockaddrIn6::with_addr(finaldst);
        // XXX: assuming M is valid in this case
        sa6.sin6_scope_id = in6_addr2scopeid(ifidx, &finaldst) as u32;
        let mut embedded = finaldst;
        if in6_embedscope(&mut embedded, &sa6, None, None).is_err() {
            // should be impossible
            return;
        }
        // SAFETY: as above; the C embeds the scope into `*ip6c_finaldst` itself.
        unsafe { p.ip6c_finaldst.write_unaligned(embedded) };
    } else {
        // XXX: translate addresses into internal form
        sa6 = sa_sin6;
        if in6_embedscope(&mut sa6.sin6_addr, &sa_sin6, None, None).is_err() {
            // should be impossible
            return;
        }
    }

    if !ip6.is_null() {
        // XXX: We assume that when IPV6 is non NULL, M and OFF are valid.
        let Some(m) = m else {
            return;
        };

        // check if we can safely examine src and dst ports (`struct udp_portonly`)
        if (m.m_pkthdr().len.get() as usize) < off as usize + 2 * size_of::<u16>() {
            return;
        }

        let mut ports = [0u8; 4];
        m_copydata(m, off, &mut ports);
        let uh_sport = u16::from_ne_bytes([ports[0], ports[1]]);
        let uh_dport = u16::from_ne_bytes([ports[2], ports[3]]);

        // SAFETY: `ip6c_ip6` points at the quoted IPv6 header inside `m`, alive for the
        // call; the address is read unaligned.
        let src = unsafe { ptr::addr_of!((*ip6).ip6_src).read_unaligned() };
        let mut sa6_src = SockaddrIn6::with_addr(src);
        sa6_src.sin6_scope_id = in6_addr2scopeid(ifidx, &src) as u32;
        let scoped = sa6_src;
        if in6_embedscope(&mut sa6_src.sin6_addr, &scoped, None, None).is_err() {
            // should be impossible
            return;
        }

        if cmd == PRC_MSGSIZE {
            // Check to see if we have a valid UDP socket corresponding to the address in the
            // ICMPv6 message payload.
            let inp = in6_pcblookup(
                &UDB6TABLE,
                &sa6.sin6_addr,
                uh_dport,
                &sa6_src.sin6_addr,
                uh_sport,
                rdomain,
            );
            // #if 0 in the C: as the use of sendto(2) is fairly popular, we may want to allow
            // non-connected pcb too (in6_pcblookup_listen). But it could be too weak against
            // attacks... We should at least check if the local address (= s) is really ours.

            // Depending on the value of "valid" and routing table size (mtudisc_{hi,lo}wat),
            // we will:
            // - recalculate the new MTU and create the corresponding routing entry, or
            // - ignore the MTU change notification.
            if let Some(p) = ip6cp {
                icmp6_mtudisc_update(p, inp.is_some());
            }
            in_pcbunref(inp);

            // regardless of if we called icmp6_mtudisc_update(), we need to call
            // in6_pcbnotify(), to notify path MTU change to the userland (2292bis-02),
            // because some unconnected sockets may share the same destination and want to
            // know the path MTU.
        }

        in6_pcbnotify(
            &UDB6TABLE,
            &sa6,
            u32::from(uh_dport),
            Some(&sa6_src),
            u32::from(uh_sport),
            rdomain,
            cmd,
            cmdarg,
            Some(notify),
        );
    } else {
        in6_pcbnotify(
            &UDB6TABLE,
            &sa6,
            0,
            Some(&SA6_ANY),
            0,
            rdomain,
            cmd,
            cmdarg,
            Some(notify),
        );
    }
}

/// `udp_ctlinput`: an ICMP error (`cmd`) about a datagram to `sa`, whose IP header `v`
/// returned: notifies the socket that sent it, or every socket talking to `sa`.
///
/// # Safety
///
/// `PrCtlinputFn`'s contract: `sa` is NULL or a readable socket address of its `sa_len`
/// bytes; `v` is NULL or the returned IP header followed by at least the UDP ports.
pub unsafe fn udp_ctlinput(cmd: i32, sa: *const Sockaddr, rdomain: u32, v: *mut c_void) {
    let mut ip = v.cast_const().cast::<u8>();
    let mut notify: InpNotifyFn = udp_notify;

    if sa.is_null() {
        return;
    }
    // SAFETY: the caller's contract: a readable socket address.
    let (family, len) = unsafe { ((*sa).sa_family, (*sa).sa_len) };
    if family != AF_INET || usize::from(len) != size_of::<SockaddrIn>() {
        return;
    }
    // SAFETY: a `sockaddr_in` (checked), read unaligned.
    let dst = unsafe { sa.cast::<SockaddrIn>().read_unaligned() };
    if dst.sin_addr.s_addr == INADDR_ANY {
        return;
    }

    if cmd as u32 as usize >= PRC_NCMDS {
        return;
    }
    let errno = INETCTLERRMAP[cmd as usize];
    if prc_is_redirect(cmd) {
        notify = in_pcbrtchange;
        ip = ptr::null();
    } else if cmd == PRC_HOSTDEAD {
        ip = ptr::null();
    } else if errno.is_none() {
        return;
    }

    if !ip.is_null() {
        // SAFETY: the caller's contract: the returned IP header, then the UDP ports.
        let iph = unsafe { ip.cast::<Ip>().read_unaligned() };
        // SAFETY: as above, at the header's length.
        let uhp = unsafe { uh_read(ip.add(usize::from(iph.ip_hl()) << 2)) };
        let udpencap_port_local = UDPENCAP_PORT.load(Ordering::Relaxed);
        // PMTU discovery for udpencap
        if cmd == PRC_MSGSIZE
            && ip_mtudisc.load(Ordering::Relaxed) != 0
            && UDPENCAP_ENABLE.load(Ordering::Relaxed) != 0
            && udpencap_port_local != 0
            && i32::from(uhp.uh_sport) == udpencap_port_local
        {
            // SAFETY: this function's own contract, passed on.
            unsafe { udpencap_ctlinput(cmd, sa, rdomain, v) };
            return;
        }
        let inp = in_pcblookup(
            &UDBTABLE,
            iph.ip_dst,
            uhp.uh_dport,
            iph.ip_src,
            uhp.uh_sport,
            rdomain,
        );
        let so = inp.and_then(in_pcbsolock);
        if let (Some(_), Some(i)) = (so, inp) {
            notify(i, errno);
        }
        in_pcbsounlock(inp, so);
        in_pcbunref(inp);
    } else {
        in_pcbnotifyall(&UDBTABLE, &dst, rdomain, errno, Some(notify));
    }
}

/// `udp_output`: sends datagram `m` from `inp` to `addr` (or its peer), with the source
/// address an `IP_SENDSRCADDR` control message may choose.
pub fn udp_output(
    inp: &'static Inpcb,
    m: &'static Mbuf,
    addr: Option<&Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let mut sin: Option<SockaddrIn> = None;
    let mut ipsecflowinfo: u32 = 0;
    let mut src_sin = SockaddrIn::default();
    let len = m.m_pkthdr().len.get();
    let mut laddr = InAddr::default();

    #[cfg(feature = "inet6")]
    if inp.has_flags(INP_IPV6) {
        return udp6_output(inp, m, addr, control);
    }

    let result: Result<&'static Mbuf, Errno> = 'release: {
        // Compute the packet length of the IP header, and punt if the length looks bogus.
        if len as usize + size_of::<Udpiphdr>() > IP_MAXPACKET {
            break 'release Err(Errno::EMSGSIZE);
        }

        if let Some(control) = control {
            // XXX: Currently, we assume all the optional information is stored in a single
            // mbuf.
            if control.m_next().get().is_some() {
                break 'release Err(Errno::EINVAL);
            }

            let mut clen = control.m_len().get() as usize;
            let mut cmsgs = mtod::<u8>(control);
            loop {
                if clen < cmsg_len(0) {
                    break 'release Err(Errno::EINVAL);
                }
                // SAFETY: at least a header's bytes remain in the control mbuf (checked);
                // read unaligned.
                let cm = unsafe { cmsgs.cast::<Cmsghdr>().read_unaligned() };
                let cmlen = cm.cmsg_len as usize;
                if cmlen < cmsg_len(0) || cmsg_align(cmlen) > clen {
                    break 'release Err(Errno::EINVAL);
                }
                if inp.has_flags(INP_IPSECFLOWINFO)
                    && cmlen == cmsg_len(size_of::<u32>())
                    && cm.cmsg_level == IPPROTO_IP
                    && cm.cmsg_type == IP_IPSECFLOWINFO
                {
                    // SAFETY: the message holds a `uint32_t` after its header (its length
                    // says so and fits in the mbuf, checked).
                    ipsecflowinfo =
                        unsafe { cmsg_data(cmsgs.cast()).cast::<u32>().read_unaligned() };
                } else if cmlen == cmsg_len(size_of::<InAddr>())
                    && cm.cmsg_level == IPPROTO_IP
                    && cm.cmsg_type == IP_SENDSRCADDR
                {
                    // SAFETY: the message holds an `in_addr` after its header (its length
                    // says so and fits in the mbuf, checked).
                    src_sin.sin_addr =
                        unsafe { cmsg_data(cmsgs.cast()).cast::<InAddr>().read_unaligned() };
                    src_sin.sin_family = AF_INET;
                    src_sin.sin_len = size_of::<SockaddrIn>() as u8;
                    // no check on reuse when sin->sin_port == 0
                    if let Err(e) =
                        in_pcbaddrisavail(inp, &mut src_sin, 0, curproc_or_panic("udp_output"))
                    {
                        break 'release Err(e);
                    }
                }
                clen -= cmsg_align(cmlen);
                cmsgs = cmsgs.wrapping_add(cmsg_align(cmlen));
                if clen == 0 {
                    break;
                }
            }
        }

        if let Some(addr) = addr {
            let s = match in_nam2sin(addr) {
                // SAFETY: `in_nam2sin` checked the mbuf holds a whole `sockaddr_in`.
                Ok(s) => unsafe { s.read_unaligned() },
                Err(e) => break 'release Err(e),
            };
            if s.sin_port == 0 {
                break 'release Err(Errno::EADDRNOTAVAIL);
            }
            if inp.inp_faddr.get().s_addr != INADDR_ANY {
                break 'release Err(Errno::EISCONN);
            }
            if let Err(e) = in_pcbselsrc(&mut laddr, &s, inp) {
                break 'release Err(e);
            }

            if inp.inp_lport.get() == 0
                && let Err(e) = in_pcbbind(inp, None, curproc_or_panic("udp_output"))
            {
                break 'release Err(e);
            }

            if src_sin.sin_len > 0
                && src_sin.sin_addr.s_addr != INADDR_ANY
                && src_sin.sin_addr.s_addr != inp.inp_laddr.get().s_addr
            {
                src_sin.sin_port = inp.inp_lport.get();
                if inp.inp_laddr.get().s_addr != INADDR_ANY
                    && let Err(e) =
                        in_pcbaddrisavail(inp, &mut src_sin, 0, curproc_or_panic("udp_output"))
                {
                    break 'release Err(e);
                }
                laddr = src_sin.sin_addr;
            }
            sin = Some(s);
        } else {
            if inp.inp_faddr.get().s_addr == INADDR_ANY {
                break 'release Err(Errno::ENOTCONN);
            }
            laddr = inp.inp_laddr.get();
        }
        Ok(m)
    };
    let m = match result {
        Ok(m) => m,
        Err(e) => {
            // release:
            m_freem(m);
            m_freem(control);
            return Err(e);
        }
    };

    // Calculate data length and get a mbuf for UDP and IP headers.
    let Some(m) = m_prepend(m, size_of::<Udpiphdr>() as i32, M_DONTWAIT) else {
        m_freem(control);
        return Err(Errno::ENOBUFS);
    };

    // Fill in mbuf with extended UDP header and addresses and length put into network
    // format.
    // SAFETY: `m_prepend` made the first mbuf hold the header's bytes; read unaligned.
    let mut ui = unsafe { mtod::<Udpiphdr>(m).read_unaligned() };
    ui.ui_i.ih_x1 = [0; 9];
    ui.ui_i.ih_pr = IPPROTO_UDP as u8;
    ui.ui_i.ih_len = ((len as u16).wrapping_add(size_of::<Udphdr>() as u16)).to_be();
    ui.ui_i.ih_src = laddr;
    ui.ui_i.ih_dst = sin.map_or(inp.inp_faddr.get(), |s| s.sin_addr);
    ui.ui_u.uh_sport = inp.inp_lport.get();
    ui.ui_u.uh_dport = sin.map_or(inp.inp_fport.get(), |s| s.sin_port);
    ui.ui_u.uh_ulen = ui.ui_i.ih_len;
    // SAFETY: as above.
    unsafe { mtod::<Udpiphdr>(m).write_unaligned(ui) };
    let mut ip = mtod_ip(m);
    ip.ip_len = ((size_of::<Udpiphdr>() as i32 + len) as u16).to_be();
    ip.ip_ttl = inp.inp_ip.get().ip_ttl;
    ip.ip_tos = inp.inp_ip.get().ip_tos;
    mtod_ip_store(m, &ip);
    if UDPCKSUM.load(Ordering::Relaxed) != 0 {
        m.m_pkthdr()
            .csum_flags
            .set(m.m_pkthdr().csum_flags.get() | M_UDP_CSUM_OUT);
    }

    udpstat_inc(UdpstatCounters::UdpsOpackets);

    // force routing table
    m.m_pkthdr().ph_rtableid.set(inp.inp_rtableid.get());

    if inp.socket().has_state(SS_ISCONNECTED) {
        pf_mbuf_link_inpcb(m, Some(inp));
        m.m_pkthdr().ph_flowid.set(inp.inp_flowid.get());
        let cf = &m.m_pkthdr().csum_flags;
        cf.set(cf.get() | crate::sys::mbuf::M_FLOWID);
    }

    let error = ip_output(
        m,
        inp.inp_options.get(),
        Some(&inp.inp_route),
        inp.socket().so_options.get() & SO_BROADCAST,
        inp.moptions(),
        Some(&inp.inp_seclevel.get()),
        ipsecflowinfo,
    );

    // bail:
    m_freem(control);
    error
}

/// `sotoinpcb(so)` of a UDP socket the C knows to be attached.
fn inpcb_of(so: &Socket) -> &'static Inpcb {
    match sotoinpcb(so) {
        Some(inp) => inp,
        None => panic(format_args!("udp socket {:p}: no inpcb", so)),
    }
}

/// `udp_attach`: a control block in `udbtable`, the default TTL, the buffer sizes.
pub fn udp_attach(so: &'static Socket, _proto: i32, wait: i32) -> Result<(), Errno> {
    if !so.so_pcb.get().is_null() {
        return Err(Errno::EINVAL);
    }

    soreserve(
        so,
        UDP_SENDSPACE.load(Ordering::Relaxed) as u64,
        UDP_RECVSPACE.load(Ordering::Relaxed) as u64,
    )?;

    #[cfg(feature = "inet6")]
    let table = if so.dom_family() == i32::from(PF_INET6) {
        &UDB6TABLE
    } else {
        &UDBTABLE
    };
    #[cfg(not(feature = "inet6"))]
    let table = &UDBTABLE;
    in_pcballoc(so, table, wait)?;
    let inp = inpcb_of(so);
    #[cfg(feature = "inet6")]
    if inp.has_flags(INP_IPV6) {
        let mut ip6 = inp.inp_ipv6.get();
        ip6.ip6_hlim = IP6_DEFHLIM.load(Ordering::Relaxed) as u8;
        inp.inp_ipv6.set(ip6);
        return Ok(());
    }
    let mut ip = inp.inp_ip.get();
    ip.ip_ttl = IP_DEFTTL.load(Ordering::Relaxed) as u8;
    inp.inp_ip.set(ip);
    Ok(())
}

/// `udp_detach`.
pub fn udp_detach(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);

    let Some(inp) = sotoinpcb(so) else {
        return Err(Errno::EINVAL);
    };

    in_pcbdetach(inp);
    Ok(())
}

/// `udp_bind`.
pub fn udp_bind(so: &'static Socket, addr: &'static Mbuf, p: &Proc) -> Result<(), Errno> {
    let inp = inpcb_of(so);

    soassertlocked(so);
    in_pcbbind(inp, Some(addr), p)
}

/// `udp_connect`.
pub fn udp_connect(so: &'static Socket, addr: &'static Mbuf) -> Result<(), Errno> {
    let inp = inpcb_of(so);

    soassertlocked(so);

    #[cfg(feature = "inet6")]
    let connected = if inp.has_flags(INP_IPV6) {
        !in6_is_addr_unspecified(&inp.inp_faddr6.get())
    } else {
        inp.inp_faddr.get().s_addr != INADDR_ANY
    };
    #[cfg(not(feature = "inet6"))]
    let connected = inp.inp_faddr.get().s_addr != INADDR_ANY;
    if connected {
        return Err(Errno::EISCONN);
    }
    in_pcbconnect(inp, addr)?;

    soisconnected(so);
    Ok(())
}

/// `udp_disconnect`.
pub fn udp_disconnect(so: &'static Socket) -> Result<(), Errno> {
    let inp = inpcb_of(so);

    soassertlocked(so);

    #[cfg(feature = "inet6")]
    let unconnected = if inp.has_flags(INP_IPV6) {
        in6_is_addr_unspecified(&inp.inp_faddr6.get())
    } else {
        inp.inp_faddr.get().s_addr == INADDR_ANY
    };
    #[cfg(not(feature = "inet6"))]
    let unconnected = inp.inp_faddr.get().s_addr == INADDR_ANY;
    if unconnected {
        return Err(Errno::ENOTCONN);
    }
    in_pcbunset_laddr(inp);
    in_pcbdisconnect(inp);
    so.clear_state(SS_ISCONNECTED); // XXX

    Ok(())
}

/// `udp_shutdown`.
pub fn udp_shutdown(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);
    socantsendmore(so);
    Ok(())
}

/// `udp_send`.
pub fn udp_send(
    so: &'static Socket,
    m: Option<&'static Mbuf>,
    addr: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    soassertlocked_readonly(so);

    let Some(inp) = sotoinpcb(so) else {
        // PCB could be destroyed, but socket still spliced.
        m_freem(m);
        m_freem(control);
        return Err(Errno::EINVAL);
    };

    // PIPEX: pipex_l2tp_userland_lookup_session and pipex_l2tp_userland_output; not
    // configured.

    // sosend always hands over a packet (a pkthdr mbuf, empty or not).
    let Some(m) = m else {
        m_freem(control);
        return Err(Errno::EINVAL);
    };
    udp_output(inp, m, addr, control)
}

/// `udp_sysctl`: sysctl for udp variables.
pub fn udp_sysctl(
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
        UDPCTL_ROOTONLY | UDPCTL_BADDYNAMIC => {
            if *n == UDPCTL_ROOTONLY && newp != 0 && SECURELEVEL.load(Ordering::Relaxed) > 0 {
                return Err(Errno::EPERM);
            }
            let ports = if *n == UDPCTL_ROOTONLY {
                &ROOTONLYPORTS
            } else {
                &BADDYNAMICPORTS
            };
            let mut buf = [0u8; DP_MAPSIZE * size_of::<u32>()];

            net_lock_shared();
            for (i, w) in ports.udp.iter().enumerate() {
                buf[i * 4..i * 4 + 4].copy_from_slice(&w.load(Ordering::Relaxed).to_ne_bytes());
            }
            net_unlock_shared();

            let error = sysctl_struct(oldp, oldlenp, newp, newlen, &mut buf);

            if error.is_ok() && newp != 0 {
                net_lock();
                for (i, w) in ports.udp.iter().enumerate() {
                    let mut b = [0u8; 4];
                    b.copy_from_slice(&buf[i * 4..i * 4 + 4]);
                    w.store(u32::from_ne_bytes(b), Ordering::Relaxed);
                }
                net_unlock();
            }

            error
        }
        UDPCTL_STATS => {
            if newp != 0 {
                return Err(Errno::EPERM);
            }

            udp_sysctl_udpstat(oldp, oldlenp, newp)
        }

        _ => sysctl_bounded_arr(&UDPCTL_VARS, name, oldp, oldlenp, newp, newlen),
    }
}

/// `udp_sysctl_udpstat`: `net.inet.udp.stats`, the counters as a `struct udpstat`.
fn udp_sysctl_udpstat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    const _: () = assert!(size_of::<Udpstat>() == UDPS_NCOUNTERS * size_of::<u64>());
    let mut bytes = [0u8; UDPS_NCOUNTERS * size_of::<u64>()];
    for (i, c) in UDPCOUNTERS.iter().enumerate() {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&c.load(Ordering::Relaxed).to_ne_bytes());
    }

    sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
}

#[cfg(test)]
mod tests;
