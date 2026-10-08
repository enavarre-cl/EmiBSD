/*	$OpenBSD: virtio_pci.c,v 1.53 2025/12/22 20:24:49 sf Exp $	*/
/*	$NetBSD: virtio.c,v 1.3 2011/11/02 23:05:52 njoly Exp $	*/
/* <LICENSES> */
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
/* </LICENSES> */

/* <CODE> */
//! The virtio PCI transport: `virtio* at pci?`, modern (virtio 1.0, capabilities in
//! configuration space, memory BARs) and legacy (virtio 0.9, one I/O BAR).
//!
//! Upstream: sys/dev/pci/virtio_pci.c @ 3ce1f3f79392
//!
//! XXX: Before being used on big endian arches, the access to config registers needs to be
//! reviewed/fixed. The non-device specific registers are PCI-endian while the device specific
//! registers are native endian.
//!
//! ## Deviations
//! - `struct virtio_pci_softc` begins with the `struct virtio_softc`, as in C; the
//!   `(struct virtio_pci_softc *)vsc` cast of the `virtio_ops` functions is [`psc`], which
//!   checks that the softc's ops are this transport's before casting.
//! - The softc's tags and handles are `Option`s (all-zero is `None`), set by attach.
//! - The C's `#if defined(__i386__) || defined(__amd64__)` around forcing
//!   `PCI_FLAGS_MSI_ENABLED` is the machine's `PCI_MSI_PER_BRIDGE` (`machine::pci_machdep`).
//! - Capabilities are read into arrays of `pcireg_t` and decoded with
//!   `VirtioPciCap::from_regs` instead of the C's union; a capability that names a BAR
//!   beyond the six, or a device needing more than the four BAR slots of the softc, is refused
//!   (`EIO`) where the C would index past its arrays.
//! - `virtio_pci_msix_establish` returns `EPERM` where `pci_intr_map_msix` fails: the C
//!   returns that function's `1`, which is `EPERM`'s number; the callers only test for
//!   nonzero.
//! - `virtio_pci_adjust_config_region` returns `true` where the C returns 1 (failure).
//! - Interrupt names are handed over as copies (`virtio_intr_name`, `dev/pv/virtio.rs`).
//! - On the machines here MSI and MSI-X are refused by `pci_intr_map_msi*` (no MP tables,
//!   `arch/amd64/pci/pci_machdep.rs`), so `virtio_pci_attach_finish` falls back to the INTx
//!   line the firmware routed, as the C does on such a machine.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};

use crate::dev::pci::pci::{pci_get_capability, pci_intr_msix_count};
use crate::dev::pci::pci_map::{pci_mapreg_map, pci_mapreg_type};
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_OPENBSD_CONTROL, PCI_VENDOR_OPENBSD, PCI_VENDOR_QUMRANET,
};
use crate::dev::pci::pcireg::{
    PCI_CAP_VENDSPEC, PCI_MAPREG_END, PCI_MAPREG_START, PCI_SUBSYS_ID_REG, pci_product,
    pci_revision, pci_vendor,
};
use crate::dev::pci::pcivar::{PCI_FLAGS_MSI_ENABLED, PciAttachArgs, Pcireg};
use crate::dev::pci::virtio_pcireg::{
    VIRTIO_CONFIG_DEVICE_CONFIG_MSI, VIRTIO_CONFIG_DEVICE_CONFIG_NOMSI,
    VIRTIO_CONFIG_DEVICE_FEATURES, VIRTIO_CONFIG_DEVICE_STATUS, VIRTIO_CONFIG_GUEST_FEATURES,
    VIRTIO_CONFIG_ISR_CONFIG_CHANGE, VIRTIO_CONFIG_ISR_STATUS, VIRTIO_CONFIG_QUEUE_ADDRESS,
    VIRTIO_CONFIG_QUEUE_NOTIFY, VIRTIO_CONFIG_QUEUE_SELECT, VIRTIO_CONFIG_QUEUE_SIZE,
    VIRTIO_MSI_CONFIG_VECTOR, VIRTIO_MSI_NO_VECTOR, VIRTIO_MSI_QUEUE_VECTOR,
    VIRTIO_PCI_CAP_COMMON_CFG, VIRTIO_PCI_CAP_DEVICE_CFG, VIRTIO_PCI_CAP_ISR_CFG,
    VIRTIO_PCI_CAP_NOTIFY_CFG, VirtioPciCap, VirtioPciCommonCfg,
};
use crate::dev::pv::virtio::{virtio_check_vq, virtio_check_vqs, virtio_intr_name};
use crate::dev::pv::virtioreg::{
    VIRTIO_CONFIG_DEVICE_STATUS_ACK, VIRTIO_CONFIG_DEVICE_STATUS_DRIVER,
    VIRTIO_CONFIG_DEVICE_STATUS_FAILED, VIRTIO_CONFIG_DEVICE_STATUS_FEATURES_OK,
    VIRTIO_F_ACCESS_PLATFORM, VIRTIO_F_ANY_LAYOUT, VIRTIO_F_NOTIFY_ON_EMPTY,
    VIRTIO_F_RING_EVENT_IDX, VIRTIO_F_RING_INDIRECT_DESC, VIRTIO_F_VERSION_1, VIRTIO_PAGE_SIZE,
};
use crate::dev::pv::virtiovar::{
    VIRTIO_CF_NO_EVENT_IDX, VIRTIO_CF_NO_INDIRECT, VIRTIO_CF_PREFER_VERSION_09, VIRTIO_CHILD_ERROR,
    VIRTIO_DEBUG, VirtioAttachArgs, VirtioFeatureName, VirtioOps, VirtioSoftc, Virtqueue,
    virtio_device_reset, virtio_set_status,
};
use crate::kassert;
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::subr_autoconf::{config_detach, config_found};
use crate::kern::subr_prf::{Str, panic, printf, snprintf};
use crate::machine::bus::{
    BusSize, BusSpaceHandle, BusSpaceTag, bus_space_read_1, bus_space_read_2, bus_space_read_4,
    bus_space_subregion, bus_space_unmap, bus_space_write_1, bus_space_write_2, bus_space_write_4,
};
use crate::machine::cpu::CpuInfo;
use crate::machine::intr::{IPL_MPSAFE, IntrFn, intr_barrier};
use crate::machine::pci_machdep::{
    PCI_MSI_PER_BRIDGE, PciChipsetTag, PciIntrStr, Pcitag, pci_conf_read, pci_intr_disestablish,
    pci_intr_establish, pci_intr_establish_cpu, pci_intr_map, pci_intr_map_msi, pci_intr_map_msix,
    pci_intr_string,
};
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::systm::{kernel_lock, kernel_unlock};

/// `MAX_MSIX_VECS`.
const MAX_MSIX_VECS: i32 = 16;

/// `NMAPREG`: the BARs of a type 0 header.
const NMAPREG: usize = ((PCI_MAPREG_END - PCI_MAPREG_START) as usize) / size_of::<Pcireg>();

/// `enum irq_type`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim
pub enum IrqType {
    /// `IRQ_NO_MSIX`.
    IRQ_NO_MSIX = 0,
    /// `IRQ_MSIX_SHARED`: vec 0: config irq, vec 1 shared by all vqs.
    IRQ_MSIX_SHARED,
    /// `IRQ_MSIX_PER_VQ`: vec 0: config irq, vec n: irq of vq[n-1].
    IRQ_MSIX_PER_VQ,
    /// `IRQ_MSIX_CHILD`: assigned by child driver.
    IRQ_MSIX_CHILD,
}

pub use IrqType::*;

