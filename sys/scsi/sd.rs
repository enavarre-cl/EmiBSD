/*	$OpenBSD: sd.c,v 1.343 2026/06/24 17:03:06 krw Exp $	*/
/*	$NetBSD: sd.c,v 1.111 1997/04/02 02:29:41 mycroft Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1998, 2003, 2004 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum.
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

/*
 * Originally written by Julian Elischer (julian@dialix.oz.au)
 * for TRW Financial Systems for use under the MACH(2.5) operating system.
 *
 * TRW Financial Systems, in accordance with their agreement with Carnegie
 * Mellon University, makes this software available to CMU to distribute
 * or use in any manner that they see fit as long as this message is kept with
 * the software. For this reason TFS also grants any other persons or
 * organisations permission to use or modify this software.
 *
 * TFS supplies this software to be publicly redistributed
 * on the understanding that TFS is not responsible for the correct
 * functioning of this software in any circumstances.
 *
 * Ported to run under 386BSD by Julian Elischer (julian@dialix.oz.au) Sept 1992
 */
/* </LICENSES> */
//! sd(4): the SCSI disk driver. It attaches to every direct-access, reduced-direct-access
//! and optical device a `scsibus` finds, reads its capacity and geometry, and turns the
//! buffers of the block and character devices into READ and WRITE commands on its link.
//!
//! Upstream: sys/scsi/sd.c @ 3ce1f3f79392
//!
//! `sdattach` spins the disk up, reads its capacity (READ CAPACITY (10) or (16)), its thin
//! provisioning VPD pages and its geometry mode pages (`sd_get_parms`), prints the
//! `sd0: 64MB, 512 bytes/sector, 131072 sectors` line, turns the write cache on and attaches
//! the disk. The block (`bdevsw[4]`) and character (`cdevsw[13]`) entries open the disk
//! (loading its label through `readdisklabel`), queue buffers on the softc's `bufq`
//! (`sdstrategy`), and `sdstart`, the link's transfer handler, makes one SCSI transfer of
//! each, completed by `sd_buf_done`. `sdioctl` handles the disk label, cache and eject
//! commands and hands the rest to `scsi_do_ioctl`.
//!
//! ## Deviations
//! - `dma_alloc(9)` (`kern/dma_alloc.c`) is not ported. The fixed-size reply buffers
//!   (`scsi_read_cap_data*`, `scsi_vpd_*`, the mode sense buffer) are locals, as the callers
//!   of `scsi_inquire` already pass them: every command here is synchronous
//!   (`scsi_xs_sync`), so they outlive the transfer, and `bus_dmamap_load` reaches kernel
//!   stacks through `pmap_extract`. The allocation failures the C tests for them cannot
//!   happen. `sd_thin_pages`' page of variable length comes from `malloc(9)` (`M_TEMP`).
//! - The C's `-1` returns (`sd_read_cap*`, `sd_get_parms`) are `Err(EIO)`; every caller
//!   only tests for non-zero, and `sdopen` turns a failed `sd_get_parms` into `ENXIO` as the
//!   C does.
//! - The in-core label is handled as in `rd.rs`: `sdgetdisklabel` builds the label in a
//!   local (a Rust `&mut` may not alias the label `sdstrategy` and `sdstart` read), first
//!   publishing the initialised label when no partition is open (what the C's in-core label
//!   holds while `readdisklabel` reads into it), and `sdopen`/`DIOCRLDINFO` install the
//!   result. `DIOCWDINFO` writes a copy of the in-core label. `sdstart` and `sdminphys` read
//!   a copy of the label.
//! - `sdsize` opens the partition through [`sdopen_noproc`], `sdopen` without its thread
//!   argument, which `sdopen` never reads: `d_open` takes a `&Proc` and `sdsize` passes NULL.
//! - `sd_buf_done` reads the buffer from `xs->bp`, which `sdstart` sets to the same buffer
//!   as `xs->cookie` (the C reads the cookie).
//! - `viscpy` stops at the end of the source field: the C keeps reading past `len` source
//!   bytes while it skips unprintable ones.
//! - `caddr_t addr` of `sdioctl` is the kernel copy of the argument as a byte slice
//!   (`d_ioctl`'s type); the structures are read out of it and written back
//!   (`DIOCGCACHE`, `DIOCINQ`, ...), where the C changes them in place.
//! - `SCSIDEBUG` is not configured: the `SC_DEBUG` sites and `sd_get_parms`' geometry check
//!   are comments. `notyet` is not defined: `sd_vpd_thin`'s choice of a delete method
//!   (`sd_unmap`, `sd_write_same_16`, which the C does not have either) is a comment.
//!   `SD_DUMP_NOT_TRUSTED` is not defined: `sddump` writes.
//! - `sddump` is complete, but nothing calls it yet: `dumpsys` and `dumpconf` are not ported.

use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicBool, Ordering};

use libkern::strlcpy;

use crate::kern::init_main::BOOTHOWTO;
use crate::kern::kern_bufq::{
    bufq_dequeue, bufq_destroy, bufq_drain, bufq_init, bufq_peek, bufq_queue,
};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_physio::{minphys, physio};
use crate::kern::subr_autoconf::device_unref;
use crate::kern::subr_disk::{
    bounds_check_with_label, disk_attach, disk_busy, disk_closepart, disk_detach, disk_gone,
    disk_lock, disk_lock_nointr, disk_lookup, disk_openpart, disk_unbusy, disk_unlock, dkcksum,
    initdisklabel, setdisklabel,
};
use crate::kern::subr_prf::{Str, panic, printf, snprintf};
use crate::kern::vfs_bio::biodone;
use crate::machine::disklabel::{readdisklabel, writedisklabel};
use crate::machine::intr::{splbio, splx};
use crate::scsi::scsi_all::{
    PR_ALLOW, PR_PREVENT, READ_CAP_16_TPE, SENSE_NOT_READY_BECOMING_READY,
    SENSE_NOT_READY_INIT_REQUIRED, SI_PG_SERIAL, SI_PG_SUPPORTED, SID_TYPE, SKEY_NOT_READY,
    SMH_DSP_WRITE_PROT, SMS_PF, SSD_ERRCODE, SSD_ERRCODE_CURRENT, SSD_ERRCODE_DEFERRED, SSD_KEY,
    ScsiGeneric, ScsiModeSenseBuf, ScsiReadCapData, ScsiReadCapData16, ScsiVpdHdr, ScsiVpdSerial,
    ScsiWire, T_DIRECT, T_FIXED, T_OPTICAL, T_RDIRECT, T_REMOV, asc_ascq, wire_mut,
};
use crate::scsi::scsi_base::{
    scsi_delay, scsi_do_mode_sense, scsi_inquire_vpd, scsi_interpret_sense, scsi_mode_select,
    scsi_mode_select_big, scsi_parse_blkdesc, scsi_prevent, scsi_read_cap_10, scsi_read_cap_16,
    scsi_start, scsi_test_unit_ready, scsi_xs_exec, scsi_xs_get, scsi_xs_put, scsi_xs_sync,
    scsi_xsh_add, scsi_xsh_del, scsi_xsh_set,
};
use crate::scsi::scsi_disk::{
    PAGE_CACHING_MODE, PAGE_FLEX_GEOMETRY, PAGE_REDUCED_GEOMETRY, PAGE_RIGID_GEOMETRY,
    PG_CACHE_FL_RCD, PG_CACHE_FL_WCE, PageCachingMode, PageFlexGeometry, PageReducedGeometry,
    PageRigidGeometry, READ_10, READ_12, READ_16, READ_COMMAND, SI_PG_DISK_LIMITS,
    SI_PG_DISK_LIMITS_LEN_THIN, SI_PG_DISK_THIN, SSS_LOEJ, SSS_START, SSS_STOP, SYNCHRONIZE_CACHE,
    ScsiRw, ScsiRw10, ScsiRw12, ScsiRw16, ScsiSynchronizeCache, ScsiVpdDiskLimits, ScsiVpdDiskThin,
    WRITE_10, WRITE_12, WRITE_16, WRITE_COMMAND,
};
use crate::scsi::scsi_ioctl::scsi_do_ioctl;
use crate::scsi::scsiconf::{
    _2btol, _3btol, _4btol, _5btol, _8btol, _lto2b, _lto3b, _lto4b, _lto8b, SCSI_AUTOCONF,
    SCSI_DATA_IN, SCSI_DATA_OUT, SCSI_IGNORE_ILLEGAL_REQUEST, SCSI_IGNORE_MEDIA_CHANGE,
    SCSI_IGNORE_NOT_READY, SCSI_NOSLEEP, SCSI_REV_2, SCSI_REV_SPC2, SCSI_SILENT, SDEV_ATAPI,
    SDEV_EJECTING, SDEV_MEDIA_LOADED, SDEV_NOSYNCCACHE, SDEV_OPEN, SDEV_READONLY, SDEV_REMOVABLE,
    SDEV_UFI, SDEV_UMASS, ScsiAttachArgs, ScsiInquiryPattern, ScsiLink, ScsiXfer,
    TEST_READY_RETRIES, XS_BUSY, XS_NOERROR, XS_SENSE, XS_SHORTSENSE, XS_TIMEOUT, scsi_autoconf,
    scsi_inqmatch, scsi_strvis, sid_ansii_rev,
};
use crate::scsi::sdvar::{SDF_DIRTY, SDF_DYING, SDF_THIN, SdSoftc};
use crate::sys::buf::{B_ERROR, B_READ, B_WRITE, BUFQ_DEFAULT, BUFQ_FIFO, Buf};
use crate::sys::device::{
    CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DISK, DVACT_DEACTIVATE, DVACT_POWERDOWN,
    DVACT_RESUME, DVACT_SUSPEND, Device,
};
use crate::sys::disklabel::{
    DISKLABEL_SIZE, DISKMAGIC, DTYPE_FLOPPY, DTYPE_SCSI, Disklabel, FS_SWAP, Partinfo, RAW_PART,
    disklabeldev, diskpart, diskunit, dl_blkspersec, dl_blktosec, dl_getpoffset, dl_getpsize,
    dl_sectoblk, dl_setdsize,
};
use crate::sys::dkio::{
    DIOCCACHESYNC, DIOCEJECT, DIOCGCACHE, DIOCGDINFO, DIOCGPART, DIOCGPDINFO, DIOCINQ, DIOCLOCK,
    DIOCRLDINFO, DIOCSCACHE, DIOCSDINFO, DIOCWDINFO, DkCache, DkInquiry,
};
use crate::sys::errno::Errno::{self, *};
use crate::sys::fcntl::FWRITE;
use crate::sys::malloc::{M_NOWAIT, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::mtio::{MTIOCTOP, MTOFFL};
use crate::sys::param::{DEV_BSIZE, howmany};
use crate::sys::proc::Proc;
use crate::sys::reboot::RB_POWERDOWN;
use crate::sys::scsiio::{SCIOCCOMMAND, SCIOCDEBUG, SCIOCIDENTIFY};
use crate::sys::stat::{S_IFBLK, S_IFCHR};
use crate::sys::types::{Daddr, Dev};
use crate::sys::uio::Uio;

/// `NSD`: config(8)'s count for `sd* at scsibus?` (`sd.h`; `needs-flag`).
pub const NSD: i32 = 1;

/// `sd_ca`.
pub static SD_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<SdSoftc>(),
    ca_match: Some(sdmatch),
    ca_attach: sdattach,
    ca_detach: Some(sddetach),
    ca_activate: Some(sdactivate),
};

