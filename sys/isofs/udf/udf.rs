/*	$OpenBSD: udf.h,v 1.21 2016/06/19 11:54:33 natano Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2001, 2002 Scott Long <scottl@freebsd.org>
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * $FreeBSD: src/sys/fs/udf/udf.h,v 1.9 2004/10/29 10:40:58 phk Exp $
 */
/* </LICENSES> */

/* <CODE> */
//! `<isofs/udf/udf.h>`: the in-core UDF node (`struct unode`), the mounted volume
//! (`struct umount`), the directory stream the lookups and `readdir` walk
//! (`struct udf_dirstream`), and the block reading helpers.
//!
//! Upstream: sys/isofs/udf/udf.h @ 3ce1f3f79392
//!
//! Ported to OpenBSD by Pedro Martelletto in February 2005.
//!
//! A unode is a `unode_pool` item, hung from its vnode's `v_data` by `udf_vget` and freed by
//! `udf_reclaim`; a umount is `malloc(M_UDFMOUNT)`ed by `udf_mountfs`, hung from
//! `mnt_data`, and freed by `udf_unmount`. Both are handed around as `&'static`, their
//! members `Cell`s, which the C changes under the kernel lock and the vnode lock.
//!
//! ## Deviations
//! - `u_fentry` is the `malloc(M_UDFFENTRY)`ed copy of the (extended) file entry as bytes,
//!   with its size beside it (`u_fentry_len`); [`Unode::fentry`] and [`Unode::fentry_fe`] are
//!   the C's `struct extfile_entry *` and its `(struct file_entry *)` cast.
//! - The union `un_u` of two `long`s (`u_diroff`, `u_vatlen`) is one `Cell<i64>` with an
//!   accessor pair per name.
//! - `um_hashtbl` is the slice `hashinit` returns (`None` before it); `um_hashsz` keeps the
//!   C's meaning, the table's size - 1 (a mask).
//! - `um_stbl` is the `malloc(M_UDFMOUNT)`ed copy of the sparing table as bytes, with its
//!   size beside it (`um_stbl_size`).
//! - `struct udf_dirstream`'s `data` pointer is [`UdfData`], where the chunk starts: in the
//!   file entry (no buffer) or in `bp`'s data; `buf` is the `malloc(M_UDFFID)`ed fragment
//!   buffer, `um_bsize` bytes; `error` is a `Result`, `fid_fragment` a `bool`.
//! - `RDSECTOR(devvp, sector, size, bp)`, which reads `ump` from the caller's scope, is
//!   [`rdsector`] with `ump` as an argument; it and `udf_readlblks` return `bread`'s pair.
//! - The prototypes (`udf_allocv`, `udf_hashlookup`, `udf_hashins`, `udf_hashrem`,
//!   `udf_checktag`) are the functions of `udf_vnops.rs` and `udf_vfsops.rs`.

use core::cell::Cell;
use core::ptr::NonNull;

use crate::crypto::siphash::SiphashKey;
use crate::isofs::udf::ecma167_udf::{ExtfileEntry, FileEntry, LbAddr, LongAd, LongAdImpl, Packed};
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::bread;
use crate::machine::intr::IPL_NONE;
use crate::queue_adapter;
use crate::sys::buf::Buf;
use crate::sys::endian::letoh32;
use crate::sys::errno::Errno;
use crate::sys::mount::Mount;
use crate::sys::mutex::Mutex;
use crate::sys::param::DEV_BSIZE;
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::rwlock::Rrwlock;
use crate::sys::types::{Daddr, Dev};
use crate::sys::vnode::{VT_UDF, Vnode};

/// `UDF_HASHTBLSIZE`.
pub const UDF_HASHTBLSIZE: i32 = 100;

/// `udfino_t`.
pub type Udfino = u32;

/// `struct unode`.
pub struct Unode {
    /// `u_le`: the link of the mount's hash chain.
    pub u_le: ListEntry<Unode>,
    /// `u_vnode`.
    pub u_vnode: Cell<Option<&'static Vnode>>,
    /// `u_devvp`.
    pub u_devvp: Cell<Option<&'static Vnode>>,
    /// `u_ump`.
    pub u_ump: Cell<Option<&'static Umount>>,
    /// `u_lock`: the vnode lock.
    pub u_lock: Rrwlock,
    /// `u_dev`.
    pub u_dev: Cell<Dev>,
    /// `u_ino`.
    pub u_ino: Cell<Udfino>,
    /// `un_u`: `u_diroff` or `u_vatlen` (the module's deviations).
    pub un_u: Cell<i64>,
    /// `u_fentry`: the file entry (the module's deviations).
    pub u_fentry: Cell<Option<NonNull<u8>>>,
    /// The size of the `u_fentry` allocation.
    pub u_fentry_len: Cell<usize>,
}

