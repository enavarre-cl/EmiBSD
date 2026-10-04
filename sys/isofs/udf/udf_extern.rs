/*	$OpenBSD: udf_extern.h,v 1.16 2025/07/07 00:55:15 jsg Exp $	*/
/* <LICENSES> */
/*
 * Written by Pedro Martelletto <pedro@ambientworks.net> in February 2005.
 * Public domain.
 */
/* </LICENSES> */

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
