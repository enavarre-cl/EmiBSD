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
 * Copyright (c) 2014 Patrick Wildt <patrick@blueri.se>
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
/* </LICENSES> */

/* <CODE> */
//! The virtio MMIO transport: `virtio* at fdt?`, the `virtio,mmio` nodes of a device tree
//! (QEMU `virt` has 32 of them), legacy (version 1) and modern (version 2) register layouts.
//!
//! Upstream: sys/dev/fdt/virtio_mmio.c @ 3ce1f3f79392
//!
//! XXX: Before being used on big endian arches, the access to config registers needs to be
//! reviewed/fixed. The non-device specific registers are PCI-endian while the device specific
//! registers are native endian.
//!
//! ## Deviations
//! - `struct virtio_mmio_softc` begins with the `struct virtio_softc`, as in C; the
//!   `(struct virtio_mmio_softc *)vsc` cast of the `virtio_ops` functions is [`msc`], which
//!   checks that the softc's ops are this transport's before casting. The tags and handles
//!   are `Option`s (all-zero is `None`).
//! - `struct fdt_attach_args` and `fdt_intr_establish` come from the machine
//!   (`machine::fdt`); the interrupt name is a copy (`virtio_intr_name`,
//!   `dev/pv/virtio.rs`).
//! - `virtio_mmio_fdt_ca` (the attachment without a match function, for buses that match
//!   themselves) is kept; `ioconf.rs` uses `virtio_mmio_ca`, as `files.fdt` does.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::ofw::openfirm::OF_is_compatible;
use crate::dev::pv::virtio::{virtio_check_vqs, virtio_device_string, virtio_intr_name};
use crate::dev::pv::virtioreg::{
    VIRTIO_CONFIG_DEVICE_STATUS_ACK, VIRTIO_CONFIG_DEVICE_STATUS_DRIVER,
    VIRTIO_CONFIG_DEVICE_STATUS_FAILED, VIRTIO_F_ANY_LAYOUT, VIRTIO_F_RING_EVENT_IDX,
    VIRTIO_F_RING_INDIRECT_DESC, VIRTIO_PAGE_SIZE,
};
use crate::dev::pv::virtiovar::{
    VIRTIO_CF_NO_EVENT_IDX, VIRTIO_CF_NO_INDIRECT, VIRTIO_CHILD_ERROR, VIRTIO_DEBUG,
    VirtioAttachArgs, VirtioFeatureName, VirtioOps, VirtioSoftc, Virtqueue, virtio_device_reset,
    virtio_set_status,
};
use crate::kassert;
use crate::kern::subr_autoconf::{config_detach, config_found};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::machine::bus::{
    BusDmaTag, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_read_1,
    bus_space_read_2, bus_space_read_4, bus_space_unmap, bus_space_write_1, bus_space_write_2,
    bus_space_write_4,
};
use crate::machine::cpu::CpuInfo;
use crate::machine::fdt::{FdtAttachArgs, fdt_intr_disestablish, fdt_intr_establish};
use crate::machine::intr::{IntrFn, intr_barrier};
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::param::PAGE_SIZE;

/// `VIRTIO_MMIO_MAGIC`: "virt" in little-endian order.
pub const VIRTIO_MMIO_MAGIC: u32 =
    b'v' as u32 | (b'i' as u32) << 8 | (b'r' as u32) << 16 | (b't' as u32) << 24;

