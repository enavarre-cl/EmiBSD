/*	$OpenBSD: if_ethersubr.c,v 1.308 2025/12/19 02:04:13 dlg Exp $	*/
/*	$NetBSD: if_ethersubr.c,v 1.19 1996/05/07 02:40:30 thorpej Exp $	*/
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
 * Copyright (c) 1982, 1989, 1993
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
 *	@(#)if_ethersubr.c	8.1 (Berkeley) 6/10/93
 */

/*
%%% portions-copyright-nrl-95
Portions of this software are Copyright 1995-1998 by Randall Atkinson,
Ronald Lee, Daniel McDonald, Bao Phan, and Chris Winters. All Rights
Reserved. All rights under this copyright have been assigned to the US
Naval Research Laboratory (NRL). The NRL Copyright Notice and License
Agreement Version 1.1 (January 17, 1995) applies to these portions of the
software.
You should have received a copy of the license with this software. If you
didn't get a copy, you may request one from <license@ipv6.nrl.navy.mil>.
*/
/* </LICENSES> */

//! Ethernet: `net/if_ethersubr.c`, the link layer every Ethernet driver shares: attaching an
//! interface (`ether_ifattach`), encapsulating and sending packets (`ether_output`,
//! `ether_resolve`, `ether_encap`), receiving frames and handing them to the protocols
//! (`ether_input`), multicast filters (`ether_addmulti`/`ether_delmulti`), the CRC used by
//! hardware multicast hashes, and helpers to look inside a frame.
//!
//! Upstream: sys/net/if_ethersubr.c @ 3ce1f3f79392
//!
//! `ether_input` filters a frame in phases before the protocols see it: an aggregation port
//! (`aggr(4)`, `trunk(4)`) may take it; service-delimited (VLAN tagged) frames go to
//! `vlan(4)`; a bridge port may take it; a frame not addressed to this port must be multicast
//! or broadcast (and not our own, unless the interface is simplex); then the Ethernet type
//! picks the protocol input function, which `if_input_proto` calls in the softnet thread.
//!
//! A driver's softc embeds a `struct arpcom` (`netinet/if_ether.rs`), sets its `ac_if`
//! (name, `if_softc`, flags, `if_ioctl`, `if_start` or `IFXF_MPSAFE` + `if_qstart`) and the
//! hardware address in `ac_enaddr`, then calls `if_attach(&ac.ac_if)` and
//! `ether_ifattach(&ac)`. Received frames go to `if_input(ifp, &ml)`.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - The protocols are not ported: `ether_input` reports `ipv4_input` (`ETHERTYPE_IP`),
//!   `arpinput` (`ETHERTYPE_ARP`) and `revarpinput` (`ETHERTYPE_REVARP`) and drops the frame
//!   where the C hands it to them; `ether_resolve` reports `arpresolve` and drops the packet
//!   (which `arpresolve` would own); `ether_rtrequest` reports `arp_rtrequest`;
//!   `ether_offload_ifcap` reports `in_hdr_cksum_out`/`in_proto_cksum_out`. When
//!   `netinet/if_ether.c` and `netinet/ip_input.c` land, their arms in `ether_input`'s
//!   `match` name the input functions.
//! - Options and pseudo-devices that are not ported are not configured, their code a comment
//!   at each site: `vlan(4)` (`NVLAN` 0, so a tagged frame is service delimited and dropped
//!   unless a bridge takes it, as the C does without vlan), `carp(4)`, `pppoe(4)`/`PIPEX`,
//!   `bpe(4)`, `bpf(4)`, `af_frame` (`NAF_FRAME` 0: the frame sockets need sockets, so the
//!   whole `#if NAF_FRAME > 0` part, `ether_frm_*` and `struct ether_pcb`, is not compiled),
//!   `INET6` (`nd6_*`, `ether_ip6multicast_*`, the IPv6 cases) and `MPLS`.
//! - `ether_ifattach` takes the `struct arpcom` (the C takes its `ac_if` and casts), which
//!   lets it mark the interface for the checked cast `arpcom_of`; `ether_ioctl`,
//!   `ether_addmulti` and `ether_delmulti` take it as the C does.
//! - `ether_sprintf` returns the string in a buffer by value (NUL-terminated, 18 bytes)
//!   instead of a static buffer; `ether_ntoa(3)` is userland.
//! - `ether_addr_to_e64` and `ether_e64_to_addr` work on the six bytes (`&[u8; 6]`) rather
//!   than on `struct ether_addr`; `ether_multiaddr` returns the range instead of filling two
//!   arrays; `ether_encap` returns `Ok(None)` where the C returns NULL with error 0 (the
//!   packet was queued by address resolution).
//! - `ether_addmulti`/`ether_delmulti` return `Err(ENETRESET)` when the list changed, the
//!   C's "error" that tells the driver to reprogram its filter.
//! - The CRC tables of `ether_crc32_le_update`/`_be_update` are computed at compile time from
//!   the polynomials (`docs/C_TO_RUST.md`, constant tables); the unit tests check them
//!   against the C's values. The C's `#if 0` reference bit-loop versions are not compiled in
//!   C either.
//! - `ETHERDEBUG` is not defined, so `DPRINTF`/`DNPRINTF` print nothing, as in C.
//! - `tcphdr` and `udphdr` are not ported: `ether_extract_headers` reads the TCP data offset
//!   at its wire offset (byte 12, high nibble) and uses the wire sizes (20 and 8 bytes).

use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use crate::dev::rnd::arc4random;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::{refcnt_init_trace, refcnt_rele, refcnt_take};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::kern::uipc_mbuf::{m_adj, m_copym, m_freem, m_getptr, m_prepend, m_pullup};
use crate::machine::intr::{splnet, splx};
use crate::net::ethertypes::{
    ETHERTYPE_ARP, ETHERTYPE_IP, ETHERTYPE_QINQ, ETHERTYPE_REVARP, ETHERTYPE_VLAN,
};
use crate::net::if_::{
    IFCAP_CSUM_IPv4, IFCAP_CSUM_TCPv4, IFCAP_CSUM_TCPv6, IFCAP_CSUM_UDPv4, IFCAP_CSUM_UDPv6,
    IFF_MULTICAST, IFF_NOARP, IFF_RUNNING, IFF_SIMPLEX, Ifreq, if_alloc_sadl, if_deactivate,
    if_enqueue, if_input_local, if_input_proto,
};
use crate::net::if_dl::lladdr;
use crate::net::if_types::IFT_ETHER;
use crate::net::if_var::{IfInputFn, Ifnet, Netstack};
use crate::net::route::Rtentry;
use crate::netinet::if_ether::{
    Arpcom, ETHER_ADDR_LEN, ETHER_ALIGN, ETHER_HDR_LEN, ETHERMIN, ETHERMTU, EtherExtracted,
    EtherHeader, EtherMulti, EtherMultiList, EtherPort, EtherVlanHeader, arpcom_of,
    eth64_is_broadcast, eth64_is_multicast, ether_is_multicast, ether_lookup_multi,
    ether_map_ip_multicast,
};
use crate::netinet::in_::{INADDR_ANY, IPPROTO_TCP, IPPROTO_UDP, SockaddrIn};
use crate::netinet::ip::{IP_MF, IP_OFFMASK, Ip};
use crate::sys::endian::{htons, ntohs};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_IFMADDR, M_NOWAIT};
use crate::sys::mbuf::{
    M_BCAST, M_COPYALL, M_DONTWAIT, M_IPV4_CSUM_OUT, M_MCAST, M_PKTHDR, M_TCP_CSUM_OUT,
    M_UDP_CSUM_OUT, M_VLANTAG, Mbuf, mtod,
};
use crate::sys::queue::ListHead;
use crate::sys::refcnt::{DT_REFCNT_IDX_ETHMULTI, Refcnt};
use crate::sys::socket::{AF_INET, AF_UNSPEC, Sockaddr, pseudo_AF_HDRCMPLT};
use crate::sys::sockio::{SIOCADDMULTI, SIOCDELMULTI, SIOCSIFADDR, SIOCSIFMTU};
use crate::{kassert, unported};

