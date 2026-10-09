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
 * Copyright (c) 2013 David Gwynne <dlg@openbsd.org>
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
//! `vmwpvs(4)`: the VMware paravirtual SCSI host adapter (`vmwpvs* at pci?`, amd64).
//!
//! Upstream: sys/dev/pci/vmwpvs.c @ 3ce1f3f79392
//!
//! The adapter is driven through rings in host memory: a request ring the driver produces
//! into, a completion ring and (when the device offers it) a message ring the device
//! produces into, and a shared page of producer and consumer indices (`struct
//! vmwpvw_ring_state`). Commands to the adapter itself (set up the rings, read a
//! configuration page) are written word by word to the command data register. Each opening
//! (`struct vmwpvs_ccb`) owns a DMA map, a scatter/gather list and a sense buffer, both
//! carved out of one DMA area each; the free openings are the adapter's `scsi_iopool`.
//! Completions are matched to their opening by the 64-bit context the request carried (the
//! opening's index in its low half). Hot-plug messages probe or detach a target from the
//! system task queue.
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the device first and reached as `&'static`; its members
//!   are `Cell`s, all-zero valid. `sc_tag` is not kept (only the attach reads it, from the
//!   `pci_attach_args` it has); `sc_pc` is an `Option`.
//! - The ring and command structures are `#[repr(C)]` with the C's `__packed` layout (no
//!   member is misaligned in it, so `repr(C)` adds no padding; the sizes are checked at
//!   compile time). The ring state, the rings, the scatter/gather lists and the sense
//!   buffers live in DMA memory the device reads and writes, so they are read and written
//!   volatile through raw pointers into the areas (`vmwpvs_dma_kva`).
//! - `vmwpvs_cmd` takes the command's bytes and writes them as native-endian 32-bit words,
//!   as the C's `u_int32_t *` walk does.
//! - The openings array keeps its length (`sc_nccbs`): a completion whose context names no
//!   opening panics, where the C would index past the array. The opening's `ccb_xs`,
//!   `ccb_dmamap`, `ccb_sgl` and `ccb_sense` are `Cell`s of `Option`s or raw pointers into
//!   the DMA areas.
//! - A command descriptor block longer than the request's 16 bytes is cut to 16 (the C's
//!   `memcpy` would run into the next members); the SCSI midlayer sends at most 16 here.
//! - The residual of an underrun is the low 32 bits of `datalen - data_len`, as the C's
//!   `int` keeps them.
//! - No stubs: every function of the file is ported.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};

use crate::dev::pci::pci_map::{pci_mapreg_map, pci_mapreg_type};
use crate::dev::pci::pcidevs::{PCI_PRODUCT_VMWARE_PVSCSI, PCI_VENDOR_VMWARE};
use crate::dev::pci::pcireg::{
    PCI_MAPREG_END, PCI_MAPREG_START, PCI_MAPREG_TYPE_MASK, PCI_MAPREG_TYPE_MEM, pci_product,
    pci_vendor,
};
use crate::dev::pci::pcivar::PciAttachArgs;
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_task::{SYSTQMP, task_add, task_set};
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_ALLOCNOW, BUS_DMA_NOWAIT, BUS_DMA_WAITOK, BUS_DMA_ZERO, BUS_DMASYNC_POSTREAD,
    BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE, BusDmaSegment, BusDmaTag,
    BusDmamap, BusSize, BusSpaceHandle, BusSpaceTag, bus_dmamap_create, bus_dmamap_destroy,
    bus_dmamap_load, bus_dmamap_sync, bus_dmamap_unload, bus_dmamem_alloc, bus_dmamem_free,
    bus_dmamem_map, bus_dmamem_unmap, bus_space_read_4, bus_space_unmap, bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{IPL_BIO, IPL_MPSAFE};
use crate::machine::pci_machdep::{
    PciChipsetTag, PciIntrFn, pci_intr_establish, pci_intr_map, pci_intr_map_msi, pci_intr_string,
};
use crate::queue_adapter;
use crate::scsi::scsi_all::{ScsiSenseData, ScsiWire};
use crate::scsi::scsi_base::{scsi_done, scsi_iopool_init};
use crate::scsi::scsi_message::MSG_SIMPLE_Q_TAG;
use crate::scsi::scsiconf::{
    SCSI_DATA_IN, SCSI_DATA_OUT, SCSI_NOSLEEP, SCSI_POLL, SDEV_NO_ADAPTER_TARGET, ScsiAdapter,
    ScsiIo, ScsiIopool, ScsiXfer, ScsibusAttachArgs, ScsibusSoftc, XS_DRIVER_STUFFUP, XS_NOERROR,
    XS_SELTIMEOUT, XS_SENSE, scsi_detach_lun, scsi_probe_lun, scsiprint,
};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DETACH_FORCE, DV_DULL, Device, Softc};
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::mutex::Mutex;
use crate::sys::param::{MAXPHYS, PAGE_SIZE};
use crate::sys::queue::{SimpleqEntry, SimpleqHead};
use crate::sys::systm::{kernel_lock, kernel_unlock};
use crate::sys::task::Task;

/* pushbuttons */
/// `VMWPVS_OPENINGS`: according to the linux driver.
pub const VMWPVS_OPENINGS: u16 = 64;
/// `VMWPVS_RING_PAGES`.
pub const VMWPVS_RING_PAGES: usize = 2;
/// `VMWPVS_MAXSGL`.
pub const VMWPVS_MAXSGL: usize = MAXPHYS / PAGE_SIZE;
/// `VMWPVS_SENSELEN`: `roundup(sizeof(struct scsi_sense_data), 16)`.
pub const VMWPVS_SENSELEN: usize = size_of::<ScsiSenseData>().div_ceil(16) * 16;

/* "chip" definitions */

/// `VMWPVS_R_COMMAND`.
pub const VMWPVS_R_COMMAND: BusSize = 0x0000;
/// `VMWPVS_R_COMMAND_DATA`.
pub const VMWPVS_R_COMMAND_DATA: BusSize = 0x0004;
/// `VMWPVS_R_COMMAND_STATUS`.
pub const VMWPVS_R_COMMAND_STATUS: BusSize = 0x0008;
/// `VMWPVS_R_LAST_STS_0`.
pub const VMWPVS_R_LAST_STS_0: BusSize = 0x0100;
/// `VMWPVS_R_LAST_STS_1`.
pub const VMWPVS_R_LAST_STS_1: BusSize = 0x0104;
/// `VMWPVS_R_LAST_STS_2`.
pub const VMWPVS_R_LAST_STS_2: BusSize = 0x0108;
/// `VMWPVS_R_LAST_STS_3`.
pub const VMWPVS_R_LAST_STS_3: BusSize = 0x010c;
/// `VMWPVS_R_INTR_STATUS`.
pub const VMWPVS_R_INTR_STATUS: BusSize = 0x100c;
/// `VMWPVS_R_INTR_MASK`.
pub const VMWPVS_R_INTR_MASK: BusSize = 0x2010;
/// `VMWPVS_R_KICK_NON_RW_IO`.
pub const VMWPVS_R_KICK_NON_RW_IO: BusSize = 0x3014;
/// `VMWPVS_R_DEBUG`.
pub const VMWPVS_R_DEBUG: BusSize = 0x3018;
/// `VMWPVS_R_KICK_RW_IO`.
pub const VMWPVS_R_KICK_RW_IO: BusSize = 0x4018;

/// `VMWPVS_INTR_CMPL_0`.
pub const VMWPVS_INTR_CMPL_0: u32 = 1 << 0;
/// `VMWPVS_INTR_CMPL_1`.
pub const VMWPVS_INTR_CMPL_1: u32 = 1 << 1;
/// `VMWPVS_INTR_CMPL_MASK`.
pub const VMWPVS_INTR_CMPL_MASK: u32 = VMWPVS_INTR_CMPL_0 | VMWPVS_INTR_CMPL_1;
/// `VMWPVS_INTR_MSG_0`.
pub const VMWPVS_INTR_MSG_0: u32 = 1 << 2;
/// `VMWPVS_INTR_MSG_1`.
pub const VMWPVS_INTR_MSG_1: u32 = 1 << 3;
/// `VMWPVS_INTR_MSG_MASK`.
pub const VMWPVS_INTR_MSG_MASK: u32 = VMWPVS_INTR_MSG_0 | VMWPVS_INTR_MSG_1;
/// `VMWPVS_INTR_ALL_MASK`.
pub const VMWPVS_INTR_ALL_MASK: u32 = VMWPVS_INTR_CMPL_MASK | VMWPVS_INTR_MSG_MASK;

/// `VMWPVS_PAGE_SHIFT`.
pub const VMWPVS_PAGE_SHIFT: u32 = 12;
/// `VMWPVS_PAGE_SIZE`.
pub const VMWPVS_PAGE_SIZE: usize = 1 << VMWPVS_PAGE_SHIFT;

/// `VMWPVS_NPG_COMMAND`.
pub const VMWPVS_NPG_COMMAND: usize = 1;
/// `VMWPVS_NPG_INTR_STATUS`.
pub const VMWPVS_NPG_INTR_STATUS: usize = 1;
/// `VMWPVS_NPG_MISC`.
pub const VMWPVS_NPG_MISC: usize = 2;
/// `VMWPVS_NPG_KICK_IO`.
pub const VMWPVS_NPG_KICK_IO: usize = 2;
/// `VMWPVS_NPG_MSI_X`.
pub const VMWPVS_NPG_MSI_X: usize = 2;

/// `VMWPVS_PG_COMMAND`.
pub const VMWPVS_PG_COMMAND: usize = 0;
/// `VMWPVS_PG_INTR_STATUS`.
pub const VMWPVS_PG_INTR_STATUS: usize = VMWPVS_PG_COMMAND + VMWPVS_NPG_COMMAND * VMWPVS_PAGE_SIZE;
/// `VMWPVS_PG_MISC`.
pub const VMWPVS_PG_MISC: usize = VMWPVS_PG_INTR_STATUS + VMWPVS_NPG_INTR_STATUS * VMWPVS_PAGE_SIZE;
/// `VMWPVS_PG_KICK_IO`.
pub const VMWPVS_PG_KICK_IO: usize = VMWPVS_PG_MISC + VMWPVS_NPG_MISC * VMWPVS_PAGE_SIZE;
/// `VMWPVS_PG_MSI_X`.
pub const VMWPVS_PG_MSI_X: usize = VMWPVS_PG_KICK_IO + VMWPVS_NPG_KICK_IO * VMWPVS_PAGE_SIZE;
/// `VMMPVS_PG_LEN`: the register space mapped.
pub const VMMPVS_PG_LEN: usize = VMWPVS_PG_MSI_X + VMWPVS_NPG_MSI_X * VMWPVS_PAGE_SIZE;

