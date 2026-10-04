//! Host tests for pool(9) over real memory.
//!
//! The host double's "physical" pages are numbers and its direct map is the identity, so a
//! test loads a leaked block of its own memory as the physical segment: the page frames are the
//! block's addresses, and everything the allocators hand out is memory the test can touch.

use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::kern::kern_malloc::kmeminit;
use crate::sys::param::PAGE_MASK;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_page::{
    uvm_page_init, uvm_page_physload, uvm_page_test_reset, uvm_setpagesize,
};
use crate::uvm::uvm_param::atop;

/// The block each test loads.
const BLOCK_BYTES: usize = 32 << 20;

/// Loads a fresh block of host memory as the physical memory, brings the page system up and
/// `kmeminit`; holds the page system's lock for the test.
pub(crate) fn setup_real_memory() -> MutexGuard<'static, ()> {
    let guard = crate::uvm::uvm_pmemrange::tests::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    uvm_page_test_reset();
    UVMEXP.pagesize.store(PAGE_SIZE as i32, Ordering::Relaxed);
    uvm_setpagesize();

    let block: &'static mut [u8] = vec![0u8; BLOCK_BYTES + PAGE_SIZE].leak();
    let base = (block.as_ptr() as usize + PAGE_MASK) & !PAGE_MASK;
    let (start, end) = (atop(base), atop(base + BLOCK_BYTES));
    uvm_page_physload(start, end, start, end, 0);
    // physmem, as the machine's bootstrap records it: kmeminit_nkmempages sizes kmem_map
    // and, under KMEMSTATS, every type's ks_limit from it.
    crate::sys::systm::PHYSMEM.store(end - start, Ordering::Relaxed);

    let (mut s, mut e) = (
        crate::sys::types::Vaddr::new(0),
        crate::sys::types::Vaddr::new(0),
    );
    uvm_page_init(&mut s, &mut e);
    kmeminit();
    // The radix trees' globals (`rn_zeros`, the mask tree) point into the memory just replaced:
    // the next `rn_init` (`vfsinit`'s, `pfr_initialize`'s) starts over.
    crate::net::radix::rn_test_reset();
    guard
}

fn inside_a_page_of(pp: &Pool, p: NonNull<u8>) -> bool {
    let page = (p.as_ptr() as usize) & pp.pr_pgmask.get();
    let ph = pr_find_pagehead(pp, p.as_ptr());
    ph.ph_page.get() as usize == page || !pool_inpghdr(pp)
}

#[test]
fn small_items_cycle_through_pages() {
    let _g = setup_real_memory();
    static P: Pool = Pool::new();
    pool_init(&P, 48, 0, IPL_HIGH, 0, "test48", None);
    assert!(
        pool_inpghdr(&P),
        "a 48-byte item pool keeps its header in the page"
    );
    assert_eq!(P.pr_size.get(), 48);
    let itemsperpage = P.pr_itemsperpage.get() as usize;
    assert!(itemsperpage > 60);

    let mut items: Vec<NonNull<u8>> = Vec::new();
    for i in 0..200u8 {
        let p = pool_get(&P, PR_NOWAIT).expect("an item");
        assert_eq!(p.as_ptr() as usize % 8, 0, "aligned");
        assert!(inside_a_page_of(&P, p));
        // SAFETY: a 48-byte item of the pool, this test's.
        unsafe { ptr::write_bytes(p.as_ptr(), i, 48) };
        items.push(p);
    }
    let mut sorted = items.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 200, "every item is distinct");
    assert_eq!(P.pr_nout.get(), 200);
    assert_eq!(P.pr_npages.get() as usize, 200usize.div_ceil(itemsperpage));

    for (i, p) in items.iter().enumerate() {
        // SAFETY: as above.
        assert!((unsafe { ptr::read_volatile(p.as_ptr()) }) == i as u8);
        pool_put(&P, *p);
    }
    assert_eq!(P.pr_nout.get(), 0);
    assert_eq!(P.pr_nidle.get() as u32, P.pr_npages.get());

    let free_before = UVMEXP.free.load(Ordering::Relaxed);
    assert!(pool_reclaim(&P));
    assert_eq!(P.pr_npages.get(), 0);
    assert!(UVMEXP.free.load(Ordering::Relaxed) > free_before);
    pool_destroy(&P);
}

