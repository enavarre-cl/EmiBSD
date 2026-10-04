//! Host tests for per-CPU memory and counters. The host double is one CPU (`cpu_number()` is
//! 0, `ncpusfound` 1), so with or without `MULTIPROCESSOR` every handle has one CPU's memory.

#![cfg_attr(feature = "multiprocessor", allow(unused_imports))] // the allocating tests are UP-only

use std::{assert, assert_eq};

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::percpu::{
    CpumemBootMemory, counters_add, counters_boot_words, counters_dec, counters_inc, counters_pkt,
    cpumem_enter, cpumem_foreach,
};

/// Four boot counters, as `COUNTERS_BOOT_MEMORY(name, 4)`.
static BOOT: CpumemBootMemory<{ counters_boot_words(4) }> = CpumemBootMemory::new();

#[test]
fn boot_counters_count_read_and_zero() {
    let cm = BOOT.initializer();
    counters_inc(cm, 0);
    counters_inc(cm, 0);
    counters_add(cm, 1, 40);
    counters_pkt(cm, 2, 3, 1500);
    counters_dec(cm, 1);

    let mut out = [0u64; 4];
    let mut scratch = [0u64; 4];
    counters_read(cm, &mut out, 4, Some(&mut scratch));
    assert_eq!(out, [2, 39, 1, 1500]);

    counters_zero(cm, 4);
    counters_read(cm, &mut out, 4, Some(&mut scratch));
    assert_eq!(out, [0; 4]);
    assert_eq!(cpumem_foreach(cm).count(), 1);
}

// percpu_init cannot run twice on the static pool of slot arrays, and every test reloads
// the memory: these run on the uniprocessor build (`just test`).
#[cfg(not(feature = "multiprocessor"))]
#[test]
fn counters_alloc_starts_at_zero_and_frees() {
    let _guard = setup_real_memory();
    percpu_init_for_test();
    let cm = counters_alloc(3);
    let mut out = [7u64; 3];
    counters_read(cm, &mut out, 3, None);
    assert_eq!(out, [0; 3]);
    counters_add(cm, 2, 5);
    counters_read(cm, &mut out, 3, None);
    assert_eq!(out, [0, 0, 5]);
    // SAFETY: nobody else has `cm`.
    unsafe { counters_free(cm, 3) };
}

// percpu_init cannot run twice on the static pool of slot arrays, and every test reloads
// the memory: these run on the uniprocessor build (`just test`).
#[cfg(not(feature = "multiprocessor"))]
#[test]
fn cpumem_get_gives_zeroed_items_and_put_returns_them() {
    let _guard = setup_real_memory();
    percpu_init_for_test();
    let pp: &'static Pool = std::boxed::Box::leak(std::boxed::Box::new(Pool::new()));
    crate::kern::subr_pool::pool_init(pp, 64, 0, 0, PR_WAITOK, "pcputest", None);
    let cm = cpumem_get(pp);
    let mem = cpumem_enter(cm);
    // SAFETY: a 64-byte item, ours.
    assert!((0..64).all(|i| unsafe { mem.as_ptr().add(i).read() } == 0));
    assert_eq!(cm.size(), 64);
    assert_eq!(pp.pr_nout.get(), 1);
    // SAFETY: nobody else has `cm`.
    unsafe { cpumem_put(pp, cm) };
    assert_eq!(pp.pr_nout.get(), 0);
}

// percpu_init cannot run twice on the static pool of slot arrays, and every test reloads
// the memory: these run on the uniprocessor build (`just test`).
#[cfg(not(feature = "multiprocessor"))]
#[test]
fn cpumem_malloc_ncpus_keeps_the_boot_memory() {
    static BOOT2: CpumemBootMemory<{ counters_boot_words(2) }> = CpumemBootMemory::new();
    let _guard = setup_real_memory();
    percpu_init_for_test();
    let boot = BOOT2.initializer();
    counters_add(boot, 1, 9);
    let cm = counters_alloc_ncpus(boot, 2);
    assert_eq!(cpumem_enter(cm), cpumem_enter(boot));
    let mut out = [0u64; 2];
    counters_read(cm, &mut out, 2, None);
    assert_eq!(out, [0, 9]);
}

/// `percpu_init` on the memory `setup_real_memory` just made (a no-op without
/// `MULTIPROCESSOR`, which is how the host tests are built).
#[cfg(not(feature = "multiprocessor"))]
fn percpu_init_for_test() {
    percpu_init();
}
