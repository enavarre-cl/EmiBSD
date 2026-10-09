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

/*-
 * Copyright (c) 1993, 1994, 1995, 1996 Charles Hannum.
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Don Ahn.
 *
 * Portions Copyright (c) 1993, 1994 by
 *  jc@irbs.UUCP (John Capo)
 *  vak@zebub.msk.su (Serge Vakulenko)
 *  ache@astral.msk.su (Andrew A. Chernov)
 *  joerg_wunsch@uriah.sax.de (Joerg Wunsch)
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)fd.c	7.4 (Berkeley) 5/25/91
 */
/* </LICENSES> */

/* <CODE> */
//! fd(4): the PC floppy disk driver, `dev/isa/fd.c`.
//!
//! Upstream: sys/dev/isa/fd.c @ 3ce1f3f79392
//!
//! `fd* at fdc?` probes each drive number fdc(4) offers (motor on, recalibrate, a sense
//! interrupt status that must say "seek end"), and attaches the drives that answer with the
//! density the NVRAM (or the config flags) gives. The block (`bdevsw[2]`) and character
//! (`cdevsw[9]`) devices queue buffers on the drive (`fdstrategy`); the controller runs one
//! drive's queue at a time through the state machine of `fdintr`: motor on, seek, an ISA DMA
//! transfer of at most a page (`isadma_start` on the controller's DRQ, then the 765's READ or
//! WRITE), the result, the next part; failures retry, recalibrate, reset and finally report
//! a hard error. The minor number names the unit, a density (0 = the drive's default) and
//! the partition (`fdreg.h`). The disklabel is a fictitious one for the density unless the
//! disk carries one (`readdisklabel`).
//!
//! It exists where cfg `machine_x86` is set (amd64): `struct fd_type` and the NVRAM types are
//! x86 machine items (`machine::x86`, `docs/ARCHITECTURE.md`).
//!
//! ## Deviations
//! - The softc's members are `Cell`s, changed at `splbio` as in the C.
//! - The in-core label is handled as in `wd.rs`: `fdgetdisklabel` fills a local label that
//!   `fdopen` and `DIOCRLDINFO` install (fdstrategy does not read the label, so nothing
//!   differs while `readdisklabel` runs).
//! - The unit lookups (`fd_cd.cd_devs[FDUNIT(dev)]`) panic for a unit with no device where
//!   the C dereferences NULL (`fdopen` checks, as the C).
//! - `B_FORMAT` is `B_XXX`, as the C's "misuse"; `fdformat` takes the `FD_FORM` argument's
//!   kernel copy as bytes (the buffer the transfer reads) and reads the request from it with
//!   `read_unaligned`.
//! - `isadma_start`'s result is ignored, as the C ignores it.
//! - `caddr_t addr` of `fdioctl` is the kernel copy of the argument as a byte slice
//!   (`d_ioctl`'s type), as in `wdioctl`. `FD_STYPE` is defined but not handled (`ENOTTY`),
//!   as in the C.
//! - `DIAGNOSTIC`'s checks (`fdstrategy`'s inactive controller, `fdintr`'s block check and
//!   its "impossible" panic) are under feature `diagnostic`; `FD_DEBUG` and `DEBUG` are not
//!   defined.
//! - `fdsize` returns -1 and `fddump` `ENXIO`, as the C (swapping to floppies would not make
//!   sense; dumping is not implemented).
use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};

