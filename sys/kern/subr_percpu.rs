/*	$OpenBSD: subr_percpu.c,v 1.11 2023/09/16 09:33:27 mpi Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2016 David Gwynne <dlg@openbsd.org>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! Per-CPU memory and counters: `kern/subr_percpu.c`.
//!
//! Upstream: sys/kern/subr_percpu.c @ 3ce1f3f79392
//!
//! With `MULTIPROCESSOR` a per-CPU allocation is an array of `ncpusfound` `struct cpumem`
//! slots from the `percpumem` pool, each naming one CPU's memory (from a pool or `malloc`,
//! rounded up to a cache line). The uniprocessor implementation is a single allocation cast
//! to and from the cpumem handle; it is not scaled up to the size of a cache line because
//! there's no other cache to contend with.
//!
//! ## Deviations
//! - The handle is `sys/percpu.rs`'s [`CpumemPtr`] (see its deviations); `cpumem_put`,
//!   `cpumem_free` and `counters_free` are `unsafe fn`: the caller vouches that nobody uses
//!   the memory any more.
//! - `cpumem_malloc` and friends sleep for memory as the C's `M_WAITOK` does; a `None` from
//!   `malloc` (which the C cannot see) panics, as running out of `kmem_map` does in `malloc`.
//! - `counters_read`'s scratch buffer is a slice; without one it is `malloc`ed (`M_TEMP`) as
//!   in C.

use core::ptr::NonNull;
use core::sync::atomic::Ordering;
#[cfg(feature = "multiprocessor")]
use core::sync::atomic::{AtomicU64, fence};

use crate::kassert;
use crate::kern::init_main::NCPUSFOUND;
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_malloc::mallocarray;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::panic;
#[cfg(feature = "multiprocessor")]
use crate::sys::malloc::M_TEMP;
use crate::sys::malloc::{M_COUNTERS, M_WAITOK, M_ZERO};
use crate::sys::percpu::{CpumemIter, CpumemPtr};
use crate::sys::pool::Pool;
#[cfg(feature = "multiprocessor")]
use crate::sys::pool::{PR_WAITOK, PR_ZERO};
#[cfg(feature = "multiprocessor")]
use crate::{
    kern::subr_pool::{POOL_ALLOCATOR_SINGLE, pool_get, pool_init, pool_put},
    machine::intr::IPL_NONE,
    sys::param::roundup,
    sys::percpu::{CACHELINESIZE, Cpumem},
};
#[cfg(not(feature = "multiprocessor"))]
use crate::{
    kern::subr_pool::{pool_get, pool_put},
    machine::intr::{splhigh, splx},
    sys::pool::{PR_WAITOK, PR_ZERO},
};

/// `cpumem_pl`: the slot arrays, `ncpusfound` `struct cpumem` each.
#[cfg(feature = "multiprocessor")]
static CPUMEM_PL: Pool = Pool::new();

/// `ncpusfound` as an index bound.
pub fn ncpusfound() -> usize {
    NCPUSFOUND.load(Ordering::Relaxed).max(1) as usize
}

/// A `M_WAITOK` allocation; `malloc` returning nothing is the C's "out of space" panic.
fn malloc_waitok(sz: usize, type_: i32) -> NonNull<u8> {
    match malloc(sz, type_, M_WAITOK | M_ZERO) {
        Some(p) => p,
        None => panic(format_args!("cpumem: out of memory ({sz} bytes)")),
    }
}

/// A `PR_WAITOK` pool item.
fn pool_get_waitok(pp: &Pool, flags: i32) -> NonNull<u8> {
    match pool_get(pp, PR_WAITOK | flags) {
        Some(p) => p,
        None => panic(format_args!("cpumem: {}: no item", pp.pr_wchan.get())),
    }
}

/// A fresh slot array from `cpumem_pl`.
#[cfg(feature = "multiprocessor")]
fn cpumem_slots() -> NonNull<Cpumem> {
    let cm = pool_get_waitok(&CPUMEM_PL, 0).cast::<Cpumem>();
    for cpu in 0..ncpusfound() {
        // SAFETY: the item holds `ncpusfound` slots (`percpu_init`'s size); each is written
        // before the handle is made.
        unsafe { cm.as_ptr().add(cpu).write(Cpumem::new()) };
    }
    cm
}

/// `percpu_init`: the pool of slot arrays, once `ncpusfound` is known (`MULTIPROCESSOR`);
/// nothing without it.
pub fn percpu_init() {
    #[cfg(feature = "multiprocessor")]
    pool_init(
        &CPUMEM_PL,
        size_of::<Cpumem>() * ncpusfound(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "percpumem",
        Some(&POOL_ALLOCATOR_SINGLE),
    );
    // nop without MULTIPROCESSOR
}

/// `cpumem_get(pp)`: one zeroed item of `pp` per CPU.
pub fn cpumem_get(pp: &Pool) -> CpumemPtr {
    let sz = pp.pr_size.get() as usize;
    #[cfg(feature = "multiprocessor")]
    {
        let cm = cpumem_slots();
        for cpu in 0..ncpusfound() {
            let mem = pool_get_waitok(pp, PR_ZERO);
            // SAFETY: `cm` holds `ncpusfound` slots.
            unsafe { &*cm.as_ptr().add(cpu) }
                .mem
                .store(mem.as_ptr(), Ordering::Relaxed);
        }
        // SAFETY: every slot names `sz` bytes of `pp`.
        unsafe { CpumemPtr::from_raw(cm, sz) }
    }
    #[cfg(not(feature = "multiprocessor"))]
    {
        let mem = pool_get_waitok(pp, PR_ZERO);
        // SAFETY: an item of `sz` bytes.
        unsafe { CpumemPtr::from_raw(mem.cast(), sz) }
    }
}

/// `cpumem_put(pp, cm)`: every CPU's item back to `pp`.
///
/// # Safety
///
/// `cm` came from `cpumem_get(pp)` and no CPU uses it any more.
pub unsafe fn cpumem_put(pp: &Pool, cm: CpumemPtr) {
    #[cfg(feature = "multiprocessor")]
    {
        for slot in cm.slots() {
            if let Some(mem) = NonNull::new(slot.mem.load(Ordering::Relaxed)) {
                pool_put(pp, mem);
            }
        }
        pool_put(&CPUMEM_PL, cm.as_ptr().cast());
    }
    #[cfg(not(feature = "multiprocessor"))]
    pool_put(pp, cm.as_ptr().cast());
}

/// `cpumem_malloc(sz, type)`: `sz` zeroed bytes per CPU.
pub fn cpumem_malloc(sz: usize, type_: i32) -> CpumemPtr {
    #[cfg(feature = "multiprocessor")]
    {
        let sz = roundup(sz, CACHELINESIZE);

        let cm = cpumem_slots();
        for cpu in 0..ncpusfound() {
            let mem = malloc_waitok(sz, type_);
            // SAFETY: `cm` holds `ncpusfound` slots.
            unsafe { &*cm.as_ptr().add(cpu) }
                .mem
                .store(mem.as_ptr(), Ordering::Relaxed);
        }
        // SAFETY: every slot names `sz` bytes.
        unsafe { CpumemPtr::from_raw(cm, sz) }
    }
    #[cfg(not(feature = "multiprocessor"))]
    {
        let mem = malloc_waitok(sz, type_);
        // SAFETY: `sz` bytes.
        unsafe { CpumemPtr::from_raw(mem.cast(), sz) }
    }
}

/// `cpumem_malloc_ncpus(bootcm, sz, type)`: the boot CPU keeps its boot memory, the others
/// get `sz` zeroed bytes each; without `MULTIPROCESSOR` the boot memory is all there is.
pub fn cpumem_malloc_ncpus(bootcm: CpumemPtr, sz: usize, type_: i32) -> CpumemPtr {
    #[cfg(feature = "multiprocessor")]
    {
        let sz = roundup(sz, CACHELINESIZE);

        let cm = cpumem_slots();
        // SAFETY: `cm` holds `ncpusfound` (at least one) slots.
        unsafe { &*cm.as_ptr() }
            .mem
            .store(bootcm.cpu_mem(0).as_ptr(), Ordering::Relaxed);
        for cpu in 1..ncpusfound() {
            let mem = malloc_waitok(sz, type_);
            // SAFETY: as above.
            unsafe { &*cm.as_ptr().add(cpu) }
                .mem
                .store(mem.as_ptr(), Ordering::Relaxed);
        }
        // SAFETY: every slot names at least `sz` bytes (the boot memory is the size the caller
        // asks for, before the rounding).
        unsafe { CpumemPtr::from_raw(cm, sz.min(bootcm.size())) }
    }
    #[cfg(not(feature = "multiprocessor"))]
    {
        let _ = (sz, type_);
        bootcm
    }
}

/// `cpumem_free(cm, type, sz)`.
///
/// # Safety
///
/// `cm` came from `cpumem_malloc(sz, type)` and no CPU uses it any more.
pub unsafe fn cpumem_free(cm: CpumemPtr, type_: i32, sz: usize) {
    #[cfg(feature = "multiprocessor")]
    {
        let sz = roundup(sz, CACHELINESIZE);

        for slot in cm.slots() {
            if let Some(mem) = NonNull::new(slot.mem.load(Ordering::Relaxed)) {
                free(mem, type_, sz);
            }
        }
        pool_put(&CPUMEM_PL, cm.as_ptr().cast());
    }
    #[cfg(not(feature = "multiprocessor"))]
    free(cm.as_ptr().cast(), type_, sz);
}

/// `cpumem_first(i, cm)`: CPU 0's memory.
pub fn cpumem_first(i: &mut CpumemIter, cm: CpumemPtr) -> NonNull<u8> {
    i.cpu = 0;

    cm.cpu_mem(0)
}

/// `cpumem_next(i, cm)`: the next CPU's memory, `None` after the last.
pub fn cpumem_next(i: &mut CpumemIter, cm: CpumemPtr) -> Option<NonNull<u8>> {
    #[cfg(feature = "multiprocessor")]
    {
        i.cpu += 1;
        let cpu = i.cpu as usize;

        if cpu >= ncpusfound() {
            return None;
        }

        Some(cm.cpu_mem(cpu))
    }
    #[cfg(not(feature = "multiprocessor"))]
    {
        let _ = (i, cm);
        None
    }
}

/// The generation number in front of the counters (`MULTIPROCESSOR`).
#[cfg(feature = "multiprocessor")]
const COUNTERS_GEN: usize = 1;
/// No generation number without `MULTIPROCESSOR`.
#[cfg(not(feature = "multiprocessor"))]
const COUNTERS_GEN: usize = 0;

/// `counters_alloc(n)`: `n` zeroed counters per CPU.
pub fn counters_alloc(n: usize) -> CpumemPtr {
    kassert!(n > 0);

    let n = n + COUNTERS_GEN; // add space for a generation number
    let cm = cpumem_malloc(n * size_of::<u64>(), M_COUNTERS);

    for cpu in 0..ncpusfound_of(cm) {
        for counter in &cm.cpu_words(cpu)[..n] {
            counter.store(0, Ordering::Relaxed);
        }
    }

    cm
}

/// How many CPUs a handle has memory for: `ncpusfound`, or one without `MULTIPROCESSOR`.
fn ncpusfound_of(_cm: CpumemPtr) -> usize {
    #[cfg(feature = "multiprocessor")]
    {
        ncpusfound()
    }
    #[cfg(not(feature = "multiprocessor"))]
    {
        1
    }
}

/// `counters_alloc_ncpus(cm, n)`: the boot counters `cm` (`COUNTERS_BOOT_MEMORY`) become CPU
/// 0's, the other CPUs get their own.
pub fn counters_alloc_ncpus(cm: CpumemPtr, n: usize) -> CpumemPtr {
    // the generation number; without MULTIPROCESSOR this is unnecessary, but symmetrical
    let n = n + COUNTERS_GEN;
    cpumem_malloc_ncpus(cm, n * size_of::<u64>(), M_COUNTERS)
}

/// `counters_free(cm, n)`.
///
/// # Safety
///
/// `cm` came from `counters_alloc(n)` and no CPU uses it any more.
pub unsafe fn counters_free(cm: CpumemPtr, n: usize) {
    let n = n + COUNTERS_GEN; // generation number
    // SAFETY: the caller's guarantee; `counters_alloc` asked `cpumem_malloc` for this size.
    unsafe { cpumem_free(cm, M_COUNTERS, n * size_of::<u64>()) };
}

/// `counters_read(cm, output, n, scratch)`: the sum over the CPUs of each of the `n` counters.
/// With `MULTIPROCESSOR` a CPU's counters are copied while its generation number is even and
/// unchanged across the copy.
pub fn counters_read(cm: CpumemPtr, output: &mut [u64], n: usize, scratch: Option<&mut [u64]>) {
    let output = &mut output[..n];
    output.fill(0);

    #[cfg(feature = "multiprocessor")]
    {
        let mut own: Option<NonNull<u8>> = None;
        let temp: &mut [u64] = match scratch {
            Some(s) => &mut s[..n],
            None => {
                let Some(t) = mallocarray(n, size_of::<u64>(), M_TEMP, M_WAITOK) else {
                    panic(format_args!("counters_read: out of memory"));
                };
                own = Some(t);
                // SAFETY: `n` u64s just allocated, aligned (malloc's blocks are), ours until
                // the free below.
                unsafe { core::slice::from_raw_parts_mut(t.as_ptr().cast::<u64>(), n) }
            }
        };

        let mut i = CpumemIter::default();
        let mut gen_mem = Some(cpumem_first(&mut i, cm));
        while let Some(mem) = gen_mem {
            let words = words_at(mem, cm);
            let r#gen = &words[0];
            let counters = &words[1..=n];

            let mut enter = r#gen.load(Ordering::Acquire);
            loop {
                // the generation number is odd during an update
                while enter & 1 != 0 {
                    crate::kern::sched_bsd::r#yield();
                    enter = r#gen.load(Ordering::Acquire);
                }

                fence(Ordering::Acquire); // membar_consumer()
                for (t, c) in temp.iter_mut().zip(counters) {
                    *t = c.load(Ordering::Relaxed);
                }

                fence(Ordering::Acquire); // membar_consumer()
                let leave = r#gen.load(Ordering::Relaxed);

                if enter == leave {
                    break;
                }

                enter = leave;
            }

            for (o, t) in output.iter_mut().zip(temp.iter()) {
                *o += *t;
            }

            gen_mem = cpumem_next(&mut i, cm);
        }

        if let Some(t) = own {
            free(t, M_TEMP, n * size_of::<u64>());
        }
    }
    #[cfg(not(feature = "multiprocessor"))]
    {
        let _ = scratch;
        let counters = &cm.cpu_words(0)[..n];

        let s = splhigh();
        for (o, c) in output.iter_mut().zip(counters) {
            *o = c.load(Ordering::Relaxed);
        }
        splx(s);
    }
}

/// The counter words of one CPU's memory `mem` of `cm`.
#[cfg(feature = "multiprocessor")]
fn words_at(mem: NonNull<u8>, cm: CpumemPtr) -> &'static [AtomicU64] {
    // SAFETY: `mem` is one CPU's memory of `cm`, `cm.size()` bytes, 8-byte aligned, alive
    // while `cm` is used.
    unsafe {
        core::slice::from_raw_parts(
            mem.as_ptr().cast::<AtomicU64>(),
            cm.size() / size_of::<AtomicU64>(),
        )
    }
}

/// `counters_zero(cm, n)`: every CPU's counters, and the generation numbers, back to 0.
pub fn counters_zero(cm: CpumemPtr, n: usize) {
    #[cfg(feature = "multiprocessor")]
    {
        let mut i = CpumemIter::default();
        let mut mem = Some(cpumem_first(&mut i, cm));
        fence(Ordering::Release); // membar_producer()
        while let Some(m) = mem {
            let words = words_at(m, cm);
            for c in &words[1..=n] {
                c.store(0, Ordering::Relaxed);
            }
            // zero the generation numbers too
            fence(Ordering::Release); // membar_producer()
            words[0].store(0, Ordering::Relaxed);

            mem = cpumem_next(&mut i, cm);
        }
    }
    #[cfg(not(feature = "multiprocessor"))]
    {
        let counters = &cm.cpu_words(0)[..n];

        let s = splhigh();
        for c in counters {
            c.store(0, Ordering::Relaxed);
        }
        splx(s);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
#[cfg_attr(feature = "multiprocessor", allow(unused_imports))] // the allocating tests are UP-only
mod tests {
    // Host tests for per-CPU memory and counters. The host double is one CPU (`cpu_number()` is
    // 0, `ncpusfound` 1), so with or without `MULTIPROCESSOR` every handle has one CPU's memory.

    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::sys::percpu::{
        CpumemBootMemory, counters_add, counters_boot_words, counters_dec, counters_inc,
        counters_pkt, cpumem_enter, cpumem_foreach,
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
}
/* </TESTS> */
