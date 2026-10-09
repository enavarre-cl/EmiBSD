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
 * Copyright (c) 2005, 2006, 2009 David Gwynne <dlg@openbsd.org>
 * Copyright (c) 2005, 2008, 2009 Marco Peereboom <marco@openbsd.org>
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
//! `mpi(4)`: the LSI Fusion-MPT (MPI 1.5) SCSI, Fibre Channel and SAS host adapter driver
//! (also QEMU's `mptsas1068`): IOC bring-up through the doorbell handshake, request and
//! reply frames, the scsibus adapter, configuration pages, event handling, the SPI
//! domain validation, bio(4) and the volume sensors.
//!
//! Upstream: sys/dev/ic/mpi.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `MPI_DEBUG` is not defined by the file: `mpi_debug`, `DPRINTF`, `DNPRINTF` and the
//!   `#ifdef MPI_DEBUG` page-0 read in `mpi_ppr` are not ported.
//! - Functions that return 0 or 1 in C return `Result<(), Errno>` (`EIO` for the 1); the
//!   ones that return an errno (`mpi_ppr`, `mpi_scsi_probe`, the ioctls) return it.
//! - `ccb_cookie` is [`MpiCookie`] and `mpi_poll`'s `rv` is the ccb's `ccb_poll_rv`
//!   (`mpivar.rs`): the C keeps the address of a stack variable in the ccb and, after a
//!   timeout, leaves it there, so a late completion would write into a dead frame.
//! - `mpi_req_cfg_header` and `mpi_req_cfg_page` take the header type as a generic
//!   ([`MpiCfgHdrKind`]: `struct mpi_cfg_hdr` or `struct mpi_ecfg_hdr`) instead of a
//!   `void *` and the `MPI_PG_EXTENDED` flag (which the type now states); the page is a
//!   structure or a byte slice ([`MpiPod`]). `mpi_cfg_header`, `mpi_ecfg_header`,
//!   `mpi_cfg_page` and `mpi_ecfg_page` are functions.
//! - The message structures the C builds in place in the request frame are reached through
//!   `MpiCcb::cmd` (a `&mut` to the frame's DMA memory, owned by the ccb's holder); the
//!   replies are copied out of the reply frame by value ([`mpi_rcb_reply`]).
//! - `mpi_alloc_replies` allocates the reply control blocks zeroed (`M_ZERO`), where the C
//!   leaves them uninitialised until `mpi_push_replies`.
//! - `ccb_state` double free check of `mpi_put_ccb` is `DIAGNOSTIC`, as feature
//!   `diagnostic`.
//! - `mpi_timeout_xs` is the C's empty function (`/* XXX */`).
//! - `mpi_ioctl_cache` keeps the C's cast of the `mpi_rcb` (not its reply) to
//!   `struct mpi_msg_raid_action_reply` (External bugs): `action_status` is read from
//!   the rcb's bytes at the reply structure's offset, as the C reads them.
//! - The ccb that `mpi_portenable`, `mpi_req_cfg_header`, `mpi_req_cfg_page` and
//!   `mpi_ioctl_cache` do not give back when `mpi_poll` fails (they `return`/`goto done`
//!   without `scsi_io_put`) stays leaked, as in C.
//! - `bus_dma` for the request and reply frames is 32-bit (no `BUS_DMA_64BIT`), as in C
//!   (`MPI_DMA_DVA` truncated to `u32` in the frames).

use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};

use crate::dev::bio::bio_register;
use crate::dev::biovar::{
    BIOC_SDFAILED, BIOC_SDINVALID, BIOC_SDOFFLINE, BIOC_SDONLINE, BIOC_SDSCRUB, BIOC_SVDEGRADED,
    BIOC_SVINVALID, BIOC_SVOFFLINE, BIOC_SVONLINE, BIOC_SVREBUILD, BIOCALARM, BIOCBLINK, BIOCDISK,
    BIOCINQ, BIOCSETSTATE, BIOCVOL, BiocDisk, BiocInq, BiocVol,
};
use crate::dev::ic::mpireg::*;
use crate::dev::ic::mpivar::*;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_sensors::{sensor_attach, sensor_task_register, sensordev_install};
use crate::kern::kern_synch::{msleep_nsec, wakeup_one};
use crate::kern::kern_task::{SYSTQ, task_add, task_set};
use crate::kern::kern_timeout::timeout_set;
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_ALLOCNOW, BUS_DMA_NOWAIT, BUS_DMA_STREAMING, BUS_DMA_WAITOK, BUS_DMA_ZERO,
    BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE,
    BUS_SPACE_BARRIER_READ, BUS_SPACE_BARRIER_WRITE, BusDmaSegment, BusSize, bus_dmamap_create,
    bus_dmamap_destroy, bus_dmamap_load, bus_dmamap_sync, bus_dmamap_unload, bus_dmamem_alloc,
    bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap, bus_space_barrier, bus_space_read_4,
    bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::machine::intr::IPL_BIO;
use crate::scsi::scsi_all::{
    INQUIRY, SCSI_BUSY, SCSI_CHECK, SCSI_OK, SCSI_QUEUE_FULL, SCSI_TERMINATED, SID_TYPE,
    SKEY_ILLEGAL_REQUEST, SSD_ERRCODE_CURRENT, SSD_ERRCODE_VALID, ScsiInquiry, ScsiInquiryData,
    ScsiSenseData, T_PROCESSOR,
};
use crate::scsi::scsi_base::{
    scsi_done, scsi_io_get, scsi_io_put, scsi_ioh_add, scsi_ioh_set, scsi_iopool_init,
    scsi_req_detach, scsi_req_probe,
};
use crate::scsi::scsiconf::{
    _lto2b, SCSI_DATA_IN, SCSI_DATA_OUT, SCSI_NOSLEEP, SCSI_POLL, SDEV_ATAPI, SDEV_NOSYNC,
    SDEV_NOTAGS, SDEV_VIRTUAL, ScsiAdapter, ScsiIo, ScsiLink, ScsiXfer, ScsibusAttachArgs,
    ScsibusSoftc, XS_BUSY, XS_DRIVER_STUFFUP, XS_NOERROR, XS_RESET, XS_SELTIMEOUT, XS_SENSE,
    scsi_activate, scsi_detach_target, scsi_get_link, scsi_probe_target, scsi_strvis, scsiprint,
};
use crate::sys::device::{Cfdriver, DETACH_FORCE, DV_DULL, DVACT_DEACTIVATE, Device};
use crate::sys::dkio::{DIOCGCACHE, DIOCSCACHE, DkCache};
use crate::sys::errno::Errno;
use crate::sys::ioccom::iocparm_len;
use crate::sys::malloc::{M_CANFAIL, M_DEVBUF, M_NOWAIT, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::mutex::Mutex;
use crate::sys::param::{MAXPHYS, PAGE_SIZE, PRIBIO};
use crate::sys::sensors::{
    Ksensor, SENSOR_DRIVE, SENSOR_DRIVE_FAIL, SENSOR_DRIVE_ONLINE, SENSOR_DRIVE_PFAIL,
    SENSOR_DRIVE_REBUILD, SENSOR_S_CRIT, SENSOR_S_OK, SENSOR_S_UNKNOWN, SENSOR_S_WARN,
};
use crate::sys::systm::{INFSLP, kernel_lock, kernel_unlock};
use libkern::strlcpy;

/// `mpi_cd`.
pub static MPI_CD: Cfdriver = Cfdriver::new(b"mpi", DV_DULL, 0);

/// `mpi_switch`: the adapter's entry points.
pub static MPI_SWITCH: ScsiAdapter = ScsiAdapter {
    scsi_cmd: mpi_scsi_cmd,
    dev_minphys: None,
    dev_probe: Some(mpi_scsi_probe),
    dev_free: None,
    ioctl: Some(mpi_scsi_ioctl),
};

/// `MPI_PG_EXTENDED`.
pub const MPI_PG_EXTENDED: i32 = 1 << 0;
/// `MPI_PG_POLL`.
pub const MPI_PG_POLL: i32 = 1 << 1;
/// `MPI_PG_FMT`.
pub const MPI_PG_FMT: &[u8] = b"\x10\x02POLL\x01EXTENDED";

/// A message or page structure: plain bytes, valid for every bit pattern, so it can be read
/// from and written to the IOC and the frames as such.
///
/// # Safety
///
/// The implementor is `#[repr(C)]` (or packed) of integers and arrays of integers and
/// other `MpiPod`s only, with no padding bytes.
pub unsafe trait MpiPod: Copy {}

macro_rules! mpi_pod {
    ($($t:ty),* $(,)?) => {$(
        // SAFETY: a structure of `mpireg.rs`: integers and arrays of integers, no padding
        // (the layout checks of that file state every size and offset).
        unsafe impl MpiPod for $t {}
    )*};
}

mpi_pod!(
    MpiCfgHdr,
    MpiEcfgHdr,
    MpiMsgIocfactsRequest,
    MpiMsgIocfactsReply,
    MpiMsgIocinitRequest,
    MpiMsgIocinitReply,
    MpiCfgSpiPortPg0,
    MpiCfgSpiPortPg1,
    MpiCfgSpiDevPg0,
    MpiCfgSpiDevPg1,
    MpiCfgIocPg1,
    MpiCfgIocPg3,
    MpiCfgFcPortPg0,
    MpiCfgFcPortPg1,
    MpiCfgFcDevicePg0,
    MpiCfgSasDevPg0,
    MpiCfgRaidPhysdiskPg0,
    MpiMsgPortfactsReply,
    MpiMsgEventReply,
    MpiMsgScsiIoError,
    MpiMsgScsiTaskReply,
    MpiMsgConfigReply,
    MpiMsgFwuploadReply,
    MpiEvtSasChange,
    MpiMsgReply,
    MpiCfgRaidPhysdisk,
);

/// The header of a configuration page: `struct mpi_cfg_hdr` or the extended
/// `struct mpi_ecfg_hdr`.
pub trait MpiCfgHdrKind: MpiPod {
    /// Whether this is the extended form (`MPI_PG_EXTENDED`).
    const EXTENDED: bool;
    /// The header from the reply to a `MPI_CONFIG_REQ_ACTION_PAGE_HEADER` request.
    fn from_reply(cp: &MpiMsgConfigReply) -> Self;
    /// The page length in dwords.
    fn page_length(&self) -> usize;
    /// The header fields of a page read or write request.
    fn to_request(&self, cq: &mut MpiMsgConfigRequest);
}

impl MpiCfgHdrKind for MpiCfgHdr {
    const EXTENDED: bool = false;

    fn from_reply(cp: &MpiMsgConfigReply) -> Self {
        cp.config_header
    }

    fn page_length(&self) -> usize {
        usize::from(self.page_length)
    }

    fn to_request(&self, cq: &mut MpiMsgConfigRequest) {
        cq.config_header = *self;
    }
}

impl MpiCfgHdrKind for MpiEcfgHdr {
    const EXTENDED: bool = true;

    fn from_reply(cp: &MpiMsgConfigReply) -> Self {
        MpiEcfgHdr {
            page_version: cp.config_header.page_version,
            page_number: cp.config_header.page_number,
            page_type: cp.config_header.page_type,
            ext_page_length: cp.ext_page_length,
            ext_page_type: cp.ext_page_type,
            ..Default::default()
        }
    }

    fn page_length(&self) -> usize {
        usize::from(u16::from_le(self.ext_page_length))
    }

    fn to_request(&self, cq: &mut MpiMsgConfigRequest) {
        cq.config_header.page_version = self.page_version;
        cq.config_header.page_number = self.page_number;
        cq.config_header.page_type = self.page_type;
        cq.ext_page_len = self.ext_page_length;
        cq.ext_page_type = self.ext_page_type;
    }
}

/// `MPI_DEBUG`-less `DEVNAME(s)`.
fn devname(sc: &'static MpiSoftc) -> &'static str {
    sc.sc_dev.xname()
}

/// `mpi_read_db(s)`.
fn mpi_read_db(sc: &'static MpiSoftc) -> u32 {
    mpi_read(sc, MPI_DOORBELL)
}

/// `mpi_write_db(s, v)`.
fn mpi_write_db(sc: &'static MpiSoftc, v: u32) {
    mpi_write(sc, MPI_DOORBELL, v);
}

/// `mpi_read_intr(s)`: unbarriered.
fn mpi_read_intr(sc: &'static MpiSoftc) -> u32 {
    let (t, h) = sc.regs();
    bus_space_read_4(t, h, MPI_INTR_STATUS)
}

/// `mpi_write_intr(s, v)`.
fn mpi_write_intr(sc: &'static MpiSoftc, v: u32) {
    mpi_write(sc, MPI_INTR_STATUS, v);
}

/// `mpi_pop_reply(s)`: unbarriered.
fn mpi_pop_reply(sc: &'static MpiSoftc) -> u32 {
    let (t, h) = sc.regs();
    bus_space_read_4(t, h, MPI_REPLY_QUEUE)
}

/// `mpi_push_reply_db(s, v)`: unbarriered.
fn mpi_push_reply_db(sc: &'static MpiSoftc, v: u32) {
    let (t, h) = sc.regs();
    bus_space_write_4(t, h, MPI_REPLY_QUEUE, v);
}

/// `mpi_wait_db_int(s)`.
fn mpi_wait_db_int(sc: &'static MpiSoftc) -> Result<(), Errno> {
    mpi_wait_ne(sc, MPI_INTR_STATUS, MPI_INTR_STATUS_DOORBELL, 0)
}

/// `mpi_wait_db_ack(s)`.
fn mpi_wait_db_ack(sc: &'static MpiSoftc) -> Result<(), Errno> {
    mpi_wait_eq(sc, MPI_INTR_STATUS, MPI_INTR_STATUS_IOCDOORBELL, 0)
}

/// `mpi_cfg_header`: a header poll request for a basic page.
pub fn mpi_cfg_header(
    sc: &'static MpiSoftc,
    type_: u8,
    number: u8,
    address: u32,
    hdr: &mut MpiCfgHdr,
) -> Result<(), Errno> {
    mpi_req_cfg_header(sc, type_, number, address, MPI_PG_POLL, hdr)
}

/// `mpi_ecfg_header`: a header poll request for an extended page.
pub fn mpi_ecfg_header(
    sc: &'static MpiSoftc,
    type_: u8,
    number: u8,
    address: u32,
    hdr: &mut MpiEcfgHdr,
) -> Result<(), Errno> {
    mpi_req_cfg_header(
        sc,
        type_,
        number,
        address,
        MPI_PG_POLL | MPI_PG_EXTENDED,
        hdr,
    )
}

/// `mpi_cfg_page`: a page poll request for a basic page.
pub fn mpi_cfg_page<P: MpiPod>(
    sc: &'static MpiSoftc,
    address: u32,
    hdr: &MpiCfgHdr,
    read: bool,
    page: &mut P,
) -> Result<(), Errno> {
    mpi_req_cfg_page(sc, address, MPI_PG_POLL, hdr, read, mpi_pod_bytes_mut(page))
}

/// `mpi_cfg_page` over a `malloc`ed page of `page.len()` bytes.
pub fn mpi_cfg_page_bytes(
    sc: &'static MpiSoftc,
    address: u32,
    hdr: &MpiCfgHdr,
    read: bool,
    page: &mut [u8],
) -> Result<(), Errno> {
    mpi_req_cfg_page(sc, address, MPI_PG_POLL, hdr, read, page)
}

/// `mpi_ecfg_page`: a page poll request for an extended page.
pub fn mpi_ecfg_page<P: MpiPod>(
    sc: &'static MpiSoftc,
    address: u32,
    hdr: &MpiEcfgHdr,
    read: bool,
    page: &mut P,
) -> Result<(), Errno> {
    mpi_req_cfg_page(
        sc,
        address,
        MPI_PG_POLL | MPI_PG_EXTENDED,
        hdr,
        read,
        mpi_pod_bytes_mut(page),
    )
}

/// `mpi_ecfg_page` over a `malloc`ed page of `page.len()` bytes.
pub fn mpi_ecfg_page_bytes(
    sc: &'static MpiSoftc,
    address: u32,
    hdr: &MpiEcfgHdr,
    read: bool,
    page: &mut [u8],
) -> Result<(), Errno> {
    mpi_req_cfg_page(sc, address, MPI_PG_POLL | MPI_PG_EXTENDED, hdr, read, page)
}

/// The bytes of a structure.
fn mpi_pod_bytes_mut<P: MpiPod>(p: &mut P) -> &mut [u8] {
    // SAFETY: `MpiPod`: plain bytes with no padding, valid for every bit pattern, so
    // viewing and overwriting them as bytes is sound.
    unsafe { core::slice::from_raw_parts_mut(ptr::from_mut(p).cast::<u8>(), size_of::<P>()) }
}

/// A message of whole dwords, as the doorbell handshake moves it.
fn mpi_dwords<T: MpiPod>(t: &mut T) -> &mut [u32] {
    const { assert!(size_of::<T>().is_multiple_of(4) && align_of::<T>().is_multiple_of(4)) };
    // SAFETY: `MpiPod`; the size and alignment are multiples of four (checked above).
    unsafe { core::slice::from_raw_parts_mut(ptr::from_mut(t).cast::<u32>(), size_of::<T>() / 4) }
}

/// `mpi_dvatosge`.
#[inline]
fn mpi_dvatosge(sge: &mut MpiSge, dva: u64) {
    sge.sg_addr_lo = (dva as u32).to_le();
    sge.sg_addr_hi = ((dva >> 32) as u32).to_le();
}

/// The reply frame of `rcb`, copied out by value.
pub fn mpi_rcb_reply<T: MpiPod>(rcb: &MpiRcb) -> T {
    const { assert!(size_of::<T>() <= MPI_REPLY_SIZE) };
    // SAFETY: `rcb_reply` is a frame of `MPI_REPLY_SIZE` bytes in `sc_replies`, 4-aligned
    // and written by the IOC (hence volatile); `T` fits in a frame and is valid for any
    // bytes (`MpiPod`). `read_unaligned` makes no alignment claim for packed `T`.
    unsafe { ptr::read_volatile(rcb.rcb_reply.get().cast::<T>()) }
}

/// `scsi_io_get(&sc->sc_iopool, flags)` as a ccb.
fn mpi_scsi_io_get(sc: &'static MpiSoftc, flags: i32) -> Option<&'static MpiCcb> {
    // SAFETY: the pool is `sc_iopool`, whose `io_get` is `mpi_get_ccb_io`: the openings are
    // ccbs of `sc_ccbs`, which stay until a failed attach.
    scsi_io_get(&sc.sc_iopool, flags).map(|io| unsafe { io.cast::<MpiCcb>().as_ref() })
}

/// `scsi_io_put(&sc->sc_iopool, ccb)`.
fn mpi_scsi_io_put(sc: &'static MpiSoftc, ccb: &MpiCcb) {
    scsi_io_put(&sc.sc_iopool, NonNull::from(ccb).cast::<c_void>());
}

/// `link->bus->sb_adapter_softc`: the controller a link is on.
fn mpi_link_softc(link: &ScsiLink) -> &'static MpiSoftc {
    let p = link.bus().sb_adapter_softc.get();
    if p.is_null() {
        panic(format_args!("mpi: bus without an adapter softc"));
    }
    // SAFETY: `mpi_attach` attaches its scsibus with its own softc as `saa_adapter_softc`,
    // and only that bus's links reach `mpi_switch`; softcs are never freed.
    unsafe { &*p.cast::<MpiSoftc>().cast_const() }
}

/// `xs->io`: the ccb the midlayer took for a transfer.
fn mpi_xs_ccb(xs: &ScsiXfer) -> &'static MpiCcb {
    let Some(io) = xs.io.get() else {
        panic(format_args!("mpi: xs {:p} without a ccb", xs));
    };
    // SAFETY: the transfer's opening came from this adapter's pool (`saa_pool`), whose
    // `io_get` is `mpi_get_ccb_io`.
    unsafe { io.cast::<MpiCcb>().as_ref() }
}

/// `mpi_attach`.
pub fn mpi_attach(sc: &'static MpiSoftc) -> Result<(), Errno> {
    printf(format_args!("\n"));

    rw_init(&sc.sc_lock, "mpi_lock");
    task_set(&sc.sc_evt_rescan, mpi_fc_rescan, sc.cookie());

    // disable interrupts
    mpi_write(
        sc,
        MPI_INTR_MASK,
        MPI_INTR_MASK_REPLY | MPI_INTR_MASK_DOORBELL,
    );

    if mpi_init(sc).is_err() {
        printf(format_args!("{}: unable to initialise\n", devname(sc)));
        return Err(Errno::EIO);
    }

    if mpi_iocfacts(sc).is_err() {
        printf(format_args!("{}: unable to get iocfacts\n", devname(sc)));
        return Err(Errno::EIO);
    }

    if mpi_alloc_ccbs(sc).is_err() {
        // error already printed
        return Err(Errno::EIO);
    }

    // 0: free_ccbs, 1: free_replies
    let unwind = 'attach: {
        if mpi_alloc_replies(sc).is_err() {
            printf(format_args!(
                "{}: unable to allocate reply space\n",
                devname(sc)
            ));
            break 'attach 0;
        }

        if mpi_iocinit(sc).is_err() {
            printf(format_args!("{}: unable to send iocinit\n", devname(sc)));
            break 'attach 0;
        }

        // spin until we're operational
        if mpi_wait_eq(
            sc,
            MPI_DOORBELL,
            MPI_DOORBELL_STATE,
            MPI_DOORBELL_STATE_OPER,
        )
        .is_err()
        {
            printf(format_args!(
                "{}: state: 0x{:08x}\n",
                devname(sc),
                mpi_read_db(sc) & MPI_DOORBELL_STATE
            ));
            printf(format_args!("{}: operational state timeout\n", devname(sc)));
            break 'attach 0;
        }

        mpi_push_replies(sc);

        if mpi_portfacts(sc).is_err() {
            printf(format_args!("{}: unable to get portfacts\n", devname(sc)));
            break 'attach 1;
        }

        if mpi_cfg_coalescing(sc).is_err() {
            printf(format_args!(
                "{}: unable to configure coalescing\n",
                devname(sc)
            ));
            break 'attach 1;
        }

        let porttype = sc.sc_porttype.get();
        if porttype == MPI_PORTFACTS_PORTTYPE_SAS {
            sc.sc_evt_scan_queue.init();
            mtx_init(&sc.sc_evt_scan_mtx, IPL_BIO);
            // SAFETY: `mpi_evt_sas_detach` takes the softc as its cookie, which is never
            // freed while the handler is queued.
            unsafe {
                scsi_ioh_set(
                    &sc.sc_evt_scan_handler,
                    &sc.sc_iopool,
                    mpi_evt_sas_detach,
                    sc.cookie(),
                )
            };
        }
        if (porttype == MPI_PORTFACTS_PORTTYPE_SAS || porttype == MPI_PORTFACTS_PORTTYPE_FC)
            && mpi_eventnotify(sc).is_err()
        {
            printf(format_args!("{}: unable to enable events\n", devname(sc)));
            break 'attach 1;
        }

        if mpi_portenable(sc).is_err() {
            printf(format_args!("{}: unable to enable port\n", devname(sc)));
            break 'attach 1;
        }

        if mpi_fwupload(sc).is_err() {
            printf(format_args!("{}: unable to upload firmware\n", devname(sc)));
            break 'attach 1;
        }

        if mpi_manufacturing(sc).is_err() {
            printf(format_args!(
                "{}: unable to fetch manufacturing info\n",
                devname(sc)
            ));
            break 'attach 1;
        }

        match porttype {
            MPI_PORTFACTS_PORTTYPE_SCSI => {
                if mpi_cfg_spi_port(sc).is_err() {
                    printf(format_args!("{}: unable to configure spi\n", devname(sc)));
                    break 'attach 1;
                }
                mpi_squash_ppr(sc);
            }
            MPI_PORTFACTS_PORTTYPE_SAS => {
                if mpi_cfg_sas(sc).is_err() {
                    printf(format_args!("{}: unable to configure sas\n", devname(sc)));
                    break 'attach 1;
                }
            }
            MPI_PORTFACTS_PORTTYPE_FC if mpi_cfg_fc(sc).is_err() => {
                printf(format_args!("{}: unable to configure fc\n", devname(sc)));
                break 'attach 1;
            }
            _ => {}
        }

        // get raid pages
        mpi_get_raid(sc);
        // NBIO > 0
        if sc.sc_flags.get() & MPI_F_RAID != 0 {
            if bio_register(&sc.sc_dev, mpi_ioctl).is_err() {
                panic(format_args!(
                    "{}: controller registration failed",
                    devname(sc)
                ));
            } else {
                let mut hdr = MpiCfgHdr::default();
                if mpi_cfg_header(sc, MPI_CONFIG_REQ_PAGE_TYPE_IOC, 2, 0, &mut hdr).is_err() {
                    panic(format_args!("{}: can't get IOC page 2 hdr", devname(sc)));
                }
                sc.sc_cfg_hdr.set(hdr);

                let Some(vol_page) = mallocarray(
                    usize::from(hdr.page_length),
                    4,
                    M_TEMP,
                    M_WAITOK | M_CANFAIL,
                ) else {
                    panic(format_args!(
                        "{}: can't get memory for IOC page 2, bio disabled",
                        devname(sc)
                    ));
                };
                sc.sc_vol_page.set(vol_page.as_ptr().cast());

                // SAFETY: `vol_page` is `page_length * 4` bytes, just allocated.
                let bytes = unsafe {
                    core::slice::from_raw_parts_mut(
                        vol_page.as_ptr(),
                        usize::from(hdr.page_length) * 4,
                    )
                };
                if mpi_cfg_page_bytes(sc, 0, &hdr, true, bytes).is_err() {
                    panic(format_args!("{}: can't get IOC page 2", devname(sc)));
                }

                // sc_vol_list = (struct mpi_cfg_raid_vol *)(sc_vol_page + 1)
                sc.sc_vol_list.set(
                    // SAFETY: the list follows the page header in the same allocation.
                    unsafe { vol_page.as_ptr().add(size_of::<MpiCfgIocPg2>()) }.cast(),
                );

                sc.sc_ioctl.set(Some(mpi_ioctl));
            }
        }

        let mut saa = ScsibusAttachArgs::new();
        saa.saa_adapter = Some(&MPI_SWITCH);
        saa.saa_adapter_softc = sc.cookie();
        saa.saa_adapter_target = sc.sc_target.get() as u16;
        saa.saa_adapter_buswidth = sc.sc_buswidth.get() as u16;
        saa.saa_luns = 8;
        saa.saa_openings = (sc.sc_maxcmds.get() / sc.sc_buswidth.get()).max(16) as u16;
        saa.saa_pool = Some(&sc.sc_iopool);
        saa.saa_wwpn = sc.sc_port_wwn.get();
        saa.saa_wwnn = sc.sc_node_wwn.get();
        saa.saa_quirks = 0;
        saa.saa_flags = 0;

        let bus = config_found(&sc.sc_dev, ptr::from_mut(&mut saa).cast(), Some(scsiprint));
        // SAFETY: what attaches at mpi is a scsibus (`scsibus* at mpi?`), whose softc begins
        // with its device; softcs are never freed.
        sc.sc_scsibus
            .set(bus.map(|d| unsafe { &*d.as_ptr().cast::<ScsibusSoftc>().cast_const() }));

        // do domain validation
        if sc.sc_porttype.get() == MPI_PORTFACTS_PORTTYPE_SCSI {
            mpi_run_ppr(sc);
        }

        // enable interrupts
        mpi_write(sc, MPI_INTR_MASK, MPI_INTR_MASK_DOORBELL);

        // NBIO > 0, !SMALL_KERNEL
        let _ = mpi_create_sensors(sc);

        return Ok(());
    };

    if unwind >= 1 {
        // free_replies:
        bus_dmamap_sync(
            sc.dmat(),
            mpi_dma_map(sc.replies()),
            0,
            sc.sc_repq.get() as usize * MPI_REPLY_SIZE,
            BUS_DMASYNC_POSTREAD,
        );
        // SAFETY: the reply area allocated above, which the IOC no longer posts to being
        // unreachable after a failed attach.
        unsafe { mpi_dmamem_free(sc, sc.replies()) };
    }
    // free_ccbs:
    while let Some(ccb) = mpi_get_ccb(sc) {
        // SAFETY: the ccb's own map, which nothing else holds.
        unsafe { bus_dmamap_destroy(sc.dmat(), NonNull::from(ccb.ccb_dmamap)) };
    }
    // SAFETY: the request area allocated by mpi_alloc_ccbs, with every ccb gone.
    unsafe { mpi_dmamem_free(sc, sc.requests()) };
    if let Some(ccbs) = NonNull::new(sc.sc_ccbs.replace(ptr::null_mut())) {
        free(ccbs.cast(), M_DEVBUF, 0);
    }

    Err(Errno::EIO)
}