/// `struct virtio_pci_intr`.
pub struct VirtioPciIntr {
    /// `name`.
    pub name: Cell<[u8; 16]>,
    /// `ih`.
    pub ih: Cell<*mut c_void>,
}

/// `struct virtio_pci_softc`.
#[repr(C)]
pub struct VirtioPciSoftc {
    /// `sc_sc`.
    pub sc_sc: VirtioSoftc,
    /// `sc_pc`.
    pub sc_pc: Cell<Option<PciChipsetTag>>,
    /// `sc_ptag`.
    pub sc_ptag: Cell<Pcitag>,

    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_iosize`.
    pub sc_iosize: Cell<BusSize>,

    /// `sc_bars_iot`.
    pub sc_bars_iot: [Cell<Option<BusSpaceTag>>; 4],
    /// `sc_bars_ioh`.
    pub sc_bars_ioh: [Cell<Option<BusSpaceHandle>>; 4],
    /// `sc_bars_iosize`.
    pub sc_bars_iosize: [Cell<BusSize>; 4],

    /// `sc_notify_iot`.
    pub sc_notify_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_notify_ioh`.
    pub sc_notify_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_notify_iosize`.
    pub sc_notify_iosize: Cell<BusSize>,
    /// `sc_notify_off_multiplier`.
    pub sc_notify_off_multiplier: Cell<u32>,

    /// `sc_devcfg_iot`.
    pub sc_devcfg_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_devcfg_ioh`.
    pub sc_devcfg_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_devcfg_iosize`.
    pub sc_devcfg_iosize: Cell<BusSize>,
    /// `sc_devcfg_offset`: with 0.9, the offset of the devcfg region in the io bar changes
    /// depending on MSI-X being enabled or not. With 1.0, this field is still used to
    /// remember if MSI-X is enabled or not.
    pub sc_devcfg_offset: Cell<u32>,

    /// `sc_isr_iot`.
    pub sc_isr_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_isr_ioh`.
    pub sc_isr_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_isr_iosize`.
    pub sc_isr_iosize: Cell<BusSize>,

    /// `sc_intr`: `sc_nintr` entries.
    pub sc_intr: Cell<*mut VirtioPciIntr>,
    /// `sc_nintr`.
    pub sc_nintr: Cell<i32>,

    /// `sc_irq_type`.
    pub sc_irq_type: Cell<IrqType>,
}

impl VirtioPciSoftc {
    /// `sc->sc_pc`.
    fn pc(&self) -> PciChipsetTag {
        match self.sc_pc.get() {
            Some(pc) => pc,
            None => panic(format_args!("virtio_pci: no chipset tag")),
        }
    }

    /// `&sc->sc_intr[i]`.
    fn intr(&self, i: i32) -> &VirtioPciIntr {
        let base = self.sc_intr.get();
        if base.is_null() || i < 0 || i >= self.sc_nintr.get() {
            panic(format_args!("virtio_pci: bad interrupt index {i}"));
        }
        // SAFETY: attach allocated `sc_nintr` zeroed entries (valid all-zero) at `sc_intr`.
        unsafe { &*base.add(i as usize) }
    }
}

/// `(iot, ioh)` of a region the attach mapped.
fn region(
    t: &Cell<Option<BusSpaceTag>>,
    h: &Cell<Option<BusSpaceHandle>>,
) -> (BusSpaceTag, BusSpaceHandle) {
    match (t.get(), h.get()) {
        (Some(t), Some(h)) => (t, h),
        _ => panic(format_args!("virtio_pci: region not mapped")),
    }
}

// SAFETY: `#[repr(C)]` with the virtio softc (itself headed by the device) first; the other
// members are `Cell`s of integers, pointers, `Option`s and an enum whose zero is
// `IRQ_NO_MSIX`: all valid as zero bits.
unsafe impl Softc for VirtioPciSoftc {}

/// `struct virtio_pci_attach_args`.
#[repr(C)]
pub struct VirtioPciAttachArgs {
    /// `vpa_va`.
    pub vpa_va: VirtioAttachArgs,
    /// `vpa_pa`.
    pub vpa_pa: *mut PciAttachArgs,
    /// `vpa_msix`.
    pub vpa_msix: i32,
}

impl VirtioPciAttachArgs {
    /// `vpa->vpa_pa`.
    fn pa(&self) -> &PciAttachArgs {
        // SAFETY: virtio_pci_attach points it at its own `pci_attach_args`, which outlive the
        // child's attach (config_found runs inside virtio_pci_attach).
        unsafe { &*self.vpa_pa }
    }
}

/// `virtio_pci_ca`.
pub static VIRTIO_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<VirtioPciSoftc>(),
    ca_match: Some(virtio_pci_match),
    ca_attach: virtio_pci_attach,
    ca_detach: Some(virtio_pci_detach),
    ca_activate: None,
};

/// `virtio_pci_ops`.
pub static VIRTIO_PCI_OPS: VirtioOps = VirtioOps {
    kick: virtio_pci_kick,
    read_dev_cfg_1: virtio_pci_read_device_config_1,
    read_dev_cfg_2: virtio_pci_read_device_config_2,
    read_dev_cfg_4: virtio_pci_read_device_config_4,
    read_dev_cfg_8: virtio_pci_read_device_config_8,
    write_dev_cfg_1: virtio_pci_write_device_config_1,
    write_dev_cfg_2: virtio_pci_write_device_config_2,
    write_dev_cfg_4: virtio_pci_write_device_config_4,
    write_dev_cfg_8: virtio_pci_write_device_config_8,
    read_queue_size: virtio_pci_read_queue_size,
    setup_queue: virtio_pci_setup_queue,
    setup_intrs: virtio_pci_setup_intrs,
    get_status: virtio_pci_get_status,
    set_status: virtio_pci_set_status,
    neg_features: virtio_pci_negotiate_features,
    attach_finish: virtio_pci_attach_finish,
    poll_intr: virtio_pci_poll_intr,
    intr_barrier: virtio_pci_intr_barrier,
    intr_establish: virtio_pci_intr_establish,
};

/// `(struct virtio_pci_softc *)vsc`.
fn psc(vsc: &VirtioSoftc) -> &VirtioPciSoftc {
    if !vsc
        .sc_ops
        .get()
        .is_some_and(|o| ptr::eq(o, &VIRTIO_PCI_OPS))
    {
        panic(format_args!("virtio_pci: not a virtio_pci softc"));
    }
    // SAFETY: only virtio_pci_attach installs `VIRTIO_PCI_OPS`, on the `sc_sc` member of a
    // `VirtioPciSoftc`, which is `#[repr(C)]` with that member first.
    unsafe { &*ptr::from_ref(vsc).cast::<VirtioPciSoftc>() }
}

/// `(struct virtio_pci_softc *)arg` of the interrupt handlers.
fn psc_arg<'a>(arg: *mut c_void) -> &'a VirtioPciSoftc {
    // SAFETY: the handlers are established with the softc itself as their argument
    // (virtio_pci_attach_finish), and softcs outlive their interrupts.
    psc(unsafe { &*arg.cast::<VirtioSoftc>() })
}

/// `_cread`: `size` bytes at `off` of the common configuration.
fn _cread(sc: &VirtioPciSoftc, off: usize, size: usize) -> u64 {
    let (iot, ioh) = region(&sc.sc_iot, &sc.sc_ioh);
    match size {
        1 => u64::from(bus_space_read_1(iot, ioh, off)),
        2 => u64::from(bus_space_read_2(iot, ioh, off)),
        4 => u64::from(bus_space_read_4(iot, ioh, off)),
        8 => {
            let mut val = u64::from(bus_space_read_4(iot, ioh, off + size_of::<u32>()));
            val <<= 32;
            val + u64::from(bus_space_read_4(iot, ioh, off))
        }
        _ => 0,
    }
}

