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
//! The Noise protocol of WireGuard (`Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s`):
//! `<net/wg_noise.h>` and `net/wg_noise.c`.
//!
//! Upstream: sys/net/wg_noise.h @ 3ce1f3f79392
//! Upstream: sys/net/wg_noise.c @ 3ce1f3f79392
//!
//! A [`NoiseLocal`] holds the interface's static key pair; a [`NoiseRemote`] holds a peer's
//! static public key, the pre-computed static-static Diffie-Hellman (`r_ss`), the state of a
//! handshake in progress and up to three transport key pairs (next, current, previous). The
//! initiator calls [`noise_create_initiation`], the responder [`noise_consume_initiation`] and
//! [`noise_create_response`], the initiator [`noise_consume_response`]; both then derive the
//! transport keys with [`noise_remote_begin_session`] and move data with
//! [`noise_remote_encrypt`] and [`noise_remote_decrypt`]. The responder's new key pair waits in
//! `r_next` until the first data packet under it confirms the session.
//!
//! The local side tells its owner (`if_wg.c`) about remotes and indices through the upcalls
//! of [`NoiseUpcall`]: find the remote of a public key, allocate a session index for a remote,
//! and drop one.
//!
//! Locks: `l_identity_lock` (the local key pair, and `r_ss` of every remote, which depends on
//! it), `r_handshake_lock` (the handshake, `r_psk`, `r_timestamp`, `r_last_init`),
//! `r_keypair_mtx` (the key pairs and their list), `c_mtx` (the receive window of a counter).
//!
//! ## Deviations
//! - The header and the file share this module.
//! - Members the C changes through a shared pointer under the locks above are `Cell`s; the
//!   all-zero value of every structure is valid (`bzero`, `M_ZERO`). `noise_local_init` and
//!   `noise_remote_init` reset each member where the C `bzero`s the structure.
//! - `struct noise_upcall`'s functions take the remote as `&'static NoiseRemote` (a remote is
//!   embedded in a pool-allocated peer that outlives its indices); `l_upcall` is
//!   `Cell<Option<NoiseUpcall>>`, so a zero-filled local has none: calling through it panics
//!   where the C would call through NULL.
//! - `r_local` is `Cell<Option<&'static NoiseLocal>>`, `r_next`/`r_current`/`r_previous` are
//!   `Cell<Option<&'static NoiseKeypair>>` pointing into the remote's own `r_keypair`; the
//!   functions that store such references take `&'static NoiseRemote`.
//! - `noise_consume_initiation` returns the remote (`Result<&'static NoiseRemote, Errno>`)
//!   instead of filling `*rp` on success.
//! - `noise_remote_encrypt`/`noise_remote_decrypt` keep the C's results: `Err(ESTALE)` and
//!   `Err(ECONNRESET)` are successes that ask for a new handshake or report a confirmed
//!   session; the buffer was encrypted or decrypted. The buffer is a slice: `encrypt`
//!   encrypts `buf[..buflen]` and appends the tag (`buf` holds `buflen + 16` bytes),
//!   `decrypt` takes the ciphertext and tag as the whole slice.
//! - `noise_kdf`'s outputs are optional slices whose lengths are `a_len`, `b_len` and
//!   `c_len`; its chaining key is read from a copy, as the C reads `ck` before it writes `a`
//!   (the callers pass `ck` as both). `noise_msg_encrypt` encrypts in place: the callers copy
//!   the plaintext into the destination first (`l_public` into `es`; `ets` and the empty
//!   `en` already are). `noise_timer_expired` answers `bool` (`ETIMEDOUT` is `true`).
//! - `COUNTER_BITS` is `unsigned long`'s width, 64 on both LP64 targets; `c_send` and `c_recv`
//!   are atomics (`c_send` is the C's `atomic_inc_long_nv`; `c_recv` is read without `c_mtx`
//!   in the places the C accepts a racy read).
//! - The struct copies `*r->r_current = kp` and `r->r_handshake = hs` are [`NoiseKeypair`]'s
//!   `assign` and `Cell::set`; `explicit_bzero` of a handshake or key pair is an assignment of
//!   zeros (`crate::crypto::wipe` for locals).
//! - A remote has three key pairs and at most three are in use, so
//!   `noise_remote_keypair_allocate` never finds its list empty; if it did it panics where the
//!   C would dereference NULL.
//! - `WGTEST`'s `noise_test` (the counter, handshake and speed tests) is the host test module.

use core::cell::Cell;
use core::ffi::{c_ulong, c_void};
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

use libkern::{explicit_bzero, timingsafe_bcmp};

use crate::crypto::blake2s::{
    BLAKE2S_HASH_SIZE, Blake2sState, blake2s, blake2s_final, blake2s_hmac, blake2s_hmac_inplace,
    blake2s_init, blake2s_update,
};
use crate::crypto::chachapoly::{
    CHACHA20POLY1305_AUTHTAG_SIZE, CHACHA20POLY1305_KEY_SIZE, chacha20poly1305_decrypt,
    chacha20poly1305_decrypt_inplace, chacha20poly1305_encrypt_inplace,
};
use crate::crypto::curve25519::{
    CURVE25519_KEY_SIZE, curve25519, curve25519_clamp_secret, curve25519_generate_public,
    curve25519_generate_secret,
};
use crate::crypto::wipe;
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_init_flags, mtx_leave};
use crate::kern::kern_rwlock::{
    rw_assert_wrlock, rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write, rw_init,
};
use crate::kern::kern_tc::{getnanotime, getnanouptime};
use crate::kern::subr_prf::panic;
use crate::machine::intr::IPL_NET;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::mutex::Mutex;
use crate::sys::queue::{SlistEntry, SlistHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::time::{Timespec, timespecadd};
use crate::sys::types::Time;

/// `NOISE_PUBLIC_KEY_LEN`.
pub const NOISE_PUBLIC_KEY_LEN: usize = CURVE25519_KEY_SIZE;
/// `NOISE_SYMMETRIC_KEY_LEN`.
pub const NOISE_SYMMETRIC_KEY_LEN: usize = CHACHA20POLY1305_KEY_SIZE;
/// `NOISE_TIMESTAMP_LEN`: a TAI64N label, `sizeof(uint64_t) + sizeof(uint32_t)`.
pub const NOISE_TIMESTAMP_LEN: usize = size_of::<u64>() + size_of::<u32>();
/// `NOISE_AUTHTAG_LEN`.
pub const NOISE_AUTHTAG_LEN: usize = CHACHA20POLY1305_AUTHTAG_SIZE;
/// `NOISE_HASH_LEN`.
pub const NOISE_HASH_LEN: usize = BLAKE2S_HASH_SIZE;

// Protocol string constants

/// `NOISE_HANDSHAKE_NAME`: the Noise protocol name, the construction string.
pub const NOISE_HANDSHAKE_NAME: &[u8] = b"Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s";
/// `NOISE_IDENTIFIER_NAME`: the WireGuard identifier.
pub const NOISE_IDENTIFIER_NAME: &[u8] = b"WireGuard v1 zx2c4 Jason@zx2c4.com";

// Constants for the counter

/// `COUNTER_BITS_TOTAL`.
pub const COUNTER_BITS_TOTAL: u64 = 8192;
/// `COUNTER_BITS`: the bits of one `unsigned long` word of the window.
pub const COUNTER_BITS: u64 = c_ulong::BITS as u64;
/// `COUNTER_NUM`: the words of the window.
pub const COUNTER_NUM: usize = (COUNTER_BITS_TOTAL / COUNTER_BITS) as usize;
/// `COUNTER_WINDOW_SIZE`.
pub const COUNTER_WINDOW_SIZE: u64 = COUNTER_BITS_TOTAL - COUNTER_BITS;

// Constants for the keypair

/// `REKEY_AFTER_MESSAGES`.
pub const REKEY_AFTER_MESSAGES: u64 = 1 << 60;
/// `REJECT_AFTER_MESSAGES`.
pub const REJECT_AFTER_MESSAGES: u64 = u64::MAX - COUNTER_WINDOW_SIZE - 1;
/// `REKEY_AFTER_TIME` (seconds).
pub const REKEY_AFTER_TIME: Time = 120;
/// `REKEY_AFTER_TIME_RECV` (seconds).
pub const REKEY_AFTER_TIME_RECV: Time = 165;
/// `REJECT_AFTER_TIME` (seconds).
pub const REJECT_AFTER_TIME: Time = 180;
/// `REJECT_INTERVAL` (nanoseconds): fifty times per sec.
pub const REJECT_INTERVAL: i64 = 1_000_000_000 / 50;
/// `REJECT_INTERVAL_MASK`: 24 = floor(log2(`REJECT_INTERVAL`)).
pub const REJECT_INTERVAL_MASK: u64 = !((1u64 << 24) - 1);

/// `enum noise_state_hs`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoiseStateHs {
    /// `HS_ZEROED`.
    #[default]
    HsZeroed = 0,
    /// `CREATED_INITIATION`.
    CreatedInitiation,
    /// `CONSUMED_INITIATION`.
    ConsumedInitiation,
    /// `CREATED_RESPONSE`.
    CreatedResponse,
    /// `CONSUMED_RESPONSE`.
    ConsumedResponse,
}

/// `struct noise_handshake`: the state of a handshake in progress.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoiseHandshake {
    /// `hs_state`.
    pub hs_state: NoiseStateHs,
    /// `hs_local_index`.
    pub hs_local_index: u32,
    /// `hs_remote_index`.
    pub hs_remote_index: u32,
    /// `hs_e`: our ephemeral private key (initiator) or the peer's ephemeral public key
    /// (responder).
    pub hs_e: [u8; NOISE_PUBLIC_KEY_LEN],
    /// `hs_hash`: the handshake hash.
    pub hs_hash: [u8; NOISE_HASH_LEN],
    /// `hs_ck`: the chaining key.
    pub hs_ck: [u8; NOISE_HASH_LEN],
}

/// `struct noise_counter`: a key pair's send counter and replay window.
pub struct NoiseCounter {
    /// `c_mtx`.
    pub c_mtx: Mutex,
    /// `c_send`: the next nonce to send.
    pub c_send: AtomicU64,
    /// `c_recv`: the highest nonce received. Written under `c_mtx`.
    pub c_recv: AtomicU64,
    /// `c_backtrack`: the replay bitmap. Protected by: `c_mtx`.
    pub c_backtrack: [Cell<c_ulong>; COUNTER_NUM],
}

impl NoiseCounter {
    /// The `bzero` of a counter: everything zero, the mutex not yet initialised.
    pub const fn new() -> Self {
        Self {
            c_mtx: Mutex::new(0),
            c_send: AtomicU64::new(0),
            c_recv: AtomicU64::new(0),
            c_backtrack: [const { Cell::new(0) }; COUNTER_NUM],
        }
    }

    /// `bzero(&ctr, sizeof(ctr)); mtx_init_flags(&ctr.c_mtx, IPL_NET, name, 0)`.
    pub fn reset(&self, name: &'static str) {
        self.c_send.store(0, Ordering::Relaxed);
        self.c_recv.store(0, Ordering::Relaxed);
        for w in &self.c_backtrack {
            w.set(0);
        }
        mtx_init_flags(&self.c_mtx, IPL_NET, Some(name), 0);
    }
}

