//! Boot-time self-tests under feature `qemu`: a project helper, not OpenBSD code.
//!
//! Each test exercises a subsystem right after `main()` brought it up and prints one line that
//! `xtask smoke` asserts (`selftest: <what> ok`). They stand in for the user-space programs
//! OpenBSD would run until there is a user space; they are compiled only with feature `qemu`
//! (and need feature `alloc` for the allocator stress).

use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_pool::{pool_destroy, pool_get, pool_init, pool_put, pool_reclaim};
use crate::kprintf;
use crate::machine::intr::IPL_NONE;
use crate::machine::pmap::{
    pmap_extract, pmap_growkernel, pmap_kenter_pa, pmap_kernel, pmap_kremove, pmap_map_direct,
    pmap_update,
};
use crate::sys::malloc::{M_NOWAIT, M_TEMP, M_ZERO};
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::param::PAGE_SIZE;
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::uvm::uvm_extern::UVM_PGA_ZERO;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_km::kernel_map_min;
use crate::uvm::uvm_page::{uvm_pagealloc, uvm_pagefree, vm_page_to_phys};

/// A value that is neither all zeros nor all ones.
const PATTERN: u64 = 0x5a5a_c3c3_0f0f_a5a5;

/// Maps a fresh page at the start of the kernel map, writes through the mapping, reads back
/// through the direct map, checks `pmap_extract` before and after `pmap_kremove`.
pub fn pmap_kernel_mapping() {
    let va = kernel_map_min();
    let want = Vaddr::new(va.as_usize() + 2 * PAGE_SIZE);
    let reached = pmap_growkernel(want);
    if reached < want {
        kprintf!(
            "selftest: pmap kernel mapping FAILED: pmap_growkernel reached {:#x}, wanted {:#x}\n",
            reached.as_usize(),
            want.as_usize()
        );
        return;
    }

    let Some(pg) = uvm_pagealloc(None, 0, None, UVM_PGA_ZERO) else {
        kprintf!("selftest: pmap kernel mapping FAILED: no page\n");
        return;
    };
    let pa = vm_page_to_phys(pg);

    // SAFETY: `va` is the first page of the kernel map, which nothing has allocated yet, and
    // `pa` is the page just taken from the free list.
    unsafe { pmap_kenter_pa(va, pa, PROT_READ | PROT_WRITE) };
    pmap_update(pmap_kernel());

    // SAFETY: `va` is mapped read-write to `pa`, a page this test owns.
    unsafe { ptr::write_volatile(va.as_usize() as *mut u64, PATTERN) };
    // SAFETY: the direct map covers every page the allocator hands out.
    let seen = unsafe { ptr::read_volatile(pmap_map_direct(pg).as_usize() as *const u64) };
    let probe = Vaddr::new(va.as_usize() + 0x10);
    let extracted = pmap_extract(pmap_kernel(), probe);

    // SAFETY: the range was entered just above and is not used afterwards.
    unsafe { pmap_kremove(va, Vsize::new(PAGE_SIZE)) };
    pmap_update(pmap_kernel());
    let gone = pmap_extract(pmap_kernel(), va).is_none();
    uvm_pagefree(pg);

    let ok = seen == PATTERN && extracted == Some(Paddr::new(pa.as_usize() + 0x10)) && gone;
    if ok {
        kprintf!("selftest: pmap kernel mapping ok\n");
    } else {
        kprintf!(
            "selftest: pmap kernel mapping FAILED: read {:#x}, extract {:?}, unmapped {}\n",
            seen,
            extracted.map(Paddr::as_usize),
            gone
        );
    }
}

/// The pool the stress test allocates from.
static STRESS_POOL: Pool = Pool::new();

/// Fills a block with a byte pattern derived from its index and checks it back.
fn fill_and_check(p: NonNull<u8>, len: usize, seed: u8) -> bool {
    for i in 0..len {
        // SAFETY: `len` bytes at `p` are this test's.
        unsafe { ptr::write_volatile(p.as_ptr().add(i), seed.wrapping_add(i as u8)) };
    }
    (0..len).all(|i| {
        // SAFETY: as above.
        (unsafe { ptr::read_volatile(p.as_ptr().add(i)) }) == seed.wrapping_add(i as u8)
    })
}

