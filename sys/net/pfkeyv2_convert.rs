/* <LICENSES> */
/*
 * The author of this code is Angelos D. Keromytis (angelos@keromytis.org)
 *
 * Part of this code is based on code written by Craig Metz (cmetz@inner.net)
 * for NRL. Those licenses follow this one.
 *
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

/*
 * Copyright (c) 1995, 1996, 1997, 1998, 1999 Craig Metz. All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the author nor the names of any contributors
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
 */
/* </LICENSES> */

/* <CODE> */
//! Conversions between `PF_KEY` extensions and the kernel's IPsec state:
//! `net/pfkeyv2_convert.c`. The `import_*` functions set a TDB (or a flow, an
//! `ipsecinit`) up from a message's extensions; the `export_*` functions write a TDB's state
//! as extensions at a cursor in a message buffer.
//!
//! Upstream: sys/net/pfkeyv2_convert.c @ 3ce1f3f79392
//!
//! Status: `ported` (M9c).
//!
//! ## Deviations
//! - Extensions the C passes as typed pointers that it only reads (`struct sadb_sa *`,
//!   `struct sadb_lifetime *`, ...) come as `Option` copies (`None` for NULL); the ones whose
//!   trailing data matters (addresses, keys, identities) as raw pointers to the extension.
//! - The exports keep the C's `void **p` cursor as `&mut *mut u8`; they write whole
//!   structures with unaligned stores and are `unsafe fn`s whose contract is the room the
//!   caller sized. The buffer is zeroed, as the C's `malloc(M_ZERO)`, so fields the C leaves
//!   alone stay zero.
//! - `import_address` writes into a [`SockaddrUnion`] (its callers' `&tdb->tdb_src.sa`), at
//!   most the union's size.
//! - `import_identities` returns the shared identities (the C writes `*ids`); the identity
//!   allocations are `malloc(M_CREDENTIALS)` as in C, with `M_WAITOK` failures a panic.
//! - `import_flow` masks the flow's addresses in place in the message (`rt_maskedcopy`), as
//!   the C does.
//! - `NPF` (pf(4)) is configured: `import_tag`, `export_tag`, `import_tap`, `export_tap`;
//!   `import_tag` takes the raw extension, its name trailing the header.
//! - `INET6` is configured (feature `inet6`): the `AF_INET6` flows (`SENT_IP6`) of
//!   `import_flow`, `export_encap` and `export_flow`, `in6_embedscope`, and the
//!   `sockaddr_in6` size of `import_address`.

use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::crypto::cryptodev::{
    CRYPTO_3DES_CBC, CRYPTO_AES_128_GMAC, CRYPTO_AES_192_GMAC, CRYPTO_AES_256_GMAC, CRYPTO_AES_CBC,
    CRYPTO_AES_CTR, CRYPTO_AES_GCM_16, CRYPTO_AES_GMAC, CRYPTO_BLF_CBC, CRYPTO_CAST_CBC,
    CRYPTO_CHACHA20_POLY1305, CRYPTO_CHACHA20_POLY1305_MAC, CRYPTO_DEFLATE_COMP, CRYPTO_MD5_HMAC,
    CRYPTO_NULL, CRYPTO_RIPEMD160_HMAC, CRYPTO_SHA1_HMAC, CRYPTO_SHA2_256_HMAC,
    CRYPTO_SHA2_384_HMAC, CRYPTO_SHA2_512_HMAC,
};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::panic;
use crate::net::pf_ioctl::{pf_tag2tagname, pf_tagname2tag};
use crate::net::pfkeyv2::{
    PFKEYV2_ENCRYPTION_KEY, PFKEYV2_LIFETIME_CURRENT, PFKEYV2_LIFETIME_HARD,
    PFKEYV2_LIFETIME_LASTUSE, PFKEYV2_LIFETIME_SOFT, SADB_AALG_MD5HMAC, SADB_AALG_SHA1HMAC,
    SADB_EALG_3DESCBC, SADB_EALG_NULL, SADB_EXT_IDENTITY_DST, SADB_EXT_IDENTITY_SRC,
    SADB_IDENTTYPE_ASN1_DN, SADB_IDENTTYPE_FQDN, SADB_IDENTTYPE_PREFIX, SADB_IDENTTYPE_USERFQDN,
    SADB_SAFLAGS_PFS, SADB_SASTATE_LARVAL, SADB_SASTATE_MATURE, SADB_X_AALG_AES128GMAC,
    SADB_X_AALG_AES192GMAC, SADB_X_AALG_AES256GMAC, SADB_X_AALG_CHACHA20POLY1305,
    SADB_X_AALG_RIPEMD160HMAC, SADB_X_AALG_SHA2_256, SADB_X_AALG_SHA2_384, SADB_X_AALG_SHA2_512,
    SADB_X_CALG_DEFLATE, SADB_X_EALG_AES, SADB_X_EALG_AESCTR, SADB_X_EALG_AESGCM16,
    SADB_X_EALG_AESGMAC, SADB_X_EALG_BLF, SADB_X_EALG_CAST, SADB_X_EALG_CHACHA20POLY1305,
    SADB_X_EXT_DST_FLOW, SADB_X_EXT_DST_MASK, SADB_X_EXT_FLOW_TYPE, SADB_X_EXT_PROTOCOL,
    SADB_X_EXT_SRC_FLOW, SADB_X_EXT_SRC_MASK, SADB_X_FLOW_TYPE_ACQUIRE, SADB_X_FLOW_TYPE_BYPASS,
    SADB_X_FLOW_TYPE_DENY, SADB_X_FLOW_TYPE_DONTACQ, SADB_X_FLOW_TYPE_REQUIRE,
    SADB_X_FLOW_TYPE_USE, SADB_X_SAFLAGS_ESN, SADB_X_SAFLAGS_TUNNEL, SADB_X_SAFLAGS_UDPENCAP,
    SadbAddress, SadbHeaders, SadbIdent, SadbKey, SadbLifetime, SadbProtocol, SadbSa, SadbXCounter,
    SadbXIface, SadbXMtu, SadbXRdomain, SadbXReplay, SadbXTag, SadbXTap, SadbXUdpencap, extlen,
    padup, sadb_get, sadb_put,
};
use crate::net::pfvar::PF_TAG_NAME_SIZE;
use crate::net::route::rt_maskedcopy;
use crate::netinet::in_::{IPPROTO_IPCOMP, SockaddrIn};
#[cfg(feature = "inet6")]
use crate::netinet::ip_ipsp::SENT_IP6;
use crate::netinet::ip_ipsp::{
    IPSP_DENY, IPSP_IDENTITY_ASN1_DN, IPSP_IDENTITY_FQDN, IPSP_IDENTITY_PREFIX,
    IPSP_IDENTITY_USERFQDN, IPSP_IPSEC_ACQUIRE, IPSP_IPSEC_DONTACQ, IPSP_IPSEC_REQUIRE,
    IPSP_IPSEC_USE, IPSP_PERMIT, IpsecId, IpsecIds, IpsecInit, SENT_IP4, SENT_LEN, SockaddrEncap,
    SockaddrUnion, TDB_NCOUNTERS, TDBF_ALLOCATIONS, TDBF_BYTES, TDBF_ESN, TDBF_FIRSTUSE,
    TDBF_IFACE, TDBF_INVALID, TDBF_PFS, TDBF_SOFT_ALLOCATIONS, TDBF_SOFT_BYTES, TDBF_SOFT_FIRSTUSE,
    TDBF_SOFT_TIMER, TDBF_TIMER, TDBF_TUNNELING, TDBF_UDPENCAP, Tdb, TdbCounters, ipsp_ids_insert,
};
#[cfg(feature = "inet6")]
use crate::netinet6::in6::SockaddrIn6;
#[cfg(feature = "inet6")]
use crate::netinet6::in6_src::in6_embedscope;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_CREDENTIALS, M_WAITOK};
#[cfg(feature = "inet6")]
use crate::sys::socket::AF_INET6;
use crate::sys::socket::{AF_INET, PF_KEY, Sockaddr};

