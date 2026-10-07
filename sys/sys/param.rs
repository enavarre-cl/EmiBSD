/*	$OpenBSD: param.h,v 1.147 2026/07/16 06:21:08 deraadt Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1989, 1993
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
 */
/* </LICENSES> */

//! Machine-independent kernel parameters: `<sys/param.h>`.
//!
//! Upstream: sys/sys/param.h @ 3ce1f3f79392
//!
//! The machine-dependent half (`<machine/param.h>`: `PAGE_SIZE`, `KERNBASE`, `MSGBUFSIZE`, ...)
//! is reached through [`MachineParam`] and re-exported here, so `sys::param` is the one place to
//! import from, as `<sys/param.h>` is in C.
//!
//! ## Deviations
//! - `<sys/param.h>` also includes `errno.h`, `time.h`, `resource.h`, `ucred.h`, `uio.h`,
//!   `srp.h` and `signal.h` for convenience. Those are modules of their own here, ported with
//!   their milestones; nothing is re-exported.
//! - `NULL` (`<sys/_null.h>`) and the integer limits of `<sys/limits.h>` are `Option`/null
//!   pointers and `i32::MAX`-style constants; neither header is ported.
//! - Function-like macros become snake-case `const fn`s: `ALIGN` → [`align`],
//!   `ALIGNED_POINTER` → [`aligned_pointer`], plus [`ctod`], [`dtoc`], [`btodb`], [`dbtob`],
//!   [`howmany`], [`roundup`], [`powerof2`], [`setbit`], [`clrbit`], [`isset`], [`isclr`].
//! - `MIN`/`MAX` are `core::cmp::{min, max}`; `offsetof` is `core::mem::offset_of!`; `nitems`
//!   is `slice::len`; `SET`/`CLR`/`ISSET` are `|=`, `&= !` and `&` (or `bitflags` methods).
//! - `NBBY` belongs to `<sys/select.h>` (not ported yet); the bitmap helpers use `u8::BITS`.
//! - `NBPG`, `PGSHIFT`, `PGOFSET` are defined here once; every OpenBSD architecture defines
//!   them identically from `PAGE_*`.

use crate::machine::{Machine, MachineParam};
use crate::sys::syslimits::{
    _MAXCOMLEN, ARG_MAX, CHILD_MAX, LOGIN_NAME_MAX, NGROUPS_MAX, OPEN_MAX, PATH_MAX, SYMLOOP_MAX,
};
use crate::sys::types::{Dev, Fixpt, Gid};

/// System version (year & month).
pub const BSD: u32 = 199306;
/// 4.3BSD.
pub const BSD4_3: u32 = 1;
/// 4.4BSD.
pub const BSD4_4: u32 = 1;

/// OpenBSD version (year & month).
#[allow(non_upper_case_globals)] // OpenBSD's own spelling of its version macro
pub const OpenBSD: u32 = 202610;
/// OpenBSD 8.0.
#[allow(non_upper_case_globals)] // OpenBSD's own spelling of its version macro
pub const OpenBSD8_0: u32 = 1;

/*
 * Machine-independent constants (some used in following include files).
 * Redefined constants are from POSIX 1003.1 limits file.
 *
 * MAXCOMLEN should be >= sizeof(ac_comm) (see <acct.h>)
 * MAXLOGNAME should be >= UT_NAMESIZE (see <utmp.h>)
 */

/// Max command name remembered, without NUL.
pub const MAXCOMLEN: usize = _MAXCOMLEN - 1;
/// Max interpreter file name length.
pub const MAXINTERP: usize = 128;
/// Max login name length with NUL.
pub const MAXLOGNAME: usize = LOGIN_NAME_MAX;
/// Max simultaneous processes.
pub const MAXUPRC: usize = CHILD_MAX;
/// Max bytes for an exec function.
pub const NCARGS: usize = ARG_MAX;
/// Max number of groups.
pub const NGROUPS: usize = NGROUPS_MAX;
/// Max open files per process (soft).
pub const NOFILE: usize = OPEN_MAX;
/// Max open files per process (hard).
pub const NOFILE_MAX: usize = 1024;
/// Marker for an empty group set member.
pub const NOGROUP: Gid = 65535;
/// Max hostname length with NUL.
pub const MAXHOSTNAMELEN: usize = 256;

/*
 * Priorities.  Note that with 32 run queues, differences less than 4 are
 * insignificant.
 */

