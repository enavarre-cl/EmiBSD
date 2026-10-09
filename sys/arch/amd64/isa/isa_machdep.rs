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

/*-
 * Copyright (c) 1996, 1997 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center.
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

/*-
 * Copyright (c) 1993, 1994, 1996, 1997
 *	Charles M. Hannum.  All rights reserved.
 * Copyright (c) 1991 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz.
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
 *	@(#)isa.c	7.2 (Berkeley) 5/13/91
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 ISA machine-dependent code: `arch/amd64/isa/isa_machdep.c`.
//!
//! Upstream: sys/arch/amd64/isa/isa_machdep.c @ 3ce1f3f79392
//!
//! Milestone M4 ports the interrupt side: `isa_intr_alloc`, `isa_intr_check`,
//! `isa_intr_establish`, `isa_intr_disestablish` and `isa_attach_hook`. M13 adds the
//! `NIOAPIC > 0` pin lookup of `isa_intr_establish` (`mp_isa_bus`, set by `acpimadt`). M16a
//! adds the `NISADMA > 0` part (isadma(4) for fd(4)): `isa_bus_dma_tag`, whose maps bounce
//! transfers through pages below 16 MB (`_isa_bus_dma*`, `_isa_dma_check_buffer`,
//! `_isa_dma_alloc_bouncebuf`, `_isa_dma_free_bouncebuf`) over the common `bus_dma.rs`
//! functions; `mainbus.rs` hands it to the ISA bus.
//!
//! ## Deviations
//! - `_isa_dma_check_buffer` answers `EINVAL` (bounce) for a page `pmap_extract` does not
//!   find, where the C goes on with the previous page's address; `_isa_bus_dmamap_destroy`
//!   frees the cookie with the size it was allocated with (the C passes 0).
//! - `ISA_DMA_STATS` is not defined, as in GENERIC: the statistics macros do nothing. The
//!   `DEBUG` checks of `_isa_bus_dmamap_sync` are not configured; its `DIAGNOSTIC` one is
//!   under feature `diagnostic`.
//! - `isa_intr_alloc` returns the IRQ as `Option` where the C returns 0/1 with an out
//!   parameter.
//! - `isa_attach_hook` takes no devices (`machine::isa_machdep` passes it none: it uses none)
//!   and sets `mainbus.rs`'s `isa_has_been_seen`, as the C.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use crate::arch::amd64::amd64::bus_dma::{
    _bus_dmamap_create, _bus_dmamap_destroy, _bus_dmamap_load, _bus_dmamap_sync,
    _bus_dmamap_unload, _bus_dmamem_alloc_range, _bus_dmamem_free, _bus_dmamem_map,
    _bus_dmamem_mmap, _bus_dmamem_unmap,
};
use crate::arch::amd64::amd64::i8259::I8259_PIC;
use crate::arch::amd64::amd64::intr::{intr_disestablish, intr_establish};
use crate::arch::amd64::amd64::machdep::AVAIL_END;
use crate::arch::amd64::amd64::mainbus::{ISA_HAS_BEEN_SEEN, mp_busses, mp_isa_bus};
use crate::arch::amd64::include::bus::{
    BUS_DMA_ALLOCNOW, BUS_DMA_NOWAIT, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_PREWRITE, BusDmaSegment,
    BusDmaTag, BusDmaTagT, BusDmamap, BusDmamapT,
};
#[cfg(feature = "diagnostic")]
use crate::arch::amd64::include::bus::{BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD};
use crate::arch::amd64::include::i8259::ICU_LEN;
use crate::arch::amd64::include::i82093var::apic_irq_pin;
use crate::arch::amd64::include::intr::{IntrFn, Intrhand};
use crate::arch::amd64::include::intrdefs::{IST_EDGE, IST_LEVEL, IST_NONE, IST_PULSE};
use crate::arch::amd64::include::isa_machdep::{
    ID_HAS_BOUNCE, ID_IS_BOUNCING, ID_MIGHT_NEED_BOUNCE, ISA_DMA_BOUNCE_THRESHOLD, IsaDmaCookie,
};
use crate::arch::amd64::include::pic::Pic;
use crate::dev::isa::isavar::ISABUS_DMA_32BIT;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{BusAddr, BusSize};
use crate::machine::pmap::{pmap_extract, pmap_kernel};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::mbuf::Mbuf;
use crate::sys::param::NBPG;
use crate::sys::proc::Proc;
use crate::sys::types::Vaddr;
use crate::sys::uio::Uio;
use crate::uvm::uvm_param::{round_page, trunc_page};

/// `isa_chipset_tag_t`: unused on amd64.
pub type IsaChipsetTag = *const c_void;

/// `intrtype[ICU_LEN]`: the trigger type recorded per IRQ.
static INTRTYPE: [AtomicI32; ICU_LEN as usize] = [const { AtomicI32::new(0) }; ICU_LEN as usize];
/// `intrlevel[ICU_LEN]`.
static INTRLEVEL: [AtomicI32; ICU_LEN as usize] = [const { AtomicI32::new(0) }; ICU_LEN as usize];
/// `intrhand[ICU_LEN]`: the handler chains the allocator counts.
static INTRHAND: [AtomicPtr<Intrhand>; ICU_LEN as usize] =
    [const { AtomicPtr::new(core::ptr::null_mut()) }; ICU_LEN as usize];

/// `isa_bus_dma_tag`: entry points for ISA DMA. These are mostly wrappers around the generic
/// functions that understand how to deal with bounce buffers, if necessary.
pub static ISA_BUS_DMA_TAG: BusDmaTag = BusDmaTag {
    _cookie: ptr::null_mut(),
    _dmamap_create: _isa_bus_dmamap_create,
    _dmamap_destroy: _isa_bus_dmamap_destroy,
    _dmamap_load: _isa_bus_dmamap_load,
    _dmamap_load_mbuf: _isa_bus_dmamap_load_mbuf,
    _dmamap_load_uio: _isa_bus_dmamap_load_uio,
    _dmamap_load_raw: _isa_bus_dmamap_load_raw,
    _dmamap_unload: _isa_bus_dmamap_unload,
    _dmamap_sync: _isa_bus_dmamap_sync,
    _dmamem_alloc: _isa_bus_dmamem_alloc,
    _dmamem_alloc_range: _bus_dmamem_alloc_range,
    _dmamem_free: _bus_dmamem_free,
    _dmamem_map: _bus_dmamem_map,
    _dmamem_unmap: _bus_dmamem_unmap,
    _dmamem_mmap: _bus_dmamem_mmap,
};

/// `LEGAL_IRQ(x)`.
const fn legal_irq(x: i32) -> bool {
    x >= 0 && x < ICU_LEN && x != 2
}

/// `isa_intr_alloc`: picks an IRQ from `mask` for a `type_` interrupt, the least shared one.
pub fn isa_intr_alloc(_ic: IsaChipsetTag, mask: i32, type_: i32) -> Option<i32> {
    if type_ == IST_NONE {
        panic(format_args!("intr_alloc: bogus type"));
    }

    let mut bestirq = -1;
    let mut count = -1;

    // some interrupts should never be dynamically allocated
    let mut mask = mask & 0xdef8;

    // XXX some interrupts will be used later (6 for fdc, 12 for pms). the right answer is to
    // do "breadth-first" searching of devices.
    mask &= 0xefbf;

    for i in 0..ICU_LEN {
        if !legal_irq(i) || (mask & (1 << i)) == 0 {
            continue;
        }
        let t = INTRTYPE[i as usize].load(Ordering::Relaxed);
        if t == IST_NONE {
            // if nothing's using the irq, just return it
            return Some(i);
        } else if t == IST_EDGE || t == IST_LEVEL {
            if type_ != t {
                continue;
            }
            // if the irq is shareable, count the number of other handlers, and if it's smaller
            // than the last irq like this, remember it
            //
            // XXX We should probably also consider the interrupt level and stick IPL_TTY with
            // other IPL_TTY, etc.
            let mut tmp = 0;
            let mut q = INTRHAND[i as usize].load(Ordering::Relaxed).cast_const();
            // SAFETY: the chain's handlers are established ones, alive until disestablished.
            while let Some(ih) = unsafe { q.as_ref() } {
                q = ih.ih_next.get();
                tmp += 1;
            }
            if bestirq == -1 || count > tmp {
                bestirq = i;
                count = tmp;
            }
        } else if t == IST_PULSE {
            // this just isn't shareable
            continue;
        }
    }

    if bestirq == -1 { None } else { Some(bestirq) }
}

/// `isa_intr_check`: just check to see if an IRQ is available/can be shared. 0 = interrupt
/// not available, 1 = interrupt shareable, 2 = interrupt all to ourself.
pub fn isa_intr_check(_ic: IsaChipsetTag, irq: i32, type_: i32) -> i32 {
    if !legal_irq(irq) || type_ == IST_NONE {
        return 0;
    }

    let t = INTRTYPE[irq as usize].load(Ordering::Relaxed);
    if t == IST_NONE {
        return 2;
    }
    if t == IST_LEVEL {
        if type_ != t {
            return 0;
        }
        return 1;
    }
    if (t == IST_EDGE || t == IST_PULSE) && type_ != IST_NONE {
        return 0;
    }
    1
}

/// `isa_intr_establish`: set up an interrupt handler to start being called. XXX PRONE TO
/// RACE CONDITIONS, UGLY, 'INTERESTING' INSERTION ALGORITHM.
pub fn isa_intr_establish(
    _ic: IsaChipsetTag,
    irq: i32,
    type_: i32,
    level: i32,
    ih_fun: IntrFn,
    ih_arg: *mut c_void,
    ih_what: &'static str,
) -> Option<NonNull<Intrhand>> {
    let mut pic: &'static Pic = &I8259_PIC;
    let mut pin = irq;

    // NIOAPIC > 0
    if mp_busses().is_some() {
        let Some(isa) = mp_isa_bus() else {
            panic(format_args!("no isa bus"));
        };

        if let Some(mip) = isa.intrs().find(|mip| mip.bus_pin == pin)
            && let Some(apic) = mip.ioapic
        {
            pin = apic_irq_pin(mip.ioapic_ih);
            // SAFETY: an attached I/O APIC's pic is initialised before acpimadt maps a pin
            // to it.
            pic = unsafe { apic.pic() };
        }
    }

    let _ = &INTRLEVEL;
    intr_establish(irq, pic, pin, type_, level, None, ih_fun, ih_arg, ih_what)
}

/// `isa_intr_disestablish`: deregister an interrupt handler.
///
/// # Safety
///
/// `arg` must come from `isa_intr_establish` and not be used afterwards.
pub unsafe fn isa_intr_disestablish(_ic: IsaChipsetTag, arg: NonNull<Intrhand>) {
    // SAFETY: the caller's guarantee.
    unsafe { intr_disestablish(arg) };
}

/// `isa_attach_hook`: notify others that might need to know that the ISA bus has now been
/// attached (`isa_has_been_seen`, which `mainbus.c` defines).
pub fn isa_attach_hook() {
    if ISA_HAS_BEEN_SEEN.swap(1, Ordering::Relaxed) != 0 {
        panic(format_args!("isaattach: ISA bus already seen!"));
    }
}

// NISADMA > 0: bus.h dma interface entry points.
//
// ISA_DMA_STATS is not defined: STAT_INCR and STAT_DECR do nothing.

/// The size `_isa_bus_dmamap_create` allocates for a map's cookie: the header, and the
/// bounce segments when the map might bounce.
fn isa_dma_cookie_size(flags: i32, segcnt: i32) -> usize {
    let mut cookiesize = size_of::<IsaDmaCookie>();
    if flags & ID_MIGHT_NEED_BOUNCE != 0 {
        cookiesize += size_of::<BusDmaSegment>() * segcnt as usize;
    }
    cookiesize
}

/// `map->_dm_cookie`: the ISA cookie `_isa_bus_dmamap_create` stashed in a map of this tag.
fn isa_dma_cookie(map: &BusDmamap) -> &IsaDmaCookie {
    let cookie = map._dm_cookie.get().cast::<IsaDmaCookie>();
    if cookie.is_null() {
        panic(format_args!("isa_dma: map without a cookie"));
    }
    // SAFETY: every map of `isa_bus_dma_tag` gets a zeroed cookie from
    // `_isa_bus_dmamap_create`, freed only with the map by `_isa_bus_dmamap_destroy`.
    unsafe { &*cookie }
}

/// `_isa_bus_dmamap_create`: create an ISA DMA map.
pub fn _isa_bus_dmamap_create(
    t: BusDmaTagT,
    size: BusSize,
    nsegments: i32,
    maxsegsz: BusSize,
    boundary: BusSize,
    flags: i32,
) -> Result<BusDmamapT, Errno> {
    // Call common function to create the basic map.
    let map = _bus_dmamap_create(t, size, nsegments, maxsegsz, boundary, flags)?;
    map._dm_cookie.set(ptr::null_mut());

    // ISA only has 24-bits of address space. This means we can't DMA to pages over 16M.
    // In order to DMA to arbitrary buffers, we use "bounce buffers" - pages in memory below
    // the 16M boundary. On DMA reads, DMA happens to the bounce buffers, and is copied into
    // the caller's buffer. On writes, data is copied into the bounce buffer, and the DMA
    // happens from those pages. To software using the DMA mapping interface, this looks
    // simply like a data cache.
    //
    // If we have more than 16M of RAM in the system, we may need bounce buffers. We check
    // and remember that here.
    //
    // There are exceptions, however. VLB devices can do 32-bit DMA, and indicate that here.
    //
    // ...or, there is an opposite case. The most segments a transfer will require is
    // (maxxfer / NBPG) + 1. If the caller can't handle that many segments (e.g. the ISA DMA
    // controller), we may have to bounce it as well.
    let mut cookieflags = 0;
    if (AVAIL_END.load(Ordering::Relaxed) > ISA_DMA_BOUNCE_THRESHOLD
        && flags & ISABUS_DMA_32BIT == 0)
        || (map._dm_size / NBPG) + 1 > map._dm_segcnt as usize
    {
        cookieflags |= ID_MIGHT_NEED_BOUNCE;
    }
    let cookiesize = isa_dma_cookie_size(cookieflags, map._dm_segcnt);

    // Allocate our cookie.
    let mflags = if flags & BUS_DMA_NOWAIT != 0 {
        M_NOWAIT | M_ZERO
    } else {
        M_WAITOK | M_ZERO
    };
    let mut error = Ok(());
    match malloc(cookiesize, M_DEVBUF, mflags) {
        None => error = Err(Errno::ENOMEM),
        Some(cookiestore) => {
            let cookie = cookiestore.cast::<IsaDmaCookie>();
            // SAFETY: a fresh zeroed allocation of at least the header's size, aligned by
            // malloc(9); all-zero is a valid cookie (null pointers, zero counters).
            unsafe { cookie.as_ref() }.id_flags.set(cookieflags);
            map._dm_cookie.set(cookie.as_ptr().cast());

            // Allocate the bounce pages now if the caller wishes us to do so.
            if cookieflags & ID_MIGHT_NEED_BOUNCE != 0 && flags & BUS_DMA_ALLOCNOW != 0 {
                error = _isa_dma_alloc_bouncebuf(t, map, size, flags);
            }
        }
    }

    // out:
    if let Err(e) = error {
        if let Some(cookie) = NonNull::new(map._dm_cookie.get().cast::<u8>()) {
            free(cookie, M_DEVBUF, cookiesize);
        }
        // SAFETY: the map `_bus_dmamap_create` just made, handed to nobody.
        unsafe { _bus_dmamap_destroy(t, NonNull::from(map)) };
        return Err(e);
    }
    Ok(map)
}

/// `_isa_bus_dmamap_destroy`: destroy an ISA DMA map.
///
/// # Safety
///
/// `map` came from [`_isa_bus_dmamap_create`] and is not used afterwards.
pub unsafe fn _isa_bus_dmamap_destroy(t: BusDmaTagT, map: NonNull<BusDmamap>) {
    // SAFETY: the caller's guarantee: a live map of this tag.
    let m = unsafe { map.as_ref() };
    let cookie = isa_dma_cookie(m);

    // Free any bounce pages this map might hold.
    if cookie.id_flags.get() & ID_HAS_BOUNCE != 0 {
        _isa_dma_free_bouncebuf(t, m);
    }

    let cookiesize = isa_dma_cookie_size(cookie.id_flags.get(), m._dm_segcnt);
    free(NonNull::from(cookie).cast(), M_DEVBUF, cookiesize);
    // SAFETY: the caller's guarantee.
    unsafe { _bus_dmamap_destroy(t, map) };
}

/// `_isa_bus_dmamap_load`: load an ISA DMA map with a linear buffer.
///
/// # Safety
///
/// As for `machine::bus::BusDma::bus_dmamap_load`.
pub unsafe fn _isa_bus_dmamap_load(
    t: BusDmaTagT,
    map: &BusDmamap,
    buf: *mut u8,
    buflen: BusSize,
    p: Option<&Proc>,
    flags: i32,
) -> Result<(), Errno> {
    let cookie = isa_dma_cookie(map);

    // Check to see if we might need to bounce the transfer.
    if cookie.id_flags.get() & ID_MIGHT_NEED_BOUNCE == 0 {
        // Just use the generic load function.
        // SAFETY: the caller's contract.
        return unsafe { _bus_dmamap_load(t, map, buf, buflen, p, flags) };
    }

    // Check if all pages are below the bounce threshold. If they are, don't bother
    // bouncing.
    if _isa_dma_check_buffer(buf, buflen, map._dm_segcnt, map._dm_boundary, p).is_ok() {
        // SAFETY: the caller's contract.
        return unsafe { _bus_dmamap_load(t, map, buf, buflen, p, flags) };
    }

    // Allocate bounce pages, if necessary.
    if cookie.id_flags.get() & ID_HAS_BOUNCE == 0 {
        _isa_dma_alloc_bouncebuf(t, map, buflen, flags)?;
    }

    // Cache a pointer to the caller's buffer and load the DMA map with the bounce buffer.
    cookie.id_origbuf.set(buf);
    cookie.id_origbuflen.set(buflen);
    // SAFETY: the bounce buffer is the map's own, mapped until `_isa_dma_free_bouncebuf`,
    // which only an unload or a destroy calls.
    let error = unsafe { _bus_dmamap_load(t, map, cookie.id_bouncebuf.get(), buflen, p, flags) };

    if error.is_err() {
        // Free the bounce pages, unless our resources are reserved for our exclusive use.
        if map._dm_flags & BUS_DMA_ALLOCNOW == 0 {
            _isa_dma_free_bouncebuf(t, map);
        }
    }

    // ...so _isa_bus_dmamap_sync() knows we're bouncing
    cookie.id_flags.set(cookie.id_flags.get() | ID_IS_BOUNCING);

    error
}

/// `_isa_bus_dmamap_load_mbuf`: like `_isa_bus_dmamap_load()`, but for mbufs.
///
/// # Safety
///
/// None needed: it panics, as in the C.
pub unsafe fn _isa_bus_dmamap_load_mbuf(
    _t: BusDmaTagT,
    _map: &BusDmamap,
    _m: &Mbuf,
    _flags: i32,
) -> Result<(), Errno> {
    panic(format_args!("_isa_bus_dmamap_load_mbuf: not implemented"));
}

/// `_isa_bus_dmamap_load_uio`: like `_isa_bus_dmamap_load()`, but for uios.
///
/// # Safety
///
/// None needed: it panics, as in the C.
pub unsafe fn _isa_bus_dmamap_load_uio(
    _t: BusDmaTagT,
    _map: &BusDmamap,
    _uio: &Uio<'_>,
    _flags: i32,
) -> Result<(), Errno> {
    panic(format_args!("_isa_bus_dmamap_load_uio: not implemented"));
}

/// `_isa_bus_dmamap_load_raw`: like `_isa_bus_dmamap_load()`, but for raw memory allocated
/// with `bus_dmamem_alloc()`.
///
/// # Safety
///
/// None needed: it panics, as in the C.
pub unsafe fn _isa_bus_dmamap_load_raw(
    _t: BusDmaTagT,
    _map: &BusDmamap,
    _segs: &[BusDmaSegment],
    _size: BusSize,
    _flags: i32,
) -> Result<(), Errno> {
    panic(format_args!("_isa_bus_dmamap_load_raw: not implemented"));
}

/// `_isa_bus_dmamap_unload`: unload an ISA DMA map.
pub fn _isa_bus_dmamap_unload(t: BusDmaTagT, map: &BusDmamap) {
    let cookie = isa_dma_cookie(map);

    // If we have bounce pages, free them, unless they're reserved for our exclusive use.
    if cookie.id_flags.get() & ID_HAS_BOUNCE != 0 && map._dm_flags & BUS_DMA_ALLOCNOW == 0 {
        _isa_dma_free_bouncebuf(t, map);
    }

    cookie.id_flags.set(cookie.id_flags.get() & !ID_IS_BOUNCING);

    // Do the generic bits of the unload.
    _bus_dmamap_unload(t, map);
}

/// `_isa_bus_dmamap_sync`: synchronize an ISA DMA map.
pub fn _isa_bus_dmamap_sync(
    t: BusDmaTagT,
    map: &BusDmamap,
    offset: BusAddr,
    len: BusSize,
    op: i32,
) {
    let cookie = isa_dma_cookie(map);

    // DEBUG (the offset and length checks): not configured.
    #[cfg(feature = "diagnostic")]
    if op & (BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE) != 0
        && op & (BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE) != 0
    {
        panic(format_args!("_isa_bus_dmamap_sync: mix PRE and POST"));
    }

    // PREREAD and POSTWRITE are no-ops
    if op & BUS_DMASYNC_PREWRITE != 0 {
        // If we're bouncing this transfer, copy the caller's buffer to the bounce buffer.
        if cookie.id_flags.get() & ID_IS_BOUNCING != 0 {
            // SAFETY: while bouncing, both buffers hold the loaded length (the caller's
            // `bus_dmamap_load` contract and the bounce buffer of at least that size), and
            // `offset + len` lies within it, as `bus_dmamap_sync(9)` requires.
            unsafe {
                ptr::copy_nonoverlapping(
                    cookie.id_origbuf.get().add(offset),
                    cookie.id_bouncebuf.get().add(offset),
                    len,
                );
            }
        }
    }

    _bus_dmamap_sync(t, map, offset, len, op);

    if op & BUS_DMASYNC_POSTREAD != 0 {
        // If we're bouncing this transfer, copy the bounce buffer to the caller's buffer.
        if cookie.id_flags.get() & ID_IS_BOUNCING != 0 {
            // SAFETY: as for PREWRITE above.
            unsafe {
                ptr::copy_nonoverlapping(
                    cookie.id_bouncebuf.get().add(offset),
                    cookie.id_origbuf.get().add(offset),
                    len,
                );
            }
        }
    }
}

/// `_isa_bus_dmamem_alloc`: allocate memory safe for ISA DMA.
pub fn _isa_bus_dmamem_alloc(
    t: BusDmaTagT,
    size: BusSize,
    alignment: BusSize,
    boundary: BusSize,
    segs: &mut [BusDmaSegment],
    flags: i32,
) -> Result<usize, Errno> {
    // Try in ISA addressable region first
    if let Ok(rsegs) = _bus_dmamem_alloc_range(
        t,
        size,
        alignment,
        boundary,
        segs,
        flags,
        0,
        ISA_DMA_BOUNCE_THRESHOLD,
    ) {
        return Ok(rsegs);
    }

    // Otherwise try anywhere (we'll bounce later)
    _bus_dmamem_alloc_range(t, size, alignment, boundary, segs, flags, 0, usize::MAX)
}

// ISA DMA utility functions

/// `_isa_dma_check_buffer`: `Ok` if all pages in the passed buffer lie within the DMA'able
/// range RAM.
pub fn _isa_dma_check_buffer(
    buf: *mut u8,
    buflen: BusSize,
    segcnt: i32,
    boundary: BusSize,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    let mut vaddr = buf as usize;
    let pagemask = !boundary.wrapping_sub(1);

    let endva = round_page(vaddr + buflen);

    let mut nsegs = 1;
    let mut lastpa = 0usize;

    let pmap = match p {
        Some(p) => p.vmspace().vm_map.pmap(),
        None => pmap_kernel(),
    };

    while vaddr < endva {
        // Get physical address for this segment.
        let Some(pa) = pmap_extract(pmap, Vaddr::new(vaddr)) else {
            // See the module's deviations: an unmapped page is not DMA'able.
            return Err(Errno::EINVAL);
        };
        let pa = trunc_page(pa.as_usize());

        // Is it below the DMA'able threshold?
        if pa > ISA_DMA_BOUNCE_THRESHOLD {
            return Err(Errno::EINVAL);
        }

        if lastpa != 0 {
            // Check excessive segment count.
            if lastpa + NBPG != pa {
                nsegs += 1;
                if nsegs > segcnt {
                    return Err(Errno::EFBIG);
                }
            }

            // Check boundary restriction.
            if boundary != 0 && (lastpa ^ pa) & pagemask != 0 {
                return Err(Errno::EINVAL);
            }
        }
        lastpa = pa;
        vaddr += NBPG;
    }

    Ok(())
}

/// `_isa_dma_alloc_bouncebuf`: bounce pages below 16M for a map, mapped in the kernel.
pub fn _isa_dma_alloc_bouncebuf(
    t: BusDmaTagT,
    map: &BusDmamap,
    size: BusSize,
    flags: i32,
) -> Result<(), Errno> {
    let cookie = isa_dma_cookie(map);
    let segcnt = map._dm_segcnt as usize;
    // SAFETY: a bouncing map's cookie has `_dm_segcnt` segments after it
    // (`ID_MIGHT_NEED_BOUNCE`, the only caller's condition), and the map's owner is the
    // only one running its functions.
    let segs = unsafe { cookie.id_bouncesegs(segcnt) };

    cookie.id_bouncebuflen.set(round_page(size));
    let mut error = _bus_dmamem_alloc_range(
        t,
        cookie.id_bouncebuflen.get(),
        NBPG,
        map._dm_boundary,
        segs,
        flags,
        0,
        ISA_DMA_BOUNCE_THRESHOLD,
    )
    .map(|n| cookie.id_nbouncesegs.set(n as i32));
    if error.is_ok() {
        let n = cookie.id_nbouncesegs.get() as usize;
        error = _bus_dmamem_map(t, &mut segs[..n], cookie.id_bouncebuflen.get(), flags)
            .map(|kva| cookie.id_bouncebuf.set(kva.as_ptr()));
    }

    // out:
    if error.is_err() {
        let n = cookie.id_nbouncesegs.get() as usize;
        // SAFETY: the segments `_bus_dmamem_alloc_range` returned (none when it failed),
        // unmapped and used by nobody.
        unsafe { _bus_dmamem_free(t, &segs[..n]) };
        cookie.id_bouncebuflen.set(0);
        cookie.id_nbouncesegs.set(0);
    } else {
        cookie.id_flags.set(cookie.id_flags.get() | ID_HAS_BOUNCE);
    }

    error
}

/// `_isa_dma_free_bouncebuf`: gives a map's bounce pages back.
pub fn _isa_dma_free_bouncebuf(t: BusDmaTagT, map: &BusDmamap) {
    let cookie = isa_dma_cookie(map);
    let n = cookie.id_nbouncesegs.get() as usize;

    if let Some(kva) = NonNull::new(cookie.id_bouncebuf.get()) {
        // SAFETY: the mapping `_isa_dma_alloc_bouncebuf` made; the map no longer uses it
        // (an unload or a destroy is running).
        unsafe { _bus_dmamem_unmap(t, kva, cookie.id_bouncebuflen.get()) };
    }
    // SAFETY: a bouncing map's cookie has its segments after it; the pages are unmapped
    // above and unused.
    unsafe { _bus_dmamem_free(t, &cookie.id_bouncesegs(map._dm_segcnt as usize)[..n]) };
    cookie.id_bouncebuf.set(ptr::null_mut());
    cookie.id_bouncebuflen.set(0);
    cookie.id_nbouncesegs.set(0);
    cookie.id_flags.set(cookie.id_flags.get() & !ID_HAS_BOUNCE);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_irqs() {
        assert!(legal_irq(4));
        assert!(!legal_irq(2));
        assert!(!legal_irq(16));
        assert!(!legal_irq(-1));
        assert_eq!(isa_intr_check(core::ptr::null(), 4, IST_EDGE), 2);
        assert_eq!(isa_intr_check(core::ptr::null(), 2, IST_EDGE), 0);
        assert_eq!(isa_intr_alloc(core::ptr::null(), 0x18, IST_EDGE), Some(3));
    }
}
/* </TESTS> */