/// `sd_cd`.
pub static SD_CD: Cfdriver = Cfdriver::new(b"sd", DV_DISK, CD_COCOVM);

/// `sd_patterns`: the device types sd drives, fixed or removable.
pub static SD_PATTERNS: [ScsiInquiryPattern; 6] = [
    pattern(T_DIRECT, T_FIXED),
    pattern(T_DIRECT, T_REMOV),
    pattern(T_RDIRECT, T_FIXED),
    pattern(T_RDIRECT, T_REMOV),
    pattern(T_OPTICAL, T_FIXED),
    pattern(T_OPTICAL, T_REMOV),
];

/// `sddoingadump`: a dump is in progress (`SD_DUMP_NOT_TRUSTED` would only watch).
static SDDOINGADUMP: AtomicBool = AtomicBool::new(false);

/// `{type, removable, "", "", ""}`: an entry of [`SD_PATTERNS`].
const fn pattern(r#type: u8, removable: i32) -> ScsiInquiryPattern {
    ScsiInquiryPattern {
        r#type,
        removable,
        vendor: b"",
        product: b"",
        revision: b"",
    }
}

/// `sdlookup(unit)`: the attached unit, referenced (`disk_lookup`).
fn sdlookup(unit: u32) -> Option<NonNull<Device>> {
    disk_lookup(&SD_CD, i32::try_from(unit).ok()?)
}

/// `(struct sd_softc *)dv`.
fn sd_softc(dv: NonNull<Device>) -> &'static SdSoftc {
    // SAFETY: every `sd` device is an `SdSoftc` (`SD_CA.ca_devsize`) that autoconf allocated
    // and frees only after `sddetach`, once the references `sdlookup` takes are dropped;
    // `sddump` and the transfer paths run while the device is attached.
    unsafe { dv.as_ref().softc::<SdSoftc>() }
}

/// `link->device_softc`: the softc of the sd that `sdattach` made the link's driver.
fn sd_link_softc(link: &ScsiLink) -> &'static SdSoftc {
    match link.device_softc.get() {
        Some(dv) => sd_softc(dv),
        None => panic(format_args!("sd: scsi_link {:p} has no softc", link)),
    }
}

/// `ISSET(link->flags, f)`.
fn link_isset(link: &ScsiLink, f: u16) -> bool {
    link.flags.get() & f != 0
}

/// `SET(link->flags, f)`.
fn link_set(link: &ScsiLink, f: u16) {
    link.flags.set(link.flags.get() | f);
}

/// `CLR(link->flags, f)`.
fn link_clr(link: &ScsiLink, f: u16) {
    link.flags.set(link.flags.get() & !f);
}

/// `sdmatch`: the priority of the best [`SD_PATTERNS`] match for the device.
pub fn sdmatch(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: scsibus attaches its children with a `struct scsi_attach_args` as `aux`.
    let sa = unsafe { &*aux.cast::<ScsiAttachArgs>() };
    let inq = sa.sa_sc_link.inqdata.get();

    let (_, priority) = scsi_inqmatch(&inq, &SD_PATTERNS);

    priority
}

/// `sdattach`: the routine called by the low level scsi routine when it discovers a device
/// suitable for this driver.
pub fn sdattach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = sd_softc(NonNull::from(self_));
    // SAFETY: scsibus attaches its children with a `struct scsi_attach_args` as `aux`.
    let sa = unsafe { &*aux.cast::<ScsiAttachArgs>() };
    let link = sa.sa_sc_link;
    let mut sortby = BUFQ_DEFAULT;

    // SC_DEBUG(link, SDEV_DB2, ("sdattach:\n")): SCSIDEBUG is not configured.

    let sd_autoconf = scsi_autoconf.load(Ordering::Relaxed)
        | SCSI_SILENT
        | SCSI_IGNORE_ILLEGAL_REQUEST
        | SCSI_IGNORE_MEDIA_CHANGE;

    // Store information needed to contact our base driver.
    sc.sc_link.set(Some(link));
    link.interpret_sense.set(sd_interpret_sense);
    link.device_softc.set(Some(NonNull::from(self_)));

    if link_isset(link, SDEV_ATAPI) && link_isset(link, SDEV_REMOVABLE) {
        link.quirks.set(link.quirks.get() | SDEV_NOSYNCCACHE);
    }

    // Use the subdriver to request information regarding the drive. We cannot use
    // interrupts yet, so the request must specify this.
    printf(format_args!("\n"));

    scsi_xsh_set(&sc.sc_xsh, link, sdstart);

    // Spin up non-UMASS devices ready or not.
    if !link_isset(link, SDEV_UMASS) {
        let _ = scsi_start(link, i32::from(SSS_START), sd_autoconf);
    }

    // Some devices (e.g. BlackBerry Pearl) won't admit they have media loaded unless its
    // been locked in.
    if link_isset(link, SDEV_REMOVABLE) {
        let _ = scsi_prevent(link, i32::from(PR_PREVENT), sd_autoconf);
    }

    // Check that it is still responding and ok.
    let mut error = scsi_test_unit_ready(link, TEST_READY_RETRIES * 3, sd_autoconf);
    if error.is_ok() {
        error = sd_get_parms(sc, sd_autoconf);
    }

    if link_isset(link, SDEV_REMOVABLE) {
        let _ = scsi_prevent(link, i32::from(PR_ALLOW), sd_autoconf);
    }

    if error.is_ok() {
        let dp = sc.params.get();
        printf(format_args!(
            "{}: {}MB, {} bytes/sector, {} sectors",
            sc.sc_dev.xname(),
            dp.disksize / u64::from(1_048_576 / dp.secsize),
            dp.secsize,
            dp.disksize
        ));
        if sc.isset(SDF_THIN) {
            sortby = BUFQ_FIFO;
            printf(format_args!(", thin"));
        }
        if link_isset(link, SDEV_READONLY) {
            printf(format_args!(", readonly"));
        }
        printf(format_args!("\n"));
    }

    // Initialize disk structures.
    let mut name = [0u8; 16];
    let xname = sc.sc_dev.xname().as_bytes();
    let n = xname.len().min(name.len());
    name[..n].copy_from_slice(&xname[..n]);
    sc.sc_dk.dk_name.set(name);
    let _ = bufq_init(&sc.sc_bufq, sortby);

    // Enable write cache by default.
    let mut dkc = DkCache::default();
    if sd_ioctl_cache(sc, DIOCGCACHE, &mut dkc).is_ok() && dkc.wrcache == 0 {
        dkc.wrcache = 1;
        let _ = sd_ioctl_cache(sc, DIOCSCACHE, &mut dkc);
    }

    // Attach disk.
    disk_attach(Some(&sc.sc_dev), &sc.sc_dk);
}

/// `sdactivate`.
pub fn sdactivate(self_: &Device, act: i32) -> Result<(), Errno> {
    let sc = sd_softc(NonNull::from(self_));

    if sc.isset(SDF_DYING) {
        return Err(ENXIO);
    }
    let link = sc.link();

    match act {
        DVACT_SUSPEND => {
            // We flush the cache, since we our next step before DVACT_POWERDOWN might be a
            // hibernate operation.
            if sc.isset(SDF_DIRTY) {
                let _ = sd_flush(sc, SCSI_AUTOCONF);
            }
        }
        DVACT_POWERDOWN => {
            // Stop the disk. Stopping the disk should flush the cache, but we are paranoid
            // so we flush the cache first. We're cold at this point, so we poll for
            // completion.
            if sc.isset(SDF_DIRTY) {
                let _ = sd_flush(sc, SCSI_AUTOCONF);
            }
            if BOOTHOWTO.load(Ordering::Relaxed) & RB_POWERDOWN != 0 {
                let _ = scsi_start(
                    link,
                    i32::from(SSS_STOP),
                    SCSI_IGNORE_ILLEGAL_REQUEST | SCSI_IGNORE_NOT_READY | SCSI_AUTOCONF,
                );
            }
        }
        DVACT_RESUME => {
            let _ = scsi_start(
                link,
                i32::from(SSS_START),
                SCSI_IGNORE_ILLEGAL_REQUEST | SCSI_AUTOCONF,
            );
        }
        DVACT_DEACTIVATE => {
            sc.set(SDF_DYING);
            scsi_xsh_del(&sc.sc_xsh);
        }
        _ => {}
    }
    Ok(())
}