/// Sleep priority of the swapper.
pub const PSWP: i32 = 0;
/// Sleep priority of the VM system.
pub const PVM: i32 = 4;
/// Sleep priority for inode waits.
pub const PINOD: i32 = 8;
/// Sleep priority for block I/O.
pub const PRIBIO: i32 = 16;
/// Sleep priority for VFS waits.
pub const PVFS: i32 = 20;
/// No longer magic, shouldn't be here.
pub const PZERO: i32 = 22;
/// Sleep priority for socket waits.
pub const PSOCK: i32 = 24;
/// Sleep priority for `wait(2)`.
pub const PWAIT: i32 = 32;
/// Sleep priority for lock waits.
pub const PLOCK: i32 = 36;
/// Sleep priority for `pause(2)`.
pub const PPAUSE: i32 = 40;
/// First user priority.
pub const PUSER: i32 = 50;
/// Priorities range from 0 through MAXPRI.
pub const MAXPRI: i32 = 127;

/// `NZERO`: default \"nice\".
pub const NZERO: i32 = 20;

/// Mask selecting the priority bits of a `tsleep(9)` priority argument.
pub const PRIMASK: i32 = 0x0ff;
/// OR'd with the priority for `tsleep(9)` to check signals.
pub const PCATCH: i32 = 0x100;
/// OR'd with the priority for `msleep(9)` to not reacquire the mutex.
pub const PNORELOCK: i32 = 0x200;

/// Non-existent device.
pub const NODEV: Dev = -1;

// Machine type dependent parameters: <machine/param.h> through the machine contract.

/// log2 of the page size.
pub const PAGE_SHIFT: usize = <Machine as MachineParam>::PAGE_SHIFT;
/// Bytes per page.
pub const PAGE_SIZE: usize = <Machine as MachineParam>::PAGE_SIZE;
/// Byte offset mask within a page.
pub const PAGE_MASK: usize = <Machine as MachineParam>::PAGE_MASK;
/// Start of kernel virtual address space.
pub const KERNBASE: usize = <Machine as MachineParam>::KERNBASE;
/// Bytes per page.
pub const NBPG: usize = PAGE_SIZE;
/// LOG2(PAGE_SIZE).
pub const PGSHIFT: usize = PAGE_SHIFT;
/// Byte offset into page.
pub const PGOFSET: usize = PAGE_MASK;
/// Pages of u-area.
pub const UPAGES: usize = <Machine as MachineParam>::UPAGES;
/// Total size of the u-area.
pub const USPACE: usize = <Machine as MachineParam>::USPACE;
/// u-area alignment, 0 for none.
pub const USPACE_ALIGN: usize = <Machine as MachineParam>::USPACE_ALIGN;
/// Max mbuf cluster allocation.
pub const NMBCLUSTERS: usize = <Machine as MachineParam>::NMBCLUSTERS;
/// Default message buffer size.
pub const MSGBUFSIZE: usize = <Machine as MachineParam>::MSGBUFSIZE;

/// Rounding mask that aligns an address for every data type.
pub const ALIGNBYTES: usize = <Machine as MachineParam>::ALIGNBYTES;
/// `__STRICT_ALIGNMENT` (`<machine/endian.h>`): whether the CPU needs aligned data.
pub const STRICT_ALIGNMENT: bool = <Machine as MachineParam>::STRICT_ALIGNMENT;

/*
 * File system parameters and macros.
 *
 * The file system is made out of blocks of at most MAXBSIZE units, with
 * smaller units (fragments) only in the last direct block.  MAXBSIZE
 * primarily determines the size of buffers in the buffer pool.  It may be
 * made larger without any effect on existing file systems; however making
 * it smaller makes some file systems unmountable.
 */

/// Max raw I/O transfer size.
pub const MAXPHYS: usize = 64 * 1024;
/// Largest file system block size; sizes the buffer pool's buffers.
pub const MAXBSIZE: usize = 64 * 1024;

/// log2(DEV_BSIZE).
pub const _DEV_BSHIFT: usize = 9;
/// Disk block size in bytes.
pub const DEV_BSIZE: usize = 1 << _DEV_BSHIFT;
/// log2(DEV_BSIZE).
pub const DEV_BSHIFT: usize = _DEV_BSHIFT;
/// I/O size for block devices.
pub const BLKDEV_IOSIZE: usize = PAGE_SIZE;