/// `VIRTIO_MMIO_MAGIC_VALUE`.
pub const VIRTIO_MMIO_MAGIC_VALUE: BusSize = 0x000;
/// `VIRTIO_MMIO_VERSION`.
pub const VIRTIO_MMIO_VERSION: BusSize = 0x004;
/// `VIRTIO_MMIO_DEVICE_ID`.
pub const VIRTIO_MMIO_DEVICE_ID: BusSize = 0x008;
/// `VIRTIO_MMIO_VENDOR_ID`.
pub const VIRTIO_MMIO_VENDOR_ID: BusSize = 0x00c;
/// `VIRTIO_MMIO_HOST_FEATURES`.
pub const VIRTIO_MMIO_HOST_FEATURES: BusSize = 0x010;
/// `VIRTIO_MMIO_HOST_FEATURES_SEL`.
pub const VIRTIO_MMIO_HOST_FEATURES_SEL: BusSize = 0x014;
/// `VIRTIO_MMIO_GUEST_FEATURES`.
pub const VIRTIO_MMIO_GUEST_FEATURES: BusSize = 0x020;
/// `VIRTIO_MMIO_GUEST_FEATURES_SEL`.
pub const VIRTIO_MMIO_GUEST_FEATURES_SEL: BusSize = 0x024;
/// `VIRTIO_MMIO_GUEST_PAGE_SIZE`.
pub const VIRTIO_MMIO_GUEST_PAGE_SIZE: BusSize = 0x028;
/// `VIRTIO_MMIO_QUEUE_SEL`.
pub const VIRTIO_MMIO_QUEUE_SEL: BusSize = 0x030;
/// `VIRTIO_MMIO_QUEUE_NUM_MAX`.
pub const VIRTIO_MMIO_QUEUE_NUM_MAX: BusSize = 0x034;
/// `VIRTIO_MMIO_QUEUE_NUM`.
pub const VIRTIO_MMIO_QUEUE_NUM: BusSize = 0x038;
/// `VIRTIO_MMIO_QUEUE_ALIGN`.
pub const VIRTIO_MMIO_QUEUE_ALIGN: BusSize = 0x03c;
/// `VIRTIO_MMIO_QUEUE_PFN`.
pub const VIRTIO_MMIO_QUEUE_PFN: BusSize = 0x040;
/// `VIRTIO_MMIO_QUEUE_READY`.
pub const VIRTIO_MMIO_QUEUE_READY: BusSize = 0x044;
/// `VIRTIO_MMIO_QUEUE_NOTIFY`.
pub const VIRTIO_MMIO_QUEUE_NOTIFY: BusSize = 0x050;
/// `VIRTIO_MMIO_INTERRUPT_STATUS`.
pub const VIRTIO_MMIO_INTERRUPT_STATUS: BusSize = 0x060;
/// `VIRTIO_MMIO_INTERRUPT_ACK`.
pub const VIRTIO_MMIO_INTERRUPT_ACK: BusSize = 0x064;
/// `VIRTIO_MMIO_STATUS`.
pub const VIRTIO_MMIO_STATUS: BusSize = 0x070;
/// `VIRTIO_MMIO_QUEUE_DESC_LOW`.
pub const VIRTIO_MMIO_QUEUE_DESC_LOW: BusSize = 0x080;
/// `VIRTIO_MMIO_QUEUE_DESC_HIGH`.
pub const VIRTIO_MMIO_QUEUE_DESC_HIGH: BusSize = 0x084;
/// `VIRTIO_MMIO_QUEUE_AVAIL_LOW`.
pub const VIRTIO_MMIO_QUEUE_AVAIL_LOW: BusSize = 0x090;
/// `VIRTIO_MMIO_QUEUE_AVAIL_HIGH`.
pub const VIRTIO_MMIO_QUEUE_AVAIL_HIGH: BusSize = 0x094;
/// `VIRTIO_MMIO_QUEUE_USED_LOW`.
pub const VIRTIO_MMIO_QUEUE_USED_LOW: BusSize = 0x0a0;
/// `VIRTIO_MMIO_QUEUE_USED_HIGH`.
pub const VIRTIO_MMIO_QUEUE_USED_HIGH: BusSize = 0x0a4;
/// `VIRTIO_MMIO_CONFIG`.
pub const VIRTIO_MMIO_CONFIG: i32 = 0x100;

/// `VIRTIO_MMIO_INT_VRING`.
pub const VIRTIO_MMIO_INT_VRING: u32 = 1 << 0;
/// `VIRTIO_MMIO_INT_CONFIG`.
pub const VIRTIO_MMIO_INT_CONFIG: u32 = 1 << 1;