/// `sddetach`.
pub fn sddetach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = sd_softc(NonNull::from(self_));

    bufq_drain(&sc.sc_bufq);

    disk_gone(sdopen, self_.dv_unit.get() as u32);

    // Detach disk.
    bufq_destroy(&sc.sc_bufq);
    disk_detach(&sc.sc_dk);

    Ok(())
}

/// `sdopen`: opens the device. Make sure the partition info is as up-to-date as can be.
pub fn sdopen(dev: Dev, flag: i32, fmt: i32, _p: &Proc) -> Result<(), Errno> {
    sdopen_noproc(dev, flag, fmt)
}

/// [`sdopen`] without the thread, which it never reads (`sdsize` passes NULL).
pub fn sdopen_noproc(dev: Dev, flag: i32, fmt: i32) -> Result<(), Errno> {
    let unit = diskunit(dev);
    let part = diskpart(dev);

    let rawopen = part == RAW_PART && fmt == S_IFCHR as i32;

    let dv = sdlookup(unit).ok_or(ENXIO)?;
    let sc = sd_softc(dv);
    let unref = || {
        // SAFETY: the reference `sdlookup` took.
        unsafe { device_unref(dv) }
    };
    if sc.isset(SDF_DYING) {
        unref();
        return Err(ENXIO);
    }
    let link = sc.link();

    // SC_DEBUG(link, SDEV_DB1, ("sdopen: dev=0x%x (unit %d (of %d), partition %d)\n", ...)):
    // SCSIDEBUG is not configured.

    if flag & FWRITE != 0 && link_isset(link, SDEV_READONLY) {
        unref();
        return Err(EACCES);
    }
    if let Err(e) = disk_lock(&sc.sc_dk) {
        unref();
        return Err(e);
    }

    let error = 'die: {
        if sc.isset(SDF_DYING) {
            break 'die Err(ENXIO);
        }

        // `true`: go to `out`; `false`: go to `bad` with `error`.
        let mut error = Ok(());
        let out = if sc.sc_dk.dk_openmask.get() != 0 {
            // If any partition is open, but the disk has been invalidated, disallow further
            // opens of non-raw partition.
            if !link_isset(link, SDEV_MEDIA_LOADED) && !rawopen {
                error = Err(EIO);
                false
            } else {
                true
            }
        } else {
            // Spin up non-UMASS devices ready or not.
            if !link_isset(link, SDEV_UMASS) {
                let silent = if rawopen { SCSI_SILENT } else { 0 };
                let _ = scsi_start(
                    link,
                    i32::from(SSS_START),
                    silent | SCSI_IGNORE_ILLEGAL_REQUEST | SCSI_IGNORE_MEDIA_CHANGE,
                );
            }

            // Use sd_interpret_sense() for sense errors.
            //
            // But only after spinning the disk up! Just in case a broken device returns
            // "Initialization command required." and causes a loop of scsi_start() calls.
            if sc.isset(SDF_DYING) {
                break 'die Err(ENXIO);
            }
            link_set(link, SDEV_OPEN);

            // Try to prevent the unloading of a removable device while it's open. But allow
            // the open to proceed if the device can't be locked in.
            if link_isset(link, SDEV_REMOVABLE) {
                let _ = scsi_prevent(
                    link,
                    i32::from(PR_PREVENT),
                    SCSI_SILENT | SCSI_IGNORE_ILLEGAL_REQUEST | SCSI_IGNORE_MEDIA_CHANGE,
                );
            }

            // Check that it is still responding and ok.
            if sc.isset(SDF_DYING) {
                break 'die Err(ENXIO);
            }
            error = scsi_test_unit_ready(
                link,
                TEST_READY_RETRIES,
                SCSI_SILENT | SCSI_IGNORE_ILLEGAL_REQUEST | SCSI_IGNORE_MEDIA_CHANGE,
            );
            if error.is_err() {
                if rawopen {
                    error = Ok(());
                    true
                } else {
                    false
                }
            } else {
                // Load the physical device parameters.
                if sc.isset(SDF_DYING) {
                    break 'die Err(ENXIO);
                }
                link_set(link, SDEV_MEDIA_LOADED);
                if sd_get_parms(sc, if rawopen { SCSI_SILENT } else { 0 }).is_err() {
                    if sc.isset(SDF_DYING) {
                        break 'die Err(ENXIO);
                    }
                    link_clr(link, SDEV_MEDIA_LOADED);
                    error = Err(ENXIO);
                    false
                } else {
                    // SC_DEBUG(link, SDEV_DB3, ("Params loaded\n")).

                    // Load the partition info if not already loaded.
                    let mut lp = Disklabel::zeroed();
                    error = sdgetdisklabel(dev, sc, &mut lp, false);
                    // SAFETY: under the disk lock, with no partition open: nobody else
                    // holds the in-core label.
                    if let Some(dl) = unsafe { sc.sc_dk.label_mut() } {
                        *dl = lp;
                    }
                    // SC_DEBUG(link, SDEV_DB3, ("Disklabel loaded\n")).
                    !matches!(error, Err(EIO | ENXIO))
                }
            }
        };

        if out {
            // out:
            error = disk_openpart(&sc.sc_dk, part, fmt, true);
            // SC_DEBUG(link, SDEV_DB3, ("open complete\n")).

            // It's OK to fall through because dk_openmask is now non-zero.
        }

        // bad:
        if sc.sc_dk.dk_openmask.get() == 0 {
            if sc.isset(SDF_DYING) {
                break 'die Err(ENXIO);
            }
            if link_isset(link, SDEV_REMOVABLE) {
                let _ = scsi_prevent(
                    link,
                    i32::from(PR_ALLOW),
                    SCSI_SILENT | SCSI_IGNORE_ILLEGAL_REQUEST | SCSI_IGNORE_MEDIA_CHANGE,
                );
            }
            if sc.isset(SDF_DYING) {
                break 'die Err(ENXIO);
            }
            link_clr(link, SDEV_OPEN | SDEV_MEDIA_LOADED);
        }

        error
    };

    // die:
    disk_unlock(&sc.sc_dk);
    unref();
    error
}

/// `sdclose`: closes the device. Only called if we are the last occurrence of an open
/// device. Convenient now but usually a pain.
pub fn sdclose(dev: Dev, flag: i32, fmt: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let part = diskpart(dev);

    let dv = sdlookup(diskunit(dev)).ok_or(ENXIO)?;
    let sc = sd_softc(dv);
    let unref = || {
        // SAFETY: the reference `sdlookup` took.
        unsafe { device_unref(dv) }
    };
    if sc.isset(SDF_DYING) {
        unref();
        return Err(ENXIO);
    }
    let link = sc.link();

    disk_lock_nointr(&sc.sc_dk);

    disk_closepart(&sc.sc_dk, part, fmt);

    if (flag & FWRITE != 0 || sc.sc_dk.dk_openmask.get() == 0) && sc.isset(SDF_DIRTY) {
        let _ = sd_flush(sc, 0);
    }

    let error = 'die: {
        if sc.sc_dk.dk_openmask.get() == 0 {
            if sc.isset(SDF_DYING) {
                break 'die Err(ENXIO);
            }
            if link_isset(link, SDEV_REMOVABLE) {
                let _ = scsi_prevent(
                    link,
                    i32::from(PR_ALLOW),
                    SCSI_IGNORE_ILLEGAL_REQUEST | SCSI_IGNORE_NOT_READY | SCSI_SILENT,
                );
            }
            if sc.isset(SDF_DYING) {
                break 'die Err(ENXIO);
            }
            link_clr(link, SDEV_OPEN | SDEV_MEDIA_LOADED);

            if link_isset(link, SDEV_EJECTING) {
                let _ = scsi_start(link, i32::from(SSS_STOP | SSS_LOEJ), 0);
                if sc.isset(SDF_DYING) {
                    break 'die Err(ENXIO);
                }
                link_clr(link, SDEV_EJECTING);
            }

            scsi_xsh_del(&sc.sc_xsh);
        }
        Ok(())
    };

    // die:
    disk_unlock(&sc.sc_dk);
    unref();
    error
}

/// The zeroed label the I/O paths read before `disk_attach` allocated one.
static ZERO_LABEL: Disklabel = Disklabel::zeroed();

/// `sdstrategy`: actually translates the requested transfer into one the physical driver
/// can understand. The transfer is described by a buf and will include only one physical
/// transfer.
pub fn sdstrategy(bp: &'static Buf) {
    let dv = sdlookup(diskunit(bp.b_dev.get()));

    'done: {
        let error = 'bad: {
            let Some(dv) = dv else {
                break 'bad ENXIO;
            };
            let sc = sd_softc(dv);
            if sc.isset(SDF_DYING) {
                break 'bad ENXIO;
            }
            let link = sc.link();

            // SC_DEBUG(link, SDEV_DB2, ("sdstrategy: %ld bytes @ blk %lld\n", ...)).

            // If the device has been made invalid, error out.
            if !link_isset(link, SDEV_MEDIA_LOADED) {
                break 'bad if link_isset(link, SDEV_OPEN) {
                    EIO
                } else {
                    ENODEV
                };
            }

            // Validate the request.
            let ok = sc
                .sc_dk
                .with_label(|lp| bounds_check_with_label(bp, lp.unwrap_or(&ZERO_LABEL)));
            if !ok {
                break 'done;
            }

            // Place it in the queue of disk activities for this disk.
            bufq_queue(&sc.sc_bufq, bp);

            // Tell the device to get going on the transfer if it's not doing anything,
            // otherwise just wait for completion.
            scsi_xsh_add(&sc.sc_xsh);

            // SAFETY: the reference `sdlookup` took.
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
        // SAFETY: the reference `sdlookup` took.
        unsafe { device_unref(dv) };
    }
}

