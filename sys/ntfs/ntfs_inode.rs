/*	$OpenBSD: ntfs_inode.h,v 1.8 2021/03/11 13:31:35 jsg Exp $	*/
/*	$NetBSD: ntfs_inode.h,v 1.1 2002/12/23 17:38:33 jdolecek Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1998, 1999 Semen Ustimenko
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
 *	Id: ntfs_inode.h,v 1.4 1999/05/12 09:43:00 semenu Exp
 */
/* </LICENSES> */

//! `<ntfs/ntfs_inode.h>`: the in-core MFT record (`struct ntnode`, one per file record, with
//! its attributes), the per-attribute node a vnode hangs from (`struct fnode`), and the file
//! handle (`struct ntfid`).
//!
//! Upstream: sys/ntfs/ntfs_inode.h @ 3ce1f3f79392
//!
//! An ntnode is `malloc(M_NTFSNTNODE)`ed by `ntfs_ntlookup` and freed by `ntfs_ntput` when its
//! use count drops to zero; an fnode is `malloc(M_NTFSFNODE)`ed by `ntfs_fget` and freed by
//! `ntfs_frele`. Both are handed around as `&'static`, their members `Cell`s, which the C
//! changes under the kernel lock and the ntnode lock `i_lock`.
//!
//! ## Deviations
//! - `i_next`/`i_prev` are left out: nothing in the C reads or writes them.
//! - `f_attrname` is [`AttrNameBuf`], the C's `char *` (a `malloc(M_TEMP)`ed NUL-terminated
//!   string, or NULL) with its length beside the pointer. `f_dirblbuf` keeps its allocation
//!   size beside it (`f_dirblbuf_len`).
//! - `struct ntfid` is [`Ntfid`], copied in and out of a `struct fid` member by member; the
//!   `notyet` `ntfid_gen` is left out, as GENERIC compiles it.

use core::cell::Cell;
use core::ptr::NonNull;

use crate::kern::subr_prf::panic;
use crate::ntfs::ntfs::{NtfsTimes, Ntfsino, Ntfsmount};
use crate::ntfs::ntfs_subr::Ntvattr;
use crate::queue_adapter;
use crate::sys::mount::Fid;
use crate::sys::queue::{ListEntry, ListHead, TailqEntry};
use crate::sys::rwlock::Rwlock;
use crate::sys::types::Dev;
use crate::sys::vnode::Vnode;

/// `IN_HASHED`: inode is on hash list.
pub const IN_HASHED: u32 = 0x0800;
/// `IN_LOADED`: ntvattrs loaded.
pub const IN_LOADED: u32 = 0x8000;
/// `IN_PRELOADED`: loaded from directory entry.
pub const IN_PRELOADED: u32 = 0x4000;

/// `struct ntnode`: an MFT file record in core.
pub struct Ntnode {
    /// `i_devvp`: vnode of blk dev we live on.
    pub i_devvp: Cell<Option<&'static Vnode>>,
    /// `i_dev`: device associated with the inode.
    pub i_dev: Cell<Dev>,
    /// `i_hash`: the link of the hash chain.
    pub i_hash: ListEntry<Ntnode>,
    /// `i_loaded`: the link of the mount's LRU of loaded ntnodes.
    pub i_loaded: TailqEntry<Ntnode>,
    /// `i_mp`.
    pub i_mp: Cell<Option<&'static Ntfsmount>>,
    /// `i_number`.
    pub i_number: Cell<Ntfsino>,
    /// `i_flag`: the `IN_*`.
    pub i_flag: Cell<u32>,
    /// `i_lock`.
    pub i_lock: Rwlock,
    /// `i_usecount`.
    pub i_usecount: Cell<i32>,
    /// `i_fnlist`: the fnodes of this ntnode.
    pub i_fnlist: ListHead<FnodeList>,
    /// `i_valist`: the attributes, while `IN_LOADED`.
    pub i_valist: ListHead<crate::ntfs::ntfs_subr::NtvattrList>,
    /// `i_nlink`: MFR.
    pub i_nlink: Cell<i64>,
    /// `i_mainrec`: MFR.
    pub i_mainrec: Cell<u64>,
    /// `i_frflag`: MFR.
    pub i_frflag: Cell<u32>,
}