/// `mpi_cfg_spi_port`: makes the port's SCSI id and response ids what the driver wants.
pub fn mpi_cfg_spi_port(sc: &'static MpiSoftc) -> Result<(), Errno> {
    let mut hdr = MpiCfgHdr::default();
    let mut port = MpiCfgSpiPortPg1::default();

    mpi_cfg_header(sc, MPI_CONFIG_REQ_PAGE_TYPE_SCSI_SPI_PORT, 1, 0x0, &mut hdr)?;

    mpi_cfg_page(sc, 0x0, &hdr, true, &mut port)?;

    let target = sc.sc_target.get();
    if i32::from(port.port_scsi_id) == target
        && port.port_resp_ids == (1u16 << target).to_le()
        && port.on_bus_timer_value != 0u32.to_le()
    {
        return Ok(());
    }

    port.port_scsi_id = target as u8;
    port.port_resp_ids = (1u16 << target).to_le();
    port.on_bus_timer_value = 0x0700_0000u32.to_le(); // XXX magic

    if mpi_cfg_page(sc, 0x0, &hdr, false, &mut port).is_err() {
        printf(format_args!(
            "{}: unable to configure port scsi id\n",
            devname(sc)
        ));
        return Err(Errno::EIO);
    }

    Ok(())
}

/// `mpi_squash_ppr`: clears the requested transfer parameters of every target.
pub fn mpi_squash_ppr(sc: &'static MpiSoftc) {
    let mut hdr = MpiCfgHdr::default();
    let mut page = MpiCfgSpiDevPg1::default();

    for i in 0..sc.sc_buswidth.get() {
        if mpi_cfg_header(
            sc,
            MPI_CONFIG_REQ_PAGE_TYPE_SCSI_SPI_DEV,
            1,
            i as u32,
            &mut hdr,
        )
        .is_err()
        {
            return;
        }

        if mpi_cfg_page(sc, i as u32, &hdr, true, &mut page).is_err() {
            return;
        }

        page.req_params1 = 0x0;
        page.req_offset = 0x0;
        page.req_period = 0x0;
        page.req_params2 = 0x0;
        page.configuration = 0x0u32.to_le();

        if mpi_cfg_page(sc, i as u32, &hdr, false, &mut page).is_err() {
            return;
        }
    }
}

/// `mpi_run_ppr`: negotiates the transfer parameters of every device (domain validation).
pub fn mpi_run_ppr(sc: &'static MpiSoftc) {
    let mut hdr = MpiCfgHdr::default();
    let mut port_pg = MpiCfgSpiPortPg0::default();

    if mpi_cfg_header(sc, MPI_CONFIG_REQ_PAGE_TYPE_SCSI_SPI_PORT, 0, 0x0, &mut hdr).is_err() {
        return;
    }

    if mpi_cfg_page(sc, 0x0, &hdr, true, &mut port_pg).is_err() {
        return;
    }

    let Some(bus) = sc.sc_scsibus.get() else {
        return;
    };
    for i in 0..sc.sc_buswidth.get() {
        let Some(link) = scsi_get_link(bus, i, 0) else {
            continue;
        };

        // do not ppr volumes
        if link.flags.get() & SDEV_VIRTUAL != 0 {
            continue;
        }

        let mut tries = 0;
        while mpi_ppr(
            sc,
            Some(link),
            None,
            i32::from(port_pg.min_period),
            i32::from(port_pg.max_offset),
            tries,
        ) == Err(Errno::EAGAIN)
        {
            tries += 1;
        }
    }

    if sc.sc_flags.get() & MPI_F_RAID == 0 {
        return;
    }

    if mpi_cfg_header(sc, MPI_CONFIG_REQ_PAGE_TYPE_IOC, 3, 0x0, &mut hdr).is_err() {
        return;
    }

    let pagelen = usize::from(hdr.page_length) * 4; // dwords to bytes
    let Some(buf) = malloc(pagelen, M_TEMP, M_WAITOK | M_CANFAIL) else {
        return;
    };
    // SAFETY: `pagelen` bytes just allocated, owned here until freed below.
    let bytes = unsafe { core::slice::from_raw_parts_mut(buf.as_ptr(), pagelen) };

    'out: {
        if mpi_cfg_page_bytes(sc, 0, &hdr, true, bytes).is_err() {
            break 'out;
        }
        if pagelen < size_of::<MpiCfgIocPg3>() {
            break 'out;
        }
        // SAFETY: the page header is at least `size_of::<MpiCfgIocPg3>()` bytes of the
        // buffer (checked above); it is plain bytes (`MpiPod`) at any alignment.
        let physdisk_pg: MpiCfgIocPg3 = unsafe { ptr::read_unaligned(bytes.as_ptr().cast()) };
        let no_phys_disks = usize::from(physdisk_pg.no_phys_disks);

        for i in 0..no_phys_disks {
            let off = size_of::<MpiCfgIocPg3>() + i * size_of::<MpiCfgRaidPhysdisk>();
            if off + size_of::<MpiCfgRaidPhysdisk>() > pagelen {
                break;
            }
            // SAFETY: within the buffer (checked above); plain bytes.
            let physdisk: MpiCfgRaidPhysdisk =
                unsafe { ptr::read_unaligned(bytes.as_ptr().add(off).cast()) };

            if i32::from(physdisk.phys_disk_ioc) != sc.sc_ioc_number.get() {
                continue;
            }

            let mut tries = 0;
            while mpi_ppr(
                sc,
                None,
                Some(&physdisk),
                i32::from(port_pg.min_period),
                i32::from(port_pg.max_offset),
                tries,
            ) == Err(Errno::EAGAIN)
            {
                tries += 1;
            }
        }
    }

    free(buf, M_TEMP, pagelen);
}

/// `mpi_ppr`: one parallel protocol request for a device (a link, or a RAID physical
/// disk), `try` choosing U320, U160 or U80; `EAGAIN` asks for the next try.
pub fn mpi_ppr(
    sc: &'static MpiSoftc,
    link: Option<&'static ScsiLink>,
    physdisk: Option<&MpiCfgRaidPhysdisk>,
    mut period: i32,
    offset: i32,
    try_: i32,
) -> Result<(), Errno> {
    let mut hdr0 = MpiCfgHdr::default();
    let mut hdr1 = MpiCfgHdr::default();
    let mut pg0 = MpiCfgSpiDevPg0::default();
    let mut pg1 = MpiCfgSpiDevPg1::default();
    let raid;
    let address: u32;
    let id: i32;

    if try_ >= 3 {
        return Err(Errno::EIO);
    }

    match (physdisk, link) {
        (None, Some(link)) => {
            if link.inqdata.get().device & SID_TYPE == T_PROCESSOR {
                return Err(Errno::EIO);
            }

            raid = false;
            address = u32::from(link.target.get());
            id = i32::from(link.target.get());
        }
        (Some(physdisk), _) => {
            raid = true;
            address = (u32::from(physdisk.phys_disk_bus) << 8) | u32::from(physdisk.phys_disk_id);
            id = i32::from(physdisk.phys_disk_num);
        }
        (None, None) => panic(format_args!("mpi_ppr: no device")),
    }

    if mpi_cfg_header(
        sc,
        MPI_CONFIG_REQ_PAGE_TYPE_SCSI_SPI_DEV,
        0,
        address,
        &mut hdr0,
    )
    .is_err()
    {
        return Err(Errno::EIO);
    }

    if mpi_cfg_header(
        sc,
        MPI_CONFIG_REQ_PAGE_TYPE_SCSI_SPI_DEV,
        1,
        address,
        &mut hdr1,
    )
    .is_err()
    {
        return Err(Errno::EIO);
    }

    // MPI_DEBUG: the page 0 read is not ported.

    if mpi_cfg_page(sc, address, &hdr1, true, &mut pg1).is_err() {
        return Err(Errno::EIO);
    }

    pg1.req_params1 = 0;
    pg1.req_offset = offset as u8;
    pg1.req_period = period as u8;
    pg1.req_params2 &= !MPI_CFG_SPI_DEV_1_REQPARAMS_WIDTH;

    let nosync = link.is_some_and(|l| l.quirks.get() & SDEV_NOSYNC != 0);
    if raid || !nosync {
        pg1.req_params2 |= MPI_CFG_SPI_DEV_1_REQPARAMS_WIDTH_WIDE;

        match try_ {
            0 => {}                     // U320
            1 => pg1.req_period = 0x09, // U160
            2 => pg1.req_period = 0x0a, // U80
            _ => {}
        }

        if pg1.req_period < 0x09 {
            // Ultra320: enable QAS & PACKETIZED
            pg1.req_params1 |=
                MPI_CFG_SPI_DEV_1_REQPARAMS_QAS | MPI_CFG_SPI_DEV_1_REQPARAMS_PACKETIZED;
        }
        if pg1.req_period < 0xa {
            // >= Ultra160: enable dual xfers
            pg1.req_params1 |= MPI_CFG_SPI_DEV_1_REQPARAMS_DUALXFERS;
        }
    }

    if mpi_cfg_page(sc, address, &hdr1, false, &mut pg1).is_err() {
        return Err(Errno::EIO);
    }

    if mpi_cfg_page(sc, address, &hdr1, true, &mut pg1).is_err() {
        return Err(Errno::EIO);
    }

    if mpi_inq(sc, id as u16, raid).is_err() {
        return Err(Errno::EIO);
    }

    if mpi_cfg_page(sc, address, &hdr0, true, &mut pg0).is_err() {
        return Err(Errno::EIO);
    }

    let information = u32::from_le(pg0.information);
    if information & 0x07 == 0 && try_ == 0 {
        // U320 ppr rejected
        return Err(Errno::EAGAIN);
    }

    if ((information >> 8) & 0xff) > 0x09 && try_ == 1 {
        // U160 ppr rejected
        return Err(Errno::EAGAIN);
    }

    if information & 0x0e != 0 {
        // ppr rejected
        return Err(Errno::EAGAIN);
    }

    period = match pg0.neg_period {
        0x08 => 160,
        0x09 => 80,
        0x0a => 40,
        0x0b => 20,
        0x0c => 10,
        _ => 0,
    };

    printf(format_args!(
        "{}: {} {} {} at {}MHz width {}bit offset {} QAS {} DT {} IU {}\n",
        devname(sc),
        if raid { "phys disk" } else { "target" },
        id,
        if period != 0 { "Sync" } else { "Async" },
        period,
        if pg0.neg_params2 & MPI_CFG_SPI_DEV_0_NEGPARAMS_WIDTH_WIDE != 0 {
            16
        } else {
            8
        },
        pg0.neg_offset,
        i32::from(pg0.neg_params1 & MPI_CFG_SPI_DEV_0_NEGPARAMS_QAS != 0),
        i32::from(pg0.neg_params1 & MPI_CFG_SPI_DEV_0_NEGPARAMS_DUALXFERS != 0),
        i32::from(pg0.neg_params1 & MPI_CFG_SPI_DEV_0_NEGPARAMS_PACKETIZED != 0),
    ));

    Ok(())
}