/// `VMWPVS_REQ_SGL`.
pub const VMWPVS_REQ_SGL: u32 = 1 << 0;
/// `VMWPVS_REQ_OOBCDB`.
pub const VMWPVS_REQ_OOBCDB: u32 = 1 << 1;
/// `VMWPVS_REQ_DIR_NONE`.
pub const VMWPVS_REQ_DIR_NONE: u32 = 1 << 2;
/// `VMWPVS_REQ_DIR_IN`.
pub const VMWPVS_REQ_DIR_IN: u32 = 1 << 3;
/// `VMWPVS_REQ_DIR_OUT`.
pub const VMWPVS_REQ_DIR_OUT: u32 = 1 << 4;

/// `VMWPVS_MSG_T_ADDED`.
pub const VMWPVS_MSG_T_ADDED: u32 = 0;
/// `VMWPVS_MSG_T_REMOVED`.
pub const VMWPVS_MSG_T_REMOVED: u32 = 1;

/// `VMWPVS_MAX_RING_PAGES`.
pub const VMWPVS_MAX_RING_PAGES: usize = 32;
/// `VMWPVS_MAX_MSG_RING_PAGES`.
pub const VMWPVS_MAX_MSG_RING_PAGES: usize = 16;

/// `VMWPVS_CMD_FIRST`.
pub const VMWPVS_CMD_FIRST: u32 = 0;
/// `VMWPVS_CMD_ADAPTER_RESET`.
pub const VMWPVS_CMD_ADAPTER_RESET: u32 = 1;
/// `VMWPVS_CMD_ISSUE_SCSI`.
pub const VMWPVS_CMD_ISSUE_SCSI: u32 = 2;
/// `VMWPVS_CMD_SETUP_RINGS`.
pub const VMWPVS_CMD_SETUP_RINGS: u32 = 3;
/// `VMWPVS_CMD_RESET_BUS`.
pub const VMWPVS_CMD_RESET_BUS: u32 = 4;
/// `VMWPVS_CMD_RESET_DEVICE`.
pub const VMWPVS_CMD_RESET_DEVICE: u32 = 5;
/// `VMWPVS_CMD_ABORT_CMD`.
pub const VMWPVS_CMD_ABORT_CMD: u32 = 6;
/// `VMWPVS_CMD_CONFIG`.
pub const VMWPVS_CMD_CONFIG: u32 = 7;
/// `VMWPVS_CMD_SETUP_MSG_RING`.
pub const VMWPVS_CMD_SETUP_MSG_RING: u32 = 8;
/// `VMWPVS_CMD_DEVICE_UNPLUG`.
pub const VMWPVS_CMD_DEVICE_UNPLUG: u32 = 9;
/// `VMWPVS_CMD_LAST`.
pub const VMWPVS_CMD_LAST: u32 = 10;

/// `VMWPVS_CFGPG_CONTROLLER`.
pub const VMWPVS_CFGPG_CONTROLLER: u32 = 0x1958;
/// `VMWPVS_CFGPG_PHY`.
pub const VMWPVS_CFGPG_PHY: u32 = 0x1959;
/// `VMWPVS_CFGPG_DEVICE`.
pub const VMWPVS_CFGPG_DEVICE: u32 = 0x195a;

/// `VMWPVS_CFGPGADDR_CONTROLLER`.
pub const VMWPVS_CFGPGADDR_CONTROLLER: u32 = 0x2120;
/// `VMWPVS_CFGPGADDR_TARGET`.
pub const VMWPVS_CFGPGADDR_TARGET: u32 = 0x2121;
/// `VMWPVS_CFGPGADDR_PHY`.
pub const VMWPVS_CFGPGADDR_PHY: u32 = 0x2122;

/// `VMWPVS_HOST_STATUS_SUCCESS`.
pub const VMWPVS_HOST_STATUS_SUCCESS: u16 = 0x00;
/// `VMWPVS_HOST_STATUS_LINKED_CMD_COMPLETED`.
pub const VMWPVS_HOST_STATUS_LINKED_CMD_COMPLETED: u16 = 0x0a;
/// `VMWPVS_HOST_STATUS_LINKED_CMD_COMPLETED_WITH_FLAG`.
pub const VMWPVS_HOST_STATUS_LINKED_CMD_COMPLETED_WITH_FLAG: u16 = 0x0b;
/// `VMWPVS_HOST_STATUS_UNDERRUN`.
pub const VMWPVS_HOST_STATUS_UNDERRUN: u16 = 0x0c;
/// `VMWPVS_HOST_STATUS_SELTIMEOUT`.
pub const VMWPVS_HOST_STATUS_SELTIMEOUT: u16 = 0x11;
/// `VMWPVS_HOST_STATUS_DATARUN`.
pub const VMWPVS_HOST_STATUS_DATARUN: u16 = 0x12;
/// `VMWPVS_HOST_STATUS_BUSFREE`.
pub const VMWPVS_HOST_STATUS_BUSFREE: u16 = 0x13;
/// `VMWPVS_HOST_STATUS_INVPHASE`.
pub const VMWPVS_HOST_STATUS_INVPHASE: u16 = 0x14;
/// `VMWPVS_HOST_STATUS_LUNMISMATCH`.
pub const VMWPVS_HOST_STATUS_LUNMISMATCH: u16 = 0x17;
/// `VMWPVS_HOST_STATUS_INVPARAM`.
pub const VMWPVS_HOST_STATUS_INVPARAM: u16 = 0x1a;
/// `VMWPVS_HOST_STATUS_SENSEFAILED`.
pub const VMWPVS_HOST_STATUS_SENSEFAILED: u16 = 0x1b;
/// `VMWPVS_HOST_STATUS_TAGREJECT`.
pub const VMWPVS_HOST_STATUS_TAGREJECT: u16 = 0x1c;
/// `VMWPVS_HOST_STATUS_BADMSG`.
pub const VMWPVS_HOST_STATUS_BADMSG: u16 = 0x1d;
/// `VMWPVS_HOST_STATUS_HAHARDWARE`.
pub const VMWPVS_HOST_STATUS_HAHARDWARE: u16 = 0x20;
/// `VMWPVS_HOST_STATUS_NORESPONSE`.
pub const VMWPVS_HOST_STATUS_NORESPONSE: u16 = 0x21;
/// `VMWPVS_HOST_STATUS_SENT_RST`.
pub const VMWPVS_HOST_STATUS_SENT_RST: u16 = 0x22;
/// `VMWPVS_HOST_STATUS_RECV_RST`.
pub const VMWPVS_HOST_STATUS_RECV_RST: u16 = 0x23;
/// `VMWPVS_HOST_STATUS_DISCONNECT`.
pub const VMWPVS_HOST_STATUS_DISCONNECT: u16 = 0x24;
/// `VMWPVS_HOST_STATUS_BUS_RESET`.
pub const VMWPVS_HOST_STATUS_BUS_RESET: u16 = 0x25;
/// `VMWPVS_HOST_STATUS_ABORT_QUEUE`.
pub const VMWPVS_HOST_STATUS_ABORT_QUEUE: u16 = 0x26;
/// `VMWPVS_HOST_STATUS_HA_SOFTWARE`.
pub const VMWPVS_HOST_STATUS_HA_SOFTWARE: u16 = 0x27;
/// `VMWPVS_HOST_STATUS_HA_TIMEOUT`.
pub const VMWPVS_HOST_STATUS_HA_TIMEOUT: u16 = 0x30;
/// `VMWPVS_HOST_STATUS_SCSI_PARITY`.
pub const VMWPVS_HOST_STATUS_SCSI_PARITY: u16 = 0x34;

/// `VMWPVS_SCSI_STATUS_OK`.
pub const VMWPVS_SCSI_STATUS_OK: u16 = 0x00;
/// `VMWPVS_SCSI_STATUS_CHECK`.
pub const VMWPVS_SCSI_STATUS_CHECK: u16 = 0x02;

/// A structure the device reads or writes, or that `vmwpvs_cmd` sends word by word.
///
/// # Safety
///
/// The type is `#[repr(C)]` without padding, every bit pattern is a valid value, and its
/// size is a multiple of 4 bytes.
unsafe trait VmwpvsWire: Copy {
    /// `memset(&x, 0, sizeof(x))`.
    fn zeroed() -> Self {
        // SAFETY: every bit pattern is valid (the trait's contract).
        unsafe { core::mem::zeroed() }
    }

    /// The structure's bytes.
    fn as_bytes(&self) -> &[u8] {
        // SAFETY: no padding (the trait's contract), so every byte is initialised; the slice
        // borrows `self`.
        unsafe { core::slice::from_raw_parts(ptr::from_ref(self).cast::<u8>(), size_of::<Self>()) }
    }
}

/// `struct vmwpvw_ring_state`: the producer and consumer indices both sides share.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmwpvwRingState {
    /// `req_prod`.
    pub req_prod: u32,
    /// `req_cons`.
    pub req_cons: u32,
    /// `req_entries`: log 2.
    pub req_entries: u32,
    /// `cmp_prod`.
    pub cmp_prod: u32,
    /// `cmp_cons`.
    pub cmp_cons: u32,
    /// `cmp_entries`: log 2.
    pub cmp_entries: u32,
    /// `__reserved`.
    pub __reserved: [u32; 26],
    /// `msg_prod`.
    pub msg_prod: u32,
    /// `msg_cons`.
    pub msg_cons: u32,
    /// `msg_entries`: log 2.
    pub msg_entries: u32,
}

// SAFETY: `#[repr(C)]`, only `u32`s (no padding, any bits), 140 bytes.
unsafe impl VmwpvsWire for VmwpvwRingState {}

/// `struct vmwpvs_ring_req`: a request ring entry.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmwpvsRingReq {
    /// `context`.
    pub context: u64,
    /// `data_addr`.
    pub data_addr: u64,
    /// `data_len`.
    pub data_len: u64,
    /// `sense_addr`.
    pub sense_addr: u64,
    /// `sense_len`.
    pub sense_len: u32,
    /// `flags`: `VMWPVS_REQ_*`.
    pub flags: u32,
    /// `cdb`.
    pub cdb: [u8; 16],
    /// `cdblen`.
    pub cdblen: u8,
    /// `lun`.
    pub lun: [u8; 8],
    /// `tag`.
    pub tag: u8,
    /// `bus`.
    pub bus: u8,
    /// `target`.
    pub target: u8,
    /// `vcpu_hint`.
    pub vcpu_hint: u8,
    /// `__reserved`.
    pub __reserved: [u8; 59],
}

// SAFETY: `#[repr(C)]`; the integers and byte arrays leave no padding (128 bytes, checked
// below); any bits.
unsafe impl VmwpvsWire for VmwpvsRingReq {}

/// `VMWPVS_REQ_COUNT`.
pub const VMWPVS_REQ_COUNT: usize =
    (VMWPVS_RING_PAGES * VMWPVS_PAGE_SIZE) / size_of::<VmwpvsRingReq>();

