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
 * Copyright (c) 2005 David Gwynne <dlg@openbsd.org>
 * Copyright (c) 2005 Marco Peereboom <marco@openbsd.org>
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
//! `<dev/ic/mpivar.h>`: the mpi(4) driver's state: DMA memory, command control blocks, reply
//! control blocks and the softc shared by the bus front-ends (`mpi_pci`).
//!
//! Upstream: sys/dev/ic/mpivar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `MPI_DEBUG` (`mpi_debug`, `DPRINTF`, `DNPRINTF`, `MPI_D_*`) is not defined by the
//!   header's default configuration and is not ported.
//! - `struct mpi_dmamem` is `malloc(9)`ed as in C and written whole once its members are
//!   known (`mpi_dmamem_alloc`); it is reached as `&'static` until `mpi_dmamem_free` takes
//!   it back. `MPI_DMA_MAP`, `MPI_DMA_DVA` and `MPI_DMA_KVA` are inline functions.
//! - `struct mpi_ccb_bundle` is `#[repr(C)]` rather than `__packed`: every member already
//!   sits at its natural alignment (a compile-time check states the offsets).
//! - `ccb_cookie` (`void *`) is [`MpiCookie`], an enum of what the C points it at: the
//!   transfer, `mpi_poll`'s result, or `mpi_wait`'s mutex. `mpi_poll`'s `int rv` lives on
//!   the stack in C and the ccb keeps a pointer to it even after a timeout, so a late
//!   completion writes into a dead frame; here the result is the ccb's `ccb_poll_rv`
//!   (see `mpi.rs`, External bugs).
//! - `ccb_done` is a `Cell` of an optional function pointer (`NULL` after `mpi_put_ccb`);
//!   the ccb array, the reply control blocks and their DMA areas are reached as `&'static`
//!   (they are only freed on a failed attach, when nothing else holds them).
//! - The softc carries `sc_nccbs`, the length of `sc_ccbs`, which the C does not keep:
//!   `mpi_reply` checks a context id against it instead of indexing past the array.
//! - `DEVNAME(sc)` is `sc.sc_dev.xname()`; `dwordsof(s)` is [`dwordsof`].

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};

use crate::dev::bio::BioIoctl;
use crate::dev::ic::mpireg::{MpiCfgHdr, MpiCfgIocPg2, MpiCfgRaidVol, MpiCfgRaidVolPg0};
use crate::dev::ic::mpireg::{MpiMsgScsiIo, MpiSge};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BusDmaSegment, BusDmaTag, BusDmamap, BusSize, BusSpaceHandle, BusSpaceTag,
};
use crate::queue_adapter;
use crate::scsi::scsi_all::ScsiSenseData;
use crate::scsi::scsiconf::{ScsiIohandler, ScsiIopool, ScsiXfer, ScsibusSoftc};
use crate::sys::device::{Device, Softc};
use crate::sys::mutex::Mutex;
use crate::sys::param::PAGE_SIZE;
use crate::sys::queue::{SimpleqEntry, SimpleqHead, SlistEntry, SlistHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::sensors::{Ksensor, Ksensordev};
use crate::sys::task::Task;

/// `MPI_REQUEST_SIZE`: the size of a request frame.
pub const MPI_REQUEST_SIZE: usize = 512;
/// `MPI_REPLY_SIZE`: the size of a reply frame.
pub const MPI_REPLY_SIZE: usize = 80;
/// `MPI_REPLYQ_DEPTH`: the most reply frames the driver posts.
pub const MPI_REPLYQ_DEPTH: usize = 128;
/// `MPI_REPLY_COUNT`.
pub const MPI_REPLY_COUNT: usize = PAGE_SIZE / MPI_REPLY_SIZE;

/// `MPI_MAX_SGL`: the max number of sge's we can stuff in a request frame:
/// sizeof(scsi_io) + sizeof(sense) + sizeof(sge) * 32 = `MPI_REQUEST_SIZE`.
pub const MPI_MAX_SGL: usize = 36;

/// `dwordsof(s)`: the size of `T` in 32-bit words.
pub const fn dwordsof<T>() -> usize {
    size_of::<T>() / size_of::<u32>()
}

/// `struct mpi_dmamem`: one physically contiguous, mapped and loaded DMA area.
pub struct MpiDmamem {
    /// `mdm_map`.
    pub mdm_map: &'static BusDmamap,
    /// `mdm_seg`.
    pub mdm_seg: BusDmaSegment,
    /// `mdm_size`.
    pub mdm_size: usize,
    /// `mdm_kva`.
    pub mdm_kva: NonNull<u8>,
}

/// `MPI_DMA_MAP(_mdm)`.
#[inline]
pub fn mpi_dma_map(mdm: &MpiDmamem) -> &'static BusDmamap {
    mdm.mdm_map
}

