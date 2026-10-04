/*	$OpenBSD: ipsec_input.c,v 1.223 2026/05/07 14:58:03 claudio Exp $	*/
/* <LICENSES> */
/*
 * The authors of this code are John Ioannidis (ji@tla.org),
 * Angelos D. Keromytis (kermit@csd.uch.gr) and
 * Niels Provos (provos@physnet.uni-hamburg.de).
 *
 * This code was written by John Ioannidis for BSD/OS in Athens, Greece,
 * in November 1995.
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

//! IPsec input processing: `netinet/ipsec_input.c`. The protocol switch hands AH, ESP and
//! IPComp packets to `ah46_input`, `esp46_input` and `ipcomp46_input`; `ipsec_common_input`
//! finds the SA by SPI and destination and calls its transform, whose callback
//! `ipsec_common_input_cb` fixes the decapsulated header up, tags the packet with the SA and
//! returns the next protocol to the delivery loop. Also here: the IPsec sysctl variables and
//! handlers (`net.inet.ip.ipsec-*`, `net.inet.esp`, `net.inet.ah`, `net.inet.ipcomp`), the
//! path MTU feedback from ICMP (`*_ctlinput`) and the policy checks of `ip_input`.
//!
//! Upstream: sys/netinet/ipsec_input.c @ 3ce1f3f79392
//!
//! Status: `ported` (M9c).
//!
//! ## Deviations
//! - `espcounters`, `ahcounters`, `ipcompcounters` and `ipseccounters` (`struct cpumem *`)
//!   are static arrays of atomics; the sysctl variables are `AtomicI32`s.
//! - The packet is `&mut Option<&'static Mbuf>` (`struct mbuf **`); IP headers are read and
//!   written as copies (`mtod_ip`/`mtod_ip_store`). The `IPSEC_ISTAT` macro is a local helper
//!   that picks the counter by security protocol.
//! - `struct tcphdr` is not ported: its size and the offset of its checksum are constants
//!   here, as in `ip_output.rs`.
//! - The `*_ctlinput` functions keep `pr_ctlinput`'s `unsafe` signature: `v` points at the
//!   IP header returned in an ICMP message, which is read unaligned.
//! - `ipsec_forward_check`/`ipsec_local_check` return `Result<(), SpdError>` (any error
//!   drops the packet, as the C's non-zero).
//! - `NPF` (pf(4)) is configured: `pf_test` of transport mode packets, `pf_tag_packet`,
//!   `pf_pkt_addr_changed`, and `PF_TAG_DIVERTED` packets go to raw sockets.
//! - `NBPFILTER` is configured: the `enc(4)` interface of the SA counts a decapsulated
//!   packet, becomes its `ph_ifidx` (but for IPComp) and taps it.
//! - Not configured, each a comment at its site: `NSEC` (`sec(4)`), `INET6` (`in6_cksum`,
//!   `rip6_input`, the IPv6 header chain of `ipsec_protoff`).
//! - `KERNEL_LOCK()`/`KERNEL_UNLOCK()` are nothing without `MULTIPROCESSOR`.

use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_sysctl::{sysctl_bounded_arr, sysctl_rdstruct, sysctl_tstring};
use crate::kern::kern_tc::gettime;
use crate::kern::kern_timeout::timeout_add_sec;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_copyback, m_copydata, m_pullup};
use crate::kern::uipc_mbuf2::{m_tag_find, m_tag_get, m_tag_prepend};
use crate::net::bpf::{BPF_DIRECTION_IN, bpf_mtap_hdr};
use crate::net::if_::{if_get, if_put, unhandled_af};
use crate::net::if_enc::{Enchdr, enc_getif};
use crate::net::if_var::Netstack;
use crate::net::pf::{pf_pkt_addr_changed, pf_tag_packet, pf_test};
use crate::net::pfvar::{PF_IN, PF_PASS};
use crate::net::rtable::rtable_l2;
use crate::netinet::in_::{
    IPCTL_IPSEC_AUTH_ALGORITHM, IPCTL_IPSEC_ENC_ALGORITHM, IPCTL_IPSEC_IPCOMP_ALGORITHM,
    IPCTL_IPSEC_STATS, IPPROTO_AH, IPPROTO_DONE, IPPROTO_DSTOPTS, IPPROTO_ESP, IPPROTO_FRAGMENT,
    IPPROTO_IPCOMP, IPPROTO_IPV4, IPPROTO_IPV6, IPPROTO_ROUTING, IPPROTO_TCP, IPPROTO_UDP, InAddr,
    SockaddrIn,
};
use crate::netinet::in4_cksum::in4_cksum;
use crate::netinet::ip::{IP_MAXPACKET, Ip};
use crate::netinet::ip_ah::{AHCTL_ENABLE, AHCTL_STATS, AhstatCounters, ahstat_inc};
use crate::netinet::ip_esp::{
    ESPCTL_ENABLE, ESPCTL_STATS, ESPCTL_UDPENCAP_ENABLE, ESPCTL_UDPENCAP_PORT, EspstatCounters,
    espstat_inc,
};
use crate::netinet::ip_input::{IP_MTUDISC_TIMEOUT, ip_mtudisc};
use crate::netinet::ip_ipcomp::{
    IPCOMPCTL_ENABLE, IPCOMPCTL_STATS, IpcompCounters, ipcompstat_inc,
};
use crate::netinet::ip_ipsp::{
    IPSEC_ALLOCATIONS, IPSEC_AUTH_HMAC_RIPEMD160, IPSEC_AUTH_HMAC_SHA1, IPSEC_AUTH_MD5,
    IPSEC_AUTH_SHA2_256, IPSEC_AUTH_SHA2_384, IPSEC_AUTH_SHA2_512, IPSEC_BYTES, IPSEC_COMP_DEFLATE,
    IPSEC_DEFAULT_EMBRYONIC_SA_TIMEOUT, IPSEC_DEFAULT_EXP_ALLOCATIONS, IPSEC_DEFAULT_EXP_BYTES,
    IPSEC_DEFAULT_EXP_FIRST_USE, IPSEC_DEFAULT_EXP_TIMEOUT, IPSEC_DEFAULT_EXPIRE_ACQUIRE,
    IPSEC_DEFAULT_PFS, IPSEC_DEFAULT_SOFT_ALLOCATIONS, IPSEC_DEFAULT_SOFT_BYTES,
    IPSEC_DEFAULT_SOFT_FIRST_USE, IPSEC_DEFAULT_SOFT_TIMEOUT, IPSEC_EMBRYONIC_SA_TIMEOUT,
    IPSEC_ENC_3DES, IPSEC_ENC_AES, IPSEC_ENC_AESCTR, IPSEC_ENC_BLOWFISH, IPSEC_ENC_CAST128,
    IPSEC_ENCDEBUG, IPSEC_EXPIRE_ACQUIRE as IPSEC_EXPIRE_ACQUIRE_MIB, IPSEC_FIRSTUSE,
    IPSEC_REQUIRE_PFS as IPSEC_REQUIRE_PFS_MIB,
    IPSEC_SOFT_ALLOCATIONS as IPSEC_SOFT_ALLOCATIONS_MIB, IPSEC_SOFT_BYTES as IPSEC_SOFT_BYTES_MIB,
    IPSEC_SOFT_FIRSTUSE, IPSEC_SOFT_TIMEOUT as IPSEC_SOFT_TIMEOUT_MIB, IPSEC_TIMEOUT,
    IPSP_DIRECTION_IN, IpsecCounters, SockaddrUnion, TDB_SADB_MTX, TDBF_FIRSTUSE, TDBF_IFACE,
    TDBF_INVALID, TDBF_SOFT_FIRSTUSE, TDBF_TUNNELING, TDBF_UDPENCAP, Tdb, TdbCounters, TdbIdent,
    gettdb, gettdb_rev, gettdbbysrcdst_rev, ipsecstat_add, ipsecstat_inc, ipsecstat_pkt,
    ipsp_address, ipsp_init, tdb_ref, tdb_unref, tdbstat_add, tdbstat_inc, tdbstat_pkt,
};
use crate::netinet::ip_output::in_hdr_cksum_out;
use crate::netinet::ip_spd::{SpdError, ipsp_spd_lookup};
use crate::netinet::ip_var::{mtod_ip, mtod_ip_store};
use crate::netinet::ipsec_output::{UDPENCAP_ENABLE, UDPENCAP_PORT};
use crate::netinet::raw_ip::rip_input;
use crate::netinet::udp::Udphdr;
use crate::sys::endian::{htons, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::mbuf::{
    M_AUTH, M_COMP, M_CONF, M_TUNNEL, Mbuf, PACKET_TAG_IPSEC_IN_DONE, PF_TAG_DIVERTED, m_freemp,
};
use crate::sys::protosw::PRC_MSGSIZE;
use crate::sys::socket::{AF_INET, AF_INET6, Sockaddr};
use crate::sys::sysctl::SysctlBoundedArgs;
use crate::sys::systm::net_assert_locked;
use libkern::{strlcpy, strncasecmp, strnlen};

/// `sizeof(struct tcphdr)` (`<netinet/tcp.h>` is not ported).
const SIZEOF_TCPHDR: usize = 20;
/// `offsetof(struct tcphdr, th_sum)`.
const TH_SUM_OFFSET: usize = 16;

// sysctl variables

/// \[a\] `encdebug`.
pub static ENCDEBUG: AtomicI32 = AtomicI32::new(0);
/// \[a\] `ipsec_keep_invalid`: lifetime of embryonic SAs (in sec).
pub static IPSEC_KEEP_INVALID: AtomicI32 = AtomicI32::new(IPSEC_DEFAULT_EMBRYONIC_SA_TIMEOUT);
/// \[a\] `ipsec_require_pfs`: use Perfect Forward Secrecy.
pub static IPSEC_REQUIRE_PFS: AtomicI32 = AtomicI32::new(IPSEC_DEFAULT_PFS);
/// \[a\] `ipsec_soft_allocations`: flows/SA before renegotiation.
pub static IPSEC_SOFT_ALLOCATIONS: AtomicI32 = AtomicI32::new(IPSEC_DEFAULT_SOFT_ALLOCATIONS);
/// \[a\] `ipsec_exp_allocations`: num. of flows/SA before it expires.
pub static IPSEC_EXP_ALLOCATIONS: AtomicI32 = AtomicI32::new(IPSEC_DEFAULT_EXP_ALLOCATIONS);
/// \[a\] `ipsec_soft_bytes`: bytes/SA before renegotiation.
pub static IPSEC_SOFT_BYTES: AtomicI32 = AtomicI32::new(IPSEC_DEFAULT_SOFT_BYTES);
/// \[a\] `ipsec_exp_bytes`: num of bytes/SA before it expires.
pub static IPSEC_EXP_BYTES: AtomicI32 = AtomicI32::new(IPSEC_DEFAULT_EXP_BYTES);
/// \[a\] `ipsec_soft_timeout`: seconds/SA before renegotiation.
pub static IPSEC_SOFT_TIMEOUT: AtomicI32 = AtomicI32::new(IPSEC_DEFAULT_SOFT_TIMEOUT);
/// \[a\] `ipsec_exp_timeout`: seconds/SA before it expires.
pub static IPSEC_EXP_TIMEOUT: AtomicI32 = AtomicI32::new(IPSEC_DEFAULT_EXP_TIMEOUT);
/// \[a\] `ipsec_soft_first_use`: seconds between 1st asso & renego.
pub static IPSEC_SOFT_FIRST_USE: AtomicI32 = AtomicI32::new(IPSEC_DEFAULT_SOFT_FIRST_USE);
/// \[a\] `ipsec_exp_first_use`: seconds between 1st asso & expire.
pub static IPSEC_EXP_FIRST_USE: AtomicI32 = AtomicI32::new(IPSEC_DEFAULT_EXP_FIRST_USE);
/// \[a\] `ipsec_expire_acquire`: wait for security assoc. (in sec).
pub static IPSEC_EXPIRE_ACQUIRE: AtomicI32 = AtomicI32::new(IPSEC_DEFAULT_EXPIRE_ACQUIRE);

/// \[a\] `esp_enable`.
pub static ESP_ENABLE: AtomicI32 = AtomicI32::new(1);
/// \[a\] `ah_enable`.
pub static AH_ENABLE: AtomicI32 = AtomicI32::new(1);
/// \[a\] `ipcomp_enable`.
pub static IPCOMP_ENABLE: AtomicI32 = AtomicI32::new(0);

/// `espctl_vars`.
static ESPCTL_VARS: [SysctlBoundedArgs; 3] = [
    SysctlBoundedArgs::new(ESPCTL_ENABLE, &ESP_ENABLE, 0, 1),
    SysctlBoundedArgs::new(ESPCTL_UDPENCAP_ENABLE, &UDPENCAP_ENABLE, 0, 1),
    SysctlBoundedArgs::new(ESPCTL_UDPENCAP_PORT, &UDPENCAP_PORT, 0, 65535),
];

/// `ahctl_vars`.
static AHCTL_VARS: [SysctlBoundedArgs; 1] =
    [SysctlBoundedArgs::new(AHCTL_ENABLE, &AH_ENABLE, 0, 1)];

/// `ipcompctl_vars`.
static IPCOMPCTL_VARS: [SysctlBoundedArgs; 1] = [SysctlBoundedArgs::new(
    IPCOMPCTL_ENABLE,
    &IPCOMP_ENABLE,
    0,
    1,
)];

/// `esps_ncounters`.
const ESPS_NCOUNTERS: usize = EspstatCounters::EspsNcounters as usize;
/// `ahs_ncounters`.
const AHS_NCOUNTERS: usize = AhstatCounters::AhsNcounters as usize;
/// `ipcomps_ncounters`.
const IPCOMPS_NCOUNTERS: usize = IpcompCounters::IpcompsNcounters as usize;
/// `ipsec_ncounters`.
const IPSEC_NCOUNTERS: usize = IpsecCounters::IpsecNcounters as usize;

/// `espcounters`.
pub static ESPCOUNTERS: [AtomicU64; ESPS_NCOUNTERS] = [const { AtomicU64::new(0) }; ESPS_NCOUNTERS];
/// `ahcounters`.
pub static AHCOUNTERS: [AtomicU64; AHS_NCOUNTERS] = [const { AtomicU64::new(0) }; AHS_NCOUNTERS];
/// `ipcompcounters`.
pub static IPCOMPCOUNTERS: [AtomicU64; IPCOMPS_NCOUNTERS] =
    [const { AtomicU64::new(0) }; IPCOMPS_NCOUNTERS];
/// `ipseccounters`.
pub static IPSECCOUNTERS: [AtomicU64; IPSEC_NCOUNTERS] =
    [const { AtomicU64::new(0) }; IPSEC_NCOUNTERS];

/// `struct ipsec_sysctl_algorithm`.
struct IpsecSysctlAlgorithm {
    /// `name`.
    name: &'static [u8],
    /// `val`.
    val: i32,
}

/// `ipsec_sysctl_enc_algs` (without the C's NULL terminator).
static IPSEC_SYSCTL_ENC_ALGS: [IpsecSysctlAlgorithm; 5] = [
    IpsecSysctlAlgorithm {
        name: b"aes",
        val: IPSEC_ENC_AES,
    },
    IpsecSysctlAlgorithm {
        name: b"aesctr",
        val: IPSEC_ENC_AESCTR,
    },
    IpsecSysctlAlgorithm {
        name: b"3des",
        val: IPSEC_ENC_3DES,
    },
    IpsecSysctlAlgorithm {
        name: b"blowfish",
        val: IPSEC_ENC_BLOWFISH,
    },
    IpsecSysctlAlgorithm {
        name: b"cast128",
        val: IPSEC_ENC_CAST128,
    },
];

/// `ipsec_sysctl_auth_algs`.
static IPSEC_SYSCTL_AUTH_ALGS: [IpsecSysctlAlgorithm; 6] = [
    IpsecSysctlAlgorithm {
        name: b"hmac-sha1",
        val: IPSEC_AUTH_HMAC_SHA1,
    },
    IpsecSysctlAlgorithm {
        name: b"hmac-ripemd160",
        val: IPSEC_AUTH_HMAC_RIPEMD160,
    },
    IpsecSysctlAlgorithm {
        name: b"hmac-md5",
        val: IPSEC_AUTH_MD5,
    },
    IpsecSysctlAlgorithm {
        name: b"hmac-sha2-256",
        val: IPSEC_AUTH_SHA2_256,
    },
    IpsecSysctlAlgorithm {
        name: b"hmac-sha2-384",
        val: IPSEC_AUTH_SHA2_384,
    },
    IpsecSysctlAlgorithm {
        name: b"hmac-sha2-512",
        val: IPSEC_AUTH_SHA2_512,
    },
];

/// `ipsec_sysctl_comp_algs`.
static IPSEC_SYSCTL_COMP_ALGS: [IpsecSysctlAlgorithm; 1] = [IpsecSysctlAlgorithm {
    name: b"deflate",
    val: IPSEC_COMP_DEFLATE,
}];

/// \[a\] `ipsec_def_enc`.
pub static IPSEC_DEF_ENC: AtomicI32 = AtomicI32::new(IPSEC_ENC_AES);
/// \[a\] `ipsec_def_auth`.
pub static IPSEC_DEF_AUTH: AtomicI32 = AtomicI32::new(IPSEC_AUTH_HMAC_SHA1);
/// \[a\] `ipsec_def_comp`.
pub static IPSEC_DEF_COMP: AtomicI32 = AtomicI32::new(IPSEC_COMP_DEFLATE);

/// `ipsecctl_vars`.
static IPSECCTL_VARS: [SysctlBoundedArgs; 12] = [
    SysctlBoundedArgs::new(IPSEC_ENCDEBUG, &ENCDEBUG, 0, 1),
    SysctlBoundedArgs::new(IPSEC_EXPIRE_ACQUIRE_MIB, &IPSEC_EXPIRE_ACQUIRE, 0, i32::MAX),
    SysctlBoundedArgs::new(IPSEC_EMBRYONIC_SA_TIMEOUT, &IPSEC_KEEP_INVALID, 0, i32::MAX),
    SysctlBoundedArgs::new(IPSEC_REQUIRE_PFS_MIB, &IPSEC_REQUIRE_PFS, 0, 1),
    SysctlBoundedArgs::new(
        IPSEC_SOFT_ALLOCATIONS_MIB,
        &IPSEC_SOFT_ALLOCATIONS,
        0,
        i32::MAX,
    ),
    SysctlBoundedArgs::new(IPSEC_ALLOCATIONS, &IPSEC_EXP_ALLOCATIONS, 0, i32::MAX),
    SysctlBoundedArgs::new(IPSEC_SOFT_BYTES_MIB, &IPSEC_SOFT_BYTES, 0, i32::MAX),
    SysctlBoundedArgs::new(IPSEC_BYTES, &IPSEC_EXP_BYTES, 0, i32::MAX),
    SysctlBoundedArgs::new(IPSEC_TIMEOUT, &IPSEC_EXP_TIMEOUT, 0, i32::MAX),
    SysctlBoundedArgs::new(IPSEC_SOFT_TIMEOUT_MIB, &IPSEC_SOFT_TIMEOUT, 0, i32::MAX),
    SysctlBoundedArgs::new(IPSEC_SOFT_FIRSTUSE, &IPSEC_SOFT_FIRST_USE, 0, i32::MAX),
    SysctlBoundedArgs::new(IPSEC_FIRSTUSE, &IPSEC_EXP_FIRST_USE, 0, i32::MAX),
];

/// `ipsec_init`: the counters (static arrays here) and the SA database.
pub fn ipsec_init() {
    ipsp_init();
}

/// `IPSEC_ISTAT(x, y, z)`: bumps the ESP, AH or IPComp counter of `sproto`.
fn ipsec_istat(sproto: i32, x: EspstatCounters, y: AhstatCounters, z: IpcompCounters) {
    if sproto == IPPROTO_ESP {
        espstat_inc(x);
    } else if sproto == IPPROTO_AH {
        ahstat_inc(y);
    } else {
        ipcompstat_inc(z);
    }
}

/// `ipsec_common_input`: called when we receive an IPsec-protected packet in IPv4 or IPv6.
/// All it does is find the right TDB and call the appropriate transform. The callback takes
/// care of further processing (like ingress filtering).
pub fn ipsec_common_input(
    mp: &mut Option<&'static Mbuf>,
    skip: i32,
    protoff: i32,
    af: i32,
    sproto: i32,
    udpencap: bool,
    ns: Option<&Netstack>,
) -> i32 {
    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };
    let mut tdbp: Option<&'static Tdb> = None;

    net_assert_locked("ipsec_common_input");

    ipsecstat_pkt(
        IpsecCounters::IpsecIpackets,
        IpsecCounters::IpsecIbytes,
        m.m_pkthdr().len.get() as u64,
    );
    ipsec_istat(
        sproto,
        EspstatCounters::EspsInput,
        AhstatCounters::AhsInput,
        IpcompCounters::IpcompsInput,
    );

    'drop: {
        if sproto == IPPROTO_IPCOMP && m.m_flags().get() & M_COMP != 0 {
            crate::ipsec_dprintf!("ipsec_common_input", "repeated decompression");
            ipcompstat_inc(IpcompCounters::IpcompsPdrops);
            break 'drop;
        }

        if m.m_pkthdr().len.get() - skip < 2 * 4 {
            crate::ipsec_dprintf!("ipsec_common_input", "packet too small");
            ipsec_istat(
                sproto,
                EspstatCounters::EspsHdrops,
                AhstatCounters::AhsHdrops,
                IpcompCounters::IpcompsHdrops,
            );
            break 'drop;
        }

        // Retrieve the SPI from the relevant IPsec header
        let spi: u32 = match sproto {
            IPPROTO_ESP => {
                let mut b = [0u8; 4];
                m_copydata(m, skip, &mut b);
                u32::from_ne_bytes(b)
            }
            IPPROTO_AH => {
                let mut b = [0u8; 4];
                m_copydata(m, skip + 4, &mut b);
                u32::from_ne_bytes(b)
            }
            IPPROTO_IPCOMP => {
                let mut b = [0u8; 2];
                m_copydata(m, skip + 2, &mut b);
                let cpi = u16::from_ne_bytes(b);
                ntohl(u32::from(htons(cpi)))
            }
            _ => panic(format_args!(
                "ipsec_common_input: unknown/unsupported security protocol {sproto}"
            )),
        };

        // Find tunnel control block and (indirectly) call the appropriate kernel crypto
        // routine. The resulting mbuf chain is a valid IP packet ready to go through input
        // processing.

        let mut dst_address = SockaddrUnion::new();
        dst_address.set_sa_family(af as u8);

        match af {
            x if x == i32::from(AF_INET) => {
                let mut b = [0u8; 4];
                m_copydata(m, offset_of!(Ip, ip_dst) as i32, &mut b);
                dst_address.set_sin(&SockaddrIn {
                    sin_len: size_of::<SockaddrIn>() as u8,
                    sin_family: AF_INET,
                    sin_addr: InAddr {
                        s_addr: u32::from_ne_bytes(b),
                    },
                    ..SockaddrIn::default()
                });
            }
            // INET6: the IPv6 destination (in6_recoverscope); not configured.
            _ => {
                crate::ipsec_dprintf!("ipsec_common_input", "unsupported protocol family {}", af);
                ipsec_istat(
                    sproto,
                    EspstatCounters::EspsNopf,
                    AhstatCounters::AhsNopf,
                    IpcompCounters::IpcompsNopf,
                );
                break 'drop;
            }
        }

        tdbp = gettdb(
            rtable_l2(m.m_pkthdr().ph_rtableid.get()),
            spi,
            &dst_address,
            sproto as u8,
        );
        let Some(t) = tdbp else {
            crate::ipsec_dprintf!(
                "ipsec_common_input",
                "could not find SA for packet to {}, spi {:08x}",
                ipsp_address(&dst_address),
                ntohl(spi)
            );
            ipsec_istat(
                sproto,
                EspstatCounters::EspsNotdb,
                AhstatCounters::AhsNotdb,
                IpcompCounters::IpcompsNotdb,
            );
            break 'drop;
        };

        if t.has_flags(TDBF_INVALID) {
            crate::ipsec_dprintf!(
                "ipsec_common_input",
                "attempted to use invalid SA {}/{:08x}/{}",
                ipsp_address(&dst_address),
                ntohl(spi),
                t.tdb_sproto.get()
            );
            ipsec_istat(
                sproto,
                EspstatCounters::EspsInvalid,
                AhstatCounters::AhsInvalid,
                IpcompCounters::IpcompsInvalid,
            );
            break 'drop;
        }

        if udpencap && !t.has_flags(TDBF_UDPENCAP) {
            crate::ipsec_dprintf!(
                "ipsec_common_input",
                "attempted to use non-udpencap SA {}/{:08x}/{}",
                ipsp_address(&dst_address),
                ntohl(spi),
                t.tdb_sproto.get()
            );
            espstat_inc(EspstatCounters::EspsUdpinval);
            break 'drop;
        }

        if !udpencap && t.has_flags(TDBF_UDPENCAP) {
            crate::ipsec_dprintf!(
                "ipsec_common_input",
                "attempted to use udpencap SA {}/{:08x}/{}",
                ipsp_address(&dst_address),
                ntohl(spi),
                t.tdb_sproto.get()
            );
            espstat_inc(EspstatCounters::EspsUdpneeded);
            break 'drop;
        }

        let Some(xf) = t.tdb_xform.get() else {
            crate::ipsec_dprintf!(
                "ipsec_common_input",
                "attempted to use uninitialized SA {}/{:08x}/{}",
                ipsp_address(&dst_address),
                ntohl(spi),
                t.tdb_sproto.get()
            );
            ipsec_istat(
                sproto,
                EspstatCounters::EspsNoxform,
                AhstatCounters::AhsNoxform,
                IpcompCounters::IpcompsNoxform,
            );
            break 'drop;
        };

        // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
        // Register first use, setup expiration timer.
        if t.tdb_first_use.get() == 0 {
            t.tdb_first_use.set(gettime() as u64);
            if t.has_flags(TDBF_FIRSTUSE)
                && timeout_add_sec(&t.tdb_first_tmo, t.tdb_exp_first_use.get() as i32)
            {
                tdb_ref(Some(t));
            }
            if t.has_flags(TDBF_SOFT_FIRSTUSE)
                && timeout_add_sec(&t.tdb_sfirst_tmo, t.tdb_soft_first_use.get() as i32)
            {
                tdb_ref(Some(t));
            }
        }

        tdbstat_pkt(
            t,
            TdbCounters::TdbIpackets,
            TdbCounters::TdbIbytes,
            m.m_pkthdr().len.get() as u64,
        );

        // Call appropriate transform and return -- callback takes care of everything else.
        let prot = (xf.xf_input)(mp, t, skip, protoff, ns);
        if prot == IPPROTO_DONE {
            ipsecstat_inc(IpsecCounters::IpsecIdrops);
            tdbstat_inc(t, TdbCounters::TdbIdrops);
        }
        tdb_unref(Some(t));
        // KERNEL_UNLOCK()
        return prot;
    }
    // drop:
    m_freemp(mp);
    ipsecstat_inc(IpsecCounters::IpsecIdrops);
    if let Some(t) = tdbp {
        tdbstat_inc(t, TdbCounters::TdbIdrops);
    }
    tdb_unref(tdbp);
    IPPROTO_DONE
}

/// `ipsec_common_input_cb`: IPsec input callback, called by the transform callback. Takes
/// care of filtering and other sanity checks on the processed packet.
pub fn ipsec_common_input_cb(
    mp: &mut Option<&'static Mbuf>,
    tdbp: &'static Tdb,
    skip: i32,
    _protoff: i32,
    _ns: Option<&Netstack>,
) -> i32 {
    let Some(mut m) = *mp else {
        return IPPROTO_DONE;
    };

    let af = tdbp.tdb_dst.get().sa_family();
    let sproto = i32::from(tdbp.tdb_sproto.get());
    let istat = |x, y, z| ipsec_istat(sproto, x, y, z);

    tdbp.tdb_last_used.set(gettime() as u64);

    'baddone: {
        let mut prot: u8 = 0;

        // Fix IPv4 header
        if af == AF_INET {
            if (m.m_len().get() as i32) < skip {
                *mp = m_pullup(m, skip);
                match *mp {
                    Some(mm) => m = mm,
                    None => {
                        crate::ipsec_dprintf!(
                            "ipsec_common_input_cb",
                            "processing failed for SA {}/{:08x}",
                            ipsp_address(&tdbp.tdb_dst.get()),
                            ntohl(tdbp.tdb_spi.get())
                        );
                        istat(
                            EspstatCounters::EspsHdrops,
                            AhstatCounters::AhsHdrops,
                            IpcompCounters::IpcompsHdrops,
                        );
                        break 'baddone;
                    }
                }
            }
            if m.m_pkthdr().len.get() > IP_MAXPACKET as i32 {
                istat(
                    EspstatCounters::EspsToobig,
                    AhstatCounters::AhsToobig,
                    IpcompCounters::IpcompsToobig,
                );
                break 'baddone;
            }

            let mut ip = mtod_ip(m);
            ip.ip_len = htons(m.m_pkthdr().len.get() as u16);
            mtod_ip_store(m, &ip);
            in_hdr_cksum_out(m, None);
            prot = ip.ip_p;
        }

        // INET6: fix the IPv6 header (ip6_plen, the next header at protoff); not configured.

        // Fix TCP/UDP checksum of UDP encapsulated transport mode ESP packet. (RFC3948
        // 3.1.2)
        if (af == AF_INET || af == AF_INET6)
            && tdbp.has_flags(TDBF_UDPENCAP)
            && !tdbp.has_flags(TDBF_TUNNELING)
        {
            match i32::from(prot) {
                IPPROTO_UDP => {
                    if (m.m_pkthdr().len.get() as usize) < skip as usize + size_of::<Udphdr>() {
                        istat(
                            EspstatCounters::EspsHdrops,
                            AhstatCounters::AhsHdrops,
                            IpcompCounters::IpcompsHdrops,
                        );
                        break 'baddone;
                    }
                    let cksum: u16 = 0;
                    let _ = m_copyback(
                        m,
                        skip + offset_of!(Udphdr, uh_sum) as i32,
                        &cksum.to_ne_bytes(),
                        M_NOWAIT,
                    );
                    // INET6: in6_cksum of the UDP datagram; not configured.
                }
                IPPROTO_TCP => {
                    if (m.m_pkthdr().len.get() as usize) < skip as usize + SIZEOF_TCPHDR {
                        istat(
                            EspstatCounters::EspsHdrops,
                            AhstatCounters::AhsHdrops,
                            IpcompCounters::IpcompsHdrops,
                        );
                        break 'baddone;
                    }
                    let mut cksum: u16 = 0;
                    let _ = m_copyback(
                        m,
                        skip + TH_SUM_OFFSET as i32,
                        &cksum.to_ne_bytes(),
                        M_NOWAIT,
                    );
                    if af == AF_INET {
                        cksum =
                            in4_cksum(m, IPPROTO_TCP as u8, skip, m.m_pkthdr().len.get() - skip);
                    }
                    // INET6: in6_cksum of the TCP segment; not configured.
                    let _ = m_copyback(
                        m,
                        skip + TH_SUM_OFFSET as i32,
                        &cksum.to_ne_bytes(),
                        M_NOWAIT,
                    );
                }
                _ => {}
            }
        }

        // Record what we've done to the packet (under what SA it was processed).
        if sproto != IPPROTO_IPCOMP {
            let Some(mtag) = m_tag_get(
                PACKET_TAG_IPSEC_IN_DONE,
                size_of::<TdbIdent>() as i32,
                M_NOWAIT,
            ) else {
                crate::ipsec_dprintf!("ipsec_common_input_cb", "failed to get tag");
                istat(
                    EspstatCounters::EspsHdrops,
                    AhstatCounters::AhsHdrops,
                    IpcompCounters::IpcompsHdrops,
                );
                break 'baddone;
            };

            // SAFETY: the tag was allocated with `size_of::<TdbIdent>()` bytes of data.
            unsafe { TdbIdent::of(tdbp).write(mtag.data()) };

            m_tag_prepend(m, mtag);
        }

        match sproto {
            IPPROTO_ESP => {
                // Packet is confidential ?
                if tdbp.tdb_encalgxform.get().is_some() {
                    m.m_flags().set(m.m_flags().get() | M_CONF);
                }

                // Check if we had authenticated ESP.
                if tdbp.tdb_authalgxform.get().is_some() {
                    m.m_flags().set(m.m_flags().get() | M_AUTH);
                }
            }
            IPPROTO_AH => m.m_flags().set(m.m_flags().get() | M_AUTH),
            IPPROTO_IPCOMP => m.m_flags().set(m.m_flags().get() | M_COMP),
            _ => panic(format_args!(
                "ipsec_common_input_cb: unknown/unsupported security protocol {sproto}"
            )),
        }

        // Add pf tag if requested.
        pf_tag_packet(m, i32::from(tdbp.tdb_tag.get()), -1);
        pf_pkt_addr_changed(m);
        if tdbp.tdb_rdomain.get() != tdbp.tdb_rdomain_post.get() {
            m.m_pkthdr().ph_rtableid.set(tdbp.tdb_rdomain_post.get());
        }

        if tdbp.has_flags(TDBF_TUNNELING) {
            m.m_flags().set(m.m_flags().get() | M_TUNNEL);
        }

        ipsecstat_add(
            IpsecCounters::IpsecIdecompbytes,
            m.m_pkthdr().len.get() as u64,
        );
        tdbstat_add(
            tdbp,
            TdbCounters::TdbIdecompbytes,
            m.m_pkthdr().len.get() as u64,
        );

        if let Some(encif) = enc_getif(tdbp.tdb_rdomain_post.get(), tdbp.tdb_tap.get()) {
            encif.if_ipackets().set(encif.if_ipackets().get() + 1);
            encif
                .if_ibytes()
                .set(encif.if_ibytes().get() + m.m_pkthdr().len.get() as u64);

            if sproto != IPPROTO_IPCOMP {
                // XXX This conflicts with the scoped nature of IPv6
                m.m_pkthdr().ph_ifidx.set(encif.if_index.get());
            }
            let if_bpf = encif.if_bpf.get();
            if !if_bpf.is_null() {
                let hdr = Enchdr {
                    af: u32::from(af).to_be(),
                    spi: tdbp.tdb_spi.get(),
                    flags: u32::from(m.m_flags().get() & (M_AUTH | M_CONF)).to_be(),
                };

                let _ = bpf_mtap_hdr(if_bpf, &hdr.to_bytes(), m, BPF_DIRECTION_IN);
            }
        }

        if tdbp.has_flags(TDBF_IFACE) {
            // NSEC > 0: sec_input of a tunnel mode packet on its sec(4) interface; not
            // configured.
            break 'baddone;
        }

        // The ip_deliver() shortcut avoids running through ip_input() with the same IP
        // header twice. Packets in transport mode have to be be passed to pf explicitly. In
        // tunnel mode the inner IP header will run through ip_input() and pf anyway.
        if !tdbp.has_flags(TDBF_TUNNELING) {
            // This is the enc0 interface unless for ipcomp.
            let Some(ifp) = if_get(m.m_pkthdr().ph_ifidx.get()) else {
                break 'baddone;
            };
            if pf_test(af, PF_IN, ifp, mp) != PF_PASS {
                if_put(Some(ifp));
                break 'baddone;
            }
            if_put(Some(ifp));
            if mp.is_none() {
                return IPPROTO_DONE;
            }
        }

        // Return to the appropriate protocol handler in deliver loop.
        return i32::from(prot);
    }
    // baddone:
    m_freemp(mp);
    IPPROTO_DONE
}

/// `ipsec_sysctl`: `net.inet.ip.ipsec-*` and `net.inet.ip.encdebug`.
pub fn ipsec_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let Some(&name0) = name.first() else {
        return Err(Errno::ENOTDIR);
    };
    match name0 {
        IPCTL_IPSEC_ENC_ALGORITHM | IPCTL_IPSEC_AUTH_ALGORITHM | IPCTL_IPSEC_IPCOMP_ALGORITHM => {
            ipsec_sysctl_algorithm(name0, oldp, oldlenp, newp, newlen)
        }
        IPCTL_IPSEC_STATS => ipsec_sysctl_ipsecstat(oldp, oldlenp, newp),
        _ => sysctl_bounded_arr(&IPSECCTL_VARS, name, oldp, oldlenp, newp, newlen),
    }
}

/// `ipsec_sysctl_algorithm`: the default cipher, authenticator or compressor, by name.
fn ipsec_sysctl_algorithm(
    name: i32,
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let mut buf = [0u8; 20];

    let (algs, var): (&[IpsecSysctlAlgorithm], &AtomicI32) = match name {
        IPCTL_IPSEC_ENC_ALGORITHM => (&IPSEC_SYSCTL_ENC_ALGS, &IPSEC_DEF_ENC),
        IPCTL_IPSEC_AUTH_ALGORITHM => (&IPSEC_SYSCTL_AUTH_ALGS, &IPSEC_DEF_AUTH),
        IPCTL_IPSEC_IPCOMP_ALGORITHM => (&IPSEC_SYSCTL_COMP_ALGS, &IPSEC_DEF_COMP),
        _ => return Err(Errno::EOPNOTSUPP),
    };

    let oldval = var.load(Ordering::Relaxed);

    let p = algs.iter().find(|p| p.val == oldval);
    kassert!(p.is_some());
    if let Some(p) = p {
        strlcpy(&mut buf, p.name);
    }

    sysctl_tstring(oldp, oldlenp, newp, newlen, &mut buf)?;

    if newp != 0 {
        let buflen = strnlen(&buf, buf.len());
        if buflen == 0 {
            return Err(Errno::EINVAL);
        }

        let Some(p) = algs.iter().find(|p| strncasecmp(&buf, p.name, buflen) == 0) else {
            return Err(Errno::EINVAL);
        };

        if p.val != oldval {
            var.store(p.val, Ordering::Relaxed);
        }
    }

    Ok(())
}

/// The counters of `c` as the bytes of their C structure.
fn counters_bytes<const N: usize>(c: &[AtomicU64; N], bytes: &mut [u8]) {
    for (i, c) in c.iter().enumerate() {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&c.load(Ordering::Relaxed).to_ne_bytes());
    }
}

/// `esp_sysctl`: `net.inet.esp`.
pub fn esp_sysctl(
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
        ESPCTL_STATS => esp_sysctl_espstat(oldp, oldlenp, newp),
        _ => sysctl_bounded_arr(&ESPCTL_VARS, name, oldp, oldlenp, newp, newlen),
    }
}

/// `esp_sysctl_espstat`.
fn esp_sysctl_espstat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    let mut bytes = [0u8; ESPS_NCOUNTERS * 8];
    counters_bytes(&ESPCOUNTERS, &mut bytes);
    sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
}

/// `ah_sysctl`: `net.inet.ah`.
pub fn ah_sysctl(
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
        AHCTL_STATS => ah_sysctl_ahstat(oldp, oldlenp, newp),
        _ => sysctl_bounded_arr(&AHCTL_VARS, name, oldp, oldlenp, newp, newlen),
    }
}

/// `ah_sysctl_ahstat`.
fn ah_sysctl_ahstat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    let mut bytes = [0u8; AHS_NCOUNTERS * 8];
    counters_bytes(&AHCOUNTERS, &mut bytes);
    sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
}

/// `ipcomp_sysctl`: `net.inet.ipcomp`.
pub fn ipcomp_sysctl(
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
        IPCOMPCTL_STATS => ipcomp_sysctl_ipcompstat(oldp, oldlenp, newp),
        _ => sysctl_bounded_arr(&IPCOMPCTL_VARS, name, oldp, oldlenp, newp, newlen),
    }
}

/// `ipcomp_sysctl_ipcompstat`.
fn ipcomp_sysctl_ipcompstat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    let mut bytes = [0u8; IPCOMPS_NCOUNTERS * 8];
    counters_bytes(&IPCOMPCOUNTERS, &mut bytes);
    sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
}

/// `ipsec_sysctl_ipsecstat`.
fn ipsec_sysctl_ipsecstat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    let mut bytes = [0u8; IPSEC_NCOUNTERS * 8];
    counters_bytes(&IPSECCOUNTERS, &mut bytes);
    sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
}

/// `ipsec_input_disabled`: a disabled protocol's packets go to raw sockets.
pub fn ipsec_input_disabled(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    match af {
        x if x == i32::from(AF_INET) => rip_input(mp, offp, proto, af, ns),
        // INET6: rip6_input; not configured.
        _ => unhandled_af(af),
    }
}

/// `(*mp)->m_pkthdr.pf.flags & PF_TAG_DIVERTED`: pf(4) diverted the packet to a socket.
fn pf_diverted(mp: &Option<&'static Mbuf>) -> bool {
    mp.is_some_and(|m| m.m_pkthdr().pf.flags.get() & PF_TAG_DIVERTED != 0)
}

/// `ah46_input`: the protocol switch's AH input.
pub fn ah46_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    if pf_diverted(mp) || AH_ENABLE.load(Ordering::Relaxed) == 0 {
        return ipsec_input_disabled(mp, offp, proto, af, ns);
    }

    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };
    let protoff = ipsec_protoff(m, *offp, af);
    if protoff < 0 {
        crate::ipsec_dprintf!("ah46_input", "bad packet header chain");
        ahstat_inc(AhstatCounters::AhsHdrops);
        m_freemp(mp);
        return IPPROTO_DONE;
    }

    ipsec_common_input(mp, *offp, protoff, af, proto, false, ns)
}

/// `ah4_ctlinput`: ICMP feedback for AH.
///
/// # Safety
///
/// `pr_ctlinput`'s contract: `sa` is a socket address of its `sa_len`; `v` is NULL or the IP
/// header returned in an ICMP message, readable with what follows it (see
/// `ipsec_common_ctlinput`).
pub unsafe fn ah4_ctlinput(cmd: i32, sa: *const Sockaddr, rdomain: u32, v: *mut c_void) {
    // SAFETY: the caller's contract.
    let (family, len) = unsafe { ((*sa).sa_family, (*sa).sa_len) };
    if family != AF_INET || usize::from(len) != size_of::<SockaddrIn>() {
        return;
    }

    // SAFETY: the caller's contract.
    unsafe { ipsec_common_ctlinput(rdomain, cmd, sa, v, IPPROTO_AH) };
}

/// `esp46_input`: the protocol switch's ESP input.
pub fn esp46_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    if pf_diverted(mp) || ESP_ENABLE.load(Ordering::Relaxed) == 0 {
        return ipsec_input_disabled(mp, offp, proto, af, ns);
    }

    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };
    let protoff = ipsec_protoff(m, *offp, af);
    if protoff < 0 {
        crate::ipsec_dprintf!("esp46_input", "bad packet header chain");
        espstat_inc(EspstatCounters::EspsHdrops);
        m_freemp(mp);
        return IPPROTO_DONE;
    }

    ipsec_common_input(mp, *offp, protoff, af, proto, false, ns)
}

/// `ipcomp46_input`: IPv4 IPCOMP wrapper.
pub fn ipcomp46_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    if pf_diverted(mp) || IPCOMP_ENABLE.load(Ordering::Relaxed) == 0 {
        return ipsec_input_disabled(mp, offp, proto, af, ns);
    }

    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };
    let protoff = ipsec_protoff(m, *offp, af);
    if protoff < 0 {
        crate::ipsec_dprintf!("ipcomp46_input", "bad packet header chain");
        ipcompstat_inc(IpcompCounters::IpcompsHdrops);
        m_freemp(mp);
        return IPPROTO_DONE;
    }

    ipsec_common_input(mp, *offp, protoff, af, proto, false, ns)
}

/// `ipsec_set_mtu`: stores the path MTU `mtu`, less each SA's overhead, in the TDB and the
/// ones bundled before it.
pub fn ipsec_set_mtu(tdbp: Option<&'static Tdb>, mtu: u32) {
    net_assert_locked("ipsec_set_mtu");

    let mut mtu = mtu;
    // Walk the chain backwards to the first tdb
    let mut tdbp = tdbp;
    while let Some(t) = tdbp {
        if t.has_flags(TDBF_INVALID) {
            return;
        }
        let adjust = crate::netinet::ipsec_output::ipsec_hdrsz(t);
        if adjust == -1 {
            return;
        }

        mtu = mtu.wrapping_sub(adjust as u32);

        // Store adjusted MTU in tdb
        t.tdb_mtu.set(mtu);
        t.tdb_mtutimeout
            .set(gettime() as u64 + IP_MTUDISC_TIMEOUT.load(Ordering::Relaxed) as u64);
        crate::ipsec_dprintf!(
            "ipsec_set_mtu",
            "spi {:08x} mtu {} adjust {}",
            ntohl(t.tdb_spi.get()),
            t.tdb_mtu.get(),
            adjust
        );
        tdbp = t.tdb_inext.get();
    }
}

/// `offsetof(struct icmp, icmp_ip)`: where the returned IP header starts in an ICMP message.
const ICMP_IP_OFFSET: usize = 8;
/// `offsetof(struct icmp, icmp_nextmtu)`.
const ICMP_NEXTMTU_OFFSET: usize = 6;

/// The `icmp_nextmtu` of the ICMP message whose returned IP header is at `ip`.
///
/// # Safety
///
/// `ip` points at the `icmp_ip` member of an ICMP message in readable memory.
unsafe fn icmp_nextmtu(ip: *const u8) -> u16 {
    // SAFETY: the caller's contract: the message's start is `ICMP_IP_OFFSET` bytes before.
    unsafe {
        let icp = ip.sub(ICMP_IP_OFFSET);
        ntohs(ptr::read_unaligned(
            icp.add(ICMP_NEXTMTU_OFFSET).cast::<u16>(),
        ))
    }
}

/// `ipsec_common_ctlinput`: a "fragmentation needed" for an IPsec packet lowers the MTU of
/// the SA it went out through.
///
/// # Safety
///
/// `v` is NULL or the `icmp_ip` of an ICMP message, readable for the IP header, its options
/// and the 4 bytes of the SPI after them.
unsafe fn ipsec_common_ctlinput(
    rdomain: u32,
    cmd: i32,
    _sa: *const Sockaddr,
    v: *mut c_void,
    proto: i32,
) {
    if v.is_null() || cmd != PRC_MSGSIZE || ip_mtudisc.load(Ordering::Relaxed) == 0 {
        return;
    }
    // SAFETY: the caller's contract.
    let ip: Ip = unsafe { ptr::read_unaligned(v.cast::<Ip>()) };
    if ip.ip_v() != 4 {
        return;
    }

    let hlen = usize::from(ip.ip_hl()) << 2;

    // Find the right MTU.
    // SAFETY: the caller's contract.
    let mtu = u32::from(unsafe { icmp_nextmtu(v.cast::<u8>()) });

    // Ignore the packet, if we do not receive a MTU or the MTU is too small to be
    // acceptable.
    if mtu < 296 {
        return;
    }

    let dst = SockaddrUnion::from_sin(&SockaddrIn {
        sin_family: AF_INET,
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_addr: ip.ip_dst,
        ..SockaddrIn::default()
    });

    // SAFETY: the caller's contract: the SPI follows the returned header.
    let spi = unsafe { ptr::read_unaligned(v.cast::<u8>().add(hlen).cast::<u32>()) };

    let tdbp = gettdb_rev(rdomain, spi, &dst, proto as u8);
    ipsec_set_mtu(tdbp, mtu);
    tdb_unref(tdbp);
}

/// `udpencap_ctlinput`: the same for ESP in UDP: every UDP-encapsulated SA between the two
/// addresses.
///
/// # Safety
///
/// As for [`ah4_ctlinput`]; `v` is not NULL.
pub unsafe fn udpencap_ctlinput(_cmd: i32, _sa: *const Sockaddr, rdomain: u32, v: *mut c_void) {
    net_assert_locked("udpencap_ctlinput");

    // SAFETY: the caller's contract.
    let ip: Ip = unsafe { ptr::read_unaligned(v.cast::<Ip>()) };
    // SAFETY: the caller's contract.
    let mtu = u32::from(unsafe { icmp_nextmtu(v.cast::<u8>()) });

    // Ignore the packet, if we do not receive a MTU or the MTU is too small to be
    // acceptable.
    if mtu < 296 {
        return;
    }

    let su_dst = SockaddrUnion::from_sin(&SockaddrIn {
        sin_family: AF_INET,
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_addr: ip.ip_dst,
        ..SockaddrIn::default()
    });
    let su_src = SockaddrUnion::from_sin(&SockaddrIn {
        sin_family: AF_INET,
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_addr: ip.ip_src,
        ..SockaddrIn::default()
    });

    let first = gettdbbysrcdst_rev(rdomain, 0, &su_src, &su_dst, IPPROTO_ESP as u8);

    mtx_enter(&TDB_SADB_MTX);
    let mut tdbp = first;
    while let Some(t) = tdbp {
        let ds = su_dst.sa_bytes().len();
        let ss = su_src.sa_bytes().len();
        if i32::from(t.tdb_sproto.get()) == IPPROTO_ESP
            && t.tdb_flags.get() & (TDBF_INVALID | TDBF_UDPENCAP) == TDBF_UDPENCAP
            && t.tdb_dst.get().as_bytes()[..ds] == su_dst.as_bytes()[..ds]
            && t.tdb_src.get().as_bytes()[..ss] == su_src.as_bytes()[..ss]
        {
            ipsec_set_mtu(Some(t), mtu);
        }
        tdbp = t.tdb_snext.get();
    }
    mtx_leave(&TDB_SADB_MTX);
    tdb_unref(first);
}

/// `esp4_ctlinput`: ICMP feedback for ESP.
///
/// # Safety
///
/// As for [`ah4_ctlinput`].
pub unsafe fn esp4_ctlinput(cmd: i32, sa: *const Sockaddr, rdomain: u32, v: *mut c_void) {
    // SAFETY: the caller's contract.
    let (family, len) = unsafe { ((*sa).sa_family, (*sa).sa_len) };
    if family != AF_INET || usize::from(len) != size_of::<SockaddrIn>() {
        return;
    }

    // SAFETY: the caller's contract.
    unsafe { ipsec_common_ctlinput(rdomain, cmd, sa, v, IPPROTO_ESP) };
}

/// `ipsec_protoff`: find the offset of the next protocol field in the previous header.
pub fn ipsec_protoff(_m: &Mbuf, _off: i32, af: i32) -> i32 {
    match af {
        x if x == i32::from(AF_INET) => offset_of!(Ip, ip_p) as i32,
        // INET6: chase the IPv6 extension header chain; not configured.
        _ => unhandled_af(af),
    }
}

/// The SA an `IPSEC_IN_DONE` tag of `m` names (the inner-most one), with a reference.
fn tagged_tdb(m: &Mbuf) -> Option<&'static Tdb> {
    let mtag = m_tag_find(m, PACKET_TAG_IPSEC_IN_DONE, None)?;
    // SAFETY: `IPSEC_IN_DONE` tags carry a `struct tdb_ident` (`ipsec_common_input_cb`).
    let tdbi = unsafe { TdbIdent::read(mtag.data()) };
    gettdb(tdbi.rdomain, tdbi.spi, &tdbi.dst, tdbi.proto)
}

/// `ipsec_forward_check`: IPsec policy check for forwarded packets. Look at inner-most IPsec
/// SA used.
pub fn ipsec_forward_check(m: &Mbuf, hlen: i32, af: i32) -> Result<(), SpdError> {
    let tdb = tagged_tdb(m);
    let error = ipsp_spd_lookup(m, af, hlen, IPSP_DIRECTION_IN, tdb, None, None, None);
    tdb_unref(tdb);

    error
}

/// `ipsec_local_check`: the inbound policy check of a packet delivered locally.
pub fn ipsec_local_check(m: &Mbuf, hlen: i32, proto: i32, af: i32) -> Result<(), SpdError> {
    // If it's a protected packet for us, skip the policy check. That's because we really
    // only care about the properties of the protected packet, and not the intermediate
    // versions. While this is not the most paranoid setting, it allows some flexibility in
    // handling nested tunnels (in setting up the policies).
    if proto == IPPROTO_ESP || proto == IPPROTO_AH || proto == IPPROTO_IPCOMP {
        return Ok(());
    }

    // If the protected packet was tunneled, then we need to verify the protected packet's
    // information, not the external headers. Thus, skip the policy lookup for the external
    // packet, and keep the IPsec information linked on the packet header (the encapsulation
    // routines know how to deal with that).
    if proto == IPPROTO_IPV4 || proto == IPPROTO_IPV6 {
        return Ok(());
    }

    // When processing IPv6 header chains, do not look at the outer header. The inner
    // protocol is relevant and will be checked by the local delivery loop later.
    if af == i32::from(AF_INET6)
        && (proto == IPPROTO_DSTOPTS || proto == IPPROTO_ROUTING || proto == IPPROTO_FRAGMENT)
    {
        return Ok(());
    }

    // If the protected packet is TCP or UDP, we'll do the policy check in the respective
    // input routine, so we can check for bypass sockets.
    if proto == IPPROTO_TCP || proto == IPPROTO_UDP {
        return Ok(());
    }

    // IPsec policy check for local-delivery packets. Look at the inner-most SA that protected
    // the packet. This is in fact a bit too restrictive (it could end up causing packets to
    // be dropped that semantically follow the policy, e.g., in certain SA-bundle
    // configurations); but the alternative is very complicated (and requires keeping track
    // of what kinds of tunneling headers have been seen in-between the IPsec headers), and I
    // don't think we lose much functionality that's needed in the real world (who uses
    // bundles anyway ?).
    let tdb = tagged_tdb(m);
    let error = ipsp_spd_lookup(m, af, hlen, IPSP_DIRECTION_IN, tdb, None, None, None);
    tdb_unref(tdb);

    error
}

// LP64 sizes of the C structures the statistics are copied out as.
const _: () = {
    assert!(size_of::<crate::netinet::ip_esp::Espstat>() == ESPS_NCOUNTERS * 8);
    assert!(size_of::<crate::netinet::ip_ah::Ahstat>() == AHS_NCOUNTERS * 8);
    assert!(size_of::<crate::netinet::ip_ipcomp::Ipcompstat>() == IPCOMPS_NCOUNTERS * 8);
    assert!(size_of::<crate::netinet::ip_ipsp::Ipsecstat>() == IPSEC_NCOUNTERS * 8);
};