/// `sd_cmd_rw6`: READ (6) or WRITE (6) of `nsecs` sectors at `secno` into `generic`;
/// returns the command's length.
pub fn sd_cmd_rw6(generic: &mut ScsiGeneric, read: bool, secno: u64, nsecs: u32) -> i32 {
    let cmd: &mut ScsiRw = wire_mut(generic.as_bytes_mut());

    cmd.opcode = if read { READ_COMMAND } else { WRITE_COMMAND };
    _lto3b(secno as u32, &mut cmd.addr);
    cmd.length = nsecs as u8;

    size_of::<ScsiRw>() as i32
}

/// `sd_cmd_rw10`: READ (10) or WRITE (10).
pub fn sd_cmd_rw10(generic: &mut ScsiGeneric, read: bool, secno: u64, nsecs: u32) -> i32 {
    let cmd: &mut ScsiRw10 = wire_mut(generic.as_bytes_mut());

    cmd.opcode = if read { READ_10 } else { WRITE_10 };
    _lto4b(secno as u32, &mut cmd.addr);
    _lto2b(nsecs, &mut cmd.length);

    size_of::<ScsiRw10>() as i32
}

/// `sd_cmd_rw12`: READ (12) or WRITE (12).
pub fn sd_cmd_rw12(generic: &mut ScsiGeneric, read: bool, secno: u64, nsecs: u32) -> i32 {
    let cmd: &mut ScsiRw12 = wire_mut(generic.as_bytes_mut());

    cmd.opcode = if read { READ_12 } else { WRITE_12 };
    _lto4b(secno as u32, &mut cmd.addr);
    _lto4b(nsecs, &mut cmd.length);

    size_of::<ScsiRw12>() as i32
}

/// `sd_cmd_rw16`: READ (16) or WRITE (16).
pub fn sd_cmd_rw16(generic: &mut ScsiGeneric, read: bool, secno: u64, nsecs: u32) -> i32 {
    let cmd: &mut ScsiRw16 = wire_mut(generic.as_bytes_mut());

    cmd.opcode = if read { READ_16 } else { WRITE_16 };
    _lto8b(secno, &mut cmd.addr);
    _lto4b(nsecs, &mut cmd.length);

    size_of::<ScsiRw16>() as i32
}

/// `sdstart`: looks to see if there is a buf waiting for the device and that the device is
/// not already busy. If both are true, it dequeues the buf and creates a scsi command to
/// perform the transfer in the buf. The transfer request will call `scsi_done` on
/// completion, which will in turn call this routine again so that the next queued transfer
/// is performed. The bufs are queued by the strategy routine (`sdstrategy`).
///
/// This routine is also called after other non-queued requests have been made of the scsi
/// driver, to ensure that the queue continues to be drained.
pub fn sdstart(xs: &'static ScsiXfer) {
    let link = xs.link();
    let sc = sd_link_softc(link);

    if sc.isset(SDF_DYING) {
        scsi_xs_put(xs);
        return;
    }
    if !link_isset(link, SDEV_MEDIA_LOADED) {
        bufq_drain(&sc.sc_bufq);
        scsi_xs_put(xs);
        return;
    }

    let Some(bp) = bufq_dequeue(&sc.sc_bufq) else {
        scsi_xs_put(xs);
        return;
    };
    let read = bp.isset(B_READ);

    xs.flags
        .set(xs.flags.get() | if read { SCSI_DATA_IN } else { SCSI_DATA_OUT });
    xs.timeout.set(60000);
    // SAFETY: `b_data` maps `b_bcount` bytes of the busy buffer, which nobody else touches
    // until `sd_buf_done` hands it back with `biodone` after the transfer completed.
    unsafe { xs.set_data(bp.b_data.get(), bp.b_bcount.get() as i32) };
    xs.done.set(Some(sd_buf_done));
    xs.cookie.set(ptr::from_ref(bp).cast_mut().cast());
    xs.bp.set(Some(bp));

    let (secno, nsecs) = sc.sc_dk.with_label(|lp| {
        let lp = lp.unwrap_or(&ZERO_LABEL);
        let p = &lp.d_partitions[diskpart(bp.b_dev.get()) as usize];
        let secno = dl_getpoffset(p) + dl_blktosec(lp, bp.b_blkno.get() as u64);
        let nsecs = howmany(
            usize::try_from(bp.b_bcount.get()).unwrap_or(0),
            lp.d_secsize as usize,
        ) as u32;
        (secno, nsecs)
    });
    let disksize = sc.params.get().disksize;

    let cmdlen = xs.with_cmd(|cmd: &mut ScsiGeneric| {
        if !link_isset(link, SDEV_ATAPI | SDEV_UMASS)
            && sid_ansii_rev(&link.inqdata.get()) < SCSI_REV_2
            && (secno & 0x1f_ffff) == secno
            && (nsecs & 0xff) == nsecs
        {
            sd_cmd_rw6(cmd, read, secno, nsecs)
        } else if disksize > u64::from(u32::MAX) {
            sd_cmd_rw16(cmd, read, secno, nsecs)
        } else if nsecs <= u32::from(u16::MAX) {
            sd_cmd_rw10(cmd, read, secno, nsecs)
        } else {
            sd_cmd_rw12(cmd, read, secno, nsecs)
        }
    });
    xs.cmdlen.set(cmdlen);

    disk_busy(&sc.sc_dk);
    if !read {
        sc.set(SDF_DIRTY);
    }
    scsi_xs_exec(xs);

    // Move onto the next io.
    if bufq_peek(&sc.sc_bufq) {
        scsi_xsh_add(&sc.sc_xsh);
    }
}

/// `sd_buf_done`: the completion of an `sdstart` transfer: retries it, or finishes its
/// buffer.
pub fn sd_buf_done(xs: &'static ScsiXfer) {
    let sc = sd_link_softc(xs.link());
    let Some(bp) = xs.bp.get() else {
        panic(format_args!("sd_buf_done: xs {:p} has no buf", xs));
    };

    let ok = |bp: &Buf| {
        bp.b_error.set(None);
        bp.clr(B_ERROR);
        bp.b_resid.set(xs.resid.get());
    };

    // `None`: done; `Some(true)`: go to `retry`; `Some(false)`: the default case.
    let retry = match xs.error.get() {
        XS_NOERROR => {
            ok(bp);
            None
        }

        XS_SENSE | XS_SHORTSENSE => {
            // SC_DEBUG_SENSE(xs): SCSIDEBUG is not configured.
            match sd_interpret_sense(xs) {
                Ok(()) => {
                    ok(bp);
                    None
                }
                Err(error) => {
                    if error != ERESTART {
                        bp.b_error.set(Some(error));
                        bp.set(B_ERROR);
                        xs.retries.set(0);
                    }
                    Some(true)
                }
            }
        }

        XS_BUSY => {
            if xs.retries.get() != 0 && scsi_delay(xs, 1) != Err(ERESTART) {
                xs.retries.set(0);
            }
            Some(true)
        }

        XS_TIMEOUT => Some(true),

        _ => Some(false),
    };

    if let Some(retry) = retry {
        if retry {
            // retry:
            let retries = xs.retries.get();
            xs.retries.set(retries - 1);
            if retries != 0 {
                scsi_xs_exec(xs);
                return;
            }
        }
        // FALLTHROUGH, default:
        if bp.b_error.get().is_none() {
            bp.b_error.set(Some(EIO));
        }
        bp.set(B_ERROR);
        bp.b_resid
            .set(usize::try_from(bp.b_bcount.get()).unwrap_or(0));
    }

    disk_unbusy(
        &sc.sc_dk,
        bp.b_bcount.get() - xs.resid.get() as i64,
        bp.b_blkno.get(),
        bp.isset(B_READ),
    );

    let s = splbio();
    biodone(bp);
    splx(s);
    scsi_xs_put(xs);
}

/// `sdminphys`: trims a transfer to what the device and its adapter can do.
pub fn sdminphys(bp: &Buf) {
    let Some(dv) = sdlookup(diskunit(bp.b_dev.get())) else {
        return; // XXX - right way to fail this?
    };
    let sc = sd_softc(dv);
    if !sc.isset(SDF_DYING) {
        let link = sc.link();

        // If the device is ancient, we want to make sure that the transfer fits into a
        // 6-byte cdb.
        //
        // XXX Note that the SCSI-I spec says that 256-block transfers are allowed in a
        // 6-byte read/write, and are specified by setting the "length" to 0. However, we're
        // conservative here, allowing only 255-block transfers in case an ancient device
        // gets confused by length == 0. A length of 0 in a 10-byte read/write actually means
        // 0 blocks.
        if !link_isset(link, SDEV_ATAPI | SDEV_UMASS)
            && sid_ansii_rev(&link.inqdata.get()) < SCSI_REV_2
        {
            let secsize = sc.sc_dk.with_label(|lp| lp.map_or(0, |lp| lp.d_secsize));
            let max = i64::from(secsize) * 0xff;

            if bp.b_bcount.get() > max {
                bp.b_bcount.set(max);
            }
        }

        match link.bus().adapter().dev_minphys {
            Some(dev_minphys) => dev_minphys(bp, link),
            None => minphys(bp),
        }
    }

    // SAFETY: the reference `sdlookup` took.
    unsafe { device_unref(dv) };
}

