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
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net) at
 * Carlstedt Research & Technology.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net) at
 * Carlstedt Research & Technology.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! USB DMA memory allocation: `<dev/usb/usb_mem.h>` and `dev/usb/usb_mem.c`.
//!
//! Upstream: sys/dev/usb/usb_mem.h @ 3ce1f3f79392, sys/dev/usb/usb_mem.c @ 3ce1f3f79392
//!
//! We need to allocate a lot of small (many 8 byte, some larger) memory blocks that can be
//! used for DMA. Using the bus_dma routines directly would incur large overheads in space
//! and time. Requests of up to `USB_MEM_SMALL` bytes get a fragment of a shared block of
//! `USB_MEM_BLOCK` bytes; larger ones a block of their own, rounded up to `USB_MEM_BLOCK`.
//! Blocks are never given back to `bus_dma`: freed ones wait on a free list for the next
//! request of their tag, size, alignment and flags.
//!
//! ## Deviations
//! - The header and the file share this module. `DMAADDR(dma, o)` and `KERNADDR(dma, o)` are
//!   [`dmaaddr`] and [`kernaddr`].
//! - A block's members are written once, when it is made, so the block is a plain structure
//!   handed out as `&'static` (blocks live forever); only its `frags` pointer and free-list
//!   link are `Cell`s.
//! - The free lists are protected by `splusb()` and the kernel lock, as in C.
//! - The size computation of a large request is [`usb_mem_round`], shared with the tests.
//! - `usb_block_real_freemem` is under `#if 0` in C and is not ported.
//! - `USB_DEBUG` is not in GENERIC: the `DPRINTF`s are absent.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::dev::usb::usbdi::{USBD_NOMEM, USBD_NORMAL_COMPLETION, UsbdStatus, splusb};
use crate::dev::usb::usbdivar::{USB_DMA_COHERENT, UsbDma, UsbdBus};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BUS_DMA_64BIT, BUS_DMA_COHERENT, BUS_DMA_NOWAIT, BusAddr, BusDmaSegment, BusDmaTag, BusDmamap,
    BusSize, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load, bus_dmamap_sync,
    bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap,
};
use crate::machine::intr::splx;
use crate::queue_adapter;
use crate::sys::malloc::{M_NOWAIT, M_USB};
use crate::sys::queue::{ListEntry, ListHead};

/// `USB_MEM_SMALL`: the fragment size.
pub const USB_MEM_SMALL: usize = 64;
/// `USB_MEM_CHUNKS`: fragments per shared block.
pub const USB_MEM_CHUNKS: usize = 64;
/// `USB_MEM_BLOCK`.
pub const USB_MEM_BLOCK: usize = USB_MEM_SMALL * USB_MEM_CHUNKS;

/// `struct usb_dma_block`: a `bus_dma` allocation, mapped and loaded.
pub struct UsbDmaBlock {
    /// `tag`.
    pub tag: BusDmaTag,
    /// `map`.
    pub map: &'static BusDmamap,
    /// `kaddr`: the kernel mapping.
    pub kaddr: NonNull<u8>,
    /// `segs`.
    pub segs: [BusDmaSegment; 1],
    /// `nsegs`.
    pub nsegs: i32,
    /// `flags`: `BUS_DMA_COHERENT`, `BUS_DMA_64BIT` (with the bus's `dmaflags`).
    pub flags: i32,
    /// `size`.
    pub size: usize,
    /// `align`.
    pub align: usize,
    /// `frags`: `USB_MEM_CHUNKS` fragments when the block is shared, NULL otherwise.
    pub frags: Cell<*mut UsbFragDma>,
    /// `next`: on `usb_blk_freelist`.
    pub next: ListEntry<UsbDmaBlock>,
}

/// `struct usb_frag_dma`: a `USB_MEM_SMALL` piece of a shared block.
pub struct UsbFragDma {
    /// `block`.
    pub block: &'static UsbDmaBlock,
    /// `offs`.
    pub offs: u32,
    /// `next`: on `usb_frag_freelist`.
    pub next: ListEntry<UsbFragDma>,
}

