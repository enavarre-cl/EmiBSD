/*	$OpenBSD: conf.c,v 1.84 2025/11/12 11:34:36 hshoexer Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1994, 1995 Charles M. Hannum.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by Charles Hannum.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
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
/* </LICENSES> */

/* <CODE> */
//! amd64's device switch tables: `arch/amd64/amd64/conf.c`.
//!
//! Upstream: sys/arch/amd64/amd64/conf.c @ 3ce1f3f79392
//!
//! The major numbers are ABI: `MAKEDEV(8)` creates `/dev` nodes with them, so every slot keeps
//! the C's index, and the comment on each line is the C's.
//!
//! ## Deviations
//! - A slot whose driver is not ported holds `cdev_notdef()` (`bdev_notdef()`), where the C
//!   writes the driver's initialiser with a count (`cdev_disk_init(NWD,wd)`): its entry points
//!   answer `ENODEV` instead of the `ENXIO` a count of 0 would give, and `d_type` is 0. The
//!   drivers present are `cn` (0), `ctty` (1), `mm` (2), `pts`/`ptc` (5, 6), `com` (8),
//!   `wsdisplay` (12, M13), `wskbd` (67, M13), `wsmouse` (68, M13), `wsmux` (69, M13),
//!   `filedesc` (22), `bpf` (23), `sd` (4 block, 13 character), `cd` (6 block, 15 character), `vnd` (14 block,
//!   41 character), `rd` (17 block, 47 character), `audio` (42, M12), `usb` (61, M12), `uhid` (62, M16b), `ugen` (63, M16b), `ucom` (66, M16b), `pf` (73), `bio` (79), `ptm` (81), `diskmap` (90, M14) and `fuse` (92, feature `fuse`). `log` (7) waits for `subr_log.c`'s `logopen` ..
//!   `logkqfilter`, `random` (45) for `rnd.c`.
//! - The tables are [`Devsw`]s of `Cell`s so that a console driver can take over a slot at
//!   boot (`machine::conf::cdevsw_set`); `nblkdev`/`nchrdev` are their lengths.
//! - `findblkmajor`/`dev_rawpart` (the boot disk heuristics) and `constab[]` (the `cninit`
//!   probe list) are not here: `dk_mountroot` and `cninit` are not ported, and the console is
//!   attached directly (`consinit.rs`).
//! - `APERTURE` and `MTRR` are not configured (`mem.rs`); `cdev_ocis_init`,
//!   `cdev_nvram_init`, `cdev_vmm_init`, `cdev_psp_init` and `<machine/conf.h>`'s initialisers
//!   are for drivers that are not ported.

use core::cell::Cell;

