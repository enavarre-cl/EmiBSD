/*	$OpenBSD: ip_ah.h,v 1.37 2020/09/01 01:53:34 gnezdo Exp $	*/
/*	$OpenBSD: ip_ah.c,v 1.181 2026/08/12 18:23:14 bluhm Exp $ */
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
 * Copyright (C) 1995, 1996, 1997, 1998, 1999 John Ioannidis,
 * Angelos D. Keromytis and Niels Provos.
 * Copyright (c) 2001 Angelos D. Keromytis.
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
 * Additional features in 1999 by Angelos D. Keromytis and Niklas Hallqvist.
 *
 * Copyright (c) 1995, 1996, 1997, 1998, 1999 by John Ioannidis,
 * Angelos D. Keromytis and Niels Provos.
 * Copyright (c) 1999 Niklas Hallqvist.
 * Copyright (c) 2001 Angelos D. Keromytis.
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

//! The IP Authentication Header (AH, RFC 4302): `<netinet/ip_ah.h>` (the statistics, the
//! header, the sysctl names) and `netinet/ip_ah.c` (the transform: SA setup, input
//! verification and output authentication, with the mutable IP header fields zeroed for the
//! computation).
//!
//! Upstream: sys/netinet/ip_ah.h @ 3ce1f3f79392
//! Upstream: sys/netinet/ip_ah.c @ 3ce1f3f79392
//!
//! Status: `ported` (M9c).
//!
//! ## Deviations
//! - `ahcounters` (`struct cpumem *`) is the static array of atomics `AHCOUNTERS` in
//!   `netinet/ipsec_input.rs`, which defines the C's pointer; `ah_enable` is `AH_ENABLE`
//!   there.
//! - The saved copy of the headers and the authenticator (`malloc(M_XDATA)` in C) is a `Vec`
//!   freed on every path; the crypto request is the framework's [`Cryptop`] value.
//! - `ah_massage_headers` keeps the C's `struct mbuf **` as `&mut Option<&'static Mbuf>`
//!   (it may pull the packet up, or free it on failure); the IPv4 header is changed in a
//!   copy stored back, the options through the pulled-up first mbuf.
//! - The replay checks share `ip_esp.rs`'s `checkreplaywindow`; the counters are AH's.
//! - Not configured, each a comment at its site: `NBPFILTER` (the `enc(4)` counters and
//!   `bpf_mtap_hdr` of `ah_output`), `NPFSYNC` (`pfsync_update_tdb`), `INET6` (the IPv6
//!   header and extension header massaging).