use crate::dev::isa::fdc::{fdcresult, fdcstart, fdcstatus, out_fdc};
use crate::dev::isa::fdlink::{
    DEVIDLE, DOIO, DORECAL, DORESET, DOSEEK, FDC_TYPE_DISK, FdcAttachArgs, FdcSoftc, IOCOMPLETE,
    IOTIMEDOUT, MOTORWAIT, RECALCOMPLETE, RECALTIMEDOUT, RECALWAIT, RESETCOMPLETE, RESETTIMEDOUT,
    SEEKCOMPLETE, SEEKTIMEDOUT, SEEKWAIT,
};
use crate::dev::isa::fdreg::{
    FDC_250KBPS, FDC_300KBPS, FDC_500KBPS, FDC_MAXIOSIZE, FDO_FDMAEN, FDO_FRST, NE7_ST0BITS,
    NE7_ST1BITS, NE7_ST2BITS, NE7CMD_FORMAT, NE7CMD_READ, NE7CMD_RECAL, NE7CMD_SEEK, NE7CMD_SENSEI,
    NE7CMD_SPECIFY, NE7CMD_WRITE, fd_bsize, fdctl, fdo_moen, fdout, fdpart, fdtype, fdunit,
};
use crate::dev::isa::isadmavar::{
    DMAMODE_READ, DMAMODE_WRITE, isadma_abort, isadma_done, isadma_start,
};
use crate::kern::kern_bufq::{bufq_dequeue, bufq_init, bufq_queue};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_physio::{minphys, physio};
use crate::kern::kern_synch::tsleep_nsec;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_add_sec, timeout_del, timeout_set};
use crate::kern::subr_disk::{disk_attach, disk_busy, disk_unbusy, diskerr, dkcksum, setdisklabel};
use crate::kern::subr_prf::{Bitmask, panic, printf};
use crate::kern::vfs_bio::{biodone, biowait};
use crate::machine::bus::bus_space_write_1;
use crate::machine::cpu::delay;
use crate::machine::disklabel::{readdisklabel, writedisklabel};
use crate::machine::intr::{IPL_BIO, splassert, splbio, splx};
use crate::machine::x86::{
    FD_FORM, FD_FORMAT_VERSION, FD_GOPTS, FD_GTYPE, FD_SOPTS, FDOPT_NORETRY, FdFormData, FdFormb,
    FdIdfieldData, FdType, NVRAM_DISKETTE_12M, NVRAM_DISKETTE_144M, NVRAM_DISKETTE_360K,
    NVRAM_DISKETTE_720K, NVRAM_DISKETTE_NONE, NVRAM_DISKETTE_TYPE5, NVRAM_DISKETTE_TYPE6,
};
use crate::queue_adapter;
use crate::sys::buf::{
    B_BUSY, B_ERROR, B_PHYS, B_RAW, B_READ, B_WRITE, B_XXX, BUFQ_DEFAULT, Buf, Bufq,
};
use crate::sys::device::{
    CfMatch, Cfattach, Cfdriver, DV_DISK, DVACT_POWERDOWN, DVACT_SUSPEND, Device, Softc,
};
use crate::sys::disk::{DKF_NOLABELREAD, Disk};
use crate::sys::disklabel::{
    DISKLABEL_SIZE, DISKMAGIC, DTYPE_FLOPPY, Disklabel, Partinfo, disklabeldev, dl_setdsize,
};
use crate::sys::dkio::{DIOCGDINFO, DIOCGPART, DIOCGPDINFO, DIOCRLDINFO, DIOCSDINFO, DIOCWDINFO};
use crate::sys::errno::Errno::{self, *};
use crate::sys::fcntl::FWRITE;
use crate::sys::malloc::{M_NOWAIT, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::mtio::{MTIOCTOP, MTOFFL};
use crate::sys::param::{DEV_BSHIFT, DEV_BSIZE, howmany};
use crate::sys::proc::Proc;
use crate::sys::queue::TailqEntry;
use crate::sys::stat::{S_IFBLK, S_IFCHR};
use crate::sys::syslog::LOG_PRINTF;
use crate::sys::time::msec_to_nsec;
use crate::sys::timeout::Timeout;
use crate::sys::types::{Daddr, Dev, Mode};
use crate::sys::uio::Uio;

/// `B_FORMAT`: XXX misuse a flag to identify format operation.
const B_FORMAT: i64 = B_XXX;

/// `FD_OPEN`: it's open.
pub const FD_OPEN: i32 = 0x01;
/// `FD_MOTOR`: motor should be on.
pub const FD_MOTOR: i32 = 0x02;
/// `FD_MOTOR_WAIT`: motor coming up.
pub const FD_MOTOR_WAIT: i32 = 0x04;

/// `struct fd_softc`: software state, per disk (with up to 4 disks per ctlr).
#[repr(C)]
pub struct FdSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_dk`.
    pub sc_dk: Disk,

    /// `sc_deftype`: default type descriptor.
    pub sc_deftype: Cell<Option<&'static FdType>>,
    /// `sc_type`: current type descriptor.
    pub sc_type: Cell<Option<&'static FdType>>,

    /// `sc_blkno`: starting block number.
    pub sc_blkno: Cell<Daddr>,
    /// `sc_bcount`: byte count left.
    pub sc_bcount: Cell<i32>,
    /// `sc_opts`: user-set options.
    pub sc_opts: Cell<i32>,
    /// `sc_skip`: bytes already transferred.
    pub sc_skip: Cell<i32>,
    /// `sc_nblks`: number of blocks currently transferring.
    pub sc_nblks: Cell<i32>,
    /// `sc_nbytes`: number of bytes currently transferring.
    pub sc_nbytes: Cell<i32>,

    /// `sc_drive`: physical unit number.
    pub sc_drive: Cell<i32>,
    /// `sc_flags` (`FD_*`).
    pub sc_flags: Cell<i32>,
    /// `sc_cylin`: where we think the head is.
    pub sc_cylin: Cell<i32>,

    /// `sc_drivechain`.
    pub sc_drivechain: TailqEntry<FdSoftc>,
    /// `sc_ops`: I/O ops since last switch.
    pub sc_ops: Cell<i32>,
    /// `sc_bufq`: pending I/O.
    pub sc_bufq: Bufq,
    /// `sc_bp`: the current I/O.
    pub sc_bp: Cell<Option<&'static Buf>>,
    /// `fd_motor_on_to`.
    pub fd_motor_on_to: Timeout,
    /// `fd_motor_off_to`.
    pub fd_motor_off_to: Timeout,
    /// `fdtimeout_to`.
    pub fdtimeout_to: Timeout,
}

impl FdSoftc {
    /// `fd->sc_type`, set by `fdopen` before any transfer. Panics where the C would
    /// dereference NULL.
    fn type_(&self) -> &'static FdType {
        match self.sc_type.get() {
            Some(t) => t,
            None => panic(format_args!("{}: no type", self.sc_dev.xname())),
        }
    }

    /// `(struct fdc_softc *)fd->sc_dev.dv_parent`.
    fn fdc(&self) -> &'static FdcSoftc {
        match self.sc_dev.dv_parent.get() {
            // SAFETY: an fd(4)'s parent is its fdc(4), an `FdcSoftc` that lives as long as
            // the kernel.
            Some(p) => unsafe { &*p.as_ptr().cast::<FdcSoftc>() },
            None => panic(format_args!("{}: no controller", self.sc_dev.xname())),
        }
    }
}

// SAFETY: `repr(C)` with the device first; all-zero is valid for every member (a zeroed
// disk and bufq as `disk_attach` and `bufq_init` expect, `None` cells, null links, zeroed
// timeouts for `timeout_set`).
unsafe impl Softc for FdSoftc {}

queue_adapter!(
    /// `TAILQ_HEAD(drivehead, fd_softc)`: the controller's drive queue, through
    /// `sc_drivechain`.
    pub FdDrivechain: FdSoftc, sc_drivechain => TailqEntry<FdSoftc>
);

/// The queue type of `sc_drives`, for its `next`.
type FdDrivechainHead = crate::sys::queue::TailqHead<FdDrivechain>;

/// Where `fdintr`'s `switch` goes on: the C's labels.
enum Next {
    /// `doseek`.
    DoSeek,
    /// `doio`.
    DoIo,
}

use core::cell::Cell;

/// `fd_types[]`: the order of entries in the following table is important -- BEWARE!
pub static FD_TYPES: [FdType; 9] = [
    // 1.44MB diskette
    fd_type(
        18,
        2,
        36,
        2,
        0xff,
        0xcf,
        0x1b,
        0x6c,
        80,
        2880,
        1,
        FDC_500KBPS,
        c"1.44MB",
    ),
    // 1.2 MB AT-diskettes
    fd_type(
        15,
        2,
        30,
        2,
        0xff,
        0xdf,
        0x1b,
        0x54,
        80,
        2400,
        1,
        FDC_500KBPS,
        c"1.2MB",
    ),
    // 360kB in 1.2MB drive
    fd_type(
        9,
        2,
        18,
        2,
        0xff,
        0xdf,
        0x23,
        0x50,
        40,
        720,
        2,
        FDC_300KBPS,
        c"360KB/AT",
    ),
    // 360kB PC diskettes
    fd_type(
        9,
        2,
        18,
        2,
        0xff,
        0xdf,
        0x2a,
        0x50,
        40,
        720,
        1,
        FDC_250KBPS,
        c"360KB/PC",
    ),
    // 3.5" 720kB diskette
    fd_type(
        9,
        2,
        18,
        2,
        0xff,
        0xdf,
        0x2a,
        0x50,
        80,
        1440,
        1,
        FDC_250KBPS,
        c"720KB",
    ),
    // 720kB in 1.2MB drive
    fd_type(
        9,
        2,
        18,
        2,
        0xff,
        0xdf,
        0x23,
        0x50,
        80,
        1440,
        1,
        FDC_300KBPS,
        c"720KB/x",
    ),
    // 360kB in 720kB drive
    fd_type(
        9,
        2,
        18,
        2,
        0xff,
        0xdf,
        0x2a,
        0x50,
        40,
        720,
        2,
        FDC_250KBPS,
        c"360KB/x",
    ),
    // 2.88MB diskette
    fd_type(
        36,
        2,
        72,
        2,
        0xff,
        0xaf,
        0x1b,
        0x54,
        80,
        5760,
        1,
        FDC_500KBPS,
        c"2.88MB",
    ),
    // 1.2 MB japanese format
    fd_type(
        8,
        2,
        16,
        3,
        0xff,
        0xdf,
        0x35,
        0x74,
        77,
        1232,
        1,
        FDC_500KBPS,
        c"1.2MB/[1024bytes/sector]",
    ),
];

/// `fd_ca`.
pub static FD_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<FdSoftc>(),
    ca_match: Some(fdprobe),
    ca_attach: fdattach,
    ca_detach: None,
    ca_activate: Some(fdactivate),
};

/// `fd_cd`.
pub static FD_CD: Cfdriver = Cfdriver::new(b"fd", DV_DISK, 0);

/// One `fd_types[]` initialiser, in the C's member order.
#[allow(clippy::too_many_arguments)] // the C's aggregate initialiser
const fn fd_type(
    sectrac: i32,
    heads: i32,
    seccyl: i32,
    secsize: i32,
    datalen: i32,
    steprate: i32,
    gap1: i32,
    gap2: i32,
    tracks: i32,
    size: i32,
    step: i32,
    rate: i32,
    name: &'static core::ffi::CStr,
) -> FdType {
    FdType {
        sectrac,
        heads,
        seccyl,
        secsize,
        datalen,
        steprate,
        gap1,
        gap2,
        tracks,
        size,
        step,
        rate,
        name: name.as_ptr(),
    }
}

/// `fd_cd.cd_devs[unit]`: the attached drive, if any.
fn fd_lookup(unit: u32) -> Option<&'static FdSoftc> {
    let dv: NonNull<Device> = FD_CD.cd_dev(i32::try_from(unit).ok()?)?;
    // SAFETY: every `fd` device is an `FdSoftc` (`FD_CA.ca_devsize`), never detached.
    Some(unsafe { &*dv.as_ptr().cast::<FdSoftc>() })
}

/// `fd_cd.cd_devs[FDUNIT(dev)]` for a device that must exist (it was opened).
fn fd_softc(dev: Dev) -> &'static FdSoftc {
    match fd_lookup(fdunit(dev)) {
        Some(fd) => fd,
        None => panic(format_args!("fd: no unit {}", fdunit(dev))),
    }
}

/// `strncpy(dst, src, sizeof(dst))`.
fn strncpy(dst: &mut [u8], src: &[u8]) {
    let n = src.len().min(dst.len());
    dst[..n].copy_from_slice(&src[..n]);
    dst[n..].fill(0);
}

/// `fdgetdisklabel`: the fictitious label of the drive's current density, then the disk's
/// own if it has one.
pub fn fdgetdisklabel(
    dev: Dev,
    fd: &FdSoftc,
    lp: &mut Disklabel,
    spoofonly: bool,
) -> Result<(), Errno> {
    let ty = fd.type_();
    *lp = Disklabel::zeroed();

    lp.d_type = DTYPE_FLOPPY;
    lp.d_secsize = fd_bsize(ty.secsize) as u32;
    lp.d_secpercyl = ty.seccyl as u32;
    lp.d_nsectors = ty.sectrac as u32;
    lp.d_ncylinders = ty.tracks as u32;
    lp.d_ntracks = ty.heads as u32; // Go figure...
    dl_setdsize(lp, ty.size as u64);

    strncpy(&mut lp.d_typename, b"floppy disk");
    strncpy(&mut lp.d_packname, b"fictitious");
    lp.d_version = 1;

    lp.d_magic = DISKMAGIC;
    lp.d_magic2 = DISKMAGIC;
    lp.d_checksum = dkcksum(lp);

    // Call the generic disklabel extraction routine. If there's not a label there, fake
    // it.
    readdisklabel(disklabeldev(dev), fdstrategy, lp, spoofonly)
}

/// `fdprobe`: a drive that recalibrates to track 0.
pub fn fdprobe(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let Some(parent) = parent else {
        return 0;
    };
    // SAFETY: fd's parent is an fdc, whose softc is an `FdcSoftc`.
    let fdc = unsafe { parent.softc::<FdcSoftc>() };
    let cf = match_.cfdata();
    // SAFETY: `fdcattach_deferred` probes its children with an `fdc_attach_args`.
    let fa = unsafe { &mut *aux.cast::<FdcAttachArgs>() };
    let drive = fa.fa_drive;
    let iot = fdc.iot();
    let ioh = fdc.ioh();

    let loc0 = cf.cf_loc.first().copied().unwrap_or(-1);
    if loc0 != -1 && loc0 != i64::from(drive) {
        return 0;
    }
    // XXX This is to work around some odd interactions between this driver and SMC
    // Ethernet cards.
    if loc0 == -1 && drive >= 2 {
        return 0;
    }

    // We want to keep the flags config gave us.
    fa.fa_flags = cf.cf_flags;

    // select drive and turn on motor
    bus_space_write_1(iot, ioh, fdout, drive as u8 | FDO_FRST | fdo_moen(drive));
    // wait for motor to spin up
    let _ = tsleep_nsec(ptr::from_ref(fdc), 0, "fdprobe", msec_to_nsec(250));
    let _ = out_fdc(iot, ioh, NE7CMD_RECAL);
    let _ = out_fdc(iot, ioh, drive as u8);
    // wait for recalibrate
    let _ = tsleep_nsec(ptr::from_ref(fdc), 0, "fdprobe", msec_to_nsec(2000));
    let _ = out_fdc(iot, ioh, NE7CMD_SENSEI);
    let n = fdcresult(fdc);

    // turn off motor
    let _ = tsleep_nsec(ptr::from_ref(fdc), 0, "fdprobe", msec_to_nsec(250));
    bus_space_write_1(iot, ioh, fdout, FDO_FRST);

    // flags & 0x20 forces the drive to be found even if it won't probe
    if fa.fa_flags & 0x20 == 0 && (n != Ok(2) || (fdc.sc_status[0].get() & 0xf8) != 0x20) {
        return 0;
    }

    1
}

/// `fdattach`: controller is working, and drive responded. Attach it.
pub fn fdattach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let Some(parent) = parent else {
        return;
    };
    // SAFETY: as in `fdprobe`.
    let fdc = unsafe { parent.softc::<FdcSoftc>() };
    // SAFETY: `fd_ca`'s devices are `FdSoftc`s, which live as long as the kernel.
    let fd: &'static FdSoftc = unsafe { &*ptr::from_ref(self_.softc::<FdSoftc>()) };
    // SAFETY: as in `fdprobe`.
    let fa = unsafe { &*aux.cast::<FdcAttachArgs>() };
    let mut type_ = fa.fa_deftype;
    let drive = fa.fa_drive;

    if type_.is_none() || fa.fa_flags & 0x10 != 0 {
        // The config has overridden this.
        match fa.fa_flags & 0x07 {
            1 => type_ = Some(&FD_TYPES[7]), // 2.88MB
            2 => type_ = Some(&FD_TYPES[0]), // 1.44MB
            3 => type_ = Some(&FD_TYPES[1]), // 1.2MB
            4 => type_ = Some(&FD_TYPES[4]), // 720K
            5 => type_ = Some(&FD_TYPES[3]), // 360K
            6 => type_ = Some(&FD_TYPES[8]), // 1.2 MB japanese format
            _ => {}
        }
    }

    match type_ {
        Some(t) => printf(format_args!(
            ": {} {} cyl, {} head, {} sec\n",
            t.name(),
            t.tracks,
            t.heads,
            t.sectrac
        )),
        None => printf(format_args!(": density unknown\n")),
    };

    fd.sc_cylin.set(-1);
    fd.sc_drive.set(drive);
    fd.sc_deftype.set(type_);
    fdc.sc_type[drive as usize].set(FDC_TYPE_DISK);
    fdc.sc_link.sc_fd[drive as usize].set(Some(fd));

    // Initialize and attach the disk structure.
    fd.sc_dk.dk_flags.set(DKF_NOLABELREAD);
    let mut dk_name = [0u8; 16];
    let xname = self_.xname().as_bytes();
    let n = xname.len().min(dk_name.len());
    dk_name[..n].copy_from_slice(&xname[..n]);
    fd.sc_dk.dk_name.set(dk_name);
    let _ = bufq_init(&fd.sc_bufq, BUFQ_DEFAULT);
    disk_attach(Some(&fd.sc_dev), &fd.sc_dk);

    // Setup timeout structures
    let arg: *mut c_void = ptr::from_ref(fd).cast_mut().cast();
    timeout_set(&fd.fd_motor_on_to, fd_motor_on, arg);
    timeout_set(&fd.fd_motor_off_to, fd_motor_off, arg);
    timeout_set(&fd.fdtimeout_to, fdtimeout, arg);
}

/// `fdactivate`.
pub fn fdactivate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: as in `fdattach`.
    let fd = unsafe { self_.softc::<FdSoftc>() };
    let fdc = fd.fdc();

    match act {
        DVACT_SUSPEND => {
            if fdc.sc_state.get() != DEVIDLE {
                let _ = timeout_del(&fd.fd_motor_on_to);
                let _ = timeout_del(&fd.fd_motor_off_to);
                let _ = timeout_del(&fd.fdtimeout_to);
                fdc.sc_state.set(IOTIMEDOUT);
                fdc.sc_errors.set(4);
            }
        }
        DVACT_POWERDOWN => {
            fd_motor_off(ptr::from_ref(fd).cast_mut().cast());
        }
        _ => {}
    }

    Ok(())
}

/// `fd_nvtotype`: translate nvram type into internal data structure. Return NULL for
/// none/unknown/unusable.
pub fn fd_nvtotype(fdc: &str, nvraminfo: i32, drive: i32) -> Option<&'static FdType> {
    let type_ = (if drive == 0 {
        nvraminfo
    } else {
        nvraminfo << 4
    }) & 0xf0;
    match type_ as u32 {
        NVRAM_DISKETTE_NONE => None,
        NVRAM_DISKETTE_12M => Some(&FD_TYPES[1]),
        NVRAM_DISKETTE_TYPE5 | NVRAM_DISKETTE_TYPE6 => Some(&FD_TYPES[7]),
        NVRAM_DISKETTE_144M => Some(&FD_TYPES[0]),
        NVRAM_DISKETTE_360K => Some(&FD_TYPES[3]),
        NVRAM_DISKETTE_720K => Some(&FD_TYPES[4]),
        _ => {
            printf(format_args!(
                "{}: drive {}: unknown device type 0x{:x}\n",
                fdc, drive, type_
            ));
            None
        }
    }
}

/// `fd_dev_to_type`: the density the minor number names (0: the drive's default).
fn fd_dev_to_type(fd: &FdSoftc, dev: Dev) -> Option<&'static FdType> {
    let type_ = fdtype(dev) as usize;

    if type_ > FD_TYPES.len() {
        return None;
    }
    if type_ != 0 {
        Some(&FD_TYPES[type_ - 1])
    } else {
        fd.sc_deftype.get()
    }
}

/// `fdstrategy`: validates a buffer and queues it on its drive.
pub fn fdstrategy(bp: &'static Buf) {
    let fd = fd_softc(bp.b_dev.get());
    let fd_bsize = i64::from(fd_bsize(fd.type_().secsize));
    let bf = fd_bsize / DEV_BSIZE as i64;

    'done: {
        // bad:
        let error = 'bad: {
            // Valid unit, controller, and request?
            let blkno = bp.b_blkno.get();
            if blkno < 0
                || ((blkno % bf != 0 || bp.b_bcount.get() % fd_bsize != 0) && !bp.isset(B_FORMAT))
            {
                break 'bad EINVAL;
            }

            // If it's a null transfer, return immediately.
            if bp.b_bcount.get() == 0 {
                break 'done;
            }

            let mut sz = howmany(bp.b_bcount.get() as usize, DEV_BSIZE) as i64;

            let size = i64::from(fd.type_().size) * bf;
            if blkno + sz > size {
                sz = size - blkno;
                if sz == 0 {
                    // If exactly at end of disk, return EOF.
                    break 'done;
                }
                if sz < 0 {
                    // If past end of disk, return EINVAL.
                    break 'bad EINVAL;
                }
                // Otherwise, truncate request.
                bp.b_bcount.set(sz << DEV_BSHIFT);
            }

            bp.b_resid.set(bp.b_bcount.get() as usize);

            // Queue I/O
            bufq_queue(&fd.sc_bufq, bp);

            // Queue transfer on drive, activate drive and controller if idle.
            let s = splbio();
            let _ = timeout_del(&fd.fd_motor_off_to); // a good idea
            if fd.sc_bp.get().is_none() {
                fdstart(fd);
            } else {
                #[cfg(feature = "diagnostic")]
                {
                    let fdc = fd.fdc();
                    if fdc.sc_state.get() == DEVIDLE {
                        printf(format_args!("fdstrategy: controller inactive\n"));
                        fdcstart(fdc);
                    }
                }
            }
            splx(s);
            return;
        };

        // bad:
        bp.set(B_ERROR);
        bp.b_error.set(Some(error));
    }

    // done:
    // Toss transfer; we're done early.
    bp.b_resid.set(bp.b_bcount.get() as usize);
    let s = splbio();
    biodone(bp);
    splx(s);
}

/// `fdstart`: links the drive into its controller's queue and starts an idle controller.
pub fn fdstart(fd: &'static FdSoftc) {
    let fdc = fd.fdc();
    let active = !fdc.sc_link.sc_drives.is_empty();

    // Link into controller queue.
    fd.sc_bp.set(bufq_dequeue(&fd.sc_bufq));
    // SAFETY: the drive is in no queue (it is linked only while it has a buffer, and
    // `fdstrategy` calls this only when it has none); softcs never move.
    unsafe { fdc.sc_link.sc_drives.insert_tail(fd) };

    // If controller not already active, start it.
    if !active {
        fdcstart(fdc);
    }
}

/// `fdfinish`: completes the drive's current buffer and takes its next one.
pub fn fdfinish(fd: &'static FdSoftc, bp: &'static Buf) {
    let fdc = fd.fdc();

    splassert(IPL_BIO, "fdfinish");

    fd.sc_skip.set(0);
    fd.sc_bp.set(bufq_dequeue(&fd.sc_bufq));

    // Move this drive to the end of the queue to give others a `fair' chance. We only force
    // a switch if N operations are completed while another drive is waiting to be serviced,
    // since there is a long motor startup delay whenever we switch.
    if FdDrivechainHead::next(fd).is_some() && {
        fd.sc_ops.set(fd.sc_ops.get() + 1);
        fd.sc_ops.get() >= 8
    } {
        fd.sc_ops.set(0);
        // SAFETY: the drive is in its controller's queue (it has a successor there).
        unsafe { fdc.sc_link.sc_drives.remove(fd) };
        if fd.sc_bp.get().is_some() {
            // SAFETY: just removed from the queue; softcs never move.
            unsafe { fdc.sc_link.sc_drives.insert_tail(fd) };
        }
    }

    biodone(bp);
    // turn off motor 5s from now
    let _ = timeout_add_sec(&fd.fd_motor_off_to, 5);
    fdc.sc_state.set(DEVIDLE);
}