/// `CWRITE`'s body: `val` as `size` bytes at `off` of the common configuration.
fn _cwrite(sc: &VirtioPciSoftc, off: usize, size: usize, val: u64, func: &str, line: u32) {
    let (iot, ioh) = region(&sc.sc_iot, &sc.sc_ioh);

    if VIRTIO_DEBUG >= 2 {
        printf(format_args!(
            "{func}: {line}: off {off:#x} size {size:#x} write {val:#x}\n"
        ));
    }
    match size {
        1 => bus_space_write_1(iot, ioh, off, val as u8),
        2 => bus_space_write_2(iot, ioh, off, val as u16),
        4 => bus_space_write_4(iot, ioh, off, val as u32),
        8 => {
            bus_space_write_4(iot, ioh, off, (val & 0xffff_ffff) as u32);
            bus_space_write_4(iot, ioh, off + size_of::<u32>(), (val >> 32) as u32);
        }
        _ => {}
    }
}

/// The size of a member of `struct virtio_pci_common_cfg`.
macro_rules! cfg_size {
    ($memb:ident) => {{
        let c = VirtioPciCommonCfg::default();
        size_of_val(&c.$memb)
    }};
}

/// `CREAD(sc, memb)`.
macro_rules! cread {
    ($sc:expr, $memb:ident) => {
        _cread($sc, offset_of!(VirtioPciCommonCfg, $memb), cfg_size!($memb))
    };
}

/// `CWRITE(sc, memb, val)`.
macro_rules! cwrite {
    ($sc:expr, $memb:ident, $val:expr) => {
        _cwrite(
            $sc,
            offset_of!(VirtioPciCommonCfg, $memb),
            cfg_size!($memb),
            $val as u64,
            module_path!(),
            line!(),
        )
    };
}

/// `virtio_pci_read_queue_size`.
pub fn virtio_pci_read_queue_size(vsc: &VirtioSoftc, idx: u16) -> u16 {
    let sc = psc(vsc);
    if sc.sc_sc.sc_version_1.get() != 0 {
        cwrite!(sc, queue_select, idx);
        cread!(sc, queue_size) as u16
    } else {
        let (iot, ioh) = region(&sc.sc_iot, &sc.sc_ioh);
        bus_space_write_2(iot, ioh, VIRTIO_CONFIG_QUEUE_SELECT, idx);
        bus_space_read_2(iot, ioh, VIRTIO_CONFIG_QUEUE_SIZE)
    }
}

/// `virtio_pci_setup_queue`.
pub fn virtio_pci_setup_queue(vsc: &VirtioSoftc, vq: &Virtqueue, addr: u64) {
    let sc = psc(vsc);
    if sc.sc_sc.sc_version_1.get() != 0 {
        cwrite!(sc, queue_select, vq.vq_index.get());
        if addr == 0 {
            cwrite!(sc, queue_enable, 0);
            cwrite!(sc, queue_desc, 0);
            cwrite!(sc, queue_avail, 0);
            cwrite!(sc, queue_used, 0);
        } else {
            cwrite!(sc, queue_desc, addr);
            cwrite!(sc, queue_avail, addr + vq.vq_availoffset.get() as u64);
            cwrite!(sc, queue_used, addr + vq.vq_usedoffset.get() as u64);
            cwrite!(sc, queue_enable, 1);
            vq.vq_notify_off.set(cread!(sc, queue_notify_off) as u32);
        }
    } else {
        let (iot, ioh) = region(&sc.sc_iot, &sc.sc_ioh);
        bus_space_write_2(
            iot,
            ioh,
            VIRTIO_CONFIG_QUEUE_SELECT,
            vq.vq_index.get() as u16,
        );
        bus_space_write_4(
            iot,
            ioh,
            VIRTIO_CONFIG_QUEUE_ADDRESS,
            (addr / VIRTIO_PAGE_SIZE as u64) as u32,
        );
    }
}

/// `virtio_pci_setup_intrs`: route each queue to its MSI-X vector.
pub fn virtio_pci_setup_intrs(vsc: &VirtioSoftc) {
    let sc = psc(vsc);

    if sc.sc_irq_type.get() == IRQ_NO_MSIX {
        return;
    }

    for (i, vq) in vsc.vqs().iter().enumerate() {
        let vec = vq.vq_intr_vec.get() as u16;
        virtio_pci_set_msix_queue_vector(sc, i as u32, vec);
    }
    if vsc.sc_config_change.get().is_some() {
        virtio_pci_set_msix_config_vector(sc, 0);
    }
}

/// `virtio_pci_get_status`.
pub fn virtio_pci_get_status(vsc: &VirtioSoftc) -> i32 {
    let sc = psc(vsc);

    if sc.sc_sc.sc_version_1.get() != 0 {
        cread!(sc, device_status) as i32
    } else {
        let (iot, ioh) = region(&sc.sc_iot, &sc.sc_ioh);
        i32::from(bus_space_read_1(iot, ioh, VIRTIO_CONFIG_DEVICE_STATUS))
    }
}

/// `virtio_pci_set_status`: or `status` into the device status; 0 resets the device and
/// waits for it.
pub fn virtio_pci_set_status(vsc: &VirtioSoftc, status: i32) {
    let sc = psc(vsc);
    let mut old = 0;

    if sc.sc_sc.sc_version_1.get() != 0 {
        if status == 0 {
            cwrite!(sc, device_status, 0);
            while cread!(sc, device_status) != 0 {
                core::hint::spin_loop(); // CPU_BUSY_CYCLE()
            }
        } else {
            old = cread!(sc, device_status) as i32;
            cwrite!(sc, device_status, status | old);
        }
    } else {
        let (iot, ioh) = region(&sc.sc_iot, &sc.sc_ioh);
        if status == 0 {
            bus_space_write_1(iot, ioh, VIRTIO_CONFIG_DEVICE_STATUS, (status | old) as u8);
            while bus_space_read_1(iot, ioh, VIRTIO_CONFIG_DEVICE_STATUS) != 0 {
                core::hint::spin_loop(); // CPU_BUSY_CYCLE()
            }
        } else {
            old = i32::from(bus_space_read_1(iot, ioh, VIRTIO_CONFIG_DEVICE_STATUS));
            bus_space_write_1(iot, ioh, VIRTIO_CONFIG_DEVICE_STATUS, (status | old) as u8);
        }
    }
}

/// `virtio_pci_match`: Qumranet's virtio 0.9 (0x1000-0x103f, revision 0) and 1.0
/// (0x1040-0x107f) functions, and OpenBSD's VMM control device.
pub fn virtio_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci hands its children pci_attach_args.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let product = pci_product(pa.pa_id);
    if pci_vendor(pa.pa_id) == PCI_VENDOR_OPENBSD && product == PCI_PRODUCT_OPENBSD_CONTROL {
        return 1;
    }
    if pci_vendor(pa.pa_id) != PCI_VENDOR_QUMRANET {
        return 0;
    }
    // virtio 0.9
    if (0x1000..=0x103f).contains(&product) && pci_revision(pa.pa_class) == 0 {
        return 1;
    }
    // virtio 1.0
    if (0x1040..=0x107f).contains(&product) {
        return 1;
    }
    0
}

