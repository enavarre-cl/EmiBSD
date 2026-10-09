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
//! The UDF file system: OpenBSD `sys/isofs/udf/` (`option UDF`, feature `udf`), read-only.
//!
//! Headers (types): `ecma167_udf` (`ecma167-udf.h`, the on-disk descriptors of ECMA-167 and
//! the UDF profile), `udf` (`udf.h`, the in-core node, the mount and the directory stream),
//! `udf_extern` (`udf_extern.h`, the prototypes, re-exported). Files (functions): `udf_subr`
//! (CS0 names, the disk label spoof, the virtual allocation table), `udf_vfsops` (mount,
//! partition maps, `vget`), `udf_vnops` (lookup, read, readdir, the block map).
//!
//! `ecma167-udf.h` is `ecma167_udf.rs`: a `-` cannot appear in a Rust module name.

pub mod ecma167_udf;
#[allow(clippy::module_inception)] // OpenBSD's layout: sys/isofs/udf/udf.h
pub mod udf;
pub mod udf_extern;
pub mod udf_subr;
pub mod udf_vfsops;
pub mod udf_vnops;
/* </CODE> */
