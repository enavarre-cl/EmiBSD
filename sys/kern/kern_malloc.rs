/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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

/*
 * Copyright (c) 1987, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)kern_malloc.c	8.3 (Berkeley) 1/4/94
 */
/* </LICENSES> */

/* <CODE> */
//! The kernel memory allocator, `malloc(9)`: `kern/kern_malloc.c`.
//!
//! Upstream: sys/kern/kern_malloc.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports `malloc`, `free`, `mallocarray`, `kmeminit_nkmempages`
//! and `kmeminit`; the diagnostic tools (stage 2) `sysctl_malloc` with `buckstring` and
//! `memall`; `malloc_printit` (ddb) is not here. The `KMEMSTATS` option is feature
//! `kmemstats`.
//!
//! ## Deviations
//! - On a machine without an MMU (`PMAP_NOMMU`: the host test double) `kmem_map` is not
//!   made: `kmembase`/`kmemlimit` are the direct-map addresses of the lowest and the highest
//!   loaded page frame and `kmemusage` has one entry per frame in between.
//! - `btokup` always checks that the address is inside `[kmembase, kmemlimit)` (the C only
//!   under `DIAGNOSTIC`): it indexes an array.
//! - M11a: `malloc_mtx` is the C's mutex (`IPL_VM`) around the buckets, `kmemusage` and
//!   `kmemstats`, with or without `MULTIPROCESSOR`; a `M_WAITOK` request over a type's
//!   `ks_limit` sleeps on it as in C. `km_alloc`/`km_free` run at `splvm` with the mutex
//!   released, as in C.
//! - `poison_mem`/`poison_check`/`poison_value` (`subr_poison.c`) and the `uvm_map_checkprot`
//!   freelist check under `DIAGNOSTIC` are reported where the C calls them.
//! - `malloc_lasterr`/`ratecheck` (time) are not here: the "allocation too large" message is
//!   printed every time.
//! - `sysctl_malloc` copies `kmembuckets`/`kmemstats` out through their `to_bytes` (the C
//!   layout, freelist head zeroed) under `malloc_mtx`. `BUCKETINDX` of a negative bucket
//!   number is the smallest bucket, as the C's `int` comparisons make it.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI64, AtomicPtr, AtomicUsize, Ordering};

use libkern::StaticCell;

use crate::dev::rnd::arc4random_buf;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_sysctl::{sysctl_rdstring, sysctl_rdstruct};
#[cfg(any(feature = "kmemstats", feature = "diagnostic"))]
use crate::kern::subr_prf::snprintf;
use crate::machine::intr::{IPL_VM, splvm, splx};
use crate::machine::pmap::pmap_map_direct;
use crate::machine::{Machine, Pmap};
use crate::sys::errno::Errno;
#[cfg(any(feature = "kmemstats", feature = "diagnostic"))]
use crate::sys::malloc::INITKMEMNAMES;
#[cfg(any(feature = "kmemstats", feature = "diagnostic"))]
use crate::sys::malloc::M_SYSCTL;
use crate::sys::malloc::{
    KERN_MALLOC_BUCKET, KERN_MALLOC_BUCKETS, KERN_MALLOC_KMEMNAMES, KERN_MALLOC_KMEMSTATS,
};
use crate::sys::malloc::{
    Kmembuckets, Kmemusage, M_CANFAIL, M_NOWAIT, M_WAITOK, M_ZERO, MALLOC_MAX, MAXALLOCSAVE,
    MINBUCKET,
};
use crate::sys::mutex::Mutex;
use crate::sys::param::{PAGE_SHIFT, PAGE_SIZE};
use crate::sys::queue::XsimpleqEntry;
use crate::sys::systm::PHYSMEM;
#[cfg(feature = "diagnostic")]
use crate::unported;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_km::{
    KD_NOWAIT, KD_WAITOK, KP_DIRTY, KP_ZERO, KV_ANY, KV_INTRSAFE, kernel_map, km_alloc, km_free,
    uvm_km_suballoc,
};
use crate::uvm::uvm_map::{VM_MAP_INTRSAFE, VmMap};
use crate::uvm::uvm_page::vm_physmem;
use crate::uvm::uvm_param::{VM_KERNEL_SPACE_SIZE, atop, ptoa, round_page};
use crate::{kassert, kprintf, queue_adapter};

#[cfg(feature = "kmemstats")]
use crate::kern::kern_synch::wakeup;
#[cfg(feature = "kmemstats")]
use crate::sys::malloc::Kmemstats;
#[cfg(any(feature = "kmemstats", feature = "diagnostic"))]
use crate::sys::malloc::M_LAST;
#[cfg(feature = "diagnostic")]
use crate::sys::malloc::{M_FREE, MINALLOCSIZE};

/// `BUCKETINDX(sz)`: the bucket of a size. Note that this relies upon `MINALLOCSIZE` being
/// `1 << MINBUCKET`.
pub fn bucketindx(sz: usize) -> usize {
    let mut b = 7 + MINBUCKET as i64;
    let mut d = 4i64;
    while d != 0 {
        if sz <= (1usize << b) {
            b -= d;
        } else {
            b += d;
        }
        d >>= 1;
    }
    if sz > (1usize << b) {
        b += 1;
    }
    b as usize
}

