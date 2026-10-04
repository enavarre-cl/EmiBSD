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
