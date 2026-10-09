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
 *
 * Copyright (c) 2001 Theo de Raadt
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
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
 *
 * Effort sponsored in part by the Defense Advanced Research Projects
 * Agency (DARPA) and Air Force Research Laboratory, Air Force
 * Materiel Command, USAF, under agreement number F30602-01-2-0537.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! `<crypto/cryptodev.h>`: the structures of the kernel crypto framework (`crypto(9)`): a
//! session is described by a chain of [`Cryptoini`]s, a request by a [`Cryptop`] with one
//! [`Cryptodesc`] per operation, a driver by its [`Cryptocap`]. The functions are in
//! `crypto.rs` (`crypto_*`) and `criov.rs` (`cuio_*`).
//!
//! Upstream: sys/crypto/cryptodev.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct cryptoini` is [`Cryptoini`]`<'a>`: `cri_key` is a slice borrowed for the lifetime
//!   of the description (`caddr_t`, NULL is the empty slice), the `cri_next` chain a
//!   `Option<&Cryptoini>` (a chain is built on the stack, last element first), and the union
//!   of `cri_iv` and `cri_esn` one 64-byte array with accessors ([`Cryptoini::cri_iv`],
//!   [`Cryptoini::cri_esn`] and its setter), as the two are views of the same bytes.
//! - `struct cryptodesc` embeds its `CRD_INI` as in the C; the macros that reach into it
//!   (`crd_alg`, `crd_key`, `crd_klen`, `crd_rnd`) are written `crd.CRD_INI.cri_alg` and so on,
//!   and `crd_iv`/`crd_esn` are [`Cryptodesc::crd_iv`] and [`Cryptodesc::crd_esn`] with their
//!   setters.
//! - `struct cryptop` keeps its descriptors in a `Vec` (`crp_desc`; `crp_sdesc[2]` and
//!   `crp_ndescalloc` are not needed, `crp_ndesc` and `crp_ndescalloc` are still set by
//!   `crypto_getreq`), its data in [`CryptoBuf`] (the `void *crp_buf` that is an mbuf chain
//!   under `CRYPTO_F_IMBUF` and a `struct uio` under `CRYPTO_F_IOV`) and its MAC output
//!   `crp_mac` in an `Option<&mut [u8]>`. A request is a value, not an item of a pool (see
//!   `crypto.rs`).
//! - `struct cryptocap`'s callbacks are `Option`s of `fn` pointers returning
//!   `Result<(), Errno>`; `cc_newsession` takes the session id by `&mut` (the driver id goes
//!   in, the driver's own session number comes out), as in the C.
//! - `CHACHA20_BLOCK_LEN` is the constant of `chachapoly.h`, which `cryptodev.h` defines
//!   again with the same value.
//! - `#include <sys/task.h>` is dropped: this version of the framework has no queue of its own.

use alloc::vec::Vec;

use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::uio::Uio;

pub use super::chachapoly::CHACHA20_BLOCK_LEN;

/// `CRYPTO_DRIVERS_INITIAL`: some initial values.
pub const CRYPTO_DRIVERS_INITIAL: usize = 4;
/// `CRYPTO_DRIVERS_MAX`.
pub const CRYPTO_DRIVERS_MAX: usize = 128;
/// `CRYPTO_SW_SESSIONS`.
pub const CRYPTO_SW_SESSIONS: usize = 32;

/// `HMAC_MD5_BLOCK_LEN`: HMAC values.
pub const HMAC_MD5_BLOCK_LEN: usize = 64;
/// `HMAC_SHA1_BLOCK_LEN`.
pub const HMAC_SHA1_BLOCK_LEN: usize = 64;
/// `HMAC_RIPEMD160_BLOCK_LEN`.
pub const HMAC_RIPEMD160_BLOCK_LEN: usize = 64;
/// `HMAC_SHA2_256_BLOCK_LEN`.
pub const HMAC_SHA2_256_BLOCK_LEN: usize = 64;
/// `HMAC_SHA2_384_BLOCK_LEN`.
pub const HMAC_SHA2_384_BLOCK_LEN: usize = 128;
/// `HMAC_SHA2_512_BLOCK_LEN`.
pub const HMAC_SHA2_512_BLOCK_LEN: usize = 128;
/// `HMAC_MAX_BLOCK_LEN`: keep in sync.
pub const HMAC_MAX_BLOCK_LEN: usize = HMAC_SHA2_512_BLOCK_LEN;
/// `HMAC_IPAD_VAL`.
pub const HMAC_IPAD_VAL: u8 = 0x36;
/// `HMAC_OPAD_VAL`.
pub const HMAC_OPAD_VAL: u8 = 0x5C;

