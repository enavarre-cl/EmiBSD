/*	$OpenBSD: cryptosoft.h,v 1.16 2021/07/09 15:29:55 bluhm Exp $	*/
/*	$OpenBSD: cryptosoft.c,v 1.93 2026/07/13 16:04:23 bluhm Exp $	*/
/* <LICENSES> */
/*
 * The author of this code is Angelos D. Keromytis (angelos@cis.upenn.edu)
 *
 * This code was written by Angelos D. Keromytis in Athens, Greece, in
 * February 2000. Network Security Technologies Inc. (NSTI) kindly
 * supported the development of this code.
 *
 * Copyright (c) 2000 Angelos D. Keromytis
 *
 * Permission to use, copy, and modify this software with or without fee
 * is hereby granted, provided that this entire notice is included in
 * all source code copies of any software which is or includes a copy or
 * modification of this software.
 *
 * THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR
 * IMPLIED WARRANTY. IN PARTICULAR, NONE OF THE AUTHORS MAKES ANY
 * REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE
 * MERCHANTABILITY OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR
 * PURPOSE.
 */

/*
 * The author of this code is Angelos D. Keromytis (angelos@cis.upenn.edu)
 *
 * This code was written by Angelos D. Keromytis in Athens, Greece, in
 * February 2000. Network Security Technologies Inc. (NSTI) kindly
 * supported the development of this code.
 *
 * Copyright (c) 2000, 2001 Angelos D. Keromytis
 *
 * Permission to use, copy, and modify this software with or without fee
 * is hereby granted, provided that this entire notice is included in
 * all source code copies of any software which is or includes a copy or
 * modification of this software.
 *
 * THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR
 * IMPLIED WARRANTY. IN PARTICULAR, NONE OF THE AUTHORS MAKES ANY
 * REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE
 * MERCHANTABILITY OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR
 * PURPOSE.
 */
/* </LICENSES> */

/* <CODE> */
//! The software crypto driver: sessions made of one entry per algorithm ([`SwcrData`]), and the
//! processing of a request by running its descriptors against them: ciphers
//! ([`swcr_encdec`]), keyed-hash authenticators ([`swcr_authcompute`]) and the combined
//! encrypt-and-authenticate transforms, AES-GCM and ChaCha20-Poly1305 ([`swcr_authenc`]).
//!
//! Upstream: sys/crypto/cryptosoft.h @ 3ce1f3f79392, sys/crypto/cryptosoft.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header and the file share this module. `struct swcr_data` is [`SwcrData`] with
//!   its `SWCR_UN` union an enum ([`SwcrUn`]: authenticator, cipher or compressor), the contexts
//!   `Box`es of the [`AuthCtx`] and [`Kschedule`] enums of `xform.rs`; the `swcr_list`
//!   `SLIST` of a session is a `Vec` in the order of the `cryptoini` chain (the C inserts at the
//!   head and then after the previous entry, which is the same order). `swcr_sessions`
//!   is a `Vec` of those, its length being `swcr_sesnum`, protected like `crypto_drivers`
//!   (`with_sessions`). Dropping a [`SwcrData`] wipes its contexts, which is what
//!   `swcr_freesession` does with `explicit_bzero` before `free`.
//! - A buffer is the [`CryptoBuf`] of the request, whose variant is the `outtype` argument of
//!   the C; `COPYBACK` and `COPYDATA` are `copyback` and `copydata` over it. Functions that take
//!   the request in the C take the pieces they need (`swcr_authcompute` the `crp_mac` slot,
//!   `swcr_authenc` the session list), so that nothing borrows the session table twice.
//! - `swcr_encdec` keeps the chaining value in a local array (`prev`) instead of pointing `ivp`
//!   into the mbuf or the iovec at the previous cipher block, and runs the one block routine
//!   from all four of the C's copies of it (an mbuf run or a block straddling mbufs, an iovec
//!   run or straddling iovecs). `swcr_encdec`'s `m_copyback` and the `COPYBACK` of the
//!   authentication tag and of the IV report their error where the C ignores the result.
//! - `swcr_newsession` does not modify the caller's key: the C XORs `cri_key` with the pads in
//!   place and back; here the padded key is built in a local (and a key longer than the hash's
//!   block, whose pad length the C computes as a negative number, is `EINVAL`). A `setkey`
//!   that fails is `EINVAL` as in the C, and so is a GMAC or ChaCha20-Poly1305 `Setkey` that
//!   reports a bad key size (the C's are `void`).
//! - `swcr_compdec` gets the (de)compressed data as a `Vec<u8>` from the [`CompAlgo`] and
//!   copies it back with `COPYBACK`, whose `m_copyback` error it returns (as the C does); a
//!   failed copy of the input (`COPYDATA` of a buffer that is not there) is an error too. The
//!   trim of a `uio` shortens the `uio_iov` slice where the C decrements `uio_iovcnt`.
//! - `swcr_process` handles `CRYPTO_RIJNDAEL128_CBC` and `CRYPTO_AES_CBC` as one case (they are
//!   both 7).
//! - The `hmac_ipad_buffer` and `hmac_opad_buffer` tables are constants of the pad byte.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::slice;
use core::sync::atomic::{AtomicI32, Ordering};

use libkern::{StaticCell, explicit_bzero};

use super::criov::{cuio_apply, cuio_copyback, cuio_copydata, cuio_getptr, iov_run_mut};
use super::crypto::{crypto_get_driverid, crypto_register};
use super::cryptodev::{
    AALG_MAX_RESULT_LEN, CRD_F_COMP, CRD_F_ENCRYPT, CRD_F_ESN, CRD_F_IV_EXPLICIT, CRD_F_IV_PRESENT,
    CRYPTO_3DES_CBC, CRYPTO_AES_128_GMAC, CRYPTO_AES_192_GMAC, CRYPTO_AES_256_GMAC, CRYPTO_AES_CBC,
    CRYPTO_AES_CTR, CRYPTO_AES_GCM_16, CRYPTO_AES_GMAC, CRYPTO_AES_XTS, CRYPTO_ALG_FLAG_SUPPORTED,
    CRYPTO_ALGORITHM_MAX, CRYPTO_BLF_CBC, CRYPTO_CAST_CBC, CRYPTO_CHACHA20_POLY1305,
    CRYPTO_CHACHA20_POLY1305_MAC, CRYPTO_DEFLATE_COMP, CRYPTO_ESN, CRYPTO_F_IMBUF, CRYPTO_MD5_HMAC,
    CRYPTO_NULL, CRYPTO_RIPEMD160_HMAC, CRYPTO_SHA1_HMAC, CRYPTO_SHA2_256_HMAC,
    CRYPTO_SHA2_384_HMAC, CRYPTO_SHA2_512_HMAC, CRYPTO_SW_SESSIONS, CRYPTOCAP_F_SOFTWARE,
    CryptoBuf, Cryptodesc, Cryptoini, Cryptop, EALG_MAX_BLOCK_LEN, HMAC_IPAD_VAL,
    HMAC_MAX_BLOCK_LEN, HMAC_OPAD_VAL,
};
use super::wipe;
use super::xform::{
    AuthCtx, AuthHash, CompAlgo, EncXform, Kschedule, auth_hash_chacha20_poly1305,
    auth_hash_gmac_aes_128, auth_hash_gmac_aes_192, auth_hash_gmac_aes_256, auth_hash_hmac_md5_96,
    auth_hash_hmac_ripemd_160_96, auth_hash_hmac_sha1_96, auth_hash_hmac_sha2_256_128,
    auth_hash_hmac_sha2_384_192, auth_hash_hmac_sha2_512_256, comp_algo_deflate, enc_xform_3des,
    enc_xform_aes, enc_xform_aes_ctr, enc_xform_aes_gcm, enc_xform_aes_gmac, enc_xform_aes_xts,
    enc_xform_blf, enc_xform_cast5, enc_xform_chacha20_poly1305, enc_xform_null,
};
use crate::dev::rnd::arc4random_buf;
use crate::kassert;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_adj, m_apply, m_copyback, m_copydata, m_getptr};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::mbuf::{MAXMCLBYTES, mtod};
use crate::sys::uio::Uio;

/// `hmac_ipad_buffer`.
#[allow(non_upper_case_globals)] // the C name
pub static hmac_ipad_buffer: [u8; HMAC_MAX_BLOCK_LEN] = [HMAC_IPAD_VAL; HMAC_MAX_BLOCK_LEN];

/// `hmac_opad_buffer`.
#[allow(non_upper_case_globals)] // the C name
pub static hmac_opad_buffer: [u8; HMAC_MAX_BLOCK_LEN] = [HMAC_OPAD_VAL; HMAC_MAX_BLOCK_LEN];

/// `SWCR_AUTH`: an authenticator's part of a software session entry.
pub struct SwcrAuth {
    /// `SW_ictx`: the inner hash state after the key.
    pub sw_ictx: Option<Box<AuthCtx>>,
    /// `SW_octx`: the outer one.
    pub sw_octx: Option<Box<AuthCtx>>,
    /// `SW_klen`.
    pub sw_klen: u32,
    /// `SW_axf`.
    pub sw_axf: &'static AuthHash,
}

/// `SWCR_ENC`: a cipher's part of a software session entry.
pub struct SwcrEnc {
    /// `SW_kschedule`: the key schedule (`None` for a transform that has none).
    pub sw_kschedule: Option<Box<Kschedule>>,
    /// `SW_exf`.
    pub sw_exf: &'static EncXform,
}

/// `SWCR_COMP`: a compressor's part of a software session entry.
pub struct SwcrComp {
    /// `SW_size`.
    pub sw_size: u32,
    /// `SW_cxf`.
    pub sw_cxf: &'static CompAlgo,
}

/// `SWCR_UN`: the algorithm-specific part of a software session entry.
pub enum SwcrUn {
    /// An algorithm with nothing to keep (`CRYPTO_ESN`).
    None,
    /// `SWCR_AUTH`.
    Auth(SwcrAuth),
    /// `SWCR_ENC`.
    Enc(SwcrEnc),
    /// `SWCR_COMP`.
    Comp(SwcrComp),
}

/// `struct swcr_data`: software session entry.
#[allow(non_snake_case)] // the C name of the member, SWCR_UN
pub struct SwcrData {
    /// `sw_alg`: algorithm.
    pub sw_alg: i32,
    /// `SWCR_UN`.
    pub SWCR_UN: SwcrUn,
}

impl Drop for SwcrData {
    /// `swcr_freesession`'s `explicit_bzero` of the contexts before they are freed.
    fn drop(&mut self) {
        match &mut self.SWCR_UN {
            SwcrUn::Enc(e) => {
                if let Some(ks) = e.sw_kschedule.as_deref_mut() {
                    wipe(ks);
                }
            }
            SwcrUn::Auth(a) => {
                if let Some(c) = a.sw_ictx.as_deref_mut() {
                    wipe(c);
                }
                if let Some(c) = a.sw_octx.as_deref_mut() {
                    wipe(c);
                }
            }
            SwcrUn::Comp(_) | SwcrUn::None => {}
        }
    }
}

/// `struct swcr_list`: the entries of one session.
pub type SwcrList = Vec<SwcrData>;

/// `swcr_sessions`: the sessions, indexed by session number; entry 0 is never used and a
/// session with no entries is free. Its length is `swcr_sesnum`.
#[allow(non_upper_case_globals)] // the C name
pub static swcr_sessions: StaticCell<Vec<SwcrList>> = StaticCell::new(Vec::new());

/// `swcr_id`: the driver id of the software driver (-1 before `swcr_init`).
#[allow(non_upper_case_globals)] // the C name
pub static swcr_id: AtomicI32 = AtomicI32::new(-1);

