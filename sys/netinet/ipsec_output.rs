/*	$OpenBSD: ipsec_output.c,v 1.105 2025/07/08 00:47:41 jsg Exp $ */
/* <LICENSES> */
/*
 * The author of this code is Angelos D. Keromytis (angelos@cis.upenn.edu)
 *
 * Copyright (c) 2000-2001 Angelos D. Keromytis.
 *
 * Permission to use, copy, and modify this software with or without fee
 * is hereby granted, provided that this entire notice is included in
 * all copies of any software which is or includes a copy or
 * modification of this software.
 * You may use this code under the GNU public license if you so wish. Please
 * contribute changes back to the authors under this freer than GPL license
 * so that we may further the use of strong encryption without limitations to
 * all.
 *
 * THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR
 * IMPLIED WARRANTY. IN PARTICULAR, NONE OF THE AUTHORS MAKES ANY
 * REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE
 * MERCHANTABILITY OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR
 * PURPOSE.
 */
/* </LICENSES> */

//! IPsec output processing: `netinet/ipsec_output.c`. `ip_output` hands a packet that a
//! policy wants protected to `ipsp_process_packet`, which encapsulates it in IP when the SA
//! is a tunnel and calls the transform; the transform's callback `ipsp_process_done` adds the
//! UDP encapsulation if asked, tags the packet with the SA, applies the next SA of a bundle
//! or sends the result through `ip_output` again.
//!
//! Upstream: sys/netinet/ipsec_output.c @ 3ce1f3f79392
//!
//! Status: `ported` (M9c).
//!
//! ## Deviations
//! - The packet is `&'static Mbuf` (consumed on every path, as in C); IP headers are read and
//!   written as copies (`mtod_ip`/`mtod_ip_store`). Errors are `Result`s.
//! - `udpencap_enable`/`udpencap_port` are `AtomicI32`s.
//! - Not configured, each a comment at its site: `INET6` (the IPv6 header handling and
//!   `ip6_output`). `NPF` (pf(4)) is configured: `pf_tag_packet`, `pf_pkt_addr_changed`.
//! - `KERNEL_ASSERT_LOCKED()` is nothing without `MULTIPROCESSOR`.

use core::mem::size_of;
use core::ptr;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_tc::gettime;
use crate::kern::kern_timeout::timeout_add_sec;
use crate::kern::uipc_mbuf::{m_freem, m_makespace, m_pullup};
use crate::kern::uipc_mbuf2::{m_tag_find, m_tag_get, m_tag_prepend};
use crate::net::pf::{pf_pkt_addr_changed, pf_tag_packet};
use crate::netinet::in_::{
    INADDR_ANY, IPPROTO_AH, IPPROTO_ESP, IPPROTO_IPCOMP, IPPROTO_IPIP, IPPROTO_UDP,
};
use crate::netinet::in_cksum::in_cksum;
use crate::netinet::ip::{IP_DF, Ip};
use crate::netinet::ip_ah::AH_FLENGTH;
use crate::netinet::ip_esp::{EspstatCounters, espstat_inc};
use crate::netinet::ip_input::IP_MTUDISC_TIMEOUT;
use crate::netinet::ip_ipcomp::{IpcompCounters, ipcompstat_inc};
use crate::netinet::ip_ipip::ipip_output;
use crate::netinet::ip_ipsp::{
    IPSP_DF_INHERIT, IPSP_DF_OFF, IPSP_DF_ON, IpsecCounters, TDBF_FIRSTUSE, TDBF_INVALID,
    TDBF_SOFT_FIRSTUSE, TDBF_TUNNELING, TDBF_UDPENCAP, TDBF_USEDTUNNEL, Tdb, TdbCounters, TdbIdent,
    XF_IP4, gettdb, ipsecstat_add, ipsecstat_pkt, ipsp_address, tdb_ref, tdb_unref, tdbstat_add,
    tdbstat_pkt,
};
use crate::netinet::ip_output::ip_output;
use crate::netinet::ip_var::{IP_RAWOUTPUT, mtod_ip, mtod_ip_store};
use crate::netinet::ipsec_input::{AH_ENABLE, ESP_ENABLE, IPCOMP_ENABLE};
use crate::netinet::udp::Udphdr;
use crate::sys::endian::{htons, ntohl};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::mbuf::{Mbuf, PACKET_TAG_IPSEC_OUT_DONE, mtod};
use crate::sys::socket::AF_INET;
use crate::sys::systm::net_assert_locked;

