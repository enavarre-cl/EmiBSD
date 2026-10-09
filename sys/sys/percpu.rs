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
//! `<sys/percpu.h>`: per-CPU memory (`struct cpumem`) and per-CPU counters.
//!
//! Upstream: sys/sys/percpu.h @ 3ce1f3f79392
//!
//! With `MULTIPROCESSOR` a `struct cpumem *` is an array of `ncpusfound` slots, each pointing
//! at one CPU's memory, and `cpumem_enter` picks the slot of `cpu_number()`. Without it the
//! pointer is the memory itself, cast. Counters are `n` `uint64_t`s per CPU; with
//! `MULTIPROCESSOR` a generation number in front of them is odd while the owning CPU updates
//! them, so `counters_read` on another CPU can take a consistent copy. The functions live in
//! `kern/subr_percpu.rs`.
//!
//! ## Deviations
//! - `struct cpumem *` is [`CpumemPtr`], a copyable handle that also carries the size of one
//!   CPU's memory, so the per-CPU memory is reached through bounds-checked slices (the CPU
//!   index and the counter index are checked) instead of unchecked pointer arithmetic.
//! - The counters are `AtomicU64`s. The owning CPU updates them with relaxed `fetch_add`
//!   (`counters[c]++` in C is a plain increment an interrupt on the same CPU could tear);
//!   `membar_producer`/`membar_consumer` are release/acquire fences.
//! - `CPUMEM_BOOT_MEMORY`/`CPUMEM_BOOT_INITIALIZER` are [`CpumemBootMemory`] and its
//!   [`CpumemBootMemory::initializer`]; the MULTIPROCESSOR slot that points back at the
//!   memory is filled by `initializer` (a Rust `static` cannot hold its own address).
//!   `COUNTERS_BOOT_MEMORY(name, n)` is `CpumemBootMemory<{ counters_boot_words(n) }>`.
//! - `__upunused` is not needed: Rust does not warn about an unused field of a used type.

use core::cell::UnsafeCell;
use core::ptr::NonNull;
#[cfg(feature = "multiprocessor")]
use core::sync::atomic::fence;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use crate::kassert;
use crate::kern::subr_percpu::{cpumem_first, cpumem_next};

/// `CACHELINESIZE`: neither amd64 nor arm64 `<machine/param.h>` defines it, so the header's
/// default.
pub const CACHELINESIZE: usize = 64;

/// `struct cpumem`: one CPU's slot of a `MULTIPROCESSOR` per-CPU allocation.
pub struct Cpumem {
    /// `mem`: this CPU's memory. Written when the allocation is made, read by every CPU.
    pub mem: AtomicPtr<u8>,
}

impl Cpumem {
    /// A slot pointing nowhere.
    pub const fn new() -> Self {
        Self {
            mem: AtomicPtr::new(core::ptr::null_mut()),
        }
    }
}

