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
 * Additional features in 1999 by Angelos D. Keromytis.
 *
 * Copyright (C) 1995, 1996, 1997, 1998, 1999 by John Ioannidis,
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
/* </LICENSES> */

/* <CODE> */
//! The IP Encapsulating Security Payload (ESP, RFC 4303): `<netinet/ip_esp.h>` (the
//! statistics and the sysctl names) and `netinet/ip_esp.c` (the transform: SA setup with the
//! crypto framework, input decryption and authentication, output encryption, and the
//! anti-replay window shared with AH).
//!
//! Upstream: sys/netinet/ip_esp.h @ 3ce1f3f79392
//! Upstream: sys/netinet/ip_esp.c @ 3ce1f3f79392
//!
//! The crypto framework is synchronous at this pin: `esp_input` and `esp_output` call
//! `crypto_invoke` and go on with the result in the same call.
//!
//! Status: `ported` (M9c).
//!
//! ## Deviations
//! - `espcounters` (`struct cpumem *`) is the static array of atomics `ESPCOUNTERS` in
//!   `netinet/ipsec_input.rs`, which defines the C's pointer; `esp_enable` is `ESP_ENABLE`
//!   there, `udpencap_enable`/`udpencap_port` are in `netinet/ipsec_output.rs`.
//! - The crypto request is the framework's [`Cryptop`] value (`crypto_getreq` returns it,
//!   `crypto_freereq` takes it back); the descriptors are indexed instead of being two
//!   pointers into the request. The keys handed to the descriptors are the TDB's raw keys,
//!   borrowed while the TDB is held.
//! - `checkreplaywindow` returns the C's codes (0..3) as an `i32` and the high half of the
//!   sequence number through `&mut u32`, as the C does: the callers switch on the code.
//! - The ESP header removal of `esp_input` moves bytes inside an mbuf with `ptr::copy` (the
//!   C's `memmove`), under a `// SAFETY:` naming the bounds.
//! - `NBPFILTER` is configured: `esp_output` counts the packet on the SA's `enc(4)`
//!   interface and taps it. `NPFSYNC` is: `pfsync_update_tdb`.
//! - `INET6` is configured (feature `inet6`): `esp_output`'s IPv6 size check.

use core::ptr;
use core::sync::atomic::Ordering;

use crate::crypto::crypto::{
    crypto_freereq, crypto_freesession, crypto_getreq, crypto_invoke, crypto_newsession,
};
use crate::crypto::cryptodev::{
    CRD_F_ENCRYPT, CRD_F_ESN, CRD_F_IV_EXPLICIT, CRYPTO_AES_CTR, CRYPTO_AES_GCM_16,
    CRYPTO_AES_GMAC, CRYPTO_CHACHA20_POLY1305, CRYPTO_ESN, CRYPTO_F_IMBUF, CryptoBuf, Cryptoini,
    Cryptop,
};
use crate::crypto::xform::{
    AuthHash, EncXform, auth_hash_chacha20_poly1305, auth_hash_gmac_aes_128,
    auth_hash_gmac_aes_192, auth_hash_gmac_aes_256, auth_hash_hmac_md5_96,
    auth_hash_hmac_ripemd_160_96, auth_hash_hmac_sha1_96, auth_hash_hmac_sha2_256_128,
    auth_hash_hmac_sha2_384_192, auth_hash_hmac_sha2_512_256, enc_xform_3des, enc_xform_aes,
    enc_xform_aes_ctr, enc_xform_aes_gcm, enc_xform_aes_gmac, enc_xform_blf, enc_xform_cast5,
    enc_xform_chacha20_poly1305, enc_xform_null,
};
use crate::dev::rnd::arc4random_buf;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{
    m_adj, m_copyback, m_copydata, m_dup_pkt, m_freem, m_getptr, m_makespace,
};
use crate::net::bpf::{BPF_DIRECTION_OUT, bpf_mtap_hdr};
use crate::net::if_enc::{Enchdr, enc_getif};
use crate::net::if_var::Netstack;
use crate::net::pfkeyv2::{
    SADB_AALG_MD5HMAC, SADB_AALG_SHA1HMAC, SADB_EALG_3DESCBC, SADB_EALG_NULL,
    SADB_EXT_LIFETIME_HARD, SADB_EXT_LIFETIME_SOFT, SADB_X_AALG_AES128GMAC, SADB_X_AALG_AES192GMAC,
    SADB_X_AALG_AES256GMAC, SADB_X_AALG_CHACHA20POLY1305, SADB_X_AALG_RIPEMD160HMAC,
    SADB_X_AALG_SHA2_256, SADB_X_AALG_SHA2_384, SADB_X_AALG_SHA2_512, SADB_X_EALG_AES,
    SADB_X_EALG_AESCTR, SADB_X_EALG_AESGCM16, SADB_X_EALG_AESGMAC, SADB_X_EALG_BLF,
    SADB_X_EALG_CAST, SADB_X_EALG_CHACHA20POLY1305, pfkeyv2_expire,
};
use crate::netinet::in_::{IPPROTO_DONE, IPPROTO_ESP};
use crate::netinet::ip::IP_MAXPACKET;
use crate::netinet::ip_ipsp::{
    AH_HMAC_INITIAL_RPL, AH_HMAC_MAX_HASHLEN, IpsecCounters, IpsecInit, TDB_REPLAYMAX,
    TDB_REPLAYWASTE, TDB_SEEN_WORDS, TDBF_BYTES, TDBF_ESN, TDBF_SOFT_BYTES, Tdb, TdbCounters,
    Xformsw, ipsecstat_inc, ipsp_address, tdb_delete, tdbstat_add,
};
#[cfg(feature = "inet6")]
use crate::netinet::ip6::IPV6_MAXPACKET;
use crate::netinet::ipsec_input::{ESPCOUNTERS, ipsec_common_input_cb};
use crate::netinet::ipsec_output::ipsp_process_done;
use crate::sys::endian::{htonl, ntohl};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::malloc::{M_WAITOK, M_XDATA};
use crate::sys::mbuf::{M_AUTH, M_CONF, M_DONTWAIT, Mbuf, m_freemp, m_readonly, mtod};
use crate::sys::mutex::mutex_assert_locked;
use crate::sys::socket::AF_INET;
#[cfg(feature = "inet6")]
use crate::sys::socket::AF_INET6;
use crate::sys::systm::{kernel_lock, kernel_unlock};
use libkern::{explicit_bzero, timingsafe_bcmp};

/// `struct espstat`: the ESP statistics as `net.inet.esp.stats` returns them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Espstat {
    /// Packet shorter than header shows.
    pub esps_hdrops: u64,
    /// Protocol family not supported.
    pub esps_nopf: u64,
    /// `esps_notdb`.
    pub esps_notdb: u64,
    /// `esps_badkcr`.
    pub esps_badkcr: u64,
    /// `esps_qfull`.
    pub esps_qfull: u64,
    /// `esps_noxform`.
    pub esps_noxform: u64,
    /// `esps_badilen`.
    pub esps_badilen: u64,
    /// Replay counter wrapped around.
    pub esps_wrap: u64,
    /// Bad encryption detected.
    pub esps_badenc: u64,
    /// Only valid for transforms with auth.
    pub esps_badauth: u64,
    /// Possible packet replay detected.
    pub esps_replay: u64,
    /// Input ESP packets.
    pub esps_input: u64,
    /// Output ESP packets.
    pub esps_output: u64,
    /// Trying to use an invalid TDB.
    pub esps_invalid: u64,
    /// Input bytes.
    pub esps_ibytes: u64,
    /// Output bytes.
    pub esps_obytes: u64,
    /// Packet got larger than `IP_MAXPACKET`.
    pub esps_toobig: u64,
    /// Packet blocked due to policy.
    pub esps_pdrops: u64,
    /// Crypto processing failure.
    pub esps_crypto: u64,
    /// Input ESP-in-UDP packets.
    pub esps_udpencin: u64,
    /// Output ESP-in-UDP packets.
    pub esps_udpencout: u64,
    /// Invalid input ESP-in-UDP packets.
    pub esps_udpinval: u64,
    /// Trying to use a ESP-in-UDP TDB.
    pub esps_udpneeded: u64,
    /// Packet output failure.
    pub esps_outfail: u64,
}