/// \[a\] `udpencap_enable`: enabled by default.
pub static UDPENCAP_ENABLE: AtomicI32 = AtomicI32::new(1);
/// \[a\] `udpencap_port`: triggers decapsulation.
pub static UDPENCAP_PORT: AtomicI32 = AtomicI32::new(4500);

/// `ipsp_process_packet`: loop over a tdb chain, taking into consideration protocol
/// tunneling. `tunalready` is set if the first encapsulation header is already in place.
pub fn ipsp_process_packet(
    m: &'static Mbuf,
    tdb: &'static Tdb,
    af: i32,
    tunalready: bool,
    setdf: i32,
) -> Result<(), Errno> {
    let mut m = m;
    let mut setdf = setdf;

    let error: Errno = 'drop: {
        // Check that the transform is allowed by the administrator.
        let sproto = i32::from(tdb.tdb_sproto.get());
        if (sproto == IPPROTO_ESP && ESP_ENABLE.load(Ordering::Relaxed) == 0)
            || (sproto == IPPROTO_AH && AH_ENABLE.load(Ordering::Relaxed) == 0)
            || (sproto == IPPROTO_IPCOMP && IPCOMP_ENABLE.load(Ordering::Relaxed) == 0)
        {
            crate::ipsec_dprintf!(
                "ipsp_process_packet",
                "IPsec outbound packet dropped due to policy (check your sysctls)"
            );
            break 'drop Errno::EHOSTUNREACH;
        }

        // Sanity check.
        let Some(xf) = tdb.tdb_xform.get() else {
            crate::ipsec_dprintf!("ipsp_process_packet", "uninitialized TDB");
            break 'drop Errno::EHOSTUNREACH;
        };

        // Check if the SPI is invalid.
        if tdb.has_flags(TDBF_INVALID) {
            crate::ipsec_dprintf!(
                "ipsp_process_packet",
                "attempt to use invalid SA {}/{:08x}/{}",
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get()),
                tdb.tdb_sproto.get()
            );
            break 'drop Errno::ENXIO;
        }

        // Check that the network protocol is supported
        let dst = tdb.tdb_dst.get();
        match dst.sa_family() {
            AF_INET => {}
            // INET6: AF_INET6; not configured.
            _ => {
                crate::ipsec_dprintf!(
                    "ipsp_process_packet",
                    "attempt to use SA {}/{:08x}/{} for protocol family {}",
                    ipsp_address(&dst),
                    ntohl(tdb.tdb_spi.get()),
                    tdb.tdb_sproto.get(),
                    dst.sa_family()
                );
                break 'drop Errno::EPFNOSUPPORT;
            }
        }

        // Register first use if applicable, setup relevant expiration timer.
        if tdb.tdb_first_use.get() == 0 {
            tdb.tdb_first_use.set(gettime() as u64);
            if tdb.has_flags(TDBF_FIRSTUSE)
                && timeout_add_sec(&tdb.tdb_first_tmo, tdb.tdb_exp_first_use.get() as i32)
            {
                tdb_ref(Some(tdb));
            }
            if tdb.has_flags(TDBF_SOFT_FIRSTUSE)
                && timeout_add_sec(&tdb.tdb_sfirst_tmo, tdb.tdb_soft_first_use.get() as i32)
            {
                tdb_ref(Some(tdb));
            }
        }

        // Check for tunneling if we don't have the first header in place. When doing
        // Ethernet-over-IP, we are handed an already-encapsulated frame, so we don't need to
        // re-encapsulate.
        if !tunalready {
            let mut ip_dst = None;
            // If the target protocol family is different, we know we'll be doing tunneling.
            if af == i32::from(dst.sa_family()) {
                let hlen = match af {
                    x if x == i32::from(AF_INET) => size_of::<Ip>() as i32,
                    // INET6: sizeof(struct ip6_hdr); not configured.
                    _ => 0,
                };

                // Bring the network header in the first mbuf.
                if (m.m_len().get() as i32) < hlen {
                    match m_pullup(m, hlen) {
                        Some(mm) => m = mm,
                        None => return Err(Errno::ENOBUFS),
                    }
                }

                if af == i32::from(AF_INET) {
                    let ip = mtod_ip(m);

                    // This is not a bridge packet, remember if we had IP_DF.
                    if setdf == IPSP_DF_INHERIT {
                        setdf = if ip.ip_off & htons(IP_DF) != 0 {
                            IPSP_DF_ON
                        } else {
                            IPSP_DF_OFF
                        };
                    }
                    ip_dst = Some(ip.ip_dst);
                }

                // INET6: ip6 = mtod(m, struct ip6_hdr *); not configured.
            }

            // Do the appropriate encapsulation, if necessary.
            if i32::from(dst.sa_family()) != af // PF mismatch
                || tdb.has_flags(TDBF_TUNNELING) // Tunneling needed
                || xf.xf_type == XF_IP4 // ditto
                || (dst.sa_family() == AF_INET
                    && dst.sin_addr().s_addr != INADDR_ANY
                    && ip_dst.is_some_and(|d| dst.sin_addr().s_addr != d.s_addr))
            // INET6: an IPv6 SA destination other than the packet's; not configured.
            {
                // Fix IPv4 header checksum and length.
                if af == i32::from(AF_INET) {
                    if (m.m_len().get() as usize) < size_of::<Ip>() {
                        match m_pullup(m, size_of::<Ip>() as i32) {
                            Some(mm) => m = mm,
                            None => return Err(Errno::ENOBUFS),
                        }
                    }

                    let mut ip = mtod_ip(m);
                    ip.ip_len = htons(m.m_pkthdr().len.get() as u16);
                    ip.ip_sum = 0;
                    mtod_ip_store(m, &ip);
                    ip.ip_sum = in_cksum(m, i32::from(ip.ip_hl()) << 2);
                    mtod_ip_store(m, &ip);
                }

                // INET6: fix the IPv6 payload length (no jumbograms); not configured.

                // Encapsulate -- m may be changed or set to NULL.
                let mut mp = Some(m);
                let r = ipip_output(&mut mp, tdb);
                match (mp, r) {
                    (None, Ok(())) => return Err(Errno::EFAULT),
                    (None, Err(e)) => return Err(e),
                    (Some(mm), Err(e)) => {
                        m = mm;
                        break 'drop e;
                    }
                    (Some(mm), Ok(())) => m = mm,
                }

                if dst.sa_family() == AF_INET && setdf == IPSP_DF_ON {
                    if (m.m_len().get() as usize) < size_of::<Ip>() {
                        match m_pullup(m, size_of::<Ip>() as i32) {
                            Some(mm) => m = mm,
                            None => return Err(Errno::ENOBUFS),
                        }
                    }

                    let mut ip = mtod_ip(m);
                    ip.ip_off |= htons(IP_DF);
                    mtod_ip_store(m, &ip);
                }

                // Remember that we appended a tunnel header.
                mtx_enter(&tdb.tdb_mtx);
                tdb.set_flags(TDBF_USEDTUNNEL);
                mtx_leave(&tdb.tdb_mtx);
            }
        }

        // If this is just an IP-IP TDB and we're told there's already an encapsulation
        // header or ipip_output() has encapsulated it, move on.
        if xf.xf_type == XF_IP4 {
            return ipsp_process_done(m, tdb);
        }

        // Extract some information off the headers.
        let (hlen, off) = match dst.sa_family() {
            AF_INET => {
                let ip = mtod_ip(m);
                (
                    i32::from(ip.ip_hl()) << 2,
                    core::mem::offset_of!(Ip, ip_p) as i32,
                )
            }
            // INET6: chase the header chain for where to put AH/ESP/IPcomp; not configured.
            _ => break 'drop Errno::EPFNOSUPPORT,
        };

        if m.m_pkthdr().len.get() < hlen {
            break 'drop Errno::EINVAL;
        }

        ipsecstat_add(
            IpsecCounters::IpsecOuncompbytes,
            m.m_pkthdr().len.get() as u64,
        );
        tdbstat_add(
            tdb,
            TdbCounters::TdbOuncompbytes,
            m.m_pkthdr().len.get() as u64,
        );

        // Non expansion policy for IPCOMP
        if sproto == IPPROTO_IPCOMP
            && let Some(comp) = tdb.tdb_compalgxform.get()
            && ((m.m_pkthdr().len.get() - hlen) as usize) < comp.minlen
        {
            // No need to compress, leave the packet untouched
            ipcompstat_inc(IpcompCounters::IpcompsMinlen);
            return ipsp_process_done(m, tdb);
        }

        // Invoke the IPsec transform.
        let Some(output) = xf.xf_output else {
            break 'drop Errno::EHOSTUNREACH;
        };
        return output(m, tdb, hlen, off);
    };
    // drop:
    m_freem(m);
    Err(error)
}