/// `fdread`: the raw device's read (physio(9)).
pub fn fdread(dev: Dev, uio: &mut Uio<'_>, _flags: i32) -> Result<(), Errno> {
    physio(fdstrategy, dev, B_READ, minphys, uio)
}

/// `fdwrite`: the raw device's write (physio(9)).
pub fn fdwrite(dev: Dev, uio: &mut Uio<'_>, _flags: i32) -> Result<(), Errno> {
    physio(fdstrategy, dev, B_WRITE, minphys, uio)
}

/// `fd_set_motor`: selects the active drive and drives the motors; `reset` holds the
/// controller in reset.
pub fn fd_set_motor(fdc: &FdcSoftc, reset: bool) {
    let mut status: u8 = match fdc.sc_link.sc_drives.first() {
        Some(fd) => fd.sc_drive.get() as u8,
        None => 0,
    };
    if !reset {
        status |= FDO_FRST | FDO_FDMAEN;
    }
    for (n, slot) in fdc.sc_link.sc_fd.iter().enumerate() {
        if let Some(fd) = slot.get()
            && fd.sc_flags.get() & FD_MOTOR != 0
        {
            status |= fdo_moen(n as i32);
        }
    }
    bus_space_write_1(fdc.iot(), fdc.ioh(), fdout, status);
}

