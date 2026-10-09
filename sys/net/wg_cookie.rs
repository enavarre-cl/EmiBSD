/* <LICENSES> */
/*
 * Copyright (C) 2015-2020 Jason A. Donenfeld <Jason@zx2c4.com>. All Rights Reserved.
 * Copyright (C) 2019-2020 Matt Dunwoodie <ncon@noconroy.net>
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
/* </LICENSES> */

/* <CODE> */
//! WireGuard's handshake MACs, cookies and initiation rate limiting: `<net/wg_cookie.h>` and
//! `net/wg_cookie.c`.
//!
//! Upstream: sys/net/wg_cookie.h @ 3ce1f3f79392
//! Upstream: sys/net/wg_cookie.c @ 3ce1f3f79392
//!
//! Every handshake message ends with two MACs ([`CookieMacs`]). `mac1` is keyed with the hash
//! of the receiver's static public key, so only someone who knows that key can make the
//! receiver do Diffie-Hellman work. `mac2` is keyed with a cookie: a responder under load
//! answers an initiation without a valid `mac2` with a cookie message (an XChaCha20-Poly1305
//! sealed MAC of the sender's address under a secret that changes every two minutes), and the
//! sender puts that cookie in `mac2` of its next attempt. A [`CookieMaker`] is the sender's
//! side, per peer; the [`CookieChecker`] is the receiver's, per interface, with a token bucket
//! per source address ([`Ratelimit`]) for initiations that carry a valid cookie.
//!
//! ## Deviations
//! - The header and the file share this module.
//! - Members the C changes through a shared pointer under the locks are `Cell`s; every
//!   structure is valid all zero (`bzero`).
//! - `INET6` is configured (feature `inet6`): `cc_ratelimit_v6`, `ratelimit_entry`'s `r_in6`
//!   (side by side with `r_in`, not a union; only its top `IPV6_MASK_SIZE` bytes are used)
//!   and the `AF_INET6` branches. Without the feature an IPv6 source makes
//!   `cookie_checker_validate_macs` return `EAFNOSUPPORT` and `cookie_checker_make_cookie`
//!   draw a random cookie, as the C's `default` cases do.
//! - The source address is `&SockaddrStorage` (the C's `struct sockaddr *`): a `struct
//!   sockaddr_in6` does not fit a `&Sockaddr`; the address is copied out of the storage
//!   unaligned.
//! - `rl_table` is the slice `hashinit` returns and `rl_table_mask` is its length minus one
//!   (the C's `*hashmask`); `rl_pool` is `Option<&'static Pool>`. A `pool_get`ed entry is
//!   written whole before it is linked.
//! - `cookie_checker_update`'s NULL key is `None`; `cookie_timer_expired` answers `bool`
//!   (`ETIMEDOUT` is `true`).
//! - `WGTEST`'s `cookie_test` (rate limit timings and capacity, MACs) is the host test module;
//!   the timings test advances the timecounter where the C sleeps with `tsleep_nsec`.

use core::cell::Cell;
use core::ptr::{self, NonNull};

use libkern::{explicit_bzero, timingsafe_bcmp};

use crate::crypto::blake2s::{
    Blake2sState, blake2s_final, blake2s_init, blake2s_init_key, blake2s_update,
};
use crate::crypto::chachapoly::{
    XCHACHA20POLY1305_NONCE_SIZE, xchacha20poly1305_decrypt, xchacha20poly1305_encrypt,
};
use crate::crypto::siphash::{SipHash24, SiphashKey};
use crate::crypto::wipe;
use crate::dev::rnd::arc4random_buf;
use crate::kern::kern_rwlock::{
    rw_assert_wrlock, rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write, rw_init,
};
use crate::kern::kern_subr::{hashfree, hashinit};
use crate::kern::kern_tc::getnanouptime;
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::netinet::in_::{InAddr, SockaddrIn};
#[cfg(feature = "inet6")]
use crate::netinet6::in6::{In6Addr, SockaddrIn6};
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT};
use crate::sys::pool::{PR_NOWAIT, Pool};
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::rwlock::Rwlock;
#[cfg(feature = "inet6")]
use crate::sys::socket::AF_INET6;
use crate::sys::socket::{AF_INET, AF_UNSPEC, SockaddrStorage};
use crate::sys::time::{Timespec, timespecadd, timespecsub};
use crate::sys::types::{SaFamily, Time};

/// `COOKIE_MAC_SIZE`.
pub const COOKIE_MAC_SIZE: usize = 16;
/// `COOKIE_KEY_SIZE`.
pub const COOKIE_KEY_SIZE: usize = 32;
/// `COOKIE_NONCE_SIZE`.
pub const COOKIE_NONCE_SIZE: usize = XCHACHA20POLY1305_NONCE_SIZE;
/// `COOKIE_COOKIE_SIZE`.
pub const COOKIE_COOKIE_SIZE: usize = 16;
/// `COOKIE_SECRET_SIZE`.
pub const COOKIE_SECRET_SIZE: usize = 32;
/// `COOKIE_INPUT_SIZE`.
pub const COOKIE_INPUT_SIZE: usize = 32;
/// `COOKIE_ENCRYPTED_SIZE`.
pub const COOKIE_ENCRYPTED_SIZE: usize = COOKIE_COOKIE_SIZE + COOKIE_MAC_SIZE;

/// `COOKIE_MAC1_KEY_LABEL`.
pub const COOKIE_MAC1_KEY_LABEL: &[u8] = b"mac1----";
/// `COOKIE_COOKIE_KEY_LABEL`.
pub const COOKIE_COOKIE_KEY_LABEL: &[u8] = b"cookie--";
/// `COOKIE_SECRET_MAX_AGE` (seconds).
pub const COOKIE_SECRET_MAX_AGE: Time = 120;
/// `COOKIE_SECRET_LATENCY` (seconds).
pub const COOKIE_SECRET_LATENCY: Time = 5;

// Constants for initiation rate limiting

/// `RATELIMIT_SIZE`.
pub const RATELIMIT_SIZE: usize = 1 << 13;
/// `RATELIMIT_SIZE_MAX`.
pub const RATELIMIT_SIZE_MAX: usize = RATELIMIT_SIZE * 8;
/// `NSEC_PER_SEC`.
pub const NSEC_PER_SEC: u64 = 1_000_000_000;
/// `INITIATIONS_PER_SECOND`.
pub const INITIATIONS_PER_SECOND: u64 = 20;
/// `INITIATIONS_BURSTABLE`.
pub const INITIATIONS_BURSTABLE: u64 = 5;
/// `INITIATION_COST`.
pub const INITIATION_COST: u64 = NSEC_PER_SEC / INITIATIONS_PER_SECOND;
/// `TOKEN_MAX`.
pub const TOKEN_MAX: u64 = INITIATION_COST * INITIATIONS_BURSTABLE;
/// `ELEMENT_TIMEOUT` (seconds).
pub const ELEMENT_TIMEOUT: Time = 1;
/// `IPV4_MASK_SIZE`: use all 4 bytes of IPv4 address.
pub const IPV4_MASK_SIZE: usize = 4;
/// `IPV6_MASK_SIZE`: use top 8 bytes (/64) of IPv6 address.
pub const IPV6_MASK_SIZE: usize = 8;