impl Ntnode {
    /// A zeroed ntnode, as `malloc(M_WAITOK | M_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            i_devvp: Cell::new(None),
            i_dev: Cell::new(0),
            i_hash: ListEntry::new(),
            i_loaded: TailqEntry::new(),
            i_mp: Cell::new(None),
            i_number: Cell::new(0),
            i_flag: Cell::new(0),
            i_lock: Rwlock::new("ntnode"),
            i_usecount: Cell::new(0),
            i_fnlist: ListHead::new(),
            i_valist: ListHead::new(),
            i_nlink: Cell::new(0),
            i_mainrec: Cell::new(0),
            i_frflag: Cell::new(0),
        }
    }

    /// `ip->i_mp`, which `ntfs_ntlookup` sets.
    pub fn mp(&self) -> &'static Ntfsmount {
        match self.i_mp.get() {
            Some(m) => m,
            None => panic(format_args!("ntnode {:p}: no i_mp", self)),
        }
    }

    /// `ip->i_devvp`, which `ntfs_ntlookup` sets.
    pub fn devvp(&self) -> &'static Vnode {
        match self.i_devvp.get() {
            Some(vp) => vp,
            None => panic(format_args!("ntnode {:p}: no i_devvp", self)),
        }
    }

    /// `ip->i_flag |= f`.
    pub fn set_flag(&self, f: u32) {
        self.i_flag.set(self.i_flag.get() | f);
    }

    /// `ip->i_flag &= ~f`.
    pub fn clr_flag(&self, f: u32) {
        self.i_flag.set(self.i_flag.get() & !f);
    }
}

impl Default for Ntnode {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// The ntnode hash chains (`LIST_ENTRY(ntnode) i_hash`).
    pub NtnodeHash: Ntnode, i_hash => ListEntry<Ntnode>
);

queue_adapter!(
    /// The mount's LRU of loaded ntnodes (`TAILQ_ENTRY(ntnode) i_loaded`).
    pub NtnodeLoaded: Ntnode, i_loaded => TailqEntry<Ntnode>
);

/// `FN_PRELOADED`.
pub const FN_PRELOADED: u64 = 0x0001;
/// `FN_VALID`.
pub const FN_VALID: u64 = 0x0002;
/// `FN_AATTRNAME`: space allocated for `f_attrname`.
pub const FN_AATTRNAME: u64 = 0x0004;

/// An attribute name the C keeps as a `char *`: `len` bytes and a NUL, `malloc(M_TEMP)`ed
/// by `ntfs_ntlookupattr` (the module's deviations).
#[derive(Clone, Copy, Debug)]
pub struct AttrNameBuf {
    /// The allocation, `len + 1` bytes.
    pub p: NonNull<u8>,
    /// The name's length, without the NUL.
    pub len: usize,
}

impl AttrNameBuf {
    /// The name, without the NUL.
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: `p` is the `malloc`ed copy `ntfs_ntlookupattr` made, `len + 1` initialised
        // bytes, freed only by whoever owns the name (`ntfs_frele`, `ntfs_ntlookupfile`)
        // after its last use; nothing writes it meanwhile.
        unsafe { core::slice::from_raw_parts(self.p.as_ptr(), self.len) }
    }
}

/// `struct fnode`: one attribute of an ntnode, the object a vnode hangs from.
pub struct Fnode {
    /// `f_fnlist`: the link of the ntnode's fnode list.
    pub f_fnlist: ListEntry<Fnode>,
    /// `f_vp`: associated vnode.
    pub f_vp: Cell<Option<&'static Vnode>>,
    /// `f_ip`: associated ntnode.
    pub f_ip: Cell<Option<&'static Ntnode>>,
    /// `f_flag`: the `FN_*`.
    pub f_flag: Cell<u64>,
    /// `f_times`: $NAME/dirinfo.
    pub f_times: Cell<NtfsTimes>,
    /// `f_pnumber`: $NAME/dirinfo.
    pub f_pnumber: Cell<u32>,
    /// `f_fflag`: $NAME/dirinfo.
    pub f_fflag: Cell<u32>,
    /// `f_size`: defattr/dirinfo.
    pub f_size: Cell<u64>,
    /// `f_allocated`: defattr/dirinfo.
    pub f_allocated: Cell<u64>,
    /// `f_attrtype`.
    pub f_attrtype: Cell<u32>,
    /// `f_attrname` (the module's deviations).
    pub f_attrname: Cell<Option<AttrNameBuf>>,
    /// `f_lastdattr`: for `ntfs_ntreaddir`.
    pub f_lastdattr: Cell<u32>,
    /// `f_lastdblnum`.
    pub f_lastdblnum: Cell<u32>,
    /// `f_lastdoff`.
    pub f_lastdoff: Cell<u32>,
    /// `f_lastdnum`.
    pub f_lastdnum: Cell<u32>,
    /// `f_dirblbuf`: the directory block buffer of `ntfs_ntreaddir`.
    pub f_dirblbuf: Cell<Option<NonNull<u8>>>,
    /// The size of the `f_dirblbuf` allocation.
    pub f_dirblbuf_len: Cell<usize>,
    /// `f_dirblsz`.
    pub f_dirblsz: Cell<u32>,
}