/// Runs `f` on the session table, under the same rules as `crypto.rs`'s `with_drivers`.
fn with_sessions<R>(f: impl FnOnce(&mut Vec<SwcrList>) -> R) -> R {
    // SAFETY: serialized by the kernel lock and splvm, as the C's [K]; `f` is the only user of
    // the table while it runs, and none of the code it calls opens the table again.
    f(unsafe { swcr_sessions.get_mut() })
}

/// `swcr_sesnum`: the number of session slots.
pub fn swcr_sesnum() -> u32 {
    with_sessions(|s| s.len() as u32)
}

/// `COPYBACK`: writes `data` into the request's buffer at `off`.
fn copyback(buf: &CryptoBuf<'_>, off: i32, data: &[u8]) -> Result<(), Errno> {
    match buf {
        CryptoBuf::Mbuf(m) => m_copyback(*m, off, data, M_NOWAIT),
        CryptoBuf::Iov(uio) => {
            cuio_copyback(uio, off, data);
            Ok(())
        }
        CryptoBuf::None => Err(Errno::EINVAL),
    }
}

/// `COPYDATA`: reads the request's buffer at `off` into `out`.
fn copydata(buf: &CryptoBuf<'_>, off: i32, out: &mut [u8]) -> Result<(), Errno> {
    match buf {
        CryptoBuf::Mbuf(m) => {
            m_copydata(m, off, out);
            Ok(())
        }
        CryptoBuf::Iov(uio) => {
            cuio_copydata(uio, off, out);
            Ok(())
        }
        CryptoBuf::None => Err(Errno::EINVAL),
    }
}

/// The chaining state of a CBC run: the previous ciphertext block (the IV before the first).
struct Chain {
    prev: [u8; EALG_MAX_BLOCK_LEN],
}

/// Runs the cipher over one block `blk` of `exf.blocksize` bytes, in place: CBC chaining for a
/// transform without `reinit`, the transform's own IV handling for one that has it.
fn swcr_block(
    exf: &EncXform,
    ks: &mut Kschedule,
    encrypt: bool,
    chain: &mut Chain,
    blk: &mut [u8],
) -> Result<(), Errno> {
    let blks = blk.len();
    let (Some(enc), Some(dec)) = (exf.encrypt, exf.decrypt) else {
        return Err(Errno::EINVAL);
    };

    if exf.reinit.is_some() {
        if encrypt {
            enc(ks, blk);
        } else {
            dec(ks, blk);
        }
    } else if encrypt {
        // XOR with previous block/IV
        for (b, p) in blk.iter_mut().zip(&chain.prev) {
            *b ^= p;
        }

        enc(ks, blk);

        // Keep encrypted block for XOR'ing with next block
        chain.prev[..blks].copy_from_slice(blk);
    } else {
        // Keep encrypted block for XOR'ing with next block
        let mut save = [0u8; EALG_MAX_BLOCK_LEN];
        save[..blks].copy_from_slice(blk);

        dec(ks, blk);

        // XOR with previous block/IV
        for (b, p) in blk.iter_mut().zip(&chain.prev) {
            *b ^= p;
        }
        chain.prev[..blks].copy_from_slice(&save[..blks]);
    }
    Ok(())
}

/// `swcr_encdec`: apply a symmetric encryption/decryption algorithm to the `crd_len` bytes
/// from `crd_skip` of the request's buffer.
pub fn swcr_encdec(
    crd: &Cryptodesc<'_>,
    sw: &mut SwcrData,
    buf: &mut CryptoBuf<'_>,
) -> Result<(), Errno> {
    let SwcrUn::Enc(enc) = &mut sw.SWCR_UN else {
        return Err(Errno::EINVAL);
    };
    let exf = enc.sw_exf;
    let blks = usize::from(exf.blocksize);
    let ivlen = usize::from(exf.ivsize);
    let encrypt = (crd.crd_flags & CRD_F_ENCRYPT) != 0;
    let mut iv = [0u8; EALG_MAX_BLOCK_LEN];

    // Check for non-padded data
    if blks == 0 || crd.crd_len % blks as i32 != 0 {
        return Err(Errno::EINVAL);
    }

    // Initialize the IV
    if encrypt {
        // IV explicitly provided ?
        if (crd.crd_flags & CRD_F_IV_EXPLICIT) != 0 {
            iv[..ivlen].copy_from_slice(&crd.crd_iv()[..ivlen]);
        } else {
            arc4random_buf(&mut iv[..ivlen]);
        }

        // Do we need to write the IV
        if (crd.crd_flags & CRD_F_IV_PRESENT) == 0 {
            copyback(buf, crd.crd_inject, &iv[..ivlen])?;
        }
    } else {
        // Decryption: IV explicitly provided ?
        if (crd.crd_flags & CRD_F_IV_EXPLICIT) != 0 {
            iv[..ivlen].copy_from_slice(&crd.crd_iv()[..ivlen]);
        } else {
            // Get IV off buf
            copydata(buf, crd.crd_inject, &mut iv[..ivlen])?;
        }
    }

    let mut chain = Chain { prev: iv };
    let mut dummy = Kschedule::None;
    let ks: &mut Kschedule = match enc.sw_kschedule.as_deref_mut() {
        Some(k) => k,
        None => &mut dummy,
    };

    // xforms that provide a reinit method perform all IV handling themselves.
    if let Some(reinit) = exf.reinit {
        reinit(ks, &iv[..ivlen]);
    }

    match buf {
        CryptoBuf::Mbuf(m0) => {
            // Find beginning of data
            let Some((mut m, mut k)) = m_getptr(m0, crd.crd_skip) else {
                return Err(Errno::EINVAL);
            };

            let mut i = crd.crd_len;
            let b = blks as i32;

            while i > 0 {
                // If there's insufficient data at the end of an mbuf, we have to do some
                // copying.
                let mlen = m.m_len().get() as i32;
                if mlen < k + b && mlen != k {
                    let mut blk = [0u8; EALG_MAX_BLOCK_LEN];
                    m_copydata(m, k, &mut blk[..blks]);

                    // Actual encryption/decryption
                    swcr_block(exf, ks, encrypt, &mut chain, &mut blk[..blks])?;

                    // Copy back decrypted block
                    m_copyback(m, k, &blk[..blks], M_NOWAIT)?;

                    // Advance pointer
                    let Some((m2, k2)) = m_getptr(m, k + b) else {
                        return Err(Errno::EINVAL);
                    };
                    m = m2;
                    k = k2;

                    i -= b;

                    // Could be done...
                    if i == 0 {
                        break;
                    }
                }

                // Skip possibly empty mbufs
                if k == m.m_len().get() as i32 {
                    let mut next = m.m_next().get();
                    while let Some(n) = next
                        && n.m_len().get() == 0
                    {
                        next = n.m_next().get();
                    }
                    k = 0;
                    // Sanity check
                    match next {
                        Some(n) => m = n,
                        None => return Err(Errno::EINVAL),
                    }
                }

                // Only run over whole blocks of this mbuf: the data is at `k`.
                while m.m_len().get() as i32 >= k + b && i > 0 {
                    // SAFETY: `k + blks <= m_len`, so the block is inside this mbuf's data,
                    // which the request owns for the duration of the call; no other reference
                    // to these bytes is live.
                    let idat =
                        unsafe { slice::from_raw_parts_mut(mtod::<u8>(m).add(k as usize), blks) };
                    swcr_block(exf, ks, encrypt, &mut chain, idat)?;

                    k += b;
                    i -= b;
                }
            }
        }
        CryptoBuf::Iov(uio) => {
            let uio: &Uio<'_> = uio;
            // Find beginning of data
            let mut count = crd.crd_skip;
            let Some((mut ind, mut k)) = cuio_getptr(uio, count) else {
                return Err(Errno::EINVAL);
            };

            let mut i = crd.crd_len;
            let b = blks as i32;

            while i > 0 {
                // If there's insufficient data at the end, we have to do some copying.
                let ilen = uio.uio_iov[ind].iov_len as i32;
                if ilen < k + b && ilen != k {
                    let mut blk = [0u8; EALG_MAX_BLOCK_LEN];
                    cuio_copydata(uio, count, &mut blk[..blks]);

                    // Actual encryption/decryption
                    swcr_block(exf, ks, encrypt, &mut chain, &mut blk[..blks])?;

                    // Copy back decrypted block
                    cuio_copyback(uio, count, &blk[..blks]);

                    count += b;

                    // Advance pointer
                    let Some((i2, k2)) = cuio_getptr(uio, count) else {
                        return Err(Errno::EINVAL);
                    };
                    ind = i2;
                    k = k2;

                    i -= b;

                    // Could be done...
                    if i == 0 {
                        break;
                    }
                }

                while uio.uio_iov[ind].iov_len as i32 >= k + b && i > 0 {
                    let idat = iov_run_mut(&uio.uio_iov[ind], k as usize, blks);
                    swcr_block(exf, ks, encrypt, &mut chain, idat)?;

                    count += b;
                    k += b;
                    i -= b;
                }

                // Advance to the next iov if the end of the current iov is aligned with the end
                // of a cipher block. Note that the code is equivalent to calling:
                //	ind = cuio_getptr(uio, count, &k);
                if i > 0 && k == uio.uio_iov[ind].iov_len as i32 {
                    k = 0;
                    ind += 1;
                    if ind >= uio.uio_iovcnt() {
                        return Err(Errno::EINVAL);
                    }
                }
            }
        }
        CryptoBuf::None => return Err(Errno::EINVAL),
    }

    Ok(()) // Done with encryption/decryption
}

/// `swcr_authcompute`: compute keyed-hash authenticator. The MAC is injected into an mbuf
/// buffer at `crd_inject`, and goes to `crp_mac` for an uio.
pub fn swcr_authcompute(
    crp_mac: &mut Option<&mut [u8]>,
    crd: &Cryptodesc<'_>,
    sw: &SwcrData,
    buf: &mut CryptoBuf<'_>,
) -> Result<(), Errno> {
    let SwcrUn::Auth(auth) = &sw.SWCR_UN else {
        return Err(Errno::EINVAL);
    };
    let Some(ictx) = &auth.sw_ictx else {
        return Err(Errno::EINVAL);
    };
    let axf = auth.sw_axf;
    let mut aalg = [0u8; AALG_MAX_RESULT_LEN];

    let mut ctx: AuthCtx = **ictx;

    match buf {
        CryptoBuf::Mbuf(m) => {
            m_apply(m, crd.crd_skip, crd.crd_len, |b| (axf.Update)(&mut ctx, b))?;
        }
        CryptoBuf::Iov(uio) => {
            cuio_apply(uio, crd.crd_skip, crd.crd_len, |b| {
                (axf.Update)(&mut ctx, b)
            })?;
        }
        CryptoBuf::None => return Err(Errno::EINVAL),
    }

    if (crd.crd_flags & CRD_F_ESN) != 0 {
        (axf.Update)(&mut ctx, &crd.crd_esn())?;
    }

    match sw.sw_alg {
        CRYPTO_MD5_HMAC
        | CRYPTO_SHA1_HMAC
        | CRYPTO_RIPEMD160_HMAC
        | CRYPTO_SHA2_256_HMAC
        | CRYPTO_SHA2_384_HMAC
        | CRYPTO_SHA2_512_HMAC => {
            let Some(octx) = &auth.sw_octx else {
                return Err(Errno::EINVAL);
            };

            (axf.Final)(&mut aalg, &mut ctx);
            ctx = **octx;
            (axf.Update)(&mut ctx, &aalg[..usize::from(axf.hashsize)])?;
            (axf.Final)(&mut aalg, &mut ctx);
        }
        _ => {}
    }

    // Inject the authentication data
    let authsize = usize::from(axf.authsize);
    match buf {
        CryptoBuf::Mbuf(_) => copyback(buf, crd.crd_inject, &aalg[..authsize])?,
        _ => {
            let Some(mac) = crp_mac.as_deref_mut() else {
                return Err(Errno::EINVAL);
            };
            if mac.len() < authsize {
                return Err(Errno::EINVAL);
            }
            mac[..authsize].copy_from_slice(&aalg[..authsize]);
        }
    }

    Ok(())
}

