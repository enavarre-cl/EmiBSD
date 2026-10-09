/* <LICENSES> */
/*
 * Copyright (c) 2007 Artur Grabowski <art@openbsd.org>
 * Copyright (c) 2012-2016,2019 Bob Beck <beck@openbsd.org>
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
//! The memory behind the buffer cache: each buffer's pages live in its own (or its cluster's)
//! `uvm_object`, and a `MAXPHYS` slot of a kernel virtual arena maps them while the buffer is
//! busy; idle buffers keep their mapping on an LRU (`buf_valist`) until a busy one needs the
//! slot.
//!
//! Upstream: sys/kern/vfs_biomem.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - On a machine without an MMU (`PMAP_NOMMU`: only the host test double) `buf_mem_init`
//!   reserves no kernel virtual space: the slots are only counted, `buf_alloc_pages` gets
//!   physically contiguous pages (`uvm_pagealloc_multi` does that there) and `b_data` points
//!   at them through the direct map. The `pmap_kenter_pa`/`pmap_kremove` calls still run
//!   (the host pmap records them).
//! - The globals (`buf_kva_start`, `buf_kva_end`, `buf_needva`) are atomics; `buf_valist`
//!   is a `Sync` static changed at `splbio`.

use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use crate::kassert;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_pool::pool_put;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{BCSTATS, BUFPOOL, bufbackoff, cleanerproc, dma_constraint};
use crate::kern::vfs_sync::syncerproc;
use crate::machine::Machine;
use crate::machine::cpu::curproc;
use crate::machine::intr::{IPL_BIO, splassert};
use crate::machine::pmap::{
    Pmap, pmap_kenter_pa, pmap_kernel, pmap_kremove, pmap_map_direct, pmap_update,
};
use crate::sys::buf::{B_BUSY, B_RELEASED, BValist, Buf, RESERVE_SLOTS};
use crate::sys::mman::{MADV_NORMAL, MAP_INHERIT_NONE, PROT_NONE, PROT_READ, PROT_WRITE};
use crate::sys::param::{MAXPHYS, PRIBIO};
use crate::sys::queue::TailqHead;
use crate::sys::systm::INFSLP;
use crate::sys::types::{Vaddr, Vsize};
use crate::uvm::uvm_extern::{
    UVM_PLA_NOWAIT, UVM_PLA_NOWAKE, UVM_PLA_WAITOK, UVM_UNKNOWN_OFFSET, uvm_mapflag,
};
use crate::uvm::uvm_km::kernel_map;
use crate::uvm::uvm_map::uvm_map;
use crate::uvm::uvm_object::{BUFCACHE_PAGER, uvm_obj_free, uvm_obj_init};
use crate::uvm::uvm_page::{uvm_pagealloc_multi, uvm_pagelookup, vm_page_to_phys};
use crate::uvm::uvm_param::{atop, ptoa, round_page};

/// `buf_valist`'s type: a `Sync` static queue.
pub struct BufValist(pub TailqHead<BValist>);

// SAFETY: changed at `splbio` under the kernel lock, as in C.
unsafe impl Sync for BufValist {}

/// `buf_kva_start`: the first slot of the arena never handed out.
pub static BUF_KVA_START: AtomicUsize = AtomicUsize::new(0);
/// `buf_kva_end`: the end of the arena.
pub static BUF_KVA_END: AtomicUsize = AtomicUsize::new(0);
/// `buf_needva`: threads sleeping for a slot.
pub static BUF_NEEDVA: AtomicI32 = AtomicI32::new(0);
/// `buf_valist`: idle mapped buffers, least recently used first.
pub static BUF_VALIST: BufValist = BufValist(TailqHead::new());

/// Whether the machine has no MMU (the host double; see the module's deviations).
fn nommu() -> bool {
    <Machine as Pmap>::PMAP_NOMMU
}

/// `buf_mem_init(size)`: reserves the arena of `size` bytes in `kernel_map`.
pub fn buf_mem_init(size: usize) {
    BUF_VALIST.0.init();

    let start = if nommu() {
        // No MMU: the slots are only counted; `buf_map` uses the direct map. Start above 0 so
        // a slot address is never NULL.
        MAXPHYS
    } else {
        let mut start = kernel_map().min_offset.get();
        if uvm_map(
            kernel_map(),
            &mut start,
            size,
            None,
            UVM_UNKNOWN_OFFSET,
            crate::sys::param::PAGE_SIZE,
            uvm_mapflag(PROT_NONE, PROT_NONE, MAP_INHERIT_NONE, MADV_NORMAL, 0),
        )
        .is_err()
        {
            panic(format_args!("buf_mem_init: can't reserve VM for buffers"));
        }
        start
    };
    BUF_KVA_START.store(start, Ordering::Relaxed);
    BUF_KVA_END.store(start + size, Ordering::Relaxed);

    // Contiguous mapping
    let slots = (size / MAXPHYS) as i64;
    BCSTATS.kvaslots.store(slots, Ordering::Relaxed);
    BCSTATS.kvaslots_avail.store(slots, Ordering::Relaxed);
}

/// `buf_acquire`: sets `B_BUSY` and ensures that the buffer is mapped in the kvm.
pub fn buf_acquire(bp: &'static Buf) {
    kassert!(!bp.isset(B_BUSY));
    splassert(IPL_BIO, "buf_acquire");
    // Busy before waiting for kvm.
    bp.set(B_BUSY);
    buf_map(bp);
}

/// Acquire a buf but do not map it. Preserve any mapping it did have.
pub fn buf_acquire_nomap(bp: &'static Buf) {
    splassert(IPL_BIO, "buf_acquire_nomap");
    bp.set(B_BUSY);
    if !bp.b_data.get().is_null() {
        // SAFETY: an idle mapped buffer is on `buf_valist` (`buf_release`).
        unsafe { BUF_VALIST.0.remove(bp) };
        BCSTATS.kvaslots_avail.fetch_sub(1, Ordering::Relaxed);
        BCSTATS.busymapped.fetch_add(1, Ordering::Relaxed);
    }
}

/// `buf_map(bp)`: gives a busy buffer a mapping: a fresh slot of the arena, or the slot of the
/// least recently used idle buffer, which loses its own.
pub fn buf_map(bp: &'static Buf) {
    splassert(IPL_BIO, "buf_map");

    if bp.b_data.get().is_null() {
        // First, just use the pre-allocated space until we run out.
        let va = if BUF_KVA_START.load(Ordering::Relaxed) < BUF_KVA_END.load(Ordering::Relaxed) {
            let va = BUF_KVA_START.fetch_add(MAXPHYS, Ordering::Relaxed);
            BCSTATS.kvaslots_avail.fetch_sub(1, Ordering::Relaxed);
            va
        } else {
            // Find some buffer we can steal the space from.
            let cur = curproc().map_or(ptr::null(), ptr::from_ref);
            let mut vbp = BUF_VALIST.0.first();
            while (cur != syncerproc()
                && cur != cleanerproc()
                && BCSTATS.kvaslots_avail.load(Ordering::Relaxed) <= RESERVE_SLOTS)
                || vbp.is_none()
            {
                BUF_NEEDVA.fetch_add(1, Ordering::Relaxed);
                let _ = tsleep_nsec(ptr::from_ref(&BUF_NEEDVA), PRIBIO, "buf_needva", INFSLP);
                vbp = BUF_VALIST.0.first();
            }
            match vbp {
                Some(vbp) => buf_unmap(vbp),
                None => panic(format_args!("buf_map: no idle buffer")),
            }
        };

        let Some(pobj) = bp.pobj() else {
            panic(format_args!("buf_map: buffer {:p} has no pages", bp));
        };
        let mut first = None;
        for i in 0..atop(bp.b_bufsize.get() as usize) {
            let Some(pg) = uvm_pagelookup(pobj, bp.b_poffs.get() + ptoa(i) as i64) else {
                panic(format_args!("buf_map: page {i} of {:p} is gone", bp));
            };
            first.get_or_insert(pg);

            // SAFETY: `va` is a slot of the buffer arena that this buffer now owns, and `pg` a
            // page of the buffer's object.
            unsafe {
                pmap_kenter_pa(
                    Vaddr::new(va + ptoa(i)),
                    vm_page_to_phys(pg),
                    PROT_READ | PROT_WRITE,
                )
            };
        }
        pmap_update(pmap_kernel());
        let data = match first {
            // No MMU: the pages are contiguous (see the module's deviations).
            Some(pg) if nommu() => pmap_map_direct(pg).as_usize(),
            _ => va,
        };
        bp.b_data.set(data as *mut u8);
    } else {
        // SAFETY: an idle mapped buffer is on `buf_valist` (`buf_release`).
        unsafe { BUF_VALIST.0.remove(bp) };
        BCSTATS.kvaslots_avail.fetch_sub(1, Ordering::Relaxed);
    }

    BCSTATS.busymapped.fetch_add(1, Ordering::Relaxed);
}

/// `buf_release`: clears `B_BUSY` and allows the buffer to become unmapped.
pub fn buf_release(bp: &'static Buf) {
    kassert!(bp.isset(B_BUSY));
    splassert(IPL_BIO, "buf_release");

    if !bp.b_data.get().is_null() {
        BCSTATS.busymapped.fetch_sub(1, Ordering::Relaxed);
        // SAFETY: a busy buffer is on no `buf_valist` position; it stays in its pool item
        // while linked (`buf_unmap` frees a released one only after unlinking it).
        unsafe { BUF_VALIST.0.insert_tail(bp) };
        BCSTATS.kvaslots_avail.fetch_add(1, Ordering::Relaxed);
        if BUF_NEEDVA.load(Ordering::Relaxed) != 0 {
            BUF_NEEDVA.store(0, Ordering::Relaxed);
            wakeup(ptr::from_ref(&BUF_NEEDVA));
        }
    }
    bp.clr(B_BUSY);
}

/// Deallocate all memory resources for this buffer. We need to be careful to not drop kvm
/// since we have no way to reclaim it. So, if the buffer has kvm, we need to free it later. We
/// put it on the front of the freelist just so it gets picked up faster.
///
/// Also, lots of assertions count on bp->b_data being NULL, so we set it temporarily to NULL.
///
/// Return non-zero if we take care of the freeing later.
pub fn buf_dealloc_mem(bp: &'static Buf) -> i32 {
    splassert(IPL_BIO, "buf_dealloc_mem");

    let data = bp.b_data.get();
    bp.b_data.set(ptr::null_mut());

    if !data.is_null() {
        if bp.isset(B_BUSY) {
            BCSTATS.busymapped.fetch_sub(1, Ordering::Relaxed);
        }
        // SAFETY: `data` is the buffer's slot, mapped by `buf_map` over `b_bufsize` bytes; the
        // buffer is going away.
        unsafe {
            pmap_kremove(
                Vaddr::new(data as usize),
                Vsize::new(bp.b_bufsize.get() as usize),
            )
        };
        pmap_update(pmap_kernel());
    }

    if !bp.b_pobj.get().is_null() {
        buf_free_pages(bp);
    }

    if data.is_null() {
        return 0;
    }

    bp.b_data.set(data);
    if !bp.isset(B_BUSY) {
        // XXX - need better test
        // SAFETY: an idle mapped buffer is on `buf_valist`.
        unsafe { BUF_VALIST.0.remove(bp) };
        BCSTATS.kvaslots_avail.fetch_sub(1, Ordering::Relaxed);
    } else {
        bp.clr(B_BUSY);
        if BUF_NEEDVA.load(Ordering::Relaxed) != 0 {
            BUF_NEEDVA.store(0, Ordering::Relaxed);
            wakeup(ptr::from_ref(&BUF_NEEDVA));
        }
    }
    bp.set(B_RELEASED);
    // SAFETY: the buffer was just taken off `buf_valist` or was busy (on no position); it
    // stays in its pool item until `buf_unmap` frees it.
    unsafe { BUF_VALIST.0.insert_head(bp) };
    BCSTATS.kvaslots_avail.fetch_add(1, Ordering::Relaxed);

    1
}

/// Only used by bread_cluster.
pub fn buf_fix_mapping(bp: &'static Buf, newsize: usize) {
    let va = bp.b_data.get() as usize;

    let bufsize = bp.b_bufsize.get() as usize;
    if newsize < bufsize {
        // SAFETY: the tail of the buffer's own mapping, which the other buffers of the cluster
        // no longer reach through it.
        unsafe { pmap_kremove(Vaddr::new(va + newsize), Vsize::new(bufsize - newsize)) };
        pmap_update(pmap_kernel());
        // Note: the size we lost is actually with the other buffers read in by bread_cluster
        bp.b_bufsize.set(newsize as i64);
    }
}

/// `buf_unmap(bp)`: takes an idle buffer's mapping away and returns its slot (for internal
/// use only).
pub fn buf_unmap(bp: &'static Buf) -> usize {
    kassert!(!bp.isset(B_BUSY));
    kassert!(!bp.b_data.get().is_null());
    splassert(IPL_BIO, "buf_unmap");

    // SAFETY: an idle mapped buffer is on `buf_valist`.
    unsafe { BUF_VALIST.0.remove(bp) };
    BCSTATS.kvaslots_avail.fetch_sub(1, Ordering::Relaxed);
    let data = bp.b_data.get();
    bp.b_data.set(ptr::null_mut());
    // SAFETY: the buffer's mapping, which nothing uses: the buffer is idle.
    unsafe {
        pmap_kremove(
            Vaddr::new(data as usize),
            Vsize::new(bp.b_bufsize.get() as usize),
        )
    };
    pmap_update(pmap_kernel());

    if bp.isset(B_RELEASED) {
        pool_put(&BUFPOOL, ptr::NonNull::from(bp).cast());
    }

    data as usize
}

/// Always allocates in dma-reachable memory
pub fn buf_alloc_pages(bp: &'static Buf, size: usize) {
    kassert!(size == round_page(size));
    kassert!(bp.b_pobj.get().is_null());
    kassert!(bp.b_data.get().is_null());
    splassert(IPL_BIO, "buf_alloc_pages");

    uvm_obj_init(&bp.b_uobj, Some(&BUFCACHE_PAGER), 1);

    // Attempt to allocate with NOWAIT. if we can't, then throw away some clean pages and try
    // again. Finally, if that fails, do a WAITOK allocation so the page daemon can find memory
    // for us.
    let npages = atop(size) as u64;
    let mut r;
    loop {
        r = uvm_pagealloc_multi(&bp.b_uobj, 0, size, UVM_PLA_NOWAIT | UVM_PLA_NOWAKE);
        if r.is_ok() {
            break;
        }
        if bufbackoff(dma_constraint(), npages as i64) < npages {
            break;
        }
    }
    if r.is_err() {
        r = uvm_pagealloc_multi(&bp.b_uobj, 0, size, UVM_PLA_WAITOK);
    }
    // should not happen
    if r.is_err() {
        panic(format_args!(
            "uvm_pagealloc_multi unable to allocate an buf_object of size {size}"
        ));
    }

    BCSTATS
        .numbufpages
        .fetch_add(npages as i64, Ordering::Relaxed);
    bp.b_pobj.set(&bp.b_uobj);
    bp.b_poffs.set(0);
    bp.b_bufsize.set(size as i64);
}

/// `buf_free_pages(bp)`: frees the buffer's pages (it has no mapping).
pub fn buf_free_pages(bp: &'static Buf) {
    kassert!(bp.b_data.get().is_null());
    splassert(IPL_BIO, "buf_free_pages");

    let Some(uobj) = bp.pobj() else {
        panic(format_args!("buf_free_pages: buffer {:p} has no pages", bp));
    };

    let off = bp.b_poffs.get();
    bp.b_pobj.set(ptr::null());
    bp.b_poffs.set(0);

    for i in 0..atop(bp.b_bufsize.get() as usize) {
        let Some(pg) = uvm_pagelookup(uobj, off + ptoa(i) as i64) else {
            panic(format_args!("buf_free_pages: page {i} of {:p} is gone", bp));
        };
        kassert!(pg.wire_count.get() == 1);
        pg.wire_count.set(0);
        BCSTATS.numbufpages.fetch_sub(1, Ordering::Relaxed);
    }

    // XXX refactor to do this without splbio later
    uvm_obj_free(uobj);
}
/* </CODE> */