/// `struct cookie_macs`: the two MACs at the end of a handshake message.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CookieMacs {
    /// `mac1`.
    pub mac1: [u8; COOKIE_MAC_SIZE],
    /// `mac2`.
    pub mac2: [u8; COOKIE_MAC_SIZE],
}

/// `struct ratelimit_entry`: the token bucket of one source address.
pub struct RatelimitEntry {
    /// `r_entry`.
    pub r_entry: ListEntry<RatelimitEntry>,
    /// `r_af`.
    pub r_af: Cell<SaFamily>,
    /// `r_in`.
    pub r_in: Cell<InAddr>,
    /// `r_in6` (the C union's other member): only the top `IPV6_MASK_SIZE` bytes are kept.
    #[cfg(feature = "inet6")]
    pub r_in6: Cell<In6Addr>,
    /// `r_last_time`: nanouptime.
    pub r_last_time: Cell<Timespec>,
    /// `r_tokens`.
    pub r_tokens: Cell<u64>,
}

impl RatelimitEntry {
    /// An entry on no list.
    pub const fn new() -> Self {
        Self {
            r_entry: ListEntry::new(),
            r_af: Cell::new(AF_UNSPEC),
            r_in: Cell::new(InAddr { s_addr: 0 }),
            #[cfg(feature = "inet6")]
            r_in6: Cell::new(In6Addr { s6_addr: [0; 16] }),
            r_last_time: Cell::new(Timespec::new(0, 0)),
            r_tokens: Cell::new(0),
        }
    }
}

impl Default for RatelimitEntry {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `LIST_HEAD(, ratelimit_entry)`: a bucket of `rl_table`.
    pub RatelimitEntries: RatelimitEntry, r_entry => ListEntry<RatelimitEntry>
);

/// `struct ratelimit`: the token buckets of the sources of one address family.
pub struct Ratelimit {
    /// `rl_secret`: the hash key of the table.
    pub rl_secret: Cell<SiphashKey>,
    /// `rl_pool`: where the entries come from.
    pub rl_pool: Cell<Option<&'static Pool>>,

    /// `rl_lock`.
    pub rl_lock: Rwlock,
    /// `rl_table`. Protected by: `rl_lock`.
    pub rl_table: Cell<Option<&'static [ListHead<RatelimitEntries>]>>,
    /// `rl_table_mask`.
    pub rl_table_mask: Cell<u64>,
    /// `rl_table_num`. Protected by: `rl_lock`.
    pub rl_table_num: Cell<usize>,
    /// `rl_last_gc`: nanouptime. Protected by: `rl_lock`.
    pub rl_last_gc: Cell<Timespec>,
}

impl Ratelimit {
    /// An all-zero rate limiter.
    pub const fn new() -> Self {
        Self {
            rl_secret: Cell::new(SiphashKey { k0: 0, k1: 0 }),
            rl_pool: Cell::new(None),
            rl_lock: Rwlock::new("ratelimit_lock"),
            rl_table: Cell::new(None),
            rl_table_mask: Cell::new(0),
            rl_table_num: Cell::new(0),
            rl_last_gc: Cell::new(Timespec::new(0, 0)),
        }
    }

    /// `rl->rl_table`, made by `ratelimit_init`.
    fn table(&self) -> &'static [ListHead<RatelimitEntries>] {
        self.rl_table.get().unwrap_or(&[])
    }
}

impl Default for Ratelimit {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct cookie_maker`: the sender's side: the keys derived from the peer's public key and
/// the last cookie the peer sent.
pub struct CookieMaker {
    /// `cp_mac1_key`. Immutable after `cookie_maker_init`.
    pub cp_mac1_key: Cell<[u8; COOKIE_KEY_SIZE]>,
    /// `cp_cookie_key`. Immutable after `cookie_maker_init`.
    pub cp_cookie_key: Cell<[u8; COOKIE_KEY_SIZE]>,

    /// `cp_lock`.
    pub cp_lock: Rwlock,
    /// `cp_cookie`. Protected by: `cp_lock`.
    pub cp_cookie: Cell<[u8; COOKIE_COOKIE_SIZE]>,
    /// `cp_birthdate`: nanouptime. Protected by: `cp_lock`.
    pub cp_birthdate: Cell<Timespec>,
    /// `cp_mac1_valid`. Protected by: `cp_lock`.
    pub cp_mac1_valid: Cell<bool>,
    /// `cp_mac1_last`. Protected by: `cp_lock`.
    pub cp_mac1_last: Cell<[u8; COOKIE_MAC_SIZE]>,
}

// SAFETY: the keys are written once by `cookie_maker_init` before the maker is shared; the
// rest changes under `cp_lock` (`cookie_maker_mac` writes the last MAC under the read lock, as
// the C does).
unsafe impl Sync for CookieMaker {}

impl CookieMaker {
    /// An all-zero cookie maker.
    pub const fn new() -> Self {
        Self {
            cp_mac1_key: Cell::new([0; COOKIE_KEY_SIZE]),
            cp_cookie_key: Cell::new([0; COOKIE_KEY_SIZE]),
            cp_lock: Rwlock::new("cookie_maker"),
            cp_cookie: Cell::new([0; COOKIE_COOKIE_SIZE]),
            cp_birthdate: Cell::new(Timespec::new(0, 0)),
            cp_mac1_valid: Cell::new(false),
            cp_mac1_last: Cell::new([0; COOKIE_MAC_SIZE]),
        }
    }
}

impl Default for CookieMaker {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct cookie_checker`: the receiver's side: the keys derived from our public key, the
/// rotating cookie secret and the rate limiters.
pub struct CookieChecker {
    /// `cc_ratelimit_v4`.
    pub cc_ratelimit_v4: Ratelimit,
    /// `cc_ratelimit_v6`.
    #[cfg(feature = "inet6")]
    pub cc_ratelimit_v6: Ratelimit,
    /// `cc_key_lock`.
    pub cc_key_lock: Rwlock,
    /// `cc_mac1_key`. Protected by: `cc_key_lock`.
    pub cc_mac1_key: Cell<[u8; COOKIE_KEY_SIZE]>,
    /// `cc_cookie_key`. Protected by: `cc_key_lock`.
    pub cc_cookie_key: Cell<[u8; COOKIE_KEY_SIZE]>,