/// `import_sa`: (partly) initialize a TDB based on an `SADB_SA` payload. Other parts of the
/// TDB will be initialized by other import routines, and `tdb_init()`.
pub fn import_sa(tdb: &Tdb, sadb_sa: Option<SadbSa>, ii: Option<&mut IpsecInit<'_>>) {
    let Some(sadb_sa) = sadb_sa else {
        return;
    };

    mtx_enter(&tdb.tdb_mtx);
    if let Some(ii) = ii {
        ii.ii_encalg = sadb_sa.sadb_sa_encrypt;
        ii.ii_authalg = sadb_sa.sadb_sa_auth;
        ii.ii_compalg = sadb_sa.sadb_sa_encrypt; // Yeurk!

        tdb.tdb_spi.set(sadb_sa.sadb_sa_spi);
        tdb.tdb_wnd.set(sadb_sa.sadb_sa_replay);

        if sadb_sa.sadb_sa_flags & SADB_SAFLAGS_PFS != 0 {
            tdb.set_flags(TDBF_PFS);
        }

        if sadb_sa.sadb_sa_flags & SADB_X_SAFLAGS_TUNNEL != 0 {
            tdb.set_flags(TDBF_TUNNELING);
        }

        if sadb_sa.sadb_sa_flags & SADB_X_SAFLAGS_UDPENCAP != 0 {
            tdb.set_flags(TDBF_UDPENCAP);
        }

        if sadb_sa.sadb_sa_flags & SADB_X_SAFLAGS_ESN != 0 {
            tdb.set_flags(TDBF_ESN);
        }
    }

    if sadb_sa.sadb_sa_state != SADB_SASTATE_MATURE {
        tdb.set_flags(TDBF_INVALID);
    }
    mtx_leave(&tdb.tdb_mtx);
}

/// `export_sa`: export some of the information on a TDB.
///
/// # Safety
///
/// `*p` points at `size_of::<SadbSa>()` writable (zeroed) bytes; it is advanced past them.
pub unsafe fn export_sa(p: &mut *mut u8, tdb: &Tdb) {
    let mut sadb_sa = SadbSa {
        sadb_sa_len: (size_of::<SadbSa>() / size_of::<u64>()) as u16,
        sadb_sa_spi: tdb.tdb_spi.get(),
        sadb_sa_replay: tdb.tdb_wnd.get(),
        sadb_sa_state: if tdb.has_flags(TDBF_INVALID) {
            SADB_SASTATE_LARVAL
        } else {
            SADB_SASTATE_MATURE
        },
        ..SadbSa::default()
    };

    if i32::from(tdb.tdb_sproto.get()) == IPPROTO_IPCOMP
        && let Some(comp) = tdb.tdb_compalgxform.get()
        && comp.type_ == CRYPTO_DEFLATE_COMP
    {
        sadb_sa.sadb_sa_encrypt = SADB_X_CALG_DEFLATE;
    }

    if let Some(auth) = tdb.tdb_authalgxform.get() {
        match auth.type_ {
            CRYPTO_MD5_HMAC => sadb_sa.sadb_sa_auth = SADB_AALG_MD5HMAC,
            CRYPTO_SHA1_HMAC => sadb_sa.sadb_sa_auth = SADB_AALG_SHA1HMAC,
            CRYPTO_RIPEMD160_HMAC => sadb_sa.sadb_sa_auth = SADB_X_AALG_RIPEMD160HMAC,
            CRYPTO_SHA2_256_HMAC => sadb_sa.sadb_sa_auth = SADB_X_AALG_SHA2_256,
            CRYPTO_SHA2_384_HMAC => sadb_sa.sadb_sa_auth = SADB_X_AALG_SHA2_384,
            CRYPTO_SHA2_512_HMAC => sadb_sa.sadb_sa_auth = SADB_X_AALG_SHA2_512,
            CRYPTO_AES_128_GMAC => sadb_sa.sadb_sa_auth = SADB_X_AALG_AES128GMAC,
            CRYPTO_AES_192_GMAC => sadb_sa.sadb_sa_auth = SADB_X_AALG_AES192GMAC,
            CRYPTO_AES_256_GMAC => sadb_sa.sadb_sa_auth = SADB_X_AALG_AES256GMAC,
            CRYPTO_CHACHA20_POLY1305_MAC => {
                sadb_sa.sadb_sa_auth = SADB_X_AALG_CHACHA20POLY1305;
            }
            _ => {}
        }
    }

    if let Some(enc) = tdb.tdb_encalgxform.get() {
        match enc.type_ {
            CRYPTO_NULL => sadb_sa.sadb_sa_encrypt = SADB_EALG_NULL,
            CRYPTO_3DES_CBC => sadb_sa.sadb_sa_encrypt = SADB_EALG_3DESCBC,
            CRYPTO_AES_CBC => sadb_sa.sadb_sa_encrypt = SADB_X_EALG_AES,
            CRYPTO_AES_CTR => sadb_sa.sadb_sa_encrypt = SADB_X_EALG_AESCTR,
            CRYPTO_AES_GCM_16 => sadb_sa.sadb_sa_encrypt = SADB_X_EALG_AESGCM16,
            CRYPTO_AES_GMAC => sadb_sa.sadb_sa_encrypt = SADB_X_EALG_AESGMAC,
            CRYPTO_CAST_CBC => sadb_sa.sadb_sa_encrypt = SADB_X_EALG_CAST,
            CRYPTO_BLF_CBC => sadb_sa.sadb_sa_encrypt = SADB_X_EALG_BLF,
            CRYPTO_CHACHA20_POLY1305 => {
                sadb_sa.sadb_sa_encrypt = SADB_X_EALG_CHACHA20POLY1305;
            }
            _ => {}
        }
    }

    if tdb.has_flags(TDBF_PFS) {
        sadb_sa.sadb_sa_flags |= SADB_SAFLAGS_PFS;
    }

    if tdb.has_flags(TDBF_TUNNELING) {
        sadb_sa.sadb_sa_flags |= SADB_X_SAFLAGS_TUNNEL;
    }

    if tdb.has_flags(TDBF_UDPENCAP) {
        sadb_sa.sadb_sa_flags |= SADB_X_SAFLAGS_UDPENCAP;
    }

    if tdb.has_flags(TDBF_ESN) {
        sadb_sa.sadb_sa_flags |= SADB_X_SAFLAGS_ESN;
    }

    // SAFETY: the caller's contract.
    unsafe {
        sadb_put(*p, sadb_sa);
        *p = p.add(size_of::<SadbSa>());
    }
}