/// `struct virtio_mmio_softc`.
#[repr(C)]
pub struct VirtioMmioSoftc {
    /// `sc_sc`.
    pub sc_sc: VirtioSoftc,

    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_iosize`.
    pub sc_iosize: Cell<BusSize>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,

    /// `sc_ih`.
    pub sc_ih: Cell<*mut c_void>,

    /// `sc_config_offset`.
    pub sc_config_offset: Cell<i32>,
    /// `sc_version`.
    pub sc_version: Cell<u32>,
}

impl VirtioMmioSoftc {
    /// `(sc->sc_iot, sc->sc_ioh)`, mapped by attach.
    fn io(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.sc_iot.get(), self.sc_ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("virtio_mmio: registers not mapped")),
        }
    }
}

// SAFETY: `#[repr(C)]` with the virtio softc (itself headed by the device) first; the other
// members are `Cell`s of integers, pointers and `Option`s, all valid as zero bits.
unsafe impl Softc for VirtioMmioSoftc {}

/// `struct virtio_mmio_attach_args`.
#[repr(C)]
pub struct VirtioMmioAttachArgs {
    /// `vma_va`.
    pub vma_va: VirtioAttachArgs,
    /// `vma_fa`.
    pub vma_fa: *mut c_void,
}

/// `virtio_mmio_ca`.
pub static VIRTIO_MMIO_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<VirtioMmioSoftc>(),
    ca_match: Some(virtio_mmio_match),
    ca_attach: virtio_mmio_attach,
    ca_detach: Some(virtio_mmio_detach),
    ca_activate: None,
};

/// `virtio_mmio_fdt_ca`.
pub static VIRTIO_MMIO_FDT_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<VirtioMmioSoftc>(),
    ca_match: None,
    ca_attach: virtio_mmio_attach,
    ca_detach: Some(virtio_mmio_detach),
    ca_activate: None,
};

/// `virtio_mmio_ops`.
pub static VIRTIO_MMIO_OPS: VirtioOps = VirtioOps {
    kick: virtio_mmio_kick,
    read_dev_cfg_1: virtio_mmio_read_device_config_1,
    read_dev_cfg_2: virtio_mmio_read_device_config_2,
    read_dev_cfg_4: virtio_mmio_read_device_config_4,
    read_dev_cfg_8: virtio_mmio_read_device_config_8,
    write_dev_cfg_1: virtio_mmio_write_device_config_1,
    write_dev_cfg_2: virtio_mmio_write_device_config_2,
    write_dev_cfg_4: virtio_mmio_write_device_config_4,
    write_dev_cfg_8: virtio_mmio_write_device_config_8,
    read_queue_size: virtio_mmio_read_queue_size,
    setup_queue: virtio_mmio_setup_queue,
    setup_intrs: virtio_mmio_setup_intrs,
    get_status: virtio_mmio_get_status,
    set_status: virtio_mmio_set_status,
    neg_features: virtio_mmio_negotiate_features,
    attach_finish: virtio_mmio_attach_finish,
    poll_intr: virtio_mmio_intr,
    intr_barrier: virtio_mmio_intr_barrier,
    intr_establish: virtio_mmio_intr_establish,
};

/// `(struct virtio_mmio_softc *)vsc`.
fn msc(vsc: &VirtioSoftc) -> &VirtioMmioSoftc {
    if !vsc
        .sc_ops
        .get()
        .is_some_and(|o| ptr::eq(o, &VIRTIO_MMIO_OPS))
    {
        panic(format_args!("virtio_mmio: not a virtio_mmio softc"));
    }
    // SAFETY: only virtio_mmio_attach installs `VIRTIO_MMIO_OPS`, on the `sc_sc` member of a
    // `VirtioMmioSoftc`, which is `#[repr(C)]` with that member first.
    unsafe { &*ptr::from_ref(vsc).cast::<VirtioMmioSoftc>() }
}

/// `virtio_mmio_read_queue_size`.
pub fn virtio_mmio_read_queue_size(vsc: &VirtioSoftc, idx: u16) -> u16 {
    let (iot, ioh) = msc(vsc).io();
    bus_space_write_4(iot, ioh, VIRTIO_MMIO_QUEUE_SEL, u32::from(idx));
    bus_space_read_4(iot, ioh, VIRTIO_MMIO_QUEUE_NUM_MAX) as u16
}

