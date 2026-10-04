/*	$OpenBSD: vnd.c,v 1.183 2025/11/17 14:27:43 jsg Exp $	*/
/*	$NetBSD: vnd.c,v 1.26 1996/03/30 23:06:11 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1988 University of Utah.
 * Copyright (c) 1990, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * the Systems Programming Group of the University of Utah Computer
 * Science Department.
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
 */
/* </LICENSES> */

//! vnd(4): the vnode disk driver, a disk whose sectors are the bytes of a file (or of a
//! block device): file system images become mountable disks.
//!
//! Upstream: sys/dev/vnd.c @ 3ce1f3f79392
//!
//! There is a security issue involved with this driver. Once mounted all access to the
//! contents of the "mapped" file via the special file is controlled by the permissions on
//! the special file, the protection of the mapped file is ignored (effectively, by using
//! root credentials in all transactions).
//!
//! `vndattach` (`pseudo-device vnd 4` in `pdevinit[]`) allocates the units `vnd0` ..
//! `vnd3`; they are not in the device tree and have no `cfdriver`, as in C. vnconfig(8)
//! configures a unit over a file with [`VNDIOCSET`] (`/dev/rvndNc`), which opens the file,
//! records its size and attaches the disk. The block (`bdevsw[14]`) and character
//! (`cdevsw[41]`) entries, the same majors on amd64 and arm64, read and write the file
//! through [`vndstrategy`], which turns each buffer into a `vn_rdwr` on the file at the
//! partition's offset; an image with no disklabel gets the label `vndgetdisklabel`
//! fabricates (the raw partition covers the whole file) plus what `readdisklabel` spoofs
//! from its first sector (a FAT boot sector gives partition `i`).
//!
//! ## Deviations
//! - `vnd_softc` and `numvnd` are an `AtomicPtr` to the `mallocarray`'d softcs and an
//!   `AtomicI32`, written once by `vndattach`; [`vnd_softc`] is `&vnd_softc[unit]` with the
//!   bounds check the callers do in C. The softc's members that change after attach are
//!   `Cell`s (the C changes them through the pointer, under `dk_lock` or the kernel lock);
//!   `sc_vp`, `sc_cred` and `sc_keyctx` are `Option`s of the C's possibly NULL pointers.
//! - `vndgetdisklabel` builds the label in the caller's local, not in the in-core label
//!   `vndstrategy` checks transfers against (a Rust `&mut` may not alias what the strategy
//!   reads): when no partition is open it first publishes the fabricated label as
//!   `initdisklabel` leaves it (what the C's in-core label holds while `readdisklabel`
//!   reads: the raw partition, the sector size and the geometry do not change during the
//!   read), and `vndopen` and `DIOCRLDINFO` install the result, as rd(4) does. `DIOCWDINFO`
//!   writes a copy of the in-core label for the same reason.
//! - `VNDIOCSET`'s body is the helper `vndioctl_set` and its `goto fail` a labelled block;
//!   the clamped `vnd_keylen` and the returned `vnd_size` are written back into the ioctl
//!   buffer only when the ioctl succeeds (`sys_ioctl` copies nothing out on an error).
//! - `VNDIOCCLR` also clears `sc_keyctx` after freeing it (the C leaves the stale pointer
//!   until the next `VNDIOCSET` overwrites it; no path reads it in between).
//! - `vndbdevsize` has no `d_ioctl == NULL` test: a Rust `fn` is never NULL; an empty
//!   `bdevsw` slot answers `ENODEV`, which gives the same 0.
//! - `VNDDEBUG`'s `DNPRINTF` tracing is not configured (it is off in GENERIC); the
//!   `DIAGNOSTIC` "sloppy" message is behind the `diagnostic` feature.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use libkern::{explicit_bzero, strlcpy};

