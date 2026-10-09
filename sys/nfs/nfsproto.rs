/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*
 * Copyright (c) 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Rick Macklem at The University of Guelph.
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
 *	@(#)nfsproto.h	8.2 (Berkeley) 3/30/95
 */
/* </LICENSES> */

/* <CODE> */
//! `<nfs/nfsproto.h>`: constants and wire structures of the Sun NFS version 2 and 3
//! protocols ("NFS: Network File System Protocol Specification", RFC 1094, and "NFS: Network
//! File System Version 3 Protocol Specification"): status numbers, `NFSX_*` sizes, procedure
//! numbers, the file handle, times, attributes and the file system information replies.
//!
//! Upstream: sys/nfs/nfsproto.h @ 3ce1f3f79392
//!
//! The wire structures are `#[repr(C)]` structures of raw XDR words (network byte order, as
//! they lie in the mbufs); `fxdr_unsigned`/`txdr_unsigned` (`xdr_subs.rs`) convert one word.
//! They are plain data ([`AbiPod`]), so `nfsm_dissect`'s view copies them out of a reply
//! ([`XdrIn::read`]) and `nfsm_build`'s view copies them in ([`XdrOut::write`]).
//!
//! [`XdrIn::read`]: crate::nfs::nfsm_subs::XdrIn::read
//! [`XdrOut::write`]: crate::nfs::nfsm_subs::XdrOut::write
//!
//! ## Deviations
//! - The unions of the attribute and statfs replies (`fa_un`, `sf_un`) are arrays of raw
//!   words; the C's accessor macros (`fa2_size`, `fa3_mtime`, `sf_tbytes`, ...) are methods
//!   of the same name, with `set_*` methods for the server's writes. `union nfsfh` is
//!   [`Nfsfh`], the bytes, with [`Nfsfh::fh_generic`]/[`Nfsfh::set_fh_generic`] as the
//!   `fhandle_t` member.
//! - The `NFSX_*(v3)` macros are `const fn`s of a `bool`; `NFSX_V3FH` and
//!   `NFSX_V3SRVSATTR` are the sizes of the Rust structures (checked against the C below).
//! - Procedure numbers (`NFSPROC_*`, `NFSV2PROC_*`) are `usize`: they index the
//!   `nfsv2_procid`/`nfsv3_procid` tables, `nfsstats.rpccnt` and the server's dispatch table.
//!   Status numbers (`NFS_OK`, `NFSERR_*`) are `i32`, the C's `int error` and `nd_repstat`
//!   that hold them; the flag bits `NFSERR_RETVOID`/`NFSERR_AUTHERR`/`NFSERR_RETERR` too.
//!   Other protocol values are `u32` words.
//! - The conversion macros (`vtonfsv2_mode`, `nfsv3tov_type`, ...) are functions over the
//!   tables of `nfs_subs.rs`, where the C defines them.

use crate::machine::copy::AbiPod;
use crate::nfs::nfs_subs::{NFSV2_TYPE, NFSV3_TYPE, NV2TOV_TYPE, NV3TOV_TYPE};
use crate::nfs::xdr_subs::{fxdr_unsigned, txdr_unsigned};
use crate::sys::mount::Fhandle;
use crate::sys::param::MAXBSIZE;
use crate::sys::types::Mode;
use crate::sys::vnode::{VCHR, VFIFO, Vtype, makeimode};

/// `NFS_PORT`.
pub const NFS_PORT: u16 = 2049;
/// `NFS_PROG`.
pub const NFS_PROG: u32 = 100003;
/// `NFS_VER2`.
pub const NFS_VER2: u32 = 2;
/// `NFS_VER3`.
pub const NFS_VER3: u32 = 3;
/// `NFS_VER4`.
pub const NFS_VER4: u32 = 4;
/// `NFS_V2MAXDATA`.
pub const NFS_V2MAXDATA: usize = 8192;
/// `NFS_MAXDGRAMDATA`.
pub const NFS_MAXDGRAMDATA: usize = 32768;
/// `NFS_MAXDATA`.
pub const NFS_MAXDATA: usize = MAXBSIZE;
/// `NFS_MAXPATHLEN`.
pub const NFS_MAXPATHLEN: usize = 1024;
/// `NFS_MAXNAMLEN`.
pub const NFS_MAXNAMLEN: usize = 255;
/// `NFS_MAXPKTHDR`.
pub const NFS_MAXPKTHDR: usize = 404;
/// `NFS_MAXPACKET`.
pub const NFS_MAXPACKET: usize = NFS_MAXPKTHDR + NFS_MAXDATA;
/// `NFS_MINPACKET`.
pub const NFS_MINPACKET: usize = 20;
/// `NFS_FABLKSIZE`: size in bytes of a block wrt `fa_blocks`.
pub const NFS_FABLKSIZE: u64 = 512;

/// `NFS_OK`: stat numbers for RPC returns (version 2 and 3).
pub const NFS_OK: i32 = 0;
/// `NFSERR_PERM`.
pub const NFSERR_PERM: i32 = 1;
/// `NFSERR_NOENT`.
pub const NFSERR_NOENT: i32 = 2;
/// `NFSERR_IO`.
pub const NFSERR_IO: i32 = 5;
/// `NFSERR_NXIO`.
pub const NFSERR_NXIO: i32 = 6;
/// `NFSERR_ACCES`.
pub const NFSERR_ACCES: i32 = 13;
/// `NFSERR_EXIST`.
pub const NFSERR_EXIST: i32 = 17;
/// `NFSERR_XDEV`: version 3 only.
pub const NFSERR_XDEV: i32 = 18;
/// `NFSERR_NODEV`.
pub const NFSERR_NODEV: i32 = 19;
/// `NFSERR_NOTDIR`.
pub const NFSERR_NOTDIR: i32 = 20;
/// `NFSERR_ISDIR`.
pub const NFSERR_ISDIR: i32 = 21;
/// `NFSERR_INVAL`: version 3 only.
pub const NFSERR_INVAL: i32 = 22;
/// `NFSERR_FBIG`.
pub const NFSERR_FBIG: i32 = 27;
/// `NFSERR_NOSPC`.
pub const NFSERR_NOSPC: i32 = 28;
/// `NFSERR_ROFS`.
pub const NFSERR_ROFS: i32 = 30;
/// `NFSERR_MLINK`: version 3 only.
pub const NFSERR_MLINK: i32 = 31;
/// `NFSERR_NAMETOL`.
pub const NFSERR_NAMETOL: i32 = 63;
/// `NFSERR_NOTEMPTY`.
pub const NFSERR_NOTEMPTY: i32 = 66;
/// `NFSERR_DQUOT`.
pub const NFSERR_DQUOT: i32 = 69;
/// `NFSERR_STALE`.
pub const NFSERR_STALE: i32 = 70;
/// `NFSERR_REMOTE`: version 3 only.
pub const NFSERR_REMOTE: i32 = 71;
/// `NFSERR_WFLUSH`: version 2 only.
pub const NFSERR_WFLUSH: i32 = 99;
/// `NFSERR_BADHANDLE`: the rest version 3 only.
pub const NFSERR_BADHANDLE: i32 = 10001;
/// `NFSERR_NOT_SYNC`.
pub const NFSERR_NOT_SYNC: i32 = 10002;
/// `NFSERR_BAD_COOKIE`.
pub const NFSERR_BAD_COOKIE: i32 = 10003;
/// `NFSERR_NOTSUPP`.
pub const NFSERR_NOTSUPP: i32 = 10004;
/// `NFSERR_TOOSMALL`.
pub const NFSERR_TOOSMALL: i32 = 10005;
/// `NFSERR_SERVERFAULT`.
pub const NFSERR_SERVERFAULT: i32 = 10006;
/// `NFSERR_BADTYPE`.
pub const NFSERR_BADTYPE: i32 = 10007;
/// `NFSERR_JUKEBOX`.
pub const NFSERR_JUKEBOX: i32 = 10008;
/// `NFSERR_TRYLATER`.
pub const NFSERR_TRYLATER: i32 = NFSERR_JUKEBOX;
/// `NFSERR_STALEWRITEVERF`: fake return for `nfs_commit()`.
pub const NFSERR_STALEWRITEVERF: i32 = 30001;

/// `NFSERR_RETVOID`: return void, not error.
pub const NFSERR_RETVOID: i32 = 0x2000_0000;
/// `NFSERR_AUTHERR`: mark an authentication error.
pub const NFSERR_AUTHERR: i32 = 0x4000_0000;
/// `NFSERR_RETERR`: mark an error return for V3.
pub const NFSERR_RETERR: i32 = 0x8000_0000_u32 as i32;

/// `NFSX_UNSIGNED`: sizes in bytes of various NFS RPC components.
pub const NFSX_UNSIGNED: usize = 4;

/// `NFSX_V2FH`: specific to NFS version 2.
pub const NFSX_V2FH: usize = 32;
/// `NFSX_V2FATTR`.
pub const NFSX_V2FATTR: usize = 68;
/// `NFSX_V2SATTR`.
pub const NFSX_V2SATTR: usize = 32;
/// `NFSX_V2COOKIE`.
pub const NFSX_V2COOKIE: usize = 4;
/// `NFSX_V2STATFS`.
pub const NFSX_V2STATFS: usize = 20;

/// `NFSX_V3FH`: size this server uses (`sizeof (fhandle_t)`).
pub const NFSX_V3FH: usize = size_of::<Fhandle>();
/// `NFSX_V3FHMAX`: max. allowed by protocol.
pub const NFSX_V3FHMAX: usize = 64;
/// `NFSX_V3FATTR`.
pub const NFSX_V3FATTR: usize = 84;
/// `NFSX_V3SATTR`: max. all fields filled in.
pub const NFSX_V3SATTR: usize = 60;
/// `NFSX_V3SRVSATTR`: `sizeof (struct nfsv3_sattr)`.
pub const NFSX_V3SRVSATTR: usize = size_of::<Nfsv3Sattr>();
/// `NFSX_V3POSTOPATTR`.
pub const NFSX_V3POSTOPATTR: usize = NFSX_V3FATTR + NFSX_UNSIGNED;
/// `NFSX_V3WCCDATA`.
pub const NFSX_V3WCCDATA: usize = NFSX_V3POSTOPATTR + 8 * NFSX_UNSIGNED;
/// `NFSX_V3COOKIEVERF`.
pub const NFSX_V3COOKIEVERF: usize = 8;
/// `NFSX_V3WRITEVERF`.
pub const NFSX_V3WRITEVERF: usize = 8;
/// `NFSX_V3CREATEVERF`.
pub const NFSX_V3CREATEVERF: usize = 8;
/// `NFSX_V3STATFS`.
pub const NFSX_V3STATFS: usize = 52;
/// `NFSX_V3FSINFO`.
pub const NFSX_V3FSINFO: usize = 48;
/// `NFSX_V3PATHCONF`.
pub const NFSX_V3PATHCONF: usize = 24;

/// `NFSX_FH(v3)`: variants for both versions.
pub const fn nfsx_fh(v3: bool) -> usize {
    if v3 {
        NFSX_V3FHMAX + NFSX_UNSIGNED
    } else {
        NFSX_V2FH
    }
}

/// `NFSX_SRVFH(v3)`.
pub const fn nfsx_srvfh(v3: bool) -> usize {
    if v3 { NFSX_V3FH } else { NFSX_V2FH }
}

/// `NFSX_FATTR(v3)`.
pub const fn nfsx_fattr(v3: bool) -> usize {
    if v3 { NFSX_V3FATTR } else { NFSX_V2FATTR }
}

/// `NFSX_PREOPATTR(v3)`.
pub const fn nfsx_preopattr(v3: bool) -> usize {
    if v3 { 7 * NFSX_UNSIGNED } else { 0 }
}

/// `NFSX_POSTOPATTR(v3)`.
pub const fn nfsx_postopattr(v3: bool) -> usize {
    if v3 { NFSX_V3FATTR + NFSX_UNSIGNED } else { 0 }
}

/// `NFSX_POSTOPORFATTR(v3)`.
pub const fn nfsx_postoporfattr(v3: bool) -> usize {
    if v3 {
        NFSX_V3FATTR + NFSX_UNSIGNED
    } else {
        NFSX_V2FATTR
    }
}

/// `NFSX_WCCDATA(v3)`.
pub const fn nfsx_wccdata(v3: bool) -> usize {
    if v3 { NFSX_V3WCCDATA } else { 0 }
}

/// `NFSX_WCCORFATTR(v3)`.
pub const fn nfsx_wccorfattr(v3: bool) -> usize {
    if v3 { NFSX_V3WCCDATA } else { NFSX_V2FATTR }
}

/// `NFSX_SATTR(v3)`.
pub const fn nfsx_sattr(v3: bool) -> usize {
    if v3 { NFSX_V3SATTR } else { NFSX_V2SATTR }
}

/// `NFSX_COOKIEVERF(v3)`.
pub const fn nfsx_cookieverf(v3: bool) -> usize {
    if v3 { NFSX_V3COOKIEVERF } else { 0 }
}

/// `NFSX_WRITEVERF(v3)`.
pub const fn nfsx_writeverf(v3: bool) -> usize {
    if v3 { NFSX_V3WRITEVERF } else { 0 }
}

/// `NFSX_READDIR(v3)`.
pub const fn nfsx_readdir(v3: bool) -> usize {
    if v3 {
        5 * NFSX_UNSIGNED
    } else {
        2 * NFSX_UNSIGNED
    }
}

/// `NFSX_STATFS(v3)`.
pub const fn nfsx_statfs(v3: bool) -> usize {
    if v3 { NFSX_V3STATFS } else { NFSX_V2STATFS }
}

/// `NFSPROC_NULL`: NFS RPC procedure numbers (before version mapping).
pub const NFSPROC_NULL: usize = 0;
/// `NFSPROC_GETATTR`.
pub const NFSPROC_GETATTR: usize = 1;
/// `NFSPROC_SETATTR`.
pub const NFSPROC_SETATTR: usize = 2;
/// `NFSPROC_LOOKUP`.
pub const NFSPROC_LOOKUP: usize = 3;
/// `NFSPROC_ACCESS`.
pub const NFSPROC_ACCESS: usize = 4;
/// `NFSPROC_READLINK`.
pub const NFSPROC_READLINK: usize = 5;
/// `NFSPROC_READ`.
pub const NFSPROC_READ: usize = 6;
/// `NFSPROC_WRITE`.
pub const NFSPROC_WRITE: usize = 7;
/// `NFSPROC_CREATE`.
pub const NFSPROC_CREATE: usize = 8;
/// `NFSPROC_MKDIR`.
pub const NFSPROC_MKDIR: usize = 9;
/// `NFSPROC_SYMLINK`.
pub const NFSPROC_SYMLINK: usize = 10;
/// `NFSPROC_MKNOD`.
pub const NFSPROC_MKNOD: usize = 11;
/// `NFSPROC_REMOVE`.
pub const NFSPROC_REMOVE: usize = 12;
/// `NFSPROC_RMDIR`.
pub const NFSPROC_RMDIR: usize = 13;
/// `NFSPROC_RENAME`.
pub const NFSPROC_RENAME: usize = 14;
/// `NFSPROC_LINK`.
pub const NFSPROC_LINK: usize = 15;
/// `NFSPROC_READDIR`.
pub const NFSPROC_READDIR: usize = 16;
/// `NFSPROC_READDIRPLUS`.
pub const NFSPROC_READDIRPLUS: usize = 17;
/// `NFSPROC_FSSTAT`.
pub const NFSPROC_FSSTAT: usize = 18;
/// `NFSPROC_FSINFO`.
pub const NFSPROC_FSINFO: usize = 19;
/// `NFSPROC_PATHCONF`.
pub const NFSPROC_PATHCONF: usize = 20;
/// `NFSPROC_COMMIT`.
pub const NFSPROC_COMMIT: usize = 21;
/// `NFSPROC_NOOP`.
pub const NFSPROC_NOOP: usize = 22;
/// `NFS_NPROCS`.
pub const NFS_NPROCS: usize = 23;

/// `NFSV2PROC_NULL`: actual version 2 procedure numbers.
pub const NFSV2PROC_NULL: usize = 0;
/// `NFSV2PROC_GETATTR`.
pub const NFSV2PROC_GETATTR: usize = 1;
/// `NFSV2PROC_SETATTR`.
pub const NFSV2PROC_SETATTR: usize = 2;
/// `NFSV2PROC_NOOP`.
pub const NFSV2PROC_NOOP: usize = 3;
/// `NFSV2PROC_ROOT`: obsolete.
pub const NFSV2PROC_ROOT: usize = NFSV2PROC_NOOP;
/// `NFSV2PROC_LOOKUP`.
pub const NFSV2PROC_LOOKUP: usize = 4;
/// `NFSV2PROC_READLINK`.
pub const NFSV2PROC_READLINK: usize = 5;
/// `NFSV2PROC_READ`.
pub const NFSV2PROC_READ: usize = 6;
/// `NFSV2PROC_WRITECACHE`: obsolete.
pub const NFSV2PROC_WRITECACHE: usize = NFSV2PROC_NOOP;
/// `NFSV2PROC_WRITE`.
pub const NFSV2PROC_WRITE: usize = 8;
/// `NFSV2PROC_CREATE`.
pub const NFSV2PROC_CREATE: usize = 9;
/// `NFSV2PROC_REMOVE`.
pub const NFSV2PROC_REMOVE: usize = 10;
/// `NFSV2PROC_RENAME`.
pub const NFSV2PROC_RENAME: usize = 11;
/// `NFSV2PROC_LINK`.
pub const NFSV2PROC_LINK: usize = 12;
/// `NFSV2PROC_SYMLINK`.
pub const NFSV2PROC_SYMLINK: usize = 13;
/// `NFSV2PROC_MKDIR`.
pub const NFSV2PROC_MKDIR: usize = 14;
/// `NFSV2PROC_RMDIR`.
pub const NFSV2PROC_RMDIR: usize = 15;
/// `NFSV2PROC_READDIR`.
pub const NFSV2PROC_READDIR: usize = 16;
/// `NFSV2PROC_STATFS`.
pub const NFSV2PROC_STATFS: usize = 17;

/// `NFSV3SATTRTIME_DONTCHANGE`: constants used by the version 3 protocol for various RPCs.
pub const NFSV3SATTRTIME_DONTCHANGE: u32 = 0;
/// `NFSV3SATTRTIME_TOSERVER`.
pub const NFSV3SATTRTIME_TOSERVER: u32 = 1;
/// `NFSV3SATTRTIME_TOCLIENT`.
pub const NFSV3SATTRTIME_TOCLIENT: u32 = 2;

/// `NFSV3ACCESS_READ`.
pub const NFSV3ACCESS_READ: u32 = 0x01;
/// `NFSV3ACCESS_LOOKUP`.
pub const NFSV3ACCESS_LOOKUP: u32 = 0x02;
/// `NFSV3ACCESS_MODIFY`.
pub const NFSV3ACCESS_MODIFY: u32 = 0x04;
/// `NFSV3ACCESS_EXTEND`.
pub const NFSV3ACCESS_EXTEND: u32 = 0x08;
/// `NFSV3ACCESS_DELETE`.
pub const NFSV3ACCESS_DELETE: u32 = 0x10;
/// `NFSV3ACCESS_EXECUTE`.
pub const NFSV3ACCESS_EXECUTE: u32 = 0x20;

/// `NFSV3WRITE_UNSTABLE`.
pub const NFSV3WRITE_UNSTABLE: u32 = 0;
/// `NFSV3WRITE_DATASYNC`.
pub const NFSV3WRITE_DATASYNC: u32 = 1;
/// `NFSV3WRITE_FILESYNC`.
pub const NFSV3WRITE_FILESYNC: u32 = 2;

/// `NFSV3CREATE_UNCHECKED`.
pub const NFSV3CREATE_UNCHECKED: u32 = 0;
/// `NFSV3CREATE_GUARDED`.
pub const NFSV3CREATE_GUARDED: u32 = 1;
/// `NFSV3CREATE_EXCLUSIVE`.
pub const NFSV3CREATE_EXCLUSIVE: u32 = 2;

/// `NFSV3FSINFO_LINK`.
pub const NFSV3FSINFO_LINK: u32 = 0x01;
/// `NFSV3FSINFO_SYMLINK`.
pub const NFSV3FSINFO_SYMLINK: u32 = 0x02;
/// `NFSV3FSINFO_HOMOGENEOUS`.
pub const NFSV3FSINFO_HOMOGENEOUS: u32 = 0x08;
/// `NFSV3FSINFO_CANSETTIME`.
pub const NFSV3FSINFO_CANSETTIME: u32 = 0x10;

/// `NFS_MAXFHSIZE`: the largest file handle (version 3).
pub const NFS_MAXFHSIZE: usize = 64;

/// `nfstype`: file types.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types, clippy::upper_case_acronyms)] // OpenBSD names, verbatim
pub enum Nfstype {
    /// `NFNON`.
    NFNON = 0,
    /// `NFREG`.
    NFREG = 1,
    /// `NFDIR`.
    NFDIR = 2,
    /// `NFBLK`.
    NFBLK = 3,
    /// `NFCHR`.
    NFCHR = 4,
    /// `NFLNK`.
    NFLNK = 5,
    /// `NFSOCK`.
    NFSOCK = 6,
    /// `NFFIFO`.
    NFFIFO = 7,
}

pub use Nfstype::{NFBLK, NFCHR, NFDIR, NFFIFO, NFLNK, NFNON, NFREG, NFSOCK};

/// `union nfsfh` (`nfsfh_t`): file handle (32 bytes for version 2), variable up to 64 for
/// version 3. The bytes are `fh_bytes`; the `fh_generic` member is the `fhandle_t` at their
/// start.
#[repr(C, align(4))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nfsfh {
    /// `fh_bytes`.
    pub fh_bytes: [u8; NFS_MAXFHSIZE],
}

