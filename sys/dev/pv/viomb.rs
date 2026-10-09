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
 * Copyright (c) 2012 Talypov Dinar <dinar@i-nk.ru>
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
//! `viomb(4)`: the virtio memory balloon (`viomb* at virtio?`).
//!
//! Upstream: sys/dev/pv/viomb.c @ 3ce1f3f79392
//!
//! The host sets the balloon's desired size in pages (`num_pages` in the device
//! configuration) and raises a configuration change; the driver's task then compares it with
//! the current size (`actual`) and inflates (takes pages from UVM and hands their frame numbers
//! to the host on the inflate queue) or deflates (hands frames back on the deflate queue and
//! returns the pages to UVM), at most [`PGS_PER_REQ`] pages (1 MB) a request. Each request's
//! interrupt writes the new `actual` and queues the task again while there is work left. Two
//! sensors, `hw.sensors.viomb0.raw0` (desired) and `raw1` (current), give the sizes in bytes as
//! the task last saw them.
//!
//! The C's quirks are kept: the sensors are updated by the task only, so they show `actual`
//! as it was before the last request completed; a deflate that runs out of balloon pages
//! records `i - 1` entries; the requests' lengths are always `nvpages` words; and the frame
//! number arrays, which the device reads, are queued device-writable (`VRING_READ` is 0,
//! `virtio_enqueue_p`'s `write` false, which sets `VRING_DESC_F_WRITE`).
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the device first; its members are `Cell`s, page lists,
//!   a task and sensors, all-zero valid. `bl_pages` is the `dma_alloc`ed array's address,
//!   written volatile (the device reads it by DMA); `bl_dmamap` and `sc_taskq` are `Option`s.
//! - `VIRTIO_PAGE_SIZE != PAGE_SIZE` is a compile-time assertion (the C's `#error`); the
//!   attach's run-time check is kept and never fires.
//! - How many pages one request moves is [`viomb_nvpages`], host-tested; the C computes it
//!   inline in both directions.
//! - `VIOMBDEBUG` is empty unless `VIRTIO_DEBUG`; its messages are compiled behind
//!   `if VIRTIO_DEBUG > 0`, and `viomb_feature_names` is empty unless `VIRTIO_DEBUG`.
//! - The `dequeue failed` message prints the errno's number, as the C's `%d`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::pv::virtio::{
    virtio_alloc_vq, virtio_attach_finish, virtio_dequeue, virtio_dequeue_commit,
    virtio_enqueue_commit, virtio_enqueue_p, virtio_enqueue_prep, virtio_enqueue_reserve,
    virtio_free_vq, virtio_start_vq_intr,
};
use crate::dev::pv::virtioreg::{PCI_PRODUCT_VIRTIO_BALLOON, VIRTIO_PAGE_SIZE};
use crate::dev::pv::virtiovar::{
    VIRTIO_CHILD_ERROR, VIRTIO_DEBUG, VirtioAttachArgs, VirtioFeatureName, VirtioSoftc, Virtqueue,
    virtio_has_feature, virtio_negotiate_features, virtio_read_device_config_4,
    virtio_write_device_config_4,
};
use crate::kassert;
use crate::kern::dma_alloc::{dma_alloc, dma_free};
use crate::kern::kern_sensors::{sensor_attach, sensordev_install};
use crate::kern::kern_task::{task_add, task_set, taskq_create};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::Machine;
use crate::machine::bus::{
    BUS_DMA_NOWAIT, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREWRITE, BusDmamap, bus_dmamap_create,
    bus_dmamap_destroy, bus_dmamap_load, bus_dmamap_sync,
};
use crate::machine::intr::{IPL_BIO, splbio, splx};
use crate::machine::pmap::Pmap;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::param::{PAGE_SHIFT, PAGE_SIZE};
use crate::sys::pool::{PR_NOWAIT, PR_ZERO};
use crate::sys::sensors::{Ksensor, Ksensordev, SENSOR_INTEGER};
use crate::sys::task::{Task, Taskq};
use crate::sys::types::Paddr;
use crate::uvm::uvm_extern::UVM_PLA_NOWAIT;
use crate::uvm::uvm_page::{Pglist, uvm_pglistalloc, uvm_pglistfree};

// Flags used to specify kind of operation, actually should be moved to virtiovar.h.
/// `VRING_READ`: the device reads the buffer.
pub const VRING_READ: bool = false;
/// `VRING_WRITE`: the device writes the buffer.
pub const VRING_WRITE: bool = true;