/// `ETHER_CRC_POLY_LE` as the table generator uses it.
const CRC_POLY_LE: u32 = 0xedb8_8320;
/// The big-endian polynomial with its top bit, as the C's table holds it (`0x04c11db7`).
const CRC_POLY_BE: u32 = 0x04c1_1db7;

/// `ether_crc32_le_update`'s `crctab`: the CRC of each nibble, little-endian.
const CRCTAB_LE: [u32; 16] = crctab_le();
/// `ether_crc32_be_update`'s `crctab`.
const CRCTAB_BE: [u32; 16] = crctab_be();
/// `ether_crc32_be_update`'s `rev`: each nibble bit-reversed.
const REV: [u8; 16] = nibble_rev();

/// The wire size of `struct tcphdr` (not ported).
const TCPHDR_LEN: usize = 20;
/// The wire offset of `th_off` (the high nibble of the byte before `th_flags`).
const TCPHDR_OFF_BYTE: usize = 12;
/// The wire size of `struct udphdr` (not ported).
const UDPHDR_LEN: usize = 8;

/// `etherbroadcastaddr`.
pub static ETHERBROADCASTADDR: [u8; ETHER_ADDR_LEN] = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
/// `etheranyaddr`.
pub static ETHERANYADDR: [u8; ETHER_ADDR_LEN] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// `ether_ipmulticast_min`: the first Ethernet address of the IP multicast range.
pub static ETHER_IPMULTICAST_MIN: [u8; ETHER_ADDR_LEN] = [0x01, 0x00, 0x5e, 0x00, 0x00, 0x00];
/// `ether_ipmulticast_max`: the last one.
pub static ETHER_IPMULTICAST_MAX: [u8; ETHER_ADDR_LEN] = [0x01, 0x00, 0x5e, 0x7f, 0xff, 0xff];
// INET6: ether_ip6multicast_min/max (33:33:00:00:00:00 .. 33:33:ff:ff:ff:ff); not configured.

/// `ether_fakeaddr`'s `unit`.
static FAKEADDR_UNIT: AtomicI32 = AtomicI32::new(0);

/// `ether_ioctl`: the Ethernet part of a driver's `if_ioctl`: `SIOCSIFMTU` and the multicast
/// list; `ENOTTY` for what the driver must handle itself.
///
/// # Safety
///
/// `data` points at the `struct ifreq` the command takes.
pub unsafe fn ether_ioctl(ifp: &Ifnet, arp: &Arpcom, cmd: u64, data: *mut u8) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ifr = unsafe { &*data.cast::<Ifreq>() };

    match cmd {
        SIOCSIFADDR => Ok(()),

        SIOCSIFMTU => {
            let mtu = ifr.ifr_mtu();
            if mtu < ETHERMIN as i32 || mtu as u32 > ifp.if_hardmtu.get() {
                Err(Errno::EINVAL)
            } else {
                ifp.if_mtu.set(mtu as u32);
                Ok(())
            }
        }

        SIOCADDMULTI | SIOCDELMULTI => {
            if ifp.if_flags.get() & IFF_MULTICAST != 0 {
                if cmd == SIOCADDMULTI {
                    ether_addmulti(ifr, arp)
                } else {
                    ether_delmulti(ifr, arp)
                }
            } else {
                Err(Errno::ENOTTY)
            }
        }

        _ => Err(Errno::ENOTTY),
    }
}

/// `ether_rtrequest`: an Ethernet interface's `if_rtrequest`, by the route's family.
pub fn ether_rtrequest(ifp: &'static Ifnet, req: i32, rt: Option<&'static Rtentry>) {
    let Some(rt) = rt else {
        return;
    };

    // SAFETY: a route's key is a valid sockaddr.
    if unsafe { (*rt.rt_key()).sa_family } == AF_INET {
        // arp_rtrequest(ifp, req, rt): netinet/if_ether.c is not ported.
        let _ = (ifp, req);
        let _ = unported!("arp_rtrequest");
    }
    // INET6: nd6_rtrequest(ifp, req, rt) for AF_INET6; IPv6 is not configured.
}

