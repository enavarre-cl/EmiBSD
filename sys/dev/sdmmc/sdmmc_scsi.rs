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
 * Copyright (c) 2006 Uwe Stuehler <uwe@openbsd.org>
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
//! A SCSI adapter emulation to access SD/MMC memory cards: each card found in the slot is a
//! target of a `scsibus` (target 0 is the host); INQUIRY, READ CAPACITY and the no-op
//! commands are answered at once, READ and WRITE (6 and 10) are queued on the slot's task
//! thread, which runs them through `sdmmc_mem_read_block` and `sdmmc_mem_write_block`.
//!
//! Upstream: sys/dev/sdmmc/sdmmc_scsi.h @ 3ce1f3f79392
//! Upstream: sys/dev/sdmmc/sdmmc_scsi.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The bus emulation's softc is allocated whole (`M_ZERO`) and reached through the slot's
//!   `sc_scsibus` (the C's `void *`); its members are `Cell`s. The CCBs are written whole
//!   into their array; their `ccb_state` (the C's `volatile` enum) is a `Cell`, changed at
//!   `splbio`, as in C.
//! - `sdmmc_alloc_ccbs` returns `Result<(), Errno>` (`ENOMEM` for the C's 1);
//!   `sdmmc_ccb_alloc` and `sdmmc_ccb_free` are the pool's `unsafe` functions over its
//!   cookie, as in the other adapters.
//! - `sdmmc_scsi_cmd` checks the target against `sc_ntargets` before it indexes `sc_tgt`
//!   (the C computes the address first, then checks).
//! - A target without a card in `sdmmc_minphys`, `sdmmc_inquiry` or `sdmmc_complete_xs`
//!   panics where the C dereferences NULL (the bus only sends them for the cards' targets).
//! - Not configured, as in GENERIC: `SDMMC_DEBUG` (`DPRINTF`s are comments) and `HIBERNATE`
//!   (`sdmmc_scsi_hibernate_io` is not compiled, as in nvme.rs and ahci.rs).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::sdmmc::sdmmc::{sdmmc_add_task, sdmmc_del_task};
use crate::dev::sdmmc::sdmmc_mem::{sdmmc_mem_read_block, sdmmc_mem_write_block};
use crate::dev::sdmmc::sdmmcvar::{
    SMC_CAPS_NONREMOVABLE, SdmmcAttachArgs, SdmmcCommand, SdmmcFunction, SdmmcSoftc, SdmmcTask,
    sdmmc_init_task, sdmmc_task_pending,
};
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_physio::minphys;
use crate::kern::kern_rwlock::rw_assert_wrlock;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
use crate::kern::subr_autoconf::{config_detach, config_found};
use crate::kern::subr_prf::{panic, printf, snprintf};
use crate::machine::intr::{IPL_BIO, splbio, splx};
use crate::queue_adapter;
use crate::scsi::scsi_all::{
    INQUIRY, SI_EVPD, SID_REMOVABLE, SID_SCSI2_ALEN, SID_SCSI2_RESPONSE, START_STOP, ScsiInquiry,
    ScsiInquiryData, ScsiReadCapData, ScsiWire, T_DIRECT, TEST_UNIT_READY, wire_ref,
};
use crate::scsi::scsi_base::{scsi_copy_internal_data, scsi_done, scsi_iopool_init};
use crate::scsi::scsi_disk::{
    READ_10, READ_CAPACITY, READ_COMMAND, SRW_TOPADDR, SYNCHRONIZE_CACHE, ScsiRw, ScsiRw10,
    WRITE_10, WRITE_COMMAND,
};
use crate::scsi::scsiconf::{
    _2btol, _3btol, _4btol, _lto4b, SCSI_DATA_IN, SCSI_POLL, SCSI_REV_2, ScsiAdapter, ScsiIo,
    ScsiIopool, ScsiLink, ScsiXfer, ScsibusAttachArgs, XS_DRIVER_STUFFUP, XS_NOERROR, scsiprint,
};
use crate::sys::buf::Buf;
use crate::sys::device::{DETACH_FORCE, Device};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::mutex::Mutex;
use crate::sys::param::DEV_BSIZE;
use crate::sys::queue::{TailqEntry, TailqHead};
use libkern::strlcpy;

