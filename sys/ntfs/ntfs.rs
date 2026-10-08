/*	$OpenBSD: ntfs.h,v 1.19 2022/01/11 03:13:59 jsg Exp $	*/
/*	$NetBSD: ntfs.h,v 1.5 2003/04/24 07:50:19 christos Exp $	*/
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
 *	Id: ntfs.h,v 1.5 1999/05/12 09:42:51 semenu Exp
 */
/* </LICENSES> */

/* <CODE> */
//! `<ntfs/ntfs.h>`: the on-disk structures of NTFS (the boot file, MFT file records and their
//! fixup header, attribute headers, `$FILE_NAME`, `$INDEX_ROOT`, `$INDEX_ALLOCATION` buffers
//! and their index entries, `$ATTRIBUTE_LIST` entries, `$AttrDef` records), the mounted
//! volume (`struct ntfsmount`) and the cluster arithmetic macros.
//!
//! Upstream: sys/ntfs/ntfs.h @ 3ce1f3f79392
//!
//! The volume is read in the machine's byte order, as the C does: NTFS is little-endian, and
//! so are both of our machines.
//!
//! ## Deviations
//! - The `__packed` on-disk structures are `#[repr(C, packed)]` with the C's members, marked
//!   with [`Packed`] (the UDF port's idiom, `docs/C_TO_RUST.md`): [`Packed::at`] is the C's
//!   cast when the structure fits in the bytes, [`Packed::read`] copies it out and reads zeros
//!   for the part past the end of the bytes (where the C reads past its buffer). Members are
//!   read by value.
//! - `struct attr`'s union `a_S` (`a_r`, `a_nr`) is its 48 bytes, [`Attr::a_s`]; [`Attr::a_r`]
//!   and [`Attr::a_nr`] read the two views out of them.
//! - The flexible tails `n_name[1]` (`struct attr_name`) and `al_name[1]`
//!   (`struct attr_attrlist`) are `[u16; 0]`: the names are read from the bytes past the
//!   fixed part ([`wchar_at`]). `struct attr_indexentry` keeps its `ie_fname[NTFS_MAXFILENAME]`
//!   (its entries are copied out with [`Packed::read`]).
//! - `struct ntfsmount`: the members are `Cell`s; `ntm_ad` is the `mallocarray`ed table with
//!   its count `ntm_adnum` beside it ([`Ntfsmount::ad`]); `ntm_export` (`struct netexport`)
//!   is kept whether or not `nfsserver` is configured, as msdosfs's `pm_export`; the three name hooks are function pointers whose shapes take slices
//!   ([`NtfsWgetFunc`], [`NtfsWputFunc`], [`NtfsWcmpFunc`]).
//! - `ntfs_cntobn`, `ntfs_cntob`, `ntfs_btocn`, `ntfs_btocl`, `ntfs_btocnoff` and `ntfs_bntob`
//!   read `ntmp` from the caller's scope in C; they are methods of [`Ntfsmount`], computing in
//!   64 bits (the C computes in the argument's type, which is 64 bits for every cluster number
//!   and offset; a 32-bit argument cannot reach the difference).
//! - `NTFS_NEXTREC(s, type)` is the offset arithmetic of its callers (`off + reclen`).
//! - `VFSTONTFS`, `VTONT`, `VTOF`, `FTOV` and `FTONT` are functions with the macros' names; the
//!   first three check what they cast.
//! - `NTFS_DEBUG` is off in GENERIC: `ntfs_debug` and `DNPRINTF`/`DPRINTF`/`DDPRINTF` are not
//!   ported (their calls are left out of the files).
//! - `ntfs_vops` is `NTFS_VOPS` in `ntfs_vnops.rs`.

use core::cell::Cell;
use core::ptr::NonNull;

use crate::kern::subr_prf::panic;
use crate::ntfs::ntfs_inode::{Fnode, Ntnode, NtnodeLoaded};
use crate::sys::mount::{Mount, Netexport};
use crate::sys::queue::TailqHead;
use crate::sys::types::{Daddr, Dev, Gid, Mode, Off, Uid};
use crate::sys::vnode::{VT_NTFS, Vnode};

/// `cn_t`: a cluster number.
pub type Cn = u64;
/// `wchar`: a UTF-16 code unit.
pub type Wchar = u16;