/// `DES3_BLOCK_LEN`: encryption algorithm block sizes.
pub const DES3_BLOCK_LEN: usize = 8;
/// `BLOWFISH_BLOCK_LEN`.
pub const BLOWFISH_BLOCK_LEN: usize = 8;
/// `CAST128_BLOCK_LEN`.
pub const CAST128_BLOCK_LEN: usize = 8;
/// `RIJNDAEL128_BLOCK_LEN`.
pub const RIJNDAEL128_BLOCK_LEN: usize = 16;
/// `EALG_MAX_BLOCK_LEN`: keep this updated.
pub const EALG_MAX_BLOCK_LEN: usize = 64;

/// `AALG_MAX_RESULT_LEN`: maximum hash algorithm result length; keep this updated.
pub const AALG_MAX_RESULT_LEN: usize = 64;

/// `CRYPTO_3DES_CBC`.
pub const CRYPTO_3DES_CBC: i32 = 1;
/// `CRYPTO_BLF_CBC`.
pub const CRYPTO_BLF_CBC: i32 = 2;
/// `CRYPTO_CAST_CBC`.
pub const CRYPTO_CAST_CBC: i32 = 3;
/// `CRYPTO_MD5_HMAC`.
pub const CRYPTO_MD5_HMAC: i32 = 4;
/// `CRYPTO_SHA1_HMAC`.
pub const CRYPTO_SHA1_HMAC: i32 = 5;
/// `CRYPTO_RIPEMD160_HMAC`.
pub const CRYPTO_RIPEMD160_HMAC: i32 = 6;
/// `CRYPTO_RIJNDAEL128_CBC`: 128 bit blocksize.
pub const CRYPTO_RIJNDAEL128_CBC: i32 = 7;
/// `CRYPTO_AES_CBC`: 128 bit blocksize -- the same as above.
pub const CRYPTO_AES_CBC: i32 = 7;
/// `CRYPTO_DEFLATE_COMP`: Deflate compression algorithm.
pub const CRYPTO_DEFLATE_COMP: i32 = 8;
/// `CRYPTO_NULL`.
pub const CRYPTO_NULL: i32 = 9;
/// `CRYPTO_SHA2_256_HMAC`.
pub const CRYPTO_SHA2_256_HMAC: i32 = 11;
/// `CRYPTO_SHA2_384_HMAC`.
pub const CRYPTO_SHA2_384_HMAC: i32 = 12;
/// `CRYPTO_SHA2_512_HMAC`.
pub const CRYPTO_SHA2_512_HMAC: i32 = 13;
/// `CRYPTO_AES_CTR`.
pub const CRYPTO_AES_CTR: i32 = 14;
/// `CRYPTO_AES_XTS`.
pub const CRYPTO_AES_XTS: i32 = 15;
/// `CRYPTO_AES_GCM_16`.
pub const CRYPTO_AES_GCM_16: i32 = 16;
/// `CRYPTO_AES_128_GMAC`.
pub const CRYPTO_AES_128_GMAC: i32 = 17;
/// `CRYPTO_AES_192_GMAC`.
pub const CRYPTO_AES_192_GMAC: i32 = 18;
/// `CRYPTO_AES_256_GMAC`.
pub const CRYPTO_AES_256_GMAC: i32 = 19;
/// `CRYPTO_AES_GMAC`.
pub const CRYPTO_AES_GMAC: i32 = 20;
/// `CRYPTO_CHACHA20_POLY1305`.
pub const CRYPTO_CHACHA20_POLY1305: i32 = 21;
/// `CRYPTO_CHACHA20_POLY1305_MAC`.
pub const CRYPTO_CHACHA20_POLY1305_MAC: i32 = 22;
/// `CRYPTO_ESN`: support for Extended Sequence Numbers.
pub const CRYPTO_ESN: i32 = 23;
/// `CRYPTO_ALGORITHM_MAX`: keep updated.
pub const CRYPTO_ALGORITHM_MAX: usize = 23;