impl Default for NoiseCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct noise_keypair`: a pair of transport keys. Protected by: the remote's
/// `r_keypair_mtx` (but `kp_ctr`, which has its own lock).
pub struct NoiseKeypair {
    /// `kp_entry`: the link in `r_unused_keypairs`.
    pub kp_entry: SlistEntry<NoiseKeypair>,
    /// `kp_valid`.
    pub kp_valid: Cell<bool>,
    /// `kp_is_initiator`.
    pub kp_is_initiator: Cell<bool>,
    /// `kp_local_index`.
    pub kp_local_index: Cell<u32>,
    /// `kp_remote_index`.
    pub kp_remote_index: Cell<u32>,
    /// `kp_send`.
    pub kp_send: Cell<[u8; NOISE_SYMMETRIC_KEY_LEN]>,
    /// `kp_recv`.
    pub kp_recv: Cell<[u8; NOISE_SYMMETRIC_KEY_LEN]>,
    /// `kp_birthdate`: nanouptime.
    pub kp_birthdate: Cell<Timespec>,
    /// `kp_ctr`.
    pub kp_ctr: NoiseCounter,
}

impl NoiseKeypair {
    /// An all-zero key pair.
    pub const fn new() -> Self {
        Self {
            kp_entry: SlistEntry::new(),
            kp_valid: Cell::new(false),
            kp_is_initiator: Cell::new(false),
            kp_local_index: Cell::new(0),
            kp_remote_index: Cell::new(0),
            kp_send: Cell::new([0; NOISE_SYMMETRIC_KEY_LEN]),
            kp_recv: Cell::new([0; NOISE_SYMMETRIC_KEY_LEN]),
            kp_birthdate: Cell::new(Timespec::new(0, 0)),
            kp_ctr: NoiseCounter::new(),
        }
    }

    /// `*kp = new`: the struct copy of `noise_remote_begin_session`, from the values the C
    /// builds in a stack `struct noise_keypair` (its counter freshly zeroed and initialised).
    fn assign(&self, new: &KeypairInit) {
        self.kp_valid.set(new.kp_valid);
        self.kp_is_initiator.set(new.kp_is_initiator);
        self.kp_local_index.set(new.kp_local_index);
        self.kp_remote_index.set(new.kp_remote_index);
        self.kp_send.set(new.kp_send);
        self.kp_recv.set(new.kp_recv);
        self.kp_birthdate.set(new.kp_birthdate);
        self.kp_ctr.reset("noise_counter");
    }
}

impl Default for NoiseKeypair {
    fn default() -> Self {
        Self::new()
    }
}

/// The members of the stack `struct noise_keypair kp` in `noise_remote_begin_session`.
#[derive(Default)]
struct KeypairInit {
    kp_valid: bool,
    kp_is_initiator: bool,
    kp_local_index: u32,
    kp_remote_index: u32,
    kp_send: [u8; NOISE_SYMMETRIC_KEY_LEN],
    kp_recv: [u8; NOISE_SYMMETRIC_KEY_LEN],
    kp_birthdate: Timespec,
}

queue_adapter!(
    /// `SLIST_HEAD(,noise_keypair)`: `r_unused_keypairs`.
    pub NoiseKeypairs: NoiseKeypair, kp_entry => SlistEntry<NoiseKeypair>
);

/// `u_remote_get`: the remote whose static public key is the argument, if the owner knows one.
pub type NoiseRemoteGetFn =
    fn(*mut c_void, &[u8; NOISE_PUBLIC_KEY_LEN]) -> Option<&'static NoiseRemote>;
/// `u_index_set`: allocates a session index for the remote's handshake.
pub type NoiseIndexSetFn = fn(*mut c_void, &'static NoiseRemote) -> u32;
/// `u_index_drop`: frees a session index.
pub type NoiseIndexDropFn = fn(*mut c_void, u32);

/// `struct noise_upcall`: what the local side asks its owner.
#[derive(Clone, Copy, Debug)]
pub struct NoiseUpcall {
    /// `u_arg`: the owner's argument to every upcall.
    pub u_arg: *mut c_void,
    /// `u_remote_get`.
    pub u_remote_get: NoiseRemoteGetFn,
    /// `u_index_set`.
    pub u_index_set: NoiseIndexSetFn,
    /// `u_index_drop`.
    pub u_index_drop: NoiseIndexDropFn,
}

/// `struct noise_remote`: a peer as Noise sees it.
pub struct NoiseRemote {
    /// `r_public`: the peer's static public key. Immutable after `noise_remote_init`.
    pub r_public: Cell<[u8; NOISE_PUBLIC_KEY_LEN]>,
    /// `r_local`. Immutable after `noise_remote_init`.
    pub r_local: Cell<Option<&'static NoiseLocal>>,
    /// `r_ss`: the static-static DH. Protected by: the local's `l_identity_lock`.
    pub r_ss: Cell<[u8; NOISE_PUBLIC_KEY_LEN]>,

    /// `r_handshake_lock`.
    pub r_handshake_lock: Rwlock,
    /// `r_handshake`. Protected by: `r_handshake_lock`.
    pub r_handshake: Cell<NoiseHandshake>,
    /// `r_psk`. Protected by: `r_handshake_lock`.
    pub r_psk: Cell<[u8; NOISE_SYMMETRIC_KEY_LEN]>,
    /// `r_timestamp`: the newest initiation timestamp. Protected by: `r_handshake_lock`.
    pub r_timestamp: Cell<[u8; NOISE_TIMESTAMP_LEN]>,
    /// `r_last_init`: nanouptime. Protected by: `r_handshake_lock`.
    pub r_last_init: Cell<Timespec>,

    /// `r_keypair_mtx`.
    pub r_keypair_mtx: Mutex,
    /// `r_unused_keypairs`. Protected by: `r_keypair_mtx`.
    pub r_unused_keypairs: SlistHead<NoiseKeypairs>,
    /// `r_next`. Protected by: `r_keypair_mtx`.
    pub r_next: Cell<Option<&'static NoiseKeypair>>,
    /// `r_current`. Protected by: `r_keypair_mtx`.
    pub r_current: Cell<Option<&'static NoiseKeypair>>,
    /// `r_previous`. Protected by: `r_keypair_mtx`.
    pub r_previous: Cell<Option<&'static NoiseKeypair>>,
    /// `r_keypair`: 3: next, current, previous.
    pub r_keypair: [NoiseKeypair; 3],
}

// SAFETY: every member is either immutable after `noise_remote_init` or changed under the lock
// its doc comment names, as in the C.
unsafe impl Sync for NoiseRemote {}

impl NoiseRemote {
    /// An all-zero remote, what `bzero` leaves.
    pub const fn new() -> Self {
        Self {
            r_public: Cell::new([0; NOISE_PUBLIC_KEY_LEN]),
            r_local: Cell::new(None),
            r_ss: Cell::new([0; NOISE_PUBLIC_KEY_LEN]),
            r_handshake_lock: Rwlock::new("noise_handshake"),
            r_handshake: Cell::new(NoiseHandshake {
                hs_state: NoiseStateHs::HsZeroed,
                hs_local_index: 0,
                hs_remote_index: 0,
                hs_e: [0; NOISE_PUBLIC_KEY_LEN],
                hs_hash: [0; NOISE_HASH_LEN],
                hs_ck: [0; NOISE_HASH_LEN],
            }),
            r_psk: Cell::new([0; NOISE_SYMMETRIC_KEY_LEN]),
            r_timestamp: Cell::new([0; NOISE_TIMESTAMP_LEN]),
            r_last_init: Cell::new(Timespec::new(0, 0)),
            r_keypair_mtx: Mutex::new(0),
            r_unused_keypairs: SlistHead::new(),
            r_next: Cell::new(None),
            r_current: Cell::new(None),
            r_previous: Cell::new(None),
            r_keypair: [const { NoiseKeypair::new() }; 3],
        }
    }

    /// `r->r_local`, set by `noise_remote_init`.
    fn local(&self) -> &'static NoiseLocal {
        match self.r_local.get() {
            Some(l) => l,
            None => panic(format_args!("noise_remote {:p}: no local", self)),
        }
    }
}

impl Default for NoiseRemote {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct noise_local`: our static identity.
pub struct NoiseLocal {
    /// `l_identity_lock`.
    pub l_identity_lock: Rwlock,
    /// `l_has_identity`. Protected by: `l_identity_lock`.
    pub l_has_identity: Cell<bool>,
    /// `l_public`. Protected by: `l_identity_lock`.
    pub l_public: Cell<[u8; NOISE_PUBLIC_KEY_LEN]>,
    /// `l_private`. Protected by: `l_identity_lock`.
    pub l_private: Cell<[u8; NOISE_PUBLIC_KEY_LEN]>,
    /// `l_upcall`. Immutable after `noise_local_init`.
    pub l_upcall: Cell<Option<NoiseUpcall>>,
}

// SAFETY: the identity changes under `l_identity_lock`; the upcall is set once by
// `noise_local_init` before the local is shared.
unsafe impl Sync for NoiseLocal {}

impl NoiseLocal {
    /// An all-zero local, what `bzero` leaves.
    pub const fn new() -> Self {
        Self {
            l_identity_lock: Rwlock::new("noise_local_identity"),
            l_has_identity: Cell::new(false),
            l_public: Cell::new([0; NOISE_PUBLIC_KEY_LEN]),
            l_private: Cell::new([0; NOISE_PUBLIC_KEY_LEN]),
            l_upcall: Cell::new(None),
        }
    }

    /// `&l->l_upcall`.
    fn upcall(&self) -> NoiseUpcall {
        match self.l_upcall.get() {
            Some(u) => u,
            None => panic(format_args!("noise_local {:p}: no upcall", self)),
        }
    }
}

impl Default for NoiseLocal {
    fn default() -> Self {
        Self::new()
    }
}

// Set/Get noise parameters

/// `noise_local_init`: a local without identity that calls `upcall`.
pub fn noise_local_init(l: &NoiseLocal, upcall: &NoiseUpcall) {
    // bzero(l, sizeof(*l))
    l.l_has_identity.set(false);
    l.l_public.set([0; NOISE_PUBLIC_KEY_LEN]);
    l.l_private.set([0; NOISE_PUBLIC_KEY_LEN]);
    rw_init(&l.l_identity_lock, "noise_local_identity");
    l.l_upcall.set(Some(*upcall));
}

/// `noise_local_lock_identity`.
pub fn noise_local_lock_identity(l: &NoiseLocal) {
    rw_enter_write(&l.l_identity_lock);
}

/// `noise_local_unlock_identity`.
pub fn noise_local_unlock_identity(l: &NoiseLocal) {
    rw_exit_write(&l.l_identity_lock);
}

