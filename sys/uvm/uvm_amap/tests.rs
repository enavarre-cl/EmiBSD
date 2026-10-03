//! Host tests for the amap layer over real memory (see `subr_pool/tests.rs` for the setup).

use std::sync::MutexGuard;
use std::{assert, assert_eq};

use super::*;
use crate::kern::kern_rwlock::rw_obj_init;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::uvm::uvm_anon::uvm_anon_init;

/// Real memory, the lock objects, the anon pool and the amap pools.
fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_real_memory();
    rw_obj_init();
    uvm_anon_init();
    amap_init();
    guard
}

/// An anon that shares `amap`'s lock, as `uvm_fault` makes them.
fn anon_for(amap: &VmAmap) -> &'static VmAnon {
    let anon = uvm_analloc().expect("an anon");
    anon.an_lock.set(amap.am_lock.get());
    anon
}

#[test]
fn small_amap_add_lookup_unadd() {
    let _g = setup();
    let amap = amap_alloc(4 * PAGE_SIZE, M_WAITOK, false).expect("an amap");
    assert!(amap.is_small());
    assert_eq!(amap.am_nslot.get(), 4);
    assert_eq!(amap_refs(amap), 1);
    let aref = VmAref::new();
    aref.ar_amap.set(amap);

    let anon = anon_for(amap);
    amap_lock(amap, RW_WRITE);
    assert!(amap_add(&aref, 2 * PAGE_SIZE, anon, false));
    assert_eq!(amap.am_nused.get(), 1);
    assert!(amap_lookup(&aref, 2 * PAGE_SIZE).is_some_and(|a| ptr::eq(a, anon)));
    assert!(amap_lookup(&aref, 0).is_none());

    let mut anons: [*const VmAnon; 4] = [ptr::null(); 4];
    amap_lookups(&aref, 0, &mut anons);
    assert!(anons[0].is_null() && anons[1].is_null() && anons[3].is_null());
    assert!(ptr::eq(anons[2], anon));

    amap_unadd(&aref, 2 * PAGE_SIZE);
    assert_eq!(amap.am_nused.get(), 0);
    assert!(amap_lookup(&aref, 2 * PAGE_SIZE).is_none());
    // the anon left the amap with its reference: drop it under the shared lock
    anon.an_ref.set(0);
    uvm_anfree(anon);
    amap_unlock(amap);

    amap_unref(amap, 0, 4, true);
}

#[test]
fn large_amap_chunks_and_buckets() {
    let _g = setup();
    let slots = UVM_AMAP_LARGE + 5;

    // eager: every chunk exists, the last one short
    let eager = amap_alloc(slots * PAGE_SIZE, M_WAITOK, false).expect("an amap");
    assert!(!eager.is_small());
    assert_eq!(eager.am_hashshift.get(), 0);
    assert_eq!(eager.am_nbuckets.get(), 17);
    assert_eq!(eager.am_ncused.get(), 17);
    let last = eager.am_chunks.iter().last().expect("a chunk");
    assert_eq!(last.ac_baseslot.get(), 256);
    assert_eq!(last.ac_nslot.get(), 5);
    amap_unref(eager, 0, slots, true);

    // lazy: 17 chunks hash into 5 buckets of 4 (log2(17) rounds to 5, so 4 + 1 < 5 fails)
    let lazy = amap_alloc(slots * PAGE_SIZE, M_WAITOK, true).expect("an amap");
    assert_eq!(lazy.am_hashshift.get(), 2);
    assert_eq!(lazy.am_nbuckets.get(), 5);
    assert_eq!(lazy.am_ncused.get(), 0);
    let aref = VmAref::new();
    aref.ar_amap.set(lazy);
    let (a0, a1) = (anon_for(lazy), anon_for(lazy));
    amap_lock(lazy, RW_WRITE);
    assert!(amap_add(&aref, 0, a0, false));
    assert!(amap_add(&aref, 260 * PAGE_SIZE, a1, false));
    assert_eq!(lazy.am_ncused.get(), 2);
    assert_eq!(lazy.am_nused.get(), 2);
    assert!(amap_lookup(&aref, 260 * PAGE_SIZE).is_some_and(|a| ptr::eq(a, a1)));
    assert!(amap_lookup(&aref, 100 * PAGE_SIZE).is_none());
    amap_unadd(&aref, 0);
    assert_eq!(
        lazy.am_ncused.get(),
        1,
        "an emptied chunk goes back to the pool"
    );
    a0.an_ref.set(0);
    uvm_anfree(a0);
    amap_unlock(lazy);
    amap_unref(lazy, 0, slots, true);
}