use crate::crypto::blf::{
    BLF_MAXUTILIZED, BlfCtx, blf_cbc_decrypt, blf_cbc_encrypt, blf_ecb_encrypt, blf_key,
};
use crate::dev::vndioctl::{VNDIOCCLR, VNDIOCGET, VNDIOCSET, VNDNLEN, VndIoctl, VndUser};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_physio::{minphys, physio};
use crate::kern::kern_prot::{crdup, crfree, suser};
use crate::kern::subr_autoconf::device_ref;
use crate::kern::subr_disk::{
    bounds_check_with_label, disk_attach, disk_closepart, disk_construct, disk_detach, disk_lock,
    disk_lock_nointr, disk_openpart, disk_unlock, dkcksum, initdisklabel, setdisklabel,
};
use crate::kern::subr_prf::{panic, printf, snprintf};
use crate::kern::vfs_bio::biodone;
use crate::kern::vfs_lookup::ndinit;
use crate::kern::vfs_vnops::{vn_close, vn_open, vn_rdwr};
use crate::kern::vfs_vops::{VOP_GETATTR, VOP_UNLOCK};
use crate::machine::conf::bdevsw;
use crate::machine::copy::{copyin, copyinstr};
use crate::machine::cpu::curproc;
use crate::machine::disklabel::{readdisklabel, writedisklabel};
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::{B_ERROR, B_READ, B_WRITE, Buf};
use crate::sys::device::Device;
use crate::sys::disk::Disk;
use crate::sys::disklabel::{
    DISKLABEL_SIZE, DISKMAGIC, Disklabel, Partinfo, disklabeldev, diskpart, diskunit,
    dl_getpoffset, dl_getpsize, dl_setdsize,
};
use crate::sys::dkio::{DIOCGDINFO, DIOCGPART, DIOCGPDINFO, DIOCRLDINFO, DIOCSDINFO, DIOCWDINFO};
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::limits::UINT_MAX;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::namei::{NiDirp, UNVEIL_READ, UNVEIL_WRITE};
use crate::sys::param::{DEV_BSIZE, dbtob};
use crate::sys::proc::Proc;
use crate::sys::types::{Daddr, Dev, Off, major};
use crate::sys::ucred::Ucred;
use crate::sys::uio::{Uio, UioRw, UioSeg};
use crate::sys::vnode::{IO_NOCACHE, IO_NOLIMIT, IO_SYNC, VBLK, Vattr, Vnode};

/// `NVND`: vnd(4) units (`pseudo-device vnd 4` in GENERIC).
pub const NVND: i32 = 4;

// sc_flags
/// `VNF_INITED`: the unit is configured over a file.
pub const VNF_INITED: i32 = 0x0001;
/// `VNF_HAVELABEL`: the label has been read.
pub const VNF_HAVELABEL: i32 = 0x0002;
/// `VNF_READONLY`: the file could only be opened for reading.
pub const VNF_READONLY: i32 = 0x0004;