/// `CRYPTO_ALG_FLAG_SUPPORTED`: algorithm flags: algorithm is supported.
pub const CRYPTO_ALG_FLAG_SUPPORTED: i32 = 0x01;

/// `CRD_F_ENCRYPT`: set when doing encryption.
pub const CRD_F_ENCRYPT: i32 = 0x01;
/// `CRD_F_IV_PRESENT`: when encrypting, IV is already in place, so don't copy.
pub const CRD_F_IV_PRESENT: i32 = 0x02;
/// `CRD_F_IV_EXPLICIT`: IV explicitly provided.
pub const CRD_F_IV_EXPLICIT: i32 = 0x04;
/// `CRD_F_COMP`: set when doing compression.
pub const CRD_F_COMP: i32 = 0x10;
/// `CRD_F_ESN`: set when ESN field is provided.
pub const CRD_F_ESN: i32 = 0x20;

/// `CRYPTO_F_IMBUF`: input/output are mbuf chains, otherwise contig.
pub const CRYPTO_F_IMBUF: i32 = 0x0001;
/// `CRYPTO_F_IOV`: input/output are uio.
pub const CRYPTO_F_IOV: i32 = 0x0002;

/// `CRYPTO_BUF_IOV`.
pub const CRYPTO_BUF_IOV: i32 = 0x1;
/// `CRYPTO_BUF_MBUF`.
pub const CRYPTO_BUF_MBUF: i32 = 0x2;

/// `CRYPTO_OP_DECRYPT`.
pub const CRYPTO_OP_DECRYPT: i32 = 0x0;
/// `CRYPTO_OP_ENCRYPT`.
pub const CRYPTO_OP_ENCRYPT: i32 = 0x1;

/// `CRYPTOCAP_F_CLEANUP`.
pub const CRYPTOCAP_F_CLEANUP: u8 = 0x01;
/// `CRYPTOCAP_F_SOFTWARE`.
pub const CRYPTOCAP_F_SOFTWARE: u8 = 0x02;
/// `CRYPTOCAP_F_MPSAFE`.
pub const CRYPTOCAP_F_MPSAFE: u8 = 0x04;

/// `struct cryptoini`: standard initialization structure beginning.
#[derive(Clone, Copy, Debug)]
pub struct Cryptoini<'a> {
    /// `cri_alg`: algorithm to use.
    pub cri_alg: i32,
    /// `cri_klen`: key length, in bits.
    pub cri_klen: i32,
    /// `cri_rnd`: algorithm rounds, where relevant.
    pub cri_rnd: i32,
    /// `cri_key`: key to use.
    pub cri_key: &'a [u8],
    /// `u`: the union of `iv` (IV to use) and `esn` (high-order ESN): the same bytes.
    pub u: [u8; EALG_MAX_BLOCK_LEN],
    /// `cri_next`.
    pub cri_next: Option<&'a Cryptoini<'a>>,
}

impl Default for Cryptoini<'_> {
    fn default() -> Self {
        Self {
            cri_alg: 0,
            cri_klen: 0,
            cri_rnd: 0,
            cri_key: &[],
            u: [0; EALG_MAX_BLOCK_LEN],
            cri_next: None,
        }
    }
}

impl Cryptoini<'_> {
    /// `cri_iv`: the IV.
    pub fn cri_iv(&self) -> &[u8; EALG_MAX_BLOCK_LEN] {
        &self.u
    }

    /// `cri_iv`, writable.
    pub fn cri_iv_mut(&mut self) -> &mut [u8; EALG_MAX_BLOCK_LEN] {
        &mut self.u
    }

    /// `cri_esn`: the high-order ESN, the first four bytes of the union.
    pub fn cri_esn(&self) -> [u8; 4] {
        [self.u[0], self.u[1], self.u[2], self.u[3]]
    }

    /// Sets `cri_esn`.
    pub fn set_cri_esn(&mut self, esn: [u8; 4]) {
        self.u[..4].copy_from_slice(&esn);
    }
}

