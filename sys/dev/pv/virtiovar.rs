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
 * Copyright (c) 2012 Stefan Fritsch.
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

/*
 * Part of the file derived from `Virtio PCI Card Specification v0.8.6 DRAFT'
 * Appendix A.
 */
/* An interface for efficient virtio implementation.
 *
 * This header is BSD licensed so anyone can use the definitions
 * to implement compatible drivers/servers.
 *
 * Copyright 2007, 2009, IBM Corporation
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
 * 3. Neither the name of IBM nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL IBM OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/pv/virtiovar.h>`: the virtio core's types, shared by the transports
//! (`virtio_pci`, `virtio_mmio`) and the device drivers (`vio`).
//!
//! Upstream: sys/dev/pv/virtiovar.h @ 3ce1f3f79392
//!
//! A transport attaches as `virtio*` and fills in a [`VirtioSoftc`] (`sc_dmat`, `sc_ops`);
//! its child driver sets `sc_ipl`, `sc_nvqs`/`sc_vqs`, `sc_child` and `sc_config_change`,
//! negotiates features, allocates its queues with `virtio_alloc_vq` and finishes with
//! `virtio_attach_finish` (`dev/pv/virtio.rs`).
//!
//! ## Deviations
//! - The structures are `Cell`s (all-zero valid, as the C's `M_ZERO` allocations need): the
//!   transport, the driver and the interrupt handlers change them through shared pointers.
//! - The ring pointers (`vq_desc`, `vq_avail`, `vq_used`, `vq_indirect`) point into DMA
//!   memory the device reads and writes; they are read and written only with volatile
//!   accesses, through the accessor methods of [`Virtqueue`] (the avail and used rings, the
//!   event indexes `VQ_USED_EVENT`/`VQ_AVAIL_EVENT`) and `dev/pv/virtio.rs` (descriptors).
//! - `struct virtio_ops` is a table of Rust `fn` pointers taking `&VirtioSoftc`; the ones that
//!   return 0 or an errno return `Result` (`attach_finish`'s `-EIO` is `Err(EIO)`), and the
//!   `const struct virtio_feature_name *` table is `Option<&[VirtioFeatureName]>` (NULL is
//!   `None`, the C's `{ 0, NULL }` terminator is the slice's end).
//! - `sc_child` keeps the C's three values: NULL, a device, or [`VIRTIO_CHILD_ERROR`] (`(void
//!   *)1`); [`VirtioSoftc::child`] hands out the device only.
//! - `VIRTIO_DEBUG` is the constant 0 as in C; the code it guards is compiled and type-checked
//!   behind `if VIRTIO_DEBUG > 0` instead of `#if`.
//! - The access macros (`virtio_read_device_config_1`, `virtio_negotiate_features`, ...) and
//!   `virtio_has_feature` are functions.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::dev::pv::virtioreg::{
    VRING_AVAIL_RING, VRING_USED_RING, VringAvail, VringDesc, VringUsed, VringUsedElem,
};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{BusDmaSegment, BusDmaTag, BusDmamap};
use crate::machine::cpu::CpuInfo;
use crate::machine::intr::IntrFn;
use crate::queue_adapter;
use crate::sys::device::{Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::queue::{SlistEntry, SlistHead};

/// `VIRTIO_DEBUG`: 0, as the C defines it unless the build says otherwise.
pub const VIRTIO_DEBUG: i32 = 0;

// Flags for config(8).

/// `VIRTIO_CF_NO_INDIRECT`.
pub const VIRTIO_CF_NO_INDIRECT: i32 = 1;
/// `VIRTIO_CF_NO_EVENT_IDX`.
pub const VIRTIO_CF_NO_EVENT_IDX: i32 = 2;
/// `VIRTIO_CF_PREFER_VERSION_09`.
pub const VIRTIO_CF_PREFER_VERSION_09: i32 = 8;

/// `struct virtio_attach_args`: what a virtio child driver is attached with. The transports
/// pass a larger structure that begins with it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtioAttachArgs {
    /// `va_devid`: virtio device id.
    pub va_devid: i32,
    /// `va_nintr`: number of intr vectors.
    pub va_nintr: u32,
}

/// `struct vq_entry`: one descriptor slot of a virtqueue.
#[repr(C)]
pub struct VqEntry {
    /// `qe_list`: free list.
    pub qe_list: SlistEntry<VqEntry>,
    /// `qe_index`: index in `vq_desc` array.
    pub qe_index: Cell<u16>,
    // The following are used only in the `head' entry.
    /// `qe_next`: next enq slot.
    pub qe_next: Cell<i16>,
    /// `qe_indirect`: 1 if using indirect.
    pub qe_indirect: Cell<i16>,
    /// `qe_vr_index`: index in `sc_reqs` array.
    pub qe_vr_index: Cell<i16>,
    /// `qe_desc_base`: pointer to vd array.
    pub qe_desc_base: Cell<*mut VringDesc>,
}

queue_adapter!(
    /// `SLIST_ENTRY(vq_entry) qe_list`: `vq_freelist`.
    pub VqFreelist: VqEntry, qe_list => SlistEntry<VqEntry>
);

/// The type of `vq_done`: `int (*)(struct virtqueue *)`.
pub type VqDoneFn = fn(&Virtqueue) -> i32;

/// `struct virtqueue`.
///
/// Allocated zeroed (the driver's `mallocarray(..., M_ZERO)`), so every member is valid as
/// all-zero bits: null pointers, `None`, 0.
pub struct Virtqueue {
    /// `vq_owner`.
    pub vq_owner: Cell<*const VirtioSoftc>,
    /// `vq_num`: queue size (# of entries), 0 if unused/non-existent.
    pub vq_num: Cell<u32>,
    /// `vq_mask`: `vq_num - 1`.
    pub vq_mask: Cell<u32>,
    /// `vq_index`: queue number (0, 1, ...).
    pub vq_index: Cell<i32>,

    // vring pointers (KVA)
    /// `vq_desc`.
    pub vq_desc: Cell<*mut VringDesc>,
    /// `vq_avail`.
    pub vq_avail: Cell<*mut VringAvail>,
    /// `vq_used`.
    pub vq_used: Cell<*mut VringUsed>,
    /// `vq_indirect`.
    pub vq_indirect: Cell<*mut VringDesc>,

    // virtqueue allocation info
    /// `vq_vaddr`.
    pub vq_vaddr: Cell<*mut u8>,
    /// `vq_availoffset`.
    pub vq_availoffset: Cell<i32>,
    /// `vq_usedoffset`.
    pub vq_usedoffset: Cell<i32>,
    /// `vq_indirectoffset`.
    pub vq_indirectoffset: Cell<i32>,
    /// `vq_segs`.
    pub vq_segs: [Cell<BusDmaSegment>; 1],
    /// `vq_bytesize`.
    pub vq_bytesize: Cell<u32>,
    /// `vq_dmamap`.
    pub vq_dmamap: Cell<Option<&'static BusDmamap>>,

    /// `vq_maxnsegs`.
    pub vq_maxnsegs: Cell<i32>,

    // free entry management
    /// `vq_entries`: `vq_num` entries.
    pub vq_entries: Cell<*mut VqEntry>,
    /// `vq_freelist`.
    pub vq_freelist: SlistHead<VqFreelist>,

    // enqueue/dequeue status
    /// `vq_avail_idx`.
    pub vq_avail_idx: Cell<u16>,
    /// `vq_used_idx`.
    pub vq_used_idx: Cell<u16>,
    /// `vq_queued`.
    pub vq_queued: Cell<i32>,

    /// `vq_done`: interrupt handler.
    pub vq_done: Cell<Option<VqDoneFn>>,
    /// `vq_notify_off`: 1.x only: offset for notify address calculation.
    pub vq_notify_off: Cell<u32>,
    /// `vq_intr_vec`.
    pub vq_intr_vec: Cell<i32>,
}

impl Virtqueue {
    /// `memset(vq, 0, sizeof(*vq))`.
    pub fn clear(&self) {
        self.vq_owner.set(ptr::null());
        self.vq_num.set(0);
        self.vq_mask.set(0);
        self.vq_index.set(0);
        self.vq_desc.set(ptr::null_mut());
        self.vq_avail.set(ptr::null_mut());
        self.vq_used.set(ptr::null_mut());
        self.vq_indirect.set(ptr::null_mut());
        self.vq_vaddr.set(ptr::null_mut());
        self.vq_availoffset.set(0);
        self.vq_usedoffset.set(0);
        self.vq_indirectoffset.set(0);
        self.vq_segs[0].set(BusDmaSegment::default());
        self.vq_bytesize.set(0);
        self.vq_dmamap.set(None);
        self.vq_maxnsegs.set(0);
        self.vq_entries.set(ptr::null_mut());
        self.vq_freelist.init();
        self.vq_avail_idx.set(0);
        self.vq_used_idx.set(0);
        self.vq_queued.set(0);
        self.vq_done.set(None);
        self.vq_notify_off.set(0);
        self.vq_intr_vec.set(0);
    }

    /// `vq->vq_owner`.
    pub fn owner(&self) -> &VirtioSoftc {
        let sc = self.vq_owner.get();
        if sc.is_null() {
            panic(format_args!("virtqueue without an owner"));
        }
        // SAFETY: `virtio_alloc_vq` points `vq_owner` at the softc whose `sc_vqs` holds this
        // queue; a softc outlives its queues (it is only freed by `config_detach`, after the
        // child freed them).
        unsafe { &*sc }
    }

    /// `vq->vq_dmamap`, which every allocated queue has.
    pub fn dmamap(&self) -> &'static BusDmamap {
        match self.vq_dmamap.get() {
            Some(map) => map,
            None => panic(format_args!("virtqueue without a dmamap")),
        }
    }

    /// `&vq->vq_entries[slot]`.
    pub fn entry(&self, slot: usize) -> &VqEntry {
        let base = self.vq_entries.get();
        if base.is_null() || slot >= self.vq_num.get() as usize {
            panic(format_args!("virtqueue: bad slot {slot}"));
        }
        // SAFETY: `virtio_alloc_vq` allocated `vq_num` zeroed entries (valid `VqEntry`s) and
        // they stay until `virtio_free_vq`; `slot` is below `vq_num`.
        unsafe { &*base.add(slot) }
    }

    /// The ring memory must be allocated for the accessors below.
    fn ring<T>(p: *mut T) -> *mut T {
        if p.is_null() {
            panic(format_args!("virtqueue: ring not allocated"));
        }
        p
    }

    /// `vq->vq_avail->flags`.
    pub fn avail_flags(&self) -> u16 {
        let a = Self::ring(self.vq_avail.get());
        // SAFETY: `vq_avail` points at the avail ring inside the queue's DMA memory, mapped
        // until `virtio_free_vq`; the device may access it concurrently, hence volatile.
        unsafe { ptr::read_volatile(&raw const (*a).flags) }
    }

    /// `vq->vq_avail->flags = v`.
    pub fn set_avail_flags(&self, v: u16) {
        let a = Self::ring(self.vq_avail.get());
        // SAFETY: as for `avail_flags`.
        unsafe { ptr::write_volatile(&raw mut (*a).flags, v) }
    }

    /// `vq->vq_avail->idx`.
    pub fn avail_idx(&self) -> u16 {
        let a = Self::ring(self.vq_avail.get());
        // SAFETY: as for `avail_flags`.
        unsafe { ptr::read_volatile(&raw const (*a).idx) }
    }

    /// `vq->vq_avail->idx = v`.
    pub fn set_avail_idx(&self, v: u16) {
        let a = Self::ring(self.vq_avail.get());
        // SAFETY: as for `avail_flags`.
        unsafe { ptr::write_volatile(&raw mut (*a).idx, v) }
    }

    /// The address of `vq->vq_avail->ring[i]`, `i` up to `vq_num` (the slot after the ring is
    /// `used_event`).
    fn avail_ring_slot(&self, i: u32) -> *mut u16 {
        if i > self.vq_num.get() {
            panic(format_args!("virtqueue: bad avail ring index {i}"));
        }
        let a = Self::ring(self.vq_avail.get()).cast::<u8>();
        // SAFETY: the avail ring is `VRING_AVAIL_RING + 2 * (vq_num + 1)` bytes inside the
        // queue's allocation (`virtio_alloc_vq` sizes it with `hdrlen` 3 when event
        // indexes may be used, and the page rounding covers the slot otherwise).
        unsafe { a.add(VRING_AVAIL_RING + 2 * i as usize).cast::<u16>() }
    }

    /// `vq->vq_avail->ring[i]`.
    pub fn avail_ring(&self, i: u32) -> u16 {
        // SAFETY: a slot inside the ring (`avail_ring_slot`), accessed volatile.
        unsafe { ptr::read_volatile(self.avail_ring_slot(i)) }
    }

    /// `vq->vq_avail->ring[i] = v`.
    pub fn set_avail_ring(&self, i: u32, v: u16) {
        // SAFETY: as for `avail_ring`.
        unsafe { ptr::write_volatile(self.avail_ring_slot(i), v) }
    }

    /// `VQ_USED_EVENT(vq)`: the used event index the driver publishes at the end of the
    /// avail ring.
    pub fn vq_used_event(&self) -> u16 {
        self.avail_ring(self.vq_num.get())
    }

    /// `VQ_USED_EVENT(vq) = v`.
    pub fn set_vq_used_event(&self, v: u16) {
        self.set_avail_ring(self.vq_num.get(), v)
    }

    /// `vq->vq_used->flags`.
    pub fn used_flags(&self) -> u16 {
        let u = Self::ring(self.vq_used.get());
        // SAFETY: `vq_used` points at the used ring inside the queue's DMA memory, which the
        // device writes; read volatile.
        unsafe { ptr::read_volatile(&raw const (*u).flags) }
    }

    /// `vq->vq_used->idx`: the device's free-running index.
    pub fn used_idx(&self) -> u16 {
        let u = Self::ring(self.vq_used.get());
        // SAFETY: as for `used_flags`.
        unsafe { ptr::read_volatile(&raw const (*u).idx) }
    }

    /// `vq->vq_used->ring[i]`, `i` below `vq_num`.
    pub fn used_ring(&self, i: u32) -> VringUsedElem {
        if i >= self.vq_num.get() {
            panic(format_args!("virtqueue: bad used ring index {i}"));
        }
        let u = Self::ring(self.vq_used.get()).cast::<u8>();
        // SAFETY: the used ring holds `vq_num` elements after its header, inside the queue's
        // allocation (`allocsize2`); 4-byte aligned like the ring; read volatile.
        unsafe {
            ptr::read_volatile(
                u.add(VRING_USED_RING + size_of::<VringUsedElem>() * i as usize)
                    .cast::<VringUsedElem>(),
            )
        }
    }

    /// `VQ_AVAIL_EVENT(vq)`: the avail event index the device publishes at the end of the
    /// used ring.
    pub fn vq_avail_event(&self) -> u16 {
        let u = Self::ring(self.vq_used.get()).cast::<u8>();
        let off = VRING_USED_RING + size_of::<VringUsedElem>() * self.vq_num.get() as usize;
        // SAFETY: the used ring is allocated with `hdrlen` (3 with event indexes) 16-bit
        // words besides the elements, and page rounded: the word after the last element is
        // inside the allocation; read volatile.
        unsafe { ptr::read_volatile(u.add(off).cast::<u16>()) }
    }
}

// SAFETY: a virtqueue is reached from its softc under the driver's mutexes, or at splnet
// under the kernel lock, as in C.
unsafe impl Sync for Virtqueue {}

/// `struct virtio_feature_name`.
#[derive(Clone, Copy, Debug)]
pub struct VirtioFeatureName {
    /// `bit`.
    pub bit: u64,
    /// `name`.
    pub name: &'static str,
}

/// The type of `virtio_ops.intr_establish`: `(sc, va, vec, ci, func, arg)`.
pub type VirtioIntrEstablishFn = fn(
    &VirtioSoftc,
    *mut VirtioAttachArgs,
    i32,
    Option<&'static CpuInfo>,
    IntrFn,
    *mut c_void,
) -> Result<(), Errno>;

/// `struct virtio_ops`: what the transport does for the core and the child driver.
pub struct VirtioOps {
    /// `kick`: notify the device of new buffers in queue `idx`.
    pub kick: fn(&VirtioSoftc, u16),
    /// `read_dev_cfg_1`.
    pub read_dev_cfg_1: fn(&VirtioSoftc, i32) -> u8,
    /// `read_dev_cfg_2`.
    pub read_dev_cfg_2: fn(&VirtioSoftc, i32) -> u16,
    /// `read_dev_cfg_4`.
    pub read_dev_cfg_4: fn(&VirtioSoftc, i32) -> u32,
    /// `read_dev_cfg_8`.
    pub read_dev_cfg_8: fn(&VirtioSoftc, i32) -> u64,
    /// `write_dev_cfg_1`.
    pub write_dev_cfg_1: fn(&VirtioSoftc, i32, u8),
    /// `write_dev_cfg_2`.
    pub write_dev_cfg_2: fn(&VirtioSoftc, i32, u16),
    /// `write_dev_cfg_4`.
    pub write_dev_cfg_4: fn(&VirtioSoftc, i32, u32),
    /// `write_dev_cfg_8`.
    pub write_dev_cfg_8: fn(&VirtioSoftc, i32, u64),
    /// `read_queue_size`.
    pub read_queue_size: fn(&VirtioSoftc, u16) -> u16,
    /// `setup_queue`: tell the device where the queue's rings are (0: no queue).
    pub setup_queue: fn(&VirtioSoftc, &Virtqueue, u64),
    /// `setup_intrs`.
    pub setup_intrs: fn(&VirtioSoftc),
    /// `get_status`.
    pub get_status: fn(&VirtioSoftc) -> i32,
    /// `set_status`.
    pub set_status: fn(&VirtioSoftc, i32),
    /// `neg_features`.
    pub neg_features: fn(&VirtioSoftc, Option<&[VirtioFeatureName]>) -> Result<(), Errno>,
    /// `attach_finish`: the transport's part of `virtio_attach_finish` (interrupts); `va` is
    /// the transport's own attach arguments, which begin with a [`VirtioAttachArgs`].
    pub attach_finish: fn(&VirtioSoftc, *mut VirtioAttachArgs) -> Result<(), Errno>,
    /// `poll_intr`: the interrupt handler to call when polling; its argument is the softc.
    pub poll_intr: IntrFn,
    /// `intr_barrier`.
    pub intr_barrier: fn(&VirtioSoftc),
    /// `intr_establish(sc, va, vec, ci, func, arg)`.
    pub intr_establish: VirtioIntrEstablishFn,
}

/// `VIRTIO_CHILD_ERROR`: the child's attach failed (`sc_child`).
pub const VIRTIO_CHILD_ERROR: *const Device = ptr::without_provenance(1);

/// The type of `sc_config_change`.
pub type ConfigChangeFn = fn(&VirtioSoftc) -> i32;

/// `struct virtio_softc`: what every transport's softc begins with.
#[repr(C)]
pub struct VirtioSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_dmat`: set by transport.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_ops`: set by transport.
    pub sc_ops: Cell<Option<&'static VirtioOps>>,

    /// `sc_ipl`: set by child.
    pub sc_ipl: Cell<i32>,

    /// `sc_driver_features`.
    pub sc_driver_features: Cell<u64>,
    /// `sc_active_features`.
    pub sc_active_features: Cell<u64>,
    /// `sc_indirect`.
    pub sc_indirect: Cell<i32>,
    /// `sc_version_1`.
    pub sc_version_1: Cell<i32>,

    /// `sc_nvqs`: size of `sc_vqs`, set by child.
    pub sc_nvqs: Cell<i32>,
    /// `sc_vqs`: set by child.
    pub sc_vqs: Cell<*mut Virtqueue>,

    /// `sc_child`: set by child, [`VIRTIO_CHILD_ERROR`] on error.
    pub sc_child: Cell<*const Device>,
    /// `sc_config_change`: set by child.
    pub sc_config_change: Cell<Option<ConfigChangeFn>>,
}

impl VirtioSoftc {
    /// `sc->sc_ops`, which the transport sets before anything uses it.
    pub fn ops(&self) -> &'static VirtioOps {
        match self.sc_ops.get() {
            Some(ops) => ops,
            None => panic(format_args!("virtio: no transport ops")),
        }
    }

    /// `sc->sc_dmat`, which the transport sets at attach.
    pub fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => panic(format_args!("virtio: no dma tag")),
        }
    }

    /// `sc->sc_vqs[0 .. sc_nvqs]`.
    pub fn vqs(&self) -> &[Virtqueue] {
        let base = self.sc_vqs.get();
        let n = self.sc_nvqs.get();
        if base.is_null() || n <= 0 {
            return &[];
        }
        // SAFETY: the child sets `sc_vqs` to a zeroed `mallocarray` of `sc_nvqs` queues
        // (valid all-zero) and sets both back to NULL/0 before freeing it.
        unsafe { core::slice::from_raw_parts(base, n as usize) }
    }

    /// `&sc->sc_vqs[i]`.
    pub fn vq(&self, i: usize) -> &Virtqueue {
        match self.vqs().get(i) {
            Some(vq) => vq,
            None => panic(format_args!("virtio: no virtqueue {i}")),
        }
    }

    /// `sc->sc_child` when it is a device (neither NULL nor [`VIRTIO_CHILD_ERROR`]).
    pub fn child(&self) -> Option<&Device> {
        let c = self.sc_child.get();
        if c.is_null() || c == VIRTIO_CHILD_ERROR {
            return None;
        }
        // SAFETY: the child stored its own `struct device`, which lives as long as the
        // parent (a parent is detached after its children).
        Some(unsafe { &*c })
    }
}

// SAFETY: `#[repr(C)]` with the device first; every other member is a `Cell` of an integer,
// a pointer or an `Option` of a reference or a `fn`, all valid as zero bits.
unsafe impl Softc for VirtioSoftc {}

// The public interface: the macros of the header.

/// `virtio_read_device_config_1(sc, o)`.
pub fn virtio_read_device_config_1(sc: &VirtioSoftc, o: i32) -> u8 {
    (sc.ops().read_dev_cfg_1)(sc, o)
}

/// `virtio_read_device_config_2(sc, o)`.
pub fn virtio_read_device_config_2(sc: &VirtioSoftc, o: i32) -> u16 {
    (sc.ops().read_dev_cfg_2)(sc, o)
}

/// `virtio_read_device_config_4(sc, o)`.
pub fn virtio_read_device_config_4(sc: &VirtioSoftc, o: i32) -> u32 {
    (sc.ops().read_dev_cfg_4)(sc, o)
}

/// `virtio_read_device_config_8(sc, o)`.
pub fn virtio_read_device_config_8(sc: &VirtioSoftc, o: i32) -> u64 {
    (sc.ops().read_dev_cfg_8)(sc, o)
}

/// `virtio_write_device_config_1(sc, o, v)`.
pub fn virtio_write_device_config_1(sc: &VirtioSoftc, o: i32, v: u8) {
    (sc.ops().write_dev_cfg_1)(sc, o, v)
}

/// `virtio_write_device_config_2(sc, o, v)`.
pub fn virtio_write_device_config_2(sc: &VirtioSoftc, o: i32, v: u16) {
    (sc.ops().write_dev_cfg_2)(sc, o, v)
}

/// `virtio_write_device_config_4(sc, o, v)`.
pub fn virtio_write_device_config_4(sc: &VirtioSoftc, o: i32, v: u32) {
    (sc.ops().write_dev_cfg_4)(sc, o, v)
}

/// `virtio_write_device_config_8(sc, o, v)`.
pub fn virtio_write_device_config_8(sc: &VirtioSoftc, o: i32, v: u64) {
    (sc.ops().write_dev_cfg_8)(sc, o, v)
}

/// `virtio_read_queue_size(sc, i)`.
pub fn virtio_read_queue_size(sc: &VirtioSoftc, i: u16) -> u16 {
    (sc.ops().read_queue_size)(sc, i)
}

/// `virtio_setup_queue(sc, vq, addr)`.
pub fn virtio_setup_queue(sc: &VirtioSoftc, vq: &Virtqueue, addr: u64) {
    (sc.ops().setup_queue)(sc, vq, addr)
}

/// `virtio_negotiate_features(sc, guest_feature_names)`.
pub fn virtio_negotiate_features(
    sc: &VirtioSoftc,
    guest_feature_names: Option<&[VirtioFeatureName]>,
) -> Result<(), Errno> {
    (sc.ops().neg_features)(sc, guest_feature_names)
}

/// `virtio_poll_intr(sc)`.
pub fn virtio_poll_intr(sc: &VirtioSoftc) -> i32 {
    (sc.ops().poll_intr)(ptr::from_ref(sc).cast_mut().cast())
}

/// `virtio_get_status(sc)`.
pub fn virtio_get_status(sc: &VirtioSoftc) -> i32 {
    (sc.ops().get_status)(sc)
}

/// `virtio_set_status(sc, i)`.
pub fn virtio_set_status(sc: &VirtioSoftc, i: i32) {
    (sc.ops().set_status)(sc, i)
}

/// `virtio_intr_barrier(sc)`.
pub fn virtio_intr_barrier(sc: &VirtioSoftc) {
    (sc.ops().intr_barrier)(sc)
}

/// `virtio_intr_establish(sc, va, v, ci, fn, a)`. It only works if `va_nintr > 1`. If it is
/// called by a child driver, the transport driver will skip automatic intr allocation and the
/// child driver must allocate all required interrupts itself. Vector 0 is always used for
/// the config change interrupt.
pub fn virtio_intr_establish(
    sc: &VirtioSoftc,
    va: *mut VirtioAttachArgs,
    v: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    a: *mut c_void,
) -> Result<(), Errno> {
    (sc.ops().intr_establish)(sc, va, v, ci, func, a)
}

/// `virtio_device_reset(sc)`: only for transport drivers.
pub fn virtio_device_reset(sc: &VirtioSoftc) {
    virtio_set_status(sc, 0)
}

/// `virtio_has_feature(sc, fbit)`.
#[inline]
pub fn virtio_has_feature(sc: &VirtioSoftc, fbit: u64) -> bool {
    sc.sc_active_features.get() & fbit != 0
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/pv/virtiovar.h");
        crate::reftest::assert_defines!(defs;
            VIRTIO_DEBUG, VIRTIO_CF_NO_INDIRECT, VIRTIO_CF_NO_EVENT_IDX,
            VIRTIO_CF_PREFER_VERSION_09,
        );
    }

    #[test]
    fn child_error_is_neither_null_nor_a_device() {
        let sc_child = Cell::new(VIRTIO_CHILD_ERROR);
        assert!(!sc_child.get().is_null());
        assert_eq!(sc_child.get().addr(), 1);
    }
}
/* </TESTS> */
