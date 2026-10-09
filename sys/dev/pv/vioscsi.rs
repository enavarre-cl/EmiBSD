/* <LICENSES> */
/*
 * Copyright (c) 2013 Google Inc.
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
//! `vioscsi(4)`: the virtio SCSI host adapter driver (`vioscsi* at virtio?`), a SCSI
//! adapter whose bus is the device's targets and logical units.
//!
//! Upstream: sys/dev/pv/vioscsi.c @ 3ce1f3f79392
//!
//! Three virtqueues: control and event (allocated, never used, as in the C) and the request
//! queue. Each request slot (`struct vioscsi_req`, in DMA memory) owns a contiguous block of
//! `ALLOC_SEGS` descriptors (or one indirect slot), a map for its request and response
//! headers and a map for the payload, all made at attach time; the free slots are the
//! adapter's `scsi_iopool`, so the SCSI midlayer only issues a command once its slot exists.
//! `vioscsi_scsi_cmd` puts the CDB, the LUN in the single-level format the spec demands and
//! the payload on the queue; `vioscsi_vq_done` completes the requests the device has used
//! with the status and sense data of the response header. With `SCSI_POLL` the command
//! polls the interrupt handler for up to a second.
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the device first; it is reached as `&'static` (softcs are
//!   never freed while the device exists). Its members are `Cell`s, all-zero valid. The
//!   parent's softc is kept in `sc_virtio` (the C reads it from `sc_dev.dv_parent`), as
//!   `vioblk.rs` does.
//! - `struct vioscsi_req` keeps the C's layout (`#[repr(C)]`, the two packed headers first,
//!   `VR_DMA_END` = `offset_of!(VioscsiReq, vr_xs)`), because its first bytes are what the
//!   device reads and writes; the response header the device writes is read volatile as a
//!   whole. The slots live in the `bus_dmamem` area `sc_reqs` points to, reached through
//!   the bounds-checked `VioscsiSoftc::req`.
//! - The part of `vioscsi_req_done` that turns the response header into the transfer's error,
//!   status, residual and sense data is the function `vioscsi_xs_result`, split out so that it
//!   is host-tested; the C does it inline.
//! - `vr_xs`, `vr_control` and `vr_data` are `Option`s: a slot without a transfer or a map
//!   panics where the C would dereference NULL.
//! - `sc_nreqs` is a `usize`.
//! - `DPRINTF` is `if VIOSCSI_DEBUG { printf(..) }`, `vioscsi_debug` being the C's
//!   `enum { vioscsi_debug = 0 }`.
//! - The contiguity check of `vioscsi_alloc_reqs` reads `vq_desc[slot + r].next` volatile
//!   (`vioscsi_desc_next`) and takes an index past the descriptor table as "not
//!   contiguous" where the C would read past it. As the C, a failure part way through
//!   returns the number of slots made so far and keeps them.
//! - `virtio_negotiate_features` is given no feature names (the C passes NULL), and no
//!   driver features are requested (`sc_driver_features` stays 0): `VIRTIO_SCSI_F_INOUT` and
//!   `_HOTPLUG` are not used, as the C's "TODO(matthew)" notes.
//! - No stubs: every function of the file is ported.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};

use crate::dev::pv::vioscsireg::{
    VIRTIO_SCSI_CONFIG_CMD_PER_LUN, VIRTIO_SCSI_CONFIG_MAX_LUN, VIRTIO_SCSI_CONFIG_MAX_TARGET,
    VIRTIO_SCSI_CONFIG_SEG_MAX, VIRTIO_SCSI_S_OK, VIRTIO_SCSI_S_SIMPLE, VirtioScsiReqHdr,
    VirtioScsiResHdr,
};
use crate::dev::pv::virtio::{
    virtio_alloc_vq, virtio_attach_finish, virtio_dequeue, virtio_enqueue, virtio_enqueue_commit,
    virtio_enqueue_p, virtio_enqueue_prep, virtio_enqueue_reserve, virtio_enqueue_trim,
};
use crate::dev::pv::virtioreg::PCI_PRODUCT_VIRTIO_SCSI;
use crate::dev::pv::virtiovar::{
    VIRTIO_CHILD_ERROR, VirtioAttachArgs, VirtioSoftc, Virtqueue, virtio_negotiate_features,
    virtio_poll_intr, virtio_read_device_config_2, virtio_read_device_config_4,
};
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_64BIT, BUS_DMA_ALLOCNOW, BUS_DMA_NOWAIT, BUS_DMA_READ, BUS_DMA_WRITE,
    BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE,
    BusDmaSegment, BusDmamap, bus_dmamap_create, bus_dmamap_load, bus_dmamap_sync,
    bus_dmamap_unload, bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_map,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{IPL_BIO, splbio, splx};
use crate::queue_adapter;
use crate::scsi::scsi_all::ScsiWire;
use crate::scsi::scsi_base::{scsi_done, scsi_iopool_init};
use crate::scsi::scsiconf::{
    SCSI_DATA_IN, SCSI_DATA_OUT, SCSI_POLL, SDEV_NO_ADAPTER_TARGET, ScsiAdapter, ScsiIo,
    ScsiIopool, ScsiXfer, ScsibusAttachArgs, XS_DRIVER_STUFFUP, XS_NOERROR, XS_SENSE, XS_TIMEOUT,
    scsiprint,
};
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::mutex::Mutex;
use crate::sys::param::{MAXPHYS, PAGE_SIZE};
use crate::sys::queue::{SlistEntry, SlistHead};

/// `vioscsi_debug`: `DPRINTF` prints when set.
const VIOSCSI_DEBUG: bool = false;

/// `SEG_MAX`: number of DMA segments for buffers that the device must support.
pub const SEG_MAX: usize = MAXPHYS / PAGE_SIZE + 1;
/// `ALLOC_SEGS`: in the virtqueue, we need space for header and footer, too.
pub const ALLOC_SEGS: usize = SEG_MAX + 2;

/// `struct vioscsi_req`: one request slot. The request and response headers (up to
/// [`VR_DMA_END`]) are what the device reads and writes, through `vr_control`.
///
/// The slots live in zeroed DMA memory: every member is valid as all-zero bits.
#[repr(C)]
pub struct VioscsiReq {
    /// `vr_req`: the request header the device reads.
    pub vr_req: Cell<VirtioScsiReqHdr>,
    /// `vr_res`: the response header the device writes; read volatile.
    pub vr_res: Cell<VirtioScsiResHdr>,
    /// `vr_xs`: the transfer in flight.
    pub vr_xs: Cell<Option<&'static ScsiXfer>>,
    /// `vr_control`: the map of the two headers.
    pub vr_control: Cell<Option<&'static BusDmamap>>,
    /// `vr_data`: the map of the transfer's data.
    pub vr_data: Cell<Option<&'static BusDmamap>>,
    /// `vr_list`: `sc_freelist`.
    pub vr_list: SlistEntry<VioscsiReq>,
    /// `vr_qe_index`: the head of the slot's descriptors.
    pub vr_qe_index: Cell<i32>,
}

impl VioscsiReq {
    /// `vr->vr_control`, which every allocated slot has.
    fn control(&self) -> &'static BusDmamap {
        match self.vr_control.get() {
            Some(map) => map,
            None => panic(format_args!("vioscsi: request without a control map")),
        }
    }

    /// `vr->vr_data`, which every allocated slot has.
    fn data(&self) -> &'static BusDmamap {
        match self.vr_data.get() {
            Some(map) => map,
            None => panic(format_args!("vioscsi: request without a data map")),
        }
    }

    /// `vr->vr_xs`, set while the slot is in flight.
    fn xs(&self) -> &'static ScsiXfer {
        match self.vr_xs.get() {
            Some(xs) => xs,
            None => panic(format_args!("vioscsi: request without a transfer")),
        }
    }

    /// Whether `xs` is the transfer in flight (`vr->vr_xs == xs`).
    fn is_xs(&self, xs: &ScsiXfer) -> bool {
        self.vr_xs.get().is_some_and(|x| ptr::eq(x, xs))
    }

    /// `vr->vr_res`, as the device left it.
    fn res(&self) -> VirtioScsiResHdr {
        // SAFETY: the cell's own bytes, inside the live slot; the device writes them by DMA
        // behind the compiler's back, hence volatile; any bytes are a valid header.
        unsafe { ptr::read_volatile(self.vr_res.as_ptr()) }
    }
}

/// The bytes of the request and response headers, the part of a slot the device sees:
/// `offsetof(struct vioscsi_req, vr_xs)`.
pub const VR_DMA_END: usize = offset_of!(VioscsiReq, vr_xs);

queue_adapter!(
    /// `SLIST_HEAD(, vioscsi_req)`: `sc_freelist`, through `vr_list`.
    pub VioscsiFreelist: VioscsiReq, vr_list => SlistEntry<VioscsiReq>
);

/// `struct vioscsi_softc`.
#[repr(C)]
pub struct VioscsiSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_virtio`: the parent.
    pub sc_virtio: Cell<Option<&'static VirtioSoftc>>,
    /// `sc_iopool`: the free slots, for the SCSI midlayer.
    pub sc_iopool: ScsiIopool,
    /// `sc_vr_mtx`: protects `sc_freelist`.
    pub sc_vr_mtx: Mutex,

    /// `sc_vqs`: control, event and request.
    pub sc_vqs: [Virtqueue; 3],
    /// `sc_reqs`: `sc_nreqs` request slots in DMA memory.
    pub sc_reqs: Cell<*mut VioscsiReq>,
    /// `sc_nreqs`.
    pub sc_nreqs: Cell<usize>,
    /// `sc_reqs_segs`.
    pub sc_reqs_segs: [Cell<BusDmaSegment>; 1],
    /// `sc_freelist`.
    pub sc_freelist: SlistHead<VioscsiFreelist>,
}

impl VioscsiSoftc {
    /// `sc->sc_dev.dv_parent`, which `vioscsi_attach` stores first.
    pub fn virtio(&self) -> &'static VirtioSoftc {
        match self.sc_virtio.get() {
            Some(vsc) => vsc,
            None => panic(format_args!("vioscsi: no virtio softc")),
        }
    }

    /// `&sc->sc_vqs[2]`: the request queue.
    pub fn vq(&self) -> &Virtqueue {
        &self.sc_vqs[2]
    }

    /// `&sc->sc_reqs[i]`, `i` below `sc_nreqs`.
    pub fn req(&self, i: usize) -> &'static VioscsiReq {
        if i >= self.sc_nreqs.get() {
            panic(format_args!(
                "{}: bad request slot {i}",
                self.sc_dev.xname()
            ));
        }
        // SAFETY: `vioscsi_alloc_reqs` mapped (zeroed) room for `sc_nreqs` slots at
        // `sc_reqs`, which stays mapped while the device exists.
        unsafe { vioscsi_req_at(self.sc_reqs.get(), i) }
    }
}

// SAFETY: `#[repr(C)]` with the device first; the virtqueues, the iopool, the mutex and the
// list head are all-zero valid (`dev/pv/virtiovar.rs`, `scsi/scsiconf.rs`, `sys/mutex.rs`,
// `sys/queue.rs`), and every other member is a `Cell` of an integer, a raw pointer, a DMA
// segment or an `Option` of a reference.
unsafe impl Softc for VioscsiSoftc {}

/// `vioscsi_ca`.
pub static VIOSCSI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<VioscsiSoftc>(),
    ca_match: Some(vioscsi_match),
    ca_attach: vioscsi_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `vioscsi_cd`.
pub static VIOSCSI_CD: Cfdriver = Cfdriver::new(b"vioscsi", DV_DULL, CD_COCOVM);

/// `vioscsi_switch`: the adapter's entry points.
pub static VIOSCSI_SWITCH: ScsiAdapter = ScsiAdapter {
    scsi_cmd: vioscsi_scsi_cmd,
    dev_minphys: None,
    dev_probe: None,
    dev_free: None,
    ioctl: None,
};

/// `vioscsi_vq_names[]`.
pub static VIOSCSI_VQ_NAMES: [&str; 3] = ["control", "event", "request"];

/// `&base[i]` of the request slots.
///
/// # Safety
///
/// `base` maps at least `i + 1` zeroed or initialised slots that stay mapped while the
/// device exists.
unsafe fn vioscsi_req_at(base: *mut VioscsiReq, i: usize) -> &'static VioscsiReq {
    if base.is_null() {
        panic(format_args!("vioscsi: no request slots"));
    }
    // SAFETY: the caller's guarantee; all-zero bits are a valid slot.
    unsafe { &*base.add(i) }
}

/// `(struct vioscsi_softc *)vsc->sc_child`.
fn vioscsi_child(vsc: &VirtioSoftc) -> &'static VioscsiSoftc {
    let c = vsc.sc_child.get();
    if c.is_null() || c == VIRTIO_CHILD_ERROR {
        panic(format_args!("vioscsi: virtio without its child"));
    }
    // SAFETY: vioscsi_attach stored its own device, the head of a `VioscsiSoftc` that lives
    // as long as the kernel; only vioscsi's queue handler calls this.
    unsafe { &*c.cast::<VioscsiSoftc>() }
}

/// `xs->sc_link->bus->sb_adapter_softc`: the softc of the vioscsi a transfer is for.
fn vioscsi_xs_softc(xs: &ScsiXfer) -> &'static VioscsiSoftc {
    let p = xs.link().bus().sb_adapter_softc.get();
    if p.is_null() {
        panic(format_args!("vioscsi: bus without an adapter softc"));
    }
    // SAFETY: vioscsi_attach attaches its scsibus with its own softc as `saa_adapter_softc`,
    // and only that bus's transfers reach `vioscsi_switch`; softcs are never freed.
    unsafe { &*p.cast::<VioscsiSoftc>().cast_const() }
}

/// `vioscsi_match`: the virtio SCSI host adapter.
pub fn vioscsi_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: virtio transports attach their children with arguments that begin with a
    // `virtio_attach_args`.
    let va = unsafe { &*aux.cast::<VirtioAttachArgs>() };

    i32::from(va.va_devid == PCI_PRODUCT_VIRTIO_SCSI)
}

/// `vioscsi_attach`.
pub fn vioscsi_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `vioscsi_ca`, whose softc is a `VioscsiSoftc`; softcs are
    // never freed while the device exists, so the softc may be borrowed for 'static.
    let sc: &'static VioscsiSoftc = unsafe { &*ptr::from_ref(self_.softc::<VioscsiSoftc>()) };
    let Some(parent) = parent else {
        return;
    };
    // SAFETY: `vioscsi* at virtio?`: the parent is a virtio transport, whose softc begins
    // with the `struct virtio_softc`; it outlives its child.
    let vsc: &'static VirtioSoftc = unsafe { &*ptr::from_ref(parent.softc::<VirtioSoftc>()) };
    let va = aux.cast::<VirtioAttachArgs>();

    if !vsc.sc_child.get().is_null() {
        printf(format_args!(": parent already has a child\n"));
        return;
    }
    vsc.sc_child.set(ptr::from_ref(self_));
    vsc.sc_ipl.set(IPL_BIO);
    sc.sc_virtio.set(Some(vsc));

    // TODO(matthew): Negotiate hotplug.

    vsc.sc_vqs.set(sc.sc_vqs.as_ptr().cast_mut());
    vsc.sc_nvqs.set(sc.sc_vqs.len() as i32);

    let attached: Result<(), ()> = 'err: {
        if virtio_negotiate_features(vsc, None).is_err() {
            break 'err Err(());
        }
        let cmd_per_lun = virtio_read_device_config_4(vsc, VIRTIO_SCSI_CONFIG_CMD_PER_LUN);
        let seg_max = virtio_read_device_config_4(vsc, VIRTIO_SCSI_CONFIG_SEG_MAX);
        let max_target = virtio_read_device_config_2(vsc, VIRTIO_SCSI_CONFIG_MAX_TARGET);
        let max_lun = virtio_read_device_config_4(vsc, VIRTIO_SCSI_CONFIG_MAX_LUN);

        if (seg_max as usize) < SEG_MAX {
            printf(format_args!(
                "\nMax number of segments {} too small\n",
                seg_max as i32
            ));
            break 'err Err(());
        }

        for (i, vq) in sc.sc_vqs.iter().enumerate() {
            if virtio_alloc_vq(vsc, vq, i as i32, ALLOC_SEGS as i32, VIOSCSI_VQ_NAMES[i]).is_err() {
                printf(format_args!(": failed to allocate virtqueue {i}\n"));
                break 'err Err(());
            }
            vq.vq_done.set(Some(vioscsi_vq_done));
        }

        let qsize = sc.vq().vq_num.get() as usize;
        printf(format_args!(": qsize {qsize}\n"));

        sc.sc_freelist.init();
        mtx_init(&sc.sc_vr_mtx, IPL_BIO);
        // SAFETY: vioscsi_req_get and vioscsi_req_put take this softc as their cookie, and the
        // softc is never freed.
        unsafe {
            scsi_iopool_init(
                &sc.sc_iopool,
                ptr::from_ref(sc).cast_mut().cast(),
                vioscsi_req_get,
                vioscsi_req_put,
            )
        };

        let nreqs = vioscsi_alloc_reqs(sc, qsize);
        if nreqs == 0 {
            printf(format_args!("\nCan't alloc reqs\n"));
            break 'err Err(());
        }
        sc.sc_nreqs.set(nreqs);

        let mut saa = ScsibusAttachArgs::new();
        saa.saa_adapter = Some(&VIOSCSI_SWITCH);
        saa.saa_adapter_softc = ptr::from_ref(self_).cast_mut().cast();
        saa.saa_adapter_target = SDEV_NO_ADAPTER_TARGET;
        saa.saa_adapter_buswidth = max_target;
        saa.saa_luns = (max_lun + 1).min(u32::from(u8::MAX)) as u8;
        // At most `cmd_per_lun` and the queue size (at most 32768).
        saa.saa_openings = if nreqs as u32 > cmd_per_lun {
            cmd_per_lun as u16
        } else {
            nreqs as u16
        };
        saa.saa_pool = Some(&sc.sc_iopool);
        saa.saa_quirks = 0;
        saa.saa_flags = 0;
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

/// The LUN field of a request for `target` and `lun`: "The only supported format for the LUN
/// field is: first byte set to 1, second byte set to target, third and fourth byte
/// representing a single level LUN structure, followed by four zero bytes." `None` when
/// the address does not fit.
pub fn vioscsi_lun(target: u16, lun: u16) -> Option<[u8; 8]> {
    if target >= 256 || lun >= 16384 {
        return None;
    }
    Some([
        1,
        target as u8,
        0x40 | (lun >> 8) as u8,
        lun as u8,
        0,
        0,
        0,
        0,
    ])
}

/// `vioscsi_scsi_cmd`: the adapter's `scsi_cmd`.
pub fn vioscsi_scsi_cmd(xs: &'static ScsiXfer) {
    let sc = vioscsi_xs_softc(xs);
    let vsc = sc.virtio();
    let Some(io) = xs.io.get() else {
        panic(format_args!(
            "vioscsi_scsi_cmd: xs {:p} without a request",
            xs
        ));
    };
    // SAFETY: the transfer's opening came from this adapter's pool, whose `io_get`
    // (vioscsi_req_get) hands out slots of `sc_reqs`, mapped while the device exists.
    let vr: &VioscsiReq = unsafe { io.cast::<VioscsiReq>().as_ref() };
    let vq = sc.vq();
    let slot = vr.vr_qe_index.get();

    if VIOSCSI_DEBUG {
        printf(format_args!("vioscsi_scsi_cmd: enter\n"));
    }

    'stuffup: {
        // TODO(matthew): Support bidirectional SCSI commands?
        let flags = xs.flags.get();
        if flags & (SCSI_DATA_IN | SCSI_DATA_OUT) == (SCSI_DATA_IN | SCSI_DATA_OUT) {
            break 'stuffup;
        }

        vr.vr_xs.set(Some(xs));

        let link = xs.link();
        let Some(lun) = vioscsi_lun(link.target.get(), link.lun.get()) else {
            break 'stuffup;
        };
        let mut req = vr.vr_req.get();
        req.lun = lun;

        let cmdlen = xs.cmdlen.get() as usize;
        if cmdlen > req.cdb.len() {
            break 'stuffup;
        }
        req.cdb = [0; 32];
        req.cdb[..cmdlen].copy_from_slice(&xs.cmd.get().as_bytes()[..cmdlen]);
        vr.vr_req.set(req);

        let isread = flags & SCSI_DATA_IN != 0;
        let hasdata = flags & (SCSI_DATA_IN | SCSI_DATA_OUT) != 0;

        let mut nsegs = 2;
        if hasdata {
            let dflags = if isread { BUS_DMA_READ } else { BUS_DMA_WRITE } | BUS_DMA_NOWAIT;
            // SAFETY: `set_data`'s contract: `xs.data()` is valid for `datalen` bytes and
            // reserved for this transfer until it completes, which unloads the map
            // (vioscsi_req_done).
            let loaded = unsafe {
                bus_dmamap_load(
                    vsc.dmat(),
                    vr.data(),
                    xs.data(),
                    xs.datalen().max(0) as usize,
                    None,
                    dflags,
                )
            };
            if loaded.is_err() {
                break 'stuffup;
            }
            nsegs += vr.data().dm_nsegs.get();
        }

        // Adjust reservation to the number needed, or virtio gets upset. Note that it may
        // trim UP if 'xs' is being recycled w/o getting a new reservation!
        let s = splbio();
        virtio_enqueue_trim(vq, slot, nsegs);
        splx(s);

        bus_dmamap_sync(
            vsc.dmat(),
            vr.control(),
            offset_of!(VioscsiReq, vr_req),
            size_of::<VirtioScsiReqHdr>(),
            BUS_DMASYNC_PREWRITE,
        );
        bus_dmamap_sync(
            vsc.dmat(),
            vr.control(),
            offset_of!(VioscsiReq, vr_res),
            size_of::<VirtioScsiResHdr>(),
            BUS_DMASYNC_PREREAD,
        );
        if hasdata {
            bus_dmamap_sync(
                vsc.dmat(),
                vr.data(),
                0,
                xs.datalen().max(0) as usize,
                if isread {
                    BUS_DMASYNC_PREREAD
                } else {
                    BUS_DMASYNC_PREWRITE
                },
            );
        }

        let s = splbio();
        virtio_enqueue_p(
            vq,
            slot,
            vr.control(),
            offset_of!(VioscsiReq, vr_req),
            size_of::<VirtioScsiReqHdr>(),
            true,
        );
        if flags & SCSI_DATA_OUT != 0 {
            virtio_enqueue(vq, slot, vr.data(), true);
        }
        virtio_enqueue_p(
            vq,
            slot,
            vr.control(),
            offset_of!(VioscsiReq, vr_res),
            size_of::<VirtioScsiResHdr>(),
            false,
        );
        if flags & SCSI_DATA_IN != 0 {
            virtio_enqueue(vq, slot, vr.data(), false);
        }

        virtio_enqueue_commit(vsc, vq, slot, true);

        if flags & SCSI_POLL != 0 {
            if VIOSCSI_DEBUG {
                printf(format_args!("vioscsi_scsi_cmd: polling...\n"));
            }
            let mut timeout = 1000;
            loop {
                let _ = virtio_poll_intr(vsc);
                if !vr.is_xs(xs) {
                    break;
                }
                delay(1000);
                timeout -= 1;
                if timeout <= 0 {
                    break;
                }
            }
            if vr.is_xs(xs) {
                // TODO(matthew): Abort the request.
                xs.error.set(XS_TIMEOUT);
                xs.resid.set(xs.datalen().max(0) as usize);
                if VIOSCSI_DEBUG {
                    printf(format_args!("vioscsi_scsi_cmd: polling timeout\n"));
                }
                scsi_done(xs);
            }
            if VIOSCSI_DEBUG {
                printf(format_args!("vioscsi_scsi_cmd: done (timeout={timeout})\n"));
            }
        }
        splx(s);
        return;
    }

    // stuffup:
    xs.error.set(XS_DRIVER_STUFFUP);
    xs.resid.set(xs.datalen().max(0) as usize);
    if VIOSCSI_DEBUG {
        printf(format_args!("vioscsi_scsi_cmd: stuffup\n"));
    }
    scsi_done(xs);
}

/// `vioscsi_req_done`: completes a request the device has used: the response header's
/// status, residual and sense data into the transfer.
pub fn vioscsi_req_done(_sc: &VioscsiSoftc, vsc: &VirtioSoftc, vr: &VioscsiReq) {
    let xs = vr.xs();
    if VIOSCSI_DEBUG {
        printf(format_args!(
            "vioscsi_req_done: enter vr: {:p} xs: {:p}\n",
            vr, xs
        ));
    }

    let flags = xs.flags.get();
    let isread = flags & SCSI_DATA_IN != 0;
    bus_dmamap_sync(
        vsc.dmat(),
        vr.control(),
        offset_of!(VioscsiReq, vr_req),
        size_of::<VirtioScsiReqHdr>(),
        BUS_DMASYNC_POSTWRITE,
    );
    bus_dmamap_sync(
        vsc.dmat(),
        vr.control(),
        offset_of!(VioscsiReq, vr_res),
        size_of::<VirtioScsiResHdr>(),
        BUS_DMASYNC_POSTREAD,
    );
    if flags & (SCSI_DATA_IN | SCSI_DATA_OUT) != 0 {
        bus_dmamap_sync(
            vsc.dmat(),
            vr.data(),
            0,
            xs.datalen().max(0) as usize,
            if isread {
                BUS_DMASYNC_POSTREAD
            } else {
                BUS_DMASYNC_POSTWRITE
            },
        );
        bus_dmamap_unload(vsc.dmat(), vr.data());
    }

    vioscsi_xs_result(xs, &vr.res());

    // done:
    vr.vr_xs.set(None);
    scsi_done(xs);
}

/// The response header's status, residual and sense data into the transfer (the part of
/// `vioscsi_req_done` after the DMA syncs, split out so that it is host-tested).
pub fn vioscsi_xs_result(xs: &ScsiXfer, res: &VirtioScsiResHdr) {
    if res.response != VIRTIO_SCSI_S_OK {
        xs.error.set(XS_DRIVER_STUFFUP);
        xs.resid.set(xs.datalen().max(0) as usize);
        if VIOSCSI_DEBUG {
            let response = res.response;
            printf(format_args!("vioscsi_req_done: stuffup: {response}\n"));
        }
    } else {
        let mut sense = xs.sense.get();
        let sense_len = (res.sense_len as usize).min(size_of_val(&sense));
        let res_sense = res.sense;
        sense.as_bytes_mut()[..sense_len].copy_from_slice(&res_sense[..sense_len]);
        xs.sense.set(sense);
        xs.error
            .set(if sense_len == 0 { XS_NOERROR } else { XS_SENSE });

        xs.status.set(res.status);
        xs.resid.set(res.residual as usize);

        if VIOSCSI_DEBUG {
            let status = res.status;
            printf(format_args!(
                "vioscsi_req_done: done {}, {}, {}\n",
                xs.error.get(),
                status,
                xs.resid.get()
            ));
        }
    }
}

/// `vioscsi_vq_done`: a queue's `vq_done`: completes every request the device has used.
pub fn vioscsi_vq_done(vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let sc = vioscsi_child(vsc);
    let mut ret = 0;

    if VIOSCSI_DEBUG {
        printf(format_args!("vioscsi_vq_done: enter\n"));
    }

    loop {
        let s = splbio();
        let r = virtio_dequeue(vsc, vq);
        splx(s);
        let Ok((slot, _len)) = r else {
            break;
        };

        if VIOSCSI_DEBUG {
            printf(format_args!("vioscsi_vq_done: slot={slot}\n"));
        }
        let qe = vq.entry(slot as usize);
        let vr = sc.req(qe.qe_vr_index.get() as usize);
        vioscsi_req_done(sc, vsc, vr);
        ret = 1;
    }

    if VIOSCSI_DEBUG {
        printf(format_args!("vioscsi_vq_done: exit {ret}\n"));
    }

    ret
}

/// `vioscsi_req_get`: provides the SCSI layer with all the resources necessary to start an
/// I/O on the device.
///
/// Since the size of the I/O is unknown at this time the resources allocated (a.k.a.
/// reserved) must be sufficient to allow the maximum possible I/O size.
///
/// When the I/O is actually attempted via `vioscsi_scsi_cmd()` excess resources will be
/// returned via `virtio_enqueue_trim()`.
///
/// # Safety
///
/// `cookie` is a live [`VioscsiSoftc`] (the one `vioscsi_attach` gave `scsi_iopool_init`).
pub unsafe fn vioscsi_req_get(cookie: *mut c_void) -> Option<ScsiIo> {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*cookie.cast::<VioscsiSoftc>().cast_const() };

    mtx_enter(&sc.sc_vr_mtx);
    let vr = sc.sc_freelist.first().map(NonNull::from);
    if vr.is_some() {
        // SAFETY: the list is not empty.
        unsafe { sc.sc_freelist.remove_head() };
    }
    mtx_leave(&sc.sc_vr_mtx);

    if VIOSCSI_DEBUG {
        printf(format_args!(
            "vioscsi_req_get: {:p}\n",
            vr.map_or(ptr::null_mut(), NonNull::as_ptr)
        ));
    }

    vr.map(NonNull::cast)
}

/// `vioscsi_req_put`: gives a request slot back to the free list.
///
/// # Safety
///
/// `cookie` is a live [`VioscsiSoftc`] and `io` one of its request slots, from
/// [`vioscsi_req_get`] and not on the free list.
pub unsafe fn vioscsi_req_put(cookie: *mut c_void, io: ScsiIo) {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*cookie.cast::<VioscsiSoftc>().cast_const() };
    // SAFETY: the caller's guarantee: a slot in `sc_reqs`, mapped while the device exists.
    let vr = unsafe { io.cast::<VioscsiReq>().as_ref() };

    if VIOSCSI_DEBUG {
        printf(format_args!("vioscsi_req_put: {:p}\n", vr));
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

/// `vioscsi_alloc_reqs`: allocates the request slots (one per `ALLOC_SEGS` descriptors, or
/// one per descriptor with indirect descriptors), reserves each one's descriptors and makes
/// its maps; returns how many it made, 0 on failure.
pub fn vioscsi_alloc_reqs(sc: &VioscsiSoftc, qsize: usize) -> usize {
    let vq = sc.vq();
    let dmat = sc.virtio().dmat();

    let nreqs = if !vq.vq_indirect.get().is_null() {
        qsize
    } else {
        qsize / ALLOC_SEGS
    };

    let allocsize = size_of::<VioscsiReq>() * nreqs;
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
            "bus_dmamem_alloc, size {allocsize}, error {}\n",
            r as i32
        ));
        return 0;
    }
    sc.sc_reqs_segs[0].set(segs[0]);
    let vaddr = match bus_dmamem_map(dmat, &mut segs, allocsize, BUS_DMA_NOWAIT) {
        Ok(vaddr) => vaddr,
        Err(r) => {
            printf(format_args!("bus_dmamem_map failed, error {}\n", r as i32));
            // SAFETY: the segment allocated above, not mapped.
            unsafe { bus_dmamem_free(dmat, &segs) };
            return 0;
        }
    };
    sc.sc_reqs.set(vaddr.as_ptr().cast());
    // SAFETY: `vaddr` maps `allocsize` bytes, just allocated and used by nothing else.
    unsafe { ptr::write_bytes(vaddr.as_ptr(), 0, allocsize) };

    for i in 0..nreqs {
        // Assign descriptors and create the DMA maps for each allocated request.
        // SAFETY: `i` is below `nreqs`, the slots mapped (and zeroed) above.
        let vr = unsafe { vioscsi_req_at(sc.sc_reqs.get(), i) };
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
                let next = vioscsi_desc_next(vq, slot + r);
                if VIOSCSI_DEBUG {
                    printf(format_args!(
                        "vd[{r}].next = {next:?} should be {}\n",
                        slot + r + 1
                    ));
                }
                if next.map(usize::from) != Some(slot + r + 1) {
                    return i;
                }
                r += 1;
            }
            if vioscsi_desc_next(vq, slot + r) != Some(0) {
                return i;
            }
            if VIOSCSI_DEBUG {
                printf(format_args!("Reserved slots are contiguous as required!\n"));
            }
        }

        // Slots and request counts are below the queue size, at most 32768.
        vr.vr_qe_index.set(slot);
        let mut req = vr.vr_req.get();
        req.id = slot as u64;
        req.task_attr = VIRTIO_SCSI_S_SIMPLE;
        vr.vr_req.set(req);
        vq.entry(slot as usize).qe_vr_index.set(i as i16);

        match bus_dmamap_create(
            dmat,
            VR_DMA_END,
            1,
            VR_DMA_END,
            0,
            BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW | BUS_DMA_64BIT,
        ) {
            Ok(map) => vr.vr_control.set(Some(map)),
            Err(r) => {
                printf(format_args!(
                    "bus_dmamap_create vr_control failed, error  {}\n",
                    r as i32
                ));
                return i;
            }
        }
        match bus_dmamap_create(
            dmat,
            MAXPHYS,
            SEG_MAX as i32,
            MAXPHYS,
            0,
            BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW | BUS_DMA_64BIT,
        ) {
            Ok(map) => vr.vr_data.set(Some(map)),
            Err(r) => {
                printf(format_args!(
                    "bus_dmamap_create vr_data failed, error {}\n",
                    r as i32
                ));
                return i;
            }
        }
        // SAFETY: the slot's first `VR_DMA_END` bytes (the two headers), in the DMA memory
        // mapped above, which stays while the device exists.
        if let Err(r) = unsafe {
            bus_dmamap_load(
                dmat,
                vr.control(),
                vr.vr_req.as_ptr().cast(),
                VR_DMA_END,
                None,
                BUS_DMA_NOWAIT,
            )
        } {
            printf(format_args!(
                "bus_dmamap_load vr_control failed, error {}\n",
                r as i32
            ));
            return i;
        }

        // SAFETY: the slot is on no list yet and stays in the DMA memory while the device
        // exists.
        unsafe { sc.sc_freelist.insert_head(vr) };
    }

    nreqs
}

/// `vq->vq_desc[i].next`; `None` when `i` is past the descriptor table (the C reads on;
/// such a block is not contiguous either way).
fn vioscsi_desc_next(vq: &Virtqueue, i: usize) -> Option<u16> {
    let vd = vq.vq_desc.get();
    if vd.is_null() || i >= vq.vq_num.get() as usize {
        return None;
    }
    // SAFETY: `virtio_alloc_vq` allocated `vq_num` descriptors at `vq_desc`, in DMA memory
    // the device may read concurrently: read volatile.
    Some(unsafe { ptr::read_volatile(&raw const (*vd.add(i)).next) })
}

const _: () = {
    assert!(VR_DMA_END == 160);
    assert!(offset_of!(VioscsiReq, vr_res) == size_of::<VirtioScsiReqHdr>());
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of `vioscsi.rs`: the request slot layout and free list, the LUN encoding the
    // device demands and the translation of the response header into the transfer.

    use std::alloc::{Layout, alloc_zeroed};
    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::scsi::scsi_all::ScsiSenseData;
    use crate::scsi::scsiconf::XS_SENSE;

    /// A leaked, zero-filled `T` (all-zero is valid for the softcs and the request slots).
    fn zeroed<T>(n: usize) -> *mut T {
        let layout = Layout::array::<T>(n).unwrap();
        // SAFETY: a non-zero layout.
        let p = unsafe { alloc_zeroed(layout) };
        assert!(!p.is_null());
        p.cast()
    }

    /// A vioscsi softc with `n` request slots on its free list (in `vioscsi_alloc_reqs`' order).
    fn softc(n: usize) -> &'static VioscsiSoftc {
        // SAFETY: leaked zeroed block; all-zero is a valid softc.
        let sc: &'static VioscsiSoftc = unsafe { &*zeroed::<VioscsiSoftc>(1) };
        sc.sc_freelist.init();
        mtx_init(&sc.sc_vr_mtx, IPL_BIO);
        sc.sc_reqs.set(zeroed::<VioscsiReq>(n));
        sc.sc_nreqs.set(n);
        for i in 0..n {
            let vr = sc.req(i);
            vr.vr_qe_index.set((i * ALLOC_SEGS) as i32);
            // SAFETY: a fresh slot, leaked.
            unsafe { sc.sc_freelist.insert_head(vr) };
        }
        sc
    }

    fn cookie(sc: &VioscsiSoftc) -> *mut c_void {
        ptr::from_ref(sc).cast_mut().cast()
    }

    /// The slot index of an opening.
    fn index(sc: &VioscsiSoftc, io: ScsiIo) -> usize {
        (io.as_ptr() as usize - sc.sc_reqs.get() as usize) / size_of::<VioscsiReq>()
    }

    #[test]
    fn the_request_layout_is_the_c_one() {
        // 51 bytes of request header, 108 of response header, then the pointers at 8 bytes.
        assert_eq!(offset_of!(VioscsiReq, vr_req), 0);
        assert_eq!(offset_of!(VioscsiReq, vr_res), 51);
        assert_eq!(VR_DMA_END, 160);
        assert_eq!(SEG_MAX, MAXPHYS / PAGE_SIZE + 1);
        assert_eq!(ALLOC_SEGS, SEG_MAX + 2);
    }

    #[test]
    fn req_get_and_put_cycle_the_free_list() {
        let sc = softc(3);
        let mut got = Vec::new();
        // SAFETY: the cookie is the live softc; every opening goes back once.
        unsafe {
            while let Some(io) = vioscsi_req_get(cookie(sc)) {
                got.push(index(sc, io));
            }
        }
        // SLIST_INSERT_HEAD: the last slot made is handed out first.
        assert_eq!(got, [2, 1, 0]);
        assert!(sc.sc_freelist.is_empty());

        let io = NonNull::from(sc.req(1)).cast();
        // SAFETY: slot 1 was taken above and is on no list.
        unsafe { vioscsi_req_put(cookie(sc), io) };
        // SAFETY: as above.
        let again = unsafe { vioscsi_req_get(cookie(sc)) }.unwrap();
        assert_eq!(index(sc, again), 1);
        // SAFETY: as above.
        assert!(unsafe { vioscsi_req_get(cookie(sc)) }.is_none());
    }

    #[test]
    fn the_lun_is_a_single_level_structure() {
        assert_eq!(vioscsi_lun(0, 0), Some([1, 0, 0x40, 0, 0, 0, 0, 0]));
        assert_eq!(vioscsi_lun(3, 0x0102), Some([1, 3, 0x41, 0x02, 0, 0, 0, 0]));
        assert_eq!(
            vioscsi_lun(255, 16383),
            Some([1, 255, 0x7f, 0xff, 0, 0, 0, 0])
        );
        assert_eq!(vioscsi_lun(256, 0), None);
        assert_eq!(vioscsi_lun(0, 16384), None);
    }

    #[test]
    fn an_ok_response_carries_status_residual_and_sense() {
        let xs = ScsiXfer::new();
        let mut res = VirtioScsiResHdr::new();
        res.status = 2; // CHECK CONDITION
        res.residual = 512;
        res.sense_len = 18;
        res.sense[0] = 0x70;
        res.sense[2] = 0x05;
        res.sense[12] = 0x24;
        vioscsi_xs_result(&xs, &res);
        assert_eq!(xs.error.get(), XS_SENSE);
        assert_eq!(xs.status.get(), 2);
        assert_eq!(xs.resid.get(), 512);
        let sense = xs.sense.get();
        assert_eq!(
            (sense.error_code, sense.flags, sense.add_sense_code),
            (0x70, 0x05, 0x24)
        );

        // No sense data: no error; a longer sense length than the structure is clamped.
        let xs = ScsiXfer::new();
        let mut res = VirtioScsiResHdr::new();
        res.status = 0;
        vioscsi_xs_result(&xs, &res);
        assert_eq!(xs.error.get(), XS_NOERROR);
        assert_eq!(xs.sense.get(), ScsiSenseData::new());
        res.sense_len = 96;
        res.sense = [0xaa; 96];
        vioscsi_xs_result(&xs, &res);
        assert_eq!(xs.error.get(), XS_SENSE);
        assert_eq!(xs.sense.get().sense_key_spec_3, 0xaa);
    }

    #[test]
    fn a_failed_response_is_a_driver_stuffup_with_everything_left() {
        let xs = ScsiXfer::new();
        let mut buf = [0u8; 64];
        // SAFETY: `buf` outlives the transfer, which only has its length read.
        unsafe { xs.set_data(buf.as_mut_ptr(), 64) };
        let mut res = VirtioScsiResHdr::new();
        res.response = VIRTIO_SCSI_S_BUSY_FOR_TEST;
        vioscsi_xs_result(&xs, &res);
        assert_eq!(xs.error.get(), XS_DRIVER_STUFFUP);
        assert_eq!(xs.resid.get(), 64);
    }

    /// `VIRTIO_SCSI_S_BUSY`: any response but OK.
    const VIRTIO_SCSI_S_BUSY_FOR_TEST: u8 = crate::dev::pv::vioscsireg::VIRTIO_SCSI_S_BUSY;
}
/* </TESTS> */