/// `noise_local_set_private`: our static private key (clamped) and its public key; `ENXIO`
/// when the key has no public key (all zero). Called with the identity locked.
pub fn noise_local_set_private(
    l: &NoiseLocal,
    private: &[u8; NOISE_PUBLIC_KEY_LEN],
) -> Result<(), Errno> {
    rw_assert_wrlock(&l.l_identity_lock);

    let mut clamped = *private;
    curve25519_clamp_secret(&mut clamped);
    l.l_private.set(clamped);
    explicit_bzero(&mut clamped);
    let mut public = [0u8; NOISE_PUBLIC_KEY_LEN];
    let has_identity = curve25519_generate_public(&mut public, private);
    l.l_public.set(public);
    l.l_has_identity.set(has_identity);

    if has_identity {
        Ok(())
    } else {
        Err(Errno::ENXIO)
    }
}

/// `noise_local_keys`: copies out our public and private keys; `ENXIO` without an identity.
pub fn noise_local_keys(
    l: &NoiseLocal,
    public: Option<&mut [u8; NOISE_PUBLIC_KEY_LEN]>,
    private: Option<&mut [u8; NOISE_PUBLIC_KEY_LEN]>,
) -> Result<(), Errno> {
    let mut ret = Ok(());
    rw_enter_read(&l.l_identity_lock);
    if l.l_has_identity.get() {
        if let Some(public) = public {
            *public = l.l_public.get();
        }
        if let Some(private) = private {
            *private = l.l_private.get();
        }
    } else {
        ret = Err(Errno::ENXIO);
    }
    rw_exit_read(&l.l_identity_lock);
    ret
}

/// `noise_remote_init`: a remote for the peer with static key `public`, seen from `l`.
pub fn noise_remote_init(
    r: &'static NoiseRemote,
    public: &[u8; NOISE_PUBLIC_KEY_LEN],
    l: &'static NoiseLocal,
) {
    // bzero(r, sizeof(*r))
    r.r_ss.set([0; NOISE_PUBLIC_KEY_LEN]);
    r.r_handshake.set(NoiseHandshake::default());
    r.r_psk.set([0; NOISE_SYMMETRIC_KEY_LEN]);
    r.r_timestamp.set([0; NOISE_TIMESTAMP_LEN]);
    r.r_last_init.set(Timespec::default());
    r.r_next.set(None);
    r.r_current.set(None);
    r.r_previous.set(None);
    r.r_unused_keypairs.init();
    for kp in &r.r_keypair {
        kp.assign(&KeypairInit::default());
    }

    r.r_public.set(*public);
    rw_init(&r.r_handshake_lock, "noise_handshake");
    mtx_init_flags(&r.r_keypair_mtx, IPL_NET, Some("noise_keypair"), 0);

    for kp in &r.r_keypair {
        // SAFETY: the list was just emptied and each key pair is inserted once.
        unsafe { r.r_unused_keypairs.insert_head(kp) };
    }

    // KASSERT(l != NULL): a reference.
    r.r_local.set(Some(l));

    rw_enter_write(&l.l_identity_lock);
    noise_remote_precompute(r);
    rw_exit_write(&l.l_identity_lock);
}

/// `noise_remote_set_psk`: sets the pre-shared key; `EEXIST` when it is the one already set.
pub fn noise_remote_set_psk(
    r: &NoiseRemote,
    psk: &[u8; NOISE_SYMMETRIC_KEY_LEN],
) -> Result<(), Errno> {
    rw_enter_write(&r.r_handshake_lock);
    let same = !timingsafe_bcmp(&r.r_psk.get(), psk);
    if !same {
        r.r_psk.set(*psk);
    }
    rw_exit_write(&r.r_handshake_lock);
    if same { Err(Errno::EEXIST) } else { Ok(()) }
}

/// `noise_remote_keys`: copies out the peer's public key and the pre-shared key; `ENOENT`
/// when there is no pre-shared key (it is all zero).
pub fn noise_remote_keys(
    r: &NoiseRemote,
    public: Option<&mut [u8; NOISE_PUBLIC_KEY_LEN]>,
    psk: Option<&mut [u8; NOISE_SYMMETRIC_KEY_LEN]>,
) -> Result<(), Errno> {
    static NULL_PSK: [u8; NOISE_SYMMETRIC_KEY_LEN] = [0; NOISE_SYMMETRIC_KEY_LEN];

    if let Some(public) = public {
        *public = r.r_public.get();
    }

    rw_enter_read(&r.r_handshake_lock);
    let mut r_psk = r.r_psk.get();
    if let Some(psk) = psk {
        *psk = r_psk;
    }
    let ret = timingsafe_bcmp(&r_psk, &NULL_PSK);
    explicit_bzero(&mut r_psk);
    rw_exit_read(&r.r_handshake_lock);

    // If r_psk != null_psk return 0, else ENOENT (no psk)
    if ret { Ok(()) } else { Err(Errno::ENOENT) }
}

/// `noise_remote_precompute`: the static-static DH with the current identity, and the end of
/// any handshake in progress. Should be called anytime `noise_local_set_private` is called,
/// with the identity locked.
pub fn noise_remote_precompute(r: &NoiseRemote) {
    let l = r.local();
    rw_assert_wrlock(&l.l_identity_lock);
    let mut ss = [0u8; NOISE_PUBLIC_KEY_LEN];
    if !l.l_has_identity.get() || !curve25519(&mut ss, &l.l_private.get(), &r.r_public.get()) {
        explicit_bzero(&mut ss);
    }
    r.r_ss.set(ss);
    explicit_bzero(&mut ss);

    rw_enter_write(&r.r_handshake_lock);
    noise_remote_handshake_index_drop(r);
    r.r_handshake.set(NoiseHandshake::default());
    rw_exit_write(&r.r_handshake_lock);
}

// Handshake functions

/// `noise_create_initiation`: the initiator's first message: our ephemeral public key `ue`,
/// our static public key encrypted (`es`) and the encrypted timestamp (`ets`); `s_idx` is the
/// session index the owner allocated.
pub fn noise_create_initiation(
    r: &'static NoiseRemote,
    s_idx: &mut u32,
    ue: &mut [u8; NOISE_PUBLIC_KEY_LEN],
    es: &mut [u8; NOISE_PUBLIC_KEY_LEN + NOISE_AUTHTAG_LEN],
    ets: &mut [u8; NOISE_TIMESTAMP_LEN + NOISE_AUTHTAG_LEN],
) -> Result<(), Errno> {
    let l = r.local();
    let mut key = [0u8; NOISE_SYMMETRIC_KEY_LEN];

    rw_enter_read(&l.l_identity_lock);
    rw_enter_write(&r.r_handshake_lock);
    // hs = &r->r_handshake: changed in a copy, stored back before the lock is released.
    let mut hs = r.r_handshake.get();
    let ret = 'error: {
        if !l.l_has_identity.get() {
            break 'error Err(Errno::EINVAL);
        }
        noise_param_init(&mut hs.hs_ck, &mut hs.hs_hash, &r.r_public.get());

        // e
        curve25519_generate_secret(&mut hs.hs_e);
        if !curve25519_generate_public(ue, &hs.hs_e) {
            break 'error Err(Errno::EINVAL);
        }
        noise_msg_ephemeral(&mut hs.hs_ck, &mut hs.hs_hash, ue);

        // es
        if noise_mix_dh(&mut hs.hs_ck, Some(&mut key), &hs.hs_e, &r.r_public.get()).is_err() {
            break 'error Err(Errno::EINVAL);
        }

        // s
        es[..NOISE_PUBLIC_KEY_LEN].copy_from_slice(&l.l_public.get());
        noise_msg_encrypt(es, NOISE_PUBLIC_KEY_LEN, &key, &mut hs.hs_hash);

        // ss
        if noise_mix_ss(&mut hs.hs_ck, &mut key, &r.r_ss.get()).is_err() {
            break 'error Err(Errno::EINVAL);
        }

        // {t}
        let mut t = [0u8; NOISE_TIMESTAMP_LEN];
        noise_tai64n_now(&mut t);
        ets[..NOISE_TIMESTAMP_LEN].copy_from_slice(&t);
        noise_msg_encrypt(ets, NOISE_TIMESTAMP_LEN, &key, &mut hs.hs_hash);

        noise_remote_handshake_index_drop(r);
        hs.hs_state = NoiseStateHs::CreatedInitiation;
        hs.hs_local_index = noise_remote_handshake_index_get(r);
        *s_idx = hs.hs_local_index;
        Ok(())
    };
    r.r_handshake.set(hs);
    wipe(&mut hs);
    rw_exit_write(&r.r_handshake_lock);
    rw_exit_read(&l.l_identity_lock);
    explicit_bzero(&mut key);
    ret
}

/// `noise_consume_initiation`: the responder's processing of an initiation: finds the remote
/// whose static key `es` carries (through `u_remote_get`), checks the timestamp against
/// replays and floods, and keeps the handshake for `noise_create_response`.
pub fn noise_consume_initiation(
    l: &NoiseLocal,
    s_idx: u32,
    ue: &[u8; NOISE_PUBLIC_KEY_LEN],
    es: &[u8; NOISE_PUBLIC_KEY_LEN + NOISE_AUTHTAG_LEN],
    ets: &[u8; NOISE_TIMESTAMP_LEN + NOISE_AUTHTAG_LEN],
) -> Result<&'static NoiseRemote, Errno> {
    let mut hs = NoiseHandshake::default();
    let mut key = [0u8; NOISE_SYMMETRIC_KEY_LEN];
    let mut r_public = [0u8; NOISE_PUBLIC_KEY_LEN];
    let mut timestamp = [0u8; NOISE_TIMESTAMP_LEN];

    rw_enter_read(&l.l_identity_lock);
    let ret = 'error: {
        if !l.l_has_identity.get() {
            break 'error Err(Errno::EINVAL);
        }
        noise_param_init(&mut hs.hs_ck, &mut hs.hs_hash, &l.l_public.get());

        // e
        noise_msg_ephemeral(&mut hs.hs_ck, &mut hs.hs_hash, ue);

        // es
        if noise_mix_dh(&mut hs.hs_ck, Some(&mut key), &l.l_private.get(), ue).is_err() {
            break 'error Err(Errno::EINVAL);
        }

        // s
        if noise_msg_decrypt(&mut r_public, es, &key, &mut hs.hs_hash).is_err() {
            break 'error Err(Errno::EINVAL);
        }

        // Lookup the remote we received from
        let u = l.upcall();
        let Some(r) = (u.u_remote_get)(u.u_arg, &r_public) else {
            break 'error Err(Errno::EINVAL);
        };

        // ss
        if noise_mix_ss(&mut hs.hs_ck, &mut key, &r.r_ss.get()).is_err() {
            break 'error Err(Errno::EINVAL);
        }

        // {t}
        if noise_msg_decrypt(&mut timestamp, ets, &key, &mut hs.hs_hash).is_err() {
            break 'error Err(Errno::EINVAL);
        }

        hs.hs_e = *ue;

        // We have successfully computed the same results, now we ensure that this is not an
        // initiation replay, or a flood attack
        rw_enter_write(&r.r_handshake_lock);
        let ret = 'error_set: {
            // Replay
            if timestamp > r.r_timestamp.get() {
                r.r_timestamp.set(timestamp);
            } else {
                break 'error_set Err(Errno::EINVAL);
            }
            // Flood attack
            if noise_timer_expired(&r.r_last_init.get(), 0, REJECT_INTERVAL) {
                r.r_last_init.set(getnanouptime());
            } else {
                break 'error_set Err(Errno::EINVAL);
            }

            // Ok, we're happy to accept this initiation now
            noise_remote_handshake_index_drop(r);
            hs.hs_state = NoiseStateHs::ConsumedInitiation;
            hs.hs_local_index = noise_remote_handshake_index_get(r);
            hs.hs_remote_index = s_idx;
            r.r_handshake.set(hs);
            Ok(r)
        };
        rw_exit_write(&r.r_handshake_lock);
        ret
    };
    rw_exit_read(&l.l_identity_lock);
    explicit_bzero(&mut key);
    wipe(&mut hs);
    ret
}

