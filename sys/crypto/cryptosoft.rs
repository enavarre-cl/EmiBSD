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

#[cfg(test)]
mod tests;
