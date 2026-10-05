/*	$OpenBSD: stand.h,v 1.72 2021/12/01 17:25:35 kettenis Exp $	*/
/*	$NetBSD: stand.h,v 1.18 1996/11/30 04:35:51 gwr Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)stand.h	8.1 (Berkeley) 6/11/93
 */
/* </LICENSES> */

//! `stand.h`: the standalone library's interface: the file system and device switches, the
//! open file table entry, and what the program that links libsa must define.
//!
//! Upstream: sys/lib/libsa/stand.h @ 3ce1f3f79392
//!
//! A boot program defines the tables libsa works through (`file_system[]`, `devsw[]`,
//! `constab[]` in its `conf.c`) and the machine-dependent routines it calls (`devopen()`,
//! `_rtt()`); the C resolves them when it links. Here the program gathers them in one
//! [`SaConf`] and hands it to [`sa_conf_register`] before it calls anything else in libsa.
//!
//! ## Deviations
//! - `extern struct fs_ops file_system[]; extern int nfsys;`, `extern struct devsw devsw[];
//!   extern int ndevs;`, `extern struct consdev constab[]` (from `cons.c`), `devopen()`,
//!   `_rtt()` and `<machine/loadfile_machdep.h>`'s `LOADADDR` are the members of [`SaConf`]
//!   (slices carry their lengths: no `nfsys`/`ndevs`). Until a program registers its table
//!   libsa works on an empty one: no file systems, no devices, no console.
//! - `struct fs_ops` and `struct devsw` hold Rust `fn` pointers; buffers are slices, the C's
//!   `size` is the slice's length, a NULL `resid`/`rsize` is `None`, and the routines return
//!   `Result<_, Errno>` where the C returns an error number or -1. `dv_open`'s variadic
//!   `char **file` is a `&mut &[u8]` the device may advance past its own prefix.
//!   `readdir`'s `char *name` (NULL to rewind) is `Option<&mut [u8]>`.
//! - `f_fsdata` is an owned `Box<dyn Any + Send>`: each file system keeps its own state type
//!   there and finds it again with a downcast, where the C casts a `void *` it `alloc()`ed.
//! - `struct stat` is libsa's subset ([`Stat`]); `max`/`min`, `bzero`/`bcmp`/`bcopy` and the
//!   string functions are Rust's own slice operations at the call sites; `btochs` has no
//!   caller in the boot programs ported here and is not declared.
//! - The prototypes of functions defined elsewhere in libsa live with their definitions.

use alloc::boxed::Box;
use core::any::Any;
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::hdr::cons::ConsDev;
use crate::hdr::stat::Stat;
use crate::hdr::types::{Daddr, Mode, Off};
use crate::saerrno::Errno;

/// `SEEK_SET`: set file offset to offset.
pub const SEEK_SET: i32 = 0;
/// `SEEK_CUR`: set file offset to current plus offset.
pub const SEEK_CUR: i32 = 1;
/// `SEEK_END`: set file offset to EOF plus offset.
pub const SEEK_END: i32 = 2;

/// `SOPEN_MAX`: the number of open files.
pub const SOPEN_MAX: usize = 4;

/// `F_READ`: file opened for reading.
pub const F_READ: i32 = 0x0001;
/// `F_WRITE`: file opened for writing.
pub const F_WRITE: i32 = 0x0002;
/// `F_RAW`: raw device open, no file system.
pub const F_RAW: i32 = 0x0004;
/// `F_NODEV`: network open, no device.
pub const F_NODEV: i32 = 0x0008;
/// `F_NOWRITE`: bootblock writing broken or unsupported.
pub const F_NOWRITE: i32 = 0x0010;

/// `BOOTRANDOM`: the random seed file.
pub const BOOTRANDOM: &[u8] = b"/etc/random.seed";
/// `BOOTRANDOM_MAX`: no point being greater than `RC4STATE`.
pub const BOOTRANDOM_MAX: usize = 256;

/// `O_RDONLY`: open for reading only.
pub const O_RDONLY: i32 = 0x0000;
/// `O_WRONLY`: open for writing only.
pub const O_WRONLY: i32 = 0x0001;
/// `O_RDWR`: open for reading and writing.
pub const O_RDWR: i32 = 0x0002;