/// `import_lifetime`: initialize expirations and counters based on lifetime payload.
pub fn import_lifetime(tdb: &Tdb, sadb_lifetime: Option<SadbLifetime>, type_: i32) {
    let Some(lt) = sadb_lifetime else {
        return;
    };

    // Sets the bound `v` with its flag, or clears the flag for 0.
    let bound = |field: &core::cell::Cell<u64>, v: u64, flag: u32| {
        field.set(v);
        if v != 0 {
            tdb.set_flags(flag);
        } else {
            tdb.clr_flags(flag);
        }
    };

    mtx_enter(&tdb.tdb_mtx);
    match type_ {
        PFKEYV2_LIFETIME_HARD => {
            tdb.tdb_exp_allocations.set(lt.sadb_lifetime_allocations);
            if lt.sadb_lifetime_allocations != 0 {
                tdb.set_flags(TDBF_ALLOCATIONS);
            } else {
                tdb.clr_flags(TDBF_ALLOCATIONS);
            }

            bound(&tdb.tdb_exp_bytes, lt.sadb_lifetime_bytes, TDBF_BYTES);
            bound(&tdb.tdb_exp_timeout, lt.sadb_lifetime_addtime, TDBF_TIMER);
            bound(
                &tdb.tdb_exp_first_use,
                lt.sadb_lifetime_usetime,
                TDBF_FIRSTUSE,
            );
        }

        PFKEYV2_LIFETIME_SOFT => {
            tdb.tdb_soft_allocations.set(lt.sadb_lifetime_allocations);
            if lt.sadb_lifetime_allocations != 0 {
                tdb.set_flags(TDBF_SOFT_ALLOCATIONS);
            } else {
                tdb.clr_flags(TDBF_SOFT_ALLOCATIONS);
            }

            bound(&tdb.tdb_soft_bytes, lt.sadb_lifetime_bytes, TDBF_SOFT_BYTES);
            bound(
                &tdb.tdb_soft_timeout,
                lt.sadb_lifetime_addtime,
                TDBF_SOFT_TIMER,
            );
            bound(
                &tdb.tdb_soft_first_use,
                lt.sadb_lifetime_usetime,
                TDBF_SOFT_FIRSTUSE,
            );
        }

        PFKEYV2_LIFETIME_CURRENT => {
            // Nothing fancy here.
            tdb.tdb_cur_allocations.set(lt.sadb_lifetime_allocations);
            tdb.tdb_cur_bytes.set(lt.sadb_lifetime_bytes);
            tdb.tdb_established.set(lt.sadb_lifetime_addtime);
            tdb.tdb_first_use.set(lt.sadb_lifetime_usetime);
        }
        _ => {}
    }
    mtx_leave(&tdb.tdb_mtx);
}

/// `export_lifetime`: export TDB expiration information.
///
/// # Safety
///
/// `*p` points at `size_of::<SadbLifetime>()` writable (zeroed) bytes; it is advanced past
/// them.
pub unsafe fn export_lifetime(p: &mut *mut u8, tdb: &Tdb, type_: i32) {
    let mut lt = SadbLifetime {
        sadb_lifetime_len: (size_of::<SadbLifetime>() / size_of::<u64>()) as u16,
        ..SadbLifetime::default()
    };

    match type_ {
        PFKEYV2_LIFETIME_HARD => {
            if tdb.has_flags(TDBF_ALLOCATIONS) {
                lt.sadb_lifetime_allocations = tdb.tdb_exp_allocations.get();
            }

            if tdb.has_flags(TDBF_BYTES) {
                lt.sadb_lifetime_bytes = tdb.tdb_exp_bytes.get();
            }

            if tdb.has_flags(TDBF_TIMER) {
                lt.sadb_lifetime_addtime = tdb.tdb_exp_timeout.get();
            }

            if tdb.has_flags(TDBF_FIRSTUSE) {
                lt.sadb_lifetime_usetime = tdb.tdb_exp_first_use.get();
            }
        }

        PFKEYV2_LIFETIME_SOFT => {
            if tdb.has_flags(TDBF_SOFT_ALLOCATIONS) {
                lt.sadb_lifetime_allocations = tdb.tdb_soft_allocations.get();
            }

            if tdb.has_flags(TDBF_SOFT_BYTES) {
                lt.sadb_lifetime_bytes = tdb.tdb_soft_bytes.get();
            }

            if tdb.has_flags(TDBF_SOFT_TIMER) {
                lt.sadb_lifetime_addtime = tdb.tdb_soft_timeout.get();
            }

            if tdb.has_flags(TDBF_SOFT_FIRSTUSE) {
                lt.sadb_lifetime_usetime = tdb.tdb_soft_first_use.get();
            }
        }

        PFKEYV2_LIFETIME_CURRENT => {
            lt.sadb_lifetime_allocations = tdb.tdb_cur_allocations.get();
            lt.sadb_lifetime_bytes = tdb.tdb_cur_bytes.get();
            lt.sadb_lifetime_addtime = tdb.tdb_established.get();
            lt.sadb_lifetime_usetime = tdb.tdb_first_use.get();
        }

        PFKEYV2_LIFETIME_LASTUSE => {
            lt.sadb_lifetime_allocations = 0;
            lt.sadb_lifetime_bytes = 0;
            lt.sadb_lifetime_addtime = 0;
            lt.sadb_lifetime_usetime = tdb.tdb_last_used.get();
        }
        _ => {}
    }

    // SAFETY: the caller's contract.
    unsafe {
        sadb_put(*p, lt);
        *p = p.add(size_of::<SadbLifetime>());
    }
}