/// The request frame of `mpi_inq`: the SCSI I/O, one SGE, and the INQUIRY data and sense
/// the IOC writes in the same frame.
#[repr(C)]
struct MpiInqBundle {
    io: MpiMsgScsiIo,
    sge: MpiSge,
    inqbuf: ScsiInquiryData,
    sense: ScsiSenseData,
}

/// `mpi_inq`: an INQUIRY to `target` (a physical disk when `physdisk`), to make the IOC
/// negotiate with it.
pub fn mpi_inq(sc: &'static MpiSoftc, target: u16, physdisk: bool) -> Result<(), Errno> {
    let mut inq = ScsiInquiry {
        opcode: INQUIRY,
        ..Default::default()
    };
    _lto2b(size_of::<ScsiInquiryData>() as u32, &mut inq.length);

    let Some(ccb) = mpi_scsi_io_get(sc, SCSI_NOSLEEP) else {
        return Err(Errno::EIO);
    };

    ccb.ccb_done.set(Some(mpi_empty_done));

    // SAFETY: the ccb was just taken from the pool: its frame is ours and zeroed.
    let bundle = unsafe { ccb.cmd::<MpiInqBundle>() };
    const { assert!(size_of::<MpiInqBundle>() <= MPI_REQUEST_SIZE) };

    bundle.io.function = if physdisk {
        MPI_FUNCTION_RAID_SCSI_IO_PASSTHROUGH
    } else {
        MPI_FUNCTION_SCSI_IO_REQUEST
    };
    // bus is always 0
    bundle.io.target_id = target as u8;

    bundle.io.cdb_length = size_of::<ScsiInquiry>() as u8;
    bundle.io.sense_buf_len = size_of::<ScsiSenseData>() as u8;
    bundle.io.msg_flags = MPI_SCSIIO_SENSE_BUF_ADDR_WIDTH_64;

    // always lun 0

    bundle.io.direction = MPI_SCSIIO_DIR_READ;
    bundle.io.tagging = MPI_SCSIIO_ATTR_NO_DISCONNECT;

    bundle.io.cdb[0] = inq.opcode;
    bundle.io.cdb[1] = inq.flags;
    bundle.io.cdb[2] = inq.pagecode;
    bundle.io.cdb[3] = inq.length[0];
    bundle.io.cdb[4] = inq.length[1];
    bundle.io.cdb[5] = inq.control;

    bundle.io.data_length = (size_of::<ScsiInquiryData>() as u32).to_le();

    bundle.io.sense_buf_low_addr =
        ((ccb.ccb_cmd_dva.get() + offset_of!(MpiInqBundle, sense) as u64) as u32).to_le();

    bundle.sge.sg_hdr = (MPI_SGE_FL_TYPE_SIMPLE
        | MPI_SGE_FL_SIZE_64
        | MPI_SGE_FL_LAST
        | MPI_SGE_FL_EOB
        | MPI_SGE_FL_EOL
        | size_of::<ScsiInquiry>() as u32)
        .to_le();

    mpi_dvatosge(
        &mut bundle.sge,
        ccb.ccb_cmd_dva.get() + offset_of!(MpiInqBundle, inqbuf) as u64,
    );

    mpi_poll(sc, ccb, 5000)?;

    if let Some(rcb) = ccb.ccb_rcb.get() {
        mpi_push_reply(sc, rcb);
    }

    mpi_scsi_io_put(sc, ccb);

    Ok(())
}

/// `mpi_cfg_sas`: lets SATA devices queue 32 commands.
pub fn mpi_cfg_sas(sc: &'static MpiSoftc) -> Result<(), Errno> {
    let mut ehdr = MpiEcfgHdr::default();

    if mpi_ecfg_header(sc, MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_IO_UNIT, 1, 0, &mut ehdr).is_err() {
        return Ok(());
    }

    let pagelen = usize::from(u16::from_le(ehdr.ext_page_length)) * 4;
    let Some(buf) = malloc(pagelen, M_TEMP, M_ZERO) else {
        return Err(Errno::ENOMEM);
    };
    // SAFETY: `pagelen` zeroed bytes just allocated, owned here until freed below.
    let bytes = unsafe { core::slice::from_raw_parts_mut(buf.as_ptr(), pagelen) };

    'out: {
        if mpi_ecfg_page_bytes(sc, 0, &ehdr, true, bytes).is_err() {
            break 'out;
        }

        let off = offset_of!(MpiCfgSasIouPg1, max_sata_q_depth);
        if bytes.get(off).copied() != Some(32) && off < pagelen {
            bytes[off] = 32;

            if mpi_ecfg_page_bytes(sc, 0, &ehdr, false, bytes).is_err() {
                break 'out;
            }
        }
    }

    free(buf, M_TEMP, pagelen);
    Ok(())
}

/// `mpi_cfg_fc`: reads the port's names and asks for immediate errors and verbose rescans.
pub fn mpi_cfg_fc(sc: &'static MpiSoftc) -> Result<(), Errno> {
    let mut hdr = MpiCfgHdr::default();
    let mut pg0 = MpiCfgFcPortPg0::default();
    let mut pg1 = MpiCfgFcPortPg1::default();

    if mpi_cfg_header(sc, MPI_CONFIG_REQ_PAGE_TYPE_FC_PORT, 0, 0, &mut hdr).is_err() {
        printf(format_args!(
            "{}: unable to fetch FC port header 0\n",
            devname(sc)
        ));
        return Err(Errno::EIO);
    }

    if mpi_cfg_page(sc, 0, &hdr, true, &mut pg0).is_err() {
        printf(format_args!(
            "{}: unable to fetch FC port page 0\n",
            devname(sc)
        ));
        return Err(Errno::EIO);
    }

    sc.sc_port_wwn.set(u64::from_le(pg0.wwpn));
    sc.sc_node_wwn.set(u64::from_le(pg0.wwnn));

    // configure port config more to our liking
    if mpi_cfg_header(sc, MPI_CONFIG_REQ_PAGE_TYPE_FC_PORT, 1, 0, &mut hdr).is_err() {
        printf(format_args!(
            "{}: unable to fetch FC port header 1\n",
            devname(sc)
        ));
        return Err(Errno::EIO);
    }

    if mpi_cfg_page(sc, 0, &hdr, true, &mut pg1).is_err() {
        printf(format_args!(
            "{}: unable to fetch FC port page 1\n",
            devname(sc)
        ));
        return Err(Errno::EIO);
    }

    pg1.flags |=
        (MPI_CFG_FC_PORT_0_FLAGS_IMMEDIATE_ERROR | MPI_CFG_FC_PORT_0_FLAGS_VERBOSE_RESCAN).to_le();

    if mpi_cfg_page(sc, 0, &hdr, false, &mut pg1).is_err() {
        printf(format_args!(
            "{}: unable to set FC port page 1\n",
            devname(sc)
        ));
        return Err(Errno::EIO);
    }

    Ok(())
}