/// `ipsp_process_done`: called by the IPsec output transform callbacks, to transmit the
/// packet or do further processing, as necessary.
pub fn ipsp_process_done(m: &'static Mbuf, tdb: &'static Tdb) -> Result<(), Errno> {
    net_assert_locked("ipsp_process_done");

    tdb.tdb_last_used.set(gettime() as u64);

    let dst = tdb.tdb_dst.get();
    let error: Errno = 'drop: {
        if tdb.has_flags(TDBF_UDPENCAP) {
            let udpencap_port_local = UDPENCAP_PORT.load(Ordering::Relaxed);

            if UDPENCAP_ENABLE.load(Ordering::Relaxed) == 0 || udpencap_port_local == 0 {
                break 'drop Errno::ENXIO;
            }

            let iphlen = match dst.sa_family() {
                AF_INET => size_of::<Ip>() as i32,
                // INET6: sizeof(struct ip6_hdr); not configured.
                _ => {
                    crate::ipsec_dprintf!(
                        "ipsp_process_done",
                        "unknown protocol family ({})",
                        dst.sa_family()
                    );
                    break 'drop Errno::EPFNOSUPPORT;
                }
            };

            let Some((mi, roff)) = m_makespace(m, iphlen, size_of::<Udphdr>() as i32) else {
                break 'drop Errno::ENOMEM;
            };
            let sport = htons(udpencap_port_local as u16);
            let dport = if tdb.tdb_udpencap_port.get() != 0 {
                tdb.tdb_udpencap_port.get()
            } else {
                sport
            };
            let ulen = htons((m.m_pkthdr().len.get() - iphlen) as u16);
            let uh = Udphdr {
                uh_sport: sport,
                uh_dport: dport,
                uh_ulen: ulen,
                uh_sum: 0,
            };
            // SAFETY: `m_makespace` made `sizeof(struct udphdr)` contiguous bytes at `roff` of
            // `mi`; written unaligned.
            unsafe { ptr::write_unaligned(mtod::<u8>(mi).add(roff as usize).cast::<Udphdr>(), uh) };
            // INET6: M_UDP_CSUM_OUT for an IPv6 SA; not configured.
            espstat_inc(EspstatCounters::EspsUdpencout);
        }

        match dst.sa_family() {
            AF_INET => {
                // Fix the header length, for AH processing.
                let mut ip = mtod_ip(m);
                ip.ip_len = htons(m.m_pkthdr().len.get() as u16);
                if tdb.has_flags(TDBF_UDPENCAP) {
                    ip.ip_p = IPPROTO_UDP as u8;
                }
                mtod_ip_store(m, &ip);
            }
            // INET6: fix ip6_plen (no jumbograms) and ip6_nxt; not configured.
            _ => {
                crate::ipsec_dprintf!(
                    "ipsp_process_done",
                    "unknown protocol family ({})",
                    dst.sa_family()
                );
                break 'drop Errno::EPFNOSUPPORT;
            }
        }

        // Add a record of what we've done or what needs to be done to the packet.
        let Some(mtag) = m_tag_get(
            PACKET_TAG_IPSEC_OUT_DONE,
            size_of::<TdbIdent>() as i32,
            M_NOWAIT,
        ) else {
            crate::ipsec_dprintf!("ipsp_process_done", "could not allocate packet tag");
            break 'drop Errno::ENOMEM;
        };

        // SAFETY: the tag was allocated with `size_of::<TdbIdent>()` bytes of data.
        unsafe { TdbIdent::of(tdb).write(mtag.data()) };

        m_tag_prepend(m, mtag);

        ipsecstat_pkt(
            IpsecCounters::IpsecOpackets,
            IpsecCounters::IpsecObytes,
            m.m_pkthdr().len.get() as u64,
        );
        tdbstat_pkt(
            tdb,
            TdbCounters::TdbOpackets,
            TdbCounters::TdbObytes,
            m.m_pkthdr().len.get() as u64,
        );

        // If there's another (bundled) TDB to apply, do so.
        if let Some(tdbo) = tdb_ref(tdb.tdb_onext.get()) {
            // KERNEL_ASSERT_LOCKED(): nothing without MULTIPROCESSOR.
            let error =
                ipsp_process_packet(m, tdbo, i32::from(dst.sa_family()), false, IPSP_DF_INHERIT);
            tdb_unref(Some(tdbo));
            return error;
        }

        // Add pf tag if requested.
        pf_tag_packet(m, i32::from(tdb.tdb_tag.get()), -1);
        pf_pkt_addr_changed(m);
        if tdb.tdb_rdomain.get() != tdb.tdb_rdomain_post.get() {
            m.m_pkthdr().ph_rtableid.set(tdb.tdb_rdomain_post.get());
        }

        // We're done with IPsec processing, transmit the packet using the appropriate network
        // protocol (IP or IPv6). SPD lookup will be performed again there.
        return match dst.sa_family() {
            AF_INET => ip_output(m, None, None, IP_RAWOUTPUT, None, None, 0),
            // INET6: ip6_output; not configured.
            _ => {
                m_freem(m);
                Err(Errno::EPFNOSUPPORT)
            }
        };
    };
    // drop:
    m_freem(m);
    Err(error)
}