/// `fs_ops.fchmod`.
pub type FchmodFn = fn(f: &mut OpenFile, mode: Mode) -> Result<(), Errno>;

/// `devsw.dv_strategy`.
pub type StrategyFn = fn(
    devdata: *mut c_void,
    rw: i32,
    blk: Daddr,
    buf: &mut [u8],
    rsize: Option<&mut usize>,
) -> Result<(), Errno>;

/// `struct fs_ops`: the operations of one file system.
pub struct FsOps {
    /// `open`: look `path` up and set `f_fsdata`.
    pub open: fn(path: &[u8], f: &mut OpenFile) -> Result<(), Errno>,
    /// `close`: release `f_fsdata`.
    pub close: fn(f: &mut OpenFile) -> Result<(), Errno>,
    /// `read`: fill `buf` from the file offset; `resid` gets what was not read.
    pub read: fn(f: &mut OpenFile, buf: &mut [u8], resid: &mut usize) -> Result<(), Errno>,
    /// `write`.
    pub write: fn(f: &mut OpenFile, buf: &[u8], resid: &mut usize) -> Result<(), Errno>,
    /// `seek`: the new offset.
    pub seek: fn(f: &mut OpenFile, offset: Off, whence: i32) -> Result<Off, Errno>,
    /// `stat`.
    pub stat: fn(f: &mut OpenFile, sb: &mut Stat) -> Result<(), Errno>,
    /// `readdir`: the next name into `name` (NUL-terminated), or rewind when `None`.
    pub readdir: fn(f: &mut OpenFile, name: Option<&mut [u8]>) -> Result<(), Errno>,
    /// `fchmod`: `None` where the file system cannot write (the C's NULL slot).
    pub fchmod: Option<FchmodFn>,
}

/// `struct devsw`: a device switch entry.
pub struct Devsw {
    /// `dv_name`.
    pub dv_name: &'static str,
    /// `dv_strategy`: transfer `buf.len()` bytes at block `blk` (in `DEV_BSIZE` units)
    /// in direction `rw` ([`F_READ`] or [`F_WRITE`]); `rsize` gets the size moved.
    pub dv_strategy: StrategyFn,
    /// `dv_open`: claim the device named at the front of `*file` and leave `*file` at the
    /// file name; any `Err` means "not this device".
    pub dv_open: for<'a> fn(f: &mut OpenFile, file: &mut &'a [u8]) -> Result<(), Errno>,
    /// `dv_close`.
    pub dv_close: fn(f: &mut OpenFile) -> Result<(), Errno>,
    /// `dv_ioctl`.
    pub dv_ioctl: fn(f: &mut OpenFile, cmd: u64, data: *mut c_void) -> Result<(), Errno>,
}

/// `struct open_file`.
pub struct OpenFile {
    /// `f_flags`: see `F_*`.
    pub f_flags: i32,
    /// `f_dev`: the device operations.
    pub f_dev: Option<&'static Devsw>,
    /// `f_devdata`: device specific data, which only the device's routines interpret.
    pub f_devdata: *mut c_void,
    /// `f_ops`: the file system operations.
    pub f_ops: Option<&'static FsOps>,
    /// `f_fsdata`: file system specific data.
    pub f_fsdata: Option<Box<dyn Any + Send>>,
    /// `f_offset`: the current file offset (`F_RAW`).
    pub f_offset: Off,
}

// SAFETY: the standalone programs run on one CPU without threads or interrupts that touch
// libsa; `f_devdata` is only dereferenced by the device routines of the program that set it.
unsafe impl Send for OpenFile {}

impl OpenFile {
    /// A free slot.
    pub const fn new() -> Self {
        Self {
            f_flags: 0,
            f_dev: None,
            f_devdata: core::ptr::null_mut(),
            f_ops: None,
            f_fsdata: None,
            f_offset: 0,
        }
    }

    /// The device half of the open file, which the file systems read blocks through while
    /// they hold their own state (`f_fsdata`) borrowed.
    pub fn io(&self) -> DevIo {
        DevIo {
            dev: self.f_dev,
            data: self.f_devdata,
        }
    }

