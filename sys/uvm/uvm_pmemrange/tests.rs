//! Host tests for the physical page allocator over synthetic physical segments.
//!
//! The page system is global, so every test takes [`LOCK`], forgets the previous memory and
//! loads two segments: one inside the host's "ISA" range (use count 2 after `uvm_pmr_init`)
//! and one above 4 GiB (use count 0), so allocations prefer the second. The page array and
//! the ranges are stolen from the segments, so the free total is a little under the loaded
//! size; [`total`] reads it from the segments.

use std::sync::{Mutex, MutexGuard};
use std::vec::Vec;
use std::{assert, assert_eq, assert_ne, vec};

use super::*;
use crate::sys::param::PAGE_SIZE;
use crate::sys::types::Vaddr;
use crate::uvm::uvm_page::{
    PG_BUSY, PHYS_TO_VM_PAGE, uvm_page_init, uvm_page_physload, uvm_page_test_reset, uvm_pagealloc,
    uvm_pagefree, uvm_pglistalloc, uvm_pglistfree, uvm_setpagesize, vm_physmem,
};

/// Serialises the tests: the page system is one set of statics.
pub(crate) static LOCK: Mutex<()> = Mutex::new(());

/// A segment in the ISA range: pages 0x100..0x200 (1 MiB..2 MiB).
const SEG_ISA: (usize, usize) = (0x100, 0x200);
/// A segment above 4 GiB: pages 0x10_0000..0x10_1000 (16 MiB).
const SEG_HIGH: (usize, usize) = (0x10_0000, 0x10_1000);

fn pages(seg: (usize, usize)) -> usize {
    seg.1 - seg.0
}

/// The pages the segments hold after the boot-time stealing.
fn total() -> usize {
    vm_physmem()
        .iter()
        .map(|seg| seg.avail_end - seg.avail_start)
        .sum()
}

fn free() -> usize {
    UVMEXP.free.load(Ordering::Relaxed) as usize
}

fn in_seg(pg: &VmPage, seg: (usize, usize)) -> bool {
    (seg.0..seg.1).contains(&pgno(pg))
}

fn assert_all_valid() {
    for pmr in UVM.pmr_control.addr.iter() {
        uvm_pmr_assertvalid(pmr);
    }
}

fn setup() -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    uvm_page_test_reset();
    UVMEXP.pagesize.store(PAGE_SIZE as i32, Ordering::Relaxed);
    uvm_setpagesize();
    uvm_page_physload(SEG_HIGH.0, SEG_HIGH.1, SEG_HIGH.0, SEG_HIGH.1, 0);
    uvm_page_physload(SEG_ISA.0, SEG_ISA.1, SEG_ISA.0, SEG_ISA.1, 0);
    let (mut start, mut end) = (Vaddr::new(0), Vaddr::new(0));
    uvm_page_init(&mut start, &mut end);
    assert!(start < end, "the host reports a kernel virtual range");
    assert!(
        total() > pages(SEG_ISA) + pages(SEG_HIGH) - 256,
        "little was stolen"
    );
    guard
}