    /// `cc_secret_lock`.
    pub cc_secret_lock: Rwlock,
    /// `cc_secret_birthdate`: nanouptime. Protected by: `cc_secret_lock`.
    pub cc_secret_birthdate: Cell<Timespec>,
    /// `cc_secret`. Protected by: `cc_secret_lock`.
    pub cc_secret: Cell<[u8; COOKIE_SECRET_SIZE]>,
}

// SAFETY: the keys change under `cc_key_lock`, the secret under `cc_secret_lock`, a rate
// limiter's table under its `rl_lock`; `rl_secret` and `rl_pool` are written by
// `cookie_checker_init` before the checker is shared.
unsafe impl Sync for CookieChecker {}

impl CookieChecker {
    /// An all-zero cookie checker.
    pub const fn new() -> Self {
        Self {
            cc_ratelimit_v4: Ratelimit::new(),
            #[cfg(feature = "inet6")]
            cc_ratelimit_v6: Ratelimit::new(),
            cc_key_lock: Rwlock::new("cookie_checker_key"),
            cc_mac1_key: Cell::new([0; COOKIE_KEY_SIZE]),
            cc_cookie_key: Cell::new([0; COOKIE_KEY_SIZE]),
            cc_secret_lock: Rwlock::new("cookie_checker_secret"),
            cc_secret_birthdate: Cell::new(Timespec::new(0, 0)),
            cc_secret: Cell::new([0; COOKIE_SECRET_SIZE]),
        }
    }
}

impl Default for CookieChecker {
    fn default() -> Self {
        Self::new()
    }
}

// Public Functions

/// `cookie_maker_init`: a maker for the peer whose public key is `key`.
pub fn cookie_maker_init(cp: &CookieMaker, key: &[u8; COOKIE_INPUT_SIZE]) {
    // bzero(cp, sizeof(*cp))
    cp.cp_cookie.set([0; COOKIE_COOKIE_SIZE]);
    cp.cp_birthdate.set(Timespec::default());
    cp.cp_mac1_valid.set(false);
    cp.cp_mac1_last.set([0; COOKIE_MAC_SIZE]);

    let mut k = [0u8; COOKIE_KEY_SIZE];
    cookie_precompute_key(&mut k, key, COOKIE_MAC1_KEY_LABEL);
    cp.cp_mac1_key.set(k);
    cookie_precompute_key(&mut k, key, COOKIE_COOKIE_KEY_LABEL);
    cp.cp_cookie_key.set(k);
    explicit_bzero(&mut k);
    rw_init(&cp.cp_lock, "cookie_maker");
}

/// `cookie_checker_init`: a checker without keys whose rate limiters take entries from
/// `pool`; `ENOBUFS` when a table cannot be allocated.
pub fn cookie_checker_init(cc: &CookieChecker, pool: &'static Pool) -> Result<(), Errno> {
    // bzero(cc, sizeof(*cc))
    cc.cc_mac1_key.set([0; COOKIE_KEY_SIZE]);
    cc.cc_cookie_key.set([0; COOKIE_KEY_SIZE]);
    cc.cc_secret_birthdate.set(Timespec::default());
    cc.cc_secret.set([0; COOKIE_SECRET_SIZE]);
    let rl = &cc.cc_ratelimit_v4;
    rl.rl_table.set(None);
    rl.rl_table_mask.set(0);
    rl.rl_table_num.set(0);
    rl.rl_last_gc.set(Timespec::default());
    #[cfg(feature = "inet6")]
    {
        let rl = &cc.cc_ratelimit_v6;
        rl.rl_table.set(None);
        rl.rl_table_mask.set(0);
        rl.rl_table_num.set(0);
        rl.rl_last_gc.set(Timespec::default());
    }

    rw_init(&cc.cc_key_lock, "cookie_checker_key");
    rw_init(&cc.cc_secret_lock, "cookie_checker_secret");

    ratelimit_init(&cc.cc_ratelimit_v4, pool)?;
    #[cfg(feature = "inet6")]
    if let Err(res) = ratelimit_init(&cc.cc_ratelimit_v6, pool) {
        ratelimit_deinit(&cc.cc_ratelimit_v4);
        return Err(res);
    }
    Ok(())
}

/// `cookie_checker_update`: the MAC and cookie keys for our public key `key`, or none (no
/// identity).
pub fn cookie_checker_update(cc: &CookieChecker, key: Option<&[u8; COOKIE_INPUT_SIZE]>) {
    rw_enter_write(&cc.cc_key_lock);
    if let Some(key) = key {
        let mut k = [0u8; COOKIE_KEY_SIZE];
        cookie_precompute_key(&mut k, key, COOKIE_MAC1_KEY_LABEL);
        cc.cc_mac1_key.set(k);
        cookie_precompute_key(&mut k, key, COOKIE_COOKIE_KEY_LABEL);
        cc.cc_cookie_key.set(k);
        explicit_bzero(&mut k);
    } else {
        cc.cc_mac1_key.set([0; COOKIE_KEY_SIZE]);
        cc.cc_cookie_key.set([0; COOKIE_KEY_SIZE]);
    }
    rw_exit_write(&cc.cc_key_lock);
}

/// `cookie_checker_deinit`: frees the rate limiters' entries and tables.
pub fn cookie_checker_deinit(cc: &CookieChecker) {
    ratelimit_deinit(&cc.cc_ratelimit_v4);
    #[cfg(feature = "inet6")]
    ratelimit_deinit(&cc.cc_ratelimit_v6);
}

/// `cookie_checker_create_payload`: the body of a cookie message for the sender at `sa`: a
/// random `nonce` and the cookie sealed under our cookie key, with the sender's `mac1` as
/// associated data.
pub fn cookie_checker_create_payload(
    cc: &CookieChecker,
    cm: &CookieMacs,
    nonce: &mut [u8; COOKIE_NONCE_SIZE],
    ecookie: &mut [u8; COOKIE_ENCRYPTED_SIZE],
    sa: &SockaddrStorage,
) {
    let mut cookie = [0u8; COOKIE_COOKIE_SIZE];

    cookie_checker_make_cookie(cc, &mut cookie, sa);
    arc4random_buf(nonce);

    rw_enter_read(&cc.cc_key_lock);
    let mut key = cc.cc_cookie_key.get();
    xchacha20poly1305_encrypt(ecookie, &cookie, &cm.mac1, nonce, &key);
    explicit_bzero(&mut key);
    rw_exit_read(&cc.cc_key_lock);

    explicit_bzero(&mut cookie);
}

/// `cookie_maker_consume_payload`: opens a cookie message answering our last MAC'd message
/// and keeps the cookie for `mac2`. `ETIMEDOUT` when no message waits for one (or the cookie
/// was already taken), `EINVAL` when it does not open.
pub fn cookie_maker_consume_payload(
    cp: &CookieMaker,
    nonce: &[u8; COOKIE_NONCE_SIZE],
    ecookie: &[u8; COOKIE_ENCRYPTED_SIZE],
) -> Result<(), Errno> {
    let mut cookie = [0u8; COOKIE_COOKIE_SIZE];

    rw_enter_write(&cp.cp_lock);

    let ret = 'error: {
        if !cp.cp_mac1_valid.get() {
            break 'error Err(Errno::ETIMEDOUT);
        }

        let mut key = cp.cp_cookie_key.get();
        let opened =
            xchacha20poly1305_decrypt(&mut cookie, ecookie, &cp.cp_mac1_last.get(), nonce, &key);
        explicit_bzero(&mut key);
        if !opened {
            break 'error Err(Errno::EINVAL);
        }

        cp.cp_cookie.set(cookie);
        cp.cp_birthdate.set(getnanouptime());
        cp.cp_mac1_valid.set(false);
        Ok(())
    };

    rw_exit_write(&cp.cp_lock);
    explicit_bzero(&mut cookie);
    ret
}

