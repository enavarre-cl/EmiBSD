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
//! `sdmmc(4)`: the host controller independent SD/MMC bus driver, based on information from
//! SanDisk SD Card Product Manual Revision 2.2 (SanDisk), SDIO Simple Specification Version
//! 1.0 (SDIO) and the Linux "mmc" driver.
//!
//! Upstream: sys/dev/sdmmc/sdmmc.c @ 3ce1f3f79392
//!
//! One `sdmmc` attaches per card slot, below its host controller (`sdmmc* at sdhc?`). Attach
//! starts a kernel thread that runs the slot's tasks: the discovery task powers the card up,
//! identifies its I/O functions and memory cards (`sdmmc_io.rs`, `sdmmc_mem.rs`) and attaches
//! the SCSI emulation for memory (`sdmmc_scsi.rs`) and the drivers of the I/O functions; the
//! SCSI emulation queues its transfers on the same thread. Every command goes through the
//! chip's `exec_command` under the slot's `sc_lock`.
//!
//! ## Deviations
//! - The C's 0/1 returns (`sdmmc_enable`, `sdmmc_scan`, `sdmmc_init`, `sdmmc_set_bus_power`,
//!   `sdmmc_send_if_cond`, `sdmmc_set_relative_addr`) are `Result<(), Errno>`, with `EIO`
//!   for the bare 1; the errno returns are `Result<(), Errno>` too.
//! - `sdmmc_function_alloc` writes the whole function into its `M_ZERO` allocation and
//!   returns it as `&'static`; `sdmmc_function_free` is `unsafe` (nothing may use the
//!   function after it).
//! - `sdmmc_create_thread` stores the new thread in `sc_task_thread` when `kthread_create`
//!   returns, where the C's `kthread_create` writes it through its last argument.
//! - A task on the queue without a function panics where the C would call address zero.
//! - Not configured, as in GENERIC: `SDMMC_IOCTL` (`sdmmc_ioctl` and its `bio_register` are
//!   not compiled) and `SDMMC_DEBUG` (`sdmmc_dump_command` is not compiled; `DPRINTF`s are
//!   comments).

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::conf::param::TICK;
use crate::dev::sdmmc::sdmmc_io::{sdmmc_intr_task, sdmmc_io_attach, sdmmc_io_detach};
use crate::dev::sdmmc::sdmmc_io::{sdmmc_io_enable, sdmmc_io_init, sdmmc_io_scan};
use crate::dev::sdmmc::sdmmc_mem::{sdmmc_mem_enable, sdmmc_mem_init, sdmmc_mem_scan};
use crate::dev::sdmmc::sdmmc_scsi::{sdmmc_scsi_attach, sdmmc_scsi_detach};
use crate::dev::sdmmc::sdmmcchip::{
    SDMMC_SDCLK_400KHZ, SDMMC_SDCLK_OFF, SDMMC_TIMING_LEGACY, SdmmcbusAttachArgs,
    sdmmc_chip_bus_clock, sdmmc_chip_bus_power, sdmmc_chip_card_detect, sdmmc_chip_exec_command,
    sdmmc_chip_host_maxblklen, sdmmc_chip_host_ocr,
};
use crate::dev::sdmmc::sdmmcreg::{
    MMC_APP_CMD, MMC_GO_IDLE_STATE, MMC_R1_APP_CMD, MMC_SELECT_CARD, MMC_SET_RELATIVE_ADDR,
    SD_OCR_VOL_MASK, SD_SEND_IF_COND, SD_SEND_RELATIVE_ADDR, mmc_arg_rca, mmc_r1, sd_r6_rca,
};
use crate::dev::sdmmc::sdmmcvar::{
    SCF_CMD_AC, SCF_CMD_BC, SCF_CMD_BCR, SCF_RSP_R0, SCF_RSP_R1, SCF_RSP_R6, SCF_RSP_R7,
    SDMMC_FUNCTION_INVALID, SDMMC_MAXNSEGS, SDMMC_PRODUCT_INVALID, SDMMC_VENDOR_INVALID, SFF_ERROR,
    SMC_CAPS_4BIT_MODE, SMC_CAPS_8BIT_MODE, SMC_CAPS_DMA, SMC_CAPS_MMC_DDR52,
    SMC_CAPS_MMC_HIGHSPEED, SMC_CAPS_MMC_HS200, SMC_CAPS_NONREMOVABLE, SMC_CAPS_SD_HIGHSPEED,
    SMC_CAPS_UHS_SDR50, SMC_CAPS_UHS_SDR104, SMF_CARD_ATTACHED, SMF_CARD_PRESENT,
    SMF_CONFIG_PENDING, SMF_IO_MODE, SMF_MEM_MODE, SMF_SD_MODE, SdmmcCid, SdmmcCis, SdmmcCommand,
    SdmmcCsd, SdmmcFunction, SdmmcScr, SdmmcSoftc, SdmmcTask, sdmmc_init_task, sdmmc_task_pending,
    splsdmmc,
};
use crate::kern::kern_kthread::{kthread_create, kthread_create_deferred, kthread_exit};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{rw_assert_wrlock, rw_enter_write, rw_exit, rw_init};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_autoconf::{
    config_activate_children, config_pending_decr, config_pending_incr,
};
use crate::kern::subr_disk::ROOTDV;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_ALLOCNOW, BUS_DMA_NOWAIT, BusSize, bus_dmamap_create, bus_dmamap_destroy,
};
use crate::machine::cpu::delay;
use crate::machine::intr::splx;
use crate::sys::device::{
    CfMatch, Cfattach, Cfdriver, DETACH_FORCE, DV_DULL, DVACT_RESUME, DVACT_SUSPEND, Device,
};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::param::{MAXPHYS, PWAIT};
use crate::sys::queue::SimpleqEntry;
use crate::sys::systm::{COLD, INFSLP};
use crate::sys::time::usec_to_nsec;