/// `sdread`: the raw device's read, straight into the user's buffer (physio(9)).
pub fn sdread(dev: Dev, uio: &mut Uio<'_>, _ioflag: i32) -> Result<(), Errno> {
    physio(sdstrategy, dev, B_READ, sdminphys, uio)
}

/// `sdwrite`: the raw device's write, straight from the user's buffer (physio(9)).
pub fn sdwrite(dev: Dev, uio: &mut Uio<'_>, _ioflag: i32) -> Result<(), Errno> {
    physio(sdstrategy, dev, B_WRITE, sdminphys, uio)
}

/// `sdioctl`: performs special action on behalf of the user. Knows about the internals of
/// this device.
pub fn sdioctl(dev: Dev, cmd: u64, addr: &mut [u8], flag: i32, _p: &Proc) -> Result<(), Errno> {
    let part = diskpart(dev);

    let dv = sdlookup(diskunit(dev)).ok_or(ENXIO)?;
    let sc = sd_softc(dv);
    let result = if sc.isset(SDF_DYING) {
        Err(ENXIO)
    } else {
        sdioctl_locked(sc, dev, part, cmd, addr, flag)
    };

    // exit:
    // SAFETY: the reference `sdlookup` took.
    unsafe { device_unref(dv) };
    result
}

/// The body of [`sdioctl`], between the lookup and the `exit` label.
fn sdioctl_locked(
    sc: &'static SdSoftc,
    dev: Dev,
    part: u32,
    cmd: u64,
    addr: &mut [u8],
    flag: i32,
) -> Result<(), Errno> {
    let link = sc.link();

    // SC_DEBUG(link, SDEV_DB2, ("sdioctl 0x%lx\n", cmd)).

    // If the device is not valid, abandon ship.
    if !link_isset(link, SDEV_MEDIA_LOADED) {
        let raw_ok = matches!(
            cmd,
            DIOCLOCK | DIOCEJECT | SCIOCIDENTIFY | SCIOCCOMMAND | SCIOCDEBUG
        ) && part == RAW_PART;
        if !raw_ok {
            return Err(if !link_isset(link, SDEV_OPEN) {
                ENODEV
            } else {
                EIO
            });
        }
    }

    match cmd {
        DIOCRLDINFO => {
            let mut lp = Disklabel::zeroed();
            let _ = sdgetdisklabel(dev, sc, &mut lp, false);
            // SAFETY: the driver's own label; no other reference to it is live.
            if let Some(dl) = unsafe { sc.sc_dk.label_mut() } {
                *dl = lp;
            }
            Ok(())
        }

        DIOCGPDINFO => {
            let mut lp = Disklabel::zeroed();
            let _ = sdgetdisklabel(dev, sc, &mut lp, true);
            copyout_label(&lp, addr);
            Ok(())
        }

        DIOCGDINFO => {
            if let Some(lp) = sc.sc_dk.label() {
                copyout_label(&lp, addr);
            }
            Ok(())
        }

        DIOCGPART => {
            if let Some(lp) = sc.sc_dk.dk_label.get() {
                let pi = Partinfo {
                    disklab: lp.as_ptr(),
                    // SAFETY: `lp` is the live in-core label; the projection only computes
                    // the address of one of its partitions.
                    part: unsafe { &raw mut (*lp.as_ptr()).d_partitions[part as usize] },
                };
                pi.store(addr);
            }
            Ok(())
        }

        DIOCWDINFO | DIOCSDINFO => {
            if flag & FWRITE == 0 {
                return Err(EBADF);
            }

            disk_lock(&sc.sc_dk)?;

            let mut nlp = Disklabel::from_bytes(addr);
            // SAFETY: under the disk lock; the borrow ends before the strategy runs.
            let mut error = match unsafe { sc.sc_dk.label_mut() } {
                Some(olp) => setdisklabel(olp, &mut nlp, sc.sc_dk.dk_openmask.get()),
                None => Err(ENXIO),
            };
            if error.is_ok() && cmd == DIOCWDINFO {
                let mut lp = sc.sc_dk.label().unwrap_or_default();
                error = writedisklabel(disklabeldev(dev), sdstrategy, &mut lp);
            }

            disk_unlock(&sc.sc_dk);
            error
        }

        DIOCLOCK => {
            let r#type = if ioctl_int(addr) != 0 {
                PR_PREVENT
            } else {
                PR_ALLOW
            };
            scsi_prevent(link, i32::from(r#type), 0)
        }

        MTIOCTOP | DIOCEJECT => {
            if cmd == MTIOCTOP && mtop_op(addr) != MTOFFL {
                return Err(EIO);
            }
            // FALLTHROUGH
            if !link_isset(link, SDEV_REMOVABLE) {
                return Err(ENOTTY);
            }
            link_set(link, SDEV_EJECTING);
            Ok(())
        }

        DIOCINQ => match scsi_do_ioctl(link, cmd, addr, flag) {
            Err(ENOTTY) => {
                let mut di = dk_inquiry_zeroed();
                let error = sd_ioctl_inquiry(sc, &mut di);
                if error.is_ok() {
                    dk_inquiry_store(&di, addr);
                }
                error
            }
            error => error,
        },

        DIOCSCACHE | DIOCGCACHE => {
            if cmd == DIOCSCACHE && flag & FWRITE == 0 {
                return Err(EBADF);
            }
            // FALLTHROUGH
            let mut dkc = dk_cache_load(addr);
            let error = sd_ioctl_cache(sc, cmd, &mut dkc);
            dk_cache_store(&dkc, addr);
            error
        }

        DIOCCACHESYNC => {
            if flag & FWRITE == 0 {
                return Err(EBADF);
            }
            if sc.isset(SDF_DIRTY) || ioctl_int(addr) != 0 {
                sd_flush(sc, 0)
            } else {
                Ok(())
            }
        }

        _ => {
            if part != RAW_PART {
                return Err(ENOTTY);
            }
            scsi_do_ioctl(link, cmd, addr, flag)
        }
    }
}

/// `*(struct disklabel *)addr = *lp`: the label into an `ioctl` buffer.
fn copyout_label(lp: &Disklabel, addr: &mut [u8]) {
    let n = addr.len().min(DISKLABEL_SIZE);
    addr[..n].copy_from_slice(&lp.as_bytes()[..n]);
}

/// `*(int *)addr`.
fn ioctl_int(addr: &[u8]) -> i32 {
    let mut b = [0u8; 4];
    let n = addr.len().min(b.len());
    b[..n].copy_from_slice(&addr[..n]);
    i32::from_ne_bytes(b)
}

/// `((struct mtop *)addr)->mt_op`.
fn mtop_op(addr: &[u8]) -> i16 {
    let mut b = [0u8; 2];
    let n = addr.len().min(b.len());
    b[..n].copy_from_slice(&addr[..n]);
    i16::from_ne_bytes(b)
}

/// `*(struct dk_cache *)addr`, read.
fn dk_cache_load(addr: &[u8]) -> DkCache {
    let mut b = [0u8; size_of::<DkCache>()];
    let n = addr.len().min(b.len());
    b[..n].copy_from_slice(&addr[..n]);
    DkCache {
        wrcache: u32::from_ne_bytes([b[0], b[1], b[2], b[3]]),
        rdcache: u32::from_ne_bytes([b[4], b[5], b[6], b[7]]),
    }
}

/// `*(struct dk_cache *)addr`, written.
fn dk_cache_store(dkc: &DkCache, addr: &mut [u8]) {
    let mut b = [0u8; size_of::<DkCache>()];
    b[offset_of!(DkCache, wrcache)..][..4].copy_from_slice(&dkc.wrcache.to_ne_bytes());
    b[offset_of!(DkCache, rdcache)..][..4].copy_from_slice(&dkc.rdcache.to_ne_bytes());
    let n = addr.len().min(b.len());
    addr[..n].copy_from_slice(&b[..n]);
}

/// A `struct dk_inquiry` of zeros (`bzero`).
fn dk_inquiry_zeroed() -> DkInquiry {
    DkInquiry {
        vendor: [0; 64],
        product: [0; 128],
        revision: [0; 64],
        serial: [0; 64],
    }
}

/// `*(struct dk_inquiry *)addr`, written: four byte arrays, no padding.
fn dk_inquiry_store(di: &DkInquiry, addr: &mut [u8]) {
    let fields: [(usize, &[u8]); 4] = [
        (offset_of!(DkInquiry, vendor), &di.vendor),
        (offset_of!(DkInquiry, product), &di.product),
        (offset_of!(DkInquiry, revision), &di.revision),
        (offset_of!(DkInquiry, serial), &di.serial),
    ];
    for (off, bytes) in fields {
        if let Some(dst) = addr.get_mut(off..) {
            let n = dst.len().min(bytes.len());
            dst[..n].copy_from_slice(&bytes[..n]);
        }
    }
}

/// `sd_ioctl_inquiry`: `DIOCINQ` when the adapter has no answer: the identification
/// strings of the inquiry data and the unit serial number VPD page.
pub fn sd_ioctl_inquiry(sc: &SdSoftc, di: &mut DkInquiry) -> Result<(), Errno> {
    let mut vpd = ScsiVpdSerial::zeroed();

    if sc.isset(SDF_DYING) {
        return Err(ENXIO);
    }
    let link = sc.link();

    *di = dk_inquiry_zeroed();
    let inq = link.inqdata.get();
    scsi_strvis(&mut di.vendor, &inq.vendor);
    scsi_strvis(&mut di.product, &inq.product);
    scsi_strvis(&mut di.revision, &inq.revision);

    // the serial vpd page is optional
    if scsi_inquire_vpd(link, vpd.as_bytes_mut(), SI_PG_SERIAL, 0).is_ok() {
        scsi_strvis(&mut di.serial, &vpd.serial);
    } else {
        let n = vpd.serial.len();
        strlcpy(&mut di.serial[..n], b"(unknown)");
    }

    Ok(())
}

/// `sd_ioctl_cache`: `DIOCGCACHE`/`DIOCSCACHE` through the caching mode page, when the
/// adapter has no special handling.
pub fn sd_ioctl_cache(sc: &SdSoftc, cmd: u64, dkc: &mut DkCache) -> Result<(), Errno> {
    if sc.isset(SDF_DYING) {
        return Err(ENXIO);
    }
    let link = sc.link();

    if link_isset(link, SDEV_UMASS) {
        return Err(EOPNOTSUPP);
    }

    // See if the adapter has special handling.
    let mut raw = [0u8; size_of::<DkCache>()];
    dk_cache_store(dkc, &mut raw);
    let rv = scsi_do_ioctl(link, cmd, &mut raw, 0);
    if rv != Err(ENOTTY) {
        *dkc = dk_cache_load(&raw);
        return rv;
    }

    let mut buf = ScsiModeSenseBuf::new();

    if sc.isset(SDF_DYING) {
        return Err(ENXIO);
    }
    let flags = scsi_autoconf.load(Ordering::Relaxed) | SCSI_SILENT;
    let (mode, big) = match scsi_do_mode_sense(
        link,
        i32::from(PAGE_CACHING_MODE),
        &mut buf,
        (size_of::<PageCachingMode>() - 4) as i32,
        flags,
    ) {
        Ok((Some(mode), big)) => (mode, big),
        Ok((None, _)) => return Err(EIO),
        Err(e) => return Err(e),
    };

    // `mode->flags`, in place in the reply.
    let fl = mode + offset_of!(PageCachingMode, flags);
    let wrcache = u32::from(buf.buf[fl] & PG_CACHE_FL_WCE != 0);
    let rdcache = u32::from(buf.buf[fl] & PG_CACHE_FL_RCD == 0);

    match cmd {
        DIOCGCACHE => {
            dkc.wrcache = wrcache;
            dkc.rdcache = rdcache;
            Ok(())
        }

        DIOCSCACHE => {
            if dkc.wrcache == wrcache && dkc.rdcache == rdcache {
                return Ok(());
            }

            if dkc.wrcache != 0 {
                buf.buf[fl] |= PG_CACHE_FL_WCE;
            } else {
                buf.buf[fl] &= !PG_CACHE_FL_WCE;
            }

            if dkc.rdcache != 0 {
                buf.buf[fl] &= !PG_CACHE_FL_RCD;
            } else {
                buf.buf[fl] |= PG_CACHE_FL_RCD;
            }

            if sc.isset(SDF_DYING) {
                return Err(ENXIO);
            }
            if big {
                scsi_mode_select_big(link, i32::from(SMS_PF), &mut buf.buf, flags, 20000)
            } else {
                scsi_mode_select(link, i32::from(SMS_PF), &mut buf.buf, flags, 20000)
            }
        }

        _ => Ok(()),
    }
}

/// `sdgetdisklabel`: loads the label information on the named device.
pub fn sdgetdisklabel(
    dev: Dev,
    sc: &SdSoftc,
    lp: &mut Disklabel,
    spoofonly: bool,
) -> Result<(), Errno> {
    if sc.isset(SDF_DYING) {
        return Err(ENXIO);
    }
    let link = sc.link();
    let dp = sc.params.get();

    *lp = Disklabel::zeroed();

    lp.d_secsize = dp.secsize;
    lp.d_ntracks = dp.heads;
    lp.d_nsectors = dp.sectors;
    lp.d_ncylinders = dp.cyls;
    lp.d_secpercyl = lp.d_ntracks.wrapping_mul(lp.d_nsectors);
    if lp.d_secpercyl == 0 {
        lp.d_secpercyl = 100;
        // As long as it's not 0 - readdisklabel divides by it.
    }

    let inq = link.inqdata.get();
    if link_isset(link, SDEV_UFI) {
        lp.d_type = DTYPE_FLOPPY;
        strncpy(&mut lp.d_typename, b"USB floppy disk");
    } else {
        lp.d_type = DTYPE_SCSI;
        if inq.device & SID_TYPE == T_OPTICAL {
            strncpy(&mut lp.d_typename, b"SCSI optical");
        } else {
            strncpy(&mut lp.d_typename, b"SCSI disk");
        }
    }

    // Try to fit '<vendor> <product>' into d_packname. If that doesn't fit then leave out
    // '<vendor> ' and use only as much of '<product>' as does fit.
    let mut vendor = [0u8; 9];
    let mut product = [0u8; 17];
    viscpy(&mut vendor, &inq.vendor, 8);
    viscpy(&mut product, &inq.product, 16);
    let mut packname = [0u8; 17]; // sizeof(lp->d_packname) + 1
    let mut len = snprintf(
        &mut packname,
        format_args!("{} {}", Str(&vendor), Str(&product)),
    );
    if len > lp.d_packname.len() {
        strlcpy(&mut packname, &product);
        len = packname
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(packname.len());
    }
    // It is safe to use len as the count of characters to copy because packname is
    // sizeof(lp->d_packname)+1, the string in packname is always null terminated and len
    // does not count the terminating null. d_packname is not a null terminated string.
    lp.d_packname[..len].copy_from_slice(&packname[..len]);

    dl_setdsize(lp, dp.disksize);
    lp.d_version = 1;

    lp.d_magic = DISKMAGIC;
    lp.d_magic2 = DISKMAGIC;
    lp.d_checksum = dkcksum(lp);

    // The label sdstrategy checks the reads below against (see the module's deviations).
    if sc.sc_dk.dk_openmask.get() == 0 {
        let mut incore = *lp;
        if initdisklabel(&mut incore).is_ok() {
            // SAFETY: no partition is open and the caller serialises label changes (the
            // disk lock in `sdopen`): nobody else holds the in-core label.
            if let Some(dl) = unsafe { sc.sc_dk.label_mut() } {
                *dl = incore;
            }
        }
    }

    // Call the generic disklabel extraction routine.
    readdisklabel(disklabeldev(dev), sdstrategy, lp, spoofonly)
}

/// `strncpy(dst, src, sizeof(dst))`: `src`, then NULs to the end of `dst`.
fn strncpy(dst: &mut [u8], src: &[u8]) {
    let n = src.len().min(dst.len());
    dst[..n].copy_from_slice(&src[..n]);
    dst[n..].fill(0);
}

/// `sd_interpret_sense`: checks errors. Lets the generic code handle everything except a
/// few categories of LUN not ready errors on open devices.
pub fn sd_interpret_sense(xs: &'static ScsiXfer) -> Result<(), Errno> {
    let sense = xs.sense.get();
    let link = xs.link();
    let serr = sense.error_code & SSD_ERRCODE;

    // Let the generic code handle everything except a few categories of LUN not ready
    // errors on open devices.
    if !link_isset(link, SDEV_OPEN)
        || (serr != SSD_ERRCODE_CURRENT && serr != SSD_ERRCODE_DEFERRED)
        || sense.flags & SSD_KEY != SKEY_NOT_READY
        || sense.extra_len < 6
    {
        return scsi_interpret_sense(xs);
    }

    if xs.flags.get() & SCSI_IGNORE_NOT_READY != 0 {
        return Ok(());
    }

    match asc_ascq(&sense) {
        SENSE_NOT_READY_BECOMING_READY => {
            // SC_DEBUG(link, SDEV_DB1, ("becoming ready.\n")).
            scsi_delay(xs, 5)
        }

        SENSE_NOT_READY_INIT_REQUIRED => {
            // SC_DEBUG(link, SDEV_DB1, ("spinning up\n")).
            match scsi_start(
                link,
                i32::from(SSS_START),
                SCSI_IGNORE_ILLEGAL_REQUEST | SCSI_NOSLEEP,
            ) {
                Ok(()) => Err(ERESTART),
                // Can't issue the command. Fall back on a delay.
                Err(ENOMEM) => scsi_delay(xs, 5),
                // SC_DEBUG(link, SDEV_DB1, ("spin up failed (%#x)\n", retval)).
                Err(e) => Err(e),
            }
        }

        _ => scsi_interpret_sense(xs),
    }
}

/// `sdsize`: the size of a swap partition in `DEV_BSIZE` blocks, -1 for anything else.
pub fn sdsize(dev: Dev) -> Daddr {
    let Some(dv) = sdlookup(diskunit(dev)) else {
        return -1;
    };
    let sc = sd_softc(dv);

    let size = 'exit: {
        if sc.isset(SDF_DYING) {
            break 'exit -1;
        }

        let part = diskpart(dev);
        let omask = sc.sc_dk.dk_openmask.get() & (1u64 << part);

        if omask == 0 && sdopen_noproc(dev, 0, S_IFBLK as i32).is_err() {
            break 'exit -1;
        }

        let lp = sc.sc_dk.label().unwrap_or_default();
        if sc.isset(SDF_DYING) {
            break 'exit -1;
        }
        let p = &lp.d_partitions[part as usize];
        let mut size = if !link_isset(sc.link(), SDEV_MEDIA_LOADED) || p.p_fstype != FS_SWAP {
            -1
        } else {
            dl_sectoblk(&lp, dl_getpsize(p)) as Daddr
        };
        if omask == 0 && sdclose(dev, 0, S_IFBLK as i32, None).is_err() {
            size = -1;
        }
        size
    };

    // exit:
    // SAFETY: the reference `sdlookup` took.
    unsafe { device_unref(dv) };
    size
}