/// `kmem_map_store`.
static KMEM_MAP_STORE: VmMap = VmMap::new();
/// `kmem_map`: null until `kmeminit`.
static KMEM_MAP: AtomicPtr<VmMap> = AtomicPtr::new(core::ptr::null_mut());

/// `kmem_map`, once `kmeminit` made it.
pub fn kmem_map() -> &'static VmMap {
    let map = KMEM_MAP.load(Ordering::Acquire);
    // SAFETY: a non-null pointer is `KMEM_MAP_STORE`, set up by `uvm_km_suballoc`.
    match unsafe { map.as_ref() } {
        Some(map) => map,
        None => crate::kern::subr_prf::panic(format_args!("kmem_map used before kmeminit")),
    }
}

/// `NKMEMPAGES`: the configured number of pages in `kmem_map`; -1 asks for the run-time
/// calculation.
const NKMEMPAGES_OPTION: i64 = -1;

/// `struct kmem_freelist`: normally the freelist structure is used only to hold the list
/// pointer for free objects. However, when running with diagnostics, the first 8 bytes of the
/// structure is unused except for diagnostic information, and the free list pointer is at
/// offset 8 in the structure. Since the first 8 bytes is the portion of the structure most
/// often modified, this helps to detect memory reuse problems and avoid free list corruption.
#[repr(C)]
pub struct KmemFreelist {
    /// Poison, under `DIAGNOSTIC`.
    pub kf_spare0: Cell<i32>,
    /// The type the block was freed as, under `DIAGNOSTIC`.
    pub kf_type: Cell<i16>,
    /// Unused.
    pub kf_spare1: Cell<i16>,
    /// The bucket's free list.
    pub kf_flist: XsimpleqEntry<KmemFreelist>,
}

queue_adapter!(
    /// `XSIMPLEQ_HEAD(, kmem_freelist) kb_freelist`.
    pub KfList: KmemFreelist, kf_flist => XsimpleqEntry<KmemFreelist>
);

/// `nkmempages`: default number of pages in kmem_map. We attempt to calculate this at
/// run-time, but allow it to be either patched or set in the kernel config file.
static NKMEMPAGES: AtomicI64 = AtomicI64::new(NKMEMPAGES_OPTION);
/// `malloc_mtx`: guards `bucket[]`, `kmemstats[]` and `kmemusage`.
static MALLOC_MTX: Mutex = Mutex::new(IPL_VM);
/// `bucket[]`.
static BUCKET: [Kmembuckets; (MINBUCKET + 16) as usize] =
    [const { Kmembuckets::new() }; (MINBUCKET + 16) as usize];
/// `kmemstats[]`.
#[cfg(feature = "kmemstats")]
static KMEMSTATS: [Kmemstats; M_LAST as usize] = [const { Kmemstats::new() }; M_LAST as usize];
/// `kmemusage`: one descriptor per page of `kmem_map`.
static KMEMUSAGE: AtomicPtr<Kmemusage> = AtomicPtr::new(ptr::null_mut());
/// How many descriptors `kmemusage` has.
static KMEMUSAGE_LEN: AtomicUsize = AtomicUsize::new(0);
/// `kmembase`.
static KMEMBASE: AtomicUsize = AtomicUsize::new(0);
/// `kmemlimit`.
static KMEMLIMIT: AtomicUsize = AtomicUsize::new(0);
/// `buckstring`: the bucket sizes, comma separated, for `kern.malloc.buckets` (`[I]`:
/// written once by `kmeminit` under `KMEMSTATS`, before any sysctl; empty otherwise).
static BUCKSTRING: StaticCell<[u8; 16 * 8]> = StaticCell::new([0; 16 * 8]);
/// `memall`: the memory type names, comma separated (`kern.malloc.kmemnames`; `[I]`:
/// allocated once by `kmeminit`). Its length, NUL included, is `MEMALL_LEN`.
#[cfg(any(feature = "kmemstats", feature = "diagnostic"))]
static MEMALL: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
/// The size of the `memall` allocation.
#[cfg(any(feature = "kmemstats", feature = "diagnostic"))]
static MEMALL_LEN: AtomicUsize = AtomicUsize::new(0);

/// `addrmask[]`: this structure provides a set of masks to catch unaligned frees.
#[cfg(feature = "diagnostic")]
const ADDRMASK: [usize; 17] = [
    0,
    0x0000_0001,
    0x0000_0003,
    0x0000_0007,
    0x0000_000f,
    0x0000_001f,
    0x0000_003f,
    0x0000_007f,
    0x0000_00ff,
    0x0000_01ff,
    0x0000_03ff,
    0x0000_07ff,
    0x0000_0fff,
    0x0000_1fff,
    0x0000_3fff,
    0x0000_7fff,
    0x0000_ffff,
];

/// `memname[type]`, `"???"` for an unknown type. Only `DIAGNOSTIC` paths name a type here:
/// the `KMEMSTATS` user, the `msleep_nsec` wait message over `ks_limit`, is reported (M5);
/// `memall` reads `INITKMEMNAMES` directly.
#[cfg(feature = "diagnostic")]
fn memname(type_: i32) -> &'static str {
    usize::try_from(type_)
        .ok()
        .and_then(|t| INITKMEMNAMES.get(t).copied().flatten())
        .unwrap_or("???")
}