/// `fd_motor_off`: the motor-off timeout.
pub fn fd_motor_off(arg: *mut c_void) {
    // SAFETY: the timeouts and `fdactivate` pass the drive's softc.
    let fd = unsafe { &*arg.cast::<FdSoftc>() };

    let s = splbio();
    fd.sc_flags
        .set(fd.sc_flags.get() & !(FD_MOTOR | FD_MOTOR_WAIT));
    fd_set_motor(fd.fdc(), false);
    splx(s);
}

/// `fd_motor_on`: the motor is up to speed.
pub fn fd_motor_on(arg: *mut c_void) {
    // SAFETY: as in `fd_motor_off`.
    let fd = unsafe { &*arg.cast::<FdSoftc>() };
    let fdc = fd.fdc();

    let s = splbio();
    fd.sc_flags.set(fd.sc_flags.get() & !FD_MOTOR_WAIT);
    if fdc
        .sc_link
        .sc_drives
        .first()
        .is_some_and(|f| ptr::eq(f, fd))
        && fdc.sc_state.get() == MOTORWAIT
    {
        let _ = fdintr(fdc);
    }
    splx(s);
}

/// `fdopen`.
pub fn fdopen(dev: Dev, _flags: i32, fmt: i32, _p: &Proc) -> Result<(), Errno> {
    let unit = fdunit(dev);
    if unit as i32 >= FD_CD.cd_ndevs.get() {
        return Err(ENXIO);
    }
    let fd = fd_lookup(unit).ok_or(ENXIO)?;
    let type_ = fd_dev_to_type(fd, dev).ok_or(ENXIO)?;

    if fd.sc_flags.get() & FD_OPEN != 0 && !fd.sc_type.get().is_some_and(|t| ptr::eq(t, type_)) {
        return Err(EBUSY);
    }

    fd.sc_type.set(Some(type_));
    fd.sc_cylin.set(-1);
    fd.sc_flags.set(fd.sc_flags.get() | FD_OPEN);

    // Only update the disklabel if we're not open anywhere else.
    if fd.sc_dk.dk_openmask.get() == 0 {
        let mut lp = Disklabel::zeroed();
        let _ = fdgetdisklabel(dev, fd, &mut lp, false);
        // SAFETY: no partition is open: nobody else holds the in-core label.
        if let Some(dl) = unsafe { fd.sc_dk.label_mut() } {
            *dl = lp;
        }
    }

    let pmask = 1u64 << fdpart(dev);

    match fmt as Mode {
        S_IFCHR => fd
            .sc_dk
            .dk_copenmask
            .set(fd.sc_dk.dk_copenmask.get() | pmask),
        S_IFBLK => fd
            .sc_dk
            .dk_bopenmask
            .set(fd.sc_dk.dk_bopenmask.get() | pmask),
        _ => {}
    }
    fd.sc_dk
        .dk_openmask
        .set(fd.sc_dk.dk_copenmask.get() | fd.sc_dk.dk_bopenmask.get());

    Ok(())
}