impl Nfsfh {
    /// An all-zero handle.
    pub const fn new() -> Self {
        Self {
            fh_bytes: [0; NFS_MAXFHSIZE],
        }
    }

    /// `fh_generic`: the `fhandle_t` member (a copy).
    pub fn fh_generic(&self) -> Fhandle {
        crate::nfs::xdr_subs::xdr_get(&self.fh_bytes)
    }

    /// Stores `fh` as the `fh_generic` member.
    pub fn set_fh_generic(&mut self, fh: &Fhandle) {
        self.fh_bytes[..NFSX_V3FH].copy_from_slice(crate::nfs::xdr_subs::xdr_bytes(fh));
    }
}

impl Default for Nfsfh {
    fn default() -> Self {
        Self::new()
    }
}

/// Generates the two-word wire structures with their raw-word conversions.
macro_rules! xdr_pair {
    ($(#[$meta:meta])* $name:ident { $(#[$am:meta])* $a:ident, $(#[$bm:meta])* $b:ident }) => {
        $(#[$meta])*
        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name {
            $(#[$am])*
            pub $a: u32,
            $(#[$bm])*
            pub $b: u32,
        }

        impl $name {
            /// The structure made of two raw XDR words.
            pub const fn from_words(w: [u32; 2]) -> Self {
                Self { $a: w[0], $b: w[1] }
            }

            /// The two raw XDR words.
            pub const fn words(&self) -> [u32; 2] {
                [self.$a, self.$b]
            }
        }
    };
}

xdr_pair!(
    /// `struct nfsv2_time` (`nfstime2`).
    Nfsv2Time {
        /// `nfsv2_sec`.
        nfsv2_sec,
        /// `nfsv2_usec`.
        nfsv2_usec
    }
);

xdr_pair!(
    /// `struct nfsv3_time` (`nfstime3`).
    Nfsv3Time {
        /// `nfsv3_sec`.
        nfsv3_sec,
        /// `nfsv3_nsec`.
        nfsv3_nsec
    }
);

xdr_pair!(
    /// `struct nfsv3_spec` (`nfsv3spec`): NFS version 3 special file number.
    Nfsv3Spec {
        /// `specdata1`.
        specdata1,
        /// `specdata2`.
        specdata2
    }
);

/// `struct nfs_uquad` (`nfsuint64`): quads are defined as arrays of 2 longs to ensure dense
/// packing for the protocol and to facilitate XDR conversion.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nfsuint64 {
    /// `nfsuquad`.
    pub nfsuquad: [u32; 2],
}

impl Nfsuint64 {
    /// The structure made of two raw XDR words.
    pub const fn from_words(w: [u32; 2]) -> Self {
        Self { nfsuquad: w }
    }

    /// The two raw XDR words.
    pub const fn words(&self) -> [u32; 2] {
        self.nfsuquad
    }
}

/// Generates the accessors of a raw-word union member: `$get`/`$set` for word `$i` of
/// `$arr`, or for the pair of words at `$i` as a two-word structure `$t`.
macro_rules! un_word {
    ($arr:ident, $get:ident, $set:ident, $i:expr) => {
        #[doc = concat!("`", stringify!($get), "`: the raw XDR word.")]
        pub const fn $get(&self) -> u32 {
            self.$arr[$i]
        }

        #[doc = concat!("Stores the raw XDR word of `", stringify!($get), "`.")]
        pub const fn $set(&mut self, v: u32) {
            self.$arr[$i] = v;
        }
    };
    ($arr:ident, $get:ident, $set:ident, $i:expr, $t:ident) => {
        #[doc = concat!("`", stringify!($get), "`.")]
        pub const fn $get(&self) -> $t {
            $t::from_words([self.$arr[$i], self.$arr[$i + 1]])
        }

        #[doc = concat!("Stores `", stringify!($get), "`.")]
        pub const fn $set(&mut self, v: $t) {
            let w = v.words();
            self.$arr[$i] = w[0];
            self.$arr[$i + 1] = w[1];
        }
    };
}

/// `struct nfs_fattr`: file attributes, version 2 and 3. These go out on the wire and must be
/// densely packed: you can't take `size_of::<NfsFattr>()` for a reply, you must use
/// [`nfsx_fattr`]. `fa_un` holds the version's member (`fa_nfsv2` or `fa_nfsv3`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NfsFattr {
    /// `fa_type`.
    pub fa_type: u32,
    /// `fa_mode`.
    pub fa_mode: u32,
    /// `fa_nlink`.
    pub fa_nlink: u32,
    /// `fa_uid`.
    pub fa_uid: u32,
    /// `fa_gid`.
    pub fa_gid: u32,
    /// `fa_un`: the version 2 or version 3 member, as raw words.
    pub fa_un: [u32; 16],
}

impl NfsFattr {
    un_word!(fa_un, fa2_size, set_fa2_size, 0);
    un_word!(fa_un, fa2_blocksize, set_fa2_blocksize, 1);
    un_word!(fa_un, fa2_rdev, set_fa2_rdev, 2);
    un_word!(fa_un, fa2_blocks, set_fa2_blocks, 3);
    un_word!(fa_un, fa2_fsid, set_fa2_fsid, 4);
    un_word!(fa_un, fa2_fileid, set_fa2_fileid, 5);
    un_word!(fa_un, fa2_atime, set_fa2_atime, 6, Nfsv2Time);
    un_word!(fa_un, fa2_mtime, set_fa2_mtime, 8, Nfsv2Time);
    un_word!(fa_un, fa2_ctime, set_fa2_ctime, 10, Nfsv2Time);
    un_word!(fa_un, fa3_size, set_fa3_size, 0, Nfsuint64);
    un_word!(fa_un, fa3_used, set_fa3_used, 2, Nfsuint64);
    un_word!(fa_un, fa3_rdev, set_fa3_rdev, 4, Nfsv3Spec);
    un_word!(fa_un, fa3_fsid, set_fa3_fsid, 6, Nfsuint64);
    un_word!(fa_un, fa3_fileid, set_fa3_fileid, 8, Nfsuint64);
    un_word!(fa_un, fa3_atime, set_fa3_atime, 10, Nfsv3Time);
    un_word!(fa_un, fa3_mtime, set_fa3_mtime, 12, Nfsv3Time);
    un_word!(fa_un, fa3_ctime, set_fa3_ctime, 14, Nfsv3Time);
}

/// `struct nfsv2_sattr`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nfsv2Sattr {
    /// `sa_mode`.
    pub sa_mode: u32,
    /// `sa_uid`.
    pub sa_uid: u32,
    /// `sa_gid`.
    pub sa_gid: u32,
    /// `sa_size`.
    pub sa_size: u32,
    /// `sa_atime`.
    pub sa_atime: Nfsv2Time,
    /// `sa_mtime`.
    pub sa_mtime: Nfsv2Time,
}

/// `struct nfsv3_sattr`: NFS version 3 sattr structure for the new node creation case.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nfsv3Sattr {
    /// `sa_modetrue`.
    pub sa_modetrue: u32,
    /// `sa_mode`.
    pub sa_mode: u32,
    /// `sa_uidfalse`.
    pub sa_uidfalse: u32,
    /// `sa_gidfalse`.
    pub sa_gidfalse: u32,
    /// `sa_sizefalse`.
    pub sa_sizefalse: u32,
    /// `sa_atimetype`.
    pub sa_atimetype: u32,
    /// `sa_atime`.
    pub sa_atime: Nfsv3Time,
    /// `sa_mtimetype`.
    pub sa_mtimetype: u32,
    /// `sa_mtime`.
    pub sa_mtime: Nfsv3Time,
}