/// `virtio_pci_dump_caps` (`VIRTIO_DEBUG`): every vendor-specific capability.
pub fn virtio_pci_dump_caps(sc: &VirtioPciSoftc) {
    let pc = sc.pc();
    let tag = sc.sc_ptag.get();

    let Some((mut offset, _)) = pci_get_capability(pc, tag, PCI_CAP_VENDSPEC) else {
        return;
    };

    printf(format_args!("\n"));
    loop {
        let mut reg = [0 as Pcireg; 4];
        for (i, r) in reg.iter_mut().enumerate() {
            *r = pci_conf_read(pc, tag, offset + i as i32 * 4);
        }
        let vcap = VirtioPciCap::from_regs(&reg);
        printf(format_args!(
            "virtio_pci_dump_caps: cfgoff {offset:#x} len {:#x} type {:#x} bar {:#x}: off {:#x} len {:#x}\n",
            vcap.cap_len, vcap.cfg_type, vcap.bar, vcap.offset, vcap.length
        ));
        offset = i32::from(vcap.cap_next);
        if offset == 0 {
            break;
        }
    }
}

/// `virtio_pci_find_cap`: reads the vendor-specific capability of `cfg_type` into `buf`
/// (whole registers, the capability's length rounded up); `ENOENT` when there is none,
/// `ERANGE` when `buf` is too small.
pub fn virtio_pci_find_cap(
    sc: &VirtioPciSoftc,
    cfg_type: u8,
    buf: &mut [Pcireg],
) -> Result<(), Errno> {
    let pc = sc.pc();
    let tag = sc.sc_ptag.get();

    if size_of_val(buf) < size_of::<VirtioPciCap>() {
        return Err(Errno::ERANGE);
    }

    let (mut offset, _) = pci_get_capability(pc, tag, PCI_CAP_VENDSPEC).ok_or(Errno::ENOENT)?;

    let mut vcap;
    loop {
        for (i, r) in buf.iter_mut().enumerate().take(4) {
            *r = pci_conf_read(pc, tag, offset + i as i32 * 4);
        }
        vcap = VirtioPciCap::from_regs(&[buf[0], buf[1], buf[2], buf[3]]);
        if vcap.cfg_type == cfg_type {
            break;
        }
        offset = i32::from(vcap.cap_next);
        if offset == 0 {
            break;
        }
    }

    if offset == 0 {
        return Err(Errno::ENOENT);
    }

    if usize::from(vcap.cap_len) > size_of::<VirtioPciCap>() {
        let len = usize::from(vcap.cap_len).next_multiple_of(size_of::<Pcireg>());
        if len > size_of_val(buf) {
            printf(format_args!("virtio_pci_find_cap: cap too large\n"));
            return Err(Errno::ERANGE);
        }
        for (i, r) in buf
            .iter_mut()
            .enumerate()
            .take(len / size_of::<Pcireg>())
            .skip(4)
        {
            *r = pci_conf_read(pc, tag, offset + i as i32 * 4);
        }
    }

    Ok(())
}

/// `virtio_pci_attach_10`: the virtio 1.0 layout: map the BARs the capabilities name and
/// take the common, notify, ISR and device regions out of them.
pub fn virtio_pci_attach_10(sc: &VirtioPciSoftc, pa: &PciAttachArgs) -> Result<(), Errno> {
    let xname = sc.sc_sc.sc_dev.dv_xname.get();
    let mut common = [0 as Pcireg; 4];
    let mut isr = [0 as Pcireg; 4];
    let mut device = [0 as Pcireg; 4];
    let mut notify = [0 as Pcireg; 5];
    let mut have_device_cfg = false;
    let mut bars: [BusSize; NMAPREG] = [0; NMAPREG];
    let mut bars_idx: [usize; NMAPREG] = [0; NMAPREG];
    let mut j = 0;

    virtio_pci_find_cap(sc, VIRTIO_PCI_CAP_COMMON_CFG, &mut common).map_err(|_| Errno::ENODEV)?;
    virtio_pci_find_cap(sc, VIRTIO_PCI_CAP_NOTIFY_CFG, &mut notify).map_err(|_| Errno::ENODEV)?;
    virtio_pci_find_cap(sc, VIRTIO_PCI_CAP_ISR_CFG, &mut isr).map_err(|_| Errno::ENODEV)?;
    if virtio_pci_find_cap(sc, VIRTIO_PCI_CAP_DEVICE_CFG, &mut device).is_err() {
        device = [0; 4];
    } else {
        have_device_cfg = true;
    }
    let common = VirtioPciCap::from_regs(&common);
    let isr = VirtioPciCap::from_regs(&isr);
    let device = VirtioPciCap::from_regs(&device);
    let notify_cap = VirtioPciCap::from_regs(&[notify[0], notify[1], notify[2], notify[3]]);
    let notify_off_multiplier = notify[4];

    // XXX Maybe there are devices that offer the pci caps but not the VERSION_1 feature bit?
    // XXX Then we should check the feature bit here and fall back to 0.9 out if not present.

    // Figure out which bars we need to map
    for cap in [&common, &isr, &device, &notify_cap] {
        let bar = usize::from(cap.bar);
        let len = cap.offset as BusSize + cap.length as BusSize;
        if cap.length == 0 {
            continue;
        }
        // The C indexes bars[] with the device's number unchecked.
        let Some(b) = bars.get_mut(bar) else {
            return Err(Errno::EIO);
        };
        if *b < len {
            *b = len;
        }
    }

    for (i, &maxsize) in bars.iter().enumerate() {
        if maxsize == 0 {
            continue;
        }
        let reg = PCI_MAPREG_START + i as i32 * 4;
        let type_ = pci_mapreg_type(sc.pc(), sc.sc_ptag.get(), reg);
        // The softc has four BAR slots; the C would write past them.
        let mapped = if j < sc.sc_bars_iot.len() {
            pci_mapreg_map(pa, reg, type_, 0, maxsize)
        } else {
            Err(Errno::EIO)
        };
        let Ok((iot, ioh, _, size)) = mapped else {
            printf(format_args!("{}: can't map bar {i} \n", Str(&xname)));
            // there is no pci_mapreg_unmap()
            return Err(Errno::EIO);
        };
        sc.sc_bars_iot[j].set(Some(iot));
        sc.sc_bars_ioh[j].set(Some(ioh));
        sc.sc_bars_iosize[j].set(size);
        bars_idx[i] = j;
        j += 1;
    }

    // A BAR slot's region.
    let bar_region =
        |cap: &VirtioPciCap, what: &str| -> Result<(BusSpaceTag, BusSpaceHandle), Errno> {
            let i = bars_idx[usize::from(cap.bar)];
            let (iot, ioh) = region(&sc.sc_bars_iot[i], &sc.sc_bars_ioh[i]);
            match bus_space_subregion(iot, ioh, cap.offset as BusSize, cap.length as BusSize) {
                Ok(h) => Ok((iot, h)),
                Err(_) => {
                    printf(format_args!(
                        "{}: can't map {what} i/o space\n",
                        Str(&xname)
                    ));
                    // there is no pci_mapreg_unmap()
                    Err(Errno::EIO)
                }
            }
        };

    let (iot, ioh) = bar_region(&notify_cap, "notify")?;
    sc.sc_notify_ioh.set(Some(ioh));
    sc.sc_notify_iosize.set(notify_cap.length as BusSize);
    sc.sc_notify_iot.set(Some(iot));
    sc.sc_notify_off_multiplier.set(notify_off_multiplier);

    if have_device_cfg {
        let (iot, ioh) = bar_region(&device, "devcfg")?;
        sc.sc_devcfg_ioh.set(Some(ioh));
        sc.sc_devcfg_iosize.set(device.length as BusSize);
        sc.sc_devcfg_iot.set(Some(iot));
    }

    let (iot, ioh) = bar_region(&isr, "isr")?;
    sc.sc_isr_ioh.set(Some(ioh));
    sc.sc_isr_iosize.set(isr.length as BusSize);
    sc.sc_isr_iot.set(Some(iot));

    let (iot, ioh) = bar_region(&common, "common")?;
    sc.sc_ioh.set(Some(ioh));
    sc.sc_iosize.set(common.length as BusSize);
    sc.sc_iot.set(Some(iot));

    sc.sc_sc.sc_version_1.set(1);
    Ok(())
}