/// `sddump`: dumps all of physical memory into the partition specified, starting at offset
/// `dumplo` into the partition: `size` bytes at `va` to block `blkno`.
pub fn sddump(dev: Dev, blkno: Daddr, va: *mut u8, size: usize) -> Result<(), Errno> {
    // Check if recursive dump; if so, punt.
    if SDDOINGADUMP.load(Ordering::Relaxed) {
        return Err(EFAULT);
    }
    if blkno < 0 {
        return Err(EINVAL);
    }

    // Mark as active early.
    SDDOINGADUMP.store(true, Ordering::Relaxed);

    let unit = diskunit(dev); // Decompose unit & partition.
    let part = diskpart(dev);

    // Check for acceptable drive number.
    let Some(dv) = i32::try_from(unit).ok().and_then(|u| SD_CD.cd_dev(u)) else {
        return Err(ENXIO);
    };
    let sc = sd_softc(dv);

    // XXX Can't do this check, since the media might have been
    // XXX marked `invalid' by successful unmounting of all
    // XXX filesystems.
    // #if 0: make sure it was initialized (SDEV_MEDIA_LOADED, else ENXIO).

    // Convert to disk sectors. Request must be a multiple of size.
    let lp = sc.sc_dk.label().unwrap_or_default();
    let sectorsize = lp.d_secsize;
    if sectorsize == 0 || !size.is_multiple_of(sectorsize as usize) {
        return Err(EFAULT);
    }
    if !(blkno as u64).is_multiple_of(dl_blkspersec(&lp)) {
        return Err(EFAULT);
    }
    let mut totwrt = (size / sectorsize as usize) as u64; // sectors left
    let mut blkno = dl_blktosec(&lp, blkno as u64);

    let nsects = dl_getpsize(&lp.d_partitions[part as usize]); // partition sectors
    let sectoff = dl_getpoffset(&lp.d_partitions[part as usize]); // partition offset

    // Check transfer bounds against partition size.
    if blkno + totwrt > nsects {
        return Err(EINVAL);
    }

    // Offset block number to start of partition.
    blkno += sectoff;

    let mut va = va;
    while totwrt > 0 {
        // sectors to write
        let nwrt = totwrt.min(u64::from(u32::MAX)) as u32;

        // #ifndef SD_DUMP_NOT_TRUSTED
        let xs = scsi_xs_get(sc.link(), SCSI_NOSLEEP | SCSI_DATA_OUT).ok_or(ENOMEM)?;

        xs.timeout.set(10000);
        // SAFETY: `d_dump`'s contract: `va` maps the `size` bytes being dumped, which stay
        // mapped and unchanged (the machine is stopped) while the synchronous write runs.
        unsafe { xs.set_data(va, nwrt.wrapping_mul(sectorsize) as i32) };

        let cmdlen = xs.with_cmd(|cmd: &mut ScsiGeneric| sd_cmd_rw10(cmd, false, blkno, nwrt)); // XXX
        xs.cmdlen.set(cmdlen);

        let rv = scsi_xs_sync(xs);
        scsi_xs_put(xs);
        if rv.is_err() {
            return Err(ENXIO);
        }
        // #else SD_DUMP_NOT_TRUSTED: print the address and block, wait half a second.

        // Update block count.
        totwrt -= u64::from(nwrt);
        blkno += u64::from(nwrt);
        va = va.wrapping_add(sectorsize as usize * nwrt as usize);
    }

    SDDOINGADUMP.store(false, Ordering::Relaxed);

    Ok(())
}

