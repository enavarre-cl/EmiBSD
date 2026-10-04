/*	$OpenBSD: disk.h,v 1.42 2025/09/15 10:33:03 krw Exp $	*/
/*	$NetBSD: disk.h,v 1.11 1996/04/28 20:22:50 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1995 Jason R. Thorpe.  All rights reserved.
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This software was developed by the Computer Systems Engineering group
 * at Lawrence Berkeley Laboratory under DARPA contract BG 91-66 and
 * contributed to Berkeley.
 *
 * All advertising materials mentioning features or use of this software
 * must display the following acknowledgement:
 *	This product includes software developed by the University of
 *	California, Lawrence Berkeley Laboratory.
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
 * from: Header: disk.h,v 1.5 92/11/19 04:33:03 torek Exp  (LBL)
 *
 *	@(#)disk.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

//! `<sys/disk.h>`: disk device structures (`struct disk`, `struct diskstats`) and the global
//! list of disks.
//!
//! Upstream: sys/sys/disk.h @ 3ce1f3f79392
//!
//! A disk driver embeds a [`Disk`] in its softc, after the [`Device`], and hands it to
//! `disk_attach` (`kern/subr_disk.rs`), which links it on `disklist` and allocates its in-core
//! label. The functions the header declares (`disk_init` .. `duid_format`) live in
//! `kern/subr_disk.rs`, with `disklist`, `disk_count` and `disk_change`.
//!
//! ## Deviations
//! - Every member that changes after attach is a `Cell` (or the `Rwlock`/`Mutex` the C has):
//!   the C changes them through `struct disk *` under `dk_lock`, `dk_mtx` or the kernel lock;
//!   one CPU here. All-zero is a valid `Disk` (it lives in an `M_ZERO` softc).
//! - `dk_name` is a copy of the name (`DS_DISKNAMELEN` bytes, NUL padded) instead of a
//!   `char *` into the softc's `dv_xname`: the same bytes without a self-reference.
//! - `dk_device` and `dk_label` are `Option<NonNull<_>>` (the C's possibly NULL pointers);
//!   [`Disk::label`] and [`Disk::label_mut`] reach the label.
//! - `struct diskstats` has an explicit `ds_pad0` where the C compiler inserts four bytes of
//!   padding before `ds_rxfer`: the layout is the C's (checked in `sys/sysctl.rs`) with no
//!   uninitialised bytes, so `hw.diskstats` copies it out as bytes.

use core::cell::Cell;
use core::ptr::NonNull;

use crate::queue_adapter;
use crate::sys::device::Device;
use crate::sys::disklabel::Disklabel;
use crate::sys::mutex::Mutex;
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::time::Timeval;
use crate::sys::types::Dev;

/// `DS_DISKNAMELEN`.
pub const DS_DISKNAMELEN: usize = 16;

/// `struct diskstats`: what `hw.diskstats` reports per disk.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Diskstats {
    /// `ds_name`.
    pub ds_name: [u8; DS_DISKNAMELEN],
    /// `ds_busy`: busy counter.
    pub ds_busy: i32,
    /// The four bytes of padding the C compiler puts here to align `ds_rxfer`.
    pub ds_pad0: u32,
    /// `ds_rxfer`: total number of read transfers.
    pub ds_rxfer: u64,
    /// `ds_wxfer`: total number of write transfers.
    pub ds_wxfer: u64,
    /// `ds_seek`: total independent seek operations.
    pub ds_seek: u64,
    /// `ds_rbytes`: total bytes read.
    pub ds_rbytes: u64,
    /// `ds_wbytes`: total bytes written.
    pub ds_wbytes: u64,
    /// `ds_attachtime`: time disk was attached.
    pub ds_attachtime: Timeval,
    /// `ds_timestamp`: time of first busy or any unbusy.
    pub ds_timestamp: Timeval,
    /// `ds_time`: total time spent busy.
    pub ds_time: Timeval,
}

/// `DKF_CONSTRUCTED`.
pub const DKF_CONSTRUCTED: i32 = 0x0001;
/// `DKF_OPENED`.
pub const DKF_OPENED: i32 = 0x0002;
/// `DKF_NOLABELREAD`.
pub const DKF_NOLABELREAD: i32 = 0x0004;

/// `struct disk`.
pub struct Disk {
    /// `dk_link`: link in global disklist.
    pub dk_link: TailqEntry<Disk>,
    /// `dk_lock`: disk lock.
    pub dk_lock: Rwlock,
    /// `dk_mtx`: busy/unbusy mtx.
    pub dk_mtx: Mutex,
    /// `dk_name`: disk name.
    pub dk_name: Cell<[u8; DS_DISKNAMELEN]>,
    /// `dk_device`: disk device structure.
    pub dk_device: Cell<Option<NonNull<Device>>>,
    /// `dk_devno`: disk device number.
    pub dk_devno: Cell<Dev>,
    /// `dk_flags`: disk flags (`DKF_*`).
    pub dk_flags: Cell<i32>,

    // Metrics data; note that some metrics may have no meaning on certain types of disks.
    /// `dk_busy`: busy counter.
    pub dk_busy: Cell<i32>,
    /// `dk_rxfer`: total number of read transfers.
    pub dk_rxfer: Cell<u64>,
    /// `dk_wxfer`: total number of write transfers.
    pub dk_wxfer: Cell<u64>,
    /// `dk_seek`: total independent seek operations.
    pub dk_seek: Cell<u64>,
    /// `dk_rbytes`: total bytes read.
    pub dk_rbytes: Cell<u64>,
    /// `dk_wbytes`: total bytes written.
    pub dk_wbytes: Cell<u64>,
    /// `dk_attachtime`: time disk was attached.
    pub dk_attachtime: Cell<Timeval>,
    /// `dk_timestamp`: time of first busy or any unbusy.
    pub dk_timestamp: Cell<Timeval>,
    /// `dk_time`: total time spent busy.
    pub dk_time: Cell<Timeval>,

    /// `dk_bopenmask`: block devices open.
    pub dk_bopenmask: Cell<u64>,
    /// `dk_copenmask`: character devices open.
    pub dk_copenmask: Cell<u64>,
    /// `dk_openmask`: composite (bopen|copen).
    pub dk_openmask: Cell<u64>,
    /// `dk_state`: label state (`DK_*`).
    pub dk_state: Cell<i32>,
    /// `dk_blkshift`: shift to convert `DEV_BSIZE` to blks.
    pub dk_blkshift: Cell<i32>,
    /// `dk_byteshift`: shift to convert bytes to blks.
    pub dk_byteshift: Cell<i32>,

    /// `dk_label`: disk label information. Storage for the in-core disk label must be
    /// dynamically allocated, otherwise the size of this structure becomes
    /// machine-dependent.
    pub dk_label: Cell<Option<NonNull<Disklabel>>>,
}

// SAFETY: the members change under `dk_lock`, `dk_mtx` or the kernel lock, as in C; the kernel
// runs one CPU.
unsafe impl Sync for Disk {}

impl Disk {
    /// A disk that is not attached (all zero, as in an `M_ZERO` softc).
    pub const fn new() -> Self {
        Self {
            dk_link: TailqEntry::new(),
            dk_lock: Rwlock::new(""),
            dk_mtx: Mutex::new(0),
            dk_name: Cell::new([0; DS_DISKNAMELEN]),
            dk_device: Cell::new(None),
            dk_devno: Cell::new(0),
            dk_flags: Cell::new(0),
            dk_busy: Cell::new(0),
            dk_rxfer: Cell::new(0),
            dk_wxfer: Cell::new(0),
            dk_seek: Cell::new(0),
            dk_rbytes: Cell::new(0),
            dk_wbytes: Cell::new(0),
            dk_attachtime: Cell::new(Timeval::new(0, 0)),
            dk_timestamp: Cell::new(Timeval::new(0, 0)),
            dk_time: Cell::new(Timeval::new(0, 0)),
            dk_bopenmask: Cell::new(0),
            dk_copenmask: Cell::new(0),
            dk_openmask: Cell::new(0),
            dk_state: Cell::new(0),
            dk_blkshift: Cell::new(0),
            dk_byteshift: Cell::new(0),
            dk_label: Cell::new(None),
        }
    }

    /// `dk_name` up to its NUL, as a `&str` (`"?"` if it is not UTF-8).
    pub fn name(&self) -> &str {
        // SAFETY: `dk_name` is written once by the driver's attach, before the disk is
        // shared, and never again, so no `set` overlaps this view of the cell's bytes.
        let bytes = unsafe { &*self.dk_name.as_ptr() };
        let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        core::str::from_utf8(&bytes[..len]).unwrap_or("?")
    }

    /// `*dk->dk_label`, read: `None` before `disk_attach` allocated it.
    pub fn label(&self) -> Option<Disklabel> {
        // SAFETY: `dk_label` is NULL or the label `disk_attach` allocated, which lives until
        // `disk_detach`; the copy is taken while no `label_mut` borrow is live (the
        // drivers' rule, see `label_mut`).
        self.dk_label.get().map(|p| unsafe { *p.as_ptr() })
    }

    /// `dk->dk_label`, writable: `None` before `disk_attach` allocated it.
    ///
    /// # Safety
    ///
    /// The caller is the disk's driver, serialised against every other user of the label
    /// (by `dk_lock`, the kernel lock or `splbio`, as the C's callers are), and keeps no
    /// other reference to the label while the returned one is live.
    #[allow(clippy::mut_from_ref)] // the C's `struct disklabel *` member, shared by pointer
    pub unsafe fn label_mut(&self) -> Option<&mut Disklabel> {
        // SAFETY: the caller's contract; the pointer is the live allocation of `disk_attach`.
        self.dk_label.get().map(|p| unsafe { &mut *p.as_ptr() })
    }
}

impl Default for Disk {
    fn default() -> Self {
        Self::new()
    }
}

// states

/// `DK_CLOSED`: drive is closed.
pub const DK_CLOSED: i32 = 0;
/// `DK_WANTOPEN`: drive being opened.
pub const DK_WANTOPEN: i32 = 1;
/// `DK_WANTOPENRAW`: drive being opened.
pub const DK_WANTOPENRAW: i32 = 2;
/// `DK_RDLABEL`: label being read.
pub const DK_RDLABEL: i32 = 3;
/// `DK_OPEN`: label read, drive open.
pub const DK_OPEN: i32 = 4;
/// `DK_OPENRAW`: open without label.
pub const DK_OPENRAW: i32 = 5;

// Disk map flags.

/// `DM_OPENPART`: Open raw partition.
pub const DM_OPENPART: i32 = 0x1;
/// `DM_OPENBLCK`: Open block device.
pub const DM_OPENBLCK: i32 = 0x2;

queue_adapter!(
    /// `TAILQ_ENTRY(disk) dk_link`: `disklist`.
    pub DiskList: Disk, dk_link => TailqEntry<Disk>
);

/// `TAILQ_HEAD(disklist_head, disk)`: the disklist is a TAILQ.
pub struct DisklistHead(pub TailqHead<DiskList>);

// SAFETY: the list changes under the kernel lock (`disk_attach`, `disk_detach`); one CPU here.
unsafe impl Sync for DisklistHead {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_disk_has_no_label_and_its_name_reads_back() {
        let dk = Disk::new();
        assert!(dk.label().is_none());
        assert_eq!(dk.name(), "");
        let mut name = [0u8; DS_DISKNAMELEN];
        name[..3].copy_from_slice(b"rd0");
        dk.dk_name.set(name);
        assert_eq!(dk.name(), "rd0");
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/disk.h");
        let ours: &[(&str, i64)] = &[
            ("DS_DISKNAMELEN", DS_DISKNAMELEN as i64),
            ("DKF_CONSTRUCTED", DKF_CONSTRUCTED.into()),
            ("DKF_OPENED", DKF_OPENED.into()),
            ("DKF_NOLABELREAD", DKF_NOLABELREAD.into()),
            ("DK_CLOSED", DK_CLOSED.into()),
            ("DK_OPENRAW", DK_OPENRAW.into()),
            ("DM_OPENPART", DM_OPENPART.into()),
            ("DM_OPENBLCK", DM_OPENBLCK.into()),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