/// `MPI_DMA_DVA(_mdm)`: the device address of the first segment.
#[inline]
pub fn mpi_dma_dva(mdm: &MpiDmamem) -> u64 {
    mdm.mdm_map
        .dm_segs()
        .first()
        .map_or(0, |s| s.get().ds_addr as u64)
}

/// `MPI_DMA_KVA(_mdm)`.
#[inline]
pub fn mpi_dma_kva(mdm: &MpiDmamem) -> *mut u8 {
    mdm.mdm_kva.as_ptr()
}

/// `struct mpi_ccb_bundle`: the SCSI I/O request, its scatter/gather list and the sense
/// buffer, laid out in one request frame.
#[repr(C)]
pub struct MpiCcbBundle {
    /// `mcb_io`: the sgl must follow.
    pub mcb_io: MpiMsgScsiIo,
    /// `mcb_sgl`.
    pub mcb_sgl: [MpiSge; MPI_MAX_SGL],
    /// `mcb_sense`.
    pub mcb_sense: ScsiSenseData,
}

const _: () = assert!(size_of::<MpiCcbBundle>() <= MPI_REQUEST_SIZE);
const _: () = assert!(core::mem::offset_of!(MpiCcbBundle, mcb_sgl) == size_of::<MpiMsgScsiIo>());

/// `struct mpi_rcb`: a reply frame the IOC may post to.
#[repr(C)]
pub struct MpiRcb {
    /// `rcb_link`: `sc_evt_ack_queue` and `sc_evt_scan_queue`.
    pub rcb_link: SimpleqEntry<MpiRcb>,
    /// `rcb_reply`: the frame's kernel address.
    pub rcb_reply: Cell<*mut u8>,
    /// `rcb_offset`: the frame's offset in `sc_replies`.
    pub rcb_offset: Cell<BusSize>,
    /// `rcb_reply_dva`: the frame's device address.
    pub rcb_reply_dva: Cell<u32>,
}

queue_adapter!(
    /// `SIMPLEQ_HEAD(mpi_rcb_list, mpi_rcb)`, through `rcb_link`.
    pub MpiRcbList: MpiRcb, rcb_link => SimpleqEntry<MpiRcb>
);

/// `ccb_done`: what completes a command.
pub type MpiDoneFn = fn(ccb: &'static MpiCcb);

/// `ccb_cookie`: what the command's done function works on.
#[derive(Clone, Copy)]
pub enum MpiCookie {
    /// `NULL`.
    None,
    /// A SCSI transfer (`mpi_scsi_cmd`).
    Xs(&'static ScsiXfer),
    /// `mpi_poll`'s `&rv`: the result is `ccb_poll_rv`.
    Poll,
    /// `mpi_wait`'s mutex.
    Wait(*const Mutex),
}

/// `ccb_state`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MpiCcbState {
    /// `MPI_CCB_FREE`.
    Free,
    /// `MPI_CCB_READY`.
    Ready,
    /// `MPI_CCB_QUEUED`.
    Queued,
}

/// `struct mpi_ccb`: a command control block; its index in `sc_ccbs` is the message context.
pub struct MpiCcb {
    /// `ccb_sc`.
    pub ccb_sc: Cell<*const MpiSoftc>,
    /// `ccb_id`.
    pub ccb_id: Cell<i32>,

    /// `ccb_cookie`.
    pub ccb_cookie: Cell<MpiCookie>,
    /// `ccb_poll_rv`: `mpi_poll`'s `rv` (see the module's deviations).
    pub ccb_poll_rv: Cell<i32>,
    /// `ccb_dmamap`: the data of a transfer.
    pub ccb_dmamap: &'static BusDmamap,

    /// `ccb_offset`: the offset of the request frame in `sc_requests`.
    pub ccb_offset: Cell<BusSize>,
    /// `ccb_cmd`: the request frame.
    pub ccb_cmd: Cell<*mut u8>,
    /// `ccb_cmd_dva`: its device address.
    pub ccb_cmd_dva: Cell<u64>,

    /// `ccb_state`.
    pub ccb_state: Cell<MpiCcbState>,
    /// `ccb_done`.
    pub ccb_done: Cell<Option<MpiDoneFn>>,
    /// `ccb_rcb`: the reply frame, if there was one.
    pub ccb_rcb: Cell<Option<&'static MpiRcb>>,

    /// `ccb_link`: `sc_ccb_free`.
    pub ccb_link: SlistEntry<MpiCcb>,
}

queue_adapter!(
    /// `SLIST_HEAD(mpi_ccb_list, mpi_ccb)`, through `ccb_link`.
    pub MpiCcbList: MpiCcb, ccb_link => SlistEntry<MpiCcb>
);

impl MpiCcb {
    /// `ccb->ccb_sc`.
    pub fn sc(&self) -> &'static MpiSoftc {
        let p = self.ccb_sc.get();
        if p.is_null() {
            panic(format_args!("mpi: ccb without a softc"));
        }
        // SAFETY: `mpi_alloc_ccbs` stored the softc, which is never freed while the ccb
        // exists.
        unsafe { &*p }
    }

    /// `ccb->ccb_cmd` as the request frame `T`.
    ///
    /// # Safety
    ///
    /// The caller holds the ccb (it came from the iopool and has not been put back), so
    /// nothing else references the frame, and the IOC is not processing it; `T` is one of
    /// `mpireg.rs`'s messages, valid as the zero bytes `mpi_put_ccb` leaves in the frame.
    #[allow(clippy::mut_from_ref)] // the frame is DMA memory the ccb's holder owns
    pub unsafe fn cmd<T>(&self) -> &mut T {
        if size_of::<T>() > MPI_REQUEST_SIZE {
            panic(format_args!("mpi: request frame type too large"));
        }
        // SAFETY: the caller's guarantee; the frame is `MPI_REQUEST_SIZE` bytes, 4-aligned.
        unsafe { &mut *self.ccb_cmd.get().cast::<T>() }
    }
}