impl Unode {
    /// A zeroed unode, as `pool_get(&unode_pool, PR_WAITOK | PR_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            u_le: ListEntry::new(),
            u_vnode: Cell::new(None),
            u_devvp: Cell::new(None),
            u_ump: Cell::new(None),
            u_lock: Rrwlock::new("unode"),
            u_dev: Cell::new(0),
            u_ino: Cell::new(0),
            un_u: Cell::new(0),
            u_fentry: Cell::new(None),
            u_fentry_len: Cell::new(0),
        }
    }

    /// `u_diroff`: where the last lookup in this directory found its name.
    pub fn u_diroff(&self) -> i64 {
        self.un_u.get()
    }

    /// `u_diroff = off`.
    pub fn set_u_diroff(&self, off: i64) {
        self.un_u.set(off);
    }

    /// `u_vatlen`: the number of entries of the VAT this node is.
    pub fn u_vatlen(&self) -> i64 {
        self.un_u.get()
    }

    /// `u_vatlen = len`.
    pub fn set_u_vatlen(&self, len: i64) {
        self.un_u.set(len);
    }

    /// `up->u_ump`, which `udf_vget` sets.
    pub fn ump(&self) -> &'static Umount {
        match self.u_ump.get() {
            Some(ump) => ump,
            None => panic(format_args!("unode {:p}: no u_ump", self)),
        }
    }

    /// `up->u_devvp`, which `udf_vget` sets.
    pub fn devvp(&self) -> &'static Vnode {
        match self.u_devvp.get() {
            Some(vp) => vp,
            None => panic(format_args!("unode {:p}: no u_devvp", self)),
        }
    }

    /// The bytes of `u_fentry`: the file entry, its extended attributes and allocation
    /// descriptors, then zeros (empty before `udf_vget` sets it).
    pub fn fentry_bytes(&self) -> &[u8] {
        match self.u_fentry.get() {
            // SAFETY: `u_fentry` is a `malloc`ed, zeroed (so initialised) allocation of
            // `u_fentry_len` bytes, set once by `udf_vget` (or `udf_vat_get`) and freed only
            // when the node is (`udf_reclaim`, `udf_unmount`); nothing writes it meanwhile, so
            // shared slices of it may coexist.
            Some(p) => unsafe { core::slice::from_raw_parts(p.as_ptr(), self.u_fentry_len.get()) },
            None => &[],
        }
    }

    /// `up->u_fentry` (`struct extfile_entry *`).
    pub fn fentry(&self) -> &ExtfileEntry {
        match ExtfileEntry::at(self.fentry_bytes(), 0) {
            Some(fe) => fe,
            None => panic(format_args!("unode {:p}: no file entry", self)),
        }
    }

    /// `(struct file_entry *)up->u_fentry`.
    pub fn fentry_fe(&self) -> &FileEntry {
        match FileEntry::at(self.fentry_bytes(), 0) {
            Some(fe) => fe,
            None => panic(format_args!("unode {:p}: no file entry", self)),
        }
    }
}

impl Default for Unode {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// The mount's unode hash chains (`LIST_ENTRY(unode) u_le`).
    pub UnodeHash: Unode, u_le => ListEntry<Unode>
);

/// `LIST_HEAD(udf_hash_lh, unode)`.
pub type UdfHashLh = ListHead<UnodeHash>;