// Notify or don't notify.
/// `VRING_NO_NOTIFY`.
pub const VRING_NO_NOTIFY: bool = false;
/// `VRING_NOTIFY`.
pub const VRING_NOTIFY: bool = true;

// Configuration registers.
/// `VIRTIO_BALLOON_CONFIG_NUM_PAGES`: 32bit.
pub const VIRTIO_BALLOON_CONFIG_NUM_PAGES: i32 = 0;
/// `VIRTIO_BALLOON_CONFIG_ACTUAL`: 32bit.
pub const VIRTIO_BALLOON_CONFIG_ACTUAL: i32 = 4;

// Feature bits.
/// `VIRTIO_BALLOON_F_MUST_TELL_HOST`.
pub const VIRTIO_BALLOON_F_MUST_TELL_HOST: u64 = 1 << 0;
/// `VIRTIO_BALLOON_F_STATS_VQ`.
pub const VIRTIO_BALLOON_F_STATS_VQ: u64 = 1 << 1;

/// `viomb_feature_names[]` with `VIRTIO_DEBUG`.
static VIOMB_FEATURE_NAMES_DEBUG: [VirtioFeatureName; 2] = [
    VirtioFeatureName {
        bit: VIRTIO_BALLOON_F_MUST_TELL_HOST,
        name: "TellHost",
    },
    VirtioFeatureName {
        bit: VIRTIO_BALLOON_F_STATS_VQ,
        name: "StatVQ",
    },
];

/// `PGS_PER_REQ`: 1MB, 4KB/page.
pub const PGS_PER_REQ: u32 = 256;
/// `VQ_INFLATE`.
pub const VQ_INFLATE: usize = 0;
/// `VQ_DEFLATE`.
pub const VQ_DEFLATE: usize = 1;

/// The bytes of a request's frame number array.
const BL_PAGES_SIZE: usize = size_of::<u32>() * PGS_PER_REQ as usize;

/// `struct balloon_req`: the one request in flight.
pub struct BalloonReq {
    /// `bl_dmamap`.
    pub bl_dmamap: Cell<Option<&'static BusDmamap>>,
    /// `bl_pglist`: the pages of the request.
    pub bl_pglist: Pglist,
    /// `bl_nentries`.
    pub bl_nentries: Cell<i32>,
    /// `bl_pages`: [`PGS_PER_REQ`] page frame numbers, from `dma_alloc`.
    pub bl_pages: Cell<*mut u32>,
}

impl BalloonReq {
    /// `b->bl_dmamap`, which a running device has.
    fn dmamap(&self) -> &'static BusDmamap {
        match self.bl_dmamap.get() {
            Some(map) => map,
            None => panic(format_args!("viomb: no dmamap")),
        }
    }

    /// `b->bl_pages[i] = v`.
    fn set_page(&self, i: usize, v: u32) {
        let pages = self.bl_pages.get();
        if pages.is_null() || i >= PGS_PER_REQ as usize {
            panic(format_args!("viomb: bad page entry {i}"));
        }
        // SAFETY: `bl_pages` is the live `dma_alloc` of `PGS_PER_REQ` words and `i` is below
        // it; the device reads it by DMA, hence volatile.
        unsafe { ptr::write_volatile(pages.add(i), v) }
    }
}