/// `mpi_detach`.
pub fn mpi_detach(_sc: &'static MpiSoftc) {}

/// `mpi_intr`: the interrupt handler: pops and handles every posted reply.
pub fn mpi_intr(arg: *mut c_void) -> i32 {
    // SAFETY: established by mpi_pci_attach with the softc as the argument; softcs are never
    // freed while their interrupt is established.
    let sc: &'static MpiSoftc = unsafe { &*arg.cast::<MpiSoftc>().cast_const() };
    let mut rv = 0;

    if mpi_read_intr(sc) & MPI_INTR_STATUS_REPLY == 0 {
        return rv;
    }

    loop {
        let reg = mpi_pop_reply(sc);
        if reg == 0xffff_ffff {
            break;
        }
        mpi_reply(sc, reg);
        rv = 1;
    }

    rv
}

/// `mpi_reply`: completes the command a reply names.
pub fn mpi_reply(sc: &'static MpiSoftc, reg: u32) {
    let mut rcb: Option<&'static MpiRcb> = None;
    let id: u32;

    if reg & MPI_REPLY_QUEUE_ADDRESS != 0 {
        let reply_dva = (reg & MPI_REPLY_QUEUE_ADDRESS_MASK) << 1;
        let i = reply_dva.wrapping_sub(mpi_dma_dva(sc.replies()) as u32) / MPI_REPLY_SIZE as u32;
        let r = sc.rcb(i as usize);

        bus_dmamap_sync(
            sc.dmat(),
            mpi_dma_map(sc.replies()),
            r.rcb_offset.get(),
            MPI_REPLY_SIZE,
            BUS_DMASYNC_POSTREAD,
        );

        let reply: MpiMsgReply = mpi_rcb_reply(r);
        id = u32::from_le(reply.msg_context);
        rcb = Some(r);
    } else {
        match reg & MPI_REPLY_QUEUE_TYPE_MASK {
            MPI_REPLY_QUEUE_TYPE_INIT => id = reg & MPI_REPLY_QUEUE_CONTEXT,
            _ => panic(format_args!("{}: unsupported context reply", devname(sc))),
        }
    }

    let ccb = sc.ccb(id as usize);

    bus_dmamap_sync(
        sc.dmat(),
        mpi_dma_map(sc.requests()),
        ccb.ccb_offset.get(),
        MPI_REQUEST_SIZE,
        BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
    );
    ccb.ccb_state.set(MpiCcbState::Ready);
    ccb.ccb_rcb.set(rcb);

    match ccb.ccb_done.get() {
        Some(done) => done(ccb),
        None => panic(format_args!(
            "{}: reply for a ccb without done",
            devname(sc)
        )),
    }
}

/// `mpi_dmamem_alloc`: `size` bytes of zeroed, page aligned DMA memory, mapped and loaded.
pub fn mpi_dmamem_alloc(sc: &'static MpiSoftc, size: usize) -> Option<&'static MpiDmamem> {
    let dmat = sc.dmat();

    let mem = malloc(size_of::<MpiDmamem>(), M_DEVBUF, M_ZERO)?;

    let map = match bus_dmamap_create(dmat, size, 1, size, 0, BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW) {
        Ok(map) => map,
        Err(_) => {
            // mdmfree:
            free(mem, M_DEVBUF, size_of::<MpiDmamem>());
            return None;
        }
    };

    let mut segs = [BusDmaSegment::default(); 1];
    let destroy = |mem: NonNull<u8>| {
        // SAFETY: the map created above, unused.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
        free(mem, M_DEVBUF, size_of::<MpiDmamem>());
    };
    let nsegs = match bus_dmamem_alloc(
        dmat,
        size,
        PAGE_SIZE,
        0,
        &mut segs,
        BUS_DMA_NOWAIT | BUS_DMA_ZERO,
    ) {
        Ok(n) => n,
        Err(_) => {
            destroy(mem);
            return None;
        }
    };

    let kva = match bus_dmamem_map(dmat, &mut segs[..nsegs], size, BUS_DMA_NOWAIT) {
        Ok(kva) => kva,
        Err(_) => {
            // free:
            // SAFETY: the segment allocated above, not mapped.
            unsafe { bus_dmamem_free(dmat, &segs[..nsegs]) };
            destroy(mem);
            return None;
        }
    };

    // SAFETY: `kva` maps `size` bytes allocated above, which stay until mpi_dmamem_free
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

    let mdm = mem.cast::<MpiDmamem>();
    // SAFETY: a fresh allocation of `size_of::<MpiDmamem>()` bytes (malloc aligns it),
    // written whole before any use; it lives until mpi_dmamem_free.
    unsafe {
        mdm.as_ptr().write(MpiDmamem {
            mdm_map: map,
            mdm_seg: segs[0],
            mdm_size: size,
            mdm_kva: kva,
        });
        Some(&*mdm.as_ptr())
    }
}

/// `mpi_dmamem_free`.
///
/// # Safety
///
/// `mdm` came from [`mpi_dmamem_alloc`], the IOC no longer uses it and nothing references
/// it afterwards.
pub unsafe fn mpi_dmamem_free(sc: &'static MpiSoftc, mdm: &'static MpiDmamem) {
    let dmat = sc.dmat();

    bus_dmamap_unload(dmat, mdm.mdm_map);
    // SAFETY: the caller's guarantee: the area's own mapping, segment and map, unloaded.
    unsafe {
        bus_dmamem_unmap(dmat, mdm.mdm_kva, mdm.mdm_size);
        bus_dmamem_free(dmat, core::slice::from_ref(&mdm.mdm_seg));
        bus_dmamap_destroy(dmat, NonNull::from(mdm.mdm_map));
    }
    free(NonNull::from(mdm).cast(), M_DEVBUF, size_of::<MpiDmamem>());
}

/// `mpi_alloc_ccbs`: `sc_maxcmds` ccbs, their request frames and DMA maps, and the iopool.
pub fn mpi_alloc_ccbs(sc: &'static MpiSoftc) -> Result<(), Errno> {
    sc.sc_ccb_free.init();
    mtx_init(&sc.sc_ccb_mtx, IPL_BIO);

    let n = sc.sc_maxcmds.get().max(0) as usize;
    let Some(ccbs) = mallocarray(
        n,
        size_of::<MpiCcb>(),
        M_DEVBUF,
        M_WAITOK | M_CANFAIL | M_ZERO,
    ) else {
        printf(format_args!("{}: unable to allocate ccbs\n", devname(sc)));
        return Err(Errno::ENOMEM);
    };
    let ccbs = ccbs.cast::<MpiCcb>();
    sc.sc_ccbs.set(ccbs.as_ptr());

    let Some(requests) = mpi_dmamem_alloc(sc, MPI_REQUEST_SIZE * n) else {
        printf(format_args!(
            "{}: unable to allocate ccb dmamem\n",
            devname(sc)
        ));
        // free_ccbs:
        free(ccbs.cast(), M_DEVBUF, 0);
        sc.sc_ccbs.set(ptr::null_mut());
        return Err(Errno::ENOMEM);
    };
    sc.sc_requests.set(Some(requests));
    let cmd = mpi_dma_kva(requests);
    // SAFETY: the area is `MPI_REQUEST_SIZE * n` bytes, ours alone until the IOC is told.
    unsafe { ptr::write_bytes(cmd, 0, MPI_REQUEST_SIZE * n) };

    for i in 0..n {
        let Ok(map) = bus_dmamap_create(
            sc.dmat(),
            MAXPHYS,
            sc.sc_max_sgl_len.get(),
            MAXPHYS,
            0,
            BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW,
        ) else {
            printf(format_args!("{}: unable to create dma map\n", devname(sc)));
            // free_maps:
            while let Some(ccb) = mpi_get_ccb(sc) {
                // SAFETY: the ccb's own map, which nothing else holds.
                unsafe { bus_dmamap_destroy(sc.dmat(), NonNull::from(ccb.ccb_dmamap)) };
            }
            // SAFETY: the request area allocated above, with every ccb gone.
            unsafe { mpi_dmamem_free(sc, requests) };
            sc.sc_requests.set(None);
            // free_ccbs:
            free(ccbs.cast(), M_DEVBUF, 0);
            sc.sc_ccbs.set(ptr::null_mut());
            return Err(Errno::ENOMEM);
        };

        let offset = MPI_REQUEST_SIZE * i;
        // SAFETY: `i` is below `n`, the length of the allocation; the slot is written whole
        // before anything reads it.
        let ccb = unsafe {
            let p = ccbs.as_ptr().add(i);
            p.write(MpiCcb {
                ccb_sc: core::cell::Cell::new(ptr::from_ref(sc)),
                ccb_id: core::cell::Cell::new(i as i32),
                ccb_cookie: core::cell::Cell::new(MpiCookie::None),
                ccb_poll_rv: core::cell::Cell::new(0),
                ccb_dmamap: map,
                ccb_offset: core::cell::Cell::new(offset),
                ccb_cmd: core::cell::Cell::new(cmd.add(offset)),
                ccb_cmd_dva: core::cell::Cell::new(
                    u64::from(mpi_dma_dva(requests) as u32) + offset as u64,
                ),
                ccb_state: core::cell::Cell::new(MpiCcbState::Ready),
                ccb_done: core::cell::Cell::new(None),
                ccb_rcb: core::cell::Cell::new(None),
                ccb_link: crate::sys::queue::SlistEntry::new(),
            });
            &*p
        };

        mpi_put_ccb(sc, ccb);
    }

    // SAFETY: the softc is the iopool's cookie for `mpi_get_ccb_io` and `mpi_put_ccb_io`.
    unsafe { scsi_iopool_init(&sc.sc_iopool, sc.cookie(), mpi_get_ccb_io, mpi_put_ccb_io) };
    sc.sc_nccbs.set(n as i32);

    Ok(())
}

/// `mpi_get_ccb`: a free ccb, if any.
pub fn mpi_get_ccb(sc: &'static MpiSoftc) -> Option<&'static MpiCcb> {
    mtx_enter(&sc.sc_ccb_mtx);
    let ccb = sc.sc_ccb_free.first().map(NonNull::from);
    if let Some(ccb) = ccb {
        // SAFETY: the list is not empty.
        unsafe { sc.sc_ccb_free.remove_head() };
        // SAFETY: a ccb of `sc_ccbs`, which stays until a failed attach.
        unsafe { ccb.as_ref() }.ccb_state.set(MpiCcbState::Ready);
    }
    mtx_leave(&sc.sc_ccb_mtx);

    // SAFETY: as above.
    ccb.map(|p| unsafe { &*p.as_ptr() })
}

/// `mpi_put_ccb`: gives a ccb back, with a zeroed request frame.
pub fn mpi_put_ccb(sc: &'static MpiSoftc, ccb: &'static MpiCcb) {
    #[cfg(feature = "diagnostic")]
    if ccb.ccb_state.get() == MpiCcbState::Free {
        panic(format_args!("mpi_put_ccb: double free"));
    }

    ccb.ccb_state.set(MpiCcbState::Free);
    ccb.ccb_cookie.set(MpiCookie::None);
    ccb.ccb_done.set(None);
    // SAFETY: the frame is `MPI_REQUEST_SIZE` bytes of `sc_requests`, ours alone.
    unsafe { ptr::write_bytes(ccb.ccb_cmd.get(), 0, MPI_REQUEST_SIZE) };
    mtx_enter(&sc.sc_ccb_mtx);
    // SAFETY: the ccb is on no list and stays in `sc_ccbs` until a failed attach.
    unsafe { sc.sc_ccb_free.insert_head(ccb) };
    mtx_leave(&sc.sc_ccb_mtx);
}

/// `mpi_get_ccb` as the iopool's `io_get`.
///
/// # Safety
///
/// `cookie` is a live [`MpiSoftc`] (the one `mpi_alloc_ccbs` gave `scsi_iopool_init`).
unsafe fn mpi_get_ccb_io(cookie: *mut c_void) -> Option<ScsiIo> {
    // SAFETY: the caller's guarantee.
    let sc: &'static MpiSoftc = unsafe { &*cookie.cast::<MpiSoftc>().cast_const() };

    mpi_get_ccb(sc).map(|ccb| NonNull::from(ccb).cast::<c_void>())
}

/// `mpi_put_ccb` as the iopool's `io_put`.
///
/// # Safety
///
/// `cookie` is a live [`MpiSoftc`] and `io` one of its ccbs, from `mpi_get_ccb_io`.
unsafe fn mpi_put_ccb_io(cookie: *mut c_void, io: ScsiIo) {
    // SAFETY: the caller's guarantee.
    let sc: &'static MpiSoftc = unsafe { &*cookie.cast::<MpiSoftc>().cast_const() };
    // SAFETY: the caller's guarantee: a ccb of `sc_ccbs`.
    let ccb: &'static MpiCcb = unsafe { io.cast::<MpiCcb>().as_ref() };

    mpi_put_ccb(sc, ccb);
}

/// `mpi_alloc_replies`: the reply control blocks and the reply frames.
pub fn mpi_alloc_replies(sc: &'static MpiSoftc) -> Result<(), Errno> {
    let repq = sc.sc_repq.get().max(0) as usize;

    let Some(rcbs) = mallocarray(
        repq,
        size_of::<MpiRcb>(),
        M_DEVBUF,
        M_WAITOK | M_CANFAIL | M_ZERO,
    ) else {
        return Err(Errno::ENOMEM);
    };
    sc.sc_rcbs.set(rcbs.as_ptr().cast());

    let Some(replies) = mpi_dmamem_alloc(sc, repq * MPI_REPLY_SIZE) else {
        free(rcbs, M_DEVBUF, 0);
        sc.sc_rcbs.set(ptr::null_mut());
        return Err(Errno::ENOMEM);
    };
    sc.sc_replies.set(Some(replies));

    Ok(())
}

/// `mpi_push_reply`: gives a reply frame back to the IOC.
pub fn mpi_push_reply(sc: &'static MpiSoftc, rcb: &MpiRcb) {
    bus_dmamap_sync(
        sc.dmat(),
        mpi_dma_map(sc.replies()),
        rcb.rcb_offset.get(),
        MPI_REPLY_SIZE,
        BUS_DMASYNC_PREREAD,
    );
    mpi_push_reply_db(sc, rcb.rcb_reply_dva.get());
}

/// `mpi_push_replies`: posts every reply frame.
pub fn mpi_push_replies(sc: &'static MpiSoftc) {
    let kva = mpi_dma_kva(sc.replies());
    let repq = sc.sc_repq.get().max(0) as usize;

    bus_dmamap_sync(
        sc.dmat(),
        mpi_dma_map(sc.replies()),
        0,
        repq * MPI_REPLY_SIZE,
        BUS_DMASYNC_PREREAD,
    );

    for i in 0..repq {
        let rcb = sc.rcb(i);

        // SAFETY: frame `i` of the reply area, `MPI_REPLY_SIZE * repq` bytes.
        rcb.rcb_reply.set(unsafe { kva.add(MPI_REPLY_SIZE * i) });
        rcb.rcb_offset.set(MPI_REPLY_SIZE * i);
        rcb.rcb_reply_dva
            .set(mpi_dma_dva(sc.replies()) as u32 + (MPI_REPLY_SIZE * i) as u32);
        mpi_push_reply_db(sc, rcb.rcb_reply_dva.get());
    }
}

/// `mpi_start`: queues a request frame to the IOC.
pub fn mpi_start(sc: &'static MpiSoftc, ccb: &'static MpiCcb) {
    // SAFETY: the caller holds the ccb (it is not queued), so the frame is ours; every
    // message begins with this header.
    let msg = unsafe { ccb.cmd::<MpiMsgRequest>() };
    msg.msg_context = (ccb.ccb_id.get() as u32).to_le();

    bus_dmamap_sync(
        sc.dmat(),
        mpi_dma_map(sc.requests()),
        ccb.ccb_offset.get(),
        MPI_REQUEST_SIZE,
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    ccb.ccb_state.set(MpiCcbState::Queued);
    let (t, h) = sc.regs();
    bus_space_write_4(t, h, MPI_REQ_QUEUE, ccb.ccb_cmd_dva.get() as u32);
}

/// `mpi_poll`: runs a ccb to completion by polling the reply queue; `timeout` is in
/// milliseconds.
pub fn mpi_poll(
    sc: &'static MpiSoftc,
    ccb: &'static MpiCcb,
    mut timeout: i32,
) -> Result<(), Errno> {
    let done = ccb.ccb_done.get();
    let cookie = ccb.ccb_cookie.get();

    ccb.ccb_done.set(Some(mpi_poll_done));
    ccb.ccb_cookie.set(MpiCookie::Poll);
    ccb.ccb_poll_rv.set(1);

    mpi_start(sc, ccb);
    while ccb.ccb_poll_rv.get() == 1 {
        let reg = mpi_pop_reply(sc);
        if reg == 0xffff_ffff {
            let expired = timeout == 0;
            timeout = timeout.wrapping_sub(1);
            if expired {
                printf(format_args!("{}: timeout\n", devname(sc)));
                return Err(Errno::EIO);
            }

            delay(1000);
            continue;
        }

        mpi_reply(sc, reg);
    }

    ccb.ccb_cookie.set(cookie);
    if let Some(done) = done {
        done(ccb);
    }

    Ok(())
}

/// `mpi_poll_done`.
pub fn mpi_poll_done(ccb: &'static MpiCcb) {
    ccb.ccb_poll_rv.set(0);
}

/// `mpi_wait`: runs a ccb to completion by sleeping.
pub fn mpi_wait(sc: &'static MpiSoftc, ccb: &'static MpiCcb) {
    let cookie = Mutex::new(0);
    mtx_init(&cookie, IPL_BIO);

    let done = ccb.ccb_done.get();
    ccb.ccb_done.set(Some(mpi_wait_done));
    ccb.ccb_cookie.set(MpiCookie::Wait(&raw const cookie));

    // XXX this will wait forever for the ccb to complete

    mpi_start(sc, ccb);

    mtx_enter(&cookie);
    while !matches!(ccb.ccb_cookie.get(), MpiCookie::None) {
        let _ = msleep_nsec(ptr::from_ref(ccb), &cookie, PRIBIO, "mpiwait", INFSLP);
    }
    mtx_leave(&cookie);

    if let Some(done) = done {
        done(ccb);
    }
}

/// `mpi_wait_done`.
pub fn mpi_wait_done(ccb: &'static MpiCcb) {
    let MpiCookie::Wait(cookie) = ccb.ccb_cookie.get() else {
        panic(format_args!("mpi_wait_done: ccb without a wait cookie"));
    };
    // SAFETY: `mpi_wait` set the cookie to its mutex and does not return until this
    // function clears it (below, with the mutex held), so the mutex is still alive.
    let cookie = unsafe { &*cookie };

    mtx_enter(cookie);
    ccb.ccb_cookie.set(MpiCookie::None);
    wakeup_one(ptr::from_ref(ccb));
    mtx_leave(cookie);
}

/// `mpi_scsi_cmd`: the adapter's `scsi_cmd`.
pub fn mpi_scsi_cmd(xs: &'static ScsiXfer) {
    let link = xs.link();
    let sc = mpi_link_softc(link);

    kernel_unlock();

    'done: {
        'stuffup: {
            if xs.cmdlen.get() > MPI_CDB_LEN as i32 {
                xs.sense.set(ScsiSenseData {
                    error_code: SSD_ERRCODE_VALID | SSD_ERRCODE_CURRENT,
                    flags: SKEY_ILLEGAL_REQUEST,
                    add_sense_code: 0x20,
                    ..Default::default()
                });
                xs.error.set(XS_SENSE);
                break 'done;
            }

            let ccb = mpi_xs_ccb(xs);

            ccb.ccb_cookie.set(MpiCookie::Xs(xs));
            ccb.ccb_done.set(Some(mpi_scsi_cmd_done));

            // SAFETY: the transfer's opening is ours until the transfer completes.
            let mcb = unsafe { ccb.cmd::<MpiCcbBundle>() };
            let io = &mut mcb.mcb_io;

            io.function = MPI_FUNCTION_SCSI_IO_REQUEST;
            // bus is always 0
            io.target_id = link.target.get() as u8;

            io.cdb_length = xs.cmdlen.get() as u8;
            io.sense_buf_len = size_of::<ScsiSenseData>() as u8;
            io.msg_flags = MPI_SCSIIO_SENSE_BUF_ADDR_WIDTH_64;

            io.lun[0] = link.lun.get().to_be();

            io.direction = match xs.flags.get() & (SCSI_DATA_IN | SCSI_DATA_OUT) {
                SCSI_DATA_IN => MPI_SCSIIO_DIR_READ,
                SCSI_DATA_OUT => MPI_SCSIIO_DIR_WRITE,
                _ => MPI_SCSIIO_DIR_NONE,
            };

            if sc.sc_porttype.get() != MPI_PORTFACTS_PORTTYPE_SCSI
                && link.quirks.get() & SDEV_NOTAGS != 0
            {
                io.tagging = MPI_SCSIIO_ATTR_UNTAGGED;
            } else {
                io.tagging = MPI_SCSIIO_ATTR_SIMPLE_Q;
            }

            let cmd = xs.cmd.get();
            let cmdlen = xs.cmdlen.get().max(0) as usize;
            if cmdlen > 0 {
                io.cdb[0] = cmd.opcode;
                io.cdb[1..cmdlen].copy_from_slice(&cmd.bytes[..cmdlen - 1]);
            }

            io.data_length = (xs.datalen().max(0) as u32).to_le();

            io.sense_buf_low_addr =
                ((ccb.ccb_cmd_dva.get() + offset_of!(MpiCcbBundle, mcb_sense) as u64) as u32)
                    .to_le();

            if mpi_load_xs(ccb).is_err() {
                break 'stuffup;
            }

            timeout_set(
                &xs.stimeout,
                mpi_timeout_xs,
                ptr::from_ref(ccb).cast_mut().cast(),
            );

            if xs.flags.get() & SCSI_POLL != 0 {
                if mpi_poll(sc, ccb, xs.timeout.get()).is_err() {
                    break 'stuffup;
                }
            } else {
                mpi_start(sc, ccb);
            }

            kernel_lock();
            return;
        }

        // stuffup:
        xs.error.set(XS_DRIVER_STUFFUP);
    }

    // done:
    kernel_lock();
    scsi_done(xs);
}

/// `mpi_scsi_cmd_done`: finishes a SCSI transfer from the IOC's reply, if any.
pub fn mpi_scsi_cmd_done(ccb: &'static MpiCcb) {
    let sc = ccb.sc();
    let MpiCookie::Xs(xs) = ccb.ccb_cookie.get() else {
        panic(format_args!("mpi_scsi_cmd_done: ccb without a transfer"));
    };
    let dmap = ccb.ccb_dmamap;

    if xs.datalen() != 0 {
        bus_dmamap_sync(
            sc.dmat(),
            dmap,
            0,
            dmap.dm_mapsize.get(),
            if xs.flags.get() & SCSI_DATA_IN != 0 {
                BUS_DMASYNC_POSTREAD
            } else {
                BUS_DMASYNC_POSTWRITE
            },
        );

        bus_dmamap_unload(sc.dmat(), dmap);
    }

    // timeout_del
    xs.error.set(XS_NOERROR);
    xs.resid.set(0);

    let Some(rcb) = ccb.ccb_rcb.get() else {
        // no scsi error, we're ok so drop out early
        xs.status.set(SCSI_OK);
        kernel_lock();
        scsi_done(xs);
        kernel_unlock();
        return;
    };

    let sie: MpiMsgScsiIoError = mpi_rcb_reply(rcb);

    if sie.scsi_state & MPI_SCSIIO_ERR_STATE_NO_SCSI_STATUS != 0 {
        xs.status.set(SCSI_TERMINATED);
    } else {
        xs.status.set(sie.scsi_status);
    }
    xs.resid.set(0);

    let ioc_status = u16::from_le(sie.ioc_status);
    match ioc_status {
        MPI_IOCSTATUS_SCSI_DATA_UNDERRUN
        | MPI_IOCSTATUS_SUCCESS
        | MPI_IOCSTATUS_SCSI_RECOVERED_ERROR => {
            if ioc_status == MPI_IOCSTATUS_SCSI_DATA_UNDERRUN {
                xs.resid.set(
                    (xs.datalen().max(0) as u32).wrapping_sub(u32::from_le(sie.transfer_count))
                        as usize,
                );
            }
            xs.error.set(match xs.status.get() {
                SCSI_OK => XS_NOERROR,
                SCSI_CHECK => XS_SENSE,
                SCSI_BUSY | SCSI_QUEUE_FULL => XS_BUSY,
                _ => XS_DRIVER_STUFFUP,
            });
        }

        MPI_IOCSTATUS_BUSY | MPI_IOCSTATUS_INSUFFICIENT_RESOURCES => xs.error.set(XS_BUSY),

        MPI_IOCSTATUS_SCSI_INVALID_BUS
        | MPI_IOCSTATUS_SCSI_INVALID_TARGETID
        | MPI_IOCSTATUS_SCSI_DEVICE_NOT_THERE => xs.error.set(XS_SELTIMEOUT),

        MPI_IOCSTATUS_SCSI_IOC_TERMINATED | MPI_IOCSTATUS_SCSI_EXT_TERMINATED => {
            xs.error.set(XS_RESET)
        }

        _ => xs.error.set(XS_DRIVER_STUFFUP),
    }

    if sie.scsi_state & MPI_SCSIIO_ERR_STATE_AUTOSENSE_VALID != 0 {
        // SAFETY: the sense area of the ccb's request frame, written by the IOC (hence
        // volatile) and completed (the reply says so).
        let mcb = unsafe { ccb.cmd::<MpiCcbBundle>() };
        // SAFETY: as above; `ScsiSenseData` is plain bytes.
        xs.sense
            .set(unsafe { ptr::read_volatile(&raw const mcb.mcb_sense) });
    }

    mpi_push_reply(sc, rcb);
    kernel_lock();
    scsi_done(xs);
    kernel_unlock();
}

/// `mpi_timeout_xs`: `/* XXX */`.
pub fn mpi_timeout_xs(_arg: *mut c_void) {}

/// `mpi_load_xs`: loads the transfer's data and builds the scatter/gather list, chaining
/// when the request frame is full.
pub fn mpi_load_xs(ccb: &'static MpiCcb) -> Result<(), Errno> {
    let sc = ccb.sc();
    let MpiCookie::Xs(xs) = ccb.ccb_cookie.get() else {
        panic(format_args!("mpi_load_xs: ccb without a transfer"));
    };
    // SAFETY: the transfer's opening is ours until the transfer completes.
    let mcb = unsafe { ccb.cmd::<MpiCcbBundle>() };
    let dmap = ccb.ccb_dmamap;
    let sgl_off = offset_of!(MpiCcbBundle, mcb_sgl);
    let sge_size = size_of::<MpiSge>();

    if xs.datalen() == 0 {
        mcb.mcb_sgl[0].sg_hdr =
            (MPI_SGE_FL_TYPE_SIMPLE | MPI_SGE_FL_LAST | MPI_SGE_FL_EOB | MPI_SGE_FL_EOL).to_le();
        return Ok(());
    }

    // SAFETY: `set_data`'s contract: `xs.data()` is valid for `datalen` bytes and reserved
    // for this transfer until it completes, which unloads the map (mpi_scsi_cmd_done).
    let error = unsafe {
        bus_dmamap_load(
            sc.dmat(),
            dmap,
            xs.data(),
            xs.datalen().max(0) as usize,
            None,
            BUS_DMA_STREAMING
                | if xs.flags.get() & SCSI_NOSLEEP != 0 {
                    BUS_DMA_NOWAIT
                } else {
                    BUS_DMA_WAITOK
                },
        )
    };
    if let Err(e) = error {
        printf(format_args!(
            "{}: error {} loading dmamap\n",
            devname(sc),
            e as i32
        ));
        return Err(e);
    }

    let mut flags = MPI_SGE_FL_TYPE_SIMPLE | MPI_SGE_FL_SIZE_64;
    if xs.flags.get() & SCSI_DATA_OUT != 0 {
        flags |= MPI_SGE_FL_DIR_OUT;
    }

    let nsegs = dmap.dm_nsegs.get().max(0) as usize;
    let first_sgl_len = sc.sc_first_sgl_len.get() as usize;
    let chain_len = sc.sc_chain_len.get() as usize;
    let mut sge: Option<usize> = None;
    let mut nsge: usize = 0;
    let mut ce: Option<usize> = None;

    if nsegs > first_sgl_len {
        ce = Some(first_sgl_len - 1);
        // (u32 *)ce - (u32 *)io
        mcb.mcb_io.chain_offset = ((sgl_off + (first_sgl_len - 1) * sge_size) / 4) as u8;
    }

    let segs = dmap.dm_segs();
    for (i, seg) in segs.iter().enumerate().take(nsegs) {
        if Some(nsge) == ce {
            nsge += 1;
            if let Some(s) = sge {
                mcb.mcb_sgl[s].sg_hdr |= MPI_SGE_FL_LAST.to_le();
            }

            let nce;
            let addr;
            if (nsegs - i) > chain_len {
                nce = Some(nsge + chain_len - 1);
                // (u32 *)nce - (u32 *)nsge
                let words = ((chain_len - 1) * sge_size / 4) as u32;
                addr = words << 16 | (sge_size * chain_len) as u32;
            } else {
                nce = None;
                addr = (sge_size * (nsegs - i)) as u32;
            }

            let cei = ce.unwrap_or(0);
            mcb.mcb_sgl[cei].sg_hdr = (MPI_SGE_FL_TYPE_CHAIN | MPI_SGE_FL_SIZE_64 | addr).to_le();

            mpi_dvatosge(
                &mut mcb.mcb_sgl[cei],
                ccb.ccb_cmd_dva.get() + (sgl_off + nsge * sge_size) as u64,
            );

            ce = nce;
        }

        let s = nsge;
        sge = Some(s);
        nsge += 1;

        let seg = seg.get();
        mcb.mcb_sgl[s].sg_hdr = (flags | seg.ds_len as u32).to_le();
        mpi_dvatosge(&mut mcb.mcb_sgl[s], seg.ds_addr as u64);
    }

    // terminate list
    if let Some(s) = sge {
        mcb.mcb_sgl[s].sg_hdr |= (MPI_SGE_FL_LAST | MPI_SGE_FL_EOB | MPI_SGE_FL_EOL).to_le();
    }

    bus_dmamap_sync(
        sc.dmat(),
        dmap,
        0,
        dmap.dm_mapsize.get(),
        if xs.flags.get() & SCSI_DATA_IN != 0 {
            BUS_DMASYNC_PREREAD
        } else {
            BUS_DMASYNC_PREWRITE
        },
    );

    Ok(())
}

/// `mpi_scsi_probe_virtual`: marks a RAID volume's link `SDEV_VIRTUAL`.
pub fn mpi_scsi_probe_virtual(link: &'static ScsiLink) -> Result<(), Errno> {
    let sc = mpi_link_softc(link);
    let mut hdr = MpiCfgHdr::default();

    if sc.sc_flags.get() & MPI_F_RAID == 0 {
        return Ok(());
    }

    if link.lun.get() > 0 {
        return Ok(());
    }

    if mpi_req_cfg_header(
        sc,
        MPI_CONFIG_REQ_PAGE_TYPE_RAID_VOL,
        0,
        u32::from(link.target.get()),
        MPI_PG_POLL,
        &mut hdr,
    )
    .is_err()
    {
        return Ok(());
    }

    let len = usize::from(hdr.page_length) * 4;
    let Some(rp0) = malloc(len, M_TEMP, M_NOWAIT) else {
        return Err(Errno::ENOMEM);
    };
    // SAFETY: `len` bytes just allocated, owned here until freed below.
    let bytes = unsafe { core::slice::from_raw_parts_mut(rp0.as_ptr(), len) };

    if mpi_req_cfg_page(
        sc,
        u32::from(link.target.get()),
        MPI_PG_POLL,
        &hdr,
        true,
        bytes,
    )
    .is_ok()
    {
        link.flags.set(link.flags.get() | SDEV_VIRTUAL);
    }

    free(rp0, M_TEMP, len);
    Ok(())
}

/// `mpi_scsi_probe`: the adapter's `dev_probe`: volumes, and ATAPI devices on SAS.
pub fn mpi_scsi_probe(link: &'static ScsiLink) -> Result<(), Errno> {
    let sc = mpi_link_softc(link);
    let mut ehdr = MpiEcfgHdr::default();
    let mut pg0 = MpiCfgSasDevPg0::default();

    mpi_scsi_probe_virtual(link)?;

    if link.flags.get() & SDEV_VIRTUAL != 0 {
        return Ok(());
    }

    if sc.sc_porttype.get() != MPI_PORTFACTS_PORTTYPE_SAS {
        return Ok(());
    }

    let address = MPI_CFG_SAS_DEV_ADDR_BUS | u32::from(link.target.get());

    if mpi_ecfg_header(
        sc,
        MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_DEVICE,
        0,
        address,
        &mut ehdr,
    )
    .is_err()
    {
        return Err(Errno::EIO);
    }

    if mpi_ecfg_page(sc, address, &ehdr, true, &mut pg0).is_err() {
        return Ok(());
    }

    if u32::from_le(pg0.device_info) & MPI_CFG_SAS_DEV_0_DEVINFO_ATAPI_DEVICE != 0 {
        link.flags.set(link.flags.get() | SDEV_ATAPI);
    }

    Ok(())
}

/// `mpi_read`.
pub fn mpi_read(sc: &'static MpiSoftc, r: BusSize) -> u32 {
    let (t, h) = sc.regs();
    bus_space_barrier(t, h, r, 4, BUS_SPACE_BARRIER_READ);
    bus_space_read_4(t, h, r)
}

/// `mpi_write`.
pub fn mpi_write(sc: &'static MpiSoftc, r: BusSize, v: u32) {
    let (t, h) = sc.regs();
    bus_space_write_4(t, h, r, v);
    bus_space_barrier(t, h, r, 4, BUS_SPACE_BARRIER_WRITE);
}

/// `mpi_wait_eq`: waits (up to ten seconds) for `reg & mask == target`.
pub fn mpi_wait_eq(sc: &'static MpiSoftc, r: BusSize, mask: u32, target: u32) -> Result<(), Errno> {
    for _ in 0..10000 {
        if mpi_read(sc, r) & mask == target {
            return Ok(());
        }
        delay(1000);
    }

    Err(Errno::EIO)
}

/// `mpi_wait_ne`: waits (up to ten seconds) for `reg & mask != target`.
pub fn mpi_wait_ne(sc: &'static MpiSoftc, r: BusSize, mask: u32, target: u32) -> Result<(), Errno> {
    for _ in 0..10000 {
        if mpi_read(sc, r) & mask != target {
            return Ok(());
        }
        delay(1000);
    }

    Err(Errno::EIO)
}

/// `mpi_init`: gets the IOC to the READY state.
pub fn mpi_init(sc: &'static MpiSoftc) -> Result<(), Errno> {
    // spin until the IOC leaves the RESET state
    if mpi_wait_ne(
        sc,
        MPI_DOORBELL,
        MPI_DOORBELL_STATE,
        MPI_DOORBELL_STATE_RESET,
    )
    .is_err()
    {
        return Err(Errno::EIO);
    }

    // check current ownership
    let mut db = mpi_read_db(sc);
    if db & MPI_DOORBELL_WHOINIT == MPI_DOORBELL_WHOINIT_PCIPEER {
        return Ok(());
    }

    for _ in 0..5 {
        match db & MPI_DOORBELL_STATE {
            MPI_DOORBELL_STATE_READY => return Ok(()),

            MPI_DOORBELL_STATE_OPER | MPI_DOORBELL_STATE_FAULT => {
                if mpi_reset_soft(sc).is_err() {
                    let _ = mpi_reset_hard(sc);
                }
            }

            MPI_DOORBELL_STATE_RESET
                if mpi_wait_ne(
                    sc,
                    MPI_DOORBELL,
                    MPI_DOORBELL_STATE,
                    MPI_DOORBELL_STATE_RESET,
                )
                .is_err() =>
            {
                return Err(Errno::EIO);
            }
            _ => {}
        }
        db = mpi_read_db(sc);
    }

    Err(Errno::EIO)
}

/// `mpi_reset_soft`: a message unit reset.
pub fn mpi_reset_soft(sc: &'static MpiSoftc) -> Result<(), Errno> {
    if mpi_read_db(sc) & MPI_DOORBELL_INUSE != 0 {
        return Err(Errno::EIO);
    }

    mpi_write_db(
        sc,
        mpi_doorbell_function(u32::from(MPI_FUNCTION_IOC_MESSAGE_UNIT_RESET)),
    );
    mpi_wait_eq(sc, MPI_INTR_STATUS, MPI_INTR_STATUS_IOCDOORBELL, 0)?;

    mpi_wait_eq(
        sc,
        MPI_DOORBELL,
        MPI_DOORBELL_STATE,
        MPI_DOORBELL_STATE_READY,
    )?;

    Ok(())
}

/// `mpi_reset_hard`: resets the adapter through the diagnostic register.
pub fn mpi_reset_hard(sc: &'static MpiSoftc) -> Result<(), Errno> {
    // enable diagnostic register
    mpi_write(sc, MPI_WRITESEQ, 0xff);
    mpi_write(sc, MPI_WRITESEQ, MPI_WRITESEQ_1);
    mpi_write(sc, MPI_WRITESEQ, MPI_WRITESEQ_2);
    mpi_write(sc, MPI_WRITESEQ, MPI_WRITESEQ_3);
    mpi_write(sc, MPI_WRITESEQ, MPI_WRITESEQ_4);
    mpi_write(sc, MPI_WRITESEQ, MPI_WRITESEQ_5);

    // reset ioc
    mpi_write(sc, MPI_HOSTDIAG, MPI_HOSTDIAG_RESET_ADAPTER);

    delay(10000);

    // disable diagnostic register
    mpi_write(sc, MPI_WRITESEQ, 0xff);

    // restore pci bits?

    // firmware bits?
    Ok(())
}

/// `mpi_handshake_send`: sends a message through the doorbell.
pub fn mpi_handshake_send(sc: &'static MpiSoftc, query: &[u32]) -> Result<(), Errno> {
    // make sure the doorbell is not in use.
    if mpi_read_db(sc) & MPI_DOORBELL_INUSE != 0 {
        return Err(Errno::EIO);
    }

    // clear pending doorbell interrupts
    if mpi_read_intr(sc) & MPI_INTR_STATUS_DOORBELL != 0 {
        mpi_write_intr(sc, 0);
    }

    // first write the doorbell with the handshake function and the dword count.
    mpi_write_db(
        sc,
        mpi_doorbell_function(u32::from(MPI_FUNCTION_HANDSHAKE))
            | mpi_doorbell_dwords(query.len() as u32),
    );

    // the doorbell used bit will be set because a doorbell function has started. Wait for
    // the interrupt and then ack it.
    mpi_wait_db_int(sc)?;
    mpi_write_intr(sc, 0);

    // poll for the acknowledgement.
    mpi_wait_db_ack(sc)?;

    // write the query through the doorbell.
    for q in query {
        mpi_write_db(sc, q.to_le());
        mpi_wait_db_ack(sc)?;
    }

    Ok(())
}

/// `mpi_handshake_recv_dword`: one dword of a reply, as two doorbell words.
pub fn mpi_handshake_recv_dword(sc: &'static MpiSoftc, dword: &mut u32) -> Result<(), Errno> {
    let mut bytes = dword.to_ne_bytes();

    for i in 0..2 {
        mpi_wait_db_int(sc)?;
        let word = u16::from_le((mpi_read_db(sc) & MPI_DOORBELL_DATA_MASK) as u16);
        bytes[i * 2..i * 2 + 2].copy_from_slice(&word.to_ne_bytes());
        mpi_write_intr(sc, 0);
    }

    *dword = u32::from_ne_bytes(bytes);
    Ok(())
}

/// `mpi_handshake_recv`: a reply of up to `buf.len()` dwords through the doorbell.
pub fn mpi_handshake_recv(sc: &'static MpiSoftc, buf: &mut [u32]) -> Result<(), Errno> {
    let dwords = buf.len();
    let mut dummy = 0u32;

    // get the first dword so we can read the length out of the header.
    mpi_handshake_recv_dword(sc, &mut buf[0])?;

    // the total length, in dwords, is in the message length field of the reply header
    // (byte 2 of `struct mpi_msg_reply`).
    let msg_length = ((buf[0].to_le() >> 16) & 0xff) as usize;

    let mut i = 1;
    while i < dwords.min(msg_length) {
        mpi_handshake_recv_dword(sc, &mut buf[i])?;
        i += 1;
    }

    // if there's extra stuff to come off the ioc, discard it
    loop {
        let more = i < msg_length;
        i += 1;
        if !more {
            break;
        }
        mpi_handshake_recv_dword(sc, &mut dummy)?;
    }

    // wait for the doorbell used bit to be reset and clear the intr
    mpi_wait_db_int(sc)?;
    mpi_write_intr(sc, 0);

    Ok(())
}

/// `mpi_empty_done`.
pub fn mpi_empty_done(_ccb: &'static MpiCcb) {
    // nothing to do
}

/// `mpi_iocfacts`: asks the IOC what it can do and sizes the driver to it.
pub fn mpi_iocfacts(sc: &'static MpiSoftc) -> Result<(), Errno> {
    let mut ifq = MpiMsgIocfactsRequest::default();
    let mut ifp = MpiMsgIocfactsReply::default();

    ifq.function = MPI_FUNCTION_IOC_FACTS;
    ifq.chain_offset = 0;
    ifq.msg_flags = 0;
    ifq.msg_context = 0xdead_beefu32.to_le();

    mpi_handshake_send(sc, mpi_dwords(&mut ifq))?;

    mpi_handshake_recv(sc, mpi_dwords(&mut ifp))?;

    sc.sc_fw_maj.set(ifp.fw_version_maj);
    sc.sc_fw_min.set(ifp.fw_version_min);
    sc.sc_fw_unit.set(ifp.fw_version_unit);
    sc.sc_fw_dev.set(ifp.fw_version_dev);

    sc.sc_maxcmds
        .set(i32::from(u16::from_le(ifp.global_credits)));
    sc.sc_maxchdepth.set(i32::from(ifp.max_chain_depth));
    sc.sc_ioc_number.set(i32::from(ifp.ioc_number));
    if sc.sc_flags.get() & MPI_F_SPI != 0 {
        sc.sc_buswidth.set(16);
    } else {
        sc.sc_buswidth.set(if ifp.max_devices == 0 {
            256
        } else {
            i32::from(ifp.max_devices)
        });
    }
    if ifp.flags & MPI_IOCFACTS_FLAGS_FW_DOWNLOAD_BOOT != 0 {
        sc.sc_fw_len.set(u32::from_le(ifp.fw_image_size) as usize);
    }

    sc.sc_repq
        .set((MPI_REPLYQ_DEPTH as i32).min(i32::from(u16::from_le(ifp.reply_queue_depth))));

    // you can fit sg elements on the end of the io cmd if they fit in the request frame
    // size.
    let request_frame_size = i32::from(u16::from_le(ifp.request_frame_size));
    sc.sc_first_sgl_len.set(
        (request_frame_size * 4 - size_of::<MpiMsgScsiIo>() as i32) / size_of::<MpiSge>() as i32,
    );

    sc.sc_chain_len
        .set((request_frame_size * 4) / size_of::<MpiSge>() as i32);

    // the sgl tailing the io cmd loses an entry to the chain element.
    sc.sc_max_sgl_len.set(MPI_MAX_SGL as i32 - 1);
    // the sgl chains lose an entry for each chain element
    sc.sc_max_sgl_len.set(
        sc.sc_max_sgl_len.get()
            - (MPI_MAX_SGL as i32 - sc.sc_first_sgl_len.get()) / sc.sc_chain_len.get(),
    );

    // XXX we're ignoring the max chain depth

    Ok(())
}

/// `mpi_iocinit`: tells the IOC the driver's frame sizes and starts it.
pub fn mpi_iocinit(sc: &'static MpiSoftc) -> Result<(), Errno> {
    let mut iiq = MpiMsgIocinitRequest::default();
    let mut iip = MpiMsgIocinitReply::default();

    iiq.function = MPI_FUNCTION_IOC_INIT;
    iiq.whoinit = MPI_WHOINIT_HOST_DRIVER;

    iiq.max_devices = if sc.sc_buswidth.get() == 256 {
        0
    } else {
        sc.sc_buswidth.get() as u8
    };
    iiq.max_buses = 1;

    iiq.msg_context = 0xd00f_d00fu32.to_le();

    iiq.reply_frame_size = (MPI_REPLY_SIZE as u16).to_le();

    let hi_addr = (mpi_dma_dva(sc.requests()) >> 32) as u32;
    iiq.host_mfa_hi_addr = hi_addr.to_le();
    iiq.sense_buffer_hi_addr = hi_addr.to_le();

    iiq.msg_version_maj = 0x01;
    iiq.msg_version_min = 0x02;

    iiq.hdr_version_unit = 0x0d;
    iiq.hdr_version_dev = 0x00;

    mpi_handshake_send(sc, mpi_dwords(&mut iiq))?;

    mpi_handshake_recv(sc, mpi_dwords(&mut iip))?;

    Ok(())
}

/// `mpi_portfacts`: the port's type and SCSI id.
pub fn mpi_portfacts(sc: &'static MpiSoftc) -> Result<(), Errno> {
    let Some(ccb) = mpi_scsi_io_get(sc, SCSI_NOSLEEP) else {
        return Err(Errno::EIO);
    };

    ccb.ccb_done.set(Some(mpi_empty_done));
    // SAFETY: the ccb was just taken from the pool: its frame is ours.
    let pfq = unsafe { ccb.cmd::<MpiMsgPortfactsRequest>() };

    pfq.function = MPI_FUNCTION_PORT_FACTS;
    pfq.chain_offset = 0;
    pfq.msg_flags = 0;
    pfq.port_number = 0;

    let rv = 'err: {
        if mpi_poll(sc, ccb, 50000).is_err() {
            break 'err Err(Errno::EIO);
        }

        let Some(rcb) = ccb.ccb_rcb.get() else {
            // empty portfacts reply
            break 'err Err(Errno::EIO);
        };
        let pfp: MpiMsgPortfactsReply = mpi_rcb_reply(rcb);

        sc.sc_porttype.set(pfp.port_type);
        if sc.sc_target.get() == -1 {
            sc.sc_target.set(i32::from(u16::from_le(pfp.port_scsi_id)));
        }

        mpi_push_reply(sc, rcb);
        Ok(())
    };

    mpi_scsi_io_put(sc, ccb);

    rv
}

/// `mpi_cfg_coalescing`: turns reply coalescing off.
pub fn mpi_cfg_coalescing(sc: &'static MpiSoftc) -> Result<(), Errno> {
    let mut hdr = MpiCfgHdr::default();
    let mut pg = MpiCfgIocPg1::default();

    mpi_cfg_header(sc, MPI_CONFIG_REQ_PAGE_TYPE_IOC, 1, 0, &mut hdr)?;

    mpi_cfg_page(sc, 0, &hdr, true, &mut pg)?;

    let flags = u32::from_le(pg.flags);
    if flags & MPI_CFG_IOC_1_REPLY_COALESCING == 0 {
        return Ok(());
    }

    pg.flags &= !MPI_CFG_IOC_1_REPLY_COALESCING.to_le();
    mpi_cfg_page(sc, 0, &hdr, false, &mut pg)?;

    Ok(())
}

/// `mpi_eventnotify`: asks the IOC for event notifications, with one ccb kept for them.
pub fn mpi_eventnotify(sc: &'static MpiSoftc) -> Result<(), Errno> {
    let Some(ccb) = mpi_scsi_io_get(sc, SCSI_NOSLEEP) else {
        return Err(Errno::EIO);
    };

    sc.sc_evt_ccb.set(Some(ccb));
    sc.sc_evt_ack_queue.init();
    mtx_init(&sc.sc_evt_ack_mtx, IPL_BIO);
    // SAFETY: `mpi_eventack` takes the softc as its cookie, which is never freed while the
    // handler is queued.
    unsafe {
        scsi_ioh_set(
            &sc.sc_evt_ack_handler,
            &sc.sc_iopool,
            mpi_eventack,
            sc.cookie(),
        )
    };

    ccb.ccb_done.set(Some(mpi_eventnotify_done));
    // SAFETY: the ccb was just taken from the pool: its frame is ours.
    let enq = unsafe { ccb.cmd::<MpiMsgEventRequest>() };

    enq.function = MPI_FUNCTION_EVENT_NOTIFICATION;
    enq.chain_offset = 0;
    enq.event_switch = MPI_EVENT_SWITCH_ON;

    mpi_start(sc, ccb);
    Ok(())
}

/// `mpi_eventnotify_done`.
pub fn mpi_eventnotify_done(ccb: &'static MpiCcb) {
    let sc = ccb.sc();
    let Some(rcb) = ccb.ccb_rcb.get() else {
        panic(format_args!("{}: event without a reply", devname(sc)));
    };
    let enp: MpiMsgEventReply = mpi_rcb_reply(rcb);

    match u32::from_le(enp.event) {
        // ignore these
        MPI_EVENT_EVENT_CHANGE | MPI_EVENT_SAS_PHY_LINK_STATUS => {}

        MPI_EVENT_SAS_DEVICE_STATUS_CHANGE => {
            if sc.sc_scsibus.get().is_some() && mpi_evt_sas(sc, rcb) {
                // reply is freed later on
                return;
            }
        }

        MPI_EVENT_RESCAN
            if sc.sc_scsibus.get().is_some()
                && sc.sc_porttype.get() == MPI_PORTFACTS_PORTTYPE_FC =>
        {
            task_add(SYSTQ, &sc.sc_evt_rescan);
        }

        _ => {}
    }

    mpi_eventnotify_free(sc, rcb);
}

/// `mpi_eventnotify_free`: acknowledges the event, or gives the reply back.
pub fn mpi_eventnotify_free(sc: &'static MpiSoftc, rcb: &'static MpiRcb) {
    let enp: MpiMsgEventReply = mpi_rcb_reply(rcb);

    if enp.ack_required != 0 {
        mtx_enter(&sc.sc_evt_ack_mtx);
        // SAFETY: the rcb is on no list and stays in `sc_rcbs`.
        unsafe { sc.sc_evt_ack_queue.insert_tail(rcb) };
        mtx_leave(&sc.sc_evt_ack_mtx);
        scsi_ioh_add(&sc.sc_evt_ack_handler);
    } else {
        mpi_push_reply(sc, rcb);
    }
}

/// `mpi_evt_sas`: a SAS device status change; true when the reply is kept for later.
pub fn mpi_evt_sas(sc: &'static MpiSoftc, rcb: &'static MpiRcb) -> bool {
    // SAFETY: the event data follows the reply header in the 80-byte frame
    // (`struct mpi_evt_sas_change` fits); it is plain bytes (`MpiPod`).
    let ch: MpiEvtSasChange = unsafe {
        ptr::read_volatile(
            rcb.rcb_reply
                .get()
                .add(size_of::<MpiMsgEventReply>())
                .cast::<MpiEvtSasChange>(),
        )
    };
    let Some(bus) = sc.sc_scsibus.get() else {
        return false;
    };

    if ch.bus != 0 {
        return false;
    }

    match ch.reason {
        MPI_EVT_SASCH_REASON_ADDED | MPI_EVT_SASCH_REASON_NO_PERSIST_ADDED => {
            kernel_lock();
            if scsi_req_probe(bus, i32::from(ch.target), -1).is_err() {
                printf(format_args!(
                    "{}: unable to request attach of {}\n",
                    devname(sc),
                    ch.target
                ));
            }
            kernel_unlock();
        }

        MPI_EVT_SASCH_REASON_NOT_RESPONDING => {
            kernel_lock();
            let _ = scsi_activate(bus, i32::from(ch.target), -1, DVACT_DEACTIVATE);
            kernel_unlock();

            mtx_enter(&sc.sc_evt_scan_mtx);
            // SAFETY: the rcb is on no list and stays in `sc_rcbs`.
            unsafe { sc.sc_evt_scan_queue.insert_tail(rcb) };
            mtx_leave(&sc.sc_evt_scan_mtx);
            scsi_ioh_add(&sc.sc_evt_scan_handler);

            // we'll handle event ack later on
            return true;
        }

        MPI_EVT_SASCH_REASON_SMART_DATA
        | MPI_EVT_SASCH_REASON_UNSUPPORTED
        | MPI_EVT_SASCH_REASON_INTERNAL_RESET => {}
        _ => {
            printf(format_args!(
                "{}: unknown reason for SAS device status change: 0x{:02x}\n",
                devname(sc),
                ch.reason
            ));
        }
    }

    false
}

/// `mpi_evt_sas_detach`: the scan handler: resets the target that stopped responding.
///
/// # Safety
///
/// `cookie` is the [`MpiSoftc`] `mpi_attach` gave `scsi_ioh_set`.
pub unsafe fn mpi_evt_sas_detach(cookie: *mut c_void, io: Option<ScsiIo>) {
    // SAFETY: the caller's guarantee.
    let sc: &'static MpiSoftc = unsafe { &*cookie.cast::<MpiSoftc>().cast_const() };
    let Some(io) = io else {
        return;
    };
    // SAFETY: an opening of `sc_iopool`, whose `io_get` is `mpi_get_ccb_io`.
    let ccb: &'static MpiCcb = unsafe { io.cast::<MpiCcb>().as_ref() };

    mtx_enter(&sc.sc_evt_scan_mtx);
    let first = sc.sc_evt_scan_queue.first().map(NonNull::from);
    let mut next = None;
    if let Some(rcb) = first {
        // SAFETY: an rcb of `sc_rcbs`, on the queue.
        next = crate::sys::queue::SimpleqHead::<MpiRcbList>::next(unsafe { rcb.as_ref() });
        // SAFETY: the queue is not empty.
        unsafe { sc.sc_evt_scan_queue.remove_head() };
    }
    let next = next.is_some();
    mtx_leave(&sc.sc_evt_scan_mtx);

    let Some(rcb) = first else {
        mpi_scsi_io_put(sc, ccb);
        return;
    };
    // SAFETY: as above.
    let rcb: &'static MpiRcb = unsafe { &*rcb.as_ptr() };

    // SAFETY: the event data follows the reply header in the frame; plain bytes.
    let ch: MpiEvtSasChange = unsafe {
        ptr::read_volatile(
            rcb.rcb_reply
                .get()
                .add(size_of::<MpiMsgEventReply>())
                .cast::<MpiEvtSasChange>(),
        )
    };

    ccb.ccb_done.set(Some(mpi_evt_sas_detach_done));
    // SAFETY: the ccb is the opening just handed to us: its frame is ours.
    let str_ = unsafe { ccb.cmd::<MpiMsgScsiTaskRequest>() };

    str_.target_id = ch.target;
    str_.bus = 0;
    str_.function = MPI_FUNCTION_SCSI_TASK_MGMT;

    str_.task_type = MPI_MSG_SCSI_TASK_TYPE_TARGET_RESET;

    mpi_eventnotify_free(sc, rcb);

    mpi_start(sc, ccb);

    if next {
        scsi_ioh_add(&sc.sc_evt_scan_handler);
    }
}

/// `mpi_evt_sas_detach_done`.
pub fn mpi_evt_sas_detach_done(ccb: &'static MpiCcb) {
    let sc = ccb.sc();
    let Some(rcb) = ccb.ccb_rcb.get() else {
        panic(format_args!("{}: task reply missing", devname(sc)));
    };
    let r: MpiMsgScsiTaskReply = mpi_rcb_reply(rcb);

    kernel_lock();
    if let Some(bus) = sc.sc_scsibus.get()
        && scsi_req_detach(bus, i32::from(r.target_id), -1, DETACH_FORCE).is_err()
    {
        printf(format_args!(
            "{}: unable to request detach of {}\n",
            devname(sc),
            r.target_id
        ));
    }
    kernel_unlock();

    mpi_push_reply(sc, rcb);
    mpi_scsi_io_put(sc, ccb);
}

/// `mpi_fc_rescan`: the rescan task: finds the devices the fabric shows and probes or
/// detaches targets to match.
pub fn mpi_fc_rescan(xsc: *mut c_void) {
    // SAFETY: registered by mpi_attach with the softc as the argument; softcs are never
    // freed while their task may run.
    let sc: &'static MpiSoftc = unsafe { &*xsc.cast::<MpiSoftc>().cast_const() };
    let mut hdr = MpiCfgHdr::default();
    let mut devmap = [0u8; 256 / 8];
    let mut id: u32 = 0xff_ffff;

    loop {
        if mpi_req_cfg_header(sc, MPI_CONFIG_REQ_PAGE_TYPE_FC_DEV, 0, id, 0, &mut hdr).is_err() {
            printf(format_args!(
                "{}: header get for rescan of 0x{:08x} failed\n",
                devname(sc),
                id
            ));
            return;
        }

        let mut pg = MpiCfgFcDevicePg0::default();
        if mpi_req_cfg_page(sc, id, 0, &hdr, true, mpi_pod_bytes_mut(&mut pg)).is_err() {
            break;
        }

        if pg.flags & MPI_CFG_FC_DEV_0_FLAGS_BUSADDR_VALID != 0 && pg.current_bus == 0 {
            crate::sys::param::setbit(&mut devmap, usize::from(pg.current_target_id));
        }

        id = u32::from_le(pg.port_id);
        if id > 0xff_0000 {
            break;
        }
    }

    let Some(bus) = sc.sc_scsibus.get() else {
        return;
    };
    for i in 0..sc.sc_buswidth.get() {
        let link = scsi_get_link(bus, i, 0);

        if crate::sys::param::isset(&devmap, i as usize) {
            if link.is_none() {
                let _ = scsi_probe_target(bus, i);
            }
        } else if link.is_some() {
            let _ = scsi_activate(bus, i, -1, DVACT_DEACTIVATE);
            let _ = scsi_detach_target(bus, i, DETACH_FORCE);
        }
    }
}

/// `mpi_eventack`: the ack handler: acknowledges the first queued event.
///
/// # Safety
///
/// `cookie` is the [`MpiSoftc`] `mpi_eventnotify` gave `scsi_ioh_set`.
pub unsafe fn mpi_eventack(cookie: *mut c_void, io: Option<ScsiIo>) {
    // SAFETY: the caller's guarantee.
    let sc: &'static MpiSoftc = unsafe { &*cookie.cast::<MpiSoftc>().cast_const() };
    let Some(io) = io else {
        return;
    };
    // SAFETY: an opening of `sc_iopool`, whose `io_get` is `mpi_get_ccb_io`.
    let ccb: &'static MpiCcb = unsafe { io.cast::<MpiCcb>().as_ref() };

    mtx_enter(&sc.sc_evt_ack_mtx);
    let first = sc.sc_evt_ack_queue.first().map(NonNull::from);
    let mut next = None;
    if let Some(rcb) = first {
        // SAFETY: an rcb of `sc_rcbs`, on the queue.
        next = crate::sys::queue::SimpleqHead::<MpiRcbList>::next(unsafe { rcb.as_ref() });
        // SAFETY: the queue is not empty.
        unsafe { sc.sc_evt_ack_queue.remove_head() };
    }
    let next = next.is_some();
    mtx_leave(&sc.sc_evt_ack_mtx);

    let Some(rcb) = first else {
        mpi_scsi_io_put(sc, ccb);
        return;
    };
    // SAFETY: as above.
    let rcb: &'static MpiRcb = unsafe { &*rcb.as_ptr() };

    let enp: MpiMsgEventReply = mpi_rcb_reply(rcb);

    ccb.ccb_done.set(Some(mpi_eventack_done));
    // SAFETY: the ccb is the opening just handed to us: its frame is ours.
    let eaq = unsafe { ccb.cmd::<MpiMsgEventackRequest>() };

    eaq.function = MPI_FUNCTION_EVENT_ACK;

    eaq.event = enp.event;
    eaq.event_context = enp.event_context;

    mpi_push_reply(sc, rcb);
    mpi_start(sc, ccb);

    if next {
        scsi_ioh_add(&sc.sc_evt_ack_handler);
    }
}

/// `mpi_eventack_done`.
pub fn mpi_eventack_done(ccb: &'static MpiCcb) {
    let sc = ccb.sc();

    if let Some(rcb) = ccb.ccb_rcb.get() {
        mpi_push_reply(sc, rcb);
    }
    mpi_scsi_io_put(sc, ccb);
}

/// `mpi_portenable`.
pub fn mpi_portenable(sc: &'static MpiSoftc) -> Result<(), Errno> {
    let mut rv = Ok(());

    let Some(ccb) = mpi_scsi_io_get(sc, SCSI_NOSLEEP) else {
        return Err(Errno::EIO);
    };

    ccb.ccb_done.set(Some(mpi_empty_done));
    // SAFETY: the ccb was just taken from the pool: its frame is ours.
    let peq = unsafe { ccb.cmd::<MpiMsgPortenableRequest>() };

    peq.function = MPI_FUNCTION_PORT_ENABLE;
    peq.port_number = 0;

    mpi_poll(sc, ccb, 50000)?;

    match ccb.ccb_rcb.get() {
        None => rv = Err(Errno::EIO),
        Some(rcb) => mpi_push_reply(sc, rcb),
    }

    mpi_scsi_io_put(sc, ccb);

    rv
}

/// The request frame of `mpi_fwupload`.
#[repr(C)]
struct MpiFwuploadBundle {
    req: MpiMsgFwuploadRequest,
    sge: MpiSge,
}

/// `mpi_fwupload`: uploads the IOC's firmware image into host memory, when it asks for it.
pub fn mpi_fwupload(sc: &'static MpiSoftc) -> Result<(), Errno> {
    let mut rv = Ok(());

    if sc.sc_fw_len.get() == 0 {
        return Ok(());
    }

    let Some(fw) = mpi_dmamem_alloc(sc, sc.sc_fw_len.get()) else {
        return Err(Errno::EIO);
    };
    sc.sc_fw.set(Some(fw));

    let err = 'err: {
        let Some(ccb) = mpi_scsi_io_get(sc, SCSI_NOSLEEP) else {
            break 'err true;
        };

        ccb.ccb_done.set(Some(mpi_empty_done));
        // SAFETY: the ccb was just taken from the pool: its frame is ours.
        let bundle = unsafe { ccb.cmd::<MpiFwuploadBundle>() };

        bundle.req.function = MPI_FUNCTION_FW_UPLOAD;

        bundle.req.image_type = MPI_FWUPLOAD_IMAGETYPE_IOC_FW;

        bundle.req.tce.details_length = 12;
        bundle.req.tce.image_size = (sc.sc_fw_len.get() as u32).to_le();

        bundle.sge.sg_hdr = (MPI_SGE_FL_TYPE_SIMPLE
            | MPI_SGE_FL_SIZE_64
            | MPI_SGE_FL_LAST
            | MPI_SGE_FL_EOB
            | MPI_SGE_FL_EOL
            | sc.sc_fw_len.get() as u32)
            .to_le();
        mpi_dvatosge(&mut bundle.sge, mpi_dma_dva(fw));

        if mpi_poll(sc, ccb, 50000).is_err() {
            break 'err true;
        }

        let Some(rcb) = ccb.ccb_rcb.get() else {
            panic(format_args!("{}: unable to do fw upload", devname(sc)));
        };
        let upp: MpiMsgFwuploadReply = mpi_rcb_reply(rcb);

        if u16::from_le(upp.ioc_status) != MPI_IOCSTATUS_SUCCESS {
            rv = Err(Errno::EIO);
        }

        mpi_push_reply(sc, rcb);
        mpi_scsi_io_put(sc, ccb);

        false
    };

    if err {
        // SAFETY: the firmware area allocated above, which the IOC does not use.
        unsafe { mpi_dmamem_free(sc, fw) };
        sc.sc_fw.set(None);
        return Err(Errno::EIO);
    }

    rv
}