/// `viscpy`: copies up to `len` chars from `src` to `dst`, ignoring non-printables. There
/// must be room for `len + 1` chars in `dst` so we can write the NUL. Does not assume `src`
/// is NUL-terminated.
pub fn viscpy(dst: &mut [u8], src: &[u8], len: usize) {
    let mut len = len;
    let mut d = 0;
    for &c in src {
        if len == 0 || c == b'\0' {
            break;
        }
        if !(0x20..0x80).contains(&c) {
            continue;
        }
        dst[d] = c;
        d += 1;
        len -= 1;
    }
    dst[d] = b'\0';
}

/// `sd_read_cap_10`: the capacity from READ CAPACITY (10).
pub fn sd_read_cap_10(sc: &SdSoftc, flags: i32) -> Result<(), Errno> {
    let mut rdcap = ScsiReadCapData::zeroed();

    if sc.isset(SDF_DYING) {
        return Err(EIO);
    }

    scsi_read_cap_10(sc.link(), &mut rdcap, flags)?;
    if _4btol(&rdcap.addr) == 0 {
        return Err(EIO);
    }
    let mut dp = sc.params.get();
    dp.disksize = u64::from(_4btol(&rdcap.addr)) + 1;
    dp.secsize = _4btol(&rdcap.length);
    sc.params.set(dp);
    sc.clr(SDF_THIN);

    Ok(())
}

/// `sd_read_cap_16`: the capacity from READ CAPACITY (16), and thin provisioning.
pub fn sd_read_cap_16(sc: &SdSoftc, flags: i32) -> Result<(), Errno> {
    let mut rdcap = ScsiReadCapData16::zeroed();

    if sc.isset(SDF_DYING) {
        return Err(EIO);
    }

    scsi_read_cap_16(sc.link(), &mut rdcap, flags)?;
    if _8btol(&rdcap.addr) == 0 {
        return Err(EIO);
    }
    let mut dp = sc.params.get();
    dp.disksize = _8btol(&rdcap.addr) + 1;
    dp.secsize = _4btol(&rdcap.length);
    sc.params.set(dp);
    if _2btol(&rdcap.lowest_aligned) & u32::from(READ_CAP_16_TPE) != 0 {
        sc.set(SDF_THIN);
    } else {
        sc.clr(SDF_THIN);
    }

    Ok(())
}

/// `sd_read_cap`: the capacity, with the READ CAPACITY the device is likely to know.
pub fn sd_read_cap(sc: &SdSoftc, flags: i32) -> Result<(), Errno> {
    let flags = flags & !SCSI_IGNORE_ILLEGAL_REQUEST;

    // post-SPC2 (i.e. post-SCSI-3) devices can start with 16 byte read capacity commands.
    // Older devices start with the 10 byte version and move up to the 16 byte version if
    // the device says it has more sectors than can be reported via the 10 byte read
    // capacity.
    if sid_ansii_rev(&sc.link().inqdata.get()) > SCSI_REV_SPC2 {
        sd_read_cap_16(sc, flags).or_else(|_| sd_read_cap_10(sc, flags))
    } else {
        let rv = sd_read_cap_10(sc, flags);
        if rv.is_ok() && sc.params.get().disksize == 0x1_0000_0000 {
            sd_read_cap_16(sc, flags)
        } else {
            rv
        }
    }
}

/// `sd_thin_pages`: whether the device has both VPD pages thin provisioning needs.
pub fn sd_thin_pages(sc: &SdSoftc, flags: i32) -> Result<(), Errno> {
    let mut pg = ScsiVpdHdr::zeroed();

    if sc.isset(SDF_DYING) {
        return Err(ENXIO);
    }
    scsi_inquire_vpd(sc.link(), pg.as_bytes_mut(), SI_PG_SUPPORTED, flags)?;

    let len = _2btol(&pg.page_length) as usize;

    let total = size_of::<ScsiVpdHdr>() + len;
    let wait = if flags & SCSI_NOSLEEP != 0 {
        M_NOWAIT
    } else {
        M_WAITOK
    };
    let mem = malloc(total, M_TEMP, wait | M_ZERO).ok_or(ENOMEM)?;
    // SAFETY: `total` zeroed bytes, ours alone until the `free` below.
    let pg = unsafe { slice::from_raw_parts_mut(mem.as_ptr(), total) };

    let rv = (|| {
        if sc.isset(SDF_DYING) {
            return Err(ENXIO);
        }
        scsi_inquire_vpd(sc.link(), pg, SI_PG_SUPPORTED, flags)?;

        let pages = &pg[size_of::<ScsiVpdHdr>()..];
        if pages.first() != Some(&SI_PG_SUPPORTED) {
            return Err(EIO);
        }

        let score = pages
            .iter()
            .skip(1)
            .filter(|&&p| p == SI_PG_DISK_LIMITS || p == SI_PG_DISK_THIN)
            .count();

        if score < 2 { Err(EOPNOTSUPP) } else { Ok(()) }
    })();

    free(mem, M_TEMP, total);
    rv
}

/// `sd_vpd_block_limits`: the unmap limits from the block limits VPD page.
pub fn sd_vpd_block_limits(sc: &SdSoftc, flags: i32) -> Result<(), Errno> {
    let mut pg = ScsiVpdDiskLimits::zeroed();

    if sc.isset(SDF_DYING) {
        return Err(ENXIO);
    }
    scsi_inquire_vpd(sc.link(), pg.as_bytes_mut(), SI_PG_DISK_LIMITS, flags)?;

    if _2btol(&pg.hdr.page_length) == u32::from(SI_PG_DISK_LIMITS_LEN_THIN) {
        let mut dp = sc.params.get();
        dp.unmap_sectors = _4btol(&pg.max_unmap_lba_count);
        dp.unmap_descs = _4btol(&pg.max_unmap_desc_count);
        sc.params.set(dp);
        Ok(())
    } else {
        Err(EOPNOTSUPP)
    }
}

