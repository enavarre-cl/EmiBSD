//! Host tests for the fault handler: a page fault on an anonymous mapping of a user vmspace,
//! served against the host pmap double (see `uvm_map/tests.rs` for the map side).

use std::sync::MutexGuard;
use std::{assert, assert_eq, assert_ne};

use super::*;
use crate::kern::kern_rwlock::rw_obj_init;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::machine::vmparam::VmParam;
use crate::sys::mman::{MAP_INHERIT_COPY, PROT_EXEC, PROT_READ};
use crate::sys::proc::Process;
use crate::uvm::uvm_amap::{amap_init, amap_refs};
use crate::uvm::uvm_anon::uvm_anon_init;
use crate::uvm::uvm_extern::{UVM_FLAG_COPYONW, UVM_FLAG_FIXED, Vmspace, uvm_mapflag};
use crate::uvm::uvm_map::{
    uvm_map_init, uvm_mapanon, uvmspace_alloc, uvmspace_fork, uvmspace_free,
};
use crate::uvm::uvmexp::counters_read;

const RW: VmProt = PROT_READ | PROT_WRITE;

/// Real memory, the locks, the anon, amap and map pools and the console.
fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_real_memory();
    crate::machine::cons::consinit();
    rw_obj_init();
    uvm_anon_init();
    amap_init();
    uvm_map_init();
    uvmfault_init();
    guard
}

/// A vmspace with `len` bytes of private anonymous read-write memory at `addr`.
fn vmspace_with_anon(addr: usize, len: usize) -> &'static Vmspace {
    let vm = uvmspace_alloc(
        <Machine as VmParam>::VM_MIN_ADDRESS,
        <Machine as VmParam>::VM_MAXUSER_ADDRESS,
        true,
        false,
    );
    vm.vm_daddr.set(256 << 20);
    vm.vm_minsaddr.set(<Machine as VmParam>::USRSTACK);
    vm.vm_maxsaddr
        .set(<Machine as VmParam>::USRSTACK - <Machine as VmParam>::MAXSSIZ);
    let mut a = addr;
    let flags = uvm_mapflag(
        RW,
        RW | PROT_EXEC,
        MAP_INHERIT_COPY,
        MADV_NORMAL,
        UVM_FLAG_FIXED | UVM_FLAG_COPYONW,
    );
    uvm_mapanon(&vm.vm_map, &mut a, len, 0, flags).expect("mapanon");
    vm
}

/// The anon the amap of `vm` holds at `va`, under the map lock.
fn anon_of(vm: &Vmspace, va: usize) -> Option<&'static VmAnon> {
    let map = &vm.vm_map;
    vm_map_lock_read(map);
    let anon = uvm_map_lookup_entry(map, va).and_then(|e| amap_lookup(&e.aref, va - e.start.get()));
    vm_map_unlock_read(map);
    anon
}

#[test]
fn zero_fill_fault_maps_a_fresh_anon_page() {
    let _g = setup();
    let va = 0x2000_0000;
    let vm = vmspace_with_anon(va, 4 * PAGE_SIZE);
    let pmap = vm.vm_map.pmap();
    assert!(pmap_extract(pmap, Vaddr::new(va)).is_none());
    let faults = counters_read(UvmExpCounters::Faults);

    // A read fault: the lazy amap is made, an anon with a zeroed page is promoted and
    // mapped (case 2B), the neighbours are not (no anons yet).
    uvm_fault(&vm.vm_map, va + 8, VM_FAULT_INVALID, PROT_READ).expect("read fault");
    let pa = pmap_extract(pmap, Vaddr::new(va)).expect("mapped");
    let anon = anon_of(vm, va).expect("an anon");
    let pg = anon.page().expect("its page");
    assert_eq!(vm_page_to_phys(pg), pa.trunc_page());
    assert_ne!(pg.flags() & PQ_ANON, 0);
    assert_eq!(pg.flags() & (PG_BUSY | PG_FAKE), 0);
    assert_eq!(anon.an_ref.get(), 1);
    assert_eq!(counters_read(UvmExpCounters::Faults), faults + 1);

    // A write fault on the same page is case 1A: the same anon and page stay.
    uvm_fault(&vm.vm_map, va, VM_FAULT_INVALID, PROT_WRITE).expect("write fault");
    assert!(anon_of(vm, va).is_some_and(|a| core::ptr::eq(a, anon)));
    assert_eq!(pmap_extract(pmap, Vaddr::new(va)), Some(pa));

    // The next page faults in with its own anon; the first stays.
    uvm_fault(&vm.vm_map, va + PAGE_SIZE, VM_FAULT_INVALID, PROT_WRITE).expect("second page");
    let anon2 = anon_of(vm, va + PAGE_SIZE).expect("second anon");
    assert!(!core::ptr::eq(anon, anon2));
    uvmspace_free(vm);
}

#[test]
fn faults_outside_or_beyond_protection_fail() {
    let _g = setup();
    let va = 0x3000_0000;
    let vm = vmspace_with_anon(va, PAGE_SIZE);

    // Free space: a case 0 fault.
    assert_eq!(
        uvm_fault(&vm.vm_map, va + 0x10_0000, VM_FAULT_INVALID, PROT_READ),
        Err(Errno::EFAULT)
    );
    // Outside the map.
    assert_eq!(
        uvm_fault(&vm.vm_map, 0, VM_FAULT_INVALID, PROT_READ),
        Err(Errno::EFAULT)
    );
    // More access than the entry allows.
    assert_eq!(
        uvm_fault(&vm.vm_map, va, VM_FAULT_INVALID, PROT_EXEC),
        Err(Errno::EACCES)
    );
    uvmspace_free(vm);
}