use alloc::vec;
use core::mem::offset_of;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::crypto::crypto::{crypto_freereq, crypto_freesession, crypto_getreq, crypto_newsession};
use crate::crypto::cryptodev::{CRD_F_ESN, CRYPTO_ESN, CRYPTO_F_IMBUF, CryptoBuf, Cryptoini};
use crate::crypto::xform::{
    AuthHash, auth_hash_hmac_md5_96, auth_hash_hmac_ripemd_160_96, auth_hash_hmac_sha1_96,
    auth_hash_hmac_sha2_256_128, auth_hash_hmac_sha2_384_192, auth_hash_hmac_sha2_512_256,
};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{
    m_copyback, m_copydata, m_dup_pkt, m_freem, m_getptr, m_makespace, m_pullup,
};
use crate::net::if_var::Netstack;
use crate::net::pfkeyv2::{
    SADB_AALG_MD5HMAC, SADB_AALG_SHA1HMAC, SADB_EXT_LIFETIME_HARD, SADB_EXT_LIFETIME_SOFT,
    SADB_X_AALG_RIPEMD160HMAC, SADB_X_AALG_SHA2_256, SADB_X_AALG_SHA2_384, SADB_X_AALG_SHA2_512,
    pfkeyv2_expire,
};
use crate::netinet::in_::{IPPROTO_AH, IPPROTO_DONE};
use crate::netinet::ip::{
    IP_MAXPACKET, IPOPT_EOL, IPOPT_LSRR, IPOPT_NOP, IPOPT_SECURITY, IPOPT_SSRR, Ip,
};
use crate::netinet::ip_esp::{checkreplaywindow, esp_strip_header, ipsec_crypto_invoke};
use crate::netinet::ip_ipsp::{
    AH_ALEN_MAX, AH_HMAC_INITIAL_RPL, IPSEC_ZEROES_SIZE, IpsecCounters, IpsecInit, TDBF_BYTES,
    TDBF_ESN, TDBF_SOFT_BYTES, Tdb, TdbCounters, Xformsw, ipsecstat_inc, ipsp_address, tdb_delete,
    tdbstat_add,
};
use crate::netinet::ip_var::{mtod_ip, mtod_ip_store};
use crate::netinet::ipsec_input::{AHCOUNTERS, ipsec_common_input_cb};
use crate::netinet::ipsec_output::ipsp_process_done;
use crate::sys::endian::{htonl, htons, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::malloc::{M_WAITOK, M_XDATA};
use crate::sys::mbuf::{M_DONTWAIT, Mbuf, m_freemp, m_readonly, mtod};
use crate::sys::socket::AF_INET;
use libkern::{explicit_bzero, timingsafe_bcmp};

/// `struct ahstat`: the AH statistics as `net.inet.ah.stats` returns them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Ahstat {
    /// Packet shorter than header shows.
    pub ahs_hdrops: u64,
    /// Protocol family not supported.
    pub ahs_nopf: u64,
    /// `ahs_notdb`.
    pub ahs_notdb: u64,
    /// `ahs_badkcr`.
    pub ahs_badkcr: u64,
    /// `ahs_badauth`.
    pub ahs_badauth: u64,
    /// `ahs_noxform`.
    pub ahs_noxform: u64,
    /// `ahs_qfull`.
    pub ahs_qfull: u64,
    /// `ahs_wrap`.
    pub ahs_wrap: u64,
    /// `ahs_replay`.
    pub ahs_replay: u64,
    /// Bad authenticator length.
    pub ahs_badauthl: u64,
    /// Input AH packets.
    pub ahs_input: u64,
    /// Output AH packets.
    pub ahs_output: u64,
    /// Trying to use an invalid TDB.
    pub ahs_invalid: u64,
    /// Input bytes.
    pub ahs_ibytes: u64,
    /// Output bytes.
    pub ahs_obytes: u64,
    /// Packet got larger than `IP_MAXPACKET`.
    pub ahs_toobig: u64,
    /// Packet blocked due to policy.
    pub ahs_pdrops: u64,
    /// Crypto processing failure.
    pub ahs_crypto: u64,
    /// Packet output failure.
    pub ahs_outfail: u64,
}

/// `struct ah`: the AH header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ah {
    /// `ah_nh`: next header.
    pub ah_nh: u8,
    /// `ah_hl`: length in 32-bit words, less 2.
    pub ah_hl: u8,
    /// `ah_rv`: reserved.
    pub ah_rv: u16,
    /// `ah_spi`.
    pub ah_spi: u32,
    /// `ah_rpl`: we may not use this, if we're using old xforms.
    pub ah_rpl: u32,
}

/// `AH_FLENGTH`: length of base AH header.
pub const AH_FLENGTH: usize = 8;

// Names for AH sysctl objects

/// `AHCTL_ENABLE`: enable AH processing.
pub const AHCTL_ENABLE: i32 = 1;
/// `AHCTL_STATS`: AH stats.
pub const AHCTL_STATS: i32 = 2;
/// `AHCTL_MAXID`.
pub const AHCTL_MAXID: i32 = 3;

/// `enum ahstat_counters`: one per field of [`Ahstat`], in the same order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum AhstatCounters {
    /// Packet shorter than header shows.
    AhsHdrops,
    /// Protocol family not supported.
    AhsNopf,
    /// `ahs_notdb`.
    AhsNotdb,
    /// `ahs_badkcr`.
    AhsBadkcr,
    /// `ahs_badauth`.
    AhsBadauth,
    /// `ahs_noxform`.
    AhsNoxform,
    /// `ahs_qfull`.
    AhsQfull,
    /// `ahs_wrap`.
    AhsWrap,
    /// `ahs_replay`.
    AhsReplay,
    /// Bad authenticator length.
    AhsBadauthl,
    /// Input AH packets.
    AhsInput,
    /// Output AH packets.
    AhsOutput,
    /// Trying to use an invalid TDB.
    AhsInvalid,
    /// Input bytes.
    AhsIbytes,
    /// Output bytes.
    AhsObytes,
    /// Packet got larger than `IP_MAXPACKET`.
    AhsToobig,
    /// Packet blocked due to policy.
    AhsPdrops,
    /// Crypto processing failure.
    AhsCrypto,
    /// Packet output failure.
    AhsOutfail,
    /// `ahs_ncounters`.
    AhsNcounters,
}

