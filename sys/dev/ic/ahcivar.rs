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
 * Copyright (c) 2006 David Gwynne <dlg@openbsd.org>
 * Copyright (c) 2010 Conformal Systems LLC <info@conformal.com>
 * Copyright (c) 2010 Jonathan Matthew <jonathan@d14n.org>
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
//! `<dev/ic/ahcivar.h>`: the `ahci(4)` driver's state: DMA memory, command control blocks,
//! ports and the softc shared by the bus front-ends (`ahci_pci`; `ahci_fdt` and `ahci_acpi`
//! are not ported).
//!
//! Upstream: sys/dev/ic/ahcivar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `AHCI_DEBUG` is not defined (`NO_AHCI_DEBUG`): `ap_name` does not exist and `PORTNAME(ap)`
//!   is `DEVNAME(ap->ap_sc)`. `AHCI_COALESCE` is not defined: `ap_num` and the `sc_ccc_*`
//!   members do not exist. `ap_err_busy` exists with feature `diagnostic` (`DIAGNOSTIC`).
//! - `struct ahci_dmamem` and `struct ahci_port` are `malloc(9)`ed as in C and written whole
//!   once allocated (`ahci_dmamem_alloc`, `ahci_port_alloc`); the port's members the C fills
//!   in later are `Cell`s. The ccb array is `mallocarray`ed and each ccb written whole when
//!   its DMA map exists; only ccbs written that way are ever on a list, which is all
//!   `ahci_port_free` walks.
//! - The ccb's command header and command table, and the port's received FIS area, are DMA
//!   memory: raw pointers, read and written through the volatile accessors of [`AhciCcb`]
//!   and [`AhciPort::rfis`] (no reference into DMA memory). `htolem16`/`htolem64`/`lemtoh32`
//!   are `to_le`/`from_le` in those accessors.
//! - The `volatile u_int32_t` members (`ap_active`, `ap_active_cnt`, `ap_sactive`,
//!   `ap_pmp_ncq_port`) are `AtomicU32`s read and written `Relaxed`.
//! - `sc_ports` is an array of `Cell<Option<NonNull<AhciPort>>>` (zeroed is `None`) behind
//!   [`AhciSoftc::port`]; `sc_atascsi` an `Option` of the atascsi `atascsi_attach` returned;
//!   `sc_port_start` returns `Result<(), Errno>`.
//! - `AHCI_DMA_MAP`, `AHCI_DMA_DVA`, `AHCI_DMA_KVA` and `ahci_port_start(_p, _f)` are inline
//!   functions; `DEVNAME(_s)` is `sc.sc_dev.xname()`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::AtomicU32;