/// `noise_create_response`: the responder's message after a consumed initiation: our
/// ephemeral public key `ue` and the empty encrypted payload `en`; `s_idx` is our session
/// index, `r_idx` the initiator's.
pub fn noise_create_response(
    r: &NoiseRemote,
    s_idx: &mut u32,
    r_idx: &mut u32,
    ue: &mut [u8; NOISE_PUBLIC_KEY_LEN],
    en: &mut [u8; NOISE_AUTHTAG_LEN],
) -> Result<(), Errno> {
    let l = r.local();
    let mut key = [0u8; NOISE_SYMMETRIC_KEY_LEN];
    let mut e = [0u8; NOISE_PUBLIC_KEY_LEN];

    rw_enter_read(&l.l_identity_lock);
    rw_enter_write(&r.r_handshake_lock);
    // hs = &r->r_handshake: changed in a copy, stored back before the lock is released.
    let mut hs = r.r_handshake.get();
    let ret = 'error: {
        if hs.hs_state != NoiseStateHs::ConsumedInitiation {
            break 'error Err(Errno::EINVAL);
        }

        // e
        curve25519_generate_secret(&mut e);
        if !curve25519_generate_public(ue, &e) {
            break 'error Err(Errno::EINVAL);
        }
        noise_msg_ephemeral(&mut hs.hs_ck, &mut hs.hs_hash, ue);

        // ee
        if noise_mix_dh(&mut hs.hs_ck, None, &e, &hs.hs_e).is_err() {
            break 'error Err(Errno::EINVAL);
        }

        // se
        if noise_mix_dh(&mut hs.hs_ck, None, &e, &r.r_public.get()).is_err() {
            break 'error Err(Errno::EINVAL);
        }

        // psk
        noise_mix_psk(&mut hs.hs_ck, &mut hs.hs_hash, &mut key, &r.r_psk.get());

        // {}
        noise_msg_encrypt(en, 0, &key, &mut hs.hs_hash);

        hs.hs_state = NoiseStateHs::CreatedResponse;
        *r_idx = hs.hs_remote_index;
        *s_idx = hs.hs_local_index;
        Ok(())
    };
    r.r_handshake.set(hs);
    wipe(&mut hs);
    rw_exit_write(&r.r_handshake_lock);
    rw_exit_read(&l.l_identity_lock);
    explicit_bzero(&mut key);
    explicit_bzero(&mut e);
    ret
}

/// `noise_consume_response`: the initiator's processing of a response to the initiation whose
/// index is `r_idx`; `s_idx` is the responder's index.
pub fn noise_consume_response(
    r: &NoiseRemote,
    s_idx: u32,
    r_idx: u32,
    ue: &[u8; NOISE_PUBLIC_KEY_LEN],
    en: &[u8; NOISE_AUTHTAG_LEN],
) -> Result<(), Errno> {
    let l = r.local();
    let mut hs = NoiseHandshake::default();
    let mut key = [0u8; NOISE_SYMMETRIC_KEY_LEN];
    let mut preshared_key = [0u8; NOISE_PUBLIC_KEY_LEN];

    rw_enter_read(&l.l_identity_lock);
    let ret = 'error: {
        if !l.l_has_identity.get() {
            break 'error Err(Errno::EINVAL);
        }

        rw_enter_read(&r.r_handshake_lock);
        hs = r.r_handshake.get();
        preshared_key = r.r_psk.get();
        rw_exit_read(&r.r_handshake_lock);

        if hs.hs_state != NoiseStateHs::CreatedInitiation || hs.hs_local_index != r_idx {
            break 'error Err(Errno::EINVAL);
        }

        // e
        noise_msg_ephemeral(&mut hs.hs_ck, &mut hs.hs_hash, ue);

        // ee
        if noise_mix_dh(&mut hs.hs_ck, None, &hs.hs_e, ue).is_err() {
            break 'error Err(Errno::EINVAL);
        }

        // se
        if noise_mix_dh(&mut hs.hs_ck, None, &l.l_private.get(), ue).is_err() {
            break 'error Err(Errno::EINVAL);
        }

        // psk
        noise_mix_psk(&mut hs.hs_ck, &mut hs.hs_hash, &mut key, &preshared_key);

        // {}
        if noise_msg_decrypt(&mut [], en, &key, &mut hs.hs_hash).is_err() {
            break 'error Err(Errno::EINVAL);
        }

        hs.hs_remote_index = s_idx;

        let mut ret = Err(Errno::EINVAL);
        rw_enter_write(&r.r_handshake_lock);
        let cur = r.r_handshake.get();
        if cur.hs_state == hs.hs_state && cur.hs_local_index == hs.hs_local_index {
            hs.hs_state = NoiseStateHs::ConsumedResponse;
            r.r_handshake.set(hs);
            ret = Ok(());
        }
        rw_exit_write(&r.r_handshake_lock);
        ret
    };
    rw_exit_read(&l.l_identity_lock);
    wipe(&mut hs);
    explicit_bzero(&mut key);
    explicit_bzero(&mut preshared_key);
    ret
}

/// `noise_remote_begin_session`: derives the transport key pair from a finished handshake and
/// installs it: as current for the initiator, as next (until confirmed) for the responder.
pub fn noise_remote_begin_session(r: &'static NoiseRemote) -> Result<(), Errno> {
    let mut kp = KeypairInit::default();

    rw_enter_write(&r.r_handshake_lock);
    let mut hs = r.r_handshake.get();

    // We now derive the keypair from the handshake
    if hs.hs_state == NoiseStateHs::ConsumedResponse {
        kp.kp_is_initiator = true;
        noise_kdf(
            Some(&mut kp.kp_send),
            Some(&mut kp.kp_recv),
            None,
            &[],
            &hs.hs_ck,
        );
    } else if hs.hs_state == NoiseStateHs::CreatedResponse {
        kp.kp_is_initiator = false;
        noise_kdf(
            Some(&mut kp.kp_recv),
            Some(&mut kp.kp_send),
            None,
            &[],
            &hs.hs_ck,
        );
    } else {
        wipe(&mut hs);
        rw_exit_write(&r.r_handshake_lock);
        return Err(Errno::EINVAL);
    }

    kp.kp_valid = true;
    kp.kp_local_index = hs.hs_local_index;
    kp.kp_remote_index = hs.hs_remote_index;
    kp.kp_birthdate = getnanouptime();
    // bzero(&kp.kp_ctr), mtx_init_flags(&kp.kp_ctr.c_mtx, ...): NoiseKeypair::assign.

    // Now we need to add_new_keypair
    mtx_enter(&r.r_keypair_mtx);
    let next = r.r_next.get();
    let current = r.r_current.get();
    let previous = r.r_previous.get();

    if kp.kp_is_initiator {
        if next.is_some() {
            r.r_next.set(None);
            r.r_previous.set(next);
            noise_remote_keypair_free(r, current);
        } else {
            r.r_previous.set(current);
        }

        noise_remote_keypair_free(r, previous);

        let slot = noise_remote_keypair_allocate(r);
        slot.assign(&kp);
        r.r_current.set(Some(slot));
    } else {
        noise_remote_keypair_free(r, next);
        r.r_previous.set(None);
        noise_remote_keypair_free(r, previous);

        let slot = noise_remote_keypair_allocate(r);
        slot.assign(&kp);
        r.r_next.set(Some(slot));
    }
    mtx_leave(&r.r_keypair_mtx);

    r.r_handshake.set(NoiseHandshake::default());
    wipe(&mut hs);
    rw_exit_write(&r.r_handshake_lock);

    wipe(&mut kp);
    Ok(())
}

/// `noise_remote_clear`: forgets the handshake and every key pair.
pub fn noise_remote_clear(r: &NoiseRemote) {
    rw_enter_write(&r.r_handshake_lock);
    noise_remote_handshake_index_drop(r);
    r.r_handshake.set(NoiseHandshake::default());
    rw_exit_write(&r.r_handshake_lock);

    mtx_enter(&r.r_keypair_mtx);
    noise_remote_keypair_free(r, r.r_next.get());
    noise_remote_keypair_free(r, r.r_current.get());
    noise_remote_keypair_free(r, r.r_previous.get());
    r.r_next.set(None);
    r.r_current.set(None);
    r.r_previous.set(None);
    mtx_leave(&r.r_keypair_mtx);
}

/// `noise_remote_expire_current`: invalidates the next and current key pairs (the identity
/// changed).
pub fn noise_remote_expire_current(r: &NoiseRemote) {
    mtx_enter(&r.r_keypair_mtx);
    if let Some(next) = r.r_next.get() {
        next.kp_valid.set(false);
    }
    if let Some(current) = r.r_current.get() {
        current.kp_valid.set(false);
    }
    mtx_leave(&r.r_keypair_mtx);
}

/// `noise_remote_ready`: may we send with the current key pair? `EINVAL` when not.
pub fn noise_remote_ready(r: &NoiseRemote) -> Result<(), Errno> {
    mtx_enter(&r.r_keypair_mtx);
    // kp_ctr isn't locked here, we're happy to accept a racy read.
    let ret = match r.r_current.get() {
        Some(kp)
            if kp.kp_valid.get()
                && !noise_timer_expired(&kp.kp_birthdate.get(), REJECT_AFTER_TIME, 0)
                && kp.kp_ctr.c_recv.load(Ordering::Relaxed) < REJECT_AFTER_MESSAGES
                && kp.kp_ctr.c_send.load(Ordering::Relaxed) < REJECT_AFTER_MESSAGES =>
        {
            Ok(())
        }
        _ => Err(Errno::EINVAL),
    };
    mtx_leave(&r.r_keypair_mtx);
    ret
}