/// `struct nfs_statfs`: `sf_un` holds the version's member (`sf_nfsv2` or `sf_nfsv3`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NfsStatfs {
    /// `sf_un`: the version 2 or version 3 member, as raw words.
    pub sf_un: [u32; 13],
}

impl NfsStatfs {
    un_word!(sf_un, sf_tsize, set_sf_tsize, 0);
    un_word!(sf_un, sf_bsize, set_sf_bsize, 1);
    un_word!(sf_un, sf_blocks, set_sf_blocks, 2);
    un_word!(sf_un, sf_bfree, set_sf_bfree, 3);
    un_word!(sf_un, sf_bavail, set_sf_bavail, 4);
    un_word!(sf_un, sf_tbytes, set_sf_tbytes, 0, Nfsuint64);
    un_word!(sf_un, sf_fbytes, set_sf_fbytes, 2, Nfsuint64);
    un_word!(sf_un, sf_abytes, set_sf_abytes, 4, Nfsuint64);
    un_word!(sf_un, sf_tfiles, set_sf_tfiles, 6, Nfsuint64);
    un_word!(sf_un, sf_ffiles, set_sf_ffiles, 8, Nfsuint64);
    un_word!(sf_un, sf_afiles, set_sf_afiles, 10, Nfsuint64);
    un_word!(sf_un, sf_invarsec, set_sf_invarsec, 12);
}