/// `struct viomb_softc`.
#[repr(C)]
pub struct ViombSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_virtio`: the parent.
    pub sc_virtio: Cell<Option<&'static VirtioSoftc>>,
    /// `sc_vq`: the inflate and deflate queues.
    pub sc_vq: [Virtqueue; 2],
    /// `sc_npages`: desired pages.
    pub sc_npages: Cell<u32>,
    /// `sc_actual`: current pages.
    pub sc_actual: Cell<u32>,
    /// `sc_req`.
    pub sc_req: BalloonReq,
    /// `sc_taskq`.
    pub sc_taskq: Cell<Option<&'static Taskq>>,
    /// `sc_task`: `viomb_worker`.
    pub sc_task: Task,
    /// `sc_balloon_pages`: the pages in the balloon.
    pub sc_balloon_pages: Pglist,
    /// `sc_sens`: desired and current size, in bytes.
    pub sc_sens: [Ksensor; 2],
    /// `sc_sensdev`.
    pub sc_sensdev: Ksensordev,
}

impl ViombSoftc {
    /// `sc->sc_virtio`, set first by the attach.
    fn virtio(&self) -> &'static VirtioSoftc {
        match self.sc_virtio.get() {
            Some(vsc) => vsc,
            None => panic(format_args!("viomb: no virtio softc")),
        }
    }

    /// `sc->sc_taskq`, which a running device has.
    fn taskq(&self) -> &'static Taskq {
        match self.sc_taskq.get() {
            Some(tq) => tq,
            None => panic(format_args!("viomb: no taskq")),
        }
    }

    /// `DEVNAME(sc)`.
    fn devname(&self) -> &str {
        self.sc_dev.xname()
    }
}

// SAFETY: `#[repr(C)]` with the device first; the virtqueues, the page lists, the task and the
// sensors are all-zero valid (`dev/pv/virtiovar.rs`, `sys/queue.rs`, `sys/task.rs`:
// `Task::zeroed`, `sys/sensors.rs`: `Ksensor::new`, `Ksensordev::new`), and every other member
// is a `Cell` of an integer, a raw pointer or an `Option` of a reference.
unsafe impl Softc for ViombSoftc {}

/// `viomb_ca`.
pub static VIOMB_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<ViombSoftc>(),
    ca_match: Some(viomb_match),
    ca_attach: viomb_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `viomb_cd`.
pub static VIOMB_CD: Cfdriver = Cfdriver::new(b"viomb", DV_DULL, 0);

/// `viomb_feature_names`: the table `virtio_negotiate_features` is given, empty unless
/// `VIRTIO_DEBUG`.
fn viomb_feature_names() -> &'static [VirtioFeatureName] {
    if VIRTIO_DEBUG > 0 {
        &VIOMB_FEATURE_NAMES_DEBUG
    } else {
        &[]
    }
}

/// `VIOMBDEBUG(sc, ...)`.
fn viombdebug(sc: &ViombSoftc, args: core::fmt::Arguments<'_>) {
    if VIRTIO_DEBUG > 0 {
        printf(format_args!("{}: {}", sc.devname(), args));
    }
}

/// `(struct viomb_softc *)vsc->sc_child`.
fn viomb_child(vsc: &VirtioSoftc) -> &'static ViombSoftc {
    let c = vsc.sc_child.get();
    if c.is_null() || c == VIRTIO_CHILD_ERROR {
        panic(format_args!("viomb: virtio without its child"));
    }
    // SAFETY: viomb_attach stored its own device, the head of a `ViombSoftc` that lives as long
    // as the kernel; only viomb's handlers call this.
    unsafe { &*c.cast::<ViombSoftc>() }
}

/// `viomb_match`: the virtio memory balloon.
pub fn viomb_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: virtio transports attach their children with arguments that begin with a
    // `virtio_attach_args`.
    let va = unsafe { &*aux.cast::<VirtioAttachArgs>() };
    i32::from(va.va_devid == PCI_PRODUCT_VIRTIO_BALLOON)
}