queue_adapter!(
    /// `LIST_ENTRY(usb_dma_block) next`.
    pub UsbDmaBlockList: UsbDmaBlock, next => ListEntry<UsbDmaBlock>
);

queue_adapter!(
    /// `LIST_ENTRY(usb_frag_dma) next`.
    pub UsbFragDmaList: UsbFragDma, next => ListEntry<UsbFragDma>
);

/// `LIST_HEAD(, usb_dma_block)`, made `Sync`.
struct BlkFreelist(ListHead<UsbDmaBlockList>);
// SAFETY: changed only at `splusb()` under the kernel lock (every USB path holds it).
unsafe impl Sync for BlkFreelist {}

/// `LIST_HEAD(, usb_frag_dma)`, made `Sync`.
struct FragFreelist(ListHead<UsbFragDmaList>);
// SAFETY: as for `BlkFreelist`.
unsafe impl Sync for FragFreelist {}

/// `usb_blk_freelist`.
static USB_BLK_FREELIST: BlkFreelist = BlkFreelist(ListHead::new());
/// `usb_blk_nfree`.
static USB_BLK_NFREE: AtomicI32 = AtomicI32::new(0);
/// `usb_frag_freelist`: XXX should have different free list for different tags (for speed).
static USB_FRAG_FREELIST: FragFreelist = FragFreelist(ListHead::new());

impl UsbDma {
    /// `dma->block`. Panics on a piece `usb_allocmem` did not fill.
    pub fn block(&self) -> &'static UsbDmaBlock {
        match self.block.get() {
            Some(b) => b,
            None => panic(format_args!("usb_dma without a block")),
        }
    }
}

/// `DMAADDR(dma, o)`: the bus address of byte `o` of the piece.
pub fn dmaaddr(dma: &UsbDma, o: usize) -> BusAddr {
    let b = dma.block();
    let seg0 = b.map.dm_segs().first().map(|s| s.get()).unwrap_or_default();
    seg0.ds_addr + dma.offs.get() as usize + o
}

/// `KERNADDR(dma, o)`: the kernel address of byte `o` of the piece.
pub fn kernaddr(dma: &UsbDma, o: usize) -> *mut u8 {
    let b = dma.block();
    b.kaddr
        .as_ptr()
        .wrapping_add(dma.offs.get() as usize)
        .wrapping_add(o)
}

/// The size `usb_allocmem` asks for: `None` for a request that fits a fragment, or the size
/// of the block of its own, rounded up to `USB_MEM_BLOCK`.
pub const fn usb_mem_round(size: usize, align: usize) -> Option<usize> {
    if size > USB_MEM_SMALL || align > USB_MEM_SMALL {
        Some((size + USB_MEM_BLOCK - 1) & !(USB_MEM_BLOCK - 1))
    } else {
        None
    }
}