/// `memname[type]` as `malloc`'s wait message over `ks_limit`.
#[cfg(feature = "kmemstats")]
fn kmemstats_wmesg(type_: i32) -> &'static str {
    usize::try_from(type_)
        .ok()
        .and_then(|t| INITKMEMNAMES.get(t).copied().flatten())
        .unwrap_or("???")
}

/// `btokup(addr)`: the usage descriptor of the page holding `addr`.
fn btokup(addr: usize) -> &'static Kmemusage {
    let base = KMEMBASE.load(Ordering::Relaxed);
    let limit = KMEMLIMIT.load(Ordering::Relaxed);
    if addr < base || addr >= limit {
        #[allow(clippy::panic)] // the C panics here too (under DIAGNOSTIC)
        {
            panic!("free: non-malloced addr {:#x}", addr);
        }
    }
    let idx = (addr - base) >> PAGE_SHIFT;
    let usage = KMEMUSAGE.load(Ordering::Relaxed);
    if usage.is_null() || idx >= KMEMUSAGE_LEN.load(Ordering::Relaxed) {
        #[allow(clippy::panic)] // kmeminit has not run, or the table is too small
        {
            panic!("btokup: no kmemusage for {:#x}", addr);
        }
    }
    // SAFETY: `idx` is inside the table `kmeminit` allocated, which lives forever.
    unsafe { &*usage.add(idx) }
}

