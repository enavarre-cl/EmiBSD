/*	$OpenBSD: swapgeneric.c,v 1.6 2024/10/30 07:28:17 jsg Exp $ */
/*	$NetBSD: swapgeneric.c,v 1.12 1996/05/03 19:42:28 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz.
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
 *	@(#)swapgeneric.c	5.5 (Berkeley) 5/9/91
 */
/* </LICENSES> */

//! The root and swap configuration `config(8)` generates for a kernel: `swapgeneric.c` for
//! `config bsd swap generic` (GENERIC), and, as a deviation, the values its `swapbsd.c` would
//! hold for `config bsd root on rd0a swap on rd0b` (arm64's RAMDISK line; amd64's adds
//! `wd0b` and `sd0b`, drivers not ported).
//!
//! Upstream: sys/conf/swapgeneric.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - OpenBSD builds two kernels, `bsd` (GENERIC, "swap generic") and `bsd.rd` (RAMDISK,
//!   "root on rd0a"), and boot(8) loads one. Here one kernel serves both: the statics hold
//!   the generic values, and `swapconf_rdroot` switches them to the RAMDISK values when the
//!   boot loader hands over a ramdisk image (`sys/stand`, before `main`), as booting
//!   `bsd.rd` would. Without a ramdisk the kernel stays generic.
//! - `swdevt[]` has room for the RAMDISK configuration's one swap device; `uvm_swap.c`, its
//!   reader, is not ported.

use core::sync::atomic::AtomicI32;

use libkern::StaticCell;

use crate::sys::disklabel::makediskdev;
use crate::sys::param::NODEV;
use crate::sys::systm::MountrootFn;

/// `mountroot`: the routine that mounts the root file system; NULL tells `autoconf.c` that
/// the kernel is "generic" (`setroot` then picks one).
pub static MOUNTROOT: StaticCell<Option<MountrootFn>> = StaticCell::new(None);

/// `rootdev`: the root device, `NODEV` until `setroot` picks one.
pub static ROOTDEV: AtomicI32 = AtomicI32::new(NODEV);

/// `dumpdev`: the dump device.
pub static DUMPDEV: AtomicI32 = AtomicI32::new(NODEV);

/// `swdevt[]`: the swap devices, `NODEV`-terminated ("to be filled in" by `setroot`).
pub static SWDEVT: [AtomicI32; 2] = [AtomicI32::new(NODEV), AtomicI32::new(NODEV)];

/// The `rd(4)` block major on both architectures (`nam2blk[]` of their `autoconf.c`).
const RD_BMAJOR: u32 = 17;

/// What `swapbsd.c` holds for `config bsd root on rd0a swap on rd0b`: `rootdev` rd0a,
/// `dumpdev` and `swdevt[0]` rd0b, `mountroot` `dk_mountroot`.
///
/// # Safety
/// Called once, on the boot CPU before `main` reads `mountroot` (no other access).
pub unsafe fn swapconf_rdroot() {
    use core::sync::atomic::Ordering::Relaxed;
    ROOTDEV.store(makediskdev(RD_BMAJOR, 0, 0), Relaxed);
    DUMPDEV.store(makediskdev(RD_BMAJOR, 0, 1), Relaxed);
    SWDEVT[0].store(makediskdev(RD_BMAJOR, 0, 1), Relaxed);
    SWDEVT[1].store(NODEV, Relaxed);
    // SAFETY: the caller guarantees the boot CPU is the only accessor.
    unsafe { MOUNTROOT.write(Some(crate::kern::subr_disk::dk_mountroot)) };
}
