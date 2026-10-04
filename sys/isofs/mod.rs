//! The CD-ROM and DVD file systems: OpenBSD `sys/isofs/`.
//!
//! `cd9660` is ISO 9660 with the Rock Ridge and Joliet extensions (feature `cd9660`,
//! `option CD9660`).

#[cfg(feature = "cd9660")]
pub mod cd9660;