/// `viomb_attach`.
pub fn viomb_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `viomb_ca`, whose softc is a `ViombSoftc`; softcs are never
    // freed while the device exists, so the softc may be borrowed for 'static.
    let sc: &'static ViombSoftc = unsafe { &*ptr::from_ref(self_.softc::<ViombSoftc>()) };
    let Some(parent) = parent else {
        return;
    };
    // SAFETY: `viomb* at virtio?`: the parent is a virtio transport, whose softc begins with
    // the `struct virtio_softc`; it outlives its child.
    let vsc: &'static VirtioSoftc = unsafe { &*ptr::from_ref(parent.softc::<VirtioSoftc>()) };
    let va = aux.cast::<VirtioAttachArgs>();

    if !vsc.sc_child.get().is_null() {
        printf(format_args!(
            "child already attached for {}; something wrong...\n",
            parent.xname()
        ));
        return;
    }

    // fail on non-4K page size archs
    if VIRTIO_PAGE_SIZE != PAGE_SIZE {
        printf(format_args!(
            "non-4K page size arch found, needs {VIRTIO_PAGE_SIZE}, got {PAGE_SIZE}\n"
        ));
        return;
    }

    sc.sc_virtio.set(Some(vsc));
    vsc.sc_vqs.set(sc.sc_vq.as_ptr().cast_mut());
    vsc.sc_nvqs.set(0);
    vsc.sc_child.set(ptr::from_ref(self_));
    vsc.sc_ipl.set(IPL_BIO);
    vsc.sc_config_change.set(Some(viomb_config_change));

    vsc.sc_driver_features.set(VIRTIO_BALLOON_F_MUST_TELL_HOST);

    // `goto err` is `Err(false)`, `goto err_dmamap` (the map made, to destroy) `Err(true)`.
    let attached: Result<(), bool> = 'err: {
        if virtio_negotiate_features(vsc, Some(viomb_feature_names())).is_err() {
            break 'err Err(false);
        }

        if virtio_alloc_vq(vsc, &sc.sc_vq[VQ_INFLATE], VQ_INFLATE as i32, 1, "inflate").is_err() {
            break 'err Err(false);
        }
        vsc.sc_nvqs.set(vsc.sc_nvqs.get() + 1);
        if virtio_alloc_vq(vsc, &sc.sc_vq[VQ_DEFLATE], VQ_DEFLATE as i32, 1, "deflate").is_err() {
            break 'err Err(false);
        }
        vsc.sc_nvqs.set(vsc.sc_nvqs.get() + 1);

        sc.sc_vq[VQ_INFLATE].vq_done.set(Some(viomb_inflate_intr));
        sc.sc_vq[VQ_DEFLATE].vq_done.set(Some(viomb_deflate_intr));
        virtio_start_vq_intr(vsc, &sc.sc_vq[VQ_INFLATE]);
        virtio_start_vq_intr(vsc, &sc.sc_vq[VQ_DEFLATE]);

        viomb_read_config(sc);
        sc.sc_balloon_pages.init();

        let Some(pages) = dma_alloc(BL_PAGES_SIZE, PR_NOWAIT | PR_ZERO) else {
            printf(format_args!("{}: Can't alloc DMA memory.\n", sc.devname()));
            break 'err Err(false);
        };
        sc.sc_req.bl_pages.set(pages.as_ptr().cast());
        let map = match bus_dmamap_create(
            vsc.dmat(),
            BL_PAGES_SIZE,
            1,
            BL_PAGES_SIZE,
            0,
            BUS_DMA_NOWAIT,
        ) {
            Ok(map) => map,
            Err(_) => {
                printf(format_args!("{}: dmamap creation failed.\n", sc.devname()));
                break 'err Err(false);
            }
        };
        sc.sc_req.bl_dmamap.set(Some(map));
        // SAFETY: `bl_pages` is a live kernel allocation of `BL_PAGES_SIZE` bytes, freed only
        // after the map is destroyed (the error path below).
        if unsafe {
            bus_dmamap_load(
                vsc.dmat(),
                map,
                sc.sc_req.bl_pages.get().cast(),
                BL_PAGES_SIZE,
                None,
                BUS_DMA_NOWAIT,
            )
        }
        .is_err()
        {
            printf(format_args!("{}: dmamap load failed.\n", sc.devname()));
            break 'err Err(true);
        }

        let Some(tq) = taskq_create(b"viomb", 1, IPL_BIO, 0) else {
            break 'err Err(true);
        };
        sc.sc_taskq.set(Some(tq));
        task_set(
            &sc.sc_task,
            viomb_worker,
            ptr::from_ref(sc).cast_mut().cast(),
        );

        let mut xname = [0u8; 16];
        libkern::strlcpy(&mut xname, sc.devname().as_bytes());
        sc.sc_sensdev.xname.set(xname);
        let mut desc = [0u8; 32];
        libkern::strlcpy(&mut desc, b"desired");
        sc.sc_sens[0].desc.set(desc);
        sc.sc_sens[0].r#type.set(SENSOR_INTEGER);
        sensor_attach(&sc.sc_sensdev, &sc.sc_sens[0]);
        sc.sc_sens[0]
            .value
            .set(i64::from(sc.sc_npages.get() << PAGE_SHIFT));

        let mut desc = [0u8; 32];
        libkern::strlcpy(&mut desc, b"current");
        sc.sc_sens[1].desc.set(desc);
        sc.sc_sens[1].r#type.set(SENSOR_INTEGER);
        sensor_attach(&sc.sc_sensdev, &sc.sc_sens[1]);
        sc.sc_sens[1]
            .value
            .set(i64::from(sc.sc_actual.get() << PAGE_SHIFT));

        sensordev_install(&sc.sc_sensdev);

        printf(format_args!("\n"));
        if virtio_attach_finish(vsc, va).is_err() {
            break 'err Err(true);
        }
        Ok(())
    };

    let Err(destroy) = attached else {
        return;
    };
    if destroy {
        // err_dmamap:
        if let Some(map) = sc.sc_req.bl_dmamap.take() {
            // SAFETY: the map came from `bus_dmamap_create` above and nothing uses it any
            // more (the child is marked failed below, so no request is ever queued).
            unsafe { bus_dmamap_destroy(vsc.dmat(), NonNull::from(map)) };
        }
    }
    // err:
    if let Some(pages) = NonNull::new(sc.sc_req.bl_pages.get()) {
        dma_free(pages.cast(), BL_PAGES_SIZE);
    }
    for vq in vsc.vqs() {
        let _ = virtio_free_vq(vsc, vq);
    }
    vsc.sc_nvqs.set(0);
    vsc.sc_child.set(VIRTIO_CHILD_ERROR);
}