/// `sdmmc_ca`.
pub static SDMMC_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<SdmmcSoftc>(),
    ca_match: Some(sdmmc_match),
    ca_attach: sdmmc_attach,
    ca_detach: Some(sdmmc_detach),
    ca_activate: Some(sdmmc_activate),
};

/// `sdmmc_cd`.
pub static SDMMC_CD: Cfdriver = Cfdriver::new(b"sdmmc", DV_DULL, 0);

/// `(struct sdmmc_softc *)self` for a device made for `sdmmc_ca`.
fn sdmmc_softc(self_: &Device) -> &'static SdmmcSoftc {
    // SAFETY: only devices made for `sdmmc_ca` reach here (the bus's own functions and the
    // host controller's `hp->sdmmc`), whose softc is an `SdmmcSoftc`; softcs are never freed
    // while the device exists, so the softc may be borrowed for 'static.
    unsafe { &*ptr::from_ref(self_.softc::<SdmmcSoftc>()) }
}

/// The slot a task argument names (`struct sdmmc_softc *sc = arg`).
fn sdmmc_softc_arg(arg: *mut c_void) -> &'static SdmmcSoftc {
    if arg.is_null() {
        panic(format_args!("sdmmc: task without its softc"));
    }
    // SAFETY: the slot's tasks and thread are set up with the softc as their argument
    // (`sdmmc_attach`), and softcs are never freed while the device exists.
    unsafe { &*arg.cast::<SdmmcSoftc>().cast_const() }
}

/// `sdmmc_match`.
pub fn sdmmc_match(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();
    // SAFETY: the `sdmmcbus` attribute's parents attach their children with a
    // `struct sdmmcbus_attach_args`.
    let saa = unsafe { &*aux.cast::<SdmmcbusAttachArgs>().cast_const() };

    i32::from(saa.saa_busname == cf.cf_driver.cd_name)
}

