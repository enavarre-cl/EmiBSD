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
//! - `ip6_exthdr_get` (`netinet6/ip6_input.c`, not ported) is a private helper here: the
//!   `m_pulldown` of the header and its address. `udp_sbappend` takes the UDP header as a
//!   copy; an `inp_upcall` gets the address of a copy of it.
//! - `udp_ctlinput` is an `unsafe fn` (`PrCtlinputFn`): it reads the returned IP and UDP
//!   headers through the raw argument; its `notify` takes `Option<Errno>` (`in_pcb.rs`).
//! - `udp_sysctl`'s port bitmaps are copied through a byte buffer on the stack (the C's
//!   `malloc(M_SYSCTL)`), as `sysctl_struct` takes bytes.
//! - Not configured, each a comment at its site: `INET6` (`udb6table`, `udp6_usrreqs`,
//!   `udp6_ctlinput`, `udp6_output`, the IPv6 paths), `IPSEC` (UDP encapsulation of ESP, the
//!   SPD lookup, `IP_IPSECFLOWINFO` control messages), `NPF` (`pf_inp_lookup`,
//!   `pf_inp_link`, `pf_mbuf_link_inpcb`), `PIPEX` and `NSTOEPLITZ` (the flow id).
//! - `SMALL_KERNEL` is not set: the sysctl handlers are compiled.