/// `struct umount`.
pub struct Umount {
    /// `um_flags`: the `UDF_MNT_*`.
    pub um_flags: Cell<i32>,
    /// `um_mountp`.
    pub um_mountp: Cell<Option<&'static Mount>>,
    /// `um_devvp`.
    pub um_devvp: Cell<Option<&'static Vnode>>,
    /// `um_dev`.
    pub um_dev: Cell<Dev>,
    /// `um_bsize`: the logical block size.
    pub um_bsize: Cell<i32>,
    /// `um_bshift`.
    pub um_bshift: Cell<i32>,
    /// `um_bmask`.
    pub um_bmask: Cell<i32>,
    /// `um_start`: the partition's first sector.
    pub um_start: Cell<u32>,
    /// `um_realstart`.
    pub um_realstart: Cell<u32>,
    /// `um_len`.
    pub um_len: Cell<u32>,
    /// `um_reallen`.
    pub um_reallen: Cell<u32>,
    /// `um_meta_start`.
    pub um_meta_start: Cell<u32>,
    /// `um_meta_len`.
    pub um_meta_len: Cell<u32>,
    /// `um_vat`: the node of the virtual allocation table.
    pub um_vat: Cell<Option<&'static Unode>>,
    /// `um_root_icb`.
    pub um_root_icb: Cell<LongAd>,
    /// `um_hashtbl`.
    pub um_hashtbl: Cell<Option<&'static [UdfHashLh]>>,
    /// `um_hashkey`.
    pub um_hashkey: Cell<SiphashKey>,
    /// `um_hashsz`: the hash table's size - 1.
    pub um_hashsz: Cell<u64>,
    /// `um_hashmtx`.
    pub um_hashmtx: Mutex,
    /// `um_psecs`: sectors per packet.
    pub um_psecs: Cell<i32>,
    /// `um_stbl_len`.
    pub um_stbl_len: Cell<i32>,
    /// `um_stbl`: the sparing table (the module's deviations).
    pub um_stbl: Cell<Option<NonNull<u8>>>,
    /// The size of the `um_stbl` allocation.
    pub um_stbl_size: Cell<usize>,
}

impl Umount {
    /// A zeroed umount, as `malloc(M_WAITOK | M_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            um_flags: Cell::new(0),
            um_mountp: Cell::new(None),
            um_devvp: Cell::new(None),
            um_dev: Cell::new(0),
            um_bsize: Cell::new(0),
            um_bshift: Cell::new(0),
            um_bmask: Cell::new(0),
            um_start: Cell::new(0),
            um_realstart: Cell::new(0),
            um_len: Cell::new(0),
            um_reallen: Cell::new(0),
            um_meta_start: Cell::new(0),
            um_meta_len: Cell::new(0),
            um_vat: Cell::new(None),
            um_root_icb: Cell::new(LongAd {
                len: 0,
                loc: LbAddr {
                    lb_num: 0,
                    part_num: 0,
                },
                impl_: LongAdImpl { bytes: [0; 6] },
            }),
            um_hashtbl: Cell::new(None),
            um_hashkey: Cell::new(SiphashKey { k0: 0, k1: 0 }),
            um_hashsz: Cell::new(0),
            um_hashmtx: Mutex::new(IPL_NONE),
            um_psecs: Cell::new(0),
            um_stbl_len: Cell::new(0),
            um_stbl: Cell::new(None),
            um_stbl_size: Cell::new(0),
        }
    }

    /// `ump->um_mountp`, which `udf_mountfs` sets.
    pub fn mountp(&self) -> &'static Mount {
        match self.um_mountp.get() {
            Some(mp) => mp,
            None => panic(format_args!("umount {:p}: no um_mountp", self)),
        }
    }

    /// `ump->um_devvp`, which `udf_mountfs` sets.
    pub fn devvp(&self) -> &'static Vnode {
        match self.um_devvp.get() {
            Some(vp) => vp,
            None => panic(format_args!("umount {:p}: no um_devvp", self)),
        }
    }

    /// The bytes of the sparing table `um_stbl` (empty when there is none).
    pub fn stbl_bytes(&self) -> &[u8] {
        match self.um_stbl.get() {
            // SAFETY: `um_stbl` is a `malloc`ed allocation of `um_stbl_size` bytes, filled
            // once by `udf_get_spartmap` (zeroed first) and freed only by `udf_unmount` or the
            // mount's error path; nothing writes it meanwhile.
            Some(p) => unsafe { core::slice::from_raw_parts(p.as_ptr(), self.um_stbl_size.get()) },
            None => &[],
        }
    }
}

impl Default for Umount {
    fn default() -> Self {
        Self::new()
    }
}

/// `UDF_MNT_FIND_VAT`: indicates a VAT must be found.
pub const UDF_MNT_FIND_VAT: i32 = 0x01;
/// `UDF_MNT_USES_VAT`: indicates a VAT must be used.
pub const UDF_MNT_USES_VAT: i32 = 0x02;
/// `UDF_MNT_USES_META`: indicates we are using a Metadata partition.
pub const UDF_MNT_USES_META: i32 = 0x04;