/// `sdmmc_attach`.
pub fn sdmmc_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = sdmmc_softc(self_);
    // SAFETY: as in `sdmmc_match`; the parent owns the arguments for the attach.
    let saa = unsafe { &*aux.cast::<SdmmcbusAttachArgs>().cast_const() };

    if saa.caps & SMC_CAPS_8BIT_MODE != 0 {
        printf(format_args!(": 8-bit"));
    } else if saa.caps & SMC_CAPS_4BIT_MODE != 0 {
        printf(format_args!(": 4-bit"));
    } else {
        printf(format_args!(": 1-bit"));
    }
    if saa.caps & SMC_CAPS_SD_HIGHSPEED != 0 {
        printf(format_args!(", sd high-speed"));
    }
    if saa.caps & SMC_CAPS_UHS_SDR50 != 0 {
        printf(format_args!(", sdr50"));
    }
    if saa.caps & SMC_CAPS_UHS_SDR104 != 0 {
        printf(format_args!(", sdr104"));
    }
    if saa.caps & SMC_CAPS_MMC_HIGHSPEED != 0 {
        printf(format_args!(", mmc high-speed"));
    }
    if saa.caps & SMC_CAPS_MMC_DDR52 != 0 {
        printf(format_args!(", ddr52"));
    }
    if saa.caps & SMC_CAPS_MMC_HS200 != 0 {
        printf(format_args!(", hs200"));
    }
    if saa.caps & SMC_CAPS_DMA != 0 {
        printf(format_args!(", dma"));
    }
    printf(format_args!("\n"));

    sc.sct.set(saa.sct);
    sc.sch.set(saa.sch);
    sc.sc_dmat.set(saa.dmat);
    sc.sc_dmap.set(saa.dmap);
    sc.sc_flags.set(saa.flags);
    sc.sc_caps.set(saa.caps);
    sc.sc_max_seg.set(if saa.max_seg != 0 {
        saa.max_seg
    } else {
        MAXPHYS as i64
    });
    sc.sc_max_xfer.set(saa.max_xfer);
    sc.sc_cookies.set(saa.cookies);

    if sc.has_caps(SMC_CAPS_DMA) && sc.sc_dmap.get().is_none() {
        match bus_dmamap_create(
            sc.dmat(),
            MAXPHYS as BusSize,
            SDMMC_MAXNSEGS as i32,
            sc.sc_max_seg.get() as BusSize,
            saa.dma_boundary,
            BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW,
        ) {
            Ok(map) => sc.sc_dmap.set(Some(map)),
            Err(_) => {
                printf(format_args!("{}: can't create DMA map\n", sc.devname()));
                return;
            }
        }
    }

    sc.sf_head.init();
    sc.sc_tskq.init();
    sc.sc_intrq.init();
    let arg = ptr::from_ref(sc).cast_mut().cast::<c_void>();
    sdmmc_init_task(&sc.sc_discover_task, sdmmc_discover_task, arg);
    sdmmc_init_task(&sc.sc_intr_task, sdmmc_intr_task, arg);
    rw_init(&sc.sc_lock, sc.devname());

    // SDMMC_IOCTL is not configured: no bio_register(self, sdmmc_ioctl).

    // Create the event thread that will attach and detach cards and perform other lengthy
    // operations. Enter config_pending state until the discovery task has run for the
    // first time.
    sc.set_flag(SMF_CONFIG_PENDING);
    config_pending_incr();
    kthread_create_deferred(sdmmc_create_thread, arg);
}

/// `sdmmc_detach`.
pub fn sdmmc_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = sdmmc_softc(self_);

    sc.sc_dying.set(1);
    while sc.sc_task_thread.get().is_some() {
        wakeup(ptr::from_ref(&sc.sc_tskq));
        let _ = tsleep_nsec(ptr::from_ref(sc), PWAIT, "mmcdie", INFSLP);
    }

    if let Some(map) = sc.sc_dmap.get() {
        // SAFETY: the map attach created; the task thread that used it is gone.
        unsafe { bus_dmamap_destroy(sc.dmat(), NonNull::from(map)) };
    }

    Ok(())
}

/// `sdmmc_activate`.
pub fn sdmmc_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    let sc = sdmmc_softc(self_);

    match act {
        DVACT_SUSPEND => {
            let rv = config_activate_children(self_, act);
            // If card in slot, cause a detach/re-attach
            if sc.has_flag(SMF_CARD_PRESENT)
                && !sc.has_caps(SMC_CAPS_NONREMOVABLE)
                && !sdmmc_holds_root_device(sc)
            {
                sc.sc_dying.set(-1);
            }
            rv
        }
        DVACT_RESUME => {
            wakeup(ptr::from_ref(&sc.sc_tskq));
            config_activate_children(self_, act)
        }
        _ => config_activate_children(self_, act),
    }
}

/// `sdmmc_holds_root_device`: is the root disk a child of this slot's SCSI bus?
pub fn sdmmc_holds_root_device(sc: &SdmmcSoftc) -> bool {
    let rootdv = ROOTDV.load(Ordering::Relaxed);
    // SAFETY: `rootdv` is set once to an attached device that is never freed.
    let Some(rootdv) = (unsafe { rootdv.as_ref() }) else {
        return false;
    };
    rootdv
        .parent()
        .and_then(Device::parent)
        .is_some_and(|d| ptr::eq(d, &sc.sc_dev))
}