/// `fdclose`.
pub fn fdclose(dev: Dev, _flags: i32, fmt: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let fd = fd_softc(dev);
    let pmask = 1u64 << fdpart(dev);

    fd.sc_flags.set(fd.sc_flags.get() & !FD_OPEN);
    fd.sc_opts.set(fd.sc_opts.get() & !FDOPT_NORETRY);

    match fmt as Mode {
        S_IFCHR => fd
            .sc_dk
            .dk_copenmask
            .set(fd.sc_dk.dk_copenmask.get() & !pmask),
        S_IFBLK => fd
            .sc_dk
            .dk_bopenmask
            .set(fd.sc_dk.dk_bopenmask.get() & !pmask),
        _ => {}
    }
    fd.sc_dk
        .dk_openmask
        .set(fd.sc_dk.dk_copenmask.get() | fd.sc_dk.dk_bopenmask.get());

    Ok(())
}

/// `fdsize`: swapping to floppies would not make sense.
pub fn fdsize(_dev: Dev) -> Daddr {
    -1
}

/// `fddump`: not implemented.
pub fn fddump(_dev: Dev, _blkno: Daddr, _va: *mut u8, _size: usize) -> Result<(), Errno> {
    Err(ENXIO)
}

/// `fdintr`: called from the controller: runs the active drive's transfer one step.
pub fn fdintr(fdc: &FdcSoftc) -> i32 {
    let iot = fdc.iot();
    let ioh = fdc.ioh();
    let ioh_ctl = fdc.ioh_ctl();
    let st0 = || fdc.sc_status[0].get();
    let cyl = || i32::from(fdc.sc_status[1].get());

    'loop_: loop {
        // Is there a transfer to this drive? If not, deactivate drive.
        let Some(fd) = fdc.sc_link.sc_drives.first() else {
            fdc.sc_state.set(DEVIDLE);
            return 1;
        };
        // SAFETY: the queue links softcs, which live as long as the kernel.
        let fd: &'static FdSoftc = unsafe { &*ptr::from_ref(fd) };
        let fd_bsize = fd_bsize(fd.type_().secsize);

        let Some(bp) = fd.sc_bp.get() else {
            fd.sc_ops.set(0);
            // SAFETY: `fd` is the queue's first drive.
            unsafe { fdc.sc_link.sc_drives.remove(fd) };
            continue 'loop_;
        };

        let finfo: Option<*mut FdFormb> = if bp.isset(B_FORMAT) {
            Some(bp.b_data.get().cast::<FdFormb>())
        } else {
            None
        };

        let mut cylin = ((bp.b_blkno.get() * DEV_BSIZE as i64)
            + (bp.b_bcount.get() - bp.b_resid.get() as i64))
            / i64::from(fd_bsize * fd.type_().seccyl);

        let mut next = match fdc.sc_state.get() {
            DEVIDLE => {
                fdc.sc_errors.set(0);
                fd.sc_skip.set(0);
                fd.sc_bcount.set(bp.b_bcount.get() as i32);
                fd.sc_blkno
                    .set(bp.b_blkno.get() / i64::from(fd_bsize / DEV_BSIZE as i32));
                let _ = timeout_del(&fd.fd_motor_off_to);
                if fd.sc_flags.get() & FD_MOTOR_WAIT != 0 {
                    fdc.sc_state.set(MOTORWAIT);
                    return 1;
                }
                if fd.sc_flags.get() & FD_MOTOR == 0 {
                    // Turn on the motor, being careful about pairing.
                    let ofd = fdc.sc_link.sc_fd[(fd.sc_drive.get() ^ 1) as usize].get();
                    if let Some(ofd) = ofd
                        && ofd.sc_flags.get() & FD_MOTOR != 0
                    {
                        let _ = timeout_del(&ofd.fd_motor_off_to);
                        ofd.sc_flags
                            .set(ofd.sc_flags.get() & !(FD_MOTOR | FD_MOTOR_WAIT));
                    }
                    fd.sc_flags
                        .set(fd.sc_flags.get() | FD_MOTOR | FD_MOTOR_WAIT);
                    fd_set_motor(fdc, false);
                    fdc.sc_state.set(MOTORWAIT);
                    // Allow .25s for motor to stabilize.
                    let _ = timeout_add_msec(&fd.fd_motor_on_to, 250);
                    return 1;
                }
                // Make sure the right drive is selected.
                fd_set_motor(fdc, false);

                // FALLTHROUGH
                Next::DoSeek
            }
            DOSEEK => Next::DoSeek,

            DOIO => Next::DoIo,

            SEEKWAIT => {
                let _ = timeout_del(&fd.fdtimeout_to);
                fdc.sc_state.set(SEEKCOMPLETE);
                // allow 1/50 second for heads to settle
                let _ = timeout_add_msec(&fdc.fdcpseudointr_to, 20);
                return 1;
            }

            SEEKCOMPLETE => {
                disk_unbusy(&fd.sc_dk, 0, 0, false); // no data on seek

                // Make sure seek really happened.
                let _ = out_fdc(iot, ioh, NE7CMD_SENSEI);
                if fdcresult(fdc) != Ok(2)
                    || (st0() & 0xf8) != 0x20
                    || i64::from(cyl()) != cylin * i64::from(fd.type_().step)
                {
                    fdretry(fd);
                    continue 'loop_;
                }
                fd.sc_cylin.set(cylin as i32);
                Next::DoIo
            }

            IOTIMEDOUT => {
                isadma_abort(fdc.sc_drq.get());
                fdretry(fd);
                continue 'loop_;
            }
            SEEKTIMEDOUT | RECALTIMEDOUT | RESETTIMEDOUT => {
                fdretry(fd);
                continue 'loop_;
            }

            IOCOMPLETE => {
                // IO DONE, post-analyze
                let _ = timeout_del(&fd.fdtimeout_to);

                disk_unbusy(
                    &fd.sc_dk,
                    bp.b_bcount.get() - bp.b_resid.get() as i64,
                    fd.sc_blkno.get(),
                    bp.isset(B_READ),
                );

                if fdcresult(fdc) != Ok(7) || (st0() & 0xf8) != 0 {
                    isadma_abort(fdc.sc_drq.get());
                    fdretry(fd);
                    continue 'loop_;
                }
                isadma_done(fdc.sc_drq.get());
                if fdc.sc_errors.get() != 0 {
                    diskerr(
                        bp,
                        "fd",
                        "soft error",
                        LOG_PRINTF,
                        fd.sc_skip.get() / fd_bsize,
                        None,
                    );
                    printf(format_args!("\n"));
                    fdc.sc_errors.set(0);
                }

                fd.sc_blkno
                    .set(fd.sc_blkno.get() + i64::from(fd.sc_nblks.get()));
                fd.sc_skip.set(fd.sc_skip.get() + fd.sc_nbytes.get());
                fd.sc_bcount.set(fd.sc_bcount.get() - fd.sc_nbytes.get());
                bp.b_resid
                    .set(bp.b_resid.get() - fd.sc_nbytes.get() as usize);
                if finfo.is_none() && fd.sc_bcount.get() > 0 {
                    cylin = fd.sc_blkno.get() / i64::from(fd.type_().seccyl);
                    Next::DoSeek
                } else {
                    fdfinish(fd, bp);
                    continue 'loop_;
                }
            }

            DORESET => {
                // try a reset, keep motor on
                fd_set_motor(fdc, true);
                delay(100);
                fd_set_motor(fdc, false);
                fdc.sc_state.set(RESETCOMPLETE);
                let _ = timeout_add_msec(&fd.fdtimeout_to, 500);
                return 1; // will return later
            }

            RESETCOMPLETE | DORECAL => {
                if fdc.sc_state.get() == RESETCOMPLETE {
                    let _ = timeout_del(&fd.fdtimeout_to);
                    // clear the controller output buffer
                    for _ in 0..4 {
                        let _ = out_fdc(iot, ioh, NE7CMD_SENSEI);
                        let _ = fdcresult(fdc);
                    }
                    // FALLTHROUGH
                }
                let _ = out_fdc(iot, ioh, NE7CMD_RECAL); // recal function
                let _ = out_fdc(iot, ioh, fd.sc_drive.get() as u8);
                fdc.sc_state.set(RECALWAIT);
                let _ = timeout_add_sec(&fd.fdtimeout_to, 5);
                return 1; // will return later
            }

            RECALWAIT => {
                let _ = timeout_del(&fd.fdtimeout_to);
                fdc.sc_state.set(RECALCOMPLETE);
                // allow 1/30 second for heads to settle
                let _ = timeout_add_msec(&fdc.fdcpseudointr_to, 1000 / 30);
                return 1; // will return later
            }

            RECALCOMPLETE => {
                let _ = out_fdc(iot, ioh, NE7CMD_SENSEI);
                if fdcresult(fdc) != Ok(2) || (st0() & 0xf8) != 0x20 || cyl() != 0 {
                    fdretry(fd);
                    continue 'loop_;
                }
                fd.sc_cylin.set(0);
                Next::DoSeek
            }

            MOTORWAIT => {
                if fd.sc_flags.get() & FD_MOTOR_WAIT != 0 {
                    return 1; // time's not up yet
                }
                Next::DoSeek
            }

            _ => {
                fdcstatus(&fd.sc_dev, 0, "stray interrupt");
                return 1;
            }
        };

        loop {
            match next {
                Next::DoSeek => {
                    if i64::from(fd.sc_cylin.get()) == cylin {
                        next = Next::DoIo;
                        continue;
                    }

                    let ty = fd.type_();
                    let _ = out_fdc(iot, ioh, NE7CMD_SPECIFY); // specify command
                    let _ = out_fdc(iot, ioh, ty.steprate as u8);
                    let _ = out_fdc(iot, ioh, 6); // XXX head load time == 6ms

                    let _ = out_fdc(iot, ioh, NE7CMD_SEEK); // seek function
                    let _ = out_fdc(iot, ioh, fd.sc_drive.get() as u8); // drive number
                    let _ = out_fdc(iot, ioh, (cylin * i64::from(ty.step)) as u8);

                    fd.sc_cylin.set(-1);
                    fdc.sc_state.set(SEEKWAIT);

                    fd.sc_dk.dk_seek.set(fd.sc_dk.dk_seek.get() + 1);
                    disk_busy(&fd.sc_dk);

                    let _ = timeout_add_sec(&fd.fdtimeout_to, 4);
                    return 1;
                }

                Next::DoIo => {
                    let ty = fd.type_();
                    // SAFETY: a format request's `b_data` is `fdformat`'s `FD_FORM` argument,
                    // a whole `struct fd_formb`, alive until the buffer completes.
                    let fb: Option<FdFormb> = finfo.map(|f| unsafe { f.read_unaligned() });
                    if fb.is_some() {
                        fd.sc_skip.set(
                            (offset_of!(FdFormb, format_info) + offset_of!(FdFormData, idfields))
                                as i32,
                        );
                    }
                    let mut sec = (fd.sc_blkno.get() % i64::from(ty.seccyl)) as i32;
                    let mut nblks = ty.seccyl - sec;
                    nblks = nblks.min(fd.sc_bcount.get() / fd_bsize);
                    nblks = nblks.min(FDC_MAXIOSIZE / fd_bsize);
                    fd.sc_nblks.set(nblks);
                    fd.sc_nbytes.set(if fb.is_some() {
                        bp.b_bcount.get() as i32
                    } else {
                        nblks * fd_bsize
                    });
                    let head = sec / ty.sectrac;
                    sec -= head * ty.sectrac;
                    #[cfg(feature = "diagnostic")]
                    {
                        let block = (fd.sc_cylin.get() * ty.heads + head) * ty.sectrac + sec;
                        if i64::from(block) != fd.sc_blkno.get() {
                            panic(format_args!(
                                "fdintr: block {} != blkno {}",
                                block,
                                fd.sc_blkno.get()
                            ));
                        }
                    }
                    let read = if bp.isset(B_READ) {
                        DMAMODE_READ
                    } else {
                        DMAMODE_WRITE
                    };
                    // SAFETY: `b_data` holds `b_bcount` bytes (the buffer's owner keeps it
                    // until biodone; physio maps a raw transfer into the kernel), of which
                    // this part is `sc_nbytes` from `sc_skip`.
                    let _ = unsafe {
                        isadma_start(
                            bp.b_data.get().add(fd.sc_skip.get() as usize),
                            fd.sc_nbytes.get() as usize,
                            fdc.sc_drq.get(),
                            read,
                        )
                    };
                    bus_space_write_1(iot, ioh_ctl, fdctl, ty.rate as u8);
                    if let Some(f) = fb {
                        // formatting
                        if out_fdc(iot, ioh, NE7CMD_FORMAT).is_err() {
                            fdc.sc_errors.set(4);
                            fdretry(fd);
                            continue 'loop_;
                        }
                        let fi = &f.format_info;
                        let _ = out_fdc(iot, ioh, ((head << 2) | fd.sc_drive.get()) as u8);
                        let _ = out_fdc(iot, ioh, fi.secshift);
                        let _ = out_fdc(iot, ioh, fi.nsecs);
                        let _ = out_fdc(iot, ioh, fi.gaplen);
                        let _ = out_fdc(iot, ioh, fi.fillbyte);
                    } else {
                        if read != 0 {
                            let _ = out_fdc(iot, ioh, NE7CMD_READ); // READ
                        } else {
                            let _ = out_fdc(iot, ioh, NE7CMD_WRITE); // WRITE
                        }
                        let _ = out_fdc(iot, ioh, ((head << 2) | fd.sc_drive.get()) as u8);
                        let _ = out_fdc(iot, ioh, fd.sc_cylin.get() as u8); // track
                        let _ = out_fdc(iot, ioh, head as u8);
                        let _ = out_fdc(iot, ioh, (sec + 1) as u8); // sec +1
                        let _ = out_fdc(iot, ioh, ty.secsize as u8); // sec size
                        let _ = out_fdc(iot, ioh, ty.sectrac as u8); // secs/track
                        let _ = out_fdc(iot, ioh, ty.gap1 as u8); // gap1 size
                        let _ = out_fdc(iot, ioh, ty.datalen as u8); // data len
                    }
                    fdc.sc_state.set(IOCOMPLETE);

                    disk_busy(&fd.sc_dk);

                    // allow 2 seconds for operation
                    let _ = timeout_add_sec(&fd.fdtimeout_to, 2);
                    return 1; // will return later
                }
            }
        }
    }
}