/// `virtio_pci_attach_09`: the virtio 0.9 layout: everything in the I/O BAR.
pub fn virtio_pci_attach_09(sc: &VirtioPciSoftc, pa: &PciAttachArgs) -> Result<(), Errno> {
    let xname = sc.sc_sc.sc_dev.dv_xname.get();

    let type_ = pci_mapreg_type(pa.pa_pc, pa.pa_tag, PCI_MAPREG_START);
    let Ok((iot, ioh, _, size)) = pci_mapreg_map(pa, PCI_MAPREG_START, type_, 0, 0) else {
        printf(format_args!("{}: can't map i/o space\n", Str(&xname)));
        return Err(Errno::EIO);
    };
    sc.sc_iot.set(Some(iot));
    sc.sc_ioh.set(Some(ioh));
    sc.sc_iosize.set(size);

    let Ok(h) = bus_space_subregion(iot, ioh, VIRTIO_CONFIG_QUEUE_NOTIFY, 2) else {
        printf(format_args!(
            "{}: can't map notify i/o space\n",
            Str(&xname)
        ));
        return Err(Errno::EIO);
    };
    sc.sc_notify_ioh.set(Some(h));
    sc.sc_notify_iosize.set(2);
    sc.sc_notify_iot.set(Some(iot));

    let Ok(h) = bus_space_subregion(iot, ioh, VIRTIO_CONFIG_ISR_STATUS, 1) else {
        printf(format_args!("{}: can't map isr i/o space\n", Str(&xname)));
        return Err(Errno::EIO);
    };
    sc.sc_isr_ioh.set(Some(h));
    sc.sc_isr_iosize.set(1);
    sc.sc_isr_iot.set(Some(iot));

    Ok(())
}

/// `virtio_pci_attach`.
pub fn virtio_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `virtio_pci_ca`, whose softc is a `VirtioPciSoftc`.
    let sc = unsafe { self_.softc::<VirtioPciSoftc>() };
    let vsc = &sc.sc_sc;
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach and lets them change it (`pa_flags` below), as the C does.
    let pa = unsafe { &mut *aux.cast::<PciAttachArgs>() };
    let pc = pa.pa_pc;
    let tag = pa.pa_tag;
    let mut ret: Result<(), Errno> = Err(Errno::ENODEV);
    let mut vpa = VirtioPciAttachArgs {
        vpa_va: VirtioAttachArgs::default(),
        // From `aux` itself, not from `pa`, so the child's accesses do not depend on the
        // reborrow above staying unused.
        vpa_pa: aux.cast(),
        vpa_msix: 0,
    };

    let mut revision = pci_revision(pa.pa_class);
    let product = pci_product(pa.pa_id);
    let vendor = pci_vendor(pa.pa_id);
    let id = if vendor == PCI_VENDOR_OPENBSD
        || ((0x1000..=0x103f).contains(&product) && revision == 0)
    {
        // OpenBSD VMMCI and virtio 0.9
        pci_product(pci_conf_read(pc, tag, PCI_SUBSYS_ID_REG))
    } else if (0x1040..=0x107f).contains(&product) {
        // virtio 1.0
        revision = 1;
        product - 0x1040
    } else {
        printf(format_args!(
            "unknown device prod 0x{product:04x} rev 0x{revision:02x}; giving up\n"
        ));
        return;
    };

    sc.sc_pc.set(Some(pc));
    sc.sc_ptag.set(pa.pa_tag);
    vsc.sc_dmat.set(Some(pa.pa_dmat));

    if PCI_MSI_PER_BRIDGE {
        // For virtio, ignore normal MSI black/white-listing depending on the PCI bridge but
        // enable it unconditionally.
        pa.pa_flags |= PCI_FLAGS_MSI_ENABLED;
    }

    if VIRTIO_DEBUG > 0 {
        virtio_pci_dump_caps(sc);
    }

    sc.sc_nintr.set(MAX_MSIX_VECS.min(pci_intr_msix_count(pa)));
    if sc.sc_nintr.get() > 0 {
        vpa.vpa_msix = 1;
    } else {
        sc.sc_nintr.set(1);
    }
    vpa.vpa_va.va_nintr = sc.sc_nintr.get() as u32;

    let Some(intr) = mallocarray(
        sc.sc_nintr.get() as usize,
        size_of::<VirtioPciIntr>(),
        M_DEVBUF,
        M_WAITOK | M_ZERO,
    ) else {
        panic(format_args!("virtio_pci_attach: out of memory"));
    };
    sc.sc_intr.set(intr.as_ptr().cast());

    vsc.sc_ops.set(Some(&VIRTIO_PCI_OPS));
    let flags = vsc.sc_dev.cfdata().cf_flags;
    if flags & VIRTIO_CF_PREFER_VERSION_09 == 0 {
        ret = virtio_pci_attach_10(sc, pa);
    }
    if ret.is_err() && revision == 0 {
        // revision 0 means 0.9 only or both 0.9 and 1.0
        ret = virtio_pci_attach_09(sc, pa);
    }
    if ret.is_err() && flags & VIRTIO_CF_PREFER_VERSION_09 != 0 {
        ret = virtio_pci_attach_10(sc, pa);
    }

    'free: {
        if let Err(e) = ret {
            printf(format_args!(": Cannot attach ({})\n", e as i32));
            break 'free;
        }

        sc.sc_irq_type.set(IRQ_NO_MSIX);
        if virtio_pci_adjust_config_region(sc, VIRTIO_CONFIG_DEVICE_CONFIG_NOMSI) {
            break 'free;
        }

        virtio_device_reset(vsc);
        virtio_set_status(vsc, VIRTIO_CONFIG_DEVICE_STATUS_ACK);
        virtio_set_status(vsc, VIRTIO_CONFIG_DEVICE_STATUS_DRIVER);

        printf(format_args!("\n"));
        vpa.vpa_va.va_devid = id as i32;
        vsc.sc_child.set(ptr::null());
        config_found(self_, ptr::from_mut(&mut vpa).cast(), None);
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

        // err:
        // no pci_mapreg_unmap() or pci_intr_unmap()
        virtio_set_status(vsc, VIRTIO_CONFIG_DEVICE_STATUS_FAILED);
    }
    // free:
    free(
        intr,
        M_DEVBUF,
        sc.sc_nintr.get() as usize * size_of::<VirtioPciIntr>(),
    );
    sc.sc_intr.set(ptr::null_mut());
}

