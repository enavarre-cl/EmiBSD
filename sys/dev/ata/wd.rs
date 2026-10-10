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
 * Copyright (c) 1998, 2001 Manuel Bouyer.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *	notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *	notice, this list of conditions and the following disclaimer in the
 *	documentation and/or other materials provided with the distribution.
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

/*-
 * Copyright (c) 1998, 2003, 2004 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum and by Onno van der Linden.
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! wd(4): the ATA disk driver. It attaches to every ATA drive a wdc(4) channel finds, reads
//! its IDENTIFY data, prints the `wd0: <QEMU HARDDISK>` and `16-sector PIO, LBA48, 64MB,
//! 131072 sectors` lines, sets up look-ahead, the write cache and the security freeze lock,
//! and attaches the disk. The block (`bdevsw[0]`) and character (`cdevsw[3]`) devices open
//! it (loading its label through `readdisklabel`), queue buffers (`wdstrategy`) and turn
//! each into an `ata_bio` the channel runs (`__wdstart`, `wdc_ata_bio`), completed by
//! `wddone` with resets and retries; `wdioctl` handles the label and cache commands and
//! hands the rest to `wdc_ioctl`; `wddump` writes a crash dump by polling.
//!
//! Upstream: sys/dev/ata/wd.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `wd_get_params` works on `wd->sc_params`, which is what every caller passes; its
//!   "If we already have drive parameters" copy into another buffer never happens.
//! - The in-core label is handled as in `sd.rs`: `wdgetdisklabel` builds the label in a
//!   local (a Rust `&mut` may not alias the label `wdstrategy` and the transfers read),
//!   first publishing the initialised label when no partition is open (what the C's in-core
//!   label holds while `readdisklabel` reads into it), and `wdopen`/`DIOCRLDINFO` install the
//!   result. `DIOCRLDINFO`'s label is `malloc(9)`ed as in the C. `DIOCWDINFO` writes a copy
//!   of the in-core label.
//! - `wdsize` opens the partition through [`wdopen_noproc`], `wdopen` without its thread
//!   argument, which `wdopen` never reads (`wdsize` passes NULL).
//! - `caddr_t addr` of `wdioctl` is the kernel copy of the argument as a byte slice
//!   (`d_ioctl`'s type), as in `sdioctl`.
//! - `wddone` takes the softc (`void *v`), `wdrestart` is the timeout's `fn(*mut c_void)`.
//! - `B_FORMAT` is not defined: `wdformat` is not compiled, as in the C.
//!   `WD_DUMP_NOT_TRUSTED` is not defined: `wddump` writes. `option HIBERNATE` is not
//!   configured.
//! - `wddump` is complete, but nothing calls it yet: `dumpsys` and `dumpconf` are not ported.
//! - The commands of `wdattach`, `wd_flushcache` and `wd_standby` go through `ata.rs`'s
//!   `exec_on_stack`, which panics without `AT_WAIT` or `AT_POLL` (every caller passes one).
//! - The `#define sc_drive`/`sc_mode` aliases name `ata_bio` members that do not exist and
//!   are unused; `sc_multi` is `sc_wdc_bio.multi`.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::dev::ata::ata::{ata_get_params, ata_perror, exec_on_stack};
use crate::dev::ata::ata_wdc::wdc_ata_bio;
#[cfg(feature = "wdcdebug")]
use crate::dev::ata::ata_wdc::wdcdebug_wd_mask;
use crate::dev::ata::atareg::{
    ATA_CFG_FIXED, ATAPI_CMD2_48AD, WDC_CAP_LBA, WDC_CMD1_AHEAD, WDC_CMD1_CACHE,
};
use crate::dev::ata::atavar::{
    AT_DF, AT_ERROR, AT_POLL, AT_TIMEOU, AT_WAIT, AtaAtapiAttach, CMD_AGAIN, CMD_ERR, CMD_OK,
    NERRS_MAX, T_ATA, WDC_COMPLETE, WDC_QUEUED, WDC_TRY_AGAIN, WdcCommand,
};
use crate::dev::ata::wdvar::{
    ATA_LBA, ATA_LBA48, ATA_POLL, ATA_READ, ATA_SINGLE, ERR_DF, ERR_DMA, ERR_NODEV, ERROR, NOERROR,
    READY, RECAL, TIMEOUT, WDF_LBA, WDF_LBA48, WDF_LOADED, WdSoftc,
};
use crate::dev::ic::wdc::{
    at_poll, wdc_disable_intr, wdc_drvp_chp, wdc_enable_intr, wdc_ioctl, wdc_print_caps,
    wdc_probe_caps, wdc_reset_channel, wdcdebug_print,
};
use crate::dev::ic::wdcreg::{
    SET_FEATURES, WDCC_FLUSHCACHE, WDCC_FLUSHCACHE_EXT, WDCC_SEC_FREEZE_LOCK, WDCC_STANDBY_IMMED,
    WDCE_ABRT, WDCE_MC, WDCE_MCR, WDCS_DRDY, WDSF_EN_WR_CACHE, WDSF_READAHEAD_EN,
};
use crate::kern::init_main::BOOTHOWTO;
use crate::kern::kern_bufq::{bufq_dequeue, bufq_destroy, bufq_drain, bufq_init, bufq_queue};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_physio::{minphys, physio};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_add_sec, timeout_del, timeout_set};
use crate::kern::subr_autoconf::device_unref;
use crate::kern::subr_disk::{
    bounds_check_with_label, disk_attach, disk_busy, disk_closepart, disk_detach, disk_gone,
    disk_lock, disk_lock_nointr, disk_lookup, disk_openpart, disk_unbusy, disk_unlock, diskerr,
    dkcksum, initdisklabel, setdisklabel,
};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::kern::vfs_bio::biodone;
use crate::machine::cpu::delay;
use crate::machine::disklabel::{readdisklabel, writedisklabel};
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::{B_ERROR, B_READ, B_WRITE, BUFQ_DEFAULT, Buf};
use crate::sys::device::{
    CfMatch, Cfattach, Cfdriver, DV_DISK, DVACT_POWERDOWN, DVACT_RESUME, DVACT_SUSPEND, Device,
};
use crate::sys::disklabel::{
    DISKLABEL_SIZE, DISKMAGIC, DTYPE_ESDI, DTYPE_ST506, Disklabel, Partinfo, disklabeldev,
    diskpart, diskunit, dl_blktosec, dl_getdsize, dl_getpoffset, dl_getpsize, dl_sectoblk,
    dl_setdsize,
};
use crate::sys::dkio::{
    DIOCCACHESYNC, DIOCGDINFO, DIOCGPART, DIOCGPDINFO, DIOCRLDINFO, DIOCSDINFO, DIOCWDINFO,
};
use crate::sys::errno::Errno::{self, *};
use crate::sys::fcntl::FWRITE;
use crate::sys::malloc::{M_TEMP, M_WAITOK};
use crate::sys::param::{DEV_BSIZE, howmany};
use crate::sys::proc::Proc;
use crate::sys::reboot::RB_POWERDOWN;
use crate::sys::select::NBBY;
use crate::sys::stat::S_IFBLK;
use crate::sys::syslog::LOG_PRINTF;
use crate::sys::types::{Daddr, Dev};
use crate::sys::uio::Uio;

/// `LBA48_THRESHOLD`: 128GB / `DEV_BSIZE`.
const LBA48_THRESHOLD: u64 = 0xfffffff;

/// `WDIORETRIES_SINGLE`: number of retries before single-sector.
const WDIORETRIES_SINGLE: i32 = 4;
/// `WDIORETRIES`: number of retries before giving up.
const WDIORETRIES: i32 = 5;
/// `RECOVERYTIME_MSEC`: time to wait before retrying a cmd.
const RECOVERYTIME_MSEC: u64 = 500;

/// `DEBUG_INTR`.
#[allow(dead_code)] // the C defines it for this file too
const DEBUG_INTR: i32 = 0x01;
/// `DEBUG_XFERS`.
const DEBUG_XFERS: i32 = 0x02;
/// `DEBUG_STATUS`.
#[allow(dead_code)] // the C defines it for this file too
const DEBUG_STATUS: i32 = 0x04;
/// `DEBUG_FUNCS`.
const DEBUG_FUNCS: i32 = 0x08;
/// `DEBUG_PROBE`.
const DEBUG_PROBE: i32 = 0x10;

/// `wd_ca`.
pub static WD_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<WdSoftc>(),
    ca_match: Some(wdprobe),
    ca_attach: wdattach,
    ca_detach: Some(wddetach),
    ca_activate: Some(wdactivate),
};

/// `wd_cd`.
pub static WD_CD: Cfdriver = Cfdriver::new(b"wd", DV_DISK, 0);

/// `wddoingadump`.
static WDDOINGADUMP: AtomicBool = AtomicBool::new(false);
/// `wddumprecalibrated`.
static WDDUMPRECALIBRATED: AtomicBool = AtomicBool::new(false);
/// `wddumpmulti`.
static WDDUMPMULTI: AtomicI32 = AtomicI32::new(1);

/// The zeroed label the I/O paths read before `disk_attach` allocated one.
static ZERO_LABEL: Disklabel = Disklabel::zeroed();

/// `wdlookup(unit)`: the attached unit, referenced (`disk_lookup`).
fn wdlookup(unit: u32) -> Option<NonNull<Device>> {
    disk_lookup(&WD_CD, i32::try_from(unit).ok()?)
}

/// `(struct wd_softc *)dv`.
fn wd_softc(dv: NonNull<Device>) -> &'static WdSoftc {
    // SAFETY: every `wd` device is a `WdSoftc` (`WD_CA.ca_devsize`) that autoconf allocated
    // and frees only after `wddetach`, once the references `wdlookup` takes are dropped.
    unsafe { dv.as_ref().softc::<WdSoftc>() }
}

/// `wdprobe`: an ATA drive at the locators' channel and drive.
pub fn wdprobe(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    if aux.is_null() {
        return 0;
    }
    // SAFETY: wdc's children are attached with a `struct ata_atapi_attach` as `aux`.
    let aa_link = unsafe { &*aux.cast::<AtaAtapiAttach>().cast_const() };
    let cf = match_.cfdata();

    if aa_link.aa_type != T_ATA {
        return 0;
    }

    if cf.cf_loc[0] != -1 && cf.cf_loc[0] != i64::from(aa_link.aa_channel) {
        return 0;
    }

    let drive = aa_link.aa_drv_data.map_or(0, |d| d.drive.get());
    if cf.cf_loc[1] != -1 && cf.cf_loc[1] != i64::from(drive) {
        return 0;
    }

    1
}

/// The model string of `wdattach`'s message: runs of blanks become one, trailing ones go.
pub fn wd_model(model: &[u8; 40], buf: &mut [u8; 41]) {
    let mut blank = false;
    let mut q = 0;
    for &c in model {
        if c == b'\0' {
            break;
        }
        if c != b' ' {
            if blank {
                buf[q] = b' ';
                q += 1;
                blank = false;
            }
            buf[q] = c;
            q += 1;
        } else {
            blank = true;
        }
    }
    buf[q] = b'\0';
}

/// `wdattach`.
pub fn wdattach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let wd = wd_softc(NonNull::from(self_));
    // SAFETY: wdc's children are attached with a `struct ata_atapi_attach` as `aux`.
    let aa_link = unsafe { &*aux.cast::<AtaAtapiAttach>().cast_const() };
    wdcdebug_print!(wdcdebug_wd_mask, DEBUG_FUNCS | DEBUG_PROBE, "wdattach\n");

    wd.openings.set(i32::from(aa_link.aa_openings));
    let Some(drvp) = aa_link.aa_drv_data else {
        panic(format_args!("{}: no drive data", self_.xname()));
    };
    wd.drvp.set(Some(drvp));

    let mut name = [0u8; 31];
    let _ = libkern::strlcpy(&mut name, self_.xname().as_bytes());
    drvp.drive_name.set(name);
    drvp.cf_flags.set(self_.cfdata().cf_flags);

    if NERRS_MAX - 2 > 0 {
        drvp.n_dmaerrs.set(NERRS_MAX - 2);
    } else {
        drvp.n_dmaerrs.set(0);
    }

    // read our drive info
    if wd_get_params(wd, at_poll.load(Ordering::Relaxed) as u8) != 0 {
        let _ = printf(format_args!("{}: IDENTIFY failed\n", self_.xname()));
        return;
    }

    let params = wd.params();
    let mut buf = [0u8; 41];
    wd_model(&params.atap_model, &mut buf);

    let _ = printf(format_args!(": <{}>\n", Str(&buf)));

    wdc_probe_caps(drvp, params);
    wdc_print_caps(drvp);

    if params.atap_multi & 0xff > 1 {
        wd.sc_wdc_bio.multi.set(i32::from(params.atap_multi & 0xff));
    } else {
        wd.sc_wdc_bio.multi.set(1);
    }

    let _ = printf(format_args!(
        "{}: {}-sector PIO,",
        self_.xname(),
        wd.sc_wdc_bio.multi.get()
    ));

    // use 48-bit LBA if enabled
    if params.atap_cmd2_en & ATAPI_CMD2_48AD != 0 {
        wd.set(WDF_LBA48);
    }

    // Prior to ATA-4, LBA was optional.
    if params.atap_capabilities1 & WDC_CAP_LBA != 0 {
        wd.set(WDF_LBA);
    }
    // #if 0: ATA-4 requires LBA (atap_ataversion >= WDC_VER_ATA4 sets WDF_LBA).

    let mb = 1_048_576 / DEV_BSIZE as u64;
    if wd.isset(WDF_LBA48) {
        let lba = &params.atap_max_lba;
        let cap = (u64::from(lba[3]) << 48)
            | (u64::from(lba[2]) << 32)
            | (u64::from(lba[1]) << 16)
            | u64::from(lba[0]);
        wd.sc_capacity.set(cap);
        let _ = printf(format_args!(" LBA48, {}MB, {} sectors\n", cap / mb, cap));
    } else if wd.isset(WDF_LBA) {
        // The C's int arithmetic, sign extended into the 64-bit capacity.
        let cap = ((i32::from(params.atap_capacity[1]) << 16) | i32::from(params.atap_capacity[0]))
            as i64 as u64;
        wd.sc_capacity.set(cap);
        let _ = printf(format_args!(" LBA, {}MB, {} sectors\n", cap / mb, cap));
    } else {
        let cap = (i32::from(params.atap_cylinders)
            * i32::from(params.atap_heads)
            * i32::from(params.atap_sectors)) as i64 as u64;
        wd.sc_capacity.set(cap);
        let _ = printf(format_args!(
            " CHS, {}MB, {} cyl, {} head, {} sec, {} sectors\n",
            cap / mb,
            params.atap_cylinders,
            params.atap_heads,
            params.atap_sectors,
            cap
        ));
    }
    wdcdebug_print!(
        wdcdebug_wd_mask,
        DEBUG_PROBE,
        "{}: atap_dmatiming_mimi={}, atap_dmatiming_recom={}\n",
        self_.xname(),
        params.atap_dmatiming_mimi,
        params.atap_dmatiming_recom
    );

    let poll = at_poll.load(Ordering::Relaxed);

    // use read look ahead if supported
    if params.atap_cmd_set1 & WDC_CMD1_AHEAD != 0 {
        let wdc_c = WdcCommand::new();
        wdc_c.r_command.set(SET_FEATURES);
        wdc_c.r_features.set(WDSF_READAHEAD_EN);
        wdc_c.timeout.set(1000);
        wdc_c.flags.set(poll);

        if exec_on_stack(drvp, &wdc_c, "wdattach") != WDC_COMPLETE {
            let _ = printf(format_args!(
                "{}: enable look ahead command didn't complete\n",
                self_.xname()
            ));
        }
    }

    // use write cache if supported
    if params.atap_cmd_set1 & WDC_CMD1_CACHE != 0 {
        let wdc_c = WdcCommand::new();
        wdc_c.r_command.set(SET_FEATURES);
        wdc_c.r_features.set(WDSF_EN_WR_CACHE);
        wdc_c.timeout.set(1000);
        wdc_c.flags.set(poll);

        if exec_on_stack(drvp, &wdc_c, "wdattach") != WDC_COMPLETE {
            let _ = printf(format_args!(
                "{}: enable write cache command didn't complete\n",
                self_.xname()
            ));
        }
    }

    // FREEZE LOCK the drive so malicious users can't lock it on us. As there is no harm in
    // issuing this to drives that don't support the security feature set we just send it,
    // and don't bother checking if the drive sends a command abort to tell us it doesn't
    // support it.
    let wdc_c = WdcCommand::new();

    wdc_c.r_command.set(WDCC_SEC_FREEZE_LOCK);
    wdc_c.timeout.set(1000);
    wdc_c.flags.set(poll);
    if exec_on_stack(drvp, &wdc_c, "wdattach") != WDC_COMPLETE {
        let _ = printf(format_args!(
            "{}: freeze lock command didn't complete\n",
            self_.xname()
        ));
    }

    // Initialize disk structures.
    let mut dk_name = [0u8; 16];
    let xname = self_.xname().as_bytes();
    let n = xname.len().min(dk_name.len());
    dk_name[..n].copy_from_slice(&xname[..n]);
    wd.sc_dk.dk_name.set(dk_name);
    let _ = bufq_init(&wd.sc_bufq, BUFQ_DEFAULT);
    timeout_set(
        &wd.sc_restart_timeout,
        wdrestart,
        ptr::from_ref(wd).cast_mut().cast(),
    );

    // Attach disk.
    disk_attach(Some(&wd.sc_dev), &wd.sc_dk);
    wd.sc_wdc_bio.lp.set(wd.sc_dk.dk_label.get());
}

/// `wdactivate`.
pub fn wdactivate(self_: &Device, act: i32) -> Result<(), Errno> {
    let wd = wd_softc(NonNull::from(self_));

    match act {
        DVACT_SUSPEND => {}
        DVACT_POWERDOWN => {
            let _ = wd_flushcache(wd, AT_POLL);
            if BOOTHOWTO.load(Ordering::Relaxed) & RB_POWERDOWN != 0 {
                wd_standby(wd, AT_POLL);
            }
        }
        DVACT_RESUME => {
            // Do two resets separated by a small delay. The first wakes the controller, the
            // second resets the channel.
            let drvp = wd.drvp();
            wdc_disable_intr(wdc_drvp_chp(drvp));
            wdc_reset_channel(drvp, true);
            delay(10000);
            wdc_reset_channel(drvp, false);
            wdc_enable_intr(wdc_drvp_chp(drvp));
            let _ = wd_get_params(wd, at_poll.load(Ordering::Relaxed) as u8);
        }
        _ => {}
    }
    Ok(())
}

/// `wddetach`.
pub fn wddetach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = wd_softc(NonNull::from(self_));

    let _ = timeout_del(&sc.sc_restart_timeout);

    bufq_drain(&sc.sc_bufq);

    disk_gone(wdopen, self_.dv_unit.get() as u32);

    // Detach disk.
    bufq_destroy(&sc.sc_bufq);
    disk_detach(&sc.sc_dk);

    Ok(())
}

/// `wdstrategy`: read/write routine for a buffer. Validates the arguments and schedules
/// the transfer. Does not wait for the transfer to complete.
pub fn wdstrategy(bp: &'static Buf) {
    let dv = wdlookup(diskunit(bp.b_dev.get()));

    'done: {
        let error = 'bad: {
            let Some(dv) = dv else {
                break 'bad ENXIO;
            };
            let wd = wd_softc(dv);

            wdcdebug_print!(
                wdcdebug_wd_mask,
                DEBUG_XFERS,
                "wdstrategy ({})\n",
                wd.sc_dev.xname()
            );

            // If device invalidated (e.g. media change, door open), error.
            if !wd.isset(WDF_LOADED) {
                break 'bad EIO;
            }

            // Validate the request.
            let ok = wd
                .sc_dk
                .with_label(|lp| bounds_check_with_label(bp, lp.unwrap_or(&ZERO_LABEL)));
            if !ok {
                break 'done;
            }

            // Check that the number of sectors can fit in a byte.
            let secsize = wd
                .sc_dk
                .with_label(|lp| i64::from(lp.unwrap_or(&ZERO_LABEL).d_secsize));
            if bp.b_bcount.get() / secsize >= (1i64 << NBBY) {
                break 'bad EINVAL;
            }

            // Queue transfer on drive, activate drive and controller if idle.
            bufq_queue(&wd.sc_bufq, bp);
            let s = splbio();
            wdstart(wd);
            splx(s);
            // SAFETY: the reference `wdlookup` took.
            unsafe { device_unref(dv) };
            return;
        };

        // bad:
        bp.b_error.set(Some(error));
        bp.set(B_ERROR);
        bp.b_resid
            .set(usize::try_from(bp.b_bcount.get()).unwrap_or(0));
    }

    // done:
    let s = splbio();
    biodone(bp);
    splx(s);
    if let Some(dv) = dv {
        // SAFETY: the reference `wdlookup` took.
        unsafe { device_unref(dv) };
    }
}

/// `wdstart`: queues a drive for I/O.
pub fn wdstart(wd: &'static WdSoftc) {
    wdcdebug_print!(
        wdcdebug_wd_mask,
        DEBUG_XFERS,
        "wdstart {}\n",
        wd.sc_dev.xname()
    );
    while wd.openings.get() > 0 {
        // Is there a buf for us ?
        let Some(bp) = bufq_dequeue(&wd.sc_bufq) else {
            return;
        };
        // Make the command. First lock the device
        wd.openings.set(wd.openings.get() - 1);

        wd.retries.set(0);
        __wdstart(wd, bp);
    }
}

/// `__wdstart`: turns `bp` into the softc's `ata_bio` and hands it to the channel.
pub fn __wdstart(wd: &'static WdSoftc, bp: &'static Buf) {
    let bio = &wd.sc_wdc_bio;

    let (blkno, nsecs) = wd.sc_dk.with_label(|lp| {
        let lp = lp.unwrap_or(&ZERO_LABEL);
        let part = &lp.d_partitions[diskpart(bp.b_dev.get()) as usize];
        let blkno = dl_blktosec(
            lp,
            (bp.b_blkno.get() as u64).wrapping_add(dl_sectoblk(lp, dl_getpoffset(part))),
        );
        let nsecs = howmany(bp.b_bcount.get() as usize, lp.d_secsize as usize) as u64;
        (blkno, nsecs)
    });
    bio.blkno.set(blkno as Daddr);
    bio.blkdone.set(0);
    wd.sc_bp.set(Some(bp));
    // If we're retrying, retry in single-sector mode. This will give us the sector number
    // of the problem, and will eventually allow the transfer to succeed.
    if wd.retries.get() >= WDIORETRIES_SINGLE {
        bio.flags.set(ATA_SINGLE);
    } else {
        bio.flags.set(0);
    }
    if wd.isset(WDF_LBA48)
        // use LBA48 only if really need
        && (blkno.wrapping_add(nsecs).wrapping_sub(1) >= LBA48_THRESHOLD || nsecs > 0xff)
    {
        bio.set(ATA_LBA48);
    }
    if wd.isset(WDF_LBA) {
        bio.set(ATA_LBA);
    }
    if bp.isset(B_READ) {
        bio.set(ATA_READ);
    }
    bio.bcount.set(bp.b_bcount.get());
    bio.databuf.set(bp.b_data.get());
    bio.wd.set(Some(NonNull::from(wd)));
    // Instrumentation.
    disk_busy(&wd.sc_dk);
    match wdc_ata_bio(wd.drvp(), bio) {
        WDC_TRY_AGAIN => {
            let _ = timeout_add_sec(&wd.sc_restart_timeout, 1);
        }
        WDC_QUEUED => {}
        WDC_COMPLETE => {
            // This code is never executed because we never set the ATA_POLL flag above
            // (#if 0: wddone(wd) for a polled transfer).
        }
        _ => panic(format_args!(
            "__wdstart: bad return code from wdc_ata_bio()"
        )),
    }
}

/// `wddone`: the channel completed the softc's `ata_bio`: finishes its buffer, or resets the
/// channel and retries it.
pub fn wddone(wd: &'static WdSoftc) {
    let Some(bp) = wd.sc_bp.get() else {
        panic(format_args!("{}: wddone without a buf", wd.sc_dev.xname()));
    };
    let bio = &wd.sc_wdc_bio;
    let mut buf = [0u8; 256];
    wdcdebug_print!(
        wdcdebug_wd_mask,
        DEBUG_XFERS,
        "wddone {}\n",
        wd.sc_dev.xname()
    );

    bp.b_resid
        .set(usize::try_from(bio.bcount.get()).unwrap_or(0));
    // `None`: the buffer's fate is set; `Some(what)`: goto retry with that message.
    let retry: Option<&str> = match bio.error.get() {
        ERR_NODEV => {
            bp.set(B_ERROR);
            bp.b_error.set(Some(ENXIO));
            None
        }
        ERR_DMA => Some("DMA error"),
        ERR_DF => Some("device fault"),
        TIMEOUT => Some("device timeout"),
        ERROR => {
            // Don't care about media change bits
            let r_error = bio.r_error.get();
            if r_error != 0 && r_error & !(WDCE_MC | WDCE_MCR) == 0 {
                wd_noerror(wd);
                None
            } else {
                ata_perror(wd.drvp(), i32::from(r_error), &mut buf);
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                Some(core::str::from_utf8(&buf[..len]).unwrap_or("?"))
            }
        }
        NOERROR => {
            wd_noerror(wd);
            None
        }
        _ => None,
    };
    if let Some(errbuf) = retry {
        // Just reset and retry. Can we do more ?
        wdc_reset_channel(wd.drvp(), false);
        wd.sc_dk
            .with_label(|lp| diskerr(bp, "wd", errbuf, LOG_PRINTF, bio.blkdone.get() as i32, lp));
        let retries = wd.retries.get();
        wd.retries.set(retries + 1);
        if retries < WDIORETRIES {
            let _ = printf(format_args!(", retrying\n"));
            let _ = timeout_add_msec(&wd.sc_restart_timeout, RECOVERYTIME_MSEC);
            return;
        }
        let _ = printf(format_args!("\n"));
        bp.set(B_ERROR);
        bp.b_error.set(Some(EIO));
    }
    disk_unbusy(
        &wd.sc_dk,
        bp.b_bcount.get() - bp.b_resid.get() as i64,
        bp.b_blkno.get(),
        bp.isset(B_READ),
    );
    biodone(bp);
    wd.openings.set(wd.openings.get() + 1);
    wdstart(wd);
}

/// The `noerror:` case of `wddone`.
fn wd_noerror(wd: &WdSoftc) {
    if wd.sc_wdc_bio.isset(crate::dev::ata::wdvar::ATA_CORR) || wd.retries.get() > 0 {
        let _ = printf(format_args!(
            "{}: soft error (corrected)\n",
            wd.sc_dev.xname()
        ));
    }
}

/// `wdrestart`: the restart timeout: hands the current buffer to the channel again.
pub fn wdrestart(v: *mut c_void) {
    // SAFETY: `wdattach` set the timeout with its softc, which outlives it (`wddetach`
    // deletes it).
    let wd: &'static WdSoftc = unsafe { &*v.cast::<WdSoftc>().cast_const() };
    let Some(bp) = wd.sc_bp.get() else {
        panic(format_args!(
            "{}: wdrestart without a buf",
            wd.sc_dev.xname()
        ));
    };
    wdcdebug_print!(
        wdcdebug_wd_mask,
        DEBUG_XFERS,
        "wdrestart {}\n",
        wd.sc_dev.xname()
    );

    let chnl = wdc_drvp_chp(wd.drvp());
    if chnl.dying.get() != 0 {
        return;
    }

    let s = splbio();
    disk_unbusy(&wd.sc_dk, 0, 0, bp.isset(B_READ));
    __wdstart(wd, bp);
    splx(s);
}

/// `wdread`: the raw device's read, straight into the user's buffer (physio(9)).
pub fn wdread(dev: Dev, uio: &mut Uio<'_>, _flags: i32) -> Result<(), Errno> {
    wdcdebug_print!(wdcdebug_wd_mask, DEBUG_XFERS, "wdread\n");
    physio(wdstrategy, dev, B_READ, minphys, uio)
}

/// `wdwrite`: the raw device's write, straight from the user's buffer (physio(9)).
pub fn wdwrite(dev: Dev, uio: &mut Uio<'_>, _flags: i32) -> Result<(), Errno> {
    wdcdebug_print!(wdcdebug_wd_mask, DEBUG_XFERS, "wdwrite\n");
    physio(wdstrategy, dev, B_WRITE, minphys, uio)
}

/// `wdopen`.
pub fn wdopen(dev: Dev, flag: i32, fmt: i32, _p: &Proc) -> Result<(), Errno> {
    wdopen_noproc(dev, flag, fmt)
}

/// [`wdopen`] without the thread, which it never reads (`wdsize` passes NULL).
pub fn wdopen_noproc(dev: Dev, _flag: i32, fmt: i32) -> Result<(), Errno> {
    wdcdebug_print!(wdcdebug_wd_mask, DEBUG_FUNCS, "wdopen\n");

    let unit = diskunit(dev);
    let dv = wdlookup(unit).ok_or(ENXIO)?;
    let wd = wd_softc(dv);
    let unref = || {
        // SAFETY: the reference `wdlookup` took.
        unsafe { device_unref(dv) }
    };
    let chnl = wdc_drvp_chp(wd.drvp());
    if chnl.dying.get() != 0 {
        unref();
        return Err(ENXIO);
    }

    // If this is the first open of this device, add a reference to the adapter.
    if let Err(e) = disk_lock(&wd.sc_dk) {
        // bad4:
        unref();
        return Err(e);
    }

    let error = 'bad: {
        if wd.sc_dk.dk_openmask.get() != 0 {
            // If any partition is open, but the disk has been invalidated, disallow further
            // opens.
            if !wd.isset(WDF_LOADED) {
                break 'bad Err(EIO); // goto bad3
            }
        } else if !wd.isset(WDF_LOADED) {
            wd.set(WDF_LOADED);

            // Load the physical device parameters.
            let _ = wd_get_params(wd, AT_WAIT as u8);

            // Load the partition info if not already loaded.
            let mut lp = Disklabel::zeroed();
            let error = wdgetdisklabel(dev, wd, &mut lp, false);
            // SAFETY: under the disk lock, with no partition open: nobody else holds the
            // in-core label.
            if let Some(dl) = unsafe { wd.sc_dk.label_mut() } {
                *dl = lp;
            }
            if error == Err(EIO) {
                break 'bad Err(EIO);
            }
        }

        let part = diskpart(dev);

        disk_openpart(&wd.sc_dk, part, fmt, true)
    };

    // bad: (nothing to undo when no partition is open)
    // bad3:
    disk_unlock(&wd.sc_dk);
    // bad4:
    unref();
    error
}

/// `wdclose`.
pub fn wdclose(dev: Dev, _flag: i32, fmt: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let part = diskpart(dev);

    let dv = wdlookup(diskunit(dev)).ok_or(ENXIO)?;
    let wd = wd_softc(dv);

    wdcdebug_print!(wdcdebug_wd_mask, DEBUG_FUNCS, "wdclose\n");

    disk_lock_nointr(&wd.sc_dk);

    disk_closepart(&wd.sc_dk, part, fmt);

    if wd.sc_dk.dk_openmask.get() == 0 {
        let _ = wd_flushcache(wd, AT_WAIT);
        // XXXX Must wait for I/O to complete!
    }

    disk_unlock(&wd.sc_dk);

    // SAFETY: the reference `wdlookup` took.
    unsafe { device_unref(dv) };
    Ok(())
}

/// `strncpy(dst, src, sizeof(dst))`: `src` up to its NUL, then NULs to the end of `dst`.
fn strncpy(dst: &mut [u8], src: &[u8]) {
    let src = &src[..src.iter().position(|&c| c == 0).unwrap_or(src.len())];
    let n = src.len().min(dst.len());
    dst[..n].copy_from_slice(&src[..n]);
    dst[n..].fill(0);
}

/// `wdgetdefaultlabel`: the label of a disk without one, from its IDENTIFY data.
pub fn wdgetdefaultlabel(wd: &WdSoftc, lp: &mut Disklabel) {
    wdcdebug_print!(wdcdebug_wd_mask, DEBUG_FUNCS, "wdgetdefaultlabel\n");
    *lp = Disklabel::zeroed();

    let params = wd.params();
    lp.d_secsize = DEV_BSIZE as u32;
    dl_setdsize(lp, wd.sc_capacity.get());
    lp.d_ntracks = u32::from(params.atap_heads);
    lp.d_nsectors = u32::from(params.atap_sectors);
    lp.d_secpercyl = lp.d_ntracks * lp.d_nsectors;
    lp.d_ncylinders = (dl_getdsize(lp) / u64::from(lp.d_secpercyl)) as u32;
    if wd.drvp().ata_vers.get() == -1 {
        lp.d_type = DTYPE_ST506;
        strncpy(&mut lp.d_typename, b"ST506/MFM/RLL");
    } else {
        lp.d_type = DTYPE_ESDI;
        strncpy(&mut lp.d_typename, b"ESDI/IDE disk");
    }
    // XXX - user viscopy() like sd.c
    strncpy(&mut lp.d_packname, &params.atap_model);
    lp.d_version = 1;

    lp.d_magic = DISKMAGIC;
    lp.d_magic2 = DISKMAGIC;
    lp.d_checksum = dkcksum(lp);
}

/// `wdgetdisklabel`: fabricates a default disk label, and tries to read the correct one.
pub fn wdgetdisklabel(
    dev: Dev,
    wd: &WdSoftc,
    lp: &mut Disklabel,
    spoofonly: bool,
) -> Result<(), Errno> {
    wdcdebug_print!(wdcdebug_wd_mask, DEBUG_FUNCS, "wdgetdisklabel\n");

    wdgetdefaultlabel(wd, lp);

    let drvp = wd.drvp();
    if drvp.state.get() > RECAL {
        drvp.set(crate::dev::ata::atavar::DRIVE_RESET);
    }

    // The label wdstrategy checks the reads below against (see the module's deviations).
    if wd.sc_dk.dk_openmask.get() == 0 {
        let mut incore = *lp;
        if initdisklabel(&mut incore).is_ok() {
            // SAFETY: no partition is open and the caller serialises label changes (the
            // disk lock in `wdopen`): nobody else holds the in-core label.
            if let Some(dl) = unsafe { wd.sc_dk.label_mut() } {
                *dl = incore;
            }
        }
    }

    let error = readdisklabel(disklabeldev(dev), wdstrategy, lp, spoofonly);
    if drvp.state.get() > RECAL {
        drvp.set(crate::dev::ata::atavar::DRIVE_RESET);
    }
    error
}

/// `*(struct disklabel *)addr = *lp`: the label into an `ioctl` buffer.
fn copyout_label(lp: &Disklabel, addr: &mut [u8]) {
    let n = addr.len().min(DISKLABEL_SIZE);
    addr[..n].copy_from_slice(&lp.as_bytes()[..n]);
}

/// `wdioctl`.
pub fn wdioctl(dev: Dev, xfer: u64, addr: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    wdcdebug_print!(wdcdebug_wd_mask, DEBUG_FUNCS, "wdioctl\n");

    let dv = wdlookup(diskunit(dev)).ok_or(ENXIO)?;
    let wd = wd_softc(dv);

    let error = if !wd.isset(WDF_LOADED) {
        Err(EIO)
    } else {
        wdioctl_loaded(wd, dev, xfer, addr, flag, p)
    };

    // exit:
    // SAFETY: the reference `wdlookup` took.
    unsafe { device_unref(dv) };
    error
}

/// The `switch` of [`wdioctl`], between the lookup and the `exit` label.
fn wdioctl_loaded(
    wd: &WdSoftc,
    dev: Dev,
    xfer: u64,
    addr: &mut [u8],
    flag: i32,
    p: &Proc,
) -> Result<(), Errno> {
    match xfer {
        DIOCRLDINFO => {
            let Some(mem) = malloc(size_of::<Disklabel>(), M_TEMP, M_WAITOK) else {
                panic(format_args!("wdioctl: malloc(M_WAITOK) failed"));
            };
            let lpp = mem.cast::<Disklabel>();
            // SAFETY: a fresh allocation the size of a label, written whole before the
            // reference is made, ours alone until it is freed below.
            let lp = unsafe {
                lpp.as_ptr().write(Disklabel::zeroed());
                &mut *lpp.as_ptr()
            };
            let _ = wdgetdisklabel(dev, wd, lp, false);
            // SAFETY: the driver's own label; no other reference to it is live.
            if let Some(dl) = unsafe { wd.sc_dk.label_mut() } {
                *dl = *lp;
            }
            free(mem, M_TEMP, size_of::<Disklabel>());
            Ok(())
        }

        DIOCGPDINFO => {
            let mut lp = Disklabel::zeroed();
            let _ = wdgetdisklabel(dev, wd, &mut lp, true);
            copyout_label(&lp, addr);
            Ok(())
        }

        DIOCGDINFO => {
            wd.sc_dk.with_label(|lp| {
                if let Some(lp) = lp {
                    copyout_label(lp, addr);
                }
            });
            Ok(())
        }

        DIOCGPART => {
            if let Some(lp) = wd.sc_dk.dk_label.get() {
                let pi = Partinfo {
                    disklab: lp.as_ptr(),
                    // SAFETY: `lp` is the live in-core label; the projection only computes
                    // the address of one of its partitions.
                    part: unsafe { &raw mut (*lp.as_ptr()).d_partitions[diskpart(dev) as usize] },
                };
                pi.store(addr);
            }
            Ok(())
        }

        DIOCWDINFO | DIOCSDINFO => {
            if flag & FWRITE == 0 {
                return Err(EBADF);
            }

            disk_lock(&wd.sc_dk)?;

            let mut nlp = Disklabel::from_bytes(addr);
            // SAFETY: under the disk lock; the borrow ends before the strategy runs.
            let mut error = match unsafe { wd.sc_dk.label_mut() } {
                Some(olp) => setdisklabel(olp, &mut nlp, wd.sc_dk.dk_openmask.get()),
                None => Err(ENXIO),
            };
            if error.is_ok() {
                let drvp = wd.drvp();
                if drvp.state.get() > RECAL {
                    drvp.set(crate::dev::ata::atavar::DRIVE_RESET);
                }
                if xfer == DIOCWDINFO {
                    let mut lp = wd.sc_dk.label().unwrap_or_default();
                    error = writedisklabel(disklabeldev(dev), wdstrategy, &mut lp);
                }
            }

            disk_unlock(&wd.sc_dk);
            error
        }

        DIOCCACHESYNC => {
            if flag & FWRITE == 0 {
                return Err(EBADF);
            }
            wd_flushcache(wd, AT_WAIT)
        }

        _ => wdc_ioctl(wd.drvp(), xfer, addr, flag, p),
    }
}

/// `wdsize`: the size of a partition in `DEV_BSIZE` blocks, -1 if it cannot be opened.
pub fn wdsize(dev: Dev) -> Daddr {
    wdcdebug_print!(wdcdebug_wd_mask, DEBUG_FUNCS, "wdsize\n");

    let Some(dv) = wdlookup(diskunit(dev)) else {
        return -1;
    };
    let wd = wd_softc(dv);

    let part = diskpart(dev);
    let omask = wd.sc_dk.dk_openmask.get() & (1u64 << part);

    let size = 'exit: {
        if omask == 0 && wdopen_noproc(dev, 0, S_IFBLK as i32).is_err() {
            break 'exit -1;
        }

        let mut size = wd.sc_dk.with_label(|lp| {
            let lp = lp.unwrap_or(&ZERO_LABEL);
            dl_sectoblk(lp, dl_getpsize(&lp.d_partitions[part as usize])) as Daddr
        });
        if omask == 0 && wdclose(dev, 0, S_IFBLK as i32, None).is_err() {
            size = -1;
        }
        size
    };

    // exit:
    // SAFETY: the reference `wdlookup` took.
    unsafe { device_unref(dv) };
    size
}

/// `wddump`: dumps core after a system crash: `size` bytes at `va` to block `blkno` of the
/// partition, by polled writes.
pub fn wddump(dev: Dev, blkno: Daddr, va: *mut u8, size: usize) -> Result<(), Errno> {
    // Check if recursive dump; if so, punt.
    if WDDOINGADUMP.load(Ordering::Relaxed) {
        return Err(EFAULT);
    }
    WDDOINGADUMP.store(true, Ordering::Relaxed);

    let unit = diskunit(dev);
    let dv = wdlookup(unit).ok_or(ENXIO)?;
    let wd = wd_softc(dv);

    let part = diskpart(dev);

    // Make sure it was initialized.
    let drvp = wd.drvp();
    if drvp.state.get() < READY {
        return Err(ENXIO);
    }

    // Convert to disk sectors. Request must be a multiple of size.
    let (secsize, psize, poffset) = wd.sc_dk.with_label(|lp| {
        let lp = lp.unwrap_or(&ZERO_LABEL);
        let p = &lp.d_partitions[part as usize];
        (lp.d_secsize as usize, dl_getpsize(p), dl_getpoffset(p))
    });
    if secsize == 0 || !size.is_multiple_of(secsize) {
        return Err(EFAULT);
    }
    let mut nblks = (size / secsize) as i32;
    let mut blkno = blkno / (secsize / DEV_BSIZE) as Daddr;

    // Check transfer bounds against partition size.
    if blkno < 0 || (blkno + Daddr::from(nblks)) as u64 > psize {
        return Err(EINVAL);
    }

    // Offset block number to start of partition.
    blkno += poffset as Daddr;

    // Recalibrate, if first dump transfer.
    if !WDDUMPRECALIBRATED.load(Ordering::Relaxed) {
        WDDUMPMULTI.store(wd.sc_wdc_bio.multi.get(), Ordering::Relaxed);
        WDDUMPRECALIBRATED.store(true, Ordering::Relaxed);
        drvp.state.set(RECAL);
    }

    let bio = &wd.sc_wdc_bio;
    let mut va = va;
    while nblks > 0 {
        let nwrt = nblks.min(WDDUMPMULTI.load(Ordering::Relaxed));
        bio.blkno.set(blkno);
        bio.flags.set(ATA_POLL);
        if wd.isset(WDF_LBA48) {
            bio.set(ATA_LBA48);
        }
        if wd.isset(WDF_LBA) {
            bio.set(ATA_LBA);
        }
        bio.bcount.set(i64::from(nwrt) * secsize as i64);
        bio.databuf.set(va);
        bio.wd.set(Some(NonNull::from(wd)));
        // #ifndef WD_DUMP_NOT_TRUSTED
        match wdc_ata_bio(drvp, bio) {
            WDC_TRY_AGAIN => panic(format_args!("wddump: try again")),
            WDC_QUEUED => panic(format_args!("wddump: polled command has been queued")),
            _ => {} // WDC_COMPLETE
        }
        let err = match bio.error.get() {
            TIMEOUT => {
                let _ = printf(format_args!("wddump: device timed out"));
                Err(EIO)
            }
            ERR_DF => {
                let _ = printf(format_args!("wddump: drive fault"));
                Err(EIO)
            }
            ERR_DMA => {
                let _ = printf(format_args!("wddump: DMA error"));
                Err(EIO)
            }
            ERROR => {
                let mut errbuf = [0u8; 256];
                ata_perror(drvp, i32::from(bio.r_error.get()), &mut errbuf);
                let _ = printf(format_args!("wddump: {}", Str(&errbuf)));
                Err(EIO)
            }
            NOERROR => Ok(()),
            _ => panic(format_args!("wddump: unknown error type")),
        };
        if err.is_err() {
            let _ = printf(format_args!("\n"));
            return err;
        }
        // #else WD_DUMP_NOT_TRUSTED: print the address, cylinder, head and sector, wait half
        // a second.

        // update block count
        nblks -= nwrt;
        blkno += Daddr::from(nwrt);
        va = va.wrapping_add(nwrt as usize * secsize);
    }

    WDDOINGADUMP.store(false, Ordering::Relaxed);
    Ok(())
}

/// `wd_get_params`: (re)reads the drive's IDENTIFY data into `wd->sc_params`; 1 when the
/// command could not be issued.
pub fn wd_get_params(wd: &WdSoftc, flags: u8) -> i32 {
    // SAFETY: the drive's attach, open (under the disk lock) and resume paths are
    // serialised, and nothing holds a reference to the block across them.
    let params = unsafe { wd.params_mut() };
    match ata_get_params(wd.drvp(), flags, params) {
        CMD_AGAIN => 1,
        CMD_ERR => {
            // If we already have drive parameters, reuse them.
            if params.atap_cylinders != 0 {
                return 0;
            }
            // We `know' there's a drive here; just assume it's old. This geometry is only
            // used to read the MBR and print a (false) attach message.
            *params = crate::dev::ata::atareg::Ataparams::zeroed();
            strncpy(&mut params.atap_model, b"ST506");
            params.atap_config = ATA_CFG_FIXED;
            params.atap_cylinders = 1024;
            params.atap_heads = 8;
            params.atap_sectors = 17;
            params.atap_multi = 1;
            params.atap_capabilities1 = 0;
            params.atap_capabilities2 = 0;
            wd.drvp().ata_vers.set(-1); // Mark it as pre-ATA
            0
        }
        CMD_OK => 0,
        _ => panic(format_args!(
            "wd_get_params: bad return code from ata_get_params"
        )),
    }
}

/// `wd_flushcache`: FLUSH CACHE (EXT).
pub fn wd_flushcache(wd: &WdSoftc, flags: u16) -> Result<(), Errno> {
    let drvp = wd.drvp();
    if drvp.ata_vers.get() < 4 {
        // WDCC_FLUSHCACHE is here since ATA-4
        return Err(EIO);
    }
    let wdc_c = WdcCommand::new();
    wdc_c.r_command.set(if wd.isset(WDF_LBA48) {
        WDCC_FLUSHCACHE_EXT
    } else {
        WDCC_FLUSHCACHE
    });
    wdc_c.r_st_bmask.set(WDCS_DRDY);
    wdc_c.r_st_pmask.set(WDCS_DRDY);
    wdc_c.flags.set(flags);
    wdc_c.timeout.set(30000); // 30s timeout
    if exec_on_stack(drvp, &wdc_c, "wd_flushcache") != WDC_COMPLETE {
        let _ = printf(format_args!(
            "{}: flush cache command didn't complete\n",
            wd.sc_dev.xname()
        ));
        return Err(EIO);
    }
    // The C tests the ata_bio error code ERR_NODEV against the command's AT_* flags.
    if wdc_c.flags.get() & ERR_NODEV as u16 != 0 {
        return Err(ENODEV);
    }
    if wdc_c.isset(AT_TIMEOU) {
        let _ = printf(format_args!(
            "{}: flush cache command timeout\n",
            wd.sc_dev.xname()
        ));
        return Err(EIO);
    }
    if wdc_c.isset(AT_ERROR) {
        if wdc_c.r_error.get() == WDCE_ABRT {
            // command not supported
            return Err(ENODEV);
        }
        let _ = printf(format_args!(
            "{}: flush cache command: error 0x{:x}\n",
            wd.sc_dev.xname(),
            wdc_c.r_error.get()
        ));
        return Err(EIO);
    }
    if wdc_c.isset(AT_DF) {
        let _ = printf(format_args!(
            "{}: flush cache command: drive fault\n",
            wd.sc_dev.xname()
        ));
        return Err(EIO);
    }
    Ok(())
}

/// `wd_standby`: STANDBY IMMEDIATE.
pub fn wd_standby(wd: &WdSoftc, flags: u16) {
    let wdc_c = WdcCommand::new();
    wdc_c.r_command.set(WDCC_STANDBY_IMMED);
    wdc_c.r_st_bmask.set(WDCS_DRDY);
    wdc_c.r_st_pmask.set(WDCS_DRDY);
    wdc_c.flags.set(flags);
    wdc_c.timeout.set(30000); // 30s timeout
    if exec_on_stack(wd.drvp(), &wdc_c, "wd_standby") != WDC_COMPLETE {
        let _ = printf(format_args!(
            "{}: standby command didn't complete\n",
            wd.sc_dev.xname()
        ));
    }
    if wdc_c.isset(AT_TIMEOU) {
        let _ = printf(format_args!(
            "{}: standby command timeout\n",
            wd.sc_dev.xname()
        ));
    }
    if wdc_c.isset(AT_DF) {
        let _ = printf(format_args!(
            "{}: standby command: drive fault\n",
            wd.sc_dev.xname()
        ));
    }
    // Ignore error register, it shouldn't report anything else than COMMAND ABORTED, which
    // means the device doesn't support standby
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use std::format;

    fn model(s: &[u8]) -> std::string::String {
        let mut m = [b' '; 40];
        m[..s.len()].copy_from_slice(s);
        let mut buf = [0u8; 41];
        wd_model(&m, &mut buf);
        format!("{}", Str(&buf))
    }

    #[test]
    fn model_blanks_collapse() {
        assert_eq!(model(b"QEMU HARDDISK"), "QEMU HARDDISK");
        assert_eq!(model(b"WDC  WD800JB-00JJC0"), "WDC WD800JB-00JJC0");
        // A leading run of blanks becomes one blank, as in the C.
        assert_eq!(model(b"  ST506"), " ST506");
        let mut m = [0u8; 40];
        m[..4].copy_from_slice(b"AB\0C");
        let mut buf = [0u8; 41];
        wd_model(&m, &mut buf);
        assert_eq!(format!("{}", Str(&buf)), "AB");
    }

    #[test]
    fn strncpy_pads_and_stops_at_nul() {
        let mut d = [b'x'; 8];
        strncpy(&mut d, b"ST506\0zz");
        assert_eq!(&d, b"ST506\0\0\0");
        let mut d = [0u8; 4];
        strncpy(&mut d, b"ESDI/IDE disk");
        assert_eq!(&d, b"ESDI");
    }
}
/* </TESTS> */
