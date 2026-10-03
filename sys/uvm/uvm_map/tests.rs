//! Host tests for the map operations over real memory (see `subr_pool/tests.rs` for the
//! setup). The map, the entry tree, the selectors and the vmspace life cycle run on the
//! host's pmap double; the tree checks of `VMMAP_DEBUG` are on in tests.

use std::sync::MutexGuard;
use std::{assert, assert_eq, assert_ne};

use super::*;
use crate::kern::kern_rwlock::rw_obj_init;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::mman::MAP_INHERIT_COPY;
use crate::uvm::uvm_amap::amap_init;
use crate::uvm::uvm_anon::uvm_anon_init;

const RW: VmProt = PROT_READ | PROT_WRITE;
const RWX: VmProt = PROT_READ | PROT_WRITE | PROT_EXEC;

/// Real memory, the lock objects, the anon and amap pools and the map pools.
fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_real_memory();
    // The console, so that a kernel panic in a test says why.
    crate::machine::cons::consinit();
    rw_obj_init();
    uvm_anon_init();
    amap_init();
    uvm_map_init();
    guard
}

/// A vmspace laid out as `exec` leaves one: data at 256 MiB, the stack below `USRSTACK`.
fn user_vmspace() -> &'static Vmspace {
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
    vm
}

/// `uvm_mapanon` flags for a private read-write mapping, `extra` on top.
fn anon_flags(extra: u32) -> u32 {
    uvm_mapflag(
        RW,
        RWX,
        MAP_INHERIT_COPY,
        MADV_NORMAL,
        UVM_FLAG_COPYONW | extra,
    )
}

/// The entry that holds `addr`, under the read lock.
fn entry_holding(map: &VmMap, addr: usize) -> Option<(usize, usize, VmProt, i32)> {
    vm_map_lock_read(map);
    let found = uvm_map_lookup_entry(map, addr).map(|e| {
        (
            e.start.get(),
            e.end.get(),
            e.protection.get(),
            e.etype.get(),
        )
    });
    vm_map_unlock_read(map);
    found
}

#[test]
fn setup_entries_cover_the_map() {
    let _g = setup();
    let vm = user_vmspace();
    let map = &vm.vm_map;
    uvm_tree_sanity(map, "test");
    uvm_tree_size_chk(map, "test");
    assert_eq!(map.size.get().as_usize(), 0);
    assert!(map.addr.min().is_some());
    assert!(map.uaddr_any[0].get().is_some(), "the rnd selector");
    assert!(
        map.uaddr_brk_stack.get().is_some(),
        "the stack/brk selector"
    );
    uvmspace_free(vm);
}