/// `mpi_manufacturing`: reads the board name and prints the board and firmware version.
pub fn mpi_manufacturing(sc: &'static MpiSoftc) -> Result<(), Errno> {
    let mut board_name = [0u8; 33];
    let mut hdr = MpiCfgHdr::default();

    mpi_cfg_header(sc, MPI_CONFIG_REQ_PAGE_TYPE_MANUFACTURING, 0, 0, &mut hdr)?;

    let pagelen = usize::from(hdr.page_length) * 4; // dwords to bytes
    if pagelen < size_of::<MpiCfgManufacturingPg0>() {
        return Err(Errno::EIO);
    }

    let Some(buf) = malloc(pagelen, M_TEMP, M_WAITOK | M_CANFAIL) else {
        return Err(Errno::EIO);
    };
    // SAFETY: `pagelen` bytes just allocated, owned here until freed below.
    let bytes = unsafe { core::slice::from_raw_parts_mut(buf.as_ptr(), pagelen) };

    let rv = if mpi_cfg_page_bytes(sc, 0, &hdr, true, bytes).is_err() {
        Err(Errno::EIO)
    } else {
        let off = offset_of!(MpiCfgManufacturingPg0, board_name);
        scsi_strvis(&mut board_name, &bytes[off..off + 16]);

        printf(format_args!(
            "{}: {}, firmware {}.{}.{}.{}\n",
            devname(sc),
            crate::kern::subr_prf::Str(&board_name),
            sc.sc_fw_maj.get(),
            sc.sc_fw_min.get(),
            sc.sc_fw_unit.get(),
            sc.sc_fw_dev.get()
        ));

        Ok(())
    };

    free(buf, M_TEMP, pagelen);
    rv
}