/// `ahstat_inc(c)`.
pub fn ahstat_inc(c: AhstatCounters) {
    AHCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `ahstat_add(c, v)`.
pub fn ahstat_add(c: AhstatCounters, v: u64) {
    AHCOUNTERS[c as usize].fetch_add(v, Ordering::Relaxed);
}

/// `ipseczeroes`: zeroes!
static IPSECZEROES: [u8; IPSEC_ZEROES_SIZE] = [0; IPSEC_ZEROES_SIZE];

/// `ah_attach`: called from the transformation initialization code.
pub fn ah_attach() -> i32 {
    0
}

/// `ah_init`: called when an SPI is being set up.
pub fn ah_init(tdbp: &Tdb, xsp: &'static Xformsw, ii: &mut IpsecInit<'_>) -> Result<(), Errno> {
    // Authentication operation.
    let thash: &'static AuthHash = match ii.ii_authalg {
        SADB_AALG_MD5HMAC => &auth_hash_hmac_md5_96,
        SADB_AALG_SHA1HMAC => &auth_hash_hmac_sha1_96,
        SADB_X_AALG_RIPEMD160HMAC => &auth_hash_hmac_ripemd_160_96,
        SADB_X_AALG_SHA2_256 => &auth_hash_hmac_sha2_256_128,
        SADB_X_AALG_SHA2_384 => &auth_hash_hmac_sha2_384_192,
        SADB_X_AALG_SHA2_512 => &auth_hash_hmac_sha2_512_256,
        _ => {
            crate::ipsec_dprintf!(
                "ah_init",
                "unsupported authentication algorithm {} specified",
                ii.ii_authalg
            );
            return Err(Errno::EINVAL);
        }
    };

    if ii.ii_authkeylen != thash.keysize && thash.keysize != 0 {
        crate::ipsec_dprintf!(
            "ah_init",
            "keylength {} doesn't match algorithm {} keysize ({})",
            ii.ii_authkeylen,
            thash.name,
            thash.keysize
        );
        return Err(Errno::EINVAL);
    }

    tdbp.tdb_xform.set(Some(xsp));
    tdbp.tdb_authalgxform.set(Some(thash));
    tdbp.tdb_rpl.set(AH_HMAC_INITIAL_RPL);

    crate::ipsec_dprintf!(
        "ah_init",
        "initialized TDB with hash algorithm {}",
        thash.name
    );

    let key = &ii.ii_authkey[..usize::from(ii.ii_authkeylen).min(ii.ii_authkey.len())];
    tdbp.tdb_amxkeylen.set(ii.ii_authkeylen);
    let Some(p) = malloc(key.len().max(1), M_XDATA, M_WAITOK) else {
        panic(format_args!("ah_init: malloc(M_WAITOK) failed"));
    };
    // SAFETY: a fresh allocation of at least `key.len()` bytes.
    unsafe { ptr::copy_nonoverlapping(key.as_ptr(), p.as_ptr(), key.len()) };
    tdbp.tdb_amxkey.set(p.as_ptr());

    // Initialize crypto session.
    let mut crin = Cryptoini::default();
    let mut cria = Cryptoini {
        cri_alg: thash.type_,
        cri_klen: i32::from(ii.ii_authkeylen) * 8,
        cri_key: ii.ii_authkey,
        ..Cryptoini::default()
    };

    if tdbp.tdb_wnd.get() > 0 && tdbp.has_flags(TDBF_ESN) {
        crin.cri_alg = CRYPTO_ESN;
        cria.cri_next = Some(&crin);
    }

    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    let sid = crypto_newsession(&cria, 0)?;
    tdbp.tdb_cryptoid.set(sid);
    Ok(())
}

/// `ah_zeroize`: paranoia.
pub fn ah_zeroize(tdbp: &Tdb) -> Result<(), Errno> {
    if let Some(p) = ptr::NonNull::new(tdbp.tdb_amxkey.get()) {
        let len = usize::from(tdbp.tdb_amxkeylen.get());
        // SAFETY: the key of `len` bytes `ah_init` allocated.
        explicit_bzero(unsafe { core::slice::from_raw_parts_mut(p.as_ptr(), len) });
        free(p, M_XDATA, len.max(1));
        tdbp.tdb_amxkey.set(ptr::null_mut());
    }

    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    let error = crypto_freesession(tdbp.tdb_cryptoid.get());
    tdbp.tdb_cryptoid.set(0);
    error
}

