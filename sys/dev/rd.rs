/*	$OpenBSD: rd.c,v 1.14 2022/04/06 18:59:27 naddy Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2011 Matthew Dempsky <matthew@dempsky.org>
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

//! rd(4): the RAM disk, a disk whose sectors are the bytes of a file system image the kernel
//! carries (OpenBSD's install and rescue kernels boot from one, `bsd.rd`).
//!
//! Upstream: sys/dev/rd.c @ 3ce1f3f79392
//!
//! `rdattach` (a pseudo-device in `pdevinit[]`) attaches `rd0`; its block (`bdevsw[17]`) and
//! character (`cdevsw[47]`) entries open, read and write the image through
//! [`rdstrategy`], with the label `rdgetdisklabel` spoofs and `readdisklabel` reads from
//! sector 1 of the image (`disklabel(5)`'s `rdroot` type: partition `a` holds the root file
//! system). `rd0a` is the root device of a RAMDISK kernel (`config bsd root on rd0a`).
//!
//! ## Deviations
//! - The image is not the array `rd_root_image[ROOTBYTES]` that `rdsetroot(8)` patches into
//!   the kernel file: it is the Limine module `/ramdisk.ffs` (made by `makefs(8)` from the
//!   userland build), which `stand` hands over with [`rd_root_image_set`] before `main`. The
//!   bootloader loads it into "executable and modules" memory that is never reclaimed and maps
//!   it writable in its direct map, so the C's writes to the image still work. Without a
//!   module the image is empty (`rd_root_size` 0) and `rd0` still attaches, as the C always
//!   attaches one; it then has no label to read: `rdgetdisklabel` returns the spoofed label
//!   (initialised, DUID zero) without calling `readdisklabel` (M14c). The C never runs
//!   with an empty image; there the read of sector 1 hits the end of the disk, ends with
//!   no error and nothing transferred, and the label check looks at the stale contents of
//!   the buffer, which on an installed arm64 system sometimes were the root disk's label,
//!   so `rd0` got the root's DUID and opendev(3) refused the duplicate (`fsck` failed).
//! - `MINIROOTSIZE` is the cargo feature `miniroot` (M14c, `bsd.rd`): the kernel then holds
//!   the C's `rd_root_image[ROOTBYTES]` and `rd_root_size`, under those symbol names so that
//!   `rdsetroot` (`cargo xtask rdsetroot`, `tools/xtask/src/rdsetroot.rs`) finds them
//!   in the ELF and copies the miniroot file system in, as `usr.sbin/rdsetroot` does. The
//!   size is the build-time number `EMIBSD_MINIROOTSIZE` (sectors; the C takes `option
//!   MINIROOTSIZE=n` from the kernel configuration), 512 by default. The kernel then roots
//!   on `rd0a` by itself (`rdattach` calls `swapconf_rdroot`, the C's `config bsd root on
//!   rd0a`), unless a module already did. A module (Limine's `ramdisk.ffs`) still overrides
//!   the compiled-in image.
//! - `rdgetdisklabel` writes the label being read into a local, not into the in-core label
//!   `rdstrategy` checks transfers against (a Rust `&mut` may not alias what the strategy
//!   reads): when no partition is open it first publishes the initialised label
//!   (`initdisklabel` of the spoofed one, which is what the C's in-core label holds while
//!   `readdisklabel` reads: the raw partition, the sector size and the geometry do not change
//!   during the read), and `rdopen` installs the result. `DIOCWDINFO` writes a copy of the
//!   in-core label for the same reason.
//! - The fake `cfdata` of `rdattach` is a `static` [`Cfdata`] instead of a function-local
//!   `static struct cfdata`.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use crate::kern::kern_malloc::{malloc, mallocarray};
use crate::kern::kern_physio::{minphys, physio};
use crate::kern::subr_autoconf::{ALLDEVS, device_ref, device_unref};
use crate::kern::subr_disk::{
    bounds_check_with_label, disk_attach, disk_closepart, disk_detach, disk_gone, disk_lock,
    disk_lock_nointr, disk_lookup, disk_openpart, disk_unlock, dkcksum, initdisklabel,
    setdisklabel,
};
use crate::kern::subr_prf::{panic, snprintf};
use crate::kern::vfs_bio::biodone;
use crate::machine::disklabel::{readdisklabel, writedisklabel};
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::{B_ERROR, B_READ, B_WRITE, Buf};
use crate::sys::device::{
    CfMatch, Cfattach, Cfdata, Cfdriver, DV_DISK, DVF_ACTIVE, Device, FSTATE_NOTFOUND, Softc,
};
use crate::sys::disk::Disk;
use crate::sys::disklabel::{
    DISKLABEL_SIZE, DISKMAGIC, DTYPE_SCSI, Disklabel, Partinfo, disklabeldev, diskpart, diskunit,
    dl_getpoffset, dl_setdsize,
};
use crate::sys::dkio::{DIOCGDINFO, DIOCGPART, DIOCGPDINFO, DIOCRLDINFO, DIOCSDINFO, DIOCWDINFO};
use crate::sys::errno::Errno;
use crate::sys::fcntl::FWRITE;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};
use crate::sys::param::{DEV_BSHIFT, DEV_BSIZE};
use crate::sys::proc::Proc;
use crate::sys::types::{Daddr, Dev};
use crate::sys::uio::Uio;

/// `MINIROOTSIZE`: the size of the compiled-in image, in sectors: `EMIBSD_MINIROOTSIZE`
/// when the build sets it, the C's default 512 otherwise.
pub const MINIROOTSIZE: usize = match option_env!("EMIBSD_MINIROOTSIZE") {
    Some(text) => parse_sectors(text),
    None => 512,
};

/// The decimal number in `text`, at least 1 (a compile-time error otherwise).
const fn parse_sectors(text: &str) -> usize {
    let digits = text.as_bytes();
    assert!(!digits.is_empty(), "EMIBSD_MINIROOTSIZE is empty");
    let mut n = 0usize;
    let mut i = 0;
    while i < digits.len() {
        assert!(
            digits[i].is_ascii_digit(),
            "EMIBSD_MINIROOTSIZE is not a number"
        );
        n = n * 10 + (digits[i] - b'0') as usize;
        i += 1;
    }
    assert!(n > 0, "EMIBSD_MINIROOTSIZE is zero");
    n
}

/// `ROOTBYTES`: the default size of the compiled-in image, in bytes.
pub const ROOTBYTES: usize = MINIROOTSIZE << DEV_BSHIFT;

/// `NRD`: rd(4) units (`pseudo-device rd 1`).
pub const NRD: i32 = 1;

/// `struct rd_softc`.
#[repr(C)]
pub struct RdSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_dk`.
    pub sc_dk: Disk,
}

// SAFETY: `#[repr(C)]` with the device first; the disk is all-zero valid (`sys/disk.rs`).
unsafe impl Softc for RdSoftc {}

/// `rd_root_image[ROOTBYTES]` (feature `miniroot`): the compiled-in image, `rdsetroot`'s
/// target, under the C's symbol name. Not all zero, so it is file contents in the ELF's data
/// segment (the C's `"|This is the root ramdisk!\n"`).
#[cfg(feature = "miniroot")]
#[repr(transparent)]
struct RootImage(core::cell::UnsafeCell<[u8; ROOTBYTES]>);

// SAFETY: rd(4) alone touches the array, through `RD_ROOT_IMAGE`, and (like the C) as one
// disk with its own serialisation (`rdstrategy` at splbio / the disk queue).
#[cfg(feature = "miniroot")]
unsafe impl Sync for RootImage {}

#[cfg(feature = "miniroot")]
#[unsafe(export_name = "rd_root_image")]
static ROOT_IMAGE: RootImage = RootImage({
    let banner = b"|This is the root ramdisk!\n";
    let mut image = [0u8; ROOTBYTES];
    let mut i = 0;
    while i < banner.len() {
        image[i] = banner[i];
        i += 1;
    }
    core::cell::UnsafeCell::new(image)
});

/// `rd_root_image`: the file system image (the Limine module, see the module's
/// deviations, or the compiled-in array with feature `miniroot`); NULL without either.
#[cfg(feature = "miniroot")]
static RD_ROOT_IMAGE: AtomicPtr<u8> = AtomicPtr::new(ROOT_IMAGE.0.get().cast::<u8>());
#[cfg(not(feature = "miniroot"))]
static RD_ROOT_IMAGE: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());

/// `rd_root_size`: the image's size in bytes (`ROOTBYTES` with feature `miniroot`: the
/// symbol `rdsetroot` reads).
#[cfg(feature = "miniroot")]
#[unsafe(export_name = "rd_root_size")]
static RD_ROOT_SIZE: AtomicU32 = AtomicU32::new(ROOTBYTES as u32);
#[cfg(not(feature = "miniroot"))]
static RD_ROOT_SIZE: AtomicU32 = AtomicU32::new(0);

/// `rd_ca`.
pub static RD_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<RdSoftc>(),
    ca_match: Some(rd_match),
    ca_attach: rd_attach,
    ca_detach: Some(rd_detach),
    ca_activate: None,
};

/// `rd_cd`.
pub static RD_CD: Cfdriver = Cfdriver::new(b"rd", DV_DISK, 0);

/// `rdattach`'s fake `cf`.
static RD_CF: Cfdata = Cfdata::new(&RD_CA, &RD_CD, 0, FSTATE_NOTFOUND, &[], 0, &[], 0, 0);

/// Hands the root image to the driver: the boot glue's module, `size` bytes at `image`.
///
/// # Safety
///
/// Called once, before `main` attaches the pseudo-devices. `image` points to `size` bytes
/// that are readable and writable for the kernel's lifetime and that nothing else uses (the
/// caller does not read the module through another reference afterwards).
pub unsafe fn rd_root_image_set(image: *mut u8, size: usize) {
    RD_ROOT_IMAGE.store(image, Ordering::Relaxed);
    RD_ROOT_SIZE.store(u32::try_from(size).unwrap_or(u32::MAX), Ordering::Relaxed);
}

/// `rd_root_size`: the image's size in bytes, 0 without a module.
pub fn rd_root_size() -> u32 {
    RD_ROOT_SIZE.load(Ordering::Relaxed)
}

/// `rdlookup(unit)`: the attached unit, referenced (`disk_lookup`).
fn rdlookup(unit: u32) -> Option<NonNull<Device>> {
    disk_lookup(&RD_CD, i32::try_from(unit).ok()?)
}

/// The softc of a unit `rdlookup` returned.
fn rd_softc(dv: NonNull<Device>) -> &'static RdSoftc {
    // SAFETY: every `rd` device is an `RdSoftc` allocated by `rdattach`, which is never freed
    // while the reference `rdlookup` took is held (or, for `rd_attach`, while attached).
    unsafe { dv.as_ref().softc::<RdSoftc>() }
}

/// `rdattach(num)`: attaches the one unit there is.
pub fn rdattach(_num: i32) {
    // Feature `miniroot`: the compiled-in image makes this kernel `bsd.rd`, rooted on rd0a
    // (`config bsd root on rd0a swap on rd0b`); a module's image already did it.
    #[cfg(feature = "miniroot")]
    if RD_ROOT_IMAGE.load(Ordering::Relaxed) == ROOT_IMAGE.0.get().cast::<u8>() {
        // SAFETY: `main` attaches the pseudo-devices on the boot CPU alone, before it
        // configures the root device and reads `mountroot`.
        unsafe { crate::conf::swapgeneric::swapconf_rdroot() };
    }
    // There's only one rd_root_image, so only attach one rd.
    let num = 1usize;

    // XXX: Fake up more?

    RD_CD.cd_ndevs.set(num as i32);
    let slot = size_of::<Option<NonNull<Device>>>();
    let Some(devs) = mallocarray(num, slot, M_DEVBUF, M_NOWAIT) else {
        panic(format_args!("rdattach: out of memory"));
    };
    let devs = devs.cast::<Option<NonNull<Device>>>();
    RD_CD.cd_devs.set(devs.as_ptr());

    for i in 0..num {
        // Allocate the softc and initialize it.
        let Some(mem) = malloc(size_of::<RdSoftc>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
            panic(format_args!("rdattach: out of memory"));
        };
        let sc = mem.cast::<RdSoftc>();
        // SAFETY: `size_of::<RdSoftc>()` zeroed bytes, aligned (malloc's chunks are aligned
        // to their power-of-two size); all-zero is a valid `RdSoftc` (`Softc`).
        let dev: &'static Device = unsafe { &(*sc.as_ptr()).sc_dev };
        dev.dv_class.set(DV_DISK);
        dev.dv_cfdata.set(Some(&RD_CF));
        dev.dv_flags.set(DVF_ACTIVE);
        dev.dv_unit.set(i as i32);
        let mut name = [0u8; 16];
        if snprintf(&mut name, format_args!("rd{i}")) >= name.len() {
            panic(format_args!("rdattach: device name too long"));
        }
        dev.dv_xname.set(name);
        dev.dv_ref.store(1, Ordering::Relaxed);

        // Attach it to the device tree.
        // SAFETY: `devs` has `num` slots, `i < num`.
        unsafe { devs.as_ptr().add(i).write(Some(NonNull::from(dev))) };
        // SAFETY: the device is in no list and lives until it is detached.
        unsafe { ALLDEVS.0.insert_tail(dev) };
        device_ref(dev);

        // Finish initializing.
        rd_attach(None, dev, ptr::null_mut());
    }
}

/// `rd_match`: rd never attaches through autoconfiguration.
pub fn rd_match(_parent: Option<&Device>, _match: &CfMatch, _aux: *mut c_void) -> i32 {
    0
}

/// `rd_attach`: attaches the disk.
pub fn rd_attach(_parent: Option<&Device>, self_: &Device, _aux: *mut c_void) {
    let sc = rd_softc(NonNull::from(self_));

    // Attach disk.
    let mut name = [0u8; 16];
    let xname = sc.sc_dev.xname().as_bytes();
    let n = xname.len().min(name.len());
    name[..n].copy_from_slice(&xname[..n]);
    sc.sc_dk.dk_name.set(name);
    disk_attach(Some(&sc.sc_dev), &sc.sc_dk);
}

/// `rd_detach`.
pub fn rd_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = rd_softc(NonNull::from(self_));

    disk_gone(rdopen, self_.dv_unit.get() as u32);

    // Detach disk.
    disk_detach(&sc.sc_dk);

    Ok(())
}

/// `rdopen`.
pub fn rdopen(dev: Dev, _flag: i32, fmt: i32, _p: &Proc) -> Result<(), Errno> {
    let unit = diskunit(dev);
    let part = diskpart(dev);

    let dv = rdlookup(unit).ok_or(Errno::ENXIO)?;
    let sc = rd_softc(dv);

    let result = disk_lock(&sc.sc_dk).and_then(|()| {
        let result = (|| {
            if sc.sc_dk.dk_openmask.get() == 0 {
                // Load the partition info if not already loaded.
                let mut lp = Disklabel::zeroed();
                rdgetdisklabel(dev, sc, &mut lp, false)?;
                // SAFETY: under the disk lock, with no partition open: nobody else holds
                // the in-core label.
                if let Some(dl) = unsafe { sc.sc_dk.label_mut() } {
                    *dl = lp;
                }
            }

            disk_openpart(&sc.sc_dk, part, fmt, true)
        })();
        disk_unlock(&sc.sc_dk);
        result
    });

    // SAFETY: the reference `rdlookup` took.
    unsafe { device_unref(dv) };
    result
}

/// `rdclose`.
pub fn rdclose(dev: Dev, _flag: i32, fmt: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let unit = diskunit(dev);
    let part = diskpart(dev);

    let dv = rdlookup(unit).ok_or(Errno::ENXIO)?;
    let sc = rd_softc(dv);

    disk_lock_nointr(&sc.sc_dk);

    disk_closepart(&sc.sc_dk, part, fmt);

    disk_unlock(&sc.sc_dk);
    // SAFETY: the reference `rdlookup` took.
    unsafe { device_unref(dv) };
    Ok(())
}

/// `rdstrategy`: copies between the buffer and the image.
pub fn rdstrategy(bp: &'static Buf) {
    let dv = rdlookup(diskunit(bp.b_dev.get()));
    let label = dv.and_then(|dv| rd_softc(dv).sc_dk.label());

    match label {
        None => {
            bp.b_error.set(Some(Errno::ENXIO));
            // bad:
            bp.set(B_ERROR);
            bp.b_resid
                .set(usize::try_from(bp.b_bcount.get()).unwrap_or(0));
        }
        Some(lp) => {
            // Validate the request.
            if bounds_check_with_label(bp, &lp) {
                rd_transfer(bp, &lp);
            }
        }
    }

    // done:
    let s = splbio();
    biodone(bp);
    splx(s);
    if let Some(dv) = dv {
        // SAFETY: the reference `rdlookup` took.
        unsafe { device_unref(dv) };
    }
}

/// The transfer of `rdstrategy`, once the request is validated.
fn rd_transfer(bp: &Buf, lp: &Disklabel) {
    // XXX: Worry about overflow when computing off?
    let size = rd_root_size() as u64;
    let p = &lp.d_partitions[diskpart(bp.b_dev.get()) as usize];
    let mut off =
        dl_getpoffset(p) * u64::from(lp.d_secsize) + (bp.b_blkno.get() as u64) * DEV_BSIZE as u64;
    if off > size {
        off = size;
    }
    let bcount = usize::try_from(bp.b_bcount.get()).unwrap_or(0);
    let xfer = bcount.min((size - off) as usize);
    let image = RD_ROOT_IMAGE.load(Ordering::Relaxed);
    if xfer > 0 && !image.is_null() {
        // SAFETY: `off + xfer <= rd_root_size`, the image's length (`rd_root_image_set`'s
        // contract); `b_data` is the busy buffer's mapping of at least `b_bcount >= xfer`
        // bytes; the image and a buffer never overlap.
        unsafe {
            let addr = image.add(off as usize);
            if bp.isset(B_READ) {
                ptr::copy_nonoverlapping(addr, bp.b_data.get(), xfer);
            } else {
                ptr::copy_nonoverlapping(bp.b_data.get(), addr, xfer);
            }
        }
    }
    bp.b_resid.set(bcount - xfer);
}

/// `rdioctl`.
pub fn rdioctl(dev: Dev, cmd: u64, data: &mut [u8], fflag: i32, _p: &Proc) -> Result<(), Errno> {
    let dv = rdlookup(diskunit(dev)).ok_or(Errno::ENXIO)?;
    let sc = rd_softc(dv);

    let result = match cmd {
        DIOCRLDINFO => {
            let mut lp = Disklabel::zeroed();
            let _ = rdgetdisklabel(dev, sc, &mut lp, false);
            // SAFETY: the driver's own label, no other reference to it is live.
            if let Some(dl) = unsafe { sc.sc_dk.label_mut() } {
                *dl = lp;
            }
            Ok(())
        }

        DIOCGPDINFO => {
            let mut lp = Disklabel::zeroed();
            let _ = rdgetdisklabel(dev, sc, &mut lp, true);
            copyout_label(&lp, data);
            Ok(())
        }

        DIOCGDINFO => {
            if let Some(lp) = sc.sc_dk.label() {
                copyout_label(&lp, data);
            }
            Ok(())
        }

        DIOCGPART => {
            if let Some(lp) = sc.sc_dk.dk_label.get() {
                let part = diskpart(dev) as usize;
                let pi = Partinfo {
                    disklab: lp.as_ptr(),
                    // SAFETY: `lp` is the live in-core label; the projection only computes
                    // the address of one of its partitions.
                    part: unsafe { &raw mut (*lp.as_ptr()).d_partitions[part] },
                };
                pi.store(data);
            }
            Ok(())
        }

        DIOCWDINFO | DIOCSDINFO => {
            if fflag & FWRITE == 0 {
                Err(Errno::EBADF)
            } else {
                disk_lock(&sc.sc_dk).and_then(|()| {
                    let mut nlp = Disklabel::from_bytes(data);
                    // SAFETY: under the disk lock; the borrow ends before the strategy runs.
                    let result = match unsafe { sc.sc_dk.label_mut() } {
                        Some(olp) => setdisklabel(olp, &mut nlp, sc.sc_dk.dk_openmask.get()),
                        None => Err(Errno::ENXIO),
                    };
                    let result = result.and_then(|()| {
                        if cmd == DIOCWDINFO {
                            let mut lp = sc.sc_dk.label().unwrap_or_default();
                            writedisklabel(disklabeldev(dev), rdstrategy, &mut lp)
                        } else {
                            Ok(())
                        }
                    });
                    disk_unlock(&sc.sc_dk);
                    result
                })
            }
        }

        _ => Ok(()),
    };

    // SAFETY: the reference `rdlookup` took.
    unsafe { device_unref(dv) };
    result
}

/// `*(struct disklabel *)data = *lp`: the label into an `ioctl` buffer.
fn copyout_label(lp: &Disklabel, data: &mut [u8]) {
    let n = data.len().min(DISKLABEL_SIZE);
    data[..n].copy_from_slice(&lp.as_bytes()[..n]);
}

/// `rdgetdisklabel`: spoofs a label for the whole image and reads the one it carries.
pub fn rdgetdisklabel(
    dev: Dev,
    sc: &RdSoftc,
    lp: &mut Disklabel,
    spoofonly: bool,
) -> Result<(), Errno> {
    let rd_root_size = rd_root_size();
    *lp = Disklabel::zeroed();

    lp.d_secsize = DEV_BSIZE as u32;
    lp.d_ntracks = 1;
    lp.d_nsectors = rd_root_size >> DEV_BSHIFT;
    lp.d_ncylinders = 1;
    lp.d_secpercyl = lp.d_nsectors;
    if lp.d_secpercyl == 0 {
        lp.d_secpercyl = 100;
        // as long as it's not 0 - readdisklabel divides by it
    }

    strncpy(&mut lp.d_typename, b"RAM disk");
    lp.d_type = DTYPE_SCSI;
    strncpy(&mut lp.d_packname, b"fictitious");
    dl_setdsize(lp, u64::from(lp.d_nsectors));
    lp.d_version = 1;

    lp.d_magic = DISKMAGIC;
    lp.d_magic2 = DISKMAGIC;
    lp.d_checksum = dkcksum(lp);

    // The label rdstrategy checks the reads below against (see the module's deviations).
    if sc.sc_dk.dk_openmask.get() == 0 {
        let mut incore = *lp;
        if initdisklabel(&mut incore).is_ok() {
            // SAFETY: no partition is open and the caller serialises label changes (the
            // disk lock in `rdopen`): nobody else holds the in-core label.
            if let Some(dl) = unsafe { sc.sc_dk.label_mut() } {
                *dl = incore;
            }
        }
    }

    // No image (see the module's deviations): there is no label to read, and a read of the
    // empty disk ends at once with nothing transferred, which would leave readdisklabel
    // looking at whatever its buffer held before (another disk's label, its DUID with it).
    if rd_root_size == 0 {
        return initdisklabel(lp);
    }

    // Call the generic disklabel extraction routine.
    readdisklabel(disklabeldev(dev), rdstrategy, lp, spoofonly)
}

/// `strncpy(dst, src, sizeof(dst))`: `src`, then NULs to the end of `dst`.
fn strncpy(dst: &mut [u8], src: &[u8]) {
    let n = src.len().min(dst.len());
    dst[..n].copy_from_slice(&src[..n]);
    dst[n..].fill(0);
}

/// `rdread`: the raw device's read, straight into the user's buffer (physio(9)).
pub fn rdread(dev: Dev, uio: &mut Uio<'_>, _ioflag: i32) -> Result<(), Errno> {
    physio(rdstrategy, dev, B_READ, minphys, uio)
}

/// `rdwrite`: the raw device's write, straight from the user's buffer (physio(9)).
pub fn rdwrite(dev: Dev, uio: &mut Uio<'_>, _ioflag: i32) -> Result<(), Errno> {
    physio(rdstrategy, dev, B_WRITE, minphys, uio)
}

/// `rddump`.
pub fn rddump(_dev: Dev, _blkno: Daddr, _va: *mut u8, _size: usize) -> Result<(), Errno> {
    Err(Errno::ENXIO)
}

/// `rdsize`.
pub fn rdsize(_dev: Dev) -> Daddr {
    -1
}

#[cfg(test)]
mod tests;
