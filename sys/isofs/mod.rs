//! The CD-ROM and DVD file systems: OpenBSD `sys/isofs/`.
//!
//! `cd9660` is ISO 9660 with the Rock Ridge and Joliet extensions (feature `cd9660`,
//! `option CD9660`); `udf` is the Universal Disk Format of DVDs (ECMA-167 with the OSTA UDF
//! profile; feature `udf`, `option UDF`), read-only.

#[cfg(feature = "cd9660")]
pub mod cd9660;
#[cfg(feature = "udf")]
pub mod udf;
