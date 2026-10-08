/*	$OpenBSD: vioblk.c,v 1.47 2025/09/16 12:18:10 hshoexer Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2012 Stefan Fritsch.
 * Copyright (c) 2010 Minoura Makoto.
 * Copyright (c) 1998, 2001 Manuel Bouyer.
 * All rights reserved.
 *
 * This code is based in part on the NetBSD ld_virtio driver and the
 * OpenBSD vdsk driver.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *	notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *	notice, this list of conditions and the following disclaimer in the
 *	documentation and/or other materials provided with the distribution.
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
 * Copyright (c) 2009, 2011 Mark Kettenis
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
//! `vioblk(4)`: the virtio block device driver (`vioblk* at virtio?`), presented to the
//! system as a SCSI adapter with one disk.
//!
//! Upstream: sys/dev/pv/vioblk.c @ 3ce1f3f79392
//!
//! One virtqueue carries the requests. Each request slot (`struct virtio_blk_req`, in DMA
//! memory) owns a contiguous block of `ALLOC_SEGS` descriptors (or one indirect slot), a map
//! for its header and status byte and a map for the payload, all made at attach time; the
//! free slots are the adapter's `scsi_iopool`, so the SCSI midlayer only issues a command
//! once its slot exists. `vioblk_scsi_cmd` turns READ/WRITE (6, 10, 12, 16) into
//! `VIRTIO_BLK_T_IN`/`OUT` requests and SYNCHRONIZE CACHE into `VIRTIO_BLK_T_FLUSH`, and
//! answers INQUIRY, READ CAPACITY (10, 16), TEST UNIT READY, START STOP and PREVENT ALLOW
//! itself; `vioblk_vq_done` completes the requests the device has used. With `SCSI_POLL`
//! the command polls the interrupt handler for up to 15 seconds and resets the device when
//! it times out.
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the device first; it is reached as `&'static` (softcs are
//!   never freed while the device exists). Its members are `Cell`s, all-zero valid.
//! - `struct virtio_blk_req` keeps the C's layout (`#[repr(C)]`, the packed header first,
//!   `VR_DMA_END` = `offset_of!(VirtioBlkReq, vr_qe_index)`), because its first bytes are
//!   what the device reads and writes; the status byte the device writes is read volatile.
//!   The slots live in the `bus_dmamem` area `sc_reqs` points to, reached through the
//!   bounds-checked `VioblkSoftc::req`.
//! - The opcode dispatch of `vioblk_scsi_cmd` is the function `vioblk_scsi_op`, returning
//!   what to do ([`VioblkCmd`]), and the CDB decoding by command length is `vioblk_rw_decode`
//!   (still by `cmdlen`, as the C does, not by opcode as `scsi_cmd_rw_decode`); the
//!   INQUIRY and READ CAPACITY replies are built by `vioblk_inquiry_data`,
//!   `vioblk_read_cap_data` and `vioblk_read_cap_data_16`. The C does all of it inline;
//!   split out, it is host-tested. The `bcopy(&rcd, xs->data, MIN(sizeof(rcd),
//!   xs->datalen))` of both capacity replies is `vioblk_copy_reply`.
//! - `vr_xs`, `vr_cmdsts` and `vr_payload` are `Option`s: a slot without a transfer or a map
//!   panics where the C would dereference NULL.
//! - `sc_nreqs` is a `usize`, `sc_notify_on_empty` a `bool`.
//! - `vioblk_feature_names` is empty unless `VIRTIO_DEBUG` (the C's `#if`), as in `vio`.
//! - `DNPRINTF` is `if VIRTIO_DEBUG >= n { printf(..) }`.
//! - The contiguity check of `vioblk_alloc_reqs` reads `vq_desc[slot + r].next` volatile
//!   (`vioblk_desc_next`) and takes an index past the descriptor table as "not contiguous"
//!   where the C would read past it.
//! - No stubs: every function of the file is ported. (This revision has no
//!   `vioblk_minphys`, `dev_probe` or `dev_free`: `vioblk_switch` is `{ vioblk_scsi_cmd,
//!   NULL, NULL, NULL, NULL }`.)

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};

use crate::dev::pv::vioblkreg::{
    VIRTIO_BLK_CONFIG_CAPACITY, VIRTIO_BLK_CONFIG_SEG_MAX, VIRTIO_BLK_CONFIG_SIZE_MAX,
    VIRTIO_BLK_F_BARRIER, VIRTIO_BLK_F_BLK_SIZE, VIRTIO_BLK_F_CONFIG_WCE, VIRTIO_BLK_F_DISCARD,
    VIRTIO_BLK_F_FLUSH, VIRTIO_BLK_F_GEOMETRY, VIRTIO_BLK_F_LIFETIME, VIRTIO_BLK_F_MQ,
    VIRTIO_BLK_F_RO, VIRTIO_BLK_F_SCSI, VIRTIO_BLK_F_SECURE_ERASE, VIRTIO_BLK_F_SEG_MAX,
    VIRTIO_BLK_F_SIZE_MAX, VIRTIO_BLK_F_TOPOLOGY, VIRTIO_BLK_F_WRITE_ZEROES, VIRTIO_BLK_S_OK,
    VIRTIO_BLK_SECTOR_SIZE, VIRTIO_BLK_T_FLUSH, VIRTIO_BLK_T_IN, VIRTIO_BLK_T_OUT, VirtioBlkReqHdr,
};
use crate::dev::pv::virtio::{
    virtio_alloc_vq, virtio_attach_finish, virtio_check_vq, virtio_dequeue, virtio_enqueue,
    virtio_enqueue_commit, virtio_enqueue_p, virtio_enqueue_prep, virtio_enqueue_reserve,
    virtio_enqueue_trim, virtio_reinit_end, virtio_reinit_start, virtio_reset,
    virtio_start_vq_intr, virtio_stop_vq_intr,
};
use crate::dev::pv::virtioreg::{PCI_PRODUCT_VIRTIO_BLOCK, VIRTIO_F_NOTIFY_ON_EMPTY};
use crate::dev::pv::virtiovar::{
    VIRTIO_CHILD_ERROR, VIRTIO_DEBUG, VirtioAttachArgs, VirtioFeatureName, VirtioSoftc, Virtqueue,
    virtio_has_feature, virtio_negotiate_features, virtio_poll_intr, virtio_read_device_config_4,
    virtio_read_device_config_8,
};
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_64BIT, BUS_DMA_ALLOCNOW, BUS_DMA_NOWAIT, BUS_DMA_READ, BUS_DMA_WRITE,
    BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE,
    BusDmaSegment, BusDmamap, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load,
    bus_dmamap_sync, bus_dmamap_unload, bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_map,
    bus_dmamem_unmap,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{IPL_BIO, splbio, splx};
use crate::scsi::scsi_all::{
    INQUIRY, MODE_SENSE, MODE_SENSE_BIG, PREVENT_ALLOW, REPORT_LUNS, SI_EVPD, SID_CmdQue,
    SID_SCSI2_ALEN, SID_SCSI2_RESPONSE, START_STOP, ScsiGeneric, ScsiInquiry, ScsiInquiryData,
    ScsiReadCapData, ScsiReadCapData16, ScsiWire, T_DIRECT, TEST_UNIT_READY, wire_ref,
};
use crate::scsi::scsi_base::{scsi_copy_internal_data, scsi_done, scsi_iopool_init};
use crate::scsi::scsi_disk::{
    READ_10, READ_12, READ_16, READ_CAPACITY, READ_CAPACITY_16, READ_COMMAND, SRW_TOPADDR,
    SYNCHRONIZE_CACHE, ScsiRw, ScsiRw10, ScsiRw12, ScsiRw16, WRITE_10, WRITE_12, WRITE_16,
    WRITE_COMMAND,
};
use crate::scsi::scsiconf::{
    _2btol, _3btol, _4btol, _8btol, _lto4b, _lto8b, SCSI_POLL, SCSI_REV_SPC3,
    SDEV_NO_ADAPTER_TARGET, SDEV_READONLY, ScsiAdapter, ScsiIo, ScsiIopool, ScsiXfer,
    ScsibusAttachArgs, XS_DRIVER_STUFFUP, XS_NOERROR, scsiprint,
};
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::mutex::Mutex;
use crate::sys::param::{MAXPHYS, PAGE_SIZE};
use crate::sys::queue::{SlistEntry, SlistHead};
use crate::{kassert, queue_adapter};

/// `VIOBLK_DONE`: the `vr_len` of a request slot that is not in flight.
pub const VIOBLK_DONE: i32 = -1;

/// `SEG_MAX`: number of DMA segments for buffers that the device must support.
pub const SEG_MAX: usize = MAXPHYS / PAGE_SIZE + 1;
/// `ALLOC_SEGS`: in the virtqueue, we need space for header and footer, too.
pub const ALLOC_SEGS: usize = SEG_MAX + 2;

/// `vioblk_feature_names[]` with `VIRTIO_DEBUG`.
static VIOBLK_FEATURE_NAMES_DEBUG: [VirtioFeatureName; 15] = [
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_BARRIER,
        name: "Barrier",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_SIZE_MAX,
        name: "SizeMax",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_SEG_MAX,
        name: "SegMax",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_GEOMETRY,
        name: "Geometry",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_RO,
        name: "RO",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_BLK_SIZE,
        name: "BlkSize",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_SCSI,
        name: "SCSI",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_FLUSH,
        name: "Flush",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_TOPOLOGY,
        name: "Topology",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_CONFIG_WCE,
        name: "ConfigWCE",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_MQ,
        name: "MQ",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_DISCARD,
        name: "Discard",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_WRITE_ZEROES,
        name: "Write0s",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_LIFETIME,
        name: "Lifetime",
    },
    VirtioFeatureName {
        bit: VIRTIO_BLK_F_SECURE_ERASE,
        name: "SecErase",
    },
];

/// `vioblk_feature_names`: the table `virtio_negotiate_features` is given, empty unless
/// `VIRTIO_DEBUG`.
fn vioblk_feature_names() -> &'static [VirtioFeatureName] {
    if VIRTIO_DEBUG > 0 {
        &VIOBLK_FEATURE_NAMES_DEBUG
    } else {
        &[]
    }
}

/// `struct virtio_blk_req`: one request slot. The header and the status byte (up to
/// [`VR_DMA_END`]) are what the device reads and writes, through `vr_cmdsts`.
///
/// The slots live in zeroed DMA memory: every member is valid as all-zero bits.
#[repr(C)]
pub struct VirtioBlkReq {
    /// `vr_hdr`: the request header the device reads.
    pub vr_hdr: Cell<VirtioBlkReqHdr>,
    /// `vr_status`: the status byte the device writes (`VIRTIO_BLK_S_*`); read volatile.
    pub vr_status: Cell<u8>,
    /// `vr_qe_index`: the head of the slot's descriptors.
    pub vr_qe_index: Cell<i16>,
    /// `vr_len`: the payload's length, [`VIOBLK_DONE`] when not in flight.
    pub vr_len: Cell<i32>,
    /// `vr_xs`: the transfer in flight.
    pub vr_xs: Cell<Option<&'static ScsiXfer>>,
    /// `vr_cmdsts`: the map of the header and the status byte.
    pub vr_cmdsts: Cell<Option<&'static BusDmamap>>,
    /// `vr_payload`: the map of the transfer's data.
    pub vr_payload: Cell<Option<&'static BusDmamap>>,
    /// `vr_list`: `sc_freelist`.
    pub vr_list: SlistEntry<VirtioBlkReq>,
}

impl VirtioBlkReq {
    /// `vr->vr_cmdsts`, which every allocated slot has.
    fn cmdsts(&self) -> &'static BusDmamap {
        match self.vr_cmdsts.get() {
            Some(map) => map,
            None => panic(format_args!("vioblk: request without a cmdsts map")),
        }
    }

    /// `vr->vr_payload`, which every allocated slot has.
    fn payload(&self) -> &'static BusDmamap {
        match self.vr_payload.get() {
            Some(map) => map,
            None => panic(format_args!("vioblk: request without a payload map")),
        }
    }

    /// `vr->vr_xs`, set while the slot is in flight.
    fn xs(&self) -> &'static ScsiXfer {
        match self.vr_xs.get() {
            Some(xs) => xs,
            None => panic(format_args!("vioblk: request without a transfer")),
        }
    }

    /// `vr->vr_status`, as the device left it.
    fn status(&self) -> u8 {
        // SAFETY: the cell's own byte, inside the live slot; the device writes it by DMA
        // behind the compiler's back, hence volatile.
        unsafe { ptr::read_volatile(self.vr_status.as_ptr()) }
    }
}

/// `VR_DMA_END`: `offsetof(struct virtio_blk_req, vr_qe_index)`, the bytes the device sees.
pub const VR_DMA_END: usize = offset_of!(VirtioBlkReq, vr_qe_index);

queue_adapter!(
    /// `SLIST_HEAD(, virtio_blk_req)`: `sc_freelist`, through `vr_list`.
    pub VioblkFreelist: VirtioBlkReq, vr_list => SlistEntry<VirtioBlkReq>
);

/// `struct vioblk_softc`.
#[repr(C)]
pub struct VioblkSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_virtio`: the parent.
    pub sc_virtio: Cell<Option<&'static VirtioSoftc>>,

    /// `sc_vq`.
    pub sc_vq: [Virtqueue; 1],
    /// `sc_reqs`: `sc_nreqs` request slots in DMA memory.
    pub sc_reqs: Cell<*mut VirtioBlkReq>,
    /// `sc_reqs_segs`.
    pub sc_reqs_segs: [Cell<BusDmaSegment>; 1],
    /// `sc_nreqs`.
    pub sc_nreqs: Cell<usize>,

    /// `sc_iopool`: the free slots, for the SCSI midlayer.
    pub sc_iopool: ScsiIopool,
    /// `sc_vr_mtx`: protects `sc_freelist`.
    pub sc_vr_mtx: Mutex,
    /// `sc_freelist`.
    pub sc_freelist: SlistHead<VioblkFreelist>,

    /// `sc_notify_on_empty`.
    pub sc_notify_on_empty: Cell<bool>,

    /// `sc_queued`.
    pub sc_queued: Cell<u32>,

    /// `sc_capacity`: in 512-byte sectors.
    pub sc_capacity: Cell<u64>,
}

impl VioblkSoftc {
    /// `sc->sc_virtio`, which `vioblk_attach` sets first.
    pub fn virtio(&self) -> &'static VirtioSoftc {
        match self.sc_virtio.get() {
            Some(vsc) => vsc,
            None => panic(format_args!("vioblk: no virtio softc")),
        }
    }

    /// `&sc->sc_vq[0]`.
    pub fn vq(&self) -> &Virtqueue {
        &self.sc_vq[0]
    }

    /// `&sc->sc_reqs[i]`, `i` below `sc_nreqs`.
    pub fn req(&self, i: usize) -> &'static VirtioBlkReq {
        if i >= self.sc_nreqs.get() {
            panic(format_args!(
                "{}: bad request slot {i}",
                self.sc_dev.xname()
            ));
        }
        // SAFETY: `vioblk_alloc_reqs` mapped (zeroed) room for `sc_nreqs` slots at
        // `sc_reqs`, which stays mapped while the device exists.
        unsafe { vioblk_req_at(self.sc_reqs.get(), i) }
    }
}

// SAFETY: `#[repr(C)]` with the device first; the virtqueue, the iopool, the mutex and the
// list head are all-zero valid (`dev/pv/virtiovar.rs`, `scsi/scsiconf.rs`, `sys/mutex.rs`,
// `sys/queue.rs`), and every other member is a `Cell` of an integer, a `bool`, a raw
// pointer, a DMA segment or an `Option` of a reference.
unsafe impl Softc for VioblkSoftc {}

/// What `vioblk_scsi_cmd` does with a CDB, by opcode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VioblkCmd {
    /// A request on the queue: its `VIRTIO_BLK_T_*` type and whether it reads.
    Request {
        /// `operation`.
        operation: u32,
        /// `isread`.
        isread: bool,
    },
    /// INQUIRY: `vioblk_scsi_inq`.
    Inquiry,
    /// READ CAPACITY (10): `vioblk_scsi_capacity`.
    Capacity,
    /// READ CAPACITY (16): `vioblk_scsi_capacity16`.
    Capacity16,
    /// Finished at once with this `XS_*` error.
    Done(i32),
    /// An opcode the driver does not know: reported, then `XS_DRIVER_STUFFUP`.
    Unknown,
}

/// `vioblk_ca`.
pub static VIOBLK_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<VioblkSoftc>(),
    ca_match: Some(vioblk_match),
    ca_attach: vioblk_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `vioblk_cd`.
pub static VIOBLK_CD: Cfdriver = Cfdriver::new(b"vioblk", DV_DULL, CD_COCOVM);

/// `vioblk_switch`: the adapter's entry points.
pub static VIOBLK_SWITCH: ScsiAdapter = ScsiAdapter {
    scsi_cmd: vioblk_scsi_cmd,
    dev_minphys: None,
    dev_probe: None,
    dev_free: None,
    ioctl: None,
};

/// `&base[i]` of the request slots.
///
/// # Safety
///
/// `base` maps at least `i + 1` zeroed or initialised slots that stay mapped while the
/// device exists.
unsafe fn vioblk_req_at(base: *mut VirtioBlkReq, i: usize) -> &'static VirtioBlkReq {
    if base.is_null() {
        panic(format_args!("vioblk: no request slots"));
    }
    // SAFETY: the caller's guarantee; all-zero bits are a valid slot.
    unsafe { &*base.add(i) }
}

/// `(struct vioblk_softc *)vsc->sc_child`.
fn vioblk_child(vsc: &VirtioSoftc) -> &'static VioblkSoftc {
    let c = vsc.sc_child.get();
    if c.is_null() || c == VIRTIO_CHILD_ERROR {
        panic(format_args!("vioblk: virtio without its child"));
    }
    // SAFETY: vioblk_attach stored its own device, the head of a `VioblkSoftc` that lives as
    // long as the kernel; only vioblk's queue handler calls this.
    unsafe { &*c.cast::<VioblkSoftc>() }
}

/// `xs->sc_link->bus->sb_adapter_softc`: the softc of the vioblk a transfer is for.
fn vioblk_xs_softc(xs: &ScsiXfer) -> &'static VioblkSoftc {
    let p = xs.link().bus().sb_adapter_softc.get();
    if p.is_null() {
        panic(format_args!("vioblk: bus without an adapter softc"));
    }
    // SAFETY: vioblk_attach attaches its scsibus with its own softc as `saa_adapter_softc`,
    // and only that bus's transfers reach `vioblk_switch`; softcs are never freed.
    unsafe { &*p.cast::<VioblkSoftc>().cast_const() }
}

/// `vioblk_match`: the virtio block device.
pub fn vioblk_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: virtio transports attach their children with arguments that begin with a
    // `virtio_attach_args`.
    let va = unsafe { &*aux.cast::<VirtioAttachArgs>() };

    i32::from(va.va_devid == PCI_PRODUCT_VIRTIO_BLOCK)
}

/// `vioblk_attach`.
pub fn vioblk_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `vioblk_ca`, whose softc is a `VioblkSoftc`; softcs are
    // never freed while the device exists, so the softc may be borrowed for 'static.
    let sc: &'static VioblkSoftc = unsafe { &*ptr::from_ref(self_.softc::<VioblkSoftc>()) };
    let Some(parent) = parent else {
        return;
    };
    // SAFETY: `vioblk* at virtio?`: the parent is a virtio transport, whose softc begins
    // with the `struct virtio_softc`; it outlives its child.
    let vsc: &'static VirtioSoftc = unsafe { &*ptr::from_ref(parent.softc::<VirtioSoftc>()) };
    let va = aux.cast::<VirtioAttachArgs>();

    vsc.sc_vqs.set(sc.sc_vq.as_ptr().cast_mut());
    vsc.sc_nvqs.set(1);
    if !vsc.sc_child.get().is_null() {
        panic(format_args!("already attached to something else"));
    }
    vsc.sc_child.set(ptr::from_ref(self_));
    vsc.sc_ipl.set(IPL_BIO);
    sc.sc_virtio.set(Some(vsc));
    vsc.sc_driver_features.set(
        VIRTIO_BLK_F_RO
            | VIRTIO_F_NOTIFY_ON_EMPTY
            | VIRTIO_BLK_F_SIZE_MAX
            | VIRTIO_BLK_F_SEG_MAX
            | VIRTIO_BLK_F_FLUSH,
    );

    let attached: Result<(), ()> = 'err: {
        if virtio_negotiate_features(vsc, Some(vioblk_feature_names())).is_err() {
            break 'err Err(());
        }

        if virtio_has_feature(vsc, VIRTIO_BLK_F_SIZE_MAX) {
            let size_max = virtio_read_device_config_4(vsc, VIRTIO_BLK_CONFIG_SIZE_MAX);
            if (size_max as usize) < PAGE_SIZE {
                printf(format_args!("\nMax segment size {size_max} too low\n"));
                break 'err Err(());
            }
        }

        if virtio_has_feature(vsc, VIRTIO_BLK_F_SEG_MAX) {
            let seg_max = virtio_read_device_config_4(vsc, VIRTIO_BLK_CONFIG_SEG_MAX);
            if (seg_max as usize) < SEG_MAX {
                printf(format_args!(
                    "\nMax number of segments {} too small\n",
                    seg_max as i32
                ));
                break 'err Err(());
            }
        }

        sc.sc_capacity
            .set(virtio_read_device_config_8(vsc, VIRTIO_BLK_CONFIG_CAPACITY));

        if virtio_alloc_vq(vsc, sc.vq(), 0, ALLOC_SEGS as i32, "I/O request").is_err() {
            printf(format_args!("\nCan't alloc virtqueue\n"));
            break 'err Err(());
        }
        let qsize = sc.vq().vq_num.get() as usize;
        sc.vq().vq_done.set(Some(vioblk_vq_done));

        if virtio_has_feature(vsc, VIRTIO_F_NOTIFY_ON_EMPTY) {
            virtio_stop_vq_intr(vsc, sc.vq());
            sc.sc_notify_on_empty.set(true);
        } else {
            sc.sc_notify_on_empty.set(false);
        }

        sc.sc_queued.set(0);

        sc.sc_freelist.init();
        mtx_init(&sc.sc_vr_mtx, IPL_BIO);
        // SAFETY: vioblk_req_get and vioblk_req_put take this softc as their cookie, and the
        // softc is never freed.
        unsafe {
            scsi_iopool_init(
                &sc.sc_iopool,
                ptr::from_ref(sc).cast_mut().cast(),
                vioblk_req_get,
                vioblk_req_put,
            )
        };

        sc.sc_nreqs.set(vioblk_alloc_reqs(sc, qsize));
        if sc.sc_nreqs.get() == 0 {
            printf(format_args!("\nCan't alloc reqs\n"));
            break 'err Err(());
        }
        if VIRTIO_DEBUG >= 1 {
            printf(format_args!("vioblk_attach: qsize: {qsize}\n"));
        }
        printf(format_args!("\n"));

        let mut saa = ScsibusAttachArgs::new();
        saa.saa_adapter = Some(&VIOBLK_SWITCH);
        saa.saa_adapter_softc = ptr::from_ref(self_).cast_mut().cast();
        saa.saa_adapter_buswidth = 1;
        saa.saa_luns = 1;
        saa.saa_adapter_target = SDEV_NO_ADAPTER_TARGET;
        // At most the queue size (a power of two of at most 32768), as in C.
        saa.saa_openings = sc.sc_nreqs.get() as u16;
        saa.saa_pool = Some(&sc.sc_iopool);
        if virtio_has_feature(vsc, VIRTIO_BLK_F_RO) {
            saa.saa_flags = SDEV_READONLY;
        } else {
            saa.saa_flags = 0;
        }
        saa.saa_quirks = 0;
        saa.saa_wwpn = 0;
        saa.saa_wwnn = 0;

        if virtio_attach_finish(vsc, va).is_err() {
            break 'err Err(());
        }
        config_found(self_, ptr::from_mut(&mut saa).cast(), Some(scsiprint));
        Ok(())
    };

    if attached.is_err() {
        // err:
        vsc.sc_child.set(VIRTIO_CHILD_ERROR);
    }
}

/// `vioblk_req_get`: provides the SCSI layer with all the resources necessary to start an
/// I/O on the device.
///
/// Since the size of the I/O is unknown at this time the resources allocated (a.k.a.
/// reserved) must be sufficient to allow the maximum possible I/O size.
///
/// When the I/O is actually attempted via `vioblk_scsi_cmd()` excess resources will be
/// returned via `virtio_enqueue_trim()`.
///
/// # Safety
///
/// `cookie` is a live [`VioblkSoftc`] (the one `vioblk_attach` gave `scsi_iopool_init`).
pub unsafe fn vioblk_req_get(cookie: *mut c_void) -> Option<ScsiIo> {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*cookie.cast::<VioblkSoftc>().cast_const() };

    mtx_enter(&sc.sc_vr_mtx);
    let vr = sc.sc_freelist.first().map(NonNull::from);
    if vr.is_some() {
        // SAFETY: the list is not empty.
        unsafe { sc.sc_freelist.remove_head() };
    }
    mtx_leave(&sc.sc_vr_mtx);

    if VIRTIO_DEBUG >= 2 {
        printf(format_args!(
            "vioblk_req_get: {:p}\n",
            vr.map_or(ptr::null_mut(), NonNull::as_ptr)
        ));
    }

    vr.map(NonNull::cast)
}

/// `vioblk_req_put`: gives a request slot back to the free list.
///
/// # Safety
///
/// `cookie` is a live [`VioblkSoftc`] and `io` one of its request slots, from
/// [`vioblk_req_get`] and not on the free list.
pub unsafe fn vioblk_req_put(cookie: *mut c_void, io: ScsiIo) {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*cookie.cast::<VioblkSoftc>().cast_const() };
    // SAFETY: the caller's guarantee: a slot in `sc_reqs`, mapped while the device exists.
    let vr = unsafe { io.cast::<VirtioBlkReq>().as_ref() };

    if VIRTIO_DEBUG >= 2 {
        printf(format_args!("vioblk_req_put: {:p}\n", vr));
    }

    mtx_enter(&sc.sc_vr_mtx);
    // Do *NOT* call virtio_dequeue_commit()!
    //
    // Descriptors are permanently associated with the vioscsi_req and should not be placed
    // on the free list!
    // SAFETY: the slot is on no list (the caller's guarantee) and stays in place, in the
    // DMA memory, while the device exists.
    unsafe { sc.sc_freelist.insert_head(vr) };
    mtx_leave(&sc.sc_vr_mtx);
}

/// `vioblk_vq_done`: the queue's `vq_done`: completes every request the device has used.
pub fn vioblk_vq_done(vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let sc = vioblk_child(vsc);
    let mut ret = 0;

    if !sc.sc_notify_on_empty.get() {
        virtio_stop_vq_intr(vsc, vq);
    }
    loop {
        let slot = match virtio_dequeue(vsc, vq) {
            Ok((slot, _len)) => slot,
            Err(_) => {
                if sc.sc_notify_on_empty.get() {
                    break;
                }
                virtio_start_vq_intr(vsc, vq);
                match virtio_dequeue(vsc, vq) {
                    Ok((slot, _len)) => slot,
                    Err(_) => break,
                }
            }
        };
        let qe = vq.entry(slot as usize);
        vioblk_vq_done1(sc, vsc, vq, qe.qe_vr_index.get() as usize);
        ret = 1;
    }
    ret
}

/// `vioblk_vq_done1`: completes the request in slot `slot` of `sc_reqs`.
pub fn vioblk_vq_done1(sc: &VioblkSoftc, vsc: &VirtioSoftc, _vq: &Virtqueue, slot: usize) {
    let vr = sc.req(slot);
    let xs = vr.xs();
    kassert!(vr.vr_len.get() != VIOBLK_DONE);
    bus_dmamap_sync(
        vsc.dmat(),
        vr.cmdsts(),
        0,
        size_of::<VirtioBlkReqHdr>(),
        BUS_DMASYNC_POSTWRITE,
    );
    let r#type = vr.vr_hdr.get().r#type;
    if r#type != VIRTIO_BLK_T_FLUSH {
        bus_dmamap_sync(
            vsc.dmat(),
            vr.payload(),
            0,
            vr.vr_len.get() as usize,
            if r#type == VIRTIO_BLK_T_IN {
                BUS_DMASYNC_POSTREAD
            } else {
                BUS_DMASYNC_POSTWRITE
            },
        );
        bus_dmamap_unload(vsc.dmat(), vr.payload());
    }
    bus_dmamap_sync(
        vsc.dmat(),
        vr.cmdsts(),
        size_of::<VirtioBlkReqHdr>(),
        size_of::<u8>(),
        BUS_DMASYNC_POSTREAD,
    );

    let datalen = xs.datalen();
    if vr.status() != VIRTIO_BLK_S_OK {
        if VIRTIO_DEBUG >= 1 {
            printf(format_args!("vioblk_vq_done1: EIO\n"));
        }
        xs.error.set(XS_DRIVER_STUFFUP);
        xs.resid.set(datalen.max(0) as usize);
    } else {
        xs.error.set(XS_NOERROR);
        xs.resid.set((datalen - vr.vr_len.get()).max(0) as usize);
    }
    vr.vr_len.set(VIOBLK_DONE);
    scsi_done(xs);
}

/// `vioblk_reset`: resets the device and fails every request still in flight.
pub fn vioblk_reset(sc: &VioblkSoftc) {
    // reset device to stop DMA
    virtio_reset(sc.virtio());

    // finish requests that have been completed
    virtio_check_vq(sc.virtio(), sc.vq());

    // abort all remaining requests
    for i in 0..sc.sc_nreqs.get() {
        let vr = sc.req(i);

        if vr.vr_len.get() == VIOBLK_DONE {
            continue;
        }
        let xs = vr.xs();

        xs.error.set(XS_DRIVER_STUFFUP);
        xs.resid.set(xs.datalen().max(0) as usize);
        scsi_done(xs);
    }
}

/// The opcode dispatch of `vioblk_scsi_cmd`: what to do with a command, given whether the
/// device negotiated `VIRTIO_BLK_F_FLUSH`.
pub fn vioblk_scsi_op(opcode: u8, has_flush: bool) -> VioblkCmd {
    match opcode {
        READ_COMMAND | READ_10 | READ_12 | READ_16 => VioblkCmd::Request {
            operation: VIRTIO_BLK_T_IN,
            isread: true,
        },
        WRITE_COMMAND | WRITE_10 | WRITE_12 | WRITE_16 => VioblkCmd::Request {
            operation: VIRTIO_BLK_T_OUT,
            isread: false,
        },

        SYNCHRONIZE_CACHE => {
            if !has_flush {
                VioblkCmd::Done(XS_NOERROR)
            } else {
                VioblkCmd::Request {
                    operation: VIRTIO_BLK_T_FLUSH,
                    isread: false,
                }
            }
        }

        INQUIRY => VioblkCmd::Inquiry,
        READ_CAPACITY => VioblkCmd::Capacity,
        READ_CAPACITY_16 => VioblkCmd::Capacity16,

        TEST_UNIT_READY | START_STOP | PREVENT_ALLOW => VioblkCmd::Done(XS_NOERROR),

        MODE_SENSE | MODE_SENSE_BIG | REPORT_LUNS => VioblkCmd::Done(XS_DRIVER_STUFFUP),
        _ => VioblkCmd::Unknown,
    }
}

/// The CDB decoding of `vioblk_scsi_cmd`: `(lba, sector_count)` of a READ/WRITE/SYNCHRONIZE
/// command by its length (`(0, 0)` for another length). SYNCHRONIZE CACHE has the same
/// layout as the 10-byte READ/WRITE commands.
pub fn vioblk_rw_decode(cmd: &ScsiGeneric, cmdlen: i32) -> (u64, u32) {
    let bytes = cmd.as_bytes();
    match cmdlen {
        6 => {
            let rw = wire_ref::<ScsiRw>(bytes);
            let lba = _3btol(&rw.addr) & ((SRW_TOPADDR as u32) << 16 | 0xffff);
            let sector_count = if rw.length != 0 {
                u32::from(rw.length)
            } else {
                0x100
            };
            (u64::from(lba), sector_count)
        }
        10 => {
            let rw10 = wire_ref::<ScsiRw10>(bytes);
            (u64::from(_4btol(&rw10.addr)), _2btol(&rw10.length))
        }
        12 => {
            let rw12 = wire_ref::<ScsiRw12>(bytes);
            (u64::from(_4btol(&rw12.addr)), _4btol(&rw12.length))
        }
        16 => {
            let rw16 = wire_ref::<ScsiRw16>(bytes);
            (_8btol(&rw16.addr), _4btol(&rw16.length))
        }
        _ => (0, 0),
    }
}

/// `vioblk_scsi_cmd`: the adapter's `scsi_cmd`.
pub fn vioblk_scsi_cmd(xs: &'static ScsiXfer) {
    let sc = vioblk_xs_softc(xs);
    let vq = sc.vq();
    let vsc = sc.virtio();
    let cmd = xs.cmd.get();

    let (operation, isread) =
        match vioblk_scsi_op(cmd.opcode, virtio_has_feature(vsc, VIRTIO_BLK_F_FLUSH)) {
            VioblkCmd::Request { operation, isread } => (operation, isread),
            VioblkCmd::Inquiry => {
                vioblk_scsi_inq(xs);
                return;
            }
            VioblkCmd::Capacity => {
                vioblk_scsi_capacity(xs);
                return;
            }
            VioblkCmd::Capacity16 => {
                vioblk_scsi_capacity16(xs);
                return;
            }
            VioblkCmd::Done(error) => {
                vioblk_scsi_done(xs, error);
                return;
            }
            VioblkCmd::Unknown => {
                printf(format_args!("vioblk_scsi_cmd cmd 0x{:02x}\n", cmd.opcode));
                vioblk_scsi_done(xs, XS_DRIVER_STUFFUP);
                return;
            }
        };

    // READ/WRITE/SYNCHRONIZE commands.
    let (lba, sector_count) = vioblk_rw_decode(&cmd, xs.cmdlen.get());

    let s = splbio();
    let Some(io) = xs.io.get() else {
        panic(format_args!(
            "vioblk_scsi_cmd: xs {:p} without a request",
            xs
        ));
    };
    // SAFETY: the transfer's opening came from this adapter's pool, whose `io_get`
    // (vioblk_req_get) hands out slots of `sc_reqs`, mapped while the device exists.
    let vr: &VirtioBlkReq = unsafe { io.cast::<VirtioBlkReq>().as_ref() };
    let slot = i32::from(vr.vr_qe_index.get());
    let len: usize;
    let nsegs: i32;
    if operation != VIRTIO_BLK_T_FLUSH {
        let want = u64::from(sector_count) * u64::from(VIRTIO_BLK_SECTOR_SIZE);
        len = (xs.datalen().max(0) as u64).min(want) as usize;
        let flags = if isread { BUS_DMA_READ } else { BUS_DMA_WRITE } | BUS_DMA_NOWAIT;
        // SAFETY: `set_data`'s contract: `xs.data()` is valid for `datalen` bytes (`len` is
        // no more) and reserved for this transfer until it completes, which unloads the map
        // (vioblk_vq_done1).
        let ret = unsafe { bus_dmamap_load(vsc.dmat(), vr.payload(), xs.data(), len, None, flags) };
        if let Err(e) = ret {
            printf(format_args!(
                "vioblk_scsi_cmd: bus_dmamap_load: {}",
                e as i32
            ));
            // out_done:
            splx(s);
            vioblk_scsi_done(xs, XS_DRIVER_STUFFUP);
            return;
        }
        nsegs = vr.payload().dm_nsegs.get() + 2;
    } else {
        len = 0;
        nsegs = 2;
    }

    // Adjust reservation to the number needed, or virtio gets upset. Note that it may trim
    // UP if 'xs' is being recycled w/o getting a new reservation!
    virtio_enqueue_trim(vq, slot, nsegs);

    vr.vr_xs.set(Some(xs));
    vr.vr_hdr.set(VirtioBlkReqHdr {
        r#type: operation,
        ioprio: 0,
        sector: lba,
    });
    vr.vr_len.set(len as i32);

    bus_dmamap_sync(
        vsc.dmat(),
        vr.cmdsts(),
        0,
        size_of::<VirtioBlkReqHdr>(),
        BUS_DMASYNC_PREWRITE,
    );
    if operation != VIRTIO_BLK_T_FLUSH {
        bus_dmamap_sync(
            vsc.dmat(),
            vr.payload(),
            0,
            len,
            if isread {
                BUS_DMASYNC_PREREAD
            } else {
                BUS_DMASYNC_PREWRITE
            },
        );
    }
    bus_dmamap_sync(
        vsc.dmat(),
        vr.cmdsts(),
        offset_of!(VirtioBlkReq, vr_status),
        size_of::<u8>(),
        BUS_DMASYNC_PREREAD,
    );

    virtio_enqueue_p(vq, slot, vr.cmdsts(), 0, size_of::<VirtioBlkReqHdr>(), true);
    if operation != VIRTIO_BLK_T_FLUSH {
        virtio_enqueue(vq, slot, vr.payload(), !isread);
    }
    virtio_enqueue_p(
        vq,
        slot,
        vr.cmdsts(),
        offset_of!(VirtioBlkReq, vr_status),
        size_of::<u8>(),
        false,
    );
    virtio_enqueue_commit(vsc, vq, slot, true);
    sc.sc_queued.set(sc.sc_queued.get().wrapping_add(1));

    if xs.flags.get() & SCSI_POLL == 0 {
        // check if some xfers are done:
        if sc.sc_queued.get() > 1 {
            virtio_check_vq(vsc, vq);
        }
        splx(s);
        return;
    }

    let mut timeout = 15 * 1000;
    loop {
        if virtio_poll_intr(vsc) != 0 && vr.vr_len.get() == VIOBLK_DONE {
            break;
        }

        delay(1000);
        timeout -= 1;
        if timeout <= 0 {
            break;
        }
    }
    if timeout <= 0 {
        printf(format_args!("vioblk_scsi_cmd: SCSI_POLL timed out\n"));
        vioblk_reset(sc);
        virtio_reinit_start(vsc);
        virtio_reinit_end(vsc);
    }
    splx(s);
}

/// The INQUIRY reply of `vioblk_scsi_inq`: a SPC-3 direct access device, "VirtIO" "Block
/// Device".
pub fn vioblk_inquiry_data() -> ScsiInquiryData {
    let mut inqd = ScsiInquiryData::zeroed();

    inqd.device = T_DIRECT;
    inqd.version = SCSI_REV_SPC3;
    inqd.response_format = SID_SCSI2_RESPONSE;
    inqd.additional_length = SID_SCSI2_ALEN as u8;
    inqd.flags |= SID_CmdQue;
    inqd.vendor.copy_from_slice(b"VirtIO  ");
    inqd.product.copy_from_slice(b"Block Device    ");
    inqd
}

/// `vioblk_scsi_inq`: answers INQUIRY (not the vital product data pages).
pub fn vioblk_scsi_inq(xs: &'static ScsiXfer) {
    let inq = xs.cmd_as::<ScsiInquiry>();

    if inq.flags & SI_EVPD != 0 {
        vioblk_scsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let inqd = vioblk_inquiry_data();

    scsi_copy_internal_data(xs, inqd.as_bytes());

    vioblk_scsi_done(xs, XS_NOERROR);
}

/// The READ CAPACITY (10) reply for a disk of `capacity` sectors: the last LBA (at most
/// `0xffffffff`) and the 512-byte block length.
pub fn vioblk_read_cap_data(capacity: u64) -> ScsiReadCapData {
    let mut rcd = ScsiReadCapData::zeroed();

    let capacity = capacity.wrapping_sub(1).min(0xffff_ffff);

    _lto4b(capacity as u32, &mut rcd.addr);
    _lto4b(VIRTIO_BLK_SECTOR_SIZE, &mut rcd.length);
    rcd
}

/// The READ CAPACITY (16) reply for a disk of `capacity` sectors.
pub fn vioblk_read_cap_data_16(capacity: u64) -> ScsiReadCapData16 {
    let mut rcd = ScsiReadCapData16::zeroed();

    _lto8b(capacity.wrapping_sub(1), &mut rcd.addr);
    _lto4b(VIRTIO_BLK_SECTOR_SIZE, &mut rcd.length);
    rcd
}

/// `bcopy(reply, xs->data, MIN(sizeof(reply), xs->datalen))`.
fn vioblk_copy_reply(xs: &ScsiXfer, reply: &[u8]) {
    // SAFETY: the adapter owns the transfer between `scsi_cmd` and `scsi_done`, and holds no
    // other slice of its data.
    let data = unsafe { xs.data_slice() };
    let n = reply.len().min(data.len());
    data[..n].copy_from_slice(&reply[..n]);
}

/// `vioblk_scsi_capacity`: answers READ CAPACITY (10).
pub fn vioblk_scsi_capacity(xs: &'static ScsiXfer) {
    let sc = vioblk_xs_softc(xs);
    let rcd = vioblk_read_cap_data(sc.sc_capacity.get());

    vioblk_copy_reply(xs, rcd.as_bytes());
    vioblk_scsi_done(xs, XS_NOERROR);
}

/// `vioblk_scsi_capacity16`: answers READ CAPACITY (16).
pub fn vioblk_scsi_capacity16(xs: &'static ScsiXfer) {
    let sc = vioblk_xs_softc(xs);
    let rcd = vioblk_read_cap_data_16(sc.sc_capacity.get());

    vioblk_copy_reply(xs, rcd.as_bytes());
    vioblk_scsi_done(xs, XS_NOERROR);
}

/// `vioblk_scsi_done`: finishes `xs` with `error` (`XS_*`).
pub fn vioblk_scsi_done(xs: &'static ScsiXfer, error: i32) {
    xs.error.set(error);
    scsi_done(xs);
}

/// `vioblk_alloc_reqs`: allocates the request slots (one per `ALLOC_SEGS` descriptors, or
/// one per descriptor with indirect descriptors), reserves each one's descriptors and makes
/// its maps; returns how many it made, 0 on failure.
pub fn vioblk_alloc_reqs(sc: &VioblkSoftc, qsize: usize) -> usize {
    let vq = sc.vq();
    let dmat = sc.virtio().dmat();

    let mut nreqs = if !vq.vq_indirect.get().is_null() {
        qsize
    } else {
        qsize / ALLOC_SEGS
    };

    let allocsize = size_of::<VirtioBlkReq>() * nreqs;
    let mut segs = [BusDmaSegment::default(); 1];
    if let Err(r) = bus_dmamem_alloc(
        dmat,
        allocsize,
        0,
        0,
        &mut segs,
        BUS_DMA_NOWAIT | BUS_DMA_64BIT,
    ) {
        printf(format_args!(
            "DMA memory allocation failed, size {allocsize}, error {}\n",
            r as i32
        ));
        // err_none:
        return 0;
    }
    sc.sc_reqs_segs[0].set(segs[0]);
    let vaddr = match bus_dmamem_map(dmat, &mut segs, allocsize, BUS_DMA_NOWAIT) {
        Ok(vaddr) => vaddr,
        Err(r) => {
            printf(format_args!("DMA memory map failed, error {}\n", r as i32));
            // err_dmamem_alloc:
            // SAFETY: the segment allocated above, not mapped.
            unsafe { bus_dmamem_free(dmat, &segs) };
            return 0;
        }
    };
    sc.sc_reqs.set(vaddr.as_ptr().cast());
    // SAFETY: `vaddr` maps `allocsize` bytes, just allocated and used by nothing else.
    unsafe { ptr::write_bytes(vaddr.as_ptr(), 0, allocsize) };

    let failed: Result<(), ()> = 'err_reqs: {
        for i in 0..nreqs {
            // Assign descriptors and create the DMA maps for each allocated request.
            // SAFETY: `i` is below `nreqs`, the slots mapped (and zeroed) above.
            let vr = unsafe { vioblk_req_at(sc.sc_reqs.get(), i) };
            let slot = match virtio_enqueue_prep(vq) {
                Ok(slot) => slot,
                Err(_) => return i,
            };
            if virtio_enqueue_reserve(vq, slot, ALLOC_SEGS as i32).is_err() {
                return i;
            }

            if vq.vq_indirect.get().is_null() {
                // The reserved slots must be a contiguous block starting at vq_desc[slot].
                let slot = slot as usize;
                let mut r = 0;
                while r < ALLOC_SEGS - 1 {
                    let next = vioblk_desc_next(vq, slot + r);
                    if VIRTIO_DEBUG >= 2 {
                        printf(format_args!(
                            "vioblk_alloc_reqs: vd[{r}].next = {next:?} should be {}\n",
                            slot + r + 1
                        ));
                    }
                    if next.map(usize::from) != Some(slot + r + 1) {
                        return i;
                    }
                    r += 1;
                }
                if vioblk_desc_next(vq, slot + r) != Some(0) {
                    return i;
                }
                if VIRTIO_DEBUG >= 2 {
                    printf(format_args!(
                        "vioblk_alloc_reqs: reserved slots are contiguous (good!)\n"
                    ));
                }
            }

            // Slots and request counts are below the queue size, at most 32768.
            vr.vr_qe_index.set(slot as i16);
            vq.entry(slot as usize).qe_vr_index.set(i as i16);
            vr.vr_len.set(VIOBLK_DONE);

            match bus_dmamap_create(
                dmat,
                VR_DMA_END,
                1,
                VR_DMA_END,
                0,
                BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW | BUS_DMA_64BIT,
            ) {
                Ok(map) => vr.vr_cmdsts.set(Some(map)),
                Err(r) => {
                    printf(format_args!(
                        "cmd dmamap creation failed, err {}\n",
                        r as i32
                    ));
                    nreqs = i;
                    break 'err_reqs Err(());
                }
            }
            // SAFETY: the slot's first `VR_DMA_END` bytes (header and status), in the DMA
            // memory mapped above, which stays until the error path below unmaps it after
            // destroying the maps.
            if let Err(r) = unsafe {
                bus_dmamap_load(
                    dmat,
                    vr.cmdsts(),
                    vr.vr_hdr.as_ptr().cast(),
                    VR_DMA_END,
                    None,
                    BUS_DMA_NOWAIT,
                )
            } {
                printf(format_args!(
                    "command dmamap load failed, err {}\n",
                    r as i32
                ));
                nreqs = i;
                break 'err_reqs Err(());
            }
            match bus_dmamap_create(
                dmat,
                MAXPHYS,
                SEG_MAX as i32,
                MAXPHYS,
                0,
                BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW | BUS_DMA_64BIT,
            ) {
                Ok(map) => vr.vr_payload.set(Some(map)),
                Err(r) => {
                    printf(format_args!(
                        "payload dmamap creation failed, err {}\n",
                        r as i32
                    ));
                    nreqs = i;
                    break 'err_reqs Err(());
                }
            }
            // SAFETY: the slot is on no list yet and stays in the DMA memory while the
            // device exists.
            unsafe { sc.sc_freelist.insert_head(vr) };
        }
        Ok(())
    };
    if failed.is_ok() {
        return nreqs;
    }

    // err_reqs:
    for i in 0..nreqs {
        // SAFETY: as above.
        let vr = unsafe { vioblk_req_at(sc.sc_reqs.get(), i) };
        if let Some(map) = vr.vr_cmdsts.take() {
            // SAFETY: the slot's own map, which nothing else holds.
            unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
        }
        if let Some(map) = vr.vr_payload.take() {
            // SAFETY: as above.
            unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
        }
    }
    // SAFETY: the mapping made above; its maps are gone.
    unsafe { bus_dmamem_unmap(dmat, vaddr, allocsize) };
    // err_dmamem_alloc:
    // SAFETY: the segment allocated above, now unmapped.
    unsafe { bus_dmamem_free(dmat, &segs) };
    // err_none:
    0
}

/// `vq->vq_desc[i].next`; `None` when `i` is past the descriptor table (the C reads on;
/// such a block is not contiguous either way).
fn vioblk_desc_next(vq: &Virtqueue, i: usize) -> Option<u16> {
    let vd = vq.vq_desc.get();
    if vd.is_null() || i >= vq.vq_num.get() as usize {
        return None;
    }
    // SAFETY: `virtio_alloc_vq` allocated `vq_num` descriptors at `vq_desc`, in DMA memory
    // the device may read concurrently: read volatile.
    Some(unsafe { ptr::read_volatile(&raw const (*vd.add(i)).next) })
}

const _: () = assert!(VR_DMA_END == size_of::<VirtioBlkReqHdr>() + 2);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of `vioblk.rs`: the request slot free list, the SCSI command translation and
    // the replies the driver fakes.

    use std::alloc::{Layout, alloc_zeroed};
    use std::boxed::Box;
    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::scsi::scsiconf::{ScsiLink, ScsibusSoftc};

    /// A leaked, zero-filled `T` (all-zero is valid for the softcs and the request slots).
    fn zeroed<T>(n: usize) -> *mut T {
        let layout = Layout::array::<T>(n).unwrap();
        // SAFETY: a non-zero layout.
        let p = unsafe { alloc_zeroed(layout) };
        assert!(!p.is_null());
        p.cast()
    }

    /// A vioblk softc with `n` request slots on its free list (in `vioblk_alloc_reqs`' order),
    /// `capacity` sectors, over a virtio softc with `features`.
    fn softc(n: usize, capacity: u64, features: u64) -> &'static VioblkSoftc {
        // SAFETY: leaked zeroed blocks; all-zero is a valid softc.
        let sc: &'static VioblkSoftc = unsafe { &*zeroed::<VioblkSoftc>(1) };
        // SAFETY: as above.
        let vsc: &'static VirtioSoftc = unsafe { &*zeroed::<VirtioSoftc>(1) };
        vsc.sc_active_features.set(features);
        sc.sc_virtio.set(Some(vsc));
        sc.sc_capacity.set(capacity);
        sc.sc_freelist.init();
        mtx_init(&sc.sc_vr_mtx, IPL_BIO);
        sc.sc_reqs.set(zeroed::<VirtioBlkReq>(n));
        sc.sc_nreqs.set(n);
        for i in 0..n {
            let vr = sc.req(i);
            vr.vr_qe_index.set((i * ALLOC_SEGS) as i16);
            vr.vr_len.set(VIOBLK_DONE);
            // SAFETY: a fresh slot, leaked.
            unsafe { sc.sc_freelist.insert_head(vr) };
        }
        sc
    }

    fn cookie(sc: &VioblkSoftc) -> *mut c_void {
        ptr::from_ref(sc).cast_mut().cast()
    }

    /// The slot index of an opening.
    fn index(sc: &VioblkSoftc, io: ScsiIo) -> usize {
        (io.as_ptr() as usize - sc.sc_reqs.get() as usize) / size_of::<VirtioBlkReq>()
    }

    #[test]
    fn the_request_layout_is_the_c_one() {
        assert_eq!(offset_of!(VirtioBlkReq, vr_status), 16);
        assert_eq!(VR_DMA_END, 18);
        assert_eq!(SEG_MAX, MAXPHYS / PAGE_SIZE + 1);
        assert_eq!(ALLOC_SEGS, SEG_MAX + 2);
    }

    #[test]
    fn req_get_and_put_cycle_the_free_list() {
        let sc = softc(3, 0, 0);
        let mut got = Vec::new();
        // SAFETY: the cookie is the live softc; every opening goes back once.
        unsafe {
            while let Some(io) = vioblk_req_get(cookie(sc)) {
                got.push(index(sc, io));
            }
        }
        // SLIST_INSERT_HEAD: the last slot made is handed out first.
        assert_eq!(got, [2, 1, 0]);
        assert!(sc.sc_freelist.is_empty());

        let io = NonNull::from(sc.req(1)).cast();
        // SAFETY: slot 1 was taken above and is on no list.
        unsafe { vioblk_req_put(cookie(sc), io) };
        // SAFETY: as above.
        let again = unsafe { vioblk_req_get(cookie(sc)) }.unwrap();
        assert_eq!(index(sc, again), 1);
        // SAFETY: as above.
        assert!(unsafe { vioblk_req_get(cookie(sc)) }.is_none());
    }

    #[test]
    fn opcodes_map_to_virtio_requests() {
        let rd = VioblkCmd::Request {
            operation: VIRTIO_BLK_T_IN,
            isread: true,
        };
        let wr = VioblkCmd::Request {
            operation: VIRTIO_BLK_T_OUT,
            isread: false,
        };
        for op in [READ_COMMAND, READ_10, READ_12, READ_16] {
            assert_eq!(vioblk_scsi_op(op, false), rd);
        }
        for op in [WRITE_COMMAND, WRITE_10, WRITE_12, WRITE_16] {
            assert_eq!(vioblk_scsi_op(op, true), wr);
        }
        assert_eq!(
            vioblk_scsi_op(SYNCHRONIZE_CACHE, true),
            VioblkCmd::Request {
                operation: VIRTIO_BLK_T_FLUSH,
                isread: false
            }
        );
        assert_eq!(
            vioblk_scsi_op(SYNCHRONIZE_CACHE, false),
            VioblkCmd::Done(XS_NOERROR)
        );
        assert_eq!(vioblk_scsi_op(INQUIRY, false), VioblkCmd::Inquiry);
        assert_eq!(vioblk_scsi_op(READ_CAPACITY, false), VioblkCmd::Capacity);
        assert_eq!(
            vioblk_scsi_op(READ_CAPACITY_16, false),
            VioblkCmd::Capacity16
        );
        for op in [TEST_UNIT_READY, START_STOP, PREVENT_ALLOW] {
            assert_eq!(vioblk_scsi_op(op, false), VioblkCmd::Done(XS_NOERROR));
        }
        for op in [MODE_SENSE, MODE_SENSE_BIG, REPORT_LUNS] {
            assert_eq!(
                vioblk_scsi_op(op, false),
                VioblkCmd::Done(XS_DRIVER_STUFFUP)
            );
        }
        assert_eq!(vioblk_scsi_op(0x03, false), VioblkCmd::Unknown); // REQUEST SENSE
    }

    fn cdb(bytes: &[u8]) -> ScsiGeneric {
        ScsiGeneric::read_from(bytes)
    }

    #[test]
    fn cdbs_decode_by_length() {
        // READ (6): 21-bit address, length 0 means 256.
        let c = cdb(&[READ_COMMAND, 0xff, 0x34, 0x56, 0, 0]);
        assert_eq!(vioblk_rw_decode(&c, 6), (0x1f_3456, 0x100));
        let c = cdb(&[WRITE_COMMAND, 0x01, 0x02, 0x03, 8, 0]);
        assert_eq!(vioblk_rw_decode(&c, 6), (0x01_0203, 8));
        // READ (10) and SYNCHRONIZE CACHE (10), same layout.
        let c = cdb(&[READ_10, 0, 0x12, 0x34, 0x56, 0x78, 0, 0x01, 0x00, 0]);
        assert_eq!(vioblk_rw_decode(&c, 10), (0x1234_5678, 0x100));
        let c = cdb(&[SYNCHRONIZE_CACHE, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(vioblk_rw_decode(&c, 10), (0, 0));
        // WRITE (12).
        let c = cdb(&[WRITE_12, 0, 0, 0, 0x10, 0, 0, 0x01, 0, 0, 0, 0]);
        assert_eq!(vioblk_rw_decode(&c, 12), (0x1000, 0x1_0000));
        // READ (16): 64-bit address.
        let c = cdb(&[
            READ_16, 0, 0x01, 0, 0, 0, 0, 0, 0, 0x02, 0, 0, 0, 0x80, 0, 0,
        ]);
        assert_eq!(vioblk_rw_decode(&c, 16), (0x0100_0000_0000_0002, 0x80));
        // Any other length leaves the C's zeroes.
        assert_eq!(vioblk_rw_decode(&c, 8), (0, 0));
    }

    #[test]
    fn the_inquiry_reply_is_a_virtio_disk() {
        let inqd = vioblk_inquiry_data();
        let b = inqd.as_bytes();
        assert_eq!(
            &b[..8],
            &[T_DIRECT, 0, SCSI_REV_SPC3, 2, 31, 0, 0, SID_CmdQue]
        );
        assert_eq!(&b[8..16], b"VirtIO  ");
        assert_eq!(&b[16..32], b"Block Device    ");
        assert!(b[32..].iter().all(|&x| x == 0));
    }

    #[test]
    fn capacity_replies_give_the_last_lba() {
        let rcd = vioblk_read_cap_data(2048);
        assert_eq!(rcd.as_bytes(), &[0, 0, 0x07, 0xff, 0, 0, 0x02, 0x00]);
        // Past 2^32 sectors, READ CAPACITY (10) says 0xffffffff.
        let rcd = vioblk_read_cap_data(0x1_0000_0002);
        assert_eq!(&rcd.addr, &[0xff; 4]);
        let rcd = vioblk_read_cap_data_16(0x1_0000_0002);
        assert_eq!(&rcd.addr, &[0, 0, 0, 0x01, 0, 0, 0, 0x01]);
        assert_eq!(&rcd.length, &[0, 0, 0x02, 0x00]);
        assert!(rcd.as_bytes()[12..].iter().all(|&x| x == 0));
    }

    fn nodone(_xs: &'static ScsiXfer) {}

    /// A transfer on a link of a bus whose adapter softc is `sc`, with `cdb` and `data`.
    fn xfer(sc: &'static VioblkSoftc, cdb: &[u8], data: &'static mut [u8]) -> &'static ScsiXfer {
        // SAFETY: a leaked zeroed block; all-zero is a valid bus softc.
        let bus: &'static ScsibusSoftc = unsafe { &*zeroed::<ScsibusSoftc>(1) };
        bus.sb_adapter_softc.set(cookie(sc));
        bus.sb_adapter.set(Some(&VIOBLK_SWITCH));
        let link: &'static ScsiLink = Box::leak(Box::new(ScsiLink::new()));
        link.bus.set(Some(bus));
        let xs: &'static ScsiXfer = Box::leak(Box::new(ScsiXfer::new()));
        xs.sc_link.set(Some(link));
        xs.cmd.set(ScsiGeneric::read_from(cdb));
        xs.cmdlen.set(cdb.len() as i32);
        // SAFETY: leaked, used by this transfer only.
        unsafe { xs.set_data(data.as_mut_ptr(), data.len() as i32) };
        xs.done.set(Some(nodone));
        xs.error.set(-1);
        xs
    }

    fn buf(n: usize) -> &'static mut [u8] {
        Box::leak(std::vec![0xaau8; n].into_boxed_slice())
    }

    #[test]
    fn faked_commands_complete_at_once() {
        let sc = softc(1, 4096, 0);

        let data = buf(96);
        let p = data.as_ptr();
        let xs = xfer(sc, &[INQUIRY, 0, 0, 0, 96, 0], data);
        vioblk_scsi_cmd(xs);
        assert_eq!(xs.error.get(), XS_NOERROR);
        assert_eq!(xs.resid.get(), 0);
        // SAFETY: the leaked buffer, no longer written.
        let got = unsafe { core::slice::from_raw_parts(p, 96) };
        assert_eq!(got, vioblk_inquiry_data().as_bytes());

        // EVPD pages are refused.
        let xs = xfer(sc, &[INQUIRY, SI_EVPD, 0x80, 0, 96, 0], buf(96));
        vioblk_scsi_cmd(xs);
        assert_eq!(xs.error.get(), XS_DRIVER_STUFFUP);

        // READ CAPACITY copies at most datalen bytes.
        let data = buf(6);
        let p = data.as_ptr();
        let xs = xfer(sc, &[READ_CAPACITY, 0, 0, 0, 0, 0, 0, 0, 0, 0], data);
        vioblk_scsi_cmd(xs);
        assert_eq!(xs.error.get(), XS_NOERROR);
        // SAFETY: as above.
        let got = unsafe { core::slice::from_raw_parts(p, 6) };
        assert_eq!(got, &[0, 0, 0x0f, 0xff, 0, 0]);

        let data = buf(32);
        let p = data.as_ptr();
        let mut c16 = [0u8; 16];
        c16[0] = READ_CAPACITY_16;
        let xs = xfer(sc, &c16, data);
        vioblk_scsi_cmd(xs);
        assert_eq!(xs.error.get(), XS_NOERROR);
        // SAFETY: as above.
        let got = unsafe { core::slice::from_raw_parts(p, 32) };
        assert_eq!(got, vioblk_read_cap_data_16(4096).as_bytes());

        for c in [TEST_UNIT_READY, START_STOP, PREVENT_ALLOW] {
            let xs = xfer(sc, &[c, 0, 0, 0, 0, 0], buf(1));
            vioblk_scsi_cmd(xs);
            assert_eq!(xs.error.get(), XS_NOERROR);
        }
        for c in [MODE_SENSE, REPORT_LUNS, 0x03] {
            let xs = xfer(sc, &[c, 0, 0, 0, 0, 0], buf(1));
            vioblk_scsi_cmd(xs);
            assert_eq!(xs.error.get(), XS_DRIVER_STUFFUP);
        }
        // Without VIRTIO_BLK_F_FLUSH, SYNCHRONIZE CACHE succeeds at once.
        let xs = xfer(sc, &[SYNCHRONIZE_CACHE, 0, 0, 0, 0, 0, 0, 0, 0, 0], buf(1));
        vioblk_scsi_cmd(xs);
        assert_eq!(xs.error.get(), XS_NOERROR);
    }
}
/* </TESTS> */