use crate::arch::amd64::amd64::mem::{mmclose, mmioctl, mmmmap, mmopen, mmrw};
use crate::dev::audio::{
    NAUDIO, audioclose, audioioctl, audiokqfilter, audioopen, audioread, audiowrite,
};
use crate::dev::bio::{NBIO, bioclose, bioioctl, bioopen};
use crate::dev::cons::{cnclose, cnioctl, cnkqfilter, cnopen, cnread, cnstop, cnwrite};
use crate::dev::diskmap::{diskmapclose, diskmapioctl, diskmapopen, diskmapread, diskmapwrite};
use crate::dev::gpio::gpio::{NGPIO, gpioclose, gpioioctl, gpioopen};
use crate::dev::ic::com::{comclose, comioctl, comopen, comread, comstop, comtty, comwrite};
use crate::dev::rd::{NRD, rdclose, rddump, rdioctl, rdopen, rdread, rdsize, rdstrategy, rdwrite};
use crate::dev::usb::ucom::{
    NUCOM, ucomclose, ucomioctl, ucomopen, ucomread, ucomstop, ucomtty, ucomwrite,
};
use crate::dev::usb::ugen::{
    NUGEN, ugenclose, ugenioctl, ugenkqfilter, ugenopen, ugenread, ugenwrite,
};
use crate::dev::usb::uhid::{
    NUHID, uhidclose, uhidioctl, uhidkqfilter, uhidopen, uhidread, uhidwrite,
};
use crate::dev::usb::usb::{NUSB, usbclose, usbioctl, usbopen};
use crate::dev::vnd::{
    NVND, vndclose, vnddump, vndioctl, vndopen, vndread, vndsize, vndstrategy, vndwrite,
};
use crate::dev::wscons::wsdisplay::{
    wsdisplayclose, wsdisplayioctl, wsdisplaykqfilter, wsdisplaymmap, wsdisplayopen, wsdisplayread,
    wsdisplaystop, wsdisplaytty, wsdisplaywrite,
};
use crate::dev::wscons::wskbd::{
    NWSKBD, wskbdclose, wskbdioctl, wskbdkqfilter, wskbdopen, wskbdread,
};
use crate::dev::wscons::wsmouse::{
    NWSMOUSE, wsmouseclose, wsmouseioctl, wsmousekqfilter, wsmouseopen, wsmouseread,
};
use crate::dev::wscons::wsmux::{
    NWSMUX, wsmuxclose, wsmuxioctl, wsmuxkqfilter, wsmuxopen, wsmuxread,
};
use crate::kern::kern_descrip::filedescopen;
use crate::kern::tty_pty::{
    NPTY, ptcclose, ptckqfilter, ptcopen, ptcread, ptcwrite, ptmclose, ptmioctl, ptmopen, ptsclose,
    ptsopen, ptsread, ptsstop, ptswrite, ptyioctl, ptytty,
};
use crate::kern::tty_tty::{cttyioctl, cttykqfilter, cttyopen, cttyread, cttywrite};
use crate::machine::conf::Devsw;
#[cfg(feature = "fuse")]
use crate::miscfs::fuse::fuse_device::{
    NFUSE, fuseclose, fusekqfilter, fuseopen, fuseread, fusewrite,
};
use crate::net::bpf::{NBPFILTER, bpfclose, bpfioctl, bpfkqfilter, bpfopen, bpfread, bpfwrite};
use crate::net::pf_ioctl::{NPF, pfclose, pfioctl, pfopen};
use crate::scsi::cd::{NCD, cdclose, cddump, cdioctl, cdopen, cdread, cdsize, cdstrategy, cdwrite};
use crate::scsi::sd::{NSD, sdclose, sddump, sdioctl, sdopen, sdread, sdsize, sdstrategy, sdwrite};
#[cfg(feature = "fuse")]
use crate::sys::conf::cdev_fuse_init;
use crate::sys::conf::{
    Bdevsw, Cdevsw, bdev_disk_init, bdev_notdef, cdev_audio_init, cdev_bio_init, cdev_bpf_init,
    cdev_cn_init, cdev_ctty_init, cdev_disk_init, cdev_fd_init, cdev_gpio_init, cdev_mm_init,
    cdev_mouse_init, cdev_notdef, cdev_pf_init, cdev_ptc_init, cdev_ptm_init, cdev_tty_init,
    cdev_usb_init, cdev_usbdev_init, cdev_wsdisplay_init,
};
use crate::sys::param::NODEV;
use crate::sys::types::{Dev, major, makedev, minor};

/// `NCOM`: `com0` to `com3` at `isa?` in GENERIC.
pub const NCOM: i32 = 4;

/// `NWSDISPLAY`: `wsdisplay0 at efifb?` in GENERIC (M13; its other lines wait for vga, inteldrm, radeondrm, amdgpu and udl).
pub const NWSDISPLAY: i32 = 1;

/// An empty block slot.
const fn bnotdef() -> Cell<Bdevsw> {
    Cell::new(bdev_notdef())
}

/// An empty character slot.
const fn cnotdef() -> Cell<Cdevsw> {
    Cell::new(cdev_notdef())
}

/// `bdevsw[]`.
pub static BDEVSW: Devsw<Bdevsw, 20> = Devsw([
    bnotdef(), // 0: ST506/ESDI/IDE disk (wd: not ported)
    bnotdef(), // 1: swap pseudo-device (sw: uvm_swap.c, not ported)
    bnotdef(), // 2: floppy diskette (fd: not ported)
    bnotdef(), // 3
    // 4: SCSI disk
    Cell::new(bdev_disk_init(
        NSD, sdopen, sdclose, sdstrategy, sdioctl, sddump, sdsize,
    )),
    bnotdef(), // 5: was: SCSI tape
    // 6: SCSI CD-ROM
    Cell::new(bdev_disk_init(
        NCD, cdopen, cdclose, cdstrategy, cdioctl, cddump, cdsize,
    )),
    bnotdef(), // 7
    bnotdef(), // 8
    bnotdef(), // 9
    bnotdef(), // 10
    bnotdef(), // 11
    bnotdef(), // 12
    bnotdef(), // 13
    // 14: vnode disk driver
    Cell::new(bdev_disk_init(
        NVND,
        vndopen,
        vndclose,
        vndstrategy,
        vndioctl,
        vnddump,
        vndsize,
    )),
    bnotdef(), // 15: was: Sony CD-ROM
    bnotdef(), // 16: was: concatenated disk driver
    // 17: ram disk driver
    Cell::new(bdev_disk_init(
        NRD, rdopen, rdclose, rdstrategy, rdioctl, rddump, rdsize,
    )),
    bnotdef(), // 18
    bnotdef(), // 19 was: RAIDframe disk driver
]);