/// `ether_resolve`: fills the Ethernet header `eh` for a packet to `dst`: the destination
/// from address resolution (or the sockaddr), the type, our source address. On an error the
/// packet has been freed (or kept by address resolution).
///
/// # Safety
///
/// `dst` points at a readable socket address of `sa_len` bytes.
pub unsafe fn ether_resolve(
    ifp: &'static Ifnet,
    m: &'static Mbuf,
    dst: *const Sockaddr,
    rt: Option<&'static Rtentry>,
    eh: &mut EtherHeader,
) -> Result<(), Errno> {
    let ac = arpcom_of(ifp);
    // SAFETY: the caller's contract.
    let af = unsafe { (*dst).sa_family };

    let error = 'bad: {
        if ifp.if_flags.get() & IFF_RUNNING == 0 {
            break 'bad Errno::ENETDOWN;
        }

        kassert!(
            rt.is_some()
                || m.m_flags().get() & (M_MCAST | M_BCAST) != 0
                || af == AF_UNSPEC
                || af == pseudo_AF_HDRCMPLT
        );

        #[cfg(feature = "diagnostic")]
        {
            // rtable_l2(m->m_pkthdr.ph_rtableid): net/rtable.c is not ported; every table is
            // in routing domain 0.
            let _ = unported!("rtable_l2");
            if ifp.if_rdomain.get() != 0 {
                printf(format_args!(
                    "{}: trying to send packet on wrong domain. if {} vs. mbuf {}\n",
                    Str(&ifp.if_xname.get()),
                    ifp.if_rdomain.get(),
                    0
                ));
            }
        }

        match af {
            AF_INET => {
                // error = arpresolve(ifp, rt, m, dst, eh->ether_dhost): netinet/if_ether.c
                // is not ported; arpresolve owns the packet when it fails.
                let error: Result<(), Errno> = Err(unported!("arpresolve"));
                if let Err(e) = error {
                    m_freem(m);
                    return Err(e);
                }
                eh.ether_type = htons(ETHERTYPE_IP);

                // If broadcasting on a simplex interface, loopback a copy. The checksum must
                // be calculated in software. Keep the condition in sync with
                // in_ifcap_cksum().
                if m.m_flags().get() & M_BCAST != 0
                    && ifp.if_flags.get() & IFF_SIMPLEX != 0
                    && m.m_pkthdr().pf.routed.get() == 0
                {
                    // XXX Should we input an unencrypted IPsec packet?
                    if let Some(mcopy) = m_copym(m, 0, M_COPYALL, M_NOWAIT) {
                        let _ = if_input_local(ifp, mcopy, af, None);
                    }
                }
            }
            // INET6: nd6_resolve and ETHERTYPE_IPV6; IPv6 is not configured.
            // MPLS: the gateway's link address or address resolution, ETHERTYPE_MPLS(_MCAST);
            // not configured.
            af if af == pseudo_AF_HDRCMPLT => {
                // take the whole header from the sa
                // SAFETY: `sa_data` holds the fourteen bytes of an Ethernet header.
                unsafe { copy_header(eh, dst) };
                return Ok(());
            }

            AF_UNSPEC => {
                // take the dst and type from the sa, but get src below
                // SAFETY: as above.
                unsafe { copy_header(eh, dst) };
            }

            _ => {
                printf(format_args!(
                    "{}: can't handle af{}\n",
                    Str(&ifp.if_xname.get()),
                    af
                ));
                break 'bad Errno::EAFNOSUPPORT;
            }
        }

        eh.ether_shost = ac.ac_enaddr.get();

        return Ok(());
    };

    // bad:
    m_freem(m);
    Err(error)
}

/// `memcpy(eh, dst->sa_data, sizeof(*eh))`.
///
/// # Safety
///
/// `dst` points at a readable `struct sockaddr`.
unsafe fn copy_header(eh: &mut EtherHeader, dst: *const Sockaddr) {
    // SAFETY: `sa_data` is fourteen bytes, an Ethernet header's size (checked in
    // `netinet/if_ether.rs`); `eh` is any bytes.
    unsafe {
        ptr::copy_nonoverlapping(
            ptr::addr_of!((*dst).sa_data).cast::<u8>(),
            ptr::from_mut(eh).cast::<u8>(),
            size_of::<EtherHeader>(),
        );
    }
}

/// `ether_encap`: resolves the destination and prepends the Ethernet header. `Ok(None)` when
/// address resolution kept the packet for later (the C's `EAGAIN`, returned as success).
///
/// # Safety
///
/// As for [`ether_resolve`].
pub unsafe fn ether_encap(
    ifp: &'static Ifnet,
    m: &'static Mbuf,
    dst: *const Sockaddr,
    rt: Option<&'static Rtentry>,
) -> Result<Option<&'static Mbuf>, Errno> {
    let mut eh = EtherHeader::default();

    // SAFETY: the caller's contract.
    match unsafe { ether_resolve(ifp, m, dst, rt, &mut eh) } {
        Ok(()) => {}
        Err(Errno::EAGAIN) => return Ok(None),
        Err(e) => return Err(e),
    }

    let Some(m) = m_prepend(
        m,
        (ETHER_ALIGN + size_of::<EtherHeader>()) as i32,
        M_DONTWAIT,
    ) else {
        return Err(Errno::ENOBUFS);
    };

    m_adj(m, ETHER_ALIGN as i32);
    // SAFETY: `m_prepend` left the header's bytes at `m_data`; an unaligned write needs no
    // alignment.
    unsafe { ptr::write_unaligned(mtod::<EtherHeader>(m), eh) };

    Ok(Some(m))
}