/// `mpi_get_raid`: sets `MPI_F_RAID` when IOC page 2 says the controller has RAID.
pub fn mpi_get_raid(sc: &'static MpiSoftc) {
    let mut hdr = MpiCfgHdr::default();

    if mpi_cfg_header(sc, MPI_CONFIG_REQ_PAGE_TYPE_IOC, 2, 0, &mut hdr).is_err() {
        return;
    }

    let pagelen = usize::from(hdr.page_length) * 4; // dwords to bytes
    let Some(buf) = malloc(pagelen, M_TEMP, M_WAITOK | M_CANFAIL) else {
        return;
    };
    // SAFETY: `pagelen` bytes just allocated, owned here until freed below.
    let bytes = unsafe { core::slice::from_raw_parts_mut(buf.as_ptr(), pagelen) };

    'out: {
        if mpi_cfg_page_bytes(sc, 0, &hdr, true, bytes).is_err() {
            break 'out;
        }
        if pagelen < size_of::<MpiCfgIocPg2>() {
            break 'out;
        }

        // SAFETY: the buffer holds the page header (checked above); plain bytes.
        let vol_page: MpiCfgIocPg2 = unsafe { ptr::read_unaligned(bytes.as_ptr().cast()) };
        let capabilities = u32::from_le(vol_page.capabilities);

        // don't walk list if there are no RAID capability
        if capabilities == 0xdead_beef {
            printf(format_args!(
                "{}: deadbeef in raid configuration\n",
                devname(sc)
            ));
            break 'out;
        }

        if capabilities & MPI_CFG_IOC_2_CAPABILITIES_RAID != 0 {
            sc.sc_flags.set(sc.sc_flags.get() | MPI_F_RAID);
        }
    }

    free(buf, M_TEMP, pagelen);
}