/// The kind of buffer a request carries, from its flags: `CRYPTO_BUF_MBUF` or `CRYPTO_BUF_IOV`
/// (as `swcr_process` computes it), and whether the buffer is the one the flags say.
fn buf_matches_flags(crp: &Cryptop<'_>) -> bool {
    match crp.crp_buf {
        CryptoBuf::Mbuf(_) => (crp.crp_flags & CRYPTO_F_IMBUF) != 0,
        CryptoBuf::Iov(_) => (crp.crp_flags & CRYPTO_F_IMBUF) == 0,
        CryptoBuf::None => false,
    }
}

/// `swcr_authenc`: apply a combined encryption-authentication transformation (a cipher
/// descriptor and its authenticator descriptor of `session`).
pub fn swcr_authenc(crp: &mut Cryptop<'_>, session: &mut SwcrList) -> Result<(), Errno> {
    let mut blk = [0u8; EALG_MAX_BLOCK_LEN];
    let mut aalg = [0u8; AALG_MAX_RESULT_LEN];
    let mut iv = [0u8; EALG_MAX_BLOCK_LEN];
    let mut ctx = AuthCtx::None;
    let mut crda: Option<Cryptodesc<'_>> = None;
    let mut crde: Option<Cryptodesc<'_>> = None;
    let mut swe: Option<usize> = None;
    let mut axf: Option<&'static AuthHash> = None;
    let mut exf: Option<&'static EncXform> = None;
    let (mut blksz, mut ivlen) = (0usize, 0usize);
    let (mut iskip, mut oskip) = (0usize, 0usize);

    for i in 0..(crp.crp_ndesc as usize).min(crp.crp_desc.len()) {
        let crd = crp.crp_desc[i];
        let Some(si) = session
            .iter()
            .position(|sw| sw.sw_alg == crd.CRD_INI.cri_alg)
        else {
            return Err(Errno::EINVAL);
        };
        let sw = &session[si];

        match sw.sw_alg {
            CRYPTO_AES_GCM_16 | CRYPTO_AES_GMAC | CRYPTO_CHACHA20_POLY1305 => {
                let SwcrUn::Enc(enc) = &sw.SWCR_UN else {
                    return Err(Errno::EINVAL);
                };
                swe = Some(si);
                crde = Some(crd);
                exf = Some(enc.sw_exf);
                ivlen = usize::from(enc.sw_exf.ivsize);
            }
            CRYPTO_AES_128_GMAC
            | CRYPTO_AES_192_GMAC
            | CRYPTO_AES_256_GMAC
            | CRYPTO_CHACHA20_POLY1305_MAC => {
                let SwcrUn::Auth(auth) = &sw.SWCR_UN else {
                    return Err(Errno::EINVAL);
                };
                crda = Some(crd);
                axf = Some(auth.sw_axf);
                let Some(ictx) = &auth.sw_ictx else {
                    return Err(Errno::EINVAL);
                };
                ctx = **ictx;
                blksz = usize::from(auth.sw_axf.blocksize);
            }
            _ => return Err(Errno::EINVAL),
        }
    }
    let (Some(crde), Some(crda), Some(swe), Some(exf), Some(axf)) = (crde, crda, swe, exf, axf)
    else {
        return Err(Errno::EINVAL);
    };

    if !buf_matches_flags(crp) {
        return Err(Errno::EINVAL);
    }

    // Initialize the IV
    if (crde.crd_flags & CRD_F_ENCRYPT) != 0 {
        // IV explicitly provided ?
        if (crde.crd_flags & CRD_F_IV_EXPLICIT) != 0 {
            iv[..ivlen].copy_from_slice(&crde.crd_iv()[..ivlen]);
        } else {
            arc4random_buf(&mut iv[..ivlen]);
        }

        // Do we need to write the IV
        if (crde.crd_flags & CRD_F_IV_PRESENT) == 0 {
            copyback(&crp.crp_buf, crde.crd_inject, &iv[..ivlen])?;
        }
    } else {
        // Decryption: IV explicitly provided ?
        if (crde.crd_flags & CRD_F_IV_EXPLICIT) != 0 {
            iv[..ivlen].copy_from_slice(&crde.crd_iv()[..ivlen]);
        } else {
            // Get IV off buf
            copydata(&crp.crp_buf, crde.crd_inject, &mut iv[..ivlen])?;
        }
    }

    // Supply MAC with IV
    if let Some(reinit) = axf.Reinit {
        reinit(&mut ctx, &iv[..ivlen]);
    }

    // Supply MAC with AAD
    let mut aadlen = crda.crd_len;
    let hashsize = usize::from(axf.hashsize);
    // Section 5 of RFC 4106 specifies that AAD construction consists of {SPI, ESN, SN} whereas
    // the real packet contains only {SPI, SN}. Unfortunately it doesn't follow a good example
    // set in the Section 3.3.2.1 of RFC 4303 where upper part of the ESN, located in the
    // external (to the packet) memory buffer, is processed by the hash function in the end
    // thus allowing to retain simple programming interfaces and avoid kludges like the one
    // below.
    if (crda.crd_flags & CRD_F_ESN) != 0 {
        aadlen += 4;
        // SPI
        copydata(&crp.crp_buf, crda.crd_skip, &mut blk[..4])?;
        iskip = 4; // loop below will start with an offset of 4
        // ESN
        blk[4..8].copy_from_slice(&crda.crd_esn());
        oskip = iskip + 4; // offset output buffer blk by 8
    }
    let mut i = iskip;
    while (i as i32) < crda.crd_len {
        let len = ((crda.crd_len - i as i32) as usize).min(hashsize - oskip);
        copydata(
            &crp.crp_buf,
            crda.crd_skip + i as i32,
            &mut blk[oskip..oskip + len],
        )?;
        blk[len + oskip..hashsize].fill(0);
        (axf.Update)(&mut ctx, &blk[..hashsize])?;
        oskip = 0; // reset initial output offset
        i += hashsize;
    }

    let SwcrUn::Enc(enc) = &mut session[swe].SWCR_UN else {
        return Err(Errno::EINVAL);
    };
    let mut dummy = Kschedule::None;
    let ks: &mut Kschedule = match enc.sw_kschedule.as_deref_mut() {
        Some(k) => k,
        None => &mut dummy,
    };
    if let Some(reinit) = exf.reinit {
        reinit(ks, &iv[..ivlen]);
    }

    // Do encryption/decryption with MAC
    let mut i = 0usize;
    while (i as i32) < crde.crd_len {
        let len = ((crde.crd_len - i as i32) as usize).min(blksz);
        if len < blksz {
            blk[..blksz].fill(0);
        }
        copydata(&crp.crp_buf, crde.crd_skip + i as i32, &mut blk[..len])?;
        if (crde.crd_flags & CRD_F_ENCRYPT) != 0 {
            let Some(encrypt) = exf.encrypt else {
                return Err(Errno::EINVAL);
            };
            encrypt(ks, &mut blk[..blksz]);
            (axf.Update)(&mut ctx, &blk[..len])?;
        } else {
            let Some(decrypt) = exf.decrypt else {
                return Err(Errno::EINVAL);
            };
            (axf.Update)(&mut ctx, &blk[..len])?;
            decrypt(ks, &mut blk[..blksz]);
        }
        copyback(&crp.crp_buf, crde.crd_skip + i as i32, &blk[..len])?;
        i += blksz;
    }

    // Do any required special finalization
    match crda.CRD_INI.cri_alg {
        CRYPTO_AES_128_GMAC | CRYPTO_AES_192_GMAC | CRYPTO_AES_256_GMAC => {
            // length block
            blk[..hashsize].fill(0);
            blk[4..8].copy_from_slice(&((aadlen as u32).wrapping_mul(8)).to_be_bytes());
            blk[12..16].copy_from_slice(&((crde.crd_len as u32).wrapping_mul(8)).to_be_bytes());
            (axf.Update)(&mut ctx, &blk[..hashsize])?;
        }
        CRYPTO_CHACHA20_POLY1305_MAC => {
            // length block
            blk[..hashsize].fill(0);
            blk[0..4].copy_from_slice(&(aadlen as u32).to_le_bytes());
            blk[8..12].copy_from_slice(&(crde.crd_len as u32).to_le_bytes());
            (axf.Update)(&mut ctx, &blk[..hashsize])?;
        }
        _ => {}
    }

    // Finalize MAC
    (axf.Final)(&mut aalg, &mut ctx);

    // Inject the authentication data
    let authsize = usize::from(axf.authsize);
    match &crp.crp_buf {
        CryptoBuf::Mbuf(_) => copyback(&crp.crp_buf, crda.crd_inject, &aalg[..authsize])?,
        _ => {
            let Some(mac) = crp.crp_mac.as_deref_mut() else {
                return Err(Errno::EINVAL);
            };
            if mac.len() < authsize {
                return Err(Errno::EINVAL);
            }
            mac[..authsize].copy_from_slice(&aalg[..authsize]);
        }
    }

    explicit_bzero(&mut blk);
    explicit_bzero(&mut iv);
    wipe(&mut ctx);
    Ok(())
}

/// `swcr_compdec`: apply a compression/decompression algorithm. The (de)compressed data
/// replaces the `crd_len` bytes at `crd_skip` of the buffer, which is extended or trimmed to
/// fit; its length is left in `sw_size`. Compressed data that is not shorter than the input
/// is not written back (the caller sees `sw_size` and keeps the original).
pub fn swcr_compdec(
    crd: &Cryptodesc<'_>,
    sw: &mut SwcrData,
    buf: &mut CryptoBuf<'_>,
) -> Result<(), Errno> {
    let SwcrUn::Comp(comp) = &mut sw.SWCR_UN else {
        return Err(Errno::EINVAL);
    };
    let cxf = comp.sw_cxf;
    let crd_len = usize::try_from(crd.crd_len).map_err(|_| Errno::EINVAL)?;

    // We must handle the whole buffer of data in one time then if there is not all the data
    // in the mbuf, we must copy in a buffer.
    let mut data = Vec::new();
    if data.try_reserve_exact(crd_len).is_err() {
        return Err(Errno::EINVAL);
    }
    data.resize(crd_len, 0);
    copydata(buf, crd.crd_skip, &mut data)?;

    let out = if crd.crd_flags & CRD_F_COMP != 0 {
        (cxf.compress)(&data)
    } else {
        (cxf.decompress)(&data)
    };

    drop(data);
    let out = match out {
        Ok(out) if !out.is_empty() => out,
        _ => return Err(Errno::EINVAL),
    };
    let result = out.len();

    // Copy back the (de)compressed data. m_copyback is extending the mbuf as necessary.
    comp.sw_size = result as u32;
    // Check the compressed size when doing compression
    if crd.crd_flags & CRD_F_COMP != 0 {
        if result > crd_len {
            // Compression was useless, we lost time
            return Ok(());
        }
    } else {
        // Decompressed IP packet must fit into mbuf cluster.
        if matches!(buf, CryptoBuf::Mbuf(_)) && result > MAXMCLBYTES {
            return Err(Errno::EMSGSIZE);
        }
    }

    copyback(buf, crd.crd_skip, &out)?;
    if result < crd_len {
        match buf {
            CryptoBuf::Mbuf(m) => m_adj(*m, result as i32 - crd.crd_len),
            CryptoBuf::Iov(uio) => {
                let mut adj = crd_len - result;
                let mut iov = core::mem::take(&mut uio.uio_iov);
                while adj > 0 {
                    let Some(last) = iov.last_mut() else {
                        break;
                    };
                    if adj < last.iov_len {
                        last.iov_len -= adj;
                        break;
                    }
                    adj -= last.iov_len;
                    last.iov_len = 0;
                    // uio_iovcnt--
                    let n = iov.len() - 1;
                    iov = &mut iov[..n];
                }
                uio.uio_iov = iov;
            }
            CryptoBuf::None => {}
        }
    }
    Ok(())
}

