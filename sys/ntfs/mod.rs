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
/* </LICENSES> */

/* <CODE> */
//! The NTFS file system: OpenBSD `sys/ntfs/` (`option NTFS`, feature `ntfs`), read-only.
//!
//! Headers (types): `ntfs` (`ntfs.h`, the on-disk structures and the mount), `ntfs_inode`
//! (`ntfs_inode.h`, the ntnode and the fnode), `ntfsmount` (`ntfsmount.h`, the mount flags).
//! Header and file pairs: `ntfs_compr` (LZNT1 decompression), `ntfs_ihash` (the ntnode
//! hash), `ntfs_subr` (the in-core attribute, ntnodes, run lists, attribute reads, directory
//! lookup and reading, fixups, the upper-case table), `ntfs_vfsops` (mount, `vget`). Files:
//! `ntfs_conv` (the UTF-8 name hooks), `ntfs_vnops` (the vnode operations).

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/ntfs/ntfs.h
pub mod ntfs;
pub mod ntfs_compr;
pub mod ntfs_conv;
pub mod ntfs_ihash;
pub mod ntfs_inode;
pub mod ntfs_subr;
pub mod ntfs_vfsops;
pub mod ntfs_vnops;
pub mod ntfsmount;
/* </CODE> */