/// `ether_output`: an Ethernet interface's `if_output`: encapsulate and enqueue.
///
/// # Safety
///
/// As for `if_output` (`IfOutputFn`).
pub unsafe fn ether_output(
    ifp: &'static Ifnet,
    m: &'static Mbuf,
    dst: *const Sockaddr,
    rt: Option<&'static Rtentry>,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let Some(m) = (unsafe { ether_encap(ifp, m, dst, rt) })? else {
        return Ok(());
    };

    if_enqueue(ifp, m)
}

/// `ether_port_input`: offers the frame to the port installed at `epp` (if any), which
/// returns it when it does not take it.
fn ether_port_input(
    ifp: &'static Ifnet,
    m: &'static Mbuf,
    dst: u64,
    epp: &AtomicPtr<EtherPort>,
    ns: Option<&Netstack>,
) -> Option<&'static Mbuf> {
    // smr_read_enter(): no SMR; the port stays installed while its reference is held.
    let ep = epp.load(Ordering::Acquire);
    // SAFETY: an installed port is a `&'static EtherPort` (`ether_brport_set`).
    let Some(ep) = (unsafe { ep.as_ref() }) else {
        return Some(m);
    };
    let reference = (ep.ep_port_take)(ep.ep_port);
    // smr_read_leave()
    let m = (ep.ep_input)(ifp, m, dst, ep.ep_port, ns);
    (ep.ep_port_rele)(reference, ep.ep_port);

    m
}

/// The frame's Ethernet header, copied out of the mbuf (it may be unaligned).
fn ether_header(m: &Mbuf) -> EtherHeader {
    // SAFETY: the caller checked `m_len >= ETHER_HDR_LEN`, so the header's bytes are at
    // `m_data`.
    unsafe { ptr::read_unaligned(mtod::<EtherHeader>(m)) }
}

/// `ether_input`: process a received Ethernet packet. Ethernet input has several "phases" of
/// filtering packets to support virtual/pseudo interfaces before actual layer 3 protocol
/// handling.
pub fn ether_input(ifp: &'static Ifnet, m: &'static Mbuf, ns: Option<&Netstack>) {
    let ac = arpcom_of(ifp);
    let mut sdelim = false;
    let mut m = m;

    'dropanyway: {
        // Drop short frames
        if (m.m_len().get() as usize) < ETHER_HDR_LEN {
            break 'dropanyway;
        }

        let eh = ether_header(m);
        let dst = ether_addr_to_e64(&eh.ether_dhost);

        // First phase:
        //
        // The first phase supports drivers that aggregate multiple Ethernet ports into a
        // single logical interface, ie, aggr(4) and trunk(4).
        m = match ether_port_input(ifp, m, dst, &ac.ac_trport, ns) {
            Some(m) => m,
            None => return,
        };

        // Second phase: service delimited packet filtering.
        //
        // Let vlan(4) and svlan(4) look at "service delimited" packets. If a virtual
        // interface does not exist to take those packets, they're returned to ether_input()
        // so a bridge can have a go at forwarding them.

        let etype = ntohs(eh.ether_type);

        if m.m_flags().get() & M_VLANTAG != 0 || etype == ETHERTYPE_VLAN || etype == ETHERTYPE_QINQ
        {
            // NVLAN > 0: m = vlan_input(ifp, m, &sdelim, ns); vlan(4) is not configured.
            sdelim = true;
        }

        // Third phase: bridge processing.
        //
        // Give the packet to a bridge interface, ie, bridge(4), veb(4), or tpmr(4), if it is
        // configured. A bridge may take the packet and forward it to another port, or it may
        // return it here to ether_input() to support local delivery to this port.
        m = match ether_port_input(ifp, m, dst, &ac.ac_brport, ns) {
            Some(m) => m,
            None => return,
        };

        let input: Option<IfInputFn> = 'drop: {
            // Fourth phase: drop service delimited packets.
            //
            // If the packet has a tag, and a bridge didn't want it, it's not for this port.

            if sdelim {
                break 'drop None;
            }

            // Fifth phase: destination address check.
            //
            // Is the packet specifically addressed to this port?

            let eh = ether_header(m);
            let self_ = ether_addr_to_e64(&ac.ac_enaddr.get());
            if dst != self_ {
                // NCARP > 0: if it's not for this port, it could be for carp(4)
                // (carp_input on an IFT_ETHER with carp interfaces); not configured.

                // If not, it must be multicast or broadcast to go further.
                if !eth64_is_multicast(dst) {
                    break 'drop None;
                }

                // If this is not a simplex interface, drop the packet if it came from us.
                if ifp.if_flags.get() & IFF_SIMPLEX == 0 {
                    let src = ether_addr_to_e64(&eh.ether_shost);
                    if self_ == src {
                        break 'drop None;
                    }
                }

                m.m_flags().set(
                    m.m_flags().get()
                        | if eth64_is_broadcast(dst) {
                            M_BCAST
                        } else {
                            M_MCAST
                        },
                );
                ifp.if_imcasts().set(ifp.if_imcasts().get() + 1);
            }

            // Sixth phase: protocol demux.
            //
            // At this point it is known that the packet is destined for layer 3 protocol
            // handling on the local port.
            let etype = ntohs(eh.ether_type);

            match etype {
                ETHERTYPE_IP => {
                    // input = ipv4_input: netinet/ip_input.c is not ported.
                    let _ = unported!("ipv4_input");
                    None
                }

                ETHERTYPE_ARP => {
                    if ifp.if_flags.get() & IFF_NOARP != 0 {
                        break 'drop None;
                    }
                    // input = arpinput: netinet/if_ether.c is not ported.
                    let _ = unported!("arpinput");
                    None
                }

                ETHERTYPE_REVARP => {
                    if ifp.if_flags.get() & IFF_NOARP != 0 {
                        break 'drop None;
                    }
                    // input = revarpinput: netinet/if_ether.c is not ported.
                    let _ = unported!("revarpinput");
                    None
                }

                // INET6: ETHERTYPE_IPV6 goes to ipv6_input; IPv6 is not configured.
                // NPPPOE > 0 || PIPEX: ETHERTYPE_PPPOEDISC/ETHERTYPE_PPPOE go to the pppoe
                // queues or a pipex session; not configured.
                // MPLS: ETHERTYPE_MPLS(_MCAST) go to mpls_input; not configured.
                // NBPE > 0: ETHERTYPE_PBB goes to bpe_input; not configured.
                _ => {
                    // NAF_FRAME > 0: m = ether_frm_input(ifp, m, dst, etype); frame sockets
                    // are not configured.
                    None
                }
            }
        };

        let Some(input) = input else {
            break 'dropanyway;
        };

        m_adj(m, size_of::<EtherHeader>() as i32);
        if_input_proto(ifp, m, input, ns);
        return;
    }

    // dropanyway:
    m_freem(m);
}