/// `swcr_newsession`: generate a new software session. `sid` is the driver id on entry and the
/// session number on return.
pub fn swcr_newsession(sid: &mut u32, cri: &Cryptoini<'_>) -> Result<(), Errno> {
    let i = with_sessions(|sessions| -> Result<usize, Errno> {
        let mut free = None;
        for (n, s) in sessions.iter().enumerate().skip(1) {
            if s.is_empty() {
                free = Some(n);
                break;
            }
        }
        if let Some(n) = free {
            return Ok(n);
        }

        let (first, newnum) = if sessions.is_empty() {
            (1, CRYPTO_SW_SESSIONS) // We leave swcr_sessions[0] empty
        } else {
            (sessions.len(), sessions.len() * 2)
        };
        if sessions.try_reserve_exact(newnum - sessions.len()).is_err() {
            return Err(Errno::ENOBUFS);
        }
        sessions.resize_with(newnum, Vec::new);
        Ok(first)
    })?;

    let mut list: SwcrList = Vec::new();
    let mut cri = Some(cri);

    while let Some(c) = cri {
        let mut swd = SwcrData {
            sw_alg: 0,
            SWCR_UN: SwcrUn::None,
        };

        match c.cri_alg {
            CRYPTO_3DES_CBC => swcr_enc(&mut swd, c, &enc_xform_3des)?,
            CRYPTO_BLF_CBC => swcr_enc(&mut swd, c, &enc_xform_blf)?,
            CRYPTO_CAST_CBC => swcr_enc(&mut swd, c, &enc_xform_cast5)?,
            CRYPTO_AES_CBC => swcr_enc(&mut swd, c, &enc_xform_aes)?,
            CRYPTO_AES_CTR => swcr_enc(&mut swd, c, &enc_xform_aes_ctr)?,
            CRYPTO_AES_XTS => swcr_enc(&mut swd, c, &enc_xform_aes_xts)?,
            CRYPTO_AES_GCM_16 => swcr_enc(&mut swd, c, &enc_xform_aes_gcm)?,
            CRYPTO_AES_GMAC => {
                swd.SWCR_UN = SwcrUn::Enc(SwcrEnc {
                    sw_kschedule: None,
                    sw_exf: &enc_xform_aes_gmac,
                });
            }
            CRYPTO_CHACHA20_POLY1305 => swcr_enc(&mut swd, c, &enc_xform_chacha20_poly1305)?,
            CRYPTO_NULL => swcr_enc(&mut swd, c, &enc_xform_null)?,

            CRYPTO_MD5_HMAC => swcr_hmac(&mut swd, c, &auth_hash_hmac_md5_96)?,
            CRYPTO_SHA1_HMAC => swcr_hmac(&mut swd, c, &auth_hash_hmac_sha1_96)?,
            CRYPTO_RIPEMD160_HMAC => swcr_hmac(&mut swd, c, &auth_hash_hmac_ripemd_160_96)?,
            CRYPTO_SHA2_256_HMAC => swcr_hmac(&mut swd, c, &auth_hash_hmac_sha2_256_128)?,
            CRYPTO_SHA2_384_HMAC => swcr_hmac(&mut swd, c, &auth_hash_hmac_sha2_384_192)?,
            CRYPTO_SHA2_512_HMAC => swcr_hmac(&mut swd, c, &auth_hash_hmac_sha2_512_256)?,

            CRYPTO_AES_128_GMAC => swcr_authencommon(&mut swd, c, &auth_hash_gmac_aes_128)?,
            CRYPTO_AES_192_GMAC => swcr_authencommon(&mut swd, c, &auth_hash_gmac_aes_192)?,
            CRYPTO_AES_256_GMAC => swcr_authencommon(&mut swd, c, &auth_hash_gmac_aes_256)?,
            CRYPTO_CHACHA20_POLY1305_MAC => {
                swcr_authencommon(&mut swd, c, &auth_hash_chacha20_poly1305)?
            }

            CRYPTO_DEFLATE_COMP => {
                swd.SWCR_UN = SwcrUn::Comp(SwcrComp {
                    sw_size: 0,
                    sw_cxf: &comp_algo_deflate,
                });
            }
            CRYPTO_ESN => {
                // nothing to do
            }
            _ => return Err(Errno::EINVAL),
        }

        swd.sw_alg = c.cri_alg;
        list.push(swd);
        cri = c.cri_next;
    }

    with_sessions(|sessions| sessions[i] = list);
    *sid = i as u32;
    Ok(())
}

/// `enccommon`: a cipher's schedule from the key of `cri`.
fn swcr_enc(swd: &mut SwcrData, cri: &Cryptoini<'_>, txf: &'static EncXform) -> Result<(), Errno> {
    let klen = usize::try_from(cri.cri_klen / 8).map_err(|_| Errno::EINVAL)?;
    if klen > cri.cri_key.len() {
        return Err(Errno::EINVAL);
    }
    let mut kschedule = None;

    if txf.ctxsize > 0 {
        kschedule = Some(Box::new(Kschedule::None));
    }
    if let Some(setkey) = txf.setkey {
        let mut dummy = Kschedule::None;
        let ks: &mut Kschedule = match kschedule.as_deref_mut() {
            Some(k) => k,
            None => &mut dummy,
        };
        if setkey(ks, &cri.cri_key[..klen]).is_err() {
            return Err(Errno::EINVAL);
        }
    }
    swd.SWCR_UN = SwcrUn::Enc(SwcrEnc {
        sw_kschedule: kschedule,
        sw_exf: txf,
    });
    Ok(())
}

/// `authcommon`: the inner and outer contexts of an HMAC, the key XORed with the pads and
/// padded to the block size.
fn swcr_hmac(swd: &mut SwcrData, cri: &Cryptoini<'_>, axf: &'static AuthHash) -> Result<(), Errno> {
    let klen = usize::try_from(cri.cri_klen / 8).map_err(|_| Errno::EINVAL)?;
    let blocksize = usize::from(axf.blocksize);
    if klen > cri.cri_key.len() || klen > blocksize {
        return Err(Errno::EINVAL);
    }
    let key = &cri.cri_key[..klen];
    let mut ictx = Box::new(AuthCtx::None);
    let mut octx = Box::new(AuthCtx::None);
    let mut pad = [0u8; HMAC_MAX_BLOCK_LEN];

    for k in 0..klen {
        pad[k] = key[k] ^ HMAC_IPAD_VAL;
    }

    (axf.Init)(&mut ictx);
    (axf.Update)(&mut ictx, &pad[..klen])?;
    (axf.Update)(&mut ictx, &hmac_ipad_buffer[..blocksize - klen])?;

    for k in 0..klen {
        pad[k] = key[k] ^ HMAC_OPAD_VAL;
    }

    (axf.Init)(&mut octx);
    (axf.Update)(&mut octx, &pad[..klen])?;
    (axf.Update)(&mut octx, &hmac_opad_buffer[..blocksize - klen])?;

    explicit_bzero(&mut pad);
    swd.SWCR_UN = SwcrUn::Auth(SwcrAuth {
        sw_ictx: Some(ictx),
        sw_octx: Some(octx),
        sw_klen: 0,
        sw_axf: axf,
    });
    Ok(())
}

/// `authenccommon`: the context of a combined-mode authenticator, keyed.
fn swcr_authencommon(
    swd: &mut SwcrData,
    cri: &Cryptoini<'_>,
    axf: &'static AuthHash,
) -> Result<(), Errno> {
    let klen = usize::try_from(cri.cri_klen / 8).map_err(|_| Errno::EINVAL)?;
    if klen > cri.cri_key.len() {
        return Err(Errno::EINVAL);
    }
    let mut ictx = Box::new(AuthCtx::None);

    (axf.Init)(&mut ictx);
    if let Some(setkey) = axf.Setkey {
        setkey(&mut ictx, &cri.cri_key[..klen]).map_err(|_| Errno::EINVAL)?;
    }
    swd.SWCR_UN = SwcrUn::Auth(SwcrAuth {
        sw_ictx: Some(ictx),
        sw_octx: None,
        sw_klen: 0,
        sw_axf: axf,
    });
    Ok(())
}

/// `swcr_freesession`: free a session.
pub fn swcr_freesession(tid: u64) -> Result<(), Errno> {
    let sid = (tid & 0xffffffff) as usize;

    with_sessions(|sessions| {
        if sid >= sessions.len() || sessions[sid].is_empty() {
            return Err(Errno::EINVAL);
        }

        // Silently accept and return
        if sid == 0 {
            return Ok(());
        }

        // Dropping the entries wipes their contexts.
        drop(core::mem::take(&mut sessions[sid]));
        Ok(())
    })
}

/// `swcr_process`: process a software request.
pub fn swcr_process(crp: &mut Cryptop<'_>) -> Result<(), Errno> {
    kassert!(crp.crp_ndesc >= 1);

    if matches!(crp.crp_buf, CryptoBuf::None) {
        return Err(Errno::EINVAL);
    }

    let lid = (crp.crp_sid & 0xffffffff) as usize;

    if !buf_matches_flags(crp) {
        return Err(Errno::EINVAL);
    }

    with_sessions(|sessions| {
        if lid >= sessions.len() || lid == 0 || sessions[lid].is_empty() {
            return Err(Errno::ENOENT);
        }

        // Go through crypto descriptors, processing as we go
        let session = &mut sessions[lid];
        for i in 0..(crp.crp_ndesc as usize).min(crp.crp_desc.len()) {
            let crd = crp.crp_desc[i];

            // Find the crypto context.
            //
            // XXX Note that the logic here prevents us from having XXX the same algorithm
            // multiple times in a session XXX (or rather, we can but it won't give us the
            // right XXX results). To do that, we'd need some way of differentiating XXX
            // between the various instances of an algorithm (so we can XXX locate the correct
            // crypto context).
            let Some(si) = session
                .iter()
                .position(|sw| sw.sw_alg == crd.CRD_INI.cri_alg)
            else {
                // No such context ?
                return Err(Errno::EINVAL);
            };

            match session[si].sw_alg {
                CRYPTO_NULL => {}
                CRYPTO_3DES_CBC | CRYPTO_BLF_CBC | CRYPTO_CAST_CBC | CRYPTO_AES_CBC
                | CRYPTO_AES_CTR | CRYPTO_AES_XTS => {
                    swcr_encdec(&crd, &mut session[si], &mut crp.crp_buf)?;
                }
                CRYPTO_MD5_HMAC
                | CRYPTO_SHA1_HMAC
                | CRYPTO_RIPEMD160_HMAC
                | CRYPTO_SHA2_256_HMAC
                | CRYPTO_SHA2_384_HMAC
                | CRYPTO_SHA2_512_HMAC => {
                    swcr_authcompute(&mut crp.crp_mac, &crd, &session[si], &mut crp.crp_buf)?;
                }

                CRYPTO_AES_GCM_16
                | CRYPTO_AES_GMAC
                | CRYPTO_AES_128_GMAC
                | CRYPTO_AES_192_GMAC
                | CRYPTO_AES_256_GMAC
                | CRYPTO_CHACHA20_POLY1305
                | CRYPTO_CHACHA20_POLY1305_MAC => {
                    return swcr_authenc(crp, session);
                }

                CRYPTO_DEFLATE_COMP => {
                    swcr_compdec(&crd, &mut session[si], &mut crp.crp_buf)?;
                    if let SwcrUn::Comp(c) = &session[si].SWCR_UN {
                        crp.crp_olen = c.sw_size as i32;
                    }
                }

                _ => {
                    // Unknown/unsupported algorithm
                    return Err(Errno::EINVAL);
                }
            }
        }
        Ok(())
    })
}