/// `malloc`: allocate a block of memory.
pub fn malloc(size: usize, type_: i32, flags: i32) -> Option<NonNull<u8>> {
    #[cfg(feature = "kmemstats")]
    let ksp = {
        if type_ <= 1 || type_ >= M_LAST {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!("malloc: bogus type {}", type_);
            }
        }
        &KMEMSTATS[type_ as usize]
    };
    #[cfg(not(feature = "kmemstats"))]
    let _ = type_;

    kassert!(flags & (M_WAITOK | M_NOWAIT) != 0);

    #[cfg(feature = "diagnostic")]
    if flags & M_NOWAIT == 0 {
        crate::kern::subr_xxx::assertwaitok();
        if crate::kern::subr_pool::POOL_DEBUG.load(Ordering::Relaxed) == 2 {
            crate::kern::sched_bsd::r#yield();
        }
    }

    if size > MALLOC_MAX {
        if flags & M_CANFAIL != 0 {
            // ratecheck(&malloc_lasterr, &malloc_errintvl): see the module's deviations.
            kprintf!(
                "malloc(): allocation too large, type = {}, size = {}\n",
                type_,
                size
            );
            return None;
        }
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!(
                "malloc: allocation too large, type = {}, size = {}",
                type_, size
            );
        }
    }

    let indx = bucketindx(size);
    let allocsize = if size > MAXALLOCSAVE {
        round_page(size)
    } else {
        1 << indx
    };
    let kbp = &BUCKET[indx];
    mtx_enter(&MALLOC_MTX);
    #[cfg(feature = "kmemstats")]
    {
        while ksp.ks_memuse.get() >= ksp.ks_limit.get() {
            if flags & M_NOWAIT != 0 {
                mtx_leave(&MALLOC_MTX);
                return None;
            }
            #[cfg(feature = "diagnostic")]
            if flags & M_WAITOK != 0
                && crate::machine::cpu::curproc()
                    .is_some_and(|p| ptr::eq(p, &crate::kern::init_main::PROC0))
            {
                crate::kern::subr_prf::panic(format_args!(
                    "malloc: cannot sleep for memory during boot"
                ));
            }
            if ksp.ks_limblocks.get() < 65535 {
                ksp.ks_limblocks.set(ksp.ks_limblocks.get() + 1);
            }
            let _ = crate::kern::kern_synch::msleep_nsec(
                ptr::from_ref(ksp),
                &MALLOC_MTX,
                crate::sys::param::PSWP + 2,
                kmemstats_wmesg(type_),
                crate::sys::systm::INFSLP,
            );
        }
        ksp.ks_memuse.set(ksp.ks_memuse.get() + allocsize as i64); // account for this early
        ksp.ks_size.set(ksp.ks_size.get() | (1 << indx));
    }
    // freshalloc: the block comes from pages allocated just below (the C sets it once they
    // are, and clears it when the bucket had a free block; a failure returns before use).
    #[cfg(feature = "diagnostic")]
    let freshalloc = kbp.kb_freelist.first().is_none();
    if kbp.kb_freelist.first().is_none() {
        mtx_leave(&MALLOC_MTX);
        let npg = atop(round_page(allocsize));
        let swpages = UVMEXP.swpages.load(Ordering::Relaxed);
        let swpgonly = UVMEXP.swpgonly.load(Ordering::Relaxed);
        kassert!(swpgonly <= swpages);
        let kdp = if flags & M_NOWAIT != 0
            || (flags & M_CANFAIL != 0 && (swpages - swpgonly) as usize <= npg)
        {
            &KD_NOWAIT
        } else {
            &KD_WAITOK
        };
        let s = splvm();
        let va = km_alloc(ptoa(npg), &KV_INTRSAFE, &KP_DIRTY, kdp);
        splx(s);
        let Some(va) = va else {
            // Kmem_malloc() can return NULL, even if it can wait, if there is no map space
            // available, because it can't fix that problem. Neither can we, right now. (We
            // should release pages which are completely free and which are in buckets with
            // too many free elements.)
            if flags & (M_NOWAIT | M_CANFAIL) == 0 {
                #[allow(clippy::panic)] // the C panics here too
                {
                    panic!("malloc: out of space in kmem_map");
                }
            }

            #[cfg(feature = "kmemstats")]
            {
                mtx_enter(&MALLOC_MTX);
                ksp.ks_memuse.set(ksp.ks_memuse.get() - allocsize as i64);
                let wake = ksp.ks_memuse.get() + allocsize as i64 >= ksp.ks_limit.get()
                    && ksp.ks_memuse.get() < ksp.ks_limit.get();
                mtx_leave(&MALLOC_MTX);
                if wake {
                    wakeup(ptr::from_ref(ksp));
                }
            }
            return None;
        };
        let va = va.as_ptr() as usize;
        mtx_enter(&MALLOC_MTX);
        #[cfg(feature = "kmemstats")]
        kbp.kb_total.set(kbp.kb_total.get() + kbp.kb_elmpercl.get());
        let kup = btokup(va);
        kup.ku_indx.set(indx as i16);
        if allocsize > MAXALLOCSAVE {
            kup.set_ku_pagecnt(npg as u16);
            return malloc_out(
                kbp,
                #[cfg(feature = "kmemstats")]
                ksp,
                va,
                size,
                flags,
            );
        }
        #[cfg(feature = "kmemstats")]
        {
            kup.set_ku_freecnt(kbp.kb_elmpercl.get() as u16);
            kbp.kb_totalfree
                .set(kbp.kb_totalfree.get() + kbp.kb_elmpercl.get());
        }
        let mut cp = va + (npg * PAGE_SIZE) - allocsize;
        loop {
            // SAFETY: `cp` is the start of a block inside the pages just allocated; any bit
            // pattern is a valid kmem_freelist (three integers and a link).
            let freep = unsafe { &*(cp as *const KmemFreelist) };
            #[cfg(feature = "diagnostic")]
            {
                // Copy in known text to detect modification after freeing.
                let _ = unported!("poison_mem (subr_poison.c) in malloc");
                freep.kf_type.set(M_FREE as i16);
            }
            // SAFETY: a block of a fresh allocation is on no list.
            unsafe { kbp.kb_freelist.insert_head(freep) };
            if cp <= va {
                break;
            }
            cp -= allocsize;
        }
    }
    let Some(freep) = kbp.kb_freelist.first() else {
        // Cannot happen: the bucket was just filled (the C dereferences it).
        mtx_leave(&MALLOC_MTX);
        return None;
    };
    let freep: *const KmemFreelist = freep;
    // SAFETY: `freep` is the head of the bucket's list.
    unsafe { kbp.kb_freelist.remove_head() };
    let va = freep as usize;
    #[cfg(feature = "diagnostic")]
    {
        // SAFETY: `freep` was a linked block; it is the caller's now.
        let freep = unsafe { &*freep };
        let savedtype = memname(i32::from(freep.kf_type.get()));
        if !freshalloc && kbp.kb_freelist.first().is_some() {
            // vm_map_lock(kmem_map); uvm_map_checkprot(kmem_map, addr, addr + sizeof(struct
            // kmem_freelist), PROT_WRITE): uvm_map.c.
            let _ = unported!("uvm_map_checkprot (malloc freelist check)");
        }

        // Fill the fields that we've used with poison and check that the data hasn't been
        // modified.
        let _ = unported!("poison_mem/poison_check (subr_poison.c) in malloc");
        let _ = savedtype;

        freep.kf_spare0.set(0);
    }
    #[cfg(feature = "kmemstats")]
    {
        let kup = btokup(va);
        if kup.ku_indx.get() as usize != indx {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!("malloc: wrong bucket");
            }
        }
        if kup.ku_freecnt() == 0 {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!("malloc: lost data");
            }
        }
        kup.set_ku_freecnt(kup.ku_freecnt() - 1);
        kbp.kb_totalfree.set(kbp.kb_totalfree.get() - 1);
    }
    malloc_out(
        kbp,
        #[cfg(feature = "kmemstats")]
        ksp,
        va,
        size,
        flags,
    )
}

/// `malloc`'s `out:` label: the statistics, the lock release and `M_ZERO`.
fn malloc_out(
    kbp: &Kmembuckets,
    #[cfg(feature = "kmemstats")] ksp: &Kmemstats,
    va: usize,
    size: usize,
    flags: i32,
) -> Option<NonNull<u8>> {
    #[cfg(feature = "kmemstats")]
    {
        kbp.kb_calls.set(kbp.kb_calls.get() + 1);
        ksp.ks_inuse.set(ksp.ks_inuse.get() + 1);
        ksp.ks_calls.set(ksp.ks_calls.get() + 1);
        if ksp.ks_memuse.get() > ksp.ks_maxused.get() {
            ksp.ks_maxused.set(ksp.ks_memuse.get());
        }
    }
    #[cfg(not(feature = "kmemstats"))]
    let _ = kbp;
    mtx_leave(&MALLOC_MTX);

    let va = NonNull::new(va as *mut u8)?;
    if flags & M_ZERO != 0 {
        // SAFETY: `size` bytes at `va` are the block just allocated, the caller's now.
        unsafe { ptr::write_bytes(va.as_ptr(), 0, size) };
    }

    // TRACEPOINT(uvm, malloc): not configured.

    Some(va)
}