/*
 * MAXPATHLEN defines the longest permissible path length after expanding
 * symbolic links. It is used to allocate a temporary buffer from the buffer
 * pool in which to do the name expansion, hence should be a power of two,
 * and must be less than or equal to MAXBSIZE.  MAXSYMLINKS defines the
 * maximum number of symbolic links that may be expanded in a path name.
 * It should be set high enough to allow all legitimate uses, but halt
 * infinite loops reasonably quickly.
 */

/// Longest permissible path length after expanding symbolic links.
pub const MAXPATHLEN: usize = PATH_MAX;
/// Maximum number of symbolic links that may be expanded in a path name.
pub const MAXSYMLINKS: usize = SYMLOOP_MAX;

/*
 * Scale factor for scaled integers used to count %cpu time and load avgs.
 *
 * The number of CPU `tick's that map to a unique `%age' can be expressed
 * by the formula (1 / (2 ^ (FSHIFT - 11))).  The maximum load average that
 * can be calculated (assuming 32 bits) can be closely approximated using
 * the formula (2 ^ (2 * (16 - FSHIFT))) for (FSHIFT < 15).
 *
 * For the scheduler to maintain a 1:1 mapping of CPU `tick' to `%age',
 * FSHIFT must be at least 11; this gives us a maximum load avg of ~1024.
 */

/// Bits to the right of the fixed binary point.
pub const _FSHIFT: u32 = 11;
/// Bits to the right of the fixed binary point.
pub const FSHIFT: u32 = _FSHIFT;
/// One, as a fixed-point number.
pub const FSCALE: Fixpt = 1 << _FSHIFT;

/// `ALIGN(p)`: rounds `p` (a pointer or byte index) up to a value aligned for all data types.
pub const fn align(p: usize) -> usize {
    (p + ALIGNBYTES) & !ALIGNBYTES
}

/// `ALIGNED_POINTER(p, t)`: whether a `T` may be fetched from address `p` on this machine.
pub fn aligned_pointer<T>(p: usize) -> bool {
    <Machine as MachineParam>::aligned_pointer::<T>(p)
}

/// `ctod(x)`: pages to disk blocks.
pub const fn ctod(x: usize) -> usize {
    x << (PAGE_SHIFT - _DEV_BSHIFT)
}

/// `dtoc(x)`: disk blocks to pages.
pub const fn dtoc(x: usize) -> usize {
    x >> (PAGE_SHIFT - _DEV_BSHIFT)
}

/// `btodb(x)`: bytes to disk blocks.
pub const fn btodb(x: usize) -> usize {
    x >> _DEV_BSHIFT
}

/// `dbtob(x)`: disk blocks to bytes.
pub const fn dbtob(x: usize) -> usize {
    x << _DEV_BSHIFT
}

// Bit map related macros: `a` is a byte array, `i` a bit index.

/// `setbit(a, i)`: sets bit `i` of the bitmap `a`.
pub fn setbit(a: &mut [u8], i: usize) {
    a[i / u8::BITS as usize] |= 1 << (i % u8::BITS as usize);
}

/// `clrbit(a, i)`: clears bit `i` of the bitmap `a`.
pub fn clrbit(a: &mut [u8], i: usize) {
    a[i / u8::BITS as usize] &= !(1 << (i % u8::BITS as usize));
}

/// `isset(a, i)`: whether bit `i` of the bitmap `a` is set.
pub fn isset(a: &[u8], i: usize) -> bool {
    a[i / u8::BITS as usize] & (1 << (i % u8::BITS as usize)) != 0
}

/// `isclr(a, i)`: whether bit `i` of the bitmap `a` is clear.
pub fn isclr(a: &[u8], i: usize) -> bool {
    !isset(a, i)
}

// Macros for counting and rounding.

/// `howmany(x, y)`: how many `y`-sized units cover `x` (division rounding up).
pub const fn howmany(x: usize, y: usize) -> usize {
    x.div_ceil(y)
}

/// `roundup(x, y)`: `x` rounded up to a multiple of `y`.
pub const fn roundup(x: usize, y: usize) -> usize {
    x.div_ceil(y) * y
}

/// `powerof2(x)`: whether `x` is a power of two. As in C, `powerof2(0)` is true.
pub const fn powerof2(x: usize) -> bool {
    (x.wrapping_sub(1) & x) == 0
}

#[cfg(test)]
mod tests;
