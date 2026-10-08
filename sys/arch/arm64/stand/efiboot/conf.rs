/*	$OpenBSD: conf.c,v 1.55 2026/09/06 17:10:00 kettenis Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 Michael Shalayeff
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
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! efiboot's configuration: its version, and the tables libsa works through: the file
//! systems, the devices, the consoles and the network interface drivers.
//!
//! Upstream: sys/arch/arm64/stand/efiboot/conf.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The tables and the machine-dependent routines are gathered in [`SA_CONF`] (libsa's)
//!   and [`BOOT_MD`] (boot(8)'s), which `efi_main` registers (the C's linker resolves them).
//! - `debug` is not declared: only `efidev.c`'s `BIOS_DEBUG` traces read it, and efiboot
//!   does not define `BIOS_DEBUG`.
//! - `devsw[]`'s `sr` routines (`sropen`, `srstrategy`, `srclose`, `srioctl`) belong to
//!   softraid_arm64.c, which is not ported (with libsa's softraid.c and its crypto): here
//!   `sropen` says so and fails with `ENXIO`, so no `sr` name opens; `srclose` and
//!   `srioctl` are the C's, `srstrategy` fails with `ENXIO` (no open hands it a volume).

use core::ffi::c_void;
use core::sync::atomic::Ordering;

use boot::boot::{BootMd, Stty};
use libsa::cd9660::*;
use libsa::hdr::cons::ConsDev;
use libsa::hdr::types::Daddr;
use libsa::netif::NetifDriver;
use libsa::printf;
use libsa::saerrno::Errno;
use libsa::stand::{Devsw, FsOps, OpenFile, SaConf};
use libsa::tftp::{tftp_close, tftp_read, tftp_readdir, tftp_seek, tftp_stat, tftp_write};
use libsa::ufs::*;
use libsa::ufs2::*;

use crate::efiboot::{
    _rtt, CMD_MACHINE, EFI_LOADADDR, cnspeed, devboot, devopen, devopen_args, efi_com_getc,
    efi_com_init, efi_com_probe, efi_com_putc, efi_cons_getc, efi_cons_init, efi_cons_probe,
    efi_cons_putc, efi_fb_getc, efi_fb_init, efi_fb_probe, efi_fb_putc, getsecs, machdep, mdrandom,
    ttydev, ttyname,
};
use crate::efidev::*;
use crate::efipxe::*;
use crate::efirng::fwrandom;
use crate::exec::run_loadfile;

/// `version[]`: efiboot's version.
pub const VERSION: &str = "1.26";

/// `file_system[]`.
pub static FILE_SYSTEM: [FsOps; 6] = [
    FsOps {
        open: mtftp_open,
        close: mtftp_close,
        read: mtftp_read,
        write: mtftp_write,
        seek: mtftp_seek,
        stat: mtftp_stat,
        readdir: mtftp_readdir,
        fchmod: None,
    },
    FsOps {
        open: efitftp_open,
        close: tftp_close,
        read: tftp_read,
        write: tftp_write,
        seek: tftp_seek,
        stat: tftp_stat,
        readdir: tftp_readdir,
        fchmod: None,
    },
    FsOps {
        open: ufs_open,
        close: ufs_close,
        read: ufs_read,
        write: ufs_write,
        seek: ufs_seek,
        stat: ufs_stat,
        readdir: ufs_readdir,
        fchmod: Some(ufs_fchmod),
    },
    FsOps {
        open: ufs2_open,
        close: ufs2_close,
        read: ufs2_read,
        write: ufs2_write,
        seek: ufs2_seek,
        stat: ufs2_stat,
        readdir: ufs2_readdir,
        fchmod: Some(ufs2_fchmod),
    },
    FsOps {
        open: cd9660_open,
        close: cd9660_close,
        read: cd9660_read,
        write: cd9660_write,
        seek: cd9660_seek,
        stat: cd9660_stat,
        readdir: cd9660_readdir,
        fchmod: None,
    },
    FsOps {
        open: esp_open,
        close: esp_close,
        read: esp_read,
        write: esp_write,
        seek: esp_seek,
        stat: esp_stat,
        readdir: esp_readdir,
        fchmod: None,
    },
];

/// `devsw[]`.
pub static DEVSW: [Devsw; 4] = [
    Devsw {
        dv_name: "esp",
        dv_strategy: espstrategy,
        dv_open: espopen,
        dv_close: espclose,
        dv_ioctl: espioctl,
    },
    Devsw {
        dv_name: "tftp",
        dv_strategy: tftpstrategy,
        dv_open: tftpopen,
        dv_close: tftpclose,
        dv_ioctl: tftpioctl,
    },
    Devsw {
        dv_name: "sd",
        dv_strategy: efistrategy,
        dv_open: efiopen,
        dv_close: eficlose,
        dv_ioctl: efiioctl,
    },
    Devsw {
        dv_name: "sr",
        dv_strategy: srstrategy,
        dv_open: sropen,
        dv_close: srclose,
        dv_ioctl: srioctl,
    },
];

/// `constab[]`.
pub static CONSTAB: [ConsDev; 3] = [
    ConsDev::new(efi_cons_probe, efi_cons_init, efi_cons_getc, efi_cons_putc),
    ConsDev::new(efi_com_probe, efi_com_init, efi_com_getc, efi_com_putc),
    ConsDev::new(efi_fb_probe, efi_fb_init, efi_fb_getc, efi_fb_putc),
];

/// `netif_drivers[]`.
pub static NETIF_DRIVERS: [&NetifDriver; 1] = [&EFINET_DRIVER];

/// libsa's view of efiboot.
pub static SA_CONF: SaConf = SaConf {
    file_system: &FILE_SYSTEM,
    devsw: &DEVSW,
    constab: &CONSTAB,
    devopen,
    rtt: _rtt,
    loadaddr,
    netif_drivers: &NETIF_DRIVERS,
    getsecs,
};

/// boot(8)'s view of efiboot (libsa.h: `MACHINE_CMD`; the Makefile: `MDRANDOM`,
/// `FWRANDOM`, `HIBERNATE`, `BOOT_STTY`; no `CHECK_SKIP_CONF`).
pub static BOOT_MD: BootMd = BootMd {
    machine: "arm64",
    version: VERSION,
    machdep,
    devboot,
    run_loadfile,
    getsecs,
    cmd_machine: Some(&CMD_MACHINE),
    check_skip_conf: None,
    mdrandom: Some(mdrandom),
    fwrandom: Some(fwrandom),
    bootdev_has_hibernate: Some(bootdev_has_hibernate),
    stty: Some(Stty {
        ttyname,
        ttydev,
        cnspeed,
    }),
};

/// `LOADADDR(a)` of arm64's `<machine/loadfile_machdep.h>`: the kernel goes to
/// `efi_loadaddr` plus its address within 512 GB.
fn loadaddr(a: u64, offset: u64) -> u64 {
    (a.wrapping_add(offset) & 0x7f_ffff_ffff).wrapping_add(EFI_LOADADDR.load(Ordering::Relaxed))
}

/// `sropen(f, unit, part)` (softraid_arm64.c): not ported; no softraid volume opens.
fn sropen(_f: &mut OpenFile, file: &mut &[u8]) -> Result<(), Errno> {
    let (unit, _) = devopen_args(file);
    printf!(
        "sr{}: softraid volumes are not supported (softraid_arm64.c is not ported)\n",
        unit
    );
    Err(Errno::ENXIO)
}

/// `srstrategy(devdata, ...)` (softraid_arm64.c): not ported; `sropen` never succeeds, so
/// nothing calls it.
fn srstrategy(
    _devdata: *mut c_void,
    _rw: i32,
    _blk: Daddr,
    _buf: &mut [u8],
    _rsize: Option<&mut usize>,
) -> Result<(), Errno> {
    Err(Errno::ENXIO)
}

/// `srclose(f)` (softraid_arm64.c).
fn srclose(f: &mut OpenFile) -> Result<(), Errno> {
    f.f_devdata = core::ptr::null_mut();
    Ok(())
}

/// `srioctl(f, cmd, data)` (softraid_arm64.c).
fn srioctl(_f: &mut OpenFile, _cmd: u64, _data: *mut c_void) -> Result<(), Errno> {
    Ok(())
}
/* </CODE> */