/// `viomb_config_change`: the host changed the desired size.
pub fn viomb_config_change(vsc: &VirtioSoftc) -> i32 {
    let sc = viomb_child(vsc);

    task_add(sc.taskq(), &sc.sc_task);

    1
}

/// `viomb_worker`: moves the balloon one request towards the desired size and updates the
/// sensors.
pub fn viomb_worker(arg1: *mut c_void) {
    // SAFETY: `viomb_attach` set the task with its softc, which is never freed.
    let sc = unsafe { &*arg1.cast_const().cast::<ViombSoftc>() };

    let s = splbio();
    viomb_read_config(sc);
    if sc.sc_npages.get() > sc.sc_actual.get() {
        viombdebug(
            sc,
            format_args!(
                "inflating balloon from {} to {}.\n",
                sc.sc_actual.get(),
                sc.sc_npages.get()
            ),
        );
        viomb_inflate(sc);
    } else if sc.sc_npages.get() < sc.sc_actual.get() {
        viombdebug(
            sc,
            format_args!(
                "deflating balloon from {} to {}.\n",
                sc.sc_actual.get(),
                sc.sc_npages.get()
            ),
        );
        viomb_deflate(sc);
    }

    sc.sc_sens[0]
        .value
        .set(i64::from(sc.sc_npages.get() << PAGE_SHIFT));
    sc.sc_sens[1]
        .value
        .set(i64::from(sc.sc_actual.get() << PAGE_SHIFT));

    splx(s);
}

/// The pages one request moves from `from` towards `to` (`to` past `from`): the difference,
/// at most [`PGS_PER_REQ`].
pub const fn viomb_nvpages(to: u32, from: u32) -> u32 {
    let n = to - from;
    if n > PGS_PER_REQ { PGS_PER_REQ } else { n }
}

/// `viomb_inflate`: takes up to [`PGS_PER_REQ`] pages from UVM and gives their frame numbers
/// to the host.
pub fn viomb_inflate(sc: &ViombSoftc) {
    let vsc = sc.virtio();
    let vq = &sc.sc_vq[VQ_INFLATE];
    let b = &sc.sc_req;

    let nvpages = viomb_nvpages(sc.sc_npages.get(), sc.sc_actual.get());

    if let Err(error) = uvm_pglistalloc(
        nvpages as usize * PAGE_SIZE,
        Paddr::new(0),
        <Machine as Pmap>::DMA_CONSTRAINT.ucr_high,
        Paddr::new(0),
        Paddr::new(0),
        &b.bl_pglist,
        nvpages as i32,
        UVM_PLA_NOWAIT,
    ) {
        printf(format_args!(
            "{} unable to allocate {} physmem pages,error {}\n",
            sc.devname(),
            nvpages,
            error as i32
        ));
        return;
    }

    b.bl_nentries.set(nvpages as i32);
    let mut i = 0usize;
    for p in b.bl_pglist.iter() {
        b.set_page(i, (p.phys_addr.as_usize() / VIRTIO_PAGE_SIZE) as u32);
        i += 1;
    }

    kassert!(i == nvpages as usize);

    'err: {
        let Ok(slot) = virtio_enqueue_prep(vq) else {
            printf(format_args!(
                "{}:virtio_enqueue_prep() vq_num {}\n",
                sc.devname(),
                vq.vq_num.get()
            ));
            break 'err;
        };
        if virtio_enqueue_reserve(vq, slot, 1).is_err() {
            printf(format_args!(
                "{}:virtio_enqueue_reserve vq_num {}\n",
                sc.devname(),
                vq.vq_num.get()
            ));
            break 'err;
        }
        bus_dmamap_sync(
            vsc.dmat(),
            b.dmamap(),
            0,
            size_of::<u32>() * nvpages as usize,
            BUS_DMASYNC_PREWRITE,
        );
        virtio_enqueue_p(
            vq,
            slot,
            b.dmamap(),
            0,
            size_of::<u32>() * nvpages as usize,
            VRING_READ,
        );
        virtio_enqueue_commit(vsc, vq, slot, VRING_NOTIFY);
        return;
    }
    // err:
    uvm_pglistfree(&b.bl_pglist);
}