/// `virtio_pci_attach_finish`: the interrupts: MSI-X per queue, MSI-X shared, or one MSI or
/// INTx interrupt for everything.
pub fn virtio_pci_attach_finish(vsc: &VirtioSoftc, va: *mut VirtioAttachArgs) -> Result<(), Errno> {
    let sc = psc(vsc);
    // SAFETY: virtio_pci_attach passes its `VirtioPciAttachArgs` (which begins with the
    // `VirtioAttachArgs`) down to the child, which hands it back here.
    let vpa = unsafe { &*va.cast::<VirtioPciAttachArgs>() };
    let pc = vpa.pa().pa_pc;
    let xname = vsc.sc_dev.dv_xname.get();

    let intrstr = if sc.sc_irq_type.get() == IRQ_MSIX_CHILD {
        PciIntrStr::new(format_args!("msix"))
    } else if virtio_pci_setup_msix(sc, vpa, false).is_ok() {
        sc.sc_irq_type.set(IRQ_MSIX_PER_VQ);
        PciIntrStr::new(format_args!("msix per-VQ"))
    } else if virtio_pci_setup_msix(sc, vpa, true).is_ok() {
        sc.sc_irq_type.set(IRQ_MSIX_SHARED);
        PciIntrStr::new(format_args!("msix shared"))
    } else {
        let mut ih_func: IntrFn = virtio_pci_legacy_intr;
        let Some(ih) = pci_intr_map_msi(vpa.pa()).or_else(|| pci_intr_map(vpa.pa())) else {
            printf(format_args!("{}: couldn't map interrupt\n", Str(&xname)));
            return Err(Errno::EIO);
        };
        let intrstr = pci_intr_string(pc, ih);
        // We always set the IPL_MPSAFE flag in order to do the relatively expensive ISR
        // read without lock, and then grab the kernel lock in the interrupt handler.
        if vsc.sc_ipl.get() & IPL_MPSAFE != 0 {
            ih_func = virtio_pci_legacy_intr_mpsafe;
        }
        let name = virtio_intr_name(&vsc.child().map_or([0; 16], |c| c.dv_xname.get()));
        let ih = pci_intr_establish(
            pc,
            ih,
            vsc.sc_ipl.get() | IPL_MPSAFE,
            ih_func,
            ptr::from_ref(sc).cast_mut().cast(),
            name,
        );
        let Some(ih) = ih else {
            printf(format_args!(
                "{}: couldn't establish interrupt",
                Str(&xname)
            ));
            printf(format_args!(" at {intrstr}"));
            printf(format_args!("\n"));
            return Err(Errno::EIO);
        };
        sc.intr(0).ih.set(ih.as_ptr());
        intrstr
    };

    printf(format_args!("{}: {intrstr}\n", Str(&xname)));
    Ok(())
}

/// `virtio_pci_detach`.
pub fn virtio_pci_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `virtio_pci_ca`.
    let sc = unsafe { self_.softc::<VirtioPciSoftc>() };
    let vsc = &sc.sc_sc;

    if let Some(child) = vsc.child() {
        // SAFETY: the child is an attached device; config_detach frees it on success and
        // nothing here uses it afterwards.
        unsafe { config_detach(NonNull::from(child), flags) }?;
    }
    kassert!(vsc.sc_child.get().is_null() || vsc.sc_child.get() == VIRTIO_CHILD_ERROR);
    kassert!(vsc.sc_vqs.get().is_null());
    virtio_pci_free_irqs(sc);
    if sc.sc_iosize.get() != 0 {
        let (iot, ioh) = region(&sc.sc_iot, &sc.sc_ioh);
        bus_space_unmap(iot, ioh, sc.sc_iosize.get());
    }
    sc.sc_iosize.set(0);

    Ok(())
}

/// `virtio_pci_adjust_config_region`: with 0.9, the device configuration starts after the
/// MSI-X registers when MSI-X is on; `true` when the region cannot be mapped.
pub fn virtio_pci_adjust_config_region(sc: &VirtioPciSoftc, offset: u32) -> bool {
    if sc.sc_sc.sc_version_1.get() != 0 {
        return false;
    }
    if sc.sc_devcfg_offset.get() == offset {
        return false;
    }
    sc.sc_devcfg_offset.set(offset);
    sc.sc_devcfg_iosize
        .set(sc.sc_iosize.get() - offset as BusSize);
    sc.sc_devcfg_iot.set(sc.sc_iot.get());
    let (iot, ioh) = region(&sc.sc_iot, &sc.sc_ioh);
    match bus_space_subregion(iot, ioh, offset as BusSize, sc.sc_devcfg_iosize.get()) {
        Ok(h) => {
            sc.sc_devcfg_ioh.set(Some(h));
            false
        }
        Err(_) => {
            printf(format_args!(
                "{}: can't map config i/o space\n",
                Str(&sc.sc_sc.sc_dev.dv_xname.get())
            ));
            true
        }
    }
}