#[test]
fn mapanon_fixed_then_unmap() {
    let _g = setup();
    let vm = user_vmspace();
    let map = &vm.vm_map;
    let mut addr = 0x2000_0000;
    let len = 4 * PAGE_SIZE;

    uvm_mapanon(map, &mut addr, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("mapanon");
    assert_eq!(addr, 0x2000_0000);
    assert_eq!(map.size.get().as_usize(), len);
    assert_eq!(vm.vm_dused.get(), 4);
    let (start, end, prot, etype) = entry_holding(map, addr + PAGE_SIZE).expect("mapped");
    assert_eq!((start, end, prot), (addr, addr + len, RW));
    assert_ne!(etype & UVM_ET_COPYONWRITE, 0);
    assert_ne!(etype & UVM_ET_NEEDSCOPY, 0);
    assert!(
        entry_holding(map, addr + len).is_none(),
        "nothing past the end"
    );

    // The same range again is not available.
    let mut again = addr;
    assert_eq!(
        uvm_mapanon(map, &mut again, len, 0, anon_flags(UVM_FLAG_FIXED)),
        Err(Errno::ENOMEM)
    );

    uvm_unmap(map, addr, addr + len);
    assert_eq!(map.size.get().as_usize(), 0);
    assert_eq!(vm.vm_dused.get(), 0);
    assert!(entry_holding(map, addr).is_none());
    uvmspace_free(vm);
}

#[test]
fn mapanon_finds_space_and_honours_a_hint() {
    let _g = setup();
    let vm = user_vmspace();
    let map = &vm.vm_map;
    let len = 8 * PAGE_SIZE;

    let mut a = 0;
    uvm_mapanon(map, &mut a, len, 0, anon_flags(0)).expect("first");
    assert_eq!(a & PAGE_MASK, 0);
    assert!(a >= <Machine as VmParam>::VM_MIN_ADDRESS);
    assert!(a + len <= <Machine as VmParam>::VM_MAXUSER_ADDRESS);

    let mut b = 0;
    uvm_mapanon(map, &mut b, len, 0, anon_flags(0)).expect("second");
    assert!(b + len <= a || a + len <= b, "no overlap: {a:#x} {b:#x}");

    // A free, aligned address is taken as given.
    let mut c = 0x3000_0000;
    uvm_mapanon(map, &mut c, len, 0, anon_flags(0)).expect("hinted");
    assert_eq!(c, 0x3000_0000);

    // An alignment request is honoured.
    let mut d = 0;
    uvm_mapanon(map, &mut d, len, 1 << 21, anon_flags(0)).expect("aligned");
    assert_eq!(d & ((1 << 21) - 1), 0);

    assert_eq!(map.size.get().as_usize(), 4 * len);
    vm_map_lock_read(map);
    assert!(uvm_map_checkprot(map, c, c + len, PROT_READ));
    assert!(!uvm_map_checkprot(map, c, c + len, PROT_EXEC));
    assert!(
        !uvm_map_checkprot(map, c, c + 2 * len, PROT_READ),
        "a hole fails"
    );
    vm_map_unlock_read(map);
    uvmspace_free(vm);
}

#[test]
fn protect_clips_and_checks_limits() {
    let _g = setup();
    let vm = user_vmspace();
    let map = &vm.vm_map;
    let mut addr = 0x4000_0000;
    let len = 4 * PAGE_SIZE;
    uvm_mapanon(map, &mut addr, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("mapanon");

    // Beyond max_protection.
    assert_eq!(
        uvm_map_protect(map, addr, addr + len, RWX | 0x8, 0, false, true),
        Err(Errno::EACCES)
    );

    // The middle two pages become read-only: three entries.
    uvm_map_protect(
        map,
        addr + PAGE_SIZE,
        addr + 3 * PAGE_SIZE,
        PROT_READ,
        0,
        false,
        true,
    )
    .expect("protect");
    let (s0, e0, p0, _) = entry_holding(map, addr).expect("first");
    let (s1, e1, p1, _) = entry_holding(map, addr + PAGE_SIZE).expect("middle");
    let (s2, e2, p2, _) = entry_holding(map, addr + 3 * PAGE_SIZE).expect("last");
    assert_eq!((s0, e0, p0), (addr, addr + PAGE_SIZE, RW));
    assert_eq!(
        (s1, e1, p1),
        (addr + PAGE_SIZE, addr + 3 * PAGE_SIZE, PROT_READ)
    );
    assert_eq!((s2, e2, p2), (addr + 3 * PAGE_SIZE, addr + len, RW));
    assert_eq!(map.size.get().as_usize(), len);
    assert_eq!(vm.vm_dused.get(), 4);

    // PROT_NONE drops the pages from vm_dused; back to RW restores them.
    uvm_map_protect(map, addr, addr + len, PROT_NONE, 0, false, true).expect("none");
    assert_eq!(vm.vm_dused.get(), 0);
    uvm_map_protect(map, addr, addr + len, RW, 0, false, true).expect("rw");
    assert_eq!(vm.vm_dused.get(), 4);

    uvmspace_free(vm);
}

#[test]
fn immutable_entries_refuse_changes() {
    let _g = setup();
    let vm = user_vmspace();
    let map = &vm.vm_map;
    let mut addr = 0x5000_0000;
    let len = 2 * PAGE_SIZE;
    uvm_mapanon(map, &mut addr, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("mapanon");
    uvm_map_immutable(map, addr, addr + len, true).expect("immutable");

    assert_eq!(
        uvm_map_protect(map, addr, addr + len, PROT_READ, 0, false, true),
        Err(Errno::EPERM)
    );
    assert_eq!(
        uvm_map_inherit(map, addr, addr + len, MAP_INHERIT_SHARE),
        Err(Errno::EPERM)
    );
    let mut over = addr;
    assert_eq!(
        uvm_mapanon(
            map,
            &mut over,
            len,
            0,
            anon_flags(UVM_FLAG_FIXED | UVM_FLAG_UNMAP)
        ),
        Err(Errno::EPERM)
    );

    // Without the check the entries go away as usual.
    uvm_unmap(map, addr, addr + len);
    assert_eq!(map.size.get().as_usize(), 0);
    uvmspace_free(vm);
}

#[test]
fn inherit_advice_and_fork() {
    let _g = setup();
    let vm = user_vmspace();
    let map = &vm.vm_map;
    let len = 2 * PAGE_SIZE;
    let (mut copy, mut share, mut none, mut overlay) =
        (0x6000_0000, 0x6100_0000, 0x6200_0000, 0x6300_0000);
    uvm_mapanon(map, &mut copy, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("copy");
    uvm_mapanon(map, &mut share, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("share");
    uvm_mapanon(map, &mut none, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("none");
    uvm_mapanon(
        map,
        &mut overlay,
        len,
        0,
        anon_flags(UVM_FLAG_FIXED | UVM_FLAG_OVERLAY),
    )
    .expect("overlay");
    uvm_map_inherit(map, share, share + len, MAP_INHERIT_SHARE).expect("inherit share");
    uvm_map_inherit(map, none, none + len, MAP_INHERIT_NONE).expect("inherit none");
    assert_eq!(
        uvm_map_inherit(map, none, none + len, 7),
        Err(Errno::EINVAL)
    );
    uvm_map_advice(map, copy, copy + PAGE_SIZE, MADV_RANDOM).expect("advice");
    assert_eq!(
        uvm_map_advice(map, copy, copy + len, 99),
        Err(Errno::EINVAL)
    );
    assert_eq!(map.size.get().as_usize(), 4 * len);

    let pr = Process::new();
    pr.ps_vmspace.set(vm);
    let child = uvmspace_fork(&pr);
    let cmap = &child.vm_map;

    // copy, share and overlay are inherited; none is not.
    assert_eq!(cmap.size.get().as_usize(), 3 * len);
    assert_eq!(child.vm_dused.get(), 6);
    let (_, _, _, etype) = entry_holding(cmap, copy).expect("copy inherited");
    assert_ne!(etype & UVM_ET_NEEDSCOPY, 0);
    assert!(entry_holding(cmap, share).is_some(), "share inherited");
    assert!(entry_holding(cmap, none).is_none(), "none not inherited");
    assert!(entry_holding(cmap, overlay).is_some(), "overlay inherited");
    vm_map_lock_read(cmap);
    let child_amap = uvm_map_lookup_entry(cmap, overlay)
        .and_then(|e| e.aref.amap())
        .expect("the overlay's amap is shared with the child");
    vm_map_unlock_read(cmap);
    assert_eq!(amap_refs(child_amap), 2);

    uvmspace_free(child);
    assert_eq!(amap_refs(child_amap), 1, "the child's reference is gone");
    uvmspace_free(vm);
}

#[test]
fn kernel_style_map_reserves_grows_and_allocates() {
    let _g = setup();
    let min = <Machine as VmParam>::VM_MIN_KERNEL_ADDRESS;
    let max = min + (1 << 30);
    UVM_MAXKADDR.store(min, AtomicOrdering::Relaxed);
    pmap_reference(pmap_kernel());
    let map = uvm_map_create(pmap_kernel(), min, max, VM_MAP_PAGEABLE).expect("map");
    assert!(map.uaddr_any[3].get().is_some(), "the bootstrap selector");
    let kflags = uvm_mapflag(RW, RW, MAP_INHERIT_NONE, MADV_RANDOM, UVM_FLAG_FIXED);

    // The bootstrap reservation, as uvm_km_init makes it, grows uvm_maxkaddr.
    let mut base = min;
    let reserved = 16 * PAGE_SIZE;
    uvm_map(
        map,
        &mut base,
        reserved,
        None,
        UVM_UNKNOWN_OFFSET,
        0,
        kflags,
    )
    .expect("reserve");
    assert_eq!(base, min);
    assert!(UVM_MAXKADDR.load(AtomicOrdering::Relaxed) >= min + reserved);

    // A non-fixed allocation lands above it, through uaddr_kbootstrap.
    let mut va = 0;
    let flags = uvm_mapflag(RW, RW, MAP_INHERIT_NONE, MADV_RANDOM, 0);
    uvm_map(
        map,
        &mut va,
        4 * PAGE_SIZE,
        None,
        UVM_UNKNOWN_OFFSET,
        0,
        flags,
    )
    .expect("alloc");
    assert!(va >= min + reserved && va + 4 * PAGE_SIZE <= max, "{va:#x}");
    assert_eq!(map.size.get().as_usize(), reserved + 4 * PAGE_SIZE);

    // W^X is refused on the kernel map only; this map is not kernel_map.
    assert!(!is_kernel_map(map));

    uvm_unmap(map, va, va + 4 * PAGE_SIZE);
    assert_eq!(map.size.get().as_usize(), reserved);
    uvm_map_deallocate(map);
}

#[test]
fn bestfit_switch_on_an_empty_kernel_map() {
    let _g = setup();
    let min = <Machine as VmParam>::VM_MIN_KERNEL_ADDRESS;
    let max = min + (1 << 30);
    UVM_MAXKADDR.store(min, AtomicOrdering::Relaxed);
    pmap_reference(pmap_kernel());
    let map = uvm_map_create(pmap_kernel(), min, max, VM_MAP_PAGEABLE).expect("map");

    // uvm_init's switch, with nothing reserved below uvm_maxkaddr (amd64's case).
    uvm_map_set_uaddr(
        map,
        UvmMapUaddrSlot::Any(3),
        Some(crate::uvm::uvm_addr::uaddr_bestfit_create(min, max)),
    );
    let flags = uvm_mapflag(RW, RW, MAP_INHERIT_NONE, MADV_RANDOM, 0);
    let mut va = 0;
    uvm_map(
        map,
        &mut va,
        2 * PAGE_SIZE,
        None,
        UVM_UNKNOWN_OFFSET,
        0,
        flags,
    )
    .expect("alloc");
    assert!(va >= min && va + 2 * PAGE_SIZE <= max, "{va:#x}");
    let mut vb = 0;
    uvm_map(
        map,
        &mut vb,
        2 * PAGE_SIZE,
        None,
        UVM_UNKNOWN_OFFSET,
        0,
        flags,
    )
    .expect("alloc 2");
    assert!(vb + 2 * PAGE_SIZE <= va || va + 2 * PAGE_SIZE <= vb);
    assert!(UVM_MAXKADDR.load(AtomicOrdering::Relaxed) >= va + 2 * PAGE_SIZE);
    uvm_unmap(map, va, va + 2 * PAGE_SIZE);
    uvm_unmap(map, vb, vb + 2 * PAGE_SIZE);
    assert_eq!(map.size.get().as_usize(), 0);
    uvm_map_deallocate(map);
}

#[test]
fn mquery_skips_used_space() {
    let _g = setup();
    let vm = user_vmspace();
    let map = &vm.vm_map;
    let mut addr = 0x7000_0000;
    let len = 4 * PAGE_SIZE;
    uvm_mapanon(map, &mut addr, len, 0, anon_flags(UVM_FLAG_FIXED)).expect("mapanon");

    let mut q = addr + PAGE_SIZE;
    uvm_map_mquery(map, &mut q, PAGE_SIZE, UVM_UNKNOWN_OFFSET, 0).expect("mquery");
    assert!(q >= addr + len, "{q:#x} is past the mapping");

    let mut fixed = addr + PAGE_SIZE;
    assert_eq!(
        uvm_map_mquery(
            map,
            &mut fixed,
            PAGE_SIZE,
            UVM_UNKNOWN_OFFSET,
            UVM_FLAG_FIXED
        ),
        Err(Errno::EINVAL)
    );
    uvmspace_free(vm);
}

#[test]
fn pie_addresses_stay_in_range() {
    for _ in 0..64 {
        let addr = uvm_map_pie(1 << 21);
        assert_eq!(addr & ((1 << 21) - 1), 0);
        assert!(addr >= VM_PIE_MIN_ADDR && addr < VM_PIE_MAX_ADDR + (1 << 21));
    }
}

#[test]
fn exec_recycles_a_single_user_vmspace() {
    let _g = setup();
    let vm = user_vmspace();
    let map = &vm.vm_map;
    let mut addr = 0x2000_0000;
    uvm_mapanon(map, &mut addr, PAGE_SIZE, 0, anon_flags(UVM_FLAG_FIXED)).expect("mapanon");
    let pr = Process::new();
    pr.ps_vmspace.set(vm);
    let p = Proc::new();
    p.p_p.set(&pr);
    p.p_vmspace.set(vm);

    uvmspace_exec(&p, PAGE_SIZE, 0x1000_0000);
    assert_eq!(map.min_offset.get(), PAGE_SIZE);
    assert_eq!(map.max_offset.get(), 0x1000_0000);
    assert_eq!(map.size.get().as_usize(), 0);
    assert!(entry_holding(map, addr).is_none());
    uvm_tree_sanity(map, "after exec");
    uvmspace_free(vm);
}