/// `sdmmc_create_thread`.
pub fn sdmmc_create_thread(arg: *mut c_void) {
    let sc = sdmmc_softc_arg(arg);

    match kthread_create(sdmmc_task_thread, arg, sc.devname().as_bytes()) {
        Ok(p) => sc.sc_task_thread.set(Some(p)),
        Err(_) => {
            printf(format_args!("{}: can't create task thread\n", sc.devname()));
        }
    }
}

/// `sdmmc_task_thread`: runs the slot's tasks until the bus goes away.
pub fn sdmmc_task_thread(arg: *mut c_void) {
    let sc = sdmmc_softc_arg(arg);

    // restart:
    loop {
        sdmmc_needs_discover(&sc.sc_dev);

        let mut s = splsdmmc();
        while sc.sc_dying.get() == 0 {
            while let Some(task) = sc.sc_tskq.first() {
                splx(s);
                sdmmc_del_task(task);
                let Some(func) = task.func.get() else {
                    panic(format_args!("{}: task without a function", sc.devname()));
                };
                func(task.arg.get());
                s = splsdmmc();
            }
            let _ = tsleep_nsec(ptr::from_ref(&sc.sc_tskq), PWAIT, "mmctsk", INFSLP);
        }
        splx(s);

        if sc.has_flag(SMF_CARD_PRESENT) {
            rw_enter_write(&sc.sc_lock);
            sdmmc_card_detach(sc, DETACH_FORCE);
            rw_exit(&sc.sc_lock);
        }

        // During a suspend, the card is detached since we do not know if it is the same
        // upon wakeup. Go re-discover the bus.
        if sc.sc_dying.get() == -1 {
            sc.clr_flag(SMF_CARD_PRESENT);
            sc.sc_dying.set(0);
            continue;
        }
        break;
    }
    sc.sc_task_thread.set(None);
    wakeup(ptr::from_ref(sc));
    kthread_exit(0);
}

/// `sdmmc_add_task`: queues `task` on the slot's thread.
pub fn sdmmc_add_task(sc: &'static SdmmcSoftc, task: &SdmmcTask) {
    let s = splsdmmc();
    // SAFETY: the task is on no queue (the callers check `sdmmc_task_pending`), and it lives
    // in the softc, a transfer's opening or a caller's frame that waits for it, until the
    // thread takes it off the queue.
    unsafe { sc.sc_tskq.insert_tail(task) };
    task.onqueue.set(1);
    task.sc.set(ptr::from_ref(sc));
    wakeup(ptr::from_ref(&sc.sc_tskq));
    splx(s);
}

/// `sdmmc_del_task`: takes `task` off its slot's queue, if it is on one.
pub fn sdmmc_del_task(task: &SdmmcTask) {
    let scp = task.sc.get();
    // SAFETY: `task.sc` is null or the slot whose queue holds the task (`sdmmc_add_task`);
    // softcs are never freed while the device exists.
    let Some(sc) = (unsafe { scp.as_ref() }) else {
        return;
    };

    let s = splsdmmc();
    task.sc.set(ptr::null());
    task.onqueue.set(0);
    // SAFETY: the task is on `sc_tskq` (its `sc` said so).
    unsafe { sc.sc_tskq.remove(task) };
    splx(s);
}

/// `sdmmc_needs_discover`: queues the discovery task unless it is already queued.
pub fn sdmmc_needs_discover(self_: &Device) {
    let sc = sdmmc_softc(self_);

    if !sdmmc_task_pending(&sc.sc_discover_task) {
        sdmmc_add_task(sc, &sc.sc_discover_task);
    }
}

/// `sdmmc_discover_task`: attaches a card that appeared, detaches one that went away.
pub fn sdmmc_discover_task(arg: *mut c_void) {
    let sc = sdmmc_softc_arg(arg);

    if sdmmc_chip_card_detect(sc.sct(), sc.sch.get()) {
        if !sc.has_flag(SMF_CARD_PRESENT) {
            sc.set_flag(SMF_CARD_PRESENT);
            sdmmc_card_attach(sc);
        }
    } else if sc.has_flag(SMF_CARD_PRESENT) {
        sc.clr_flag(SMF_CARD_PRESENT);
        rw_enter_write(&sc.sc_lock);
        sdmmc_card_detach(sc, DETACH_FORCE);
        rw_exit(&sc.sc_lock);
    }

    if sc.has_flag(SMF_CONFIG_PENDING) {
        sc.clr_flag(SMF_CONFIG_PENDING);
        config_pending_decr();
    }
}

