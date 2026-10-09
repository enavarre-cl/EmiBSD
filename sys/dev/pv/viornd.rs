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
 * Copyright (c) 2014 Stefan Fritsch <sf@sfritsch.de>
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
//! `viornd(4)`: the virtio entropy device (`viornd* at virtio?`), a source for `rnd(4)`.
//!
//! Upstream: sys/dev/pv/viornd.c @ 3ce1f3f79392
//!
//! The host may not have an unlimited supply of entropy, so the driver does not take all it
//! can: it asks for [`VIORND_BUFSIZE`] bytes once at boot (a timeout of one tick after the
//! attach) and again every `15 * (1 << interval_shift)` seconds after each answer. The shift
//! comes from the second byte of the config flags (the lowest byte is the transport's), 5
//! when it is 0; [`VIORND_ONESHOT`] asks only once. Each answer's 32-bit words go to
//! `enqueue_randomness`.
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the device first; its members are `Cell`s and the
//!   virtqueue and timeout are all-zero valid. `sc_buf` is the `dma_alloc`ed buffer's address
//!   (`*mut i32`), its words read volatile (the device writes them by DMA); `sc_dmamap` is an
//!   `Option` of the map.
//! - The interval and the word count are the functions [`viornd_interval`] and
//!   [`viornd_nwords`], host-tested; the C computes them inline.
//! - `VIORND_DEBUG` is the constant 0 as in C; its messages are compiled behind
//!   `if VIORND_DEBUG > 0`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::pv::virtio::{
    virtio_alloc_vq, virtio_attach_finish, virtio_dequeue, virtio_dequeue_commit, virtio_enqueue,
    virtio_enqueue_commit, virtio_enqueue_prep, virtio_enqueue_reserve, virtio_start_vq_intr,
};
use crate::dev::pv::virtioreg::PCI_PRODUCT_VIRTIO_ENTROPY;
use crate::dev::pv::virtiovar::{
    VIRTIO_CHILD_ERROR, VirtioAttachArgs, VirtioSoftc, Virtqueue, virtio_negotiate_features,
};
use crate::dev::rnd::enqueue_randomness;
use crate::kern::dma_alloc::{dma_alloc, dma_free};
use crate::kern::kern_timeout::{timeout_add, timeout_add_sec, timeout_set};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_ALLOCNOW, BUS_DMA_NOWAIT, BUS_DMA_READ, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_PREREAD,
    BusDmamap, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load, bus_dmamap_sync,
};
use crate::machine::intr::IPL_NET;
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::pool::{PR_NOWAIT, PR_ZERO};
use crate::sys::timeout::Timeout;

/// `VIORND_INTERVAL_SHIFT_DEFAULT`.
pub const VIORND_INTERVAL_SHIFT_DEFAULT: u32 = 5;
/// `VIORND_ONESHOT`: ask for entropy at boot only.
pub const VIORND_ONESHOT: i32 = 0x1000;
/// `VIORND_BUFSIZE`: the bytes asked for each time.
pub const VIORND_BUFSIZE: usize = 16;

/// `VIORND_DEBUG`.
const VIORND_DEBUG: i32 = 0;

/// `VIORND_INTERVAL_SHIFT(f)`: the interval's shift, from the flags' second byte.
#[inline]
pub const fn viornd_interval_shift(f: i32) -> u32 {
    ((f >> 8) & 0xf) as u32
}

/// `struct viornd_softc`.
#[repr(C)]
pub struct ViorndSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_virtio`: the parent.
    pub sc_virtio: Cell<Option<&'static VirtioSoftc>>,

    /// `sc_vq`.
    pub sc_vq: [Virtqueue; 1],
    /// `sc_buf`: [`VIORND_BUFSIZE`] bytes from `dma_alloc`, which the device fills.
    pub sc_buf: Cell<*mut i32>,
    /// `sc_dmamap`.
    pub sc_dmamap: Cell<Option<&'static BusDmamap>>,

    /// `sc_interval`: seconds between requests, 0 for one request only.
    pub sc_interval: Cell<u32>,
    /// `sc_tick`: the next request.
    pub sc_tick: Timeout,
}

impl ViorndSoftc {
    /// `sc->sc_virtio`, set first by the attach.
    fn virtio(&self) -> &'static VirtioSoftc {
        match self.sc_virtio.get() {
            Some(vsc) => vsc,
            None => panic(format_args!("viornd: no virtio softc")),
        }
    }

    /// `sc->sc_dmamap`, which a running device has.
    fn dmamap(&self) -> &'static BusDmamap {
        match self.sc_dmamap.get() {
            Some(map) => map,
            None => panic(format_args!("viornd: no dmamap")),
        }
    }

    /// `sc->sc_buf[i]`, as the device wrote it.
    fn buf_word(&self, i: usize) -> i32 {
        let buf = self.sc_buf.get();
        if buf.is_null() || i >= VIORND_BUFSIZE / size_of::<i32>() {
            panic(format_args!("viornd: bad buffer word {i}"));
        }
        // SAFETY: `sc_buf` is the live `dma_alloc` of `VIORND_BUFSIZE` bytes (pool items are
        // aligned beyond 4), `i` is inside it; the device writes it by DMA, hence volatile.
        unsafe { ptr::read_volatile(buf.add(i)) }
    }
}

// SAFETY: `#[repr(C)]` with the device first; the virtqueue and the timeout are all-zero valid
// (`dev/pv/virtiovar.rs`, `sys/timeout.rs`: `Timeout::zeroed`), and every other member is a
// `Cell` of an integer, a raw pointer or an `Option` of a reference.
unsafe impl Softc for ViorndSoftc {}