/// `usb_block_allocmem`: a block of `size` bytes from the free list, or a new one.
pub fn usb_block_allocmem(
    tag: BusDmaTag,
    size: usize,
    align: usize,
    flags: i32,
) -> Result<&'static UsbDmaBlock, UsbdStatus> {
    let s = splusb();
    // First check the free list.
    for p in USB_BLK_FREELIST.0.iter() {
        if ptr_eq_tag(p.tag, tag) && p.size >= size && p.align >= align && p.flags == flags {
            // SAFETY: `p` is on the free list (found by iterating it); at splusb().
            unsafe { ListHead::<UsbDmaBlockList>::remove(p) };
            USB_BLK_NFREE.fetch_sub(1, Ordering::Relaxed);
            splx(s);
            // SAFETY: blocks are never freed (see the module's doc).
            return Ok(unsafe { &*ptr::from_ref(p) });
        }
    }
    splx(s);

    let Some(mem) = malloc(size_of::<UsbDmaBlock>(), M_USB, M_NOWAIT) else {
        return Err(USBD_NOMEM);
    };
    let pblk = mem.cast::<UsbDmaBlock>();

    let mut segs = [BusDmaSegment::default(); 1];
    'free0: {
        let Ok(nsegs) = bus_dmamem_alloc(
            tag,
            size,
            align,
            0,
            &mut segs,
            BUS_DMA_NOWAIT | (flags & BUS_DMA_64BIT),
        ) else {
            break 'free0;
        };

        'free1: {
            let Ok(kaddr) = bus_dmamem_map(
                tag,
                &mut segs[..nsegs],
                size,
                BUS_DMA_NOWAIT | (flags & BUS_DMA_COHERENT),
            ) else {
                break 'free1;
            };

            'unmap: {
                let Ok(map) = bus_dmamap_create(
                    tag,
                    size,
                    1,
                    size,
                    0,
                    BUS_DMA_NOWAIT | (flags & BUS_DMA_64BIT),
                ) else {
                    break 'unmap;
                };

                // SAFETY: `kaddr` maps `size` bytes this block owns forever.
                if unsafe { bus_dmamap_load(tag, map, kaddr.as_ptr(), size, None, BUS_DMA_NOWAIT) }
                    .is_err()
                {
                    // destroy:
                    // SAFETY: the map was just created and is not used again.
                    unsafe { bus_dmamap_destroy(tag, NonNull::from(map)) };
                    break 'unmap;
                }

                // SAFETY: a fresh allocation of the block's size and alignment, written once
                // before anybody sees it.
                unsafe {
                    pblk.write(UsbDmaBlock {
                        tag,
                        map,
                        kaddr,
                        segs,
                        nsegs: nsegs as i32,
                        flags,
                        size,
                        align,
                        frags: Cell::new(ptr::null_mut()),
                        next: ListEntry::new(),
                    })
                };
                // SAFETY: just written; blocks are never freed.
                return Ok(unsafe { &*pblk.as_ptr() });
            }
            // unmap:
            // SAFETY: the mapping made above, not used again.
            unsafe { bus_dmamem_unmap(tag, kaddr, size) };
        }
        // free1:
        // SAFETY: the segments allocated above, unmapped and not used again.
        unsafe { bus_dmamem_free(tag, &segs[..nsegs]) };
    }
    // free0:
    free(mem, M_USB, size_of::<UsbDmaBlock>());
    Err(USBD_NOMEM)
}

/// `p->tag == tag`: two DMA tags are the same tag.
fn ptr_eq_tag(a: BusDmaTag, b: BusDmaTag) -> bool {
    // A tag is a `&'static` table (or a zero-sized stand-in on the host); its identity is
    // its bytes.
    let pa = ptr::from_ref(&a).cast::<u8>();
    let pb = ptr::from_ref(&b).cast::<u8>();
    // SAFETY: both are initialised values of the same `Copy` type, read as bytes.
    unsafe {
        core::slice::from_raw_parts(pa, size_of::<BusDmaTag>())
            == core::slice::from_raw_parts(pb, size_of::<BusDmaTag>())
    }
}

/// `usb_block_freemem`: do not free the memory unconditionally since we might be called from
/// an interrupt context and that is BAD. XXX when should we really free?
pub fn usb_block_freemem(p: &'static UsbDmaBlock) {
    let s = splusb();
    // SAFETY: a block in use is on no list; it lives forever; at splusb().
    unsafe { USB_BLK_FREELIST.0.insert_head(p) };
    USB_BLK_NFREE.fetch_add(1, Ordering::Relaxed);
    splx(s);
}

