/* <LICENSES> */
/*
 * Copyright (c) 2001 Jean-Jacques Bernard-Gundol (jj@wabbitt.org)
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *   notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *   notice, this list of conditions and the following disclaimer in the
 *   documentation and/or other materials provided with the distribution.
 * 3. The name of the author may not be used to endorse or promote products
 *   derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! IP payload compression protocol (IPComp), see RFC 2393: `<netinet/ip_ipcomp.h>` (the
//! statistics, the header, the sysctl names) and the transform of `netinet/ip_ipcomp.c`, which
//! `xformsw[]` names: `ipcomp_init` sets up a TDB with a deflate session of the crypto
//! framework, `ipcomp_output` compresses the payload after `skip` and inserts the IPComp
//! header (or sends the packet as it is when compression would not shrink it), and
//! `ipcomp_input` decompresses and removes the header before `ipsec_common_input_cb`.
//!
//! Upstream: sys/netinet/ip_ipcomp.h @ 3ce1f3f79392
//! Upstream: sys/netinet/ip_ipcomp.c @ 3ce1f3f79392
//!
//! The two files carry the same licence, kept once above. The crypto framework is
//! synchronous at this pin: both directions call `crypto_invoke` and go on with the result.
//!
//! ## Deviations
//! - `ipcompcounters` (`struct cpumem *`) is the static array of atomics `IPCOMPCOUNTERS` in
//!   `netinet/ipsec_input.rs`, which defines the C's pointer; `ipcomp_enable` is
//!   `IPCOMP_ENABLE` there too (`net.inet.ipcomp.enable`, 0 by default as in the C).
//! - The crypto request is the framework's [`Cryptop`] value, as in `ip_esp.rs`; the `EAGAIN`
//!   retry loop is `ipsec_crypto_invoke`.
//! - The IPComp header removal of `ipcomp_input` is `esp_strip_header` of `ip_esp.rs`, the
//!   same three cases (header at the start of an mbuf, straddling two, in the middle) as the
//!   C's copy of that code. The next-protocol byte is read with `m_copydata` after the
//!   `m_pullup` (the C reads it through `mtod`).
//! - A TDB whose `tdb_compalgxform` is not set drops the packet and counts `ipsec_noxform`
//!   (the C dereferences it; `ipcomp_init` always sets it first).
//! - `NBPFILTER` is configured: `ipcomp_output` counts the packet on the SA's `enc(4)`
//!   interface (rdomain 0, as the C asks) and taps it.
//! - `INET6` is configured (feature `inet6`): the IPv6 size check and `ip6_nxt` in
//!   `ipcomp_output`. `ENCDEBUG`'s `DPRINTF` is `ipsec_dprintf!`.

use core::ptr;
use core::sync::atomic::Ordering;

use crate::crypto::crypto::{crypto_freereq, crypto_freesession, crypto_getreq, crypto_newsession};
use crate::crypto::cryptodev::{CRD_F_COMP, CRYPTO_F_IMBUF, CryptoBuf, Cryptoini, Cryptop};
use crate::crypto::xform::{CompAlgo, comp_algo_deflate};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::uipc_mbuf::{
    m_copyback, m_copydata, m_dup_pkt, m_freem, m_getptr, m_makespace, m_pullup,
};
use crate::net::bpf::{BPF_DIRECTION_OUT, bpf_mtap_hdr};
use crate::net::if_enc::{Enchdr, enc_getif};
use crate::net::if_var::Netstack;
use crate::net::pfkeyv2::{
    SADB_EXT_LIFETIME_HARD, SADB_EXT_LIFETIME_SOFT, SADB_X_CALG_DEFLATE, pfkeyv2_expire,
};
use crate::netinet::in_::{IPPROTO_DONE, IPPROTO_IPCOMP};
use crate::netinet::ip::IP_MAXPACKET;
use crate::netinet::ip_esp::{esp_strip_header, ipsec_crypto_invoke};
use crate::netinet::ip_ipsp::{
    IpsecCounters, IpsecInit, TDBF_BYTES, TDBF_SOFT_BYTES, Tdb, TdbCounters, Xformsw,
    ipsecstat_inc, ipsp_address, tdb_delete, tdbstat_add,
};
use crate::netinet::ip_var::{mtod_ip, mtod_ip_store};
#[cfg(feature = "inet6")]
use crate::netinet::ip6::IPV6_MAXPACKET;
use crate::netinet::ipsec_input::{IPCOMPCOUNTERS, ipsec_common_input_cb};
use crate::netinet::ipsec_output::ipsp_process_done;
#[cfg(feature = "inet6")]
use crate::netinet6::ip6_var::{mtod_ip6, mtod_ip6_store};
use crate::sys::endian::{htons, ntohl};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::mbuf::{M_DONTWAIT, Mbuf, m_freemp, m_readonly, mtod};
use crate::sys::socket::AF_INET;
#[cfg(feature = "inet6")]
use crate::sys::socket::AF_INET6;
use crate::sys::systm::{kernel_lock, kernel_unlock};

/// `struct ipcompstat`: the IPComp statistics as `net.inet.ipcomp.stats` returns them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Ipcompstat {
    /// Packet shorter than header shows.
    pub ipcomps_hdrops: u64,
    /// Protocol family not supported.
    pub ipcomps_nopf: u64,
    /// `ipcomps_notdb`.
    pub ipcomps_notdb: u64,
    /// `ipcomps_badkcr`.
    pub ipcomps_badkcr: u64,
    /// `ipcomps_qfull`.
    pub ipcomps_qfull: u64,
    /// `ipcomps_noxform`.
    pub ipcomps_noxform: u64,
    /// `ipcomps_wrap`.
    pub ipcomps_wrap: u64,
    /// Input IPcomp packets.
    pub ipcomps_input: u64,
    /// Output IPcomp packets.
    pub ipcomps_output: u64,
    /// Trying to use an invalid TDB.
    pub ipcomps_invalid: u64,
    /// Input bytes.
    pub ipcomps_ibytes: u64,
    /// Output bytes.
    pub ipcomps_obytes: u64,
    /// Packet got larger than `IP_MAXPACKET`.
    pub ipcomps_toobig: u64,
    /// Packet blocked due to policy.
    pub ipcomps_pdrops: u64,
    /// "Crypto" processing failure.
    pub ipcomps_crypto: u64,
    /// Packets too short for compress.
    pub ipcomps_minlen: u64,
    /// Packet output failure.
    pub ipcomps_outfail: u64,
}

/// `struct ipcomp`: the IPCOMP header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ipcomp {
    /// Next header.
    pub ipcomp_nh: u8,
    /// Flags: reserved field: 0.
    pub ipcomp_flags: u8,
    /// Compression Parameter Index, network order.
    pub ipcomp_cpi: u16,
}

/// `IPCOMP_HLENGTH`: length of IPCOMP header.
pub const IPCOMP_HLENGTH: usize = 4;

// Names for IPCOMP sysctl objects

/// `IPCOMPCTL_ENABLE`: enable COMP processing.
pub const IPCOMPCTL_ENABLE: i32 = 1;
/// `IPCOMPCTL_STATS`: COMP stats.
pub const IPCOMPCTL_STATS: i32 = 2;
/// `IPCOMPCTL_MAXID`.
pub const IPCOMPCTL_MAXID: i32 = 3;

/// `enum ipcomp_counters`: one per field of [`Ipcompstat`], in the same order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum IpcompCounters {
    /// Packet shorter than header shows.
    IpcompsHdrops,
    /// Protocol family not supported.
    IpcompsNopf,
    /// `ipcomps_notdb`.
    IpcompsNotdb,
    /// `ipcomps_badkcr`.
    IpcompsBadkcr,
    /// `ipcomps_qfull`.
    IpcompsQfull,
    /// `ipcomps_noxform`.
    IpcompsNoxform,
    /// `ipcomps_wrap`.
    IpcompsWrap,
    /// Input IPcomp packets.
    IpcompsInput,
    /// Output IPcomp packets.
    IpcompsOutput,
    /// Trying to use an invalid TDB.
    IpcompsInvalid,
    /// Input bytes.
    IpcompsIbytes,
    /// Output bytes.
    IpcompsObytes,
    /// Packet got larger than `IP_MAXPACKET`.
    IpcompsToobig,
    /// Packet blocked due to policy.
    IpcompsPdrops,
    /// "Crypto" processing failure.
    IpcompsCrypto,
    /// Packets too short for compress.
    IpcompsMinlen,
    /// Packet output failure.
    IpcompsOutfail,
    /// `ipcomps_ncounters`.
    IpcompsNcounters,
}

/// `ipcompstat_inc(c)`.
pub fn ipcompstat_inc(c: IpcompCounters) {
    IPCOMPCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `ipcompstat_add(c, v)`.
pub fn ipcompstat_add(c: IpcompCounters, v: u64) {
    IPCOMPCOUNTERS[c as usize].fetch_add(v, Ordering::Relaxed);
}

/// `ipcomp_attach`: called from the transformation code.
pub fn ipcomp_attach() -> i32 {
    0
}

/// `ipcomp_init`: called when a CPI is being set up: picks the compression algorithm and
/// makes the crypto session.
pub fn ipcomp_init(tdbp: &Tdb, xsp: &'static Xformsw, ii: &mut IpsecInit<'_>) -> Result<(), Errno> {
    let tcomp: &'static CompAlgo = match ii.ii_compalg {
        SADB_X_CALG_DEFLATE => &comp_algo_deflate,
        _ => {
            crate::ipsec_dprintf!(
                "ipcomp_init",
                "unsupported compression algorithm {} specified",
                ii.ii_compalg
            );
            return Err(Errno::EINVAL);
        }
    };

    tdbp.tdb_compalgxform.set(Some(tcomp));

    crate::ipsec_dprintf!(
        "ipcomp_init",
        "initialized TDB with ipcomp algorithm {}",
        tcomp.name
    );

    tdbp.tdb_xform.set(Some(xsp));

    // Initialize crypto session
    let cric = Cryptoini {
        cri_alg: tcomp.type_,
        ..Cryptoini::default()
    };

    kernel_lock();
    let sid = crypto_newsession(&cric, 0);
    kernel_unlock();
    tdbp.tdb_cryptoid.set(sid?);
    Ok(())
}

/// `ipcomp_zeroize`: used when an IPCA is deleted: frees the crypto session.
pub fn ipcomp_zeroize(tdbp: &Tdb) -> Result<(), Errno> {
    kernel_lock();
    let error = crypto_freesession(tdbp.tdb_cryptoid.get());
    kernel_unlock();
    tdbp.tdb_cryptoid.set(0);
    error
}

/// `ipcomp_input`: called to uncompress an input packet whose IPComp header is at `skip`;
/// the next-protocol field to restore is at `protoff`.
pub fn ipcomp_input(
    mp: &mut Option<&'static Mbuf>,
    tdb: &'static Tdb,
    skip: i32,
    protoff: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let Some(mut m) = *mp else {
        return IPPROTO_DONE;
    };
    let hlen = IPCOMP_HLENGTH as i32;

    'drop: {
        let Some(ipcompx) = tdb.tdb_compalgxform.get() else {
            ipsecstat_inc(IpsecCounters::IpsecNoxform);
            break 'drop;
        };

        // Get crypto descriptors
        let Some(mut crp) = crypto_getreq(1) else {
            crate::ipsec_dprintf!("ipcomp_input", "failed to acquire crypto descriptors");
            ipcompstat_inc(IpcompCounters::IpcompsCrypto);
            break 'drop;
        };
        let crdc = &mut crp.crp_desc[0];

        crdc.crd_skip = skip + hlen;
        crdc.crd_len = m.m_pkthdr().len.get() - (skip + hlen);
        crdc.crd_inject = skip;

        // Decompression operation
        crdc.CRD_INI.cri_alg = ipcompx.type_;

        // Crypto operation descriptor
        crp.crp_ilen = m.m_pkthdr().len.get() - (skip + hlen);
        crp.crp_flags = CRYPTO_F_IMBUF;
        crp.crp_buf = CryptoBuf::Mbuf(m);
        crp.crp_sid = tdb.tdb_cryptoid.get();

        if let Err(error) = ipsec_crypto_invoke(tdb, &mut crp) {
            crate::ipsec_dprintf!("ipcomp_input", "crypto error {}", error as i32);
            ipsecstat_inc(IpsecCounters::IpsecNoxform);
            crypto_freereq(Some(crp));
            break 'drop;
        }

        let clen = crp.crp_olen;

        // Release the crypto descriptors
        crypto_freereq(Some(crp));

        // update the counters
        let ibytes = (m.m_pkthdr().len.get() - (skip + hlen)) as u64;
        tdb.tdb_cur_bytes.set(tdb.tdb_cur_bytes.get() + ibytes);
        tdbstat_add(tdb, TdbCounters::TdbIbytes, ibytes);
        ipcompstat_add(IpcompCounters::IpcompsIbytes, ibytes);

        // Hard expiration
        if tdb.has_flags(TDBF_BYTES) && tdb.tdb_cur_bytes.get() >= tdb.tdb_exp_bytes.get() {
            ipsecstat_inc(IpsecCounters::IpsecExctdb);
            let _ = pfkeyv2_expire(tdb, SADB_EXT_LIFETIME_HARD);
            tdb_delete(tdb);
            break 'drop;
        }
        // Notify on soft expiration
        mtx_enter(&tdb.tdb_mtx);
        if tdb.has_flags(TDBF_SOFT_BYTES) && tdb.tdb_cur_bytes.get() >= tdb.tdb_soft_bytes.get() {
            tdb.clr_flags(TDBF_SOFT_BYTES); // Turn off checking
            mtx_leave(&tdb.tdb_mtx);
            // may sleep in solock() for the pfkey socket
            let _ = pfkeyv2_expire(tdb, SADB_EXT_LIFETIME_SOFT);
        } else {
            mtx_leave(&tdb.tdb_mtx);
        }

        // In case it's not done already, adjust the size of the mbuf chain
        m.m_pkthdr().len.set(clen + hlen + skip);

        if (m.m_len().get() as i32) < skip + hlen {
            *mp = m_pullup(m, skip + hlen);
            let Some(mm) = *mp else {
                ipcompstat_inc(IpcompCounters::IpcompsHdrops);
                break 'drop;
            };
            m = mm;
        }

        // Find the beginning of the IPCOMP header
        let Some((m1, roff)) = m_getptr(m, skip) else {
            crate::ipsec_dprintf!(
                "ipcomp_input",
                "bad mbuf chain, IPCA {}/{:08x}",
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            ipcompstat_inc(IpcompCounters::IpcompsHdrops);
            break 'drop;
        };
        // Keep the next protocol field
        let mut nproto = [0u8; 1];
        m_copydata(m, skip, &mut nproto);

        // Remove the IPCOMP header from the mbuf
        esp_strip_header(m, m1, roff, hlen);

        // Restore the Next Protocol field
        let _ = m_copyback(m, protoff, &nproto, M_NOWAIT);

        // Back to generic IPsec input processing
        return ipsec_common_input_cb(mp, tdb, skip, protoff, ns);
    }
    // drop:
    m_freemp(mp);
    IPPROTO_DONE
}

/// `ipcomp_output`: IPComp output routine, called by `ipsp_process_packet()`. Compresses the
/// payload after `skip` and inserts the IPComp header in front of it; a payload that does not
/// shrink is sent as it is.
pub fn ipcomp_output(
    m: &'static Mbuf,
    tdb: &'static Tdb,
    skip: i32,
    _protoff: i32,
) -> Result<(), Errno> {
    let mut m = m;

    if let Some(encif) = enc_getif(0, tdb.tdb_tap.get()) {
        encif.if_opackets().set(encif.if_opackets().get() + 1);
        encif
            .if_obytes()
            .set(encif.if_obytes().get() + m.m_pkthdr().len.get() as u64);

        let if_bpf = encif.if_bpf.get();
        if !if_bpf.is_null() {
            let hdr = Enchdr {
                af: u32::from(tdb.tdb_dst.get().sa_family()).to_be(),
                spi: tdb.tdb_spi.get(),
                flags: 0,
            };

            let _ = bpf_mtap_hdr(if_bpf, &hdr.to_bytes(), m, BPF_DIRECTION_OUT);
        }
    }

    let hlen = IPCOMP_HLENGTH as i32;

    ipcompstat_inc(IpcompCounters::IpcompsOutput);

    let error: Errno = 'drop: {
        let Some(ipcompx) = tdb.tdb_compalgxform.get() else {
            ipsecstat_inc(IpsecCounters::IpsecNoxform);
            break 'drop Errno::EINVAL;
        };

        match tdb.tdb_dst.get().sa_family() {
            AF_INET => {
                // Check for IPv4 maximum packet size violations. Since compression is going
                // to reduce the size, no need to worry.
                if m.m_pkthdr().len.get() + hlen > IP_MAXPACKET as i32 {
                    crate::ipsec_dprintf!(
                        "ipcomp_output",
                        "packet in IPCA {}/{:08x} got too big",
                        ipsp_address(&tdb.tdb_dst.get()),
                        ntohl(tdb.tdb_spi.get())
                    );
                    ipcompstat_inc(IpcompCounters::IpcompsToobig);
                    break 'drop Errno::EMSGSIZE;
                }
            }
            #[cfg(feature = "inet6")]
            AF_INET6 => {
                // Check for IPv6 maximum packet size violations
                if m.m_pkthdr().len.get() as usize + hlen as usize > IPV6_MAXPACKET {
                    crate::ipsec_dprintf!(
                        "ipcomp_output",
                        "packet in IPCA {}/{:08x} got too big",
                        ipsp_address(&tdb.tdb_dst.get()),
                        ntohl(tdb.tdb_spi.get())
                    );
                    ipcompstat_inc(IpcompCounters::IpcompsToobig);
                    break 'drop Errno::EMSGSIZE;
                }
            }
            _ => {
                crate::ipsec_dprintf!(
                    "ipcomp_output",
                    "unknown/unsupported protocol family {}, IPCA {}/{:08x}",
                    tdb.tdb_dst.get().sa_family(),
                    ipsp_address(&tdb.tdb_dst.get()),
                    ntohl(tdb.tdb_spi.get())
                );
                ipcompstat_inc(IpcompCounters::IpcompsNopf);
                break 'drop Errno::EPFNOSUPPORT;
            }
        }

        // Update the counters
        let obytes = (m.m_pkthdr().len.get() - skip) as u64;
        tdb.tdb_cur_bytes.set(tdb.tdb_cur_bytes.get() + obytes);
        ipcompstat_add(IpcompCounters::IpcompsObytes, obytes);

        // Hard byte expiration
        if tdb.has_flags(TDBF_BYTES) && tdb.tdb_cur_bytes.get() >= tdb.tdb_exp_bytes.get() {
            ipsecstat_inc(IpsecCounters::IpsecExctdb);
            let _ = pfkeyv2_expire(tdb, SADB_EXT_LIFETIME_HARD);
            tdb_delete(tdb);
            break 'drop Errno::EINVAL;
        }

        // Soft byte expiration
        mtx_enter(&tdb.tdb_mtx);
        if tdb.has_flags(TDBF_SOFT_BYTES) && tdb.tdb_cur_bytes.get() >= tdb.tdb_soft_bytes.get() {
            tdb.clr_flags(TDBF_SOFT_BYTES); // Turn off checking
            mtx_leave(&tdb.tdb_mtx);
            // may sleep in solock() for the pfkey socket
            let _ = pfkeyv2_expire(tdb, SADB_EXT_LIFETIME_SOFT);
        } else {
            mtx_leave(&tdb.tdb_mtx);
        }

        // Loop through mbuf chain; if we find a readonly mbuf, copy the packet.
        let mut mi = Some(m);
        while let Some(x) = mi
            && !m_readonly(x)
        {
            mi = x.m_next().get();
        }

        if mi.is_some() {
            let Some(n) = m_dup_pkt(m, 0, M_DONTWAIT) else {
                crate::ipsec_dprintf!(
                    "ipcomp_output",
                    "bad mbuf chain, IPCA {}/{:08x}",
                    ipsp_address(&tdb.tdb_dst.get()),
                    ntohl(tdb.tdb_spi.get())
                );
                ipcompstat_inc(IpcompCounters::IpcompsHdrops);
                break 'drop Errno::ENOBUFS;
            };

            m_freem(m);
            m = n;
        }
        // Ok now, we can pass to the crypto processing

        // Get crypto descriptors
        let Some(mut crp): Option<Cryptop<'_>> = crypto_getreq(1) else {
            crate::ipsec_dprintf!("ipcomp_output", "failed to acquire crypto descriptors");
            ipcompstat_inc(IpcompCounters::IpcompsCrypto);
            break 'drop Errno::ENOBUFS;
        };
        let crdc = &mut crp.crp_desc[0];

        // Compression descriptor
        crdc.crd_skip = skip;
        crdc.crd_len = m.m_pkthdr().len.get() - skip;
        crdc.crd_flags = CRD_F_COMP;
        crdc.crd_inject = skip;

        // Compression operation
        crdc.CRD_INI.cri_alg = ipcompx.type_;

        // Crypto operation descriptor
        crp.crp_ilen = m.m_pkthdr().len.get(); // Total input length
        crp.crp_flags = CRYPTO_F_IMBUF;
        crp.crp_buf = CryptoBuf::Mbuf(m);
        crp.crp_sid = tdb.tdb_cryptoid.get();

        if let Err(error) = ipsec_crypto_invoke(tdb, &mut crp) {
            crate::ipsec_dprintf!("ipcomp_output", "crypto error {}", error as i32);
            ipsecstat_inc(IpsecCounters::IpsecNoxform);
            crypto_freereq(Some(crp));
            break 'drop error;
        }

        let ilen = crp.crp_ilen;
        let olen = crp.crp_olen;

        // Release the crypto descriptors
        crypto_freereq(Some(crp));

        let rlen = ilen - skip;

        // Check sizes.
        if rlen > olen + hlen {
            // Inject IPCOMP header
            let Some((mo, roff)) = m_makespace(m, skip, hlen) else {
                crate::ipsec_dprintf!(
                    "ipcomp_output",
                    "failed to inject IPCOMP header for IPCA {}/{:08x}",
                    ipsp_address(&tdb.tdb_dst.get()),
                    ntohl(tdb.tdb_spi.get())
                );
                ipcompstat_inc(IpcompCounters::IpcompsWrap);
                break 'drop Errno::ENOBUFS;
            };

            // Initialize the IPCOMP header
            let cpi = ntohl(tdb.tdb_spi.get()) as u16;
            let mut ipcomp = Ipcomp {
                ipcomp_cpi: htons(cpi),
                ..Ipcomp::default()
            };

            // m_pullup before ?
            match tdb.tdb_dst.get().sa_family() {
                AF_INET => {
                    let mut ip = mtod_ip(m);
                    ipcomp.ipcomp_nh = ip.ip_p;
                    ip.ip_p = IPPROTO_IPCOMP as u8;
                    mtod_ip_store(m, &ip);
                }
                #[cfg(feature = "inet6")]
                AF_INET6 => {
                    let mut ip6 = mtod_ip6(m);
                    ipcomp.ipcomp_nh = ip6.ip6_nxt;
                    ip6.ip6_nxt = IPPROTO_IPCOMP as u8;
                    mtod_ip6_store(m, &ip6);
                }
                _ => {
                    crate::ipsec_dprintf!(
                        "ipcomp_output",
                        "unsupported protocol family {}, IPCA {}/{:08x}",
                        tdb.tdb_dst.get().sa_family(),
                        ipsp_address(&tdb.tdb_dst.get()),
                        ntohl(tdb.tdb_spi.get())
                    );
                    ipcompstat_inc(IpcompCounters::IpcompsNopf);
                    break 'drop Errno::EPFNOSUPPORT;
                }
            }
            // SAFETY: `m_makespace` made `IPCOMP_HLENGTH` contiguous bytes at `roff` of `mo`;
            // the header is written unaligned.
            unsafe {
                ptr::write_unaligned(mtod::<u8>(mo).add(roff as usize).cast::<Ipcomp>(), ipcomp)
            };
        } else {
            // Compression was useless, we have lost time.
            ipcompstat_inc(IpcompCounters::IpcompsMinlen); // misnomer, but like to count
        }

        // skiphdr:
        let error = ipsp_process_done(m, tdb);
        if error.is_err() {
            ipcompstat_inc(IpcompCounters::IpcompsOutfail);
        }
        return error;
    };
    // drop:
    m_freem(m);
    Err(error)
}

// LP64 sizes of the C structures.
const _: () = {
    assert!(size_of::<Ipcompstat>() == IpcompCounters::IpcompsNcounters as usize * 8);
    assert!(size_of::<Ipcomp>() == IPCOMP_HLENGTH);
};
/* </CODE> */