/// `import_flow`: import flow information to two `struct sockaddr_encap`s. Either all or none
/// of the address arguments are NULL.
///
/// # Safety
///
/// The address pointers are null together, or each points at a whole address extension that
/// `pfkeyv2_parsemessage` checked (its socket address is masked in place).
#[allow(clippy::too_many_arguments)] // the C's prototype
pub unsafe fn import_flow(
    flow: &mut SockaddrEncap,
    flowmask: &mut SockaddrEncap,
    ssrc: *mut u8,
    ssrcmask: *mut u8,
    ddst: *mut u8,
    ddstmask: *mut u8,
    sab: Option<SadbProtocol>,
    ftype: Option<SadbProtocol>,
) -> Result<(), Errno> {
    if ssrc.is_null() {
        return Ok(()); // There wasn't any information to begin with.
    }

    // SAFETY: (for the function) the caller's contract: four whole address extensions.
    let sa_of = |ext: *mut u8| unsafe { ext.add(size_of::<SadbAddress>()).cast::<Sockaddr>() };
    let src = sa_of(ssrc);
    let dst = sa_of(ddst);
    let srcmask = sa_of(ssrcmask);
    let dstmask = sa_of(ddstmask);

    *flow = SockaddrEncap::new();
    *flowmask = SockaddrEncap::new();

    let transproto = sab.map_or(0, |s| s.sadb_protocol_proto);

    // Check that all the address families match. We know they are valid and supported
    // because pfkeyv2_parsemessage() checked that.
    // SAFETY: as above; `sa_family` is a byte at offset 1.
    let family = |sa: *mut Sockaddr| unsafe { (*sa).sa_family };
    if family(src) != family(dst)
        || family(src) != family(srcmask)
        || family(src) != family(dstmask)
    {
        return Err(Errno::EINVAL);
    }

    // We set these as an indication that tdb_filter/tdb_filtermask are in fact initialized.
    flow.set_sen_family(PF_KEY);
    flowmask.set_sen_family(PF_KEY);
    flow.set_sen_len(SENT_LEN as u8);
    flowmask.set_sen_len(SENT_LEN as u8);

    if family(src) == AF_INET {
        {
            // netmask handling
            // SAFETY: the addresses are `sockaddr_in`s inside their extensions; masking a
            // socket address onto itself is what the C does.
            unsafe {
                rt_maskedcopy(src, src, srcmask);
                rt_maskedcopy(dst, dst, dstmask);
            }

            // SAFETY: `sockaddr_in`s, checked by `pfkeyv2_parsemessage`.
            let (s, d, sm, dm): (SockaddrIn, SockaddrIn, SockaddrIn, SockaddrIn) = unsafe {
                (
                    sadb_get(src.cast()),
                    sadb_get(dst.cast()),
                    sadb_get(srcmask.cast()),
                    sadb_get(dstmask.cast()),
                )
            };

            flow.set_sen_type(SENT_IP4);
            flow.set_sen_direction(ftype.map_or(0, |f| f.sadb_protocol_direction));
            flow.set_sen_ip_src(s.sin_addr);
            flow.set_sen_ip_dst(d.sin_addr);
            flow.set_sen_proto(transproto);
            flow.set_sen_sport(s.sin_port);
            flow.set_sen_dport(d.sin_port);

            flowmask.set_sen_type(SENT_IP4);
            flowmask.set_sen_direction(0xff);
            flowmask.set_sen_ip_src(sm.sin_addr);
            flowmask.set_sen_ip_dst(dm.sin_addr);
            flowmask.set_sen_sport(sm.sin_port);
            flowmask.set_sen_dport(dm.sin_port);
            if transproto != 0 {
                flowmask.set_sen_proto(0xff);
            }
        }
    }
    #[cfg(feature = "inet6")]
    if family(src) == AF_INET6 {
        // SAFETY: `sockaddr_in6`s, checked by `pfkeyv2_parsemessage`; the message copies are
        // written back (the C embeds the scope in place).
        let (mut s, mut d, sm, dm): (SockaddrIn6, SockaddrIn6, SockaddrIn6, SockaddrIn6) = unsafe {
            (
                sadb_get(src.cast()),
                sadb_get(dst.cast()),
                sadb_get(srcmask.cast()),
                sadb_get(dstmask.cast()),
            )
        };
        let sin6 = s;
        let _ = in6_embedscope(&mut s.sin6_addr, &sin6, None, None);
        let din6 = d;
        let _ = in6_embedscope(&mut d.sin6_addr, &din6, None, None);
        // SAFETY: as above.
        unsafe {
            sadb_put(src.cast(), s);
            sadb_put(dst.cast(), d);

            // netmask handling
            rt_maskedcopy(src, src, srcmask);
            rt_maskedcopy(dst, dst, dstmask);

            s = sadb_get(src.cast());
            d = sadb_get(dst.cast());
        }

        flow.set_sen_type(SENT_IP6);
        flow.set_sen_ip6_direction(ftype.map_or(0, |f| f.sadb_protocol_direction));
        flow.set_sen_ip6_src(s.sin6_addr);
        flow.set_sen_ip6_dst(d.sin6_addr);
        flow.set_sen_ip6_proto(transproto);
        flow.set_sen_ip6_sport(s.sin6_port);
        flow.set_sen_ip6_dport(d.sin6_port);

        flowmask.set_sen_type(SENT_IP6);
        flowmask.set_sen_ip6_direction(0xff);
        flowmask.set_sen_ip6_src(sm.sin6_addr);
        flowmask.set_sen_ip6_dst(dm.sin6_addr);
        flowmask.set_sen_ip6_sport(sm.sin6_port);
        flowmask.set_sen_ip6_dport(dm.sin6_port);
        if transproto != 0 {
            flowmask.set_sen_ip6_proto(0xff);
        }
    }

    Ok(())
}