/// `struct cryptodesc`: describe boundaries of a single crypto operation.
#[derive(Clone, Copy, Debug, Default)]
#[allow(non_snake_case)] // the C name of the member, CRD_INI
pub struct Cryptodesc<'a> {
    /// `crd_skip`: how many bytes to ignore from start.
    pub crd_skip: i32,
    /// `crd_len`: how many bytes to process.
    pub crd_len: i32,
    /// `crd_inject`: where to inject results, if applicable.
    pub crd_inject: i32,
    /// `crd_flags`: `CRD_F_*`.
    pub crd_flags: i32,
    /// `CRD_INI`: initialization/context data.
    pub CRD_INI: Cryptoini<'a>,
}

impl Cryptodesc<'_> {
    /// `crd_iv`.
    pub fn crd_iv(&self) -> &[u8; EALG_MAX_BLOCK_LEN] {
        self.CRD_INI.cri_iv()
    }

    /// `crd_iv`, writable.
    pub fn crd_iv_mut(&mut self) -> &mut [u8; EALG_MAX_BLOCK_LEN] {
        self.CRD_INI.cri_iv_mut()
    }

    /// `crd_esn`.
    pub fn crd_esn(&self) -> [u8; 4] {
        self.CRD_INI.cri_esn()
    }

    /// Sets `crd_esn`.
    pub fn set_crd_esn(&mut self, esn: [u8; 4]) {
        self.CRD_INI.set_cri_esn(esn);
    }
}

/// The data a request works on: `crp_buf`, an mbuf chain or a `struct uio` according to
/// `crp_flags`.
#[derive(Default)]
pub enum CryptoBuf<'a> {
    /// `crp_buf == NULL`.
    #[default]
    None,
    /// `CRYPTO_F_IMBUF`: an mbuf chain.
    Mbuf(&'a Mbuf),
    /// `CRYPTO_F_IOV`: a uio of kernel buffers.
    Iov(&'a mut Uio<'a>),
}

/// `struct cryptop`: structure describing complete operation.
#[derive(Default)]
pub struct Cryptop<'a> {
    /// `crp_sid`: session ID.
    pub crp_sid: u64,
    /// `crp_ilen`: input data total length.
    pub crp_ilen: i32,
    /// `crp_olen`: result total length.
    pub crp_olen: i32,
    /// `crp_alloctype`: type of buf to allocate if needed.
    pub crp_alloctype: i32,
    /// `crp_flags`: `CRYPTO_F_*`.
    pub crp_flags: i32,
    /// `crp_buf`: data to be processed.
    pub crp_buf: CryptoBuf<'a>,
    /// `crp_desc`: list of processing descriptors.
    pub crp_desc: Vec<Cryptodesc<'a>>,
    /// `crp_ndesc`: amount of descriptors to use.
    pub crp_ndesc: i32,
    /// `crp_ndescalloc`: amount of descriptors allocated.
    pub crp_ndescalloc: i32,
    /// `crp_mac`: where the MAC goes for a non-mbuf buffer.
    pub crp_mac: Option<&'a mut [u8]>,
}

/// `cc_newsession`: `int (*)(u_int32_t *, struct cryptoini *)`.
pub type CcNewsession = fn(&mut u32, &Cryptoini<'_>) -> Result<(), Errno>;
/// `cc_process`: `int (*)(struct cryptop *)`.
pub type CcProcess = fn(&mut Cryptop<'_>) -> Result<(), Errno>;
/// `cc_freesession`: `int (*)(u_int64_t)`.
pub type CcFreesession = fn(u64) -> Result<(), Errno>;

/// `struct cryptocap`: crypto capabilities structure.
#[derive(Clone, Copy, Default)]
pub struct Cryptocap {
    /// `cc_operations`: counter of how many ops done.
    pub cc_operations: u64,
    /// `cc_bytes`: counter of how many bytes done.
    pub cc_bytes: u64,
    /// `cc_sessions`: how many sessions allocated.
    pub cc_sessions: u32,
    /// `cc_alg`: symmetric/hash algorithms supported.
    pub cc_alg: [i32; CRYPTO_ALGORITHM_MAX + 1],
    /// `cc_flags`: `CRYPTOCAP_F_*`.
    pub cc_flags: u8,
    /// `cc_newsession`.
    pub cc_newsession: Option<CcNewsession>,
    /// `cc_process`.
    pub cc_process: Option<CcProcess>,
    /// `cc_freesession`.
    pub cc_freesession: Option<CcFreesession>,
}
/* </CODE> */