/// `struct nfsv3_fsinfo`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nfsv3Fsinfo {
    /// `fs_rtmax`.
    pub fs_rtmax: u32,
    /// `fs_rtpref`.
    pub fs_rtpref: u32,
    /// `fs_rtmult`.
    pub fs_rtmult: u32,
    /// `fs_wtmax`.
    pub fs_wtmax: u32,
    /// `fs_wtpref`.
    pub fs_wtpref: u32,
    /// `fs_wtmult`.
    pub fs_wtmult: u32,
    /// `fs_dtpref`.
    pub fs_dtpref: u32,
    /// `fs_maxfilesize`.
    pub fs_maxfilesize: Nfsuint64,
    /// `fs_timedelta`.
    pub fs_timedelta: Nfsv3Time,
    /// `fs_properties`.
    pub fs_properties: u32,
}

/// `struct nfsv3_pathconf`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nfsv3Pathconf {
    /// `pc_linkmax`.
    pub pc_linkmax: u32,
    /// `pc_namemax`.
    pub pc_namemax: u32,
    /// `pc_notrunc`.
    pub pc_notrunc: u32,
    /// `pc_chownrestricted`.
    pub pc_chownrestricted: u32,
    /// `pc_caseinsensitive`.
    pub pc_caseinsensitive: u32,
    /// `pc_casepreserving`.
    pub pc_casepreserving: u32,
}