/// `noise_remote_encrypt`: encrypts `buf[..buflen]` in place with the current key pair and
/// appends the tag (`buf` holds `buflen + NOISE_AUTHTAG_LEN` bytes); the peer's index and the
/// nonce go back through `r_idx` and `nonce`. `Err(ESTALE)`: encrypted, but a new handshake
/// is due.
pub fn noise_remote_encrypt(
    r: &NoiseRemote,
    r_idx: &mut u32,
    nonce: &mut u64,
    buf: &mut [u8],
    buflen: usize,
) -> Result<(), Errno> {
    mtx_enter(&r.r_keypair_mtx);
    let ret = 'error: {
        let Some(kp) = r.r_current.get() else {
            break 'error Err(Errno::EINVAL);
        };

        // We confirm that our values are within our tolerances. We want:
        //  - a valid keypair
        //  - our keypair to be less than REJECT_AFTER_TIME seconds old
        //  - our receive counter to be less than REJECT_AFTER_MESSAGES
        //  - our send counter to be less than REJECT_AFTER_MESSAGES
        //
        // kp_ctr isn't locked here, we're happy to accept a racy read.
        if !kp.kp_valid.get()
            || noise_timer_expired(&kp.kp_birthdate.get(), REJECT_AFTER_TIME, 0)
            || kp.kp_ctr.c_recv.load(Ordering::Relaxed) >= REJECT_AFTER_MESSAGES
            || {
                *nonce = noise_counter_send(&kp.kp_ctr);
                *nonce > REJECT_AFTER_MESSAGES
            }
        {
            break 'error Err(Errno::EINVAL);
        }

        // We encrypt into the same buffer, so the caller must ensure that buf has
        // NOISE_AUTHTAG_LEN bytes to store the MAC. The nonce and index are passed back out
        // to the caller through the provided data pointer.
        *r_idx = kp.kp_remote_index.get();
        let mut send = kp.kp_send.get();
        chacha20poly1305_encrypt_inplace(buf, buflen, &[], *nonce, &send);
        explicit_bzero(&mut send);

        // If our values are still within tolerances, but we are approaching the tolerances,
        // we notify the caller with ESTALE that they should establish a new keypair. The
        // current keypair can continue to be used until the tolerances are hit. We notify if:
        //  - our send counter is valid and not less than REKEY_AFTER_MESSAGES
        //  - we're the initiator and our keypair is older than REKEY_AFTER_TIME seconds
        if (kp.kp_valid.get() && *nonce >= REKEY_AFTER_MESSAGES)
            || (kp.kp_is_initiator.get()
                && noise_timer_expired(&kp.kp_birthdate.get(), REKEY_AFTER_TIME, 0))
        {
            break 'error Err(Errno::ESTALE);
        }

        Ok(())
    };
    mtx_leave(&r.r_keypair_mtx);
    ret
}

/// `noise_remote_decrypt`: decrypts `buf` (ciphertext and tag) in place with the key pair
/// whose local index is `r_idx`, then checks `nonce` against the replay window.
/// `Err(ECONNRESET)`: decrypted, and it confirmed the next key pair (now current);
/// `Err(ESTALE)`: decrypted, but a new handshake is due.
pub fn noise_remote_decrypt(
    r: &NoiseRemote,
    r_idx: u32,
    nonce: u64,
    buf: &mut [u8],
) -> Result<(), Errno> {
    // We retrieve the keypair corresponding to the provided index. We attempt the current
    // keypair first as that is most likely. We also want to make sure that the keypair is
    // valid as it would be catastrophic to decrypt against a zero'ed keypair.
    mtx_enter(&r.r_keypair_mtx);

    let ret = 'error: {
        let with_index =
            |kp: Option<&'static NoiseKeypair>| kp.filter(|kp| kp.kp_local_index.get() == r_idx);
        let Some(kp) = with_index(r.r_current.get())
            .or_else(|| with_index(r.r_previous.get()))
            .or_else(|| with_index(r.r_next.get()))
        else {
            break 'error Err(Errno::EINVAL);
        };

        // We confirm that our values are within our tolerances. These values are the same as
        // the encrypt routine.
        //
        // kp_ctr isn't locked here, we're happy to accept a racy read.
        if noise_timer_expired(&kp.kp_birthdate.get(), REJECT_AFTER_TIME, 0)
            || kp.kp_ctr.c_recv.load(Ordering::Relaxed) >= REJECT_AFTER_MESSAGES
        {
            break 'error Err(Errno::EINVAL);
        }

        // Decrypt, then validate the counter. We don't want to validate the counter before
        // decrypting as we do not know the message is authentic prior to decryption.
        let mut recv = kp.kp_recv.get();
        let authentic = chacha20poly1305_decrypt_inplace(buf, &[], nonce, &recv);
        explicit_bzero(&mut recv);
        if !authentic {
            break 'error Err(Errno::EINVAL);
        }

        if noise_counter_recv(&kp.kp_ctr, nonce).is_err() {
            break 'error Err(Errno::EINVAL);
        }

        // If we've received the handshake confirming data packet then move the next keypair
        // into current. If we do slide the next keypair in, then we skip the
        // REKEY_AFTER_TIME_RECV check. This is safe to do as a data packet can't confirm a
        // session that we are an INITIATOR of.
        if r.r_next.get().is_some_and(|n| ptr::eq(n, kp)) && kp.kp_local_index.get() == r_idx {
            noise_remote_keypair_free(r, r.r_previous.get());
            r.r_previous.set(r.r_current.get());
            r.r_current.set(r.r_next.get());
            r.r_next.set(None);

            break 'error Err(Errno::ECONNRESET);
        }

        // Similar to when we encrypt, we want to notify the caller when we are approaching
        // our tolerances. We notify if:
        //  - we're the initiator and the current keypair is older than REKEY_AFTER_TIME_RECV
        //    seconds.
        if let Some(kp) = r.r_current.get()
            && kp.kp_valid.get()
            && kp.kp_is_initiator.get()
            && noise_timer_expired(&kp.kp_birthdate.get(), REKEY_AFTER_TIME_RECV, 0)
        {
            break 'error Err(Errno::ESTALE);
        }

        Ok(())
    };

    mtx_leave(&r.r_keypair_mtx);
    ret
}

// Private functions - these should not be called outside this file under any circumstances.

/// `noise_remote_keypair_allocate`: an unused key pair of the remote.
fn noise_remote_keypair_allocate(r: &'static NoiseRemote) -> &'static NoiseKeypair {
    let Some(kp) = r.r_unused_keypairs.first() else {
        panic(format_args!(
            "noise_remote_keypair_allocate: no free keypair"
        ));
    };
    // SAFETY: the list is not empty (its first element was just read), under r_keypair_mtx.
    unsafe { r.r_unused_keypairs.remove_head() };
    kp
}

/// `noise_remote_keypair_free`: gives a key pair back to the unused list, drops its index and
/// clears its keys.
fn noise_remote_keypair_free(r: &NoiseRemote, kp: Option<&NoiseKeypair>) {
    let u = r.local().upcall();
    if let Some(kp) = kp {
        // SAFETY: a key pair in use (next, current or previous) is on no list.
        unsafe { r.r_unused_keypairs.insert_head(kp) };
        (u.u_index_drop)(u.u_arg, kp.kp_local_index.get());
        kp.kp_send.set([0; NOISE_SYMMETRIC_KEY_LEN]);
        kp.kp_recv.set([0; NOISE_SYMMETRIC_KEY_LEN]);
    }
}

/// `noise_remote_handshake_index_get`: a new session index from the owner.
fn noise_remote_handshake_index_get(r: &'static NoiseRemote) -> u32 {
    let u = r.local().upcall();
    (u.u_index_set)(u.u_arg, r)
}

/// `noise_remote_handshake_index_drop`: frees the index of the handshake in progress, if any.
fn noise_remote_handshake_index_drop(r: &NoiseRemote) {
    let hs = r.r_handshake.get();
    let u = r.local().upcall();
    rw_assert_wrlock(&r.r_handshake_lock);
    if hs.hs_state != NoiseStateHs::HsZeroed {
        (u.u_index_drop)(u.u_arg, hs.hs_local_index);
    }
}

/// `noise_counter_send`: the next nonce to send (`atomic_inc_long_nv(&c_send) - 1`).
fn noise_counter_send(ctr: &NoiseCounter) -> u64 {
    ctr.c_send.fetch_add(1, Ordering::Relaxed)
}

/// `noise_counter_recv`: records `recv` in the replay window; `EEXIST` for a replayed,
/// too old or exhausted nonce.
fn noise_counter_recv(ctr: &NoiseCounter, recv: u64) -> Result<(), Errno> {
    let mut ret = Err(Errno::EEXIST);

    mtx_enter(&ctr.c_mtx);

    'error: {
        let c_recv = ctr.c_recv.load(Ordering::Relaxed);

        // Check that the recv counter is valid
        if c_recv >= REJECT_AFTER_MESSAGES || recv >= REJECT_AFTER_MESSAGES {
            break 'error;
        }

        // If the packet is out of the window, invalid
        if recv + COUNTER_WINDOW_SIZE < c_recv {
            break 'error;
        }

        // If the new counter is ahead of the current counter, we'll need to zero out the
        // bitmap that has previously been used
        let mut index_recv = recv / COUNTER_BITS;
        let index_ctr = c_recv / COUNTER_BITS;

        if recv > c_recv {
            let top = (index_recv - index_ctr).min(COUNTER_NUM as u64);
            for i in 1..=top {
                ctr.c_backtrack[((i + index_ctr) & (COUNTER_NUM as u64 - 1)) as usize].set(0);
            }
            ctr.c_recv.store(recv, Ordering::Relaxed);
        }

        index_recv %= COUNTER_NUM as u64;
        let bit: c_ulong = 1 << (recv % COUNTER_BITS);
        let word = &ctr.c_backtrack[index_recv as usize];

        if word.get() & bit != 0 {
            break 'error;
        }

        word.set(word.get() | bit);

        ret = Ok(());
    }
    mtx_leave(&ctr.c_mtx);
    ret
}

/// `noise_kdf`: HKDF over HMAC-BLAKE2s: extracts a secret from `x` under the chaining key `ck`,
/// then expands it into up to three outputs `a`, `b`, `c` (each at most `BLAKE2S_HASH_SIZE`
/// bytes, the slices' lengths; a later one only with the earlier ones).
fn noise_kdf(
    a: Option<&mut [u8]>,
    b: Option<&mut [u8]>,
    c: Option<&mut [u8]>,
    x: &[u8],
    ck: &[u8; NOISE_HASH_LEN],
) {
    let mut out = [0u8; BLAKE2S_HASH_SIZE + 1];
    let mut sec = [0u8; BLAKE2S_HASH_SIZE];
    let a_len = a.as_ref().map_or(0, |a| a.len());
    let b_len = b.as_ref().map_or(0, |b| b.len());
    let c_len = c.as_ref().map_or(0, |c| c.len());

    kassert!(
        a_len <= BLAKE2S_HASH_SIZE && b_len <= BLAKE2S_HASH_SIZE && c_len <= BLAKE2S_HASH_SIZE
    );
    kassert!(!(b.is_some() || c.is_some()) || a_len != 0);
    kassert!(c.is_none() || b_len != 0);

    'out: {
        // Extract entropy from "x" into sec
        blake2s_hmac(&mut sec, x, ck, BLAKE2S_HASH_SIZE);

        let Some(a) = a.filter(|a| !a.is_empty()) else {
            break 'out;
        };

        // Expand first key: key = sec, data = 0x1
        out[0] = 1;
        blake2s_hmac_inplace(&mut out, 1, &sec, BLAKE2S_HASH_SIZE);
        a.copy_from_slice(&out[..a_len]);

        let Some(b) = b.filter(|b| !b.is_empty()) else {
            break 'out;
        };

        // Expand second key: key = sec, data = "a" || 0x2
        out[BLAKE2S_HASH_SIZE] = 2;
        blake2s_hmac_inplace(&mut out, BLAKE2S_HASH_SIZE + 1, &sec, BLAKE2S_HASH_SIZE);
        b.copy_from_slice(&out[..b_len]);

        let Some(c) = c.filter(|c| !c.is_empty()) else {
            break 'out;
        };

        // Expand third key: key = sec, data = "b" || 0x3
        out[BLAKE2S_HASH_SIZE] = 3;
        blake2s_hmac_inplace(&mut out, BLAKE2S_HASH_SIZE + 1, &sec, BLAKE2S_HASH_SIZE);
        c.copy_from_slice(&out[..c_len]);
    }

    // Clear sensitive data from stack
    explicit_bzero(&mut sec);
    explicit_bzero(&mut out);
}