/// `enum { MPI_F_SPI, MPI_F_RAID }` of `sc_flags`: a parallel SCSI controller.
pub const MPI_F_SPI: i32 = 1 << 0;
/// `MPI_F_RAID`: the controller has RAID volumes.
pub const MPI_F_RAID: i32 = 1 << 1;

/// `struct mpi_softc`.
#[repr(C)]
pub struct MpiSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,

    /// `sc_port_wwn`.
    pub sc_port_wwn: Cell<u64>,
    /// `sc_node_wwn`.
    pub sc_node_wwn: Cell<u64>,

    /// `sc_flags`: `MPI_F_*`.
    pub sc_flags: Cell<i32>,

    /// `sc_scsibus`.
    pub sc_scsibus: Cell<Option<&'static ScsibusSoftc>>,

    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_ios`.
    pub sc_ios: Cell<BusSize>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,

    /// `sc_fw_maj`.
    pub sc_fw_maj: Cell<u8>,
    /// `sc_fw_min`.
    pub sc_fw_min: Cell<u8>,
    /// `sc_fw_unit`.
    pub sc_fw_unit: Cell<u8>,
    /// `sc_fw_dev`.
    pub sc_fw_dev: Cell<u8>,

    /// `sc_porttype`.
    pub sc_porttype: Cell<u8>,
    /// `sc_maxcmds`.
    pub sc_maxcmds: Cell<i32>,
    /// `sc_maxchdepth`.
    pub sc_maxchdepth: Cell<i32>,
    /// `sc_first_sgl_len`.
    pub sc_first_sgl_len: Cell<i32>,
    /// `sc_chain_len`.
    pub sc_chain_len: Cell<i32>,
    /// `sc_max_sgl_len`.
    pub sc_max_sgl_len: Cell<i32>,

    /// `sc_buswidth`.
    pub sc_buswidth: Cell<i32>,
    /// `sc_target`.
    pub sc_target: Cell<i32>,
    /// `sc_ioc_number`.
    pub sc_ioc_number: Cell<i32>,

    /// `sc_requests`.
    pub sc_requests: Cell<Option<&'static MpiDmamem>>,
    /// `sc_ccbs`.
    pub sc_ccbs: Cell<*mut MpiCcb>,
    /// The number of `sc_ccbs` (not in the C; see the module's deviations).
    pub sc_nccbs: Cell<i32>,
    /// `sc_ccb_free`.
    pub sc_ccb_free: SlistHead<MpiCcbList>,
    /// `sc_ccb_mtx`.
    pub sc_ccb_mtx: Mutex,
    /// `sc_iopool`.
    pub sc_iopool: ScsiIopool,

    /// `sc_replies`.
    pub sc_replies: Cell<Option<&'static MpiDmamem>>,
    /// `sc_rcbs`.
    pub sc_rcbs: Cell<*mut MpiRcb>,
    /// `sc_repq`.
    pub sc_repq: Cell<i32>,

    /// `sc_evt_ccb`.
    pub sc_evt_ccb: Cell<Option<&'static MpiCcb>>,
    /// `sc_evt_ack_queue`.
    pub sc_evt_ack_queue: SimpleqHead<MpiRcbList>,
    /// `sc_evt_ack_mtx`.
    pub sc_evt_ack_mtx: Mutex,
    /// `sc_evt_ack_handler`.
    pub sc_evt_ack_handler: ScsiIohandler,

    /// `sc_evt_scan_queue`.
    pub sc_evt_scan_queue: SimpleqHead<MpiRcbList>,
    /// `sc_evt_scan_mtx`.
    pub sc_evt_scan_mtx: Mutex,
    /// `sc_evt_scan_handler`.
    pub sc_evt_scan_handler: ScsiIohandler,

    /// `sc_evt_rescan`.
    pub sc_evt_rescan: Task,

    /// `sc_fw_len`.
    pub sc_fw_len: Cell<usize>,
    /// `sc_fw`.
    pub sc_fw: Cell<Option<&'static MpiDmamem>>,

    /// `sc_ioctl`: scsi ioctl from sd device.
    pub sc_ioctl: Cell<Option<BioIoctl>>,

    /// `sc_lock`.
    pub sc_lock: Rwlock,
    /// `sc_cfg_hdr`.
    pub sc_cfg_hdr: Cell<MpiCfgHdr>,
    /// `sc_vol_page`: IOC page 2, `malloc`ed.
    pub sc_vol_page: Cell<*mut MpiCfgIocPg2>,
    /// `sc_vol_list`: the volume list that follows the page.
    pub sc_vol_list: Cell<*mut MpiCfgRaidVol>,
    /// `sc_rpg0`: RAID volume page 0, `malloc`ed.
    pub sc_rpg0: Cell<*mut MpiCfgRaidVolPg0>,

    /// `sc_sensors`.
    pub sc_sensors: Cell<*mut Ksensor>,
    /// `sc_sensordev`.
    pub sc_sensordev: Ksensordev,
}

// SAFETY: `#[repr(C)]` with the device first; the other members are mutexes, an rwlock, an
// iopool, list heads, a task and sensors (all-zero valid, `sys/mutex.rs`, `sys/rwlock.rs`,
// `scsi/scsiconf.rs`, `sys/queue.rs`, `sys/task.rs`, `sys/sensors.rs`), and `Cell`s of
// integers, raw pointers, `Option`s of references, bus tags and function pointers, and of
// the configuration page header (bytes).
unsafe impl Softc for MpiSoftc {}

