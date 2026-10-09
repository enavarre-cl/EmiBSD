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
 * Copyright (c) 2014 David Gwynne <dlg@openbsd.org>
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
//! `<dev/ic/nvmevar.h>`: the nvme(4) driver's state: DMA memory, command control blocks,
//! queues, the controller operations and the softc shared by the bus front-ends
//! (`nvme_pci`, and `aplns` on Apple silicon).
//!
//! Upstream: sys/dev/ic/nvmevar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct nvme_dmamem`, `struct nvme_queue` and the ccb array are `malloc(9)`ed as in C
//!   and written whole once their members are known (`nvme_dmamem_alloc`, `nvme_q_alloc`,
//!   `nvme_ccbs_alloc`); they are reached as `&'static` until their `*_free` function takes
//!   them back. Their members that never change after that are plain fields.
//! - `ccb_cookie` (`void *`) is [`NvmeCookie`], an enum of what the C points it at; the
//!   command and the poll state the C keeps on the caller's stack are held by value, so no
//!   pointer to a stack frame outlives it. `ccb_done` is a `Cell` of a function pointer.
//! - `ccb_prpl` stays a raw pointer into the PRP list memory (`sc_ccb_prpls`), written
//!   volatile by `nvme_scsi_io`.
//! - The softc carries `sc_nccbs`, the length of `sc_ccbs`, which the C does not keep:
//!   `nvme_q_complete` checks a completion's command identifier against it instead of
//!   indexing past the array.
//! - `sc_identify` is an `UnsafeCell` written once by `nvme_identify` during attach (4 KiB:
//!   it is copied in place, never through the stack); `sc_namespaces` is the C's
//!   `mallocarray`ed array of `sc_nn + 1` entries, behind the bounds-checked
//!   [`NvmeSoftc::namespace`].
//! - The `nvme_ops` members the C calls with a NULL ccb (the hibernate path) take an
//!   `Option<&NvmeCcb>`; `op_q_alloc` returns a `Result`.
//! - `NVME_DMA_MAP`, `NVME_DMA_LEN`, `NVME_DMA_DVA`, `NVME_DMA_KVA`, `nvme_read4`,
//!   `nvme_write4` and `nvme_barrier` are inline functions; `DEVNAME(sc)` is
//!   `sc.sc_dev.xname()`.

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::ptr::NonNull;