// Names for ESP sysctl objects

/// `ESPCTL_ENABLE`: enable ESP processing.
pub const ESPCTL_ENABLE: i32 = 1;
/// `ESPCTL_UDPENCAP_ENABLE`: enable ESP over UDP.
pub const ESPCTL_UDPENCAP_ENABLE: i32 = 2;
/// `ESPCTL_UDPENCAP_PORT`: UDP port for encapsulation.
pub const ESPCTL_UDPENCAP_PORT: i32 = 3;
/// `ESPCTL_STATS`: ESP stats.
pub const ESPCTL_STATS: i32 = 4;
/// `ESPCTL_MAXID`.
pub const ESPCTL_MAXID: i32 = 5;

/// `enum espstat_counters`: one per field of [`Espstat`], in the same order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum EspstatCounters {
    /// Packet shorter than header shows.
    EspsHdrops,
    /// Protocol family not supported.
    EspsNopf,
    /// `esps_notdb`.
    EspsNotdb,
    /// `esps_badkcr`.
    EspsBadkcr,
    /// `esps_qfull`.
    EspsQfull,
    /// `esps_noxform`.
    EspsNoxform,
    /// `esps_badilen`.
    EspsBadilen,
    /// Replay counter wrapped around.
    EspsWrap,
    /// Bad encryption detected.
    EspsBadenc,
    /// Only valid for transforms with auth.
    EspsBadauth,
    /// Possible packet replay detected.
    EspsReplay,
    /// Input ESP packets.
    EspsInput,
    /// Output ESP packets.
    EspsOutput,
    /// Trying to use an invalid TDB.
    EspsInvalid,
    /// Input bytes.
    EspsIbytes,
    /// Output bytes.
    EspsObytes,
    /// Packet got larger than `IP_MAXPACKET`.
    EspsToobig,
    /// Packet blocked due to policy.
    EspsPdrops,
    /// Crypto processing failure.
    EspsCrypto,
    /// Input ESP-in-UDP packets.
    EspsUdpencin,
    /// Output ESP-in-UDP packets.
    EspsUdpencout,
    /// Invalid input ESP-in-UDP packets.
    EspsUdpinval,
    /// Trying to use a ESP-in-UDP TDB.
    EspsUdpneeded,
    /// Packet output failure.
    EspsOutfail,
    /// `esps_ncounters`.
    EspsNcounters,
}