/// `virtio_mmio_setup_queue`.
pub fn virtio_mmio_setup_queue(vsc: &VirtioSoftc, vq: &Virtqueue, addr: u64) {
    let sc = msc(vsc);
    let (iot, ioh) = sc.io();
    bus_space_write_4(iot, ioh, VIRTIO_MMIO_QUEUE_SEL, vq.vq_index.get() as u32);
    bus_space_write_4(
        iot,
        ioh,
        VIRTIO_MMIO_QUEUE_NUM,
        bus_space_read_4(iot, ioh, VIRTIO_MMIO_QUEUE_NUM_MAX),
    );
    if sc.sc_version.get() == 1 {
        bus_space_write_4(iot, ioh, VIRTIO_MMIO_QUEUE_ALIGN, PAGE_SIZE as u32);
        bus_space_write_4(
            iot,
            ioh,
            VIRTIO_MMIO_QUEUE_PFN,
            (addr / VIRTIO_PAGE_SIZE as u64) as u32,
        );
    } else {
        let avail = addr + vq.vq_availoffset.get() as u64;
        let used = addr + vq.vq_usedoffset.get() as u64;
        bus_space_write_4(iot, ioh, VIRTIO_MMIO_QUEUE_DESC_LOW, addr as u32);
        bus_space_write_4(iot, ioh, VIRTIO_MMIO_QUEUE_DESC_HIGH, (addr >> 32) as u32);
        bus_space_write_4(iot, ioh, VIRTIO_MMIO_QUEUE_AVAIL_LOW, avail as u32);
        bus_space_write_4(iot, ioh, VIRTIO_MMIO_QUEUE_AVAIL_HIGH, (avail >> 32) as u32);
        bus_space_write_4(iot, ioh, VIRTIO_MMIO_QUEUE_USED_LOW, used as u32);
        bus_space_write_4(iot, ioh, VIRTIO_MMIO_QUEUE_USED_HIGH, (used >> 32) as u32);
        bus_space_write_4(iot, ioh, VIRTIO_MMIO_QUEUE_READY, 1);
    }
}

/// `virtio_mmio_setup_intrs`: one interrupt for everything; nothing to route.
pub fn virtio_mmio_setup_intrs(_vsc: &VirtioSoftc) {}

/// `virtio_mmio_get_status`.
pub fn virtio_mmio_get_status(vsc: &VirtioSoftc) -> i32 {
    let (iot, ioh) = msc(vsc).io();
    bus_space_read_4(iot, ioh, VIRTIO_MMIO_STATUS) as i32
}

/// `virtio_mmio_set_status`: or `status` into the device status; 0 resets the device and
/// waits for it.
pub fn virtio_mmio_set_status(vsc: &VirtioSoftc, status: i32) {
    let (iot, ioh) = msc(vsc).io();

    if status == 0 {
        bus_space_write_4(iot, ioh, VIRTIO_MMIO_STATUS, 0);
        while bus_space_read_4(iot, ioh, VIRTIO_MMIO_STATUS) != 0 {
            core::hint::spin_loop(); // CPU_BUSY_CYCLE()
        }
    } else {
        let old = bus_space_read_4(iot, ioh, VIRTIO_MMIO_STATUS);
        bus_space_write_4(iot, ioh, VIRTIO_MMIO_STATUS, status as u32 | old);
    }
}

/// `(struct fdt_attach_args *)aux`.
fn fdt_args<'a>(aux: *mut c_void) -> &'a FdtAttachArgs<'a> {
    // SAFETY: the fdt bus (mainbus, simplebus) hands its children `fdt_attach_args`, alive
    // for the duration of the match and attach.
    unsafe { &*aux.cast::<FdtAttachArgs<'a>>() }
}

/// `virtio_mmio_match`: a `virtio,mmio` node.
pub fn virtio_mmio_match(_parent: Option<&Device>, _cfdata: &CfMatch, aux: *mut c_void) -> i32 {
    let faa = fdt_args(aux);

    i32::from(OF_is_compatible(faa.fa_node, b"virtio,mmio"))
}