/// `usb_allocmem`: `size` bytes of DMA memory for `bus`, aligned to `align`, into `p`.
pub fn usb_allocmem(
    bus: &UsbdBus,
    size: usize,
    align: usize,
    flags: i32,
    p: &UsbDma,
) -> UsbdStatus {
    let tag = bus.dmatag();

    let mut flags = if flags & USB_DMA_COHERENT != 0 {
        BUS_DMA_COHERENT
    } else {
        0
    };
    flags |= bus.dmaflags.get();

    // If the request is large then just use a full block.
    if let Some(size) = usb_mem_round(size, align) {
        return match usb_block_allocmem(tag, size, align, flags) {
            Ok(b) => {
                b.frags.set(ptr::null_mut());
                p.block.set(Some(b));
                p.offs.set(0);
                USBD_NORMAL_COMPLETION
            }
            Err(e) => e,
        };
    }

    let s = splusb();
    // Check for free fragments.
    let mut f = USB_FRAG_FREELIST
        .0
        .iter()
        .find(|f| ptr_eq_tag(f.block.tag, tag) && f.block.flags == flags);
    if f.is_none() {
        let b = match usb_block_allocmem(tag, USB_MEM_BLOCK, USB_MEM_SMALL, flags) {
            Ok(b) => b,
            Err(e) => {
                splx(s);
                return e;
            }
        };
        let Some(frags) = mallocarray(USB_MEM_CHUNKS, size_of::<UsbFragDma>(), M_USB, M_NOWAIT)
        else {
            splx(s);
            usb_block_freemem(b);
            return USBD_NOMEM;
        };
        let frags = frags.cast::<UsbFragDma>();
        b.frags.set(frags.as_ptr());
        for i in 0..USB_MEM_CHUNKS {
            // SAFETY: `USB_MEM_CHUNKS` slots were allocated for fragments; each is written
            // once before it is linked, and lives as long as its block (forever).
            unsafe {
                let fp = frags.as_ptr().add(i);
                fp.write(UsbFragDma {
                    block: b,
                    offs: (USB_MEM_SMALL * i) as u32,
                    next: ListEntry::new(),
                });
                USB_FRAG_FREELIST.0.insert_head(&*fp);
            }
        }
        f = USB_FRAG_FREELIST.0.first();
    }
    let Some(f) = f else {
        splx(s);
        return USBD_NOMEM;
    };
    p.block.set(Some(f.block));
    p.offs.set(f.offs);
    // SAFETY: `f` is on the fragment free list (found there); at splusb().
    unsafe { ListHead::<UsbFragDmaList>::remove(f) };
    splx(s);
    USBD_NORMAL_COMPLETION
}

/// `usb_freemem`: gives `p` back (its fragment to the fragment list, a whole block to the
/// block list).
pub fn usb_freemem(_bus: &UsbdBus, p: &UsbDma) {
    let b = p.block();
    let frags = b.frags.get();
    if frags.is_null() {
        usb_block_freemem(b);
        return;
    }
    let s = splusb();
    // SAFETY: a shared block has `USB_MEM_CHUNKS` fragments at `frags` (usb_allocmem), and
    // `offs / USB_MEM_SMALL` is the index of the one this piece came from, which is off the
    // free list while in use; at splusb().
    unsafe {
        let f = &*frags.add(p.offs.get() as usize / USB_MEM_SMALL);
        USB_FRAG_FREELIST.0.insert_head(f);
    }
    splx(s);
}

/// `usb_syncmem`: `bus_dmamap_sync` over `len` bytes at `offset` in the piece.
pub fn usb_syncmem(p: &UsbDma, offset: BusAddr, len: BusSize, ops: i32) {
    let b = p.block();
    bus_dmamap_sync(b.tag, b.map, p.offs.get() as usize + offset, len, ops);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_requests_share_blocks() {
        assert_eq!(usb_mem_round(0, 0), None);
        assert_eq!(usb_mem_round(8, 0), None);
        assert_eq!(usb_mem_round(USB_MEM_SMALL, USB_MEM_SMALL), None);
    }

    #[test]
    fn large_requests_round_up_to_a_block() {
        assert_eq!(USB_MEM_BLOCK, 4096);
        assert_eq!(usb_mem_round(65, 0), Some(4096));
        assert_eq!(usb_mem_round(4096, 0), Some(4096));
        assert_eq!(usb_mem_round(4097, 0), Some(8192));
        // A small request with a large alignment gets a block of its own.
        assert_eq!(usb_mem_round(8, 128), Some(4096));
    }
}
/* </TESTS> */
