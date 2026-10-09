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
//! The memory file system: OpenBSD `sys/ufs/mfs/`, feature `mfs` (`option MFS`).
//!
//! Headers (types): `mfsnode`, `mfs_extern`. Files (functions): `mfs_vfsops`, `mfs_vnops`.
//! An MFS is an FFS (`ufs/ffs`) whose "disk" is a block of memory in the process that
//! mounted it (`mount_mfs(8)`), which stays in the kernel serving the file system's I/O.

pub mod mfs_extern;
pub mod mfs_vfsops;
pub mod mfs_vnops;
pub mod mfsnode;
/* </CODE> */