/// `cookie_maker_mac`: the MACs of `buf` (a handshake message up to its MACs): `mac1` always,
/// `mac2` with the peer's cookie while it is fresh, zero otherwise.
pub fn cookie_maker_mac(cp: &CookieMaker, cm: &mut CookieMacs, buf: &[u8]) {
    rw_enter_read(&cp.cp_lock);

    cookie_macs_mac1(cm, buf, &cp.cp_mac1_key.get());

    cp.cp_mac1_last.set(cm.mac1);
    cp.cp_mac1_valid.set(true);

    if !cookie_timer_expired(
        &cp.cp_birthdate.get(),
        COOKIE_SECRET_MAX_AGE - COOKIE_SECRET_LATENCY,
        0,
    ) {
        cookie_macs_mac2(cm, buf, &cp.cp_cookie.get());
    } else {
        cm.mac2 = [0; COOKIE_MAC_SIZE];
    }

    rw_exit_read(&cp.cp_lock);
}

/// `cookie_checker_validate_macs`: checks the MACs of a received handshake message from
/// `sa`. `EINVAL`: a bad `mac1` (drop it). Under load (`busy`): `EAGAIN` for a bad `mac2` (send
/// a cookie), `ECONNREFUSED` when the source is over its rate, `EAFNOSUPPORT` for an address
/// family without a rate limiter.
pub fn cookie_checker_validate_macs(
    cc: &CookieChecker,
    cm: &CookieMacs,
    buf: &[u8],
    busy: bool,
    sa: &SockaddrStorage,
) -> Result<(), Errno> {
    let mut our_cm = CookieMacs::default();
    let mut cookie = [0u8; COOKIE_COOKIE_SIZE];

    // Validate incoming MACs
    rw_enter_read(&cc.cc_key_lock);
    cookie_macs_mac1(&mut our_cm, buf, &cc.cc_mac1_key.get());
    rw_exit_read(&cc.cc_key_lock);

    // If mac1 is invalid, we want to drop the packet
    if timingsafe_bcmp(&our_cm.mac1, &cm.mac1) {
        return Err(Errno::EINVAL);
    }

    if busy {
        cookie_checker_make_cookie(cc, &mut cookie, sa);
        cookie_macs_mac2(&mut our_cm, buf, &cookie);
        explicit_bzero(&mut cookie);

        // If the mac2 is invalid, we want to send a cookie response
        if timingsafe_bcmp(&our_cm.mac2, &cm.mac2) {
            return Err(Errno::EAGAIN);
        }

        // If the mac2 is valid, we may want rate limit the peer. ratelimit_allow will return
        // either 0 or ECONNREFUSED, implying there is no ratelimiting, or we should ratelimit
        // (refuse) respectively.
        if sa.ss_family == AF_INET {
            return ratelimit_allow(&cc.cc_ratelimit_v4, sa);
        }
        #[cfg(feature = "inet6")]
        if sa.ss_family == AF_INET6 {
            return ratelimit_allow(&cc.cc_ratelimit_v6, sa);
        }
        return Err(Errno::EAFNOSUPPORT);
    }
    Ok(())
}

// Private functions

/// `satosin(sa)`, read: the `struct sockaddr_in` of an `AF_INET` address (see the module's
/// deviations).
fn sin_of(sa: &SockaddrStorage) -> SockaddrIn {
    // SAFETY: a `sockaddr_storage` is longer than a `struct sockaddr_in` (asserted below) and
    // that is made of integers, so any bytes are a value; the read is unaligned.
    unsafe { ptr::from_ref(sa).cast::<SockaddrIn>().read_unaligned() }
}

/// `satosin6(sa)`, read: the `struct sockaddr_in6` of an `AF_INET6` address.
#[cfg(feature = "inet6")]
fn sin6_of(sa: &SockaddrStorage) -> SockaddrIn6 {
    // SAFETY: as in `sin_of`, for a `struct sockaddr_in6`.
    unsafe { ptr::from_ref(sa).cast::<SockaddrIn6>().read_unaligned() }
}

/// `cookie_precompute_key`: `key = HASH(label || input)`.
fn cookie_precompute_key(
    key: &mut [u8; COOKIE_KEY_SIZE],
    input: &[u8; COOKIE_INPUT_SIZE],
    label: &[u8],
) {
    let mut blake = Blake2sState::default();

    blake2s_init(&mut blake, COOKIE_KEY_SIZE);
    blake2s_update(&mut blake, label);
    blake2s_update(&mut blake, input);
    blake2s_final(&mut blake, key);
}

/// `cookie_macs_mac1`: `mac1 = MAC(key, buf)`.
fn cookie_macs_mac1(cm: &mut CookieMacs, buf: &[u8], key: &[u8; COOKIE_KEY_SIZE]) {
    let mut state = Blake2sState::default();
    blake2s_init_key(&mut state, COOKIE_MAC_SIZE, key);
    blake2s_update(&mut state, buf);
    blake2s_final(&mut state, &mut cm.mac1);
}

/// `cookie_macs_mac2`: `mac2 = MAC(cookie, buf || mac1)`.
fn cookie_macs_mac2(cm: &mut CookieMacs, buf: &[u8], key: &[u8; COOKIE_COOKIE_SIZE]) {
    let mut state = Blake2sState::default();
    blake2s_init_key(&mut state, COOKIE_MAC_SIZE, key);
    blake2s_update(&mut state, buf);
    blake2s_update(&mut state, &cm.mac1);
    blake2s_final(&mut state, &mut cm.mac2);
}

/// `cookie_timer_expired`: is `birthdate` unset, or more than `sec` seconds and `nsec`
/// nanoseconds of uptime ago? (The C's `ETIMEDOUT`.)
fn cookie_timer_expired(birthdate: &Timespec, sec: Time, nsec: i64) -> bool {
    let expire = Timespec::new(sec, nsec);

    if birthdate.tv_sec == 0 && birthdate.tv_nsec == 0 {
        return true;
    }

    let uptime = getnanouptime();
    let expire = timespecadd(birthdate, &expire);
    uptime > expire
}

/// The `AF_INET6` half of `cookie_checker_make_cookie`: hashes `sin6_addr` and `sin6_port`;
/// `false` when `sa` is not an IPv6 address (always, without `INET6`).
#[cfg(feature = "inet6")]
fn cookie_update_in6(state: &mut Blake2sState, sa: &SockaddrStorage) -> bool {
    if sa.ss_family != AF_INET6 {
        return false;
    }
    let sin6 = sin6_of(sa);
    blake2s_update(state, &sin6.sin6_addr.s6_addr);
    blake2s_update(state, &sin6.sin6_port.to_ne_bytes());
    true
}

/// The `AF_INET6` half of `cookie_checker_make_cookie` (without `INET6`: never).
#[cfg(not(feature = "inet6"))]
fn cookie_update_in6(_state: &mut Blake2sState, _sa: &SockaddrStorage) -> bool {
    false
}