/// `sdmmc_card_attach`: called from process context when a card is present.
pub fn sdmmc_card_attach(sc: &'static SdmmcSoftc) {
    // DPRINTF(1,("%s: attach card\n", DEVNAME(sc)));

    rw_enter_write(&sc.sc_lock);
    sc.clr_flag(SMF_CARD_ATTACHED);

    'err: {
        // Power up the card (or card stack).
        if sdmmc_enable(sc).is_err() {
            printf(format_args!("{}: can't enable card\n", sc.devname()));
            break 'err;
        }

        // Scan for I/O functions and memory cards on the bus, allocating a sdmmc_function
        // structure for each.
        if sdmmc_scan(sc).is_err() {
            printf(format_args!("{}: no functions\n", sc.devname()));
            break 'err;
        }

        // Initialize the I/O functions and memory cards.
        if sdmmc_init(sc).is_err() {
            printf(format_args!("{}: init failed\n", sc.devname()));
            break 'err;
        }

        // Attach SCSI emulation for memory cards.
        if sc.has_flag(SMF_MEM_MODE) {
            sdmmc_scsi_attach(sc);
        }

        // Attach I/O function drivers.
        if sc.has_flag(SMF_IO_MODE) {
            sdmmc_io_attach(sc);
        }

        sc.set_flag(SMF_CARD_ATTACHED);
        rw_exit(&sc.sc_lock);
        return;
    }
    // err:
    sdmmc_card_detach(sc, DETACH_FORCE);
    rw_exit(&sc.sc_lock);
}

/// `sdmmc_card_detach`: called from process context with `DETACH_*` flags from
/// `<sys/device.h>` when cards are gone.
pub fn sdmmc_card_detach(sc: &'static SdmmcSoftc, _flags: i32) {
    rw_assert_wrlock(&sc.sc_lock);

    // DPRINTF(1,("%s: detach card\n", DEVNAME(sc)));

    if sc.has_flag(SMF_CARD_ATTACHED) {
        // Detach I/O function drivers.
        if sc.has_flag(SMF_IO_MODE) {
            sdmmc_io_detach(sc);
        }

        // Detach the SCSI emulation for memory cards.
        if sc.has_flag(SMF_MEM_MODE) {
            sdmmc_scsi_detach(sc);
        }

        sc.clr_flag(SMF_CARD_ATTACHED);
    }

    // Power down.
    sdmmc_disable(sc);

    // Free all sdmmc_function structures.
    for sf in sc.sf_head.iter() {
        // SAFETY: every function on the list was made by `sdmmc_function_alloc`; the
        // iterator has read the next link, the list is reset below, and nothing else keeps
        // a function: `sc_card` was cleared by `sdmmc_disable` and `sc_fn0` is cleared
        // below, both under `sc_lock`.
        unsafe { sdmmc_function_free(sf) };
    }
    sc.sf_head.init();
    sc.sc_function_count.set(0);
    sc.sc_fn0.set(ptr::null());
}

/// `sdmmc_enable`: powers the card up and sets the host and card modes.
pub fn sdmmc_enable(sc: &'static SdmmcSoftc) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    let error = 'err: {
        // Calculate the equivalent of the card OCR from the host capabilities and select
        // the maximum supported bus voltage.
        let host_ocr = sdmmc_chip_host_ocr(sc.sct(), sc.sch.get());
        if let Err(e) = sdmmc_chip_bus_power(sc.sct(), sc.sch.get(), host_ocr) {
            printf(format_args!("{}: can't supply bus power\n", sc.devname()));
            break 'err Err(e);
        }

        // Select the minimum clock frequency.
        if let Err(e) = sdmmc_chip_bus_clock(
            sc.sct(),
            sc.sch.get(),
            SDMMC_SDCLK_400KHZ,
            SDMMC_TIMING_LEGACY,
        ) {
            printf(format_args!("{}: can't supply clock\n", sc.devname()));
            break 'err Err(e);
        }

        // XXX wait for card to power up
        sdmmc_delay(250000);

        // Initialize SD I/O card function(s).
        if let Err(e) = sdmmc_io_enable(sc) {
            break 'err Err(e);
        }

        // Initialize SD/MMC memory card(s).
        if sc.has_flag(SMF_MEM_MODE) {
            sdmmc_mem_enable(sc)
        } else {
            Ok(())
        }
    };
    // err:
    if error.is_err() {
        sdmmc_disable(sc);
    }

    error
}