/// `virtio_mmio_attach`.
pub fn virtio_mmio_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let faa = fdt_args(aux);
    // SAFETY: `self_` was made for `virtio_mmio_ca`, whose softc is a `VirtioMmioSoftc`.
    let sc = unsafe { self_.softc::<VirtioMmioSoftc>() };
    let vsc = &sc.sc_sc;
    let mut vma = VirtioMmioAttachArgs {
        vma_va: VirtioAttachArgs::default(),
        vma_fa: aux,
    };

    let Some(reg) = faa.fa_reg.first() else {
        printf(format_args!(": no register data\n"));
        return;
    };

    sc.sc_iosize.set(reg.size as BusSize);
    sc.sc_iot.set(Some(faa.fa_iot));
    sc.sc_dmat.set(Some(faa.fa_dmat));
    // SAFETY: the node's `reg` names the device's registers, which this driver owns.
    match unsafe { bus_space_map(faa.fa_iot, reg.addr as usize, reg.size as BusSize, 0) } {
        Ok(h) => sc.sc_ioh.set(Some(h)),
        Err(_) => panic(format_args!("virtio_mmio_attach: bus_space_map failed!")),
    }
    let (iot, ioh) = sc.io();

    let magic = bus_space_read_4(iot, ioh, VIRTIO_MMIO_MAGIC_VALUE);
    if magic != VIRTIO_MMIO_MAGIC {
        printf(format_args!(
            ": wrong magic value 0x{magic:08x}; giving up\n"
        ));
        return;
    }

    sc.sc_version
        .set(bus_space_read_4(iot, ioh, VIRTIO_MMIO_VERSION));
    if sc.sc_version.get() < 1 || sc.sc_version.get() > 2 {
        printf(format_args!(
            ": unknown version 0x{:02x}; giving up\n",
            sc.sc_version.get()
        ));
        return;
    }

    let id = bus_space_read_4(iot, ioh, VIRTIO_MMIO_DEVICE_ID);
    printf(format_args!(
        ": Virtio {} Device",
        virtio_device_string(id as i32)
    ));

    printf(format_args!("\n"));

    // No device connected.
    if id == 0 {
        return;
    }

    if sc.sc_version.get() == 1 {
        bus_space_write_4(iot, ioh, VIRTIO_MMIO_GUEST_PAGE_SIZE, PAGE_SIZE as u32);
    }

    vsc.sc_ops.set(Some(&VIRTIO_MMIO_OPS));
    vsc.sc_dmat.set(sc.sc_dmat.get());
    sc.sc_config_offset.set(VIRTIO_MMIO_CONFIG);

    virtio_device_reset(vsc);
    virtio_mmio_set_status(vsc, VIRTIO_CONFIG_DEVICE_STATUS_ACK);
    virtio_mmio_set_status(vsc, VIRTIO_CONFIG_DEVICE_STATUS_DRIVER);

    vma.vma_va.va_devid = id as i32;
    vma.vma_va.va_nintr = 1;
    vsc.sc_child.set(ptr::null());
    config_found(self_, ptr::from_mut(&mut vma).cast(), None);
    if vsc.sc_child.get().is_null() {
        printf(format_args!(
            "{}: no matching child driver; not configured\n",
            Str(&vsc.sc_dev.dv_xname.get())
        ));
    } else if vsc.sc_child.get() == VIRTIO_CHILD_ERROR {
        printf(format_args!(
            "{}: virtio configuration failed\n",
            Str(&vsc.sc_dev.dv_xname.get())
        ));
    } else {
        return;
    }

    // fail:
    virtio_set_status(vsc, VIRTIO_CONFIG_DEVICE_STATUS_FAILED);
}

/// `virtio_mmio_attach_finish`: the node's interrupt.
pub fn virtio_mmio_attach_finish(
    vsc: &VirtioSoftc,
    va: *mut VirtioAttachArgs,
) -> Result<(), Errno> {
    let sc = msc(vsc);
    // SAFETY: virtio_mmio_attach passes its `VirtioMmioAttachArgs` (which begins with the
    // `VirtioAttachArgs`) down to the child, which hands it back here.
    let vma = unsafe { &*va.cast::<VirtioMmioAttachArgs>() };
    let faa = fdt_args(vma.vma_fa);

    let ih = fdt_intr_establish(
        faa.fa_node,
        vsc.sc_ipl.get(),
        virtio_mmio_intr,
        ptr::from_ref(sc).cast_mut().cast(),
        virtio_intr_name(&vsc.sc_dev.dv_xname.get()),
    );
    let Some(ih) = ih else {
        printf(format_args!(
            "{}: couldn't establish interrupt\n",
            Str(&vsc.sc_dev.dv_xname.get())
        ));
        return Err(Errno::EIO);
    };
    sc.sc_ih.set(ih.as_ptr());
    Ok(())
}