/// `swcr_init`: initialize the driver, called from the kernel main().
pub fn swcr_init() {
    let flags = CRYPTOCAP_F_SOFTWARE;
    let mut algs = [0i32; CRYPTO_ALGORITHM_MAX + 1];

    let Ok(id) = crypto_get_driverid(flags) else {
        // This should never happen
        panic(format_args!("Software crypto device cannot initialize!"));
    };
    swcr_id.store(id as i32, Ordering::Relaxed);

    for alg in [
        CRYPTO_3DES_CBC,
        CRYPTO_BLF_CBC,
        CRYPTO_CAST_CBC,
        CRYPTO_MD5_HMAC,
        CRYPTO_SHA1_HMAC,
        CRYPTO_RIPEMD160_HMAC,
        CRYPTO_AES_CBC,
        CRYPTO_AES_CTR,
        CRYPTO_AES_XTS,
        CRYPTO_AES_GCM_16,
        CRYPTO_AES_GMAC,
        CRYPTO_DEFLATE_COMP,
        CRYPTO_NULL,
        CRYPTO_SHA2_256_HMAC,
        CRYPTO_SHA2_384_HMAC,
        CRYPTO_SHA2_512_HMAC,
        CRYPTO_AES_128_GMAC,
        CRYPTO_AES_192_GMAC,
        CRYPTO_AES_256_GMAC,
        CRYPTO_CHACHA20_POLY1305,
        CRYPTO_CHACHA20_POLY1305_MAC,
        CRYPTO_ESN,
    ] {
        algs[alg as usize] = CRYPTO_ALG_FLAG_SUPPORTED;
    }

    if crypto_register(id, &algs, swcr_newsession, swcr_freesession, swcr_process).is_err() {
        panic(format_args!("Software crypto device cannot register!"));
    }
}