/// `ether_brport_isset`: `EBUSY` when a bridge port is installed.
pub fn ether_brport_isset(ifp: &Ifnet) -> Result<(), Errno> {
    let ac = arpcom_of(ifp);

    // KERNEL_ASSERT_LOCKED(): one CPU.
    if !ac.ac_brport.load(Ordering::Relaxed).is_null() {
        return Err(Errno::EBUSY);
    }

    Ok(())
}

/// `ether_brport_set`: installs a bridge port on the interface.
pub fn ether_brport_set(ifp: &Ifnet, ep: &'static EtherPort) {
    let ac = arpcom_of(ifp);

    // KERNEL_ASSERT_LOCKED(): one CPU.
    if !ac.ac_brport.load(Ordering::Relaxed).is_null() {
        panic(format_args!(
            "{} setting an already set brport",
            Str(&ifp.if_xname.get())
        ));
    }

    ac.ac_brport
        .store(ptr::from_ref(ep).cast_mut(), Ordering::Release);
}

/// `ether_brport_clr`: removes the bridge port.
pub fn ether_brport_clr(ifp: &Ifnet) {
    let ac = arpcom_of(ifp);

    // KERNEL_ASSERT_LOCKED(): one CPU.
    if ac.ac_brport.load(Ordering::Relaxed).is_null() {
        panic(format_args!(
            "{} clearing an already clear brport",
            Str(&ifp.if_xname.get())
        ));
    }

    ac.ac_brport.store(ptr::null_mut(), Ordering::Release);
}

/// `ether_brport_get`: the bridge port, inside an SMR read section in C.
pub fn ether_brport_get(ifp: &Ifnet) -> Option<&'static EtherPort> {
    let ac = arpcom_of(ifp);
    // SMR_ASSERT_CRITICAL(): no SMR.
    // SAFETY: an installed port is a `&'static EtherPort` (`ether_brport_set`).
    unsafe { ac.ac_brport.load(Ordering::Acquire).as_ref() }
}

/// `ether_brport_get_locked`: the bridge port, with the kernel lock held.
pub fn ether_brport_get_locked(ifp: &Ifnet) -> Option<&'static EtherPort> {
    let ac = arpcom_of(ifp);
    // KERNEL_ASSERT_LOCKED(): one CPU.
    // SAFETY: as above.
    unsafe { ac.ac_brport.load(Ordering::Relaxed).as_ref() }
}

/// `ether_sprintf`: convert Ethernet address to printable (loggable) representation,
/// `xx:xx:xx:xx:xx:xx` and a NUL.
pub fn ether_sprintf(ap: &[u8; ETHER_ADDR_LEN]) -> [u8; ETHER_ADDR_LEN * 3] {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut etherbuf = [0u8; ETHER_ADDR_LEN * 3];

    for (i, &b) in ap.iter().enumerate() {
        etherbuf[i * 3] = DIGITS[usize::from(b >> 4)];
        etherbuf[i * 3 + 1] = DIGITS[usize::from(b & 0xf)];
        etherbuf[i * 3 + 2] = b':';
    }
    etherbuf[ETHER_ADDR_LEN * 3 - 1] = 0;
    etherbuf
}

/// `ether_fakeaddr`: generate a (hopefully) acceptable MAC address, if asked.
pub fn ether_fakeaddr(ifp: &Ifnet) {
    let rng = arc4random();
    let unit = FAKEADDR_UNIT.fetch_add(1, Ordering::Relaxed);

    // Non-multicast; locally administered address
    arpcom_of(ifp).ac_enaddr.set([
        0xfe,
        0xe1,
        0xba,
        0xd0 | (unit & 0xf) as u8,
        rng as u8,
        (rng >> 8) as u8,
    ]);
}

/// `ether_ifattach`: perform common duties while attaching to interface list: the Ethernet
/// type, lengths and MTU, `ether_input`/`ether_output`, the link address.
pub fn ether_ifattach(ac: &'static Arpcom) {
    let ifp = &ac.ac_if;
    // SAFETY: `ifp` is the `ac_if` of `ac`, which is `'static`.
    unsafe { ifp.set_arpcom() };

    // Any interface which provides a MAC address which is obviously invalid gets whacked, so
    // that users will notice.
    if ether_is_multicast(&ac.ac_enaddr.get()) {
        ether_fakeaddr(ifp);
    }

    ifp.if_type.set(IFT_ETHER);
    ifp.if_addrlen.set(ETHER_ADDR_LEN as u8);
    ifp.if_hdrlen.set(ETHER_HDR_LEN as u8);
    ifp.if_mtu.set(ETHERMTU as u32);
    ifp.if_input.set(Some(ether_input));
    if ifp.if_output.get().is_none() {
        ifp.if_output.set(Some(ether_output));
    }
    ifp.if_rtrequest.set(Some(ether_rtrequest));

    if ifp.if_hardmtu.get() == 0 {
        ifp.if_hardmtu.set(ETHERMTU as u32);
    }

    if_alloc_sadl(ifp);
    // SAFETY: `if_alloc_sadl` made room for `if_addrlen` bytes after the name.
    unsafe {
        ptr::copy_nonoverlapping(
            ac.ac_enaddr.get().as_ptr(),
            lladdr(ifp.if_sadl.get()),
            usize::from(ifp.if_addrlen.get()),
        );
    }
    ac.ac_multiaddrs.init();
    // NBPFILTER > 0: bpfattach(&ifp->if_bpf, ifp, DLT_EN10MB, ETHER_HDR_LEN); not configured.
}

