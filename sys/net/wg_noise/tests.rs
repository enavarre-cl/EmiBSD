//! Host tests for the Noise handshake: `WGTEST`'s `noise_counter_test`, `noise_handshake_test`
//! and `noise_speed_test` (two in-memory peers, Alice the initiator and Bob the responder,
//! whose upcalls hand back each other's remote), plus vectors for the protocol constants and
//! the KDF computed with an independent BLAKE2s (Python's `hashlib` and `hmac`).

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
fn upcall_get(x0: *mut c_void, _x1: &[u8; NOISE_PUBLIC_KEY_LEN]) -> Option<&'static NoiseRemote> {
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
    let _g = uptime_past_reject_interval();
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
    let _g = uptime_past_reject_interval();
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