/// `free`: free a block of memory allocated by `malloc`.
pub fn free(addr: NonNull<u8>, type_: i32, freedsize: usize) {
    #[cfg(feature = "kmemstats")]
    let ksp = &KMEMSTATS[type_ as usize];
    let addr = addr.as_ptr() as usize;

    // DIAGNOSTIC's range check is btokup's.

    // TRACEPOINT(uvm, free): not configured.

    mtx_enter(&MALLOC_MTX);
    let kup = btokup(addr);
    let indx = kup.ku_indx.get() as usize;
    let mut size = 1usize << indx;
    let kbp = &BUCKET[indx];
    if size > MAXALLOCSAVE {
        size = (kup.ku_pagecnt() as usize) << PAGE_SHIFT;
    }
    #[cfg(feature = "diagnostic")]
    {
        if freedsize != 0 && freedsize > size {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!(
                    "free: size too large {} > {} ({:#x}) type {}",
                    freedsize,
                    size,
                    addr,
                    memname(type_)
                );
            }
        }
        if freedsize != 0 && size > MINALLOCSIZE && freedsize <= size / 2 {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!(
                    "free: size too small {} <= {} / 2 ({:#x}) type {}",
                    freedsize,
                    size,
                    addr,
                    memname(type_)
                );
            }
        }
        // Check for returns of data that do not point to the beginning of the allocation.
        let alloc = if size > PAGE_SIZE {
            ADDRMASK[bucketindx(PAGE_SIZE)]
        } else {
            ADDRMASK[indx]
        };
        if addr & alloc != 0 {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!(
                    "free: unaligned addr {:#x}, size {}, type {}, mask {}",
                    addr,
                    size,
                    memname(type_),
                    alloc
                );
            }
        }
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = (freedsize, type_);
    if size > MAXALLOCSAVE {
        let pagecnt = kup.ku_pagecnt();

        kup.ku_indx.set(0);
        kup.set_ku_pagecnt(0);
        mtx_leave(&MALLOC_MTX);
        let s = splvm();
        if let Some(v) = NonNull::new(addr as *mut u8) {
            km_free(v, ptoa(pagecnt as usize), &KV_INTRSAFE, &KP_DIRTY);
        }
        splx(s);
        #[cfg(feature = "kmemstats")]
        {
            mtx_enter(&MALLOC_MTX);
            ksp.ks_memuse.set(ksp.ks_memuse.get() - size as i64);
            let wake = ksp.ks_memuse.get() + size as i64 >= ksp.ks_limit.get()
                && ksp.ks_memuse.get() < ksp.ks_limit.get();
            ksp.ks_inuse.set(ksp.ks_inuse.get() - 1);
            kbp.kb_total.set(kbp.kb_total.get() - 1);
            mtx_leave(&MALLOC_MTX);
            if wake {
                wakeup(ptr::from_ref(ksp));
            }
        }
        return;
    }
    // SAFETY: the block is `size` (>= a kmem_freelist) bytes the caller returns; any bit
    // pattern is a valid kmem_freelist.
    let freep = unsafe { &*(addr as *const KmemFreelist) };
    #[cfg(feature = "diagnostic")]
    {
        // Check for multiple frees. Use a quick check to see if it looks free before
        // laboriously searching the freelist: poison_value, subr_poison.c.
        let _ = unported!("poison_value/poison_mem (subr_poison.c) in free");
        for fp in kbp.kb_freelist.iter() {
            if ptr::eq(fp, freep) {
                kprintf!("multiply freed item {:#x}\n", addr);
                #[allow(clippy::panic)] // the C panics here too
                {
                    panic!("free: duplicated free");
                }
            }
        }
        // Save the type being freed so we can list likely culprit if modification is
        // detected when the object is reallocated.
        freep.kf_type.set(type_ as i16);
    }
    #[cfg(feature = "kmemstats")]
    {
        kup.set_ku_freecnt(kup.ku_freecnt() + 1);
        if u64::from(kup.ku_freecnt()) >= kbp.kb_elmpercl.get() {
            if u64::from(kup.ku_freecnt()) > kbp.kb_elmpercl.get() {
                #[allow(clippy::panic)] // the C panics here too
                {
                    panic!("free: multiple frees");
                }
            } else if kbp.kb_totalfree.get() > kbp.kb_highwat.get() {
                kbp.kb_couldfree.set(kbp.kb_couldfree.get() + 1);
            }
        }
        kbp.kb_totalfree.set(kbp.kb_totalfree.get() + 1);
        ksp.ks_memuse.set(ksp.ks_memuse.get() - size as i64);
    }
    #[cfg(feature = "kmemstats")]
    let wake = ksp.ks_memuse.get() + size as i64 >= ksp.ks_limit.get()
        && ksp.ks_memuse.get() < ksp.ks_limit.get();
    #[cfg(feature = "kmemstats")]
    ksp.ks_inuse.set(ksp.ks_inuse.get() - 1);
    // SAFETY: the block is on no list (the DIAGNOSTIC search above is the C's).
    unsafe { kbp.kb_freelist.insert_tail(freep) };
    mtx_leave(&MALLOC_MTX);
    #[cfg(feature = "kmemstats")]
    if wake {
        wakeup(ptr::from_ref(ksp));
    }
}