/// `cdevsw[]`.
pub static CDEVSW: Devsw<Cdevsw, 102> = Devsw([
    // 0: virtual console
    Cell::new(cdev_cn_init(
        1, cnopen, cnclose, cnread, cnwrite, cnioctl, cnstop, cnkqfilter,
    )),
    // 1: controlling terminal
    Cell::new(cdev_ctty_init(
        1,
        cttyopen,
        cttyread,
        cttywrite,
        cttyioctl,
        cttykqfilter,
    )),
    // 2: /dev/{null,mem,kmem,...}
    Cell::new(cdev_mm_init(
        1, mmopen, mmclose, mmrw, mmrw, mmioctl, mmmmap,
    )),
    cnotdef(), // 3: ST506/ESDI/IDE disk (wd: not ported)
    cnotdef(), // 4 was /dev/drum
    // 5: pseudo-tty slave
    Cell::new(cdev_tty_init(
        NPTY, ptsopen, ptsclose, ptsread, ptswrite, ptyioctl, ptsstop, ptytty,
    )),
    // 6: pseudo-tty master
    Cell::new(cdev_ptc_init(
        NPTY,
        ptcopen,
        ptcclose,
        ptcread,
        ptcwrite,
        ptyioctl,
        ptytty,
        ptckqfilter,
    )),
    cnotdef(), // 7: /dev/klog (log: subr_log.c's logopen .. logkqfilter, not ported)
    // 8: serial port
    Cell::new(cdev_tty_init(
        NCOM, comopen, comclose, comread, comwrite, comioctl, comstop, comtty,
    )),
    cnotdef(), // 9: floppy disk (fd: not ported)
    cnotdef(), // 10 vmm (not ported)
    cnotdef(), // 11: Sony CD-ROM
    // 12: frame buffers, etc.
    Cell::new(cdev_wsdisplay_init(
        NWSDISPLAY,
        wsdisplayopen,
        wsdisplayclose,
        wsdisplayread,
        wsdisplaywrite,
        wsdisplayioctl,
        wsdisplaystop,
        wsdisplaytty,
        wsdisplaymmap,
        wsdisplaykqfilter,
    )),
    // 13: SCSI disk
    Cell::new(cdev_disk_init(
        NSD, sdopen, sdclose, sdread, sdwrite, sdioctl,
    )),
    cnotdef(), // 14: SCSI tape (st: not ported)
    // 15: SCSI CD-ROM
    Cell::new(cdev_disk_init(
        NCD, cdopen, cdclose, cdread, cdwrite, cdioctl,
    )),
    cnotdef(), // 16: parallel printer (lpt: not ported)
    cnotdef(), // 17: SCSI autochanger (ch: not ported)
    cnotdef(), // 18: kexec (not ported)
    cnotdef(), // 19: kcov (not ported)
    cnotdef(), // 20: unknown SCSI (uk: not ported)
    cnotdef(), // 21
    // 22: file descriptor pseudo-device
    Cell::new(cdev_fd_init(1, filedescopen)),
    Cell::new(cdev_bpf_init(
        NBPFILTER,
        bpfopen,
        bpfclose,
        bpfread,
        bpfwrite,
        bpfioctl,
        bpfkqfilter,
    )), // 23: Berkeley packet filter
    cnotdef(), // 24
    cnotdef(), // 25
    cnotdef(), // 26
    cnotdef(), // 27: PC speaker (spkr: not ported)
    cnotdef(), // 28 was LKM
    cnotdef(), // 29
    cnotdef(), // 30: dynamic tracer (dt: not ported)
    cnotdef(), // 31
    cnotdef(), // 32
    cnotdef(), // 33
    cnotdef(), // 34
    cnotdef(), // 35: Microsoft mouse
    cnotdef(), // 36: Logitech mouse
    cnotdef(), // 37: Extended PS/2 mouse
    cnotdef(), // 38: Cyclom serial port (cy: not ported)
    cnotdef(), // 39: Mitsumi CD-ROM
    cnotdef(), // 40: network tunnel (tun: not ported)
    // 41: vnode disk driver
    Cell::new(cdev_disk_init(
        NVND, vndopen, vndclose, vndread, vndwrite, vndioctl,
    )),
    // 42: generic audio I/O
    Cell::new(cdev_audio_init(
        NAUDIO,
        audioopen,
        audioclose,
        audioread,
        audiowrite,
        audioioctl,
        audiokqfilter,
    )),
    cnotdef(), // 43
    cnotdef(), // 44: generic video I/O (video: not ported)
    cnotdef(), // 45: random data source (random: rnd.c's randomopen .., not ported)
    cnotdef(), // 46: performance counters (pctr: not ported)
    // 47: ram disk driver
    Cell::new(cdev_disk_init(
        NRD, rdopen, rdclose, rdread, rdwrite, rdioctl,
    )),
    cnotdef(), // 48
    cnotdef(), // 49: Bt848 video capture device (bktr: not ported)
    cnotdef(), // 50: Kernel symbols device (ksyms: not ported)
    cnotdef(), // 51: Kernel statistics (kstat: not ported)
    cnotdef(), // 52: MIDI I/O (midi: not ported)
    cnotdef(), // 53 was: sequencer I/O
    cnotdef(), // 54 was: RAIDframe disk driver
    cnotdef(), // 55:
    // The following slots are reserved for isdn4bsd.
    cnotdef(), // 56: i4b main device
    cnotdef(), // 57: i4b control device
    cnotdef(), // 58: i4b raw b-channel access
    cnotdef(), // 59: i4b trace device
    cnotdef(), // 60: i4b phone device
    // End of reserved slots for isdn4bsd.
    Cell::new(cdev_usb_init(NUSB, usbopen, usbclose, usbioctl)), // 61: USB controller
    // 62: USB generic HID
    Cell::new(cdev_usbdev_init(
        NUHID,
        uhidopen,
        uhidclose,
        uhidread,
        uhidwrite,
        uhidioctl,
        uhidkqfilter,
    )),
    // 63: USB generic driver
    Cell::new(cdev_usbdev_init(
        NUGEN,
        ugenopen,
        ugenclose,
        ugenread,
        ugenwrite,
        ugenioctl,
        ugenkqfilter,
    )),
    cnotdef(), // 64: USB printers (ulpt: not ported)
    cnotdef(), // 65: urio
    // 66: USB tty
    Cell::new(cdev_tty_init(
        NUCOM, ucomopen, ucomclose, ucomread, ucomwrite, ucomioctl, ucomstop, ucomtty,
    )),
    // 67: keyboards
    Cell::new(cdev_mouse_init(
        NWSKBD,
        wskbdopen,
        wskbdclose,
        wskbdread,
        wskbdioctl,
        wskbdkqfilter,
    )),
    // 68: mice
    Cell::new(cdev_mouse_init(
        NWSMOUSE,
        wsmouseopen,
        wsmouseclose,
        wsmouseread,
        wsmouseioctl,
        wsmousekqfilter,
    )),
    // 69: ws multiplexor
    Cell::new(cdev_mouse_init(
        NWSMUX,
        wsmuxopen,
        wsmuxclose,
        wsmuxread,
        wsmuxioctl,
        wsmuxkqfilter,
    )),
    cnotdef(),                                                   // 70: was: /dev/crypto
    cnotdef(), // 71: Cyclades-Z serial port (cztty: not ported)
    cnotdef(), // 72: PCI user (USER_PCICONF not configured)
    Cell::new(cdev_pf_init(NPF, pfopen, pfclose, pfioctl)), // 73: packet filter
    cnotdef(), // 74: ALTQ (deprecated)
    cnotdef(), // 75
    cnotdef(), // 76: generic radio I/O (radio: not ported)
    cnotdef(), // 77: was USB scanners
    cnotdef(), // 78
    Cell::new(cdev_bio_init(NBIO, bioopen, bioclose, bioioctl)), // 79: ioctl tunnel
    cnotdef(), // 80
    // 81: pseudo-tty ptm device
    Cell::new(cdev_ptm_init(NPTY, ptmopen, ptmclose, ptmioctl)),
    cnotdef(), // 82: devices hot plugging (hotplug: not ported)
    cnotdef(), // 83: ACPI (acpi: not ported)
    cnotdef(), // 84: EFI (efi: not ported)
    cnotdef(), // 85: NVRAM interface (nvram: not ported)
    cnotdef(), // 86
    cnotdef(), // 87: drm (not ported)
    Cell::new(cdev_gpio_init(NGPIO, gpioopen, gpioclose, gpioioctl)), // 88: gpio
    cnotdef(), // 89: vscsi (not ported)
    // 90: disk mapper
    Cell::new(cdev_disk_init(
        1,
        diskmapopen,
        diskmapclose,
        diskmapread,
        diskmapwrite,
        diskmapioctl,
    )),
    cnotdef(), // 91: pppx (not ported)
    #[cfg(feature = "fuse")]
    Cell::new(cdev_fuse_init(
        NFUSE,
        fuseopen,
        fuseclose,
        fuseread,
        fusewrite,
        fusekqfilter,
    )), // 92: fuse
    #[cfg(not(feature = "fuse"))]
    cnotdef(), // 92: fuse (feature `fuse` off)
    cnotdef(), // 93: Ethernet network tunnel (tap: not ported)
    cnotdef(), // 94: virtio console (viocon: not ported)
    cnotdef(), // 95: pvbus(4) control interface (not ported)
    cnotdef(), // 96: ipmi (not ported)
    cnotdef(), // 97: was switch(4)
    cnotdef(), // 98: FIDO/U2F security keys (fido: not ported)
    cnotdef(), // 99: PPP Access Concentrator (pppac: not ported)
    cnotdef(), // 100: USB joystick/gamecontroller (ujoy: not ported)
    cnotdef(), // 101: PSP (psp: not ported)
]);