use crate::dev::ata::atascsi::{AtaFisD2h, AtaFisH2d, AtaXfer, Atascsi};
use crate::dev::ic::ahcireg::{AHCI_MAX_PORTS, AhciCmdHdr, AhciCmdTable, AhciPrdt, AhciRfis};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BusDmaSegment, BusDmaTag, BusDmamap, BusSize, BusSpaceHandle, BusSpaceTag,
};
use crate::queue_adapter;
use crate::sys::device::{Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::mutex::Mutex;
use crate::sys::queue::{TailqEntry, TailqHead};

/// `struct ahci_dmamem`: one physically contiguous, mapped and loaded DMA area.
pub struct AhciDmamem {
    /// `adm_map`.
    pub adm_map: &'static BusDmamap,
    /// `adm_seg`.
    pub adm_seg: BusDmaSegment,
    /// `adm_size`.
    pub adm_size: usize,
    /// `adm_kva`.
    pub adm_kva: NonNull<u8>,
}

/// `ccb_done`: what completes a command.
pub type AhciDoneFn = fn(ccb: &'static AhciCcb);

/// `struct ahci_ccb`: a command slot of a port.
#[repr(C)]
pub struct AhciCcb {
    /// `ccb_xa`: the ATA xfer associated with this CCB. Must be 1st struct member.
    pub ccb_xa: AtaXfer,

    /// `ccb_slot`.
    pub ccb_slot: i32,
    /// `ccb_port`.
    pub ccb_port: &'static AhciPort,

    /// `ccb_dmamap`.
    pub ccb_dmamap: &'static BusDmamap,
    /// `ccb_cmd_hdr`: the slot's command list entry (DMA memory).
    pub ccb_cmd_hdr: *mut AhciCmdHdr,
    /// `ccb_cmd_table`: the slot's command table (DMA memory).
    pub ccb_cmd_table: *mut AhciCmdTable,

    /// `ccb_done`.
    pub ccb_done: Cell<AhciDoneFn>,

    /// `ccb_entry`: `ap_ccb_free` or `ap_ccb_pending`.
    pub ccb_entry: TailqEntry<AhciCcb>,
}

queue_adapter!(
    /// `TAILQ_HEAD(, ahci_ccb)`: `ap_ccb_free` and `ap_ccb_pending`, through `ccb_entry`.
    pub AhciCcbList: AhciCcb, ccb_entry => TailqEntry<AhciCcb>
);

impl AhciCcb {
    /// `ccb->ccb_cmd_hdr->flags = htole16(flags)` (`htolem16`).
    pub fn set_hdr_flags(&self, flags: u16) {
        // SAFETY: `ccb_cmd_hdr` is this slot's entry of the port's command list, which lives
        // as long as the port; the controller reads it only once the slot is issued, and the
        // pointer is aligned (the list is page-aligned, entries 32 bytes).
        unsafe { ptr::write_volatile(ptr::addr_of_mut!((*self.ccb_cmd_hdr).flags), flags.to_le()) }
    }

    /// `ccb->ccb_cmd_hdr->prdtl = htole16(prdtl)`.
    pub fn set_hdr_prdtl(&self, prdtl: u16) {
        // SAFETY: as for `set_hdr_flags`.
        unsafe { ptr::write_volatile(ptr::addr_of_mut!((*self.ccb_cmd_hdr).prdtl), prdtl.to_le()) }
    }

    /// `ccb->ccb_cmd_hdr->prdbc = htole32(prdbc)`.
    pub fn set_hdr_prdbc(&self, prdbc: u32) {
        // SAFETY: as for `set_hdr_flags`.
        unsafe { ptr::write_volatile(ptr::addr_of_mut!((*self.ccb_cmd_hdr).prdbc), prdbc.to_le()) }
    }

    /// `lemtoh32(&ccb->ccb_cmd_hdr->prdbc)`: the bytes the controller transferred.
    pub fn hdr_prdbc(&self) -> u32 {
        // SAFETY: as for `set_hdr_flags`; the controller writes the count, hence volatile.
        u32::from_le(unsafe { ptr::read_volatile(ptr::addr_of!((*self.ccb_cmd_hdr).prdbc)) })
    }

    /// `htolem64(&ccb->ccb_cmd_hdr->ctba, ctba)`.
    pub fn set_hdr_ctba(&self, ctba: u64) {
        // SAFETY: as for `set_hdr_flags`.
        unsafe { ptr::write_volatile(ptr::addr_of_mut!((*self.ccb_cmd_hdr).ctba), ctba.to_le()) }
    }

    /// `memset(ccb->ccb_cmd_table, 0, sizeof(struct ahci_cmd_table))`.
    pub fn zero_cmd_table(&self) {
        // SAFETY: `ccb_cmd_table` is this slot's 512-byte command table, which lives as long
        // as the port and which the controller reads only once the slot is issued.
        unsafe {
            ptr::write_bytes(
                self.ccb_cmd_table.cast::<u8>(),
                0,
                size_of::<AhciCmdTable>(),
            )
        }
    }

    /// `ccb->ccb_cmd_table->cfis[i] = v`.
    pub fn set_cfis(&self, i: usize, v: u8) {
        if i >= 64 {
            panic(format_args!("ahci: cfis byte {i} out of range"));
        }
        // SAFETY: as for `zero_cmd_table`; `i` is within the 64-byte `cfis`.
        unsafe {
            ptr::write_volatile(
                ptr::addr_of_mut!((*self.ccb_cmd_table).cfis)
                    .cast::<u8>()
                    .add(i),
                v,
            )
        }
    }

    /// `ccb->ccb_cmd_table->cfis` as a FIS (`(struct ata_fis_h2d *)cfis`).
    pub fn cfis(&self) -> *mut AtaFisH2d {
        // SAFETY: only the address is taken; `cfis` is the first member of the table.
        unsafe { ptr::addr_of_mut!((*self.ccb_cmd_table).cfis).cast() }
    }

    /// `ccb->ccb_cmd_table->acmd`.
    pub fn acmd(&self) -> *mut u8 {
        // SAFETY: only the address is taken.
        unsafe { ptr::addr_of_mut!((*self.ccb_cmd_table).acmd).cast() }
    }

    /// `ahci_load_prdt_seg(&ccb->ccb_cmd_table->prdt[i], ...)`'s store.
    pub fn set_prdt(&self, i: usize, prd: AhciPrdt) {
        if i >= crate::dev::ic::ahcireg::AHCI_MAX_PRDT {
            panic(format_args!("ahci: prdt entry {i} out of range"));
        }
        // SAFETY: as for `zero_cmd_table`; `i` is below `AHCI_MAX_PRDT`, and the entries are
        // 8-byte aligned in the 128-byte-aligned table.
        unsafe {
            ptr::write_volatile(
                ptr::addr_of_mut!((*self.ccb_cmd_table).prdt)
                    .cast::<AhciPrdt>()
                    .add(i),
                prd,
            )
        }
    }
}

/// `AP_S_NORMAL`.
pub const AP_S_NORMAL: u32 = 0;
/// `AP_S_PMP_PROBE`.
pub const AP_S_PMP_PROBE: u32 = 1;
/// `AP_S_PMP_PORT_PROBE`.
pub const AP_S_PMP_PORT_PROBE: u32 = 2;
/// `AP_S_ERROR_RECOVERY`.
pub const AP_S_ERROR_RECOVERY: u32 = 3;
/// `AP_S_FATAL_ERROR`.
pub const AP_S_FATAL_ERROR: u32 = 4;

/// `struct ahci_port`.
pub struct AhciPort {
    /// `ap_sc`: set once the port's register window exists.
    pub ap_sc: Cell<Option<&'static AhciSoftc>>,
    /// `ap_ioh`: the port's register window.
    pub ap_ioh: Cell<Option<BusSpaceHandle>>,

    /// `ap_rfis`: the received FIS area (DMA memory).
    pub ap_rfis: Cell<*mut AhciRfis>,
    /// `ap_dmamem_rfis`.
    pub ap_dmamem_rfis: Cell<Option<&'static AhciDmamem>>,

    /// `ap_dmamem_cmd_list`.
    pub ap_dmamem_cmd_list: Cell<Option<&'static AhciDmamem>>,
    /// `ap_dmamem_cmd_table`.
    pub ap_dmamem_cmd_table: Cell<Option<&'static AhciDmamem>>,

    /// `ap_active`: the standard commands on the chip.
    pub ap_active: AtomicU32,
    /// `ap_active_cnt`.
    pub ap_active_cnt: AtomicU32,
    /// `ap_sactive`: the NCQ commands on the chip.
    pub ap_sactive: AtomicU32,
    /// `ap_pmp_ncq_port`.
    pub ap_pmp_ncq_port: AtomicU32,
    /// `ap_ccbs`: `sc_ncmds` ccbs.
    pub ap_ccbs: Cell<*mut AhciCcb>,

    /// `ap_ccb_free`. Protected by `ap_ccb_mtx`.
    pub ap_ccb_free: TailqHead<AhciCcbList>,
    /// `ap_ccb_pending`. Protected by `splbio`.
    pub ap_ccb_pending: TailqHead<AhciCcbList>,
    /// `ap_ccb_mtx`.
    pub ap_ccb_mtx: Mutex,
    /// `ap_ccb_err`: the ccb kept for error recovery.
    pub ap_ccb_err: Cell<Option<&'static AhciCcb>>,

    /// `ap_state`: `AP_S_*`.
    pub ap_state: Cell<u32>,

    /// `ap_pmp_ports`.
    pub ap_pmp_ports: Cell<i32>,
    /// `ap_port`.
    pub ap_port: Cell<i32>,
    /// `ap_pmp_ignore_ifs`.
    pub ap_pmp_ignore_ifs: Cell<i32>,

    /// `ap_err_busy` (`DIAGNOSTIC`).
    #[cfg(feature = "diagnostic")]
    pub ap_err_busy: Cell<i32>,
    /// `ap_err_saved_sactive`.
    pub ap_err_saved_sactive: Cell<u32>,
    /// `ap_err_saved_active`.
    pub ap_err_saved_active: Cell<u32>,
    /// `ap_err_saved_active_cnt`.
    pub ap_err_saved_active_cnt: Cell<u32>,
    /// `ap_saved_cmd`.
    pub ap_saved_cmd: Cell<u32>,

    /// `ap_err_scratch`: `DEV_BSIZE` bytes for the NCQ error log page.
    pub ap_err_scratch: Cell<*mut u8>,
}

impl AhciPort {
    /// A new port, as `malloc(M_ZERO)` leaves it.
    pub const fn new() -> Self {
        Self {
            ap_sc: Cell::new(None),
            ap_ioh: Cell::new(None),
            ap_rfis: Cell::new(ptr::null_mut()),
            ap_dmamem_rfis: Cell::new(None),
            ap_dmamem_cmd_list: Cell::new(None),
            ap_dmamem_cmd_table: Cell::new(None),
            ap_active: AtomicU32::new(0),
            ap_active_cnt: AtomicU32::new(0),
            ap_sactive: AtomicU32::new(0),
            ap_pmp_ncq_port: AtomicU32::new(0),
            ap_ccbs: Cell::new(ptr::null_mut()),
            ap_ccb_free: TailqHead::new(),
            ap_ccb_pending: TailqHead::new(),
            ap_ccb_mtx: Mutex::new(0),
            ap_ccb_err: Cell::new(None),
            ap_state: Cell::new(AP_S_NORMAL),
            ap_pmp_ports: Cell::new(0),
            ap_port: Cell::new(0),
            ap_pmp_ignore_ifs: Cell::new(0),
            #[cfg(feature = "diagnostic")]
            ap_err_busy: Cell::new(0),
            ap_err_saved_sactive: Cell::new(0),
            ap_err_saved_active: Cell::new(0),
            ap_err_saved_active_cnt: Cell::new(0),
            ap_saved_cmd: Cell::new(0),
            ap_err_scratch: Cell::new(ptr::null_mut()),
        }
    }

    /// `ap->ap_sc`, which `ahci_port_alloc` sets before using the port.
    pub fn sc(&self) -> &'static AhciSoftc {
        match self.ap_sc.get() {
            Some(sc) => sc,
            None => panic(format_args!("ahci: port without a controller")),
        }
    }

    /// `PORTNAME(ap)`: `DEVNAME(ap->ap_sc)` without `AHCI_DEBUG`.
    pub fn portname(&self) -> &'static str {
        self.sc().sc_dev.xname()
    }

    /// `&ap->ap_ccbs[slot]`; a slot past `sc_ncmds` panics where the C would index past the
    /// array.
    pub fn ccb(&self, slot: u32) -> &'static AhciCcb {
        let ccbs = self.ap_ccbs.get();
        if ccbs.is_null() || slot >= self.sc().sc_ncmds.get() {
            panic(format_args!("{}: bad ccb slot {slot}", self.portname()));
        }
        // SAFETY: `ahci_port_alloc` allocated `sc_ncmds` ccbs at `ap_ccbs` and wrote every
        // one it puts on a list; the slots named by the controller's CI/SACT bits and by the
        // ccb accessors are ccbs that were issued, hence written. They stay until
        // `ahci_port_free`.
        unsafe { &*ccbs.add(slot as usize) }
    }

    /// `memcpy(&xa->rfis, ap->ap_rfis->rfis, sizeof(struct ata_fis_d2h))`: the D2H Register
    /// FIS the device last sent.
    pub fn rfis(&self) -> AtaFisD2h {
        let rfis = self.ap_rfis.get();
        if rfis.is_null() {
            panic(format_args!("{}: no received FIS area", self.portname()));
        }
        // SAFETY: `ap_rfis` is the port's 256-byte received FIS area, which lives as long as
        // the port; the controller writes it, hence volatile; `AtaFisD2h` has alignment 1 and
        // `rfis` is 24 bytes.
        unsafe { ptr::read_volatile(ptr::addr_of!((*rfis).rfis).cast::<AtaFisD2h>()) }
    }
}