/// `kmeminit_nkmempages`: compute the number of pages that kmem_map will map, that is, the
/// size of the kernel malloc arena.
pub fn kmeminit_nkmempages() {
    if NKMEMPAGES.load(Ordering::Relaxed) != -1 {
        // It's already been set (by us being here before, or by patching or kernel config
        // options), bail out now.
        return;
    }

    // We use the following (simple) formula:
    //
    // Up to 1G physmem use physical memory / 4, above 1G add an extra 16MB per 1G of memory.
    //
    // Clamp it down depending on VM_KERNEL_SPACE_SIZE
    // - up and including 512M -> 64MB
    // - between 512M and 1024M -> 128MB
    // - over 1024M clamping to VM_KERNEL_SPACE_SIZE / 4
    let physmem = PHYSMEM.load(Ordering::Relaxed);
    let one_g = atop(1024 * 1024 * 1024);
    let mut npages = physmem.min(one_g) / 4;
    if physmem > one_g {
        npages += (physmem - one_g) / 64;
    }

    if VM_KERNEL_SPACE_SIZE <= 512 * 1024 * 1024 {
        npages = npages.min(atop(64 * 1024 * 1024));
    } else if VM_KERNEL_SPACE_SIZE <= 1024 * 1024 * 1024 {
        npages = npages.min(atop(128 * 1024 * 1024));
    } else if npages > atop(VM_KERNEL_SPACE_SIZE) / 4 {
        npages = atop(VM_KERNEL_SPACE_SIZE) / 4;
    }

    NKMEMPAGES.store(npages as i64, Ordering::Relaxed);
}

/// `nkmempages`: the size of the kernel malloc arena, in pages (0 before
/// `kmeminit_nkmempages`).
pub fn nkmempages() -> usize {
    NKMEMPAGES.load(Ordering::Relaxed).max(0) as usize
}

/// `kmeminit`: initialize the kernel memory allocator.
pub fn kmeminit() {
    #[cfg(feature = "diagnostic")]
    if size_of::<KmemFreelist>() > (1 << MINBUCKET) {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("kmeminit: minbucket too small/struct freelist too big");
        }
    }

    // Compute the number of kmem_map pages, if we have not done so already.
    kmeminit_nkmempages();

    let (base, limit, frames) = if <Machine as Pmap>::PMAP_NOMMU {
        // No MMU (the host double): the direct map stands in for kmem_map, from the lowest
        // to the highest loaded page frame (see the module's deviations).
        let segs = vm_physmem();
        let Some(lowest) = segs.iter().min_by_key(|seg| seg.start) else {
            crate::kern::subr_prf::panic(format_args!("kmeminit: no physical memory"));
        };
        // SAFETY: uvm_page_init set every segment's page array.
        let pg0 = unsafe { lowest.page(0) };
        let base = pmap_map_direct(pg0).as_usize();
        let frames = segs.iter().map(|seg| seg.end).max().unwrap_or(0) - lowest.start;
        (base, base + ptoa(frames), frames)
    } else {
        let npages = NKMEMPAGES.load(Ordering::Relaxed) as usize;
        let mut base = kernel_map().min_offset.get();
        let mut limit = 0;
        let map = uvm_km_suballoc(
            kernel_map(),
            &mut base,
            &mut limit,
            npages << PAGE_SHIFT,
            VM_MAP_INTRSAFE,
            false,
            Some(&KMEM_MAP_STORE),
        );
        KMEM_MAP.store(ptr::from_ref(map).cast_mut(), Ordering::Release);
        (base, limit, npages)
    };
    KMEMBASE.store(base, Ordering::Relaxed);
    KMEMLIMIT.store(limit, Ordering::Relaxed);
    let Some(kmemusage) = km_alloc(
        round_page(frames * size_of::<Kmemusage>()),
        &KV_ANY,
        &KP_ZERO,
        &KD_WAITOK,
    ) else {
        #[allow(clippy::panic)] // km_alloc with kd_waitok does not return NULL in the C
        {
            panic!("kmeminit: no memory for kmemusage");
        }
    };
    KMEMUSAGE.store(kmemusage.as_ptr().cast::<Kmemusage>(), Ordering::Relaxed);
    KMEMUSAGE_LEN.store(frames, Ordering::Relaxed);
    for kb in &BUCKET {
        let mut cookie = [0u8; size_of::<usize>()];
        arc4random_buf(&mut cookie);
        kb.kb_freelist.init(usize::from_ne_bytes(cookie));
    }
    #[cfg(feature = "kmemstats")]
    {
        for (indx, kb) in BUCKET.iter().enumerate() {
            let elmpercl = if 1 << indx >= PAGE_SIZE {
                1
            } else {
                (PAGE_SIZE / (1 << indx)) as u64
            };
            kb.kb_elmpercl.set(elmpercl);
            kb.kb_highwat.set(5 * elmpercl);
        }
        for ks in &KMEMSTATS {
            ks.ks_limit
                .set(nkmempages() as i64 * PAGE_SIZE as i64 * 6 / 10);
        }

        // SAFETY: `buckstring` is `[I]`: kmeminit runs once on the boot CPU, before any
        // sysctl can read it.
        let buckstring = unsafe { BUCKSTRING.get_mut() };
        buckstring.fill(0);
        let mut siz = 0;
        for i in MINBUCKET..MINBUCKET + 16 {
            siz += snprintf(&mut buckstring[siz..], format_args!("{},", 1u32 << i));
        }
        // Remove trailing comma
        if siz > 0 {
            buckstring[siz - 1] = 0;
        }
    }
    #[cfg(any(feature = "kmemstats", feature = "diagnostic"))]
    {
        // Figure out how large a buffer we need.
        let totlen: usize = INITKMEMNAMES
            .iter()
            .map(|n| n.map_or(0, str::len) + 1)
            .sum();
        let len = totlen + M_LAST as usize;
        let Some(mem) = malloc(len, M_SYSCTL, M_WAITOK | M_ZERO) else {
            #[allow(clippy::panic)] // M_WAITOK does not fail in the C
            {
                panic!("kmeminit: no memory for memall");
            }
        };
        // SAFETY: a fresh, zeroed `len`-byte allocation, published below and never freed;
        // nothing else sees it yet.
        let memall = unsafe { core::slice::from_raw_parts_mut(mem.as_ptr(), len) };
        let mut siz = 0;
        for name in INITKMEMNAMES {
            siz += snprintf(&mut memall[siz..], format_args!("{},", name.unwrap_or("")));
        }
        // Remove trailing comma
        if siz > 0 {
            memall[siz - 1] = 0;
        }
        // Now, convert all spaces to underscores.
        for c in memall[..totlen].iter_mut() {
            if *c == b' ' {
                *c = b'_';
            }
        }
        MEMALL_LEN.store(len, Ordering::Relaxed);
        MEMALL.store(mem.as_ptr(), Ordering::Release);
    }
}