/// `BBSIZE`: the size of the boot block read at mount.
pub const BBSIZE: i32 = 1024;
/// `BBOFF`.
pub const BBOFF: Off = 0;
/// `BBLOCK`: the boot block's device block.
pub const BBLOCK: Daddr = 0;
/// `NTFS_MFTINO`: `$MFT`.
pub const NTFS_MFTINO: Ntfsino = 0;
/// `NTFS_VOLUMEINO`: `$Volume`.
pub const NTFS_VOLUMEINO: Ntfsino = 3;
/// `NTFS_ATTRDEFINO`: `$AttrDef`.
pub const NTFS_ATTRDEFINO: Ntfsino = 4;
/// `NTFS_ROOTINO`: the root directory.
pub const NTFS_ROOTINO: Ntfsino = 5;
/// `NTFS_BITMAPINO`: `$Bitmap`.
pub const NTFS_BITMAPINO: Ntfsino = 6;
/// `NTFS_BOOTINO`: `$Boot`.
pub const NTFS_BOOTINO: Ntfsino = 7;
/// `NTFS_BADCLUSINO`: `$BadClus`.
pub const NTFS_BADCLUSINO: Ntfsino = 8;
/// `NTFS_UPCASEINO`: `$UpCase`.
pub const NTFS_UPCASEINO: Ntfsino = 10;
/// `NTFS_MAXFILENAME`.
pub const NTFS_MAXFILENAME: usize = 255;

/// `ntfsino_t`: UFS directories use 32bit inode numbers internally, regardless of what the
/// system on top of it uses.
pub type Ntfsino = u32;

/// An on-disk structure that may be read in place from any byte offset.
///
/// # Safety
///
/// The implementor is a `#[repr(C, packed)]` structure whose members are integers, arrays of
/// them and other `Packed` types only: its alignment is 1 and every bit pattern is a valid
/// value.
pub unsafe trait Packed: Copy {
    /// `sizeof`.
    const SIZE: usize = size_of::<Self>();

    /// The structure at byte `off` of `buf` (the C's cast of `&buf[off]`), `None` when it
    /// does not fit in `buf`.
    fn at(buf: &[u8], off: usize) -> Option<&Self> {
        let end = off.checked_add(Self::SIZE)?;
        let bytes = buf.get(off..end)?;
        // SAFETY: the trait's contract: alignment 1 and no invalid bit patterns, so any
        // `SIZE` initialised bytes are a valid value; the reference borrows `buf`.
        Some(unsafe { &*bytes.as_ptr().cast::<Self>() })
    }

    /// A copy of the structure at byte `off` of `buf`; the bytes past the end of `buf` read
    /// as zeros (the C reads past its buffer).
    fn read(buf: &[u8], off: usize) -> Self {
        let mut b = [0u8; 1024];
        let n = Self::SIZE.min(b.len());
        if let Some(src) = buf.get(off..) {
            let k = src.len().min(n);
            b[..k].copy_from_slice(&src[..k]);
        }
        // SAFETY: `b` holds at least `SIZE` initialised bytes (every implementor is smaller
        // than 1024 bytes, asserted where it is marked), and the trait's contract makes any
        // bytes a valid value; the read is unaligned.
        unsafe { core::ptr::read_unaligned(b.as_ptr().cast::<Self>()) }
    }

    /// The structure's bytes.
    fn as_bytes(&self) -> &[u8] {
        // SAFETY: `self` is `SIZE` initialised bytes (alignment 1, no padding in a packed
        // structure of integers).
        unsafe { core::slice::from_raw_parts(core::ptr::from_ref(self).cast::<u8>(), Self::SIZE) }
    }
}

/// Marks each listed type [`Packed`] and checks at compile time that its alignment is 1 and
/// that [`Packed::read`]'s buffer holds it.
macro_rules! packed {
    ($($t:ty),* $(,)?) => {
        $(
            // SAFETY: `$t` is `#[repr(C, packed)]` and made of integers, arrays of them and
            // other `Packed` types (see its definition); the assertions below pin the
            // alignment and the size.
            unsafe impl Packed for $t {}
            const _: () = assert!(align_of::<$t>() == 1 && size_of::<$t>() <= 1024);
        )*
    };
}

/// The `i`th `wchar` of a name that starts at byte `off` of `buf` (0 past the end of `buf`).
pub fn wchar_at(buf: &[u8], off: usize, i: usize) -> Wchar {
    let o = off.saturating_add(i.saturating_mul(2));
    match buf.get(o..o.saturating_add(2)) {
        Some(b) => u16::from_ne_bytes([b[0], b[1]]),
        None => 0,
    }
}

/// `struct fixuphdr`: the update sequence header of a multi-sector record.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Fixuphdr {
    /// `fh_magic`.
    pub fh_magic: u32,
    /// `fh_foff`: offset of the update sequence array.
    pub fh_foff: u16,
    /// `fh_fnum`: its number of entries (the sequence number plus one per sector).
    pub fh_fnum: u16,
}

/// `NTFS_AF_INRUN`: the attribute is non-resident (its data is in runs of clusters).
pub const NTFS_AF_INRUN: u32 = 0x0000_0001;