/// Exercises `malloc(9)` directly, the Rust allocator through `Vec` and `Box`, and a pool:
/// allocate, write, verify, free, several rounds, and reports the free page count before and
/// after.
pub fn malloc_pool_stress() {
    let free_before = UVMEXP.free.load(Ordering::Relaxed);
    let mut ok = true;
    let mut mallocs = 0u32;

    // malloc(9): every bucket size up to and past MAXALLOCSAVE, with and without M_ZERO.
    let sizes: [usize; 10] = [16, 24, 100, 256, 1000, 4096, 8192, 12288, 65536, 200_000];
    for round in 0..4u8 {
        let mut blocks: [Option<NonNull<u8>>; 10] = [None; 10];
        for (i, &sz) in sizes.iter().enumerate() {
            let flags = if i % 2 == 0 {
                M_NOWAIT
            } else {
                M_NOWAIT | M_ZERO
            };
            let Some(p) = malloc(sz, M_TEMP, flags) else {
                kprintf!("selftest: malloc({}) failed in round {}\n", sz, round);
                ok = false;
                continue;
            };
            if flags & M_ZERO != 0 {
                // SAFETY: `sz` bytes at `p` are this test's.
                ok &= (0..sz).all(|k| (unsafe { ptr::read_volatile(p.as_ptr().add(k)) }) == 0);
            }
            ok &= fill_and_check(p, sz, round.wrapping_mul(7).wrapping_add(i as u8));
            blocks[i] = Some(p);
            mallocs += 1;
        }
        for (i, &sz) in sizes.iter().enumerate() {
            if let Some(p) = blocks[i] {
                ok &= fill_and_check(p, sz, round.wrapping_add(i as u8));
                free(p, M_TEMP, sz);
            }
        }
    }

    // The Rust allocator: a growing Vec and a batch of Boxes.
    let mut v: Vec<u64> = Vec::new();
    for i in 0..4096u64 {
        v.push(i.wrapping_mul(0x9e37_79b9));
    }
    ok &= v
        .iter()
        .enumerate()
        .all(|(i, &x)| x == (i as u64).wrapping_mul(0x9e37_79b9));
    let boxes: Vec<Box<[u8; 100]>> = (0..64u8).map(|i| Box::new([i; 100])).collect();
    ok &= boxes
        .iter()
        .enumerate()
        .all(|(i, b)| b.iter().all(|&x| x == i as u8));
    drop(boxes);
    drop(v);

    // A pool of 48-byte items: take 300 (more than a page's worth), check, return, repeat.
    pool_init(&STRESS_POOL, 48, 0, IPL_NONE, 0, "stress", None);
    let mut pool_ok = true;
    for round in 0..3u8 {
        let mut items: Vec<NonNull<u8>> = Vec::with_capacity(300);
        for i in 0..300u32 {
            let flags = if round == 2 {
                PR_NOWAIT | PR_ZERO
            } else {
                PR_NOWAIT
            };
            let Some(p) = pool_get(&STRESS_POOL, flags) else {
                pool_ok = false;
                break;
            };
            if flags & PR_ZERO != 0 {
                // SAFETY: a 48-byte item of the pool, this test's.
                pool_ok &= (0..48).all(|k| (unsafe { ptr::read_volatile(p.as_ptr().add(k)) }) == 0);
            }
            pool_ok &= fill_and_check(p, 48, i as u8);
            items.push(p);
        }
        pool_ok &= items.len() == 300;
        let mut seen = items.clone();
        seen.sort_unstable();
        pool_ok &= seen
            .windows(2)
            .all(|w| w[1].as_ptr() as usize - w[0].as_ptr() as usize >= 48);
        for (i, p) in items.iter().enumerate() {
            pool_ok &= fill_and_check(*p, 48, (i as u8).wrapping_add(round));
        }
        for p in items {
            pool_put(&STRESS_POOL, p);
        }
    }
    let reclaimed = pool_reclaim(&STRESS_POOL);
    pool_ok &= STRESS_POOL.pr_nout.get() == 0;
    pool_destroy(&STRESS_POOL);
    ok &= pool_ok;

    let free_after = UVMEXP.free.load(Ordering::Relaxed);
    if ok {
        kprintf!(
            "selftest: malloc/pool stress ok ({} mallocs, {} pages free before, {} after, pool pages reclaimed: {})\n",
            mallocs,
            free_before,
            free_after,
            reclaimed
        );
    } else {
        kprintf!(
            "selftest: malloc/pool stress FAILED (pool {}, {} pages free before, {} after)\n",
            pool_ok,
            free_before,
            free_after
        );
    }
}