/// `sdmmc_disable`: deselects the card and turns the bus power and clock off.
pub fn sdmmc_disable(sc: &SdmmcSoftc) {
    // XXX complete commands if card is still present.

    rw_assert_wrlock(&sc.sc_lock);

    // Make sure no card is still selected.
    let _ = sdmmc_select_card(sc, None);

    // Turn off bus power and clock.
    let _ = sdmmc_chip_bus_clock(sc.sct(), sc.sch.get(), SDMMC_SDCLK_OFF, SDMMC_TIMING_LEGACY);
    let _ = sdmmc_chip_bus_power(sc.sct(), sc.sch.get(), 0);
}

/// `sdmmc_set_bus_power`: sets the lowest bus voltage supported by the card and the host.
pub fn sdmmc_set_bus_power(sc: &SdmmcSoftc, host_ocr: u32, card_ocr: u32) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    // Mask off unsupported voltage levels and select the lowest.
    // DPRINTF(1,("%s: host_ocr=%x ", DEVNAME(sc), host_ocr));
    let ocr = sdmmc_lowest_ocr(host_ocr, card_ocr);
    // DPRINTF(1,("card_ocr=%x new_ocr=%x\n", card_ocr, host_ocr));

    if ocr == 0 || sdmmc_chip_bus_power(sc.sct(), sc.sch.get(), ocr).is_err() {
        return Err(Errno::EIO);
    }
    Ok(())
}

/// The OCR `sdmmc_set_bus_power` asks the chip for: the voltages both sides support, cut to
/// the lowest one and the one above it.
pub fn sdmmc_lowest_ocr(host_ocr: u32, card_ocr: u32) -> u32 {
    let mut host_ocr = host_ocr & card_ocr;
    for bit in 4..23 {
        if host_ocr & (1 << bit) != 0 {
            host_ocr &= 3 << bit;
            break;
        }
    }
    host_ocr
}

/// `sdmmc_function_alloc`.
pub fn sdmmc_function_alloc(sc: &'static SdmmcSoftc) -> &'static SdmmcFunction {
    let Some(p) = malloc(size_of::<SdmmcFunction>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!(
            "sdmmc_function_alloc: M_WAITOK allocation failed"
        ));
    };
    let sf = p.cast::<SdmmcFunction>();
    // SAFETY: a fresh allocation of the function's size (malloc(9) aligns it for any type),
    // written whole before it is shared; it lives until `sdmmc_function_free`.
    unsafe {
        sf.as_ptr().write(SdmmcFunction {
            sc,
            rca: Default::default(),
            flags: Default::default(),
            cookie: Default::default(),
            sf_list: SimpleqEntry::new(),
            number: (-1).into(),
            child: Default::default(),
            cis: SdmmcCis {
                manufacturer: SDMMC_VENDOR_INVALID,
                product: SDMMC_PRODUCT_INVALID,
                function: SDMMC_FUNCTION_INVALID,
                ..SdmmcCis::new()
            }
            .into(),
            cur_blklen: (sdmmc_chip_host_maxblklen(sc.sct(), sc.sch.get()) as u32).into(),
            csd: SdmmcCsd::default().into(),
            cid: SdmmcCid::default().into(),
            raw_cid: [0; 4].into(),
            scr: SdmmcScr::default().into(),
        });
        sf.as_ref()
    }
}

/// `sdmmc_function_free`.
///
/// # Safety
///
/// `sf` came from [`sdmmc_function_alloc`], is on no list, and nothing uses it afterwards.
pub unsafe fn sdmmc_function_free(sf: &SdmmcFunction) {
    free(
        NonNull::from(sf).cast(),
        M_DEVBUF,
        size_of::<SdmmcFunction>(),
    );
}

