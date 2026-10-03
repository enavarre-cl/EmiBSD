/*	$OpenBSD: conf.h,v 1.168 2025/09/08 17:25:46 helg Exp $	*/
/*	$NetBSD: conf.h,v 1.33 1996/05/03 20:03:32 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1990, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)conf.h	8.3 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! `<sys/conf.h>`: the device driver entry switches (`struct bdevsw`, `struct cdevsw`), the
//! `cdev_*_init`/`bdev_*_init` initialisers each architecture's `conf.c` builds its tables
//! with, and the line discipline switch (`struct linesw`).
//!
//! Upstream: sys/sys/conf.h @ 3ce1f3f79392
//!
//! The tables themselves (`bdevsw[]`, `cdevsw[]`, `chrtoblktbl[]`, `mem_no`) are each
//! architecture's `conf.c`, reached through `machine::conf` (`docs/ARCHITECTURE.md`, "The
//! device switch").
//!
//! ## Deviations
//! - The entry points are typed `fn` pointers ([`DevTypeOpen`], ...) returning `Result`
//!   where the C returns an `int` errno. `d_close` takes `Option<&Proc>` (`spec_close` may
//!   have no thread); `d_ioctl`'s `caddr_t data` is the kernel copy of the argument as a
//!   byte slice, as `fo_ioctl`'s; `d_strategy` takes the `&'static Buf` the buffer cache
//!   hands out (`sys/buf.rs`); `d_kqfilter`'s `struct knote *` is a raw pointer until
//!   `kern_event.c` is ported.
//! - `d_mmap` returns `Option<Paddr>`: `None` where the C returns `-1`. The C fills the
//!   slot of a driver without one with `enodev` cast to `paddr_t (*)()`, which "returns" the
//!   address 19; here such a slot answers `None`.
//! - Members the C may leave NULL are `Option`s (`d_tty`, `d_kqfilter`, `d_psize`).
//! - The `cdev_*_init(c, n)` macros paste the driver's name onto each entry point
//!   (`dev_init(c,n,open)` is `nopen`); Rust has no token pasting, so the constructors are
//!   `const fn`s taking the driver's count and its functions in the C's order, and keep the
//!   macros' choices: an entry `dev_init` names becomes `enxio` when the count is 0, the
//!   others are `enodev`, `nullop`, `seltrue_kqfilter`, `ttkqfilter` or NULL as the macro
//!   writes them. The casts of `enodev`/`enxio`/`nullop` to each pointer type are small
//!   functions of the right type here.
//! - `seltrue_kqfilter` belongs to `kern_event.c`, which is not ported: the stand-in here
//!   reports itself (no `kevent(2)` can reach it yet).
//! - `l_ioctl` returns `Ok(false)` for the C's `-1` ("not mine, try `ttioctl`"), `Ok(true)`
//!   for 0 (`docs/C_TO_RUST.md`); `-1` is `ERESTART` in `Errno`.
//! - The `cdev_decl`/`bdev_decl` prototypes and the `ptstty`/`ptctty`/`ptsioctl`/`ptcioctl`
//!   aliases are the drivers' own functions in their modules.

use core::ffi::c_void;

use crate::kern::subr_xxx::{enodev, enxio, nullop};
use crate::kern::tty::ttkqfilter;
use crate::sys::buf::Buf;
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::tty::Tty;
use crate::sys::types::{Daddr, Dev, Off, Paddr};
use crate::sys::uio::Uio;
use crate::unported;

// Types for d_type

/// A disk.
pub const D_DISK: u32 = 1;
/// A terminal.
pub const D_TTY: u32 = 2;

// Flags for d_flags

/// Clone upon open.
pub const D_CLONE: u32 = 0x0001;

/// `dev_type_open(n)`: `int n(dev_t, int, int, struct proc *)`.
pub type DevTypeOpen = fn(Dev, i32, i32, &Proc) -> Result<(), Errno>;
/// `dev_type_close(n)`: `int n(dev_t, int, int, struct proc *)`, the thread possibly NULL.
pub type DevTypeClose = fn(Dev, i32, i32, Option<&Proc>) -> Result<(), Errno>;
/// `dev_type_strategy(n)`: `void n(struct buf *)`.
pub type DevTypeStrategy = fn(&'static Buf);
/// `dev_type_ioctl(n)`: `int n(dev_t, u_long, caddr_t, int, struct proc *)`.
pub type DevTypeIoctl = fn(Dev, u64, &mut [u8], i32, &Proc) -> Result<(), Errno>;
/// `dev_type_dump(n)`: `int n(dev_t, daddr_t, caddr_t, size_t)`.
pub type DevTypeDump = fn(Dev, Daddr, *mut u8, usize) -> Result<(), Errno>;
/// `dev_type_size(n)`: `daddr_t n(dev_t)`.
pub type DevTypeSize = fn(Dev) -> Daddr;
/// `dev_type_read(n)`: `int n(dev_t, struct uio *, int)`.
pub type DevTypeRead = fn(Dev, &mut Uio<'_>, i32) -> Result<(), Errno>;
/// `dev_type_write(n)`: `int n(dev_t, struct uio *, int)`.
pub type DevTypeWrite = fn(Dev, &mut Uio<'_>, i32) -> Result<(), Errno>;
/// `dev_type_stop(n)`: `int n(struct tty *, int)`.
pub type DevTypeStop = fn(&Tty, i32) -> Result<(), Errno>;
/// `dev_type_tty(n)`: `struct tty *n(dev_t)`.
pub type DevTypeTty = fn(Dev) -> Option<&'static Tty>;
/// `dev_type_mmap(n)`: `paddr_t n(dev_t, off_t, int)`, `None` for the C's `-1`.
pub type DevTypeMmap = fn(Dev, Off, i32) -> Option<Paddr>;
/// `dev_type_kqfilter(n)`: `int n(dev_t, struct knote *)`.
pub type DevTypeKqfilter = fn(Dev, *mut c_void) -> Result<(), Errno>;

/// `struct bdevsw`: block device switch table.
#[derive(Clone, Copy)]
pub struct Bdevsw {
    /// `d_open`.
    pub d_open: DevTypeOpen,
    /// `d_close`.
    pub d_close: DevTypeClose,
    /// `d_strategy`.
    pub d_strategy: DevTypeStrategy,
    /// `d_ioctl`.
    pub d_ioctl: DevTypeIoctl,
    /// `d_dump`.
    pub d_dump: DevTypeDump,
    /// `d_psize`, NULL for a device without one.
    pub d_psize: Option<DevTypeSize>,
    /// `d_type`.
    pub d_type: u32,
    // u_int d_flags; (commented out in C)
}

/// `struct cdevsw`: character device switch table.
#[derive(Clone, Copy)]
pub struct Cdevsw {
    /// `d_open`.
    pub d_open: DevTypeOpen,
    /// `d_close`.
    pub d_close: DevTypeClose,
    /// `d_read`.
    pub d_read: DevTypeRead,
    /// `d_write`.
    pub d_write: DevTypeWrite,
    /// `d_ioctl`.
    pub d_ioctl: DevTypeIoctl,
    /// `d_stop`.
    pub d_stop: DevTypeStop,
    /// `d_tty`, NULL for a device that is not a tty.
    pub d_tty: Option<DevTypeTty>,
    /// `d_mmap`.
    pub d_mmap: DevTypeMmap,
    /// `d_type`.
    pub d_type: u32,
    /// `d_flags`.
    pub d_flags: u32,
    /// `d_kqfilter`, NULL for a device without one.
    pub d_kqfilter: Option<DevTypeKqfilter>,
}

/// The type of `l_ioctl`: `Ok(false)` where the C returns `-1` (the command is not the line
/// discipline's).
pub type LineIoctl = fn(&Tty, u64, &mut [u8], i32, &Proc) -> Result<bool, Errno>;

/// `struct linesw`: line discipline switch table.
#[derive(Clone, Copy)]
pub struct Linesw {
    /// `l_open`.
    pub l_open: fn(Dev, &Tty, &Proc) -> Result<(), Errno>,
    /// `l_close`.
    pub l_close: fn(&Tty, i32, Option<&Proc>) -> Result<(), Errno>,
    /// `l_read`.
    pub l_read: fn(&Tty, &mut Uio<'_>, i32) -> Result<(), Errno>,
    /// `l_write`.
    pub l_write: fn(&Tty, &mut Uio<'_>, i32) -> Result<(), Errno>,
    /// `l_ioctl`: `Ok(false)` where the C returns `-1` (not handled).
    pub l_ioctl: LineIoctl,
    /// `l_rint`.
    pub l_rint: fn(i32, &Tty) -> i32,
    /// `l_start`.
    pub l_start: fn(&Tty) -> i32,
    /// `l_modem`.
    pub l_modem: fn(&Tty, i32) -> i32,
}

// The C's casts of enodev, enxio and nullop to each entry point type.

fn enodev_open(_: Dev, _: i32, _: i32, _: &Proc) -> Result<(), Errno> {
    enodev()
}
fn enxio_open(_: Dev, _: i32, _: i32, _: &Proc) -> Result<(), Errno> {
    enxio()
}
fn enodev_close(_: Dev, _: i32, _: i32, _: Option<&Proc>) -> Result<(), Errno> {
    enodev()
}
fn enxio_close(_: Dev, _: i32, _: i32, _: Option<&Proc>) -> Result<(), Errno> {
    enxio()
}
fn nullop_close(_: Dev, _: i32, _: i32, _: Option<&Proc>) -> Result<(), Errno> {
    nullop()
}
/// The C casts `enodev` to `void (*)(struct buf *)`: the call returns and the buffer is
/// left as it was.
fn enodev_strategy(_: &'static Buf) {
    let _ = enodev();
}
fn enxio_strategy(_: &'static Buf) {
    let _ = enxio();
}
fn enodev_ioctl(_: Dev, _: u64, _: &mut [u8], _: i32, _: &Proc) -> Result<(), Errno> {
    enodev()
}
fn enxio_ioctl(_: Dev, _: u64, _: &mut [u8], _: i32, _: &Proc) -> Result<(), Errno> {
    enxio()
}
fn enodev_dump(_: Dev, _: Daddr, _: *mut u8, _: usize) -> Result<(), Errno> {
    enodev()
}
fn enxio_dump(_: Dev, _: Daddr, _: *mut u8, _: usize) -> Result<(), Errno> {
    enxio()
}
fn enodev_rw(_: Dev, _: &mut Uio<'_>, _: i32) -> Result<(), Errno> {
    enodev()
}
fn enxio_rw(_: Dev, _: &mut Uio<'_>, _: i32) -> Result<(), Errno> {
    enxio()
}
fn enodev_stop(_: &Tty, _: i32) -> Result<(), Errno> {
    enodev()
}
fn enxio_stop(_: &Tty, _: i32) -> Result<(), Errno> {
    enxio()
}
fn nullop_stop(_: &Tty, _: i32) -> Result<(), Errno> {
    nullop()
}
fn enxio_tty(_: Dev) -> Option<&'static Tty> {
    None
}
fn enodev_mmap(_: Dev, _: Off, _: i32) -> Option<Paddr> {
    None
}
fn enxio_mmap(_: Dev, _: Off, _: i32) -> Option<Paddr> {
    None
}
fn enxio_kqfilter(_: Dev, _: *mut c_void) -> Result<(), Errno> {
    enxio()
}
/// `seltrue_kqfilter(dev, kn)` (`kern_event.c`, not ported): the filter of a device that is
/// always ready.
pub fn seltrue_kqfilter(_dev: Dev, _kn: *mut c_void) -> Result<(), Errno> {
    Err(unported!("seltrue_kqfilter (kern_event.c)"))
}

/// `dev_init(c, n, open)`.
const fn di_open(c: i32, f: DevTypeOpen) -> DevTypeOpen {
    if c > 0 { f } else { enxio_open }
}
/// `dev_init(c, n, close)`.
const fn di_close(c: i32, f: DevTypeClose) -> DevTypeClose {
    if c > 0 { f } else { enxio_close }
}
/// `dev_init(c, n, strategy)`.
const fn di_strategy(c: i32, f: DevTypeStrategy) -> DevTypeStrategy {
    if c > 0 { f } else { enxio_strategy }
}
/// `dev_init(c, n, ioctl)`.
const fn di_ioctl(c: i32, f: DevTypeIoctl) -> DevTypeIoctl {
    if c > 0 { f } else { enxio_ioctl }
}
/// `dev_init(c, n, dump)`.
const fn di_dump(c: i32, f: DevTypeDump) -> DevTypeDump {
    if c > 0 { f } else { enxio_dump }
}
/// `dev_init(c, n, read)` and `dev_init(c, n, write)`.
const fn di_rw(c: i32, f: DevTypeRead) -> DevTypeRead {
    if c > 0 { f } else { enxio_rw }
}
/// `dev_init(c, n, stop)`.
const fn di_stop(c: i32, f: DevTypeStop) -> DevTypeStop {
    if c > 0 { f } else { enxio_stop }
}
/// `dev_init(c, n, tty)`.
const fn di_tty(c: i32, f: DevTypeTty) -> DevTypeTty {
    if c > 0 { f } else { enxio_tty }
}
/// `dev_init(c, n, mmap)`.
const fn di_mmap(c: i32, f: DevTypeMmap) -> DevTypeMmap {
    if c > 0 { f } else { enxio_mmap }
}
/// `dev_init(c, n, kqfilter)`.
const fn di_kqfilter(c: i32, f: DevTypeKqfilter) -> DevTypeKqfilter {
    if c > 0 { f } else { enxio_kqfilter }
}

/// A character switch entry from its members, in the structure's order.
#[allow(clippy::too_many_arguments)] // the C structure's eleven members
const fn cdev(
    d_open: DevTypeOpen,
    d_close: DevTypeClose,
    d_read: DevTypeRead,
    d_write: DevTypeWrite,
    d_ioctl: DevTypeIoctl,
    d_stop: DevTypeStop,
    d_tty: Option<DevTypeTty>,
    d_mmap: DevTypeMmap,
    d_type: u32,
    d_flags: u32,
    d_kqfilter: Option<DevTypeKqfilter>,
) -> Cdevsw {
    Cdevsw {
        d_open,
        d_close,
        d_read,
        d_write,
        d_ioctl,
        d_stop,
        d_tty,
        d_mmap,
        d_type,
        d_flags,
        d_kqfilter,
    }
}

/// `bdev_disk_init(c, n)`.
pub const fn bdev_disk_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    strategy: DevTypeStrategy,
    ioctl: DevTypeIoctl,
    dump: DevTypeDump,
    size: DevTypeSize,
) -> Bdevsw {
    Bdevsw {
        d_open: di_open(c, open),
        d_close: di_close(c, close),
        d_strategy: di_strategy(c, strategy),
        d_ioctl: di_ioctl(c, ioctl),
        d_dump: di_dump(c, dump),
        // dev_size_init(c, n): (c > 0 ? nsize : 0)
        d_psize: if c > 0 { Some(size) } else { None },
        d_type: D_DISK,
    }
}

/// `bdev_swap_init(c, n)`.
pub const fn bdev_swap_init(c: i32, strategy: DevTypeStrategy) -> Bdevsw {
    Bdevsw {
        d_open: enodev_open,
        d_close: enodev_close,
        d_strategy: di_strategy(c, strategy),
        d_ioctl: enodev_ioctl,
        d_dump: enodev_dump,
        d_psize: None,
        d_type: 0,
    }
}

/// `bdev_notdef()`.
pub const fn bdev_notdef() -> Bdevsw {
    Bdevsw {
        d_open: enodev_open,
        d_close: enodev_close,
        d_strategy: enodev_strategy,
        d_ioctl: enodev_ioctl,
        d_dump: enodev_dump,
        d_psize: None,
        d_type: 0,
    }
}

/// `cdev_disk_init(c, n)`: open, close, read, write, ioctl.
pub const fn cdev_disk_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        di_rw(c, read),
        di_rw(c, write),
        di_ioctl(c, ioctl),
        enodev_stop,
        None,
        enodev_mmap,
        D_DISK,
        0,
        Some(seltrue_kqfilter),
    )
}

/// `cdev_tape_init(c, n)`: open, close, read, write, ioctl.
pub const fn cdev_tape_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        di_rw(c, read),
        di_rw(c, write),
        di_ioctl(c, ioctl),
        enodev_stop,
        None,
        enodev_mmap,
        0,
        0,
        Some(seltrue_kqfilter),
    )
}

/// `cdev_tty_init(c, n)`: open, close, read, write, ioctl, stop, tty.
#[allow(clippy::too_many_arguments)] // the macro's seven entry points and the count
pub const fn cdev_tty_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    stop: DevTypeStop,
    tty: DevTypeTty,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        di_rw(c, read),
        di_rw(c, write),
        di_ioctl(c, ioctl),
        di_stop(c, stop),
        Some(di_tty(c, tty)),
        enodev_mmap,
        D_TTY,
        0,
        Some(ttkqfilter),
    )
}

/// `cdev_mouse_init(c, n)`: open, close, read, ioctl, kqfilter.
pub const fn cdev_mouse_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        di_rw(c, read),
        enodev_rw,
        di_ioctl(c, ioctl),
        enodev_stop,
        None,
        enodev_mmap,
        0,
        0,
        Some(di_kqfilter(c, kqfilter)),
    )
}

/// `cdev_notdef()`: an empty slot.
pub const fn cdev_notdef() -> Cdevsw {
    cdev(
        enodev_open,
        enodev_close,
        enodev_rw,
        enodev_rw,
        enodev_ioctl,
        enodev_stop,
        None,
        enodev_mmap,
        0,
        0,
        Some(seltrue_kqfilter),
    )
}

/// `cdev_cn_init(c, n)`: open, close, read, write, ioctl, kqfilter -- XXX should be a tty.
#[allow(clippy::too_many_arguments)] // the macro's seven entry points and the count
pub const fn cdev_cn_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    stop: DevTypeStop,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        di_rw(c, read),
        di_rw(c, write),
        di_ioctl(c, ioctl),
        di_stop(c, stop),
        None,
        enodev_mmap,
        D_TTY,
        0,
        Some(di_kqfilter(c, kqfilter)),
    )
}

/// `cdev_ctty_init(c, n)`: open, read, write, ioctl, kqfilter -- XXX should be a tty.
pub const fn cdev_ctty_init(
    c: i32,
    open: DevTypeOpen,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        nullop_close,
        di_rw(c, read),
        di_rw(c, write),
        di_ioctl(c, ioctl),
        nullop_stop,
        None,
        enodev_mmap,
        D_TTY,
        0,
        Some(di_kqfilter(c, kqfilter)),
    )
}

/// `cdev_mm_init(c, n)`: open, close, read, write, ioctl, mmap.
pub const fn cdev_mm_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    mmap: DevTypeMmap,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        di_rw(c, read),
        di_rw(c, write),
        di_ioctl(c, ioctl),
        enodev_stop,
        None,
        di_mmap(c, mmap),
        0,
        0,
        Some(seltrue_kqfilter),
    )
}

/// `cdev_ptc_init(c, n)`: open, close, read, write, ioctl, tty, kqfilter.
#[allow(clippy::too_many_arguments)] // the macro's seven entry points and the count
pub const fn cdev_ptc_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    tty: DevTypeTty,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        di_rw(c, read),
        di_rw(c, write),
        di_ioctl(c, ioctl),
        nullop_stop,
        Some(di_tty(c, tty)),
        enodev_mmap,
        D_TTY,
        0,
        Some(di_kqfilter(c, kqfilter)),
    )
}

/// `cdev_ptm_init(c, n)`: open, close, ioctl.
pub const fn cdev_ptm_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        enodev_rw,
        enodev_rw,
        di_ioctl(c, ioctl),
        enodev_stop,
        None,
        enodev_mmap,
        0,
        0,
        None,
    )
}

/// `cdev_log_init(c, n)`: open, close, read, ioctl, kqfilter.
pub const fn cdev_log_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev_mouse_init(c, open, close, read, ioctl, kqfilter)
}

/// `cdev_fd_init(c, n)`: open.
pub const fn cdev_fd_init(c: i32, open: DevTypeOpen) -> Cdevsw {
    cdev(
        di_open(c, open),
        enodev_close,
        enodev_rw,
        enodev_rw,
        enodev_ioctl,
        enodev_stop,
        None,
        enodev_mmap,
        0,
        0,
        None,
    )
}

/// The shape of `cdev_tun_init`, `cdev_pppx_init`, `cdev_audio_init`, `cdev_midi_init`,
/// `cdev_random_init`, `cdev_usbdev_init`, `cdev_fido_init`, `cdev_ujoy_init` and (with
/// `D_CLONE`) `cdev_bpf_init`: open, close, read, write, ioctl, kqfilter.
#[allow(clippy::too_many_arguments)] // the macro's six entry points, the count and the flags
const fn cdev_rwk(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
    flags: u32,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        di_rw(c, read),
        di_rw(c, write),
        di_ioctl(c, ioctl),
        enodev_stop,
        None,
        enodev_mmap,
        0,
        flags,
        Some(di_kqfilter(c, kqfilter)),
    )
}

/// `cdev_tun_init(c, n)`: open, close, read, write, ioctl, kqfilter.
pub const fn cdev_tun_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev_rwk(c, open, close, read, write, ioctl, kqfilter, 0)
}

/// `cdev_vscsi_init(c, n)`: open, close, ioctl, kqfilter.
pub const fn cdev_vscsi_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        enodev_rw,
        enodev_rw,
        di_ioctl(c, ioctl),
        enodev_stop,
        None,
        enodev_mmap,
        0,
        0,
        Some(di_kqfilter(c, kqfilter)),
    )
}

/// `cdev_pppx_init(c, n)`: open, close, read, write, ioctl, kqfilter.
pub const fn cdev_pppx_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev_rwk(c, open, close, read, write, ioctl, kqfilter, 0)
}

/// `cdev_bpf_init(c, n)`: open, close, read, write, ioctl, kqfilter, cloning.
pub const fn cdev_bpf_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev_rwk(c, open, close, read, write, ioctl, kqfilter, D_CLONE)
}

/// The shape of the open/close/ioctl initialisers (`cdev_ch_init`, `cdev_uk_init`,
/// `cdev_kstat_init`, `cdev_usb_init`, `cdev_pci_init`, `cdev_radio_init`,
/// `cdev_gpio_init`, `cdev_bio_init`, `cdev_amdmsr_init`, `cdev_pvbus_init`,
/// `cdev_efi_init`).
const fn cdev_oci(c: i32, open: DevTypeOpen, close: DevTypeClose, ioctl: DevTypeIoctl) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        enodev_rw,
        enodev_rw,
        di_ioctl(c, ioctl),
        enodev_stop,
        None,
        enodev_mmap,
        0,
        0,
        None,
    )
}

/// `cdev_ch_init(c, n)`: open, close, ioctl.
pub const fn cdev_ch_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev_oci(c, open, close, ioctl)
}

/// `cdev_uk_init(c, n)`: open, close, ioctl.
pub const fn cdev_uk_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev_oci(c, open, close, ioctl)
}

/// `cdev_audio_init(c, n)`: open, close, read, write, ioctl, kqfilter.
pub const fn cdev_audio_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev_rwk(c, open, close, read, write, ioctl, kqfilter, 0)
}

/// `cdev_midi_init(c, n)`: open, close, read, write, ioctl, kqfilter.
pub const fn cdev_midi_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev_rwk(c, open, close, read, write, ioctl, kqfilter, 0)
}

/// `cdev_ksyms_init(c, n)`: open, close, read.
pub const fn cdev_ksyms_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        di_rw(c, read),
        enodev_rw,
        enodev_ioctl,
        enodev_stop,
        None,
        enodev_mmap,
        0,
        0,
        Some(seltrue_kqfilter),
    )
}

/// `cdev_kstat_init(c, n)`: open, close, ioctl.
pub const fn cdev_kstat_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev_oci(c, open, close, ioctl)
}

/// `cdev_wsdisplay_init(c, n)`: open, close, read, write, ioctl, stop, tty, mmap, kqfilter.
#[allow(clippy::too_many_arguments)] // the macro's nine entry points and the count
pub const fn cdev_wsdisplay_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    stop: DevTypeStop,
    tty: DevTypeTty,
    mmap: DevTypeMmap,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        di_rw(c, read),
        di_rw(c, write),
        di_ioctl(c, ioctl),
        di_stop(c, stop),
        Some(di_tty(c, tty)),
        di_mmap(c, mmap),
        D_TTY,
        0,
        Some(di_kqfilter(c, kqfilter)),
    )
}

/// `cdev_random_init(c, n)`: open, close, read, write, ioctl, kqfilter.
pub const fn cdev_random_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev_rwk(c, open, close, read, write, ioctl, kqfilter, 0)
}

/// `cdev_usb_init(c, n)`: open, close, ioctl, nokqfilter.
pub const fn cdev_usb_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev_oci(c, open, close, ioctl)
}

/// `cdev_ulpt_init(c, n)`: open, close, write.
pub const fn cdev_ulpt_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    write: DevTypeWrite,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        enodev_rw,
        di_rw(c, write),
        enodev_ioctl,
        enodev_stop,
        None,
        enodev_mmap,
        0,
        0,
        None,
    )
}

/// `cdev_pf_init(c, n)`: open, close, ioctl.
pub const fn cdev_pf_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    let mut sw = cdev_oci(c, open, close, ioctl);
    sw.d_flags = D_CLONE;
    sw
}

/// `cdev_usbdev_init(c, n)`: open, close, read, write, ioctl, kqfilter.
pub const fn cdev_usbdev_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev_rwk(c, open, close, read, write, ioctl, kqfilter, 0)
}

/// `cdev_fido_init(c, n)`: `fidoopen`, `uhidclose`, `uhidread`, `uhidwrite`, `fidoioctl`,
/// `uhidkqfilter`.
pub const fn cdev_fido_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev_rwk(c, open, close, read, write, ioctl, kqfilter, 0)
}

/// `cdev_ujoy_init(c, n)`: `ujoyopen`, `uhidclose`, `uhidread`, `uhidwrite`, `ujoyioctl`,
/// `uhidkqfilter`.
pub const fn cdev_ujoy_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev_rwk(c, open, close, read, write, ioctl, kqfilter, 0)
}

/// `cdev_pci_init(c, n)`: open, close, ioctl.
pub const fn cdev_pci_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev_oci(c, open, close, ioctl)
}

/// `cdev_radio_init(c, n)`: open, close, ioctl.
pub const fn cdev_radio_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev_oci(c, open, close, ioctl)
}

/// `cdev_video_init(c, n)`: open, close, ioctl, read, mmap, kqfilter.
pub const fn cdev_video_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    ioctl: DevTypeIoctl,
    mmap: DevTypeMmap,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        di_rw(c, read),
        enodev_rw,
        di_ioctl(c, ioctl),
        enodev_stop,
        None,
        di_mmap(c, mmap),
        0,
        0,
        Some(di_kqfilter(c, kqfilter)),
    )
}

/// `cdev_spkr_init(c, n)`: open, close, write, ioctl.
pub const fn cdev_spkr_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    write: DevTypeWrite,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        enodev_rw,
        di_rw(c, write),
        di_ioctl(c, ioctl),
        enodev_stop,
        None,
        enodev_mmap,
        0,
        0,
        Some(seltrue_kqfilter),
    )
}

/// `cdev_lpt_init(c, n)`: open, close, write.
pub const fn cdev_lpt_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    write: DevTypeWrite,
) -> Cdevsw {
    let mut sw = cdev_ulpt_init(c, open, close, write);
    sw.d_kqfilter = Some(seltrue_kqfilter);
    sw
}

/// `cdev_bktr_init(c, n)`: open, close, read, ioctl, mmap.
pub const fn cdev_bktr_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    ioctl: DevTypeIoctl,
    mmap: DevTypeMmap,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        di_rw(c, read),
        enodev_rw,
        di_ioctl(c, ioctl),
        enodev_stop,
        None,
        di_mmap(c, mmap),
        0,
        0,
        Some(seltrue_kqfilter),
    )
}

/// `cdev_hotplug_init(c, n)`: open, close, read, ioctl, kqfilter.
pub const fn cdev_hotplug_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    ioctl: DevTypeIoctl,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev_mouse_init(c, open, close, read, ioctl, kqfilter)
}

/// `cdev_gpio_init(c, n)`: open, close, ioctl.
pub const fn cdev_gpio_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev_oci(c, open, close, ioctl)
}

/// `cdev_bio_init(c, n)`: open, close, ioctl.
pub const fn cdev_bio_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev_oci(c, open, close, ioctl)
}

/// `cdev_drm_init(c, n)`: open, close, read, ioctl, mmap, kqfilter, cloning.
pub const fn cdev_drm_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    ioctl: DevTypeIoctl,
    mmap: DevTypeMmap,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    let mut sw = cdev_video_init(c, open, close, read, ioctl, mmap, kqfilter);
    sw.d_flags = D_CLONE;
    sw
}

/// `cdev_amdmsr_init(c, n)`: open, close, ioctl.
pub const fn cdev_amdmsr_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev_oci(c, open, close, ioctl)
}

/// `cdev_fuse_init(c, n)`: open, close, read, write, kqfilter, cloning.
pub const fn cdev_fuse_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    write: DevTypeWrite,
    kqfilter: DevTypeKqfilter,
) -> Cdevsw {
    cdev(
        di_open(c, open),
        di_close(c, close),
        di_rw(c, read),
        di_rw(c, write),
        enodev_ioctl,
        enodev_stop,
        None,
        enodev_mmap,
        0,
        D_CLONE,
        Some(di_kqfilter(c, kqfilter)),
    )
}

/// `cdev_pvbus_init(c, n)`: open, close, ioctl.
pub const fn cdev_pvbus_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev_oci(c, open, close, ioctl)
}

/// `cdev_ipmi_init(c, n)`: open, close, ioctl.
pub const fn cdev_ipmi_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    let mut sw = cdev_oci(c, open, close, ioctl);
    sw.d_kqfilter = Some(seltrue_kqfilter);
    sw
}

/// `cdev_efi_init(c, n)`: open, close, ioctl.
pub const fn cdev_efi_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    cdev_oci(c, open, close, ioctl)
}

/// `cdev_kcov_init(c, n)`: open, close, ioctl, mmap, cloning.
pub const fn cdev_kcov_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    ioctl: DevTypeIoctl,
    mmap: DevTypeMmap,
) -> Cdevsw {
    let mut sw = cdev_oci(c, open, close, ioctl);
    sw.d_mmap = di_mmap(c, mmap);
    sw.d_flags = D_CLONE;
    sw
}

/// `cdev_dt_init(c, n)`: open, close, read, ioctl, cloning.
pub const fn cdev_dt_init(
    c: i32,
    open: DevTypeOpen,
    close: DevTypeClose,
    read: DevTypeRead,
    ioctl: DevTypeIoctl,
) -> Cdevsw {
    let mut sw = cdev_oci(c, open, close, ioctl);
    sw.d_read = di_rw(c, read);
    sw.d_flags = D_CLONE;
    sw
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::param::NODEV;

    fn open_ok(_: Dev, _: i32, _: i32, _: &Proc) -> Result<(), Errno> {
        Ok(())
    }
    fn close_ok(_: Dev, _: i32, _: i32, _: Option<&Proc>) -> Result<(), Errno> {
        Ok(())
    }
    fn rw_ok(_: Dev, _: &mut Uio<'_>, _: i32) -> Result<(), Errno> {
        Ok(())
    }
    fn ioctl_ok(_: Dev, _: u64, _: &mut [u8], _: i32, _: &Proc) -> Result<(), Errno> {
        Ok(())
    }
    fn stop_ok(_: &Tty, _: i32) -> Result<(), Errno> {
        Ok(())
    }
    fn tty_none(_: Dev) -> Option<&'static Tty> {
        None
    }

    #[test]
    fn initialisers_follow_the_macros() {
        let notdef = cdev_notdef();
        assert_eq!(notdef.d_type, 0);
        assert!(notdef.d_tty.is_none());
        assert_eq!((notdef.d_mmap)(NODEV, 0, 0), None);
        let tp = Tty::new();
        assert_eq!((notdef.d_stop)(&tp, 0), Err(Errno::ENODEV));

        let tty = cdev_tty_init(
            1, open_ok, close_ok, rw_ok, rw_ok, ioctl_ok, stop_ok, tty_none,
        );
        assert_eq!(tty.d_type, D_TTY);
        assert!(tty.d_tty.is_some());
        assert_eq!((tty.d_stop)(&tp, 0), Ok(()));

        // A count of zero turns every dev_init entry into enxio.
        let absent = cdev_tty_init(
            0, open_ok, close_ok, rw_ok, rw_ok, ioctl_ok, stop_ok, tty_none,
        );
        assert_eq!((absent.d_stop)(&tp, 0), Err(Errno::ENXIO));

        assert_eq!(
            cdev_bpf_init(
                1,
                open_ok,
                close_ok,
                rw_ok,
                rw_ok,
                ioctl_ok,
                seltrue_kqfilter
            )
            .d_flags,
            D_CLONE
        );
        assert!(cdev_fd_init(1, open_ok).d_kqfilter.is_none());
        let ctty = cdev_ctty_init(1, open_ok, rw_ok, rw_ok, ioctl_ok, seltrue_kqfilter);
        assert_eq!((ctty.d_close)(NODEV, 0, 0, None), Ok(()));
        assert_eq!((ctty.d_stop)(&tp, 0), Ok(()));

        let b = bdev_notdef();
        assert!(b.d_psize.is_none());
        assert_eq!((b.d_close)(NODEV, 0, 0, None), Err(Errno::ENODEV));
    }
}