/// `struct vnd_softc`.
#[repr(C)]
pub struct VndSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_dk`.
    pub sc_dk: Disk,
    /// `sc_file`: file we're covering.
    pub sc_file: Cell<[u8; VNDNLEN]>,
    /// `sc_flags`: flags (`VNF_*`).
    pub sc_flags: Cell<i32>,
    /// `sc_type`: `d_type` we are emulating.
    pub sc_type: Cell<u16>,
    /// `sc_size`: size of vnd in sectors.
    pub sc_size: Cell<usize>,
    /// `sc_secsize`: sector size in bytes.
    pub sc_secsize: Cell<usize>,
    /// `sc_nsectors`: # of sectors per track.
    pub sc_nsectors: Cell<usize>,
    /// `sc_ntracks`: # of tracks per cylinder.
    pub sc_ntracks: Cell<usize>,
    /// `sc_vp`: vnode.
    pub sc_vp: Cell<Option<&'static Vnode>>,
    /// `sc_cred`: credentials.
    pub sc_cred: Cell<Option<&'static Ucred>>,
    /// `sc_keyctx`: key context (a `malloc`'d Blowfish context), `None` without a key.
    pub sc_keyctx: Cell<Option<NonNull<BlfCtx>>>,
}

/// `vnd_softc`: the `numvnd` softcs `vndattach` allocated (NULL before).
static VND_SOFTC: AtomicPtr<VndSoftc> = AtomicPtr::new(ptr::null_mut());

/// `numvnd`.
static NUMVND: AtomicI32 = AtomicI32::new(0);

/// `numvnd`: the units `vndattach` allocated.
pub fn numvnd() -> i32 {
    NUMVND.load(Ordering::Relaxed)
}

/// `&vnd_softc[unit]`, or `None` when `unit >= numvnd` (the C's `ENXIO` checks).
pub fn vnd_softc(unit: u32) -> Option<&'static VndSoftc> {
    if i64::from(unit) >= i64::from(numvnd()) {
        return None;
    }
    let base = VND_SOFTC.load(Ordering::Relaxed);
    // SAFETY: `vndattach` allocated `numvnd` softcs at `base` (stored before `numvnd`) and
    // they are never freed; `unit < numvnd`.
    Some(unsafe { &*base.add(unit as usize) })
}

/// `VNDRW(v)`: the open mode of the file.
fn vndrw(sc: &VndSoftc) -> i32 {
    if sc.sc_flags.get() & VNF_READONLY != 0 {
        FREAD
    } else {
        FREAD | FWRITE
    }
}

/// `vndencrypt`: encrypts (or decrypts) `addr`, the sectors from `off` on, a `DEV_BSIZE`
/// sector at a time in CBC mode, each with the encrypted sector number as its IV.
pub fn vndencrypt(sc: &VndSoftc, addr: &mut [u8], off: Daddr, encrypt: bool) {
    let Some(ctx) = sc.sc_keyctx.get() else {
        return;
    };
    // SAFETY: the context `VNDIOCSET` allocated and keyed; `VNDIOCCLR` frees it only when
    // no other partition of the unit is open, so no transfer runs on it.
    let ctx = unsafe { ctx.as_ref() };

    let bsize = dbtob(1);
    for (off, sector) in (off..).zip(addr.chunks_exact_mut(bsize)) {
        let mut iv = off.to_ne_bytes();
        blf_ecb_encrypt(ctx, &mut iv);
        if encrypt {
            blf_cbc_encrypt(ctx, &iv, sector);
        } else {
            blf_cbc_decrypt(ctx, &iv, sector);
        }
    }
}

/// `vndencryptbuf`: [`vndencrypt`] over a buffer's data, from its block number on.
pub fn vndencryptbuf(sc: &VndSoftc, bp: &Buf, encrypt: bool) {
    // SAFETY: the buffer is busy and ours (the strategy routine's), mapped for `b_bcount`
    // bytes.
    let data = unsafe { bp.data() };
    vndencrypt(sc, data, bp.b_blkno.get(), encrypt);
}

/// `vndattach(num)`: allocates `num` units; called by `main()` at boot time.
pub fn vndattach(num: i32) {
    if num <= 0 {
        return;
    }
    let n = num as usize;
    let Some(mem) = mallocarray(n, size_of::<VndSoftc>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        printf(format_args!("WARNING: no memory for vnode disks\n"));
        return;
    };
    let base = mem.cast::<VndSoftc>();
    for i in 0..n {
        // SAFETY: `n` zeroed softcs, aligned (malloc's chunks are aligned to their
        // power-of-two size); all-zero is a valid `VndSoftc` (the device and the disk are
        // all-zero valid, the `Option`s are `None`), never freed.
        let sc: &'static VndSoftc = unsafe { &*base.as_ptr().add(i) };

        sc.sc_dev.dv_unit.set(i as i32);
        let mut name = [0u8; 16];
        snprintf(&mut name, format_args!("vnd{i}"));
        sc.sc_dev.dv_xname.set(name);
        disk_construct(&sc.sc_dk);
        device_ref(&sc.sc_dev);
    }
    VND_SOFTC.store(base.as_ptr(), Ordering::Relaxed);
    NUMVND.store(num, Ordering::Relaxed);
}

/// `vndopen`.
pub fn vndopen(dev: Dev, flags: i32, mode: i32, _p: &Proc) -> Result<(), Errno> {
    let unit = diskunit(dev);
    let sc = vnd_softc(unit).ok_or(Errno::ENXIO)?;

    disk_lock(&sc.sc_dk)?;

    let result = (|| {
        if flags & FWRITE != 0 && sc.sc_flags.get() & VNF_READONLY != 0 {
            return Err(Errno::EROFS);
        }

        if sc.sc_flags.get() & VNF_INITED != 0
            && sc.sc_flags.get() & VNF_HAVELABEL == 0
            && sc.sc_dk.dk_openmask.get() == 0
        {
            sc.sc_flags.set(sc.sc_flags.get() | VNF_HAVELABEL);
            let mut lp = Disklabel::zeroed();
            let _ = vndgetdisklabel(dev, sc, &mut lp, false);
            // SAFETY: under the disk lock, with no partition open: nobody else holds the
            // in-core label.
            if let Some(dl) = unsafe { sc.sc_dk.label_mut() } {
                *dl = lp;
            }
        }

        let part = diskpart(dev);
        disk_openpart(
            &sc.sc_dk,
            part,
            mode,
            sc.sc_flags.get() & VNF_HAVELABEL != 0,
        )
    })();

    // bad:
    disk_unlock(&sc.sc_dk);
    result
}

/// `vndgetdisklabel`: load the label information on the named device: the label the
/// geometry of `VNDIOCSET` gives (one partition, the raw one, covering the file), then
/// what `readdisklabel` finds or spoofs.
pub fn vndgetdisklabel(
    dev: Dev,
    sc: &VndSoftc,
    lp: &mut Disklabel,
    spoofonly: bool,
) -> Result<(), Errno> {
    *lp = Disklabel::zeroed();

    // `VNDIOCSET` checked that the geometry fits the label's 32-bit fields.
    lp.d_secsize = sc.sc_secsize.get() as u32;
    lp.d_nsectors = sc.sc_nsectors.get() as u32;
    lp.d_ntracks = sc.sc_ntracks.get() as u32;
    lp.d_secpercyl = lp.d_ntracks.wrapping_mul(lp.d_nsectors);
    if lp.d_secpercyl != 0 {
        lp.d_ncylinders = (sc.sc_size.get() / lp.d_secpercyl as usize) as u32;
    }

    strncpy(&mut lp.d_typename, b"vnd device");
    lp.d_type = sc.sc_type.get();
    strncpy(&mut lp.d_packname, b"fictitious");
    dl_setdsize(lp, sc.sc_size.get() as u64);
    lp.d_version = 1;

    lp.d_magic = DISKMAGIC;
    lp.d_magic2 = DISKMAGIC;
    lp.d_checksum = dkcksum(lp);

    // The label vndstrategy checks the reads below against (see the module's deviations).
    if sc.sc_dk.dk_openmask.get() == 0 {
        let mut incore = *lp;
        if initdisklabel(&mut incore).is_ok() {
            // SAFETY: no partition is open and the caller serialises label changes (the
            // disk lock in `vndopen`): nobody else holds the in-core label.
            if let Some(dl) = unsafe { sc.sc_dk.label_mut() } {
                *dl = incore;
            }
        }
    }

    // Call the generic disklabel extraction routine
    readdisklabel(disklabeldev(dev), vndstrategy, lp, spoofonly)
}

/// `strncpy(dst, src, sizeof(dst))`: `src`, then NULs to the end of `dst`.
fn strncpy(dst: &mut [u8], src: &[u8]) {
    let n = src.len().min(dst.len());
    dst[..n].copy_from_slice(&src[..n]);
    dst[n..].fill(0);
}

/// `vndclose`.
pub fn vndclose(dev: Dev, _flags: i32, mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let unit = diskunit(dev);
    let sc = vnd_softc(unit).ok_or(Errno::ENXIO)?;

    disk_lock_nointr(&sc.sc_dk);

    let part = diskpart(dev);

    disk_closepart(&sc.sc_dk, part, mode);

    // The C keeps, under `#if 0`, a reset of VNF_HAVELABEL once the last partition closes.

    disk_unlock(&sc.sc_dk);
    Ok(())
}