/// `sysctl_malloc`: return kernel malloc statistics information (`kern.malloc`): the bucket
/// sizes, one bucket's counters, one type's statistics (`KMEMSTATS`) or the type names.
pub fn sysctl_malloc(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    let Some(&what) = name.first() else {
        return Err(Errno::ENOTDIR);
    };
    if name.len() != 2 && what != KERN_MALLOC_BUCKETS && what != KERN_MALLOC_KMEMNAMES {
        return Err(Errno::ENOTDIR); // overloaded
    }

    match what {
        KERN_MALLOC_BUCKETS => {
            // SAFETY: `[I]`: written only by kmeminit, before any sysctl.
            let buckstring = unsafe { BUCKSTRING.get() };
            sysctl_rdstring(oldp, oldlenp, newp, buckstring)
        }
        KERN_MALLOC_BUCKET => {
            // BUCKETINDX of an int: a negative size compares below every bucket.
            let sz = usize::try_from(name[1]).unwrap_or(0);
            // The freelist head is left out (zeroed).
            mtx_enter(&MALLOC_MTX);
            let kb = BUCKET[bucketindx(sz)].to_bytes();
            mtx_leave(&MALLOC_MTX);
            sysctl_rdstruct(oldp, oldlenp, newp, &kb)
        }
        KERN_MALLOC_KMEMSTATS => {
            #[cfg(feature = "kmemstats")]
            {
                if name[1] < 0 || name[1] >= M_LAST {
                    return Err(Errno::EINVAL);
                }
                mtx_enter(&MALLOC_MTX);
                let km = KMEMSTATS[name[1] as usize].to_bytes();
                mtx_leave(&MALLOC_MTX);
                sysctl_rdstruct(oldp, oldlenp, newp, &km)
            }
            #[cfg(not(feature = "kmemstats"))]
            Err(Errno::EOPNOTSUPP)
        }
        #[cfg(any(feature = "kmemstats", feature = "diagnostic"))]
        KERN_MALLOC_KMEMNAMES => {
            let p = MEMALL.load(Ordering::Acquire);
            if p.is_null() {
                return sysctl_rdstring(oldp, oldlenp, newp, b"");
            }
            // SAFETY: published by kmeminit once filled, `MEMALL_LEN` bytes, never freed
            // nor written again.
            let memall =
                unsafe { core::slice::from_raw_parts(p, MEMALL_LEN.load(Ordering::Relaxed)) };
            sysctl_rdstring(oldp, oldlenp, newp, memall)
        }
        _ => Err(Errno::EOPNOTSUPP),
    }
}

/// `MUL_NO_OVERFLOW`: products of two factors below it cannot overflow.
const MUL_NO_OVERFLOW: usize = 1 << (size_of::<usize>() * 4);

/// `mallocarray`: `malloc` of `nmemb * size` bytes, refusing an overflow.
pub fn mallocarray(nmemb: usize, size: usize, type_: i32, flags: i32) -> Option<NonNull<u8>> {
    if (nmemb >= MUL_NO_OVERFLOW || size >= MUL_NO_OVERFLOW)
        && nmemb > 0
        && usize::MAX / nmemb < size
    {
        if flags & M_CANFAIL != 0 {
            return None;
        }
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("mallocarray: overflow {} * {}", nmemb, size);
        }
    }
    malloc(size * nmemb, type_, flags)
}