/// `viomb_deflate`: takes up to [`PGS_PER_REQ`] pages out of the balloon and gives their frame
/// numbers back to the host.
pub fn viomb_deflate(sc: &ViombSoftc) {
    let vsc = sc.virtio();
    let vq = &sc.sc_vq[VQ_DEFLATE];
    let b = &sc.sc_req;

    let nvpages = u64::from(viomb_nvpages(sc.sc_actual.get(), sc.sc_npages.get()));
    b.bl_nentries.set(nvpages as i32);

    b.bl_pglist.init();
    for i in 0..nvpages as usize {
        let Some(p) = sc.sc_balloon_pages.first() else {
            b.bl_nentries.set(i as i32 - 1);
            break;
        };
        // SAFETY: `p` is the first page of the balloon list; it moves to the request's list,
        // and both heads are in the softc, which never moves.
        unsafe {
            sc.sc_balloon_pages.remove(p);
            b.bl_pglist.insert_tail(p);
        }
        b.set_page(i, (p.phys_addr.as_usize() / VIRTIO_PAGE_SIZE) as u32);
    }

    'err: {
        let Ok(slot) = virtio_enqueue_prep(vq) else {
            printf(format_args!(
                "{}:virtio_get_slot(def) vq_num {}\n",
                sc.devname(),
                vq.vq_num.get()
            ));
            break 'err;
        };
        if virtio_enqueue_reserve(vq, slot, 1).is_err() {
            printf(format_args!(
                "{}:virtio_enqueue_reserve() vq_num {}\n",
                sc.devname(),
                vq.vq_num.get()
            ));
            break 'err;
        }
        bus_dmamap_sync(
            vsc.dmat(),
            b.dmamap(),
            0,
            size_of::<u32>() * nvpages as usize,
            BUS_DMASYNC_PREWRITE,
        );
        virtio_enqueue_p(
            vq,
            slot,
            b.dmamap(),
            0,
            size_of::<u32>() * nvpages as usize,
            VRING_READ,
        );

        if !virtio_has_feature(vsc, VIRTIO_BALLOON_F_MUST_TELL_HOST) {
            uvm_pglistfree(&b.bl_pglist);
        }
        virtio_enqueue_commit(vsc, vq, slot, VRING_NOTIFY);
        return;
    }
    // err:
    // SAFETY: both heads are in the softc, which never moves.
    unsafe { sc.sc_balloon_pages.concat(&b.bl_pglist) };
}

/// `viomb_read_config`: the desired and current sizes, from the device.
pub fn viomb_read_config(sc: &ViombSoftc) {
    let vsc = sc.virtio();

    // these values are explicitly specified as little-endian
    let reg = virtio_read_device_config_4(vsc, VIRTIO_BALLOON_CONFIG_NUM_PAGES);
    sc.sc_npages.set(u32::from_le(reg));
    let reg = virtio_read_device_config_4(vsc, VIRTIO_BALLOON_CONFIG_ACTUAL);
    sc.sc_actual.set(u32::from_le(reg));
    viombdebug(
        sc,
        format_args!(
            "sc->sc_npages {}, sc->sc_actual {}\n",
            sc.sc_npages.get(),
            sc.sc_actual.get()
        ),
    );
}

/// `viomb_vq_dequeue`: takes the finished request off `vq`.
pub fn viomb_vq_dequeue(vq: &Virtqueue) -> Result<(), crate::sys::errno::Errno> {
    let vsc = vq.owner();
    let sc = viomb_child(vsc);

    let slot = match virtio_dequeue(vsc, vq) {
        Ok((slot, _len)) => slot,
        Err(r) => {
            printf(format_args!(
                "{}: dequeue failed, errno {}\n",
                sc.devname(),
                r as i32
            ));
            return Err(r);
        }
    };
    virtio_dequeue_commit(vq, slot);
    Ok(())
}