/// Forgets every software session: the host tests start from an empty driver.
#[cfg(test)]
pub(crate) fn swcr_reset() {
    with_sessions(|s| s.clear());
    swcr_id.store(-1, Ordering::Relaxed);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests of the software crypto driver through the framework (`crypto_newsession` and
    // `crypto_invoke`) with known answers: AES-CBC (NIST SP 800-38A), AES-CTR (RFC 3686), AES-XTS
    // (IEEE 1619), the HMACs, AES-GCM (the GCM specification's test case 4 in the layout of an ESP
    // packet), ChaCha20-Poly1305 (RFC 8439 section 2.8.2), 3DES, Blowfish and CAST-128 in CBC
    // mode (`openssl enc`), over buffers that are one iovec, several iovecs and chains of mbufs
    // with blocks straddling the segments.

    use core::ffi::c_void;
    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq, vec};

    use super::*;
    use crate::crypto::crypto::crypto_freesession;
    use crate::crypto::crypto::{
        crypto_freereq, crypto_getreq, crypto_invoke, crypto_newsession, crypto_reset,
    };
    use crate::crypto::cryptodev::CRYPTO_F_IOV;
    use crate::crypto::testutil::hex;
    use crate::kern::uipc_mbuf::tests::setup as mbuf_setup;
    use crate::kern::uipc_mbuf::{m_copydata as mbuf_copydata, m_get, m_gethdr};
    use crate::sys::mbuf::{M_DONTWAIT, MT_DATA, Mbuf, mclgetl};
    use crate::sys::uio::{Iovec, UioRw, UioSeg};

    /// The framework's state is global: one test at a time, each from an empty driver table with
    /// the software driver registered.
    fn fw() -> MutexGuard<'static, ()> {
        let g = crate::crypto::testutil::serial();
        crypto_reset();
        swcr_reset();
        swcr_init();
        g
    }

    fn ini<'a>(alg: i32, key: &'a [u8], next: Option<&'a Cryptoini<'a>>) -> Cryptoini<'a> {
        Cryptoini {
            cri_alg: alg,
            cri_klen: key.len() as i32 * 8,
            cri_key: key,
            cri_next: next,
            ..Cryptoini::default()
        }
    }

    fn desc(alg: i32, skip: i32, len: i32, inject: i32, flags: i32) -> Cryptodesc<'static> {
        Cryptodesc {
            crd_skip: skip,
            crd_len: len,
            crd_inject: inject,
            crd_flags: flags,
            CRD_INI: Cryptoini {
                cri_alg: alg,
                ..Cryptoini::default()
            },
        }
    }

    fn with_iv(mut d: Cryptodesc<'static>, iv: &[u8]) -> Cryptodesc<'static> {
        d.crd_iv_mut()[..iv.len()].copy_from_slice(iv);
        d
    }

    /// Runs the request over `buf` split into iovecs of the given lengths.
    fn run_iov(
        sid: u64,
        buf: &mut [u8],
        cuts: &[usize],
        descs: &[Cryptodesc<'static>],
        mac: Option<&mut [u8]>,
    ) -> Result<(), Errno> {
        let len = buf.len();
        let base = buf.as_mut_ptr();
        let mut iov: Vec<Iovec> = Vec::new();
        let mut off = 0;
        for &c in cuts {
            iov.push(Iovec {
                iov_base: base.wrapping_add(off).cast::<c_void>(),
                iov_len: c,
            });
            off += c;
        }
        assert_eq!(off, len, "the cuts cover the buffer");
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: len,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_WRITE,
            uio_procp: None,
        };
        let mut crp = crypto_getreq(descs.len() as i32).expect("a request");
        crp.crp_desc.copy_from_slice(descs);
        crp.crp_sid = sid;
        crp.crp_ilen = len as i32;
        crp.crp_flags = CRYPTO_F_IOV;
        crp.crp_buf = CryptoBuf::Iov(&mut uio);
        crp.crp_mac = mac;
        let r = crypto_invoke(&mut crp);
        crypto_freereq(Some(crp));
        r
    }

    /// An mbuf chain with `data` split over segments of the given lengths (and room to grow).
    fn chain(data: &[u8], cuts: &[usize]) -> &'static Mbuf {
        let mut top: Option<&'static Mbuf> = None;
        let mut last: Option<&'static Mbuf> = None;
        let mut off = 0;
        for &c in cuts {
            let m = if top.is_none() {
                m_gethdr(M_DONTWAIT, MT_DATA).expect("a header mbuf")
            } else {
                m_get(M_DONTWAIT, MT_DATA).expect("an mbuf")
            };
            if c > crate::kern::uipc_mbuf::m_trailingspace(m) as usize {
                assert!(mclgetl(m, M_DONTWAIT, c as u32).is_some());
            }
            m.m_len().set(c as u32);
            crate::kern::uipc_mbuf::m_copyback(m, 0, &data[off..off + c], M_DONTWAIT)
                .expect("copyback");
            off += c;
            match last {
                None => top = Some(m),
                Some(l) => l.m_next().set(Some(m)),
            }
            last = Some(m);
        }
        assert_eq!(off, data.len());
        top.expect("a chain")
    }

    fn chain_bytes(m: &Mbuf) -> Vec<u8> {
        let mut n = 0;
        let mut p = Some(m);
        while let Some(x) = p {
            n += x.m_len().get() as usize;
            p = x.m_next().get();
        }
        let mut v = vec![0u8; n];
        mbuf_copydata(m, 0, &mut v);
        v
    }

    /// Runs the request over the mbuf chain `m`.
    fn run_mbuf(
        sid: u64,
        m: &'static Mbuf,
        len: usize,
        descs: &[Cryptodesc<'static>],
    ) -> Result<(), Errno> {
        let mut crp = crypto_getreq(descs.len() as i32).expect("a request");
        crp.crp_desc.copy_from_slice(descs);
        crp.crp_sid = sid;
        crp.crp_ilen = len as i32;
        crp.crp_flags = CRYPTO_F_IMBUF;
        crp.crp_buf = CryptoBuf::Mbuf(m);
        let r = crypto_invoke(&mut crp);
        crypto_freereq(Some(crp));
        r
    }

    fn pt48() -> Vec<u8> {
        (0..48).map(|i| ((i * 7 + 3) % 256) as u8).collect()
    }

    const ENC_EXPLICIT: i32 = CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT | CRD_F_IV_PRESENT;
    const DEC_EXPLICIT: i32 = CRD_F_IV_EXPLICIT;

    /// CBC known answers: `(algorithm, key, ciphertext of pt48 under IV 00..)`.
    fn cbc_cases() -> Vec<(i32, Vec<u8>, usize, Vec<u8>)> {
        let k16: Vec<u8> = (0..16).map(|i| ((i * 3 + 1) % 256) as u8).collect();
        let aes = |n: usize| -> Vec<u8> { (0..n).map(|i| ((i * 11 + 2) % 256) as u8).collect() };
        vec![
            (
                CRYPTO_3DES_CBC,
                hex("0123456789abcdef23456789abcdef01456789abcdef0123"),
                8,
                hex(
                    "c1397d01f9d38a1c41b1b50eee2fba9b723441959e72ad963bf9e2da15a5038f8ad1a388f5275b36d17c8ee71c8bac06",
                ),
            ),
            (
                CRYPTO_BLF_CBC,
                k16.clone(),
                8,
                hex(
                    "22b7a722c76dcffdc66dbfd14485b0a1e1057ffa301f38d94236fdf655bf70a417e675cfd291cde2f254b88d5fa46d81",
                ),
            ),
            (
                CRYPTO_CAST_CBC,
                k16,
                8,
                hex(
                    "9dc49d75637f30360174714cfae00e0337e008494367c6702b2df927950d05ba627abb6297818c3051746f0f68c4c3ec",
                ),
            ),
            (
                CRYPTO_AES_CBC,
                aes(16),
                16,
                hex(
                    "399373ba48aa7aad4eef24f0fa0695894e7eefaa20301c0554766e08ce2c5df58c335a96b08765f39303d49ae70aac7b",
                ),
            ),
            (
                CRYPTO_AES_CBC,
                aes(24),
                16,
                hex(
                    "29481826e36ac4c5173b0ddfe1dde381dfdac2ace9f9603903445a2731466063431c4a852c2d64d47f9efc5b5f5ab5e5",
                ),
            ),
            (
                CRYPTO_AES_CBC,
                aes(32),
                16,
                hex(
                    "e9e2eb941a89270076e2dba770763c262a89e5c16cffa117ff754e0523da217efe6c3a49d188fdfa75716e01be75e537",
                ),
            ),
        ]
    }

    #[test]
    fn cbc_ciphers_over_one_and_several_iovecs() {
        let _g = fw();
        for (alg, key, ivlen, want) in cbc_cases() {
            let c = ini(alg, &key, None);
            let sid = crypto_newsession(&c, 0).expect("a session");
            let iv: Vec<u8> = (0..ivlen as u8).collect();
            let enc = with_iv(desc(alg, 0, 48, 0, ENC_EXPLICIT), &iv);
            let dec = with_iv(desc(alg, 0, 48, 0, DEC_EXPLICIT), &iv);

            // Odd cuts put blocks across the iovecs.
            for cuts in [
                vec![48usize],
                vec![7, 25, 16],
                vec![1, 47],
                vec![20, 0, 28],
                vec![3, 3, 3, 39],
            ] {
                let mut buf = pt48();
                run_iov(sid, &mut buf, &cuts, &[enc], None).expect("encrypt");
                assert_eq!(buf, want, "alg {alg} key {} cuts {cuts:?}", key.len());
                run_iov(sid, &mut buf, &cuts, &[dec], None).expect("decrypt");
                assert_eq!(buf, pt48(), "alg {alg} back, cuts {cuts:?}");
            }
            crypto_freesession(sid).expect("free");
        }
    }

    #[test]
    fn cbc_over_mbuf_chains_with_straddling_blocks() {
        let _g = fw();
        let _m = mbuf_setup();
        for (alg, key, ivlen, want) in cbc_cases() {
            let c = ini(alg, &key, None);
            let sid = crypto_newsession(&c, 0).expect("a session");
            let iv: Vec<u8> = (0..ivlen as u8).collect();
            let enc = with_iv(desc(alg, 0, 48, 0, ENC_EXPLICIT), &iv);
            let dec = with_iv(desc(alg, 0, 48, 0, DEC_EXPLICIT), &iv);

            for cuts in [
                vec![48usize],
                vec![5, 16, 11, 16],
                vec![1, 47],
                vec![17, 17, 14],
                vec![16, 0, 16, 0, 16],
                vec![0, 48],
            ] {
                let m = chain(&pt48(), &cuts);
                run_mbuf(sid, m, 48, &[enc]).expect("encrypt");
                assert_eq!(chain_bytes(m), want, "alg {alg} cuts {cuts:?}");
                run_mbuf(sid, m, 48, &[dec]).expect("decrypt");
                assert_eq!(chain_bytes(m), pt48(), "alg {alg} back, cuts {cuts:?}");
                crate::kern::uipc_mbuf::m_freem(m);
            }
            crypto_freesession(sid).expect("free");
        }
    }

    #[test]
    fn skip_and_a_partial_range_leave_the_rest_alone() {
        let _g = fw();
        let key: Vec<u8> = (0..16).map(|i| ((i * 11 + 2) % 256) as u8).collect();
        let c = ini(CRYPTO_AES_CBC, &key, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let iv: Vec<u8> = (0..16u8).collect();
        let mut buf: Vec<u8> = (0..70).map(|i| (i as u8).wrapping_mul(3)).collect();
        let orig = buf.clone();
        // Encrypt bytes 10..42 (two blocks) only.
        let enc = with_iv(desc(CRYPTO_AES_CBC, 10, 32, 0, ENC_EXPLICIT), &iv);
        run_iov(sid, &mut buf, &[30, 40], &[enc], None).expect("encrypt");
        assert_eq!(buf[..10], orig[..10]);
        assert_eq!(buf[42..], orig[42..]);
        assert_ne!(buf[10..42], orig[10..42]);
        // A length that is not a multiple of the block is refused before anything is touched.
        let bad = with_iv(desc(CRYPTO_AES_CBC, 0, 20, 0, ENC_EXPLICIT), &iv);
        let mut b2 = orig.clone();
        assert_eq!(
            run_iov(sid, &mut b2, &[70], &[bad], None),
            Err(Errno::EINVAL)
        );
        assert_eq!(b2, orig);
    }

    #[test]
    fn the_iv_is_written_to_and_read_from_the_buffer() {
        let _g = fw();
        let key: Vec<u8> = (0..16).map(|i| ((i * 11 + 2) % 256) as u8).collect();
        let c = ini(CRYPTO_AES_CBC, &key, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let iv: Vec<u8> = (0..16u8).collect();
        // IV explicit but not present: it is injected before the data, which starts at 16.
        let mut buf = vec![0u8; 16 + 48];
        buf[16..].copy_from_slice(&pt48());
        let enc = with_iv(
            desc(CRYPTO_AES_CBC, 16, 48, 0, CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT),
            &iv,
        );
        run_iov(sid, &mut buf, &[64], &[enc], None).expect("encrypt");
        assert_eq!(buf[..16], iv[..]);
        assert_eq!(buf[16..], cbc_cases()[3].3[..]);
        // Decrypting without an explicit IV takes it from the buffer.
        let dec = desc(CRYPTO_AES_CBC, 16, 48, 0, 0);
        run_iov(sid, &mut buf, &[64], &[dec], None).expect("decrypt");
        assert_eq!(buf[16..], pt48()[..]);
    }

    #[test]
    fn aes_ctr_rfc3686() {
        let _g = fw();
        // Test vector 1: 16-byte key and nonce, one block.
        let mut key = hex("ae6852f8121067cc4bf7a5765577f39e");
        key.extend_from_slice(&hex("00000030"));
        let c = ini(CRYPTO_AES_CTR, &key, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let mut buf = b"Single block msg".to_vec();
        let d = with_iv(desc(CRYPTO_AES_CTR, 0, 16, 0, ENC_EXPLICIT), &[0; 8]);
        run_iov(sid, &mut buf, &[16], &[d], None).expect("encrypt");
        assert_eq!(buf, hex("e4095d4fb7a7b3792d6175a3261311b8"));
        // Test vector 2: two blocks; CTR is its own inverse.
        let mut key = hex("7e24067817fae0d743d6ce1f32539163");
        key.extend_from_slice(&hex("006cb6db"));
        let c = ini(CRYPTO_AES_CTR, &key, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let mut buf: Vec<u8> = (0..32).collect();
        let d = with_iv(
            desc(CRYPTO_AES_CTR, 0, 32, 0, ENC_EXPLICIT),
            &hex("c0543b59da48d90b"),
        );
        run_iov(sid, &mut buf, &[11, 21], &[d], None).expect("encrypt");
        assert_eq!(
            buf,
            hex("5104a106168a72d9790d41ee8edad388eb2e1efc46da57c8fce630df9141be28")
        );
        let d = with_iv(
            desc(CRYPTO_AES_CTR, 0, 32, 0, DEC_EXPLICIT),
            &hex("c0543b59da48d90b"),
        );
        run_iov(sid, &mut buf, &[32], &[d], None).expect("decrypt");
        assert_eq!(buf, (0..32).collect::<Vec<u8>>());
        // A 256-bit key and 96 of 100 bytes (the length must be a multiple of the block).
        let mut key: Vec<u8> = (0..32).collect();
        key.extend_from_slice(&hex("01020304"));
        let c = ini(CRYPTO_AES_CTR, &key, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let data: Vec<u8> = (0..100).map(|i| ((i * 5 + 1) % 256) as u8).collect();
        let want = hex(
            "da560817d9101eebd67de41557ac33a0e7f818c7817c306a347cb1fee5190d41d3533d079b2397d6d61bd7248ac8c3562f6210bab010f17c1dfd4a56a1f5229d27b7c8476d7ea8333d37bd993d6f548c881c06c88fd34aa4d1805defc92eff66",
        );
        let mut buf = data[..96].to_vec();
        let d = with_iv(
            desc(CRYPTO_AES_CTR, 0, 96, 0, ENC_EXPLICIT),
            &hex("1112131415161718"),
        );
        run_iov(sid, &mut buf, &[96], &[d], None).expect("encrypt");
        assert_eq!(buf, want);
        let mut bad = data.clone();
        let d = with_iv(
            desc(CRYPTO_AES_CTR, 0, 100, 0, ENC_EXPLICIT),
            &hex("1112131415161718"),
        );
        assert_eq!(
            run_iov(sid, &mut bad, &[100], &[d], None),
            Err(Errno::EINVAL)
        );
    }

    #[test]
    fn aes_xts_ieee_1619() {
        let _g = fw();
        // Vector 1: zero keys, sector 0, 32 zero bytes.
        let c = ini(CRYPTO_AES_XTS, &[0; 32], None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let mut buf = vec![0u8; 32];
        let d = with_iv(desc(CRYPTO_AES_XTS, 0, 32, 0, ENC_EXPLICIT), &[0; 8]);
        run_iov(sid, &mut buf, &[32], &[d], None).expect("encrypt");
        assert_eq!(
            buf,
            hex("917cf69ebd68b2ec9b9fe9a3eadda692cd43d2f59598ed858c02c2652fbf922e")
        );
        let d = with_iv(desc(CRYPTO_AES_XTS, 0, 32, 0, DEC_EXPLICIT), &[0; 8]);
        run_iov(sid, &mut buf, &[32], &[d], None).expect("decrypt");
        assert_eq!(buf, vec![0u8; 32]);

        // The other key sizes and sectors against an independent XTS built on openssl's ECB.
        let pt: Vec<u8> = (0..64).map(|i| ((i * 3 + 1) % 256) as u8).collect();
        let k32: Vec<u8> = (0..32).map(|i| ((i * 13 + 5) % 256) as u8).collect();
        let k64: Vec<u8> = (0..64).map(|i| ((i * 13 + 5) % 256) as u8).collect();
        let cases = [
            (
                k32,
                0x1234u64,
                "74a72892904edcc17d28d41ddb175bb51febe143bfa6661c779b3ca05587fb679409eadc94e0769b62df63aed5bdff4cb1ea7493bf3865d8d32639bf7a8824d0",
            ),
            (
                k64,
                7,
                "cc6d52ebeb292ad3824b0e33e4a89f1b9925551c4347549fbe9ef6d3aac71b217c5bffe04ebdd79528b7ff82d745967955ae8c7b04de05794c21d0554e817661",
            ),
        ];
        for (key, sector, want) in cases {
            let c = ini(CRYPTO_AES_XTS, &key, None);
            let sid = crypto_newsession(&c, 0).expect("a session");
            let mut buf = pt.clone();
            let d = with_iv(
                desc(CRYPTO_AES_XTS, 0, 64, 0, ENC_EXPLICIT),
                &sector.to_le_bytes(),
            );
            run_iov(sid, &mut buf, &[9, 55], &[d], None).expect("encrypt");
            assert_eq!(buf, hex(want), "key {}", key.len());
        }
        // A key that is not 32 or 64 bytes is refused when the session is made.
        let c = ini(CRYPTO_AES_XTS, &[0; 48], None);
        assert_eq!(crypto_newsession(&c, 0), Err(Errno::EINVAL));
    }

    #[test]
    fn null_transform_leaves_the_data() {
        let _g = fw();
        let c = ini(CRYPTO_NULL, &[], None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let mut buf = pt48();
        let d = with_iv(desc(CRYPTO_NULL, 0, 48, 0, ENC_EXPLICIT), &[0; 4]);
        run_iov(sid, &mut buf, &[48], &[d], None).expect("process");
        assert_eq!(buf, pt48());
    }

    /// `(algorithm, MAC of 77 bytes under a 24-byte key, bytes on the wire)`.
    fn hmac_cases() -> Vec<(i32, &'static str, usize)> {
        vec![
            (CRYPTO_MD5_HMAC, "686a920987b128c29f2168fff2f292e4", 12),
            (
                CRYPTO_SHA1_HMAC,
                "566ab0b17c5b5e6ba9383fb40f66129075aaebde",
                12,
            ),
            (
                CRYPTO_RIPEMD160_HMAC,
                "f108a58c711e47cc18cd396761dc61e0e7f8830c",
                12,
            ),
            (
                CRYPTO_SHA2_256_HMAC,
                "273b13b4dba37652f2689dddb7b80f681ca6c77efb33bae7eab7f54e16a2d385",
                16,
            ),
            (
                CRYPTO_SHA2_384_HMAC,
                "544afb91fb4d2b8e08351dc4778f26e0373349e6e3d49d2d13c2a30a74217b532b3e702fa522be4deeded334525b5ac2",
                24,
            ),
            (
                CRYPTO_SHA2_512_HMAC,
                "db8ef2a017e6f2d7fd3385ad28224c68edef96d329df08f73f08c18d103c078fed91e2bd1b54718e583de1190c7c5c77d3b648effb8e2154d9634317e85f4fae",
                32,
            ),
        ]
    }

    fn hmac_msg() -> Vec<u8> {
        (0..77).map(|i| ((i * 9 + 4) % 256) as u8).collect()
    }

    fn hmac_key() -> Vec<u8> {
        (0..24).map(|i| ((i * 5 + 7) % 256) as u8).collect()
    }

    #[test]
    fn hmacs_into_the_mac_slot_of_an_iovec_request() {
        let _g = fw();
        for (alg, want, authsize) in hmac_cases() {
            let key = hmac_key();
            let c = ini(alg, &key, None);
            let sid = crypto_newsession(&c, 0).expect("a session");
            let mut buf = hmac_msg();
            let mut mac = vec![0xeeu8; 40];
            // Authenticate bytes 0..77, in three iovecs.
            let d = desc(alg, 0, 77, 0, 0);
            run_iov(sid, &mut buf, &[10, 40, 27], &[d], Some(&mut mac)).expect("hmac");
            assert_eq!(buf, hmac_msg(), "the data is untouched");
            assert_eq!(mac[..authsize], hex(want)[..authsize], "alg {alg}");
            assert!(
                mac[authsize..].iter().all(|b| *b == 0xee),
                "nothing past authsize"
            );
            // A skip: authenticating a suffix equals authenticating it alone.
            let mut data = vec![0xaau8; 5];
            data.extend_from_slice(&hmac_msg());
            let d = desc(alg, 5, 77, 0, 0);
            let mut mac2 = vec![0u8; 40];
            run_iov(sid, &mut data, &[30, 52], &[d], Some(&mut mac2)).expect("hmac");
            assert_eq!(mac2[..authsize], mac[..authsize]);
        }
    }

    #[test]
    fn hmacs_injected_into_an_mbuf_chain() {
        let _g = fw();
        let _m = mbuf_setup();
        for (alg, want, authsize) in hmac_cases() {
            let key = hmac_key();
            let c = ini(alg, &key, None);
            let sid = crypto_newsession(&c, 0).expect("a session");
            let m = chain(&hmac_msg(), &[13, 30, 34]);
            // Inject at the end: the chain grows by the MAC.
            let d = desc(alg, 0, 77, 77, 0);
            run_mbuf(sid, m, 77, &[d]).expect("hmac");
            let bytes = chain_bytes(m);
            assert_eq!(bytes.len(), 77 + authsize, "alg {alg}");
            assert_eq!(bytes[..77], hmac_msg()[..]);
            assert_eq!(bytes[77..], hex(want)[..authsize], "alg {alg}");
            crate::kern::uipc_mbuf::m_freem(m);
        }
    }

    #[test]
    fn hmac_keys_of_up_to_a_block_and_not_more() {
        let _g = fw();
        // MD5's block is 64 bytes: a 64-byte key is used as is.
        let key64: Vec<u8> = (0..64).map(|i| ((i * 5 + 7) % 256) as u8).collect();
        let c = ini(CRYPTO_MD5_HMAC, &key64, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let mut buf = hmac_msg();
        let mut mac = [0u8; 16];
        run_iov(
            sid,
            &mut buf,
            &[77],
            &[desc(CRYPTO_MD5_HMAC, 0, 77, 0, 0)],
            Some(&mut mac),
        )
        .expect("hmac");
        assert_eq!(mac[..12], hex("4ba61595f9dde4746b3bf94d9a2dff16")[..12]);
        // SHA-512's is 128.
        let key128: Vec<u8> = (0..128).map(|i| ((i * 5 + 7) % 256) as u8).collect();
        let c = ini(CRYPTO_SHA2_512_HMAC, &key128, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let mut mac = [0u8; 32];
        run_iov(
            sid,
            &mut buf,
            &[77],
            &[desc(CRYPTO_SHA2_512_HMAC, 0, 77, 0, 0)],
            Some(&mut mac),
        )
        .expect("hmac");
        assert_eq!(
            mac[..],
            hex("2ea55a48adee55b97bbc0ad77b5f27f991a9057207918d7d5ddda0685a365c34")[..]
        );
        // Longer than the block (the C computes a negative pad length): refused.
        let c = ini(CRYPTO_MD5_HMAC, &[1u8; 65], None);
        assert_eq!(crypto_newsession(&c, 0), Err(Errno::EINVAL));
        // The caller's key is not modified (the C XORs it in place and back).
        let key = hmac_key();
        let c = ini(CRYPTO_SHA1_HMAC, &key, None);
        crypto_newsession(&c, 0).expect("a session");
        assert_eq!(key, hmac_key());
    }

    #[test]
    fn esn_is_authenticated_after_the_data() {
        let _g = fw();
        // With CRD_F_ESN the 4 high bytes of the sequence number go in after the data: the MAC is
        // that of data || esn.
        let key = hmac_key();
        let c = ini(CRYPTO_SHA1_HMAC, &key, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let mut with_esn = hmac_msg();
        let mut mac_a = [0u8; 12];
        let mut d = desc(CRYPTO_SHA1_HMAC, 0, 77, 0, CRD_F_ESN);
        d.set_crd_esn([1, 2, 3, 4]);
        run_iov(sid, &mut with_esn, &[77], &[d], Some(&mut mac_a)).expect("hmac");
        // The same MAC over the 81 bytes, directly.
        let mut data = hmac_msg();
        data.extend_from_slice(&[1, 2, 3, 4]);
        let mut mac_b = [0u8; 12];
        run_iov(
            sid,
            &mut data,
            &[81],
            &[desc(CRYPTO_SHA1_HMAC, 0, 81, 0, 0)],
            Some(&mut mac_b),
        )
        .expect("hmac");
        assert_eq!(mac_a, mac_b);
    }

    const GCM_KEY: &str = "feffe9928665731c6d6a8f9467308308";
    const GCM_SALT: &str = "cafebabe";
    const GCM_IV: &str = "facedbaddecaf888";
    const GCM_AAD: &str = "feedfacedeadbeeffeedfacedeadbeefabaddad2";
    const GCM_P: &str = "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b391aafd255";
    const GCM_C: &str = "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091";
    const GCM_TAG: &str = "5bc94fbc3221a5db94fae95ae7121a47";

    /// A packet in the layout of ESP with an AEAD: `AAD (20) | IV (8) | payload (60) | tag (16)`.
    fn gcm_packet(payload: &[u8]) -> Vec<u8> {
        let mut p = hex(GCM_AAD);
        p.extend_from_slice(&[0; 8]);
        p.extend_from_slice(payload);
        p.extend_from_slice(&[0; 16]);
        p
    }

    #[test]
    fn aes_gcm_the_gcm_specification_test_case_4_in_an_esp_layout() {
        let _g = fw();
        let mut material = hex(GCM_KEY);
        material.extend_from_slice(&hex(GCM_SALT));
        let gmac = ini(CRYPTO_AES_128_GMAC, &material, None);
        let c = ini(CRYPTO_AES_GCM_16, &material, Some(&gmac));
        let sid = crypto_newsession(&c, 0).expect("a session");

        // Encrypt: the IV is injected before the payload; the tag of an iovec request goes to
        // `crp_mac` (an mbuf request gets it at `crd_inject`).
        let enc = with_iv(
            desc(
                CRYPTO_AES_GCM_16,
                28,
                60,
                20,
                CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT,
            ),
            &hex(GCM_IV),
        );
        let auth = desc(CRYPTO_AES_128_GMAC, 0, 20, 88, 0);
        let mut pkt = gcm_packet(&hex(GCM_P)[..60]);
        let mut tag = [0u8; 16];
        run_iov(sid, &mut pkt, &[104], &[auth, enc], Some(&mut tag)).expect("encrypt");
        assert_eq!(pkt[20..28], hex(GCM_IV)[..], "the IV was written");
        assert_eq!(pkt[28..88], hex(GCM_C)[..]);
        assert_eq!(tag.to_vec(), hex(GCM_TAG));
        // Over several iovecs, the descriptors in the other order.
        let mut pkt = gcm_packet(&hex(GCM_P)[..60]);
        let mut tag = [0u8; 16];
        run_iov(sid, &mut pkt, &[50, 54], &[enc, auth], Some(&mut tag)).expect("encrypt");
        assert_eq!(pkt[28..88], hex(GCM_C)[..]);
        assert_eq!(tag.to_vec(), hex(GCM_TAG));
        // No MAC slot, no tag.
        let mut pkt = gcm_packet(&hex(GCM_P)[..60]);
        assert_eq!(
            run_iov(sid, &mut pkt, &[104], &[auth, enc], None),
            Err(Errno::EINVAL)
        );

        // Decrypt: the tag is computed over the ciphertext (the caller compares it), the payload
        // comes out as plaintext; the IV is taken from the packet.
        let mut pkt = gcm_packet(&hex(GCM_C));
        pkt[20..28].copy_from_slice(&hex(GCM_IV));
        let dec = desc(CRYPTO_AES_GCM_16, 28, 60, 20, 0);
        let mut tag = [0u8; 16];
        run_iov(sid, &mut pkt, &[104], &[auth, dec], Some(&mut tag)).expect("decrypt");
        assert_eq!(pkt[28..88], hex(GCM_P)[..60]);
        assert_eq!(tag.to_vec(), hex(GCM_TAG));
    }

    #[test]
    fn aes_gcm_over_an_mbuf_chain() {
        let _g = fw();
        let _m = mbuf_setup();
        let mut material = hex(GCM_KEY);
        material.extend_from_slice(&hex(GCM_SALT));
        let gmac = ini(CRYPTO_AES_128_GMAC, &material, None);
        let c = ini(CRYPTO_AES_GCM_16, &material, Some(&gmac));
        let sid = crypto_newsession(&c, 0).expect("a session");
        let enc = with_iv(
            desc(
                CRYPTO_AES_GCM_16,
                28,
                60,
                20,
                CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT,
            ),
            &hex(GCM_IV),
        );
        let auth = desc(CRYPTO_AES_128_GMAC, 0, 20, 88, 0);
        let pkt = gcm_packet(&hex(GCM_P)[..60]);
        let m = chain(&pkt, &[11, 25, 9, 59]);
        run_mbuf(sid, m, 104, &[auth, enc]).expect("encrypt");
        let out = chain_bytes(m);
        assert_eq!(out[28..88], hex(GCM_C)[..]);
        assert_eq!(out[88..], hex(GCM_TAG)[..]);
        crate::kern::uipc_mbuf::m_freem(m);
    }

    #[test]
    fn gmac_only_with_the_esp_gmac_pair_and_the_esn_kludge() {
        let _g = fw();
        // ESP with AES-GMAC: the "encryption" descriptor has length 0 and the authenticator covers
        // the whole packet; the tag is that of an AAD-only GCM (RFC 4543).
        let mut material = hex(GCM_KEY);
        material.extend_from_slice(&hex(GCM_SALT));
        let gmac = ini(CRYPTO_AES_128_GMAC, &material, None);
        let c = ini(CRYPTO_AES_GMAC, &material, Some(&gmac));
        let sid = crypto_newsession(&c, 0).expect("a session");
        // Packet: AAD (20) | IV (8) | tag; the hashed data is the 20 bytes (the IV is a nonce
        // that is not authenticated as data).
        let mut pkt = hex(GCM_AAD);
        pkt.extend_from_slice(&[0; 8]);
        pkt.extend_from_slice(&[0; 16]);
        let enc = with_iv(
            desc(
                CRYPTO_AES_GMAC,
                28,
                0,
                20,
                CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT,
            ),
            &hex(GCM_IV),
        );
        let auth = desc(CRYPTO_AES_128_GMAC, 0, 20, 28, 0);
        let mut tag = [0u8; 16];
        run_iov(sid, &mut pkt, &[44], &[auth, enc], Some(&mut tag)).expect("gmac");
        assert_eq!(pkt[20..28], hex(GCM_IV)[..]);
        assert_eq!(pkt[..20], hex(GCM_AAD)[..]);
        assert_eq!(tag.to_vec(), hex("346434fd51d5cd0c5887ec63e39b907a"));
    }

    #[test]
    fn chacha20_poly1305_rfc8439_2_8_2_in_an_esp_layout() {
        let _g = fw();
        let mut material: Vec<u8> = (0..32).map(|i| 0x80 + i as u8).collect();
        material.extend_from_slice(&hex("07000000"));
        let mac = ini(CRYPTO_CHACHA20_POLY1305_MAC, &material, None);
        let c = ini(CRYPTO_CHACHA20_POLY1305, &material, Some(&mac));
        let sid = crypto_newsession(&c, 0).expect("a session");

        let aad = hex("50515253c0c1c2c3c4c5c6c7");
        let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let want = hex(
            "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b6116",
        );
        // AAD (12) | IV (8) | payload (114) | tag (16)
        let mut pkt = aad.clone();
        pkt.extend_from_slice(&[0; 8]);
        pkt.extend_from_slice(pt);
        pkt.extend_from_slice(&[0; 16]);
        let enc = with_iv(
            desc(
                CRYPTO_CHACHA20_POLY1305,
                20,
                114,
                12,
                CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT,
            ),
            &hex("4041424344454647"),
        );
        let auth = desc(CRYPTO_CHACHA20_POLY1305_MAC, 0, 12, 134, 0);
        let mut tag = [0u8; 16];
        let total = pkt.len();
        run_iov(
            sid,
            &mut pkt,
            &[40, total - 40],
            &[auth, enc],
            Some(&mut tag),
        )
        .expect("encrypt");
        assert_eq!(pkt[20..134], want[..114]);
        assert_eq!(tag.to_vec(), hex("1ae10b594f09e26a7e902ecbd0600691"));

        // And back.
        let dec = desc(CRYPTO_CHACHA20_POLY1305, 20, 114, 12, 0);
        let mut tag2 = [0u8; 16];
        run_iov(sid, &mut pkt, &[total], &[auth, dec], Some(&mut tag2)).expect("decrypt");
        assert_eq!(pkt[20..134], pt[..]);
        assert_eq!(tag2, tag);
    }

    #[test]
    fn session_errors() {
        let _g = fw();
        // An algorithm no driver supports.
        let c = ini(99, &[0; 16], None);
        assert_eq!(crypto_newsession(&c, 0), Err(Errno::EINVAL));
        // Bad key sizes.
        let c = ini(CRYPTO_AES_CBC, &[0; 17], None);
        assert_eq!(crypto_newsession(&c, 0), Err(Errno::EINVAL));
        let c = ini(CRYPTO_3DES_CBC, &[0; 16], None);
        assert_eq!(crypto_newsession(&c, 0), Err(Errno::EINVAL));
        let c = ini(CRYPTO_AES_128_GMAC, &[0; 21], None);
        assert_eq!(crypto_newsession(&c, 0), Err(Errno::EINVAL));
        // A failed session does not use up a slot.
        assert_eq!(swcr_sesnum() as usize, CRYPTO_SW_SESSIONS);
        let used = with_sessions(|s| s.iter().filter(|l| !l.is_empty()).count());
        assert_eq!(used, 0);

        // A request for a session that was freed or never made.
        let key = [7u8; 16];
        let c = ini(CRYPTO_AES_CBC, &key, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        crypto_freesession(sid).expect("free");
        let mut buf = vec![0u8; 16];
        let d = with_iv(desc(CRYPTO_AES_CBC, 0, 16, 0, ENC_EXPLICIT), &[0; 16]);
        assert_eq!(
            run_iov(sid, &mut buf, &[16], &[d], None),
            Err(Errno::ENOENT)
        );
        assert_eq!(crypto_freesession(sid), Err(Errno::EINVAL));
        // A descriptor whose algorithm is not in the session.
        let sid = crypto_newsession(&c, 0).expect("a session");
        let d = with_iv(desc(CRYPTO_BLF_CBC, 0, 16, 0, ENC_EXPLICIT), &[0; 8]);
        assert_eq!(
            run_iov(sid, &mut buf, &[16], &[d], None),
            Err(Errno::EINVAL)
        );
        // No buffer.
        let mut crp = crypto_getreq(1).expect("a request");
        crp.crp_sid = sid;
        crp.crp_desc[0] = d;
        assert_eq!(crypto_invoke(&mut crp), Err(Errno::EINVAL));
        // A buffer that is not the kind the flags say.
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: 16,
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: 16,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_WRITE,
            uio_procp: None,
        };
        let mut crp = crypto_getreq(1).expect("a request");
        crp.crp_sid = sid;
        crp.crp_desc[0] = d;
        crp.crp_flags = CRYPTO_F_IMBUF;
        crp.crp_buf = CryptoBuf::Iov(&mut uio);
        assert_eq!(crypto_invoke(&mut crp), Err(Errno::EINVAL));
    }

    #[test]
    fn sessions_are_reused_and_the_table_grows() {
        let _g = fw();
        let key = [3u8; 16];
        let c = ini(CRYPTO_AES_CBC, &key, None);
        let mut sids = Vec::new();
        // 32 slots with 0 unused: the 32nd session makes the table double.
        for _ in 0..40 {
            sids.push(crypto_newsession(&c, 0).expect("a session"));
        }
        assert_eq!(swcr_sesnum(), 2 * CRYPTO_SW_SESSIONS as u32);
        let numbers: Vec<u32> = sids.iter().map(|s| (*s & 0xffff_ffff) as u32).collect();
        assert_eq!(numbers, (1..=40).collect::<Vec<u32>>());
        // Freed numbers are used again, lowest first.
        crypto_freesession(sids[4]).expect("free");
        crypto_freesession(sids[1]).expect("free");
        let again = crypto_newsession(&c, 0).expect("a session");
        assert_eq!(again & 0xffff_ffff, 2);
        let again = crypto_newsession(&c, 0).expect("a session");
        assert_eq!(again & 0xffff_ffff, 5);
    }

    #[test]
    fn free_wipes_the_contexts() {
        let _g = fw();
        let key = hmac_key();
        let c = ini(CRYPTO_SHA1_HMAC, &key, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let list = with_sessions(|s| core::mem::take(&mut s[(sid & 0xffff_ffff) as usize]));
        assert_eq!(list.len(), 1);
        let SwcrUn::Auth(a) = &list[0].SWCR_UN else {
            panic!("an authenticator")
        };
        let ictx = a.sw_ictx.as_deref().copied();
        assert!(matches!(ictx, Some(AuthCtx::Sha1(_))));
        drop(list);
        // The driver's own free of a taken slot reports the empty session.
        assert_eq!(swcr_freesession(sid), Err(Errno::EINVAL));
    }

    /// A 20-byte "header" and a compressible payload, as IPComp hands them to the driver.
    fn ipcomp_packet() -> (Vec<u8>, Vec<u8>) {
        let hdr: Vec<u8> = (0..20).collect();
        let payload = b"payload compressed by IPComp, ".repeat(20);
        (hdr, payload)
    }

    /// Runs one deflate descriptor over the mbuf chain `m`; returns `crp_olen`.
    fn run_deflate_mbuf(sid: u64, m: &'static Mbuf, d: Cryptodesc<'static>) -> Result<i32, Errno> {
        let mut crp = crypto_getreq(1).expect("a request");
        crp.crp_desc[0] = d;
        crp.crp_sid = sid;
        crp.crp_flags = CRYPTO_F_IMBUF;
        crp.crp_buf = CryptoBuf::Mbuf(m);
        let r = crypto_invoke(&mut crp).map(|()| crp.crp_olen);
        crypto_freereq(Some(crp));
        r
    }

    #[test]
    fn deflate_compresses_and_decompresses_an_mbuf_chain() {
        let _g = fw();
        let _m = mbuf_setup();
        let sid = crypto_newsession(&ini(CRYPTO_DEFLATE_COMP, &[], None), 0).unwrap();
        let (hdr, payload) = ipcomp_packet();
        let packet = [hdr.clone(), payload.clone()].concat();
        let m = chain(&packet, &[50, 400, packet.len() - 450]);
        let plen = payload.len() as i32;

        // Compression replaces the payload and trims the chain.
        let olen = run_deflate_mbuf(sid, m, desc(CRYPTO_DEFLATE_COMP, 20, plen, 20, CRD_F_COMP));
        let compressed = (comp_algo_deflate.compress)(&payload).unwrap();
        assert_eq!(olen, Ok(compressed.len() as i32));
        assert_eq!(chain_bytes(m), [hdr.clone(), compressed.clone()].concat());

        // Decompression restores it, growing the chain.
        let clen = compressed.len() as i32;
        let olen = run_deflate_mbuf(sid, m, desc(CRYPTO_DEFLATE_COMP, 20, clen, 20, 0));
        assert_eq!(olen, Ok(plen));
        assert_eq!(chain_bytes(m), packet);

        // Garbage does not decompress.
        let bad = chain(&[hdr.clone(), std::vec![0xff; 8]].concat(), &[28]);
        assert_eq!(
            run_deflate_mbuf(sid, bad, desc(CRYPTO_DEFLATE_COMP, 20, 8, 20, 0)),
            Err(Errno::EINVAL)
        );
    }

    #[test]
    fn deflate_leaves_incompressible_data_alone() {
        let _g = fw();
        let _m = mbuf_setup();
        let sid = crypto_newsession(&ini(CRYPTO_DEFLATE_COMP, &[], None), 0).unwrap();
        let noise: Vec<u8> = (0..200u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let m = chain(&noise, &[200]);
        let olen =
            run_deflate_mbuf(sid, m, desc(CRYPTO_DEFLATE_COMP, 0, 200, 0, CRD_F_COMP)).unwrap();
        assert!(olen > 200, "the caller sees the useless size");
        assert_eq!(chain_bytes(m), noise);
    }

    #[test]
    fn deflate_trims_the_iovecs() {
        let _g = fw();
        let (hdr, payload) = ipcomp_packet();
        let mut packet = [hdr.clone(), payload.clone()].concat();
        let len = packet.len();
        let base = packet.as_mut_ptr();
        let cuts = [100, 100, 100, len - 300];
        let mut iov: Vec<Iovec> = Vec::new();
        let mut off = 0;
        for c in cuts {
            iov.push(Iovec {
                iov_base: base.wrapping_add(off).cast::<c_void>(),
                iov_len: c,
            });
            off += c;
        }
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: len,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_WRITE,
            uio_procp: None,
        };
        let mut sw = SwcrData {
            sw_alg: CRYPTO_DEFLATE_COMP,
            SWCR_UN: SwcrUn::Comp(SwcrComp {
                sw_size: 0,
                sw_cxf: &comp_algo_deflate,
            }),
        };
        let d = desc(
            CRYPTO_DEFLATE_COMP,
            20,
            payload.len() as i32,
            20,
            CRD_F_COMP,
        );
        let mut buf = CryptoBuf::Iov(&mut uio);
        swcr_compdec(&d, &mut sw, &mut buf).unwrap();
        let compressed = (comp_algo_deflate.compress)(&payload).unwrap();
        let SwcrUn::Comp(c) = &sw.SWCR_UN else {
            panic!("a compressor")
        };
        assert_eq!(c.sw_size as usize, compressed.len());
        let CryptoBuf::Iov(uio) = buf else {
            panic!("the uio")
        };
        // 20 + compressed bytes fit in the first iovec: the others are dropped, it is shortened.
        assert!(20 + compressed.len() < 100);
        assert_eq!(uio.uio_iov.len(), 1);
        assert_eq!(uio.uio_iov[0].iov_len, 20 + compressed.len());
        assert_eq!(&packet[20..20 + compressed.len()], &compressed[..]);

        // A session entry that is not a compressor is refused.
        let mut sw = SwcrData {
            sw_alg: CRYPTO_DEFLATE_COMP,
            SWCR_UN: SwcrUn::None,
        };
        assert_eq!(
            swcr_compdec(&d, &mut sw, &mut CryptoBuf::None),
            Err(Errno::EINVAL)
        );
    }
}
/* </TESTS> */