/// `ether_ifdetach`: undo pseudo-driver changes and free the multicast list.
pub fn ether_ifdetach(ifp: &Ifnet) {
    let ac = arpcom_of(ifp);

    // Undo pseudo-driver changes.
    if_deactivate(ifp);

    while let Some(enm) = ac.ac_multiaddrs.first() {
        // SAFETY: `enm` is on the list; it was `malloc`ed by `ether_addmulti`.
        unsafe { ListHead::<EtherMultiList>::remove(enm) };
        free(
            NonNull::from(enm).cast(),
            M_IFMADDR,
            size_of::<EtherMulti>(),
        );
    }
}

/// `ether_crc32_le_update`: the little-endian Ethernet CRC of `buf`, from `crc`, a nibble at
/// a time.
pub fn ether_crc32_le_update(mut crc: u32, buf: &[u8]) -> u32 {
    for &b in buf {
        crc ^= u32::from(b);
        crc = (crc >> 4) ^ CRCTAB_LE[(crc & 0xf) as usize];
        crc = (crc >> 4) ^ CRCTAB_LE[(crc & 0xf) as usize];
    }

    crc
}

/// `ether_crc32_be_update`: the big-endian Ethernet CRC of `buf`, from `crc`.
pub fn ether_crc32_be_update(mut crc: u32, buf: &[u8]) -> u32 {
    for &data in buf {
        crc = (crc << 4)
            ^ CRCTAB_BE[((crc >> 28) ^ u32::from(REV[usize::from(data & 0xf)])) as usize];
        crc =
            (crc << 4) ^ CRCTAB_BE[((crc >> 28) ^ u32::from(REV[usize::from(data >> 4)])) as usize];
    }

    crc
}

/// `ether_crc32_le`.
pub fn ether_crc32_le(buf: &[u8]) -> u32 {
    ether_crc32_le_update(0xffff_ffff, buf)
}

/// `ether_crc32_be`.
pub fn ether_crc32_be(buf: &[u8]) -> u32 {
    ether_crc32_be_update(0xffff_ffff, buf)
}

/// The little-endian nibble table: entry `i` is the CRC register after shifting nibble `i`
/// out with the reflected polynomial.
const fn crctab_le() -> [u32; 16] {
    let mut tab = [0u32; 16];
    let mut i = 0;
    while i < 16 {
        let mut c = i as u32;
        let mut j = 0;
        while j < 4 {
            c = if c & 1 != 0 {
                (c >> 1) ^ CRC_POLY_LE
            } else {
                c >> 1
            };
            j += 1;
        }
        tab[i] = c;
        i += 1;
    }
    tab
}

/// The big-endian nibble table: entry `i` is nibble `i` at the top of the register, shifted
/// out with the polynomial.
const fn crctab_be() -> [u32; 16] {
    let mut tab = [0u32; 16];
    let mut i = 0;
    while i < 16 {
        let mut c = (i as u32) << 28;
        let mut j = 0;
        while j < 4 {
            c = if c & 0x8000_0000 != 0 {
                (c << 1) ^ CRC_POLY_BE
            } else {
                c << 1
            };
            j += 1;
        }
        tab[i] = c;
        i += 1;
    }
    tab
}

/// Each nibble with its four bits reversed.
const fn nibble_rev() -> [u8; 16] {
    let mut tab = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        let n = i as u8;
        tab[i] = ((n & 1) << 3) | ((n & 2) << 1) | ((n & 4) >> 1) | ((n & 8) >> 3);
        i += 1;
    }
    tab
}

/// `ether_multiaddr`: convert a sockaddr into an Ethernet address or range of Ethernet
/// addresses: `(addrlo, addrhi)`.
///
/// # Safety
///
/// `sa` points at a readable socket address of its family's size.
pub unsafe fn ether_multiaddr(
    sa: *const Sockaddr,
) -> Result<([u8; ETHER_ADDR_LEN], [u8; ETHER_ADDR_LEN]), Errno> {
    // SAFETY: the caller's contract.
    match unsafe { (*sa).sa_family } {
        AF_UNSPEC => {
            let mut addrlo = [0u8; ETHER_ADDR_LEN];
            // SAFETY: `sa_data` is fourteen readable bytes.
            let data = unsafe { (*sa).sa_data };
            addrlo.copy_from_slice(&data[..ETHER_ADDR_LEN]);
            Ok((addrlo, addrlo))
        }

        AF_INET => {
            // SAFETY: an `AF_INET` sockaddr is a `sockaddr_in`, as long as a `sockaddr`.
            let sin = unsafe { &*sa.cast::<SockaddrIn>() };
            if sin.sin_addr.s_addr == INADDR_ANY {
                // An IP address of INADDR_ANY means listen to or stop listening to all of the
                // Ethernet multicast addresses used for IP. (This is for the sake of IP
                // multicast routers.)
                Ok((ETHER_IPMULTICAST_MIN, ETHER_IPMULTICAST_MAX))
            } else {
                let addrlo = ether_map_ip_multicast(&sin.sin_addr);
                Ok((addrlo, addrlo))
            }
        }
        // INET6: an AF_INET6 address maps with ETHER_MAP_IPV6_MULTICAST (all of 33:33:* for
        // the unspecified address); IPv6 is not configured.
        _ => Err(Errno::EAFNOSUPPORT),
    }
}

