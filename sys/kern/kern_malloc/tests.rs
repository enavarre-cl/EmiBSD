//! Host tests for `malloc(9)` over real memory (see `subr_pool/tests.rs` for the setup).

use std::vec::Vec;
use std::{assert, assert_eq};

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::malloc::{M_DEVBUF, M_TEMP};

#[test]
fn bucketindx_follows_the_c_formula() {
    assert_eq!(bucketindx(1), 4);
    assert_eq!(bucketindx(16), 4);
    assert_eq!(bucketindx(17), 5);
    assert_eq!(bucketindx(100), 7);
    assert_eq!(bucketindx(4096), 12);
    assert_eq!(bucketindx(4097), 13);
    assert_eq!(bucketindx(8192), 13);
    assert_eq!(bucketindx(65536), 16);
}

#[test]
fn small_blocks_come_from_buckets_and_are_reused() {
    let _g = setup_real_memory();
    let first: Vec<NonNull<u8>> = (0..40)
        .map(|i| {
            let p = malloc(100, M_TEMP, M_NOWAIT).expect("a block");
            assert_eq!(
                p.as_ptr() as usize % 128,
                0,
                "a 100-byte request is a 128-byte chunk"
            );
            // SAFETY: 100 bytes at `p` are this test's.
            unsafe { ptr::write_bytes(p.as_ptr(), i as u8, 100) };
            p
        })
        .collect();
    let mut addrs: Vec<usize> = first.iter().map(|p| p.as_ptr() as usize).collect();
    addrs.sort_unstable();
    assert!(
        addrs.windows(2).all(|w| w[1] - w[0] >= 128),
        "distinct chunks"
    );
    let pages_used = UVMEXP.free.load(Ordering::Relaxed);
    let mut pages: Vec<usize> = addrs.iter().map(|a| a & !(PAGE_SIZE - 1)).collect();
    pages.dedup();
    assert_eq!(pages.len(), 2, "40 chunks of 128 bytes take two pages");
    for (i, p) in first.iter().enumerate() {
        // SAFETY: as above.
        assert_eq!(unsafe { ptr::read_volatile(p.as_ptr()) }, i as u8);
        free(*p, M_TEMP, 100);
    }
    // The bucket keeps its pages: a second round takes chunks from the same two pages (the
    // never-used ones first, then the freed ones) and allocates nothing new.
    let again: Vec<usize> = (0..40)
        .map(|_| malloc(100, M_TEMP, M_NOWAIT).expect("a block").as_ptr() as usize)
        .collect();
    assert!(
        again
            .iter()
            .all(|a| pages.contains(&(a & !(PAGE_SIZE - 1))))
    );
    assert_eq!(
        UVMEXP.free.load(Ordering::Relaxed),
        pages_used,
        "no new page"
    );
    for p in again {
        free(NonNull::new(p as *mut u8).expect("non-null"), M_TEMP, 100);
    }
}

#[test]
fn large_blocks_take_and_return_pages() {
    let _g = setup_real_memory();
    let free_before = UVMEXP.free.load(Ordering::Relaxed);
    let p = malloc(20_000, M_DEVBUF, M_NOWAIT).expect("a block");
    assert_eq!(p.as_ptr() as usize % PAGE_SIZE, 0, "page aligned");
    assert_eq!(UVMEXP.free.load(Ordering::Relaxed), free_before - 5);
    // SAFETY: 20000 bytes at `p` are this test's.
    unsafe { ptr::write_bytes(p.as_ptr(), 0x5a, 20_000) };
    free(p, M_DEVBUF, 20_000);
    assert_eq!(UVMEXP.free.load(Ordering::Relaxed), free_before);
}

#[test]
fn m_zero_zeroes_and_mallocarray_refuses_overflow() {
    let _g = setup_real_memory();
    let p = malloc(300, M_TEMP, M_NOWAIT).expect("a block");
    // SAFETY: 300 bytes at `p` are this test's.
    unsafe { ptr::write_bytes(p.as_ptr(), 0xff, 300) };
    free(p, M_TEMP, 300);
    let q = malloc(300, M_TEMP, M_NOWAIT | M_ZERO).expect("a block");
    // SAFETY: as above.
    assert!((0..300).all(|i| (unsafe { ptr::read_volatile(q.as_ptr().add(i)) }) == 0));
    free(q, M_TEMP, 300);

    assert!(mallocarray(usize::MAX / 2, 4, M_TEMP, M_NOWAIT | M_CANFAIL).is_none());
    let r = mallocarray(3, 8, M_TEMP, M_NOWAIT).expect("a block");
    free(r, M_TEMP, 24);
    assert!(malloc(MALLOC_MAX + 1, M_TEMP, M_NOWAIT | M_CANFAIL).is_none());
}