/// `virtio_mmio_detach`.
pub fn virtio_mmio_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `virtio_mmio_ca`.
    let sc = unsafe { self_.softc::<VirtioMmioSoftc>() };
    let vsc = &sc.sc_sc;

    if let Some(child) = vsc.child() {
        // SAFETY: the child is an attached device; config_detach frees it on success and
        // nothing here uses it afterwards.
        unsafe { config_detach(NonNull::from(child), flags) }?;
    }
    kassert!(vsc.sc_child.get().is_null() || vsc.sc_child.get() == VIRTIO_CHILD_ERROR);
    kassert!(vsc.sc_vqs.get().is_null());
    if let Some(ih) = NonNull::new(sc.sc_ih.get()) {
        // SAFETY: the handle came from fdt_intr_establish and is dropped here.
        unsafe { fdt_intr_disestablish(ih) };
    }
    sc.sc_ih.set(ptr::null_mut());
    if sc.sc_iosize.get() != 0 {
        let (iot, ioh) = sc.io();
        bus_space_unmap(iot, ioh, sc.sc_iosize.get());
    }
    sc.sc_iosize.set(0);

    Ok(())
}

/// `virtio_mmio_negotiate_features`: feature negotiation (the low 32 bits). Prints available
/// / negotiated features if `guest_feature_names` is given and `VIRTIO_DEBUG` is 1.
pub fn virtio_mmio_negotiate_features(
    vsc: &VirtioSoftc,
    guest_feature_names: Option<&[VirtioFeatureName]>,
) -> Result<(), Errno> {
    let (iot, ioh) = msc(vsc).io();

    vsc.sc_active_features.set(0);

    let my_flags = vsc.sc_dev.cfdata().cf_flags;
    let child_flags = vsc.child().map_or(0, |c| c.cfdata().cf_flags);

    // We enable indirect descriptors by default. They can be switched off by setting bit 1
    // in the driver flags, see config(8).
    if my_flags & VIRTIO_CF_NO_INDIRECT == 0 && child_flags & VIRTIO_CF_NO_INDIRECT == 0 {
        vsc.sc_driver_features
            .set(vsc.sc_driver_features.get() | VIRTIO_F_RING_INDIRECT_DESC);
    } else if guest_feature_names.is_some() {
        printf(format_args!("RingIndirectDesc disabled by UKC\n"));
    }
    // The driver must add VIRTIO_F_RING_EVENT_IDX if it supports it. If it did, check if it
    // is disabled by bit 2 in the driver flags.
    if vsc.sc_driver_features.get() & VIRTIO_F_RING_EVENT_IDX != 0
        && (my_flags & VIRTIO_CF_NO_EVENT_IDX != 0 || child_flags & VIRTIO_CF_NO_EVENT_IDX != 0)
    {
        if guest_feature_names.is_some() {
            printf(format_args!(" RingEventIdx disabled by UKC"));
        }
        vsc.sc_driver_features
            .set(vsc.sc_driver_features.get() & !VIRTIO_F_RING_EVENT_IDX);
    }

    vsc.sc_driver_features
        .set(vsc.sc_driver_features.get() | VIRTIO_F_ANY_LAYOUT);
    bus_space_write_4(iot, ioh, VIRTIO_MMIO_HOST_FEATURES_SEL, 0);
    let host = u64::from(bus_space_read_4(iot, ioh, VIRTIO_MMIO_HOST_FEATURES));
    let neg = host & vsc.sc_driver_features.get();
    if VIRTIO_DEBUG > 0
        && let Some(names) = guest_feature_names
    {
        crate::dev::pv::virtio::virtio_log_features(host, neg, names);
    }
    bus_space_write_4(iot, ioh, VIRTIO_MMIO_GUEST_FEATURES_SEL, 0);
    bus_space_write_4(iot, ioh, VIRTIO_MMIO_GUEST_FEATURES, neg as u32);
    vsc.sc_active_features.set(neg);
    vsc.sc_indirect
        .set(i32::from(neg & VIRTIO_F_RING_INDIRECT_DESC != 0));

    Ok(())
}

// Device configuration registers.

/// The register of device configuration byte `index`.
fn config_reg(vsc: &VirtioSoftc, index: i32) -> (BusSpaceTag, BusSpaceHandle, BusSize) {
    let sc = msc(vsc);
    let (iot, ioh) = sc.io();
    (iot, ioh, (sc.sc_config_offset.get() + index) as BusSize)
}

/// `virtio_mmio_read_device_config_1`.
pub fn virtio_mmio_read_device_config_1(vsc: &VirtioSoftc, index: i32) -> u8 {
    let (t, h, o) = config_reg(vsc, index);
    bus_space_read_1(t, h, o)
}

/// `virtio_mmio_read_device_config_2`.
pub fn virtio_mmio_read_device_config_2(vsc: &VirtioSoftc, index: i32) -> u16 {
    let (t, h, o) = config_reg(vsc, index);
    bus_space_read_2(t, h, o)
}