/// `ipsec_hdrsz`: the bytes `tdbp` adds to a packet, -1 when unknown.
pub fn ipsec_hdrsz(tdbp: &Tdb) -> isize {
    let mut adjust: isize = match i32::from(tdbp.tdb_sproto.get()) {
        IPPROTO_IPIP => 0,
        IPPROTO_ESP => {
            let Some(enc) = tdbp.tdb_encalgxform.get() else {
                return -1;
            };

            // Header length
            let mut adjust = 2 * 4 + tdbp.tdb_ivlen.get() as isize;
            if tdbp.has_flags(TDBF_UDPENCAP) {
                adjust += size_of::<Udphdr>() as isize;
            }
            // Authenticator
            if let Some(auth) = tdbp.tdb_authalgxform.get() {
                adjust += auth.authsize as isize;
            }
            // Padding
            adjust += (enc.blocksize as isize).max(4);
            adjust
        }
        IPPROTO_AH => {
            let Some(auth) = tdbp.tdb_authalgxform.get() else {
                return -1;
            };

            AH_FLENGTH as isize + 4 + auth.authsize as isize
        }
        _ => return -1,
    };

    if !tdbp.has_flags(TDBF_TUNNELING) && !tdbp.has_flags(TDBF_USEDTUNNEL) {
        return adjust;
    }

    if tdbp.tdb_dst.get().sa_family() == AF_INET {
        adjust += size_of::<Ip>() as isize;
    }
    // INET6: sizeof(struct ip6_hdr); not configured.

    adjust
}