/// `virtio_pci_negotiate_features`: feature negotiation. Prints available / negotiated
/// features if `guest_feature_names` is given and `VIRTIO_DEBUG` is 1.
pub fn virtio_pci_negotiate_features(
    vsc: &VirtioSoftc,
    guest_feature_names: Option<&[VirtioFeatureName]>,
) -> Result<(), Errno> {
    let sc = psc(vsc);

    vsc.sc_active_features.set(0);
    vsc.sc_driver_features
        .set(vsc.sc_driver_features.get() | VIRTIO_F_ANY_LAYOUT);

    let my_flags = vsc.sc_dev.cfdata().cf_flags;
    let child_flags = vsc.child().map_or(0, |c| c.cfdata().cf_flags);

    // We enable indirect descriptors by default. They can be switched off by setting bit 1
    // in the driver flags, see config(8)
    if my_flags & VIRTIO_CF_NO_INDIRECT == 0 && child_flags & VIRTIO_CF_NO_INDIRECT == 0 {
        vsc.sc_driver_features
            .set(vsc.sc_driver_features.get() | VIRTIO_F_RING_INDIRECT_DESC);
    } else if guest_feature_names.is_some() {
        printf(format_args!(" RingIndirectDesc disabled by UKC"));
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

    if vsc.sc_version_1.get() != 0 {
        return virtio_pci_negotiate_features_10(vsc, guest_feature_names);
    }

    // virtio 0.9 only
    let (iot, ioh) = region(&sc.sc_iot, &sc.sc_ioh);
    let host = u64::from(bus_space_read_4(iot, ioh, VIRTIO_CONFIG_DEVICE_FEATURES));
    let negotiated = host & vsc.sc_driver_features.get();
    if VIRTIO_DEBUG > 0
        && let Some(names) = guest_feature_names
    {
        crate::dev::pv::virtio::virtio_log_features(host, negotiated, names);
    }
    bus_space_write_4(iot, ioh, VIRTIO_CONFIG_GUEST_FEATURES, negotiated as u32);
    vsc.sc_active_features.set(negotiated);
    vsc.sc_indirect
        .set(i32::from(negotiated & VIRTIO_F_RING_INDIRECT_DESC != 0));
    Ok(())
}

/// `virtio_pci_negotiate_features_10`: the 64-bit feature words and `FEATURES_OK`.
pub fn virtio_pci_negotiate_features_10(
    vsc: &VirtioSoftc,
    guest_feature_names: Option<&[VirtioFeatureName]>,
) -> Result<(), Errno> {
    let sc = psc(vsc);

    vsc.sc_driver_features
        .set(vsc.sc_driver_features.get() | VIRTIO_F_VERSION_1);
    // XXX Without this SEV doesn't work with a KVM/qemu hypervisor on
    // XXX amd64.
    vsc.sc_driver_features
        .set(vsc.sc_driver_features.get() | VIRTIO_F_ACCESS_PLATFORM);
    // notify on empty is 0.9 only
    vsc.sc_driver_features
        .set(vsc.sc_driver_features.get() & !VIRTIO_F_NOTIFY_ON_EMPTY);
    cwrite!(sc, device_feature_select, 0);
    let mut host = cread!(sc, device_feature);
    cwrite!(sc, device_feature_select, 1);
    host |= cread!(sc, device_feature) << 32;

    let negotiated = host & vsc.sc_driver_features.get();
    if VIRTIO_DEBUG > 0
        && let Some(names) = guest_feature_names
    {
        crate::dev::pv::virtio::virtio_log_features(host, negotiated, names);
    }
    cwrite!(sc, driver_feature_select, 0);
    cwrite!(sc, driver_feature, negotiated & 0xffff_ffff);
    cwrite!(sc, driver_feature_select, 1);
    cwrite!(sc, driver_feature, negotiated >> 32);
    virtio_pci_set_status(vsc, VIRTIO_CONFIG_DEVICE_STATUS_FEATURES_OK);

    if cread!(sc, device_status) as i32 & VIRTIO_CONFIG_DEVICE_STATUS_FEATURES_OK == 0 {
        printf(format_args!(
            "{}: Feature negotiation failed\n",
            Str(&vsc.sc_dev.dv_xname.get())
        ));
        cwrite!(sc, device_status, VIRTIO_CONFIG_DEVICE_STATUS_FAILED);
        return Err(Errno::ENXIO);
    }
    vsc.sc_active_features.set(negotiated);

    vsc.sc_indirect
        .set(i32::from(negotiated & VIRTIO_F_RING_INDIRECT_DESC != 0));

    if negotiated & VIRTIO_F_VERSION_1 == 0 {
        if VIRTIO_DEBUG > 0 {
            printf(format_args!(
                "virtio_pci_negotiate_features_10: Host rejected Version_1\n"
            ));
        }
        cwrite!(sc, device_status, VIRTIO_CONFIG_DEVICE_STATUS_FAILED);
        return Err(Errno::EINVAL);
    }
    Ok(())
}

// Device configuration registers.

/// The device configuration region.
fn devcfg(vsc: &VirtioSoftc) -> (BusSpaceTag, BusSpaceHandle) {
    let sc = psc(vsc);
    region(&sc.sc_devcfg_iot, &sc.sc_devcfg_ioh)
}

/// `virtio_pci_read_device_config_1`.
pub fn virtio_pci_read_device_config_1(vsc: &VirtioSoftc, index: i32) -> u8 {
    let (t, h) = devcfg(vsc);
    bus_space_read_1(t, h, index as BusSize)
}

/// `virtio_pci_read_device_config_2`.
pub fn virtio_pci_read_device_config_2(vsc: &VirtioSoftc, index: i32) -> u16 {
    let (t, h) = devcfg(vsc);
    bus_space_read_2(t, h, index as BusSize)
}

/// `virtio_pci_read_device_config_4`.
pub fn virtio_pci_read_device_config_4(vsc: &VirtioSoftc, index: i32) -> u32 {
    let (t, h) = devcfg(vsc);
    bus_space_read_4(t, h, index as BusSize)
}

/// `virtio_pci_read_device_config_8`.
pub fn virtio_pci_read_device_config_8(vsc: &VirtioSoftc, index: i32) -> u64 {
    let (t, h) = devcfg(vsc);
    let mut r = u64::from(bus_space_read_4(t, h, index as BusSize + size_of::<u32>()));
    r <<= 32;
    r + u64::from(bus_space_read_4(t, h, index as BusSize))
}

/// `virtio_pci_write_device_config_1`.
pub fn virtio_pci_write_device_config_1(vsc: &VirtioSoftc, index: i32, value: u8) {
    let (t, h) = devcfg(vsc);
    bus_space_write_1(t, h, index as BusSize, value);
}

/// `virtio_pci_write_device_config_2`.
pub fn virtio_pci_write_device_config_2(vsc: &VirtioSoftc, index: i32, value: u16) {
    let (t, h) = devcfg(vsc);
    bus_space_write_2(t, h, index as BusSize, value);
}

/// `virtio_pci_write_device_config_4`.
pub fn virtio_pci_write_device_config_4(vsc: &VirtioSoftc, index: i32, value: u32) {
    let (t, h) = devcfg(vsc);
    bus_space_write_4(t, h, index as BusSize, value);
}

/// `virtio_pci_write_device_config_8`.
pub fn virtio_pci_write_device_config_8(vsc: &VirtioSoftc, index: i32, value: u64) {
    let (t, h) = devcfg(vsc);
    bus_space_write_4(t, h, index as BusSize, (value & 0xffff_ffff) as u32);
    bus_space_write_4(
        t,
        h,
        index as BusSize + size_of::<u32>(),
        (value >> 32) as u32,
    );
}

/// `virtio_pci_msix_establish`: MSI-X vector `idx` for `handler(ih_arg)`.
pub fn virtio_pci_msix_establish(
    sc: &VirtioPciSoftc,
    vpa: &VirtioPciAttachArgs,
    idx: i32,
    ci: Option<&'static CpuInfo>,
    handler: IntrFn,
    ih_arg: *mut c_void,
) -> Result<(), Errno> {
    let vsc = &sc.sc_sc;

    kassert!(idx < sc.sc_nintr.get());

    if vpa.vpa_msix == 0 {
        return Err(Errno::ENXIO);
    }

    let Some(ih) = pci_intr_map_msix(vpa.pa(), idx) else {
        if VIRTIO_DEBUG > 0 {
            printf(format_args!(
                "{}[{idx}]: pci_intr_map_msix failed\n",
                Str(&vsc.sc_dev.dv_xname.get())
            ));
        }
        return Err(Errno::EPERM);
    };
    let child = vsc.child().map_or([0; 16], |c| c.dv_xname.get());
    let mut name = [0u8; 16];
    snprintf(&mut name, format_args!("{}:{idx}", Str(&child)));
    let intr = sc.intr(idx);
    intr.name.set(name);
    let ih = pci_intr_establish_cpu(
        sc.pc(),
        ih,
        vsc.sc_ipl.get(),
        ci,
        handler,
        ih_arg,
        virtio_intr_name(&name),
    );
    let Some(ih) = ih else {
        printf(format_args!(
            "{}[{idx}]: couldn't establish msix interrupt\n",
            Str(&child)
        ));
        return Err(Errno::ENOMEM);
    };
    intr.ih.set(ih.as_ptr());
    virtio_pci_adjust_config_region(sc, VIRTIO_CONFIG_DEVICE_CONFIG_MSI);
    Ok(())
}

/// `virtio_pci_set_msix_queue_vector`.
pub fn virtio_pci_set_msix_queue_vector(sc: &VirtioPciSoftc, idx: u32, vector: u16) {
    if sc.sc_sc.sc_version_1.get() != 0 {
        cwrite!(sc, queue_select, idx);
        cwrite!(sc, queue_msix_vector, vector);
    } else {
        let (iot, ioh) = region(&sc.sc_iot, &sc.sc_ioh);
        bus_space_write_2(iot, ioh, VIRTIO_CONFIG_QUEUE_SELECT, idx as u16);
        bus_space_write_2(iot, ioh, VIRTIO_MSI_QUEUE_VECTOR, vector);
    }
}

/// `virtio_pci_set_msix_config_vector`.
pub fn virtio_pci_set_msix_config_vector(sc: &VirtioPciSoftc, vector: u16) {
    if sc.sc_sc.sc_version_1.get() != 0 {
        cwrite!(sc, config_msix_vector, vector);
    } else {
        let (iot, ioh) = region(&sc.sc_iot, &sc.sc_ioh);
        bus_space_write_2(iot, ioh, VIRTIO_MSI_CONFIG_VECTOR, vector);
    }
}

/// `virtio_pci_free_irqs`.
pub fn virtio_pci_free_irqs(sc: &VirtioPciSoftc) {
    let vsc = &sc.sc_sc;

    if sc.sc_devcfg_offset.get() == VIRTIO_CONFIG_DEVICE_CONFIG_MSI {
        for i in 0..vsc.vqs().len() {
            virtio_pci_set_msix_queue_vector(sc, i as u32, VIRTIO_MSI_NO_VECTOR);
        }
    }

    if !sc.sc_intr.get().is_null() {
        for i in 0..sc.sc_nintr.get() {
            let intr = sc.intr(i);
            if let Some(ih) = NonNull::new(intr.ih.get()) {
                // SAFETY: the handle came from pci_intr_establish* and is dropped here.
                unsafe { pci_intr_disestablish(sc.pc(), ih) };
                intr.ih.set(ptr::null_mut());
            }
        }
    }

    // XXX msix_delroute does not unset PCI_MSIX_MC_MSIXE -> leave alone?
    virtio_pci_adjust_config_region(sc, VIRTIO_CONFIG_DEVICE_CONFIG_NOMSI);
}

/// `virtio_pci_setup_msix`: vector 0 for configuration changes, then one vector per queue
/// or, `shared`, one for all of them.
pub fn virtio_pci_setup_msix(
    sc: &VirtioPciSoftc,
    vpa: &VirtioPciAttachArgs,
    shared: bool,
) -> Result<(), Errno> {
    let vsc = &sc.sc_sc;
    let nvqs = vsc.vqs().len() as u32;

    // Shared needs config + queue
    if shared && vpa.vpa_va.va_nintr < 1 + 1 {
        return Err(Errno::ERANGE);
    }
    // Per VQ needs config + N * queue
    if !shared && vpa.vpa_va.va_nintr < 1 + nvqs {
        return Err(Errno::ERANGE);
    }

    let vsc_arg: *mut c_void = ptr::from_ref(vsc).cast_mut().cast();
    virtio_pci_msix_establish(sc, vpa, 0, None, virtio_pci_config_intr, vsc_arg)?;

    let r = if shared {
        virtio_pci_msix_establish(sc, vpa, 1, None, virtio_pci_shared_queue_intr, vsc_arg).map(
            |()| {
                for vq in vsc.vqs() {
                    vq.vq_intr_vec.set(1);
                }
            },
        )
    } else {
        vsc.vqs().iter().enumerate().try_for_each(|(i, vq)| {
            virtio_pci_msix_establish(
                sc,
                vpa,
                i as i32 + 1,
                None,
                virtio_pci_queue_intr,
                ptr::from_ref(vq).cast_mut().cast(),
            )?;
            vq.vq_intr_vec.set(i as i32 + 1);
            Ok(())
        })
    };

    if r.is_err() {
        // fail:
        virtio_pci_free_irqs(sc);
    }
    r
}

/// `virtio_pci_intr_establish`: a child's own MSI-X vector `vec`.
pub fn virtio_pci_intr_establish(
    vsc: &VirtioSoftc,
    va: *mut VirtioAttachArgs,
    vec: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
) -> Result<(), Errno> {
    if !vsc
        .sc_ops
        .get()
        .is_some_and(|o| ptr::eq(o, &VIRTIO_PCI_OPS))
    {
        return Err(Errno::ENXIO);
    }

    // SAFETY: as in virtio_pci_attach_finish.
    let vpa = unsafe { &*va.cast::<VirtioPciAttachArgs>() };
    let sc = psc(vsc);

    if vec >= sc.sc_nintr.get() || sc.sc_nintr.get() <= 1 {
        return Err(Errno::ERANGE);
    }

    sc.sc_irq_type.set(IRQ_MSIX_CHILD);
    virtio_pci_msix_establish(sc, vpa, vec, ci, func, arg)
}

/// `virtio_pci_intr_barrier`.
pub fn virtio_pci_intr_barrier(vsc: &VirtioSoftc) {
    let sc = psc(vsc);

    if sc.sc_intr.get().is_null() {
        return;
    }
    for i in 0..sc.sc_nintr.get() {
        if let Some(ih) = NonNull::new(sc.intr(i).ih.get()) {
            intr_barrier(ih);
        }
    }
}

// Interrupt handler.

/// `virtio_pci_legacy_intr`: only used without MSI-X.
pub fn virtio_pci_legacy_intr(arg: *mut c_void) -> i32 {
    let sc = psc_arg(arg);
    let vsc = &sc.sc_sc;
    let mut r = 0;

    // check and ack the interrupt
    let (iot, ioh) = region(&sc.sc_isr_iot, &sc.sc_isr_ioh);
    let isr = bus_space_read_1(iot, ioh, 0);
    if isr == 0 {
        return 0;
    }
    kernel_lock();
    if isr & VIRTIO_CONFIG_ISR_CONFIG_CHANGE != 0
        && let Some(config_change) = vsc.sc_config_change.get()
    {
        r = config_change(vsc);
    }
    r |= virtio_check_vqs(vsc);
    kernel_unlock();

    r
}

/// `virtio_pci_legacy_intr_mpsafe`.
pub fn virtio_pci_legacy_intr_mpsafe(arg: *mut c_void) -> i32 {
    let sc = psc_arg(arg);
    let vsc = &sc.sc_sc;
    let mut r = 0;

    // check and ack the interrupt
    let (iot, ioh) = region(&sc.sc_isr_iot, &sc.sc_isr_ioh);
    let isr = bus_space_read_1(iot, ioh, 0);
    if isr == 0 {
        return 0;
    }
    if isr & VIRTIO_CONFIG_ISR_CONFIG_CHANGE != 0
        && let Some(config_change) = vsc.sc_config_change.get()
    {
        r = config_change(vsc);
    }
    r |= virtio_check_vqs(vsc);
    r
}

/// `virtio_pci_config_intr`: only used with MSI-X.
pub fn virtio_pci_config_intr(arg: *mut c_void) -> i32 {
    // SAFETY: established with the virtio softc as its argument (virtio_pci_setup_msix).
    let vsc = unsafe { &*arg.cast::<VirtioSoftc>() };

    match vsc.sc_config_change.get() {
        Some(config_change) => config_change(vsc),
        None => 0,
    }
}

/// `virtio_pci_queue_intr`: only used with MSI-X.
pub fn virtio_pci_queue_intr(arg: *mut c_void) -> i32 {
    // SAFETY: established with one of the softc's queues as its argument
    // (virtio_pci_setup_msix); the queues outlive their interrupts.
    let vq = unsafe { &*arg.cast::<Virtqueue>() };
    let vsc = vq.owner();

    virtio_check_vq(vsc, vq)
}

/// `virtio_pci_shared_queue_intr`.
pub fn virtio_pci_shared_queue_intr(arg: *mut c_void) -> i32 {
    // SAFETY: established with the virtio softc as its argument (virtio_pci_setup_msix).
    let vsc = unsafe { &*arg.cast::<VirtioSoftc>() };

    virtio_check_vqs(vsc)
}

/// `virtio_pci_poll_intr`: interrupt handler to be used when polling. We cannot use isr
/// here because it is not defined in MSI-X mode.
pub fn virtio_pci_poll_intr(arg: *mut c_void) -> i32 {
    let sc = psc_arg(arg);
    let vsc = &sc.sc_sc;
    let mut r = 0;

    if let Some(config_change) = vsc.sc_config_change.get() {
        r = config_change(vsc);
    }

    r |= virtio_check_vqs(vsc);

    r
}

/// `virtio_pci_kick`: notify the device of new buffers in queue `idx`.
pub fn virtio_pci_kick(vsc: &VirtioSoftc, idx: u16) {
    let sc = psc(vsc);
    let mut offset = 0;
    if vsc.sc_version_1.get() != 0 {
        offset = vsc.vq(usize::from(idx)).vq_notify_off.get() as BusSize
            * sc.sc_notify_off_multiplier.get() as BusSize;
    }
    let (iot, ioh) = region(&sc.sc_notify_iot, &sc.sc_notify_ioh);
    bus_space_write_2(iot, ioh, offset, idx);
}
/* </CODE> */