/// `struct vmwpvs_ring_cmp`: a completion ring entry.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmwpvsRingCmp {
    /// `context`: the request's.
    pub context: u64,
    /// `data_len`: the bytes moved.
    pub data_len: u64,
    /// `sense_len`.
    pub sense_len: u32,
    /// `host_status`: `VMWPVS_HOST_STATUS_*`.
    pub host_status: u16,
    /// `scsi_status`.
    pub scsi_status: u16,
    /// `__reserved`.
    pub __reserved: [u32; 2],
}

// SAFETY: `#[repr(C)]`, 32 bytes without padding (checked below); any bits.
unsafe impl VmwpvsWire for VmwpvsRingCmp {}

/// `VMWPVS_CMP_COUNT`.
pub const VMWPVS_CMP_COUNT: usize =
    (VMWPVS_RING_PAGES * VMWPVS_PAGE_SIZE) / size_of::<VmwpvsRingCmp>();

/// `struct vmwpvs_sge`: a scatter/gather element.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmwpvsSge {
    /// `addr`.
    pub addr: u64,
    /// `len`.
    pub len: u32,
    /// `flags`.
    pub flags: u32,
}

// SAFETY: `#[repr(C)]`, 16 bytes without padding; any bits.
unsafe impl VmwpvsWire for VmwpvsSge {}

/// `struct vmwpvs_ring_msg`: a message ring entry.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmwpvsRingMsg {
    /// `type`: `VMWPVS_MSG_T_*`.
    pub type_: u32,
    /// `__args`.
    pub __args: [u32; 31],
}

// SAFETY: `#[repr(C)]`, only `u32`s; any bits.
unsafe impl VmwpvsWire for VmwpvsRingMsg {}

/// `VMWPVS_MSG_COUNT`.
pub const VMWPVS_MSG_COUNT: usize =
    (VMWPVS_RING_PAGES * VMWPVS_PAGE_SIZE) / size_of::<VmwpvsRingMsg>();

/// `struct vmwpvs_ring_msg_dev`: a device added or removed.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmwpvsRingMsgDev {
    /// `type`.
    pub type_: u32,
    /// `bus`.
    pub bus: u32,
    /// `target`.
    pub target: u32,
    /// `lun`.
    pub lun: [u8; 8],
    /// `__pad`.
    pub __pad: [u32; 27],
}

// SAFETY: `#[repr(C)]`; the `lun` bytes end on a 4-byte boundary, so no padding (128 bytes,
// checked below); any bits.
unsafe impl VmwpvsWire for VmwpvsRingMsgDev {}

/// `struct vmwpvs_cfg_cmd`: `VMWPVS_CMD_CONFIG`'s argument.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmwpvsCfgCmd {
    /// `cmp_addr`: where the page goes.
    pub cmp_addr: u64,
    /// `pg_addr`.
    pub pg_addr: u32,
    /// `pg_addr_type`.
    pub pg_addr_type: u32,
    /// `pg_num`.
    pub pg_num: u32,
    /// `__reserved`.
    pub __reserved: u32,
}

// SAFETY: `#[repr(C)]`, 24 bytes without padding; any bits.
unsafe impl VmwpvsWire for VmwpvsCfgCmd {}

/// `struct vmwpvs_setup_rings_cmd`: `VMWPVS_CMD_SETUP_RINGS`'s argument.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmwpvsSetupRingsCmd {
    /// `req_pages`.
    pub req_pages: u32,
    /// `cmp_pages`.
    pub cmp_pages: u32,
    /// `state_ppn`.
    pub state_ppn: u64,
    /// `req_page_ppn`.
    pub req_page_ppn: [u64; VMWPVS_MAX_RING_PAGES],
    /// `cmp_page_ppn`.
    pub cmp_page_ppn: [u64; VMWPVS_MAX_RING_PAGES],
}

// SAFETY: `#[repr(C)]`; the two `u32`s fill the first 8 bytes, so no padding (528 bytes,
// checked below); any bits.
unsafe impl VmwpvsWire for VmwpvsSetupRingsCmd {}

/// `struct vmwpvs_setup_rings_msg`: `VMWPVS_CMD_SETUP_MSG_RING`'s argument.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmwpvsSetupRingsMsg {
    /// `msg_pages`.
    pub msg_pages: u32,
    /// `__reserved`.
    pub __reserved: u32,
    /// `msg_page_ppn`.
    pub msg_page_ppn: [u64; VMWPVS_MAX_MSG_RING_PAGES],
}

// SAFETY: `#[repr(C)]`, 136 bytes without padding; any bits.
unsafe impl VmwpvsWire for VmwpvsSetupRingsMsg {}

/// `struct vmwpvs_cfg_pg_header`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmwpvsCfgPgHeader {
    /// `pg_num`.
    pub pg_num: u32,
    /// `num_dwords`.
    pub num_dwords: u16,
    /// `host_status`.
    pub host_status: u16,
    /// `scsi_status`.
    pub scsi_status: u16,
    /// `__reserved`.
    pub __reserved: [u16; 3],
}

// SAFETY: `#[repr(C)]`, 16 bytes without padding; any bits.
unsafe impl VmwpvsWire for VmwpvsCfgPgHeader {}

/// `struct vmwpvs_cfg_pg_controller`: the controller's configuration page.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmwpvsCfgPgController {
    /// `header`.
    pub header: VmwpvsCfgPgHeader,
    /// `wwnn`.
    pub wwnn: u64,
    /// `manufacturer`.
    pub manufacturer: [u16; 64],
    /// `serial_number`.
    pub serial_number: [u16; 64],
    /// `oprom_version`.
    pub oprom_version: [u16; 32],
    /// `hardware_version`.
    pub hardware_version: [u16; 32],
    /// `firmware_version`.
    pub firmware_version: [u16; 32],
    /// `num_phys`.
    pub num_phys: u32,
    /// `use_consec_phy_wwns`.
    pub use_consec_phy_wwns: u8,
    /// `__reserved`.
    pub __reserved: [u8; 3],
}

// SAFETY: `#[repr(C)]`, 480 bytes without padding (checked below); any bits.
unsafe impl VmwpvsWire for VmwpvsCfgPgController {}

/* driver stuff */

/// `struct vmwpvs_dmamem`: a DMA area mapped in the kernel and loaded in its own map.
pub struct VmwpvsDmamem {
    /// `dm_map`.
    pub dm_map: &'static BusDmamap,
    /// `dm_seg`.
    pub dm_seg: BusDmaSegment,
    /// `dm_size`.
    pub dm_size: usize,
    /// `dm_kva`.
    pub dm_kva: NonNull<u8>,
}

/// `struct vmwpvs_sgl`: an opening's scatter/gather list.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VmwpvsSgl {
    /// `list`.
    pub list: [VmwpvsSge; VMWPVS_MAXSGL],
}

/// `struct vmwpvs_ccb`: an opening.
pub struct VmwpvsCcb {
    /// `ccb_entry`: `sc_ccb_list`, or a completion list.
    pub ccb_entry: SimpleqEntry<VmwpvsCcb>,
    /// `ccb_dmamap`: the map of the transfer's data.
    pub ccb_dmamap: Cell<Option<&'static BusDmamap>>,
    /// `ccb_xs`: the transfer in flight.
    pub ccb_xs: Cell<Option<&'static ScsiXfer>>,
    /// `ccb_ctx`: the request's context.
    pub ccb_ctx: Cell<u64>,
    /// `ccb_sgl`: the opening's list in `sc_sgls`.
    pub ccb_sgl: Cell<*mut VmwpvsSgl>,
    /// `ccb_sgl_offset`.
    pub ccb_sgl_offset: Cell<usize>,
    /// `ccb_sense`: the opening's sense buffer in `sc_sense`.
    pub ccb_sense: Cell<*mut u8>,
    /// `ccb_sense_offset`.
    pub ccb_sense_offset: Cell<usize>,
}

impl VmwpvsCcb {
    /// `ccb->ccb_dmamap`, which every opening has once attach made it.
    fn dmamap(&self) -> &'static BusDmamap {
        match self.ccb_dmamap.get() {
            Some(map) => map,
            None => panic(format_args!("vmwpvs: opening without a map")),
        }
    }

    /// `ccb->ccb_xs`, set while the opening is in flight.
    fn xs(&self) -> &'static ScsiXfer {
        match self.ccb_xs.get() {
            Some(xs) => xs,
            None => panic(format_args!("vmwpvs: opening without a transfer")),
        }
    }
}

queue_adapter!(
    /// `SIMPLEQ_HEAD(vmwpvs_ccb_list, vmwpvs_ccb)`, through `ccb_entry`.
    pub VmwpvsCcbList: VmwpvsCcb, ccb_entry => SimpleqEntry<VmwpvsCcb>
);

/// `struct vmwpvs_softc`.
#[repr(C)]
pub struct VmwpvsSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_pc`.
    pub sc_pc: Cell<Option<PciChipsetTag>>,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_ios`.
    pub sc_ios: Cell<BusSize>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_req_ring`.
    pub sc_req_ring: Cell<Option<&'static VmwpvsDmamem>>,
    /// `sc_cmp_ring`.
    pub sc_cmp_ring: Cell<Option<&'static VmwpvsDmamem>>,
    /// `sc_msg_ring`: `None` when the device has no message ring.
    pub sc_msg_ring: Cell<Option<&'static VmwpvsDmamem>>,
    /// `sc_ring_state`.
    pub sc_ring_state: Cell<Option<&'static VmwpvsDmamem>>,
    /// `sc_ring_mtx`: protects the rings and their state.
    pub sc_ring_mtx: Mutex,
    /// `sc_sgls`.
    pub sc_sgls: Cell<Option<&'static VmwpvsDmamem>>,
    /// `sc_sense`.
    pub sc_sense: Cell<Option<&'static VmwpvsDmamem>>,
    /// `sc_ccbs`: the openings.
    pub sc_ccbs: Cell<*mut VmwpvsCcb>,
    /// The number of openings at `sc_ccbs`.
    pub sc_nccbs: Cell<usize>,
    /// `sc_ccb_list`: the free openings.
    pub sc_ccb_list: SimpleqHead<VmwpvsCcbList>,
    /// `sc_ccb_mtx`: protects `sc_ccb_list`.
    pub sc_ccb_mtx: Mutex,
    /// `sc_ih`.
    pub sc_ih: Cell<*mut c_void>,
    /// `sc_msg_task`.
    pub sc_msg_task: Task,
    /// `sc_bus_width`.
    pub sc_bus_width: Cell<u32>,
    /// `sc_iopool`.
    pub sc_iopool: ScsiIopool,
    /// `sc_scsibus`.
    pub sc_scsibus: Cell<Option<&'static ScsibusSoftc>>,
}

impl VmwpvsSoftc {
    /// `DEVNAME(sc)`.
    fn devname(&'static self) -> &'static str {
        self.sc_dev.xname()
    }

    /// `sc->sc_dmat`, which attach sets first.
    fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => panic(format_args!("vmwpvs: no DMA tag")),
        }
    }

    /// One of the DMA areas attach allocated.
    fn area(&self, a: &Cell<Option<&'static VmwpvsDmamem>>) -> &'static VmwpvsDmamem {
        match a.get() {
            Some(dm) => dm,
            None => panic(format_args!("vmwpvs: DMA area not allocated")),
        }
    }

    /// `&sc->sc_ccbs[i]`, `i` below `sc_nccbs`.
    fn ccb(&self, i: usize) -> &'static VmwpvsCcb {
        if i >= self.sc_nccbs.get() || self.sc_ccbs.get().is_null() {
            panic(format_args!("vmwpvs: bad opening {i}"));
        }
        // SAFETY: attach initialised `sc_nccbs` openings at `sc_ccbs`, which are never freed
        // while the device is attached.
        unsafe { &*self.sc_ccbs.get().add(i) }
    }

    /// The ring state page.
    fn ring_state(&self) -> *mut VmwpvwRingState {
        vmwpvs_dma_kva(self.area(&self.sc_ring_state))
    }
}

// SAFETY: `#[repr(C)]` with the device first; the mutexes, the queue head, the task and the
// iopool are all-zero valid (`sys/mutex.rs`, `sys/queue.rs`, `sys/task.rs`,
// `scsi/scsiconf.rs`), and every other member is a `Cell` of an integer, a raw pointer or an
// `Option` of a tag, a handle or a reference.
unsafe impl Softc for VmwpvsSoftc {}