// SAFETY: 64 `u8`s with the alignment raised to 4, which adds no bytes (checked below): every
// byte is initialised and every bit pattern is a valid value.
unsafe impl AbiPod for Nfsfh {}
// SAFETY: this and the impls below are `#[repr(C)]` structures of `u32` words with no padding:
// every byte is initialised and every bit pattern is a valid value.
unsafe impl AbiPod for Nfsv2Time {}
// SAFETY: as above.
unsafe impl AbiPod for Nfsv3Time {}
// SAFETY: as above.
unsafe impl AbiPod for Nfsv3Spec {}
// SAFETY: as above.
unsafe impl AbiPod for Nfsuint64 {}
// SAFETY: as above.
unsafe impl AbiPod for NfsFattr {}
// SAFETY: as above.
unsafe impl AbiPod for Nfsv2Sattr {}
// SAFETY: as above.
unsafe impl AbiPod for Nfsv3Sattr {}
// SAFETY: as above.
unsafe impl AbiPod for NfsStatfs {}
// SAFETY: as above.
unsafe impl AbiPod for Nfsv3Fsinfo {}
// SAFETY: as above.
unsafe impl AbiPod for Nfsv3Pathconf {}

/// `vtonfsv2_mode(t, m)`.
pub fn vtonfsv2_mode(t: Vtype, m: Mode) -> u32 {
    txdr_unsigned(if t == VFIFO {
        makeimode(VCHR, m)
    } else {
        makeimode(t, m)
    })
}