/// `viornd_ca`.
pub static VIORND_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<ViorndSoftc>(),
    ca_match: Some(viornd_match),
    ca_attach: viornd_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `viornd_cd`.
pub static VIORND_CD: Cfdriver = Cfdriver::new(b"viornd", DV_DULL, CD_COCOVM);

/// `viornd_match`: the virtio entropy device.
pub fn viornd_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: virtio transports attach their children with arguments that begin with a
    // `virtio_attach_args`.
    let va = unsafe { &*aux.cast::<VirtioAttachArgs>() };
    i32::from(va.va_devid == PCI_PRODUCT_VIRTIO_ENTROPY)
}

/// The seconds between two requests for config flags `flags`: 0 with [`VIORND_ONESHOT`],
/// else `15 * (1 << shift)`, the shift [`VIORND_INTERVAL_SHIFT_DEFAULT`] when the flags give
/// none.
pub const fn viornd_interval(flags: i32) -> u32 {
    if flags & VIORND_ONESHOT != 0 {
        0
    } else {
        let mut shift = viornd_interval_shift(flags);
        if shift == 0 {
            shift = VIORND_INTERVAL_SHIFT_DEFAULT;
        }
        15 * (1 << shift)
    }
}

/// `viornd_attach`.
pub fn viornd_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `viornd_ca`, whose softc is a `ViorndSoftc`; softcs are
    // never freed while the device exists, so the softc may be borrowed for 'static.
    let sc: &'static ViorndSoftc = unsafe { &*ptr::from_ref(self_.softc::<ViorndSoftc>()) };
    let Some(parent) = parent else {
        return;
    };
    // SAFETY: `viornd* at virtio?`: the parent is a virtio transport, whose softc begins with
    // the `struct virtio_softc`; it outlives its child.
    let vsc: &'static VirtioSoftc = unsafe { &*ptr::from_ref(parent.softc::<VirtioSoftc>()) };
    let va = aux.cast::<VirtioAttachArgs>();

    vsc.sc_vqs.set(sc.sc_vq.as_ptr().cast_mut());
    vsc.sc_nvqs.set(1);
    if !vsc.sc_child.get().is_null() {
        panic(format_args!("already attached to something else"));
    }
    vsc.sc_child.set(ptr::from_ref(self_));
    vsc.sc_ipl.set(IPL_NET);
    sc.sc_virtio.set(Some(vsc));

    // `goto err` is `Err(false)`, `goto err2` (the map made, to destroy) is `Err(true)`.
    let attached: Result<(), bool> = 'err: {
        if virtio_negotiate_features(vsc, None).is_err() {
            break 'err Err(false);
        }

        sc.sc_interval
            .set(viornd_interval(sc.sc_dev.cfdata().cf_flags));
        if VIORND_DEBUG > 0 {
            printf(format_args!(
                ": request interval: {}s\n",
                sc.sc_interval.get()
            ));
        }

        let Some(buf) = dma_alloc(VIORND_BUFSIZE, PR_NOWAIT | PR_ZERO) else {
            printf(format_args!(": Can't alloc dma buffer\n"));
            break 'err Err(false);
        };
        sc.sc_buf.set(buf.as_ptr().cast());
        let map = match bus_dmamap_create(
            vsc.dmat(),
            VIORND_BUFSIZE,
            1,
            VIORND_BUFSIZE,
            0,
            BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW,
        ) {
            Ok(map) => map,
            Err(_) => {
                printf(format_args!(": Can't alloc dmamap\n"));
                break 'err Err(false);
            }
        };
        sc.sc_dmamap.set(Some(map));
        // SAFETY: `sc_buf` is a live kernel allocation of `VIORND_BUFSIZE` bytes, freed only
        // after the map is destroyed (the error path below).
        if unsafe {
            bus_dmamap_load(
                vsc.dmat(),
                map,
                sc.sc_buf.get().cast(),
                VIORND_BUFSIZE,
                None,
                BUS_DMA_NOWAIT | BUS_DMA_READ,
            )
        }
        .is_err()
        {
            printf(format_args!(": Can't load dmamap\n"));
            break 'err Err(true);
        }

        if virtio_alloc_vq(vsc, &sc.sc_vq[0], 0, 1, "Entropy request").is_err() {
            printf(format_args!(": Can't alloc virtqueue\n"));
            break 'err Err(true);
        }

        sc.sc_vq[0].vq_done.set(Some(viornd_vq_done));
        virtio_start_vq_intr(vsc, &sc.sc_vq[0]);
        timeout_set(
            &sc.sc_tick,
            viornd_tick,
            ptr::from_ref(sc).cast_mut().cast(),
        );
        timeout_add(&sc.sc_tick, 1);

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
        // err2:
        if let Some(map) = sc.sc_dmamap.take() {
            // SAFETY: the map came from `bus_dmamap_create` above and nothing uses it any
            // more (the queue handler is never called: the child is marked failed below).
            unsafe { bus_dmamap_destroy(vsc.dmat(), NonNull::from(map)) };
        }
    }
    // err:
    vsc.sc_child.set(VIRTIO_CHILD_ERROR);
    if let Some(buf) = NonNull::new(sc.sc_buf.get()) {
        dma_free(buf.cast(), VIORND_BUFSIZE);
        sc.sc_buf.set(ptr::null_mut());
    }
}