/// `vmwpvs_ca`.
pub static VMWPVS_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<VmwpvsSoftc>(),
    ca_match: Some(vmwpvs_match),
    ca_attach: vmwpvs_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `vmwpvs_cd`.
pub static VMWPVS_CD: Cfdriver = Cfdriver::new(b"vmwpvs", DV_DULL, 0);

/// `vmwpvs_switch`.
pub static VMWPVS_SWITCH: ScsiAdapter = ScsiAdapter {
    scsi_cmd: vmwpvs_scsi_cmd,
    dev_minphys: None,
    dev_probe: None,
    dev_free: None,
    ioctl: None,
};

/// `VMWPVS_DMA_KVA(dm)`: the area's kernel address.
fn vmwpvs_dma_kva<T>(dm: &VmwpvsDmamem) -> *mut T {
    dm.dm_kva.as_ptr().cast()
}

/// `VMWPVS_DMA_DVA(dm)`: the area's device address.
fn vmwpvs_dma_dva(dm: &VmwpvsDmamem) -> u64 {
    dm.dm_map.dm_segs()[0].get().ds_addr as u64
}

/// `vmwpvs_read`.
fn vmwpvs_read(sc: &VmwpvsSoftc, r: BusSize) -> u32 {
    let (Some(t), Some(h)) = (sc.sc_iot.get(), sc.sc_ioh.get()) else {
        panic(format_args!("vmwpvs: registers not mapped"));
    };
    bus_space_read_4(t, h, r)
}

/// `vmwpvs_write`.
fn vmwpvs_write(sc: &VmwpvsSoftc, r: BusSize, v: u32) {
    let (Some(t), Some(h)) = (sc.sc_iot.get(), sc.sc_ioh.get()) else {
        panic(format_args!("vmwpvs: registers not mapped"));
    };
    bus_space_write_4(t, h, r, v);
}

/// Reads the ring state page whole (the device writes it by DMA).
fn vmwpvs_state_read(sc: &VmwpvsSoftc) -> VmwpvwRingState {
    // SAFETY: the ring state area is a page of zeroed, mapped DMA memory, page aligned, that
    // lives while the device is attached; any bits are a valid state.
    unsafe { ptr::read_volatile(sc.ring_state()) }
}

/// Writes one consumer or producer index of the ring state page (`s->field = v`), the
/// member at byte `offset` (an `offset_of!` of a `u32` member of [`VmwpvwRingState`]).
fn vmwpvs_state_write(sc: &VmwpvsSoftc, offset: usize, v: u32) {
    kassert!(offset.is_multiple_of(4) && offset < size_of::<VmwpvwRingState>());
    // SAFETY: the offset names a `u32` member of the page (4-byte aligned in a page-aligned
    // area that lives while the device is attached).
    unsafe { ptr::write_volatile(sc.ring_state().cast::<u8>().add(offset).cast::<u32>(), v) }
}

/// `vmwpvs_match`.
pub fn vmwpvs_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    i32::from(
        pci_vendor(pa.pa_id) == PCI_VENDOR_VMWARE
            && pci_product(pa.pa_id) == PCI_PRODUCT_VMWARE_PVSCSI,
    )
}

/// Where `vmwpvs_attach` unwinds to after a failure (the C's labels, in order).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Unwind {
    FreeCcbs,
    FreeSgl,
    FreeMsgRing,
    FreeCmpRing,
    FreeReqRing,
    FreeRingState,
    Unmap,
}