/// `ether_addmulti`: add an Ethernet multicast address or range of addresses to the list for
/// a given interface. `Err(ENETRESET)` tells the driver that the list has changed and its
/// reception filter should be adjusted accordingly.
pub fn ether_addmulti(ifr: &Ifreq, ac: &Arpcom) -> Result<(), Errno> {
    let s = splnet();

    // SAFETY: `ifr_addr` is a `struct sockaddr` inside the request.
    let (addrlo, addrhi) = match unsafe { ether_multiaddr(ifr.ifr_addr()) } {
        Ok(range) => range,
        Err(e) => {
            splx(s);
            return Err(e);
        }
    };

    // Verify that we have valid Ethernet multicast addresses.
    if addrlo[0] & 0x01 != 1 || addrhi[0] & 0x01 != 1 {
        splx(s);
        return Err(Errno::EINVAL);
    }
    // See if the address range is already in the list.
    if let Some(enm) = ether_lookup_multi(&addrlo, &addrhi, ac) {
        // Found it; just increment the reference count.
        refcnt_take(&enm.enm_refcnt);
        splx(s);
        return Ok(());
    }
    // New address or range; malloc a new multicast record and link it into the interface's
    // multicast list.
    let Some(enm) = malloc(size_of::<EtherMulti>(), M_IFMADDR, M_NOWAIT) else {
        splx(s);
        return Err(Errno::ENOBUFS);
    };
    let enm = enm.cast::<EtherMulti>();
    // SAFETY: a fresh, aligned block of the record's size, written once before use.
    unsafe {
        enm.as_ptr().write(EtherMulti {
            enm_addrlo: addrlo,
            enm_addrhi: addrhi,
            enm_refcnt: Refcnt::new(),
            enm_list: crate::sys::queue::ListEntry::new(),
        })
    };
    // SAFETY: initialised above; it lives until `ether_delmulti` or `ether_ifdetach` frees it.
    let enm: &'static EtherMulti = unsafe { enm.as_ref() };
    refcnt_init_trace(&enm.enm_refcnt, DT_REFCNT_IDX_ETHMULTI);
    // SAFETY: a new record, on no list.
    unsafe { ac.ac_multiaddrs.insert_head(enm) };
    ac.ac_multicnt.set(ac.ac_multicnt.get() + 1);
    if addrlo != addrhi {
        ac.ac_multirangecnt.set(ac.ac_multirangecnt.get() + 1);
    }
    splx(s);
    // Return ENETRESET to inform the driver that the list has changed and its reception
    // filter should be adjusted accordingly.
    Err(Errno::ENETRESET)
}

/// `ether_delmulti`: delete a multicast address record; `Err(ENETRESET)` when it went away.
pub fn ether_delmulti(ifr: &Ifreq, ac: &Arpcom) -> Result<(), Errno> {
    let s = splnet();

    // SAFETY: `ifr_addr` is a `struct sockaddr` inside the request.
    let (addrlo, addrhi) = match unsafe { ether_multiaddr(ifr.ifr_addr()) } {
        Ok(range) => range,
        Err(e) => {
            splx(s);
            return Err(e);
        }
    };

    // Look up the address in our list.
    let Some(enm) = ether_lookup_multi(&addrlo, &addrhi, ac) else {
        splx(s);
        return Err(Errno::ENXIO);
    };
    if !refcnt_rele(&enm.enm_refcnt) {
        // Still some claims to this record.
        splx(s);
        return Ok(());
    }
    // No remaining claims to this record; unlink and free it.
    // SAFETY: `enm` is on the list; it was `malloc`ed by `ether_addmulti`.
    unsafe { ListHead::<EtherMultiList>::remove(enm) };
    free(
        NonNull::from(enm).cast(),
        M_IFMADDR,
        size_of::<EtherMulti>(),
    );
    ac.ac_multicnt.set(ac.ac_multicnt.get() - 1);
    if addrlo != addrhi {
        ac.ac_multirangecnt.set(ac.ac_multirangecnt.get() - 1);
    }
    splx(s);
    // Return ENETRESET to inform the driver that the list has changed and its reception
    // filter should be adjusted accordingly.
    Err(Errno::ENETRESET)
}

/// `ether_addr_to_e64`: the address as a 48-bit integer, first octet highest.
pub fn ether_addr_to_e64(ea: &[u8; ETHER_ADDR_LEN]) -> u64 {
    ea.iter().fold(0, |e64, &b| (e64 << 8) | u64::from(b))
}

/// `ether_e64_to_addr`: the inverse of `ether_addr_to_e64`.
pub fn ether_e64_to_addr(ea: &mut [u8; ETHER_ADDR_LEN], mut e64: u64) {
    for b in ea.iter_mut().rev() {
        *b = e64 as u8;
        e64 >>= 8;
    }
}