/// `SDMMC_SCSIID_HOST`.
pub const SDMMC_SCSIID_HOST: u16 = 0x00;
/// `SDMMC_SCSIID_MAX`.
pub const SDMMC_SCSIID_MAX: usize = 0x0f;

/// `SDMMC_SCSI_MAXCMDS`.
pub const SDMMC_SCSI_MAXCMDS: usize = 8;

/// `SDMMC_CCB_F_ERR`.
pub const SDMMC_CCB_F_ERR: i32 = 0x0001;

/// `struct sdmmc_scsi_target`.
pub struct SdmmcScsiTarget {
    /// `card`.
    pub card: Cell<Option<&'static SdmmcFunction>>,
}

/// The `ccb_state` of a CCB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdmmcCcbState {
    /// `SDMMC_CCB_FREE`.
    Free,
    /// `SDMMC_CCB_READY`.
    Ready,
    /// `SDMMC_CCB_QUEUED`.
    Queued,
}

/// `struct sdmmc_ccb`: a transfer's opening.
pub struct SdmmcCcb {
    /// `ccb_scbus`.
    pub ccb_scbus: Cell<*const SdmmcScsiSoftc>,
    /// `ccb_xs`.
    pub ccb_xs: Cell<Option<&'static ScsiXfer>>,
    /// `ccb_flags`: `SDMMC_CCB_F_*`.
    pub ccb_flags: Cell<i32>,
    /// `ccb_blockno`.
    pub ccb_blockno: Cell<u32>,
    /// `ccb_blockcnt`.
    pub ccb_blockcnt: Cell<u32>,
    /// `ccb_state`.
    pub ccb_state: Cell<SdmmcCcbState>,
    /// `ccb_cmd`.
    pub ccb_cmd: SdmmcCommand,
    /// `ccb_task`: runs the transfer on the slot's thread.
    pub ccb_task: SdmmcTask,
    /// `ccb_link`: the free or the run queue.
    pub ccb_link: TailqEntry<SdmmcCcb>,
}