/// `sd_vpd_thin`: reads the thin provisioning VPD page.
pub fn sd_vpd_thin(sc: &SdSoftc, flags: i32) -> Result<(), Errno> {
    let mut pg = ScsiVpdDiskThin::zeroed();

    if sc.isset(SDF_DYING) {
        return Err(ENXIO);
    }
    scsi_inquire_vpd(sc.link(), pg.as_bytes_mut(), SI_PG_DISK_THIN, flags)?;

    // #ifdef notyet: VPD_DISK_THIN_TPU picks sd_unmap as sc_delete, VPD_DISK_THIN_TPWS
    // sd_write_same_16 with one unmap descriptor (WRITE SAME 16 only does one), anything
    // else is EOPNOTSUPP.

    Ok(())
}

/// `sd_thin_params`: the thin provisioning parameters, when the device has them all.
pub fn sd_thin_params(sc: &SdSoftc, flags: i32) -> Result<(), Errno> {
    sd_thin_pages(sc, flags)?;

    sd_vpd_block_limits(sc, flags)?;

    sd_vpd_thin(sc, flags)?;

    Ok(())
}

/// `sd_get_parms`: fills out the disk parameter structure. `Ok` if the structure is
/// correctly filled in.
///
/// The caller is responsible for clearing the `SDEV_MEDIA_LOADED` flag if the structure
/// cannot be completed.
pub fn sd_get_parms(sc: &SdSoftc, flags: i32) -> Result<(), Errno> {
    let link = sc.link();

    if sd_read_cap(sc, flags).is_err() {
        return Err(EIO);
    }

    if sc.isset(SDF_THIN) && sd_thin_params(sc, flags).is_err() {
        // we don't know the unmap limits, so we can't use this shizz
        sc.clr(SDF_THIN);
    }

    // Work on a copy of the values initialized by sd_read_cap() and sd_thin_params().
    let mut dp = sc.params.get();

    let mut buf = ScsiModeSenseBuf::new();

    'validate: {
        if sc.isset(SDF_DYING) {
            return Err(EIO); // die
        }

        // Ask for page 0 (vendor specific) mode sense data to find READONLY info. The only
        // thing USB devices will ask for.
        //
        // page0 == NULL is a valid situation.
        let err = scsi_do_mode_sense(link, 0, &mut buf, 1, flags | SCSI_SILENT);
        if sc.isset(SDF_DYING) {
            return Err(EIO); // die
        }
        let mut big = false;
        if let Ok((_page0, b)) = err {
            big = b;
            if (big && buf.hdr_big().dev_spec & SMH_DSP_WRITE_PROT != 0)
                || (!big && buf.hdr().dev_spec & SMH_DSP_WRITE_PROT != 0)
            {
                link_set(link, SDEV_READONLY);
            } else {
                link_clr(link, SDEV_READONLY);
            }
        }

        // Many UMASS devices choke when asked about their geometry. Most don't have a
        // meaningful geometry anyway, so just fake it if sd_read_cap() worked.
        if link_isset(link, SDEV_UMASS) && dp.disksize > 0 {
            break 'validate;
        }

        match link.inqdata.get().device & SID_TYPE {
            T_OPTICAL => {
                // No more information needed or available.
            }

            T_RDIRECT => {
                // T_RDIRECT supports only PAGE_REDUCED_GEOMETRY (6).
                if let Ok((reduced, b)) = scsi_do_mode_sense(
                    link,
                    i32::from(PAGE_REDUCED_GEOMETRY),
                    &mut buf,
                    size_of::<PageReducedGeometry>() as i32,
                    flags | SCSI_SILENT,
                ) {
                    big = b;
                    scsi_parse_blkdesc(link, &buf, big, None, None, Some(&mut dp.secsize));
                    if let Some(off) = reduced {
                        let reduced = PageReducedGeometry::read_from(&buf.buf[off..]);
                        if dp.disksize == 0 {
                            dp.disksize = _5btol(&reduced.sectors);
                        }
                        if dp.secsize == 0 {
                            dp.secsize = _2btol(&reduced.bytes_s);
                        }
                    }
                }
            }

            _ => {
                // NOTE: Some devices leave off the last four bytes of PAGE_RIGID_GEOMETRY
                // and PAGE_FLEX_GEOMETRY mode sense pages. The only information in those
                // four bytes is RPM information so accept the page. The extra bytes will be
                // zero and RPM will end up with the default value of 3600.
                let err = if !link_isset(link, SDEV_ATAPI) || !link_isset(link, SDEV_REMOVABLE) {
                    scsi_do_mode_sense(
                        link,
                        i32::from(PAGE_RIGID_GEOMETRY),
                        &mut buf,
                        (size_of::<PageRigidGeometry>() - 4) as i32,
                        flags | SCSI_SILENT,
                    )
                } else {
                    Ok((None, big))
                };
                match err {
                    Ok((rigid, b)) => {
                        big = b;
                        scsi_parse_blkdesc(link, &buf, big, None, None, Some(&mut dp.secsize));
                        if let Some(off) = rigid {
                            let rigid = PageRigidGeometry::read_from(&buf.buf[off..]);
                            dp.heads = u32::from(rigid.nheads);
                            dp.cyls = _3btol(&rigid.ncyl);
                            let hc = dp.heads.wrapping_mul(dp.cyls);
                            if hc > 0 {
                                dp.sectors = (dp.disksize / u64::from(hc)) as u32;
                            }
                        }
                    }
                    Err(_) => {
                        if sc.isset(SDF_DYING) {
                            return Err(EIO); // die
                        }
                        if let Ok((flex, b)) = scsi_do_mode_sense(
                            link,
                            i32::from(PAGE_FLEX_GEOMETRY),
                            &mut buf,
                            (size_of::<PageFlexGeometry>() - 4) as i32,
                            flags | SCSI_SILENT,
                        ) {
                            big = b;
                            scsi_parse_blkdesc(link, &buf, big, None, None, Some(&mut dp.secsize));
                            if let Some(off) = flex {
                                let flex = PageFlexGeometry::read_from(&buf.buf[off..]);
                                dp.sectors = u32::from(flex.ph_sec_tr);
                                dp.heads = u32::from(flex.nheads);
                                dp.cyls = _2btol(&flex.ncyl);
                                if dp.secsize == 0 {
                                    dp.secsize = _2btol(&flex.bytes_s);
                                }
                                if dp.disksize == 0 {
                                    dp.disksize = u64::from(dp.cyls)
                                        * u64::from(dp.heads)
                                        * u64::from(dp.sectors);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // validate:
    if dp.disksize == 0 {
        return Err(EIO);
    }

    // Restrict secsize values to powers of two between 512 and 64k.
    match dp.secsize {
        0 => dp.secsize = DEV_BSIZE as u32,
        // 0x200 == 512, == DEV_BSIZE on all architectures.
        0x200 | 0x400 | 0x800 | 0x1000 | 0x2000 | 0x4000 | 0x8000 | 0x10000 => {}
        _ => {
            // SC_DEBUG(sc->sc_link, SDEV_DB1, ("sd_get_parms: bad secsize: %#x\n", ...)).
            return Err(EIO);
        }
    }

    // XXX THINK ABOUT THIS!! Using values such that sectors * heads * cyls is <= disk_size
    // can lead to wasted space. We need a more careful calculation/validation to make
    // everything work out optimally.
    if dp.disksize > 0xffff_ffff && dp.heads.wrapping_mul(dp.sectors) < 0xffff {
        dp.heads = 511;
        dp.sectors = 255;
        dp.cyls = 0;
    }

    // Use standard geometry values for anything we still don't know.
    if dp.heads == 0 {
        dp.heads = 255;
    }
    if dp.sectors == 0 {
        dp.sectors = 63;
    }
    if dp.cyls == 0 {
        dp.cyls = (dp.disksize / u64::from(dp.heads.wrapping_mul(dp.sectors))) as u32;
        if dp.cyls == 0 {
            // Put everything into one cylinder.
            dp.heads = 1;
            dp.cyls = 1;
            dp.sectors = dp.disksize as u32;
        }
    }

    // SCSIDEBUG (not configured): sc_print_addr and a warning when disksize differs from
    // cyls * heads * sectors/track.

    sc.params.set(dp);
    Ok(())
}

/// `sd_flush`: SYNCHRONIZE CACHE of the whole disk.
pub fn sd_flush(sc: &SdSoftc, flags: i32) -> Result<(), Errno> {
    if sc.isset(SDF_DYING) {
        return Err(ENXIO);
    }
    let link = sc.link();

    if link.quirks.get() & SDEV_NOSYNCCACHE != 0 {
        return Ok(());
    }

    // Issue a SYNCHRONIZE CACHE. Address 0, length 0 means "all remaining blocks starting
    // at address 0". Ignore ILLEGAL REQUEST in the event that the command is not supported
    // by the device.

    let Some(xs) = scsi_xs_get(link, flags | SCSI_IGNORE_ILLEGAL_REQUEST) else {
        // SC_DEBUG(link, SDEV_DB1, ("cache sync failed to get xs\n")).
        return Err(EIO);
    };

    xs.with_cmd(|cmd: &mut ScsiSynchronizeCache| cmd.opcode = SYNCHRONIZE_CACHE);

    xs.cmdlen.set(size_of::<ScsiSynchronizeCache>() as i32);
    xs.timeout.set(100000);

    let error = scsi_xs_sync(xs);

    scsi_xs_put(xs);

    if error.is_ok() {
        sc.clr(SDF_DIRTY);
    } else {
        // SC_DEBUG(link, SDEV_DB1, ("cache sync failed\n")).
    }

    error
}

#[cfg(test)]
mod tests;
