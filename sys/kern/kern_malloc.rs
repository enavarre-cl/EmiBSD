/*	$OpenBSD: kern_malloc.c,v 1.158 2026/02/11 22:34:41 deraadt Exp $	*/
/*	$NetBSD: kern_malloc.c,v 1.15.4.2 1996/06/13 17:10:56 cgd Exp $	*/
/* <LICENSES> */
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

//! The kernel memory allocator, `malloc(9)`: `kern/kern_malloc.c`.
//!
//! Upstream: sys/kern/kern_malloc.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports `malloc`, `free`, `mallocarray`, `kmeminit_nkmempages`
//! and `kmeminit`; `sysctl_malloc` and `malloc_printit` (ddb) come with M7. The `KMEMSTATS`
//! option is feature `kmemstats`.
//!
//! ## Deviations
//! - `kmem_map` is not a submap (`uvm_km_suballoc` waits for `uvm_map.c`): `km_alloc` serves
//!   `kv_intrsafe` through the direct map, so `kmembase`/`kmemlimit` are the direct-map
//!   addresses of the lowest and the highest loaded page frame and `kmemusage` has one entry
//!   per frame in between, allocated as the C allocates its `nkmempages` entries.
//! - `btokup` always checks that the address is inside `[kmembase, kmemlimit)` (the C only
//!   under `DIAGNOSTIC`): it indexes an array.
//! - `malloc_mtx` (M5) is not here; the boot CPU is alone. A `M_WAITOK` request over a type's
//!   `ks_limit` cannot `msleep`: it is reported and fails.
//! - `poison_mem`/`poison_check`/`poison_value` (`subr_poison.c`) and the `uvm_map_checkprot`
//!   freelist check under `DIAGNOSTIC` are reported where the C calls them.
//! - `malloc_lasterr`/`ratecheck` (time) are not here: the "allocation too large" message is
//!   printed every time. `buckstring` and `memall` (sysctl strings) wait for M7.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI64, AtomicPtr, AtomicUsize, Ordering};

use crate::dev::rnd::arc4random_buf;
use crate::machine::pmap::pmap_map_direct;
#[cfg(any(feature = "kmemstats", feature = "diagnostic"))]
use crate::sys::malloc::INITKMEMNAMES;
use crate::sys::malloc::{
    Kmembuckets, Kmemusage, M_CANFAIL, M_NOWAIT, M_WAITOK, M_ZERO, MALLOC_MAX, MAXALLOCSAVE,
    MINBUCKET,
};
use crate::sys::param::{PAGE_SHIFT, PAGE_SIZE};
use crate::sys::queue::XsimpleqEntry;
use crate::sys::systm::PHYSMEM;
#[cfg(any(feature = "kmemstats", feature = "diagnostic"))]
use crate::unported;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_km::{
    KD_NOWAIT, KD_WAITOK, KP_DIRTY, KP_ZERO, KV_ANY, KV_INTRSAFE, km_alloc, km_free,
};
use crate::uvm::uvm_page::vm_physmem;
use crate::uvm::uvm_param::{VM_KERNEL_SPACE_SIZE, atop, ptoa, round_page};
use crate::{kassert, kprintf, queue_adapter};

#[cfg(feature = "kmemstats")]
use crate::kern::kern_synch::wakeup;
#[cfg(feature = "kmemstats")]
use crate::sys::malloc::{Kmemstats, M_LAST};
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

// static struct vm_map kmem_map_store; struct vm_map *kmem_map: see the module's deviations.

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
// malloc_mtx: M5.
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
// buckstring, memall: sysctl (M7).

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