/// `sdmmc_scan`: scans for I/O functions and memory cards on the bus, allocating a
/// `sdmmc_function` structure for each.
pub fn sdmmc_scan(sc: &'static SdmmcSoftc) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    // Scan for I/O functions.
    if sc.has_flag(SMF_IO_MODE) {
        sdmmc_io_scan(sc);
    }

    // Scan for memory cards on the bus.
    if sc.has_flag(SMF_MEM_MODE) {
        sdmmc_mem_scan(sc);
    }

    // There should be at least one function now.
    if sc.sf_head.is_empty() {
        printf(format_args!("{}: can't identify card\n", sc.devname()));
        return Err(Errno::EIO);
    }
    Ok(())
}

/// `sdmmc_init`: initializes all the distinguished functions of the card, be it I/O or
/// memory functions.
pub fn sdmmc_init(sc: &'static SdmmcSoftc) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    // Initialize all identified card functions.
    for sf in sc.sf_head.iter() {
        if sc.has_flag(SMF_IO_MODE) && sdmmc_io_init(sc, sf).is_err() {
            printf(format_args!("{}: i/o init failed\n", sc.devname()));
        }

        if sc.has_flag(SMF_MEM_MODE) && sdmmc_mem_init(sc, sf).is_err() {
            printf(format_args!("{}: mem init failed\n", sc.devname()));
        }
    }

    // Any good functions left after initialization?
    if sc.sf_head.iter().any(|sf| sf.flags.get() & SFF_ERROR == 0) {
        return Ok(());
    }
    // No, we should probably power down the card.
    Err(Errno::EIO)
}

/// `sdmmc_delay`: sleeps (or spins while cold, or for less than a tick) `usecs`
/// microseconds.
pub fn sdmmc_delay(usecs: u32) {
    if !COLD.load(Ordering::Relaxed) && usecs > TICK.load(Ordering::Relaxed) as u32 {
        let chan = sdmmc_delay as fn(u32) as *const c_void;
        let _ = tsleep_nsec(chan, PWAIT, "mmcdly", usec_to_nsec(u64::from(usecs)));
    } else {
        delay(usecs);
    }
}

/// `sdmmc_app_command`: sends CMD55, then the application command `cmd`.
pub fn sdmmc_app_command(sc: &SdmmcSoftc, cmd: &mut SdmmcCommand) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    let mut acmd = SdmmcCommand::new();
    acmd.c_opcode = MMC_APP_CMD;
    acmd.c_arg = 0;
    // SAFETY: `sc_card` is null or a function on `sf_head`, which lives under `sc_lock`.
    if let Some(card) = unsafe { sc.sc_card.get().as_ref() } {
        acmd.c_arg = u32::from(card.rca.get()) << 16;
    }
    acmd.c_flags = SCF_CMD_AC | SCF_RSP_R1;

    sdmmc_mmc_command(sc, &mut acmd)?;

    if mmc_r1(&acmd.c_resp) & MMC_R1_APP_CMD == 0 {
        // Card does not support application commands.
        return Err(Errno::ENODEV);
    }

    sdmmc_mmc_command(sc, cmd)
}

/// `sdmmc_mmc_command`: executes an MMC command and its data transfer. All interactions
/// with the host controller to complete the command happen in the context of the current
/// process.
pub fn sdmmc_mmc_command(sc: &SdmmcSoftc, cmd: &mut SdmmcCommand) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    sdmmc_chip_exec_command(sc.sct(), sc.sch.get(), cmd);

    // SDMMC_DEBUG: sdmmc_dump_command(sc, cmd);

    let error = cmd.c_error;
    if !COLD.load(Ordering::Relaxed) {
        wakeup(ptr::from_ref(cmd));
    }

    match error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// `sdmmc_go_idle_state`: sends the "GO IDLE STATE" command.
pub fn sdmmc_go_idle_state(sc: &SdmmcSoftc) {
    rw_assert_wrlock(&sc.sc_lock);

    let mut cmd = SdmmcCommand::new();
    cmd.c_opcode = MMC_GO_IDLE_STATE;
    cmd.c_flags = SCF_CMD_BC | SCF_RSP_R0;

    let _ = sdmmc_mmc_command(sc, &mut cmd);
}