/// `export_encap`: helper to export addresses from a `struct sockaddr_encap`.
///
/// # Safety
///
/// As for [`export_address`], for a `sockaddr_in` (`sockaddr_in6`).
unsafe fn export_encap(p: &mut *mut u8, encap: &SockaddrEncap, type_: u16) {
    let saddr = *p;
    // SAFETY: (for the function) the caller's contract.
    unsafe {
        *p = p.add(size_of::<SadbAddress>());
    }

    match encap.sen_type() {
        SENT_IP4 => {
            let src = type_ == SADB_X_EXT_SRC_FLOW || type_ == SADB_X_EXT_SRC_MASK;
            let sin = SockaddrIn {
                sin_len: size_of::<SockaddrIn>() as u8,
                sin_family: AF_INET,
                sin_addr: if src {
                    encap.sen_ip_src()
                } else {
                    encap.sen_ip_dst()
                },
                sin_port: if src {
                    encap.sen_sport()
                } else {
                    encap.sen_dport()
                },
                ..SockaddrIn::default()
            };
            // SAFETY: the caller's contract.
            unsafe {
                sadb_put(
                    saddr,
                    SadbAddress {
                        sadb_address_len: ((size_of::<SadbAddress>()
                            + padup(size_of::<SockaddrIn>()))
                            / size_of::<u64>()) as u16,
                        ..SadbAddress::default()
                    },
                );
                sadb_put(*p, sin);
                *p = p.add(padup(size_of::<SockaddrIn>()));
            }
        }
        #[cfg(feature = "inet6")]
        SENT_IP6 => {
            let src = type_ == SADB_X_EXT_SRC_FLOW || type_ == SADB_X_EXT_SRC_MASK;
            let sin6 = SockaddrIn6 {
                sin6_len: size_of::<SockaddrIn6>() as u8,
                sin6_family: AF_INET6,
                sin6_addr: if src {
                    encap.sen_ip6_src()
                } else {
                    encap.sen_ip6_dst()
                },
                sin6_port: if src {
                    encap.sen_ip6_sport()
                } else {
                    encap.sen_ip6_dport()
                },
                ..SockaddrIn6::default()
            };
            // SAFETY: the caller's contract.
            unsafe {
                sadb_put(
                    saddr,
                    SadbAddress {
                        sadb_address_len: ((size_of::<SadbAddress>()
                            + padup(size_of::<SockaddrIn6>()))
                            / size_of::<u64>()) as u16,
                        ..SadbAddress::default()
                    },
                );
                sadb_put(*p, sin6);
                *p = p.add(padup(size_of::<SockaddrIn6>()));
            }
        }
        _ => {}
    }
}

/// `export_flow`: export flow information from two `struct sockaddr_encap`s.
///
/// # Safety
///
/// `*p` points at enough writable (zeroed) bytes for two `sadb_protocol`s and four address
/// extensions; the headers are set to them.
pub unsafe fn export_flow(
    p: &mut *mut u8,
    ftype: u8,
    flow: &SockaddrEncap,
    flowmask: &SockaddrEncap,
    headers: &mut SadbHeaders,
) {
    headers[usize::from(SADB_X_EXT_FLOW_TYPE)] = *p;
    let mut sab = SadbProtocol {
        sadb_protocol_len: (size_of::<SadbProtocol>() / size_of::<u64>()) as u16,
        ..SadbProtocol::default()
    };

    sab.sadb_protocol_proto = match ftype {
        IPSP_IPSEC_USE => SADB_X_FLOW_TYPE_USE,
        IPSP_IPSEC_ACQUIRE => SADB_X_FLOW_TYPE_ACQUIRE,
        IPSP_IPSEC_REQUIRE => SADB_X_FLOW_TYPE_REQUIRE,
        IPSP_DENY => SADB_X_FLOW_TYPE_DENY,
        IPSP_PERMIT => SADB_X_FLOW_TYPE_BYPASS,
        IPSP_IPSEC_DONTACQ => SADB_X_FLOW_TYPE_DONTACQ,
        _ => 0,
    };

    match flow.sen_type() {
        SENT_IP4 => sab.sadb_protocol_direction = flow.sen_direction(),
        #[cfg(feature = "inet6")]
        SENT_IP6 => sab.sadb_protocol_direction = flow.sen_ip6_direction(),
        _ => {}
    }

    // SAFETY: (for the function) the caller's contract.
    unsafe {
        sadb_put(*p, sab);
        *p = p.add(size_of::<SadbProtocol>());

        headers[usize::from(SADB_X_EXT_PROTOCOL)] = *p;
        let mut sab = SadbProtocol {
            sadb_protocol_len: (size_of::<SadbProtocol>() / size_of::<u64>()) as u16,
            ..SadbProtocol::default()
        };
        match flow.sen_type() {
            SENT_IP4 => sab.sadb_protocol_proto = flow.sen_proto(),
            #[cfg(feature = "inet6")]
            SENT_IP6 => sab.sadb_protocol_proto = flow.sen_ip6_proto(),
            _ => {}
        }
        sadb_put(*p, sab);
        *p = p.add(size_of::<SadbProtocol>());

        headers[usize::from(SADB_X_EXT_SRC_FLOW)] = *p;
        export_encap(p, flow, SADB_X_EXT_SRC_FLOW);

        headers[usize::from(SADB_X_EXT_SRC_MASK)] = *p;
        export_encap(p, flowmask, SADB_X_EXT_SRC_MASK);

        headers[usize::from(SADB_X_EXT_DST_FLOW)] = *p;
        export_encap(p, flow, SADB_X_EXT_DST_FLOW);

        headers[usize::from(SADB_X_EXT_DST_MASK)] = *p;
        export_encap(p, flowmask, SADB_X_EXT_DST_MASK);
    }
}

/// `import_address`: copy an `SADB_ADDRESS` payload to a socket address (nothing for NULL).
///
/// # Safety
///
/// `sadb_address` is null or points at a whole address extension.
pub unsafe fn import_address(sa: &mut SockaddrUnion, sadb_address: *const u8) {
    if sadb_address.is_null() {
        return;
    }
    // SAFETY: (for the function) the caller's contract.
    let ssa = unsafe { sadb_address.add(size_of::<SadbAddress>()) };
    // SAFETY: as above: the extension holds at least a `struct sockaddr`.
    let (ssa_len, ssa_family) = unsafe { (*ssa, *ssa.add(1)) };

    let salen = if ssa_len != 0 {
        usize::from(ssa_len)
    } else {
        match ssa_family {
            AF_INET => size_of::<SockaddrIn>(),
            #[cfg(feature = "inet6")]
            AF_INET6 => size_of::<SockaddrIn6>(),
            _ => return,
        }
    };

    // SAFETY: as above; the extension holds the address (`pfkeyv2_parsemessage` checked its
    // length against `sa_len`).
    let avail = unsafe { extlen(sadb_address) }.saturating_sub(size_of::<SadbAddress>());
    let n = salen.min(avail).min(sa.as_bytes().len());
    // SAFETY: `n` bytes of the extension are readable and fit in the union.
    unsafe { ptr::copy_nonoverlapping(ssa, sa.as_bytes_mut().as_mut_ptr(), n) };
    sa.set_sa_len(salen as u8);
}