/// `struct attrhdr`: the header every attribute record starts with.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Attrhdr {
    /// `a_type`: the `NTFS_A_*`, `0xffffffff` after the last attribute.
    pub a_type: u32,
    /// `reclen`.
    pub reclen: u32,
    /// `a_flag`: `NTFS_AF_INRUN`.
    pub a_flag: u8,
    /// `a_namelen`: in `wchar`s.
    pub a_namelen: u8,
    /// `a_nameoff`.
    pub a_nameoff: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `a_compression`.
    pub a_compression: u8,
    /// `reserved2`.
    pub reserved2: u8,
    /// `a_index`.
    pub a_index: u16,
}

/// `NTFS_A_STD`: `$STANDARD_INFORMATION`.
pub const NTFS_A_STD: u32 = 0x10;
/// `NTFS_A_ATTRLIST`: `$ATTRIBUTE_LIST`.
pub const NTFS_A_ATTRLIST: u32 = 0x20;
/// `NTFS_A_NAME`: `$FILE_NAME`.
pub const NTFS_A_NAME: u32 = 0x30;
/// `NTFS_A_VOLUMENAME`: `$VOLUME_NAME`.
pub const NTFS_A_VOLUMENAME: u32 = 0x60;
/// `NTFS_A_DATA`: `$DATA`.
pub const NTFS_A_DATA: u32 = 0x80;
/// `NTFS_A_INDXROOT`: `$INDEX_ROOT`.
pub const NTFS_A_INDXROOT: u32 = 0x90;
/// `NTFS_A_INDX`: `$INDEX_ALLOCATION`.
pub const NTFS_A_INDX: u32 = 0xA0;
/// `NTFS_A_INDXBITMAP`: `$BITMAP`.
pub const NTFS_A_INDXBITMAP: u32 = 0xB0;

/// `NTFS_MAXATTRNAME`.
pub const NTFS_MAXATTRNAME: usize = 255;

/// `a_S.a_S_r`: the rest of a resident attribute's header.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct AttrR {
    /// `a_datalen`.
    pub a_datalen: u16,
    /// `reserved1`.
    pub reserved1: u16,
    /// `a_dataoff`.
    pub a_dataoff: u16,
    /// `a_indexed`.
    pub a_indexed: u16,
}

/// `a_S.a_S_nr`: the rest of a non-resident attribute's header.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct AttrNr {
    /// `a_vcnstart`: the first virtual cluster this record maps.
    pub a_vcnstart: Cn,
    /// `a_vcnend`: the last one.
    pub a_vcnend: Cn,
    /// `a_dataoff`: the offset of the run list.
    pub a_dataoff: u16,
    /// `a_compressalg`.
    pub a_compressalg: u16,
    /// `reserved1`.
    pub reserved1: u32,
    /// `a_allocated`.
    pub a_allocated: u64,
    /// `a_datalen`.
    pub a_datalen: u64,
    /// `a_initialized`.
    pub a_initialized: u64,
}

/// `struct attr`: an attribute record of a file record.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Attr {
    /// `a_hdr`.
    pub a_hdr: Attrhdr,
    /// `a_S`: the union of [`AttrR`] and [`AttrNr`] (the module's deviations).
    pub a_s: [u8; 48],
}

impl Attr {
    /// `a_r`: the resident view of `a_S`.
    pub fn a_r(&self) -> AttrR {
        let s = self.a_s;
        AttrR::read(&s, 0)
    }

    /// `a_nr`: the non-resident view of `a_S`.
    pub fn a_nr(&self) -> AttrNr {
        let s = self.a_s;
        AttrNr::read(&s, 0)
    }
}

/// `ntfs_times_t`: the four times of a file, in 100 ns units since 1601.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NtfsTimes {
    /// `t_create`.
    pub t_create: u64,
    /// `t_write`.
    pub t_write: u64,
    /// `t_mftwrite`.
    pub t_mftwrite: u64,
    /// `t_access`.
    pub t_access: u64,
}

/// `NTFS_FFLAG_RDONLY`.
pub const NTFS_FFLAG_RDONLY: u64 = 0x01;
/// `NTFS_FFLAG_HIDDEN`.
pub const NTFS_FFLAG_HIDDEN: u64 = 0x02;
/// `NTFS_FFLAG_SYSTEM`.
pub const NTFS_FFLAG_SYSTEM: u64 = 0x04;
/// `NTFS_FFLAG_ARCHIVE`.
pub const NTFS_FFLAG_ARCHIVE: u64 = 0x20;
/// `NTFS_FFLAG_COMPRESSED`.
pub const NTFS_FFLAG_COMPRESSED: u64 = 0x0800;
/// `NTFS_FFLAG_DIR`.
pub const NTFS_FFLAG_DIR: u64 = 0x1000_0000;