#[test]
fn ppref_counts_partial_references() {
    let _g = setup();
    let amap = amap_alloc(8 * PAGE_SIZE, M_WAITOK, false).expect("an amap");
    amap_ref(amap, 0, 8, AMAP_REFALL);
    assert_eq!(amap_refs(amap), 2);
    assert!(
        amap.am_ppref.get().is_null(),
        "a whole reference needs no ppref"
    );

    amap_ref(amap, 2, 4, 0);
    assert_eq!(amap_refs(amap), 3);
    let ppref = amap.ppref().expect("a ppref array");
    assert_eq!(pp_getreflen(ppref, 0), (2, 2));
    assert_eq!(pp_getreflen(ppref, 2), (3, 4));
    assert_eq!(pp_getreflen(ppref, 6), (2, 2));

    amap_unref(amap, 2, 4, false);
    assert_eq!(amap_refs(amap), 2);
    assert_eq!(
        pp_getreflen(ppref, 0),
        (2, 6),
        "the first changed entry merges with the unchanged one before it"
    );
    assert_eq!(pp_getreflen(ppref, 6), (2, 2));

    amap_unref(amap, 0, 8, true);
    assert_eq!(amap_refs(amap), 1);
    amap_unref(amap, 0, 8, true);
}

#[test]
fn amap_copy_takes_over_or_copies() {
    let _g = setup();
    let map = VmMap::new();
    let entry = VmMapEntry::new();
    entry.start.set(0x1000);
    entry.end.set(0x1000 + 4 * PAGE_SIZE);
    entry.etype.set(UVM_ET_NEEDSCOPY);

    // no amap yet: one is made for the entry
    amap_copy(&map, &entry, M_WAITOK, false, 0, 0);
    let amap = entry.aref.amap().expect("an amap");
    assert_eq!(amap.am_nslot.get(), 4);
    assert_eq!(entry.etype.get() & UVM_ET_NEEDSCOPY, 0);

    // a sole reference is taken over
    entry.etype.set(UVM_ET_NEEDSCOPY);
    amap_copy(&map, &entry, M_WAITOK, false, 0, 0);
    assert!(ptr::eq(entry.aref.amap().expect("the amap"), amap));
    assert_eq!(entry.etype.get() & UVM_ET_NEEDSCOPY, 0);

    // a shared amap is copied: the anons gain a reference, the source loses one
    let anon = anon_for(amap);
    amap_lock(amap, RW_WRITE);
    assert!(amap_add(&entry.aref, PAGE_SIZE, anon, false));
    amap_unlock(amap);
    amap_ref(amap, 0, 4, AMAP_REFALL); // the other map entry, as fork makes it
    entry.etype.set(UVM_ET_NEEDSCOPY);
    amap_copy(&map, &entry, M_WAITOK, false, 0, 0);
    let copy = entry.aref.amap().expect("the copy");
    assert!(!ptr::eq(copy, amap));
    assert_eq!(amap_refs(amap), 1);
    assert_eq!(copy.am_nused.get(), 1);
    assert_eq!(anon.an_ref.get(), 2);
    assert!(
        ptr::eq(copy.am_lock.get(), amap.am_lock.get()),
        "a copy with shared anons shares the lock"
    );
    amap_lock(copy, RW_WRITE);
    assert!(amap_lookup(&entry.aref, PAGE_SIZE).is_some_and(|a| ptr::eq(a, anon)));
    amap_unlock(copy);

    amap_unref(copy, 0, 4, true);
    assert_eq!(anon.an_ref.get(), 1);
    amap_unref(amap, 0, 4, true);
}