/// `export_address`: export a `struct sockaddr` as an `SADB_ADDRESS` payload.
///
/// # Safety
///
/// `sa` points at a socket address of its `sa_len`; `*p` points at
/// `size_of::<SadbAddress>() + PADUP(sa_len)` writable (zeroed) bytes; it is advanced past
/// them.
pub unsafe fn export_address(p: &mut *mut u8, sa: *const Sockaddr) {
    // SAFETY: (for the function) the caller's contract.
    unsafe {
        let sa_len = usize::from((*sa).sa_len);
        sadb_put(
            *p,
            SadbAddress {
                sadb_address_len: ((size_of::<SadbAddress>() + padup(sa_len)) / size_of::<u64>())
                    as u16,
                ..SadbAddress::default()
            },
        );

        *p = p.add(size_of::<SadbAddress>());
        ptr::copy_nonoverlapping(sa.cast::<u8>(), *p, sa_len);
        *p.add(1) = (*sa).sa_family;
        *p = p.add(padup(sa_len));
    }
}

/// `import_identity`: import an identity payload into the TDB (NULL for none or an unknown
/// type).
///
/// # Safety
///
/// `sadb_ident` is null or points at a whole identity extension.
unsafe fn import_identity(sadb_ident: *const u8) -> Option<NonNull<IpsecId>> {
    if sadb_ident.is_null() {
        return None;
    }

    // SAFETY: (for the function) the caller's contract.
    let (ident, id_len) = unsafe {
        (
            sadb_get::<SadbIdent>(sadb_ident),
            extlen(sadb_ident) - size_of::<SadbIdent>(),
        )
    };
    let id_sz = size_of::<IpsecId>() + id_len;
    let Some(mem) = malloc(id_sz, M_CREDENTIALS, M_WAITOK) else {
        panic(format_args!("import_identity: malloc(M_WAITOK) failed"));
    };
    let id = mem.cast::<IpsecId>();

    let type_ = match ident.sadb_ident_type {
        SADB_IDENTTYPE_PREFIX => IPSP_IDENTITY_PREFIX,
        SADB_IDENTTYPE_FQDN => IPSP_IDENTITY_FQDN,
        SADB_IDENTTYPE_USERFQDN => IPSP_IDENTITY_USERFQDN,
        SADB_IDENTTYPE_ASN1_DN => IPSP_IDENTITY_ASN1_DN,
        _ => {
            free(mem, M_CREDENTIALS, id_sz);
            return None;
        }
    };
    // SAFETY: a fresh allocation of the header and `id_len` bytes, written before use; the
    // identity's data comes from the extension's `id_len` bytes after its header.
    unsafe {
        id.as_ptr().write(IpsecId {
            type_,
            len: id_len as i16,
        });
        ptr::copy_nonoverlapping(
            sadb_ident.add(size_of::<SadbIdent>()),
            id.as_ptr().add(1).cast::<u8>(),
            id_len,
        );
    }
    Some(id)
}

/// The size `import_identity` allocated `id` with.
fn id_size(id: NonNull<IpsecId>) -> usize {
    // SAFETY: an identity `import_identity` made.
    size_of::<IpsecId>() + usize::try_from(unsafe { id.as_ref() }.len).unwrap_or(0)
}

/// `import_identities`: the shared pair of identities of `srcid`/`dstid` (local and remote,
/// or the other way round with `swapped`), `None` without both or when no flow number is
/// left.
///
/// # Safety
///
/// `srcid` and `dstid` are null or point at whole identity extensions.
pub unsafe fn import_identities(
    swapped: bool,
    srcid: *const u8,
    dstid: *const u8,
) -> Option<&'static IpsecIds> {
    let (l, r) = if swapped {
        (dstid, srcid)
    } else {
        (srcid, dstid)
    };
    // SAFETY: the caller's contract.
    let id_local = unsafe { import_identity(l) };
    // SAFETY: the caller's contract.
    let id_remote = unsafe { import_identity(r) };

    if let (Some(local), Some(remote)) = (id_local, id_remote) {
        let Some(mem) = malloc(size_of::<IpsecIds>(), M_CREDENTIALS, M_WAITOK) else {
            panic(format_args!("import_identities: malloc(M_WAITOK) failed"));
        };
        let tmp = mem.cast::<IpsecIds>().as_ptr();
        // SAFETY: a fresh allocation of an `IpsecIds`, written before use; it lives until the
        // identity garbage collector frees it (or right below).
        unsafe { tmp.write(IpsecIds::new(local, remote)) };
        // SAFETY: as above.
        let tmp: &'static IpsecIds = unsafe { &*tmp };
        let ids = ipsp_ids_insert(tmp);
        if ids.is_some_and(|i| ptr::eq(i, tmp)) {
            return ids;
        }
        free(local.cast(), M_CREDENTIALS, id_size(local));
        free(remote.cast(), M_CREDENTIALS, id_size(remote));
        free(mem, M_CREDENTIALS, size_of::<IpsecIds>());
        return ids;
    }
    if let Some(local) = id_local {
        free(local.cast(), M_CREDENTIALS, id_size(local));
    }
    if let Some(remote) = id_remote {
        free(remote.cast(), M_CREDENTIALS, id_size(remote));
    }
    None
}

/// `export_identity`.
///
/// # Safety
///
/// `*p` points at `size_of::<SadbIdent>() + PADUP(id->len)` writable (zeroed) bytes; it is
/// advanced past them.
unsafe fn export_identity(p: &mut *mut u8, id: &IpsecId) {
    let data = id.data();
    let mut sadb_ident = SadbIdent {
        sadb_ident_len: ((size_of::<SadbIdent>() + padup(data.len())) / size_of::<u64>()) as u16,
        ..SadbIdent::default()
    };

    match id.type_ {
        IPSP_IDENTITY_PREFIX => sadb_ident.sadb_ident_type = SADB_IDENTTYPE_PREFIX,
        IPSP_IDENTITY_FQDN => sadb_ident.sadb_ident_type = SADB_IDENTTYPE_FQDN,
        IPSP_IDENTITY_USERFQDN => sadb_ident.sadb_ident_type = SADB_IDENTTYPE_USERFQDN,
        IPSP_IDENTITY_ASN1_DN => sadb_ident.sadb_ident_type = SADB_IDENTTYPE_ASN1_DN,
        _ => {}
    }
    // SAFETY: the caller's contract.
    unsafe {
        sadb_put(*p, sadb_ident);
        *p = p.add(size_of::<SadbIdent>());
        ptr::copy_nonoverlapping(data.as_ptr(), *p, data.len());
        *p = p.add(padup(data.len()));
    }
}