/// `noise_mix_dh`: mixes the DH of `private` and `public` into the chaining key, and derives
/// `key` when wanted; `EINVAL` for a DH of zero (a low-order point).
fn noise_mix_dh(
    ck: &mut [u8; NOISE_HASH_LEN],
    key: Option<&mut [u8; NOISE_SYMMETRIC_KEY_LEN]>,
    private: &[u8; NOISE_PUBLIC_KEY_LEN],
    public: &[u8; NOISE_PUBLIC_KEY_LEN],
) -> Result<(), Errno> {
    let mut dh = [0u8; NOISE_PUBLIC_KEY_LEN];

    if !curve25519(&mut dh, private, public) {
        return Err(Errno::EINVAL);
    }
    let mut ck_in = *ck;
    noise_kdf(Some(ck), key.map(|k| &mut k[..]), None, &dh, &ck_in);
    explicit_bzero(&mut ck_in);
    explicit_bzero(&mut dh);
    Ok(())
}

/// `noise_mix_ss`: mixes the pre-computed static-static DH into the chaining key and derives
/// `key`; `ENOENT` when there is none (no identity, or a low-order peer key).
fn noise_mix_ss(
    ck: &mut [u8; NOISE_HASH_LEN],
    key: &mut [u8; NOISE_SYMMETRIC_KEY_LEN],
    ss: &[u8; NOISE_PUBLIC_KEY_LEN],
) -> Result<(), Errno> {
    static NULL_POINT: [u8; NOISE_PUBLIC_KEY_LEN] = [0; NOISE_PUBLIC_KEY_LEN];
    if !timingsafe_bcmp(ss, &NULL_POINT) {
        return Err(Errno::ENOENT);
    }
    let mut ck_in = *ck;
    noise_kdf(Some(ck), Some(key), None, ss, &ck_in);
    explicit_bzero(&mut ck_in);
    Ok(())
}

/// `noise_mix_hash`: `hash = HASH(hash || src)`.
fn noise_mix_hash(hash: &mut [u8; NOISE_HASH_LEN], src: &[u8]) {
    let mut blake = Blake2sState::default();

    blake2s_init(&mut blake, NOISE_HASH_LEN);
    blake2s_update(&mut blake, hash);
    blake2s_update(&mut blake, src);
    blake2s_final(&mut blake, hash);
}

/// `noise_mix_psk`: mixes the pre-shared key into the chaining key, the hash and `key`.
fn noise_mix_psk(
    ck: &mut [u8; NOISE_HASH_LEN],
    hash: &mut [u8; NOISE_HASH_LEN],
    key: &mut [u8; NOISE_SYMMETRIC_KEY_LEN],
    psk: &[u8; NOISE_SYMMETRIC_KEY_LEN],
) {
    let mut tmp = [0u8; NOISE_HASH_LEN];

    let mut ck_in = *ck;
    noise_kdf(Some(ck), Some(&mut tmp), Some(key), psk, &ck_in);
    explicit_bzero(&mut ck_in);
    noise_mix_hash(hash, &tmp);
    explicit_bzero(&mut tmp);
}

/// `noise_param_init`: the initial chaining key and hash for the responder's static key `s`.
fn noise_param_init(
    ck: &mut [u8; NOISE_HASH_LEN],
    hash: &mut [u8; NOISE_HASH_LEN],
    s: &[u8; NOISE_PUBLIC_KEY_LEN],
) {
    let mut blake = Blake2sState::default();

    blake2s(ck, NOISE_HANDSHAKE_NAME, &[], NOISE_HASH_LEN);
    blake2s_init(&mut blake, NOISE_HASH_LEN);
    blake2s_update(&mut blake, ck);
    blake2s_update(&mut blake, NOISE_IDENTIFIER_NAME);
    blake2s_final(&mut blake, hash);

    noise_mix_hash(hash, s);
}

/// `noise_msg_encrypt`: encrypts `dst[..src_len]` in place, the hash as associated data and a
/// zero nonce, appends the tag and mixes the ciphertext into the hash.
fn noise_msg_encrypt(
    dst: &mut [u8],
    src_len: usize,
    key: &[u8; NOISE_SYMMETRIC_KEY_LEN],
    hash: &mut [u8; NOISE_HASH_LEN],
) {
    // Nonce always zero for Noise_IK
    chacha20poly1305_encrypt_inplace(dst, src_len, hash, 0, key);
    noise_mix_hash(hash, &dst[..src_len + NOISE_AUTHTAG_LEN]);
}

/// `noise_msg_decrypt`: authenticates and decrypts `src` (ciphertext and tag) into `dst`, then
/// mixes the ciphertext into the hash; `EINVAL` when it is not authentic.
fn noise_msg_decrypt(
    dst: &mut [u8],
    src: &[u8],
    key: &[u8; NOISE_SYMMETRIC_KEY_LEN],
    hash: &mut [u8; NOISE_HASH_LEN],
) -> Result<(), Errno> {
    // Nonce always zero for Noise_IK
    if !chacha20poly1305_decrypt(dst, src, hash, 0, key) {
        return Err(Errno::EINVAL);
    }
    noise_mix_hash(hash, src);
    Ok(())
}

/// `noise_msg_ephemeral`: mixes an ephemeral public key into the hash and the chaining key.
fn noise_msg_ephemeral(
    ck: &mut [u8; NOISE_HASH_LEN],
    hash: &mut [u8; NOISE_HASH_LEN],
    src: &[u8; NOISE_PUBLIC_KEY_LEN],
) {
    noise_mix_hash(hash, src);
    let mut ck_in = *ck;
    noise_kdf(Some(ck), None, None, src, &ck_in);
    explicit_bzero(&mut ck_in);
}

/// `noise_tai64n_now`: the current time as a TAI64N label, its nanoseconds rounded down.
fn noise_tai64n_now(output: &mut [u8; NOISE_TIMESTAMP_LEN]) {
    let mut time = getnanotime();

    // Round down the nsec counter to limit precise timing leak.
    time.tv_nsec &= REJECT_INTERVAL_MASK as i64;

    // https://cr.yp.to/libtai/tai64.html
    let sec = 0x400000000000000a_u64.wrapping_add(time.tv_sec as u64);
    let nsec = time.tv_nsec as u32;

    output[..size_of::<u64>()].copy_from_slice(&sec.to_be_bytes());
    output[size_of::<u64>()..].copy_from_slice(&nsec.to_be_bytes());
}