/// `mpi_req_cfg_header`: the header of a configuration page (`H` states whether it is a
/// basic or an extended page).
pub fn mpi_req_cfg_header<H: MpiCfgHdrKind>(
    sc: &'static MpiSoftc,
    type_: u8,
    number: u8,
    address: u32,
    flags: i32,
    hdr: &mut H,
) -> Result<(), Errno> {
    let mut type_ = type_;
    let mut etype = 0;

    let Some(ccb) = mpi_scsi_io_get(
        sc,
        if flags & MPI_PG_POLL != 0 {
            SCSI_NOSLEEP
        } else {
            0
        },
    ) else {
        return Err(Errno::EIO);
    };

    if H::EXTENDED {
        etype = type_;
        type_ = MPI_CONFIG_REQ_PAGE_TYPE_EXTENDED;
    }

    // SAFETY: the ccb was just taken from the pool: its frame is ours.
    let cq = unsafe { ccb.cmd::<MpiMsgConfigRequest>() };

    cq.function = MPI_FUNCTION_CONFIG;

    cq.action = MPI_CONFIG_REQ_ACTION_PAGE_HEADER;

    cq.config_header.page_number = number;
    cq.config_header.page_type = type_;
    cq.ext_page_type = etype;
    cq.page_address = address.to_le();
    cq.page_buffer.sg_hdr =
        (MPI_SGE_FL_TYPE_SIMPLE | MPI_SGE_FL_LAST | MPI_SGE_FL_EOB | MPI_SGE_FL_EOL).to_le();

    ccb.ccb_done.set(Some(mpi_empty_done));
    if flags & MPI_PG_POLL != 0 {
        mpi_poll(sc, ccb, 50000)?;
    } else {
        mpi_wait(sc, ccb);
    }

    let Some(rcb) = ccb.ccb_rcb.get() else {
        panic(format_args!(
            "{}: unable to fetch config header",
            devname(sc)
        ));
    };
    let cp: MpiMsgConfigReply = mpi_rcb_reply(rcb);

    let rv = if u16::from_le(cp.ioc_status) != MPI_IOCSTATUS_SUCCESS {
        Err(Errno::EIO)
    } else {
        *hdr = H::from_reply(&cp);
        Ok(())
    };

    mpi_push_reply(sc, rcb);
    mpi_scsi_io_put(sc, ccb);

    rv
}

/// `mpi_req_cfg_page`: reads (`read`) or writes the page `hdr` names, bouncing it through
/// the request frame.
pub fn mpi_req_cfg_page<H: MpiCfgHdrKind>(
    sc: &'static MpiSoftc,
    address: u32,
    flags: i32,
    hdr: &H,
    read: bool,
    page: &mut [u8],
) -> Result<(), Errno> {
    let len = page.len();
    let page_length = hdr.page_length();

    if len > MPI_REQUEST_SIZE - size_of::<MpiMsgConfigRequest>() || len < page_length * 4 {
        return Err(Errno::EIO);
    }

    let Some(ccb) = mpi_scsi_io_get(
        sc,
        if flags & MPI_PG_POLL != 0 {
            SCSI_NOSLEEP
        } else {
            0
        },
    ) else {
        return Err(Errno::EIO);
    };

    // SAFETY: the ccb was just taken from the pool: its frame is ours.
    let cq = unsafe { ccb.cmd::<MpiMsgConfigRequest>() };

    cq.function = MPI_FUNCTION_CONFIG;

    cq.action = if read {
        MPI_CONFIG_REQ_ACTION_PAGE_READ_CURRENT
    } else {
        MPI_CONFIG_REQ_ACTION_PAGE_WRITE_CURRENT
    };

    hdr.to_request(cq);
    cq.config_header.page_type &= MPI_CONFIG_REQ_PAGE_TYPE_MASK;
    cq.page_address = address.to_le();
    cq.page_buffer.sg_hdr = (MPI_SGE_FL_TYPE_SIMPLE
        | MPI_SGE_FL_LAST
        | MPI_SGE_FL_EOB
        | MPI_SGE_FL_EOL
        | (page_length * 4) as u32
        | if read {
            MPI_SGE_FL_DIR_IN
        } else {
            MPI_SGE_FL_DIR_OUT
        })
    .to_le();

    // bounce the page via the request space to avoid more bus_dma games
    mpi_dvatosge(
        &mut cq.page_buffer,
        ccb.ccb_cmd_dva.get() + size_of::<MpiMsgConfigRequest>() as u64,
    );

    // SAFETY: the page area follows the request in the frame (`len` fits: checked above).
    let kva = unsafe { ccb.ccb_cmd.get().add(size_of::<MpiMsgConfigRequest>()) };
    if !read {
        // SAFETY: `len` bytes of the frame (checked above) from a slice of `len` bytes.
        unsafe { ptr::copy_nonoverlapping(page.as_ptr(), kva, len) };
    }

    ccb.ccb_done.set(Some(mpi_empty_done));
    if flags & MPI_PG_POLL != 0 {
        mpi_poll(sc, ccb, 50000)?;
    } else {
        mpi_wait(sc, ccb);
    }

    let Some(rcb) = ccb.ccb_rcb.get() else {
        mpi_scsi_io_put(sc, ccb);
        return Err(Errno::EIO);
    };
    let cp: MpiMsgConfigReply = mpi_rcb_reply(rcb);

    let rv = if u16::from_le(cp.ioc_status) != MPI_IOCSTATUS_SUCCESS {
        Err(Errno::EIO)
    } else {
        if read {
            // SAFETY: `len` bytes of the frame (checked above) into a slice of `len` bytes.
            unsafe { ptr::copy_nonoverlapping(kva, page.as_mut_ptr(), len) };
        }
        Ok(())
    };

    mpi_push_reply(sc, rcb);
    mpi_scsi_io_put(sc, ccb);

    rv
}

/// `mpi_scsi_ioctl`: the adapter's `ioctl`: the volume write cache, and the bio(4) commands.
///
/// # Safety
///
/// `addr` is the aligned kernel copy of the command's argument (`ScsiAdapterIoctlFn`).
pub unsafe fn mpi_scsi_ioctl(
    link: &'static ScsiLink,
    cmd: u64,
    addr: *mut u8,
    _flag: i32,
) -> Result<(), Errno> {
    let sc = mpi_link_softc(link);

    match cmd {
        DIOCGCACHE | DIOCSCACHE => {
            if link.flags.get() & SDEV_VIRTUAL != 0 {
                // SAFETY: the caller's guarantee: these commands carry a `struct dk_cache`.
                let dc = unsafe { &mut *addr.cast::<DkCache>() };
                return mpi_ioctl_cache(link, cmd, dc);
            }
        }

        _ => {
            if let Some(ioctl) = sc.sc_ioctl.get() {
                // SAFETY: the caller's guarantee: `addr` holds the command's argument,
                // `IOCPARM_LEN(cmd)` bytes.
                let data =
                    unsafe { core::slice::from_raw_parts_mut(addr, iocparm_len(cmd) as usize) };
                return ioctl(&sc.sc_dev, cmd, data);
            }
        }
    }

    Err(Errno::ENOTTY)
}

/// `mpi_ioctl_cache`: DIOCGCACHE and DIOCSCACHE on a RAID volume.
pub fn mpi_ioctl_cache(link: &'static ScsiLink, cmd: u64, dc: &mut DkCache) -> Result<(), Errno> {
    let sc = mpi_link_softc(link);
    let mut hdr = MpiCfgHdr::default();

    if mpi_req_cfg_header(
        sc,
        MPI_CONFIG_REQ_PAGE_TYPE_RAID_VOL,
        0,
        u32::from(link.target.get()),
        MPI_PG_POLL,
        &mut hdr,
    )
    .is_err()
    {
        return Err(Errno::EIO);
    }

    let vol_page = sc.sc_vol_page.get();
    if vol_page.is_null() {
        return Err(Errno::EIO);
    }
    // SAFETY: `mpi_attach` allocated and filled the page (`MPI_F_RAID`), never freed.
    let max_physdisks = usize::from(unsafe { (*vol_page).max_physdisks });
    let len = size_of::<MpiCfgRaidVolPg0>() + max_physdisks * size_of::<MpiCfgRaidVolPg0Physdisk>();
    let Some(buf) = malloc(len, M_TEMP, M_NOWAIT) else {
        return Err(Errno::ENOMEM);
    };
    // SAFETY: `len` bytes just allocated, owned here until freed below.
    let bytes = unsafe { core::slice::from_raw_parts_mut(buf.as_ptr(), len) };

    let rv = 'done: {
        if mpi_req_cfg_page(
            sc,
            u32::from(link.target.get()),
            MPI_PG_POLL,
            &hdr,
            true,
            bytes,
        )
        .is_err()
        {
            break 'done Err(Errno::EIO);
        }

        // SAFETY: `len` is at least the size of the page header; plain bytes.
        let rpg0: MpiCfgRaidVolPg0 = unsafe { ptr::read_unaligned(bytes.as_ptr().cast()) };

        let settings_vs = u16::from_le(rpg0.settings.volume_settings);
        let enabled = settings_vs & MPI_CFG_RAID_VOL_0_SETTINGS_WRITE_CACHE_EN != 0;

        if cmd == DIOCGCACHE {
            dc.wrcache = u32::from(enabled);
            dc.rdcache = 0;
            break 'done Ok(());
        } // else DIOCSCACHE

        if dc.rdcache != 0 {
            break 'done Err(Errno::EOPNOTSUPP);
        }

        if (dc.wrcache != 0) == enabled {
            break 'done Ok(());
        }

        let mut settings = rpg0.settings;
        if dc.wrcache != 0 {
            settings.volume_settings |= MPI_CFG_RAID_VOL_0_SETTINGS_WRITE_CACHE_EN.to_le();
        } else {
            settings.volume_settings &= !MPI_CFG_RAID_VOL_0_SETTINGS_WRITE_CACHE_EN.to_le();
        }

        let Some(ccb) = mpi_scsi_io_get(sc, SCSI_NOSLEEP) else {
            break 'done Err(Errno::ENOMEM);
        };

        // SAFETY: the ccb was just taken from the pool: its frame is ours.
        let req = unsafe { ccb.cmd::<MpiMsgRaidActionRequest>() };
        req.function = MPI_FUNCTION_RAID_ACTION;
        req.action = MPI_MSG_RAID_ACTION_CH_VOL_SETTINGS;
        req.vol_id = rpg0.volume_id;
        req.vol_bus = rpg0.volume_bus;

        let sb = (settings.volume_settings).to_ne_bytes();
        req.data_word =
            u32::from_ne_bytes([sb[0], sb[1], settings.hot_spare_pool, settings.reserved2]);
        ccb.ccb_done.set(Some(mpi_empty_done));
        if mpi_poll(sc, ccb, 50000).is_err() {
            break 'done Err(Errno::EIO);
        }

        // rep = (struct mpi_msg_raid_action_reply *)ccb->ccb_rcb;  (External bugs: the C
        // casts the rcb, not its reply)
        let Some(rcb) = ccb.ccb_rcb.get() else {
            panic(format_args!(
                "{}: raid volume settings change failed",
                devname(sc)
            ));
        };
        const { assert!(size_of::<MpiRcb>() >= offset_of!(MpiMsgRaidActionReply, action_status) + 2) };
        // SAFETY: the rcb is at least that long (checked above); the bytes are the rcb's
        // own, as in the C.
        let action_status = u16::from_le(unsafe {
            ptr::read_unaligned(
                ptr::from_ref(rcb)
                    .cast::<u8>()
                    .add(offset_of!(MpiMsgRaidActionReply, action_status))
                    .cast::<u16>(),
            )
        });

        let rv = match action_status {
            MPI_RAID_ACTION_STATUS_OK => Ok(()),
            _ => Err(Errno::EIO),
        };

        mpi_push_reply(sc, rcb);
        mpi_scsi_io_put(sc, ccb);

        rv
    };

    free(buf, M_TEMP, len);
    rv
}

/// A bio(4) argument structure, read out of the ioctl's bytes.
///
/// # Safety
///
/// The implementor is `#[repr(C)]` of integers, byte arrays, raw pointers and [`Bio`], so
/// every bit pattern is a valid value.
unsafe trait MpiBioArg: Copy {}

// SAFETY: a `Bio` (a raw pointer and integers), integers and byte arrays.
unsafe impl MpiBioArg for BiocInq {}
// SAFETY: as above.
unsafe impl MpiBioArg for BiocVol {}
// SAFETY: as above.
unsafe impl MpiBioArg for BiocDisk {}

/// `(struct T *)data`: a copy of the bio(4) argument structure (`EINVAL` when the bytes are
/// short).
fn mpi_bio_arg<T: MpiBioArg>(addr: &[u8]) -> Result<T, Errno> {
    if addr.len() < size_of::<T>() {
        return Err(Errno::EINVAL);
    }
    // SAFETY: `addr` holds at least `size_of::<T>()` initialised bytes and every bit pattern
    // is a `T` (`MpiBioArg`); the read is unaligned-safe.
    Ok(unsafe { ptr::read_unaligned(addr.as_ptr().cast::<T>()) })
}

/// Stores `bytes` at offset `off` of the ioctl's bytes (one member written back).
fn mpi_bio_put(addr: &mut [u8], off: usize, bytes: &[u8]) {
    if let Some(d) = addr.get_mut(off..off + bytes.len()) {
        d.copy_from_slice(bytes);
    }
}

/// `mpi_bio_get_pg0_raid`: IOC page 2 again, and the RAID volume page 0 of volume `id`
/// into `sc_rpg0`; `EINVAL` when any step fails.
pub fn mpi_bio_get_pg0_raid(sc: &'static MpiSoftc, id: i32) -> Result<(), Errno> {
    let mut hdr = MpiCfgHdr::default();
    let cfg_hdr = sc.sc_cfg_hdr.get();
    let vol_page = sc.sc_vol_page.get();
    if vol_page.is_null() {
        return Err(Errno::EINVAL);
    }

    // get IOC page 2
    // SAFETY: the page is `page_length * 4` bytes, allocated by mpi_attach, never freed.
    let page = unsafe {
        core::slice::from_raw_parts_mut(vol_page.cast::<u8>(), usize::from(cfg_hdr.page_length) * 4)
    };
    if mpi_req_cfg_page(sc, 0, 0, &cfg_hdr, true, page).is_err() {
        return Err(Errno::EINVAL);
    }

    // XXX return something else than EINVAL to indicate within hs range
    // SAFETY: as above.
    let (active_vols, max_physdisks) = unsafe {
        (
            i32::from((*vol_page).active_vols),
            usize::from((*vol_page).max_physdisks),
        )
    };
    if id > active_vols {
        return Err(Errno::EINVAL);
    }

    // replace current buffer with new one
    let len = size_of::<MpiCfgRaidVolPg0>() + max_physdisks * size_of::<MpiCfgRaidVolPg0Physdisk>();
    let Some(rpg0) = malloc(len, M_DEVBUF, M_WAITOK | M_CANFAIL) else {
        printf(format_args!(
            "{}: can't get memory for RAID page 0, bio disabled\n",
            devname(sc)
        ));
        return Err(Errno::EINVAL);
    };
    if let Some(old) = NonNull::new(sc.sc_rpg0.replace(rpg0.as_ptr().cast())) {
        free(old.cast(), M_DEVBUF, 0);
    }

    // get raid vol page 0
    let vol_list = sc.sc_vol_list.get();
    // SAFETY: `id` is within the volumes the page lists (checked above); plain bytes.
    let vol: MpiCfgRaidVol = unsafe { ptr::read_unaligned(vol_list.add(id as usize)) };
    let address = u32::from(vol.vol_id) | (u32::from(vol.vol_bus) << 8);
    mpi_req_cfg_header(
        sc,
        MPI_CONFIG_REQ_PAGE_TYPE_RAID_VOL,
        0,
        address,
        0,
        &mut hdr,
    )
    .map_err(|_| Errno::EINVAL)?;
    // SAFETY: `len` bytes just allocated, owned by `sc_rpg0`.
    let bytes = unsafe { core::slice::from_raw_parts_mut(rpg0.as_ptr(), len) };
    if mpi_req_cfg_page(sc, address, 0, &hdr, true, bytes).is_err() {
        return Err(Errno::EINVAL);
    }

    Ok(())
}

/// `mpi_ioctl`: the controller's bio(4) entry point.
pub fn mpi_ioctl(dev: &Device, cmd: u64, addr: &mut [u8]) -> Result<(), Errno> {
    // SAFETY: `mpi_attach` registered the controller's own device, the head of an
    // `MpiSoftc` that lives while the device is attached.
    let sc: &'static MpiSoftc = unsafe { &*ptr::from_ref(dev.softc::<MpiSoftc>()) };

    // make sure we have bio enabled
    if sc.sc_ioctl.get().is_none() {
        return Err(Errno::EINVAL);
    }

    rw_enter_write(&sc.sc_lock);

    let error = match cmd {
        BIOCINQ => mpi_bio_arg::<BiocInq>(addr).and_then(|mut bi| {
            let r = mpi_ioctl_inq(sc, &mut bi);
            mpi_bio_put(addr, offset_of!(BiocInq, bi_dev), &bi.bi_dev);
            mpi_bio_put(
                addr,
                offset_of!(BiocInq, bi_novol),
                &bi.bi_novol.to_ne_bytes(),
            );
            mpi_bio_put(
                addr,
                offset_of!(BiocInq, bi_nodisk),
                &bi.bi_nodisk.to_ne_bytes(),
            );
            r
        }),

        BIOCVOL => mpi_bio_arg::<BiocVol>(addr).and_then(|mut bv| {
            let r = mpi_ioctl_vol(sc, &mut bv);
            mpi_bio_put(
                addr,
                offset_of!(BiocVol, bv_status),
                &bv.bv_status.to_ne_bytes(),
            );
            mpi_bio_put(
                addr,
                offset_of!(BiocVol, bv_size),
                &bv.bv_size.to_ne_bytes(),
            );
            mpi_bio_put(
                addr,
                offset_of!(BiocVol, bv_level),
                &bv.bv_level.to_ne_bytes(),
            );
            mpi_bio_put(
                addr,
                offset_of!(BiocVol, bv_nodisk),
                &bv.bv_nodisk.to_ne_bytes(),
            );
            mpi_bio_put(addr, offset_of!(BiocVol, bv_vendor), &bv.bv_vendor);
            mpi_bio_put(addr, offset_of!(BiocVol, bv_dev), &bv.bv_dev);
            r
        }),

        BIOCDISK => mpi_bio_arg::<BiocDisk>(addr).and_then(|mut bd| {
            let r = mpi_ioctl_disk(sc, &mut bd);
            mpi_bio_put(
                addr,
                offset_of!(BiocDisk, bd_channel),
                &bd.bd_channel.to_ne_bytes(),
            );
            mpi_bio_put(
                addr,
                offset_of!(BiocDisk, bd_target),
                &bd.bd_target.to_ne_bytes(),
            );
            mpi_bio_put(addr, offset_of!(BiocDisk, bd_lun), &bd.bd_lun.to_ne_bytes());
            mpi_bio_put(
                addr,
                offset_of!(BiocDisk, bd_size),
                &bd.bd_size.to_ne_bytes(),
            );
            mpi_bio_put(addr, offset_of!(BiocDisk, bd_vendor), &bd.bd_vendor);
            mpi_bio_put(
                addr,
                offset_of!(BiocDisk, bd_status),
                &bd.bd_status.to_ne_bytes(),
            );
            r
        }),

        BIOCALARM => Ok(()),

        BIOCBLINK => Ok(()),

        BIOCSETSTATE => mpi_ioctl_setstate(sc),

        _ => Err(Errno::ENOTTY),
    };

    rw_exit_write(&sc.sc_lock);

    error
}