/// `ipsec_adjust_mtu`: lowers the MTU of every SA the packet went out through (its
/// `IPSEC_OUT_DONE` tags) to fit `mtu`.
pub fn ipsec_adjust_mtu(m: &Mbuf, mtu: u32) {
    net_assert_locked("ipsec_adjust_mtu");

    let mut mtu = mtu;
    let mut mtag = m_tag_find(m, PACKET_TAG_IPSEC_OUT_DONE, None);
    while let Some(t) = mtag {
        // SAFETY: `IPSEC_OUT_DONE` tags carry a `struct tdb_ident` (`ipsp_process_done`).
        let tdbi = unsafe { TdbIdent::read(t.data()) };
        let Some(tdbp) = gettdb(tdbi.rdomain, tdbi.spi, &tdbi.dst, tdbi.proto) else {
            break;
        };

        let adjust = ipsec_hdrsz(tdbp);
        if adjust == -1 {
            tdb_unref(Some(tdbp));
            break;
        }

        mtu = mtu.wrapping_sub(adjust as u32);
        tdbp.tdb_mtu.set(mtu);
        tdbp.tdb_mtutimeout
            .set(gettime() as u64 + IP_MTUDISC_TIMEOUT.load(Ordering::Relaxed) as u64);
        crate::ipsec_dprintf!(
            "ipsec_adjust_mtu",
            "spi {:08x} mtu {} adjust {} mbuf {:p}",
            ntohl(tdbp.tdb_spi.get()),
            tdbp.tdb_mtu.get(),
            adjust,
            m
        );
        tdb_unref(Some(tdbp));
        mtag = m_tag_find(m, PACKET_TAG_IPSEC_OUT_DONE, Some(t));
    }
}