/// `vndstrategy`: reads or writes the buffer's sectors from or to the file, synchronously.
pub fn vndstrategy(bp: &'static Buf) {
    let unit = diskunit(bp.b_dev.get());

    let label = vnd_softc(unit).and_then(|sc| {
        if sc.sc_flags.get() & VNF_HAVELABEL == 0 {
            None
        } else {
            sc.sc_dk.label().map(|lp| (sc, lp))
        }
    });

    match label {
        None => {
            bp.b_error.set(Some(Errno::ENXIO));
            // bad:
            bp.set(B_ERROR);
            bp.b_resid
                .set(usize::try_from(bp.b_bcount.get()).unwrap_or(0));
        }
        Some((sc, lp)) => vnd_transfer(sc, bp, &lp),
    }

    // done:
    let s = splbio();
    biodone(bp);
    splx(s);
}

/// The transfer of `vndstrategy`, once the unit and its label are known.
fn vnd_transfer(sc: &VndSoftc, bp: &'static Buf, lp: &Disklabel) {
    // Many of the distrib scripts assume they can issue arbitrary sized requests to raw
    // vnd devices irrespective of the emulated disk geometry.
    //
    // To continue supporting this, round the block count up to a multiple of d_secsize for
    // bounds_check_with_label(), and then restore afterwards.
    //
    // We only do this for non-encrypted vnd, because encryption requires operating on
    // blocks at a time.
    let origbcount = bp.b_bcount.get();
    if sc.sc_keyctx.get().is_none() {
        let secsize = i64::from(lp.d_secsize);
        bp.b_bcount
            .set(origbcount.wrapping_add(secsize - 1) & !(secsize - 1));
        #[cfg(feature = "diagnostic")]
        if bp.b_bcount.get() != origbcount
            && let Some(p) = curproc()
        {
            let pr = p.process();
            printf(format_args!(
                "{}: sloppy {} from proc {} ({}): blkno {} bcount {}\n",
                sc.sc_dev.xname(),
                if bp.isset(B_READ) { "read" } else { "write" },
                pr.ps_pid.get(),
                core::str::from_utf8(pr.comm()).unwrap_or("?"),
                bp.b_blkno.get(),
                origbcount
            ));
        }
    }

    if !bounds_check_with_label(bp, lp) {
        bp.b_bcount.set(origbcount);
        bp.b_resid.set(usize::try_from(origbcount).unwrap_or(0));
        return;
    }

    if origbcount < bp.b_bcount.get() {
        bp.b_bcount.set(origbcount);
    }

    let p = &lp.d_partitions[diskpart(bp.b_dev.get()) as usize];
    let off = (dl_getpoffset(p) * u64::from(lp.d_secsize))
        .wrapping_add((bp.b_blkno.get() as u64).wrapping_mul(DEV_BSIZE as u64))
        as Off;

    if sc.sc_keyctx.get().is_some() && !bp.isset(B_READ) {
        vndencryptbuf(sc, bp, true);
    }

    // Use IO_NOLIMIT because upper layer has already checked I/O for limits, so there is
    // no need to do it again.
    //
    // We use IO_NOCACHE because this data should be cached at the upper layer, so there is
    // no need to cache it again.
    let rw = if bp.isset(B_READ) {
        UioRw::UIO_READ
    } else {
        UioRw::UIO_WRITE
    };
    let mut resid = bp.b_resid.get();
    let error = match sc.sc_vp.get() {
        Some(vp) => vn_rdwr(
            rw,
            vp,
            bp.b_data.get().cast::<c_void>(),
            usize::try_from(bp.b_bcount.get()).unwrap_or(0),
            off,
            UioSeg::UIO_SYSSPACE,
            IO_NOCACHE | IO_SYNC | IO_NOLIMIT,
            sc.sc_cred.get().map_or(ptr::null(), ptr::from_ref),
            Some(&mut resid),
            curproc(),
        ),
        // VNF_HAVELABEL is only set on a configured unit, which has its vnode.
        None => Err(Errno::ENXIO),
    };
    bp.b_resid.set(resid);
    bp.b_error.set(error.err());
    if error.is_err() {
        bp.set(B_ERROR);
    }

    // Data in buffer cache needs to be in clear
    if sc.sc_keyctx.get().is_some() {
        vndencryptbuf(sc, bp, false);
    }
}