/// `espstat_inc(c)`.
pub fn espstat_inc(c: EspstatCounters) {
    ESPCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `espstat_add(c, v)`.
pub fn espstat_add(c: EspstatCounters, v: u64) {
    ESPCOUNTERS[c as usize].fetch_add(v, Ordering::Relaxed);
}

/// `esp_attach`: called from the transformation initialization code.
pub fn esp_attach() -> i32 {
    0
}

/// Copies `key` into a new `malloc(M_XDATA)` buffer (the TDB's raw key).
fn esp_savekey(key: &[u8]) -> *mut u8 {
    let Some(p) = malloc(key.len().max(1), M_XDATA, M_WAITOK) else {
        panic(format_args!("esp_savekey: malloc(M_WAITOK) failed"));
    };
    // SAFETY: a fresh allocation of at least `key.len()` bytes.
    unsafe { ptr::copy_nonoverlapping(key.as_ptr(), p.as_ptr(), key.len()) };
    p.as_ptr()
}

/// `esp_init`: called when an SPI is being set up: picks the cipher and the authenticator,
/// saves the raw keys and creates the crypto session.
pub fn esp_init(tdbp: &Tdb, xsp: &'static Xformsw, ii: &mut IpsecInit<'_>) -> Result<(), Errno> {
    let mut txform: Option<&'static EncXform> = None;
    let mut thash: Option<&'static AuthHash> = None;

    if ii.ii_encalg == 0 && ii.ii_authalg == 0 {
        crate::ipsec_dprintf!(
            "esp_init",
            "neither authentication nor encryption algorithm given"
        );
        return Err(Errno::EINVAL);
    }

    if ii.ii_encalg != 0 {
        let t: &'static EncXform = match ii.ii_encalg {
            SADB_EALG_NULL => &enc_xform_null,
            SADB_EALG_3DESCBC => &enc_xform_3des,
            SADB_X_EALG_AES => &enc_xform_aes,
            SADB_X_EALG_AESCTR => &enc_xform_aes_ctr,
            SADB_X_EALG_AESGCM16 => &enc_xform_aes_gcm,
            SADB_X_EALG_AESGMAC => &enc_xform_aes_gmac,
            SADB_X_EALG_CHACHA20POLY1305 => &enc_xform_chacha20_poly1305,
            SADB_X_EALG_BLF => &enc_xform_blf,
            SADB_X_EALG_CAST => &enc_xform_cast5,
            _ => {
                crate::ipsec_dprintf!(
                    "esp_init",
                    "unsupported encryption algorithm {} specified",
                    ii.ii_encalg
                );
                return Err(Errno::EINVAL);
            }
        };

        if ii.ii_enckeylen < t.minkey {
            crate::ipsec_dprintf!(
                "esp_init",
                "keylength {} too small (min length is {}) for algorithm {}",
                ii.ii_enckeylen,
                t.minkey,
                t.name
            );
            return Err(Errno::EINVAL);
        }

        if ii.ii_enckeylen > t.maxkey {
            crate::ipsec_dprintf!(
                "esp_init",
                "keylength {} too large (max length is {}) for algorithm {}",
                ii.ii_enckeylen,
                t.maxkey,
                t.name
            );
            return Err(Errno::EINVAL);
        }

        if ii.ii_encalg == SADB_X_EALG_AESGCM16 || ii.ii_encalg == SADB_X_EALG_AESGMAC {
            match ii.ii_enckeylen {
                20 => ii.ii_authalg = SADB_X_AALG_AES128GMAC,
                28 => ii.ii_authalg = SADB_X_AALG_AES192GMAC,
                36 => ii.ii_authalg = SADB_X_AALG_AES256GMAC,
                _ => {}
            }
            ii.ii_authkeylen = ii.ii_enckeylen;
            ii.ii_authkey = ii.ii_enckey;
        } else if ii.ii_encalg == SADB_X_EALG_CHACHA20POLY1305 {
            ii.ii_authalg = SADB_X_AALG_CHACHA20POLY1305;
            ii.ii_authkeylen = ii.ii_enckeylen;
            ii.ii_authkey = ii.ii_enckey;
        }

        tdbp.tdb_encalgxform.set(Some(t));
        txform = Some(t);

        crate::ipsec_dprintf!("esp_init", "initialized TDB with enc algorithm {}", t.name);

        tdbp.tdb_ivlen.set(t.ivsize);
    }

    if ii.ii_authalg != 0 {
        let h: &'static AuthHash = match ii.ii_authalg {
            SADB_AALG_MD5HMAC => &auth_hash_hmac_md5_96,
            SADB_AALG_SHA1HMAC => &auth_hash_hmac_sha1_96,
            SADB_X_AALG_RIPEMD160HMAC => &auth_hash_hmac_ripemd_160_96,
            SADB_X_AALG_SHA2_256 => &auth_hash_hmac_sha2_256_128,
            SADB_X_AALG_SHA2_384 => &auth_hash_hmac_sha2_384_192,
            SADB_X_AALG_SHA2_512 => &auth_hash_hmac_sha2_512_256,
            SADB_X_AALG_AES128GMAC => &auth_hash_gmac_aes_128,
            SADB_X_AALG_AES192GMAC => &auth_hash_gmac_aes_192,
            SADB_X_AALG_AES256GMAC => &auth_hash_gmac_aes_256,
            SADB_X_AALG_CHACHA20POLY1305 => &auth_hash_chacha20_poly1305,
            _ => {
                crate::ipsec_dprintf!(
                    "esp_init",
                    "unsupported authentication algorithm {} specified",
                    ii.ii_authalg
                );
                return Err(Errno::EINVAL);
            }
        };

        if ii.ii_authkeylen != h.keysize {
            crate::ipsec_dprintf!(
                "esp_init",
                "keylength {} doesn't match algorithm {} keysize ({})",
                ii.ii_authkeylen,
                h.name,
                h.keysize
            );
            return Err(Errno::EINVAL);
        }

        tdbp.tdb_authalgxform.set(Some(h));
        thash = Some(h);

        crate::ipsec_dprintf!("esp_init", "initialized TDB with hash algorithm {}", h.name);
    }

    tdbp.tdb_xform.set(Some(xsp));
    tdbp.tdb_rpl.set(AH_HMAC_INITIAL_RPL);

    // Initialize crypto session: the descriptions are chained last first (crin, cria, crie).
    let mut crin = Cryptoini::default();
    let mut cria = Cryptoini::default();
    let mut crie = Cryptoini::default();

    if thash.is_some() {
        // Save the raw keys
        let authkey = &ii.ii_authkey[..usize::from(ii.ii_authkeylen).min(ii.ii_authkey.len())];
        tdbp.tdb_amxkeylen.set(ii.ii_authkeylen);
        tdbp.tdb_amxkey.set(esp_savekey(authkey));

        if tdbp.tdb_wnd.get() > 0 && tdbp.has_flags(TDBF_ESN) {
            crin.cri_alg = CRYPTO_ESN;
        }
    }
    let crin_ref = (crin.cri_alg != 0).then_some(&crin);

    if let Some(h) = thash {
        cria.cri_alg = h.type_;
        cria.cri_next = crin_ref;
        cria.cri_klen = i32::from(ii.ii_authkeylen) * 8;
        cria.cri_key = ii.ii_authkey;
    }

    if let Some(t) = txform {
        // Save the raw keys
        let enckey = &ii.ii_enckey[..usize::from(ii.ii_enckeylen).min(ii.ii_enckey.len())];
        tdbp.tdb_emxkeylen.set(ii.ii_enckeylen);
        tdbp.tdb_emxkey.set(esp_savekey(enckey));

        crie.cri_alg = t.type_;
        crie.cri_next = if thash.is_some() { Some(&cria) } else { None };
        crie.cri_klen = i32::from(ii.ii_enckeylen) * 8;
        crie.cri_key = ii.ii_enckey;
        // XXX Rounds ?
    }

    kernel_lock();
    let r = crypto_newsession(if txform.is_some() { &crie } else { &cria }, 0);
    kernel_unlock();
    match r {
        Ok(sid) => {
            tdbp.tdb_cryptoid.set(sid);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Wipes and frees a TDB raw key.
fn esp_freekey(key: &core::cell::Cell<*mut u8>, len: u16) {
    if let Some(p) = ptr::NonNull::new(key.get()) {
        // SAFETY: a key of `len` bytes `esp_savekey` (or `ah_init`) allocated.
        explicit_bzero(unsafe { core::slice::from_raw_parts_mut(p.as_ptr(), usize::from(len)) });
        free(p, M_XDATA, usize::from(len).max(1));
        key.set(ptr::null_mut());
    }
}

/// `esp_zeroize`: paranoia: wipes the keys and frees the crypto session.
pub fn esp_zeroize(tdbp: &Tdb) -> Result<(), Errno> {
    esp_freekey(&tdbp.tdb_amxkey, tdbp.tdb_amxkeylen.get());
    esp_freekey(&tdbp.tdb_emxkey, tdbp.tdb_emxkeylen.get());

    kernel_lock();
    let error = crypto_freesession(tdbp.tdb_cryptoid.get());
    kernel_unlock();
    tdbp.tdb_cryptoid.set(0);
    error
}

/// Runs `crp`, making the session again when the framework migrated it (`EAGAIN`).
pub(crate) fn ipsec_crypto_invoke(tdb: &Tdb, crp: &mut Cryptop<'_>) -> Result<(), Errno> {
    loop {
        match crypto_invoke(crp) {
            Err(Errno::EAGAIN) => {
                // Reset the session ID
                if tdb.tdb_cryptoid.get() != 0 {
                    tdb.tdb_cryptoid.set(crp.crp_sid);
                }
            }
            r => return r,
        }
    }
}

/// The replay window check of `esp_input` and `ah_input`: counts and logs a failure;
/// `false` when the packet is to be dropped.
fn esp_replay_check(tdb: &Tdb, func: &str, chk_rpl: i32) -> bool {
    let _ = func;
    match chk_rpl {
        0 => true, // All's well
        1 => {
            crate::ipsec_dprintf!(
                func,
                "replay counter wrapped for SA {}/{:08x}",
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            espstat_inc(EspstatCounters::EspsWrap);
            false
        }
        2 => {
            crate::ipsec_dprintf!(
                func,
                "old packet received in SA {}/{:08x}",
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            espstat_inc(EspstatCounters::EspsReplay);
            false
        }
        3 => {
            crate::ipsec_dprintf!(
                func,
                "duplicate packet received in SA {}/{:08x}",
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            espstat_inc(EspstatCounters::EspsReplay);
            false
        }
        _ => {
            crate::ipsec_dprintf!(
                func,
                "bogus value from checkreplaywindow() in SA {}/{:08x}",
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            espstat_inc(EspstatCounters::EspsReplay);
            false
        }
    }
}

/// `esp_input`: ESP input processing, called (eventually) through the protocol switch.
pub fn esp_input(
    mp: &mut Option<&'static Mbuf>,
    tdb: &'static Tdb,
    skip: i32,
    protoff: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let esph = tdb.tdb_authalgxform.get();
    let espx = tdb.tdb_encalgxform.get();
    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };
    let mut esn: u32 = 0;
    let mut abuf = [0u8; AH_HMAC_MAX_HASHLEN];
    let mut lastthree = [0u8; 3];
    let mut aalg = [0u8; AH_HMAC_MAX_HASHLEN];

    'drop: {
        // Determine the ESP header length
        let hlen = 2 * 4 + i32::from(tdb.tdb_ivlen.get()); // "new" ESP
        let alen = esph.map_or(0, |h| i32::from(h.authsize));
        let plen = m.m_pkthdr().len.get() - (skip + hlen + alen);
        if plen <= 0 {
            crate::ipsec_dprintf!("esp_input", "invalid payload length");
            espstat_inc(EspstatCounters::EspsBadilen);
            break 'drop;
        }

        if let Some(espx) = espx {
            // Verify payload length is multiple of encryption algorithm block size.
            if plen & (i32::from(espx.blocksize) - 1) != 0 {
                crate::ipsec_dprintf!(
                    "esp_input",
                    "payload of {} octets not a multiple of {} octets, SA {}/{:08x}",
                    plen,
                    espx.blocksize,
                    ipsp_address(&tdb.tdb_dst.get()),
                    ntohl(tdb.tdb_spi.get())
                );
                espstat_inc(EspstatCounters::EspsBadilen);
                break 'drop;
            }
        }

        // Replay window checking, if appropriate -- no value commitment.
        if tdb.tdb_wnd.get() > 0 {
            let mut b = [0u8; 4];
            m_copydata(m, skip + 4, &mut b);
            let btsx = ntohl(u32::from_ne_bytes(b));

            mtx_enter(&tdb.tdb_mtx);
            let chk_rpl = checkreplaywindow(tdb, tdb.tdb_rpl.get(), btsx, &mut esn, false);
            mtx_leave(&tdb.tdb_mtx);
            if !esp_replay_check(tdb, "esp_input", chk_rpl) {
                break 'drop;
            }
        }

        // Update the counters
        tdb.tdb_cur_bytes.set(tdb.tdb_cur_bytes.get() + plen as u64);
        tdbstat_add(tdb, TdbCounters::TdbIbytes, plen as u64);
        espstat_add(EspstatCounters::EspsIbytes, plen as u64);

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

        // Get crypto descriptors
        let Some(mut crp) = crypto_getreq(if esph.is_some() && espx.is_some() {
            2
        } else {
            1
        }) else {
            crate::ipsec_dprintf!("esp_input", "failed to acquire crypto descriptors");
            espstat_inc(EspstatCounters::EspsCrypto);
            break 'drop;
        };

        let crde = if let Some(esph) = esph {
            let crda = &mut crp.crp_desc[0];

            // Authentication descriptor
            crda.crd_skip = skip;
            crda.crd_inject = m.m_pkthdr().len.get() - alen;

            crda.CRD_INI.cri_alg = esph.type_;
            // SAFETY: the TDB is held for the whole call; its keys live until `xf_zeroize`.
            crda.CRD_INI.cri_key = unsafe { tdb.tdb_amxkey() };
            crda.CRD_INI.cri_klen = i32::from(tdb.tdb_amxkeylen.get()) * 8;

            if tdb.tdb_wnd.get() > 0 && tdb.has_flags(TDBF_ESN) {
                esn = htonl(esn);
                crda.set_crd_esn(esn.to_ne_bytes());
                crda.crd_flags |= CRD_F_ESN;
            }

            if espx.is_some_and(|x| {
                x.type_ == CRYPTO_AES_GCM_16 || x.type_ == CRYPTO_CHACHA20_POLY1305
            }) {
                crda.crd_len = hlen - i32::from(tdb.tdb_ivlen.get());
            } else {
                crda.crd_len = m.m_pkthdr().len.get() - (skip + alen);
            }

            // Copy the authenticator
            m_copydata(m, m.m_pkthdr().len.get() - alen, &mut abuf[..alen as usize]);
            1
        } else {
            0
        };

        // Crypto operation descriptor
        crp.crp_ilen = m.m_pkthdr().len.get(); // Total input length
        crp.crp_flags = CRYPTO_F_IMBUF;
        crp.crp_buf = CryptoBuf::Mbuf(m);
        crp.crp_sid = tdb.tdb_cryptoid.get();

        // Decryption descriptor
        if let Some(espx) = espx {
            let crde = &mut crp.crp_desc[crde];
            crde.crd_skip = skip + hlen;
            crde.crd_inject = skip + hlen - i32::from(tdb.tdb_ivlen.get());
            crde.CRD_INI.cri_alg = espx.type_;
            // SAFETY: as for the authentication key.
            crde.CRD_INI.cri_key = unsafe { tdb.tdb_emxkey() };
            crde.CRD_INI.cri_klen = i32::from(tdb.tdb_emxkeylen.get()) * 8;
            // XXX Rounds ?

            if crde.CRD_INI.cri_alg == CRYPTO_AES_GMAC {
                crde.crd_len = 0;
            } else {
                crde.crd_len = plen;
            }
        }

        if let Err(error) = ipsec_crypto_invoke(tdb, &mut crp) {
            let _ = error;
            crate::ipsec_dprintf!("esp_input", "crypto error {}", error as i32);
            ipsecstat_inc(IpsecCounters::IpsecNoxform);
            crypto_freereq(Some(crp));
            break 'drop;
        }

        // Release the crypto descriptors
        crypto_freereq(Some(crp));

        // If authentication was performed, check now.
        if let Some(esph) = esph {
            let authsize = usize::from(esph.authsize);
            // Copy the authenticator from the packet
            m_copydata(
                m,
                m.m_pkthdr().len.get() - authsize as i32,
                &mut aalg[..authsize],
            );

            // Verify authenticator
            if timingsafe_bcmp(&abuf[..authsize], &aalg[..authsize]) {
                crate::ipsec_dprintf!(
                    "esp_input",
                    "authentication failed for packet in SA {}/{:08x}",
                    ipsp_address(&tdb.tdb_dst.get()),
                    ntohl(tdb.tdb_spi.get())
                );
                espstat_inc(EspstatCounters::EspsBadauth);
                break 'drop;
            }

            // Remove trailing authenticator
            m_adj(m, -(authsize as i32));
        }

        // Replay window checking, if appropriate
        if tdb.tdb_wnd.get() > 0 {
            let mut b = [0u8; 4];
            m_copydata(m, skip + 4, &mut b);
            let btsx = ntohl(u32::from_ne_bytes(b));

            mtx_enter(&tdb.tdb_mtx);
            let chk_rpl = checkreplaywindow(tdb, tdb.tdb_rpl.get(), btsx, &mut esn, true);
            mtx_leave(&tdb.tdb_mtx);
            if chk_rpl == 0 {
                // All's well
                crate::net::if_pfsync::pfsync_update_tdb(tdb, false);
            }
            if !esp_replay_check(tdb, "esp_input", chk_rpl) {
                break 'drop;
            }
        }

        // Find beginning of ESP header
        let Some((m1, roff)) = m_getptr(m, skip) else {
            crate::ipsec_dprintf!(
                "esp_input",
                "bad mbuf chain, SA {}/{:08x}",
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            espstat_inc(EspstatCounters::EspsHdrops);
            break 'drop;
        };

        // Remove the ESP header and IV from the mbuf.
        esp_strip_header(m, m1, roff, hlen);

        // Save the last three bytes of decrypted data
        m_copydata(m, m.m_pkthdr().len.get() - 3, &mut lastthree);

        // Verify pad length
        if i32::from(lastthree[1]) + 2 > m.m_pkthdr().len.get() - skip {
            crate::ipsec_dprintf!(
                "esp_input",
                "invalid padding length {} for packet in SA {}/{:08x}",
                lastthree[1],
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            espstat_inc(EspstatCounters::EspsBadilen);
            break 'drop;
        }

        // Verify correct decryption by checking the last padding bytes
        if lastthree[1] != lastthree[0] && lastthree[1] != 0 {
            crate::ipsec_dprintf!(
                "esp_input",
                "decryption failed for packet in SA {}/{:08x}",
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            espstat_inc(EspstatCounters::EspsBadenc);
            break 'drop;
        }

        // Trim the mbuf chain to remove the padding
        m_adj(m, -(i32::from(lastthree[1]) + 2));

        // Restore the Next Protocol field
        let _ = m_copyback(m, protoff, &lastthree[2..3], M_NOWAIT);

        // Back to generic IPsec input processing
        return ipsec_common_input_cb(mp, tdb, skip, protoff, ns);
    }
    // drop:
    m_freemp(mp);
    IPPROTO_DONE
}

/// Removes the `hlen` bytes of a header (ESP, or AH) found at offset `roff` of `m1` in the
/// chain `m`, as `esp_input` and `ah_input` do: at the start of `m1`, straddling into the next
/// mbufs, or in its middle.
pub(crate) fn esp_strip_header(m: &'static Mbuf, m1: &'static Mbuf, roff: i32, hlen: i32) {
    if roff == 0 {
        // The ESP header was conveniently at the beginning of the mbuf
        m_adj(m1, hlen);
        // If m1 is the first mbuf, it has set M_PKTHDR and m_adj() has already adjusted the
        // packet header length for us.
        if !ptr::eq(m1, m) {
            m.m_pkthdr().len.set(m.m_pkthdr().len.get() - hlen);
        }
    } else if roff + hlen >= m1.m_len().get() as i32 {
        // Part or all of the ESP header is at the end of this mbuf, so first let's remove the
        // remainder of the ESP header from the beginning of the remainder of the mbuf chain,
        // if any.
        if roff + hlen > m1.m_len().get() as i32 {
            let adjlen = roff + hlen - m1.m_len().get() as i32;

            // Adjust the next mbuf by the remainder
            m_adj(m1.m_next().get(), adjlen);

            // The second mbuf is guaranteed not to have a pkthdr
            m.m_pkthdr().len.set(m.m_pkthdr().len.get() - adjlen);
        }

        // Now, let's unlink the mbuf chain for a second...
        let mo = m1.m_next().get();
        m1.m_next().set(None);

        // ...and trim the end of the first part of the chain...sick
        let adjlen = m1.m_len().get() as i32 - roff;
        m_adj(m1, -adjlen);
        // If m1 is the first mbuf, it has set M_PKTHDR and m_adj() has already adjusted the
        // packet header length for us.
        if !ptr::eq(m1, m) {
            m.m_pkthdr().len.set(m.m_pkthdr().len.get() - adjlen);
        }

        // Finally, let's relink
        m1.m_next().set(mo);
    } else {
        // The ESP header lies in the "middle" of the mbuf...do an overlapping copy of the
        // remainder of the mbuf over the ESP header.
        let base = mtod::<u8>(m1);
        let n = m1.m_len().get() as usize - (roff + hlen) as usize;
        // SAFETY: `roff + hlen < m_len` (the branch above), so both ranges lie inside the
        // mbuf's valid data; `ptr::copy` allows the overlap.
        unsafe { ptr::copy(base.add((roff + hlen) as usize), base.add(roff as usize), n) };
        m1.m_len().set(m1.m_len().get() - hlen as u32);
        m.m_pkthdr().len.set(m.m_pkthdr().len.get() - hlen);
    }
}

/// `esp_output`: ESP output routine, called by `ipsp_process_packet()`.
pub fn esp_output(
    m: &'static Mbuf,
    tdb: &'static Tdb,
    skip: i32,
    protoff: i32,
) -> Result<(), Errno> {
    let espx = tdb.tdb_encalgxform.get();
    let esph = tdb.tdb_authalgxform.get();
    let mut m = m;

    if let Some(encif) = enc_getif(tdb.tdb_rdomain.get(), tdb.tdb_tap.get()) {
        encif.if_opackets().set(encif.if_opackets().get() + 1);
        encif
            .if_obytes()
            .set(encif.if_obytes().get() + m.m_pkthdr().len.get() as u64);

        let if_bpf = encif.if_bpf.get();
        if !if_bpf.is_null() {
            let mut hdr = Enchdr {
                af: u32::from(tdb.tdb_dst.get().sa_family()).to_be(),
                spi: tdb.tdb_spi.get(),
                flags: 0,
            };

            // The C sets these two in host order, unlike ah_output and ipsec_input.
            if espx.is_some() {
                hdr.flags |= u32::from(M_CONF);
            }
            if esph.is_some() {
                hdr.flags |= u32::from(M_AUTH);
            }

            let _ = bpf_mtap_hdr(if_bpf, &hdr.to_bytes(), m, BPF_DIRECTION_OUT);
        }
    }

    let hlen = 2 * 4 + i32::from(tdb.tdb_ivlen.get());

    let rlen = m.m_pkthdr().len.get() - skip; // Raw payload length.
    let blks = match espx {
        Some(x) => i32::from(x.blocksize).max(4),
        None => 4, // If no encryption, we have to be 4-byte aligned.
    };

    let padding = ((blks - ((rlen + 2) % blks)) % blks) + 2;

    let alen = esph.map_or(0, |h| i32::from(h.authsize));
    espstat_inc(EspstatCounters::EspsOutput);

    let error: Errno = 'drop: {
        match tdb.tdb_dst.get().sa_family() {
            AF_INET => {
                // Check for IP maximum packet size violations.
                if skip + hlen + rlen + padding + alen > IP_MAXPACKET as i32 {
                    crate::ipsec_dprintf!(
                        "esp_output",
                        "packet in SA {}/{:08x} got too big",
                        ipsp_address(&tdb.tdb_dst.get()),
                        ntohl(tdb.tdb_spi.get())
                    );
                    espstat_inc(EspstatCounters::EspsToobig);
                    break 'drop Errno::EMSGSIZE;
                }
            }
            #[cfg(feature = "inet6")]
            AF_INET6 => {
                // Check for IPv6 maximum packet size violations.
                if (skip + hlen + rlen + padding + alen) as usize > IPV6_MAXPACKET {
                    crate::ipsec_dprintf!(
                        "esp_output",
                        "packet in SA {}/{:08x} got too big",
                        ipsp_address(&tdb.tdb_dst.get()),
                        ntohl(tdb.tdb_spi.get())
                    );
                    espstat_inc(EspstatCounters::EspsToobig);
                    break 'drop Errno::EMSGSIZE;
                }
            }
            _ => {
                crate::ipsec_dprintf!(
                    "esp_output",
                    "unknown/unsupported protocol family {}, SA {}/{:08x}",
                    tdb.tdb_dst.get().sa_family(),
                    ipsp_address(&tdb.tdb_dst.get()),
                    ntohl(tdb.tdb_spi.get())
                );
                espstat_inc(EspstatCounters::EspsNopf);
                break 'drop Errno::EPFNOSUPPORT;
            }
        }

        // Update the counters.
        tdb.tdb_cur_bytes
            .set(tdb.tdb_cur_bytes.get() + (m.m_pkthdr().len.get() - skip) as u64);
        espstat_add(
            EspstatCounters::EspsObytes,
            (m.m_pkthdr().len.get() - skip) as u64,
        );

        // Hard byte expiration.
        if tdb.has_flags(TDBF_BYTES) && tdb.tdb_cur_bytes.get() >= tdb.tdb_exp_bytes.get() {
            ipsecstat_inc(IpsecCounters::IpsecExctdb);
            let _ = pfkeyv2_expire(tdb, SADB_EXT_LIFETIME_HARD);
            tdb_delete(tdb);
            break 'drop Errno::EINVAL;
        }

        // Soft byte expiration.
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
                    "esp_output",
                    "bad mbuf chain, SA {}/{:08x}",
                    ipsp_address(&tdb.tdb_dst.get()),
                    ntohl(tdb.tdb_spi.get())
                );
                espstat_inc(EspstatCounters::EspsHdrops);
                break 'drop Errno::ENOBUFS;
            };

            m_freem(m);
            m = n;
        }

        // Inject ESP header.
        let Some((mo, roff)) = m_makespace(m, skip, hlen) else {
            crate::ipsec_dprintf!(
                "esp_output",
                "failed to inject ESP header for SA {}/{:08x}",
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            espstat_inc(EspstatCounters::EspsHdrops);
            break 'drop Errno::ENOBUFS;
        };

        // Initialize ESP header.
        mtx_enter(&tdb.tdb_mtx);
        let replay64 = tdb.tdb_rpl.get(); // used for both header and ESN
        tdb.tdb_rpl.set(replay64.wrapping_add(1));
        mtx_leave(&tdb.tdb_mtx);
        let replay = htonl(replay64 as u32);
        // SAFETY: `m_makespace` made `hlen` contiguous bytes at `roff` of `mo`; the SPI and
        // the replay counter are its first 8.
        unsafe {
            let p = mtod::<u8>(mo).add(roff as usize);
            ptr::write_unaligned(p.cast::<u32>(), tdb.tdb_spi.get());
            ptr::write_unaligned(p.add(4).cast::<u32>(), replay);
        }

        crate::net::if_pfsync::pfsync_update_tdb(tdb, true);

        // Add padding -- better to do it ourselves than use the crypto engine, although
        // if/when we support compression, we'd have to do that.
        let Some((mo, roff)) = m_makespace(m, m.m_pkthdr().len.get(), padding + alen) else {
            crate::ipsec_dprintf!(
                "esp_output",
                "m_makespace() failed for SA {}/{:08x}",
                ipsp_address(&tdb.tdb_dst.get()),
                ntohl(tdb.tdb_spi.get())
            );
            espstat_inc(EspstatCounters::EspsHdrops);
            break 'drop Errno::ENOBUFS;
        };
        // SAFETY: `m_makespace` made `padding + alen` contiguous bytes at `roff` of `mo`.
        let pad = unsafe {
            core::slice::from_raw_parts_mut(
                mtod::<u8>(mo).add(roff as usize),
                (padding + alen) as usize,
            )
        };

        // Apply self-describing padding
        for (ilen, b) in pad[..(padding - 2) as usize].iter_mut().enumerate() {
            *b = (ilen + 1) as u8;
        }

        // Fix padding length and Next Protocol in padding itself.
        pad[(padding - 2) as usize] = (padding - 2) as u8;
        m_copydata(
            m,
            protoff,
            &mut pad[(padding - 1) as usize..padding as usize],
        );

        // Fix Next Protocol in IPv4/IPv6 header.
        let prot = [IPPROTO_ESP as u8];
        let _ = m_copyback(m, protoff, &prot, M_NOWAIT);

        // Get crypto descriptors.
        let Some(mut crp) = crypto_getreq(if esph.is_some() && espx.is_some() {
            2
        } else {
            1
        }) else {
            crate::ipsec_dprintf!("esp_output", "failed to acquire crypto descriptors");
            espstat_inc(EspstatCounters::EspsCrypto);
            break 'drop Errno::ENOBUFS;
        };

        let crda = if let Some(espx) = espx {
            let crde = &mut crp.crp_desc[0];

            // Encryption descriptor.
            crde.crd_skip = skip + hlen;
            crde.crd_flags = CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT;
            crde.crd_inject = skip + hlen - i32::from(tdb.tdb_ivlen.get());

            // Encryption operation.
            crde.CRD_INI.cri_alg = espx.type_;
            // SAFETY: the TDB is held for the whole call; its keys live until `xf_zeroize`.
            crde.CRD_INI.cri_key = unsafe { tdb.tdb_emxkey() };
            crde.CRD_INI.cri_klen = i32::from(tdb.tdb_emxkeylen.get()) * 8;
            // XXX Rounds ?

            if crde.CRD_INI.cri_alg == CRYPTO_AES_GMAC {
                crde.crd_len = 0;
            } else {
                crde.crd_len = m.m_pkthdr().len.get() - (skip + hlen + alen);
            }

            // GCM & friends just require a NONCE (non-repeating!)
            if espx.type_ == CRYPTO_AES_CTR
                || espx.type_ == CRYPTO_AES_GCM_16
                || espx.type_ == CRYPTO_AES_GMAC
                || espx.type_ == CRYPTO_CHACHA20_POLY1305
            {
                crde.crd_iv_mut()[..8].copy_from_slice(&replay64.to_ne_bytes());
            } else {
                arc4random_buf(&mut crde.crd_iv_mut()[..usize::from(espx.ivsize)]);
            }
            1
        } else {
            0
        };

        // Crypto operation descriptor.
        crp.crp_ilen = m.m_pkthdr().len.get(); // Total input length.
        crp.crp_flags = CRYPTO_F_IMBUF;
        crp.crp_buf = CryptoBuf::Mbuf(m);
        crp.crp_sid = tdb.tdb_cryptoid.get();

        if let Some(esph) = esph {
            let crda = &mut crp.crp_desc[crda];
            // Authentication descriptor.
            crda.crd_skip = skip;
            crda.crd_inject = m.m_pkthdr().len.get() - alen;

            // Authentication operation.
            crda.CRD_INI.cri_alg = esph.type_;
            // SAFETY: as for the encryption key.
            crda.CRD_INI.cri_key = unsafe { tdb.tdb_amxkey() };
            crda.CRD_INI.cri_klen = i32::from(tdb.tdb_amxkeylen.get()) * 8;

            if tdb.tdb_wnd.get() > 0 && tdb.has_flags(TDBF_ESN) {
                let esn = htonl((replay64 >> 32) as u32);
                crda.set_crd_esn(esn.to_ne_bytes());
                crda.crd_flags |= CRD_F_ESN;
            }

            if espx.is_some_and(|x| {
                x.type_ == CRYPTO_AES_GCM_16 || x.type_ == CRYPTO_CHACHA20_POLY1305
            }) {
                crda.crd_len = hlen - i32::from(tdb.tdb_ivlen.get());
            } else {
                crda.crd_len = m.m_pkthdr().len.get() - (skip + alen);
            }
        }

        if let Err(error) = ipsec_crypto_invoke(tdb, &mut crp) {
            crate::ipsec_dprintf!("esp_output", "crypto error {}", error as i32);
            ipsecstat_inc(IpsecCounters::IpsecNoxform);
            crypto_freereq(Some(crp));
            break 'drop error;
        }

        // Release the crypto descriptors
        crypto_freereq(Some(crp));

        // Call the IPsec input callback.
        let error = ipsp_process_done(m, tdb);
        if error.is_err() {
            espstat_inc(EspstatCounters::EspsOutfail);
        }
        return error;
    };
    // drop:
    m_freem(m);
    Err(error)
}

/// `SEEN_SIZE`: the words of the anti-replay window.
const SEEN_SIZE: usize = TDB_SEEN_WORDS;

/// `checkreplaywindow`: the anti-replay check of sequence number `seq` against the window
/// ending at `t` (the TDB's `tdb_rpl`), committing it to the window with `commit`. `*seqh`
/// gets the high half of the 64-bit sequence number (ESN).
///
/// - return 0 on success
/// - return 1 for counter == 0
/// - return 2 for very old packet
/// - return 3 for packet within current window but already received
pub fn checkreplaywindow(tdb: &Tdb, t: u64, seq: u32, seqh: &mut u32, commit: bool) -> i32 {
    let mut window: u32 = TDB_REPLAYMAX - TDB_REPLAYWASTE;
    let esn = tdb.has_flags(TDBF_ESN);

    mutex_assert_locked(&tdb.tdb_mtx, "checkreplaywindow");

    let tl = t as u32;
    let th = (t >> 32) as u32;

    // Zero SN is not allowed
    if (esn && seq == 0 && u64::from(tl) <= AH_HMAC_INITIAL_RPL && th == 0) || (!esn && seq == 0) {
        return 1;
    }

    if th == 0 && tl < window {
        window = tl;
    }
    // Current replay window starts here
    let wl = tl.wrapping_sub(window).wrapping_add(1);

    let idx = ((seq % TDB_REPLAYMAX) / 32) as usize;
    let packet = 1u32 << (31 - (seq & 31));

    // Clears the window between the last one and `idx`, or all of it, and records `seq`.
    let advance = |seqh: u32| {
        if seq.wrapping_sub(tl) > window {
            for w in &tdb.tdb_seen {
                w.set(0);
            }
        } else {
            let mut i = ((tl % TDB_REPLAYMAX) / 32) as usize;

            while i != idx {
                i = (i + 1) % SEEN_SIZE;
                tdb.tdb_seen[i].set(0);
            }
        }
        tdb.tdb_seen[idx].set(tdb.tdb_seen[idx].get() | packet);
        tdb.tdb_rpl.set((u64::from(seqh) << 32) | u64::from(seq));
    };

    // We keep the high part intact when:
    // 1) the SN is within [wl, 0xffffffff] and the whole window is within one subspace;
    // 2) the SN is within [0, wl) and window spans two subspaces.
    if (tl >= window.wrapping_sub(1) && seq >= wl) || (tl < window.wrapping_sub(1) && seq < wl) {
        *seqh = th;
        if seq > tl {
            if commit {
                advance(*seqh);
            }
        } else {
            if tl - seq >= window {
                return 2;
            }
            if tdb.tdb_seen[idx].get() & packet != 0 {
                return 3;
            }
            if commit {
                tdb.tdb_seen[idx].set(tdb.tdb_seen[idx].get() | packet);
            }
        }
        return 0;
    }

    // Can't wrap if not doing ESN
    if !esn {
        return 2;
    }

    // (3) SN is within [wl, 0xffffffff] and wl is within (0xffffffff-window+1, 0xffffffff].
    // This means we got a SN which is within our replay window, but in the previous
    // subspace.
    if tl < window.wrapping_sub(1) && seq >= wl {
        if tdb.tdb_seen[idx].get() & packet != 0 {
            return 3;
        }
        *seqh = th.wrapping_sub(1);
        if commit {
            tdb.tdb_seen[idx].set(tdb.tdb_seen[idx].get() | packet);
        }
        return 0;
    }

    // (4) SN has wrapped and the last authenticated SN is in the old subspace.
    *seqh = th.wrapping_add(1);
    if *seqh == 0 {
        // Don't let high bit to wrap
        return 1;
    }
    if commit {
        advance(*seqh);
    }

    0
}

// LP64 sizes of the C structures.
const _: () = {
    assert!(size_of::<Espstat>() == EspstatCounters::EspsNcounters as usize * 8);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for ESP: the anti-replay window, and packets that go out through
    // `ipsp_process_packet` (the transform, `ipsp_process_done`, `ip_output` onto a test
    // Ethernet interface) and come back in through `esp46_input`, in transport and in tunnel
    // mode, with AES-CBC and HMAC-SHA2-256 and with AES-GCM, through the crypto framework's
    // software driver.

    use std::sync::MutexGuard;
    use std::vec::Vec;

    use super::*;
    use crate::crypto::crypto::crypto_reset;
    use crate::crypto::cryptosoft::swcr_init;
    use crate::net::ethertypes::{ETHERTYPE_ARP, ETHERTYPE_IP};
    use crate::net::if_::tests::test_packet;
    use crate::net::if_ethersubr::ether_input;
    use crate::net::ifq::ifq_dequeue;
    use crate::netinet::if_ether::{EtherArp, EtherHeader, arpintr};
    use crate::netinet::in_::{IPPROTO_ICMP, IPPROTO_IPV4};
    use crate::netinet::in_cksum::in_cksum;
    use crate::netinet::ip_input::tests::{
        ADDR, GATEWAY, OURS, PEER, bytes, configure, frame, sin, test_ether,
    };
    use crate::netinet::ip_ipsp::{
        IPSP_DF_INHERIT, IpsecInit, SockaddrUnion, TDBF_TUNNELING, XF_ESP, ipsp_reset, puttdb,
        tdb_alloc, tdb_init,
    };
    use crate::netinet::ip_var::mtod_ip;
    use crate::netinet::ipsec_input::{ESP_ENABLE, esp46_input};
    use crate::netinet::ipsec_output::ipsp_process_packet;
    use crate::sys::endian::htons;
    use crate::sys::socket::AF_INET;

    /// A TDB with replay window `wnd` and `rpl` as its counter, outside the tables.
    fn window(esn: bool, rpl: u64) -> Tdb {
        let t = Tdb::new();
        t.tdb_wnd.set(32);
        t.tdb_rpl.set(rpl);
        if esn {
            t.set_flags(TDBF_ESN);
        }
        t
    }

    /// `checkreplaywindow` under the TDB's mutex; the high half of the sequence number too.
    fn check(t: &Tdb, seq: u32, commit: bool) -> (i32, u32) {
        let mut seqh = 0;
        mtx_enter(&t.tdb_mtx);
        let r = checkreplaywindow(t, t.tdb_rpl.get(), seq, &mut seqh, commit);
        mtx_leave(&t.tdb_mtx);
        (r, seqh)
    }

    #[test]
    fn the_replay_window_accepts_new_numbers_once() {
        let t = window(false, AH_HMAC_INITIAL_RPL);

        assert_eq!(check(&t, 0, true).0, 1, "sequence number 0 is never valid");
        assert_eq!(check(&t, 1, false).0, 0, "first packet: checked");
        assert_eq!(check(&t, 1, true).0, 0, "then committed");
        assert_eq!(check(&t, 1, true).0, 3, "a duplicate");
        assert_eq!(check(&t, 5, true).0, 0, "ahead: the window moves");
        assert_eq!(t.tdb_rpl.get(), 5);
        assert_eq!(
            check(&t, 3, true).0,
            0,
            "behind, inside the window, not seen"
        );
        assert_eq!(check(&t, 3, false).0, 3, "seen now");
        assert_eq!(check(&t, 4, false).0, 0, "not seen, not committed");
        assert_eq!(check(&t, 4, false).0, 0, "still not seen");

        // Far ahead: the window starts over; old numbers are rejected.
        let far = 5 + TDB_REPLAYMAX + 10;
        assert_eq!(check(&t, far, true).0, 0);
        assert_eq!(t.tdb_rpl.get(), u64::from(far));
        assert_eq!(check(&t, 5, true).0, 2, "too old");
        let edge = far - (TDB_REPLAYMAX - TDB_REPLAYWASTE) + 1;
        assert_eq!(
            check(&t, edge, true).0,
            0,
            "the oldest number the window still covers"
        );
        assert_eq!(check(&t, edge - 1, true).0, 2, "one older");

        // Without ESN the counter cannot wrap.
        let t = window(false, 0xffff_fff0);
        assert_eq!(check(&t, 0xffff_fff8, true).0, 0);
        assert_eq!(check(&t, 3, true).0, 2);
    }

    #[test]
    fn with_esn_the_window_crosses_into_the_next_subspace() {
        let t = window(true, 0xffff_fff0);
        assert_eq!(check(&t, 0xffff_fffe, true), (0, 0));
        // Wrapped: the high half goes up.
        assert_eq!(check(&t, 2, true), (0, 1));
        assert_eq!(t.tdb_rpl.get(), (1 << 32) | 2);
        // A late packet of the previous subspace, inside the window.
        assert_eq!(check(&t, 0xffff_fff8, true), (0, 0));
        assert_eq!(
            check(&t, 0xffff_fff8, true).0,
            3,
            "a duplicate across the wrap"
        );
        assert_eq!(check(&t, 1, true), (0, 1));
    }

    /// Memory, the network with `tvio0` at 10.0.2.15/24, an empty SA database and the software
    /// crypto driver (under the crypto tests' lock).
    fn setup() -> (
        (MutexGuard<'static, ()>, MutexGuard<'static, ()>),
        MutexGuard<'static, ()>,
        &'static crate::net::if_var::Ifnet,
    ) {
        let g = crate::netinet::ip_input::tests::setup();
        let s = crate::crypto::testutil::serial();
        crypto_reset();
        swcr_init();
        ipsp_reset();
        ESP_ENABLE.store(1, core::sync::atomic::Ordering::Relaxed);
        let ifp = test_ether();
        configure(ifp, ADDR, [255, 255, 255, 0]);
        while let Some(m) = ifq_dequeue(&ifp.if_snd) {
            m_freem(m);
        }
        (g, s, ifp)
    }

    /// The cipher and authenticator of an SA.
    #[derive(Clone, Copy)]
    enum Suite {
        /// AES-128-CBC with HMAC-SHA2-256.
        CbcSha256,
        /// AES-128-GCM.
        Gcm,
    }

    const ENCKEY: [u8; 20] = [
        0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32,
        0x10, 0xca, 0xfe, 0xba, 0xbe,
    ];
    const AUTHKEY: [u8; 32] = [0x5a; 32];

    /// An ESP SA from us (10.0.2.15) to the gateway, set up by `tdb_init` (as `SADB_ADD` does)
    /// and put in the tables.
    fn esp_sa(suite: Suite, tunnel: bool) -> &'static Tdb {
        let t = tdb_alloc(0);
        t.tdb_spi.set(htonl(0x1234));
        t.tdb_sproto.set(IPPROTO_ESP as u8);
        t.tdb_src.set(SockaddrUnion::from_sin(&sin(ADDR)));
        t.tdb_dst.set(SockaddrUnion::from_sin(&sin(GATEWAY)));
        t.tdb_wnd.set(16);
        if tunnel {
            t.set_flags(TDBF_TUNNELING);
        }
        let mut ii = match suite {
            Suite::CbcSha256 => IpsecInit {
                ii_encalg: SADB_X_EALG_AES,
                ii_enckey: &ENCKEY[..16],
                ii_enckeylen: 16,
                ii_authalg: SADB_X_AALG_SHA2_256,
                ii_authkey: &AUTHKEY,
                ii_authkeylen: 32,
                ..IpsecInit::default()
            },
            Suite::Gcm => IpsecInit {
                ii_encalg: SADB_X_EALG_AESGCM16,
                ii_enckey: &ENCKEY,
                ii_enckeylen: 20,
                ..IpsecInit::default()
            },
        };
        tdb_init(t, XF_ESP, &mut ii).expect("esp_init");
        puttdb(t);
        t
    }

    /// An ICMP echo request from `src` to `dst` with a recognisable payload.
    fn icmp_packet(src: [u8; 4], dst: [u8; 4]) -> Vec<u8> {
        let mut p = std::vec![0u8; 20 + 8 + 37];
        p[0] = 0x45;
        let len = p.len() as u16;
        p[2..4].copy_from_slice(&len.to_be_bytes());
        p[6] = 0x40; // DF
        p[8] = 64;
        p[9] = IPPROTO_ICMP as u8;
        p[12..16].copy_from_slice(&src);
        p[16..20].copy_from_slice(&dst);
        p[20] = 8;
        for (i, b) in p[28..].iter_mut().enumerate() {
            *b = b"the quick brown fox jumps over IPsec"[i % 36];
        }
        // The header checksum (RFC 1071), computed here: no mbufs exist before the setup.
        let mut sum: u32 = p[..20]
            .chunks(2)
            .map(|w| u32::from(u16::from_be_bytes([w[0], w[1]])))
            .sum();
        while sum >> 16 != 0 {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        p[10..12].copy_from_slice(&(!(sum as u16)).to_be_bytes());
        p
    }

    /// What `ip_output` sent on `ifp`: the IP packet of the frame, answering the gateway's ARP
    /// request first if the packet waits for it.
    fn sent(ifp: &'static crate::net::if_var::Ifnet) -> Vec<u8> {
        let m = ifq_dequeue(&ifp.if_snd).expect("a frame");
        let b = bytes(m);
        m_freem(m);
        if b[12..14] == ETHERTYPE_ARP.to_be_bytes() {
            let reply = EtherArp {
                ea_hdr: crate::net::if_arp::Arphdr {
                    ar_hrd: htons(crate::net::if_arp::ARPHRD_ETHER),
                    ar_pro: htons(ETHERTYPE_IP),
                    ar_hln: 6,
                    ar_pln: 4,
                    ar_op: htons(crate::net::if_arp::ARPOP_REPLY),
                },
                arp_sha: PEER,
                arp_spa: GATEWAY,
                arp_tha: OURS,
                arp_tpa: ADDR,
            };
            // SAFETY: an `ether_arp` is plain bytes.
            let arp = unsafe {
                core::slice::from_raw_parts(
                    ptr::from_ref(&reply).cast::<u8>(),
                    size_of::<EtherArp>(),
                )
            };
            ether_input(ifp, frame(ifp, OURS, ETHERTYPE_ARP, arp), None);
            arpintr();
            return sent(ifp);
        }
        assert_eq!(&b[12..14], &ETHERTYPE_IP.to_be_bytes());
        b[size_of::<EtherHeader>()..].to_vec()
    }

    /// Sends `inner` through the SA of `suite`/`tunnel`, checks the ESP packet on the wire, then
    /// feeds it back through `esp46_input` with an inbound SA of the same keys and returns the
    /// protocol it answers and the packet after it.
    fn round_trip(suite: Suite, tunnel: bool, inner: &[u8]) -> (i32, Vec<u8>) {
        let (_g, _s, ifp) = setup();

        let out = esp_sa(suite, tunnel);
        let m = test_packet(inner);
        ipsp_process_packet(m, out, i32::from(AF_INET), false, IPSP_DF_INHERIT).expect("sent");
        assert_eq!(
            out.tdb_rpl.get(),
            AH_HMAC_INITIAL_RPL + 1,
            "one sequence number used"
        );
        let wire = sent(ifp);

        // On the wire: an IPv4 packet from us to the gateway carrying ESP, the SPI and sequence
        // number 1 after the header, and none of the plaintext.
        assert_eq!(wire[0], 0x45);
        assert_eq!(wire[9], IPPROTO_ESP as u8);
        assert_eq!(&wire[12..16], &ADDR);
        assert_eq!(&wire[16..20], &GATEWAY);
        assert_eq!(&wire[20..24], &0x1234u32.to_be_bytes());
        assert_eq!(&wire[24..28], &1u32.to_be_bytes());
        let fox = b"quick brown fox";
        assert!(!wire.windows(fox.len()).any(|w| w == fox), "encrypted");
        let m = test_packet(&wire);
        assert_eq!(in_cksum(m, 20), 0, "the outer header checksum");
        m_freem(m);

        // The way back: the same keys on an inbound SA (the outbound one gone).
        tdb_delete(out);
        let inb = esp_sa(suite, tunnel);
        let m = test_packet(&wire);
        m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
        let mut mp = Some(m);
        let mut off = 20;
        let prot = esp46_input(&mut mp, &mut off, IPPROTO_ESP, i32::from(AF_INET), None);
        let m = mp.expect("decrypted");
        assert_ne!(m.m_flags().get() & crate::sys::mbuf::M_CONF, 0);
        assert_ne!(m.m_flags().get() & crate::sys::mbuf::M_AUTH, 0);
        let back = bytes(m);
        m_freem(m);
        assert_eq!(
            inb.tdb_rpl.get(),
            1,
            "the window moved to the packet's number"
        );

        // A replay of the same packet is dropped.
        let m = test_packet(&wire);
        let mut mp = Some(m);
        let mut off = 20;
        let again = esp46_input(&mut mp, &mut off, IPPROTO_ESP, i32::from(AF_INET), None);
        assert_eq!(again, crate::netinet::in_::IPPROTO_DONE);
        assert!(mp.is_none());

        // A damaged packet fails authentication.
        let mut bad = wire.clone();
        let n = bad.len();
        bad[n - 20] ^= 1;
        bad[24..28].copy_from_slice(&2u32.to_be_bytes());
        let m = test_packet(&bad);
        let mut mp = Some(m);
        let mut off = 20;
        let r = esp46_input(&mut mp, &mut off, IPPROTO_ESP, i32::from(AF_INET), None);
        assert_eq!(r, crate::netinet::in_::IPPROTO_DONE);

        tdb_delete(inb);
        (prot, back)
    }

    #[test]
    fn esp_cbc_sha256_transport_mode_round_trip() {
        let inner = icmp_packet(ADDR, GATEWAY);
        let (prot, back) = round_trip(Suite::CbcSha256, false, &inner);
        assert_eq!(prot, IPPROTO_ICMP, "the next protocol is restored");
        assert_eq!(back.len(), inner.len());
        assert_eq!(&back[20..], &inner[20..], "the payload");
        let ip = mtod_ip(test_packet(&back));
        assert_eq!(ip.ip_p, IPPROTO_ICMP as u8);
        assert_eq!(usize::from(u16::from_be(ip.ip_len)), inner.len());
    }

    #[test]
    fn esp_gcm_transport_mode_round_trip() {
        let inner = icmp_packet(ADDR, GATEWAY);
        let (prot, back) = round_trip(Suite::Gcm, false, &inner);
        assert_eq!(prot, IPPROTO_ICMP);
        assert_eq!(&back[20..], &inner[20..]);
    }

    #[test]
    fn esp_cbc_sha256_tunnel_mode_round_trip() {
        let inner = icmp_packet([10, 77, 1, 1], [10, 77, 2, 1]);
        let (prot, back) = round_trip(Suite::CbcSha256, true, &inner);
        assert_eq!(prot, IPPROTO_IPV4, "an IP packet inside");
        assert_eq!(back[9], IPPROTO_IPV4 as u8, "the outer header says so");
        assert_eq!(&back[20..], &inner[..], "the inner packet, untouched");
    }

    #[test]
    fn esp_gcm_tunnel_mode_round_trip() {
        let inner = icmp_packet([10, 77, 1, 1], [10, 77, 2, 1]);
        let (prot, back) = round_trip(Suite::Gcm, true, &inner);
        assert_eq!(prot, IPPROTO_IPV4);
        assert_eq!(&back[20..], &inner[..]);
    }

    #[test]
    fn ipsec_hdrsz_counts_the_overhead() {
        let (_g, _s, _ifp) = setup();
        let t = esp_sa(Suite::CbcSha256, true);
        // SPI + sequence (8) + IV (16) + authenticator (16) + padding (16) + outer header (20).
        assert_eq!(
            crate::netinet::ipsec_output::ipsec_hdrsz(t),
            8 + 16 + 16 + 16 + 20
        );
        tdb_delete(t);
    }
}
/* </TESTS> */