/// `ah_massage_headers`: massage IPv4/IPv6 headers for AH processing: the mutable fields and
/// options are zeroed (on output, a source route's final destination is put in place).
fn ah_massage_headers(
    mp: &mut Option<&'static Mbuf>,
    af: u8,
    skip: i32,
    _alg: i32,
    out: bool,
) -> Result<(), Errno> {
    let Some(m) = *mp else {
        return Err(Errno::EINVAL);
    };

    let error: Errno = 'drop: {
        if af == AF_INET {
            {
                // This is the least painful way of dealing with IPv4 header and option
                // processing -- just make sure they're in contiguous memory.
                *mp = m_pullup(m, skip);
                let Some(m) = *mp else {
                    crate::ipsec_dprintf!("ah_massage_headers", "m_pullup() failed");
                    ahstat_inc(AhstatCounters::AhsHdrops);
                    return Err(Errno::ENOBUFS);
                };

                // Fix the IP header
                let mut ip = mtod_ip(m);
                ip.ip_tos = 0;
                ip.ip_ttl = 0;
                ip.ip_sum = 0;
                ip.ip_off = 0;
                mtod_ip_store(m, &ip);

                // SAFETY: `m_pullup` made the first `skip` bytes contiguous in `m`.
                let ptr = unsafe { core::slice::from_raw_parts_mut(mtod::<u8>(m), skip as usize) };

                // IPv4 option processing
                let mut off = size_of::<Ip>();
                let skip = skip as usize;
                while off < skip {
                    if ptr[off] != IPOPT_EOL && ptr[off] != IPOPT_NOP && off + 1 >= skip {
                        crate::ipsec_dprintf!(
                            "ah_massage_headers",
                            "illegal IPv4 option length for option {}",
                            ptr[off]
                        );
                        ahstat_inc(AhstatCounters::AhsHdrops);
                        break 'drop Errno::EINVAL;
                    }

                    match ptr[off] {
                        IPOPT_EOL => off = skip, // End the loop.
                        IPOPT_NOP => off += 1,
                        // 0x82, extended security (0x85), commercial security (0x86), router
                        // alert (0x94), RFC1770 (0x95).
                        IPOPT_SECURITY | 0x85 | 0x86 | 0x94 | 0x95 => {
                            // Sanity check for option length.
                            if ptr[off + 1] < 2 {
                                crate::ipsec_dprintf!(
                                    "ah_massage_headers",
                                    "illegal IPv4 option length for option {}",
                                    ptr[off]
                                );
                                ahstat_inc(AhstatCounters::AhsHdrops);
                                break 'drop Errno::EINVAL;
                            }

                            off += usize::from(ptr[off + 1]);
                        }
                        opt => {
                            if opt == IPOPT_LSRR || opt == IPOPT_SSRR {
                                // Sanity check for option length.
                                if ptr[off + 1] < 2 {
                                    crate::ipsec_dprintf!(
                                        "ah_massage_headers",
                                        "illegal IPv4 option length for option {}",
                                        ptr[off]
                                    );
                                    ahstat_inc(AhstatCounters::AhsHdrops);
                                    break 'drop Errno::EINVAL;
                                }

                                // On output, if we have either of the source routing
                                // options, we should swap the destination address of the IP
                                // header with the last address specified in the option, as
                                // that is what the destination's IP header will look like.
                                let olen = usize::from(ptr[off + 1]);
                                if out && olen >= 2 + 4 && off + olen <= skip {
                                    let at = off + olen - 4;
                                    let dst = offset_of!(Ip, ip_dst);
                                    ptr.copy_within(at..at + 4, dst);
                                }

                                // FALLTHROUGH
                            }
                            // Sanity check for option length.
                            if ptr[off + 1] < 2 {
                                crate::ipsec_dprintf!(
                                    "ah_massage_headers",
                                    "illegal IPv4 option length for option {}",
                                    ptr[off]
                                );
                                ahstat_inc(AhstatCounters::AhsHdrops);
                                break 'drop Errno::EINVAL;
                            }

                            // Zeroize all other options.
                            let count = usize::from(ptr[off + 1]);
                            let end = (off + count).min(skip);
                            ptr[off..end].fill(0);
                            off += count;
                        }
                    }

                    // Sanity check.
                    if off > skip {
                        crate::ipsec_dprintf!(
                            "ah_massage_headers",
                            "malformed IPv4 options header"
                        );
                        ahstat_inc(AhstatCounters::AhsHdrops);
                        break 'drop Errno::EINVAL;
                    }
                }
            }
        }
        // INET6: for AF_INET6, cook the IPv6 header (flow, hop limit, scoped addresses) and
        // zero the mutable options of the extension headers; not configured.

        return Ok(());
    };
    // drop:
    m_freemp(mp);
    Err(error)
}

