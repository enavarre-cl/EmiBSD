/*	$OpenBSD: conf.c,v 1.49 2026/05/03 13:10:46 stsp Exp $	*/
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
//! efiboot's configuration: its version, the probes `machdep()` runs, and the tables libsa
//! works through: the file systems, the devices and the consoles.
//!
//! Upstream: sys/arch/amd64/stand/efiboot/conf.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The tables and the machine-dependent routines are gathered in [`SA_CONF`] (libsa's)
//!   and [`BOOT_MD`] (boot(8)'s), which `efi_main` registers (the C's linker resolves them).
//! - `sa_cleanup` is not declared: efiboot leaves it NULL ("can't use sa_cleanup since
//!   printf is used after sa_cleanup()", `efiboot.c`), so `run_loadfile` never calls it.

use boot::boot::{BootMd, Stty};
use libsa::cd9660::*;
use libsa::cons::cninit;
use libsa::hdr::cons::ConsDev;
use libsa::stand::{Devsw, FsOps, SaConf};
use libsa::ufs::*;
use libsa::ufs2::*;

use crate::cmd_i386::CMD_MACHINE;
use crate::dev_i386::{cnspeed, devboot, devopen, ttydev, ttyname};
use crate::diskprobe::{bootdev_has_hibernate, diskprobe};
use crate::efiboot::{
    _rtt, EFI_LOADADDR, efi_com_getc, efi_com_init, efi_com_probe, efi_com_putc, efi_cons_getc,
    efi_cons_init, efi_cons_probe, efi_cons_putc, efi_diskprobe, efi_memprobe, getsecs,
};
use crate::efidev::*;
use crate::efipxe::*;
use crate::efirng::fwrandom;
use crate::exec_i386::run_loadfile;
use crate::libsa_md::I386BootProbes;
use crate::machdep::{check_skip_conf, machdep};
use crate::mdrandom::mdrandom;

/// `version[]`: efiboot's version.
pub const VERSION: &str = "3.71";

/// `i386_probe1[]`.
static I386_PROBE1: [fn(); 2] = [cninit, efi_memprobe];
/// `i386_probe2[]`.
static I386_PROBE2: [fn(); 3] = [efi_pxeprobe, efi_diskprobe, diskprobe];

/// `probe_list[]`.
pub static PROBE_LIST: [I386BootProbes; 2] = [
    I386BootProbes {
        name: "probing",
        probes: &I386_PROBE1,
    },
    I386BootProbes {
        name: "disk",
        probes: &I386_PROBE2,
    },
];

/// `file_system[]`.
pub static FILE_SYSTEM: [FsOps; 5] = [
    FsOps {
        open: tftp_open,
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
pub static DEVSW: [Devsw; 3] = [
    Devsw {
        dv_name: "ESP",
        dv_strategy: espstrategy,
        dv_open: espopen,
        dv_close: espclose,
        dv_ioctl: espioctl,
    },
    Devsw {
        dv_name: "TFTP",
        dv_strategy: tftpstrategy,
        dv_open: tftpopen,
        dv_close: tftpclose,
        dv_ioctl: tftpioctl,
    },
    Devsw {
        dv_name: "EFI",
        dv_strategy: efistrategy,
        dv_open: efiopen,
        dv_close: eficlose,
        dv_ioctl: efiioctl,
    },
];

/// `constab[]`.
pub static CONSTAB: [ConsDev; 2] = [
    ConsDev::new(efi_cons_probe, efi_cons_init, efi_cons_getc, efi_cons_putc),
    ConsDev::new(efi_com_probe, efi_com_init, efi_com_getc, efi_com_putc),
];

/// `LOADADDR(a)` of `<machine/loadfile_machdep.h>` with `EFIBOOT`: the kernel goes to
/// `efi_loadaddr` plus its address within 256 MB.
fn loadaddr(a: u64, offset: u64) -> u64 {
    (a.wrapping_add(offset) & 0x0fff_ffff)
        + EFI_LOADADDR.load(core::sync::atomic::Ordering::Relaxed)
}

/// libsa's view of efiboot.
pub static SA_CONF: SaConf = SaConf {
    file_system: &FILE_SYSTEM,
    devsw: &DEVSW,
    constab: &CONSTAB,
    devopen,
    rtt: _rtt,
    loadaddr,
    netif_drivers: &[],
    getsecs,
};

/// boot(8)'s view of efiboot.
pub static BOOT_MD: BootMd = BootMd {
    machine: "amd64",
    version: VERSION,
    machdep,
    devboot,
    run_loadfile,
    getsecs,
    cmd_machine: Some(&CMD_MACHINE),
    check_skip_conf: Some(check_skip_conf),
    mdrandom: Some(mdrandom),
    fwrandom: Some(fwrandom),
    bootdev_has_hibernate: Some(bootdev_has_hibernate),
    stty: Some(Stty {
        ttyname,
        ttydev,
        cnspeed,
    }),
};
/* </CODE> */