/// `struct attr_name`: the value of a `$FILE_NAME` attribute.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct AttrName {
    /// `n_pnumber`: parent ntnode.
    pub n_pnumber: u32,
    /// `reserved`.
    pub reserved: u32,
    /// `n_times`.
    pub n_times: NtfsTimes,
    /// `n_size`.
    pub n_size: u64,
    /// `n_attrsz`.
    pub n_attrsz: u64,
    /// `n_flag`.
    pub n_flag: u64,
    /// `n_namelen`.
    pub n_namelen: u8,
    /// `n_nametype`.
    pub n_nametype: u8,
    /// `n_name[1]`: the name follows (the module's deviations).
    pub n_name: [u16; 0],
}

/// `NTFS_IRFLAG_INDXALLOC`: the index has an `$INDEX_ALLOCATION`.
pub const NTFS_IRFLAG_INDXALLOC: u16 = 0x0000_0001;

/// `struct attr_indexroot`: the value of an `$INDEX_ROOT` attribute, before its entries.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct AttrIndexroot {
    /// `ir_unkn1`: always 0x30.
    pub ir_unkn1: u32,
    /// `ir_unkn2`: always 0x1.
    pub ir_unkn2: u32,
    /// `ir_size`: the size of an index buffer.
    pub ir_size: u32,
    /// `ir_unkn3`: number of cluster.
    pub ir_unkn3: u32,
    /// `ir_unkn4`: always 0x10.
    pub ir_unkn4: u32,
    /// `ir_datalen`: sizeof something.
    pub ir_datalen: u32,
    /// `ir_allocated`: same as above.
    pub ir_allocated: u32,
    /// `ir_flag`: `NTFS_IRFLAG_INDXALLOC`.
    pub ir_flag: u16,
    /// `ir_unkn7`.
    pub ir_unkn7: u16,
}

/// `struct attr_attrlist`: an entry of an `$ATTRIBUTE_LIST`.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct AttrAttrlist {
    /// `al_type`: attribute type.
    pub al_type: u32,
    /// `reclen`: length of this entry.
    pub reclen: u16,
    /// `al_namelen`: attribute name len.
    pub al_namelen: u8,
    /// `al_nameoff`: name offset from entry start.
    pub al_nameoff: u8,
    /// `al_vcnstart`: VCN number.
    pub al_vcnstart: u64,
    /// `al_inumber`: parent ntnode.
    pub al_inumber: u32,
    /// `reserved`.
    pub reserved: u32,
    /// `al_index`: attribute index in MFT record.
    pub al_index: u16,
    /// `al_name[1]`: the name (the module's deviations).
    pub al_name: [u16; 0],
}

/// `NTFS_INDXMAGIC`: "INDX".
pub const NTFS_INDXMAGIC: u32 = 0x5844_4E49;

/// `struct attr_indexalloc`: the header of an `$INDEX_ALLOCATION` buffer.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct AttrIndexalloc {
    /// `ia_fixup`.
    pub ia_fixup: Fixuphdr,
    /// `unknown1`.
    pub unknown1: u64,
    /// `ia_bufcn`.
    pub ia_bufcn: Cn,
    /// `ia_hdrsize`: the entries start at `ia_hdrsize + 0x18`.
    pub ia_hdrsize: u16,
    /// `unknown2`.
    pub unknown2: u16,
    /// `ia_inuse`.
    pub ia_inuse: u32,
    /// `ia_allocated`.
    pub ia_allocated: u32,
}

/// `NTFS_IEFLAG_SUBNODE`: the entry has subnodes.
pub const NTFS_IEFLAG_SUBNODE: u32 = 0x0000_0001;
/// `NTFS_IEFLAG_LAST`: the last entry of its node.
pub const NTFS_IEFLAG_LAST: u32 = 0x0000_0002;

/// `struct attr_indexentry`: an entry of a directory index. A subnode's entry ends with the
/// `cn_t` of its buffer (`ie_bufcn`), at `reclen - sizeof(cn_t)`.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct AttrIndexentry {
    /// `ie_number`.
    pub ie_number: u32,
    /// `unknown1`.
    pub unknown1: u32,
    /// `reclen`.
    pub reclen: u16,
    /// `ie_size`.
    pub ie_size: u16,
    /// `ie_flag`: 1 - has subnodes, 2 - last.
    pub ie_flag: u32,
    /// `ie_fpnumber`.
    pub ie_fpnumber: u32,
    /// `unknown2`.
    pub unknown2: u32,
    /// `ie_ftimes`.
    pub ie_ftimes: NtfsTimes,
    /// `ie_fallocated`.
    pub ie_fallocated: u64,
    /// `ie_fsize`.
    pub ie_fsize: u64,
    /// `ie_fflag`.
    pub ie_fflag: u64,
    /// `ie_fnamelen`.
    pub ie_fnamelen: u8,
    /// `ie_fnametype`.
    pub ie_fnametype: u8,
    /// `ie_fname`.
    pub ie_fname: [Wchar; NTFS_MAXFILENAME],
}