/// `fdtimeout`: a step of the state machine took too long.
pub fn fdtimeout(arg: *mut c_void) {
    // SAFETY: `fdattach` set the timeout with the drive's softc.
    let fd = unsafe { &*arg.cast::<FdSoftc>() };
    let fdc = fd.fdc();

    let s = splbio();
    fdcstatus(&fd.sc_dev, 0, "timeout");

    if fd.sc_bp.get().is_some() {
        fdc.sc_state.set(fdc.sc_state.get() + 1);
    } else {
        fdc.sc_state.set(DEVIDLE);
    }

    let _ = fdintr(fdc);
    splx(s);
}

/// `fdretry`: the next recovery step after a failure, or the hard error.
pub fn fdretry(fd: &'static FdSoftc) {
    let fdc = fd.fdc();
    let Some(bp) = fd.sc_bp.get() else {
        panic(format_args!("fdretry: no buffer"));
    };

    let errors = if fd.sc_opts.get() & FDOPT_NORETRY != 0 {
        -1 // goto fail
    } else {
        fdc.sc_errors.get()
    };
    match errors {
        // try again
        0 => fdc.sc_state.set(DOSEEK),

        // didn't work; try recalibrating
        1..=3 => fdc.sc_state.set(DORECAL),

        // still no go; reset the bastard
        4 => fdc.sc_state.set(DORESET),

        _ => {
            // fail:
            let st = |i: usize| u64::from(fdc.sc_status[i].get());
            diskerr(
                bp,
                "fd",
                "hard error",
                LOG_PRINTF,
                fd.sc_skip.get() / fd_bsize(fd.type_().secsize),
                None,
            );
            printf(format_args!(
                " (st0 {} st1 {} st2 {} cyl {} head {} sec {})\n",
                Bitmask(st(0), NE7_ST0BITS),
                Bitmask(st(1), NE7_ST1BITS),
                Bitmask(st(2), NE7_ST2BITS),
                st(3),
                st(4),
                st(5)
            ));

            bp.set(B_ERROR);
            bp.b_error.set(Some(EIO));
            bp.b_resid.set(bp.b_bcount.get() as usize);
            fdfinish(fd, bp);
        }
    }
    fdc.sc_errors.set(fdc.sc_errors.get() + 1);
}