/// `vndread`: the raw device's read, straight into the user's buffer (physio(9)).
pub fn vndread(dev: Dev, uio: &mut Uio<'_>, _flags: i32) -> Result<(), Errno> {
    physio(vndstrategy, dev, B_READ, minphys, uio)
}

/// `vndwrite`: the raw device's write, straight from the user's buffer (physio(9)).
pub fn vndwrite(dev: Dev, uio: &mut Uio<'_>, _flags: i32) -> Result<(), Errno> {
    physio(vndstrategy, dev, B_WRITE, minphys, uio)
}

/// `vndbdevsize`: the size in sectors of the partition a block device vnode is, from its
/// driver's `DIOCGPART`; 0 when the driver cannot tell.
pub fn vndbdevsize(vp: &'static Vnode, p: &Proc) -> usize {
    let dev = vp.v_rdev();
    let bsw = bdevsw(major(dev));
    let mut data = [0u8; size_of::<Partinfo>()];
    if (bsw.d_ioctl)(dev, DIOCGPART, &mut data, FREAD, p).is_err() {
        return 0;
    }
    let Some(pi) = Partinfo::load(&data) else {
        return 0;
    };
    if pi.part.is_null() {
        return 0;
    }
    // SAFETY: `DIOCGPART` answered with a partition of the driver's in-core label, which
    // lives while the device is attached (the vnode is open).
    let part = unsafe { &*pi.part };
    dl_getpsize(part) as usize
}

