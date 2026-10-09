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
//! The ISO 9660 file system: OpenBSD `sys/isofs/cd9660/`.
//!
//! Headers (types): `iso` (the on-disc structures), `iso_rrip` (the Rock Ridge analysis),
//! `cd9660_extern` (the mount). Files (functions): `cd9660_bmap`, `cd9660_lookup`,
//! `cd9660_node` (with its header), `cd9660_rrip` (with its header), `cd9660_util`,
//! `cd9660_vfsops`, `cd9660_vnops`. `TODO.hibler` is notes, not code.

pub mod cd9660_bmap;
pub mod cd9660_extern;
pub mod cd9660_lookup;
pub mod cd9660_node;
pub mod cd9660_rrip;
pub mod cd9660_util;
pub mod cd9660_vfsops;
pub mod cd9660_vnops;
pub mod iso;
pub mod iso_rrip;
/* </CODE> */
