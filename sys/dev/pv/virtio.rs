/* <LICENSES> */
/*
 * Copyright (c) 2012 Stefan Fritsch, Alexander Fiveg.
 * Copyright (c) 2010 Minoura Makoto.
 * All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! The virtio core, `virtio(4)`: virtqueue allocation, the descriptor rings, enqueue and
//! dequeue, interrupt suppression and the device-independent half of reset.
//!
//! Upstream: sys/dev/pv/virtio.c @ 3ce1f3f79392
//!
//! The transports (`dev/pci/virtio_pci.rs`, `dev/fdt/virtio_mmio.rs`) attach as `virtio*`
//! (`virtio_cd`) and give the core a `struct virtio_ops`; the device drivers (`vio(4)`) use
//! the functions here on the queues they allocate.
//!
//! ## Deviations
//! - The rings are reached through raw pointers with volatile reads and writes (the
//!   accessors of `Virtqueue` in `virtiovar.rs` and the descriptor helpers below), never
//!   through references: the device changes them behind the compiler's back. The barriers
//!   (`virtio_membar_*`) come from `machine::atomic`.
//! - Out parameters become return values: `virtio_enqueue_prep` returns the slot,
//!   `virtio_dequeue` the slot and length; functions whose `int` is always 0
//!   (`virtio_enqueue`, `virtio_enqueue_p`, `virtio_enqueue_abort`) return nothing, and the
//!   ones whose `int` is a boolean (`virtio_check_vq`'s callers use it as "work done", which
//!   stays `i32`; `virtio_postpone_intr*`, `virtio_start_vq_intr`) return `bool`.
//! - `virtio_alloc_vq` and `virtio_free_vq` return `Result`: the C's `-1` is the errno of the
//!   step that failed (every caller only tests for nonzero).
//! - `VIRTIO_ASSERT` (empty unless `VIRTIO_DEBUG`) is `kdassert!`; the `VIRTIO_DEBUG` code
//!   (`virtio_log_features`, `virtio_vq_dump`, the allocation message) is compiled behind
//!   `if VIRTIO_DEBUG > 0`.
//! - `virtio_device_name[]`'s NULL entries are printed by the kernel's `printf` as
//!   `(null)`; `virtio_device_string` returns that text for them.
//! - [`virtio_intr_name`] is this port's: the machines' `*_intr_establish` keep the name as a
//!   `&'static str` (`evcount(9)`), and the C's `dv_xname` and `sc_intr[i].name` are arrays
//!   inside a softc, so the transports hand over a copy that is never freed (16 bytes per
//!   established interrupt).

use core::ptr::{self, NonNull};

use crate::dev::pv::virtioreg::{
    VIRTIO_CONFIG_DEVICE_STATUS_ACK, VIRTIO_CONFIG_DEVICE_STATUS_DRIVER,
    VIRTIO_CONFIG_DEVICE_STATUS_DRIVER_OK, VIRTIO_F_ACCESS_PLATFORM, VIRTIO_F_ANY_LAYOUT,
    VIRTIO_F_BAD_FEATURE, VIRTIO_F_IN_ORDER, VIRTIO_F_NOTIF_CONFIG_DATA,
    VIRTIO_F_NOTIFICATION_DATA, VIRTIO_F_NOTIFY_ON_EMPTY, VIRTIO_F_ORDER_PLATFORM,
    VIRTIO_F_RING_EVENT_IDX, VIRTIO_F_RING_INDIRECT_DESC, VIRTIO_F_RING_PACKED,
    VIRTIO_F_RING_RESET, VIRTIO_F_SR_IOV, VIRTIO_F_VERSION_1, VIRTIO_PAGE_SIZE,
    VRING_AVAIL_F_NO_INTERRUPT, VRING_AVAIL_RING, VRING_DESC_F_INDIRECT, VRING_DESC_F_NEXT,
    VRING_DESC_F_WRITE, VRING_USED_F_NO_NOTIFY, VRING_USED_RING, VringAvail, VringDesc, VringUsed,
    VringUsedElem,
};
use crate::dev::pv::virtiovar::{
    VIRTIO_DEBUG, VirtioAttachArgs, VirtioFeatureName, VirtioSoftc, Virtqueue, VqEntry,
    virtio_device_reset, virtio_has_feature, virtio_negotiate_features, virtio_read_queue_size,
    virtio_set_status, virtio_setup_queue,
};
use crate::kdassert;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::machine::atomic::{virtio_membar_consumer, virtio_membar_producer, virtio_membar_sync};
use crate::machine::bus::{
    BUS_DMA_64BIT, BUS_DMA_NOWAIT, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE,
    BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE, BusAddr, BusDmaSegment, BusDmamap, BusSize,
    bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load, bus_dmamap_sync, bus_dmamap_unload,
    bus_dmamem_alloc_range, bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap,
};
use crate::sys::device::{CD_COCOVM, Cfdriver, DV_DULL};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};

/// `VIRTQUEUE_ALIGN(n)`: rounds up to the virtio page size.
const fn virtqueue_align(n: usize) -> usize {
    (n + (VIRTIO_PAGE_SIZE - 1)) & !(VIRTIO_PAGE_SIZE - 1)
}

/// `virtio_cd`.
pub static VIRTIO_CD: Cfdriver = Cfdriver::new(b"virtio", DV_DULL, CD_COCOVM);

/// `virtio_device_name[]`.
static VIRTIO_DEVICE_NAME: [Option<&str>; 17] = [
    Some("Unknown (0)"),    // 0
    Some("Network"),        // 1
    Some("Block"),          // 2
    Some("Console"),        // 3
    Some("Entropy"),        // 4
    Some("Memory Balloon"), // 5
    Some("IO Memory"),      // 6
    Some("Rpmsg"),          // 7
    Some("SCSI host"),      // 8
    Some("9P Transport"),   // 9
    Some("mac80211 wlan"),  // 10
    None,                   // 11
    None,                   // 12
    None,                   // 13
    None,                   // 14
    None,                   // 15
    Some("GPU"),            // 16
];

/// `transport_feature_names[]` (`VIRTIO_DEBUG`).
static TRANSPORT_FEATURE_NAMES: [VirtioFeatureName; 14] = [
    VirtioFeatureName {
        bit: VIRTIO_F_NOTIFY_ON_EMPTY,
        name: "NotifyOnEmpty",
    },
    VirtioFeatureName {
        bit: VIRTIO_F_ANY_LAYOUT,
        name: "AnyLayout",
    },
    VirtioFeatureName {
        bit: VIRTIO_F_RING_INDIRECT_DESC,
        name: "RingIndirectDesc",
    },
    VirtioFeatureName {
        bit: VIRTIO_F_RING_EVENT_IDX,
        name: "RingEventIdx",
    },
    VirtioFeatureName {
        bit: VIRTIO_F_BAD_FEATURE,
        name: "BadFeature",
    },
    VirtioFeatureName {
        bit: VIRTIO_F_VERSION_1,
        name: "Version1",
    },
    VirtioFeatureName {
        bit: VIRTIO_F_ACCESS_PLATFORM,
        name: "AccessPlatf",
    },
    VirtioFeatureName {
        bit: VIRTIO_F_RING_PACKED,
        name: "RingPacked",
    },
    VirtioFeatureName {
        bit: VIRTIO_F_IN_ORDER,
        name: "InOrder",
    },
    VirtioFeatureName {
        bit: VIRTIO_F_ORDER_PLATFORM,
        name: "OrderPlatf",
    },
    VirtioFeatureName {
        bit: VIRTIO_F_SR_IOV,
        name: "SrIov",
    },
    VirtioFeatureName {
        bit: VIRTIO_F_NOTIFICATION_DATA,
        name: "NotifData",
    },
    VirtioFeatureName {
        bit: VIRTIO_F_NOTIF_CONFIG_DATA,
        name: "NotifConfData",
    },
    VirtioFeatureName {
        bit: VIRTIO_F_RING_RESET,
        name: "RingReset",
    },
];

/// `virtio_device_string(id)`: the name of virtio device `id`.
pub fn virtio_device_string(id: i32) -> &'static str {
    match usize::try_from(id)
        .ok()
        .and_then(|i| VIRTIO_DEVICE_NAME.get(i))
    {
        Some(Some(name)) => name,
        // A NULL entry, as the kernel's printf shows it.
        Some(None) => "(null)",
        None => "Unknown",
    }
}

/// The `const char *name` an interrupt is established with (`evcount(9)` keeps it): a copy
/// of `name`, up to its NUL, that lives as long as the kernel. A port helper, not an OpenBSD
/// function (see the module's deviations).
pub fn virtio_intr_name(name: &[u8]) -> &'static str {
    let n = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let Some(p) = malloc(n.max(1), M_DEVBUF, M_NOWAIT) else {
        return "virtio";
    };
    // SAFETY: a fresh allocation of at least `n` bytes that is never freed, so the copy may
    // be borrowed for the rest of the kernel's life.
    let copy: &'static mut [u8] = unsafe { core::slice::from_raw_parts_mut(p.as_ptr(), n) };
    copy.copy_from_slice(&name[..n]);
    core::str::from_utf8(copy).unwrap_or("virtio")
}

/// `virtio_log_features(host, neg, guest_feature_names)` (`VIRTIO_DEBUG`): the host's
/// features, `+` for the negotiated ones.
pub fn virtio_log_features(host: u64, neg: u64, guest_feature_names: &[VirtioFeatureName]) {
    for i in 0..64 {
        if i == 30 {
            // VIRTIO_F_BAD_FEATURE is only used for checking correct negotiation
            continue;
        }
        let bit = 1u64 << i;
        if host & bit == 0 {
            continue;
        }
        let name = guest_feature_names
            .iter()
            .chain(TRANSPORT_FEATURE_NAMES.iter())
            .find(|n| n.bit == bit)
            .map(|n| n.name);
        let c = if neg & bit != 0 { '+' } else { '-' };
        match name {
            Some(name) => printf(format_args!(" {c}{name}")),
            None => printf(format_args!(" {c}Unknown({i})")),
        };
    }
}

/// `virtio_reset`: reset the device.
///
/// To reset the device to a known state, do following:
/// ```text
///     virtio_reset(sc);            // this will stop the device activity
///     <dequeue finished requests>; // virtio_dequeue() still can be called
///     <revoke pending requests in the vqs if any>;
///     virtio_reinit_start(sc);     // dequeue prohibited
///     <some other initialization>;
///     virtio_reinit_end(sc);       // device activated; enqueue allowed
/// ```
/// Once attached, features are assumed to not change again.
pub fn virtio_reset(sc: &VirtioSoftc) {
    virtio_device_reset(sc);
    sc.sc_active_features.set(0);
}

/// `virtio_attach_finish`: the transport establishes the interrupts, every queue is handed
/// to the device and the device is told the driver is ready.
pub fn virtio_attach_finish(sc: &VirtioSoftc, va: *mut VirtioAttachArgs) -> Result<(), Errno> {
    (sc.ops().attach_finish)(sc, va)?;

    (sc.ops().setup_intrs)(sc);
    for vq in sc.vqs() {
        if vq.vq_num.get() == 0 {
            continue;
        }
        virtio_setup_queue(sc, vq, vq_ring_addr(vq));
    }
    virtio_set_status(sc, VIRTIO_CONFIG_DEVICE_STATUS_DRIVER_OK);
    Ok(())
}

/// `vq->vq_dmamap->dm_segs[0].ds_addr`: the bus address of the queue's rings.
fn vq_ring_addr(vq: &Virtqueue) -> u64 {
    vq.dmamap()
        .dm_segs()
        .first()
        .map_or(0, |s| s.get().ds_addr as u64)
}

/// `virtio_reinit_start`: feature negotiation, interrupts and queues again after a reset.
pub fn virtio_reinit_start(sc: &VirtioSoftc) {
    virtio_set_status(sc, VIRTIO_CONFIG_DEVICE_STATUS_ACK);
    virtio_set_status(sc, VIRTIO_CONFIG_DEVICE_STATUS_DRIVER);
    let _ = virtio_negotiate_features(sc, None);
    (sc.ops().setup_intrs)(sc);
    for vq in sc.vqs() {
        if vq.vq_num.get() == 0 {
            // not used
            continue;
        }
        let n = virtio_read_queue_size(sc, vq.vq_index.get() as u16);
        if u32::from(n) != vq.vq_num.get() {
            panic(format_args!(
                "{}: virtqueue size changed, vq index {}",
                Str(&sc.sc_dev.dv_xname.get()),
                vq.vq_index.get()
            ));
        }
        virtio_init_vq(sc, vq);
        virtio_setup_queue(sc, vq, vq_ring_addr(vq));
    }
}

/// `virtio_reinit_end`: device activated; enqueue allowed.
pub fn virtio_reinit_end(sc: &VirtioSoftc) {
    virtio_set_status(sc, VIRTIO_CONFIG_DEVICE_STATUS_DRIVER_OK);
}

// dmamap sync operations for a virtqueue.
//
// XXX These should be more fine grained. Syncing the whole ring if we only need a few bytes
// XXX is inefficient if we use bounce buffers.

/// `vq_sync_descs`: the descriptor table; `availoffset == sizeof(vring_desc) * vq_num`.
fn vq_sync_descs(sc: &VirtioSoftc, vq: &Virtqueue, ops: i32) {
    bus_dmamap_sync(
        sc.dmat(),
        vq.dmamap(),
        0,
        vq.vq_availoffset.get() as BusSize,
        ops,
    );
}

/// `vq_sync_aring`: the avail ring.
fn vq_sync_aring(sc: &VirtioSoftc, vq: &Virtqueue, ops: i32) {
    bus_dmamap_sync(
        sc.dmat(),
        vq.dmamap(),
        vq.vq_availoffset.get() as BusAddr,
        VRING_AVAIL_RING + vq.vq_num.get() as usize * size_of::<u16>(),
        ops,
    );
}

/// `vq_sync_aring_used_event`: the used event index after the avail ring.
fn vq_sync_aring_used_event(sc: &VirtioSoftc, vq: &Virtqueue, ops: i32) {
    bus_dmamap_sync(
        sc.dmat(),
        vq.dmamap(),
        vq.vq_availoffset.get() as BusAddr
            + VRING_AVAIL_RING
            + vq.vq_num.get() as usize * size_of::<u16>(),
        size_of::<u16>(),
        ops,
    );
}

/// `vq_sync_uring`: the used ring.
fn vq_sync_uring(sc: &VirtioSoftc, vq: &Virtqueue, ops: i32) {
    bus_dmamap_sync(
        sc.dmat(),
        vq.dmamap(),
        vq.vq_usedoffset.get() as BusAddr,
        VRING_USED_RING + vq.vq_num.get() as usize * size_of::<VringUsedElem>(),
        ops,
    );
}

/// `vq_sync_uring_avail_event`: the avail event index after the used ring.
fn vq_sync_uring_avail_event(sc: &VirtioSoftc, vq: &Virtqueue, ops: i32) {
    bus_dmamap_sync(
        sc.dmat(),
        vq.dmamap(),
        vq.vq_usedoffset.get() as BusAddr
            + VRING_USED_RING
            + vq.vq_num.get() as usize * size_of::<VringUsedElem>(),
        size_of::<u16>(),
        ops,
    );
}

/// `vq_sync_indirect`: the indirect table of `slot`.
fn vq_sync_indirect(sc: &VirtioSoftc, vq: &Virtqueue, slot: usize, ops: i32) {
    let offset = vq.vq_indirectoffset.get() as usize
        + size_of::<VringDesc>() * vq.vq_maxnsegs.get() as usize * slot;

    bus_dmamap_sync(
        sc.dmat(),
        vq.dmamap(),
        offset,
        size_of::<VringDesc>() * vq.vq_maxnsegs.get() as usize,
        ops,
    );
}

/// `virtio_check_vqs`: scan the queues, `bus_dmamap_sync` the rings (not the payload) and
/// call `vq_done` for those with consumed entries. For use in transport specific irq
/// handlers.
pub fn virtio_check_vqs(sc: &VirtioSoftc) -> i32 {
    let mut r = 0;

    // going backwards is better for if_vio
    for vq in sc.vqs().iter().rev() {
        if vq.vq_num.get() == 0 {
            // not used
            continue;
        }
        r |= virtio_check_vq(sc, vq);
    }

    r
}

/// `virtio_check_vq`: one queue of [`virtio_check_vqs`].
pub fn virtio_check_vq(sc: &VirtioSoftc, vq: &Virtqueue) -> i32 {
    if vq.vq_queued.get() != 0 {
        vq.vq_queued.set(0);
        vq_sync_aring(sc, vq, BUS_DMASYNC_POSTWRITE);
    }
    vq_sync_uring(sc, vq, BUS_DMASYNC_POSTREAD);
    if vq.vq_used_idx.get() != vq.used_idx()
        && let Some(done) = vq.vq_done.get()
    {
        return done(vq);
    }

    0
}

// Descriptor tables. A table is `vq_desc` (`vq_num` entries) or one slot's part of
// `vq_indirect` (`vq_maxnsegs` entries); the indexes come from the free list or from `next`
// links this file wrote, so they stay inside the table.

/// `&vd[i]`.
///
/// # Safety
///
/// `base` is a descriptor table of the queue's ring memory with more than `i` entries.
unsafe fn vd_at(base: *mut VringDesc, i: usize) -> *mut VringDesc {
    // SAFETY: the caller's guarantee.
    unsafe { base.add(i) }
}

/// `vd[i].flags`.
///
/// # Safety
///
/// As for [`vd_at`].
unsafe fn vd_flags(base: *mut VringDesc, i: usize) -> u16 {
    // SAFETY: the caller's guarantee; the ring memory is shared with the device: volatile.
    unsafe { ptr::read_volatile(&raw const (*vd_at(base, i)).flags) }
}

/// `vd[i].flags = v`.
///
/// # Safety
///
/// As for [`vd_at`].
unsafe fn vd_set_flags(base: *mut VringDesc, i: usize, v: u16) {
    // SAFETY: as for `vd_flags`.
    unsafe { ptr::write_volatile(&raw mut (*vd_at(base, i)).flags, v) }
}

/// `vd[i].next`.
///
/// # Safety
///
/// As for [`vd_at`].
unsafe fn vd_next(base: *mut VringDesc, i: usize) -> u16 {
    // SAFETY: as for `vd_flags`.
    unsafe { ptr::read_volatile(&raw const (*vd_at(base, i)).next) }
}

/// `vd[i].next = v`.
///
/// # Safety
///
/// As for [`vd_at`].
unsafe fn vd_set_next(base: *mut VringDesc, i: usize, v: u16) {
    // SAFETY: as for `vd_flags`.
    unsafe { ptr::write_volatile(&raw mut (*vd_at(base, i)).next, v) }
}

/// `vd[i].addr = addr; vd[i].len = len`.
///
/// # Safety
///
/// As for [`vd_at`].
unsafe fn vd_set_buf(base: *mut VringDesc, i: usize, addr: u64, len: u32) {
    // SAFETY: as for `vd_flags`.
    unsafe {
        ptr::write_volatile(&raw mut (*vd_at(base, i)).addr, addr);
        ptr::write_volatile(&raw mut (*vd_at(base, i)).len, len);
    }
}

/// `vd[i].len = len`.
///
/// # Safety
///
/// As for [`vd_at`].
unsafe fn vd_set_len(base: *mut VringDesc, i: usize, len: u32) {
    // SAFETY: as for `vd_flags`.
    unsafe { ptr::write_volatile(&raw mut (*vd_at(base, i)).len, len) }
}

/// `virtio_init_vq`: initialize vq structure.
pub fn virtio_init_vq(sc: &VirtioSoftc, vq: &Virtqueue) {
    let vq_size = vq.vq_num.get() as usize;

    kdassert!(vq_size > 0);
    // SAFETY: `vq_vaddr` maps the queue's `vq_bytesize` bytes of DMA memory, which only the
    // device and this queue's functions touch.
    unsafe { ptr::write_bytes(vq.vq_vaddr.get(), 0, vq.vq_bytesize.get() as usize) };

    // build the indirect descriptor chain
    let indirect = vq.vq_indirect.get();
    if !indirect.is_null() {
        let maxnsegs = vq.vq_maxnsegs.get() as usize;
        for i in 0..vq_size {
            for j in 0..maxnsegs - 1 {
                // SAFETY: the indirect area holds `vq_size * maxnsegs` descriptors
                // (`virtio_alloc_vq`'s allocsize3).
                unsafe { vd_set_next(indirect, maxnsegs * i + j, (j + 1) as u16) };
            }
        }
    }

    // free slot management
    vq.vq_freelist.init();
    // virtio_enqueue_trim needs monotonely raising entries, therefore initialize in reverse
    // order
    for i in (0..vq_size).rev() {
        let qe = vq.entry(i);
        // SAFETY: the free list was just emptied, so the entry is in no list; the entries
        // stay in place until virtio_free_vq.
        unsafe { vq.vq_freelist.insert_head(qe) };
        qe.qe_index.set(i as u16);
    }

    bus_dmamap_sync(
        sc.dmat(),
        vq.dmamap(),
        0,
        vq.vq_bytesize.get() as BusSize,
        BUS_DMASYNC_PREWRITE,
    );
    // enqueue/dequeue status
    vq.vq_avail_idx.set(0);
    vq.vq_used_idx.set(0);
    vq_sync_uring(sc, vq, BUS_DMASYNC_PREREAD);
    vq.vq_queued.set(1);
}

/// `virtio_alloc_vq`: allocate queue `index` of the device into `vq`.
///
/// `maxnsegs` denotes how much space should be allocated for indirect descriptors.
/// `maxnsegs == 1` can be used to disable use indirect descriptors for this queue.
pub fn virtio_alloc_vq(
    sc: &VirtioSoftc,
    vq: &Virtqueue,
    index: i32,
    maxnsegs: i32,
    name: &str,
) -> Result<(), Errno> {
    vq.clear();
    let mut allocsize = 0;

    let r = 'alloc: {
        let vq_size = usize::from(virtio_read_queue_size(sc, index as u16));
        if vq_size == 0 {
            printf(format_args!(
                "virtqueue not exist, index {index} for {name}\n"
            ));
            break 'alloc Err(Errno::ENXIO);
        }
        if ((vq_size - 1) & vq_size) != 0 {
            panic(format_args!("vq_size not power of two: {vq_size}"));
        }

        let hdrlen = if virtio_has_feature(sc, VIRTIO_F_RING_EVENT_IDX) {
            3
        } else {
            2
        };

        // allocsize1: descriptor table + avail ring + pad
        let allocsize1 = virtqueue_align(
            size_of::<VringDesc>() * vq_size + size_of::<u16>() * (hdrlen + vq_size),
        );
        // allocsize2: used ring + pad
        let allocsize2 =
            virtqueue_align(size_of::<u16>() * hdrlen + size_of::<VringUsedElem>() * vq_size);
        // allocsize3: indirect table
        let allocsize3 = if sc.sc_indirect.get() != 0 && maxnsegs > 1 {
            size_of::<VringDesc>() * maxnsegs as usize * vq_size
        } else {
            0
        };
        allocsize = allocsize1 + allocsize2 + allocsize3;

        // alloc and map the memory
        //
        // With virtio 0.9, the ring memory must be in the lowest 2^32 pages. For simplicity,
        // we use this limit even for virtio 1.0.
        let mut segs = [BusDmaSegment::default(); 1];
        if let Err(r) = bus_dmamem_alloc_range(
            sc.dmat(),
            allocsize,
            VIRTIO_PAGE_SIZE,
            0,
            &mut segs,
            BUS_DMA_NOWAIT,
            0,
            ((VIRTIO_PAGE_SIZE as u64) << 32) as usize - 1,
        ) {
            printf(format_args!(
                "virtqueue {index} for {name} allocation failed, error {}\n",
                r as i32
            ));
            break 'alloc Err(r);
        }
        vq.vq_segs[0].set(segs[0]);
        let vaddr = match bus_dmamem_map(sc.dmat(), &mut segs, allocsize, BUS_DMA_NOWAIT) {
            Ok(va) => va,
            Err(r) => {
                printf(format_args!(
                    "virtqueue {index} for {name} map failed, error {}\n",
                    r as i32
                ));
                break 'alloc Err(r);
            }
        };
        vq.vq_vaddr.set(vaddr.as_ptr());
        let map = match bus_dmamap_create(
            sc.dmat(),
            allocsize,
            1,
            allocsize,
            0,
            BUS_DMA_NOWAIT | BUS_DMA_64BIT,
        ) {
            Ok(map) => map,
            Err(r) => {
                printf(format_args!(
                    "virtqueue {index} for {name} dmamap creation failed, error {}\n",
                    r as i32
                ));
                break 'alloc Err(r);
            }
        };
        vq.vq_dmamap.set(Some(map));
        // SAFETY: `vaddr` maps `allocsize` bytes of DMA memory that stay allocated until
        // virtio_free_vq unloads the map before unmapping and freeing them.
        if let Err(r) = unsafe {
            bus_dmamap_load(
                sc.dmat(),
                map,
                vaddr.as_ptr(),
                allocsize,
                None,
                BUS_DMA_NOWAIT,
            )
        } {
            printf(format_args!(
                "virtqueue {index} for {name} dmamap load failed, error {}\n",
                r as i32
            ));
            break 'alloc Err(r);
        }

        // remember addresses and offsets for later use
        vq.vq_owner.set(ptr::from_ref(sc));
        vq.vq_num.set(vq_size as u32);
        vq.vq_mask.set(vq_size as u32 - 1);
        vq.vq_index.set(index);
        vq.vq_desc.set(vaddr.as_ptr().cast());
        vq.vq_availoffset
            .set((size_of::<VringDesc>() * vq_size) as i32);
        // SAFETY: both offsets are inside the `allocsize` bytes just mapped.
        unsafe {
            vq.vq_avail.set(
                vaddr
                    .as_ptr()
                    .add(vq.vq_availoffset.get() as usize)
                    .cast::<VringAvail>(),
            );
            vq.vq_used
                .set(vaddr.as_ptr().add(allocsize1).cast::<VringUsed>());
        }
        vq.vq_usedoffset.set(allocsize1 as i32);
        if allocsize3 > 0 {
            vq.vq_indirectoffset.set((allocsize1 + allocsize2) as i32);
            // SAFETY: as above.
            vq.vq_indirect.set(unsafe {
                vaddr
                    .as_ptr()
                    .add(allocsize1 + allocsize2)
                    .cast::<VringDesc>()
            });
        }
        vq.vq_bytesize.set(allocsize as u32);
        vq.vq_maxnsegs.set(maxnsegs);

        // free slot management
        let Some(entries) = mallocarray(vq_size, size_of::<VqEntry>(), M_DEVBUF, M_NOWAIT | M_ZERO)
        else {
            break 'alloc Err(Errno::ENOMEM);
        };
        vq.vq_entries.set(entries.as_ptr().cast());

        virtio_init_vq(sc, vq);

        if VIRTIO_DEBUG > 0 {
            printf(format_args!(
                "\nallocated {allocsize} byte for virtqueue {index} for {name}, size {vq_size}\n"
            ));
            if allocsize3 > 0 {
                printf(format_args!(
                    "using {allocsize3} byte ({} entries) indirect descriptors\n",
                    maxnsegs as usize * vq_size
                ));
            }
        }
        Ok(())
    };

    if r.is_err() {
        // err:
        if let Some(map) = vq.vq_dmamap.get() {
            // SAFETY: the map came from bus_dmamap_create above and nothing else has it.
            unsafe { bus_dmamap_destroy(sc.dmat(), NonNull::from(map)) };
        }
        if let Some(va) = NonNull::new(vq.vq_vaddr.get()) {
            // SAFETY: the mapping came from bus_dmamem_map above with this size.
            unsafe { bus_dmamem_unmap(sc.dmat(), va, allocsize) };
        }
        if vq.vq_segs[0].get().ds_addr != 0 {
            // SAFETY: the segment came from bus_dmamem_alloc_range above and is unmapped.
            unsafe { bus_dmamem_free(sc.dmat(), &[vq.vq_segs[0].get()]) };
        }
        vq.clear();
    }
    r
}

/// `virtio_free_vq`: give a queue's memory back; the device must be already deactivated.
pub fn virtio_free_vq(sc: &VirtioSoftc, vq: &Virtqueue) -> Result<(), Errno> {
    if vq.vq_num.get() == 0 {
        // virtio_alloc_vq() was never called
        return Ok(());
    }

    // device must be already deactivated
    // confirm the vq is empty
    let i = vq.vq_freelist.iter().count();
    if i != vq.vq_num.get() as usize {
        printf(format_args!(
            "{}: freeing non-empty vq, index {}\n",
            Str(&sc.sc_dev.dv_xname.get()),
            vq.vq_index.get()
        ));
        return Err(Errno::EBUSY);
    }

    // tell device that there's no virtqueue any longer
    virtio_setup_queue(sc, vq, 0);

    if let Some(entries) = NonNull::new(vq.vq_entries.get()) {
        free(entries.cast(), M_DEVBUF, 0);
    }
    let map = vq.dmamap();
    bus_dmamap_unload(sc.dmat(), map);
    // SAFETY: the queue's map, unloaded and no longer used (the queue is cleared below).
    unsafe { bus_dmamap_destroy(sc.dmat(), NonNull::from(map)) };
    if let Some(va) = NonNull::new(vq.vq_vaddr.get()) {
        // SAFETY: the queue's mapping, of `vq_bytesize` bytes, no longer used.
        unsafe { bus_dmamem_unmap(sc.dmat(), va, vq.vq_bytesize.get() as usize) };
    }
    // SAFETY: the queue's memory, unmapped and unloaded above.
    unsafe { bus_dmamem_free(sc.dmat(), &[vq.vq_segs[0].get()]) };
    vq.clear();

    Ok(())
}

/// `vq_alloc_entry`: free descriptor management.
pub fn vq_alloc_entry(vq: &Virtqueue) -> Option<&VqEntry> {
    let qe = vq.vq_freelist.first()?;
    // SAFETY: the list is not empty.
    unsafe { vq.vq_freelist.remove_head() };

    Some(qe)
}

/// `vq_free_entry`.
pub fn vq_free_entry(vq: &Virtqueue, qe: &VqEntry) {
    // SAFETY: a slot is on the free list or in exactly one request; the callers free a
    // request's slots once, so `qe` is in no list.
    unsafe { vq.vq_freelist.insert_head(qe) };
}

// Enqueue several dmamaps as a single request.
//
// Typical usage:
//  <queue size> number of followings are stored in arrays
//  - command blocks (in dmamem) should be pre-allocated and mapped
//  - dmamaps for command blocks should be pre-allocated and loaded
//  - dmamaps for payload should be pre-allocated
//      r = virtio_enqueue_prep(sc, vq, &slot);         // allocate a slot
//      if (r)          // currently 0 or EAGAIN
//        return r;
//      r = bus_dmamap_load(dmat, dmamap_payload[slot], data, count, ..);
//      if (r) {
//        virtio_enqueue_abort(sc, vq, slot);
//        bus_dmamap_unload(dmat, dmamap_payload[slot]);
//        return r;
//      }
//      r = virtio_enqueue_reserve(sc, vq, slot,
//                                 dmamap_payload[slot]->dm_nsegs+1);
//                                                      // ^ +1 for command
//      if (r) {        // currently 0 or EAGAIN
//        bus_dmamap_unload(dmat, dmamap_payload[slot]);
//        return r;                                     // do not call abort()
//      }
//      <setup and prepare commands>
//      bus_dmamap_sync(dmat, dmamap_cmd[slot],... BUS_DMASYNC_PREWRITE);
//      bus_dmamap_sync(dmat, dmamap_payload[slot],...);
//      virtio_enqueue(sc, vq, slot, dmamap_cmd[slot], 0);
//      virtio_enqueue(sc, vq, slot, dmamap_payload[slot], iswrite);
//      virtio_enqueue_commit(sc, vq, slot, 1);
//
// Alternative usage with statically allocated slots:
//      <during initialization>
//      // while not out of slots, do
//      virtio_enqueue_prep(sc, vq, &slot);             // allocate a slot
//      virtio_enqueue_reserve(sc, vq, slot, max_segs); // reserve all slots
//                                              that may ever be needed
//
//      <when enqueuing a request>
//      // Don't call virtio_enqueue_prep()
//      bus_dmamap_load(dmat, dmamap_payload[slot], data, count, ..);
//      bus_dmamap_sync(dmat, dmamap_cmd[slot],... BUS_DMASYNC_PREWRITE);
//      bus_dmamap_sync(dmat, dmamap_payload[slot],...);
//      virtio_enqueue_trim(sc, vq, slot, num_segs_needed);
//      virtio_enqueue(sc, vq, slot, dmamap_cmd[slot], 0);
//      virtio_enqueue(sc, vq, slot, dmamap_payload[slot], iswrite);
//      virtio_enqueue_commit(sc, vq, slot, 1);
//
//      <when dequeuing>
//      // don't call virtio_dequeue_commit()

/// `virtio_enqueue_prep`: allocate a slot number; `EAGAIN` when the queue is full.
pub fn virtio_enqueue_prep(vq: &Virtqueue) -> Result<i32, Errno> {
    let qe1 = vq_alloc_entry(vq).ok_or(Errno::EAGAIN)?;
    // next slot is not allocated yet
    qe1.qe_next.set(-1);
    Ok(i32::from(qe1.qe_index.get()))
}

/// `virtio_enqueue_reserve`: allocate remaining slots and build the descriptor chain. Calls
/// [`virtio_enqueue_abort`] on failure (`EAGAIN`).
pub fn virtio_enqueue_reserve(vq: &Virtqueue, slot: i32, nsegs: i32) -> Result<(), Errno> {
    let qe1 = vq.entry(slot as usize);

    kdassert!(qe1.qe_next.get() == -1);
    kdassert!(1 <= nsegs && nsegs as u32 <= vq.vq_num.get());

    let indirect = vq.vq_indirect.get();
    if !indirect.is_null() && nsegs > 1 && nsegs <= vq.vq_maxnsegs.get() {
        qe1.qe_indirect.set(1);

        let maxnsegs = vq.vq_maxnsegs.get() as usize;
        let idx = usize::from(qe1.qe_index.get());
        let addr = vq_ring_addr(vq)
            + vq.vq_indirectoffset.get() as u64
            + (size_of::<VringDesc>() * maxnsegs * idx) as u64;
        // SAFETY: `idx` is a slot of the queue, below `vq_num`, the size of `vq_desc`.
        unsafe {
            vd_set_buf(
                vq.vq_desc.get(),
                idx,
                addr,
                (size_of::<VringDesc>() * nsegs as usize) as u32,
            );
            vd_set_flags(vq.vq_desc.get(), idx, VRING_DESC_F_INDIRECT);
        }

        // SAFETY: slot `idx`'s part of the indirect area, `maxnsegs` descriptors.
        let vd = unsafe { indirect.add(maxnsegs * idx) };
        qe1.qe_desc_base.set(vd);

        let n = nsegs as usize;
        for i in 0..n - 1 {
            // SAFETY: `i < nsegs <= maxnsegs`.
            unsafe { vd_set_flags(vd, i, VRING_DESC_F_NEXT) };
        }
        // SAFETY: as above.
        unsafe { vd_set_flags(vd, n - 1, 0) };
        qe1.qe_next.set(0);

        Ok(())
    } else {
        qe1.qe_indirect.set(0);

        let vd = vq.vq_desc.get();
        qe1.qe_desc_base.set(vd);
        qe1.qe_next.set(qe1.qe_index.get() as i16);
        let mut s = slot as usize;
        for _ in 0..nsegs - 1 {
            let Some(qe) = vq_alloc_entry(vq) else {
                // SAFETY: `s` is a slot of the queue (below `vq_num`).
                unsafe { vd_set_flags(vd, s, 0) };
                virtio_enqueue_abort(vq, slot);
                return Err(Errno::EAGAIN);
            };
            // SAFETY: as above.
            unsafe {
                vd_set_flags(vd, s, VRING_DESC_F_NEXT);
                vd_set_next(vd, s, qe.qe_index.get());
            }
            s = usize::from(qe.qe_index.get());
        }
        // SAFETY: as above.
        unsafe { vd_set_flags(vd, s, 0) };

        Ok(())
    }
}

/// `virtio_enqueue`: enqueue a single dmamap; `write` is true when the device only reads the
/// buffer.
pub fn virtio_enqueue(vq: &Virtqueue, slot: i32, dmamap: &BusDmamap, write: bool) {
    let qe1 = vq.entry(slot as usize);
    let vd = qe1.qe_desc_base.get();
    let mut s = qe1.qe_next.get();

    kdassert!(s >= 0);
    kdassert!(dmamap.dm_nsegs.get() > 0);
    if dmamap.dm_nsegs.get() > vq.vq_maxnsegs.get() {
        if VIRTIO_DEBUG > 0 {
            let nsegs = dmamap.dm_nsegs.get() as usize;
            for (i, seg) in dmamap.dm_segs().iter().take(nsegs).enumerate() {
                let seg = seg.get();
                printf(format_args!(
                    " {i} ({}): {:#x} {:x} \n",
                    i32::from(write),
                    seg.ds_addr,
                    seg.ds_len
                ));
            }
        }
        panic(format_args!(
            "dmamap->dm_nseg {} > vq->vq_maxnsegs {}",
            dmamap.dm_nsegs.get(),
            vq.vq_maxnsegs.get()
        ));
    }

    let nsegs = dmamap.dm_nsegs.get() as usize;
    for seg in dmamap.dm_segs().iter().take(nsegs) {
        let seg = seg.get();
        let i = s as usize;
        // SAFETY: `s` walks the chain virtio_enqueue_reserve built in `qe_desc_base`'s
        // table, whose length the reservation covers.
        unsafe {
            vd_set_buf(vd, i, seg.ds_addr as u64, seg.ds_len as u32);
            if !write {
                vd_set_flags(vd, i, vd_flags(vd, i) | VRING_DESC_F_WRITE);
            }
            s = vd_next(vd, i) as i16;
        }
    }
    qe1.qe_next.set(s);
}

/// `virtio_enqueue_p`: enqueue `[start, start + len)` of a single-segment dmamap.
pub fn virtio_enqueue_p(
    vq: &Virtqueue,
    slot: i32,
    dmamap: &BusDmamap,
    start: BusAddr,
    len: BusSize,
    write: bool,
) {
    let qe1 = vq.entry(slot as usize);
    let vd = qe1.qe_desc_base.get();
    let s = qe1.qe_next.get();

    kdassert!(s >= 0);
    // XXX todo: handle more segments
    kdassert!(dmamap.dm_nsegs.get() == 1);
    let seg0 = dmamap
        .dm_segs()
        .first()
        .map(|s| s.get())
        .unwrap_or_default();
    kdassert!(seg0.ds_len > start && seg0.ds_len >= start + len);

    let i = s as usize;
    // SAFETY: `s` is the next descriptor of the chain virtio_enqueue_reserve built.
    unsafe {
        vd_set_buf(vd, i, (seg0.ds_addr + start) as u64, len as u32);
        if !write {
            vd_set_flags(vd, i, vd_flags(vd, i) | VRING_DESC_F_WRITE);
        }
        qe1.qe_next.set(vd_next(vd, i) as i16);
    }
}

/// `publish_avail_idx`.
fn publish_avail_idx(sc: &VirtioSoftc, vq: &Virtqueue) {
    // first make sure the avail ring entries are visible to the device
    vq_sync_aring(sc, vq, BUS_DMASYNC_PREWRITE);

    virtio_membar_producer();
    vq.set_avail_idx(vq.vq_avail_idx.get());
    // make the avail idx visible to the device
    vq_sync_aring(sc, vq, BUS_DMASYNC_PREWRITE);
    vq.vq_queued.set(1);
}

/// `virtio_enqueue_commit`: add it to the aring; with `slot < 0` only notify. With
/// `notifynow` the device is kicked unless it said it does not need to be.
pub fn virtio_enqueue_commit(sc: &VirtioSoftc, vq: &Virtqueue, slot: i32, notifynow: bool) {
    if slot >= 0 {
        vq_sync_descs(sc, vq, BUS_DMASYNC_PREWRITE);
        let qe1 = vq.entry(slot as usize);
        if qe1.qe_indirect.get() != 0 {
            vq_sync_indirect(sc, vq, slot as usize, BUS_DMASYNC_PREWRITE);
        }
        let idx = vq.vq_avail_idx.get();
        vq.vq_avail_idx.set(idx.wrapping_add(1));
        vq.set_avail_ring(u32::from(idx) & vq.vq_mask.get(), slot as u16);
    }

    // notify:
    if notifynow {
        if virtio_has_feature(vq.owner(), VIRTIO_F_RING_EVENT_IDX) {
            let o = vq.avail_idx();
            let n = vq.vq_avail_idx.get();
            publish_avail_idx(sc, vq);

            virtio_membar_sync();
            vq_sync_uring_avail_event(sc, vq, BUS_DMASYNC_POSTREAD);
            let t = vq.vq_avail_event().wrapping_add(1);
            if n.wrapping_sub(t) < n.wrapping_sub(o) {
                (sc.ops().kick)(sc, vq.vq_index.get() as u16);
            }
        } else {
            publish_avail_idx(sc, vq);

            virtio_membar_sync();
            vq_sync_uring(sc, vq, BUS_DMASYNC_POSTREAD);
            if vq.used_flags() & VRING_USED_F_NO_NOTIFY == 0 {
                (sc.ops().kick)(sc, vq.vq_index.get() as u16);
            }
        }
    }
}

/// `virtio_notify(sc, vq)`: `virtio_enqueue_commit(sc, vq, -1, 1)`.
pub fn virtio_notify(sc: &VirtioSoftc, vq: &Virtqueue) {
    virtio_enqueue_commit(sc, vq, -1, true)
}

/// `virtio_enqueue_abort`: rollback.
pub fn virtio_enqueue_abort(vq: &Virtqueue, slot: i32) {
    let mut qe = vq.entry(slot as usize);

    if qe.qe_next.get() < 0 {
        vq_free_entry(vq, qe);
        return;
    }

    let mut s = slot as usize;
    let vd = vq.vq_desc.get();
    // SAFETY: `s` follows the direct chain virtio_enqueue_reserve built in `vq_desc`.
    while unsafe { vd_flags(vd, s) } & VRING_DESC_F_NEXT != 0 {
        // SAFETY: as above.
        s = usize::from(unsafe { vd_next(vd, s) });
        vq_free_entry(vq, qe);
        qe = vq.entry(s);
    }
    vq_free_entry(vq, qe);
}

/// `virtio_enqueue_trim`: adjust buffer size to given # of segments, a.k.a. descriptors.
pub fn virtio_enqueue_trim(vq: &Virtqueue, slot: i32, nsegs: i32) {
    let qe1 = vq.entry(slot as usize);
    let mut vd = vq.vq_desc.get();
    let mut slot = slot as usize;

    // SAFETY: `slot` is a slot of the queue, below `vq_num`.
    if unsafe { vd_flags(vd, slot) } & VRING_DESC_F_INDIRECT == 0 {
        qe1.qe_next.set(qe1.qe_index.get() as i16);
        // N.B.: the vq_entries are ASSUMED to be a contiguous block with slot being the
        // index to the first one.
    } else {
        qe1.qe_next.set(0);
        // SAFETY: as above.
        unsafe {
            vd_set_len(
                vd,
                usize::from(qe1.qe_index.get()),
                (size_of::<VringDesc>() * nsegs as usize) as u32,
            )
        };
        vd = qe1.qe_desc_base.get();
        slot = 0;
    }

    for _ in 0..nsegs - 1 {
        // SAFETY: the statically reserved chain is contiguous from `slot` and at least
        // `nsegs` long (the C's assumption above).
        unsafe { vd_set_flags(vd, slot, VRING_DESC_F_NEXT) };
        slot += 1;
    }
    // SAFETY: as above.
    unsafe { vd_set_flags(vd, slot, 0) };
}

/// `virtio_dequeue`: dequeue a request from uring; returns its slot and the length the
/// device wrote, `ENOENT` when there is none. `bus_dmamap_sync` for uring must already have
/// been done, usually by `virtio_check_vq()` in the interrupt handler. This means that
/// polling `virtio_dequeue()` repeatedly until it returns 0 does not work.
pub fn virtio_dequeue(sc: &VirtioSoftc, vq: &Virtqueue) -> Result<(i32, i32), Errno> {
    if vq.vq_used_idx.get() == vq.used_idx() {
        return Err(Errno::ENOENT);
    }
    let usedidx = vq.vq_used_idx.get();
    vq.vq_used_idx.set(usedidx.wrapping_add(1));
    let usedidx = u32::from(usedidx) & vq.vq_mask.get();

    virtio_membar_consumer();
    vq_sync_uring(sc, vq, BUS_DMASYNC_POSTREAD);
    let elem = vq.used_ring(usedidx);
    let slot = elem.id as u16;
    let qe = vq.entry(usize::from(slot));

    if qe.qe_indirect.get() != 0 {
        vq_sync_indirect(sc, vq, usize::from(slot), BUS_DMASYNC_POSTWRITE);
    }

    Ok((i32::from(slot), elem.len as i32))
}

/// `virtio_dequeue_commit`: complete dequeue; the slot is recycled for future use. If you
/// forget to call this the slot will be leaked.
///
/// Don't call this if you use statically allocated slots and `virtio_enqueue_trim()`.
///
/// Returns the number of freed slots.
pub fn virtio_dequeue_commit(vq: &Virtqueue, slot: i32) -> i32 {
    let mut qe = vq.entry(slot as usize);
    let vd = vq.vq_desc.get();
    let mut s = slot as usize;
    let mut r = 1;

    // SAFETY: `s` follows the request's chain in `vq_desc` (the head of an indirect request
    // has no NEXT flag).
    while unsafe { vd_flags(vd, s) } & VRING_DESC_F_NEXT != 0 {
        // SAFETY: as above.
        s = usize::from(unsafe { vd_next(vd, s) });
        vq_free_entry(vq, qe);
        qe = vq.entry(s);
        r += 1;
    }
    vq_free_entry(vq, qe);

    r
}

/// `virtio_postpone_intr`: increase the event index in order to delay interrupts. Returns
/// `false` on success; `true` if the used ring has already advanced too far, and the caller
/// must process the queue again (otherwise, no more interrupts will happen).
pub fn virtio_postpone_intr(vq: &Virtqueue, nslots: u16) -> bool {
    let idx = vq.vq_used_idx.get().wrapping_add(nslots);

    // set the new event index: avail_ring->used_event = idx
    vq.set_vq_used_event(idx);
    virtio_membar_sync();

    vq_sync_aring_used_event(vq.owner(), vq, BUS_DMASYNC_PREWRITE);
    vq.vq_queued.set(vq.vq_queued.get() + 1);

    i32::from(nslots) < virtio_nused(vq)
}

/// `virtio_postpone_intr_smart`: postpone interrupt until 3/4 of the available descriptors
/// have been consumed.
pub fn virtio_postpone_intr_smart(vq: &Virtqueue) -> bool {
    let nslots = (u32::from(vq.avail_idx().wrapping_sub(vq.vq_used_idx.get())) * 3 / 4) as u16;

    virtio_postpone_intr(vq, nslots)
}

/// `virtio_postpone_intr_far`: postpone interrupt until all of the available descriptors
/// have been consumed.
pub fn virtio_postpone_intr_far(vq: &Virtqueue) -> bool {
    let nslots = vq.avail_idx().wrapping_sub(vq.vq_used_idx.get());

    virtio_postpone_intr(vq, nslots)
}

/// `virtio_stop_vq_intr`: stop vq interrupt. No guarantee.
pub fn virtio_stop_vq_intr(sc: &VirtioSoftc, vq: &Virtqueue) {
    if virtio_has_feature(sc, VIRTIO_F_RING_EVENT_IDX) {
        // No way to disable the interrupt completely with RingEventIdx. Instead advance
        // used_event by half the possible value. This won't happen soon and is far enough in
        // the past to not trigger a spurious interrupt.
        vq.set_vq_used_event(vq.vq_used_idx.get().wrapping_add(0x8000));
        vq_sync_aring_used_event(sc, vq, BUS_DMASYNC_PREWRITE);
    } else {
        vq.set_avail_flags(vq.avail_flags() | VRING_AVAIL_F_NO_INTERRUPT);
    }
    vq_sync_aring(sc, vq, BUS_DMASYNC_PREWRITE);
    vq.vq_queued.set(vq.vq_queued.get() + 1);
}

/// `virtio_start_vq_intr`: start vq interrupt. No guarantee. Returns `true` if entries were
/// consumed meanwhile (the caller must process the queue).
pub fn virtio_start_vq_intr(sc: &VirtioSoftc, vq: &Virtqueue) -> bool {
    // If event index feature is negotiated, enabling interrupts is done through setting the
    // latest consumed index in the used_event field
    if virtio_has_feature(sc, VIRTIO_F_RING_EVENT_IDX) {
        vq.set_vq_used_event(vq.vq_used_idx.get());
        vq_sync_aring_used_event(sc, vq, BUS_DMASYNC_PREWRITE);
    } else {
        vq.set_avail_flags(vq.avail_flags() & !VRING_AVAIL_F_NO_INTERRUPT);
        vq_sync_aring(sc, vq, BUS_DMASYNC_PREWRITE);
    }

    virtio_membar_sync();

    vq.vq_queued.set(vq.vq_queued.get() + 1);

    vq_sync_uring(sc, vq, BUS_DMASYNC_POSTREAD);
    vq.vq_used_idx.get() != vq.used_idx()
}

/// `virtio_nused`: returns a number of slots in the used ring available to be supplied to
/// the avail ring.
pub fn virtio_nused(vq: &Virtqueue) -> i32 {
    vq_sync_uring(vq.owner(), vq, BUS_DMASYNC_POSTREAD);
    let n = vq.used_idx().wrapping_sub(vq.vq_used_idx.get());
    kdassert!(u32::from(n) <= vq.vq_num.get());

    i32::from(n)
}

/// `virtio_vq_dump` (`VIRTIO_DEBUG`).
pub fn virtio_vq_dump(vq: &Virtqueue) {
    // Common fields
    printf(format_args!(" + addr: {:p}\n", ptr::from_ref(vq)));
    if vq.vq_num.get() == 0 {
        printf(format_args!(" + vq is unused\n"));
        return;
    }
    printf(format_args!(" + vq num: {}\n", vq.vq_num.get()));
    printf(format_args!(" + vq mask: 0x{:X}\n", vq.vq_mask.get()));
    printf(format_args!(" + vq index: {}\n", vq.vq_index.get()));
    printf(format_args!(" + vq used idx: {}\n", vq.vq_used_idx.get()));
    printf(format_args!(" + vq avail idx: {}\n", vq.vq_avail_idx.get()));
    printf(format_args!(" + vq queued: {}\n", vq.vq_queued.get()));
    if VIRTIO_DEBUG >= 2 {
        for i in 0..vq.vq_num.get() as usize {
            let vd = vq.vq_desc.get();
            // SAFETY: `i` is below `vq_num`, the size of `vq_desc`.
            let desc = unsafe { ptr::read_volatile(vd_at(vd, i)) };
            printf(format_args!(
                "  D{i:<3} len:{} flags:{} next:{}\n",
                desc.len, desc.flags, desc.next
            ));
        }
    }
    // Avail ring fields
    printf(format_args!(" + avail flags: 0x{:X}\n", vq.avail_flags()));
    printf(format_args!(" + avail idx: {}\n", vq.avail_idx()));
    printf(format_args!(" + avail event: {}\n", vq.vq_avail_event()));
    if VIRTIO_DEBUG >= 2 {
        for i in 0..vq.vq_num.get() {
            printf(format_args!("  A{i:<3} idx:{}\n", vq.avail_ring(i)));
        }
    }
    // Used ring fields
    printf(format_args!(" + used flags: 0x{:X}\n", vq.used_flags()));
    printf(format_args!(" + used idx: {}\n", vq.used_idx()));
    printf(format_args!(" + used event: {}\n", vq.vq_used_event()));
    if VIRTIO_DEBUG >= 2 {
        for i in 0..vq.vq_num.get() {
            let u = vq.used_ring(i);
            printf(format_args!("  U{i:<3} id:{} len:{}\n", u.id, u.len));
        }
    }
    printf(format_args!(" +++++++++++++++++++++++++++\n"));
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of the virtio core: a queue built by hand over heap memory, a test transport
    // that counts kicks, and the device's side of the rings written by the test.

    use super::*;

    use core::cell::Cell;
    use core::ffi::c_void;
    use std::alloc::{Layout, alloc_zeroed};
    use std::boxed::Box;
    use std::thread_local;

    use crate::dev::pv::virtiovar::{VirtioAttachArgs, VirtioOps};
    use crate::machine::cpu::CpuInfo;
    use crate::machine::intr::IntrFn;

    thread_local! {
        static KICKS: Cell<u32> = const { Cell::new(0) };
        static DONE: Cell<u32> = const { Cell::new(0) };
    }

    fn t_kick(_sc: &VirtioSoftc, _idx: u16) {
        KICKS.with(|k| k.set(k.get() + 1));
    }
    fn t_r1(_sc: &VirtioSoftc, _o: i32) -> u8 {
        0
    }
    fn t_r2(_sc: &VirtioSoftc, _o: i32) -> u16 {
        0
    }
    fn t_r4(_sc: &VirtioSoftc, _o: i32) -> u32 {
        0
    }
    fn t_r8(_sc: &VirtioSoftc, _o: i32) -> u64 {
        0
    }
    fn t_w1(_sc: &VirtioSoftc, _o: i32, _v: u8) {}
    fn t_w2(_sc: &VirtioSoftc, _o: i32, _v: u16) {}
    fn t_w4(_sc: &VirtioSoftc, _o: i32, _v: u32) {}
    fn t_w8(_sc: &VirtioSoftc, _o: i32, _v: u64) {}
    fn t_qsize(_sc: &VirtioSoftc, _i: u16) -> u16 {
        QSIZE as u16
    }
    fn t_setup_queue(_sc: &VirtioSoftc, _vq: &Virtqueue, _addr: u64) {}
    fn t_setup_intrs(_sc: &VirtioSoftc) {}
    fn t_get_status(_sc: &VirtioSoftc) -> i32 {
        0
    }
    fn t_set_status(_sc: &VirtioSoftc, _s: i32) {}
    fn t_neg(_sc: &VirtioSoftc, _n: Option<&[VirtioFeatureName]>) -> Result<(), Errno> {
        Ok(())
    }
    fn t_finish(_sc: &VirtioSoftc, _va: *mut VirtioAttachArgs) -> Result<(), Errno> {
        Ok(())
    }
    fn t_poll(_arg: *mut c_void) -> i32 {
        0
    }
    fn t_barrier(_sc: &VirtioSoftc) {}
    fn t_establish(
        _sc: &VirtioSoftc,
        _va: *mut VirtioAttachArgs,
        _vec: i32,
        _ci: Option<&'static CpuInfo>,
        _f: IntrFn,
        _a: *mut c_void,
    ) -> Result<(), Errno> {
        Err(Errno::ENXIO)
    }

    static TEST_OPS: VirtioOps = VirtioOps {
        kick: t_kick,
        read_dev_cfg_1: t_r1,
        read_dev_cfg_2: t_r2,
        read_dev_cfg_4: t_r4,
        read_dev_cfg_8: t_r8,
        write_dev_cfg_1: t_w1,
        write_dev_cfg_2: t_w2,
        write_dev_cfg_4: t_w4,
        write_dev_cfg_8: t_w8,
        read_queue_size: t_qsize,
        setup_queue: t_setup_queue,
        setup_intrs: t_setup_intrs,
        get_status: t_get_status,
        set_status: t_set_status,
        neg_features: t_neg,
        attach_finish: t_finish,
        poll_intr: t_poll,
        intr_barrier: t_barrier,
        intr_establish: t_establish,
    };

    const QSIZE: usize = 8;
    const MAXNSEGS: usize = 4;

    /// A leaked, zero-filled `T` (all-zero is valid for the virtio structures).
    fn zeroed<T>(n: usize) -> *mut T {
        let layout = Layout::array::<T>(n).unwrap();
        // SAFETY: a non-zero layout.
        let p = unsafe { alloc_zeroed(layout) };
        assert!(!p.is_null());
        p.cast()
    }

    fn map(addr: usize, len: usize) -> &'static BusDmamap {
        Box::leak(Box::new(BusDmamap {
            dm_mapsize: Cell::new(len),
            dm_nsegs: Cell::new(1),
            segs: [Cell::new(BusDmaSegment {
                ds_addr: addr,
                ds_len: len,
            })],
        }))
    }

    /// A softc with one queue of `QSIZE` slots laid out as `virtio_alloc_vq` lays it out
    /// (event indexes possible, indirect tables of `MAXNSEGS` when `indirect`).
    fn fixture(features: u64, indirect: bool) -> (&'static VirtioSoftc, &'static Virtqueue) {
        // SAFETY: leaked zeroed blocks; all-zero is a valid softc and queue.
        let sc: &'static VirtioSoftc = unsafe { &*zeroed::<VirtioSoftc>(1) };
        // SAFETY: as above.
        let vq: &'static Virtqueue = unsafe { &*zeroed::<Virtqueue>(1) };
        sc.sc_ops.set(Some(&TEST_OPS));
        sc.sc_dmat
            .set(Some(crate::machine::bus::BusDmaTag::default()));
        sc.sc_active_features.set(features);
        sc.sc_vqs.set(core::ptr::from_ref(vq).cast_mut());
        sc.sc_nvqs.set(1);

        let hdrlen = 3;
        let a1 = virtqueue_align(16 * QSIZE + 2 * (hdrlen + QSIZE));
        let a2 = virtqueue_align(2 * hdrlen + 8 * QSIZE);
        let a3 = if indirect { 16 * MAXNSEGS * QSIZE } else { 0 };
        let size = a1 + a2 + a3;
        let mem = zeroed::<u64>(size / 8).cast::<u8>();

        vq.vq_owner.set(core::ptr::from_ref(sc));
        vq.vq_num.set(QSIZE as u32);
        vq.vq_mask.set(QSIZE as u32 - 1);
        vq.vq_desc.set(mem.cast());
        vq.vq_availoffset.set((16 * QSIZE) as i32);
        // SAFETY: offsets inside the `size` bytes of `mem`.
        vq.vq_avail.set(unsafe { mem.add(16 * QSIZE) }.cast());
        vq.vq_usedoffset.set(a1 as i32);
        // SAFETY: as above.
        vq.vq_used.set(unsafe { mem.add(a1) }.cast());
        if indirect {
            vq.vq_indirectoffset.set((a1 + a2) as i32);
            // SAFETY: as above.
            vq.vq_indirect.set(unsafe { mem.add(a1 + a2) }.cast());
        }
        vq.vq_vaddr.set(mem);
        vq.vq_bytesize.set(size as u32);
        vq.vq_maxnsegs
            .set(if indirect { MAXNSEGS as i32 } else { 1 });
        vq.vq_dmamap.set(Some(map(0x10_0000, size)));
        vq.vq_entries.set(zeroed::<VqEntry>(QSIZE));
        virtio_init_vq(sc, vq);
        (sc, vq)
    }

    fn desc(vq: &Virtqueue, base: *mut VringDesc, i: usize) -> VringDesc {
        let _ = vq;
        // SAFETY: `base` is one of the fixture's descriptor tables and `i` inside it.
        unsafe { ptr::read_volatile(base.add(i)) }
    }

    /// The device consumed `slot` and wrote `len` bytes: the used ring's next element.
    fn device_uses(vq: &Virtqueue, slot: u32, len: u32) {
        let u = vq.vq_used.get().cast::<u8>();
        // SAFETY: the fixture's used ring, `QSIZE` elements after the header.
        unsafe {
            let idx = ptr::read_volatile(u.add(2).cast::<u16>());
            let e = u
                .add(VRING_USED_RING + 8 * (idx as usize % QSIZE))
                .cast::<VringUsedElem>();
            ptr::write_volatile(e, VringUsedElem { id: slot, len });
            ptr::write_volatile(u.add(2).cast::<u16>(), idx.wrapping_add(1));
        }
    }

    fn free_slots(vq: &Virtqueue) -> usize {
        vq.vq_freelist.iter().count()
    }

    #[test]
    fn init_puts_every_slot_on_the_free_list_in_order() {
        let (_sc, vq) = fixture(0, false);
        let order: std::vec::Vec<u16> = vq.vq_freelist.iter().map(|e| e.qe_index.get()).collect();
        assert_eq!(order, (0..QSIZE as u16).collect::<std::vec::Vec<_>>());
        assert_eq!(vq.vq_queued.get(), 1);
    }

    #[test]
    fn a_direct_chain_is_enqueued_committed_and_dequeued() {
        let (sc, vq) = fixture(0, false);
        KICKS.with(|k| k.set(0));

        let slot = virtio_enqueue_prep(vq).unwrap();
        assert_eq!(slot, 0);
        virtio_enqueue_reserve(vq, slot, 3).unwrap();
        assert_eq!(free_slots(vq), QSIZE - 3);
        let d = vq.vq_desc.get();
        assert_eq!(desc(vq, d, 0).flags, VRING_DESC_F_NEXT);
        assert_eq!(desc(vq, d, 0).next, 1);
        assert_eq!(desc(vq, d, 1).flags, VRING_DESC_F_NEXT);
        assert_eq!(desc(vq, d, 1).next, 2);
        assert_eq!(desc(vq, d, 2).flags, 0);

        let hdr = map(0x2000, 12);
        let payload = map(0x3000, 1500);
        virtio_enqueue_p(vq, slot, hdr, 0, 12, true);
        virtio_enqueue(vq, slot, payload, true);
        let rx = map(0x4000, 2048);
        virtio_enqueue(vq, slot, rx, false);
        assert_eq!(desc(vq, d, 0).addr, 0x2000);
        assert_eq!(desc(vq, d, 0).len, 12);
        assert_eq!(desc(vq, d, 1).addr, 0x3000);
        assert_eq!(desc(vq, d, 2).flags, VRING_DESC_F_WRITE);

        virtio_enqueue_commit(sc, vq, slot, true);
        assert_eq!(vq.avail_ring(0), 0);
        assert_eq!(vq.avail_idx(), 1);
        assert_eq!(KICKS.with(Cell::get), 1);

        // Nothing used yet.
        assert_eq!(virtio_dequeue(sc, vq), Err(Errno::ENOENT));

        device_uses(vq, 0, 100);
        fn done(_vq: &Virtqueue) -> i32 {
            DONE.with(|d| d.set(d.get() + 1));
            1
        }
        vq.vq_done.set(Some(done));
        DONE.with(|d| d.set(0));
        assert_eq!(virtio_check_vqs(sc), 1);
        assert_eq!(DONE.with(Cell::get), 1);

        assert_eq!(virtio_dequeue(sc, vq), Ok((0, 100)));
        assert_eq!(virtio_dequeue_commit(vq, 0), 3);
        assert_eq!(free_slots(vq), QSIZE);
        assert_eq!(virtio_dequeue(sc, vq), Err(Errno::ENOENT));
    }

    #[test]
    fn reserve_without_enough_slots_aborts_and_frees() {
        let (_sc, vq) = fixture(0, false);
        for _ in 0..6 {
            let s = virtio_enqueue_prep(vq).unwrap();
            virtio_enqueue_reserve(vq, s, 1).unwrap();
        }
        let s = virtio_enqueue_prep(vq).unwrap();
        assert_eq!(free_slots(vq), 1);
        assert_eq!(virtio_enqueue_reserve(vq, s, 3), Err(Errno::EAGAIN));
        // The head and the one extra slot it took are back.
        assert_eq!(free_slots(vq), 2);

        let s = virtio_enqueue_prep(vq).unwrap();
        virtio_enqueue_abort(vq, s);
        assert_eq!(free_slots(vq), 2);
    }

    #[test]
    fn an_indirect_request_uses_one_ring_slot() {
        let (sc, vq) = fixture(0, true);
        // virtio_init_vq links every slot's indirect table.
        let ind = vq.vq_indirect.get();
        for j in 0..MAXNSEGS - 1 {
            assert_eq!(desc(vq, ind, MAXNSEGS + j).next, (j + 1) as u16);
        }

        let s0 = virtio_enqueue_prep(vq).unwrap();
        let slot = virtio_enqueue_prep(vq).unwrap();
        assert_eq!(slot, 1);
        virtio_enqueue_reserve(vq, slot, 3).unwrap();
        assert_eq!(free_slots(vq), QSIZE - 2);
        let d = desc(vq, vq.vq_desc.get(), 1);
        assert_eq!(d.flags, VRING_DESC_F_INDIRECT);
        assert_eq!(d.len, 48);
        assert_eq!(
            d.addr,
            0x10_0000 + vq.vq_indirectoffset.get() as u64 + (16 * MAXNSEGS) as u64
        );
        let base = vq.entry(1).qe_desc_base.get();
        assert_eq!(desc(vq, base, 0).flags, VRING_DESC_F_NEXT);
        assert_eq!(desc(vq, base, 1).flags, VRING_DESC_F_NEXT);
        assert_eq!(desc(vq, base, 2).flags, 0);

        virtio_enqueue_trim(vq, slot, 2);
        assert_eq!(desc(vq, vq.vq_desc.get(), 1).len, 32);
        assert_eq!(desc(vq, base, 1).flags, 0);

        virtio_enqueue_commit(sc, vq, slot, false);
        device_uses(vq, 1, 0);
        assert_eq!(virtio_dequeue(sc, vq), Ok((1, 0)));
        assert_eq!(virtio_dequeue_commit(vq, 1), 1);
        virtio_enqueue_abort(vq, s0);
        assert_eq!(free_slots(vq), QSIZE);
    }

    #[test]
    fn event_index_decides_the_kick() {
        let (sc, vq) = fixture(VIRTIO_F_RING_EVENT_IDX, false);
        KICKS.with(|k| k.set(0));

        // The device asked to be notified when entry 0 is published (avail_event 0).
        let s = virtio_enqueue_prep(vq).unwrap();
        virtio_enqueue_reserve(vq, s, 1).unwrap();
        virtio_enqueue_commit(sc, vq, s, true);
        assert_eq!(KICKS.with(Cell::get), 1);

        // It has not moved its event index: publishing entry 1 needs no kick.
        let s = virtio_enqueue_prep(vq).unwrap();
        virtio_enqueue_reserve(vq, s, 1).unwrap();
        virtio_enqueue_commit(sc, vq, s, true);
        assert_eq!(KICKS.with(Cell::get), 1);
        assert_eq!(vq.avail_idx(), 2);
    }

    #[test]
    fn interrupt_suppression_flags_and_event_indexes() {
        let (sc, vq) = fixture(0, false);
        virtio_stop_vq_intr(sc, vq);
        assert_eq!(
            vq.avail_flags() & VRING_AVAIL_F_NO_INTERRUPT,
            VRING_AVAIL_F_NO_INTERRUPT
        );
        assert!(!virtio_start_vq_intr(sc, vq));
        assert_eq!(vq.avail_flags() & VRING_AVAIL_F_NO_INTERRUPT, 0);
        device_uses(vq, 0, 0);
        assert!(virtio_start_vq_intr(sc, vq));

        let (sc, vq) = fixture(VIRTIO_F_RING_EVENT_IDX, false);
        virtio_stop_vq_intr(sc, vq);
        assert_eq!(vq.vq_used_event(), 0x8000);
        assert!(!virtio_start_vq_intr(sc, vq));
        assert_eq!(vq.vq_used_event(), 0);
        assert!(!virtio_postpone_intr(vq, 3));
        assert_eq!(vq.vq_used_event(), 3);
        device_uses(vq, 0, 0);
        device_uses(vq, 1, 0);
        assert_eq!(virtio_nused(vq), 2);
        assert!(virtio_postpone_intr(vq, 1));
    }

    #[test]
    fn device_names() {
        assert_eq!(virtio_device_string(1), "Network");
        assert_eq!(virtio_device_string(2), "Block");
        assert_eq!(virtio_device_string(0), "Unknown (0)");
        assert_eq!(virtio_device_string(12), "(null)");
        assert_eq!(virtio_device_string(16), "GPU");
        assert_eq!(virtio_device_string(17), "Unknown");
        assert_eq!(virtio_device_string(-1), "Unknown");
    }
}
/* </TESTS> */