/// `VTOU(vp)`: the unode of a UDF vnode.
#[allow(non_snake_case)] // the C macro's name
pub fn VTOU(vp: &Vnode) -> &'static Unode {
    let data = vp.v_data.get();
    if data.is_null() || vp.v_tag.get() != VT_UDF {
        panic(format_args!("VTOU: vnode {:p} has no unode", vp));
    }
    // SAFETY: a UDF vnode's `v_data` (checked above) is the unode `udf_vget` hung there,
    // which lives until `udf_reclaim` gives it back and clears `v_data`; the caller holds the
    // vnode (a reference or its lock), so it is not reclaimed meanwhile.
    unsafe { &*data.cast::<Unode>() }
}

/// Where the data `udf_readatoffset` found starts (its `*data`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UdfData {
    /// At this offset of the node's `u_fentry`: the file data recorded in the allocation
    /// descriptor area (no buffer was read).
    Fentry(usize),
    /// At this offset of the data of the buffer read.
    Buf(usize),
}

/// `struct udf_dirstream`: a walk over the file identifier descriptors of a directory.
pub struct UdfDirstream {
    /// `node`: the directory.
    pub node: &'static Unode,
    /// `ump`.
    pub ump: &'static Umount,
    /// `bp`: the buffer of the current chunk.
    pub bp: Option<&'static Buf>,
    /// `data`: where the current chunk starts (the module's deviations).
    pub data: UdfData,
    /// `buf`: the fragment buffer, `um_bsize` bytes.
    pub buf: Option<NonNull<u8>>,
    /// `fsize`: the size of the directory.
    pub fsize: i32,
    /// `off`: the offset of the next descriptor in the current chunk.
    pub off: i32,
    /// `this_off`: the directory offset after the descriptor returned last.
    pub this_off: i32,
    /// `offset`: the directory offset of the current chunk.
    pub offset: i32,
    /// `size`: the size of the current chunk.
    pub size: i32,
    /// `error`.
    pub error: Result<(), Errno>,
    /// `fid_fragment`: the descriptor returned last is in `buf`.
    pub fid_fragment: bool,
}

/// `VFSTOUDFFS(mp)`: the umount of a UDF mount.
#[allow(non_snake_case)] // the C macro's name
pub fn VFSTOUDFFS(mp: &Mount) -> &'static Umount {
    let data = mp.mnt_data.get();
    if data.is_null() {
        panic(format_args!("VFSTOUDFFS: mount {:p} has no umount", mp));
    }
    // SAFETY: a UDF mount's `mnt_data` is the umount `udf_mountfs` hung there, which lives
    // until `udf_unmount` frees it and clears `mnt_data`.
    unsafe { &*data.cast::<Umount>() }
}

/// `RDSECTOR(devvp, sector, size, bp)`: read `size` bytes at logical block `sector` of the
/// device. The block layer refers to things in terms of 512 byte blocks by default.
pub fn rdsector(
    ump: &Umount,
    devvp: &'static Vnode,
    sector: Daddr,
    size: i32,
) -> (&'static Buf, Result<(), Errno>) {
    bread(
        devvp,
        (sector << ump.um_bshift.get()) / DEV_BSIZE as Daddr,
        size,
    )
}

/// `udf_readlblks(ump, sector, size, bp)`: read the logical blocks holding `size` bytes from
/// `sector` on.
pub fn udf_readlblks(ump: &Umount, sector: i32, size: i32) -> (&'static Buf, Result<(), Errno>) {
    let bmask = ump.um_bmask.get();
    rdsector(
        ump,
        ump.devvp(),
        Daddr::from(sector),
        size.wrapping_add(bmask) & !bmask,
    )
}

/// `udf_getid(icb)`: produce a suitable file number from an ICB. The passed in ICB is
/// expected to be in little endian. If the fileno resolves to 0, we might be in big trouble.
/// Assumes the ICB is a `long_ad`. This struct is compatible with `short_ad`, but not
/// `ext_ad`.
pub fn udf_getid(icb: &LongAd) -> Udfino {
    letoh32(icb.loc.lb_num)
}

/// `unicode_t`.
pub type Unicode = u16;
/* </CODE> */