use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_sysctl::{SECURELEVEL, sysctl_bounded_arr, sysctl_rdstruct, sysctl_struct};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_adj, m_copym, m_freem, m_prepend};
use crate::kern::uipc_mbuf2::m_pulldown;
use crate::kern::uipc_socket::{sorwakeup, sowwakeup};
use crate::kern::uipc_socket2::{
    sbappendaddr, sbcreatecontrol, soassertlocked, soassertlocked_readonly, socantsendmore,
    soisconnected, soreserve,
};
use crate::machine::cpu::curproc;
use crate::net::if_var::Netstack;
use crate::net::rtable::rtable_l2;
use crate::netinet::in_::{
    INADDR_ANY, IP_RECVDSTPORT, IP_SENDSRCADDR, IPPROTO_DONE, IPPROTO_IP, IPPROTO_UDP, InAddr,
    SockaddrIn, in_control, in_nam2sin,
};
use crate::netinet::in_pcb::{
    BADDYNAMICPORTS, DP_MAPSIZE, INP_CONTROLOPTS, INP_IPV6, INP_RECVDSTPORT, InpNotifyFn, Inpcb,
    InpcbIterator, Inpcbtable, ROOTONLYPORTS, in_flowid, in_pcb_iterator, in_pcb_iterator_abort,
    in_pcbaddrisavail, in_pcballoc, in_pcbbind, in_pcbconnect, in_pcbdetach, in_pcbdisconnect,
    in_pcbinit, in_pcblookup, in_pcblookup_listen, in_pcbnotifyall, in_pcbref, in_pcbrtchange,
    in_pcbselsrc, in_pcbsolock, in_pcbsounlock, in_pcbunref, in_pcbunset_laddr, in_peeraddr,
    in_sockaddr, sotoinpcb,
};
use crate::netinet::in4_cksum::in4_cksum;
use crate::netinet::ip::{IP_MAXPACKET, Ip};
use crate::netinet::ip_icmp::{ICMP_UNREACH, ICMP_UNREACH_PORT, icmp_error};
use crate::netinet::ip_input::{INETCTLERRMAP, IP_DEFTTL, ip_savecontrol};
use crate::netinet::ip_output::ip_output;
use crate::netinet::ip_var::{mtod_ip, mtod_ip_store};
use crate::netinet::udp::Udphdr;
use crate::netinet::udp_var::{
    UDPCTL_BADDYNAMIC, UDPCTL_CHECKSUM, UDPCTL_RECVSPACE, UDPCTL_ROOTONLY, UDPCTL_SENDSPACE,
    UDPCTL_STATS, UDPS_NCOUNTERS, Udpiphdr, Udpstat, UdpstatCounters, udpstat_inc,
};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{
    M_BCAST, M_COPYALL, M_DONTWAIT, M_MCAST, M_UDP_CSUM_IN_BAD, M_UDP_CSUM_IN_OK, M_UDP_CSUM_OUT,
    Mbuf, mtod,
};
use crate::sys::proc::Proc;
use crate::sys::protosw::{PRC_HOSTDEAD, PRC_NCMDS, PrUsrreqs, prc_is_redirect};
use crate::sys::socket::{
    AF_INET, Cmsghdr, SO_BROADCAST, SO_REUSEADDR, SO_REUSEPORT, SO_TIMESTAMP, Sockaddr, cmsg_align,
    cmsg_data, cmsg_len,
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

// INET6: udp6_usrreqs; not configured.

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
// INET6: udb6table; not configured.

/// `udpcounters`.
pub static UDPCOUNTERS: [AtomicU64; UDPS_NCOUNTERS] = [const { AtomicU64::new(0) }; UDPS_NCOUNTERS];

/// The bytes of a `sockaddr_in`.
fn sin_bytes(sin: &SockaddrIn) -> &[u8] {
    // SAFETY: `SockaddrIn` is `#[repr(C)]` without padding: its bytes are initialised.
    unsafe { core::slice::from_raw_parts(ptr::from_ref(sin).cast::<u8>(), size_of::<SockaddrIn>()) }
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
    // INET6: in_pcbinit(&udb6table, ...); not configured.
}

/// `ip6_exthdr_get(mp, off, len)` (`netinet6/ip6_input.c`): makes `len` bytes at `off`
/// contiguous and returns their address; `None` (and `*mp` cleared, the chain freed) when the
/// packet is too short.
fn ip6_exthdr_get(mp: &mut Option<&'static Mbuf>, off: i32, len: i32) -> Option<*mut u8> {
    let m = (*mp)?;
    let mut toff = 0;
    let Some(t) = m_pulldown(m, off, len, Some(&mut toff)) else {
        *mp = None;
        return None;
    };
    Some(mtod::<u8>(t).wrapping_add(toff as usize))
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
    let ipsecflowinfo: u32 = 0;
    // IPSEC: udpencap_port_local; not configured.

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
        // drop.
        let len = i32::from(u16::from_be(hdr.uh_ulen));
        let (ip, save_ip) = if af == i32::from(AF_INET) {
            let plen = m.m_pkthdr().len.get() - iphlen;
            if plen != len {
                if len > plen || len < size_of::<Udphdr>() as i32 {
                    udpstat_inc(UdpstatCounters::UdpsBadlen);
                    break 'bad;
                }
                m_adj(m, len - plen);
            }
            let ip = mtod_ip(m);
            // Save a copy of the IP header in case we want restore it for sending an ICMP
            // error message in response.
            (ip, ip)
        } else {
            // INET6: AF_INET6 (jumbograms, the length check); not configured.
            crate::net::if_::unhandled_af(af);
        };

        // Checksum extended UDP header and data. from W.R.Stevens: check incoming udp cksums
        // even if udpcksum is not set.
        let savesum = hdr.uh_sum;
        if hdr.uh_sum == 0 {
            udpstat_inc(UdpstatCounters::UdpsNosum);
            // INET6: in IPv6, the UDP checksum is ALWAYS used; not configured.
        } else if m.m_pkthdr().csum_flags.get() & M_UDP_CSUM_IN_OK == 0 {
            if m.m_pkthdr().csum_flags.get() & M_UDP_CSUM_IN_BAD != 0 {
                udpstat_inc(UdpstatCounters::UdpsBadsum);
                break 'bad;
            }
            udpstat_inc(UdpstatCounters::UdpsInswcsum);

            let sum = in4_cksum(m, IPPROTO_UDP as u8, iphlen, len);
            // SAFETY: as for `hdr`.
            unsafe { uh_set_sum(uh, sum) };
            // INET6: in6_cksum; not configured.
            if sum != 0 {
                udpstat_inc(UdpstatCounters::UdpsBadsum);
                break 'bad;
            }
        }
        m.m_pkthdr()
            .csum_flags
            .set(m.m_pkthdr().csum_flags.get() & !M_UDP_CSUM_OUT);

        // IPSEC: UDP encapsulated ESP (udpencap_enable, udpencap_port, the SPI check and
        // ipsec_common_input); not configured.

        let srcsa = SockaddrIn {
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_family: AF_INET,
            sin_port: hdr.uh_sport,
            sin_addr: ip.ip_src,
            ..SockaddrIn::default()
        };
        // dstsa: the C fills it and does not read it further.
        // INET6: the sockaddr_in6 pair with in6_recoverscope; not configured.

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

            // INET6: udb6table for IPv6; not configured.
            let table = &UDBTABLE;

            mtx_enter(&table.inpt_mtx);
            // SAFETY: the table mutex is held around every call; `iter` lives on this frame
            // until the walk ends with `None` or is aborted.
            while let Some(i) = unsafe { in_pcb_iterator(table, inp, &iter) } {
                inp = Some(i);
                kassert!(!i.has_flags(INP_IPV6));

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
                // INET6: the minimum hop limit and the IPv6 local address; not configured.
                {
                    let minttl = i.inp_ip_minttl.get();
                    if minttl != 0 && minttl > ip.ip_ttl {
                        continue;
                    }

                    let laddr = i.inp_laddr.get().s_addr;
                    if laddr != INADDR_ANY && laddr != ip.ip_dst.s_addr {
                        continue;
                    }
                }
                let faddr = i.inp_faddr.get().s_addr;
                if faddr != INADDR_ANY
                    && (faddr != ip.ip_src.s_addr || i.inp_fport.get() != hdr.uh_sport)
                {
                    continue;
                }

                if let Some(l) = last {
                    mtx_leave(&table.inpt_mtx);

                    if let Some(n) = m_copym(m, 0, M_COPYALL, M_DONTWAIT) {
                        udp_sbappend(l, n, Some(&ip), iphlen, &hdr, &srcsa, 0, ns);
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

            udp_sbappend(last, m, Some(&ip), iphlen, &hdr, &srcsa, 0, ns);
            in_pcbunref(Some(last));

            *mp = None;
            return IPPROTO_DONE;
        }
        // Locate pcb for datagram.
        // NPF > 0: inp = pf_inp_lookup(m); not configured.
        // INET6: in6_pcblookup for IPv6; not configured.
        inp = in_pcblookup(
            &UDBTABLE,
            ip.ip_src,
            hdr.uh_sport,
            ip.ip_dst,
            hdr.uh_dport,
            m.m_pkthdr().ph_rtableid.get(),
        );
        if inp.is_none() {
            udpstat_inc(UdpstatCounters::UdpsPcbhashmiss);
            // INET6: in6_pcblookup_listen for IPv6; not configured.
            inp = in_pcblookup_listen(
                &UDBTABLE,
                ip.ip_dst,
                hdr.uh_dport,
                Some(m),
                m.m_pkthdr().ph_rtableid.get(),
            );
        }

        // IPSEC: the PACKET_TAG_IPSEC_IN_DONE tdb, ipsp_spd_lookup and the flow info;
        // not configured.

        let Some(i) = inp else {
            udpstat_inc(UdpstatCounters::UdpsNoport);
            if m.m_flags().get() & (M_BCAST | M_MCAST) != 0 {
                udpstat_inc(UdpstatCounters::UdpsNoportbcast);
                break 'bad;
            }
            // INET6: icmp6_error for IPv6; not configured.
            mtod_ip_store(m, &save_ip);
            // SAFETY: as for `hdr`; `mtod_ip_store` rewrote only the IP header.
            unsafe { uh_set_sum(uh, savesum) };
            icmp_error(m, ICMP_UNREACH, ICMP_UNREACH_PORT, 0, 0);
            *mp = None;
            return IPPROTO_DONE;
        };

        soassertlocked_readonly(i.socket());

        // INET6: the minimum hop limit for IPv6; not configured.
        let minttl = i.inp_ip_minttl.get();
        if minttl != 0 && minttl > ip.ip_ttl {
            break 'bad;
        }

        // NPF > 0: pf_inp_link for a connected socket; not configured.
        // PIPEX: pipex_l2tp_lookup_session and pipex_l2tp_input; not configured.

        udp_sbappend(i, m, Some(&ip), iphlen, &hdr, &srcsa, ipsecflowinfo, ns);
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

/// `udp_sbappend`: appends datagram `m` from `srcaddr` (its IP header `ip`, `hlen` bytes, and
/// UDP header `uh` still in front) to the receive buffer of `inp`'s socket, with the control
/// messages the socket asked for.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn udp_sbappend(
    inp: &Inpcb,
    m: &'static Mbuf,
    ip: Option<&Ip>,
    hlen: i32,
    uh: &Udphdr,
    srcaddr: &SockaddrIn,
    _ipsecflowinfo: u32,
    ns: Option<&Netstack>,
) {
    let so = inp.socket();
    let mut opts: Option<&'static Mbuf> = None;
    let mut m = m;

    let hlen = hlen + size_of::<Udphdr>() as i32;

    if let Some(upcall) = inp.inp_upcall.get() {
        let ipp = ip.map_or(ptr::null(), ptr::from_ref);
        let mut uhc = *uh;
        let Some(n) = upcall(
            inp.inp_upcall_arg.get(),
            m,
            ipp,
            ptr::null(),
            ptr::from_mut(&mut uhc).cast::<c_void>(),
            hlen,
            ns,
        ) else {
            return;
        };
        m = n;
    }

    // INET6: ip6_savecontrol; not configured.
    if let Some(ip) = ip
        && (inp.has_flags(INP_CONTROLOPTS) || so.has_options(SO_TIMESTAMP))
    {
        ip_savecontrol(inp, &mut opts, ip, m);
    }
    // INET6: IPV6_RECVDSTPORT; not configured.
    if ip.is_some() && inp.has_flags(INP_RECVDSTPORT) {
        let n = sbcreatecontrol(&uh.uh_dport.to_ne_bytes(), IP_RECVDSTPORT, IPPROTO_IP);
        match opts {
            None => opts = n,
            Some(mut t) => {
                while let Some(next) = t.m_next().get() {
                    t = next;
                }
                t.m_next().set(n);
            }
        }
    }
    // IPSEC: IP_IPSECFLOWINFO; not configured.
    m_adj(m, hlen);

    mtx_enter(&so.so_rcv.sb_mtx);
    if !sbappendaddr(&so.so_rcv, sin_bytes(srcaddr), Some(m), opts) {
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

// INET6: udp6_ctlinput; not configured.

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
        // IPSEC: PMTU discovery for udpencap (udpencap_ctlinput); not configured.
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
    let ipsecflowinfo: u32 = 0;
    let mut src_sin = SockaddrIn::default();
    let len = m.m_pkthdr().len.get();
    let mut laddr = InAddr::default();

    // INET6: udp6_output for INP_IPV6; not configured.

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
                // IPSEC: IP_IPSECFLOWINFO with INP_IPSECFLOWINFO; not configured.
                if cmlen == cmsg_len(size_of::<InAddr>())
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
        // NPF > 0: pf_mbuf_link_inpcb; NSTOEPLITZ > 0: the flow id; not configured.
    }

    let error = ip_output(
        m,
        inp.inp_options.get(),
        Some(&inp.inp_route),
        inp.socket().so_options.get() & SO_BROADCAST,
        inp.moptions(),
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

    // INET6: udb6table for PF_INET6; not configured.
    let table = &UDBTABLE;
    in_pcballoc(so, table, wait)?;
    // INET6: ip6_hlim for INP_IPV6; not configured.
    let inp = inpcb_of(so);
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

    // INET6: the IPv6 foreign address for INP_IPV6; not configured.
    if inp.inp_faddr.get().s_addr != INADDR_ANY {
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

    // INET6: the IPv6 foreign address for INP_IPV6; not configured.
    if inp.inp_faddr.get().s_addr == INADDR_ANY {
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
