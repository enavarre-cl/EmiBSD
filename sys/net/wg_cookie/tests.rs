//! Host tests for the cookie MACs and the rate limiter: `WGTEST`'s
//! `cookie_ratelimit_timings_test` (the sleeps are timecounter advances), `cookie_ratelimit_
//! capacity_test` and `cookie_mac_test`, plus `mac1`/`mac2` vectors computed with an independent
//! BLAKE2s (Python's `hashlib`).

use std::sync::MutexGuard;

use super::*;
use crate::dev::rnd::arc4random;
use crate::kern::subr_pool::{pool_destroy, pool_init};
use crate::machine::intr::IPL_NONE;
use crate::net::wg_noise::tests::{advance_uptime, hex, tc_lock};

/// `MESSAGE_LEN`.
const MESSAGE_LEN: usize = 64;

/// Real memory for the pools and the tables, then the timecounter lock.
fn setup() -> (MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
    let m = crate::kern::subr_pool::tests::setup_real_memory();
    (m, tc_lock())
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

/// The `struct sockaddr *` of a `sockaddr_in` (both 16 bytes).
fn sa(sin: &SockaddrIn) -> &Sockaddr {
    // SAFETY: `sintosa` of a `sockaddr_in`, which is as long as a `sockaddr` (asserted in the
    // module); `Sockaddr` is byte-aligned.
    unsafe { &*ptr::from_ref(sin).cast::<Sockaddr>() }
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
    // INET6: the sin6 half of the test; not configured.

    for (i, (result, sleep_time)) in RL_EXPECTED.iter().enumerate() {
        if *sleep_time != 0 {
            advance_uptime(*sleep_time);
        }

        // The first v4 ratelimit_allow is against a constant address, and should be
        // indifferent to the port.
        sin.sin_addr.s_addr = 0x01020304;
        sin.sin_port = arc4random() as u16;

        assert_eq!(
            ratelimit_allow(&rl, sa(&sin)),
            *result,
            "malicious v4, iter {i}"
        );

        // The second ratelimit_allow is to test that an arbitrary address is still allowed.
        sin.sin_addr.s_addr += i as u32 + 1;
        sin.sin_port = arc4random() as u16;

        assert_eq!(
            ratelimit_allow(&rl, sa(&sin)),
            Ok(()),
            "non-malicious v4, iter {i}"
        );
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
                ratelimit_allow(&rl, sa(&sin)),
                Err(Errno::ECONNREFUSED),
                "reject, iter {i}"
            );
        } else {
            assert_eq!(ratelimit_allow(&rl, sa(&sin)), Ok(()), "allow, iter {i}");
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
            cookie_checker_validate_macs(&checker, &cm, &message, false, sa(&sin)),
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
        cookie_checker_validate_macs(&checker, &cm, &message, false, sa(&sin)),
        Ok(()),
        "validate_macs_noload_normal"
    );

    // Check we get a EAGAIN if no mac2 and under load
    assert_eq!(
        cookie_checker_validate_macs(&checker, &cm, &message, true, sa(&sin)),
        Err(Errno::EAGAIN),
        "validate_macs_load_normal"
    );

    // Simulate a cookie message
    cookie_checker_create_payload(&checker, &cm, &mut nonce, &mut cookie, sa(&sin));

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
        cookie_checker_validate_macs(&checker, &cm, &message, true, sa(&sin)),
        Ok(()),
        "validate_macs_load_normal_mac2"
    );

    sin.sin_addr.s_addr = !sin.sin_addr.s_addr;
    // Check we get EAGAIN if we munge the source IP
    assert_eq!(
        cookie_checker_validate_macs(&checker, &cm, &message, true, sa(&sin)),
        Err(Errno::EAGAIN),
        "validate_macs_load_spoofip_mac2"
    );
    sin.sin_addr.s_addr = !sin.sin_addr.s_addr;

    // Check we get OK if mac2 and under load
    assert_eq!(
        cookie_checker_validate_macs(&checker, &cm, &message, true, sa(&sin)),
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
        cookie_checker_validate_macs(&checker, &cm, &message, false, sa(&sin)),
        Ok(())
    );
    cookie_checker_update(&checker, None);
    assert_eq!(
        cookie_checker_validate_macs(&checker, &cm, &message, false, sa(&sin)),
        Err(Errno::EINVAL)
    );
}
