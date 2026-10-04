/*	$OpenBSD: bio.c,v 1.20 2026/08/11 16:25:29 deraadt Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2002 Niklas Hallqvist.  All rights reserved.
 * Copyright (c) 2012 Joel Sing <jsing@openbsd.org>.  All rights reserved.
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

//! bio(4): a device controller ioctl tunnelling device.
//!
//! Upstream: sys/dev/bio.c @ 3ce1f3f79392
//!
//! RAID controllers and volume managers (softraid(4), the scsibus) register themselves with
//! [`bio_register`] and get their `BIOC*` ioctls through `/dev/bio`. A program first asks for
//! the cookie of a controller by name (`BIOCLOCATE`: `bioctl(8)` passes `"softraid0"`), then
//! puts that cookie in the `struct bio` that heads every other bio ioctl; [`bioioctl`] finds
//! the registered device by its cookie and hands the ioctl to the function it registered. The
//! cookies are random, so a program cannot guess another device's.
//!
//! `pseudo-device bio 1` is configured ([`NBIO`]); the `bioattach` pseudo-device attach
//! function is in `pdevinit[]` and `bioopen`/`bioclose`/`bioioctl` are `cdevsw[79]`
//! (`cdev_bio_init`).
//!
//! ## Deviations
//! - `RBT_HEAD(bio_mappings, bio_mapping)` is an [`RbtHead`] of [`BioMapping`] through
//!   [`tree_adapter!`]; `bios` is a `Sync` wrapper ([`BioMappings`]) over it. Like the C it has
//!   no lock of its own: the tree is changed by autoconfiguration and by ioctls, which run
//!   under the kernel lock.
//! - A lookup key (`bio_validate`) has no device and no ioctl function, so `bm_dev` and
//!   `bm_ioctl` are `Option`s; a registered mapping always has both.
//! - `bm_ioctl` is a [`BioIoctl`] (`fn(&Device, u64, &mut [u8]) -> Result<(), Errno>`, like
//!   `scsibusbioctl`) and the device is `&'static Device`: a registered controller stays
//!   attached until it calls [`bio_unregister`].
//! - `bio_register` returns `Result<(), Errno>` (`ENOMEM` for a failed `M_NOWAIT` allocation)
//!   and `bio_validate` takes the cookie as a `usize`.
//! - The ioctl argument is a byte slice, the kernel copy `sys_ioctl` made. `struct bio_locate`
//!   and `struct bio` are not [`AbiPod`](crate::machine::copy::AbiPod) (they have padding), so
//!   [`bioioctl`] reads the cookie and the name's user address, and writes the cookie, at the
//!   `offset_of!` positions of those fields instead of copying a whole structure. An argument
//!   too short for them is `EINVAL` (`sys_ioctl` sizes it from the command, so it never is).
//! - `bio_info`, `bio_warn`, `bio_error` and `bio_status` take `core::fmt::Arguments` where the
//!   C takes a format and a `va_list`; `print` is a `bool`.
//! - `bio_status_init` zeroes the structure by assigning a new one.

use core::cell::Cell;
use core::cmp::Ordering;
use core::fmt::Arguments;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};

use libkern::strlcpy;

use crate::dev::biovar::{
    BIO_MSG_COUNT, BIO_MSG_ERROR, BIO_MSG_INFO, BIO_MSG_LEN, BIO_MSG_WARN, BIO_STATUS_UNKNOWN,
    BIOCALARM, BIOCBLINK, BIOCCREATERAID, BIOCDELETERAID, BIOCDISCIPLINE, BIOCDISK, BIOCINQ,
    BIOCINSTALLBOOT, BIOCLOCATE, BIOCPATROL, BIOCSETSTATE, BIOCVOL, Bio, BioLocate, BioMsg,
    BioStatus,
};
use crate::dev::rnd::arc4random;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::{Str, vsnprintf};
use crate::kprintf;
use crate::machine::copy::copyinstr;
use crate::sys::device::Device;
use crate::sys::errno::Errno::{self, EINVAL, ENOENT, ENOMEM, ENOTTY};
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT};
use crate::sys::proc::Proc;
use crate::sys::tree::{RbtEntry, RbtHead};
use crate::sys::types::Dev;
use crate::tree_adapter;

/// `NBIO`: the count `config(8)` writes into `bio.h` for `pseudo-device bio 1`.
pub const NBIO: i32 = 1;

/// The ioctl function a controller registers: its device, the command and the kernel copy of
/// the argument (`int (*bm_ioctl)(struct device *, u_long, caddr_t)`).
pub type BioIoctl = fn(&Device, u64, &mut [u8]) -> Result<(), Errno>;

/// `struct bio_mapping`: a registered controller.
pub struct BioMapping {
    /// `bm_link`: the links of `bios`.
    bm_link: RbtEntry,
    /// `bm_cookie`: the random number `BIOCLOCATE` gives out for this controller.
    bm_cookie: Cell<usize>,
    /// `bm_dev`: the controller; `None` only in the key of `bio_validate`.
    bm_dev: Option<&'static Device>,
    /// `bm_ioctl`: its ioctl function; `None` only in the key of `bio_validate`.
    bm_ioctl: Option<BioIoctl>,
}

/// `bio_cookie_cmp`: the order of `bios`, by cookie, descending (as the C has it).
fn bio_cookie_cmp(a: &BioMapping, b: &BioMapping) -> Ordering {
    b.bm_cookie.get().cmp(&a.bm_cookie.get())
}

tree_adapter!(
    /// `RBT_HEAD(bio_mappings, bio_mapping)`: the registered controllers, through `bm_link`,
    /// ordered by `bio_cookie_cmp`.
    pub BioMappingsRbt: BioMapping, bm_link => RbtEntry, bio_cookie_cmp
);

/// `bios`'s type: the tree, made `Sync` (see the module's Deviations).
pub struct BioMappings(RbtHead<BioMappingsRbt>);

// SAFETY: as in C, the tree is touched only under the kernel lock (autoconfiguration and
// ioctls), never concurrently.
unsafe impl Sync for BioMappings {}

/// `bios`: the registered controllers.
pub static BIOS: BioMappings = BioMappings(RbtHead::new());

/// `bioattach`: the pseudo-device's attach function; nothing to do.
pub fn bioattach(_nunits: i32) {}

/// `bioopen`.
pub fn bioopen(_dev: Dev, _flags: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

/// `bioclose`.
pub fn bioclose(_dev: Dev, _flags: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    Ok(())
}

/// The `usize` at byte `off` of the ioctl argument.
fn arg_usize(addr: &[u8], off: usize) -> Result<usize, Errno> {
    let bytes = addr
        .get(off..off + size_of::<usize>())
        .ok_or(EINVAL)?
        .try_into()
        .map_err(|_| EINVAL)?;
    Ok(usize::from_ne_bytes(bytes))
}

/// `bioioctl`: `BIOCLOCATE` names a controller's cookie; every other bio ioctl goes to the
/// controller whose cookie its `struct bio` carries.
pub fn bioioctl(_dev: Dev, cmd: u64, addr: &mut [u8], _flag: i32, _p: &Proc) -> Result<(), Errno> {
    match cmd {
        BIOCLOCATE => {
            let uname = arg_usize(addr, offset_of!(BioLocate, bl_name))?;
            let mut name = [0u8; 16];
            copyinstr(uname, &mut name)?;
            let bm = bio_lookup(&name).ok_or(ENOENT)?;

            let off = offset_of!(BioLocate, bl_bio) + offset_of!(Bio, bio_cookie);
            addr.get_mut(off..off + size_of::<usize>())
                .ok_or(EINVAL)?
                .copy_from_slice(&bm.bm_cookie.get().to_ne_bytes());
            Ok(())
        }
        BIOCINQ | BIOCDISK | BIOCVOL | BIOCALARM | BIOCBLINK | BIOCSETSTATE | BIOCCREATERAID
        | BIOCDELETERAID | BIOCDISCIPLINE | BIOCINSTALLBOOT | BIOCPATROL => {
            let cookie = arg_usize(addr, offset_of!(Bio, bio_cookie))?;
            let bm = bio_validate(cookie).ok_or(ENOENT)?;
            bio_delegate_ioctl(bm, cmd, addr)
        }
        _ => Err(ENOTTY),
    }
}

/// `bio_register`: makes `dev` reachable through bio(4) under a new random cookie; its
/// ioctls go to `ioctl`.
pub fn bio_register(dev: &'static Device, ioctl: BioIoctl) -> Result<(), Errno> {
    let Some(mem) = malloc(size_of::<BioMapping>(), M_DEVBUF, M_NOWAIT) else {
        return Err(ENOMEM);
    };
    let bm = mem.cast::<BioMapping>();
    // SAFETY: a fresh allocation of `size_of::<BioMapping>()` bytes, aligned by `malloc` (at
    // least 16 bytes); it lives until `bio_unregister` frees it.
    let bm: &'static BioMapping = unsafe {
        bm.as_ptr().write(BioMapping {
            bm_link: RbtEntry::new(),
            bm_cookie: Cell::new(0),
            bm_dev: Some(dev),
            bm_ioctl: Some(ioctl),
        });
        &*bm.as_ptr()
    };
    loop {
        bm.bm_cookie.set(arc4random() as usize);
        // Lets hope we don't have 4 billion bio_registers.
        //
        // SAFETY: `bm` is in no tree; it stays allocated and in place until `bio_unregister`
        // removes it and frees it.
        if unsafe { BIOS.0.insert(bm) }.is_none() {
            return Ok(());
        }
    }
}

/// `bio_unregister`: forgets the mappings of `dev`.
pub fn bio_unregister(dev: &Device) {
    for bm in BIOS.0.iter() {
        if bm.bm_dev.is_some_and(|d| ptr::eq(d, dev)) {
            // SAFETY: `bm` is in `bios`; the iterator has already read its successor.
            unsafe { BIOS.0.remove(bm) };
            free(
                NonNull::from(bm).cast::<u8>(),
                M_DEVBUF,
                size_of::<BioMapping>(),
            );
        }
    }
}

/// `bio_lookup`: the mapping of the controller called `name` (a NUL-terminated name).
pub fn bio_lookup(name: &[u8]) -> Option<&'static BioMapping> {
    let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];

    BIOS.0
        .iter()
        .find(|bm| bm.bm_dev.is_some_and(|d| d.xname().as_bytes() == name))
}

/// `bio_validate`: the mapping with this cookie.
pub fn bio_validate(cookie: usize) -> Option<&'static BioMapping> {
    let key = BioMapping {
        bm_link: RbtEntry::new(),
        bm_cookie: Cell::new(cookie),
        bm_dev: None,
        bm_ioctl: None,
    };

    BIOS.0.find(&key)
}

/// `bio_delegate_ioctl`: hands the ioctl to the controller's function.
pub fn bio_delegate_ioctl(bm: &BioMapping, cmd: u64, addr: &mut [u8]) -> Result<(), Errno> {
    match (bm.bm_dev, bm.bm_ioctl) {
        (Some(dev), Some(ioctl)) => ioctl(dev, cmd, addr),
        _ => Err(ENOENT),
    }
}

/// `bio_info`: adds an informational message to `bs`.
pub fn bio_info(bs: &mut BioStatus, print: bool, args: Arguments<'_>) {
    bio_status(bs, print, BIO_MSG_INFO, args);
}

/// `bio_warn`: adds a warning to `bs`.
pub fn bio_warn(bs: &mut BioStatus, print: bool, args: Arguments<'_>) {
    bio_status(bs, print, BIO_MSG_WARN, args);
}

/// `bio_error`: adds an error message to `bs`.
pub fn bio_error(bs: &mut BioStatus, print: bool, args: Arguments<'_>) {
    bio_status(bs, print, BIO_MSG_ERROR, args);
}

/// `bio_status_init`: an empty status of the controller `dv`.
pub fn bio_status_init(bs: &mut BioStatus, dv: &Device) {
    *bs = BioStatus {
        bs_controller: [0; 16],
        bs_status: BIO_STATUS_UNKNOWN,
        bs_msg_count: 0,
        bs_msgs: [BioMsg {
            bm_type: 0,
            bm_msg: [0; BIO_MSG_LEN],
        }; BIO_MSG_COUNT],
    };

    strlcpy(&mut bs.bs_controller, dv.xname().as_bytes());
}

/// `bio_status`: adds a message of `msg_type` to `bs` (and prints it when `print`); a full
/// `bs` only says so.
pub fn bio_status(bs: &mut BioStatus, print: bool, msg_type: i32, args: Arguments<'_>) {
    if bs.bs_msg_count >= BIO_MSG_COUNT as i32 {
        kprintf!("{}: insufficient message buffers\n", Str(&bs.bs_controller));
        return;
    }

    let idx = bs.bs_msg_count as usize;
    bs.bs_msg_count += 1;

    bs.bs_msgs[idx].bm_type = msg_type;
    vsnprintf(&mut bs.bs_msgs[idx].bm_msg, args);

    if print {
        kprintf!(
            "{}: {}\n",
            Str(&bs.bs_controller),
            Str(&bs.bs_msgs[idx].bm_msg)
        );
    }
}

#[cfg(test)]
mod tests;