#[test]
fn copy_on_write_after_fork_promotes_a_new_anon() {
    let _g = setup();
    let va = 0x4000_0000;
    let vm = vmspace_with_anon(va, PAGE_SIZE);
    uvm_fault(&vm.vm_map, va, VM_FAULT_INVALID, PROT_WRITE).expect("parent fault");
    let parent_anon = anon_of(vm, va).expect("parent anon");
    let parent_pa = pmap_extract(vm.vm_map.pmap(), Vaddr::new(va)).expect("parent pa");

    let pr = Process::new();
    pr.ps_vmspace.set(vm);
    let child = uvmspace_fork(&pr);
    // The amap is shared and the anon has two references until the child writes.
    let parent_amap = {
        let map = &vm.vm_map;
        vm_map_lock_read(map);
        let amap = uvm_map_lookup_entry(map, va).and_then(|e| e.aref.amap());
        vm_map_unlock_read(map);
        amap.expect("parent amap")
    };
    assert_eq!(amap_refs(parent_amap), 2);

    // The child's read fault sees the parent's page, read-only (case 1A, an_ref > 1).
    uvm_fault(&child.vm_map, va, VM_FAULT_INVALID, PROT_READ).expect("child read");
    assert_eq!(
        pmap_extract(child.vm_map.pmap(), Vaddr::new(va)),
        Some(parent_pa)
    );

    // The child's write fault copies the amap (needs_copy), then promotes a new anon with
    // a copy of the page (case 1B): the two maps now differ.
    uvm_fault(&child.vm_map, va, VM_FAULT_INVALID, PROT_WRITE).expect("child write");
    let child_anon = anon_of(child, va).expect("child anon");
    assert!(!core::ptr::eq(child_anon, parent_anon));
    assert_eq!(parent_anon.an_ref.get(), 1);
    assert_eq!(child_anon.an_ref.get(), 1);
    let child_pa = pmap_extract(child.vm_map.pmap(), Vaddr::new(va)).expect("child pa");
    assert_ne!(child_pa, parent_pa);
    assert_eq!(
        pmap_extract(vm.vm_map.pmap(), Vaddr::new(va)),
        Some(parent_pa)
    );
    assert_eq!(amap_refs(parent_amap), 1);
    assert!(counters_read(UvmExpCounters::FltAcow) >= 1);

    uvmspace_free(child);
    uvmspace_free(vm);
}

#[test]
fn wire_and_unwire_a_range() {
    let _g = setup();
    let va = 0x5000_0000;
    let len = 3 * PAGE_SIZE;
    let vm = vmspace_with_anon(va, len);
    let pmap = vm.vm_map.pmap();

    uvm_fault_wire(&vm.vm_map, va, va + len, RW).expect("wire");
    for i in 0..3 {
        let pa = pmap_extract(pmap, Vaddr::new(va + i * PAGE_SIZE)).expect("mapped");
        let pg = PHYS_TO_VM_PAGE(pa).expect("a managed page");
        assert_eq!(pg.wire_count.get(), 1, "page {i} wired");
        assert_eq!(pg.flags() & PG_CLEAN, 0);
    }

    uvm_fault_unwire(&vm.vm_map, va, va + len);
    for i in 0..3 {
        let pa = pmap_extract(pmap, Vaddr::new(va + i * PAGE_SIZE)).expect("still mapped");
        let pg = PHYS_TO_VM_PAGE(pa).expect("a managed page");
        assert_eq!(pg.wire_count.get(), 0, "page {i} unwired");
    }
    uvmspace_free(vm);
}

#[test]
fn neighbour_anons_are_mapped_ahead() {
    let _g = setup();
    let va = 0x6000_0000;
    let vm = vmspace_with_anon(va, 8 * PAGE_SIZE);
    let pmap = vm.vm_map.pmap();

    // Fault the pages in one by one, then drop their mappings: the anons stay.
    for i in 0..8 {
        uvm_fault(&vm.vm_map, va + i * PAGE_SIZE, VM_FAULT_INVALID, PROT_WRITE).expect("fault");
    }
    crate::machine::pmap::pmap_remove(pmap, Vaddr::new(va), Vaddr::new(va + 8 * PAGE_SIZE));
    assert!(pmap_extract(pmap, Vaddr::new(va + 4 * PAGE_SIZE)).is_none());

    // One read fault in the middle maps the MADV_NORMAL window (3 back, 4 ahead) too.
    uvm_fault(&vm.vm_map, va + 4 * PAGE_SIZE, VM_FAULT_INVALID, PROT_READ).expect("refault");
    for i in 1..8 {
        assert!(
            pmap_extract(pmap, Vaddr::new(va + i * PAGE_SIZE)).is_some(),
            "page {i} mapped by the fault-ahead"
        );
    }
    assert!(
        pmap_extract(pmap, Vaddr::new(va)).is_none(),
        "page 0 is behind the window"
    );
    assert!(counters_read(UvmExpCounters::FltNamap) >= 6);
    uvmspace_free(vm);
}