/// `vndioctl`.
pub fn vndioctl(dev: Dev, cmd: u64, addr: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let unit = diskunit(dev);

    suser(p)?;
    let sc = vnd_softc(unit).ok_or(Errno::ENXIO)?;

    match cmd {
        VNDIOCSET => vndioctl_set(dev, sc, addr, p),

        VNDIOCCLR => {
            disk_lock(&sc.sc_dk)?;
            if sc.sc_flags.get() & VNF_INITED == 0 {
                disk_unlock(&sc.sc_dk);
                return Err(Errno::ENXIO);
            }

            // Don't unconfigure if any other partitions are open or if both the character
            // and block flavors of this partition are open.
            let part = diskpart(dev);
            let pmask = 1u64 << part;
            if sc.sc_dk.dk_openmask.get() & !pmask != 0
                || (sc.sc_dk.dk_bopenmask.get() & pmask != 0
                    && sc.sc_dk.dk_copenmask.get() & pmask != 0)
            {
                disk_unlock(&sc.sc_dk);
                return Err(Errno::EBUSY);
            }

            vndclear(sc);

            // Free crypto key
            if let Some(ctx) = sc.sc_keyctx.take() {
                // SAFETY: the context `VNDIOCSET` allocated, which nothing else uses now
                // (no other partition is open); a `BlfCtx` is all `u32`s, no padding.
                let bytes = unsafe {
                    core::slice::from_raw_parts_mut(ctx.as_ptr().cast::<u8>(), size_of::<BlfCtx>())
                };
                explicit_bzero(bytes);
                free(ctx.cast(), M_DEVBUF, size_of::<BlfCtx>());
            }

            // Detach the disk.
            disk_detach(&sc.sc_dk);
            disk_unlock(&sc.sc_dk);
            Ok(())
        }

        VNDIOCGET => {
            let mut vnu = ioctl_arg::<VndUser>(addr);

            if vnu.vnu_unit == -1 {
                vnu.vnu_unit = unit as i32;
            }
            if vnu.vnu_unit >= numvnd() {
                return Err(Errno::ENXIO);
            }
            if vnu.vnu_unit < 0 {
                return Err(Errno::EINVAL);
            }

            let sc = vnd_softc(vnu.vnu_unit as u32).ok_or(Errno::ENXIO)?;

            match sc.sc_vp.get() {
                Some(vp) if sc.sc_flags.get() & VNF_INITED != 0 => {
                    let mut vattr = Vattr::new();
                    VOP_GETATTR(vp, &mut vattr, p.p_ucred.get(), p)?;

                    strlcpy(&mut vnu.vnu_file, &sc.sc_file.get());
                    vnu.vnu_dev = vattr.va_fsid as Dev;
                    vnu.vnu_ino = vattr.va_fileid;
                }
                _ => {
                    vnu.vnu_dev = 0;
                    vnu.vnu_ino = 0;
                }
            }

            ioctl_ret(addr, &vnu);
            Ok(())
        }

        DIOCRLDINFO => {
            if sc.sc_flags.get() & VNF_HAVELABEL == 0 {
                return Err(Errno::ENOTTY);
            }
            let mut lp = Disklabel::zeroed();
            let _ = vndgetdisklabel(dev, sc, &mut lp, false);
            // SAFETY: the driver's own label, no other reference to it is live.
            if let Some(dl) = unsafe { sc.sc_dk.label_mut() } {
                *dl = lp;
            }
            Ok(())
        }

        DIOCGPDINFO => {
            if sc.sc_flags.get() & VNF_HAVELABEL == 0 {
                return Err(Errno::ENOTTY);
            }
            let mut lp = Disklabel::zeroed();
            let _ = vndgetdisklabel(dev, sc, &mut lp, true);
            copyout_label(&lp, addr);
            Ok(())
        }

        DIOCGDINFO => {
            if sc.sc_flags.get() & VNF_HAVELABEL == 0 {
                return Err(Errno::ENOTTY);
            }
            if let Some(lp) = sc.sc_dk.label() {
                copyout_label(&lp, addr);
            }
            Ok(())
        }

        DIOCGPART => {
            if sc.sc_flags.get() & VNF_HAVELABEL == 0 {
                return Err(Errno::ENOTTY);
            }
            if let Some(lp) = sc.sc_dk.dk_label.get() {
                let part = diskpart(dev) as usize;
                let pi = Partinfo {
                    disklab: lp.as_ptr(),
                    // SAFETY: `lp` is the live in-core label; the projection only computes
                    // the address of one of its partitions.
                    part: unsafe { &raw mut (*lp.as_ptr()).d_partitions[part] },
                };
                pi.store(addr);
            }
            Ok(())
        }

        DIOCWDINFO | DIOCSDINFO => {
            if sc.sc_flags.get() & VNF_HAVELABEL == 0 {
                return Err(Errno::ENOTTY);
            }
            if flag & FWRITE == 0 {
                return Err(Errno::EBADF);
            }

            disk_lock(&sc.sc_dk)?;

            let mut nlp = Disklabel::from_bytes(addr);
            // SAFETY: under the disk lock; the borrow ends before the strategy runs.
            let mut result = match unsafe { sc.sc_dk.label_mut() } {
                Some(olp) => setdisklabel(olp, &mut nlp, /* sc->sc_dk.dk_openmask */ 0),
                None => Err(Errno::ENXIO),
            };
            if result.is_ok() && cmd == DIOCWDINFO {
                let mut lp = sc.sc_dk.label().unwrap_or_default();
                result = writedisklabel(disklabeldev(dev), vndstrategy, &mut lp);
            }

            disk_unlock(&sc.sc_dk);
            result
        }

        _ => Err(Errno::ENOTTY),
    }
}