/// `mem_no`: major device number of memory special file.
pub const MEM_NO: u32 = 2;

/// `swapdev`: the fake device implemented in `sw.c` used only internally to get to
/// `swstrategy`. It cannot be provided to the users, because the `swstrategy` routine munches
/// the `b_dev` and `b_blkno` entries before calling the appropriate driver. This would
/// horribly confuse, e.g. the hashing routines. Instead, `/dev/drum` is provided as a
/// character (raw) device.
pub const SWAPDEV: Dev = makedev(1, 0);

/// `chrtoblktbl[]`: the block major of each character major (`nchrtoblktbl` is its length).
pub static CHRTOBLKTBL: [Dev; 48] = [
    // VCHR      VBLK
    NODEV, // 0
    NODEV, // 1
    NODEV, // 2
    0,     // 3: wd
    NODEV, // 4
    NODEV, // 5
    NODEV, // 6
    NODEV, // 7
    NODEV, // 8
    2,     // 9: fd
    NODEV, // 10
    NODEV, // 11
    NODEV, // 12
    4,     // 13: sd
    NODEV, // 14
    6,     // 15: cd
    NODEV, // 16
    NODEV, // 17
    NODEV, // 18
    NODEV, // 19
    NODEV, // 20
    NODEV, // 21
    NODEV, // 22
    NODEV, // 23
    NODEV, // 24
    NODEV, // 25
    NODEV, // 26
    NODEV, // 27
    NODEV, // 28
    NODEV, // 29
    NODEV, // 30
    NODEV, // 31
    NODEV, // 32
    NODEV, // 33
    NODEV, // 34
    NODEV, // 35
    NODEV, // 36
    NODEV, // 37
    NODEV, // 38
    NODEV, // 39
    NODEV, // 40
    14,    // 41: vnd
    NODEV, // 42
    NODEV, // 43
    NODEV, // 44
    NODEV, // 45
    NODEV, // 46
    17,    // 47: rd
];

/// `iskmemdev`: returns true if dev is /dev/mem or /dev/kmem.
pub fn iskmemdev(dev: Dev) -> bool {
    major(dev) == MEM_NO && minor(dev) < 2
}

/// `iszerodev`: returns true if dev is /dev/zero.
pub fn iszerodev(dev: Dev) -> bool {
    major(dev) == MEM_NO && minor(dev) == 12
}

/// `getnulldev`: the device number of /dev/null.
pub fn getnulldev() -> Dev {
    makedev(MEM_NO, 2)
}
/* </CODE> */
