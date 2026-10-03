//! Miscellaneous file systems: OpenBSD `sys/miscfs/`.
//!
//! `deadfs` holds the operations of revoked vnodes; `fifofs` and `fuse` are not ported.

pub mod deadfs;
