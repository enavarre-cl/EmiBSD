/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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

/* <CODE> */
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
//! - `INET6` is configured (feature `inet6`): the `AF_INET6` outer and `IPPROTO_IPV6` inner
//!   cases, `ip6_input_if` and the IPv6 outer header of `ipip_output`. The inner header's
//!   scoped addresses are cleared through `m_copydata`/`m_copyback` (the C writes through
//!   `mtod`), so a header split over several mbufs is handled too. `NBPFILTER && NGIF`
//!   (`bpf_mtap_af` on `gif(4)`) is not configured. `NPF`
//!   is configured (`pf_pkt_addr_changed`). `SMALL_KERNEL` is not defined: the sysctls are here.
//! - `unhandled_af` panics on an outer family that is neither `AF_INET` nor `AF_INET6`, as in
//!   C.

use core::mem::{offset_of, size_of};
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::kassert;
use crate::kern::kern_sysctl::{sysctl_int_bounded, sysctl_rdstruct};
#[cfg(feature = "inet6")]
use crate::kern::uipc_mbuf::m_copyback;
use crate::kern::uipc_mbuf::{m_adj, m_copydata, m_prepend, m_pullup};
use crate::net::if_::{IFF_LOOPBACK, if_get, if_put, unhandled_af};
use crate::net::if_var::{Ifnet, Netstack};
use crate::net::pf::pf_pkt_addr_changed;
use crate::net::route::{RTF_LOCAL, rtalloc, rtfree};
#[cfg(feature = "inet6")]
use crate::netinet::in_::IPPROTO_IPV6;
use crate::netinet::in_::{
    INADDR_ANY, IPPROTO_DONE, IPPROTO_IPIP, IPPROTO_IPV4, InAddr, SockaddrIn,
};
use crate::netinet::ip::{IP_DF, IP_MF, IP_OFFMASK, IPVERSION, Ip};
use crate::netinet::ip_ecn::{
    ECN_ALLOWED, ECN_ALLOWED_IPSEC, ip_ecn_egress, ip_ecn_ingress, ip_tos_patch,
};
use crate::netinet::ip_id::ip_randomid;
use crate::netinet::ip_input::{IP_DEFTTL, ip_input_if};
use crate::netinet::ip_ipsp::{IpsecInit, Tdb, XF_IP4, Xformsw, ipsp_address};
use crate::netinet::ip_var::{mtod_ip, mtod_ip_store};
#[cfg(feature = "inet6")]
use crate::netinet::ip6::{IPV6_VERSION, Ip6Hdr};
#[cfg(feature = "inet6")]
use crate::netinet6::in6::{In6Addr, SockaddrIn6, in6_is_addr_unspecified, in6_is_scope_embed};
#[cfg(feature = "inet6")]
use crate::netinet6::in6_proto::IP6_DEFHLIM;
#[cfg(feature = "inet6")]
use crate::netinet6::in6_src::in6_embedscope;
#[cfg(feature = "inet6")]
use crate::netinet6::ip6_input::ip6_input_if;
#[cfg(feature = "inet6")]
use crate::netinet6::ip6_var::{mtod_ip6, mtod_ip6_store};
#[cfg(feature = "inet6")]
use crate::sys::endian::htonl;
use crate::sys::endian::{htons, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{M_AUTH, M_CONF, M_DONTWAIT, Mbuf, m_freemp};
#[cfg(feature = "inet6")]
use crate::sys::socket::AF_INET6;
use crate::sys::socket::{AF_INET, Sockaddr, SockaddrStorage};

/// The source address of the inner header of a decapsulated packet (the C's `ip`/`ip6`
/// pointers, one of them non-NULL).
enum InnerSrc {
    /// An `IPPROTO_IPV4` inner header's `ip_src`.
    V4(InAddr),
    /// An `IPPROTO_IPV6` inner header's `ip6_src`.
    #[cfg(feature = "inet6")]
    V6(In6Addr),
}

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
            #[cfg(feature = "inet6")]
            x if x == i32::from(AF_INET6) => size_of::<Ip6Hdr>() as i32,
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
        let otos = match oaf {
            x if x == i32::from(AF_INET) => mtod_ip(m).ip_tos,
            #[cfg(feature = "inet6")]
            x if x == i32::from(AF_INET6) => ((ntohl(mtod_ip6(m).ip6_flow) >> 20) & 0xff) as u8,
            _ => unhandled_af(oaf),
        };

        // Remove outer IP header
        kassert!(*offp > 0);
        m_adj(m, *offp);
        *offp = 0;

        match proto {
            IPPROTO_IPV4 => hlen = size_of::<Ip>() as i32,
            #[cfg(feature = "inet6")]
            IPPROTO_IPV6 => hlen = size_of::<Ip6Hdr>() as i32,
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
        let inner = match proto {
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
                InnerSrc::V4(ip.ip_src)
            }
            #[cfg(feature = "inet6")]
            IPPROTO_IPV6 => {
                let mut ip6 = mtod_ip6(m);
                let mut itos = ((ntohl(ip6.ip6_flow) >> 20) & 0xff) as u8;
                if !ip_ecn_egress(ECN_ALLOWED, &otos, &mut itos) {
                    crate::ipsec_dprintf!("ipip_input_if", "ip_ecn_egress() failed");
                    ipipstat_inc(IpipstatCounters::IpipsPdrops);
                    break 'bad;
                }
                ip6.ip6_flow &= !htonl(0xff << 20);
                ip6.ip6_flow |= htonl(u32::from(itos) << 20);
                mtod_ip6_store(m, &ip6);
                InnerSrc::V6(ip6.ip6_src)
            }
            _ => break 'bad,
        };

        // Check for local address spoofing.
        if ifp.if_flags.get() & IFF_LOOPBACK == 0 && allow != 2 {
            let mut ss = SockaddrStorage::default();
            match inner {
                InnerSrc::V4(src) => {
                    let sin = SockaddrIn {
                        sin_family: AF_INET,
                        sin_len: size_of::<SockaddrIn>() as u8,
                        sin_addr: src,
                        ..SockaddrIn::default()
                    };
                    // SAFETY: a `sockaddr_storage` holds any socket address; `SockaddrIn`
                    // has no padding.
                    unsafe {
                        core::ptr::write_unaligned(
                            core::ptr::from_mut(&mut ss).cast::<SockaddrIn>(),
                            sin,
                        )
                    };
                }
                #[cfg(feature = "inet6")]
                InnerSrc::V6(src) => {
                    let sin6 = SockaddrIn6 {
                        sin6_family: AF_INET6,
                        sin6_len: size_of::<SockaddrIn6>() as u8,
                        sin6_addr: src,
                        ..SockaddrIn6::default()
                    };
                    // SAFETY: as above; `SockaddrIn6` has no padding either.
                    unsafe {
                        core::ptr::write_unaligned(
                            core::ptr::from_mut(&mut ss).cast::<SockaddrIn6>(),
                            sin6,
                        )
                    };
                }
            }
            // SAFETY: `ss` holds the `sockaddr_in` or `sockaddr_in6` just written.
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
        #[cfg(feature = "inet6")]
        if proto == IPPROTO_IPV6 {
            return ip6_input_if(mp, offp, proto, oaf, ifp, ns);
        }
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
                match tp {
                    // If the inner protocol is IP...
                    IPVERSION => {
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
                    #[cfg(feature = "inet6")]
                    x if x == IPV6_VERSION >> 4 => {
                        // Save ECN notification.
                        let mut b = [0u8; 4];
                        m_copydata(
                            m,
                            (size_of::<Ip>() + offset_of!(Ip6Hdr, ip6_flow)) as i32,
                            &mut b,
                        );
                        itos = (ntohl(u32::from_ne_bytes(b)) >> 20) as u8;
                        ipo.ip_p = IPPROTO_IPV6 as u8;
                        ipo.ip_off = 0;
                    }
                    _ => {
                        ipipstat_inc(IpipstatCounters::IpipsFamily);
                        break 'drop Errno::EAFNOSUPPORT;
                    }
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
            #[cfg(feature = "inet6")]
            AF_INET6 => {
                if in6_is_addr_unspecified(&dst.sin6_addr())
                    || src.sa_family() != AF_INET6
                    || in6_is_addr_unspecified(&src.sin6_addr())
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

                // If the inner protocol is IPv6, clear link local scope
                if tp == IPV6_VERSION >> 4 {
                    // scoped address handling: ip6_src and ip6_dst of the inner header.
                    let mut b = [0u8; 2 * size_of::<In6Addr>()];
                    m_copydata(m, offset_of!(Ip6Hdr, ip6_src) as i32, &mut b);
                    let (s, d) = b.split_at(size_of::<In6Addr>());
                    let mut ip6_src = In6Addr::new(s.try_into().unwrap_or_default());
                    let mut ip6_dst = In6Addr::new(d.try_into().unwrap_or_default());
                    if in6_is_scope_embed(&ip6_src) {
                        ip6_src.set_s6_addr16(1, 0);
                    }
                    if in6_is_scope_embed(&ip6_dst) {
                        ip6_dst.set_s6_addr16(1, 0);
                    }
                    b[..16].copy_from_slice(&ip6_src.s6_addr);
                    b[16..].copy_from_slice(&ip6_dst.s6_addr);
                    let _ = m_copyback(m, offset_of!(Ip6Hdr, ip6_src) as i32, &b, M_DONTWAIT);
                }

                *mp = m_prepend(m, size_of::<Ip6Hdr>() as i32, M_DONTWAIT);
                let Some(m) = *mp else {
                    crate::ipsec_dprintf!("ipip_output", "M_PREPEND failed");
                    ipipstat_inc(IpipstatCounters::IpipsHdrops);
                    break 'drop Errno::ENOBUFS;
                };

                // Initialize IPv6 header
                let mut ip6o = Ip6Hdr::zeroed();
                ip6o.ip6_flow = 0;
                ip6o.set_ip6_vfc(IPV6_VERSION);
                ip6o.ip6_plen =
                    htons((m.m_pkthdr().len.get() as usize - size_of::<Ip6Hdr>()) as u16);
                ip6o.ip6_hlim = IP6_DEFHLIM.load(Ordering::Relaxed) as u8;
                let _ = in6_embedscope(&mut ip6o.ip6_src, &src.sin6(), None, None);
                let _ = in6_embedscope(&mut ip6o.ip6_dst, &dst.sin6(), None, None);

                let itos;
                match tp {
                    IPVERSION => {
                        // Save ECN notification
                        let mut b = [0u8; 1];
                        m_copydata(
                            m,
                            (size_of::<Ip6Hdr>() + offset_of!(Ip, ip_tos)) as i32,
                            &mut b,
                        );
                        itos = b[0];

                        // This is really IPVERSION.
                        ip6o.ip6_nxt = IPPROTO_IPIP as u8;
                    }
                    x if x == IPV6_VERSION >> 4 => {
                        // Save ECN notification.
                        let mut b = [0u8; 4];
                        m_copydata(
                            m,
                            (size_of::<Ip6Hdr>() + offset_of!(Ip6Hdr, ip6_flow)) as i32,
                            &mut b,
                        );
                        itos = (ntohl(u32::from_ne_bytes(b)) >> 20) as u8;

                        ip6o.ip6_nxt = IPPROTO_IPV6 as u8;
                    }
                    _ => {
                        ipipstat_inc(IpipstatCounters::IpipsFamily);
                        break 'drop Errno::EAFNOSUPPORT;
                    }
                }

                let mut otos = 0;
                ip_ecn_ingress(ECN_ALLOWED, &mut otos, &itos);
                ip6o.ip6_flow |= htonl(u32::from(otos) << 20);
                mtod_ip6_store(m, &ip6o);

                obytes = (m.m_pkthdr().len.get() as usize - size_of::<Ip6Hdr>()) as u64;
                if tdb.tdb_xform.get().is_some_and(|x| x.xf_type == XF_IP4) {
                    tdb.tdb_cur_bytes.set(tdb.tdb_cur_bytes.get() + obytes);
                }
            }
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
#[cfg(feature = "inet6")]
mod tests {
    // Host tests for IP-in-IP over IPv6: `ipip_output` puts an IPv6 header in front of an IPv4 or
    // IPv6 packet (or an IPv4 header in front of an IPv6 one), keeps the inner traffic class
    // and clears the scope of link-local inner addresses; `ipip_input_if` takes an IPv6 header
    // as the outer or as the inner one.

    use std::sync::MutexGuard;
    use std::vec;
    use std::vec::Vec;

    use super::*;
    use crate::kern::uipc_mbuf::{m_copydata, m_freem};
    use crate::net::if_::tests::{test_ifnet, test_packet};
    use crate::netinet::ip_ipsp::SockaddrUnion;
    use crate::netinet6::ip6_input::IP6COUNTERS;
    use crate::netinet6::ip6_var::Ip6statCounters;
    use crate::sys::mbuf::M_AUTH;

    fn setup() -> (MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
        crate::netinet::ip_input::tests::setup()
    }

    /// An IPv6 address from its eight 16-bit words.
    fn a6(w: [u16; 8]) -> In6Addr {
        let mut a = [0u8; 16];
        for (i, w) in w.iter().enumerate() {
            a[2 * i..2 * i + 2].copy_from_slice(&w.to_be_bytes());
        }
        In6Addr::new(a)
    }

    fn su6(a: In6Addr) -> SockaddrUnion {
        SockaddrUnion::from_sin6(&SockaddrIn6 {
            sin6_len: size_of::<SockaddrIn6>() as u8,
            sin6_family: AF_INET6,
            sin6_addr: a,
            ..SockaddrIn6::default()
        })
    }

    fn su4(a: [u8; 4]) -> SockaddrUnion {
        SockaddrUnion::from_sin(&SockaddrIn {
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_family: AF_INET,
            sin_addr: InAddr {
                s_addr: u32::from_ne_bytes(a),
            },
            ..SockaddrIn::default()
        })
    }

    /// A TDB from `src` to `dst`.
    fn tdb(src: SockaddrUnion, dst: SockaddrUnion) -> Tdb {
        let t = Tdb::new();
        t.tdb_src.set(src);
        t.tdb_dst.set(dst);
        t
    }

    /// An IPv4 packet with type of service `tos` and 8 bytes of payload.
    fn ip4_packet(tos: u8) -> Vec<u8> {
        let mut p = vec![0x45, tos];
        p.extend_from_slice(&28u16.to_be_bytes());
        p.extend_from_slice(&[0x12, 0x34, 0x40, 0x00, 64, 17, 0, 0]);
        p.extend_from_slice(&[10, 0, 0, 1, 10, 0, 0, 2]);
        p.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        p
    }

    /// An IPv6 packet with traffic class `tclass`, UDP, and 8 bytes of payload.
    fn ip6_packet(tclass: u8, src: In6Addr, dst: In6Addr) -> Vec<u8> {
        let flow = 0x6000_0000u32 | (u32::from(tclass) << 20) | 0x12345;
        let mut p = flow.to_be_bytes().to_vec();
        p.extend_from_slice(&8u16.to_be_bytes());
        p.extend_from_slice(&[17, 64]);
        p.extend_from_slice(&src.s6_addr);
        p.extend_from_slice(&dst.s6_addr);
        p.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        p
    }

    fn bytes(m: &Mbuf) -> Vec<u8> {
        let mut v = vec![0u8; m.m_pkthdr().len.get() as usize];
        m_copydata(m, 0, &mut v);
        v
    }

    fn counter(c: IpipstatCounters) -> u64 {
        IPIPCOUNTERS[c as usize].load(Ordering::Relaxed)
    }

    #[test]
    fn ipv4_travels_in_an_ipv6_tunnel() {
        let _g = setup();
        let (src, dst) = (
            a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
            a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]),
        );
        let t = tdb(su6(src), su6(dst));

        // The inner type of service carries over (CE becomes ECT(0)).
        for (itos, otos) in [(0xb8u8, 0xb8u8), (0xbb, 0xba), (0, 0)] {
            let inner = ip4_packet(itos);
            let mut mp = Some(test_packet(&inner));
            ipip_output(&mut mp, &t).expect("encapsulated");
            let out = bytes(mp.expect("the packet"));
            assert_eq!(out.len(), 40 + inner.len());
            assert_eq!(out[0] >> 4, 6, "version");
            let flow = u32::from_be_bytes([out[0], out[1], out[2], out[3]]);
            assert_eq!(flow & 0x000f_ffff, 0, "no flow label");
            assert_eq!((flow >> 20) & 0xff, u32::from(otos), "traffic class");
            assert_eq!(
                usize::from(u16::from_be_bytes([out[4], out[5]])),
                inner.len(),
                "ip6_plen"
            );
            assert_eq!(out[6], IPPROTO_IPIP as u8, "ip6_nxt");
            assert_eq!(out[7], 64, "ip6_hlim");
            assert_eq!(&out[8..24], &src.s6_addr);
            assert_eq!(&out[24..40], &dst.s6_addr);
            assert_eq!(&out[40..], &inner[..]);
            m_freem(mp);
        }
    }

    #[test]
    fn ipv6_travels_in_an_ipv6_tunnel_without_its_link_local_scope() {
        let _g = setup();
        let (src, dst) = (
            a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
            a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]),
        );
        let t = tdb(su6(src), su6(dst));

        // An inner packet between link-local addresses has its embedded scope zeroed.
        let isrc = a6([0xfe80, 1, 0, 0, 0, 0, 0, 5]);
        let idst = a6([0xfe80, 1, 0, 0, 0, 0, 0, 6]);
        let inner = ip6_packet(0xb8, isrc, idst);
        let mut mp = Some(test_packet(&inner));
        ipip_output(&mut mp, &t).expect("encapsulated");
        let out = bytes(mp.expect("the packet"));
        assert_eq!(out.len(), 88);
        assert_eq!(out[6], IPPROTO_IPV6 as u8);
        assert_eq!(
            (u32::from_be_bytes([out[0], out[1], out[2], out[3]]) >> 20) & 0xff,
            0xb8
        );
        assert_eq!(&out[8..24], &src.s6_addr);
        assert_eq!(&out[24..40], &dst.s6_addr);
        // The inner header is the packet's, but for the scope words.
        assert_eq!(&out[40..48], &inner[..8]);
        assert_eq!(&out[48..50], &[0xfe, 0x80]);
        assert_eq!(&out[50..52], &[0, 0], "ip6_src scope");
        assert_eq!(&out[52..64], &inner[12..24]);
        assert_eq!(&out[64..66], &[0xfe, 0x80]);
        assert_eq!(&out[66..68], &[0, 0], "ip6_dst scope");
        assert_eq!(&out[68..], &inner[28..]);
        m_freem(mp);
    }

    #[test]
    fn ipv6_travels_in_an_ipv4_tunnel() {
        let _g = setup();
        let t = tdb(su4([192, 168, 77, 1]), su4([192, 168, 77, 2]));
        let inner = ip6_packet(
            0xb8,
            a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
            a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]),
        );
        let mut mp = Some(test_packet(&inner));
        ipip_output(&mut mp, &t).expect("encapsulated");
        let out = bytes(mp.expect("the packet"));
        assert_eq!(out.len(), 20 + inner.len());
        assert_eq!(out[0], 0x45);
        assert_eq!(out[1], 0xb8, "the inner traffic class");
        assert_eq!(u16::from_be_bytes([out[2], out[3]]) as usize, out.len());
        assert_eq!(&out[6..8], &[0, 0], "ip_off: no DF or fragment bits");
        assert_eq!(out[9], IPPROTO_IPV6 as u8, "ip_p");
        assert_eq!(&out[12..16], &[192, 168, 77, 1]);
        assert_eq!(&out[16..20], &[192, 168, 77, 2]);
        assert_eq!(&out[20..], &inner[..]);
        m_freem(mp);
    }

    #[test]
    fn bad_tunnel_endpoints_and_inner_packets_are_refused() {
        let _g = setup();
        let (src, dst) = (
            a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
            a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]),
        );
        let inner = ip4_packet(0);

        // An unspecified or mismatched endpoint (IPv6 outer).
        for t in [
            tdb(su6(src), su6(In6Addr::default())),
            tdb(su6(In6Addr::default()), su6(dst)),
            tdb(su4([10, 0, 0, 1]), su6(dst)),
        ] {
            let before = counter(IpipstatCounters::IpipsUnspec);
            let mut mp = Some(test_packet(&inner));
            assert_eq!(ipip_output(&mut mp, &t), Err(Errno::EINVAL));
            assert!(mp.is_none(), "the packet is freed");
            assert_eq!(counter(IpipstatCounters::IpipsUnspec), before + 1);
        }

        // Neither IPv4 nor IPv6 inside.
        let t = tdb(su6(src), su6(dst));
        let before = counter(IpipstatCounters::IpipsFamily);
        let mut mp = Some(test_packet(&[0x10; 40]));
        assert_eq!(ipip_output(&mut mp, &t), Err(Errno::EAFNOSUPPORT));
        assert!(mp.is_none());
        assert_eq!(counter(IpipstatCounters::IpipsFamily), before + 1);
    }

    fn ip6_total() -> u64 {
        IP6COUNTERS[Ip6statCounters::Ip6sTotal as usize].load(Ordering::Relaxed)
    }

    #[test]
    fn ipv6_is_taken_out_of_an_ipv4_or_ipv6_tunnel() {
        let _g = setup();
        let ifp = test_ifnet(b"gif0");
        let (osrc, odst) = (
            a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
            a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]),
        );
        let inner = ip6_packet(
            0,
            a6([0x2001, 0xdb8, 5, 0, 0, 0, 0, 1]),
            a6([0x2001, 0xdb8, 5, 0, 0, 0, 0, 2]),
        );

        // IPv6 inside IPv4: the outer header goes, the inner packet goes to ip6_input_if.
        let mut outer = vec![0x45, 0];
        outer.extend_from_slice(&((20 + inner.len()) as u16).to_be_bytes());
        outer.extend_from_slice(&[
            0,
            0,
            0,
            0,
            64,
            IPPROTO_IPV6 as u8,
            0,
            0,
            192,
            168,
            77,
            2,
            192,
            168,
            77,
            1,
        ]);
        outer.extend_from_slice(&inner);
        let m = test_packet(&outer);
        m.m_flags().set(m.m_flags().get() | M_AUTH);
        let (bytes_before, total_before) = (counter(IpipstatCounters::IpipsIbytes), ip6_total());
        let mut mp = Some(m);
        let mut off = 20;
        // `allow` 2 skips the spoofing check, which needs a routing table.
        let r = ipip_input_if(
            &mut mp,
            &mut off,
            IPPROTO_IPV6,
            i32::from(AF_INET),
            2,
            ifp,
            None,
        );
        assert_eq!(r, IPPROTO_DONE);
        assert_eq!(off, 0);
        assert_eq!(
            ip6_total(),
            total_before + 1,
            "the inner packet reached ip6_input_if"
        );
        assert_eq!(
            counter(IpipstatCounters::IpipsIbytes),
            bytes_before + 8,
            "inner bytes after its header"
        );

        // IPv6 inside IPv6: the outer header is the IPv6 one (an IPv6 outer header's next
        // header is the inner protocol).
        let mut outer = ip6_packet(0, osrc, odst);
        outer[6] = IPPROTO_IPV6 as u8;
        outer[4..6].copy_from_slice(&(inner.len() as u16).to_be_bytes());
        outer.truncate(40);
        outer.extend_from_slice(&inner);
        let m = test_packet(&outer);
        m.m_flags().set(m.m_flags().get() | M_AUTH);
        let total_before = ip6_total();
        let mut mp = Some(m);
        let mut off = 40;
        let r = ipip_input_if(
            &mut mp,
            &mut off,
            IPPROTO_IPV6,
            i32::from(AF_INET6),
            2,
            ifp,
            None,
        );
        assert_eq!(r, IPPROTO_DONE);
        assert_eq!(ip6_total(), total_before + 1);

        // An inner header that is cut short is a header drop.
        let mut outer = ip6_packet(0, osrc, odst);
        outer[6] = IPPROTO_IPV6 as u8;
        outer.truncate(40);
        outer.extend_from_slice(&inner[..20]);
        let m = test_packet(&outer);
        let before = counter(IpipstatCounters::IpipsHdrops);
        let mut mp = Some(m);
        let mut off = 40;
        let r = ipip_input_if(
            &mut mp,
            &mut off,
            IPPROTO_IPV6,
            i32::from(AF_INET6),
            2,
            ifp,
            None,
        );
        assert_eq!(r, IPPROTO_DONE);
        assert!(mp.is_none());
        assert_eq!(counter(IpipstatCounters::IpipsHdrops), before + 1);

        // A protocol that is not IP-in-IP is counted as a family drop.
        let mut outer = ip6_packet(0, osrc, odst);
        outer.extend_from_slice(&[0; 40]);
        let m = test_packet(&outer);
        let before = counter(IpipstatCounters::IpipsFamily);
        let mut mp = Some(m);
        let mut off = 40;
        let r = ipip_input_if(&mut mp, &mut off, 99, i32::from(AF_INET6), 2, ifp, None);
        assert_eq!(r, IPPROTO_DONE);
        assert_eq!(counter(IpipstatCounters::IpipsFamily), before + 1);
    }
}
/* </TESTS> */