/// `mpi_ioctl_inq`: BIOCINQ.
pub fn mpi_ioctl_inq(sc: &'static MpiSoftc, bi: &mut BiocInq) -> Result<(), Errno> {
    if sc.sc_flags.get() & MPI_F_RAID == 0 {
        bi.bi_novol = 0;
        bi.bi_nodisk = 0;
    }

    let cfg_hdr = sc.sc_cfg_hdr.get();
    let vol_page = sc.sc_vol_page.get();
    if vol_page.is_null() {
        return Err(Errno::EINVAL);
    }
    // SAFETY: the page is `page_length * 4` bytes, allocated by mpi_attach, never freed.
    let page = unsafe {
        core::slice::from_raw_parts_mut(vol_page.cast::<u8>(), usize::from(cfg_hdr.page_length) * 4)
    };
    if mpi_cfg_page_bytes(sc, 0, &cfg_hdr, true, page).is_err() {
        return Err(Errno::EINVAL);
    }

    // SAFETY: as above.
    let (active_vols, active_physdisks) = unsafe {
        (
            i32::from((*vol_page).active_vols),
            i32::from((*vol_page).active_physdisks),
        )
    };
    bi.bi_novol = active_vols;
    bi.bi_nodisk = active_physdisks;
    strlcpy(&mut bi.bi_dev, devname(sc).as_bytes());

    Ok(())
}

/// The RAID volume page 0 `mpi_bio_get_pg0_raid` fetched.
fn mpi_rpg0(sc: &MpiSoftc) -> Option<MpiCfgRaidVolPg0> {
    let rpg0 = sc.sc_rpg0.get();
    if rpg0.is_null() {
        return None;
    }
    // SAFETY: `mpi_bio_get_pg0_raid` allocated at least the page header and filled it; plain
    // bytes.
    Some(unsafe { ptr::read_unaligned(rpg0) })
}

/// `mpi_ioctl_vol`: BIOCVOL.
pub fn mpi_ioctl_vol(sc: &'static MpiSoftc, bv: &mut BiocVol) -> Result<(), Errno> {
    let id = bv.bv_volid;
    mpi_bio_get_pg0_raid(sc, id)?;

    let vol_page = sc.sc_vol_page.get();
    // SAFETY: the page was fetched just above.
    if id > i32::from(unsafe { (*vol_page).active_vols }) {
        return Err(Errno::EINVAL); // XXX deal with hot spares
    }

    let Some(rpg0) = mpi_rpg0(sc) else {
        return Err(Errno::EINVAL);
    };

    // determine status
    bv.bv_status = match rpg0.volume_state {
        MPI_CFG_RAID_VOL_0_STATE_OPTIMAL => BIOC_SVONLINE,
        MPI_CFG_RAID_VOL_0_STATE_DEGRADED => BIOC_SVDEGRADED,
        MPI_CFG_RAID_VOL_0_STATE_FAILED | MPI_CFG_RAID_VOL_0_STATE_MISSING => BIOC_SVOFFLINE,
        _ => BIOC_SVINVALID,
    };

    // override status if scrubbing or something
    if rpg0.volume_status & MPI_CFG_RAID_VOL_0_STATUS_RESYNCING != 0 {
        bv.bv_status = BIOC_SVREBUILD;
    }

    bv.bv_size = u64::from(u32::from_le(rpg0.max_lba)) * 512;

    // SAFETY: `id` is within the volumes the page lists (checked above); plain bytes.
    let vol: MpiCfgRaidVol = unsafe { ptr::read_unaligned(sc.sc_vol_list.get().add(id as usize)) };
    bv.bv_level = match vol.vol_type {
        MPI_CFG_RAID_TYPE_RAID_IS => 0,
        MPI_CFG_RAID_TYPE_RAID_IME | MPI_CFG_RAID_TYPE_RAID_IM => 1,
        MPI_CFG_RAID_TYPE_RAID_5 => 5,
        MPI_CFG_RAID_TYPE_RAID_6 => 6,
        MPI_CFG_RAID_TYPE_RAID_10 => 10,
        MPI_CFG_RAID_TYPE_RAID_50 => 50,
        _ => -1,
    };

    bv.bv_nodisk = i32::from(rpg0.num_phys_disks);

    let mut vol_n = -1;
    if let Some(bus) = sc.sc_scsibus.get() {
        for i in 0..sc.sc_buswidth.get() {
            let Some(link) = scsi_get_link(bus, i, 0) else {
                continue;
            };

            // skip if not a virtual disk
            if link.flags.get() & SDEV_VIRTUAL == 0 {
                continue;
            }

            vol_n += 1;
            // are we it?
            if vol_n == bv.bv_volid {
                let inq = link.inqdata.get();
                // memcpy(bv->bv_vendor, link->inqdata.vendor, sizeof bv->bv_vendor): the
                // 32 bytes from the vendor, through the product and the revision.
                // SAFETY: `ScsiInquiryData` is `repr(C)` plain bytes with no padding.
                let raw = unsafe {
                    core::slice::from_raw_parts(
                        ptr::from_ref(&inq).cast::<u8>(),
                        size_of::<ScsiInquiryData>(),
                    )
                };
                let voff = offset_of!(ScsiInquiryData, vendor);
                let n = bv.bv_vendor.len();
                bv.bv_vendor.copy_from_slice(&raw[voff..voff + n]);
                bv.bv_vendor[n - 1] = 0;
                if let Some(dev) = link.device_softc.get() {
                    // SAFETY: the device attached on the link; softcs live while attached.
                    let dev = unsafe { dev.as_ref() };
                    strlcpy(&mut bv.bv_dev, dev.xname().as_bytes());
                }
                break;
            }
        }
    }

    Ok(())
}

/// `mpi_ioctl_disk`: BIOCDISK.
pub fn mpi_ioctl_disk(sc: &'static MpiSoftc, bd: &mut BiocDisk) -> Result<(), Errno> {
    let mut hdr = MpiCfgHdr::default();
    let mut pdpg0 = MpiCfgRaidPhysdiskPg0::default();

    let id = bd.bd_volid;
    mpi_bio_get_pg0_raid(sc, id)?;

    let vol_page = sc.sc_vol_page.get();
    // SAFETY: the page was fetched just above.
    if id > i32::from(unsafe { (*vol_page).active_vols }) {
        return Err(Errno::EINVAL); // XXX deal with hot spares
    }

    let Some(rpg0) = mpi_rpg0(sc) else {
        return Err(Errno::EINVAL);
    };

    let pdid = bd.bd_diskid;
    if pdid > i32::from(rpg0.num_phys_disks) {
        return Err(Errno::EINVAL);
    }
    // physdisk = (struct mpi_cfg_raid_vol_pg0_physdisk *)(rpg0 + 1) + pdid
    // SAFETY: the page buffer holds `max_physdisks` entries after the header; `pdid` is at
    // most `num_phys_disks`, as in the C; plain bytes.
    let physdisk: MpiCfgRaidVolPg0Physdisk = unsafe {
        ptr::read_unaligned(
            sc.sc_rpg0
                .get()
                .add(1)
                .cast::<MpiCfgRaidVolPg0Physdisk>()
                .add(pdid as usize),
        )
    };

    // get raid phys disk page 0
    let address = u32::from(physdisk.phys_disk_num);
    if mpi_cfg_header(sc, MPI_CONFIG_REQ_PAGE_TYPE_RAID_PD, 0, address, &mut hdr).is_err() {
        return Err(Errno::EINVAL);
    }
    if mpi_cfg_page(sc, address, &hdr, true, &mut pdpg0).is_err() {
        bd.bd_status = BIOC_SDFAILED;
        return Ok(());
    }
    bd.bd_channel = u16::from(pdpg0.phys_disk_bus);
    bd.bd_target = u16::from(pdpg0.phys_disk_id);
    bd.bd_lun = 0;
    bd.bd_size = u64::from(u32::from_le(pdpg0.max_lba)) * 512;
    // strlcpy(bd->bd_vendor, (char *)pdpg0.vendor_id, sizeof(bd->bd_vendor)): the C string
    // starts at vendor_id and runs on into the following members until a NUL.
    // SAFETY: `MpiCfgRaidPhysdiskPg0` is plain bytes with no padding.
    let raw = unsafe {
        core::slice::from_raw_parts(
            ptr::from_ref(&pdpg0).cast::<u8>(),
            size_of::<MpiCfgRaidPhysdiskPg0>(),
        )
    };
    strlcpy(
        &mut bd.bd_vendor,
        &raw[offset_of!(MpiCfgRaidPhysdiskPg0, vendor_id)..],
    );

    bd.bd_status = match pdpg0.phys_disk_state {
        MPI_CFG_RAID_PHYDISK_0_STATE_ONLINE => BIOC_SDONLINE,
        MPI_CFG_RAID_PHYDISK_0_STATE_MISSING | MPI_CFG_RAID_PHYDISK_0_STATE_FAILED => BIOC_SDFAILED,
        MPI_CFG_RAID_PHYDISK_0_STATE_HOSTFAIL
        | MPI_CFG_RAID_PHYDISK_0_STATE_OTHER
        | MPI_CFG_RAID_PHYDISK_0_STATE_OFFLINE => BIOC_SDOFFLINE,
        MPI_CFG_RAID_PHYDISK_0_STATE_INIT => BIOC_SDSCRUB,
        _ => BIOC_SDINVALID,
    };

    // XXX figure this out
    // bd_serial[32];
    // bd_procdev[16];

    Ok(())
}

/// `mpi_ioctl_setstate`: BIOCSETSTATE is not supported.
pub fn mpi_ioctl_setstate(_sc: &'static MpiSoftc) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

/// `mpi_create_sensors`: one drive sensor per RAID volume.
pub fn mpi_create_sensors(sc: &'static MpiSoftc) -> Result<(), Errno> {
    let Some(bus) = sc.sc_scsibus.get() else {
        return Ok(());
    };

    // count volumes
    let mut vol = 0usize;
    for i in 0..sc.sc_buswidth.get() {
        let Some(link) = scsi_get_link(bus, i, 0) else {
            continue;
        };
        // skip if not a virtual disk
        if link.flags.get() & SDEV_VIRTUAL == 0 {
            continue;
        }

        vol += 1;
    }
    if vol == 0 {
        return Ok(());
    }

    let Some(sensors) = mallocarray(vol, size_of::<Ksensor>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        return Err(Errno::ENOMEM);
    };
    let sensors = sensors.cast::<Ksensor>();
    sc.sc_sensors.set(sensors.as_ptr());
    let nsensors = vol;

    let mut xname = [0u8; 16];
    strlcpy(&mut xname, devname(sc).as_bytes());
    sc.sc_sensordev.xname.set(xname);

    let mut vol = 0usize;
    for i in 0..sc.sc_buswidth.get() {
        let Some(link) = scsi_get_link(bus, i, 0) else {
            continue;
        };
        // skip if not a virtual disk
        if link.flags.get() & SDEV_VIRTUAL == 0 {
            continue;
        }

        // SAFETY: `vol` is below `nsensors`, the length of the zeroed allocation (all-zero
        // is a valid `Ksensor`), kept until a failed registration frees it.
        let sensor: &'static Ksensor = unsafe { &*sensors.as_ptr().add(vol) };
        if let Some(dev) = link.device_softc.get() {
            // SAFETY: the device attached on the link; softcs live while attached.
            let dev = unsafe { dev.as_ref() };
            let mut desc = [0u8; 32];
            strlcpy(&mut desc, dev.xname().as_bytes());
            sensor.desc.set(desc);
        }
        sensor.r#type.set(SENSOR_DRIVE);
        sensor.status.set(SENSOR_S_UNKNOWN);
        sensor_attach(&sc.sc_sensordev, sensor);

        vol += 1;
    }

    if sensor_task_register(sc.cookie(), mpi_refresh_sensors, 10).is_none() {
        // bad:
        free(sensors.cast(), M_DEVBUF, nsensors * size_of::<Ksensor>());
        sc.sc_sensors.set(ptr::null_mut());
        return Err(Errno::ENOMEM);
    }

    sensordev_install(&sc.sc_sensordev);

    Ok(())
}

/// `mpi_refresh_sensors`: the sensor task: the state of every volume.
pub fn mpi_refresh_sensors(arg: *mut c_void) {
    // SAFETY: registered by mpi_create_sensors with the softc as the argument; softcs are
    // never freed while their sensor task runs.
    let sc: &'static MpiSoftc = unsafe { &*arg.cast::<MpiSoftc>().cast_const() };

    rw_enter_write(&sc.sc_lock);

    'done: {
        let Some(bus) = sc.sc_scsibus.get() else {
            break 'done;
        };
        let mut vol = 0usize;
        for i in 0..sc.sc_buswidth.get() {
            let Some(link) = scsi_get_link(bus, i, 0) else {
                continue;
            };
            // skip if not a virtual disk
            if link.flags.get() & SDEV_VIRTUAL == 0 {
                continue;
            }

            if mpi_bio_get_pg0_raid(sc, vol as i32).is_err() {
                continue;
            }

            let Some(rpg0) = mpi_rpg0(sc) else {
                break 'done;
            };

            // SAFETY: `vol` counts the virtual links, as many as the sensors
            // mpi_create_sensors made.
            let sensor: &Ksensor = unsafe { &*sc.sc_sensors.get().add(vol) };

            // determine status
            match rpg0.volume_state {
                MPI_CFG_RAID_VOL_0_STATE_OPTIMAL => {
                    sensor.value.set(SENSOR_DRIVE_ONLINE);
                    sensor.status.set(SENSOR_S_OK);
                }
                MPI_CFG_RAID_VOL_0_STATE_DEGRADED => {
                    sensor.value.set(SENSOR_DRIVE_PFAIL);
                    sensor.status.set(SENSOR_S_WARN);
                }
                MPI_CFG_RAID_VOL_0_STATE_FAILED | MPI_CFG_RAID_VOL_0_STATE_MISSING => {
                    sensor.value.set(SENSOR_DRIVE_FAIL);
                    sensor.status.set(SENSOR_S_CRIT);
                }
                _ => {
                    sensor.value.set(0); // unknown
                    sensor.status.set(SENSOR_S_UNKNOWN);
                }
            }

            // override status if scrubbing or something
            if rpg0.volume_status & MPI_CFG_RAID_VOL_0_STATUS_RESYNCING != 0 {
                sensor.value.set(SENSOR_DRIVE_REBUILD);
                sensor.status.set(SENSOR_S_WARN);
            }

            vol += 1;
        }
    }

    rw_exit_write(&sc.sc_lock);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_header_round_trips_through_the_reply_and_the_request() {
        let cp = MpiMsgConfigReply {
            config_header: MpiCfgHdr {
                page_version: 2,
                page_length: 5,
                page_number: 1,
                page_type: MPI_CONFIG_REQ_PAGE_TYPE_IOC,
            },
            ..Default::default()
        };
        let hdr = MpiCfgHdr::from_reply(&cp);
        assert_eq!(hdr.page_length(), 5);

        let mut cq = MpiMsgConfigRequest::default();
        hdr.to_request(&mut cq);
        assert_eq!({ cq.config_header }.page_number, 1);
        assert_eq!({ cq.config_header }.page_length, 5);
        assert!(!MpiCfgHdr::EXTENDED);
    }

    #[test]
    fn extended_header_takes_the_length_and_type_from_the_reply() {
        let cp = MpiMsgConfigReply {
            ext_page_length: 9u16.to_le(),
            ext_page_type: MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_DEVICE,
            config_header: MpiCfgHdr {
                page_version: 1,
                page_length: 0,
                page_number: 0,
                page_type: MPI_CONFIG_REQ_PAGE_TYPE_EXTENDED,
            },
            ..Default::default()
        };
        let ehdr = MpiEcfgHdr::from_reply(&cp);
        assert_eq!(ehdr.page_length(), 9);
        assert!(MpiEcfgHdr::EXTENDED);

        let mut cq = MpiMsgConfigRequest::default();
        ehdr.to_request(&mut cq);
        assert_eq!({ cq.ext_page_len }, 9u16.to_le());
        assert_eq!(cq.ext_page_type, MPI_CONFIG_REQ_EXTPAGE_TYPE_SAS_DEVICE);
        assert_eq!(
            { cq.config_header }.page_type,
            MPI_CONFIG_REQ_PAGE_TYPE_EXTENDED
        );
    }

    #[test]
    fn messages_are_whole_dwords_for_the_handshake() {
        let mut ifq = MpiMsgIocfactsRequest {
            function: MPI_FUNCTION_IOC_FACTS,
            msg_context: 0xdead_beefu32.to_le(),
            ..Default::default()
        };
        let dwords = mpi_dwords(&mut ifq);
        assert_eq!(dwords.len(), dwordsof::<MpiMsgIocfactsRequest>());
        // function is byte 3 of the first dword; the context is the third dword.
        assert_eq!(dwords[0].to_le() >> 24, u32::from(MPI_FUNCTION_IOC_FACTS));
        assert_eq!(dwords[2].to_le(), 0xdead_beef);

        let mut ifp = MpiMsgIocfactsReply::default();
        assert_eq!(mpi_dwords(&mut ifp).len(), 20);
    }

    #[test]
    fn scatter_gather_header_flags() {
        // A 4 KiB simple 64-bit element that ends the list, as mpi_load_xs writes it.
        let hdr = MPI_SGE_FL_TYPE_SIMPLE
            | MPI_SGE_FL_SIZE_64
            | MPI_SGE_FL_LAST
            | MPI_SGE_FL_EOB
            | MPI_SGE_FL_EOL
            | 4096;
        assert_eq!(hdr, 0xd300_1000);
        let mut sge = MpiSge::default();
        mpi_dvatosge(&mut sge, 0x1_2345_6000);
        assert_eq!(sge.sg_addr_lo, 0x2345_6000u32.to_le());
        assert_eq!(sge.sg_addr_hi, 1u32.to_le());
    }

    #[test]
    fn the_request_frame_holds_the_largest_message() {
        assert!(size_of::<MpiCcbBundle>() <= MPI_REQUEST_SIZE);
        assert!(size_of::<MpiInqBundle>() <= MPI_REQUEST_SIZE);
        assert!(size_of::<MpiFwuploadBundle>() <= MPI_REQUEST_SIZE);
        assert!(size_of::<MpiMsgIocinitRequest>().is_multiple_of(4));
        assert_eq!(MPI_PG_FMT, b"\x10\x02POLL\x01EXTENDED");
    }
}
/* </TESTS> */