impl Fnode {
    /// A zeroed fnode, as `malloc(M_WAITOK | M_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            f_fnlist: ListEntry::new(),
            f_vp: Cell::new(None),
            f_ip: Cell::new(None),
            f_flag: Cell::new(0),
            f_times: Cell::new(NtfsTimes {
                t_create: 0,
                t_write: 0,
                t_mftwrite: 0,
                t_access: 0,
            }),
            f_pnumber: Cell::new(0),
            f_fflag: Cell::new(0),
            f_size: Cell::new(0),
            f_allocated: Cell::new(0),
            f_attrtype: Cell::new(0),
            f_attrname: Cell::new(None),
            f_lastdattr: Cell::new(0),
            f_lastdblnum: Cell::new(0),
            f_lastdoff: Cell::new(0),
            f_lastdnum: Cell::new(0),
            f_dirblbuf: Cell::new(None),
            f_dirblbuf_len: Cell::new(0),
            f_dirblsz: Cell::new(0),
        }
    }

    /// The attribute's name (`f_attrname`, without the NUL), `None` for the unnamed one.
    pub fn attrname(&self) -> Option<&[u8]> {
        self.f_attrname.get().map(|n| {
            // SAFETY: as `AttrNameBuf::bytes`: the fnode owns the name until `ntfs_frele`.
            unsafe { core::slice::from_raw_parts(n.p.as_ptr(), n.len) }
        })
    }

    /// `f_dirblbuf` as a slice (empty when there is none).
    ///
    /// # Safety
    ///
    /// The caller holds the ntnode's lock (`ntfs_ntreaddir`) or is the only user of the
    /// fnode's directory buffer (`ntfs_readdir`, under the vnode), and no other slice of it
    /// is alive.
    #[allow(clippy::mut_from_ref)] // the buffer belongs to the fnode, as the C's pointer
    pub unsafe fn dirblbuf(&self) -> &mut [u8] {
        match self.f_dirblbuf.get() {
            // SAFETY: `f_dirblbuf` is a `malloc`ed allocation of `f_dirblbuf_len` bytes
            // (zeroed when made), freed only by `ntfs_readdir` or `ntfs_frele`, which clear
            // the pointer; the caller's contract makes the slice unique.
            Some(p) => unsafe {
                core::slice::from_raw_parts_mut(p.as_ptr(), self.f_dirblbuf_len.get())
            },
            None => &mut [],
        }
    }
}

impl Default for Fnode {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// The ntnode's fnodes (`LIST_ENTRY(fnode) f_fnlist`).
    pub FnodeList: Fnode, f_fnlist => ListEntry<Fnode>
);

/// `struct ntfid`: this overlays the fid structure (see `<sys/mount.h>`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ntfid {
    /// `ntfid_len`: length of structure.
    pub ntfid_len: u16,
    /// `ntfid_pad`: force 32-bit alignment.
    pub ntfid_pad: u16,
    /// `ntfid_ino`: file number (ino).
    pub ntfid_ino: Ntfsino,
    /// `ntfid_attr`: attribute identifier.
    pub ntfid_attr: u8,
}

impl Ntfid {
    /// `sizeof(struct ntfid)`: 4 + 4 + 1, padded to the `ntfsino_t`'s alignment.
    pub const SIZE: usize = 12;

    /// `(struct ntfid *)fhp`.
    pub fn from_fid(fid: &Fid) -> Self {
        let d = &fid.fid_data;
        Self {
            ntfid_len: fid.fid_len,
            ntfid_pad: fid.fid_reserved,
            ntfid_ino: u32::from_ne_bytes([d[0], d[1], d[2], d[3]]),
            ntfid_attr: d[4],
        }
    }

    /// Store the handle into `fid`, member by member (the rest of `fid` is left as it is).
    pub fn to_fid(&self, fid: &mut Fid) {
        fid.fid_len = self.ntfid_len;
        fid.fid_reserved = self.ntfid_pad;
        fid.fid_data[0..4].copy_from_slice(&self.ntfid_ino.to_ne_bytes());
        fid.fid_data[4] = self.ntfid_attr;
    }
}

/// The ntnode's attributes, as the C's `LIST_FOREACH(vap, &ip->i_valist, va_list)` walks
/// them.
pub fn valist(ip: &'static Ntnode) -> impl Iterator<Item = &'static Ntvattr> {
    ip.i_valist.iter()
}