/// `*(T *)addr = *v`: a value into an `ioctl` buffer.
fn copyout_ioctl<T>(v: &T, addr: &mut [u8]) {
    let n = addr.len().min(size_of::<T>());
    // SAFETY: `v` is `size_of::<T>()` readable bytes of a `repr(C)` value without padding
    // that matters (the C copies the same bytes); `n` fits both.
    unsafe { ptr::copy_nonoverlapping(ptr::from_ref(v).cast::<u8>(), addr.as_mut_ptr(), n) };
}

/// `fdioctl`.
pub fn fdioctl(dev: Dev, cmd: u64, addr: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let fd = fd_softc(dev);

    match cmd {
        MTIOCTOP => {
            let mt_op = i16::from_ne_bytes([addr[0], addr[1]]);
            if mt_op != MTOFFL {
                return Err(EIO);
            }
            Ok(())
        }

        DIOCRLDINFO => {
            let Some(mem) = malloc(size_of::<Disklabel>(), M_TEMP, M_WAITOK) else {
                panic(format_args!("fdioctl: malloc(M_WAITOK) failed"));
            };
            let lpp = mem.cast::<Disklabel>();
            // SAFETY: a fresh allocation the size of a label, written whole before the
            // reference is made, ours alone until it is freed below.
            let lp = unsafe {
                lpp.as_ptr().write(Disklabel::zeroed());
                &mut *lpp.as_ptr()
            };
            let _ = fdgetdisklabel(dev, fd, lp, false);
            // SAFETY: the driver's own label; no other reference to it is live.
            if let Some(dl) = unsafe { fd.sc_dk.label_mut() } {
                *dl = *lp;
            }
            free(mem, M_TEMP, size_of::<Disklabel>());
            Ok(())
        }

        DIOCGPDINFO => {
            let mut lp = Disklabel::zeroed();
            let _ = fdgetdisklabel(dev, fd, &mut lp, true);
            let n = addr.len().min(DISKLABEL_SIZE);
            addr[..n].copy_from_slice(&lp.as_bytes()[..n]);
            Ok(())
        }

        DIOCGDINFO => {
            fd.sc_dk.with_label(|lp| {
                if let Some(lp) = lp {
                    let n = addr.len().min(DISKLABEL_SIZE);
                    addr[..n].copy_from_slice(&lp.as_bytes()[..n]);
                }
            });
            Ok(())
        }

        DIOCGPART => {
            if let Some(lp) = fd.sc_dk.dk_label.get() {
                let pi = Partinfo {
                    disklab: lp.as_ptr(),
                    // SAFETY: `lp` is the live in-core label; the projection only computes
                    // the address of one of its partitions.
                    part: unsafe { &raw mut (*lp.as_ptr()).d_partitions[fdpart(dev) as usize] },
                };
                pi.store(addr);
            }
            Ok(())
        }

        DIOCWDINFO | DIOCSDINFO => {
            if flag & FWRITE == 0 {
                return Err(EBADF);
            }

            let mut nlp = Disklabel::from_bytes(addr);
            // SAFETY: the driver's own label; the borrow ends before the strategy runs.
            let mut error = match unsafe { fd.sc_dk.label_mut() } {
                Some(olp) => setdisklabel(olp, &mut nlp, 0),
                None => Err(ENXIO),
            };
            if error.is_ok() && cmd == DIOCWDINFO {
                let mut lp = fd.sc_dk.label().unwrap_or_default();
                error = writedisklabel(disklabeldev(dev), fdstrategy, &mut lp);
            }
            error
        }

        FD_FORM => {
            if flag & FWRITE == 0 {
                return Err(EBADF); // must be opened for writing
            }
            let version = i32::from_ne_bytes([addr[0], addr[1], addr[2], addr[3]]);
            if version != FD_FORMAT_VERSION {
                return Err(EINVAL); // wrong version of formatting prog
            }
            fdformat(dev, addr, p)
        }

        FD_GTYPE => {
            // get drive type
            copyout_ioctl(fd.type_(), addr);
            Ok(())
        }

        FD_GOPTS => {
            // get drive options
            copyout_ioctl(&fd.sc_opts.get(), addr);
            Ok(())
        }

        FD_SOPTS => {
            // set drive options
            fd.sc_opts
                .set(i32::from_ne_bytes([addr[0], addr[1], addr[2], addr[3]]));
            Ok(())
        }

        _ => Err(ENOTTY),
    }
}