#[test]
fn large_items_keep_their_headers_in_phpool() {
    let _g = setup_real_memory();
    static P: Pool = Pool::new();
    pool_init(&P, 1024, 0, IPL_HIGH, 0, "test1k", None);
    // 1 KiB items make the pool use 8 KiB pages (size * 8 > PAGE_SIZE), which the items fill
    // exactly, so the header goes off-page into phpool.
    assert!(!pool_inpghdr(&P), "the header goes off-page");
    assert_eq!(P.pr_pgsize.get() as usize, 2 * PAGE_SIZE);
    assert_eq!(P.pr_itemsperpage.get(), 8);

    let items: Vec<NonNull<u8>> = (0..10)
        .map(|_| pool_get(&P, PR_NOWAIT).expect("an item"))
        .collect();
    assert_eq!(P.pr_npages.get(), 2);
    assert_eq!(P.pr_phtree.iter().count(), 2);
    for p in &items {
        let ph = pr_find_pagehead(&P, p.as_ptr());
        let page = ph.ph_page.get() as usize;
        let pgsize = P.pr_pgsize.get() as usize;
        assert!(page <= p.as_ptr() as usize && (p.as_ptr() as usize) < page + pgsize);
    }
    for p in items {
        pool_put(&P, p);
    }
    assert!(pool_reclaim(&P));
    assert_eq!(P.pr_phtree.iter().count(), 0);
    pool_destroy(&P);
}

#[test]
fn hard_limit_and_limitfail() {
    let _g = setup_real_memory();
    static P: Pool = Pool::new();
    pool_init(&P, 64, 0, IPL_HIGH, 0, "testlim", None);
    assert_eq!(pool_sethardlimit(&P, 2), Ok(()));
    let a = pool_get(&P, PR_NOWAIT).expect("first");
    let b = pool_get(&P, PR_NOWAIT).expect("second");
    assert!(pool_get(&P, PR_NOWAIT | PR_LIMITFAIL).is_none());
    assert_eq!(P.pr_nfail.get(), 1);
    assert_eq!(pool_sethardlimit(&P, 1), Err(Errno::EINVAL));
    pool_put(&P, a);
    pool_put(&P, b);
    pool_destroy(&P);
}

#[test]
fn pr_zero_clears_a_reused_item() {
    let _g = setup_real_memory();
    static P: Pool = Pool::new();
    pool_init(&P, 96, 0, IPL_HIGH, 0, "testzero", None);
    let p = pool_get(&P, PR_NOWAIT).expect("an item");
    // SAFETY: a 96-byte item of the pool, this test's.
    unsafe { ptr::write_bytes(p.as_ptr(), 0xa5, 96) };
    pool_put(&P, p);
    let q = pool_get(&P, PR_NOWAIT | PR_ZERO).expect("an item");
    // SAFETY: as above.
    assert!((0..96).all(|i| (unsafe { ptr::read_volatile(q.as_ptr().add(i)) }) == 0));
    pool_put(&P, q);
    pool_destroy(&P);
}

#[test]
fn setlowat_primes_pages_and_reclaim_keeps_them() {
    let _g = setup_real_memory();
    static P: Pool = Pool::new();
    pool_init(&P, 128, 0, IPL_HIGH, 0, "testlow", None);
    pool_setlowat(&P, 100);
    assert!(P.pr_nitems.get() >= 100);
    assert!(P.pr_npages.get() >= 2);
    assert!(
        !pool_reclaim(&P),
        "nothing above the low water mark to release"
    );
    assert!(P.pr_npages.get() >= 2);
    pool_setlowat(&P, 0);
    assert!(pool_reclaim(&P));
    assert_eq!(P.pr_npages.get(), 0);
    pool_destroy(&P);
}

#[test]
fn freelist_order_is_randomised_and_items_do_not_overlap() {
    let _g = setup_real_memory();
    static P: Pool = Pool::new();
    pool_init(&P, 200, 0, IPL_HIGH, 0, "testrnd", None);
    let items: Vec<usize> = (0..40)
        .map(|_| pool_get(&P, PR_NOWAIT).expect("an item").as_ptr() as usize)
        .collect();
    let ascending = items.windows(2).all(|w| w[1] > w[0]);
    let descending = items.windows(2).all(|w| w[1] < w[0]);
    assert!(!ascending && !descending, "the free list is shuffled");
    let mut sorted = items.clone();
    sorted.sort_unstable();
    assert!(sorted.windows(2).all(|w| w[1] - w[0] >= 200), "no overlap");
    for p in items {
        pool_put(&P, NonNull::new(p as *mut u8).expect("non-null"));
    }
    pool_destroy(&P);
}