/// The replay window check of `ah_input`: counts and logs a failure; `false` when the packet
/// is to be dropped.
fn ah_replay_check(tdb: &Tdb, chk_rpl: i32) -> bool {
    let (what, c) = match chk_rpl {
        0 => return true, // All's well.
        1 => ("replay counter wrapped", AhstatCounters::AhsWrap),
        2 => ("old packet received", AhstatCounters::AhsReplay),
        3 => ("duplicate packet received", AhstatCounters::AhsReplay),
        _ => (
            "bogus value from checkreplaywindow()",
            AhstatCounters::AhsReplay,
        ),
    };
    let _ = what;
    crate::ipsec_dprintf!(
        "ah_input",
        "{} in SA {}/{:08x}",
        what,
        ipsp_address(&tdb.tdb_dst.get()),
        ntohl(tdb.tdb_spi.get())
    );
    ahstat_inc(c);
    false
}

/// `ah_input`: verifies that an input packet passes authentication.
pub fn ah_input(
    mp: &mut Option<&'static Mbuf>,
    tdb: &'static Tdb,
    skip: i32,
    protoff: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let Some(ahx) = tdb.tdb_authalgxform.get() else {
        m_freemp(mp);
        return IPPROTO_DONE;
    };
    let Some(mut m) = *mp else {
        return IPPROTO_DONE;
    };
    let mut esn: u32 = 0;
    let mut calc = [0u8; AH_ALEN_MAX];
    let authsize = i32::from(ahx.authsize);

    'drop: {
        let rplen = (AH_FLENGTH + 4) as i32;
        if m.m_pkthdr().len.get() < skip + rplen {
            ahstat_inc(AhstatCounters::AhsHdrops);
            break 'drop;
        }

        // Save the AH header, we use it throughout.
        let mut hl = [0u8; 1];
        m_copydata(m, skip + offset_of!(Ah, ah_hl) as i32, &mut hl);
        let hl = hl[0];

        // Replay window checking, if applicable.
        if tdb.tdb_wnd.get() > 0 {
            let mut b = [0u8; 4];
            m_copydata(m, skip + offset_of!(Ah, ah_rpl) as i32, &mut b);
            let btsx = ntohl(u32::from_ne_bytes(b));

            mtx_enter(&tdb.tdb_mtx);
            let chk_rpl = checkreplaywindow(tdb, tdb.tdb_rpl.get(), btsx, &mut esn, false);
            mtx_leave(&tdb.tdb_mtx);
            if !ah_replay_check(tdb, chk_rpl) {
                break 'drop;
            }
        }

        // Verify AH header length.
        if i32::from(hl) * 4 != authsize + rplen - AH_FLENGTH as i32 {
            crate::ipsec_dprintf!(
                "ah_input",
                "bad authenticator length {} for packet in SA {}/{:08x}",
                i32::from(hl) * 4,
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            ahstat_inc(AhstatCounters::AhsBadauthl);
            break 'drop;
        }
        if skip + authsize + rplen > m.m_pkthdr().len.get() {
            crate::ipsec_dprintf!(
                "ah_input",
                "bad mbuf length {} (expecting {}) for packet in SA {}/{:08x}",
                m.m_pkthdr().len.get(),
                skip + authsize + rplen,
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            ahstat_inc(AhstatCounters::AhsBadauthl);
            break 'drop;
        }

        // Update the counters.
        let ibytes = (m.m_pkthdr().len.get() - skip - i32::from(hl) * 4) as u64;
        tdb.tdb_cur_bytes.set(tdb.tdb_cur_bytes.get() + ibytes);
        tdbstat_add(tdb, TdbCounters::TdbIbytes, ibytes);
        ahstat_add(AhstatCounters::AhsIbytes, ibytes);

        // Hard expiration.
        if tdb.has_flags(TDBF_BYTES) && tdb.tdb_cur_bytes.get() >= tdb.tdb_exp_bytes.get() {
            ipsecstat_inc(IpsecCounters::IpsecExctdb);
            let _ = pfkeyv2_expire(tdb, SADB_EXT_LIFETIME_HARD);
            tdb_delete(tdb);
            break 'drop;
        }

        // Notify on expiration.
        mtx_enter(&tdb.tdb_mtx);
        if tdb.has_flags(TDBF_SOFT_BYTES) && tdb.tdb_cur_bytes.get() >= tdb.tdb_soft_bytes.get() {
            tdb.clr_flags(TDBF_SOFT_BYTES); // Turn off checking
            mtx_leave(&tdb.tdb_mtx);
            // may sleep in solock() for the pfkey socket
            let _ = pfkeyv2_expire(tdb, SADB_EXT_LIFETIME_SOFT);
        } else {
            mtx_leave(&tdb.tdb_mtx);
        }

        // Get crypto descriptors.
        let Some(mut crp) = crypto_getreq(1) else {
            crate::ipsec_dprintf!("ah_input", "failed to acquire crypto descriptors");
            ahstat_inc(AhstatCounters::AhsCrypto);
            break 'drop;
        };

        {
            let crda = &mut crp.crp_desc[0];

            crda.crd_skip = 0;
            crda.crd_len = m.m_pkthdr().len.get();
            crda.crd_inject = skip + rplen;

            // Authentication operation.
            crda.CRD_INI.cri_alg = ahx.type_;
            // SAFETY: the TDB is held for the whole call; its key lives until `xf_zeroize`.
            crda.CRD_INI.cri_key = unsafe { tdb.tdb_amxkey() };
            crda.CRD_INI.cri_klen = i32::from(tdb.tdb_amxkeylen.get()) * 8;

            if tdb.tdb_wnd.get() > 0 && tdb.has_flags(TDBF_ESN) {
                esn = htonl(esn);
                crda.set_crd_esn(esn.to_ne_bytes());
                crda.crd_flags |= CRD_F_ESN;
            }
        }

        // Allocate IPsec-specific opaque crypto info.
        let mut ptr = vec![0u8; (skip + rplen + authsize) as usize];

        // Save the authenticator, the skipped portion of the packet, and the AH header.
        m_copydata(m, 0, &mut ptr);

        // Zeroize the authenticator on the packet.
        let _ = m_copyback(m, skip + rplen, &IPSECZEROES[..authsize as usize], M_NOWAIT);

        // "Massage" the packet headers for crypto processing.
        let r = ah_massage_headers(mp, tdb.tdb_dst.get().sa_family(), skip, ahx.type_, false);
        // callee may change or free mbuf
        match *mp {
            Some(mm) if r.is_ok() => m = mm,
            _ => {
                crypto_freereq(Some(crp));
                break 'drop;
            }
        }

        // Crypto operation descriptor.
        crp.crp_ilen = m.m_pkthdr().len.get(); // Total input length.
        crp.crp_flags = CRYPTO_F_IMBUF;
        crp.crp_buf = CryptoBuf::Mbuf(m);
        crp.crp_sid = tdb.tdb_cryptoid.get();

        if let Err(error) = ipsec_crypto_invoke(tdb, &mut crp) {
            crate::ipsec_dprintf!("ah_input", "crypto error {}", error as i32);
            ipsecstat_inc(IpsecCounters::IpsecNoxform);
            crypto_freereq(Some(crp));
            break 'drop;
        }

        // Release the crypto descriptors
        crypto_freereq(Some(crp));

        // Copy authenticator off the packet.
        m_copydata(m, skip + rplen, &mut calc[..authsize as usize]);

        // Verify authenticator.
        let saved = (skip + rplen) as usize;
        if timingsafe_bcmp(
            &ptr[saved..saved + authsize as usize],
            &calc[..authsize as usize],
        ) {
            crate::ipsec_dprintf!(
                "ah_input",
                "authentication failed for packet in SA {}/{:08x}",
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            ahstat_inc(AhstatCounters::AhsBadauth);
            break 'drop;
        }

        // Fix the Next Protocol field.
        ptr[protoff as usize] = ptr[skip as usize];

        // Copyback the saved (uncooked) network headers.
        let _ = m_copyback(m, 0, &ptr[..skip as usize], M_NOWAIT);

        drop(ptr);

        // Replay window checking, if applicable.
        if tdb.tdb_wnd.get() > 0 {
            let mut b = [0u8; 4];
            m_copydata(m, skip + offset_of!(Ah, ah_rpl) as i32, &mut b);
            let btsx = ntohl(u32::from_ne_bytes(b));

            mtx_enter(&tdb.tdb_mtx);
            let chk_rpl = checkreplaywindow(tdb, tdb.tdb_rpl.get(), btsx, &mut esn, true);
            mtx_leave(&tdb.tdb_mtx);
            // NPFSYNC > 0: pfsync_update_tdb(tdb, 0) when all's well; not configured.
            if !ah_replay_check(tdb, chk_rpl) {
                break 'drop;
            }
        }

        // Record the beginning of the AH header.
        let Some((m1, roff)) = m_getptr(m, skip) else {
            crate::ipsec_dprintf!(
                "ah_input",
                "bad mbuf chain for packet in SA {}/{:08x}",
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            ahstat_inc(AhstatCounters::AhsHdrops);
            break 'drop;
        };

        // Remove the AH header from the mbuf.
        esp_strip_header(m, m1, roff, rplen + authsize);

        return ipsec_common_input_cb(mp, tdb, skip, protoff, ns);
    }
    // drop:
    m_freemp(mp);
    IPPROTO_DONE
}

/// `ah_output`: AH output routine, called by `ipsp_process_packet()`.
pub fn ah_output(
    m: &'static Mbuf,
    tdb: &'static Tdb,
    skip: i32,
    protoff: i32,
) -> Result<(), Errno> {
    let mut m = m;
    let Some(ahx) = tdb.tdb_authalgxform.get() else {
        m_freem(m);
        return Err(Errno::EINVAL);
    };
    let authsize = i32::from(ahx.authsize);

    // NBPFILTER > 0: the enc(4) interface of tdb_rdomain/tdb_tap counts the packet and taps
    // it with an enchdr; not configured.

    ahstat_inc(AhstatCounters::AhsOutput);

    let rplen = (AH_FLENGTH + 4) as i32;

    let error: Errno = 'drop: {
        match tdb.tdb_dst.get().sa_family() {
            AF_INET => {
                // Check for IP maximum packet size violations.
                if rplen + authsize + m.m_pkthdr().len.get() > IP_MAXPACKET as i32 {
                    crate::ipsec_dprintf!(
                        "ah_output",
                        "packet in SA {}/{:08x} got too big",
                        ipsp_address(&tdb.tdb_dst.get()),
                        ntohl(tdb.tdb_spi.get())
                    );
                    ahstat_inc(AhstatCounters::AhsToobig);
                    break 'drop Errno::EMSGSIZE;
                }
            }
            // INET6: the IPV6_MAXPACKET check; not configured.
            _ => {
                crate::ipsec_dprintf!(
                    "ah_output",
                    "unknown/unsupported protocol family {}, SA {}/{:08x}",
                    tdb.tdb_dst.get().sa_family(),
                    ipsp_address(&tdb.tdb_dst.get()),
                    ntohl(tdb.tdb_spi.get())
                );
                ahstat_inc(AhstatCounters::AhsNopf);
                break 'drop Errno::EPFNOSUPPORT;
            }
        }

        // Update the counters.
        tdb.tdb_cur_bytes
            .set(tdb.tdb_cur_bytes.get() + (m.m_pkthdr().len.get() - skip) as u64);
        ahstat_add(
            AhstatCounters::AhsObytes,
            (m.m_pkthdr().len.get() - skip) as u64,
        );

        // Hard expiration.
        if tdb.has_flags(TDBF_BYTES) && tdb.tdb_cur_bytes.get() >= tdb.tdb_exp_bytes.get() {
            ipsecstat_inc(IpsecCounters::IpsecExctdb);
            let _ = pfkeyv2_expire(tdb, SADB_EXT_LIFETIME_HARD);
            tdb_delete(tdb);
            break 'drop Errno::EINVAL;
        }

        // Notify on expiration.
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
                ahstat_inc(AhstatCounters::AhsHdrops);
                break 'drop Errno::ENOBUFS;
            };

            m_freem(m);
            m = n;
        }

        // Inject AH header.
        let Some((mi, roff)) = m_makespace(m, skip, rplen + authsize) else {
            crate::ipsec_dprintf!(
                "ah_output",
                "failed to inject AH header for SA {}/{:08x}",
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            ahstat_inc(AhstatCounters::AhsHdrops);
            break 'drop Errno::ENOBUFS;
        };

        // The AH header is guaranteed by m_makespace() to be in contiguous memory, at 'roff'
        // of the returned mbuf.
        // SAFETY: `m_makespace` made `rplen + authsize` contiguous bytes at `roff` of `mi`.
        let ah = unsafe { mtod::<u8>(mi).add(roff as usize) };

        // Initialize the AH header.
        let mut nh = [0u8; 1];
        m_copydata(m, protoff, &mut nh);
        let mut hdr = Ah {
            ah_nh: nh[0],
            ah_hl: ((rplen + authsize - AH_FLENGTH as i32) / 4) as u8,
            ah_rv: 0,
            ah_spi: tdb.tdb_spi.get(),
            ah_rpl: 0,
        };

        // Zeroize authenticator.
        let _ = m_copyback(m, skip + rplen, &IPSECZEROES[..authsize as usize], M_NOWAIT);

        mtx_enter(&tdb.tdb_mtx);
        let replay64 = tdb.tdb_rpl.get();
        tdb.tdb_rpl.set(replay64.wrapping_add(1));
        mtx_leave(&tdb.tdb_mtx);
        hdr.ah_rpl = htonl(replay64 as u32);
        // SAFETY: the AH header's place, `size_of::<Ah>()` bytes of the space made above.
        unsafe { ptr::write_unaligned(ah.cast::<Ah>(), hdr) };
        // NPFSYNC > 0: pfsync_update_tdb(tdb, 1); not configured.

        // Get crypto descriptors.
        let Some(mut crp) = crypto_getreq(1) else {
            crate::ipsec_dprintf!("ah_output", "failed to acquire crypto descriptors");
            ahstat_inc(AhstatCounters::AhsCrypto);
            break 'drop Errno::ENOBUFS;
        };

        {
            let crda = &mut crp.crp_desc[0];

            crda.crd_skip = 0;
            crda.crd_inject = skip + rplen;
            crda.crd_len = m.m_pkthdr().len.get();

            // Authentication operation.
            crda.CRD_INI.cri_alg = ahx.type_;
            // SAFETY: the TDB is held for the whole call; its key lives until `xf_zeroize`.
            crda.CRD_INI.cri_key = unsafe { tdb.tdb_amxkey() };
            crda.CRD_INI.cri_klen = i32::from(tdb.tdb_amxkeylen.get()) * 8;

            if tdb.tdb_wnd.get() > 0 && tdb.has_flags(TDBF_ESN) {
                let esn = htonl((replay64 >> 32) as u32);
                crda.set_crd_esn(esn.to_ne_bytes());
                crda.crd_flags |= CRD_F_ESN;
            }
        }

        let mut ptr = vec![0u8; skip as usize];

        // Save the skipped portion of the packet.
        m_copydata(m, 0, &mut ptr);

        // Fix IP header length on the header used for authentication. We don't need to fix
        // the original header length as it will be fixed by our caller.
        if tdb.tdb_dst.get().sa_family() == AF_INET {
            let at = offset_of!(Ip, ip_len);
            let iplen = u16::from_ne_bytes([ptr[at], ptr[at + 1]]);
            let iplen = htons(ntohs(iplen).wrapping_add((rplen + authsize) as u16));
            let _ = m_copyback(m, at as i32, &iplen.to_ne_bytes(), M_NOWAIT);
        }
        // INET6: the same for ip6_plen; not configured.

        // Fix the Next Header field in saved header.
        ptr[protoff as usize] = IPPROTO_AH as u8;

        // Update the Next Protocol field in the IP header.
        let prot = [IPPROTO_AH as u8];
        let _ = m_copyback(m, protoff, &prot, M_NOWAIT);

        // "Massage" the packet headers for crypto processing.
        let mut mp = Some(m);
        if let Err(error) = ah_massage_headers(
            &mut mp,
            tdb.tdb_dst.get().sa_family(),
            skip,
            ahx.type_,
            true,
        ) {
            // mbuf was freed by callee.
            crypto_freereq(Some(crp));
            return Err(error);
        }
        let Some(mm) = mp else {
            crypto_freereq(Some(crp));
            return Err(Errno::ENOBUFS);
        };
        m = mm;

        // Crypto operation descriptor.
        crp.crp_ilen = m.m_pkthdr().len.get(); // Total input length.
        crp.crp_flags = CRYPTO_F_IMBUF;
        crp.crp_buf = CryptoBuf::Mbuf(m);
        crp.crp_sid = tdb.tdb_cryptoid.get();

        if let Err(error) = ipsec_crypto_invoke(tdb, &mut crp) {
            crate::ipsec_dprintf!("ah_output", "crypto error {}", error as i32);
            ipsecstat_inc(IpsecCounters::IpsecNoxform);
            crypto_freereq(Some(crp));
            break 'drop error;
        }

        // Release the crypto descriptors
        crypto_freereq(Some(crp));

        // Copy original headers (with the new protocol number) back in place.
        let _ = m_copyback(m, 0, &ptr, M_NOWAIT);
        drop(ptr);

        // Call the IPsec input callback.
        let error = ipsp_process_done(m, tdb);
        if error.is_err() {
            ahstat_inc(AhstatCounters::AhsOutfail);
        }
        return error;
    };
    // drop:
    m_freem(m);
    Err(error)
}

// LP64 sizes of the C structures.
const _: () = {
    assert!(size_of::<Ahstat>() == AhstatCounters::AhsNcounters as usize * 8);
    assert!(size_of::<Ah>() == AH_FLENGTH + 4);
};
