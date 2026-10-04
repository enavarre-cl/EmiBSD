//! Host tests for the anonymous object pager over real memory (see `subr_pool/tests.rs` for
//! the setup).

use std::sync::MutexGuard;
use std::{assert, assert_eq};

use super::*;
use crate::kern::kern_rwlock::rw_obj_init;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::uvm::uvm_page::uvm_page_unbusy;
use crate::uvm::uvm_pager::PGO_DONTCARE;

/// Real memory, the lock objects and the aobj pools.
fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_real_memory();
    rw_obj_init();
    uao_init();
    guard
}

#[test]
fn small_aobj_keeps_swap_slots_in_an_array() {
    let _g = setup();
    let uobj = uao_create(Vsize::new(4 * PAGE_SIZE), 0).expect("an aobj");
    let aobj = aobj(uobj);
    assert!(uvm_obj_is_aobj(uobj));
    assert!(!aobj.uses_swhash());
    assert_eq!(aobj.u_pages.get(), 4);
    assert_eq!(uobj.uo_refs.get(), 1);

    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
    assert_eq!(uao_find_swslot(uobj, 1), 0);
    assert_eq!(uao_set_swslot(uobj, 1, 7), 0);
    assert_eq!(uao_find_swslot(uobj, 1), 7);
    assert_eq!(uao_set_swslot(uobj, 1, 9), 7);
    assert_eq!(uao_dropswap(uobj, 1), 9);
    assert_eq!(uao_find_swslot(uobj, 1), 0);
    rw_exit(uobj.vmobjlock());

    uao_reference(uobj);
    assert_eq!(uobj.uo_refs.get(), 2);
    uao_detach(uobj);
    assert_eq!(uobj.uo_refs.get(), 1);
    uao_detach(uobj);
}

#[test]
fn large_aobj_hashes_swap_slots() {
    let _g = setup();
    let pages = (UAO_SWHASH_THRESHOLD + 1) as usize;
    let uobj = uao_create(Vsize::new(pages * PAGE_SIZE), 0).expect("an aobj");
    let aobj = aobj(uobj);
    assert!(aobj.uses_swhash());
    assert_eq!(aobj.swhashmask(), 3, "65 pages hash into 4 buckets");

    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
    assert_eq!(uao_set_swslot(uobj, 3, 11), 0);
    assert_eq!(uao_set_swslot(uobj, 5, 12), 0); // the same cluster
    assert_eq!(uao_set_swslot(uobj, 40, 13), 0); // another one
    let elt = uao_find_swhash_elt(aobj, 3, false, false).expect("an element");
    assert_eq!(elt.count.get(), 2);
    assert_eq!(uao_find_swslot(uobj, 5), 12);
    assert_eq!(uao_find_swslot(uobj, 40), 13);
    assert_eq!(uao_find_swslot(uobj, 41), 0);

    assert_eq!(uao_set_swslot(uobj, 3, 0), 11);
    assert_eq!(elt.count.get(), 1);
    assert_eq!(uao_set_swslot(uobj, 5, 0), 12);
    assert!(
        uao_find_swhash_elt(aobj, 3, false, false).is_none(),
        "an element with no slots left goes back to the pool"
    );

    // the slot at 40 stands for a page only in swap; dropping the range accounts for it
    UVMEXP.swpgonly.fetch_add(1, Ordering::Relaxed);
    uao_dropswap_range(uobj, 0, 0);
    assert_eq!(uao_find_swslot(uobj, 40), 0);
    assert_eq!(UVMEXP.swpgonly.load(Ordering::Relaxed), 0);
    rw_exit(uobj.vmobjlock());

    uao_detach(uobj);
}

#[test]
fn uao_get_zero_fills_and_finds_resident_pages() {
    let _g = setup();
    let uobj = uao_create(Vsize::new(4 * PAGE_SIZE), 0).expect("an aobj");
    let off = PAGE_SIZE as Voff;
    let mut pps: [*const VmPage; 1] = [ptr::null()];
    let mut npages = 1;

    // PGO_LOCKED on a page that is not resident: the caller must unlock and come back
    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
    let rv = uao_get(
        uobj,
        off,
        &mut pps,
        &mut npages,
        0,
        PROT_READ,
        0,
        PGO_LOCKED,
    );
    assert_eq!(rv, VM_PAGER_UNLOCK);
    assert_eq!(npages, 0);
    assert!(pps[0].is_null());

    // the full path allocates a zero-filled page, busy for the caller, and drops the lock
    npages = 1;
    let rv = uao_get(
        uobj,
        off,
        &mut pps,
        &mut npages,
        0,
        PROT_READ | PROT_WRITE,
        0,
        0,
    );
    assert_eq!(rv, VM_PAGER_OK);
    // SAFETY: `uao_get` returned OK, so the slot holds a page of the object.
    let pg = unsafe { pps[0].as_ref() }.expect("a page");
    assert!(pg.flags() & PG_BUSY != 0);
    assert_eq!(pg.flags() & PG_FAKE, 0);
    assert!(pg.flags() & PQ_AOBJ != 0);
    assert_eq!(pg.offset.get(), off);
    assert!(pg.uobject().is_some_and(|o| ptr::eq(o, uobj)));
    assert_eq!(uobj.uo_npages.get(), 1);

    // once unbusied, PGO_LOCKED finds it and leaves the PGO_DONTCARE slot alone
    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
    uvm_page_unbusy(&[Some(pg)]);
    let mut pps2: [*const VmPage; 2] = [PGO_DONTCARE, ptr::null()];
    let mut n2 = 2;
    let rv = uao_get(uobj, 0, &mut pps2, &mut n2, 1, PROT_READ, 0, PGO_LOCKED);
    assert_eq!(rv, VM_PAGER_OK);
    assert_eq!(n2, 1);
    assert!(ptr::eq(pps2[1], pg));
    assert!(pgo_dontcare(pps2[0]));

    // past the object: VM_PAGER_BAD, and the lock is dropped
    let mut pps3: [*const VmPage; 1] = [ptr::null()];
    let mut n3 = 1;
    let rv = uao_get(uobj, 4 * off, &mut pps3, &mut n3, 0, PROT_READ, 0, 0);
    assert_eq!(rv, VM_PAGER_BAD);
    assert_eq!(n3, 0);
    assert!(!rw_write_held(uobj.vmobjlock()));

    // the last reference frees the page with the object
    uao_detach(uobj);
}