/// `NTFS_FILEMAGIC`: "FILE".
pub const NTFS_FILEMAGIC: u32 = 0x454C_4946;
/// `NTFS_FRFLAG_DIR`.
pub const NTFS_FRFLAG_DIR: u32 = 0x0002;

/// `struct filerec`: the header of an MFT file record.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Filerec {
    /// `fr_fixup`.
    pub fr_fixup: Fixuphdr,
    /// `reserved`.
    pub reserved: [u8; 8],
    /// `fr_seqnum`: sequence number.
    pub fr_seqnum: u16,
    /// `fr_nlink`.
    pub fr_nlink: u16,
    /// `fr_attroff`: offset to attributes.
    pub fr_attroff: u16,
    /// `fr_flags`: 1-nonresident attr, 2-directory.
    pub fr_flags: u16,
    /// `fr_size`: hdr + attributes.
    pub fr_size: u32,
    /// `fr_allocated`: allocated length of record.
    pub fr_allocated: u32,
    /// `fr_mainrec`: main record.
    pub fr_mainrec: u64,
    /// `fr_attrnum`: maximum attr number + 1 ???
    pub fr_attrnum: u16,
}

/// `NTFS_ATTRNAME_MAXLEN`.
pub const NTFS_ATTRNAME_MAXLEN: usize = 0x40;
/// `NTFS_ADFLAG_NONRES`: attrib can be non resident.
pub const NTFS_ADFLAG_NONRES: u32 = 0x0080;
/// `NTFS_ADFLAG_INDEX`: attrib can be indexed.
pub const NTFS_ADFLAG_INDEX: u32 = 0x0002;

/// `struct attrdef`: a record of `$AttrDef`.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Attrdef {
    /// `ad_name`.
    pub ad_name: [Wchar; NTFS_ATTRNAME_MAXLEN],
    /// `ad_type`.
    pub ad_type: u32,
    /// `reserved1`.
    pub reserved1: [u32; 2],
    /// `ad_flag`.
    pub ad_flag: u32,
    /// `ad_minlen`.
    pub ad_minlen: u64,
    /// `ad_maxlen`: -1 for nonlimited.
    pub ad_maxlen: u64,
}

/// `struct ntvattrdef`: an attribute definition as the mount keeps it.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Ntvattrdef {
    /// `ad_name`.
    pub ad_name: [u8; 0x40],
    /// `ad_namelen`.
    pub ad_namelen: i32,
    /// `ad_type`.
    pub ad_type: u32,
}

/// `NTFS_BBID`.
pub const NTFS_BBID: &[u8; 8] = b"NTFS    ";
/// `NTFS_BBIDLEN`.
pub const NTFS_BBIDLEN: usize = 8;

/// `struct bootfile`: the boot sector's parameters.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Bootfile {
    /// `reserved1`: asm jmp near ...
    pub reserved1: [u8; 3],
    /// `bf_sysid`: 'NTFS    '.
    pub bf_sysid: [u8; 8],
    /// `bf_bps`: bytes per sector.
    pub bf_bps: u16,
    /// `bf_spc`: sectors per cluster.
    pub bf_spc: u8,
    /// `reserved2`: unused (zeroed).
    pub reserved2: [u8; 7],
    /// `bf_media`: media desc. (0xF8).
    pub bf_media: u8,
    /// `reserved3`.
    pub reserved3: [u8; 2],
    /// `bf_spt`: sectors per track.
    pub bf_spt: u16,
    /// `bf_heads`: number of heads.
    pub bf_heads: u16,
    /// `reserver4`.
    pub reserver4: [u8; 12],
    /// `bf_spv`: sectors per volume.
    pub bf_spv: u64,
    /// `bf_mftcn`: `$MFT` cluster number.
    pub bf_mftcn: Cn,
    /// `bf_mftmirrcn`: `$MFTMirr` cn.
    pub bf_mftmirrcn: Cn,
    /// `bf_mftrecsz`: MFT record size (clust); 0xF6 indicates 1/4.
    pub bf_mftrecsz: u8,
    /// `bf_ibsz`: index buffer size.
    pub bf_ibsz: u32,
    /// `bf_volsn`: volume ser. num.
    pub bf_volsn: u32,
}

packed!(
    Fixuphdr,
    Attrhdr,
    AttrR,
    AttrNr,
    Attr,
    NtfsTimes,
    AttrName,
    AttrIndexroot,
    AttrAttrlist,
    AttrIndexalloc,
    AttrIndexentry,
    Filerec,
    Attrdef,
    Ntvattrdef,
    Bootfile,
);