    /// The file system state of type `T` in `f_fsdata`, if that is what it holds.
    pub fn fsdata<T: 'static>(&mut self) -> Option<&mut T> {
        self.f_fsdata.as_mut().and_then(|d| d.downcast_mut::<T>())
    }
}

/// `f->f_dev` and `f->f_devdata`, copied out of an [`OpenFile`].
#[derive(Clone, Copy)]
pub struct DevIo {
    /// `f_dev`.
    pub dev: Option<&'static Devsw>,
    /// `f_devdata`.
    pub data: *mut c_void,
}

impl DevIo {
    /// `(f->f_dev->dv_strategy)(f->f_devdata, rw, blk, size, buf, rsize)`, preceded by the
    /// `twiddle()` the file systems print before every transfer.
    pub fn strategy(
        self,
        rw: i32,
        blk: Daddr,
        buf: &mut [u8],
        rsize: Option<&mut usize>,
    ) -> Result<(), Errno> {
        crate::printf::twiddle();
        match self.dev {
            Some(dv) => (dv.dv_strategy)(self.data, rw, blk, buf, rsize),
            None => Err(Errno::ENXIO),
        }
    }
}

impl Default for OpenFile {
    fn default() -> Self {
        Self::new()
    }
}

/// What a program that links libsa defines for it: its `conf.c` tables and its
/// machine-dependent routines.
pub struct SaConf {
    /// `file_system[]`: the file systems `open()` tries, in order.
    pub file_system: &'static [FsOps],
    /// `devsw[]`: the devices `devopen()` tries.
    pub devsw: &'static [Devsw],
    /// `constab[]`: the consoles `cninit()` probes.
    pub constab: &'static [ConsDev],
    /// `devopen(f, fname, &file)`: open the device `fname` names; the file name that follows
    /// it (empty for the raw device).
    pub devopen: for<'a> fn(f: &mut OpenFile, fname: &'a [u8]) -> Result<&'a [u8], Errno>,
    /// `_rtt()`: return to the firmware (reboot).
    pub rtt: fn() -> !,
    /// `LOADADDR(a)` of `<machine/loadfile_machdep.h>`: where `loadfile` puts the byte the
    /// kernel expects at address `a`, given the `offset` it was called with.
    pub loadaddr: fn(a: u64, offset: u64) -> u64,
}

/// The table before a program registers its own.
static EMPTY_CONF: SaConf = SaConf {
    file_system: &[],
    devsw: &[],
    constab: &[],
    devopen: |_, _| Err(Errno::ENXIO),
    rtt: || loop {
        core::hint::spin_loop();
    },
    loadaddr: |a, offset| a.wrapping_add(offset),
};

/// The registered table (null: [`EMPTY_CONF`]).
static SA_CONF: AtomicPtr<SaConf> = AtomicPtr::new(core::ptr::null_mut());

/// Registers the program's tables and routines; the program calls it first.
pub fn sa_conf_register(conf: &'static SaConf) {
    SA_CONF.store(core::ptr::from_ref(conf).cast_mut(), Ordering::Release);
}

/// The registered table.
pub fn sa_conf() -> &'static SaConf {
    let p = SA_CONF.load(Ordering::Acquire);
    if p.is_null() {
        &EMPTY_CONF
    } else {
        // SAFETY: a non-null value was stored by `sa_conf_register` from a `&'static SaConf`,
        // which is never written through.
        unsafe { &*p }
    }
}

/// `isupper(c)`.
pub const fn isupper(c: u8) -> bool {
    c.is_ascii_uppercase()
}

/// `islower(c)`.
pub const fn islower(c: u8) -> bool {
    c.is_ascii_lowercase()
}

/// `isalpha(c)`.
pub const fn isalpha(c: u8) -> bool {
    isupper(c) || islower(c)
}

/// `tolower(c)`.
pub const fn tolower(c: u8) -> u8 {
    c.to_ascii_lowercase()
}

/// `toupper(c)`.
pub const fn toupper(c: u8) -> u8 {
    c.to_ascii_uppercase()
}

/// `isspace(c)`: libsa's, a blank or a tab only.
pub const fn isspace(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

/// `isdigit(c)`.
pub const fn isdigit(c: u8) -> bool {
    c.is_ascii_digit()
}