/// `viomb_inflate_intr`: the host took the pages; they join the balloon.
pub fn viomb_inflate_intr(vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let sc = viomb_child(vsc);

    if viomb_vq_dequeue(vq).is_err() {
        return 1;
    }

    let b = &sc.sc_req;
    let nvpages = b.bl_nentries.get() as u64;
    bus_dmamap_sync(
        vsc.dmat(),
        b.dmamap(),
        0,
        size_of::<u32>() * nvpages as usize,
        BUS_DMASYNC_POSTWRITE,
    );
    // SAFETY: both heads are in the softc, which never moves.
    unsafe { sc.sc_balloon_pages.concat(&b.bl_pglist) };
    viombdebug(
        sc,
        format_args!(
            "updating sc->sc_actual from {} to {}\n",
            sc.sc_actual.get(),
            u64::from(sc.sc_actual.get()) + nvpages
        ),
    );
    virtio_write_device_config_4(
        vsc,
        VIRTIO_BALLOON_CONFIG_ACTUAL,
        (u64::from(sc.sc_actual.get()) + nvpages) as u32,
    );
    viomb_read_config(sc);

    // if we have more work to do, add it to the task list
    if sc.sc_npages.get() > sc.sc_actual.get() {
        task_add(sc.taskq(), &sc.sc_task);
    }

    1
}

/// `viomb_deflate_intr`: the host gave the pages back.
pub fn viomb_deflate_intr(vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let sc = viomb_child(vsc);

    if viomb_vq_dequeue(vq).is_err() {
        return 1;
    }

    let b = &sc.sc_req;
    let nvpages = b.bl_nentries.get() as u64;
    bus_dmamap_sync(
        vsc.dmat(),
        b.dmamap(),
        0,
        size_of::<u32>() * nvpages as usize,
        BUS_DMASYNC_POSTWRITE,
    );

    if virtio_has_feature(vsc, VIRTIO_BALLOON_F_MUST_TELL_HOST) {
        uvm_pglistfree(&b.bl_pglist);
    }

    viombdebug(
        sc,
        format_args!(
            "updating sc->sc_actual from {} to {}\n",
            sc.sc_actual.get(),
            u64::from(sc.sc_actual.get()).wrapping_sub(nvpages)
        ),
    );
    virtio_write_device_config_4(
        vsc,
        VIRTIO_BALLOON_CONFIG_ACTUAL,
        u64::from(sc.sc_actual.get()).wrapping_sub(nvpages) as u32,
    );
    viomb_read_config(sc);

    // if we have more work to do, add it to tasks list
    if sc.sc_npages.get() < sc.sc_actual.get() {
        task_add(sc.taskq(), &sc.sc_task);
    }

    1
}

// The C's `#if VIRTIO_PAGE_SIZE!=PAGE_SIZE #error non-4K page sizes are not supported yet`.
const _: () = assert!(VIRTIO_PAGE_SIZE == PAGE_SIZE);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of viomb(4): how many pages a request moves, and the sensors' byte values.

    use std::assert_eq;

    use super::*;

    #[test]
    fn a_request_moves_at_most_a_megabyte() {
        assert_eq!(viomb_nvpages(65536, 0), PGS_PER_REQ);
        assert_eq!(viomb_nvpages(65536, 65280), 256);
        assert_eq!(viomb_nvpages(65536, 65400), 136);
        assert_eq!(viomb_nvpages(1, 0), 1);
        assert_eq!(viomb_nvpages(7, 7), 0);
    }

    #[test]
    fn sensor_values_are_bytes_of_32_bit_page_counts() {
        // OpenBSD 8.0 on QEMU, `balloon 768` of 1024 MB: 65536 pages desired, the task last
        // saw 65280 (the last request's interrupt raises `actual` without updating the sensor).
        assert_eq!(i64::from(65536u32 << PAGE_SHIFT), 268_435_456);
        assert_eq!(i64::from(65280u32 << PAGE_SHIFT), 267_386_880);
        assert_eq!(i64::from(256u32 << PAGE_SHIFT), 1_048_576);
        // As in C, the shift is done on the 32-bit count.
        assert_eq!(i64::from(0x0010_0000u32 << PAGE_SHIFT), 0);
    }
}
/* </TESTS> */