/// `ntfs_wget_func_t`: decode one character of `s` at `*pos` and advance `*pos` past it.
pub type NtfsWgetFunc = fn(s: &[u8], pos: &mut usize) -> Wchar;
/// `ntfs_wput_func_t`: encode `wc` into `s` (`n` is its length); the bytes written, 0 when
/// they do not fit.
pub type NtfsWputFunc = fn(s: &mut [u8], wc: Wchar) -> usize;
/// `ntfs_wcmp_func_t`: compare two wide characters.
pub type NtfsWcmpFunc = fn(Wchar, Wchar) -> i32;

/// `LOADED_NTNODE_HI`: maximum number of ntnodes to keep in memory. We do not want to leave
/// large data structures hanging off vnodes indefinitely and the data needed to reload the
/// ntnode should already be in the buffer cache.
pub const LOADED_NTNODE_HI: i32 = 16;

/// `TAILQ_HEAD(ntnodeq, ntnode)`.
pub type Ntnodeq = TailqHead<NtnodeLoaded>;

/// `NTFS_SYSNODESNUM`: the system files whose vnodes the mount may keep.
pub const NTFS_SYSNODESNUM: usize = 0x0B;

/// `struct ntfsmount`: a mounted NTFS volume.
pub struct Ntfsmount {
    /// `ntm_mountp`: filesystem vfs structure.
    pub ntm_mountp: Cell<Option<&'static Mount>>,
    /// `ntm_bootfile`.
    pub ntm_bootfile: Cell<Bootfile>,
    /// `ntm_dev`: device mounted.
    pub ntm_dev: Cell<Dev>,
    /// `ntm_devvp`: block device mounted vnode.
    pub ntm_devvp: Cell<Option<&'static Vnode>>,
    /// `ntm_sysvn`.
    pub ntm_sysvn: [Cell<Option<&'static Vnode>>; NTFS_SYSNODESNUM],
    /// `ntm_bpmftrec`: sectors per MFT record.
    pub ntm_bpmftrec: Cell<u32>,
    /// `ntm_uid`.
    pub ntm_uid: Cell<Uid>,
    /// `ntm_gid`.
    pub ntm_gid: Cell<Gid>,
    /// `ntm_mode`.
    pub ntm_mode: Cell<Mode>,
    /// `ntm_flag`: the `NTFS_MFLAG_*`.
    pub ntm_flag: Cell<u64>,
    /// `ntm_cfree`: free clusters.
    pub ntm_cfree: Cell<Cn>,
    /// `ntm_ad`: the attribute definitions (the module's deviations).
    pub ntm_ad: Cell<Option<NonNull<Ntvattrdef>>>,
    /// `ntm_adnum`.
    pub ntm_adnum: Cell<i32>,
    /// `ntm_wget`: decode string to Unicode string.
    pub ntm_wget: Cell<Option<NtfsWgetFunc>>,
    /// `ntm_wput`: encode Unicode string to string.
    pub ntm_wput: Cell<Option<NtfsWputFunc>>,
    /// `ntm_wcmp`: compare to wide characters.
    pub ntm_wcmp: Cell<Option<NtfsWcmpFunc>>,
    /// `ntm_ntnodes`: number of loaded ntnodes.
    pub ntm_ntnodes: Cell<i32>,
    /// `ntm_ntnodeq`: queue of ntnodes (LRU).
    pub ntm_ntnodeq: Ntnodeq,
    /// `ntm_export`: export information.
    pub ntm_export: Netexport,
}

impl Ntfsmount {
    /// A zeroed ntfsmount, as `malloc(M_WAITOK | M_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            ntm_mountp: Cell::new(None),
            ntm_bootfile: Cell::new(Bootfile {
                reserved1: [0; 3],
                bf_sysid: [0; 8],
                bf_bps: 0,
                bf_spc: 0,
                reserved2: [0; 7],
                bf_media: 0,
                reserved3: [0; 2],
                bf_spt: 0,
                bf_heads: 0,
                reserver4: [0; 12],
                bf_spv: 0,
                bf_mftcn: 0,
                bf_mftmirrcn: 0,
                bf_mftrecsz: 0,
                bf_ibsz: 0,
                bf_volsn: 0,
            }),
            ntm_dev: Cell::new(0),
            ntm_devvp: Cell::new(None),
            ntm_sysvn: [const { Cell::new(None) }; NTFS_SYSNODESNUM],
            ntm_bpmftrec: Cell::new(0),
            ntm_uid: Cell::new(0),
            ntm_gid: Cell::new(0),
            ntm_mode: Cell::new(0),
            ntm_flag: Cell::new(0),
            ntm_cfree: Cell::new(0),
            ntm_ad: Cell::new(None),
            ntm_adnum: Cell::new(0),
            ntm_wget: Cell::new(None),
            ntm_wput: Cell::new(None),
            ntm_wcmp: Cell::new(None),
            ntm_ntnodes: Cell::new(0),
            ntm_ntnodeq: TailqHead::new(),
            ntm_export: Netexport::new(),
        }
    }

    /// `ntm_mftcn` (`ntm_bootfile.bf_mftcn`).
    pub fn ntm_mftcn(&self) -> Cn {
        self.ntm_bootfile.get().bf_mftcn
    }

    /// `ntm_mftmirrcn` (`ntm_bootfile.bf_mftmirrcn`).
    pub fn ntm_mftmirrcn(&self) -> Cn {
        self.ntm_bootfile.get().bf_mftmirrcn
    }

    /// `ntm_mftrecsz` (`ntm_bootfile.bf_mftrecsz`).
    pub fn ntm_mftrecsz(&self) -> u8 {
        self.ntm_bootfile.get().bf_mftrecsz
    }

    /// `ntm_spc` (`ntm_bootfile.bf_spc`).
    pub fn ntm_spc(&self) -> u8 {
        self.ntm_bootfile.get().bf_spc
    }

    /// `ntm_bps` (`ntm_bootfile.bf_bps`).
    pub fn ntm_bps(&self) -> u16 {
        self.ntm_bootfile.get().bf_bps
    }

    /// `ntm_spc * ntm_bps`: the cluster size.
    fn clsize(&self) -> i64 {
        i64::from(self.ntm_spc()) * i64::from(self.ntm_bps())
    }

    /// `ntfs_cntobn(cn)`: the device block of cluster `cn`.
    pub fn ntfs_cntobn(&self, cn: Cn) -> Daddr {
        cn.wrapping_mul(u64::from(self.ntm_spc())) as Daddr
    }

    /// `ntfs_cntob(cn)`: the byte offset of cluster `cn`.
    pub fn ntfs_cntob(&self, cn: Cn) -> Off {
        (cn as Off).wrapping_mul(self.clsize())
    }

    /// `ntfs_btocn(off)`: the cluster of byte `off`. A volume with a zero cluster size
    /// cannot be mounted (`ntfs_mountfs` divides by it first).
    pub fn ntfs_btocn(&self, off: Off) -> Cn {
        off.checked_div(self.clsize()).unwrap_or(0) as Cn
    }

    /// `ntfs_btocl(off)`: the clusters `off` bytes take.
    pub fn ntfs_btocl(&self, off: Off) -> Cn {
        off.wrapping_add(self.ntfs_cntob(1) - 1)
            .checked_div(self.clsize())
            .unwrap_or(0) as Cn
    }

    /// `ntfs_btocnoff(off)`: the offset of byte `off` in its cluster.
    pub fn ntfs_btocnoff(&self, off: Off) -> Off {
        off.checked_rem(self.clsize()).unwrap_or(0)
    }

    /// `ntfs_bntob(bn)`: the bytes of `bn` sectors.
    pub fn ntfs_bntob(&self, bn: u32) -> i32 {
        bn.wrapping_mul(u32::from(self.ntm_bps())) as i32
    }

    /// `ntmp->ntm_mountp`, which `ntfs_mountfs` sets.
    pub fn mountp(&self) -> &'static Mount {
        match self.ntm_mountp.get() {
            Some(mp) => mp,
            None => panic(format_args!("ntfsmount {:p}: no ntm_mountp", self)),
        }
    }

    /// `ntmp->ntm_devvp`, which `ntfs_mountfs` sets.
    pub fn devvp(&self) -> &'static Vnode {
        match self.ntm_devvp.get() {
            Some(vp) => vp,
            None => panic(format_args!("ntfsmount {:p}: no ntm_devvp", self)),
        }
    }

    /// `ntmp->ntm_sysvn[i]`, which `ntfs_mountfs` sets for `$MFT`, the root and `$Bitmap`.
    pub fn sysvn(&self, i: Ntfsino) -> &'static Vnode {
        match self.ntm_sysvn.get(i as usize).and_then(Cell::get) {
            Some(vp) => vp,
            None => panic(format_args!("ntfsmount {:p}: no system vnode {}", self, i)),
        }
    }

    /// `(*ntmp->ntm_wget)(&s)`.
    pub fn wget(&self, s: &[u8], pos: &mut usize) -> Wchar {
        match self.ntm_wget.get() {
            Some(f) => f(s, pos),
            None => panic(format_args!("ntfsmount {:p}: no ntm_wget", self)),
        }
    }

    /// `(*ntmp->ntm_wput)(s, n, wc)`.
    pub fn wput(&self, s: &mut [u8], wc: Wchar) -> usize {
        match self.ntm_wput.get() {
            Some(f) => f(s, wc),
            None => panic(format_args!("ntfsmount {:p}: no ntm_wput", self)),
        }
    }

    /// `(*ntmp->ntm_wcmp)(a, b)`.
    pub fn wcmp(&self, a: Wchar, b: Wchar) -> i32 {
        match self.ntm_wcmp.get() {
            Some(f) => f(a, b),
            None => panic(format_args!("ntfsmount {:p}: no ntm_wcmp", self)),
        }
    }

    /// `ntm_ad[0 .. ntm_adnum]`: the attribute definitions (empty before `ntfs_mountfs` reads
    /// them).
    pub fn ad(&self) -> &[Ntvattrdef] {
        match self.ntm_ad.get() {
            // SAFETY: `ntm_ad` is the `mallocarray`ed table of `ntm_adnum` entries that
            // `ntfs_mountfs` filled before it set the pointer, freed only with the mount;
            // nothing writes it meanwhile. `Ntvattrdef` has alignment 1.
            Some(p) => unsafe {
                core::slice::from_raw_parts(
                    p.as_ptr(),
                    usize::try_from(self.ntm_adnum.get()).unwrap_or(0),
                )
            },
            None => &[],
        }
    }
}