/// `vmwpvs_attach`.
pub fn vmwpvs_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `vmwpvs_ca`, whose softc is a `VmwpvsSoftc`; softcs are
    // never freed while the device exists, so the softc may be borrowed for 'static.
    let sc: &'static VmwpvsSoftc = unsafe { &*ptr::from_ref(self_.softc::<VmwpvsSoftc>()) };
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    sc.sc_pc.set(Some(pa.pa_pc));
    sc.sc_dmat.set(Some(pa.pa_dmat));

    sc.sc_bus_width.set(16);
    mtx_init(&sc.sc_ring_mtx, IPL_BIO);
    mtx_init(&sc.sc_ccb_mtx, IPL_BIO);
    task_set(
        &sc.sc_msg_task,
        vmwpvs_msg_task,
        ptr::from_ref(sc).cast_mut().cast(),
    );
    sc.sc_ccb_list.init();

    let mut r = PCI_MAPREG_START;
    let mut memtype = 0;
    while r < PCI_MAPREG_END {
        memtype = pci_mapreg_type(pa.pa_pc, pa.pa_tag, r);
        if memtype & PCI_MAPREG_TYPE_MASK == PCI_MAPREG_TYPE_MEM {
            break;
        }
        r += size_of::<u32>() as i32;
    }
    if r >= PCI_MAPREG_END {
        printf(format_args!(": unable to locate registers\n"));
        return;
    }

    let Ok((iot, ioh, _base, ios)) = pci_mapreg_map(pa, r, memtype, 0, VMMPVS_PG_LEN as BusSize)
    else {
        printf(format_args!(": unable to map registers\n"));
        return;
    };
    sc.sc_iot.set(Some(iot));
    sc.sc_ioh.set(Some(ioh));
    sc.sc_ios.set(ios);

    let mut use_msg = false;
    let mut nccbs = 0;
    let failed: Option<Unwind> = 'attach: {
        // hook up the interrupt
        vmwpvs_write(sc, VMWPVS_R_INTR_MASK, 0);

        let mut isr: PciIntrFn = vmwpvs_intx;
        let ih = match pci_intr_map_msi(pa) {
            Some(ih) => {
                isr = vmwpvs_intr;
                ih
            }
            None => match pci_intr_map(pa) {
                Some(ih) => ih,
                None => {
                    printf(format_args!(": unable to map interrupt\n"));
                    break 'attach Some(Unwind::Unmap);
                }
            },
        };
        printf(format_args!(": {}\n", pci_intr_string(pa.pa_pc, ih)));

        // do we have msg support?
        vmwpvs_write(sc, VMWPVS_R_COMMAND, VMWPVS_CMD_SETUP_MSG_RING);
        use_msg = vmwpvs_read(sc, VMWPVS_R_COMMAND_STATUS) != 0xffff_ffff;

        if vmwpvs_get_config(sc).is_err() {
            printf(format_args!("{}: get configuration failed\n", sc.devname()));
            break 'attach Some(Unwind::Unmap);
        }

        let Some(dm) = vmwpvs_dmamem_zalloc(sc, VMWPVS_PAGE_SIZE) else {
            printf(format_args!(
                "{}: unable to allocate ring state\n",
                sc.devname()
            ));
            break 'attach Some(Unwind::Unmap);
        };
        sc.sc_ring_state.set(Some(dm));

        let Some(dm) = vmwpvs_dmamem_zalloc(sc, VMWPVS_RING_PAGES * VMWPVS_PAGE_SIZE) else {
            printf(format_args!(
                "{}: unable to allocate req ring\n",
                sc.devname()
            ));
            break 'attach Some(Unwind::FreeRingState);
        };
        sc.sc_req_ring.set(Some(dm));

        let Some(dm) = vmwpvs_dmamem_zalloc(sc, VMWPVS_RING_PAGES * VMWPVS_PAGE_SIZE) else {
            printf(format_args!(
                "{}: unable to allocate cmp ring\n",
                sc.devname()
            ));
            break 'attach Some(Unwind::FreeReqRing);
        };
        sc.sc_cmp_ring.set(Some(dm));

        if use_msg {
            let Some(dm) = vmwpvs_dmamem_zalloc(sc, VMWPVS_RING_PAGES * VMWPVS_PAGE_SIZE) else {
                printf(format_args!(
                    "{}: unable to allocate msg ring\n",
                    sc.devname()
                ));
                break 'attach Some(Unwind::FreeCmpRing);
            };
            sc.sc_msg_ring.set(Some(dm));
        }

        nccbs = (VMWPVS_RING_PAGES * VMWPVS_PAGE_SIZE) / size_of::<VmwpvsRingReq>();

        let Some(dm) = vmwpvs_dmamem_alloc(sc, nccbs * size_of::<VmwpvsSgl>()) else {
            printf(format_args!("{}: unable to allocate sgls\n", sc.devname()));
            break 'attach Some(Unwind::FreeMsgRing);
        };
        sc.sc_sgls.set(Some(dm));

        let Some(dm) = vmwpvs_dmamem_alloc(sc, nccbs * VMWPVS_SENSELEN) else {
            printf(format_args!(
                "{}: unable to allocate sense data\n",
                sc.devname()
            ));
            break 'attach Some(Unwind::FreeSgl);
        };
        sc.sc_sense.set(Some(dm));

        // can't fail
        let Some(ccbs) = mallocarray(nccbs, size_of::<VmwpvsCcb>(), M_DEVBUF, M_WAITOK) else {
            panic(format_args!("vmwpvs: M_WAITOK allocation failed"));
        };
        let ccbs = ccbs.cast::<VmwpvsCcb>().as_ptr();
        sc.sc_ccbs.set(ccbs);

        let sgls = vmwpvs_dma_kva::<VmwpvsSgl>(sc.area(&sc.sc_sgls));
        let sense = vmwpvs_dma_kva::<u8>(sc.area(&sc.sc_sense));
        for i in 0..nccbs {
            let dmamap = match bus_dmamap_create(
                sc.dmat(),
                MAXPHYS as BusSize,
                VMWPVS_MAXSGL as i32,
                MAXPHYS as BusSize,
                0,
                BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW,
            ) {
                Ok(map) => map,
                Err(_) => {
                    printf(format_args!("{}: unable to create ccb map\n", sc.devname()));
                    break 'attach Some(Unwind::FreeCcbs);
                }
            };
            // SAFETY: `ccbs` has room for `nccbs` openings (mallocarray above), and the
            // offsets stay inside the sgl and sense areas, which hold `nccbs` of each.
            unsafe {
                ccbs.add(i).write(VmwpvsCcb {
                    ccb_entry: SimpleqEntry::new(),
                    ccb_dmamap: Cell::new(Some(dmamap)),
                    ccb_xs: Cell::new(None),
                    ccb_ctx: Cell::new(0xdead_beef_0000_0000 | i as u64),
                    ccb_sgl: Cell::new(sgls.add(i)),
                    ccb_sgl_offset: Cell::new(i * size_of::<VmwpvsSgl>()),
                    ccb_sense: Cell::new(sense.add(i * VMWPVS_SENSELEN)),
                    ccb_sense_offset: Cell::new(i * VMWPVS_SENSELEN),
                });
            }
            sc.sc_nccbs.set(i + 1);
            // SAFETY: the opening just made, on no list, never moved or freed while the
            // device is attached; the cookie is the softc.
            unsafe {
                vmwpvs_ccb_put(
                    ptr::from_ref(sc).cast_mut().cast(),
                    NonNull::new_unchecked(ccbs.add(i)).cast(),
                )
            };
        }

        let Some(ih) = pci_intr_establish(
            pa.pa_pc,
            ih,
            IPL_BIO | IPL_MPSAFE,
            isr,
            ptr::from_ref(sc).cast_mut().cast(),
            sc.devname(),
        ) else {
            break 'attach Some(Unwind::FreeMsgRing);
        };
        sc.sc_ih.set(ih.as_ptr());

        let ring_bytes = VMWPVS_RING_PAGES * VMWPVS_PAGE_SIZE;
        bus_dmamap_sync(
            sc.dmat(),
            sc.area(&sc.sc_cmp_ring).dm_map,
            0,
            ring_bytes,
            BUS_DMASYNC_PREREAD,
        );
        bus_dmamap_sync(
            sc.dmat(),
            sc.area(&sc.sc_req_ring).dm_map,
            0,
            ring_bytes,
            BUS_DMASYNC_PREWRITE,
        );
        if use_msg {
            bus_dmamap_sync(
                sc.dmat(),
                sc.area(&sc.sc_msg_ring).dm_map,
                0,
                ring_bytes,
                BUS_DMASYNC_PREREAD,
            );
        }
        bus_dmamap_sync(
            sc.dmat(),
            sc.area(&sc.sc_ring_state).dm_map,
            0,
            VMWPVS_PAGE_SIZE,
            BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
        );

        let mut intmask = VMWPVS_INTR_CMPL_MASK;

        vmwpvs_setup_rings(sc);
        if use_msg {
            vmwpvs_setup_msg_ring(sc);
            intmask |= VMWPVS_INTR_MSG_MASK;
        }

        vmwpvs_write(sc, VMWPVS_R_INTR_MASK, intmask);

        // SAFETY: vmwpvs_ccb_get and vmwpvs_ccb_put take this softc as their cookie, and the
        // softc is never freed.
        unsafe {
            scsi_iopool_init(
                &sc.sc_iopool,
                ptr::from_ref(sc).cast_mut().cast(),
                vmwpvs_ccb_get,
                vmwpvs_ccb_put,
            )
        };

        let mut saa = ScsibusAttachArgs::new();
        saa.saa_adapter = Some(&VMWPVS_SWITCH);
        saa.saa_adapter_softc = ptr::from_ref(sc).cast_mut().cast();
        saa.saa_adapter_target = SDEV_NO_ADAPTER_TARGET;
        saa.saa_adapter_buswidth = sc.sc_bus_width.get() as u16;
        saa.saa_luns = 8;
        saa.saa_openings = VMWPVS_OPENINGS;
        saa.saa_pool = Some(&sc.sc_iopool);
        saa.saa_quirks = 0;
        saa.saa_flags = 0;
        saa.saa_wwpn = 0;
        saa.saa_wwnn = 0;

        let bus = config_found(&sc.sc_dev, ptr::from_mut(&mut saa).cast(), Some(scsiprint));
        // SAFETY: what attaches at vmwpvs is a scsibus (`scsibus* at scsi?`), whose softc
        // begins with its device; softcs are never freed.
        sc.sc_scsibus
            .set(bus.map(|d| unsafe { &*d.as_ptr().cast::<ScsibusSoftc>().cast_const() }));

        None
    };

    let Some(from) = failed else {
        return;
    };
    let dmat = sc.dmat();
    // Each label falls through to the ones after it, as in the C.
    if from <= Unwind::FreeCcbs && from == Unwind::FreeCcbs {
        // SAFETY: the softc is the pool's cookie; the openings on the free list are the ones
        // made so far.
        while let Some(io) = unsafe { vmwpvs_ccb_get(ptr::from_ref(sc).cast_mut().cast()) } {
            // SAFETY: an opening of `sc_ccbs`, whose map attach created and nothing uses.
            let ccb = unsafe { io.cast::<VmwpvsCcb>().as_ref() };
            // SAFETY: the map is unused and dropped with the opening.
            unsafe { bus_dmamap_destroy(dmat, NonNull::from(ccb.dmamap())) };
        }
        if let Some(p) = NonNull::new(sc.sc_ccbs.replace(ptr::null_mut())) {
            free(p.cast(), M_DEVBUF, nccbs * size_of::<VmwpvsCcb>());
        }
        sc.sc_nccbs.set(0);
        // free_sense:
        // SAFETY: allocated above and unused by the device (no ring was set up).
        unsafe { vmwpvs_dmamem_free(sc, sc.area(&sc.sc_sense)) };
    }
    if from <= Unwind::FreeSgl {
        // SAFETY: as above.
        unsafe { vmwpvs_dmamem_free(sc, sc.area(&sc.sc_sgls)) };
    }
    if from <= Unwind::FreeMsgRing && use_msg {
        // SAFETY: as above.
        unsafe { vmwpvs_dmamem_free(sc, sc.area(&sc.sc_msg_ring)) };
    }
    if from <= Unwind::FreeCmpRing {
        // SAFETY: as above.
        unsafe { vmwpvs_dmamem_free(sc, sc.area(&sc.sc_cmp_ring)) };
    }
    if from <= Unwind::FreeReqRing {
        // SAFETY: as above.
        unsafe { vmwpvs_dmamem_free(sc, sc.area(&sc.sc_req_ring)) };
    }
    if from <= Unwind::FreeRingState {
        // SAFETY: as above.
        unsafe { vmwpvs_dmamem_free(sc, sc.area(&sc.sc_ring_state)) };
    }
    // unmap:
    bus_space_unmap(iot, ioh, sc.sc_ios.get());
    sc.sc_ios.set(0);
}

/// Writes `cmd` with its argument structure `arg` (`vmwpvs_cmd(sc, cmd, &arg, sizeof(arg))`).
fn vmwpvs_cmd_wire<T: VmwpvsWire>(sc: &VmwpvsSoftc, cmd: u32, arg: &T) {
    vmwpvs_cmd(sc, cmd, arg.as_bytes());
}

/// `vmwpvs_setup_rings`.
pub fn vmwpvs_setup_rings(sc: &VmwpvsSoftc) {
    let mut cmd = VmwpvsSetupRingsCmd::zeroed();
    cmd.req_pages = VMWPVS_RING_PAGES as u32;
    cmd.cmp_pages = VMWPVS_RING_PAGES as u32;
    cmd.state_ppn = vmwpvs_dma_dva(sc.area(&sc.sc_ring_state)) >> VMWPVS_PAGE_SHIFT;

    let ppn = vmwpvs_dma_dva(sc.area(&sc.sc_req_ring)) >> VMWPVS_PAGE_SHIFT;
    for (i, p) in cmd.req_page_ppn[..VMWPVS_RING_PAGES].iter_mut().enumerate() {
        *p = ppn + i as u64;
    }

    let ppn = vmwpvs_dma_dva(sc.area(&sc.sc_cmp_ring)) >> VMWPVS_PAGE_SHIFT;
    for (i, p) in cmd.cmp_page_ppn[..VMWPVS_RING_PAGES].iter_mut().enumerate() {
        *p = ppn + i as u64;
    }

    vmwpvs_cmd_wire(sc, VMWPVS_CMD_SETUP_RINGS, &cmd);
}

/// `vmwpvs_setup_msg_ring`.
pub fn vmwpvs_setup_msg_ring(sc: &VmwpvsSoftc) {
    let mut cmd = VmwpvsSetupRingsMsg::zeroed();
    cmd.msg_pages = VMWPVS_RING_PAGES as u32;

    let ppn = vmwpvs_dma_dva(sc.area(&sc.sc_msg_ring)) >> VMWPVS_PAGE_SHIFT;
    for (i, p) in cmd.msg_page_ppn[..VMWPVS_RING_PAGES].iter_mut().enumerate() {
        *p = ppn + i as u64;
    }

    vmwpvs_cmd_wire(sc, VMWPVS_CMD_SETUP_MSG_RING, &cmd);
}

/// Whether a configuration page header reports success (`host_status` and `scsi_status`).
pub fn vmwpvs_cfg_ok(hdr: &VmwpvsCfgPgHeader) -> bool {
    hdr.host_status == VMWPVS_HOST_STATUS_SUCCESS && hdr.scsi_status == VMWPVS_SCSI_STATUS_OK
}

/// `vmwpvs_get_config`: reads the controller's configuration page for its bus width.
pub fn vmwpvs_get_config(sc: &'static VmwpvsSoftc) -> Result<(), crate::sys::errno::Errno> {
    use crate::sys::errno::Errno::{EIO, ENOMEM};

    let dm = vmwpvs_dmamem_alloc(sc, VMWPVS_PAGE_SIZE).ok_or(ENOMEM)?;

    let mut cmd = VmwpvsCfgCmd::zeroed();
    cmd.cmp_addr = vmwpvs_dma_dva(dm);
    cmd.pg_addr_type = VMWPVS_CFGPGADDR_CONTROLLER;
    cmd.pg_num = VMWPVS_CFGPG_CONTROLLER;

    let pg = vmwpvs_dma_kva::<VmwpvsCfgPgController>(dm);
    let mut hdr = VmwpvsCfgPgHeader::zeroed();
    hdr.host_status = VMWPVS_HOST_STATUS_INVPARAM;
    hdr.scsi_status = VMWPVS_SCSI_STATUS_CHECK;
    // SAFETY: the area is a mapped page, larger than the controller page, page aligned, and
    // ours until it is freed below.
    unsafe {
        ptr::write_bytes(vmwpvs_dma_kva::<u8>(dm), 0, VMWPVS_PAGE_SIZE);
        ptr::write_volatile(ptr::addr_of_mut!((*pg).header), hdr);
    }

    bus_dmamap_sync(
        sc.dmat(),
        dm.dm_map,
        0,
        VMWPVS_PAGE_SIZE,
        BUS_DMASYNC_PREREAD,
    );
    vmwpvs_cmd_wire(sc, VMWPVS_CMD_CONFIG, &cmd);
    bus_dmamap_sync(
        sc.dmat(),
        dm.dm_map,
        0,
        VMWPVS_PAGE_SIZE,
        BUS_DMASYNC_POSTREAD,
    );

    // SAFETY: as above; the device wrote the page by DMA, so it is read volatile; any bits
    // are a valid page.
    let page = unsafe { ptr::read_volatile(pg) };
    let rv = if vmwpvs_cfg_ok(&page.header) {
        sc.sc_bus_width.set(page.num_phys);
        Ok(())
    } else {
        Err(EIO)
    };

    // done:
    // SAFETY: the area allocated above; the command has completed and nothing keeps it.
    unsafe { vmwpvs_dmamem_free(sc, dm) };

    rv
}