/// `VNDIOCSET`: configure the unit over the file `vnd_file` names.
fn vndioctl_set(dev: Dev, sc: &'static VndSoftc, addr: &mut [u8], p: &Proc) -> Result<(), Errno> {
    let mut vio = ioctl_arg::<VndIoctl>(addr);
    let mut name = [0u8; VNDNLEN];
    let mut key = [0u8; BLF_MAXUTILIZED];

    if sc.sc_flags.get() & VNF_INITED != 0 {
        return Err(Errno::EBUSY);
    }

    // Geometry eventually has to fit into label fields
    let uint_max = UINT_MAX as usize;
    if vio.vnd_secsize > uint_max
        || vio.vnd_secsize == 0
        || vio.vnd_ntracks > uint_max
        || vio.vnd_nsectors > uint_max
    {
        return Err(Errno::EINVAL);
    }

    copyinstr(vio.vnd_file, &mut name)?;

    if vio.vnd_keylen > 0 {
        if vio.vnd_keylen as usize > key.len() {
            vio.vnd_keylen = key.len() as i32;
        }

        copyin(vio.vnd_key, &mut key[..vio.vnd_keylen as usize])?;
    }
    let keylen = usize::try_from(vio.vnd_keylen).unwrap_or(0);

    // Open for read and write first. This lets vn_open() weed out directories, sockets,
    // etc. so we don't have to worry about them.
    let mut rw = FREAD | FWRITE;
    let vp = {
        let mut nd = ndinit(0, 0, NiDirp::Sys(&name), p);
        nd.ni_unveil = UNVEIL_READ | UNVEIL_WRITE;
        let mut error = vn_open(&mut nd, FREAD | FWRITE, 0);
        if error == Err(Errno::EROFS) {
            nd = ndinit(0, 0, NiDirp::Sys(&name), p);
            nd.ni_unveil = UNVEIL_READ | UNVEIL_WRITE;
            rw = FREAD;
            error = vn_open(&mut nd, FREAD, 0);
        }
        error?;
        match nd.ni_vp {
            Some(vp) => vp,
            None => panic(format_args!("vndioctl: vn_open without a vnode")),
        }
    };
    let mut vplocked = true;
    let mut cred: Option<&'static Ucred> = None;

    let error: Errno = 'fail: {
        let mut vattr = Vattr::new();
        if let Err(e) = VOP_GETATTR(vp, &mut vattr, p.p_ucred.get(), p) {
            break 'fail e;
        }

        // Cannot put a vnd on top of a vnd
        if major(vattr.va_fsid as Dev) == major(dev) {
            break 'fail Errno::EINVAL;
        }

        match vndsetcred(p, vp, &vio) {
            Ok(c) => cred = Some(c),
            Err(e) => break 'fail e,
        }

        let _ = VOP_UNLOCK(vp);
        vplocked = false;

        let size = if vp.v_type.get() == VBLK {
            // XXX is size 0 ok?
            vndbdevsize(vp, p)
        } else {
            (vattr.va_size / vio.vnd_secsize as u64) as usize
        };

        if let Err(e) = disk_lock(&sc.sc_dk) {
            break 'fail e;
        }
        if sc.sc_flags.get() & VNF_INITED != 0 {
            disk_unlock(&sc.sc_dk);
            break 'fail Errno::EBUSY;
        }

        // Set geometry for device.
        sc.sc_type.set(vio.vnd_type);
        sc.sc_secsize.set(vio.vnd_secsize);
        sc.sc_ntracks.set(vio.vnd_ntracks);
        sc.sc_nsectors.set(vio.vnd_nsectors);
        sc.sc_size.set(size);

        if rw == FREAD {
            sc.sc_flags.set(sc.sc_flags.get() | VNF_READONLY);
        } else {
            sc.sc_flags.set(sc.sc_flags.get() & !VNF_READONLY);
        }

        sc.sc_file.set(name);

        if keylen > 0 {
            let Some(mem) = malloc(size_of::<BlfCtx>(), M_DEVBUF, M_WAITOK) else {
                panic(format_args!("vndioctl: out of memory"));
            };
            let ctx = mem.cast::<BlfCtx>();
            // SAFETY: a fresh allocation of `size_of::<BlfCtx>()` bytes, aligned (malloc's
            // chunks are aligned to their power-of-two size), ours until `VNDIOCCLR`.
            let c = unsafe {
                ctx.as_ptr().write(BlfCtx::default());
                &mut *ctx.as_ptr()
            };
            blf_key(c, &key[..keylen]);
            explicit_bzero(&mut key[..keylen]);
            sc.sc_keyctx.set(Some(ctx));
        } else {
            sc.sc_keyctx.set(None);
        }

        sc.sc_vp.set(Some(vp));
        sc.sc_cred.set(cred);
        vio.vnd_size = (sc.sc_size.get() as u64).wrapping_mul(sc.sc_secsize.get() as u64) as Off;
        sc.sc_flags.set(sc.sc_flags.get() | VNF_INITED);

        // Attach the disk.
        let mut dkname = [0u8; 16];
        let xname = sc.sc_dev.xname().as_bytes();
        let n = xname.len().min(dkname.len());
        dkname[..n].copy_from_slice(&xname[..n]);
        sc.sc_dk.dk_name.set(dkname);
        disk_attach(Some(&sc.sc_dev), &sc.sc_dk);

        disk_unlock(&sc.sc_dk);

        ioctl_ret(addr, &vio);
        return Ok(());
    };

    // fail:
    if vplocked {
        let _ = VOP_UNLOCK(vp);
    }
    let _ = vn_close(vp, rw, p.p_ucred.get(), Some(p));
    if let Some(cred) = cred {
        crfree(cred);
    }
    Err(error)
}