#[test]
fn init_frees_every_page_and_splits_the_ranges() {
    let _g = setup();
    assert_eq!(free(), total());
    assert_all_valid();

    // The host's constraints (ISA at 16 MiB, DMA at 4 GiB) split the single range in three,
    // sorted by use on the use queue: the unconstrained range first.
    let uses: Vec<i32> = UVM
        .pmr_control
        .r#use
        .iter()
        .map(|p| p.r#use.get())
        .collect();
    assert_eq!(uses, vec![0, 1, 2]);
    assert_eq!(UVM.pmr_control.addr.iter().count(), 3);

    let isa = uvm_pmemrange_find(SEG_ISA.0).expect("the ISA segment has a range");
    assert_eq!((isa.low.get(), isa.high.get()), (0, 0x1000));
    assert_eq!(isa.nsegs.get(), 1);
    let high = uvm_pmemrange_find(SEG_HIGH.0).expect("the high segment has a range");
    assert_eq!(high.low.get(), 0x10_0000);
    assert_eq!(high.nsegs.get(), 1);

    // Boot memory was stolen from a boundary of one segment (`uvm_page_physsteal`): what is
    // left of each segment is one free range.
    for seg in vm_physmem() {
        let pg = PHYS_TO_VM_PAGE(Paddr::new(seg.avail_start << crate::sys::param::PAGE_SHIFT))
            .expect("the first available page of a segment has a structure");
        assert!(uvm_pmr_isfree(pg));
        assert_eq!(pg.fpgsz.get(), seg.avail_end - seg.avail_start);
    }
    assert!(
        total() < pages(SEG_ISA) + pages(SEG_HIGH),
        "the page array came out of a segment"
    );
}

#[test]
fn getone_prefers_the_least_used_range() {
    let _g = setup();
    let pg = uvm_pmr_getone(UVM_PLA_NOWAIT).expect("a page");
    assert!(
        in_seg(pg, SEG_HIGH),
        "page {:#x} is not in the high segment",
        pgno(pg)
    );
    assert_eq!(pg.flags() & PQ_FREE, 0);
    assert_eq!(free(), total() - 1);
    assert!(!uvm_pmr_isfree(pg));
    assert_all_valid();

    uvm_pmr_freepages(pg, 1);
    assert_eq!(free(), total());
    assert!(uvm_pmr_isfree(pg));
    assert_all_valid();
    // The page joined its range again: one segment per range.
    let high = uvm_pmemrange_find(SEG_HIGH.0).expect("range");
    assert_eq!(high.nsegs.get(), 1);
}

#[test]
fn contiguous_aligned_and_bounded() {
    let _g = setup();
    let list = Pglist::new();
    list.init();
    let npages = 16;
    let r = uvm_pglistalloc(
        npages * PAGE_SIZE,
        Paddr::new(SEG_HIGH.0 << crate::sys::param::PAGE_SHIFT),
        Paddr::new((SEG_HIGH.1 << crate::sys::param::PAGE_SHIFT) - 1),
        Paddr::new(16 * PAGE_SIZE),
        Paddr::new(64 * PAGE_SIZE),
        &list,
        1,
        UVM_PLA_NOWAIT,
    );
    assert_eq!(r, Ok(()));
    let got: Vec<usize> = list.iter().map(pgno).collect();
    assert_eq!(got.len(), npages);
    assert_eq!(got[0] % 16, 0, "aligned");
    assert_eq!(got[0] / 64, got[npages - 1] / 64, "no boundary crossing");
    for w in got.windows(2) {
        assert_eq!(w[1], w[0] + 1, "contiguous");
    }
    assert!(got.iter().all(|&p| (SEG_HIGH.0..SEG_HIGH.1).contains(&p)));
    assert_eq!(free(), total() - npages);
    assert_all_valid();

    uvm_pglistfree(&list);
    assert!(list.is_empty());
    assert_eq!(free(), total());
    assert_all_valid();
}

#[test]
fn constrained_to_the_isa_range() {
    let _g = setup();
    let list = Pglist::new();
    list.init();
    let r = uvm_pglistalloc(
        4 * PAGE_SIZE,
        Paddr::new(0),
        Paddr::new(0x00ff_ffff),
        Paddr::new(0),
        Paddr::new(0),
        &list,
        4,
        UVM_PLA_NOWAIT,
    );
    assert_eq!(r, Ok(()));
    assert_eq!(list.iter().count(), 4);
    assert!(list.iter().all(|pg| in_seg(pg, SEG_ISA)));
    uvm_pglistfree(&list);
    assert_eq!(free(), total());
    assert_all_valid();
}

#[test]
fn exhaustion_fails_cleanly() {
    let _g = setup();
    let list = Pglist::new();
    list.init();

    // More than exists.
    let r = uvm_pmr_getpages(total() + 1, 0, 0, 1, 0, 1, UVM_PLA_NOWAIT, &list);
    assert_eq!(r, Err(Errno::ENOMEM));
    assert!(list.is_empty());
    assert_eq!(free(), total());

    // Everything but the reserves, in as many segments as it takes.
    let reserve = UVMEXP.reserve_kernel.load(Ordering::Relaxed) as usize;
    let count = total() - reserve - 1;
    let r = uvm_pmr_getpages(count, 0, 0, 1, 0, count as i32, UVM_PLA_NOWAIT, &list);
    assert_eq!(r, Ok(()));
    assert_eq!(list.iter().count(), count);
    assert_eq!(free(), total() - count);
    assert!(list.iter().all(|pg| pg.flags() & PQ_FREE == 0));
    assert_all_valid();

    // Into the reserve: refused without UVM_PLA_USERESERVE.
    let more = Pglist::new();
    more.init();
    let r = uvm_pmr_getpages(2, 0, 0, 1, 0, 2, UVM_PLA_NOWAIT, &more);
    assert_eq!(r, Err(Errno::ENOMEM));
    assert!(more.is_empty());

    uvm_pmr_freepageq(&list);
    assert!(list.is_empty());
    assert_eq!(free(), total());
    assert_all_valid();
    for pmr in UVM.pmr_control.addr.iter() {
        assert!(pmr.nsegs.get() <= 1, "every range is one free chunk again");
    }
}

#[test]
fn freeing_a_descending_list_joins_the_range() {
    let _g = setup();
    let list = Pglist::new();
    list.init();
    let r = uvm_pmr_getpages(32, 0, 0, 1, 0, 1, UVM_PLA_NOWAIT, &list);
    assert_eq!(r, Ok(()));

    // Rebuild the list with the pages in descending order of physical address.
    let reversed = Pglist::new();
    reversed.init();
    while let Some(pg) = list.first() {
        // SAFETY: `pg` is the head of `list`; it goes to `reversed` right after.
        unsafe {
            list.remove(pg);
            reversed.insert_head(pg);
        }
    }
    let order: Vec<usize> = reversed.iter().map(pgno).collect();
    assert!(order.windows(2).all(|w| w[0] == w[1] + 1));

    uvm_pmr_freepageq(&reversed);
    assert!(reversed.is_empty());
    assert_eq!(free(), total());
    assert_all_valid();
    let high = uvm_pmemrange_find(SEG_HIGH.0).expect("range");
    assert_eq!(high.nsegs.get(), 1);
}

#[test]
fn zeroed_pages_are_counted() {
    let _g = setup();
    let before = UVMEXP.pga_zeromiss.load(Ordering::Relaxed);
    let pg = uvm_pmr_getone(UVM_PLA_NOWAIT | UVM_PLA_ZERO).expect("a page");
    assert_eq!(UVMEXP.pga_zeromiss.load(Ordering::Relaxed), before + 1);
    assert_eq!(pg.flags() & (PG_ZERO | PQ_FREE), 0);
    uvm_pmr_freepages(pg, 1);
    assert_eq!(free(), total());
}

#[test]
fn pagealloc_and_pagefree() {
    let _g = setup();
    let pg = uvm_pagealloc(None, 0, None, 0).expect("a page");
    assert_ne!(pg.flags() & PG_BUSY, 0);
    assert_eq!(pg.flags() & PQ_FREE, 0);
    assert!(pg.uobject().is_none());
    assert_eq!(free(), total() - 1);

    uvm_pagefree(pg);
    assert_eq!(free(), total());
    assert!(uvm_pmr_isfree(pg));
    assert_all_valid();
}

#[test]
fn pow2divide_rounds_up_to_a_power_of_two() {
    assert_eq!(pow2divide(1, 8), 1);
    assert_eq!(pow2divide(8, 8), 1);
    assert_eq!(pow2divide(9, 8), 2);
    assert_eq!(pow2divide(100, 7), 16);
    assert_eq!(pow2divide(4096, 1), 4096);
}

#[test]
fn range_predicates() {
    assert!(pmr_is_subrange_of(10, 20, 0, 0));
    assert!(pmr_is_subrange_of(10, 20, 10, 20));
    assert!(!pmr_is_subrange_of(10, 21, 10, 20));
    assert!(pmr_intersects_with(10, 20, 19, 0));
    assert!(!pmr_intersects_with(10, 20, 20, 0));
    assert!(pmr_intersects_with(10, 20, 0, 11));
    assert!(!pmr_intersects_with(10, 20, 0, 10));
    assert_eq!(pmr_align(17, 16), 32);
    assert_eq!(pmr_align_down(17, 16), 16);
}