#[cfg(feature = "tmpfs")]
#[test]
fn grow_and_shrink_keep_the_swap_slots_below_the_end() {
    let _g = setup();
    let uobj = uao_create(Vsize::new(4 * PAGE_SIZE), 0).expect("an aobj");
    let aobj = aobj(uobj);

    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
    assert_eq!(uao_set_swslot(uobj, 1, 7), 0);
    assert_eq!(uao_set_swslot(uobj, 3, 9), 0);

    // case 2: the array grows
    uao_grow(uobj, 8).expect("grow the array");
    assert!(!aobj.uses_swhash());
    assert_eq!(aobj.u_pages.get(), 8);
    assert_eq!(uao_find_swslot(uobj, 1), 7);
    assert_eq!(uao_find_swslot(uobj, 7), 0);

    // case 3: past the threshold the slots move to a hash table
    uao_grow(uobj, 100).expect("convert to a hash");
    assert!(aobj.uses_swhash());
    assert!(aobj.u_swslots.get().is_null());
    assert_eq!(uao_find_swslot(uobj, 1), 7);
    assert_eq!(uao_find_swslot(uobj, 3), 9);

    // case 1: a bigger table, the elements rehashed by their tags
    uao_grow(uobj, 300).expect("grow the hash");
    assert_eq!(aobj.swhashmask(), 31, "18 buckets round up to 32");
    assert_eq!(uao_set_swslot(uobj, 290, 11), 0);
    assert_eq!(uao_set_swslot(uobj, 70, 12), 0);
    assert_eq!(uao_find_swslot(uobj, 3), 9);

    // shrinking the hash drops the slots past the end (a page only in swap each)
    UVMEXP.swpgonly.fetch_add(1, Ordering::Relaxed);
    uao_shrink(uobj, 200).expect("shrink the hash");
    assert_eq!(aobj.u_pages.get(), 200);
    assert_eq!(UVMEXP.swpgonly.load(Ordering::Relaxed), 0);
    assert_eq!(uao_find_swslot(uobj, 70), 12);
    assert_eq!(uao_find_swslot(uobj, 1), 7);

    // case 1 of shrink: back to an array below the threshold
    UVMEXP.swpgonly.fetch_add(1, Ordering::Relaxed);
    uao_shrink(uobj, 10).expect("convert to an array");
    assert!(!aobj.uses_swhash());
    assert!(aobj.u_swhash.get().is_none());
    assert_eq!(UVMEXP.swpgonly.load(Ordering::Relaxed), 0);
    assert_eq!(uao_find_swslot(uobj, 1), 7);
    assert_eq!(uao_find_swslot(uobj, 3), 9);

    // case 2 of shrink: a smaller array (the slot at 3 goes)
    UVMEXP.swpgonly.fetch_add(1, Ordering::Relaxed);
    uao_shrink(uobj, 2).expect("shrink the array");
    assert_eq!(UVMEXP.swpgonly.load(Ordering::Relaxed), 0);
    assert_eq!(aobj.u_pages.get(), 2);
    assert_eq!(uao_find_swslot(uobj, 1), 7);
    assert_eq!(uao_set_swslot(uobj, 1, 0), 7);
    rw_exit(uobj.vmobjlock());

    uao_detach(uobj);
}

#[cfg(feature = "tmpfs")]
#[test]
fn shrink_frees_the_resident_pages_past_the_end() {
    let _g = setup();
    let uobj = uao_create(Vsize::new(4 * PAGE_SIZE), 0).expect("an aobj");
    for idx in [0, 3] {
        let mut pps: [*const VmPage; 1] = [ptr::null()];
        let mut npages = 1;
        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        let rv = uao_get(
            uobj,
            (idx * PAGE_SIZE) as Voff,
            &mut pps,
            &mut npages,
            0,
            PROT_READ | PROT_WRITE,
            0,
            0,
        );
        assert_eq!(rv, VM_PAGER_OK);
        // SAFETY: `uao_get` returned OK, so the slot holds a page of the object.
        let pg = unsafe { pps[0].as_ref() }.expect("a page");
        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        uvm_page_unbusy(&[Some(pg)]);
        rw_exit(uobj.vmobjlock());
    }
    assert_eq!(uobj.uo_npages.get(), 2);

    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
    uao_shrink(uobj, 1).expect("shrink");
    assert_eq!(uobj.uo_npages.get(), 1, "the page at index 3 is gone");
    assert!(uvm_pagelookup(uobj, 0).is_some());
    uao_grow(uobj, 4).expect("grow");
    assert_eq!(uobj.uo_npages.get(), 1);
    rw_exit(uobj.vmobjlock());

    uao_detach(uobj);
}