/// `*(struct disklabel *)addr = *lp`: the label into an `ioctl` buffer.
fn copyout_label(lp: &Disklabel, addr: &mut [u8]) {
    let n = addr.len().min(DISKLABEL_SIZE);
    addr[..n].copy_from_slice(&lp.as_bytes()[..n]);
}

/// `vndsetcred`: duplicate the current processes' credentials. Since we are called only as
/// the result of a SET ioctl and only root can do that, any future access to this "disk"
/// is essentially as root. Note that credentials may change if some other uid can write
/// directly to the mapped file (NFS).
pub fn vndsetcred(p: &Proc, vp: &'static Vnode, _vio: &VndIoctl) -> Result<&'static Ucred, Errno> {
    let new = crdup(p.ucred());
    let Some(buf) = malloc(DEV_BSIZE, M_TEMP, M_WAITOK) else {
        panic(format_args!("vndsetcred: out of memory"));
    };

    // XXX: Horrible kludge to establish credentials for NFS
    let error = vn_rdwr(
        UioRw::UIO_READ,
        vp,
        buf.as_ptr().cast::<c_void>(),
        DEV_BSIZE,
        0,
        UioSeg::UIO_SYSSPACE,
        0,
        ptr::from_ref(new),
        None,
        curproc(),
    );

    free(buf, M_TEMP, DEV_BSIZE);
    match error {
        Ok(()) => Ok(new),
        Err(e) => {
            crfree(new);
            Err(e)
        }
    }
}

/// `vndclear`: closes the file and forgets the configuration.
pub fn vndclear(sc: &VndSoftc) {
    let Some(vp) = sc.sc_vp.get() else {
        panic(format_args!("vndioctl: null vp"));
    };
    let p = curproc(); // XXX

    let cred = sc.sc_cred.get();
    let _ = vn_close(vp, vndrw(sc), cred.map_or(ptr::null(), ptr::from_ref), p);
    if let Some(cred) = cred {
        crfree(cred);
    }
    sc.sc_flags.set(0);
    sc.sc_vp.set(None);
    sc.sc_cred.set(None);
    sc.sc_size.set(0);
    sc.sc_file.set([0; VNDNLEN]);
}

/// `vndsize`: we don't support swapping to vnd anymore.
pub fn vndsize(_dev: Dev) -> Daddr {
    -1
}

/// `vnddump`: not implemented.
pub fn vnddump(_dev: Dev, _blkno: Daddr, _va: *mut u8, _size: usize) -> Result<(), Errno> {
    Err(Errno::ENXIO)
}

#[cfg(test)]
mod tests;