/// How many whole 32-bit words a `len`-byte answer holds: the C's loop bound,
/// `(i + 1) * sizeof(int) <= len`.
pub const fn viornd_nwords(len: i32) -> usize {
    if len <= 0 {
        0
    } else {
        len as usize / size_of::<i32>()
    }
}

/// `viornd_vq_done`: the queue's `vq_done`: hands the device's answer to `rnd(4)` and
/// schedules the next request.
pub fn viornd_vq_done(vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let sc = viornd_child(vsc);

    let Ok((slot, len)) = virtio_dequeue(vsc, vq) else {
        return 0;
    };
    bus_dmamap_sync(
        vsc.dmat(),
        sc.dmamap(),
        0,
        VIORND_BUFSIZE,
        BUS_DMASYNC_POSTREAD,
    );
    if len > VIORND_BUFSIZE as i32 {
        printf(format_args!(
            "{}: inconsistent descriptor length {} > {}\n",
            sc.sc_dev.xname(),
            len,
            VIORND_BUFSIZE
        ));
    } else {
        if VIORND_DEBUG > 0 {
            printf(format_args!("viornd_vq_done: got {len} bytes of entropy\n"));
        }
        for i in 0..viornd_nwords(len) {
            enqueue_randomness(sc.buf_word(i) as u32);
        }

        if sc.sc_interval.get() != 0 {
            timeout_add_sec(&sc.sc_tick, sc.sc_interval.get() as i32);
        }
    }

    // out:
    virtio_dequeue_commit(vq, slot);
    1
}

/// `viornd_tick`: puts the buffer on the queue for the device to fill.
pub fn viornd_tick(arg: *mut c_void) {
    // SAFETY: `viornd_attach` set the timeout with its softc, which is never freed.
    let sc = unsafe { &*arg.cast_const().cast::<ViorndSoftc>() };
    let vsc = sc.virtio();
    let vq = &sc.sc_vq[0];

    bus_dmamap_sync(
        vsc.dmat(),
        sc.dmamap(),
        0,
        VIORND_BUFSIZE,
        BUS_DMASYNC_PREREAD,
    );
    let slot = match virtio_enqueue_prep(vq) {
        Ok(slot) if virtio_enqueue_reserve(vq, slot, 1).is_ok() => slot,
        _ => panic(format_args!(
            "{}: virtqueue enqueue failed",
            sc.sc_dev.xname()
        )),
    };
    virtio_enqueue(vq, slot, sc.dmamap(), false);
    virtio_enqueue_commit(vsc, vq, slot, true);
}

/// `(struct viornd_softc *)vsc->sc_child`.
fn viornd_child(vsc: &VirtioSoftc) -> &'static ViorndSoftc {
    let c = vsc.sc_child.get();
    if c.is_null() || c == VIRTIO_CHILD_ERROR {
        panic(format_args!("viornd: virtio without its child"));
    }
    // SAFETY: viornd_attach stored its own device, the head of a `ViorndSoftc` that lives as
    // long as the kernel; only viornd's queue handler calls this.
    unsafe { &*c.cast::<ViorndSoftc>() }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of viornd(4): the request interval from the config flags, and how many words
    // of an answer go to rnd(4).

    use std::assert_eq;

    use super::*;

    #[test]
    fn interval_from_flags() {
        // GENERIC's `viornd* at virtio?` has no flags: every 15 << 5 seconds.
        assert_eq!(viornd_interval(0), 480);
        // The lowest byte is the transport's.
        assert_eq!(viornd_interval(0x00ff), 480);
        assert_eq!(viornd_interval(0x0100), 30);
        assert_eq!(viornd_interval(0x0f00), 15 << 15);
        assert_eq!(viornd_interval(VIORND_ONESHOT), 0);
        assert_eq!(viornd_interval(VIORND_ONESHOT | 0x0300), 0);
    }

    #[test]
    fn whole_words_only() {
        assert_eq!(viornd_nwords(0), 0);
        assert_eq!(viornd_nwords(3), 0);
        assert_eq!(viornd_nwords(4), 1);
        assert_eq!(viornd_nwords(15), 3);
        assert_eq!(viornd_nwords(VIORND_BUFSIZE as i32), 4);
        assert_eq!(viornd_nwords(-1), 0);
    }
}
/* </TESTS> */