use crate::dev::ic::nvme::NvmePollState;
use crate::dev::ic::nvmereg::{NvmIdentifyController, NvmIdentifyNamespace, NvmeSqe};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BusDmaSegment, BusDmaTag, BusDmamap, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_barrier,
    bus_space_read_4, bus_space_write_4,
};
use crate::queue_adapter;
use crate::scsi::scsiconf::{ScsiIopool, ScsiXfer, ScsibusSoftc};
use crate::sys::device::{Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::mutex::Mutex;
use crate::sys::queue::{SimpleqEntry, SimpleqHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::sensors::{Ksensor, Ksensordev};

/// `NVME_IO_Q`: the I/O queue's identifier.
pub const NVME_IO_Q: u16 = 1;
/// `NVME_HIB_Q`: the hibernate queue's identifier.
pub const NVME_HIB_Q: u16 = 2;
/// `NVME_MAXPHYS`: the largest transfer the driver issues.
pub const NVME_MAXPHYS: usize = 128 * 1024;

/// `struct nvme_dmamem`: one physically contiguous, mapped and loaded DMA area.
pub struct NvmeDmamem {
    /// `ndm_map`.
    pub ndm_map: &'static BusDmamap,
    /// `ndm_seg`.
    pub ndm_seg: BusDmaSegment,
    /// `ndm_size`.
    pub ndm_size: usize,
    /// `ndm_kva`.
    pub ndm_kva: NonNull<u8>,
}

/// `ccb_done`: what completes a command.
pub type NvmeDoneFn = fn(sc: &NvmeSoftc, ccb: &NvmeCcb);

/// The `fill` argument of `nvme_q_submit` and `nvme_poll`: writes a command into a zeroed
/// submission queue entry (`void *slot`).
pub type NvmeFillFn = fn(sc: &NvmeSoftc, ccb: &NvmeCcb, slot: &mut NvmeSqe);

/// `ccb_cookie`: what the command's fill and done functions work on.
#[derive(Clone, Copy)]
pub enum NvmeCookie {
    /// No cookie.
    None,
    /// A prepared command, copied into the queue by `nvme_sqe_fill`.
    Sqe(NvmeSqe),
    /// The DMA memory an identify command fills (`nvme_fill_identify`).
    Dmamem(&'static NvmeDmamem),
    /// A SCSI transfer (`nvme_scsi_io`, `nvme_scsi_sync`).
    Xs(&'static ScsiXfer),
    /// `nvme_poll`'s state: the command and, once done, the completion's flags.
    Poll(NvmePollState),
}

/// `struct nvme_ccb`: a command control block; its index in `sc_ccbs` is the command
/// identifier.
pub struct NvmeCcb {
    /// `ccb_entry`: `sc_ccb_list`, and `nvme_q_complete`'s done list.
    pub ccb_entry: SimpleqEntry<NvmeCcb>,

    /// `ccb_dmamap`: the data of a transfer.
    pub ccb_dmamap: &'static BusDmamap,

    /// `ccb_cookie`.
    pub ccb_cookie: Cell<NvmeCookie>,
    /// `ccb_done`.
    pub ccb_done: Cell<NvmeDoneFn>,

    /// `ccb_prpl_off`: the offset of this ccb's PRP list in `sc_ccb_prpls`.
    pub ccb_prpl_off: usize,
    /// `ccb_prpl_dva`: its device address.
    pub ccb_prpl_dva: u64,
    /// `ccb_prpl`: its kernel address (`sc_max_prpl` entries).
    pub ccb_prpl: *mut u64,

    /// `ccb_id`.
    pub ccb_id: u16,
    /// `ccb_cqe_flags`: the flags of the completion.
    pub ccb_cqe_flags: Cell<u16>,
}

queue_adapter!(
    /// `SIMPLEQ_HEAD(nvme_ccb_list, nvme_ccb)`, through `ccb_entry`.
    pub NvmeCcbList: NvmeCcb, ccb_entry => SimpleqEntry<NvmeCcb>
);

/// `struct nvme_queue`: a submission queue and its completion queue.
pub struct NvmeQueue {
    /// `q_sq_mtx`: the submission side.
    pub q_sq_mtx: Mutex,
    /// `q_cq_mtx`: the completion side.
    pub q_cq_mtx: Mutex,
    /// `q_sq_dmamem`.
    pub q_sq_dmamem: &'static NvmeDmamem,
    /// `q_cq_dmamem`.
    pub q_cq_dmamem: &'static NvmeDmamem,
    /// `q_nvmmu_dmamem`: for aplns(4).
    pub q_nvmmu_dmamem: Cell<Option<&'static NvmeDmamem>>,
    /// `q_sqtdbl`: submission queue tail doorbell.
    pub q_sqtdbl: BusSize,
    /// `q_cqhdbl`: completion queue head doorbell.
    pub q_cqhdbl: BusSize,
    /// `q_id`.
    pub q_id: u16,
    /// `q_entries`.
    pub q_entries: u32,
    /// `q_sq_tail`. Protected by `q_sq_mtx`.
    pub q_sq_tail: Cell<u32>,
    /// `q_cq_head`. Protected by `q_cq_mtx`.
    pub q_cq_head: Cell<u32>,
    /// `q_cq_phase`. Protected by `q_cq_mtx`.
    pub q_cq_phase: Cell<u16>,
}

/// `struct nvme_namespace`.
pub struct NvmeNamespace {
    /// `ident`: the namespace's identify data, `malloc`ed by `nvme_scsi_probe`.
    pub ident: Cell<Option<NonNull<NvmIdentifyNamespace>>>,
}

/// `op_q_alloc` of [`NvmeOps`]: a front-end's own queue setup.
pub type NvmeQAllocFn = fn(sc: &NvmeSoftc, q: &NvmeQueue) -> Result<(), Errno>;

/// `struct nvme_ops`: what a bus front-end may override.
pub struct NvmeOps {
    /// `op_enable`.
    pub op_enable: Option<fn(sc: &NvmeSoftc)>,

    /// `op_q_alloc`.
    pub op_q_alloc: Option<NvmeQAllocFn>,
    /// `op_q_free`.
    pub op_q_free: Option<fn(sc: &NvmeSoftc, q: &NvmeQueue)>,

    /// `op_sq_enter`: the tail slot to fill, with the submission side locked.
    pub op_sq_enter: fn(sc: &NvmeSoftc, q: &NvmeQueue, ccb: Option<&NvmeCcb>) -> u32,
    /// `op_sq_leave`: rings the doorbell and unlocks.
    pub op_sq_leave: fn(sc: &NvmeSoftc, q: &NvmeQueue, ccb: Option<&NvmeCcb>),
    /// `op_sq_enter_locked`.
    pub op_sq_enter_locked: fn(sc: &NvmeSoftc, q: &NvmeQueue, ccb: Option<&NvmeCcb>) -> u32,
    /// `op_sq_leave_locked`.
    pub op_sq_leave_locked: fn(sc: &NvmeSoftc, q: &NvmeQueue, ccb: Option<&NvmeCcb>),

    /// `op_cq_done`.
    pub op_cq_done: fn(sc: &NvmeSoftc, q: &NvmeQueue, ccb: &NvmeCcb),
}

/// `struct nvme_softc`.
#[repr(C)]
pub struct NvmeSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,

    /// `sc_ops`.
    pub sc_ops: Cell<Option<&'static NvmeOps>>,
    /// `sc_openings`.
    pub sc_openings: Cell<u32>,

    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_ios`.
    pub sc_ios: Cell<BusSize>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,

    /// `sc_ih`: the interrupt handle, or null.
    pub sc_ih: Cell<*mut c_void>,

    /// `sc_rdy_to`: the ready timeout, in milliseconds.
    pub sc_rdy_to: Cell<u32>,
    /// `sc_mps`: the memory page size.
    pub sc_mps: Cell<usize>,
    /// `sc_mdts`: the largest transfer.
    pub sc_mdts: Cell<usize>,
    /// `sc_max_prpl`: PRP list entries per command.
    pub sc_max_prpl: Cell<u32>,
    /// `sc_dstrd`: the doorbell stride.
    pub sc_dstrd: Cell<u32>,
    /// `sc_sqe_size`: the I/O submission queue entry size.
    pub sc_sqe_size: Cell<u32>,

    /// `sc_identify`: the controller's identify data, written once by `nvme_identify`.
    pub sc_identify: UnsafeCell<NvmIdentifyController>,

    /// `sc_nn`: the number of namespaces.
    pub sc_nn: Cell<u32>,
    /// `sc_namespaces`: `sc_nn + 1` entries, indexed by namespace (target).
    pub sc_namespaces: Cell<*mut NvmeNamespace>,

    /// `sc_admin_q`.
    pub sc_admin_q: Cell<Option<&'static NvmeQueue>>,
    /// `sc_q`.
    pub sc_q: Cell<Option<&'static NvmeQueue>>,
    /// `sc_hib_q`.
    pub sc_hib_q: Cell<Option<&'static NvmeQueue>>,

    /// `sc_ccb_mtx`: protects `sc_ccb_list`.
    pub sc_ccb_mtx: Mutex,
    /// `sc_ccbs`.
    pub sc_ccbs: Cell<*mut NvmeCcb>,
    /// The number of `sc_ccbs` (not in the C; see the module's deviations).
    pub sc_nccbs: Cell<u32>,
    /// `sc_ccb_list`: the free ccbs.
    pub sc_ccb_list: SimpleqHead<NvmeCcbList>,
    /// `sc_ccb_prpls`: the PRP lists of all ccbs.
    pub sc_ccb_prpls: Cell<Option<&'static NvmeDmamem>>,
    /// `sc_iopool`: the ccbs, for the SCSI midlayer.
    pub sc_iopool: ScsiIopool,
    /// `sc_lock`: serialises the passthrough and bio(4) commands.
    pub sc_lock: Rwlock,
    /// `sc_scsibus`.
    pub sc_scsibus: Cell<Option<&'static ScsibusSoftc>>,

    /// `sc_sensordev`.
    pub sc_sensordev: Ksensordev,
    /// `sc_temp_sensor`.
    pub sc_temp_sensor: Ksensor,
    /// `sc_spare_sensor`.
    pub sc_spare_sensor: Ksensor,
    /// `sc_usage_sensor`.
    pub sc_usage_sensor: Ksensor,
}

impl NvmeSoftc {
    /// `sc->sc_iot` and `sc->sc_ioh`, which the bus front-end maps before `nvme_attach`.
    pub fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.sc_iot.get(), self.sc_ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!(
                "{}: registers not mapped",
                self.sc_dev.xname()
            )),
        }
    }

    /// `sc->sc_dmat`, which the bus front-end sets before `nvme_attach`.
    pub fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => panic(format_args!("{}: no DMA tag", self.sc_dev.xname())),
        }
    }

    /// `sc->sc_ops`, which `nvme_attach` sets first.
    pub fn ops(&self) -> &'static NvmeOps {
        match self.sc_ops.get() {
            Some(ops) => ops,
            None => panic(format_args!("{}: no nvme_ops", self.sc_dev.xname())),
        }
    }

    /// `sc->sc_admin_q`.
    pub fn admin_q(&self) -> &'static NvmeQueue {
        match self.sc_admin_q.get() {
            Some(q) => q,
            None => panic(format_args!("{}: no admin queue", self.sc_dev.xname())),
        }
    }

    /// `sc->sc_q`.
    pub fn io_q(&self) -> &'static NvmeQueue {
        match self.sc_q.get() {
            Some(q) => q,
            None => panic(format_args!("{}: no I/O queue", self.sc_dev.xname())),
        }
    }

    /// `&sc->sc_ccbs[id]`; a command identifier past the array panics where the C would
    /// index past it.
    pub fn ccb(&self, id: usize) -> &NvmeCcb {
        let ccbs = self.sc_ccbs.get();
        if ccbs.is_null() || id >= self.sc_nccbs.get() as usize {
            panic(format_args!("{}: bad command id {id}", self.sc_dev.xname()));
        }
        // SAFETY: `nvme_ccbs_alloc` wrote `sc_nccbs` ccbs at `sc_ccbs`, which stay until
        // `nvme_ccbs_free` (which zeroes the count first).
        unsafe { &*ccbs.add(id) }
    }

    /// `sc->sc_identify`.
    pub fn identify(&self) -> &NvmIdentifyController {
        // SAFETY: written only by `nvme_identify` during attach, before anything reads it,
        // and never while a reference from here is alive (attach is single-threaded).
        unsafe { &*self.sc_identify.get() }
    }

    /// `&sc->sc_namespaces[target]`, `target` up to `sc_nn`; `None` past the array (the C
    /// would index past it) or before `nvme_attach` allocates it.
    pub fn namespace(&self, target: usize) -> Option<&NvmeNamespace> {
        let ns = self.sc_namespaces.get();
        if ns.is_null() || target > self.sc_nn.get() as usize {
            return None;
        }
        // SAFETY: `nvme_attach` allocated (zeroed) `sc_nn + 1` entries, never freed.
        Some(unsafe { &*ns.add(target) })
    }

    /// `sc->sc_namespaces[target].ident`.
    pub fn ns_ident(&self, target: usize) -> Option<&NvmIdentifyNamespace> {
        let ident = self.namespace(target)?.ident.get()?;
        // SAFETY: `nvme_scsi_probe` stored a `malloc`ed copy that stays until
        // `nvme_scsi_free` takes it out.
        Some(unsafe { ident.as_ref() })
    }
}

// SAFETY: `#[repr(C)]` with the device first; the other members are mutexes, an rwlock, an
// iopool, a list head and sensors (all-zero valid, `sys/mutex.rs`, `sys/rwlock.rs`,
// `scsi/scsiconf.rs`, `sys/queue.rs`, `sys/sensors.rs`), the identify data (integers), and
// `Cell`s of integers, raw pointers and `Option`s of references and bus tags.
unsafe impl Softc for NvmeSoftc {}

/// `NVME_DMA_MAP(_ndm)`.
#[inline]
pub fn nvme_dma_map(ndm: &NvmeDmamem) -> &'static BusDmamap {
    ndm.ndm_map
}

/// `NVME_DMA_LEN(_ndm)`: the length of the loaded segment.
#[inline]
pub fn nvme_dma_len(ndm: &NvmeDmamem) -> usize {
    ndm.ndm_map.dm_segs().first().map_or(0, |s| s.get().ds_len)
}

/// `NVME_DMA_DVA(_ndm)`: the device address of the loaded segment.
#[inline]
pub fn nvme_dma_dva(ndm: &NvmeDmamem) -> u64 {
    ndm.ndm_map
        .dm_segs()
        .first()
        .map_or(0, |s| s.get().ds_addr as u64)
}

/// `NVME_DMA_KVA(_ndm)`.
#[inline]
pub fn nvme_dma_kva(ndm: &NvmeDmamem) -> *mut u8 {
    ndm.ndm_kva.as_ptr()
}

/// `nvme_read4(_s, _r)`.
#[inline]
pub fn nvme_read4(sc: &NvmeSoftc, r: BusSize) -> u32 {
    let (t, h) = sc.regs();
    bus_space_read_4(t, h, r)
}

/// `nvme_write4(_s, _r, _v)`.
#[inline]
pub fn nvme_write4(sc: &NvmeSoftc, r: BusSize, v: u32) {
    let (t, h) = sc.regs();
    bus_space_write_4(t, h, r, v)
}

/// `nvme_barrier(_s, _r, _l, _f)`.
#[inline]
pub fn nvme_barrier(sc: &NvmeSoftc, r: BusSize, l: BusSize, f: u32) {
    let (t, h) = sc.regs();
    bus_space_barrier(t, h, r, l, f)
}
/* </CODE> */