impl Default for Cpumem {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct cpumem *`: a per-CPU allocation (see the module's deviations).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpumemPtr {
    /// With `MULTIPROCESSOR` the slot array; without it the memory itself.
    cm: NonNull<Cpumem>,
    /// The size of one CPU's memory, in bytes.
    sz: usize,
}

// SAFETY: the handle is an address and a size; what it points at is per-CPU memory each CPU
// reaches only through its own slot (or under the protocol of the memory's user).
unsafe impl Send for CpumemPtr {}
// SAFETY: as above.
unsafe impl Sync for CpumemPtr {}

impl CpumemPtr {
    /// A handle on `cm` (the slot array, or the memory) whose per-CPU memory is `sz` bytes.
    ///
    /// # Safety
    ///
    /// With `MULTIPROCESSOR`, `cm` must point at `ncpusfound` initialised slots, each naming
    /// `sz` bytes; without it, at `sz` bytes. They must stay allocated while the handle is used.
    pub const unsafe fn from_raw(cm: NonNull<Cpumem>, sz: usize) -> Self {
        Self { cm, sz }
    }

    /// The pointer the C passes around.
    pub const fn as_ptr(self) -> NonNull<Cpumem> {
        self.cm
    }

    /// The size of one CPU's memory.
    pub const fn size(self) -> usize {
        self.sz
    }

    /// The slot array (`MULTIPROCESSOR`).
    #[cfg(feature = "multiprocessor")]
    pub(crate) fn slots(self) -> &'static [Cpumem] {
        // SAFETY: `from_raw`'s contract: `ncpusfound` initialised slots that outlive the
        // handle's use.
        unsafe {
            core::slice::from_raw_parts(self.cm.as_ptr(), crate::kern::subr_percpu::ncpusfound())
        }
    }

    /// The memory of CPU `cpu`.
    pub fn cpu_mem(self, cpu: usize) -> NonNull<u8> {
        #[cfg(feature = "multiprocessor")]
        {
            let mem = self.slots()[cpu].mem.load(Ordering::Relaxed);
            match NonNull::new(mem) {
                Some(mem) => mem,
                None => {
                    crate::kern::subr_prf::panic(format_args!("cpumem: cpu{cpu} has no memory"))
                }
            }
        }
        #[cfg(not(feature = "multiprocessor"))]
        {
            kassert!(cpu == 0);
            self.cm.cast()
        }
    }

    /// The memory of CPU `cpu` as `uint64_t`s (counters).
    pub fn cpu_words(self, cpu: usize) -> &'static [AtomicU64] {
        let mem = self.cpu_mem(cpu);
        kassert!((mem.as_ptr() as usize).is_multiple_of(align_of::<AtomicU64>()));
        // SAFETY: `sz` bytes of per-CPU memory, 8-byte aligned (malloc and pool items are),
        // alive while the handle is used; atomics make the shared accesses sound.
        unsafe {
            core::slice::from_raw_parts(
                mem.as_ptr().cast::<AtomicU64>(),
                self.sz / size_of::<AtomicU64>(),
            )
        }
    }
}

/// `struct cpumem_iter`: where `cpumem_first`/`cpumem_next` are.
#[derive(Clone, Copy, Debug, Default)]
pub struct CpumemIter {
    /// The CPU last returned.
    pub cpu: u32,
}

/// `struct counters_ref`: what `counters_enter` hands `counters_leave`.
#[derive(Clone, Copy, Debug, Default)]
pub struct CountersRef {
    /// `g`: the generation number at entry (`MULTIPROCESSOR`).
    pub g: u64,
    /// `c`: this CPU's counter memory (the generation number first with `MULTIPROCESSOR`).
    pub c: Option<&'static [AtomicU64]>,
}

/// `cpumem_enter(cm)`: this CPU's memory.
pub fn cpumem_enter(cm: CpumemPtr) -> NonNull<u8> {
    #[cfg(feature = "multiprocessor")]
    {
        cm.cpu_mem(crate::machine::cpu::cpu_number() as usize)
    }
    #[cfg(not(feature = "multiprocessor"))]
    {
        cm.cpu_mem(0)
    }
}

/// `cpumem_leave(cm, mem)`.
pub fn cpumem_leave(_cm: CpumemPtr, _mem: NonNull<u8>) {
    // KDASSERT?
}

/// `CPUMEM_BOOT_MEMORY(_name, _sz)`: per-CPU memory for the boot CPU before `percpu_init`,
/// `W` 64-bit words, cache-line aligned. `cpumem_malloc_ncpus` keeps it as CPU 0's memory.
#[repr(C, align(64))]
pub struct CpumemBootMemory<const W: usize> {
    /// `mem`.
    mem: UnsafeCell<[u64; W]>,
    /// `cpumem`: the slot pointing at `mem` (`MULTIPROCESSOR`).
    cpumem: Cpumem,
}

// SAFETY: `mem` is reached only through the handle `initializer` gives, as the boot CPU's
// per-CPU memory (atomics for counters); the slot is atomic.
unsafe impl<const W: usize> Sync for CpumemBootMemory<W> {}

impl<const W: usize> CpumemBootMemory<W> {
    /// The zeroed boot memory.
    pub const fn new() -> Self {
        Self {
            mem: UnsafeCell::new([0; W]),
            cpumem: Cpumem::new(),
        }
    }

    /// `CPUMEM_BOOT_INITIALIZER(_name)`: the handle on the boot memory. Without
    /// `MULTIPROCESSOR` it is the memory itself; with it, a one-slot array, the only one
    /// `cpumem_enter` may use before `cpumem_malloc_ncpus` replaces the handle.
    pub fn initializer(&'static self) -> CpumemPtr {
        let mem = self.mem.get().cast::<u8>();
        #[cfg(feature = "multiprocessor")]
        {
            self.cpumem.mem.store(mem, Ordering::Relaxed);
            CpumemPtr {
                cm: NonNull::from(&self.cpumem),
                sz: W * size_of::<u64>(),
            }
        }
        #[cfg(not(feature = "multiprocessor"))]
        {
            let _ = &self.cpumem;
            CpumemPtr {
                // SAFETY: the address of a static is not null.
                cm: unsafe { NonNull::new_unchecked(mem.cast::<Cpumem>()) },
                sz: W * size_of::<u64>(),
            }
        }
    }
}

impl<const W: usize> Default for CpumemBootMemory<W> {
    fn default() -> Self {
        Self::new()
    }
}

/// `CPUMEM_FOREACH(_var, _iter, _cpumem)`: every CPU's memory, CPU 0 first.
pub fn cpumem_foreach(cm: CpumemPtr) -> impl Iterator<Item = NonNull<u8>> {
    let mut i = CpumemIter::default();
    let mut next = Some(cpumem_first(&mut i, cm));
    core::iter::from_fn(move || {
        let cur = next?;
        next = cpumem_next(&mut i, cm);
        Some(cur)
    })
}

// per cpu counters

/// The generation number's slot in front of the counters (`MULTIPROCESSOR`).
#[cfg(feature = "multiprocessor")]
const COUNTERS_GEN: usize = 1;
/// No generation number without `MULTIPROCESSOR`.
#[cfg(not(feature = "multiprocessor"))]
const COUNTERS_GEN: usize = 0;

/// `COUNTERS_BOOT_MEMORY(_name, _n)`'s size in 64-bit words: `n` counters, plus the
/// generation number with `MULTIPROCESSOR`.
pub const fn counters_boot_words(n: usize) -> usize {
    n + COUNTERS_GEN
}

/// `counters_enter(ref, cm)`: this CPU's counters, the generation number made odd.
pub fn counters_enter(r: &mut CountersRef, cm: CpumemPtr) -> &'static [AtomicU64] {
    #[cfg(feature = "multiprocessor")]
    let words = cm.cpu_words(crate::machine::cpu::cpu_number() as usize);
    #[cfg(not(feature = "multiprocessor"))]
    let words = cm.cpu_words(0);
    r.c = Some(words);
    #[cfg(feature = "multiprocessor")]
    {
        // make the generation number odd
        r.g = words[0].load(Ordering::Relaxed) + 1;
        words[0].store(r.g, Ordering::Relaxed);
        fence(Ordering::Release); // membar_producer()
    }
    &words[COUNTERS_GEN..]
}

/// `counters_leave(ref, cm)`: the generation number even again.
pub fn counters_leave(r: &mut CountersRef, cm: CpumemPtr) {
    #[cfg(feature = "multiprocessor")]
    if let Some(words) = r.c {
        fence(Ordering::Release); // membar_producer()
        // make the generation number even again
        r.g += 1;
        words[0].store(r.g, Ordering::Release);
    }
    if let Some(words) = r.c
        && let Some(mem) = NonNull::new(words.as_ptr().cast_mut().cast::<u8>())
    {
        cpumem_leave(cm, mem);
    }
}

/// `counters_inc(cm, c)`.
pub fn counters_inc(cm: CpumemPtr, c: usize) {
    let mut r = CountersRef::default();
    let counters = counters_enter(&mut r, cm);
    counters[c].fetch_add(1, Ordering::Relaxed);
    counters_leave(&mut r, cm);
}

/// `counters_dec(cm, c)`.
pub fn counters_dec(cm: CpumemPtr, c: usize) {
    let mut r = CountersRef::default();
    let counters = counters_enter(&mut r, cm);
    counters[c].fetch_sub(1, Ordering::Relaxed);
    counters_leave(&mut r, cm);
}

/// `counters_add(cm, c, v)`.
pub fn counters_add(cm: CpumemPtr, c: usize, v: u64) {
    let mut r = CountersRef::default();
    let counters = counters_enter(&mut r, cm);
    counters[c].fetch_add(v, Ordering::Relaxed);
    counters_leave(&mut r, cm);
}

/// `counters_pkt(cm, c, b, v)`: one more packet in `c`, `v` more bytes in `b`.
pub fn counters_pkt(cm: CpumemPtr, c: usize, b: usize, v: u64) {
    let mut r = CountersRef::default();
    let counters = counters_enter(&mut r, cm);
    counters[c].fetch_add(1, Ordering::Relaxed);
    counters[b].fetch_add(v, Ordering::Relaxed);
    counters_leave(&mut r, cm);
}
/* </CODE> */