impl SdmmcCcb {
    /// `ccb->ccb_xs`, set while the CCB carries a transfer.
    fn xs(&self) -> &'static ScsiXfer {
        match self.ccb_xs.get() {
            Some(xs) => xs,
            None => panic(format_args!("sdmmc: ccb without a transfer")),
        }
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(sdmmc_ccb_list, sdmmc_ccb)`, through `ccb_link`.
    pub SdmmcCcbList: SdmmcCcb, ccb_link => TailqEntry<SdmmcCcb>
);

/// `struct sdmmc_scsi_softc`.
pub struct SdmmcScsiSoftc {
    /// `sc_child`: the scsibus.
    pub sc_child: Cell<Option<NonNull<Device>>>,
    /// `sc_tgt`: `SDMMC_SCSIID_MAX + 1` targets.
    pub sc_tgt: Cell<*mut SdmmcScsiTarget>,
    /// `sc_ntargets`.
    pub sc_ntargets: Cell<i32>,
    /// `sc_ccbs`: allocated ccbs.
    pub sc_ccbs: Cell<*mut SdmmcCcb>,
    /// `sc_nccbs`.
    pub sc_nccbs: Cell<i32>,
    /// `sc_ccb_freeq`: free ccbs.
    pub sc_ccb_freeq: TailqHead<SdmmcCcbList>,
    /// `sc_ccb_runq`: queued ccbs.
    pub sc_ccb_runq: TailqHead<SdmmcCcbList>,
    /// `sc_ccb_mtx`: protects `sc_ccb_freeq`.
    pub sc_ccb_mtx: Mutex,
    /// `sc_iopool`.
    pub sc_iopool: ScsiIopool,
}

impl SdmmcScsiSoftc {
    /// `&scbus->sc_tgt[t]`, `t` at most `SDMMC_SCSIID_MAX`.
    fn tgt(&self, t: usize) -> &SdmmcScsiTarget {
        let tgts = self.sc_tgt.get();
        if tgts.is_null() || t > SDMMC_SCSIID_MAX {
            panic(format_args!("sdmmc: bad target {t}"));
        }
        // SAFETY: `sdmmc_scsi_attach` allocated `SDMMC_SCSIID_MAX + 1` zeroed targets (a
        // zeroed target has no card), freed only with the softc.
        unsafe { &*tgts.add(t) }
    }

    /// The card of target `t`.
    fn card(&self, t: usize) -> &'static SdmmcFunction {
        match self.tgt(t).card.get() {
            Some(card) => card,
            None => panic(format_args!("sdmmc: target {t} without a card")),
        }
    }
}

/// `sdmmc_switch`.
pub static SDMMC_SWITCH: ScsiAdapter = ScsiAdapter {
    scsi_cmd: sdmmc_scsi_cmd,
    dev_minphys: Some(sdmmc_minphys),
    dev_probe: None,
    dev_free: None,
    ioctl: None,
};

/// `link->bus->sb_adapter_softc`: the slot a link's bus belongs to.
fn sdmmc_link_softc(link: &ScsiLink) -> &'static SdmmcSoftc {
    let p = link.bus().sb_adapter_softc.get();
    if p.is_null() {
        panic(format_args!("sdmmc: bus without an adapter softc"));
    }
    // SAFETY: `sdmmc_scsi_attach` attaches its scsibus with the slot's softc as
    // `saa_adapter_softc`, and only that bus's links reach `sdmmc_switch`; softcs are never
    // freed while the device exists.
    unsafe { &*p.cast::<SdmmcSoftc>().cast_const() }
}

/// `sc->sc_scsibus`: the slot's bus emulation.
fn sdmmc_scbus(sc: &SdmmcSoftc) -> &'static SdmmcScsiSoftc {
    let p = sc.sc_scsibus.get();
    if p.is_null() {
        panic(format_args!("{}: no SCSI bus emulation", sc.devname()));
    }
    // SAFETY: `sdmmc_scsi_attach` stores its softc there before it attaches the bus and
    // clears it before it frees it, both under `sc_lock`; links only exist in between.
    unsafe { &*p.cast::<SdmmcScsiSoftc>().cast_const() }
}

/// The CCB a pool cookie and opening name.
///
/// # Safety
///
/// `xccb` is one of `sc_ccbs` (an opening of the bus's pool).
unsafe fn sdmmc_ccb_ref(xccb: *mut c_void) -> &'static SdmmcCcb {
    // SAFETY: the caller's guarantee; the CCB array lives as long as the bus emulation.
    unsafe { &*xccb.cast::<SdmmcCcb>().cast_const() }
}

/// The CCB a task or timeout argument names (`struct sdmmc_ccb *ccb = arg`).
fn sdmmc_ccb_arg(arg: *mut c_void) -> &'static SdmmcCcb {
    if arg.is_null() {
        panic(format_args!("sdmmc: task without its ccb"));
    }
    // SAFETY: the task and the timeout of a CCB are set up with the CCB as their argument
    // (`sdmmc_start_xs`), and `sdmmc_scsi_detach` passes a CCB of the run queue; the CCB
    // array lives as long as the bus emulation.
    unsafe { sdmmc_ccb_ref(arg) }
}

/// `sdmmc_scsi_attach`: attaches the SCSI bus emulation of the slot's memory cards.
pub fn sdmmc_scsi_attach(sc: &'static SdmmcSoftc) {
    rw_assert_wrlock(&sc.sc_lock);

    let Some(p) = malloc(size_of::<SdmmcScsiSoftc>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!(
            "sdmmc_scsi_attach: M_WAITOK allocation failed"
        ));
    };
    // SAFETY: a zeroed allocation of the softc's size; the all-zero softc is valid (empty
    // queues, an unused mutex and pool, `Cell`s of zero integers, null pointers and `None`).
    let scbus: &'static SdmmcScsiSoftc = unsafe { p.cast::<SdmmcScsiSoftc>().as_ref() };

    let tgtsize = size_of::<SdmmcScsiTarget>() * (SDMMC_SCSIID_MAX + 1);
    let Some(t) = mallocarray(
        size_of::<SdmmcScsiTarget>(),
        SDMMC_SCSIID_MAX + 1,
        M_DEVBUF,
        M_WAITOK | M_ZERO,
    ) else {
        panic(format_args!(
            "sdmmc_scsi_attach: M_WAITOK allocation failed"
        ));
    };
    scbus.sc_tgt.set(t.cast::<SdmmcScsiTarget>().as_ptr());

    // Each card that sent us a CID in the identification stage gets a SCSI ID > 0, whether
    // it is a memory card or not.
    scbus.sc_ntargets.set(1);
    for sf in sc.sf_head.iter() {
        if scbus.sc_ntargets.get() as usize > SDMMC_SCSIID_MAX {
            break;
        }
        scbus
            .tgt(scbus.sc_ntargets.get() as usize)
            .card
            .set(Some(sf));
        scbus.sc_ntargets.set(scbus.sc_ntargets.get() + 1);
    }

    'free_sctgt: {
        // Preallocate some CCBs and initialize the CCB lists.
        if sdmmc_alloc_ccbs(scbus, SDMMC_SCSI_MAXCMDS).is_err() {
            printf(format_args!("{}: can't allocate ccbs\n", sc.devname()));
            break 'free_sctgt;
        }

        sc.sc_scsibus
            .set(ptr::from_ref(scbus).cast_mut().cast::<c_void>());

        let mut saa = SdmmcAttachArgs {
            saa: ScsibusAttachArgs::new(),
            sf: None,
        };
        saa.saa.saa_adapter_target = SDMMC_SCSIID_HOST;
        saa.saa.saa_adapter_buswidth = scbus.sc_ntargets.get() as u16;
        saa.saa.saa_adapter_softc = ptr::from_ref(sc).cast_mut().cast();
        saa.saa.saa_luns = 1;
        saa.saa.saa_adapter = Some(&SDMMC_SWITCH);
        saa.saa.saa_openings = 1;
        saa.saa.saa_pool = Some(&scbus.sc_iopool);
        saa.saa.saa_quirks = 0;
        saa.saa.saa_flags = 0;
        saa.saa.saa_wwpn = 0;
        saa.saa.saa_wwnn = 0;

        scbus.sc_child.set(config_found(
            &sc.sc_dev,
            ptr::from_mut(&mut saa).cast(),
            Some(scsiprint),
        ));
        if scbus.sc_child.get().is_none() {
            printf(format_args!("{}: can't attach scsibus\n", sc.devname()));
            // free_ccbs:
            sc.sc_scsibus.set(ptr::null_mut());
            sdmmc_free_ccbs(scbus);
            break 'free_sctgt;
        }
        return;
    }

    // free_sctgt:
    free(t, M_DEVBUF, tgtsize);
    free(p, M_DEVBUF, size_of::<SdmmcScsiSoftc>());
}

/// `sdmmc_scsi_detach`: completes the open transfers, detaches the scsibus and frees the
/// bus emulation.
pub fn sdmmc_scsi_detach(sc: &'static SdmmcSoftc) {
    rw_assert_wrlock(&sc.sc_lock);

    if sc.sc_scsibus.get().is_null() {
        return;
    }
    let scbus = sdmmc_scbus(sc);

    // Complete all open scsi xfers.
    let s = splbio();
    while let Some(ccb) = scbus.sc_ccb_runq.first() {
        sdmmc_stimeout(ptr::from_ref(ccb).cast_mut().cast());
    }
    splx(s);

    if let Some(child) = scbus.sc_child.get() {
        // SAFETY: the scsibus `sdmmc_scsi_attach` attached, still attached.
        let _ = unsafe { config_detach(child, DETACH_FORCE) };
    }

    if let Some(t) = NonNull::new(scbus.sc_tgt.get()) {
        free(
            t.cast(),
            M_DEVBUF,
            size_of::<SdmmcScsiTarget>() * (SDMMC_SCSIID_MAX + 1),
        );
    }

    sdmmc_free_ccbs(scbus);
    free(
        NonNull::from(scbus).cast(),
        M_DEVBUF,
        size_of::<SdmmcScsiSoftc>(),
    );
    sc.sc_scsibus.set(ptr::null_mut());
}

/*
 * CCB management
 */

/// `sdmmc_alloc_ccbs`.
pub fn sdmmc_alloc_ccbs(scbus: &'static SdmmcScsiSoftc, nccbs: usize) -> Result<(), Errno> {
    let ccbs = mallocarray(nccbs, size_of::<SdmmcCcb>(), M_DEVBUF, M_NOWAIT)
        .ok_or(Errno::ENOMEM)?
        .cast::<SdmmcCcb>()
        .as_ptr();
    scbus.sc_ccbs.set(ccbs);
    scbus.sc_nccbs.set(nccbs as i32);

    scbus.sc_ccb_freeq.init();
    scbus.sc_ccb_runq.init();
    mtx_init(&scbus.sc_ccb_mtx, IPL_BIO);
    // SAFETY: sdmmc_ccb_alloc and sdmmc_ccb_free take this bus emulation as their cookie,
    // and it lives as long as its pool is used.
    unsafe {
        scsi_iopool_init(
            &scbus.sc_iopool,
            ptr::from_ref(scbus).cast_mut().cast(),
            sdmmc_ccb_alloc,
            sdmmc_ccb_free,
        )
    };

    for i in 0..nccbs {
        // SAFETY: `ccbs` has room for `nccbs` CCBs; each is written whole before it is
        // linked, and the array lives until `sdmmc_free_ccbs`.
        let ccb = unsafe {
            let p = ccbs.add(i);
            p.write(SdmmcCcb {
                ccb_scbus: Cell::new(ptr::from_ref(scbus)),
                ccb_xs: Cell::new(None),
                ccb_flags: Cell::new(0),
                ccb_blockno: Cell::new(0),
                ccb_blockcnt: Cell::new(0),
                ccb_state: Cell::new(SdmmcCcbState::Free),
                ccb_cmd: SdmmcCommand::new(),
                ccb_task: SdmmcTask::new(),
                ccb_link: TailqEntry::new(),
            });
            &*p
        };

        // SAFETY: the CCB is on no queue and stays in place while the bus emulation lives.
        unsafe { scbus.sc_ccb_freeq.insert_tail(ccb) };
    }
    Ok(())
}

/// `sdmmc_free_ccbs`.
pub fn sdmmc_free_ccbs(scbus: &SdmmcScsiSoftc) {
    if let Some(ccbs) = NonNull::new(scbus.sc_ccbs.get()) {
        free(
            ccbs.cast(),
            M_DEVBUF,
            scbus.sc_nccbs.get() as usize * size_of::<SdmmcCcb>(),
        );
        scbus.sc_ccbs.set(ptr::null_mut());
    }
}

/// `sdmmc_ccb_alloc`: the pool's `io_get`.
///
/// # Safety
///
/// `xscbus` is a live [`SdmmcScsiSoftc`] (the cookie `sdmmc_alloc_ccbs` gave the pool).
pub unsafe fn sdmmc_ccb_alloc(xscbus: *mut c_void) -> Option<ScsiIo> {
    // SAFETY: the caller's guarantee.
    let scbus = unsafe { &*xscbus.cast::<SdmmcScsiSoftc>().cast_const() };

    mtx_enter(&scbus.sc_ccb_mtx);
    let ccb = scbus.sc_ccb_freeq.first();
    if let Some(ccb) = ccb {
        // SAFETY: the CCB is on the free queue (its first element).
        unsafe { scbus.sc_ccb_freeq.remove(ccb) };
        ccb.ccb_state.set(SdmmcCcbState::Ready);
    }
    mtx_leave(&scbus.sc_ccb_mtx);

    ccb.map(|c| NonNull::from(c).cast())
}

/// `sdmmc_ccb_free`: the pool's `io_put`.
///
/// # Safety
///
/// `xscbus` is a live [`SdmmcScsiSoftc`] and `xccb` one of its CCBs from
/// [`sdmmc_ccb_alloc`], not on the free queue.
pub unsafe fn sdmmc_ccb_free(xscbus: *mut c_void, xccb: ScsiIo) {
    // SAFETY: the caller's guarantee.
    let scbus = unsafe { &*xscbus.cast::<SdmmcScsiSoftc>().cast_const() };
    // SAFETY: the caller's guarantee.
    let ccb = unsafe { sdmmc_ccb_ref(xccb.as_ptr()) };

    let s = splbio();
    if ccb.ccb_state.get() == SdmmcCcbState::Queued {
        // SAFETY: a queued CCB is on the run queue (`sdmmc_start_xs`).
        unsafe { scbus.sc_ccb_runq.remove(ccb) };
    }
    splx(s);

    ccb.ccb_state.set(SdmmcCcbState::Free);
    ccb.ccb_flags.set(0);
    ccb.ccb_xs.set(None);

    mtx_enter(&scbus.sc_ccb_mtx);
    // SAFETY: the CCB is on no queue now and stays in place while the bus emulation lives.
    unsafe { scbus.sc_ccb_freeq.insert_tail(ccb) };
    mtx_leave(&scbus.sc_ccb_mtx);
}

/*
 * SCSI command emulation
 */

/// `sdmmc_scsi_decode_rw`: the block number and count of a READ or WRITE (6 or 10).
/// XXX move to some sort of "scsi emulation layer".
pub fn sdmmc_scsi_decode_rw(xs: &ScsiXfer) -> (u32, u32) {
    let cmd = xs.cmd.get();
    if xs.cmdlen.get() == 6 {
        let rw = wire_ref::<ScsiRw>(cmd.as_bytes());
        let blockno = _3btol(&rw.addr) & ((u32::from(SRW_TOPADDR) << 16) | 0xffff);
        let blockcnt = if rw.length != 0 {
            u32::from(rw.length)
        } else {
            0x100
        };
        (blockno, blockcnt)
    } else {
        let rw10 = wire_ref::<ScsiRw10>(cmd.as_bytes());
        (_4btol(&rw10.addr), _2btol(&rw10.length))
    }
}

/// `sdmmc_scsi_cmd`: the adapter's `scsi_cmd`.
pub fn sdmmc_scsi_cmd(xs: &'static ScsiXfer) {
    let link = xs.link();
    let sc = sdmmc_link_softc(link);
    let scbus = sdmmc_scbus(sc);
    let target = usize::from(link.target.get());

    if target >= scbus.sc_ntargets.get() as usize
        || scbus.tgt(target).card.get().is_none()
        || link.lun.get() != 0
    {
        // DPRINTF(("%s: sdmmc_scsi_cmd: no target %d\n", DEVNAME(sc), link->target));
        // XXX should be XS_SENSE and sense filled out
        xs.error.set(XS_DRIVER_STUFFUP);
        scsi_done(xs);
        return;
    }
    let card = scbus.card(target);

    // DPRINTF(("%s: scsi cmd target=%d opcode=%#x proc=\"%s\" (poll=%#x)\n", ...));

    xs.error.set(XS_NOERROR);

    match xs.cmd.get().opcode {
        READ_COMMAND | READ_10 | WRITE_COMMAND | WRITE_10 => {
            // Deal with I/O outside the switch.
        }

        INQUIRY => {
            sdmmc_inquiry(xs);
            return;
        }

        TEST_UNIT_READY | START_STOP | SYNCHRONIZE_CACHE => {
            scsi_done(xs);
            return;
        }

        READ_CAPACITY => {
            let rcd = sdmmc_read_cap_data(card.csd.get().capacity, card.csd.get().sector_size);
            // SAFETY: the adapter owns the transfer between `scsi_cmd` and `scsi_done`, and
            // holds no other slice of its data.
            let data = unsafe { xs.data_slice() };
            let n = data.len().min(size_of::<ScsiReadCapData>());
            data[..n].copy_from_slice(&rcd.as_bytes()[..n]);
            scsi_done(xs);
            return;
        }

        _ => {
            // DPRINTF(("%s: unsupported scsi command %#x\n", DEVNAME(sc), xs->cmd.opcode));
            xs.error.set(XS_DRIVER_STUFFUP);
            scsi_done(xs);
            return;
        }
    }

    // A read or write operation.
    let (blockno, blockcnt) = sdmmc_scsi_decode_rw(xs);

    let capacity = card.csd.get().capacity as u32;
    if blockno >= capacity || blockno.wrapping_add(blockcnt) > capacity {
        // DPRINTF(("%s: out of bounds %u-%u >= %u\n", DEVNAME(sc), blockno, blockcnt,
        //     tgt->card->csd.capacity));
        xs.error.set(XS_DRIVER_STUFFUP);
        scsi_done(xs);
        return;
    }

    let Some(io) = xs.io.get() else {
        panic(format_args!("sdmmc_scsi_cmd: xs {:p} without a ccb", xs));
    };
    // SAFETY: the transfer's opening came from this bus's pool (`sdmmc_ccb_alloc`).
    let ccb = unsafe { sdmmc_ccb_ref(io.as_ptr()) };

    ccb.ccb_xs.set(Some(xs));
    ccb.ccb_blockcnt.set(blockcnt);
    ccb.ccb_blockno.set(blockno);

    sdmmc_start_xs(sc, ccb);
}

/// The READ CAPACITY reply: the last sector and the sector size.
pub fn sdmmc_read_cap_data(capacity: i32, sector_size: i32) -> ScsiReadCapData {
    let mut rcd = ScsiReadCapData::zeroed();
    _lto4b(capacity.wrapping_sub(1) as u32, &mut rcd.addr);
    _lto4b(sector_size as u32, &mut rcd.length);
    rcd
}

/// The INQUIRY reply for a card of manufacturer `mid`, product name `pnm` and revision
/// `rev`.
pub fn sdmmc_inquiry_data(mid: i32, pnm: &[u8; 8], rev: i32, removable: bool) -> ScsiInquiryData {
    let mut vendor = [0u8; 8 + 1];
    let mut product = [0u8; 16 + 1];
    let mut revision = [0u8; 4 + 1];

    let v: &[u8] = match mid {
        0x02 | 0x03 | 0x45 => b"Sandisk\0",
        0x11 => b"Toshiba\0",
        0x13 => b"Micron\0",
        0x15 => b"Samsung\0",
        0x27 => b"Apacer\0",
        0x70 => b"Kingston\0",
        0x90 => b"Hynix\0",
        _ => b"SD/MMC\0",
    };
    strlcpy(&mut vendor, v);
    strlcpy(&mut product, pnm);
    snprintf(&mut revision, format_args!("{:04X}", rev as u32));

    let mut inq = ScsiInquiryData::zeroed();
    inq.device = T_DIRECT;
    if removable {
        inq.dev_qual2 = SID_REMOVABLE;
    }
    inq.version = SCSI_REV_2;
    inq.response_format = SID_SCSI2_RESPONSE;
    inq.additional_length = SID_SCSI2_ALEN as u8;
    inq.vendor.copy_from_slice(&vendor[..8]);
    inq.product.copy_from_slice(&product[..16]);
    inq.revision.copy_from_slice(&revision[..4]);
    inq
}

/// `sdmmc_inquiry`: answers INQUIRY (not the vital product data pages).
pub fn sdmmc_inquiry(xs: &'static ScsiXfer) {
    let link = xs.link();
    let sc = sdmmc_link_softc(link);
    let scbus = sdmmc_scbus(sc);

    'done: {
        if xs.cmdlen.get() != size_of::<ScsiInquiry>() as i32 {
            xs.error.set(XS_DRIVER_STUFFUP);
            break 'done;
        }

        let cdb = xs.cmd_as::<ScsiInquiry>();
        if cdb.flags & SI_EVPD != 0 {
            xs.error.set(XS_DRIVER_STUFFUP);
            break 'done;
        }

        let cid = scbus.card(usize::from(link.target.get())).cid.get();
        let inq = sdmmc_inquiry_data(
            cid.mid,
            &cid.pnm,
            cid.rev,
            !sc.has_caps(SMC_CAPS_NONREMOVABLE),
        );

        scsi_copy_internal_data(xs, inq.as_bytes());
    }

    // done:
    scsi_done(xs);
}

/// `sdmmc_start_xs`: queues a read or write on the slot's thread (runs it at once when
/// polled).
pub fn sdmmc_start_xs(sc: &'static SdmmcSoftc, ccb: &'static SdmmcCcb) {
    let scbus = sdmmc_scbus(sc);
    let xs = ccb.xs();
    let arg = ptr::from_ref(ccb).cast_mut().cast::<c_void>();

    timeout_set(&xs.stimeout, sdmmc_stimeout, arg);
    sdmmc_init_task(&ccb.ccb_task, sdmmc_complete_xs, arg);

    let s = splbio();
    // SAFETY: the CCB is off the free queue (READY) and on no other; it stays in place
    // while the bus emulation lives.
    unsafe { scbus.sc_ccb_runq.insert_tail(ccb) };
    ccb.ccb_state.set(SdmmcCcbState::Queued);
    splx(s);

    if xs.flags.get() & SCSI_POLL != 0 {
        sdmmc_complete_xs(arg);
        return;
    }

    timeout_add_msec(&xs.stimeout, xs.timeout.get() as u64);
    sdmmc_add_task(sc, &ccb.ccb_task);
}

/// `sdmmc_complete_xs`: runs a queued read or write.
pub fn sdmmc_complete_xs(arg: *mut c_void) {
    let ccb = sdmmc_ccb_arg(arg);
    let xs = ccb.xs();
    let link = xs.link();
    let sc = sdmmc_link_softc(link);
    let scbus = sdmmc_scbus(sc);
    let card = scbus.card(usize::from(link.target.get()));

    // DPRINTF(("%s: scsi cmd target=%d opcode=%#x proc=\"%s\" (poll=%#x) complete\n", ...));

    let s = splbio();

    let len = ccb.ccb_blockcnt.get() as usize * DEV_BSIZE;
    let error = if xs.flags.get() & SCSI_DATA_IN != 0 {
        // SAFETY: the transfer's data holds the `blockcnt` sectors the midlayer asked for,
        // and the adapter owns it until `scsi_done`.
        unsafe { sdmmc_mem_read_block(card, ccb.ccb_blockno.get() as i32, xs.data(), len) }
    } else {
        // SAFETY: as above.
        unsafe { sdmmc_mem_write_block(card, ccb.ccb_blockno.get() as i32, xs.data(), len) }
    };

    if error.is_err() {
        xs.error.set(XS_DRIVER_STUFFUP);
    }

    sdmmc_done_xs(ccb);
    splx(s);
}

/// `sdmmc_done_xs`: completes a CCB's transfer.
pub fn sdmmc_done_xs(ccb: &SdmmcCcb) {
    let xs = ccb.xs();

    timeout_del(&xs.stimeout);

    // DPRINTF(("%s: scsi cmd target=%d opcode=%#x proc=\"%s\" (error=%#x) done\n", ...));

    xs.resid.set(0);

    if ccb.ccb_flags.get() & SDMMC_CCB_F_ERR != 0 {
        xs.error.set(XS_DRIVER_STUFFUP);
    }

    scsi_done(xs);
}

/// `sdmmc_stimeout`: a queued transfer timed out (or the bus is going away).
pub fn sdmmc_stimeout(arg: *mut c_void) {
    let ccb = sdmmc_ccb_arg(arg);

    let s = splbio();
    ccb.ccb_flags.set(ccb.ccb_flags.get() | SDMMC_CCB_F_ERR);
    if sdmmc_task_pending(&ccb.ccb_task) {
        sdmmc_del_task(&ccb.ccb_task);
        sdmmc_done_xs(ccb);
    }
    splx(s);
}

/// `sdmmc_minphys`: limits a transfer to the maximum transfer size supported by the
/// card/host.
pub fn sdmmc_minphys(bp: &Buf, sl: &'static ScsiLink) {
    let sc = sdmmc_link_softc(sl);
    let scbus = sdmmc_scbus(sc);
    let sf = scbus.card(usize::from(sl.target.get()));

    // limit to max. transfer size supported by card/host
    let limit = i64::from(sf.csd.get().sector_size) * sc.sc_max_xfer.get();
    if sc.sc_max_xfer.get() != 0 && bp.b_bcount.get() > limit {
        bp.b_bcount.set(limit);
    } else {
        minphys(bp);
    }
}

// HIBERNATE: not configured (sdmmc_scsi_hibernate_io).
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inquiry_reply_of_qemus_card() {
        // QEMU's CID: MID 0xaa, PNM "QEMU!", PRV 0x01: "<SD/MMC, QEMU!, 0001> removable".
        let mut pnm = [0u8; 8];
        pnm[..5].copy_from_slice(b"QEMU!");
        let inq = sdmmc_inquiry_data(0xaa, &pnm, 1, true);
        assert_eq!(&inq.vendor, b"SD/MMC\0\0");
        assert_eq!(&inq.product[..6], b"QEMU!\0");
        assert!(inq.product[5..].iter().all(|&c| c == 0));
        assert_eq!(&inq.revision, b"0001");
        assert_eq!(inq.dev_qual2, SID_REMOVABLE);
        assert_eq!(inq.version, SCSI_REV_2);
        assert_eq!(inq.additional_length, 31);
        // A Sandisk card, not removable; a revision of more than four digits is cut.
        let inq = sdmmc_inquiry_data(0x45, &pnm, 0x12345, false);
        assert_eq!(&inq.vendor, b"Sandisk\0");
        assert_eq!(&inq.revision, b"1234");
        assert_eq!(inq.dev_qual2, 0);
    }

    #[test]
    fn read_capacity_reply() {
        let rcd = sdmmc_read_cap_data(131072, 512);
        assert_eq!(rcd.as_bytes(), &[0, 1, 0xff, 0xff, 0, 0, 2, 0]);
    }
}
/* </TESTS> */
