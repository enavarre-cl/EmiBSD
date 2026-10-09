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
//! The UNIX file system: OpenBSD `sys/ufs/`.
//!
//! `ufs` is the layer the UFS-like file systems share (inodes, directories, the vnode
//! operations); `ffs` is the fast file system on it. Both are compiled with feature `ffs`
//! (`option FFS`), as `conf/files` builds them for `ffs | mfs`; `mfs` is the memory file
//! system on `ffs`, compiled with feature `mfs` (`option MFS`); `ext2fs` is the second extended
//! file system, compiled with feature `ext2fs` (`option EXT2FS`).

#[cfg(feature = "ext2fs")]
pub mod ext2fs;
pub mod ffs;
#[cfg(feature = "mfs")]
pub mod mfs;
#[allow(clippy::module_inception)] // OpenBSD's sys/ufs/ufs
pub mod ufs;
/* </CODE> */