/// `export_identities`.
///
/// # Safety
///
/// `*p` points at enough writable (zeroed) bytes for both identities; the headers are set to
/// them.
pub unsafe fn export_identities(
    p: &mut *mut u8,
    ids: &IpsecIds,
    swapped: bool,
    headers: &mut SadbHeaders,
) {
    headers[usize::from(SADB_EXT_IDENTITY_SRC)] = *p;
    // SAFETY: the caller's contract.
    unsafe {
        export_identity(
            p,
            if swapped {
                ids.id_remote()
            } else {
                ids.id_local()
            },
        );
    }
    headers[usize::from(SADB_EXT_IDENTITY_DST)] = *p;
    // SAFETY: the caller's contract.
    unsafe {
        export_identity(
            p,
            if swapped {
                ids.id_local()
            } else {
                ids.id_remote()
            },
        );
    }
}

/// `import_key`: the key of a `SADB_KEY` payload, borrowed from the message.
///
/// # Safety
///
/// `sadb_key` is null or points at a whole key extension that lives as long as `'a`.
pub unsafe fn import_key<'a>(ii: &mut IpsecInit<'a>, sadb_key: *const u8, type_: i32) {
    if sadb_key.is_null() {
        return;
    }

    // SAFETY: (for the function) the caller's contract.
    let (k, avail) = unsafe {
        (
            sadb_get::<SadbKey>(sadb_key),
            extlen(sadb_key) - size_of::<SadbKey>(),
        )
    };
    let keylen = k.sadb_key_bits / 8;
    // SAFETY: the key bytes after the header, at most the extension's.
    let key: &'a [u8] = unsafe {
        core::slice::from_raw_parts(
            sadb_key.add(size_of::<SadbKey>()),
            usize::from(keylen).min(avail),
        )
    };

    if type_ == PFKEYV2_ENCRYPTION_KEY {
        // Encryption key
        ii.ii_enckeylen = keylen;
        ii.ii_enckey = key;
    } else {
        ii.ii_authkeylen = keylen;
        ii.ii_authkey = key;
    }
}

/// `export_key`.
///
/// # Safety
///
/// `*p` points at `size_of::<SadbKey>() + PADUP(keylen)` writable (zeroed) bytes; it is
/// advanced past them.
pub unsafe fn export_key(p: &mut *mut u8, tdb: &Tdb, type_: i32) {
    // SAFETY: the caller holds the TDB; its keys live until it is freed.
    let key = unsafe {
        if type_ == PFKEYV2_ENCRYPTION_KEY {
            tdb.tdb_emxkey()
        } else {
            tdb.tdb_amxkey()
        }
    };
    let sadb_key = SadbKey {
        sadb_key_len: ((size_of::<SadbKey>() + padup(key.len())) / size_of::<u64>()) as u16,
        sadb_key_bits: (key.len() * 8) as u16,
        ..SadbKey::default()
    };
    // SAFETY: the caller's contract.
    unsafe {
        sadb_put(*p, sadb_key);
        *p = p.add(size_of::<SadbKey>());
        ptr::copy_nonoverlapping(key.as_ptr(), *p, key.len());
        *p = p.add(padup(key.len()));
    }
}

/// `import_udpencap`: import remote port for UDP Encapsulation.
pub fn import_udpencap(tdb: &Tdb, sadb_udpencap: Option<SadbXUdpencap>) {
    if let Some(u) = sadb_udpencap {
        tdb.tdb_udpencap_port.set(u.sadb_x_udpencap_port);
    }
}

/// Writes `v` at the cursor and advances it.
///
/// # Safety
///
/// `*p` points at `size_of::<T>()` writable bytes.
unsafe fn put_adv<T>(p: &mut *mut u8, v: T) {
    // SAFETY: the caller's contract.
    unsafe {
        sadb_put(*p, v);
        *p = p.add(size_of::<T>());
    }
}

/// `export_udpencap`: export remote port for UDP Encapsulation.
///
/// # Safety
///
/// `*p` points at `size_of::<SadbXUdpencap>()` writable bytes; it is advanced past them.
pub unsafe fn export_udpencap(p: &mut *mut u8, tdb: &Tdb) {
    let u = SadbXUdpencap {
        sadb_x_udpencap_port: tdb.tdb_udpencap_port.get(),
        sadb_x_udpencap_reserved: 0,
        sadb_x_udpencap_len: (size_of::<SadbXUdpencap>() / size_of::<u64>()) as u16,
        ..SadbXUdpencap::default()
    };
    // SAFETY: the caller's contract.
    unsafe { put_adv(p, u) };
}

/// `export_replay`: export PF replay for SA.
///
/// # Safety
///
/// `*p` points at `size_of::<SadbXReplay>()` writable (zeroed) bytes; it is advanced past
/// them.
pub unsafe fn export_replay(p: &mut *mut u8, tdb: &Tdb) {
    mtx_enter(&tdb.tdb_mtx);
    let count = tdb.tdb_rpl.get();
    mtx_leave(&tdb.tdb_mtx);
    let sreplay = SadbXReplay {
        sadb_x_replay_count: count,
        sadb_x_replay_len: (size_of::<SadbXReplay>() / size_of::<u64>()) as u16,
        ..SadbXReplay::default()
    };
    // SAFETY: the caller's contract.
    unsafe { put_adv(p, sreplay) };
}

/// `export_mtu`: export mtu for SA.
///
/// # Safety
///
/// `*p` points at `size_of::<SadbXMtu>()` writable (zeroed) bytes; it is advanced past them.
pub unsafe fn export_mtu(p: &mut *mut u8, tdb: &Tdb) {
    let smtu = SadbXMtu {
        sadb_x_mtu_mtu: tdb.tdb_mtu.get(),
        sadb_x_mtu_len: (size_of::<SadbXMtu>() / size_of::<u64>()) as u16,
        ..SadbXMtu::default()
    };
    // SAFETY: the caller's contract.
    unsafe { put_adv(p, smtu) };
}

/// `import_rdomain`: import rdomain switch for SA.
pub fn import_rdomain(tdb: &Tdb, srdomain: Option<SadbXRdomain>) {
    if let Some(r) = srdomain {
        tdb.tdb_rdomain_post.set(u32::from(r.sadb_x_rdomain_dom2));
    }
}

/// `export_rdomain`: export rdomain switch for SA.
///
/// # Safety
///
/// `*p` points at `size_of::<SadbXRdomain>()` writable (zeroed) bytes; it is advanced past
/// them.
pub unsafe fn export_rdomain(p: &mut *mut u8, tdb: &Tdb) {
    let srdomain = SadbXRdomain {
        sadb_x_rdomain_dom1: tdb.tdb_rdomain.get() as u16,
        sadb_x_rdomain_dom2: tdb.tdb_rdomain_post.get() as u16,
        sadb_x_rdomain_len: (size_of::<SadbXRdomain>() / size_of::<u64>()) as u16,
        ..SadbXRdomain::default()
    };
    // SAFETY: the caller's contract.
    unsafe { put_adv(p, srdomain) };
}

