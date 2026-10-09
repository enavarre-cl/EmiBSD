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
 * Written by Pedro Martelletto <pedro@ambientworks.net> in February 2005.
 * Public domain.
 */
/* </LICENSES> */

/* <CODE> */
//! `<isofs/udf/udf_extern.h>`: the prototypes of the UDF functions and its memory pools,
//! for the other UDF files.
//!
//! Upstream: sys/isofs/udf/udf_extern.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header declares no types; its prototypes are the functions of `udf_subr.rs`,
//!   `udf_vfsops.rs` and `udf_vnops.rs`, and its `extern` pools and `udf_vops` their
//!   statics, re-exported here so that `use crate::isofs::udf::udf_extern::*` reads like the
//!   C's `#include`. The `VOP` functions take their argument structure (`void *v` in C).

pub use crate::isofs::udf::udf_subr::{udf_rawnametounicode, udf_vat_get, udf_vat_map};
pub use crate::isofs::udf::udf_vfsops::{
    UDF_DS_POOL, UDF_TRANS_POOL, UNODE_POOL, udf_checkexp, udf_fhtovp, udf_init, udf_mount,
    udf_quotactl, udf_root, udf_start, udf_statfs, udf_sync, udf_unmount, udf_vget, udf_vptofh,
};
pub use crate::isofs::udf::udf_vnops::{
    UDF_VOPS, udf_access, udf_bmap, udf_close, udf_getattr, udf_inactive, udf_ioctl, udf_islocked,
    udf_lock, udf_lookup, udf_open, udf_pathconf, udf_print, udf_read, udf_readatoffset,
    udf_readdir, udf_readlink, udf_reclaim, udf_strategy, udf_transname, udf_unlock,
};
/* </CODE> */