impl Default for Ntfsmount {
    fn default() -> Self {
        Self::new()
    }
}

/// `VFSTONTFS(mp)`: convert mount ptr to ntfsmount ptr.
#[allow(non_snake_case)] // the C macro's name
pub fn VFSTONTFS(mp: &Mount) -> &'static Ntfsmount {
    let data = mp.mnt_data.get();
    if data.is_null() {
        panic(format_args!("VFSTONTFS: mount {:p} has no ntfsmount", mp));
    }
    // SAFETY: an NTFS mount's `mnt_data` is the ntfsmount `ntfs_mountfs` hung there, which
    // lives until `ntfs_unmount` (or the mount's error path) frees it and clears `mnt_data`.
    unsafe { &*data.cast::<Ntfsmount>() }
}

/// `VTOF(v)`: the fnode of an NTFS vnode.
#[allow(non_snake_case)] // the C macro's name
pub fn VTOF(vp: &Vnode) -> &'static Fnode {
    let data = vp.v_data.get();
    if data.is_null() || vp.v_tag.get() != VT_NTFS {
        panic(format_args!("VTOF: vnode {:p} has no fnode", vp));
    }
    // SAFETY: an NTFS vnode's `v_data` (checked above) is the fnode `ntfs_vgetex` hung
    // there, which lives until `ntfs_reclaim` releases it and clears `v_data`; the caller
    // holds the vnode, so it is not reclaimed meanwhile.
    unsafe { &*data.cast::<Fnode>() }
}