/// `vtonfsv3_mode(m)`.
pub const fn vtonfsv3_mode(m: Mode) -> u32 {
    txdr_unsigned(m & 0o7777)
}

/// `nfstov_mode(a)`.
pub const fn nfstov_mode(a: u32) -> Mode {
    fxdr_unsigned(a) & 0o7777
}

/// `vtonfsv2_type(a)`.
pub const fn vtonfsv2_type(a: Vtype) -> u32 {
    txdr_unsigned(NFSV2_TYPE[a as usize] as u32)
}

/// `vtonfsv3_type(a)`.
pub const fn vtonfsv3_type(a: Vtype) -> u32 {
    txdr_unsigned(NFSV3_TYPE[a as usize] as u32)
}

/// `nfsv2tov_type(a)`.
pub const fn nfsv2tov_type(a: u32) -> Vtype {
    NV2TOV_TYPE[(fxdr_unsigned(a) & 0x7) as usize]
}

/// `nfsv3tov_type(a)`.
pub const fn nfsv3tov_type(a: u32) -> Vtype {
    NV3TOV_TYPE[(fxdr_unsigned(a) & 0x7) as usize]
}

const _: () = {
    assert!(size_of::<Nfsfh>() == NFS_MAXFHSIZE);
    assert!(NFSX_V3FH <= NFS_MAXFHSIZE);
    assert!(size_of::<Nfsv2Time>() == 8);
    assert!(size_of::<Nfsv3Time>() == 8);
    assert!(size_of::<Nfsuint64>() == 8);
    assert!(size_of::<Nfsv3Spec>() == 8);
    assert!(size_of::<NfsFattr>() == NFSX_V3FATTR);
    assert!(size_of::<Nfsv2Sattr>() == NFSX_V2SATTR);
    assert!(size_of::<Nfsv3Sattr>() == 44);
    assert!(size_of::<NfsStatfs>() == NFSX_V3STATFS);
    assert!(size_of::<Nfsv3Fsinfo>() == NFSX_V3FSINFO);
    assert!(size_of::<Nfsv3Pathconf>() == NFSX_V3PATHCONF);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `<nfs/nfsproto.h>`: the union accessors, the type conversions and, against
    // the C header, the constants.

    use std::string::String;

    use super::*;
    use crate::sys::vnode::{VBLK, VDIR, VLNK, VNON, VREG, VSOCK};

    #[test]
    fn union_accessors_name_the_c_words() {
        let mut fa = NfsFattr::default();
        fa.set_fa2_size(1);
        fa.set_fa2_ctime(Nfsv2Time::from_words([2, 3]));
        assert_eq!(fa.fa_un[0], 1);
        assert_eq!(fa.fa_un[10..12], [2, 3]);
        assert_eq!(fa.fa3_size(), Nfsuint64::from_words([1, 0]));
        fa.set_fa3_ctime(Nfsv3Time::from_words([7, 8]));
        assert_eq!(fa.fa_un[14..16], [7, 8]);
        assert_eq!(fa.fa3_ctime().nfsv3_nsec, 8);

        let mut sf = NfsStatfs::default();
        sf.set_sf_invarsec(9);
        sf.set_sf_afiles(Nfsuint64::from_words([4, 5]));
        assert_eq!(sf.sf_un[10..13], [4, 5, 9]);
    }

    #[test]
    fn file_handle_generic_member_is_the_first_bytes() {
        let mut fh = Nfsfh::new();
        let mut g = Fhandle::default();
        g.fh_fsid.val = [1, 2];
        g.fh_fid.fid_len = 12;
        g.fh_fid.fid_data[0] = 0xaa;
        fh.set_fh_generic(&g);
        assert_eq!(fh.fh_generic(), g);
        assert_eq!(fh.fh_bytes[0..4], 1i32.to_ne_bytes());
        assert!(fh.fh_bytes[NFSX_V3FH..].iter().all(|&b| b == 0));
    }

    #[test]
    fn type_and_mode_conversions() {
        assert_eq!(nfsv3tov_type(txdr_unsigned(NFFIFO as u32)), VFIFO);
        assert_eq!(nfsv2tov_type(txdr_unsigned(NFSOCK as u32)), VNON);
        assert_eq!(nfsv3tov_type(txdr_unsigned(NFSOCK as u32)), VSOCK);
        assert_eq!(nfsv3tov_type(txdr_unsigned(8 | NFDIR as u32)), VDIR);
        assert_eq!(fxdr_unsigned(vtonfsv3_type(VLNK)), NFLNK as u32);
        assert_eq!(fxdr_unsigned(vtonfsv2_type(VFIFO)), NFCHR as u32);
        assert_eq!(fxdr_unsigned(vtonfsv2_type(VREG)), NFREG as u32);
        assert_eq!(fxdr_unsigned(vtonfsv3_type(VBLK)), NFBLK as u32);
        assert_eq!(fxdr_unsigned(vtonfsv2_mode(VFIFO, 0o644)), 0o020644);
        assert_eq!(fxdr_unsigned(vtonfsv2_mode(VDIR, 0o755)), 0o040755);
        assert_eq!(fxdr_unsigned(vtonfsv3_mode(0o104755)), 0o4755);
        assert_eq!(nfstov_mode(txdr_unsigned(0o100644)), 0o644);
    }

    #[test]
    fn version_sizes() {
        assert_eq!(nfsx_fh(true), 68);
        assert_eq!(nfsx_fh(false), 32);
        assert_eq!(nfsx_srvfh(true), 28);
        assert_eq!(nfsx_postoporfattr(true), 88);
        assert_eq!(nfsx_wccdata(true), 120);
        assert_eq!(nfsx_readdir(false), 8);
        assert_eq!(NFSX_V3SRVSATTR, 44);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/nfs/nfsproto.h");
        let ours: &[(&str, i64)] = &[
            ("NFS_PORT", NFS_PORT.into()),
            ("NFS_PROG", NFS_PROG.into()),
            ("NFS_VER2", NFS_VER2.into()),
            ("NFS_VER3", NFS_VER3.into()),
            ("NFS_VER4", NFS_VER4.into()),
            ("NFS_V2MAXDATA", NFS_V2MAXDATA as i64),
            ("NFS_MAXDGRAMDATA", NFS_MAXDGRAMDATA as i64),
            ("NFS_MAXPATHLEN", NFS_MAXPATHLEN as i64),
            ("NFS_MAXNAMLEN", NFS_MAXNAMLEN as i64),
            ("NFS_MAXPKTHDR", NFS_MAXPKTHDR as i64),
            ("NFS_MINPACKET", NFS_MINPACKET as i64),
            ("NFS_FABLKSIZE", NFS_FABLKSIZE as i64),
            ("NFS_OK", NFS_OK.into()),
            ("NFSERR_PERM", NFSERR_PERM.into()),
            ("NFSERR_NOENT", NFSERR_NOENT.into()),
            ("NFSERR_IO", NFSERR_IO.into()),
            ("NFSERR_NXIO", NFSERR_NXIO.into()),
            ("NFSERR_ACCES", NFSERR_ACCES.into()),
            ("NFSERR_EXIST", NFSERR_EXIST.into()),
            ("NFSERR_XDEV", NFSERR_XDEV.into()),
            ("NFSERR_NODEV", NFSERR_NODEV.into()),
            ("NFSERR_NOTDIR", NFSERR_NOTDIR.into()),
            ("NFSERR_ISDIR", NFSERR_ISDIR.into()),
            ("NFSERR_INVAL", NFSERR_INVAL.into()),
            ("NFSERR_FBIG", NFSERR_FBIG.into()),
            ("NFSERR_NOSPC", NFSERR_NOSPC.into()),
            ("NFSERR_ROFS", NFSERR_ROFS.into()),
            ("NFSERR_MLINK", NFSERR_MLINK.into()),
            ("NFSERR_NAMETOL", NFSERR_NAMETOL.into()),
            ("NFSERR_NOTEMPTY", NFSERR_NOTEMPTY.into()),
            ("NFSERR_DQUOT", NFSERR_DQUOT.into()),
            ("NFSERR_STALE", NFSERR_STALE.into()),
            ("NFSERR_REMOTE", NFSERR_REMOTE.into()),
            ("NFSERR_WFLUSH", NFSERR_WFLUSH.into()),
            ("NFSERR_BADHANDLE", NFSERR_BADHANDLE.into()),
            ("NFSERR_NOT_SYNC", NFSERR_NOT_SYNC.into()),
            ("NFSERR_BAD_COOKIE", NFSERR_BAD_COOKIE.into()),
            ("NFSERR_NOTSUPP", NFSERR_NOTSUPP.into()),
            ("NFSERR_TOOSMALL", NFSERR_TOOSMALL.into()),
            ("NFSERR_SERVERFAULT", NFSERR_SERVERFAULT.into()),
            ("NFSERR_BADTYPE", NFSERR_BADTYPE.into()),
            ("NFSERR_JUKEBOX", NFSERR_JUKEBOX.into()),
            ("NFSERR_TRYLATER", NFSERR_TRYLATER.into()),
            ("NFSERR_STALEWRITEVERF", NFSERR_STALEWRITEVERF.into()),
            ("NFSERR_RETVOID", NFSERR_RETVOID.into()),
            ("NFSERR_AUTHERR", NFSERR_AUTHERR.into()),
            ("NFSERR_RETERR", i64::from(NFSERR_RETERR as u32)),
            ("NFSX_UNSIGNED", NFSX_UNSIGNED as i64),
            ("NFSX_V2FH", NFSX_V2FH as i64),
            ("NFSX_V2FATTR", NFSX_V2FATTR as i64),
            ("NFSX_V2SATTR", NFSX_V2SATTR as i64),
            ("NFSX_V2COOKIE", NFSX_V2COOKIE as i64),
            ("NFSX_V2STATFS", NFSX_V2STATFS as i64),
            ("NFSX_V3FHMAX", NFSX_V3FHMAX as i64),
            ("NFSX_V3FATTR", NFSX_V3FATTR as i64),
            ("NFSX_V3SATTR", NFSX_V3SATTR as i64),
            ("NFSX_V3POSTOPATTR", NFSX_V3POSTOPATTR as i64),
            ("NFSX_V3WCCDATA", NFSX_V3WCCDATA as i64),
            ("NFSX_V3COOKIEVERF", NFSX_V3COOKIEVERF as i64),
            ("NFSX_V3WRITEVERF", NFSX_V3WRITEVERF as i64),
            ("NFSX_V3CREATEVERF", NFSX_V3CREATEVERF as i64),
            ("NFSX_V3STATFS", NFSX_V3STATFS as i64),
            ("NFSX_V3FSINFO", NFSX_V3FSINFO as i64),
            ("NFSX_V3PATHCONF", NFSX_V3PATHCONF as i64),
            ("NFSPROC_NULL", NFSPROC_NULL as i64),
            ("NFSPROC_GETATTR", NFSPROC_GETATTR as i64),
            ("NFSPROC_SETATTR", NFSPROC_SETATTR as i64),
            ("NFSPROC_LOOKUP", NFSPROC_LOOKUP as i64),
            ("NFSPROC_ACCESS", NFSPROC_ACCESS as i64),
            ("NFSPROC_READLINK", NFSPROC_READLINK as i64),
            ("NFSPROC_READ", NFSPROC_READ as i64),
            ("NFSPROC_WRITE", NFSPROC_WRITE as i64),
            ("NFSPROC_CREATE", NFSPROC_CREATE as i64),
            ("NFSPROC_MKDIR", NFSPROC_MKDIR as i64),
            ("NFSPROC_SYMLINK", NFSPROC_SYMLINK as i64),
            ("NFSPROC_MKNOD", NFSPROC_MKNOD as i64),
            ("NFSPROC_REMOVE", NFSPROC_REMOVE as i64),
            ("NFSPROC_RMDIR", NFSPROC_RMDIR as i64),
            ("NFSPROC_RENAME", NFSPROC_RENAME as i64),
            ("NFSPROC_LINK", NFSPROC_LINK as i64),
            ("NFSPROC_READDIR", NFSPROC_READDIR as i64),
            ("NFSPROC_READDIRPLUS", NFSPROC_READDIRPLUS as i64),
            ("NFSPROC_FSSTAT", NFSPROC_FSSTAT as i64),
            ("NFSPROC_FSINFO", NFSPROC_FSINFO as i64),
            ("NFSPROC_PATHCONF", NFSPROC_PATHCONF as i64),
            ("NFSPROC_COMMIT", NFSPROC_COMMIT as i64),
            ("NFSPROC_NOOP", NFSPROC_NOOP as i64),
            ("NFS_NPROCS", NFS_NPROCS as i64),
            ("NFSV2PROC_NULL", NFSV2PROC_NULL as i64),
            ("NFSV2PROC_GETATTR", NFSV2PROC_GETATTR as i64),
            ("NFSV2PROC_SETATTR", NFSV2PROC_SETATTR as i64),
            ("NFSV2PROC_NOOP", NFSV2PROC_NOOP as i64),
            ("NFSV2PROC_ROOT", NFSV2PROC_ROOT as i64),
            ("NFSV2PROC_LOOKUP", NFSV2PROC_LOOKUP as i64),
            ("NFSV2PROC_READLINK", NFSV2PROC_READLINK as i64),
            ("NFSV2PROC_READ", NFSV2PROC_READ as i64),
            ("NFSV2PROC_WRITECACHE", NFSV2PROC_WRITECACHE as i64),
            ("NFSV2PROC_WRITE", NFSV2PROC_WRITE as i64),
            ("NFSV2PROC_CREATE", NFSV2PROC_CREATE as i64),
            ("NFSV2PROC_REMOVE", NFSV2PROC_REMOVE as i64),
            ("NFSV2PROC_RENAME", NFSV2PROC_RENAME as i64),
            ("NFSV2PROC_LINK", NFSV2PROC_LINK as i64),
            ("NFSV2PROC_SYMLINK", NFSV2PROC_SYMLINK as i64),
            ("NFSV2PROC_MKDIR", NFSV2PROC_MKDIR as i64),
            ("NFSV2PROC_RMDIR", NFSV2PROC_RMDIR as i64),
            ("NFSV2PROC_READDIR", NFSV2PROC_READDIR as i64),
            ("NFSV2PROC_STATFS", NFSV2PROC_STATFS as i64),
            (
                "NFSV3SATTRTIME_DONTCHANGE",
                NFSV3SATTRTIME_DONTCHANGE.into(),
            ),
            ("NFSV3SATTRTIME_TOSERVER", NFSV3SATTRTIME_TOSERVER.into()),
            ("NFSV3SATTRTIME_TOCLIENT", NFSV3SATTRTIME_TOCLIENT.into()),
            ("NFSV3ACCESS_READ", NFSV3ACCESS_READ.into()),
            ("NFSV3ACCESS_LOOKUP", NFSV3ACCESS_LOOKUP.into()),
            ("NFSV3ACCESS_MODIFY", NFSV3ACCESS_MODIFY.into()),
            ("NFSV3ACCESS_EXTEND", NFSV3ACCESS_EXTEND.into()),
            ("NFSV3ACCESS_DELETE", NFSV3ACCESS_DELETE.into()),
            ("NFSV3ACCESS_EXECUTE", NFSV3ACCESS_EXECUTE.into()),
            ("NFSV3WRITE_UNSTABLE", NFSV3WRITE_UNSTABLE.into()),
            ("NFSV3WRITE_DATASYNC", NFSV3WRITE_DATASYNC.into()),
            ("NFSV3WRITE_FILESYNC", NFSV3WRITE_FILESYNC.into()),
            ("NFSV3CREATE_UNCHECKED", NFSV3CREATE_UNCHECKED.into()),
            ("NFSV3CREATE_GUARDED", NFSV3CREATE_GUARDED.into()),
            ("NFSV3CREATE_EXCLUSIVE", NFSV3CREATE_EXCLUSIVE.into()),
            ("NFSV3FSINFO_LINK", NFSV3FSINFO_LINK.into()),
            ("NFSV3FSINFO_SYMLINK", NFSV3FSINFO_SYMLINK.into()),
            ("NFSV3FSINFO_HOMOGENEOUS", NFSV3FSINFO_HOMOGENEOUS.into()),
            ("NFSV3FSINFO_CANSETTIME", NFSV3FSINFO_CANSETTIME.into()),
            ("NFS_MAXFHSIZE", NFS_MAXFHSIZE as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
        // NFS_MAXDATA is MAXBSIZE, NFSX_V3FH and NFSX_V3SRVSATTR are sizeof expressions.
        assert_eq!(
            defs.get("NFS_MAXDATA").map(String::as_str),
            Some("MAXBSIZE")
        );
        assert_eq!(
            defs.get("NFSX_V3FH").map(String::as_str),
            Some("(sizeof (fhandle_t))")
        );
    }
}
/* </TESTS> */