impl MpiSoftc {
    /// `sc->sc_iot` and `sc->sc_ioh`, which the bus front-end maps before `mpi_attach`.
    pub fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.sc_iot.get(), self.sc_ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!(
                "{}: registers not mapped",
                self.sc_dev.xname()
            )),
        }
    }

    /// `sc->sc_dmat`, which the bus front-end sets before `mpi_attach`.
    pub fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => panic(format_args!("{}: no DMA tag", self.sc_dev.xname())),
        }
    }

    /// `sc->sc_requests`, which `mpi_alloc_ccbs` sets.
    pub fn requests(&self) -> &'static MpiDmamem {
        match self.sc_requests.get() {
            Some(m) => m,
            None => panic(format_args!("{}: no request frames", self.sc_dev.xname())),
        }
    }

    /// `sc->sc_replies`, which `mpi_alloc_replies` sets.
    pub fn replies(&self) -> &'static MpiDmamem {
        match self.sc_replies.get() {
            Some(m) => m,
            None => panic(format_args!("{}: no reply frames", self.sc_dev.xname())),
        }
    }

    /// `&sc->sc_ccbs[id]`; an id past the array panics where the C would index past it.
    pub fn ccb(&self, id: usize) -> &'static MpiCcb {
        let ccbs = self.sc_ccbs.get();
        if ccbs.is_null() || id >= self.sc_nccbs.get() as usize {
            panic(format_args!("{}: bad context id {id}", self.sc_dev.xname()));
        }
        // SAFETY: `mpi_alloc_ccbs` wrote `sc_nccbs` ccbs at `sc_ccbs`, which stay until a
        // failed attach frees them.
        unsafe { &*ccbs.add(id) }
    }

    /// `&sc->sc_rcbs[i]`; an index past the array panics where the C would index past it.
    pub fn rcb(&self, i: usize) -> &'static MpiRcb {
        let rcbs = self.sc_rcbs.get();
        if rcbs.is_null() || i >= self.sc_repq.get().max(0) as usize {
            panic(format_args!("{}: bad reply index {i}", self.sc_dev.xname()));
        }
        // SAFETY: `mpi_alloc_replies` allocated `sc_repq` rcbs, written whole by
        // `mpi_push_replies`.
        unsafe { &*rcbs.add(i) }
    }

    /// The softc as the `void *` cookie of the iopool, the handlers and the interrupt.
    pub fn cookie(&self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::ic::mpireg::{MpiMsgIocfactsReply, MpiMsgIocfactsRequest};

    #[test]
    fn sizes_and_counts() {
        assert_eq!(dwordsof::<MpiMsgIocfactsRequest>(), 3);
        assert_eq!(dwordsof::<MpiMsgIocfactsReply>(), 20);
        assert_eq!(MPI_REPLY_COUNT, PAGE_SIZE / 80);
        assert!(size_of::<MpiCcbBundle>() <= MPI_REQUEST_SIZE);
    }
}
/* </TESTS> */