/// `fdformat`: formats the track `finfo` (the `FD_FORM` argument's bytes, a `struct
/// fd_formb`) names through `fdstrategy` and waits for it.
pub fn fdformat(dev: Dev, finfo: &mut [u8], p: &Proc) -> Result<(), Errno> {
    let fd = fd_softc(dev);
    let type_ = fd.type_();
    let fd_bsize = fd_bsize(type_.secsize);
    if finfo.len() < size_of::<FdFormb>() {
        // Never: the ioctl copies FD_FORM's whole argument in.
        return Err(EINVAL);
    }
    // SAFETY: `finfo` holds a whole `struct fd_formb` (checked above); every bit pattern is
    // a valid one (integers and bytes).
    let f = unsafe { finfo.as_ptr().cast::<FdFormb>().read_unaligned() };

    // set up a buffer header for fdstrategy()
    let mem = malloc(size_of::<Buf>(), M_TEMP, M_NOWAIT | M_ZERO).ok_or(ENOBUFS)?;
    let bpp = mem.cast::<Buf>();
    // SAFETY: a fresh allocation the size of a buffer header, written whole before the
    // reference is made; it is freed only after `biowait` returned.
    let bp: &'static Buf = unsafe {
        bpp.as_ptr().write(Buf::new());
        &*bpp.as_ptr()
    };

    bp.b_flags.set(B_BUSY | B_PHYS | B_FORMAT | B_RAW);
    bp.b_proc.set(ptr::from_ref(p));
    bp.b_dev.set(dev);

    // calculate a fake blkno, so fdstrategy() would initiate a seek to the requested
    // cylinder
    bp.b_blkno.set(
        i64::from((f.cyl * (type_.sectrac * type_.heads) + f.head * type_.sectrac) * fd_bsize)
            / DEV_BSIZE as i64,
    );

    bp.b_bcount
        .set((size_of::<FdIdfieldData>() * usize::from(f.format_info.nsecs)) as i64);
    bp.b_data.set(finfo.as_mut_ptr());

    // now do the format
    fdstrategy(bp);

    // ...and wait for it to complete
    let rv = biowait(bp);
    free(mem, M_TEMP, size_of::<Buf>());
    rv
}
/* </CODE> */