/// `cookie_checker_make_cookie`: the cookie of the source `sa`: its address and port MAC'd
/// with the secret, made anew every `COOKIE_SECRET_MAX_AGE` seconds.
fn cookie_checker_make_cookie(
    cc: &CookieChecker,
    cookie: &mut [u8; COOKIE_COOKIE_SIZE],
    sa: &SockaddrStorage,
) {
    let mut state = Blake2sState::default();

    rw_enter_write(&cc.cc_secret_lock);
    if cookie_timer_expired(&cc.cc_secret_birthdate.get(), COOKIE_SECRET_MAX_AGE, 0) {
        let mut secret = [0u8; COOKIE_SECRET_SIZE];
        arc4random_buf(&mut secret);
        cc.cc_secret.set(secret);
        explicit_bzero(&mut secret);
        cc.cc_secret_birthdate.set(getnanouptime());
    }
    let mut secret = cc.cc_secret.get();
    blake2s_init_key(&mut state, COOKIE_COOKIE_SIZE, &secret);
    explicit_bzero(&mut secret);
    rw_exit_write(&cc.cc_secret_lock);

    if sa.ss_family == AF_INET {
        let sin = sin_of(sa);
        blake2s_update(&mut state, &sin.sin_addr.s_addr.to_ne_bytes());
        blake2s_update(&mut state, &sin.sin_port.to_ne_bytes());
        blake2s_final(&mut state, cookie);
    } else if cookie_update_in6(&mut state, sa) {
        blake2s_final(&mut state, cookie);
    } else {
        arc4random_buf(cookie);
    }
    wipe(&mut state);
}

/// `ratelimit_init`: an empty table and a random hash key; `ENOBUFS` without memory.
fn ratelimit_init(rl: &Ratelimit, pool: &'static Pool) -> Result<(), Errno> {
    rw_init(&rl.rl_lock, "ratelimit_lock");
    // arc4random_buf(&rl->rl_secret, sizeof(rl->rl_secret)): the key's two words.
    let (mut k0, mut k1) = ([0u8; 8], [0u8; 8]);
    arc4random_buf(&mut k0);
    arc4random_buf(&mut k1);
    rl.rl_secret.set(SiphashKey {
        k0: u64::from_ne_bytes(k0),
        k1: u64::from_ne_bytes(k1),
    });
    explicit_bzero(&mut k0);
    explicit_bzero(&mut k1);
    let table = hashinit::<RatelimitEntries>(RATELIMIT_SIZE as i32, M_DEVBUF, M_NOWAIT);
    rl.rl_table.set(table);
    rl.rl_table_mask
        .set(table.map_or(0, |t| t.len() as u64 - 1));
    rl.rl_pool.set(Some(pool));
    rl.rl_table_num.set(0);
    if table.is_none() {
        Err(Errno::ENOBUFS)
    } else {
        Ok(())
    }
}

/// `ratelimit_deinit`: frees every entry and the table.
fn ratelimit_deinit(rl: &Ratelimit) {
    rw_enter_write(&rl.rl_lock);
    ratelimit_gc(rl, true);
    if let Some(table) = rl.rl_table.take() {
        // SAFETY: the table `ratelimit_init` made with these arguments; the lists were
        // emptied above and the table is no longer reachable (`rl_table` is cleared).
        unsafe { hashfree(table, RATELIMIT_SIZE as i32, M_DEVBUF) };
    }
    rw_exit_write(&rl.rl_lock);
}

/// Unlinks an entry and gives it back to its pool. Called with `rl_lock` held for writing.
fn ratelimit_entry_free(rl: &Ratelimit, r: &RatelimitEntry) {
    rl.rl_table_num.set(rl.rl_table_num.get() - 1);
    // SAFETY: `r` is on a bucket of `rl_table`, which `rl_lock` (held) protects.
    unsafe { ListHead::<RatelimitEntries>::remove(r) };
    if let Some(pool) = rl.rl_pool.get() {
        pool_put(pool, NonNull::from(r).cast());
    }
}

/// `ratelimit_gc`: frees the entries idle for more than `ELEMENT_TIMEOUT` (at most once per
/// `ELEMENT_TIMEOUT`), or all of them when `force`.
fn ratelimit_gc(rl: &Ratelimit, force: bool) {
    rw_assert_wrlock(&rl.rl_lock);

    if force {
        for bucket in rl.table() {
            for r in bucket.iter() {
                ratelimit_entry_free(rl, r);
            }
        }
        return;
    }

    if cookie_timer_expired(&rl.rl_last_gc.get(), ELEMENT_TIMEOUT, 0) && rl.rl_table_num.get() > 0 {
        rl.rl_last_gc.set(getnanouptime());
        let mut expiry = getnanouptime();
        expiry.tv_sec -= ELEMENT_TIMEOUT;

        for bucket in rl.table() {
            for r in bucket.iter() {
                if r.r_last_time.get() < expiry {
                    ratelimit_entry_free(rl, r);
                }
            }
        }
    }
}

/// The `AF_INET6` key of `ratelimit_allow`: `SipHash24` of the top `IPV6_MASK_SIZE` bytes of
/// `sin6_addr`; `None` when `sa` is not an IPv6 address (always, without `INET6`).
#[cfg(feature = "inet6")]
fn ratelimit_key_in6(rl: &Ratelimit, sa: &SockaddrStorage) -> Option<u64> {
    if sa.ss_family != AF_INET6 {
        return None;
    }
    let sin6 = sin6_of(sa);
    Some(SipHash24(
        &rl.rl_secret.get(),
        &sin6.sin6_addr.s6_addr[..IPV6_MASK_SIZE],
    ))
}

/// The `AF_INET6` key of `ratelimit_allow` (without `INET6`: none).
#[cfg(not(feature = "inet6"))]
fn ratelimit_key_in6(_rl: &Ratelimit, _sa: &SockaddrStorage) -> Option<u64> {
    None
}

/// `ratelimit_allow`: takes an initiation's cost from the bucket of `sa`'s address (a new
/// bucket starts nearly full); `ECONNREFUSED` when it is empty, or no bucket can be made.
fn ratelimit_allow(rl: &Ratelimit, sa: &SockaddrStorage) -> Result<(), Errno> {
    let mut ret = Err(Errno::ECONNREFUSED);

    let sin = sin_of(sa);
    let key = if sa.ss_family == AF_INET {
        SipHash24(
            &rl.rl_secret.get(),
            &sin.sin_addr.s_addr.to_ne_bytes()[..IPV4_MASK_SIZE],
        )
    } else if let Some(key) = ratelimit_key_in6(rl, sa) {
        key
    } else {
        return ret;
    };

    rw_enter_write(&rl.rl_lock);

    'error: {
        let table = rl.table();
        let Some(bucket) = table.get((key & rl.rl_table_mask.get()) as usize) else {
            break 'error;
        };
        for r in bucket.iter() {
            if r.r_af.get() != sa.ss_family {
                continue;
            }

            if r.r_af.get() == AF_INET && r.r_in.get() != sin.sin_addr {
                continue;
            }

            #[cfg(feature = "inet6")]
            if r.r_af.get() == AF_INET6
                && r.r_in6.get().s6_addr[..IPV6_MASK_SIZE]
                    != sin6_of(sa).sin6_addr.s6_addr[..IPV6_MASK_SIZE]
            {
                continue;
            }

            // If we get to here, we've found an entry for the endpoint. We apply standard
            // token bucket, by calculating the time lapsed since our last_time, adding that,
            // ensuring that we cap the tokens at TOKEN_MAX. If the endpoint has no tokens left
            // (that is tokens <= INITIATION_COST) then we block the request, otherwise we
            // subtract the INITIATION_COST and return OK.
            let last = r.r_last_time.get();
            r.r_last_time.set(getnanouptime());
            let diff = timespecsub(&r.r_last_time.get(), &last);

            let tokens = r
                .r_tokens
                .get()
                .wrapping_add((diff.tv_sec as u64).wrapping_mul(NSEC_PER_SEC))
                .wrapping_add(diff.tv_nsec as u64)
                .min(TOKEN_MAX);

            if tokens >= INITIATION_COST {
                r.r_tokens.set(tokens - INITIATION_COST);
                ret = Ok(());
            } else {
                r.r_tokens.set(tokens);
            }
            break 'error;
        }

        // If we get to here, we didn't have an entry for the endpoint.
        ratelimit_gc(rl, false);

        // Hard limit on number of entries
        if rl.rl_table_num.get() >= RATELIMIT_SIZE_MAX {
            break 'error;
        }

        // Goto error if out of memory
        let Some(pool) = rl.rl_pool.get() else {
            break 'error;
        };
        let Some(p) = pool_get(pool, PR_NOWAIT) else {
            break 'error;
        };
        let p = p.cast::<RatelimitEntry>();
        // SAFETY: a fresh pool item of the pool's size (`sizeof(struct ratelimit_entry)`),
        // aligned for it; it is written whole before any reference to it exists.
        let r: &'static RatelimitEntry = unsafe {
            p.as_ptr().write(RatelimitEntry::new());
            p.as_ref()
        };

        rl.rl_table_num.set(rl.rl_table_num.get() + 1);

        // Insert entry into the hashtable and ensure it's initialised
        // SAFETY: a new entry, on no list; the table is protected by `rl_lock` (held).
        unsafe { bucket.insert_head(r) };
        r.r_af.set(sa.ss_family);
        if r.r_af.get() == AF_INET {
            r.r_in.set(sin.sin_addr);
        }
        #[cfg(feature = "inet6")]
        if r.r_af.get() == AF_INET6 {
            let mut a = In6Addr::default();
            a.s6_addr[..IPV6_MASK_SIZE]
                .copy_from_slice(&sin6_of(sa).sin6_addr.s6_addr[..IPV6_MASK_SIZE]);
            r.r_in6.set(a);
        }

        r.r_last_time.set(getnanouptime());
        r.r_tokens.set(TOKEN_MAX - INITIATION_COST);
        ret = Ok(());
    }
    rw_exit_write(&rl.rl_lock);
    ret
}