/// `vmwpvs_cmd`: writes the command, then its argument's bytes as 32-bit words.
pub fn vmwpvs_cmd(sc: &VmwpvsSoftc, cmd: u32, buf: &[u8]) {
    vmwpvs_write(sc, VMWPVS_R_COMMAND, cmd);
    for w in buf.as_chunks::<4>().0 {
        vmwpvs_write(sc, VMWPVS_R_COMMAND_DATA, u32::from_ne_bytes(*w));
    }
}

/// `vmwpvs_intx`: the INTx handler, which checks and acknowledges the status first.
pub fn vmwpvs_intx(xsc: *mut c_void) -> i32 {
    // SAFETY: established with the softc as its argument; softcs are never freed.
    let sc = unsafe { &*xsc.cast::<VmwpvsSoftc>().cast_const() };

    let status = vmwpvs_read(sc, VMWPVS_R_INTR_STATUS);
    if status & VMWPVS_INTR_ALL_MASK == 0 {
        return 0;
    }

    vmwpvs_write(sc, VMWPVS_R_INTR_STATUS, status);

    vmwpvs_intr(xsc)
}

/// `vmwpvs_intr`: completes what the completion ring holds, and queues the message task.
pub fn vmwpvs_intr(xsc: *mut c_void) -> i32 {
    // SAFETY: established with the softc as its argument; softcs are never freed.
    let sc: &'static VmwpvsSoftc = unsafe { &*xsc.cast::<VmwpvsSoftc>().cast_const() };
    let ring = vmwpvs_dma_kva::<VmwpvsRingCmp>(sc.area(&sc.sc_cmp_ring));
    let list: SimpleqHead<VmwpvsCcbList> = SimpleqHead::new();
    let state_map = sc.area(&sc.sc_ring_state).dm_map;
    let cmp_map = sc.area(&sc.sc_cmp_ring).dm_map;

    mtx_enter(&sc.sc_ring_mtx);

    bus_dmamap_sync(
        sc.dmat(),
        state_map,
        0,
        VMWPVS_PAGE_SIZE,
        BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
    );
    let s = vmwpvs_state_read(sc);
    let mut cons = s.cmp_cons;
    let prod = s.cmp_prod;
    vmwpvs_state_write(sc, offset_of!(VmwpvwRingState, cmp_cons), prod);

    let msg = sc.sc_msg_ring.get().is_some() && s.msg_cons != s.msg_prod;

    bus_dmamap_sync(
        sc.dmat(),
        state_map,
        0,
        VMWPVS_PAGE_SIZE,
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    if cons != prod {
        bus_dmamap_sync(
            sc.dmat(),
            cmp_map,
            0,
            VMWPVS_PAGE_SIZE,
            BUS_DMASYNC_POSTREAD,
        );

        loop {
            let i = cons as usize % VMWPVS_CMP_COUNT;
            cons = cons.wrapping_add(1);
            // SAFETY: `i` is inside the completion ring, mapped while the device is attached;
            // the device wrote the entry by DMA; any bits are a valid entry.
            let c = unsafe { ptr::read_volatile(ring.add(i)) };
            let ccb = vmwpvs_scsi_cmd_done(sc, &c);
            // SAFETY: the opening completed, so it is on no list; it stays in place.
            unsafe { list.insert_tail(ccb) };
            if cons == prod {
                break;
            }
        }

        bus_dmamap_sync(sc.dmat(), cmp_map, 0, VMWPVS_PAGE_SIZE, BUS_DMASYNC_PREREAD);
    }

    mtx_leave(&sc.sc_ring_mtx);

    while let Some(ccb) = list.first() {
        let xs = ccb.xs();
        // SAFETY: the list is not empty.
        unsafe { list.remove_head() };
        scsi_done(xs);
    }

    if msg {
        task_add(SYSTQMP, &sc.sc_msg_task);
    }

    1
}

/// `vmwpvs_msg_task`: probes or detaches the targets the device's messages name.
pub fn vmwpvs_msg_task(xsc: *mut c_void) {
    // SAFETY: task_set gave the softc as the argument; softcs are never freed.
    let sc: &'static VmwpvsSoftc = unsafe { &*xsc.cast::<VmwpvsSoftc>().cast_const() };
    let ring = vmwpvs_dma_kva::<VmwpvsRingMsg>(sc.area(&sc.sc_msg_ring));
    let state_map = sc.area(&sc.sc_ring_state).dm_map;
    let msg_map = sc.area(&sc.sc_msg_ring).dm_map;
    let ring_bytes = VMWPVS_RING_PAGES * VMWPVS_PAGE_SIZE;

    mtx_enter(&sc.sc_ring_mtx);
    bus_dmamap_sync(
        sc.dmat(),
        state_map,
        0,
        VMWPVS_PAGE_SIZE,
        BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
    );
    let s = vmwpvs_state_read(sc);
    let mut cons = s.msg_cons;
    let prod = s.msg_prod;
    bus_dmamap_sync(
        sc.dmat(),
        state_map,
        0,
        VMWPVS_PAGE_SIZE,
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );
    mtx_leave(&sc.sc_ring_mtx);

    // we dont have to lock around the msg ring cos the system taskq has only one thread.

    bus_dmamap_sync(sc.dmat(), msg_map, 0, ring_bytes, BUS_DMASYNC_POSTREAD);
    while cons != prod {
        let i = cons as usize % VMWPVS_MSG_COUNT;
        cons = cons.wrapping_add(1);
        // SAFETY: `i` is inside the message ring, mapped while the device is attached; the
        // device wrote the entry; any bits are a valid message.
        let msg = unsafe { ptr::read_volatile(ring.add(i)) };
        // SAFETY: the same entry seen as a device message (same size, any bits valid).
        let dvmsg = unsafe { ptr::read_volatile(ring.add(i).cast::<VmwpvsRingMsgDev>()) };

        match u32::from_le(msg.type_) {
            VMWPVS_MSG_T_ADDED => {
                if u32::from_le(dvmsg.bus) != 0 {
                    printf(format_args!(
                        "{}: ignoring request to add device on bus {}\n",
                        sc.devname(),
                        u32::from_le(msg.type_) as i32
                    ));
                    continue;
                }

                kernel_lock();
                if let Some(sb) = sc.sc_scsibus.get()
                    && scsi_probe_lun(
                        sb,
                        u32::from_le(dvmsg.target) as i32,
                        i32::from(dvmsg.lun[1]),
                    )
                    .is_err()
                {
                    printf(format_args!(
                        "{}: error probing target {} lun {}\n",
                        sc.devname(),
                        u32::from_le(dvmsg.target) as i32,
                        dvmsg.lun[1]
                    ));
                }
                kernel_unlock();
            }

            VMWPVS_MSG_T_REMOVED => {
                if u32::from_le(dvmsg.bus) != 0 {
                    printf(format_args!(
                        "{}: ignoring request to remove device on bus {}\n",
                        sc.devname(),
                        u32::from_le(msg.type_) as i32
                    ));
                    continue;
                }

                kernel_lock();
                if let Some(sb) = sc.sc_scsibus.get()
                    && scsi_detach_lun(
                        sb,
                        u32::from_le(dvmsg.target) as i32,
                        i32::from(dvmsg.lun[1]),
                        DETACH_FORCE,
                    )
                    .is_err()
                {
                    printf(format_args!(
                        "{}: error detaching target {} lun {}\n",
                        sc.devname(),
                        u32::from_le(dvmsg.target) as i32,
                        dvmsg.lun[1]
                    ));
                }
                kernel_unlock();
            }

            t => {
                printf(format_args!("{}: unknown msg type {t}\n", sc.devname()));
            }
        }
    }
    bus_dmamap_sync(sc.dmat(), msg_map, 0, ring_bytes, BUS_DMASYNC_PREREAD);

    mtx_enter(&sc.sc_ring_mtx);
    bus_dmamap_sync(
        sc.dmat(),
        state_map,
        0,
        VMWPVS_PAGE_SIZE,
        BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
    );
    vmwpvs_state_write(sc, offset_of!(VmwpvwRingState, msg_cons), prod);
    bus_dmamap_sync(
        sc.dmat(),
        state_map,
        0,
        VMWPVS_PAGE_SIZE,
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );
    mtx_leave(&sc.sc_ring_mtx);
}

/// The direction flag of a request for transfer flags `flags`.
pub fn vmwpvs_req_dir(flags: i32) -> u32 {
    match flags & (SCSI_DATA_IN | SCSI_DATA_OUT) {
        SCSI_DATA_IN => VMWPVS_REQ_DIR_IN,
        SCSI_DATA_OUT => VMWPVS_REQ_DIR_OUT,
        _ => VMWPVS_REQ_DIR_NONE,
    }
}

/// `vmwpvs_scsi_cmd`: the adapter's `scsi_cmd`: puts the request on the ring and kicks it.
pub fn vmwpvs_scsi_cmd(xs: &'static ScsiXfer) {
    let link = xs.link();
    let p = link.bus().sb_adapter_softc.get();
    if p.is_null() {
        panic(format_args!("vmwpvs: bus without an adapter softc"));
    }
    // SAFETY: vmwpvs_attach attaches its scsibus with its own softc as `saa_adapter_softc`,
    // and only that bus's transfers reach `vmwpvs_switch`; softcs are never freed.
    let sc: &'static VmwpvsSoftc = unsafe { &*p.cast::<VmwpvsSoftc>().cast_const() };
    let Some(io) = xs.io.get() else {
        panic(format_args!(
            "vmwpvs_scsi_cmd: xs {:p} without an opening",
            xs
        ));
    };
    // SAFETY: the transfer's opening came from this adapter's pool, whose `io_get`
    // (vmwpvs_ccb_get) hands out openings of `sc_ccbs`, which live while the device does.
    let ccb: &'static VmwpvsCcb = unsafe { io.cast::<VmwpvsCcb>().as_ref() };
    let dmap = ccb.dmamap();
    let ring = vmwpvs_dma_kva::<VmwpvsRingReq>(sc.area(&sc.sc_req_ring));
    let state_map = sc.area(&sc.sc_ring_state).dm_map;
    let req_map = sc.area(&sc.sc_req_ring).dm_map;
    let flags = xs.flags.get();

    ccb.ccb_xs.set(Some(xs));

    if xs.datalen() > 0 {
        // SAFETY: the transfer's data is valid for `datalen` bytes and reserved for it until
        // it completes, which unloads the map (vmwpvs_scsi_cmd_done).
        let loaded = unsafe {
            bus_dmamap_load(
                sc.dmat(),
                dmap,
                xs.data(),
                xs.datalen() as BusSize,
                None,
                if flags & SCSI_NOSLEEP != 0 {
                    BUS_DMA_NOWAIT
                } else {
                    BUS_DMA_WAITOK
                },
            )
        };
        if loaded.is_err() {
            xs.error.set(XS_DRIVER_STUFFUP);
            scsi_done(xs);
            return;
        }

        bus_dmamap_sync(
            sc.dmat(),
            dmap,
            0,
            dmap.dm_mapsize.get(),
            if flags & SCSI_DATA_IN != 0 {
                BUS_DMASYNC_PREREAD
            } else {
                BUS_DMASYNC_PREWRITE
            },
        );
    }

    mtx_enter(&sc.sc_ring_mtx);

    bus_dmamap_sync(
        sc.dmat(),
        state_map,
        0,
        VMWPVS_PAGE_SIZE,
        BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
    );

    let prod = vmwpvs_state_read(sc).req_prod;
    let slot = prod as usize % VMWPVS_REQ_COUNT;

    bus_dmamap_sync(
        sc.dmat(),
        req_map,
        prod as usize * size_of::<VmwpvsRingReq>(),
        size_of::<VmwpvsRingReq>(),
        BUS_DMASYNC_POSTWRITE,
    );

    let mut r = VmwpvsRingReq::zeroed();
    r.context = ccb.ccb_ctx.get();

    if xs.datalen() > 0 {
        r.data_len = xs.datalen() as u64;
        let nsegs = dmap.dm_nsegs.get() as usize;
        if nsegs == 1 {
            r.data_addr = dmap.dm_segs()[0].get().ds_addr as u64;
        } else {
            let sgl = ccb.ccb_sgl.get();

            r.data_addr = vmwpvs_dma_dva(sc.area(&sc.sc_sgls)) + ccb.ccb_sgl_offset.get() as u64;
            r.flags = VMWPVS_REQ_SGL;

            for i in 0..nsegs {
                let seg = dmap.dm_segs()[i].get();
                let sge = VmwpvsSge {
                    addr: seg.ds_addr as u64,
                    len: seg.ds_len as u32,
                    flags: 0,
                };
                // SAFETY: the opening's list in the sgl area holds VMWPVS_MAXSGL entries,
                // the map's segment limit, so `i` is inside it.
                unsafe { ptr::write_volatile(ptr::addr_of_mut!((*sgl).list[i]), sge) };
            }

            bus_dmamap_sync(
                sc.dmat(),
                sc.area(&sc.sc_sgls).dm_map,
                ccb.ccb_sgl_offset.get(),
                size_of::<VmwpvsSge>() * nsegs,
                BUS_DMASYNC_PREWRITE,
            );
        }
    }
    r.sense_addr = vmwpvs_dma_dva(sc.area(&sc.sc_sense)) + ccb.ccb_sense_offset.get() as u64;
    r.sense_len = size_of::<ScsiSenseData>() as u32;

    bus_dmamap_sync(
        sc.dmat(),
        req_map,
        0,
        VMWPVS_RING_PAGES * VMWPVS_PAGE_SIZE,
        BUS_DMASYNC_POSTWRITE,
    );

    r.flags |= vmwpvs_req_dir(flags);

    let cmdlen = (xs.cmdlen.get().max(0) as usize).min(r.cdb.len());
    r.cdb[..cmdlen].copy_from_slice(&xs.cmd.get().as_bytes()[..cmdlen]);
    r.cdblen = xs.cmdlen.get() as u8;
    r.lun[1] = link.lun.get() as u8; // ugly :(
    r.tag = MSG_SIMPLE_Q_TAG;
    r.bus = 0;
    r.target = link.target.get() as u8;
    r.vcpu_hint = 0;
    // SAFETY: `slot` is inside the request ring, mapped while the device is attached; the
    // device reads the entry by DMA, hence volatile.
    unsafe { ptr::write_volatile(ring.add(slot), r) };

    bus_dmamap_sync(
        sc.dmat(),
        req_map,
        0,
        VMWPVS_RING_PAGES * VMWPVS_PAGE_SIZE,
        BUS_DMASYNC_PREWRITE,
    );

    vmwpvs_state_write(
        sc,
        offset_of!(VmwpvwRingState, req_prod),
        prod.wrapping_add(1),
    );

    bus_dmamap_sync(
        sc.dmat(),
        state_map,
        0,
        VMWPVS_PAGE_SIZE,
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    vmwpvs_write(
        sc,
        if xs.bp.get().is_none() {
            VMWPVS_R_KICK_NON_RW_IO
        } else {
            VMWPVS_R_KICK_RW_IO
        },
        0,
    );

    if flags & SCSI_POLL == 0 {
        mtx_leave(&sc.sc_ring_mtx);
        return;
    }

    let list: SimpleqHead<VmwpvsCcbList> = SimpleqHead::new();
    loop {
        let done = vmwpvs_scsi_cmd_poll(sc);
        // SAFETY: the opening completed, so it is on no list; it stays in place.
        unsafe { list.insert_tail(done) };
        if ptr::eq(done, ccb) {
            break;
        }
    }

    mtx_leave(&sc.sc_ring_mtx);

    while let Some(c) = list.first() {
        let xs = c.xs();
        // SAFETY: the list is not empty.
        unsafe { list.remove_head() };
        scsi_done(xs);
    }
}

/// `vmwpvs_scsi_cmd_poll`: waits for the next completion and completes it.
pub fn vmwpvs_scsi_cmd_poll(sc: &'static VmwpvsSoftc) -> &'static VmwpvsCcb {
    let ring = vmwpvs_dma_kva::<VmwpvsRingCmp>(sc.area(&sc.sc_cmp_ring));
    let state_map = sc.area(&sc.sc_ring_state).dm_map;
    let cmp_map = sc.area(&sc.sc_cmp_ring).dm_map;

    let cons = loop {
        bus_dmamap_sync(
            sc.dmat(),
            state_map,
            0,
            VMWPVS_PAGE_SIZE,
            BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
        );

        let s = vmwpvs_state_read(sc);
        let (cons, prod) = (s.cmp_cons, s.cmp_prod);

        if cons != prod {
            vmwpvs_state_write(
                sc,
                offset_of!(VmwpvwRingState, cmp_cons),
                cons.wrapping_add(1),
            );
        }

        bus_dmamap_sync(
            sc.dmat(),
            state_map,
            0,
            VMWPVS_PAGE_SIZE,
            BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
        );

        if cons != prod {
            break cons;
        }
        delay(1000);
    };

    bus_dmamap_sync(
        sc.dmat(),
        cmp_map,
        0,
        VMWPVS_PAGE_SIZE * VMWPVS_RING_PAGES,
        BUS_DMASYNC_POSTREAD,
    );
    // SAFETY: the index is inside the completion ring, mapped while the device is attached;
    // the device wrote the entry; any bits are a valid entry.
    let c = unsafe { ptr::read_volatile(ring.add(cons as usize % VMWPVS_CMP_COUNT)) };
    let ccb = vmwpvs_scsi_cmd_done(sc, &c);
    bus_dmamap_sync(
        sc.dmat(),
        cmp_map,
        0,
        VMWPVS_PAGE_SIZE * VMWPVS_RING_PAGES,
        BUS_DMASYNC_PREREAD,
    );

    ccb
}

/// The completion's outcome for a transfer of `datalen` bytes: `(error, resid, sense)`,
/// where `resid` is `None` when the C leaves it, and `sense` says whether the sense data is
/// copied (the part of `vmwpvs_scsi_cmd_done` after the DMA syncs, apart for the host tests).
pub fn vmwpvs_cmp_result(c: &VmwpvsRingCmp, datalen: i32) -> (i32, Option<usize>, bool) {
    match c.host_status {
        VMWPVS_HOST_STATUS_SUCCESS
        | VMWPVS_HOST_STATUS_LINKED_CMD_COMPLETED
        | VMWPVS_HOST_STATUS_LINKED_CMD_COMPLETED_WITH_FLAG => {
            if c.scsi_status == VMWPVS_SCSI_STATUS_CHECK {
                (XS_SENSE, Some(0), true)
            } else {
                (XS_NOERROR, Some(0), false)
            }
        }

        VMWPVS_HOST_STATUS_UNDERRUN | VMWPVS_HOST_STATUS_DATARUN => {
            let resid = (i64::from(datalen) as u64).wrapping_sub(c.data_len) as u32;
            (XS_NOERROR, Some(resid as i32 as usize), false)
        }

        VMWPVS_HOST_STATUS_SELTIMEOUT => (XS_SELTIMEOUT, None, false),

        _ => (XS_DRIVER_STUFFUP, None, false),
    }
}

/// `vmwpvs_scsi_cmd_done`: the completion `c` into its opening's transfer.
pub fn vmwpvs_scsi_cmd_done(sc: &'static VmwpvsSoftc, c: &VmwpvsRingCmp) -> &'static VmwpvsCcb {
    let ctx = c.context;
    let ccb = sc.ccb((ctx & 0xffff_ffff) as usize);
    let dmap = ccb.dmamap();
    let xs = ccb.xs();
    let flags = xs.flags.get();

    bus_dmamap_sync(
        sc.dmat(),
        sc.area(&sc.sc_sense).dm_map,
        ccb.ccb_sense_offset.get(),
        size_of::<ScsiSenseData>(),
        BUS_DMASYNC_POSTREAD,
    );

    if xs.datalen() > 0 {
        let nsegs = dmap.dm_nsegs.get() as usize;
        if nsegs > 1 {
            bus_dmamap_sync(
                sc.dmat(),
                sc.area(&sc.sc_sgls).dm_map,
                ccb.ccb_sgl_offset.get(),
                size_of::<VmwpvsSge>() * nsegs,
                BUS_DMASYNC_POSTWRITE,
            );
        }

        bus_dmamap_sync(
            sc.dmat(),
            dmap,
            0,
            dmap.dm_mapsize.get(),
            if flags & SCSI_DATA_IN != 0 {
                BUS_DMASYNC_POSTREAD
            } else {
                BUS_DMASYNC_POSTWRITE
            },
        );

        bus_dmamap_unload(sc.dmat(), dmap);
    }

    xs.status.set(c.scsi_status as u8);
    let (error, resid, sense) = vmwpvs_cmp_result(c, xs.datalen());
    if sense {
        let mut s = xs.sense.get();
        let bytes = s.as_bytes_mut();
        // SAFETY: the opening's sense buffer holds VMWPVS_SENSELEN bytes, at least
        // sizeof(struct scsi_sense_data), in the sense area; the device wrote it by DMA.
        unsafe {
            ptr::copy_nonoverlapping(ccb.ccb_sense.get(), bytes.as_mut_ptr(), bytes.len());
        }
        xs.sense.set(s);
    }
    if error == XS_DRIVER_STUFFUP {
        let (h, s) = (c.host_status, c.scsi_status);
        printf(format_args!(
            "{}: vmwpvs_scsi_cmd_done:{} h:0x{:x} s:0x{:x}\n",
            sc.devname(),
            line!(),
            h,
            s
        ));
    }
    xs.error.set(error);
    if let Some(resid) = resid {
        xs.resid.set(resid);
    }

    ccb
}

/// `vmwpvs_ccb_get`: an opening from the free list.
///
/// # Safety
///
/// `xsc` is the softc of an attached vmwpvs.
pub unsafe fn vmwpvs_ccb_get(xsc: *mut c_void) -> Option<ScsiIo> {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*xsc.cast::<VmwpvsSoftc>().cast_const() };

    mtx_enter(&sc.sc_ccb_mtx);
    let ccb = sc.sc_ccb_list.first().map(NonNull::from);
    if ccb.is_some() {
        // SAFETY: the list is not empty.
        unsafe { sc.sc_ccb_list.remove_head() };
    }
    mtx_leave(&sc.sc_ccb_mtx);

    ccb.map(NonNull::cast)
}

/// `vmwpvs_ccb_put`: gives an opening back to the free list.
///
/// # Safety
///
/// `xsc` is the softc of an attached vmwpvs and `io` one of its openings, on no list.
pub unsafe fn vmwpvs_ccb_put(xsc: *mut c_void, io: ScsiIo) {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*xsc.cast::<VmwpvsSoftc>().cast_const() };
    // SAFETY: the caller's guarantee: an opening of `sc_ccbs`, which stays in place.
    let ccb = unsafe { io.cast::<VmwpvsCcb>().as_ref() };

    mtx_enter(&sc.sc_ccb_mtx);
    // SAFETY: on no list (the caller's guarantee), and never moved or freed while attached.
    unsafe { sc.sc_ccb_list.insert_head(ccb) };
    mtx_leave(&sc.sc_ccb_mtx);
}

/// `vmwpvs_dmamem_alloc`: `size` bytes of DMA memory in one segment, mapped and loaded.
pub fn vmwpvs_dmamem_alloc(sc: &VmwpvsSoftc, size: usize) -> Option<&'static VmwpvsDmamem> {
    let dmat = sc.dmat();

    let mem = malloc(size_of::<VmwpvsDmamem>(), M_DEVBUF, M_NOWAIT | M_ZERO)?;

    let Ok(map) = bus_dmamap_create(dmat, size, 1, size, 0, BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW)
    else {
        // dmfree:
        free(mem, M_DEVBUF, size_of::<VmwpvsDmamem>());
        return None;
    };
    let destroy = |mem: NonNull<u8>| {
        // SAFETY: the map created above, unused.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
        free(mem, M_DEVBUF, size_of::<VmwpvsDmamem>());
    };

    let mut segs = [BusDmaSegment::default(); 1];
    let Ok(nsegs) = bus_dmamem_alloc(
        dmat,
        size,
        PAGE_SIZE,
        0,
        &mut segs,
        BUS_DMA_NOWAIT | BUS_DMA_ZERO,
    ) else {
        destroy(mem);
        return None;
    };

    let Ok(kva) = bus_dmamem_map(dmat, &mut segs[..nsegs], size, BUS_DMA_NOWAIT) else {
        // free:
        // SAFETY: the segment allocated above, not mapped.
        unsafe { bus_dmamem_free(dmat, &segs[..nsegs]) };
        destroy(mem);
        return None;
    };

    // SAFETY: `kva` maps `size` bytes allocated above, which stay until vmwpvs_dmamem_free
    // unloads the map first.
    if unsafe { bus_dmamap_load(dmat, map, kva.as_ptr(), size, None, BUS_DMA_NOWAIT) }.is_err() {
        // unmap:
        // SAFETY: the mapping and the segment made above, unused.
        unsafe {
            bus_dmamem_unmap(dmat, kva, size);
            bus_dmamem_free(dmat, &segs[..nsegs]);
        }
        destroy(mem);
        return None;
    }

    let dm = mem.cast::<VmwpvsDmamem>();
    // SAFETY: a fresh allocation of `size_of::<VmwpvsDmamem>()` bytes (malloc aligns it),
    // written whole before any use; it lives until vmwpvs_dmamem_free.
    unsafe {
        dm.as_ptr().write(VmwpvsDmamem {
            dm_map: map,
            dm_seg: segs[0],
            dm_size: size,
            dm_kva: kva,
        });
        Some(&*dm.as_ptr())
    }
}

/// `vmwpvs_dmamem_zalloc`: as [`vmwpvs_dmamem_alloc`], the memory zeroed.
pub fn vmwpvs_dmamem_zalloc(sc: &VmwpvsSoftc, size: usize) -> Option<&'static VmwpvsDmamem> {
    let dm = vmwpvs_dmamem_alloc(sc, size)?;

    // SAFETY: the area maps `size` bytes, ours until it is freed.
    unsafe { ptr::write_bytes(vmwpvs_dma_kva::<u8>(dm), 0, size) };

    Some(dm)
}