/// `import_tag`: import PF tag information for SA. `stag` is the extension (header and
/// name), NULL when the message has none.
///
/// # Safety
///
/// A non-null `stag` points at an `SADB_X_EXT_TAG` extension `pfkeyv2_parsemessage`
/// accepted (its length covers the header and at most `PF_TAG_NAME_SIZE` bytes of name).
pub unsafe fn import_tag(tdb: &Tdb, stag: *const u8) {
    if stag.is_null() {
        return;
    }
    // SAFETY: the caller's contract.
    let hdr: SadbXTag = unsafe { sadb_get(stag) };
    let len = (usize::from(hdr.sadb_x_tag_len) * size_of::<u64>())
        .saturating_sub(size_of::<SadbXTag>())
        .min(PF_TAG_NAME_SIZE);
    // SAFETY: the caller's contract: the name follows the header, `len` bytes of it.
    let s = unsafe { core::slice::from_raw_parts(stag.add(size_of::<SadbXTag>()), len) };
    tdb.tdb_tag.set(pf_tagname2tag(s, true));
}

/// `export_tag`: export PF tag information for SA.
///
/// # Safety
///
/// `*p` points at `size_of::<SadbXTag>() + PADUP(PF_TAG_NAME_SIZE)` writable (zeroed) bytes;
/// it is advanced past the ones used.
pub unsafe fn export_tag(p: &mut *mut u8, tdb: &Tdb) {
    let mut s = [0u8; PF_TAG_NAME_SIZE];
    pf_tag2tagname(tdb.tdb_tag.get(), &mut s);
    let taglen = s
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(PF_TAG_NAME_SIZE - 1)
        + 1;
    let stag = SadbXTag {
        sadb_x_tag_taglen: taglen as u32,
        sadb_x_tag_len: ((size_of::<SadbXTag>() + padup(taglen)) / size_of::<u64>()) as u16,
        ..SadbXTag::default()
    };
    // SAFETY: the caller's contract: the header, then the name and its NUL inside the
    // `PADUP(PF_TAG_NAME_SIZE)` bytes after it.
    unsafe {
        sadb_put(*p, stag);
        ptr::copy_nonoverlapping(s.as_ptr(), p.add(size_of::<SadbXTag>()), taglen);
        *p = p.add(size_of::<SadbXTag>() + padup(taglen));
    }
}

/// `import_tap`: import enc(4) tap device information for SA.
pub fn import_tap(tdb: &Tdb, stap: Option<SadbXTap>) {
    if let Some(s) = stap {
        tdb.tdb_tap.set(s.sadb_x_tap_unit);
    }
}

/// `export_tap`: export enc(4) tap device information for SA.
///
/// # Safety
///
/// `*p` points at `size_of::<SadbXTap>()` writable (zeroed) bytes; it is advanced past them.
pub unsafe fn export_tap(p: &mut *mut u8, tdb: &Tdb) {
    let stap = SadbXTap {
        sadb_x_tap_unit: tdb.tdb_tap.get(),
        sadb_x_tap_len: (size_of::<SadbXTap>() / size_of::<u64>()) as u16,
        ..SadbXTap::default()
    };
    // SAFETY: the caller's contract.
    unsafe { put_adv(p, stap) };
}

/// `import_iface`: import interface information for SA.
pub fn import_iface(tdb: &Tdb, siface: Option<SadbXIface>) {
    if let Some(s) = siface {
        tdb.set_flags(TDBF_IFACE);
        tdb.tdb_iface.set(s.sadb_x_iface_unit);
        tdb.tdb_iface_dir.set(s.sadb_x_iface_direction);
    }
}

/// `export_iface`: export interface information for SA.
///
/// # Safety
///
/// `*p` points at `size_of::<SadbXIface>()` writable (zeroed) bytes; it is advanced past them.
pub unsafe fn export_iface(p: &mut *mut u8, tdb: &Tdb) {
    let siface = SadbXIface {
        sadb_x_iface_len: (size_of::<SadbXIface>() / size_of::<u64>()) as u16,
        sadb_x_iface_unit: tdb.tdb_iface.get(),
        sadb_x_iface_direction: tdb.tdb_iface_dir.get(),
        ..SadbXIface::default()
    };
    // SAFETY: the caller's contract.
    unsafe { put_adv(p, siface) };
}

/// `export_satype`.
///
/// # Safety
///
/// `*p` points at `size_of::<SadbProtocol>()` writable (zeroed) bytes; it is advanced past
/// them.
pub unsafe fn export_satype(p: &mut *mut u8, tdb: &Tdb) {
    let sab = SadbProtocol {
        sadb_protocol_len: (size_of::<SadbProtocol>() / size_of::<u64>()) as u16,
        sadb_protocol_proto: tdb.tdb_satype.get(),
        ..SadbProtocol::default()
    };
    // SAFETY: the caller's contract.
    unsafe { put_adv(p, sab) };
}

/// `export_counter`.
///
/// # Safety
///
/// `*p` points at `size_of::<SadbXCounter>()` writable (zeroed) bytes; it is advanced past
/// them.
pub unsafe fn export_counter(p: &mut *mut u8, tdb: &Tdb) {
    let mut counters = [0u64; TDB_NCOUNTERS];
    for (c, a) in counters.iter_mut().zip(&tdb.tdb_counters) {
        *c = a.load(Ordering::Relaxed);
    }
    let at = |c: TdbCounters| counters[c as usize];

    let scnt = SadbXCounter {
        sadb_x_counter_len: (size_of::<SadbXCounter>() / size_of::<u64>()) as u16,
        sadb_x_counter_pad: 0,
        sadb_x_counter_ipackets: at(TdbCounters::TdbIpackets),
        sadb_x_counter_opackets: at(TdbCounters::TdbOpackets),
        sadb_x_counter_ibytes: at(TdbCounters::TdbIbytes),
        sadb_x_counter_obytes: at(TdbCounters::TdbObytes),
        sadb_x_counter_idrops: at(TdbCounters::TdbIdrops),
        sadb_x_counter_odrops: at(TdbCounters::TdbOdrops),
        sadb_x_counter_idecompbytes: at(TdbCounters::TdbIdecompbytes),
        sadb_x_counter_ouncompbytes: at(TdbCounters::TdbOuncompbytes),
        ..SadbXCounter::default()
    };
    // SAFETY: the caller's contract.
    unsafe { put_adv(p, scnt) };
}
/* </CODE> */