/// `ether_extract_headers`: parse different TCP/IP protocol headers for a quick view inside
/// an mbuf. Members stay NULL for headers that were not recognized.
pub fn ether_extract_headers(m0: &Mbuf, ext: &mut EtherExtracted) {
    *ext = EtherExtracted::new();

    kassert!(m0.m_flags().get() & M_PKTHDR != 0);
    ext.paylen = m0.m_pkthdr().len.get() as u32;

    let m0_len = m0.m_len().get() as usize;
    if m0_len < size_of::<EtherHeader>() {
        return;
    }
    ext.eh = mtod::<EtherHeader>(m0);
    let mut hlen = size_of::<EtherHeader>();
    if (ext.paylen as usize) < hlen {
        ext.eh = ptr::null_mut();
        return;
    }
    ext.paylen -= hlen as u32;
    // SAFETY: `m0_len` covers the Ethernet header at `m_data`.
    let ether_type = ntohs(unsafe { ptr::read_unaligned(ext.eh) }.ether_type);

    // NVLAN > 0: an ETHERTYPE_VLAN frame is looked at through its ether_vlan_header (evh);
    // vlan(4) is not configured.

    let (m, hoff, ipproto) = match ether_type {
        ETHERTYPE_IP => {
            let Some((m, hoff)) = m_getptr(m0, hlen as i32) else {
                return;
            };
            let hoff = hoff as usize;
            let m_len = m.m_len().get() as usize;
            if m_len - hoff < size_of::<Ip>() {
                return;
            }
            let ip4 = mtod::<u8>(m).wrapping_add(hoff).cast::<Ip>();
            ext.ip4 = ip4;

            // SAFETY: `m_len - hoff` covers an IP header at `ip4`.
            let ip = unsafe { ptr::read_unaligned(ip4) };
            hlen = usize::from(ip.ip_hl()) << 2;
            if m_len - hoff < hlen {
                ext.ip4 = ptr::null_mut();
                return;
            }
            if (ext.paylen as usize) < hlen {
                ext.ip4 = ptr::null_mut();
                return;
            }
            let iplen = usize::from(ntohs(ip.ip_len));
            if (ext.paylen as usize) < iplen {
                ext.ip4 = ptr::null_mut();
                return;
            }
            if iplen < hlen {
                ext.ip4 = ptr::null_mut();
                return;
            }
            ext.iplen = iplen as u32;
            ext.iphlen = hlen as u32;
            ext.paylen -= hlen as u32;
            let ipproto = i32::from(ip.ip_p);

            if ntohs(ip.ip_off) & (IP_MF | IP_OFFMASK) != 0 {
                return;
            }
            (m, hoff, ipproto)
        }
        // INET6: an ETHERTYPE_IPV6 frame's ip6_hdr; IPv6 is not configured.
        _ => return,
    };

    match ipproto {
        IPPROTO_TCP => {
            let Some((m, hoff)) = m_getptr(m, (hoff + hlen) as i32) else {
                return;
            };
            let hoff = hoff as usize;
            let m_len = m.m_len().get() as usize;
            if m_len - hoff < TCPHDR_LEN {
                return;
            }
            let tcp = mtod::<u8>(m).wrapping_add(hoff);
            ext.tcp = tcp;

            // SAFETY: `m_len - hoff` covers a TCP header at `tcp`.
            let off = unsafe { tcp.add(TCPHDR_OFF_BYTE).read() } >> 4;
            let thlen = usize::from(off) << 2;
            if m_len - hoff < thlen {
                ext.tcp = ptr::null_mut();
                return;
            }
            if ((ext.iplen - ext.iphlen) as usize) < thlen {
                ext.tcp = ptr::null_mut();
                return;
            }
            ext.tcphlen = thlen as u32;
            ext.paylen -= thlen as u32;
        }

        IPPROTO_UDP => {
            let Some((m, hoff)) = m_getptr(m, (hoff + hlen) as i32) else {
                return;
            };
            let hoff = hoff as usize;
            if (m.m_len().get() as usize) - hoff < UDPHDR_LEN {
                return;
            }
            ext.udp = mtod::<u8>(m).wrapping_add(hoff);

            if ((ext.iplen - ext.iphlen) as usize) < UDPHDR_LEN {
                ext.udp = ptr::null_mut();
            }
        }
        _ => {}
    }
}

/// `ether_offload_ifcap`: does in software the checksums (and VLAN tag) the interface cannot
/// offload; NULL when the packet was lost.
pub fn ether_offload_ifcap(ifp: &Ifnet, m: &'static Mbuf) -> Option<&'static Mbuf> {
    let mut m = m;
    let mut ext = EtherExtracted::new();
    let caps = ifp.if_capabilities.get();
    let csum_flags = m.m_pkthdr().csum_flags.get();
    let mut csum = false;

    // NVLAN > 0: an M_VLANTAG packet on an interface without IFCAP_VLAN_HWTAGGING gets its tag
    // injected (vlan_inject); vlan(4) is not configured.

    if csum_flags & M_IPV4_CSUM_OUT != 0 && caps & IFCAP_CSUM_IPv4 == 0 {
        csum = true;
    }

    if csum_flags & M_TCP_CSUM_OUT != 0
        && (caps & IFCAP_CSUM_TCPv4 == 0 || caps & IFCAP_CSUM_TCPv6 == 0)
    {
        csum = true;
    }

    if csum_flags & M_UDP_CSUM_OUT != 0
        && (caps & IFCAP_CSUM_UDPv4 == 0 || caps & IFCAP_CSUM_UDPv6 == 0)
    {
        csum = true;
    }

    if csum {
        ether_extract_headers(m, &mut ext);

        let mut ethlen = size_of::<EtherHeader>();
        if !ext.evh.is_null() {
            ethlen = size_of::<EtherVlanHeader>();
        }

        let hlen = m.m_pkthdr().len.get() as u32 - ext.paylen;

        if m.m_len().get() < hlen {
            m = m_pullup(m, hlen as i32)?;
        }

        // hide ethernet header
        m.m_data().set(m.m_data().get().wrapping_add(ethlen));
        m.m_len().set(m.m_len().get() - ethlen as u32);
        m.m_pkthdr().len.set(m.m_pkthdr().len.get() - ethlen as i32);

        if !ext.ip4.is_null() {
            // in_hdr_cksum_out(m, ifp), in_proto_cksum_out(m, ifp): netinet/ip_output.c is
            // not ported.
            let _ = unported!("in_hdr_cksum_out");
            let _ = unported!("in_proto_cksum_out");
        }
        // INET6: in6_proto_cksum_out(m, ifp) for an IPv6 packet; not configured.

        // show ethernet header again
        m.m_data().set(m.m_data().get().wrapping_sub(ethlen));
        m.m_len().set(m.m_len().get() + ethlen as u32);
        m.m_pkthdr().len.set(m.m_pkthdr().len.get() + ethlen as i32);
    }

    Some(m)
}

// NAF_FRAME > 0: the frame sockets (struct ether_pcb, ether_pcb_group, ether_frm_usrreqs,
// ether_frm_attach/detach/bind/connect/disconnect/shutdown/send/sockaddr/peeraddr,
// ether_frm_group, the FRAME_* socket options, ether_frm_ctloutput, ether_frm_recv and
// ether_frm_input) need sockets, which are not ported; af_frame is not configured.

const _: () = {
    assert!(size_of::<EtherHeader>() == 14);
    assert!(CRCTAB_LE[1] == 0x1db7_1064);
    assert!(CRCTAB_BE[1] == 0x04c1_1db7);
};

#[cfg(test)]
mod tests;
