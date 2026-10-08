/*	$OpenBSD: nvme.c,v 1.130 2026/09/06 19:27:01 kettenis Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2014 David Gwynne <dlg@openbsd.org>
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
//! `nvme(4)`: the NVM Express controller driver, presented to the system as a SCSI adapter
//! with one target per namespace (`scsibus* at nvme?`, `sd* at scsibus?`).
//!
//! Upstream: sys/dev/ic/nvme.c @ 3ce1f3f79392
//!
//! The bus front-end (`nvme_pci`) maps the registers, establishes the interrupt and calls
//! `nvme_attach`, which resets the controller, sets up the admin queue, identifies the
//! controller, creates one I/O queue pair and attaches a scsibus whose targets are the
//! namespaces (`nvme_scsi_probe` identifies each one). `nvme_scsi_cmd` turns READ and WRITE
//! (6, 10, 12, 16) into NVM reads and writes with PRP entries (a PRP list per ccb for
//! transfers of more than two pages), SYNCHRONIZE CACHE into a flush, and answers INQUIRY
//! and READ CAPACITY (10, 16) from the identify data. Completions are reaped by
//! `nvme_q_complete`, from the interrupt or from `nvme_poll` (commands issued with
//! `SCSI_POLL`, and every admin command). The controller also registers with bio(4)
//! (`bioctl nvme0`), takes the passthrough ioctl (`NVME_PASSTHROUGH_CMD`) and reports its
//! temperature, spare and wear through the sensors framework.
//!
//! ## Deviations
//! - `HIBERNATE` is not configured: `nvme_hibernate_admin_cmd`, `nvme_hibernate_io` and the
//!   allocation of `sc_hib_q` in `nvme_attach` are not compiled, as in a kernel without that
//!   option. `SMALL_KERNEL` is not defined: the sensors are compiled; `NBIO` is 1 (bio(4)).
//! - The functions that return the C's 0/1 (`nvme_attach`, `nvme_ready`, `nvme_enable`,
//!   `nvme_disable`, `nvme_resume`, `nvme_identify`, `nvme_ccbs_alloc`, `nvme_q_create`,
//!   `nvme_q_delete`) return `Result<(), Errno>`; where the C hands back the completion
//!   flags of a failed admin command as its error (`nvme_scsi_probe`, `nvme_q_create`,
//!   `nvme_q_delete`) the error is `EIO`. `nvme_poll` still returns the flags.
//! - `nvme_q_submit` builds the entry in a local `NvmeSqe` (the fill functions write into
//!   it) and copies it into the ring with volatile writes, after zeroing the slot as the
//!   C's `memset` does; completions are read volatile, with an acquire fence for
//!   `membar_consumer`. A completion naming a command identifier past `sc_ccbs` panics
//!   (`NvmeSoftc::ccb`) where the C would index past the array.
//! - `ccb_cookie` is the enum `NvmeCookie` (`nvmevar.rs`): the command `nvme_sqe_fill`
//!   copies and `nvme_poll`'s state are held by value. `nvme_poll_done` ignores a cookie
//!   that is not its state: a completion arriving after its poll timed out finds the cookie
//!   restored and is dropped, where the C would write through that cookie as if it were the
//!   poll state.
//! - The `KASSERT(ccb != NULL)` after `scsi_io_get` (which sleeps until a ccb is free) is a
//!   `panic` where the C would dereference NULL. So is a missing namespace in
//!   `nvme_scsi_inquiry` and the capacity commands.
//! - `nvme_ccbs_alloc` fails when the PRP list memory cannot be allocated (the C goes on
//!   with a NULL area); `nvme_identify` computes the transfer limit with saturating shifts
//!   (a large `mdts` is undefined behaviour in C before the `NVME_MAXPHYS` clamp); the
//!   bio(4) functions index the 16 LBA formats and the four `rpdesc` strings with checks
//!   (the C reads past them for `nlbaf > 15` or `rp > 3`).
//! - `nvme_passthrough_cmd` copies out at most the 20 bytes of `struct nvme_pt_status`, not
//!   `pt_statuslen` bytes of the kernel stack.
//! - `nvme_q_delete` clears `sc_q` when it frees that queue (the C leaves the pointer
//!   dangling until `nvme_resume` allocates a new one), and `nvme_intr` skips a missing queue.
//! - bio(4)'s argument (`caddr_t`) is the kernel copy of the ioctl's bytes: the structure
//!   is read out of it and the members the C changes are written back.
//! - The `nvme_dumpregs` debugging aid is ported but, as in C, called from nowhere.

use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::sync::atomic::{Ordering, fence};

use libkern::{strlcat, strlcpy};

use crate::dev::bio::{bio_register, bio_status, bio_status_init};
use crate::dev::biovar::{
    BIO_MSG_INFO, BIO_MSG_LEN, BIO_STATUS_SUCCESS, BIOC_SDOFFLINE, BIOC_SDONLINE, BIOC_SDUNUSED,
    BIOC_SVINVALID, BIOC_SVOFFLINE, BIOC_SVONLINE, BIOCDISK, BIOCINQ, BIOCVOL, Bio, BioStatus,
    BiocDisk, BiocInq, BiocVol,
};
use crate::dev::ic::nvmeio::{NVME_PASSTHROUGH_CMD, NvmePtCmd, NvmePtStatus};
use crate::dev::ic::nvmereg::{
    NVM_ADMIN_ADD_IOCQ, NVM_ADMIN_ADD_IOSQ, NVM_ADMIN_DEL_IOCQ, NVM_ADMIN_DEL_IOSQ,
    NVM_ADMIN_GET_LOG_PG, NVM_ADMIN_IDENTIFY, NVM_ADMIN_SELFTEST, NVM_CMD_FLUSH, NVM_CMD_READ,
    NVM_CMD_WRITE, NVM_HEALTH_CW_SPARE, NVM_HEALTH_CW_TEMP, NVM_ID_CTRL_CTRATT_FMT,
    NVM_ID_CTRL_FNA_CRYPTOFORMAT, NVM_ID_CTRL_LPA_PE, NVM_ID_CTRL_OACS_FMT, NVM_ID_CTRL_ONCS_FMT,
    NVM_ID_CTRL_SANICAP_FMT, NVM_ID_CTRL_VWC_PRESENT, NVM_LOG_PAGE_SMART_HEALTH, NVM_SQE_CQ_IEN,
    NVM_SQE_Q_PC, NVME_ACQ, NVME_ADMIN_Q, NVME_AQA, NVME_ASQ, NVME_CAP, NVME_CC, NVME_CC_AMS_MASK,
    NVME_CC_AMS_RR, NVME_CC_CSS_MASK, NVME_CC_CSS_NVM, NVME_CC_EN, NVME_CC_IOCQES_MASK,
    NVME_CC_IOSQES_MASK, NVME_CC_MPS_MASK, NVME_CC_SHN_ABRUPT, NVME_CC_SHN_MASK, NVME_CC_SHN_NONE,
    NVME_CC_SHN_NORMAL, NVME_CQE_PHASE, NVME_CQE_SC_SUCCESS, NVME_CSTS, NVME_CSTS_CFS,
    NVME_CSTS_RDY, NVME_CSTS_SHST_DONE, NVME_CSTS_SHST_MASK, NVME_ID_NS_DPS_PIP,
    NVME_ID_NS_NSFEAT_FMT, NVME_ID_NS_NSFEAT_THIN_PROV, NVME_INTMC, NVME_INTMS, NVME_VS,
    NvmIdentifyController, NvmIdentifyNamespace, NvmSmartHealth, NvmeCqe, NvmeSqe, nvme_aqa_acqs,
    nvme_aqa_asqs, nvme_cap_ams, nvme_cap_cqr, nvme_cap_css, nvme_cap_dstrd, nvme_cap_mpsmax,
    nvme_cap_mpsmin, nvme_cap_mqes, nvme_cap_nssrs, nvme_cap_to, nvme_cc_ams, nvme_cc_ams_r,
    nvme_cc_css, nvme_cc_css_r, nvme_cc_iocqes, nvme_cc_iocqes_r, nvme_cc_iosqes, nvme_cc_iosqes_r,
    nvme_cc_mps, nvme_cc_mps_r, nvme_cc_shn, nvme_cc_shn_r, nvme_cqe_sc, nvme_cqhdbl,
    nvme_id_ns_dps_type, nvme_id_ns_flbas, nvme_sqtdbl, nvme_vs_mjr, nvme_vs_mnr,
};
use crate::dev::ic::nvmevar::{
    NVME_IO_Q, NVME_MAXPHYS, NvmeCcb, NvmeCcbList, NvmeCookie, NvmeDmamem, NvmeFillFn,
    NvmeNamespace, NvmeOps, NvmeQueue, NvmeSoftc, nvme_barrier, nvme_dma_dva, nvme_dma_kva,
    nvme_dma_len, nvme_dma_map, nvme_read4, nvme_write4,
};
use crate::kern::kern_lock::{mtx_enter, mtx_enter_try, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_sensors::{sensor_attach, sensor_task_register, sensordev_install};
use crate::kern::subr_autoconf::{config_activate_children, config_found};
use crate::kern::subr_prf::{Bitmask, Str, panic, printf, snprintf};
use crate::machine::bus::{
    BUS_DMA_64BIT, BUS_DMA_ALLOCNOW, BUS_DMA_NOWAIT, BUS_DMA_WAITOK, BUS_DMA_ZERO,
    BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE,
    BUS_SPACE_BARRIER_READ, BUS_SPACE_BARRIER_WRITE, BusDmaSegment, BusSize, bus_dmamap_create,
    bus_dmamap_destroy, bus_dmamap_load, bus_dmamap_sync, bus_dmamap_unload, bus_dmamem_alloc,
    bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap,
};
use crate::machine::copy::copyout;
use crate::machine::cpu::delay;
use crate::machine::intr::IPL_BIO;
use crate::scsi::scsi_all::{
    INQUIRY, PREVENT_ALLOW, READ_CAP_16_TPE, SCSI_OK, SI_EVPD, SID_CmdQue, SID_SCSI2_ALEN,
    SID_SCSI2_RESPONSE, START_STOP, ScsiInquiry, ScsiInquiryData, ScsiReadCapData,
    ScsiReadCapData16, ScsiWire, T_DIRECT, TEST_UNIT_READY,
};
use crate::scsi::scsi_base::{
    scsi_cmd_rw_decode, scsi_copy_internal_data, scsi_done, scsi_io_get, scsi_io_put,
    scsi_iopool_init,
};
use crate::scsi::scsi_disk::{
    READ_10, READ_12, READ_16, READ_CAPACITY, READ_CAPACITY_16, READ_COMMAND, SYNCHRONIZE_CACHE,
    ScsiReadCapacity, ScsiReadCapacity16, WRITE_10, WRITE_12, WRITE_16, WRITE_COMMAND,
};
use crate::scsi::scsiconf::{
    _lto2b, _lto4b, _lto8b, SCSI_DATA_IN, SCSI_DATA_OUT, SCSI_NOSLEEP, SCSI_POLL, SCSI_REV_SPC4,
    SDEV_S_DYING, ScsiAdapter, ScsiIo, ScsiLink, ScsiXfer, ScsibusAttachArgs, ScsibusSoftc,
    XS_DRIVER_STUFFUP, XS_NOERROR, scsi_get_link, scsi_strvis, scsiprint,
};
use crate::scsi::sdvar::{SDF_DYING, SdSoftc};
use crate::sys::buf::Buf;
use crate::sys::device::{Cfdriver, DV_DULL, DVACT_POWERDOWN, DVACT_RESUME, Device};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_CANFAIL, M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::mutex::Mutex;
use crate::sys::param::{MAXPHYS, PAGE_SHIFT};
use crate::sys::queue::SimpleqHead;
use crate::sys::sensors::{
    SENSOR_PERCENT, SENSOR_S_CRIT, SENSOR_S_OK, SENSOR_S_UNKNOWN, SENSOR_TEMP,
};

/// `NVME_TIMO_QOP`: ms to create/delete queue.
pub const NVME_TIMO_QOP: u32 = 5000;
/// `NVME_TIMO_PT`: ms to complete passthrough.
pub const NVME_TIMO_PT: u32 = 5000;
/// `NVME_TIMO_IDENT`: ms to probe/identify.
pub const NVME_TIMO_IDENT: u32 = 10000;
/// `NVME_TIMO_LOG_PAGE`: ms to read log pages.
pub const NVME_TIMO_LOG_PAGE: u32 = 5000;
/// `NVME_TIMO_DELAYNS`: ns to delay() in poll loop (the C's name; `delay` takes
/// microseconds).
pub const NVME_TIMO_DELAYNS: u32 = 10;

/// `struct nvme_poll_state`: the command `nvme_poll` submits and the flags of its
/// completion (`NVME_CQE_PHASE` set once it is done).
#[derive(Clone, Copy, Debug)]
pub struct NvmePollState {
    /// `s`.
    pub s: NvmeSqe,
    /// `c`.
    pub c: NvmeCqe,
}

/// How far `nvme_attach` got before failing: what its error path undoes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NvmeAttachUnwind {
    /// `free_admin_q`.
    FreeAdminQ,
    /// `free_ccbs`.
    FreeCcbs,
    /// `disable`.
    Disable,
    /// `free_q`.
    FreeQ,
}

/// `nvme_cd`.
pub static NVME_CD: Cfdriver = Cfdriver::new(b"nvme", DV_DULL, 0);

/// `nvme_switch`: the adapter's entry points.
pub static NVME_SWITCH: ScsiAdapter = ScsiAdapter {
    scsi_cmd: nvme_scsi_cmd,
    dev_minphys: Some(nvme_minphys),
    dev_probe: Some(nvme_scsi_probe),
    dev_free: Some(nvme_scsi_free),
    ioctl: Some(nvme_scsi_ioctl),
};

/// `nvme_ops`: the operations of a standard controller.
pub static NVME_OPS: NvmeOps = NvmeOps {
    op_enable: None,
    op_q_alloc: None,
    op_q_free: None,
    op_sq_enter: nvme_op_sq_enter,
    op_sq_leave: nvme_op_sq_leave,
    op_sq_enter_locked: nvme_op_sq_enter_locked,
    op_sq_leave_locked: nvme_op_sq_leave_locked,

    op_cq_done: nvme_op_cq_done,
};

/// A bio(4) argument structure, read out of the ioctl's bytes.
///
/// # Safety
///
/// The implementor is `#[repr(C)]` of integers, byte arrays, raw pointers and [`Bio`], so
/// every bit pattern is a valid value.
unsafe trait NvmeBioArg: Copy {}

// SAFETY: a `Bio` (a raw pointer and integers), integers and byte arrays.
unsafe impl NvmeBioArg for BiocInq {}
// SAFETY: as above.
unsafe impl NvmeBioArg for BiocVol {}
// SAFETY: as above.
unsafe impl NvmeBioArg for BiocDisk {}
// SAFETY: as above, and user addresses (`usize`).
unsafe impl NvmeBioArg for NvmePtCmd {}

/// `link->bus->sb_adapter_softc`: the controller a link is on.
fn nvme_link_softc(link: &ScsiLink) -> &'static NvmeSoftc {
    let p = link.bus().sb_adapter_softc.get();
    if p.is_null() {
        panic(format_args!("nvme: bus without an adapter softc"));
    }
    // SAFETY: `nvme_attach` attaches its scsibus with its own softc as `saa_adapter_softc`,
    // and only that bus's links reach `nvme_switch`; softcs are never freed.
    unsafe { &*p.cast::<NvmeSoftc>().cast_const() }
}

/// The ccb behind an opening of `sc_iopool`.
///
/// # Safety
///
/// `io` came from `nvme_ccb_get` (directly or through `scsi_io_get` on `sc_iopool`) and
/// its ccbs have not been freed.
unsafe fn nvme_io_ccb(io: ScsiIo) -> &'static NvmeCcb {
    // SAFETY: the caller's guarantee: a ccb of `sc_ccbs`, which stays until
    // `nvme_ccbs_free`.
    unsafe { io.cast::<NvmeCcb>().as_ref() }
}

/// `xs->io`: the ccb the midlayer took for a transfer.
fn nvme_xs_ccb(xs: &ScsiXfer) -> &'static NvmeCcb {
    let Some(io) = xs.io.get() else {
        panic(format_args!("nvme: xs {:p} without a ccb", xs));
    };
    // SAFETY: the transfer's opening came from this adapter's pool (`saa_pool`), whose
    // `io_get` is `nvme_ccb_get`.
    unsafe { nvme_io_ccb(io) }
}

/// The softc as the `void *` cookie of the iopool and the interrupt handler.
fn nvme_cookie(sc: &NvmeSoftc) -> *mut c_void {
    ptr::from_ref(sc).cast_mut().cast()
}

/// `scsi_io_get(&sc->sc_iopool, 0)` and the C's `KASSERT(ccb != NULL)`.
fn nvme_scsi_io_get(sc: &'static NvmeSoftc) -> (ScsiIo, &'static NvmeCcb) {
    let Some(io) = scsi_io_get(&sc.sc_iopool, 0) else {
        panic(format_args!(
            "{}: scsi_io_get without a ccb",
            sc.sc_dev.xname()
        ));
    };
    // SAFETY: an opening of `sc_iopool`, whose `io_get` is `nvme_ccb_get`.
    (io, unsafe { nvme_io_ccb(io) })
}

/// `nvme_read8`.
///
/// Some controllers, at least Apple NVMe, always require split transfers, so don't use
/// `bus_space_{read,write}_8()` on LP64.
pub fn nvme_read8(sc: &NvmeSoftc, r: BusSize) -> u64 {
    u64::from(nvme_read4(sc, r)) | (u64::from(nvme_read4(sc, r + 4)) << 32)
}

/// `nvme_write8`.
pub fn nvme_write8(sc: &NvmeSoftc, r: BusSize, v: u64) {
    nvme_write4(sc, r, v as u32);
    nvme_write4(sc, r + 4, (v >> 32) as u32);
}

/// `nvme_dumpregs`: prints the controller's registers.
pub fn nvme_dumpregs(sc: &NvmeSoftc) {
    let n = sc.sc_dev.xname();

    let r8 = nvme_read8(sc, NVME_CAP);
    printf(format_args!(
        "{n}: cap  0x{:016x}\n",
        nvme_read8(sc, NVME_CAP)
    ));
    printf(format_args!(
        "{n}:  mpsmax {} ({})\n",
        nvme_cap_mpsmax(r8),
        1u64 << nvme_cap_mpsmax(r8)
    ));
    printf(format_args!(
        "{n}:  mpsmin {} ({})\n",
        nvme_cap_mpsmin(r8),
        1u64 << nvme_cap_mpsmin(r8)
    ));
    printf(format_args!("{n}:  css {}\n", nvme_cap_css(r8)));
    printf(format_args!(
        "{n}:  nssrs {}\n",
        u32::from(nvme_cap_nssrs(r8))
    ));
    printf(format_args!("{n}:  dstrd {}\n", nvme_cap_dstrd(r8)));
    printf(format_args!("{n}:  to {} msec\n", nvme_cap_to(r8)));
    printf(format_args!("{n}:  ams {}\n", nvme_cap_ams(r8)));
    printf(format_args!("{n}:  cqr {}\n", u32::from(nvme_cap_cqr(r8))));
    printf(format_args!("{n}:  mqes {}\n", nvme_cap_mqes(r8)));

    printf(format_args!(
        "{n}: vs   0x{:04x}\n",
        nvme_read4(sc, NVME_VS)
    ));

    let r4 = nvme_read4(sc, NVME_CC);
    printf(format_args!("{n}: cc   0x{r4:04x}\n"));
    printf(format_args!("{n}:  iocqes {}\n", nvme_cc_iocqes_r(r4)));
    printf(format_args!("{n}:  iosqes {}\n", nvme_cc_iosqes_r(r4)));
    printf(format_args!("{n}:  shn {}\n", nvme_cc_shn_r(r4)));
    printf(format_args!("{n}:  ams {}\n", nvme_cc_ams_r(r4)));
    printf(format_args!("{n}:  mps {}\n", nvme_cc_mps_r(r4)));
    printf(format_args!("{n}:  css {}\n", nvme_cc_css_r(r4)));
    printf(format_args!("{n}:  en {}\n", r4 & NVME_CC_EN));

    printf(format_args!(
        "{n}: csts 0x{:08x}\n",
        nvme_read4(sc, NVME_CSTS)
    ));
    printf(format_args!(
        "{n}: aqa  0x{:08x}\n",
        nvme_read4(sc, NVME_AQA)
    ));
    printf(format_args!(
        "{n}: asq  0x{:016x}\n",
        nvme_read8(sc, NVME_ASQ)
    ));
    printf(format_args!(
        "{n}: acq  0x{:016x}\n",
        nvme_read8(sc, NVME_ACQ)
    ));
}

/// `nvme_ready`: waits up to `sc_rdy_to` milliseconds for `CSTS.RDY` to equal `rdy`.
pub fn nvme_ready(sc: &NvmeSoftc, rdy: u32) -> Result<(), Errno> {
    let mut i = 0u32;

    while nvme_read4(sc, NVME_CSTS) & NVME_CSTS_RDY != rdy {
        let over = i > sc.sc_rdy_to.get();
        i = i.wrapping_add(1);
        if over {
            return Err(Errno::ETIMEDOUT);
        }

        delay(1000);
        nvme_barrier(sc, NVME_CSTS, 4, BUS_SPACE_BARRIER_READ);
    }

    Ok(())
}

/// `nvme_enable`: programs the admin queue and the configuration and enables the
/// controller.
pub fn nvme_enable(sc: &NvmeSoftc) -> Result<(), Errno> {
    let mut cc = nvme_read4(sc, NVME_CC);
    if cc & NVME_CC_EN != 0 {
        return nvme_ready(sc, NVME_CSTS_RDY);
    }

    if let Some(op_enable) = sc.ops().op_enable {
        op_enable(sc);
    }

    let aq = sc.admin_q();
    nvme_write4(
        sc,
        NVME_AQA,
        nvme_aqa_acqs(aq.q_entries) | nvme_aqa_asqs(aq.q_entries),
    );
    nvme_barrier(sc, 0, sc.sc_ios.get(), BUS_SPACE_BARRIER_WRITE);

    nvme_write8(sc, NVME_ASQ, nvme_dma_dva(aq.q_sq_dmamem));
    nvme_barrier(sc, 0, sc.sc_ios.get(), BUS_SPACE_BARRIER_WRITE);
    nvme_write8(sc, NVME_ACQ, nvme_dma_dva(aq.q_cq_dmamem));
    nvme_barrier(sc, 0, sc.sc_ios.get(), BUS_SPACE_BARRIER_WRITE);

    cc &= !(NVME_CC_IOCQES_MASK
        | NVME_CC_IOSQES_MASK
        | NVME_CC_SHN_MASK
        | NVME_CC_AMS_MASK
        | NVME_CC_MPS_MASK
        | NVME_CC_CSS_MASK);
    cc |= nvme_cc_iosqes(6); // Submission queue size == 2**6 (64)
    cc |= nvme_cc_iocqes(4); // Completion queue size == 2**4 (16)
    cc |= nvme_cc_shn(NVME_CC_SHN_NONE);
    cc |= nvme_cc_css(NVME_CC_CSS_NVM);
    cc |= nvme_cc_ams(NVME_CC_AMS_RR);
    // ffs(sc_mps) - 1: the page size's shift.
    cc |= nvme_cc_mps(sc.sc_mps.get().trailing_zeros());
    cc |= NVME_CC_EN;

    nvme_write4(sc, NVME_CC, cc);
    nvme_barrier(
        sc,
        0,
        sc.sc_ios.get(),
        BUS_SPACE_BARRIER_READ | BUS_SPACE_BARRIER_WRITE,
    );

    nvme_ready(sc, NVME_CSTS_RDY)
}

/// `nvme_disable`: disables the controller, waiting for a running one to be ready first.
pub fn nvme_disable(sc: &NvmeSoftc) -> Result<(), Errno> {
    let mut cc = nvme_read4(sc, NVME_CC);
    if cc & NVME_CC_EN != 0 {
        let csts = nvme_read4(sc, NVME_CSTS);
        if csts & NVME_CSTS_CFS == 0 && nvme_ready(sc, NVME_CSTS_RDY).is_err() {
            return Err(Errno::ETIMEDOUT);
        }
    }

    cc &= !NVME_CC_EN;

    nvme_write4(sc, NVME_CC, cc);
    nvme_barrier(
        sc,
        0,
        sc.sc_ios.get(),
        BUS_SPACE_BARRIER_READ | BUS_SPACE_BARRIER_WRITE,
    );

    nvme_ready(sc, 0)
}

/// `nvme_attach`: brings the controller up and attaches its scsibus. The attach line's
/// tail (`NVMe 1.4`) is printed here.
pub fn nvme_attach(sc: &'static NvmeSoftc) -> Result<(), Errno> {
    let mut nccbs = 0u32;

    mtx_init(&sc.sc_ccb_mtx, IPL_BIO);
    rw_init(&sc.sc_lock, "nvme_lock");
    sc.sc_ccb_list.init();
    // SAFETY: nvme_ccb_get and nvme_ccb_put take this softc as their cookie, and the softc
    // is never freed.
    unsafe { scsi_iopool_init(&sc.sc_iopool, nvme_cookie(sc), nvme_ccb_get, nvme_ccb_put) };
    if sc.sc_ops.get().is_none() {
        sc.sc_ops.set(Some(&NVME_OPS));
    }
    if sc.sc_openings.get() == 0 {
        sc.sc_openings.set(64);
    }

    let reg = nvme_read4(sc, NVME_VS);
    if reg == 0xffff_ffff {
        printf(format_args!("invalid mapping\n"));
        return Err(Errno::ENXIO);
    }

    printf(format_args!(
        "NVMe {}.{}\n",
        nvme_vs_mjr(reg),
        nvme_vs_mnr(reg)
    ));

    let cap = nvme_read8(sc, NVME_CAP);
    sc.sc_dstrd.set(nvme_cap_dstrd(cap));
    if nvme_cap_mpsmin(cap) > PAGE_SHIFT as u32 {
        printf(format_args!(
            "{}: NVMe minimum page size {} is greater than CPU page size {}\n",
            sc.sc_dev.xname(),
            1u64 << nvme_cap_mpsmin(cap),
            1u64 << PAGE_SHIFT
        ));
        return Err(Errno::ENXIO);
    }
    if nvme_cap_mpsmax(cap) < PAGE_SHIFT as u32 {
        sc.sc_mps.set(1 << nvme_cap_mpsmax(cap));
    } else {
        sc.sc_mps.set(1 << PAGE_SHIFT);
    }

    sc.sc_rdy_to.set(nvme_cap_to(cap) as u32);
    sc.sc_mdts.set(MAXPHYS);
    sc.sc_max_prpl
        .set((sc.sc_mdts.get() / sc.sc_mps.get()) as u32);

    if nvme_disable(sc).is_err() {
        printf(format_args!(
            "{}: unable to disable controller\n",
            sc.sc_dev.xname()
        ));
        return Err(Errno::EIO);
    }

    let Some(admin_q) = nvme_q_alloc(sc, NVME_ADMIN_Q, 128, sc.sc_dstrd.get()) else {
        printf(format_args!(
            "{}: unable to allocate admin queue\n",
            sc.sc_dev.xname()
        ));
        return Err(Errno::ENOMEM);
    };
    sc.sc_admin_q.set(Some(admin_q));

    let unwind = 'attach: {
        if nvme_ccbs_alloc(sc, 16).is_err() {
            printf(format_args!(
                "{}: unable to allocate initial ccbs\n",
                sc.sc_dev.xname()
            ));
            break 'attach NvmeAttachUnwind::FreeAdminQ;
        }
        nccbs = 16;

        if nvme_enable(sc).is_err() {
            printf(format_args!(
                "{}: unable to enable controller\n",
                sc.sc_dev.xname()
            ));
            break 'attach NvmeAttachUnwind::FreeCcbs;
        }

        if nvme_identify(sc, nvme_cap_mpsmin(cap)).is_err() {
            printf(format_args!(
                "{}: unable to identify controller\n",
                sc.sc_dev.xname()
            ));
            break 'attach NvmeAttachUnwind::Disable;
        }

        // We now know the real values of sc_mdts and sc_max_prpl.
        nvme_ccbs_free(sc, nccbs);
        if nvme_ccbs_alloc(sc, 64).is_err() {
            printf(format_args!(
                "{}: unable to allocate ccbs\n",
                sc.sc_dev.xname()
            ));
            break 'attach NvmeAttachUnwind::FreeAdminQ;
        }
        nccbs = 64;

        let Some(q) = nvme_q_alloc(sc, NVME_IO_Q, 128, sc.sc_dstrd.get()) else {
            printf(format_args!(
                "{}: unable to allocate io q\n",
                sc.sc_dev.xname()
            ));
            break 'attach NvmeAttachUnwind::Disable;
        };
        sc.sc_q.set(Some(q));

        if nvme_q_create(sc, q).is_err() {
            printf(format_args!(
                "{}: unable to create io q\n",
                sc.sc_dev.xname()
            ));
            break 'attach NvmeAttachUnwind::FreeQ;
        }

        // HIBERNATE: not configured (sc_hib_q, NVME_HIB_Q with 4 entries).

        nvme_write4(sc, NVME_INTMC, 1);

        let nn = sc.sc_nn.get() as usize;
        let Some(ns) = mallocarray(
            nn + 1,
            size_of::<NvmeNamespace>(),
            M_DEVBUF,
            M_WAITOK | M_ZERO,
        ) else {
            panic(format_args!("nvme_attach: mallocarray(M_WAITOK) failed"));
        };
        // Zeroed: every entry's `ident` is `None`.
        sc.sc_namespaces.set(ns.as_ptr().cast());

        let mut saa = ScsibusAttachArgs::new();
        saa.saa_adapter = Some(&NVME_SWITCH);
        saa.saa_adapter_softc = nvme_cookie(sc);
        saa.saa_adapter_buswidth = (nn + 1) as u16;
        saa.saa_luns = 1;
        saa.saa_adapter_target = 0;
        saa.saa_openings = sc.sc_openings.get() as u16;
        saa.saa_pool = Some(&sc.sc_iopool);
        saa.saa_quirks = 0;
        saa.saa_flags = 0;
        saa.saa_wwpn = 0;
        saa.saa_wwnn = 0;

        let mut xname = [0u8; 16];
        strlcpy(&mut xname, sc.sc_dev.xname().as_bytes());
        sc.sc_sensordev.xname.set(xname);

        // !SMALL_KERNEL
        sc.sc_temp_sensor.r#type.set(SENSOR_TEMP);
        sc.sc_temp_sensor.status.set(SENSOR_S_UNKNOWN);
        sensor_attach(&sc.sc_sensordev, &sc.sc_temp_sensor);

        sc.sc_usage_sensor.r#type.set(SENSOR_PERCENT);
        sc.sc_usage_sensor.status.set(SENSOR_S_UNKNOWN);
        let mut desc = [0u8; 32];
        strlcpy(&mut desc, b"endurance used");
        sc.sc_usage_sensor.desc.set(desc);
        sensor_attach(&sc.sc_sensordev, &sc.sc_usage_sensor);

        sc.sc_spare_sensor.r#type.set(SENSOR_PERCENT);
        sc.sc_spare_sensor.status.set(SENSOR_S_UNKNOWN);
        let mut desc = [0u8; 32];
        strlcpy(&mut desc, b"available spare");
        sc.sc_spare_sensor.desc.set(desc);
        sensor_attach(&sc.sc_sensordev, &sc.sc_spare_sensor);

        if sensor_task_register(nvme_cookie(sc), nvme_refresh_sensors, 60).is_none() {
            break 'attach NvmeAttachUnwind::FreeQ;
        }

        sensordev_install(&sc.sc_sensordev);

        let bus = config_found(&sc.sc_dev, ptr::from_mut(&mut saa).cast(), Some(scsiprint));
        // SAFETY: what attaches at nvme is a scsibus (`scsibus* at nvme?`), whose softc
        // begins with its device; softcs are never freed.
        sc.sc_scsibus
            .set(bus.map(|d| unsafe { &*d.as_ptr().cast::<ScsibusSoftc>().cast_const() }));
        // NBIO > 0
        if bio_register(&sc.sc_dev, nvme_bioctl).is_err() {
            printf(format_args!(
                "{}: unable to register bioctl\n",
                sc.sc_dev.xname()
            ));
        }

        return Ok(());
    };

    if unwind >= NvmeAttachUnwind::FreeQ
        && let Some(q) = sc.sc_q.take()
    {
        // SAFETY: the queue allocated above, which nothing uses any more.
        unsafe { nvme_q_free(sc, q) };
    }
    if unwind >= NvmeAttachUnwind::Disable {
        let _ = nvme_disable(sc);
    }
    if unwind >= NvmeAttachUnwind::FreeCcbs {
        nvme_ccbs_free(sc, nccbs);
    }
    // free_admin_q:
    if let Some(q) = sc.sc_admin_q.take() {
        // SAFETY: the admin queue allocated above, which nothing uses any more.
        unsafe { nvme_q_free(sc, q) };
    }

    Err(Errno::EIO)
}

/// `nvme_resume`: brings the controller back after a suspend.
pub fn nvme_resume(sc: &'static NvmeSoftc) -> Result<(), Errno> {
    if nvme_disable(sc).is_err() {
        printf(format_args!(
            "{}: unable to disable controller\n",
            sc.sc_dev.xname()
        ));
        return Err(Errno::EIO);
    }

    if nvme_q_reset(sc, sc.admin_q()).is_err() {
        printf(format_args!(
            "{}: unable to reset admin queue\n",
            sc.sc_dev.xname()
        ));
        return Err(Errno::EIO);
    }

    if nvme_enable(sc).is_err() {
        printf(format_args!(
            "{}: unable to enable controller\n",
            sc.sc_dev.xname()
        ));
        return Err(Errno::EIO);
    }

    let failed_q = 'resume: {
        let Some(q) = nvme_q_alloc(sc, NVME_IO_Q, 128, sc.sc_dstrd.get()) else {
            printf(format_args!(
                "{}: unable to allocate io q\n",
                sc.sc_dev.xname()
            ));
            break 'resume None;
        };
        sc.sc_q.set(Some(q));

        if nvme_q_create(sc, q).is_err() {
            printf(format_args!(
                "{}: unable to create io q\n",
                sc.sc_dev.xname()
            ));
            break 'resume Some(q);
        }

        nvme_write4(sc, NVME_INTMC, 1);

        return Ok(());
    };

    // free_q:
    if let Some(q) = failed_q {
        sc.sc_q.set(None);
        // SAFETY: the queue allocated above, never created on the controller.
        unsafe { nvme_q_free(sc, q) };
    }
    // disable:
    let _ = nvme_disable(sc);

    Err(Errno::EIO)
}

/// `nvme_scsi_probe`: the adapter's `dev_probe`: identifies the namespace a target
/// stands for, and keeps its identify data when it has a size.
pub fn nvme_scsi_probe(link: &'static ScsiLink) -> Result<(), Errno> {
    let sc = nvme_link_softc(link);
    let target = link.target.get();

    let (io, ccb) = nvme_scsi_io_get(sc);

    let Some(mem) = nvme_dmamem_alloc(sc, size_of::<NvmIdentifyNamespace>()) else {
        return Err(Errno::ENOMEM);
    };

    let mut sqe = NvmeSqe::zeroed();
    sqe.opcode = NVM_ADMIN_IDENTIFY;
    sqe.nsid = u32::from(target).to_le();
    sqe.entry.set_prp(0, nvme_dma_dva(mem));
    sqe.cdw10 = 0u32.to_le();

    ccb.ccb_done.set(nvme_empty_done);
    ccb.ccb_cookie.set(NvmeCookie::Sqe(sqe));

    nvme_dmamem_sync(sc, mem, BUS_DMASYNC_PREREAD);
    let flags = nvme_poll(sc, sc.admin_q(), ccb, nvme_sqe_fill, NVME_TIMO_IDENT);
    nvme_dmamem_sync(sc, mem, BUS_DMASYNC_POSTREAD);

    scsi_io_put(&sc.sc_iopool, io);

    let mut rv = if flags == 0 { Ok(()) } else { Err(Errno::EIO) };
    if rv.is_ok() {
        // SAFETY: the DMA area holds the identify data the controller wrote (synced
        // above), as large as `NvmIdentifyNamespace`, all of whose bit patterns are valid.
        let identify = unsafe { &*nvme_dma_kva(mem).cast::<NvmIdentifyNamespace>() };
        if nvme_scsi_size(identify) > 0 {
            // Commit namespace if it has a size greater than zero.
            let Some(copy) = malloc(size_of::<NvmIdentifyNamespace>(), M_DEVBUF, M_WAITOK) else {
                panic(format_args!("nvme_scsi_probe: malloc(M_WAITOK) failed"));
            };
            let copy = copy.cast::<NvmIdentifyNamespace>();
            // SAFETY: a fresh allocation of the structure's size (malloc aligns it); the
            // source is the DMA area above; they do not overlap.
            unsafe { ptr::copy_nonoverlapping(ptr::from_ref(identify), copy.as_ptr(), 1) };
            match sc.namespace(usize::from(target)) {
                Some(ns) => ns.ident.set(Some(copy)),
                None => panic(format_args!(
                    "{}: target {target} past the namespaces",
                    sc.sc_dev.xname()
                )),
            }
        } else {
            // Don't attach a namespace if its size is zero.
            rv = Err(Errno::ENXIO);
        }
    }

    // SAFETY: allocated above, no longer used by the controller or by anyone else.
    unsafe { nvme_dmamem_free(sc, mem) };

    rv
}

/// `nvme_shutdown`: deletes the I/O queue and shuts the controller down (or disables it).
pub fn nvme_shutdown(sc: &'static NvmeSoftc) -> Result<(), Errno> {
    nvme_write4(sc, NVME_INTMC, 0);

    if nvme_q_delete(sc, sc.io_q()).is_err() {
        printf(format_args!(
            "{}: unable to delete q, disabling\n",
            sc.sc_dev.xname()
        ));
        let _ = nvme_disable(sc);
        return Ok(());
    }

    let mut cc = nvme_read4(sc, NVME_CC);
    cc &= !NVME_CC_SHN_MASK;
    cc |= nvme_cc_shn(NVME_CC_SHN_NORMAL);
    nvme_write4(sc, NVME_CC, cc);

    for _ in 0..4000 {
        nvme_barrier(
            sc,
            0,
            sc.sc_ios.get(),
            BUS_SPACE_BARRIER_READ | BUS_SPACE_BARRIER_WRITE,
        );
        let csts = nvme_read4(sc, NVME_CSTS);
        if csts & NVME_CSTS_SHST_MASK == NVME_CSTS_SHST_DONE {
            return Ok(());
        }

        delay(1000);
    }

    printf(format_args!(
        "{}: unable to shutdown, disabling\n",
        sc.sc_dev.xname()
    ));

    // disable:
    let _ = nvme_disable(sc);
    Ok(())
}

/// `nvme_activate`: suspend, resume and power-down.
pub fn nvme_activate(sc: &'static NvmeSoftc, act: i32) -> Result<(), Errno> {
    match act {
        DVACT_POWERDOWN => {
            let rv = config_activate_children(&sc.sc_dev, act);
            let _ = nvme_shutdown(sc);
            rv
        }
        DVACT_RESUME => {
            nvme_resume(sc)?;
            config_activate_children(&sc.sc_dev, act)
        }
        _ => config_activate_children(&sc.sc_dev, act),
    }
}

/// `nvme_scsi_cmd`: the adapter's `scsi_cmd`.
pub fn nvme_scsi_cmd(xs: &'static ScsiXfer) {
    match xs.cmd.get().opcode {
        READ_COMMAND | READ_10 | READ_12 | READ_16 => {
            nvme_scsi_io(xs, SCSI_DATA_IN);
            return;
        }
        WRITE_COMMAND | WRITE_10 | WRITE_12 | WRITE_16 => {
            nvme_scsi_io(xs, SCSI_DATA_OUT);
            return;
        }

        SYNCHRONIZE_CACHE => {
            nvme_scsi_sync(xs);
            return;
        }

        INQUIRY => {
            nvme_scsi_inq(xs);
            return;
        }
        READ_CAPACITY_16 => {
            nvme_scsi_capacity16(xs);
            return;
        }
        READ_CAPACITY => {
            nvme_scsi_capacity(xs);
            return;
        }

        TEST_UNIT_READY | PREVENT_ALLOW | START_STOP => {
            xs.error.set(XS_NOERROR);
            scsi_done(xs);
            return;
        }

        _ => {}
    }

    xs.error.set(XS_DRIVER_STUFFUP);
    scsi_done(xs);
}

/// `nvme_minphys`: the adapter's `dev_minphys`: at most `sc_mdts` bytes per transfer.
pub fn nvme_minphys(bp: &Buf, link: &'static ScsiLink) {
    let sc = nvme_link_softc(link);

    let mdts = sc.sc_mdts.get() as i64;
    if bp.b_bcount.get() > mdts {
        bp.b_bcount.set(mdts);
    }
}

/// `nvme_scsi_io`: a read or a write (`dir` is `SCSI_DATA_IN` or `SCSI_DATA_OUT`).
pub fn nvme_scsi_io(xs: &'static ScsiXfer, dir: i32) {
    let link = xs.link();
    let sc = nvme_link_softc(link);
    let ccb = nvme_xs_ccb(xs);
    let dmap = ccb.ccb_dmamap;
    let dmat = sc.dmat();

    'stuffup: {
        if xs.flags.get() & (SCSI_DATA_IN | SCSI_DATA_OUT) != dir {
            break 'stuffup;
        }

        ccb.ccb_done.set(nvme_scsi_io_done);
        ccb.ccb_cookie.set(NvmeCookie::Xs(xs));

        let flags = if xs.flags.get() & SCSI_NOSLEEP != 0 {
            BUS_DMA_NOWAIT
        } else {
            BUS_DMA_WAITOK
        };
        // SAFETY: `set_data`'s contract: `xs.data()` is valid for `datalen` bytes and
        // reserved for this transfer until it completes, which unloads the map
        // (nvme_scsi_io_done).
        let loaded = unsafe {
            bus_dmamap_load(
                dmat,
                dmap,
                xs.data(),
                xs.datalen().max(0) as usize,
                None,
                flags,
            )
        };
        if loaded.is_err() {
            break 'stuffup;
        }

        bus_dmamap_sync(
            dmat,
            dmap,
            0,
            dmap.dm_mapsize.get(),
            if xs.flags.get() & SCSI_DATA_IN != 0 {
                BUS_DMASYNC_PREREAD
            } else {
                BUS_DMASYNC_PREWRITE
            },
        );

        let nsegs = dmap.dm_nsegs.get().max(0) as usize;
        if nsegs > 2 {
            for (i, seg) in dmap.dm_segs().iter().enumerate().take(nsegs).skip(1) {
                // SAFETY: the ccb's PRP list has `sc_max_prpl` entries and the map at most
                // `sc_max_prpl + 1` segments; the list is DMA memory the controller reads,
                // hence volatile.
                unsafe {
                    ptr::write_volatile(ccb.ccb_prpl.add(i - 1), (seg.get().ds_addr as u64).to_le())
                };
            }
            if let Some(prpls) = sc.sc_ccb_prpls.get() {
                bus_dmamap_sync(
                    dmat,
                    nvme_dma_map(prpls),
                    ccb.ccb_prpl_off,
                    size_of::<u64>() * (nsegs - 1),
                    BUS_DMASYNC_PREWRITE,
                );
            }
        }

        if xs.flags.get() & SCSI_POLL != 0 {
            nvme_poll(
                sc,
                sc.io_q(),
                ccb,
                nvme_scsi_io_fill,
                xs.timeout.get().max(0) as u32,
            );
            return;
        }

        nvme_q_submit(sc, sc.io_q(), ccb, nvme_scsi_io_fill);
        return;
    }

    // stuffup:
    xs.error.set(XS_DRIVER_STUFFUP);
    scsi_done(xs);
}

/// The transfer a SCSI command's ccb carries.
fn nvme_ccb_xs(ccb: &NvmeCcb) -> &'static ScsiXfer {
    match ccb.ccb_cookie.get() {
        NvmeCookie::Xs(xs) => xs,
        _ => panic(format_args!("nvme: ccb {} without a transfer", ccb.ccb_id)),
    }
}

/// `nvme_scsi_io_fill`: the NVM read or write for the transfer's CDB.
pub fn nvme_scsi_io_fill(_sc: &NvmeSoftc, ccb: &NvmeCcb, slot: &mut NvmeSqe) {
    let xs = nvme_ccb_xs(ccb);
    let link = xs.link();
    let dmap = ccb.ccb_dmamap;
    let segs = dmap.dm_segs();

    let (lba, blocks) = scsi_cmd_rw_decode(&xs.cmd.get());

    let sqe = slot.as_io_mut();
    sqe.opcode = if xs.flags.get() & SCSI_DATA_IN != 0 {
        NVM_CMD_READ
    } else {
        NVM_CMD_WRITE
    };
    sqe.nsid = u32::from(link.target.get()).to_le();

    sqe.entry.set_prp(0, segs[0].get().ds_addr as u64);
    match dmap.dm_nsegs.get() {
        1 => {}
        2 => sqe.entry.set_prp(1, segs[1].get().ds_addr as u64),
        // the prp list is already set up and synced
        _ => sqe.entry.set_prp(1, ccb.ccb_prpl_dva),
    }

    sqe.slba = lba.to_le();
    sqe.nlb = (blocks.wrapping_sub(1) as u16).to_le();
}

/// `nvme_scsi_io_done`: unloads the transfer and finishes it.
pub fn nvme_scsi_io_done(sc: &NvmeSoftc, ccb: &NvmeCcb) {
    let xs = nvme_ccb_xs(ccb);
    let dmap = ccb.ccb_dmamap;
    let dmat = sc.dmat();

    let nsegs = dmap.dm_nsegs.get().max(0) as usize;
    if nsegs > 2
        && let Some(prpls) = sc.sc_ccb_prpls.get()
    {
        bus_dmamap_sync(
            dmat,
            nvme_dma_map(prpls),
            ccb.ccb_prpl_off,
            size_of::<u64>() * (nsegs - 1),
            BUS_DMASYNC_POSTWRITE,
        );
    }

    bus_dmamap_sync(
        dmat,
        dmap,
        0,
        dmap.dm_mapsize.get(),
        if xs.flags.get() & SCSI_DATA_IN != 0 {
            BUS_DMASYNC_POSTREAD
        } else {
            BUS_DMASYNC_POSTWRITE
        },
    );

    bus_dmamap_unload(dmat, dmap);

    xs.error.set(
        if nvme_cqe_sc(ccb.ccb_cqe_flags.get()) == NVME_CQE_SC_SUCCESS {
            XS_NOERROR
        } else {
            XS_DRIVER_STUFFUP
        },
    );
    xs.status.set(SCSI_OK);
    xs.resid.set(0);
    scsi_done(xs);
}

/// `nvme_scsi_sync`: SYNCHRONIZE CACHE as an NVM flush.
pub fn nvme_scsi_sync(xs: &'static ScsiXfer) {
    let link = xs.link();
    let sc = nvme_link_softc(link);
    let ccb = nvme_xs_ccb(xs);

    ccb.ccb_done.set(nvme_scsi_sync_done);
    ccb.ccb_cookie.set(NvmeCookie::Xs(xs));

    if xs.flags.get() & SCSI_POLL != 0 {
        nvme_poll(
            sc,
            sc.io_q(),
            ccb,
            nvme_scsi_sync_fill,
            xs.timeout.get().max(0) as u32,
        );
        return;
    }

    nvme_q_submit(sc, sc.io_q(), ccb, nvme_scsi_sync_fill);
}

/// `nvme_scsi_sync_fill`.
pub fn nvme_scsi_sync_fill(_sc: &NvmeSoftc, ccb: &NvmeCcb, slot: &mut NvmeSqe) {
    let xs = nvme_ccb_xs(ccb);
    let link = xs.link();

    slot.opcode = NVM_CMD_FLUSH;
    slot.nsid = u32::from(link.target.get()).to_le();
}

/// `nvme_scsi_sync_done`.
pub fn nvme_scsi_sync_done(_sc: &NvmeSoftc, ccb: &NvmeCcb) {
    let xs = nvme_ccb_xs(ccb);

    xs.error.set(
        if nvme_cqe_sc(ccb.ccb_cqe_flags.get()) == NVME_CQE_SC_SUCCESS {
            XS_NOERROR
        } else {
            XS_DRIVER_STUFFUP
        },
    );
    xs.status.set(SCSI_OK);
    xs.resid.set(0);
    scsi_done(xs);
}

/// `nvme_scsi_inq`: INQUIRY (no vital product data pages).
pub fn nvme_scsi_inq(xs: &'static ScsiXfer) {
    let inq = xs.cmd_as::<ScsiInquiry>();

    if inq.flags & SI_EVPD == 0 {
        nvme_scsi_inquiry(xs);
        return;
    }

    // switch (inq->pagecode): no page is answered.
    let _ = inq.pagecode;

    xs.error.set(XS_DRIVER_STUFFUP);
    scsi_done(xs);
}

/// The INQUIRY reply of `nvme_scsi_inquiry`: an SPC-4 direct access device, vendor
/// "NVMe", the controller's model number and firmware revision.
pub fn nvme_inquiry_data(mn: &[u8; 40], fr: &[u8; 8]) -> ScsiInquiryData {
    let mut inq = ScsiInquiryData::zeroed();

    inq.device = T_DIRECT;
    inq.version = SCSI_REV_SPC4;
    inq.response_format = SID_SCSI2_RESPONSE;
    inq.additional_length = SID_SCSI2_ALEN as u8;
    inq.flags |= SID_CmdQue;
    inq.vendor.copy_from_slice(b"NVMe    ");
    let n = inq.product.len();
    inq.product.copy_from_slice(&mn[..n]);
    let n = inq.revision.len();
    inq.revision.copy_from_slice(&fr[..n]);
    inq
}

/// The identify data of the namespace a transfer is for, which `nvme_scsi_probe` kept.
fn nvme_xs_ns(sc: &NvmeSoftc, xs: &ScsiXfer) -> &'static NvmIdentifyNamespace {
    let target = usize::from(xs.link().target.get());
    match sc.ns_ident(target) {
        // SAFETY: the copy lives until `nvme_scsi_free`, which runs after the link's
        // last transfer.
        Some(ns) => unsafe { &*ptr::from_ref(ns) },
        None => panic(format_args!(
            "{}: no namespace for target {target}",
            sc.sc_dev.xname()
        )),
    }
}

/// `nvme_scsi_inquiry`.
pub fn nvme_scsi_inquiry(xs: &'static ScsiXfer) {
    let link = xs.link();
    let sc = nvme_link_softc(link);

    let _ns = nvme_xs_ns(sc, xs);

    let id = sc.identify();
    let inq = nvme_inquiry_data(&id.mn, &id.fr);

    scsi_copy_internal_data(xs, inq.as_bytes());

    xs.error.set(XS_NOERROR);
    scsi_done(xs);
}

/// `memcpy(xs->data, reply, MIN(sizeof(reply), xs->datalen))`.
fn nvme_copy_reply(xs: &ScsiXfer, reply: &[u8]) {
    // SAFETY: the adapter owns the transfer between `scsi_cmd` and `scsi_done`, and holds
    // no other slice of its data.
    let data = unsafe { xs.data_slice() };
    let n = reply.len().min(data.len());
    data[..n].copy_from_slice(&reply[..n]);
}

/// The READ CAPACITY (16) reply for a namespace.
pub fn nvme_read_cap_data_16(ns: &NvmIdentifyNamespace) -> ScsiReadCapData16 {
    let tpe: u16 = READ_CAP_16_TPE;

    let addr = nvme_scsi_size(ns).wrapping_sub(1);
    let f = &ns.lbaf[usize::from(nvme_id_ns_flbas(ns.flbas))];

    let mut rcd = ScsiReadCapData16::zeroed();
    _lto8b(addr, &mut rcd.addr);
    _lto4b(
        1u32.checked_shl(u32::from(f.lbads)).unwrap_or(0),
        &mut rcd.length,
    );
    _lto2b(u32::from(tpe), &mut rcd.lowest_aligned);
    rcd
}

/// The READ CAPACITY (10) reply for a namespace (the last LBA clamped to 32 bits).
pub fn nvme_read_cap_data(ns: &NvmIdentifyNamespace) -> ScsiReadCapData {
    let mut addr = nvme_scsi_size(ns).wrapping_sub(1);
    if addr > 0xffff_ffff {
        addr = 0xffff_ffff;
    }

    let f = &ns.lbaf[usize::from(nvme_id_ns_flbas(ns.flbas))];

    let mut rcd = ScsiReadCapData::zeroed();
    _lto4b(addr as u32, &mut rcd.addr);
    _lto4b(
        1u32.checked_shl(u32::from(f.lbads)).unwrap_or(0),
        &mut rcd.length,
    );
    rcd
}

/// `nvme_scsi_capacity16`: READ CAPACITY (16).
pub fn nvme_scsi_capacity16(xs: &'static ScsiXfer) {
    let link = xs.link();
    let sc = nvme_link_softc(link);

    let ns = nvme_xs_ns(sc, xs);

    if xs.cmdlen.get() != size_of::<ScsiReadCapacity16>() as i32 {
        xs.error.set(XS_DRIVER_STUFFUP);
        scsi_done(xs);
        return;
    }

    let rcd = nvme_read_cap_data_16(ns);
    nvme_copy_reply(xs, rcd.as_bytes());

    xs.error.set(XS_NOERROR);
    scsi_done(xs);
}

/// `nvme_scsi_capacity`: READ CAPACITY (10).
pub fn nvme_scsi_capacity(xs: &'static ScsiXfer) {
    let link = xs.link();
    let sc = nvme_link_softc(link);

    let ns = nvme_xs_ns(sc, xs);

    if xs.cmdlen.get() != size_of::<ScsiReadCapacity>() as i32 {
        xs.error.set(XS_DRIVER_STUFFUP);
        scsi_done(xs);
        return;
    }

    let rcd = nvme_read_cap_data(ns);
    nvme_copy_reply(xs, rcd.as_bytes());

    xs.error.set(XS_NOERROR);
    scsi_done(xs);
}

/// `nvme_scsi_free`: the adapter's `dev_free`: forgets the namespace's identify data.
pub fn nvme_scsi_free(link: &'static ScsiLink) {
    let sc = nvme_link_softc(link);

    let Some(ns) = sc.namespace(usize::from(link.target.get())) else {
        return;
    };
    if let Some(identify) = ns.ident.take() {
        free(identify.cast(), M_DEVBUF, size_of::<NvmIdentifyNamespace>());
    }
}

/// `nvme_scsi_size`: the namespace's size in blocks (its capacity when thin provisioned
/// and smaller).
pub fn nvme_scsi_size(ns: &NvmIdentifyNamespace) -> u64 {
    let ncap = u64::from_le(ns.ncap); // Max allowed allocation.
    let nsze = u64::from_le(ns.nsze);

    if ns.nsfeat & NVME_ID_NS_NSFEAT_THIN_PROV != 0 && ncap < nsze {
        ncap
    } else {
        nsze
    }
}

/// `nvme_passthrough_cmd`: one admin command from user space (identify, log page, self
/// test), its data and status copied out.
pub fn nvme_passthrough_cmd(
    sc: &'static NvmeSoftc,
    pt: &NvmePtCmd,
    dv_unit: i32,
    nsid: i32,
) -> Result<(), Errno> {
    let mut mem: Option<&'static NvmeDmamem> = None;

    let (_io, ccb) = nvme_scsi_io_get(sc);

    let mut sqe = NvmeSqe::zeroed();
    sqe.opcode = pt.pt_opcode;
    sqe.nsid = pt.pt_nsid.to_le();
    sqe.cdw10 = pt.pt_cdw10.to_le();
    sqe.cdw11 = pt.pt_cdw11.to_le();
    sqe.cdw12 = pt.pt_cdw12.to_le();
    sqe.cdw13 = pt.pt_cdw13.to_le();
    sqe.cdw14 = pt.pt_cdw14.to_le();
    sqe.cdw15 = pt.pt_cdw15.to_le();

    ccb.ccb_done.set(nvme_empty_done);
    ccb.ccb_cookie.set(NvmeCookie::Sqe(sqe));

    let rv = 'done: {
        match pt.pt_opcode {
            NVM_ADMIN_IDENTIFY | NVM_ADMIN_GET_LOG_PG | NVM_ADMIN_SELFTEST => {}
            _ => break 'done Err(Errno::ENOTTY),
        }

        if pt.pt_databuflen > 0 {
            let Some(m) = nvme_dmamem_alloc(sc, pt.pt_databuflen as usize) else {
                break 'done Err(Errno::ENOMEM);
            };
            mem = Some(m);
            sqe.entry.set_prp(0, nvme_dma_dva(m));
            ccb.ccb_cookie.set(NvmeCookie::Sqe(sqe));
            nvme_dmamem_sync(sc, m, BUS_DMASYNC_PREREAD);
        }

        let flags = nvme_poll(sc, sc.admin_q(), ccb, nvme_sqe_fill, NVME_TIMO_PT);

        let mut rv = Ok(());
        if let Some(m) = mem {
            nvme_dmamem_sync(sc, m, BUS_DMASYNC_POSTREAD);
            if flags == 0 {
                // SAFETY: the DMA area of `pt_databuflen` bytes allocated above, which the
                // controller has finished writing (synced above).
                let data = unsafe {
                    core::slice::from_raw_parts(nvme_dma_kva(m), pt.pt_databuflen as usize)
                };
                rv = copyout(data, pt.pt_databuf);
            }
        }

        if rv.is_ok() && pt.pt_statuslen > 0 {
            let pt_status = NvmePtStatus {
                ps_dv_unit: dv_unit,
                ps_nsid: nsid,
                ps_flags: i32::from(flags),
                ps_cc: nvme_read4(sc, NVME_CC),
                ps_csts: nvme_read4(sc, NVME_CSTS),
            };
            let bytes = pt_status.to_bytes();
            let n = bytes.len().min(pt.pt_statuslen as usize);
            rv = copyout(&bytes[..n], pt.pt_status);
        }

        rv
    };

    // done:
    if let Some(m) = mem {
        // SAFETY: allocated above, no longer used by the controller.
        unsafe { nvme_dmamem_free(sc, m) };
    }
    // SAFETY: the ccb taken above, finished.
    unsafe { nvme_ccb_put(nvme_cookie(sc), NonNull::from(ccb).cast()) };

    rv
}

/// `nvme_scsi_ioctl`: the adapter's `ioctl`: the passthrough command on a namespace.
///
/// # Safety
///
/// `addr` is the aligned kernel copy of the command's argument (`ScsiAdapterIoctlFn`).
pub unsafe fn nvme_scsi_ioctl(
    link: &'static ScsiLink,
    cmd: u64,
    addr: *mut u8,
    _flag: i32,
) -> Result<(), Errno> {
    let sc = nvme_link_softc(link);

    match cmd {
        NVME_PASSTHROUGH_CMD => {}
        _ => return Err(Errno::ENOTTY),
    }

    let pt = addr.cast::<NvmePtCmd>();
    // SAFETY: the caller's guarantee: `NVME_PASSTHROUGH_CMD` carries a `struct nvme_pt_cmd`,
    // every bit pattern of which is valid.
    let mut cmdv = unsafe { ptr::read_unaligned(pt) };
    if cmdv.pt_cdw10 & 0xff == 0 {
        cmdv.pt_nsid = u32::from(link.target.get());
        // SAFETY: as above; one member written back, as the C does.
        unsafe { ptr::write_unaligned(&raw mut (*pt).pt_nsid, cmdv.pt_nsid) };
    }

    rw_enter_write(&sc.sc_lock);
    let rv = nvme_passthrough_cmd(
        sc,
        &cmdv,
        sc.sc_dev.dv_unit.get(),
        i32::from(link.target.get()),
    );
    rw_exit_write(&sc.sc_lock);

    rv
}

/// `nvme_op_sq_enter`: locks the submission side; the tail slot.
pub fn nvme_op_sq_enter(sc: &NvmeSoftc, q: &NvmeQueue, ccb: Option<&NvmeCcb>) -> u32 {
    mtx_enter(&q.q_sq_mtx);
    nvme_op_sq_enter_locked(sc, q, ccb)
}

/// `nvme_op_sq_enter_locked`.
pub fn nvme_op_sq_enter_locked(_sc: &NvmeSoftc, q: &NvmeQueue, _ccb: Option<&NvmeCcb>) -> u32 {
    q.q_sq_tail.get()
}

/// `nvme_op_sq_leave_locked`: advances the tail and rings the doorbell.
pub fn nvme_op_sq_leave_locked(sc: &NvmeSoftc, q: &NvmeQueue, _ccb: Option<&NvmeCcb>) {
    let mut tail = q.q_sq_tail.get() + 1;
    if tail >= q.q_entries {
        tail = 0;
    }
    q.q_sq_tail.set(tail);
    nvme_write4(sc, q.q_sqtdbl, tail);
}

/// `nvme_op_sq_leave`.
pub fn nvme_op_sq_leave(sc: &NvmeSoftc, q: &NvmeQueue, ccb: Option<&NvmeCcb>) {
    nvme_op_sq_leave_locked(sc, q, ccb);
    mtx_leave(&q.q_sq_mtx);
}

/// Writes `sqe` into the submission queue slot at `slot` of `sqe_size` bytes, zeroing the
/// rest of a larger slot, with volatile writes (the controller reads the ring).
///
/// # Safety
///
/// `slot` is a slot of a submission queue's DMA area, `sqe_size` (64 or 128) bytes long
/// and 8-byte aligned, owned by the caller between `op_sq_enter` and `op_sq_leave`.
unsafe fn nvme_sqe_write(slot: *mut u8, sqe_size: usize, sqe: &NvmeSqe) {
    let words = slot.cast::<u64>();
    for i in size_of::<NvmeSqe>() / 8..sqe_size / 8 {
        // SAFETY: inside the slot (the caller's guarantee).
        unsafe { ptr::write_volatile(words.add(i), 0) };
    }
    // SAFETY: as above; `NvmeSqe` is 64 bytes, aligned to 8.
    unsafe { ptr::write_volatile(slot.cast::<NvmeSqe>(), *sqe) };
}

/// `nvme_q_submit`: fills the queue's tail slot with `fill` and submits it.
pub fn nvme_q_submit(sc: &NvmeSoftc, q: &NvmeQueue, ccb: &NvmeCcb, fill: NvmeFillFn) {
    let dmat = sc.dmat();
    let sqe_size = if q.q_id == NVME_ADMIN_Q {
        size_of::<NvmeSqe>()
    } else {
        sc.sc_sqe_size.get() as usize
    };

    let tail = (sc.ops().op_sq_enter)(sc, q, Some(ccb)) as usize;
    if tail >= q.q_entries as usize {
        panic(format_args!("nvme_q_submit: tail {tail} past the queue"));
    }

    // SAFETY: `tail` is below `q_entries` and the area holds `q_entries` slots of
    // `sqe_size` bytes (nvme_q_alloc).
    let slot = unsafe { nvme_dma_kva(q.q_sq_dmamem).add(tail * sqe_size) };

    bus_dmamap_sync(
        dmat,
        nvme_dma_map(q.q_sq_dmamem),
        sqe_size * tail,
        sqe_size,
        BUS_DMASYNC_POSTWRITE,
    );
    let mut sqe = NvmeSqe::zeroed();
    fill(sc, ccb, &mut sqe);
    sqe.cid = ccb.ccb_id;
    // SAFETY: the slot above, ours until op_sq_leave.
    unsafe { nvme_sqe_write(slot, sqe_size, &sqe) };
    bus_dmamap_sync(
        dmat,
        nvme_dma_map(q.q_sq_dmamem),
        sqe_size * tail,
        sqe_size,
        BUS_DMASYNC_PREWRITE,
    );

    (sc.ops().op_sq_leave)(sc, q, Some(ccb));
}

/// `nvme_poll`: submits the command `fill` writes and reaps the queue until it is done
/// or `ms` milliseconds have passed (0: forever); then runs the ccb's own done function.
/// Returns the completion's flags without the phase bit (0: success).
pub fn nvme_poll(sc: &NvmeSoftc, q: &NvmeQueue, ccb: &NvmeCcb, fill: NvmeFillFn, ms: u32) -> u16 {
    let mut state = NvmePollState {
        s: NvmeSqe::zeroed(),
        c: NvmeCqe::default(),
    };
    fill(sc, ccb, &mut state.s);

    let done = ccb.ccb_done.get();
    let cookie = ccb.ccb_cookie.get();

    ccb.ccb_done.set(nvme_poll_done);
    ccb.ccb_cookie.set(NvmeCookie::Poll(state));

    nvme_q_submit(sc, q, ccb, nvme_poll_fill);
    let mut us = i64::from(ms) * 1000;
    while ms == 0 || us > 0 {
        if let NvmeCookie::Poll(st) = ccb.ccb_cookie.get()
            && st.c.flags & NVME_CQE_PHASE != 0
        {
            break;
        }
        if nvme_q_complete(sc, q) == 0 {
            delay(NVME_TIMO_DELAYNS);
        }
        nvme_barrier(sc, NVME_CSTS, 4, BUS_SPACE_BARRIER_READ);
        us -= i64::from(NVME_TIMO_DELAYNS);
    }

    ccb.ccb_cookie.set(cookie);
    done(sc, ccb);

    ccb.ccb_cqe_flags.get() & !NVME_CQE_PHASE
}

/// `nvme_poll_fill`: the command `nvme_poll` was given.
pub fn nvme_poll_fill(_sc: &NvmeSoftc, ccb: &NvmeCcb, slot: &mut NvmeSqe) {
    match ccb.ccb_cookie.get() {
        NvmeCookie::Poll(state) => *slot = state.s,
        _ => panic(format_args!(
            "nvme_poll_fill: ccb {} not polled",
            ccb.ccb_id
        )),
    }
}

/// `nvme_poll_done`: marks `nvme_poll`'s state done with the completion's flags.
pub fn nvme_poll_done(_sc: &NvmeSoftc, ccb: &NvmeCcb) {
    if let NvmeCookie::Poll(mut state) = ccb.ccb_cookie.get() {
        state.c.flags = ccb.ccb_cqe_flags.get() | NVME_CQE_PHASE;
        ccb.ccb_cookie.set(NvmeCookie::Poll(state));
    }
    // Otherwise the poll timed out and restored the cookie: see the module's deviations.
}

/// `nvme_sqe_fill`: the command the cookie holds.
pub fn nvme_sqe_fill(_sc: &NvmeSoftc, ccb: &NvmeCcb, slot: &mut NvmeSqe) {
    match ccb.ccb_cookie.get() {
        NvmeCookie::Sqe(src) => *slot = src,
        _ => panic(format_args!(
            "nvme_sqe_fill: ccb {} without a command",
            ccb.ccb_id
        )),
    }
}

/// `nvme_empty_done`.
pub fn nvme_empty_done(_sc: &NvmeSoftc, _ccb: &NvmeCcb) {}

/// `nvme_op_cq_done`: nop.
pub fn nvme_op_cq_done(_sc: &NvmeSoftc, _q: &NvmeQueue, _ccb: &NvmeCcb) {}

/// `nvme_q_complete`: reaps the completion queue and runs the done function of every
/// completed command. 1 if something completed, 0 if not, -1 if another CPU is reaping.
pub fn nvme_q_complete(sc: &NvmeSoftc, q: &NvmeQueue) -> i32 {
    let ring = nvme_dma_kva(q.q_cq_dmamem).cast::<NvmeCqe>();
    let mut rv = 0;

    if !mtx_enter_try(&q.q_cq_mtx) {
        return -1;
    }

    let done_list: SimpleqHead<NvmeCcbList> = SimpleqHead::new();
    let mut head = q.q_cq_head.get();

    nvme_dmamem_sync(sc, q.q_cq_dmamem, BUS_DMASYNC_POSTREAD);
    loop {
        // SAFETY: `head` stays below `q_entries`, the number of entries of the area.
        let cqe = unsafe { ring.add(head as usize) };
        // SAFETY: an entry of the ring, which the controller writes: read volatile.
        let flags = u16::from_le(unsafe { ptr::read_volatile(&raw const (*cqe).flags) });
        if flags & NVME_CQE_PHASE != q.q_cq_phase.get() {
            break;
        }

        fence(Ordering::Acquire); // membar_consumer()

        // SAFETY: as above.
        let cid = unsafe { ptr::read_volatile(&raw const (*cqe).cid) };
        let ccb = sc.ccb(usize::from(cid));
        (sc.ops().op_cq_done)(sc, q, ccb);

        // SAFETY: as above.
        ccb.ccb_cqe_flags.set(u16::from_le(unsafe {
            ptr::read_volatile(&raw const (*cqe).flags)
        }));
        // SAFETY: a completed ccb is on no list (taken off the free list when issued); it
        // stays in `sc_ccbs` while the list below is walked.
        unsafe { done_list.insert_tail(ccb) };

        head += 1;
        if head >= q.q_entries {
            head = 0;
            q.q_cq_phase.set(q.q_cq_phase.get() ^ NVME_CQE_PHASE);
        }

        rv = 1;
    }
    nvme_dmamem_sync(sc, q.q_cq_dmamem, BUS_DMASYNC_PREREAD);

    if rv != 0 {
        q.q_cq_head.set(head);
        nvme_write4(sc, q.q_cqhdbl, head);
    }
    mtx_leave(&q.q_cq_mtx);

    // SIMPLEQ_FOREACH_SAFE: the iterator reads each next link before yielding, so a done
    // function may put its ccb back on the free list.
    for ccb in done_list.iter() {
        (ccb.ccb_done.get())(sc, ccb);
    }

    rv
}

/// `nvme_identify`: identifies the controller, prints it and sets the transfer limits and
/// the number of namespaces.
pub fn nvme_identify(sc: &'static NvmeSoftc, mpsmin: u32) -> Result<(), Errno> {
    let mut sn = [0u8; 41];
    let mut mn = [0u8; 81];
    let mut fr = [0u8; 17];

    // SAFETY: the softc is the iopool's own cookie.
    let Some(io) = (unsafe { nvme_ccb_get(nvme_cookie(sc)) }) else {
        panic(format_args!("nvme_identify: nvme_ccb_get returned NULL"));
    };
    // SAFETY: from nvme_ccb_get.
    let ccb = unsafe { nvme_io_ccb(io) };

    let Some(mem) = nvme_dmamem_alloc(sc, size_of::<NvmIdentifyController>()) else {
        return Err(Errno::ENOMEM);
    };

    ccb.ccb_done.set(nvme_empty_done);
    ccb.ccb_cookie.set(NvmeCookie::Dmamem(mem));

    nvme_dmamem_sync(sc, mem, BUS_DMASYNC_PREREAD);
    let flags = nvme_poll(sc, sc.admin_q(), ccb, nvme_fill_identify, NVME_TIMO_IDENT);
    nvme_dmamem_sync(sc, mem, BUS_DMASYNC_POSTREAD);

    // SAFETY: the ccb taken above, finished.
    unsafe { nvme_ccb_put(nvme_cookie(sc), io) };

    let rv = 'done: {
        if flags != 0 {
            break 'done Err(Errno::EIO);
        }

        // SAFETY: the DMA area holds the identify data the controller wrote (synced above),
        // as large as `NvmIdentifyController`, all of whose bit patterns are valid.
        let identify = unsafe { &*nvme_dma_kva(mem).cast::<NvmIdentifyController>() };

        scsi_strvis(&mut sn, &identify.sn);
        scsi_strvis(&mut mn, &identify.mn);
        scsi_strvis(&mut fr, &identify.fr);

        printf(format_args!(
            "{}: {}, firmware {}, serial {}\n",
            sc.sc_dev.xname(),
            Str(&mn),
            Str(&fr),
            Str(&sn)
        ));

        if identify.mdts > 0 {
            let mdts = 1usize
                .checked_shl(u32::from(identify.mdts))
                .unwrap_or(usize::MAX)
                .saturating_mul(1usize.checked_shl(mpsmin).unwrap_or(usize::MAX));
            sc.sc_mdts.set(mdts.min(NVME_MAXPHYS));
            sc.sc_max_prpl
                .set((sc.sc_mdts.get() / sc.sc_mps.get()) as u32);
        }

        sc.sc_nn.set(u32::from_le(identify.nn));

        if sc.sc_sqe_size.get() == 0 {
            sc.sc_sqe_size.set(size_of::<NvmeSqe>() as u32);
        }

        // At least one Apple NVMe device presents a second, bogus disk that is
        // inaccessible, so cap targets at 1.
        //
        // sd1 at scsibus1 targ 2 lun 0: <NVMe, APPLE SSD AP0512, 16.1> [..]
        // sd1: 0MB, 4096 bytes/sector, 2 sectors
        if sc.sc_nn.get() > 1 && mn.starts_with(b"APPLE") {
            sc.sc_nn.set(1);
        }

        // SAFETY: `sc_identify` is written only here, during attach, before anything reads
        // it; the source is the DMA area above; they do not overlap. Copied in place, not
        // through the stack (4 KiB).
        unsafe { ptr::copy_nonoverlapping(ptr::from_ref(identify), sc.sc_identify.get(), 1) };

        Ok(())
    };

    // done:
    // SAFETY: allocated above, no longer used by the controller.
    unsafe { nvme_dmamem_free(sc, mem) };

    rv
}

/// `nvme_q_create`: creates the queue pair on the controller (completion queue first).
pub fn nvme_q_create(sc: &'static NvmeSoftc, q: &NvmeQueue) -> Result<(), Errno> {
    let (io, ccb) = nvme_scsi_io_get(sc);

    let rv = 'fail: {
        let mut sqe = NvmeSqe::zeroed();
        {
            let s = sqe.as_q_mut();
            s.opcode = NVM_ADMIN_ADD_IOCQ;
            s.prp1 = nvme_dma_dva(q.q_cq_dmamem).to_le();
            s.qsize = ((q.q_entries - 1) as u16).to_le();
            s.qid = q.q_id.to_le();
            s.qflags = NVM_SQE_CQ_IEN | NVM_SQE_Q_PC;
        }
        ccb.ccb_done.set(nvme_empty_done);
        ccb.ccb_cookie.set(NvmeCookie::Sqe(sqe));

        if nvme_poll(sc, sc.admin_q(), ccb, nvme_sqe_fill, NVME_TIMO_QOP) != 0 {
            break 'fail Err(Errno::EIO);
        }

        let mut sqe = NvmeSqe::zeroed();
        {
            let s = sqe.as_q_mut();
            s.opcode = NVM_ADMIN_ADD_IOSQ;
            s.prp1 = nvme_dma_dva(q.q_sq_dmamem).to_le();
            s.qsize = ((q.q_entries - 1) as u16).to_le();
            s.qid = q.q_id.to_le();
            s.cqid = q.q_id.to_le();
            s.qflags = NVM_SQE_Q_PC;
        }
        ccb.ccb_done.set(nvme_empty_done);
        ccb.ccb_cookie.set(NvmeCookie::Sqe(sqe));

        if nvme_poll(sc, sc.admin_q(), ccb, nvme_sqe_fill, NVME_TIMO_QOP) != 0 {
            break 'fail Err(Errno::EIO);
        }

        Ok(())
    };

    // fail:
    scsi_io_put(&sc.sc_iopool, io);
    rv
}

/// `nvme_q_delete`: deletes the queue pair on the controller (submission queue first) and
/// frees it.
pub fn nvme_q_delete(sc: &'static NvmeSoftc, q: &'static NvmeQueue) -> Result<(), Errno> {
    let (io, ccb) = nvme_scsi_io_get(sc);

    let rv = 'fail: {
        let mut sqe = NvmeSqe::zeroed();
        {
            let s = sqe.as_q_mut();
            s.opcode = NVM_ADMIN_DEL_IOSQ;
            s.qid = q.q_id.to_le();
        }
        ccb.ccb_done.set(nvme_empty_done);
        ccb.ccb_cookie.set(NvmeCookie::Sqe(sqe));

        if nvme_poll(sc, sc.admin_q(), ccb, nvme_sqe_fill, NVME_TIMO_QOP) != 0 {
            break 'fail Err(Errno::EIO);
        }

        let mut sqe = NvmeSqe::zeroed();
        {
            let s = sqe.as_q_mut();
            s.opcode = NVM_ADMIN_DEL_IOCQ;
            s.qid = q.q_id.to_le();
        }
        ccb.ccb_done.set(nvme_empty_done);
        ccb.ccb_cookie.set(NvmeCookie::Sqe(sqe));

        if nvme_poll(sc, sc.admin_q(), ccb, nvme_sqe_fill, NVME_TIMO_QOP) != 0 {
            break 'fail Err(Errno::EIO);
        }

        if sc.sc_q.get().is_some_and(|cur| ptr::eq(cur, q)) {
            sc.sc_q.set(None);
        }
        // SAFETY: the queue is gone from the controller and from the softc.
        unsafe { nvme_q_free(sc, q) };

        Ok(())
    };

    // fail:
    scsi_io_put(&sc.sc_iopool, io);
    rv
}

/// `nvme_fill_identify`: Identify Controller into the cookie's DMA area.
pub fn nvme_fill_identify(_sc: &NvmeSoftc, ccb: &NvmeCcb, slot: &mut NvmeSqe) {
    let NvmeCookie::Dmamem(mem) = ccb.ccb_cookie.get() else {
        panic(format_args!(
            "nvme_fill_identify: ccb {} without memory",
            ccb.ccb_id
        ));
    };

    slot.opcode = NVM_ADMIN_IDENTIFY;
    slot.entry.set_prp(0, nvme_dma_dva(mem));
    slot.cdw10 = 1u32.to_le();
}

/// `nvme_ccbs_alloc`: `nccbs` ccbs, each with a transfer map and a PRP list, on the free
/// list.
pub fn nvme_ccbs_alloc(sc: &NvmeSoftc, nccbs: u32) -> Result<(), Errno> {
    let dmat = sc.dmat();
    let max_prpl = sc.sc_max_prpl.get() as usize;

    let Some(ccbs) = mallocarray(
        nccbs as usize,
        size_of::<NvmeCcb>(),
        M_DEVBUF,
        M_WAITOK | M_CANFAIL,
    ) else {
        return Err(Errno::ENOMEM);
    };
    let ccbs = ccbs.cast::<NvmeCcb>();
    sc.sc_ccbs.set(ccbs.as_ptr());

    let Some(prpls) = nvme_dmamem_alloc(sc, size_of::<u64>() * max_prpl * nccbs as usize) else {
        free(ccbs.cast(), M_DEVBUF, nccbs as usize * size_of::<NvmeCcb>());
        sc.sc_ccbs.set(ptr::null_mut());
        return Err(Errno::ENOMEM);
    };
    sc.sc_ccb_prpls.set(Some(prpls));

    let mut prpl = nvme_dma_kva(prpls).cast::<u64>();
    let mut off = 0usize;

    for i in 0..nccbs as usize {
        let Ok(map) = bus_dmamap_create(
            dmat,
            sc.sc_mdts.get(),
            (max_prpl + 1) as i32, // we get a free prp in the sqe
            sc.sc_mps.get(),
            sc.sc_mps.get(),
            BUS_DMA_WAITOK | BUS_DMA_ALLOCNOW | BUS_DMA_64BIT,
        ) else {
            // free_maps:
            nvme_ccbs_free(sc, nccbs);
            return Err(Errno::ENOMEM);
        };

        // SAFETY: `i` is below `nccbs`, the length of the allocation; the slot is written
        // whole before anything reads it.
        let ccb = unsafe {
            let p = ccbs.as_ptr().add(i);
            p.write(NvmeCcb {
                ccb_entry: crate::sys::queue::SimpleqEntry::new(),
                ccb_dmamap: map,
                ccb_cookie: core::cell::Cell::new(NvmeCookie::None),
                ccb_done: core::cell::Cell::new(nvme_empty_done),
                ccb_prpl_off: off,
                ccb_prpl_dva: nvme_dma_dva(prpls) + off as u64,
                ccb_prpl: prpl,
                ccb_id: i as u16,
                ccb_cqe_flags: core::cell::Cell::new(0),
            });
            &*p
        };

        mtx_enter(&sc.sc_ccb_mtx);
        // SAFETY: a new ccb, on no list, in place until nvme_ccbs_free.
        unsafe { sc.sc_ccb_list.insert_tail(ccb) };
        mtx_leave(&sc.sc_ccb_mtx);

        // SAFETY: still inside the PRP list area (`max_prpl` entries per ccb).
        prpl = unsafe { prpl.add(max_prpl) };
        off += size_of::<u64>() * max_prpl;
    }
    sc.sc_nccbs.set(nccbs);

    Ok(())
}

/// `nvme_ccb_get`: the iopool's `io_get`: a free ccb.
///
/// # Safety
///
/// `cookie` is a live [`NvmeSoftc`] (the one `nvme_attach` gave `scsi_iopool_init`).
pub unsafe fn nvme_ccb_get(cookie: *mut c_void) -> Option<ScsiIo> {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*cookie.cast::<NvmeSoftc>().cast_const() };

    mtx_enter(&sc.sc_ccb_mtx);
    let ccb = sc.sc_ccb_list.first().map(NonNull::from);
    if ccb.is_some() {
        // SAFETY: the list is not empty.
        unsafe { sc.sc_ccb_list.remove_head() };
    }
    mtx_leave(&sc.sc_ccb_mtx);

    ccb.map(NonNull::cast)
}

/// `nvme_ccb_put`: the iopool's `io_put`: the ccb back on the free list.
///
/// # Safety
///
/// `cookie` is a live [`NvmeSoftc`] and `io` one of its ccbs, from [`nvme_ccb_get`] and
/// not on the free list.
pub unsafe fn nvme_ccb_put(cookie: *mut c_void, io: ScsiIo) {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { &*cookie.cast::<NvmeSoftc>().cast_const() };
    // SAFETY: the caller's guarantee.
    let ccb = unsafe { nvme_io_ccb(io) };

    mtx_enter(&sc.sc_ccb_mtx);
    // SAFETY: the ccb is on no list and stays in `sc_ccbs` until nvme_ccbs_free.
    unsafe { sc.sc_ccb_list.insert_head(ccb) };
    mtx_leave(&sc.sc_ccb_mtx);
}

/// `nvme_ccbs_free`: destroys the maps of the free ccbs and frees the ccbs and their PRP
/// lists.
pub fn nvme_ccbs_free(sc: &NvmeSoftc, nccbs: u32) {
    let dmat = sc.dmat();

    while let Some(ccb) = sc.sc_ccb_list.first() {
        let map = ccb.ccb_dmamap;
        // SAFETY: the list is not empty.
        unsafe { sc.sc_ccb_list.remove_head() };
        // SAFETY: the ccb's own map, which nothing else holds.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
    }

    if let Some(prpls) = sc.sc_ccb_prpls.take() {
        // SAFETY: the PRP lists of ccbs that no longer exist.
        unsafe { nvme_dmamem_free(sc, prpls) };
    }
    sc.sc_nccbs.set(0);
    let ccbs = sc.sc_ccbs.replace(ptr::null_mut());
    if let Some(ccbs) = NonNull::new(ccbs) {
        free(ccbs.cast(), M_DEVBUF, nccbs as usize * size_of::<NvmeCcb>());
    }
}

/// `nvme_q_alloc`: a queue pair of `entries` entries with identifier `id` (not yet
/// created on the controller).
pub fn nvme_q_alloc(
    sc: &NvmeSoftc,
    id: u16,
    entries: u32,
    dstrd: u32,
) -> Option<&'static NvmeQueue> {
    let mem = malloc(size_of::<NvmeQueue>(), M_DEVBUF, M_WAITOK | M_CANFAIL)?;
    let qp = mem.cast::<NvmeQueue>();

    let sqe_size = if id == NVME_ADMIN_Q {
        size_of::<NvmeSqe>()
    } else {
        sc.sc_sqe_size.get() as usize
    };
    let Some(sq) = nvme_dmamem_alloc(sc, sqe_size * entries as usize) else {
        // free:
        free(mem, M_DEVBUF, size_of::<NvmeQueue>());
        return None;
    };

    let Some(cq) = nvme_dmamem_alloc(sc, size_of::<NvmeCqe>() * entries as usize) else {
        // free_sq:
        // SAFETY: allocated above, never given to the controller.
        unsafe { nvme_dmamem_free(sc, sq) };
        free(mem, M_DEVBUF, size_of::<NvmeQueue>());
        return None;
    };

    // SAFETY: both areas were just allocated, `NVME_DMA_LEN` bytes each, and nothing else
    // uses them yet.
    unsafe {
        ptr::write_bytes(nvme_dma_kva(sq), 0, nvme_dma_len(sq));
        ptr::write_bytes(nvme_dma_kva(cq), 0, nvme_dma_len(cq));
    }

    // SAFETY: a fresh allocation of `size_of::<NvmeQueue>()` bytes (malloc aligns it),
    // written whole before any use; it lives until nvme_q_free.
    let q: &'static NvmeQueue = unsafe {
        qp.as_ptr().write(NvmeQueue {
            q_sq_mtx: Mutex::new(IPL_BIO),
            q_cq_mtx: Mutex::new(IPL_BIO),
            q_sq_dmamem: sq,
            q_cq_dmamem: cq,
            q_nvmmu_dmamem: core::cell::Cell::new(None),
            q_sqtdbl: nvme_sqtdbl(id, dstrd),
            q_cqhdbl: nvme_cqhdbl(id, dstrd),
            q_id: id,
            q_entries: entries,
            q_sq_tail: core::cell::Cell::new(0),
            q_cq_head: core::cell::Cell::new(0),
            q_cq_phase: core::cell::Cell::new(NVME_CQE_PHASE),
        });
        &*qp.as_ptr()
    };
    mtx_init(&q.q_sq_mtx, IPL_BIO);
    mtx_init(&q.q_cq_mtx, IPL_BIO);

    if let Some(op_q_alloc) = sc.ops().op_q_alloc
        && op_q_alloc(sc, q).is_err()
    {
        // free_cq, free_sq, free:
        // SAFETY: allocated above, never given to the controller.
        unsafe {
            nvme_dmamem_free(sc, cq);
            nvme_dmamem_free(sc, sq);
        }
        free(mem, M_DEVBUF, size_of::<NvmeQueue>());
        return None;
    }

    nvme_dmamem_sync(sc, sq, BUS_DMASYNC_PREWRITE);
    nvme_dmamem_sync(sc, cq, BUS_DMASYNC_PREREAD);

    Some(q)
}

/// `nvme_q_reset`: empties the queue pair (after a controller reset).
pub fn nvme_q_reset(sc: &NvmeSoftc, q: &NvmeQueue) -> Result<(), Errno> {
    // SAFETY: the queue's own areas, `NVME_DMA_LEN` bytes each; the controller is disabled.
    unsafe {
        ptr::write_bytes(nvme_dma_kva(q.q_sq_dmamem), 0, nvme_dma_len(q.q_sq_dmamem));
        ptr::write_bytes(nvme_dma_kva(q.q_cq_dmamem), 0, nvme_dma_len(q.q_cq_dmamem));
    }

    q.q_sq_tail.set(0);
    q.q_cq_head.set(0);
    q.q_cq_phase.set(NVME_CQE_PHASE);

    nvme_dmamem_sync(sc, q.q_sq_dmamem, BUS_DMASYNC_PREWRITE);
    nvme_dmamem_sync(sc, q.q_cq_dmamem, BUS_DMASYNC_PREREAD);

    Ok(())
}

/// `nvme_q_free`: frees a queue pair.
///
/// # Safety
///
/// `q` came from [`nvme_q_alloc`], the controller no longer uses it (deleted or never
/// created) and nothing references it afterwards.
pub unsafe fn nvme_q_free(sc: &NvmeSoftc, q: &'static NvmeQueue) {
    nvme_dmamem_sync(sc, q.q_cq_dmamem, BUS_DMASYNC_POSTREAD);
    nvme_dmamem_sync(sc, q.q_sq_dmamem, BUS_DMASYNC_POSTWRITE);

    if let Some(op_q_free) = sc.ops().op_q_free {
        op_q_free(sc, q);
    }

    // SAFETY: the caller's guarantee; the areas are the queue's own.
    unsafe {
        nvme_dmamem_free(sc, q.q_cq_dmamem);
        nvme_dmamem_free(sc, q.q_sq_dmamem);
    }
    free(NonNull::from(q).cast(), M_DEVBUF, size_of::<NvmeQueue>());
}

/// `nvme_intr`: the MSI and MSI-X interrupt handler: reaps both queues.
pub fn nvme_intr(xsc: *mut c_void) -> i32 {
    // SAFETY: established by nvme_pci_attach with the softc as the argument; softcs are
    // never freed while their interrupt is established.
    let sc = unsafe { &*xsc.cast::<NvmeSoftc>().cast_const() };
    let mut rv = 0;

    if let Some(q) = sc.sc_q.get()
        && nvme_q_complete(sc, q) != 0
    {
        rv = 1;
    }
    if let Some(q) = sc.sc_admin_q.get()
        && nvme_q_complete(sc, q) != 0
    {
        rv = 1;
    }

    rv
}

/// `nvme_intr_intx`: the INTx handler: masks the controller's interrupt around
/// `nvme_intr`.
pub fn nvme_intr_intx(xsc: *mut c_void) -> i32 {
    // SAFETY: as for nvme_intr.
    let sc = unsafe { &*xsc.cast::<NvmeSoftc>().cast_const() };

    nvme_write4(sc, NVME_INTMS, 1);
    let rv = nvme_intr(xsc);
    nvme_write4(sc, NVME_INTMC, 1);

    rv
}

/// `nvme_dmamem_alloc`: `size` bytes of zeroed, physically contiguous DMA memory aligned to
/// the controller's page size, mapped and loaded.
pub fn nvme_dmamem_alloc(sc: &NvmeSoftc, size: usize) -> Option<&'static NvmeDmamem> {
    let dmat = sc.dmat();

    let mem = malloc(size_of::<NvmeDmamem>(), M_DEVBUF, M_WAITOK | M_ZERO)?;

    let map = match bus_dmamap_create(
        dmat,
        size,
        1,
        size,
        0,
        BUS_DMA_WAITOK | BUS_DMA_ALLOCNOW | BUS_DMA_64BIT,
    ) {
        Ok(map) => map,
        Err(_) => {
            // ndmfree:
            free(mem, M_DEVBUF, size_of::<NvmeDmamem>());
            return None;
        }
    };

    let mut segs = [BusDmaSegment::default(); 1];
    let destroy = |mem: NonNull<u8>| {
        // SAFETY: the map created above, unused.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
        free(mem, M_DEVBUF, size_of::<NvmeDmamem>());
    };
    let nsegs = match bus_dmamem_alloc(
        dmat,
        size,
        sc.sc_mps.get(),
        0,
        &mut segs,
        BUS_DMA_WAITOK | BUS_DMA_ZERO | BUS_DMA_64BIT,
    ) {
        Ok(n) => n,
        Err(_) => {
            destroy(mem);
            return None;
        }
    };

    let kva = match bus_dmamem_map(dmat, &mut segs[..nsegs], size, BUS_DMA_WAITOK) {
        Ok(kva) => kva,
        Err(_) => {
            // free:
            // SAFETY: the segment allocated above, not mapped.
            unsafe { bus_dmamem_free(dmat, &segs[..nsegs]) };
            destroy(mem);
            return None;
        }
    };

    // SAFETY: `kva` maps `size` bytes allocated above, which stay until nvme_dmamem_free
    // unloads the map first.
    if unsafe { bus_dmamap_load(dmat, map, kva.as_ptr(), size, None, BUS_DMA_WAITOK) }.is_err() {
        // unmap:
        // SAFETY: the mapping and the segment made above, unused.
        unsafe {
            bus_dmamem_unmap(dmat, kva, size);
            bus_dmamem_free(dmat, &segs[..nsegs]);
        }
        destroy(mem);
        return None;
    }

    let ndm = mem.cast::<NvmeDmamem>();
    // SAFETY: a fresh allocation of `size_of::<NvmeDmamem>()` bytes (malloc aligns it),
    // written whole before any use; it lives until nvme_dmamem_free.
    unsafe {
        ndm.as_ptr().write(NvmeDmamem {
            ndm_map: map,
            ndm_seg: segs[0],
            ndm_size: size,
            ndm_kva: kva,
        });
        Some(&*ndm.as_ptr())
    }
}

/// `nvme_dmamem_sync`: syncs the whole area.
pub fn nvme_dmamem_sync(sc: &NvmeSoftc, mem: &NvmeDmamem, ops: i32) {
    bus_dmamap_sync(sc.dmat(), nvme_dma_map(mem), 0, nvme_dma_len(mem), ops);
}

/// `nvme_dmamem_free`.
///
/// # Safety
///
/// `ndm` came from [`nvme_dmamem_alloc`], the controller no longer uses it and nothing
/// references it afterwards.
pub unsafe fn nvme_dmamem_free(sc: &NvmeSoftc, ndm: &'static NvmeDmamem) {
    let dmat = sc.dmat();

    bus_dmamap_unload(dmat, ndm.ndm_map);
    // SAFETY: the caller's guarantee: the area's own mapping, segment and map, unloaded.
    unsafe {
        bus_dmamem_unmap(dmat, ndm.ndm_kva, ndm.ndm_size);
        bus_dmamem_free(dmat, core::slice::from_ref(&ndm.ndm_seg));
        bus_dmamap_destroy(dmat, NonNull::from(ndm.ndm_map));
    }
    free(NonNull::from(ndm).cast(), M_DEVBUF, size_of::<NvmeDmamem>());
}

// HIBERNATE: not configured (nvme_hibernate_admin_cmd, nvme_hibernate_io).

/// `(struct T *)data`: a copy of the bio(4) argument structure (`EINVAL` when the bytes are
/// short).
fn nvme_bio_arg<T: NvmeBioArg>(addr: &[u8]) -> Result<T, Errno> {
    if addr.len() < size_of::<T>() {
        return Err(Errno::EINVAL);
    }
    // SAFETY: `addr` holds at least `size_of::<T>()` initialised bytes and every bit pattern
    // is a `T` (`NvmeBioArg`); the read is unaligned-safe.
    Ok(unsafe { ptr::read_unaligned(addr.as_ptr().cast::<T>()) })
}

/// Stores `bytes` at offset `off` of the ioctl's bytes (one member written back).
fn nvme_bio_put(addr: &mut [u8], off: usize, bytes: &[u8]) {
    if let Some(d) = addr.get_mut(off..off + bytes.len()) {
        d.copy_from_slice(bytes);
    }
}

/// The bytes of a `struct bio_status` (integers and byte arrays, no padding).
fn nvme_bio_status_bytes(bs: &BioStatus) -> &[u8] {
    // SAFETY: `BioStatus` is `#[repr(C)]` of byte arrays and `int`s with no padding (its
    // size is the sum of its members', asserted below), so all its bytes are initialised.
    unsafe { core::slice::from_raw_parts(ptr::from_ref(bs).cast::<u8>(), size_of::<BioStatus>()) }
}

/// `nvme_bioctl`: the controller's bio(4) entry point.
pub fn nvme_bioctl(self_: &Device, cmd: u64, data: &mut [u8]) -> Result<(), Errno> {
    // SAFETY: `nvme_attach` registered the controller's own device, the head of an
    // `NvmeSoftc` that lives while the device is attached.
    let sc: &'static NvmeSoftc = unsafe { &*ptr::from_ref(self_.softc::<NvmeSoftc>()) };

    rw_enter_write(&sc.sc_lock);

    let error = match cmd {
        BIOCINQ => nvme_bio_arg::<BiocInq>(data).map(|mut bi| {
            nvme_bioctl_inq(sc, &mut bi);
            nvme_bio_put(data, offset_of!(BiocInq, bi_dev), &bi.bi_dev);
            nvme_bio_put(
                data,
                offset_of!(BiocInq, bi_novol),
                &bi.bi_novol.to_ne_bytes(),
            );
            nvme_bio_put(
                data,
                offset_of!(BiocInq, bi_nodisk),
                &bi.bi_nodisk.to_ne_bytes(),
            );
            nvme_bio_put(
                data,
                offset_of!(BiocInq, bi_bio) + offset_of!(Bio, bio_status),
                nvme_bio_status_bytes(&bi.bi_bio.bio_status),
            );
        }),
        BIOCVOL => nvme_bio_arg::<BiocVol>(data).map(|mut bv| {
            nvme_bioctl_vol(sc, &mut bv);
            nvme_bio_put(
                data,
                offset_of!(BiocVol, bv_status),
                &bv.bv_status.to_ne_bytes(),
            );
            nvme_bio_put(
                data,
                offset_of!(BiocVol, bv_size),
                &bv.bv_size.to_ne_bytes(),
            );
            nvme_bio_put(
                data,
                offset_of!(BiocVol, bv_level),
                &bv.bv_level.to_ne_bytes(),
            );
            nvme_bio_put(
                data,
                offset_of!(BiocVol, bv_nodisk),
                &bv.bv_nodisk.to_ne_bytes(),
            );
            nvme_bio_put(data, offset_of!(BiocVol, bv_dev), &bv.bv_dev);
        }),
        BIOCDISK => nvme_bio_arg::<BiocDisk>(data).and_then(|mut bd| {
            let r = nvme_bioctl_disk(sc, &mut bd);
            nvme_bio_put(
                data,
                offset_of!(BiocDisk, bd_channel),
                &bd.bd_channel.to_ne_bytes(),
            );
            nvme_bio_put(
                data,
                offset_of!(BiocDisk, bd_target),
                &bd.bd_target.to_ne_bytes(),
            );
            nvme_bio_put(data, offset_of!(BiocDisk, bd_lun), &bd.bd_lun.to_ne_bytes());
            nvme_bio_put(
                data,
                offset_of!(BiocDisk, bd_status),
                &bd.bd_status.to_ne_bytes(),
            );
            nvme_bio_put(
                data,
                offset_of!(BiocDisk, bd_size),
                &bd.bd_size.to_ne_bytes(),
            );
            nvme_bio_put(data, offset_of!(BiocDisk, bd_serial), &bd.bd_serial);
            nvme_bio_put(data, offset_of!(BiocDisk, bd_procdev), &bd.bd_procdev);
            nvme_bio_put(
                data,
                offset_of!(BiocDisk, bd_bio) + offset_of!(Bio, bio_status),
                nvme_bio_status_bytes(&bd.bd_bio.bio_status),
            );
            r
        }),
        NVME_PASSTHROUGH_CMD => nvme_bio_arg::<NvmePtCmd>(data)
            .and_then(|pt| nvme_passthrough_cmd(sc, &pt, sc.sc_dev.dv_unit.get(), -1)),
        _ => {
            printf(format_args!("nvme_bioctl() Unknown command ({cmd})\n"));
            Err(Errno::ENOTTY)
        }
    };

    rw_exit_write(&sc.sc_lock);

    error
}

/// `nvme_bio_status`: an informational message for bioctl(8), not printed.
pub fn nvme_bio_status(bs: &mut BioStatus, args: core::fmt::Arguments<'_>) {
    bio_status(bs, false, BIO_MSG_INFO, args);
}

/// `nvme_bioctl_sdname`: the name of the `sd` of namespace `target`, unless it is going
/// away or the controller is gone.
pub fn nvme_bioctl_sdname(sc: &NvmeSoftc, target: i32) -> Option<&'static str> {
    let link = scsi_get_link(sc.sc_scsibus.get()?, target, 0)?;
    let sd = link.device_softc.get().map(|d| {
        // SAFETY: the device attached on a link of an nvme scsibus is an `sd`, whose softc
        // begins with its device; softcs live while attached.
        unsafe { &*d.as_ptr().cast::<SdSoftc>().cast_const() }
    });
    let sd = match sd {
        Some(sd) if link.state.get() & SDEV_S_DYING == 0 && sd.flags.get() & SDF_DYING == 0 => sd,
        _ => return None,
    };

    if nvme_read4(sc, NVME_VS) == 0xffff_ffff {
        return None;
    }

    Some(sd.sc_dev.xname())
}

/// `nvme_bioctl_inq`: BIOCINQ: the controller, its limits and its state.
pub fn nvme_bioctl_inq(sc: &NvmeSoftc, bi: &mut BiocInq) {
    let mut sn = [0u8; 41];
    let mut mn = [0u8; 81];
    let mut fr = [0u8; 17];
    let idctrl = sc.identify();

    // Don't tell bioctl about namespaces > last configured namespace.
    let mut nn = sc.sc_nn.get();
    while nn > 0 {
        if sc.ns_ident(nn as usize).is_some() {
            break;
        }
        nn -= 1;
    }
    bi.bi_novol = nn as i32;
    bi.bi_nodisk = nn as i32;
    strlcpy(&mut bi.bi_dev, sc.sc_dev.xname().as_bytes());

    let bs = &mut bi.bi_bio.bio_status;
    bio_status_init(bs, &sc.sc_dev);
    bs.bs_status = BIO_STATUS_SUCCESS;

    scsi_strvis(&mut sn, &idctrl.sn);
    scsi_strvis(&mut mn, &idctrl.mn);
    scsi_strvis(&mut fr, &idctrl.fr);

    nvme_bio_status(bs, format_args!("{}, {}, {}", Str(&mn), Str(&fr), Str(&sn)));
    nvme_bio_status(
        bs,
        format_args!(
            "Max i/o {} bytes{}{}{}, Sanitize 0x{}",
            sc.sc_mdts.get(),
            if idctrl.lpa & NVM_ID_CTRL_LPA_PE != 0 {
                ", Persistent Event Log"
            } else {
                ""
            },
            if idctrl.fna & NVM_ID_CTRL_FNA_CRYPTOFORMAT != 0 {
                ", CryptoFormat"
            } else {
                ""
            },
            if idctrl.vwc & NVM_ID_CTRL_VWC_PRESENT != 0 {
                ", Volatile Write Cache"
            } else {
                ""
            },
            Bitmask(
                u64::from(u32::from_le(idctrl.sanicap)),
                NVM_ID_CTRL_SANICAP_FMT
            )
        ),
    );

    if idctrl.ctratt != 0 {
        nvme_bio_status(
            bs,
            format_args!(
                "Features 0x{}",
                Bitmask(
                    u64::from(u32::from_le(idctrl.ctratt)),
                    NVM_ID_CTRL_CTRATT_FMT
                )
            ),
        );
    }

    if idctrl.oacs != 0 || idctrl.oncs != 0 {
        nvme_bio_status(
            bs,
            format_args!(
                "Admin commands 0x{}, NVM commands 0x{}",
                Bitmask(u64::from(u16::from_le(idctrl.oacs)), NVM_ID_CTRL_OACS_FMT),
                Bitmask(u64::from(u16::from_le(idctrl.oncs)), NVM_ID_CTRL_ONCS_FMT)
            ),
        );
    }

    let cc = nvme_read4(sc, NVME_CC);
    let csts = nvme_read4(sc, NVME_CSTS);
    let vs = nvme_read4(sc, NVME_VS);

    if vs == 0xffff_ffff {
        nvme_bio_status(bs, format_args!("Invalid PCIe register mapping"));
        return;
    }

    nvme_bio_status(
        bs,
        format_args!(
            "NVMe {}.{}{}{}{}abled, {}Ready{}{}{}{}",
            nvme_vs_mjr(vs),
            nvme_vs_mnr(vs),
            if nvme_cc_css_r(cc) == NVME_CC_CSS_NVM {
                ", NVM I/O command set"
            } else {
                ""
            },
            if nvme_cc_css_r(cc) == 0x7 {
                ", Admin command set only"
            } else {
                ""
            },
            if cc & NVME_CC_EN != 0 { ", En" } else { "Dis" },
            if csts & NVME_CSTS_RDY != 0 {
                ""
            } else {
                "Not "
            },
            if csts & NVME_CSTS_CFS != 0 {
                ", Fatal Error, "
            } else {
                ""
            },
            if nvme_cc_shn_r(cc) == NVME_CC_SHN_NORMAL {
                ", Normal shutdown"
            } else {
                ""
            },
            if nvme_cc_shn_r(cc) == NVME_CC_SHN_ABRUPT {
                ", Abrupt shutdown"
            } else {
                ""
            },
            if csts & NVME_CSTS_SHST_DONE != 0 {
                " complete"
            } else {
                ""
            }
        ),
    );
}

/// The index of a namespace's formatted LBA size, with the extended bits when it has more
/// than 16 formats.
fn nvme_ns_lbaf(idns: &NvmIdentifyNamespace, extended_over: usize) -> usize {
    let mut lbaf = usize::from(nvme_id_ns_flbas(idns.flbas));
    if usize::from(idns.nlbaf) > extended_over {
        lbaf |= usize::from((idns.flbas >> 1) & 0x3f);
    }
    lbaf
}

/// `1 << lbads` of LBA format `lbaf` (0 for a format past the 16, or a shift too large).
fn nvme_ns_lbads(idns: &NvmIdentifyNamespace, lbaf: usize) -> u32 {
    idns.lbaf.get(lbaf).map_or(0, |f| u32::from(f.lbads))
}

/// `nvme_bioctl_vol`: BIOCVOL: a namespace as a volume.
pub fn nvme_bioctl_vol(sc: &NvmeSoftc, bv: &mut BiocVol) {
    let target = bv.bv_volid.wrapping_add(1);
    if target as u32 > sc.sc_nn.get() {
        bv.bv_status = BIOC_SVINVALID;
        return;
    }

    bv.bv_level = i32::from(b'c');
    bv.bv_nodisk = 1;

    let Some(idns) = sc.ns_ident(target as usize) else {
        bv.bv_status = BIOC_SVINVALID;
        return;
    };

    let lbaf = nvme_ns_lbaf(idns, 16);
    bv.bv_size = nvme_scsi_size(idns)
        .checked_shl(nvme_ns_lbads(idns, lbaf))
        .unwrap_or(0);

    if let Some(sd) = nvme_bioctl_sdname(sc, target) {
        strlcpy(&mut bv.bv_dev, sd.as_bytes());
        bv.bv_status = BIOC_SVONLINE;
    } else {
        bv.bv_status = BIOC_SVOFFLINE;
    }
}

/// `nvme_bioctl_disk`: BIOCDISK: a namespace as a disk: formats, features, protection.
pub fn nvme_bioctl_disk(sc: &NvmeSoftc, bd: &mut BiocDisk) -> Result<(), Errno> {
    const RPDESC: [&str; 4] = [" (Best)", " (Better)", " (Good)", " (Degraded)"];
    const PROTECTION: [&str; 4] = ["not enabled", "Type 1", "Type 2", "Type 3"];
    let mut buf = [0u8; 32];
    let mut msg = [0u8; BIO_MSG_LEN];

    let target = bd.bd_volid.wrapping_add(1);
    if target as u32 > sc.sc_nn.get() {
        return Err(Errno::EINVAL);
    }
    bd.bd_channel = sc
        .sc_scsibus
        .get()
        .map_or(0, |sb| sb.sc_dev.dv_unit.get() as u16);
    bd.bd_target = target as u16;
    bd.bd_lun = 0;
    snprintf(
        &mut bd.bd_procdev,
        format_args!("Namespace {}", target as u32),
    );

    let bs = &mut bd.bd_bio.bio_status;
    bs.bs_status = BIO_STATUS_SUCCESS;
    snprintf(
        &mut bs.bs_controller,
        format_args!("{:11}", bd.bd_diskid as u32),
    );

    let Some(idns) = sc.ns_ident(target as usize) else {
        bd.bd_status = BIOC_SDUNUSED;
        return Ok(());
    };

    let lbaf = nvme_ns_lbaf(idns, idns.lbaf.len());
    bd.bd_size = u64::from_le(idns.nsze)
        .checked_shl(nvme_ns_lbads(idns, lbaf))
        .unwrap_or(0);

    if idns.nguid != [0u8; 16] {
        let id1 = u64::from_ne_bytes(idns.nguid[..8].try_into().unwrap_or([0; 8]));
        let id2 = u64::from_ne_bytes(idns.nguid[8..].try_into().unwrap_or([0; 8]));
        snprintf(&mut bd.bd_serial, format_args!("{id1:08x}{id2:08x}"));
    } else if idns.eui64 != [0u8; 8] {
        let id1 = u64::from_ne_bytes(idns.eui64);
        snprintf(&mut bd.bd_serial, format_args!("{id1:08x}"));
    }

    let strlen = |s: &[u8]| s.iter().position(|&c| c == 0).unwrap_or(s.len());
    for (i, f) in idns
        .lbaf
        .iter()
        .enumerate()
        .take(usize::from(idns.nlbaf) + 1)
    {
        if f.lbads == 0 {
            continue;
        }
        let n = snprintf(
            &mut buf,
            format_args!(
                "{}{}{}",
                if strlen(&msg) != 0 { ", " } else { "" },
                if i == lbaf { "*" } else { "" },
                1u32.checked_shl(u32::from(f.lbads)).unwrap_or(0)
            ),
        );
        strlcat(&mut msg, &buf[..n.min(buf.len() - 1)]);
        let ms = u16::from_le(f.ms);
        if ms != 0 {
            let n = snprintf(&mut buf, format_args!("+{ms}"));
            strlcat(&mut msg, &buf[..n.min(buf.len() - 1)]);
        }
        strlcat(
            &mut msg,
            RPDESC
                .get(usize::from(f.rp))
                .copied()
                .unwrap_or("")
                .as_bytes(),
        );
    }
    nvme_bio_status(bs, format_args!("Formats {}", Str(&msg)));

    if idns.nsfeat != 0 {
        nvme_bio_status(
            bs,
            format_args!(
                "Features 0x{}",
                Bitmask(u64::from(idns.nsfeat), NVME_ID_NS_NSFEAT_FMT)
            ),
        );
    }

    if idns.dps != 0 {
        let dps = idns.dps;
        snprintf(
            &mut msg,
            format_args!("Data Protection (0x{dps:02x}) Protection Data in "),
        );
        if dps & NVME_ID_NS_DPS_PIP != 0 {
            strlcat(&mut msg, b"first");
        } else {
            strlcat(&mut msg, b"last");
        }
        strlcat(&mut msg, b"bytes of metadata, Protection ");
        match PROTECTION.get(usize::from(nvme_id_ns_dps_type(dps))) {
            Some(p) => strlcat(&mut msg, p.as_bytes()),
            None => strlcat(&mut msg, b"Type unknown"),
        };
        nvme_bio_status(bs, format_args!("{}", Str(&msg)));
    }

    if nvme_bioctl_sdname(sc, target).is_none() {
        bd.bd_status = BIOC_SDOFFLINE;
    } else {
        bd.bd_status = BIOC_SDONLINE;
    }

    Ok(())
}

/// `nvme_refresh_sensors`: the sensor task: reads the SMART / Health log page into the
/// temperature, spare and wear sensors.
pub fn nvme_refresh_sensors(arg: *mut c_void) {
    // SAFETY: registered by nvme_attach with the softc as the argument; softcs are never
    // freed while their sensor task runs.
    let sc: &'static NvmeSoftc = unsafe { &*arg.cast::<NvmeSoftc>().cast_const() };
    let mut mem: Option<&'static NvmeDmamem> = None;
    let mut io: Option<ScsiIo> = None;

    'failed: {
        // SAFETY: the softc is the iopool's own cookie.
        let Some(got) = (unsafe { nvme_ccb_get(nvme_cookie(sc)) }) else {
            break 'failed;
        };
        io = Some(got);
        // SAFETY: from nvme_ccb_get.
        let ccb = unsafe { nvme_io_ccb(got) };

        let Some(m) = nvme_dmamem_alloc(sc, size_of::<NvmSmartHealth>()) else {
            break 'failed;
        };
        mem = Some(m);
        nvme_dmamem_sync(sc, m, BUS_DMASYNC_PREREAD);

        let dwlen = ((size_of::<NvmSmartHealth>() >> 2) - 1) as u32;
        let mut sqe = NvmeSqe::zeroed();
        sqe.opcode = NVM_ADMIN_GET_LOG_PG;
        sqe.nsid = 0xffff_ffffu32.to_le();
        sqe.cdw10 = ((dwlen << 16) | NVM_LOG_PAGE_SMART_HEALTH).to_le();
        sqe.entry.set_prp(0, nvme_dma_dva(m));

        ccb.ccb_done.set(nvme_empty_done);
        ccb.ccb_cookie.set(NvmeCookie::Sqe(sqe));
        let flags = nvme_poll(sc, sc.admin_q(), ccb, nvme_sqe_fill, NVME_TIMO_LOG_PAGE);

        nvme_dmamem_sync(sc, m, BUS_DMASYNC_POSTREAD);

        if flags != 0 {
            break 'failed;
        }

        // SAFETY: the DMA area holds the log page the controller wrote (synced above), as
        // large as the packed `NvmSmartHealth` (alignment 1), any bit pattern valid.
        let health = unsafe { &*nvme_dma_kva(m).cast::<NvmSmartHealth>() };
        let cw = health.critical_warning;

        sc.sc_temp_sensor
            .status
            .set(if cw & NVM_HEALTH_CW_TEMP != 0 {
                SENSOR_S_CRIT
            } else {
                SENSOR_S_OK
            });
        let temperature = health.temperature;
        let temp = i64::from(u16::from_le(temperature));
        sc.sc_temp_sensor.value.set((temp * 1_000_000) + 150_000);

        sc.sc_spare_sensor
            .status
            .set(if cw & NVM_HEALTH_CW_SPARE != 0 {
                SENSOR_S_CRIT
            } else {
                SENSOR_S_OK
            });
        sc.sc_spare_sensor
            .value
            .set(i64::from(health.avail_spare) * 1000);

        sc.sc_usage_sensor.status.set(SENSOR_S_OK);
        sc.sc_usage_sensor
            .value
            .set(i64::from(health.percent_used) * 1000);

        // done:
        if let Some(m) = mem {
            // SAFETY: allocated above, no longer used by the controller.
            unsafe { nvme_dmamem_free(sc, m) };
        }
        if let Some(io) = io {
            // SAFETY: the ccb taken above, finished.
            unsafe { nvme_ccb_put(nvme_cookie(sc), io) };
        }
        return;
    }

    // failed:
    sc.sc_temp_sensor.status.set(SENSOR_S_UNKNOWN);
    sc.sc_usage_sensor.status.set(SENSOR_S_UNKNOWN);
    sc.sc_spare_sensor.status.set(SENSOR_S_UNKNOWN);
    // done:
    if let Some(m) = mem {
        // SAFETY: allocated above, no longer used by the controller.
        unsafe { nvme_dmamem_free(sc, m) };
    }
    if let Some(io) = io {
        // SAFETY: the ccb taken above, finished.
        unsafe { nvme_ccb_put(nvme_cookie(sc), io) };
    }
}

const _: () = assert!(size_of::<BioStatus>() == 16 + 4 + 4 + 5 * (4 + BIO_MSG_LEN));
const _: () = assert!(size_of::<NvmeCqe>() == 16 && size_of::<NvmeSqe>() == 64);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::boxed::Box;

    use super::*;
    use crate::dev::ic::nvmereg::NvmNamespaceFormat;

    /// A zeroed namespace identify page on the heap (4 KiB).
    fn namespace() -> Box<NvmIdentifyNamespace> {
        // SAFETY: `NvmIdentifyNamespace` is integers and arrays of them: all-zero is valid.
        Box::new(unsafe { core::mem::zeroed() })
    }

    #[test]
    fn size_is_capacity_only_when_thin_and_smaller() {
        let mut ns = namespace();
        ns.nsze = 131_072u64.to_le();
        ns.ncap = 65_536u64.to_le();
        assert_eq!(nvme_scsi_size(&ns), 131_072);
        ns.nsfeat = NVME_ID_NS_NSFEAT_THIN_PROV;
        assert_eq!(nvme_scsi_size(&ns), 65_536);
        ns.ncap = 262_144u64.to_le();
        assert_eq!(nvme_scsi_size(&ns), 131_072);
    }

    #[test]
    fn read_capacity_replies() {
        let mut ns = namespace();
        ns.nsze = 131_072u64.to_le();
        ns.ncap = ns.nsze;
        ns.flbas = 1;
        ns.lbaf[1] = NvmNamespaceFormat {
            ms: 0,
            lbads: 12,
            rp: 0,
        };
        let rcd = nvme_read_cap_data(&ns);
        assert_eq!(rcd.addr, [0x00, 0x01, 0xff, 0xff]);
        assert_eq!(rcd.length, [0, 0, 0x10, 0]);
        let rcd16 = nvme_read_cap_data_16(&ns);
        assert_eq!(rcd16.addr, [0, 0, 0, 0, 0, 0x01, 0xff, 0xff]);
        assert_eq!(rcd16.length, [0, 0, 0x10, 0]);
        assert_eq!(rcd16.lowest_aligned, [0x80, 0x00]);

        // More than 2^32 blocks: READ CAPACITY (10) says 0xffffffff.
        ns.nsze = (1u64 << 33).to_le();
        assert_eq!(nvme_read_cap_data(&ns).addr, [0xff; 4]);
    }

    #[test]
    fn inquiry_names_the_model_and_firmware() {
        let mut mn = [b' '; 40];
        mn[..14].copy_from_slice(b"QEMU NVMe Ctrl");
        let fr = *b"11.1.0  ";
        let inq = nvme_inquiry_data(&mn, &fr);
        assert_eq!(inq.device, T_DIRECT);
        assert_eq!(inq.version, SCSI_REV_SPC4);
        assert_eq!(&inq.vendor, b"NVMe    ");
        assert_eq!(&inq.product, b"QEMU NVMe Ctrl  ");
        assert_eq!(&inq.revision, b"11.1");
        assert_eq!(inq.flags & SID_CmdQue, SID_CmdQue);
    }

    #[test]
    fn formatted_lba_size_index() {
        let mut ns = namespace();
        ns.flbas = 0x23;
        ns.nlbaf = 3;
        assert_eq!(nvme_ns_lbaf(&ns, 16), 3);
        ns.nlbaf = 17;
        assert_eq!(nvme_ns_lbaf(&ns, 16), 3 | 0x11);
        ns.lbaf[3].lbads = 9;
        assert_eq!(nvme_ns_lbads(&ns, 3), 9);
        assert_eq!(nvme_ns_lbads(&ns, 40), 0);
    }

    #[test]
    fn poll_state_marks_completion() {
        let mut st = NvmePollState {
            s: NvmeSqe::zeroed(),
            c: NvmeCqe::default(),
        };
        st.s.opcode = NVM_ADMIN_IDENTIFY;
        assert_eq!(st.c.flags & NVME_CQE_PHASE, 0);
        st.c.flags = (NVME_CQE_SC_SUCCESS) | NVME_CQE_PHASE;
        assert_eq!(st.c.flags & !NVME_CQE_PHASE, 0);
    }

    #[test]
    fn sqe_write_zeroes_a_long_slot() {
        let mut slot = [0xffu64; 16]; // a 128-byte slot (Apple T2)
        let mut sqe = NvmeSqe::zeroed();
        sqe.opcode = NVM_CMD_READ;
        sqe.cid = 7;
        // SAFETY: a 128-byte, 8-aligned buffer of our own.
        unsafe { nvme_sqe_write(slot.as_mut_ptr().cast(), 128, &sqe) };
        assert_eq!(slot[0] & 0xff, u64::from(NVM_CMD_READ));
        assert_eq!((slot[0] >> 16) & 0xffff, 7);
        assert!(slot[8..].iter().all(|&w| w == 0));
    }
}
/* </TESTS> */