/// `noise_timer_expired`: has more than `sec` seconds and `nsec` nanoseconds of uptime passed
/// since `birthdate`? (The C's `ETIMEDOUT`.)
fn noise_timer_expired(birthdate: &Timespec, sec: Time, nsec: i64) -> bool {
    let expire = Timespec::new(sec, nsec);

    // We don't really worry about a zeroed birthdate, to avoid the extra check on every
    // encrypt/decrypt. This does mean that r_last_init check may fail if getnanouptime is
    // < REJECT_INTERVAL from 0.

    let uptime = getnanouptime();
    let expire = timespecadd(birthdate, &expire);
    uptime > expire
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for the Noise handshake: `WGTEST`'s `noise_counter_test`, `noise_handshake_test`
    // and `noise_speed_test` (two in-memory peers, Alice the initiator and Bob the responder,
    // whose upcalls hand back each other's remote), plus vectors for the protocol constants and
    // the KDF computed with an independent BLAKE2s (Python's `hashlib` and `hmac`).

    use std::boxed::Box;
    use std::sync::MutexGuard;
    use std::vec::Vec;

    use super::*;
    use crate::dev::rnd::arc4random_buf;
    use crate::kern::kern_tc::{nanouptime, tc_ticktock};

    /// `MESSAGE_LEN`.
    const MESSAGE_LEN: usize = 64;
    /// `LARGE_MESSAGE_LEN`.
    const LARGE_MESSAGE_LEN: usize = 1420;
    /// `T_LIM`.
    const T_LIM: u64 = COUNTER_WINDOW_SIZE + 1;

    /// The bytes of a hexadecimal string.
    pub(crate) fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
            .collect()
    }

    /// The timecounter tests' lock: the tests that wind the timehands hold it.
    pub(crate) fn tc_lock() -> MutexGuard<'static, ()> {
        crate::kern::kern_tc::tests::LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// Winds the timehands until `getnanouptime` is at least `ns` nanoseconds later than now. The
    /// dummy timecounter advances one microsecond per read; the caller holds [`tc_lock`].
    pub(crate) fn advance_uptime(ns: i64) {
        let start = getnanouptime();
        let until = timespecadd(
            &start,
            &Timespec::new(ns / 1_000_000_000, ns % 1_000_000_000),
        );
        for _ in 0..100_000 {
            if getnanouptime() >= until {
                return;
            }
            for _ in 0..1000 {
                let _ = nanouptime();
            }
            tc_ticktock();
        }
        panic!("the uptime does not advance: {:?}", getnanouptime());
    }

    /// The flood check of `noise_consume_initiation` refuses an initiation in the first 20 ms
    /// after boot (the C's own comment in `noise_timer_expired`): winds the uptime past it, under
    /// [`tc_lock`], which the caller keeps.
    pub(crate) fn uptime_past_reject_interval() -> MutexGuard<'static, ()> {
        let g = tc_lock();
        advance_uptime(2 * REJECT_INTERVAL);
        g
    }

    /// The lock order of the tests that take both: the timecounter's, then the memory's. The
    /// memory lock also keeps out the tests that make a thread `curproc`, which the rwlocks
    /// record as their owner (on the host `curproc` is one global).
    fn setup() -> (MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
        let t = uptime_past_reject_interval();
        (t, crate::kern::subr_pool::tests::setup_real_memory())
    }

    /// `T_INIT`: a zeroed counter.
    fn t_init() -> NoiseCounter {
        let ctr = NoiseCounter::new();
        ctr.reset("counter");
        ctr
    }

    /// `T(num, v, e)`.
    fn t(ctr: &NoiseCounter, num: u32, v: u64, e: Result<(), Errno>) {
        assert_eq!(
            noise_counter_recv(ctr, v),
            e,
            "noise_counter_test, test {num}: failed"
        );
    }

    const EEXIST: Result<(), Errno> = Err(Errno::EEXIST);

    #[test]
    fn noise_counter_test() {
        let ctr = t_init();
        // T(test number, nonce, expected_response)
        t(&ctr, 1, 0, Ok(()));
        t(&ctr, 2, 1, Ok(()));
        t(&ctr, 3, 1, EEXIST);
        t(&ctr, 4, 9, Ok(()));
        t(&ctr, 5, 8, Ok(()));
        t(&ctr, 6, 7, Ok(()));
        t(&ctr, 7, 7, EEXIST);
        t(&ctr, 8, T_LIM, Ok(()));
        t(&ctr, 9, T_LIM - 1, Ok(()));
        t(&ctr, 10, T_LIM - 1, EEXIST);
        t(&ctr, 11, T_LIM - 2, Ok(()));
        t(&ctr, 12, 2, Ok(()));
        t(&ctr, 13, 2, EEXIST);
        t(&ctr, 14, T_LIM + 16, Ok(()));
        t(&ctr, 15, 3, EEXIST);
        t(&ctr, 16, T_LIM + 16, EEXIST);
        t(&ctr, 17, T_LIM * 4, Ok(()));
        t(&ctr, 18, T_LIM * 4 - (T_LIM - 1), Ok(()));
        t(&ctr, 19, 10, EEXIST);
        t(&ctr, 20, T_LIM * 4 - T_LIM, EEXIST);
        t(&ctr, 21, T_LIM * 4 - (T_LIM + 1), EEXIST);
        t(&ctr, 22, T_LIM * 4 - (T_LIM - 2), Ok(()));
        t(&ctr, 23, T_LIM * 4 + 1 - T_LIM, EEXIST);
        t(&ctr, 24, 0, EEXIST);
        t(&ctr, 25, REJECT_AFTER_MESSAGES, EEXIST);
        t(&ctr, 26, REJECT_AFTER_MESSAGES - 1, Ok(()));
        t(&ctr, 27, REJECT_AFTER_MESSAGES, EEXIST);
        t(&ctr, 28, REJECT_AFTER_MESSAGES - 1, EEXIST);
        t(&ctr, 29, REJECT_AFTER_MESSAGES - 2, Ok(()));
        t(&ctr, 30, REJECT_AFTER_MESSAGES + 1, EEXIST);
        t(&ctr, 31, REJECT_AFTER_MESSAGES + 2, EEXIST);
        t(&ctr, 32, REJECT_AFTER_MESSAGES - 2, EEXIST);
        t(&ctr, 33, REJECT_AFTER_MESSAGES - 3, Ok(()));
        t(&ctr, 34, 0, EEXIST);

        let ctr = t_init();
        for i in 1..=COUNTER_WINDOW_SIZE {
            t(&ctr, 35, i, Ok(()));
        }
        t(&ctr, 36, 0, Ok(()));
        t(&ctr, 37, 0, EEXIST);

        let ctr = t_init();
        for i in 2..=COUNTER_WINDOW_SIZE + 1 {
            t(&ctr, 38, i, Ok(()));
        }
        t(&ctr, 39, 1, Ok(()));
        t(&ctr, 40, 0, EEXIST);

        let ctr = t_init();
        for i in (0..COUNTER_WINDOW_SIZE + 1).rev() {
            t(&ctr, 41, i, Ok(()));
        }

        let ctr = t_init();
        for i in (1..COUNTER_WINDOW_SIZE + 2).rev() {
            t(&ctr, 42, i, Ok(()));
        }
        t(&ctr, 43, 0, EEXIST);

        let ctr = t_init();
        for i in (1..COUNTER_WINDOW_SIZE + 1).rev() {
            t(&ctr, 44, i, Ok(()));
        }
        t(&ctr, 45, COUNTER_WINDOW_SIZE + 1, Ok(()));
        t(&ctr, 46, 0, EEXIST);

        let ctr = t_init();
        for i in (1..COUNTER_WINDOW_SIZE + 1).rev() {
            t(&ctr, 47, i, Ok(()));
        }
        t(&ctr, 48, 0, Ok(()));
        t(&ctr, 49, COUNTER_WINDOW_SIZE + 1, Ok(()));
    }

    #[test]
    fn counter_constants_are_the_headers() {
        assert_eq!(COUNTER_BITS, 64);
        assert_eq!(COUNTER_NUM, 128);
        assert_eq!(COUNTER_WINDOW_SIZE, 8128);
        assert_eq!(REJECT_AFTER_MESSAGES, u64::MAX - 8128 - 1);
        assert_eq!(NOISE_TIMESTAMP_LEN, 12);
        assert_eq!(REJECT_INTERVAL_MASK, 0xffff_ffff_ff00_0000);
    }

    /// `upcall_get`: the remote is the argument.
    fn upcall_get(
        x0: *mut c_void,
        _x1: &[u8; NOISE_PUBLIC_KEY_LEN],
    ) -> Option<&'static NoiseRemote> {
        // SAFETY: the tests make the argument a leaked `NoiseRemote`.
        unsafe { x0.cast::<NoiseRemote>().as_ref() }
    }

    /// `upcall_set`: every index is 5.
    fn upcall_set(_x0: *mut c_void, _x1: &'static NoiseRemote) -> u32 {
        5
    }

    /// `upcall_drop`.
    fn upcall_drop(_x0: *mut c_void, _x1: u32) {}

    /// Alice's local and remote (Bob), Bob's local and remote (Alice).
    struct Peers {
        al: &'static NoiseLocal,
        ar: &'static NoiseRemote,
        bl: &'static NoiseLocal,
        br: &'static NoiseRemote,
    }

    fn leak<T>(v: T) -> &'static T {
        Box::leak(Box::new(v))
    }

    /// `noise_handshake_init`: two locals with random keys, each with a remote for the other,
    /// sharing a random pre-shared key.
    fn noise_handshake_init() -> Peers {
        let p = Peers {
            al: leak(NoiseLocal::new()),
            ar: leak(NoiseRemote::new()),
            bl: leak(NoiseLocal::new()),
            br: leak(NoiseRemote::new()),
        };
        let mut apriv = [0u8; NOISE_PUBLIC_KEY_LEN];
        let mut bpriv = [0u8; NOISE_PUBLIC_KEY_LEN];
        let mut apub = [0u8; NOISE_PUBLIC_KEY_LEN];
        let mut bpub = [0u8; NOISE_PUBLIC_KEY_LEN];
        let mut psk = [0u8; NOISE_SYMMETRIC_KEY_LEN];

        let mut upcall = NoiseUpcall {
            u_arg: ptr::null_mut(),
            u_remote_get: upcall_get,
            u_index_set: upcall_set,
            u_index_drop: upcall_drop,
        };

        upcall.u_arg = ptr::from_ref(p.ar).cast_mut().cast();
        noise_local_init(p.al, &upcall);
        upcall.u_arg = ptr::from_ref(p.br).cast_mut().cast();
        noise_local_init(p.bl, &upcall);

        arc4random_buf(&mut apriv);
        arc4random_buf(&mut bpriv);

        noise_local_lock_identity(p.al);
        noise_local_set_private(p.al, &apriv).expect("alice's key");
        noise_local_unlock_identity(p.al);

        noise_local_lock_identity(p.bl);
        noise_local_set_private(p.bl, &bpriv).expect("bob's key");
        noise_local_unlock_identity(p.bl);

        noise_local_keys(p.al, Some(&mut apub), None).expect("alice's public key");
        noise_local_keys(p.bl, Some(&mut bpub), None).expect("bob's public key");

        noise_remote_init(p.ar, &bpub, p.al);
        noise_remote_init(p.br, &apub, p.bl);

        arc4random_buf(&mut psk);
        noise_remote_set_psk(p.ar, &psk).expect("psk");
        noise_remote_set_psk(p.br, &psk).expect("psk");
        p
    }

    /// `struct noise_initiation`.
    struct Initiation {
        s_idx: u32,
        ue: [u8; NOISE_PUBLIC_KEY_LEN],
        es: [u8; NOISE_PUBLIC_KEY_LEN + NOISE_AUTHTAG_LEN],
        ets: [u8; NOISE_TIMESTAMP_LEN + NOISE_AUTHTAG_LEN],
    }

    impl Default for Initiation {
        fn default() -> Self {
            Self {
                s_idx: 0,
                ue: [0; NOISE_PUBLIC_KEY_LEN],
                es: [0; NOISE_PUBLIC_KEY_LEN + NOISE_AUTHTAG_LEN],
                ets: [0; NOISE_TIMESTAMP_LEN + NOISE_AUTHTAG_LEN],
            }
        }
    }

    /// `struct noise_response`.
    #[derive(Default)]
    struct Response {
        s_idx: u32,
        r_idx: u32,
        ue: [u8; NOISE_PUBLIC_KEY_LEN],
        en: [u8; NOISE_AUTHTAG_LEN],
    }

    #[test]
    fn noise_handshake_test() {
        let _g = setup();
        let p = noise_handshake_init();
        let mut init = Initiation::default();
        let mut resp = Response::default();
        let mut index = 0u32;
        let mut nonce = 0u64;
        let mut data = [0u8; MESSAGE_LEN + NOISE_AUTHTAG_LEN];

        // Create initiation
        noise_create_initiation(
            p.ar,
            &mut init.s_idx,
            &mut init.ue,
            &mut init.es,
            &mut init.ets,
        )
        .expect("create_initiation");

        // Check encrypted (es) validation
        for i in 0..init.es.len() {
            init.es[i] = !init.es[i];
            assert_eq!(
                noise_consume_initiation(p.bl, init.s_idx, &init.ue, &init.es, &init.ets).err(),
                Some(Errno::EINVAL),
                "consume_initiation_es"
            );
            init.es[i] = !init.es[i];
        }

        // Check encrypted (ets) validation
        for i in 0..init.ets.len() {
            init.ets[i] = !init.ets[i];
            assert_eq!(
                noise_consume_initiation(p.bl, init.s_idx, &init.ue, &init.es, &init.ets).err(),
                Some(Errno::EINVAL),
                "consume_initiation_ets"
            );
            init.ets[i] = !init.ets[i];
        }

        // Consume initiation properly
        let r = noise_consume_initiation(p.bl, init.s_idx, &init.ue, &init.es, &init.ets)
            .expect("consume_initiation");
        assert!(ptr::eq(r, p.br), "remote_lookup");

        // Replay initiation
        assert_eq!(
            noise_consume_initiation(p.bl, init.s_idx, &init.ue, &init.es, &init.ets).err(),
            Some(Errno::EINVAL),
            "consume_initiation_replay"
        );

        // Create response
        noise_create_response(
            p.br,
            &mut resp.s_idx,
            &mut resp.r_idx,
            &mut resp.ue,
            &mut resp.en,
        )
        .expect("create_response");
        assert_eq!((resp.s_idx, resp.r_idx), (5, init.s_idx));

        // Check encrypted (en) validation
        for i in 0..resp.en.len() {
            resp.en[i] = !resp.en[i];
            assert_eq!(
                noise_consume_response(p.ar, resp.s_idx, resp.r_idx, &resp.ue, &resp.en),
                Err(Errno::EINVAL),
                "consume_response_en"
            );
            resp.en[i] = !resp.en[i];
        }

        // Consume response properly
        noise_consume_response(p.ar, resp.s_idx, resp.r_idx, &resp.ue, &resp.en)
            .expect("consume_response");

        // Derive keys on both sides
        noise_remote_begin_session(p.ar).expect("promote_ar");
        noise_remote_begin_session(p.br).expect("promote_br");

        // The transport keys match: Alice sends with what Bob receives with, and back.
        let akp = p.ar.r_current.get().expect("alice's current keypair");
        let bkp = p.br.r_next.get().expect("bob's next keypair");
        assert_eq!(akp.kp_send.get(), bkp.kp_recv.get());
        assert_eq!(akp.kp_recv.get(), bkp.kp_send.get());
        assert_ne!(akp.kp_send.get(), akp.kp_recv.get());
        assert!(akp.kp_is_initiator.get() && !bkp.kp_is_initiator.get());

        for (i, b) in data.iter_mut().take(MESSAGE_LEN).enumerate() {
            *b = i as u8;
        }

        // Since bob is responder, he must not encrypt until confirmed
        assert_eq!(
            noise_remote_encrypt(p.br, &mut index, &mut nonce, &mut data, MESSAGE_LEN),
            Err(Errno::EINVAL),
            "encrypt_kci_wait"
        );

        // Alice now encrypt and gets bob to decrypt
        noise_remote_encrypt(p.ar, &mut index, &mut nonce, &mut data, MESSAGE_LEN)
            .expect("encrypt_akp");
        assert!(
            data[..MESSAGE_LEN]
                .iter()
                .enumerate()
                .any(|(i, b)| *b != i as u8)
        );
        assert_eq!(
            noise_remote_decrypt(p.br, index, nonce, &mut data),
            Err(Errno::ECONNRESET),
            "decrypt_bkp"
        );

        for (i, b) in data.iter().take(MESSAGE_LEN).enumerate() {
            assert_eq!(*b, i as u8, "decrypt_message_akp_bkp");
        }

        // A replay of the same packet is refused by the counter.
        let mut replay = data;
        noise_remote_encrypt(p.ar, &mut index, &mut nonce, &mut replay, MESSAGE_LEN)
            .expect("encrypt_again");
        let copy = replay;
        assert_eq!(
            noise_remote_decrypt(p.br, index, nonce, &mut replay),
            Ok(())
        );
        replay = copy;
        assert_eq!(
            noise_remote_decrypt(p.br, index, nonce, &mut replay),
            Err(Errno::EINVAL),
            "replay"
        );

        // Now bob has received confirmation, he can encrypt
        noise_remote_encrypt(p.br, &mut index, &mut nonce, &mut data, MESSAGE_LEN)
            .expect("encrypt_kci_ready");
        assert_eq!(
            noise_remote_decrypt(p.ar, index, nonce, &mut data),
            Ok(()),
            "decrypt_akp"
        );

        for (i, b) in data.iter().take(MESSAGE_LEN).enumerate() {
            assert_eq!(*b, i as u8, "decrypt_message_bkp_akp");
        }

        // Clearing forgets every key pair.
        noise_remote_clear(p.ar);
        assert_eq!(noise_remote_ready(p.ar), Err(Errno::EINVAL));
        assert_eq!(noise_remote_ready(p.br), Ok(()));
        noise_remote_expire_current(p.br);
        assert_eq!(noise_remote_ready(p.br), Err(Errno::EINVAL));
    }

    #[test]
    fn no_identity_no_handshake() {
        let _g = setup();
        let p = noise_handshake_init();
        let mut init = Initiation::default();
        // A remote of a local without identity has no static-static DH: no initiation.
        let l = leak(NoiseLocal::new());
        let upcall = NoiseUpcall {
            u_arg: ptr::null_mut(),
            u_remote_get: upcall_get,
            u_index_set: upcall_set,
            u_index_drop: upcall_drop,
        };
        noise_local_init(l, &upcall);
        assert_eq!(noise_local_keys(l, None, None), Err(Errno::ENXIO));
        let r = leak(NoiseRemote::new());
        noise_remote_init(r, &p.al.l_public.get(), l);
        assert_eq!(r.r_ss.get(), [0; NOISE_PUBLIC_KEY_LEN]);
        assert_eq!(
            noise_create_initiation(
                r,
                &mut init.s_idx,
                &mut init.ue,
                &mut init.es,
                &mut init.ets
            ),
            Err(Errno::EINVAL)
        );
        // The all-zero private key has no public key.
        noise_local_lock_identity(l);
        assert_eq!(
            noise_local_set_private(l, &[0; NOISE_PUBLIC_KEY_LEN]),
            Err(Errno::ENXIO)
        );
        noise_local_unlock_identity(l);
        // The psk: the same one again is EEXIST, none is ENOENT.
        let mut psk = [0u8; NOISE_SYMMETRIC_KEY_LEN];
        assert_eq!(
            noise_remote_keys(r, None, Some(&mut psk)),
            Err(Errno::ENOENT)
        );
        assert_eq!(noise_remote_set_psk(r, &psk), Err(Errno::EEXIST));
        psk[0] = 1;
        assert_eq!(noise_remote_set_psk(r, &psk), Ok(()));
        assert_eq!(noise_remote_keys(r, None, None), Ok(()));
    }

    #[test]
    #[ignore = "WGTEST noise_speed_test: timings only"]
    fn noise_speed_test() {
        const SPEED_ITER: u32 = 1 << 16;
        let _g = setup();
        let p = noise_handshake_init();
        let mut init = Initiation::default();
        let mut resp = Response::default();
        let mut index = 0u32;
        let mut nonce = 0u64;
        let mut data = [0u8; MESSAGE_LEN + NOISE_AUTHTAG_LEN];
        let mut largedata = [0u8; LARGE_MESSAGE_LEN + NOISE_AUTHTAG_LEN];

        noise_create_initiation(
            p.ar,
            &mut init.s_idx,
            &mut init.ue,
            &mut init.es,
            &mut init.ets,
        )
        .expect("create_initiation");
        noise_consume_initiation(p.bl, init.s_idx, &init.ue, &init.es, &init.ets)
            .expect("consume_initiation");
        noise_create_response(
            p.br,
            &mut resp.s_idx,
            &mut resp.r_idx,
            &mut resp.ue,
            &mut resp.en,
        )
        .expect("create_response");
        noise_consume_response(p.ar, resp.s_idx, resp.r_idx, &resp.ue, &resp.en)
            .expect("consume_response");
        noise_remote_begin_session(p.ar).expect("begin_ar");
        noise_remote_begin_session(p.br).expect("begin_br");

        let start = std::time::Instant::now();
        for _ in 0..SPEED_ITER {
            noise_remote_encrypt(p.ar, &mut index, &mut nonce, &mut data, MESSAGE_LEN)
                .expect("encrypt_akp");
        }
        std::println!(
            "noise_speed_test {SPEED_ITER} {MESSAGE_LEN} byte encryptions: {:?}",
            start.elapsed()
        );
        let start = std::time::Instant::now();
        for _ in 0..SPEED_ITER {
            noise_remote_encrypt(
                p.ar,
                &mut index,
                &mut nonce,
                &mut largedata,
                LARGE_MESSAGE_LEN,
            )
            .expect("encrypt_akp");
        }
        std::println!(
            "noise_speed_test {SPEED_ITER} {LARGE_MESSAGE_LEN} byte encryptions: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn construction_and_identifier_hashes() {
        // The WireGuard paper, 5.4: Ci = HASH(CONSTRUCTION), Hi = HASH(Ci || IDENTIFIER).
        let mut ck = [0u8; NOISE_HASH_LEN];
        let mut hash = [0u8; NOISE_HASH_LEN];
        let mut s = [0u8; NOISE_PUBLIC_KEY_LEN];
        s[0] = 9;
        noise_param_init(&mut ck, &mut hash, &s);
        assert_eq!(
            ck.to_vec(),
            hex("60e26daef327efc02ec335e2a025d2d016eb4206f87277f52d38d1988b78cd36")
        );
        // Hi, then HASH(Hi || s): the responder's static key mixed in.
        let mut hi = [0u8; NOISE_HASH_LEN];
        let mut blake = Blake2sState::default();
        blake2s_init(&mut blake, NOISE_HASH_LEN);
        blake2s_update(&mut blake, &ck);
        blake2s_update(&mut blake, NOISE_IDENTIFIER_NAME);
        blake2s_final(&mut blake, &mut hi);
        assert_eq!(
            hi.to_vec(),
            hex("2211b361081ac566691243db458ad5322d9c6c662293e8b70ee19c65ba079ef3")
        );
        assert_eq!(
            hash.to_vec(),
            hex("575bad75a5a30f85f58df113422a55d41873e357b40a3a2ea91f456c9508211a")
        );
    }

    #[test]
    fn kdf_expands_three_keys() {
        let ck: [u8; NOISE_HASH_LEN] =
            hex("60e26daef327efc02ec335e2a025d2d016eb4206f87277f52d38d1988b78cd36")
                .try_into()
                .expect("32 bytes");
        let x = [0x42u8; 32];
        let (mut a, mut b, mut c) = ([0u8; 32], [0u8; 32], [0u8; 32]);
        noise_kdf(Some(&mut a), Some(&mut b), Some(&mut c), &x, &ck);
        assert_eq!(
            a.to_vec(),
            hex("7c2e52a7caca44d65a8e13a5eedc5a1c053d4923396dde6808ca7381c9fa31d1")
        );
        assert_eq!(
            b.to_vec(),
            hex("32016483cdab35bc3625293aaf8a06aeff1505e5ad3f7bce2858ba7c24fbc9c3")
        );
        assert_eq!(
            c.to_vec(),
            hex("8ad1f3ddf2249e1bf68b8b5d329d6c5751696093facfa5a2f8993152009a41c8")
        );
        // Fewer outputs are prefixes of the same expansion; the chaining key may be the output.
        let mut ck2 = ck;
        let ck_in = ck2;
        noise_kdf(Some(&mut ck2), None, None, &x, &ck_in);
        assert_eq!(ck2, a);
    }

    #[test]
    fn tai64n_labels_grow_and_are_rounded() {
        let mut t = [0u8; NOISE_TIMESTAMP_LEN];
        noise_tai64n_now(&mut t);
        // 2^62 + 10 + seconds: the label starts with 0x40.
        assert_eq!(t[0], 0x40);
        let nsec = u32::from_be_bytes([t[8], t[9], t[10], t[11]]);
        assert_eq!(u64::from(nsec) & !REJECT_INTERVAL_MASK, 0);
    }
}
/* </TESTS> */