/// `sdmmc_send_if_cond`: sends the "SEND_IF_COND" command, to check operating condition.
pub fn sdmmc_send_if_cond(sc: &SdmmcSoftc, card_ocr: u32) -> Result<(), Errno> {
    let pat: u8 = 0x23; // any pattern will do here

    rw_assert_wrlock(&sc.sc_lock);

    let mut cmd = SdmmcCommand::new();

    cmd.c_opcode = SD_SEND_IF_COND;
    cmd.c_arg = (u32::from(card_ocr & SD_OCR_VOL_MASK != 0) << 8) | u32::from(pat);
    cmd.c_flags = SCF_CMD_BCR | SCF_RSP_R7;

    if sdmmc_mmc_command(sc, &mut cmd).is_err() {
        return Err(Errno::EIO);
    }

    let res = cmd.c_resp[0] as u8;
    if res != pat { Err(Errno::EIO) } else { Ok(()) }
}

/// `sdmmc_set_relative_addr`: retrieves (SD) or sets (MMC) the relative card address (RCA).
pub fn sdmmc_set_relative_addr(sc: &SdmmcSoftc, sf: &SdmmcFunction) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    let mut cmd = SdmmcCommand::new();

    if sc.has_flag(SMF_SD_MODE) {
        cmd.c_opcode = SD_SEND_RELATIVE_ADDR;
        cmd.c_flags = SCF_CMD_BCR | SCF_RSP_R6;
    } else {
        cmd.c_opcode = MMC_SET_RELATIVE_ADDR;
        cmd.c_arg = mmc_arg_rca(sf.rca.get());
        cmd.c_flags = SCF_CMD_AC | SCF_RSP_R1;
    }

    if sdmmc_mmc_command(sc, &mut cmd).is_err() {
        return Err(Errno::EIO);
    }

    if sc.has_flag(SMF_SD_MODE) {
        sf.rca.set(sd_r6_rca(&cmd.c_resp));
    }
    Ok(())
}

/// `sdmmc_select_card`: selects `sf` (deselects every card for `None`).
pub fn sdmmc_select_card(sc: &SdmmcSoftc, sf: Option<&SdmmcFunction>) -> Result<(), Errno> {
    rw_assert_wrlock(&sc.sc_lock);

    let sfp = sf.map_or(ptr::null(), ptr::from_ref);
    // SAFETY: `sc_card` is null or a function on `sf_head`, which lives under `sc_lock`.
    let card = unsafe { sc.sc_card.get().as_ref() };
    if ptr::eq(sc.sc_card.get(), sfp)
        || matches!((sf, card), (Some(sf), Some(card)) if card.rca.get() == sf.rca.get())
    {
        sc.sc_card.set(sfp);
        return Ok(());
    }

    let mut cmd = SdmmcCommand::new();
    cmd.c_opcode = MMC_SELECT_CARD;
    cmd.c_arg = sf.map_or(0, |sf| mmc_arg_rca(sf.rca.get()));
    cmd.c_flags = SCF_CMD_AC | if sf.is_none() { SCF_RSP_R0 } else { SCF_RSP_R1 };
    let error = sdmmc_mmc_command(sc, &mut cmd);
    if error.is_ok() || sf.is_none() {
        sc.sc_card.set(sfp);
    }
    error
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::sdmmc::sdmmcreg::{
        MMC_OCR_1_65V_1_95V, MMC_OCR_2_9V_3_0V, MMC_OCR_3_0V_3_1V, MMC_OCR_3_2V_3_3V,
        MMC_OCR_3_3V_3_4V,
    };

    #[test]
    fn the_lowest_common_voltage_is_chosen() {
        // sdhc's 3.3 V host and a card that takes 2.7-3.6 V: 3.2-3.4 V.
        let host = MMC_OCR_3_2V_3_3V | MMC_OCR_3_3V_3_4V;
        let card = 0x00ff_8000;
        assert_eq!(sdmmc_lowest_ocr(host, card), host);
        // A host with 1.8, 3.0 and 3.3 V and a card from 2.9 V up: 2.9-3.1 V.
        let host = MMC_OCR_1_65V_1_95V
            | MMC_OCR_2_9V_3_0V
            | MMC_OCR_3_0V_3_1V
            | MMC_OCR_3_2V_3_3V
            | MMC_OCR_3_3V_3_4V;
        assert_eq!(
            sdmmc_lowest_ocr(host, 0x00fe_0000),
            MMC_OCR_2_9V_3_0V | MMC_OCR_3_0V_3_1V
        );
        // Nothing in common.
        assert_eq!(sdmmc_lowest_ocr(MMC_OCR_1_65V_1_95V, 0x00ff_8000), 0);
    }
}
/* </TESTS> */