/// `vmwpvs_dmamem_free`.
///
/// # Safety
///
/// `dm` came from [`vmwpvs_dmamem_alloc`], the device no longer uses it and nothing
/// references it afterwards.
pub unsafe fn vmwpvs_dmamem_free(sc: &VmwpvsSoftc, dm: &'static VmwpvsDmamem) {
    let dmat = sc.dmat();

    bus_dmamap_unload(dmat, dm.dm_map);
    // SAFETY: the caller's guarantee: the area's own mapping, segment and map, unloaded.
    unsafe {
        bus_dmamem_unmap(dmat, dm.dm_kva, dm.dm_size);
        bus_dmamem_free(dmat, core::slice::from_ref(&dm.dm_seg));
        bus_dmamap_destroy(dmat, NonNull::from(dm.dm_map));
    }
    free(
        NonNull::from(dm).cast(),
        M_DEVBUF,
        size_of::<VmwpvsDmamem>(),
    );
}

const _: () = {
    assert!(size_of::<VmwpvwRingState>() == 140);
    assert!(size_of::<VmwpvsRingReq>() == 128);
    assert!(size_of::<VmwpvsRingCmp>() == 32);
    assert!(size_of::<VmwpvsSge>() == 16);
    assert!(size_of::<VmwpvsRingMsg>() == 128);
    assert!(size_of::<VmwpvsRingMsgDev>() == 128);
    assert!(size_of::<VmwpvsCfgCmd>() == 24);
    assert!(size_of::<VmwpvsSetupRingsCmd>() == 528);
    assert!(size_of::<VmwpvsSetupRingsMsg>() == 136);
    assert!(size_of::<VmwpvsCfgPgHeader>() == 16);
    assert!(size_of::<VmwpvsCfgPgController>() == 480);
    assert!(VMWPVS_REQ_COUNT == 64 && VMWPVS_CMP_COUNT == 256 && VMWPVS_MSG_COUNT == 64);
    assert!(VMMPVS_PG_LEN == 0x8000);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_match_the_packed_c_structures() {
        assert_eq!(core::mem::offset_of!(VmwpvwRingState, msg_prod), 32 * 4);
        assert_eq!(core::mem::offset_of!(VmwpvsRingReq, cdb), 40);
        assert_eq!(core::mem::offset_of!(VmwpvsRingReq, cdblen), 56);
        assert_eq!(core::mem::offset_of!(VmwpvsRingReq, lun), 57);
        assert_eq!(core::mem::offset_of!(VmwpvsRingReq, target), 67);
        assert_eq!(core::mem::offset_of!(VmwpvsRingCmp, host_status), 20);
        assert_eq!(core::mem::offset_of!(VmwpvsRingMsgDev, __pad), 20);
        assert_eq!(core::mem::offset_of!(VmwpvsSetupRingsCmd, state_ppn), 8);
        assert_eq!(core::mem::offset_of!(VmwpvsCfgPgController, num_phys), 472);
        assert_eq!(VMWPVS_SENSELEN % 16, 0);
        assert!(VMWPVS_SENSELEN >= size_of::<ScsiSenseData>());
    }

    #[test]
    fn command_words_are_the_structure_in_order() {
        let mut cmd = VmwpvsCfgCmd::zeroed();
        cmd.cmp_addr = 0x1122_3344_5566_7788;
        cmd.pg_addr_type = VMWPVS_CFGPGADDR_CONTROLLER;
        cmd.pg_num = VMWPVS_CFGPG_CONTROLLER;
        let words: std::vec::Vec<u32> = cmd
            .as_bytes()
            .chunks_exact(4)
            .map(|w| u32::from_ne_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        assert_eq!(words.len(), 6);
        assert_eq!(words[3], VMWPVS_CFGPGADDR_CONTROLLER);
        assert_eq!(words[4], VMWPVS_CFGPG_CONTROLLER);
    }

    #[test]
    fn config_page_status() {
        let mut hdr = VmwpvsCfgPgHeader::zeroed();
        assert!(vmwpvs_cfg_ok(&hdr));
        // What vmwpvs_get_config preloads: a device that ignores the command fails it.
        hdr.host_status = VMWPVS_HOST_STATUS_INVPARAM;
        hdr.scsi_status = VMWPVS_SCSI_STATUS_CHECK;
        assert!(!vmwpvs_cfg_ok(&hdr));
    }

    #[test]
    fn completion_outcomes() {
        let mut c = VmwpvsRingCmp::zeroed();
        assert_eq!(vmwpvs_cmp_result(&c, 512), (XS_NOERROR, Some(0), false));
        c.scsi_status = VMWPVS_SCSI_STATUS_CHECK;
        assert_eq!(vmwpvs_cmp_result(&c, 512), (XS_SENSE, Some(0), true));
        c.scsi_status = 0;
        c.host_status = VMWPVS_HOST_STATUS_UNDERRUN;
        c.data_len = 200;
        assert_eq!(vmwpvs_cmp_result(&c, 512), (XS_NOERROR, Some(312), false));
        c.host_status = VMWPVS_HOST_STATUS_SELTIMEOUT;
        assert_eq!(vmwpvs_cmp_result(&c, 512), (XS_SELTIMEOUT, None, false));
        c.host_status = VMWPVS_HOST_STATUS_BUS_RESET;
        assert_eq!(vmwpvs_cmp_result(&c, 512), (XS_DRIVER_STUFFUP, None, false));
    }

    #[test]
    fn request_direction() {
        assert_eq!(vmwpvs_req_dir(SCSI_DATA_IN), VMWPVS_REQ_DIR_IN);
        assert_eq!(
            vmwpvs_req_dir(SCSI_DATA_OUT | SCSI_POLL),
            VMWPVS_REQ_DIR_OUT
        );
        assert_eq!(vmwpvs_req_dir(0), VMWPVS_REQ_DIR_NONE);
    }
}
/* </TESTS> */