impl Default for AhciPort {
    fn default() -> Self {
        Self::new()
    }
}

/// `AHCI_F_NO_NCQ`.
pub const AHCI_F_NO_NCQ: i32 = 1 << 0;
/// `AHCI_F_IPMS_PROBE`: IPMS on failed PMP probe.
pub const AHCI_F_IPMS_PROBE: i32 = 1 << 1;
/// `AHCI_F_NO_PMP`: ignore PMP capability.
pub const AHCI_F_NO_PMP: i32 = 1 << 2;
/// `AHCI_F_NO_MSI`: disable MSI.
pub const AHCI_F_NO_MSI: i32 = 1 << 3;

/// `sc_port_start`: starts a port's command (and FIS receive) engine.
pub type AhciPortStartFn = fn(ap: &AhciPort, fre_only: bool) -> Result<(), Errno>;

/// `struct ahci_softc`.
#[repr(C)]
pub struct AhciSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,

    /// `sc_ih`: the interrupt handle, or null.
    pub sc_ih: Cell<*mut c_void>,

    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_ios`.
    pub sc_ios: Cell<BusSize>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,

    /// `sc_flags`: `AHCI_F_*`.
    pub sc_flags: Cell<i32>,

    /// `sc_ncmds`: command slots per port.
    pub sc_ncmds: Cell<u32>,

    /// `sc_ports`.
    pub sc_ports: [Cell<Option<NonNull<AhciPort>>>; AHCI_MAX_PORTS],

    /// `sc_atascsi`.
    pub sc_atascsi: Cell<Option<&'static Atascsi>>,

    /// `sc_cap`.
    pub sc_cap: Cell<u32>,

    /// `sc_port_start`.
    pub sc_port_start: Cell<Option<AhciPortStartFn>>,
}

impl AhciSoftc {
    /// `sc->sc_iot` and `sc->sc_ioh`, which the bus front-end maps before `ahci_attach`.
    pub fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.sc_iot.get(), self.sc_ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!(
                "{}: registers not mapped",
                self.sc_dev.xname()
            )),
        }
    }

    /// `sc->sc_dmat`, which the bus front-end sets before `ahci_attach`.
    pub fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => panic(format_args!("{}: no DMA tag", self.sc_dev.xname())),
        }
    }

    /// `sc->sc_ports[port]`.
    pub fn port(&self, port: usize) -> Option<&'static AhciPort> {
        let ap = self.sc_ports.get(port)?.get()?;
        // SAFETY: `ahci_port_alloc` stores a port it allocated and wrote, which stays until
        // `ahci_port_free` clears the entry.
        Some(unsafe { ap.as_ref() })
    }
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of raw pointers,
// integers, `Option`s of references, `NonNull`s, function pointers and bus tags and handles,
// all-zero valid as in `NvmeSoftc`.
unsafe impl Softc for AhciSoftc {}

/// `AHCI_DMA_MAP(_adm)`.
#[inline]
pub fn ahci_dma_map(adm: &AhciDmamem) -> &'static BusDmamap {
    adm.adm_map
}

/// `AHCI_DMA_DVA(_adm)`: the device address of the loaded segment.
#[inline]
pub fn ahci_dma_dva(adm: &AhciDmamem) -> u64 {
    adm.adm_map
        .dm_segs()
        .first()
        .map_or(0, |s| s.get().ds_addr as u64)
}

/// `AHCI_DMA_KVA(_adm)`.
#[inline]
pub fn ahci_dma_kva(adm: &AhciDmamem) -> *mut u8 {
    adm.adm_kva.as_ptr()
}

/// `ahci_port_start(_p, _f)`: `sc_port_start` of the port's controller.
#[inline]
pub fn ahci_port_start(ap: &AhciPort, fre_only: bool) -> Result<(), Errno> {
    match ap.sc().sc_port_start.get() {
        Some(f) => f(ap, fre_only),
        None => panic(format_args!("{}: no sc_port_start", ap.portname())),
    }
}
/* </CODE> */