/// `memname[type]`, `"???"` for an unknown type.
#[cfg(any(feature = "kmemstats", feature = "diagnostic"))]
fn memname(type_: i32) -> &'static str {
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

    // DIAGNOSTIC: assertwaitok() and the pool_debug == 2 yield(): M5.

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
    // mtx_enter(&malloc_mtx): M5.
    #[cfg(feature = "kmemstats")]
    {
        if ksp.ks_memuse.get() >= ksp.ks_limit.get() {
            if flags & M_NOWAIT != 0 {
                return None;
            }
            // DIAGNOSTIC: "cannot sleep for memory during boot" with curproc == &proc0.
            if ksp.ks_limblocks.get() < 65535 {
                ksp.ks_limblocks.set(ksp.ks_limblocks.get() + 1);
            }
            let _ = unported!("msleep_nsec (malloc over ks_limit, M5)");
            return None;
        }
        ksp.ks_memuse.set(ksp.ks_memuse.get() + allocsize as i64); // account for this early
        ksp.ks_size.set(ksp.ks_size.get() | (1 << indx));
    }
    #[cfg(feature = "diagnostic")]
    let mut freshalloc = false;
    if kbp.kb_freelist.first().is_none() {
        // mtx_leave(&malloc_mtx): M5.
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
        // splvm(): M4.
        let Some(va) = km_alloc(ptoa(npg), &KV_INTRSAFE, &KP_DIRTY, kdp) else {
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
                ksp.ks_memuse.set(ksp.ks_memuse.get() - allocsize as i64);
                let wake = ksp.ks_memuse.get() + allocsize as i64 >= ksp.ks_limit.get()
                    && ksp.ks_memuse.get() < ksp.ks_limit.get();
                if wake {
                    wakeup(ptr::from_ref(ksp));
                }
            }
            return None;
        };
        let va = va.as_ptr() as usize;
        // mtx_enter(&malloc_mtx): M5.
        #[cfg(feature = "kmemstats")]
        kbp.kb_total.set(kbp.kb_total.get() + kbp.kb_elmpercl.get());
        let kup = btokup(va);
        kup.ku_indx.set(indx as i16);
        #[cfg(feature = "diagnostic")]
        {
            freshalloc = true;
        }
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
    } else {
        #[cfg(feature = "diagnostic")]
        {
            freshalloc = false;
        }
    }
    let freep = kbp.kb_freelist.first()?;
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
    // mtx_leave(&malloc_mtx): M5.

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

    // mtx_enter(&malloc_mtx): M5.
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
        // mtx_leave(&malloc_mtx): M5. splvm(): M4.
        if let Some(v) = NonNull::new(addr as *mut u8) {
            km_free(v, ptoa(pagecnt as usize), &KV_INTRSAFE, &KP_DIRTY);
        }
        #[cfg(feature = "kmemstats")]
        {
            ksp.ks_memuse.set(ksp.ks_memuse.get() - size as i64);
            let wake = ksp.ks_memuse.get() + size as i64 >= ksp.ks_limit.get()
                && ksp.ks_memuse.get() < ksp.ks_limit.get();
            ksp.ks_inuse.set(ksp.ks_inuse.get() - 1);
            kbp.kb_total.set(kbp.kb_total.get() - 1);
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
        let wake = ksp.ks_memuse.get() + size as i64 >= ksp.ks_limit.get()
            && ksp.ks_memuse.get() < ksp.ks_limit.get();
        ksp.ks_inuse.set(ksp.ks_inuse.get() - 1);
        if wake {
            wakeup(ptr::from_ref(ksp));
        }
    }
    // SAFETY: the block is on no list (the DIAGNOSTIC search above is the C's).
    unsafe { kbp.kb_freelist.insert_tail(freep) };
    // mtx_leave(&malloc_mtx): M5.
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

    // base = vm_map_min(kernel_map); kmem_map = uvm_km_suballoc(kernel_map, &base, &limit,
    // nkmempages << PAGE_SHIFT, VM_MAP_INTRSAFE, FALSE, &kmem_map_store): the direct map
    // stands in (see the module's deviations), from frame 0 to the highest loaded frame.
    let segs = vm_physmem();
    let Some(lowest) = segs.iter().min_by_key(|seg| seg.start) else {
        #[allow(clippy::panic)] // uvm_page_init ran before us
        {
            panic!("kmeminit: no physical memory");
        }
    };
    // SAFETY: uvm_page_init set every segment's page array.
    let pg0 = unsafe { lowest.page(0) };
    let base = pmap_map_direct(pg0).as_usize();
    let frames = segs.iter().map(|seg| seg.end).max().unwrap_or(0) - lowest.start;
    let limit = base + ptoa(frames);
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
        // buckstring: sysctl (M7).
    }
    // memall (KMEMSTATS || DIAGNOSTIC): sysctl (M7).
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

#[cfg(test)]
mod tests;
