//! Tests of the crypto framework's driver table and dispatch with fake drivers: driver ids and
//! growth of the table, which driver a session lands on (hardware before software, `hard`),
//! the session id, the counters, a driver that goes away (`ERESTART`, `CRYPTOCAP_F_CLEANUP`)
//! and the migration of a session to another driver.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::crypto::cryptodev::{
    CRYPTO_AES_CBC, CRYPTO_ALG_FLAG_SUPPORTED, CRYPTO_SHA1_HMAC, CryptoBuf,
};
use crate::crypto::cryptosoft::{swcr_init, swcr_reset};
use crate::crypto::testutil::serial;

fn fresh() -> std::sync::MutexGuard<'static, ()> {
    let g = serial();
    crypto_reset();
    swcr_reset();
    g
}

static NEWSESSIONS: AtomicU32 = AtomicU32::new(0);
static FREED: AtomicUsize = AtomicUsize::new(0);
static PROCESSED: AtomicUsize = AtomicUsize::new(0);

/// A fake driver's session numbers start at 100 and count up.
fn fake_new(sid: &mut u32, _cri: &Cryptoini<'_>) -> Result<(), Errno> {
    *sid = 100 + NEWSESSIONS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

fn fake_new_fails(_sid: &mut u32, _cri: &Cryptoini<'_>) -> Result<(), Errno> {
    Err(Errno::ENOBUFS)
}

fn fake_free(_sid: u64) -> Result<(), Errno> {
    FREED.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

fn fake_process_ok(_crp: &mut Cryptop<'_>) -> Result<(), Errno> {
    PROCESSED.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

fn fake_process_restart(_crp: &mut Cryptop<'_>) -> Result<(), Errno> {
    Err(Errno::ERESTART)
}

fn algs(list: &[i32]) -> [i32; CRYPTO_ALGORITHM_MAX + 1] {
    let mut a = [0; CRYPTO_ALGORITHM_MAX + 1];
    for alg in list {
        a[*alg as usize] = CRYPTO_ALG_FLAG_SUPPORTED;
    }
    a
}

fn cri(alg: i32) -> Cryptoini<'static> {
    Cryptoini {
        cri_alg: alg,
        cri_klen: 128,
        cri_key: &[0x42; 16],
        ..Cryptoini::default()
    }
}

#[test]
fn nothing_works_before_the_first_driver() {
    let _g = fresh();
    assert_eq!(crypto_drivers_num(), 0);
    assert_eq!(
        crypto_newsession(&cri(CRYPTO_AES_CBC), 0),
        Err(Errno::EINVAL)
    );
    assert_eq!(crypto_freesession(5 << 32), Err(Errno::EINVAL));
    let mut crp = crypto_getreq(1).expect("a request");
    assert_eq!(crypto_invoke(&mut crp), Err(Errno::EINVAL));
    assert_eq!(crypto_unregister(0, 1), Err(Errno::EINVAL));
    assert_eq!(
        crypto_register(0, &algs(&[]), fake_new, fake_free, fake_process_ok),
        Err(Errno::EINVAL)
    );
}

#[test]
fn driver_ids_and_growth_of_the_table() {
    let _g = fresh();
    let mut ids = Vec::new();
    for _ in 0..CRYPTO_DRIVERS_INITIAL {
        ids.push(crypto_get_driverid(0).expect("an id"));
    }
    assert_eq!(ids, [0, 1, 2, 3]);
    assert_eq!(crypto_drivers_num(), CRYPTO_DRIVERS_INITIAL);
    // The fifth makes the table double.
    assert_eq!(crypto_get_driverid(CRYPTOCAP_F_SOFTWARE), Ok(4));
    assert_eq!(crypto_drivers_num(), 2 * CRYPTO_DRIVERS_INITIAL);
    let flags = with_drivers(|d| (d[4].cc_flags, d[4].cc_sessions));
    assert_eq!(flags, (CRYPTOCAP_F_SOFTWARE, 1)); // marked
    // Fill it up to the maximum, then refuse.
    for expect in 5..CRYPTO_DRIVERS_MAX as u32 {
        assert_eq!(crypto_get_driverid(0), Ok(expect));
    }
    assert_eq!(crypto_drivers_num(), CRYPTO_DRIVERS_MAX);
    assert_eq!(crypto_get_driverid(0), Err(Errno::ENOMEM));
}

#[test]
fn a_slot_is_reused_once_unregistered() {
    let _g = fresh();
    let a = crypto_get_driverid(0).expect("an id");
    let b = crypto_get_driverid(0).expect("an id");
    crypto_register(
        a,
        &algs(&[CRYPTO_AES_CBC]),
        fake_new,
        fake_free,
        fake_process_ok,
    )
    .expect("register");
    crypto_register(
        b,
        &algs(&[CRYPTO_AES_CBC]),
        fake_new,
        fake_free,
        fake_process_ok,
    )
    .expect("register");
    crypto_unregister(a, CRYPTO_ALGORITHM_MAX as i32 + 1).expect("unregister");
    assert_eq!(crypto_get_driverid(0), Ok(a));
}

#[test]
fn hardware_before_software_and_the_hard_flag() {
    let _g = fresh();
    swcr_init(); // driver 0, software
    let hw = crypto_get_driverid(0).expect("an id");
    crypto_register(
        hw,
        &algs(&[CRYPTO_AES_CBC]),
        fake_new,
        fake_free,
        fake_process_ok,
    )
    .expect("register");
    NEWSESSIONS.store(0, Ordering::Relaxed);

    // AES-CBC: the hardware driver wins, though the software one supports it too.
    let sid = crypto_newsession(&cri(CRYPTO_AES_CBC), 0).expect("a session");
    assert_eq!(sid >> 32, u64::from(hw));
    assert_eq!(sid & 0xffff_ffff, 100);
    assert_eq!(with_drivers(|d| d[hw as usize].cc_sessions), 1);

    // HMAC-SHA1: only the software driver; `hard` asks for hardware only.
    let sw = crypto_newsession(&cri(CRYPTO_SHA1_HMAC), 0).expect("a session");
    assert_eq!(sw >> 32, 0);
    assert_eq!(
        crypto_newsession(&cri(CRYPTO_SHA1_HMAC), 1),
        Err(Errno::EINVAL)
    );
    // A chain: the driver must support all of its algorithms.
    let hmac = cri(CRYPTO_SHA1_HMAC);
    let chain = Cryptoini {
        cri_next: Some(&hmac),
        ..cri(CRYPTO_AES_CBC)
    };
    assert_eq!(crypto_newsession(&chain, 0).expect("a session") >> 32, 0);
    // An algorithm number out of range is unsupported, not a crash.
    assert_eq!(crypto_newsession(&cri(-1), 0), Err(Errno::EINVAL));
    assert_eq!(crypto_newsession(&cri(1000), 0), Err(Errno::EINVAL));
}

#[test]
fn the_least_loaded_driver_of_two_equal_ones_wins() {
    let _g = fresh();
    let a = crypto_get_driverid(0).expect("an id");
    let b = crypto_get_driverid(0).expect("an id");
    for d in [a, b] {
        crypto_register(
            d,
            &algs(&[CRYPTO_AES_CBC]),
            fake_new,
            fake_free,
            fake_process_ok,
        )
        .expect("register");
    }
    // `<=` prefers the later driver on a tie: b, then (a has fewer) a, then b again.
    let picks: Vec<u64> = (0..4)
        .map(|_| crypto_newsession(&cri(CRYPTO_AES_CBC), 0).expect("a session") >> 32)
        .collect();
    assert_eq!(
        picks,
        [u64::from(b), u64::from(a), u64::from(b), u64::from(a)]
    );
}

#[test]
fn a_failing_driver_creates_no_session() {
    let _g = fresh();
    let d = crypto_get_driverid(0).expect("an id");
    crypto_register(
        d,
        &algs(&[CRYPTO_AES_CBC]),
        fake_new_fails,
        fake_free,
        fake_process_ok,
    )
    .expect("register");
    assert_eq!(
        crypto_newsession(&cri(CRYPTO_AES_CBC), 0),
        Err(Errno::ENOBUFS)
    );
    assert_eq!(with_drivers(|dr| dr[d as usize].cc_sessions), 0);
}

#[test]
fn sessions_are_counted_and_freed() {
    let _g = fresh();
    let d = crypto_get_driverid(0).expect("an id");
    crypto_register(
        d,
        &algs(&[CRYPTO_AES_CBC]),
        fake_new,
        fake_free,
        fake_process_ok,
    )
    .expect("register");
    FREED.store(0, Ordering::Relaxed);
    let s1 = crypto_newsession(&cri(CRYPTO_AES_CBC), 0).expect("a session");
    let s2 = crypto_newsession(&cri(CRYPTO_AES_CBC), 0).expect("a session");
    assert_eq!(with_drivers(|dr| dr[d as usize].cc_sessions), 2);
    crypto_freesession(s1).expect("free");
    crypto_freesession(s2).expect("free");
    assert_eq!(FREED.load(Ordering::Relaxed), 2);
    assert_eq!(with_drivers(|dr| dr[d as usize].cc_sessions), 0);
    // The count does not go below zero.
    crypto_freesession(s2).expect("free");
    assert_eq!(with_drivers(|dr| dr[d as usize].cc_sessions), 0);
    // A driver that does not exist.
    assert_eq!(crypto_freesession(99 << 32), Err(Errno::ENOENT));
}

#[test]
fn invoke_counts_operations_and_bytes() {
    let _g = fresh();
    let d = crypto_get_driverid(0).expect("an id");
    crypto_register(
        d,
        &algs(&[CRYPTO_AES_CBC]),
        fake_new,
        fake_free,
        fake_process_ok,
    )
    .expect("register");
    let sid = crypto_newsession(&cri(CRYPTO_AES_CBC), 0).expect("a session");
    PROCESSED.store(0, Ordering::Relaxed);
    let mut crp = crypto_getreq(2).expect("a request");
    assert_eq!(
        (crp.crp_ndesc, crp.crp_ndescalloc, crp.crp_desc.len()),
        (2, 2, 2)
    );
    crp.crp_sid = sid;
    crp.crp_ilen = 1400;
    crypto_invoke(&mut crp).expect("invoke");
    crypto_invoke(&mut crp).expect("invoke");
    assert_eq!(PROCESSED.load(Ordering::Relaxed), 2);
    let (ops, bytes) = with_drivers(|dr| (dr[d as usize].cc_operations, dr[d as usize].cc_bytes));
    assert_eq!((ops, bytes), (2, 2800));
    // No descriptors: refused.
    crp.crp_ndesc = 0;
    assert_eq!(crypto_invoke(&mut crp), Err(Errno::EINVAL));
}

#[test]
fn getreq_makes_a_request_with_its_descriptors() {
    let crp = crypto_getreq(5).expect("a request");
    assert_eq!(
        (crp.crp_ndesc, crp.crp_ndescalloc, crp.crp_desc.len()),
        (5, 5, 5)
    );
    assert!(matches!(crp.crp_buf, CryptoBuf::None));
    assert_eq!(crp.crp_sid, 0);
    crypto_freereq(Some(crp));
    crypto_freereq(None);
    assert!(crypto_getreq(-1).is_none());
    crypto_init();
}

#[test]
fn unregistering_one_algorithm_at_a_time() {
    let _g = fresh();
    let d = crypto_get_driverid(0).expect("an id");
    crypto_register(
        d,
        &algs(&[CRYPTO_AES_CBC, CRYPTO_SHA1_HMAC]),
        fake_new,
        fake_free,
        fake_process_ok,
    )
    .expect("register");
    assert_eq!(crypto_unregister(d, 0), Err(Errno::EINVAL));
    assert_eq!(crypto_unregister(d + 1, CRYPTO_AES_CBC), Err(Errno::EINVAL));
    assert_eq!(
        crypto_unregister(d, CRYPTO_ALGORITHM_MAX as i32 + 2),
        Err(Errno::EINVAL)
    );
    crypto_unregister(d, CRYPTO_AES_CBC).expect("unregister");
    assert_eq!(crypto_unregister(d, CRYPTO_AES_CBC), Err(Errno::EINVAL));
    assert_eq!(
        crypto_newsession(&cri(CRYPTO_AES_CBC), 0),
        Err(Errno::EINVAL)
    );
    crypto_newsession(&cri(CRYPTO_SHA1_HMAC), 0).expect("still there");
    // The last algorithm going leaves a driver with a session marked for cleanup.
    crypto_unregister(d, CRYPTO_SHA1_HMAC).expect("unregister");
    let (flags, sessions) =
        with_drivers(|dr| (dr[d as usize].cc_flags, dr[d as usize].cc_sessions));
    assert_eq!((flags, sessions), (CRYPTOCAP_F_CLEANUP, 1));
    // Its slot is not reused until the session is freed, and then it is clean.
    assert_ne!(crypto_get_driverid(0), Ok(d));
    crypto_freesession(u64::from(d) << 32).expect("free");
    assert_eq!(with_drivers(|dr| dr[d as usize].cc_flags), 0);
}

#[test]
fn a_driver_that_asks_to_restart_is_unregistered_and_the_session_migrates() {
    let _g = fresh();
    swcr_init(); // driver 0, software
    let hw = crypto_get_driverid(0).expect("an id");
    crypto_register(
        hw,
        &algs(&[CRYPTO_AES_CBC]),
        fake_new,
        fake_free,
        fake_process_restart,
    )
    .expect("register");
    let sid = crypto_newsession(&cri(CRYPTO_AES_CBC), 0).expect("a session");
    assert_eq!(sid >> 32, u64::from(hw));

    // The descriptor carries what is needed to make the session again.
    let key = [0x11u8; 16];
    let mut buf = vec![0u8; 32];
    let mut crp = crypto_getreq(1).expect("a request");
    crp.crp_sid = sid;
    crp.crp_ilen = 32;
    crp.crp_desc[0].crd_len = 32;
    crp.crp_desc[0].crd_flags = CRD_F_ENCRYPT_EXPLICIT;
    crp.crp_desc[0].CRD_INI = Cryptoini {
        cri_alg: CRYPTO_AES_CBC,
        cri_klen: 128,
        cri_key: &key,
        ..Cryptoini::default()
    };
    let mut iov = [crate::sys::uio::Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: 32,
    }];
    let mut uio = crate::sys::uio::Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: 32,
        uio_segflg: crate::sys::uio::UioSeg::UIO_SYSSPACE,
        uio_rw: crate::sys::uio::UioRw::UIO_WRITE,
        uio_procp: None,
    };
    crp.crp_flags = crate::crypto::cryptodev::CRYPTO_F_IOV;
    crp.crp_buf = CryptoBuf::Iov(&mut uio);

    // The first invoke: the driver says ERESTART; it is unregistered; the session is made again
    // on the software driver, and the caller is told to try again.
    assert_eq!(crypto_invoke(&mut crp), Err(Errno::EAGAIN));
    assert_eq!(
        crp.crp_sid >> 32,
        0,
        "the new session is the software driver's"
    );
    assert_ne!(crp.crp_sid, sid);
    // (The old session still counts on the unregistered driver, marked for cleanup.)
    assert_eq!(
        with_drivers(|dr| dr[hw as usize].cc_flags),
        CRYPTOCAP_F_CLEANUP
    );
    // The second goes through the software driver.
    crp.crp_desc[0].crd_iv_mut()[..16].copy_from_slice(&[0; 16]);
    crypto_invoke(&mut crp).expect("invoke");
    assert_ne!(
        buf,
        vec![0u8; 32],
        "the software driver encrypted the buffer"
    );
}

/// `CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT | CRD_F_IV_PRESENT`.
const CRD_F_ENCRYPT_EXPLICIT: i32 = 0x01 | 0x04 | 0x02;

#[test]
fn a_session_on_a_driver_marked_for_cleanup_migrates_too() {
    let _g = fresh();
    swcr_init();
    let hw = crypto_get_driverid(0).expect("an id");
    crypto_register(
        hw,
        &algs(&[CRYPTO_AES_CBC]),
        fake_new,
        fake_free,
        fake_process_ok,
    )
    .expect("register");
    let sid = crypto_newsession(&cri(CRYPTO_AES_CBC), 0).expect("a session");
    crypto_unregister(hw, CRYPTO_ALGORITHM_MAX as i32 + 1).expect("unregister");
    let mut crp = crypto_getreq(1).expect("a request");
    crp.crp_sid = sid;
    crp.crp_desc[0].CRD_INI = cri(CRYPTO_AES_CBC);
    assert_eq!(crypto_invoke(&mut crp), Err(Errno::EAGAIN));
    assert_eq!(crp.crp_sid >> 32, 0);
    // The failed-over request freed the old session, which released the driver's slot.
    assert_eq!(with_drivers(|dr| dr[hw as usize].cc_flags), 0);
}