const _: () = assert!(size_of::<SockaddrIn>() <= size_of::<SockaddrStorage>());
#[cfg(feature = "inet6")]
const _: () = assert!(size_of::<SockaddrIn6>() <= size_of::<SockaddrStorage>());
const _: () = assert!(size_of::<CookieMacs>() == 2 * COOKIE_MAC_SIZE);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the cookie MACs and the rate limiter: `WGTEST`'s
    // `cookie_ratelimit_timings_test` (the sleeps are timecounter advances), `cookie_ratelimit_
    // capacity_test` and `cookie_mac_test`, plus `mac1`/`mac2` vectors computed with an independent
    // BLAKE2s (Python's `hashlib`).

    use std::sync::MutexGuard;

    use super::*;
    use crate::dev::rnd::arc4random;
    use crate::kern::subr_pool::{pool_destroy, pool_init};
    use crate::machine::intr::IPL_NONE;
    use crate::net::wg_noise::tests::{advance_uptime, hex, tc_lock};

    /// `MESSAGE_LEN`.
    const MESSAGE_LEN: usize = 64;

    /// The timecounter lock, then real memory for the pools and the tables (the order every
    /// test that takes both keeps).
    fn setup() -> (MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
        let t = tc_lock();
        (t, crate::kern::subr_pool::tests::setup_real_memory())
    }

    /// `pool_init(&rl_pool, sizeof(struct ratelimit_entry), 0, IPL_NONE, 0, "rl", NULL)`.
    fn rl_pool(pool: &'static Pool) {
        pool_init(
            pool,
            size_of::<RatelimitEntry>(),
            0,
            IPL_NONE,
            0,
            "rl",
            None,
        );
    }

    /// The `struct sockaddr *` of a `sockaddr_in`, as a `sockaddr_storage`.
    fn sa(sin: &SockaddrIn) -> SockaddrStorage {
        let mut ss = SockaddrStorage::zeroed();
        // SAFETY: a `sockaddr_storage` is longer than a `sockaddr_in` (asserted in the module);
        // both are made of integers.
        unsafe {
            ptr::copy_nonoverlapping(
                ptr::from_ref(sin).cast::<u8>(),
                ptr::from_mut(&mut ss).cast::<u8>(),
                size_of::<SockaddrIn>(),
            )
        };
        ss
    }

    /// The `struct sockaddr *` of a `sockaddr_in6`, as a `sockaddr_storage`.
    #[cfg(feature = "inet6")]
    fn sa6(sin6: &SockaddrIn6) -> SockaddrStorage {
        let mut ss = SockaddrStorage::zeroed();
        // SAFETY: as in `sa`, for a `sockaddr_in6`.
        unsafe {
            ptr::copy_nonoverlapping(
                ptr::from_ref(sin6).cast::<u8>(),
                ptr::from_mut(&mut ss).cast::<u8>(),
                size_of::<SockaddrIn6>(),
            )
        };
        ss
    }

    /// `struct expected_results`: the result of the constant address and the sleep before it.
    const RL_EXPECTED: [(Result<(), Errno>, i64); 11] = {
        let cost = (NSEC_PER_SEC / INITIATIONS_PER_SECOND) as i64;
        [
            (Ok(()), 0),
            (Ok(()), 0),
            (Ok(()), 0),
            (Ok(()), 0),
            (Ok(()), 0),
            (Err(Errno::ECONNREFUSED), 0),
            (Ok(()), cost),
            (Err(Errno::ECONNREFUSED), 0),
            (Ok(()), cost * 2),
            (Ok(()), 0),
            (Err(Errno::ECONNREFUSED), 0),
        ]
    };

    #[test]
    fn cookie_ratelimit_timings_test() {
        let _g = setup();
        static RL_POOL: Pool = Pool::new();
        rl_pool(&RL_POOL);
        let rl = Ratelimit::new();
        ratelimit_init(&rl, &RL_POOL).expect("ratelimit_init");

        let mut sin = SockaddrIn {
            sin_family: AF_INET,
            ..SockaddrIn::default()
        };
        #[cfg(feature = "inet6")]
        let mut sin6 = SockaddrIn6 {
            sin6_family: AF_INET6,
            ..SockaddrIn6::default()
        };

        for (i, (result, sleep_time)) in RL_EXPECTED.iter().enumerate() {
            if *sleep_time != 0 {
                advance_uptime(*sleep_time);
            }

            // The first v4 ratelimit_allow is against a constant address, and should be
            // indifferent to the port.
            sin.sin_addr.s_addr = 0x01020304;
            sin.sin_port = arc4random() as u16;

            assert_eq!(
                ratelimit_allow(&rl, &sa(&sin)),
                *result,
                "malicious v4, iter {i}"
            );

            // The second ratelimit_allow is to test that an arbitrary address is still allowed.
            sin.sin_addr.s_addr += i as u32 + 1;
            sin.sin_port = arc4random() as u16;

            assert_eq!(
                ratelimit_allow(&rl, &sa(&sin)),
                Ok(()),
                "non-malicious v4, iter {i}"
            );

            #[cfg(feature = "inet6")]
            {
                // The first v6 ratelimit_allow is against a constant address, and should be
                // indifferent to the port. We also mutate the lower 64 bits of the address as we
                // want to ensure ratelimit occurs against the higher 64 bits (/64 network).
                sin6.sin6_addr.set_s6_addr32(0, 0x01020304);
                sin6.sin6_addr.set_s6_addr32(1, 0x05060708);
                sin6.sin6_addr.set_s6_addr32(2, i as u32);
                sin6.sin6_addr.set_s6_addr32(3, i as u32);
                sin6.sin6_port = arc4random() as u16;

                assert_eq!(
                    ratelimit_allow(&rl, &sa6(&sin6)),
                    *result,
                    "malicious v6, iter {i}"
                );

                // Again, test that an address different to above is still allowed.
                sin6.sin6_addr
                    .set_s6_addr32(0, sin6.sin6_addr.s6_addr32(0) + i as u32 + 1);
                sin6.sin6_port = arc4random() as u16;

                assert_eq!(
                    ratelimit_allow(&rl, &sa6(&sin6)),
                    Ok(()),
                    "non-malicious v6, iter {i}"
                );
            }
        }
        ratelimit_deinit(&rl);
        pool_destroy(&RL_POOL);
    }

    #[test]
    fn cookie_ratelimit_capacity_test() {
        let _g = setup();
        static RL_POOL: Pool = Pool::new();
        rl_pool(&RL_POOL);
        let rl = Ratelimit::new();
        ratelimit_init(&rl, &RL_POOL).expect("ratelimit_init");

        let mut sin = SockaddrIn {
            sin_family: AF_INET,
            sin_port: 1234,
            ..SockaddrIn::default()
        };

        // Here we test that the ratelimiter has an upper bound on the number of addresses to be
        // limited
        for i in 0..=RATELIMIT_SIZE_MAX {
            sin.sin_addr.s_addr = i as u32;
            if i == RATELIMIT_SIZE_MAX {
                assert_eq!(
                    ratelimit_allow(&rl, &sa(&sin)),
                    Err(Errno::ECONNREFUSED),
                    "reject, iter {i}"
                );
            } else {
                assert_eq!(ratelimit_allow(&rl, &sa(&sin)), Ok(()), "allow, iter {i}");
            }
        }
        assert_eq!(rl.rl_table_num.get(), RATELIMIT_SIZE_MAX);
        ratelimit_deinit(&rl);
        assert_eq!(rl.rl_table_num.get(), 0);
        pool_destroy(&RL_POOL);
    }

    #[test]
    fn cookie_mac_test() {
        let _g = setup();
        static RL_POOL: Pool = Pool::new();
        let checker = CookieChecker::new();
        let maker = CookieMaker::new();
        let mut cm = CookieMacs::default();
        let mut nonce = [0u8; COOKIE_NONCE_SIZE];
        let mut cookie = [0u8; COOKIE_ENCRYPTED_SIZE];
        let mut shared = [0u8; COOKIE_INPUT_SIZE];
        let mut message = [0u8; MESSAGE_LEN];

        arc4random_buf(&mut shared);
        arc4random_buf(&mut message);

        // Init cookie_maker.
        cookie_maker_init(&maker, &shared);

        // Init cookie_checker.
        rl_pool(&RL_POOL);

        cookie_checker_init(&checker, &RL_POOL).expect("cookie_checker_allocate");
        cookie_checker_update(&checker, Some(&shared));

        // Create dummy sockaddr
        let mut sin = SockaddrIn {
            sin_family: AF_INET,
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_port: 51820,
            ..SockaddrIn::default()
        };
        sin.sin_addr.s_addr = 1;

        // MAC message
        cookie_maker_mac(&maker, &mut cm, &message);

        // Check we have a null mac2
        assert_eq!(
            cm.mac2, [0; COOKIE_MAC_SIZE],
            "validate_macs_noload_mac2_zeroed"
        );

        // Validate all bytes are checked in mac1
        for i in 0..cm.mac1.len() {
            cm.mac1[i] = !cm.mac1[i];
            assert_eq!(
                cookie_checker_validate_macs(&checker, &cm, &message, false, &sa(&sin)),
                Err(Errno::EINVAL),
                "validate_macs_noload_munge"
            );
            cm.mac1[i] = !cm.mac1[i];
        }

        // Check mac2 is zeroed
        assert!(
            cm.mac2.iter().all(|b| *b == 0),
            "validate_macs_mac2_checkzero"
        );

        // Check we can successfully validate the MAC
        assert_eq!(
            cookie_checker_validate_macs(&checker, &cm, &message, false, &sa(&sin)),
            Ok(()),
            "validate_macs_noload_normal"
        );

        // Check we get a EAGAIN if no mac2 and under load
        assert_eq!(
            cookie_checker_validate_macs(&checker, &cm, &message, true, &sa(&sin)),
            Err(Errno::EAGAIN),
            "validate_macs_load_normal"
        );

        // Simulate a cookie message
        cookie_checker_create_payload(&checker, &cm, &mut nonce, &mut cookie, &sa(&sin));

        // Validate all bytes are checked in cookie
        for i in 0..cookie.len() {
            cookie[i] = !cookie[i];
            assert_eq!(
                cookie_maker_consume_payload(&maker, &nonce, &cookie),
                Err(Errno::EINVAL),
                "consume_payload_munge"
            );
            cookie[i] = !cookie[i];
        }

        // Check we can actually consume the payload
        assert_eq!(
            cookie_maker_consume_payload(&maker, &nonce, &cookie),
            Ok(()),
            "consume_payload_normal"
        );

        // Check replay isn't allowed
        assert_eq!(
            cookie_maker_consume_payload(&maker, &nonce, &cookie),
            Err(Errno::ETIMEDOUT),
            "consume_payload_normal_replay"
        );

        // MAC message again, with MAC2
        cookie_maker_mac(&maker, &mut cm, &message);

        // Check we added a mac2
        assert!(cm.mac2.iter().any(|b| *b != 0), "validate_macs_make_mac2");

        // Check we get OK if mac2 and under load
        assert_eq!(
            cookie_checker_validate_macs(&checker, &cm, &message, true, &sa(&sin)),
            Ok(()),
            "validate_macs_load_normal_mac2"
        );

        sin.sin_addr.s_addr = !sin.sin_addr.s_addr;
        // Check we get EAGAIN if we munge the source IP
        assert_eq!(
            cookie_checker_validate_macs(&checker, &cm, &message, true, &sa(&sin)),
            Err(Errno::EAGAIN),
            "validate_macs_load_spoofip_mac2"
        );
        sin.sin_addr.s_addr = !sin.sin_addr.s_addr;

        // Check we get OK if mac2 and under load
        assert_eq!(
            cookie_checker_validate_macs(&checker, &cm, &message, true, &sa(&sin)),
            Ok(()),
            "validate_macs_load_normal_mac2_retry"
        );

        // A cookie expires COOKIE_SECRET_MAX_AGE - COOKIE_SECRET_LATENCY seconds after it came:
        // make it that old.
        let now = getnanouptime();
        maker.cp_birthdate.set(Timespec::new(
            now.tv_sec - (COOKIE_SECRET_MAX_AGE - COOKIE_SECRET_LATENCY) - 1,
            now.tv_nsec,
        ));
        cookie_maker_mac(&maker, &mut cm, &message);
        assert_eq!(cm.mac2, [0; COOKIE_MAC_SIZE], "expired cookie");

        cookie_checker_deinit(&checker);
        pool_destroy(&RL_POOL);
    }

    #[test]
    fn mac1_and_mac2_match_blake2s() {
        let _g = setup();
        let mut public = [0u8; COOKIE_INPUT_SIZE];
        for (i, b) in public.iter_mut().enumerate() {
            *b = i as u8 + 1;
        }
        let message: [u8; MESSAGE_LEN] = core::array::from_fn(|i| i as u8);

        let mut key = [0u8; COOKIE_KEY_SIZE];
        cookie_precompute_key(&mut key, &public, COOKIE_MAC1_KEY_LABEL);
        assert_eq!(
            key.to_vec(),
            hex("121b33018813efaa1d3128cdec1392897828e98831f01822c250ddfdf7090183")
        );
        cookie_precompute_key(&mut key, &public, COOKIE_COOKIE_KEY_LABEL);
        assert_eq!(
            key.to_vec(),
            hex("2f87e1b72843ca5c7d1f3d56271f4a9e22cd2c8f01c86b1b88bd8fb7d7871de1")
        );

        // The maker's mac1 under the peer's key, and mac2 under a cookie it was given.
        let maker = CookieMaker::new();
        cookie_maker_init(&maker, &public);
        let mut cm = CookieMacs::default();
        cookie_macs_mac1(&mut cm, &message, &maker.cp_mac1_key.get());
        assert_eq!(cm.mac1.to_vec(), hex("2e743ab5b23e1b0298b197f5c23943fd"));
        let cookie: [u8; COOKIE_COOKIE_SIZE] = core::array::from_fn(|i| 100 + i as u8);
        cookie_macs_mac2(&mut cm, &message, &cookie);
        assert_eq!(cm.mac2.to_vec(), hex("9ee62d59a538b2d54f1c086f5a6f3693"));

        // A checker for the same public key accepts that mac1 and refuses any other.
        let checker = CookieChecker::new();
        cookie_checker_update(&checker, Some(&public));
        let sin = SockaddrIn::default();
        assert_eq!(
            cookie_checker_validate_macs(&checker, &cm, &message, false, &sa(&sin)),
            Ok(())
        );
        cookie_checker_update(&checker, None);
        assert_eq!(
            cookie_checker_validate_macs(&checker, &cm, &message, false, &sa(&sin)),
            Err(Errno::EINVAL)
        );
    }

    #[cfg(feature = "inet6")]
    #[test]
    fn cookie_mac_test_v6() {
        let _g = setup();
        static RL_POOL: Pool = Pool::new();
        let checker = CookieChecker::new();
        let maker = CookieMaker::new();
        let mut cm = CookieMacs::default();
        let mut nonce = [0u8; COOKIE_NONCE_SIZE];
        let mut cookie = [0u8; COOKIE_ENCRYPTED_SIZE];
        let mut shared = [0u8; COOKIE_INPUT_SIZE];
        let mut message = [0u8; MESSAGE_LEN];

        arc4random_buf(&mut shared);
        arc4random_buf(&mut message);

        cookie_maker_init(&maker, &shared);
        rl_pool(&RL_POOL);
        cookie_checker_init(&checker, &RL_POOL).expect("cookie_checker_allocate");
        cookie_checker_update(&checker, Some(&shared));

        let mut sin6 = SockaddrIn6 {
            sin6_family: AF_INET6,
            sin6_len: size_of::<SockaddrIn6>() as u8,
            sin6_port: 51820,
            ..SockaddrIn6::default()
        };
        sin6.sin6_addr.s6_addr[0] = 0x20;
        sin6.sin6_addr.s6_addr[1] = 0x01;
        sin6.sin6_addr.s6_addr[15] = 1;

        cookie_maker_mac(&maker, &mut cm, &message);

        // Without load only mac1 counts.
        assert_eq!(
            cookie_checker_validate_macs(&checker, &cm, &message, false, &sa6(&sin6)),
            Ok(())
        );
        // Under load, no mac2 means a cookie.
        assert_eq!(
            cookie_checker_validate_macs(&checker, &cm, &message, true, &sa6(&sin6)),
            Err(Errno::EAGAIN)
        );

        // The cookie of an IPv6 source is made from its address and port.
        cookie_checker_create_payload(&checker, &cm, &mut nonce, &mut cookie, &sa6(&sin6));
        assert_eq!(
            cookie_maker_consume_payload(&maker, &nonce, &cookie),
            Ok(())
        );
        cookie_maker_mac(&maker, &mut cm, &message);
        assert!(cm.mac2.iter().any(|b| *b != 0));
        assert_eq!(
            cookie_checker_validate_macs(&checker, &cm, &message, true, &sa6(&sin6)),
            Ok(())
        );

        // Another address or another port is another cookie.
        let mut other = sin6;
        other.sin6_addr.s6_addr[15] = 2;
        assert_eq!(
            cookie_checker_validate_macs(&checker, &cm, &message, true, &sa6(&other)),
            Err(Errno::EAGAIN)
        );
        let mut other = sin6;
        other.sin6_port = 51821;
        assert_eq!(
            cookie_checker_validate_macs(&checker, &cm, &message, true, &sa6(&other)),
            Err(Errno::EAGAIN)
        );

        cookie_checker_deinit(&checker);
        pool_destroy(&RL_POOL);
    }

    #[cfg(feature = "inet6")]
    #[test]
    fn ratelimit_v6_counts_a_slash_64() {
        let _g = setup();
        static RL_POOL: Pool = Pool::new();
        rl_pool(&RL_POOL);
        let rl = Ratelimit::new();
        ratelimit_init(&rl, &RL_POOL).expect("ratelimit_init");

        let mut sin6 = SockaddrIn6 {
            sin6_family: AF_INET6,
            ..SockaddrIn6::default()
        };
        sin6.sin6_addr.set_s6_addr32(0, 0x01020304);
        sin6.sin6_addr.set_s6_addr32(1, 0x05060708);

        // INITIATIONS_BURSTABLE initiations pass, whatever the interface identifier or the port,
        // then the whole /64 is refused.
        for i in 0..INITIATIONS_BURSTABLE as u32 {
            sin6.sin6_addr.set_s6_addr32(3, i);
            sin6.sin6_port = i as u16;
            assert_eq!(ratelimit_allow(&rl, &sa6(&sin6)), Ok(()), "iter {i}");
        }
        sin6.sin6_addr.set_s6_addr32(3, 99);
        assert_eq!(ratelimit_allow(&rl, &sa6(&sin6)), Err(Errno::ECONNREFUSED));

        // A different /64 has its own bucket, and so does the same bits as an IPv4 address.
        sin6.sin6_addr.set_s6_addr32(1, 0x05060709);
        assert_eq!(ratelimit_allow(&rl, &sa6(&sin6)), Ok(()));
        assert_eq!(rl.rl_table_num.get(), 2);

        // An address that is neither IPv4 nor IPv6 is refused.
        let mut ss = sa6(&sin6);
        ss.ss_family = AF_UNSPEC;
        assert_eq!(ratelimit_allow(&rl, &ss), Err(Errno::ECONNREFUSED));

        ratelimit_deinit(&rl);
        pool_destroy(&RL_POOL);
    }
}
/* </TESTS> */