/// `virtio_mmio_read_device_config_4`.
pub fn virtio_mmio_read_device_config_4(vsc: &VirtioSoftc, index: i32) -> u32 {
    let (t, h, o) = config_reg(vsc, index);
    bus_space_read_4(t, h, o)
}

/// `virtio_mmio_read_device_config_8`.
pub fn virtio_mmio_read_device_config_8(vsc: &VirtioSoftc, index: i32) -> u64 {
    let (t, h, o) = config_reg(vsc, index);
    let mut r = u64::from(bus_space_read_4(t, h, o + size_of::<u32>()));
    r <<= 32;
    r + u64::from(bus_space_read_4(t, h, o))
}

/// `virtio_mmio_write_device_config_1`.
pub fn virtio_mmio_write_device_config_1(vsc: &VirtioSoftc, index: i32, value: u8) {
    let (t, h, o) = config_reg(vsc, index);
    bus_space_write_1(t, h, o, value);
}

/// `virtio_mmio_write_device_config_2`.
pub fn virtio_mmio_write_device_config_2(vsc: &VirtioSoftc, index: i32, value: u16) {
    let (t, h, o) = config_reg(vsc, index);
    bus_space_write_2(t, h, o, value);
}

/// `virtio_mmio_write_device_config_4`.
pub fn virtio_mmio_write_device_config_4(vsc: &VirtioSoftc, index: i32, value: u32) {
    let (t, h, o) = config_reg(vsc, index);
    bus_space_write_4(t, h, o, value);
}

/// `virtio_mmio_write_device_config_8`.
pub fn virtio_mmio_write_device_config_8(vsc: &VirtioSoftc, index: i32, value: u64) {
    let (t, h, o) = config_reg(vsc, index);
    bus_space_write_4(t, h, o, (value & 0xffff_ffff) as u32);
    bus_space_write_4(t, h, o + size_of::<u32>(), (value >> 32) as u32);
}

/// `virtio_mmio_intr`: interrupt handler.
pub fn virtio_mmio_intr(arg: *mut c_void) -> i32 {
    // SAFETY: established with the softc as its argument (virtio_mmio_attach_finish), which
    // begins with the virtio softc; softcs outlive their interrupts.
    let sc = msc(unsafe { &*arg.cast::<VirtioSoftc>() });
    let vsc = &sc.sc_sc;
    let (iot, ioh) = sc.io();
    let mut r = 0;

    // check and ack the interrupt
    let isr = bus_space_read_4(iot, ioh, VIRTIO_MMIO_INTERRUPT_STATUS);
    bus_space_write_4(iot, ioh, VIRTIO_MMIO_INTERRUPT_ACK, isr);
    if isr & VIRTIO_MMIO_INT_CONFIG != 0
        && let Some(config_change) = vsc.sc_config_change.get()
    {
        r = config_change(vsc);
    }
    if isr & VIRTIO_MMIO_INT_VRING != 0 {
        r |= virtio_check_vqs(vsc);
    }

    r
}

/// `virtio_mmio_kick`.
pub fn virtio_mmio_kick(vsc: &VirtioSoftc, idx: u16) {
    let (iot, ioh) = msc(vsc).io();
    bus_space_write_4(iot, ioh, VIRTIO_MMIO_QUEUE_NOTIFY, u32::from(idx));
}

/// `virtio_mmio_intr_barrier`.
pub fn virtio_mmio_intr_barrier(vsc: &VirtioSoftc) {
    let sc = msc(vsc);
    if let Some(ih) = NonNull::new(sc.sc_ih.get()) {
        intr_barrier(ih);
    }
}

/// `virtio_mmio_intr_establish`: one interrupt only, children cannot have their own.
pub fn virtio_mmio_intr_establish(
    _vsc: &VirtioSoftc,
    _va: *mut VirtioAttachArgs,
    _vec: i32,
    _ci: Option<&'static CpuInfo>,
    _func: IntrFn,
    _arg: *mut c_void,
) -> Result<(), Errno> {
    Err(Errno::ENXIO)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_magic_is_virt() {
        assert_eq!(VIRTIO_MMIO_MAGIC.to_le_bytes(), *b"virt");
        assert_eq!(VIRTIO_MMIO_MAGIC, 0x7472_6976);
    }
}
/* </TESTS> */