/// Host tests: forget every type's usage. `setup_real_memory` (`subr_pool.rs`) replaces
/// the memory the counted allocations lived in, which nobody frees, so the counts start over
/// with it; otherwise each test that sets the vfs up again (`ufs_ihashinit`'s table, ...)
/// leaves its usage behind until a type passes its `ks_limit`.
#[cfg(all(test, feature = "kmemstats"))]
pub(crate) fn kmemstats_test_reset() {
    for ks in &KMEMSTATS {
        ks.ks_inuse.set(0);
        ks.ks_memuse.set(0);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `malloc(9)` over real memory (see `subr_pool.rs` for the setup).

    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::sys::malloc::{M_DEVBUF, M_TEMP};

    #[test]
    fn bucketindx_follows_the_c_formula() {
        assert_eq!(bucketindx(1), 4);
        assert_eq!(bucketindx(16), 4);
        assert_eq!(bucketindx(17), 5);
        assert_eq!(bucketindx(100), 7);
        assert_eq!(bucketindx(4096), 12);
        assert_eq!(bucketindx(4097), 13);
        assert_eq!(bucketindx(8192), 13);
        assert_eq!(bucketindx(65536), 16);
    }

    #[test]
    fn small_blocks_come_from_buckets_and_are_reused() {
        let _g = setup_real_memory();
        let first: Vec<NonNull<u8>> = (0..40)
            .map(|i| {
                let p = malloc(100, M_TEMP, M_NOWAIT).expect("a block");
                assert_eq!(
                    p.as_ptr() as usize % 128,
                    0,
                    "a 100-byte request is a 128-byte chunk"
                );
                // SAFETY: 100 bytes at `p` are this test's.
                unsafe { ptr::write_bytes(p.as_ptr(), i as u8, 100) };
                p
            })
            .collect();
        let mut addrs: Vec<usize> = first.iter().map(|p| p.as_ptr() as usize).collect();
        addrs.sort_unstable();
        assert!(
            addrs.windows(2).all(|w| w[1] - w[0] >= 128),
            "distinct chunks"
        );
        let pages_used = UVMEXP.free.load(Ordering::Relaxed);
        let mut pages: Vec<usize> = addrs.iter().map(|a| a & !(PAGE_SIZE - 1)).collect();
        pages.dedup();
        assert_eq!(pages.len(), 2, "40 chunks of 128 bytes take two pages");
        for (i, p) in first.iter().enumerate() {
            // SAFETY: as above.
            assert_eq!(unsafe { ptr::read_volatile(p.as_ptr()) }, i as u8);
            free(*p, M_TEMP, 100);
        }
        // The bucket keeps its pages: a second round takes chunks from the same two pages (the
        // never-used ones first, then the freed ones) and allocates nothing new.
        let again: Vec<usize> = (0..40)
            .map(|_| malloc(100, M_TEMP, M_NOWAIT).expect("a block").as_ptr() as usize)
            .collect();
        assert!(
            again
                .iter()
                .all(|a| pages.contains(&(a & !(PAGE_SIZE - 1))))
        );
        assert_eq!(
            UVMEXP.free.load(Ordering::Relaxed),
            pages_used,
            "no new page"
        );
        for p in again {
            free(NonNull::new(p as *mut u8).expect("non-null"), M_TEMP, 100);
        }
    }

    #[test]
    fn large_blocks_take_and_return_pages() {
        let _g = setup_real_memory();
        let free_before = UVMEXP.free.load(Ordering::Relaxed);
        let p = malloc(20_000, M_DEVBUF, M_NOWAIT).expect("a block");
        assert_eq!(p.as_ptr() as usize % PAGE_SIZE, 0, "page aligned");
        assert_eq!(UVMEXP.free.load(Ordering::Relaxed), free_before - 5);
        // SAFETY: 20000 bytes at `p` are this test's.
        unsafe { ptr::write_bytes(p.as_ptr(), 0x5a, 20_000) };
        free(p, M_DEVBUF, 20_000);
        assert_eq!(UVMEXP.free.load(Ordering::Relaxed), free_before);
    }

    #[test]
    fn m_zero_zeroes_and_mallocarray_refuses_overflow() {
        let _g = setup_real_memory();
        let p = malloc(300, M_TEMP, M_NOWAIT).expect("a block");
        // SAFETY: 300 bytes at `p` are this test's.
        unsafe { ptr::write_bytes(p.as_ptr(), 0xff, 300) };
        free(p, M_TEMP, 300);
        let q = malloc(300, M_TEMP, M_NOWAIT | M_ZERO).expect("a block");
        // SAFETY: as above.
        assert!((0..300).all(|i| (unsafe { ptr::read_volatile(q.as_ptr().add(i)) }) == 0));
        free(q, M_TEMP, 300);

        assert!(mallocarray(usize::MAX / 2, 4, M_TEMP, M_NOWAIT | M_CANFAIL).is_none());
        let r = mallocarray(3, 8, M_TEMP, M_NOWAIT).expect("a block");
        free(r, M_TEMP, 24);
        assert!(malloc(MALLOC_MAX + 1, M_TEMP, M_NOWAIT | M_CANFAIL).is_none());
    }
}
/* </TESTS> */
