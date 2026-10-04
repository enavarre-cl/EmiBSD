/*	$OpenBSD: ip_ipip.h,v 1.15 2025/03/02 21:28:32 bluhm Exp $ */
/*	$OpenBSD: ip_ipip.c,v 1.111 2025/07/18 08:39:14 mvs Exp $ */
/* <LICENSES> */
/*
 * The authors of this code are John Ioannidis (ji@tla.org),
 * Angelos D. Keromytis (kermit@csd.uch.gr) and
 * Niels Provos (provos@physnet.uni-hamburg.de).
 *
 * The original version of this code was written by John Ioannidis
 * for BSD/OS in Athens, Greece, in November 1995.
 *
 * Ported to OpenBSD and NetBSD, with additional transforms, in December 1996,
 * by Angelos D. Keromytis.
 *
 * Additional transforms and features in 1997 and 1998 by Angelos D. Keromytis
 * and Niels Provos.
 *
 * Additional features in 1999 by Angelos D. Keromytis.
 *
 * Copyright (C) 1995, 1996, 1997, 1998, 1999 by John Ioannidis,
 * Angelos D. Keromytis and Niels Provos.
 * Copyright (c) 2001, Angelos D. Keromytis.
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

/*
 * The authors of this code are John Ioannidis (ji@tla.org),
 * Angelos D. Keromytis (kermit@csd.uch.gr) and
 * Niels Provos (provos@physnet.uni-hamburg.de).
 *
 * The original version of this code was written by John Ioannidis
 * for BSD/OS in Athens, Greece, in November 1995.
 *
 * Ported to OpenBSD and NetBSD, with additional transforms, in December 1996,
 * by Angelos D. Keromytis.
 *
 * Additional transforms and features in 1997 and 1998 by Angelos D. Keromytis
 * and Niels Provos.
 *
 * Additional features in 1999 by Angelos D. Keromytis.
 *
 * Copyright (C) 1995, 1996, 1997, 1998, 1999 by John Ioannidis,
 * Angelos D. Keromytis and Niels Provos.
 * Copyright (c) 2001, Angelos D. Keromytis.
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

//! IP-inside-IP processing: `<netinet/ip_ipip.h>` and `netinet/ip_ipip.c`. Not quite all the
//! functionality of RFC-1853, but the main idea is there. Tunnel mode IPsec decapsulates
//! through here (ESP or AH hands the inner packet back as protocol `IPPROTO_IPV4`), and
//! `ipip_output` adds the outer header of a tunnel mode SA.
//!
//! Upstream: sys/netinet/ip_ipip.h @ 3ce1f3f79392
//! Upstream: sys/netinet/ip_ipip.c @ 3ce1f3f79392
//!
//! Status: `ported` (M9c).
//!
//! ## Deviations
//! - `ipipcounters` (`struct cpumem *`) is the static array of atomics `IPIPCOUNTERS`.
//! - The packet is `&mut Option<&'static Mbuf>` (`struct mbuf **`); `ipip_output` returns a
//!   `Result`. The IP headers are read and written as copies (`mtod_ip`/`mtod_ip_store`).
//! - The local address spoofing check builds its `sockaddr_in` in a `struct
//!   sockaddr_storage`, as the C does, and calls the `unsafe` `rtalloc` over it.
//! - Not configured, each a comment at its site: `INET6` (the `AF_INET6` outer and
//!   `IPPROTO_IPV6` inner cases) and `NBPFILTER && NGIF` (`bpf_mtap_af` on `gif(4)`). `NPF`
//!   is configured (`pf_pkt_addr_changed`). `SMALL_KERNEL` is not defined: the sysctls are here.
//! - `unhandled_af` panics on an outer family that is neither `AF_INET` nor `AF_INET6`, as in
//!   C.

use core::mem::{offset_of, size_of};
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::kassert;
use crate::kern::kern_sysctl::{sysctl_int_bounded, sysctl_rdstruct};
use crate::kern::uipc_mbuf::{m_adj, m_copydata, m_prepend, m_pullup};
use crate::net::if_::{IFF_LOOPBACK, if_get, if_put, unhandled_af};
use crate::net::if_var::{Ifnet, Netstack};
use crate::net::pf::pf_pkt_addr_changed;
use crate::net::route::{RTF_LOCAL, rtalloc, rtfree};
use crate::netinet::in_::{INADDR_ANY, IPPROTO_DONE, IPPROTO_IPIP, IPPROTO_IPV4, SockaddrIn};
use crate::netinet::ip::{IP_DF, IP_MF, IP_OFFMASK, IPVERSION, Ip};
use crate::netinet::ip_ecn::{
    ECN_ALLOWED, ECN_ALLOWED_IPSEC, ip_ecn_egress, ip_ecn_ingress, ip_tos_patch,
};
use crate::netinet::ip_id::ip_randomid;
use crate::netinet::ip_input::{IP_DEFTTL, ip_input_if};
use crate::netinet::ip_ipsp::{IpsecInit, Tdb, XF_IP4, Xformsw, ipsp_address};
use crate::netinet::ip_var::{mtod_ip, mtod_ip_store};
use crate::sys::endian::{htons, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{M_AUTH, M_CONF, M_DONTWAIT, Mbuf, m_freemp};
use crate::sys::socket::{AF_INET, Sockaddr, SockaddrStorage};

/// `struct ipipstat`: the IP-in-IP statistics as `net.inet.ipip.stats` returns them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Ipipstat {
    /// Total input packets.
    pub ipips_ipackets: u64,
    /// Total output packets.
    pub ipips_opackets: u64,
    /// Packet shorter than header shows.
    pub ipips_hdrops: u64,
    /// `ipips_qfull`.
    pub ipips_qfull: u64,
    /// `ipips_ibytes`.
    pub ipips_ibytes: u64,
    /// `ipips_obytes`.
    pub ipips_obytes: u64,
    /// Packet dropped due to policy.
    pub ipips_pdrops: u64,
    /// IP spoofing attempts.
    pub ipips_spoof: u64,
    /// Protocol family mismatch.
    pub ipips_family: u64,
    /// Missing tunnel endpoint address.
    pub ipips_unspec: u64,
}

/// `IP4_DEFAULT_TTL`.
pub const IP4_DEFAULT_TTL: i32 = 0;
/// `IP4_SAME_TTL`.
pub const IP4_SAME_TTL: i32 = -1;

// Names for IPIP sysctl objects

/// `IPIPCTL_ALLOW`: accept incoming IP4 packets.
pub const IPIPCTL_ALLOW: i32 = 1;
/// `IPIPCTL_STATS`: IPIP stats.
pub const IPIPCTL_STATS: i32 = 2;
/// `IPIPCTL_MAXID`.
pub const IPIPCTL_MAXID: i32 = 3;

/// `enum ipipstat_counters`: one per field of [`Ipipstat`], in the same order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum IpipstatCounters {
    /// `ipips_ipackets`.
    IpipsIpackets,
    /// `ipips_opackets`.
    IpipsOpackets,
    /// `ipips_hdrops`.
    IpipsHdrops,
    /// `ipips_qfull`.
    IpipsQfull,
    /// `ipips_ibytes`.
    IpipsIbytes,
    /// `ipips_obytes`.
    IpipsObytes,
    /// `ipips_pdrops`.
    IpipsPdrops,
    /// `ipips_spoof`.
    IpipsSpoof,
    /// `ipips_family`.
    IpipsFamily,
    /// `ipips_unspec`.
    IpipsUnspec,
    /// `ipips_ncounters`.
    IpipsNcounters,
}

/// `ipips_ncounters`.
const IPIPS_NCOUNTERS: usize = IpipstatCounters::IpipsNcounters as usize;

/// `ipipcounters`.
pub static IPIPCOUNTERS: [AtomicU64; IPIPS_NCOUNTERS] =
    [const { AtomicU64::new(0) }; IPIPS_NCOUNTERS];

/// `ipipstat_inc(c)`.
pub fn ipipstat_inc(c: IpipstatCounters) {
    IPIPCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `ipipstat_add(c, v)`.
pub fn ipipstat_add(c: IpipstatCounters, v: u64) {
    IPIPCOUNTERS[c as usize].fetch_add(v, Ordering::Relaxed);
}

/// `ipipstat_pkt(p, b, v)`.
pub fn ipipstat_pkt(p: IpipstatCounters, b: IpipstatCounters, v: u64) {
    IPIPCOUNTERS[p as usize].fetch_add(1, Ordering::Relaxed);
    IPIPCOUNTERS[b as usize].fetch_add(v, Ordering::Relaxed);
}

/// \[a\] `ipip_allow`: we can control the acceptance of IP4 packets by altering the sysctl
/// `net.inet.ipip.allow` value. Zero means drop them, all else is acceptance.
pub static IPIP_ALLOW: AtomicI32 = AtomicI32::new(0);

/// `ipip_init`: `counters_alloc` of the statistics (a static array here).
pub fn ipip_init() {}

/// `ipip_input`: really only a wrapper for `ipip_input_if()`, for use with `pr_input`.
pub fn ipip_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    nxt: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let ipip_allow_local = IPIP_ALLOW.load(Ordering::Relaxed);
    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };

    // If we do not accept IP-in-IP explicitly, drop.
    if ipip_allow_local == 0 && m.m_flags().get() & (M_AUTH | M_CONF) == 0 {
        crate::ipsec_dprintf!("ipip_input", "dropped due to policy");
        ipipstat_inc(IpipstatCounters::IpipsPdrops);
        m_freemp(mp);
        return IPPROTO_DONE;
    }

    let Some(ifp) = if_get(m.m_pkthdr().ph_ifidx.get()) else {
        m_freemp(mp);
        return IPPROTO_DONE;
    };
    let nxt = ipip_input_if(mp, offp, nxt, af, ipip_allow_local, ifp, ns);
    if_put(ifp);

    nxt
}

/// `ipip_input_if`: called when we receive an IP{46} encapsulated packet, either because we
/// got it at a real interface, or because AH or ESP were being used in tunnel mode (in which
/// case the `ph_ifidx` element will contain the index of the encX interface associated with
/// the tunnel).
pub fn ipip_input_if(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    oaf: i32,
    allow: i32,
    ifp: &'static Ifnet,
    ns: Option<&Netstack>,
) -> i32 {
    let Some(mut m) = *mp else {
        return IPPROTO_DONE;
    };

    ipipstat_inc(IpipstatCounters::IpipsIpackets);

    'bad: {
        let mut hlen = match oaf {
            x if x == i32::from(AF_INET) => size_of::<Ip>() as i32,
            // INET6: sizeof(struct ip6_hdr); not configured.
            _ => unhandled_af(oaf),
        };

        // Bring the IP header in the first mbuf, if not there already
        if (m.m_len().get() as i32) < hlen {
            *mp = m_pullup(m, hlen);
            let Some(mm) = *mp else {
                crate::ipsec_dprintf!("ipip_input_if", "m_pullup() failed");
                ipipstat_inc(IpipstatCounters::IpipsHdrops);
                break 'bad;
            };
            m = mm;
        }

        // Keep outer ecn field.
        // INET6: (ntohl(ip6->ip6_flow) >> 20) & 0xff; not configured.
        let otos = mtod_ip(m).ip_tos;

        // Remove outer IP header
        kassert!(*offp > 0);
        m_adj(m, *offp);
        *offp = 0;

        match proto {
            IPPROTO_IPV4 => hlen = size_of::<Ip>() as i32,
            // INET6: IPPROTO_IPV6, sizeof(struct ip6_hdr); not configured.
            _ => {
                ipipstat_inc(IpipstatCounters::IpipsFamily);
                break 'bad;
            }
        }

        // Sanity check
        if m.m_pkthdr().len.get() < hlen {
            ipipstat_inc(IpipstatCounters::IpipsHdrops);
            break 'bad;
        }

        // Bring the inner header into the first mbuf, if not there already.
        if (m.m_len().get() as i32) < hlen {
            *mp = m_pullup(m, hlen);
            let Some(mm) = *mp else {
                crate::ipsec_dprintf!("ipip_input_if", "m_pullup() failed");
                ipipstat_inc(IpipstatCounters::IpipsHdrops);
                break 'bad;
            };
            m = mm;
        }

        // RFC 1853 specifies that the inner TTL should not be touched on decapsulation.
        // There's no reason this comment should be here, but this is as good as any a
        // position.

        // Some sanity checks in the inner IP header
        let ip = match proto {
            IPPROTO_IPV4 => {
                let mut ip = mtod_ip(m);
                hlen = i32::from(ip.ip_hl()) << 2;
                if m.m_pkthdr().len.get() < hlen {
                    ipipstat_inc(IpipstatCounters::IpipsHdrops);
                    break 'bad;
                }
                let mut itos = ip.ip_tos;
                let mode = if m.m_flags().get() & (M_AUTH | M_CONF) != 0 {
                    ECN_ALLOWED_IPSEC
                } else {
                    ECN_ALLOWED
                };
                if !ip_ecn_egress(mode, &otos, &mut itos) {
                    crate::ipsec_dprintf!("ipip_input_if", "ip_ecn_egress() failed");
                    ipipstat_inc(IpipstatCounters::IpipsPdrops);
                    break 'bad;
                }
                // re-calculate the checksum if ip_tos was changed
                if itos != ip.ip_tos {
                    ip_tos_patch(&mut ip, itos);
                    mtod_ip_store(m, &ip);
                }
                ip
            }
            // INET6: the IPv6 inner header's traffic class; not configured.
            _ => break 'bad,
        };

        // Check for local address spoofing.
        if ifp.if_flags.get() & IFF_LOOPBACK == 0 && allow != 2 {
            let mut ss = SockaddrStorage::default();
            let sin = SockaddrIn {
                sin_family: AF_INET,
                sin_len: size_of::<SockaddrIn>() as u8,
                sin_addr: ip.ip_src,
                ..SockaddrIn::default()
            };
            // SAFETY: a `sockaddr_storage` holds any socket address; `SockaddrIn` has no
            // padding.
            unsafe {
                core::ptr::write_unaligned(core::ptr::from_mut(&mut ss).cast::<SockaddrIn>(), sin)
            };
            // INET6: a sockaddr_in6 of ip6_src; not configured.
            // SAFETY: `ss` holds the `sockaddr_in` just written.
            let rt = unsafe {
                rtalloc(
                    core::ptr::from_ref(&ss).cast::<Sockaddr>(),
                    0,
                    m.m_pkthdr().ph_rtableid.get(),
                )
            };
            if let Some(r) = rt
                && r.rt_flags.get() & RTF_LOCAL != 0
            {
                ipipstat_inc(IpipstatCounters::IpipsSpoof);
                rtfree(rt);
                break 'bad;
            }
            rtfree(rt);
        }

        // Statistics
        ipipstat_add(
            IpipstatCounters::IpipsIbytes,
            (m.m_pkthdr().len.get() - hlen) as u64,
        );

        // NBPFILTER > 0 && NGIF > 0: bpf_mtap_af on a gif(4) interface; not configured.
        pf_pkt_addr_changed(m);

        // Interface pointer stays the same; if no IPsec processing has been done (or will be
        // done), this will point to a normal interface. Otherwise, it'll point to an enc
        // interface, which will allow a packet filter to distinguish between secure and
        // untrusted packets.

        if proto == IPPROTO_IPV4 {
            return ip_input_if(mp, offp, proto, oaf, ifp, ns);
        }
        // INET6: IPPROTO_IPV6 through ip6_input_if; not configured.
    }
    // bad:
    m_freemp(mp);
    IPPROTO_DONE
}

/// `ipip_output`: adds the outer IP header of the tunnel `tdb` in front of the packet. The
/// packet may be replaced, or freed (and `*mp` cleared) on failure.
pub fn ipip_output(mp: &mut Option<&'static Mbuf>, tdb: &Tdb) -> Result<(), Errno> {
    let Some(m) = *mp else {
        return Err(Errno::EINVAL);
    };

    // XXX Deal with empty TDB source/destination addresses.

    let mut tp = [0u8; 1];
    m_copydata(m, 0, &mut tp);
    let tp = tp[0] >> 4; // Get the IP version number.

    let dst = tdb.tdb_dst.get();
    let src = tdb.tdb_src.get();
    let error: Errno = 'drop: {
        let obytes: u64;
        match dst.sa_family() {
            AF_INET => {
                if src.sa_family() != AF_INET
                    || src.sin_addr().s_addr == INADDR_ANY
                    || dst.sin_addr().s_addr == INADDR_ANY
                {
                    crate::ipsec_dprintf!(
                        "ipip_output",
                        "unspecified tunnel endpoint address in SA {}/{:08x}",
                        ipsp_address(&dst),
                        ntohl(tdb.tdb_spi.get())
                    );

                    ipipstat_inc(IpipstatCounters::IpipsUnspec);
                    break 'drop Errno::EINVAL;
                }

                *mp = m_prepend(m, size_of::<Ip>() as i32, M_DONTWAIT);
                let Some(m) = *mp else {
                    crate::ipsec_dprintf!("ipip_output", "M_PREPEND failed");
                    ipipstat_inc(IpipstatCounters::IpipsHdrops);
                    break 'drop Errno::ENOBUFS;
                };

                let mut ipo = Ip::default();
                ipo.set_ip_v(IPVERSION);
                ipo.set_ip_hl(5);
                ipo.ip_len = htons(m.m_pkthdr().len.get() as u16);
                ipo.ip_ttl = IP_DEFTTL.load(Ordering::Relaxed) as u8;
                ipo.ip_sum = 0;
                ipo.ip_src = src.sin_addr();
                ipo.ip_dst = dst.sin_addr();

                // We do the htons() to prevent snoopers from determining our endianness.
                ipo.ip_id = htons(ip_randomid());

                let itos;
                // If the inner protocol is IP...
                if tp == IPVERSION {
                    // Save ECN notification
                    let mut b = [0u8; 1];
                    m_copydata(m, (size_of::<Ip>() + offset_of!(Ip, ip_tos)) as i32, &mut b);
                    itos = b[0];

                    ipo.ip_p = IPPROTO_IPIP as u8;

                    // We should be keeping tunnel soft-state and send back ICMPs if needed.
                    let mut off = [0u8; 2];
                    m_copydata(
                        m,
                        (size_of::<Ip>() + offset_of!(Ip, ip_off)) as i32,
                        &mut off,
                    );
                    let mut ip_off = ntohs(u16::from_ne_bytes(off));
                    ip_off &= !(IP_DF | IP_MF | IP_OFFMASK);
                    ipo.ip_off = htons(ip_off);
                }
                // INET6: an IPv6 inner packet (IPPROTO_IPV6, the traffic class); not
                // configured.
                else {
                    ipipstat_inc(IpipstatCounters::IpipsFamily);
                    break 'drop Errno::EAFNOSUPPORT;
                }

                let mut otos = 0;
                ip_ecn_ingress(ECN_ALLOWED, &mut otos, &itos);
                ipo.ip_tos = otos;
                mtod_ip_store(m, &ipo);

                obytes = (m.m_pkthdr().len.get() as usize - size_of::<Ip>()) as u64;
                if tdb.tdb_xform.get().is_some_and(|x| x.xf_type == XF_IP4) {
                    tdb.tdb_cur_bytes.set(tdb.tdb_cur_bytes.get() + obytes);
                }
            }
            // INET6: an IPv6 outer header; not configured.
            _ => {
                crate::ipsec_dprintf!(
                    "ipip_output",
                    "unsupported protocol family {}",
                    dst.sa_family()
                );
                ipipstat_inc(IpipstatCounters::IpipsFamily);
                break 'drop Errno::EPFNOSUPPORT;
            }
        }

        ipipstat_pkt(
            IpipstatCounters::IpipsOpackets,
            IpipstatCounters::IpipsObytes,
            obytes,
        );
        return Ok(());
    };
    // drop:
    m_freemp(mp);
    Err(error)
}

/// `ipe4_attach`.
pub fn ipe4_attach() -> i32 {
    0
}

/// `ipe4_init`: an IP-in-IP SA needs no keys.
pub fn ipe4_init(tdbp: &Tdb, xsp: &'static Xformsw, _ii: &mut IpsecInit<'_>) -> Result<(), Errno> {
    tdbp.tdb_xform.set(Some(xsp));
    Ok(())
}

/// `ipe4_zeroize`.
pub fn ipe4_zeroize(_tdbp: &Tdb) -> Result<(), Errno> {
    Ok(())
}

/// `ipe4_input`: never called (IP-in-IP packets go through `ipip_input`); returns `EINVAL`
/// as the protocol, as the C does.
pub fn ipe4_input(
    mp: &mut Option<&'static Mbuf>,
    _tdb: &'static Tdb,
    _hlen: i32,
    _proto: i32,
    _ns: Option<&Netstack>,
) -> i32 {
    // This is a rather serious mistake, so no conditional printing.
    crate::kprintf!("ipe4_input: should never be called\n");
    m_freemp(mp);
    Errno::EINVAL as i32
}

/// `ipip_sysctl_ipipstat`.
fn ipip_sysctl_ipipstat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    const N: usize = IPIPS_NCOUNTERS;
    const _: () = assert!(size_of::<Ipipstat>() == N * size_of::<u64>());
    let mut bytes = [0u8; N * size_of::<u64>()];
    for (i, c) in IPIPCOUNTERS.iter().enumerate() {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&c.load(Ordering::Relaxed).to_ne_bytes());
    }
    sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
}

/// `ipip_sysctl`: `net.inet.ipip`.
pub fn ipip_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    // All sysctl names at this level are terminal.
    let [name0] = name else {
        return Err(Errno::ENOTDIR);
    };

    match *name0 {
        IPIPCTL_ALLOW => sysctl_int_bounded(oldp, oldlenp, newp, newlen, &IPIP_ALLOW, 0, 2),
        IPIPCTL_STATS => ipip_sysctl_ipipstat(oldp, oldlenp, newp),
        _ => Err(Errno::ENOPROTOOPT),
    }
}