/// `VTONT(v)`: the ntnode of an NTFS vnode.
#[allow(non_snake_case)] // the C macro's name
pub fn VTONT(vp: &Vnode) -> &'static Ntnode {
    FTONT(VTOF(vp))
}

/// `FTOV(f)`: the vnode of an fnode.
#[allow(non_snake_case)] // the C macro's name
pub fn FTOV(fp: &Fnode) -> Option<&'static Vnode> {
    fp.f_vp.get()
}

/// `FTONT(f)`: the ntnode of an fnode.
#[allow(non_snake_case)] // the C macro's name
pub fn FTONT(fp: &Fnode) -> &'static Ntnode {
    match fp.f_ip.get() {
        Some(ip) => ip,
        None => panic(format_args!("FTONT: fnode {:p} has no ntnode", fp)),
    }
}

// The C layouts (amd64 and arm64 alike: `__packed`).
const _: () = {
    use core::mem::offset_of;
    assert!(size_of::<Fixuphdr>() == 8);
    assert!(size_of::<Attrhdr>() == 16);
    assert!(size_of::<AttrR>() == 8);
    assert!(size_of::<AttrNr>() == 48);
    assert!(size_of::<Attr>() == 64);
    assert!(size_of::<NtfsTimes>() == 32);
    assert!(offset_of!(AttrName, n_name) == 66);
    assert!(size_of::<AttrIndexroot>() == 32);
    assert!(offset_of!(AttrAttrlist, al_name) == 26);
    assert!(size_of::<AttrIndexalloc>() == 36);
    assert!(offset_of!(AttrIndexentry, ie_fname) == 82);
    assert!(size_of::<AttrIndexentry>() == 82 + 2 * NTFS_MAXFILENAME);
    assert!(size_of::<Filerec>() == 42);
    assert!(size_of::<Attrdef>() == 160);
    assert!(size_of::<Ntvattrdef>() == 72);
    assert!(offset_of!(Bootfile, bf_mftrecsz) == 64);
    assert!(size_of::<Bootfile>() == 73);
};
/* </CODE> */
