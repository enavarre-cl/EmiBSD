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
